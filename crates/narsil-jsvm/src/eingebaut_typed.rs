// eingebaut_typed.rs (JS, W16, 04.10.2026) — **ArrayBuffer**,
// **SharedArrayBuffer** (Attrappe) und **DataView**.
//
// ## WARUM DIESE DATEI
//
// Der Motor hatte **kein** Byte-Speichermodell. GEMESSEN, was daraus auf
// echten Seiten fehlt (das ist die Begründung der Leitung fuer PRIO 1):
//
// ```text
// crypto.getRandomValues(new Uint8Array(4))
// new TextDecoder().decode(new Uint8Array([104,105]))
// fetch(url).then(r => r.arrayBuffer())
// WebAssembly.Memory.buffer
// new ImageData(new Uint8Array(4), 2, 2)     // Canvas
// gl.bufferData(gl.ARRAY_BUFFER, new Uint8Array([1,2]), gl.STATIC_DRAW)
// ```
//
// Alle brauchen dasselbe: einen **Byte**speicher, den zwei verschiedene
// Sichten (Uint8Array / Float64Array) **gemeinsam** sehen.
//
// ## DER FELSCHE WEG, DEN ICH ZUERST GEWAEHLT HABE
//
// `HObj::Feld` haelt `Vec<GVal>` — **eine `GVal` je Element**. Damit waere
// `new Uint8Array(buf)[0]` und `new Float64Array(buf)[0]` **derselbe**
// Elementplatz, und die beiden Sichten muessen aber **verschiedene** Werte
// liefern (1 Byte gegen 8 Byte). Also eine eigene Heap-Variante mit `Vec<u8>`:
// `HObj::BytePuffer { daten: Vec<u8>, geteilt: bool, .. }` (heap.rs).
//
// ## WAS IN DIESER DATEI STEHT — und was noch nicht
//
// **DRIN:** die Elementarten-Tabelle (Groesse, Name, Lesen, Schreiben) als
// Fundament fuer alle elf TypedArrays, `ArrayBuffer` mit `byteLength` und
// `slice`, `SharedArrayBuffer` als Attrappe, und `DataView` mit
// `getUint8`…`getFloat64`/`setUint8`…`setFloat64` inklusive Endianness.
//
// **NOCH NICHT DRIN (der naechste Schritt, kein halber Bau):** die elf
// TypedArray-Konstruktoren selbst, der indexierte Zugriff `ta[i]`, `length`,
// `subarray`/`set`/`fill`/`map`, die Iteration und `from`/`of`. Der indexierte
// Zugriff braucht eine Zeile in `eigenschaften.rs` (`hole`/`setze`), und die
// Konstruktoren eine Registrierung in `builtin.rs` — beides fremde Baustellen,
// die im BRETT angekündigt sind. Ein TypedArray ohne `ta[0]` waere wie Proxy
// ohne `get`: es kompiliert und ist falsch.
//
// ## JEDER SOLLWERT STAMMT AUS node 22 (v22.23.3)
//!
//! ```text
//! new ArrayBuffer(8).byteLength            -> 8
//! new ArrayBuffer(3).byteLength            -> 3
//! new ArrayBuffer(8).slice(2).byteLength    -> 6
//! new ArrayBuffer(8).slice(2,5).byteLength  -> 3
//! Object.prototype.toString.call(buf)       -> [object ArrayBuffer]
//! sab instanceof ArrayBuffer                -> false   (getrennte Klassen!)
//! new SharedArrayBuffer(4).byteLength       -> 4
//! sab.growable                              -> boolean
//! dv.byteLength / dv.byteOffset             -> 8 / 0
//! dv.setFloat64(0,1.5); dv.getFloat64(0)    -> 1.5
//! dv.setUint8(0,255); dv.getUint8(0)        -> 255
//! dv.setInt16(0,-2,true/false)              -> -2 / -2
//! dv.getUint8(99) / dv.getUint8(-1)         -> RangeError / RangeError
//! ```

use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};
use std::collections::HashMap;

/// **Elementart** eines TypedArray (ES2023 23.2.7 `%TypedArray%`).
///
/// Die Tabelle ist das Fundament fuer alle elf Arten: sie beantwortet
/// „wie gross ist ein Element" (das ist `BYTES_PER_ELEMENT` **und** die
/// Grundlage fuer `length = byteLength / bytesPerElement`) und „wie liest und
/// schreibt man ein Element an dieser Stelle".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElementArt {
    Uint8,
    Int8,
    Uint8Clamped,
    Uint16,
    Int16,
    Uint32,
    Int32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
    /// Release 15.10.2026: `Float16Array` (ES2025, IEEE 754 binary16).
    Float16,
}

impl ElementArt {
    /// `BYTES_PER_ELEMENT` — ES2023 23.2.7.
    pub(crate) fn bytes(self) -> usize {
        match self {
            ElementArt::Uint8 | ElementArt::Int8 | ElementArt::Uint8Clamped => 1,
            ElementArt::Uint16 | ElementArt::Int16 | ElementArt::Float16 => 2,
            ElementArt::Uint32 | ElementArt::Int32 | ElementArt::Float32 => 4,
            ElementArt::Float64 | ElementArt::BigInt64 | ElementArt::BigUint64 => 8,
        }
    }

