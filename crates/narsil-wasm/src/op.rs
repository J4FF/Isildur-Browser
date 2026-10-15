//! Befehlssatz: WebAssembly 2.0 Core ohne SIMD/Threads/Tail-Call (Auftrag W2
//! nennt ausdruecklich: MVP + sign-ext + sat-trunc + bulk-memory +
//! multi-value + reference-types).
//!
//! Eine Datei, drei Aufgaben:
//!   * `lies_befehl` liest Opcode + Immediates aus der Binarform (W1),
//!   * `code()` schreibt einen Befehl wieder binaer (fuer Tests und den
//!     Modul-Bau aus WAT, W3),
//!   * `text()` erzeugt die WAT-Textzeile fuer die Disassembly der
//!     Entwicklerwerkzeuge (W5, `tools.browser`-Reiter WebAssembly).
//!
//! SIMD (`0xFD`, `v128`) wird bewusst abgelehnt statt ignoriert: ein Modul
//! mit SIMD soll einen *benannten* Fehler liefern, nicht still ein
//! falsches Ergebnis (team/REGELN.txt: "Kein Panic bei Eingaben").

use crate::bytes::Leser;
use crate::fehler::{ART_BLOCKTYP, ART_OPCODE, ART_OPCODE_PRAEFIX, ART_UNVOLLSTAENDIG, Fehler};
use crate::grenzen::MAX_VERSCHACHTELUNG_HART;
use crate::modell::{Reftyp, Valtyp, valtyp_lesen};

/// Blocktyp: leer (`0x40`), ein Werttyp (Bytes `0x7f`..) oder Typindex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blockart {
    Leer,
    Werttyp(Valtyp),
    Typ(u32),
}

impl Blockart {
    pub fn ergebnis(&self, typen: &[crate::modell::FuncTyp]) -> Vec<Valtyp> {
        match self {
            Blockart::Leer => Vec::new(),
            Blockart::Werttyp(v) => vec![*v],
            Blockart::Typ(i) => typen
                .get(*i as usize)
                .map(|t| t.ergebnis.clone())
                .unwrap_or_default(),
        }
    }
    pub fn parameter(&self, typen: &[crate::modell::FuncTyp]) -> Vec<Valtyp> {
        match self {
            Blockart::Leer | Blockart::Werttyp(_) => Vec::new(),
            Blockart::Typ(i) => typen
                .get(*i as usize)
                .map(|t| t.parameter.clone())
                .unwrap_or_default(),
        }
    }
    pub fn text(&self, typen: &[crate::modell::FuncTyp]) -> String {
        match self {
            Blockart::Leer => String::new(),
            Blockart::Werttyp(v) => format!(" (result {})", v.text()),
            Blockart::Typ(i) => match typen.get(*i as usize) {
                None => format!(" (typ {i})"),
                Some(t) => {
                    if t.parameter.is_empty() && t.ergebnis.len() == 1 {
                        format!(" (result {})", t.ergebnis[0].text())
                    } else {
                        let p: Vec<&str> = t.parameter.iter().map(|v| v.text()).collect();
                        let e: Vec<&str> = t.ergebnis.iter().map(|v| v.text()).collect();
                        format!(" ({} {})", p.join(" "), e.join(" "))
                    }
                }
            },
        }
    }
}

/// Lesefehler bei einem Blocktyp.
pub fn blocktyp_lesefehler(t: impl std::fmt::Display) -> Fehler {
    Fehler::form(ART_BLOCKTYP, format!("Blocktyp {t} ungueltig"))
}

/// Ein `catch`-Zweig von `try_table`: `tag` ist der Tag-Index (`None` =
/// `catch_all`), `label` die Label-Tiefe (`None` = an den Aufrufer
/// weiterwerfen).
///
/// Exception Handling, Wasm 3.0. GEMESSEN 02.10. (`instance.wast`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fang {
    pub tag: Option<u32>,
    pub label: Option<u32>,
}

/// Ein Befehl mit seinen Immediates.
#[derive(Debug, Clone, PartialEq)]
pub enum Befehl {
    // ---- Kontrolle --------------------------------------------------------
    Unreachable,
    Nop,
    /// `try_table <blocktyp> <catch>*` (Exception Handling, Wasm 3.0).
    TryTable(Blockart, Vec<Fang>),
    /// `throw <tagindex>`.
    Throw(u32),
    /// `array.new_default <typindex> <laenge>` — ein Array mit
    /// Standardwerten (Wasm 3.0 GC). GEMESSEN 03.10. (`table_init.wast`).
    ArrayNewDefault(u32),
    /// `struct.new $t` — die Feldwerte liegen in Reihenfolge auf dem Stapel.
    ///
    /// GEMESSEN 03.10. (`struct.wast` Z. 55).
    StructNew(u32),
    /// `struct.get $t $f` / `struct.get_s` / `struct.get_u`: das Feld `f`.
    /// `_s`/`_u` sind die gepackten Formen (nur `i8`/`i16`).
    StructGet(u32, u32),
    StructGetS(u32, u32),
    StructGetU(u32, u32),
    /// `struct.set $t $f (x, v)`: schreibt ein Feld (nur `(mut …)`).
    StructSet(u32, u32),
    /// `array.new $t (length, value)`: ein Array mit `value` in jedem
    /// Platz.
    ///
    /// GEMESSEN 03.10. (`array.wast`: 47 Faelle).
    ArrayNew(u32),
    /// `array.len x`: die Zahl der Elemente.
    ArrayLen,
    /// `array.get $t x` / `array.get_s` / `array.get_u`: das Element an
    /// Index `i` — `get` nimmt den deklarierten Typ, `_s`/`_u` sind die
    /// gepackten Formen (Vorzeichen bzw. 31/32 Bit).
    ArrayGet(u32),
    ArrayGetS(u32),
    ArrayGetU(u32),
    /// `array.set $t x (i, v)`: schreibt ein Element.
    ArraySet(u32),
    /// `br_on_cast $l rt1 rt2` / `br_on_cast_fail $l rt1 rt2` (Wasm 3.0 GC):
    /// Verzweigen, wenn der Referenzwert zu `rt2` passt (`_fail` kehrt es
    /// um).
    ///
    /// GEMESSEN 03.10. (`br_on_cast.wast` 6/31, `br_on_cast_fail.wast` 6/31).
    BrOnCast(u32, Reftyp, Reftyp),
    BrOnCastFail(u32, Reftyp, Reftyp),
    /// `array.new_fixed $t n`: die `n` Werte auf dem Stapel (erstes Feld
    /// zuerst) fuellen den Array.
    ArrayNewFixed(u32, u32),
    /// `array.new_data $t $d (offset, size)`: aus einem Datenabschnitt
    /// (Bytes, pro Element `element_breite`).
    ArrayNewData(u32, u32),
    /// `array.new_elem $t $e (offset, size)`: aus einem Elementabschnitt.
    ArrayNewElem(u32, u32),
    /// `array.init_data $t $d (a, i, doff, size)`: schreibt in einen
    /// vorhandenen Array.
    ArrayInitData(u32, u32),
    /// `array.init_elem $t $e (a, i, eoff, size)`.
    ArrayInitElem(u32, u32),
    /// `array.fill $t (a, i, v, n)`: schreibt `n` mal den Wert `v` ab
    /// Index `i`.
    ///
    /// GEMESSEN 03.10. (`array_fill.wast`: 3/29 -> ?).
    ArrayFill(u32),
    /// `array.copy $d $s (d, di, s, si, n)`: kopiert `n` Elemente aus `s`
    /// ab `si` nach `d` ab `di` (wie `memory.copy`, also auch bei
    /// Ueberlappung).
    ///
    /// GEMESSEN 03.10. (`array_copy.wast`: 4/34 -> ?).
    ArrayCopy(u32, u32),
    /// `ref.eq` — Identitaet zweier GC-Referenzen.
    RefEq,
    /// `ref.i31 x` — ein i31-Wert (31 Bit) im GC-Heap.
    ///
    /// GEMESSEN 03.10. (`extern.wast`: 16 Faelle).
    RefI31,
    /// `struct.new_default $t` — eine Struktur mit Standardwerten.
    ///
    /// GEMESSEN 03.10. (`extern.wast`).
    StructNewDefault(u32),
    /// `ref.test <typ> x`: 1, wenn der Wert zum Typ passt, sonst 0.
    ///
    /// GEMESSEN 03.10. (`ref_test.wast`: 68 Faelle, `ref_cast.wast`: 40,
    /// `i31.wast`: 17 — zusammen 125).
    RefTest(Reftyp),
    /// `ref.cast <typ> x`: wie `ref.test`, aber der Wert wird auf den
    /// Zieltyp verengt; passt er nicht, ist es ein Trap.
    RefCast(Reftyp),
    /// `i31.get_s x` / `i31.get_u x`: der i31-Wert als `i32`,
    /// vorzeichenbehaftet (`_s`) oder auf 31 Bit maskiert (`_u`).
    ///
    /// GEMESSEN 03.10. (`i31.wast`: 57 Faelle).
    I31GetS,
    I31GetU,
    /// `any.convert_extern x` — `externref` -> `anyref`.
    ///
    /// GEMESSEN 03.10. (`extern.wast`).
    AnyConvertExtern,
    /// `extern.convert_any x` — `anyref` -> `externref`. Ein GC-Objekt
    /// wird zu `null`: es hat keine externe Entsprechung.
    ///
    /// GEMESSEN 03.10. (`extern.wast`).
    ExternConvertAny,
    Block(Blockart),
    Loop(Blockart),
    If(Blockart),
    Else,
    End,
    Br(u32),
    BrIf(u32),
    /// Sprungtabelle: Ziele + Vorgabe.
    BrTable(Vec<u32>, u32),
    Return,
    Call(u32),
    /// `call_indirect`: Typindex + Tabellenindex (Referenztypen: != 0).
    CallIndirect {
        typ: u32,
        tabelle: u32,
    },

