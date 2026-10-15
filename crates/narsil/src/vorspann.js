// Release 09.10.2026: DER VORSPANN — Web-APIs, die der Motor (narsil-jsvm)
// noch nicht nativ kennt, in JavaScript auf seinen Grundfunktionen gebaut.
// Laeuft VOR den Seitenskripten (`js.rs`, nach `anbinden_mit_stand`).
// Jede Ergaenzung prueft zuerst, ob es die API schon gibt.
//
// GEMESSENER ANLASS: youtube.com laedt `webcomponents-sd.js` (Polyfill fuer
// Custom Elements und Shadow DOM) und brach mit `'NodeFilter' ist nicht
// definiert` ab — danach blieb die Seite leer.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;

  // ---- NodeFilter (DOM §6.3) -------------------------------------------
  if (typeof g.NodeFilter === 'undefined') {
    g.NodeFilter = {
      FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3,
      SHOW_ALL: 0xFFFFFFFF, SHOW_ELEMENT: 0x1, SHOW_ATTRIBUTE: 0x2,
      SHOW_TEXT: 0x4, SHOW_CDATA_SECTION: 0x8, SHOW_ENTITY_REFERENCE: 0x10,
      SHOW_ENTITY: 0x20, SHOW_PROCESSING_INSTRUCTION: 0x40, SHOW_COMMENT: 0x80,
      SHOW_DOCUMENT: 0x100, SHOW_DOCUMENT_TYPE: 0x200,
      SHOW_DOCUMENT_FRAGMENT: 0x400, SHOW_NOTATION: 0x800
    };
  }

  // ---- Traversal (DOM §6): NodeIterator, TreeWalker, NodeFilter ---------
  // Release 2026-10-14: rewritten after the spec text — the "filter"
  // algorithm with the active flag and exceptions, ToUint32 for whatToShow,
  // read-only attributes on the prototypes, "Illegal constructor",
  // [object NodeIterator]/[object TreeWalker], and the NodeIterator
  // pre-removing steps (applied right after a removal, from the mutation
  // journal record with parent, previous and next sibling).
  // MEASURED before (WPT dom/traversal 831/1608 subtests): NodeIterator.html
  // 66/766 ("[object Object]"), NodeIterator-removal 0/25, TreeWalker.html
  // 726/761 (exceptions from the filter did not propagate).
  var traversalState = new WeakMap();
  var traversalKey = {};
  function illegal() { throw new TypeError('Illegal constructor'); }
  function travState(o, what) {
    var s = traversalState.get(o);
    if (!s) throw new TypeError("Failed to execute '" + what + "': Illegal invocation");
    return s;
  }
  function isNode(n) { return !!n && typeof n === 'object' && typeof n.nodeType === 'number'; }
  // DOM §6 "filter a node".
  function filterNode(s, n) {
    if (s.active) throw new g.DOMException('The traverser is already active.', 'InvalidStateError');
    var bit = 1 << (n.nodeType - 1);
    if (!(s.whatToShow & bit)) return 3;
    var f = s.filter;
    if (f === null) return 1;
    s.active = true;
    var r;
    try {
      if (typeof f === 'function') r = f.call(undefined, n);
      else {
        var m = f.acceptNode;
        if (typeof m !== 'function') throw new TypeError("Failed to execute 'acceptNode' on 'NodeFilter': The provided callback is not callable.");
        r = m.call(f, n);
      }
    } finally {
      s.active = false;
    }
    r = Number(r);
    return isFinite(r) ? ((Math.trunc ? Math.trunc(r) : (r < 0 ? Math.ceil(r) : Math.floor(r))) % 65536 + 65536) % 65536 : 0;
  }
  function makeState(root, whatToShow, filter, what) {
    if (!isNode(root)) throw new TypeError("Failed to execute '" + what + "' on 'Document': parameter 1 is not of type 'Node'.");
    var w = whatToShow === undefined ? 0xFFFFFFFF : Number(whatToShow) >>> 0;
    if (whatToShow !== undefined && !isFinite(Number(whatToShow))) w = 0;
    var f = filter === undefined || filter === null ? null : filter;
    if (f !== null && typeof f !== 'object' && typeof f !== 'function') throw new TypeError("Failed to execute '" + what + "' on 'Document': The provided value is not of type 'NodeFilter'.");
    return { root: root, whatToShow: w, filter: f, active: false };
  }
  function accessor(P, name, get, set) {
    try { Object.defineProperty(P, name, { get: get, set: set, configurable: true, enumerable: true }); } catch (e) {}
  }
  function method(P, name, f) {
    try { Object.defineProperty(P, name, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {}
  }
  function tag(P, name) {
    try { if (typeof Symbol === 'function' && Symbol.toStringTag) Object.defineProperty(P, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
  }

  // NodeIterator (DOM §6.1).
  var NI = function NodeIterator() { illegal(); };
  var liveIterators = [];
  function followingWithin(n, root) {
    if (n.firstChild) return n.firstChild;
    for (var t = n; t && t !== root; t = t.parentNode) {
      if (t.nextSibling) return t.nextSibling;
    }
    return null;
  }
  function precedingWithin(n, root) {
    if (n === root) return null;
    var p = n.previousSibling;
    if (p) { while (p.lastChild) p = p.lastChild; return p; }
    return n.parentNode;
  }
  // The candidate in flight is part of the iterator state, so a removal
  // during filtering retargets it like the reference; the filtered node is
  // still what the call returns.
  function iterate(it, next) {
    var s = travState(it, next ? 'nextNode' : 'previousNode');
    var flight = { ref: s.reference, before: s.pointerBefore };
    try {
      for (var guard = 0; guard < 50000000; guard++) {
        var node = flight.ref;
        if (next) {
          if (!flight.before) { node = followingWithin(node, s.root); if (!node) return null; } else flight.before = false;
        } else {
          if (flight.before) { node = precedingWithin(node, s.root); if (!node) return null; } else flight.before = true;
        }
        flight.ref = node;
        s.inflight = flight;
        if (filterNode(s, node) === 1) {
          s.reference = flight.ref;
          s.pointerBefore = flight.before;
          return node;
        }
      }
      return null;
    } finally {
      s.inflight = null;
    }
  }
  method(NI.prototype, 'nextNode', function nextNode() { return iterate(this, true); });
  method(NI.prototype, 'previousNode', function previousNode() { return iterate(this, false); });
  method(NI.prototype, 'detach', function detach() {});
  accessor(NI.prototype, 'root', function () { return travState(this, 'root').root; });
  accessor(NI.prototype, 'referenceNode', function () { return travState(this, 'referenceNode').reference; });
  accessor(NI.prototype, 'pointerBeforeReferenceNode', function () { return travState(this, 'pointerBeforeReferenceNode').pointerBefore; });
  accessor(NI.prototype, 'whatToShow', function () { return travState(this, 'whatToShow').whatToShow; });
  accessor(NI.prototype, 'filter', function () { return travState(this, 'filter').filter; });
  tag(NI.prototype, 'NodeIterator');
  // The "NodeIterator pre-removing steps", applied after the removal from
  // the journal record: parent, removed nodes, previous and next sibling.
  function containsInclusive(a, b) {
    for (var n = b, t = 0; n && t < 100000; n = n.parentNode, t++) if (n === a) return true;
    return false;
  }
  // `holder` is {ref, before}: the iterator's reference or the candidate
  // in flight. Removing an inclusive ancestor of the root changes nothing.
  function retarget(root, holder, parent, removed, previous, next) {
    for (var j = 0; j < removed.length; j++) {
      var r = removed[j];
      if (containsInclusive(r, root) || !containsInclusive(r, holder.ref)) continue;
      if (holder.before) {
        // The first node following the removed range inside the root.
        var cand = next;
        if (!cand) {
          for (var t = parent; t && !cand; t = t.parentNode) {
            if (t === root) break;
            cand = t.nextSibling;
          }
        }
        if (cand && containsInclusive(root, cand)) { holder.ref = cand; return; }
        holder.before = false;
      }
      if (previous) { var p = previous; while (p.lastChild) p = p.lastChild; holder.ref = p; }
      else holder.ref = parent;
      return;
    }
  }
  function afterRemoval(parent, removed, previous, next) {
    for (var i = 0; i < liveIterators.length; i++) {
      var s = liveIterators[i];
      var h = { ref: s.reference, before: s.pointerBefore };
      retarget(s.root, h, parent, removed, previous, next);
      s.reference = h.ref;
      s.pointerBefore = h.before;
      if (s.inflight) retarget(s.root, s.inflight, parent, removed, previous, next);
    }
  }
  try { Object.defineProperty(g, '__narsilIteratorRemoval', { value: afterRemoval, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  // The mutation journal also records removals for live iterators: the
  // observer switch is shared (observers OR iterators).
  var nativeJournalSwitch = g.__narsilMoAn, observersWantJournal = false, iteratorsWantJournal = false;
  if (typeof nativeJournalSwitch === 'function') {
    try {
      Object.defineProperty(g, '__narsilMoAn', {
        value: function (on) { observersWantJournal = !!on; return nativeJournalSwitch(observersWantJournal || iteratorsWantJournal); },
        configurable: true, writable: true, enumerable: false
      });
    } catch (e) {}
  }
  function newIterator(root, whatToShow, filter) {
    var s = makeState(root, whatToShow, filter, 'createNodeIterator');
    s.reference = root;
    s.pointerBefore = true;
    var it = Object.create(NI.prototype);
    traversalState.set(it, s);
    // Without weak references the list is capped (oldest first out).
    liveIterators.push(s);
    if (liveIterators.length > 4096) liveIterators.shift();
    if (!iteratorsWantJournal && typeof nativeJournalSwitch === 'function') {
      iteratorsWantJournal = true;
      try { nativeJournalSwitch(true); } catch (e) {}
    }
    return it;
  }

  // TreeWalker (DOM §6.2).
  var TW = function TreeWalker() { illegal(); };
  accessor(TW.prototype, 'root', function () { return travState(this, 'root').root; });
  accessor(TW.prototype, 'whatToShow', function () { return travState(this, 'whatToShow').whatToShow; });
  accessor(TW.prototype, 'filter', function () { return travState(this, 'filter').filter; });
  accessor(TW.prototype, 'currentNode', function () { return travState(this, 'currentNode').current; }, function (v) {
    var s = travState(this, 'currentNode');
    if (!isNode(v)) throw new TypeError("Failed to set the 'currentNode' property on 'TreeWalker': The provided value is not of type 'Node'.");
    s.current = v;
  });
  tag(TW.prototype, 'TreeWalker');
  method(TW.prototype, 'parentNode', function parentNode() {
    var s = travState(this, 'parentNode'), node = s.current;
    while (node && node !== s.root) {
      node = node.parentNode;
      if (node && filterNode(s, node) === 1) { s.current = node; return node; }
    }
    return null;
  });
  function traverseChildren(walker, first) {
    var s = travState(walker, first ? 'firstChild' : 'lastChild');
    var node = first ? s.current.firstChild : s.current.lastChild;
    while (node) {
      var r = filterNode(s, node);
      if (r === 1) { s.current = node; return node; }
      if (r === 3) {
        var child = first ? node.firstChild : node.lastChild;
        if (child) { node = child; continue; }
      }
      while (node) {
        var sib = first ? node.nextSibling : node.previousSibling;
        if (sib) { node = sib; break; }
        var parent = node.parentNode;
        if (!parent || parent === s.root || parent === s.current) return null;
        node = parent;
      }
    }
    return null;
  }
  method(TW.prototype, 'firstChild', function firstChild() { return traverseChildren(this, true); });
  method(TW.prototype, 'lastChild', function lastChild() { return traverseChildren(this, false); });
  function traverseSiblings(walker, next) {
    var s = travState(walker, next ? 'nextSibling' : 'previousSibling');
    var node = s.current;
    if (node === s.root) return null;
    for (var guard = 0; guard < 50000000; guard++) {
      var sib = next ? node.nextSibling : node.previousSibling;
      while (sib) {
        node = sib;
        var r = filterNode(s, node);
        if (r === 1) { s.current = node; return node; }
        sib = next ? node.firstChild : node.lastChild;
        if (r === 2 || !sib) sib = next ? node.nextSibling : node.previousSibling;
      }
      node = node.parentNode;
      if (!node || node === s.root) return null;
      if (filterNode(s, node) === 1) return null;
    }
    return null;
  }
  method(TW.prototype, 'nextSibling', function nextSibling() { return traverseSiblings(this, true); });
  method(TW.prototype, 'previousSibling', function previousSibling() { return traverseSiblings(this, false); });
  method(TW.prototype, 'previousNode', function previousNode() {
    var s = travState(this, 'previousNode'), node = s.current;
    for (var guard = 0; node !== s.root && guard < 50000000; guard++) {
      var sib = node.previousSibling;
      while (sib) {
        node = sib;
        var r = filterNode(s, node);
        while (r !== 2 && node.lastChild) { node = node.lastChild; r = filterNode(s, node); }
        if (r === 1) { s.current = node; return node; }
        sib = node.previousSibling;
      }
      if (node === s.root || !node.parentNode) return null;
      node = node.parentNode;
      if (filterNode(s, node) === 1) { s.current = node; return node; }
    }
    return null;
  });
  method(TW.prototype, 'nextNode', function nextNode() {
    var s = travState(this, 'nextNode'), node = s.current, r = 1;
    for (var guard = 0; guard < 50000000; guard++) {
      while (r !== 2 && node.firstChild) {
        node = node.firstChild;
        r = filterNode(s, node);
        if (r === 1) { s.current = node; return node; }
      }
      var sib = null;
      for (var t = node; t; t = t.parentNode) {
        if (t === s.root) return null;
        sib = t.nextSibling;
        if (sib) { node = sib; break; }
      }
      if (!sib) return null;
      r = filterNode(s, node);
      if (r === 1) { s.current = node; return node; }
    }
    return null;
  });
  function newWalker(root, whatToShow, filter) {
    var s = makeState(root, whatToShow, filter, 'createTreeWalker');
    s.current = root;
    var w = Object.create(TW.prototype);
    traversalState.set(w, s);
    return w;
  }
  g.NodeIterator = NI;
  g.TreeWalker = TW;
  function onDocument(name, f) {
    try {
      var D = g.Document && g.Document.prototype;
      if (D) Object.defineProperty(D, name, { value: f, configurable: true, writable: true, enumerable: true });
    } catch (e) {}
  }
  onDocument('createTreeWalker', function createTreeWalker(root, whatToShow, filter) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'createTreeWalker' on 'Document': 1 argument required, but only 0 present.");
    return newWalker(root, whatToShow, filter);
  });
  onDocument('createNodeIterator', function createNodeIterator(root, whatToShow, filter) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'createNodeIterator' on 'Document': 1 argument required, but only 0 present.");
    return newIterator(root, whatToShow, filter);
  });
})();

// ---- DOMException (WebIDL §3.14.1) ---------------------------------------
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.DOMException !== 'undefined') return;
  var CODES = {
    IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4,
    InvalidCharacterError: 5, NoModificationAllowedError: 7, NotFoundError: 8,
    NotSupportedError: 9, InUseAttributeError: 10, InvalidStateError: 11,
    SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
    InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18,
    NetworkError: 19, AbortError: 20, URLMismatchError: 21,
    QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24,
    DataCloneError: 25
  };
  function DOMException(message, name) {
    if (!(this instanceof DOMException)) {
      throw new TypeError("Failed to construct 'DOMException': Please use the 'new' operator");
    }
    this.message = message === undefined ? '' : String(message);
    this.name = name === undefined ? 'Error' : String(name);
    this.code = CODES[this.name] || 0;
  }
  DOMException.prototype = Object.create(Error.prototype);
  DOMException.prototype.constructor = DOMException;
  for (var k in CODES) {
    var c = k.replace(/([a-z])([A-Z])/g, '$1_$2').toUpperCase().replace(/_ERROR$/, '_ERR');
    DOMException[c] = CODES[k];
    DOMException.prototype[c] = CODES[k];
  }
  g.DOMException = DOMException;
})();

// HTML §4.13: Definitionen je Registry, Baustapel im Rust-Motor und
// Reaktionsschlangen je Element. DOM-Aufrufe liefern die Reaktionen vor
// ihrer Rueckkehr; ein verschachtelter Aufruf leert dieselbe Elementschlange.

// Release 11.10.2026: ein Ereignis, das der BROWSER feuert (Dialog,
// Popover, Fokus, Medien, Formulare, Bildlauf ...), ist vertrauenswuerdig
// (DOM §2.2 „isTrusted"). GEMESSEN vorher (WPT dialog-close): `close` hatte
// `isTrusted` false.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  try {
    Object.defineProperty(g, '__narsilUA', {
      value: function (K, typ, init) {
        var e = new K(typ, init);
        try { Object.defineProperty(e, 'isTrusted', { value: true, enumerable: true, configurable: true }); } catch (x) {}
        return e;
      },
      configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
})();
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var bruecke = g.__narsilCe;
  try { delete g.__narsilCe; } catch (e) {}
  var doc = g.document;
  if (!bruecke || !doc || g.customElements) return;
  var registries = new WeakMap(), zuordnung = new WeakMap();
  var definition = new WeakMap(), fertig = new WeakSet(), gescheitert = new WeakSet();
  var schlangen = new WeakMap(), imBau = new WeakSet();
  var RESERVIERT = ['annotation-xml', 'color-profile', 'font-face', 'font-face-src',
    'font-face-uri', 'font-face-format', 'font-face-name', 'missing-glyph'];
  var HTML = 'http://www.w3.org/1999/xhtml';
  var erzeugen = doc.createElement, erzeugenNS = doc.createElementNS;

  function gueltig(n) {
    // Aktuelles HTML §4.13.3: gueltiger Element-Lokalname, kleiner
    // ASCII-Anfang, keine ASCII-Grossbuchstaben, mindestens ein Bindestrich.
    if (n.length < 2 || n.charCodeAt(0) < 97 || n.charCodeAt(0) > 122 || n.indexOf('-') < 0 || RESERVIERT.indexOf(n) >= 0) return false;
    for (var i = 1; i < n.length; i++) {
      var c = n.charCodeAt(i);
      if (c === 0 || c === 9 || c === 10 || c === 12 || c === 13 || c === 32 || c === 47 || c === 62 || c >= 65 && c <= 90) return false;
    }
    return true;
  }
  function daten(r) {
    var d = registries.get(r);
    if (!d) throw new TypeError('Illegal invocation');
    return d;
  }
  function melden(e) {
    try { if (typeof g.reportError === 'function') { g.reportError(e); return; } } catch (x) {}
    try { console.error('Custom Element: ' + ((e && e.message) || e)); } catch (x) {}
  }
  function registry(n) {
    if (zuordnung.has(n)) return zuordnung.get(n);
    if (n.__ceRegistry) return n.__ceRegistry;
    if (n === doc) return globalRegistry;
    if (n.nodeType === 9) return null;
    var root = n.getRootNode && n.getRootNode();
    if (root && root !== n && zuordnung.has(root)) return zuordnung.get(root);
    return n.ownerDocument === doc ? globalRegistry : null;
  }
  function finden(el, r) {
    if (!el || el.nodeType !== 1 || el.namespaceURI !== HTML || !r) return null;
    var ds = daten(r).namen, lokal = el.localName;
    var d = ds[lokal];
    if (d && !d.erweitert) return d;
    var is = el.__ceIs;
    d = is && ds[is];
    return d && d.erweitert === lokal ? d : null;
  }
  function istCustom(el, d) {
    return !gescheitert.has(el) && (fertig.has(el) || d.proto.isPrototypeOf(el));
  }
  function leeren(el) {
    if (imBau.has(el)) return;
    var q = schlangen.get(el);
    while (q && q.length) {
      var r = q.shift();
      if (gescheitert.has(el)) continue;
      try { r[0].apply(el, r[1]); } catch (e) { melden(e); }
    }
  }
  function einreihen(el, f, args) {
    if (!f) return;
    var q = schlangen.get(el);
    if (!q) { q = []; schlangen.set(el, q); }
    q.push([f, args]);
  }
  function aufwerten(el, d) {
    if (fertig.has(el) || gescheitert.has(el) || imBau.has(el)) return;
    definition.set(el, d);
    if (d.proto.isPrototypeOf(el)) { fertig.add(el); return; }
    // Attribute werden in Attributreihenfolge vor dem Konstruktor erfasst.
    var attrs = el.attributes;
    if (d.attributeChangedCallback) {
      for (var i = 0; i < attrs.length; i++) {
        var a = attrs[i];
        if (d.beobachtet.indexOf(a.localName) >= 0)
          einreihen(el, d.attributeChangedCallback, [a.localName, null, a.value, a.namespaceURI || null]);
      }
    }
    if (el.isConnected) einreihen(el, d.connectedCallback, []);
    imBau.add(el);
    bruecke.bauen(el, d.proto);
    var ok = false;
    try {
      if (new d.ctor() !== el) throw new TypeError('The custom element constructor returned a different object');
      ok = true;
    } catch (e) { melden(e); }
    finally { bruecke.fertig(); imBau['delete'](el); }
    if (!ok) { gescheitert.add(el); schlangen['delete'](el); return; }
    fertig.add(el);
    formAktualisieren(el, d);
    leeren(el);
  }
  function gehen(root, f) {
    var s = [root];
    while (s.length) {
      var n = s.pop();
      if (!n) continue;
      f(n);
      var cs = n.childNodes || [];
      for (var i = cs.length - 1; i >= 0; i--) s.push(cs[i]);
      if (n.shadowRoot) s.push(n.shadowRoot);
    }
  }
  function CustomElementRegistry() {
    if (!new.target) throw new TypeError('CustomElementRegistry requires new');
    registries.set(this, {namen: Object.create(null), ctors: new Map(), warten: Object.create(null), laeuft: false});
  }
  var R = CustomElementRegistry.prototype;
  R.define = function define(name, ctor, options) {
    var ds = daten(this);
    name = String(name);
    if (typeof ctor !== 'function' || !ctor.prototype) throw new TypeError('The value is not a constructor');
    if (!gueltig(name)) throw new DOMException('Invalid custom element name', 'SyntaxError');
    if (ds.namen[name] || ds.ctors.has(ctor) || ds.laeuft) throw new DOMException('Duplicate or reentrant definition', 'NotSupportedError');
    var erweitert = options && options['extends'] !== undefined ? String(options['extends']) : null;
    if (erweitert !== null) {
      var probe = erzeugen.call(doc, erweitert);
      if (this !== globalRegistry || gueltig(erweitert) || probe instanceof g.HTMLUnknownElement || probe.localName !== erweitert)
        throw new DOMException('Invalid extended element', 'NotSupportedError');
    }
    var d = {name: name, ctor: ctor, proto: ctor.prototype, beobachtet: [], erweitert: erweitert, registry: this};
    if (d.proto === null || (typeof d.proto !== 'object' && typeof d.proto !== 'function')) throw new TypeError('Invalid prototype');
    ds.laeuft = true;
    try {
      var namen = ['connectedCallback', 'disconnectedCallback', 'adoptedCallback', 'attributeChangedCallback', 'connectedMoveCallback'];
      for (var j = 0; j < namen.length; j++) {
        var f = d.proto[namen[j]];
        if (f !== undefined && typeof f !== 'function') throw new TypeError(namen[j] + ' is not callable');
        d[namen[j]] = f;
      }
      var oa = d.attributeChangedCallback ? ctor.observedAttributes : undefined;
      if (oa !== undefined) {
        var roh = Array.from(oa);
        for (var m = 0; m < roh.length; m++) d.beobachtet.push(String(roh[m]));
      }
      var disabled = ctor.disabledFeatures;
      var features = disabled === undefined ? [] : Array.from(disabled, String);
      d.internalsAus = features.indexOf('internals') >= 0;
      d.shadowAus = features.indexOf('shadow') >= 0;
      d.formAssociated = !erweitert && !!ctor.formAssociated;
      if (d.formAssociated) {
        for (var z = 0; z < 4; z++) {
          var fn = ['formAssociatedCallback', 'formResetCallback', 'formDisabledCallback', 'formStateRestoreCallback'][z];
          var cb = d.proto[fn];
          if (cb !== undefined && typeof cb !== 'function') throw new TypeError(fn + ' is not callable');
          d[fn] = cb;
        }
      }
    } finally { ds.laeuft = false; }
    ds.namen[name] = d;
    ds.ctors.set(ctor, d);
    var lokal = erweitert || name;
    var basis = erweitert ? Object.getPrototypeOf(erzeugen.call(doc, erweitert)) : g.HTMLElement.prototype;
    bruecke.definieren(d.proto, name, lokal, basis, this, this === globalRegistry);
    doc.__ceDefinieren(name, d.beobachtet, lokal);
    var self = this;
    var kandidaten = doc.__ceKandidaten(name);
    for (var k = 0; k < kandidaten.length; k++) {
      var el = kandidaten[k];
      if (registry(el) === self && finden(el, self) === d) aufwerten(el, d);
    }
    var w = ds.warten[name];
    if (w) { w.aufloesen(ctor); delete ds.warten[name]; }
  };
  R.get = function get(name) { var d = daten(this).namen[String(name)]; return d ? d.ctor : undefined; };
  R.getName = function getName(ctor) {
    if (typeof ctor !== 'function') throw new TypeError('The value is not a constructor');
    var d = daten(this).ctors.get(ctor); return d ? d.name : null;
  };
  R.whenDefined = function whenDefined(name) {
    var ds = daten(this); name = String(name);
    if (!gueltig(name)) return Promise.reject(new DOMException('Invalid custom element name', 'SyntaxError'));
    if (ds.namen[name]) return Promise.resolve(ds.namen[name].ctor);
    var w = ds.warten[name];
    if (!w) {
      w = ds.warten[name] = {};
      w.p = new Promise(function (resolve) { w.aufloesen = resolve; });
    }
    return w.p;
  };
  R.upgrade = function upgrade(root) {
    daten(this);
    if (!root || typeof root.nodeType !== 'number') throw new TypeError('The value is not a Node');
    var self = this;
    gehen(root, function (el) {
      if (el.nodeType !== 1 || registry(el) !== self) return;
      var d = finden(el, self); if (d) aufwerten(el, d);
    });
  };
  R.initialize = function initialize(root) {
    daten(this);
    if (!root || typeof root.nodeType !== 'number') throw new TypeError('The value is not a Node');
    if (this === globalRegistry && (root.nodeType === 9 || registry(root.ownerDocument) !== this))
      throw new DOMException('The global registry cannot initialize this document', 'NotSupportedError');
    if ((root.nodeType === 9 || root instanceof g.ShadowRoot) && registry(root) === null) zuordnung.set(root, this);
    // initialize geht nur durch den normalen Baum, anders als upgrade.
    var stack = [root];
    while (stack.length) {
      var el = stack.pop(), cs = el.childNodes || [];
      if (el.nodeType === 1) {
        if (registry(el) === null) zuordnung.set(el, this);
        if (registry(el) === this) { var d = finden(el, this); if (d) aufwerten(el, d); }
      }
      for (var i = cs.length - 1; i >= 0; i--) stack.push(cs[i]);
    }
  };
  g.CustomElementRegistry = CustomElementRegistry;
  var globalRegistry = new CustomElementRegistry();
  g.customElements = globalRegistry;
  function eigenschaft(p) {
    if (p) Object.defineProperty(p, 'customElementRegistry', {get: function () { return registry(this); }, configurable: true});
  }
  eigenschaft(g.Document && g.Document.prototype);
  eigenschaft(g.Element && g.Element.prototype);
  eigenschaft(g.ShadowRoot && g.ShadowRoot.prototype);

  function erzeugt(context, ns, name, options, istNS) {
    var r = registry(context);
    if (options && typeof options === 'object' && options.customElementRegistry !== undefined) {
      r = options.customElementRegistry;
      if (r !== null) daten(r);
    }
    var el = istNS ? erzeugenNS.call(context, ns, name, options) : erzeugen.call(context, name, options);
    zuordnung.set(el, ns === HTML ? r : null);
    var d = ns === HTML ? finden(el, r) : null;
    if (d) {
      aufwerten(el, d);
      // Release 14.10.2026: synchronous construction (DOM §4.9 „create an
      // element" with the synchronous custom elements flag): a failed
      // constructor yields an HTMLUnknownElement (the error was reported),
      // and an autonomous element must come back without attributes,
      // children or parent, in this document and with this local name —
      // otherwise a NotSupportedError is reported and a fresh
      // HTMLUnknownElement is returned. MEASURED before (WPT
      // custom-elements/Document-createElement, 12/36): nothing was checked
      // or reported.
      if (gescheitert.has(el)) return alsUnbekannt(el);
      var lokal = istNS ? String(name).slice(String(name).indexOf(':') + 1) : String(name);
      if (!d.erweitert && (el.attributes.length || el.firstChild || el.parentNode || el.ownerDocument !== context || el.localName !== lokal)) {
        melden(new DOMException('The result of a custom element constructor must not have attributes, children or a parent, and must stay in its document', 'NotSupportedError'));
        var neu = istNS ? erzeugenNS.call(context, ns, name, options) : erzeugen.call(context, name, options);
        zuordnung.set(neu, r);
        gescheitert.add(neu);
        return alsUnbekannt(neu);
      }
    }
    return el;
  }
  function alsUnbekannt(el) {
    if (g.HTMLUnknownElement && g.HTMLUnknownElement.prototype) {
      try { Object.setPrototypeOf(el, g.HTMLUnknownElement.prototype); } catch (e) {}
    }
    return el;
  }
  // Auch inaktive Dokumente nutzen diese Methoden; ihre Registry ist null.
  g.Document.prototype.createElement = function createElement(name, options) {
    // Release 2026-10-15: lower-cased only in an HTML document (DOM §4.5
    // step 2; `__narsilXml` is the XML type of an XML document).
    var n = String(name);
    if (this == null || this.__narsilXml == null) n = n.replace(/[A-Z]/g, function (c) { return c.toLowerCase(); });
    return erzeugt(this, HTML, n, options, false);
  };
  g.Document.prototype.createElementNS = function createElementNS(ns, name, options) {
    return erzeugt(this, ns, String(name), options, true);
  };
  doc.createElement = g.Document.prototype.createElement;
  doc.createElementNS = g.Document.prototype.createElementNS;
  var attach = g.Element.prototype.attachShadow;
  if (attach) g.Element.prototype.attachShadow = function attachShadow(options) {
    var d = definition.get(this) || finden(this, registry(this));
    if (d && d.shadowAus) throw new DOMException('Shadow roots are disabled', 'NotSupportedError');
    var r = options && options.customElementRegistry !== undefined ? options.customElementRegistry : registry(this);
    if (r !== null) daten(r);
    var root = attach.call(this, options);
    zuordnung.set(root, r);
    shadowRoots.set(this, root);
    delegiert.set(root, !!(options && options.delegatesFocus));
    return root;
  };
  // Release 11.10.2026: `ShadowRoot.delegatesFocus` (DOM §4.8) und die
  // Wurzel eines Wirts auch im Modus „closed" fuer die Fokus-Schritte
  // (HTML §6.6.4 „focus delegate"). GEMESSEN vorher: `delegatesFocus`
  // fehlte, ein Dialog mit Schatten-Wirt fokussierte nie hinein.
  var delegiert = new WeakMap();
  if (g.ShadowRoot && g.ShadowRoot.prototype) {
    try {
      Object.defineProperty(g.ShadowRoot.prototype, 'delegatesFocus', {
        get: function () { return delegiert.get(this) === true; }, configurable: true, enumerable: true
      });
    } catch (e) {}
  }
  try {
    Object.defineProperty(g, '__narsilSchatten', {
      value: function (el) { return shadowRoots.get(el) || null; }, configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
  // ElementInternals (HTML §4.13.7). Die gespeicherten Daten halten das
  // Zielelement stark; das Element selbst haelt seine Internals schwach.
  var internals = new WeakMap(), internalsDaten = new WeakMap(), stateDaten = new WeakMap();
  var formStand = new WeakMap(), shadowRoots = new WeakMap();
  var token = {}, VALIDITY = ['valueMissing', 'typeMismatch', 'patternMismatch', 'tooLong', 'tooShort',
    'rangeUnderflow', 'rangeOverflow', 'stepMismatch', 'badInput', 'customError'];
  function internalsInfo(i, form) {
    var v = internalsDaten.get(i);
    if (!v) throw new TypeError('Illegal invocation');
    if (form && !v.d.formAssociated) throw new DOMException('Not a form-associated custom element', 'NotSupportedError');
    return v;
  }
  function formOwner(el) {
    var id = el.getAttributeNS(null,'form');
    if (id !== null) {
      var root = el.getRootNode();
      if (!root || !root.querySelectorAll) return null;
      var nodes = root.querySelectorAll('[id]');
      for (var i = 0; i < nodes.length; i++) if (nodes[i].id === id) return nodes[i].localName === 'form' ? nodes[i] : null;
      return null;
    }
    for (var p = el.parentElement; p; p = p.parentElement) if (p.localName === 'form') return p;
    return null;
  }
  function disabled(el) {
    if (el.getAttributeNS(null,'disabled')!==null) return true;
    for (var p = el.parentElement; p; p = p.parentElement) {
      if (p.localName !== 'fieldset' || p.getAttributeNS(null,'disabled')===null) continue;
      var cs = p.children, legend = null;
      for (var i = 0; i < cs.length; i++) if (cs[i].localName === 'legend') { legend = cs[i]; break; }
      if (!legend || !legend.contains(el)) return true;
    }
    return false;
  }
  function willValidate(el) {
    if (disabled(el) || el.getAttributeNS(null,'readonly')!==null) return false;
    for (var p = el.parentElement; p; p = p.parentElement) if (p.localName === 'datalist') return false;
    return true;
  }
  function formAktualisieren(el, d) {
    if (!d || !d.formAssociated || !istCustom(el, d)) return;
    var vorher = formStand.get(el) || {form: null, disabled: false};
    var form = formOwner(el), aus = disabled(el);
    formStand.set(el, {form: form, disabled: aus});
    if (form !== vorher.form) einreihen(el, d.formAssociatedCallback, [form]);
    if (aus !== vorher.disabled) einreihen(el, d.formDisabledCallback, [aus]);
  }
  function CustomStateSet(secret, el) {
    if (secret !== token) throw new TypeError('Illegal constructor');
    var s = new Set();
    Object.setPrototypeOf(s, CustomStateSet.prototype);
    stateDaten.set(s, el);
    return s;
  }
  CustomStateSet.prototype = Object.create(Set.prototype);
  Object.defineProperty(CustomStateSet.prototype, 'constructor', {value: CustomStateSet, writable: true, configurable: true});
  Object.defineProperty(CustomStateSet.prototype, Symbol.toStringTag, {value: 'CustomStateSet', configurable: true});
  CustomStateSet.prototype.add = function add(value) {
    if (!stateDaten.has(this)) throw new TypeError('Illegal invocation');
    Set.prototype.add.call(this, String(value));
    stateDaten.get(this).__ceStates(Array.from(this));
    return undefined;
  };
  CustomStateSet.prototype.has = function has(value) {
    if (!stateDaten.has(this)) throw new TypeError('Illegal invocation');
    return Set.prototype.has.call(this, String(value));
  };
  CustomStateSet.prototype['delete'] = function (value) {
    if (!stateDaten.has(this)) throw new TypeError('Illegal invocation');
    var removed = Set.prototype['delete'].call(this, String(value));
    if (removed) stateDaten.get(this).__ceStates(Array.from(this));
    return removed;
  };
  CustomStateSet.prototype.clear = function clear() {
    if (!stateDaten.has(this)) throw new TypeError('Illegal invocation');
    Set.prototype.clear.call(this);
    stateDaten.get(this).__ceStates([]);
  };
  g.CustomStateSet = CustomStateSet;
  function ElementInternals(secret, el, d) {
    if (secret !== token) throw new TypeError('Illegal constructor');
    var validity = {}, info = {el: el, d: d, value: null, state: null, message: '', validity: validity,
      flags: {}, states: new CustomStateSet(token, el), aria: Object.create(null), labels: null};
    internalsDaten.set(this, info);
    if (g.ValidityState) Object.setPrototypeOf(validity, g.ValidityState.prototype);
    for (var i = 0; i < VALIDITY.length; i++) (function (n) {
      Object.defineProperty(validity, n, {get: function () {return !!info.flags[n];}, enumerable: true});
    })(VALIDITY[i]);
    Object.defineProperty(validity, 'valid', {get: function () {
      for (var i = 0; i < VALIDITY.length; i++) if (info.flags[VALIDITY[i]]) return false;
      return true;
    }, enumerable: true});
  }
  g.ElementInternals = ElementInternals;
  var I = ElementInternals.prototype;
  Object.defineProperty(I, Symbol.toStringTag, {value: 'ElementInternals', configurable: true});
  function internalsGetter(name, form, get) {
    Object.defineProperty(I, name, {get: function () {return get(internalsInfo(this, form));}, configurable: true});
  }
  internalsGetter('states', false, function (i) { return i.states; });
  internalsGetter('shadowRoot', false, function (i) { return shadowRoots.get(i.el) || i.el.shadowRoot; });
  internalsGetter('form', true, function (i) {return formOwner(i.el);});
  internalsGetter('willValidate', true, function (i) {return willValidate(i.el);});
  internalsGetter('validity', true, function (i) {return i.validity;});
  internalsGetter('validationMessage', true, function (i) {return willValidate(i.el) ? i.message : '';});
  function labelsFinden(el) {
    var root = el.getRootNode(), labels = root && root.querySelectorAll ? root.querySelectorAll('label') : [];
    var result = [];
    for (var j = 0; j < labels.length; j++) {
      var label = labels[j], id = label.getAttribute('for');
      if (id !== null) {
        if (id !== el.id || !id) continue;
        var cs = root.querySelectorAll('[id]'), first = null;
        for (var k = 0; k < cs.length; k++) if (cs[k].id === id) { first = cs[k]; break; }
        if (first === el) result.push(label);
      } else {
        var control = labelControl(label);
        if (control === el) result.push(label);
      }
    }
    return result;
  }
  function labelControl(label) {
    var id = label.getAttribute('for'), root = label.getRootNode();
    var nodes = id !== null ? root.querySelectorAll('[id]') : label.querySelectorAll('*');
    for (var i = 0; i < nodes.length; i++) {
      var el = nodes[i];
      if (id !== null && el.id !== id) continue;
      var d = definition.get(el) || finden(el, registry(el));
      var labelable = d && d.formAssociated && istCustom(el,d) ||
        ['button','meter','output','progress','select','textarea'].indexOf(el.localName) >= 0 || el.localName === 'input' && el.type !== 'hidden';
      if (labelable) return el;
      if (id !== null) return null;
    }
    return null;
  }
  internalsGetter('labels', true, function (i) {
    if (!i.labels) {
      var list = []; Object.setPrototypeOf(list, g.NodeList.prototype);
      i.labels = new Proxy(list, {get: function (target, key) {
        var found = labelsFinden(i.el);
        if (key === 'length') return found.length;
        if (typeof key === 'string' && /^[0-9]+$/.test(key)) return found[Number(key)];
        if (key === Symbol.iterator) return function () { return found[Symbol.iterator](); };
        return target[key];
      }});
    }
    return i.labels;
  });
  I.setFormValue = function setFormValue(value, state) {
    var i = internalsInfo(this, true);
    if (value !== null && value !== undefined && !(g.File && value instanceof g.File) && !(g.FormData && value instanceof g.FormData)) value = String(value);
    if (state !== null && state !== undefined && !(g.File && state instanceof g.File) && !(g.FormData && state instanceof g.FormData)) state = String(state);
    i.value = value === undefined ? null : value;
    i.state = arguments.length < 2 ? i.value : state === undefined ? null : state;
  };
  I.setValidity = function setValidity(flags, message, anchor) {
    var i = internalsInfo(this, true), result = {}, invalid = false;
    if (flags === null || flags === undefined) flags = {};
    for (var j = 0; j < VALIDITY.length; j++) {
      var n = VALIDITY[j]; result[n] = !!flags[n]; invalid = invalid || result[n];
    }
    message = message === undefined ? '' : String(message);
    if (invalid && !message) throw new TypeError('Invalid flags require a validation message');
    if (anchor !== undefined) {
      if (!anchor || anchor.nodeType !== 1) throw new TypeError('The anchor is not an Element');
      var p = anchor, inside = false;
      for (var k = 0; p && k < 512; k++) {
        if (p === i.el) {inside = true; break;}
        p = p.parentNode || p.host;
      }
      if (!inside) throw new DOMException('The anchor is outside the element', 'NotFoundError');
    }
    i.flags = result; i.message = invalid ? message.replace(/\r\n|\r|\n/g, '\n') : '';
  };
  I.checkValidity = function checkValidity() {
    var i = internalsInfo(this, true);
    if (!willValidate(i.el) || i.validity.valid) return true;
    i.el.dispatchEvent(globalThis.__narsilUA(Event, 'invalid', {cancelable: true}));
    return false;
  };
  I.reportValidity = function reportValidity() {return I.checkValidity.call(this);};
  g.HTMLElement.prototype.attachInternals = function attachInternals() {
    var d = definition.get(this) || finden(this, registry(this));
    if (!d || d.erweitert || d.internalsAus || !istCustom(this,d) && !imBau.has(this) || internals.has(this))
      throw new DOMException('ElementInternals are not available', 'NotSupportedError');
    var i = new ElementInternals(token, this, d); internals.set(this,i); return i;
  };
  var aria = ['role','ariaActiveDescendantElement','ariaAtomic','ariaAutoComplete','ariaBrailleLabel','ariaBrailleRoleDescription',
    'ariaBusy','ariaChecked','ariaColCount','ariaColIndex','ariaColIndexText','ariaColSpan','ariaControlsElements','ariaCurrent',
    'ariaDescribedByElements','ariaDescription','ariaDetailsElements','ariaDisabled','ariaErrorMessageElements','ariaExpanded',
    'ariaFlowToElements','ariaHasPopup','ariaHidden','ariaInvalid','ariaKeyShortcuts','ariaLabel','ariaLabelledByElements',
    'ariaLevel','ariaLive','ariaModal','ariaMultiLine','ariaMultiSelectable','ariaOrientation','ariaOwnsElements','ariaPlaceholder',
    'ariaPosInSet','ariaPressed','ariaReadOnly','ariaRelevant','ariaRequired','ariaRoleDescription','ariaRowCount','ariaRowIndex',
    'ariaRowIndexText','ariaRowSpan','ariaSelected','ariaSetSize','ariaSort','ariaValueMax','ariaValueMin','ariaValueNow','ariaValueText'];
  var ARIA_EL = new WeakMap();
  function ariaElemente(el) { var m = ARIA_EL.get(el); if (!m) { m = {}; ARIA_EL.set(el, m); } return m; }
  // `ziel` is a descendant of one of `el`'s shadow-including ancestors.
  function ariaGueltig(el, ziel) {
    if (!ziel || typeof ziel.getRootNode !== 'function') return false;
    var zr = ziel.getRootNode();
    for (var r = el.getRootNode(), n = 0; r && n < 64; n++) {
      if (zr === r && ziel !== r) return true;
      if (r.nodeType === 11 && r.host) r = r.host.getRootNode(); else break;
    }
    return false;
  }
  // The first element in `el`'s tree whose ID is `id`.
  function ariaPerId(el, id) {
    var r = el.getRootNode();
    if ((r.nodeType === 9 || r.nodeType === 11) && typeof r.getElementById === 'function') return r.getElementById(id);
    if (r.nodeType === 1) {
      if (r.id === id) return r;
      var alle = r.querySelectorAll('[id]');
      for (var i = 0; i < alle.length; i++) if (alle[i].id === id) return alle[i];
    }
    return null;
  }
  for (var ar = 0; ar < aria.length; ar++) (function (name) {
    if (!name.endsWith('Element') && !name.endsWith('Elements')) {
      var attr = name === 'role' ? 'role' : 'aria-' + name.slice(4).toLowerCase();
      // Release 2026-10-15: a nullable DOMString takes undefined as null
      // (WebIDL §3.2.10). MEASURED before (WPT html/dom/aria-attribute-
      // reflection, 41 failures): `el.ariaAtomic = undefined` wrote "undefined".
      Object.defineProperty(g.Element.prototype,name,{get:function(){return this.getAttribute(attr);},
        set:function(value){if(value===null||value===undefined)this.removeAttribute(attr);else this.setAttribute(attr,String(value));},configurable:true});
    } else {
      // Release 2026-10-15: Element / FrozenArray<Element> reflection (HTML
      // §2.6.1 "reflecting content attributes in IDL attributes", ARIA
      // §ARIAMixin): an explicitly set element counts while it is in a tree
      // of one of the element's shadow-including ancestors (and the content
      // attribute is still the "" the setter wrote); otherwise the content
      // attribute's IDs resolve in the element's own tree. MEASURED before
      // (WPT html/dom/aria-element-reflection, 22 failures): the properties
      // were undefined on elements.
      var liste = name.endsWith('Elements');
      var attrE = 'aria-' + name.replace(/Elements?$/, '').slice(4).toLowerCase();
      Object.defineProperty(g.Element.prototype,name,{configurable:true,enumerable:true,
        get:function(){
          var slot = ariaElemente(this)[name], wert = this.getAttribute(attrE), raus;
          if (slot && slot.explizit && wert === '') {
            var self = this;
            raus = liste ? slot.explizit.filter(function(e){return ariaGueltig(self,e);}) : (ariaGueltig(this,slot.explizit) ? slot.explizit : null);
          } else if (wert === null) raus = null;
          else if (liste) {
            raus = [];
            var ids = wert.split(/[ \t\n\f\r]+/);
            for (var q = 0; q < ids.length; q++) { var e = ids[q] && ariaPerId(this, ids[q]); if (e) raus.push(e); }
          } else raus = wert ? ariaPerId(this, wert) : null;
          if (!liste) return raus;
          if (raus === null) return null;
          var c = ariaElemente(this)['#' + name];
          if (c && c.length === raus.length && c.every(function(e,i){return e===raus[i];})) return c;
          Object.freeze(raus);
          ariaElemente(this)['#' + name] = raus;
          return raus;
        },
        set:function(value){
          var m = ariaElemente(this);
          if (value === null || value === undefined) { delete m[name]; this.removeAttribute(attrE); return; }
          // WebIDL: an Element, or a sequence<Element> (an iterable object).
          var neu;
          if (liste) {
            if ((typeof value !== 'object' && typeof value !== 'function') || typeof value[Symbol.iterator] !== 'function')
              throw new TypeError("Failed to set the '" + name + "' property on 'Element': The provided value cannot be converted to a sequence.");
            neu = Array.from(value);
            for (var q = 0; q < neu.length; q++) {
              if (!(neu[q] instanceof g.Element)) throw new TypeError("Failed to set the '" + name + "' property on 'Element': Failed to convert value to 'Element'.");
            }
          } else {
            if (!(value instanceof g.Element)) throw new TypeError("Failed to set the '" + name + "' property on 'Element': Failed to convert value to 'Element'.");
            neu = value;
          }
          this.setAttribute(attrE, '');
          m[name] = { explizit: neu };
        }});
    }
    Object.defineProperty(I,name,{get:function(){var i=internalsInfo(this,false);return i.aria[name]===undefined?null:i.aria[name];},
      set:function(value){var i=internalsInfo(this,false);i.aria[name]=value===null?null:name.endsWith('Elements')?Object.freeze(Array.from(value)):name.endsWith('Element')?value:String(value);},configurable:true});
  })(aria[ar]);
  var reflectBool = [['draggable','draggable','true','false'],['spellcheck','spellcheck','true','false']];
  for (var rb=0; rb<reflectBool.length; rb++) (function(spec){
    Object.defineProperty(g.HTMLElement.prototype,spec[0],{get:function(){return this.getAttribute(spec[1])===spec[2];},
      set:function(value){this.setAttribute(spec[1],value?spec[2]:spec[3]);},configurable:true});
  })(reflectBool[rb]);
  // Release 2026-10-15: `translate` (HTML §3.2.6.3): "yes"/"" enable, "no"
  // disables, otherwise the parent element decides; without one it is
  // enabled. MEASURED before (WPT translate-inherit-no-parent-element, 5
  // failures): only "yes" was true.
  Object.defineProperty(g.HTMLElement.prototype, 'translate', { configurable: true, enumerable: true,
    get: function () {
      for (var e = this, n = 0; e && n < 1024; e = e.parentElement, n++) {
        var v = e.getAttribute('translate');
        if (v !== null) {
          v = v.toLowerCase();
          if (v === '' || v === 'yes') return true;
          if (v === 'no') return false;
        }
      }
      return true;
    },
    set: function (value) { this.setAttribute('translate', value ? 'yes' : 'no'); } });
  if (g.HTMLLabelElement) {
    Object.defineProperty(g.HTMLLabelElement.prototype,'control',{get:function(){return labelControl(this);}, configurable:true});
    Object.defineProperty(g.HTMLLabelElement.prototype,'form',{get:function(){var c=labelControl(this);return c?formOwner(c):null;}, configurable:true});
  }
  if (g.HTMLFormElement) g.HTMLFormElement.prototype.reset = function reset() {
    if (!this.dispatchEvent(globalThis.__narsilUA(Event, 'reset',{bubbles:true,cancelable:true}))) return;
    // Release 13.10.2026: erst die normalen Felder (siehe `__narsilFormZuruecksetzen`).
    if (typeof g.__narsilFormZuruecksetzen === 'function') g.__narsilFormZuruecksetzen(this);
    var form=this;
    gehen(this.getRootNode(),function(el){
      if (el.nodeType!==1) return;
      var d=definition.get(el)||finden(el,registry(el));
      if (d && d.formAssociated && istCustom(el,d) && formOwner(el)===form) {einreihen(el,d.formResetCallback,[]);leeren(el);}
    });
  };
  g.__narsilFormCustom = function (form, fd) {
    gehen(form.getRootNode(),function(el){
      var i=internals.get(el); if(!i) return;
      var info=internalsInfo(i,false);
      if(!info.d.formAssociated || formOwner(el)!==form || disabled(el) || info.value===null) return;
      if(info.value instanceof g.FormData) {
        info.value.forEach(function(value,key){fd.append(key,value);});
      } else {var name=el.getAttributeNS(null,'name');if(name)fd.append(name,info.value);}
    });
  };

  bruecke.haken(function (reaktionen) {
    var betroffene = [];
    for (var i = 0; i < reaktionen.length; i++) {
      var r = reaktionen[i], el = r[1];
      if (r[0] === 5) { if (g.__narsilMoSatz) g.__narsilMoSatz(r); continue; }
      if (r[0] === 6) { if (g.__narsilKlick) g.__narsilKlick(el); continue; }
      if (r[0] === 9) { if (g.__narsilMedien) g.__narsilMedien(el, r[2]); continue; }
      if (r[0] === 10) { if (g.__narsilSkript) g.__narsilSkript(el); continue; }
      if (r[0] === 11) { if (g.__narsilSlotEin) g.__narsilSlotEin(el); continue; }
      var d = definition.get(el) || finden(el, registry(el));
      if (!d) continue;
      if (r[0] === 3 || r[0] === 1 && !istCustom(el, d)) { aufwerten(el, d); continue; }
      if (!istCustom(el, d) && !imBau.has(el)) continue;
      if (r[0] === 1) einreihen(el, d.connectedCallback, []);
      if (r[0] === 2) einreihen(el, d.disconnectedCallback, []);
      if (r[0] === 4 && d.beobachtet.indexOf(r[2]) >= 0) einreihen(el, d.attributeChangedCallback, [r[2], r[3], r[4], r[5] || null]);
      if (r[0] === 7) einreihen(el, d.adoptedCallback, [r[2], r[3]]);
      if (!imBau.has(el)) formAktualisieren(el, d);
      if (betroffene.indexOf(el) < 0) betroffene.push(el);
    }
    for (var j = 0; j < betroffene.length; j++) leeren(betroffene[j]);
  });
})();


// ---- MessageChannel / MessagePort (HTML §9.4.4) --------------------------
//
// GEMESSEN (09.10.2026, youtube.com kevlar_base): `'MessageChannel' ist
// nicht definiert` — Scheduler nutzen den Kanal als schnellen Makrotask.
// Die Nachricht wird als eigene Aufgabe (`setTimeout(…, 0)`) zugestellt.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.MessageChannel !== 'undefined') return;
  function MessagePort() {
    this.onmessage = null;
    this.onmessageerror = null;
    this._andere = null;
    this._horcher = [];
    this._offen = true;
  }
  MessagePort.prototype.postMessage = function postMessage(daten) {
    var ziel = this._andere;
    if (!ziel || !this._offen) return;
    setTimeout(function () { ziel._zustellen(daten); }, 0);
  };
  MessagePort.prototype._zustellen = function (daten) {
    if (!this._offen) return;
    var ev = {
      type: 'message', data: daten, target: this, currentTarget: this,
      ports: [], origin: '', lastEventId: '', source: null,
      defaultPrevented: false, bubbles: false, cancelable: false,
      preventDefault: function () {}, stopPropagation: function () {},
      stopImmediatePropagation: function () {}
    };
    if (typeof this.onmessage === 'function') {
      try { this.onmessage(ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
    }
    var liste = this._horcher.slice();
    for (var i = 0; i < liste.length; i++) {
      try {
        var h = liste[i];
        if (typeof h === 'function') h.call(this, ev);
        else if (h && typeof h.handleEvent === 'function') h.handleEvent(ev);
      } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
    }
  };
  MessagePort.prototype.addEventListener = function addEventListener(typ, h) {
    if (typ === 'message' && h && this._horcher.indexOf(h) < 0) this._horcher.push(h);
  };
  MessagePort.prototype.removeEventListener = function removeEventListener(typ, h) {
    var i = this._horcher.indexOf(h);
    if (typ === 'message' && i >= 0) this._horcher.splice(i, 1);
  };
  MessagePort.prototype.dispatchEvent = function dispatchEvent(ev) {
    if (ev && ev.type === 'message') this._zustellen(ev.data);
    return true;
  };
  MessagePort.prototype.start = function start() {};
  MessagePort.prototype.close = function close() { this._offen = false; };
  function MessageChannel() {
    if (!(this instanceof MessageChannel)) throw new TypeError("Failed to construct 'MessageChannel': Please use the 'new' operator");
    this.port1 = new MessagePort();
    this.port2 = new MessagePort();
    this.port1._andere = this.port2;
    this.port2._andere = this.port1;
  }
  g.MessagePort = MessagePort;
  g.MessageChannel = MessageChannel;
})();

// ---- Fenster, Geometrie, Ereignisse, XHR … (HTML, CSSOM View, DOM) -------
//
// GEMESSEN (09.10.2026, Sonde ueber 150 Namen): `getComputedStyle`,
// `innerWidth`, `devicePixelRatio`, `screen`, `XMLHttpRequest`, `Range`,
// `getSelection`, `DOMParser`, `MessageEvent`, `getBoundingClientRect` u. a.
// fehlten ganz — Seitenskripte brachen mit ReferenceError/TypeError ab
// (youtube.com: `'Range' ist nicht definiert`). Hier stehen sie auf den
// Grundfunktionen des Motors; Geometrie ist bis zum Layout eine Naeherung.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  function def(name, wert) {
    if (typeof g[name] === 'undefined') {
      try { g[name] = wert; } catch (e) {}
    }
  }
  function schnittstelle(name, eltern) {
    if (typeof g[name] !== 'undefined') return g[name];
    var C = function () { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(C, 'name', { value: name }); } catch (e) {}
    if (eltern) C.prototype = Object.create(eltern.prototype);
    C.prototype.constructor = C;
    g[name] = C;
    return C;
  }

  // -- Fenstermasse (CSSOM View §4) --
  var masse = { breite: 1280, hoehe: 800 };
  try { if (typeof g.__narsilFenster === 'function') masse = g.__narsilFenster() || masse; } catch (e) {}
  try { delete g.__narsilFenster; } catch (e) {}
  def('innerWidth', masse.breite);
  def('innerHeight', masse.hoehe);
  def('outerWidth', masse.breite);
  def('outerHeight', masse.hoehe + 80);
  def('devicePixelRatio', 1);
  def('scrollX', 0);
  def('scrollY', 0);
  def('pageXOffset', 0);
  def('pageYOffset', 0);
  def('screenX', 0);
  def('screenY', 0);
  def('screenLeft', 0);
  def('screenTop', 0);
  def('screen', {
    width: masse.breite, height: masse.hoehe + 80, availWidth: masse.breite,
    availHeight: masse.hoehe + 40, availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24,
    orientation: { type: 'landscape-primary', angle: 0, addEventListener: function () {}, removeEventListener: function () {} }
  });
  def('visualViewport', {
    width: masse.breite, height: masse.hoehe, scale: 1, offsetLeft: 0, offsetTop: 0,
    pageLeft: 0, pageTop: 0, addEventListener: function () {}, removeEventListener: function () {}
  });
  def('scrollTo', function () {});
  def('scrollBy', function () {});
  def('scroll', function () {});
  def('open', function () { return null; });
  def('close', function () {});
  def('print', function () {});
  def('focus', function () {});
  def('blur', function () {});
  def('stop', function () {});

  // -- Geometrie (Geometry Interfaces §5/§6) --
  function DOMRectReadOnly(x, y, w, h) {
    this.x = +x || 0; this.y = +y || 0; this.width = +w || 0; this.height = +h || 0;
    this.left = Math.min(this.x, this.x + this.width);
    this.right = Math.max(this.x, this.x + this.width);
    this.top = Math.min(this.y, this.y + this.height);
    this.bottom = Math.max(this.y, this.y + this.height);
  }
  DOMRectReadOnly.prototype.toJSON = function () {
    return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, right: this.right, bottom: this.bottom, left: this.left };
  };
  DOMRectReadOnly.fromRect = function (r) { r = r || {}; return new DOMRectReadOnly(r.x, r.y, r.width, r.height); };
  function DOMRect(x, y, w, h) { DOMRectReadOnly.call(this, x, y, w, h); }
  DOMRect.prototype = Object.create(DOMRectReadOnly.prototype);
  DOMRect.prototype.constructor = DOMRect;
  DOMRect.fromRect = function (r) { r = r || {}; return new DOMRect(r.x, r.y, r.width, r.height); };
  // Release 12.10.2026: Klassennamen (`[object DOMRect]`, WPT DOMRectList).
  try {
    Object.defineProperty(DOMRectReadOnly.prototype, Symbol.toStringTag, { value: 'DOMRectReadOnly', configurable: true });
    Object.defineProperty(DOMRect.prototype, Symbol.toStringTag, { value: 'DOMRect', configurable: true });
  } catch (e) {}
  def('DOMRectReadOnly', DOMRectReadOnly);
  def('DOMRect', DOMRect);
  // Release 15.10.2026: DOMPoint/DOMMatrix come from canvas2d.js (loaded
  // on first use, see the canvas loader at the end).

  // -- Element-Methoden aus CSSOM View, Fullscreen, Pointer Events --
  var E = g.Element && g.Element.prototype;
  function an(proto, name, f) {
    if (proto && typeof proto[name] !== 'function') {
      try { proto[name] = f; } catch (e) {}
    }
  }
  if (E) {
    an(E, 'getBoundingClientRect', function () { return new DOMRect(0, 0, 0, 0); });
    an(E, 'getClientRects', function () { return []; });
    an(E, 'scrollIntoView', function () {});
    an(E, 'scrollIntoViewIfNeeded', function () {});
    an(E, 'scrollTo', function () {});
    an(E, 'scrollBy', function () {});
    an(E, 'scroll', function () {});
    an(E, 'getAnimations', function () { return []; });
    an(E, 'requestFullscreen', function () { return Promise.reject(new TypeError('Vollbild ist nicht erlaubt')); });
    an(E, 'requestPointerLock', function () {});
    an(E, 'setPointerCapture', function () {});
    an(E, 'releasePointerCapture', function () {});
    an(E, 'hasPointerCapture', function () { return false; });
    an(E, 'checkVisibility', function () { return true; });
    an(E, 'getAttributeNode', function (n) {
      var v = this.getAttribute(n);
      return v === null ? null : { name: String(n).toLowerCase(), localName: String(n).toLowerCase(), value: v, ownerElement: this, specified: true };
    });
    an(E, 'animate', function () {
      var fertig = Promise.resolve();
      return {
        finished: fertig, ready: fertig, playState: 'finished', currentTime: 0,
        cancel: function () {}, finish: function () {}, play: function () {}, pause: function () {}, reverse: function () {},
        addEventListener: function () {}, removeEventListener: function () {}, onfinish: null, oncancel: null
      };
    });
  }
  var H = g.HTMLElement && g.HTMLElement.prototype;
  if (H) {
    an(H, 'attachInternals', function () {
      return { setFormValue: function () {}, setValidity: function () {}, checkValidity: function () { return true; }, reportValidity: function () { return true; }, states: new Set(), form: null, labels: [] };
    });
  }

  // -- getComputedStyle (CSSOM §9): Inline-Stil plus Vorgaben --
  var BLOCK = /^(address|article|aside|blockquote|body|dd|details|dialog|div|dl|dt|fieldset|figcaption|figure|footer|form|h[1-6]|header|hgroup|hr|html|li|main|nav|ol|p|pre|section|summary|table|ul|ytd-app)$/;
  var VORGABE = {
    visibility: 'visible', opacity: '1', position: 'static', 'z-index': 'auto', overflow: 'visible',
    'overflow-x': 'visible', 'overflow-y': 'visible', 'transition-duration': '0s', 'transition-delay': '0s',
    'animation-duration': '0s', 'animation-name': 'none', 'pointer-events': 'auto', transform: 'none',
    'box-sizing': 'content-box', 'font-size': '16px', 'line-height': 'normal', direction: 'ltr',
    'white-space': 'normal', float: 'none', 'background-color': 'rgba(0, 0, 0, 0)', color: 'rgb(0, 0, 0)',
    width: 'auto', height: 'auto', 'margin-top': '0px', 'margin-right': '0px', 'margin-bottom': '0px', 'margin-left': '0px',
    'padding-top': '0px', 'padding-right': '0px', 'padding-bottom': '0px', 'padding-left': '0px',
    'border-top-width': '0px', 'border-right-width': '0px', 'border-bottom-width': '0px', 'border-left-width': '0px',
    'font-family': 'sans-serif', 'font-weight': '400', 'text-align': 'start', cursor: 'auto', content: 'normal'
  };
  // Release 12.10.2026: `webkitTransition` -> `-webkit-transition`,
  // `cssFloat` -> `float`, Custom Properties unveraendert (WPT cssom
  // computed-style-set-property).
  function bindestrich(n) {
    n = String(n);
    if (n.indexOf('--') === 0) return n;
    if (n === 'cssFloat') return 'float';
    return n.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }).replace(/^(webkit|moz|ms)-/, '-$1-');
  }
  function kamel(n) { return String(n).replace(/-([a-z])/g, function (m, c) { return c.toUpperCase(); }); }
  def('getComputedStyle', function getComputedStyle(el, pseudo) {
    var stil = (el && el.style) || {};
    // Release 12.10.2026: ein nicht verbundenes Element und ein Pseudoelement,
    // das kein eigenes Element-Stilbild hat (`::part()`, `::slotted()`,
    // ungueltige), liefern eine LEERE Deklaration (CSSOM §9 Schritt 3/4).
    // GEMESSEN vorher (WPT getComputedStyle-pseudo-shadow, grid-template-node-
    // not-connected): Werte und `length` 259.
    var leer = false;
    try { if (el && el.isConnected === false) leer = true; } catch (e) {}
    // Release 12.10.2026: ohne fuehrenden Doppelpunkt gilt das Argument nicht
    // (CSSOM §9 Schritt 1; WPT getComputedStyle-pseudo „ignored (due to no colon)").
    if (pseudo !== undefined && pseudo !== null && String(pseudo).charAt(0) === ':') {
      // Release 14.10.2026: CSS escapes in the argument (`:bef\\oRE`,
      // `::\\000041fter`; WPT cssom getComputedStyle-pseudo "full range of CSS
      // syntax").
      var ps = String(pseudo).replace(/\\([0-9a-fA-F]{1,6})[ \t\n]?|\\([\s\S])/g, function (m, h, z) {
        if (h) { var cp = parseInt(h, 16); if (!cp || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) cp = 0xFFFD; return String.fromCodePoint(cp); }
        return z;
      }).toLowerCase();
      // Release 12.10.2026: Funktions-Pseudoelemente nur mit `::` und einem
      // Bezeichner (WPT getComputedStyle-pseudo-with-argument).
      var ID = '-?(?:[_a-z\u0080-\uffff]|\\\\.)(?:[-_a-z0-9\u0080-\uffff]|\\\\.)*';
      var gut = /^::(before|after|first-line|first-letter|marker|placeholder|selection|backdrop|file-selector-button|picker-icon|checkmark|scroll-marker|scroll-marker-group|details-content|target-text|grammar-error|spelling-error|view-transition|column|search-text)$/.test(ps)
        || /^:(before|after|first-line|first-letter)$/.test(ps)
        || new RegExp('^::(highlight|view-transition-(group|image-pair|old|new|group-children))\\(\\s*' + ID + '\\s*(\\)$|$)').test(ps)
        || /^::scroll-button\(\s*(\*|up|down|left|right|block-start|block-end|inline-start|inline-end|prev|next)\s*(\)$|$)/.test(ps)
        || /^::picker\(\s*select\s*(\)$|$)/.test(ps);
      if (!gut) leer = true;
      // Release 12.10.2026: ::before/::after mit eigenem Stilbild
      // (`__narsilPseudoStil`; WPT cssom getComputedStyle-pseudo).
      var pm = /^::?(before|after)$/.exec(ps);
      if (pm && !leer) var pseudoArt = pm[1];
      // Release 14.10.2026: the other pseudo-elements by their rules.
      if (!pm && !leer) pseudoArt = ps.replace(/^::?/, '').replace(/\s+/g, '');
    }
    // Release 12.10.2026: anstehende CSSOM-Aenderungen (insertRule …) erst in
    // die <style>-Elemente schreiben, sonst sieht die Kaskade sie nicht.
    function wert(name) {
      if (leer) return '';
      // Release 12.10.2026: die Deklaration lebt — nach dem Trennen ist sie
      // leer (WPT grid-template-node-not-connected).
      try { if (el && el.isConnected === false) return ''; } catch (e) {}
      try { if (typeof g.__narsilCssomSpiegeln === 'function') g.__narsilCssomSpiegeln(); } catch (e) {}
      // Release 15.10.2026: the animated values of this moment.
      try { if (typeof g.__narsilAnimFlush === 'function') g.__narsilAnimFlush(el); } catch (e) {}
      var n = bindestrich(name);
      if (pseudoArt && typeof g.__narsilPseudoStil === 'function' && el && el.nodeType === 1 && n.indexOf('--') !== 0) {
        var pw = null;
        try { pw = g.__narsilPseudoStil(el, pseudoArt, n); } catch (e) { pw = null; }
        if (typeof pw === 'string') return pw;
      }
      // Release 10.10.2026: der berechnete Wert aus der Kaskade (CSSOM
      // §6.7); nur wenn der Host ihn nicht kennt, der alte Weg darunter.
      if (typeof g.__narsilStil === 'function' && el && el.nodeType === 1) {
        var h = null;
        try { h = g.__narsilStil(el, n); } catch (e) { h = null; }
        if (typeof h === 'string') return h;
      }
      var v = '';
      try { v = stil[kamel(n)]; } catch (e) {}
      if (v) return String(v);
      if (n === 'display') {
        if (el && el.hidden) return 'none';
        var t = el && el.localName;
        return t && (BLOCK.test(t)) ? 'block' : 'inline';
      }
      if (n.indexOf('--') === 0) return '';
      return VORGABE[n] !== undefined ? VORGABE[n] : '';
    }
    // Release 12.10.2026: die Aufzaehlung (`length`, `item()`, `[i]`,
    // Iteration) ueber die Langformen des Motors, und nur-lesbar (CSSOM §9:
    // `setProperty` wirft NoModificationAllowedError). GEMESSEN vorher (WPT
    // cssom getComputedStyle-property-order, -logical-enumeration):
    // „Wert ist nicht iterierbar".
    var namenListe = [];
    try { if (!leer && typeof g.__narsilStilNamen === 'function') namenListe = g.__narsilStilNamen(el) || []; } catch (e) {}
    function nurLesen(m) {
      return function () {
        throw new DOMException("Failed to execute '" + m + "' on 'CSSStyleDeclaration': These styles are computed, and therefore are read-only.", 'NoModificationAllowedError');
      };
    }
    var o = {
      getPropertyValue: function (n) { return wert(n); },
      getPropertyPriority: function () { return ''; },
      item: function (i) { i = Number(i) >>> 0; return i < namenListe.length ? namenListe[i] : ''; },
      setProperty: nurLesen('setProperty'),
      removeProperty: nurLesen('removeProperty'),
      length: namenListe.length,
      parentRule: null
    };
    try {
      Object.defineProperty(o, 'cssText', { get: function () { return ''; }, set: nurLesen('cssText'), enumerable: true, configurable: true });
    } catch (e) { o.cssText = ''; }
    if (typeof Symbol === 'function' && Symbol.iterator) {
      o[Symbol.iterator] = function () {
        var i = 0;
        return { next: function () { return i < namenListe.length ? { value: namenListe[i++], done: false } : { value: undefined, done: true }; } };
      };
    }
    // Release 10.10.2026: `'display' in getComputedStyle(el)` — die
    // Eigenschaften, die der Host berechnet (oder die Vorgaben kennen), sind
    // da. GEMESSEN vorher: `false` fuer jede Eigenschaft; die WPT-Hilfe
    // `test_computed_value` brach damit vor dem Vergleich ab (`… doesn't seem
    // to be supported in the computed style`, ~1 600 Teiltests).
    function bekannt(k) {
      var n = bindestrich(k);
      if (n.indexOf('--') === 0) return false;
      if (typeof g.__narsilStil === 'function' && el && el.nodeType === 1) {
        try { if (typeof g.__narsilStil(el, n) === 'string') return true; } catch (e) {}
      }
      return n === 'display' || VORGABE[n] !== undefined;
    }
    if (typeof Proxy === 'function') {
      return new Proxy(o, {
        get: function (z, k) {
          if (k in z) return z[k];
          if (typeof k !== 'string') return undefined;
          if (/^[0-9]+$/.test(k)) return Number(k) < namenListe.length ? namenListe[Number(k)] : undefined;
          return wert(k);
        },
        set: function (z, k, v) {
          if (typeof k === 'string' && !(k in z) && (bekannt(k) || /^[0-9]+$/.test(k))) nurLesen('setProperty')();
          z[k] = v;
          return true;
        },
        has: function (z, k) { return k in z || (typeof k === 'string' && bekannt(k)); }
      });
    }
    var namen = ['display', 'visibility', 'opacity', 'position', 'zIndex', 'overflow', 'overflowX', 'overflowY',
      'transitionDuration', 'transitionDelay', 'animationDuration', 'animationName', 'pointerEvents', 'transform',
      'boxSizing', 'fontSize', 'lineHeight', 'direction', 'whiteSpace', 'float', 'backgroundColor', 'color', 'width', 'height',
      'marginTop', 'marginRight', 'marginBottom', 'marginLeft', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
      'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth', 'fontFamily', 'fontWeight', 'textAlign', 'cursor',
      'top', 'left', 'right', 'bottom', 'maxHeight', 'maxWidth', 'minHeight', 'minWidth', 'flexDirection', 'content'];
    for (var i = 0; i < namen.length; i++) o[namen[i]] = wert(namen[i]);
    return o;
  });

  // -- Leere Schnittstellen fuer `instanceof`/Feature-Tests --
  ['NodeList', 'HTMLCollection', 'DOMTokenList', 'NamedNodeMap', 'Attr', 'CSSStyleDeclaration',
    'StyleSheet', 'CSSRule', 'CSSStyleRule', 'CSSMediaRule', 'MediaList', 'StyleSheetList', 'MediaQueryList',
    'Location', 'History', 'Navigator', 'Screen', 'Storage', 'DocumentType', 'DOMImplementation', 'XPathResult',
    'ScreenOrientation', 'VisualViewport', 'BarProp', 'MimeType', 'Plugin', 'PluginArray', 'DOMStringList',
    'CSSStyleSheet', 'DOMStringMap'].forEach(function (n) { schnittstelle(n); });
  // Release 12.10.2026: ein Doctype ist ein Knoten (DOM §4.6).
  try {
    if (g.DocumentType && g.Node && Object.getPrototypeOf(g.DocumentType.prototype) !== g.Node.prototype) {
      Object.setPrototypeOf(g.DocumentType.prototype, g.Node.prototype);
      Object.setPrototypeOf(g.DocumentType, g.Node);
    }
  } catch (e) {}
  // Release 11.10.2026: Klassennamen (`Object.prototype.toString`) und die
  // Zeichenkette einer DOMTokenList (WebIDL „stringifier", DOM §7.1).
  (function () {
    var tag = typeof Symbol === 'function' && Symbol.toStringTag;
    ['NodeList', 'HTMLCollection', 'DOMTokenList', 'NamedNodeMap', 'Attr', 'CSSStyleDeclaration', 'DOMStringMap'].forEach(function (n) {
      var P = g[n] && g[n].prototype;
      if (P && tag) { try { Object.defineProperty(P, tag, { value: n, configurable: true }); } catch (e) {} }
    });
    // Release 12.10.2026: `[...el.style]` — die Iteration ueber die
    // Langformen des Inline-Stils (WebIDL iterable ueber `item()`; WPT
    // cssom CSSStyleDeclaration-iterator, serialize-all-longhands).
    var SD = g.CSSStyleDeclaration && g.CSSStyleDeclaration.prototype;
    if (SD && typeof Symbol === 'function' && Symbol.iterator && !Object.prototype.hasOwnProperty.call(SD, Symbol.iterator)) {
      try {
        Object.defineProperty(SD, Symbol.iterator, { configurable: true, writable: true, enumerable: false, value: function () {
          var self = this, i = 0;
          return { next: function () { var n = self.length >>> 0; return i < n ? { value: self.item(i++), done: false } : { value: undefined, done: true }; } };
        } });
      } catch (e) {}
    }
    var TL = g.DOMTokenList && g.DOMTokenList.prototype;
    if (TL) {
      var def2 = function (n, f) { try { Object.defineProperty(TL, n, { value: f, configurable: true, writable: true, enumerable: false }); } catch (e) {} };
      def2('toString', function toString() { return String(this.value); });
      def2('supports', function supports() { throw new TypeError("Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens."); });
      // Release 2026-10-15: an iterable with an indexed getter takes the
      // Array.prototype functions (WebIDL §3.7.9.1). MEASURED before (WPT
      // dom/lists/DOMTokenList-iteration): forEach handed an array copy as
      // its third argument, `list[Symbol.iterator]` was our own function.
      if (typeof Symbol === 'function' && Symbol.iterator) def2(Symbol.iterator, Array.prototype[Symbol.iterator]);
      ['entries', 'keys', 'values', 'forEach'].forEach(function (m) { if (Array.prototype[m]) def2(m, Array.prototype[m]); });
    }
  })();
  if (g.NodeList && !g.NodeList.prototype.forEach) {
    g.NodeList.prototype.forEach = Array.prototype.forEach;
    g.NodeList.prototype.item = function (i) { return this[i] || null; };
  }
  // Release 11.10.2026: die Host-Listen tragen jetzt diese Prototypen
  // (`als_sammlung` im Motor): `item`/`namedItem`, Iteration, bei NodeList
  // `keys`/`values`/`entries` (DOM §4.2.10, WebIDL „iterable").
  (function () {
    function nichtAufzaehlbar(o, n, v) { try { Object.defineProperty(o, n, { value: v, configurable: true, writable: true, enumerable: false }); } catch (e) {} }
    function index(i) { var n = Number(i); n = isFinite(n) ? Math.floor(n) % 4294967296 : 0; return n < 0 ? n + 4294967296 : n; }
    var HC = g.HTMLCollection && g.HTMLCollection.prototype;
    var NL = g.NodeList && g.NodeList.prototype;
    var iter = typeof Symbol === 'function' && Symbol.iterator;
    if (HC) {
      // Release 2026-10-15: item() reads indices only (ToUint32; 2^32 - 1
      // is no index and must not fall back to a named property).
      nichtAufzaehlbar(HC, 'item', function item(i) { var k = index(i); if (k >= this.length) return null; var x = this[k]; return x === undefined ? null : x; });
      nichtAufzaehlbar(HC, 'namedItem', function namedItem(n) {
        n = String(n);
        if (!n) return null;
        // Release 2026-10-15: a live collection answers in the host (null
        // when nothing matches); a snapshot array has no such property.
        var fast = this['__narsilNamedItem:' + n];
        if (fast !== undefined) return fast;
        for (var i = 0; i < this.length; i++) {
          var e = this[i];
          if (e && (e.id === n || (e.namespaceURI === 'http://www.w3.org/1999/xhtml' && e.getAttribute && e.getAttribute('name') === n))) return e;
        }
        return null;
      });
      if (iter) nichtAufzaehlbar(HC, iter, Array.prototype[iter]);
    }
    if (NL) {
      nichtAufzaehlbar(NL, 'item', function item(i) { var k = index(i); if (k >= this.length) return null; var x = this[k]; return x === undefined ? null : x; });
      nichtAufzaehlbar(NL, 'forEach', Array.prototype.forEach);
      ['keys', 'values', 'entries'].forEach(function (m) { if (Array.prototype[m]) nichtAufzaehlbar(NL, m, Array.prototype[m]); });
      if (iter) nichtAufzaehlbar(NL, iter, Array.prototype[iter]);
    }
  })();
  // -- NamedNodeMap (DOM §4.9.1) --
  // Release 09.10.2026: `el.attributes` war ein blankes Array; youtube.com
  // (Polymer-Huelle `setUpProps`) ruft `attributes.getNamedItem("class-name")`.
  // Die Engine gibt dem Feld aus dem Host diesen Prototyp und das Feld
  // `__el` (siehe `eigenschaften.rs`, `als_named_node_map`).
  if (g.NamedNodeMap && !g.NamedNodeMap.prototype.getNamedItem) {
    var NNM = g.NamedNodeMap.prototype;
    NNM.item = function (i) { return this[i] || null; };
    NNM.getNamedItem = function (n) {
      n = String(n);
      var el = this.__el;
      if (el && el.namespaceURI === 'http://www.w3.org/1999/xhtml') n = n.toLowerCase();
      for (var i = 0; i < this.length; i++) if (this[i] && this[i].name === n) return this[i];
      return null;
    };
    NNM.getNamedItemNS = function (ns, n) { return this.getNamedItem(n); };
    NNM.setNamedItem = function (a) {
      var alt = this.getNamedItem(a.name);
      if (this.__el) this.__el.setAttribute(a.name, a.value);
      return alt;
    };
    NNM.setNamedItemNS = NNM.setNamedItem;
    NNM.removeNamedItem = function (n) {
      var alt = this.getNamedItem(n);
      if (!alt) throw new DOMException('Attribut nicht gefunden', 'NotFoundError');
      if (this.__el) this.__el.removeAttribute(alt.name);
      return alt;
    };
    NNM.removeNamedItemNS = function (ns, n) { return this.removeNamedItem(n); };
    if (typeof Symbol !== 'undefined' && Symbol.iterator) NNM[Symbol.iterator] = Array.prototype[Symbol.iterator];
  }
  if (g.CSSStyleSheet && !g.CSSStyleSheet.prototype.replaceSync) {
    // Constructable Stylesheets werden nicht angewendet (dokumentiert).
    var B = function CSSStyleSheet() { this.cssRules = []; this.rules = this.cssRules; this.disabled = false; };
    B.prototype = g.CSSStyleSheet.prototype;
    B.prototype.replaceSync = function () {};
    B.prototype.replace = function () { return Promise.resolve(this); };
    B.prototype.insertRule = function (r, i) { this.cssRules.splice(i || 0, 0, { cssText: String(r) }); return i || 0; };
    B.prototype.deleteRule = function (i) { this.cssRules.splice(i, 1); };
    g.CSSStyleSheet = B;
  }

  // -- Ereignis-Unterklassen (HTML §8.1.7, UI Events) --
  var Ev = g.Event;
  function ereignis(name, felder) {
    if (typeof g[name] !== 'undefined' || typeof Ev !== 'function') return;
    var C = function (typ, init) {
      // Release 11.10.2026: WebIDL — der Typ ist Pflicht, die Felder werden
      // nach ihrem Typ umgewandelt (DOMString, double, boolean). GEMESSEN
      // vorher (WPT toggleevent-interface): `new ToggleEvent()` warf nicht,
      // `oldState: null` blieb null statt "null".
      if (arguments.length < 1) throw new TypeError("Failed to construct '" + name + "': 1 argument required, but only 0 present.");
      var e;
      try { e = new Ev(typ, init); } catch (x) { e = { type: String(typ) }; }
      init = init || {};
      for (var i = 0; i < felder.length; i++) {
        var f = felder[i][0], vor = felder[i][1];
        var v = init[f] !== undefined ? init[f] : vor;
        if (init[f] !== undefined) {
          if (vor === '') v = String(v);
          else if (typeof vor === 'number') v = Number(v);
          else if (typeof vor === 'boolean') v = !!v;
        }
        try { Object.defineProperty(e, f, { value: v, enumerable: true, configurable: true }); } catch (x) { e[f] = v; }
      }
      try { Object.setPrototypeOf(e, C.prototype); } catch (x) {}
      return e;
    };
    C.prototype = Object.create(Ev.prototype);
    C.prototype.constructor = C;
    try { Object.defineProperty(C, 'name', { value: name }); } catch (x) {}
    g[name] = C;
  }
  ereignis('MessageEvent', [['data', null], ['origin', ''], ['lastEventId', ''], ['source', null], ['ports', []]]);
  ereignis('StorageEvent', [['key', null], ['oldValue', null], ['newValue', null], ['url', ''], ['storageArea', null]]);
  ereignis('AnimationEvent', [['animationName', ''], ['elapsedTime', 0], ['pseudoElement', '']]);
  ereignis('TransitionEvent', [['propertyName', ''], ['elapsedTime', 0], ['pseudoElement', '']]);
  ereignis('PageTransitionEvent', [['persisted', false]]);
  ereignis('PopStateEvent', [['state', null]]);
  ereignis('HashChangeEvent', [['oldURL', ''], ['newURL', '']]);
  ereignis('ErrorEvent', [['message', ''], ['filename', ''], ['lineno', 0], ['colno', 0], ['error', null]]);
  ereignis('PromiseRejectionEvent', [['promise', null], ['reason', undefined]]);
  ereignis('SecurityPolicyViolationEvent', [['violatedDirective', ''], ['blockedURI', '']]);
  ereignis('BeforeUnloadEvent', [['returnValue', '']]);
  ereignis('SubmitEvent', [['submitter', null]]);
  ereignis('ToggleEvent', [['oldState', ''], ['newState', '']]);
  ereignis('DragEvent', [['dataTransfer', null], ['clientX', 0], ['clientY', 0]]);
  ereignis('ClipboardEvent', [['clipboardData', null]]);
  ereignis('FormDataEvent', [['formData', null]]);

  // -- window.postMessage: eine eigene Aufgabe mit `message`-Ereignis --
  if (typeof g.postMessage !== 'function') {
    g.postMessage = function postMessage(daten) {
      var herkunft = '';
      try { herkunft = g.location.origin; } catch (e) {}
      setTimeout(function () {
        var ev;
        try { ev = new g.MessageEvent('message', { data: daten, origin: herkunft, source: g.window || g }); }
        catch (e) { ev = { type: 'message', data: daten, origin: herkunft, source: g }; }
        try {
          if (typeof g.onmessage === 'function') g.onmessage(ev);
        } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
        try { if (g.window && typeof g.window.dispatchEvent === 'function') g.window.dispatchEvent(ev); } catch (e) {}
      }, 0);
    };
  }

  // -- Range / StaticRange / AbstractRange (DOM §5) und Selection --
  // Release 12.10.2026: echte Bereiche nach DOM §5. Die Grenzpunkte liegen im
  // lebenden DOM (`__narsilBereich`, `bereich.rs`), und JEDE Aenderung am Baum
  // nimmt sie mit (DOM §4.2.3 insert/remove, §4.10 replace data, splitText,
  // normalize, innerHTML, textContent). Was den Baum aendert
  // (`extractContents`, `insertNode`, …), geht ueber die normalen
  // DOM-Methoden — mit Mutation Records und Custom-Element-Reaktionen.
  // GEMESSEN vorher (WPT dom/ranges, Commit 147859dd): 8 von 72 Dateien,
  // 8.829 von 43.906 Teiltests; `isPointInRange` war immer `false`,
  // `compareBoundaryPoints` immer 0, `comparePoint` fehlte, und `createRange`
  // gab es nur am Hauptdokument.
  (function () {
    var H = g.__narsilBereich;
    if (typeof H !== 'function' || !doc || !g.Node) return;
    try { delete g.__narsilBereich; } catch (e) {}
    var N = g.Node;
    var zustand = new WeakMap();
    function wirf(name, text) { throw new g.DOMException(text || ('The operation failed: ' + name + '.'), name); }
    function typFehler(was, wo, text) { return new TypeError("Failed to execute '" + was + "' on '" + wo + "': " + text); }
    function pflicht(args, n, was, wo) {
      if (args.length < n) throw typFehler(was, wo || 'Range', n + ' argument' + (n > 1 ? 's' : '') + ' required, but only ' + args.length + ' present.');
    }
    function knotenArg(n, was, nr, wo) {
      if (!(n instanceof N)) throw typFehler(was, wo || 'Range', 'parameter ' + (nr || 1) + " is not of type 'Node'.");
      return n;
    }
    // WebIDL `unsigned long` / `unsigned short`.
    function ulong(v) {
      if ((v >>> 0) === v) return v;
      var n = Number(v);
      if (!isFinite(n)) return 0;
      n = (n < 0 ? Math.ceil(n) : Math.floor(n)) % 4294967296;
      return n < 0 ? n + 4294967296 : n;
    }
    function ushort(v) { return ulong(v) % 65536; }
    function istAttr(n) { return n.nodeType === 2; }
    function istZeichen(n) { var t = n.nodeType; return t === 3 || t === 4 || t === 7 || t === 8; }
    function laenge(n) { return istAttr(n) ? 0 : H(13, n); }
    function dokVon(n) { return n.nodeType === 9 ? n : (n.ownerDocument || doc); }

    function AbstractRange() { throw new TypeError('Illegal constructor'); }
    // Der innere Zustand; Range-Methoden verlangen einen lebenden Bereich.
    function zuA(r, was) {
      var z = r !== null && (typeof r === 'object' || typeof r === 'function') ? zustand.get(r) : undefined;
      if (!z) throw new TypeError("Failed to execute '" + (was || 'get') + "' on 'AbstractRange': Illegal invocation");
      return z;
    }
    function zu(r, was) {
      var z = r !== null && (typeof r === 'object' || typeof r === 'function') ? zustand.get(r) : undefined;
      if (!z || !z.lebend) throw new TypeError("Failed to execute '" + (was || 'get') + "' on 'Range': Illegal invocation");
      return z;
    }
    // Ein lebender Bereich: Platz und Generation im Host, dazu die zuletzt
    // gelesenen Punkte (falls der Host den Platz verdraengt, M3).
    function anlegen(r, d) {
      var p = H(0, d);
      var z = { i: p ? p[0] : -1, g: p ? p[1] : -1, d: d, attr: null, s: [d, 0, d, 0], lebend: true };
      zustand.set(r, z);
      return z;
    }
    function wiederbeleben(z) {
      var p = H(0, z.d);
      if (!p) return false;
      z.i = p[0]; z.g = p[1];
      H(5, z.i, z.g, z.s[0], z.s[1], z.s[2], z.s[3]);
      return true;
    }
    function punkte(z) {
      if (!z.lebend) return z.s;
      if (z.attr) return [z.attr, 0, z.attr, 0];
      var p = H(1, z.i, z.g);
      if (p === null) { wiederbeleben(z); return z.s; }
      z.s = p;
      return p;
    }
    // Ruft eine Host-Operation auf dem Platz; ein verdraengter Platz wird
    // mit den letzten Punkten neu angelegt.
    function amPlatz(z, op, a, b, c, d) {
      var r = H(op, z.i, z.g, a, b, c, d);
      if (r === null && wiederbeleben(z)) r = H(op, z.i, z.g, a, b, c, d);
      if (typeof r === 'string') wirf(r);
      return r;
    }
    function setzen(z, sc, so, ec, eo) {
      z.attr = null;
      z.s = [sc, so, ec, eo];
      if (H(5, z.i, z.g, sc, so, ec, eo) !== true) wiederbeleben(z);
    }

    var AP = AbstractRange.prototype;
    function lesbar(o, name, f) { Object.defineProperty(o, name, { get: f, configurable: true, enumerable: true }); }
    function methode(o, name, f) { Object.defineProperty(o, name, { value: f, configurable: true, writable: true, enumerable: true }); }
    function stat(z) { return z.lebend ? punkte(z) : z.opak ? opakPunkte(z) : z.s; }
    // Release 12.10.2026: die Getter ohne Zwischenaufrufe (ein JS-Aufruf
    // kostet hier ~3 µs; WPT Range-set liest sie ~50.000-mal).
    function punkteVon(r, was) {
      var z = r !== null && typeof r === 'object' ? zustand.get(r) : undefined;
      if (!z) throw new TypeError("Failed to execute '" + was + "' on 'AbstractRange': Illegal invocation");
      if (z.lebend && !z.attr) {
        var p = H(1, z.i, z.g);
        if (p !== null) { z.s = p; return p; }
      }
      return stat(z);
    }
    lesbar(AP, 'startContainer', function () { return punkteVon(this, 'startContainer')[0]; });
    lesbar(AP, 'startOffset', function () { return punkteVon(this, 'startOffset')[1]; });
    lesbar(AP, 'endContainer', function () { return punkteVon(this, 'endContainer')[2]; });
    lesbar(AP, 'endOffset', function () { return punkteVon(this, 'endOffset')[3]; });
    lesbar(AP, 'collapsed', function () { var p = punkteVon(this, 'collapsed'); return p[0] === p[2] && p[1] === p[3]; });

    function Range() {
      if (!(this instanceof Range)) throw new TypeError("Failed to construct 'Range': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      anlegen(this, doc);
    }
    Range.prototype = Object.create(AP);
    var RP = Range.prototype;
    methode(RP, 'constructor', Range);
    var konst = [['START_TO_START', 0], ['START_TO_END', 1], ['END_TO_END', 2], ['END_TO_START', 3]];
    for (var ki = 0; ki < konst.length; ki++) {
      Object.defineProperty(Range, konst[ki][0], { value: konst[ki][1], enumerable: true });
      Object.defineProperty(RP, konst[ki][0], { value: konst[ki][1], enumerable: true });
    }
    try { Object.setPrototypeOf(Range, AbstractRange); } catch (e) {}

    lesbar(RP, 'commonAncestorContainer', function () {
      var z = zu(this, 'commonAncestorContainer');
      if (z.attr) return z.attr;
      var c = amPlatz(z, 9);
      return c === null ? z.s[0] : c;
    });

    // DOM §5.4 „set the start or end" (modus 0 Punkt, 1 davor, 2 dahinter).
    function grenze(z, ende, n, v, modus) {
      if (istAttr(n)) {
        if (modus) wirf('InvalidNodeTypeError', 'The node provided has no parent.');
        if (v > 0) wirf('IndexSizeError', 'The offset ' + v + " is larger than the node's length (0).");
        z.attr = n;
        return;
      }
      var warAttr = !!z.attr;
      amPlatz(z, 2, ende, n, v, modus);
      if (warAttr) { z.attr = null; amPlatz(z, 3, !ende); }
    }
    methode(RP, 'setStart', function setStart(n, v) {
      if (arguments.length < 2) pflicht(arguments, 2, 'setStart');
      var z = zu(this, 'setStart'); grenze(z, false, knotenArg(n, 'setStart'), ulong(v), 0);
    });
    methode(RP, 'setEnd', function setEnd(n, v) {
      if (arguments.length < 2) pflicht(arguments, 2, 'setEnd');
      var z = zu(this, 'setEnd'); grenze(z, true, knotenArg(n, 'setEnd'), ulong(v), 0);
    });
    methode(RP, 'setStartBefore', function setStartBefore(n) {
      pflicht(arguments, 1, 'setStartBefore');
      var z = zu(this, 'setStartBefore'); grenze(z, false, knotenArg(n, 'setStartBefore'), 0, 1);
    });
    methode(RP, 'setStartAfter', function setStartAfter(n) {
      pflicht(arguments, 1, 'setStartAfter');
      var z = zu(this, 'setStartAfter'); grenze(z, false, knotenArg(n, 'setStartAfter'), 0, 2);
    });
    methode(RP, 'setEndBefore', function setEndBefore(n) {
      pflicht(arguments, 1, 'setEndBefore');
      var z = zu(this, 'setEndBefore'); grenze(z, true, knotenArg(n, 'setEndBefore'), 0, 1);
    });
    methode(RP, 'setEndAfter', function setEndAfter(n) {
      pflicht(arguments, 1, 'setEndAfter');
      var z = zu(this, 'setEndAfter'); grenze(z, true, knotenArg(n, 'setEndAfter'), 0, 2);
    });
    methode(RP, 'collapse', function collapse(zumStart) {
      var z = zu(this, 'collapse');
      if (z.attr) return;
      amPlatz(z, 3, !!zumStart);
    });
    methode(RP, 'selectNode', function selectNode(n) {
      pflicht(arguments, 1, 'selectNode');
      var z = zu(this, 'selectNode'); knotenArg(n, 'selectNode');
      if (istAttr(n)) wirf('InvalidNodeTypeError', 'The node provided has no parent.');
      amPlatz(z, 4, n, false);
      z.attr = null;
    });
    methode(RP, 'selectNodeContents', function selectNodeContents(n) {
      pflicht(arguments, 1, 'selectNodeContents');
      var z = zu(this, 'selectNodeContents'); knotenArg(n, 'selectNodeContents');
      if (istAttr(n)) { z.attr = n; return; }
      amPlatz(z, 4, n, true);
      z.attr = null;
    });
    methode(RP, 'compareBoundaryPoints', function compareBoundaryPoints(wie, q) {
      pflicht(arguments, 2, 'compareBoundaryPoints');
      var z = zu(this, 'compareBoundaryPoints');
      var y = q !== null && typeof q === 'object' ? zustand.get(q) : undefined;
      if (!y || !y.lebend) throw typFehler('compareBoundaryPoints', 'Range', "parameter 2 is not of type 'Range'.");
      wie = ushort(wie);
      if (wie > 3) wirf('NotSupportedError', 'The comparison method provided must be one of START_TO_START, START_TO_END, END_TO_END, or END_TO_START.');
      if (z.attr || y.attr) {
        if (z.attr !== y.attr) wirf('WrongDocumentError', 'The two Ranges are not in the same tree.');
        return 0;
      }
      punkte(y);
      var r = amPlatz(z, 6, wie, y.i, y.g);
      return r === null ? 0 : r;
    });
    function punktFrage(self, n, v, im, was) {
      var z = zu(self, was); knotenArg(n, was); v = ulong(v);
      if (z.attr || istAttr(n)) {
        if (z.attr !== n) { if (im) return false; wirf('WrongDocumentError', 'The node provided and the Range are not in the same tree.'); }
        if (v > 0) wirf('IndexSizeError', 'The offset ' + v + " is larger than the node's length (0).");
        return im ? true : 0;
      }
      return amPlatz(z, 7, n, v, !!im);
    }
    methode(RP, 'comparePoint', function comparePoint(n, v) {
      if (arguments.length < 2) pflicht(arguments, 2, 'comparePoint');
      return punktFrage(this, n, v, false, 'comparePoint');
    });
    methode(RP, 'isPointInRange', function isPointInRange(n, v) {
      if (arguments.length < 2) pflicht(arguments, 2, 'isPointInRange');
      return punktFrage(this, n, v, true, 'isPointInRange');
    });
    methode(RP, 'intersectsNode', function intersectsNode(n) {
      pflicht(arguments, 1, 'intersectsNode');
      var z = zu(this, 'intersectsNode'); knotenArg(n, 'intersectsNode');
      if (z.attr || istAttr(n)) return z.attr === n;
      return amPlatz(z, 8, n) === true;
    });
    methode(RP, 'cloneRange', function cloneRange() {
      var z = zu(this, 'cloneRange');
      // Ein Host-Aufruf: neuer Platz mit denselben Punkten.
      if (!z.attr) {
        var k = H(20, z.i, z.g);
        if (k !== null) {
          var neu = Object.create(RP);
          zustand.set(neu, { i: k[0], g: k[1], d: z.d, attr: null, s: z.s, lebend: true });
          return neu;
        }
      }
      var p = punkte(z);
      var r = Object.create(RP);
      var y = anlegen(r, z.d);
      if (z.attr) y.attr = z.attr; else setzen(y, p[0], p[1], p[2], p[3]);
      return r;
    });
    methode(RP, 'detach', function detach() { zu(this, 'detach'); });
    methode(RP, 'toString', function toString() {
      var z = zu(this, 'toString');
      if (z.attr) return '';
      var t = H(10, z.i, z.g);
      if (t === null && wiederbeleben(z)) t = H(10, z.i, z.g);
      return typeof t === 'string' ? t : '';
    });

    // DOM §5.5 „extract" (ziehen) und „clone the contents" — rekursiv ueber
    // die teilweise enthaltenen Kinder, mit den Punkten von VOR der Aenderung.
    function auszug(sc, so, ec, eo, ziehen) {
      var frag = dokVon(sc).createDocumentFragment();
      if (sc === ec && so === eo) return [frag, sc, so];
      if (sc === ec && istZeichen(sc)) {
        var k0 = sc.cloneNode(false);
        k0.data = sc.substringData(so, eo - so);
        frag.appendChild(k0);
        if (ziehen) sc.replaceData(so, eo - so, '');
        return [frag, sc, so];
      }
      var t = H(11, sc, so, ec, eo);
      if (typeof t === 'string') wirf(t);
      if (t === null) return [frag, sc, so];
      var erstes = t[1], letztes = t[2], kinder = t[3];
      if (erstes && istZeichen(erstes)) {
        var k1 = sc.cloneNode(false);
        var l1 = laenge(sc);
        k1.data = sc.substringData(so, l1 - so);
        frag.appendChild(k1);
        if (ziehen) sc.replaceData(so, l1 - so, '');
      } else if (erstes) {
        var k2 = erstes.cloneNode(false);
        frag.appendChild(k2);
        k2.appendChild(auszug(sc, so, erstes, laenge(erstes), ziehen)[0]);
      }
      for (var i = 0; i < kinder.length; i++) frag.appendChild(ziehen ? kinder[i] : kinder[i].cloneNode(true));
      if (letztes && istZeichen(letztes)) {
        var k3 = ec.cloneNode(false);
        k3.data = ec.substringData(0, eo);
        frag.appendChild(k3);
        if (ziehen) ec.replaceData(0, eo, '');
      } else if (letztes) {
        var k4 = letztes.cloneNode(false);
        frag.appendChild(k4);
        k4.appendChild(auszug(letztes, 0, ec, eo, ziehen)[0]);
      }
      return [frag, t[4], t[5]];
    }
    function ziehen(z) {
      if (z.attr) return dokVon(z.attr).createDocumentFragment();
      var p = punkte(z);
      var r = auszug(p[0], p[1], p[2], p[3], true);
      if (!(p[0] === p[2] && p[1] === p[3])) setzen(z, r[1], r[2], r[1], r[2]);
      return r[0];
    }
    methode(RP, 'extractContents', function extractContents() { return ziehen(zu(this, 'extractContents')); });
    methode(RP, 'cloneContents', function cloneContents() {
      var z = zu(this, 'cloneContents');
      if (z.attr) return dokVon(z.attr).createDocumentFragment();
      var p = punkte(z);
      return auszug(p[0], p[1], p[2], p[3], false)[0];
    });
    methode(RP, 'deleteContents', function deleteContents() {
      var z = zu(this, 'deleteContents');
      if (z.attr) return;
      var p = punkte(z), sc = p[0], so = p[1], ec = p[2], eo = p[3];
      if (sc === ec && so === eo) return;
      if (sc === ec && istZeichen(sc)) { sc.replaceData(so, eo - so, ''); return; }
      var t = H(17, sc, so, ec, eo);
      if (!t) return;
      if (istZeichen(sc)) sc.replaceData(so, laenge(sc) - so, '');
      var weg = t[2];
      for (var i = 0; i < weg.length; i++) { var pa = weg[i].parentNode; if (pa) pa.removeChild(weg[i]); }
      if (istZeichen(ec)) ec.replaceData(0, eo, '');
      setzen(z, t[0], t[1], t[0], t[1]);
    });
    // DOM §5.5 „insert" in einen Bereich.
    function einfuegen(z, n) {
      if (z.attr) wirf('HierarchyRequestError', 'The node cannot be inserted into an attribute.');
      var p = punkte(z), sc = p[0], so = p[1];
      var typ = sc.nodeType, text = typ === 3 || typ === 4;
      if (typ === 7 || typ === 8 || (text && !sc.parentNode) || sc === n) wirf('HierarchyRequestError', 'Nodes of type \'' + sc.nodeName + '\' may not be inserted inside nodes of this type.');
      if (istAttr(n)) wirf('HierarchyRequestError', 'The node to be inserted is an attribute.');
      var ref = text ? sc : (sc.childNodes[so] || null);
      var eltern = ref ? ref.parentNode : sc;
      var f = H(15, eltern, n, ref);
      if (typeof f === 'string') wirf(f, 'The node cannot be inserted here.');
      if (text) ref = sc.splitText(so);
      if (n === ref) ref = ref.nextSibling;
      if (n.parentNode) n.parentNode.removeChild(n);
      var neu = ref ? H(14, ref) : laenge(eltern);
      neu += n.nodeType === 11 ? laenge(n) : 1;
      eltern.insertBefore(n, ref);
      var q = punkte(z);
      if (q[0] === q[2] && q[1] === q[3]) setzen(z, q[0], q[1], eltern, neu);
    }
    methode(RP, 'insertNode', function insertNode(n) {
      pflicht(arguments, 1, 'insertNode');
      var z = zu(this, 'insertNode');
      einfuegen(z, knotenArg(n, 'insertNode'));
    });
    methode(RP, 'surroundContents', function surroundContents(n) {
      pflicht(arguments, 1, 'surroundContents');
      var z = zu(this, 'surroundContents'); knotenArg(n, 'surroundContents');
      if (!z.attr) {
        var p = punkte(z);
        if (H(16, p[0], p[1], p[2], p[3]) === true) wirf('InvalidStateError', 'The Range has partially selected a non-Text node.');
      }
      var t = n.nodeType;
      if (t === 9 || t === 10 || t === 11) wirf('InvalidNodeTypeError', "The node provided is of type '" + n.nodeName + "'.");
      var frag = ziehen(z);
      if (n.firstChild) n.textContent = '';
      einfuegen(z, n);
      n.appendChild(frag);
      amPlatz(z, 4, n, false);
    });
    // HTML §8.5 `createContextualFragment`: der Kontext ist der Startknoten
    // (Text/Kommentar: sein Elternelement), sonst `body`; Skripte darin sind
    // NICHT „already started".
    methode(RP, 'createContextualFragment', function createContextualFragment(html) {
      pflicht(arguments, 1, 'createContextualFragment');
      var z = zu(this, 'createContextualFragment');
      var n = punkte(z)[0];
      var el = null;
      if (n.nodeType === 1) el = n;
      else if (n.nodeType === 3 || n.nodeType === 4 || n.nodeType === 8) el = n.parentElement;
      var od = dokVon(n);
      var HTML = 'http://www.w3.org/1999/xhtml';
      var hilf;
      if (!el || (el.localName === 'html' && el.namespaceURI === HTML)) hilf = od.createElementNS(HTML, 'body');
      else hilf = el.namespaceURI ? od.createElementNS(el.namespaceURI, el.localName) : od.createElement(el.localName);
      hilf.innerHTML = String(html);
      // Release 11.10.2026: Skripte aus `createContextualFragment` sind NICHT
      // „already started" (anders als bei `innerHTML`) — frische Elemente.
      // Release 12.10.2026: ein Skript, das bis zum Ende offen blieb, ist
      // „already started" (HTML §13.2.6.4.8 EOF im Text) und laeuft nie.
      var roh = String(html).toLowerCase(), letztes = roh.lastIndexOf('<script');
      var offen = letztes >= 0 && roh.indexOf('</script', letztes) < 0;
      var skripte = hilf.querySelectorAll('script');
      Array.prototype.forEach.call(skripte, function (alt, nr) {
        if (offen && nr === skripte.length - 1) return;
        var neu = od.createElementNS(alt.namespaceURI || HTML, alt.localName);
        for (var i = 0; i < alt.attributes.length; i++) neu.setAttribute(alt.attributes[i].name, alt.attributes[i].value);
        neu.textContent = alt.textContent;
        alt.parentNode.replaceChild(neu, alt);
      });
      var f = od.createDocumentFragment();
      // Release 2026-10-14: in an XML document the XML fragment parser also
      // returns `html`/`head`/`body` (HTML namespace); they are dropped and
      // their children kept, recursively (HTML §8.5 createContextualFragment).
      // MEASURED before (WPT createContextualFragment-xhtml with the XML
      // innerHTML): 1/4.
      var contentType = '';
      try { contentType = String(od.contentType || ''); } catch (e) {}
      var unwrapInto = function (target, source, depth) {
        while (source.firstChild) {
          var c = source.firstChild;
          if (depth < 64 && c.nodeType === 1 && c.namespaceURI === HTML && (c.localName === 'html' || c.localName === 'head' || c.localName === 'body')) {
            source.removeChild(c);
            unwrapInto(target, c, depth + 1);
          } else {
            target.appendChild(c);
          }
        }
      };
      if (contentType && contentType !== 'text/html') unwrapInto(f, hilf, 0);
      else while (hilf.firstChild) f.appendChild(hilf.firstChild);
      // Release 12.10.2026: der Fragment-Parser legt Custom Elements des
      // Kontextdokuments an (HTML §13.4, Upgrade am Ende der CEReactions).
      // GEMESSEN vorher (WPT custom-elements/reactions/Range.html): das
      // Element im Fragment blieb ohne Konstruktor.
      if (od === doc && g.customElements && typeof g.customElements.upgrade === 'function') {
        try { g.customElements.upgrade(f); } catch (e) {}
      }
      return f;
    });
    // CSSOM View §6.4: die Rahmenkaesten der Elemente im Bereich und die
    // Teilrechtecke der Textknoten (Release 12.10.2026, aus den Textlaeufen
    // des Layouts mit derselben Schriftmessung wie der Umbruch).
    var bereichKaesten = g.__narsilBereichKaesten, textKaesten = g.__narsilTextKaesten, gezeichnet = g.__narsilGezeichnet;
    try { delete g.__narsilBereichKaesten; delete g.__narsilTextKaesten; delete g.__narsilGezeichnet; } catch (e) {}
    // CSSOM View §6.2 `DOMRectList` (Release 12.10.2026; vorher ein Array).
    if (typeof g.DOMRectList === 'undefined') {
      var DOMRectList = function DOMRectList() { throw new TypeError('Illegal constructor'); };
      methode(DOMRectList.prototype, 'item', function item(i) { i = ulong(i); return i < this.length ? this[i] : null; });
      try { Object.defineProperty(DOMRectList.prototype, Symbol.iterator, { value: Array.prototype.values, configurable: true, writable: true }); } catch (e) {}
      marke(DOMRectList.prototype, 'DOMRectList');
      g.DOMRectList = DOMRectList;
    }
    function rechteckListe(rs) {
      var a = [];
      for (var i = 0; i < rs.length; i++) a.push(new g.DOMRect(rs[i][0], rs[i][1], rs[i][2], rs[i][3]));
      try { Object.setPrototypeOf(a, g.DOMRectList.prototype); } catch (e) { a.item = function (i) { return this[i] || null; }; }
      return a;
    }
    function umfang(rs) {
      if (!rs.length) return new g.DOMRect(0, 0, 0, 0);
      var echt = rs.filter(function (r) { return r[2] !== 0 && r[3] !== 0; });
      if (!echt.length) return new g.DOMRect(rs[0][0], rs[0][1], rs[0][2], rs[0][3]);
      var x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (var i = 0; i < echt.length; i++) {
        var r = echt[i];
        x0 = Math.min(x0, r[0]); y0 = Math.min(y0, r[1]);
        x1 = Math.max(x1, r[0] + r[2]); y1 = Math.max(y1, r[1] + r[3]);
      }
      return new g.DOMRect(x0, y0, x1 - x0, y1 - y0);
    }
    function bereichRechtecke(z) {
      if (z.attr || typeof bereichKaesten !== 'function') return [];
      var p = punkte(z);
      try { return bereichKaesten(p[0], p[1], p[2], p[3]) || []; } catch (e) { return []; }
    }
    methode(RP, 'getClientRects', function getClientRects() {
      return rechteckListe(bereichRechtecke(zu(this, 'getClientRects')));
    });
    methode(RP, 'getBoundingClientRect', function getBoundingClientRect() {
      return umfang(bereichRechtecke(zu(this, 'getBoundingClientRect')));
    });
    function marke(o, name) { try { Object.defineProperty(o, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {} }
    marke(AP, 'AbstractRange'); marke(RP, 'Range');
    g.Range = Range;
    g.AbstractRange = AbstractRange;

    // DOM §5.4 `StaticRange`: ohne Pruefung gegen den Baum, unveraenderlich.
    function StaticRange(init) {
      if (!(this instanceof StaticRange)) throw new TypeError("Failed to construct 'StaticRange': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      if (arguments.length < 1) throw new TypeError("Failed to construct 'StaticRange': 1 argument required, but only 0 present.");
      if (init === null || init === undefined || (typeof init !== 'object' && typeof init !== 'function')) throw new TypeError("Failed to construct 'StaticRange': The provided value is not of type 'StaticRangeInit'.");
      var ec = init.endContainer, eo = init.endOffset, sc = init.startContainer, so = init.startOffset;
      function feld(w, name, knoten) {
        if (w === undefined) throw new TypeError("Failed to construct 'StaticRange': Failed to read the '" + name + "' property from 'StaticRangeInit': Required member is undefined.");
        if (knoten && !(w instanceof N)) throw new TypeError("Failed to construct 'StaticRange': Failed to read the '" + name + "' property from 'StaticRangeInit': The provided value is not of type 'Node'.");
        return knoten ? w : ulong(w);
      }
      ec = feld(ec, 'endContainer', true); eo = feld(eo, 'endOffset', false);
      sc = feld(sc, 'startContainer', true); so = feld(so, 'startOffset', false);
      if (sc.nodeType === 10 || sc.nodeType === 2 || ec.nodeType === 10 || ec.nodeType === 2) wirf('InvalidNodeTypeError', "Failed to construct 'StaticRange': Neither startContainer nor endContainer can be a DocumentType or Attr node.");
      zustand.set(this, { lebend: false, s: [sc, so, ec, eo] });
    }
    StaticRange.prototype = Object.create(AP);
    methode(StaticRange.prototype, 'constructor', StaticRange);
    try { Object.setPrototypeOf(StaticRange, AbstractRange); } catch (e) {}
    marke(StaticRange.prototype, 'StaticRange');
    g.StaticRange = StaticRange;

    var DokP = g.Document && g.Document.prototype;
    var neuerBereich = function createRange() {
      if (!this || this.nodeType !== 9) throw new TypeError("Failed to execute 'createRange' on 'Document': Illegal invocation");
      var r = Object.create(RP);
      anlegen(r, this);
      return r;
    };
    if (DokP) methode(DokP, 'createRange', neuerBereich);
    try { if (Object.prototype.hasOwnProperty.call(doc, 'createRange')) delete doc.createRange; } catch (e) {}
    if (typeof doc.createRange !== 'function') methode(doc, 'createRange', neuerBereich);

    // -- OpaqueRange (tentativ, „OpaqueRange" fuer Formularfelder) --
    // Release 12.10.2026: `createValueRange(start, end)` an <textarea> und
    // <input> mit Textauswahl (text, search, url, tel, password). Der Bereich
    // lebt im Wert des Feldes: `setRangeText` verschiebt ihn wie „replace
    // data", ein neuer Wert (`value =`) setzt ihn auf 0; er trennt sich, wenn
    // das Feld das Dokument verlaesst, das Dokument wechselt oder seinen Typ
    // aendert. Der Wert selbst liegt im Host — ein Wechsel wird beim naechsten
    // Lesen erkannt. GEMESSEN vorher (WPT dom/ranges/tentative):
    // „createValueRange is not a function" in 13 Dateien.
    function OpaqueRange() { throw new TypeError('Illegal constructor'); }
    OpaqueRange.prototype = Object.create(AP);
    var OP = OpaqueRange.prototype;
    methode(OP, 'constructor', OpaqueRange);
    try { Object.setPrototypeOf(OpaqueRange, AbstractRange); } catch (e) {}
    var feldBereiche = new WeakMap();
    function feldTyp(el) { return el.localName === 'textarea' ? 'textarea' : String(el.type || 'text').toLowerCase(); }
    function mitWertBereich(el) { return /^(textarea|text|search|url|tel|password)$/.test(feldTyp(el)); }
    function opakTrennen(z) {
      z.aus = true; z.so = 0; z.eo = 0;
      var l = feldBereiche.get(z.el);
      if (l) { var i = l.indexOf(z); if (i >= 0) l.splice(i, 1); }
    }
    function opakPruefen(z) {
      if (z.aus) return;
      var el = z.el;
      if ((z.verbunden && !el.isConnected) || el.ownerDocument !== z.dok || feldTyp(el) !== z.typ) { opakTrennen(z); return; }
      var v = String(el.value);
      if (v !== z.wert) { z.wert = v; z.so = 0; z.eo = 0; }
    }
    function opakPunkte(z) { opakPruefen(z); return [undefined, z.so, undefined, z.eo]; }
    function zuO(r, was) {
      var z = r !== null && typeof r === 'object' ? zustand.get(r) : undefined;
      if (!z || !z.opak) throw new TypeError("Failed to execute '" + was + "' on 'OpaqueRange': Illegal invocation");
      return z;
    }
    methode(OP, 'disconnect', function disconnect() { var z = zuO(this, 'disconnect'); if (!z.aus) opakTrennen(z); });
    function opakRechtecke(z) {
      opakPruefen(z);
      if (z.aus || !z.el.isConnected || typeof textKaesten !== 'function') return [];
      try { return textKaesten(z.el, String(z.el.value), z.so, z.eo) || []; } catch (e) { return []; }
    }
    // Ein leerer Bereich hat keine Rechtecke, aber einen Einfuegemarken-Kasten.
    methode(OP, 'getClientRects', function getClientRects() {
      var z = zuO(this, 'getClientRects'); opakPruefen(z);
      return rechteckListe(z.so === z.eo ? [] : opakRechtecke(z));
    });
    methode(OP, 'getBoundingClientRect', function getBoundingClientRect() { return umfang(opakRechtecke(zuO(this, 'getBoundingClientRect'))); });
    marke(OP, 'OpaqueRange');
    var setRangeTextGehakt = false;
    function setRangeTextHaken() {
      if (setRangeTextGehakt) return;
      setRangeTextGehakt = true;
      [g.HTMLInputElement, g.HTMLTextAreaElement].forEach(function (K) {
        var P = K && K.prototype, alt = P && P.setRangeText;
        if (typeof alt !== 'function') return;
        methode(P, 'setRangeText', function setRangeText(text) {
          var l = feldBereiche.get(this);
          if (!l || !l.length) return alt.apply(this, arguments);
          var vorher = String(this.value);
          for (var i = 0; i < l.length; i++) opakPruefen(l[i]);
          var s, e;
          if (arguments.length >= 3) { s = ulong(arguments[1]); e = ulong(arguments[2]); }
          else { s = this.selectionStart; e = this.selectionEnd; }
          var r = alt.apply(this, arguments);
          s = Math.min(s, vorher.length); e = Math.min(Math.max(e, s), vorher.length);
          var neu = String(text).length, bis = e, nachher = String(this.value);
          var liste = (feldBereiche.get(this) || []).slice();
          for (var j = 0; j < liste.length; j++) {
            var z = liste[j];
            if (z.aus) continue;
            [['so'], ['eo']].forEach(function (f) {
              var o = z[f[0]];
              if (o > s && o <= bis) z[f[0]] = s;
              else if (o > bis) z[f[0]] = o - (bis - s) + neu;
            });
            z.wert = nachher;
          }
          return r;
        });
      });
    }
    function wertBereich(start, ende) {
      var el = this;
      if (!el || el.nodeType !== 1) throw new TypeError('Illegal invocation');
      if (!mitWertBereich(el)) wirf('NotSupportedError', "Failed to execute 'createValueRange': The element does not support value ranges.");
      setRangeTextHaken();
      var v = String(el.value), l = v.length;
      start = ulong(start); ende = ulong(ende);
      if (start > l || ende > l) wirf('IndexSizeError', "Failed to execute 'createValueRange': The offset is larger than the value's length (" + l + ').');
      if (ende < start) ende = start;
      var r = Object.create(OP);
      var z = { opak: true, lebend: false, el: el, so: start, eo: ende, wert: v, typ: feldTyp(el), verbunden: !!el.isConnected, dok: el.ownerDocument, aus: false };
      zustand.set(r, z);
      var liste = feldBereiche.get(el);
      if (!liste) { liste = []; feldBereiche.set(el, liste); }
      liste.push(z);
      if (liste.length > 4096) liste.shift();
      return r;
    }
    [g.HTMLInputElement, g.HTMLTextAreaElement].forEach(function (K) {
      if (K && K.prototype) methode(K.prototype, 'createValueRange', wertBereich);
    });
    g.OpaqueRange = OpaqueRange;

    // -- Highlight / HighlightRegistry (CSS Custom Highlight API §3/§4) --
    // Release 12.10.2026: `new Highlight(...bereiche)` (setlike aus
    // AbstractRange, `priority`, `type`) und `CSS.highlights` (maplike
    // Name -> Highlight). Gezeichnet wird noch nichts; `highlightsFromPoint`
    // braucht die Textgeometrie und liefert vorerst nichts. GEMESSEN vorher
    // (WPT OpaqueRange-highlight): „kein Zugriff auf Undefiniert (clear)".
    if (typeof g.Highlight === 'undefined' && typeof g.Set === 'function' && typeof g.Map === 'function') {
      var hlInnen = new WeakMap();
      var hlZu = function (h, was) {
        var z = h !== null && typeof h === 'object' ? hlInnen.get(h) : undefined;
        if (!z) throw new TypeError("Failed to execute '" + was + "' on 'Highlight': Illegal invocation");
        return z;
      };
      var istBereich = function (r) { return r !== null && typeof r === 'object' && zustand.has(r); };
      var Highlight = function Highlight() {
        if (!(this instanceof Highlight)) throw new TypeError("Failed to construct 'Highlight': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
        var z = { menge: new g.Set(), prioritaet: 0, typ: 'highlight' };
        hlInnen.set(this, z);
        for (var i = 0; i < arguments.length; i++) {
          if (!istBereich(arguments[i])) throw new TypeError("Failed to construct 'Highlight': parameter " + (i + 1) + " is not of type 'AbstractRange'.");
          z.menge.add(arguments[i]);
        }
      };
      var HP = Highlight.prototype;
      lesbar(HP, 'size', function () { return hlZu(this, 'size').menge.size; });
      Object.defineProperty(HP, 'priority', { get: function () { return hlZu(this, 'priority').prioritaet; }, set: function (v) { var n = Number(v); hlZu(this, 'priority').prioritaet = isFinite(n) ? (n < 0 ? Math.ceil(n) : Math.floor(n)) | 0 : 0; }, configurable: true, enumerable: true });
      Object.defineProperty(HP, 'type', { get: function () { return hlZu(this, 'type').typ; }, set: function (v) { v = String(v); if (v === 'highlight' || v === 'spelling-error' || v === 'grammar-error') hlZu(this, 'type').typ = v; }, configurable: true, enumerable: true });
      methode(HP, 'add', function add(r) {
        if (!istBereich(r)) throw new TypeError("Failed to execute 'add' on 'Highlight': parameter 1 is not of type 'AbstractRange'.");
        hlZu(this, 'add').menge.add(r); return this;
      });
      methode(HP, 'has', function has(r) { return hlZu(this, 'has').menge.has(r); });
      methode(HP, 'delete', function (r) { return hlZu(this, 'delete').menge['delete'](r); });
      methode(HP, 'clear', function clear() { hlZu(this, 'clear').menge.clear(); });
      methode(HP, 'forEach', function forEach(f, dies) {
        var h = this;
        hlZu(this, 'forEach').menge.forEach(function (r) { f.call(dies, r, r, h); });
      });
      methode(HP, 'values', function values() { return hlZu(this, 'values').menge.values(); });
      methode(HP, 'keys', function keys() { return hlZu(this, 'keys').menge.values(); });
      methode(HP, 'entries', function entries() { return hlZu(this, 'entries').menge.entries(); });
      try { Object.defineProperty(HP, Symbol.iterator, { value: HP.values, configurable: true, writable: true }); } catch (e) {}
      marke(HP, 'Highlight');
      g.Highlight = Highlight;

      var regInnen = new WeakMap();
      var regZu = function (h, was) {
        var z = h !== null && typeof h === 'object' ? regInnen.get(h) : undefined;
        if (!z) throw new TypeError("Failed to execute '" + was + "' on 'HighlightRegistry': Illegal invocation");
        return z;
      };
      var HighlightRegistry = function HighlightRegistry() { throw new TypeError('Illegal constructor'); };
      var RgP = HighlightRegistry.prototype;
      lesbar(RgP, 'size', function () { return regZu(this, 'size').size; });
      methode(RgP, 'set', function set(name, h) {
        if (!(h instanceof Highlight)) throw new TypeError("Failed to execute 'set' on 'HighlightRegistry': parameter 2 is not of type 'Highlight'.");
        regZu(this, 'set').set(String(name), h); return this;
      });
      methode(RgP, 'get', function get(name) { return regZu(this, 'get').get(String(name)); });
      methode(RgP, 'has', function has(name) { return regZu(this, 'has').has(String(name)); });
      methode(RgP, 'delete', function (name) { return regZu(this, 'delete')['delete'](String(name)); });
      methode(RgP, 'clear', function clear() { regZu(this, 'clear').clear(); });
      methode(RgP, 'forEach', function forEach(f, dies) {
        var r = this;
        regZu(this, 'forEach').forEach(function (h, n) { f.call(dies, h, n, r); });
      });
      methode(RgP, 'keys', function keys() { return regZu(this, 'keys').keys(); });
      methode(RgP, 'values', function values() { return regZu(this, 'values').values(); });
      methode(RgP, 'entries', function entries() { return regZu(this, 'entries').entries(); });
      // CSS Highlight API §4.2: die Hervorhebungen, deren Bereiche den Punkt
      // treffen — hoehere `priority` zuerst, bei Gleichstand die spaeter
      // angemeldete.
      methode(RgP, 'highlightsFromPoint', function highlightsFromPoint(x, y, optionen) {
        var karte = regZu(this, 'highlightsFromPoint');
        var schatten = optionen && optionen.shadowRoots ? Array.prototype.slice.call(optionen.shadowRoots) : [];
        // Bereiche in einem Schattenbaum nur, wenn seine Wurzel genannt ist.
        var sichtbar = function (r) {
          var z = zustand.get(r), n = z && z.opak ? z.el : r.startContainer;
          var w = n && typeof n.getRootNode === 'function' ? n.getRootNode() : null;
          return !(w && w.nodeType === 11 && w.host) || schatten.indexOf(w) >= 0;
        };
        if (arguments.length < 2) throw new TypeError("Failed to execute 'highlightsFromPoint' on 'HighlightRegistry': 2 arguments required, but only " + arguments.length + ' present.');
        x = Number(x); y = Number(y);
        if (!isFinite(x) || !isFinite(y)) throw new TypeError("Failed to execute 'highlightsFromPoint' on 'HighlightRegistry': The provided double value is non-finite.");
        if (x < 0 || y < 0 || x >= (g.innerWidth || 0) || y >= (g.innerHeight || 0)) return [];
        var liste = [], nr = 0;
        karte.forEach(function (h) { liste.push({ h: h, nr: nr++ }); });
        liste.sort(function (a, b) { return (b.h.priority - a.h.priority) || (b.nr - a.nr); });
        var raus = [];
        for (var i = 0; i < liste.length; i++) {
          var treffer = [];
          hlInnen.get(liste[i].h).menge.forEach(function (r) {
            if (!sichtbar(r)) return;
            var rs;
            try { rs = r.getClientRects(); } catch (e) { rs = []; }
            for (var j = 0; j < rs.length; j++) {
              var q = rs[j];
              if (x >= q.left && x < q.right && y >= q.top && y < q.bottom) { treffer.push(r); break; }
            }
          });
          if (treffer.length) raus.push({ highlight: liste[i].h, ranges: treffer });
        }
        return raus;
      });
      try { Object.defineProperty(RgP, Symbol.iterator, { value: RgP.entries, configurable: true, writable: true }); } catch (e) {}
      marke(RgP, 'HighlightRegistry');
      g.HighlightRegistry = HighlightRegistry;
      var register = Object.create(RgP);
      regInnen.set(register, new g.Map());
      if (g.CSS && (typeof g.CSS === 'object' || typeof g.CSS === 'function')) {
        try { Object.defineProperty(g.CSS, 'highlights', { value: register, configurable: true, enumerable: true }); } catch (e) {}
      }
    }

    // -- CaretPosition / caretPositionFromPoint / caretRangeFromPoint --
    // Release 12.10.2026: CSSOM View §6.1 (und das WebKit-`caretRangeFromPoint`)
    // aus den Textlaeufen: das Element unter dem Punkt, darin die naechste
    // Zeichengrenze. GEMESSEN vorher (WPT css/cssom caret*): `caretPositionFromPoint`
    // fehlte, `caretRangeFromPoint` gab immer `null`.
    var stelleAnPunkt = g.__narsilStelleAnPunkt;
    try { delete g.__narsilStelleAnPunkt; } catch (e) {}
    function CaretPosition() { throw new TypeError('Illegal constructor'); }
    var caretInnen = new WeakMap();
    function zuC(c, was) {
      var z = c !== null && typeof c === 'object' ? caretInnen.get(c) : undefined;
      if (!z) throw new TypeError("Failed to execute '" + was + "' on 'CaretPosition': Illegal invocation");
      return z;
    }
    lesbar(CaretPosition.prototype, 'offsetNode', function () { return zuC(this, 'offsetNode').n; });
    lesbar(CaretPosition.prototype, 'offset', function () { return zuC(this, 'offset').o; });
    methode(CaretPosition.prototype, 'getClientRect', function getClientRect() {
      var z = zuC(this, 'getClientRect'), n = z.n;
      if (n.localName === 'input' || n.localName === 'textarea') {
        var rs = [];
        try { rs = textKaesten(n, String(n.value), z.o, z.o) || []; } catch (e) { rs = []; }
        return rs.length ? new g.DOMRect(rs[0][0], rs[0][1], 0, rs[0][3]) : n.getBoundingClientRect();
      }
      var r = Object.create(RP); var y = anlegen(r, dokVon(n));
      setzen(y, n, z.o, n, z.o);
      return r.getBoundingClientRect();
    });
    marke(CaretPosition.prototype, 'CaretPosition');
    g.CaretPosition = CaretPosition;
    function punktStelle(d, x, y, fuerBereich) {
      if (d !== doc) return null;
      var br = (doc.documentElement && doc.documentElement.clientWidth) || g.innerWidth || 0;
      var hh = (doc.documentElement && doc.documentElement.clientHeight) || g.innerHeight || 0;
      if (!(x >= 0 && y >= 0 && x <= Math.max(br, g.innerWidth || 0) && y <= Math.max(hh, g.innerHeight || 0))) return null;
      var el = null;
      try { el = d.elementFromPoint(x, y); } catch (e) { el = null; }
      if (!el) return null;
      function anfragen(e, wert) {
        if (typeof stelleAnPunkt !== 'function') return null;
        try { return stelleAnPunkt(e, x, y, wert); } catch (x2) { return null; }
      }
      // Ein Element ohne eigenen Text: Formularfeld (Range: das Feld selbst,
      // CaretPosition: die Stelle im Wert), ersetztes Element (die Stelle im
      // Elternknoten, wie Chromium), sonst das Element mit 0.
      function elementStelle(e) {
        if (e.localName === 'input' || e.localName === 'textarea') {
          if (fuerBereich) return [e, 0];
          var q = anfragen(e, String(e.value));
          return [e, q && q[1] >= 0 ? q[1] : 0];
        }
        if (/^(audio|video|img|embed|object|iframe)$/.test(e.localName) && e.parentNode) {
          var i = 0, k = e;
          while ((k = k.previousSibling)) i++;
          var rr = e.getBoundingClientRect();
          return [e.parentNode, i + (x > rr.left + rr.width / 2 && e.localName === 'img' ? 1 : 0)];
        }
        return [e, 0];
      }
      if (el.localName === 'input' || el.localName === 'textarea') return elementStelle(el);
      var r = anfragen(el, null);
      if (!r) return elementStelle(el);
      if (r[1] === -1) return elementStelle(r[0]);
      return r;
    }
    function doppelt(v, was, nr) {
      var n = Number(v);
      if (!isFinite(n)) throw new TypeError("Failed to execute '" + was + "' on 'Document': The provided double value is non-finite.");
      return n;
    }
    if (DokP) {
      methode(DokP, 'caretPositionFromPoint', function caretPositionFromPoint(x, y) {
        if (arguments.length < 2) throw new TypeError("Failed to execute 'caretPositionFromPoint' on 'Document': 2 arguments required, but only " + arguments.length + ' present.');
        x = doppelt(x, 'caretPositionFromPoint'); y = doppelt(y, 'caretPositionFromPoint');
        var s = punktStelle(this, x, y, false);
        if (!s) return null;
        var c = Object.create(CaretPosition.prototype);
        caretInnen.set(c, { n: s[0], o: s[1] });
        return c;
      });
      methode(DokP, 'caretRangeFromPoint', function caretRangeFromPoint(x, y) {
        x = x === undefined ? 0 : doppelt(x, 'caretRangeFromPoint'); y = y === undefined ? 0 : doppelt(y, 'caretRangeFromPoint');
        var s = punktStelle(this, x, y, true);
        if (!s) return null;
        var r = Object.create(RP), z = anlegen(r, this);
        setzen(z, s[0], s[1], s[0], s[1]);
        return r;
      });
    }
    try { if (Object.prototype.hasOwnProperty.call(doc, 'caretRangeFromPoint')) delete doc.caretRangeFromPoint; } catch (e) {}

    // -- Selection (Selection API §3) --
    // Release 12.10.2026: hoechstens ein Bereich; Anker und Fokus folgen ihm
    // (er ist lebend), mit Richtung.
    function Selection() { throw new TypeError('Illegal constructor'); }
    var SP = Selection.prototype;
    var auswahlen = new WeakMap();
    function az(s) {
      var a = s !== null && typeof s === 'object' ? auswahlen.get(s) : undefined;
      if (!a) throw new TypeError('Illegal invocation');
      return a;
    }
    function auswahlVon(d) {
      var s = auswahlen.get(d);
      if (!s) {
        s = Object.create(SP);
        auswahlen.set(s, { d: d, r: null, rueck: false });
        auswahlen.set(d, s);
      }
      return s;
    }
    function bereichIm(a) { return a.r; }
    // „shadow-including root" (Selection API: collapse/extend/setBaseAndExtent).
    function imDok(a, n) { return !istAttr(n) && H(19, n) === a.d; }
    lesbar(SP, 'rangeCount', function () { return bereichIm(az(this)) ? 1 : 0; });
    function anker(s, fokus) {
      var a = az(s), r = bereichIm(a);
      if (!r) return null;
      var p = punkte(zustand.get(r));
      return (fokus !== a.rueck) ? [p[2], p[3]] : [p[0], p[1]];
    }
    lesbar(SP, 'anchorNode', function () { var x = anker(this, false); return x ? x[0] : null; });
    lesbar(SP, 'anchorOffset', function () { var x = anker(this, false); return x ? x[1] : 0; });
    lesbar(SP, 'focusNode', function () { var x = anker(this, true); return x ? x[0] : null; });
    lesbar(SP, 'focusOffset', function () { var x = anker(this, true); return x ? x[1] : 0; });
    lesbar(SP, 'isCollapsed', function () { var r = bereichIm(az(this)); return !r || r.collapsed; });
    lesbar(SP, 'type', function () { var r = bereichIm(az(this)); return !r ? 'None' : r.collapsed ? 'Caret' : 'Range'; });
    lesbar(SP, 'direction', function () { var a = az(this), r = bereichIm(a); return !r || r.collapsed ? 'none' : a.rueck ? 'backward' : 'forward'; });
    methode(SP, 'getRangeAt', function getRangeAt(i) {
      pflicht(arguments, 1, 'getRangeAt', 'Selection');
      var r = bereichIm(az(this));
      if (ulong(i) !== 0 || !r) wirf('IndexSizeError', 'The index provided (' + ulong(i) + ') is greater than or equal to the maximum bound (' + (r ? 1 : 0) + ').');
      return r;
    });
    methode(SP, 'addRange', function addRange(r) {
      pflicht(arguments, 1, 'addRange', 'Selection');
      var a = az(this);
      var y = r !== null && typeof r === 'object' ? zustand.get(r) : undefined;
      if (!y || !y.lebend) throw typFehler('addRange', 'Selection', "parameter 1 is not of type 'Range'.");
      if (bereichIm(a)) return;
      // Wie Chromium: ein verbundener Bereich, auch in einem Schattenbaum.
      if (y.attr || H(19, punkte(y)[0]) !== a.d) return;
      a.r = r; a.rueck = false;
    });
    methode(SP, 'removeRange', function removeRange(r) {
      pflicht(arguments, 1, 'removeRange', 'Selection');
      var a = az(this);
      if (!r || !zustand.get(r)) throw typFehler('removeRange', 'Selection', "parameter 1 is not of type 'Range'.");
      if (a.r !== r || !bereichIm(a)) wirf('NotFoundError', 'The given range isn\'t in document.');
      a.r = null;
    });
    methode(SP, 'removeAllRanges', function removeAllRanges() { az(this).r = null; });
    methode(SP, 'empty', function empty() { az(this).r = null; });
    function neuIn(a, n, v) {
      var r = Object.create(RP);
      var z = anlegen(r, a.d);
      grenze(z, false, n, v, 0);
      return r;
    }
    function punktPruefen(n, v, was) {
      if (n.nodeType === 10) wirf('InvalidNodeTypeError', "Failed to execute '" + was + "' on 'Selection': The node provided is of type 'DocumentType'.");
      if (v > laenge(n)) wirf('IndexSizeError', "Failed to execute '" + was + "' on 'Selection': " + v + " is larger than the node's length (" + laenge(n) + ').');
    }
    function zusammen(s, n, v, was) {
      var a = az(s);
      if (n === null) { a.r = null; return; }
      knotenArg(n, was, 1, 'Selection');
      v = ulong(v);
      punktPruefen(n, v, was);
      if (!imDok(a, n)) return;
      a.r = neuIn(a, n, v); a.rueck = false;
    }
    methode(SP, 'collapse', function collapse(n, v) { pflicht(arguments, 1, 'collapse', 'Selection'); zusammen(this, n, v, 'collapse'); });
    methode(SP, 'setPosition', function setPosition(n, v) { pflicht(arguments, 1, 'setPosition', 'Selection'); zusammen(this, n, v, 'setPosition'); });
    function zuEnde(s, start, was) {
      var a = az(s), r = bereichIm(a);
      if (!r) wirf('InvalidStateError', "Failed to execute '" + was + "' on 'Selection': there is no selection.");
      var p = punkte(zustand.get(r));
      a.r = neuIn(a, start ? p[0] : p[2], start ? p[1] : p[3]); a.rueck = false;
    }
    methode(SP, 'collapseToStart', function collapseToStart() { zuEnde(this, true, 'collapseToStart'); });
    methode(SP, 'collapseToEnd', function collapseToEnd() { zuEnde(this, false, 'collapseToEnd'); });
    methode(SP, 'extend', function extend(n, v) {
      pflicht(arguments, 1, 'extend', 'Selection');
      var a = az(this);
      knotenArg(n, 'extend', 1, 'Selection');
      v = ulong(v);
      if (!imDok(a, n)) return;
      var r = bereichIm(a);
      if (!r) wirf('InvalidStateError', "Failed to execute 'extend' on 'Selection': This Selection object doesn't have any Ranges.");
      punktPruefen(n, v, 'extend');
      var x = anker(this, false);
      var neu = neuIn(a, x[0], x[1]);
      var lage = H(12, n, v, x[0], x[1]);
      var nz = zustand.get(neu);
      if (lage === -1) { grenze(nz, false, n, v, 0); a.rueck = true; }
      else { grenze(nz, true, n, v, 0); a.rueck = false; }
      a.r = neu;
    });
    methode(SP, 'setBaseAndExtent', function setBaseAndExtent(an, ao, fn, fo) {
      pflicht(arguments, 4, 'setBaseAndExtent', 'Selection');
      var a = az(this);
      knotenArg(an, 'setBaseAndExtent', 1, 'Selection'); knotenArg(fn, 'setBaseAndExtent', 3, 'Selection');
      ao = ulong(ao); fo = ulong(fo);
      punktPruefen(an, ao, 'setBaseAndExtent'); punktPruefen(fn, fo, 'setBaseAndExtent');
      if (!imDok(a, an) || !imDok(a, fn)) return;
      var r = neuIn(a, an, ao), z = zustand.get(r);
      if (H(12, fn, fo, an, ao) === -1) { grenze(z, false, fn, fo, 0); a.rueck = true; }
      else { grenze(z, true, fn, fo, 0); a.rueck = false; }
      a.r = r;
    });
    methode(SP, 'selectAllChildren', function selectAllChildren(n) {
      pflicht(arguments, 1, 'selectAllChildren', 'Selection');
      var a = az(this);
      knotenArg(n, 'selectAllChildren', 1, 'Selection');
      if (n.nodeType === 10) wirf('InvalidNodeTypeError', "Failed to execute 'selectAllChildren' on 'Selection': The node provided is of type 'DocumentType'.");
      if (!imDok(a, n)) return;
      var r = Object.create(RP), z = anlegen(r, a.d);
      amPlatz(z, 4, n, true);
      a.r = r; a.rueck = false;
    });
    methode(SP, 'containsNode', function containsNode(n, teil) {
      pflicht(arguments, 1, 'containsNode', 'Selection');
      var a = az(this);
      knotenArg(n, 'containsNode', 1, 'Selection');
      var r = bereichIm(a);
      if (!r || istAttr(n) || H(18, n) !== a.d) return false;
      var p = punkte(zustand.get(r));
      var anfang = H(12, n, 0, p[0], p[1]), ende = H(12, n, laenge(n), p[2], p[3]);
      if (!teil) return anfang !== -1 && ende !== 1;
      var vorn = H(12, n, 0, p[2], p[3]), hinten = H(12, n, laenge(n), p[0], p[1]);
      return vorn !== 1 && hinten !== -1;
    });
    methode(SP, 'deleteFromDocument', function deleteFromDocument() {
      var r = bereichIm(az(this));
      if (r) r.deleteContents();
    });
    methode(SP, 'modify', function modify() {});
    methode(SP, 'getComposedRanges', function getComposedRanges() {
      var r = bereichIm(az(this));
      if (!r) return [];
      return [new StaticRange({ startContainer: r.startContainer, startOffset: r.startOffset, endContainer: r.endContainer, endOffset: r.endOffset })];
    });
    // Chromium liefert den gezeichneten Text (`text-transform`, Leerraum);
    // innerhalb eines Textknotens aus den Laeufen des Layouts, sonst der
    // Text des Bereichs.
    methode(SP, 'toString', function toString() {
      var r = bereichIm(az(this));
      if (!r) return '';
      var p = punkte(zustand.get(r));
      if (!(p[0] === p[2] && p[1] === p[3]) && typeof gezeichnet === 'function') {
        var t = null;
        try { t = gezeichnet(p[0], p[1], p[2], p[3]); } catch (e) { t = null; }
        if (typeof t === 'string') return t;
      }
      return r.toString();
    });
    marke(SP, 'Selection');
    g.Selection = Selection;
    var holen = function getSelection() {
      var d = this && this.nodeType === 9 ? this : doc;
      return auswahlVon(d);
    };
    // Ein Rahmenfenster erbt vom Hauptfenster: `this` ist dann das Rahmenfenster.
    g.getSelection = function getSelection() {
      var d = this && this !== g && this.document && this.document.nodeType === 9 ? this.document : doc;
      return auswahlVon(d);
    };
    if (DokP) methode(DokP, 'getSelection', holen);
    // `ShadowRoot.getSelection()` (Chromium): die Auswahl des Dokuments.
    if (g.ShadowRoot && g.ShadowRoot.prototype) methode(g.ShadowRoot.prototype, 'getSelection', function getSelection() {
      return auswahlVon(this.ownerDocument || doc);
    });
    try { if (Object.prototype.hasOwnProperty.call(doc, 'getSelection')) delete doc.getSelection; } catch (e) {}
    if (typeof doc.getSelection !== 'function') methode(doc, 'getSelection', holen);
  })();

  // -- Dokument-Methoden --
  if (doc) {
    // Release 11.10.2026: `elementFromPoint`/`elementsFromPoint` (CSSOM View
    // §6) aus den gezeichneten Kaesten (`getBoundingClientRect`, mit
    // `transform`): die in Dokumentreihenfolge letzten Treffer liegen oben;
    // `visibility: hidden` und `pointer-events: none` fangen nichts.
    // GEMESSEN vorher: immer `null` (WPT css-transforms „point is inside
    // element box").
    var trefferListe = function (x, y, n) {
      // Release 11.10.2026: WebIDL `double` — beide Pflicht und endlich.
      if (n < 2) throw new TypeError("Failed to execute 'elementFromPoint' on 'Document': 2 arguments required, but only " + n + " present.");
      x = Number(x); y = Number(y);
      if (!isFinite(x) || !isFinite(y)) throw new TypeError("Failed to execute 'elementFromPoint' on 'Document': The provided double value is non-finite.");
      var breit = g.innerWidth || 0, hoch = g.innerHeight || 0;
      if (!(x >= 0 && y >= 0 && x < breit && y < hoch)) return null;
      var alle = doc.getElementsByTagName('*'), raus = [];
      for (var i = 0; i < alle.length; i++) {
        var el = alle[i];
        if (typeof el.getBoundingClientRect !== 'function') continue;
        var r = el.getBoundingClientRect();
        if (!(r && r.width > 0 && r.height > 0 && x >= r.left && x < r.left + r.width && y >= r.top && y < r.top + r.height)) continue;
        var st = typeof g.getComputedStyle === 'function' ? g.getComputedStyle(el) : null;
        if (st && (st.visibility === 'hidden' || st.pointerEvents === 'none')) continue;
        // Release 11.10.2026: inerte Knoten fangen nichts (HTML §6.3).
        if (typeof g.__narsilInert === 'function' && g.__narsilInert(el)) continue;
        raus.push(el);
      }
      raus.reverse();
      // Die oberste Schicht (modale Dialoge, offene Popover) liegt ueber
      // allem, in ihrer eigenen Reihenfolge (CSS Position 4 §3).
      var schicht = typeof g.__narsilObersteSchicht === 'function' ? g.__narsilObersteSchicht() : [];
      if (schicht.length && raus.length) {
        var stufen = [];
        for (var s2 = 0; s2 <= schicht.length; s2++) stufen.push([]);
        for (var j = 0; j < raus.length; j++) {
          var st2 = 0;
          for (var q = schicht.length - 1; q >= 0; q--) { if (schicht[q].contains(raus[j])) { st2 = q + 1; break; } }
          stufen[st2].push(raus[j]);
        }
        raus = [];
        for (var s3 = stufen.length - 1; s3 >= 0; s3--) raus = raus.concat(stufen[s3]);
      }
      var wurzel = doc.documentElement;
      if (wurzel && raus.indexOf(wurzel) < 0) raus.push(wurzel);
      return raus;
    };
    var DokP = g.Document && g.Document.prototype;
    var vonPunkt = function elementFromPoint(x, y) {
      var l = trefferListe(x, y, arguments.length);
      return l && l.length ? l[0] : null;
    };
    var allesVonPunkt = function elementsFromPoint(x, y) {
      return trefferListe(x, y, arguments.length) || [];
    };
    if (typeof doc.elementFromPoint !== 'function') doc.elementFromPoint = vonPunkt;
    if (typeof doc.elementsFromPoint !== 'function') doc.elementsFromPoint = allesVonPunkt;
    if (DokP && typeof DokP.elementFromPoint !== 'function') DokP.elementFromPoint = vonPunkt;
    if (DokP && typeof DokP.elementsFromPoint !== 'function') DokP.elementsFromPoint = allesVonPunkt;
    if (typeof doc.caretRangeFromPoint !== 'function') doc.caretRangeFromPoint = function () { return null; };
    if (typeof doc.hasFocus !== 'function') doc.hasFocus = function () { return true; };
    if (typeof doc.execCommand !== 'function') doc.execCommand = function () { return false; };
    if (typeof doc.queryCommandSupported !== 'function') doc.queryCommandSupported = function () { return false; };
    if (typeof doc.exitFullscreen !== 'function') doc.exitFullscreen = function () { return Promise.resolve(); };
    if (typeof doc.getAnimations !== 'function') doc.getAnimations = function () { return []; };
    if (typeof doc.createAttribute !== 'function') doc.createAttribute = function (n) { return { name: String(n).toLowerCase(), value: '', specified: true }; };
  }

  // -- DOMParser / XMLSerializer (DOM Parsing §3/§4) --
  // Release 13.10.2026: das erzeugte Dokument traegt die Adresse des
  // Dokuments, zu dessen Fenster der PARSER gehoert (HTML §8.5.1
  // „parseFromString": URL = die des zugehoerigen Dokuments), nicht die des
  // Fensters der Methode. Jedes Fenster bekommt einen eigenen Konstruktor:
  // ein Rahmenfenster erbt ueber Object.create(g), `frames[0].DOMParser`
  // laeuft deshalb ueber einen Getter am globalen Objekt mit dem Fenster als
  // `this`. GEMESSEN vorher (WPT DOMParser-parseFromString-url*.html, 211
  // Teiltests): `doc.URL`/`documentURI` waren undefined, `baseURI` die des
  // Hauptdokuments.
  if (typeof g.DOMParser === 'undefined' && doc && doc.implementation) {
    var wirtDok = doc;
    var parserZiel = new WeakMap();
    var parserKonstruktoren = new WeakMap();
    var adresseSetzen = function (d, url) {
      if (!d || typeof url !== 'string') return;
      try { Object.defineProperty(d, 'URL', { get: function () { return url; }, configurable: true }); } catch (e) {}
      try { Object.defineProperty(d, 'documentURI', { get: function () { return url; }, configurable: true }); } catch (e) {}
      // Fertig geparst (HTML §8.5.1: „current document readiness" complete).
      try { Object.defineProperty(d, 'readyState', { get: function () { return 'complete'; }, configurable: true }); } catch (e) {}
    };
    // Ein DOMParser-Dokument ist ein `Document`, kein `XMLDocument` (DOM
    // Parsing §3.1 „a new Document"). GEMESSEN vorher (WPT
    // DOMParser-parseFromString-xml): `instanceof XMLDocument` war wahr.
    var alsDocument = function (d) {
      if (g.Document && g.Document.prototype) { try { Object.setPrototypeOf(d, g.Document.prototype); } catch (e) {} }
      return d;
    };
    var parsen = function (text, typ) {
      // Release 11.10.2026: XML-Typen mit dem XML-Parser des Hosts (DOM
      // Parsing §3.2); ein Fehler ergibt ein `parsererror`-Dokument.
      var t = String(typ);
      if (t === 'text/xml' || t === 'application/xml' || t === 'application/xhtml+xml' || t === 'image/svg+xml') {
        try {
          return alsDocument(wirtDok.__narsilXmlParsen(String(text), t));
        } catch (e) {
          // Das Fehlerdokument behaelt den Typ (`contentType`).
          var f = null;
          try {
            f = wirtDok.__narsilXmlParsen('<x/>', t);
            f.replaceChild(f.createElementNS('http://www.mozilla.org/newlayout/xml/parsererror.xml', 'parsererror'), f.documentElement);
          } catch (e2) { f = null; }
          if (!f) f = wirtDok.implementation.createDocument('http://www.mozilla.org/newlayout/xml/parsererror.xml', 'parsererror', null);
          try { f.documentElement.textContent = String(e && e.message || e); } catch (e2) {}
          return alsDocument(f);
        }
      }
      if (t !== 'text/html') throw new TypeError("Failed to execute 'parseFromString' on 'DOMParser': The provided value '" + t + "' is not a valid enum value of type DOMParserSupportedType.");
      // Release 13.10.2026: das ganze Dokument mit dem Baumbauer des Hosts
      // (Attribute am `<html>`, Doctype, `<noscript>` als Markup).
      if (typeof wirtDok.__narsilHtmlParsen === 'function') {
        try { return wirtDok.__narsilHtmlParsen(String(text), false); } catch (e) {}
      }
      var d = wirtDok.implementation.createHTMLDocument('');
      var html = String(text);
      try {
        // Release 12.10.2026: ein fuehrender Doctype (ohne `<html>`) kommt
        // vorher weg — im Fragment-Parser landete danach `<title>` im Body.
        // GEMESSEN (WPT Range-test-iframe.html): `document.head` war leer.
        html = html.replace(/^\s*(<!--[\s\S]*?-->\s*)*<!doctype[^>]*>/i, '');
        d.documentElement.innerHTML = html.replace(/^[\s\S]*?<html[^>]*>/i, '').replace(/<\/html>[\s\S]*$/i, '');
      } catch (e) {}
      return d;
    };
    var parserBauen = function (win) {
      var P = function DOMParser() {
        if (!(this instanceof P)) throw new TypeError("Failed to construct 'DOMParser': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
        var d = null;
        try { d = win.document || null; } catch (e) {}
        var u = '';
        try { u = d ? String(d.URL) : ''; } catch (e) {}
        parserZiel.set(this, { win: win, dok: d, url: u });
      };
      var parseFromString = function parseFromString(text, typ) {
        var z = parserZiel.get(this);
        if (!z) throw new TypeError('Illegal invocation');
        if (arguments.length < 2) throw new TypeError("Failed to execute 'parseFromString' on 'DOMParser': 2 arguments required, but only " + arguments.length + ' present.');
        // Nach einer Navigation des Rahmens gilt die Adresse von damals.
        var jetzt = null;
        try { jetzt = z.win.document; } catch (e) {}
        var url = z.url;
        try { if (z.dok && jetzt === z.dok) url = String(z.dok.URL); } catch (e) {}
        var d = parsen(text, typ);
        adresseSetzen(d, url);
        return d;
      };
      try { Object.defineProperty(P.prototype, 'parseFromString', { value: parseFromString, writable: true, configurable: true, enumerable: true }); } catch (e) { P.prototype.parseFromString = parseFromString; }
      try { if (typeof Symbol === 'function' && Symbol.toStringTag) Object.defineProperty(P.prototype, Symbol.toStringTag, { value: 'DOMParser', configurable: true }); } catch (e) {}
      return P;
    };
    var hauptParser = parserBauen(g);
    parserKonstruktoren.set(g, hauptParser);
    try {
      Object.defineProperty(g, 'DOMParser', {
        configurable: true, enumerable: false,
        get: function () {
          var w = this;
          if (w === g || w === null || w === undefined || (typeof w !== 'object' && typeof w !== 'function')) return hauptParser;
          var k = parserKonstruktoren.get(w);
          if (!k) { k = parserBauen(w); parserKonstruktoren.set(w, k); }
          return k;
        },
        set: function (v) {
          try { Object.defineProperty(this, 'DOMParser', { value: v, writable: true, configurable: true, enumerable: false }); } catch (e) {}
        }
      });
    } catch (e) { g.DOMParser = hauptParser; }
  }
  // Release 13.10.2026: die XML-Serialisierung (DOM Parsing §3.2) mit
  // Namensraum-Praefix-Karte, erzeugten Praefixen (`ns1` …) und `<x/>` fuer
  // leere Elemente ausserhalb von HTML — statt `outerHTML`. GEMESSEN vorher
  // (WPT XMLSerializer-serializeToString 3/33, xml-serialization.xhtml
  // 0/17): `<head></head>` statt `<head/>`, Namensraum-Deklarationen
  // fehlten, Kommentare/Doctypes/PIs ergaben "".
  var XS_XMLNS = 'http://www.w3.org/2000/xmlns/', XS_XML = 'http://www.w3.org/XML/1998/namespace', XS_HTML = 'http://www.w3.org/1999/xhtml';
  var XS_LEER = { area: 1, base: 1, basefont: 1, bgsound: 1, br: 1, col: 1, embed: 1, frame: 1, hr: 1, img: 1, input: 1, keygen: 1, link: 1, menuitem: 1, meta: 1, param: 1, source: 1, track: 1, wbr: 1 };
  var xsFehler = function () {
    try { return new g.DOMException('The node could not be serialized as well-formed XML.', 'InvalidStateError'); } catch (e) { return new Error('InvalidStateError'); }
  };
  // XML-`Char` (XML 1.0 §2.2): keine Steuerzeichen ausser Tab/LF/CR, keine
  // einzelnen Ersatzzeichen, kein U+FFFE/U+FFFF.
  var xsZeichenOk = function (s) {
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 0x20 && c !== 9 && c !== 10 && c !== 13) return false;
      if (c === 0xFFFE || c === 0xFFFF) return false;
      if (c >= 0xD800 && c <= 0xDBFF) {
        var n = s.charCodeAt(i + 1);
        if (!(n >= 0xDC00 && n <= 0xDFFF)) return false;
        i++;
      } else if (c >= 0xDC00 && c <= 0xDFFF) return false;
    }
    return true;
  };
  var xsNameOk = function (s) {
    return /^[A-Z_a-zÀ-ÖØ-öø-˿Ͱ-ͽͿ-῿‌‍⁰-↏Ⰰ-⿯、-퟿豈-﷏ﷰ-�\uD800-􏰀-\uDFFF][A-Z_a-zÀ-ÖØ-öø-˿Ͱ-ͽͿ-῿‌‍⁰-↏Ⰰ-⿯、-퟿豈-﷏ﷰ-�\uD800-􏰀-\uDFFF\-.0-9·̀-ͯ‿⁀]*$/.test(s);
  };
  var xsKopie = function (k) { var n = new Map(); k.forEach(function (v, s) { n.set(s, v.slice()); }); return n; };
  var xsHat = function (k, ns, p) { var l = k.get(ns); return !!l && l.indexOf(p) >= 0; };
  var xsDazu = function (k, ns, p) { var l = k.get(ns); if (l) l.push(p); else k.set(ns, [p]); };
  // „retrieving a preferred prefix string": das gewuenschte, sonst das
  // zuletzt hinzugefuegte Praefix des Namensraums.
  var xsBevorzugt = function (k, ns, p) {
    var l = k.get(ns);
    if (!l || !l.length) return null;
    for (var i = 0; i < l.length; i++) if (l[i] === p) return p;
    return l[l.length - 1];
  };
  var xsErzeugen = function (k, ns, z) { var p = 'ns' + z.index; z.index++; xsDazu(k, ns, p); return p; };
  var xsAttrWert = function (v, wohl) {
    if (v === null || v === undefined) return '';
    v = String(v);
    if (wohl && !xsZeichenOk(v)) throw xsFehler();
    // Tab/LF/CR als Zeichenreferenz, damit sie den Rueckweg ueberstehen
    // (wie die Browser, DOM-Parsing issue 59).
    return v.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/\t/g, '&#9;').replace(/\n/g, '&#10;').replace(/\r/g, '&#13;');
  };
  var xsAttribute = function (el) {
    var r = null, l = [];
    try { r = el.__narsilAttributeRoh; } catch (e) {}
    if (!r || typeof r.length !== 'number') r = el.attributes || [];
    for (var i = 0; i < r.length && i < 65536; i++) {
      var a = r[i];
      if (!a) continue;
      var ns = a.namespaceURI || null, q = String(a.name), ln = a.localName ? String(a.localName) : q, p = null;
      if (a.prefix !== undefined) p = a.prefix || null;
      else if (ns !== null && q.indexOf(':') > 0) p = q.slice(0, q.indexOf(':'));
      l.push({ ns: ns, p: p, ln: ln, w: a.value === null || a.value === undefined ? '' : String(a.value) });
    }
    return l;
  };
  var xsIstStd = function (a) { return a.p === null && a.ln === 'xmlns' && (a.ns === XS_XMLNS || a.ns === null); };
  // „recording the namespace information": Praefix-Deklarationen in die
  // Karte, Rueckgabe die Default-Deklaration (`xmlns="…"`). Wie Chromium
  // zaehlt auch ein `xmlns` ohne Namensraum (aus `setAttribute`).
  var xsAufzeichnen = function (attrs, karte, lokal) {
    var std = null;
    for (var i = 0; i < attrs.length; i++) {
      var a = attrs[i];
      if (xsIstStd(a)) { std = a.w; continue; }
      if (a.ns !== XS_XMLNS || a.p === null) continue;
      if (a.w === XS_XML) continue;
      var nsDef = a.w === '' ? null : a.w;
      if (xsHat(karte, nsDef, a.ln)) continue;
      xsDazu(karte, nsDef, a.ln);
      lokal[a.ln] = a.w;
    }
    return std;
  };
  var xsAttributeSchreiben = function (attrs, karte, z, lokal, ignorieren, wohl) {
    var r = '', gesehen = Object.create(null);
    for (var i = 0; i < attrs.length; i++) {
      var a = attrs[i];
      var schl = (a.ns === null ? '' : a.ns) + '\u0000' + a.ln;
      if (wohl && gesehen[schl]) throw xsFehler();
      gesehen[schl] = true;
      var kand = null;
      if (xsIstStd(a)) {
        if (a.w === XS_XML || ignorieren) continue;
        if (wohl && a.ns === null) throw xsFehler();
        r += ' xmlns="' + xsAttrWert(a.w, wohl) + '"';
        continue;
      }
      if (a.ns !== null) {
        kand = xsBevorzugt(karte, a.ns, a.p);
        if (a.ns === XS_XMLNS) {
          if (a.w === XS_XML) continue;
          if (a.p !== null && (!Object.prototype.hasOwnProperty.call(lokal, a.ln) || lokal[a.ln] !== a.w)) continue;
          if (wohl && a.w === XS_XMLNS) throw xsFehler();
          if (wohl && a.w === '') throw xsFehler();
          if (a.p === 'xmlns') kand = 'xmlns';
        } else if (kand === null) {
          kand = xsErzeugen(karte, a.ns, z);
          r += ' xmlns:' + kand + '="' + xsAttrWert(a.ns, wohl) + '"';
        }
      }
      if (wohl && (a.ln.indexOf(':') >= 0 || !xsNameOk(a.ln) || (a.ln === 'xmlns' && a.ns === null))) throw xsFehler();
      r += ' ' + (kand !== null ? kand + ':' : '') + a.ln + '="' + xsAttrWert(a.w, wohl) + '"';
    }
    return r;
  };
  var xsElement = function (el, kontext, karte, z, wohl) {
    var lokalName = String(el.localName);
    if (wohl && (lokalName.indexOf(':') >= 0 || !xsNameOk(lokalName))) throw xsFehler();
    var m = '<', qname = '', ohneEnde = false, ignorieren = false;
    var map = xsKopie(karte), lokal = Object.create(null);
    var attrs = xsAttribute(el);
    var std = xsAufzeichnen(attrs, map, lokal);
    var geerbt = kontext;
    var ns = el.namespaceURI || null;
    if (geerbt === ns) {
      if (std !== null) ignorieren = true;
      qname = ns === XS_XML ? 'xml:' + lokalName : lokalName;
      m += qname;
    } else {
      var praefix = el.prefix || null;
      var kand = xsBevorzugt(map, ns, praefix);
      if (praefix === 'xmlns') { if (wohl) throw xsFehler(); kand = praefix; }
      if (kand !== null) {
        qname = kand + ':' + lokalName;
        if (std !== null && std !== XS_XML) geerbt = std === '' ? null : std;
        m += qname;
      } else if (praefix !== null) {
        if (Object.prototype.hasOwnProperty.call(lokal, praefix)) praefix = xsErzeugen(map, ns, z);
        else xsDazu(map, ns, praefix);
        qname = praefix + ':' + lokalName;
        m += qname + ' xmlns:' + praefix + '="' + xsAttrWert(ns, wohl) + '"';
        if (std !== null) geerbt = std === '' ? null : std;
      } else if (std === null || std !== ns) {
        ignorieren = true;
        qname = lokalName;
        geerbt = ns;
        m += qname + ' xmlns="' + xsAttrWert(ns, wohl) + '"';
      } else {
        qname = lokalName;
        geerbt = ns;
        m += qname;
      }
    }
    m += xsAttributeSchreiben(attrs, map, z, lokal, ignorieren, wohl);
    var leer = !el.firstChild;
    if (ns === XS_HTML && leer && XS_LEER[lokalName] === 1) { m += ' /'; ohneEnde = true; }
    else if (ns !== XS_HTML && leer) { m += '/'; ohneEnde = true; }
    m += '>';
    if (ohneEnde) return m;
    if (ns === XS_HTML && lokalName === 'template' && el.content) m += xsKnoten(el.content, geerbt, map, z, wohl);
    else for (var c = el.firstChild; c; c = c.nextSibling) m += xsKnoten(c, geerbt, map, z, wohl);
    return m + '</' + qname + '>';
  };
  var xsKnoten = function (n, kontext, karte, z, wohl) {
    if (++z.tiefe > 4096) { z.tiefe--; throw new RangeError('XMLSerializer: Baum zu tief'); }
    try {
      var s = '', c;
      switch (n.nodeType) {
        case 1: return xsElement(n, kontext, karte, z, wohl);
        case 9:
          if (wohl && !n.documentElement) throw xsFehler();
          for (c = n.firstChild; c; c = c.nextSibling) s += xsKnoten(c, kontext, karte, z, wohl);
          return s;
        case 11:
          for (c = n.firstChild; c; c = c.nextSibling) s += xsKnoten(c, kontext, karte, z, wohl);
          return s;
        case 8: {
          var d = String(n.data);
          if (wohl && (!xsZeichenOk(d) || d.indexOf('--') >= 0 || d.charAt(d.length - 1) === '-')) throw xsFehler();
          return '<!--' + d + '-->';
        }
        case 4: return '<![CDATA[' + String(n.data) + ']]>';
        case 3: {
          var t = String(n.data);
          if (wohl && !xsZeichenOk(t)) throw xsFehler();
          return t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        }
        case 10: {
          var pub = String(n.publicId || ''), sys = String(n.systemId || '');
          if (wohl && /[^\x20\x0D\x0Aa-zA-Z0-9\-'()+,.\/:=?;!*#@$_%]/.test(pub)) throw xsFehler();
          if (wohl && (!xsZeichenOk(sys) || (sys.indexOf('"') >= 0 && sys.indexOf("'") >= 0))) throw xsFehler();
          var m = '<!DOCTYPE ' + n.name;
          if (pub) m += ' PUBLIC "' + pub + '"';
          if (sys && !pub) m += ' SYSTEM';
          if (sys) m += ' "' + sys + '"';
          return m + '>';
        }
        case 7: {
          var ziel = String(n.target), daten = String(n.data);
          if (wohl && (ziel.indexOf(':') >= 0 || ziel.toLowerCase() === 'xml' || !xsZeichenOk(daten) || daten.indexOf('?>') >= 0)) throw xsFehler();
          return '<?' + ziel + ' ' + daten + '?>';
        }
      }
      return '';
    } finally { z.tiefe--; }
  };
  // „produce an XML serialization" (DOM Parsing §3.2): Kontext null, Karte
  // mit `xml`, Praefixzaehler 1.
  var xmlSerialisieren = function (n, wohl) {
    var karte = new Map();
    xsDazu(karte, XS_XML, 'xml');
    return xsKnoten(n, null, karte, { index: 1, tiefe: 0 }, !!wohl);
  };
  try { Object.defineProperty(g, '__narsilXmlSerialisieren', { value: xmlSerialisieren, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  if (typeof g.XMLSerializer === 'undefined') {
    var XMLSerializer = function XMLSerializer() {
      if (!(this instanceof XMLSerializer)) throw new TypeError("Failed to construct 'XMLSerializer': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    };
    XMLSerializer.prototype.serializeToString = function serializeToString(n) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'serializeToString' on 'XMLSerializer': 1 argument required, but only 0 present.");
      if (!n || typeof n !== 'object' || typeof n.nodeType !== 'number') throw new TypeError("Failed to execute 'serializeToString' on 'XMLSerializer': parameter 1 is not of type 'Node'.");
      return xmlSerialisieren(n, false);
    };
    g.XMLSerializer = XMLSerializer;
  }
  // Release 2026-10-14: `innerHTML`/`outerHTML` in XML documents (DOM
  // Parsing §2.1-§2.3, HTML §13.4 "XML fragment parsing"): reading is the XML
  // serialization with "require well-formed", writing parses with the
  // host's XML parser inside a wrapper that declares every namespace in
  // scope at the context element. The host leaves both to the prelude for
  // XML elements (`dom::xml_element`); for HTML the built-in accessors stay.
  // MEASURED before (WPT domparsing innerhtml-01/-03/-05.xhtml 2/11): HTML
  // serialization (`<br>`, no `xmlns`), no error for malformed markup.
  (function () {
    var E = g.Element && g.Element.prototype;
    if (!E) return;
    var nativeInner = Object.getOwnPropertyDescriptor(E, 'innerHTML') || {};
    var nativeOuter = Object.getOwnPropertyDescriptor(E, 'outerHTML') || {};
    var isXml = function (el) {
      var d = null;
      try { d = el.ownerDocument; } catch (e) {}
      var t = '';
      try { t = d ? String(d.contentType || '') : ''; } catch (e) {}
      return !!t && t !== 'text/html';
    };
    var PARSERERROR_NS = 'http://www.mozilla.org/newlayout/xml/parsererror.xml';
    var syntaxError = function (text) {
      try { return new g.DOMException(text, 'SyntaxError'); } catch (e) { return new SyntaxError(text); }
    };
    var serializeChildren = function (el) {
      var q = el.namespaceURI === XS_HTML && el.localName === 'template' && el.content ? el.content : el, s = '';
      for (var c = q.firstChild; c; c = c.nextSibling) s += xmlSerialisieren(c, true);
      return s;
    };
    // The namespaces in scope at the context element (nearest wins), as
    // attributes of the wrapper element.
    var wrapperAttributes = function (ctx) {
      var prefixes = Object.create(null), attr = '';
      for (var n = ctx, depth = 0; n && n.nodeType === 1 && depth < 4096; n = n.parentNode, depth++) {
        if (n.prefix && n.namespaceURI && !(n.prefix in prefixes)) prefixes[n.prefix] = n.namespaceURI;
        var list = xsAttribute(n);
        for (var i = 0; i < list.length; i++) {
          if (list[i].ns === XS_XMLNS && list[i].p === 'xmlns' && !(list[i].ln in prefixes)) prefixes[list[i].ln] = list[i].w;
        }
      }
      for (var p in prefixes) {
        if (p === 'xml' || p === 'xmlns' || !prefixes[p]) continue;
        attr += ' xmlns:' + p + '="' + xsAttrWert(prefixes[p], false) + '"';
      }
      var defaultNs = null;
      try { defaultNs = ctx.lookupNamespaceURI(null); } catch (e) {}
      if (defaultNs) attr += ' xmlns="' + xsAttrWert(defaultNs, false) + '"';
      return attr;
    };
    var parseFragment = function (ctx, markup) {
      var ownerDoc = ctx.ownerDocument || g.document, parsed;
      try {
        parsed = g.document.__narsilXmlParsen('<narsil-wrapper' + wrapperAttributes(ctx) + '>' + markup + '</narsil-wrapper>', 'application/xml');
      } catch (e) {
        throw syntaxError('Failed to set the markup: the provided markup is not well-formed XML.');
      }
      var wrapper = parsed && parsed.documentElement;
      if (!wrapper || wrapper.namespaceURI === PARSERERROR_NS) throw syntaxError('Failed to set the markup: the provided markup is not well-formed XML.');
      var fragment = ownerDoc.createDocumentFragment();
      while (wrapper.firstChild) fragment.appendChild(ownerDoc.adoptNode(wrapper.firstChild));
      return fragment;
    };
    Object.defineProperty(E, 'innerHTML', {
      configurable: true, enumerable: true,
      get: function () {
        if (!isXml(this) && nativeInner.get) return nativeInner.get.call(this);
        return serializeChildren(this);
      },
      set: function (v) {
        if (!isXml(this) && nativeInner.set) { nativeInner.set.call(this, v); return; }
        var fragment = parseFragment(this, v === null ? '' : String(v));
        var target = this.namespaceURI === XS_HTML && this.localName === 'template' && this.content ? this.content : this;
        target.replaceChildren(fragment);
      }
    });
    Object.defineProperty(E, 'outerHTML', {
      configurable: true, enumerable: true,
      get: function () {
        if (!isXml(this) && nativeOuter.get) return nativeOuter.get.call(this);
        return xmlSerialisieren(this, true);
      },
      set: function (v) {
        if (!isXml(this) && nativeOuter.set) { nativeOuter.set.call(this, v); return; }
        var parent = this.parentNode;
        if (!parent) return;
        if (parent.nodeType === 9) throw new g.DOMException('Failed to set the outerHTML property: the element is a child of a document.', 'NoModificationAllowedError');
        var ctx = parent.nodeType === 11 ? (this.ownerDocument || g.document).createElementNS(XS_HTML, 'body') : parent;
        var fragment = parseFragment(ctx, v === null ? '' : String(v));
        parent.replaceChild(fragment, this);
      }
    });
    // Release 2026-10-15: insertAdjacentHTML in an XML document parses with
    // the XML fragment parser (DOM Parsing §2.2, "insertAdjacentHTML").
    // MEASURED before (WPT domparsing/insert_adjacent_html-xhtml): "<p>"
    // was accepted instead of throwing a SyntaxError.
    var nativeAdjacent = E.insertAdjacentHTML;
    Object.defineProperty(E, 'insertAdjacentHTML', {
      configurable: true, enumerable: true, writable: true,
      value: function insertAdjacentHTML(position, text) {
        if (!isXml(this) && typeof nativeAdjacent === 'function') return nativeAdjacent.apply(this, arguments);
        var where = String(position).replace(/[A-Z]/g, function (c) { return c.toLowerCase(); }), ctx;
        if (where === 'beforebegin' || where === 'afterend') {
          ctx = this.parentNode;
          if (!ctx || ctx.nodeType === 9) throw new g.DOMException("Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent.", 'NoModificationAllowedError');
        } else if (where === 'afterbegin' || where === 'beforeend') {
          ctx = this;
        } else {
          throw syntaxError("Failed to execute 'insertAdjacentHTML' on 'Element': The value provided ('" + String(position) + "') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'.");
        }
        if (ctx.nodeType !== 1) ctx = (this.ownerDocument || g.document).createElementNS(XS_HTML, 'body');
        var fragment = parseFragment(ctx, String(text));
        if (where === 'beforebegin') this.parentNode.insertBefore(fragment, this);
        else if (where === 'afterbegin') this.insertBefore(fragment, this.firstChild);
        else if (where === 'beforeend') this.appendChild(fragment);
        else this.parentNode.insertBefore(fragment, this.nextSibling);
      }
    });
  })();

  // -- XMLHttpRequest (XHR §3) ueber `fetch`, nur asynchron --
  if (typeof g.XMLHttpRequest === 'undefined' && typeof g.fetch === 'function') {
    var XHR = function XMLHttpRequest() {
      this.readyState = 0; this.status = 0; this.statusText = ''; this.responseText = ''; this.response = '';
      this.responseType = ''; this.responseURL = ''; this.responseXML = null; this.timeout = 0; this.withCredentials = false;
      this.onreadystatechange = null; this.onload = null; this.onerror = null; this.onloadend = null; this.onloadstart = null;
      this.onprogress = null; this.onabort = null; this.ontimeout = null;
      this.upload = { addEventListener: function () {}, removeEventListener: function () {}, onprogress: null };
      this._kopf = {}; this._antwortKopf = null; this._horcher = {}; this._abgebrochen = false;
    };
    XHR.UNSENT = 0; XHR.OPENED = 1; XHR.HEADERS_RECEIVED = 2; XHR.LOADING = 3; XHR.DONE = 4;
    var XP = XHR.prototype;
    XP.UNSENT = 0; XP.OPENED = 1; XP.HEADERS_RECEIVED = 2; XP.LOADING = 3; XP.DONE = 4;
    XP._feuern = function (typ) {
      var ev = { type: typ, target: this, currentTarget: this, lengthComputable: false, loaded: 0, total: 0, preventDefault: function () {}, stopPropagation: function () {} };
      var f = this['on' + typ];
      try { if (typeof f === 'function') f.call(this, ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
      var liste = (this._horcher[typ] || []).slice();
      for (var i = 0; i < liste.length; i++) {
        try { liste[i].call(this, ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
      }
    };
    XP._zustand = function (n) { this.readyState = n; this._feuern('readystatechange'); };
    XP.addEventListener = function (typ, f) { if (typeof f === 'function') (this._horcher[typ] = this._horcher[typ] || []).push(f); };
    XP.removeEventListener = function (typ, f) { var l = this._horcher[typ]; if (l) { var i = l.indexOf(f); if (i >= 0) l.splice(i, 1); } };
    XP.open = function (methode, url, asynchron) {
      this._methode = String(methode || 'GET').toUpperCase();
      this._url = String(url);
      this._kopf = {}; this._abgebrochen = false;
      this._zustand(1);
    };
    XP.setRequestHeader = function (n, v) { this._kopf[String(n)] = String(v); };
    XP.overrideMimeType = function () {};
    XP.getResponseHeader = function (n) {
      if (!this._antwortKopf) return null;
      try { var v = this._antwortKopf.get(String(n)); return v === undefined ? null : v; } catch (e) { return null; }
    };
    XP.getAllResponseHeaders = function () {
      if (!this._antwortKopf) return '';
      var raus = '';
      try { this._antwortKopf.forEach(function (v, k) { raus += k + ': ' + v + '\r\n'; }); } catch (e) {}
      return raus;
    };
    XP.abort = function () { this._abgebrochen = true; if (this.readyState > 0 && this.readyState < 4) { this.readyState = 4; this._feuern('readystatechange'); this._feuern('abort'); this._feuern('loadend'); } this.readyState = 0; };
    XP.send = function (koerper) {
      var x = this;
      var init = { method: x._methode, headers: x._kopf, credentials: x.withCredentials ? 'include' : 'same-origin' };
      if (koerper !== undefined && koerper !== null && x._methode !== 'GET' && x._methode !== 'HEAD') init.body = koerper;
      x._feuern('loadstart');
      g.fetch(x._url, init).then(function (antwort) {
        if (x._abgebrochen) return;
        x.status = antwort.status; x.statusText = antwort.statusText || ''; x.responseURL = antwort.url || x._url;
        x._antwortKopf = antwort.headers;
        x._zustand(2);
        x._zustand(3);
        var art = x.responseType;
        var holen = art === 'arraybuffer' && typeof antwort.arrayBuffer === 'function' ? antwort.arrayBuffer() : antwort.text();
        return holen.then(function (inhalt) {
          if (x._abgebrochen) return;
          if (art === 'json') { try { x.response = JSON.parse(inhalt); } catch (e) { x.response = null; } }
          else if (art === 'arraybuffer') { x.response = inhalt; }
          else { x.responseText = inhalt; x.response = inhalt; }
          x._zustand(4);
          x._feuern('load');
          x._feuern('loadend');
        });
      }).catch(function () {
        if (x._abgebrochen) return;
        x.status = 0;
        x._zustand(4);
        x._feuern('error');
        x._feuern('loadend');
      });
    };
    g.XMLHttpRequest = XHR;
    schnittstelle('XMLHttpRequestUpload');
    schnittstelle('XMLHttpRequestEventTarget');
  }

  // -- WeakRef / FinalizationRegistry (ES2021, stark gehalten) --
  def('WeakRef', function WeakRef(ziel) {
    if (ziel === null || (typeof ziel !== 'object' && typeof ziel !== 'function')) throw new TypeError('WeakRef: Ziel muss ein Objekt sein');
    this.deref = function () { return ziel; };
  });
  if (typeof g.FinalizationRegistry === 'undefined') {
    var FR = function FinalizationRegistry(f) { if (typeof f !== 'function') throw new TypeError('FinalizationRegistry braucht eine Funktion'); };
    FR.prototype.register = function () {};
    FR.prototype.unregister = function () { return false; };
    g.FinalizationRegistry = FR;
  }

  // -- FileReader (File API §6) ueber Blob.text()/arrayBuffer() --
  if (typeof g.FileReader === 'undefined') {
    var FileReader = function FileReader() { this.readyState = 0; this.result = null; this.error = null; this.onload = null; this.onloadend = null; this.onerror = null; this._horcher = {}; };
    FileReader.EMPTY = 0; FileReader.LOADING = 1; FileReader.DONE = 2;
    var FP = FileReader.prototype;
    FP.addEventListener = function (t, f) { (this._horcher[t] = this._horcher[t] || []).push(f); };
    FP.removeEventListener = function () {};
    FP._ende = function (typ) {
      this.readyState = 2;
      var ev = { type: typ, target: this };
      var namen = [typ, 'loadend'];
      for (var n = 0; n < namen.length; n++) {
        var f = this['on' + namen[n]];
        try { if (typeof f === 'function') f.call(this, ev); } catch (e) {}
        var l = this._horcher[namen[n]] || [];
        for (var i = 0; i < l.length; i++) { try { l[i].call(this, ev); } catch (e) {} }
      }
    };
    // Release 10.10.2026: Data-URL und Binaertext aus den BYTES (vorher aus
    // `text()` — jedes Bild wurde dabei verdorben).
    function binaer(puffer) {
      var u = new Uint8Array(puffer), s = '', n = 0x8000;
      for (var i = 0; i < u.length; i += n) s += String.fromCharCode.apply(null, Array.prototype.slice.call(u, i, i + n));
      return s;
    }
    FP._lesen = function (blob, art) {
      var r = this; r.readyState = 1;
      var roh = art !== 'text' && blob && typeof blob.arrayBuffer === 'function';
      var p = roh ? blob.arrayBuffer()
        : (blob && typeof blob.text === 'function' ? blob.text() : Promise.resolve(String(blob)));
      p.then(function (v) {
        if (art === 'url') v = 'data:' + ((blob && blob.type) || 'application/octet-stream') + ';base64,' + btoa(roh ? binaer(v) : unescape(encodeURIComponent(v)));
        else if (art === 'binaer') v = roh ? binaer(v) : v;
        r.result = v; r._ende('load');
      }, function (e) { r.error = e; r._ende('error'); });
    };
    FP.readAsText = function (b) { this._lesen(b, 'text'); };
    FP.readAsDataURL = function (b) { this._lesen(b, 'url'); };
    FP.readAsArrayBuffer = function (b) { this._lesen(b, 'buffer'); };
    FP.readAsBinaryString = function (b) { this._lesen(b, 'binaer'); };
    FP.abort = function () {};
    g.FileReader = FileReader;
  }

  // -- PerformanceObserver (Performance Timeline §4): beobachtet nichts --
  if (typeof g.PerformanceObserver === 'undefined') {
    var PO = function PerformanceObserver(f) { this._f = f; };
    PO.prototype.observe = function () {};
    PO.prototype.disconnect = function () {};
    PO.prototype.takeRecords = function () { return []; };
    PO.supportedEntryTypes = [];
    g.PerformanceObserver = PO;
  }

  // -- Audio / Option (HTML §4.8.11, §4.10.10) --
  if (typeof g.Audio === 'undefined' && doc) {
    g.Audio = function Audio(src) { var a = doc.createElement('audio'); if (src !== undefined) a.setAttribute('src', String(src)); return a; };
  }
  if (typeof g.Option === 'undefined' && doc) {
    g.Option = function Option(text, wert, vorgabe, gewaehlt) {
      var o = doc.createElement('option');
      if (text !== undefined) o.textContent = String(text);
      if (wert !== undefined) o.setAttribute('value', String(wert));
      if (vorgabe) o.setAttribute('selected', '');
      // Release 11.10.2026: das vierte Argument ist die AUSWAHL (HTML §4.10.10).
      // Release 14.10.2026: the selectedness is set directly (false too, even
      // with defaultSelected) and the dirtiness stays false, so later
      // changes of the `selected` attribute still apply. MEASURED before (WPT
      // option-element-constructor): `new Option(t, v, true)` was selected,
      // and the constructor made the option dirty.
      if (typeof g.__narsilOptionAuswahl === 'function') g.__narsilOptionAuswahl(o, !!gewaehlt);
      else if (gewaehlt) o.selected = true;
      return o;
    };
  }

  // -- dispatchEvent mit Ereignisweg (DOM §2.9 "dispatch") --------------
  // Release 09.10.2026: an DOM-Knoten laeuft ein aus dem Skript gesendetes
  // Ereignis durch Capture (Fenster -> Ziel), Ziel und Bubble (Ziel ->
  // Fenster); `composed` ueberquert Schattenwurzeln (Ziel wird fuer aussen
  // auf den Wirt umgelenkt), `composedPath()` nennt den Weg, `stop(Immediate)
  // Propagation` und `once` wirken. Die Lauscher liegen beim Host
  // (`__narsilLauscher`). GEMESSEN vorher: kein einziger Lauscher lief —
  // nicht einmal der am Ziel — und `composedPath()` war immer leer.
  (function () {
    var ET = g.EventTarget && g.EventTarget.prototype;
    var hol = g.__narsilLauscher;
    if (!ET || typeof hol !== 'function' || typeof ET.dispatchEvent !== 'function') return;
    var nativ = ET.dispatchEvent;
    var fenster = g.window;
    function istKnoten(x) {
      return x !== null && typeof x === 'object' && typeof x.nodeType === 'number';
    }
    // Host-Objekte haben keine feste Identitaet (jeder Zugriff baut ein neues
    // Objekt) — das Fenster wird darum an seinen Merkmalen erkannt.
    function istFenster(x) {
      return x === g || x === fenster || (x !== null && typeof x === 'object' && !istKnoten(x) &&
        x.document !== undefined && x.location !== undefined && typeof x.setTimeout === 'function');
    }
    // DOM §4.8 "retarget A against B".
    function wurzel(n) { try { return n.getRootNode ? n.getRootNode() : null; } catch (e) { return null; } }
    function istSchattenWurzel(r) { return !!r && r.nodeType === 11 && !!r.host; }
    function imSchatten(n) { return istKnoten(n) && istSchattenWurzel(wurzel(n)); }
    function schattenVorfahr(a, b) {
      for (var x = b, k = 0; x && k < 4096; k++) {
        if (x === a) return true;
        var p = x.parentNode;
        if (!p && x.nodeType === 11 && x.host) p = x.host;
        x = p;
      }
      return false;
    }
    function zielUmlenken(a, b) {
      for (var k = 0; k < 256; k++) {
        if (!istKnoten(a)) return a;
        var r = wurzel(a);
        if (!istSchattenWurzel(r) || (istKnoten(b) && schattenVorfahr(r, b))) return a;
        a = r.host;
      }
      return a;
    }
    function setze(ev, name, wert) {
      try { ev[name] = wert; } catch (e) {}
      if (ev[name] !== wert) {
        try { Object.defineProperty(ev, name, { value: wert, configurable: true, writable: true }); } catch (e2) {}
      }
    }
    function melde(e) {
      try {
        if (typeof g.reportError === 'function') g.reportError(e);
        else if (g.console && g.console.error) g.console.error(e);
      } catch (e2) {}
    }
    // Release 11.10.2026: ein `on*`-Attribut als Handler (HTML §8.1.8.1
    // „getting the current value of the event handler"): der Text ist der
    // Rumpf einer Funktion mit `event` (am Fenster bei `error` mit `event,
    // source, lineno, colno, error`); `return false` bricht ab.
    function anweisung(knoten, quelle, typ) {
      var fehler = typ === 'error' && istFenster(knoten), f;
      try {
        f = fehler ? new Function('event', 'source', 'lineno', 'colno', 'error', quelle) : new Function('event', quelle);
      } catch (e) { melde(e); return null; }
      return function (ev) {
        var r = fehler ? f.call(knoten, ev, '', 0, 0, ev && ev.error) : f.call(knoten, ev);
        if (r === false && ev && typeof ev.preventDefault === 'function') ev.preventDefault();
        return r;
      };
    }
    ET.dispatchEvent = function dispatchEvent(ev) {
      var selbstFenster = istFenster(this);
      if (ev === null || typeof ev !== 'object' || typeof ev.type !== 'string') {
        throw new TypeError("Failed to execute 'dispatchEvent': parameter 1 is not of type 'Event'.");
      }
      // Release 2026-10-15: an event from createEvent() before init*Event()
      // is not initialized (DOM §2.9 step 1); an empty type is fine.
      if (ev.__narsilLaeuft || (typeof g.__narsilEventUninitialized === 'function' && g.__narsilEventUninitialized(ev))) {
        throw new DOMException(ev.__narsilLaeuft ? 'The event is already being dispatched.' : 'The event provided is uninitialized.', 'InvalidStateError');
      }
      // Release 2026-10-15: relatedTarget is retargeted against each object
      // of the path (DOM §2.9 "retarget"), and after the dispatch a target
      // or relatedTarget inside a shadow tree is cleared ("clearTargets").
      // MEASURED before (WPT dom/events/relatedTarget.window 0/6): the
      // shadow root stayed visible as relatedTarget and target.
      var hatRel = Object.prototype.hasOwnProperty.call(ev, 'relatedTarget') || 'relatedTarget' in ev;
      var origRel = hatRel ? ev.relatedTarget : null;
      if (!istKnoten(this) && !selbstFenster) {
        // Release 2026-10-15: `window.event` is the event during the
        // listeners of any EventTarget (HTML §8.1.8). MEASURED before (WPT
        // event-global-event-targets 1/5): undefined for AbortSignal & co.
        if (origRel) setze(ev, 'relatedTarget', zielUmlenken(origRel, this));
        var setzeE = typeof g.__narsilEreignis === 'function' ? g.__narsilEreignis : null;
        var vorher = setzeE ? setzeE(ev) : undefined;
        try { return nativ.call(this, ev); } finally { if (setzeE) setzeE(vorher); }
      }
      var relAmZiel = origRel ? zielUmlenken(origRel, this) : null;
      // "If target is not relatedTarget or target is event's relatedTarget":
      // otherwise no listener runs (the early return), the targets are
      // still cleared.
      var frueh = !!origRel && relAmZiel === this && origRel !== this;
      var composed = !!ev.composed;
      // Der Weg: Ziel, Vorfahren, (Schattenwirt bei `composed`), Dokument, Fenster.
      var weg = [], ziele = [], n = this, umgelenkt = this, schritte = 0;
      while (n && schritte++ < 2048) {
        weg.push(n); ziele.push(umgelenkt);
        if (n.nodeType === 11 && n.host) {
          if (!composed) break;
          n = n.host; umgelenkt = n; continue;
        }
        if (n.nodeType === 9) {
          if (fenster) { weg.push(fenster); ziele.push(umgelenkt); }
          break;
        }
        n = n.parentNode;
      }
      if (selbstFenster) { weg = [fenster || this]; ziele = [fenster || this]; }
      // clearTargets looks at the last struct of the path with a target (the
      // outermost; for a composed event the host) as it was built, not at
      // moves made by the listeners.
      var letzt = ziele.length - 1;
      var endZiel = letzt >= 0 ? ziele[letzt] : this;
      var relEnde = origRel ? zielUmlenken(origRel, letzt >= 0 ? weg[letzt] : this) : null;
      var leeren = imSchatten(endZiel) || imSchatten(relEnde);
      // Release 14.10.2026: a stop flag set before the dispatch (e.g.
      // `ev.cancelBubble = true`) means no listener runs (DOM §2.9 „inner
      // invoke"); the flags are cleared when the dispatch ends.
      var gestoppt = !!ev.__narsilStopp, sofort = false;
      var altStopp = ev.stopPropagation, altSofort = ev.stopImmediatePropagation;
      setze(ev, 'stopPropagation', function stopPropagation() { gestoppt = true; try { altStopp && altStopp.call(ev); } catch (e) {} });
      setze(ev, 'stopImmediatePropagation', function stopImmediatePropagation() { gestoppt = true; sofort = true; try { altSofort && altSofort.call(ev); } catch (e) {} });
      setze(ev, 'composedPath', function composedPath() { return weg.slice(); });
      setze(ev, '__narsilLaeuft', true);
      function rufe(i, capture, phase) {
        var knoten = weg[i];
        var liste;
        try { liste = hol(istFenster(knoten) ? undefined : knoten, ev.type, capture); } catch (e) { liste = null; }
        if (!liste || !liste.length) return;
        setze(ev, 'currentTarget', knoten);
        setze(ev, 'target', ziele[i]);
        if (origRel) setze(ev, 'relatedTarget', zielUmlenken(origRel, knoten));
        setze(ev, 'eventPhase', phase);
        sofort = false;
        // `window.event` (HTML §8.1.8 „current event") waehrend der Lauscher.
        var setzeEreignis = typeof g.__narsilEreignis === 'function' ? g.__narsilEreignis : function () {};
        var altesEreignis = setzeEreignis(ev);
        for (var j = 0; j < liste.length; j++) {
          var f = liste[j], passiv = false;
          // Release 2026-10-15: a passive listener comes as {f, passive}.
          if (f && typeof f === 'object' && typeof f.f === 'function') { passiv = !!f.passive; f = f.f; }
          if (typeof f === 'string') f = anweisung(knoten, f, ev.type);
          if (typeof f !== 'function') continue;
          if (passiv) setze(ev, 'preventDefault', function preventDefault() {});
          try { f.call(knoten, ev); } catch (e) { melde(e); }
          if (passiv) { try { delete ev.preventDefault; } catch (e) {} }
          if (sofort) break;
        }
        setzeEreignis(altesEreignis);
      }
      try {
        var i;
        if (!frueh) {
          for (i = weg.length - 1; i > 0 && !gestoppt; i--) rufe(i, true, 1);
          if (!gestoppt) rufe(0, true, 2);
          if (!gestoppt) rufe(0, false, 2);
          if (ev.bubbles) {
            for (i = 1; i < weg.length && !gestoppt; i++) rufe(i, false, 3);
          }
        }
      } finally {
        setze(ev, '__narsilLaeuft', false);
        try { Object.defineProperty(ev, '__narsilStopp', { value: 0, writable: true, configurable: true }); } catch (e) {}
        setze(ev, 'currentTarget', null);
        setze(ev, 'eventPhase', 0);
        if (leeren) {
          setze(ev, 'target', null);
          if (hatRel) setze(ev, 'relatedTarget', null);
        } else {
          setze(ev, 'target', endZiel);
          if (origRel) setze(ev, 'relatedTarget', relEnde);
        }
        try { delete ev.composedPath; } catch (e) {}
        setze(ev, 'composedPath', function composedPath() { return []; });
      }
      return !ev.defaultPrevented;
    };
  })();

  // -- Event.cancelBubble / returnValue (DOM §2.2, legacy) --
  // Release 14.10.2026: `cancelBubble` reads the stop propagation flag;
  // setting it to true stops propagation (false does nothing).
  // `returnValue` is !canceled; setting false cancels. MEASURED before (WPT
  // dom/events Event-stopImmediatePropagation, Event-cancelBubble):
  // `e.cancelBubble` was undefined.
  (function () {
    var EP = g.Event && g.Event.prototype;
    if (!EP) return;
    function merken(ev, wert) {
      try { Object.defineProperty(ev, '__narsilStopp', { value: wert, writable: true, configurable: true }); } catch (e) {}
    }
    var stopp = EP.stopPropagation, sofort = EP.stopImmediatePropagation;
    if (typeof stopp === 'function') {
      try {
        Object.defineProperty(EP, 'stopPropagation', { configurable: true, writable: true, value: function stopPropagation() {
          if (!this.__narsilStopp) merken(this, 1);
          return stopp.call(this);
        } });
      } catch (e) {}
    }
    if (typeof sofort === 'function') {
      try {
        Object.defineProperty(EP, 'stopImmediatePropagation', { configurable: true, writable: true, value: function stopImmediatePropagation() {
          merken(this, 2);
          return sofort.call(this);
        } });
      } catch (e) {}
    }
    // `initEvent` re-initializes the stop flags (DOM §2.5), except during a
    // dispatch.
    var init = EP.initEvent;
    if (typeof init === 'function') {
      try {
        Object.defineProperty(EP, 'initEvent', { configurable: true, writable: true, value: function initEvent() {
          if (arguments.length < 1) throw new TypeError("Failed to execute 'initEvent' on 'Event': 1 argument required, but only 0 present.");
          // During a dispatch initEvent does nothing (DOM §2.5 step 1).
          if (this.__narsilLaeuft) return;
          merken(this, 0);
          return init.apply(this, arguments);
        } });
      } catch (e) {}
    }
    if (!Object.getOwnPropertyDescriptor(EP, 'cancelBubble')) {
      try {
        Object.defineProperty(EP, 'cancelBubble', { configurable: true, enumerable: true,
          get: function () { return !!this.__narsilStopp; },
          set: function (v) { if (v) this.stopPropagation(); } });
      } catch (e) {}
    }
    // `srcElement` is the legacy alias of `target` (DOM §2.2).
    if (!Object.getOwnPropertyDescriptor(EP, 'srcElement')) {
      try {
        Object.defineProperty(EP, 'srcElement', { configurable: true, enumerable: true,
          get: function () { var t = this.target; return t === undefined ? null : t; } });
      } catch (e) {}
    }
    if (!Object.getOwnPropertyDescriptor(EP, 'returnValue')) {
      try {
        Object.defineProperty(EP, 'returnValue', { configurable: true, enumerable: true,
          get: function () { return !this.defaultPrevented; },
          set: function (v) { if (!v) this.preventDefault(); } });
      } catch (e) {}
    }
  })();

  // -- MutationObserver (DOM §4.3) ----------------------------------------
  // Release 09.10.2026: echte Datensaetze aus dem Aenderungsprotokoll des
  // lebenden DOM (`__narsilMoAn`, Abholung ueber den Reaktionshaken),
  // Zustellung in einem Mikrotask. GEMESSEN vorher: der eingebaute
  // `MutationObserver` meldete nie etwas (youtube.com nutzt ihn 15-mal).
  (function () {
    var an = g.__narsilMoAn;
    if (typeof an !== 'function') return;
    var beobachter = [];
    var geplant = false;
    function liste(a) { a.item = function (i) { return this[i] || null; }; return a; }
    function MutationRecord() {}
    // Release 12.10.2026: die Reihenfolge der Zustellung ist die des
    // Anlegens (DOM §4.3.1 „mutation observers" der Umgebung).
    var naechsteNummer = 0;
    function MutationObserver(cb) {
      if (typeof cb !== 'function') throw new TypeError("Failed to construct 'MutationObserver': parameter 1 is not of type 'MutationCallback'.");
      this._cb = cb; this._reg = []; this._saetze = []; this._nr = naechsteNummer++;
    }
    MutationObserver.prototype.observe = function observe(ziel, o) {
      if (!ziel || typeof ziel.nodeType !== 'number') throw new TypeError("Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.");
      o = o || {};
      var opt = {
        childList: !!o.childList, subtree: !!o.subtree,
        attributes: o.attributes, characterData: o.characterData,
        attributeOldValue: !!o.attributeOldValue, characterDataOldValue: !!o.characterDataOldValue,
        attributeFilter: o.attributeFilter ? Array.prototype.slice.call(o.attributeFilter).map(String) : null
      };
      // Release 12.10.2026: DOM §4.3.1 `observe` Schritte 1–6 („vorhanden"
      // heisst angegeben, auch mit `false`).
      if (opt.attributes === undefined) opt.attributes = o.attributeOldValue !== undefined || o.attributeFilter !== undefined;
      if (opt.characterData === undefined) opt.characterData = o.characterDataOldValue !== undefined;
      opt.attributes = !!opt.attributes; opt.characterData = !!opt.characterData;
      if (!opt.childList && !opt.attributes && !opt.characterData) {
        throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.");
      }
      if (opt.attributeOldValue && !opt.attributes) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.");
      if (opt.attributeFilter && !opt.attributes) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeFilter' when 'attributes' is true or not present.");
      if (opt.characterDataOldValue && !opt.characterData) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.");
      // Eine neue Beobachtung desselben Knotens entfernt dessen fluechtige
      // Registrierungen (DOM §4.3.1 Schritt 7.1).
      var self = this;
      this._reg = this._reg.filter(function (r) { return !(r.fluechtig && r.quelle && r.quelle.ziel === ziel && r.beobachter === self); });
      for (var i = 0; i < this._reg.length; i++) {
        if (this._reg[i].ziel === ziel && !this._reg[i].fluechtig) { this._reg[i].opt = opt; return; }
      }
      this._reg.push({ ziel: ziel, opt: opt });
      if (beobachter.indexOf(this) < 0) beobachter.push(this);
      an(true);
    };
    MutationObserver.prototype.disconnect = function disconnect() {
      this._reg = []; this._saetze = [];
      var i = beobachter.indexOf(this);
      if (i >= 0) beobachter.splice(i, 1);
      if (!beobachter.length) an(false);
    };
    MutationObserver.prototype.takeRecords = function takeRecords() {
      var s = this._saetze; this._saetze = []; return s;
    };
    function innen(ziel, k) {
      var n = k, s = 0;
      while (n && s++ < 4096) { if (n === ziel) return true; n = n.parentNode; }
      return false;
    }
    function zustellen() {
      geplant = false;
      var alle = beobachter.slice().sort(function (a, b) { return a._nr - b._nr; });
      // Fluechtige Registrierungen enden mit der Zustellung (DOM §4.3.3).
      for (var f = 0; f < alle.length; f++) {
        alle[f]._reg = alle[f]._reg.filter(function (r) { return !r.fluechtig; });
      }
      for (var i = 0; i < alle.length; i++) {
        var b = alle[i];
        if (!b._saetze.length) continue;
        var s = b.takeRecords();
        try { b._cb.call(b, s, b); } catch (e) {
          try { if (typeof g.reportError === 'function') g.reportError(e); else g.console.error(e); } catch (e2) {}
        }
      }
    }
    var mikro = typeof g.queueMicrotask === 'function' ? g.queueMicrotask : function (f) { Promise.resolve().then(f); };
    g.__narsilMoSatz = function (r) {
      // [5, typ, ziel, knoten | [hinzu…], hinzu? | [weg…], vorher, nachher, name, alt]
      var typ = r[1], ziel = r[2];
      if (!ziel) return;
      var hinzu = [], weg = [];
      if (typ === 'childList') {
        // Release 12.10.2026: ein gesammelter Satz traegt Listen (DOM §4.2.3).
        if (Array.isArray(r[3])) { hinzu = r[3]; weg = r[4] || []; }
        else if (r[3]) { if (r[4]) hinzu = [r[3]]; else weg = [r[3]]; }
        // Release 2026-10-14: live NodeIterators follow removals (DOM §6.1
        // "pre-removing steps", applied from the record).
        if (weg.length && typeof g.__narsilIteratorRemoval === 'function') {
          try { g.__narsilIteratorRemoval(ziel, weg, r[5] || null, r[6] || null); } catch (e) {}
        }
      }
      for (var i = 0; i < beobachter.length; i++) {
        var b = beobachter[i], passt = false, alterWert = false;
        // Release 12.10.2026: EIN Satz je Beobachter, auch bei mehreren
        // passenden Registrierungen; der alte Wert, wenn eine ihn will
        // (DOM §4.3.2 „queue a mutation record" Schritt 4).
        for (var j = 0; j < b._reg.length; j++) {
          var reg = b._reg[j], o = reg.opt;
          if (typ === 'childList' && !o.childList) continue;
          if (typ === 'attributes' && (!o.attributes || (o.attributeFilter && o.attributeFilter.indexOf(r[7]) < 0))) continue;
          if (typ === 'characterData' && !o.characterData) continue;
          if (!(reg.ziel === ziel || (o.subtree && innen(reg.ziel, ziel)))) continue;
          passt = true;
          if ((typ === 'attributes' && o.attributeOldValue) || (typ === 'characterData' && o.characterDataOldValue)) alterWert = true;
        }
        if (passt) {
          var s = new MutationRecord();
          s.type = typ; s.target = ziel;
          s.addedNodes = liste(hinzu.slice());
          s.removedNodes = liste(weg.slice());
          s.previousSibling = r[5] || null; s.nextSibling = r[6] || null;
          s.attributeName = typ === 'attributes' ? r[7] : null;
          s.attributeNamespace = typ === 'attributes' && r[9] ? r[9] : null;
          s.oldValue = alterWert ? r[8] : null;
          b._saetze.push(s);
          if (!geplant) { geplant = true; mikro(zustellen); }
        }
        // Release 12.10.2026: ein entfernter Knoten bleibt fuer Beobachter
        // mit `subtree` ueber einem Vorfahren beobachtet, bis zur naechsten
        // Zustellung (DOM §4.2.3 remove Schritt 14, fluechtige Registrierung).
        if (weg.length) {
          var neue = [];
          for (var q = 0; q < b._reg.length; q++) {
            var rg = b._reg[q];
            if (!rg.opt.subtree || !(rg.ziel === ziel || innen(rg.ziel, ziel))) continue;
            for (var w = 0; w < weg.length && neue.length < 4096; w++) {
              neue.push({ ziel: weg[w], opt: rg.opt, fluechtig: true, quelle: rg.fluechtig ? rg.quelle : rg, beobachter: b });
            }
          }
          for (var nn = 0; nn < neue.length && b._reg.length < 65536; nn++) b._reg.push(neue[nn]);
          // Das Entfernen stellt die Zustellung in die Warteschlange, auch
          // ohne Datensatz — sonst blieben die fluechtigen Registrierungen.
          if (neue.length && !geplant) { geplant = true; mikro(zustellen); }
        }
      }
    };
    g.MutationObserver = MutationObserver;
    g.WebKitMutationObserver = MutationObserver;
    g.MutationRecord = MutationRecord;
  })();

  // -- IntersectionObserver (Intersection Observer §2/§3) ----------------
  // Release 10.10.2026: echte Schnittflaechen gegen die Ansicht (mit
  // `rootMargin`) bzw. gegen das `root`-Element, gemeldet beim ersten
  // gemessenen Stand und bei jedem Wechsel ueber eine Schwelle (§3.2.8
  // „run the update intersection observations steps"), geprueft alle
  // 100 ms, solange Beobachter da sind. Ein Ziel ohne echten Kasten (nur
  // geschaetzt, siehe `__narsilMass`) wartet auf das Layout.
  // GEMESSEN vorher: jedes Ziel wurde EINMAL gleich nach `observe` als
  // sichtbar gemeldet — mit der Flaeche von DAMALS (oft 0 x 0, das Layout
  // stand noch aus); youtube.com verlangt `intersectionRect.height > 0`
  // und setzte darum kein einziges Vorschaubild.
  (function () {
    var liste = [], geplant = false, MAX_WARTEN = 10;
    function zahl(v, d) { v = +v; return isFinite(v) ? v : d; }
    function rand(text) {
      var t = String(text == null ? '0px' : text).trim().split(/\s+/);
      if (t.length === 1) t = [t[0], t[0], t[0], t[0]];
      else if (t.length === 2) t = [t[0], t[1], t[0], t[1]];
      else if (t.length === 3) t = [t[0], t[1], t[2], t[1]];
      var r = [];
      for (var i = 0; i < 4; i++) {
        var m = /^(-?[0-9.]+)(px|%)?$/.exec(t[i] || '0');
        if (!m) throw new SyntaxError("Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.");
        r.push({ v: zahl(m[1], 0), p: m[2] === '%' });
      }
      return r;
    }
    function IntersectionObserver(cb, opt) {
      if (typeof cb !== 'function') throw new TypeError("Failed to construct 'IntersectionObserver': parameter 1 is not of type 'Function'.");
      opt = opt || {};
      this._cb = cb; this._ziele = []; this._warte = [];
      this.root = opt.root || null;
      this._rand = rand(opt.rootMargin);
      this.rootMargin = this._rand.map(function (x) { return x.v + (x.p ? '%' : 'px'); }).join(' ');
      var t = opt.threshold;
      t = t === undefined ? [0] : (Array.isArray(t) ? t.slice() : [t]);
      for (var i = 0; i < t.length; i++) {
        t[i] = +t[i];
        if (!(t[i] >= 0 && t[i] <= 1)) throw new RangeError("Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1");
      }
      t.sort(function (a, b) { return a - b; });
      this.thresholds = t.length ? t : [0];
    }
    var IP = IntersectionObserver.prototype;
    var mass = g.__narsilMass;
    function kasten(el) {
      var m = null;
      try { m = typeof mass === 'function' ? mass(el) : null; } catch (e) { m = null; }
      if (!m) return null;
      var x = m[0], y = m[1] - m[4], w = m[2], h = m[3];
      return { x: x, y: y, top: y, left: x, right: x + w, bottom: y + h, width: w, height: h, geschaetzt: !!m[5] };
    }
    function rechteck(x, y, w, h) {
      return g.DOMRectReadOnly ? new g.DOMRectReadOnly(x, y, w, h) : { x: x, y: y, width: w, height: h, top: y, left: x, right: x + w, bottom: y + h };
    }
    function wurzel(b) {
      var r;
      if (b.root && b.root.nodeType === 1) {
        r = kasten(b.root);
        if (!r || r.geschaetzt) return null;
      } else {
        var w = g.innerWidth || 1280, h = g.innerHeight || 800;
        r = { top: 0, left: 0, right: w, bottom: h, width: w, height: h };
      }
      var m = b._rand;
      function px(i, bezug) { return m[i].p ? m[i].v * bezug / 100 : m[i].v; }
      var top = r.top - px(0, r.height), right = r.right + px(1, r.width);
      var bottom = r.bottom + px(2, r.height), left = r.left - px(3, r.width);
      return { top: top, left: left, right: right, bottom: bottom, width: right - left, height: bottom - top };
    }
    // Der Schwellen-Index (§3.2.8 Schritt 2.2.10): -1 = schneidet nicht.
    function stufe(b, schneidet, anteil) {
      if (!schneidet) return -1;
      var i = 0;
      while (i < b.thresholds.length && b.thresholds[i] <= anteil) i++;
      return i;
    }
    function pruefen() {
      geplant = false;
      var alle = liste.slice(), jetzt = (g.performance && g.performance.now) ? g.performance.now() : Date.now();
      for (var i = 0; i < alle.length; i++) {
        var b = alle[i], es = [], wb = wurzel(b);
        for (var j = 0; j < b._ziele.length; j++) {
          var z = b._ziele[j], r = kasten(z.el);
          if (!r || r.geschaetzt || !wb) {
            // Noch kein Layout: warten; bleibt es so, einmal „schneidet nicht".
            z.warten++;
            if (z.stand !== null || z.warten < MAX_WARTEN) continue;
            r = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 };
            var leer = true;
          } else {
            leer = false;
          }
          var ix = 0, iy = 0, iw = 0, ih = 0, schneidet = false;
          if (!leer && wb) {
            var l = Math.max(r.left, wb.left), t = Math.max(r.top, wb.top);
            var rr = Math.min(r.right, wb.right), bb = Math.min(r.bottom, wb.bottom);
            schneidet = rr >= l && bb >= t;
            if (schneidet) { ix = l; iy = t; iw = rr - l; ih = bb - t; }
          }
          var flaeche = r.width * r.height;
          var anteil = schneidet ? (flaeche > 0 ? (iw * ih) / flaeche : 1) : 0;
          // Ganz sichtbar ist GENAU 1 (Chrome rechnet in Layout-Einheiten;
          // bei Bruchteilen ergaebe die Division 0,99999998). WPT
          // intersection-ratio-with-fractional-bounds.
          if (schneidet && flaeche > 0 && Math.abs(iw - r.width) < 1e-6 && Math.abs(ih - r.height) < 1e-6) anteil = 1;
          var s = stufe(b, schneidet, anteil);
          if (s === z.stand) continue;
          z.stand = s;
          es.push(new IntersectionObserverEntry({
            target: z.el, time: jetzt, isIntersecting: schneidet, isVisible: schneidet,
            intersectionRatio: anteil,
            boundingClientRect: rechteck(r.left, r.top, r.width, r.height),
            intersectionRect: rechteck(ix, iy, iw, ih),
            rootBounds: wb ? rechteck(wb.left, wb.top, wb.width, wb.height) : null
          }));
        }
        // Release 13.10.2026: die Eintraege warten in der Schlange des
        // Beobachters und gehen als EIGENE Aufgabe hinaus (§3.2.5 „queue an
        // intersection observer task"); `takeRecords` holt sie vorher ab.
        if (es.length) {
          Array.prototype.push.apply(b._warte, es);
          melden(b);
        }
      }
      if (liste.length) plane(100);
    }
    function melden(b) {
      if (b._gemeldet) return;
      b._gemeldet = true;
      setTimeout(function () {
        b._gemeldet = false;
        var es = b._warte;
        b._warte = [];
        if (!es.length || liste.indexOf(b) < 0) return;
        try { b._cb.call(b, es, b); } catch (e) { try { if (typeof g.reportError === 'function') g.reportError(e); else g.console.error(e); } catch (e2) {} }
      }, 0);
    }
    function plane(ms) { if (!geplant) { geplant = true; setTimeout(pruefen, ms); } }
    // Release 13.10.2026: „update the rendering" (HTML §8.1.7.3 Schritt 16):
    // nach den rAF-Rueckrufen eines Bildes werden die Schnitte neu bestimmt
    // — nicht erst beim naechsten 100-ms-Takt. GEMESSEN vorher (WPT
    // intersection-observer, Hilfe `waitForNotification` = rAF, rAF,
    // Timeout): die zweite Meldung nach einem Bildlauf kam zu spaet,
    // „entries.length expected 2 but got 1" in rund 30 Dateien.
    var bildGeplant = false;
    function nachBild() {
      if (bildGeplant || !liste.length) return;
      bildGeplant = true;
      Promise.resolve().then(function () { bildGeplant = false; pruefen(); });
    }
    var altRaf = g.requestAnimationFrame;
    if (typeof altRaf === 'function') {
      g.requestAnimationFrame = function requestAnimationFrame(cb) {
        if (typeof cb !== 'function') return altRaf.apply(this, arguments);
        return altRaf.call(g, function (t) {
          try { return cb.call(this, t); } finally { nachBild(); }
        });
      };
    }
    IP.observe = function observe(el) {
      if (!el || typeof el.nodeType !== 'number') throw new TypeError("Failed to execute 'observe' on 'IntersectionObserver': parameter 1 is not of type 'Element'.");
      for (var i = 0; i < this._ziele.length; i++) if (this._ziele[i].el === el) return;
      this._ziele.push({ el: el, stand: null, warten: 0 });
      if (liste.indexOf(this) < 0) liste.push(this);
      plane(0);
    };
    IP.unobserve = function unobserve(el) {
      for (var i = 0; i < this._ziele.length; i++) if (this._ziele[i].el === el) { this._ziele.splice(i, 1); break; }
      if (!this._ziele.length) { var k = liste.indexOf(this); if (k >= 0) liste.splice(k, 1); }
    };
    IP.disconnect = function disconnect() {
      this._ziele = [];
      var k = liste.indexOf(this); if (k >= 0) liste.splice(k, 1);
    };
    IP.takeRecords = function takeRecords() { var es = this._warte || []; this._warte = []; return es; };
    // Die Eintraege sind `IntersectionObserverEntry` mit den Feldern am
    // Prototyp — daran erkennt das W3C-Polyfill (youtube.com laedt es) einen
    // vollstaendigen eingebauten Beobachter und ersetzt ihn nicht.
    function IntersectionObserverEntry(d) {
      if (d) for (var k in d) this[k] = d[k];
    }
    var EP = IntersectionObserverEntry.prototype;
    EP.time = 0; EP.target = null; EP.rootBounds = null; EP.boundingClientRect = null;
    EP.intersectionRect = null; EP.intersectionRatio = 0; EP.isIntersecting = false; EP.isVisible = false;
    g.IntersectionObserver = IntersectionObserver;
    g.IntersectionObserverEntry = IntersectionObserverEntry;
  })();

  // -- Geometrie aus dem Layout (CSSOM View §6/§7) ------------------------
  // Release 09.10.2026: `getBoundingClientRect`, `offset*`, `client*`,
  // `scroll*` aus den Kaesten des letzten Layouts (`__narsilMass`). Ein
  // Element, das noch nie gelayoutet wurde, hat 0 x 0. GEMESSEN vorher:
  // `offsetWidth` war `undefined`, das Rechteck immer leer.
  (function () {
    var mass = g.__narsilMass;
    var E = g.Element && g.Element.prototype;
    if (typeof mass !== 'function' || !E) return;
    var Rect = g.DOMRect || function (x, y, w, h) { this.x = x; this.y = y; this.width = w; this.height = h; this.left = x; this.top = y; this.right = x + w; this.bottom = y + h; };
    function m(el) {
      // Release 15.10.2026: geometry is a style flush too (animations and
      // transitions of this moment, `__narsilAnimFlush`).
      try { if (typeof g.__narsilAnimFlush === 'function') g.__narsilAnimFlush(el); } catch (e) {}
      try { return mass(el); } catch (e) { return null; }
    }
    E.getBoundingClientRect = function getBoundingClientRect() {
      var r = m(this);
      return r ? new Rect(r[0], r[1] - r[4], r[2], r[3]) : new Rect(0, 0, 0, 0);
    };
    // Release 12.10.2026: eine `DOMRectList` (CSSOM View §6.2); ein
    // gelayouteter Kasten der Groesse 0 ist ein Rechteck (wie Chromium),
    // nur ein Element ohne Kasten hat keins.
    E.getClientRects = function getClientRects() {
      var r = m(this);
      var a = r ? [new Rect(r[0], r[1] - r[4], r[2], r[3])] : [];
      if (g.DOMRectList) { try { Object.setPrototypeOf(a, g.DOMRectList.prototype); } catch (e) {} }
      if (typeof a.item !== 'function') a.item = function (i) { return this[i] || null; };
      return a;
    };
    var H = (g.HTMLElement && g.HTMLElement.prototype) || E;
    function getter(proto, name, f) {
      var d = null;
      try { d = Object.getOwnPropertyDescriptor(proto, name); } catch (e) {}
      if (d && (d.get || d.value !== undefined)) return;
      try { Object.defineProperty(proto, name, { get: f, configurable: true, enumerable: true }); } catch (e) {}
    }
    function zahl(i) { return function () { var r = m(this); return r ? Math.round(r[i]) : 0; }; }
    // Release 11.10.2026: CSSOM View §7 — `offsetParent` (naechster
    // positionierter Vorfahr; bei statischen Elementen auch `td`/`th`/
    // `table`; sonst `body`), `offsetTop`/`offsetLeft` gegen dessen
    // Innenkante, `client*` = Innenabstandsbox (am Wurzelelement die
    // Ansicht), `clientLeft`/`clientTop` = Rahmen, `scroll*` mit dem
    // Ueberlauf der Nachfahren. GEMESSEN vorher: `offsetTop` war die
    // Seitenkoordinate, `offsetParent` fehlte, `clientWidth` war die
    // Rahmenbox (WPT check-layout-Tests in css-grid/flexbox/sizing).
    // Masse: [x, y, b, h, rollen, geschaetzt, rahmen l, o, r, u, position,
    // rechte Kante, untere Kante].
    var html = function () { return doc && doc.documentElement; };
    function offsetEltern(el) {
      var d = el.ownerDocument || doc;
      if (el === d.body || el === d.documentElement) return null;
      var r = m(el);
      if (!r || r[5] === true) return null;
      if (r[10] === 3) return null;
      for (var p = el.parentElement; p; p = p.parentElement) {
        if (p === d.body) return p;
        var rp = m(p);
        if (rp && rp[5] !== true && rp[10] !== 0) return p;
        if (r[10] === 0 && /^(td|th|table)$/.test(p.localName)) return p;
      }
      return d.body || null;
    }
    function offset(achse) {
      return function () {
        var r = m(this);
        if (!r) return 0;
        var p = offsetEltern(this);
        var d = this.ownerDocument || doc;
        if (!p || p === d.body) return Math.round(r[achse]);
        var rp = m(p);
        if (!rp) return Math.round(r[achse]);
        return Math.round(r[achse] - rp[achse] - (achse === 0 ? rp[6] : rp[7]));
      };
    }
    function innen(breit) {
      return function () {
        if (this === html()) return breit ? (g.innerWidth || 0) : (g.innerHeight || 0);
        var r = m(this);
        if (!r) return 0;
        // Release 11.10.2026: die Hauptbox einer Tabelle ist die Huellbox
        // ohne Rahmen (CSS 2 §17.4) — `client*` = `offset*`. GEMESSEN
        // vorher (WPT table-with-border-client-width-height): 42 statt 58.
        if (this.localName === 'table') return Math.round(breit ? r[2] : r[3]);
        return Math.max(0, Math.round(breit ? r[2] - r[6] - r[8] : r[3] - r[7] - r[9]));
      };
    }
    function ueberlauf(breit) {
      return function () {
        var r = m(this);
        var sicht = innen(breit).call(this);
        if (!r) return sicht;
        var anfang = breit ? r[0] + r[6] : r[1] + r[7];
        var ende = breit ? r[11] : r[12];
        if (typeof ende !== 'number' || !isFinite(ende)) return sicht;
        return Math.max(sicht, Math.round(ende - anfang - (this === html() ? 0 : (breit ? r[8] : r[9]))));
      };
    }
    getter(H, 'offsetWidth', zahl(2));
    getter(H, 'offsetHeight', zahl(3));
    getter(H, 'offsetLeft', offset(0));
    getter(H, 'offsetTop', offset(1));
    getter(H, 'offsetParent', function () { return offsetEltern(this); });
    getter(E, 'clientWidth', innen(true));
    getter(E, 'clientHeight', innen(false));
    getter(E, 'scrollWidth', ueberlauf(true));
    getter(E, 'scrollHeight', ueberlauf(false));
    getter(E, 'clientLeft', function () { var r = m(this); return r ? Math.round(r[6] || 0) : 0; });
    getter(E, 'clientTop', function () { var r = m(this); return r ? Math.round(r[7] || 0) : 0; });
  })();

  // -- ResizeObserver (Resize Observer §3) -------------------------------
  // Release 09.10.2026: meldet die Groesse beim Anmelden und jede Aenderung
  // (geprueft alle 250 ms, solange Beobachter da sind — die Groessen kommen
  // aus dem Layout). GEMESSEN vorher: der eingebaute meldete nie.
  (function () {
    var liste = [], geplant = false;
    function ResizeObserver(cb) {
      if (typeof cb !== 'function') throw new TypeError("Failed to construct 'ResizeObserver': parameter 1 is not of type 'ResizeObserverCallback'.");
      this._cb = cb; this._ziele = [];
    }
    var RP = ResizeObserver.prototype;
    function groesse(el) {
      try { var r = el.getBoundingClientRect(); return [r.width || 0, r.height || 0]; } catch (e) { return [0, 0]; }
    }
    function eintrag(el, w, h) {
      var cr = g.DOMRectReadOnly ? new g.DOMRectReadOnly(0, 0, w, h) : { x: 0, y: 0, width: w, height: h, top: 0, left: 0, right: w, bottom: h };
      var s = [{ inlineSize: w, blockSize: h }];
      return { target: el, contentRect: cr, borderBoxSize: s, contentBoxSize: s, devicePixelContentBoxSize: s };
    }
    function pruefen() {
      geplant = false;
      var alle = liste.slice();
      for (var i = 0; i < alle.length; i++) {
        var b = alle[i], es = [];
        for (var j = 0; j < b._ziele.length; j++) {
          var z = b._ziele[j], gr = groesse(z.el);
          if (gr[0] !== z.w || gr[1] !== z.h) { z.w = gr[0]; z.h = gr[1]; es.push(eintrag(z.el, gr[0], gr[1])); }
        }
        if (es.length) {
          try { b._cb.call(b, es, b); } catch (e) { try { g.console.error(e); } catch (e2) {} }
        }
      }
      if (liste.length) plane(250);
    }
    function plane(ms) { if (!geplant) { geplant = true; setTimeout(pruefen, ms); } }
    RP.observe = function observe(el) {
      if (!el || typeof el.nodeType !== 'number') throw new TypeError("Failed to execute 'observe' on 'ResizeObserver': parameter 1 is not of type 'Element'.");
      for (var i = 0; i < this._ziele.length; i++) if (this._ziele[i].el === el) return;
      this._ziele.push({ el: el, w: -1, h: -1 });
      if (liste.indexOf(this) < 0) liste.push(this);
      plane(0);
    };
    RP.unobserve = function unobserve(el) {
      for (var i = 0; i < this._ziele.length; i++) if (this._ziele[i].el === el) { this._ziele.splice(i, 1); break; }
      if (!this._ziele.length) { var k = liste.indexOf(this); if (k >= 0) liste.splice(k, 1); }
    };
    RP.disconnect = function disconnect() {
      this._ziele = [];
      var k = liste.indexOf(this); if (k >= 0) liste.splice(k, 1);
    };
    g.ResizeObserver = ResizeObserver;
    g.ResizeObserverEntry = function ResizeObserverEntry() {};
  })();

  // -- Dateiauswahl (HTML §4.10.5.1.18, File API §4) ---------------------
  // Release 10.10.2026: `input.files` liefert echte `File`-Objekte (Name,
  // Groesse, Typ, Zeit, Inhalt — nie der Pfad); `input.click()` und
  // `showPicker()` auf einem Dateifeld oeffnen den Dialog (die Sitzung prueft
  // die Nutzeraktion). GEMESSEN vorher: `files` war `undefined`, und ein
  // gestalteter Hochlade-Knopf (verstecktes Feld + `click()`) tat nichts.
  (function () {
    var liste = g.__narsilDateien, lies = g.__narsilDateiBytes, dialog = g.__narsilDateiDialog;
    var I = (g.HTMLInputElement && g.HTMLInputElement.prototype) || (g.HTMLElement && g.HTMLElement.prototype);
    if (!I || typeof liste !== 'function') return;
    function istDatei(el) {
      try { return String(el.type || el.getAttribute('type') || '').toLowerCase() === 'file'; } catch (e) { return false; }
    }
    if (typeof g.FileList !== 'function') {
      g.FileList = function FileList() { throw new TypeError('Illegal constructor'); };
    }
    // Die File-Objekte einer Auswahl bleiben gleich, bis sich die Auswahl
    // aendert (Schluessel aus Host-Id und Auswahlstand).
    var gemerkt = {}, reihe = [];
    function dateiListe(el) {
      var roh = null;
      try { roh = liste(el); } catch (e) {}
      if (!roh || !roh.length) {
        var leer = Object.create(g.FileList.prototype);
        leer.length = 0;
        leer.item = function () { return null; };
        return leer;
      }
      var schluessel = roh[0].schluessel;
      if (gemerkt[schluessel]) return gemerkt[schluessel];
      var fl = Object.create(g.FileList.prototype);
      for (var i = 0; i < roh.length; i++) {
        var d = roh[i], inhalt = null;
        try { inhalt = lies(el, i); } catch (e) {}
        var f = new g.File(inhalt ? [inhalt] : [], d.name, { type: d.type || '', lastModified: d.lastModified });
        fl[i] = f;
      }
      fl.length = roh.length;
      fl.item = function (n) { return this[n] || null; };
      if (typeof g.Symbol === 'function' && g.Symbol.iterator) {
        fl[g.Symbol.iterator] = function () { return Array.prototype.slice.call(this, 0, this.length)[g.Symbol.iterator](); };
      }
      gemerkt[schluessel] = fl;
      reihe.push(schluessel);
      // M4: hoechstens vier Auswahlen im Speicher der Seite.
      while (reihe.length > 4) { delete gemerkt[reihe.shift()]; }
      return fl;
    }
    try {
      Object.defineProperty(I, 'files', {
        configurable: true,
        enumerable: true,
        get: function () { return istDatei(this) ? dateiListe(this) : null; },
        set: function () {}
      });
    } catch (e) {}
    // `el.click()` (HTML §6.4.5): ein `click` (bubbles, cancelable), danach
    // — wenn nicht verhindert — das Standardverhalten.
    // Release 11.10.2026: DOM §2.9 „dispatch" mit Aktivierungsverhalten —
    // das INNERSTE aktivierbare Element im Pfad (Ziel oder Vorfahr) ist das
    // Aktivierungsziel; Checkbox/Radio schalten VOR den Lauschern um
    // („legacy-pre-activation behavior") und bei `preventDefault` zurueck;
    // ein `<label>` klickt sein Feld, ein `<summary>` schaltet sein
    // `<details>`. GEMESSEN vorher (WPT Event-dispatch-single-activation-
    // behavior, 121/132 rot): nur das geklickte Element selbst wurde
    // aktiviert, ein Kind eines Formular-Knopfs sendete nie.
    function typVon(el) { return String((el.getAttribute && el.getAttribute('type')) || '').toLowerCase(); }
    function aktivierbar(el) {
      if (!el || el.nodeType !== 1) return false;
      var t = String(el.localName || '').toLowerCase();
      if (el.namespaceURI !== 'http://www.w3.org/1999/xhtml') return false;
      if (t === 'input') return /^(checkbox|radio|submit|image|reset|file|button|color)$/.test(typVon(el));
      if (t === 'button' || t === 'label') return true;
      if ((t === 'a' || t === 'area') && el.hasAttribute('href')) return true;
      if (t === 'summary') {
        var p = el.parentElement;
        if (!p || p.localName !== 'details') return false;
        for (var k = p.firstElementChild; k; k = k.nextElementSibling) if (k.localName === 'summary') return k === el;
        return false;
      }
      return false;
    }
    function aktivierungsZiel(el) {
      for (var n = el; n; n = n.parentNode || n.host || null) {
        if (n.nodeType === 1 && aktivierbar(n)) return n;
      }
      return null;
    }
    // Interaktiver Inhalt (HTML §3.2.5.2.7) — dort tut ein Label nichts.
    function interaktiv(el) {
      var t = String(el.localName || '').toLowerCase();
      if (t === 'a') return el.hasAttribute('href');
      if (t === 'audio' || t === 'video') return el.hasAttribute('controls');
      if (t === 'img') return el.hasAttribute('usemap');
      if (t === 'input') return typVon(el) !== 'hidden';
      return /^(button|details|embed|iframe|label|select|textarea)$/.test(t);
    }
    function vorAktivierung(z) {
      var t = String(z.localName || '').toLowerCase(), typ = typVon(z);
      if (t !== 'input' || z.disabled) return null;
      if (typ === 'checkbox') {
        var war = z.checked, unbestimmt = z.indeterminate;
        try { z.checked = !war; z.indeterminate = false; } catch (e) {}
        var zk = function () { try { z.checked = war; z.indeterminate = unbestimmt; } catch (e) {} };
        zk.geaendert = true;
        return zk;
      }
      if (typ === 'radio') {
        if (z.checked) { var nichts = function () {}; nichts.geaendert = false; return nichts; }
        var vorher = null, name = z.name, f = z.form, wurzel = z.getRootNode ? z.getRootNode() : null;
        if (name && wurzel && wurzel.querySelectorAll) {
          var alle = wurzel.querySelectorAll('input[type=radio i]');
          for (var i = 0; i < alle.length; i++) if (alle[i] !== z && alle[i].name === name && alle[i].form === f && alle[i].checked) vorher = alle[i];
        }
        try { z.checked = true; } catch (e) {}
        var zr = function () { try { z.checked = false; if (vorher) vorher.checked = true; } catch (e) {} };
        zr.geaendert = true;
        return zr;
      }
      return null;
    }
    function aktivieren(z, ev, start, vor) {
      var tag = String(z.localName || '').toLowerCase(), typ = typVon(z);
      if ((tag === 'button' || tag === 'input') && z.disabled) return;
      // Release 2026-10-15: `commandfor`/`command` of a button (HTML
      // §4.10.6 activation behavior) come before `popovertarget`.
      if (tag === 'button' && typeof g.__narsilBefehlAktivierung === 'function') {
        var erledigt = false;
        try { erledigt = g.__narsilBefehlAktivierung(z); } catch (e) {}
        if (erledigt) return;
      }
      // `popovertarget` (HTML §6.12.4) am Knopf.
      if (typeof g.__narsilPopoverAktivierung === 'function') {
        try { g.__narsilPopoverAktivierung(z, ev.target); } catch (e) {}
      }
      if ((tag === 'button' || tag === 'input') && typ === 'reset') {
        var fr = z.form;
        if (fr && typeof fr.reset === 'function') { try { fr.reset(); } catch (e) {} }
        return;
      }
      if (tag === 'label') {
        var c = z.control;
        if (!c || c === start) return;
        for (var n = start; n && n !== z; n = n.parentNode) if (n.nodeType === 1 && interaktiv(n)) return;
        if (typeof c.click === 'function') { try { c.click(); } catch (e) {} }
        return;
      }
      if (tag === 'summary') {
        var det = z.parentElement;
        if (det) { if (det.hasAttribute('open')) det.removeAttribute('open'); else det.setAttribute('open', ''); }
        return;
      }
      if (tag === 'input' && typ === 'file') {
        if (typeof dialog === 'function') dialog(z);
      } else if (tag === 'input' && (typ === 'checkbox' || typ === 'radio')) {
        // Nur bei einem Wechsel (ein schon gewaehltes Radio meldet nichts).
        if (!z.isConnected || (vor && vor.geaendert === false)) return;
        try { z.dispatchEvent(g.__narsilUA(g.Event, 'input', { bubbles: true, composed: true })); } catch (e) {}
        try { z.dispatchEvent(g.__narsilUA(g.Event, 'change', { bubbles: true })); } catch (e) {}
      } else if ((tag === 'a' || tag === 'area') && z.getAttribute('href') !== null) {
        var zielname = String(z.getAttribute('target') || '').toLowerCase();
        var href = z.href;
        if (!href) return;
        // Release 14.10.2026: a hyperlink navigates the browsing context of
        // ITS document, and only while that document is fully active (HTML
        // §4.6.4 „follow the hyperlink"; script navigations really happen
        // since 14.10.). MEASURED before (WPT following-hyperlinks/
        // active-document): a link in a frame's old document navigated the
        // top-level page away.
        var eigenesDok = z.ownerDocument;
        var fenster = g;
        if (eigenesDok && eigenesDok !== g.document) {
          fenster = eigenesDok.defaultView;
          if (!fenster || fenster === g || fenster.document !== eigenesDok) return;
        }
        if (zielname === '_blank' && typeof g.open === 'function') g.open(href);
        else if (fenster.location && typeof fenster.location.assign === 'function') fenster.location.assign(href);
      } else if ((tag === 'button' && z.type === 'submit') || (tag === 'input' && (typ === 'submit' || typ === 'image'))) {
        var f = z.form;
        if (f && typeof f.requestSubmit === 'function') { try { f.requestSubmit(z); } catch (e) {} }
        else if (f && typeof f.submit === 'function') { try { f.submit(); } catch (e) {} }
      }
    }
    // Das Aktivierungsverhalten haengt am Ausloesen JEDES `click`
    // (MouseEvent), auch aus `dispatchEvent` (DOM §2.9 Schritt 5.3/11).
    function mitAktivierung(start, ev, ausloesen) {
      var z = ev.bubbles ? aktivierungsZiel(start) : (aktivierbar(start) ? start : null);
      var vor = z ? vorAktivierung(z) : null;
      var r = ausloesen();
      if (!z) return r;
      if (ev.defaultPrevented) { if (vor) vor(); return r; }
      aktivieren(z, ev, start, vor);
      return r;
    }
    var ETP = g.EventTarget && g.EventTarget.prototype, altAusloesen = ETP && ETP.dispatchEvent;
    if (typeof altAusloesen === 'function') {
      ETP.dispatchEvent = function dispatchEvent(ev) {
        var self = this;
        if (ev && ev.type === 'click' && self && self.nodeType === 1 && g.MouseEvent && ev instanceof g.MouseEvent && !ev.__narsilAktiviert) {
          try { Object.defineProperty(ev, '__narsilAktiviert', { value: true, configurable: true }); } catch (e) {}
          return mitAktivierung(self, ev, function () { return altAusloesen.call(self, ev); });
        }
        return altAusloesen.call(self, ev);
      };
    }
    g.__narsilKlick = function (el) {
      if (!el || el.__narsilKlickLaeuft) return;
      // HTML §6.4.5 `click()`: ein gesperrtes Formularfeld tut nichts.
      var t0 = String(el.localName || '').toLowerCase();
      if (/^(button|input|select|textarea)$/.test(t0) && el.disabled) return;
      try { Object.defineProperty(el, '__narsilKlickLaeuft', { value: true, configurable: true, writable: true }); } catch (e) {}
      try {
        var ev;
        try { ev = new g.MouseEvent('click', { bubbles: true, cancelable: true, composed: true, view: g.window || null }); }
        catch (e) { ev = new g.Event('click', { bubbles: true, cancelable: true, composed: true }); }
        // „fire a synthetic pointer event": nicht vertrauenswuerdig.
        try { Object.defineProperty(ev, 'isTrusted', { value: false, configurable: true, enumerable: true }); } catch (e) {}
        if (g.MouseEvent && ev instanceof g.MouseEvent) { try { el.dispatchEvent(ev); } catch (e) {} }
        else mitAktivierung(el, ev, function () { try { return el.dispatchEvent(ev); } catch (e) { return true; } });
      } finally {
        try { delete el.__narsilKlickLaeuft; } catch (e) {}
      }
    };
    I.showPicker = function showPicker() {
      if (istDatei(this) && typeof dialog === 'function') {
        try { dialog(this); } catch (e) {}
      }
    };
  })();

  // -- new FormData(form [, submitter]) (XHR §5.1 „construct the entry list")
  // Release 10.10.2026: die Felder des Formulars in Dokumentreihenfolge —
  // Text, Haken (nur angehakte), Auswahllisten, Dateien (`input.files`).
  // GEMESSEN vorher: `new FormData(form)` war leer; jedes AJAX-Formular
  // schickte nichts.
  (function () {
    var Nativ = g.FormData;
    if (typeof Nativ !== 'function' || Nativ.__narsilForm) return;
    function FormData(form, submitter) {
      var fd = new Nativ();
      if (form === undefined || form === null) return fd;
      if (typeof form.querySelectorAll !== 'function') {
        throw new TypeError("Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.");
      }
      // Release 15.10.2026: the form's listed elements (HTML §4.10.21.4
      // "constructing the entry list"), including controls outside the form
      // with `form="id"`; `form.elements` is live now. MEASURED before (Agent
      // D, WPT dom/nodes getElementById-namespaced-id): such controls were
      // missing.
      var felder = form.elements && typeof form.elements.length === 'number'
        ? Array.prototype.filter.call(form.elements, function (e) { return /^(input|select|textarea|button)$/i.test(e.localName || ''); })
        : form.querySelectorAll('input, select, textarea, button');
      for (var i = 0; i < felder.length; i++) {
        var el = felder[i];
        var name = el.getAttribute('name');
        if (!name || el.hasAttribute('disabled')) continue;
        var tag = String(el.localName || '').toLowerCase();
        var typ = String(el.getAttribute('type') || '').toLowerCase();
        if (tag === 'button' || (tag === 'input' && (typ === 'submit' || typ === 'image' || typ === 'reset' || typ === 'button'))) {
          if (submitter && el === submitter) fd.append(name, el.value || '');
          continue;
        }
        if (tag === 'input' && (typ === 'checkbox' || typ === 'radio')) {
          if (el.checked) fd.append(name, el.getAttribute('value') !== null ? el.getAttribute('value') : 'on');
          continue;
        }
        if (tag === 'input' && typ === 'file') {
          var dateien = el.files;
          if (dateien && dateien.length) {
            for (var d = 0; d < dateien.length; d++) fd.append(name, dateien[d]);
          } else {
            fd.append(name, new g.File([], '', { type: 'application/octet-stream' }));
          }
          continue;
        }
        if (tag === 'select') {
          var optionen = el.querySelectorAll('option'), gewaehlt = false;
          if (el.hasAttribute('multiple')) {
            for (var o = 0; o < optionen.length; o++) {
              if (optionen[o].hasAttribute('selected')) fd.append(name, optionen[o].getAttribute('value') !== null ? optionen[o].getAttribute('value') : optionen[o].textContent);
            }
          } else {
            fd.append(name, el.value !== undefined ? el.value : '');
          }
          continue;
        }
        fd.append(name, el.value !== undefined && el.value !== null ? el.value : '');
      }
      if (g.__narsilFormCustom) g.__narsilFormCustom(form, fd);
      return fd;
    }
    FormData.prototype = Nativ.prototype;
    try { Object.defineProperty(Nativ.prototype, 'constructor', { value: FormData, configurable: true, writable: true }); } catch (e) {}
    FormData.__narsilForm = true;
    g.FormData = FormData;
  })();
})();

// -- CSSOM (CSSOM §6.1, §6.4): `style.sheet`, `link.sheet`,
// `document.styleSheets`, `insertRule`/`deleteRule`, `cssRules` --
// Release 10.10.2026: styled-components und emotion fuegen ihre Regeln im
// „speedy"-Modus per `insertRule` ein (styled-components entfernt dabei die
// SSR-<style> nach dem Rehydrieren). GEMESSEN vorher auf bbc.com:
// `document.styleSheets` war `undefined`, `style.sheet` ebenso — „kein
// Zugriff auf Undefiniert (Eigenschaft length)" brach die Hydration von
// Next.js ab („A client-side exception has occurred").
// Die Regeln eines <style>-Blatts werden — gebuendelt je Mikrotask — als
// Text in das Element gespiegelt; daraus liest die Kaskade.
// Release 12.10.2026: Neubau auf dem CSSOM-Modul des Motors
// (`narsil_css::cssom::regeln`, Host `__narsilBlatt`/`__narsilRegel`): echte
// Regelarten (CSSImportRule, CSSNamespaceRule, CSSLayerBlockRule,
// CSSScopeRule, CSSContainerRule, CSSNestedDeclarations …), kanonische
// Kopfzeilen und `cssText`, MediaList mit appendMedium/deleteMedium,
// insertRule-Regeln (SyntaxError, HierarchyRequestError, InvalidStateError).
// GEMESSEN vorher (WPT css/cssom): die Regeln wurden im Vorspann per
// Klammerzaehlen zerlegt; `CSSImportRule` war nicht definiert,
// `keyText` blieb `from`, `media.appendMedium` fehlte, `@layer;` galt.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var SE = g.HTMLStyleElement && g.HTMLStyleElement.prototype;
  var LE = g.HTMLLinkElement && g.HTMLLinkElement.prototype;
  var D = g.Document && g.Document.prototype;
  if (!SE || !D || typeof g.WeakMap !== 'function') return;
  var MAX_REGELN = 100000;
  function host(n) { var f = typeof g[n] === 'function' ? g[n] : null; try { delete g[n]; } catch (e) {} return f; }
  var NDEKL = host('__narsilDekl'), NSEL = host('__narsilSelektor'), NBLATT = host('__narsilBlatt');
  var NREGEL = host('__narsilRegel'), NMED = host('__narsilMedien'), NSCHL = host('__narsilSchluessel');
  var NAUS = host('__narsilBlattAus'), NSPIEGEL = host('__narsilBlattSpiegel');
  var blaetter = new WeakMap();
  var TAG = typeof Symbol === 'function' && Symbol.toStringTag;

  function fehler(text, name) {
    try { return new g.DOMException(text, name); } catch (e) { var f = new Error(text); f.name = name; return f; }
  }
  function getter(o, name, f, s) {
    var d = { get: f, configurable: true, enumerable: true };
    if (s) d.set = s;
    try { Object.defineProperty(o, name, d); } catch (e) {}
  }
  function methode(o, name, f) {
    try { Object.defineProperty(o, name, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) { o[name] = f; }
  }
  function intern(o, name, v) {
    try { Object.defineProperty(o, name, { value: v, configurable: true, writable: true, enumerable: false }); } catch (e) { o[name] = v; }
  }
  function klasse(name, eltern) {
    var C = g[name];
    if (typeof C !== 'function' || name === 'CSSStyleSheet') {
      var alt = C;
      C = function () { throw new TypeError('Illegal constructor'); };
      try { Object.defineProperty(C, 'name', { value: name }); } catch (e) {}
      if (alt && alt.prototype) C.prototype = alt.prototype;
      try { g[name] = C; } catch (e) {}
    }
    if (!C.prototype) C.prototype = {};
    try { Object.defineProperty(C.prototype, 'constructor', { value: C, configurable: true, writable: true }); } catch (e) {}
    if (eltern) {
      try { Object.setPrototypeOf(C.prototype, eltern.prototype); } catch (e) {}
      try { Object.setPrototypeOf(C, eltern); } catch (e) {}
    }
    if (TAG) { try { Object.defineProperty(C.prototype, TAG, { value: name, configurable: true }); } catch (e) {} }
    return C;
  }

  // -- Schnittstellen (CSSOM §6.4, CSS Conditional, Cascade 5/6, Nesting) --
  var CSSRule = klasse('CSSRule');
  var CSSGroupingRule = klasse('CSSGroupingRule', CSSRule);
  var CSSConditionRule = klasse('CSSConditionRule', CSSGroupingRule);
  var CSSStyleRule = klasse('CSSStyleRule', CSSGroupingRule);
  var CSSMediaRule = klasse('CSSMediaRule', CSSConditionRule);
  var CSSSupportsRule = klasse('CSSSupportsRule', CSSConditionRule);
  var CSSContainerRule = klasse('CSSContainerRule', CSSConditionRule);
  var CSSImportRule = klasse('CSSImportRule', CSSRule);
  var CSSNamespaceRule = klasse('CSSNamespaceRule', CSSRule);
  var CSSFontFaceRule = klasse('CSSFontFaceRule', CSSRule);
  var CSSPageRule = klasse('CSSPageRule', CSSGroupingRule);
  var CSSMarginRule = klasse('CSSMarginRule', CSSRule);
  var CSSKeyframesRule = klasse('CSSKeyframesRule', CSSRule);
  var CSSKeyframeRule = klasse('CSSKeyframeRule', CSSRule);
  var CSSLayerBlockRule = klasse('CSSLayerBlockRule', CSSGroupingRule);
  var CSSLayerStatementRule = klasse('CSSLayerStatementRule', CSSRule);
  var CSSScopeRule = klasse('CSSScopeRule', CSSGroupingRule);
  var CSSStartingStyleRule = klasse('CSSStartingStyleRule', CSSGroupingRule);
  var CSSPropertyRule = klasse('CSSPropertyRule', CSSRule);
  var CSSCounterStyleRule = klasse('CSSCounterStyleRule', CSSRule);
  var CSSFontFeatureValuesRule = klasse('CSSFontFeatureValuesRule', CSSRule);
  var CSSFontPaletteValuesRule = klasse('CSSFontPaletteValuesRule', CSSRule);
  var CSSPositionTryRule = klasse('CSSPositionTryRule', CSSRule);
  var CSSViewTransitionRule = klasse('CSSViewTransitionRule', CSSRule);
  var CSSNestedDeclarations = klasse('CSSNestedDeclarations', CSSRule);
  var CSSRuleList = klasse('CSSRuleList');
  // Release 12.10.2026: `CSS` ist ein Namensraum-Objekt (CSSOM §8:
  // `[Symbol.toStringTag]` „CSS"), `CSS.supports()` ohne Argument wirft
  // TypeError (WPT CSS-namespace-object-class-string, CSS-supports).
  if (g.CSS && typeof g.CSS.supports === 'function' && !g.CSS.supports.__narsil) {
    var altSupports = g.CSS.supports;
    var supports = function supports(a, b) {
      if (arguments.length === 0) throw new TypeError("Failed to execute 'supports' on 'CSS': 1 argument required, but only 0 present.");
      // Release 12.10.2026: Herstellerpraefixe ausser -webkit- kennt der Motor
      // nur intern (WPT css-transforms transform-computed fragt -moz-transform).
      if (arguments.length >= 2 && /^-(moz|ms|o)-/i.test(String(a))) return false;
      return arguments.length >= 2 ? altSupports.call(g.CSS, String(a), String(b)) : altSupports.call(g.CSS, String(a));
    };
    supports.__narsil = true;
    try { Object.defineProperty(g.CSS, 'supports', { value: supports, configurable: true, writable: true, enumerable: true }); } catch (e) { g.CSS.supports = supports; }
    if (TAG) { try { Object.defineProperty(g.CSS, TAG, { value: 'CSS', configurable: true }); } catch (e) {} }
  }
  var MediaList = klasse('MediaList');
  var StyleSheet = klasse('StyleSheet');
  var StyleSheetList = klasse('StyleSheetList');
  var CSSStyleDeclaration = g.CSSStyleDeclaration;

  var KONST = { STYLE_RULE: 1, CHARSET_RULE: 2, IMPORT_RULE: 3, MEDIA_RULE: 4, FONT_FACE_RULE: 5, PAGE_RULE: 6, KEYFRAMES_RULE: 7, KEYFRAME_RULE: 8, MARGIN_RULE: 9, NAMESPACE_RULE: 10, COUNTER_STYLE_RULE: 11, SUPPORTS_RULE: 12, FONT_FEATURE_VALUES_RULE: 14 };
  for (var kk in KONST) {
    try { Object.defineProperty(CSSRule, kk, { value: KONST[kk], enumerable: true }); } catch (e) {}
    try { Object.defineProperty(CSSRule.prototype, kk, { value: KONST[kk], enumerable: true }); } catch (e) {}
  }
  var TYP = { stil: 1, 'import': 3, media: 4, 'font-face': 5, page: 6, keyframes: 7, keyframe: 8, margin: 9, namespace: 10, 'counter-style': 11, supports: 12, 'font-feature-values': 14 };
  var PROTO = {
    stil: CSSStyleRule.prototype, media: CSSMediaRule.prototype, supports: CSSSupportsRule.prototype,
    container: CSSContainerRule.prototype, 'import': CSSImportRule.prototype, namespace: CSSNamespaceRule.prototype,
    'font-face': CSSFontFaceRule.prototype, page: CSSPageRule.prototype, margin: CSSMarginRule.prototype,
    keyframes: CSSKeyframesRule.prototype, keyframe: CSSKeyframeRule.prototype, 'layer-block': CSSLayerBlockRule.prototype,
    'layer-statement': CSSLayerStatementRule.prototype, scope: CSSScopeRule.prototype,
    'starting-style': CSSStartingStyleRule.prototype, property: CSSPropertyRule.prototype,
    'counter-style': CSSCounterStyleRule.prototype, 'font-feature-values': CSSFontFeatureValuesRule.prototype,
    'font-palette-values': CSSFontPaletteValuesRule.prototype, 'position-try': CSSPositionTryRule.prototype,
    'view-transition': CSSViewTransitionRule.prototype, 'nested-declarations': CSSNestedDeclarations.prototype
  };

  // -- Listen --
  function regelliste() {
    var l = [];
    try { Object.setPrototypeOf(l, CSSRuleList.prototype); } catch (e) { l.item = function (i) { return this[i] || null; }; }
    return l;
  }
  methode(CSSRuleList.prototype, 'item', function (i) { i = Number(i) >>> 0; return i < this.length ? this[i] : null; });
  if (typeof Symbol === 'function' && Symbol.iterator) {
    methode(CSSRuleList.prototype, Symbol.iterator, Array.prototype.values || function () {
      var self = this, i = 0;
      return { next: function () { return i < self.length ? { value: self[i++], done: false } : { value: undefined, done: true }; } };
    });
  }
  ['splice', 'push', 'indexOf', 'slice', 'some', 'forEach', 'map', 'filter'].forEach(function (m) {
    if (Array.prototype[m]) intern(CSSRuleList.prototype, m, Array.prototype[m]);
  });
  try {
    var lg = Object.getOwnPropertyDescriptor(Array.prototype, 'length');
    if (lg && lg.get) getter(CSSRuleList.prototype, 'length', lg.get);
  } catch (e) {}

  function ohneKommentare(s) { return s.indexOf('/*') < 0 ? s : s.replace(/\/\*[\s\S]*?(\*\/|$)/g, ''); }

  // -- Deklarationen (`rule.style`, CSSStyleDeclaration) --
  // Der Block kommt aus dem CSSOM-Modul des Motors (`__narsilDekl`):
  // kanonische Werte, Kurzformen, ungueltige Werte fallen weg. `__b` ist der
  // kodierte Block.
  function dekl(rumpf, roh) {
    return NDEKL ? NDEKL(roh === 'seite' ? 'lesen_seite' : roh === 'kf' ? 'lesen_kf' : roh ? 'lesen_roh' : 'lesen', ohneKommentare(rumpf || '')) : '';
  }
  function bindestrich(n) {
    n = String(n);
    if (n.indexOf('--') === 0) return n;
    if (n === 'cssFloat') return 'float';
    return n.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }).replace(/^(webkit|moz|ms)-/, '-$1-');
  }
  var DEKL = Object.create((CSSStyleDeclaration && CSSStyleDeclaration.prototype) || Object.prototype);
  function textArg(v) { return v === null || v === undefined ? '' : String(v); }
  function blockVon(d) { return d.__r ? d.__r.__b : d.__b; }
  function blockSetzen(d, b) { if (d.__r) d.__r.__b = b; else d.__b = b; }
  methode(DEKL, 'getPropertyValue', function (n) { return NDEKL ? NDEKL('wert', blockVon(this), String(n)) : ''; });
  methode(DEKL, 'getPropertyPriority', function (n) { return NDEKL ? NDEKL('prio', blockVon(this), String(n)) : ''; });
  methode(DEKL, 'setProperty', function (n, v, prio) {
    if (!NDEKL) return;
    var neu = NDEKL('setzen', blockVon(this), String(n), v === undefined ? 'undefined' : textArg(v), textArg(prio));
    if (typeof neu === 'string') { blockSetzen(this, neu); geaendert(this.__r); }
  });
  methode(DEKL, 'removeProperty', function (n) {
    if (!NDEKL) return '';
    var e = NDEKL('entfernen', blockVon(this), String(n));
    if (typeof e[1] === 'string') { blockSetzen(this, e[1]); geaendert(this.__r); }
    return e[0];
  });
  methode(DEKL, 'item', function (i) { return NDEKL ? NDEKL('eintrag', blockVon(this), Number(i) >>> 0) : ''; });
  getter(DEKL, 'length', function () { return NDEKL ? NDEKL('laenge', blockVon(this)) : 0; });
  getter(DEKL, 'parentRule', function () { return this.__r || null; });
  getter(DEKL, 'cssText', function () {
    return NDEKL ? NDEKL('text', blockVon(this)) : '';
  }, function (v) { blockSetzen(this, dekl(String(v), this.__roh)); geaendert(this.__r); });
  ['color', 'background', 'backgroundColor', 'backgroundImage', 'display', 'position', 'top', 'left', 'right', 'bottom',
    'width', 'height', 'minWidth', 'minHeight', 'maxWidth', 'maxHeight', 'margin', 'marginTop', 'marginRight', 'marginBottom',
    'marginLeft', 'padding', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'border', 'borderColor',
    'borderRadius', 'opacity', 'visibility', 'overflow', 'zIndex', 'transform', 'transition', 'animation', 'fontSize',
    'fontWeight', 'fontFamily', 'lineHeight', 'textAlign', 'cursor', 'content', 'flex', 'flexDirection', 'gap',
    'alignItems', 'justifyContent', 'pointerEvents', 'boxShadow', 'outline', 'whiteSpace', 'textDecoration', 'fill',
    'stroke', 'cssFloat'].forEach(function (n) {
    getter(DEKL, n, function () { return this.getPropertyValue(bindestrich(n)); },
      function (v) { this.setProperty(bindestrich(n), v); });
  });
  // Release 12.10.2026: der Block von `@page` ist eine CSSPageDescriptors
  // mit genau ihren Attributen (CSSOM §6.4.8; WPT page-descriptors).
  var CSSPageDescriptors = klasse('CSSPageDescriptors');
  var SEITE = Object.create((CSSStyleDeclaration && CSSStyleDeclaration.prototype) || Object.prototype);
  ['getPropertyValue', 'getPropertyPriority', 'setProperty', 'removeProperty', 'item', 'cssText', 'length', 'parentRule'].forEach(function (n) {
    try { var dd = Object.getOwnPropertyDescriptor(DEKL, n); if (dd) Object.defineProperty(SEITE, n, dd); } catch (e) {}
  });
  ['margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'marginTop', 'marginRight', 'marginBottom', 'marginLeft',
    'size', 'page-orientation', 'pageOrientation', 'marks', 'bleed'].forEach(function (n) {
    getter(SEITE, n, function () { return this.getPropertyValue(bindestrich(n)); }, function (v) { this.setProperty(bindestrich(n), v); });
  });
  if (TAG) { try { Object.defineProperty(SEITE, TAG, { value: 'CSSPageDescriptors', configurable: true }); } catch (e) {} }
  CSSPageDescriptors.prototype = SEITE;
  try { Object.defineProperty(SEITE, 'constructor', { value: CSSPageDescriptors, configurable: true, writable: true }); } catch (e) {}
  // Release 12.10.2026: der Block von `@font-face` (CSS Fonts 4 §12.1
  // CSSFontFaceDescriptors; WPT cssom-fontfacerule-constructors,
  // cssstyledeclaration-cssfontrule).
  var CSSFontFaceDescriptors = klasse('CSSFontFaceDescriptors');
  var SCHRIFT = Object.create(DEKL);
  ['src', 'font-family', 'font-style', 'font-weight', 'font-stretch', 'font-width', 'font-display', 'unicode-range',
    'font-feature-settings', 'font-variation-settings', 'font-named-instance', 'font-language-override', 'ascent-override',
    'descent-override', 'line-gap-override', 'size-adjust', 'subscript-position-override', 'subscript-size-override',
    'superscript-position-override', 'superscript-size-override'].forEach(function (n) {
    var k = n.replace(/-([a-z])/g, function (x, b) { return b.toUpperCase(); });
    [n, k].forEach(function (m) {
      getter(SCHRIFT, m, function () { return this.getPropertyValue(n); }, function (v) { this.setProperty(n, v); });
    });
  });
  if (TAG) { try { Object.defineProperty(SCHRIFT, TAG, { value: 'CSSFontFaceDescriptors', configurable: true }); } catch (e) {} }
  CSSFontFaceDescriptors.prototype = SCHRIFT;
  try { Object.defineProperty(SCHRIFT, 'constructor', { value: CSSFontFaceDescriptors, configurable: true, writable: true }); } catch (e) {}
  function stilVon(regel) {
    if (regel.__s) return regel.__s;
    var seite = regel.__art === 'page' || regel.__art === 'margin';
    var d = Object.create(seite ? SEITE : regel.__art === 'font-face' ? SCHRIFT : DEKL);
    intern(d, '__r', regel);
    intern(d, '__roh', seite ? 'seite' : regel.__art === 'keyframe' ? 'kf' : (regel.__art !== 'stil' && regel.__art !== 'nested-declarations'));
    var s = d;
    if (seite && typeof g.Proxy === 'function') {
      s = new g.Proxy(d, {
        get: function (t, k) {
          if (typeof k === 'string' && /^[0-9]+$/.test(k)) { var n = t.item(Number(k)); return n === '' ? undefined : n; }
          return t[k];
        }
      });
    } else if (typeof g.Proxy === 'function') {
      // `rule.style.color = 'red'` / `rule.style.backgroundColor`.
      s = new g.Proxy(d, {
        get: function (t, k) {
          if (typeof k !== 'string' || k in t) return t[k];
          if (/^[0-9]+$/.test(k)) { var n = t.item(Number(k)); return n === '' ? undefined : n; }
          return t.getPropertyValue(bindestrich(k));
        },
        set: function (t, k, v) {
          if (typeof k === 'string' && !(k in t) && k.charAt(0) !== '_') t.setProperty(bindestrich(k), v);
          else t[k] = v;
          return true;
        }
      });
    }
    intern(regel, '__s', s);
    return s;
  }

  // -- MediaList (CSSOM §4.1) --
  function medienListe(liste, besitzer) {
    var m = Object.create(MediaList.prototype);
    intern(m, '__m', liste || []);
    intern(m, '__o', besitzer || null);
    medienIndizes(m);
    return m;
  }
  function medienIndizes(m) {
    var n = m.__n || 0;
    for (var i = m.__m.length; i < n; i++) { try { delete m[i]; } catch (e) {} }
    for (var j = 0; j < m.__m.length; j++) {
      (function (k) {
        try { Object.defineProperty(m, String(k), { get: function () { return this.__m[k]; }, configurable: true, enumerable: true }); } catch (e) {}
      })(j);
    }
    intern(m, '__n', m.__m.length);
  }
  function medienGeaendert(m) {
    medienIndizes(m);
    var o = m.__o;
    if (o && o.__art) { o.__kopf = m.__m.join(', '); geaendert(o); }
  }
  getter(MediaList.prototype, 'mediaText', function () { return this.__m.join(', '); }, function (v) {
    var t = v === null ? '' : String(v);
    this.__m = NMED ? NMED(t) : (t ? t.split(',') : []);
    medienGeaendert(this);
  });
  getter(MediaList.prototype, 'length', function () { return this.__m.length; });
  methode(MediaList.prototype, 'item', function (i) { i = Number(i) >>> 0; return i < this.__m.length ? this.__m[i] : null; });
  methode(MediaList.prototype, 'appendMedium', function (medium) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'appendMedium' on 'MediaList': 1 argument required, but only 0 present.");
    var q = NMED ? NMED(String(medium)) : [String(medium)];
    if (q.length !== 1) return;
    if (this.__m.indexOf(q[0]) >= 0) return;
    this.__m.push(q[0]);
    medienGeaendert(this);
  });
  methode(MediaList.prototype, 'deleteMedium', function (medium) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'deleteMedium' on 'MediaList': 1 argument required, but only 0 present.");
    var q = NMED ? NMED(String(medium)) : [String(medium)];
    if (q.length !== 1) return;
    var vorher = this.__m.length;
    this.__m = this.__m.filter(function (x) { return x !== q[0]; });
    if (this.__m.length === vorher) throw fehler("Failed to execute 'deleteMedium' on 'MediaList': Failed to delete '" + medium + "'.", 'NotFoundError');
    medienGeaendert(this);
  });
  methode(MediaList.prototype, 'toString', function () { return this.mediaText; });

  // -- Regeln bauen --
  function feld(d, n) { return d && typeof d[n] === 'string' ? d[n] : null; }
  function bauen(d, blatt, eltern) {
    var r = Object.create(PROTO[d.art] || CSSRule.prototype);
    intern(r, '__art', d.art);
    intern(r, '__kopf', d.kopf || '');
    intern(r, '__b', typeof d.block === 'string' ? d.block : null);
    intern(r, '__roh', typeof d.roh === 'string' && d.roh ? d.roh : null);
    intern(r, '__f', d);
    intern(r, '__blatt', blatt || null);
    intern(r, '__eltern', eltern || null);
    intern(r, '__s', null);
    intern(r, '__l', null);
    if (d.kinder) {
      var l = regelliste();
      for (var i = 0; i < d.kinder.length && l.length < MAX_REGELN; i++) l.push(bauen(d.kinder[i], blatt, r));
      intern(r, '__l', l);
    }
    if (d.art === 'media' || d.art === 'import') {
      var ml = feld(d, 'medien');
      intern(r, '__media', medienListe(ml ? ml.split('\x1f') : [], d.art === 'media' ? r : null));
    }
    if (d.art === 'keyframes') schluesselIndizes(r);
    return r;
  }
  function geaendert(r) {
    for (var x = r; x; x = x.__eltern) x.__roh = null;
    if (r && r.__art === 'keyframes') schluesselIndizes(r);
    if (r && r.__blatt) spiegeln(r.__blatt);
  }
  function kinderText(r) {
    var l = r.__l || [], s = ' {\n';
    for (var i = 0; i < l.length; i++) s += einruecken(regelText(l[i])) + '\n';
    return s + '}';
  }
  function einruecken(t) { return String(t).split('\n').map(function (z) { return '  ' + z; }).join('\n'); }
  function dText(r) { return r.__b !== null && NDEKL ? NDEKL('text', r.__b) : ''; }
  function regelText(r) {
    var d, k = r.__kopf;
    switch (r.__art) {
      case 'stil':
        d = dText(r);
        if (!r.__l || r.__l.length === 0) return d ? k + ' { ' + d + ' }' : k + ' { }';
        var s = k + ' {\n';
        if (d) s += '  ' + d + '\n';
        for (var i = 0; i < r.__l.length; i++) s += einruecken(regelText(r.__l[i])) + '\n';
        return s + '}';
      case 'nested-declarations': return dText(r);
      case 'keyframe': d = dText(r); return d ? k + ' { ' + d + ' }' : k + ' { }';
      case 'margin': d = dText(r); return '@' + k + (d ? ' { ' + d + ' }' : ' { }');
      case 'font-feature-values': return merkmalText(r);
      case 'font-face': case 'page': case 'property': case 'counter-style': case 'font-palette-values':
      case 'position-try': case 'view-transition':
        d = dText(r);
        return '@' + r.__art + (k ? ' ' + k : '') + (d ? ' { ' + d + ' }' : ' { }');
      case 'media': return '@media ' + r.__media.mediaText + kinderText(r);
      case 'supports': return '@supports ' + k + kinderText(r);
      case 'container': return '@container ' + k + kinderText(r);
      case 'scope': return '@scope' + (k ? ' ' + k : '') + kinderText(r);
      case 'starting-style': return '@starting-style' + kinderText(r);
      case 'layer-block': return '@layer' + (k ? ' ' + k : '') + kinderText(r);
      case 'layer-statement': return '@layer ' + k + ';';
      case 'keyframes': return '@keyframes ' + kfText(kfName(r)) + kinderText(r);
      case 'import':
        var t = '@import url(' + JSON.stringify(feld(r.__f, 'href') || '') + ')';
        var ly = feld(r.__f, 'layer');
        if (ly !== null) t += ly ? ' layer(' + ly + ')' : ' layer';
        var su = feld(r.__f, 'supports');
        if (su !== null) t += ' supports(' + su + ')';
        if (r.__media.length) t += ' ' + r.__media.mediaText;
        return t + ';';
      case 'namespace':
        var p = feld(r.__f, 'prefix');
        return '@namespace ' + (p ? p + ' ' : '') + 'url(' + JSON.stringify(feld(r.__f, 'uri') || '') + ');';
    }
    return '';
  }

  // -- CSSRule (gemeinsam) --
  getter(CSSRule.prototype, 'type', function () { return TYP[this.__art] || 0; });
  getter(CSSRule.prototype, 'cssText', function () { return regelText(this); }, function () {});
  getter(CSSRule.prototype, 'parentRule', function () { return this.__eltern || null; });
  getter(CSSRule.prototype, 'parentStyleSheet', function () { return this.__blatt || null; });

  // -- Gruppenregeln --
  function ortVon(r) {
    // Release 12.10.2026: unter einer Stilregel schreibt sich `> a` als `& > a`
    // (direkt im @scope nicht; WPT css-cascade/at-scope-relative-syntax).
    for (var x = r; x; x = x.__eltern) {
      if (x.__art === 'stil') return 'verschachtelt_stil';
      if (x.__art === 'scope') return 'verschachtelt';
    }
    return 'gruppe';
  }
  function kinder(r) {
    if (!r.__l) intern(r, '__l', regelliste());
    return r.__l;
  }
  function regelEinfuegen(r, text, index) {
    if (arguments.length < 2) throw new TypeError("Failed to execute 'insertRule' on 'CSSGroupingRule': 1 argument required, but only 0 present.");
    var l = kinder(r);
    index = index === undefined ? 0 : Number(index) >>> 0;
    if (index > l.length) throw fehler("Failed to execute 'insertRule' on 'CSSGroupingRule': The index provided (" + index + ') is larger than the maximum index (' + l.length + ').', 'IndexSizeError');
    var t = String(text);
    if (/^\s*@(import|namespace)\b/i.test(t)) throw fehler("Failed to execute 'insertRule' on 'CSSGroupingRule': '@import' rules cannot be inserted inside a group rule.", 'HierarchyRequestError');
    var d = NREGEL ? NREGEL(t, nsVon(r.__blatt), ortVon(r), false, false) : 'SyntaxError';
    if (typeof d === 'string') throw fehler("Failed to execute 'insertRule' on 'CSSGroupingRule': Failed to parse the rule '" + t + "'.", d);
    l.splice(index, 0, bauen(d, r.__blatt, r));
    geaendert(r);
    return index;
  }
  // Release 12.10.2026: eine entfernte Regel und alle ihre (schon gebauten)
  // Kinder verlieren ihr Blatt (CSSOM §6.4.4 „remove a CSS rule"; WPT
  // CSSStyleSheet-replace-detach-old-rules).
  function abloesen(r) {
    var stapel = [r], n = 0;
    while (stapel.length && n++ < 100000) {
      var x = stapel.pop();
      x.__blatt = null;
      if (x.__l) for (var i = 0; i < x.__l.length; i++) stapel.push(x.__l[i]);
    }
  }
  function regelLoeschen(r, index) {
    var l = kinder(r);
    index = Number(index) >>> 0;
    if (index >= l.length) throw fehler("Failed to execute 'deleteRule' on 'CSSGroupingRule': The index provided (" + index + ') is larger than the maximum index (' + (l.length - 1) + ').', 'IndexSizeError');
    var weg = l.splice(index, 1)[0];
    if (weg) { abloesen(weg); weg.__eltern = null; }
    geaendert(r);
  }
  getter(CSSGroupingRule.prototype, 'cssRules', function () { return kinder(this); });
  methode(CSSGroupingRule.prototype, 'insertRule', function (text, index) {
    return regelEinfuegen.apply(null, [this].concat(Array.prototype.slice.call(arguments)));
  });
  methode(CSSGroupingRule.prototype, 'deleteRule', function (index) { regelLoeschen(this, index); });
  getter(CSSConditionRule.prototype, 'conditionText', function () { return this.__art === 'media' ? this.__media.mediaText : this.__kopf; });

  // CSSStyleRule
  getter(CSSStyleRule.prototype, 'selectorText', function () { return this.__kopf; }, function (v) {
    var o = this.__eltern ? ortVon(this.__eltern) : '';
    var s = selektor(v, this.__blatt, o === 'verschachtelt' || o === 'verschachtelt_stil' ? o : false);
    if (s === null) return;
    this.__kopf = s;
    geaendert(this);
  });
  getter(CSSStyleRule.prototype, 'style', function () { return stilVon(this); }, function (v) { stilVon(this).cssText = v; });
  getter(CSSStyleRule.prototype, 'styleMap', function () { return undefined; });
  getter(CSSNestedDeclarations.prototype, 'style', function () { return stilVon(this); }, function (v) { stilVon(this).cssText = v; });
  getter(CSSMediaRule.prototype, 'media', function () { return this.__media; }, function (v) { this.__media.mediaText = v; });
  getter(CSSMediaRule.prototype, 'matches', function () {
    try { return g.matchMedia ? g.matchMedia(this.__media.mediaText || 'all').matches : true; } catch (e) { return false; }
  });
  getter(CSSSupportsRule.prototype, 'matches', function () {
    try { return g.CSS && g.CSS.supports ? g.CSS.supports(this.__kopf) : false; } catch (e) { return false; }
  });
  getter(CSSContainerRule.prototype, 'containerName', function () { return feld(this.__f, 'name') || ''; });
  getter(CSSContainerRule.prototype, 'containerQuery', function () { return feld(this.__f, 'bedingung') || ''; });
  getter(CSSLayerBlockRule.prototype, 'name', function () { return this.__kopf; });
  getter(CSSLayerStatementRule.prototype, 'nameList', function () {
    var n = feld(this.__f, 'namen'), a = n ? n.split('\x1f') : [];
    try { Object.freeze(a); } catch (e) {}
    return a;
  });
  getter(CSSScopeRule.prototype, 'start', function () { var s = feld(this.__f, 'start'); return s ? s : null; });
  getter(CSSScopeRule.prototype, 'end', function () { var s = feld(this.__f, 'ende'); return s ? s : null; });
  // CSSImportRule
  getter(CSSImportRule.prototype, 'href', function () { return feld(this.__f, 'href') || ''; });
  getter(CSSImportRule.prototype, 'media', function () { return this.__media; }, function (v) { this.__media.mediaText = v; });
  // Release 12.10.2026: das eingebundene Blatt als eigenes CSSStyleSheet
  // (CSSOM §6.4.6): `media` ist dieselbe MediaList wie die der Regel,
  // `parentStyleSheet` das einbindende Blatt, `ownerRule` die Regel; den
  // Inhalt kennt das CSSOM nur bei `data:`-Adressen. GEMESSEN vorher (WPT
  // cssom CSSImportRule-media-identity, cssimportrule-parent,
  // cssimportrule-sheet-identity): `styleSheet` war null.
  getter(CSSImportRule.prototype, 'styleSheet', function () {
    if (!this.__blattImport) {
      var b = Object.create(P);
      intern(b, '__el', null);
      var href = feld(this.__f, 'href') || '', text = '';
      var m = /^data:([^,]*),([\s\S]*)$/i.exec(href);
      if (m) { try { text = /;base64$/i.test(m[1]) ? atob(m[2]) : decodeURIComponent(m[2]); } catch (e) { text = m[2]; } }
      // Sonst aus dem Blattschrank (vom Seitenbau geholt), falls dort.
      else if (typeof g.__narsilBlattText === 'function') {
        try {
          var abs = new URL(href, (this.__blatt && this.__blatt.href) || document.baseURI).href;
          text = g.__narsilBlattText(abs) || '';
          var gz3 = text.lastIndexOf('\uF8FFnarsil-blatt\uF8FF');
          if (gz3 >= 0) text = text.slice(gz3 + 14);
        } catch (e) {}
      }
      intern(b, '__text', text);
      intern(b, '__regeln', null);
      intern(b, '__ownerRule', this);
      intern(b, '__media', this.__media);
      intern(b, '__href', href);
      b.disabled = false;
      intern(this, '__blattImport', b);
    }
    return this.__blattImport;
  });
  getter(CSSImportRule.prototype, 'layerName', function () { return feld(this.__f, 'layer'); });
  getter(CSSImportRule.prototype, 'supportsText', function () { return feld(this.__f, 'supports'); });
  getter(CSSNamespaceRule.prototype, 'namespaceURI', function () { return feld(this.__f, 'uri') || ''; });
  getter(CSSNamespaceRule.prototype, 'prefix', function () { return feld(this.__f, 'prefix') || ''; });
  // Deskriptor-Regeln
  [CSSFontFaceRule, CSSPageRule, CSSMarginRule, CSSPropertyRule, CSSCounterStyleRule, CSSFontPaletteValuesRule, CSSPositionTryRule, CSSViewTransitionRule, CSSKeyframeRule].forEach(function (C) {
    getter(C.prototype, 'style', function () { return stilVon(this); }, function (v) { stilVon(this).cssText = v; });
  });
  getter(CSSPageRule.prototype, 'selectorText', function () { return this.__kopf; }, function (v) {
    var t = String(v).trim();
    // Release 12.10.2026: je Eintrag ein Name und Seiten-Pseudoklassen OHNE
    // Leerraum dazwischen (CSS Paged Media 3 §3.1; WPT cssom-pagerule).
    if (t !== '' && !t.split(',').every(function (x) { return /^(-?[_a-zA-Z\u0080-\uffff][-\w\u0080-\uffff]*)?(:(first|left|right|blank))*$/i.test(x.trim()) && x.trim() !== ''; })) return;
    this.__kopf = t.replace(/:(first|left|right|blank)/gi, function (x) { return x.toLowerCase(); });
    geaendert(this);
  });
  getter(CSSMarginRule.prototype, 'name', function () { return this.__kopf; });
  function deskriptor(r, n) { return r.__b !== null && NDEKL ? NDEKL('wert', r.__b, n) : ''; }
  getter(CSSPropertyRule.prototype, 'name', function () { return this.__kopf; });
  getter(CSSPropertyRule.prototype, 'syntax', function () { return deskriptor(this, 'syntax'); });
  getter(CSSPropertyRule.prototype, 'inherits', function () { return deskriptor(this, 'inherits') === 'true'; });
  getter(CSSPropertyRule.prototype, 'initialValue', function () { var v = deskriptor(this, 'initial-value'); return v === '' ? null : v; });
  getter(CSSCounterStyleRule.prototype, 'name', function () { return this.__kopf; }, function (v) {
    var t = String(v);
    if (!/^-?[_a-zA-Z][-_a-zA-Z0-9]*$/.test(t) || /^(none|decimal|disc|square|circle|disclosure-open|disclosure-closed|inherit|initial|unset|default)$/i.test(t)) return;
    this.__kopf = t;
    geaendert(this);
  });
  ['system', 'symbols', 'additiveSymbols', 'negative', 'prefix', 'suffix', 'range', 'pad', 'speakAs', 'fallback'].forEach(function (n) {
    getter(CSSCounterStyleRule.prototype, n, function () { return deskriptor(this, bindestrich(n)); });
  });
  // Release 12.10.2026: `@font-feature-values` mit seinen Merkmalbloecken als
  // CSSFontFeatureValuesMap (CSS Fonts 4 §6.9.3, maplike<CSSOMString,
  // sequence<unsigned long>>). GEMESSEN vorher (WPT cssom/
  // CSSFontFeatureValuesRule): `rule.annotation` war undefined.
  getter(CSSFontFeatureValuesRule.prototype, 'fontFamily', function () { return this.__kopf; }, function (v) {
    var t = String(v).trim();
    if (!t) return;
    this.__kopf = t;
    geaendert(this);
  });
  var CSSFontFeatureValuesMap = klasse('CSSFontFeatureValuesMap');
  var MERKMALE = { annotation: 'annotation', ornaments: 'ornaments', stylistic: 'stylistic', swash: 'swash', characterVariant: 'character-variant', styleset: 'styleset' };
  function merkmalKarten(r) {
    if (r.__mk) return r.__mk;
    var mk = {};
    for (var n in MERKMALE) {
      var k = Object.create(CSSFontFeatureValuesMap.prototype);
      intern(k, '__m', new Map());
      intern(k, '__r', r);
      mk[MERKMALE[n]] = k;
    }
    var w = feld(r.__f, 'werte') || '';
    var e = w ? w.split('\x1e') : [];
    for (var i = 0; i < e.length; i++) {
      var f = e[i].split('\x1f');
      if (mk[f[0]] && f.length === 3) mk[f[0]].__m.set(f[1], f[2].split(' ').map(Number));
    }
    intern(r, '__mk', mk);
    return mk;
  }
  Object.keys(MERKMALE).forEach(function (n) {
    getter(CSSFontFeatureValuesRule.prototype, n, function () { return merkmalKarten(this)[MERKMALE[n]]; });
  });
  getter(CSSFontFeatureValuesMap.prototype, 'size', function () { return this.__m.size; });
  methode(CSSFontFeatureValuesMap.prototype, 'get', function (k) { var v = this.__m.get(String(k)); return v ? v.slice() : undefined; });
  methode(CSSFontFeatureValuesMap.prototype, 'has', function (k) { return this.__m.has(String(k)); });
  methode(CSSFontFeatureValuesMap.prototype, 'set', function (k, v) {
    var l = Array.isArray(v) ? v : [v];
    this.__m.set(String(k), l.map(function (x) { return Number(x) >>> 0; }));
    geaendert(this.__r);
    return this;
  });
  methode(CSSFontFeatureValuesMap.prototype, 'delete', function (k) { var j = this.__m.delete(String(k)); geaendert(this.__r); return j; });
  methode(CSSFontFeatureValuesMap.prototype, 'clear', function () { this.__m.clear(); geaendert(this.__r); });
  methode(CSSFontFeatureValuesMap.prototype, 'forEach', function (fn, dies) {
    var s = this;
    this.__m.forEach(function (v, k) { fn.call(dies, v.slice(), k, s); });
  });
  methode(CSSFontFeatureValuesMap.prototype, 'keys', function () { return this.__m.keys(); });
  methode(CSSFontFeatureValuesMap.prototype, 'values', function () { return this.__m.values(); });
  methode(CSSFontFeatureValuesMap.prototype, 'entries', function () { return this.__m.entries(); });
  if (typeof Symbol === 'function' && Symbol.iterator) methode(CSSFontFeatureValuesMap.prototype, Symbol.iterator, function () { return this.__m.entries(); });
  function merkmalText(r) {
    var mk = merkmalKarten(r), s = '';
    ['annotation', 'ornaments', 'stylistic', 'swash', 'character-variant', 'styleset'].forEach(function (a) {
      var e = [];
      mk[a].__m.forEach(function (v, k) { e.push(k + ': ' + v.join(' ') + ';'); });
      if (e.length) s += ' @' + a + ' { ' + e.join(' ') + ' }';
    });
    return '@font-feature-values ' + r.__kopf + ' {' + s + ' }';
  }
  getter(CSSFontPaletteValuesRule.prototype, 'name', function () { return this.__kopf; });
  getter(CSSFontPaletteValuesRule.prototype, 'fontFamily', function () { return deskriptor(this, 'font-family'); });
  getter(CSSFontPaletteValuesRule.prototype, 'basePalette', function () { return deskriptor(this, 'base-palette'); });
  getter(CSSFontPaletteValuesRule.prototype, 'overrideColors', function () { return deskriptor(this, 'override-colors'); });
  getter(CSSPositionTryRule.prototype, 'name', function () { return this.__kopf; });
  getter(CSSViewTransitionRule.prototype, 'navigation', function () { return deskriptor(this, 'navigation'); });

  // CSSKeyframesRule / CSSKeyframeRule
  function schluesselIndizes(r) {
    var l = r.__l || [], alt = r.__ki || 0;
    for (var i = l.length; i < alt; i++) { try { delete r[i]; } catch (e) {} }
    for (var j = 0; j < l.length; j++) {
      (function (k) {
        try { Object.defineProperty(r, String(k), { get: function () { return (this.__l || [])[k]; }, configurable: true, enumerable: false }); } catch (e) {}
      })(j);
    }
    intern(r, '__ki', l.length);
  }
  // Release 12.10.2026: `name` ist der Name ohne Anfuehrung; `cssText`
  // schreibt ihn als Bezeichner, ausser `none`/CSS-weite Woerter und
  // Nicht-Bezeichner (als Zeichenkette; WPT CSSKeyframesRule).
  function kfName(r) {
    if (r.__kname !== undefined) return r.__kname;
    var k = r.__kopf || '';
    if (k.length >= 2 && (k[0] === '"' || k[0] === "'") && k[k.length - 1] === k[0]) return k.slice(1, -1).replace(/\\(.)/g, '$1');
    return k;
  }
  function kfText(n) {
    return /^-?[_a-zA-Z\u0080-\uffff][-_a-zA-Z0-9\u0080-\uffff]*$/.test(n) && !/^(none|initial|inherit|unset|default|revert|revert-layer)$/i.test(n) ? n : JSON.stringify(n);
  }
  getter(CSSKeyframesRule.prototype, 'name', function () { return kfName(this); }, function (v) {
    intern(this, '__kname', String(v));
    geaendert(this);
  });
  getter(CSSKeyframesRule.prototype, 'cssRules', function () { return kinder(this); });
  getter(CSSKeyframesRule.prototype, 'length', function () { return kinder(this).length; });
  methode(CSSKeyframesRule.prototype, 'appendRule', function (text) {
    var d = NREGEL ? NREGEL(String(text), '', 'schluesselbilder', false, false) : 'SyntaxError';
    if (typeof d === 'string') return;
    kinder(this).push(bauen(d, this.__blatt, this));
    geaendert(this);
  });
  function schluesselSuchen(r, key) {
    var k = NSCHL ? NSCHL(String(key)) : String(key), l = kinder(r);
    if (k === null) return -1;
    for (var i = l.length - 1; i >= 0; i--) if (l[i].__kopf === k) return i;
    return -1;
  }
  methode(CSSKeyframesRule.prototype, 'findRule', function (key) {
    var i = schluesselSuchen(this, key);
    return i < 0 ? null : this.__l[i];
  });
  methode(CSSKeyframesRule.prototype, 'deleteRule', function (key) {
    var i = schluesselSuchen(this, key);
    if (i < 0) return;
    var weg = this.__l.splice(i, 1)[0];
    if (weg) { abloesen(weg); weg.__eltern = null; }
    geaendert(this);
  });
  getter(CSSKeyframeRule.prototype, 'keyText', function () { return this.__kopf; }, function (v) {
    var k = NSCHL ? NSCHL(String(v)) : String(v);
    if (k === null) throw fehler("Failed to set the 'keyText' property on 'CSSKeyframeRule': The key '" + v + "' is invalid and cannot be parsed", 'SyntaxError');
    this.__kopf = k;
    geaendert(this);
  });

  // -- Selektoren, Namensraeume --
  function nsVon(blatt) {
    var l = (blatt && blatt.__regeln) || [], std = '', p = [];
    for (var i = 0; i < l.length; i++) {
      var r = l[i];
      if (!r || r.__art !== 'namespace') continue;
      var pr = feld(r.__f, 'prefix'), u = feld(r.__f, 'uri') || '';
      if (pr) p.push(pr + '\x1f' + u); else std = u;
    }
    return std + (p.length ? '\x1e' + p.join('\x1e') : '');
  }
  function selektor(text, blatt, relativ) {
    if (!NSEL) return String(text);
    var s = NSEL(String(text), nsVon(blatt), false);
    if (typeof s !== 'string' && relativ && NREGEL) {
      var d = NREGEL(String(text) + '{}', nsVon(blatt), relativ === true ? 'verschachtelt' : relativ, false, false);
      if (typeof d !== 'string' && d && d.art === 'stil') return d.kopf;
    }
    return typeof s === 'string' ? s : null;
  }

  // -- Das Blatt --
  function regelnVon(b) {
    if (!b.__regeln) {
      intern(b, '__regeln', regelliste());
      var t = b.__text || '';
      if (t && NBLATT) {
        var l = NBLATT(t, '');
        for (var i = 0; i < l.length && i < MAX_REGELN; i++) {
          if (b.__konstruiert && l[i].art === 'import') continue;
          b.__regeln.push(bauen(l[i], b, null));
        }
      }
    }
    return b.__regeln;
  }
  var offen = [];
  var spaeter = typeof g.queueMicrotask === 'function' ? function (f) { g.queueMicrotask(f); } : function (f) { Promise.resolve().then(f); };
  function spiegeln(b) {
    if (!b || !b.__el || b.__link || b.__offen) return;
    b.__offen = true;
    offen.push(b);
    if (offen.length === 1) spaeter(abarbeiten);
  }
  function blattText(b) {
    var r = regelnVon(b), t = [];
    for (var j = 0; j < r.length; j++) t.push(r[j].__roh !== null ? r[j].__roh : regelText(r[j]));
    return t.join('\n');
  }
  try { Object.defineProperty(g, '__narsilCssomSpiegeln', { value: function () { if (offen.length) abarbeiten(); }, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  function abarbeiten() {
    var l = offen;
    offen = [];
    for (var i = 0; i < l.length; i++) {
      var b = l[i];
      b.__offen = false;
      var text = blattText(b);
      // Release 13.10.2026: the cascade gets the rules through the host; the
      // element's text stays as written (WPT cssom css-style-reparse).
      var genommen = false;
      if (NSPIEGEL) { try { genommen = NSPIEGEL(b.__el, text) === true; } catch (e) {} }
      if (genommen) continue;
      try { b.__el.textContent = text; } catch (e) {}
      b.__text = text;
      try { b.__n = b.__el.childNodes.length; b.__f = b.__el.firstChild; } catch (e) {}
    }
  }
  var Alt = g.CSSStyleSheet;
  var P = (Alt && Alt.prototype) || {};
  function CSSStyleSheet(opt) {
    if (!(this instanceof CSSStyleSheet)) throw new TypeError("Failed to construct 'CSSStyleSheet': Please use the 'new' operator");
    intern(this, '__el', null);
    intern(this, '__text', '');
    intern(this, '__regeln', null);
    intern(this, '__konstruiert', true);
    var m = opt && opt.media !== undefined ? (typeof opt.media === 'string' ? opt.media : (opt.media && opt.media.mediaText) || '') : '';
    if (opt && opt.baseURL !== undefined) {
      try { new g.URL(String(opt.baseURL), g.location ? g.location.href : undefined); } catch (e) {
        throw fehler("Failed to construct 'CSSStyleSheet': Invalid baseURL '" + opt.baseURL + "'.", 'NotAllowedError');
      }
    }
    intern(this, '__media', medienListe(NMED ? NMED(m) : (m ? [m] : []), null));
    this.disabled = !!(opt && opt.disabled);
  }
  CSSStyleSheet.prototype = P;
  try { Object.defineProperty(P, 'constructor', { value: CSSStyleSheet, configurable: true, writable: true }); } catch (e) {}
  try { Object.defineProperty(CSSStyleSheet, 'name', { value: 'CSSStyleSheet' }); } catch (e) {}
  try { Object.setPrototypeOf(P, StyleSheet.prototype); } catch (e) {}
  if (TAG) { try { Object.defineProperty(P, TAG, { value: 'CSSStyleSheet', configurable: true }); } catch (e) {} }
  g.CSSStyleSheet = CSSStyleSheet;
  // Release 12.10.2026: ein fremdes oder nicht geladenes Link-Blatt ist nicht
  // „origin-clean" — cssRules/insertRule/deleteRule werfen SecurityError
  // (CSSOM §6.1.2; WPT cssom stylesheet-same-origin).
  function sauberPruefen(b, m) {
    if (b && b.__unsauber) throw fehler("Failed to " + m + " on 'CSSStyleSheet': Cannot access rules", 'SecurityError');
  }
  getter(P, 'cssRules', function () { sauberPruefen(this, "read the 'cssRules' property from 'CSSStyleSheet'"); return regelnVon(this); });
  getter(P, 'rules', function () { sauberPruefen(this, "read the 'rules' property from 'CSSStyleSheet'"); return regelnVon(this); });
  // Release 12.10.2026: ein entferntes `<style>`/`<link>` gibt sein Blatt auf
  // (CSSOM §6.1 „remove a CSS style sheet"; WPT StyleSheet-properties).
  getter(P, 'ownerNode', function () {
    if (!this.__el || this.__adoptiert) return null;
    var weg = false;
    try { weg = this.__el.isConnected === false; } catch (e) {}
    // Release 14.10.2026: a link that got `disabled` no longer owns its
    // sheet (HTML §4.6.7; WPT cssom HTMLLinkElement-disabled-001/002).
    try { weg = weg || (this.__link && this.__el.hasAttribute('disabled')); } catch (e) {}
    if (weg) { if (blaetter.get(this.__el) === this) blaetter.delete(this.__el); intern(this, '__el', null); return null; }
    return this.__el;
  });
  getter(P, 'ownerRule', function () { return this.__ownerRule || null; });
  getter(StyleSheet.prototype, 'parentStyleSheet', function () { return this.__ownerRule ? this.__ownerRule.__blatt || null : null; });
  getter(StyleSheet.prototype, 'type', function () { return 'text/css'; });
  getter(StyleSheet.prototype, 'href', function () {
    if (this.__ownerRule) { try { return new URL(this.__href, document.baseURI).href; } catch (e) { return this.__href || null; } }
    if (!this.__link) return null;
    try { return this.__el.href || null; } catch (e) { return null; }
  });
  getter(StyleSheet.prototype, 'title', function () {
    if (this.__konstruiert) return null;
    try { var t = this.__el && this.__el.getAttribute('title'); return t === null || t === undefined || t === '' ? null : t; } catch (e) { return null; }
  });
  getter(StyleSheet.prototype, 'media', function () {
    if (!this.__media) {
      var t = '';
      try { t = (this.__el && this.__el.getAttribute('media')) || ''; } catch (e) {}
      intern(this, '__media', medienListe(NMED ? NMED(t) : (t ? [t] : []), null));
    }
    return this.__media;
  }, function (v) { this.media.mediaText = v; });
  // Release 12.10.2026: `disabled` ist ein Attribut am Prototyp, und
  // HTMLStyleElement.disabled spiegelt das Blatt (CSSOM §6.1, HTML §4.2.6;
  // WPT style-sheet-interfaces-001).
  // Release 13.10.2026: a disabled sheet leaves the cascade (`__narsilBlattAus`).
  getter(StyleSheet.prototype, 'disabled', function () { return !!this.__disabled; }, function (v) {
    intern(this, '__disabled', !!v);
    if (NAUS && this.__el && !this.__link) { try { NAUS(this.__el, !!v); } catch (e) {} }
  });
  getter(SE, 'disabled', function () { var b = blaetter.get(this); return !!(b && b.__disabled); }, function (v) {
    var b = blaetter.get(this);
    if (b) b.disabled = v;
  });
  function ordnungVor(l, index, erlaubt) {
    for (var i = 0; i < index && i < l.length; i++) if (erlaubt.indexOf(l[i].__art) < 0) return true;
    return false;
  }
  P.insertRule = function (text, index) {
    sauberPruefen(this, "execute 'insertRule'");
    if (arguments.length < 1) throw new TypeError("Failed to execute 'insertRule' on 'CSSStyleSheet': 1 argument required, but only 0 present.");
    var l = regelnVon(this);
    index = index === undefined ? 0 : Number(index) >>> 0;
    if (index > l.length) throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': The index provided (" + index + ') is larger than the maximum index (' + l.length + ').', 'IndexSizeError');
    if (l.length >= MAX_REGELN) throw fehler('Zu viele Regeln im Stilblatt', 'QuotaExceededError');
    var t = String(text);
    var d = NREGEL ? NREGEL(t, nsVon(this), 'blatt', ordnungVor(l, index, ['import', 'layer-statement']), ordnungVor(l, index, ['import', 'namespace', 'layer-statement'])) : 'SyntaxError';
    if (typeof d === 'string') throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '" + t + "'.", d);
    if (d.art === 'import' && this.__konstruiert) throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': Can't insert @import rules into a constructed stylesheet.", 'SyntaxError');
    var rest = l.slice(index);
    if (d.art !== 'import' && d.art !== 'layer-statement' && rest.some(function (x) { return x.__art === 'import'; })) {
      throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to insert the rule.", 'HierarchyRequestError');
    }
    if (d.art !== 'import' && d.art !== 'namespace' && d.art !== 'layer-statement' && rest.some(function (x) { return x.__art === 'namespace'; })) {
      throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to insert the rule.", 'HierarchyRequestError');
    }
    if (d.art === 'namespace' && l.some(function (x) { return x.__art !== 'import' && x.__art !== 'namespace'; })) {
      throw fehler("Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to insert the rule.", 'InvalidStateError');
    }
    l.splice(index, 0, bauen(d, this, null));
    spiegeln(this);
    return index;
  };
  P.deleteRule = function (index) {
    sauberPruefen(this, "execute 'deleteRule'");
    if (arguments.length < 1) throw new TypeError("Failed to execute 'deleteRule' on 'CSSStyleSheet': 1 argument required, but only 0 present.");
    var l = regelnVon(this);
    index = Number(index) >>> 0;
    if (index >= l.length) throw fehler("Failed to execute 'deleteRule' on 'CSSStyleSheet': The index provided (" + index + ') is larger than the maximum index (' + (l.length - 1) + ').', 'IndexSizeError');
    if (l[index].__art === 'namespace' && l.some(function (x) { return x.__art !== 'import' && x.__art !== 'namespace'; })) {
      throw fehler("Failed to execute 'deleteRule' on 'CSSStyleSheet': Failed to delete rule.", 'InvalidStateError');
    }
    var weg = l.splice(index, 1)[0];
    if (weg) { abloesen(weg); weg.__eltern = null; }
    spiegeln(this);
  };
  P.addRule = function (sel, stil, index) {
    var l = regelnVon(this);
    this.insertRule(String(sel) + ' { ' + String(stil === undefined ? '' : stil) + ' }', index === undefined ? l.length : index);
    return -1;
  };
  P.removeRule = function (index) { this.deleteRule(index === undefined ? 0 : index); };
  P.replaceSync = function (text) {
    if (!this.__konstruiert) throw fehler("Failed to execute 'replaceSync' on 'CSSStyleSheet': Can't call replaceSync on non-constructed CSSStyleSheets.", 'NotAllowedError');
    var l = regelnVon(this);
    for (var i = 0; i < l.length; i++) { abloesen(l[i]); l[i].__eltern = null; }
    l.splice(0, l.length);
    this.__text = String(text);
    var neu = NBLATT ? NBLATT(this.__text, '') : [];
    for (var j = 0; j < neu.length && l.length < MAX_REGELN; j++) {
      if (neu[j].art === 'import') continue;
      l.push(bauen(neu[j], this, null));
    }
    spiegeln(this);
  };
  P.replace = function (text) {
    try { this.replaceSync(text); } catch (e) { return Promise.reject(e); }
    return Promise.resolve(this);
  };

  function blattFuer(el, link) {
    try {
      if (el.isConnected === false) {
        var alt = blaetter.get(el);
        if (alt) { intern(alt, '__el', null); blaetter.delete(el); }
        return null;
      }
    } catch (e) {}
    var b = blaetter.get(el);
    if (link) {
      var rel = '';
      try { rel = el.getAttribute('rel') || ''; } catch (e) {}
      if (!/(^|\s)stylesheet(\s|$)/i.test(rel)) return null;
      // Release 12.10.2026: ein Link-Blatt gibt es erst, wenn es geladen ist,
      // und nicht mit `disabled`; seine Regeln kommen aus dem Blattschrank
      // (`__narsilBlattText`; HTML §4.2.4, CSSOM §6.1). GEMESSEN vorher:
      // `link.sheet.cssRules` war leer.
      try {
        if (el.hasAttribute('disabled')) {
          // Ein frueheres Blatt verliert seinen Besitzer (WPT HTMLLinkElement-disabled-002).
          if (b) { intern(b, '__el', null); blaetter.delete(el); }
          return null;
        }
      } catch (e) {}
      var u = null;
      try { u = el.href || null; } catch (e) {}
      var st = 1;
      // Release 12.10.2026: ein `data:`-Blatt steht sofort bereit (sein Inhalt
      // ist die Adresse). GEMESSEN vorher (WPT dom/nodes insertion-removing-steps
      // Node-appendChild-script-and-style): ein im selben appendChild davor
      // eingefuegtes Skript sah `link.sheet === null`.
      var daten = u ? /^data:([^,]*),([\s\S]*)$/i.exec(u) : null;
      try { if (u && !daten && typeof g.__narsilBlattStand === 'function') st = g.__narsilBlattStand(u); } catch (e) {}
      // Noch offen: kein Blatt; ein Fehler (404) gibt ein leeres Blatt (wie
      // die Browser; WPT ttwf-cssom-doc-ext-load-tree-order).
      if (st === 0) return null;
      if (!b || b.__url !== u) {
        b = Object.create(P);
        intern(b, '__el', el);
        intern(b, '__link', true);
        intern(b, '__url', u);
        var tx = '';
        if (daten) {
          try { tx = /;base64$/i.test(daten[1]) ? atob(daten[2]) : decodeURIComponent(daten[2]); } catch (e) { tx = daten[2]; }
        }
        try { if (!daten && st === 1 && u && typeof g.__narsilBlattText === 'function') tx = g.__narsilBlattText(u) || ''; } catch (e) {}
        var gz2 = tx.lastIndexOf('\uF8FFnarsil-blatt\uF8FF');
        if (gz2 >= 0) tx = tx.slice(gz2 + 14);
        intern(b, '__text', tx);
        intern(b, '__regeln', null);
        intern(b, '__offen', false);
        var fremd = false;
        try { fremd = !daten && new URL(u, document.baseURI).origin !== new URL(document.URL).origin; } catch (e) {}
        intern(b, '__unsauber', !daten && (st !== 1 || fremd));
        b.disabled = false;
        blaetter.set(el, b);
      }
      return b;
    }
    var typ = null;
    try { typ = el.getAttribute('type'); } catch (e) {}
    if (typ && !/^\s*text\/css\s*$/i.test(typ)) return null;
    var n = 0, f = null, tx = '';
    try { n = el.childNodes.length; f = el.firstChild; tx = el.textContent || ''; } catch (e) {}
    // Release 12.10.2026: eingesetzte `@import`-Blaetter stehen vor der
    // Grenze; das Blatt selbst ist das letzte Stueck.
    var gz = tx.lastIndexOf('\uF8FFnarsil-blatt\uF8FF');
    if (gz >= 0) tx = tx.slice(gz + 14);
    // Release 12.10.2026: auch `text.data = …` aendert das Blatt (WPT
    // selectorSerialize, parse-anplusb: `firstChild.data = …`).
    if (b && (b.__offen || (b.__n === n && b.__f === f && b.__text === tx))) return b;
    b = Object.create(P);
    intern(b, '__el', el);
    intern(b, '__text', tx);
    intern(b, '__regeln', null);
    intern(b, '__n', n);
    intern(b, '__f', f);
    intern(b, '__offen', false);
    b.disabled = false;
    blaetter.set(el, b);
    return b;
  }
  getter(SE, 'sheet', function () { return blattFuer(this, false); });
  if (LE) getter(LE, 'sheet', function () { return blattFuer(this, true); });
  // Release 12.10.2026: `link.disabled` spiegelt das Attribut; beim Setzen
  // verliert das bisherige Blatt seinen Besitzer (WPT HTMLLinkElement-disabled).
  if (LE) getter(LE, 'disabled', function () { return this.hasAttribute('disabled'); }, function (v) {
    if (v) {
      var b = blaetter.get(this);
      if (b) { intern(b, '__el', null); blaetter.delete(this); }
      this.setAttribute('disabled', '');
    } else {
      // Release 14.10.2026: removing the attribute through the setter
      // enables the sheet explicitly, also an alternate one (HTML §4.6.7).
      if (this.hasAttribute('disabled') && NAUS) { try { NAUS(this, false); } catch (e) {} }
      this.removeAttribute('disabled');
    }
  });
  getter(D, 'styleSheets', function () {
    var l = [], els = [];
    try { Object.setPrototypeOf(l, StyleSheetList.prototype); } catch (e) {}
    try { els = this.querySelectorAll('style, link'); } catch (e) {}
    for (var i = 0; i < els.length; i++) {
      var b = null;
      try { if (els[i].hasAttribute('data-narsil-adoptiert')) continue; } catch (e) {}
      try { b = els[i].sheet; } catch (e) {}
      if (b) Array.prototype.push.call(l, b);
    }
    return l;
  });
  methode(StyleSheetList.prototype, 'item', function (i) { i = Number(i) >>> 0; return i < this.length ? this[i] : null; });
  try {
    var lg2 = Object.getOwnPropertyDescriptor(Array.prototype, 'length');
    if (lg2 && lg2.get) getter(StyleSheetList.prototype, 'length', lg2.get);
  } catch (e) {}
  if (typeof Symbol === 'function' && Symbol.iterator && Array.prototype.values) methode(StyleSheetList.prototype, Symbol.iterator, Array.prototype.values);

  // Release 10.10.2026: `adoptedStyleSheets` (CSSOM §6.1: konstruierte
  // Blaetter an Dokument und Schattenwurzel). Ein adoptiertes Blatt wird in
  // ein verstecktes <style> am Ende der Wurzel gespiegelt — daraus liest die
  // Kaskade, wie bei jedem anderen Blatt. GEMESSEN vorher (github.com,
  // Popover-Polyfill `P(document)`): `[q, ...document.adoptedStyleSheets]`
  // warf „kein Zugriff auf Undefiniert (Eigenschaft Symbol.iterator)" und
  // brach das environment-Modul ab.
  var adoptiert = new WeakMap();
  function adoptiertLesen() {
    var l = adoptiert.get(this);
    if (!l) { l = []; adoptiert.set(this, l); }
    return l;
  }
  function adoptiertSetzen(v) {
    if (v === null || typeof v !== 'object' || typeof v.length !== 'number') {
      throw new TypeError("Failed to set the 'adoptedStyleSheets' property: The provided value cannot be converted to a sequence.");
    }
    var neu = [];
    for (var i = 0; i < v.length && i < MAX_REGELN; i++) {
      var b = v[i];
      if (!b || Object.getPrototypeOf(b) !== P || b.__link) {
        throw new TypeError("Failed to set the 'adoptedStyleSheets' property: Failed to convert value to 'CSSStyleSheet'.");
      }
      neu.push(b);
    }
    var alt = adoptiert.get(this) || [];
    for (var j = 0; j < alt.length; j++) {
      if (neu.indexOf(alt[j]) < 0 && alt[j].__adoptEl) {
        try { alt[j].__adoptEl.remove(); } catch (e) {}
        if (alt[j].__el === alt[j].__adoptEl) alt[j].__el = null;
        alt[j].__adoptEl = null;
      }
    }
    var ziel = this.nodeType === 9 ? (this.documentElement || null) : this;
    for (var k = 0; k < neu.length; k++) {
      var bl = neu[k];
      if (!ziel) break;
      var el = bl.__adoptEl;
      if (!el) {
        try {
          el = (this.nodeType === 9 ? this : this.ownerDocument).createElement('style');
          el.setAttribute('data-narsil-adoptiert', '');
        } catch (e) { continue; }
        intern(bl, '__adoptEl', el);
      }
      // In Reihenfolge ans Ende — die Liste bestimmt die Kaskadenfolge.
      try { ziel.appendChild(el); } catch (e) {}
      if (!bl.__el) {
        intern(bl, '__el', el);
        intern(bl, '__adoptiert', true);
        // Release 13.10.2026: a sheet constructed as disabled stays out.
        if (bl.__disabled && NAUS) { try { NAUS(el, true); } catch (e) {} }
        try { el.textContent = bl.__regeln ? '' : (bl.__text || ''); } catch (e) {}
        if (bl.__regeln) spiegeln(bl);
      }
    }
    adoptiert.set(this, neu);
  }
  getter(D, 'adoptedStyleSheets', adoptiertLesen, adoptiertSetzen);
  if (g.ShadowRoot && g.ShadowRoot.prototype) getter(g.ShadowRoot.prototype, 'adoptedStyleSheets', adoptiertLesen, adoptiertSetzen);
})();

// -- `escape`/`unescape` (ES2023 Anhang B.2.1) --
// Release 10.10.2026: fehlten. GEMESSEN (github.com): „escape is not a
// function" in einer Versprechenskette (Cookie-Helfer). Node 22:
// `escape('ä ü/€')` -> `%E4%20%FC/%u20AC`, `unescape('%u20AC%41%zz')` -> `€A%zz`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var FREI = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789@*_+-./';
  var HEX = '0123456789ABCDEF';
  function def(name, f) {
    if (typeof g[name] === 'function') return;
    try { Object.defineProperty(g, name, { value: f, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = f; }
  }
  def('escape', function escape(t) {
    t = String(t);
    var r = '';
    for (var i = 0; i < t.length; i++) {
      var c = t.charCodeAt(i);
      if (c < 128 && FREI.indexOf(t.charAt(i)) >= 0) r += t.charAt(i);
      else if (c < 256) r += '%' + HEX.charAt(c >> 4) + HEX.charAt(c & 15);
      else r += '%u' + HEX.charAt((c >> 12) & 15) + HEX.charAt((c >> 8) & 15) + HEX.charAt((c >> 4) & 15) + HEX.charAt(c & 15);
    }
    return r;
  });
  def('unescape', function unescape(t) {
    t = String(t);
    var r = '', n = t.length;
    for (var i = 0; i < n; i++) {
      var ch = t.charAt(i);
      if (ch === '%') {
        if (i + 6 <= n && t.charAt(i + 1) === 'u' && /^[0-9a-fA-F]{4}$/.test(t.substr(i + 2, 4))) {
          r += String.fromCharCode(parseInt(t.substr(i + 2, 4), 16));
          i += 5;
          continue;
        }
        if (i + 3 <= n && /^[0-9a-fA-F]{2}$/.test(t.substr(i + 1, 2))) {
          r += String.fromCharCode(parseInt(t.substr(i + 1, 2), 16));
          i += 2;
          continue;
        }
      }
      r += ch;
    }
    return r;
  });
  // `CSS.escape` (CSSOM §2.1 „serialize an identifier"). GEMESSEN
  // (github.com): „escape is not a function" — `CSS` hatte nur `supports`.
  if (g.CSS && typeof g.CSS.escape !== 'function') {
    try {
      g.CSS.escape = function escape(v) {
        if (arguments.length === 0) throw new TypeError("Failed to execute 'escape' on 'CSS': 1 argument required, but only 0 present.");
        var t = String(v), n = t.length, r = '', erstes = t.charCodeAt(0);
        for (var i = 0; i < n; i++) {
          var c = t.charCodeAt(i);
          if (c === 0) { r += '�'; continue; }
          if ((c >= 1 && c <= 31) || c === 127 || (i === 0 && c >= 48 && c <= 57) ||
              (i === 1 && c >= 48 && c <= 57 && erstes === 45)) { r += '\\' + c.toString(16) + ' '; continue; }
          if (i === 0 && n === 1 && c === 45) { r += '\\' + t.charAt(i); continue; }
          if (c >= 128 || c === 45 || c === 95 || (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) { r += t.charAt(i); continue; }
          r += '\\' + t.charAt(i);
        }
        return r;
      };
    } catch (e) {}
  }
})();

// -- Lauscher-OBJEKTE (DOM §2.7 „EventListener": `{ handleEvent(e) {} }`) --
// Release 10.10.2026: der Host nimmt nur Funktionen. Ein Objekt bekommt
// darum EINE Huelle (je Objekt dieselbe — `removeEventListener` findet sie
// wieder), die `handleEvent` mit dem Objekt als `this` ruft. GEMESSEN vorher
// (github.com): `this.addEventListener('…', this)` in Custom Elements warf
// „callback must be a function; handleEvent objects are not supported yet".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.WeakMap !== 'function') return;
  var huellen = new WeakMap();
  function huelle(h, neu) {
    if (h === null || (typeof h !== 'object' && typeof h !== 'function') || typeof h === 'function') return h;
    var f = huellen.get(h);
    if (!f && neu) {
      f = function (ev) {
        var m = h.handleEvent;
        if (typeof m !== 'function') throw new TypeError("'handleEvent' property of event listener should be a function");
        return m.call(h, ev);
      };
      huellen.set(h, f);
    }
    return f || h;
  }
  // Die Methoden liegen nicht nur an `EventTarget.prototype` — `Node`,
  // `Element`, `Document` und das Fenster tragen EIGENE Host-Methoden
  // (GEMESSEN: `div.addEventListener !== EventTarget.prototype.addEventListener`).
  function umhuellen(o) {
    if (!o || !Object.prototype.hasOwnProperty.call(o, 'addEventListener')) return;
    var an = o.addEventListener, ab = o.removeEventListener;
    if (typeof an !== 'function' || an.__narsilHuelle) return;
    var neuAn = function addEventListener(t, h, opt) { return an.call(this, t, huelle(h, true), opt); };
    neuAn.__narsilHuelle = true;
    try { Object.defineProperty(o, 'addEventListener', { value: neuAn, configurable: true, writable: true, enumerable: false }); } catch (e) { o.addEventListener = neuAn; }
    if (typeof ab === 'function') {
      var neuAb = function removeEventListener(t, h, opt) { return ab.call(this, t, huelle(h, false), opt); };
      try { Object.defineProperty(o, 'removeEventListener', { value: neuAb, configurable: true, writable: true, enumerable: false }); } catch (e) { o.removeEventListener = neuAb; }
    }
  }
  ['EventTarget', 'Node', 'Element', 'HTMLElement', 'Document', 'DocumentFragment', 'ShadowRoot', 'Window', 'SVGElement', 'CharacterData', 'Text'].forEach(function (n) {
    try { if (g[n] && g[n].prototype) umhuellen(g[n].prototype); } catch (e) {}
  });
  try { umhuellen(g); } catch (e) {}
})();

// -- location und history lebendig (HTML §7.2.4, §7.4) --
// Release 10.10.2026: `location` war ein Schnappschuss der Ladeadresse —
// nach `pushState` blieb `location.pathname` alt (Next.js liest danach den
// Pfad), `location.href = x` navigierte nicht, und `pushState(s, '', '?q=2')`
// machte `?q=2` zur ganzen Adresse. `history.state` fehlte.
// Jetzt: die Teile kommen aus der AKTUELLEN Adresse (`locationHref`),
// Schreiben navigiert (nur der Anker anders: `hashchange` ohne Laden),
// `pushState`/`replaceState` loesen relativ auf und merken den Zustand.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var L = g.location, H = g.history, lies = g.locationHref, D = g.Document && g.Document.prototype;
  if (!L || typeof lies !== 'function' || typeof g.URL !== 'function') return;
  var zuweisen = L.assign, ersetzen = L.replace, neuLaden = L.reload;
  var schieben = H && H.pushState, tauschen = H && H.replaceState;
  function jetzt() { try { return String(lies()); } catch (e) { return ''; } }
  function aufloesen(u) { try { return new g.URL(String(u), jetzt()).href; } catch (e) { return String(u); } }
  function ohneAnker(u) { var i = u.indexOf('#'); return i < 0 ? u : u.slice(0, i); }
  function teil(n) { try { return new g.URL(jetzt())[n]; } catch (e) { return ''; } }
  function ankerGehen(alt, neu, ersetze) {
    if (alt === neu) return;
    var f = ersetze ? tauschen : schieben;
    try { if (typeof f === 'function') f.call(H, null, '', neu); } catch (e) {}
    setTimeout(function () {
      var ev = null;
      try { ev = new g.HashChangeEvent('hashchange', { oldURL: alt, newURL: neu }); } catch (e) {}
      if (!ev) {
        try { ev = g.__narsilUA(g.Event, 'hashchange'); ev.oldURL = alt; ev.newURL = neu; } catch (e) { return; }
      }
      try { g.dispatchEvent(ev); } catch (e) {}
    }, 0);
  }
  function gehen(ziel, ersetze) {
    var alt = jetzt();
    // Release 14.10.2026: a javascript: URL runs its code as a task in this
    // window (HTML §7.4.2.3.4); a string result would replace the document,
    // which is not modelled. MEASURED before (WPT execution-timing 028,
    // 079): `location = 'javascript:…'` did nothing.
    if (/^[\s]*javascript:/i.test(ziel)) { g.__narsilJsUrl(ziel); return; }
    if (ziel.indexOf('#') >= 0 && ohneAnker(alt) === ohneAnker(ziel)) { ankerGehen(alt, ziel, ersetze); return; }
    var f = ersetze ? ersetzen : zuweisen;
    if (typeof f === 'function') f.call(L, ziel);
  }
  ['href', 'origin', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'].forEach(function (n) {
    try {
      Object.defineProperty(L, n, {
        configurable: true,
        enumerable: true,
        get: function () { return teil(n); },
        set: function (v) {
          if (n === 'origin') return;
          if (n === 'href') { gehen(aufloesen(v), false); return; }
          var u;
          try { u = new g.URL(jetzt()); } catch (e) { return; }
          u[n] = String(v);
          gehen(u.href, false);
        }
      });
    } catch (e) {}
  });
  try {
    Object.defineProperty(g, '__narsilJsUrl', {
      configurable: true, writable: true,
      value: function (url, danach) {
        var code = String(url).replace(/^[\s]*javascript:/i, '');
        try { code = decodeURIComponent(code); } catch (e) {}
        setTimeout(function () {
          if (typeof danach === 'function') danach();
          try { (0, eval)(code); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
        }, 0);
      }
    });
  } catch (e) {}
  L.assign = function assign(u) { gehen(aufloesen(u), false); };
  L.replace = function replace(u) { gehen(aufloesen(u), true); };
  // Release 14.10.2026: `window.location` is an accessor whose setter
  // navigates ([PutForwards=href], HTML §7.2.4). MEASURED before: the global
  // was a plain data property, so `window.location = url` and
  // `location = url` (the usual redirects) replaced it with the string and
  // navigated nowhere.
  try {
    Object.defineProperty(g, 'location', {
      configurable: true, enumerable: true,
      get: function () { return L; },
      set: function (v) { L.href = v; }
    });
  } catch (e) {}
  L.reload = function reload() { if (typeof neuLaden === 'function') neuLaden.call(L); };
  L.toString = function toString() { return jetzt(); };
  if (D) {
    try {
      Object.defineProperty(D, 'location', {
        configurable: true,
        enumerable: true,
        // Release 13.10.2026: nur das Dokument eines Fensters hat einen Ort
        // (HTML §7.2.4); erzeugte Dokumente null. Rahmendokumente setzen
        // ihn selbst.
        get: function () { return this === g.document ? g.location : null; },
        set: function (v) { L.href = v; }
      });
    } catch (e) {}
    // Release 10.10.2026: `document.domain` (HTML §7.5.3) — der Host der
    // Herkunft (`""` fuer file:/opake Herkuenfte). Setzen darf nur auf die
    // eigene Domain oder eine uebergeordnete (Suffix an einer Punktgrenze),
    // sonst SecurityError; eine IP-Adresse laesst sich nicht verkuerzen.
    // GEMESSEN vorher (github.com, behaviors-Modul): `document.domain` war
    // `undefined` -> „Unable to get document domain", das Modul brach ab.
    var domain = null;
    try {
      Object.defineProperty(D, 'domain', {
        configurable: true,
        enumerable: true,
        get: function () {
          if (domain !== null) return domain;
          var p = teil('protocol');
          return p === 'http:' || p === 'https:' ? teil('hostname') : '';
        },
        set: function (v) {
          var jetztD = this.domain, w = String(v).toLowerCase();
          var ip = /^[\d.]+$/.test(jetztD) || jetztD.indexOf(':') >= 0;
          var ok = jetztD !== '' && (w === jetztD || (!ip && w.indexOf('.') > 0 &&
            jetztD.length > w.length && jetztD.slice(-(w.length + 1)) === '.' + w));
          if (!ok) {
            throw new g.DOMException("Failed to set the 'domain' property on 'Document': '" + w + "' is not a suffix of '" + jetztD + "'.", 'SecurityError');
          }
          domain = w;
        }
      });
    } catch (e) {}
    // Release 10.10.2026: `document.fonts` (CSS Font Loading §4, FontFaceSet)
    // und `FontFace`. Die Webfonts liegen beim Bau der Seite schon vor, also
    // ist der Satz sofort `loaded` und `ready` erfuellt. GEMESSEN vorher
    // (Web-Platform-Tests): `document.fonts.ready.then(…)` warf — jeder Test,
    // der so auf die Schriften wartet, lief in die Zeitgrenze.
    if (!D.fonts) {
      var FontFace = function FontFace(family, source, descriptors) {
        if (!(this instanceof FontFace)) throw new TypeError("Failed to construct 'FontFace': Please use the 'new' operator.");
        var d = descriptors || {};
        this.family = String(family);
        this.style = d.style || 'normal';
        this.weight = d.weight || 'normal';
        this.stretch = d.stretch || 'normal';
        this.unicodeRange = d.unicodeRange || 'U+0-10FFFF';
        this.display = d.display || 'auto';
        this.featureSettings = d.featureSettings || 'normal';
        this.status = 'unloaded';
        var selbst = this;
        this.loaded = new Promise(function (ja, nein) { selbst._fertig = ja; selbst._fehler = nein; });
        try { this.loaded.catch(function () {}); } catch (e) {}
        try { Object.defineProperty(this, '_quelle', { value: source, writable: true }); } catch (e) { this._quelle = source; }
        // Release 15.10.2026: binary sources are font data right away (CSS
        // Font Loading §2.1 step 11).
        if (source && typeof source === 'object' && (source instanceof ArrayBuffer || ArrayBuffer.isView(source))) {
          var bytes = source instanceof ArrayBuffer ? new Uint8Array(source) : new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
          this.status = 'loading';
          schriftAnmelden(this, bytes);
        }
      };
      // Release 15.10.2026: FontFace.load() fetches and registers the font
      // with the text engine (`__narsilSchriftAnmelden`), so canvas text and
      // layout can use it. MEASURED before: `new FontFace(...).load()` only
      // set the status; OffscreenCanvas text tests measured a fallback font.
      var schriftGewicht = function (w) {
        w = String(w === undefined ? 'normal' : w).trim().split(/\s+/)[0];
        if (w === 'normal') return 400;
        if (w === 'bold') return 700;
        var n = parseFloat(w);
        return isFinite(n) ? Math.max(1, Math.min(1000, Math.round(n))) : 400;
      };
      // font-stretch keyword or percentage -> percent.
      var fontStretchPercent = function (s) {
        s = String(s === undefined ? 'normal' : s).trim().split(/\s+/)[0].toLowerCase();
        var named = { 'ultra-condensed': 50, 'extra-condensed': 62.5, 'condensed': 75, 'semi-condensed': 87.5, 'normal': 100,
          'semi-expanded': 112.5, 'expanded': 125, 'extra-expanded': 150, 'ultra-expanded': 200 };
        if (named.hasOwnProperty(s)) return named[s];
        var n = parseFloat(s);
        return /%$/.test(s) && isFinite(n) ? n : 100;
      };
      var schriftAnmelden = function (f, bytes) {
        var ok = false;
        try {
          ok = typeof g.__narsilSchriftAnmelden === 'function'
            && g.__narsilSchriftAnmelden(f.family, schriftGewicht(f.weight), /italic|oblique/i.test(String(f.style)), bytes, fontStretchPercent(f.stretch)) === true;
        } catch (e) { ok = false; }
        if (ok) { f.status = 'loaded'; f._fertig(f); }
        else { f.status = 'error'; f._fehler(new g.DOMException('The font could not be loaded.', 'SyntaxError')); }
      };
      FontFace.prototype.load = function load() {
        var f = this;
        if (f.status !== 'unloaded') return f.loaded;
        f.status = 'loading';
        var m = /url\(\s*(['"]?)([^'")]+)\1\s*\)/.exec(String(f._quelle));
        if (!m || typeof g.fetch !== 'function') {
          f.status = 'error';
          f._fehler(new g.DOMException('The font source could not be parsed.', 'SyntaxError'));
          return f.loaded;
        }
        g.fetch(m[2]).then(function (r) {
          if (!r.ok) throw new Error('status ' + r.status);
          return r.arrayBuffer();
        }).then(function (buf) { schriftAnmelden(f, new Uint8Array(buf)); }, function () {
          f.status = 'error';
          f._fehler(new g.DOMException('The font could not be fetched.', 'NetworkError'));
        });
        return f.loaded;
      };
      var menge = [];
      var satz = {
        status: 'loaded',
        onloading: null, onloadingdone: null, onloadingerror: null,
        get size() { return menge.length; },
        check: function check() { return true; },
        load: function load() { return Promise.resolve([]); },
        add: function add(f) { if (menge.indexOf(f) < 0) menge.push(f); return satz; },
        'delete': function (f) { var i = menge.indexOf(f); if (i >= 0) menge.splice(i, 1); return i >= 0; },
        has: function has(f) { return menge.indexOf(f) >= 0; },
        clear: function clear() { menge.length = 0; },
        forEach: function forEach(cb, t) { menge.slice().forEach(function (f) { cb.call(t, f, f, satz); }); },
        values: function values() { return menge.slice()[Symbol.iterator](); },
        keys: function keys() { return menge.slice()[Symbol.iterator](); },
        entries: function entries() { return menge.map(function (f) { return [f, f]; })[Symbol.iterator](); },
        addEventListener: function addEventListener() {},
        removeEventListener: function removeEventListener() {},
        dispatchEvent: function dispatchEvent() { return true; }
      };
      satz[Symbol.iterator] = satz.values;
      // `ready` settles after every added face that is still loading.
      try {
        Object.defineProperty(satz, 'ready', {
          configurable: true, enumerable: true,
          get: function () {
            var offen = menge.filter(function (f) { return f.status === 'loading'; }).map(function (f) {
              return f.loaded.then(function () {}, function () {});
            });
            return Promise.all(offen).then(function () { return satz; });
          }
        });
      } catch (e) { satz.ready = Promise.resolve(satz); }
      try {
        Object.defineProperty(D, 'fonts', { configurable: true, enumerable: true, get: function () { return satz; } });
      } catch (e) {}
      if (!g.FontFace) g.FontFace = FontFace;
    }
  }
  if (H && typeof schieben === 'function') {
    var zustand = null;
    var klon = function (s) {
      if (s === undefined) return null;
      try { return typeof g.structuredClone === 'function' ? g.structuredClone(s) : s; } catch (e) { return s; }
    };
    H.pushState = function pushState(s, t, u) {
      var z = klon(s);
      if (u !== undefined && u !== null) u = aufloesen(u);
      else u = jetzt();
      schieben.call(H, s, t === undefined ? '' : t, u);
      zustand = z;
    };
    H.replaceState = function replaceState(s, t, u) {
      var z = klon(s);
      if (u !== undefined && u !== null) u = aufloesen(u);
      else u = jetzt();
      if (typeof tauschen === 'function') tauschen.call(H, s, t === undefined ? '' : t, u);
      zustand = z;
    };
    try { Object.defineProperty(H, 'state', { configurable: true, enumerable: true, get: function () { return zustand; } }); } catch (e) {}
    if (H.scrollRestoration === undefined) H.scrollRestoration = 'auto';
  }
})();

// -- URL.canParse / URL.parse (URL §6.1), relList (HTML §4.6.6) --
// Release 10.10.2026: core-js prueft `URL.canParse.length` (bbc.com:
// „kein Zugriff auf Undefiniert (Eigenschaft length)", abgefangen), Next.js
// fragt `link.relList.supports('prefetch')`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var U = g.URL;
  if (typeof U === 'function') {
    if (typeof U.canParse !== 'function') {
      U.canParse = function canParse(u) {
        try { new U(u, arguments.length > 1 && arguments[1] !== undefined ? arguments[1] : undefined); return true; } catch (e) { return false; }
      };
    }
    if (typeof U.parse !== 'function') {
      U.parse = function parse(u) {
        try { return new U(u, arguments.length > 1 && arguments[1] !== undefined ? arguments[1] : undefined); } catch (e) { return null; }
      };
    }
  }
  // relList: the reflection block's DOMTokenLists (Release 2026-10-15).
})();

// -- performance: die uebrigen Methoden (User Timing §3, Resource Timing §4) --
// Release 10.10.2026: bbc.com ruft `performance.clearMarks(...)` (Timer:
// „clearMarks is not a function"). Ressourcen-Timing fuehren wir nicht —
// die Methoden sind da und tun das Ehrliche (nichts zu loeschen).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var P = g.performance;
  if (!P) return;
  ['clearMarks', 'clearMeasures', 'clearResourceTimings', 'setResourceTimingBufferSize'].forEach(function (n) {
    if (typeof P[n] !== 'function') { try { P[n] = function () {}; } catch (e) {} }
  });
  if (typeof P.toJSON !== 'function') {
    try { P.toJSON = function toJSON() { return { timeOrigin: P.timeOrigin, timing: P.timing, navigation: P.navigation }; }; } catch (e) {}
  }
  if (!P.eventCounts && typeof g.Map === 'function') { try { P.eventCounts = new g.Map(); } catch (e) {} }
})();

// -- Text, Comment, DocumentFragment als Konstruktoren (DOM §4.7, §4.11, §4.14) --
// Release 11.10.2026: `new Text("x")` warf „Text ist kein Konstruktor"
// (WPT Lauf g: 16 Dateien). Der Knoten entsteht im Dokument des Fensters;
// der Prototyp bleibt derselbe (`instanceof` wie vorher).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var d = g.document;
  if (!d || typeof d.createTextNode !== 'function') return;
  function ersetzen(name, machen) {
    var alt = g[name];
    if (typeof alt !== 'function' || !alt.prototype) return;
    var neu = function () {
      if (!(this instanceof neu)) throw new TypeError("Failed to construct '" + name + "': Please use the 'new' operator");
      return machen.apply(null, arguments);
    };
    neu.prototype = alt.prototype;
    try { Object.defineProperty(alt.prototype, 'constructor', { value: neu, configurable: true, writable: true, enumerable: false }); } catch (e) {}
    try { Object.defineProperty(neu, 'name', { value: name, configurable: true }); } catch (e) {}
    try { Object.defineProperty(g, name, { value: neu, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = neu; }
  }
  function text(t) { return t === undefined ? '' : String(t); }
  ersetzen('Text', function (t) { return d.createTextNode(text(t)); });
  ersetzen('Comment', function (t) { return d.createComment(text(t)); });
  ersetzen('DocumentFragment', function () { return d.createDocumentFragment(); });
})();

// -- setHTMLUnsafe / Document.parseHTMLUnsafe (HTML §8.5) --
// Release 11.10.2026: „setHTMLUnsafe is not a function" (WPT Lauf g: 35
// Dateien). Ohne Bereinigung ist es das Setzen von `innerHTML`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  [g.Element, g.ShadowRoot].forEach(function (K) {
    var P = K && K.prototype;
    if (!P || typeof P.setHTMLUnsafe === 'function') return;
    try {
      // Release 12.10.2026: mit deklarativen Shadow Roots (Host-Methode).
      Object.defineProperty(P, 'setHTMLUnsafe', {
        value: function setHTMLUnsafe(html) {
          if (typeof this.__narsilHtmlUnsicher === 'function') this.__narsilHtmlUnsicher(String(html));
          else this.innerHTML = String(html);
        },
        configurable: true, writable: true, enumerable: false
      });
    } catch (e) {}
    // Release 12.10.2026: `getHTML(options)` (HTML §8.5): `innerHTML` plus die
    // Shadow Roots, die `serializable` sind (mit `serializableShadowRoots`)
    // oder in `shadowRoots` stehen.
    if (typeof P.getHTML !== 'function') {
      try {
        Object.defineProperty(P, 'getHTML', {
          value: function getHTML(options) {
            var ser = !!(options && options.serializableShadowRoots);
            var wurzeln = options && options.shadowRoots ? Array.prototype.slice.call(options.shadowRoots) : [];
            if (typeof this.__narsilGetHtml === 'function') return this.__narsilGetHtml(ser, wurzeln);
            return this.innerHTML;
          },
          configurable: true, writable: true, enumerable: false
        });
      } catch (e) {}
    }
  });
  var D = g.Document;
  if (D && typeof D.parseHTMLUnsafe !== 'function' && g.document && g.document.implementation) {
    try {
      Object.defineProperty(D, 'parseHTMLUnsafe', {
        value: function parseHTMLUnsafe(html) {
          var doc = g.document.implementation.createHTMLDocument('');
          doc.documentElement.innerHTML = String(html);
          return doc;
        },
        configurable: true, writable: true, enumerable: false
      });
    } catch (e) {}
  }
})();

// -- <dialog> (HTML §4.11.4) und Popover (HTML §6.12) --
// Release 11.10.2026: `show`/`showModal`/`close`/`requestClose`,
// `returnValue`, die Ereignisse `beforetoggle`/`toggle`/`cancel`/`close`;
// `showPopover`/`hidePopover`/`togglePopover` mit dem Attribut `popover`.
// GEMESSEN vorher (WPT Lauf g): the-dialog-element 14/85, popovers 3/52 —
// „showModal is not a function" usw.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || !g.HTMLElement) return;
  function ausnahme(text, name) {
    return typeof g.DOMException === 'function' ? new g.DOMException(text, name) : new Error(text);
  }
  function spaeter(f) { if (typeof g.setTimeout === 'function') g.setTimeout(f, 0); else f(); }
  function umschaltEreignis(typ, alt, neu, abbrechbar, quelle) {
    var e;
    try { e = g.__narsilUA(g.ToggleEvent, typ, { oldState: alt, newState: neu, cancelable: !!abbrechbar, source: quelle || null }); } catch (x) {
      e = g.__narsilUA(g.Event, typ, { cancelable: !!abbrechbar });
      try { Object.defineProperty(e, 'oldState', { value: alt }); Object.defineProperty(e, 'newState', { value: neu }); } catch (y) {}
    }
    return e;
  }
  // `toggle` wird zusammengefasst: ein wartendes Ereignis behaelt seinen
  // alten Zustand (HTML „queue a popover toggle event task"). Dialog und
  // Popover haben je einen eigenen Tracker (HTML „dialog toggle task
  // tracker") — GEMESSEN vorher (WPT toggle-events): `show()` + ein
  // `showPopover()` im `beforetoggle` lieferte 1 statt 2 `toggle`.
  var wartendPopover = new WeakMap(), wartendDialog = new WeakMap();
  function toggleMelden(el, alt, neu, karte, quelle) {
    karte = karte || wartendPopover;
    var w = karte.get(el);
    if (w) { w.neu = neu; w.quelle = quelle || null; return; }
    w = { alt: alt, neu: neu, quelle: quelle || null };
    karte.set(el, w);
    spaeter(function () {
      karte.delete(el);
      el.dispatchEvent(umschaltEreignis('toggle', w.alt, w.neu, false, w.quelle));
    });
  }

  var D = g.HTMLDialogElement && g.HTMLDialogElement.prototype;
  var modal = new WeakSet(), rueckgabe = new WeakMap(), vorherFokus = new WeakMap();
  var popoverOffen = new WeakSet();
  var aktiv = null;
  // Die modalen Dialoge in der Reihenfolge der obersten Schicht; `schicht`
  // ist die ganze oberste Schicht (auch offene Popover).
  var modalStapel = [], schicht = [];
  function schichtRein(el) { schichtRaus(el); schicht.push(el); }
  function schichtRaus(el) { var i = schicht.indexOf(el); if (i >= 0) schicht.splice(i, 1); }
  try {
    Object.defineProperty(g, '__narsilObersteSchicht', {
      value: function () { return schicht.filter(function (e) { return e.isConnected; }); },
      configurable: true, writable: true, enumerable: false
    });
    Object.defineProperty(g, '__narsilInert', {
      value: function (el) { return inert(el); }, configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
  // `:popover-open` (Bit 1), `:modal` (Bit 2) und `:focus` (Bit 4) fuer die
  // Kaskade — der Host fuehrt sie, sie sind kein Attribut.
  function zustandMelden(el) {
    var bits = (popoverOffen.has(el) ? 1 : 0) | (modal.has(el) ? 2 : 0) | (el === aktiv ? 4 : 0);
    try { if (typeof el.__cssZustand === 'function') el.__cssZustand(bits); } catch (e) {}
  }
  function schattenVon(el) {
    try { return typeof g.__narsilSchatten === 'function' ? g.__narsilSchatten(el) : (el.shadowRoot || null); } catch (e) { return null; }
  }
  // Schatten-einschliessender Vorfahr (DOM §4.8).
  function enthaelt(a, b) {
    for (var n = b; n; n = n.parentNode || n.host || null) if (n === a) return true;
    return false;
  }
  function obersterModal() {
    for (var i = modalStapel.length - 1; i >= 0; i--) {
      var d = modalStapel[i];
      if (d.isConnected && modal.has(d)) return d;
    }
    return null;
  }
  // HTML §6.3 „inert": das Attribut an einem Vorfahren oder ein modaler
  // Dialog, ausserhalb dessen das Element liegt („blocked by a modal dialog").
  function inert(el) {
    for (var n = el; n; n = n.parentNode || n.host || null) {
      if (n.nodeType === 1 && n.hasAttribute('inert')) return true;
    }
    var m = obersterModal();
    return !!m && !enthaelt(m, el);
  }

  // -- Fokus (HTML §6.6) --
  // Release 11.10.2026: `focus()`/`blur()` taten nichts, und
  // `document.activeElement` fehlte (WPT: Dialog-, Formular- und
  // Tastaturtests). Jetzt: fokussierbare Elemente, die Ereignisse `blur`/
  // `focusout` und `focus`/`focusin` und `:focus` in der Kaskade.
  function fokusWechseln(neu) {
    var alt = aktiv && aktiv.isConnected ? aktiv : null;
    if (alt === neu) return;
    aktiv = neu;
    if (alt) zustandMelden(alt);
    if (neu) zustandMelden(neu);
    function feuern(el, typ, blasen, anderes) {
      var e;
      try { e = g.__narsilUA(g.FocusEvent, typ, { bubbles: blasen, composed: true, relatedTarget: anderes }); } catch (x) {
        e = g.__narsilUA(g.Event, typ, { bubbles: blasen, composed: true });
      }
      try { el.dispatchEvent(e); } catch (x) {}
    }
    if (alt) { feuern(alt, 'blur', false, neu); feuern(alt, 'focusout', true, neu); }
    if (neu && aktiv === neu) { feuern(neu, 'focus', false, alt); feuern(neu, 'focusin', true, alt); }
  }
  function fokusFaehig(el) {
    if (!el || !el.isConnected || inert(el)) return false;
    var t = el.localName;
    if ((el.disabled || el.hasAttribute('disabled')) && /^(button|input|select|textarea|fieldset|optgroup|option)$/.test(t)) return false;
    if (el.hasAttribute('tabindex')) return true;
    if (t === 'input') return el.type !== 'hidden';
    if (t === 'a' || t === 'area') return el.hasAttribute('href');
    if (t === 'summary') return !!(el.parentElement && el.parentElement.localName === 'details');
    if ((t === 'audio' || t === 'video') && el.hasAttribute('controls')) return true;
    var ce = el.getAttribute('contenteditable');
    return t === 'button' || t === 'select' || t === 'textarea' || t === 'iframe' || t === 'dialog' ||
      (ce !== null && ce !== 'false');
  }
  // „sequentially focusable": fokussierbar und `tabIndex` >= 0.
  function sequentiell(el) { return fokusFaehig(el) && !(el.tabIndex < 0); }
  // HTML §6.6.4 „autofocus delegate" und „focus delegate".
  function autofokusDelegat(wo, tiefe) {
    var alle = wo.querySelectorAll('[autofocus]');
    for (var i = 0; i < alle.length; i++) {
      var d = alle[i], f = fokusFaehig(d) ? d : fokusDelegat(d, tiefe + 1);
      if (f) return f;
    }
    return null;
  }
  function fokusDelegat(ziel, tiefe) {
    tiefe = tiefe || 0;
    if (tiefe > 32) return null;
    var sr = schattenVon(ziel);
    if (sr && !sr.delegatesFocus) return null;
    var wo = sr || ziel;
    var a = autofokusDelegat(wo, tiefe);
    if (a) return a;
    var dialog = ziel.localName === 'dialog', alle = wo.querySelectorAll('*');
    for (var i = 0; i < alle.length; i++) {
      var d = alle[i], f = null;
      if (dialog ? sequentiell(d) : fokusFaehig(d)) f = d;
      else if (schattenVon(d)) f = fokusDelegat(d, tiefe + 1);
      if (f) return f;
    }
    return null;
  }
  // Die „focusing steps": ein Schatten-Wirt mit delegatesFocus gibt weiter.
  function fokusieren(el) {
    if (!el) return;
    var sr = schattenVon(el);
    if (sr && sr.delegatesFocus) {
      if (aktiv && aktiv.isConnected && enthaelt(el, aktiv)) return;
      var f = fokusDelegat(el);
      if (f) fokusWechseln(f);
      return;
    }
    if (fokusFaehig(el)) fokusWechseln(el);
  }
  // HTML §4.11.4 „dialog focusing steps".
  function fokusSchritte(dialog) {
    var control = dialog.hasAttribute('autofocus') ? dialog : null;
    if (!control) control = fokusDelegat(dialog);
    if (!control) control = dialog;
    if (control === dialog) { if (fokusFaehig(dialog)) fokusWechseln(dialog); }
    else fokusieren(control);
  }
  [g.HTMLElement, g.SVGElement].forEach(function (K) {
    var P = K && K.prototype;
    if (!P) return;
    def(P, 'focus', function focus() { fokusieren(this); });
    def(P, 'blur', function blur() { if (aktiv === this) fokusWechseln(null); });
  });
  // „focus fixup": ein fokussiertes Element, das nicht mehr haengt oder
  // inert wurde, verliert den Fokus.
  function gueltigesAktiv() {
    if (aktiv && (!aktiv.isConnected || inert(aktiv))) { var weg = aktiv; aktiv = null; zustandMelden(weg); }
    return aktiv;
  }
  // DOM §4.8 „retarget": gegen die Wurzel `gegen`.
  function zurueck(el, gegen) {
    for (var n = el, i = 0; n && i < 64; i++) {
      var r = typeof n.getRootNode === 'function' ? n.getRootNode() : null;
      if (r === gegen) return n;
      if (!r || !r.host) return null;
      n = r.host;
    }
    return null;
  }
  var DP = g.Document && g.Document.prototype;
  if (DP) {
    eig(DP, 'activeElement', function () {
      if (this !== doc) return null;
      var a = gueltigesAktiv();
      return (a && zurueck(a, doc)) || doc.body || doc.documentElement;
    });
    def(DP, 'hasFocus', function hasFocus() { return this === doc; });
  }
  var SP = g.ShadowRoot && g.ShadowRoot.prototype;
  if (SP) {
    eig(SP, 'activeElement', function () {
      var a = gueltigesAktiv();
      return a ? zurueck(a, this) : null;
    });
  }
  // Das Fenster selbst: `window.focus()`/`blur()` aendern hier nichts.
  if (typeof g.focus !== 'function') { try { g.focus = function focus() {}; } catch (e) {} }
  if (typeof g.blur !== 'function') { try { g.blur = function blur() {}; } catch (e) {} }
  function def(P, name, wert) {
    try { Object.defineProperty(P, name, { value: wert, configurable: true, writable: true, enumerable: true }); } catch (e) {}
  }
  function eig(P, name, lesen, schreiben) {
    try { Object.defineProperty(P, name, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  if (D && typeof D.showModal !== 'function') {
    eig(D, 'open', function () { return this.hasAttribute('open'); },
      function (v) { if (v) this.setAttribute('open', ''); else this.removeAttribute('open'); });
    eig(D, 'returnValue', function () { var r = rueckgabe.get(this); return r === undefined ? '' : r; },
      function (v) { rueckgabe.set(this, String(v)); });
    // HTML §4.11.4 `show()`.
    def(D, 'show', function show() {
      if (this.hasAttribute('open')) {
        if (!modal.has(this)) return;
        throw ausnahme('Der Dialog ist schon modal offen', 'InvalidStateError');
      }
      if (!this.dispatchEvent(umschaltEreignis('beforetoggle', 'closed', 'open', true))) return;
      if (this.hasAttribute('open')) return;
      toggleMelden(this, 'closed', 'open', wartendDialog);
      this.setAttribute('open', '');
      vorherFokus.set(this, gueltigesAktiv());
      fokusSchritte(this);
    });
    // HTML §4.11.4 `showModal()`.
    def(D, 'showModal', function showModal() {
      if (this.hasAttribute('open')) {
        if (modal.has(this)) return;
        throw ausnahme('Der Dialog ist schon offen', 'InvalidStateError');
      }
      if (popoverOffen.has(this)) throw ausnahme('Der Dialog ist ein offenes Popover', 'InvalidStateError');
      if (!this.isConnected) throw ausnahme('Der Dialog haengt nicht im Dokument', 'InvalidStateError');
      // HTML §4.11.4 Schritt 4: das Dokument muss „fully active" sein.
      var od = this.ownerDocument;
      if (od && od !== doc && !od.defaultView) throw ausnahme('Das Dokument ist nicht aktiv', 'InvalidStateError');
      if (!this.dispatchEvent(umschaltEreignis('beforetoggle', 'closed', 'open', true))) return;
      if (this.hasAttribute('open') || !this.isConnected || popoverOffen.has(this)) return;
      var vorher = gueltigesAktiv();
      toggleMelden(this, 'closed', 'open', wartendDialog);
      this.setAttribute('open', '');
      modal.add(this);
      var k = modalStapel.indexOf(this);
      if (k >= 0) modalStapel.splice(k, 1);
      modalStapel.push(this);
      schichtRein(this);
      zustandMelden(this);
      vorherFokus.set(this, vorher);
      fokusSchritte(this);
    });
    // HTML §4.11.4 „close the dialog": Fokus nur zurueck, wenn er im Dialog
    // lag oder der Dialog modal war.
    function schliessen(d, r) {
      if (!d.hasAttribute('open')) return;
      d.dispatchEvent(umschaltEreignis('beforetoggle', 'open', 'closed', false));
      if (!d.hasAttribute('open')) return;
      toggleMelden(d, 'open', 'closed', wartendDialog);
      d.removeAttribute('open');
      var warModal = modal.has(d);
      modal.delete(d);
      var k = modalStapel.indexOf(d);
      if (k >= 0) modalStapel.splice(k, 1);
      if (warModal) schichtRaus(d);
      zustandMelden(d);
      if (r !== undefined) rueckgabe.set(d, String(r));
      if (vorherFokus.has(d)) {
        var v = vorherFokus.get(d), jetzt = aktiv && aktiv.isConnected ? aktiv : null;
        vorherFokus.delete(d);
        if (warModal || (jetzt && enthaelt(d, jetzt))) {
          if (v && v.isConnected) fokusieren(v);
          if (aktiv && enthaelt(d, aktiv)) fokusWechseln(null);
        }
      }
      spaeter(function () { d.dispatchEvent(g.__narsilUA(g.Event, 'close')); });
    }
    def(D, 'close', function close(r) { schliessen(this, r); });
    // HTML §4.11.4 `requestClose()`: `cancel` (abbrechbar), dann schliessen;
    // ein `requestClose()` im eigenen `cancel` tut nichts („is running
    // cancel action"). GEMESSEN vorher: `cancel` kam zweimal.
    var imAbbruch = new WeakSet();
    def(D, 'requestClose', function requestClose(r) {
      if (!this.hasAttribute('open') || imAbbruch.has(this)) return;
      imAbbruch.add(this);
      var weiter = true;
      try { weiter = this.dispatchEvent(g.__narsilUA(g.Event, 'cancel', { cancelable: true })); } finally { imAbbruch.delete(this); }
      if (weiter) schliessen(this, r);
    });
  }

  // Release 11.10.2026: `<details>` meldet jeden Wechsel von `open` mit
  // `toggle` (HTML §4.11.1 „details notification task steps",
  // zusammengefasst wie beim Dialog). GEMESSEN vorher: kein `toggle`.
  var wartendDetails = new WeakMap();
  try {
    Object.defineProperty(g, '__narsilDetailsUmschalten', {
      value: function (el) {
        var offenJetzt = el.hasAttribute('open');
        // Release 2026-10-15: opening a named details closes the other open
        // members of its name group (HTML §4.11.1 "ensure details
        // exclusivity by closing other elements if needed").
        var gruppe = offenJetzt && el.getAttribute('name');
        if (gruppe && typeof el.getRootNode === 'function') {
          var wurzel = el.getRootNode(), alle = wurzel.querySelectorAll ? wurzel.querySelectorAll('details[open]') : [];
          for (var i = 0; i < alle.length; i++) {
            var d = alle[i];
            if (d !== el && d.getAttribute('name') === gruppe && d.namespaceURI === 'http://www.w3.org/1999/xhtml') d.removeAttribute('open');
          }
        }
        detailsBekannt.add(el);
        var w = wartendDetails.get(el);
        if (w) { w.neu = offenJetzt ? 'open' : 'closed'; return; }
        toggleMelden(el, offenJetzt ? 'closed' : 'open', offenJetzt ? 'open' : 'closed', wartendDetails);
      },
      configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
  // Release 2026-10-15: a details the parser created with `open` gets its
  // `toggle` too (the attribute is appended at creation, HTML §4.11.1
  // attribute change steps). MEASURED before (WPT toggleEvent "Setting open
  // from the parser fires a toggle event"): no event.
  var detailsBekannt = new WeakSet();
  function geparsteDetails() {
    var alle = doc.querySelectorAll('details[open]');
    for (var i = 0; i < alle.length; i++) {
      var d = alle[i];
      if (detailsBekannt.has(d) || d.namespaceURI !== 'http://www.w3.org/1999/xhtml') continue;
      detailsBekannt.add(d);
      if (!wartendDetails.get(d)) toggleMelden(d, 'closed', 'open', wartendDetails);
    }
  }
  if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', geparsteDetails);
  else geparsteDetails();

  // -- Popover --
  var H = g.HTMLElement.prototype;
  if (typeof H.showPopover !== 'function') {
    var offen = popoverOffen;
    function art(el) {
      var a = el.getAttribute('popover');
      if (a === null) return null;
      a = a.toLowerCase();
      return a === 'manual' ? 'manual' : a === 'hint' ? 'hint' : 'auto';
    }
    function pruefen(el, sollOffen) {
      if (art(el) === null) throw ausnahme('Das Element hat kein popover-Attribut', 'NotSupportedError');
      if (sollOffen && offen.has(el)) return false;
      if (!sollOffen && !offen.has(el)) return false;
      if (!el.isConnected) throw ausnahme('Das Popover haengt nicht im Dokument', 'InvalidStateError');
      // Release 12.10.2026: „node document is not fully active" (HTML §6.12
      // „check popover validity"): ein Dokument ohne Fenster (erzeugt mit
      // `createHTMLDocument`). GEMESSEN vorher (WPT popover-active-document):
      // kein Fehler, das Popover ging auf.
      var od = el.ownerDocument;
      if (od && od !== doc && !od.defaultView) throw ausnahme('Das Dokument ist nicht aktiv', 'InvalidStateError');
      if (sollOffen && el.localName === 'dialog' && el.hasAttribute('open')) {
        throw ausnahme('Ein offener Dialog ist kein Popover', 'InvalidStateError');
      }
      return true;
    }
    function zeigen(el, quelle) {
      if (!pruefen(el, true)) return;
      if (!el.dispatchEvent(umschaltEreignis('beforetoggle', 'closed', 'open', true, quelle))) return;
      if (offen.has(el) || !el.isConnected || art(el) === null) return;
      if (art(el) === 'auto') {
        var andere = doc.querySelectorAll('[popover]');
        for (var i = 0; i < andere.length; i++) {
          var o = andere[i];
          if (o !== el && offen.has(o) && art(o) === 'auto' && !o.contains(el)) verbergen(o);
        }
      }
      offen.add(el);
      schichtRein(el);
      zustandMelden(el);
      toggleMelden(el, 'closed', 'open', undefined, quelle);
    }
    function verbergen(el, quelle) {
      if (!pruefen(el, false)) return;
      el.dispatchEvent(umschaltEreignis('beforetoggle', 'open', 'closed', false, quelle));
      if (!offen.has(el)) return;
      offen.delete(el);
      schichtRaus(el);
      zustandMelden(el);
      toggleMelden(el, 'open', 'closed', undefined, quelle);
    }
    // Release 2026-10-15: the options' `source` (ShowPopoverOptions,
    // TogglePopoverOptions) is the events' source.
    function quelleAus(o) {
      var q = o && typeof o === 'object' ? o.source : undefined;
      if (q === undefined || q === null) return null;
      if (!(q instanceof g.HTMLElement)) throw new TypeError("Failed to read the 'source' property: Failed to convert value to 'HTMLElement'.");
      return q;
    }
    def(H, 'showPopover', function showPopover(o) { zeigen(this, quelleAus(o)); });
    def(H, 'hidePopover', function hidePopover() { verbergen(this); });
    def(H, 'togglePopover', function togglePopover(o) {
      var soll = typeof o === 'boolean' ? o : (o && typeof o === 'object' && 'force' in o) ? !!o.force : !offen.has(this);
      var q = typeof o === 'boolean' ? null : quelleAus(o);
      if (art(this) === null) throw ausnahme('Das Element hat kein popover-Attribut', 'NotSupportedError');
      if (soll !== offen.has(this)) { if (soll) zeigen(this, q); else verbergen(this, q); }
      else if (!this.isConnected) throw ausnahme('Das Popover haengt nicht im Dokument', 'InvalidStateError');
      return offen.has(this);
    });
    eig(H, 'popover', function () { return art(this); },
      function (v) { if (v === null) this.removeAttribute('popover'); else this.setAttribute('popover', String(v)); });
    // Release 11.10.2026: `popoverTargetElement` (Element-Spiegelung, HTML
    // §2.6.1: ein gesetztes Element gilt, bis das Attribut sich aendert)
    // und `popoverTargetAction` (toggle/show/hide).
    var explizit = new WeakMap();
    function zielLesen(k) {
      var e = explizit.get(k);
      if (e) {
        var neu = e.mo ? e.mo.takeRecords() : [];
        if (neu.length || !k.hasAttribute('popovertarget')) { vergessen(k); e = null; }
      }
      if (e) return enthaeltS(e.el.getRootNode(), k) ? e.el : null;
      var id = k.getAttribute('popovertarget');
      if (!id) return null;
      var r = k.getRootNode ? k.getRootNode() : doc;
      if (!r || typeof r.querySelectorAll !== 'function') return null;
      var alle = r.querySelectorAll('[id]');
      for (var i = 0; i < alle.length; i++) if (alle[i].id === id) return alle[i];
      return null;
    }
    function vergessen(k) {
      var e = explizit.get(k);
      if (e && e.mo) { try { e.mo.disconnect(); } catch (x) {} }
      explizit.delete(k);
    }
    function enthaeltS(a, b) {
      for (var n = b; n; n = n.parentNode || n.host || null) if (n === a) return true;
      return false;
    }
    function zielSetzen(v) {
      var k = this;
      vergessen(k);
      if (v === null || v === undefined) { k.removeAttribute('popovertarget'); return; }
      if (!(v instanceof g.Element)) throw new TypeError("Failed to set the 'popoverTargetElement' property: The provided value is not of type 'Element'.");
      k.setAttribute('popovertarget', '');
      var mo = null;
      try {
        mo = new g.MutationObserver(function () { vergessen(k); });
        mo.observe(k, { attributes: true, attributeFilter: ['popovertarget'] });
      } catch (x) { mo = null; }
      explizit.set(k, { el: v, mo: mo });
    }
    function aktion(k) {
      var a = String(k.getAttribute('popovertargetaction') || '').toLowerCase();
      return a === 'show' || a === 'hide' ? a : 'toggle';
    }
    [g.HTMLButtonElement, g.HTMLInputElement].forEach(function (K) {
      var P = K && K.prototype;
      if (!P) return;
      eig(P, 'popoverTargetElement', function () { return zielLesen(this); }, zielSetzen);
      eig(P, 'popoverTargetAction', function () { return aktion(this); },
        function (v) { this.setAttribute('popovertargetaction', String(v)); });
    });
    function knopfZiel(k) {
      var t = k.localName, typ = String(k.getAttribute('type') || '').toLowerCase();
      if (t === 'input' && !/^(button|reset|submit|image)$/.test(typ)) return null;
      if (t !== 'button' && t !== 'input') return null;
      if (k.disabled) return null;
      var senden = t === 'button' ? (typ !== 'button' && typ !== 'reset') : (typ === 'submit' || typ === 'image');
      if (k.form && (senden || typ === 'reset')) return null;
      return zielLesen(k);
    }
    try {
      Object.defineProperty(g, '__narsilPopoverAktivierung', {
        value: function (k, ziel) {
          var p = knopfZiel(k);
          if (!p) return;
          if (ziel && enthaeltS(p, ziel) && enthaeltS(k, p)) return;
          var a = aktion(k), zeigt = offen.has(p);
          if ((a === 'show' && zeigt) || (a === 'hide' && !zeigt)) return;
          // Release 2026-10-15: the button is the events' source.
          if (zeigt) verbergen(p, k);
          else { try { zeigen(p, k); } catch (x) {} }
        },
        configurable: true, writable: true, enumerable: false
      });
    } catch (e) {}
  }
})();

// -- Tabellen-DOM (HTML §4.9) --
// Release 11.10.2026: `table.rows`/`tBodies`/`tHead`/`tFoot`/`caption`,
// `insertRow`/`deleteRow`, `createTHead` & Co., `tr.cells`/`insertCell`/
// `deleteCell`/`rowIndex`/`sectionRowIndex`, `td.cellIndex`, `colSpan`/
// `rowSpan`. GEMESSEN vorher (WPT tabular-data): „insertRow is not a
// function", `tBodies.length` auf undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc) return;
  function proto(n) { return g[n] && g[n].prototype; }
  var T = proto('HTMLTableElement'), S = proto('HTMLTableSectionElement'),
    R = proto('HTMLTableRowElement'), C = proto('HTMLTableCellElement');
  if (!T || typeof T.insertRow === 'function') return;
  function ausnahme(text, name) {
    return typeof g.DOMException === 'function' ? new g.DOMException(text, name) : new Error(text);
  }
  function def(P, name, wert) {
    try { Object.defineProperty(P, name, { value: wert, configurable: true, writable: true, enumerable: true }); } catch (e) {}
  }
  function eig(P, name, lesen, schreiben) {
    try { Object.defineProperty(P, name, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  // Release 2026-10-15: only HTML elements count (HTML §4.9: "a tr element
  // child", "a thead element child" are HTML elements). MEASURED before
  // (WPT the-tr-element/rowIndex, createTBody, caption-methods): rows under
  // a `createElementNS("", "table")` had index 0, namespaced tbody/caption
  // children counted.
  var XHTML = 'http://www.w3.org/1999/xhtml';
  function html(el, name) { return !!el && el.localName === name && el.namespaceURI === XHTML; }
  function kinder(el, namen) {
    var raus = [];
    for (var c = el.firstElementChild; c; c = c.nextElementSibling) {
      if (namen.indexOf(c.localName) >= 0 && c.namespaceURI === XHTML) raus.push(c);
    }
    return raus;
  }
  function erstes(el, name) { return kinder(el, [name])[0] || null; }
  function sammlung(liste) {
    liste.item = function (i) { return this[i] || null; };
    liste.namedItem = function (n) {
      for (var i = 0; i < this.length; i++) if (this[i].id === n || this[i].getAttribute('name') === n) return this[i];
      return null;
    };
    return liste;
  }
  function zeilen(t) {
    var raus = [];
    var kopf = erstes(t, 'thead'), fuss = erstes(t, 'tfoot');
    if (kopf) raus = raus.concat(kinder(kopf, ['tr']));
    for (var c = t.firstElementChild; c; c = c.nextElementSibling) {
      if (html(c, 'tr')) raus.push(c);
      else if (html(c, 'tbody')) raus = raus.concat(kinder(c, ['tr']));
    }
    if (fuss) raus = raus.concat(kinder(fuss, ['tr']));
    return raus;
  }
  function neu(name) { return doc.createElement(name); }
  function zeileEinfuegen(liste, eltern, i, tag) {
    i = i === undefined ? -1 : Number(i) | 0;
    if (i < -1 || i > liste.length) throw ausnahme('Index ausserhalb', 'IndexSizeError');
    var el = neu(tag);
    eltern.insertBefore(el, i === -1 || i === liste.length ? null : liste[i]);
    return el;
  }
  function zeileLoeschen(liste, i) {
    i = Number(i) | 0;
    if (i === -1) { if (liste.length) liste[liste.length - 1].remove(); return; }
    if (i < 0 || i >= liste.length) throw ausnahme('Index ausserhalb', 'IndexSizeError');
    liste[i].remove();
  }
  // Release 2026-10-15: `rows`, `tBodies`, `cells` are the host's live
  // HTMLCollections (`__narsilLive:<name>`); the snapshot stays as fallback.
  function live(el, n) { var c = el['__narsilLive:' + n]; return c && typeof c === 'object' ? c : null; }
  // `table`
  eig(T, 'rows', function () { return live(this, 'rows') || sammlung(zeilen(this)); });
  eig(T, 'tBodies', function () { return live(this, 'tBodies') || sammlung(kinder(this, ['tbody'])); });
  // The setters take their IDL types (`HTMLTableCaptionElement?`,
  // `HTMLTableSectionElement?`): anything else is a TypeError.
  function typ(v, name, attr) {
    var K = g[name];
    if (v !== null && v !== undefined && !(typeof K === 'function' && v instanceof K))
      throw new TypeError("Failed to set the '" + attr + "' property on 'HTMLTableElement': Failed to convert value to '" + name + "'.");
  }
  eig(T, 'caption', function () { return erstes(this, 'caption'); }, function (v) {
    typ(v, 'HTMLTableCaptionElement', 'caption');
    var alt = erstes(this, 'caption');
    if (alt) alt.remove();
    if (v) this.insertBefore(v, this.firstChild);
  });
  eig(T, 'tHead', function () { return erstes(this, 'thead'); }, function (v) {
    typ(v, 'HTMLTableSectionElement', 'tHead');
    if (v && v.localName !== 'thead') throw ausnahme('tHead braucht ein thead', 'HierarchyRequestError');
    var alt = erstes(this, 'thead');
    if (alt) alt.remove();
    if (v) {
      var vor = null;
      for (var c = this.firstElementChild; c; c = c.nextElementSibling) {
        if (c.localName !== 'caption' && c.localName !== 'colgroup') { vor = c; break; }
      }
      this.insertBefore(v, vor);
    }
  });
  eig(T, 'tFoot', function () { return erstes(this, 'tfoot'); }, function (v) {
    typ(v, 'HTMLTableSectionElement', 'tFoot');
    if (v && v.localName !== 'tfoot') throw ausnahme('tFoot braucht ein tfoot', 'HierarchyRequestError');
    var alt = erstes(this, 'tfoot');
    if (alt) alt.remove();
    if (v) this.appendChild(v);
  });
  def(T, 'createCaption', function createCaption() {
    var c = erstes(this, 'caption');
    if (!c) { c = neu('caption'); this.insertBefore(c, this.firstChild); }
    return c;
  });
  def(T, 'deleteCaption', function deleteCaption() { var c = erstes(this, 'caption'); if (c) c.remove(); });
  def(T, 'createTHead', function createTHead() {
    var k = erstes(this, 'thead');
    if (!k) { k = neu('thead'); this.tHead = k; }
    return k;
  });
  def(T, 'deleteTHead', function deleteTHead() { var k = erstes(this, 'thead'); if (k) k.remove(); });
  def(T, 'createTFoot', function createTFoot() {
    var f = erstes(this, 'tfoot');
    if (!f) { f = neu('tfoot'); this.appendChild(f); }
    return f;
  });
  def(T, 'deleteTFoot', function deleteTFoot() { var f = erstes(this, 'tfoot'); if (f) f.remove(); });
  def(T, 'createTBody', function createTBody() {
    var b = neu('tbody'), alle = kinder(this, ['tbody']);
    var letzt = alle[alle.length - 1];
    this.insertBefore(b, letzt ? letzt.nextSibling : null);
    return b;
  });
  def(T, 'insertRow', function insertRow(i) {
    i = i === undefined ? -1 : Number(i) | 0;
    var alle = zeilen(this);
    if (i < -1 || i > alle.length) throw ausnahme('Index ausserhalb der Zeilen', 'IndexSizeError');
    var tr = neu('tr');
    if (alle.length === 0) {
      var koerper = kinder(this, ['tbody']);
      if (koerper.length) koerper[koerper.length - 1].appendChild(tr);
      else { var b = neu('tbody'); b.appendChild(tr); this.appendChild(b); }
    } else if (i === -1 || i === alle.length) {
      alle[alle.length - 1].parentNode.appendChild(tr);
    } else {
      alle[i].parentNode.insertBefore(tr, alle[i]);
    }
    return tr;
  });
  def(T, 'deleteRow', function deleteRow(i) { zeileLoeschen(zeilen(this), i); });
  // `thead`/`tbody`/`tfoot`
  if (S) {
    eig(S, 'rows', function () { return live(this, 'rows') || sammlung(kinder(this, ['tr'])); });
    def(S, 'insertRow', function insertRow(i) { return zeileEinfuegen(kinder(this, ['tr']), this, i, 'tr'); });
    def(S, 'deleteRow', function deleteRow(i) { zeileLoeschen(kinder(this, ['tr']), i); });
  }
  // `tr`
  if (R) {
    eig(R, 'cells', function () { return live(this, 'cells') || sammlung(kinder(this, ['td', 'th'])); });
    eig(R, 'rowIndex', function () {
      var p = this.parentElement;
      var t = p && (html(p, 'table') ? p : p.parentElement);
      if (!html(t, 'table')) return -1;
      if (p !== t && !(html(p, 'thead') || html(p, 'tbody') || html(p, 'tfoot'))) return -1;
      return zeilen(t).indexOf(this);
    });
    eig(R, 'sectionRowIndex', function () {
      var p = this.parentElement;
      if (!(html(p, 'table') || html(p, 'thead') || html(p, 'tbody') || html(p, 'tfoot'))) return -1;
      return kinder(p, ['tr']).indexOf(this);
    });
    def(R, 'insertCell', function insertCell(i) { return zeileEinfuegen(kinder(this, ['td', 'th']), this, i, 'td'); });
    def(R, 'deleteCell', function deleteCell(i) { zeileLoeschen(kinder(this, ['td', 'th']), i); });
  }
  // `td`/`th`
  if (C) {
    eig(C, 'cellIndex', function () {
      var p = this.parentElement;
      return html(p, 'tr') ? kinder(p, ['td', 'th']).indexOf(this) : -1;
    });
    // Release 2026-10-15: colSpan/rowSpan (clamped), scope (enumerated),
    // ch/chOff (`char`/`charoff`), abbr, headers, axis, bgColor come from
    // the reflection table (HTML §2.6.1). MEASURED before (WPT
    // reflection-tabular 390 failures): parseInt accepted "\v7", scope was
    // the raw value, `ch` read the attribute "ch".
  }
})();

// -- Invoker commands: commandfor/command, CommandEvent (HTML §4.10.6) --
// Release 2026-10-15: a button with `commandfor` fires a cancelable
// `command` event (CommandEvent: command, source retargeted against the
// current target) at its target, then runs the built-in command:
// toggle-/show-/hide-popover on popovers, show-modal/close/request-close on
// dialogs; custom commands ("--x") only fire the event. `commandForElement`
// is an Element reflection, `command` an enumerated one, and a button with
// commandfor or command and no valid `type` is of type "button".
// MEASURED before (WPT the-button-element/command-and-commandfor, 213
// failures): CommandEvent was not defined, commandfor did nothing.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var BP = g.HTMLButtonElement && g.HTMLButtonElement.prototype;
  if (!doc || !BP || typeof g.Event !== 'function') return;
  var XHTML = 'http://www.w3.org/1999/xhtml';
  function shadowInklusivVorfahr(a, n) {
    for (var x = n, i = 0; x && i < 4096; i++) {
      if (x === a) return true;
      x = x.nodeType === 11 && x.host ? x.host : x.parentNode;
    }
    return false;
  }
  // DOM §2.5 "retarget A against B".
  function umzielen(a, b) {
    for (var i = 0; a && i < 64; i++) {
      var r = typeof a.getRootNode === 'function' ? a.getRootNode() : null;
      if (!r || r.nodeType !== 11 || !r.host) return a;
      if (b && shadowInklusivVorfahr(r, b)) return a;
      a = r.host;
    }
    return a;
  }
  if (typeof g.CommandEvent !== 'function') {
    var DATEN = new WeakMap();
    var CE = function CommandEvent(type, init) {
      if (!new.target) throw new TypeError("Failed to construct 'CommandEvent': Please use the 'new' operator.");
      if (arguments.length < 1) throw new TypeError("Failed to construct 'CommandEvent': 1 argument required, but only 0 present.");
      init = init === undefined || init === null ? {} : init;
      var quelle = init.source === undefined ? null : init.source;
      if (quelle !== null && !(quelle instanceof g.Element)) throw new TypeError("Failed to construct 'CommandEvent': Failed to read the 'source' property from 'CommandEventInit': Failed to convert value to 'Element'.");
      var e = new g.Event(type, init);
      Object.setPrototypeOf(e, new.target.prototype);
      DATEN.set(e, { command: init.command === undefined ? '' : String(init.command), source: quelle });
      return e;
    };
    CE.prototype = Object.create(g.Event.prototype);
    Object.defineProperty(CE.prototype, 'constructor', { value: CE, writable: true, configurable: true });
    Object.defineProperty(CE.prototype, 'command', { configurable: true, enumerable: true,
      get: function () { var d = DATEN.get(this); if (!d) throw new TypeError('Illegal invocation'); return d.command; } });
    Object.defineProperty(CE.prototype, 'source', { configurable: true, enumerable: true,
      get: function () {
        var d = DATEN.get(this);
        if (!d) throw new TypeError('Illegal invocation');
        return d.source === null ? null : umzielen(d.source, this.currentTarget);
      } });
    try { Object.defineProperty(CE.prototype, Symbol.toStringTag, { value: 'CommandEvent', configurable: true }); } catch (e) {}
    try { Object.defineProperty(g, 'CommandEvent', { value: CE, writable: true, configurable: true, enumerable: false }); } catch (e) {}
  }
  // Release 2026-10-15: `ToggleEvent.source` (HTML §6.12: the invoker,
  // retargeted against the current target like CommandEvent.source).
  // MEASURED before (WPT toggleevent-interface, popover-toggle-source):
  // `source` was undefined.
  var TE = g.ToggleEvent;
  if (typeof TE === 'function' && !Object.getOwnPropertyDescriptor(TE.prototype, 'source')) {
    var QUELLE = new WeakMap();
    var NTE = function ToggleEvent(type, init) {
      if (!new.target) throw new TypeError("Failed to construct 'ToggleEvent': Please use the 'new' operator.");
      if (arguments.length < 1) throw new TypeError("Failed to construct 'ToggleEvent': 1 argument required, but only 0 present.");
      init = init === undefined || init === null ? {} : init;
      var quelle = init.source === undefined ? null : init.source;
      if (quelle !== null && !(quelle instanceof g.Element)) throw new TypeError("Failed to construct 'ToggleEvent': Failed to read the 'source' property from 'ToggleEventInit': Failed to convert value to 'Element'.");
      var e = new TE(type, init);
      if (new.target !== NTE) Object.setPrototypeOf(e, new.target.prototype);
      QUELLE.set(e, quelle);
      return e;
    };
    NTE.prototype = TE.prototype;
    Object.defineProperty(TE.prototype, 'constructor', { value: NTE, writable: true, configurable: true });
    Object.defineProperty(TE.prototype, 'source', { configurable: true, enumerable: true,
      get: function () { var q = QUELLE.get(this); return q ? umzielen(q, this.currentTarget) : null; } });
    try { Object.defineProperty(g, 'ToggleEvent', { value: NTE, writable: true, configurable: true, enumerable: false }); } catch (e) {}
  }
  var BEFEHLE = ['toggle-popover', 'show-popover', 'hide-popover', 'close', 'request-close', 'show-modal'];
  // The command attribute's state: a keyword (lowercase), a custom command
  // ("--x", as written), or null (Unknown).
  function befehl(btn) {
    var a = btn.getAttribute('command');
    if (a === null) return null;
    var k = a.toLowerCase();
    if (BEFEHLE.indexOf(k) >= 0) return k;
    return a.slice(0, 2) === '--' ? a : null;
  }
  try {
    Object.defineProperty(BP, 'command', { configurable: true, enumerable: true,
      get: function () { var b = befehl(this); return b === null ? '' : b; },
      set: function (v) { this.setAttribute('command', String(v)); } });
  } catch (e) {}
  // `commandForElement` (Element reflection, HTML §2.6.1).
  var EXPLIZIT = new WeakMap();
  function gueltig(el, ziel) {
    if (!ziel || typeof ziel.getRootNode !== 'function') return false;
    var zr = ziel.getRootNode();
    for (var r = el.getRootNode(), i = 0; r && i < 64; i++) {
      if (zr === r && ziel !== r) return true;
      if (r.nodeType === 11 && r.host) r = r.host.getRootNode(); else break;
    }
    return false;
  }
  function perId(el, id) {
    var r = el.getRootNode();
    if ((r.nodeType === 9 || r.nodeType === 11) && typeof r.getElementById === 'function') return r.getElementById(id);
    if (r.nodeType === 1) {
      if (r.id === id) return r;
      var alle = r.querySelectorAll('[id]');
      for (var i = 0; i < alle.length; i++) if (alle[i].id === id) return alle[i];
    }
    return null;
  }
  function ziel(btn) {
    var a = btn.getAttribute('commandfor');
    var x = EXPLIZIT.get(btn);
    if (x && a === '') return gueltig(btn, x) ? x : null;
    if (a === null || a === '') return null;
    return perId(btn, a);
  }
  try {
    Object.defineProperty(BP, 'commandForElement', { configurable: true, enumerable: true,
      get: function () { return ziel(this); },
      set: function (v) {
        if (v === null || v === undefined) { EXPLIZIT.delete(this); this.removeAttribute('commandfor'); return; }
        if (!(v instanceof g.Element)) throw new TypeError("Failed to set the 'commandForElement' property on 'HTMLButtonElement': Failed to convert value to 'Element'.");
        this.setAttribute('commandfor', '');
        EXPLIZIT.set(this, v);
      } });
  } catch (e) {}
  function offenesPopover(el) { try { return el.matches(':popover-open'); } catch (e) { return false; } }
  try {
    Object.defineProperty(g, '__narsilBefehlAktivierung', { configurable: true, writable: true, enumerable: false, value: function (btn) {
      var typ = btn.type;
      if ((typ === 'submit' || typ === 'reset') && btn.form) return false;
      var z = ziel(btn);
      if (!z) return false;
      var b = befehl(btn);
      if (b === null) return true;
      // A dialog command must suit its target, an HTML dialog (no event
      // otherwise); popover and custom commands always fire.
      if (b.slice(0, 2) !== '--' && !/-popover$/.test(b) && !(z.localName === 'dialog' && z.namespaceURI === XHTML)) return true;
      var ev = g.__narsilUA(g.CommandEvent, 'command', { command: b, source: btn, cancelable: true, composed: true });
      if (!z.dispatchEvent(ev) || !z.isConnected || b.slice(0, 2) === '--') return true;
      var popover = z.hasAttribute('popover');
      var wert = btn.hasAttribute('value') ? btn.getAttribute('value') : undefined;
      try {
        if (b === 'hide-popover') { if (popover && offenesPopover(z)) z.hidePopover(); }
        else if (b === 'toggle-popover') { if (popover) z.togglePopover({ source: btn }); }
        else if (b === 'show-popover') { if (popover && !offenesPopover(z)) z.showPopover({ source: btn }); }
        else if (z.localName === 'dialog' && z.namespaceURI === XHTML) {
          if (b === 'show-modal') { if (!z.hasAttribute('open') && z.isConnected && !offenesPopover(z)) z.showModal(); }
          else if (b === 'close') { if (z.hasAttribute('open')) z.close(wert); }
          else if (b === 'request-close') { if (z.hasAttribute('open')) z.requestClose(wert); }
        }
      } catch (e) {}
      return true;
    } });
  } catch (e) {}
})();

// -- headingOffset / headingReset (whatwg/html#11086) --
// Release 2026-10-15: `headingOffset` reflects `headingoffset` (an integer,
// negative or invalid is 0), `headingReset` the boolean `headingreset`; the
// level itself is the selector engine's (`:heading(n)`). MEASURED before
// (WPT sections/headingoffset-*, 100 failures): both were undefined and
// `:heading` was unknown.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var HP = g.HTMLElement && g.HTMLElement.prototype;
  if (!HP || Object.getOwnPropertyDescriptor(HP, 'headingOffset')) return;
  try {
    Object.defineProperty(HP, 'headingOffset', { configurable: true, enumerable: true,
      get: function () {
        var m = /^[\t\n\f\r ]*([-+]?)(\d+)/.exec(this.getAttribute('headingoffset') || '');
        if (!m) return 0;
        var v = parseInt(m[2], 10) * (m[1] === '-' ? -1 : 1);
        return v < 0 ? 0 : Math.min(v, 2147483647);
      },
      set: function (v) { v = Number(v) >>> 0; this.setAttribute('headingoffset', String(v > 2147483647 ? 0 : v)); } });
    Object.defineProperty(HP, 'headingReset', { configurable: true, enumerable: true,
      get: function () { return this.hasAttribute('headingreset'); },
      set: function (v) { if (v) this.setAttribute('headingreset', ''); else this.removeAttribute('headingreset'); } });
  } catch (e) {}
})();

// -- Document.body setter, lastModified (HTML §3.1.3) --
// Release 2026-10-15: the body setter takes a `body`/`frameset` element
// (an HTMLElement, else TypeError; other elements HierarchyRequestError),
// replaces the current body or appends to the document element. The
// getter is the host's. `lastModified` is the current local time as
// "MM/DD/YYYY hh:mm:ss" (no Last-Modified date known). MEASURED before
// (WPT Document.body 14, document-lastModified-01 3 failures): the setter
// did nothing, lastModified was undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var DP = g.Document && g.Document.prototype;
  if (!DP) return;
  var XHTML = 'http://www.w3.org/1999/xhtml';
  function html(e, names) { return !!e && e.nodeType === 1 && e.namespaceURI === XHTML && names.indexOf(e.localName) >= 0; }
  function bodyOf(d) {
    var r = d.documentElement;
    if (!html(r, ['html'])) return null;
    for (var c = r.firstElementChild; c; c = c.nextElementSibling) if (html(c, ['body', 'frameset'])) return c;
    return null;
  }
  function fehler(text) { return new g.DOMException("Failed to set the 'body' property on 'Document': " + text, 'HierarchyRequestError'); }
  try {
    Object.defineProperty(DP, 'body', { configurable: true, enumerable: true,
      get: function () { return bodyOf(this); },
      set: function (v) {
        if (v !== null && !(v instanceof g.HTMLElement)) throw new TypeError("Failed to set the 'body' property on 'Document': The provided value is not of type 'HTMLElement'.");
        if (!html(v, ['body', 'frameset'])) throw fehler('The new body element must be a body or frameset element.');
        var b = bodyOf(this);
        if (v === b) return;
        if (b) { b.parentNode.replaceChild(v, b); return; }
        var r = this.documentElement;
        if (!r) throw fehler('No document element exists.');
        r.appendChild(v);
      } });
  } catch (e) {}
  // A document without a browsing context (createHTMLDocument,
  // createDocument, DOMParser) is "complete" (HTML §3.1.3: its readiness
  // never changes); the main document's state is the host's.
  if (!Object.getOwnPropertyDescriptor(DP, 'readyState')) {
    try { Object.defineProperty(DP, 'readyState', { configurable: true, enumerable: true, get: function () { return 'complete'; } }); } catch (e) {}
  }
  // `self.origin` (HTML §8.1.3.3): the serialized origin of the page.
  if (!('origin' in g) && g.location) {
    try {
      Object.defineProperty(g, 'origin', { configurable: true, enumerable: true,
        get: function () { try { return g.location.origin; } catch (e) { return 'null'; } },
        set: function (v) { Object.defineProperty(g, 'origin', { value: v, writable: true, configurable: true, enumerable: true }); } });
    } catch (e) {}
  }
  function zwei(n) { return (n < 10 ? '0' : '') + n; }
  if (!Object.getOwnPropertyDescriptor(DP, 'lastModified')) {
    try {
      Object.defineProperty(DP, 'lastModified', { configurable: true, enumerable: true, get: function () {
        var d = new Date();
        return zwei(d.getMonth() + 1) + '/' + zwei(d.getDate()) + '/' + d.getFullYear() + ' ' +
          zwei(d.getHours()) + ':' + zwei(d.getMinutes()) + ':' + zwei(d.getSeconds());
      } });
    } catch (e) {}
  }
})();

// -- Rahmen: iframe/frame mit eigenem Dokument (HTML §4.8.5, §7.3) --
// Release 11.10.2026: `contentWindow`/`contentDocument`, Laden von `src`
// und `srcdoc` (anfangs `about:blank`), die Skripte des Rahmens mit EIGENEM
// `window`/`document`/`parent`/`top`/`location`, `load` am Rahmen,
// `postMessage`, Navigation (`location.href = …`), Formulare und Links mit
// `target` auf einen Rahmen. Die Engine hat EINEN globalen Bereich: das
// Fenster eines Rahmens erbt vom Hauptfenster (gleiche Konstruktoren), und
// seine Skripte laufen in `with (fenster) { … }` — ihre obersten
// Deklarationen werden ins Rahmenfenster exportiert, damit das naechste
// Skript desselben Rahmens sie sieht. GEMESSEN vorher (WPT Lauf i): 598
// gescheiterte Skripttests benutzen iframes; `contentWindow` war
// `undefined` („Failed to create new rendered document", 45 Dateien).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || !g.HTMLIFrameElement) return;
  var protos = [g.HTMLIFrameElement && g.HTMLIFrameElement.prototype, g.HTMLFrameElement && g.HTMLFrameElement.prototype].filter(Boolean);
  if (!protos.length || Object.getOwnPropertyDescriptor(protos[0], 'contentWindow')) return;
  var zustand = new WeakMap();
  var dokFenster = new WeakMap();
  // Release 11.10.2026: Dokument -> Rahmen (fuer document.write/close).
  var dokRahmen = new WeakMap();
  try {
    Object.defineProperty(g, '__narsilRahmenSkript', {
      value: function (d, code) { var z = dokRahmen.get(d); if (!z) return false; ausfuehren(z, code); return true; },
      configurable: true, writable: true, enumerable: false
    });
    Object.defineProperty(g, '__narsilRahmenFertig', {
      // Release 12.10.2026: `DOMContentLoaded` und `load` kommen als AUFGABEN
      // (HTML §13.2.7 „the end": „queue a global task"), nicht im Aufruf von
      // `document.close()`. GEMESSEN vorher (WPT custom-elements/registries/
      // per-document): der `load`-Handler des Rahmens rief wieder
      // `open()`/`close()` — Endlosrekursion, Stapelueberlauf, Prozess tot.
      value: function (d) {
        var z = dokRahmen.get(d);
        if (!z) return;
        z.laeuft = false;
        var nr = z.fertigNr = (z.fertigNr || 0) + 1;
        g.setTimeout(function () {
          if (z.fertigNr !== nr) return;
          try { d.dispatchEvent(g.__narsilUA(g.Event, 'DOMContentLoaded', { bubbles: true })); } catch (e) {}
          g.setTimeout(function () {
            if (z.fertigNr !== nr) return;
            try { feuern(z.win, 'load'); } catch (e) {}
            if (verbunden(z.el)) { try { z.el.dispatchEvent(g.__narsilUA(g.Event, 'load')); } catch (e) {} }
          }, 0);
        }, 0);
      },
      configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
  try { dokFenster.set(doc, g); } catch (e) {}
  var alleRahmen = [];

  function eig(o, name, lesen, schreiben) {
    try { Object.defineProperty(o, name, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  function wert(o, name, w) {
    try { Object.defineProperty(o, name, { value: w, configurable: true, writable: true, enumerable: true }); } catch (e) { try { o[name] = w; } catch (e2) {} }
  }
  function herkunft(url) {
    try { return new URL(url).origin; } catch (e) { return ''; }
  }
  var eigeneHerkunft = herkunft(doc.URL || (g.location && g.location.href) || '');
  function basis() { return doc.baseURI || (g.location && g.location.href) || ''; }
  function absolut(u, von) {
    try { return new URL(String(u), von || basis()).href; } catch (e) { return String(u); }
  }
  function verbunden(el) {
    if (typeof el.isConnected === 'boolean') return el.isConnected;
    for (var n = el; n; n = n.parentNode) if (n.nodeType === 9) return true;
    return false;
  }
  function fensterVonDokument(d) {
    return (d && dokFenster.get(d)) || g;
  }
  function leeresDokument() {
    var d = doc.implementation.createHTMLDocument('');
    var t = d.querySelector && d.querySelector('title');
    if (t) t.remove();
    return d;
  }
  function ortObjekt(z) {
    var o = {};
    var teil = function (name) {
      return function () {
        if (z.url === 'about:blank' || z.url === 'about:srcdoc') return name === 'href' ? z.url : name === 'protocol' ? 'about:' : name === 'pathname' ? z.url.slice(6) : '';
        try { return new URL(z.url)[name]; } catch (e) { return ''; }
      };
    };
    ['protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash', 'origin'].forEach(function (n) { eig(o, n, teil(n), n === 'hash' ? function (v) {
      var u = new URL(z.url); u.hash = String(v); var alt = z.url; z.url = u.href; feuern(z.win, 'hashchange', { oldURL: alt, newURL: z.url });
    } : undefined); });
    eig(o, 'href', teil('href'), function (v) { navigieren(z, absolut(v, z.url)); });
    wert(o, 'assign', function assign(v) { navigieren(z, absolut(v, z.url)); });
    wert(o, 'replace', function replace(v) { navigieren(z, absolut(v, z.url)); });
    wert(o, 'reload', function reload() { navigieren(z, z.url); });
    wert(o, 'toString', function toString() { return z.url; });
    return o;
  }

  // Ereignisse am Rahmenfenster (kein Knoten): eigene Lauscherliste, dazu
  // die `on…`-Eigenschaften.
  function feuern(win, typ, mehr) {
    var ev;
    try { ev = typ === 'message' && typeof g.MessageEvent === 'function' ? g.__narsilUA(g.MessageEvent, 'message', mehr || {}) : g.__narsilUA(g.Event, typ); } catch (e) { ev = { type: typ }; }
    if (mehr && typ !== 'message') for (var k in mehr) wert(ev, k, mehr[k]);
    win.dispatchEvent(ev);
    return ev;
  }
  function ereignisZiel(win) {
    var lauscher = {};
    wert(win, 'addEventListener', function addEventListener(typ, f, opt) {
      if (!f) return;
      var l = lauscher[typ] || (lauscher[typ] = []);
      for (var i = 0; i < l.length; i++) if (l[i].f === f) return;
      l.push({ f: f, einmal: !!(opt && typeof opt === 'object' && opt.once) });
    });
    wert(win, 'removeEventListener', function removeEventListener(typ, f) {
      var l = lauscher[typ];
      if (!l) return;
      for (var i = 0; i < l.length; i++) if (l[i].f === f) { l.splice(i, 1); return; }
    });
    wert(win, 'dispatchEvent', function dispatchEvent(ev) {
      try { Object.defineProperty(ev, 'target', { value: win, configurable: true }); } catch (e) {}
      try { Object.defineProperty(ev, 'currentTarget', { value: win, configurable: true }); } catch (e) {}
      var h = win['on' + ev.type];
      if (typeof h === 'function') { try { h.call(win, ev); } catch (e) { melden(e); } }
      var l = (lauscher[ev.type] || []).slice();
      for (var i = 0; i < l.length; i++) {
        if (l[i].einmal) win.removeEventListener(ev.type, l[i].f);
        try { typeof l[i].f === 'function' ? l[i].f.call(win, ev) : l[i].f.handleEvent(ev); } catch (e) { melden(e); }
      }
      return !ev.defaultPrevented;
    });
    ['onload', 'onmessage', 'onerror', 'onhashchange', 'onunload', 'onbeforeunload', 'onresize', 'onpageshow', 'onpagehide', 'onpopstate', 'onmessageerror', 'onDOMContentLoaded'].forEach(function (n) { wert(win, n, null); });
  }
  function melden(e) {
    try { if (g.console && g.console.error) g.console.error(e); } catch (e2) {}
  }

  function rahmenZustand(el) {
    var z = zustand.get(el);
    if (z) return z;
    z = { el: el, url: 'about:blank', dok: leeresDokument(), ladung: 0, exporte: [], laeuft: false };
    var eltern = fensterVonDokument(el.ownerDocument);
    var win = Object.create(g);
    z.win = win;
    wert(win, 'window', win);
    wert(win, 'self', win);
    wert(win, 'frames', win);
    wert(win, 'globalThis', win);
    eig(win, 'document', function () { return z.dok; });
    wert(win, 'parent', eltern);
    eig(win, 'top', function () { return eltern === g ? g : eltern.top; });
    wert(win, 'opener', null);
    wert(win, 'closed', false);
    wert(win, 'length', 0);
    eig(win, 'frameElement', function () { return z.el; });
    eig(win, 'name', function () { return z.el.getAttribute('name') || ''; }, function (v) { z.el.setAttribute('name', String(v)); });
    var ort = ortObjekt(z);
    eig(win, 'location', function () { return ort; }, function (v) { navigieren(z, absolut(v, z.url)); });
    wert(win, 'history', { length: 1, state: null, back: function () {}, forward: function () {}, go: function () {}, pushState: function () {}, replaceState: function () {} });
    eig(win, 'innerWidth', function () { var r = z.el.getBoundingClientRect && z.el.getBoundingClientRect(); return Math.round((r && r.width) || Number(z.el.getAttribute('width')) || 300); });
    eig(win, 'innerHeight', function () { var r = z.el.getBoundingClientRect && z.el.getBoundingClientRect(); return Math.round((r && r.height) || Number(z.el.getAttribute('height')) || 150); });
    ereignisZiel(win);
    wert(win, 'postMessage', function postMessage(daten, ziel) {
      var quelle = g.__narsilRahmenFenster || g;
      // Structured clone, transfer and target origin (`messaging.js`).
      if (typeof g.__narsilPostPrep === 'function') {
        var make = g.__narsilPostPrep(arguments, eigeneHerkunft, eigeneHerkunft);
        // The handlers run as scripts of this frame (`event.source` of what
        // they post is this window, their origin the frame's).
        if (make) setTimeout(function () {
          var vorher = g.__narsilRahmenFenster;
          g.__narsilRahmenFenster = win;
          try { win.dispatchEvent(make(quelle)); } finally { g.__narsilRahmenFenster = vorher; }
        }, 0);
        return;
      }
      setTimeout(function () { feuern(win, 'message', { data: daten, origin: eigeneHerkunft, source: quelle }); }, 0);
    });
    // Release 12.10.2026: waehrend der Auswertung ist das eigene `eval` weg —
    // sonst faende `with (__w) { eval(__c) }` wieder diese Huelle statt des
    // eingebauten `eval` (Endlosrekursion). GEMESSEN vorher (WPT
    // Range-deleteContents, Range-test-iframe.html `eval(testRangeInput)`):
    // „Maximum call stack size exceeded" und die 15-s-Wanduhrgrenze.
    var rahmenEval = function (code) {
      try { delete win.eval; } catch (e) {}
      try { return ausfuehren(z, String(code), true); } finally { wert(win, 'eval', rahmenEval); }
    };
    wert(win, 'eval', rahmenEval);
    wert(win, 'close', function close() {});
    wert(win, 'focus', function focus() {});
    wert(win, 'blur', function blur() {});
    wert(win, 'stop', function stop() { z.ladung++; });
    dokumentAnbinden(z, z.dok);
    zustand.set(el, z);
    alleRahmen.push(z);
    return z;
  }
  function dokumentAnbinden(z, d) {
    try { dokFenster.set(d, z.win); } catch (e) {}
    try { dokRahmen.set(d, z); } catch (e) {}
    // Release 11.10.2026: der Host layoutet das Dokument mit der Groesse des
    // Iframes (`zwangslayout::rahmen_layouten`).
    try { if (typeof g.__narsilRahmenAnmelden === 'function') g.__narsilRahmenAnmelden(z.el, d); } catch (e) {}
    try { Object.defineProperty(d, 'defaultView', { get: function () { return verbunden(z.el) ? z.win : null; }, configurable: true }); } catch (e) {}
    try { Object.defineProperty(d, 'location', { get: function () { return z.win.location; }, set: function (v) { z.win.location = v; }, configurable: true }); } catch (e) {}
    try { Object.defineProperty(d, 'URL', { get: function () { return z.url; }, configurable: true }); } catch (e) {}
    try { Object.defineProperty(d, 'documentURI', { get: function () { return z.url; }, configurable: true }); } catch (e) {}
    // The frame element gives about:blank/srcdoc documents their fallback
    // base URL (see `__narsilFallbackBase`).
    try { Object.defineProperty(d, '__narsilFrameElement', { get: function () { return z.el; }, configurable: true }); } catch (e) {}
    try { Object.defineProperty(d, 'readyState', { get: function () { return z.laeuft ? 'loading' : 'complete'; }, configurable: true }); } catch (e) {}
  }

  // Die obersten Deklarationen eines Skripts (Klammertiefe 0, ohne
  // Zeichenketten und Kommentare; auch in einer Zeile wie bei minifiziertem
  // Code) — sie werden ins Rahmenfenster exportiert (siehe Kopf).
  function deklarationen(code, funktionen) {
    var namen = [], tiefe = 0, i = 0, n = code.length, inVar = false, erwarteName = false;
    function dazu(w) { if (namen.indexOf(w) < 0 && namen.length < 2000) namen.push(w); }
    while (i < n) {
      var c = code.charAt(i);
      if (c === '/' && code.charAt(i + 1) === '/') { i = code.indexOf('\n', i); if (i < 0) break; continue; }
      if (c === '/' && code.charAt(i + 1) === '*') { i = code.indexOf('*/', i + 2); if (i < 0) break; i += 2; continue; }
      if (c === '"' || c === "'" || c === '`') {
        i++;
        while (i < n && code.charAt(i) !== c) { if (code.charAt(i) === '\\') i++; i++; }
        i++;
        continue;
      }
      if (c === '{' || c === '(' || c === '[') { tiefe++; i++; continue; }
      if (c === '}' || c === ')' || c === ']') { tiefe--; i++; continue; }
      if (tiefe === 0) {
        if (c === ';') { inVar = false; erwarteName = false; i++; continue; }
        if (c === ',' && inVar) { erwarteName = true; i++; continue; }
        var vor = i > 0 ? code.charAt(i - 1) : '';
        if (/[A-Za-z_$]/.test(c) && !/[\w$.]/.test(vor)) {
          var w = /^[A-Za-z_$][\w$]*/.exec(code.slice(i, i + 80))[0];
          if (erwarteName) { dazu(w); erwarteName = false; }
          else if (w === 'var' || w === 'let' || w === 'const') { inVar = true; erwarteName = true; }
          else if (w === 'function' || w === 'class') {
            var r = /^(?:function|class)\s*\*?\s*([A-Za-z_$][\w$]*)/.exec(code.slice(i, i + 200));
            if (r) dazu(r[1]);
            if (r && w === 'function' && funktionen && funktionen.length < 2000) funktionen.push(r[1]);
            inVar = false;
          }
          i += w.length;
          continue;
        }
      }
      i++;
    }
    return namen;
  }
  // Release 12.10.2026: das Rahmenfenster erbt die EINGEBAUTEN Globalen
  // (Object.create(g)), aber nicht die, die Skripte der Elternseite angelegt
  // haben — die werden am Rahmenfenster mit `undefined` verdeckt (sonst fand
  // ein Rahmenskript sie ueber die Kette oder ueber `with`). GEMESSEN vorher
  // (Agent Range, WPT dom/ranges): im Rahmen war `'setup' in window` wahr,
  // `common.js` rief dort den testharness der Eltern.
  var grundNamen = null;
  function abschirmen(win) {
    var namen = g.__narsilGrundNamen;
    if (!namen) return;
    if (!grundNamen) {
      grundNamen = Object.create(null);
      for (var i = 0; i < namen.length; i++) grundNamen[namen[i]] = true;
    }
    var alle;
    try { alle = Object.getOwnPropertyNames(g); } catch (e) { return; }
    for (var j = 0; j < alle.length && j < 50000; j++) {
      var n = alle[j];
      if (grundNamen[n] || n.indexOf('__narsil') === 0) continue;
      if (Object.prototype.hasOwnProperty.call(win, n)) continue;
      try { Object.defineProperty(win, n, { value: undefined, writable: true, configurable: true, enumerable: false }); } catch (e) {}
    }
  }
  // Release 13.10.2026: die eval-Hilfe nur EINMAL uebersetzen, und das
  // Verdecken der Eltern-Globalen nur einmal je Rahmendokument. GEMESSEN
  // vorher (Agent Range, Range-insertNode): jedes `contentWindow.eval` lief
  // ueber alle Namen des globalen Objekts und ein neues `new Function` —
  // rund 5 ms je Teiltest, die Datei lief in die 60-s-Frist.
  var evalHilfe = null;
  // Release 13.10.2026: die Ereignis-Inhaltsattribute am `<body>` eines
  // Rahmendokuments gehoeren dem Fenster (HTML §8.1.8.2 „Window-reflecting
  // body element event handler set"). GEMESSEN vorher (WPT webstorage
  // event_*): `<body onstorage=…>` im Rahmen lief nie.
  var FENSTER_HANDLER = ['onstorage', 'onload', 'onmessage', 'onmessageerror', 'onhashchange', 'onpopstate',
    'onpagehide', 'onpageshow', 'onunload', 'onbeforeunload', 'onresize', 'onerror', 'onoffline', 'ononline',
    'onlanguagechange', 'onfocus', 'onblur', 'onscroll'];
  function koerperHandler(z, d) {
    var b = d && d.body;
    if (!b || typeof b.getAttribute !== 'function') return;
    FENSTER_HANDLER.forEach(function (n) {
      var code = b.getAttribute(n);
      if (code === null || code === undefined) return;
      var f;
      try { f = ausfuehren(z, '(function (event) {\n' + code + '\n})', true); } catch (e) { melden(e); return; }
      if (typeof f === 'function') { try { z.win[n] = f; } catch (e) {} }
    });
  }
  // Release 13.10.2026: `storage`-Ereignisse (HTML §12.2.1 „broadcast"): an
  // jedes ANDERE Fenster gleicher Herkunft (die Seite und ihre Rahmen), als
  // Aufgabe. `quelle` ist das Fenster, dessen Skript den Speicher aenderte.
  function speicherMelden(bereich, schluessel, alt, neu, quelle) {
    var url = '';
    try { url = quelle && quelle !== g && quelle.document ? quelle.document.URL : doc.URL; } catch (e) { url = doc.URL; }
    var ziele = [];
    if (quelle && quelle !== g) ziele.push(g);
    alleRahmen.forEach(function (z) {
      if (z.win === quelle || !z.dok || !verbunden(z.el)) return;
      if (z.url !== 'about:blank' && z.url !== 'about:srcdoc' && herkunft(z.url) !== eigeneHerkunft) return;
      ziele.push(z.win);
    });
    if (!ziele.length) return;
    setTimeout(function () {
      ziele.forEach(function (w) {
        var ev;
        var init = { key: schluessel, oldValue: alt, newValue: neu, url: url, storageArea: bereich };
        try { ev = g.__narsilUA(g.StorageEvent, 'storage', init); } catch (e) { ev = null; }
        if (!ev) return;
        try { w.dispatchEvent(ev); } catch (e) { melden(e); }
      });
    }, 0);
  }
  try { Object.defineProperty(g, '__narsilSpeicherMelden', { value: speicherMelden, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  function ausfuehren(z, code, ergebnis) {
    if (z.abgeschirmt !== z.dok) {
      abschirmen(z.win);
      z.abgeschirmt = z.dok;
    }
    // Release 12.10.2026: Funktionsdeklarationen gelten ab dem Beginn des
    // Skripts (gehoben) — sie gehen auch dann ans Rahmenfenster, wenn das
    // Skript spaeter wirft. GEMESSEN vorher (WPT Range-insertNode,
    // Range-test-iframe.html): `testDiv.style` warf am Ende, `run` fehlte
    // („run is not a function", 1.840 Teiltests je Datei).
    var funktionen = [];
    var namen = ergebnis ? [] : deklarationen(code, funktionen);
    var ausfuhr = namen.map(function (n) { return 'try{__w[' + JSON.stringify(n) + ']=' + n + '}catch(__e){}'; }).join(';');
    var vorab = funktionen.map(function (n) { return 'try{__w[' + JSON.stringify(n) + ']=' + n + '}catch(__e){}'; }).join(';');
    // Release 12.10.2026: die `var`-Namen des Skripts gehoeren dem Rahmen-
    // fenster VON ANFANG AN (wie Globale) — dann geht `var x = …` ueber das
    // `with` direkt ans Fenster, und `contentWindow.x` zeigt immer den
    // aktuellen Wert (nicht eine Kopie vom Skriptende).
    for (var vi = 0; vi < namen.length; vi++) {
      var vn = namen[vi];
      if (funktionen.indexOf(vn) >= 0 || vn in z.win) continue;
      try { Object.defineProperty(z.win, vn, { value: undefined, writable: true, configurable: true, enumerable: true }); } catch (e) {}
    }
    var vorher = g.__narsilRahmenFenster;
    g.__narsilRahmenFenster = z.win;
    try {
      if (ergebnis) {
        if (!evalHilfe) evalHilfe = new Function('__w', '__c', 'with (__w) { return eval(__c); }');
        return evalHilfe.call(z.win, z.win, code);
      }
      new Function('__w', 'with (__w) {' + vorab + ';\n' + code + '\n;' + ausfuhr + '\n}').call(z.win, z.win);
    } catch (e) {
      melden(e);
      try { feuern(z.win, 'error', { message: String(e && e.message || e), error: e }); } catch (e2) {}
      if (ergebnis) throw e;
    } finally {
      g.__narsilRahmenFenster = vorher;
    }
  }
  function skripteAusfuehren(z, nr, weiter) {
    var liste = Array.prototype.slice.call(z.dok.querySelectorAll('script'));
    var i = 0;
    (function naechstes() {
      if (nr !== z.ladung) return;
      if (i >= liste.length) return weiter();
      var s = liste[i++];
      var typ = (s.getAttribute('type') || '').trim().toLowerCase();
      if (typ && !/^(text|application)\/(x-)?(java|ecma)script$|^module$/.test(typ)) return naechstes();
      var src = s.getAttribute('src');
      if (src) {
        g.fetch(absolut(src, z.url)).then(function (r) { return r.text(); }).then(function (code) {
          if (nr !== z.ladung) return;
          ausfuehren(z, code);
          naechstes();
        }, function () { naechstes(); });
      } else {
        ausfuehren(z, s.textContent || '');
        naechstes();
      }
    })();
  }
  function fertig(z, nr) {
    if (nr !== z.ladung) return;
    z.laeuft = false;
    // Erst nach dem `load` des Rahmens (unten) darf das der Seite kommen —
    // die Freigabe wirkt erst im naechsten Takt des Hosts.
    setTimeout(function () { if (!z.laeuft) rahmenSperre(z, false); }, 0);
    try { z.dok.dispatchEvent(g.__narsilUA(g.Event, 'DOMContentLoaded', { bubbles: true })); } catch (e) {}
    feuern(z.win, 'load');
    if (verbunden(z.el)) {
      try { z.el.dispatchEvent(g.__narsilUA(g.Event, 'load')); } catch (e) { melden(e); }
    }
  }
  function einsetzen(z, nr, text, url, typ, kodierung) {
    if (nr !== z.ladung) return;
    var d;
    typ = String(typ || '').toLowerCase();
    // Release 15.10.2026: the document's contentType is the MIME type essence
    // of the response (HTML documents: text/html). MEASURED before (Agent D,
    // WPT Document-createElement-namespace): an iframe-loaded .html file
    // reported application/xml.
    var mime = typ.split(';')[0].trim() || 'text/html';
    try {
      if (/^text\/plain/.test(typ)) {
        d = leeresDokument();
        var pre = d.createElement('pre');
        pre.textContent = text;
        d.body.appendChild(pre);
      } else if (/xml/.test(typ)) {
        // Release 13.10.2026 (Agent Range, mit Freigabe): auch XHTML ist XML
        // (`application/xhtml+xml`). GEMESSEN (WPT dom/nodes, 17 Dateien mit
        // XHTML-Rahmen): Document-createElementNS 540->585,
        // createElement-namespace 37->49; als text/html landete das "\n"
        // nach </html> im body.
        d = new g.DOMParser().parseFromString(text, /svg/.test(typ) ? 'image/svg+xml' : /xhtml/.test(typ) ? 'application/xhtml+xml' : 'application/xml');
      } else if (/^image\//.test(typ)) {
        d = leeresDokument();
        var img = d.createElement('img');
        img.setAttribute('src', url);
        d.body.appendChild(img);
      } else {
        // A navigated frame document allows declarative shadow DOM (HTML
        // §8.4.2); DOMParser does not. MEASURED before (WPT
        // declarative-shadow-dom-opt-in): <template shadowrootmode> in an
        // iframe stayed a template.
        if (typeof doc.__narsilHtmlParsen === 'function') {
          try { d = doc.__narsilHtmlParsen(String(text), true); } catch (e) { d = null; }
        }
        if (!d) d = new g.DOMParser().parseFromString(text, 'text/html');
        mime = 'text/html';
      }
    } catch (e) {
      d = leeresDokument();
      mime = 'text/html';
    }
    // XML documents keep the type their parser gave them.
    if (!/xml/.test(mime)) {
      try { Object.defineProperty(d, 'contentType', { get: function () { return mime; }, configurable: true }); } catch (e) {}
    }
    z.dok = d;
    z.url = url;
    z.laeuft = true;
    if (kodierung && kodierung !== 'UTF-8') {
      ['characterSet', 'charset', 'inputEncoding'].forEach(function (n) {
        try { Object.defineProperty(d, n, { get: function () { return kodierung; }, configurable: true }); } catch (e) {}
      });
    }
    dokumentAnbinden(z, d);
    koerperHandler(z, d);
    skripteAusfuehren(z, nr, function () { fertig(z, nr); });
  }
  // Release 11.10.2026: aufgeschobene Lazy-Rahmen laden, sobald sie in
  // die Naehe der Sicht kommen (Bildlauf oder Layout). GEMESSEN vorher (WPT
  // iframe-loading-lazy-*): `verzoegert` wurde nie wieder aufgegriffen —
  // der Rahmen lud nie.
  var lazyRahmen = [], lazyLaeuft = false;
  function lageGeschaetzt(el) {
    try { var m = typeof g.__narsilMass === 'function' ? g.__narsilMass(el) : null; return !m || m[5] === true; } catch (e) { return false; }
  }
  function lazyBeobachten(z) {
    if (lazyRahmen.indexOf(z) < 0) lazyRahmen.push(z);
    if (!lazyLaeuft) { lazyLaeuft = true; setTimeout(lazyPruefenRahmen, 250); }
  }
  function lazyPruefenRahmen() {
    lazyLaeuft = false;
    var l = lazyRahmen.slice();
    lazyRahmen = [];
    l.forEach(function (z) {
      if (!z.verzoegert) return;
      var r = null;
      try { r = z.el.getBoundingClientRect(); } catch (e) { r = null; }
      var sh = g.innerHeight || 600, sw = g.innerWidth || 800, rand = 1250;
      if (!z.el.isConnected) { lazyRahmen.push(z); return; }
      // `loading` nicht mehr `lazy` (eager oder entfernt): sofort laden.
      var nochLazy = String(z.el.getAttribute('loading') || '').toLowerCase() === 'lazy';
      if (nochLazy && lageGeschaetzt(z.el)) { lazyRahmen.push(z); return; }
      if (nochLazy && r && (r.top > sh + rand || r.bottom < -rand || r.left > sw + rand || r.right < -rand)) { lazyRahmen.push(z); return; }
      var u = z.verzoegert;
      z.verzoegert = null;
      laden(z, u);
    });
    if (lazyRahmen.length) { lazyLaeuft = true; setTimeout(lazyPruefenRahmen, 250); }
  }
  try { g.addEventListener('scroll', function () { if (lazyRahmen.length && !lazyLaeuft) { lazyLaeuft = true; setTimeout(lazyPruefenRahmen, 0); } }, true); } catch (e) {}
  // Release 12.10.2026: ein Rahmen, der VOR dem `load` der Seite laedt,
  // haelt es zurueck (HTML §4.8.5 „delay the load event"; der Host liest
  // `__narsilLadeSperre`, hoechstens 8 s). GEMESSEN vorher (WPT
  // encoding/*-decode*.html, 25 Dateien): `<body onload>` las den Rahmen,
  // bevor er geladen war — 0 Teiltests, Harness-Zeitueberschreitung.
  function rahmenSperre(z, an) {
    if (an && doc.readyState === 'complete') return;
    if (!!z.sperrt === !!an) return;
    z.sperrt = !!an;
    g.__narsilLadeSperre = Math.max(0, (g.__narsilLadeSperre | 0) + (an ? 1 : -1));
  }
  // Release 12.10.2026: die Kodierung der Antwort (HTML §13.2.3.2: BOM,
  // `charset` des Content-Type, `<meta>` in den ersten 1024 Bytes, sonst
  // UTF-8) und ihr Name in der Schreibweise von `document.characterSet`.
  // GEMESSEN vorher: jeder Rahmen wurde als UTF-8 gelesen (`r.text()`),
  // euckr_chars.html meldete `characterSet` UTF-8 und falsche Zeichen.
  function kodierungVon(bytes, typ) {
    if (bytes.length >= 3 && bytes[0] === 0xEF && bytes[1] === 0xBB && bytes[2] === 0xBF) return 'utf-8';
    if (bytes.length >= 2 && bytes[0] === 0xFE && bytes[1] === 0xFF) return 'utf-16be';
    if (bytes.length >= 2 && bytes[0] === 0xFF && bytes[1] === 0xFE) return 'utf-16le';
    var m = /charset\s*=\s*["']?([^"';\s]+)/i.exec(typ || '');
    if (m) return m[1];
    var kopf = '';
    for (var i = 0; i < bytes.length && i < 1024; i++) kopf += String.fromCharCode(bytes[i]);
    m = /<meta[^>]*?charset\s*=\s*["']?\s*([^"'\s;>\/]+)/i.exec(kopf);
    if (m) {
      var l = m[1].toLowerCase();
      // §13.2.3.2: UTF-16 aus dem Prescan ist UTF-8, x-user-defined windows-1252.
      if (/^utf-16/.test(l)) return 'utf-8';
      if (l === 'x-user-defined') return 'windows-1252';
      return l;
    }
    return 'utf-8';
  }
  function kanonisch(n) {
    if (n === 'shift_jis') return 'Shift_JIS';
    if (n === 'big5') return 'Big5';
    if (n === 'gbk') return 'GBK';
    if (/^(utf-|iso-|koi8-|euc-)/.test(n) || n === 'ibm866') return n.toUpperCase();
    return n;
  }
  function dekodieren(puffer, typ) {
    var bytes = new Uint8Array(puffer);
    var dek;
    try { dek = new g.TextDecoder(kodierungVon(bytes, typ)); } catch (e) { dek = new g.TextDecoder('utf-8'); }
    return { text: dek.decode(bytes), kodierung: kanonisch(dek.encoding || 'utf-8') };
  }
  function laden(z, url, anfrage) {
    var nr = ++z.ladung;
    var erstes = nr === 1 && url === undefined;
    if (url === undefined) {
      var srcdoc = z.el.getAttribute('srcdoc');
      var src = z.el.getAttribute(z.el.localName === 'object' ? 'data' : 'src');
      if (srcdoc !== null && z.el.localName === 'iframe') {
        rahmenSperre(z, true);
        setTimeout(function () { einsetzen(z, nr, String(srcdoc), 'about:srcdoc', 'text/html'); }, 0);
        return;
      }
      url = 'about:blank';
      if (src && src.trim()) {
        // Release 13.10.2026: eine Adresse, die sich nicht parsen laesst, ist
        // about:blank (HTML §4.8.5 Schritt „If url is failure"). GEMESSEN
        // vorher (WPT bad-src-means-about-blank): der Rahmen holte die
        // kaputte Adresse und meldete nie `load`.
        try { url = new URL(src.trim(), basis()).href; } catch (e) { url = 'about:blank'; }
      }
      // Release 11.10.2026: `loading=lazy` (HTML §2.5.7): weit ausserhalb der
      // Sicht wird nicht geladen (Chrome: 1250 px Abstand). GEMESSEN vorher:
      // jeder Rahmen lud sofort (WPT `iframe-loading-lazy-in-scroller-far`).
      if (z.el.localName === 'iframe' && String(z.el.getAttribute('loading') || '').toLowerCase() === 'lazy' && /^https?:/i.test(url)) {
        var r = null;
        try { r = z.el.getBoundingClientRect(); } catch (e) { r = null; }
        var sh = g.innerHeight || 600, sw = g.innerWidth || 800, rand = 1250;
        // Vor dem ersten Layout ist die Lage nur geschaetzt — dann warten.
        if (lageGeschaetzt(z.el) || (r && (r.top > sh + rand || r.bottom < -rand || r.left > sw + rand || r.right < -rand))) {
          // Ein aufgeschobener Rahmen haelt `load` nicht zurueck (§2.5.7).
          z.verzoegert = url;
          rahmenSperre(z, false);
          lazyBeobachten(z);
          return;
        }
      }
    }
    // Release 13.10.2026: HTML §4.8.5 „process the iframe attributes"
    // Schritt 3: beim ERSTEN Einfuegen ohne `src` (about:blank) bleibt das
    // anfaengliche Dokument, und das `load` des Rahmens kommt SOFORT — es
    // haelt nichts zurueck. GEMESSEN vorher (WPT iframe-nosrc): ein neues
    // Dokument per setTimeout ersetzte `contentDocument`, und `load` kam
    // nach den Lauschern des Skripts.
    if (erstes && url === 'about:blank') {
      z.url = 'about:blank';
      if (!z.angebunden) { z.angebunden = true; dokumentAnbinden(z, z.dok); }
      z.laeuft = false;
      rahmenSperre(z, false);
      // Erkennt erst ein Getter das Einfuegen (`contentDocument` …), war das
      // synchrone `load` laengst vorbei (vor den folgenden Skriptzeilen).
      if (verbunden(z.el) && !z.ohneLoad) {
        try { z.el.dispatchEvent(g.__narsilUA(g.Event, 'load')); } catch (e) { melden(e); }
      }
      return;
    }
    rahmenSperre(z, true);
    if (/^javascript:/i.test(url)) {
      setTimeout(function () {
        if (nr !== z.ladung) return;
        // Release 14.10.2026: evaluate the javascript: URL (HTML
        // §7.4.2.3.4): a string result becomes the frame's new document (then
        // `load`); any other result navigates nowhere — no new document and
        // no further `load` (the first load of a frame still completes).
        // MEASURED before (WPT iframe_javascript_url_remove_srcdoc,
        // execution-timing 040): a second `load` after a javascript: URL
        // without result, and a string result never became the document.
        var erg, code = url.slice(11);
        try { code = decodeURIComponent(code); } catch (e) {}
        try { erg = ausfuehren(z, code, true); } catch (e) {}
        if (nr !== z.ladung) return;
        if (typeof erg === 'string') { einsetzen(z, nr, erg, z.url || 'about:blank', 'text/html'); return; }
        if (erstes) { fertig(z, nr); return; }
        z.laeuft = false;
        setTimeout(function () { if (!z.laeuft) rahmenSperre(z, false); }, 0);
      }, 0);
      return;
    }
    if (url === 'about:blank' || !/^(https?|blob|data):/i.test(url)) {
      setTimeout(function () {
        if (nr !== z.ladung) return;
        z.dok = leeresDokument();
        z.url = 'about:blank';
        dokumentAnbinden(z, z.dok);
        fertig(z, nr);
      }, 0);
      return;
    }
    // Nur der Teil hinter `#` aendert sich: kein Neuladen (HTML §7.4.2.3.3).
    if (z.url !== 'about:blank' && url.split('#')[0] === z.url.split('#')[0] && url.indexOf('#') >= 0 && !anfrage) {
      var alt = z.url;
      z.url = url;
      setTimeout(function () { feuern(z.win, 'hashchange', { oldURL: alt, newURL: url }); }, 0);
      return;
    }
    z.laeuft = true;
    g.fetch(url, anfrage || {}).then(function (r) {
      var typ = (r.headers && r.headers.get && r.headers.get('content-type')) || '';
      if (typeof r.arrayBuffer === 'function' && typeof g.TextDecoder === 'function' && !/^image\//i.test(typ)) {
        return r.arrayBuffer().then(function (b) {
          var d = dekodieren(b, typ);
          return { text: d.text, url: r.url || url, typ: typ, kodierung: d.kodierung };
        });
      }
      return r.text().then(function (t) { return { text: t, url: r.url || url, typ: typ }; });
    }).then(function (a) {
      einsetzen(z, nr, a.text, a.url, a.typ, a.kodierung);
    }, function () {
      fertig(z, nr);
    });
  }
  function navigieren(z, url, anfrage) {
    laden(z, url, anfrage);
  }

  protos.forEach(function (P) {
    eig(P, 'contentWindow', function () {
      if (!verbunden(this)) return null;
      var z = rahmenZustand(this);
      if (!z.eingefuegt) { moEin(); z.ohneLoad = true; eingefuegt(this); z.ohneLoad = false; }
      return z.win;
    });
    eig(P, 'contentDocument', function () {
      if (!verbunden(this)) return null;
      var z = rahmenZustand(this);
      if (!z.eingefuegt) { moEin(); z.ohneLoad = true; eingefuegt(this); z.ohneLoad = false; }
      if (z.url !== 'about:blank' && z.url !== 'about:srcdoc' && herkunft(z.url) !== eigeneHerkunft) return null;
      return z.dok;
    });
    try { Object.defineProperty(P, 'getSVGDocument', { value: function getSVGDocument() { return this.contentDocument; }, configurable: true, writable: true }); } catch (e) {}
  });

  // Einfuegen, Entfernen und `src`/`srcdoc`-Aenderungen.
  function istRahmen(n) { return n && n.nodeType === 1 && (n.localName === 'iframe' || n.localName === 'frame'); }
  function rahmenIn(n, f) {
    if (!n || n.nodeType !== 1) return;
    if (istRahmen(n)) f(n);
    if (n.querySelectorAll) Array.prototype.forEach.call(n.querySelectorAll('iframe, frame'), f);
  }
  function eingefuegt(el) {
    if (!verbunden(el)) return;
    var z = rahmenZustand(el);
    if (z.eingefuegt) return;
    z.eingefuegt = true;
    laden(z);
  }
  function entfernt(el, bewegt) {
    var z = zustand.get(el);
    // Release 13.10.2026: ein VERSCHOBENER Rahmen (entfernt und wieder
    // eingefuegt) verliert seinen Browsing-Kontext und bekommt einen neuen
    // (HTML §4.8.5 „removing steps"/„insertion steps"). GEMESSEN vorher (WPT
    // move_iframe_in_dom_*): der alte Inhalt blieb, `load` kam nie.
    if (!z || (verbunden(el) && !bewegt)) return;
    z.ladung++;
    rahmenSperre(z, false);
    zustand.delete(el);
    var i = alleRahmen.indexOf(z);
    if (i >= 0) alleRahmen.splice(i, 1);
    feuern(z.win, 'unload');
    if (bewegt && verbunden(el)) eingefuegt(el);
  }
  if (typeof g.MutationObserver === 'function') {
    // Nur arbeiten, wenn es Rahmen gibt (lebende Listen; echte Seiten haendern
    // ihr DOM staendig).
    var iframeListe = doc.getElementsByTagName('iframe'), frameListe = doc.getElementsByTagName('frame');
    var beobachter = new g.MutationObserver(function (eintraege) {
      if (!alleRahmen.length && !iframeListe.length && !frameListe.length) return;
      eintraege.forEach(function (e) {
        if (e.type === 'childList') {
          Array.prototype.forEach.call(e.addedNodes || [], function (n) { rahmenIn(n, eingefuegt); });
          Array.prototype.forEach.call(e.removedNodes || [], function (n) {
            rahmenIn(n, function (el) { entfernt(el, verbunden(el)); });
          });
        } else if (e.type === 'attributes' && istRahmen(e.target) && verbunden(e.target)) {
          var z = rahmenZustand(e.target);
          z.eingefuegt = true;
          if (e.attributeName === 'src' || e.attributeName === 'srcdoc') laden(z);
        }
      });
    });
  }
  // Der Beobachter schaltet das Aenderungsprotokoll des lebenden DOM ein —
  // das kostet auf DOM-lastigen Seiten. Er laeuft deshalb erst, wenn es
  // Rahmen gibt: ein `iframe`/`frame` wird angelegt, steht im HTML, oder
  // jemand fragt nach `contentWindow`.
  var moAktiv = false;
  function moEin() {
    if (moAktiv || !beobachter) return;
    moAktiv = true;
    try { beobachter.observe(doc, { childList: true, subtree: true, attributes: true, attributeFilter: ['src', 'srcdoc'] }); } catch (e) {}
  }
  var DP = g.Document && g.Document.prototype;
  if (DP && typeof DP.createElement === 'function') {
    var altAnlegen = DP.createElement;
    wert(DP, 'createElement', function createElement(name) {
      var el = altAnlegen.apply(this, arguments);
      if (!moAktiv && /^i?frame$/i.test(String(name))) moEin();
      return el;
    });
  }
  function anfangsRahmen() {
    var l = doc.querySelectorAll('iframe, frame');
    if (l.length) moEin();
    Array.prototype.forEach.call(l, eingefuegt);
  }
  if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', anfangsRahmen);
  else setTimeout(anfangsRahmen, 0);
  try { g.addEventListener('load', anfangsRahmen); } catch (e) {}

  // `window.frames`/`window.length`/`window[i]` (HTML §7.2.2).
  function rahmenListe() {
    return Array.prototype.filter.call(doc.querySelectorAll('iframe, frame'), verbunden);
  }
  function verborgen(o, name, lesen) {
    try { Object.defineProperty(o, name, { get: lesen, configurable: true, enumerable: false }); } catch (e) {}
  }
  verborgen(g, 'length', function () { return rahmenListe().length; });
  for (var i = 0; i < 32; i++) (function (i) {
    if (Object.getOwnPropertyDescriptor(g, String(i))) return;
    verborgen(g, String(i), function () { var l = rahmenListe(); return l[i] ? l[i].contentWindow : undefined; });
  })(i);

  // Formulare und Links mit `target` auf einen Rahmen (HTML §4.10.21.3,
  // §4.6.4): die Antwort geht in den Rahmen, nicht in die Seite.
  function rahmenNamens(name) {
    if (!name || /^_(self|parent|top|blank)$/i.test(name)) return null;
    var l = rahmenListe();
    for (var i = 0; i < l.length; i++) if (l[i].getAttribute('name') === name) return rahmenZustand(l[i]);
    return null;
  }
  var FP = g.HTMLFormElement && g.HTMLFormElement.prototype;
  if (FP) {
    var altSenden = FP.submit, altAnfordern = FP.requestSubmit;
    function zielVon(form, absender) {
      var t = absender && absender.getAttribute && absender.getAttribute('formtarget');
      if (t === null || t === undefined) t = form.getAttribute('target');
      return rahmenNamens(t);
    }
    // Release 12.10.2026: der urlencoded-Koerper in der Formularkodierung
    // (`accept-charset`, sonst die des Dokuments; siehe
    // `__narsilFormKodieren`). Dateien gehen mit ihrem Namen ein.
    function formKodiert(form, fd) {
      var etiketten = form.getAttribute('accept-charset') || '';
      var kod = typeof g.__narsilFormKodieren === 'function'
        ? function (t) { return g.__narsilFormKodieren(t, etiketten); }
        : function (t) { return encodeURIComponent(t).replace(/%20/g, '+'); };
      var teile = [];
      fd.forEach(function (v, k) {
        if (typeof v !== 'string') v = v && typeof v.name === 'string' ? v.name : String(v);
        teile.push(kod(String(k)) + '=' + kod(v));
      });
      return teile.join('&');
    }
    function senden(form, absender, z) {
      var wo = (absender && absender.getAttribute('formaction')) || form.getAttribute('action') || '';
      var url = absolut(wo || doc.URL);
      var methode = ((absender && absender.getAttribute('formmethod')) || form.getAttribute('method') || 'get').toLowerCase();
      var art = ((absender && absender.getAttribute('formenctype')) || form.getAttribute('enctype') || 'application/x-www-form-urlencoded').toLowerCase();
      var fd;
      try { fd = new g.FormData(form, absender || undefined); } catch (e) { fd = new g.FormData(form); }
      if (methode === 'dialog') return;
      if (methode === 'post') {
        var koerper = art === 'multipart/form-data' ? fd : formKodiert(form, fd);
        var kopf = art === 'multipart/form-data' ? {} : { 'Content-Type': art === 'text/plain' ? 'text/plain' : 'application/x-www-form-urlencoded' };
        if (art === 'text/plain') {
          var zeilen = [];
          fd.forEach(function (v, k) { zeilen.push(k + '=' + v); });
          koerper = zeilen.join('\r\n') + '\r\n';
        }
        navigieren(z, url, { method: 'POST', body: koerper, headers: kopf });
      } else {
        var u = new URL(url);
        u.search = formKodiert(form, fd);
        navigieren(z, u.href, { method: 'GET' });
      }
    }
    // Release 11.10.2026: `submit()`/`requestSubmit()` (HTML §4.10.21.3
    // „form submission algorithm"). GEMESSEN vorher: beide fehlten ganz
    // („submit is not a function", WPT form-submission-0 u. a.) — auch ein
    // Klick auf einen Sendeknopf sendete nie.
    function absenden(form, absender) {
      if (!form.isConnected) return;
      var z = zielVon(form, absender);
      if (z) return senden(form, absender, z);
      // Release 13.10.2026: ein Formular im Dokument eines Rahmens navigiert
      // ohne Ziel (bzw. `_self`) DIESEN Rahmen (HTML §4.10.21.3 Schritt 22
      // „the form's node navigable"). GEMESSEN vorher (WPT reparent-form-
      // during-planned-navigation-task): `submit()` im Rahmen tat nichts.
      var zt = (absender && absender.getAttribute('formtarget')) || form.getAttribute('target') || '';
      var eigen = dokRahmen.get(form.ownerDocument);
      if (eigen && (!zt || /^_self$/i.test(zt))) return senden(form, absender, eigen);
      var t = (absender && absender.getAttribute('formtarget')) || form.getAttribute('target') || '';
      var wo = (absender && absender.getAttribute('formaction')) || form.getAttribute('action') || '';
      var url = absolut(wo || doc.URL);
      var methode = ((absender && absender.getAttribute('formmethod')) || form.getAttribute('method') || 'get').toLowerCase();
      if (methode === 'dialog') {
        var dlg = form.closest && form.closest('dialog');
        if (dlg && typeof dlg.close === 'function') dlg.close(absender && absender.value !== undefined ? String(absender.value) : undefined);
        return;
      }
      var fd;
      try { fd = new g.FormData(form, absender || undefined); } catch (e) { fd = new g.FormData(form); }
      var u = new URL(url);
      if (methode !== 'post') u.search = formKodiert(form, fd);
      if (/^_blank$/i.test(t) && typeof g.open === 'function') { g.open(u.href); return; }
      if (/^javascript:/i.test(u.href)) return;
      if (g.location && typeof g.location.assign === 'function') g.location.assign(u.href);
    }
    function istSendeknopf(el) {
      if (!el || el.nodeType !== 1) return false;
      var t = el.localName, typ = String(el.getAttribute('type') || '').toLowerCase();
      if (t === 'button') return typ !== 'button' && typ !== 'reset';
      return t === 'input' && (typ === 'submit' || typ === 'image');
    }
    wert(FP, 'submit', function submit() { absenden(this, null); });
    wert(FP, 'requestSubmit', function requestSubmit(absender) {
      if (absender !== undefined && absender !== null) {
        if (!istSendeknopf(absender)) throw new TypeError("Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button.");
        if (absender.form !== this) throw new g.DOMException("Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not owned by this form element.", 'NotFoundError');
      } else absender = null;
      var form = this;
      // „form cannot navigate": ein nicht verbundenes Formular sendet nicht
      // (und feuert auch kein `submit`).
      if (!form.isConnected) return;
      var pruefen = !form.hasAttribute('novalidate') && !(absender && absender.hasAttribute('formnovalidate'));
      if (pruefen && typeof form.checkValidity === 'function' && !form.checkValidity()) return;
      var ev;
      try { ev = g.__narsilUA(g.SubmitEvent || g.Event, 'submit', { bubbles: true, cancelable: true, submitter: absender }); }
      catch (e) { ev = g.__narsilUA(g.Event, 'submit', { bubbles: true, cancelable: true }); }
      try { if (ev.submitter !== absender) Object.defineProperty(ev, 'submitter', { value: absender, configurable: true }); } catch (e) {}
      if (form.dispatchEvent(ev)) absenden(form, absender);
    });
  }
  // Links: als LETZTER Lauscher am Fenster, damit `preventDefault` der
  // Seite vorher greift.
  g.addEventListener('click', function (ev) {
    if (ev.defaultPrevented) return;
    var a = ev.target && ev.target.closest ? ev.target.closest('a[href], area[href]') : null;
    if (!a) return;
    var z = rahmenNamens(a.getAttribute('target'));
    if (!z) return;
    ev.preventDefault();
    navigieren(z, absolut(a.getAttribute('href')));
  });
})();

// -- DOMImplementation, XMLDocument, new Document() (DOM §4.5) und
//    CharacterData-Methoden (DOM §4.10/§4.11) --
// Release 11.10.2026: `document.implementation` ist ein eigenes
// `DOMImplementation`-Objekt; `createDocument` liefert ein `XMLDocument`
// (Doctype, Wurzelelement, `contentType` nach Namensraum); `new Document()`
// ist ein leeres XML-Dokument. Dazu `substringData`/`appendData`/
// `insertData`/`deleteData`/`replaceData` und `splitText`/`wholeText`.
// GEMESSEN vorher (WPT dom/nodes): „XMLDocument ist nicht definiert",
// `new Document()` hatte keine Methoden (common.js brach in 3 grossen
// Dateien ab), `substringData is not a function`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc) return;
  var D = g.Document, DI = g.DOMImplementation;
  var wirt = doc.implementation;
  if (!wirt || typeof wirt.createDocument !== 'function') return;
  function def(o, n, v) { try { Object.defineProperty(o, n, { value: v, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  function pflicht(n, args, was, wo) {
    if (args.length < n) throw new TypeError("Failed to execute '" + was + "' on '" + wo + "': " + n + ' argument' + (n > 1 ? 's' : '') + ' required, but only ' + args.length + ' present.');
  }
  if (D && typeof g.XMLDocument === 'undefined') {
    var XD = function XMLDocument() { throw new TypeError('Illegal constructor'); };
    XD.prototype = Object.create(D.prototype);
    def(XD.prototype, 'constructor', XD);
    g.XMLDocument = XD;
  }
  var dokumente = new WeakMap();
  function eigeneImpl(d) {
    if (!d || typeof d !== 'object') return d;
    var i = Object.create(DI ? DI.prototype : Object.prototype);
    dokumente.set(i, d);
    try { Object.defineProperty(d, 'implementation', { value: i, configurable: true, enumerable: true }); } catch (e) {}
    return d;
  }
  function textOderNull(v) { return v === null || v === undefined ? null : String(v); }
  if (DI) {
    var P = DI.prototype;
    def(P, 'createDocument', function createDocument(ns, q, dt) {
      pflicht(2, arguments, 'createDocument', 'DOMImplementation');
      if (dt !== undefined && dt !== null && !(g.DocumentType && dt instanceof g.DocumentType) && dt.nodeType !== 10) {
        throw new TypeError("Failed to execute 'createDocument' on 'DOMImplementation': parameter 3 is not of type 'DocumentType'.");
      }
      var d = wirt.createDocument(textOderNull(ns), q === null ? '' : String(q), dt === undefined ? null : dt);
      if (g.XMLDocument) { try { Object.setPrototypeOf(d, g.XMLDocument.prototype); } catch (e) {} }
      return eigeneImpl(d);
    });
    def(P, 'createHTMLDocument', function createHTMLDocument(titel) {
      return eigeneImpl(titel === undefined ? wirt.createHTMLDocument() : wirt.createHTMLDocument(String(titel)));
    });
    def(P, 'createDocumentType', function createDocumentType(q, oeff, sys) {
      pflicht(3, arguments, 'createDocumentType', 'DOMImplementation');
      // Release 13.10.2026: der Doctype gehoert dem Dokument DIESER
      // Implementierung (DOM §4.5.1). GEMESSEN vorher (WPT
      // DOMImplementation-createDocumentType): `ownerDocument` war bei
      // `doc.implementation` immer das Hauptdokument.
      var d = dokumente.get(this);
      if (d && d !== doc && typeof d.createDocumentType === 'function') return d.createDocumentType(String(q), String(oeff), String(sys));
      return wirt.createDocumentType(String(q), String(oeff), String(sys));
    });
    def(P, 'hasFeature', function hasFeature() { return true; });
  }
  eigeneImpl(doc);
  // `new Document()` (DOM §4.5): ein leeres XML-Dokument.
  if (D) {
    var NeuD = function Document() {
      if (!(this instanceof NeuD)) throw new TypeError("Failed to construct 'Document': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      return eigeneImpl(wirt.createDocument(null, '', null));
    };
    NeuD.prototype = D.prototype;
    Object.getOwnPropertyNames(D).forEach(function (n) {
      if (n === 'prototype' || n === 'length' || n === 'name' || n === 'caller' || n === 'arguments') return;
      try { Object.defineProperty(NeuD, n, Object.getOwnPropertyDescriptor(D, n)); } catch (e) {}
    });
    def(D.prototype, 'constructor', NeuD);
    try { Object.setPrototypeOf(NeuD, Object.getPrototypeOf(D)); } catch (e) {}
    g.Document = NeuD;
  }

  // -- CharacterData (DOM §4.10) --
  function ulong(v) {
    var n = Number(v);
    if (!isFinite(n)) return 0;
    n = (n < 0 ? Math.ceil(n) : Math.floor(n)) % 4294967296;
    return n < 0 ? n + 4294967296 : n;
  }
  function indexFehler(off, len) {
    return new g.DOMException('The offset ' + off + " is greater than the node's length (" + len + ').', 'IndexSizeError');
  }
  // Release 12.10.2026: „replace data" (DOM §4.10) im Host — mit den
  // lebenden Bereichen (ein `data =` setzte jeden Grenzpunkt auf 0).
  function ersetzen(knoten, off, anz, text) {
    var d = String(knoten.data), len = d.length;
    if (off > len) throw indexFehler(off, len);
    if (off + anz > len) anz = len - off;
    if (typeof knoten.__narsilDaten === 'function') { knoten.__narsilDaten(off, anz, text); return; }
    knoten.data = d.slice(0, off) + text + d.slice(off + anz);
  }
  var methoden = {
    substringData: function substringData(off, anz) {
      pflicht(2, arguments, 'substringData', 'CharacterData');
      off = ulong(off); anz = ulong(anz);
      var d = String(this.data), len = d.length;
      if (off > len) throw indexFehler(off, len);
      return d.substring(off, Math.min(len, off + anz));
    },
    appendData: function appendData(text) {
      pflicht(1, arguments, 'appendData', 'CharacterData');
      ersetzen(this, String(this.data).length, 0, String(text));
    },
    insertData: function insertData(off, text) {
      pflicht(2, arguments, 'insertData', 'CharacterData');
      ersetzen(this, ulong(off), 0, String(text));
    },
    deleteData: function deleteData(off, anz) {
      pflicht(2, arguments, 'deleteData', 'CharacterData');
      ersetzen(this, ulong(off), ulong(anz), '');
    },
    replaceData: function replaceData(off, anz, text) {
      pflicht(3, arguments, 'replaceData', 'CharacterData');
      ersetzen(this, ulong(off), ulong(anz), String(text));
    }
  };
  ['CharacterData', 'Text', 'Comment', 'ProcessingInstruction', 'CDATASection'].forEach(function (n) {
    var K = g[n], PR = K && K.prototype;
    if (!PR) return;
    Object.keys(methoden).forEach(function (m) { if (typeof PR[m] !== 'function') def(PR, m, methoden[m]); });
  });
  var TP = g.Text && g.Text.prototype;
  if (TP) {
    if (typeof TP.splitText !== 'function') def(TP, 'splitText', function splitText(off) {
      pflicht(1, arguments, 'splitText', 'Text');
      off = ulong(off);
      var d = String(this.data), len = d.length;
      if (off > len) throw indexFehler(off, len);
      // Release 12.10.2026: im Host (DOM §4.11 mit den Bereichsschritten).
      if (typeof this.__narsilTeilen === 'function') return this.__narsilTeilen(off);
      var od = this.ownerDocument || doc;
      var neu = this.nodeType === 4 && typeof od.createCDATASection === 'function' ? od.createCDATASection(d.substring(off)) : od.createTextNode(d.substring(off));
      var p = this.parentNode;
      if (p) p.insertBefore(neu, this.nextSibling);
      this.data = d.substring(0, off);
      return neu;
    });
    if (!('wholeText' in TP)) {
      try {
        Object.defineProperty(TP, 'wholeText', {
          configurable: true, enumerable: true,
          get: function () {
            var a = this, t = '';
            while (a.previousSibling && (a.previousSibling.nodeType === 3 || a.previousSibling.nodeType === 4)) a = a.previousSibling;
            for (; a && (a.nodeType === 3 || a.nodeType === 4); a = a.nextSibling) t += a.data;
            return t;
          }
        });
      } catch (e) {}
    }
  }
})();

// -- Attr (DOM §4.9.2) --
// Release 11.10.2026: echte `Attr`-Knoten (`nodeType` 2) mit Namensraum,
// Praefix und `ownerElement`, je Element und Name dasselbe Objekt;
// `getAttributeNode(NS)`, `setAttributeNode(NS)`, `removeAttributeNode`,
// `createAttribute(NS)`, und `NamedNodeMap` liefert dieselben Knoten.
// GEMESSEN vorher (WPT dom/nodes): `getAttributeNodeNS`/`createAttributeNS`
// „is not a function", `getAttributeNode` gab ein loses Objekt ohne
// Namensraum.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var E = g.Element && g.Element.prototype;
  if (!doc || !E) return;
  var HTML = 'http://www.w3.org/1999/xhtml';
  var A = g.Attr;
  if (typeof A !== 'function') return;
  var AP = A.prototype;
  try { if (g.Node && Object.getPrototypeOf(AP) !== g.Node.prototype) Object.setPrototypeOf(AP, g.Node.prototype); } catch (e) {}
  var I = new WeakMap(); // Attr -> Zustand
  var je = new WeakMap(); // Element -> { schluessel: Attr }
  function schluessel(ns, lokal) { return (ns || '') + '\u0000' + lokal; }
  function neu(ns, praefix, lokal, wert, el, od) {
    var a = Object.create(AP);
    I.set(a, { ns: ns || null, praefix: praefix || null, lokal: lokal, wert: wert, el: el || null, od: od || (el && el.ownerDocument) || doc });
    return a;
  }
  function zustand(a) {
    var z = I.get(a);
    if (!z) throw new TypeError('Illegal invocation');
    // Hat das Element das Attribut verloren, ist der Knoten losgeloest.
    if (z.el) {
      var v = z.el.getAttributeNS(z.ns, z.lokal);
      if (v === null) { z.el = null; } else z.wert = v;
    }
    return z;
  }
  function fuer(el, ns, lokal) {
    var v = el.getAttributeNS(ns, lokal);
    if (v === null) return null;
    var m = je.get(el);
    if (!m) { m = {}; je.set(el, m); }
    var k = schluessel(ns, lokal), a = m[k];
    if (a) {
      var z = I.get(a);
      if (z.el === el || z.el === null) { z.el = el; z.wert = v; return a; }
    }
    // Release 2026-10-15: only a namespaced attribute has a prefix (an HTML
    // element's `xml:lang` is one name without namespace).
    var praefix = null, q = qname(el, ns, lokal);
    if (ns && q && q.indexOf(':') > 0) praefix = q.slice(0, q.indexOf(':'));
    a = neu(ns, praefix, lokal, v, el);
    m[k] = a;
    return a;
  }
  // Release 12.10.2026: die Rohliste des Hosts (`el.attributes` baut dieser
  // Block selbst, siehe unten).
  function roh(el) { var r = el.__narsilAttributeRoh; return r && typeof r.length === 'number' ? r : []; }
  // Der qualifizierte Name eines Attributs aus `attributes`.
  function qname(el, ns, lokal) {
    var l = roh(el);
    for (var i = 0; l && i < l.length; i++) {
      var x = l[i];
      if (x && (x.namespaceURI || null) === (ns || null) && (x.localName || x.name) === lokal) return x.name;
    }
    return lokal;
  }
  function eig(name, lesen, schreiben) {
    try { Object.defineProperty(AP, name, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  function name(z) { return z.praefix ? z.praefix + ':' + z.lokal : z.lokal; }
  eig('namespaceURI', function () { return zustand(this).ns; });
  eig('prefix', function () { return zustand(this).praefix; });
  eig('localName', function () { return zustand(this).lokal; });
  eig('name', function () { return name(zustand(this)); });
  eig('nodeName', function () { return name(zustand(this)); });
  eig('nodeType', function () { return 2; });
  eig('specified', function () { return true; });
  eig('ownerElement', function () { return zustand(this).el; });
  eig('ownerDocument', function () { return zustand(this).od; });
  eig('parentNode', function () { return null; });
  eig('parentElement', function () { return null; });
  eig('firstChild', function () { return null; });
  eig('lastChild', function () { return null; });
  eig('previousSibling', function () { return null; });
  eig('nextSibling', function () { return null; });
  eig('isConnected', function () { return false; });
  eig('childNodes', function () { return []; });
  function wertLesen() { return zustand(this).wert; }
  function wertSetzen(v) {
    var z = zustand(this);
    v = String(v);
    if (z.el) z.el.setAttributeNS(z.ns, name(z), v);
    z.wert = v;
  }
  eig('value', wertLesen, wertSetzen);
  eig('nodeValue', wertLesen, function (v) { wertSetzen.call(this, v === null ? '' : v); });
  eig('textContent', wertLesen, function (v) { wertSetzen.call(this, v === null ? '' : v); });
  function def(o, n, f) { try { Object.defineProperty(o, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  def(AP, 'hasChildNodes', function hasChildNodes() { return false; });
  def(AP, 'cloneNode', function cloneNode() { var z = zustand(this); return neu(z.ns, z.praefix, z.lokal, z.wert, null, z.od); });
  def(AP, 'isEqualNode', function isEqualNode(o) {
    if (!o || o.nodeType !== 2) return false;
    var z = zustand(this);
    return o.namespaceURI === z.ns && o.localName === z.lokal && o.value === z.wert;
  });
  // Element-Methoden.
  def(E, 'getAttributeNode', function getAttributeNode(q) {
    q = String(q);
    if (this.namespaceURI === HTML) q = q.toLowerCase();
    var l = roh(this);
    for (var i = 0; l && i < l.length; i++) if (l[i] && l[i].name === q) return fuer(this, l[i].namespaceURI || null, l[i].localName || l[i].name);
    return null;
  });
  def(E, 'getAttributeNodeNS', function getAttributeNodeNS(ns, lokal) {
    return fuer(this, ns === undefined || ns === '' ? null : ns, String(lokal));
  });
  function setzen(el, a, mitNs) {
    if (!a || !I.has(a)) throw new TypeError("Failed to execute 'setAttributeNode' on 'Element': parameter 1 is not of type 'Attr'.");
    var z = zustand(a);
    if (z.el && z.el !== el) throw new g.DOMException('The node provided is an attribute node that is already an attribute of another Element; attribute nodes must be explicitly cloned.', 'InUseAttributeError');
    if (z.el === el) return a;
    var alt = mitNs || z.ns ? fuer(el, z.ns, z.lokal) : el.getAttributeNode(name(z));
    if (alt === a) return a;
    if (alt) { var za = I.get(alt); za.wert = el.getAttributeNS(za.ns, za.lokal); za.el = null; }
    el.setAttributeNS(z.ns, name(z), z.wert);
    z.el = el;
    var m = je.get(el);
    if (!m) { m = {}; je.set(el, m); }
    m[schluessel(z.ns, z.lokal)] = a;
    return alt || null;
  }
  def(E, 'setAttributeNode', function setAttributeNode(a) { return setzen(this, a, false); });
  def(E, 'setAttributeNodeNS', function setAttributeNodeNS(a) { return setzen(this, a, true); });
  def(E, 'removeAttributeNode', function removeAttributeNode(a) {
    if (!a || !I.has(a)) throw new TypeError("Failed to execute 'removeAttributeNode' on 'Element': parameter 1 is not of type 'Attr'.");
    var z = zustand(a);
    if (z.el !== this) throw new g.DOMException('The node provided is owned by another element.', 'NotFoundError');
    this.removeAttributeNS(z.ns, z.lokal);
    z.el = null;
    return a;
  });
  // Dokument-Fabriken.
  var DP = g.Document && g.Document.prototype;
  // Release 13.10.2026: „valid attribute local name" (DOM §1.4, 2024): nicht
  // leer, kein ASCII-Leerraum, NUL, `/`, `>`, `=`. GEMESSEN vorher (WPT
  // Document-createAttribute 22/36): `invalid^Name` warf.
  function namensPruefen(n) {
    if (n === '' || /[\t\n\f\r \u0000\/>=]/.test(n)) {
      throw new g.DOMException("Failed to execute 'createAttribute' on 'Document': The localName provided ('" + n + "') contains the invalid name-character.", 'InvalidCharacterError');
    }
  }
  function istHtmlDok(d) { var t = d && d.contentType; return !t || t === 'text/html'; }
  var fabrik = function createAttribute(n) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'createAttribute' on 'Document': 1 argument required, but only 0 present.");
    n = String(n);
    namensPruefen(n);
    if (istHtmlDok(this)) n = n.toLowerCase();
    return neu(null, null, n, '', null, this);
  };
  var fabrikNS = function createAttributeNS(ns, q) {
    if (arguments.length < 2) throw new TypeError("Failed to execute 'createAttributeNS' on 'Document': 2 arguments required, but only " + arguments.length + ' present.');
    ns = ns === null || ns === undefined || ns === '' ? null : String(ns);
    q = String(q);
    // „validate and extract" (Attribut): Praefix bis zum ersten `:`.
    var i = q.indexOf(':'), praefix = i >= 0 ? q.slice(0, i) : null, lokal = i >= 0 ? q.slice(i + 1) : q;
    if (praefix !== null && (praefix === '' || /[\t\n\f\r \u0000\/>]/.test(praefix))) throw new g.DOMException('Ungueltiges Praefix', 'InvalidCharacterError');
    namensPruefen(lokal);
    if ((praefix !== null && ns === null) ||
      (praefix === 'xml' && ns !== 'http://www.w3.org/XML/1998/namespace') ||
      ((q === 'xmlns' || praefix === 'xmlns') && ns !== 'http://www.w3.org/2000/xmlns/') ||
      (ns === 'http://www.w3.org/2000/xmlns/' && q !== 'xmlns' && praefix !== 'xmlns')) {
      throw new g.DOMException('Namensraumfehler', 'NamespaceError');
    }
    return neu(ns, praefix, lokal, '', null, this);
  };
  if (DP) { def(DP, 'createAttribute', fabrik); def(DP, 'createAttributeNS', fabrikNS); }
  try { Object.defineProperty(doc, 'createAttribute', { value: fabrik, configurable: true, writable: true }); } catch (e) {}
  try { Object.defineProperty(doc, 'createAttributeNS', { value: fabrikNS, configurable: true, writable: true }); } catch (e) {}
  // Release 12.10.2026: `el.attributes` mit echten Attr-Knoten (DOM §4.9.1),
  // dieselben wie `getAttributeNode`. GEMESSEN vorher (WPT
  // MutationObserver-attributes): `attributes[i]` war ein loses Objekt,
  // `attributes[i].value = …` aenderte nichts.
  // Release 2026-10-15: one live NamedNodeMap per element ([SameObject],
  // DOM §4.9.1): indices and the attributes' qualified names (for an HTML
  // element in an HTML document only the lower-case ones) are its own
  // properties, read from the element at each access; nothing can be set.
  // MEASURED before (WPT dom/nodes attributes, attributes-namednodemap):
  // a new array per read, `el.attributes.id` undefined.
  var mapen = new WeakMap();
  function attrListe(el) {
    var r = roh(el), liste = [];
    for (var i = 0; i < r.length && i < 65536; i++) {
      var a = fuer(el, r[i].namespaceURI || null, r[i].localName || r[i].name);
      if (a) liste.push(a);
    }
    return liste;
  }
  function htmlImHtml(el) {
    if (el.namespaceURI !== HTML) return false;
    var d = el.ownerDocument;
    try { return !d || String(d.contentType || 'text/html') === 'text/html'; } catch (e) { return true; }
  }
  function namen(el) {
    var r = roh(el), out = [], nurKlein = htmlImHtml(el);
    for (var i = 0; i < r.length; i++) {
      var q = r[i].name;
      if (out.indexOf(q) >= 0 || (nurKlein && /[A-Z]/.test(q))) continue;
      out.push(q);
    }
    return out;
  }
  function istIndex(k) { return typeof k === 'string' && /^(?:0|[1-9][0-9]{0,9})$/.test(k); }
  function karte(el) {
    var m = mapen.get(el);
    if (m) return m;
    var ziel = Object.create(g.NamedNodeMap ? g.NamedNodeMap.prototype : Object.prototype);
    try { Object.defineProperty(ziel, '__el', { value: el, enumerable: false }); } catch (e) {}
    var sichtbar = function (t, k) { return typeof k === 'string' && !istIndex(k) && !(k in t) && namen(el).indexOf(k) >= 0; };
    m = typeof g.Proxy === 'function' ? new g.Proxy(ziel, {
      get: function (t, k, r) {
        if (k === 'length') return roh(el).length;
        if (istIndex(k)) return attrListe(el)[+k];
        if (sichtbar(t, k)) return el.getAttributeNode(k);
        return Reflect.get(t, k, r);
      },
      set: function (t, k, v, r) {
        if (istIndex(k) || k === 'length' || sichtbar(t, k)) return false;
        return Reflect.set(t, k, v, r);
      },
      has: function (t, k) {
        if (istIndex(k)) return +k < roh(el).length;
        return k === 'length' || Reflect.has(t, k) || sichtbar(t, k);
      },
      deleteProperty: function (t, k) {
        if ((istIndex(k) && +k < roh(el).length) || sichtbar(t, k)) return false;
        return Reflect.deleteProperty(t, k);
      },
      defineProperty: function (t, k, d) {
        if (istIndex(k) || sichtbar(t, k)) return false;
        return Reflect.defineProperty(t, k, d);
      },
      ownKeys: function (t) {
        var n = roh(el).length, keys = [];
        for (var i = 0; i < n; i++) keys.push(String(i));
        var nm = namen(el);
        for (var j = 0; j < nm.length; j++) if (!(nm[j] in t)) keys.push(nm[j]);
        return keys.concat(Reflect.ownKeys(t).filter(function (k) { return k !== '__el'; }));
      },
      getOwnPropertyDescriptor: function (t, k) {
        if (istIndex(k)) {
          var a = attrListe(el)[+k];
          return a ? { value: a, writable: false, enumerable: true, configurable: true } : undefined;
        }
        if (sichtbar(t, k)) return { value: el.getAttributeNode(k), writable: false, enumerable: false, configurable: true };
        return Reflect.getOwnPropertyDescriptor(t, k);
      }
    }) : ziel;
    mapen.set(el, m);
    return m;
  }
  try {
    Object.defineProperty(E, 'attributes', {
      configurable: true, enumerable: true,
      get: function () { return karte(this); }
    });
  } catch (e) {}
  // NamedNodeMap: dieselben Attr-Knoten.
  var NNM = g.NamedNodeMap && g.NamedNodeMap.prototype;
  if (NNM) {
    def(NNM, 'item', function item(i) { var x = this[i]; return x && this.__el ? fuer(this.__el, x.namespaceURI || null, x.localName || x.name) : (x || null); });
    def(NNM, 'getNamedItem', function getNamedItem(n) { return this.__el ? this.__el.getAttributeNode(n) : null; });
    def(NNM, 'getNamedItemNS', function getNamedItemNS(ns, n) { return this.__el ? this.__el.getAttributeNodeNS(ns, n) : null; });
    def(NNM, 'setNamedItem', function setNamedItem(a) { return this.__el ? this.__el.setAttributeNode(a) : null; });
    def(NNM, 'setNamedItemNS', function setNamedItemNS(a) { return this.__el ? this.__el.setAttributeNodeNS(a) : null; });
    def(NNM, 'removeNamedItem', function removeNamedItem(n) {
      var a = this.__el ? this.__el.getAttributeNode(n) : null;
      if (!a) throw new g.DOMException('Attribut nicht gefunden', 'NotFoundError');
      return this.__el.removeAttributeNode(a);
    });
    def(NNM, 'removeNamedItemNS', function removeNamedItemNS(ns, n) {
      var a = this.__el ? this.__el.getAttributeNodeNS(ns, n) : null;
      if (!a) throw new g.DOMException('Attribut nicht gefunden', 'NotFoundError');
      return this.__el.removeAttributeNode(a);
    });
  }
})();

// -- Spiegelung von Inhaltsattributen (HTML §2.6.1 „reflect") --
// Release 11.10.2026: eine Tabelle Element -> IDL-Attribut -> Art (die
// Attributtabellen der HTML-Spezifikation) und EIN Satz Regeln je Art
// (string, url, boolean, long, unsigned long, die „limited"/„clamped"-
// Fassungen, double, enum, tokenlist). Nur wo weder der Host noch ein
// frueherer Block die Eigenschaft fuehrt. GEMESSEN vorher (WPT
// html/dom/reflection-*): rund 20 000 rote Teiltests, z. B. `form.method`
// undefined, `td.colSpan` mit „\v7" = 7.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || !g.HTMLElement) return;
  var TABELLE = {"a":{"charset":"string","coords":"string","download":"string","href":"url","hreflang":"string","name":"string","ping":"string","referrerPolicy":{"k":["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"],"t":"enum"},"rel":"string","relList":{"a":"rel","t":"tokenlist"},"rev":"string","shape":"string","target":"string","type":"string"},"abbr":{},"address":{},"area":{"alt":"string","coords":"string","download":"string","href":"url","hreflang":"string","noHref":"boolean","ping":"string","referrerPolicy":{"k":["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"],"t":"enum"},"rel":"string","relList":{"a":"rel","t":"tokenlist"},"shape":"string","target":"string","type":"string"},"article":{},"aside":{},"audio":{"autoplay":"boolean","controls":"boolean","crossOrigin":{"d":null,"i":"anonymous","k":["anonymous","use-credentials"],"n":{"":"anonymous"},"t":"enum","u":true},"defaultMuted":{"a":"muted","t":"boolean"},"loading":{"d":"eager","i":"eager","k":["lazy","eager"],"t":"enum"},"loop":"boolean","preload":{"d":["none","metadata","auto"],"k":["none","metadata","auto"],"n":{"":"auto"},"t":"enum"},"src":"url"},"b":{},"base":{"href":{"c":true,"t":"url"},"target":"string"},"bdi":{},"bdo":{},"blockquote":{"cite":"url"},"body":{"aLink":{"t":"string","z":true},"background":"string","bgColor":{"t":"string","z":true},"link":{"t":"string","z":true},"text":{"t":"string","z":true},"vLink":{"t":"string","z":true}},"br":{"clear":"string"},"button":{"disabled":"boolean","formAction":"url","formEnctype":{"i":"application/x-www-form-urlencoded","k":["application/x-www-form-urlencoded","multipart/form-data","text/plain"],"t":"enum"},"formMethod":{"i":"get","k":["get","post","dialog"],"t":"enum"},"formNoValidate":"boolean","formTarget":"string","name":"string","type":{"d":"submit","k":["submit","reset","button"],"t":"enum"},"value":"string"},"canvas":{"height":{"d":150,"t":"unsigned long"},"width":{"d":300,"t":"unsigned long"}},"caption":{"align":"string"},"cite":{},"code":{},"col":{"align":"string","ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"span":{"d":1,"hi":1000,"lo":1,"t":"clamped unsigned long"},"vAlign":"string","width":"string"},"colgroup":{"align":"string","ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"span":{"d":1,"hi":1000,"lo":1,"t":"clamped unsigned long"},"vAlign":"string","width":"string"},"data":{"value":"string"},"datalist":{},"dd":{},"del":{"cite":"url","dateTime":"string"},"details":{"open":"boolean"},"dfn":{},"dialog":{"open":"boolean"},"dir":{"compact":"boolean"},"div":{"align":"string"},"dl":{"compact":"boolean"},"dt":{},"em":{},"embed":{"align":"string","height":"string","name":"string","src":"url","type":"string","width":"string"},"fieldset":{"disabled":"boolean","name":"string"},"figcaption":{},"figure":{},"font":{"color":{"t":"string","z":true},"face":"string","size":"string"},"footer":{},"form":{"acceptCharset":{"a":"accept-charset","t":"string"},"action":"url","autocomplete":{"d":"on","k":["on","off"],"t":"enum"},"encoding":{"a":"enctype","d":"application/x-www-form-urlencoded","k":["application/x-www-form-urlencoded","multipart/form-data","text/plain"],"t":"enum"},"enctype":{"d":"application/x-www-form-urlencoded","k":["application/x-www-form-urlencoded","multipart/form-data","text/plain"],"t":"enum"},"method":{"d":"get","k":["get","post","dialog"],"t":"enum"},"name":"string","noValidate":"boolean","target":"string"},"frame":{"frameBorder":"string","longDesc":"url","marginHeight":{"t":"string","z":true},"marginWidth":{"t":"string","z":true},"name":"string","noResize":"boolean","scrolling":"string","src":"url"},"frameset":{"cols":"string","rows":"string"},"h1":{"align":"string"},"h2":{"align":"string"},"h3":{"align":"string"},"h4":{"align":"string"},"h5":{"align":"string"},"h6":{"align":"string"},"head":{},"header":{},"hgroup":{},"hr":{"align":"string","color":"string","noShade":"boolean","size":"string","width":"string"},"html":{"version":"string"},"i":{},"iframe":{"align":"string","allowFullscreen":"boolean","frameBorder":"string","height":"string","longDesc":"url","marginHeight":{"t":"string","z":true},"marginWidth":{"t":"string","z":true},"name":"string","referrerPolicy":{"k":["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"],"t":"enum"},"sandbox":"settable tokenlist","scrolling":"string","src":"url","srcdoc":"string","width":"string"},"img":{"align":"string","alt":"string","border":{"t":"string","z":true},"crossOrigin":{"d":null,"i":"anonymous","k":["anonymous","use-credentials"],"n":{"":"anonymous"},"t":"enum","u":true},"decoding":{"d":"auto","i":"auto","k":["async","sync","auto"],"t":"enum"},"height":{"c":true,"t":"unsigned long"},"hspace":"unsigned long","isMap":"boolean","longDesc":"url","lowsrc":{"t":"url"},"name":"string","referrerPolicy":{"k":["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"],"t":"enum"},"src":"url","srcset":"string","useMap":"string","vspace":"unsigned long","width":{"c":true,"t":"unsigned long"}},"input":{"accept":"string","align":"string","alt":"string","autocomplete":{"c":true,"t":"string"},"defaultChecked":{"a":"checked","t":"boolean"},"defaultValue":{"a":"value","t":"string"},"dirName":"string","disabled":"boolean","formAction":"url","formEnctype":{"i":"application/x-www-form-urlencoded","k":["application/x-www-form-urlencoded","multipart/form-data","text/plain"],"t":"enum"},"formMethod":{"i":"get","k":["get","post"],"t":"enum"},"formNoValidate":"boolean","formTarget":"string","height":{"c":true,"t":"unsigned long"},"max":"string","maxLength":"limited long","min":"string","minLength":"limited long","multiple":"boolean","name":"string","pattern":"string","placeholder":"string","readOnly":"boolean","required":"boolean","size":{"d":20,"t":"limited unsigned long"},"src":"url","step":"string","type":{"d":"text","k":["hidden","text","search","tel","url","email","password","date","time","datetime-local","number","range","color","checkbox","radio","file","submit","image","reset","button"],"t":"enum"},"useMap":"string","width":{"c":true,"t":"unsigned long"}},"ins":{"cite":"url","dateTime":"string"},"kbd":{},"label":{"htmlFor":{"a":"for","t":"string"}},"legend":{"align":"string"},"li":{"type":"string","value":"long"},"link":{"as":{"d":"","i":"","k":["fetch","audio","document","embed","font","image","manifest","object","report","script","sharedworker","style","track","video","worker","xslt"],"t":"enum"},"charset":"string","crossOrigin":{"d":null,"i":"anonymous","k":["anonymous","use-credentials"],"n":{"":"anonymous"},"t":"enum","u":true},"href":"url","hreflang":"string","integrity":"string","media":"string","nonce":"string","referrerPolicy":{"k":["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"],"t":"enum"},"rel":"string","relList":{"a":"rel","t":"tokenlist"},"rev":"string","sizes":"settable tokenlist","target":"string","type":"string"},"main":{},"map":{"name":"string"},"mark":{},"marquee":{"behavior":{"t":{"defaultVal":"scroll","keywords":["scroll","slide","alternate"],"type":"enum"}},"bgColor":"string","direction":{"t":{"defaultVal":"left","keywords":["up","right","down","left"],"type":"enum"}},"height":"string","hspace":"unsigned long","scrollAmount":{"d":6,"t":"unsigned long"},"scrollDelay":{"d":85,"t":"unsigned long"},"trueSpeed":"boolean","vspace":"unsigned long","width":"string"},"menu":{"compact":"boolean"},"meta":{"content":"string","httpEquiv":{"a":"http-equiv","t":"string"},"media":"string","name":"string","scheme":"string"},"meter":{"high":{"c":true,"t":"double"},"low":{"c":true,"t":"double"},"max":{"c":true,"t":"double"},"min":{"c":true,"t":"double"},"optimum":{"c":true,"t":"double"},"value":{"c":true,"t":"double"}},"nav":{},"noscript":{},"object":{"align":"string","archive":"string","border":{"t":"string","z":true},"code":"string","codeBase":"url","codeType":"string","data":"url","declare":"boolean","height":"string","hspace":"unsigned long","name":"string","standby":"string","type":"string","useMap":"string","vspace":"unsigned long","width":"string"},"ol":{"compact":"boolean","reversed":"boolean","start":{"d":1,"t":"long"},"type":"string"},"optgroup":{"disabled":"boolean","label":"string"},"option":{"defaultSelected":{"a":"selected","t":"boolean"},"disabled":"boolean","label":{"c":true,"t":"string"},"value":{"c":true,"t":"string"}},"output":{"htmlFor":{"a":"for","t":"settable tokenlist"},"name":"string"},"p":{"align":"string"},"param":{"name":"string","type":"string","value":"string","valueType":"string"},"picture":{},"pre":{"width":"long"},"progress":{"max":{"d":1.0,"t":"limited double"}},"q":{"cite":"url"},"rp":{},"rt":{},"ruby":{},"s":{},"samp":{},"script":{"charset":"string","crossOrigin":{"d":null,"i":"anonymous","k":["anonymous","use-credentials"],"n":{"":"anonymous"},"t":"enum","u":true},"defer":"boolean","event":"string","htmlFor":{"a":"for","t":"string"},"integrity":"string","noModule":"boolean","src":"url","type":"string"},"section":{},"select":{"autocomplete":{"c":true,"t":"string"},"disabled":"boolean","multiple":"boolean","name":"string","required":"boolean","size":{"d":0,"t":"unsigned long"}},"slot":{"name":"string"},"small":{},"source":{"media":"string","sizes":"string","src":"url","srcset":"string","type":"string"},"span":{},"strong":{},"style":{"media":"string","nonce":"string","type":"string"},"sub":{},"summary":{},"sup":{},"table":{"align":"string","bgColor":{"t":"string","z":true},"border":"string","cellPadding":{"t":"string","z":true},"cellSpacing":{"t":"string","z":true},"frame":"string","rules":"string","summary":"string","width":"string"},"tbody":{"align":"string","ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"vAlign":"string"},"td":{"abbr":"string","align":"string","axis":"string","bgColor":{"t":"string","z":true},"ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"colSpan":{"d":1,"hi":1000,"lo":1,"t":"clamped unsigned long"},"headers":"string","height":"string","noWrap":"boolean","rowSpan":{"d":1,"hi":65534,"lo":0,"t":"clamped unsigned long"},"scope":{"k":["row","col","rowgroup","colgroup"],"t":"enum"},"vAlign":"string","width":"string"},"template":{},"textarea":{"autocomplete":{"c":true,"t":"string"},"cols":{"d":20,"t":"limited unsigned long with fallback"},"dirName":"string","disabled":"boolean","maxLength":"limited long","minLength":"limited long","name":"string","placeholder":"string","readOnly":"boolean","required":"boolean","rows":{"d":2,"t":"limited unsigned long with fallback"},"wrap":"string"},"tfoot":{"align":"string","ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"vAlign":"string"},"th":{"abbr":"string","align":"string","axis":"string","bgColor":{"t":"string","z":true},"ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"colSpan":{"d":1,"hi":1000,"lo":1,"t":"clamped unsigned long"},"headers":"string","height":"string","noWrap":"boolean","rowSpan":{"d":1,"hi":65534,"lo":0,"t":"clamped unsigned long"},"scope":{"k":["row","col","rowgroup","colgroup"],"t":"enum"},"vAlign":"string","width":"string"},"thead":{"align":"string","ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"vAlign":"string"},"time":{"dateTime":"string"},"title":{},"tr":{"align":"string","bgColor":{"t":"string","z":true},"ch":{"a":"char","t":"string"},"chOff":{"a":"charoff","t":"string"},"vAlign":"string"},"track":{"default":"boolean","kind":{"d":"subtitles","i":"metadata","k":["subtitles","captions","descriptions","chapters","metadata"],"t":"enum"},"label":"string","src":"url","srclang":"string"},"u":{},"ul":{"compact":"boolean","type":"string"},"undefinedelement":{"enterKeyHint":{"k":["enter","done","go","next","previous","search","send"],"t":"enum"},"inputMode":{"k":["none","text","tel","url","email","numeric","decimal","search"],"t":"enum"}},"var":{},"video":{"autoplay":"boolean","controls":"boolean","controlsList":{"a":"controlsList","t":"tokenlist"},"crossOrigin":{"d":null,"i":"anonymous","k":["anonymous","use-credentials"],"n":{"":"anonymous"},"t":"enum","u":true},"defaultMuted":{"a":"muted","t":"boolean"},"height":"unsigned long","loading":{"d":"eager","i":"eager","k":["lazy","eager"],"t":"enum"},"loop":"boolean","playsInline":"boolean","poster":"url","preload":{"d":["none","metadata","auto"],"k":["none","metadata","auto"],"n":{"":"auto"},"t":"enum"},"src":"url","width":"unsigned long"},"wbr":{}};
  var WEISS = /^[ \t\n\f\r]$/;
  var MAXI = 2147483647;
  function klein(s) { return String(s).replace(/[A-Z]/g, function (c) { return String.fromCharCode(c.charCodeAt(0) + 32); }); }
  // HTML §2.3.4.1 „rules for parsing integers".
  function ganz(t) {
    if (t === null) return null;
    t = String(t);
    var i = 0, vz = 1;
    while (i < t.length && WEISS.test(t[i])) i++;
    if (i < t.length && (t[i] === '-' || t[i] === '+')) { if (t[i] === '-') vz = -1; i++; }
    if (i >= t.length || t[i] < '0' || t[i] > '9') return null;
    var v = 0;
    while (i < t.length && t[i] >= '0' && t[i] <= '9') { v = v * 10 + (t.charCodeAt(i) - 48); i++; }
    return v === 0 ? 0 : vz * v;
  }
  function nichtneg(t) { var v = ganz(t); return v === null || v < 0 ? null : v; }
  // HTML §2.3.4.3 „rules for parsing floating-point number values".
  function gleitend(t) {
    if (t === null) return null;
    var m = /^[ \t\n\f\r]*(-?)(\+?)((?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?)/.exec(String(t));
    if (!m || (m[1] && m[2])) return null;
    var v = Number(m[3]);
    if (m[1]) v = -v;
    if (!isFinite(v)) return null;
    return v === 0 ? (m[1] ? -0 : 0) : v;
  }
  function uint32(v) { v = Number(v); if (!isFinite(v)) return 0; v = (v < 0 ? Math.ceil(v) : Math.floor(v)) % 4294967296; return v < 0 ? v + 4294967296 : v; }
  function int32(v) { v = uint32(v); return v >= 2147483648 ? v - 4294967296 : v; }
  function indexFehler() { return new g.DOMException('Der Wert ist ausserhalb des erlaubten Bereichs.', 'IndexSizeError'); }
  function art(spec) {
    if (typeof spec === 'string') return { t: spec };
    if (spec && typeof spec.t === 'object' && spec.t) {
      var o = { t: spec.t.type };
      if (spec.t.keywords) o.k = spec.t.keywords;
      if ('defaultVal' in spec.t) o.d = spec.t.defaultVal;
      if ('invalidVal' in spec.t) o.i = spec.t.invalidVal;
      if (spec.t.nonCanon) o.n = spec.t.nonCanon;
      return o;
    }
    return spec;
  }
  // DOMTokenLists over any attribute (DOM §7.1), one per element and
  // attribute ([SameObject]).
  // Release 2026-10-15: the lists are proxies over a DOMTokenList object so
  // `list[0]` and Object.getOwnPropertyNames see the tokens; the state sits
  // in a WeakMap (no own helper property); supports() knows the supported
  // tokens of rel (link; a/area/form), sandbox and blocking; the update
  // steps leave an absent attribute absent for an empty set; replace()
  // keeps the position of the first of the two tokens.
  // MEASURED before (WPT dom/lists/DOMTokenList-coverage-for-attributes,
  // html/semantics/rellist-feature-detection): relList was a plain object
  // ("[object Object]"), `iframe.sandbox[0]` undefined, sandbox.supports()
  // threw.
  var listen = new WeakMap(), LIST_STATE = new WeakMap();
  var LINK_REL = ['modulepreload', 'preload', 'preconnect', 'dns-prefetch', 'stylesheet', 'icon', 'alternate', 'prefetch',
    'prerender', 'next', 'manifest', 'apple-touch-icon', 'apple-touch-icon-precomposed', 'canonical', 'compression-dictionary', 'expect'];
  var ANCHOR_REL = ['noreferrer', 'noopener', 'opener'];
  var SANDBOX = ['allow-downloads', 'allow-forms', 'allow-modals', 'allow-orientation-lock', 'allow-pointer-lock', 'allow-popups',
    'allow-popups-to-escape-sandbox', 'allow-presentation', 'allow-same-origin', 'allow-scripts', 'allow-top-navigation',
    'allow-top-navigation-by-user-activation', 'allow-top-navigation-to-custom-protocols', 'allow-storage-access-by-user-activation'];
  function supportedTokens(el, attr) {
    var n = el.localName;
    if (attr === 'rel') return n === 'link' ? LINK_REL : ANCHOR_REL;
    if (attr === 'sandbox') return SANDBOX;
    if (attr === 'blocking') return ['render'];
    return null;
  }
  function isIndex(k) { return typeof k === 'string' && /^(?:0|[1-9][0-9]{0,9})$/.test(k) && Number(k) < 4294967295; }
  function tokenListe(el, attr) {
    var m = listen.get(el);
    if (!m) { m = {}; listen.set(el, m); }
    if (m[attr]) return m[attr];
    var TL = g.DOMTokenList && g.DOMTokenList.prototype;
    var target = Object.create(TL || Object.prototype);
    var st = { el: el, attr: attr, supported: supportedTokens(el, attr) };
    var l = target;
    if (typeof g.Proxy === 'function') {
      l = new g.Proxy(target, {
        get: function (t, k, r) {
          if (isIndex(k)) { var x = teile(st)[Number(k)]; return x; }
          return Reflect.get(t, k, r);
        },
        set: function (t, k, v, r) {
          if (isIndex(k)) return false;
          return Reflect.set(t, k, v, r);
        },
        has: function (t, k) {
          if (isIndex(k)) return Number(k) < teile(st).length;
          return Reflect.has(t, k);
        },
        deleteProperty: function (t, k) {
          if (isIndex(k)) return !(Number(k) < teile(st).length);
          return Reflect.deleteProperty(t, k);
        },
        defineProperty: function (t, k, d) {
          if (isIndex(k)) return false;
          return Reflect.defineProperty(t, k, d);
        },
        ownKeys: function (t) {
          var out = [], n = teile(st).length;
          for (var i = 0; i < n; i++) out.push(String(i));
          return out.concat(Reflect.ownKeys(t));
        },
        getOwnPropertyDescriptor: function (t, k) {
          if (isIndex(k)) {
            var x = teile(st)[Number(k)];
            return x === undefined ? undefined : { value: x, writable: false, enumerable: true, configurable: true };
          }
          return Reflect.getOwnPropertyDescriptor(t, k);
        }
      });
    }
    LIST_STATE.set(l, st);
    LIST_STATE.set(target, st);
    m[attr] = l;
    return l;
  }
  try { Object.defineProperty(g, '__narsilTokenList', { value: tokenListe, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  // The token set of a list state: ordered, without duplicates.
  function teile(st) {
    var v = st.el.getAttribute(st.attr), raus = [];
    if (v === null) return raus;
    var p = String(v).split(/[ \t\n\f\r]+/);
    for (var i = 0; i < p.length; i++) if (p[i] && raus.indexOf(p[i]) < 0) raus.push(p[i]);
    return raus;
  }
  (function () {
    var TL = g.DOMTokenList && g.DOMTokenList.prototype;
    if (!TL) return;
    function info(l) { var i = LIST_STATE.get(l); if (!i) throw new TypeError('Illegal invocation'); return i; }
    function pruefen(t) {
      t = String(t);
      if (t === '') throw new g.DOMException("Failed to execute on 'DOMTokenList': The token provided must not be empty.", 'SyntaxError');
      if (/[ \t\n\f\r]/.test(t)) throw new g.DOMException("Failed to execute on 'DOMTokenList': The token provided ('" + t + "') contains HTML space characters, which are not valid in tokens.", 'InvalidCharacterError');
      return t;
    }
    // The update steps (DOM §7.1).
    function setzen(i, liste) {
      if (i.el.getAttribute(i.attr) === null && liste.length === 0) return;
      i.el.setAttribute(i.attr, liste.join(' '));
    }
    function def(n, f) { if (!Object.getOwnPropertyDescriptor(TL, n)) try { Object.defineProperty(TL, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
    function eig(n, lesen, schreiben) { if (!Object.getOwnPropertyDescriptor(TL, n)) try { Object.defineProperty(TL, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {} }
    eig('length', function () { return teile(info(this)).length; });
    eig('value', function () { var i = info(this), v = i.el.getAttribute(i.attr); return v === null ? '' : v; },
      function (v) { var i = info(this); i.el.setAttribute(i.attr, String(v)); });
    def('item', function item(n) { var t = teile(info(this)), k = uint32(n); return k < t.length ? t[k] : null; });
    def('contains', function contains(t) { return teile(info(this)).indexOf(String(t)) >= 0; });
    def('add', function add() {
      var i = info(this), a = Array.prototype.map.call(arguments, pruefen), t = teile(i);
      a.forEach(function (x) { if (t.indexOf(x) < 0) t.push(x); });
      setzen(i, t);
    });
    def('remove', function remove() {
      var i = info(this), a = Array.prototype.map.call(arguments, pruefen), t = teile(i);
      setzen(i, t.filter(function (x) { return a.indexOf(x) < 0; }));
    });
    def('toggle', function toggle(x, kraft) {
      var i = info(this);
      x = pruefen(x);
      var t = teile(i), da = t.indexOf(x) >= 0;
      if (da) {
        if (kraft === undefined || !kraft) { setzen(i, t.filter(function (y) { return y !== x; })); return false; }
        return true;
      }
      if (kraft === undefined || kraft) { t.push(x); setzen(i, t); return true; }
      return false;
    });
    def('replace', function replace(a, b) {
      var i = info(this);
      a = pruefen(a); b = pruefen(b);
      var t = teile(i);
      if (t.indexOf(a) < 0) return false;
      var out = [], done = false;
      for (var j = 0; j < t.length; j++) {
        if (t[j] === a || t[j] === b) { if (!done) { out.push(b); done = true; } }
        else out.push(t[j]);
      }
      setzen(i, out);
      return true;
    });
    // supports(): the host answers it for classList itself.
    try {
      Object.defineProperty(TL, 'supports', { configurable: true, writable: true, enumerable: true, value: function supports(token) {
        var i = LIST_STATE.get(this);
        if (!i || !i.supported) throw new TypeError("Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.");
        return i.supported.indexOf(String(token).toLowerCase()) >= 0;
      } });
    } catch (e) {}
  })();
  function zugriff(spec, attr) {
    spec = art(spec);
    var t = spec.t, d = spec.d;
    // Mehrere erlaubte Vorgaben (z. B. `preload`): die empfohlene.
    if (Array.isArray(d)) d = d.indexOf('metadata') >= 0 ? 'metadata' : d[0];
    switch (t) {
      case 'string':
        return {
          get: function () { var v = this.getAttribute(attr); return v === null ? '' : v; },
          set: function (v) { this.setAttribute(attr, spec.z && v === null ? '' : v); }
        };
      case 'url':
        return {
          get: function () {
            var v = this.getAttribute(attr);
            if ((v === null || v === '') && (attr === 'action' || attr === 'formaction')) return (this.ownerDocument || doc).URL;
            // `base.href` resolves against the fallback base URL and is that
            // URL without the attribute (HTML §4.2.3).
            if (this.localName === 'base' && attr === 'href' && typeof g.__narsilFallbackBase === 'function') {
              var fb = g.__narsilFallbackBase(this.ownerDocument || doc);
              if (v === null) return fb;
              try { return new g.URL(v, fb).href; } catch (e) { return v; }
            }
            if (v === null) return '';
            try { return new g.URL(v, this.baseURI || doc.baseURI).href; } catch (e) { return v; }
          },
          set: function (v) { this.setAttribute(attr, v); }
        };
      case 'boolean':
        return {
          get: function () { return this.hasAttribute(attr); },
          set: function (v) { if (v) this.setAttribute(attr, ''); else this.removeAttribute(attr); }
        };
      case 'long':
        return {
          get: function () { var v = ganz(this.getAttribute(attr)); return v !== null && v >= -2147483648 && v <= MAXI ? v : (d === undefined || d === null ? 0 : d); },
          set: function (v) { this.setAttribute(attr, String(int32(v))); }
        };
      case 'limited long':
        return {
          get: function () { var v = ganz(this.getAttribute(attr)); return v !== null && v >= 0 && v <= MAXI ? v : (d === undefined || d === null ? -1 : d); },
          set: function (v) { v = int32(v); if (v < 0) throw indexFehler(); this.setAttribute(attr, String(v)); }
        };
      case 'unsigned long':
        return {
          get: function () { var v = nichtneg(this.getAttribute(attr)); return v !== null && v <= MAXI ? v : (d === undefined || d === null ? 0 : d); },
          set: function (v) { v = uint32(v); if (v > MAXI) v = d === undefined || d === null ? 0 : d; this.setAttribute(attr, String(v)); }
        };
      case 'limited unsigned long':
      case 'limited unsigned long with fallback':
        return {
          get: function () { var v = nichtneg(this.getAttribute(attr)); return v !== null && v > 0 && v <= MAXI ? v : (d === undefined || d === null ? 1 : d); },
          set: function (v) {
            v = uint32(v);
            if (v === 0) { if (t === 'limited unsigned long') throw indexFehler(); v = d === undefined || d === null ? 1 : d; }
            if (v > MAXI) v = d === undefined || d === null ? 1 : d;
            this.setAttribute(attr, String(v));
          }
        };
      case 'clamped unsigned long':
        return {
          get: function () {
            var v = nichtneg(this.getAttribute(attr));
            if (v === null) return d === undefined || d === null ? spec.lo : d;
            return v < spec.lo ? spec.lo : v > spec.hi ? spec.hi : v;
          },
          set: function (v) { v = uint32(v); if (v > MAXI) v = d === undefined || d === null ? spec.lo : d; this.setAttribute(attr, String(v)); }
        };
      case 'double':
      case 'limited double':
        return {
          get: function () {
            var v = gleitend(this.getAttribute(attr));
            var vor = d === undefined || d === null ? 0 : d;
            if (v === null) return vor;
            return t === 'limited double' && !(v > 0) ? vor : v;
          },
          set: function (v) {
            v = Number(v);
            if (!isFinite(v)) throw new TypeError("Failed to set the '" + attr + "' property: The provided double value is non-finite.");
            if (t === 'limited double' && !(v > 0)) return;
            this.setAttribute(attr, String(v));
          }
        };
      case 'enum':
        var woerter = spec.k || [], kanon = spec.n || {};
        var fehlt = d === undefined ? '' : d, ungueltig = spec.i === undefined ? fehlt : spec.i;
        return {
          get: function () {
            var v = this.getAttribute(attr), r;
            if (v === null) r = fehlt;
            else {
              r = ungueltig;
              var kl = klein(v);
              for (var i = 0; i < woerter.length; i++) if (klein(woerter[i]) === kl) { r = woerter[i]; break; }
            }
            if (r !== null && Object.prototype.hasOwnProperty.call(kanon, r)) r = kanon[r];
            return r;
          },
          // Release 2026-10-15: a nullable DOMString takes undefined as null
          // (WebIDL §3.2.10). MEASURED before: crossOrigin = undefined wrote
          // "undefined".
          set: function (v) { if ((v === null || v === undefined) && spec.u) this.removeAttribute(attr); else this.setAttribute(attr, v); }
        };
      case 'tokenlist':
      case 'settable tokenlist':
        return {
          get: function () { return tokenListe(this, attr); },
          set: function (v) { this.setAttribute(attr, v); }
        };
    }
    return null;
  }
  function anlegen(P, idl, spec) {
    if (!P || Object.getOwnPropertyDescriptor(P, idl)) return;
    var s = art(spec), attr = (s && s.a) || klein(idl);
    var z = zugriff(spec, attr);
    if (!z) return;
    try { Object.defineProperty(P, idl, { get: z.get, set: z.set, configurable: true, enumerable: true }); } catch (e) {}
  }
  var HP = g.HTMLElement.prototype, UP = g.HTMLUnknownElement && g.HTMLUnknownElement.prototype;
  Object.keys(TABELLE).forEach(function (tag) {
    var attrs = TABELLE[tag], namen = Object.keys(attrs);
    if (!namen.length) return;
    var P;
    try { P = Object.getPrototypeOf(doc.createElement(tag)); } catch (e) { return; }
    if (!P || P === HP || P === UP) return;
    namen.forEach(function (idl) { anlegen(P, idl, attrs[idl]); });
  });
  // Was die Tabelle nicht nennt, die Spec aber fuehrt (HTML §4.8.4,
  // §4.8.5, §4.12.1, §6.8.7).
  var LADEN = { t: 'enum', k: ['lazy', 'eager'], d: 'eager', i: 'eager' };
  var VERWEIS = { t: 'enum', k: ['', 'no-referrer', 'no-referrer-when-downgrade', 'same-origin', 'origin', 'strict-origin', 'origin-when-cross-origin', 'strict-origin-when-cross-origin', 'unsafe-url'] };
  var FARBRAUM = { t: 'enum', k: ['limited-srgb', 'display-p3'], d: 'limited-srgb', i: 'limited-srgb', a: 'colorspace' };
  [['img', 'loading', LADEN], ['iframe', 'loading', LADEN], ['script', 'referrerPolicy', VERWEIS],
    ['input', 'colorSpace', FARBRAUM], ['input', 'alpha', 'boolean'],
    ['form', 'relList', { a: 'rel', t: 'tokenlist' }], ['link', 'blocking', 'settable tokenlist'],
    ['script', 'blocking', 'settable tokenlist'], ['style', 'blocking', 'settable tokenlist']].forEach(function (x) {
    try { anlegen(Object.getPrototypeOf(doc.createElement(x[0])), x[1], x[2]); } catch (e) {}
  });
  if (g.SVGAElement && g.SVGAElement.prototype) anlegen(g.SVGAElement.prototype, 'relList', { a: 'rel', t: 'tokenlist' });
  // Globale Attribute (HTML §3.2.6) an HTMLElement.
  anlegen(HP, 'dir', { t: 'enum', k: ['ltr', 'rtl', 'auto'] });
  anlegen(HP, 'inputMode', { t: 'enum', k: ['none', 'text', 'tel', 'url', 'email', 'numeric', 'decimal', 'search'] });
  anlegen(HP, 'enterKeyHint', { t: 'enum', k: ['enter', 'done', 'go', 'next', 'previous', 'search', 'send'] });
  anlegen(HP, 'accessKey', 'string');
  anlegen(HP, 'autofocus', 'boolean');
  anlegen(HP, 'inert', 'boolean');
  anlegen(HP, 'nonce', 'string');
  // Release 2026-10-15: `nonce` is an internal slot (HTML §2.6.2 "nonce"):
  // setting the IDL attribute leaves the content attribute alone; a later
  // content attribute change wins again. MEASURED before (WPT
  // reflection-metadata): link.nonce = "" also cleared the attribute.
  var NONCE = new WeakMap();
  var nonceDesc = {
    configurable: true, enumerable: true,
    get: function () {
      var a = this.getAttribute('nonce'), s = NONCE.get(this);
      if (s && s.attr === a) return s.val;
      return a === null ? '' : a;
    },
    set: function (v) { NONCE.set(this, { val: String(v), attr: this.getAttribute('nonce') }); }
  };
  [HP, g.SVGElement && g.SVGElement.prototype, g.HTMLLinkElement && g.HTMLLinkElement.prototype,
    g.HTMLStyleElement && g.HTMLStyleElement.prototype, g.HTMLScriptElement && g.HTMLScriptElement.prototype].forEach(function (P) {
    if (P) { try { Object.defineProperty(P, 'nonce', nonceDesc); } catch (e) {} }
  });
  // The custom (natural size) width/height getters keep theirs; the setter
  // is the unsigned long one (2^31 and up set the default, 0).
  [['img', 'width'], ['img', 'height'], ['input', 'width'], ['input', 'height']].forEach(function (x) {
    var P;
    try { P = Object.getPrototypeOf(doc.createElement(x[0])); } catch (e) { return; }
    var d = P && Object.getOwnPropertyDescriptor(P, x[1]);
    if (!d || typeof d.get !== 'function') return;
    try {
      Object.defineProperty(P, x[1], { configurable: true, enumerable: true, get: d.get, set: function (v) {
        v = uint32(v);
        if (v > MAXI) v = 0;
        this.setAttribute(x[1], String(v));
      } });
    } catch (e) {}
  });
  // Document's legacy reflections (HTML §3.1.3 dir; §16 fgColor & co. on
  // the body element, [LegacyNullToEmptyString]).
  var DP = g.Document && g.Document.prototype;
  if (DP) {
    var DIRS = ['ltr', 'rtl', 'auto'];
    try {
      Object.defineProperty(DP, 'dir', { configurable: true, enumerable: true,
        get: function () {
          var r = this.documentElement;
          var v = r ? r.getAttribute('dir') : null;
          if (v === null) return '';
          var k = klein(v);
          return DIRS.indexOf(k) >= 0 ? k : '';
        },
        set: function (v) { var r = this.documentElement; if (r) r.setAttribute('dir', String(v)); } });
    } catch (e) {}
    [['fgColor', 'text'], ['linkColor', 'link'], ['vlinkColor', 'vlink'], ['alinkColor', 'alink'], ['bgColor', 'bgcolor']].forEach(function (p) {
      try {
        Object.defineProperty(DP, p[0], { configurable: true, enumerable: true,
          get: function () { var b = this.body; var v = b && b.localName === 'body' ? b.getAttribute(p[1]) : null; return v === null ? '' : v; },
          set: function (v) { var b = this.body; if (b && b.localName === 'body') b.setAttribute(p[1], v === null ? '' : String(v)); } });
      } catch (e) {}
    });
  }
})();

// -- AbortSignal: reason, abort(), timeout(), any(), throwIfAborted (DOM §3.1) --
// Release 2026-10-14: the engine's native AbortController/AbortSignal keep
// `aborted` and the listener linkage; this layer adds what DOM §3.1 needs
// on top: `reason` (an "AbortError" DOMException by default),
// `throwIfAborted()` throwing the reason, `AbortSignal.abort(reason)`
// returning a real AbortSignal, and `AbortSignal.any()` with source and
// dependent signals ("signal abort": the dependents get the same reason and
// fire after the signal itself).
// MEASURED before (WPT dom/abort 9/37 subtests): no `reason`,
// "throwIfAborted is not a function" on the signal prototype, any()
// events in the wrong order.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var S = g.AbortSignal, C = g.AbortController;
  if (typeof S !== 'function' || typeof C !== 'function' || !C.prototype) return;
  function def(o, n, f) { try { Object.defineProperty(o, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  var nativeAbort = C.prototype.abort;
  if (typeof nativeAbort !== 'function') return;
  var reasons = new WeakMap(), controllers = new WeakMap(), sources = new WeakMap(), dependents = new WeakMap();
  function abortError() {
    try { return new g.DOMException('signal is aborted without reason', 'AbortError'); } catch (e) { return new Error('AbortError'); }
  }
  function controllerOf(signal) { return controllers.get(signal) || null; }
  function signalOf(controller) { return controller && controller.signal; }
  // The "signal abort" algorithm.
  function signalAbort(signal, reason) {
    if (!signal || signal.aborted || reasons.has(signal)) return;
    reasons.set(signal, reason === undefined ? abortError() : reason);
    var toAbort = [];
    var deps = dependents.get(signal) || [];
    for (var i = 0; i < deps.length; i++) {
      if (!deps[i].aborted) { reasons.set(deps[i], reasons.get(signal)); toAbort.push(deps[i]); }
    }
    runAbortSteps(signal);
    for (var j = 0; j < toAbort.length; j++) runAbortSteps(toAbort[j]);
  }
  // The signal's abort algorithms run before its abort event (DOM §3.1.3
  // "run the abort steps"); other blocks register through
  // `__narsilAbortAlgorithm(signal, fn)`, which returns a remover.
  var algorithms = new WeakMap();
  function addAlgorithm(signal, fn) {
    var list = algorithms.get(signal);
    if (!list) { list = []; algorithms.set(signal, list); }
    var entry = { fn: fn };
    list.push(entry);
    return function () { var i = list.indexOf(entry); if (i >= 0) list.splice(i, 1); };
  }
  try { Object.defineProperty(g, '__narsilAbortAlgorithm', { value: addAlgorithm, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  function runAbortSteps(signal) {
    var list = algorithms.get(signal), failure = null;
    if (list) {
      algorithms['delete'](signal);
      for (var i = 0; i < list.length; i++) {
        try { list[i].fn(); } catch (e) { if (!failure) failure = { error: e }; }
      }
    }
    var c = controllerOf(signal);
    if (c) nativeAbort.call(c);
    // An exception from an abort algorithm comes out of abort() (WPT
    // observable-from: IteratorRecord#return() throwing).
    if (failure) throw failure.error;
  }
  def(C.prototype, 'abort', function abort(reason) {
    var s = signalOf(this);
    if (s && !controllers.has(s)) controllers.set(s, this);
    signalAbort(s, reason);
  });
  try {
    Object.defineProperty(S.prototype, 'reason', {
      configurable: true, enumerable: true,
      get: function () { return reasons.has(this) ? reasons.get(this) : undefined; }
    });
  } catch (e) {}
  def(S.prototype, 'throwIfAborted', function throwIfAborted() {
    if (this.aborted) throw this.reason;
  });
  function newSignal() {
    var c = new C(), s = c.signal;
    controllers.set(s, c);
    return s;
  }
  def(S, 'abort', function abort(reason) {
    var s = newSignal();
    signalAbort(s, reason);
    return s;
  });
  def(S, 'timeout', function timeout(ms) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'timeout' on 'AbortSignal': 1 argument required, but only 0 present.");
    var n = Number(ms);
    if (!isFinite(n) || n < 0 || n > 9007199254740991) throw new TypeError("Failed to execute 'timeout' on 'AbortSignal': Value is outside the 'unsigned long long' value range.");
    var s = newSignal();
    g.setTimeout(function () {
      var e;
      try { e = new g.DOMException('signal timed out', 'TimeoutError'); } catch (x) { e = new Error('TimeoutError'); }
      signalAbort(s, e);
    }, Math.floor(n));
    return s;
  });
  function isSignal(x) { return !!x && typeof x === 'object' && x instanceof S; }
  def(S, 'any', function any(signals) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'any' on 'AbortSignal': 1 argument required, but only 0 present.");
    var list = [];
    if (signals && typeof signals[Symbol.iterator] === 'function') {
      for (var it = signals[Symbol.iterator](), st = it.next(), n = 0; !st.done && n < 100000; st = it.next(), n++) list.push(st.value);
    } else {
      throw new TypeError("Failed to execute 'any' on 'AbortSignal': The provided value cannot be converted to a sequence.");
    }
    for (var i = 0; i < list.length; i++) {
      if (!isSignal(list[i])) throw new TypeError("Failed to execute 'any' on 'AbortSignal': Failed to convert value to 'AbortSignal'.");
    }
    var result = newSignal();
    // A dependent signal counts as aborted as soon as it has a reason —
    // before the source's abort event fires ("signal abort" step 4).
    try {
      Object.defineProperty(result, 'aborted', {
        configurable: true, enumerable: true,
        get: function () { return reasons.has(result); }, set: function () {}
      });
    } catch (e) {}
    for (var j = 0; j < list.length; j++) {
      if (list[j].aborted) { signalAbort(result, list[j].reason); return result; }
    }
    var src = [];
    function addSource(s) {
      if (src.indexOf(s) >= 0) return;
      src.push(s);
      var d = dependents.get(s);
      if (!d) { d = []; dependents.set(s, d); }
      d.push(result);
    }
    for (var k = 0; k < list.length; k++) {
      var own = sources.get(list[k]);
      if (own) { for (var m = 0; m < own.length; m++) addSource(own[m]); }
      else addSource(list[k]);
    }
    sources.set(result, src);
    return result;
  });
  // A controller's own signal is aborted through this layer, also when a
  // native path calls abort on it: remember the pairing on first sight.
  try {
    var nativeSignal = Object.getOwnPropertyDescriptor(C.prototype, 'signal');
    if (nativeSignal && nativeSignal.get) {
      Object.defineProperty(C.prototype, 'signal', {
        configurable: true, enumerable: true,
        get: function () { var s = nativeSignal.get.call(this); if (s && !controllers.has(s)) controllers.set(s, this); return s; }
      });
    }
  } catch (e) {}
})();

// -- document.open / write / writeln / close (HTML §8.4) --
// Release 11.10.2026: waehrend des Ladens fuegt `write` das HTML hinter dem
// laufenden Skript ein (Inline-Skripte darin laufen sofort); danach — und
// in Iframe- oder erzeugten Dokumenten — oeffnet `write` das Dokument neu
// (`open`: alles weg), sammelt den Text und liest ihn, sobald kein
// `<script>` mehr offen ist; Skripte laufen im Fenster des Dokuments;
// `close` liest den Rest und meldet `load`. GEMESSEN vorher (WPT
// scripting-1 execution-timing, ~150 Zeitueberschreitungen):
// `document.open` fehlte, `write` haengte nur an den Body des Hauptdokuments.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var DP = g.Document && g.Document.prototype;
  if (!doc || !DP) return;
  var zustand = new WeakMap();
  function def(n, f) { try { Object.defineProperty(DP, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  function laedt(d) { return d === doc && d.readyState === 'loading'; }
  function ausgeglichen(t) {
    var auf = (t.match(/<script[\s>\/]/gi) || []).length, zu = (t.match(/<\/script[\s>]/gi) || []).length;
    if (auf > zu) return false;
    var lt = t.lastIndexOf('<'), gt = t.lastIndexOf('>');
    return lt <= gt;
  }
  function fragment(kontext, html) {
    var r = doc.createRange();
    r.selectNodeContents(kontext);
    return r.createContextualFragment(html);
  }
  function skripteVon(wurzel) {
    var raus = [];
    if (wurzel.nodeType === 1 && wurzel.localName === 'script') raus.push(wurzel);
    if (wurzel.querySelectorAll) Array.prototype.push.apply(raus, Array.prototype.slice.call(wurzel.querySelectorAll('script')));
    return raus;
  }
  function klassisch(s) {
    var t = String(s.getAttribute('type') || '').trim().toLowerCase();
    return !t || /^(text|application)\/(x-)?(java|ecma)script$/.test(t) || t === 'text/jscript' || t === 'text/livescript';
  }
  // Ein Skript im Fenster seines Dokuments ausfuehren (Iframe: das Rahmenfenster).
  function ausfuehren(d, s) {
    if (!klassisch(s)) return;
    var src = s.getAttribute('src');
    if (src) {
      if (typeof g.fetch !== 'function') return;
      g.fetch(new g.URL(src, d.URL || doc.URL).href).then(function (r) { return r.text(); }).then(function (code) {
        lauf(d, s, code);
        try { s.dispatchEvent(g.__narsilUA(g.Event, 'load')); } catch (e) {}
      }, function () { try { s.dispatchEvent(g.__narsilUA(g.Event, 'error')); } catch (e) {} });
      return;
    }
    lauf(d, s, s.textContent || '');
  }
  function lauf(d, s, code) {
    if (typeof g.__narsilRahmenSkript === 'function' && g.__narsilRahmenSkript(d, code)) return;
    if (d === doc && typeof g.__narsilSkript === 'function') { try { g.__narsilSkript(s); } catch (e) {} return; }
    try { (0, eval)(code); } catch (e) { try { if (typeof g.reportError === 'function') g.reportError(e); } catch (x) {} }
  }
  function leeren(d, z) {
    var html = z.puffer;
    z.puffer = '';
    if (!html) return;
    var skripte = [], selbst = false;
    if (!d.documentElement) {
      var t = new g.DOMParser().parseFromString(html, 'text/html');
      var kinder = Array.prototype.slice.call(t.childNodes);
      for (var i = 0; i < kinder.length; i++) {
        skripte = skripte.concat(skripteVon(kinder[i]));
        d.appendChild(kinder[i]);
      }
    } else {
      var ziel = z.ziel && z.ziel.isConnected ? z.ziel : (d.body || d.documentElement);
      var f = fragment(ziel, html);
      skripte = skripteVon(f);
      ziel.appendChild(f);
      // Im Hauptdokument laufen frisch eingefuegte Skripte von selbst.
      selbst = d === doc;
    }
    if (selbst) return;
    for (var j = 0; j < skripte.length; j++) ausfuehren(d, skripte[j]);
  }
  def('open', function open(a, b, c) {
    if (arguments.length >= 3 && typeof g.open === 'function') return g.open(a, b, c);
    if (laedt(this)) return this;
    var k;
    while ((k = this.firstChild)) this.removeChild(k);
    zustand.set(this, { offen: true, puffer: '', ziel: null });
    return this;
  });
  function schreiben(d, text) {
    if (laedt(d)) {
      var s = doc.currentScript || null;
      var z0 = zustand.get(d);
      if (!z0 || z0.skript !== s) { z0 = { offen: false, puffer: '', skript: s, punkt: s }; zustand.set(d, z0); }
      z0.puffer += text;
      if (!ausgeglichen(z0.puffer)) return;
      var html = z0.puffer;
      z0.puffer = '';
      var punkt = z0.punkt && z0.punkt.parentNode ? z0.punkt : null;
      var eltern = punkt ? punkt.parentNode : (d.body || d.documentElement);
      var f = fragment(eltern.nodeType === 1 ? eltern : (d.body || d.documentElement), html);
      var letztes = f.lastChild;
      eltern.insertBefore(f, punkt ? punkt.nextSibling : null);
      if (letztes) z0.punkt = letztes;
      return;
    }
    var z = zustand.get(d);
    if (!z || !z.offen) { d.open(); z = zustand.get(d); }
    z.puffer += text;
    if (ausgeglichen(z.puffer)) leeren(d, z);
  }
  def('write', function write() {
    schreiben(this, Array.prototype.map.call(arguments, String).join(''));
  });
  def('writeln', function writeln() {
    schreiben(this, Array.prototype.map.call(arguments, String).join('') + '\n');
  });
  def('close', function close() {
    var z = zustand.get(this);
    if (!z || !z.offen) return;
    leeren(this, z);
    z.offen = false;
    if (typeof g.__narsilRahmenFertig === 'function') g.__narsilRahmenFertig(this);
  });
})();

// -- Event-Handler aus Inhaltsattributen, setTimeout mit Zeichenkette --
// Release 11.10.2026: `el.onclick` liefert nach `setAttribute('onclick', …)`
// den uebersetzten Handler (HTML §8.1.8.1: Rumpf mit `event`, Bereiche
// Dokument, Formular, Element); `setTimeout("code")`/`setInterval`
// uebersetzen den Text beim Ausloesen als Skript (§8.6). GEMESSEN vorher
// (WPT dynamic-import string-compilation-*): `el.onclick` war null,
// `setTimeout("…")` warf TypeError.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var NAMEN = ('abort auxclick beforeinput beforematch beforetoggle blur cancel canplay canplaythrough change click close ' +
    'contextlost contextmenu contextrestored copy cuechange cut dblclick drag dragend dragenter dragleave dragover dragstart ' +
    'drop durationchange emptied ended error focus formdata input invalid keydown keypress keyup load loadeddata ' +
    'loadedmetadata loadstart mousedown mouseenter mouseleave mousemove mouseout mouseover mouseup paste pause play playing ' +
    'progress ratechange reset resize scroll scrollend securitypolicyviolation seeked seeking select slotchange stalled ' +
    'submit suspend timeupdate toggle volumechange waiting wheel animationstart animationend animationiteration ' +
    'animationcancel transitionrun transitionstart transitionend transitioncancel pointerdown pointerup pointermove ' +
    'pointerover pointerout pointerenter pointerleave pointercancel gotpointercapture lostpointercapture selectstart ' +
    'selectionchange focusin focusout command').split(' ');
  var cache = new WeakMap();
  // Release 12.10.2026: per Eigenschaft gesetzte Handler (`el.onload = f`)
  // je Element — mit dem Attributtext zur Zeit des Setzens: ein spaeteres
  // `setAttribute('onload', …)` ersetzt sie (HTML §8.1.8.1). GEMESSEN
  // vorher (WPT update-src-complete, img.complete): der leere Setter
  // verlor jede Zuweisung, `typeof el.onclick` war danach „undefined".
  var gesetzt = new WeakMap();
  function kompilieren(el, name, text) {
    var f = null;
    try { f = el.form || null; } catch (e) { f = null; }
    var d = el.ownerDocument || doc;
    var bau = new Function('__d', '__f', '__e',
      'return function ' + name + '(event) { with (__d) { with (__f) { with (__e) {\n' + text + '\n} } } };');
    return bau(d, f || {}, el);
  }
  function lesen(name) {
    return function () {
      var t = typeof this.getAttribute === 'function' ? this.getAttribute(name) : null;
      var s = gesetzt.get(this);
      if (s && Object.prototype.hasOwnProperty.call(s, name) && s[name].t === t) return s[name].v;
      if (t === null) return null;
      var m = cache.get(this);
      if (!m) { m = {}; cache.set(this, m); }
      if (m[name] && m[name].t === t) return m[name].f;
      var fn;
      try { fn = kompilieren(this, name, t); } catch (e) { try { if (typeof g.reportError === 'function') g.reportError(e); } catch (x) {} return null; }
      m[name] = { t: t, f: fn };
      return fn;
    };
  }
  [g.HTMLElement, g.SVGElement, g.MathMLElement].forEach(function (K) {
    var P = K && K.prototype;
    if (!P) return;
    NAMEN.forEach(function (n) {
      var name = 'on' + n;
      if (Object.getOwnPropertyDescriptor(P, name)) return;
      try {
        Object.defineProperty(P, name, {
          get: lesen(name), configurable: true, enumerable: true,
          set: function (v) {
            // WebIDL `EventHandler`: nur aufrufbare Objekte, sonst null.
            if (typeof v !== 'function' && (v === null || typeof v !== 'object')) v = null;
            var s = gesetzt.get(this);
            if (!s) { s = {}; gesetzt.set(this, s); }
            s[name] = { v: v, t: typeof this.getAttribute === 'function' ? this.getAttribute(name) : null };
          }
        });
      } catch (e) {}
    });
  });
  ['setTimeout', 'setInterval'].forEach(function (n) {
    var alt = g[n];
    if (typeof alt !== 'function') return;
    var neu = function (h) {
      var args = Array.prototype.slice.call(arguments);
      if (typeof h !== 'function') {
        var code = String(h);
        args[0] = function () { (0, eval)(code); };
      }
      return alt.apply(this, args);
    };
    try { Object.defineProperty(neu, 'name', { value: n }); } catch (e) {}
    g[n] = neu;
  });
})();

// -- <progress>, <meter> (HTML §4.10.13, §4.10.14) --
// Release 11.10.2026: Werte nach den Regeln fuer Gleitkommazahlen, geklemmt
// (meter: min <= low <= high <= max, value/optimum in [min, max]);
// `progress.position` (-1 ohne `value`). GEMESSEN vorher (WPT
// the-progress/meter-element): `position` undefined, keine Klemmung.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  function zahl(t) {
    if (t === null) return null;
    // Release 14.10.2026: a leading `+` is skipped (HTML §2.3.4.3 „rules for
    // parsing floating-point number values"; `+-5` stays invalid). MEASURED
    // before (WPT meter-leading-plus-sign): `value="+5"` read as 0.
    var m = /^[ \t\n\f\r]*\+?(-?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?)/.exec(String(t));
    if (!m || /^[ \t\n\f\r]*\+-/.test(String(t))) return null;
    var v = Number(m[1]);
    return isFinite(v) ? v : null;
  }
  function eig(P, n, lesen) {
    try {
      Object.defineProperty(P, n, {
        configurable: true, enumerable: true, get: lesen,
        set: function (v) {
          v = Number(v);
          if (!isFinite(v)) throw new TypeError("Failed to set the '" + n + "' property: The provided double value is non-finite.");
          this.setAttribute(n.toLowerCase(), String(v));
        }
      });
    } catch (e) {}
  }
  var PR = g.HTMLProgressElement && g.HTMLProgressElement.prototype;
  if (PR) {
    var maxP = function (el) { var m = zahl(el.getAttribute('max')); return m !== null && m > 0 ? m : 1; };
    eig(PR, 'max', function () { return maxP(this); });
    // „limited to only positive numbers": ein Wert <= 0 wird ignoriert.
    try {
      var dmax = Object.getOwnPropertyDescriptor(PR, 'max');
      Object.defineProperty(PR, 'max', {
        configurable: true, enumerable: true, get: dmax.get,
        set: function (v) { var n = Number(v); if (!isFinite(n)) throw new TypeError("Failed to set the 'max' property: The provided double value is non-finite."); if (n > 0) this.setAttribute('max', String(n)); }
      });
    } catch (e) {}
    eig(PR, 'value', function () {
      var v = zahl(this.getAttribute('value'));
      if (v === null || v < 0) return 0;
      return Math.min(v, maxP(this));
    });
    try {
      Object.defineProperty(PR, 'position', {
        configurable: true, enumerable: true,
        get: function () { return this.hasAttribute('value') ? this.value / maxP(this) : -1; }
      });
    } catch (e) {}
  }
  var ME = g.HTMLMeterElement && g.HTMLMeterElement.prototype;
  if (ME) {
    var mn = function (el) { var v = zahl(el.getAttribute('min')); return v === null ? 0 : v; };
    var mx = function (el) { var a = mn(el), v = zahl(el.getAttribute('max')); v = v === null ? 1 : v; return Math.max(v, a); };
    var kl = function (v, a, b) { return v < a ? a : v > b ? b : v; };
    var lo = function (el) { var a = mn(el), b = mx(el), v = zahl(el.getAttribute('low')); return v === null ? a : kl(v, a, b); };
    var hi = function (el) { var a = lo(el), b = mx(el), v = zahl(el.getAttribute('high')); return v === null ? b : kl(v, a, b); };
    eig(ME, 'min', function () { return mn(this); });
    eig(ME, 'max', function () { return mx(this); });
    eig(ME, 'value', function () { var v = zahl(this.getAttribute('value')); return kl(v === null ? 0 : v, mn(this), mx(this)); });
    eig(ME, 'low', function () { return lo(this); });
    eig(ME, 'high', function () { return hi(this); });
    eig(ME, 'optimum', function () {
      var a = mn(this), b = mx(this), v = zahl(this.getAttribute('optimum'));
      return v === null ? a + (b - a) / 2 : kl(v, a, b);
    });
  }
})();

// -- Formular-Eigentuemer, form.elements, form.length (HTML §4.10.17.3, §4.10.3) --
// Release 11.10.2026: `control.form` nach dem `form`-Attribut (gleicher Baum,
// muss ein <form> sein; leer = keiner) oder dem naechsten <form>-Vorfahren;
// `legend.form` ueber sein <fieldset>; `form.elements` (HTMLFormControlsCollection,
// in Baumordnung, ohne image-Buttons), `form.length`, benannter Zugriff
// `form.elements.namedItem`. GEMESSEN vorher (WPT form_attribute.html 0/100):
// `.form` war an JEDEM Feld undefined — auch das Absenden per Klick fand nie
// sein Formular.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc) return;
  var HTML = 'http://www.w3.org/1999/xhtml';
  var GELISTET = /^(button|fieldset|input|object|output|select|textarea)$/;
  function istHtml(el, n) { return !!el && el.nodeType === 1 && el.namespaceURI === HTML && (!n || el.localName === n); }
  function eigentuemer(el) {
    if (el.hasAttribute('form')) {
      var id = el.getAttribute('form');
      if (!id) return null;
      // Im eigenen Baum suchen — auch in einem losgeloesten (HTML §4.10.17.3).
      var wurzel = el.getRootNode ? el.getRootNode() : doc;
      var ziel = null;
      if (wurzel && (wurzel.nodeType === 9 || wurzel.nodeType === 11) && typeof wurzel.getElementById === 'function') ziel = wurzel.getElementById(id);
      else if (wurzel && wurzel.querySelectorAll) {
        if (wurzel.nodeType === 1 && wurzel.id === id) ziel = wurzel;
        var alle = ziel ? [] : wurzel.querySelectorAll('[id]');
        for (var i = 0; i < alle.length; i++) if (alle[i].id === id) { ziel = alle[i]; break; }
      }
      return istHtml(ziel, 'form') ? ziel : null;
    }
    for (var p = el.parentNode; p; p = p.parentNode) if (istHtml(p, 'form')) return p;
    return null;
  }
  function eig(P, n, lesen) {
    if (!P) return;
    try { Object.defineProperty(P, n, { get: lesen, configurable: true, enumerable: true }); } catch (e) {}
  }
  ['HTMLButtonElement', 'HTMLFieldSetElement', 'HTMLInputElement', 'HTMLObjectElement', 'HTMLOutputElement',
    'HTMLSelectElement', 'HTMLTextAreaElement'].forEach(function (n) {
    var K = g[n];
    eig(K && K.prototype, 'form', function () { return eigentuemer(this); });
  });
  var LP = g.HTMLLegendElement && g.HTMLLegendElement.prototype;
  eig(LP, 'form', function () {
    var p = this.parentNode;
    return istHtml(p, 'fieldset') ? eigentuemer(p) : null;
  });
  try { Object.defineProperty(g, '__narsilFormEigentuemer', { value: eigentuemer, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  // form.elements: die gelisteten Felder, deren Eigentuemer dieses Formular ist.
  function felder(form) {
    var wurzel = form.getRootNode ? form.getRootNode() : doc;
    var alle = wurzel && wurzel.querySelectorAll ? wurzel.querySelectorAll('button, fieldset, input, object, output, select, textarea') : [];
    var raus = [];
    for (var i = 0; i < alle.length; i++) {
      var e = alle[i];
      if (!istHtml(e) || !GELISTET.test(e.localName)) continue;
      if (e.localName === 'input' && String(e.getAttribute('type') || '').toLowerCase() === 'image') continue;
      if (eigentuemer(e) === form) raus.push(e);
    }
    // Ein nicht verbundenes Formular: nur die Nachfahren.
    if (!form.isConnected) {
      raus = [];
      var n = form.querySelectorAll('button, fieldset, input, object, output, select, textarea');
      for (var j = 0; j < n.length; j++) {
        if (n[j].localName === 'input' && String(n[j].getAttribute('type') || '').toLowerCase() === 'image') continue;
        if (eigentuemer(n[j]) === form) raus.push(n[j]);
      }
    }
    return raus;
  }
  var FP = g.HTMLFormElement && g.HTMLFormElement.prototype;
  var HC = g.HTMLCollection && g.HTMLCollection.prototype;
  if (FP) {
    if (typeof g.HTMLFormControlsCollection === 'undefined' && HC) {
      var FCC = function HTMLFormControlsCollection() { throw new TypeError('Illegal constructor'); };
      FCC.prototype = Object.create(HC);
      try { Object.defineProperty(FCC.prototype, 'constructor', { value: FCC, configurable: true, writable: true }); } catch (e) {}
      try { Object.defineProperty(FCC.prototype, Symbol.toStringTag, { value: 'HTMLFormControlsCollection', configurable: true }); } catch (e) {}
      try {
        Object.defineProperty(FCC.prototype, 'namedItem', {
          configurable: true, writable: true, enumerable: true,
          value: function namedItem(n) {
            n = String(n);
            if (!n) return null;
            // Release 2026-10-15: a live collection answers in the host (an
            // element, a RadioNodeList, or null).
            var fast = this['__narsilNamedItem:' + n];
            if (fast !== undefined) return fast;
            var treffer = [];
            for (var i = 0; i < this.length; i++) {
              var e = this[i];
              if (e.id === n || e.getAttribute('name') === n) treffer.push(e);
            }
            if (!treffer.length) return null;
            if (treffer.length === 1) return treffer[0];
            var rl = g.RadioNodeList ? Object.create(g.RadioNodeList.prototype) : [];
            for (var k = 0; k < treffer.length; k++) rl[k] = treffer[k];
            try { Object.defineProperty(rl, 'length', { value: treffer.length }); } catch (e) {}
            return rl;
          }
        });
      } catch (e) {}
      g.HTMLFormControlsCollection = FCC;
    }
    // [SameObject]: je Formular EIN Objekt, beim Lesen aufgefrischt.
    var sammlungen = new WeakMap();
    eig(FP, 'elements', function () {
      // Release 2026-10-15: the host's live HTMLFormControlsCollection.
      var lebendig = this['__narsilLive:elements'];
      if (lebendig && typeof lebendig === 'object') return lebendig;
      var l = felder(this);
      var c = sammlungen.get(this);
      if (!c) {
        c = Object.create(g.HTMLFormControlsCollection ? g.HTMLFormControlsCollection.prototype : Array.prototype);
        sammlungen.set(this, c);
      }
      Object.getOwnPropertyNames(c).forEach(function (k) { try { delete c[k]; } catch (e) {} });
      for (var i = 0; i < l.length; i++) { try { Object.defineProperty(c, i, { value: l[i], configurable: true, enumerable: true }); } catch (e) {} }
      try { Object.defineProperty(c, 'length', { value: l.length, configurable: true }); } catch (e) {}
      for (var j = 0; j < l.length; j++) {
        var namen = [l[j].id, l[j].getAttribute('name')];
        for (var k = 0; k < namen.length; k++) {
          var nm = namen[k];
          if (nm && !/^[0-9]+$/.test(nm) && !(nm in c)) {
            try { Object.defineProperty(c, nm, { value: c.namedItem ? c.namedItem(nm) : l[j], configurable: true }); } catch (e) {}
          }
        }
      }
      return c;
    });
    eig(FP, 'length', function () {
      var lebendig = this['__narsilLive:elements'];
      return lebendig && typeof lebendig === 'object' ? lebendig.length : felder(this).length;
    });
  }
})();

// -- RadioNodeList and the remaining live element collections (HTML §2.6.2.3) --
// Release 2026-10-15: `RadioNodeList` (a NodeList with `value`: the checked
// radio button's value, setting it checks the first radio with that value);
// `fieldset.elements`, `map.areas` and `datalist.options` are the host's live
// HTMLCollections (`__narsilLive:<name>`).
// MEASURED before (WPT/probe): `typeof RadioNodeList` was "undefined",
// `form.elements.r` for two radios an Array, `fieldset.elements` undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var NL = g.NodeList && g.NodeList.prototype;
  if (!NL) return;
  var HTML = 'http://www.w3.org/1999/xhtml';
  function isRadio(el) {
    return !!el && el.nodeType === 1 && el.namespaceURI === HTML && el.localName === 'input' && String(el.getAttribute('type') || '').toLowerCase() === 'radio';
  }
  if (typeof g.RadioNodeList !== 'function') {
    var RNL = function RadioNodeList() { throw new TypeError('Illegal constructor'); };
    RNL.prototype = Object.create(NL);
    try { Object.defineProperty(RNL.prototype, 'constructor', { value: RNL, configurable: true, writable: true }); } catch (e) {}
    try { Object.defineProperty(RNL.prototype, Symbol.toStringTag, { value: 'RadioNodeList', configurable: true }); } catch (e) {}
    try {
      Object.defineProperty(RNL.prototype, 'value', {
        configurable: true, enumerable: true,
        get: function () {
          for (var i = 0; i < this.length; i++) {
            var el = this[i];
            if (isRadio(el) && el.checked) { var v = el.getAttribute('value'); return v === null ? 'on' : v; }
          }
          return '';
        },
        set: function (v) {
          v = String(v);
          for (var i = 0; i < this.length; i++) {
            var el = this[i];
            if (!isRadio(el)) continue;
            var w = el.getAttribute('value');
            if ((w === null ? 'on' : w) === v) { el.checked = true; return; }
          }
        }
      });
    } catch (e) {}
    try { Object.defineProperty(g, 'RadioNodeList', { value: RNL, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  }
  function liveGetter(ctor, prop) {
    var P = g[ctor] && g[ctor].prototype;
    if (!P || Object.getOwnPropertyDescriptor(P, prop)) return;
    try {
      Object.defineProperty(P, prop, {
        configurable: true, enumerable: true,
        get: function () { var c = this['__narsilLive:' + prop]; return c && typeof c === 'object' ? c : null; }
      });
    } catch (e) {}
  }
  liveGetter('HTMLFieldSetElement', 'elements');
  liveGetter('HTMLMapElement', 'areas');
  liveGetter('HTMLDataListElement', 'options');
})();

// -- SubmitEvent (HTML §4.10.21.2) --
// Release 11.10.2026: der eingebaute Konstruktor kannte `submitter` nicht
// und lief ohne `new`. GEMESSEN vorher (WPT SubmitEvent.window 0/7).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var Nat = g.SubmitEvent, Ev = g.Event;
  if (typeof Nat !== 'function' || typeof Ev !== 'function') return;
  var I = new WeakMap();
  var SubmitEvent = function SubmitEvent(typ, init) {
    if (!(this instanceof SubmitEvent)) throw new TypeError("Failed to construct 'SubmitEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'SubmitEvent': 1 argument required, but only 0 present.");
    var s = init && init.submitter !== undefined ? init.submitter : null;
    if (s !== null && !(g.HTMLElement && s instanceof g.HTMLElement)) {
      throw new TypeError("Failed to construct 'SubmitEvent': Failed to read the 'submitter' property from 'SubmitEventInit': The provided value is not of type 'HTMLElement'.");
    }
    var e = new Ev(typ, init);
    try { Object.setPrototypeOf(e, SubmitEvent.prototype); } catch (x) {}
    I.set(e, s);
    return e;
  };
  SubmitEvent.prototype = Nat.prototype;
  try { Object.defineProperty(SubmitEvent.prototype, 'constructor', { value: SubmitEvent, configurable: true, writable: true }); } catch (e) {}
  try {
    Object.defineProperty(SubmitEvent.prototype, 'submitter', {
      configurable: true, enumerable: true,
      get: function () { return I.has(this) ? I.get(this) : null; }
    });
  } catch (e) {}
  try { Object.defineProperty(SubmitEvent, 'name', { value: 'SubmitEvent' }); } catch (e) {}
  g.SubmitEvent = SubmitEvent;
})();

// -- URL.createObjectURL / revokeObjectURL (File API §8) --
// Release 11.10.2026: `blob:`-Adressen fuer Blobs/Files, aufgeloest von
// `fetch` (und damit von Iframes, XHR und Modulen, die ueber `fetch` laden).
// GEMESSEN vorher (WPT the-iframe-element u. a.): „createObjectURL is not a
// function"; Seiten, die Bilder/Downloads ueber Blob-URLs bauen, brachen ab.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var U = g.URL, B = g.Blob;
  if (typeof U !== 'function' || typeof B !== 'function') return;
  var ablage = {};
  function herkunft() {
    try { var o = g.location && g.location.origin; return o && o !== 'null' ? o : 'null'; } catch (e) { return 'null'; }
  }
  function kennung() {
    try { if (g.crypto && typeof g.crypto.randomUUID === 'function') return g.crypto.randomUUID(); } catch (e) {}
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
      var r = Math.random() * 16 | 0;
      return (c === 'x' ? r : (r & 3 | 8)).toString(16);
    });
  }
  function def(o, n, f) { try { Object.defineProperty(o, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  if (typeof U.createObjectURL !== 'function') def(U, 'createObjectURL', function createObjectURL(obj) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'createObjectURL' on 'URL': 1 argument required, but only 0 present.");
    if (!(obj instanceof B) && !(g.MediaSource && obj instanceof g.MediaSource)) {
      throw new TypeError("Failed to execute 'createObjectURL' on 'URL': Overload resolution failed.");
    }
    var u = 'blob:' + herkunft() + '/' + kennung();
    ablage[u] = obj;
    return u;
  });
  if (typeof U.revokeObjectURL !== 'function') def(U, 'revokeObjectURL', function revokeObjectURL(u) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'revokeObjectURL' on 'URL': 1 argument required, but only 0 present.");
    delete ablage[String(u)];
  });
  try { Object.defineProperty(g, '__narsilBlobFuer', { value: function (u) { return ablage[String(u).split('#')[0]] || null; }, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  // Eine Antwort aus einem Blob (Fetch §4.2 „scheme fetch", `blob:`).
  function antwort(b, url) {
    var R = g.Response;
    var a = Object.create(R && R.prototype ? R.prototype : Object.prototype);
    var kopf = typeof g.Headers === 'function' ? new g.Headers({ 'Content-Type': b.type || '', 'Content-Length': String(b.size) }) : {};
    var benutzt = false;
    function lesen(f) {
      return function () {
        if (benutzt) return Promise.reject(new TypeError('Body has already been consumed.'));
        benutzt = true;
        return f();
      };
    }
    var felder = {
      ok: true, status: 200, statusText: 'OK', url: url, type: 'basic', redirected: false, headers: kopf,
      text: lesen(function () { return b.text(); }),
      arrayBuffer: lesen(function () { return b.arrayBuffer(); }),
      blob: lesen(function () { return Promise.resolve(b); }),
      json: lesen(function () { return b.text().then(function (t) { return JSON.parse(t); }); }),
      clone: function () { return antwort(b, url); }
    };
    Object.keys(felder).forEach(function (k) { try { Object.defineProperty(a, k, { value: felder[k], configurable: true, enumerable: true }); } catch (e) {} });
    try { Object.defineProperty(a, 'bodyUsed', { get: function () { return benutzt; }, configurable: true }); } catch (e) {}
    return a;
  }
  var altFetch = g.fetch;
  if (typeof altFetch === 'function') {
    g.fetch = function fetch(eingabe, init) {
      var u = typeof eingabe === 'string' ? eingabe : (eingabe && typeof eingabe.url === 'string' ? eingabe.url : (eingabe && eingabe.href) || '');
      if (/^blob:/i.test(String(u))) {
        var b = ablage[String(u).split('#')[0]];
        if (!b) return Promise.reject(new TypeError('Failed to fetch'));
        var m = String((init && init.method) || 'GET').toUpperCase();
        if (m !== 'GET') return Promise.reject(new TypeError('Failed to fetch'));
        return Promise.resolve(antwort(b, String(u)));
      }
      return altFetch.apply(this, arguments);
    };
  }
})();

// -- <link rel="stylesheet">: load / error (HTML §4.2.4 „link type stylesheet") --
// Release 12.10.2026: der Link bleibt im DOM ein Link (siehe
// `narsil_page::blattschrank`); sobald der Blattstand seiner Adresse feststeht,
// meldet er `load` oder `error` — als Aufgabe, nie im Einfuegen selbst.
// GEMESSEN vorher (WPT HTMLLinkElement-load-event 0/1, link-load-error-events,
// HTMLLinkElement-disabled-*): kein Ereignis, die Tests liefen in die Zeit.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || typeof g.__narsilBlattStand !== 'function') return;
  var stand = g.__narsilBlattStand;
  var gemeldet = new WeakMap(), stilGemeldet = new WeakMap();
  var geplant = false, runden = 0;
  function istBlatt(el) {
    return el.localName === 'link' && el.isConnected && el.hasAttribute('href') &&
      /(^|[ \t\n\f\r])stylesheet([ \t\n\f\r]|$)/i.test(el.getAttribute('rel') || '');
  }
  function adresse(el) {
    var h = el.getAttribute('href');
    if (!h) return null;
    try { return new g.URL(h, el.baseURI || doc.baseURI).href; } catch (e) { return null; }
  }
  function feuern(el, typ) {
    try { el.dispatchEvent(g.__narsilUA(g.Event, typ)); } catch (e) {}
  }
  function pruefen() {
    geplant = false;
    var offen = false;
    var liste = doc.querySelectorAll('link');
    for (var i = 0; i < liste.length && i < 2048; i++) {
      var el = liste[i];
      if (!istBlatt(el)) continue;
      var u = adresse(el);
      if (gemeldet.has(el) && gemeldet.get(el) === u) continue;
      var st = u === null ? 2 : stand(u);
      if (st === 1) { gemeldet.set(el, u); feuern(el, 'load'); }
      else if (st === 2) { gemeldet.set(el, u); feuern(el, 'error'); }
      else offen = true;
    }
    // Release 14.10.2026: a <style> element fires `load` once its sheet is
    // processed, again whenever its text (and so its sheet) changes (HTML
    // §4.2.6). MEASURED before (WPT style_load_event, style-load-after-mutate):
    // no load event at all.
    var stile = doc.querySelectorAll('style');
    for (var j = 0; j < stile.length && j < 2048; j++) {
      var st2 = stile[j];
      if (!st2.isConnected) continue;
      var text = st2.textContent;
      if (stilGemeldet.has(st2) && stilGemeldet.get(st2) === text) continue;
      stilGemeldet.set(st2, text);
      feuern(st2, 'load');
    }
    // Offene Blaetter holt der Takt; nachsehen, bis sie da sind (begrenzt).
    if (offen && runden++ < 400) planen(50);
  }
  function planen(ms) {
    if (geplant) return;
    geplant = true;
    g.setTimeout(pruefen, ms || 0);
  }
  try { Object.defineProperty(g, '__narsilLinkPruefen', { value: function () { runden = 0; planen(0); }, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  // Die Blaetter des Parsers melden sich VOR `window.load` (sie halten es nach
  // HTML §8.1.7 auf): beim DOMContentLoaded direkt, spaetere als Aufgabe.
  if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', function () { pruefen(); });
  else planen(0);
})();

// -- <img>: leeres oder ungueltiges src (HTML §4.8.4.3.5 „update the image data") --
// Release 11.10.2026: ein `src`, das leer ist oder sich nicht aufloesen
// laesst, meldet `error` (in einer Aufgabe, nicht im Setter); `complete` ist
// dann wahr. GEMESSEN vorher (WPT the-img-element invalid-src,
// null-image-source u. a.): kein Ereignis — die Tests liefen in die Zeit.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc) return;
  function kaputt(el) {
    if (el.parentElement && el.parentElement.localName === 'picture') return false;
    var s = el.getAttribute('src');
    if (s === null) return false;
    if (el.getAttribute('srcset')) return false;
    if (!s.trim()) return true;
    try { new g.URL(s, el.baseURI || doc.baseURI); return false; } catch (e) { return true; }
  }
  // Lazy und nicht verbunden: erst beim Einfuegen melden.
  var wartend = [];
  function warten() {
    var l = wartend.slice();
    wartend = [];
    l.forEach(function (x) { melden(x.el, x.src); });
    if (wartend.length) g.setTimeout(warten, 100);
  }
  function melden(el, src) {
    if (el.getAttribute('src') !== src) return;
    if (String(el.getAttribute('loading') || '').toLowerCase() === 'lazy' && !el.isConnected) {
      if (wartend.push({ el: el, src: src }) === 1) g.setTimeout(warten, 100);
      return;
    }
    try { if (typeof g.__narsilBildKaputt === 'function') g.__narsilBildKaputt(el); } catch (e) {}
    try { el.dispatchEvent(g.__narsilUA(g.Event, 'error')); } catch (e) {}
  }
  function pruefen(el) {
    if (!kaputt(el)) return;
    var src = el.getAttribute('src');
    g.setTimeout(function () { melden(el, src); }, 0);
  }
  try { Object.defineProperty(g, '__narsilBildQuelle', { value: pruefen, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  function anfang() { Array.prototype.forEach.call(doc.querySelectorAll('img[src]'), pruefen); }
  if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', anfang); else g.setTimeout(anfang, 0);
})();

// -- Release 15.10.2026 (Agent C): animations, loaded on first use --
// CSS Animations, CSS Transitions and Web Animations live in their own module
// (`animationen.js`, host `__narsilAnimQuelle`). Until a page touches one of
// the interfaces, or a style sheet or `style` attribute declares an animation
// or transition, only these stubs exist: a page without animations pays
// nothing. MEASURED before (WPT web-animations, css-animations,
// css-transitions: 84/450 files): `animate()` gave a fixed fake object.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  function host(n) { var f = typeof g[n] === 'function' ? g[n] : null; try { delete g[n]; } catch (e) {} return f; }
  var hosts = {
    source: host('__narsilAnimQuelle'), set: host('__narsilAnimSetzen'), base: host('__narsilBasiswert'),
    bases: host('__narsilBasiswerte'), value: host('__narsilWertBerechnen'), lerp: host('__narsilInterpolieren'),
    canLerp: host('__narsilInterpolierbar'), longhands: host('__narsilLangformen'), css: host('__narsilAnimCss'),
    combine: host('__narsilKombinieren'), prev: host('__narsilVorherwerte'), normal: host('__narsilWertNormal'), start: host('__narsilStartwerte'), raf: null
  };
  var NEEDED = host('__narsilAnimNoetig');
  if (!hosts.source || typeof g.Function !== 'function') return;
  var NAMES = ['Animation', 'AnimationEffect', 'KeyframeEffect', 'AnimationTimeline', 'DocumentTimeline', 'AnimationPlaybackEvent', 'CSSAnimation', 'CSSTransition'];
  var E = g.Element && g.Element.prototype;
  var D = g.Document && g.Document.prototype;
  var loaded = false;
  function load() {
    if (loaded) return true;
    loaded = true;
    NAMES.forEach(function (n) { try { delete g[n]; } catch (e) {} });
    if (D) { try { delete D.timeline; } catch (e) {} }
    hosts.raf = g.requestAnimationFrame;
    try {
      g.Function('h', hosts.source())(hosts);
    } catch (e) {
      if (g.console && g.console.error) g.console.error('animations: ' + e);
      return false;
    }
    return true;
  }
  // Declared animations or transitions load the module (the element `el`
  // checks its own `style` attribute too).
  function loadIfNeeded(el) {
    if (loaded) return true;
    var need = false;
    try { need = !!(NEEDED && NEEDED(el && el.nodeType === 1 ? el : null)); } catch (e) {}
    return need ? load() : false;
  }
  NAMES.forEach(function (n) {
    try {
      Object.defineProperty(g, n, {
        configurable: true, enumerable: false,
        get: function () { load(); var d = Object.getOwnPropertyDescriptor(g, n); return d && 'value' in d ? d.value : undefined; },
        set: function (v) { Object.defineProperty(g, n, { value: v, writable: true, configurable: true, enumerable: false }); }
      });
    } catch (e) {}
  });
  if (D) {
    try {
      Object.defineProperty(D, 'timeline', {
        configurable: true, enumerable: true,
        get: function () { return load() ? this.timeline : null; }
      });
    } catch (e) {}
  }
  if (E) {
    Object.defineProperty(E, 'animate', {
      configurable: true, writable: true, enumerable: true,
      value: function animate(keyframes, options) {
        if (!load() || E.animate === animate) throw new TypeError("Failed to execute 'animate' on 'Element': animations are not available.");
        return E.animate.apply(this, arguments);
      }
    });
  }
  // One function for elements, documents and shadow roots (the engine finds
  // a method of `Document.prototype` before the one of `Element.prototype`).
  function getAnimations(options) {
    if (!loadIfNeeded(this) || !E || E.getAnimations === getAnimations) return [];
    return E.getAnimations.apply(this, arguments);
  }
  [E, D, g.ShadowRoot && g.ShadowRoot.prototype].forEach(function (P) {
    if (P) Object.defineProperty(P, 'getAnimations', { configurable: true, writable: true, enumerable: true, value: getAnimations });
  });
  try { if (g.document) delete g.document.getAnimations; } catch (e) {}
  // `getComputedStyle` sees the animated values of this moment (a style
  // change event: CSS animations and transitions start here).
  try {
    Object.defineProperty(g, '__narsilAnimFlush', {
      configurable: true, writable: true, enumerable: false,
      value: function (el) { if (loadIfNeeded(el) && hosts.flush) hosts.flush(); }
    });
  } catch (e) {}
  var doc = g.document;
  if (doc && typeof doc.addEventListener === 'function' && NEEDED) {
    var check = function () { loadIfNeeded(null); };
    if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', check);
    else g.setTimeout(check, 0);
    if (typeof g.addEventListener === 'function') g.addEventListener('load', check);
  }
})();

// -- [PutForwards] (WebIDL §3.7.6) --
// Release 11.10.2026: `el.style = "…"` setzt `style.cssText` (CSSOM §6.7),
// `el.classList = "…"` setzt `classList.value` (DOM §4.9). GEMESSEN vorher
// (WPT top-layer-position u. a.): der Getter ohne Setter schluckte die
// Zuweisung, der Stil blieb leer.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || typeof doc.createElement !== 'function') return;
  var probe;
  try { probe = doc.createElement('div'); } catch (e) { return; }
  [['style', 'cssText'], ['classList', 'value']].forEach(function (paar) {
    for (var o = Object.getPrototypeOf(probe); o; o = Object.getPrototypeOf(o)) {
      var dsk = Object.getOwnPropertyDescriptor(o, paar[0]);
      if (!dsk) continue;
      if (dsk.get && !dsk.set) {
        try {
          Object.defineProperty(o, paar[0], {
            get: dsk.get, configurable: true, enumerable: dsk.enumerable,
            set: function (v) { var z = this[paar[0]]; if (z) z[paar[1]] = v; }
          });
        } catch (e) {}
      }
      return;
    }
  });
})();

// -- Event-Konstanten, window.frames (DOM §2.2, HTML §7.2.2) --
// Release 11.10.2026: `Event.NONE/CAPTURING_PHASE/AT_TARGET/BUBBLING_PHASE`
// am Konstruktor und am Prototyp; `window.frames` ist das Fenster selbst.
// GEMESSEN vorher (WPT dom/events): `Event.CAPTURING_PHASE` war undefined —
// jeder Phasenvergleich in `Event-dispatch-*` scheiterte; `frames` war „nicht
// definiert".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var E = g.Event;
  if (E) {
    [['NONE', 0], ['CAPTURING_PHASE', 1], ['AT_TARGET', 2], ['BUBBLING_PHASE', 3]].forEach(function (p) {
      if (E[p[0]] !== p[1]) { try { Object.defineProperty(E, p[0], { value: p[1], enumerable: true }); } catch (e) {} }
      if (E.prototype && E.prototype[p[0]] !== p[1]) { try { Object.defineProperty(E.prototype, p[0], { value: p[1], enumerable: true }); } catch (e) {} }
    });
  }
  if (typeof g.frames === 'undefined') {
    try { Object.defineProperty(g, 'frames', { get: function () { return g.window || g; }, configurable: true }); } catch (e) {}
  }
  // `window.event` ist [Replaceable] (HTML §8.1.8): ein Getter auf das
  // „current event"; schreibt die Seite hinein, wird es eine gewoehnliche
  // Eigenschaft, die das Ausloesen nicht mehr aendert. Die Verteiler setzen
  // das aktuelle Ereignis ueber `__narsilEreignis`. GEMESSEN vorher (WPT
  // `event-global-replaceable`): das Ausloesen ueberschrieb den Wert der Seite.
  var aktuell;
  try {
    Object.defineProperty(g, 'event', {
      configurable: true, enumerable: false,
      get: function () { return aktuell; },
      set: function (v) { Object.defineProperty(g, 'event', { value: v, writable: true, configurable: true, enumerable: true }); }
    });
  } catch (e) {}
  g.__narsilEreignis = function (ev) { var alt = aktuell; aktuell = ev; return alt; };
})();

// -- DOM-Luecken: tabIndex, compatMode, baseURI, outerText … --
// Release 11.10.2026: GEMESSEN vorher (WPT html/dom, dom/nodes): `tabIndex`
// (in jeder `reflection-*.html`), `document.compatMode`, `node.baseURI`,
// `outerText`, `accessKeyLabel` und `characterSet`/`contentType` erzeugter
// Dokumente waren undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var XHTML = 'http://www.w3.org/1999/xhtml';
  function eig(o, n, lesen, schreiben) {
    if (!o) return;
    try { Object.defineProperty(o, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  var H = g.HTMLElement && g.HTMLElement.prototype;
  // HTML §6.6.3: `tabIndex` spiegelt `tabindex` („rules for parsing
  // integers"); ohne gueltigen Wert 0 fuer fokussierbare Elemente, sonst -1.
  function ganzzahl(t) {
    var m = /^[\t\n\f\r ]*([-+]?)([0-9]+)/.exec(String(t));
    if (!m) return null;
    var n = parseInt(m[2], 10) * (m[1] === '-' ? -1 : 1);
    // Release 2026-10-15: "-0" parses to 0, not -0 (WPT html/dom
    // reflection-*: one tabIndex subtest per element).
    if (n === 0) return 0;
    return n >= -2147483648 && n <= 2147483647 ? n : null;
  }
  function fokussierbar(el) {
    var n = el.localName;
    if (n === 'a' || n === 'area') return el.hasAttribute('href');
    if (n === 'summary') { var p = el.parentElement; return !!p && p.localName === 'details' && p.querySelector('summary') === el; }
    return /^(button|frame|iframe|input|object|select|textarea)$/.test(n) || el.hasAttribute('contenteditable');
  }
  function tabLesen() {
    var v = this.getAttribute('tabindex');
    var n = v === null ? null : ganzzahl(v);
    if (n !== null) return n;
    return this.namespaceURI === XHTML && fokussierbar(this) ? 0 : -1;
  }
  function tabSetzen(v) { this.setAttribute('tabindex', String(v | 0)); }
  if (H && !Object.getOwnPropertyDescriptor(H, 'tabIndex')) eig(H, 'tabIndex', tabLesen, tabSetzen);
  var SV = g.SVGElement && g.SVGElement.prototype;
  if (SV && !Object.getOwnPropertyDescriptor(SV, 'tabIndex')) eig(SV, 'tabIndex', tabLesen, tabSetzen);
  if (H) {
    eig(H, 'accessKeyLabel', function () { return ''; });
    // `outerText` (HTML §3.2.7): lesen wie `innerText`, schreiben ersetzt das
    // Element durch den Text (Zeilenumbrueche werden zu <br>).
    // Release 2026-10-15: HTML §3.2.7 "rendered text fragment": text runs
    // become Text nodes, every CR, LF or CRLF a <br> ([LegacyNullToEmpty-
    // String]). The getter is the host's (`inner_text.rs`); the setter
    // replaces all children. MEASURED before (WPT innertext-setter, 38
    // failures): the setter was `textContent`, newlines stayed text.
    function textFragment(el, v) {
      var d = el.ownerDocument, f = d.createDocumentFragment();
      var s = v === null ? '' : String(v);
      s.split(/\r\n|\r|\n/).forEach(function (teil, i) {
        if (i > 0) f.appendChild(d.createElement('br'));
        if (teil) f.appendChild(d.createTextNode(teil));
      });
      return f;
    }
    eig(H, 'innerText', function () { return this.textContent; }, function (v) {
      var f = textFragment(this, v);
      while (this.firstChild) this.removeChild(this.firstChild);
      this.appendChild(f);
    });
    // `outerText`: replaces the element; neighbouring Text nodes merge with
    // the fragment's ends ("merge with the next text node").
    eig(H, 'outerText', function () { return this.innerText; }, function (v) {
      var p = this.parentNode;
      if (!p) throw new DOMException("Failed to set the 'outerText' property on 'HTMLElement': The element has no parent.", 'NoModificationAllowedError');
      var next = this.nextSibling, prev = this.previousSibling;
      var f = textFragment(this, v);
      if (!f.firstChild) f.appendChild(this.ownerDocument.createTextNode(''));
      p.replaceChild(f, this);
      function merge(t) {
        if (!t || t.nodeType !== 3) return;
        var n = t.nextSibling;
        if (n && n.nodeType === 3) { t.appendData(n.data); n.parentNode.removeChild(n); }
      }
      if (next && next.previousSibling) merge(next.previousSibling);
      merge(prev);
    });
  }
  var D = g.Document && g.Document.prototype;
  // HTML §13.2.6.4.1: Quirks aus dem Doctype (ohne Doctype, anderer Name,
  // bekannte alte Public-IDs) — `BackCompat`, sonst `CSS1Compat`.
  var QUIRKS_ANFANG = ['+//silmaril//dtd html pro v0r11 19970101//', '-//as//dtd html 3.0 aswedit + extensions//',
    '-//advasoft ltd//dtd html 3.0 aswedit + extensions//', '-//ietf//dtd html 2.0', '-//ietf//dtd html 3', '-//ietf//dtd html level',
    '-//ietf//dtd html strict', '-//ietf//dtd html//', '-//metrius//dtd metrius presentational//', '-//microsoft//dtd internet explorer',
    '-//netscape comm. corp.//dtd', '-//o\'reilly and associates//dtd html', '-//softquad', '-//spyglass//dtd html 2.0 extended//',
    '-//sq//dtd html 2.0 hotmetal + extensions//', '-//sun microsystems corp.//dtd hotjava html//', '-//w3c//dtd html 3', '-//w3c//dtd html 4.0',
    '-//w3c//dtd w3 html//', '-//w3o//dtd w3 html 3.0//', '-//webtechs//dtd mozilla html', '-//w3c//dtd html experimental'];
  var QUIRKS_GLEICH = ['-//w3o//dtd w3 html strict 3.0//en//', '-/w3c/dtd html 4.0 transitional/en', 'html'];
  function quirks(d) {
    var t = d.doctype;
    if (!t) return true;
    if (String(t.name).toLowerCase() !== 'html') return true;
    var pub = String(t.publicId || '').toLowerCase(), sys = String(t.systemId || '').toLowerCase();
    if (QUIRKS_GLEICH.indexOf(pub) >= 0) return true;
    if (sys === 'http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd') return true;
    for (var i = 0; i < QUIRKS_ANFANG.length; i++) if (pub.indexOf(QUIRKS_ANFANG[i]) === 0) return true;
    if (!t.systemId && (pub.indexOf('-//w3c//dtd html 4.01 frameset//') === 0 || pub.indexOf('-//w3c//dtd html 4.01 transitional//') === 0)) return true;
    return false;
  }
  if (D && !('compatMode' in D)) {
    eig(D, 'compatMode', function () {
      var html = this.documentElement && this.documentElement.namespaceURI === XHTML;
      if (this.contentType && this.contentType !== 'text/html') return 'CSS1Compat';
      return html && quirks(this) ? 'BackCompat' : 'CSS1Compat';
    });
  }
  // Release 13.10.2026: `URL`/`documentURI` (DOM §4.5): erzeugte Dokumente
  // haben „about:blank". Das Hauptdokument liefert der Host, Rahmen- und
  // DOMParser-Dokumente setzen eigene Eigenschaften. GEMESSEN vorher (WPT
  // DOMImplementation-createHTMLDocument, Document-constructor): undefined,
  // auch `document.documentURI`.
  if (D && !Object.getOwnPropertyDescriptor(D, 'URL')) {
    eig(D, 'URL', function () {
      if (this === doc) { try { return String(g.location.href); } catch (e) { return ''; } }
      return 'about:blank';
    });
  }
  if (D && !Object.getOwnPropertyDescriptor(D, 'documentURI')) {
    eig(D, 'documentURI', function () { return this.URL; });
  }
  // `document.rootElement` (SVG 2 §5.14.2): das Wurzelelement, wenn es ein
  // SVG-`<svg>` ist, sonst null.
  if (D && !('rootElement' in D)) {
    eig(D, 'rootElement', function () {
      var de = this.documentElement;
      return de && de.namespaceURI === 'http://www.w3.org/2000/svg' && de.localName === 'svg' ? de : null;
    });
  }
  if (D && !('contentType' in D)) {
    eig(D, 'contentType', function () {
      if (this.__narsilTyp) return this.__narsilTyp;
      var xml = this.__narsilXml;
      if (typeof xml === 'string' && xml) return xml;
      if (this === doc) return 'text/html';
      var de = this.documentElement;
      return de && de.namespaceURI === XHTML && this.createElement('a') instanceof g.HTMLElement ? 'text/html' : 'application/xml';
    });
  }
  // Erzeugte Dokumente (createDocument, DOMParser …) sind UTF-8.
  ['characterSet', 'charset', 'inputEncoding'].forEach(function (n) {
    if (D && !(n in D)) eig(D, n, function () { return 'UTF-8'; });
  });
  // `node.baseURI` (DOM §4.4): die Basisadresse des Dokuments.
  // Release 14.10.2026: relative to the fallback base URL (HTML §2.4.1: an
  // about:blank or srcdoc frame document falls back to the base URL of the
  // document of its frame element), and the first `<base href>` gives the
  // frozen base URL (HTML §4.2.3: a failure, data: or javascript: falls
  // back). MEASURED before (WPT base_about_blank, base_srcdoc, base-data):
  // `doc.baseURI` of a frame was "about:blank", a data: base was used.
  var fallbackBase = function (d) {
    var url = d.URL || (g.location && g.location.href) || '';
    if (url === 'about:blank' || url === 'about:srcdoc') {
      var f = null;
      try { f = d.__narsilFrameElement; } catch (e) {}
      var o = f && f.ownerDocument;
      if (o && o !== d) { var b = o.baseURI; if (b) return b; }
    }
    return url;
  };
  try { Object.defineProperty(g, '__narsilFallbackBase', { value: fallbackBase, configurable: true, writable: true }); } catch (e) {}
  var N = g.Node && g.Node.prototype;
  if (N && !Object.getOwnPropertyDescriptor(N, 'baseURI')) {
    eig(N, 'baseURI', function () {
      var d = this.nodeType === 9 ? this : this.ownerDocument;
      if (!d) return null;
      if (d !== this && d.baseURI) return d.baseURI;
      var b = d.querySelector && d.querySelector('base[href]');
      var url = fallbackBase(d);
      if (b) {
        try {
          var u = new g.URL(b.getAttribute('href'), url);
          if (u.protocol !== 'data:' && u.protocol !== 'javascript:') return u.href;
        } catch (e) {}
      }
      return url;
    });
  }
})();

// -- Formularfelder: valueAsNumber/-Date, stepUp/-Down, Auswahl, Laengen --
// Release 11.10.2026: die Feld-API von `<input>`/`<textarea>` (HTML §4.10.5,
// §4.10.18.6). Den Wert selbst bereinigt der Host (`formular_werte.rs`).
// GEMESSEN vorher (WPT the-input-element, 97 von 126 gescheitert):
// `valueAsNumber`, `stepUp`, `select`, `setSelectionRange`, `maxLength`
// waren undefined bzw. „is not a function".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var I = g.HTMLInputElement && g.HTMLInputElement.prototype;
  var T = g.HTMLTextAreaElement && g.HTMLTextAreaElement.prototype;
  if (!I || typeof I.stepUp === 'function') return;
  function eig(o, n, lesen, schreiben) {
    if (!o) return;
    try { Object.defineProperty(o, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  function def(o, n, f) { if (!o) return; try { Object.defineProperty(o, n, { value: f, writable: true, configurable: true, enumerable: true }); } catch (e) {} }
  function fehler(text, name) { return new DOMException(text, name); }
  function typ(el) {
    var t = String(el.getAttribute('type') || '').toLowerCase();
    return /^(hidden|text|search|tel|url|email|password|date|month|week|time|datetime-local|number|range|color|checkbox|radio|file|submit|image|reset|button)$/.test(t) ? t : 'text';
  }

  // -- Laengen (HTML §4.10.18.2/.3): nicht negative Ganzzahl, sonst -1 --
  function ganzNichtNegativ(v) {
    var m = /^[\t\n\f\r ]*\+?([0-9]+)/.exec(String(v));
    if (!m) return -1;
    var n = parseInt(m[1], 10);
    return n <= 2147483647 ? n : -1;
  }
  // Release 2026-10-15: maxLength/minLength/size are the reflection
  // table's (limited long, limited unsigned long; "-0" is 0, 2^31 falls back
  // to the default).
  eig(I, 'defaultValue', function () { var v = this.getAttribute('value'); return v === null ? '' : v; }, function (v) { this.setAttribute('value', String(v)); });
  // Gespiegelte Texte (HTML §4.10.5): `step`, `min`, `max`, `dirName`.
  [['step', 'step'], ['min', 'min'], ['max', 'max'], ['dirName', 'dirname']].forEach(function (p) {
    if (Object.getOwnPropertyDescriptor(I, p[0])) return;
    eig(I, p[0], function () { var v = this.getAttribute(p[1]); return v === null ? '' : v; }, function (v) { this.setAttribute(p[1], String(v)); });
  });

  // Release 14.10.2026: the child text content (only own Text children,
  // HTML §4.10.11), not textContent.
  if (T) eig(T, 'defaultValue', function () {
    var out = '';
    for (var n = this.firstChild; n; n = n.nextSibling) if (n.nodeType === 3 || n.nodeType === 4) out += n.data;
    return out;
  }, function (v) { this.textContent = String(v); });
  // Release 14.10.2026: `input.list` (HTML §4.10.5.3.9): the first element
  // with that ID in the same tree if it is a <datalist>, else null — also
  // null for types the attribute does not apply to. MEASURED before (WPT
  // input-list): undefined.
  if (I && !Object.getOwnPropertyDescriptor(I, 'list')) eig(I, 'list', function () {
    var id = this.getAttribute('list');
    if (id === null || id === '') return null;
    var t = String(this.getAttribute('type') || '').toLowerCase();
    if (/^(hidden|checkbox|radio|file|submit|image|reset|button)$/.test(t)) return null;
    var root = this.getRootNode ? this.getRootNode() : this.ownerDocument;
    var el = root && root.getElementById ? root.getElementById(id) : (root && root.querySelector ? root.querySelector('#' + CSS.escape(id)) : null);
    return el && el.localName === 'datalist' && el.namespaceURI === 'http://www.w3.org/1999/xhtml' ? el : null;
  });
  if (T) eig(T, 'textLength', function () { return this.value.length; });

  // -- Zahl <-> Text je Typ (HTML §4.10.5.1.7-.12) --
  var TAG = 86400000;
  function zwei(n) { return (n < 10 ? '0' : '') + n; }
  function jahrText(j) { var s = String(j); while (s.length < 4) s = '0' + s; return s; }
  function datumMs(s) {
    var m = /^([0-9]{4,})-([0-9]{2})-([0-9]{2})$/.exec(s);
    if (!m) return NaN;
    var j = +m[1], mo = +m[2], t = +m[3];
    var ms = Date.UTC(j, mo - 1, t);
    var d = new Date(ms);
    if (d.getUTCFullYear() !== j || d.getUTCMonth() !== mo - 1 || d.getUTCDate() !== t || j < 1) return NaN;
    return ms;
  }
  function zeitMs(s) {
    var m = /^([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:\.([0-9]{1,3}))?)?$/.exec(s);
    if (!m || +m[1] > 23 || +m[2] > 59 || (m[3] && +m[3] > 59)) return NaN;
    return ((+m[1] * 60 + +m[2]) * 60 + (+m[3] || 0)) * 1000 + (m[4] ? Math.round(+('0.' + m[4]) * 1000) : 0);
  }
  function zeitText(ms) {
    ms = ((ms % TAG) + TAG) % TAG;
    var h = Math.floor(ms / 3600000), mi = Math.floor(ms / 60000) % 60, s = Math.floor(ms / 1000) % 60, r = Math.round(ms % 1000);
    var t = zwei(h) + ':' + zwei(mi);
    if (s || r) { t += ':' + zwei(s); if (r) t += '.' + String(1000 + r).slice(1).replace(/0+$/, ''); }
    return t;
  }
  function datumText(ms) {
    var d = new Date(ms);
    return jahrText(d.getUTCFullYear()) + '-' + zwei(d.getUTCMonth() + 1) + '-' + zwei(d.getUTCDate());
  }
  function wocheMs(s) {
    var m = /^([0-9]{4,})-W([0-9]{2})$/.exec(s);
    if (!m) return NaN;
    var j = +m[1], w = +m[2];
    // Montag der ISO-Woche 1: die Woche mit dem 4. Januar.
    var vier = Date.UTC(j, 0, 4), wt = (new Date(vier).getUTCDay() + 6) % 7;
    var montag = vier - wt * TAG;
    var max = wochenImJahr(j);
    if (w < 1 || w > max) return NaN;
    return montag + (w - 1) * 7 * TAG;
  }
  function wochenImJahr(j) {
    var wt = (new Date(Date.UTC(j, 0, 1)).getUTCDay() + 6) % 7;
    var schalt = (j % 4 === 0 && j % 100 !== 0) || j % 400 === 0;
    return wt === 3 || (schalt && wt === 2) ? 53 : 52;
  }
  function wocheText(ms) {
    var d = new Date(ms), wt = (d.getUTCDay() + 6) % 7;
    var donnerstag = new Date(ms - wt * TAG + 3 * TAG);
    var j = donnerstag.getUTCFullYear();
    var erster = Date.UTC(j, 0, 4), wt1 = (new Date(erster).getUTCDay() + 6) % 7;
    var w = Math.floor((donnerstag.getTime() - (erster - wt1 * TAG)) / (7 * TAG)) + 1;
    return jahrText(j) + '-W' + zwei(w);
  }
  function monatZahl(s) {
    var m = /^([0-9]{4,})-([0-9]{2})$/.exec(s);
    if (!m || +m[2] < 1 || +m[2] > 12 || +m[1] < 1) return NaN;
    return (+m[1] - 1970) * 12 + (+m[2] - 1);
  }
  function monatText(n) {
    var j = 1970 + Math.floor(n / 12), mo = ((n % 12) + 12) % 12;
    return jahrText(j) + '-' + zwei(mo + 1);
  }
  function dtMs(s) {
    var m = /^(.*)[T ](.*)$/.exec(s);
    if (!m) return NaN;
    var d = datumMs(m[1]), z = zeitMs(m[2]);
    return isNaN(d) || isNaN(z) ? NaN : d + z;
  }
  function dtText(ms) { return datumText(Math.floor(ms / TAG) * TAG) + 'T' + zeitText(ms - Math.floor(ms / TAG) * TAG); }
  var ARTEN = {
    number: { zahl: function (s) { var n = parseFloat(s); return /^-?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$/.test(s) ? n : NaN; }, text: function (n) { return String(n); }, schritt: 1, mass: 1, basis: 0 },
    range: { zahl: function (s) { return ARTEN.number.zahl(s); }, text: function (n) { return String(n); }, schritt: 1, mass: 1, basis: 0 },
    date: { zahl: datumMs, text: datumText, schritt: 1, mass: TAG, basis: 0, datum: true },
    month: { zahl: monatZahl, text: monatText, schritt: 1, mass: 1, basis: 0, datum: true },
    week: { zahl: wocheMs, text: wocheText, schritt: 1, mass: 7 * TAG, basis: -259200000, datum: true },
    time: { zahl: zeitMs, text: zeitText, schritt: 60, mass: 1000, basis: 0, datum: true },
    'datetime-local': { zahl: dtMs, text: dtText, schritt: 60, mass: 1000, basis: 0 }
  };
  eig(I, 'valueAsNumber', function () {
    var a = ARTEN[typ(this)];
    return a ? a.zahl(this.value) : NaN;
  }, function (v) {
    var a = ARTEN[typ(this)];
    if (!a) throw fehler("Failed to set the 'valueAsNumber' property on 'HTMLInputElement': This input element does not support Number values.", 'InvalidStateError');
    var n = Number(v);
    if (n === Infinity || n === -Infinity) throw new TypeError("Failed to set the 'valueAsNumber' property on 'HTMLInputElement': The value provided is infinite.");
    this.value = isNaN(n) ? '' : a.text(n);
  });
  eig(I, 'valueAsDate', function () {
    var t = typ(this), a = ARTEN[t];
    if (!a || !a.datum) return null;
    var n = a.zahl(this.value);
    if (isNaN(n)) return null;
    if (t === 'month') return new Date(Date.UTC(1970 + Math.floor(n / 12), ((n % 12) + 12) % 12, 1));
    return new Date(n);
  }, function (v) {
    var t = typ(this), a = ARTEN[t];
    if (!a || !a.datum) throw fehler("Failed to set the 'valueAsDate' property on 'HTMLInputElement': This input element does not support Date values.", 'InvalidStateError');
    if (v !== null && !(v instanceof Date)) throw new TypeError("Failed to set the 'valueAsDate' property on 'HTMLInputElement': The provided value is not a Date.");
    if (v === null || isNaN(v.getTime())) { this.value = ''; return; }
    if (t === 'month') { this.value = monatText((v.getUTCFullYear() - 1970) * 12 + v.getUTCMonth()); return; }
    if (t === 'time') { this.value = zeitText(v.getTime() - Math.floor(v.getTime() / TAG) * TAG); return; }
    this.value = a.text(v.getTime());
  });

  // -- stepUp/stepDown (HTML §4.10.5.4) --
  function schrittweite(el, a) {
    var s = el.getAttribute('step');
    if (s !== null && s.toLowerCase() === 'any') return null;
    var n = s === null ? NaN : ARTEN.number.zahl(s.trim());
    if (!(n > 0)) n = a.schritt;
    if (typ(el) === 'month' || typ(el) === 'date' || typ(el) === 'week') n = Math.round(n) || a.schritt;
    return n * a.mass;
  }
  function grenze(el, a, n) {
    var v = el.getAttribute(n);
    return v === null ? NaN : a.zahl(v);
  }
  function schreiten(el, n, richtung) {
    var t = typ(el), a = ARTEN[t];
    if (!a) throw fehler("Failed to execute 'step" + (richtung > 0 ? 'Up' : 'Down') + "' on 'HTMLInputElement': This form element is not steppable.", 'InvalidStateError');
    var schritt = schrittweite(el, a);
    if (schritt === null) throw fehler("Failed to execute 'step" + (richtung > 0 ? 'Up' : 'Down') + "' on 'HTMLInputElement': This form element does not have an allowed value step.", 'InvalidStateError');
    var min = grenze(el, a, 'min'), max = grenze(el, a, 'max');
    if (t === 'range') { if (isNaN(min)) min = 0; if (isNaN(max)) max = 100; }
    if (!isNaN(min) && !isNaN(max) && min > max) return;
    var basis = !isNaN(min) ? min : (el.getAttribute('value') !== null && !isNaN(a.zahl(el.getAttribute('value'))) ? a.zahl(el.getAttribute('value')) : a.basis);
    var wert = a.zahl(el.value);
    if (isNaN(wert)) wert = 0;
    var vorher = wert;
    var rest = (wert - basis) / schritt;
    if (Math.abs(rest - Math.round(rest)) > 1e-9) {
      wert = basis + (richtung > 0 ? Math.ceil(rest) : Math.floor(rest)) * schritt;
    } else {
      wert += richtung * n * schritt;
    }
    if (!isNaN(min) && wert < min) wert = basis + Math.ceil((min - basis) / schritt) * schritt;
    if (!isNaN(max) && wert > max) wert = basis + Math.floor((max - basis) / schritt) * schritt;
    if ((richtung < 0 && vorher < wert) || (richtung > 0 && vorher > wert)) return;
    // Rundungsrauschen der Gleitkommarechnung entfernen.
    wert = Math.round(wert * 1e9) / 1e9;
    el.value = a.text(wert);
  }
  def(I, 'stepUp', function stepUp(n) { schreiten(this, n === undefined ? 1 : Number(n) | 0, 1); });
  def(I, 'stepDown', function stepDown(n) { schreiten(this, n === undefined ? 1 : Number(n) | 0, -1); });

  // -- Textauswahl (HTML §4.10.18.6) --
  var auswahl = new WeakMap();
  function mitAuswahl(el) {
    if (el.localName === 'textarea') return true;
    return /^(text|search|url|tel|password)$/.test(typ(el));
  }
  function zustand(el) {
    var z = auswahl.get(el), l = el.value.length;
    if (!z) { z = { start: l, ende: l, richtung: 'none' }; auswahl.set(el, z); }
    if (z.start > l) z.start = l;
    if (z.ende > l) z.ende = l;
    return z;
  }
  function pruefen(el, was) {
    if (!mitAuswahl(el)) throw fehler("Failed to execute '" + was + "' on 'HTMLInputElement': The input element's type ('" + typ(el) + "') does not support selection.", 'InvalidStateError');
  }
  function setzen(el, s, e, r) {
    var l = el.value.length, z = zustand(el);
    s = Math.min(Math.max(0, s >>> 0), l);
    e = Math.min(Math.max(0, e >>> 0), l);
    if (s > e) s = e;
    z.start = s; z.ende = e;
    z.richtung = r === 'forward' || r === 'backward' ? r : 'none';
    var ev;
    try { ev = globalThis.__narsilUA(Event, 'select', { bubbles: true }); } catch (x) { ev = null; }
    if (ev) setTimeout(function () { el.dispatchEvent(ev); }, 0);
  }
  [I, T].forEach(function (P) {
    if (!P) return;
    def(P, 'select', function select() {
      if (!mitAuswahl(this)) return;
      setzen(this, 0, this.value.length, 'none');
    });
    def(P, 'setSelectionRange', function setSelectionRange(s, e, r) {
      pruefen(this, 'setSelectionRange');
      setzen(this, s, e, r);
    });
    def(P, 'setRangeText', function setRangeText(text, s, e, art) {
      pruefen(this, 'setRangeText');
      var z = zustand(this), v = this.value;
      if (s === undefined) { s = z.start; e = z.ende; }
      s = s >>> 0; e = e >>> 0;
      if (s > e) throw fehler("Failed to execute 'setRangeText': The provided start value (" + s + ') is larger than the provided end value (' + e + ').', 'IndexSizeError');
      if (s > v.length) s = v.length;
      if (e > v.length) e = v.length;
      text = String(text);
      var altS = z.start, altE = z.ende;
      this.value = v.slice(0, s) + text + v.slice(e);
      var neuEnde = s + text.length;
      art = String(art || 'preserve');
      var z2 = zustand(this);
      if (art === 'select') { z2.start = s; z2.ende = neuEnde; }
      else if (art === 'start') { z2.start = z2.ende = s; }
      else if (art === 'end') { z2.start = z2.ende = neuEnde; }
      else {
        var delta = text.length - (e - s);
        z2.start = altS > e ? altS + delta : (altS > s ? s : altS);
        z2.ende = altE > e ? altE + delta : (altE > s ? neuEnde : altE);
      }
    });
    eig(P, 'selectionStart', function () { return mitAuswahl(this) ? zustand(this).start : null; }, function (v) {
      pruefen(this, 'selectionStart');
      var z = zustand(this);
      setzen(this, v, Math.max(v >>> 0, z.ende), z.richtung);
    });
    eig(P, 'selectionEnd', function () { return mitAuswahl(this) ? zustand(this).ende : null; }, function (v) {
      pruefen(this, 'selectionEnd');
      var z = zustand(this);
      setzen(this, Math.min(z.start, v >>> 0), v, z.richtung);
    });
    eig(P, 'selectionDirection', function () { return mitAuswahl(this) ? zustand(this).richtung : null; }, function (v) {
      pruefen(this, 'selectionDirection');
      var z = zustand(this);
      setzen(this, z.start, z.ende, v);
    });
  });
})();

// -- Constraint Validation der eingebauten Felder (HTML §4.10.20) --
// Release 11.10.2026: `willValidate`, `validity` (valueMissing,
// typeMismatch, patternMismatch, rangeUnder-/Overflow, stepMismatch,
// customError), `checkValidity`/`reportValidity` mit `invalid`,
// `setCustomValidity`, `validationMessage`, dazu `form.checkValidity()`.
// GEMESSEN vorher (WPT forms/constraints, 28 von 28 gescheitert):
// `input.validity` war undefined, `checkValidity is not a function`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  if (!doc || !g.HTMLInputElement) return;
  var eigene = new WeakMap();
  function def(o, n, f) { if (!o) return; try { Object.defineProperty(o, n, { value: f, writable: true, configurable: true, enumerable: true }); } catch (e) {} }
  function eig(o, n, lesen) { if (!o) return; try { Object.defineProperty(o, n, { get: lesen, configurable: true, enumerable: true }); } catch (e) {} }
  if (typeof g.ValidityState !== 'function') {
    g.ValidityState = function ValidityState() { throw new TypeError('Illegal constructor'); };
  }
  var VS = g.ValidityState.prototype;
  function typ(el) {
    var t = String(el.getAttribute('type') || '').toLowerCase();
    return /^(hidden|text|search|tel|url|email|password|date|month|week|time|datetime-local|number|range|color|checkbox|radio|file|submit|image|reset|button)$/.test(t) ? t : 'text';
  }
  function gesperrt(el) {
    if (el.hasAttribute('disabled')) return true;
    for (var p = el.parentElement; p; p = p.parentElement) {
      if (p.localName === 'fieldset' && p.hasAttribute('disabled')) {
        // Das erste <legend> des fieldset ist ausgenommen.
        var legende = null;
        for (var k = p.firstElementChild; k; k = k.nextElementSibling) if (k.localName === 'legend') { legende = k; break; }
        if (!legende || !legende.contains(el)) return true;
      }
    }
    return false;
  }
  function inDatalist(el) { return !!(el.closest && el.closest('datalist')); }
  // „candidate for constraint validation" (§4.10.20.1).
  function kandidat(el) {
    var n = el.localName;
    if (n === 'input') {
      var t = typ(el);
      if (t === 'hidden' || t === 'reset' || t === 'button') return false;
      if (el.hasAttribute('readonly') && !/^(checkbox|radio|file|color|range|submit|image)$/.test(t)) return false;
    } else if (n === 'textarea') {
      if (el.hasAttribute('readonly')) return false;
    } else if (n === 'button') {
      var bt = String(el.getAttribute('type') || 'submit').toLowerCase();
      if (bt === 'reset' || bt === 'button') return false;
    } else if (n !== 'select') {
      return false;
    }
    return !gesperrt(el) && !inDatalist(el);
  }
  var MIT_LAENGE = /^(text|search|url|tel|email|password)$/;
  var ZAHLEN = /^(date|month|week|time|datetime-local|number|range)$/;
  function emailGueltig(t) {
    return /^[a-zA-Z0-9.!#$%&'*+\/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/.test(t);
  }
  function urlGueltig(t) { try { new g.URL(t); return true; } catch (e) { return false; } }
  function musterPasst(el, wert) {
    var m = el.getAttribute('pattern');
    if (m === null) return true;
    var re;
    try { re = new RegExp('^(?:' + m + ')$', 'v'); } catch (e) {
      try { re = new RegExp('^(?:' + m + ')$', 'u'); } catch (e2) { return true; }
    }
    if (typ(el) === 'email' && el.hasAttribute('multiple')) {
      return wert.split(',').every(function (w) { return re.test(w); });
    }
    return re.test(wert);
  }
  function radioGruppe(el) {
    var name = el.getAttribute('name');
    if (!name) return [el];
    var wurzel = el.form || (el.getRootNode ? el.getRootNode() : doc);
    var alle = wurzel.querySelectorAll ? wurzel.querySelectorAll('input[type=radio i]') : [];
    return Array.prototype.filter.call(alle, function (r) { return r.getAttribute('name') === name && r.form === el.form; });
  }
  // Release 13.10.2026: HTML §4.10.5.3.4 „required": bei Text-Feldern und
  // `<textarea>` fehlt ein Wert nur, wenn das Feld VERAENDERBAR ist (nicht
  // deaktiviert, nicht schreibgeschuetzt).
  function veraenderbar(el) {
    // `matches(':disabled')` kennt bei abgetrennten Klonen den Zustand nicht —
    // darum zuerst das Attribut und ein deaktiviertes `fieldset` darueber.
    var aus = !!el.disabled;
    if (!aus) { try { aus = !!(el.closest && el.closest('fieldset[disabled]')) || el.matches(':disabled'); } catch (e) {} }
    return !aus && !el.readOnly;
  }
  function flaggen(el) {
    var f = { valueMissing: false, typeMismatch: false, patternMismatch: false, tooLong: false, tooShort: false,
      rangeUnderflow: false, rangeOverflow: false, stepMismatch: false, badInput: false, customError: !!eigene.get(el) };
    var n = el.localName, pflicht = el.hasAttribute('required');
    if (n === 'input') {
      var t = typ(el), wert = el.value;
      if (pflicht) {
        if (t === 'checkbox') f.valueMissing = !el.checked;
        else if (t === 'radio') f.valueMissing = !radioGruppe(el).some(function (r) { return r.checked; });
        else if (t === 'file') f.valueMissing = !(el.files && el.files.length);
        else if (!/^(range|color|hidden|submit|image|reset|button)$/.test(t)) f.valueMissing = wert === '' && veraenderbar(el);
      }
      if (wert !== '') {
        if (t === 'email') f.typeMismatch = !(el.hasAttribute('multiple') ? wert.split(',').every(emailGueltig) : emailGueltig(wert));
        else if (t === 'url') f.typeMismatch = !urlGueltig(wert);
        if (MIT_LAENGE.test(t)) f.patternMismatch = !musterPasst(el, wert);
        if (ZAHLEN.test(t)) {
          var v = el.valueAsNumber;
          var minA = el.getAttribute('min'), maxA = el.getAttribute('max');
          var probe = g.document.createElement('input');
          probe.type = t;
          var zahl = function (s) { probe.value = s; return probe.valueAsNumber; };
          var min = minA === null ? NaN : zahl(minA), max = maxA === null ? NaN : zahl(maxA);
          if (!isNaN(v)) {
            if (!isNaN(min) && !isNaN(max) && min > max && t === 'time') {
              // Umgekehrter Zeitbereich (§4.10.5.1.9): ueber Mitternacht.
              f.rangeUnderflow = v > max && v < min;
              f.rangeOverflow = f.rangeUnderflow;
            } else {
              f.rangeUnderflow = !isNaN(min) && v < min;
              f.rangeOverflow = !isNaN(max) && v > max;
            }
            var s = el.getAttribute('step');
            if (!(s !== null && s.toLowerCase() === 'any') && t !== 'range') {
              var vorgaben = { number: [1, 1], date: [1, 86400000], month: [1, 1], week: [1, 604800000], time: [60, 1000], 'datetime-local': [60, 1000] };
              var st = s === null ? NaN : parseFloat(s);
              if (!(st > 0)) st = vorgaben[t][0];
              st *= vorgaben[t][1];
              var basisA = minA !== null ? min : (el.getAttribute('value') !== null ? zahl(el.getAttribute('value')) : (t === 'week' ? -259200000 : 0));
              if (isNaN(basisA)) basisA = t === 'week' ? -259200000 : 0;
              var q = (v - basisA) / st;
              f.stepMismatch = Math.abs(q - Math.round(q)) > 1e-7;
            }
          }
        }
      }
    } else if (n === 'textarea') {
      if (pflicht) f.valueMissing = el.value === '' && veraenderbar(el);
    } else if (n === 'select') {
      if (pflicht) {
        var opts = el.options || el.querySelectorAll('option');
        var sel = Array.prototype.filter.call(opts, function (o) { return o.selected; });
        var erste = opts[0];
        var platzhalter = !el.multiple && !(Number(el.getAttribute('size')) > 1) && erste && erste.value === '' && erste.parentNode === el;
        f.valueMissing = sel.length === 0 || (platzhalter && sel.length === 1 && sel[0] === erste);
      }
    }
    return f;
  }
  function gueltig(f) {
    for (var k in f) if (f[k]) return false;
    return true;
  }
  var NACHRICHT = {
    valueMissing: 'Please fill out this field.', typeMismatch: 'Please enter a valid value.', patternMismatch: 'Please match the requested format.',
    rangeUnderflow: 'Value must be greater than or equal to the minimum.', rangeOverflow: 'Value must be less than or equal to the maximum.',
    stepMismatch: 'Please enter a valid value.', tooLong: 'Please shorten this text.', tooShort: 'Please lengthen this text.', badInput: 'Please enter a valid value.'
  };
  function validityVon(el) {
    var v = Object.create(VS);
    Object.keys(NACHRICHT).concat(['customError']).forEach(function (k) {
      // Release 13.10.2026: ValidityState spiegelt die Zustaende auch fuer ein
      // ausgenommenes Feld (deaktiviert/schreibgeschuetzt); nur `checkValidity`
      // fragt nach der Kandidatur. GEMESSEN vorher (WPT constraints/*): „should
      // be true, when control is disabled" — 144 Teiltests.
      Object.defineProperty(v, k, { get: function () { return flaggen(el)[k]; }, enumerable: true });
    });
    Object.defineProperty(v, 'valid', { get: function () { return gueltig(flaggen(el)); }, enumerable: true });
    return v;
  }
  var cache = new WeakMap();
  function pruefen(el) {
    if (!kandidat(el) || gueltig(flaggen(el))) return true;
    var ev;
    try { ev = globalThis.__narsilUA(Event, 'invalid', { cancelable: true }); } catch (e) { ev = null; }
    if (ev) el.dispatchEvent(ev);
    return false;
  }
  ['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement', 'HTMLButtonElement', 'HTMLFieldSetElement', 'HTMLOutputElement', 'HTMLObjectElement'].forEach(function (n) {
    var P = g[n] && g[n].prototype;
    if (!P || Object.getOwnPropertyDescriptor(P, 'validity')) return;
    eig(P, 'willValidate', function () { return kandidat(this); });
    eig(P, 'validity', function () { var v = cache.get(this); if (!v) { v = validityVon(this); cache.set(this, v); } return v; });
    eig(P, 'validationMessage', function () {
      if (!kandidat(this)) return '';
      var f = flaggen(this);
      if (gueltig(f)) return '';
      var e = eigene.get(this);
      if (e) return e;
      for (var k in NACHRICHT) if (f[k]) return NACHRICHT[k];
      return '';
    });
    def(P, 'checkValidity', function checkValidity() { return pruefen(this); });
    def(P, 'reportValidity', function reportValidity() { return pruefen(this); });
    def(P, 'setCustomValidity', function setCustomValidity(m) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'setCustomValidity': 1 argument required, but only 0 present.");
      eigene.set(this, String(m));
    });
  });
  // Release 13.10.2026: `output.type` ist "output", `fieldset.type`
  // "fieldset" (HTML §4.10.12, §4.10.15).
  [['HTMLOutputElement', 'output'], ['HTMLFieldSetElement', 'fieldset']].forEach(function (e) {
    var P = g[e[0]] && g[e[0]].prototype;
    if (P) { try { Object.defineProperty(P, 'type', { get: function () { return e[1]; }, configurable: true, enumerable: true }); } catch (x) {} }
  });
  var F = g.HTMLFormElement && g.HTMLFormElement.prototype;
  function felder(form) {
    var alle = form.elements ? Array.prototype.slice.call(form.elements) : Array.prototype.slice.call(form.querySelectorAll('input,select,textarea,button'));
    return alle;
  }
  if (F && typeof F.checkValidity !== 'function') {
    def(F, 'checkValidity', function checkValidity() {
      var ok = true;
      felder(this).forEach(function (el) { if (typeof el.checkValidity === 'function' && !el.checkValidity()) ok = false; });
      return ok;
    });
    def(F, 'reportValidity', function reportValidity() { return this.checkValidity(); });
  }
})();

// -- <select>/<option>: Auswahlzustand und HTMLOptionsCollection (HTML §4.10.7) --
// Release 11.10.2026: die Auswahl einer Option („selectedness" mit
// „dirtiness"), der „selectedness setting algorithm" (genau eine bei
// `select-one`, sonst die erste aktivierte), `select.value`/`type`/
// `selectedIndex`/`selectedOptions`/`length`/`add`/`remove`/`item`/
// `namedItem`, `options` als lebende Sammlung, `option.selected`/`index`/
// `text`/`label`/`defaultSelected`/`form`. GEMESSEN vorher (WPT
// the-select-element): `select.options` war undefined, `select.value` der
// Text aller Optionen.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var S = g.HTMLSelectElement && g.HTMLSelectElement.prototype;
  var O = g.HTMLOptionElement && g.HTMLOptionElement.prototype;
  if (!S || !O || Object.getOwnPropertyDescriptor(S, 'selectedIndex')) return;
  var XHTML = 'http://www.w3.org/1999/xhtml';
  var zustand = new WeakMap(); // option -> { schmutzig, an }
  function eig(o, n, lesen, schreiben) { try { Object.defineProperty(o, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {} }
  function def(o, n, f) { try { Object.defineProperty(o, n, { value: f, writable: true, configurable: true, enumerable: true }); } catch (e) {} }
  function istHtml(k, n) { return !!k && k.localName === n && k.namespaceURI === XHTML; }
  // Die Optionsliste (§4.10.7): Kinder-<option> und <option> in Kind-<optgroup>.
  function optionen(sel) {
    var raus = [];
    for (var k = sel.firstElementChild; k; k = k.nextElementSibling) {
      if (istHtml(k, 'option')) raus.push(k);
      else if (istHtml(k, 'optgroup')) {
        for (var o = k.firstElementChild; o; o = o.nextElementSibling) if (istHtml(o, 'option')) raus.push(o);
      }
    }
    return raus;
  }
  function auswahlVon(o) {
    var p = o.parentElement;
    if (istHtml(p, 'select')) return p;
    if (istHtml(p, 'optgroup') && istHtml(p.parentElement, 'select')) return p.parentElement;
    return null;
  }
  function mehrfach(sel) { return sel.hasAttribute('multiple'); }
  function groesse(sel) {
    var m = /^[\t\n\f\r ]*\+?([0-9]+)/.exec(sel.getAttribute('size') || '');
    var n = m ? parseInt(m[1], 10) : 0;
    return n > 0 ? n : (mehrfach(sel) ? 4 : 1);
  }
  function rohGewaehlt(o) {
    var z = zustand.get(o);
    if (!z) return o.hasAttribute('selected');
    // Not dirty: adding/removing the `selected` attribute sets the
    // selectedness (HTML §4.10.10).
    if (!z.schmutzig && z.attr !== undefined) {
      var h = o.hasAttribute('selected');
      if (h !== z.attr) { z.attr = h; z.an = h; }
    }
    return z.an;
  }
  try {
    Object.defineProperty(g, '__narsilOptionAuswahl', {
      configurable: true, writable: true,
      value: function (o, an) { zustand.set(o, { schmutzig: false, an: an, attr: o.hasAttribute('selected') }); }
    });
  } catch (e) {}
  function gesperrt(o) {
    if (o.hasAttribute('disabled')) return true;
    var p = o.parentElement;
    return istHtml(p, 'optgroup') && p.hasAttribute('disabled');
  }
  // „selectedness setting algorithm" (§4.10.7) auf den aktuellen Stand.
  function gewaehlte(sel) {
    var opts = optionen(sel), an = opts.filter(rohGewaehlt);
    if (mehrfach(sel)) return an;
    if (an.length > 1) return [an[an.length - 1]];
    if (!an.length && groesse(sel) === 1) {
      for (var i = 0; i < opts.length; i++) if (!gesperrt(opts[i])) return [opts[i]];
    }
    return an;
  }
  function setzeEine(sel, ziel) {
    optionen(sel).forEach(function (o) { zustand.set(o, { schmutzig: o === ziel, an: o === ziel }); });
  }
  eig(O, 'selected', function () {
    var sel = auswahlVon(this);
    return sel ? gewaehlte(sel).indexOf(this) >= 0 : rohGewaehlt(this);
  }, function (v) {
    var sel = auswahlVon(this);
    if (v && sel && !mehrfach(sel)) { setzeEine(sel, this); return; }
    zustand.set(this, { schmutzig: true, an: !!v });
  });
  eig(O, 'defaultSelected', function () { return this.hasAttribute('selected'); }, function (v) {
    if (v) this.setAttribute('selected', ''); else this.removeAttribute('selected');
  });
  eig(O, 'index', function () { var sel = auswahlVon(this); return sel ? optionen(sel).indexOf(this) : 0; });
  function textVon(o) {
    var t = '';
    (function lauf(k) {
      for (var c = k.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) t += c.data;
        else if (c.nodeType === 1 && !(c.localName === 'script' && (c.namespaceURI === XHTML || c.namespaceURI === 'http://www.w3.org/2000/svg'))) lauf(c);
      }
    })(o);
    return t.split(/[\t\n\f\r ]+/).filter(Boolean).join(' ');
  }
  eig(O, 'text', function () { return textVon(this); }, function (v) { this.textContent = String(v); });
  eig(O, 'label', function () { var l = this.getAttribute('label'); return l !== null ? l : textVon(this); }, function (v) { this.setAttribute('label', String(v)); });
  eig(O, 'form', function () { var sel = auswahlVon(this); return sel ? sel.form : null; });
  // Wird das `selected`-Attribut einer nicht „schmutzigen" Option geaendert,
  // folgt die Auswahl ihm (das erledigt `rohGewaehlt` von selbst).

  // -- select --
  eig(S, 'type', function () { return mehrfach(this) ? 'select-multiple' : 'select-one'; });
  eig(S, 'value', function () { var a = gewaehlte(this); return a.length ? a[0].value : ''; }, function (v) {
    v = String(v);
    var opts = optionen(this), treffer = null;
    for (var i = 0; i < opts.length; i++) if (opts[i].value === v) { treffer = opts[i]; break; }
    opts.forEach(function (o) { zustand.set(o, { schmutzig: true, an: o === treffer }); });
  });
  eig(S, 'selectedIndex', function () { var a = gewaehlte(this); return a.length ? optionen(this).indexOf(a[0]) : -1; }, function (v) {
    var n = Number(v) | 0, opts = optionen(this), ziel = opts[n] || null;
    opts.forEach(function (o) { zustand.set(o, { schmutzig: true, an: o === ziel }); });
  });
  eig(S, 'selectedOptions', function () {
    var a = gewaehlte(this), sel = this;
    var l = a.slice();
    def(l, 'item', function (i) { return l[i] || null; });
    def(l, 'namedItem', function (n) { return benannt(l, n); });
    return l;
  });
  function benannt(liste, n) {
    n = String(n);
    if (!n) return null;
    for (var i = 0; i < liste.length; i++) if (liste[i].id === n) return liste[i];
    for (var j = 0; j < liste.length; j++) if (liste[j].getAttribute('name') === n) return liste[j];
    return null;
  }
  function laengeSetzen(sel, n) {
    n = Number(n) >>> 0;
    var opts = optionen(sel);
    if (n > 100000) return;
    if (n < opts.length) { for (var i = opts.length - 1; i >= n; i--) opts[i].parentNode.removeChild(opts[i]); }
    else { for (var j = opts.length; j < n; j++) sel.appendChild(sel.ownerDocument.createElement('option')); }
  }
  function hinzu(sel, el, vor) {
    if (!el || !(istHtml(el, 'option') || istHtml(el, 'optgroup'))) throw new TypeError("Failed to execute 'add' on 'HTMLSelectElement': The provided value is not of type '(HTMLOptGroupElement or HTMLOptionElement)'.");
    if (el.contains && el.contains(sel)) throw new DOMException('The new child element contains the parent.', 'HierarchyRequestError');
    var bezug = null;
    if (typeof vor === 'number') bezug = optionen(sel)[vor] || null;
    else if (vor !== undefined && vor !== null) {
      bezug = vor;
      if (!sel.contains(bezug)) throw new DOMException("Failed to execute 'add': The node before which the new node is to be inserted is not a child of this node.", 'NotFoundError');
    }
    if (bezug) bezug.parentNode.insertBefore(el, bezug); else sel.appendChild(el);
  }
  eig(S, 'length', function () { return optionen(this).length; }, function (n) { laengeSetzen(this, n); });
  def(S, 'item', function item(i) { return optionen(this)[Number(i) >>> 0] || null; });
  def(S, 'namedItem', function namedItem(n) { return benannt(optionen(this), n); });
  def(S, 'add', function add(el, vor) { hinzu(this, el, vor); });
  var altEntfernen = S.remove || (g.Element && g.Element.prototype.remove);
  def(S, 'remove', function remove(i) {
    if (arguments.length === 0) {
      if (typeof altEntfernen === 'function') return altEntfernen.call(this);
      if (this.parentNode) this.parentNode.removeChild(this);
      return;
    }
    var o = optionen(this)[Number(i) | 0];
    if (o && Number(i) >= 0) o.parentNode.removeChild(o);
  });
  // `options`: lebende Sammlung (HTMLOptionsCollection) mit Index-Zugriff.
  var sammlungen = new WeakMap();
  function OptionenSammlung() { throw new TypeError('Illegal constructor'); }
  var OP = OptionenSammlung.prototype;
  eig(OP, 'length', function () { return optionen(this._sel).length; }, function (n) { laengeSetzen(this._sel, n); });
  eig(OP, 'selectedIndex', function () { return this._sel.selectedIndex; }, function (v) { this._sel.selectedIndex = v; });
  def(OP, 'item', function item(i) { return optionen(this._sel)[Number(i) >>> 0] || null; });
  def(OP, 'namedItem', function namedItem(n) { return benannt(optionen(this._sel), n); });
  def(OP, 'add', function add(el, vor) { hinzu(this._sel, el, vor); });
  def(OP, 'remove', function remove(i) { var o = optionen(this._sel)[Number(i) | 0]; if (o && Number(i) >= 0) o.parentNode.removeChild(o); });
  if (typeof g.Symbol === 'function' && g.Symbol.iterator) def(OP, g.Symbol.iterator, function () { return optionen(this._sel)[g.Symbol.iterator](); });
  if (!g.HTMLOptionsCollection) g.HTMLOptionsCollection = OptionenSammlung;
  if (typeof g.Symbol === 'function' && g.Symbol.toStringTag) {
    try { Object.defineProperty(OP, g.Symbol.toStringTag, { value: 'HTMLOptionsCollection', configurable: true }); } catch (e) {}
  }
  eig(S, 'options', function () {
    var c = sammlungen.get(this);
    if (c) return c;
    var ziel = Object.create(OP);
    Object.defineProperty(ziel, '_sel', { value: this });
    var index = function (k) { return typeof k === 'string' && /^(0|[1-9][0-9]*)$/.test(k); };
    c = typeof g.Proxy === 'function' ? new g.Proxy(ziel, {
      get: function (t, k) { if (index(k)) return optionen(t._sel)[+k]; return t[k]; },
      set: function (t, k, v) {
        if (index(k)) {
          // `options[i] = option` ersetzt bzw. fuellt auf (§4.10.7 „setter").
          var i = +k, opts = optionen(t._sel);
          if (v === null || v === undefined) { if (opts[i]) opts[i].parentNode.removeChild(opts[i]); return true; }
          if (i >= opts.length) { laengeSetzen(t._sel, i); t._sel.appendChild(v); }
          else opts[i].parentNode.replaceChild(v, opts[i]);
          return true;
        }
        t[k] = v;
        return true;
      },
      has: function (t, k) { if (index(k)) return +k < optionen(t._sel).length; return k in t; },
      // Release 14.10.2026: the indices are own properties (WebIDL indexed
      // getter). MEASURED before (WPT select-remove): `options.hasOwnProperty(0)`
      // was false, assert_array_equals saw every option as "missing".
      getOwnPropertyDescriptor: function (t, k) {
        if (index(k)) {
          var o = optionen(t._sel)[+k];
          return o ? { value: o, writable: true, enumerable: true, configurable: true } : undefined;
        }
        return Object.getOwnPropertyDescriptor(t, k);
      },
      ownKeys: function (t) {
        var n = optionen(t._sel).length, keys = [];
        for (var i = 0; i < n; i++) keys.push(String(i));
        return keys.concat(Reflect.ownKeys(t));
      }
    }) : ziel;
    sammlungen.set(this, c);
    return c;
  });
})();

// -- select.selectedOptions as a live HTMLCollection (HTML §4.10.7) --
// Release 2026-10-15: one object per select over the selectedness the
// select block keeps. MEASURED before: a fresh array on every read.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  // select.selectedOptions: a live HTMLCollection over the selectedness the
  // select block keeps (one object per select).
  var SP = g.HTMLSelectElement && g.HTMLSelectElement.prototype;
  var HC = g.HTMLCollection && g.HTMLCollection.prototype;
  var alt = SP && Object.getOwnPropertyDescriptor(SP, 'selectedOptions');
  if (SP && HC && alt && typeof alt.get === 'function' && typeof g.Proxy === 'function') {
    var lists = new WeakMap();
    var isIndex = function (k) { return typeof k === 'string' && /^(?:0|[1-9][0-9]{0,9})$/.test(k); };
    Object.defineProperty(SP, 'selectedOptions', {
      configurable: true, enumerable: true,
      get: function () {
        var c = lists.get(this);
        if (c) return c;
        var sel = this;
        var now = function () { return alt.get.call(sel); };
        c = new g.Proxy(Object.create(HC), {
          get: function (t, k, r) {
            if (k === 'length') return now().length;
            if (isIndex(k)) return now()[+k];
            return Reflect.get(t, k, r);
          },
          set: function (t, k, v, r) { if (isIndex(k) || k === 'length') return false; return Reflect.set(t, k, v, r); },
          has: function (t, k) { if (isIndex(k)) return +k < now().length; return k === 'length' || Reflect.has(t, k); },
          ownKeys: function (t) { var n = now().length, keys = []; for (var i = 0; i < n; i++) keys.push(String(i)); return keys.concat(Reflect.ownKeys(t)); },
          getOwnPropertyDescriptor: function (t, k) {
            if (isIndex(k)) { var o = now()[+k]; return o ? { value: o, writable: false, enumerable: true, configurable: true } : undefined; }
            return Reflect.getOwnPropertyDescriptor(t, k);
          }
        });
        lists.set(this, c);
        return c;
      }
    });
  }
})();

// -- Bildlauf der Seite (CSSOM View §4, §6) --
// Release 11.10.2026: `scrollTo`/`scroll`/`scrollBy`, `scrollX`/`scrollY`/
// `pageX-/YOffset`, `scrollIntoView` und `scrollTop` an `<html>`/`<body>` rollen
// die Seite wirklich (der Host klemmt und laedt sichtbare Lazy-Bilder nach,
// `scroll` kommt im naechsten Takt). GEMESSEN vorher: alles waren Attrappen
// (`scrollY` immer 0), Lazy-Bilder unter der Sicht luden nie.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var rollen = g.__narsilRollen, lesen = g.__narsilRollenLesen;
  if (!doc || typeof rollen !== 'function') return;
  function y() { try { return typeof lesen === 'function' ? lesen() : 0; } catch (e) { return 0; } }
  function nach(z) { var n = Number(z); try { rollen(isFinite(n) ? Math.max(0, n) : 0); } catch (e) {} }
  function lesenWert(o, schluessel) { return o && typeof o === 'object' ? o[schluessel] : undefined; }
  ['scrollY', 'pageYOffset'].forEach(function (n) {
    try { Object.defineProperty(g, n, { get: y, set: function (v) { Object.defineProperty(g, n, { value: v, writable: true, configurable: true }); }, configurable: true, enumerable: true }); } catch (e) {}
  });
  ['scrollX', 'pageXOffset'].forEach(function (n) {
    try { Object.defineProperty(g, n, { get: function () { return 0; }, set: function (v) { Object.defineProperty(g, n, { value: v, writable: true, configurable: true }); }, configurable: true, enumerable: true }); } catch (e) {}
  });
  function scrollTo(a, b) {
    var ziel = arguments.length >= 2 ? b : lesenWert(a, 'top');
    if (ziel === undefined) return;
    nach(ziel);
  }
  g.scrollTo = scrollTo;
  g.scroll = function scroll(a, b) { scrollTo.apply(this, arguments); };
  g.scrollBy = function scrollBy(a, b) {
    var d = arguments.length >= 2 ? Number(b) : Number(lesenWert(a, 'top') || 0);
    nach(y() + (isFinite(d) ? d : 0));
  };
  var E = g.Element && g.Element.prototype;
  if (E) {
    // Release 11.10.2026: Rollstand der ELEMENTE (CSSOM View §7
    // „scrollTop"/„scrollLeft", §6 „scroll an element"): nur ein
    // Rollbehaelter (overflow auto/scroll/hidden/...) rollt, geklemmt an
    // `scrollHeight - clientHeight`; `scroll` kommt im naechsten Bild;
    // `getBoundingClientRect` der Nachfahren rueckt um den Rollstand.
    // GEMESSEN vorher: `scrollTop` war ein freies Feld (jeder Wert blieb
    // stehen, nichts rollte) — nach dem Seiten-`scrollTop` immer 0 (WPT
    // elementScroll-002, scrollintoview-*).
    var stand = new WeakMap(), anzahl = 0, melden = [];
    var mass = g.__narsilMass;
    function wurzel(el) { return el === doc.documentElement || (el === doc.body && doc.compatMode === 'BackCompat'); }
    function behaelter(el) {
      var cs;
      try { cs = g.getComputedStyle(el); } catch (e) { return false; }
      if (!cs || cs.display === 'none' || cs.display === 'contents' || cs.display === 'inline') return false;
      var ox = cs.overflowX || cs.overflow, oy = cs.overflowY || cs.overflow;
      return ox === 'auto' || ox === 'scroll' || ox === 'hidden' || oy === 'auto' || oy === 'scroll' || oy === 'hidden';
    }
    function grenze(el, breit) {
      var n = breit ? el.scrollWidth - el.clientWidth : el.scrollHeight - el.clientHeight;
      return n > 0 ? n : 0;
    }
    function lage(el, i) {
      var s = stand.get(el);
      if (!s) return 0;
      var max = grenze(el, i === 0);
      return s[i] > max ? max : s[i];
    }
    function zahlOder0(v) { var n = Number(v); return isFinite(n) ? n : 0; }
    function feuern() {
      var l = melden; melden = [];
      for (var i = 0; i < l.length; i++) {
        try { l[i].dispatchEvent(g.__narsilUA(g.Event, 'scroll')); } catch (e) {}
        try { l[i].dispatchEvent(g.__narsilUA(g.Event, 'scrollend')); } catch (e) {}
      }
    }
    function setzen(el, x, y2) {
      if (!el.isConnected || !behaelter(el)) return;
      var alt = stand.get(el) || [0, 0];
      var nx = x === null ? alt[0] : Math.min(Math.max(0, x), grenze(el, true));
      var ny = y2 === null ? alt[1] : Math.min(Math.max(0, y2), grenze(el, false));
      if (nx === lage(el, 0) && ny === lage(el, 1)) return;
      if (!stand.has(el)) anzahl++;
      stand.set(el, [nx, ny]);
      if (melden.indexOf(el) < 0 && melden.push(el) === 1) {
        if (typeof g.requestAnimationFrame === 'function') g.requestAnimationFrame(feuern); else g.setTimeout(feuern, 0);
      }
    }
    function zugriff(name, i) {
      try {
        Object.defineProperty(E, name, {
          configurable: true, enumerable: true,
          get: function () {
            if (wurzel(this)) return i === 1 ? y() : 0;
            return lage(this, i);
          },
          set: function (v) {
            var n = zahlOder0(v);
            if (wurzel(this)) { if (i === 1) nach(n); return; }
            setzen(this, i === 0 ? n : null, i === 1 ? n : null);
          }
        });
      } catch (e) {}
    }
    zugriff('scrollLeft', 0);
    zugriff('scrollTop', 1);
    // `scrollTo`/`scroll`/`scrollBy` (§6): ScrollToOptions oder (x, y).
    function ziele(args, el, relativ) {
      var x = null, z = null, o = args[0];
      if (args.length >= 2) { x = zahlOder0(args[0]); z = zahlOder0(args[1]); }
      else if (o !== undefined && o !== null) {
        if (typeof o !== 'object' && typeof o !== 'function') throw new TypeError("Failed to execute 'scrollTo' on 'Element': The provided value is not of type 'ScrollToOptions'.");
        if (o.left !== undefined) x = zahlOder0(o.left);
        if (o.top !== undefined) z = zahlOder0(o.top);
      }
      if (relativ) {
        if (x !== null) x += wurzel(el) ? 0 : lage(el, 0);
        if (z !== null) z += wurzel(el) ? y() : lage(el, 1);
      }
      return [x, z];
    }
    function elementRollen(el, args, relativ) {
      var t = ziele(args, el, relativ);
      if (wurzel(el)) { if (t[1] !== null) nach(t[1]); return; }
      setzen(el, t[0], t[1]);
    }
    // Release 11.10.2026: die Bildlauf-Methoden liefern ein Promise, das
    // sich nach dem (sofortigen) Rollen erfuellt (CSSOM View §6).
    function versprochen(f) {
      return function () {
        f.apply(this, arguments);
        return typeof g.Promise === 'function' ? g.Promise.resolve() : undefined;
      };
    }
    E.scrollTo = versprochen(function scrollTo() { elementRollen(this, arguments, false); });
    E.scroll = versprochen(function scroll() { elementRollen(this, arguments, false); });
    E.scrollBy = versprochen(function scrollBy() { elementRollen(this, arguments, true); });
    // `scrollParent` (CSSOM View §7): der naechste Rollbehaelter im flachen
    // Baum; `fixed` ohne Vorfahr mit Containing Block -> null; am Ende das
    // rollende Element des Dokuments.
    try {
      Object.defineProperty(E, 'scrollParent', {
        configurable: true, enumerable: true,
        get: function () {
          if (this === doc.documentElement || this === doc.body || !this.isConnected) return null;
          var cs;
          try { cs = g.getComputedStyle(this); } catch (e) { return null; }
          if (cs && (cs.display === 'none' || cs.position === 'fixed')) return null;
          var abs = cs && cs.position === 'absolute';
          for (var p = this.assignedSlot || this.parentNode; p; p = p.assignedSlot || p.parentNode || p.host || null) {
            if (p.nodeType === 11) continue;
            if (p.nodeType !== 1) break;
            if (p === doc.documentElement || p === doc.body) break;
            var pc;
            try { pc = g.getComputedStyle(p); } catch (e) { continue; }
            if (abs && pc && pc.position === 'static') continue;
            if (behaelter(p)) return p;
            if (pc && pc.position !== 'static') abs = pc.position === 'absolute';
          }
          return doc.scrollingElement || doc.documentElement;
        }
      });
    } catch (e) {}
    // Rechtecke der Nachfahren eines gerollten Behaelters ruecken mit.
    var Rect = g.DOMRect;
    function versatz(el) {
      if (!anzahl) return null;
      var dx = 0, dy = 0;
      for (var p = el.parentElement; p; p = p.parentElement) {
        if (stand.has(p)) { dx += lage(p, 0); dy += lage(p, 1); }
      }
      if (dy) dy -= kleben(el, dy);
      return dx || dy ? [dx, dy] : null;
    }
    // CSS Position 3 §3.4: ein `position: sticky` mit `top` bleibt an der
    // Oberkante seines gerollten Behaelters haengen, hoechstens bis zum
    // Ende seines Elternkastens. Liefert die Verschiebung nach unten.
    function kleben(el, dy) {
      for (var n = el; n && !stand.has(n); n = n.parentElement) {
        var cs;
        try { cs = g.getComputedStyle(n); } catch (e) { return 0; }
        if (!cs || cs.position !== 'sticky') continue;
        var oben = parseFloat(cs.top);
        if (!isFinite(oben)) return 0;
        var c = n.parentElement;
        while (c && !stand.has(c)) c = c.parentElement;
        if (!c) return 0;
        var rn = gbcr.call(n), rc = gbcr.call(c), vc = versatz(c);
        var obenC = rc.top - (vc ? vc[1] : 0) + c.clientTop;
        var jetzt = rn.top - dy, ziel = obenC + oben;
        if (jetzt >= ziel) return 0;
        var eltern = n.parentElement, maxVer = Infinity;
        if (eltern && eltern !== c) { var re = gbcr.call(eltern); maxVer = Math.max(0, (re.bottom - dy) - (rn.bottom - dy)); }
        return Math.min(ziel - jetzt, maxVer);
      }
      return 0;
    }
    var gbcr = E.getBoundingClientRect, gcr = E.getClientRects;
    if (typeof gbcr === 'function' && typeof Rect === 'function') {
      E.getBoundingClientRect = function getBoundingClientRect() {
        var r = gbcr.call(this), v = versatz(this);
        return v ? new Rect(r.x - v[0], r.y - v[1], r.width, r.height) : r;
      };
    }
    if (typeof gcr === 'function' && typeof Rect === 'function') {
      E.getClientRects = function getClientRects() {
        var l = gcr.call(this), v = versatz(this);
        if (!v) return l;
        var a = [];
        for (var i = 0; i < l.length; i++) a.push(new Rect(l[i].x - v[0], l[i].y - v[1], l[i].width, l[i].height));
        a.item = function (i) { return this[i] || null; };
        return a;
      };
    }
    // `scrollIntoView` (§6 „scroll a target into view"): jeder Behaelter
    // von innen nach aussen, zuletzt die Seite.
    function ziel(jetzt, a, b, sa, sb, art) {
      var h = sb - sa, eh = b - a;
      if (art === 'end') return jetzt + b - sb;
      if (art === 'center') return jetzt + (a + eh / 2) - (sa + h / 2);
      if (art === 'wenn-noetig') return a >= sa && b <= sb ? jetzt : jetzt + (a + eh / 2) - (sa + h / 2);
      if (art === 'nearest') {
        if (a >= sa && b <= sb) return jetzt;
        if (a < sa && b > sb) return jetzt;
        if ((a < sa && eh <= h) || (b > sb && eh > h)) return jetzt + a - sa;
        return jetzt + b - sb;
      }
      return jetzt + a - sa;
    }
    function einrollen(el, block, inline) {
      try { if (typeof mass === 'function' && !mass(el)) return; } catch (e) { return; }
      for (var p = el.parentElement; p && !wurzel(p) && p !== doc.body; p = p.parentElement) {
        if (!behaelter(p)) continue;
        var r = el.getBoundingClientRect(), q = p.getBoundingClientRect();
        var oben = q.top + p.clientTop, links = q.left + p.clientLeft;
        setzen(p, ziel(lage(p, 0), r.left, r.right, links, links + p.clientWidth, inline),
          ziel(lage(p, 1), r.top, r.bottom, oben, oben + p.clientHeight, block));
      }
      var r2 = el.getBoundingClientRect(), h = g.innerHeight || 600, jetzt = y();
      var neu = ziel(jetzt, r2.top, r2.bottom, 0, h, block);
      if (neu !== jetzt) nach(neu);
    }
    E.scrollIntoView = function scrollIntoView(opt) {
      var block = 'start', inline = 'nearest';
      if (opt === false) block = 'end';
      else if (opt && typeof opt === 'object') {
        if (opt.block) block = String(opt.block);
        if (opt.inline) inline = String(opt.inline);
      }
      einrollen(this, block, inline);
    };
    E.scrollIntoViewIfNeeded = function scrollIntoViewIfNeeded(mitte) {
      var art = mitte === false ? 'nearest' : 'wenn-noetig';
      einrollen(this, art, art);
    };
  }
  if (!('scrollingElement' in doc)) {
    try { Object.defineProperty(doc, 'scrollingElement', { get: function () { return doc.compatMode === 'BackCompat' ? doc.body : doc.documentElement; }, configurable: true }); } catch (e) {}
  }
})();

// -- <img>: decode(), currentSrc (HTML §4.8.4.4, §4.8.4.3) --
// Release 11.10.2026: `decode()` erfuellt sich, wenn das Bild geladen ist
// (sonst EncodingError); `currentSrc` ist die gewaehlte Quelle (`srcset`
// nach Dichte bzw. Breite bei Viewport-Breite, sonst `src`). GEMESSEN vorher
// (WPT the-img-element): „decode is not a function", `currentSrc`
// undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var P = g.HTMLImageElement && g.HTMLImageElement.prototype;
  if (!P) return;
  // Release 2026-10-15: a source that does not parse is the current URL as
  // written (HTML §4.8.4.3.5: the current request's current URL is the
  // selected source; WPT update-the-image-data/fail-to-resolve).
  function absolut(u, el) { try { return new g.URL(u, el.baseURI || g.document.baseURI).href; } catch (e) { return String(u); } }
  // Release 2026-10-15: HTML §4.8.4.3.10 "parse a srcset attribute" (the
  // splitting loop, the descriptor tokenizer with parentheses, the
  // descriptor parser for w/x/h) and §4.8.4.3.11 "parse a sizes attribute";
  // the source set gets `src` as 1x unless a 1x or w candidate exists, equal
  // densities keep the first, and the smallest density at least the device
  // pixel ratio wins (else the largest). MEASURED before (WPT
  // srcset/parse-a-srcset-attribute 1/236): commas split URLs ("data:"),
  // invalid descriptors were accepted.
  function istLeer(c) { return c === ' ' || c === '\t' || c === '\n' || c === '\f' || c === '\r'; }
  function istGanz(t) { return /^[0-9]+$/.test(t); }
  function istGleit(t) { return /^-?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$/.test(t); }
  function kandidaten(eingabe) {
    var t = String(eingabe === null || eingabe === undefined ? '' : eingabe), n = t.length, pos = 0, raus = [];
    for (var runde = 0; runde < 100000; runde++) {
      while (pos < n && (istLeer(t[pos]) || t[pos] === ',')) pos++;
      if (pos >= n) return raus;
      var start = pos;
      while (pos < n && !istLeer(t[pos])) pos++;
      var url = t.slice(start, pos), desk = [];
      if (url.charAt(url.length - 1) === ',') {
        url = url.replace(/,+$/, '');
      } else {
        while (pos < n && istLeer(t[pos])) pos++;
        var akt = '', zustand = 0; // 0 in descriptor, 1 in parens, 2 after descriptor
        for (;;) {
          var c = pos < n ? t[pos] : null;
          if (zustand === 0) {
            if (c !== null && istLeer(c)) { if (akt) { desk.push(akt); akt = ''; zustand = 2; } }
            else if (c === ',') { pos++; if (akt) desk.push(akt); break; }
            else if (c === '(') { akt += c; zustand = 1; }
            else if (c === null) { if (akt) desk.push(akt); break; }
            else akt += c;
          } else if (zustand === 1) {
            if (c === ')') { akt += c; zustand = 0; }
            else if (c === null) { desk.push(akt); break; }
            else akt += c;
          } else {
            if (c !== null && istLeer(c)) {}
            else if (c === null) break;
            else { zustand = 0; continue; }
          }
          pos++;
        }
      }
      var fehler = false, breite = null, dichte = null, hoehe = null;
      for (var i = 0; i < desk.length && !fehler; i++) {
        var d = desk[i], letzt = d.charAt(d.length - 1), kopf = d.slice(0, -1);
        if (letzt === 'w' && istGanz(kopf)) {
          if (breite !== null || dichte !== null) fehler = true;
          else { var w = parseInt(kopf, 10); if (w === 0) fehler = true; else breite = w; }
        } else if (letzt === 'x' && istGleit(kopf)) {
          if (breite !== null || dichte !== null || hoehe !== null) fehler = true;
          else { var x = parseFloat(kopf); if (!(x >= 0)) fehler = true; else dichte = x; }
        } else if (letzt === 'h' && istGanz(kopf)) {
          if (hoehe !== null || dichte !== null) fehler = true;
          else { var h = parseInt(kopf, 10); if (h === 0) fehler = true; else hoehe = h; }
        } else fehler = true;
      }
      if (hoehe !== null && breite === null) fehler = true;
      if (!fehler) raus.push({ url: url, w: breite || 0, x: breite ? 0 : (dichte === null ? 1 : dichte) });
    }
    return raus;
  }
  // HTML §4.8.4.3.11 "parse a sizes attribute" on CSS component values
  // (CSS Syntax §4 tokenizer subset, §5 component values): the last
  // component of each entry is a non-negative <length>, a math function
  // (calc/min/max/clamp, negative results clamp to 0, division by zero is
  // invalid) or `auto`; the rest is a <media-condition> (Media Queries 4,
  // three-valued with <general-enclosed> as unknown; media types are not
  // allowed). MEASURED before (WPT sizes/parse-a-sizes-attribute-*, 186
  // cases each): every value was 100vw, so the 1px cases all failed.
  function cssTokens(src) {
    var t = String(src).replace(/\r\n?|\f/g, '\n').replace(/\0/g, '�'), n = t.length, i = 0, out = [];
    function cp(k) { return k < n ? t.charCodeAt(k) : -1; }
    function ws(c) { return c === 32 || c === 9 || c === 10; }
    function dig(c) { return c >= 48 && c <= 57; }
    function hex(c) { return dig(c) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102); }
    function nstart(c) { return (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 95 || c >= 128; }
    function nch(c) { return nstart(c) || dig(c) || c === 45; }
    function esc(k) { return cp(k) === 92 && cp(k + 1) !== 10; }
    function identStart(k) {
      var c = cp(k);
      if (c === 45) { var d = cp(k + 1); return nstart(d) || d === 45 || esc(k + 1); }
      return nstart(c) || esc(k);
    }
    function numStart(k) {
      var c = cp(k);
      if (c === 43 || c === 45) { var d = cp(k + 1); return dig(d) || (d === 46 && dig(cp(k + 2))); }
      if (c === 46) return dig(cp(k + 1));
      return dig(c);
    }
    function escape() {
      var c = cp(i);
      if (c === -1) return '�';
      if (hex(c)) {
        var st = i;
        while (i - st < 6 && hex(cp(i))) i++;
        var v = parseInt(t.slice(st, i), 16);
        if (ws(cp(i))) i++;
        return v === 0 || (v >= 0xD800 && v <= 0xDFFF) || v > 0x10FFFF ? '�' : String.fromCodePoint(v);
      }
      i++;
      return t[i - 1];
    }
    function name() {
      var r = '';
      for (;;) {
        if (nch(cp(i))) r += t[i++];
        else if (esc(i)) { i++; r += escape(); }
        else return r;
      }
    }
    while (i < n) {
      var st = i, c = cp(i);
      if (c === 47 && cp(i + 1) === 42) { var e = t.indexOf('*/', i + 2); i = e < 0 ? n : e + 2; continue; }
      if (ws(c)) { while (ws(cp(i))) i++; out.push({ t: ' ', s: st, e: i }); continue; }
      if (c === 34 || c === 39) {
        i++;
        var bad = false;
        for (;;) {
          var d = cp(i);
          if (d === -1) break;
          if (d === c) { i++; break; }
          if (d === 10) { bad = true; break; }
          i += d === 92 ? 2 : 1;
        }
        out.push({ t: bad ? 'badstr' : 'str', s: st, e: Math.min(i, n) });
        continue;
      }
      if (numStart(i)) {
        if (cp(i) === 43 || cp(i) === 45) i++;
        while (dig(cp(i))) i++;
        if (cp(i) === 46 && dig(cp(i + 1))) { i += 2; while (dig(cp(i))) i++; }
        var x = cp(i), y = cp(i + 1);
        if ((x === 69 || x === 101) && (dig(y) || ((y === 43 || y === 45) && dig(cp(i + 2))))) { i += 2; while (dig(cp(i))) i++; }
        var v = parseFloat(t.slice(st, i));
        if (identStart(i)) { var u = name(); out.push({ t: 'dim', v: v, u: u.toLowerCase(), s: st, e: i }); }
        else if (cp(i) === 37) { i++; out.push({ t: '%', v: v, s: st, e: i }); }
        else out.push({ t: 'num', v: v, s: st, e: i });
        continue;
      }
      if (identStart(i)) {
        var nm = name();
        if (cp(i) === 40) { i++; out.push({ t: 'fn', v: nm.toLowerCase(), s: st, e: i }); }
        else out.push({ t: 'id', v: nm.toLowerCase(), s: st, e: i });
        continue;
      }
      i++;
      var ch = t[st];
      out.push('()[]{},:;'.indexOf(ch) >= 0 ? { t: ch, s: st, e: i } : { t: 'delim', v: ch, s: st, e: i });
    }
    // Component values: blocks and functions own their contents; an
    // unclosed one ends at the end of the input.
    var k = 0, ZU = { '(': ')', '[': ']', '{': '}', fn: ')' };
    function eins(x) {
      if (x.t !== '(' && x.t !== '[' && x.t !== '{' && x.t !== 'fn') return x;
      var blk = { t: x.t === 'fn' ? 'fn' : 'blk', o: x.t, v: x.v, kids: [], s: x.s, e: n }, zu = ZU[x.t];
      while (k < out.length) {
        var y = out[k++];
        if (y.t === zu) { blk.e = y.e; return blk; }
        blk.kids.push(eins(y));
      }
      return blk;
    }
    var cvs = [];
    while (k < out.length) cvs.push(eins(out[k++]));
    return { cvs: cvs, text: t };
  }
  function ohneLeer(l) { return l.filter(function (x) { return x.t !== ' '; }); }
  var EINHEIT = { px: 1, cm: 96 / 2.54, mm: 96 / 25.4, q: 96 / 101.6, in: 96, pc: 16, pt: 4 / 3,
    em: 16, rem: 16, ex: 8, rex: 8, ch: 8, rch: 8, cap: 11.2, rcap: 11.2, ic: 16, ric: 16, lh: 19.2, rlh: 19.2 };
  function laengePx(u, v, win) {
    if (Object.prototype.hasOwnProperty.call(EINHEIT, u)) return v * EINHEIT[u];
    var m = /^[sld]?(vw|vh|vi|vb|vmin|vmax)$/.exec(u);
    if (!m) return undefined;
    var w = win.innerWidth || 800, h = win.innerHeight || 600, b = m[1];
    var basis = b === 'vw' || b === 'vi' ? w : b === 'vh' || b === 'vb' ? h : b === 'vmin' ? Math.min(w, h) : Math.max(w, h);
    return v * basis / 100;
  }
  // A math function's value: { v, len } (len: a length in px, else a
  // number), or undefined when invalid.
  function mathe(fn, win) {
    function wert(x) {
      if (x.t === 'num') return { v: x.v, len: false };
      if (x.t === 'dim') { var px = laengePx(x.u, x.v, win); return px === undefined ? undefined : { v: px, len: true }; }
      if (x.t === 'blk' && x.o === '(') return summe(ohneLeer(x.kids));
      if (x.t === 'fn') return mathe(x, win);
      return undefined;
    }
    function produkt(l, a, b) {
      var r = wert(l[a]);
      for (var k = a + 1; r && k < b; k += 2) {
        var op = l[k], r2 = k + 1 < b ? wert(l[k + 1]) : undefined;
        if (!r2 || op.t !== 'delim' || (op.v !== '*' && op.v !== '/')) return undefined;
        if (op.v === '*') { if (r.len && r2.len) return undefined; r = { v: r.v * r2.v, len: r.len || r2.len }; }
        else { if (r2.len || r2.v === 0) return undefined; r = { v: r.v / r2.v, len: r.len }; }
      }
      return r;
    }
    function summe(l) {
      if (!l.length) return undefined;
      var r, a = 0;
      for (var k = 0; k <= l.length; k++) {
        if (k < l.length && !(l[k].t === 'delim' && (l[k].v === '+' || l[k].v === '-'))) continue;
        if (k === a) return undefined;
        var teil = produkt(l, a, k);
        if (!teil) return undefined;
        if (!r) r = teil;
        else {
          if (r.len !== teil.len) return undefined;
          r = { v: l[a - 1].v === '+' ? r.v + teil.v : r.v - teil.v, len: r.len };
        }
        a = k + 1;
      }
      return r;
    }
    function argumente(l) {
      var args = [], cur = [];
      l.forEach(function (x) { if (x.t === ',') { args.push(cur); cur = []; } else cur.push(x); });
      args.push(cur);
      var r = [];
      for (var k = 0; k < args.length; k++) {
        var a = summe(ohneLeer(args[k]));
        if (!a || (r.length && r[0].len !== a.len)) return undefined;
        r.push(a);
      }
      return r;
    }
    var kids = fn.kids, f = fn.v;
    if (f === 'calc') return summe(ohneLeer(kids));
    if (f === 'min' || f === 'max') {
      var as = argumente(kids);
      if (!as) return undefined;
      return { v: Math[f].apply(Math, as.map(function (x) { return x.v; })), len: as[0].len };
    }
    if (f === 'clamp') {
      var cs = argumente(kids);
      if (!cs || cs.length !== 3) return undefined;
      return { v: Math.max(cs[0].v, Math.min(cs[1].v, cs[2].v)), len: cs[0].len };
    }
    return undefined;
  }
  // A length in px for a media feature value (a number 0, a length, a math
  // function), or undefined.
  function mfLaenge(x, win) {
    if (x.t === 'num') return x.v === 0 ? 0 : undefined;
    if (x.t === 'dim') return laengePx(x.u, x.v, win);
    if (x.t === 'fn') { var m = mathe(x, win); return m && (m.len || m.v === 0) && isFinite(m.v) ? m.v : undefined; }
    return undefined;
  }
  // <any-value>: no bad strings and no unmatched closing token anywhere.
  function beliebigOk(l) {
    for (var k = 0; k < l.length; k++) {
      var x = l[k];
      if (x.t === ')' || x.t === ']' || x.t === '}' || x.t === 'badstr') return false;
      if (x.kids && !beliebigOk(x.kids)) return false;
    }
    return true;
  }
  var MF_LAENGE = { width: 'w', height: 'h', 'device-width': 'sw', 'device-height': 'sh' };
  var MF_SONST = ['orientation', 'aspect-ratio', 'device-aspect-ratio', 'resolution', 'color', 'color-index', 'monochrome',
    'scan', 'grid', 'update', 'overflow-block', 'overflow-inline', 'color-gamut', 'dynamic-range', 'video-dynamic-range',
    'hover', 'any-hover', 'pointer', 'any-pointer', 'prefers-color-scheme', 'prefers-reduced-motion', 'prefers-contrast',
    'prefers-reduced-transparency', 'prefers-reduced-data', 'forced-colors', 'inverted-colors', 'scripting', 'display-mode'];
  function mfMass(art, win) {
    var sc = win.screen || {};
    return art === 'w' ? (win.innerWidth || 800) : art === 'h' ? (win.innerHeight || 600) : art === 'sw' ? (sc.width || 800) : (sc.height || 600);
  }
  function vergleich(a, op, b) {
    return op === '<' ? a < b : op === '<=' ? a <= b : op === '>' ? a > b : op === '>=' ? a >= b : a === b;
  }
  // true/false, null (unknown), undefined (not a media feature).
  function mqFeature(blk, l, win, text) {
    if (!l.length || (l[0].t !== 'id' && l.length === 1)) return undefined;
    function fremd(nm) {
      if (MF_SONST.indexOf(nm) < 0) return null;
      try { return !!win.matchMedia(text.slice(blk.s, blk.e)).matches; } catch (e) { return null; }
    }
    if (l.length === 1) {
      var nm = l[0].v;
      if (MF_LAENGE[nm]) return mfMass(MF_LAENGE[nm], win) !== 0;
      return fremd(nm);
    }
    if (l[0].t === 'id' && l[1].t === ':') {
      if (l.length < 3) return undefined;
      var name = l[0].v, pre = /^(min|max)-/.exec(name), basis = pre ? name.slice(4) : name;
      if (MF_LAENGE[basis]) {
        if (l.length !== 3) return null;
        var v = mfLaenge(l[2], win);
        if (v === undefined) return null;
        var ist = mfMass(MF_LAENGE[basis], win);
        return !pre ? ist === v : pre[1] === 'min' ? ist >= v : ist <= v;
      }
      return fremd(basis);
    }
    // Range syntax: <value> <op> <name> [<op> <value>] or <name> <op> <value>.
    var teile = [], ops = [];
    for (var k = 0; k < l.length; k++) {
      var x = l[k];
      if (x.t === 'delim' && (x.v === '<' || x.v === '>' || x.v === '=')) {
        var op = x.v;
        if (op !== '=' && l[k + 1] && l[k + 1].t === 'delim' && l[k + 1].v === '=' && l[k + 1].s === x.e) { op += '='; k++; }
        ops.push(op);
        teile.push(null);
      } else teile.push(x);
    }
    var werte = [], pos = -1, j = 0, folge = [];
    for (var m = 0; m < teile.length; m++) {
      if (teile[m] === null) { folge.push(ops[j++]); continue; }
      folge.push(teile[m]);
    }
    if (folge.length !== 3 && folge.length !== 5) return undefined;
    for (var q = 0; q < folge.length; q++) {
      if ((q % 2 === 1) !== (typeof folge[q] === 'string')) return undefined;
      if (q % 2 === 0) {
        if (folge[q].t === 'id' && pos < 0 && (MF_LAENGE[folge[q].v] || MF_SONST.indexOf(folge[q].v) >= 0 || !/^(min|max)-/.test(folge[q].v))) { pos = q; werte.push(null); }
        else werte.push(folge[q]);
      }
    }
    if (pos < 0 || (folge.length === 5 && pos !== 2)) return undefined;
    var fname = folge[pos].v;
    if (!MF_LAENGE[fname]) return fremd(fname);
    var mass = mfMass(MF_LAENGE[fname], win), ok = true;
    for (var r = 1; r < folge.length; r += 2) {
      var li = folge[r - 1], re = folge[r + 1];
      var a2 = r - 1 === pos ? mass : mfLaenge(li, win), b2 = r + 1 === pos ? mass : mfLaenge(re, win);
      if (a2 === undefined || b2 === undefined) return null;
      if (!vergleich(a2, folge[r], b2)) ok = false;
    }
    return ok;
  }
  function mqInParens(x, win, text) {
    if (x.t === 'fn') return beliebigOk(x.kids) ? null : undefined;
    if (x.t !== 'blk' || x.o !== '(') return undefined;
    var l = ohneLeer(x.kids);
    var c = mqCondition(l, win, text);
    if (c !== undefined) return c;
    var f = mqFeature(x, l, win, text);
    if (f !== undefined) return f;
    return beliebigOk(x.kids) ? null : undefined;
  }
  function mqCondition(l, win, text) {
    if (!l.length) return undefined;
    if (l[0].t === 'id' && l[0].v === 'not') {
      if (l.length !== 2) return undefined;
      var nr = mqInParens(l[1], win, text);
      return nr === undefined ? undefined : nr === null ? null : !nr;
    }
    var r = mqInParens(l[0], win, text);
    if (r === undefined) return undefined;
    if (l.length === 1) return r;
    var op = l[1].t === 'id' ? l[1].v : '';
    if (op !== 'and' && op !== 'or') return undefined;
    for (var k = 1; k < l.length; k += 2) {
      if (l[k].t !== 'id' || l[k].v !== op || k + 1 >= l.length) return undefined;
      var r2 = mqInParens(l[k + 1], win, text);
      if (r2 === undefined) return undefined;
      if (op === 'and') r = r === false || r2 === false ? false : r === null || r2 === null ? null : true;
      else r = r === true || r2 === true ? true : r === null || r2 === null ? null : false;
    }
    return r;
  }
  // A <media-query-list> (Media Queries 4 §2): an empty list matches; an
  // invalid query is `not all`; unknown counts as false.
  function mqListe(text, win) {
    var tz = cssTokens(text), listen = [[]];
    tz.cvs.forEach(function (x) { if (x.t === ',') listen.push([]); else listen[listen.length - 1].push(x); });
    if (listen.length === 1 && !ohneLeer(listen[0]).length) return true;
    var VERBOTEN = ['not', 'only', 'and', 'or', 'layer'];
    for (var k = 0; k < listen.length; k++) {
      var q = ohneLeer(listen[k]);
      if (!q.length) continue;
      var c = mqCondition(q, win, tz.text);
      if (c !== undefined) { if (c === true) return true; continue; }
      var i = 0, neg = false;
      if (q[0].t === 'id' && (q[0].v === 'not' || q[0].v === 'only')) { neg = q[0].v === 'not'; i = 1; }
      if (!q[i] || q[i].t !== 'id' || VERBOTEN.indexOf(q[i].v) >= 0) continue;
      var r = q[i].v === 'all' || q[i].v === 'screen';
      i++;
      if (i < q.length) {
        if (q[i].t !== 'id' || q[i].v !== 'and' || i + 1 >= q.length) continue;
        var rest = q.slice(i + 1);
        if (rest.some(function (x) { return x.t === 'id' && x.v === 'or'; })) continue;
        var c2 = mqCondition(rest, win, tz.text);
        // Unknown makes the whole query false, `not` included.
        if (c2 === undefined || c2 === null) continue;
        r = r && c2;
      }
      if (neg ? !r : r) return true;
    }
    return false;
  }
  function quellGroesse(quelle, img) {
    var doc = img.ownerDocument, win = (doc && doc.defaultView) || g;
    var attr = quelle.getAttribute('sizes'), vw = win.innerWidth || 800;
    if (attr === null) return vw;
    var tz = cssTokens(attr), listen = [[]];
    tz.cvs.forEach(function (x) { if (x.t === ',') listen.push([]); else listen[listen.length - 1].push(x); });
    var autoErlaubt = /^\s*lazy\s*$/i.test(img.getAttribute('loading') || '') && /^auto(,|$)/i.test(attr);
    for (var k = 0; k < listen.length; k++) {
      var l = listen[k];
      while (l.length && l[l.length - 1].t === ' ') l.pop();
      if (!l.length) continue;
      var x = l.pop(), groesse;
      if (x.t === 'dim') { groesse = laengePx(x.u, x.v, win); if (groesse !== undefined && !(groesse >= 0)) groesse = undefined; }
      else if (x.t === 'num') groesse = x.v === 0 ? 0 : undefined;
      else if (x.t === 'id' && x.v === 'auto') groesse = 'auto';
      else if (x.t === 'fn') {
        var m = mathe(x, win);
        groesse = m && m.len && !isNaN(m.v) ? Math.max(0, m.v) : undefined;
      }
      if (groesse === undefined) continue;
      if (groesse === 'auto' && autoErlaubt) {
        var rects = img.getClientRects ? img.getClientRects() : [];
        if (rects && rects.length) groesse = img.getBoundingClientRect().width;
      }
      while (l.length && l[l.length - 1].t === ' ') l.pop();
      if (l.length) {
        var c = mqCondition(ohneLeer(l), win, tz.text);
        if (c !== true) continue;
      }
      if (groesse !== 'auto') return groesse;
    }
    return vw;
  }
  function gewaehlt(el) {
    var src = el.getAttribute('src');
    var srcsetEl = el;
    var liste = kandidaten(el.getAttribute('srcset'));
    // Die Quellen eines <picture> (erste passende <source>).
    var p = el.parentElement;
    if (p && p.localName === 'picture') {
      for (var k = p.firstElementChild; k && k !== el; k = k.nextElementSibling) {
        if (k.localName !== 'source') continue;
        var media = k.getAttribute('media');
        if (media !== null && !mqListe(media, (el.ownerDocument && el.ownerDocument.defaultView) || g)) continue;
        // The MIME type's essence (parameters ignored; a blank value is no
        // type) must be a supported image type.
        var ty = k.getAttribute('type');
        if (ty !== null && ty.trim() && !/^image\/(png|jpeg|jpg|pjpeg|gif|webp|svg\+xml|bmp|avif|apng|x-icon|vnd\.microsoft\.icon)$/i.test(ty.split(';')[0].trim())) continue;
        var l2 = kandidaten(k.getAttribute('srcset'));
        if (l2.length) { liste = l2; src = null; srcsetEl = k; break; }
      }
    }
    if (src !== null && src !== '' && !liste.some(function (c) { return c.w || c.x === 1; })) liste.push({ url: src, w: 0, x: 1 });
    if (!liste.length) return src ? absolut(src, el) : '';
    var groesse = liste.some(function (c) { return c.w; }) ? quellGroesse(srcsetEl, el) : 0;
    var gesehen = [], set = [];
    liste.forEach(function (c) {
      // A source size of 0 gives an infinite density (equal ones keep the
      // first candidate).
      var d = c.w ? c.w / groesse : c.x;
      if (gesehen.indexOf(d) >= 0) return;
      gesehen.push(d);
      set.push({ url: c.url, d: d });
    });
    var dpr = g.devicePixelRatio || 1, best = null;
    set.forEach(function (c) { if (c.d >= dpr && (!best || best.d < dpr || c.d < best.d)) best = c; });
    if (!best) set.forEach(function (c) { if (!best || c.d > best.d) best = c; });
    return best ? absolut(best.url, el) : '';
  }
  if (!Object.getOwnPropertyDescriptor(P, 'currentSrc')) {
    try { Object.defineProperty(P, 'currentSrc', { get: function () { return gewaehlt(this); }, configurable: true, enumerable: true }); } catch (e) {}
  }
  if (typeof P.decode !== 'function') {
    P.decode = function decode() {
      var el = this;
      return new Promise(function (ja, nein) {
        var fehler = function () { nein(new DOMException('The source image cannot be decoded.', 'EncodingError')); };
        var pruefen = function () {
          if (el.complete) { if (el.naturalWidth > 0) ja(); else fehler(); return true; }
          return false;
        };
        if (!el.getAttribute('src') && !el.getAttribute('srcset')) { setTimeout(fehler, 0); return; }
        setTimeout(function () {
          if (pruefen()) return;
          var fertig = false;
          el.addEventListener('load', function () { if (!fertig) { fertig = true; ja(); } }, { once: true });
          el.addEventListener('error', function () { if (!fertig) { fertig = true; fehler(); } }, { once: true });
        }, 0);
      });
    };
  }
})();

// -- Eingefuegte Inline-Skripte (HTML §4.12.1.1) --
// Release 11.10.2026: ein klassisches Inline-Skript laeuft beim Einfuegen
// SOFORT, im globalen Bereich, mit `document.currentScript`; ein Fehler wird
// gemeldet und haelt das einfuegende Skript nicht an. Der Host meldet es als
// Reaktion 10 (`lebend.rs` `skripte_vormerken`).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var setze = g.__narsilAktuellesSkript;
  var global = eval;
  g.__narsilSkript = function (el) {
    // Hat ein frueher eingefuegtes Skript dieses schon wieder entfernt, laeuft
    // es nicht mehr (HTML §4.12.1.1 „post-connection steps": nur, wenn es
    // noch verbunden ist). GEMESSEN vorher: WPT
    // `later-script-removed-by-earlier-script` — das zweite lief trotzdem.
    try { if (el.isConnected === false) return; } catch (e) {}
    var text = '';
    try {
      for (var k = el.firstChild; k; k = k.nextSibling) if (k.nodeType === 3) text += k.data;
    } catch (e) { text = String(el.textContent || ''); }
    var vorher = null;
    try { vorher = doc.currentScript; } catch (e) {}
    try { if (typeof setze === 'function') setze(el); } catch (e) {}
    try { global(text); } catch (e) {
      try { if (typeof g.reportError === 'function') g.reportError(e); else if (g.console) g.console.error(e); } catch (x) {}
    } finally {
      try { if (typeof setze === 'function') setze(vorher || null); } catch (e) {}
    }
  };
})();

// -- Medien: HTMLMediaElement, TextTrack, WebVTT (HTML §4.8.11-§4.8.13) --
// Release 11.10.2026: ein Modell des Medienelements nach der Spec — der
// Ladeablauf („media element load algorithm", „resource selection
// algorithm" mit stabilem Zustand, Quellzeiger und Warteschritt), die
// Bereitschaft (`readyState` mit `loadedmetadata` … `canplaythrough`),
// Abspielen mit Zeitgeber (`play`/`pause`/Ende/`loop`, `played`), Suchen
// (`seeking`/`timeupdate`/`seeked`), `canPlayType`, `MediaError`,
// `TimeRanges`, dazu Text-Spuren (`addTextTrack`, `<track>` mit WebVTT-
// Parser, `VTTCue`, `TrackEvent`, „time marches on" mit enter/exit/
// cuechange). Ereignisse des Medienelements sind Aufgaben, die `load()`
// verwirft (§4.8.11.5 Schritt 3). Dauer und Bildgroesse kommen aus den
// Dateikoepfen (MP4, WebM, Ogg, WAV, FLAC, MP3). Solange ein Element laedt,
// haelt `__narsilLadeSperre` das `load` des Fensters zurueck. Das Bild
// zeichnet weiter der Host.
// GEMESSEN vorher (WPT Lauf i): 290 von 313 media-elements-Dateien
// gescheitert, 150 Zeitueberschreitungen, `play`/`load`/`addTextTrack is
// not a function`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var HM = g.HTMLMediaElement;
  var M = HM && HM.prototype;
  if (!doc || !M || typeof M.play === 'function') return;
  var zustand = new WeakMap();
  var I = new WeakMap();
  var ET = g.EventTarget && g.EventTarget.prototype;
  var ARTEN = ['subtitles', 'captions', 'descriptions', 'chapters', 'metadata'];

  function def(o, n, v) { try { Object.defineProperty(o, n, { value: v, configurable: true, writable: true, enumerable: false }); } catch (e) {} }
  function eig(o, n, lesen, schreiben) { try { Object.defineProperty(o, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {} }
  function domFehler(text, name) { return typeof g.DOMException === 'function' ? new g.DOMException(text, name) : new Error(text); }
  function melden(e) { try { if (typeof g.reportError === 'function') g.reportError(e); else if (g.console) g.console.error(e); } catch (x) {} }
  function ereignis(typ, mehr) {
    var ev;
    try { ev = g.__narsilUA(g.Event, typ, { bubbles: false, cancelable: false }); } catch (e) { return null; }
    if (mehr) for (var k in mehr) { try { Object.defineProperty(ev, k, { value: mehr[k], configurable: true }); } catch (e) {} }
    return ev;
  }
  function feuern(ziel, typ, mehr) {
    var ev = ereignis(typ, mehr);
    if (!ev) return;
    try { ziel.dispatchEvent(ev); } catch (e) { melden(e); }
  }
  // „Await a stable state": wenn das laufende Skript fertig ist.
  function stabil(f) { Promise.resolve().then(f); }
  function aufgabe(f) { setTimeout(f, 0); }
  // Eine Aufgabe der Medien-Ereignisquelle: `load()` verwirft sie.
  function mAufgabe(el, f) {
    var s = z(el), e = s.epoche;
    setTimeout(function () { if (s.epoche === e) f(); }, 0);
  }
  function absolut(u) {
    var basis = doc.baseURI || (g.location && g.location.href);
    try { return new g.URL(String(u), basis).href; } catch (e) { return null; }
  }
  function ziffern(v) { return Object.prototype.toString.call(v) === '[object Number]' || typeof v === 'number'; }
  function doppel(v, wo) {
    var n = Number(v);
    if (!isFinite(n)) throw new TypeError("Failed to set the '" + wo + "' property: The provided double value is non-finite.");
    return n;
  }

  // Die Ladesperre: ein Element, das laedt, haelt `window.load` zurueck
  // (HTML §4.8.11.5 „delaying-the-load-event flag"); der Host liest den
  // Zaehler (`sitzung.rs` `lade_sperre`).
  function sperre(s, an) {
    if (!!s.sperrt === !!an) return;
    s.sperrt = !!an;
    g.__narsilLadeSperre = Math.max(0, (g.__narsilLadeSperre | 0) + (an ? 1 : -1));
  }

  // -- Eigene Ereignisziele (TextTrack, TextTrackList, VTTCue) --
  // Die Lauscher stehen als `_zl` am Objekt (bei Listen am Ziel des
  // Proxys — Proxy und Ziel sehen dieselben Lauscher).
  function ziel(o) {
    var z0 = o._zl;
    if (!z0) { z0 = { l: {}, h: {}, w: {} }; def(o, '_zl', z0); }
    return z0;
  }
  function zielMixin(proto, handler) {
    def(proto, 'addEventListener', function addEventListener(typ, f, opt) {
      if (f === null || f === undefined) return;
      var l = ziel(this).l, t = String(typ), liste = l[t] || (l[t] = []);
      for (var i = 0; i < liste.length; i++) if (liste[i].f === f) return;
      liste.push({ f: f, einmal: !!(opt && typeof opt === 'object' && opt.once) });
    });
    def(proto, 'removeEventListener', function removeEventListener(typ, f) {
      var liste = ziel(this).l[String(typ)] || [];
      for (var i = 0; i < liste.length; i++) if (liste[i].f === f) { liste[i].weg = true; liste.splice(i, 1); return; }
    });
    def(proto, 'dispatchEvent', function dispatchEvent(ev) {
      var selbst = this;
      try { Object.defineProperty(ev, 'target', { value: selbst, configurable: true }); } catch (e) {}
      try { Object.defineProperty(ev, 'currentTarget', { value: selbst, configurable: true }); } catch (e) {}
      try { Object.defineProperty(ev, 'srcElement', { value: selbst, configurable: true }); } catch (e) {}
      try { Object.defineProperty(ev, 'eventPhase', { value: 2, configurable: true }); } catch (e) {}
      var liste = (ziel(this).l[ev.type] || []).slice();
      var setzeEreignis = typeof g.__narsilEreignis === 'function' ? g.__narsilEreignis : function () {};
      var altesEreignis = setzeEreignis(ev);
      for (var i = 0; i < liste.length; i++) {
        var e = liste[i];
        if (e.weg) continue;
        if (e.einmal) this.removeEventListener(ev.type, e.f);
        try {
          if (typeof e.f === 'function') e.f.call(selbst, ev);
          else if (e.f && typeof e.f.handleEvent === 'function') e.f.handleEvent(ev);
        } catch (x) { melden(x); }
      }
      setzeEreignis(altesEreignis);
      try { Object.defineProperty(ev, 'currentTarget', { value: null, configurable: true }); } catch (e) {}
      return !ev.defaultPrevented;
    });
    handler.forEach(function (n) {
      eig(proto, 'on' + n, function () { var h = ziel(this).h[n]; return h === undefined ? null : h; }, function (v) {
        var zz = ziel(this);
        var neu = typeof v === 'function' || (typeof v === 'object' && v !== null) ? v : null;
        zz.h[n] = neu;
        if (neu && !zz.w[n]) {
          var selbst = this;
          zz.w[n] = function (ev) {
            var h = ziel(selbst).h[n];
            if (typeof h === 'function' && h.call(selbst, ev) === false) ev.preventDefault();
          };
          this.addEventListener(n, zz.w[n]);
        } else if (!neu && zz.w[n]) {
          this.removeEventListener(n, zz.w[n]);
          zz.w[n] = null;
        }
      });
    });
  }

  // Lebende Listen (TextTrackList, TextTrackCueList, TimeRanges): Index-
  // Zugriff liest die aktuelle Liste, Index-Schreiben ist verboten (Web IDL
  // „indexed property getter" ohne Setter).
  function lebendig(z0) {
    if (typeof g.Proxy !== 'function') return z0;
    var index = function (k) { return typeof k === 'string' && /^(0|[1-9][0-9]*)$/.test(k); };
    return new g.Proxy(z0, {
      get: function (t, k) { if (index(k)) return t._l[+k]; return t[k]; },
      set: function (t, k, v) { if (index(k)) return false; t[k] = v; return true; },
      has: function (t, k) { if (index(k)) return +k < t._l.length; return k in t; },
      defineProperty: function (t, k, d) { if (index(k)) return false; Object.defineProperty(t, k, d); return true; },
      deleteProperty: function (t, k) { if (index(k)) return !(+k < t._l.length); return delete t[k]; },
      getOwnPropertyDescriptor: function (t, k) {
        if (index(k)) return +k < t._l.length ? { value: t._l[+k], writable: false, enumerable: true, configurable: true } : undefined;
        return Object.getOwnPropertyDescriptor(t, k);
      },
      ownKeys: function (t) {
        var k = [];
        for (var i = 0; i < t._l.length; i++) k.push(String(i));
        return k.concat(Object.getOwnPropertyNames(t).filter(function (n) { return n.charAt(0) !== '_'; }));
      }
    });
  }
  function innen(o, n, v) { def(o, n, v); }
  function unerlaubt(name) { return function () { throw new TypeError("Failed to construct '" + name + "': Illegal constructor"); }; }

  // -- TimeRanges, MediaError --
  var TimeRanges = unerlaubt('TimeRanges');
  function zeitraeume(teile) { var r = Object.create(TimeRanges.prototype); innen(r, '_t', teile || []); return r; }
  eig(TimeRanges.prototype, 'length', function () { return this._t.length; });
  def(TimeRanges.prototype, 'start', function start(i) {
    i = Number(i) >>> 0;
    if (i >= this._t.length) throw domFehler('The index provided (' + i + ') is greater than or equal to the maximum bound (' + this._t.length + ').', 'IndexSizeError');
    return this._t[i][0];
  });
  def(TimeRanges.prototype, 'end', function end(i) {
    i = Number(i) >>> 0;
    if (i >= this._t.length) throw domFehler('The index provided (' + i + ') is greater than or equal to the maximum bound (' + this._t.length + ').', 'IndexSizeError');
    return this._t[i][1];
  });
  g.TimeRanges = TimeRanges;
  var MediaError = unerlaubt('MediaError');
  function medienFehler(code, text) { var f = Object.create(MediaError.prototype); innen(f, '_c', code); innen(f, '_m', text || ''); return f; }
  eig(MediaError.prototype, 'code', function () { return this._c; });
  eig(MediaError.prototype, 'message', function () { return this._m; });
  [['MEDIA_ERR_ABORTED', 1], ['MEDIA_ERR_NETWORK', 2], ['MEDIA_ERR_DECODE', 3], ['MEDIA_ERR_SRC_NOT_SUPPORTED', 4]].forEach(function (p) {
    try { Object.defineProperty(MediaError, p[0], { value: p[1], enumerable: true }); Object.defineProperty(MediaError.prototype, p[0], { value: p[1], enumerable: true }); } catch (e) {}
  });
  g.MediaError = MediaError;
  [['NETWORK_EMPTY', 0], ['NETWORK_IDLE', 1], ['NETWORK_LOADING', 2], ['NETWORK_NO_SOURCE', 3],
    ['HAVE_NOTHING', 0], ['HAVE_METADATA', 1], ['HAVE_CURRENT_DATA', 2], ['HAVE_FUTURE_DATA', 3], ['HAVE_ENOUGH_DATA', 4]].forEach(function (p) {
    try { Object.defineProperty(M, p[0], { value: p[1], enumerable: true }); Object.defineProperty(HM, p[0], { value: p[1], enumerable: true }); } catch (e) {}
  });

  // -- Dateikoepfe: Dauer und Bildgroesse --
  function kopf(bytes) {
    var b = new Uint8Array(bytes), n = b.length;
    var dv = new DataView(bytes);
    var u32 = function (o) { return o + 4 <= n ? dv.getUint32(o) : 0; };
    var text = function (o, l) { var s = ''; for (var i = 0; i < l && o + i < n; i++) s += String.fromCharCode(b[o + i]); return s; };
    var ergebnis = { dauer: NaN, breite: 0, hoehe: 0, video: false };
    var t4 = text(4, 4), t0 = text(0, 4);
    if (t4 === 'ftyp' || t4 === 'moov' || t4 === 'mdat' || t4 === 'free' || t4 === 'wide' || t4 === 'skip' || t4 === 'styp') {
      // ISO-BMFF: Kaesten rekursiv durch moov/trak/mvex.
      var skala = 0, fragment = 0;
      var lauf = function (anf, ende) {
        var o = anf;
        while (o + 8 <= ende) {
          var groesse = u32(o), art = text(o + 4, 4), kopfl = 8;
          if (groesse === 1) { groesse = u32(o + 12) + u32(o + 8) * 4294967296; kopfl = 16; }
          if (groesse === 0) groesse = ende - o;
          if (groesse < 8 || o + groesse > ende) groesse = ende - o;
          if (art === 'moov' || art === 'trak' || art === 'mdia' || art === 'minf' || art === 'mvex') lauf(o + kopfl, o + groesse);
          else if (art === 'mvhd') {
            var v = b[o + kopfl];
            skala = v === 1 ? u32(o + kopfl + 20) : u32(o + kopfl + 12);
            var dauer = v === 1 ? u32(o + kopfl + 24) * 4294967296 + u32(o + kopfl + 28) : u32(o + kopfl + 16);
            if (skala > 0) ergebnis.dauer = dauer / skala;
          } else if (art === 'mehd') {
            var v2 = b[o + kopfl];
            fragment = v2 === 1 ? u32(o + kopfl + 4) * 4294967296 + u32(o + kopfl + 8) : u32(o + kopfl + 4);
          } else if (art === 'tkhd') {
            var w = u32(o + groesse - 8) / 65536, h = u32(o + groesse - 4) / 65536;
            if (w > 0 && h > 0) { ergebnis.breite = Math.round(w); ergebnis.hoehe = Math.round(h); ergebnis.video = true; }
          }
          o += groesse;
        }
      };
      lauf(0, n);
      if (!(ergebnis.dauer > 0) && fragment > 0 && skala > 0) ergebnis.dauer = fragment / skala;
      return ergebnis;
    }
    if (b[0] === 0x1A && b[1] === 0x45 && b[2] === 0xDF && b[3] === 0xA3) {
      // Matroska/WebM: EBML-Elemente.
      var vint = function (o, id) {
        var erstes = b[o]; if (erstes === undefined) return null;
        var l = 1, maske = 0x80;
        while (l <= 8 && !(erstes & maske)) { l++; maske >>= 1; }
        if (l > 8) return null;
        var wert = id ? erstes : erstes & (maske - 1);
        for (var i = 1; i < l; i++) wert = wert * 256 + b[o + i];
        var alleEins = !id && wert === Math.pow(2, 7 * l) - 1;
        return { wert: alleEins ? -1 : wert, laenge: l };
      };
      var skala2 = 1000000, dauer2 = NaN;
      var zahl = function (o, l) { var v = 0; for (var i = 0; i < l; i++) v = v * 256 + b[o + i]; return v; };
      var webm = function (anf, ende) {
        var o = anf;
        while (o < ende) {
          var id = vint(o, true); if (!id) return;
          var gr = vint(o + id.laenge, false); if (!gr) return;
          var daten = o + id.laenge + gr.laenge;
          var groesse2 = gr.wert < 0 ? ende - daten : gr.wert;
          var X = id.wert;
          if (X === 0x18538067 || X === 0x1549A966 || X === 0x1654AE6B || X === 0xAE || X === 0xE0) webm(daten, Math.min(ende, daten + groesse2));
          else if (X === 0x2AD7B1) skala2 = zahl(daten, groesse2);
          else if (X === 0x4489) dauer2 = groesse2 === 4 ? dv.getFloat32(daten) : dv.getFloat64(daten);
          else if (X === 0xB0) { ergebnis.breite = zahl(daten, groesse2); ergebnis.video = true; }
          else if (X === 0xBA) ergebnis.hoehe = zahl(daten, groesse2);
          if (X === 0x1F43B675) return;
          o = daten + groesse2;
        }
      };
      webm(0, n);
      if (isFinite(dauer2)) ergebnis.dauer = dauer2 * skala2 / 1e9;
      else ergebnis.dauer = Infinity;
      return ergebnis;
    }
    if (t0 === 'OggS') {
      var rate = 0, vorlauf = 0;
      var ident = text(28, 8);
      if (ident.slice(1, 7) === 'vorbis') rate = dv.getUint32(28 + 12, true);
      else if (ident === 'OpusHead') { rate = 48000; vorlauf = dv.getUint16(28 + 10, true); }
      else if (ident.slice(1, 7) === 'theora') ergebnis.video = true;
      var letzte = -1;
      for (var p = n - 4; p >= 0; p--) { if (b[p] === 0x4F && b[p + 1] === 0x67 && b[p + 2] === 0x67 && b[p + 3] === 0x53) { letzte = p; break; } }
      if (letzte >= 0 && rate > 0) {
        var granule = dv.getUint32(letzte + 6, true) + dv.getUint32(letzte + 10, true) * 4294967296;
        ergebnis.dauer = Math.max(0, granule - vorlauf) / rate;
      }
      return ergebnis;
    }
    if (t0 === 'RIFF' && text(8, 4) === 'WAVE') {
      var o3 = 12, byteRate = 0;
      while (o3 + 8 <= n) {
        var art3 = text(o3, 4), l3 = dv.getUint32(o3 + 4, true);
        if (art3 === 'fmt ') byteRate = dv.getUint32(o3 + 16, true);
        if (art3 === 'data') { if (byteRate > 0) ergebnis.dauer = Math.min(l3, n - o3 - 8) / byteRate; break; }
        o3 += 8 + l3 + (l3 & 1);
      }
      return ergebnis;
    }
    if (t0 === 'fLaC') {
      var abtast = (b[18] << 12) | (b[19] << 4) | (b[20] >> 4);
      var proben = (b[21] & 0x0F) * 4294967296 + dv.getUint32(22);
      if (abtast > 0) ergebnis.dauer = proben / abtast;
      return ergebnis;
    }
    // MP3: ID3 ueberspringen, erster Rahmenkopf, Xing/Info oder Bitrate.
    var start = 0;
    if (text(0, 3) === 'ID3') start = 10 + ((b[6] & 127) << 21 | (b[7] & 127) << 14 | (b[8] & 127) << 7 | (b[9] & 127));
    for (var q = start; q + 4 < n && q < start + 65536; q++) {
      if (b[q] === 0xFF && (b[q + 1] & 0xE0) === 0xE0) {
        var ver = (b[q + 1] >> 3) & 3, schicht = (b[q + 1] >> 1) & 3, bi = b[q + 2] >> 4, ri = (b[q + 2] >> 2) & 3;
        var raten = ver === 3 ? [44100, 48000, 32000] : ver === 2 ? [22050, 24000, 16000] : [11025, 12000, 8000];
        var bitraten = ver === 3 ? [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320] : [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
        var r = raten[ri], br = bitraten[bi] * 1000;
        if (!r || !br || schicht !== 1) continue;
        var proRahmen = ver === 3 ? 1152 : 576;
        var xing = q + 4 + (ver === 3 ? ((b[q + 3] >> 6) === 3 ? 17 : 32) : ((b[q + 3] >> 6) === 3 ? 9 : 17));
        var xt = text(xing, 4);
        if ((xt === 'Xing' || xt === 'Info') && (dv.getUint32(xing + 4) & 1)) ergebnis.dauer = dv.getUint32(xing + 8) * proRahmen / r;
        else ergebnis.dauer = (n - q) * 8 / br;
        return ergebnis;
      }
    }
    return null;
  }

  var TYPEN = {
    'video/mp4': 1, 'audio/mp4': 1, 'video/webm': 1, 'audio/webm': 1, 'audio/ogg': 1, 'video/ogg': 1, 'application/ogg': 1,
    'audio/mpeg': 1, 'audio/mp3': 1, 'audio/wav': 1, 'audio/wave': 1, 'audio/x-wav': 1, 'audio/flac': 1, 'audio/x-flac': 1
  };
  var CODECS = /^(avc1(\.[0-9a-f]+)?|mp4a(\.[0-9a-f.]+)?|opus|vorbis|flac|mp3|theora|pcm|1|vp8(\.0)?|vp9(\.0)?|vp09\.00\.[0-9]{2}\.08(\.[0-9.]+)?)$/i;
  function typPruefen(typ) {
    var t = String(typ).trim().toLowerCase();
    if (!t) return '';
    var haupt = t.split(';')[0].trim();
    if (!TYPEN[haupt]) return '';
    var codecs = /codecs\s*=\s*"?([^"]*)"?/.exec(t);
    if (!codecs) return 'maybe';
    var liste = codecs[1].split(',').map(function (c) { return c.trim(); }).filter(Boolean);
    if (!liste.length) return 'maybe';
    for (var i = 0; i < liste.length; i++) if (!CODECS.test(liste[i])) return '';
    return 'probably';
  }
  function medienPasst(m) {
    try { return typeof g.matchMedia !== 'function' || g.matchMedia(m).matches; } catch (e) { return true; }
  }

  // -- Zustand je Element --
  // Nur HTML-Elemente zaehlen (`createElementNS('foo', 'source')` nicht).
  var XHTML = 'http://www.w3.org/1999/xhtml';
  function istHtml(k, n) { return !!k && k.localName === n && k.namespaceURI === XHTML; }
  function istMedium(el) { return istHtml(el, 'audio') || istHtml(el, 'video'); }
  function quellen(el) { return Array.prototype.filter.call(el.children, function (k) { return istHtml(k, 'source'); }); }
  // `ohneInit`: der Aufrufer startet den Ladeablauf selbst.
  function z(el, ohneInit) {
    var s = zustand.get(el);
    if (s) return s;
    s = {
      netz: 0, bereit: 0, pausiert: true, aktuell: 0, offiziell: 0, vorgabeStart: 0, dauer: NaN, fehler: null, quelle: '',
      suche: false, suchNr: 0, rate: 1, vorgabeRate: 1, lautstaerke: 1, stumm: null, breite: 0, hoehe: 0, ladung: 0,
      epoche: 0, uhr: null, letzte: 0, letzteMeldung: 0, versprechen: [], gespielt: [], spuren: null, sperrt: false,
      zeiger: null, zeigerVor: null, zeigerNach: null, warten: false, wartenNr: 0, autoplay: true, poster: true,
      objekt: null, letzteZeit: 0, aktiv: [], spurWechsel: false, zeitPlan: false, init: false
    };
    zustand.set(el, s);
    // Vom Parser gebaute Elemente: ihre `<track>`-Kinder gehoeren schon zur
    // Spurliste, und ihr `src`/ihre `<source>` waren beim Einfuegen da — der
    // Ladeablauf begann dort (§4.8.11.5).
    if (istMedium(el)) kinderAbgleich(el);
    if (!ohneInit && el.isConnected && istMedium(el)) {
      s.init = true;
      if (el.getAttributeNS(null, 'src') !== null || quellen(el).length) laden(el);
    }
    return s;
  }
  function beendet(el) {
    var s = z(el);
    return s.bereit >= 1 && isFinite(s.dauer) && s.aktuell >= s.dauer && s.rate >= 0 && el.getAttribute('loop') === null;
  }

  // Der Ladealgorithmus (§4.8.11.5 „media element load algorithm").
  function laden(el) {
    var s = z(el, true);
    if (s.mse) mseAbhaengen(s);
    s.epoche++;
    s.ladung++;
    stoppen(el);
    var ausstehend = s.versprechen.splice(0);
    if (s.netz === 2 || s.netz === 1) mAufgabe(el, function () { feuern(el, 'abort'); });
    if (s.netz !== 0) {
      mAufgabe(el, function () { feuern(el, 'emptied'); });
      s.bereit = 0;
      s.pausiert = true;
      s.suche = false;
      var vorher = s.offiziell;
      s.aktuell = 0; s.offiziell = 0;
      if (vorher !== 0) mAufgabe(el, function () { feuern(el, 'timeupdate'); });
      s.dauer = NaN; s.breite = 0; s.hoehe = 0;
      if (s.spuren) {
        s.aktiv = [];
        s.spuren._l.forEach(function (t) { I.get(t).aktiv.splice(0); });
      }
      s.letzteZeit = 0;
    }
    s.rate = s.vorgabeRate;
    s.fehler = null;
    s.autoplay = true;
    auswahl(el);
    ausstehend.forEach(function (p) { p.nein(domFehler('The play() request was interrupted by a new load request.', 'AbortError')); });
  }

  // Release 11.10.2026: `loading=lazy` (HTML-Vorschlag „lazy loading media
  // elements"): solange das Element nicht gerendert ist oder ausserhalb der
  // Sicht liegt, beginnt die Quellenwahl nicht (kein `loadstart`, keine
  // Ladesperre fuer `load`). GEMESSEN vorher (WPT the-video/audio-element
  // *-loading-*): auch Lazy-Medien unter der Sicht luden sofort.
  var aufgeschoben = [], pruefLaeuft = false;
  function istLazy(el) { return String(el.getAttribute('loading') || '').toLowerCase() === 'lazy'; }
  function sichtbar(el) {
    if (!el.isConnected) return false;
    if (el.localName === 'audio' && !el.hasAttribute('controls')) return false;
    var m = null;
    try { m = typeof g.__narsilMass === 'function' ? g.__narsilMass(el) : null; } catch (e) { m = null; }
    if (!m || m[5] === true) return false;
    var r = el.getBoundingClientRect();
    if (!(r.width > 0 || r.height > 0)) return false;
    var h = g.innerHeight || 600, w = g.innerWidth || 800;
    return r.bottom >= 0 && r.top <= h && r.right >= 0 && r.left <= w;
  }
  function aufschieben(el) {
    if (aufgeschoben.indexOf(el) < 0) aufgeschoben.push(el);
    if (!pruefLaeuft) { pruefLaeuft = true; g.setTimeout(lazyPruefen, 250); }
  }
  function lazyPruefen() {
    pruefLaeuft = false;
    var l = aufgeschoben.slice();
    aufgeschoben = [];
    l.forEach(function (el) { if (istLazy(el) && !sichtbar(el)) aufgeschoben.push(el); else auswahl(el); });
    if (aufgeschoben.length) { pruefLaeuft = true; g.setTimeout(lazyPruefen, 250); }
  }
  // Die Quellenwahl (§4.8.11.5 „resource selection algorithm").
  function auswahl(el) {
    var s = z(el, true);
    var nr = ++s.ladung;
    s.netz = 3;
    s.poster = true;
    s.warten = false;
    sperre(s, true);
    stabil(function () {
      if (nr !== s.ladung) return;
      if (istLazy(el) && !sichtbar(el)) { s.netz = 0; sperre(s, false); aufschieben(el); return; }
      var src = el.getAttributeNS(null, 'src');
      if (!s.objekt && src === null && !quellen(el).length) { s.netz = 0; sperre(s, false); return; }
      s.netz = 2;
      mAufgabe(el, function () { feuern(el, 'loadstart'); });
      if (s.objekt) return fehlerAttribut(el, nr);
      if (src !== null) {
        var url = src === '' ? null : absolut(src);
        if (url === null) return fehlerAttribut(el, nr);
        s.quelle = url;
        return holen(el, nr, url, null);
      }
      s.zeiger = null; s.zeigerVor = null; s.zeigerNach = null;
      naechsteQuelle(el, nr);
    });
  }
  // „Failed with attribute" -> „dedicated media source failure steps".
  function fehlerAttribut(el, nr) {
    var s = z(el);
    if (nr !== s.ladung) return;
    var ausstehend = s.versprechen.splice(0);
    mAufgabe(el, function () {
      s.fehler = medienFehler(4, 'MEDIA_ELEMENT_ERROR: Format error');
      s.netz = 3;
      s.poster = true;
      feuern(el, 'error');
      ausstehend.forEach(function (p) { p.nein(domFehler('The element has no supported sources.', 'NotSupportedError')); });
      sperre(s, false);
    });
  }
  function nachZeiger(el) {
    var s = z(el), start;
    if (s.zeiger === null) start = el.firstChild;
    else if (s.zeiger.parentNode === el) start = s.zeiger.nextSibling;
    else if (s.zeigerVor && s.zeigerVor.parentNode === el) start = s.zeigerVor.nextSibling;
    else if (s.zeigerNach && s.zeigerNach.parentNode === el) start = s.zeigerNach;
    else start = null;
    for (var k = start; k; k = k.nextSibling) if (istHtml(k, 'source')) return k;
    return null;
  }
  function naechsteQuelle(el, nr) {
    var s = z(el);
    if (nr !== s.ladung) return;
    var q = nachZeiger(el);
    if (!q) {
      // „Waiting": bis eine neue `<source>` hinter dem Zeiger kommt.
      s.netz = 3;
      s.poster = true;
      s.warten = true;
      s.wartenNr = nr;
      mAufgabe(el, function () { sperre(s, false); });
      return;
    }
    s.zeiger = q; s.zeigerVor = q.previousSibling; s.zeigerNach = q.nextSibling;
    var u = q.getAttributeNS(null, 'src');
    var url = u === null || u === '' ? null : absolut(u);
    var typ = q.getAttribute('type'), media = q.getAttribute('media');
    if (url === null || (typ !== null && !typPruefen(typ)) || (media !== null && !medienPasst(media))) return fehlerQuelle(el, nr, q);
    s.quelle = url;
    holen(el, nr, url, q);
  }
  function fehlerQuelle(el, nr, q) {
    var s = z(el);
    if (nr !== s.ladung) return;
    mAufgabe(el, function () { feuern(q, 'error'); });
    stabil(function () { naechsteQuelle(el, nr); });
  }
  function holen(el, nr, url, q) {
    var s = z(el);
    var fehlschlag = function () {
      if (nr !== s.ladung) return;
      if (q) fehlerQuelle(el, nr, q); else fehlerAttribut(el, nr);
    };
    if (/^blob:/i.test(url)) {
      var m = mseAnhaengen(el, nr, url);
      if (m === true) return;
      if (m === false) return aufgabe(fehlschlag);
    }
    if (typeof g.fetch !== 'function') return aufgabe(fehlschlag);
    g.fetch(url).then(function (r) {
      if (!r.ok) throw new Error('Status ' + r.status);
      return r.arrayBuffer();
    }).then(function (daten) {
      if (nr !== s.ladung) return;
      var k = null;
      try { k = kopf(daten); } catch (e) { k = null; }
      if (!k || !(k.dauer >= 0)) return fehlschlag();
      metadaten(el, nr, k);
    }, fehlschlag);
  }
  function fragmentStart(u) {
    var m = /#t=(?:npt:)?([0-9.]+)/.exec(u || '');
    return m ? parseFloat(m[1]) || 0 : 0;
  }
  // Bereitschaft (§4.8.11.7): Metadaten, dann (wenn die Spuren bereit sind)
  // aktuelle, kuenftige und genug Daten.
  function metadaten(el, nr, k) {
    var s = z(el);
    var video = el.localName === 'video';
    s.dauer = k.dauer;
    if (video) { s.breite = k.breite; s.hoehe = k.hoehe; }
    s.aktuell = 0; s.offiziell = 0;
    s.bereit = 1;
    mAufgabe(el, function () { feuern(el, 'durationchange'); });
    if (video && (k.breite || k.hoehe)) mAufgabe(el, function () { feuern(el, 'resize'); });
    mAufgabe(el, function () { feuern(el, 'loadedmetadata'); });
    var start = s.vorgabeStart > 0 ? s.vorgabeStart : fragmentStart(s.quelle);
    s.vorgabeStart = 0;
    if (start > 0) suchen(el, start);
    weiterBereit(el, nr);
  }
  function spurenWarten(el) {
    var s = zustand.get(el);
    if (!s || !s.spuren) return false;
    return s.spuren._l.some(function (t) {
      var x = I.get(t);
      return x.spurEl && (x.wahl || (x.modus !== 'disabled' && (x.bereit === 1 || (x.laeuft && x.bereit === 0))));
    });
  }
  function weiterBereit(el, nr) {
    var s = z(el);
    if (nr !== s.ladung || s.bereit !== 1) return;
    if (spurenWarten(el)) { s.spurNr = nr; return; }
    s.spurNr = 0;
    mAufgabe(el, function () {
      if (nr !== s.ladung) return;
      s.bereit = 2;
      feuern(el, 'loadeddata');
      sperre(s, false);
      s.bereit = 4;
      feuern(el, 'canplay');
      if (!s.pausiert) spielenMelden(el);
      mAufgabe(el, function () {
        feuern(el, 'canplaythrough');
        // Autoplay nach HAVE_ENOUGH_DATA (so ordnen es Chrome und Firefox:
        // canplay, canplaythrough, play, playing).
        if (s.pausiert && s.autoplay && el.getAttribute('autoplay') !== null) {
          s.pausiert = false;
          s.poster = false;
          mAufgabe(el, function () { feuern(el, 'play'); });
          spielenMelden(el);
          starten(el);
        }
        feuern(el, 'progress');
        s.netz = 1;
        feuern(el, 'suspend');
      });
      starten(el);
    });
  }
  // „Notify about playing".
  function spielenMelden(el) {
    var s = z(el), ausstehend = s.versprechen.splice(0);
    mAufgabe(el, function () {
      feuern(el, 'playing');
      ausstehend.forEach(function (p) { p.ja(); });
    });
  }

  // -- Abspielen --
  function stoppen(el) { var s = z(el); if (s.uhr) { clearInterval(s.uhr); s.uhr = null; } }
  function gespieltDazu(s, a, b) {
    if (!(b > a)) return;
    var l = s.gespielt.concat([[a, b]]).sort(function (x, y) { return x[0] - y[0]; }), raus = [];
    l.forEach(function (r) {
      var letzte = raus[raus.length - 1];
      if (letzte && r[0] <= letzte[1] + 1e-6) letzte[1] = Math.max(letzte[1], r[1]); else raus.push([r[0], r[1]]);
    });
    s.gespielt = raus;
  }
  function starten(el) {
    var s = z(el);
    if (s.uhr || s.pausiert) return;
    s.letzte = Date.now();
    s.letzteMeldung = s.letzte;
    s.uhr = setInterval(function () {
      var jetzt = Date.now(), dt = (jetzt - s.letzte) / 1000;
      s.letzte = jetzt;
      if (s.pausiert) { stoppen(el); return; }
      if (s.bereit < 3 || s.suche || beendet(el)) return;
      var alt = s.aktuell, neu = alt + dt * s.rate;
      var mseHalt = false;
      if (s.mse && s.rate >= 0) {
        var grenze = mseGrenze(s, alt);
        var bisEnde = s.mse.ended() && isFinite(s.dauer) && grenze >= s.dauer - 1e-6;
        if (!bisEnde && neu >= grenze) { neu = grenze; mseHalt = true; }
      }
      if (!mseHalt && isFinite(s.dauer) && s.rate >= 0 && neu >= s.dauer) {
        neu = s.dauer;
        gespieltDazu(s, alt, neu);
        s.aktuell = neu; s.offiziell = neu;
        if (el.getAttribute('loop') !== null) { suchen(el, 0); return; }
        zeitLaeuft(el, true);
        stoppen(el);
        mAufgabe(el, function () {
          feuern(el, 'timeupdate');
          if (beendet(el) && !s.pausiert) {
            s.pausiert = true;
            feuern(el, 'pause');
            s.versprechen.splice(0).forEach(function (p) { p.nein(domFehler('The play() request was interrupted because the media ended.', 'AbortError')); });
          }
          feuern(el, 'ended');
        });
        return;
      }
      if (neu < 0) neu = 0;
      if (s.rate >= 0) gespieltDazu(s, alt, neu); else gespieltDazu(s, neu, alt);
      s.aktuell = neu; s.offiziell = neu;
      zeitLaeuft(el, true);
      if (mseHalt) mseLage(el);
      if (jetzt - s.letzteMeldung >= 250) { s.letzteMeldung = jetzt; feuern(el, 'timeupdate'); }
    }, 30);
  }
  // Suchen (§4.8.11.9 „seek").
  function suchen(el, t) {
    var s = z(el);
    s.poster = false;
    if (s.bereit === 0) return;
    var nr = ++s.suchNr;
    s.suche = true;
    if (isFinite(s.dauer) && t > s.dauer) t = s.dauer;
    if (!(t > 0)) t = 0;
    s.aktuell = t; s.offiziell = t;
    mAufgabe(el, function () { feuern(el, 'seeking'); });
    aufgabe(function () {
      if (nr !== s.suchNr) return;
      if (s.mse && !mseGepuffert(s, s.aktuell)) { s.mseSuche = nr; mseBereit(el, 1); return; }
      s.suche = false;
      s.letzte = Date.now();
      zeitLaeuft(el, false);
      mAufgabe(el, function () { feuern(el, 'timeupdate'); });
      mAufgabe(el, function () { feuern(el, 'seeked'); });
      if (s.mse) mseLage(el);
    });
  }

  // -- Media Source Extensions (2026-10-15) --
  // The blob URL of a MediaSource (`URL.createObjectURL(mediaSource)`)
  // attaches the MediaSource (MSE §2.4.1) instead of fetching. The
  // MediaSource side is web_media.js in narsil-jsvm, loaded with the first
  // use of `MediaSource`; these are the element steps: the duration,
  // HAVE_METADATA after the first initialization segment, the readyState
  // from the buffered ranges around the current position, `waiting` at the
  // end of the buffered data, seeks into unbuffered time that wait for
  // appends, `ended` after endOfStream(), and detaching on load().
  function mseAnhaengen(el, nr, url) {
    var obj = typeof g.__narsilBlobFuer === 'function' ? g.__narsilBlobFuer(url) : null;
    if (!obj || typeof g.__narsilMseAttach !== 'function') return null;
    var s = z(el), video = el.localName === 'video', a = null;
    var gilt = function () { return nr === s.ladung && s.mse === a; };
    var api = {
      ranges: zeitraeume,
      currentTime: function () { return s.aktuell; },
      hasError: function () { return !!s.fehler; },
      duration: function (d) {
        if (!gilt() || d === s.dauer || (d !== d && s.dauer !== s.dauer)) return;
        s.dauer = d;
        if (s.aktuell > d) { s.aktuell = d; s.offiziell = d; }
        mAufgabe(el, function () { feuern(el, 'durationchange'); });
      },
      metadata: function (w, h) {
        if (!gilt() || s.bereit !== 0) return;
        if (video) { s.breite = w; s.hoehe = h; }
        s.bereit = 1;
        if (video && (w || h)) mAufgabe(el, function () { feuern(el, 'resize'); });
        mAufgabe(el, function () { feuern(el, 'loadedmetadata'); });
        var start = s.vorgabeStart;
        s.vorgabeStart = 0;
        if (start > 0) suchen(el, start);
        mseLage(el);
      },
      update: function () { if (gilt()) mseLage(el); },
      error: function (art) { if (gilt()) mseFehler(el, nr, art); }
    };
    a = g.__narsilMseAttach(obj, el, api);
    if (a === null || a === false) return a;
    s.mse = a;
    s.mseSuche = 0;
    s.mseDaten = false;
    // Players fetch their segments after `load`; the element does not
    // hold the window's load event back while it waits for them.
    sperre(s, false);
    return true;
  }
  function mseAbhaengen(s) {
    var a = s.mse;
    s.mse = null;
    s.mseSuche = 0;
    a.detach();
  }
  // The end of the buffered range that contains `t` (`t` when none does).
  function mseGrenze(s, t) {
    var b = s.mse.buffered();
    for (var i = 0; i < b.length; i++) if (t >= b[i][0] - 1e-6 && t < b[i][1]) return b[i][1];
    return t;
  }
  function mseGepuffert(s, t) {
    var b = s.mse.buffered();
    if (s.mse.ended() && b.length && t >= b[b.length - 1][1] - 1e-6) return true;
    for (var i = 0; i < b.length; i++) if (t >= b[i][0] - 1e-6 && t < b[i][1]) return true;
    return false;
  }
  // The readyState from the buffered data (MSE §3.5.8 steps 2-6).
  function mseLage(el) {
    var s = z(el);
    if (!s.mse || s.bereit === 0) return;
    var t = s.aktuell, b = s.mse.buffered(), ende = s.mse.ended(), r = null, neu;
    for (var i = 0; i < b.length; i++) if (t >= b[i][0] - 1e-6 && t <= b[i][1] + 1e-6) { r = b[i]; break; }
    if (ende && b.length && (t >= b[b.length - 1][1] - 1e-6 || r === b[b.length - 1])) neu = 4;
    else if (!r) neu = 1;
    else neu = r[1] - t >= 2 ? 4 : (r[1] - t > 0.1 ? 3 : 2);
    if (s.suche && s.mseSuche && s.mseSuche === s.suchNr && neu >= 2) {
      s.mseSuche = 0;
      s.suche = false;
      s.letzte = Date.now();
      zeitLaeuft(el, false);
      mAufgabe(el, function () { feuern(el, 'timeupdate'); });
      mAufgabe(el, function () { feuern(el, 'seeked'); });
    }
    mseBereit(el, neu);
  }
  // readyState transitions and their events (HTML §4.8.11.7).
  function mseBereit(el, neu) {
    var s = z(el), alt = s.bereit;
    if (neu === alt) return;
    s.bereit = neu;
    if (neu >= 2 && !s.mseDaten) {
      s.mseDaten = true;
      mAufgabe(el, function () { feuern(el, 'loadeddata'); });
    }
    if (alt >= 3 && neu <= 2 && !s.pausiert && !beendet(el)) {
      mAufgabe(el, function () { feuern(el, 'timeupdate'); feuern(el, 'waiting'); });
    }
    if (alt <= 2 && neu >= 3) {
      mAufgabe(el, function () { feuern(el, 'canplay'); });
      if (!s.pausiert) spielenMelden(el);
    }
    if (neu === 4) {
      mAufgabe(el, function () { feuern(el, 'canplaythrough'); });
      if (s.pausiert && s.autoplay && el.getAttribute('autoplay') !== null) {
        s.pausiert = false;
        s.poster = false;
        mAufgabe(el, function () { feuern(el, 'play'); });
        spielenMelden(el);
      }
    }
    if (neu >= 3) starten(el);
  }
  // endOfStream("network" | "decode") (MSE §2.4.7 step 3).
  function mseFehler(el, nr, art) {
    var s = z(el);
    if (s.bereit === 0) return fehlerAttribut(el, nr);
    s.fehler = medienFehler(art === 'network' ? 2 : 3, art === 'network' ? 'MEDIA_ELEMENT_ERROR: Network error' : 'PIPELINE_ERROR_DECODE');
    s.netz = 1;
    stoppen(el);
    mAufgabe(el, function () { feuern(el, 'error'); });
  }
  function internSpielen(el) {
    var s = z(el);
    if (s.netz === 0) auswahl(el);
    if (beendet(el) && s.rate >= 0) suchen(el, 0);
    if (s.pausiert) {
      s.pausiert = false;
      s.poster = false;
      mAufgabe(el, function () { feuern(el, 'play'); });
      if (s.bereit <= 2) mAufgabe(el, function () { feuern(el, 'waiting'); });
      else spielenMelden(el);
    } else if (s.bereit >= 3) {
      var ausstehend = s.versprechen.splice(0);
      mAufgabe(el, function () { ausstehend.forEach(function (p) { p.ja(); }); });
    }
    s.autoplay = false;
    starten(el);
  }

  def(M, 'load', function load() { laden(this); });
  def(M, 'canPlayType', function canPlayType(typ) { return typPruefen(typ); });
  def(M, 'play', function play() {
    var el = this, s = z(el);
    if (s.fehler && s.fehler.code === 4) return Promise.reject(domFehler('The element has no supported sources.', 'NotSupportedError'));
    var ja, nein;
    var p = new Promise(function (a, b) { ja = a; nein = b; });
    s.versprechen.push({ ja: ja, nein: nein });
    internSpielen(el);
    return p;
  });
  // „Internal pause steps" (§4.8.11.8).
  function internPause(el) {
    var s = z(el);
    s.autoplay = false;
    if (!s.pausiert) {
      s.pausiert = true;
      var ausstehend = s.versprechen.splice(0);
      mAufgabe(el, function () {
        feuern(el, 'timeupdate');
        feuern(el, 'pause');
        ausstehend.forEach(function (p) { p.nein(domFehler('The play() request was interrupted by a call to pause().', 'AbortError')); });
      });
      s.offiziell = s.aktuell;
    }
    stoppen(el);
  }
  def(M, 'pause', function pause() {
    var el = this, s = z(el);
    if (s.netz === 0) auswahl(el);
    internPause(el);
  });
  def(M, 'fastSeek', function fastSeek(t) { var n = doppel(t, 'fastSeek'); var s = z(this); if (s.bereit === 0) { s.vorgabeStart = n; return; } suchen(this, n); });
  def(M, 'addTextTrack', function addTextTrack(art, titel, sprache) {
    var a = String(art);
    if (ARTEN.indexOf(a) < 0) throw new TypeError("Failed to execute 'addTextTrack' on 'HTMLMediaElement': The provided value '" + a + "' is not a valid enum value of type TextTrackKind.");
    var t = spurNeu(a, titel === undefined ? '' : String(titel), sprache === undefined ? '' : String(sprache), 'hidden', this, null);
    I.get(t).bereit = 2;
    var liste = spurliste(this);
    liste._l.push(t);
    aufgabe(function () { liste.dispatchEvent(spurEreignis('addtrack', t)); });
    return t;
  });
  def(M, 'getStartDate', function getStartDate() { return new Date(NaN); });

  eig(M, 'networkState', function () { return z(this).netz; });
  eig(M, 'readyState', function () { return z(this).bereit; });
  eig(M, 'paused', function () { return z(this).pausiert; });
  eig(M, 'ended', function () { return beendet(this); });
  eig(M, 'seeking', function () { return z(this).suche; });
  eig(M, 'error', function () { return z(this).fehler; });
  eig(M, 'currentSrc', function () { return z(this).quelle; });
  eig(M, 'duration', function () { return z(this).dauer; });
  eig(M, 'currentTime', function () { var s = z(this); return s.vorgabeStart ? s.vorgabeStart : s.offiziell; }, function (v) {
    var t = doppel(v, 'currentTime'), s = z(this);
    if (s.bereit === 0) { s.vorgabeStart = t; return; }
    s.offiziell = t;
    suchen(this, t);
  });
  eig(M, 'playbackRate', function () { return z(this).rate; }, function (v) {
    var r = doppel(v, 'playbackRate'), el = this, s = z(el);
    if (r !== s.rate) { s.rate = r; mAufgabe(el, function () { feuern(el, 'ratechange'); }); }
  });
  eig(M, 'defaultPlaybackRate', function () { return z(this).vorgabeRate; }, function (v) {
    var r = doppel(v, 'defaultPlaybackRate'), el = this, s = z(el);
    if (r !== s.vorgabeRate) { s.vorgabeRate = r; mAufgabe(el, function () { feuern(el, 'ratechange'); }); }
  });
  eig(M, 'volume', function () { return z(this).lautstaerke; }, function (v) {
    var l = doppel(v, 'volume'), el = this, s = z(el);
    if (l < 0 || l > 1) throw domFehler("Failed to set the 'volume' property on 'HTMLMediaElement': The volume provided (" + l + ') is outside the range [0, 1].', 'IndexSizeError');
    if (l !== s.lautstaerke) { s.lautstaerke = l; mAufgabe(el, function () { feuern(el, 'volumechange'); }); }
  });
  eig(M, 'muted', function () { var s = z(this); return s.stumm === null ? this.getAttribute('muted') !== null : s.stumm; }, function (v) {
    var el = this, s = z(el), alt = s.stumm === null ? el.getAttribute('muted') !== null : s.stumm;
    s.stumm = !!v;
    if (alt !== s.stumm) mAufgabe(el, function () { feuern(el, 'volumechange'); });
  });
  eig(M, 'defaultMuted', function () { return this.getAttribute('muted') !== null; }, function (v) { if (v) this.setAttribute('muted', ''); else this.removeAttribute('muted'); });
  eig(M, 'buffered', function () { var s = z(this); if (s.mse) return zeitraeume(s.mse.buffered()); return zeitraeume(s.bereit >= 1 && isFinite(s.dauer) && s.dauer > 0 ? [[0, s.dauer]] : []); });
  eig(M, 'seekable', function () { var s = z(this); if (s.mse) return zeitraeume(s.bereit >= 1 ? s.mse.seekable() : []); return zeitraeume(s.bereit >= 1 && isFinite(s.dauer) ? [[0, s.dauer]] : []); });
  eig(M, 'played', function () { return zeitraeume(z(this).gespielt.map(function (r) { return [r[0], r[1]]; })); });
  eig(M, 'textTracks', function () { kinderAbgleich(this); return spurliste(this); });
  function leereSpurliste() { var l = Object.create(ET || Object.prototype); innen(l, '_l', []); def(l, 'getTrackById', function () { return null; }); eig(l, 'length', function () { return 0; }); l.onchange = null; l.onaddtrack = null; l.onremovetrack = null; return l; }
  eig(M, 'audioTracks', function () { var s = z(this); return s.ton || (s.ton = leereSpurliste()); });
  eig(M, 'videoTracks', function () { var s = z(this); return s.bild || (s.bild = leereSpurliste()); });
  eig(M, 'srcObject', function () { return z(this).objekt; }, function (v) { z(this).objekt = v === undefined ? null : v; laden(this); });
  eig(M, 'preservesPitch', function () { var s = z(this); return s.tonhoehe === undefined ? true : s.tonhoehe; }, function (v) { z(this).tonhoehe = !!v; });
  eig(M, 'src', function () { var v = this.getAttributeNS(null, 'src'); return v === null ? '' : (absolut(v) || v); }, function (v) { this.setAttribute('src', String(v)); });
  eig(M, 'crossOrigin', function () {
    var v = this.getAttribute('crossorigin');
    return v === null ? null : (v.toLowerCase() === 'use-credentials' ? 'use-credentials' : 'anonymous');
  }, function (v) { if (v === null || v === undefined) this.removeAttribute('crossorigin'); else this.setAttribute('crossorigin', String(v)); });
  eig(M, 'preload', function () {
    var v = this.getAttribute('preload');
    if (v === null) return 'metadata';
    v = v.toLowerCase();
    return v === '' ? 'auto' : (v === 'none' || v === 'metadata' || v === 'auto' ? v : 'metadata');
  }, function (v) { this.setAttribute('preload', String(v)); });
  // `controlsList` (DOMTokenList, WICG): nodownload, nofullscreen, noremoteplayback.
  var ERLAUBT = ['nodownload', 'nofullscreen', 'noremoteplayback', 'noplaybackrate'];
  function tokenListe(el, attr, erlaubt) {
    var lesen = function () { var v = el.getAttribute(attr); return v ? v.split(/[ \t\n\f\r]+/).filter(Boolean).filter(function (x, i, a) { return a.indexOf(x) === i; }) : []; };
    var schreiben = function (l) { el.setAttribute(attr, l.join(' ')); };
    var o = {
      get length() { return lesen().length; },
      get value() { return el.getAttribute(attr) || ''; },
      set value(v) { el.setAttribute(attr, String(v)); },
      item: function (i) { var l = lesen(); return i >= 0 && i < l.length ? l[i] : null; },
      contains: function (t) { return lesen().indexOf(String(t)) >= 0; },
      add: function () { var l = lesen(); for (var i = 0; i < arguments.length; i++) { var t = String(arguments[i]); if (l.indexOf(t) < 0) l.push(t); } schreiben(l); },
      remove: function () { var w = Array.prototype.map.call(arguments, String); schreiben(lesen().filter(function (t) { return w.indexOf(t) < 0; })); },
      toggle: function (t, f) { t = String(t); var l = lesen(), da = l.indexOf(t) >= 0; if (da && f !== true) { this.remove(t); return false; } if (!da && f !== false) { this.add(t); return true; } return da; },
      replace: function (a, b) { var l = lesen(), i = l.indexOf(String(a)); if (i < 0) return false; l[i] = String(b); schreiben(l); return true; },
      supports: function (t) { return erlaubt.indexOf(String(t).toLowerCase()) >= 0; },
      toString: function () { return el.getAttribute(attr) || ''; }
    };
    return o;
  }
  eig(M, 'controlsList', function () { var s = z(this); return s.kontrollen || (s.kontrollen = tokenListe(this, 'controlslist', ERLAUBT)); }, function (v) { this.setAttribute('controlslist', String(v)); });
  eig(M, 'disableRemotePlayback', function () { return this.getAttribute('disableremoteplayback') !== null; }, function (v) { if (v) this.setAttribute('disableremoteplayback', ''); else this.removeAttribute('disableremoteplayback'); });
  var V = g.HTMLVideoElement && g.HTMLVideoElement.prototype;
  if (V) {
    eig(V, 'videoWidth', function () { return z(this).bereit >= 1 ? z(this).breite : 0; });
    eig(V, 'videoHeight', function () { return z(this).bereit >= 1 ? z(this).hoehe : 0; });
    def(V, 'getVideoPlaybackQuality', function getVideoPlaybackQuality() { return { creationTime: performance.now(), droppedVideoFrames: 0, totalVideoFrames: 0, corruptedVideoFrames: 0 }; });
    def(V, 'requestVideoFrameCallback', function requestVideoFrameCallback(f) { return setTimeout(function () { f(performance.now(), {}); }, 16); });
    def(V, 'cancelVideoFrameCallback', function cancelVideoFrameCallback(i) { clearTimeout(i); });
  }

  // Meldungen des Hosts (Reaktion 9, `lebend.rs`): 1 = `src` gesetzt,
  // 2 = `<source>`/`<track>` eingefuegt (das Kind), 3 = `audio`/`video`
  // eingefuegt, 4 = `src` eines `<track>` geaendert, 5 = Kind entfernt.
  g.__narsilMedien = function (el, art) {
    if (!el) return;
    if (art === 7) { if (typeof g.__narsilDetailsUmschalten === 'function') g.__narsilDetailsUmschalten(el); return; }
    if (art === 8) { if (typeof g.__narsilBildQuelle === 'function') g.__narsilBildQuelle(el); return; }
    if (art === 9) { if (typeof g.__narsilLinkPruefen === 'function') g.__narsilLinkPruefen(); return; }
    // Release 2026-10-15: art 10 = an inserted or renamed `<details>` closes
    // (another member of its name group is open, see `details_exklusiv`).
    if (art === 10) { if (el.hasAttribute('open')) el.removeAttribute('open'); return; }
    if (art === 1) {
      if (!istMedium(el)) return;
      var neu = !zustand.get(el);
      if (el.getAttributeNS(null, 'src') !== null) laden(el);
      else if (neu) z(el);
      return;
    }
    if (art === 2) {
      var m = el.parentNode;
      if (!istMedium(m)) return;
      if (istHtml(el, 'track')) { kinderAbgleich(m); return; }
      if (!istHtml(el, 'source')) return;
      var s = z(m);
      if (s.netz === 0 && m.getAttributeNS(null, 'src') === null) { auswahl(m); return; }
      if (s.warten && nachZeiger(m)) {
        s.warten = false;
        var nr = s.wartenNr;
        stabil(function () {
          if (nr !== s.ladung) return;
          sperre(s, true);
          s.netz = 2;
          naechsteQuelle(m, nr);
        });
      }
      return;
    }
    if (art === 3) {
      var s3 = zustand.get(el);
      if (!s3) { z(el); return; }
      kinderAbgleich(el);
      if (s3.netz === 0 && (el.getAttributeNS(null, 'src') !== null || quellen(el).length)) laden(el);
      return;
    }
    if (art === 4) { spurSrcGeaendert(el); return; }
    if (art === 6) {
      var s6 = zustand.get(el);
      if (!s6) return;
      stabil(function () {
        if (el.isConnected) return;
        internPause(el);
      });
      return;
    }
    if (art === 5 && istMedium(el)) kinderAbgleich(el);
  };

  // -- Text-Spuren --
  function TrackEvent(typ, init) {
    if (!(this instanceof TrackEvent)) throw new TypeError("Failed to construct 'TrackEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'TrackEvent': 1 argument required, but only 0 present.");
    var spur = init && init.track !== undefined ? init.track : null;
    if (spur !== null && !(spur instanceof TextTrack)) throw new TypeError("Failed to construct 'TrackEvent': Failed to read the 'track' property from 'TrackEventInit': The provided value is not of type '(AudioTrack or TextTrack or VideoTrack)'.");
    var ev = new g.Event(typ, init);
    try { Object.setPrototypeOf(ev, TrackEvent.prototype); } catch (e) {}
    I.set(ev, { spur: spur });
    return ev;
  }
  if (g.Event) TrackEvent.prototype = Object.create(g.Event.prototype);
  def(TrackEvent.prototype, 'constructor', TrackEvent);
  eig(TrackEvent.prototype, 'track', function () { var x = I.get(this); return x ? x.spur : null; });
  g.TrackEvent = TrackEvent;
  function spurEreignis(typ, spur) { return new TrackEvent(typ, { track: spur }); }

  var TextTrackList = unerlaubt('TextTrackList');
  if (ET) TextTrackList.prototype = Object.create(ET);
  def(TextTrackList.prototype, 'constructor', TextTrackList);
  eig(TextTrackList.prototype, 'length', function () { return this._l.length; });
  def(TextTrackList.prototype, 'getTrackById', function getTrackById(id) {
    for (var i = 0; i < this._l.length; i++) if (this._l[i].id === String(id)) return this._l[i];
    return null;
  });
  zielMixin(TextTrackList.prototype, ['change', 'addtrack', 'removetrack']);
  if (typeof g.Symbol === 'function' && g.Symbol.iterator) def(TextTrackList.prototype, g.Symbol.iterator, function () { return this._l.slice()[g.Symbol.iterator](); });
  g.TextTrackList = TextTrackList;
  function spurliste(el) {
    var s = z(el);
    if (!s.spuren) {
      var ziel0 = Object.create(TextTrackList.prototype);
      innen(ziel0, '_l', []);
      innen(ziel0, '_el', el);
      s.spurenZiel = ziel0;
      s.spuren = lebendig(ziel0);
    }
    return s.spuren;
  }

  var TextTrackCueList = unerlaubt('TextTrackCueList');
  eig(TextTrackCueList.prototype, 'length', function () { return this._l.length; });
  def(TextTrackCueList.prototype, 'getCueById', function getCueById(id) {
    if (String(id) === '') return null;
    for (var i = 0; i < this._l.length; i++) if (this._l[i].id === String(id)) return this._l[i];
    return null;
  });
  if (typeof g.Symbol === 'function' && g.Symbol.iterator) def(TextTrackCueList.prototype, g.Symbol.iterator, function () { return this._l.slice()[g.Symbol.iterator](); });
  g.TextTrackCueList = TextTrackCueList;
  function cueListe(quelle) { var z0 = Object.create(TextTrackCueList.prototype); innen(z0, '_l', quelle); return lebendig(z0); }

  var TextTrack = unerlaubt('TextTrack');
  if (ET) TextTrack.prototype = Object.create(ET);
  def(TextTrack.prototype, 'constructor', TextTrack);
  var MODI = ['disabled', 'hidden', 'showing'];
  var zaehler = 0;
  function spurNeu(art, titel, sprache, modus, el, spurEl) {
    var t = Object.create(TextTrack.prototype);
    var x = { art: art, titel: titel, sprache: sprache, modus: modus, el: el, spurEl: spurEl, cues: [], aktiv: [], bereit: 0, laeuft: false, ladeNr: 0, wahl: false };
    x.liste = cueListe(x.cues);
    x.aktivListe = cueListe(x.aktiv);
    I.set(t, x);
    return t;
  }
  function kindVon(tr) {
    var v = tr.getAttribute('kind');
    if (v === null) return 'subtitles';
    v = v.toLowerCase();
    return ARTEN.indexOf(v) >= 0 ? v : 'metadata';
  }
  eig(TextTrack.prototype, 'kind', function () { var x = I.get(this); return x.spurEl ? kindVon(x.spurEl) : x.art; });
  eig(TextTrack.prototype, 'label', function () { var x = I.get(this); return x.spurEl ? (x.spurEl.getAttribute('label') || '') : x.titel; });
  eig(TextTrack.prototype, 'language', function () { var x = I.get(this); return x.spurEl ? (x.spurEl.getAttribute('srclang') || '') : x.sprache; });
  eig(TextTrack.prototype, 'id', function () { var x = I.get(this); return x.spurEl ? (x.spurEl.getAttribute('id') || '') : ''; });
  eig(TextTrack.prototype, 'inBandMetadataTrackDispatchType', function () { return ''; });
  function modusSetzen(t, v) {
    var x = I.get(t);
    if (MODI.indexOf(v) < 0 || x.modus === v) return;
    x.modus = v;
    if (v === 'disabled') { x.aktiv.splice(0); }
    if (x.spurEl && v !== 'disabled') spurVerarbeiten(x.spurEl);
    var el = x.el;
    if (el) {
      var s = z(el);
      if (!s.spurWechsel) {
        s.spurWechsel = true;
        var liste = spurliste(el);
        mAufgabe(el, function () { s.spurWechsel = false; liste.dispatchEvent(ereignis('change')); });
      }
      if (!s.poster) zeitPlanen(el);
    }
  }
  eig(TextTrack.prototype, 'mode', function () { return I.get(this).modus; }, function (v) { modusSetzen(this, String(v)); });
  eig(TextTrack.prototype, 'cues', function () { var x = I.get(this); return x.modus === 'disabled' ? null : x.liste; });
  eig(TextTrack.prototype, 'activeCues', function () { var x = I.get(this); return x.modus === 'disabled' ? null : x.aktivListe; });
  function cueOrdnung(a, b) {
    var x = I.get(a), y = I.get(b);
    return x.start - y.start || y.ende - x.ende || x.nr - y.nr;
  }
  def(TextTrack.prototype, 'addCue', function addCue(c) {
    if (!(c instanceof TextTrackCue)) throw new TypeError("Failed to execute 'addCue' on 'TextTrack': parameter 1 is not of type 'TextTrackCue'.");
    var x = I.get(this), ci = I.get(c);
    if (ci.spur && ci.spur !== this) ci.spur.removeCue(c);
    var i = x.cues.indexOf(c);
    if (i >= 0) x.cues.splice(i, 1);
    ci.spur = this;
    ci.nr = ++zaehler;
    x.cues.push(c);
    x.cues.sort(cueOrdnung);
    if (x.el) zeitPlanen(x.el);
  });
  def(TextTrack.prototype, 'removeCue', function removeCue(c) {
    if (!(c instanceof TextTrackCue)) throw new TypeError("Failed to execute 'removeCue' on 'TextTrack': parameter 1 is not of type 'TextTrackCue'.");
    var x = I.get(this), i = x.cues.indexOf(c);
    if (i < 0) throw domFehler("Failed to execute 'removeCue' on 'TextTrack': The specified cue is not listed in the TextTrack's list of cues.", 'NotFoundError');
    x.cues.splice(i, 1);
    var j = x.aktiv.indexOf(c);
    if (j >= 0) x.aktiv.splice(j, 1);
    I.get(c).spur = null;
    if (x.el) zeitPlanen(x.el);
  });
  zielMixin(TextTrack.prototype, ['cuechange']);
  g.TextTrack = TextTrack;

  var TextTrackCue = unerlaubt('TextTrackCue');
  if (ET) TextTrackCue.prototype = Object.create(ET);
  def(TextTrackCue.prototype, 'constructor', TextTrackCue);
  g.TextTrackCue = TextTrackCue;
  function neuSortieren(c) {
    var ci = I.get(c), sp = ci.spur;
    if (!sp) return;
    var x = I.get(sp);
    x.cues.sort(cueOrdnung);
    if (x.el) zeitPlanen(x.el);
  }
  eig(TextTrackCue.prototype, 'track', function () { return I.get(this).spur || null; });
  eig(TextTrackCue.prototype, 'id', function () { return I.get(this).id; }, function (v) { I.get(this).id = String(v); });
  eig(TextTrackCue.prototype, 'startTime', function () { return I.get(this).start; }, function (v) { I.get(this).start = doppel(v, 'startTime'); neuSortieren(this); });
  // `endTime` ist `unrestricted double` (offene Cues: +Infinity), aber NaN
  // und -Infinity sind ein TypeError.
  function endzeit(v) {
    var n = Number(v);
    if (isNaN(n) || n === -Infinity) throw new TypeError("Failed to set the 'endTime' property on 'TextTrackCue': The provided double value is non-finite.");
    return n;
  }
  eig(TextTrackCue.prototype, 'endTime', function () { return I.get(this).ende; }, function (v) { I.get(this).ende = endzeit(v); neuSortieren(this); });
  eig(TextTrackCue.prototype, 'pauseOnExit', function () { return I.get(this).pause; }, function (v) { I.get(this).pause = !!v; });
  zielMixin(TextTrackCue.prototype, ['enter', 'exit']);

  function VTTCue(start, ende, text) {
    if (!(this instanceof VTTCue)) throw new TypeError("Failed to construct 'VTTCue': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 3) throw new TypeError("Failed to construct 'VTTCue': 3 arguments required, but only " + arguments.length + ' present.');
    I.set(this, {
      start: doppel(start, 'startTime'), ende: endzeit(ende), text: String(text), id: '', pause: false, spur: null, nr: 0,
      region: null, vertical: '', snap: true, line: 'auto', lineAlign: 'start', position: 'auto', positionAlign: 'auto', size: 100, align: 'center'
    });
  }
  VTTCue.prototype = Object.create(TextTrackCue.prototype);
  def(VTTCue.prototype, 'constructor', VTTCue);
  function aufzaehlung(n, werte) {
    eig(VTTCue.prototype, n, function () { return I.get(this)[n]; }, function (v) { v = String(v); if (werte.indexOf(v) >= 0) I.get(this)[n] = v; });
  }
  aufzaehlung('vertical', ['', 'rl', 'lr']);
  aufzaehlung('lineAlign', ['start', 'center', 'end']);
  aufzaehlung('positionAlign', ['line-left', 'center', 'line-right', 'auto']);
  aufzaehlung('align', ['start', 'center', 'end', 'left', 'right']);
  eig(VTTCue.prototype, 'snapToLines', function () { return I.get(this).snap; }, function (v) { I.get(this).snap = !!v; });
  eig(VTTCue.prototype, 'line', function () { return I.get(this).line; }, function (v) { I.get(this).line = v === 'auto' ? 'auto' : doppel(v, 'line'); });
  eig(VTTCue.prototype, 'position', function () { return I.get(this).position; }, function (v) {
    if (v === 'auto') { I.get(this).position = 'auto'; return; }
    var n = doppel(v, 'position');
    if (n < 0 || n > 100) throw domFehler("Failed to set the 'position' property on 'VTTCue': The value provided (" + n + ') is outside the range [0, 100].', 'IndexSizeError');
    I.get(this).position = n;
  });
  eig(VTTCue.prototype, 'size', function () { return I.get(this).size; }, function (v) {
    var n = doppel(v, 'size');
    if (n < 0 || n > 100) throw domFehler("Failed to set the 'size' property on 'VTTCue': The value provided (" + n + ') is outside the range [0, 100].', 'IndexSizeError');
    I.get(this).size = n;
  });
  eig(VTTCue.prototype, 'region', function () { return I.get(this).region; }, function (v) { I.get(this).region = v === undefined ? null : v; });
  eig(VTTCue.prototype, 'text', function () { return I.get(this).text; }, function (v) { I.get(this).text = String(v); });
  def(VTTCue.prototype, 'getCueAsHTML', function getCueAsHTML() { return cueHtml(I.get(this).text); });
  g.VTTCue = VTTCue;
  if (typeof g.VTTRegion === 'undefined') {
    var VTTRegion = function VTTRegion() {
      if (!(this instanceof VTTRegion)) throw new TypeError("Failed to construct 'VTTRegion': Please use the 'new' operator.");
      I.set(this, { id: '', width: 100, lines: 3, rax: 0, ray: 100, vax: 0, vay: 100, scroll: '' });
    };
    eig(VTTRegion.prototype, 'id', function () { return I.get(this).id; }, function (v) { I.get(this).id = String(v); });
    [['width', 'width'], ['regionAnchorX', 'rax'], ['regionAnchorY', 'ray'], ['viewportAnchorX', 'vax'], ['viewportAnchorY', 'vay']].forEach(function (p) {
      eig(VTTRegion.prototype, p[0], function () { return I.get(this)[p[1]]; }, function (v) {
        var n = doppel(v, p[0]);
        if (n < 0 || n > 100) throw domFehler("Failed to set the '" + p[0] + "' property on 'VTTRegion': The value provided (" + n + ') is outside the range [0, 100].', 'IndexSizeError');
        I.get(this)[p[1]] = n;
      });
    });
    eig(VTTRegion.prototype, 'lines', function () { return I.get(this).lines; }, function (v) { I.get(this).lines = Number(v) >>> 0; });
    eig(VTTRegion.prototype, 'scroll', function () { return I.get(this).scroll; }, function (v) { v = String(v); if (v === '' || v === 'up') I.get(this).scroll = v; });
    g.VTTRegion = VTTRegion;
  }

  // „Time marches on" (§4.8.11.8): aktive Cues, `enter`/`exit` an den
  // Cues, `cuechange` an Spur und `<track>`; `pauseOnExit`.
  function zeitPlanen(el) {
    var s = z(el);
    if (s.zeitPlan) return;
    s.zeitPlan = true;
    aufgabe(function () { s.zeitPlan = false; zeitLaeuft(el, false); });
  }
  function zeitLaeuft(el, normal) {
    var s = zustand.get(el);
    if (!s || !s.spuren || s.poster) return;
    var t = s.aktuell, letzte = s.letzteZeit;
    var spuren = s.spuren._l.filter(function (sp) { return I.get(sp).modus !== 'disabled'; });
    var aktuelle = [], verpasst = [];
    spuren.forEach(function (sp) {
      I.get(sp).cues.forEach(function (c) {
        var ci = I.get(c);
        if (ci.start <= t && t < ci.ende) aktuelle.push(c);
        else if (normal && t >= letzte && ci.start >= letzte && ci.start <= t && ci.ende <= t) verpasst.push(c);
      });
    });
    s.letzteZeit = t;
    var vorher = s.aktiv.filter(function (c) { var sp = I.get(c).spur; return sp && spuren.indexOf(sp) >= 0; });
    var weg = s.aktiv.filter(function (c) { return aktuelle.indexOf(c) < 0; });
    var neu = aktuelle.filter(function (c) { return vorher.indexOf(c) < 0; });
    if (!verpasst.length && !weg.length && !neu.length) return;
    if (normal && !s.pausiert && weg.concat(verpasst).some(function (c) { return I.get(c).pause; })) el.pause();
    var ereignisse = [];
    verpasst.forEach(function (c) { var ci = I.get(c); ereignisse.push([ci.start, 0, c], [ci.ende, 1, c]); });
    weg.forEach(function (c) { var ci = I.get(c); ereignisse.push([Math.max(ci.start, ci.ende), 1, c]); });
    neu.forEach(function (c) { ereignisse.push([I.get(c).start, 0, c]); });
    ereignisse.sort(function (a, b) { return a[0] - b[0] || cueOrdnung(a[2], b[2]) || a[1] - b[1]; });
    var betroffen = [];
    ereignisse.forEach(function (e) { var sp = I.get(e[2]).spur; if (sp && betroffen.indexOf(sp) < 0) betroffen.push(sp); });
    weg.forEach(function (c) { var sp = I.get(c).spur; if (sp && betroffen.indexOf(sp) < 0) betroffen.push(sp); });
    betroffen.sort(function (a, b) { return s.spuren._l.indexOf(a) - s.spuren._l.indexOf(b); });
    s.aktiv = aktuelle;
    s.spuren._l.forEach(function (sp) {
      var x = I.get(sp);
      x.aktiv.splice(0, x.aktiv.length);
      if (x.modus === 'disabled') return;
      aktuelle.filter(function (c) { return I.get(c).spur === sp; }).sort(cueOrdnung).forEach(function (c) { x.aktiv.push(c); });
    });
    ereignisse.forEach(function (e) { mAufgabe(el, function () { e[2].dispatchEvent(ereignis(e[1] ? 'exit' : 'enter')); }); });
    betroffen.forEach(function (sp) {
      mAufgabe(el, function () {
        sp.dispatchEvent(ereignis('cuechange'));
        var tr = I.get(sp).spurEl;
        if (tr) feuern(tr, 'cuechange');
      });
    });
  }

  // -- WebVTT (W3C WebVTT §6.1, „WebVTT parser algorithm") --
  function vttZeit(z0, pos) {
    var p = pos.p, digs = function () { var a = p; while (p < z0.length && z0.charCodeAt(p) >= 48 && z0.charCodeAt(p) <= 57) p++; return z0.slice(a, p); };
    var stunden = false;
    var s1 = digs();
    if (!s1) return NaN;
    var v1 = parseInt(s1, 10);
    if (s1.length !== 2 || v1 > 59) stunden = true;
    if (z0.charAt(p) !== ':') return NaN;
    p++;
    var s2 = digs();
    if (s2.length !== 2) return NaN;
    var v2 = parseInt(s2, 10), v3, h = 0, m, sek;
    if (stunden || z0.charAt(p) === ':') {
      if (z0.charAt(p) !== ':') return NaN;
      p++;
      var s3 = digs();
      if (s3.length !== 2) return NaN;
      v3 = parseInt(s3, 10);
      h = v1; m = v2; sek = v3;
    } else { m = v1; sek = v2; }
    if (z0.charAt(p) !== '.') return NaN;
    p++;
    var s4 = digs();
    if (s4.length !== 3) return NaN;
    if (m > 59 || sek > 59) return NaN;
    pos.p = p;
    return h * 3600 + m * 60 + sek + parseInt(s4, 10) / 1000;
  }
  function prozent(v) {
    if (!/^[0-9]+(\.[0-9]+)?%$/.test(v)) return null;
    var n = parseFloat(v);
    return n >= 0 && n <= 100 ? n : null;
  }
  function vttEinstellungen(c, rest) {
    var x = I.get(c);
    rest.split(/[ \t\f]+/).forEach(function (e) {
      var i = e.indexOf(':');
      if (i <= 0 || i === e.length - 1) return;
      var n = e.slice(0, i), v = e.slice(i + 1);
      if (n === 'vertical') { if (v === 'rl' || v === 'lr') x.vertical = v; return; }
      if (n === 'line') {
        var k = v.indexOf(','), lp = k >= 0 ? v.slice(0, k) : v, la = k >= 0 ? v.slice(k + 1) : null, zahl;
        if (!/[0-9]/.test(lp)) return;
        if (lp.charAt(lp.length - 1) === '%') { zahl = prozent(lp); if (zahl === null) return; }
        else {
          if (/[^-0-9.]/.test(lp) || lp.indexOf('-', 1) > 0 || (lp.match(/\./g) || []).length > 1) return;
          if (lp.indexOf('.') >= 0 && !/[0-9]\.[0-9]/.test(lp)) return;
          zahl = parseFloat(lp);
          if (!isFinite(zahl)) return;
        }
        if (la !== null && ['start', 'center', 'end'].indexOf(la) < 0) return;
        x.lineAlign = la || 'start';
        x.snap = lp.charAt(lp.length - 1) !== '%';
        x.line = zahl;
        return;
      }
      if (n === 'position') {
        var k2 = v.indexOf(','), cp = k2 >= 0 ? v.slice(0, k2) : v, ca = k2 >= 0 ? v.slice(k2 + 1) : null;
        var pz = prozent(cp);
        if (pz === null) return;
        if (ca !== null && ['line-left', 'center', 'line-right'].indexOf(ca) < 0) return;
        x.position = pz;
        x.positionAlign = ca || 'auto';
        return;
      }
      if (n === 'size') { var gz = prozent(v); if (gz !== null) x.size = gz; return; }
      if (n === 'align') { if (['start', 'center', 'end', 'left', 'right'].indexOf(v) >= 0) x.align = v; }
    });
  }
  function vttZeitzeile(c, zeile) {
    var pos = { p: 0 };
    var leer = function () { while (pos.p < zeile.length && /[ \t\f]/.test(zeile.charAt(pos.p))) pos.p++; };
    leer();
    var a = vttZeit(zeile, pos);
    if (isNaN(a)) return false;
    leer();
    if (zeile.substr(pos.p, 3) !== '-->') return false;
    pos.p += 3;
    leer();
    var b = vttZeit(zeile, pos);
    if (isNaN(b)) return false;
    var x = I.get(c);
    x.start = a; x.ende = b;
    vttEinstellungen(c, zeile.slice(pos.p));
    return true;
  }
  function webvtt(text) {
    text = String(text).replace(/\0/g, '�').replace(/\r\n?/g, '\n');
    if (text.charCodeAt(0) === 0xFEFF) text = text.slice(1);
    if (!/^WEBVTT(?:[ \t\n]|$)/.test(text)) return null;
    var zeilen = text.split('\n'), i = 1, n = zeilen.length, cues = [];
    // Kopf: bis zur ersten Leerzeile (eine Zeile mit `-->` beginnt einen Cue).
    if (i < n && zeilen[i] !== '') {
      while (i < n && zeilen[i] !== '') { if (zeilen[i].indexOf('-->') >= 0) break; i++; }
    }
    while (i < n) {
      while (i < n && zeilen[i] === '') i++;
      if (i >= n) break;
      // Einen Block sammeln („collect a WebVTT block").
      var puffer = '', anzahl = 0, pfeil = false, cue = null, gesehen = false;
      while (i < n) {
        var z0 = zeilen[i];
        anzahl++;
        if (z0.indexOf('-->') >= 0) {
          if (anzahl === 1 || (anzahl === 2 && !pfeil)) {
            pfeil = true;
            i++;
            cue = new VTTCue(0, 0, '');
            I.get(cue).id = puffer;
            if (!vttZeitzeile(cue, z0)) cue = null;
            else { puffer = ''; gesehen = true; }
            continue;
          }
          break;
        }
        if (z0 === '') { i++; break; }
        if (!gesehen && anzahl === 2 && /^(STYLE|REGION)(?:[ \t]|$)/.test(puffer)) { puffer = ''; }
        if (puffer !== '') puffer += '\n';
        puffer += z0;
        i++;
      }
      if (cue) { I.get(cue).text = puffer; cues.push(cue); }
    }
    return cues;
  }

  // Cue-Text als DOM (WebVTT §6.4/§6.5): c/i/b/u/ruby/rt/v/lang, Klassen,
  // Annotationen, Zeitmarken als ProcessingInstruction `timestamp`.
  var ZEICHEN = { amp: '&', lt: '<', gt: '>', lrm: '‎', rlm: '‏', nbsp: ' ', quot: '"', apos: "'" };
  function entitaeten(t) {
    return t.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, function (m, k) {
      if (k.charAt(0) === '#') { var c = k.charAt(1) === 'x' || k.charAt(1) === 'X' ? parseInt(k.slice(2), 16) : parseInt(k.slice(1), 10); try { return String.fromCodePoint(c); } catch (e) { return m; } }
      return Object.prototype.hasOwnProperty.call(ZEICHEN, k) ? ZEICHEN[k] : m;
    });
  }
  function cueHtml(text) {
    var f = doc.createDocumentFragment(), stapel = [f], i = 0, n = text.length;
    if (!n) { f.appendChild(doc.createTextNode('')); return f; }
    var oben = function () { return stapel[stapel.length - 1]; };
    var NAMEN = { c: 'span', i: 'i', b: 'b', u: 'u', ruby: 'ruby', rt: 'rt', v: 'span', lang: 'span' };
    while (i < n) {
      var lt = text.indexOf('<', i);
      if (lt !== i) {
        var stueck = text.slice(i, lt < 0 ? n : lt);
        if (stueck) oben().appendChild(doc.createTextNode(entitaeten(stueck)));
        if (lt < 0) break;
        i = lt;
      }
      var gt = text.indexOf('>', i);
      var marke = text.slice(i + 1, gt < 0 ? n : gt);
      i = gt < 0 ? n : gt + 1;
      if (marke.charAt(0) === '/') {
        var name = marke.slice(1).trim();
        var o = oben();
        if (o._vttName === name) stapel.pop();
        else if (name === 'ruby' && o._vttName === 'rt') { stapel.pop(); if (oben()._vttName === 'ruby') stapel.pop(); }
        continue;
      }
      if (/^[0-9]/.test(marke)) {
        var pos = { p: 0 }, zeit = vttZeit(marke, pos);
        if (!isNaN(zeit) && pos.p === marke.length && typeof doc.createProcessingInstruction === 'function') {
          try { oben().appendChild(doc.createProcessingInstruction('timestamp', marke)); } catch (e) {}
        }
        continue;
      }
      var leer = marke.search(/[ \t\n\f]/);
      var kopfTeil = leer >= 0 ? marke.slice(0, leer) : marke, anmerkung = leer >= 0 ? marke.slice(leer + 1).trim().replace(/[ \t\n\f]+/g, ' ') : '';
      var teile = kopfTeil.split('.'), tag = teile.shift();
      if (!Object.prototype.hasOwnProperty.call(NAMEN, tag)) continue;
      if (tag === 'rt' && oben()._vttName !== 'ruby') continue;
      var e = doc.createElement(NAMEN[tag]);
      try { Object.defineProperty(e, '_vttName', { value: tag, configurable: true }); } catch (x) {}
      var klassen = teile.filter(Boolean);
      if (klassen.length) e.setAttribute('class', klassen.join(' '));
      if (tag === 'v') e.setAttribute('title', entitaeten(anmerkung));
      if (tag === 'lang') e.setAttribute('lang', anmerkung);
      oben().appendChild(e);
      stapel.push(e);
    }
    return f;
  }

  // -- <track> (§4.8.13): Spur, Auswahl, Verarbeitung --
  var spurVonEl = new WeakMap();
  function spurVon(tr) {
    var t = spurVonEl.get(tr);
    if (!t) { t = spurNeu('', '', '', 'disabled', null, tr); spurVonEl.set(tr, t); }
    return t;
  }
  function vorDran(a, b) {
    if (typeof a.compareDocumentPosition === 'function') return !!(a.compareDocumentPosition(b) & 4);
    var k = Array.prototype.slice.call(a.parentNode.childNodes);
    return k.indexOf(a) < k.indexOf(b);
  }
  // Die `<track>`-Kinder und die Spurliste abgleichen: neue anhaengen (in
  // Baumordnung vor den Skript-Spuren), entfernte austragen.
  function kinderAbgleich(el) {
    var liste = spurliste(el);
    var kinder = Array.prototype.filter.call(el.children, function (k) { return istHtml(k, 'track'); });
    liste._l.slice().forEach(function (t) {
      var x = I.get(t);
      if (x.spurEl && x.spurEl.parentNode !== el) {
        var i = liste._l.indexOf(t);
        if (i >= 0) liste._l.splice(i, 1);
        x.el = null;
        x.aktiv.splice(0);
        var s = z(el), j = s.aktiv.length;
        while (j--) if (I.get(s.aktiv[j]).spur === t) s.aktiv.splice(j, 1);
        aufgabe(function () { liste.dispatchEvent(spurEreignis('removetrack', t)); });
      }
    });
    kinder.forEach(function (tr) {
      var t = spurVon(tr), x = I.get(t);
      if (liste._l.indexOf(t) >= 0) return;
      x.el = el;
      var l = liste._l, pos = 0;
      while (pos < l.length && I.get(l[pos]).spurEl && vorDran(I.get(l[pos]).spurEl, tr)) pos++;
      l.splice(pos, 0, t);
      aufgabe(function () { liste.dispatchEvent(spurEreignis('addtrack', t)); });
      x.wahl = true;
      aufgabe(function () {
        x.wahl = false;
        autoWahl(el);
        bereitPruefen(el);
      });
      if (x.modus !== 'disabled') spurVerarbeiten(tr);
    });
  }
  // „Honor user preferences for automatic text track selection" (§4.8.11.11.3).
  function autoWahl(el) {
    var s = zustand.get(el);
    if (!s || !s.spuren) return;
    var l = s.spuren._l.filter(function (t) { return I.get(t).spurEl; });
    var gruppe = function (arten, modus) {
      var mit = l.filter(function (t) { return arten.indexOf(t.kind) >= 0; });
      if (mit.some(function (t) { return I.get(t).modus === 'showing'; })) return;
      for (var i = 0; i < mit.length; i++) {
        var x = I.get(mit[i]);
        if (x.spurEl.hasAttribute('default') && x.modus === 'disabled') { modusSetzen(mit[i], modus); return; }
      }
    };
    gruppe(['subtitles', 'captions'], 'showing');
    gruppe(['descriptions'], 'showing');
    gruppe(['chapters'], 'showing');
    l.forEach(function (t) {
      var x = I.get(t);
      if (t.kind === 'metadata' && x.spurEl.hasAttribute('default') && x.modus === 'disabled') modusSetzen(t, 'hidden');
    });
  }
  function bereitPruefen(el) {
    var s = zustand.get(el);
    if (s && s.spurNr && s.spurNr === s.ladung) weiterBereit(el, s.spurNr);
  }
  // Die Verarbeitung einer Spur (§4.8.13 „start the track processing model").
  function spurVerarbeiten(tr) {
    var t = spurVon(tr), x = I.get(t);
    if (x.laeuft || x.modus === 'disabled' || !istMedium(tr.parentNode)) return;
    x.laeuft = true;
    var nr = ++x.ladeNr;
    stabil(function () {
      if (nr !== x.ladeNr) return;
      x.bereit = 1;
      var src = tr.getAttributeNS(null, 'src');
      var url = src ? absolut(src) : null;
      if (!url) { aufgabe(function () { spurFertig(tr, nr, null); }); return; }
      var art = tr.parentNode && tr.parentNode.getAttribute('crossorigin');
      var opt = art === null || art === undefined ? {} : { mode: 'cors', credentials: String(art).toLowerCase() === 'use-credentials' ? 'include' : 'same-origin' };
      g.fetch(url, opt).then(function (r) { if (!r.ok) throw new Error('Status ' + r.status); return r.text(); })
        .then(function (txt) { spurFertig(tr, nr, webvtt(txt)); }, function () { spurFertig(tr, nr, null); });
    });
  }
  function spurFertig(tr, nr, cues) {
    var t = spurVon(tr), x = I.get(t);
    if (nr !== x.ladeNr) return;
    if (!cues) {
      x.bereit = 3;
      aufgabe(function () { feuern(tr, 'error'); });
    } else {
      cues.forEach(function (c) { t.addCue(c); });
      x.bereit = 2;
      aufgabe(function () { feuern(tr, 'load'); });
    }
    if (x.el) bereitPruefen(x.el);
  }
  // `src` eines `<track>` geaendert: Cues leeren, neu laden (§4.8.13).
  function spurSrcGeaendert(tr) {
    var t = spurVonEl.get(tr);
    if (!t) return;
    var x = I.get(t);
    x.ladeNr++;
    x.laeuft = false;
    x.cues.slice().forEach(function (c) { t.removeCue(c); });
    x.bereit = 0;
    if (x.modus !== 'disabled') spurVerarbeiten(tr);
  }
  var TR = g.HTMLTrackElement && g.HTMLTrackElement.prototype;
  if (TR) {
    eig(TR, 'track', function () { return spurVon(this); });
    eig(TR, 'readyState', function () { var t = spurVonEl.get(this); return t ? I.get(t).bereit : 0; });
    [['NONE', 0], ['LOADING', 1], ['LOADED', 2], ['ERROR', 3]].forEach(function (p) {
      try { Object.defineProperty(TR, p[0], { value: p[1], enumerable: true }); Object.defineProperty(g.HTMLTrackElement, p[0], { value: p[1], enumerable: true }); } catch (e) {}
    });
    eig(TR, 'kind', function () { return kindVon(this); }, function (v) { this.setAttribute('kind', String(v)); });
    eig(TR, 'src', function () { var v = this.getAttributeNS(null, 'src'); return v === null ? '' : (absolut(v) || v); }, function (v) { this.setAttribute('src', String(v)); });
    eig(TR, 'srclang', function () { var v = this.getAttribute('srclang'); return v === null ? '' : v; }, function (v) { this.setAttribute('srclang', String(v)); });
    eig(TR, 'label', function () { var v = this.getAttribute('label'); return v === null ? '' : v; }, function (v) { this.setAttribute('label', String(v)); });
    eig(TR, 'default', function () { return this.hasAttribute('default'); }, function (v) { if (v) this.setAttribute('default', ''); else this.removeAttribute('default'); });
  }

  // Medien im Ausgangs-HTML, die kein Skript angefasst hat.
  function anfangs() { Array.prototype.forEach.call(doc.querySelectorAll('video, audio'), function (el) { z(el); }); }
  if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', anfangs);
  else setTimeout(anfangs, 0);
})();

// -- Slots: assignedNodes/assignedElements/assign, assignedSlot, slotchange --
// Release 13.10.2026: DOM §4.2.2.3 „find a slot", „find slottables", „find
// flattened slottables", §4.2.2.5 „signal a slot change", HTML §4.12.4
// (`HTMLSlotElement`). GEMESSEN vorher (Codex-Lauf 12.10., shadow-dom):
// `assignedNodes`/`assignedSlot`/`assign` fehlten ganz — slots.html 0/26,
// slotchange-event 0/32, imperative-slot-api 1/16.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var SP = g.HTMLSlotElement && g.HTMLSlotElement.prototype;
  if (!doc || !SP || typeof SP.assignedNodes === 'function') return;
  var HTML = 'http://www.w3.org/1999/xhtml';
  var manuellListe = new WeakMap(), manuellSlot = new WeakMap();
  function def(o, n, v) { try { Object.defineProperty(o, n, { value: v, configurable: true, writable: true, enumerable: false }); } catch (e) {} }
  function eig(o, n, lesen) { try { Object.defineProperty(o, n, { get: lesen, configurable: true, enumerable: true }); } catch (e) {} }
  function wurzelVon(n) { try { return n.getRootNode(); } catch (e) { return null; } }
  function istSchatten(r) { return !!r && r.nodeType === 11 && !!r.host; }
  function schattenVon(el) { try { return el.__narsilSchattenIntern || null; } catch (e) { return null; } }
  function istSlot(n) { return !!n && n.nodeType === 1 && n.localName === 'slot' && n.namespaceURI === HTML; }
  function istSlottable(n) { return !!n && (n.nodeType === 1 || n.nodeType === 3); }
  function manuell(r) { try { return r.slotAssignment === 'manual'; } catch (e) { return false; } }
  function slotName(n) { return n.nodeType === 1 ? (n.getAttribute('slot') || '') : ''; }
  // Der erste Slot je Name (Baumreihenfolge) einer Wurzel.
  function slotKarte(r) {
    var karte = Object.create(null), l = r.querySelectorAll('slot');
    for (var i = 0; i < l.length; i++) {
      if (l[i].namespaceURI !== HTML) continue;
      var n = l[i].getAttribute('name') || '';
      if (!(n in karte)) karte[n] = l[i];
    }
    return karte;
  }
  function findeSlot(n, nurOffen) {
    var p = n.parentNode;
    if (!p || p.nodeType !== 1) return null;
    var r = schattenVon(p);
    if (!r) return null;
    if (nurOffen && r.mode !== 'open') return null;
    if (manuell(r)) {
      var s = manuellSlot.get(n);
      return s && wurzelVon(s) === r ? s : null;
    }
    return slotKarte(r)[slotName(n)] || null;
  }
  function findeSlottables(slot) {
    var r = wurzelVon(slot);
    if (!istSchatten(r)) return [];
    var host = r.host, raus = [];
    if (manuell(r)) {
      (manuellListe.get(slot) || []).forEach(function (n) { if (n.parentNode === host) raus.push(n); });
      return raus;
    }
    var karte = null, k = host.childNodes;
    for (var i = 0; i < k.length; i++) {
      if (!istSlottable(k[i])) continue;
      if (!karte) karte = slotKarte(r);
      if (karte[slotName(k[i])] === slot) raus.push(k[i]);
    }
    return raus;
  }
  function flach(slot, raus, tiefe) {
    // „find flattened slottables" Schritt 2: nur Slots in einem Schattenbaum.
    if (!istSchatten(wurzelVon(slot))) return raus;
    var l = findeSlottables(slot);
    if (!l.length) l = Array.prototype.filter.call(slot.childNodes, istSlottable);
    for (var i = 0; i < l.length; i++) {
      var n = l[i];
      if (istSlot(n) && istSchatten(wurzelVon(n)) && tiefe < 64) flach(n, raus, tiefe + 1);
      else raus.push(n);
    }
    return raus;
  }
  def(SP, 'assignedNodes', function assignedNodes(optionen) {
    return optionen && optionen.flatten ? flach(this, [], 0) : findeSlottables(this);
  });
  def(SP, 'assignedElements', function assignedElements(optionen) {
    var l = optionen && optionen.flatten ? flach(this, [], 0) : findeSlottables(this);
    return l.filter(function (n) { return n.nodeType === 1; });
  });
  def(SP, 'assign', function assign() {
    var slot = this, alt = manuellListe.get(slot) || [], neu = [];
    var betroffen = [slot];
    alt.forEach(function (n) { if (manuellSlot.get(n) === slot) manuellSlot.delete(n); });
    for (var i = 0; i < arguments.length; i++) {
      var n = arguments[i];
      if (!istSlottable(n)) throw new TypeError("Failed to execute 'assign' on 'HTMLSlotElement': parameter " + (i + 1) + " is not of type 'Element' or 'Text'.");
      if (neu.indexOf(n) >= 0) continue;
      var vorher = manuellSlot.get(n);
      if (vorher && vorher !== slot) {
        var vl = manuellListe.get(vorher);
        if (vl) { var j = vl.indexOf(n); if (j >= 0) vl.splice(j, 1); }
        betroffen.push(vorher);
      }
      manuellSlot.set(n, slot);
      neu.push(n);
    }
    manuellListe.set(slot, neu);
    betroffen.forEach(function (s) { pruefenSpaeter(wurzelVon(s)); });
    try { if (typeof g.__narsilSlotsGeaendert === 'function') g.__narsilSlotsGeaendert(); } catch (e) {}
  });
  ['Element', 'Text'].forEach(function (K) {
    var P = g[K] && g[K].prototype;
    if (P && !Object.getOwnPropertyDescriptor(P, 'assignedSlot')) eig(P, 'assignedSlot', function () { return findeSlot(this, true); });
  });

  // slotchange (§4.2.2.5): erst, wenn jemand darauf hoert — der Beobachter
  // schaltet das Aenderungsprotokoll ein und kostet auf DOM-lastigen Seiten.
  var an = false, beobachter = null, wurzeln = [], stand = new WeakMap(), gesehen = new WeakMap();
  // Die letzten Wurzeln aus `attachShadow` (begrenzt: ohne WeakRef haelt die
  // Liste sie fest), damit auch eine Wurzel ausserhalb des Dokuments, die VOR
  // dem ersten Lauscher entstand, beobachtet wird.
  var juengste = [];
  var offen = [], geplant = false;
  function gleich(a, b) {
    if (a.length !== b.length) return false;
    for (var i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
    return true;
  }
  function wurzelAnmelden(r) {
    if (!istSchatten(r) || wurzeln.indexOf(r) >= 0 || wurzeln.length > 20000) return;
    wurzeln.push(r);
    try {
      beobachter.observe(r, { childList: true, subtree: true, attributes: true, attributeFilter: ['name'] });
      beobachter.observe(r.host, { childList: true, subtree: true, attributes: true, attributeFilter: ['slot'] });
    } catch (e) {}
    var l = r.querySelectorAll('slot'), liste = [];
    for (var i = 0; i < l.length; i++) { stand.set(l[i], findeSlottables(l[i])); liste.push(l[i]); }
    gesehen.set(r, liste);
  }
  function pruefen(r) {
    if (!an || !istSchatten(r)) return;
    var l = Array.prototype.slice.call(r.querySelectorAll('slot')), feuer = [];
    l.forEach(function (s) {
      var jetzt = findeSlottables(s), vorher = stand.get(s) || [];
      if (!gleich(jetzt, vorher)) { stand.set(s, jetzt); feuer.push(s); }
    });
    // Ein entfernter Slot mit zugewiesenen Knoten meldet sich auch.
    (gesehen.get(r) || []).forEach(function (s) {
      if (l.indexOf(s) < 0 && (stand.get(s) || []).length) { stand.set(s, []); feuer.push(s); }
    });
    gesehen.set(r, l);
    feuer.forEach(function (s) {
      var ev;
      try { ev = g.__narsilUA(g.Event, 'slotchange', { bubbles: true }); } catch (e) { return; }
      try { s.dispatchEvent(ev); } catch (e) {}
    });
  }
  function pruefenSpaeter(r) {
    if (!an || !istSchatten(r)) return;
    if (offen.indexOf(r) < 0) offen.push(r);
    if (geplant) return;
    geplant = true;
    Promise.resolve().then(function () {
      geplant = false;
      var l = offen; offen = [];
      l.forEach(pruefen);
    });
  }
  function einschalten() {
    if (an || typeof g.MutationObserver !== 'function') return;
    an = true;
    beobachter = new g.MutationObserver(function (eintraege) {
      var betroffen = [];
      eintraege.forEach(function (e) {
        var t = e.target, r = wurzelVon(t);
        if (istSchatten(r) && betroffen.indexOf(r) < 0) betroffen.push(r);
        // Am Wirt: seine Wurzel (Kinder oder `slot` geaendert).
        var w = e.type === 'attributes' ? t.parentNode : t;
        var sr = w && w.nodeType === 1 ? schattenVon(w) : null;
        if (sr && betroffen.indexOf(sr) < 0) betroffen.push(sr);
        Array.prototype.forEach.call(e.addedNodes || [], function (n) {
          if (n.nodeType === 1 && n.querySelectorAll) {
            Array.prototype.forEach.call([n].concat(Array.prototype.slice.call(n.querySelectorAll('*'))), function (x) {
              var s2 = schattenVon(x);
              if (s2) wurzelAnmelden(s2);
            });
          }
        });
      });
      betroffen.forEach(function (r) { wurzelAnmelden(r); pruefen(r); });
    });
    // Schon vorhandene Wirte (auch deklarative).
    juengste.forEach(wurzelAnmelden);
    juengste = [];
    try {
      Array.prototype.forEach.call(doc.querySelectorAll('*'), function (x) { var s = schattenVon(x); if (s) wurzelAnmelden(s); });
    } catch (e) {}
  }
  // Der Host meldet den ersten Lauscher (Reaktion 11); am Fenster und an
  // eigenen Zielen laeuft `addEventListener` ueber den Prototyp.
  g.__narsilSlotEin = function () { einschalten(); };
  // HTML §4.12.3: die Spiegel des deklarativen Shadow DOM am `<template>`.
  var TP = g.HTMLTemplateElement && g.HTMLTemplateElement.prototype;
  if (TP && !('shadowRootMode' in TP)) {
    var aufzaehlung = function (prop, attr, werte, rueckfall) {
      try {
        Object.defineProperty(TP, prop, {
          get: function () {
            var v = this.getAttribute(attr);
            v = v === null ? null : String(v).toLowerCase();
            return werte.indexOf(v) >= 0 ? v : rueckfall;
          },
          set: function (v) { this.setAttribute(attr, String(v)); },
          configurable: true, enumerable: true
        });
      } catch (e) {}
    };
    var schalter = function (prop, attr) {
      try {
        Object.defineProperty(TP, prop, {
          get: function () { return this.hasAttribute(attr); },
          set: function (v) { if (v) this.setAttribute(attr, ''); else this.removeAttribute(attr); },
          configurable: true, enumerable: true
        });
      } catch (e) {}
    };
    aufzaehlung('shadowRootMode', 'shadowrootmode', ['open', 'closed'], '');
    aufzaehlung('shadowRootSlotAssignment', 'shadowrootslotassignment', ['manual', 'named'], 'named');
    schalter('shadowRootDelegatesFocus', 'shadowrootdelegatesfocus');
    schalter('shadowRootClonable', 'shadowrootclonable');
    schalter('shadowRootSerializable', 'shadowrootserializable');
  }
  var ETP = g.EventTarget && g.EventTarget.prototype, altHoeren = ETP && ETP.addEventListener;
  if (altHoeren) {
    def(ETP, 'addEventListener', function addEventListener(typ) {
      if (!an && typ === 'slotchange') einschalten();
      return altHoeren.apply(this, arguments);
    });
  }
  var EP = g.Element && g.Element.prototype, altSchatten = EP && EP.attachShadow;
  if (altSchatten) {
    def(EP, 'attachShadow', function attachShadow(optionen) {
      // WebIDL: `slotAssignment` ist ein Aufzaehlungstyp (SlotAssignmentMode).
      if (optionen && optionen.slotAssignment !== undefined && ['manual', 'named'].indexOf(String(optionen.slotAssignment)) < 0) {
        throw new TypeError("Failed to execute 'attachShadow' on 'Element': The provided value '" + String(optionen.slotAssignment) + "' is not a valid enum value of type SlotAssignmentMode.");
      }
      var r = altSchatten.apply(this, arguments);
      if (an) wurzelAnmelden(r);
      else { juengste.push(r); if (juengste.length > 256) juengste.shift(); }
      return r;
    });
  }
})();

// -- Storage (HTML §12.2): localStorage/sessionStorage als Storage-Objekte --
// Release 13.10.2026: benannte Eigenschaften (WebIDL §3.9 „legacy platform
// objects": `storage.x`, `storage.x = v`, `'x' in storage`, `delete`,
// `Object.keys`), ein lebendes `length` und `Storage.prototype`. GEMESSEN
// vorher (Codex-Lauf 12.10.): `length` war immer 0, `storage['x'] = 'y'`
// landete nicht im Speicher — webstorage 3/53 Dateien.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.Storage === 'function' && g.Storage.__narsil) return;
  var innen = new WeakMap();
  function Storage() { throw new TypeError('Illegal constructor'); }
  var SP = Storage.prototype;
  function n(o, f) {
    var s = innen.get(o);
    if (!s) throw new TypeError("Illegal invocation");
    return s[f];
  }
  function def(o, k, v) { try { Object.defineProperty(o, k, { value: v, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  function argu(zahl, name, args) {
    if (args.length < zahl) throw new TypeError("Failed to execute '" + name + "' on 'Storage': " + zahl + " argument" + (zahl > 1 ? 's' : '') + " required, but only " + args.length + " present.");
  }
  def(SP, 'getItem', function getItem(k) { argu(1, 'getItem', arguments); return n(this, 'holen')(String(k)); });
  function melden(bereich, k, alt, neu) {
    if (typeof g.__narsilSpeicherMelden !== 'function') return;
    try { g.__narsilSpeicherMelden(bereich, k, alt, neu, g.__narsilRahmenFenster || g); } catch (e) {}
  }
  def(SP, 'setItem', function setItem(k, v) {
    argu(2, 'setItem', arguments);
    var r;
    k = String(k); v = String(v);
    var alt = n(this, 'holen')(k);
    try { r = n(this, 'setzen')(k, v); } catch (e) {
      if (/Quota/.test(String(e && e.message || e)) && typeof g.DOMException === 'function') throw new g.DOMException("Failed to execute 'setItem' on 'Storage': Setting the value exceeded the quota.", 'QuotaExceededError');
      throw e;
    }
    if (alt !== v) melden(this, k, alt, v);
    return r;
  });
  def(SP, 'removeItem', function removeItem(k) {
    argu(1, 'removeItem', arguments);
    k = String(k);
    var alt = n(this, 'holen')(k);
    n(this, 'entfernen')(k);
    if (alt !== null) melden(this, k, alt, null);
  });
  def(SP, 'clear', function clear() {
    var vorher = n(this, 'laenge')();
    n(this, 'leeren')();
    if (vorher > 0) melden(this, null, null, null);
  });
  def(SP, 'key', function key(i) {
    argu(1, 'key', arguments);
    i = Number(i);
    if (!isFinite(i)) i = 0;
    i = Math.trunc(i) % 4294967296;
    if (i < 0) i += 4294967296;
    return n(this, 'schluessel')(i);
  });
  try {
    Object.defineProperty(SP, 'length', { get: function () { return n(this, 'laenge')(); }, configurable: true, enumerable: true });
  } catch (e) {}
  try { Object.defineProperty(SP, Symbol.toStringTag, { value: 'Storage', configurable: true }); } catch (e) {}
  def(Storage, '__narsil', true);
  try { Object.defineProperty(Storage, '__narsil', { enumerable: false }); } catch (e) {}

  function bauen(sfx) {
    var f = {
      holen: g['speicherHolen' + sfx], setzen: g['speicherSetzen' + sfx], entfernen: g['speicherEntfernen' + sfx],
      leeren: g['speicherLeeren' + sfx], schluessel: g['speicherSchluessel' + sfx], laenge: g['speicherLaenge' + sfx],
      liste: g['speicherListe' + sfx]
    };
    if (typeof f.holen !== 'function' || typeof f.laenge !== 'function') return null;
    var ziel = Object.create(SP);
    innen.set(ziel, f);
    function hat(k) { return typeof k === 'string' && f.holen(k) !== null; }
    // Benannte Eigenschaften sind sichtbar, wenn der Prototyp den Namen nicht
    // traegt ([LegacyOverrideBuiltIns] fehlt) und kein eigener existiert.
    function sichtbar(t, k) { return typeof k === 'string' && !(k in SP) && !Object.prototype.hasOwnProperty.call(t, k) && hat(k); }
    var p = new Proxy(ziel, {
      get: function (t, k, emp) {
        if (sichtbar(t, k)) return f.holen(k);
        var v = Reflect.get(t, k, emp === p ? t : emp);
        return v;
      },
      set: function (t, k, v, emp) {
        if (typeof k === 'string' && (emp === p || emp === t)) { SP.setItem.call(p, k, v); return true; }
        return Reflect.set(t, k, v, emp);
      },
      has: function (t, k) { return hat(k) || Reflect.has(t, k); },
      deleteProperty: function (t, k) {
        if (typeof k === 'string' && hat(k) && !Object.prototype.hasOwnProperty.call(t, k)) { f.entfernen(k); return true; }
        return Reflect.deleteProperty(t, k);
      },
      ownKeys: function (t) {
        var l = typeof f.liste === 'function' ? Array.prototype.slice.call(f.liste()) : [];
        Reflect.ownKeys(t).forEach(function (k) { if (l.indexOf(k) < 0) l.push(k); });
        return l;
      },
      getOwnPropertyDescriptor: function (t, k) {
        if (sichtbar(t, k)) return { value: f.holen(k), writable: true, enumerable: true, configurable: true };
        return Reflect.getOwnPropertyDescriptor(t, k);
      },
      defineProperty: function (t, k, d) {
        if (typeof k === 'string' && !(k in SP) && d && 'value' in d) { SP.setItem.call(p, k, d.value); return true; }
        return Reflect.defineProperty(t, k, d);
      },
      getPrototypeOf: function () { return SP; }
    });
    innen.set(p, f);
    return p;
  }
  [['localStorage', 'Lokal'], ['sessionStorage', 'Sitzung']].forEach(function (e) {
    var s = bauen(e[1]);
    if (!s) return;
    try { Object.defineProperty(g, e[0], { get: function () { return s; }, configurable: true, enumerable: true }); }
    catch (x) { try { g[e[0]] = s; } catch (y) {} }
  });
  try { Object.defineProperty(g, 'Storage', { value: Storage, configurable: true, writable: true, enumerable: false }); } catch (e) { g.Storage = Storage; }
})();

// -- Namensraum-Suche (DOM §4.4 „locate a namespace") --
// Release 13.10.2026: `lookupNamespaceURI`, `lookupPrefix`,
// `isDefaultNamespace` am `Node.prototype`. GEMESSEN vorher (WPT
// dom/nodes/Node-lookupNamespaceURI 0/75, Node-lookupPrefix 0/11):
// „lookupNamespaceURI is not a function".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var N = g.Node && g.Node.prototype;
  if (!N || typeof N.lookupNamespaceURI === 'function') return;
  var XML = 'http://www.w3.org/XML/1998/namespace', XMLNS = 'http://www.w3.org/2000/xmlns/';
  function attribute(el) {
    var r = null;
    try { r = el.__narsilAttributeRoh; } catch (e) {}
    if (!r || typeof r.length !== 'number') r = el.attributes || [];
    var l = [];
    for (var i = 0; i < r.length && i < 65536; i++) {
      var a = r[i];
      if (!a) continue;
      var ns = a.namespaceURI || null, q = String(a.name), ln = a.localName ? String(a.localName) : q, p = null;
      if (a.prefix !== undefined) p = a.prefix || null;
      else if (ns !== null && q.indexOf(':') > 0) p = q.slice(0, q.indexOf(':'));
      l.push({ ns: ns, p: p, ln: ln, w: a.value === null || a.value === undefined ? '' : String(a.value) });
    }
    return l;
  }
  function elternElement(n) { var p = n.parentNode; return p && p.nodeType === 1 ? p : null; }
  function besitzer(a) { try { return a.ownerElement || null; } catch (e) { return null; } }
  function suchen(n, praefix) {
    for (var tiefe = 0; n && tiefe < 100000; tiefe++) {
      switch (n.nodeType) {
        case 1: {
          if (praefix === 'xml') return XML;
          if (praefix === 'xmlns') return XMLNS;
          var ns = n.namespaceURI || null;
          if (ns !== null && (n.prefix || null) === praefix) return ns;
          var l = attribute(n);
          for (var i = 0; i < l.length; i++) {
            var a = l[i];
            if (a.ns !== XMLNS) continue;
            if ((a.p === 'xmlns' && a.ln === praefix) || (praefix === null && a.p === null && a.ln === 'xmlns')) return a.w === '' ? null : a.w;
          }
          n = elternElement(n);
          continue;
        }
        case 9: n = n.documentElement; continue;
        case 10: case 11: return null;
        case 2: n = besitzer(n); continue;
        default: n = elternElement(n); continue;
      }
    }
    return null;
  }
  function praefixSuchen(el, ns) {
    for (var tiefe = 0; el && tiefe < 100000; tiefe++) {
      if ((el.namespaceURI || null) === ns && el.prefix) return el.prefix;
      var l = attribute(el);
      for (var i = 0; i < l.length; i++) if (l[i].p === 'xmlns' && l[i].w === ns) return l[i].ln;
      el = elternElement(el);
    }
    return null;
  }
  function def(n, f) { try { Object.defineProperty(N, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} }
  function textOderNull(v) { return v === null || v === undefined ? null : String(v); }
  def('lookupNamespaceURI', function lookupNamespaceURI(praefix) {
    var p = textOderNull(praefix);
    if (p === '') p = null;
    return suchen(this, p);
  });
  def('isDefaultNamespace', function isDefaultNamespace(ns) {
    var n = textOderNull(ns);
    if (n === '') n = null;
    return suchen(this, null) === n;
  });
  def('lookupPrefix', function lookupPrefix(ns) {
    var n = textOderNull(ns);
    if (n === null || n === '') return null;
    switch (this.nodeType) {
      case 1: return praefixSuchen(this, n);
      case 9: return this.documentElement ? praefixSuchen(this.documentElement, n) : null;
      case 10: case 11: return null;
      case 2: { var b = besitzer(this); return b ? praefixSuchen(b, n) : null; }
      default: { var e = elternElement(this); return e ? praefixSuchen(e, n) : null; }
    }
  });
})();

// -- Formular-Ergaenzungen: autocomplete, labels, Zuruecksetzen --
// Release 13.10.2026: HTML §4.10.18.7.1 „autofill" (IDL-Wert von
// `autocomplete`), `form.autocomplete` (on/off), `labels` (§4.10.4) und das
// Zuruecksetzen der normalen Felder (§4.10.21.5 „reset"). GEMESSEN vorher
// (WPT forms): form-autocomplete 0/67, reset-form 0/12, labelable-elements
// 18/26.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var HTML = 'http://www.w3.org/1999/xhtml';
  function eig(P, n, lesen, schreiben) {
    if (!P) return;
    try { Object.defineProperty(P, n, { get: lesen, set: schreiben, configurable: true, enumerable: true }); } catch (e) {}
  }
  var NORMAL = ['name', 'honorific-prefix', 'given-name', 'additional-name', 'family-name', 'honorific-suffix',
    'nickname', 'username', 'new-password', 'current-password', 'one-time-code', 'organization-title',
    'organization', 'street-address', 'address-line1', 'address-line2', 'address-line3', 'address-level4',
    'address-level3', 'address-level2', 'address-level1', 'country', 'country-name', 'postal-code', 'cc-name',
    'cc-given-name', 'cc-additional-name', 'cc-family-name', 'cc-number', 'cc-exp', 'cc-exp-month',
    'cc-exp-year', 'cc-csc', 'cc-type', 'transaction-currency', 'transaction-amount', 'language', 'bday',
    'bday-day', 'bday-month', 'bday-year', 'sex', 'url', 'photo'];
  var KONTAKT = ['tel', 'tel-country-code', 'tel-national', 'tel-area-code', 'tel-local', 'tel-local-prefix',
    'tel-local-suffix', 'tel-extension', 'email', 'impp'];
  function autofill(el) {
    var roh = el.getAttribute('autocomplete');
    if (roh === null) return '';
    var t = String(roh).toLowerCase().split(/[ \t\n\f\r]+/).filter(function (x) { return x; });
    if (!t.length) return '';
    var versteckt = el.localName === 'input' && String(el.getAttribute('type') || '').toLowerCase() === 'hidden';
    if (t.length === 1 && (t[0] === 'on' || t[0] === 'off')) return versteckt ? '' : t[0];
    var i = t.length - 1, webauthn = false;
    if (t[i] === 'webauthn') {
      webauthn = true;
      if (i === 0) return 'webauthn';
      i--;
    }
    var feld = t[i], kontakt = KONTAKT.indexOf(feld) >= 0;
    if (!kontakt && NORMAL.indexOf(feld) < 0) return '';
    var max = (kontakt ? 4 : 3) + (webauthn ? 1 : 0);
    if (t.length > max) return '';
    var raus = [feld];
    i--;
    if (i >= 0 && kontakt && ['home', 'work', 'mobile', 'fax', 'pager'].indexOf(t[i]) >= 0) { raus.unshift(t[i]); i--; }
    if (i >= 0 && (t[i] === 'shipping' || t[i] === 'billing')) { raus.unshift(t[i]); i--; }
    if (i >= 0 && t[i].indexOf('section-') === 0) { raus.unshift(t[i]); i--; }
    if (i >= 0) return '';
    if (webauthn) raus.push('webauthn');
    return raus.join(' ');
  }
  ['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement'].forEach(function (n) {
    var P = g[n] && g[n].prototype;
    eig(P, 'autocomplete', function () { return autofill(this); }, function (v) { this.setAttribute('autocomplete', String(v)); });
  });
  var FP = g.HTMLFormElement && g.HTMLFormElement.prototype;
  eig(FP, 'autocomplete', function () {
    var v = String(this.getAttribute('autocomplete') || '').toLowerCase();
    return v === 'off' ? 'off' : 'on';
  }, function (v) { this.setAttribute('autocomplete', String(v)); });

  // labels: alle <label>, deren Steuerelement dieses Element ist.
  function beschriftbar(el) {
    var n = el.localName;
    if (el.namespaceURI !== HTML) return false;
    if (['button', 'meter', 'output', 'progress', 'select', 'textarea'].indexOf(n) >= 0) return true;
    return n === 'input' && String(el.getAttribute('type') || '').toLowerCase() !== 'hidden';
  }
  function steuerVon(label) {
    var f = label.getAttribute('for');
    if (f !== null) {
      var r = label.getRootNode ? label.getRootNode() : label.ownerDocument;
      var z = r && r.getElementById ? r.getElementById(f) : (r && r.querySelector ? r.querySelector('[id="' + String(f).replace(/["\\]/g, '\\$&') + '"]') : null);
      return z && beschriftbar(z) ? z : null;
    }
    var alle = label.querySelectorAll('*');
    for (var i = 0; i < alle.length; i++) if (beschriftbar(alle[i])) return alle[i];
    return null;
  }
  ['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement', 'HTMLButtonElement', 'HTMLMeterElement',
    'HTMLOutputElement', 'HTMLProgressElement'].forEach(function (n) {
    var P = g[n] && g[n].prototype;
    if (!P) return;
    eig(P, 'labels', function () {
      if (!beschriftbar(this)) return null;
      var r = this.getRootNode ? this.getRootNode() : this.ownerDocument;
      var l = r && r.querySelectorAll ? r.querySelectorAll('label') : [];
      var raus = [];
      for (var i = 0; i < l.length; i++) if (steuerVon(l[i]) === this) raus.push(l[i]);
      if (g.NodeList && g.NodeList.prototype) { try { Object.setPrototypeOf(raus, g.NodeList.prototype); } catch (e) {} }
      return raus;
    });
  });

  // HTML §4.10.12: `output.value`/`defaultValue` mit der „default value
  // override" (null, solange niemand `value` setzte).
  var OP = g.HTMLOutputElement && g.HTMLOutputElement.prototype, ueber = new WeakMap();
  if (OP) {
    eig(OP, 'defaultValue', function () { return ueber.has(this) ? ueber.get(this) : this.textContent; },
      function (v) { v = String(v); if (ueber.has(this)) ueber.set(this, v); else this.textContent = v; });
    eig(OP, 'value', function () { return this.textContent; },
      function (v) { if (!ueber.has(this)) ueber.set(this, this.textContent); this.textContent = String(v); });
    try {
      Object.defineProperty(g, '__narsilOutputZurueck', {
        value: function (el) { if (ueber.has(el)) { var d = ueber.get(el); ueber.delete(el); el.textContent = d; } },
        configurable: true, writable: true, enumerable: false
      });
    } catch (e) {}
  }
  // Die normalen Felder eines Formulars zuruecksetzen (nach dem `reset`).
  try {
    Object.defineProperty(g, '__narsilFormZuruecksetzen', {
      value: function (form) {
        var l = form.elements || [];
        for (var i = 0; i < l.length; i++) {
          var el = l[i], n = el.localName;
          try {
            var sauber = typeof g.__narsilFeldSauber === 'function';
            if (n === 'input') {
              var t = String(el.getAttribute('type') || '').toLowerCase();
              if (t === 'checkbox' || t === 'radio') el.checked = el.hasAttribute('checked');
              else if (t === 'file') el.value = '';
              else if (sauber) g.__narsilFeldSauber(el);
              else el.value = el.getAttribute('value') || '';
            } else if (n === 'textarea') {
              if (sauber) g.__narsilFeldSauber(el); else el.value = el.textContent;
            } else if (n === 'select') {
              var o = el.options || [], gewaehlt = false;
              for (var j = 0; j < o.length; j++) { o[j].selected = o[j].hasAttribute('selected'); if (o[j].selected) gewaehlt = true; }
              if (!gewaehlt && !el.multiple && !(Number(el.getAttribute('size')) > 1)) {
                for (var k = 0; k < o.length; k++) if (!o[k].disabled) { o[k].selected = true; break; }
              }
            } else if (n === 'output' && typeof g.__narsilOutputZurueck === 'function') {
              g.__narsilOutputZurueck(el);
            }
          } catch (e) {}
        }
      },
      configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
})();

// -- HTML partial updates: {append,prepend,before,after,replaceWith}HTML --
// Release 2026-10-14: the positional methods of "declarative partial
// updates" (WICG, WPT domparsing/tentative). The markup is parsed in context
// (the parent element or the element itself; at a shadow root its host).
// The safe variant removes unsafe content (scripts, frames, objects, event
// handler attributes); both variants apply `sanitizer.removeElements`; only
// `...Unsafe` with `runScripts` lets scripts run.
// MEASURED before (WPT positional-methods 6/378): "appendHTML is not a
// function".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var E = g.Element && g.Element.prototype, CD = g.CharacterData && g.CharacterData.prototype;
  var SR = g.ShadowRoot && g.ShadowRoot.prototype;
  if (!doc || !E || typeof E.appendHTML === 'function') return;
  var HTML = 'http://www.w3.org/1999/xhtml';
  var UNSAFE = { script: 1, frame: 1, iframe: 1, object: 1, embed: 1 };
  function fail(name, text) { throw new g.DOMException(text, name); }
  function isShadowRoot(n) { return !!n && n.nodeType === 11 && !!n.host; }
  // The template element owning a contents fragment (host), else null.
  function templateHost(n) {
    try { var h = n.__narsilTemplateHost; return h && h.nodeType === 1 ? h : null; } catch (e) { return null; }
  }
  function isTemplate(n) { return !!n && n.nodeType === 1 && n.localName === 'template' && n.namespaceURI === HTML; }
  function detach(n) { if (n.parentNode) n.parentNode.removeChild(n); }
  // The names from `sanitizer.removeElements` (strings or {name, namespace}).
  function removalList(opt) {
    var s = opt && opt.sanitizer, l = s && (s.removeElements || s.blockElements), out = [];
    if (!l || typeof l.length !== 'number') return out;
    for (var i = 0; i < l.length && i < 4096; i++) {
      var x = l[i];
      if (typeof x === 'string') out.push({ name: x, ns: HTML });
      else if (x && typeof x === 'object') out.push({ name: String(x.name), ns: x.namespace === undefined ? HTML : x.namespace });
    }
    return out;
  }
  function sanitize(root, safe, removals) {
    var all = [];
    (function walk(n, depth) {
      for (var c = n.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 1) { all.push(c); if (depth < 512) walk(c.localName === 'template' && c.content ? c.content : c, depth + 1); }
      }
    })(root, 0);
    for (var i = 0; i < all.length; i++) {
      var el = all[i], ln = el.localName, ns = el.namespaceURI;
      var hit = false;
      for (var j = 0; j < removals.length; j++) if (removals[j].name === ln && (removals[j].ns || null) === (ns || null)) { hit = true; break; }
      if (safe && UNSAFE[ln] === 1 && (ns === HTML || (ln === 'script' && ns === 'http://www.w3.org/2000/svg'))) hit = true;
      if (hit) { detach(el); continue; }
      if (safe) {
        var names = el.getAttributeNames ? el.getAttributeNames() : [];
        for (var k = 0; k < names.length; k++) {
          if (/^on/i.test(names[k])) el.removeAttribute(names[k]);
        }
      }
    }
  }
  // Scripts that should run become fresh elements (the ones from the
  // fragment parser are "already started").
  function reviveScripts(f, ownerDoc) {
    var scripts = f.querySelectorAll ? f.querySelectorAll('script') : [];
    Array.prototype.forEach.call(scripts, function (old) {
      var fresh = ownerDoc.createElementNS(old.namespaceURI || HTML, old.localName);
      var names = old.getAttributeNames();
      for (var i = 0; i < names.length; i++) fresh.setAttribute(names[i], old.getAttribute(names[i]));
      fresh.textContent = old.textContent;
      old.parentNode.replaceChild(fresh, old);
    });
  }
  function parse(context, html, safe, opt, ownerDoc) {
    var helper;
    if (!context || context.nodeType !== 1 || (context.localName === 'html' && context.namespaceURI === HTML)) helper = ownerDoc.createElementNS(HTML, 'body');
    else helper = context.namespaceURI ? ownerDoc.createElementNS(context.namespaceURI, context.localName) : ownerDoc.createElement(context.localName);
    helper.innerHTML = String(html);
    var source = isTemplate(helper) && helper.content ? helper.content : helper;
    var f = ownerDoc.createDocumentFragment();
    while (source.firstChild) f.appendChild(source.firstChild);
    if (typeof g.__narsilSanitize === 'function') g.__narsilSanitize(f, safe, opt && opt.sanitizer);
    else sanitize(f, safe, removalList(opt));
    if (!safe && opt && opt.runScripts) reviveScripts(f, ownerDoc);
    return f;
  }
  function options(o) { return o && typeof o === 'object' ? o : {}; }
  function def(P, n, f) { if (P) { try { Object.defineProperty(P, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} } }
  // append/prepend: on Element (a template: its content) and ShadowRoot.
  ['append', 'prepend'].forEach(function (pos) {
    [false, true].forEach(function (unsafe) {
      var name = pos + 'HTML' + (unsafe ? 'Unsafe' : '');
      var f = function (html, opt) {
        opt = options(opt);
        var target = this, context = this;
        if (isTemplate(this) && this.content) target = this.content;
        if (isShadowRoot(this)) context = this.host;
        var ownerDoc = this.ownerDocument || doc;
        target[pos](parse(context, html, !unsafe, opt, ownerDoc));
      };
      def(E, name, f);
      def(SR, name, f);
    });
  });
  // Document.parseHTML(html, options) (HTML §8.5): parseHTMLUnsafe, then the
  // safe sanitization like the positional methods.
  // MEASURED before (WPT parsed-document-origin): "parseHTML is not a function".
  var D = g.Document;
  if (D && typeof D.parseHTML !== 'function' && typeof D.parseHTMLUnsafe === 'function') {
    def(D, 'parseHTML', function parseHTML(html, opt) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'parseHTML' on 'Document': 1 argument required, but only 0 present.");
      var parsed = D.parseHTMLUnsafe(String(html));
      if (typeof g.__narsilSanitize === 'function') g.__narsilSanitize(parsed, true, options(opt).sanitizer);
      else sanitize(parsed, true, removalList(options(opt)));
      return parsed;
    });
  }
  // before/after/replaceWith: on Element and CharacterData.
  ['before', 'after', 'replaceWith'].forEach(function (pos) {
    [false, true].forEach(function (unsafe) {
      var name = pos + 'HTML' + (unsafe ? 'Unsafe' : '');
      var f = function (html, opt) {
        opt = options(opt);
        var parent = this.parentNode;
        if (!parent) return;
        // A document, a template element itself (its children are not its
        // contents) and a plain fragment cannot take parsed markup; a
        // template's contents use the template as context.
        var context = parent;
        if (parent.nodeType === 11) context = isShadowRoot(parent) ? parent.host : templateHost(parent);
        if (!context || parent.nodeType === 9 || isTemplate(parent)) fail('HierarchyRequestError', "Failed to execute '" + name + "': the parent cannot take parsed markup.");
        var ownerDoc = this.ownerDocument || doc;
        this[pos](parse(context, html, !unsafe, opt, ownerDoc));
      };
      def(E, name, f);
      def(CD, name, f);
    });
  });
})();

// -- reportError (HTML §8.1.4.6 "report an exception") --
// Release 2026-10-14: an "error" ErrorEvent at the global object (cancelable;
// not canceled: the console). MEASURED before (WPT dom/observable): errors
// reported by Subscriber.error() after close never reached
// `self.addEventListener("error")`.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.reportError === 'function' || typeof g.dispatchEvent !== 'function') return;
  var reportError = function reportError(e) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'reportError' on 'Window': 1 argument required, but only 0 present.");
    var message = '';
    try { message = e && typeof e === 'object' && 'message' in e ? String(e.message) : String(e); } catch (x) {}
    var ev;
    try { ev = new g.ErrorEvent('error', { message: message, error: e, cancelable: true }); } catch (x) { ev = null; }
    // The native ErrorEvent may ignore its init dictionary.
    if (ev) {
      // Location from the error's stack ("at f (file:line:col)").
      var line = 0, col = 0, file = '';
      try {
        var m = /\(?([^\s()]*):(\d+):(\d+)\)?/.exec(String(e && e.stack || ''));
        if (m) { line = Number(m[2]); col = Number(m[3]); file = m[1] && m[1] !== '<anonymous>' ? m[1] : ''; }
      } catch (x) {}
      if (!file) { try { file = String(g.location.href); } catch (x) {} }
      var fields = { message: message, error: e, filename: file, lineno: line, colno: col };
      for (var k in fields) {
        if (ev[k] !== fields[k]) {
          try { Object.defineProperty(ev, k, { value: fields[k], configurable: true, enumerable: true }); } catch (x) {}
        }
      }
    }
    var notCanceled = true;
    // Release 14.10.2026: the `onerror` event handler of a global object is
    // called with (message, filename, lineno, colno, error), and a return
    // value of true cancels (HTML §8.1.8.1 „the event handler processing
    // algorithm", special error event handling). MEASURED before (WPT
    // custom-elements Document-createElement): `window.onerror` got the
    // event as its only argument, `error` was undefined.
    // The handler lives on `window` (the host object), not on globalThis.
    var W = g.window && g.window !== g ? g.window : g;
    var handler = null;
    try { handler = W.onerror; } catch (x) {}
    if (typeof handler === 'function') { try { W.onerror = null; } catch (x) {} }
    try {
      if (ev) { try { notCanceled = g.dispatchEvent(ev); } catch (x) {} }
    } finally {
      if (typeof handler === 'function') { try { W.onerror = handler; } catch (x) {} }
    }
    if (typeof handler === 'function') {
      var res;
      try {
        res = handler.call(W, ev ? ev.message : message, ev ? ev.filename : '', ev ? ev.lineno : 0, ev ? ev.colno : 0, e);
      } catch (x) {}
      if (res === true) notCanceled = false;
    }
    if (notCanceled) { try { g.console.error('Uncaught', e); } catch (x) {} }
  };
  try { Object.defineProperty(g, 'reportError', { value: reportError, configurable: true, writable: true, enumerable: true }); } catch (e) { g.reportError = reportError; }
})();

// -- Observable (WICG Observable, WPT dom/observable/tentative) --
// Release 2026-10-14: Observable, Subscriber and EventTarget.when() after
// the WICG draft: a producer shared by concurrent subscribers (the "weak
// subscriber"), subscription signals, teardowns in reverse order, the
// operators (takeUntil, map, filter, take, drop, flatMap, switchMap,
// inspect, catch, finally) and the promise-returning ones (toArray,
// forEach, every, first, last, find, some, reduce), and Observable.from()
// for observables, async iterables, iterables and promises.
// MEASURED before (WPT dom/observable 0/251): "'Observable' is not defined".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.Observable === 'function') return;
  var AC = g.AbortController, AS = g.AbortSignal;
  if (typeof AC !== 'function' || typeof AS !== 'function') return;
  var obsState = new WeakMap(), subState = new WeakMap();
  var addAbortAlgorithm = typeof g.__narsilAbortAlgorithm === 'function' ? g.__narsilAbortAlgorithm : function (signal, fn) {
    var h = function () { fn(); };
    signal.addEventListener('abort', h);
    return function () { signal.removeEventListener('abort', h); };
  };
  function report(e) {
    try { if (typeof g.reportError === 'function') { g.reportError(e); return; } } catch (x) {}
    g.setTimeout(function () { throw e; }, 0);
  }
  function isCallable(f) { return typeof f === 'function'; }
  function defMethod(P, name, f) {
    try { Object.defineProperty(P, name, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {}
  }
  function defGetter(P, name, get) {
    try { Object.defineProperty(P, name, { get: get, configurable: true, enumerable: true }); } catch (e) {}
  }
  function tag(P, name) {
    try { Object.defineProperty(P, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
  }

  // ---- Subscriber ----
  var Subscriber = function Subscriber() { throw new TypeError('Illegal constructor'); };
  function newSubscriber() {
    var s = Object.create(Subscriber.prototype);
    var controller = new AC();
    subState.set(s, { active: true, observers: [], teardowns: [], controller: controller, signal: controller.signal });
    return s;
  }
  function sub(s, what) {
    var st = subState.get(s);
    if (!st) throw new TypeError("Failed to execute '" + what + "' on 'Subscriber': Illegal invocation");
    return st;
  }
  // "close a subscription": inactive, abort the controller, teardowns in
  // reverse insertion order.
  function closeSubscription(s, hasReason, reason) {
    var st = subState.get(s);
    if (!st || !st.active) return;
    st.active = false;
    // Errors from abort algorithms propagate (they come out of abort()).
    var failure = null;
    try { st.controller.abort(hasReason ? reason : undefined); } catch (e) { failure = { error: e }; }
    var t = st.teardowns.slice();
    st.teardowns.length = 0;
    for (var i = t.length - 1; i >= 0; i--) {
      try { t[i].call(undefined); } catch (e) { report(e); }
    }
    if (failure) throw failure.error;
  }
  defMethod(Subscriber.prototype, 'next', function next(value) {
    var st = sub(this, 'next');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'next' on 'Subscriber': 1 argument required, but only 0 present.");
    if (!st.active) return;
    var obs = st.observers.slice();
    for (var i = 0; i < obs.length; i++) obs[i].next(value);
  });
  defMethod(Subscriber.prototype, 'error', function error(err) {
    var st = sub(this, 'error');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'error' on 'Subscriber': 1 argument required, but only 0 present.");
    if (!st.active) { report(err); return; }
    closeSubscription(this, true, err);
    var obs = st.observers.slice();
    for (var i = 0; i < obs.length; i++) obs[i].error(err);
  });
  defMethod(Subscriber.prototype, 'complete', function complete() {
    var st = sub(this, 'complete');
    if (!st.active) return;
    closeSubscription(this, false);
    var obs = st.observers.slice();
    for (var i = 0; i < obs.length; i++) obs[i].complete();
  });
  defMethod(Subscriber.prototype, 'addTeardown', function addTeardown(teardown) {
    var st = sub(this, 'addTeardown');
    if (!isCallable(teardown)) throw new TypeError("Failed to execute 'addTeardown' on 'Subscriber': The callback provided as parameter 1 is not a function.");
    if (st.active) st.teardowns.push(teardown);
    else { try { teardown.call(undefined); } catch (e) { report(e); } }
  });
  defGetter(Subscriber.prototype, 'active', function () { return sub(this, 'active').active; });
  defGetter(Subscriber.prototype, 'signal', function () { return sub(this, 'signal').signal; });
  tag(Subscriber.prototype, 'Subscriber');

  // ---- Observable ----
  var Observable = function Observable(callback) {
    if (!(this instanceof Observable) || obsState.has(this)) throw new TypeError("Failed to construct 'Observable': Please use the 'new' operator.");
    if (!isCallable(callback)) throw new TypeError("Failed to construct 'Observable': parameter 1 is not of type 'Function'.");
    obsState.set(this, { callback: callback, native: null, weak: null });
  };
  function nativeObservable(steps) {
    var o = Object.create(Observable.prototype);
    obsState.set(o, { callback: null, native: steps, weak: null });
    return o;
  }
  function obs(o, what) {
    var st = obsState.get(o);
    if (!st) throw new TypeError("Failed to execute '" + what + "' on 'Observable': Illegal invocation");
    return st;
  }
  // An internal observer from the subscribe() argument (callback or
  // SubscriptionObserver dictionary); user callbacks report their errors.
  function internalObserver(arg) {
    if (arg && arg.__internal === true) return arg;
    if (isCallable(arg)) {
      return { next: function (v) { try { arg.call(undefined, v); } catch (e) { report(e); } }, error: report, complete: function () {} };
    }
    if (arg !== undefined && arg !== null && typeof arg !== 'object') throw new TypeError("Failed to execute 'subscribe' on 'Observable': The provided value is not of type 'SubscriptionObserver'.");
    var complete, error, next;
    if (arg) {
      complete = arg.complete; error = arg.error; next = arg.next;
      if (complete !== undefined && !isCallable(complete)) throw new TypeError("Failed to execute 'subscribe' on 'Observable': The 'complete' member is not a function.");
      if (error !== undefined && !isCallable(error)) throw new TypeError("Failed to execute 'subscribe' on 'Observable': The 'error' member is not a function.");
      if (next !== undefined && !isCallable(next)) throw new TypeError("Failed to execute 'subscribe' on 'Observable': The 'next' member is not a function.");
    }
    return {
      next: function (v) { if (next) { try { next.call(arg, v); } catch (e) { report(e); } } },
      error: function (e) { if (error) { try { error.call(arg, e); } catch (x) { report(x); } } else report(e); },
      complete: function () { if (complete) { try { complete.call(arg); } catch (e) { report(e); } } }
    };
  }
  function signalFrom(options, what) {
    if (options === undefined || options === null) return null;
    if (typeof options !== 'object') throw new TypeError("Failed to execute '" + what + "': The provided value is not of type 'SubscribeOptions'.");
    var s = options.signal;
    if (s === undefined) return null;
    if (!(s instanceof AS)) throw new TypeError("Failed to execute '" + what + "': Failed to read the 'signal' property: The provided value is not of type 'AbortSignal'.");
    return s;
  }
  function removeObserver(st, internal) {
    var i = st.observers.indexOf(internal);
    if (i >= 0) st.observers.splice(i, 1);
  }
  function watchSignal(subscriber, st, internal, signal) {
    addAbortAlgorithm(signal, function () {
      if (!st.active) return;
      removeObserver(st, internal);
      if (st.observers.length === 0) closeSubscription(subscriber, true, signal.reason);
    });
  }
  // "subscribe to an Observable".
  function subscribeTo(o, observerArg, signal) {
    var ost = obsState.get(o);
    var internal = internalObserver(observerArg);
    var ws = ost.weak, wst = ws && subState.get(ws);
    if (wst && wst.active) {
      wst.observers.push(internal);
      if (signal) {
        if (signal.aborted) removeObserver(wst, internal);
        else watchSignal(ws, wst, internal, signal);
      }
      return;
    }
    var s = newSubscriber(), st = subState.get(s);
    st.observers.push(internal);
    ost.weak = s;
    if (signal) {
      if (signal.aborted) closeSubscription(s, true, signal.reason);
      else watchSignal(s, st, internal, signal);
    }
    if (ost.callback) {
      try { ost.callback.call(undefined, s); } catch (e) { s.error(e); }
    } else {
      ost.native(s);
    }
  }
  defMethod(Observable.prototype, 'subscribe', function subscribe(observer, options) {
    obs(this, 'subscribe');
    subscribeTo(this, observer, signalFrom(options, 'subscribe'));
  });
  tag(Observable.prototype, 'Observable');
  function forward(s) {
    return {
      __internal: true,
      next: function (v) { s.next(v); },
      error: function (e) { s.error(e); },
      complete: function () { s.complete(); }
    };
  }
  function observer(next, error, complete) {
    return { __internal: true, next: next, error: error, complete: complete };
  }

  // ---- Observable.from ----
  // Iterator methods are called through Reflect.apply: the engine's native
  // generator methods have no `.call`.
  var apply = typeof Reflect === 'object' && typeof Reflect.apply === 'function'
    ? Reflect.apply
    : function (f, t, a) { return Function.prototype.apply.call(f, t, a); };
  function isObject(x) { return x !== null && (typeof x === 'object' || typeof x === 'function'); }
  // An async iterator record; without @@asyncIterator the sync iterator is
  // wrapped (CreateAsyncFromSyncIterator).
  function asyncIteratorRecord(value) {
    var m = value[Symbol.asyncIterator];
    if (m === undefined || m === null) {
      var sm = value[Symbol.iterator];
      if (!isCallable(sm)) throw new TypeError('The object is not iterable');
      var sit = apply(sm, value, []);
      if (!isObject(sit)) throw new TypeError('The iterator is not an object');
      var snext = sit.next;
      return {
        it: sit,
        next: function () {
          var r = apply(snext, sit, []);
          if (!isObject(r)) throw new TypeError('The iterator result is not an object');
          var done = r.done;
          return Promise.resolve(r.value).then(function (v) { return { done: done, value: v }; });
        },
        close: function (reason) {
          var ret = sit['return'];
          if (ret === undefined || ret === null) return;
          return apply(ret, sit, [reason]);
        }
      };
    }
    if (!isCallable(m)) throw new TypeError('@@asyncIterator is not callable');
    var it = apply(m, value, []);
    if (!isObject(it)) throw new TypeError('The iterator is not an object');
    return { it: it, nextGetter: true, close: function (reason) {
      var ret = it['return'];
      if (ret === undefined || ret === null) return;
      return apply(ret, it, [reason]);
    } };
  }
  function fromValue(value) {
    if (!isObject(value)) throw new TypeError("Failed to execute 'from' on 'Observable': The provided value cannot be converted to an Observable.");
    if (obsState.has(value)) return value;
    var asyncMethod = Symbol.asyncIterator ? value[Symbol.asyncIterator] : undefined;
    if (asyncMethod !== undefined && asyncMethod !== null) {
      if (!isCallable(asyncMethod)) throw new TypeError("Failed to execute 'from' on 'Observable': @@asyncIterator is not callable.");
      return nativeObservable(function (s) {
        var signal = s.signal;
        if (signal.aborted) return;
        var rec;
        try { rec = asyncIteratorRecord(value); } catch (e) { s.error(e); return; }
        var nextFn = null, nextError = null;
        if (rec.nextGetter) {
          try { nextFn = rec.it.next; } catch (e) { nextError = { error: e }; }
        }
        if (signal.aborted) return;
        var finished = false;
        addAbortAlgorithm(signal, function () {
          if (finished) return;
          var r;
          try { r = rec.close(signal.reason); } catch (e) { Promise.reject(e).then(null, function () {}); return; }
          if (r !== undefined) Promise.resolve(r).then(null, function () {});
        });
        (function pull() {
          var p;
          try {
            if (nextError) throw nextError.error;
            p = rec.nextGetter ? Promise.resolve(apply(nextFn, rec.it, [])) : rec.next();
          } catch (e) { p = Promise.reject(e); }
          p.then(function (res) {
            if (!isObject(res)) { finished = true; s.error(new TypeError('The iterator result is not an object')); return; }
            var done, v;
            try { done = res.done; } catch (e) { finished = true; s.error(e); return; }
            if (done) { finished = true; s.complete(); return; }
            try { v = res.value; } catch (e) { finished = true; s.error(e); return; }
            s.next(v);
            if (!signal.aborted && s.active) pull();
          }, function (r) { finished = true; s.error(r); });
        })();
      });
    }
    var iterMethod = value[Symbol.iterator];
    if (iterMethod !== undefined && iterMethod !== null) {
      if (!isCallable(iterMethod)) throw new TypeError("Failed to execute 'from' on 'Observable': @@iterator is not callable.");
      return nativeObservable(function (s) {
        var signal = s.signal;
        if (signal.aborted) return;
        var it, nextFn;
        try {
          var m = value[Symbol.iterator];
          if (!isCallable(m)) throw new TypeError('@@iterator is not callable');
          it = apply(m, value, []);
          if (!isObject(it)) throw new TypeError('The iterator is not an object');
          nextFn = it.next;
        } catch (e) { s.error(e); return; }
        if (signal.aborted) return;
        var finished = false;
        addAbortAlgorithm(signal, function () {
          if (finished) return;
          var r = it['return'];
          if (r === undefined || r === null) return;
          var res = apply(r, it, []);
          if (!isObject(res)) throw new TypeError('Iterator#return() must return an Object');
        });
        for (var guard = 0; guard < 100000000; guard++) {
          var res, done, v;
          try {
            res = apply(nextFn, it, []);
            if (!isObject(res)) throw new TypeError('The iterator result is not an object');
            done = res.done;
            if (!done) v = res.value;
          } catch (e) { finished = true; s.error(e); return; }
          if (done) { finished = true; s.complete(); return; }
          s.next(v);
          if (signal.aborted) return;
        }
      });
    }
    if (value instanceof Promise) {
      return nativeObservable(function (s) {
        Promise.prototype.then.call(value, function (v) { s.next(v); s.complete(); }, function (r) { s.error(r); });
      });
    }
    throw new TypeError("Failed to execute 'from' on 'Observable': The provided value cannot be converted to an Observable.");
  }
  defMethod(Observable, 'from', function from(value) { return fromValue(value); });

  // ---- operators returning Observables ----
  function source(o, what) { obs(o, what); return o; }
  function callback(f, what) {
    if (!isCallable(f)) throw new TypeError("Failed to execute '" + what + "' on 'Observable': The callback provided as parameter 1 is not a function.");
    return f;
  }
  // WebIDL `unsigned long long` (no [EnforceRange]): NaN/Infinity give 0,
  // negative values wrap modulo 2^64, i.e. to a huge count.
  function count(n) {
    var x = Number(n);
    if (!isFinite(x)) return 0;
    x = x < 0 ? Math.ceil(x) : Math.floor(x);
    if (x < 0) return Number.MAX_SAFE_INTEGER;
    return x;
  }
  defMethod(Observable.prototype, 'takeUntil', function takeUntil(value) {
    var src = source(this, 'takeUntil');
    var notifier = fromValue(value);
    return nativeObservable(function (s) {
      subscribeTo(notifier, observer(function () { s.complete(); }, function () { s.complete(); }, function () {}), s.signal);
      if (!s.active) return;
      subscribeTo(src, forward(s), s.signal);
    });
  });
  defMethod(Observable.prototype, 'map', function map(mapper) {
    var src = source(this, 'map'); callback(mapper, 'map');
    return nativeObservable(function (s) {
      var idx = 0;
      subscribeTo(src, observer(function (v) {
        var m;
        try { m = mapper.call(undefined, v, idx); } catch (e) { s.error(e); return; }
        idx++;
        s.next(m);
      }, function (e) { s.error(e); }, function () { s.complete(); }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'filter', function filter(predicate) {
    var src = source(this, 'filter'); callback(predicate, 'filter');
    return nativeObservable(function (s) {
      var idx = 0;
      subscribeTo(src, observer(function (v) {
        var keep;
        try { keep = predicate.call(undefined, v, idx); } catch (e) { s.error(e); return; }
        idx++;
        if (keep) s.next(v);
      }, function (e) { s.error(e); }, function () { s.complete(); }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'take', function take(amount) {
    var src = source(this, 'take'), n = count(amount);
    return nativeObservable(function (s) {
      var remaining = n;
      if (remaining === 0) { s.complete(); return; }
      subscribeTo(src, observer(function (v) {
        s.next(v);
        remaining--;
        if (remaining === 0) s.complete();
      }, function (e) { s.error(e); }, function () { s.complete(); }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'drop', function drop(amount) {
    var src = source(this, 'drop'), n = count(amount);
    return nativeObservable(function (s) {
      var remaining = n;
      subscribeTo(src, observer(function (v) {
        if (remaining > 0) { remaining--; return; }
        s.next(v);
      }, function (e) { s.error(e); }, function () { s.complete(); }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'flatMap', function flatMap(mapper) {
    var src = source(this, 'flatMap'); callback(mapper, 'flatMap');
    return nativeObservable(function (s) {
      var st = { idx: 0, outerCompleted: false, queue: [], activeInner: false };
      function processNext(v) {
        var r, inner;
        try { r = mapper.call(undefined, v, st.idx); } catch (e) { s.error(e); return; }
        st.idx++;
        try { inner = fromValue(r); } catch (e) { s.error(e); return; }
        subscribeTo(inner, observer(function (x) { s.next(x); }, function (e) { s.error(e); }, function () {
          if (st.queue.length) processNext(st.queue.shift());
          else {
            st.activeInner = false;
            if (st.outerCompleted) s.complete();
          }
        }), s.signal);
      }
      subscribeTo(src, observer(function (v) {
        if (st.activeInner) { st.queue.push(v); return; }
        st.activeInner = true;
        processNext(v);
      }, function (e) { s.error(e); }, function () {
        st.outerCompleted = true;
        if (!st.activeInner && st.queue.length === 0) s.complete();
      }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'switchMap', function switchMap(mapper) {
    var src = source(this, 'switchMap'); callback(mapper, 'switchMap');
    return nativeObservable(function (s) {
      var st = { idx: 0, outerCompleted: false, inner: null };
      function processNext(v) {
        var r, inner;
        try { r = mapper.call(undefined, v, st.idx); } catch (e) { s.error(e); return; }
        st.idx++;
        try { inner = fromValue(r); } catch (e) { s.error(e); return; }
        var signal = AS.any([st.inner.signal, s.signal]);
        subscribeTo(inner, observer(function (x) { s.next(x); }, function (e) { s.error(e); }, function () {
          if (st.outerCompleted) s.complete();
          else st.inner = null;
        }), signal);
      }
      subscribeTo(src, observer(function (v) {
        if (st.inner) st.inner.abort();
        st.inner = new AC();
        processNext(v);
      }, function (e) { s.error(e); }, function () {
        st.outerCompleted = true;
        if (!st.inner) s.complete();
      }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'inspect', function inspect(inspector) {
    var src = source(this, 'inspect');
    var cb = {};
    if (isCallable(inspector)) cb.next = inspector;
    else if (inspector !== undefined && inspector !== null) {
      if (typeof inspector !== 'object') throw new TypeError("Failed to execute 'inspect' on 'Observable': The provided value is not of type 'ObservableInspector'.");
      ['abort', 'complete', 'error', 'next', 'subscribe'].forEach(function (k) {
        var f = inspector[k];
        if (f !== undefined && !isCallable(f)) throw new TypeError("Failed to execute 'inspect' on 'Observable': The '" + k + "' member is not a function.");
        if (f !== undefined) cb[k] = f;
      });
    }
    return nativeObservable(function (s) {
      if (cb.subscribe) {
        try { cb.subscribe.call(inspector); } catch (e) { s.error(e); return; }
      }
      var removeAbort = null;
      if (cb.abort) {
        removeAbort = addAbortAlgorithm(s.signal, function () {
          try { cb.abort.call(inspector, s.signal.reason); } catch (e) { report(e); }
        });
      }
      function dropAbort() { if (removeAbort) { removeAbort(); removeAbort = null; } }
      subscribeTo(src, observer(function (v) {
        if (cb.next) {
          try { cb.next.call(inspector === cb.next ? undefined : inspector, v); } catch (e) { dropAbort(); s.error(e); return; }
        }
        s.next(v);
      }, function (e) {
        dropAbort();
        if (cb.error) {
          try { cb.error.call(inspector, e); } catch (x) { s.error(x); return; }
        }
        s.error(e);
      }, function () {
        dropAbort();
        if (cb.complete) {
          try { cb.complete.call(inspector); } catch (x) { s.error(x); return; }
        }
        s.complete();
      }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'catch', function (handler) {
    var src = source(this, 'catch'); callback(handler, 'catch');
    return nativeObservable(function (s) {
      subscribeTo(src, observer(function (v) { s.next(v); }, function (e) {
        var r, inner;
        try { r = handler.call(undefined, e); } catch (x) { s.error(x); return; }
        try { inner = fromValue(r); } catch (x) { s.error(x); return; }
        subscribeTo(inner, forward(s), s.signal);
      }, function () { s.complete(); }), s.signal);
    });
  });
  defMethod(Observable.prototype, 'finally', function (handler) {
    var src = source(this, 'finally'); callback(handler, 'finally');
    return nativeObservable(function (s) {
      s.addTeardown(handler);
      subscribeTo(src, forward(s), s.signal);
    });
  });

  // ---- operators returning promises ----
  // The shared setup: an internal controller, a dependent signal of it and
  // options.signal, rejection on abort.
  function promiseSetup(options, what, withController) {
    var outer = signalFrom(options, what);
    var r = {};
    r.promise = new Promise(function (res, rej) { r.resolve = res; r.reject = rej; });
    r.controller = withController ? new AC() : null;
    r.signal = withController ? (outer ? AS.any([r.controller.signal, outer]) : r.controller.signal) : outer;
    if (r.signal && r.signal.aborted) { r.reject(r.signal.reason); r.done = true; return r; }
    if (r.signal) {
      var sig = r.signal;
      addAbortAlgorithm(sig, function () { r.reject(sig.reason); });
    }
    return r;
  }
  defMethod(Observable.prototype, 'toArray', function toArray(options) {
    var src = source(this, 'toArray');
    var r = promiseSetup(options, 'toArray', false);
    if (r.done) return r.promise;
    var values = [];
    subscribeTo(src, observer(function (v) { values.push(v); }, function (e) { r.reject(e); }, function () { r.resolve(values); }), r.signal);
    return r.promise;
  });
  defMethod(Observable.prototype, 'forEach', function forEach(cbFn, options) {
    var src = source(this, 'forEach'); callback(cbFn, 'forEach');
    var r = promiseSetup(options, 'forEach', true);
    if (r.done) return r.promise;
    var idx = 0;
    subscribeTo(src, observer(function (v) {
      try { cbFn.call(undefined, v, idx++); } catch (e) { r.reject(e); r.controller.abort(e); }
    }, function (e) { r.reject(e); }, function () { r.resolve(undefined); }), r.signal);
    return r.promise;
  });
  function predicateOperator(name, onMatch, onComplete) {
    defMethod(Observable.prototype, name, function (predicate, options) {
      var src = source(this, name); callback(predicate, name);
      var r = promiseSetup(options, name, true);
      if (r.done) return r.promise;
      var idx = 0;
      subscribeTo(src, observer(function (v) {
        var res;
        try { res = predicate.call(undefined, v, idx++); } catch (e) { r.reject(e); r.controller.abort(e); return; }
        onMatch(r, res, v);
      }, function (e) { r.reject(e); }, function () { onComplete(r); }), r.signal);
      return r.promise;
    });
  }
  predicateOperator('every', function (r, res) { if (!res) { r.resolve(false); r.controller.abort(); } }, function (r) { r.resolve(true); });
  predicateOperator('find', function (r, res, v) { if (res) { r.resolve(v); r.controller.abort(); } }, function (r) { r.resolve(undefined); });
  predicateOperator('some', function (r, res) { if (res) { r.resolve(true); r.controller.abort(); } }, function (r) { r.resolve(false); });
  defMethod(Observable.prototype, 'first', function first(options) {
    var src = source(this, 'first');
    var r = promiseSetup(options, 'first', true);
    if (r.done) return r.promise;
    subscribeTo(src, observer(function (v) { r.resolve(v); r.controller.abort(); }, function (e) { r.reject(e); }, function () {
      r.reject(new RangeError('No values in Observable'));
    }), r.signal);
    return r.promise;
  });
  defMethod(Observable.prototype, 'last', function last(options) {
    var src = source(this, 'last');
    var r = promiseSetup(options, 'last', false);
    if (r.done) return r.promise;
    var has = false, lastValue;
    subscribeTo(src, observer(function (v) { has = true; lastValue = v; }, function (e) { r.reject(e); }, function () {
      if (has) r.resolve(lastValue); else r.reject(new RangeError('No values in Observable'));
    }), r.signal);
    return r.promise;
  });
  defMethod(Observable.prototype, 'reduce', function reduce(reducer, initialValue, options) {
    var src = source(this, 'reduce'); callback(reducer, 'reduce');
    var r = promiseSetup(options, 'reduce', true);
    if (r.done) return r.promise;
    var hasAcc = arguments.length >= 2, acc = initialValue, idx = 0;
    subscribeTo(src, observer(function (v) {
      if (!hasAcc) { acc = v; hasAcc = true; idx++; return; }
      try { acc = reducer.call(undefined, acc, v, idx); } catch (e) { r.reject(e); r.controller.abort(e); return; }
      idx++;
    }, function (e) { r.reject(e); }, function () {
      if (hasAcc) r.resolve(acc); else r.reject(new TypeError('Reduce of an empty Observable with no initial value'));
    }), r.signal);
    return r.promise;
  });

  // ---- EventTarget.when ----
  var ET = g.EventTarget && g.EventTarget.prototype;
  if (ET) {
    defMethod(ET, 'when', function when(type, options) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'when' on 'EventTarget': 1 argument required, but only 0 present.");
      var target = this, t = String(type);
      var capture = !!(options && options.capture);
      var hasPassive = !!options && options.passive !== undefined;
      var passive = hasPassive ? !!options.passive : undefined;
      return nativeObservable(function (s) {
        if (s.signal.aborted) return;
        var opts = { capture: capture, once: false, signal: s.signal };
        if (hasPassive) opts.passive = passive;
        target.addEventListener(t, function (ev) { s.next(ev); }, opts);
      });
    });
  }
  try { Object.defineProperty(g, 'Observable', { value: Observable, configurable: true, writable: true, enumerable: false }); } catch (e) { g.Observable = Observable; }
  try { Object.defineProperty(g, 'Subscriber', { value: Subscriber, configurable: true, writable: true, enumerable: false }); } catch (e) { g.Subscriber = Subscriber; }
})();

// -- UI Events and the other Event subclasses, createEvent (DOM §2.5, §4.5) --
// Release 2026-10-15: the event interfaces with their init dictionaries
// (UI Events: UIEvent <- FocusEvent, MouseEvent <- WheelEvent/PointerEvent/
// DragEvent, KeyboardEvent, CompositionEvent, InputEvent, TouchEvent; HTML:
// CustomEvent, HashChangeEvent, PopStateEvent, PageTransitionEvent,
// ErrorEvent, ProgressEvent, ...): members converted by their IDL type, the
// prototype chain of the interface (and of a subclass via new.target);
// TextEvent, DeviceMotionEvent and DeviceOrientationEvent; createEvent() for
// every legacy interface name, its events uninitialized until init*Event().
// MEASURED before (WPT dom/events Event-subclasses-constructors 12/49,
// EventTarget-dispatchEvent 5/25): `new UIEvent('x') instanceof UIEvent`
// was false, createEvent('MouseEvent') threw NotSupportedError.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var Ev = g.Event;
  if (typeof Ev !== 'function') return;
  var uninitialized = new WeakSet();
  try { Object.defineProperty(g, '__narsilEventUninitialized', { value: function (e) { return uninitialized.has(e); }, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  function toInt(v, bits, unsigned) {
    v = Number(v);
    if (!isFinite(v)) return 0;
    v = v < 0 ? Math.ceil(v) : Math.floor(v);
    var m = Math.pow(2, bits);
    v = ((v % m) + m) % m;
    return !unsigned && v >= m / 2 ? v - m : v;
  }
  function isWindow(v) { return v === g || (v !== null && typeof v === 'object' && v.window === v); }
  function conv(kind, v, member, iface) {
    switch (kind) {
      case 'bool': return !!v;
      case 'double': case 'float':
        v = Number(v);
        if (!isFinite(v)) throw new TypeError("Failed to construct '" + iface + "': The provided double value is non-finite.");
        return v;
      case 'long': return toInt(v, 32, false);
      case 'ulong': return toInt(v, 32, true);
      case 'short': return toInt(v, 16, false);
      case 'ushort': return toInt(v, 16, true);
      case 'ulonglong': v = Number(v); return isFinite(v) && v > 0 ? Math.floor(v) : 0;
      case 'str': return String(v);
      case 'strnull': return v === null ? null : String(v);
      case 'view':
        if (v === null) return null;
        if (isWindow(v)) return v;
        throw new TypeError("Failed to construct '" + iface + "': Failed to read the '" + member + "' property from '" + iface + "Init': The provided value is not of type 'Window'.");
      case 'target':
        if (v === null) return null;
        if (v !== null && typeof v === 'object' && typeof v.addEventListener === 'function') return v;
        throw new TypeError("Failed to construct '" + iface + "': Failed to read the '" + member + "' property from '" + iface + "Init': The provided value is not of type 'EventTarget'.");
      case 'list': return v == null ? [] : Array.prototype.slice.call(v);
      default: return v;
    }
  }
  var MEMBERS = new WeakMap();
  function define(name, Parent, members, extra) {
    var all = (MEMBERS.get(Parent) || []).concat(members);
    var C = function (type, init) {
      if (!new.target) throw new TypeError("Failed to construct '" + name + "': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      if (arguments.length < 1) throw new TypeError("Failed to construct '" + name + "': 1 argument required, but only 0 present.");
      if (init !== undefined && init !== null && typeof init !== 'object' && typeof init !== 'function') {
        throw new TypeError("Failed to construct '" + name + "': The provided value is not of type '" + name + "Init'.");
      }
      var dict = init == null ? {} : init;
      var vals = [];
      for (var i = 0; i < all.length; i++) {
        var v = dict[all[i][0]];
        vals.push(v === undefined ? all[i][1] : conv(all[i][2], v, all[i][0], name));
      }
      var e = new Ev(type, dict);
      for (var j = 0; j < all.length; j++) {
        try { Object.defineProperty(e, all[j][0], { value: vals[j], enumerable: true, configurable: true, writable: false }); } catch (x) { try { e[all[j][0]] = vals[j]; } catch (y) {} }
      }
      if (extra) extra(e);
      try { Object.setPrototypeOf(e, new.target.prototype); } catch (x) {}
      return e;
    };
    C.prototype = Object.create(Parent.prototype);
    try { Object.defineProperty(C.prototype, 'constructor', { value: C, configurable: true, writable: true, enumerable: false }); } catch (e) {}
    try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    try { Object.defineProperty(C, 'name', { value: name, configurable: true }); } catch (e) {}
    try { Object.defineProperty(C, 'length', { value: 1, configurable: true }); } catch (e) {}
    try { Object.setPrototypeOf(C, Parent); } catch (e) {}
    MEMBERS.set(C, all);
    try { Object.defineProperty(g, name, { value: C, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = C; }
    return C;
  }
  try { if (!Object.prototype.hasOwnProperty.call(Ev.prototype, Symbol.toStringTag)) Object.defineProperty(Ev.prototype, Symbol.toStringTag, { value: 'Event', configurable: true }); } catch (e) {}
  var MODIFIERS = [['ctrlKey', false, 'bool'], ['shiftKey', false, 'bool'], ['altKey', false, 'bool'], ['metaKey', false, 'bool']];
  var UI = define('UIEvent', Ev, [['view', null, 'view'], ['detail', 0, 'long'], ['which', 0, 'ulong']]);
  define('FocusEvent', UI, [['relatedTarget', null, 'target']]);
  var mouseExtra = function (e) {
    try {
      Object.defineProperty(e, 'pageX', { value: e.clientX, enumerable: true, configurable: true });
      Object.defineProperty(e, 'pageY', { value: e.clientY, enumerable: true, configurable: true });
      Object.defineProperty(e, 'x', { value: e.clientX, enumerable: true, configurable: true });
      Object.defineProperty(e, 'y', { value: e.clientY, enumerable: true, configurable: true });
      Object.defineProperty(e, 'offsetX', { value: 0, enumerable: true, configurable: true });
      Object.defineProperty(e, 'offsetY', { value: 0, enumerable: true, configurable: true });
    } catch (x) {}
  };
  var Mouse = define('MouseEvent', UI, MODIFIERS.concat([['screenX', 0, 'double'], ['screenY', 0, 'double'], ['clientX', 0, 'double'],
    ['clientY', 0, 'double'], ['movementX', 0, 'double'], ['movementY', 0, 'double'], ['button', 0, 'short'], ['buttons', 0, 'ushort'],
    ['relatedTarget', null, 'target']]), mouseExtra);
  define('WheelEvent', Mouse, [['deltaX', 0, 'double'], ['deltaY', 0, 'double'], ['deltaZ', 0, 'double'], ['deltaMode', 0, 'ulong']], mouseExtra);
  define('PointerEvent', Mouse, [['pointerId', 0, 'long'], ['width', 1, 'double'], ['height', 1, 'double'], ['pressure', 0, 'float'],
    ['tangentialPressure', 0, 'float'], ['tiltX', 0, 'long'], ['tiltY', 0, 'long'], ['twist', 0, 'long'], ['altitudeAngle', Math.PI / 2, 'double'],
    ['azimuthAngle', 0, 'double'], ['pointerType', '', 'str'], ['isPrimary', false, 'bool'], ['persistentDeviceId', 0, 'long']], mouseExtra);
  define('DragEvent', Mouse, [['dataTransfer', null, 'any']], mouseExtra);
  define('KeyboardEvent', UI, MODIFIERS.concat([['key', '', 'str'], ['code', '', 'str'], ['location', 0, 'ulong'], ['repeat', false, 'bool'],
    ['isComposing', false, 'bool'], ['charCode', 0, 'ulong'], ['keyCode', 0, 'ulong']]));
  define('CompositionEvent', UI, [['data', '', 'str']]);
  define('InputEvent', UI, [['data', null, 'strnull'], ['isComposing', false, 'bool'], ['inputType', '', 'str'], ['dataTransfer', null, 'any']]);
  define('TouchEvent', UI, MODIFIERS.concat([['touches', [], 'list'], ['targetTouches', [], 'list'], ['changedTouches', [], 'list']]));
  define('CustomEvent', Ev, [['detail', null, 'any']]);
  define('ClipboardEvent', Ev, [['clipboardData', null, 'any']]);
  define('HashChangeEvent', Ev, [['oldURL', '', 'str'], ['newURL', '', 'str']]);
  define('PopStateEvent', Ev, [['state', null, 'any'], ['hasUAVisualTransition', false, 'bool']]);
  define('PageTransitionEvent', Ev, [['persisted', false, 'bool']]);
  define('ProgressEvent', Ev, [['lengthComputable', false, 'bool'], ['loaded', 0, 'ulonglong'], ['total', 0, 'ulonglong']]);
  define('ErrorEvent', Ev, [['message', '', 'str'], ['filename', '', 'str'], ['lineno', 0, 'ulong'], ['colno', 0, 'ulong'], ['error', undefined, 'any']]);
  if (typeof g.DeviceMotionEvent !== 'function') {
    define('DeviceMotionEvent', Ev, [['acceleration', null, 'any'], ['accelerationIncludingGravity', null, 'any'], ['rotationRate', null, 'any'], ['interval', 0, 'double']]);
  }
  if (typeof g.DeviceOrientationEvent !== 'function') {
    define('DeviceOrientationEvent', Ev, [['alpha', null, 'any'], ['beta', null, 'any'], ['gamma', null, 'any'], ['absolute', false, 'bool']]);
  }
  // The class name of the event interfaces made elsewhere in the prelude.
  ['MessageEvent', 'StorageEvent', 'AnimationEvent', 'TransitionEvent', 'PromiseRejectionEvent', 'SubmitEvent', 'FormDataEvent',
    'ToggleEvent', 'BeforeUnloadEvent', 'SecurityPolicyViolationEvent', 'CloseEvent', 'TrackEvent', 'MediaQueryListEvent',
    'BeforeInstallPromptEvent', 'PictureInPictureEvent', 'CommandEvent', 'NavigateEvent', 'StorageEvent'].forEach(function (n) {
    var P = typeof g[n] === 'function' && g[n].prototype;
    if (P && !Object.prototype.hasOwnProperty.call(P, Symbol.toStringTag)) {
      try { Object.defineProperty(P, Symbol.toStringTag, { value: n, configurable: true }); } catch (e) {}
    }
  });
  // TextEvent: only through createEvent (no constructor).
  if (typeof g.TextEvent !== 'function') {
    var TE = function TextEvent() { throw new TypeError('Illegal constructor'); };
    TE.prototype = Object.create(UI.prototype);
    try { Object.defineProperty(TE.prototype, 'constructor', { value: TE, configurable: true, writable: true }); } catch (e) {}
    try { Object.defineProperty(TE.prototype, Symbol.toStringTag, { value: 'TextEvent', configurable: true }); } catch (e) {}
    try { Object.setPrototypeOf(TE, UI); } catch (e) {}
    try { Object.defineProperty(g, 'TextEvent', { value: TE, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  }
  // Legacy initializers (UI Events §7, DOM §2.5).
  var EP = Ev.prototype;
  function own(e, n, v) { try { Object.defineProperty(e, n, { value: v, enumerable: true, configurable: true, writable: false }); } catch (x) {} }
  function legacy(C, name, fn) {
    if (!C || !C.prototype) return;
    try { Object.defineProperty(C.prototype, name, { value: fn, configurable: true, writable: true, enumerable: true }); } catch (e) {}
  }
  function initBase(e, a) {
    if (e.__narsilLaeuft) return false;
    if (typeof EP.initEvent === 'function') EP.initEvent.call(e, String(a[0]), !!a[1], !!a[2]);
    uninitialized.delete(e);
    return true;
  }
  legacy(g.UIEvent, 'initUIEvent', function initUIEvent(type, bubbles, cancelable, view, detail) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initUIEvent' on 'UIEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'view', view == null ? null : view); own(this, 'detail', toInt(detail, 32, false));
  });
  legacy(g.MouseEvent, 'initMouseEvent', function initMouseEvent(type, bubbles, cancelable, view, detail, screenX, screenY, clientX, clientY, ctrlKey, altKey, shiftKey, metaKey, button, relatedTarget) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initMouseEvent' on 'MouseEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'view', view == null ? null : view); own(this, 'detail', toInt(detail, 32, false));
    own(this, 'screenX', toInt(screenX, 32, false)); own(this, 'screenY', toInt(screenY, 32, false));
    own(this, 'clientX', toInt(clientX, 32, false)); own(this, 'clientY', toInt(clientY, 32, false));
    own(this, 'ctrlKey', !!ctrlKey); own(this, 'altKey', !!altKey); own(this, 'shiftKey', !!shiftKey); own(this, 'metaKey', !!metaKey);
    own(this, 'button', toInt(button, 16, false)); own(this, 'relatedTarget', relatedTarget == null ? null : relatedTarget);
  });
  legacy(g.KeyboardEvent, 'initKeyboardEvent', function initKeyboardEvent(type, bubbles, cancelable, view, key, location, ctrlKey, altKey, shiftKey, metaKey) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initKeyboardEvent' on 'KeyboardEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'view', view == null ? null : view); own(this, 'key', key === undefined ? '' : String(key)); own(this, 'location', toInt(location, 32, true));
    own(this, 'ctrlKey', !!ctrlKey); own(this, 'altKey', !!altKey); own(this, 'shiftKey', !!shiftKey); own(this, 'metaKey', !!metaKey);
  });
  legacy(g.CompositionEvent, 'initCompositionEvent', function initCompositionEvent(type, bubbles, cancelable, view, data) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initCompositionEvent' on 'CompositionEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'view', view == null ? null : view); own(this, 'data', data === undefined ? '' : String(data));
  });
  legacy(g.TextEvent, 'initTextEvent', function initTextEvent(type, bubbles, cancelable, view, data) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initTextEvent' on 'TextEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'view', view == null ? null : view); own(this, 'data', data === undefined ? 'undefined' : String(data));
  });
  legacy(g.CustomEvent, 'initCustomEvent', function initCustomEvent(type, bubbles, cancelable, detail) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'initCustomEvent' on 'CustomEvent': 1 argument required, but only 0 present.");
    if (!initBase(this, arguments)) return;
    own(this, 'detail', detail === undefined ? null : detail);
  });
  // initEvent marks the event initialized.
  var baseInitEvent = EP.initEvent;
  if (typeof baseInitEvent === 'function') {
    try {
      Object.defineProperty(EP, 'initEvent', { configurable: true, writable: true, enumerable: true, value: function initEvent() {
        var r = baseInitEvent.apply(this, arguments);
        if (!this.__narsilLaeuft) uninitialized.delete(this);
        return r;
      } });
    } catch (e) {}
  }
  // The global scope is the window, an EventTarget: `dispatchEvent(e)` works
  // as a bare call like addEventListener (WPT synthetic-events-cancelable:
  // "'dispatchEvent' is not defined").
  if (!Object.prototype.hasOwnProperty.call(g, 'dispatchEvent')) {
    try {
      Object.defineProperty(g, 'dispatchEvent', { configurable: true, writable: true, enumerable: true, value: function dispatchEvent(ev) {
        var ET = g.EventTarget && g.EventTarget.prototype;
        return ET.dispatchEvent.call(g.window || g, ev);
      } });
    } catch (e) {}
  }
  // createEvent (DOM §4.5): the legacy interface names, ASCII case-insensitive.
  var ALIASES = {
    beforeunloadevent: 'BeforeUnloadEvent', compositionevent: 'CompositionEvent', customevent: 'CustomEvent',
    devicemotionevent: 'DeviceMotionEvent', deviceorientationevent: 'DeviceOrientationEvent', dragevent: 'DragEvent',
    event: 'Event', events: 'Event', focusevent: 'FocusEvent', hashchangeevent: 'HashChangeEvent', htmlevents: 'Event',
    keyboardevent: 'KeyboardEvent', messageevent: 'MessageEvent', mouseevent: 'MouseEvent', mouseevents: 'MouseEvent',
    storageevent: 'StorageEvent', svgevents: 'Event', textevent: 'TextEvent', touchevent: 'TouchEvent', uievent: 'UIEvent',
    uievents: 'UIEvent'
  };
  var DP = g.Document && g.Document.prototype;
  if (DP) {
    var createEvent = function createEvent(iface) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'createEvent' on 'Document': 1 argument required, but only 0 present.");
      var key = String(iface).replace(/[A-Z]/g, function (c) { return c.toLowerCase(); });
      var name = Object.prototype.hasOwnProperty.call(ALIASES, key) ? ALIASES[key] : null;
      var C = name ? g[name] : null;
      if (typeof C !== 'function' || (name === 'TouchEvent' && !('ontouchstart' in g))) {
        throw new g.DOMException("Failed to execute 'createEvent' on 'Document': The provided event type ('" + String(iface) + "') is invalid.", 'NotSupportedError');
      }
      var e;
      if (name === 'TextEvent') {
        e = new UI('');
        try { Object.setPrototypeOf(e, C.prototype); } catch (x) {}
        own(e, 'data', '');
      } else {
        e = new C('');
      }
      try { Object.defineProperty(e, 'isTrusted', { value: false, enumerable: true, configurable: true }); } catch (x) {}
      uninitialized.add(e);
      return e;
    };
    try { Object.defineProperty(DP, 'createEvent', { value: createEvent, configurable: true, writable: true, enumerable: true }); } catch (e) {}
    try { if (g.document && Object.prototype.hasOwnProperty.call(g.document, 'createEvent')) g.document.createEvent = createEvent; } catch (e) {}
  }
})();

// -- HTMLHyperlinkElementUtils of <a> and <area> (HTML §4.6.4) --
// Release 2026-10-15: the URL parts come from the element's url (the href
// parsed against the document base; null when it does not parse): getters
// give ":" / "" for a null url, setters run the URL setter and write the
// serialization back to `href` (none for a null url, the opaque-path and
// cannot-have-username cases are the URL setters' own rules); username
// and password exist. MEASURED before (WPT links-created-by-a-and-area-
// elements, 121 failures): a.username undefined, a.hash = "x" did not
// change href, `http://test:test/` (no valid port) gave protocol "http:".
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var U = g.URL;
  if (typeof U !== 'function') return;
  function urlVon(el) {
    var a = el.getAttribute('href');
    if (a === null) return null;
    try { return new U(a, el.baseURI || (g.document && g.document.baseURI)); } catch (e) { return null; }
  }
  var LEER = { origin: '', protocol: ':' };
  ['HTMLAnchorElement', 'HTMLAreaElement'].forEach(function (name) {
    var P = g[name] && g[name].prototype;
    if (!P) return;
    ['origin', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'].forEach(function (teil) {
      var desc = {
        configurable: true, enumerable: true,
        get: function () {
          var u = urlVon(this);
          if (!u) return Object.prototype.hasOwnProperty.call(LEER, teil) ? LEER[teil] : '';
          return u[teil];
        }
      };
      if (teil !== 'origin') {
        desc.set = function (v) {
          var u = urlVon(this);
          if (!u) return;
          try { u[teil] = String(v); } catch (e) { return; }
          this.setAttribute('href', u.href);
        };
      }
      try { Object.defineProperty(P, teil, desc); } catch (e) {}
    });
    // The `href` stringifier (WebIDL §3.7.7): a brand check on `this`, then
    // the href getter's steps (not a possibly shadowed `this.href`).
    // MEASURED before (WPT a-stringifier/area-stringifier, 16 failures):
    // `toString.call({})` returned "[object Object]".
    var K = g[name];
    try {
      Object.defineProperty(P, 'toString', { configurable: true, enumerable: true, writable: true, value: function toString() {
        if (!(this instanceof K)) throw new TypeError("Illegal invocation: 'toString' called on an object that does not implement " + name + '.');
        var a = g.Element.prototype.getAttribute.call(this, 'href');
        if (a === null) return '';
        try { return new U(a, this.baseURI || (g.document && g.document.baseURI)).href; } catch (e) { return a; }
      } });
    } catch (e) {}
  });
  // `a.text` (HTML §4.5.1): the text content.
  var AP = g.HTMLAnchorElement && g.HTMLAnchorElement.prototype;
  if (AP && !Object.getOwnPropertyDescriptor(AP, 'text')) {
    try {
      Object.defineProperty(AP, 'text', { configurable: true, enumerable: true,
        get: function () { return this.textContent; },
        set: function (v) { this.textContent = v; } });
    } catch (e) {}
  }
})();

// -- The window-reflecting event handlers of <body> and <frameset> (HTML §8.1.8.1) --
// Release 2026-10-15: on a body or frameset element, onblur/onerror/onfocus/
// onload/onresize/onscroll and the WindowEventHandlers (onmessage,
// onstorage, onpopstate, ...) are the handlers of the element's Window: the
// IDL attributes forward, and setting or removing the content attribute
// sets the window's handler. MEASURED before (WPT dom/events/Body-FrameSet-
// Event-Handlers 36/48): `body.setAttribute('onload', ...)` left
// `window.onload` null; body.onmessage did not exist.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var NAMES = ['onblur', 'onerror', 'onfocus', 'onload', 'onresize', 'onscroll', 'onafterprint', 'onbeforeprint',
    'onbeforeunload', 'onhashchange', 'onlanguagechange', 'onmessage', 'onmessageerror', 'onoffline', 'ononline',
    'onpagehide', 'onpagereveal', 'onpageshow', 'onpageswap', 'onpopstate', 'onrejectionhandled', 'onstorage',
    'onunhandledrejection', 'onunload'];
  var E = g.Element && g.Element.prototype;
  if (!E) return;
  function windowOf(el) {
    var d = null;
    try { d = el.ownerDocument; } catch (e) {}
    var w = null;
    try { w = d ? d.defaultView : null; } catch (e) {}
    return w || null;
  }
  function compile(name, code) {
    try {
      return name === 'onerror'
        ? new Function('event', 'source', 'lineno', 'colno', 'error', code)
        : new Function('event', code);
    } catch (e) {
      try { if (typeof g.reportError === 'function') g.reportError(e); } catch (x) {}
      return null;
    }
  }
  function attributeChanged(el, name, code) {
    var n = String(name).toLowerCase();
    if (NAMES.indexOf(n) < 0) return;
    var w = windowOf(el);
    if (!w) return;
    try { w[n] = code === null ? null : compile(n, String(code)); } catch (e) {}
  }
  [g.HTMLBodyElement, g.HTMLFrameSetElement].forEach(function (C) {
    var P = C && C.prototype;
    if (!P) return;
    NAMES.forEach(function (n) {
      try {
        Object.defineProperty(P, n, {
          configurable: true, enumerable: true,
          get: function () { var w = windowOf(this); if (!w) return null; var v = w[n]; return v === undefined ? null : v; },
          // [LegacyTreatNonObjectAsNull]: anything but an object is null.
          set: function (v) { var w = windowOf(this); if (w) w[n] = (v !== null && (typeof v === 'object' || typeof v === 'function')) ? v : null; }
        });
      } catch (e) {}
    });
    var setA = E.setAttribute, removeA = E.removeAttribute, toggleA = E.toggleAttribute;
    try {
      Object.defineProperty(P, 'setAttribute', { configurable: true, writable: true, enumerable: true, value: function setAttribute(name, value) {
        var r = setA.apply(this, arguments);
        attributeChanged(this, name, String(value));
        return r;
      } });
      Object.defineProperty(P, 'removeAttribute', { configurable: true, writable: true, enumerable: true, value: function removeAttribute(name) {
        var r = removeA.apply(this, arguments);
        attributeChanged(this, name, null);
        return r;
      } });
      if (typeof toggleA === 'function') {
        Object.defineProperty(P, 'toggleAttribute', { configurable: true, writable: true, enumerable: true, value: function toggleAttribute(name) {
          var r = toggleA.apply(this, arguments);
          attributeChanged(this, name, r ? (this.getAttribute(name) || '') : null);
          return r;
        } });
      }
    } catch (e) {}
  });
})();

// -- @@unscopables of the DOM mixins (DOM §4.2.6, §4.2.8, §4.2.9) --
// Release 2026-10-15: ParentNode/ChildNode methods are [Unscopable], so an
// event handler attribute (with-scoped element) sees the globals of the
// same name. MEASURED before (WPT dom/nodes/remove-unscopable 0/6):
// `Element.prototype[Symbol.unscopables]` was undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof Symbol !== 'function' || !Symbol.unscopables) return;
  function add(ctor, names) {
    var P = g[ctor] && g[ctor].prototype;
    if (!P) return;
    var u = Object.prototype.hasOwnProperty.call(P, Symbol.unscopables) ? P[Symbol.unscopables] : Object.create(null);
    for (var i = 0; i < names.length; i++) u[names[i]] = true;
    try { Object.defineProperty(P, Symbol.unscopables, { value: u, configurable: true, writable: false, enumerable: false }); } catch (e) {}
  }
  var parent = ['prepend', 'append', 'replaceChildren'], child = ['before', 'after', 'replaceWith', 'remove'];
  add('Element', parent.concat(child, ['slot']));
  add('CharacterData', child);
  add('DocumentType', child);
  add('Document', parent);
  add('DocumentFragment', parent);
})();

// -- Document metadata reflections: title.text, fetchPriority --
// Release 14.10.2026: HTMLTitleElement.text (HTML §4.2.2: the child text
// content; setting replaces all children) and fetchPriority on link, img and
// script (HTML §2.5.10, enumerated: high/low/auto, default auto). MEASURED
// before (WPT document-metadata): title.text-01..04 and
// attr-link-fetchpriority read undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  function accessor(P, name, get, set) {
    if (!P || Object.getOwnPropertyDescriptor(P, name)) return;
    try { Object.defineProperty(P, name, { get: get, set: set, configurable: true, enumerable: true }); } catch (e) {}
  }
  var TP = g.HTMLTitleElement && g.HTMLTitleElement.prototype;
  accessor(TP, 'text', function () {
    var out = '';
    for (var n = this.firstChild; n; n = n.nextSibling) if (n.nodeType === 3 || n.nodeType === 4) out += n.data;
    return out;
  }, function (v) { this.textContent = String(v); });
  ['HTMLLinkElement', 'HTMLImageElement', 'HTMLScriptElement'].forEach(function (k) {
    var P = g[k] && g[k].prototype;
    accessor(P, 'fetchPriority', function () {
      var v = String(this.getAttribute('fetchpriority') || '').toLowerCase();
      return v === 'high' || v === 'low' || v === 'auto' ? v : 'auto';
    }, function (v) { this.setAttribute('fetchpriority', String(v)); });
  });
})();

// -- TypedArray iteration and methods (ES2023 23.2.3) --
// Release 15.10.2026: the engine's typed arrays had no `values`/`keys`/
// `entries`/`[Symbol.iterator]` — `[...imageData.data]` and `for (v of
// bytes)` threw "Wert ist nicht iterierbar" (WPT html/canvas wide-gamut).
// The generic Array iterators work on typed arrays (length + indices).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var U = g.Uint8Array;
  var TA = U && Object.getPrototypeOf(U.prototype);
  if (!TA || typeof Symbol === 'undefined') return;
  var AP = Array.prototype;
  var def = function (name, f) {
    if (typeof TA[name] !== 'function' && typeof f === 'function') {
      try { Object.defineProperty(TA, name, { value: f, writable: true, configurable: true, enumerable: false }); } catch (e) {}
    }
  };
  def('values', AP.values);
  def('keys', AP.keys);
  def('entries', AP.entries);
  if (typeof TA[Symbol.iterator] !== 'function' && typeof TA.values === 'function') {
    try { Object.defineProperty(TA, Symbol.iterator, { value: TA.values, writable: true, configurable: true, enumerable: false }); } catch (e) {}
  }
  // The other %TypedArray%.prototype methods the engine lacks (ES2023
  // 23.2.3): the generic Array methods work on typed arrays; the ones
  // that create arrays return the same typed-array type.
  ['forEach', 'every', 'some', 'indexOf', 'lastIndexOf', 'includes', 'find', 'findIndex', 'findLast', 'findLastIndex',
    'reduce', 'reduceRight', 'fill', 'reverse', 'copyWithin', 'at'].forEach(function (k) { def(k, AP[k]); });
  def('slice', function slice(start, end) { return new this.constructor(AP.slice.call(this, start, end)); });
  def('filter', function filter(f, self) { return new this.constructor(AP.filter.call(this, f, self)); });
  def('map', function map(f, self) {
    var n = this.length, out = new this.constructor(n);
    for (var i = 0; i < n; i++) out[i] = f.call(self, this[i], i, this);
    return out;
  });
  def('sort', function sort(cmp) {
    var a = AP.slice.call(this);
    a.sort(typeof cmp === 'function' ? cmp : function (x, y) { return x < y ? -1 : x > y ? 1 : 0; });
    for (var i = 0; i < a.length; i++) this[i] = a[i];
    return this;
  });
})();

// -- Canvas 2D API (Release 15.10.2026) --
// `canvas2d.js` (CanvasRenderingContext2D, OffscreenCanvas, Path2D, ImageData,
// DOMMatrix, ...) is compiled only when a page first touches one of its names:
// most pages never draw on a canvas, and the module is large.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.__narsilC2dQuelle !== 'function') return;
  var quelle = g.__narsilC2dQuelle;
  var names = ['CanvasRenderingContext2D', 'OffscreenCanvasRenderingContext2D', 'OffscreenCanvas', 'CanvasGradient',
    'CanvasPattern', 'Path2D', 'ImageData', 'TextMetrics', 'ImageBitmap', 'ImageBitmapRenderingContext',
    'createImageBitmap', 'DOMMatrix', 'DOMMatrixReadOnly', 'DOMPoint', 'DOMPointReadOnly', 'WebKitCSSMatrix', 'TextCluster', 'CanvasFilter'];
  var methods = ['getContext', 'toDataURL', 'toBlob', 'transferControlToOffscreen'];
  var HCE = g.HTMLCanvasElement && g.HTMLCanvasElement.prototype;
  var loaded = false;
  function load() {
    if (loaded) return;
    loaded = true;
    for (var i = 0; i < names.length; i++) delete g[names[i]];
    if (HCE) for (var k = 0; k < methods.length; k++) delete HCE[methods[k]];
    (0, eval)(quelle());
  }
  names.forEach(function (n) {
    Object.defineProperty(g, n, {
      configurable: true, enumerable: false,
      get: function () { load(); return g[n]; },
      set: function (v) { load(); Object.defineProperty(g, n, { value: v, writable: true, configurable: true, enumerable: false }); }
    });
  });
  if (HCE) methods.forEach(function (m) {
    var stub = function () { load(); return HCE[m].apply(this, arguments); };
    try { Object.defineProperty(stub, 'name', { value: m }); } catch (e) {}
    Object.defineProperty(HCE, m, { value: stub, writable: true, configurable: true, enumerable: true });
  });
  try { delete g.__narsilC2dQuelle; } catch (e) {}
})();

// -- Navigator members (HTML §8.9.1) --
// Release 15.10.2026: `navigator` only had userAgent, language(s),
// platform, onLine, cookieEnabled, hardwareConcurrency and clipboard.
// MEASURED before: `navigator.appVersion`, `vendor`, `product`, `plugins`,
// `mimeTypes`, `maxTouchPoints`, `webdriver`, `sendBeacon`, `permissions`,
// `storage`, `locks`, `geolocation` and `mediaDevices` were undefined —
// browser-detection code such as `navigator.appVersion.indexOf(..)` or
// `navigator.plugins.length` threw a TypeError and ended its script. The
// values follow Firefox 128 (the user agent we send), the PDF viewer
// plugins are the fixed list of HTML §8.9.1.6.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var N = g.navigator;
  if (!N || typeof N !== 'object') return;
  var ua = String(N.userAgent || '');
  var win = /Windows/.test(ua), mac = /Macintosh/.test(ua);
  function def(name, value) {
    if (name in N) return;
    try { Object.defineProperty(N, name, { value: value, writable: false, enumerable: true, configurable: true }); } catch (e) {}
  }
  function methode(o, name, f) {
    try { Object.defineProperty(o, name, { value: f, writable: true, enumerable: true, configurable: true }); } catch (e) {}
  }
  function tag(o, name) {
    try { Object.defineProperty(o, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
  }
  function domFehler(name, text) { return g.DOMException ? new g.DOMException(text, name) : new Error(text); }
  if (g.Navigator && g.Navigator.prototype) { try { Object.setPrototypeOf(N, g.Navigator.prototype); } catch (e) {} }
  // The platform string as Firefox reports it (was "windows x86_64").
  try { Object.defineProperty(N, 'platform', { value: win ? 'Win32' : (mac ? 'MacIntel' : 'Linux x86_64'), writable: false, enumerable: true, configurable: true }); } catch (e) {}
  def('appCodeName', 'Mozilla');
  def('appName', 'Netscape');
  def('appVersion', win ? '5.0 (Windows)' : (mac ? '5.0 (Macintosh)' : '5.0 (X11)'));
  def('product', 'Gecko');
  def('productSub', '20100101');
  def('vendor', '');
  def('vendorSub', '');
  def('oscpu', win ? 'Windows NT 10.0; Win64; x64' : (mac ? 'Intel Mac OS X 10.15' : 'Linux x86_64'));
  def('buildID', '20181001000000');
  def('maxTouchPoints', 0);
  def('webdriver', false);
  def('pdfViewerEnabled', true);
  def('doNotTrack', 'unspecified');
  if (!('javaEnabled' in N)) methode(N, 'javaEnabled', function javaEnabled() { return false; });
  // PluginArray / MimeTypeArray with the PDF viewer entries.
  function liste(eintraege, schnittstelle) {
    var l = Object.create(g[schnittstelle] && g[schnittstelle].prototype || Object.prototype);
    eintraege.forEach(function (e, i) { try { Object.defineProperty(l, i, { value: e, enumerable: true, configurable: true }); } catch (x) {} });
    try { Object.defineProperty(l, 'length', { value: eintraege.length, configurable: true }); } catch (x) {}
    methode(l, 'item', function item(i) { return eintraege[i >>> 0] || null; });
    methode(l, 'namedItem', function namedItem(n) {
      for (var i = 0; i < eintraege.length; i++) if (eintraege[i].name === String(n) || eintraege[i].type === String(n)) return eintraege[i];
      return null;
    });
    if (typeof Symbol === 'function' && Symbol.iterator) {
      methode(l, Symbol.iterator, function () { return eintraege.slice()[Symbol.iterator](); });
    }
    tag(l, schnittstelle);
    return l;
  }
  if (!('plugins' in N) || !('mimeTypes' in N)) {
    var MP = g.MimeType && g.MimeType.prototype || Object.prototype;
    var PP = g.Plugin && g.Plugin.prototype || Object.prototype;
    var typen = [['application/pdf', 'Portable Document Format'], ['text/pdf', 'Portable Document Format']].map(function (t) {
      var m = Object.create(MP);
      m.type = t[0]; m.description = t[1]; m.suffixes = 'pdf';
      return m;
    });
    var plugins = ['PDF Viewer', 'Chrome PDF Viewer', 'Chromium PDF Viewer', 'Microsoft Edge PDF Viewer', 'WebKit built-in PDF'].map(function (name) {
      var p = Object.create(PP);
      p.name = name; p.description = 'Portable Document Format'; p.filename = 'internal-pdf-viewer';
      typen.forEach(function (m, i) { p[i] = m; });
      p.length = typen.length;
      p.item = function item(i) { return typen[i >>> 0] || null; };
      p.namedItem = function namedItem(n) { for (var i = 0; i < typen.length; i++) if (typen[i].type === String(n)) return typen[i]; return null; };
      return p;
    });
    typen.forEach(function (m) { m.enabledPlugin = plugins[0]; });
    var pl = liste(plugins, 'PluginArray');
    methode(pl, 'refresh', function refresh() {});
    def('plugins', pl);
    def('mimeTypes', liste(typen, 'MimeTypeArray'));
  }
  // sendBeacon: a POST that outlives the page (Beacon §3.1).
  if (!('sendBeacon' in N)) methode(N, 'sendBeacon', function sendBeacon(url, data) {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'sendBeacon' on 'Navigator': 1 argument required, but only 0 present.");
    var ziel;
    try { ziel = new g.URL(String(url), g.document && g.document.baseURI || undefined); } catch (e) { throw new TypeError("Failed to execute 'sendBeacon' on 'Navigator': Invalid URL"); }
    if (!/^https?:$/.test(ziel.protocol)) throw new TypeError("Failed to execute 'sendBeacon' on 'Navigator': Beacons are only supported over HTTP(S).");
    try {
      var p = g.fetch(ziel.href, { method: 'POST', body: data == null ? null : data, keepalive: true, credentials: 'include', mode: 'no-cors' });
      if (p && typeof p.catch === 'function') p.catch(function () {});
    } catch (e) {}
    return true;
  });
  // Permissions API (Permissions §5): every known permission is "prompt";
  // a page never gets one without asking.
  if (!('permissions' in N)) {
    var bekannt = ['geolocation', 'notifications', 'push', 'midi', 'camera', 'microphone', 'background-sync', 'persistent-storage',
      'clipboard-read', 'clipboard-write', 'storage-access', 'top-level-storage-access', 'screen-wake-lock', 'accelerometer',
      'gyroscope', 'magnetometer', 'ambient-light-sensor', 'payment-handler', 'local-fonts', 'display-capture', 'idle-detection',
      'window-management', 'speaker-selection', 'background-fetch', 'nfc'];
    var perms = {};
    methode(perms, 'query', function query(desc) {
      try {
        if (!desc || typeof desc !== 'object' || !('name' in desc)) throw new TypeError("Failed to execute 'query' on 'Permissions': required member name is undefined.");
        var name = String(desc.name);
        if (bekannt.indexOf(name) < 0) throw new TypeError("Failed to execute 'query' on 'Permissions': The provided value '" + name + "' is not a valid enum value of type PermissionName.");
        var st;
        try { st = g.EventTarget ? new g.EventTarget() : {}; } catch (e) { st = {}; }
        Object.defineProperty(st, 'name', { value: name, enumerable: true });
        Object.defineProperty(st, 'state', { value: name === 'clipboard-write' ? 'granted' : 'prompt', enumerable: true });
        st.onchange = null;
        tag(st, 'PermissionStatus');
        return Promise.resolve(st);
      } catch (e) { return Promise.reject(e); }
    });
    tag(perms, 'Permissions');
    def('permissions', perms);
  }
  // StorageManager (Storage §4).
  if (!('storage' in N)) {
    var sm = {};
    methode(sm, 'estimate', function estimate() { return Promise.resolve({ quota: 10 * 1024 * 1024 * 1024, usage: 0, usageDetails: {} }); });
    methode(sm, 'persisted', function persisted() { return Promise.resolve(false); });
    methode(sm, 'persist', function persist() { return Promise.resolve(false); });
    methode(sm, 'getDirectory', function getDirectory() { return Promise.reject(domFehler('SecurityError', 'The operation is insecure.')); });
    tag(sm, 'StorageManager');
    def('storage', sm);
  }
  // Web Locks (one agent, so a lock is only contended inside this page):
  // exclusive requests for one name run one after another, shared ones
  // together; `ifAvailable` and `steal` as in Web Locks §4.
  if (!('locks' in N)) {
    var gehalten = [], wartend = [];
    var frei = function (name, modus) {
      return !gehalten.some(function (l) { return l.name === name && (modus === 'exclusive' || l.mode === 'exclusive'); });
    };
    var starten = function (a) {
      var lock = { name: a.name, mode: a.mode };
      tag(lock, 'Lock');
      gehalten.push(lock);
      var r;
      try { r = Promise.resolve(a.cb(lock)); } catch (e) { r = Promise.reject(e); }
      var ende = function () { var k = gehalten.indexOf(lock); if (k >= 0) gehalten.splice(k, 1); weiter(); };
      r.then(function (v) { ende(); a.ok(v); }, function (e) { ende(); a.nein(e); });
    };
    var weiter = function () {
      for (var i = 0; i < wartend.length; i++) {
        var a = wartend[i];
        var vorher = wartend.slice(0, i).some(function (b) { return b.name === a.name; });
        if (!vorher && frei(a.name, a.mode)) { wartend.splice(i, 1); i--; starten(a); }
      }
    };
    var lm = {};
    methode(lm, 'request', function request(name, opt, cb) {
      if (typeof opt === 'function') { cb = opt; opt = {}; }
      opt = opt || {};
      if (typeof cb !== 'function') return Promise.reject(new TypeError("Failed to execute 'request' on 'LockManager': The callback provided as parameter 2 is not a function."));
      name = String(name);
      if (name.charAt(0) === '-') return Promise.reject(domFehler('NotSupportedError', "Names cannot start with '-'."));
      var mode = opt.mode === 'shared' ? 'shared' : 'exclusive';
      return new Promise(function (ok, nein) {
        var a = { name: name, mode: mode, cb: cb, ok: ok, nein: nein };
        if (opt.steal) { gehalten = gehalten.filter(function (l) { return l.name !== name; }); starten(a); return; }
        var frueher = wartend.some(function (b) { return b.name === name; });
        if (!frueher && frei(name, mode)) { starten(a); return; }
        if (opt.ifAvailable) {
          var r;
          try { r = Promise.resolve(cb(null)); } catch (e) { r = Promise.reject(e); }
          r.then(ok, nein);
          return;
        }
        wartend.push(a);
      });
    });
    methode(lm, 'query', function query() {
      var info = function (l) { return { name: l.name, mode: l.mode, clientId: '' }; };
      return Promise.resolve({ held: gehalten.map(info), pending: wartend.map(info) });
    });
    tag(lm, 'LockManager');
    def('locks', lm);
  }
  // Geolocation (no position is ever handed out without a prompt).
  if (!('geolocation' in N)) {
    var geo = {}, wid = 0;
    var abgelehnt = function (fehler) {
      if (typeof fehler !== 'function') return;
      g.setTimeout(function () { fehler({ code: 1, message: 'User denied Geolocation', PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 }); }, 0);
    };
    methode(geo, 'getCurrentPosition', function getCurrentPosition(ok, fehler) {
      if (typeof ok !== 'function') throw new TypeError("Failed to execute 'getCurrentPosition' on 'Geolocation': parameter 1 is not of type 'Function'.");
      abgelehnt(fehler);
    });
    methode(geo, 'watchPosition', function watchPosition(ok, fehler) {
      if (typeof ok !== 'function') throw new TypeError("Failed to execute 'watchPosition' on 'Geolocation': parameter 1 is not of type 'Function'.");
      abgelehnt(fehler);
      return ++wid;
    });
    methode(geo, 'clearWatch', function clearWatch() {});
    tag(geo, 'Geolocation');
    def('geolocation', geo);
  }
  // MediaDevices: no camera or microphone is exposed.
  if (!('mediaDevices' in N)) {
    var md;
    try { md = g.EventTarget ? new g.EventTarget() : {}; } catch (e) { md = {}; }
    methode(md, 'enumerateDevices', function enumerateDevices() { return Promise.resolve([]); });
    methode(md, 'getSupportedConstraints', function getSupportedConstraints() { return {}; });
    methode(md, 'getUserMedia', function getUserMedia() { return Promise.reject(domFehler('NotAllowedError', 'Permission denied')); });
    methode(md, 'getDisplayMedia', function getDisplayMedia() { return Promise.reject(domFehler('NotAllowedError', 'Permission denied')); });
    md.ondevicechange = null;
    tag(md, 'MediaDevices');
    def('mediaDevices', md);
  }
  if (!('userActivation' in N)) {
    var akt = {};
    Object.defineProperty(akt, 'hasBeenActive', { get: function () { return false; }, enumerable: true });
    Object.defineProperty(akt, 'isActive', { get: function () { return false; }, enumerable: true });
    tag(akt, 'UserActivation');
    def('userActivation', akt);
  }
  tag(N, 'Navigator');
})();

// -- Console namespace (WHATWG Console Standard §1) --
// Release 15.10.2026: the engine only had log/info/warn/error. MEASURED
// before: heise.de's akwa.js called `console.dir(banner)` in its start-up
// function, the TypeError ended it; `console.debug`, `group`, `table`,
// `time` and `count` are just as common. The missing members are built on
// the four native ones (their output still reaches the console panel);
// timers and counters follow the spec's label rules and warnings.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var C = g.console;
  if (!C) return;
  function nativ(name) {
    var f = C[name];
    return typeof f === 'function' ? f : (typeof C.log === 'function' ? C.log : function () {});
  }
  var log = nativ('log'), info = nativ('info'), warn = nativ('warn'), error = nativ('error');
  function jetzt() { return (g.performance && typeof g.performance.now === 'function') ? g.performance.now() : Date.now(); }
  function label(l) { return l === undefined ? 'default' : String(l); }
  function rest(args, ab) { return Array.prototype.slice.call(args, ab); }
  var zaehler = Object.create(null), uhren = Object.create(null), tiefe = 0;
  var neu = {
    debug: function debug() { return log.apply(C, arguments); },
    trace: function trace() { return log.apply(C, ['Trace' + (arguments.length ? ':' : '')].concat(rest(arguments, 0))); },
    assert: function assert(bedingung) {
      if (bedingung) return;
      var daten = rest(arguments, 1);
      if (!daten.length) daten = ['Assertion failed'];
      else if (typeof daten[0] === 'string') daten[0] = 'Assertion failed: ' + daten[0];
      else daten.unshift('Assertion failed:');
      return error.apply(C, daten);
    },
    clear: function clear() { tiefe = 0; },
    count: function count(l) {
      l = label(l);
      zaehler[l] = (zaehler[l] || 0) + 1;
      return info.call(C, l + ': ' + zaehler[l]);
    },
    countReset: function countReset(l) {
      l = label(l);
      if (l in zaehler) zaehler[l] = 0;
      else warn.call(C, "Count for '" + l + "' does not exist");
    },
    dir: function dir(item) { return log.call(C, item); },
    dirxml: function dirxml() { return log.apply(C, arguments); },
    table: function table(daten) { return log.call(C, daten); },
    group: function group() { tiefe++; if (arguments.length) return log.apply(C, arguments); },
    groupCollapsed: function groupCollapsed() { tiefe++; if (arguments.length) return log.apply(C, arguments); },
    groupEnd: function groupEnd() { if (tiefe > 0) tiefe--; },
    time: function time(l) {
      l = label(l);
      if (l in uhren) { warn.call(C, "Timer '" + l + "' already exists"); return; }
      uhren[l] = jetzt();
    },
    timeLog: function timeLog(l) {
      l = label(l);
      if (!(l in uhren)) { warn.call(C, "Timer '" + l + "' does not exist"); return; }
      return log.apply(C, [l + ': ' + (jetzt() - uhren[l]) + ' ms'].concat(rest(arguments, 1)));
    },
    timeEnd: function timeEnd(l) {
      l = label(l);
      if (!(l in uhren)) { warn.call(C, "Timer '" + l + "' does not exist"); return; }
      var dauer = jetzt() - uhren[l];
      delete uhren[l];
      return info.call(C, l + ': ' + dauer + ' ms');
    },
    timeStamp: function timeStamp() {},
    profile: function profile() {},
    profileEnd: function profileEnd() {}
  };
  Object.keys(neu).forEach(function (k) {
    if (typeof C[k] === 'function') return;
    try { Object.defineProperty(C, k, { value: neu[k], writable: true, enumerable: true, configurable: true }); } catch (e) {}
  });
  try {
    if (!Object.prototype.hasOwnProperty.call(C, Symbol.toStringTag)) {
      Object.defineProperty(C, Symbol.toStringTag, { value: 'console', configurable: true });
    }
  } catch (e) {}
})();

// -- Event legacy members: returnValue, srcElement (DOM §2.2) --
// Release 2026-10-14: `returnValue` is the inverse of the canceled flag
// (setting false cancels like preventDefault()), `srcElement` is the target.
// (`cancelBubble` belongs to the native Event.)
// MEASURED before (WPT dom/events): Event-returnValue 1/7,
// event-src-element-nullable 0/1 (both undefined).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var E = g.Event && g.Event.prototype;
  if (!E) return;
  try {
    if (!Object.getOwnPropertyDescriptor(E, 'returnValue')) {
      Object.defineProperty(E, 'returnValue', {
        configurable: true, enumerable: true,
        get: function () { return !this.defaultPrevented; },
        set: function (v) { if (!v) this.preventDefault(); }
      });
    }
    if (!Object.getOwnPropertyDescriptor(E, 'srcElement')) {
      Object.defineProperty(E, 'srcElement', {
        configurable: true, enumerable: true,
        get: function () { var t = this.target; return t === undefined ? null : t; }
      });
    }
  } catch (e) {}
})();

// -- WHATWG Streams (ReadableStream, WritableStream, TransformStream) --
// Release 2026-10-14: the Streams Standard in the prelude — readable
// streams with default and BYOB readers, tee(), pipeTo()/pipeThrough(),
// async iteration and ReadableStream.from(); writable streams with writers,
// backpressure and abort signals; transform streams; the queuing
// strategies; TextEncoderStream/TextDecoderStream; Response.body,
// Request.body and Blob.prototype.stream(). Queues keep a head index (no
// per-chunk array shifting); chunks are passed through without copies.
// MEASURED before: ReadableStream/WritableStream/TransformStream were
// undefined, `new Response("x").body` was undefined (WPT domparsing
// stream*HTML tests failed with "WritableStream is not defined").
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.ReadableStream === 'function') return;
  var apply = typeof Reflect === 'object' && typeof Reflect.apply === 'function'
    ? Reflect.apply
    : function (f, t, a) { return Function.prototype.apply.call(f, t, a); };
  function noop() {}
  function isObject(x) { return x !== null && (typeof x === 'object' || typeof x === 'function'); }
  function isCallable(f) { return typeof f === 'function'; }
  // ArrayBuffer.isView (missing in the engine): a typed array or DataView.
  function isView(x) {
    if (typeof ArrayBuffer.isView === 'function') return ArrayBuffer.isView(x);
    return !!x && typeof x === 'object' && x.buffer instanceof ArrayBuffer && typeof x.byteOffset === 'number' && typeof x.byteLength === 'number';
  }
  function typeError(m) { return new TypeError(m); }
  // A deferred that knows whether it is settled (`state`).
  function deferred() {
    var d = { state: 'pending' };
    d.promise = new Promise(function (res, rej) {
      d.resolve = function (v) { if (d.state === 'pending') { d.state = 'fulfilled'; res(v); } };
      d.reject = function (e) { if (d.state === 'pending') { d.state = 'rejected'; rej(e); } };
    });
    return d;
  }
  function settledRejected(e) { var d = deferred(); d.reject(e); markHandled(d.promise); return d; }
  function resolvedWith(v) { return Promise.resolve(v); }
  function rejectedWith(e) { var p = Promise.reject(e); p.then(null, noop); return p; }
  function markHandled(p) { if (p && typeof p.then === 'function') p.then(null, noop); return p; }
  function promiseCall(f, thisArg, args) {
    if (f === undefined) return resolvedWith(undefined);
    try { return Promise.resolve(apply(f, thisArg, args)); } catch (e) { return Promise.reject(e); }
  }
  function getMethod(obj, name, what) {
    if (obj === undefined || obj === null) return undefined;
    var m = obj[name];
    if (m === undefined || m === null) return undefined;
    if (!isCallable(m)) throw typeError("Failed to construct '" + what + "': '" + name + "' is not a function.");
    return m;
  }
  function highWaterMark(strategy, dflt) {
    if (strategy === undefined || strategy === null) return dflt;
    var h = strategy.highWaterMark;
    if (h === undefined) return dflt;
    h = Number(h);
    if (h !== h || h < 0) throw new RangeError('Invalid highWaterMark');
    return h;
  }
  function sizeAlgorithm(strategy) {
    var s = strategy === undefined || strategy === null ? undefined : strategy.size;
    if (s === undefined) return function () { return 1; };
    if (!isCallable(s)) throw typeError("Failed to read the 'size' property from 'QueuingStrategy': The provided value is not a function.");
    return function (chunk) { return apply(s, undefined, [chunk]); };
  }
  function defineClass(C, name) {
    try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    try { Object.defineProperty(g, name, { value: C, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = C; }
  }
  function method(P, name, f) {
    try { Object.defineProperty(P, name, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) { P[name] = f; }
  }
  function getter(P, name, f) {
    try { Object.defineProperty(P, name, { get: f, configurable: true, enumerable: true }); } catch (e) {}
  }

  // A FIFO with sizes; `head` avoids shifting the array per chunk.
  function Queue() { this.items = []; this.sizes = []; this.head = 0; this.total = 0; }
  Queue.prototype.length = function () { return this.items.length - this.head; };
  Queue.prototype.push = function (v, size) {
    if (typeof size !== 'number' || size !== size || size < 0 || size === Infinity) throw new RangeError('Invalid chunk size');
    this.items.push(v); this.sizes.push(size); this.total += size;
  };
  Queue.prototype.shift = function () {
    var v = this.items[this.head], s = this.sizes[this.head];
    this.items[this.head] = undefined;
    this.head++;
    this.total -= s;
    if (this.total < 0) this.total = 0;
    if (this.head > 1024 && this.head * 2 > this.items.length) {
      this.items = this.items.slice(this.head); this.sizes = this.sizes.slice(this.head); this.head = 0;
    }
    return v;
  };
  Queue.prototype.peek = function () { return this.items[this.head]; };
  Queue.prototype.reset = function () { this.items = []; this.sizes = []; this.head = 0; this.total = 0; };

  // ===================== ReadableStream =====================
  var RS = new WeakMap(), RDC = new WeakMap(), RDR = new WeakMap(), RBR = new WeakMap();
  var ReadableStream = function ReadableStream(underlyingSource, strategy) {
    if (!(this instanceof ReadableStream)) throw typeError("Failed to construct 'ReadableStream': Please use the 'new' operator.");
    var src = underlyingSource === undefined ? null : underlyingSource;
    if (src !== null && !isObject(src)) throw typeError("Failed to construct 'ReadableStream': The provided value is not an object.");
    var type = src ? src.type : undefined;
    initReadable(this);
    if (type !== undefined && String(type) !== 'bytes') throw typeError("Failed to construct 'ReadableStream': type must be 'bytes' or undefined.");
    var isBytes = type !== undefined;
    if (isBytes && strategy && strategy.size !== undefined) throw new RangeError("The strategy for a byte stream cannot have a size function");
    var hwm = highWaterMark(strategy, isBytes ? 0 : 1);
    var size = isBytes ? function (c) { return c.byteLength; } : sizeAlgorithm(strategy);
    var start = getMethod(src, 'start', 'ReadableStream');
    var pull = getMethod(src, 'pull', 'ReadableStream');
    var cancel = getMethod(src, 'cancel', 'ReadableStream');
    var self = this;
    setupDefaultController(this, function (c) { return start ? apply(start, src, [c]) : undefined; },
      function (c) { return promiseCall(pull, src, [c]); },
      function (r) { return promiseCall(cancel, src, [r]); }, hwm, size, isBytes);
    void self;
  };
  function initReadable(stream) {
    RS.set(stream, { state: 'readable', reader: undefined, storedError: undefined, disturbed: false, controller: undefined });
  }
  function rs(stream, what) {
    var s = RS.get(stream);
    if (!s) throw typeError("Failed to execute '" + what + "' on 'ReadableStream': Illegal invocation");
    return s;
  }
  function isLocked(stream) { return RS.get(stream).reader !== undefined; }
  // Controller (default; byte streams use it with byte chunks).
  var ReadableStreamDefaultController = function ReadableStreamDefaultController() { throw typeError('Illegal constructor'); };
  var ReadableByteStreamController = function ReadableByteStreamController() { throw typeError('Illegal constructor'); };
  function setupDefaultController(stream, startAlgo, pullAlgo, cancelAlgo, hwm, size, isBytes) {
    var c = Object.create((isBytes ? ReadableByteStreamController : ReadableStreamDefaultController).prototype);
    var cs = { stream: stream, queue: new Queue(), started: false, closeRequested: false, pullAgain: false, pulling: false,
      size: size, hwm: hwm, pull: pullAlgo, cancel: cancelAlgo, bytes: !!isBytes };
    RDC.set(c, cs);
    RS.get(stream).controller = c;
    var startResult;
    try { startResult = startAlgo(c); } catch (e) { throw e; }
    Promise.resolve(startResult).then(function () {
      cs.started = true;
      callPullIfNeeded(c);
    }, function (r) { controllerError(c, r); });
    return c;
  }
  function canCloseOrEnqueue(cs) { return !cs.closeRequested && RS.get(cs.stream).state === 'readable'; }
  function desiredSize(cs) {
    var st = RS.get(cs.stream).state;
    if (st === 'errored') return null;
    if (st === 'closed') return 0;
    return cs.hwm - cs.queue.total;
  }
  function readRequests(stream) {
    var r = RS.get(stream).reader;
    if (!r) return 0;
    var rd = RDR.get(r) || RBR.get(r);
    return rd ? rd.requests.length() : 0;
  }
  function shouldCallPull(c) {
    var cs = RDC.get(c);
    if (!canCloseOrEnqueue(cs) || !cs.started) return false;
    if (isLocked(cs.stream) && readRequests(cs.stream) > 0) return true;
    return desiredSize(cs) > 0;
  }
  function callPullIfNeeded(c) {
    var cs = RDC.get(c);
    if (!shouldCallPull(c)) return;
    if (cs.pulling) { cs.pullAgain = true; return; }
    cs.pulling = true;
    cs.pull(c).then(function () {
      cs.pulling = false;
      if (cs.pullAgain) { cs.pullAgain = false; callPullIfNeeded(c); }
    }, function (e) { controllerError(c, e); });
  }
  function clearAlgorithms(cs) { cs.pull = function () { return resolvedWith(); }; cs.cancel = function () { return resolvedWith(); }; cs.size = function () { return 1; }; }
  function controllerError(c, e) {
    var cs = RDC.get(c);
    if (RS.get(cs.stream).state !== 'readable') return;
    cs.queue.reset();
    clearAlgorithms(cs);
    readableError(cs.stream, e);
  }
  function controllerClose(c) {
    var cs = RDC.get(c);
    if (!canCloseOrEnqueue(cs)) return;
    cs.closeRequested = true;
    if (cs.queue.length() === 0) { clearAlgorithms(cs); readableClose(cs.stream); }
  }
  function controllerEnqueue(c, chunk) {
    var cs = RDC.get(c);
    if (!canCloseOrEnqueue(cs)) return;
    var stream = cs.stream, s = RS.get(stream);
    var reader = s.reader, rd = reader && RDR.get(reader), bd = reader && RBR.get(reader);
    if (rd && rd.requests.length() > 0) {
      rd.requests.shift().chunk(chunk);
    } else if (bd && bd.requests.length() > 0 && cs.bytes) {
      cs.queue.push(chunk, chunk.byteLength);
      fulfillByobRequests(cs, bd);
    } else {
      var size;
      try { size = cs.size(chunk); } catch (e) { controllerError(c, e); throw e; }
      try { cs.queue.push(chunk, size); } catch (e) { controllerError(c, e); throw e; }
    }
    callPullIfNeeded(c);
  }
  function pullSteps(c, request) {
    var cs = RDC.get(c);
    if (cs.queue.length() > 0) {
      var chunk = cs.queue.shift();
      if (cs.closeRequested && cs.queue.length() === 0) { clearAlgorithms(cs); readableClose(cs.stream); }
      else callPullIfNeeded(c);
      request.chunk(chunk);
    } else {
      var rd = RDR.get(RS.get(cs.stream).reader);
      rd.requests.push(request, 0);
      callPullIfNeeded(c);
    }
  }
  function readableClose(stream) {
    var s = RS.get(stream);
    s.state = 'closed';
    var reader = s.reader;
    if (!reader) return;
    var rd = RDR.get(reader) || RBR.get(reader);
    rd.closed.resolve(undefined);
    var reqs = rd.requests;
    rd.requests = new Queue();
    while (reqs.length() > 0) reqs.shift().close();
  }
  function readableError(stream, e) {
    var s = RS.get(stream);
    s.state = 'errored';
    s.storedError = e;
    var reader = s.reader;
    if (!reader) return;
    var rd = RDR.get(reader) || RBR.get(reader);
    rd.closed.reject(e);
    markHandled(rd.closed.promise);
    var reqs = rd.requests;
    rd.requests = new Queue();
    while (reqs.length() > 0) reqs.shift().error(e);
  }
  function readableCancel(stream, reason) {
    var s = RS.get(stream);
    s.disturbed = true;
    if (s.state === 'closed') return resolvedWith(undefined);
    if (s.state === 'errored') return rejectedWith(s.storedError);
    readableClose(stream);
    var cs = RDC.get(s.controller);
    cs.queue.reset();
    var p = cs.cancel(reason);
    clearAlgorithms(cs);
    return p.then(function () { return undefined; });
  }
  method(ReadableStreamDefaultController.prototype, 'enqueue', function enqueue(chunk) {
    var cs = RDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    if (!canCloseOrEnqueue(cs)) throw typeError("Failed to execute 'enqueue' on 'ReadableStreamDefaultController': Cannot enqueue a chunk into a closed or errored stream.");
    controllerEnqueue(this, chunk);
  });
  method(ReadableStreamDefaultController.prototype, 'close', function close() {
    var cs = RDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    if (!canCloseOrEnqueue(cs)) throw typeError("Failed to execute 'close' on 'ReadableStreamDefaultController': Cannot close a stream that is closed or closing.");
    controllerClose(this);
  });
  method(ReadableStreamDefaultController.prototype, 'error', function error(e) {
    if (!RDC.get(this)) throw typeError('Illegal invocation');
    controllerError(this, e);
  });
  getter(ReadableStreamDefaultController.prototype, 'desiredSize', function () {
    var cs = RDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    return desiredSize(cs);
  });
  // Byte controller: the same queue, chunks are ArrayBufferViews.
  method(ReadableByteStreamController.prototype, 'enqueue', function enqueue(chunk) {
    var cs = RDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    if (!chunk || !isView(chunk)) throw typeError("Failed to execute 'enqueue' on 'ReadableByteStreamController': parameter 1 is not of type 'ArrayBufferView'.");
    if (chunk.byteLength === 0) throw typeError("Failed to execute 'enqueue' on 'ReadableByteStreamController': chunk is empty.");
    if (!canCloseOrEnqueue(cs)) throw typeError("Failed to execute 'enqueue' on 'ReadableByteStreamController': Cannot enqueue a chunk into a closed or errored stream.");
    controllerEnqueue(this, chunk);
  });
  method(ReadableByteStreamController.prototype, 'close', ReadableStreamDefaultController.prototype.close);
  method(ReadableByteStreamController.prototype, 'error', ReadableStreamDefaultController.prototype.error);
  getter(ReadableByteStreamController.prototype, 'desiredSize', function () {
    var cs = RDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    return desiredSize(cs);
  });
  getter(ReadableByteStreamController.prototype, 'byobRequest', function () { return null; });

  // Default reader.
  var ReadableStreamDefaultReader = function ReadableStreamDefaultReader(stream) {
    if (!(this instanceof ReadableStreamDefaultReader)) throw typeError("Failed to construct 'ReadableStreamDefaultReader': Please use the 'new' operator.");
    acquireDefaultReader(stream, this);
  };
  function genericInit(reader, stream, map) {
    var s = RS.get(stream);
    if (!s) throw typeError("Failed to construct the reader: parameter 1 is not of type 'ReadableStream'.");
    if (s.reader !== undefined) throw typeError("Failed to construct the reader: This stream has already been locked for exclusive reading by another reader");
    var rd = { stream: stream, requests: new Queue(), closed: deferred(), leftover: null };
    map.set(reader, rd);
    s.reader = reader;
    if (s.state === 'closed') rd.closed.resolve(undefined);
    else if (s.state === 'errored') { rd.closed.reject(s.storedError); markHandled(rd.closed.promise); }
    return rd;
  }
  function acquireDefaultReader(stream, reader) {
    reader = reader || Object.create(ReadableStreamDefaultReader.prototype);
    genericInit(reader, stream, RDR);
    return reader;
  }
  function defaultRead(reader, request) {
    var rd = RDR.get(reader);
    var s = RS.get(rd.stream);
    s.disturbed = true;
    if (s.state === 'closed') request.close();
    else if (s.state === 'errored') request.error(s.storedError);
    else pullSteps(s.controller, request);
  }
  function genericRelease(reader, map) {
    var rd = map.get(reader);
    var s = RS.get(rd.stream);
    var err = typeError('Reader was released');
    if (rd.closed.state === 'pending') { rd.closed.reject(err); markHandled(rd.closed.promise); }
    else rd.closed = settledRejected(err);
    s.reader = undefined;
    var reqs = rd.requests;
    rd.requests = new Queue();
    rd.stream = undefined;
    while (reqs.length() > 0) reqs.shift().error(typeError('Reader was released'));
  }
  method(ReadableStreamDefaultReader.prototype, 'read', function read() {
    var rd = RDR.get(this);
    if (!rd) return rejectedWith(typeError('Illegal invocation'));
    if (!rd.stream) return rejectedWith(typeError("Failed to execute 'read' on 'ReadableStreamDefaultReader': This readable stream reader has been released and cannot be used to read from its previous owner stream"));
    var d = deferred();
    defaultRead(this, {
      chunk: function (c) { d.resolve({ value: c, done: false }); },
      close: function () { d.resolve({ value: undefined, done: true }); },
      error: function (e) { d.reject(e); }
    });
    return d.promise;
  });
  method(ReadableStreamDefaultReader.prototype, 'releaseLock', function releaseLock() {
    var rd = RDR.get(this);
    if (!rd) throw typeError('Illegal invocation');
    if (!rd.stream) return;
    genericRelease(this, RDR);
  });
  method(ReadableStreamDefaultReader.prototype, 'cancel', function cancel(reason) {
    var rd = RDR.get(this);
    if (!rd) return rejectedWith(typeError('Illegal invocation'));
    if (!rd.stream) return rejectedWith(typeError("Failed to execute 'cancel' on 'ReadableStreamDefaultReader': This readable stream reader has been released and cannot be used to cancel its previous owner stream"));
    return readableCancel(rd.stream, reason);
  });
  getter(ReadableStreamDefaultReader.prototype, 'closed', function () {
    var rd = RDR.get(this);
    if (!rd) return rejectedWith(typeError('Illegal invocation'));
    return rd.closed.promise;
  });

  // BYOB reader over a byte stream: fills the given view from the queue.
  var ReadableStreamBYOBReader = function ReadableStreamBYOBReader(stream) {
    if (!(this instanceof ReadableStreamBYOBReader)) throw typeError("Failed to construct 'ReadableStreamBYOBReader': Please use the 'new' operator.");
    var s = RS.get(stream);
    if (!s || !RDC.get(s.controller).bytes) throw typeError("Failed to construct 'ReadableStreamBYOBReader': Cannot use a BYOB reader with a non-byte stream");
    genericInit(this, stream, RBR);
  };
  function fulfillByobRequests(cs, bd) {
    while (bd.requests.length() > 0 && cs.queue.length() > 0) {
      var req = bd.requests.shift();
      var view = req.view, filled = 0, target = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
      while (filled < target.length && cs.queue.length() > 0) {
        var chunk = cs.queue.peek();
        var src = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        var n = Math.min(src.length, target.length - filled);
        target.set(src.subarray(0, n), filled);
        filled += n;
        if (n === src.length) cs.queue.shift();
        else {
          cs.queue.items[cs.queue.head] = new Uint8Array(chunk.buffer, chunk.byteOffset + n, chunk.byteLength - n);
          cs.queue.sizes[cs.queue.head] -= n;
          cs.queue.total -= n;
        }
      }
      var Ctor = view.constructor, per = Ctor.BYTES_PER_ELEMENT || 1;
      req.chunk(new Ctor(view.buffer, view.byteOffset, Math.floor(filled / per)));
    }
    if (cs.closeRequested && cs.queue.length() === 0) { clearAlgorithms(cs); readableClose(cs.stream); }
  }
  method(ReadableStreamBYOBReader.prototype, 'read', function read(view) {
    var bd = RBR.get(this);
    if (!bd) return rejectedWith(typeError('Illegal invocation'));
    if (!view || !isView(view)) return rejectedWith(typeError("Failed to execute 'read' on 'ReadableStreamBYOBReader': parameter 1 is not of type 'ArrayBufferView'."));
    if (view.byteLength === 0) return rejectedWith(typeError("Failed to execute 'read' on 'ReadableStreamBYOBReader': This readable stream reader cannot be used to read as the view has byte length equal to 0"));
    if (!bd.stream) return rejectedWith(typeError("Failed to execute 'read' on 'ReadableStreamBYOBReader': This readable stream reader has been released"));
    var s = RS.get(bd.stream), cs = RDC.get(s.controller);
    s.disturbed = true;
    var d = deferred();
    if (s.state === 'errored') { d.reject(s.storedError); return d.promise; }
    var req = {
      view: view,
      chunk: function (v) { d.resolve({ value: v, done: false }); },
      close: function () { d.resolve({ value: new view.constructor(view.buffer, view.byteOffset, 0), done: true }); },
      error: function (e) { d.reject(e); }
    };
    if (s.state === 'closed') { req.close(); return d.promise; }
    bd.requests.push(req, 0);
    fulfillByobRequests(cs, bd);
    callPullIfNeeded(s.controller);
    return d.promise;
  });
  method(ReadableStreamBYOBReader.prototype, 'releaseLock', function releaseLock() {
    var bd = RBR.get(this);
    if (!bd) throw typeError('Illegal invocation');
    if (!bd.stream) return;
    genericRelease(this, RBR);
  });
  method(ReadableStreamBYOBReader.prototype, 'cancel', function cancel(reason) {
    var bd = RBR.get(this);
    if (!bd || !bd.stream) return rejectedWith(typeError('The reader is released'));
    return readableCancel(bd.stream, reason);
  });
  getter(ReadableStreamBYOBReader.prototype, 'closed', function () {
    var bd = RBR.get(this);
    if (!bd) return rejectedWith(typeError('Illegal invocation'));
    return bd.closed.promise;
  });

  // ReadableStream methods.
  var RSP = ReadableStream.prototype;
  getter(RSP, 'locked', function () { return rs(this, 'locked').reader !== undefined; });
  method(RSP, 'getReader', function getReader(options) {
    rs(this, 'getReader');
    var mode = options && options.mode;
    if (mode === undefined) return acquireDefaultReader(this);
    if (String(mode) !== 'byob') throw typeError("Failed to execute 'getReader' on 'ReadableStream': The provided value is not a valid enum value of type ReadableStreamReaderMode.");
    return new ReadableStreamBYOBReader(this);
  });
  method(RSP, 'cancel', function cancel(reason) {
    var s = RS.get(this);
    if (!s) return rejectedWith(typeError('Illegal invocation'));
    if (s.reader !== undefined) return rejectedWith(typeError("Failed to execute 'cancel' on 'ReadableStream': Cannot cancel a locked stream"));
    return readableCancel(this, reason);
  });
  // Create a readable stream from internal algorithms.
  function createReadable(start, pull, cancel, hwm, size) {
    var stream = Object.create(RSP);
    initReadable(stream);
    setupDefaultController(stream, start || noop, pull || function () { return resolvedWith(); }, cancel || function () { return resolvedWith(); },
      hwm === undefined ? 1 : hwm, size || function () { return 1; }, false);
    return stream;
  }
  method(RSP, 'tee', function tee() {
    rs(this, 'tee');
    var stream = this, reader = acquireDefaultReader(stream);
    var reading = false, readAgain = false, canceled1 = false, canceled2 = false, reason1, reason2, branch1, branch2;
    var cancelPromise = deferred();
    function pull() {
      if (reading) { readAgain = true; return resolvedWith(); }
      reading = true;
      defaultRead(reader, {
        chunk: function (chunk) {
          Promise.resolve().then(function () {
            readAgain = false;
            if (!canceled1) controllerEnqueue(RS.get(branch1).controller, chunk);
            if (!canceled2) controllerEnqueue(RS.get(branch2).controller, chunk);
            reading = false;
            if (readAgain) pull();
          });
        },
        close: function () {
          reading = false;
          if (!canceled1) controllerClose(RS.get(branch1).controller);
          if (!canceled2) controllerClose(RS.get(branch2).controller);
          if (!canceled1 || !canceled2) cancelPromise.resolve(undefined);
        },
        error: function () { reading = false; }
      });
      return resolvedWith();
    }
    function cancelBranch(which) {
      return function (reason) {
        if (which === 1) { canceled1 = true; reason1 = reason; } else { canceled2 = true; reason2 = reason; }
        if (canceled1 && canceled2) cancelPromise.resolve(readableCancel(stream, [reason1, reason2]));
        return cancelPromise.promise;
      };
    }
    branch1 = createReadable(noop, pull, cancelBranch(1));
    branch2 = createReadable(noop, pull, cancelBranch(2));
    RDR.get(reader).closed.promise.then(null, function (r) {
      controllerError(RS.get(branch1).controller, r);
      controllerError(RS.get(branch2).controller, r);
      if (!canceled1 || !canceled2) cancelPromise.resolve(undefined);
    });
    return [branch1, branch2];
  });
  // pipeTo (Streams §4.9.13 ReadableStreamPipeTo), with the shutdown rules.
  function pipe(source, dest, preventClose, preventAbort, preventCancel, signal) {
    var reader = acquireDefaultReader(source), writer = acquireWriter(dest);
    var rd = RDR.get(reader), wd = WDW.get(writer);
    RS.get(source).disturbed = true;
    var shuttingDown = false, currentWrite = resolvedWith(), done = deferred();
    var removeAbort = noop;
    function finalize(isError, error) {
      try { writerRelease(writer); } catch (e) {}
      try { genericRelease(reader, RDR); } catch (e) {}
      removeAbort();
      if (isError) done.reject(error); else done.resolve(undefined);
    }
    function waitForWrites() {
      var old = currentWrite;
      return old.then(function () { return old !== currentWrite ? waitForWrites() : undefined; });
    }
    function shutdownWithAction(action, isError, error) {
      if (shuttingDown) return;
      shuttingDown = true;
      var ws = WS.get(dest);
      var go = function () {
        promiseCall(action, undefined, []).then(function () { finalize(isError, error); }, function (e) { finalize(true, e); });
      };
      if (ws.state === 'writable' && !closeQueuedOrInFlight(dest)) waitForWrites().then(go, go); else go();
    }
    function shutdown(isError, error) {
      if (shuttingDown) return;
      shuttingDown = true;
      var ws = WS.get(dest);
      var fin = function () { finalize(isError, error); };
      if (ws.state === 'writable' && !closeQueuedOrInFlight(dest)) waitForWrites().then(fin, fin); else fin();
    }
    if (signal) {
      var abortAlgo = function () {
        var error = signal.reason;
        shutdownWithAction(function () {
          var actions = [];
          if (!preventAbort && WS.get(dest).state === 'writable') actions.push(writableAbort(dest, error));
          if (!preventCancel && RS.get(source).state === 'readable') actions.push(readableCancel(source, error));
          return Promise.all(actions);
        }, true, error);
      };
      if (signal.aborted) { abortAlgo(); return done.promise; }
      if (typeof g.__narsilAbortAlgorithm === 'function') removeAbort = g.__narsilAbortAlgorithm(signal, abortAlgo);
      else { signal.addEventListener('abort', abortAlgo); removeAbort = function () { signal.removeEventListener('abort', abortAlgo); }; }
    }
    // Error and close propagation (forward and backward).
    function checkStates() {
      var s = RS.get(source), w = WS.get(dest);
      if (s.state === 'errored') {
        if (!preventAbort) shutdownWithAction(function () { return writableAbort(dest, s.storedError); }, true, s.storedError);
        else shutdown(true, s.storedError);
        return true;
      }
      if (w.state === 'errored' || w.state === 'erroring') {
        if (!preventCancel) shutdownWithAction(function () { return readableCancel(source, w.storedError); }, true, w.storedError);
        else shutdown(true, w.storedError);
        return true;
      }
      if (s.state === 'closed') {
        if (!preventClose) shutdownWithAction(function () { return writerCloseWithErrorPropagation(writer); });
        else shutdown();
        return true;
      }
      if (closeQueuedOrInFlight(dest) || w.state === 'closed') {
        var err = typeError('the destination writable stream closed before all data could be piped to it');
        if (!preventCancel) shutdownWithAction(function () { return readableCancel(source, err); }, true, err);
        else shutdown(true, err);
        return true;
      }
      return false;
    }
    rd.closed.promise.then(function () { if (!shuttingDown) checkStates(); }, function () { if (!shuttingDown) checkStates(); });
    wd.closed.promise.then(function () { if (!shuttingDown) checkStates(); }, function () { if (!shuttingDown) checkStates(); });
    function step() {
      if (shuttingDown) return;
      if (checkStates()) return;
      wd.ready.promise.then(function () {
        if (shuttingDown || checkStates()) return;
        defaultRead(reader, {
          chunk: function (chunk) {
            currentWrite = markHandled(writerWrite(writer, chunk).then(noop, noop));
            step();
          },
          close: function () { checkStates(); },
          error: function () { checkStates(); }
        });
      }, function () { checkStates(); });
    }
    step();
    return done.promise;
  }
  method(RSP, 'pipeTo', function pipeTo(destination, options) {
    if (!RS.get(this)) return rejectedWith(typeError('Illegal invocation'));
    if (!WS.get(destination)) return rejectedWith(typeError("Failed to execute 'pipeTo' on 'ReadableStream': parameter 1 is not of type 'WritableStream'."));
    var o = options || {};
    var signal = o.signal;
    if (signal !== undefined && !(g.AbortSignal && signal instanceof g.AbortSignal)) return rejectedWith(typeError("Failed to execute 'pipeTo' on 'ReadableStream': Failed to read the 'signal' property from 'StreamPipeOptions': Failed to convert value to 'AbortSignal'."));
    if (isLocked(this)) return rejectedWith(typeError("Failed to execute 'pipeTo' on 'ReadableStream': Cannot pipe a locked stream"));
    if (WS.get(destination).writer !== undefined) return rejectedWith(typeError("Failed to execute 'pipeTo' on 'ReadableStream': Cannot pipe to a locked stream"));
    return pipe(this, destination, !!o.preventClose, !!o.preventAbort, !!o.preventCancel, signal);
  });
  method(RSP, 'pipeThrough', function pipeThrough(transform, options) {
    rs(this, 'pipeThrough');
    if (!isObject(transform)) throw typeError("Failed to execute 'pipeThrough' on 'ReadableStream': parameter 1 is not an object.");
    var writable = transform.writable, readable = transform.readable;
    if (!WS.get(writable)) throw typeError("Failed to execute 'pipeThrough' on 'ReadableStream': Failed to read the 'writable' property from 'ReadableWritablePair': Failed to convert value to 'WritableStream'.");
    if (!RS.get(readable)) throw typeError("Failed to execute 'pipeThrough' on 'ReadableStream': Failed to read the 'readable' property from 'ReadableWritablePair': Failed to convert value to 'ReadableStream'.");
    if (isLocked(this)) throw typeError("Failed to execute 'pipeThrough' on 'ReadableStream': Cannot pipe a locked stream");
    if (WS.get(writable).writer !== undefined) throw typeError("Failed to execute 'pipeThrough' on 'ReadableStream': Cannot pipe to a locked stream");
    var o = options || {};
    markHandled(pipe(this, writable, !!o.preventClose, !!o.preventAbort, !!o.preventCancel, o.signal));
    return readable;
  });
  // Async iteration.
  var asyncIterProto = {};
  if (typeof Symbol === 'function' && Symbol.asyncIterator) {
    try {
      var AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}.prototype));
      if (AsyncIteratorPrototype) asyncIterProto = Object.create(AsyncIteratorPrototype);
    } catch (e) {}
  }
  if (typeof Symbol === 'function' && Symbol.asyncIterator && typeof asyncIterProto[Symbol.asyncIterator] !== 'function') {
    method(asyncIterProto, Symbol.asyncIterator, function () { return this; });
  }
  var ITER = new WeakMap();
  method(asyncIterProto, 'next', function next() {
    var it = ITER.get(this);
    if (!it) return rejectedWith(typeError('Illegal invocation'));
    if (it.done) return resolvedWith({ value: undefined, done: true });
    var chain = it.ongoing.then(function () {
      if (it.done) return { value: undefined, done: true };
      var d = deferred();
      defaultRead(it.reader, {
        chunk: function (c) { d.resolve({ value: c, done: false }); },
        close: function () { it.done = true; try { genericRelease(it.reader, RDR); } catch (e) {} d.resolve({ value: undefined, done: true }); },
        error: function (e) { it.done = true; try { genericRelease(it.reader, RDR); } catch (x) {} d.reject(e); }
      });
      return d.promise;
    });
    it.ongoing = chain.then(noop, noop);
    return chain;
  });
  method(asyncIterProto, 'return', function (value) {
    var it = ITER.get(this);
    if (!it) return rejectedWith(typeError('Illegal invocation'));
    var chain = it.ongoing.then(function () {
      if (it.done) return { value: value, done: true };
      it.done = true;
      var rd = RDR.get(it.reader);
      if (!rd.stream) return { value: value, done: true };
      if (!it.preventCancel) {
        var p = readableCancel(rd.stream, value);
        genericRelease(it.reader, RDR);
        return p.then(function () { return { value: value, done: true }; });
      }
      genericRelease(it.reader, RDR);
      return { value: value, done: true };
    });
    it.ongoing = chain.then(noop, noop);
    return chain;
  });
  function values(options) {
    rs(this, 'values');
    var reader = acquireDefaultReader(this);
    var it = Object.create(asyncIterProto);
    ITER.set(it, { reader: reader, preventCancel: !!(options && options.preventCancel), done: false, ongoing: resolvedWith() });
    return it;
  }
  method(RSP, 'values', values);
  if (typeof Symbol === 'function' && Symbol.asyncIterator) method(RSP, Symbol.asyncIterator, values);
  // ReadableStream.from(asyncIterable | iterable).
  method(ReadableStream, 'from', function from(asyncIterable) {
    if (!isObject(asyncIterable) && typeof asyncIterable !== 'string') throw typeError("Failed to execute 'from' on 'ReadableStream': The provided value is not iterable.");
    if (RS.get(asyncIterable)) return asyncIterable;
    var it, nextFn;
    var am = Symbol.asyncIterator ? asyncIterable[Symbol.asyncIterator] : undefined;
    if (am !== undefined && am !== null) {
      it = apply(am, asyncIterable, []);
    } else {
      var sm = asyncIterable[Symbol.iterator];
      if (!isCallable(sm)) throw typeError("Failed to execute 'from' on 'ReadableStream': The provided value is not iterable.");
      var sit = apply(sm, asyncIterable, []);
      var snext = sit.next;
      it = {
        next: function () { var r = apply(snext, sit, []); return Promise.resolve(r.value).then(function (v) { return { value: v, done: r.done }; }); },
        'return': function (v) { var ret = sit['return']; return Promise.resolve(ret ? apply(ret, sit, [v]) : { done: true, value: v }); }
      };
    }
    if (!isObject(it)) throw typeError("Failed to execute 'from' on 'ReadableStream': The iterator is not an object.");
    nextFn = it.next;
    return createReadable(noop, function (c) {
      var p;
      try { p = Promise.resolve(apply(nextFn, it, [])); } catch (e) { return rejectedWith(e); }
      return p.then(function (r) {
        if (!isObject(r)) throw typeError('The iterator result is not an object');
        if (r.done) controllerClose(c); else controllerEnqueue(c, r.value);
      });
    }, function (reason) {
      var ret;
      try { ret = it['return']; } catch (e) { return rejectedWith(e); }
      if (ret === undefined || ret === null) return resolvedWith();
      var p;
      try { p = Promise.resolve(apply(ret, it, [reason])); } catch (e) { return rejectedWith(e); }
      return p.then(function (r) { if (!isObject(r)) throw typeError('The iterator return() result is not an object'); });
    }, 0);
  });

  // ===================== WritableStream =====================
  var WS = new WeakMap(), WDC = new WeakMap(), WDW = new WeakMap();
  var CLOSE_SENTINEL = {};
  var WritableStream = function WritableStream(underlyingSink, strategy) {
    if (!(this instanceof WritableStream)) throw typeError("Failed to construct 'WritableStream': Please use the 'new' operator.");
    var sink = underlyingSink === undefined ? null : underlyingSink;
    if (sink !== null && !isObject(sink)) throw typeError("Failed to construct 'WritableStream': The provided value is not an object.");
    if (sink && sink.type !== undefined) throw new RangeError("Failed to construct 'WritableStream': Invalid type is specified");
    var hwm = highWaterMark(strategy, 1), size = sizeAlgorithm(strategy);
    var start = getMethod(sink, 'start', 'WritableStream'), write = getMethod(sink, 'write', 'WritableStream');
    var close = getMethod(sink, 'close', 'WritableStream'), abort = getMethod(sink, 'abort', 'WritableStream');
    initWritable(this);
    setupWritableController(this, function (c) { return start ? apply(start, sink, [c]) : undefined; },
      function (chunk, c) { return promiseCall(write, sink, [chunk, c]); },
      function () { return promiseCall(close, sink, []); },
      function (r) { return promiseCall(abort, sink, [r]); }, hwm, size);
  };
  function initWritable(stream) {
    WS.set(stream, { state: 'writable', storedError: undefined, writer: undefined, controller: undefined, inFlightWrite: undefined,
      closeRequest: undefined, inFlightClose: undefined, pendingAbort: undefined, writeRequests: new Queue(), backpressure: false });
  }
  var WritableStreamDefaultController = function WritableStreamDefaultController() { throw typeError('Illegal constructor'); };
  function setupWritableController(stream, startAlgo, writeAlgo, closeAlgo, abortAlgo, hwm, size) {
    var c = Object.create(WritableStreamDefaultController.prototype);
    var cs = { stream: stream, queue: new Queue(), started: false, size: size, hwm: hwm, write: writeAlgo, close: closeAlgo,
      abort: abortAlgo, abortController: g.AbortController ? new g.AbortController() : null };
    WDC.set(c, cs);
    var ws = WS.get(stream);
    ws.controller = c;
    updateBackpressure(stream, getBackpressure(cs));
    var startResult = startAlgo(c);
    Promise.resolve(startResult).then(function () {
      cs.started = true;
      advanceQueueIfNeeded(c);
    }, function (r) {
      cs.started = true;
      dealWithRejection(stream, r);
    });
    return c;
  }
  function wDesiredSize(cs) { return cs.hwm - cs.queue.total; }
  function getBackpressure(cs) { return wDesiredSize(cs) <= 0; }
  function closeQueuedOrInFlight(stream) { var s = WS.get(stream); return s.closeRequest !== undefined || s.inFlightClose !== undefined; }
  function hasOperationInFlight(s) { return s.inFlightWrite !== undefined || s.inFlightClose !== undefined; }
  function updateBackpressure(stream, bp) {
    var s = WS.get(stream);
    if (s.writer !== undefined && bp !== s.backpressure) {
      var wd = WDW.get(s.writer);
      if (bp) wd.ready = deferred(); else wd.ready.resolve(undefined);
    }
    s.backpressure = bp;
  }
  function ensureClosedRejected(wd, e) {
    if (wd.closed.state === 'pending') { wd.closed.reject(e); markHandled(wd.closed.promise); }
    else wd.closed = settledRejected(e);
  }
  function wClearAlgorithms(cs) { cs.write = null; cs.close = null; cs.abort = null; cs.size = function () { return 1; }; }
  function advanceQueueIfNeeded(c) {
    var cs = WDC.get(c), stream = cs.stream, s = WS.get(stream);
    if (!cs.started || s.inFlightWrite !== undefined) return;
    if (s.state === 'erroring') { finishErroring(stream); return; }
    if (cs.queue.length() === 0) return;
    var value = cs.queue.peek();
    if (value === CLOSE_SENTINEL) processClose(c); else processWrite(c, value);
  }
  function processWrite(c, chunk) {
    var cs = WDC.get(c), stream = cs.stream, s = WS.get(stream);
    s.inFlightWrite = s.writeRequests.shift();
    cs.write(chunk, c).then(function () {
      s.inFlightWrite.resolve(undefined);
      s.inFlightWrite = undefined;
      cs.queue.shift();
      if (!closeQueuedOrInFlight(stream) && s.state === 'writable') updateBackpressure(stream, getBackpressure(cs));
      advanceQueueIfNeeded(c);
    }, function (reason) {
      if (s.state === 'writable') wClearAlgorithms(cs);
      s.inFlightWrite.reject(reason);
      s.inFlightWrite = undefined;
      dealWithRejection(stream, reason);
    });
  }
  function processClose(c) {
    var cs = WDC.get(c), stream = cs.stream, s = WS.get(stream);
    s.inFlightClose = s.closeRequest;
    s.closeRequest = undefined;
    cs.queue.shift();
    var p = cs.close();
    wClearAlgorithms(cs);
    p.then(function () {
      s.inFlightClose.resolve(undefined);
      s.inFlightClose = undefined;
      if (s.state === 'erroring') {
        s.storedError = undefined;
        if (s.pendingAbort) { s.pendingAbort.d.resolve(undefined); s.pendingAbort = undefined; }
      }
      s.state = 'closed';
      if (s.writer) WDW.get(s.writer).closed.resolve(undefined);
    }, function (reason) {
      s.inFlightClose.reject(reason);
      s.inFlightClose = undefined;
      if (s.pendingAbort) { s.pendingAbort.d.reject(reason); s.pendingAbort = undefined; }
      dealWithRejection(stream, reason);
    });
  }
  function dealWithRejection(stream, error) {
    var s = WS.get(stream);
    if (s.state === 'writable') { startErroring(stream, error); return; }
    finishErroring(stream);
  }
  function startErroring(stream, reason) {
    var s = WS.get(stream), cs = WDC.get(s.controller);
    s.state = 'erroring';
    s.storedError = reason;
    if (s.writer) ensureReadyRejected(WDW.get(s.writer), reason);
    if (!hasOperationInFlight(s) && cs.started) finishErroring(stream);
  }
  function finishErroring(stream) {
    var s = WS.get(stream), cs = WDC.get(s.controller);
    s.state = 'errored';
    cs.queue.reset();
    var err = s.storedError;
    var reqs = s.writeRequests;
    s.writeRequests = new Queue();
    while (reqs.length() > 0) reqs.shift().reject(err);
    if (!s.pendingAbort) { rejectCloseAndClosed(stream); return; }
    var ab = s.pendingAbort;
    s.pendingAbort = undefined;
    if (ab.wasAlreadyErroring) { ab.d.reject(err); rejectCloseAndClosed(stream); return; }
    var abort = cs.abort;
    wClearAlgorithms(cs);
    var p = abort ? abort(ab.reason) : resolvedWith();
    p.then(function () { ab.d.resolve(undefined); rejectCloseAndClosed(stream); },
      function (r) { ab.d.reject(r); rejectCloseAndClosed(stream); });
  }
  function rejectCloseAndClosed(stream) {
    var s = WS.get(stream);
    if (s.closeRequest !== undefined) { s.closeRequest.reject(s.storedError); s.closeRequest = undefined; }
    if (s.writer) ensureClosedRejected(WDW.get(s.writer), s.storedError);
  }
  function ensureReadyRejected(wd, e) {
    if (wd.ready.state === 'pending') { wd.ready.reject(e); markHandled(wd.ready.promise); }
    else wd.ready = settledRejected(e);
  }
  function writableAbort(stream, reason) {
    var s = WS.get(stream);
    if (s.state === 'closed' || s.state === 'errored') return resolvedWith(undefined);
    var cs = WDC.get(s.controller);
    if (cs.abortController) { try { cs.abortController.abort(reason); } catch (e) {} }
    if (s.state === 'closed' || s.state === 'errored') return resolvedWith(undefined);
    if (s.pendingAbort) return s.pendingAbort.d.promise;
    var wasAlreadyErroring = s.state === 'erroring';
    if (wasAlreadyErroring) reason = undefined;
    var d = deferred();
    s.pendingAbort = { d: d, reason: reason, wasAlreadyErroring: wasAlreadyErroring };
    if (!wasAlreadyErroring) startErroring(stream, reason);
    return d.promise;
  }
  function writableClose(stream) {
    var s = WS.get(stream);
    if (s.state === 'closed' || s.state === 'errored') return rejectedWith(typeError('The stream is closed or errored'));
    var d = deferred();
    s.closeRequest = d;
    if (s.writer && s.backpressure && s.state === 'writable') WDW.get(s.writer).ready.resolve(undefined);
    var cs = WDC.get(s.controller);
    cs.queue.push(CLOSE_SENTINEL, 0);
    advanceQueueIfNeeded(s.controller);
    return d.promise;
  }
  function controllerErrorIfNeeded(cs, e) {
    if (WS.get(cs.stream).state === 'writable') { wClearAlgorithms(cs); startErroring(cs.stream, e); }
  }
  method(WritableStreamDefaultController.prototype, 'error', function error(e) {
    var cs = WDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    if (WS.get(cs.stream).state !== 'writable') return;
    wClearAlgorithms(cs);
    startErroring(cs.stream, e);
  });
  getter(WritableStreamDefaultController.prototype, 'signal', function () {
    var cs = WDC.get(this);
    if (!cs) throw typeError('Illegal invocation');
    return cs.abortController ? cs.abortController.signal : undefined;
  });
  // Writer.
  var WritableStreamDefaultWriter = function WritableStreamDefaultWriter(stream) {
    if (!(this instanceof WritableStreamDefaultWriter)) throw typeError("Failed to construct 'WritableStreamDefaultWriter': Please use the 'new' operator.");
    acquireWriter(stream, this);
  };
  function acquireWriter(stream, writer) {
    var s = WS.get(stream);
    if (!s) throw typeError("Failed to construct 'WritableStreamDefaultWriter': parameter 1 is not of type 'WritableStream'.");
    if (s.writer !== undefined) throw typeError("Failed to construct 'WritableStreamDefaultWriter': Cannot create writer when WritableStream is locked");
    writer = writer || Object.create(WritableStreamDefaultWriter.prototype);
    var wd = { stream: stream, ready: deferred(), closed: deferred() };
    WDW.set(writer, wd);
    s.writer = writer;
    if (s.state === 'writable') {
      if (!closeQueuedOrInFlight(stream) && s.backpressure) {} else wd.ready.resolve(undefined);
    } else if (s.state === 'erroring') {
      wd.ready = settledRejected(s.storedError);
    } else if (s.state === 'closed') {
      wd.ready.resolve(undefined); wd.closed.resolve(undefined);
    } else {
      wd.ready = settledRejected(s.storedError);
      wd.closed = settledRejected(s.storedError);
    }
    return writer;
  }
  function writerWrite(writer, chunk) {
    var wd = WDW.get(writer), stream = wd.stream, s = WS.get(stream), cs = WDC.get(s.controller);
    var size;
    try { size = cs.size(chunk); } catch (e) { controllerErrorIfNeeded(cs, e); size = 1; }
    if (stream !== wd.stream) return rejectedWith(typeError('The writer is released'));
    if (s.state === 'errored') return rejectedWith(s.storedError);
    if (closeQueuedOrInFlight(stream) || s.state === 'closed') return rejectedWith(typeError('The stream is closing or closed and cannot be written to'));
    if (s.state === 'erroring') return rejectedWith(s.storedError);
    var d = deferred();
    s.writeRequests.push(d, 0);
    try { cs.queue.push(chunk, size); } catch (e) { controllerErrorIfNeeded(cs, e); return d.promise; }
    if (!closeQueuedOrInFlight(stream) && s.state === 'writable') updateBackpressure(stream, getBackpressure(cs));
    advanceQueueIfNeeded(s.controller);
    return d.promise;
  }
  function writerRelease(writer) {
    var wd = WDW.get(writer);
    var stream = wd.stream;
    if (!stream) return;
    var s = WS.get(stream);
    var err = typeError("Writer was released and can no longer be used to monitor the stream's closedness");
    ensureReadyRejected(wd, err);
    ensureClosedRejected(wd, err);
    s.writer = undefined;
    wd.stream = undefined;
  }
  function writerCloseWithErrorPropagation(writer) {
    var wd = WDW.get(writer), stream = wd.stream, s = WS.get(stream);
    if (closeQueuedOrInFlight(stream) || s.state === 'closed') return resolvedWith(undefined);
    if (s.state === 'errored') return rejectedWith(s.storedError);
    return writableClose(stream);
  }
  var WWP = WritableStreamDefaultWriter.prototype;
  method(WWP, 'write', function write(chunk) {
    var wd = WDW.get(this);
    if (!wd) return rejectedWith(typeError('Illegal invocation'));
    if (!wd.stream) return rejectedWith(typeError("Failed to execute 'write' on 'WritableStreamDefaultWriter': This writable stream writer has been released and cannot be used to write to its previous owner stream"));
    return writerWrite(this, chunk);
  });
  method(WWP, 'close', function close() {
    var wd = WDW.get(this);
    if (!wd) return rejectedWith(typeError('Illegal invocation'));
    if (!wd.stream) return rejectedWith(typeError("Failed to execute 'close' on 'WritableStreamDefaultWriter': This writable stream writer has been released and cannot be used to close its previous owner stream"));
    if (closeQueuedOrInFlight(wd.stream)) return rejectedWith(typeError("Failed to execute 'close' on 'WritableStreamDefaultWriter': Cannot close a writable stream that has already been requested to be closed"));
    return writableClose(wd.stream);
  });
  method(WWP, 'abort', function abort(reason) {
    var wd = WDW.get(this);
    if (!wd) return rejectedWith(typeError('Illegal invocation'));
    if (!wd.stream) return rejectedWith(typeError("Failed to execute 'abort' on 'WritableStreamDefaultWriter': This writable stream writer has been released and cannot be used to abort its previous owner stream"));
    return writableAbort(wd.stream, reason);
  });
  method(WWP, 'releaseLock', function releaseLock() {
    var wd = WDW.get(this);
    if (!wd) throw typeError('Illegal invocation');
    writerRelease(this);
  });
  getter(WWP, 'closed', function () { var wd = WDW.get(this); return wd ? wd.closed.promise : rejectedWith(typeError('Illegal invocation')); });
  getter(WWP, 'ready', function () { var wd = WDW.get(this); return wd ? wd.ready.promise : rejectedWith(typeError('Illegal invocation')); });
  getter(WWP, 'desiredSize', function () {
    var wd = WDW.get(this);
    if (!wd) throw typeError('Illegal invocation');
    if (!wd.stream) throw typeError("Failed to read the 'desiredSize' property from 'WritableStreamDefaultWriter': This writable stream writer has been released");
    var s = WS.get(wd.stream);
    if (s.state === 'errored' || s.state === 'erroring') return null;
    if (s.state === 'closed') return 0;
    return wDesiredSize(WDC.get(s.controller));
  });
  var WSP = WritableStream.prototype;
  getter(WSP, 'locked', function () { var s = WS.get(this); if (!s) throw typeError('Illegal invocation'); return s.writer !== undefined; });
  method(WSP, 'getWriter', function getWriter() { if (!WS.get(this)) throw typeError('Illegal invocation'); return acquireWriter(this); });
  method(WSP, 'abort', function abort(reason) {
    var s = WS.get(this);
    if (!s) return rejectedWith(typeError('Illegal invocation'));
    if (s.writer !== undefined) return rejectedWith(typeError("Failed to execute 'abort' on 'WritableStream': Cannot abort a locked stream"));
    return writableAbort(this, reason);
  });
  method(WSP, 'close', function close() {
    var s = WS.get(this);
    if (!s) return rejectedWith(typeError('Illegal invocation'));
    if (s.writer !== undefined) return rejectedWith(typeError("Failed to execute 'close' on 'WritableStream': Cannot close a locked stream"));
    if (closeQueuedOrInFlight(this)) return rejectedWith(typeError("Failed to execute 'close' on 'WritableStream': Cannot close a writable stream that has already been requested to be closed"));
    return writableClose(this);
  });

  // ===================== TransformStream =====================
  var TS = new WeakMap(), TSC = new WeakMap();
  var TransformStreamDefaultController = function TransformStreamDefaultController() { throw typeError('Illegal constructor'); };
  var TransformStream = function TransformStream(transformer, writableStrategy, readableStrategy) {
    if (!(this instanceof TransformStream)) throw typeError("Failed to construct 'TransformStream': Please use the 'new' operator.");
    var tr = transformer === undefined ? null : transformer;
    if (tr !== null && !isObject(tr)) throw typeError("Failed to construct 'TransformStream': The provided value is not an object.");
    if (tr && tr.readableType !== undefined) throw new RangeError("Failed to construct 'TransformStream': Invalid readableType is specified");
    if (tr && tr.writableType !== undefined) throw new RangeError("Failed to construct 'TransformStream': Invalid writableType is specified");
    var rHwm = highWaterMark(readableStrategy, 0), rSize = sizeAlgorithm(readableStrategy);
    var wHwm = highWaterMark(writableStrategy, 1), wSize = sizeAlgorithm(writableStrategy);
    var start = getMethod(tr, 'start', 'TransformStream'), transform = getMethod(tr, 'transform', 'TransformStream');
    var flush = getMethod(tr, 'flush', 'TransformStream'), cancel = getMethod(tr, 'cancel', 'TransformStream');
    var startD = deferred();
    var ts = initTransform(this, startD.promise, wHwm, wSize, rHwm, rSize);
    var c = Object.create(TransformStreamDefaultController.prototype);
    TSC.set(c, { stream: this });
    ts.controller = c;
    ts.transform = transform ? function (chunk) { return promiseCall(transform, tr, [chunk, c]); }
      : function (chunk) { try { tEnqueue(c, chunk); return resolvedWith(); } catch (e) { return rejectedWith(e); } };
    ts.flush = function () { return promiseCall(flush, tr, [c]); };
    ts.cancel = function (r) { return promiseCall(cancel, tr, [r]); };
    var startResult = start ? apply(start, tr, [c]) : undefined;
    startD.resolve(startResult);
  };
  function initTransform(stream, startPromise, wHwm, wSize, rHwm, rSize) {
    var ts = { readable: null, writable: null, backpressure: undefined, bpChange: null, controller: null, transform: null, flush: null, cancel: null, finish: null };
    TS.set(stream, ts);
    var w = Object.create(WSP);
    initWritable(w);
    setupWritableController(w, function () { return startPromise; },
      function (chunk) { return sinkWrite(stream, chunk); },
      function () { return sinkClose(stream); },
      function (reason) { return sinkAbort(stream, reason); }, wHwm, wSize);
    ts.writable = w;
    ts.readable = createReadable(function () { return startPromise; }, function () { return sourcePull(stream); },
      function (reason) { return sourceCancel(stream, reason); }, rHwm, rSize);
    setBackpressure(stream, true);
    return ts;
  }
  function setBackpressure(stream, bp) {
    var ts = TS.get(stream);
    if (ts.bpChange) ts.bpChange.resolve(undefined);
    ts.bpChange = deferred();
    ts.backpressure = bp;
  }
  function readableControllerOf(ts) { return RS.get(ts.readable).controller; }
  function errorWritableAndUnblockWrite(stream, e) {
    var ts = TS.get(stream);
    ts.transform = function () { return resolvedWith(); };
    ts.flush = function () { return resolvedWith(); };
    var ws = WS.get(ts.writable);
    controllerErrorIfNeeded(WDC.get(ws.controller), e);
    if (ts.backpressure) setBackpressure(stream, false);
  }
  function transformError(stream, e) {
    var ts = TS.get(stream);
    controllerError(readableControllerOf(ts), e);
    errorWritableAndUnblockWrite(stream, e);
  }
  function tEnqueue(c, chunk) {
    var stream = TSC.get(c).stream, ts = TS.get(stream), rc = readableControllerOf(ts), rcs = RDC.get(rc);
    if (!canCloseOrEnqueue(rcs)) throw typeError("Failed to execute 'enqueue' on 'TransformStreamDefaultController': Readable side is not in a state that permits enqueue");
    try { controllerEnqueue(rc, chunk); } catch (e) { errorWritableAndUnblockWrite(stream, e); throw RS.get(ts.readable).storedError; }
    var bp = !shouldCallPull(rc) && desiredSize(rcs) <= 0;
    if (bp !== ts.backpressure && bp) setBackpressure(stream, true);
  }
  function performTransform(stream, chunk) {
    var ts = TS.get(stream);
    return ts.transform(chunk).then(null, function (r) { transformError(stream, r); throw r; });
  }
  function sinkWrite(stream, chunk) {
    var ts = TS.get(stream);
    if (ts.backpressure) {
      return ts.bpChange.promise.then(function () {
        var ws = WS.get(ts.writable);
        if (ws.state === 'erroring') throw ws.storedError;
        return performTransform(stream, chunk);
      });
    }
    return performTransform(stream, chunk);
  }
  function sinkAbort(stream, reason) {
    var ts = TS.get(stream);
    if (ts.finish) return ts.finish.promise;
    var readable = ts.readable;
    ts.finish = deferred();
    var p = ts.cancel(reason);
    ts.transform = function () { return resolvedWith(); };
    ts.flush = function () { return resolvedWith(); };
    ts.cancel = function () { return resolvedWith(); };
    p.then(function () {
      var rs0 = RS.get(readable);
      if (rs0.state === 'errored') ts.finish.reject(rs0.storedError);
      else { controllerError(rs0.controller, reason); ts.finish.resolve(undefined); }
    }, function (r) { controllerError(RS.get(readable).controller, r); ts.finish.reject(r); });
    return ts.finish.promise;
  }
  function sinkClose(stream) {
    var ts = TS.get(stream);
    if (ts.finish) return ts.finish.promise;
    var readable = ts.readable;
    ts.finish = deferred();
    var p = ts.flush();
    ts.transform = function () { return resolvedWith(); };
    ts.flush = function () { return resolvedWith(); };
    ts.cancel = function () { return resolvedWith(); };
    p.then(function () {
      var rs0 = RS.get(readable);
      if (rs0.state === 'errored') { ts.finish.reject(rs0.storedError); return; }
      controllerClose(rs0.controller);
      ts.finish.resolve(undefined);
    }, function (r) { controllerError(RS.get(readable).controller, r); ts.finish.reject(r); });
    return ts.finish.promise;
  }
  function sourcePull(stream) {
    setBackpressure(stream, false);
    return TS.get(stream).bpChange.promise;
  }
  function sourceCancel(stream, reason) {
    var ts = TS.get(stream);
    if (ts.finish) return ts.finish.promise;
    var writable = ts.writable;
    ts.finish = deferred();
    var p = ts.cancel(reason);
    ts.transform = function () { return resolvedWith(); };
    ts.flush = function () { return resolvedWith(); };
    ts.cancel = function () { return resolvedWith(); };
    p.then(function () {
      var ws = WS.get(writable);
      if (ws.state === 'errored') ts.finish.reject(ws.storedError);
      else { controllerErrorIfNeeded(WDC.get(ws.controller), reason); if (ts.backpressure) setBackpressure(stream, false); ts.finish.resolve(undefined); }
    }, function (r) {
      controllerErrorIfNeeded(WDC.get(WS.get(writable).controller), r);
      if (ts.backpressure) setBackpressure(stream, false);
      ts.finish.reject(r);
    });
    return ts.finish.promise;
  }
  var TSCP = TransformStreamDefaultController.prototype;
  method(TSCP, 'enqueue', function enqueue(chunk) { if (!TSC.get(this)) throw typeError('Illegal invocation'); tEnqueue(this, chunk); });
  method(TSCP, 'error', function error(e) { var t = TSC.get(this); if (!t) throw typeError('Illegal invocation'); transformError(t.stream, e); });
  method(TSCP, 'terminate', function terminate() {
    var t = TSC.get(this);
    if (!t) throw typeError('Illegal invocation');
    var ts = TS.get(t.stream);
    controllerClose(readableControllerOf(ts));
    errorWritableAndUnblockWrite(t.stream, typeError('TransformStream terminated'));
  });
  getter(TSCP, 'desiredSize', function () {
    var t = TSC.get(this);
    if (!t) throw typeError('Illegal invocation');
    return desiredSize(RDC.get(readableControllerOf(TS.get(t.stream))));
  });
  getter(TransformStream.prototype, 'readable', function () { var ts = TS.get(this); if (!ts) throw typeError('Illegal invocation'); return ts.readable; });
  getter(TransformStream.prototype, 'writable', function () { var ts = TS.get(this); if (!ts) throw typeError('Illegal invocation'); return ts.writable; });

  // ===================== queuing strategies =====================
  var CountQueuingStrategy = function CountQueuingStrategy(init) {
    if (!(this instanceof CountQueuingStrategy)) throw typeError("Failed to construct 'CountQueuingStrategy': Please use the 'new' operator.");
    if (!isObject(init) || init.highWaterMark === undefined) throw typeError("Failed to construct 'CountQueuingStrategy': Failed to read the 'highWaterMark' property from 'QueuingStrategyInit': Required member is undefined.");
    var h = Number(init.highWaterMark);
    try { Object.defineProperty(this, '__hwm', { value: h }); } catch (e) {}
  };
  getter(CountQueuingStrategy.prototype, 'highWaterMark', function () { return this.__hwm; });
  var countSize = function size() { return 1; };
  getter(CountQueuingStrategy.prototype, 'size', function () { return countSize; });
  var ByteLengthQueuingStrategy = function ByteLengthQueuingStrategy(init) {
    if (!(this instanceof ByteLengthQueuingStrategy)) throw typeError("Failed to construct 'ByteLengthQueuingStrategy': Please use the 'new' operator.");
    if (!isObject(init) || init.highWaterMark === undefined) throw typeError("Failed to construct 'ByteLengthQueuingStrategy': Failed to read the 'highWaterMark' property from 'QueuingStrategyInit': Required member is undefined.");
    var h = Number(init.highWaterMark);
    try { Object.defineProperty(this, '__hwm', { value: h }); } catch (e) {}
  };
  getter(ByteLengthQueuingStrategy.prototype, 'highWaterMark', function () { return this.__hwm; });
  var byteSize = function size(chunk) { return chunk.byteLength; };
  getter(ByteLengthQueuingStrategy.prototype, 'size', function () { return byteSize; });

  // ===================== encoding streams =====================
  var TextEncoderStream = function TextEncoderStream() {
    if (!(this instanceof TextEncoderStream)) throw typeError("Failed to construct 'TextEncoderStream': Please use the 'new' operator.");
    var enc = new g.TextEncoder(), pending = '';
    var t = new TransformStream({
      transform: function (chunk, c) {
        var s = pending + String(chunk);
        pending = '';
        // A trailing high surrogate waits for its pair.
        var last = s.charCodeAt(s.length - 1);
        if (last >= 0xD800 && last <= 0xDBFF) { pending = s.charAt(s.length - 1); s = s.slice(0, -1); }
        if (s) c.enqueue(enc.encode(s));
      },
      flush: function (c) { if (pending) c.enqueue(enc.encode('�')); }
    });
    try { Object.defineProperty(this, '__t', { value: t }); } catch (e) {}
  };
  getter(TextEncoderStream.prototype, 'encoding', function () { return 'utf-8'; });
  getter(TextEncoderStream.prototype, 'readable', function () { return this.__t.readable; });
  getter(TextEncoderStream.prototype, 'writable', function () { return this.__t.writable; });
  var TextDecoderStream = function TextDecoderStream(label, options) {
    if (!(this instanceof TextDecoderStream)) throw typeError("Failed to construct 'TextDecoderStream': Please use the 'new' operator.");
    var dec = new g.TextDecoder(label === undefined ? 'utf-8' : label, options);
    var t = new TransformStream({
      transform: function (chunk, c) {
        if (!chunk || (!isView(chunk) && !(chunk instanceof ArrayBuffer))) throw typeError("Failed to execute 'transform': The provided value is not of type '(ArrayBuffer or ArrayBufferView)'");
        var s = dec.decode(chunk, { stream: true });
        if (s) c.enqueue(s);
      },
      flush: function (c) { var s = dec.decode(); if (s) c.enqueue(s); }
    });
    try { Object.defineProperty(this, '__t', { value: t }); } catch (e) {}
    try { Object.defineProperty(this, '__d', { value: dec }); } catch (e) {}
  };
  getter(TextDecoderStream.prototype, 'encoding', function () { return this.__d.encoding; });
  getter(TextDecoderStream.prototype, 'fatal', function () { return this.__d.fatal; });
  getter(TextDecoderStream.prototype, 'ignoreBOM', function () { return this.__d.ignoreBOM; });
  getter(TextDecoderStream.prototype, 'readable', function () { return this.__t.readable; });
  getter(TextDecoderStream.prototype, 'writable', function () { return this.__t.writable; });

  defineClass(ReadableStream, 'ReadableStream');
  defineClass(ReadableStreamDefaultReader, 'ReadableStreamDefaultReader');
  defineClass(ReadableStreamBYOBReader, 'ReadableStreamBYOBReader');
  defineClass(ReadableStreamDefaultController, 'ReadableStreamDefaultController');
  defineClass(ReadableByteStreamController, 'ReadableByteStreamController');
  defineClass(WritableStream, 'WritableStream');
  defineClass(WritableStreamDefaultWriter, 'WritableStreamDefaultWriter');
  defineClass(WritableStreamDefaultController, 'WritableStreamDefaultController');
  defineClass(TransformStream, 'TransformStream');
  defineClass(TransformStreamDefaultController, 'TransformStreamDefaultController');
  defineClass(CountQueuingStrategy, 'CountQueuingStrategy');
  defineClass(ByteLengthQueuingStrategy, 'ByteLengthQueuingStrategy');
  if (typeof g.TextEncoder === 'function') defineClass(TextEncoderStream, 'TextEncoderStream');
  if (typeof g.TextDecoder === 'function') defineClass(TextDecoderStream, 'TextDecoderStream');

  // ===================== body streams =====================
  // Response.body / Request.body / Blob.stream(): a byte stream whose first
  // pull reads the whole body once (the engine delivers bodies whole); the
  // chunk is the body's own buffer, not a copy.
  function bodyStream(owner, read) {
    return createByteStream(function (c) {
      return Promise.resolve(read(owner)).then(function (buf) {
        var u = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
        var cs = RDC.get(c);
        if (u.byteLength > 0 && canCloseOrEnqueue(cs)) controllerEnqueue(c, u);
        if (canCloseOrEnqueue(cs)) controllerClose(c);
      });
    });
  }
  function createByteStream(pullOnce) {
    var stream = Object.create(RSP), pulled = false;
    initReadable(stream);
    setupDefaultController(stream, noop, function (c) {
      if (pulled) return resolvedWith();
      pulled = true;
      return pullOnce(c);
    }, function () { return resolvedWith(); }, 0, function (c) { return c.byteLength; }, true);
    return stream;
  }
  var BODY = new WeakMap();
  // The body as bytes with what the object offers: arrayBuffer(), blob(),
  // or text() encoded as UTF-8.
  function bodyBytes(o) {
    if (typeof o.arrayBuffer === 'function') return function (x) { return x.arrayBuffer(); };
    if (typeof o.blob === 'function') return function (x) { return x.blob().then(function (b) { return b.arrayBuffer(); }); };
    if (typeof o.text === 'function' && typeof g.TextEncoder === 'function') {
      return function (x) { return x.text().then(function (t) { return new g.TextEncoder().encode(t); }); };
    }
    return null;
  }
  function bodyGetter(Proto, nativeDesc) {
    if (!Proto) return;
    try {
      Object.defineProperty(Proto, 'body', {
        configurable: true, enumerable: true,
        get: function () {
          if (BODY.has(this)) return BODY.get(this);
          var nativeValue = nativeDesc && nativeDesc.get ? apply(nativeDesc.get, this, []) : undefined;
          if (nativeValue && RS.get(nativeValue)) { BODY.set(this, nativeValue); return nativeValue; }
          var read = bodyReaders.get(Proto) || bodyBytes(this);
          if (!read) return null;
          var s = bodyStream(this, read);
          BODY.set(this, s);
          return s;
        }
      });
    } catch (e) {}
  }
  // ---- Body mixin (Fetch §6.2) over the native Response/Request ----
  // bodyUsed (also after reading the body stream), a second read rejecting
  // with TypeError, blob()/bytes()/formData()/clone() where missing, and
  // `new Response(readableStream)`. Reads go to the native methods once;
  // a clone shares one cached copy of the bytes.
  var USED = new WeakSet();
  var CACHED = new WeakMap();
  var STREAM_SRC = new WeakMap();
  function bytesOfChunk(c) {
    if (typeof c === 'string') return new g.TextEncoder().encode(c);
    if (c instanceof ArrayBuffer) return new Uint8Array(c);
    if (isView(c)) return new Uint8Array(c.buffer, c.byteOffset, c.byteLength);
    throw typeError('The body stream produced a chunk that is not a BufferSource');
  }
  function readAllBytes(stream) {
    var reader = acquireDefaultReader(stream), chunks = [], total = 0;
    return (function pump() {
      var d = deferred();
      defaultRead(reader, {
        chunk: function (c) { try { var u = bytesOfChunk(c); chunks.push(u); total += u.byteLength; d.resolve(false); } catch (e) { d.reject(e); } },
        close: function () { d.resolve(true); },
        error: function (e) { d.reject(e); }
      });
      return d.promise.then(function (done) {
        if (!done) return pump();
        try { genericRelease(reader, RDR); } catch (e) {}
        if (chunks.length === 1 && chunks[0].byteOffset === 0 && chunks[0].byteLength === chunks[0].buffer.byteLength) return chunks[0].buffer;
        var out = new Uint8Array(total), off = 0;
        for (var i = 0; i < chunks.length; i++) { out.set(chunks[i], off); off += chunks[i].byteLength; }
        return out.buffer;
      });
    })();
  }
  function installBody(Proto, kind) {
    if (!Proto) return;
    var nat = {};
    ['text', 'json', 'arrayBuffer', 'blob', 'formData', 'bytes', 'clone'].forEach(function (n) {
      nat[n] = typeof Proto[n] === 'function' ? Proto[n] : null;
    });
    // The body bytes without the used check (for the body stream, clone()).
    function rawBytes(self) {
      if (CACHED.has(self)) return CACHED.get(self).then(function (b) { return b.slice(0); });
      if (STREAM_SRC.has(self)) return readAllBytes(STREAM_SRC.get(self));
      if (nat.arrayBuffer) return Promise.resolve(apply(nat.arrayBuffer, self, []));
      if (nat.text && typeof g.TextEncoder === 'function') {
        return Promise.resolve(apply(nat.text, self, [])).then(function (t) { return new g.TextEncoder().encode(t).buffer; });
      }
      return resolvedWith(new ArrayBuffer(0));
    }
    function disturbed(self) {
      var st = BODY.get(self);
      return !!(st && RS.get(st) && (RS.get(st).disturbed || RS.get(st).reader !== undefined));
    }
    function consume(self) {
      if (USED.has(self) || disturbed(self)) return rejectedWith(typeError("Failed to execute on '" + kind + "': body stream already read"));
      USED.add(self);
      return null;
    }
    function contentType(self) {
      try { return (self.headers && self.headers.get('content-type')) || ''; } catch (e) { return ''; }
    }
    method(Proto, 'arrayBuffer', function arrayBuffer() { return consume(this) || rawBytes(this); });
    method(Proto, 'text', function text() {
      var bad = consume(this);
      if (bad) return bad;
      if (!CACHED.has(this) && !STREAM_SRC.has(this) && nat.text) return Promise.resolve(apply(nat.text, this, []));
      return rawBytes(this).then(function (b) { return new g.TextDecoder().decode(new Uint8Array(b)); });
    });
    method(Proto, 'json', function json() {
      var self = this, bad = consume(self);
      if (bad) return bad;
      var t = (!CACHED.has(self) && !STREAM_SRC.has(self) && nat.text) ? Promise.resolve(apply(nat.text, self, []))
        : rawBytes(self).then(function (b) { return new g.TextDecoder().decode(new Uint8Array(b)); });
      return t.then(function (s) { return JSON.parse(s); });
    });
    method(Proto, 'bytes', function bytes() {
      var bad = consume(this);
      return bad || rawBytes(this).then(function (b) { return new Uint8Array(b); });
    });
    method(Proto, 'blob', function blob() {
      var self = this, bad = consume(self);
      if (bad) return bad;
      return rawBytes(self).then(function (b) { return new g.Blob([b], { type: contentType(self).toLowerCase() }); });
    });
    method(Proto, 'formData', function formData() {
      var self = this, bad = consume(self);
      if (bad) return bad;
      if (nat.formData && !CACHED.has(self) && !STREAM_SRC.has(self)) return Promise.resolve(apply(nat.formData, self, []));
      var type = contentType(self).toLowerCase();
      return rawBytes(self).then(function (b) {
        if (type.indexOf('application/x-www-form-urlencoded') === 0 && g.URLSearchParams && g.FormData) {
          var fd = new g.FormData();
          new g.URLSearchParams(new g.TextDecoder().decode(new Uint8Array(b))).forEach(function (v, k) { fd.append(k, v); });
          return fd;
        }
        throw typeError("Failed to execute 'formData' on '" + kind + "': Could not parse content as FormData.");
      });
    });
    getter(Proto, 'bodyUsed', function () { return USED.has(this) || disturbed(this); });
    if (kind === 'Response') {
      method(Proto, 'clone', function clone() {
        if (USED.has(this) || disturbed(this)) throw typeError("Failed to execute 'clone' on 'Response': Response body is already used");
        var copy;
        try { copy = new NativeResponse(null, { status: this.status >= 200 && this.status <= 599 ? this.status : 200, statusText: this.statusText, headers: this.headers }); }
        catch (e) { copy = new NativeResponse(null); }
        var src = STREAM_SRC.get(this) || (BODY.has(this) ? BODY.get(this) : null);
        if (src && RS.get(src)) {
          var pair = RSP.tee.call(src);
          STREAM_SRC.set(this, pair[0]); BODY.set(this, pair[0]);
          STREAM_SRC.set(copy, pair[1]); BODY.set(copy, pair[1]);
        } else {
          if (!CACHED.has(this)) { var once = rawBytes(this); markHandled(once); CACHED.set(this, once); }
          CACHED.set(copy, CACHED.get(this));
        }
        var self = this;
        ['url', 'type', 'redirected'].forEach(function (n) {
          try { var v = self[n]; Object.defineProperty(copy, n, { get: function () { return v; }, configurable: true, enumerable: true }); } catch (e) {}
        });
        return copy;
      });
    }
    // The body stream reads the raw bytes once.
    bodyReaders.set(Proto, rawBytes);
  }
  var bodyReaders = new WeakMap();
  var NativeResponse = g.Response;
  if (NativeResponse && NativeResponse.prototype) installBody(NativeResponse.prototype, 'Response');
  if (g.Request && g.Request.prototype) installBody(g.Request.prototype, 'Request');
  // new Response(readableStream): the stream is the body.
  if (typeof NativeResponse === 'function') {
    var construct = typeof Reflect === 'object' && typeof Reflect.construct === 'function' ? Reflect.construct : null;
    var WrappedResponse = function Response(body, init) {
      if (!(this instanceof WrappedResponse)) throw typeError("Failed to construct 'Response': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      var st = body !== null && body !== undefined ? RS.get(body) : null;
      var args = st ? [null, init] : Array.prototype.slice.call(arguments, 0, 2);
      if (st && (st.disturbed || st.reader !== undefined)) throw typeError("Failed to construct 'Response': Response body object should not be disturbed or locked");
      var r = construct ? construct(NativeResponse, args, NativeResponse) : (args.length === 0 ? new NativeResponse() : new NativeResponse(args[0], args[1]));
      if (st) { STREAM_SRC.set(r, body); BODY.set(r, body); }
      return r;
    };
    WrappedResponse.prototype = NativeResponse.prototype;
    // Response.json(data, init) (Fetch §6.4) when the engine lacks it.
    if (typeof NativeResponse.json !== 'function') {
      method(WrappedResponse, 'json', function json(data, init) {
        var body = JSON.stringify(data);
        if (body === undefined) throw typeError("Failed to execute 'json' on 'Response': The data is not JSON serializable");
        var i = init && typeof init === 'object' ? init : {};
        var headers = new g.Headers(i.headers || {});
        if (!headers.has('content-type')) headers.set('content-type', 'application/json');
        return new WrappedResponse(body, { status: i.status === undefined ? 200 : i.status, statusText: i.statusText || '', headers: headers });
      });
    }
    Object.getOwnPropertyNames(NativeResponse).forEach(function (n) {
      if (n === 'prototype' || n === 'length' || n === 'name' || n === 'caller' || n === 'arguments') return;
      try { Object.defineProperty(WrappedResponse, n, Object.getOwnPropertyDescriptor(NativeResponse, n)); } catch (e) {}
    });
    try { Object.defineProperty(NativeResponse.prototype, 'constructor', { value: WrappedResponse, configurable: true, writable: true }); } catch (e) {}
    try { Object.defineProperty(g, 'Response', { value: WrappedResponse, configurable: true, writable: true, enumerable: false }); } catch (e) { g.Response = WrappedResponse; }
  }
  if (g.Response && g.Response.prototype) bodyGetter(g.Response.prototype, Object.getOwnPropertyDescriptor(g.Response.prototype, 'body'));
  if (g.Request && g.Request.prototype) bodyGetter(g.Request.prototype, Object.getOwnPropertyDescriptor(g.Request.prototype, 'body'));
  if (g.Blob && g.Blob.prototype && typeof g.Blob.prototype.stream !== 'function') {
    method(g.Blob.prototype, 'stream', function stream() {
      if (typeof this.arrayBuffer !== 'function') throw typeError('Illegal invocation');
      return bodyStream(this, function (o) { return o.arrayBuffer(); });
    });
  }
})();

// -- Sanitizer core and streaming HTML (WICG declarative partial updates) --
// Release 2026-10-14: `__narsilSanitize(root, safe, config)` — the
// sanitizer configuration members elements / removeElements /
// replaceWithChildrenElements / attributes / removeAttributes / comments /
// dataAttributes, plus the "safe" baseline (no scripts, frames, objects,
// embeds, event handler attributes, javascript: URLs) — and the streaming
// methods stream{,Append,Prepend,Before,After,ReplaceWith}HTML{,Unsafe}
// returning a WritableStream. Each write re-parses the text received so
// far in context and syncs the result into the target: finished top-level
// nodes stay, the open last node grows in place, new nodes go to the
// insertion point (so interleaved streams append like the real parser).
// MEASURED before (WPT domparsing/tentative stream-*): "streamHTML is not a
// function", WritableStream undefined.
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  var doc = g.document;
  var E = g.Element && g.Element.prototype, CD = g.CharacterData && g.CharacterData.prototype;
  var SR = g.ShadowRoot && g.ShadowRoot.prototype;
  if (!doc || !E || typeof g.WritableStream !== 'function' || typeof E.streamHTML === 'function') return;
  var HTML = 'http://www.w3.org/1999/xhtml', SVG = 'http://www.w3.org/2000/svg';
  var UNSAFE = { script: 1, frame: 1, iframe: 1, object: 1, embed: 1 };
  var URL_ATTRS = { href: 1, src: 1, action: 1, formaction: 1, xlinkhref: 1 };

  function nameList(list) {
    if (!list || typeof list.length !== 'number') return null;
    var out = [];
    for (var i = 0; i < list.length && i < 4096; i++) {
      var x = list[i];
      if (typeof x === 'string') out.push({ name: x, ns: HTML, attrs: null, removeAttrs: null });
      else if (x && typeof x === 'object') {
        out.push({ name: String(x.name), ns: x.namespace === undefined ? HTML : x.namespace,
          attrs: x.attributes ? attrList(x.attributes) : null, removeAttrs: x.removeAttributes ? attrList(x.removeAttributes) : null });
      }
    }
    return out;
  }
  function attrList(list) {
    if (!list || typeof list.length !== 'number') return null;
    var out = [];
    for (var i = 0; i < list.length && i < 4096; i++) {
      var x = list[i];
      if (typeof x === 'string') out.push({ name: x, ns: null });
      else if (x && typeof x === 'object') out.push({ name: String(x.name), ns: x.namespace === undefined ? null : x.namespace });
    }
    return out;
  }
  function findEl(list, el) {
    if (!list) return null;
    var ns = el.namespaceURI || null;
    for (var i = 0; i < list.length; i++) if (list[i].name === el.localName && (list[i].ns || null) === ns) return list[i];
    return null;
  }
  function hasAttr(list, name, ns) {
    if (!list) return false;
    for (var i = 0; i < list.length; i++) if (list[i].name === name && (list[i].ns || null) === (ns || null)) return true;
    return false;
  }
  function compile(config) {
    if (config && config.__compiled) return config;
    var c = config && typeof config === 'object' ? config : {};
    if (g.Sanitizer && c instanceof g.Sanitizer && typeof c.get === 'function') c = c.get();
    // A configuration with both an allow and a remove list is invalid.
    if (c.elements !== undefined && c.removeElements !== undefined) throw new TypeError("Failed to read the 'sanitizer' property: 'elements' and 'removeElements' cannot both be specified.");
    if (c.attributes !== undefined && c.removeAttributes !== undefined) throw new TypeError("Failed to read the 'sanitizer' property: 'attributes' and 'removeAttributes' cannot both be specified.");
    return {
      __compiled: true,
      elements: nameList(c.elements), removeElements: nameList(c.removeElements),
      replaceWithChildren: nameList(c.replaceWithChildrenElements),
      attributes: attrList(c.attributes), removeAttributes: attrList(c.removeAttributes),
      comments: c.comments === undefined ? true : !!c.comments,
      dataAttributes: c.dataAttributes === undefined ? null : !!c.dataAttributes
    };
  }
  function isUnsafeUrl(v) { return /^\s*javascript:/i.test(String(v)); }
  function sanitizeNode(parent, cfg, safe, depth) {
    var child = parent.firstChild;
    while (child) {
      var next = child.nextSibling;
      if (child.nodeType === 8) {
        if (!cfg.comments) parent.removeChild(child);
      } else if (child.nodeType === 1) {
        var ln = child.localName, ns = child.namespaceURI;
        var remove = false, unwrap = false;
        if (safe && UNSAFE[ln] === 1 && (ns === HTML || (ln === 'script' && ns === SVG))) remove = true;
        if (findEl(cfg.removeElements, child)) remove = true;
        var allowed = cfg.elements ? findEl(cfg.elements, child) : null;
        if (!remove && cfg.elements && !allowed && !findEl(cfg.replaceWithChildren, child)) remove = true;
        if (!remove && findEl(cfg.replaceWithChildren, child)) unwrap = true;
        if (remove) { parent.removeChild(child); }
        else {
          // Attributes.
          var names = child.getAttributeNames ? child.getAttributeNames() : [];
          for (var k = 0; k < names.length; k++) {
            var an = names[k], keep = true;
            if (safe && /^on/i.test(an)) keep = false;
            if (safe && URL_ATTRS[an.replace(':', '')] === 1 && isUnsafeUrl(child.getAttribute(an))) keep = false;
            if (hasAttr(cfg.removeAttributes, an, null)) keep = false;
            if (allowed && allowed.removeAttrs && hasAttr(allowed.removeAttrs, an, null)) keep = false;
            if (keep && cfg.attributes) {
              var ok = hasAttr(cfg.attributes, an, null) || (allowed && allowed.attrs && hasAttr(allowed.attrs, an, null));
              if (!ok && cfg.dataAttributes && /^data-/.test(an)) ok = true;
              if (!ok) keep = false;
            } else if (keep && allowed && allowed.attrs && !hasAttr(allowed.attrs, an, null)) {
              if (!(cfg.dataAttributes && /^data-/.test(an))) keep = false;
            }
            if (keep && cfg.dataAttributes === false && /^data-/.test(an)) keep = false;
            if (!keep) child.removeAttribute(an);
          }
          if (depth < 512) {
            sanitizeNode(child, cfg, safe, depth + 1);
            if (ln === 'template' && ns === HTML && child.content) sanitizeNode(child.content, cfg, safe, depth + 1);
          }
          if (unwrap) {
            while (child.firstChild) parent.insertBefore(child.firstChild, child);
            parent.removeChild(child);
          }
        }
      }
      child = next;
    }
  }
  function sanitize(root, safe, config) { sanitizeNode(root, compile(config), !!safe, 0); }
  try { Object.defineProperty(g, '__narsilSanitize', { value: sanitize, configurable: true, writable: true, enumerable: false }); } catch (e) {}

  // Declarative shadow roots after sanitizing: a remaining
  // <template shadowrootmode> becomes its parent's shadow root (nested ones
  // inside first).
  function attachDeclarative(node, depth) {
    if (depth > 256) return;
    var child = node.firstChild;
    while (child) {
      var next = child.nextSibling;
      if (child.nodeType === 1) {
        var mode = child.localName === 'template' && child.namespaceURI === HTML ? child.getAttribute('shadowrootmode') : null;
        var host = child.parentNode;
        if ((mode === 'open' || mode === 'closed') && host && host.nodeType === 1 && !host.shadowRoot && child.content) {
          attachDeclarative(child.content, depth + 1);
          var root = null;
          try {
            root = host.attachShadow({ mode: mode, clonable: child.hasAttribute('shadowrootclonable'),
              serializable: child.hasAttribute('shadowrootserializable'), delegatesFocus: child.hasAttribute('shadowrootdelegatesfocus') });
          } catch (e) { root = null; }
          if (root) {
            while (child.content.firstChild) root.appendChild(child.content.firstChild);
            host.removeChild(child);
          }
        } else {
          attachDeclarative(child, depth + 1);
          if (child.localName === 'template' && child.content) attachDeclarative(child.content, depth + 1);
        }
      }
      child = next;
    }
  }
  // Parse `text` as a fragment in `context` (like innerHTML).
  function parseFragment(context, text, unsafe, ownerDoc) {
    var helper;
    if (!context || context.nodeType !== 1 || (context.localName === 'html' && context.namespaceURI === HTML)) helper = ownerDoc.createElementNS(HTML, 'body');
    else helper = context.namespaceURI ? ownerDoc.createElementNS(context.namespaceURI, context.localName) : ownerDoc.createElement(context.localName);
    helper.innerHTML = text;
    var src = helper.localName === 'template' && helper.namespaceURI === HTML && helper.content ? helper.content : helper;
    var f = ownerDoc.createDocumentFragment();
    while (src.firstChild) f.appendChild(src.firstChild);
    return f;
  }
  var revived = new WeakSet();
  function reviveScript(old, ownerDoc) {
    var fresh = ownerDoc.createElementNS(old.namespaceURI || HTML, old.localName);
    var names = old.getAttributeNames();
    for (var i = 0; i < names.length; i++) fresh.setAttribute(names[i], old.getAttribute(names[i]));
    fresh.textContent = old.textContent;
    revived.add(fresh);
    old.parentNode.replaceChild(fresh, old);
    return fresh;
  }
  function sameShape(a, b) {
    if (a.nodeType !== b.nodeType) return false;
    if (a.nodeType === 1) return a.localName === b.localName && a.namespaceURI === b.namespaceURI;
    return true;
  }
  // A text node the stream merged into keeps the part that was there before.
  var textPrefix = new WeakMap();
  // Sync the fresh nodes into `container` where `ours` are the nodes this
  // stream inserted at this level; `ref` is the insertion point.
  function syncLevel(container, ours, fresh, ref, depth) {
    var k = ours.length;
    if (k > 0 && fresh.length >= k) {
      var last = ours[k - 1], neu = fresh[k - 1];
      if (sameShape(last, neu)) {
        if (last.nodeType === 3 || last.nodeType === 8) {
          var pre = textPrefix.get(last) || 0;
          var whole = String(last.data), d0 = whole.slice(pre), d1 = String(neu.data);
          if (d0 !== d1) {
            var atEnd = last.nextSibling === ref;
            if (!atEnd && last.nodeType === 3 && d1.indexOf(d0) === 0) {
              var extra = (container.ownerDocument || doc).createTextNode(d1.slice(d0.length));
              container.insertBefore(extra, ref);
              ours.push(extra);
            } else last.data = whole.slice(0, pre) + d1;
          }
        } else if (last.nodeType === 1 && depth < 512) {
          var names = neu.getAttributeNames();
          for (var a = 0; a < names.length; a++) {
            if (last.getAttribute(names[a]) !== neu.getAttribute(names[a])) last.setAttribute(names[a], neu.getAttribute(names[a]));
          }
          var lc = last.localName === 'template' && last.content ? last.content : last;
          var nc = neu.localName === 'template' && neu.content ? neu.content : neu;
          syncLevel(lc, Array.prototype.slice.call(lc.childNodes), Array.prototype.slice.call(nc.childNodes), null, depth + 1);
        }
      } else {
        container.replaceChild(neu, last);
        ours[k - 1] = neu;
      }
    }
    for (var i = k; i < fresh.length; i++) {
      // Like the parser: text right after a text node joins it.
      var before = ref ? ref.previousSibling : container.lastChild;
      if (i === 0 && depth === 0 && k === 0 && fresh[i].nodeType === 3 && before && before.nodeType === 3) {
        textPrefix.set(before, String(before.data).length);
        before.data = String(before.data) + String(fresh[i].data);
        ours.push(before);
        continue;
      }
      container.insertBefore(fresh[i], ref);
      ours.push(fresh[i]);
    }
  }
  function walkScripts(nodes, fn) {
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.nodeType !== 1) continue;
      if (n.localName === 'script') fn(n, i === nodes.length - 1);
      else walkScripts(Array.prototype.slice.call(n.childNodes), function (s) { fn(s, false); });
    }
  }
  // The writable stream for one streaming call.
  // A <script> (HTML or SVG) cannot be the target of streamed markup.
  function rejectScriptContext(ctx) {
    if (ctx && ctx.nodeType === 1 && ctx.localName === 'script' && (ctx.namespaceURI === HTML || ctx.namespaceURI === SVG)) {
      throw new g.DOMException('Streaming into a script element is not supported.', 'NotSupportedError');
    }
  }
  function prepare(context, opt) {
    rejectScriptContext(context);
    var options = opt && typeof opt === 'object' ? opt : {};
    return { options: options, config: compile(options.sanitizer) };
  }
  function streamInto(container, refNode, context, unsafe, prepared) {
    var options = prepared.options;
    var runScripts = unsafe && !!options.runScripts;
    var config = prepared.config;
    var hasConfig = options.sanitizer !== undefined && options.sanitizer !== null;
    var ownerDoc = container.ownerDocument || doc;
    var text = '', ours = [], ended = false;
    function render(final) {
      // The insertion point is gone: the write rejects.
      if (refNode && refNode.parentNode !== container) {
        throw new g.DOMException('The node before which the markup is streamed is no longer a child of the target.', 'HierarchyRequestError');
      }
      var f = parseFragment(context, text, unsafe, ownerDoc);
      sanitize(f, !unsafe, config);
      // Declarative shadow DOM: in the unsafe variants, and in the safe ones
      // when a sanitizer configuration is given.
      if (unsafe || hasConfig) attachDeclarative(f, 0);
      syncLevel(container, ours, Array.prototype.slice.call(f.childNodes), refNode, 0);
      if (runScripts && !(container.nodeType === 11 && !container.host && container !== ownerDoc)) {
        var tail = text.slice(text.toLowerCase().lastIndexOf('<script'));
        var lastClosed = /<\/script/i.test(tail);
        walkScripts(ours.slice(), function (s, isLast) {
          if (revived.has(s) || (isLast && !lastClosed)) return;
          var idx = ours.indexOf(s);
          var fresh = reviveScript(s, ownerDoc);
          if (idx >= 0) ours[idx] = fresh;
        });
      }
    }
    return new g.WritableStream({
      write: function (chunk) {
        if (ended) return;
        if (typeof chunk === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string');
        text += String(chunk);
        render(false);
      },
      close: function () { ended = true; render(true); },
      abort: function () { ended = true; }
    });
  }
  function def(P, n, f) { if (P) { try { Object.defineProperty(P, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} } }
  function isTemplate(n) { return !!n && n.nodeType === 1 && n.localName === 'template' && n.namespaceURI === HTML; }
  function contentOf(n) { return isTemplate(n) && n.content ? n.content : n; }
  function hierarchyError(name) { throw new g.DOMException("Failed to execute '" + name + "': the parent cannot take parsed markup.", 'HierarchyRequestError'); }
  [false, true].forEach(function (unsafe) {
    var sfx = unsafe ? 'Unsafe' : '';
    // streamHTML: the existing content goes away immediately.
    var whole = function (opt) {
      var target = contentOf(this), context = this.nodeType === 11 && this.host ? this.host : this;
      var prepared = prepare(context, opt);
      while (target.firstChild) target.removeChild(target.firstChild);
      return streamInto(target, null, context, unsafe, prepared);
    };
    def(E, 'streamHTML' + sfx, whole);
    def(SR, 'streamHTML' + sfx, whole);
    var append = function (opt) {
      var context = this.nodeType === 11 && this.host ? this.host : this;
      return streamInto(contentOf(this), null, context, unsafe, prepare(context, opt));
    };
    var prepend = function (opt) {
      var target = contentOf(this), context = this.nodeType === 11 && this.host ? this.host : this;
      var prepared = prepare(context, opt);
      return streamInto(target, target.firstChild, context, unsafe, prepared);
    };
    def(E, 'streamAppendHTML' + sfx, append); def(SR, 'streamAppendHTML' + sfx, append);
    def(E, 'streamPrependHTML' + sfx, prepend); def(SR, 'streamPrependHTML' + sfx, prepend);
    function positional(name, pick) {
      var f = function (opt) {
        var parent = this.parentNode;
        if (!parent) return new g.WritableStream({});
        var context = parent;
        if (parent.nodeType === 11) {
          if (parent.host) context = parent.host;
          else {
            var host = null;
            try { host = parent.__narsilTemplateHost; } catch (e) {}
            if (host && host.nodeType === 1) context = host; else hierarchyError(name);
          }
        }
        if (parent.nodeType === 9 || isTemplate(parent)) hierarchyError(name);
        var prepared = prepare(context, opt);
        return streamInto(parent, pick(this, parent), context, unsafe, prepared);
      };
      def(E, name, f);
      def(CD, name, f);
    }
    positional('streamBeforeHTML' + sfx, function (self) { return self; });
    positional('streamAfterHTML' + sfx, function (self) { return self.nextSibling; });
    positional('streamReplaceWithHTML' + sfx, function (self, parent) { var ref = self.nextSibling; parent.removeChild(self); return ref; });
  });
})();

// -- DOMStringMap: element.dataset (HTML §3.2.6.6) --
// Release 2026-10-15: `dataset` is a proxy over the element's data-*
// attributes — named getter/setter/deleter, `in`, and the supported
// property names for Object.keys/getOwnPropertyNames (the host leaves
// `dataset` to the prelude). One proxy per element.
// MEASURED before (WPT dom/collections/domstringmap-supported-property-names
// 0/5): Object.getOwnPropertyNames(el.dataset) gave ["__host", "__art"].
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (typeof g.Proxy !== 'function') return;
  var DSM = g.DOMStringMap;
  if (typeof DSM !== 'function') {
    DSM = function DOMStringMap() { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(g, 'DOMStringMap', { value: DSM, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  }
  var proto = DSM.prototype;
  var maps = new WeakMap();
  function attrName(prop) {
    return 'data-' + prop.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); });
  }
  function propName(attr) {
    return attr.slice(5).replace(/-([a-z])/g, function (m, c) { return c.toUpperCase(); });
  }
  // The supported property names: data-* attributes without ASCII upper
  // case, in attribute order.
  function names(el) {
    var out = [], list = el.getAttributeNames ? el.getAttributeNames() : [];
    for (var i = 0; i < list.length && i < 65536; i++) {
      var a = list[i];
      if (a.length >= 5 && a.slice(0, 5) === 'data-' && !/[A-Z]/.test(a)) out.push(propName(a));
    }
    return out;
  }
  function supported(el, k) { return typeof k === 'string' && names(el).indexOf(k) >= 0; }
  function setProp(el, k, v) {
    if (/-[a-z]/.test(k)) throw new g.DOMException("Failed to set the '" + k + "' property on 'DOMStringMap': '" + k + "' is not a valid property name.", 'SyntaxError');
    el.setAttribute(attrName(k), String(v));
  }
  function mapFor(el) {
    var m = maps.get(el);
    if (m) return m;
    m = new Proxy(Object.create(proto), {
      get: function (t, k, r) {
        if (typeof k === 'string' && supported(el, k)) return el.getAttribute(attrName(k));
        return Reflect.get(t, k, r);
      },
      set: function (t, k, v, r) {
        if (typeof k !== 'string') return Reflect.set(t, k, v, r);
        setProp(el, k, v);
        return true;
      },
      has: function (t, k) { return supported(el, k) || (k in t); },
      deleteProperty: function (t, k) {
        if (supported(el, k)) { el.removeAttribute(attrName(k)); return true; }
        return Reflect.deleteProperty(t, k);
      },
      ownKeys: function (t) { return names(el).concat(Reflect.ownKeys(t)); },
      getOwnPropertyDescriptor: function (t, k) {
        if (supported(el, k)) return { value: el.getAttribute(attrName(k)), writable: true, enumerable: true, configurable: true };
        return Reflect.getOwnPropertyDescriptor(t, k);
      },
      defineProperty: function (t, k, desc) {
        if (typeof k === 'string' && desc && 'value' in desc) { setProp(el, k, desc.value); return true; }
        return Reflect.defineProperty(t, k, desc);
      }
    });
    maps.set(el, m);
    return m;
  }
  ['HTMLElement', 'SVGElement', 'MathMLElement'].forEach(function (n) {
    var P = g[n] && g[n].prototype;
    if (!P) return;
    try {
      Object.defineProperty(P, 'dataset', { configurable: true, enumerable: true, get: function () { return mapFor(this); } });
    } catch (e) {}
  });
})();

// -- window.name (HTML §7.3.1): the name of the browsing context --
// MEASURED before: a bare `name` threw ReferenceError (WPT
// SharedWorker-constructor.html passes it as the worker name).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  if (Object.getOwnPropertyDescriptor(g, 'name')) return;
  var value = '';
  try {
    Object.defineProperty(g, 'name', {
      configurable: true, enumerable: true,
      get: function () { return value; },
      set: function (v) { value = String(v); }
    });
  } catch (e) {}
})();

// -- Die eingebauten Namen des globalen Objekts --
// Release 12.10.2026: am ENDE des Vorspanns, also vor jedem Seitenskript —
// alles, was danach am globalen Objekt entsteht, gehoert der Seite (siehe
// `abschirmen` im Rahmen-Block).
(function () {
  var g = typeof globalThis !== 'undefined' ? globalThis : this;
  try {
    Object.defineProperty(g, '__narsilGrundNamen', {
      value: Object.getOwnPropertyNames(g), configurable: true, writable: true, enumerable: false
    });
  } catch (e) {}
})();