    /// Der Name der Art, wie `Object.prototype.toString` ihn ueber
    /// `Symbol.toStringTag` nennt.
    pub(crate) fn name(self) -> &'static str {
        match self {
            ElementArt::Uint8 => "Uint8Array",
            ElementArt::Int8 => "Int8Array",
            ElementArt::Uint8Clamped => "Uint8ClampedArray",
            ElementArt::Uint16 => "Uint16Array",
            ElementArt::Int16 => "Int16Array",
            ElementArt::Uint32 => "Uint32Array",
            ElementArt::Int32 => "Int32Array",
            ElementArt::Float32 => "Float32Array",
            ElementArt::Float64 => "Float64Array",
            ElementArt::BigInt64 => "BigInt64Array",
            ElementArt::BigUint64 => "BigUint64Array",
            ElementArt::Float16 => "Float16Array",
        }
    }

    /// Die Art hinter einem Konstruktor-Namen. `None`, wenn der Name keiner
    /// der elf ist — dann war es ein normales Skript.
    pub(crate) fn aus_name(name: &str) -> Option<ElementArt> {
        Some(match name {
            "Uint8Array" => ElementArt::Uint8,
            "Int8Array" => ElementArt::Int8,
            "Uint8ClampedArray" => ElementArt::Uint8Clamped,
            "Uint16Array" => ElementArt::Uint16,
            "Int16Array" => ElementArt::Int16,
            "Uint32Array" => ElementArt::Uint32,
            "Int32Array" => ElementArt::Int32,
            "Float32Array" => ElementArt::Float32,
            "Float64Array" => ElementArt::Float64,
            "BigInt64Array" => ElementArt::BigInt64,
            "BigUint64Array" => ElementArt::BigUint64,
            "Float16Array" => ElementArt::Float16,
            _ => return None,
        })
    }

    /// Liest das Element an Byte-Offset `bei`.
    ///
    /// ES2023 23.2.7 `GetValue`: **Big little endian**, zwei's complement
    /// fuer die Integerarten, `Float32` ueber `f32` (mit Runden — genau
    /// deshalb ist `Float32Array` nicht dasselbe wie `Float64Array`).
    /// GEMESSEN (node 22) fuer `getInt16(0, true)` mit `-2` -> `-2`.
    pub(crate) fn lesen(self, daten: &[u8], bei: usize) -> GVal {
        let b = |i: usize| -> u8 { daten.get(bei + i).copied().unwrap_or(0) };
        match self {
            ElementArt::Uint8 => GVal::Zahl(b(0) as f64),
            ElementArt::Uint8Clamped => GVal::Zahl(b(0) as f64),
            ElementArt::Int8 => GVal::Zahl(b(0) as i8 as f64),
            ElementArt::Uint16 => GVal::Zahl(u16::from_le_bytes([b(0), b(1)]) as f64),
            ElementArt::Int16 => GVal::Zahl(i16::from_le_bytes([b(0), b(1)]) as f64),
            ElementArt::Uint32 => GVal::Zahl(u32::from_le_bytes([b(0), b(1), b(2), b(3)]) as f64),
            ElementArt::Int32 => GVal::Zahl(i32::from_le_bytes([b(0), b(1), b(2), b(3)]) as f64),
            ElementArt::Float32 => {
                let a = [b(0), b(1), b(2), b(3)];
                GVal::Zahl(f32::from_le_bytes(a) as f64)
            }
            ElementArt::Float16 => GVal::Zahl(crate::es_modern::f16_zu_f64(u16::from_le_bytes([b(0), b(1)]))),
            ElementArt::Float64 => {
                let a = [b(0), b(1), b(2), b(3), b(4), b(5), b(6), b(7)];
                GVal::Zahl(f64::from_le_bytes(a))
            }
            // BigInt-Arten: der Wert ist eine **Ganzzahl**, keine Zahl.
            ElementArt::BigInt64 => {
                let a = [b(0), b(1), b(2), b(3), b(4), b(5), b(6), b(7)];
                GVal::Ganz(i64::from_le_bytes(a) as i128)
            }
            ElementArt::BigUint64 => {
                let a = [b(0), b(1), b(2), b(3), b(4), b(5), b(6), b(7)];
                // `u64` laesst sich nicht verlustfrei in `i128` erzwingen,
                // wenn das hoechste Bit gesetzt ist — als **Vorzeichen**
                // gelesen ist es das, was `i64` sieht, und das ist hier die
                // ehrliche Grenze (ein eigener Schritt, dokumentiert).
                GVal::Ganz(u64::from_le_bytes(a) as i128)
            }
        }
    }

    /// Schreibt `wert` an Byte-Offset `bei` (Big little endian).
    ///
    /// **Der Wert kommt als `Schreibwert`** — und das ist nicht Geschmack:
    /// `ElementArt` hat keinen `Engine`, mit dem man `ToNumber` machen
    /// koennte, und die BigInt-Arten brauchen eine **Ganzzahl**, keine Zahl.
    /// `Schreibwert` ist die einzige Form, die beides traegt.
    pub(crate) fn schreiben(self, daten: &mut [u8], bei: usize, wert: Schreibwert) {
        let mut setz = |i: usize, v: u8| {
            if bei + i < daten.len() {
                daten[bei + i] = v;
            }
        };
        // `NaN` -> 0 ist die Spec-Regel der Integerarten (ES2023 23.2.7
        // SetValue: „if number is NaN, return +0").
        let n = |v: Schreibwert| -> f64 {
            match v {
                Schreibwert::Zahl(z) => z,
                Schreibwert::Ganz(g) => g as f64,
            }
        };
        match self {
            ElementArt::Uint8 => {
                let v = n(wert);
                setz(0, ganz_modulo(v, 8) as u8);
            }
            ElementArt::Int8 => {
                let v = n(wert);
                setz(0, ganz_modulo(v, 8) as u8);
            }
            ElementArt::Uint8Clamped => {
                // ES2023 23.2.7: **runden**, dann klemmen. Das ist der ganze
                // Unterschied zu `Uint8Array`.
                let v = n(wert);
                setz(0, v.round_ties_even().clamp(0.0, 255.0) as u8);
            }
            ElementArt::Uint16 => {
                let a = (ganz_modulo(n(wert), 16) as u16).to_le_bytes();
                setz(0, a[0]);
                setz(1, a[1]);
            }
            ElementArt::Int16 => {
                let a = (ganz_modulo(n(wert), 16) as u16).to_le_bytes();
                setz(0, a[0]);
                setz(1, a[1]);
            }
            ElementArt::Uint32 => {
                for (i, b) in (ganz_modulo(n(wert), 32) as u32).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
            ElementArt::Int32 => {
                for (i, b) in (ganz_modulo(n(wert), 32) as u32).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
            ElementArt::Float32 => {
                for (i, b) in (n(wert) as f32).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
            ElementArt::Float16 => {
                for (i, b) in crate::es_modern::f64_zu_f16(n(wert)).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
            ElementArt::Float64 => {
                for (i, b) in n(wert).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
            ElementArt::BigInt64 | ElementArt::BigUint64 => {
                let g = match wert {
                    Schreibwert::Ganz(g) => g,
                    Schreibwert::Zahl(z) => z.trunc() as i128,
                };
                for (i, b) in (g as i64).to_le_bytes().iter().enumerate() {
                    setz(i, *b);
                }
            }
        }
    }
}

/// Was in ein Element geschrieben wird — eine Zahl **oder** eine Ganzzahl.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Schreibwert {
    Zahl(f64),
    Ganz(i128),
}

/// Element type for the DataView getter/setter dispatch.
pub(crate) fn dataview_art_fuer(nativ: crate::builtin::Nativ) -> ElementArt {
    use crate::builtin::Nativ as N;
    match nativ {
        N::DataviewGetUint8 | N::DataviewSetUint8 => ElementArt::Uint8,
        N::DataviewGetInt8 | N::DataviewSetInt8 => ElementArt::Int8,
        N::DataviewGetUint16 | N::DataviewSetUint16 => ElementArt::Uint16,
        N::DataviewGetInt16 | N::DataviewSetInt16 => ElementArt::Int16,
        N::DataviewGetUint32 | N::DataviewSetUint32 => ElementArt::Uint32,
        N::DataviewGetInt32 | N::DataviewSetInt32 => ElementArt::Int32,
        N::DataviewGetFloat32 | N::DataviewSetFloat32 => ElementArt::Float32,
        N::DataviewGetFloat64 | N::DataviewSetFloat64 => ElementArt::Float64,
        N::DataviewGetBigInt64 | N::DataviewSetBigInt64 => ElementArt::BigInt64,
        N::DataviewGetBigUint64 | N::DataviewSetBigUint64 => ElementArt::BigUint64,
        _ => ElementArt::Uint8,
    }
}

/// `DataView`-Lesen/Schreiben: die Art wird **nicht** aus dem Objekt
/// genommen, sondern aus dem Methodennamen. Deshalb steht sie hier und nicht
/// im `DataView`-Objekt (ES2023 25.2.5 `GetViewValue`).
pub(crate) fn dataview_art(lesen: bool) -> &'static [ElementArt] {
    let _ = lesen;
    &[
        ElementArt::Int8,
        ElementArt::Uint8,
        ElementArt::Int16,
        ElementArt::Uint16,
        ElementArt::Int32,
        ElementArt::Uint32,
        ElementArt::Float32,
        ElementArt::Float64,
        ElementArt::BigInt64,
        ElementArt::BigUint64,
    ]
}

impl Engine {
    /// `this` als Byte-Puffer, sonst `TypeError`.
    ///
    /// **Dieselbe Form wie `Engine::karte_als_ref`** (builtin3.rs:692) — das
    /// ist das Hausmuster fuer „`this` muss ein X sein": ein `TypeError` mit
    /// dem Methodennamen, und `undefined`/`null` fallen mit hinein.
    fn puffer_von(&mut self, this: GVal) -> Option<HeapRef> {
        let GVal::Ref(r) = this else { return None };
        match self.heap.holen(r) {
            HObj::BytePuffer { .. } => Some(r),
            _ => None,
        }
    }

    /// `GVal` -> `Schreibwert` fuer `ElementArt::schreiben`.
    ///
    /// Eine **Ganzzahl** bleibt Ganzzahl (BigInt-Arten), alles andere laeuft
    /// durch `ToNumber` — und ein `Symbol` wird dabei `NaN`, was die
    /// Integerarten laut Spec auf `+0` setzen.
    fn schreibwert(&mut self, v: GVal) -> Schreibwert {
        match v {
            GVal::Ganz(g) => Schreibwert::Ganz(g),
            andere => Schreibwert::Zahl(self.zu_zahl(andere).unwrap_or(f64::NAN)),
        }
    }

    /// Der `TypeError` fuer ein falsches `this` — **dieselbe Form wie
    /// `Engine::karte_als_ref`** (builtin3.rs:692), damit die Meldung im Haus
    /// einheitlich bleibt: der Methodenname und „braucht <Typ>".
    fn puffer_typfehler(&mut self, typ: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("TypeError", &format!("braucht {typ}")))
    }

    /// `puffer_von` mit dem passenden `TypeError`.
    fn puffer_pflicht(&mut self, this: GVal, name: &str) -> Result<HeapRef, Stoerung> {
        self.puffer_von(this).ok_or_else(|| {
            Stoerung::Wurf(self.fehler_wert("TypeError", &format!("{name} braucht ArrayBuffer")))
        })
    }

    /// Einen Byte-Puffer anlegen (`ArrayBuffer`/`SharedArrayBuffer`).
    pub(crate) fn neuer_bytepuffer(
        &mut self,
        len: usize,
        geteilt: bool,
    ) -> Result<HeapRef, Stoerung> {
        let proto = if geteilt {
            self.proto_shared_puffer
        } else {
            self.proto_puffer
        };
        if len > self.grenzen.max_heap_bytes {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Byte-Puffer zu gross")));
        }
        // Check the heap budget before allocating the backing Vec.
        self.vor_allok(len.saturating_add(crate::heap::KOSTEN_EINTRAG))?;
        let obj = HObj::BytePuffer {
            proto: Some(proto),
            daten: vec![0u8; len],
            geteilt,
            detached: false,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        Ok(self.belege(obj, kosten))
    }

    /// `new ArrayBuffer(len)` / `new SharedArrayBuffer(len)`.
    ///
    /// ES2023 25.1.5 / 25.2.5: die Laenge laeuft durch `ToIndex`. GEMESSEN
    /// (node 22): `new ArrayBuffer(8).byteLength` -> `8`.
    pub(crate) fn n_puffer_ctor(&mut self, args: &[GVal], geteilt: bool) -> Result<GVal, Stoerung> {
        let len = self.typedarray_index(self.arg(args, 0))? as u64;
        // Obergrenze: der Heap hat eine Grenze (`max_heap_bytes`), und ein
        // `new ArrayBuffer(2**53)` darf sie nicht umgehen.
        if len > self.grenzen.max_heap_bytes as u64 {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "RangeError",
                "ArrayBuffer zu gross fuer das Heap-Limit",
            )));
        }
        // Release 15.10.2026: `{maxByteLength}` makes it resizable
        // (growable for a SharedArrayBuffer), ES2024 25.1.4.1.
        let max = self.es_puffer_max_option(self.arg(args, 1), len as usize)?;
        let r = self.neuer_bytepuffer(len as usize, geteilt)?;
        self.es_puffer_max_setzen(r, max);
        Ok(GVal::Ref(r))
    }

    /// `byteLength` als Getter (ES2023 25.1.5 `get ArrayBuffer.prototype.byteLength`).
    pub(crate) fn n_puffer_bytelaenge(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        match self.puffer_von(this) {
            Some(r) => {
                let n = match self.heap.holen(r) {
                    HObj::BytePuffer { daten, .. } => daten.len(),
                    _ => 0,
                };
                Ok(GVal::Zahl(n as f64))
            }
            None => Err(self.puffer_typfehler("ArrayBuffer")),
        }
    }

    /// `growable` — **nur** `SharedArrayBuffer` (ES2023 25.2.5). Bei uns
    /// immer `false`: es gibt keine Groessenaenderung ohne Threads.
    /// GEMESSEN (node 22): `typeof sab.growable` -> `"boolean"`.
    pub(crate) fn n_puffer_growable(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        match self.puffer_von(this) {
            Some(_) => {
                // ## W53 — IMMER `false`, und **nicht** `geteilt`
                //
                // `growable` ist `[[Growable]]`, **nicht** „ist ein
                // `SharedArrayBuffer`". Die alte Fassung gab `geteilt`
                // zurueck, also `true` fuer jeden `SharedArrayBuffer` —
                // GEMESSEN vorher `true`, **node 22 (v22.23.3) `false`**:
                // ```js
                // new SharedArrayBuffer(4).growable   -> false
                // ```
                // Wir koennen ohne Threads nicht wachsen, also ist es
                // **immer** `false` — auch fuer einen `SharedArrayBuffer`.
                // Der eigene Kommentar an dieser Funktion sagte das bereits;
                // der Code tat es nicht. **Ein Kommentar, der etwas anderes
                // behauptet als der Code, ist ein Befund, kein Dokument.**
                let _ = &this;
                Ok(GVal::Bool(false))
            }
            None => Err(self.puffer_typfehler("SharedArrayBuffer")),
        }
    }

    /// `ArrayBuffer.prototype.detached` (ES2024 25.1.6.3).
    pub(crate) fn n_puffer_detached(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        match self.puffer_von(this) {
            Some(r) => Ok(GVal::Bool(matches!(
                self.heap.holen(r),
                HObj::BytePuffer { detached: true, .. }
            ))),
            None => Err(self.puffer_typfehler("ArrayBuffer")),
        }
    }

    /// `ArrayBuffer.prototype.transfer(newLength)` and
    /// `transferToFixedLength` (ES2024 25.1.6.7/8): a new buffer with the
    /// bytes (moved, not copied, at the same length), this one detached.
    pub(crate) fn n_puffer_transfer(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let Some(r) = self.puffer_von(this) else {
            return Err(self.puffer_typfehler("ArrayBuffer"));
        };
        let (alt_len, geteilt, detached) = match self.heap.holen(r) {
            HObj::BytePuffer { daten, geteilt, detached, .. } => (daten.len(), *geteilt, *detached),
            _ => return Err(self.puffer_typfehler("ArrayBuffer")),
        };
        if geteilt {
            return Err(self.puffer_typfehler("ArrayBuffer"));
        }
        let neu_len = match self.arg(args, 0) {
            GVal::Undefiniert => alt_len,
            v => self.typedarray_index(v)?,
        };
        if detached {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "ArrayBuffer is detached")));
        }
        // A WebAssembly.Memory buffer cannot be detached by JS.
        if self.platform_wasm_buffer(r) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "cannot transfer a WebAssembly.Memory buffer")));
        }
        let neu = self.neuer_bytepuffer(0, false)?;
        let mut bytes = match self.heap.holen_mut(r) {
            HObj::BytePuffer { daten, detached, .. } => {
                *detached = true;
                std::mem::take(daten)
            }
            _ => Vec::new(),
        };
        bytes.resize(neu_len, 0);
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(neu) {
            *daten = bytes;
        }
        Ok(GVal::Ref(neu))
    }

    /// `ArrayBuffer.prototype.slice(begin, end)` — ES2023 25.1.5.1.
    ///
    /// GEMESSEN (node 22): `new ArrayBuffer(8).slice(2).byteLength` -> `6`,
    /// `.slice(2,5).byteLength` -> `3`.
    pub(crate) fn n_puffer_slice(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let Some(r) = self.puffer_von(this) else {
            return Err(self.puffer_typfehler("ArrayBuffer"));
        };
        let (daten, geteilt) = match self.heap.holen(r) {
            HObj::BytePuffer { daten, geteilt, .. } => (daten.clone(), *geteilt),
            _ => return Err(self.puffer_typfehler("ArrayBuffer")),
        };
        let len = daten.len() as f64;
        let mut von = if args.is_empty() {
            0.0
        } else {
            self.zu_zahl(args[0])?
        };
        let mut bis = if args.len() < 2 {
            len
        } else {
            self.zu_zahl(args[1])?
        };
        // `ToInteger` statt `ToIndex`: negative Werte zaehlen von hinten, und
        // `undefined` heisst 0 bzw. `len`.
        von = if von.is_nan() { 0.0 } else { von.trunc() };
        bis = if bis.is_nan() { len } else { bis.trunc() };
        if von < 0.0 {
            von = (len + von).max(0.0);
        } else {
            von = von.min(len);
        }
        if bis < 0.0 {
            bis = (len + bis).max(0.0);
        } else {
            bis = bis.min(len);
        }
        let an = (bis - von).max(0.0) as usize;
        let neu = self.neuer_bytepuffer(an, geteilt)?;
        {
            let HObj::BytePuffer { daten: ziel, .. } = self.heap.holen_mut(neu) else {
                return Err(self.puffer_typfehler("ArrayBuffer"));
            };
            ziel[..an].copy_from_slice(&daten[von as usize..von as usize + an]);
        }
        self.wurzeln.push(GVal::Ref(neu));
        Ok(GVal::Ref(neu))
    }

    /// `new DataView(buffer[, byteOffset[, byteLength]])`.
    ///
    /// Byte offsets need not be aligned; offset and length must fit the buffer.
    pub(crate) fn n_dataview_ctor(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let arg0 = self.arg(args, 0);
        let Some(buf) = self.puffer_von(arg0) else {
            return Err(self.puffer_typfehler("ArrayBuffer"));
        };
        let blen = match self.heap.holen(buf) {
            HObj::BytePuffer { daten, .. } => daten.len(),
            _ => 0,
        };
        let versatz = self.typedarray_index(self.arg(args, 1))?;
        if versatz > blen {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "DataView: byteOffset ausserhalb des Puffers")));
        }
        let umfang = if matches!(self.arg(args, 2), GVal::Undefiniert) {
            blen - versatz
        } else {
            self.typedarray_index(self.arg(args, 2))?
        };
        if umfang > blen - versatz {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "DataView: byteLength ausserhalb des Puffers")));
        }
        let proto = self.proto_dataview;
        let o = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        // Die drei Felder sind **nicht aufzaehlbar** und nicht
        // konfigurierbar: sie gehoeren dem DataView, nicht dem Skript.
        for (name, wert) in [
            // Release 15.10.2026: private names, so `Reflect.ownKeys(dataView)`
            // is empty as in browsers (MEASURED before: three `__` keys).
            ("#es_dv_puffer", GVal::Ref(buf)),
            ("#es_dv_versatz", GVal::Zahl(versatz as f64)),
            ("#es_dv_umfang", GVal::Zahl(umfang as f64)),
        ] {
            let e = crate::heap::Eigenschaft {
                wert,
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: false,
            };
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(o) {
                eigenschaften.insert(name.to_string(), e);
            }
        }
        self.wurzeln.push(GVal::Ref(o));
        // Release 15.10.2026: length tracking over a resizable buffer.
        self.es_spur_markieren(GVal::Ref(o), args);
        Ok(GVal::Ref(o))
    }

    /// Die drei Felder eines `DataView` lesen: `(puffer, versatz, umfang)`.
    pub(crate) fn dataview_teile(&mut self, this: GVal) -> Result<(HeapRef, usize, usize), Stoerung> {
        let GVal::Ref(o) = this else {
            return Err(self.puffer_typfehler("DataView"));
        };
        let b = self.hole(this, "#es_dv_puffer")?;
        let v = self.hole(this, "#es_dv_versatz")?;
        let u = self.hole(this, "#es_dv_umfang")?;
        let Some(buf) = self.puffer_von(b) else {
            return Err(self.puffer_typfehler("ArrayBuffer"));
        };
        let z = self.zu_zahl(v).unwrap_or(0.0) as usize;
        let n = self.zu_zahl(u).unwrap_or(0.0) as usize;
        // Release 15.10.2026: a length-tracking view follows the buffer.
        let n = self.es_dataview_spur(o, buf, z).unwrap_or(n);
        Ok((buf, z, n))
    }

    /// `DataView.prototype.byteLength` / `byteOffset`.
    pub(crate) fn n_dataview_groesse(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, _, n) = self.dataview_teile(this)?;
        Ok(GVal::Zahl(n as f64))
    }

    pub(crate) fn n_dataview_versatz(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, z, _) = self.dataview_teile(this)?;
        Ok(GVal::Zahl(z as f64))
    }

    /// `buffer` — der Puffer dahinter.
    pub(crate) fn n_dataview_puffer(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (b, _, _) = self.dataview_teile(this)?;
        Ok(GVal::Ref(b))
    }

    /// `getX( byteOffset [ , littleEndian ] )` — ES2023 25.2.5.1 `GetViewValue`.
    ///
    /// `art` ist die Art aus dem **Methodennamen** (`getUint8` -> `Uint8`).
    /// GEMESSEN (node 22): `dv.getInt16(0, true)` und `(0, false)` liefern
    /// beide `-2` (der Unterschied zeigt sich erst am Bytebild); ein
    /// Offset ausserhalb liefert `RangeError`.
    pub(crate) fn n_dataview_get(
        &mut self, this: GVal, args: &[GVal], art: ElementArt,
    ) -> Result<GVal, Stoerung> {
        let (buf, versatz, sicht) = self.dataview_teile(this)?;
        let pos = self.typedarray_index(self.arg(args, 0))?;
        let klein = self.zu_bool(self.arg(args, 1));
        let groesse = art.bytes();
        if pos > sicht || groesse > sicht - pos {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "DataView: Offset ausserhalb der Sicht")));
        }
        let bei = versatz + pos;
        let HObj::BytePuffer { daten, .. } = self.heap.holen(buf) else { unreachable!() };
        if bei > daten.len() || groesse > daten.len() - bei {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "DataView: Puffer ausserhalb der Sicht")));
        }
        // Copy only the requested element, never the complete backing buffer.
        let mut roh = [0u8; 8];
        roh[..groesse].copy_from_slice(&daten[bei..bei + groesse]);
        if !klein { roh[..groesse].reverse(); }
        Ok(art.lesen(&roh, 0))
    }

    /// DataView writes are limited to the view, with conversion before bounds checks.
    pub(crate) fn n_dataview_set(
        &mut self, this: GVal, args: &[GVal], art: ElementArt,
    ) -> Result<GVal, Stoerung> {
        let (buf, versatz, sicht) = self.dataview_teile(this)?;
        let pos = self.typedarray_index(self.arg(args, 0))?;
        let wert = self.typedarray_schreibwert(art, self.arg(args, 1))?;
        let klein = self.zu_bool(self.arg(args, 2));
        let groesse = art.bytes();
        if pos > sicht || groesse > sicht - pos {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "DataView: Offset ausserhalb der Sicht")));
        }
        let bei = versatz + pos;
        let mut roh = [0u8; 8];
        art.schreiben(&mut roh[..groesse], 0, wert);
        if !klein { roh[..groesse].reverse(); }
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(buf) {
            if bei > daten.len() || groesse > daten.len() - bei {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "DataView: Puffer ausserhalb der Sicht")));
            }
            daten[bei..bei + groesse].copy_from_slice(&roh[..groesse]);
        }
        Ok(GVal::Undefiniert)
    }

    /// ToIndex: truncate finite values, rejecting negative and oversized indices.
    pub(crate) fn typedarray_index(&mut self, value: GVal) -> Result<usize, Stoerung> {
        let n = self.zu_zahl(value)?;
        let n = if n.is_nan() { 0.0 } else { n.trunc() };
        if n < 0.0 || n > 9_007_199_254_740_991.0 || n > usize::MAX as f64 {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "ungueltiger Byte- oder Elementindex")));
        }
        Ok(n as usize)
    }

    pub(crate) fn typedarray_schreibwert(&mut self, art: ElementArt, value: GVal) -> Result<Schreibwert, Stoerung> {
        if matches!(art, ElementArt::BigInt64 | ElementArt::BigUint64) {
            let v = self.zu_primitiv(value, false)?;
            if matches!(v, GVal::Zahl(_)) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "BigInt-TypedArray braucht BigInt-Werte")));
            }
            match self.n_bigint_ctor(&[v])? {
                GVal::Ganz(n) => Ok(Schreibwert::Ganz(n)),
                _ => unreachable!(),
            }
        } else {
            if matches!(value, GVal::Ganz(_)) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Number-TypedArray akzeptiert kein BigInt")));
            }
            Ok(Schreibwert::Zahl(self.zu_zahl(value)?))
        }
    }

    pub(crate) fn typedarray_element_setzen(&mut self, r: HeapRef, index: Option<usize>, value: GVal) -> Result<(), Stoerung> {
        let (_, art, offset, len, buffer) = self.typedarray_this(GVal::Ref(r))?;
        let value = self.typedarray_schreibwert(art, value)?;
        if let Some(i) = index.filter(|i| *i < len) {
            if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(buffer) {
                art.schreiben(daten, offset + i * art.bytes(), value);
            }
        }
        Ok(())
    }

    pub(crate) fn typedarray_ctor(&mut self, ctor_ref: HeapRef, args: &[GVal], art: ElementArt) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(ctor_ref));
        self.wurzeln.extend_from_slice(args);
        let result = self.typedarray_ctor_inner(ctor_ref, args, art);
        // Release 15.10.2026: `new TA(resizableBuffer)` tracks its length.
        if let Ok(v) = result {
            self.es_spur_markieren(v, args);
        }
        self.wurzel_zurueck(marke);
        result
    }

    fn typedarray_ctor_inner(&mut self, ctor_ref: HeapRef, args: &[GVal], art: ElementArt) -> Result<GVal, Stoerung> {
        let input = self.arg(args, 0);
        let mut values = None;
        let (puffer_ref, versatz, laenge) = if let GVal::Ref(r) = input {
            match self.heap.holen(r) {
                HObj::Zeichenkette(_) => {
                    let len = self.typedarray_index(input)?;
                    let bytes = len.checked_mul(art.bytes()).ok_or_else(|| Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray zu gross")))?;
                    (self.neuer_bytepuffer(bytes, false)?, 0, len)
                }
                HObj::BytePuffer { daten, .. } => {
                    let blen = daten.len();
                    let offset = self.typedarray_index(self.arg(args, 1))?;
                    if offset > blen || offset % art.bytes() != 0 {
                        return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "byteOffset ausserhalb oder nicht ausgerichtet")));
                    }
                    let len = if matches!(self.arg(args, 2), GVal::Undefiniert) {
                        if (blen - offset) % art.bytes() != 0 {
                            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Pufferlaenge ist nicht ausgerichtet")));
                        }
                        (blen - offset) / art.bytes()
                    } else {
                        let len = self.typedarray_index(self.arg(args, 2))?;
                        if len > (blen - offset) / art.bytes() {
                            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray reicht ueber den Puffer hinaus")));
                        }
                        len
                    };
                    (r, offset, len)
                }
                _ => {
                    let vals = if let HObj::TypedArray { art: source_art, puffer, versatz, laenge, .. } = self.heap.holen(r) {
                        let bigint = |a| matches!(a, ElementArt::BigInt64 | ElementArt::BigUint64);
                        if bigint(*source_art) != bigint(art) {
                            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Number und BigInt TypedArrays sind nicht mischbar")));
                        }
                        let HObj::BytePuffer { daten, .. } = self.heap.holen(*puffer) else { unreachable!() };
                        (0..*laenge).map(|i| source_art.lesen(daten, *versatz + i * source_art.bytes())).collect()
                    } else {
                        let iterator = self.hole(input, "@sym1")?;
                        if !matches!(iterator, GVal::Undefiniert | GVal::Null)
                            || matches!(self.heap.holen(r), HObj::Menge { .. } | HObj::Karte { .. }) {
                            self.iter_werte(input)?
                        } else {
                            let length = self.hole(input, "length")?;
                            let n = self.zu_zahl(length)?;
                            let len = if n.is_nan() || n < 0.0 { 0 } else { n.trunc().min(9_007_199_254_740_991.0) as usize };
                            if len > self.grenzen.max_heap_bytes / art.bytes() {
                                return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray zu gross")));
                            }
                            let mut v = Vec::with_capacity(len);
                            for i in 0..len {
                                let x = self.hole(input, &i.to_string())?;
                                self.wurzeln.push(x);
                                v.push(x);
                            }
                            v
                        }
                    };
                    self.wurzeln.extend_from_slice(&vals);
                    let len = vals.len();
                    values = Some(vals);
                    let bytes = len.checked_mul(art.bytes()).ok_or_else(|| Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray zu gross")))?;
                    (self.neuer_bytepuffer(bytes, false)?, 0, len)
                }
            }
        } else {
            let len = self.typedarray_index(input)?;
            let bytes = len.checked_mul(art.bytes()).ok_or_else(|| Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray zu gross")))?;
            (self.neuer_bytepuffer(bytes, false)?, 0, len)
        };
        self.wurzeln.push(GVal::Ref(puffer_ref));
        let proto = match self.heap.holen(ctor_ref) {
            HObj::Nativ { eigenschaften, .. } | HObj::Funktion { eigenschaften, .. } => {
                match eigenschaften.get("prototype").map(|v| v.wert) {
                    Some(GVal::Ref(q)) => q,
                    _ => self.proto_objekt,
                }
            }
            _ => self.proto_objekt,
        };
        let obj = HObj::TypedArray { proto: Some(proto), art, puffer: puffer_ref, versatz, laenge,
            eigenschaften: Default::default(), sperre: crate::heap::Sperre::Offen };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        if let Some(vals) = values {
            for (i, v) in vals.into_iter().enumerate() {
                self.typedarray_element_setzen(r, Some(i), v)?;
            }
        }
        Ok(GVal::Ref(r))
    }

    pub(crate) fn n_typedarray_ctor_uint8(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Uint8)
    }
    pub(crate) fn n_typedarray_ctor_int8(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Int8)
    }
    pub(crate) fn n_typedarray_ctor_uint8_clamped(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Uint8Clamped)
    }
    pub(crate) fn n_typedarray_ctor_uint16(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Uint16)
    }
    pub(crate) fn n_typedarray_ctor_int16(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Int16)
    }
    pub(crate) fn n_typedarray_ctor_uint32(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Uint32)
    }
    pub(crate) fn n_typedarray_ctor_int32(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Int32)
    }
    pub(crate) fn n_typedarray_ctor_float32(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Float32)
    }
    pub(crate) fn n_typedarray_ctor_float64(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::Float64)
    }
    pub(crate) fn n_typedarray_ctor_bigint64(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::BigInt64)
    }
    pub(crate) fn n_typedarray_ctor_biguint64(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.typedarray_ctor(ctor_ref, args, crate::eingebaut_typed::ElementArt::BigUint64)
    }

    // %TypedArray%.prototype Getter
    pub(crate) fn typedarray_this(&mut self, this: GVal) -> Result<(HeapRef, crate::eingebaut_typed::ElementArt, usize, usize, HeapRef), Stoerung> {
        let GVal::Ref(r) = this else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "nicht ein TypedArray")));
        };
        match self.heap.holen(r) {
            HObj::TypedArray { art, puffer, versatz, laenge, .. } => {
                // 2026-10-15: a view over a detached (or too short) buffer
                // is out of bounds: length, byteLength and byteOffset are 0
                // (ES2024 IsTypedArrayOutOfBounds).
                let (art, puffer, versatz, laenge) = (*art, *puffer, *versatz, *laenge);
                let ende = versatz.saturating_add(laenge.saturating_mul(art.bytes()));
                let im_rahmen = match self.heap.holen(puffer) {
                    HObj::BytePuffer { daten, detached, .. } => !*detached && ende <= daten.len(),
                    _ => true,
                };
                if im_rahmen {
                    Ok((r, art, versatz, laenge, puffer))
                } else {
                    Ok((r, art, 0, 0, puffer))
                }
            }
            _ => Err(Stoerung::Wurf(self.fehler_wert("TypeError", "nicht ein TypedArray"))),
        }
    }

    pub(crate) fn n_typedarray_laenge(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, _, _, laenge, _) = self.typedarray_this(this)?;
        Ok(GVal::Zahl(laenge as f64))
    }

    pub(crate) fn n_typedarray_bytelaenge(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, art, _, laenge, _) = self.typedarray_this(this)?;
        Ok(GVal::Zahl((laenge * art.bytes()) as f64))
    }

    pub(crate) fn n_typedarray_byteversatz(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, _, versatz, _, _) = self.typedarray_this(this)?;
        Ok(GVal::Zahl(versatz as f64))
    }

    pub(crate) fn n_typedarray_puffer(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, _, _, _, puffer) = self.typedarray_this(this)?;
        Ok(GVal::Ref(puffer))
    }

    pub(crate) fn n_typedarray_bytes_per_element(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, art, _, _, _) = self.typedarray_this(this)?;
        Ok(GVal::Zahl(art.bytes() as f64))
    }

    pub(crate) fn n_typedarray_join(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let (_, _, _, len, _) = self.typedarray_this(this)?;
        let separator = if matches!(self.arg(args, 0), GVal::Undefiniert) { ",".into() } else { self.zu_text(self.arg(args, 0))? };
        let mut text = String::new();
        for i in 0..len {
            if i != 0 { text.push_str(&separator); }
            let value = self.hole(this, &i.to_string())?;
            text.push_str(&self.zu_text(value)?);
            self.string_grenze(text.len()).map_err(Stoerung::Fehler)?;
        }
        self.text_wert(&text).map_err(Stoerung::Fehler)
    }

    // TypedArray sources are snapshotted for overlapping views. Array-like sources
    // are read live after validating their complete length, including getters.
    pub(crate) fn n_typedarray_set(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(this);
        self.wurzeln.extend_from_slice(args);
        let result = self.typedarray_set_inner(this, args);
        self.wurzel_zurueck(marke);
        result
    }

    fn typedarray_set_inner(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let (target, art, _, len, _) = self.typedarray_this(this)?;
        let offset = self.typedarray_index(self.arg(args, 1))?;
        if offset > len {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray.set Offset ausserhalb")));
        }
        let source = self.arg(args, 0);
        if matches!(source, GVal::Null | GVal::Undefiniert) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "TypedArray.set braucht eine Quelle")));
        }
        let snapshot = if let GVal::Ref(r) = source {
            if let HObj::TypedArray { art: a, puffer, versatz, laenge, .. } = self.heap.holen(r) {
                let bigint = |a| matches!(a, ElementArt::BigInt64 | ElementArt::BigUint64);
                if bigint(*a) != bigint(art) {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Number und BigInt TypedArrays sind nicht mischbar")));
                }
                if *laenge > len - offset {
                    return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Quelle zu gross fuer TypedArray.set")));
                }
                let HObj::BytePuffer { daten, .. } = self.heap.holen(*puffer) else { unreachable!() };
                Some((0..*laenge).map(|i| a.lesen(daten, *versatz + i * a.bytes())).collect::<Vec<_>>())
            } else { None }
        } else { None };
        if let Some(values) = snapshot {
            for (i, value) in values.into_iter().enumerate() {
                self.typedarray_element_setzen(target, Some(offset + i), value)?;
            }
        } else {
            let length = self.hole(source, "length")?;
            let n = self.zu_zahl(length)?;
            let count = if n.is_nan() || n < 0.0 { 0 } else { n.trunc() as usize };
            if count > len - offset {
                return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Quelle zu gross fuer TypedArray.set")));
            }
            for i in 0..count {
                let value = self.hole(source, &i.to_string())?;
                self.typedarray_element_setzen(target, Some(offset + i), value)?;
            }
        }
        Ok(GVal::Undefiniert)
    }

    // %TypedArray%.prototype.subarray(begin, end?)
    pub(crate) fn n_typedarray_subarray(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let (_, art, versatz, laenge, puffer) = self.typedarray_this(this)?;
        let begin = if args.is_empty() {
            0
        } else {
            let v = self.zu_zahl(args[0]).unwrap_or(0.0);
            if v < 0.0 {
                (laenge as f64 + v).max(0.0) as usize
            } else {
                (v as usize).min(laenge)
            }
        };
        let end = if args.len() < 2 {
            laenge
        } else {
            let v = self.zu_zahl(args[1]).unwrap_or(laenge as f64);
            if v < 0.0 {
                (laenge as f64 + v).max(0.0) as usize
            } else {
                (v as usize).min(laenge)
            }
        };
        let new_len = end.saturating_sub(begin);
        let new_offset = versatz + begin * art.bytes();
        // Release 15.10.2026: TypedArraySpeciesCreate(O, «buffer, offset,
        // length») (23.2.3.30). MEASURED before (node 22 in brackets):
        // `class M extends Uint8Array {}; new M(4).subarray(1) instanceof M`
        // false (true).
        if let GVal::Ref(o) = this
            && let Some(c) = self.ta_spezies(o, art)?
        {
            let r = self.ta_aus_spezies(c, &[GVal::Ref(puffer), GVal::Zahl(new_offset as f64), GVal::Zahl(new_len as f64)], art, 0)?;
            return Ok(GVal::Ref(r));
        }

        // Neues TypedArray mit gleichem Puffer aber anderem Offset/Laenge
        // Konstruktor ueber globalen Bereich holen
        let ctor_name = art.name();
        let ctor_val = self.hole(GVal::Ref(self.global_objekt), ctor_name).unwrap_or(GVal::Undefiniert);
        let GVal::Ref(ctor_ref) = ctor_val else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Konstruktor nicht gefunden")));
        };
        // Neues Objekt manuell anlegen (wie im Konstruktor, aber mit gegebenem Puffer/Offset/Laenge)
        let proto = match self.heap.holen(ctor_ref).clone() {
            HObj::Nativ { eigenschaften, .. } | HObj::Funktion { eigenschaften, .. } => {
                match eigenschaften.get("prototype") {
                    Some(v) => match v.wert {
                        GVal::Ref(q) => q,
                        _ => self.proto_objekt,
                    },
                    _ => self.proto_objekt,
                }
            }
            _ => self.proto_objekt,
        };
        let obj = HObj::TypedArray {
            proto: Some(proto),
            art,
            puffer,
            versatz: new_offset,
            laenge: new_len,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(GVal::Ref(r))
    }
}

/// ES2023 7.1.5 `ToIndex`: `ToIntegerOrInfinity`, und negativ oder `NaN`
/// ergibt `0`. Fuer die Laenge eines Puffers ist das genau die Regel — und
/// der Aufrufer prueft danach die **Heap-Grenze**, weil `ToIndex` nach oben
/// unbeschaenkt ist.
fn to_index(n: f64) -> u64 {
    if n.is_nan() || n <= 0.0 {
        return 0;
    }
    if n >= 18_446_744_073_709_551_616.0 {
        return u64::MAX;
    }
    n.trunc() as u64
}

fn ganz_modulo(n: f64, bits: i32) -> u64 {
    if !n.is_finite() || n == 0.0 { 0 } else { n.trunc().rem_euclid(2f64.powi(bits)) as u64 }
}

pub(crate) fn typedarray_numerischer_schluessel(name: &str) -> bool {
    matches!(name, "-0" | "NaN" | "Infinity" | "-Infinity") ||
        name.parse::<f64>().is_ok_and(|n| n.to_string() == name)
}