    // ---- Referenztypen ----------------------------------------------------
    RefNull(Reftyp),
    RefIsNull,
    RefFunc(u32),
    /// `ref.as_non_null` (0xd3, Wasm 3.0 Typed Function References):
    /// `(ref null X)` -> `(ref X)`.
    RefAsNonNull,
    /// `return_call $f` (0x12): END-AUFRUF. Die Parameter der
    /// aufgerufenen Funktion sind das ERGEBNIS des aufrufenden
    /// Rahmens; der Rahmen wird ersetzt, nicht gestapelt (Wasm 3.0,
    /// Tail Calls).
    ///
    /// GEMESSEN 03.10. (`return_call*.wast`: 119 Faelle).
    ReturnCall(u32),
    /// `return_call_indirect $t` (0x13) mit optionalem Tabellenindex.
    ReturnCallIndirect {
        typ: u32,
        tabelle: u32,
    },
    /// `call_ref $t` (0x14): ruft die Funktion hinter einer
    /// Funktionsreferenz vom Typ `$t` auf. Ohne Tabellenindex — der Typ
    /// ENTHAELT die Signatur.
    ///
    /// GEMESSEN 03.10. (`unreached-valid.wast`: 9 `assert_trap`-Faelle).
    CallRef(u32),
    /// `br_on_null $l` (0xd5): springt, wenn die Referenz `null` ist.
    BrOnNull(u32),
    /// `br_on_non_null $l <typ>` (Wasm 3.0, Typed Function References /
    /// GC): springt, wenn der Referenzwert **nicht** `null` ist, und legt
    /// ihn danach mit dem nicht-nullbaren Typ ab.
    ///
    /// GEMESSEN 03.10.: `br_on_cast_fail.wast` (3/31) brauchte genau diesen
    /// Befehl — ohne ihn war das ganze Modul unlesbar („Befehl
    /// \"br_on_non_null\" unbekannt").
    ///
    /// **KEIN Typ-Flag** (GEMESSEN an `br_on_non_null.wast` Z. 7:
    /// `(br_on_non_null $l (local.get $r))`): der Typ steht im Stapel, der
    /// Befehl nimmt nur die Beschriftung — wie `br_on_null`.
    BrOnNonNull(u32),
    /// `return_call_ref $t` (0x15): END-AUFRUF ueber eine
    /// Funktionsreferenz vom Typ `$t` — ohne Tabelle.
    ReturnCallRef(u32),

    // ---- Parametrisch -----------------------------------------------------
    Drop,
    Select,
    /// `select` mit Typenliste (Wasm 2.0 Reference Types).
    SelectTyp(Vec<Valtyp>),

    // ---- Variablen --------------------------------------------------------
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),

    // ---- Speicher laden ---------------------------------------------------
    I32Load {
        versatz: u32,
    },
    I64Load {
        versatz: u32,
    },
    F32Load {
        versatz: u32,
    },
    F64Load {
        versatz: u32,
    },
    I32Load8S {
        versatz: u32,
    },
    I32Load8U {
        versatz: u32,
    },
    I32Load16S {
        versatz: u32,
    },
    I32Load16U {
        versatz: u32,
    },
    I64Load8S {
        versatz: u32,
    },
    I64Load8U {
        versatz: u32,
    },
    I64Load16S {
        versatz: u32,
    },
    I64Load16U {
        versatz: u32,
    },
    I64Load32S {
        versatz: u32,
    },
    I64Load32U {
        versatz: u32,
    },

    // ---- Speicher schreiben -----------------------------------------------
    I32Store {
        versatz: u32,
    },
    I64Store {
        versatz: u32,
    },
    F32Store {
        versatz: u32,
    },
    F64Store {
        versatz: u32,
    },
    I32Store8 {
        versatz: u32,
    },
    I32Store16 {
        versatz: u32,
    },
    I64Store8 {
        versatz: u32,
    },
    I64Store16 {
        versatz: u32,
    },
    I64Store32 {
        versatz: u32,
    },
    /// Zugriff auf einen bestimmten Speicher (Multi-Memory, Wasm 3.0).
    /// Der innere Befehl ist ein Lade-/Speicherbefehl oder `memory.*`.
    ///
    /// Warum eine Huelle statt eines Feldes in 22 Varianten: WebAssembly 2.0
    /// hat genau EINEN Speicher, der Index ist also immer 0. Die Huelle
    /// entsteht nur dort, wo er wirklich vorkommt — Textform
    /// `(i32.load $m1 …)` oder memarg mit Bit 6 der Ausrichtung.
    MitSpeicher {
        befehl: Box<Befehl>,
        speicher: u32,
    },
    MemorySize,
    MemoryGrow,
    /// `memory.init` (Bulk Memory).
    MemoryInit(u32),
    DataDrop(u32),
    /// `memory.copy ziel quelle` — Wasm 3.0 (Multi-Memory) nennt BEIDE
    /// Speicher; ohne Angabe ist es `(memory.copy 0 0)`.
    ///
    /// GEMESSEN 03.10. (`memory_copy1.wast` 4 Faelle: der Befehl trug die
    /// Speicherindizes nicht, `(memory.copy $a $b …)` kopierte in den
    /// falschen Speicher).
    MemoryCopy {
        ziel: u32,
        quelle: u32,
    },
    MemoryFill,
    TableInit {
        element: u32,
        tabelle: u32,
    },
    ElemDrop(u32),
    TableGet(u32),
    TableSet(u32),
    TableCopy {
        ziel: u32,
        quelle: u32,
    },
    TableGrow(u32),
    TableSize(u32),
    TableFill(u32),

    // ---- Konstanten -------------------------------------------------------
    I32Konst(i32),
    I64Konst(i64),
    /// f32 als Bitmuster: NaN-Bitfaelle bleiben so bitgenau erhalten
    /// (Wichtig fuer `f32.wast`/`conversions.wast` der Testsuite).
    F32Konst(u32),
    F64Konst(u64),

    // ---- i32 --------------------------------------------------------------
    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,

    // ---- i64 --------------------------------------------------------------
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,

    // ---- f32 --------------------------------------------------------------
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
    F32Sqrt,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32Copysign,

    // ---- f64 --------------------------------------------------------------
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
    F64Sqrt,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    F64Copysign,

    // ---- Konvertierungen --------------------------------------------------
    I32WrapI64,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64ExtendI32S,
    I64ExtendI32U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,
    I32ReinterpretF32,
    I64ReinterpretF64,
    F32ReinterpretI32,
    F64ReinterpretI64,

    // ---- sign-ext (Wasm 2.0) ----------------------------------------------
    I32Extend8S,
    I32Extend16S,
    I64Extend8S,
    I64Extend16S,
    I64Extend32S,

    // ---- sat-trunc (Wasm 2.0, Praefix 0xFC 0..7) --------------------------
    I32TruncSatF32S,
    I32TruncSatF32U,
    I32TruncSatF64S,
    I32TruncSatF64U,
    I64TruncSatF32S,
    I64TruncSatF32U,
    I64TruncSatF64S,
    I64TruncSatF64U,
}

/// Opcode-Nummer fuer die Binarform. `0xFC00 | n` = Praefix-Befehle.
/// `0` bedeutet: nicht darstellbar (kommt nicht vor).
impl Befehl {
    pub fn code(&self) -> u32 {
        use Befehl::*;
        match self {
            Unreachable => 0x00,
            Nop => 0x01,
            TryTable(_, _) => 0x1f,
            Throw(_) => 0x08,
            // `ref.eq` hat kein eigenes Ein-Byte-Opcode (Praefix 0xfb).
            RefEq => 0,
            ArrayNewDefault(_) => 0,
            // Alle haben kein Ein-Byte-Opcode (Praefix 0xfb).
            StructNew(_) => 0,
            BrOnCast(_, _, _) | BrOnCastFail(_, _, _) => 0,
            StructGet(_, _) | StructGetS(_, _) | StructGetU(_, _) | StructSet(_, _) => 0,
            ArrayNew(_) => 0,
            ArrayLen => 0,
            ArrayGet(_) | ArrayGetS(_) | ArrayGetU(_) | ArraySet(_) => 0,
            ArrayFill(_) => 0,
            ArrayCopy(_, _) => 0,
            ArrayNewFixed(_, _) => 0,
            ArrayNewData(_, _) | ArrayNewElem(_, _) => 0,
            ArrayInitData(_, _) | ArrayInitElem(_, _) => 0,
            RefTest(_) => 0,
            RefCast(_) => 0,
            I31GetS => 0,
            I31GetU => 0,
            RefI31 => 0,
            StructNewDefault(_) => 0,
            AnyConvertExtern => 0,
            ExternConvertAny => 0,
            Block(_) => 0x02,
            Loop(_) => 0x03,
            If(_) => 0x04,
            Else => 0x05,
            End => 0x0b,
            Br(_) => 0x0c,
            BrIf(_) => 0x0d,
            BrTable(_, _) => 0x0e,
            Return => 0x0f,
            Call(_) => 0x10,
            CallIndirect { .. } => 0x11,
            RefNull(_) => 0xd0,
            RefIsNull => 0xd1,
            RefFunc(_) => 0xd2,
            ReturnCall(_) => 0x12,
            ReturnCallIndirect { .. } => 0x13,
            RefAsNonNull => 0xd3,
            CallRef(_) => 0x14,
            BrOnNull(_) => 0xd5,
            // Keine Nummer: siehe Anmerkung am 0xfb-Zweig. Der Befehl hat
            // bisher nur die Textform; eine falsche Nummer wuerde
            // Binaermodule still falsch lesen, das ist schlimmer als eine
            // saubere „unbekannter Opcode"-Meldung.
            BrOnNonNull(_) => 0,
            ReturnCallRef(_) => 0x15,
            Drop => 0x1a,
            Select => 0x1b,
            SelectTyp(_) => 0x1c,
            LocalGet(_) => 0x20,
            LocalSet(_) => 0x21,
            LocalTee(_) => 0x22,
            GlobalGet(_) => 0x23,
            GlobalSet(_) => 0x24,
            I32Load { .. } => 0x28,
            I64Load { .. } => 0x29,
            F32Load { .. } => 0x2a,
            F64Load { .. } => 0x2b,
            I32Load8S { .. } => 0x2c,
            I32Load8U { .. } => 0x2d,
            I32Load16S { .. } => 0x2e,
            I32Load16U { .. } => 0x2f,
            I64Load8S { .. } => 0x30,
            I64Load8U { .. } => 0x31,
            I64Load16S { .. } => 0x32,
            I64Load16U { .. } => 0x33,
            I64Load32S { .. } => 0x34,
            I64Load32U { .. } => 0x35,
            I32Store { .. } => 0x36,
            I64Store { .. } => 0x37,
            F32Store { .. } => 0x38,
            F64Store { .. } => 0x39,
            I32Store8 { .. } => 0x3a,
            I32Store16 { .. } => 0x3b,
            I64Store8 { .. } => 0x3c,
            I64Store16 { .. } => 0x3d,
            I64Store32 { .. } => 0x3e,
            // Die Huelle hat keinen eigenen Opcode: `schreibe_binaer`
            // entpackt sie (mit memarg-Bit 6 bzw. Speicherindex).
            MitSpeicher { .. } => 0,
            MemorySize => 0x3f,
            MemoryGrow => 0x40,
            I32Konst(_) => 0x41,
            I64Konst(_) => 0x42,
            F32Konst(_) => 0x43,
            F64Konst(_) => 0x44,
            I32Eqz => 0x45,
            I32Eq => 0x46,
            I32Ne => 0x47,
            I32LtS => 0x48,
            I32LtU => 0x49,
            I32GtS => 0x4a,
            I32GtU => 0x4b,
            I32LeS => 0x4c,
            I32LeU => 0x4d,
            I32GeS => 0x4e,
            I32GeU => 0x4f,
            I64Eqz => 0x50,
            I64Eq => 0x51,
            I64Ne => 0x52,
            I64LtS => 0x53,
            I64LtU => 0x54,
            I64GtS => 0x55,
            I64GtU => 0x56,
            I64LeS => 0x57,
            I64LeU => 0x58,
            I64GeS => 0x59,
            I64GeU => 0x5a,
            F32Eq => 0x5b,
            F32Ne => 0x5c,
            F32Lt => 0x5d,
            F32Gt => 0x5e,
            F32Le => 0x5f,
            F32Ge => 0x60,
            F64Eq => 0x61,
            F64Ne => 0x62,
            F64Lt => 0x63,
            F64Gt => 0x64,
            F64Le => 0x65,
            F64Ge => 0x66,
            I32Clz => 0x67,
            I32Ctz => 0x68,
            I32Popcnt => 0x69,
            I32Add => 0x6a,
            I32Sub => 0x6b,
            I32Mul => 0x6c,
            I32DivS => 0x6d,
            I32DivU => 0x6e,
            I32RemS => 0x6f,
            I32RemU => 0x70,
            I32And => 0x71,
            I32Or => 0x72,
            I32Xor => 0x73,
            I32Shl => 0x74,
            I32ShrS => 0x75,
            I32ShrU => 0x76,
            I32Rotl => 0x77,
            I32Rotr => 0x78,
            I64Clz => 0x79,
            I64Ctz => 0x7a,
            I64Popcnt => 0x7b,
            I64Add => 0x7c,
            I64Sub => 0x7d,
            I64Mul => 0x7e,
            I64DivS => 0x7f,
            I64DivU => 0x80,
            I64RemS => 0x81,
            I64RemU => 0x82,
            I64And => 0x83,
            I64Or => 0x84,
            I64Xor => 0x85,
            I64Shl => 0x86,
            I64ShrS => 0x87,
            I64ShrU => 0x88,
            I64Rotl => 0x89,
            I64Rotr => 0x8a,
            F32Abs => 0x8b,
            F32Neg => 0x8c,
            F32Ceil => 0x8d,
            F32Floor => 0x8e,
            F32Trunc => 0x8f,
            F32Nearest => 0x90,
            F32Sqrt => 0x91,
            F32Add => 0x92,
            F32Sub => 0x93,
            F32Mul => 0x94,
            F32Div => 0x95,
            F32Min => 0x96,
            F32Max => 0x97,
            F32Copysign => 0x98,
            F64Abs => 0x99,
            F64Neg => 0x9a,
            F64Ceil => 0x9b,
            F64Floor => 0x9c,
            F64Trunc => 0x9d,
            F64Nearest => 0x9e,
            F64Sqrt => 0x9f,
            F64Add => 0xa0,
            F64Sub => 0xa1,
            F64Mul => 0xa2,
            F64Div => 0xa3,
            F64Min => 0xa4,
            F64Max => 0xa5,
            F64Copysign => 0xa6,
            I32WrapI64 => 0xa7,
            I32TruncF32S => 0xa8,
            I32TruncF32U => 0xa9,
            I32TruncF64S => 0xaa,
            I32TruncF64U => 0xab,
            I64ExtendI32S => 0xac,
            I64ExtendI32U => 0xad,
            I64TruncF32S => 0xae,
            I64TruncF32U => 0xaf,
            I64TruncF64S => 0xb0,
            I64TruncF64U => 0xb1,
            F32ConvertI32S => 0xb2,
            F32ConvertI32U => 0xb3,
            F32ConvertI64S => 0xb4,
            F32ConvertI64U => 0xb5,
            F32DemoteF64 => 0xb6,
            F64ConvertI32S => 0xb7,
            F64ConvertI32U => 0xb8,
            F64ConvertI64S => 0xb9,
            F64ConvertI64U => 0xba,
            F64PromoteF32 => 0xbb,
            I32ReinterpretF32 => 0xbc,
            I64ReinterpretF64 => 0xbd,
            F32ReinterpretI32 => 0xbe,
            F64ReinterpretI64 => 0xbf,
            I32Extend8S => 0xc0,
            I32Extend16S => 0xc1,
            I64Extend8S => 0xc2,
            I64Extend16S => 0xc3,
            I64Extend32S => 0xc4,
            I32TruncSatF32S => 0xfc00,
            I32TruncSatF32U => 0xfc01,
            I32TruncSatF64S => 0xfc02,
            I32TruncSatF64U => 0xfc03,
            I64TruncSatF32S => 0xfc04,
            I64TruncSatF32U => 0xfc05,
            I64TruncSatF64S => 0xfc06,
            I64TruncSatF64U => 0xfc07,
            MemoryInit(_) => 0xfc08,
            DataDrop(_) => 0xfc09,
            MemoryCopy { .. } => 0xfc0a,
            MemoryFill => 0xfc0b,
            TableGet(_) => 0x25,
            TableSet(_) => 0x26,
            TableInit { .. } => 0xfc0c,
            ElemDrop(_) => 0xfc0d,
            TableCopy { .. } => 0xfc0e,
            TableGrow(_) => 0xfc0f,
            TableSize(_) => 0xfc10,
            TableFill(_) => 0xfc11,
        }
    }
}

/// Befehl aus seinem Textnamen (W3: der WAT-Leser). Rund 150 Namen,
/// deshalb eine Tabelle statt eines `match` mit 150 Armen.
fn befehl_aus_text(name: &str) -> Option<u32> {
    /// (Text, Opcode-Nummer; 0xFCxx = Praefix-Befehl)
    const TABELLE: &[(&str, u32)] = &[
        ("unreachable", 0x00),
        ("nop", 0x01),
        ("return", 0x0f),
        ("drop", 0x1a),
        ("select", 0x1b),
        ("ref.is_null", 0xd1),
        ("memory.size", 0x3f),
        ("memory.grow", 0x40),
        ("i32.eqz", 0x45),
        ("i32.eq", 0x46),
        ("i32.ne", 0x47),
        ("i32.lt_s", 0x48),
        ("i32.lt_u", 0x49),
        ("i32.gt_s", 0x4a),
        ("i32.gt_u", 0x4b),
        ("i32.le_s", 0x4c),
        ("i32.le_u", 0x4d),
        ("i32.ge_s", 0x4e),
        ("i32.ge_u", 0x4f),
        ("i64.eqz", 0x50),
        ("i64.eq", 0x51),
        ("i64.ne", 0x52),
        ("i64.lt_s", 0x53),
        ("i64.lt_u", 0x54),
        ("i64.gt_s", 0x55),
        ("i64.gt_u", 0x56),
        ("i64.le_s", 0x57),
        ("i64.le_u", 0x58),
        ("i64.ge_s", 0x59),
        ("i64.ge_u", 0x5a),
        ("f32.eq", 0x5b),
        ("f32.ne", 0x5c),
        ("f32.lt", 0x5d),
        ("f32.gt", 0x5e),
        ("f32.le", 0x5f),
        ("f32.ge", 0x60),
        ("f64.eq", 0x61),
        ("f64.ne", 0x62),
        ("f64.lt", 0x63),
        ("f64.gt", 0x64),
        ("f64.le", 0x65),
        ("f64.ge", 0x66),
        ("i32.clz", 0x67),
        ("i32.ctz", 0x68),
        ("i32.popcnt", 0x69),
        ("i32.add", 0x6a),
        ("i32.sub", 0x6b),
        ("i32.mul", 0x6c),
        ("i32.div_s", 0x6d),
        ("i32.div_u", 0x6e),
        ("i32.rem_s", 0x6f),
        ("i32.rem_u", 0x70),
        ("i32.and", 0x71),
        ("i32.or", 0x72),
        ("i32.xor", 0x73),
        ("i32.shl", 0x74),
        ("i32.shr_s", 0x75),
        ("i32.shr_u", 0x76),
        ("i32.rotl", 0x77),
        ("i32.rotr", 0x78),
        ("i64.clz", 0x79),
        ("i64.ctz", 0x7a),
        ("i64.popcnt", 0x7b),
        ("i64.add", 0x7c),
        ("i64.sub", 0x7d),
        ("i64.mul", 0x7e),
        ("i64.div_s", 0x7f),
        ("i64.div_u", 0x80),
        ("i64.rem_s", 0x81),
        ("i64.rem_u", 0x82),
        ("i64.and", 0x83),
        ("i64.or", 0x84),
        ("i64.xor", 0x85),
        ("i64.shl", 0x86),
        ("i64.shr_s", 0x87),
        ("i64.shr_u", 0x88),
        ("i64.rotl", 0x89),
        ("i64.rotr", 0x8a),
        ("f32.abs", 0x8b),
        ("f32.neg", 0x8c),
        ("f32.ceil", 0x8d),
        ("f32.floor", 0x8e),
        ("f32.trunc", 0x8f),
        ("f32.nearest", 0x90),
        ("f32.sqrt", 0x91),
        ("f32.add", 0x92),
        ("f32.sub", 0x93),
        ("f32.mul", 0x94),
        ("f32.div", 0x95),
        ("f32.min", 0x96),
        ("f32.max", 0x97),
        ("f32.copysign", 0x98),
        ("f64.abs", 0x99),
        ("f64.neg", 0x9a),
        ("f64.ceil", 0x9b),
        ("f64.floor", 0x9c),
        ("f64.trunc", 0x9d),
        ("f64.nearest", 0x9e),
        ("f64.sqrt", 0x9f),
        ("f64.add", 0xa0),
        ("f64.sub", 0xa1),
        ("f64.mul", 0xa2),
        ("f64.div", 0xa3),
        ("f64.min", 0xa4),
        ("f64.max", 0xa5),
        ("f64.copysign", 0xa6),
        ("i32.wrap_i64", 0xa7),
        ("i32.trunc_f32_s", 0xa8),
        ("i32.trunc_f32_u", 0xa9),
        ("i32.trunc_f64_s", 0xaa),
        ("i32.trunc_f64_u", 0xab),
        ("i64.extend_i32_s", 0xac),
        ("i64.extend_i32_u", 0xad),
        ("i64.trunc_f32_s", 0xae),
        ("i64.trunc_f32_u", 0xaf),
        ("i64.trunc_f64_s", 0xb0),
        ("i64.trunc_f64_u", 0xb1),
        ("f32.convert_i32_s", 0xb2),
        ("f32.convert_i32_u", 0xb3),
        ("f32.convert_i64_s", 0xb4),
        ("f32.convert_i64_u", 0xb5),
        ("f32.demote_f64", 0xb6),
        ("f64.convert_i32_s", 0xb7),
        ("f64.convert_i32_u", 0xb8),
        ("f64.convert_i64_s", 0xb9),
        ("f64.convert_i64_u", 0xba),
        ("f64.promote_f32", 0xbb),
        ("i32.reinterpret_f32", 0xbc),
        ("i64.reinterpret_f64", 0xbd),
        ("f32.reinterpret_i32", 0xbe),
        ("f64.reinterpret_i64", 0xbf),
        ("i32.extend8_s", 0xc0),
        ("i32.extend16_s", 0xc1),
        ("i64.extend8_s", 0xc2),
        ("i64.extend16_s", 0xc3),
        ("i64.extend32_s", 0xc4),
        ("i32.trunc_sat_f32_s", 0xfc00),
        ("i32.trunc_sat_f32_u", 0xfc01),
        ("i32.trunc_sat_f64_s", 0xfc02),
        ("i32.trunc_sat_f64_u", 0xfc03),
        ("i64.trunc_sat_f32_s", 0xfc04),
        ("i64.trunc_sat_f32_u", 0xfc05),
        ("i64.trunc_sat_f64_s", 0xfc06),
        ("i64.trunc_sat_f64_u", 0xfc07),
        ("memory.copy", 0xfc0a),
        ("memory.fill", 0xfc0b),
    ];
    TABELLE.iter().find(|(t, _)| *t == name).map(|(_, c)| *c)
}

/// Befehl als WAT-Textzeile (W5: Disassembly der Entwicklerwerkzeuge).
pub fn text(b: &Befehl) -> String {
    use Befehl::*;
    match b {
        Unreachable => "unreachable".into(),
        Nop => "nop".into(),
        Block(bt) => format!("block{}", bt.text(&[])),
        Loop(bt) => format!("loop{}", bt.text(&[])),
        If(bt) => format!("if{}", bt.text(&[])),
        Else => "else".into(),
        End => "end".into(),
        Br(l) => format!("br {l}"),
        BrIf(l) => format!("br_if {l}"),
        BrTable(z, v) => {
            let mut s = String::from("br_table");
            for x in z {
                s.push_str(&format!(" {x}"));
            }
            s.push_str(&format!(" {v}"));
            s
        }
        Return => "return".into(),
        Call(i) => format!("call {i}"),
        CallIndirect { typ, tabelle } => format!("call_indirect (type {typ}) (table {tabelle})"),
        RefI31 => "ref.i31".into(),
        BrOnCast(l, a, b) => format!("br_on_cast {l} {} {}", a.text(), b.text()),
        BrOnCastFail(l, a, b) => format!("br_on_cast_fail {l} {} {}", a.text(), b.text()),
        StructNew(t) => format!("struct.new {t}"),
        StructGet(t, f) => format!("struct.get {t} {f}"),
        StructGetS(t, f) => format!("struct.get_s {t} {f}"),
        StructGetU(t, f) => format!("struct.get_u {t} {f}"),
        StructSet(t, f) => format!("struct.set {t} {f}"),
        ArrayNew(t) => format!("array.new {t}"),
        ArrayLen => "array.len".into(),
        ArrayGet(t) => format!("array.get {t}"),
        ArrayGetS(t) => format!("array.get_s {t}"),
        ArrayGetU(t) => format!("array.get_u {t}"),
        ArraySet(t) => format!("array.set {t}"),
        ArrayNewFixed(t, n) => format!("array.new_fixed {t} {n}"),
        ArrayNewData(t, d) => format!("array.new_data {t} {d}"),
        ArrayNewElem(t, e) => format!("array.new_elem {t} {e}"),
        ArrayInitData(t, d) => format!("array.init_data {t} {d}"),
        ArrayInitElem(t, e) => format!("array.init_elem {t} {e}"),
        ArrayFill(t) => format!("array.fill {t}"),
        ArrayCopy(d, s) => format!("array.copy {d} {s}"),
        RefTest(r) => format!("ref.test {}", r.text()),
        RefCast(r) => format!("ref.cast {}", r.text()),
        I31GetS => "i31.get_s".into(),
        I31GetU => "i31.get_u".into(),
        StructNewDefault(t) => format!("struct.new_default {t}"),
        AnyConvertExtern => "any.convert_extern".into(),
        ExternConvertAny => "extern.convert_any".into(),
        RefNull(r) => format!("ref.null {}", r.text()),
        RefIsNull => "ref.is_null".into(),
        ReturnCall(f) => format!("return_call {f}"),
        ReturnCallIndirect { typ, tabelle } => {
            format!("return_call_indirect {typ} (Tabelle {tabelle})")
        }
        ReturnCallRef(t) => format!("return_call_ref {t}"),
        RefAsNonNull => "ref.as_non_null".into(),
        CallRef(t) => format!("call_ref {t}"),
        BrOnNull(l) => format!("br_on_null {l}"),
        BrOnNonNull(l) => format!("br_on_non_null {l}"),
        RefFunc(f) => format!("ref.func {f}"),
        Drop => "drop".into(),
        Select => "select".into(),
        SelectTyp(ts) => {
            let t: Vec<&str> = ts.iter().map(|v| v.text()).collect();
            format!("select (result {})", t.join(" "))
        }
        LocalGet(i) => format!("local.get {i}"),
        LocalSet(i) => format!("local.set {i}"),
        LocalTee(i) => format!("local.tee {i}"),
        GlobalGet(i) => format!("global.get {i}"),
        GlobalSet(i) => format!("global.set {i}"),
        TableGet(i) => format!("table.get {i}"),
        TableSet(i) => format!("table.set {i}"),
        MitSpeicher { befehl, speicher } => format!("{}$m{}", text(befehl.as_ref()), speicher),
        MemorySize => "memory.size".into(),
        MemoryGrow => "memory.grow".into(),
        MemoryInit(d) => format!("memory.init {d}"),
        DataDrop(d) => format!("data.drop {d}"),
        MemoryCopy { ziel, quelle } => {
            if *ziel == 0 && *quelle == 0 {
                "memory.copy".into()
            } else {
                format!("memory.copy {ziel} {quelle}")
            }
        }
        MemoryFill => "memory.fill".into(),
        TableInit { element, tabelle } => format!("table.init {element} {tabelle}"),
        ElemDrop(e) => format!("elem.drop {e}"),
        TableCopy { ziel, quelle } => format!("table.copy {ziel} {quelle}"),
        TableGrow(i) => format!("table.grow {i}"),
        TableSize(i) => format!("table.size {i}"),
        TableFill(i) => format!("table.fill {i}"),
        I32Konst(v) => format!("i32.const {v}"),
        I64Konst(v) => format!("i64.const {v}"),
        F32Konst(bits) => format!(
            "f32.const {} (0x{bits:08x})",
            crate::wert::als_f32_text(*bits)
        ),
        F64Konst(bits) => format!(
            "f64.const {} (0x{bits:016x})",
            crate::wert::als_f64_text(*bits)
        ),
        I32Eqz => "i32.eqz".into(),
        I32Eq => "i32.eq".into(),
        I32Ne => "i32.ne".into(),
        I32LtS => "i32.lt_s".into(),
        I32LtU => "i32.lt_u".into(),
        I32GtS => "i32.gt_s".into(),
        I32GtU => "i32.gt_u".into(),
        I32LeS => "i32.le_s".into(),
        I32LeU => "i32.le_u".into(),
        I32GeS => "i32.ge_s".into(),
        I32GeU => "i32.ge_u".into(),
        I32Clz => "i32.clz".into(),
        I32Ctz => "i32.ctz".into(),
        I32Popcnt => "i32.popcnt".into(),
        I32Add => "i32.add".into(),
        I32Sub => "i32.sub".into(),
        I32Mul => "i32.mul".into(),
        I32DivS => "i32.div_s".into(),
        I32DivU => "i32.div_u".into(),
        I32RemS => "i32.rem_s".into(),
        I32RemU => "i32.rem_u".into(),
        I32And => "i32.and".into(),
        I32Or => "i32.or".into(),
        I32Xor => "i32.xor".into(),
        I32Shl => "i32.shl".into(),
        I32ShrS => "i32.shr_s".into(),
        I32ShrU => "i32.shr_u".into(),
        I32Rotl => "i32.rotl".into(),
        I32Rotr => "i32.rotr".into(),
        I64Eqz => "i64.eqz".into(),
        I64Eq => "i64.eq".into(),
        I64Ne => "i64.ne".into(),
        I64LtS => "i64.lt_s".into(),
        I64LtU => "i64.lt_u".into(),
        I64GtS => "i64.gt_s".into(),
        I64GtU => "i64.gt_u".into(),
        I64LeS => "i64.le_s".into(),
        I64LeU => "i64.le_u".into(),
        I64GeS => "i64.ge_s".into(),
        I64GeU => "i64.ge_u".into(),
        I64Clz => "i64.clz".into(),
        I64Ctz => "i64.ctz".into(),
        I64Popcnt => "i64.popcnt".into(),
        I64Add => "i64.add".into(),
        I64Sub => "i64.sub".into(),
        I64Mul => "i64.mul".into(),
        I64DivS => "i64.div_s".into(),
        I64DivU => "i64.div_u".into(),
        I64RemS => "i64.rem_s".into(),
        I64RemU => "i64.rem_u".into(),
        I64And => "i64.and".into(),
        I64Or => "i64.or".into(),
        I64Xor => "i64.xor".into(),
        I64Shl => "i64.shl".into(),
        I64ShrS => "i64.shr_s".into(),
        I64ShrU => "i64.shr_u".into(),
        I64Rotl => "i64.rotl".into(),
        I64Rotr => "i64.rotr".into(),
        F32Eq => "f32.eq".into(),
        F32Ne => "f32.ne".into(),
        F32Lt => "f32.lt".into(),
        F32Gt => "f32.gt".into(),
        F32Le => "f32.le".into(),
        F32Ge => "f32.ge".into(),
        F32Abs => "f32.abs".into(),
        F32Neg => "f32.neg".into(),
        F32Ceil => "f32.ceil".into(),
        F32Floor => "f32.floor".into(),
        F32Trunc => "f32.trunc".into(),
        F32Nearest => "f32.nearest".into(),
        F32Sqrt => "f32.sqrt".into(),
        F32Add => "f32.add".into(),
        F32Sub => "f32.sub".into(),
        F32Mul => "f32.mul".into(),
        F32Div => "f32.div".into(),
        F32Min => "f32.min".into(),
        F32Max => "f32.max".into(),
        F32Copysign => "f32.copysign".into(),
        F64Eq => "f64.eq".into(),
        F64Ne => "f64.ne".into(),
        F64Lt => "f64.lt".into(),
        F64Gt => "f64.gt".into(),
        F64Le => "f64.le".into(),
        F64Ge => "f64.ge".into(),
        F64Abs => "f64.abs".into(),
        F64Neg => "f64.neg".into(),
        F64Ceil => "f64.ceil".into(),
        F64Floor => "f64.floor".into(),
        F64Trunc => "f64.trunc".into(),
        F64Nearest => "f64.nearest".into(),
        F64Sqrt => "f64.sqrt".into(),
        F64Add => "f64.add".into(),
        F64Sub => "f64.sub".into(),
        F64Mul => "f64.mul".into(),
        F64Div => "f64.div".into(),
        F64Min => "f64.min".into(),
        F64Max => "f64.max".into(),
        F64Copysign => "f64.copysign".into(),
        I32WrapI64 => "i32.wrap_i64".into(),
        I32TruncF32S => "i32.trunc_f32_s".into(),
        I32TruncF32U => "i32.trunc_f32_u".into(),
        I32TruncF64S => "i32.trunc_f64_s".into(),
        I32TruncF64U => "i32.trunc_f64_u".into(),
        I64ExtendI32S => "i64.extend_i32_s".into(),
        I64ExtendI32U => "i64.extend_i32_u".into(),
        I64TruncF32S => "i64.trunc_f32_s".into(),
        I64TruncF32U => "i64.trunc_f32_u".into(),
        I64TruncF64S => "i64.trunc_f64_s".into(),
        I64TruncF64U => "i64.trunc_f64_u".into(),
        F32ConvertI32S => "f32.convert_i32_s".into(),
        F32ConvertI32U => "f32.convert_i32_u".into(),
        F32ConvertI64S => "f32.convert_i64_s".into(),
        F32ConvertI64U => "f32.convert_i64_u".into(),
        F32DemoteF64 => "f32.demote_f64".into(),
        F64ConvertI32S => "f64.convert_i32_s".into(),
        F64ConvertI32U => "f64.convert_i32_u".into(),
        F64ConvertI64S => "f64.convert_i64_s".into(),
        F64ConvertI64U => "f64.convert_i64_u".into(),
        F64PromoteF32 => "f64.promote_f32".into(),
        I32ReinterpretF32 => "i32.reinterpret_f32".into(),
        I64ReinterpretF64 => "i64.reinterpret_f64".into(),
        F32ReinterpretI32 => "f32.reinterpret_i32".into(),
        F64ReinterpretI64 => "f64.reinterpret_i64".into(),
        I32Extend8S => "i32.extend8_s".into(),
        I32Extend16S => "i32.extend16_s".into(),
        I64Extend8S => "i64.extend8_s".into(),
        I64Extend16S => "i64.extend16_s".into(),
        I64Extend32S => "i64.extend32_s".into(),
        I32TruncSatF32S => "i32.trunc_sat_f32_s".into(),
        I32TruncSatF32U => "i32.trunc_sat_f32_u".into(),
        I32TruncSatF64S => "i32.trunc_sat_f64_s".into(),
        I32TruncSatF64U => "i32.trunc_sat_f64_u".into(),
        I64TruncSatF32S => "i64.trunc_sat_f32_s".into(),
        I64TruncSatF32U => "i64.trunc_sat_f32_u".into(),
        I64TruncSatF64S => "i64.trunc_sat_f64_s".into(),
        I64TruncSatF64U => "i64.trunc_sat_f64_u".into(),
        // Speicherbefehle: Offset nur nennen, wenn ungleich 0.
        _ => speicher_text(b),
    }
}

/// Speicherbefehle als Text (Offset nur bei ungleich 0).
fn speicher_text(b: &Befehl) -> String {
    use Befehl::*;
    let (name, versatz) = match b {
        I32Load { versatz } => ("i32.load", *versatz),
        I64Load { versatz } => ("i64.load", *versatz),
        F32Load { versatz } => ("f32.load", *versatz),
        F64Load { versatz } => ("f64.load", *versatz),
        I32Load8S { versatz } => ("i32.load8_s", *versatz),
        I32Load8U { versatz } => ("i32.load8_u", *versatz),
        I32Load16S { versatz } => ("i32.load16_s", *versatz),
        I32Load16U { versatz } => ("i32.load16_u", *versatz),
        I64Load8S { versatz } => ("i64.load8_s", *versatz),
        I64Load8U { versatz } => ("i64.load8_u", *versatz),
        I64Load16S { versatz } => ("i64.load16_s", *versatz),
        I64Load16U { versatz } => ("i64.load16_u", *versatz),
        I64Load32S { versatz } => ("i64.load32_s", *versatz),
        I64Load32U { versatz } => ("i64.load32_u", *versatz),
        I32Store { versatz } => ("i32.store", *versatz),
        I64Store { versatz } => ("i64.store", *versatz),
        F32Store { versatz } => ("f32.store", *versatz),
        F64Store { versatz } => ("f64.store", *versatz),
        I32Store8 { versatz } => ("i32.store8", *versatz),
        I32Store16 { versatz } => ("i32.store16", *versatz),
        I64Store8 { versatz } => ("i64.store8", *versatz),
        I64Store16 { versatz } => ("i64.store16", *versatz),
        I64Store32 { versatz } => ("i64.store32", *versatz),
        _ => return format!("{b:?}"),
    };
    if versatz == 0 {
        name.to_string()
    } else {
        format!("{name} offset={versatz}")
    }
}

/// Befehl aus seinem Textnamen; unbekannte Namen ergeben einen BENANNTEN
/// Formfehler (kein Panic — `team/REGELN.txt`).
pub fn aus_text(name: &str) -> Result<Befehl, Fehler> {
    use Befehl::*;
    let code = befehl_aus_text(name)
        .ok_or_else(|| Fehler::form(ART_OPCODE, format!("WAT: Befehl {name:?} unbekannt")))?;
    if code < 0x100 {
        return Befehl::aus_code(code as u8);
    }
    Ok(match code {
        0xfc00 => I32TruncSatF32S,
        0xfc01 => I32TruncSatF32U,
        0xfc02 => I32TruncSatF64S,
        0xfc03 => I32TruncSatF64U,
        0xfc04 => I64TruncSatF32S,
        0xfc05 => I64TruncSatF32U,
        0xfc06 => I64TruncSatF64S,
        0xfc07 => I64TruncSatF64U,
        // Ohne Kontext (nur der Name) sind die Speicherindizes 0.
        0xfc0a => MemoryCopy { ziel: 0, quelle: 0 },
        _ => MemoryFill,
    })
}

/// Lade-/Speicherbefehl aus Opcode und Versatz (memarg ist schon gelesen).
/// Lade-/Speicherbefehl aus Opcode und Versatz (memarg ist schon gelesen).
fn speicherbefehl(code: u8, versatz: u32) -> Befehl {
    use Befehl::*;
    match code {
        0x28 => I32Load { versatz },
        0x29 => I64Load { versatz },
        0x2a => F32Load { versatz },
        0x2b => F64Load { versatz },
        0x2c => I32Load8S { versatz },
        0x2d => I32Load8U { versatz },
        0x2e => I32Load16S { versatz },
        0x2f => I32Load16U { versatz },
        0x30 => I64Load8S { versatz },
        0x31 => I64Load8U { versatz },
        0x32 => I64Load16S { versatz },
        0x33 => I64Load16U { versatz },
        0x34 => I64Load32S { versatz },
        0x35 => I64Load32U { versatz },
        0x36 => I32Store { versatz },
        0x37 => I64Store { versatz },
        0x38 => F32Store { versatz },
        0x39 => F64Store { versatz },
        0x3a => I32Store8 { versatz },
        0x3b => I32Store16 { versatz },
        0x3c => I64Store8 { versatz },
        0x3d => I64Store16 { versatz },
        0x3e => I64Store32 { versatz },
        _ => Unreachable, // vom Decoder nie erreicht
    }
}

/// Natuerliche Ausrichtung (in Bit) je Lade-/Speicherbefehl — der Validator
/// vergleicht `align` damit (Spec: "alignment must not be larger than natural").
pub fn natuerliche_ausrichtung(b: &Befehl) -> u32 {
    use Befehl::*;
    match b {
        I32Load { .. } | F32Load { .. } | I32Store { .. } | F32Store { .. } => 2,
        I64Load { .. } | F64Load { .. } | I64Store { .. } | F64Store { .. } => 3,
        I32Load8S { .. }
        | I32Load8U { .. }
        | I64Load8S { .. }
        | I64Load8U { .. }
        | I32Store8 { .. }
        | I64Store8 { .. } => 0,
        I32Load16S { .. }
        | I32Load16U { .. }
        | I64Load16S { .. }
        | I64Load16U { .. }
        | I32Store16 { .. }
        | I64Store16 { .. } => 1,
        I64Load32S { .. } | I64Load32U { .. } | I64Store32 { .. } => 2,
        _ => 0,
    }
}

/// Liest einen Opcode mit allen Immediates. `tiefe` ist die aktuelle
/// Blockverschachtelung (M5).
pub fn lies_befehl(l: &mut Leser, tiefe: usize) -> Result<Befehl, Fehler> {
    use Befehl::*;
    if tiefe > MAX_VERSCHACHTELUNG_HART {
        return Err(Fehler::form(
            crate::fehler::ART_GRENZE,
            format!("Blockverschachtelung {tiefe} ueber der Grenze {MAX_VERSCHACHTELUNG_HART}"),
        ));
    }
    let code = l.u8("Opcode")?;
    Ok(match code {
        0x00 => Unreachable,
        0x01 => Nop,
        0x02 => Block(lies_blockart(l)?),
        0x03 => Loop(lies_blockart(l)?),
        0x04 => If(lies_blockart(l)?),
        0x05 => Else,
        0x0b => End,
        0x0c => Br(l.leb_u32(true)?),
        0x0d => BrIf(l.leb_u32(true)?),
        0x0e => {
            let n = l.vektor_len(1 << 20, "br_table")?;
            // M4: Vec::with_capacity erst NACH der Pruefung (vektor_len hat
            // geprueft, dass hoechstens `uebrig()` Bytes da sind).
            let mut ziele = Vec::with_capacity(n.min(1024));
            for _ in 0..n {
                ziele.push(l.leb_u32(true)?);
            }
            BrTable(ziele, l.leb_u32(true)?)
        }
        0x0f => Return,
        0x10 => Call(l.leb_u32(true)?),
        0x11 => {
            let typ = l.leb_u32(true)?;
            let tabelle = l.leb_u32(true)?;
            CallIndirect { typ, tabelle }
        }
        0x1a => Drop,
        0x1b => Select,
        0x1c => {
            let n = l.vektor_len(256, "select")?;
            let mut t = Vec::with_capacity(n.min(1024));
            for _ in 0..n {
                t.push(valtyp_lesen(l.u8("select-Typ")?)?);
            }
            if t.len() != 1 {
                return Err(Fehler::form(
                    crate::fehler::V_SELECT,
                    format!("select mit {} Typen (genau 1 erlaubt)", t.len()),
                ));
            }
            SelectTyp(t)
        }
        0x20 => LocalGet(l.leb_u32(true)?),
        0x21 => LocalSet(l.leb_u32(true)?),
        0x22 => LocalTee(l.leb_u32(true)?),
        0x23 => GlobalGet(l.leb_u32(true)?),
        0x24 => GlobalSet(l.leb_u32(true)?),
        0x25 => TableGet(l.leb_u32(true)?),
        0x26 => TableSet(l.leb_u32(true)?),
        0x28..=0x3e => {
            // memarg = align (u32) + versatz (u32). Bit 6 des align-Feldes
            // waere Multi-Memory (Wasm 3.0) - das lehnen wir ab.
            let align = l.leb_u32(true)?;
            if (align & 0x40) != 0 {
                return Err(Fehler::form(
                    ART_OPCODE,
                    "memarg mit Speicherindex (Multi-Memory) wird nicht unterstuetzt",
                ));
            }
            let versatz = l.leb_u32(true)?;
            let b = speicherbefehl(code, versatz);
            // Spec: „alignment must not be larger than natural" — `align` ist
            // der Exponent (2^align Byte). GEMESSEN 01.10.: `align.wast`
            // hatte 51 `assert_invalid`-Faelle, die ohne diese Pruefung
            // durchkamen.
            let nat = natuerliche_ausrichtung(&b);
            if align > nat {
                return Err(Fehler::form(
                    ART_OPCODE,
                    format!("Ausrichtung 2^{align} groesser als natuerlich 2^{nat}"),
                ));
            }
            b
        }
        0x3f => {
            pruefe_nullbyte(l)?;
            MemorySize
        }
        0x40 => {
            pruefe_nullbyte(l)?;
            MemoryGrow
        }
        0x41 => I32Konst(l.leb_s32(true)?),
        0x42 => I64Konst(l.leb_s64(true)?),
        0x43 => F32Konst(l.u32le("f32.const")?),
        0x44 => F64Konst(u64::from_le_bytes({
            let b = l.nimm(8, "f64.const")?;
            [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]
        })),
        0xd0 => {
            let b = l.u8("ref.null")?;
            RefNull(
                Reftyp::von_byte(b)
                    .ok_or_else(|| crate::modell::reftyp_ungueltig(format!("{b:#04x}")))?,
            )
        }
        0xd1 => RefIsNull,
        0xd2 => RefFunc(l.leb_u32(true)?),
        0x12 => ReturnCall(l.leb_u32(true)?),
        0x13 => {
            let typ = l.leb_u32(true)?;
            let tabelle = l.leb_u32(true)?;
            ReturnCallIndirect { typ, tabelle }
        }
        // GEMESSEN 03.10. gegen node 22: `ref.eq` ist **0xd3**,
        // `ref.as_non_null` **0xd4**. Vorher stand hier
        // `0xd3 => RefAsNonNull`, und `ref.eq` lag im 0xfb-Block auf
        // 0x1d — das ist `i31.get_s`.
        0xd3 => RefEq,
        0xd4 => RefAsNonNull,
        0x14 => CallRef(l.leb_u32(true)?),
        0x15 => ReturnCallRef(l.leb_u32(true)?),
        0xd5 => BrOnNull(l.leb_u32(true)?),
        0xfc => {
            let n = l.leb_u32(true)?;
            match n {
                0..=7 => {
                    use Befehl::*;
                    match n {
                        0 => I32TruncSatF32S,
                        1 => I32TruncSatF32U,
                        2 => I32TruncSatF64S,
                        3 => I32TruncSatF64U,
                        4 => I64TruncSatF32S,
                        5 => I64TruncSatF32U,
                        6 => I64TruncSatF64S,
                        _ => I64TruncSatF64U,
                    }
                }
                8 => {
                    let d = l.leb_u32(true)?;
                    pruefe_nullbyte(l)?;
                    MemoryInit(d)
                }
                9 => DataDrop(l.leb_u32(true)?),
                10 => {
                    // Zwei Speicherindizes (Multi-Memory).
                    let ziel = l.leb_u32(true)?;
                    let quelle = l.leb_u32(true)?;
                    MemoryCopy { ziel, quelle }
                }
                11 => {
                    pruefe_nullbyte(l)?;
                    MemoryFill
                }
                12 => {
                    let e = l.leb_u32(true)?;
                    let t = l.leb_u32(true)?;
                    TableInit {
                        element: e,
                        tabelle: t,
                    }
                }
                13 => ElemDrop(l.leb_u32(true)?),
                14 => {
                    let z = l.leb_u32(true)?;
                    let q = l.leb_u32(true)?;
                    TableCopy { ziel: z, quelle: q }
                }
                15 => TableGrow(l.leb_u32(true)?),
                16 => TableSize(l.leb_u32(true)?),
                17 => TableFill(l.leb_u32(true)?),
                _ => {
                    return Err(Fehler::form(
                        ART_OPCODE_PRAEFIX,
                        format!("0xFC-Befehl {n} unbekannt (SIMD ist 0xFD, Threads 0xFE)"),
                    ));
                }
            }
        }
        0xfd => {
            return Err(Fehler::form(
                ART_OPCODE_PRAEFIX,
                "SIMD (0xFD) wird nicht unterstuetzt (offener Punkt, siehe WUNSCH_WASM.txt)",
            ));
        }
        0x1f => {
            // try_table: Blocktyp, dann die catch-Liste (Bytes 0x00 catch,
            // 0x01 catch_ref, 0x02 catch_all, 0x03 catch_all_ref; jeweils
            // mit Tagindex und Labelindex).
            let bt = lies_blockart(l)?;
            let n = l.vektor_len(1 << 16, "catch-Liste")?;
            let mut faenge = Vec::with_capacity(n.min(1024));
            for _ in 0..n {
                let art = l.u8("catch-Art")?;
                let tag = l.leb_u32(true)?;
                let label = l.leb_u32(true)?;
                match art {
                    0x00 => faenge.push(Fang {
                        tag: Some(tag),
                        label: Some(label),
                    }),
                    0x02 => faenge.push(Fang {
                        tag: None,
                        label: Some(label),
                    }),
                    0x01 | 0x03 => {
                        return Err(Fehler::form(
                            ART_OPCODE,
                            format!("catch_ref (Art {art:#04x}) wird nicht unterstuetzt"),
                        ));
                    }
                    _ => {
                        return Err(Fehler::form(
                            ART_OPCODE,
                            format!("catch-Art {art:#04x} unbekannt"),
                        ));
                    }
                }
            }
            TryTable(bt, faenge)
        }
        0x08 => Throw(l.leb_u32(true)?),
        0xfb => {
            // GC-Praefix (Wasm 3.0). GEMESSEN 03.10. (`table_init.wast`
            // Z. 790: 1 Fall).
            let n = l.u8("GC-Befehl")?;
            // GEMESSEN 03.10. gegen node 22 (jede Zeile einzeln geprueft,
            // /tmp/opencode/opcodes*.py): die Tabelle stand vorher aus
            // einem aelteren Proposal-Entwurf und war an zehn Stellen
            // falsch (`array.new` auf 0x00 usw.). Der Kommentar unten ist
            // die Messung, nicht die Erinnerung.
            match n {
                0x00 => StructNew(l.leb_u32(true)?),
                0x01 => StructNewDefault(l.leb_u32(true)?),
                0x02 => StructGet(l.leb_u32(true)?, l.leb_u32(true)?),
                0x03 => StructGetS(l.leb_u32(true)?, l.leb_u32(true)?),
                0x04 => StructGetU(l.leb_u32(true)?, l.leb_u32(true)?),
                0x05 => StructSet(l.leb_u32(true)?, l.leb_u32(true)?),
                0x06 => ArrayNew(l.leb_u32(true)?),
                0x07 => ArrayNewDefault(l.leb_u32(true)?),
                0x0b => ArrayGet(l.leb_u32(true)?),
                0x0c => ArrayGetS(l.leb_u32(true)?),
                0x0d => ArrayGetU(l.leb_u32(true)?),
                0x0e => ArraySet(l.leb_u32(true)?),
                0x0f => ArrayLen,
                // GEMESSEN 03.10. gegen node 22: 0x10 `array.fill`,
                // 0x11 `array.copy` (siehe die Tabelle im 0xfb-Arm).
                0x08 => ArrayNewFixed(l.leb_u32(true)?, l.leb_u32(true)?),
                0x09 => ArrayNewData(l.leb_u32(true)?, l.leb_u32(true)?),
                0x0a => ArrayNewElem(l.leb_u32(true)?, l.leb_u32(true)?),
                0x12 => ArrayInitData(l.leb_u32(true)?, l.leb_u32(true)?),
                0x13 => ArrayInitElem(l.leb_u32(true)?, l.leb_u32(true)?),
                0x10 => ArrayFill(l.leb_u32(true)?),
                0x11 => ArrayCopy(l.leb_u32(true)?, l.leb_u32(true)?),
                // GEMESSEN 03.10. an node 22: 0x16 `br_on_cast`,
                // 0x17 `br_on_cast_fail` (die 0x18/0x19 sind die
                // Cast-Flags-Varianten).
                0x16 => BrOnCast(
                    l.leb_u32(true)?,
                    binaer_heaptyp(l.u8("br_on_cast")?)?,
                    binaer_heaptyp(l.u8("br_on_cast")?)?,
                ),
                0x17 => BrOnCastFail(
                    l.leb_u32(true)?,
                    binaer_heaptyp(l.u8("br_on_cast_fail")?)?,
                    binaer_heaptyp(l.u8("br_on_cast_fail")?)?,
                ),
                // `br_on_non_null` hat hier **keinen** Eintrag, und das ist
                // gemessen: node 22 belegt im 0xfb-Raum nur `0x00`..`0x1e`
                // (ab `0x1f`: „invalid gc opcode"), und im einbyteigen Raum
                // ist `0xd7`/��`0xd8` „Invalid opcode". Die Nummer ist also
                // mit dem verfuegbaren Orakel **nicht** bestimmbar; eine
                // geratene wuerde Binaermodule still falsch lesen. Die
                // Textform traegt die Suite-Faelle (`br_on_non_null.wast`
                // 1/9 -> 7/9).
                0x14 => RefTest(binaer_heaptyp(l.u8("ref.test")?)?),
                0x15 => RefCast(binaer_heaptyp(l.u8("ref.cast")?)?),
                // 0x16/0x17 `br_on_cast`/`br_on_cast_fail` (ohne Flags),
                // 0x18/0x19 dieselben mit Cast-Flags, 0x08 `array.new_fixed`,
                // 0x09 `array.new_data`, 0x0a `array.new_elem`,
                // 0x10 `array.fill`, 0x11 `array.copy`, 0x12 `array.init_data`,
                // 0x13 `array.init_elem` — noch nicht gebaut.
                0x1a => AnyConvertExtern,
                0x1b => ExternConvertAny,
                0x1c => RefI31,
                0x1d => I31GetS,
                0x1e => I31GetU,
                _ => {
                    return Err(Fehler::form(
                        ART_OPCODE_PRAEFIX,
                        format!("GC-Befehl 0xfb {n:#04x} wird nicht unterstuetzt"),
                    ));
                }
            }
        }
        0x09 | 0x06 | 0x07 | 0x18 | 0x19 => {
            return Err(Fehler::form(
                ART_OPCODE,
                format!(
                    "Exception-Handling-Befehl {code:#04x} (try/catch/delegate/rethrow) wird nicht unterstuetzt"
                ),
            ));
        }
        // `0x12` ist `return_call` (Wasm 3.0 Tail Calls) und wird oben
        // gelesen — GEMESSEN 03.10. (vorher stand es hier und machte den
        // Befehl unlesbar).
        0xfe => {
            return Err(Fehler::form(
                ART_OPCODE_PRAEFIX,
                format!("Atombehle/Threads/Erweiterungen ({code:#04x}) werden nicht unterstuetzt"),
            ));
        }
        _ => Befehl::aus_code(code)?,
    })
}

/// Blocktyp lesen: `0x40` leer, sonst Werttyp-Byte (das ist zugleich ein
/// gueltiges s33 mit Vorzeichenbit -> negativ) oder Typindex (s33 >= 0).
pub fn lies_blockart(l: &mut Leser) -> Result<Blockart, Fehler> {
    let b = l
        .sieht()
        .ok_or_else(|| Fehler::form(ART_UNVOLLSTAENDIG, "Blocktyp fehlt"))?;
    if b == 0x40 {
        l.u8("Blocktyp")?;
        return Ok(Blockart::Leer);
    }
    if let Some(v) = Valtyp::von_byte(b) {
        l.u8("Blocktyp")?;
        return Ok(Blockart::Werttyp(v));
    }
    let s = l.leb_s33(true)?;
    if s < 0 || s > u32::MAX as i64 {
        return Err(blocktyp_lesefehler(s));
    }
    Ok(Blockart::Typ(s as u32))
}

/// `memory.size`/`grow`/`fill`/`copy` verlangen ein 0x00-Reservierungsbyte.
/// Ein Heaptyp aus dem Binaerformat (das Immediate von `ref.test` und
/// `ref.cast`).
///
/// GEMESSEN 03.10. (`ref_test.wast`, `ref_cast.wast`).
fn binaer_heaptyp(b: u8) -> Result<Reftyp, Fehler> {
    Reftyp::von_byte(b).ok_or_else(|| {
        Fehler::form(
            crate::fehler::ART_REFTYP,
            format!("Heaptyp {b:#04x} unbekannt"),
        )
    })
}

fn pruefe_nullbyte(l: &mut Leser) -> Result<(), Fehler> {
    let b = l.u8("Reservierungsbyte")?;
    if b != 0x00 {
        return Err(Fehler::form(
            ART_OPCODE,
            format!("Reservierungsbyte {b:#04x}, muss 0x00 sein (Multi-Memory)"),
        ));
    }
    Ok(())
}

/// Befehle ohne Immediates nach Opcode-Nummer (Rest der 0x45..0xC4).
impl Befehl {
    pub fn aus_code(code: u8) -> Result<Befehl, Fehler> {
        use Befehl::*;
        Ok(match code {
            0x45 => I32Eqz,
            0x46 => I32Eq,
            0x47 => I32Ne,
            0x48 => I32LtS,
            0x49 => I32LtU,
            0x4a => I32GtS,
            0x4b => I32GtU,
            0x4c => I32LeS,
            0x4d => I32LeU,
            0x4e => I32GeS,
            0x4f => I32GeU,
            0x50 => I64Eqz,
            0x51 => I64Eq,
            0x52 => I64Ne,
            0x53 => I64LtS,
            0x54 => I64LtU,
            0x55 => I64GtS,
            0x56 => I64GtU,
            0x57 => I64LeS,
            0x58 => I64LeU,
            0x59 => I64GeS,
            0x5a => I64GeU,
            0x5b => F32Eq,
            0x5c => F32Ne,
            0x5d => F32Lt,
            0x5e => F32Gt,
            0x5f => F32Le,
            0x60 => F32Ge,
            0x61 => F64Eq,
            0x62 => F64Ne,
            0x63 => F64Lt,
            0x64 => F64Gt,
            0x65 => F64Le,
            0x66 => F64Ge,
            0x67 => I32Clz,
            0x68 => I32Ctz,
            0x69 => I32Popcnt,
            0x6a => I32Add,
            0x6b => I32Sub,
            0x6c => I32Mul,
            0x6d => I32DivS,
            0x6e => I32DivU,
            0x6f => I32RemS,
            0x70 => I32RemU,
            0x71 => I32And,
            0x72 => I32Or,
            0x73 => I32Xor,
            0x74 => I32Shl,
            0x75 => I32ShrS,
            0x76 => I32ShrU,
            0x77 => I32Rotl,
            0x78 => I32Rotr,
            0x79 => I64Clz,
            0x7a => I64Ctz,
            0x7b => I64Popcnt,
            0x7c => I64Add,
            0x7d => I64Sub,
            0x7e => I64Mul,
            0x7f => I64DivS,
            0x80 => I64DivU,
            0x81 => I64RemS,
            0x82 => I64RemU,
            0x83 => I64And,
            0x84 => I64Or,
            0x85 => I64Xor,
            0x86 => I64Shl,
            0x87 => I64ShrS,
            0x88 => I64ShrU,
            0x89 => I64Rotl,
            0x8a => I64Rotr,
            0x8b => F32Abs,
            0x8c => F32Neg,
            0x8d => F32Ceil,
            0x8e => F32Floor,
            0x8f => F32Trunc,
            0x90 => F32Nearest,
            0x91 => F32Sqrt,
            0x92 => F32Add,
            0x93 => F32Sub,
            0x94 => F32Mul,
            0x95 => F32Div,
            0x96 => F32Min,
            0x97 => F32Max,
            0x98 => F32Copysign,
            0x99 => F64Abs,
            0x9a => F64Neg,
            0x9b => F64Ceil,
            0x9c => F64Floor,
            0x9d => F64Trunc,
            0x9e => F64Nearest,
            0x9f => F64Sqrt,
            0xa0 => F64Add,
            0xa1 => F64Sub,
            0xa2 => F64Mul,
            0xa3 => F64Div,
            0xa4 => F64Min,
            0xa5 => F64Max,
            0xa6 => F64Copysign,
            0xa7 => I32WrapI64,
            0xa8 => I32TruncF32S,
            0xa9 => I32TruncF32U,
            0xaa => I32TruncF64S,
            0xab => I32TruncF64U,
            0xac => I64ExtendI32S,
            0xad => I64ExtendI32U,
            0xae => I64TruncF32S,
            0xaf => I64TruncF32U,
            0xb0 => I64TruncF64S,
            0xb1 => I64TruncF64U,
            0xb2 => F32ConvertI32S,
            0xb3 => F32ConvertI32U,
            0xb4 => F32ConvertI64S,
            0xb5 => F32ConvertI64U,
            0xb6 => F32DemoteF64,
            0xb7 => F64ConvertI32S,
            0xb8 => F64ConvertI32U,
            0xb9 => F64ConvertI64S,
            0xba => F64ConvertI64U,
            0xbb => F64PromoteF32,
            0xbc => I32ReinterpretF32,
            0xbd => I64ReinterpretF64,
            0xbe => F32ReinterpretI32,
            0xbf => F64ReinterpretI64,
            0xc0 => I32Extend8S,
            0xc1 => I32Extend16S,
            0xc2 => I64Extend8S,
            0xc3 => I64Extend16S,
            0xc4 => I64Extend32S,
            _ => {
                return Err(Fehler::form(
                    ART_OPCODE,
                    format!(
                        "Opcode {code:#04x} unbekannt (Wasm 2.0 Core ohne SIMD/Threads/Tail-Call)"
                    ),
                ));
            }
        })
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    fn l(d: &[u8]) -> Leser<'_> {
        Leser::neu(d)
    }

    /// M7: Opcode-Nummern sichern (Binarform ist Vertrag).
    #[test]
    fn opcodes_stabil() {
        assert_eq!(Befehl::I32Add.code(), 0x6a);
        assert_eq!(Befehl::MemoryFill.code(), 0xfc0b);
        assert_eq!(Befehl::I64TruncSatF64U.code(), 0xfc07);
        assert_eq!(Befehl::RefFunc(3).code(), 0xd2);
        assert_eq!(Befehl::I32Load { versatz: 4 }.code(), 0x28);
    }

    #[test]
    fn lesen_mit_immediates() {
        assert_eq!(lies_befehl(&mut l(&[0x01]), 0).unwrap(), Befehl::Nop);
        assert_eq!(
            lies_befehl(&mut l(&[0x41, 0xe5, 0x8e, 0x26]), 0).unwrap(),
            Befehl::I32Konst(624_485)
        );
        assert_eq!(
            lies_befehl(&mut l(&[0x28, 0x02, 0x10]), 0).unwrap(),
            Befehl::I32Load { versatz: 16 }
        );
        assert_eq!(
            lies_befehl(&mut l(&[0xfc, 0x0b, 0x00]), 0).unwrap(),
            Befehl::MemoryFill
        );
        assert_eq!(
            lies_befehl(&mut l(&[0xfc, 0x0c, 0x01, 0x02]), 0).unwrap(),
            Befehl::TableInit {
                element: 1,
                tabelle: 2
            }
        );
        assert_eq!(
            lies_befehl(&mut l(&[0x02, 0x40]), 0).unwrap(),
            Befehl::Block(Blockart::Leer)
        );
    }

    #[test]
    fn benannte_fehler_statt_panic() {
        assert_eq!(
            lies_befehl(&mut l(&[0xfd, 0x00]), 0).unwrap_err().art(),
            ART_OPCODE_PRAEFIX
        );
        assert_eq!(
            lies_befehl(&mut l(&[0x06]), 0).unwrap_err().art(),
            ART_OPCODE
        );
        assert_eq!(
            lies_befehl(&mut l(&[]), 0).unwrap_err().art(),
            ART_UNVOLLSTAENDIG
        );
        // Speicherindex im memarg (Multi-Memory) -> benannter Fehler
        assert_eq!(
            lies_befehl(&mut l(&[0x28, 0x42, 0x00]), 0)
                .unwrap_err()
                .art(),
            ART_OPCODE
        );
        // Reservierungsbyte != 0
        assert_eq!(
            lies_befehl(&mut l(&[0x3f, 0x01]), 0).unwrap_err().art(),
            ART_OPCODE
        );
        // SIMD-Praefix
        assert!(
            lies_befehl(&mut l(&[0xfd, 0x01]), 0)
                .unwrap_err()
                .kurz()
                .contains("SIMD")
        );
        // Verschachtelungsgrenze
        let e = lies_befehl(&mut l(&[0x01]), MAX_VERSCHACHTELUNG_HART + 1).unwrap_err();
        assert_eq!(e.art(), crate::fehler::ART_GRENZE);
    }

    #[test]
    fn ausrichtung_je_befehl() {
        assert_eq!(natuerliche_ausrichtung(&Befehl::I32Load { versatz: 0 }), 2);
        assert_eq!(
            natuerliche_ausrichtung(&Befehl::I64Store32 { versatz: 0 }),
            2
        );
        assert_eq!(
            natuerliche_ausrichtung(&Befehl::I32Load8U { versatz: 0 }),
            0
        );
        assert_eq!(natuerliche_ausrichtung(&Befehl::F64Store { versatz: 0 }), 3);
    }
}
