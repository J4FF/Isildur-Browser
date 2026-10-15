//! Der flache Schnell-Interpreter (Hebel 3, LEITUNGSAUFTRAG 3).
//!
//! `interp::schritt` ist der Debug-Pfad: Einzelschritt, Haltepunkte,
//! Spur — und ueberall dort, wo ein Befehl einen `Result`-Rueckweg hat,
//! vierzig Byte pro Befehlskopie und eine Pruefung je Schritt. Dieses
//! Modul ist der PARALLELE Laufpfad fuer die Faelle, die beides nicht
//! brauchen: kein Haltepunkt, keine Spur (`lauf.spur.is_none()`), nur
//! Ausfuehrung.
//!
//! Aufbau in drei Phasen je Funktion, alles OHNE `unsafe`, alles
//! `get_unchecked`-frei (Architekturregel: `unsafe_code = "forbid"`):
//!
//! 1. **Uebersetzung** (`uebersetze`): Der Koerper wird EINMAL in ein
//!    flaches `Vec<Op>` uebersetzt. `Block`, `Loop`, `End`, `Nop` und
//!    `Drop` verschwinden dabei ganz (Lauf-Faltung: ihr Treibstoff sitzt
//!    im Gewicht des naechsten Ops). Sprungziele sind FLACHE
//!    Operationsindizes, Labels und Blocke existieren zur Laufzeit nicht
//!    mehr. Jedes Op kennt seine statische Stapelhohe — der Wertestapel
//!    ist ein `Vec<u64>` (typgeloescht, `to_bits`), Locals liegen wie
//!    bisher IMSELBEN Stapel.
//! 2. **Cache** (`kompiziere`): Uebersetzte Programme liegen im Store
//!    (`Store::schnell`). Der Aufrufgraph wird beim ersten Eintritt
//!    vollstaendig geschlossen: WAS ein Op kann, entscheidet sich zur
//!    Uebersetzungszeit — fehlt etwas (Speicher, Globals, i64, GC, …),
//!    scheitert die Uebersetzung und ALLE Betroffenen faellen auf den
//!    alten Pfad zurueck. Kein zurueckgetragener Teilerfolg, keine
//!    verwaosten Halbprogramme.
//! 3. **Ausfuehrung** (`versuche`): EINE `loop { match op }` ohne
//!    `Result` je Befehl, Treibstoff nur an Spruengen und Aufrufen wie
//!    im alten Pfad (`zaehle`/`pruefe_brennstoff`), ein
//!    Bounds-Check je Stapelzugriff ueber die statischen Indizes.
//!
//! **Warum die Uebersetzung scheitern DARF:** Der Rueckfall ist immer
//! korrekt — `interp` fuehrt denselben Koerper mit derselben Semantik
//! aus. Der Schnellpfad ist eine BESCHLEUNIGUNG, keine zweite
//! Wahrheit; die Testsuite (20974 Faelle) laeuft mit BEIDEN Pfaden und
//! muss identische Ergebnisse, identische Traps und identischen
//! `verbraucht`-Zaehler liefern.
//!
//! T1: Konstanten, Locals, i32-Rechnen, Select, Struktur, Spruenge,
//! direkte Aufrufe, `unreachable`.
//! T2a (diese Fassung): ZUSAETZLICH i64-, f32-, f64-Rechnen und alle
//! Konversionen (`wrap`/`extend`/`convert`/`trunc(_sat)`/`demote`/
//! `promote`); die vier `reinterpret`-Befehle fallen als Bit-Identitaet
//! ganz aus. T2b: Speicher und Globals. T2c: Bulk-Speicher, Tabellen,
//! `ref.*` und `call_indirect`. Uebrig (Rueckfall): GC, Exceptions,
//! Tail Calls, `call_ref`/`br_on_*`, `MitSpeicher` (Multi-Memory).

use std::sync::Arc;

use crate::fehler::{
    Fehler, L_SIGNATUR, T_AUFRUFTIEFE, T_DIVISION, T_DIVERSE, T_EINBETTUNG, T_INDIREKT,
    T_OVERFLOW, T_SPEICHER_LESEN, T_SPEICHER_SCHREIBEN, T_TABELLE_LESEN, T_TABELLE_SCHREIBEN,
    T_UNERREICHBAR,
};
use crate::interp::{speicher_mut, speicher_von, wasm_max32, wasm_max64, wasm_min32, wasm_min64};
use crate::lauf::{Funktion, Host, Lauf, ModulKontext};
use crate::modell::{FuncTyp, Reftyp, Valtyp};
use crate::op::{Befehl, Blockart};
use crate::wert::{ist_nan32, ist_nan64, nan32, nan64, nearest, nearest32, trunk, trunk_sat, Ref, Wert};

/// „Keine Kopie" als Marker in `Op::k` / `Kopie`-Verweisen.
const KEIN: u32 = u32::MAX;

/// Sprung ans Rahmenende (`Op::ziel == RAHMEN`): Ergebnisse holen,
/// Rahmen zurueckgeben. Die Zahl kann nie ein echter Operationsindex
/// sein (der Koerper ist hart auf `MAX_BEFEHLE_HART` begrenzt).
const RAHMEN: u32 = u32::MAX;

// ===========================================================================
// Op — das flache, Kopier-Befehlwort (LEITUNG: <= 16 Byte, `Copy`)
// ===========================================================================

/// Ein einzelner i32-Rechenbefehl. Die Art steckt im Discriminant,
/// die Stapelhohe daneben — kein weiterer Speicher je Befehl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I32Art {
    Eqz,
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
    Clz,
    Ctz,
    Popcnt,
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    RemS,
    RemU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Rotl,
    Rotr,
}

/// Ein einzelner i64-Rechenbefehl (T2a) — spiegelt `I32Art` Zeile
/// fuer Zeile, nur mit 64-Bit-Breite.
#[derive(Clone, Copy, Debug)]
pub enum I64Art {
    Eqz,
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
    Clz,
    Ctz,
    Popcnt,
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    RemS,
    RemU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Rotl,
    Rotr,
}

/// Ein einzelner f32-Rechenbefehl (T2a).
///
/// Vergleiche ergeben 0/1 (auf dem flachen Stapel wie `kod` es als
/// i32 legen wuerde: nullerweitert). Das Rechnen geht ueber den WERT
/// (`from_bits`/`to_bits`) — und NUR was der alte Pfad ueber
/// `f32_un`/`f32_bi` kanonisiert, kanonisiert auch dieser Pfad
/// (GEMESSEN: `f32.min` mit fremdem NaN-Muster).
#[derive(Clone, Copy, Debug)]
pub enum F32Art {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Abs,
    Neg,
    Ceil,
    Floor,
    Trunc,
    Nearest,
    Sqrt,
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Copysign,
}

/// Ein einzelner f64-Rechenbefehl (T2a) — spiegelt `F32Art`.
#[derive(Clone, Copy, Debug)]
pub enum F64Art {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Abs,
    Neg,
    Ceil,
    Floor,
    Trunc,
    Nearest,
    Sqrt,
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Copysign,
}

/// Wandlungs- und Verlaengerungsbefehle (T2a): IMMER 1 -> 1, sie
/// schreiben auf den Quellplatz. `i32.wrap_i64` und
/// `i64.extend_i32_u` sind bitidentisch und teilen sich `Wrap`.
/// Die vier `reinterpret`-Befehle fehlen bewusst: ihre Bits sind auf
/// dem flachen Stapel IDENTISCH, sie erzeugen kein Op (ihr
/// Treibstoff faehrt als `pending` zum naechsten Op mit).
#[derive(Clone, Copy, Debug)]
pub enum WandelArt {
    /// `i32.wrap_i64` UND `i64.extend_i32_u`
    Wrap,
    ExtendI32S,
    ExtendI328S,
    ExtendI3216S,
    ExtendI648S,
    ExtendI6416S,
    ExtendI6432S,
    TruncI32F32S,
    TruncI32F32U,
    TruncI32F64S,
    TruncI32F64U,
    TruncI64F32S,
    TruncI64F32U,
    TruncI64F64S,
    TruncI64F64U,
    SatI32F32S,
    SatI32F32U,
    SatI32F64S,
    SatI32F64U,
    SatI64F32S,
    SatI64F32U,
    SatI64F64S,
    SatI64F64U,
    ConvF32I32S,
    ConvF32I32U,
    ConvF32I64S,
    ConvF32I64U,
    ConvF64I32S,
    ConvF64I32U,
    ConvF64I64S,
    ConvF64I64U,
    /// `f32.demote_f64` (NaN wie alter Pfad kanonisiert)
    Demote,
    /// `f64.promote_f32` (NaN wie alter Pfad kanonisiert)
    Promote,
}

/// Speicher-Ladebefehl (T2b): Ergebnistyp UND Breite UND Vorzeichen
/// in EINER Art — der Kopf bleibt h -> h (Adresse raus, Wert rein).
#[derive(Clone, Copy, Debug)]
pub enum LadeArt {
    I32,
    I32S8,
    I32U8,
    I32S16,
    I32U16,
    I64,
    I64S8,
    I64U8,
    I64S16,
    I64U16,
    I64S32,
    I64U32,
    F32,
    F64,
}

impl LadeArt {
    /// Zugriffsbreite in Bytes (`1 << breite` wie im alten Pfad).
    fn bytes(self) -> usize {
        match self {
            LadeArt::I32S8 | LadeArt::I32U8 | LadeArt::I64S8 | LadeArt::I64U8 => 1,
            LadeArt::I32S16 | LadeArt::I32U16 | LadeArt::I64S16 | LadeArt::I64U16 => 2,
            LadeArt::I32 | LadeArt::I64S32 | LadeArt::I64U32 | LadeArt::F32 => 4,
            LadeArt::I64 | LadeArt::F64 => 8,
        }
    }
}

/// Speicher-Schreibbefehl (T2b): Wert oben, Adresse darunter, Kopf
/// h -> h-2. Die Breite steckt in der Art (8/16/32/Breite).
#[derive(Clone, Copy, Debug)]
pub enum SchreibArt {
    I32,
    I32B8,
    I32B16,
    I64,
    I64B8,
    I64B16,
    I64B32,
    F32,
    F64,
}

impl SchreibArt {
    /// Zugriffsbreite in Bytes.
    fn bytes(self) -> usize {
        match self {
            SchreibArt::I32B8 | SchreibArt::I64B8 => 1,
            SchreibArt::I32B16 | SchreibArt::I64B16 => 2,
            SchreibArt::I32 | SchreibArt::I64B32 | SchreibArt::F32 => 4,
            SchreibArt::I64 | SchreibArt::F64 => 8,
        }
    }
}

/// Ein uebersetzter Befehl.
///
/// Layout-Regeln (LEITUNG): kein Payload ueber 12 Byte in EINEM
/// Variant (4×u32 waeren 17 Byte mit Discriminant), alles andere in
/// Nebentabellen (`kopien`, `werte`, `aufrufe`, `brtabellen`).
/// `h` ist IMMER die statische Stapelhohe VOR dem Befehl (Slots
/// `basis..basis+h` sind gueltig, der naechste Schreibplatz ist `h`).
#[derive(Clone, Copy, Debug)]
pub enum Op {
    /// Leichter Traeger fuer gefaltete Struktur: zaehlt, sonst nichts.
    Nichts,
    Konst32 { h: u32, v: u32 },
    /// `i64`/`f32`/`f64`-Konstante: Bits in `Programm::werte[w]`.
    Konst64 { h: u32, w: u32 },
    LokalHolen { h: u32, i: u32 },
    LokalSetzen { h: u32, i: u32 },
    LokalHalten { h: u32, i: u32 },
    I32 { art: I32Art, h: u32 },
    I64 { art: I64Art, h: u32 },
    F32 { art: F32Art, h: u32 },
    F64 { art: F64Art, h: u32 },
    /// Wandlung/Verlaengerung: 1 -> 1, Ergebnis auf den Quellplatz.
    Wandel { art: WandelArt, h: u32 },
    /// Laden: Adresse (i32, roh) raus, Wert rein — Kopf bleibt h -> h.
    /// `versatz` wird wie im alten Pfad in u64-RECHNUNG addiert.
    Lade { art: LadeArt, h: u32, versatz: u32 },
    /// Speichern: Wert oben, Adresse darunter — Kopf h -> h-2.
    Schreib { art: SchreibArt, h: u32, versatz: u32 },
    /// `memory.size`: pusht die Seitenzahl (0 -> 1).
    SpeicherGroesse { h: u32 },
    /// `memory.grow`: poppt das Delta, pusht die alte Seitenzahl.
    SpeicherWachs { h: u32 },
    /// `global.get i`: pusht den rohen Zellenwert (0 -> 1).
    GlobalHolen { i: u32, h: u32 },
    /// `global.set i`: schreibt den oberen Rohwert in die Zelle.
    GlobalSetzen { i: u32, h: u32 },
    // ---- T6-Superbefehle (GEMESSEN: perf.rs::haeufigkeit) --------------
    /// `local.get l; i32.const c; <cmp>; br_if` — der Vergleichsweg
    /// steckt in `Programm::vergleiche[b]`; das Ergebnis kommt NICHT
    /// auf den Stapel, es entscheidet sofort ueber den Sprung.
    BrWennWahrVergleich { h: u32, ziel: u32, b: u32 },
    /// `local.get l; i32.const c; <cmp>; if` — Sprung zum Zweig bei
    /// FALSE, ohne Kopie (wie `BrWennFalsch`).
    BrWennFalschVergleich { ziel: u32, b: u32 },
    /// `local.get z; local.get q; i32.add; local.set z` (z += q).
    LokalAddLokal { ziel: u32, quel: u32 },
    /// `local.get z; i32.const c; i32.add; local.set z` (z += c).
    LokalAddKonst { ziel: u32, konst: u32 },
    /// `local.get z; i32.add; local.set z` (z += oberster Wert).
    LokalAddOben { ziel: u32, h: u32 },
    /// `local.get l; i32.const c; i32.sub` — pusht `l - c` (fuettert
    /// den naechsten Aufruf, z. B. `call $fib (n-1)`).
    LokalSubKonst { l: u32, c: u32, h: u32 },
    /// `select` / `select t` — identische Ausfuehrung.
    Wahl { h: u32 },
    /// `br_if`: bei Null weiter, sonst pruefen, kopieren, springen.
    BrWennWahr { h: u32, ziel: u32, k: u32 },
    /// `if`: Bedingung ist bei `h-1`, bei FALSE zum Zweig sprungen.
    /// Kein Treibstoff-Test (alter Pfad: `If` prueft auch nicht).
    BrWennFalsch { h: u32, ziel: u32 },
    /// `br`: pruefen, kopieren, springen. `ziel == RAHMEN` beendet
    /// den Rahmen (Rueckkehr auf `br 0`).
    Sprung { h: u32, ziel: u32, k: u32 },
    /// Then-Zweig ueberspringen (am `else`): kein Test, keine Kopie.
    Weiter { ziel: u32 },
    /// `br_table`: Eintraege in `Programm::brtabellen[t]`, Default
    /// am Ende (so greift der Index-Vergleich wie im alten Pfad).
    BrTabelle { h: u32, t: u32 },
    /// `return` oder Durchlauf bis zum Koerperende: OHNE
    /// Treibstoff-Test (alter Pfad: `Return`/`end` pruefen auch nicht).
    Rueckkehr { h: u32 },
    /// Direkter Aufruf: Seitenliste `Programm::aufrufe[a]`.
    Aufruf { h: u32, a: u32 },
    // ---- T2c: Bulk, Tabellen, Ref, call_indirect -----------------------
    /// Bulk-Speicher (inkl. `data.drop`): `h` = Hoehe VOR dem Befehl
    /// (die drei Slots `ziel/quelle/laenge` liegen darunter),
    /// `a` = Abschnittsindex bzw. Ziel-Speicher, `b` = Quellspeicher
    /// (`memory.copy`) — alle MODUL-lokal, Aufloesung zur Laufzeit
    /// ueber `k` (der Cache gilt fuer alle Instanzen).
    Bulk { art: BulkArt, h: u32, a: u32, b: u32 },
    /// Tabellenbefehl: `h` = Hoehe VOR dem Befehl, `a` = Tabellen-
    /// bzw. Elementindex (modul-lokal), `b` = zweiter Tabellenindex
    /// (nur `table.copy`).
    Tabelle {
        art: TabellenArt,
        h: u32,
        a: u32,
        b: u32,
    },
    /// `call_indirect (type t) (table x)`: Weg in `Programm::indizierte`.
    /// Der Tabellenindex liegt als oberster Wert bei `h`.
    Indirekt { h: u32, i: u32 },
    /// `ref.is_null`: roh 0 (Func-/Extern-null) -> 1, sonst 0 (h -> h).
    /// `Gcref(None)` traegt `KENN` und ist NICHT null (wie alter Pfad).
    RefIstNull { h: u32 },
    /// `ref.as_non_null`: null trapt (`T_INDIREKT`), sonst bleibt der
    /// Wert liegen (h -> h).
    RefNichtNull { h: u32 },
    /// `ref.func f`: pusht Store-Index + 1 (Indexraum `k.funktionen`).
    RefFunktion { h: u32, f: u32 },
    Unreachable,
}

// LEITUNG: haerte Obergrenze fuer die Dispatch-Schleife.
const _: () = assert!(std::mem::size_of::<Op>() <= 16);

/// Art eines `Op::Bulk`-Befehls (T2c).
#[derive(Clone, Copy, Debug)]
pub enum BulkArt {
    /// `memory.init a` — `a` = Datenabschnitt (modul-lokal), die drei
    /// Slots sind ziel/quelle/laenge.
    Abschnitt,
    /// `memory.copy` — `a` = Ziel-, `b` = Quellspeicher (modul-lokal).
    Kopie,
    /// `memory.fill` — Slots ziel/wert/laenge.
    Fuellen,
    /// `data.drop a`.
    DatenWeg,
}

/// Art eines `Op::Tabelle`-Befehls (T2c).
#[derive(Clone, Copy, Debug)]
pub enum TabellenArt {
    /// `table.get a` — Index oben, Ergebnis auf denselben Platz.
    Holen,
    /// `table.set a` — wert oben, index darunter.
    Setzen,
    /// `table.size a` — pusht die Laenge.
    Anzahl,
    /// `table.grow a` — wert darunter, delta oben; pusht die alte Laenge.
    Wachsen,
    /// `table.fill a` — start/wert/laenge.
    Fuellen,
    /// `table.copy` — `a` = Ziel-, `b` = Quelltabelle (modul-lokal).
    Kopieren,
    /// `table.init` — `a` = Elementabschnitt, `b` = Tabelle.
    Abschnitt,
    /// `elem.drop a`.
    ElementWeg,
}

/// Kopierbefehl fuer Spruerge mit Ergebniswerten (Nebentabelle, weil
/// drei u32 das Op sonst auf 17 Byte druecken wuerden).
#[derive(Clone, Copy, Debug)]
pub struct Kopie {    /// Quelle relativ zu `basis` (hoechster Wert zuletzt).
    von: u32,
    /// Ziel relativ zu `basis` (Label-Hoehe).
    nach: u32,
    /// Anzahl der Werte (Label-Arity).
    anzahl: u32,
}

/// Ein Vergleichsweg der T6-Superbefehle: `stapel[lokal] OP konst`.
/// Das Ergebnis kommt NICHT auf den Stapel — es entscheidet sofort
/// ueber den Sprung, deshalb liegt der Weg in einer Nebentabelle
/// (I32Art + drei u32 duerften das Op sonst zu gross machen).
#[derive(Clone, Copy, Debug)]
struct VergleichWeg {
    /// Lokaler Index (Rohwert, wie `LokalHolen` liest).
    lokal: u32,
    /// Konstante aus `i32.const` (Bits).
    konst: u32,
    art: I32Art,
    /// Label-Arity fuer `br_if` (Kopie wie `BrWennWahr`); bei `if`
    /// `KEIN` — dort wird nichts kopiert.
    k: u32,
}

/// Ein `br_table`-Eintrag: Ziel-Operationsindex + Kopie.
#[derive(Clone, Copy, Debug)]
pub struct BrEintrag {
    ziel: u32,
    k: u32,
}

/// Seitenliste eines direkten Aufrufs.
#[derive(Debug)]
pub struct AufrufZiel {
    global: u32,
    /// Parameteranzahl (auch fuer Eigen: bestimmt `lokal`).
    n: u32,
}

/// Ein `call_indirect`-Weg: Tabelle und Typindex (beide MODUL-lokal —
/// die Aufloesung geschieht zur Laufzeit ueber den Kontext `k`).
#[derive(Debug)]
struct IndirektWeg {
    tabelle: u32,
    typ: u32,
}

/// Das uebersetzte Programm EINER Funktion.
#[derive(Debug)]
pub struct Programm {
    ops: Vec<Op>,
    /// Treibstoffgewicht je Op (Quellbefehle, die darin stecken).
    gewichte: Vec<u32>,
    kopien: Vec<Kopie>,
    werte: Vec<u64>,
    aufrufe: Vec<AufrufZiel>,
    /// Wege der T6-Vergleichs-Superbefehle.
    vergleiche: Vec<VergleichWeg>,
    brtabellen: Vec<Vec<BrEintrag>>,
    /// Wege der `call_indirect`-Aufrufe (T2c).
    indizierte: Vec<IndirektWeg>,
    /// Ergebnisanzahl (Rahmenende).
    ergebnis_anzahl: usize,
    /// Stapelfelder ueber `basis` (statische Maximalhoehe + 1).
    feld: usize,
    /// Nicht-Parameter-Lokale (Defaults wie `interp`).
    lokale: Vec<Valtyp>,
    /// Store-Indizes der Eigen-Aufrufe (Schliessung zur Uebersetzungszeit).
    call_ziele: Vec<u32>,
}

/// Cache-Eintrag je Store-Index in `Store::schnell`.
#[derive(Debug)]
pub enum Status {
    /// Noch nie geprueft.
    Neu,
    /// Uebersetzung laeuft (Zyklusmarker).
    Laeuft,
    /// Uebersetzt und im Aufrufgraphen bestaetigt.
    Okay(Arc<Programm>),
    /// Nicht uebersetzbar — dauerhaft (der Koerper ist statisch).
    Aus,
}

// ===========================================================================
// Wert-Kodierung: Wert <-> u64
// ===========================================================================

/// Kennzeichnung fuer GC-Referenzen: `Gcref` braucht einen eigenen
/// Bereich, weil `RefIsNull` nur `Func(None)|Extern(None)` als null
/// sieht, `RefAsNonNull` dagegen AUCH `Gcref(None)` trappt.
const KENN: u64 = 1 << 63;

/// `Wert` -> Rohwert auf dem flachen Stapel.
///
/// `i32`/`f32` nullerweitert, `i64`/`f64` roh, Referenzen:
/// null = 0 (Func/Extern), Id + 1, GC mit `KENN`-Bit (auch null, weil
/// der alte Pfad `Gcref(None)` NICHT als null sieht — GEMESSEN an
/// `ref.is_null`/`ref.as_non_null` in `interp`).
fn kod(w: Wert) -> u64 {
    match w {
        Wert::I32(v) => v as u32 as u64,
        Wert::I64(v) => v as u64,
        Wert::F32(b) => b as u64,
        Wert::F64(b) => b,
        Wert::Func(None) | Wert::Extern(None) => 0,
        Wert::Func(Some(i)) | Wert::Extern(Some(i)) => i as u64 + 1,
        Wert::Gcref(None) => KENN,
        Wert::Gcref(Some(i)) => KENN | (i as u64 + 1),
    }
}

/// Rohwert -> `Wert`, typgerichtet an der SCHNITTSTELLE (Argumente,
/// Ergebnisse, Hostaufrufe). Im Inneren bleibt alles roh.
fn dek(t: Valtyp, w: u64) -> Wert {
    match t {
        Valtyp::I32 => Wert::I32(w as u32 as i32),
        Valtyp::I64 => Wert::I64(w as i64),
        Valtyp::F32 => Wert::F32(w as u32),
        Valtyp::F64 => Wert::F64(w),
        // Extern: `externref` UND die nicht-leerbare Wildcardform
        // `(ref extern)` = `Konkret { typ: JEDER_EXTERNTYP }` (die der
        // Parser liefert) — GEMESSEN an `Reftyp::ist_extern`.
        Valtyp::Ref(r) if r.ist_extern() && w & KENN == 0 => Wert::Extern(if w == 0 {
            None
        } else {
            Some((w - 1) as u32)
        }),
        Valtyp::Ref(_) if w == 0 => Wert::Func(None),
        Valtyp::Ref(_) if w & KENN != 0 => {
            let id = w & !KENN;
            Wert::Gcref(if id == 0 { None } else { Some((id - 1) as u32) })
        }
        // Nullerweiterte Func-/Extern-Id (Kodierungsbereich 1..2^63).
        Valtyp::Ref(_) => Wert::Func(Some((w - 1) as u32)),
    }
}

/// `Wert`-Zelle typsicher fuellen, wenn nur der FLACHE Rohwert da ist
/// (T2b `global.set`): die Zelle selbst ist die Vorlage fuer den Typ —
/// ihre Variante entscheidet, wie `dek(Valtyp, w)` hier waehlen wuerde.
fn dek_wie(vorlage: &Wert, w: u64) -> Wert {
    match vorlage {
        Wert::I32(_) => Wert::I32(w as u32 as i32),
        Wert::I64(_) => Wert::I64(w as i64),
        Wert::F32(_) => Wert::F32(w as u32),
        Wert::F64(_) => Wert::F64(w),
        Wert::Extern(_) => Wert::Extern(if w == 0 { None } else { Some((w - 1) as u32) }),
        // Func/Gcref-Zelle: dieselbe Entscheidung wie `dek` bei
        // `Valtyp::Ref(_)` (0 = null, KENN = gcref, sonst func-id).
        _ => {
            if w == 0 {
                Wert::Func(None)
            } else if w & KENN != 0 {
                let id = w & !KENN;
                Wert::Gcref(if id == 0 { None } else { Some((id - 1) as u32) })
            } else {
                Wert::Func(Some((w - 1) as u32))
            }
        }
    }
}

// ===========================================================================
// Uebersetzung — Phase 0: Struktur-Matching
// ===========================================================================

/// Sprungziele je Struktur-Beginn: `(else-Position, end-Position)`.
/// Identische Logik wie `interp::Sprungtabelle::baue` — nur ohne
/// Laufzeitbezug (die Tabelle wird zur Uebersetzungszeit gebraucht,
/// nicht mehr je Schritt).
fn baue_sprung(koerper: &[Befehl]) -> Vec<Option<(Option<usize>, usize)>> {
    let mut ziele = vec![None; koerper.len()];
    let mut offen: Vec<(usize, Option<usize>)> = Vec::new();
    for (i, b) in koerper.iter().enumerate() {
        match b {
            Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_) | Befehl::TryTable(..) => {
                offen.push((i, None));
            }
            Befehl::Else => {
                if let Some(o) = offen.last_mut() {
                    if o.1.is_none() {
                        o.1 = Some(i);
                    }
                }
            }
            Befehl::End => {
                if let Some(s) = offen.pop() {
                    ziele[s.0] = Some((s.1, i));
                }
            }
            _ => {}
        }
    }
    ziele
}

/// Blocktyp -> `(Parameter, Ergebnisse)` — 1:1 wie `interp::blocktypen`
/// (`Werttyp` ist (0, 1), ein unbekannter Typindex (0, 0)).
fn blocktypen(k: &ModulKontext, bt: &Blockart) -> (usize, usize) {
    match bt {
        Blockart::Leer => (0, 0),
        Blockart::Werttyp(_) => (0, 1),
        Blockart::Typ(i) => k
            .typen
            .get(*i as usize)
            .map(|t| (t.parameter.len(), t.ergebnis.len()))
            .unwrap_or((0, 0)),
    }
}

/// i32-Befehl -> Art (T1-Umfang). `None` = nicht unterstuetzt.
fn i32_art(b: &Befehl) -> Option<I32Art> {
    use I32Art as A;
    let a = match b {
        Befehl::I32Eqz => A::Eqz,
        Befehl::I32Eq => A::Eq,
        Befehl::I32Ne => A::Ne,
        Befehl::I32LtS => A::LtS,
        Befehl::I32LtU => A::LtU,
        Befehl::I32GtS => A::GtS,
        Befehl::I32GtU => A::GtU,
        Befehl::I32LeS => A::LeS,
        Befehl::I32LeU => A::LeU,
        Befehl::I32GeS => A::GeS,
        Befehl::I32GeU => A::GeU,
        Befehl::I32Clz => A::Clz,
        Befehl::I32Ctz => A::Ctz,
        Befehl::I32Popcnt => A::Popcnt,
        Befehl::I32Add => A::Add,
        Befehl::I32Sub => A::Sub,
        Befehl::I32Mul => A::Mul,
        Befehl::I32DivS => A::DivS,
        Befehl::I32DivU => A::DivU,
        Befehl::I32RemS => A::RemS,
        Befehl::I32RemU => A::RemU,
        Befehl::I32And => A::And,
        Befehl::I32Or => A::Or,
        Befehl::I32Xor => A::Xor,
        Befehl::I32Shl => A::Shl,
        Befehl::I32ShrS => A::ShrS,
        Befehl::I32ShrU => A::ShrU,
        Befehl::I32Rotl => A::Rotl,
        Befehl::I32Rotr => A::Rotr,
        _ => return None,
    };
    Some(a)
}

/// Bindet der i32-Befehl keine Werte? (Hoehe bleibt.)
fn i32_unaer(a: I32Art) -> bool {
    matches!(
        a,
        I32Art::Eqz | I32Art::Clz | I32Art::Ctz | I32Art::Popcnt
    )
}

/// Nur die binären Vergleichs-Arten (`br_if`/`if`-Superwege).
fn ist_vergleich(a: I32Art) -> bool {
    use I32Art as A;
    matches!(
        a,
        A::Eq | A::Ne | A::LtS | A::LtU | A::GtS | A::GtU | A::LeS | A::LeU | A::GeS | A::GeU
    )
}

/// Vergleich wie in `Op::I32` (gleiche `cmp_u`/`cmp_s`-Lesart): roher
/// Lokalwert `a` gegen die Konstante `b`.
fn vergleicht(a: u32, b: u32, art: I32Art) -> bool {
    use I32Art as A;
    match art {
        A::Eq => a == b,
        A::Ne => a != b,
        A::LtS => (a as i32) < (b as i32),
        A::LtU => a < b,
        A::GtS => (a as i32) > (b as i32),
        A::GtU => a > b,
        A::LeS => (a as i32) <= (b as i32),
        A::LeU => a <= b,
        A::GeS => (a as i32) >= (b as i32),
        A::GeU => a >= b,
        // Wird nie erzeugt: `muster` fusst nur Vergleichs-Arten.
        _ => false,
    }
}

/// T6: Die Supermuster der vier Lasten (GEMESSEN in
/// `perf.rs::haeufigkeit_der_vier_lasten`) — gerade Linien, die MIT
/// einem `local.get` starten. Rückgabe: (Muster, Anzahl Quellbefehle).
enum Muster {
    /// `get l; i32.const c; <cmp>; br_if t`
    BrVergleich { l: u32, c: u32, art: I32Art, t: u32 },
    /// `get l; i32.const c; <cmp>; if bt`
    WennVergleich { l: u32, c: u32, art: I32Art, bt: Blockart },
    /// `get z; get q; i32.add; local.set z`
    AddLokale { ziel: u32, quel: u32 },
    /// `get z; i32.const c; i32.add; local.set z`
    AddKonst { ziel: u32, konst: u32 },
    /// `get z; i32.add; local.set z`
    AddOben { ziel: u32 },
    /// `get l; i32.const c; i32.sub` — pusht `l - c`
    SubKonst { l: u32, c: u32 },
}

fn muster(koerper: &[Befehl], pc: usize) -> Option<(Muster, usize)> {
    use Befehl as B;
    let B::LocalGet(l) = koerper.get(pc)? else {
        return None;
    };
    let l = *l;
    let z1 = koerper.get(pc + 1)?;
    let z2 = koerper.get(pc + 2)?;
    match z1 {
        B::I32Konst(konst) => {
            let konst = *konst as u32;
            // Vierer mit Vergleich: get; const; cmp; br_if / if.
            let art = i32_art(z2).filter(|&a| ist_vergleich(a));
            if let (Some(art), d) = (art, koerper.get(pc + 3)) {
                match d {
                    Some(B::BrIf(t)) => {
                        return Some((
                            Muster::BrVergleich {
                                l,
                                c: konst,
                                art,
                                t: *t,
                            },
                            4,
                        ));
                    }
                    Some(B::If(bt)) => {
                        return Some((
                            Muster::WennVergleich {
                                l,
                                c: konst,
                                art,
                                bt: *bt,
                            },
                            4,
                        ));
                    }
                    _ => {}
                }
            }
            // Vierer: get; const; add; set — und Dreier: get; const; sub.
            if i32_art(z2) == Some(I32Art::Add)
                && matches!(koerper.get(pc + 3), Some(B::LocalSet(z)) if *z == l)
            {
                return Some((Muster::AddKonst { ziel: l, konst }, 4));
            }
            if i32_art(z2) == Some(I32Art::Sub) {
                return Some((Muster::SubKonst { l, c: konst }, 3));
            }
        }
        B::LocalGet(q) => {
            // Vierer: get z; get q; add; set z
            if i32_art(z2) == Some(I32Art::Add)
                && matches!(koerper.get(pc + 3), Some(B::LocalSet(z)) if *z == l)
            {
                return Some((
                    Muster::AddLokale {
                        ziel: l,
                        quel: *q,
                    },
                    4,
                ));
            }
        }
        _ if i32_art(z1) == Some(I32Art::Add) => {
            // Dreier: get z; add; set z
            if matches!(z2, B::LocalSet(z) if *z == l) {
                return Some((Muster::AddOben { ziel: l }, 3));
            }
        }
        _ => {}
    }
    None
}

/// i64-Befehl -> Art (T2a). `None` = nicht unterstuetzt.
fn i64_art(b: &Befehl) -> Option<I64Art> {
    use I64Art as A;
    let a = match b {
        Befehl::I64Eqz => A::Eqz,
        Befehl::I64Eq => A::Eq,
        Befehl::I64Ne => A::Ne,
        Befehl::I64LtS => A::LtS,
        Befehl::I64LtU => A::LtU,
        Befehl::I64GtS => A::GtS,
        Befehl::I64GtU => A::GtU,
        Befehl::I64LeS => A::LeS,
        Befehl::I64LeU => A::LeU,
        Befehl::I64GeS => A::GeS,
        Befehl::I64GeU => A::GeU,
        Befehl::I64Clz => A::Clz,
        Befehl::I64Ctz => A::Ctz,
        Befehl::I64Popcnt => A::Popcnt,
        Befehl::I64Add => A::Add,
        Befehl::I64Sub => A::Sub,
        Befehl::I64Mul => A::Mul,
        Befehl::I64DivS => A::DivS,
        Befehl::I64DivU => A::DivU,
        Befehl::I64RemS => A::RemS,
        Befehl::I64RemU => A::RemU,
        Befehl::I64And => A::And,
        Befehl::I64Or => A::Or,
        Befehl::I64Xor => A::Xor,
        Befehl::I64Shl => A::Shl,
        Befehl::I64ShrS => A::ShrS,
        Befehl::I64ShrU => A::ShrU,
        Befehl::I64Rotl => A::Rotl,
        Befehl::I64Rotr => A::Rotr,
        _ => return None,
    };
    Some(a)
}

/// Bindet der i64-Befehl keine Werte? (Hoehe bleibt.)
fn i64_unaer(a: I64Art) -> bool {
    matches!(
        a,
        I64Art::Eqz | I64Art::Clz | I64Art::Ctz | I64Art::Popcnt
    )
}

/// f32-Befehl -> Art (T2a). `None` = nicht unterstuetzt.
fn f32_art(b: &Befehl) -> Option<F32Art> {
    use F32Art as A;
    let a = match b {
        Befehl::F32Eq => A::Eq,
        Befehl::F32Ne => A::Ne,
        Befehl::F32Lt => A::Lt,
        Befehl::F32Gt => A::Gt,
        Befehl::F32Le => A::Le,
        Befehl::F32Ge => A::Ge,
        Befehl::F32Abs => A::Abs,
        Befehl::F32Neg => A::Neg,
        Befehl::F32Ceil => A::Ceil,
        Befehl::F32Floor => A::Floor,
        Befehl::F32Trunc => A::Trunc,
        Befehl::F32Nearest => A::Nearest,
        Befehl::F32Sqrt => A::Sqrt,
        Befehl::F32Add => A::Add,
        Befehl::F32Sub => A::Sub,
        Befehl::F32Mul => A::Mul,
        Befehl::F32Div => A::Div,
        Befehl::F32Min => A::Min,
        Befehl::F32Max => A::Max,
        Befehl::F32Copysign => A::Copysign,
        _ => return None,
    };
    Some(a)
}

/// Bindet der f32-Befehl keine Werte? (Hoehe bleibt.)
fn f32_unaer(a: F32Art) -> bool {
    matches!(
        a,
        F32Art::Abs
            | F32Art::Neg
            | F32Art::Ceil
            | F32Art::Floor
            | F32Art::Trunc
            | F32Art::Nearest
            | F32Art::Sqrt
    )
}

/// f64-Befehl -> Art (T2a). `None` = nicht unterstuetzt.
fn f64_art(b: &Befehl) -> Option<F64Art> {
    use F64Art as A;
    let a = match b {
        Befehl::F64Eq => A::Eq,
        Befehl::F64Ne => A::Ne,
        Befehl::F64Lt => A::Lt,
        Befehl::F64Gt => A::Gt,
        Befehl::F64Le => A::Le,
        Befehl::F64Ge => A::Ge,
        Befehl::F64Abs => A::Abs,
        Befehl::F64Neg => A::Neg,
        Befehl::F64Ceil => A::Ceil,
        Befehl::F64Floor => A::Floor,
        Befehl::F64Trunc => A::Trunc,
        Befehl::F64Nearest => A::Nearest,
        Befehl::F64Sqrt => A::Sqrt,
        Befehl::F64Add => A::Add,
        Befehl::F64Sub => A::Sub,
        Befehl::F64Mul => A::Mul,
        Befehl::F64Div => A::Div,
        Befehl::F64Min => A::Min,
        Befehl::F64Max => A::Max,
        Befehl::F64Copysign => A::Copysign,
        _ => return None,
    };
    Some(a)
}

/// Bindet der f64-Befehl keine Werte? (Hoehe bleibt.)
fn f64_unaer(a: F64Art) -> bool {
    matches!(
        a,
        F64Art::Abs
            | F64Art::Neg
            | F64Art::Ceil
            | F64Art::Floor
            | F64Art::Trunc
            | F64Art::Nearest
            | F64Art::Sqrt
    )
}

/// Wandlungs-/Verlaengerungsbefehl -> Art (T2a). IMMER Hoehe 1 -> 1.
fn wandel_art(b: &Befehl) -> Option<WandelArt> {
    use WandelArt as W;
    let a = match b {
        Befehl::I32WrapI64 | Befehl::I64ExtendI32U => W::Wrap,
        Befehl::I64ExtendI32S => W::ExtendI32S,
        Befehl::I32Extend8S => W::ExtendI328S,
        Befehl::I32Extend16S => W::ExtendI3216S,
        Befehl::I64Extend8S => W::ExtendI648S,
        Befehl::I64Extend16S => W::ExtendI6416S,
        Befehl::I64Extend32S => W::ExtendI6432S,
        Befehl::I32TruncF32S => W::TruncI32F32S,
        Befehl::I32TruncF32U => W::TruncI32F32U,
        Befehl::I32TruncF64S => W::TruncI32F64S,
        Befehl::I32TruncF64U => W::TruncI32F64U,
        Befehl::I64TruncF32S => W::TruncI64F32S,
        Befehl::I64TruncF32U => W::TruncI64F32U,
        Befehl::I64TruncF64S => W::TruncI64F64S,
        Befehl::I64TruncF64U => W::TruncI64F64U,
        Befehl::I32TruncSatF32S => W::SatI32F32S,
        Befehl::I32TruncSatF32U => W::SatI32F32U,
        Befehl::I32TruncSatF64S => W::SatI32F64S,
        Befehl::I32TruncSatF64U => W::SatI32F64U,
        Befehl::I64TruncSatF32S => W::SatI64F32S,
        Befehl::I64TruncSatF32U => W::SatI64F32U,
        Befehl::I64TruncSatF64S => W::SatI64F64S,
        Befehl::I64TruncSatF64U => W::SatI64F64U,
        Befehl::F32ConvertI32S => W::ConvF32I32S,
        Befehl::F32ConvertI32U => W::ConvF32I32U,
        Befehl::F32ConvertI64S => W::ConvF32I64S,
        Befehl::F32ConvertI64U => W::ConvF32I64U,
        Befehl::F64ConvertI32S => W::ConvF64I32S,
        Befehl::F64ConvertI32U => W::ConvF64I32U,
        Befehl::F64ConvertI64S => W::ConvF64I64S,
        Befehl::F64ConvertI64U => W::ConvF64I64U,
        Befehl::F32DemoteF64 => W::Demote,
        Befehl::F64PromoteF32 => W::Promote,
        _ => return None,
    };
    Some(a)
}

/// Speicher-Ladebefehl -> (Art, Versatz) (T2b). `None` = nicht
/// unterstuetzt (`MitSpeicher` mit fremdem Speicherindex bleibt alt).
fn lade_versatz(b: &Befehl) -> Option<(LadeArt, u32)> {
    use LadeArt as A;
    let (a, v) = match b {
        Befehl::I32Load { versatz } => (A::I32, *versatz),
        Befehl::I32Load8S { versatz } => (A::I32S8, *versatz),
        Befehl::I32Load8U { versatz } => (A::I32U8, *versatz),
        Befehl::I32Load16S { versatz } => (A::I32S16, *versatz),
        Befehl::I32Load16U { versatz } => (A::I32U16, *versatz),
        Befehl::I64Load { versatz } => (A::I64, *versatz),
        Befehl::I64Load8S { versatz } => (A::I64S8, *versatz),
        Befehl::I64Load8U { versatz } => (A::I64U8, *versatz),
        Befehl::I64Load16S { versatz } => (A::I64S16, *versatz),
        Befehl::I64Load16U { versatz } => (A::I64U16, *versatz),
        Befehl::I64Load32S { versatz } => (A::I64S32, *versatz),
        Befehl::I64Load32U { versatz } => (A::I64U32, *versatz),
        Befehl::F32Load { versatz } => (A::F32, *versatz),
        Befehl::F64Load { versatz } => (A::F64, *versatz),
        _ => return None,
    };
    Some((a, v))
}

/// Speicher-Schreibbefehl -> (Art, Versatz) (T2b). `None` = nicht
/// unterstuetzt.
fn schreib_versatz(b: &Befehl) -> Option<(SchreibArt, u32)> {
    use SchreibArt as S;
    let (a, v) = match b {
        Befehl::I32Store { versatz } => (S::I32, *versatz),
        Befehl::I32Store8 { versatz } => (S::I32B8, *versatz),
        Befehl::I32Store16 { versatz } => (S::I32B16, *versatz),
        Befehl::I64Store { versatz } => (S::I64, *versatz),
        Befehl::I64Store8 { versatz } => (S::I64B8, *versatz),
        Befehl::I64Store16 { versatz } => (S::I64B16, *versatz),
        Befehl::I64Store32 { versatz } => (S::I64B32, *versatz),
        Befehl::F32Store { versatz } => (S::F32, *versatz),
        Befehl::F64Store { versatz } => (S::F64, *versatz),
        _ => return None,
    };
    Some((a, v))
}

// ===========================================================================
// Rohwert-Referenzen und Modul->Store-Zuordnung (T2c)
// ===========================================================================

/// Rohwert -> `Option<Ref>` fuer Tabellen-Op's: der flache Stapel
/// traegt keine Variante (Func/Extern ist typatisch), die Tabelle
/// entscheidet ueber `extern_` — validierter Code bringt nur passende
/// Werte. Null = 0, `Gcref` = `KENN`-Bit (Zaehler 0 = null).
fn roh_ref(w: u64, extern_: bool) -> Option<Ref> {
    if w == 0 {
        return None;
    }
    if w & KENN != 0 {
        let id = w & !KENN;
        return if id == 0 {
            None
        } else {
            Some(Ref::Gcref((id - 1) as u32))
        };
    }
    let id = (w - 1) as u32;
    if extern_ {
        Some(Ref::Extern(id))
    } else {
        Some(Ref::Func(id))
    }
}

/// `speicher_paar` des alten Pfades auf dem Modulkontext: lokale
/// Speicherindizes -> Store-Indizes, dieselben Trap-Worte.
fn speicher_paar_k(
    k: &ModulKontext,
    ziel: u32,
    quelle: u32,
) -> Result<(u32, u32), Fehler> {
    let hole = |i: u32| -> Result<u32, Fehler> {
        k.speicher.get(i as usize).copied().ok_or_else(|| {
            Fehler::trap(
                T_SPEICHER_LESEN,
                format!(
                    "memory.copy: Speicher {i} unbekannt ({} Speicher)",
                    k.speicher.len()
                ),
            )
        })
    };
    Ok((hole(ziel)?, hole(quelle)?))
}

// ===========================================================================
// Uebersetzung — Phase 1: Stuetzen, Marken, Reichbarkeit, Unterstuetzung
// ===========================================================================

/// Ergebnis der Scan-Phase.
struct Scan {
    /// Hoehe VOR jedem Quellplatz (`len + 1` Eintraege: der letzte ist
    /// die Endhoehe fuer den Durchlauf).
    hoehe: Vec<u32>,
    /// Erreichbar ueber irgendeinen Pfad (linear ODER Sprungziel).
    reichbar: Vec<bool>,
    /// Sprungzielhoehe je Position (die Marke erzwingt Reichbarkeit).
    marke: Vec<Option<u32>>,
    /// Store-Indizes der EIGEN-Aufrufe (Aufrufgraph fuer `dfs`).
    call_ziele: Vec<u32>,
    /// Stapelfelder ueber `basis` (hoechste gueltige Hoehe + 1).
    feld: usize,
    /// Hoehe/Reichbarkeit hinter dem Koerper (Durchlauf-Rueckkehr).
    h_end: u32,
    flag_end: bool,
}

/// Eine Marke setzen. Konflikt zweier Zielhohen an derselben Position
/// heisst: der Koerper ist nicht abbildbar -> Uebersetzung scheitert
/// (sicherer Rueckfall auf `interp`).
fn setze(marke: &mut [Option<u32>], pos: usize, wert: u32) -> Option<()> {
    if pos >= marke.len() {
        // Hinter dem Koerper liegt kein Op mehr (Werttyp-Verschachtelung
        // des Koerperendes gibt es nicht: das schliessende `end` der
        // Funktion steht immer davor).
        return Some(());
    }
    match marke[pos] {
        None => {
            marke[pos] = Some(wert);
            Some(())
        }
        Some(alt) if alt == wert => Some(()),
        Some(_) => None,
    }
}

/// Phase 1: Unterstuetzungs-Pruefung, statische Höhen, Sprungmarken,
/// Reichbarkeit — EIN Gang ueber den Koerper.
fn scan(
    k: &ModulKontext,
    koerper: &[Befehl],
    sprung: &[Option<(Option<usize>, usize)>],
    erg: u32,
    lauf: &Lauf,
) -> Option<Scan> {
    let len = koerper.len();
    let mut hoehe = vec![0u32; len + 1];
    let mut reichbar = vec![false; len];
    let mut marke = vec![None; len];
    let mut call_ziele = Vec::new();
    let mut h: u32 = 0;
    let mut flag = true;
    let mut max_h: u32 = 0;
    for pc in 0..len {
        // Eine Marke macht den Platz lebendig und setzt die anfliegende
        // Hoehe — auch wenn der lineare Pfad davor tot ist (z. B. das
        // `end` eines then-Zweigs, auf den ein `br_if` ohne else faellt).
        if let Some(m) = marke[pc] {
            flag = true;
            h = m;
        }
        hoehe[pc] = h;
        reichbar[pc] = flag;
        if !flag {
            // Toter Bereich: Struktur wird nur zaehlerisch nicht mehr
            // gezaehlt; die Hoehe dort ist bedeutungslos (nichts wird
            // emittiert), die Marke lebendiger Plaetze setzt sie neu.
            continue;
        }
        max_h = max_h.max(h);
        match &koerper[pc] {
            Befehl::Nop => {}
            Befehl::Drop => h = h.checked_sub(1)?,
            Befehl::Block(bt) => {
                let (p, e) = blocktypen(k, bt);
                let lh = h.checked_sub(p as u32)?;
                let (_, end_pos) = *sprung.get(pc)?.as_ref()?;
                setze(&mut marke, end_pos + 1, lh + e as u32)?;
                // Der Koerper betritt den Block mit den p Parametern —
                // die Hoehe ändert sich erst am `end` (validiert).
            }
            Befehl::Loop(bt) => {
                let (p, e) = blocktypen(k, bt);
                let lh = h.checked_sub(p as u32)?;
                // `br` zur Schleife landet hinter der `loop`.
                setze(&mut marke, pc + 1, lh + p as u32)?;
                let (_, end_pos) = *sprung.get(pc)?.as_ref()?;
                setze(&mut marke, end_pos + 1, lh + e as u32)?;
            }
            Befehl::If(bt) => {
                let (p, e) = blocktypen(k, bt);
                let nach = h.checked_sub(1)?;
                let lh = nach.checked_sub(p as u32)?;
                let (else_pos, end_pos) = *sprung.get(pc)?.as_ref()?;
                // if OHNE else mit anderer Arity ist nicht abbildbar:
                // der alte Pfad liefe bei Bedarf auf das `end` und
                // wuerde die Ergebnisse dort vorfinden, der flache
                // Pfad muesste die Parameter zusaetzlich schieben.
                if else_pos.is_none() && p != e {
                    return None;
                }
                // If-false ohne else -> ans `end`.
                setze(&mut marke, end_pos, lh + e as u32)?;
                if let Some(el) = else_pos {
                    // If-false mit else -> hinter das `else`.
                    setze(&mut marke, el + 1, lh + p as u32)?;
                }
                // `br` aus dem then -> hinter das `end`.
                setze(&mut marke, end_pos + 1, lh + e as u32)?;
                h = nach;
            }
            Befehl::Else => {
                // Der then-Pfad endet; `else+1` ist markiert und setzt
                // die Hoehe des else-Zweigs neu.
                flag = false;
            }
            // Das `end` selbst aendert die Hoehe nicht: validiert steht
            // genau `label.hoehe + e` auf dem Stapel.
            Befehl::End => {}
            Befehl::Br(_) | Befehl::Return | Befehl::Unreachable => flag = false,
            Befehl::BrIf(_) => h = h.checked_sub(1)?,
            Befehl::BrTable(..) => {
                h = h.checked_sub(1)?;
                flag = false;
            }
            Befehl::Call(idx) => {
                let g = k.funktionen.get(*idx as usize).copied().unwrap_or(*idx);
                let f = lauf.store.funktionen.get(g as usize)?;
                let (p, e) = match f.as_ref() {
                    Funktion::Eigen { typ, .. } => {
                        call_ziele.push(g);
                        (typ.parameter.len(), typ.ergebnis.len())
                    }
                    Funktion::Fremd { typ, .. } => (typ.parameter.len(), typ.ergebnis.len()),
                };
                h = h.checked_sub(p as u32)?;
                h = h.checked_add(e as u32)?;
            }
            Befehl::LocalGet(_) => h = h.checked_add(1)?,
            Befehl::LocalSet(_) => h = h.checked_sub(1)?,
            Befehl::LocalTee(_) => {}
            Befehl::I32Konst(_) | Befehl::I64Konst(_) | Befehl::F32Konst(_) | Befehl::F64Konst(_) => {
                h = h.checked_add(1)?
            }
            Befehl::Select | Befehl::SelectTyp(_) => h = h.checked_sub(2)?,
            b if i32_art(b).is_some() => {
                let a = i32_art(b)?;
                if !i32_unaer(a) {
                    h = h.checked_sub(1)?;
                }
            }
            b if i64_art(b).is_some() => {
                let a = i64_art(b)?;
                if !i64_unaer(a) {
                    h = h.checked_sub(1)?;
                }
            }
            b if f32_art(b).is_some() => {
                let a = f32_art(b)?;
                if !f32_unaer(a) {
                    h = h.checked_sub(1)?;
                }
            }
            b if f64_art(b).is_some() => {
                let a = f64_art(b)?;
                if !f64_unaer(a) {
                    h = h.checked_sub(1)?;
                }
            }
            b if wandel_art(b).is_some() => {
                // Wandeln 1 -> 1: der Wert bleibt auf dem Quellplatz.
            }
            Befehl::I32ReinterpretF32
            | Befehl::I64ReinterpretF64
            | Befehl::F32ReinterpretI32
            | Befehl::F64ReinterpretI64 => {
                // Bit-Identitaet auf dem flachen Stapel: 1 -> 1.
            }
            b if lade_versatz(b).is_some() => {
                // Laden: Adresse raus (h-1), Wert rein — gleicher Platz.
            }
            b if schreib_versatz(b).is_some() => {
                h = h.checked_sub(2)?;
            }
            Befehl::MemorySize => h = h.checked_add(1)?,
            Befehl::MemoryGrow => {}
            Befehl::GlobalGet(_) => h = h.checked_add(1)?,
            Befehl::GlobalSet(_) => h = h.checked_sub(1)?,
            // ---- T2c: Bulk, Tabellen, Ref, call_indirect ---------------
            Befehl::MemoryInit(_) | Befehl::MemoryCopy { .. } | Befehl::MemoryFill => {
                h = h.checked_sub(3)?
            }
            Befehl::DataDrop(_) | Befehl::ElemDrop(_) => {}
            Befehl::TableGet(_) | Befehl::RefIsNull | Befehl::RefAsNonNull => {
                // 1 -> 1: der oberste Wert bleibt liegen; Zugriff
                // braucht mindestens EIN Feld (sonst invalid — aber
                // nie panic: Uebersetzung scheitert, alt trappt).
                h = h.checked_sub(1)?;
                h = h.checked_add(1)?;
            }
            Befehl::TableSet(_) => h = h.checked_sub(2)?,
            Befehl::TableGrow(_) => {
                // 2 -> 1 (wert darunter, delta oben).
                h = h.checked_sub(2)?;
                h = h.checked_add(1)?;
            }
            Befehl::TableSize(_) | Befehl::RefNull(_) | Befehl::RefFunc(_) => h = h.checked_add(1)?,
            Befehl::TableFill(_) | Befehl::TableCopy { .. } | Befehl::TableInit { .. } => {
                h = h.checked_sub(3)?
            }
            Befehl::CallIndirect { typ, .. } => {
                // Stapel: [Parameter des ANGEGEBENEN Typs, Index] ->
                // Ergebnisse des Typs (der Validator gilt).
                let ft = k.typen.get(*typ as usize)?;
                h = h.checked_sub(1)?;
                h = h.checked_sub(ft.parameter.len() as u32)?;
                h = h.checked_add(ft.ergebnis.len() as u32)?;
            }
            // Alles andere (GC, Exceptions, Tail Calls, call_ref,
            // br_on_*, Multi-Memory-Huelle): erstmal der alte Pfad.
            _ => return None,
        }
    }
    hoehe[len] = h;
    if flag && h < erg {
        // Der Durchlauf muesste Ergebnisse tragen, die nicht da sind —
        // nur in ungueltigem Code moeglich. Sicherer Rueckfall.
        return None;
    }
    Some(Scan {
        hoehe,
        reichbar,
        marke,
        call_ziele,
        feld: max_h as usize + 1,
        h_end: h,
        flag_end: flag,
    })
}

// ===========================================================================
// Uebersetzung — Phase 2: flach emittieren
// ===========================================================================

/// Laufende Struktur beim Emissieren (nur fuer Sprungziel-Aufloesung).
#[derive(Clone, Copy)]
struct Schicht {
    /// Quellplatz des `block`/`loop`/`if` — `usize::MAX` = Funktionsrahmen.
    start: usize,
    hoehe: u32,
    p: u32,
    e: u32,
    schleife: bool,
}

/// Ein noch offener Sprung: der Quellplatz des Ziels wird bekannt,
/// sein FLACHER Operationsindex entsteht erst beim Laufen ueber den
/// Koerper (alle Ziele sind vorwaerts).
enum Patch {
    /// Index in `ops`.
    Op { op: usize, pos: usize },
    /// Eintrag `eintrag` in `brtabellen[t]`.
    Tabelle {
        t: usize,
        eintrag: usize,
        pos: usize,
    },
}

/// Aufgeloestes Sprunglabel.
enum Ziel {
    /// Funktionsrahmen beenden (ohne Kopie — alter Pfad: `ziel_idx == 0`).
    Rahmen,
    /// Quellplatz hinter `end` bzw. hinter der `loop`.
    Stelle { src: usize, a: u32, nach: u32 },
}

/// `br`-Tiefe in ein Ziel uebersetzen (Stack von innen nach aussen).
fn loese(
    struktur: &[Schicht],
    t: u32,
    sprung: &[Option<(Option<usize>, usize)>],
) -> Option<Ziel> {
    let t = t as usize;
    if struktur.len() <= t {
        // Ungueltige Tiefe: der alte Pfad trappt zur Laufzeit — hier
        // scheitert die Uebersetzung, der Rueckfall liefert denselben
        // Trap (und kommt dort nie hin, weil der Validator gilt).
        return None;
    }
    let s = struktur[struktur.len() - 1 - t];
    if s.start == usize::MAX {
        return Some(Ziel::Rahmen);
    }
    if s.schleife {
        Some(Ziel::Stelle {
            src: s.start + 1,
            a: s.p,
            nach: s.hoehe,
        })
    } else {
        let (_, end_pos) = *sprung.get(s.start)?.as_ref()?;
        Some(Ziel::Stelle {
            src: end_pos + 1,
            a: s.e,
            nach: s.hoehe,
        })
    }
}

/// Phase 2: Ausgabe. Jede REICHBARE Position wird emittiert, jede
/// tote Position nur strukturell uebersprungen. Strukturlaeufe faellen
/// in das Gewicht des naechsten Ops; an Sprungzielen wird der Lauf
/// getrennt (ein `Op::Nichts` traegt das Gewicht des Vorsprungs — es
/// wird nur auf dem linearen Weg ausgefuehrt, ein Sprung landet dahinter).
fn emit(
    k: &ModulKontext,
    koerper: &[Befehl],
    typ: &FuncTyp,
    lokale: &[Valtyp],
    sprung: &[Option<(Option<usize>, usize)>],
    scan: &Scan,
    lauf: &Lauf,
) -> Option<Programm> {
    let len = koerper.len();
    let erg = typ.ergebnis.len() as u32;
    let mut ops: Vec<Op> = Vec::with_capacity(len);
    let mut gewichte: Vec<u32> = Vec::with_capacity(len);
    let mut kopien: Vec<Kopie> = Vec::new();
    let mut werte: Vec<u64> = Vec::new();
    let mut aufrufe: Vec<AufrufZiel> = Vec::new();
    let mut vergleiche: Vec<VergleichWeg> = Vec::new();
    let mut brtabellen: Vec<Vec<BrEintrag>> = Vec::new();
    let mut indizierte: Vec<IndirektWeg> = Vec::new();
    // Quellplatz -> Operationsindex (gepatcht, nachdem alles emittiert
    // ist: alle Ziele sind VORWAERTS, aber ihr flacher Index entsteht
    // erst beim Laufen ueber den Koerper).
    let mut src: Vec<usize> = vec![usize::MAX; len];
    let mut struktur: Vec<Schicht> = vec![Schicht {
        start: usize::MAX,
        hoehe: 0,
        p: 0,
        e: erg,
        schleife: false,
    }];
    let mut patches: Vec<Patch> = Vec::new();
    let mut pending: u32 = 0;
    let mut skip: usize = 0;

    for pc in 0..len {
        if skip > 0 {
            // T6: Quellbefehl steckt in einem Superbefehl — hier
            // nichts emittieren; sein Gewicht faehrt als
            // `pending + Anzahl` am Musteranfang mit.
            skip -= 1;
            continue;
        }
        if !scan.reichbar[pc] {
            // Tot: Struktur fortschreiben (fuer Verschachtelungs-
            // Zaehler), nicht zaehlen, nichts emittieren.
            match &koerper[pc] {
                Befehl::Block(bt) => {
                    let (p, e) = blocktypen(k, bt);
                    struktur.push(Schicht {
                        start: pc,
                        hoehe: scan.hoehe[pc].saturating_sub(p as u32),
                        p: p as u32,
                        e: e as u32,
                        schleife: false,
                    });
                }
                Befehl::Loop(bt) => {
                    let (p, e) = blocktypen(k, bt);
                    struktur.push(Schicht {
                        start: pc,
                        hoehe: scan.hoehe[pc].saturating_sub(p as u32),
                        p: p as u32,
                        e: e as u32,
                        schleife: true,
                    });
                }
                Befehl::If(bt) => {
                    let (p, e) = blocktypen(k, bt);
                    struktur.push(Schicht {
                        start: pc,
                        hoehe: scan.hoehe[pc]
                            .saturating_sub(1)
                            .saturating_sub(p as u32),
                        p: p as u32,
                        e: e as u32,
                        schleife: false,
                    });
                }
                Befehl::End => {
                    struktur.pop()?;
                }
                _ => {}
            }
            continue;
        }
        // Am Sprungziel endet der Vorsprung: sein Gewicht braucht einen
        // eigenen Traeger, der NUR auf dem linearen Weg laeuft.
        if scan.marke[pc].is_some() && pending > 0 {
            ops.push(Op::Nichts);
            gewichte.push(pending);
            pending = 0;
        }
        src[pc] = ops.len();
        match &koerper[pc] {
            // Zaehler ohne eigenes Op: `nop`/`drop` und die vier
            // `reinterpret`-Befehle (Bitmuster flach identisch) — ihr
            // Treibstoff faehrt als `pending` zum naechsten Op mit.
            Befehl::Nop
            | Befehl::Drop
            | Befehl::I32ReinterpretF32
            | Befehl::I64ReinterpretF64
            | Befehl::F32ReinterpretI32
            | Befehl::F64ReinterpretI64 => pending += 1,
            Befehl::Block(bt) => {
                let (p, e) = blocktypen(k, bt);
                struktur.push(Schicht {
                    start: pc,
                    hoehe: scan.hoehe[pc].saturating_sub(p as u32),
                    p: p as u32,
                    e: e as u32,
                    schleife: false,
                });
                pending += 1;
            }
            Befehl::Loop(bt) => {
                let (p, e) = blocktypen(k, bt);
                struktur.push(Schicht {
                    start: pc,
                    hoehe: scan.hoehe[pc].saturating_sub(p as u32),
                    p: p as u32,
                    e: e as u32,
                    schleife: true,
                });
                pending += 1;
            }
            Befehl::If(bt) => {
                let (p, e) = blocktypen(k, bt);
                let h = scan.hoehe[pc];
                let lh = h.checked_sub(1)?.checked_sub(p as u32)?;
                struktur.push(Schicht {
                    start: pc,
                    hoehe: lh,
                    p: p as u32,
                    e: e as u32,
                    schleife: false,
                });
                let (else_pos, end_pos) = *sprung.get(pc)?.as_ref()?;
                let ziel_pos = match else_pos {
                    Some(el) => el + 1,
                    None => end_pos,
                };
                ops.push(Op::BrWennFalsch { h, ziel: 0 });
                gewichte.push(pending + 1);
                pending = 0;
                patches.push(Patch::Op {
                    op: ops.len() - 1,
                    pos: ziel_pos,
                });
            }
            Befehl::Else => {
                let s = *struktur.last()?;
                if s.start == usize::MAX {
                    return None; // else ohne if (alter Pfad trappt)
                }
                let (_, end_pos) = *sprung.get(s.start)?.as_ref()?;
                ops.push(Op::Weiter { ziel: 0 });
                gewichte.push(pending + 1);
                pending = 0;
                patches.push(Patch::Op {
                    op: ops.len() - 1,
                    pos: end_pos,
                });
            }
            Befehl::End => {
                struktur.pop()?;
                pending += 1;
            }
            Befehl::Br(t) => {
                let h = scan.hoehe[pc];
                match loese(&struktur, *t, sprung)? {
                    Ziel::Rahmen => {
                        // `br 0`: pruefen, Rahmen beenden, KEINE Kopie
                        // (alter Pfad: `ziel_idx == 0` vor der Kopie).
                        ops.push(Op::Sprung {
                            h,
                            ziel: RAHMEN,
                            k: KEIN,
                        });
                    }
                    Ziel::Stelle { src: z, a, nach } => {
                        let k = if a > 0 {
                            let von = h.checked_sub(a)?;
                            kopien.push(Kopie {
                                von,
                                nach,
                                anzahl: a,
                            });
                            (kopien.len() - 1) as u32
                        } else {
                            KEIN
                        };
                        if z >= len {
                            // Defensive wie alter Pfad: Sprung bis ans
                            // Koerperende beendet den Rahmen.
                            ops.push(Op::Sprung {
                                h,
                                ziel: RAHMEN,
                                k,
                            });
                        } else {
                            ops.push(Op::Sprung { h, ziel: 0, k });
                            patches.push(Patch::Op {
                                op: ops.len() - 1,
                                pos: z,
                            });
                        }
                    }
                }
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::BrIf(t) => {
                let h = scan.hoehe[pc];
                match loese(&struktur, *t, sprung)? {
                    Ziel::Rahmen => {
                        ops.push(Op::BrWennWahr {
                            h,
                            ziel: RAHMEN,
                            k: KEIN,
                        });
                    }
                    Ziel::Stelle { src: z, a, nach } => {
                        // Die Bedingung liegt bei h-1 und faellt weg:
                        // Quelle der Werte ist (h - 1) - a.
                        let k = if a > 0 {
                            let von = h.checked_sub(1)?.checked_sub(a)?;
                            kopien.push(Kopie {
                                von,
                                nach,
                                anzahl: a,
                            });
                            (kopien.len() - 1) as u32
                        } else {
                            KEIN
                        };
                        if z >= len {
                            ops.push(Op::BrWennWahr {
                                h,
                                ziel: RAHMEN,
                                k,
                            });
                        } else {
                            ops.push(Op::BrWennWahr { h, ziel: 0, k });
                            patches.push(Patch::Op {
                                op: ops.len() - 1,
                                pos: z,
                            });
                        }
                    }
                }
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::BrTable(ziele, vorgabe) => {
                let h = scan.hoehe[pc];
                let h1 = h.checked_sub(1)?;
                let mut eintraege: Vec<BrEintrag> = Vec::with_capacity(ziele.len() + 1);
                let mut ziele_pos: Vec<Option<usize>> = Vec::with_capacity(ziele.len() + 1);
                for &d in ziele.iter().chain(std::iter::once(vorgabe)) {
                    match loese(&struktur, d, sprung)? {
                        Ziel::Rahmen => {
                            eintraege.push(BrEintrag {
                                ziel: RAHMEN,
                                k: KEIN,
                            });
                            ziele_pos.push(None);
                        }
                        Ziel::Stelle { src: z, a, nach } => {
                            let k = if a > 0 {
                                let von = h1.checked_sub(a)?;
                                kopien.push(Kopie {
                                    von,
                                    nach,
                                    anzahl: a,
                                });
                                (kopien.len() - 1) as u32
                            } else {
                                KEIN
                            };
                            if z >= len {
                                eintraege.push(BrEintrag {
                                    ziel: RAHMEN,
                                    k,
                                });
                                ziele_pos.push(None);
                            } else {
                                eintraege.push(BrEintrag { ziel: 0, k });
                                ziele_pos.push(Some(z));
                            }
                        }
                    }
                }
                let t_idx = brtabellen.len();
                brtabellen.push(eintraege);
                ops.push(Op::BrTabelle { h, t: t_idx as u32 });
                gewichte.push(pending + 1);
                pending = 0;
                for (e, z) in ziele_pos.into_iter().enumerate() {
                    if let Some(z) = z {
                        patches.push(Patch::Tabelle {
                            t: t_idx,
                            eintrag: e,
                            pos: z,
                        });
                    }
                }
            }
            Befehl::Return => {
                let h = scan.hoehe[pc];
                ops.push(Op::Rueckkehr { h });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::Unreachable => {
                ops.push(Op::Unreachable);
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::Call(idx) => {
                let h = scan.hoehe[pc];
                let g = k.funktionen.get(*idx as usize).copied().unwrap_or(*idx);
                let f = lauf.store.funktionen.get(g as usize)?;
                let p = match f.as_ref() {
                    Funktion::Eigen { typ, .. } | Funktion::Fremd { typ, .. } => {
                        typ.parameter.len()
                    }
                };
                let a = aufrufe.len();
                aufrufe.push(AufrufZiel {
                    global: g,
                    n: p as u32,
                });
                ops.push(Op::Aufruf { h, a: a as u32 });
                gewichte.push(pending + 1);
                pending = 0;
            }
            // ---- T2c: Bulk, Tabellen, Ref, call_indirect ---------------
            Befehl::MemoryInit(i) => {
                let h = scan.hoehe[pc];
                ops.push(Op::Bulk {
                    art: BulkArt::Abschnitt,
                    h,
                    a: *i,
                    b: 0,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::MemoryCopy { ziel, quelle } => {
                let h = scan.hoehe[pc];
                ops.push(Op::Bulk {
                    art: BulkArt::Kopie,
                    h,
                    a: *ziel,
                    b: *quelle,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::MemoryFill => {
                let h = scan.hoehe[pc];
                ops.push(Op::Bulk {
                    art: BulkArt::Fuellen,
                    h,
                    a: 0,
                    b: 0,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::DataDrop(i) => {
                let h = scan.hoehe[pc];
                ops.push(Op::Bulk {
                    art: BulkArt::DatenWeg,
                    h,
                    a: *i,
                    b: 0,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::TableGet(i) | Befehl::TableSet(i) | Befehl::TableSize(i)
            | Befehl::TableGrow(i) | Befehl::TableFill(i) => {
                let h = scan.hoehe[pc];
                let art = match &koerper[pc] {
                    Befehl::TableGet(_) => TabellenArt::Holen,
                    Befehl::TableSet(_) => TabellenArt::Setzen,
                    Befehl::TableSize(_) => TabellenArt::Anzahl,
                    Befehl::TableGrow(_) => TabellenArt::Wachsen,
                    _ => TabellenArt::Fuellen,
                };
                ops.push(Op::Tabelle {
                    art,
                    h,
                    a: *i,
                    b: 0,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::TableCopy { ziel, quelle } => {
                let h = scan.hoehe[pc];
                ops.push(Op::Tabelle {
                    art: TabellenArt::Kopieren,
                    h,
                    a: *ziel,
                    b: *quelle,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::TableInit { element, tabelle } => {
                let h = scan.hoehe[pc];
                ops.push(Op::Tabelle {
                    art: TabellenArt::Abschnitt,
                    h,
                    a: *element,
                    b: *tabelle,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::ElemDrop(i) => {
                let h = scan.hoehe[pc];
                ops.push(Op::Tabelle {
                    art: TabellenArt::ElementWeg,
                    h,
                    a: *i,
                    b: 0,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::CallIndirect { typ, tabelle } => {
                let h = scan.hoehe[pc];
                let i = indizierte.len();
                indizierte.push(IndirektWeg {
                    tabelle: *tabelle,
                    typ: *typ,
                });
                ops.push(Op::Indirekt { h, i: i as u32 });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::RefNull(_) => {
                // Beide Varianten (func/extern) sind roh 0 — Konstante.
                let h = scan.hoehe[pc];
                ops.push(Op::Konst32 { h, v: 0 });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::RefIsNull => {
                let h = scan.hoehe[pc];
                ops.push(Op::RefIstNull { h });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::RefFunc(f) => {
                let h = scan.hoehe[pc];
                ops.push(Op::RefFunktion { h, f: *f });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::RefAsNonNull => {
                let h = scan.hoehe[pc];
                ops.push(Op::RefNichtNull { h });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::Select | Befehl::SelectTyp(_) => {
                let h = scan.hoehe[pc];
                ops.push(Op::Wahl { h });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::LocalGet(i) => {
                // T6-Superbefehle (GEMESSEN: perf.rs::haeufigkeit —
                // `local.get` allein sind 33 %, gefolgt von Konst,
                // Rechnen und `local.set`): die vier Lasten bestehen
                // ueberwiegend aus geraden Linien, die MIT `local.get`
                // beginnen. Das Muster steckt komplett hier — die
                // folgenden Quellbefehle laufen ueber `skip`, ihre
                // vier Befehle zahlen als `pending + Anzahl` am Op mit.
                if let Some((weg, anzahl)) = muster(koerper, pc) {
                    skip = anzahl - 1;
                    let gewicht = pending + anzahl as u32;
                    match weg {
                        Muster::BrVergleich { l, c, art, t } => {
                            // Wie der `BrIf`-Arm, nur mit `h` VOR dem
                            // `br_if` (pc+3) und ohne Bedingungs-Op:
                            // die Hoehe ist dieselbe, weil das Muster
                            // netto 0 bringt (get+1 const+1 cmp-1 br-1).
                            let h = scan.hoehe[pc + 3];
                            let (mut ziel, mut k, mut ziel_pos) = (RAHMEN, KEIN, None);
                            match loese(&struktur, t, sprung)? {
                                Ziel::Rahmen => {}
                                Ziel::Stelle { src: z, a, nach } => {
                                    k = if a > 0 {
                                        let von = h.checked_sub(1)?.checked_sub(a)?;
                                        kopien.push(Kopie {
                                            von,
                                            nach,
                                            anzahl: a,
                                        });
                                        (kopien.len() - 1) as u32
                                    } else {
                                        KEIN
                                    };
                                    if z < len {
                                        ziel = 0;
                                        ziel_pos = Some(z);
                                    }
                                }
                            }
                            let b = vergleiche.len() as u32;
                            vergleiche.push(VergleichWeg {
                                lokal: l,
                                konst: c,
                                art,
                                k,
                            });
                            ops.push(Op::BrWennWahrVergleich { h, ziel, b });
                            gewichte.push(gewicht);
                            pending = 0;
                            if let Some(z) = ziel_pos {
                                patches.push(Patch::Op {
                                    op: ops.len() - 1,
                                    pos: z,
                                });
                            }
                        }
                        Muster::WennVergleich { l, c, art, bt } => {
                            // Wie der `If`-Arm: Schicht anlegen (pc+3),
                            // Sprung zum else/zum end bei FALSE.
                            let h = scan.hoehe[pc + 3];
                            let (p, e) = blocktypen(k, &bt);
                            let lh = h.checked_sub(1)?.checked_sub(p as u32)?;
                            struktur.push(Schicht {
                                start: pc + 3,
                                hoehe: lh,
                                p: p as u32,
                                e: e as u32,
                                schleife: false,
                            });
                            let (else_pos, end_pos) = *sprung.get(pc + 3)?.as_ref()?;
                            let ziel_pos = match else_pos {
                                Some(el) => el + 1,
                                None => end_pos,
                            };
                            let b = vergleiche.len() as u32;
                            vergleiche.push(VergleichWeg {
                                lokal: l,
                                konst: c,
                                art,
                                k: KEIN,
                            });
                            ops.push(Op::BrWennFalschVergleich { ziel: 0, b });
                            gewichte.push(gewicht);
                            pending = 0;
                            patches.push(Patch::Op {
                                op: ops.len() - 1,
                                pos: ziel_pos,
                            });
                        }
                        Muster::AddLokale { ziel, quel } => {
                            ops.push(Op::LokalAddLokal { ziel, quel });
                            gewichte.push(gewicht);
                            pending = 0;
                        }
                        Muster::AddKonst { ziel, konst } => {
                            ops.push(Op::LokalAddKonst { ziel, konst });
                            gewichte.push(gewicht);
                            pending = 0;
                        }
                        Muster::AddOben { ziel } => {
                            let h = scan.hoehe[pc];
                            ops.push(Op::LokalAddOben { ziel, h });
                            gewichte.push(gewicht);
                            pending = 0;
                        }
                        Muster::SubKonst { l, c } => {
                            let h = scan.hoehe[pc];
                            ops.push(Op::LokalSubKonst { l, c, h });
                            gewichte.push(gewicht);
                            pending = 0;
                        }
                    }
                } else {
                    ops.push(Op::LokalHolen {
                        h: scan.hoehe[pc],
                        i: *i,
                    });
                    gewichte.push(pending + 1);
                    pending = 0;
                }
            }
            Befehl::LocalSet(i) => {
                ops.push(Op::LokalSetzen {
                    h: scan.hoehe[pc],
                    i: *i,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::LocalTee(i) => {
                ops.push(Op::LokalHalten {
                    h: scan.hoehe[pc],
                    i: *i,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::I32Konst(v) => {
                ops.push(Op::Konst32 {
                    h: scan.hoehe[pc],
                    v: *v as u32,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::I64Konst(v) => {
                let w = werte.len();
                werte.push(*v as u64);
                ops.push(Op::Konst64 {
                    h: scan.hoehe[pc],
                    w: w as u32,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::F32Konst(b) => {
                let w = werte.len();
                werte.push(*b as u64);
                ops.push(Op::Konst64 {
                    h: scan.hoehe[pc],
                    w: w as u32,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::F64Konst(b) => {
                let w = werte.len();
                werte.push(*b);
                ops.push(Op::Konst64 {
                    h: scan.hoehe[pc],
                    w: w as u32,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if i32_art(b).is_some() => {
                let art = i32_art(b)?;
                ops.push(Op::I32 {
                    art,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if i64_art(b).is_some() => {
                let art = i64_art(b)?;
                ops.push(Op::I64 {
                    art,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if f32_art(b).is_some() => {
                let art = f32_art(b)?;
                ops.push(Op::F32 {
                    art,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if f64_art(b).is_some() => {
                let art = f64_art(b)?;
                ops.push(Op::F64 {
                    art,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if wandel_art(b).is_some() => {
                let art = wandel_art(b)?;
                ops.push(Op::Wandel {
                    art,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if lade_versatz(b).is_some() => {
                let (art, versatz) = lade_versatz(b)?;
                ops.push(Op::Lade {
                    art,
                    h: scan.hoehe[pc],
                    versatz,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            b if schreib_versatz(b).is_some() => {
                let (art, versatz) = schreib_versatz(b)?;
                ops.push(Op::Schreib {
                    art,
                    h: scan.hoehe[pc],
                    versatz,
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::MemorySize => {
                ops.push(Op::SpeicherGroesse {
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::MemoryGrow => {
                ops.push(Op::SpeicherWachs {
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::GlobalGet(i) => {
                ops.push(Op::GlobalHolen {
                    i: *i,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            Befehl::GlobalSet(i) => {
                ops.push(Op::GlobalSetzen {
                    i: *i,
                    h: scan.hoehe[pc],
                });
                gewichte.push(pending + 1);
                pending = 0;
            }
            _ => return None,
        }
    }

    if scan.flag_end {
        // Der lineare Pfad faellt hinter das schliessende `end` des
        // Koerpers: das schliesst den Funktionsrahmen. Sein Gewicht
        // traegt NUR die Struktur dahinter (das `end` selbst zaehlt
        // wie im alten Pfad, der Rahmenende-Schritt nicht).
        if scan.h_end < erg {
            return None;
        }
        ops.push(Op::Rueckkehr { h: scan.h_end });
        gewichte.push(pending);
    }

    // Sprungziele aufloesen: Quellplatz -> flacher Operationsindex.
    for p in patches {
        let pos = match &p {
            Patch::Op { pos, .. } | Patch::Tabelle { pos, .. } => *pos,
        };
        if pos >= len {
            return None;
        }
        let z = src[pos];
        if z == usize::MAX {
            return None; // Ziel nie emittiert (darf nicht vorkommen)
        }
        match p {
            Patch::Op { op, .. } => {
                let ziel = &mut ops[op];
                match ziel {
                    Op::BrWennWahr { ziel, .. }
                    | Op::BrWennFalsch { ziel, .. }
                    | Op::BrWennWahrVergleich { ziel, .. }
                    | Op::BrWennFalschVergleich { ziel, .. }
                    | Op::Sprung { ziel, .. }
                    | Op::Weiter { ziel } => *ziel = z as u32,
                    _ => return None,
                }
            }
            Patch::Tabelle { t, eintrag, .. } => {
                brtabellen[t][eintrag].ziel = z as u32;
            }
        }
    }

    Some(Programm {
        ops,
        gewichte,
        kopien,
        werte,
        aufrufe,
        vergleiche,
        brtabellen,
        indizierte,
        ergebnis_anzahl: typ.ergebnis.len(),
        feld: scan.feld,
        lokale: lokale.to_vec(),
        call_ziele: scan.call_ziele.clone(),
    })
}

// ===========================================================================
// Uebersetzen + Aufrufgraph
// ===========================================================================

/// Eine Funktion uebersetzen. `None` = nicht unterstuetzt.
fn uebersetze(
    k: &ModulKontext,
    typ: &FuncTyp,
    koerper: &[Befehl],
    lokale: &[Valtyp],
    lauf: &Lauf,
) -> Option<Programm> {
    if koerper.is_empty() {
        return None;
    }
    let erg = typ.ergebnis.len() as u32;
    let sprung = baue_sprung(koerper);
    let scan = scan(k, koerper, &sprung, erg, lauf)?;
    emit(k, koerper, typ, lokale, &sprung, &scan, lauf)
}

/// Den Aufrufgraphen von `idx` tiefen-first erschliessen.
///
/// Zyklusregel: `Laeuft` wird optimistisch wahr (sonst faellt jede
/// Rekursion durch — fib wuerde immer auf den alten Pfad). Schlaegt
/// der Gesamtlauf fehl, setzt `kompiziere` ALLE in diesem Lauf
/// optimistisch als Okay markierten Funktionen auf `Neu` zurueck.
/// `Aus` bleibt stehen: es wurzelt immer in einem dauerhaft nicht
/// unterstuetzten Koerper (der Koerper und sein Kontext sind statisch).
fn dfs(lauf: &mut Lauf, idx: u32, beruehrt: &mut Vec<u32>) -> bool {
    let i = idx as usize;
    while lauf.store.schnell.len() <= i {
        lauf.store.schnell.push(Status::Neu);
    }
    match &lauf.store.schnell[i] {
        Status::Okay(_) => return true,
        Status::Aus => return false,
        Status::Laeuft => return true,
        Status::Neu => {}
    }
    beruehrt.push(idx);
    lauf.store.schnell[i] = Status::Laeuft;
    let Some(k) = lauf
        .store
        .funktion_kontext
        .get(i)
        .and_then(|s| s.as_ref())
        .map(Arc::clone)
    else {
        lauf.store.schnell[i] = Status::Aus;
        return false;
    };
    let Some(f) = lauf.store.funktionen.get(i).cloned() else {
        lauf.store.schnell[i] = Status::Aus;
        return false;
    };
    let programm = match f.as_ref() {
        Funktion::Eigen {
            typ,
            koerper,
            lokale,
            ..
        } => uebersetze(&k, typ, koerper, lokale, lauf),
        Funktion::Fremd { .. } => None,
    };
    match programm {
        None => {
            lauf.store.schnell[i] = Status::Aus;
            false
        }
        Some(p) => {
            let ziele = p.call_ziele.clone();
            let mut ok = true;
            for z in ziele {
                if !dfs(lauf, z, beruehrt) {
                    ok = false;
                    break;
                }
            }
            if ok {
                lauf.store.schnell[i] = Status::Okay(Arc::new(p));
                true
            } else {
                lauf.store.schnell[i] = Status::Aus;
                false
            }
        }
    }
}

/// Programm fuer `idx` beschaffen (Cache erst beim ersten Aufruf).
/// `None` = der ganze Aufrufgraph ist nicht unterstuetzt -> Rueckfall.
pub fn kompiziere(lauf: &mut Lauf, idx: u32) -> Option<Arc<Programm>> {
    let i = idx as usize;
    while lauf.store.schnell.len() <= i {
        lauf.store.schnell.push(Status::Neu);
    }
    match &lauf.store.schnell[i] {
        Status::Okay(p) => return Some(p.clone()),
        Status::Aus => return None,
        Status::Neu | Status::Laeuft => {}
    }
    let mut beruehrt: Vec<u32> = Vec::new();
    if !dfs(lauf, idx, &mut beruehrt) {
        // Ruecksetzen: Nicht-bestaetigte Okay-Marken duerfen nicht
        // stehen bleiben (ihre Aufrufer waeren sonst auf Programme
        // fixiert, deren Aufrufziel unabhaengig neu bewertet wurde).
        for &t in &beruehrt {
            let t = t as usize;
            if matches!(lauf.store.schnell[t], Status::Okay(_) | Status::Laeuft) {
                lauf.store.schnell[t] = Status::Neu;
            }
        }
        return None;
    }
    match &lauf.store.schnell[i] {
        Status::Okay(p) => Some(p.clone()),
        _ => None,
    }
}

// ===========================================================================
// Einhängung
// ===========================================================================

/// Der Einhaengpunkt aus `interp::aufrufen` (Eigen-Arm, nur ohne
/// Haltepunkt/Spur). `None` = Rueckfall auf den alten Pfad.
///
/// Reihenfolge wie `erster_rahmen` + `aufrufen`: Argumentpruefung,
/// dann Treibstoff-Tiefe, dann Brennstoff, dann `zaehle(1)` — EXAKT
/// dieselben Fehler in derselben Reihenfolge.
pub fn versuche(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Option<Result<Vec<Wert>, Fehler>> {
    let (typ, _koerper, lokale) = match funktion.as_ref() {
        Funktion::Eigen {
            typ,
            koerper,
            lokale,
            ..
        } => (typ, koerper, lokale),
        Funktion::Fremd { .. } => return None,
    };
    if args.len() != typ.parameter.len() {
        return Some(Err(Fehler::link(
            L_SIGNATUR,
            format!(
                "Funktion {eintr}: {} Parameter erwartet, {}",
                typ.parameter.len(),
                args.len()
            ),
        )));
    }
    let programm = kompiziere(lauf, eintr)?;
    // Modulkontext des EINSTIEGS: Speicher- und Globalindexraum fuer
    // die Op's. Fehlt er (nur wenn), faellt alles auf den alten Pfad —
    // `kompiziere` hat denselben Eintrag ja schon benoetigt.
    let k = lauf
        .store
        .funktion_kontext
        .get(eintr as usize)
        .and_then(|s| s.as_ref())
        .map(Arc::clone)?;
    if let Err(e) = pruefe_tiefe(lauf, 1) {
        return Some(Err(e));
    }
    if let Err(e) = lauf.pruefe_brennstoff() {
        return Some(Err(e));
    }
    lauf.zaehle(1);
    Some(fuehre_aus(lauf, host, programm, typ, lokale, args, k))
}

/// Aufruftiefe pruefen wie `interp::pruefe_tiefe` (gleiche Meldung).
fn pruefe_tiefe(lauf: &Lauf, neu: usize) -> Result<(), Fehler> {
    if neu > lauf.grenzen.aufruftiefe {
        return Err(Fehler::trap(
            T_AUFRUFTIEFE,
            format!(
                "Aufruftiefe {neu} ueber der Grenze {}",
                lauf.grenzen.aufruftiefe
            ),
        ));
    }
    Ok(())
}

// ===========================================================================
// Ausfuehrung
// ===========================================================================

/// Ein Rueckkehrplatz (der Aufrufer liegt nicht mehr im Haupt-Loop,
/// seine Register werden hier gesichert — der Wertestapel ist ja
/// ohnehin gemeinsam).
struct Rahmen {
    lokal: usize,
    basis: usize,
    pc: usize,
    oben: Arc<Programm>,
    /// Modulkontext dieses Rahmens (Speicher- und Globalindexraum) —
    /// `Rc`, kein Klon der Vecs: pro Aufruf ein Inkrement statt Allok.
    k: Arc<ModulKontext>,
}

struct Ausfuehrung {
    stapel: Vec<u64>,
    rahmen: Vec<Rahmen>,
    lokal: usize,
    basis: usize,
    pc: usize,
    oben: Arc<Programm>,
    /// Modulkontext des LAUFENDEN Rahmens.
    k: Arc<ModulKontext>,
    /// Wiederverwendeter Puffer fuer Host-Argumente (KEIN Alloc je
    /// Hostaufruf — die alte Ausfuehrung slice-te auch ohne Alloc).
    host_args: Vec<Wert>,
}

fn fuehre_aus(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    programm: Arc<Programm>,
    typ: &FuncTyp,
    lokale: &[Valtyp],
    args: &[Wert],
    k: Arc<ModulKontext>,
) -> Result<Vec<Wert>, Fehler> {
    let mut a = Ausfuehrung {
        stapel: Vec::with_capacity(64 + args.len() + lokale.len()),
        rahmen: Vec::with_capacity(8),
        lokal: 0,
        basis: 0,
        pc: 0,
        oben: programm,
        k,
        host_args: Vec::new(),
    };
    for x in args {
        a.stapel.push(kod(*x));
    }
    for t in lokale {
        a.stapel.push(kod(Wert::standard(*t)));
    }
    a.basis = a.stapel.len();
    a.stapel.resize(a.basis + a.oben.feld, 0);
    a.laeuft(lauf, host, &typ.ergebnis)
}

impl Ausfuehrung {
    /// EINE Schleife, EIN Match — kein `Result` je Befehl, Treibstoff
    /// nur an Spruengen/Aufrufen wie im alten Pfad.
    fn laeuft(
        &mut self,
        lauf: &mut Lauf,
        host: &mut dyn Host,
        ergebnis: &[Valtyp],
    ) -> Result<Vec<Wert>, Fehler> {
        loop {
            let Some(op) = self.oben.ops.get(self.pc).copied() else {
                return Err(Fehler::trap(
                    T_DIVERSE,
                    "schnell: Programmlauf ohne Rueckkehr",
                ));
            };
            lauf.zaehle(self.oben.gewichte[self.pc] as u64);
            self.pc += 1;
            match op {
                Op::Nichts => {}
                Op::Konst32 { h, v } => self.stapel[self.basis + h as usize] = v as u64,
                Op::Konst64 { h, w } => {
                    self.stapel[self.basis + h as usize] = self.oben.werte[w as usize];
                }
                Op::LokalHolen { h, i } => {
                    let w = self.stapel[self.lokal + i as usize];
                    self.stapel[self.basis + h as usize] = w;
                }
                Op::LokalSetzen { h, i } => {
                    let w = self.stapel[self.basis + h as usize - 1];
                    self.stapel[self.lokal + i as usize] = w;
                }
                Op::LokalHalten { h, i } => {
                    let w = self.stapel[self.basis + h as usize - 1];
                    self.stapel[self.lokal + i as usize] = w;
                }
                // T6-Superbefehle: die vier Rechnen-Muster lesen und
                // schreiben NUR Locals (bzw. den einen Kernwert) — der
                // Operandenstapel bleibt unberuehrt, die Hoehe steckt
                // wie immer in den folgenden Ops.
                Op::LokalAddLokal { ziel, quel } => {
                    let a = self.stapel[self.lokal + ziel as usize] as u32;
                    let b = self.stapel[self.lokal + quel as usize] as u32;
                    self.stapel[self.lokal + ziel as usize] = a.wrapping_add(b) as u64;
                }
                Op::LokalAddKonst { ziel, konst } => {
                    let a = self.stapel[self.lokal + ziel as usize] as u32;
                    self.stapel[self.lokal + ziel as usize] = a.wrapping_add(konst) as u64;
                }
                Op::LokalAddOben { ziel, h } => {
                    // Das Muster hat das lokale Feld (bei h) zuletzt
                    // gepusht, dann `add` und `set` verbraucht — der
                    // Kernwert lag bei h-1 und faellt wie gehabt weg.
                    let a = self.stapel[self.lokal + ziel as usize] as u32;
                    let x = self.stapel[self.basis + h as usize - 1] as u32;
                    self.stapel[self.lokal + ziel as usize] = a.wrapping_add(x) as u64;
                }
                Op::LokalSubKonst { l, c, h } => {
                    let a = self.stapel[self.lokal + l as usize] as u32;
                    self.stapel[self.basis + h as usize] = a.wrapping_sub(c) as u64;
                }
                Op::I32 { art, h } => {
                    use I32Art as A;
                    let b = self.basis + h as usize;
                    macro_rules! bin_u {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1] as u32;
                            let av = self.stapel[b - 2] as u32;
                            #[allow(clippy::redundant_closure_call)]
                            let w: u32 = ($f)(av, bv);
                            self.stapel[b - 2] = w as u64;
                        }};
                    }
                    macro_rules! cmp_u {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1] as u32;
                            let av = self.stapel[b - 2] as u32;
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(av, bv);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    macro_rules! cmp_s {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1] as u32 as i32;
                            let av = self.stapel[b - 2] as u32 as i32;
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(av, bv);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    macro_rules! eins_i {
                        ($f:expr) => {{
                            let av = self.stapel[b - 1] as u32 as i32;
                            #[allow(clippy::redundant_closure_call)]
                            let w: i32 = ($f)(av);
                            self.stapel[b - 1] = w as u32 as u64;
                        }};
                    }
                    match art {
                        A::Eqz => eins_i!(|a: i32| i32::from(a == 0)),
                        A::Eq => cmp_u!(|a: u32, b: u32| a == b),
                        A::Ne => cmp_u!(|a: u32, b: u32| a != b),
                        A::LtS => cmp_s!(|a: i32, b: i32| a < b),
                        A::LtU => cmp_u!(|a: u32, b: u32| a < b),
                        A::GtS => cmp_s!(|a: i32, b: i32| a > b),
                        A::GtU => cmp_u!(|a: u32, b: u32| a > b),
                        A::LeS => cmp_s!(|a: i32, b: i32| a <= b),
                        A::LeU => cmp_u!(|a: u32, b: u32| a <= b),
                        A::GeS => cmp_s!(|a: i32, b: i32| a >= b),
                        A::GeU => cmp_u!(|a: u32, b: u32| a >= b),
                        A::Clz => eins_i!(|a: i32| a.leading_zeros() as i32),
                        A::Ctz => eins_i!(|a: i32| a.trailing_zeros() as i32),
                        A::Popcnt => eins_i!(|a: i32| a.count_ones() as i32),
                        A::Add => bin_u!(|a: u32, b: u32| a.wrapping_add(b)),
                        A::Sub => bin_u!(|a: u32, b: u32| a.wrapping_sub(b)),
                        A::Mul => bin_u!(|a: u32, b: u32| a.wrapping_mul(b)),
                        A::DivS => {
                            let bv = self.stapel[b - 1] as u32 as i32;
                            let av = self.stapel[b - 2] as u32 as i32;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            if av == i32::MIN && bv == -1 {
                                return Err(Fehler::trap(
                                    T_OVERFLOW,
                                    "integer overflow (i32.div_s)",
                                ));
                            }
                            self.stapel[b - 2] = av.wrapping_div(bv) as u32 as u64;
                        }
                        A::DivU => {
                            let bv = self.stapel[b - 1] as u32;
                            let av = self.stapel[b - 2] as u32;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = (av / bv) as u64;
                        }
                        A::RemS => {
                            let bv = self.stapel[b - 1] as u32 as i32;
                            let av = self.stapel[b - 2] as u32 as i32;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = av.wrapping_rem(bv) as u32 as u64;
                        }
                        A::RemU => {
                            let bv = self.stapel[b - 1] as u32;
                            let av = self.stapel[b - 2] as u32;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = (av % bv) as u64;
                        }
                        A::And => bin_u!(|a: u32, b: u32| a & b),
                        A::Or => bin_u!(|a: u32, b: u32| a | b),
                        A::Xor => bin_u!(|a: u32, b: u32| a ^ b),
                        A::Shl => bin_u!(|a: u32, b: u32| a.wrapping_shl(b & 31)),
                        A::ShrU => bin_u!(|a: u32, b: u32| a.wrapping_shr(b & 31)),
                        A::Rotl => bin_u!(|a: u32, b: u32| a.rotate_left(b & 31)),
                        A::Rotr => bin_u!(|a: u32, b: u32| a.rotate_right(b & 31)),
                        A::ShrS => {
                            let bv = (self.stapel[b - 1] as u32) & 31;
                            let av = self.stapel[b - 2] as u32 as i32;
                            self.stapel[b - 2] = av.wrapping_shr(bv) as u32 as u64;
                        }
                    }
                }
                Op::I64 { art, h } => {
                    use I64Art as A;
                    let b = self.basis + h as usize;
                    macro_rules! bin64 {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1];
                            let av = self.stapel[b - 2];
                            #[allow(clippy::redundant_closure_call)]
                            let w: u64 = ($f)(av, bv);
                            self.stapel[b - 2] = w;
                        }};
                    }
                    macro_rules! cmp64u {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1];
                            let av = self.stapel[b - 2];
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(av, bv);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    macro_rules! cmp64s {
                        ($f:expr) => {{
                            let bv = self.stapel[b - 1] as i64;
                            let av = self.stapel[b - 2] as i64;
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(av, bv);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    match art {
                        A::Eqz => {
                            self.stapel[b - 1] = u64::from(self.stapel[b - 1] as i64 == 0)
                        }
                        A::Clz => {
                            self.stapel[b - 1] = (self.stapel[b - 1] as i64).leading_zeros() as u64
                        }
                        A::Ctz => {
                            self.stapel[b - 1] = (self.stapel[b - 1] as i64).trailing_zeros() as u64
                        }
                        A::Popcnt => {
                            self.stapel[b - 1] = (self.stapel[b - 1] as i64).count_ones() as u64
                        }
                        A::Eq => cmp64u!(|a: u64, b: u64| a == b),
                        A::Ne => cmp64u!(|a: u64, b: u64| a != b),
                        A::LtS => cmp64s!(|a: i64, b: i64| a < b),
                        A::LtU => cmp64u!(|a: u64, b: u64| a < b),
                        A::GtS => cmp64s!(|a: i64, b: i64| a > b),
                        A::GtU => cmp64u!(|a: u64, b: u64| a > b),
                        A::LeS => cmp64s!(|a: i64, b: i64| a <= b),
                        A::LeU => cmp64u!(|a: u64, b: u64| a <= b),
                        A::GeS => cmp64s!(|a: i64, b: i64| a >= b),
                        A::GeU => cmp64u!(|a: u64, b: u64| a >= b),
                        A::Add => bin64!(|a: u64, b: u64| a.wrapping_add(b)),
                        A::Sub => bin64!(|a: u64, b: u64| a.wrapping_sub(b)),
                        A::Mul => bin64!(|a: u64, b: u64| a.wrapping_mul(b)),
                        A::DivS => {
                            let bv = self.stapel[b - 1] as i64;
                            let av = self.stapel[b - 2] as i64;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            if av == i64::MIN && bv == -1 {
                                return Err(Fehler::trap(
                                    T_OVERFLOW,
                                    "integer overflow (i64.div_s)",
                                ));
                            }
                            self.stapel[b - 2] = av.wrapping_div(bv) as u64;
                        }
                        A::DivU => {
                            let bv = self.stapel[b - 1];
                            let av = self.stapel[b - 2];
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = av / bv;
                        }
                        A::RemS => {
                            let bv = self.stapel[b - 1] as i64;
                            let av = self.stapel[b - 2] as i64;
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = av.wrapping_rem(bv) as u64;
                        }
                        A::RemU => {
                            let bv = self.stapel[b - 1];
                            let av = self.stapel[b - 2];
                            if bv == 0 {
                                return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
                            }
                            self.stapel[b - 2] = av % bv;
                        }
                        A::And => bin64!(|a: u64, b: u64| a & b),
                        A::Or => bin64!(|a: u64, b: u64| a | b),
                        A::Xor => bin64!(|a: u64, b: u64| a ^ b),
                        A::Shl => bin64!(|a: u64, b: u64| a.wrapping_shl((b & 63) as u32)),
                        A::ShrU => bin64!(|a: u64, b: u64| a.wrapping_shr((b & 63) as u32)),
                        A::Rotl => bin64!(|a: u64, b: u64| a.rotate_left((b & 63) as u32)),
                        A::Rotr => bin64!(|a: u64, b: u64| a.rotate_right((b & 63) as u32)),
                        A::ShrS => {
                            let bv = (self.stapel[b - 1] & 63) as u32;
                            let av = self.stapel[b - 2] as i64;
                            self.stapel[b - 2] = av.wrapping_shr(bv) as u64;
                        }
                    }
                }
                Op::F32 { art, h } => {
                    use F32Art as A;
                    let b = self.basis + h as usize;
                    // Kanonisierung EXAKT wie `f32_un`/`f32_bi` im
                    // alten Pfad: nur das ERGEBNIS geht durch `nan32`.
                    macro_rules! v32 {
                        ($f:expr) => {{
                            let e = f32::from_bits(self.stapel[b - 1] as u32);
                            #[allow(clippy::redundant_closure_call)]
                            let r: f32 = ($f)(e);
                            self.stapel[b - 1] = if r.is_nan() {
                                nan32() as u64
                            } else {
                                r.to_bits() as u64
                            };
                        }};
                    }
                    macro_rules! w32 {
                        ($f:expr) => {{
                            let eb = f32::from_bits(self.stapel[b - 1] as u32);
                            let ea = f32::from_bits(self.stapel[b - 2] as u32);
                            #[allow(clippy::redundant_closure_call)]
                            let r: f32 = ($f)(ea, eb);
                            self.stapel[b - 2] = if r.is_nan() {
                                nan32() as u64
                            } else {
                                r.to_bits() as u64
                            };
                        }};
                    }
                    macro_rules! c32 {
                        ($f:expr) => {{
                            let eb = f32::from_bits(self.stapel[b - 1] as u32);
                            let ea = f32::from_bits(self.stapel[b - 2] as u32);
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(ea, eb);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    match art {
                        A::Eq => c32!(|a: f32, b: f32| a == b),
                        A::Ne => c32!(|a: f32, b: f32| a != b),
                        A::Lt => c32!(|a: f32, b: f32| a < b),
                        A::Gt => c32!(|a: f32, b: f32| a > b),
                        A::Le => c32!(|a: f32, b: f32| a <= b),
                        A::Ge => c32!(|a: f32, b: f32| a >= b),
                        A::Abs => self.stapel[b - 1] &= 0x7fff_ffff,
                        A::Neg => self.stapel[b - 1] ^= 0x8000_0000,
                        A::Ceil => v32!(|a: f32| a.ceil()),
                        A::Floor => v32!(|a: f32| a.floor()),
                        A::Trunc => v32!(|a: f32| a.trunc()),
                        A::Nearest => v32!(|a: f32| nearest32(a)),
                        A::Sqrt => v32!(|a: f32| a.sqrt()),
                        A::Add => w32!(|a: f32, b: f32| a + b),
                        A::Sub => w32!(|a: f32, b: f32| a - b),
                        A::Mul => w32!(|a: f32, b: f32| a * b),
                        A::Div => w32!(|a: f32, b: f32| a / b),
                        A::Min => w32!(wasm_min32),
                        A::Max => w32!(wasm_max32),
                        A::Copysign => {
                            let bv = self.stapel[b - 1] as u32;
                            let av = self.stapel[b - 2] as u32;
                            self.stapel[b - 2] =
                                ((av & 0x7fff_ffff) | (bv & 0x8000_0000)) as u64;
                        }
                    }
                }
                Op::F64 { art, h } => {
                    use F64Art as A;
                    let b = self.basis + h as usize;
                    macro_rules! v64 {
                        ($f:expr) => {{
                            let e = f64::from_bits(self.stapel[b - 1]);
                            #[allow(clippy::redundant_closure_call)]
                            let r: f64 = ($f)(e);
                            self.stapel[b - 1] = if r.is_nan() { nan64() } else { r.to_bits() };
                        }};
                    }
                    macro_rules! w64 {
                        ($f:expr) => {{
                            let eb = f64::from_bits(self.stapel[b - 1]);
                            let ea = f64::from_bits(self.stapel[b - 2]);
                            #[allow(clippy::redundant_closure_call)]
                            let r: f64 = ($f)(ea, eb);
                            self.stapel[b - 2] = if r.is_nan() { nan64() } else { r.to_bits() };
                        }};
                    }
                    macro_rules! c64 {
                        ($f:expr) => {{
                            let eb = f64::from_bits(self.stapel[b - 1]);
                            let ea = f64::from_bits(self.stapel[b - 2]);
                            #[allow(clippy::redundant_closure_call)]
                            let w: bool = ($f)(ea, eb);
                            self.stapel[b - 2] = u64::from(w);
                        }};
                    }
                    match art {
                        A::Eq => c64!(|a: f64, b: f64| a == b),
                        A::Ne => c64!(|a: f64, b: f64| a != b),
                        A::Lt => c64!(|a: f64, b: f64| a < b),
                        A::Gt => c64!(|a: f64, b: f64| a > b),
                        A::Le => c64!(|a: f64, b: f64| a <= b),
                        A::Ge => c64!(|a: f64, b: f64| a >= b),
                        A::Abs => self.stapel[b - 1] &= 0x7fff_ffff_ffff_ffff,
                        A::Neg => self.stapel[b - 1] ^= 0x8000_0000_0000_0000,
                        A::Ceil => v64!(|a: f64| a.ceil()),
                        A::Floor => v64!(|a: f64| a.floor()),
                        A::Trunc => v64!(|a: f64| a.trunc()),
                        A::Nearest => v64!(|a: f64| nearest(a)),
                        A::Sqrt => v64!(|a: f64| a.sqrt()),
                        A::Add => w64!(|a: f64, b: f64| a + b),
                        A::Sub => w64!(|a: f64, b: f64| a - b),
                        A::Mul => w64!(|a: f64, b: f64| a * b),
                        A::Div => w64!(|a: f64, b: f64| a / b),
                        A::Min => w64!(wasm_min64),
                        A::Max => w64!(wasm_max64),
                        A::Copysign => {
                            let bv = self.stapel[b - 1];
                            let av = self.stapel[b - 2];
                            self.stapel[b - 2] =
                                (av & 0x7fff_ffff_ffff_ffff) | (bv & 0x8000_0000_0000_0000);
                        }
                    }
                }
                Op::Wandel { art, h } => {
                    use WandelArt as W;
                    let b = self.basis + h as usize;
                    let v = self.stapel[b - 1];
                    let neu = match art {
                        W::Wrap => v as u32 as u64,
                        W::ExtendI32S => v as u32 as i32 as i64 as u64,
                        W::ExtendI328S => (v as u32 as i8 as i32) as u32 as u64,
                        W::ExtendI3216S => (v as u32 as i16 as i32) as u32 as u64,
                        W::ExtendI648S => v as i64 as i8 as i64 as u64,
                        W::ExtendI6416S => v as i64 as i16 as i64 as u64,
                        W::ExtendI6432S => v as i32 as i64 as u64,
                        W::TruncI32F32S => {
                            trunk(f32::from_bits(v as u32) as f64, 32, true)? as u32 as u64
                        }
                        W::TruncI32F32U => {
                            trunk(f32::from_bits(v as u32) as f64, 32, false)? as u32 as u64
                        }
                        W::TruncI32F64S => trunk(f64::from_bits(v), 32, true)? as u32 as u64,
                        W::TruncI32F64U => trunk(f64::from_bits(v), 32, false)? as u32 as u64,
                        W::TruncI64F32S => {
                            trunk(f32::from_bits(v as u32) as f64, 64, true)? as u64
                        }
                        W::TruncI64F32U => {
                            trunk(f32::from_bits(v as u32) as f64, 64, false)? as u64
                        }
                        W::TruncI64F64S => trunk(f64::from_bits(v), 64, true)? as u64,
                        W::TruncI64F64U => trunk(f64::from_bits(v), 64, false)? as u64,
                        W::SatI32F32S => {
                            trunk_sat(f32::from_bits(v as u32) as f64, 32, true) as u32 as u64
                        }
                        W::SatI32F32U => {
                            trunk_sat(f32::from_bits(v as u32) as f64, 32, false) as u32 as u64
                        }
                        W::SatI32F64S => trunk_sat(f64::from_bits(v), 32, true) as u32 as u64,
                        W::SatI32F64U => trunk_sat(f64::from_bits(v), 32, false) as u32 as u64,
                        W::SatI64F32S => trunk_sat(f32::from_bits(v as u32) as f64, 64, true) as u64,
                        W::SatI64F32U => {
                            trunk_sat(f32::from_bits(v as u32) as f64, 64, false) as u64
                        }
                        W::SatI64F64S => trunk_sat(f64::from_bits(v), 64, true) as u64,
                        W::SatI64F64U => trunk_sat(f64::from_bits(v), 64, false) as u64,
                        W::ConvF32I32S => (v as u32 as i32 as f32).to_bits() as u64,
                        W::ConvF32I32U => (v as u32 as f32).to_bits() as u64,
                        W::ConvF32I64S => (v as i64 as f32).to_bits() as u64,
                        W::ConvF32I64U => (v as f32).to_bits() as u64,
                        W::ConvF64I32S => (v as u32 as i32 as f64).to_bits(),
                        W::ConvF64I32U => (v as u32 as f64).to_bits(),
                        W::ConvF64I64S => (v as i64 as f64).to_bits(),
                        W::ConvF64I64U => (v as f64).to_bits(),
                        W::Demote => {
                            let r = f64::from_bits(v) as f32;
                            let bw = r.to_bits();
                            if ist_nan32(bw) { nan32() as u64 } else { bw as u64 }
                        }
                        W::Promote => {
                            let r = f32::from_bits(v as u32) as f64;
                            let bw = r.to_bits();
                            if ist_nan64(bw) { nan64() } else { bw }
                        }
                    };
                    self.stapel[b - 1] = neu;
                }
                Op::Lade { art, h, versatz } => {
                    use LadeArt as A;
                    let b = self.basis + h as usize;
                    // Adresse wie alter Pfad: i32 roh, Versatz in
                    // u64-RECHNUNG (ueberlauf-sicher wie `wrapping_add`).
                    let adresse =
                        (self.stapel[b - 1] as u32 as u64).wrapping_add(versatz as u64);
                    let sp = speicher_von(lauf, self.k.speicher.first().copied())?;
                    let roh = sp.lies(adresse, art.bytes())?;
                    let wert: u64 = match art {
                        A::I32 | A::I32S8 | A::I32U8 | A::I32S16 | A::I32U16 => {
                            let mut v: u32 = 0;
                            for (i, x) in roh.iter().enumerate() {
                                v |= (*x as u32) << (8 * i);
                            }
                            let x: i32 = match art {
                                A::I32S8 => v as u8 as i8 as i32,
                                A::I32S16 => v as u16 as i16 as i32,
                                _ => v as i32,
                            };
                            x as u32 as u64
                        }
                        A::I64 | A::I64S8 | A::I64U8 | A::I64S16 | A::I64U16 | A::I64S32
                        | A::I64U32 => {
                            let mut v: u64 = 0;
                            for (i, x) in roh.iter().enumerate() {
                                v |= (*x as u64) << (8 * i);
                            }
                            let x: i64 = match art {
                                A::I64S8 => v as u8 as i8 as i64,
                                A::I64S16 => v as u16 as i16 as i64,
                                A::I64S32 => v as u32 as i32 as i64,
                                _ => v as i64,
                            };
                            x as u64
                        }
                        A::F32 => {
                            let mut v: u32 = 0;
                            for (i, x) in roh.iter().enumerate() {
                                v |= (*x as u32) << (8 * i);
                            }
                            v as u64
                        }
                        A::F64 => {
                            let mut v: u64 = 0;
                            for (i, x) in roh.iter().enumerate() {
                                v |= (*x as u64) << (8 * i);
                            }
                            v
                        }
                    };
                    self.stapel[b - 1] = wert;
                }
                Op::Schreib { art, h, versatz } => {
                    use SchreibArt as S;
                    let b = self.basis + h as usize;
                    let wert = self.stapel[b - 1];
                    let adresse =
                        (self.stapel[b - 2] as u32 as u64).wrapping_add(versatz as u64);
                    let mut roh = [0u8; 8];
                    match art {
                        S::I32 | S::I32B8 | S::I32B16 | S::F32 => {
                            roh[..4].copy_from_slice(&(wert as u32).to_le_bytes());
                        }
                        S::I64 | S::I64B8 | S::I64B16 | S::I64B32 | S::F64 => {
                            roh.copy_from_slice(&wert.to_le_bytes());
                        }
                    }
                    speicher_mut(lauf, self.k.speicher.first().copied())?
                        .schreib(adresse, &roh[..art.bytes()])?;
                }
                Op::SpeicherGroesse { h } => {
                    let b = self.basis + h as usize;
                    let sp = speicher_von(lauf, self.k.speicher.first().copied())?;
                    self.stapel[b] = sp.seiten() as u32 as u64;
                }
                Op::SpeicherWachs { h } => {
                    let b = self.basis + h as usize;
                    let delta = self.stapel[b - 1] as u32 as u64;
                    let alt =
                        speicher_mut(lauf, self.k.speicher.first().copied())?.waechst(delta);
                    self.stapel[b - 1] = alt as u32 as u64;
                }
                Op::GlobalHolen { i, h } => {
                    let b = self.basis + h as usize;
                    let gi = self.k.globale_index.get(i as usize).copied().ok_or_else(|| {
                        Fehler::trap(
                            T_DIVERSE,
                            format!(
                                "global.get {i}: ausserhalb ({} globale Variablen)",
                                self.k.globale_index.len()
                            ),
                        )
                    })?;
                    let w = lauf.zelle(gi).copied().ok_or_else(|| {
                        Fehler::trap(T_DIVERSE, format!("global.get {i}: ausserhalb"))
                    })?;
                    self.stapel[b] = kod(w);
                }
                Op::GlobalSetzen { i, h } => {
                    let b = self.basis + h as usize;
                    let roh = self.stapel[b - 1];
                    let gi = self.k.globale_index.get(i as usize).copied().ok_or_else(|| {
                        Fehler::trap(
                            T_DIVERSE,
                            format!(
                                "global.set {i}: ausserhalb ({} globale Variablen)",
                                self.k.globale_index.len()
                            ),
                        )
                    })?;
                    let zelle = lauf.zelle_mut(gi).ok_or_else(|| {
                        Fehler::trap(T_DIVERSE, format!("global.set {i}: ausserhalb"))
                    })?;
                    let neu = dek_wie(zelle, roh);
                    *zelle = neu;
                }
                Op::Wahl { h } => {
                    let b = self.basis + h as usize;
                    let bed = self.stapel[b - 1];
                    let wert_b = self.stapel[b - 2];
                    let wert_a = self.stapel[b - 3];
                    self.stapel[b - 3] = if bed != 0 { wert_a } else { wert_b };
                }
                Op::BrWennWahr { h, ziel, k } => {
                    if self.stapel[self.basis + h as usize - 1] as u32 != 0 {
                        lauf.pruefe_brennstoff()?;
                        if ziel == RAHMEN {
                            // Die Bedingung lag bei h-1 und IST gefallen —
                            // der alte Pfad poppt SIE, ehe er den Rahmen
                            // beendet. Ohne Kopie sind nur h-1 Werte da
                            // (GEMESSES: func.wast `break-br_if-num`).
                            let fh = if k == KEIN {
                                h.saturating_sub(1) as usize
                            } else {
                                self.anwenden(k, h)
                            };
                            if let Some(w) = self.rahmen_ende(fh, ergebnis)? {
                                return Ok(w);
                            }
                        } else {
                            self.anwenden(k, h);
                            self.pc = ziel as usize;
                        }
                    }
                }
                Op::BrWennFalsch { h, ziel } => {
                    if self.stapel[self.basis + h as usize - 1] as u32 == 0 {
                        self.pc = ziel as usize;
                    }
                }
                Op::BrWennWahrVergleich { h, ziel, b } => {
                    let weg = self.oben.vergleiche[b as usize];
                    let x = self.stapel[self.lokal + weg.lokal as usize] as u32;
                    if vergleicht(x, weg.konst, weg.art) {
                        lauf.pruefe_brennstoff()?;
                        if ziel == RAHMEN {
                            let fh = if weg.k == KEIN {
                                h.saturating_sub(1) as usize
                            } else {
                                self.anwenden(weg.k, h)
                            };
                            if let Some(w) = self.rahmen_ende(fh, ergebnis)? {
                                return Ok(w);
                            }
                        } else {
                            self.anwenden(weg.k, h);
                            self.pc = ziel as usize;
                        }
                    }
                }
                Op::BrWennFalschVergleich { ziel, b } => {
                    let weg = self.oben.vergleiche[b as usize];
                    let x = self.stapel[self.lokal + weg.lokal as usize] as u32;
                    if !vergleicht(x, weg.konst, weg.art) {
                        self.pc = ziel as usize;
                    }
                }
                Op::Sprung { h, ziel, k } => {
                    lauf.pruefe_brennstoff()?;
                    let fh = self.anwenden(k, h);
                    if ziel == RAHMEN {
                        if let Some(w) = self.rahmen_ende(fh, ergebnis)? {
                            return Ok(w);
                        }
                    } else {
                        self.pc = ziel as usize;
                    }
                }
                Op::Weiter { ziel } => self.pc = ziel as usize,
                Op::BrTabelle { h, t } => {
                    let idx = self.stapel[self.basis + h as usize - 1] as u32 as usize;
                    lauf.pruefe_brennstoff()?;
                    let e = {
                        let tab = &self.oben.brtabellen[t as usize];
                        let n = tab.len() - 1;
                        if idx < n {
                            tab[idx]
                        } else {
                            tab[n]
                        }
                    };
                    let fh = if e.k == KEIN {
                        // Index gefallen wie die Bedingung: Rahmenende
                        // zählt nur noch h-1 (GEMESSEN: br_table.wast
                        // `as-loop-first`).
                        h.saturating_sub(1) as usize
                    } else {
                        self.anwenden(e.k, h)
                    };
                    if e.ziel == RAHMEN {
                        if let Some(w) = self.rahmen_ende(fh, ergebnis)? {
                            return Ok(w);
                        }
                    } else {
                        self.pc = e.ziel as usize;
                    }
                }
                Op::Rueckkehr { h } => {
                    if let Some(w) = self.rahmen_ende(h as usize, ergebnis)? {
                        return Ok(w);
                    }
                }
                Op::Aufruf { h, a } => {
                    lauf.pruefe_brennstoff()?;
                    let z = &self.oben.aufrufe[a as usize];
                    let global = z.global;
                    let lokal = self.basis + h as usize - z.n as usize;
                    self.rufe(lauf, host, global, lokal)?;
                }
                // ---- T2c: Bulk-Speicher --------------------------------
                Op::Bulk { art, h, a, b } => {
                    use BulkArt as B;
                    let oben = self.basis + h as usize;
                    match art {
                        B::Abschnitt => {
                            // `memory.init`: Grenzen VOR der Kopie (M4).
                            let idx = self.k.daten_start + a;
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let quelle = self.stapel[oben - 2] as u32 as u64;
                            let ziel = self.stapel[oben - 3] as u32 as u64;
                            let dq = match lauf.store.daten.get(idx as usize) {
                                Some(Some(d)) => d.len() as u64,
                                Some(None) => 0, // verworfen (`data.drop`)
                                None => {
                                    return Err(Fehler::trap(
                                        T_SPEICHER_LESEN,
                                        format!("memory.init: Datenabschnitt {idx} unbekannt"),
                                    ));
                                }
                            };
                            let sp_max =
                                speicher_von(lauf, self.k.speicher.first().copied())?
                                    .byte_len() as u64;
                            if quelle + laenge > dq || ziel + laenge > sp_max {
                                return Err(Fehler::trap(
                                    T_SPEICHER_SCHREIBEN,
                                    format!(
                                        "memory.init: {quelle}+{laenge} (Abschnitt hat {dq} Byte) -> \
                                         {ziel}+{laenge} (Speicher hat {sp_max} Byte)"
                                    ),
                                ));
                            }
                            let stuecke: Vec<u8> = lauf.store.daten[idx as usize]
                                .as_ref()
                                .map(|d| d[quelle as usize..(quelle + laenge) as usize].to_vec())
                                .unwrap_or_default();
                            speicher_mut(lauf, self.k.speicher.first().copied())?
                                .schreib(ziel, &stuecke)?;
                        }
                        B::DatenWeg => {
                            let idx = self.k.daten_start + a;
                            if let Some(d) = lauf.store.daten.get_mut(idx as usize) {
                                *d = None;
                            }
                        }
                        B::Kopie => {
                            // Quelle erst pruefen, dann schreiben (M4).
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let quelle = self.stapel[oben - 2] as u32 as u64;
                            let ziel = self.stapel[oben - 3] as u32 as u64;
                            let (ziel_store, qu_store) =
                                speicher_paar_k(&self.k, a, b)?;
                            let max_q = lauf
                                .store
                                .speicher
                                .get(qu_store as usize)
                                .map(|s| s.byte_len() as u64)
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_SPEICHER_LESEN,
                                        "memory.copy: Quelle fehlt im Store",
                                    )
                                })?;
                            if laenge > max_q || quelle + laenge > max_q {
                                return Err(Fehler::trap(
                                    T_SPEICHER_LESEN,
                                    format!(
                                        "memory.copy: Quelle {quelle}+{laenge} > {max_q} Byte"
                                    ),
                                ));
                            }
                            if ziel_store == qu_store {
                                let s = lauf
                                    .store
                                    .speicher
                                    .get_mut(ziel_store as usize)
                                    .ok_or_else(|| {
                                        Fehler::trap(
                                            T_SPEICHER_SCHREIBEN,
                                            "memory.copy: Ziel fehlt im Store",
                                        )
                                    })?;
                                let max = s.byte_len() as u64;
                                if laenge > max || ziel + laenge > max {
                                    return Err(Fehler::trap(
                                        T_SPEICHER_SCHREIBEN,
                                        format!(
                                            "memory.copy {quelle}+{laenge} -> {ziel}+{laenge} > {max} Byte"
                                        ),
                                    ));
                                }
                                s.kopiere(ziel, quelle, laenge as usize)?;
                            } else {
                                let stueck: Vec<u8> = {
                                    let qs = lauf.store.speicher.get(qu_store as usize)
                                        .ok_or_else(|| {
                                            Fehler::trap(
                                                T_SPEICHER_LESEN,
                                                "memory.copy: Quelle fehlt im Store",
                                            )
                                        })?;
                                    qs.lies(quelle, laenge as usize)?.to_vec()
                                };
                                let zs = lauf
                                    .store
                                    .speicher
                                    .get_mut(ziel_store as usize)
                                    .ok_or_else(|| {
                                        Fehler::trap(
                                            T_SPEICHER_SCHREIBEN,
                                            "memory.copy: Ziel fehlt im Store",
                                        )
                                    })?;
                                let max_z = zs.byte_len() as u64;
                                if laenge > max_z || ziel + laenge > max_z {
                                    return Err(Fehler::trap(
                                        T_SPEICHER_SCHREIBEN,
                                        format!(
                                            "memory.copy: Ziel {ziel}+{laenge} > {max_z} Byte"
                                        ),
                                    ));
                                }
                                zs.kopiere_von(ziel, &stueck)?;
                            }
                        }
                        B::Fuellen => {
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let wert = self.stapel[oben - 2] as u32 as u8;
                            let ziel = self.stapel[oben - 3] as u32 as u64;
                            let s = speicher_mut(lauf, self.k.speicher.first().copied())?;
                            let max = s.byte_len() as u64;
                            if ziel + laenge > max {
                                return Err(Fehler::trap(
                                    T_SPEICHER_SCHREIBEN,
                                    format!("memory.fill {ziel}+{laenge} > {max} Byte"),
                                ));
                            }
                            s.fuelle(ziel, wert, laenge as usize)?;
                        }
                    }
                }
                // ---- T2c: Tabellen -------------------------------------
                Op::Tabelle { art, h, a, b } => {
                    use TabellenArt as TA;
                    let oben = self.basis + h as usize;
                    match art {
                        TA::Holen => {
                            let index = self.stapel[oben - 1] as u32 as u64;
                            let ti = self
                                .k
                                .tabellen_index
                                .get(a as usize)
                                .copied()
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.get: Tabelle {a} fehlt"),
                                    )
                                })?;
                            let tabelle = lauf
                                .store
                                .tabellen
                                .get(ti as usize)
                                .ok_or_else(|| {
                                    Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt im Store")
                                })?;
                            // Der `null`-Wert eines leeren Platzes
                            // richtet sich nach dem ELEMENTTYP.
                            let reftyp = tabelle.reftyp();
                            let wert = tabelle.lies(index)?;
                            self.stapel[oben - 1] =
                                kod(crate::interp::ref_wert_typ(wert, &reftyp));
                        }
                        TA::Setzen => {
                            let roh = self.stapel[oben - 1];
                            let index = self.stapel[oben - 2] as u32 as u64;
                            let ti = self
                                .k
                                .tabellen_index
                                .get(a as usize)
                                .copied()
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.set: Tabelle {a} fehlt"),
                                    )
                                })?;
                            let tabelle = lauf
                                .store
                                .tabellen
                                .get_mut(ti as usize)
                                .ok_or_else(|| {
                                    Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt im Store")
                                })?;
                            let wert = roh_ref(roh, tabelle.reftyp().ist_extern());
                            tabelle.schreib(index, wert)?;
                        }
                        TA::Anzahl => {
                            let ti = self
                                .k
                                .tabellen_index
                                .get(a as usize)
                                .copied()
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.size: Tabelle {a} fehlt"),
                                    )
                                })?;
                            let n = lauf
                                .store
                                .tabellen
                                .get(ti as usize)
                                .map(|t| t.len())
                                .unwrap_or(0) as i32;
                            self.stapel[oben] = n as u32 as u64;
                        }
                        TA::Wachsen => {
                            let anzahl = self.stapel[oben - 1] as u32 as u64;
                            let roh = self.stapel[oben - 2];
                            let ti = self
                                .k
                                .tabellen_index
                                .get(a as usize)
                                .copied()
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.grow: Tabelle {a} fehlt"),
                                    )
                                })?;
                            let tabelle = lauf
                                .store
                                .tabellen
                                .get_mut(ti as usize)
                                .ok_or_else(|| {
                                    Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt")
                                })?;
                            let wert = roh_ref(roh, tabelle.reftyp().ist_extern());
                            let alt = tabelle.waechst(anzahl, wert);
                            // 2 -> 1: das Ergebnis liegt auf dem
                            // Platz des WERTES (delta gefallen).
                            self.stapel[oben - 2] = alt as u32 as u64;
                        }
                        TA::Fuellen => {
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let roh = self.stapel[oben - 2];
                            let start = self.stapel[oben - 3] as u32 as u64;
                            let ti = self
                                .k
                                .tabellen_index
                                .get(a as usize)
                                .copied()
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.fill: Tabelle {a} fehlt"),
                                    )
                                })?;
                            let tabelle = lauf
                                .store
                                .tabellen
                                .get_mut(ti as usize)
                                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt"))?;
                            let wert = roh_ref(roh, tabelle.reftyp().ist_extern());
                            tabelle.fuelle(start, wert, laenge)?;
                        }
                        TA::Kopieren => {
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let q = self.stapel[oben - 2] as u32 as u64;
                            let z = self.stapel[oben - 3] as u32 as u64;
                            let zi = self.k.tabellen_index.get(a as usize).copied().ok_or_else(
                                || Fehler::trap(T_TABELLE_LESEN, "table.copy: Zieltabelle fehlt"),
                            )?;
                            let qi = self.k.tabellen_index.get(b as usize).copied().ok_or_else(
                                || Fehler::trap(T_TABELLE_LESEN, "table.copy: Quelltabelle fehlt"),
                            )?;
                            // Grenzen VOR dem Kopieren (M4 und Spec).
                            let laenge_q = lauf
                                .store
                                .tabellen
                                .get(qi as usize)
                                .map(|t| t.len() as u64)
                                .ok_or_else(|| {
                                    Fehler::trap(T_TABELLE_LESEN, "Quelltabelle fehlt")
                                })?;
                            let laenge_z = lauf
                                .store
                                .tabellen
                                .get(zi as usize)
                                .map(|t| t.len() as u64)
                                .ok_or_else(|| {
                                    Fehler::trap(T_TABELLE_LESEN, "Zieltabelle fehlt")
                                })?;
                            if q.saturating_add(laenge) > laenge_q {
                                return Err(Fehler::trap(
                                    T_TABELLE_LESEN,
                                    format!(
                                        "table.copy: Quelle {q}+{laenge} > {laenge_q} Eintraege"
                                    ),
                                ));
                            }
                            if z.saturating_add(laenge) > laenge_z {
                                return Err(Fehler::trap(
                                    T_TABELLE_SCHREIBEN,
                                    format!(
                                        "table.copy: Ziel {z}+{laenge} > {laenge_z} Eintraege"
                                    ),
                                ));
                            }
                            if zi == qi {
                                lauf.store
                                    .tabellen
                                    .get_mut(zi as usize)
                                    .ok_or_else(|| {
                                        Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt")
                                    })?
                                    .kopiere(z, q, laenge)?;
                            } else {
                                let stuecke: Vec<Option<Ref>> = (0..laenge)
                                    .map(|i| {
                                        lauf.store
                                            .tabellen
                                            .get(qi as usize)
                                            .and_then(|t| t.lies(q + i).ok())
                                            .flatten()
                                    })
                                    .collect();
                                debug_assert_eq!(stuecke.len() as u64, laenge);
                                lauf.store
                                    .tabellen
                                    .get_mut(zi as usize)
                                    .ok_or_else(|| {
                                        Fehler::trap(T_TABELLE_LESEN, "Zieltabelle fehlt")
                                    })?
                                    .init(&stuecke, z, 0, laenge)?;
                            }
                        }
                        TA::Abschnitt => {
                            // Verworfener Abschnitt wirkt wie leerer.
                            let element = self.k.elemente_start + a;
                            let laenge = self.stapel[oben - 1] as u32 as u64;
                            let quelle = self.stapel[oben - 2] as u32 as u64;
                            let ziel = self.stapel[oben - 3] as u32 as u64;
                            let el = lauf
                                .store
                                .elemente
                                .get(element as usize)
                                .map(|e| e.clone().unwrap_or_default())
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.init: Element {element} unbekannt"),
                                    )
                                })?;
                            let ti = self.k.tabellen_index.get(b as usize).copied().ok_or_else(
                                || {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("table.init: Tabelle {b} fehlt"),
                                    )
                                },
                            )?;
                            lauf.store
                                .tabellen
                                .get_mut(ti as usize)
                                .ok_or_else(|| {
                                    Fehler::trap(
                                        T_TABELLE_LESEN,
                                        format!("Tabelle {ti} fehlt im Store"),
                                    )
                                })?
                                .init(&el, ziel, quelle, laenge)?;
                        }
                        TA::ElementWeg => {
                            let idx = self.k.elemente_start + a;
                            if let Some(e) = lauf.store.elemente.get_mut(idx as usize) {
                                *e = None;
                            }
                        }
                    }
                }
                // ---- T2c: call_indirect --------------------------------
                Op::Indirekt { h, i } => {
                    // Reihenfolge wie `CallIndirect` im alten Pfad:
                    // Tabelle, reftyp, Eintrag, Signatur, Subtyp — dann
                    // erst Brennstoff (in `rufe`: Lookup, dann Aufbau).
                    let (tabelle, typ) = {
                        let w = &self.oben.indizierte[i as usize];
                        (w.tabelle, w.typ)
                    };
                    let ti = self
                        .k
                        .tabellen_index
                        .get(tabelle as usize)
                        .copied()
                        .ok_or_else(|| {
                            Fehler::trap(
                                T_INDIREKT,
                                format!("call_indirect: Tabelle {tabelle} fehlt"),
                            )
                        })?;
                    let passt = lauf
                        .store
                        .tabellen
                        .get(ti as usize)
                        .map(|t| matches!(t.reftyp(), Reftyp::Func | Reftyp::Konkret { .. }))
                        .unwrap_or(true);
                    if !passt {
                        return Err(Fehler::trap(
                            T_INDIREKT,
                            format!("call_indirect: Tabelle {ti} haelt keine funcref-Eintraege"),
                        ));
                    }
                    let laenge = lauf
                        .store
                        .tabellen
                        .get(ti as usize)
                        .map(|t| t.len())
                        .unwrap_or(0);
                    let index = self.stapel[self.basis + h as usize - 1] as u32 as u64;
                    let eintrag = lauf
                        .store
                        .tabellen
                        .get(ti as usize)
                        .and_then(|t| t.lies(index).ok())
                        .ok_or_else(|| {
                            Fehler::trap(
                                T_INDIREKT,
                                format!(
                                    "undefined element: Index {index} (Tabelle {ti} hat {laenge} Plaetze)"
                                ),
                            )
                        })?;
                    let func = match eintrag {
                        Some(Ref::Func(f)) => f,
                        _ => {
                            return Err(Fehler::trap(
                                T_INDIREKT,
                                format!("uninitialized element {index}"),
                            ));
                        }
                    };
                    let n_param = lauf
                        .store
                        .funktionen
                        .get(func as usize)
                        .map(|f| crate::lauf::signatur(f))
                        .ok_or_else(|| {
                            Fehler::trap(T_INDIREKT, format!("Funktion {func} fehlt im Store"))
                        })?;
                    if let Some(wt) = self.k.typen.get(typ as usize) {
                        if !crate::interp::subtyp_indirekt_ktx(
                            lauf, &self.k, func, typ, &n_param, wt,
                        ) {
                            return Err(Fehler::trap(
                                T_INDIREKT,
                                format!(
                                    "indirect call type mismatch: {} != {}",
                                    crate::interp::zeile(&n_param),
                                    crate::interp::zeile(wt)
                                ),
                            ));
                        }
                    }
                    lauf.pruefe_brennstoff()?;
                    let n = n_param.parameter.len();
                    let h1 = (h as usize).saturating_sub(1);
                    if self.basis + h1 < n {
                        return Err(Fehler::trap(
                            T_DIVERSE,
                            format!("call: {n} Argumente fehlen auf dem Stapel"),
                        ));
                    }
                    let lokal = self.basis + h1 - n;
                    self.rufe(lauf, host, func, lokal)?;
                }
                Op::RefIstNull { h } => {
                    let w = self.stapel[self.basis + h as usize - 1];
                    self.stapel[self.basis + h as usize - 1] = u64::from(w == 0);
                }
                Op::RefNichtNull { h } => {
                    let w = self.stapel[self.basis + h as usize - 1];
                    if w == 0 || w == KENN {
                        return Err(Fehler::trap(T_INDIREKT, "null reference"));
                    }
                }
                Op::RefFunktion { h, f } => {
                    let g = self.k.funktionen.get(f as usize).copied().unwrap_or(f);
                    self.stapel[self.basis + h as usize] = g as u64 + 1;
                }
                Op::Unreachable => return Err(Fehler::trap(T_UNERREICHBAR, "unreachable")),
            }
        }
    }

    /// Aufruf auf den Store-Index `global`: die `n` Argumente liegen ab
    /// `lokal`; bei `call_indirect` steht der Tabellenindex noch darauf
    /// (`lokal + n`) und wird beim Kuerzen mit entfernt. Reihenfolge
    /// und Treibstoff wie `unteraufruf` des alten Pfades.
    fn rufe(
        &mut self,
        lauf: &mut Lauf,
        host: &mut dyn Host,
        global: u32,
        lokal: usize,
    ) -> Result<(), Fehler> {
        // 2026-10-15: the store entries are `Arc`s now (the store moves
        // between threads inside the JS engine). A wasm -> wasm call reads
        // the parameter count through a borrow instead of cloning the
        // function, and moves the caller's program and context into the
        // frame instead of cloning them: 4 reference count updates per
        // call and return instead of 10.
        let eigen_n = match lauf.store.funktionen.get(global as usize).map(|f| f.as_ref()) {
            Some(Funktion::Eigen { typ, .. }) => Some(typ.parameter.len()),
            Some(Funktion::Fremd { .. }) => None,
            None => {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!("Funktion {global} fehlt im Store"),
                ));
            }
        };
        if let Some(n) = eigen_n {
            let p = match lauf.store.schnell.get(global as usize) {
                Some(Status::Okay(p)) => p.clone(),
                // Im geschlossenen Graphen nie noetig; defensiv
                // nachziehen, sonst Rueckfall.
                _ => kompiziere(lauf, global).ok_or_else(|| {
                    Fehler::trap(
                        T_DIVERSE,
                        format!("schnell: Funktion {global} nicht uebersetzbar"),
                    )
                })?,
            };
            // Argumente SIND die ersten Locals und bleiben liegen
            // (Hebel 1, identisch zum alten Pfad): Locals kommen
            // dahinter — bei `call_indirect` wird damit auch der
            // Tabellenindex ueber den Argumenten entfernt.
            //
            // Kontext des GERUFENEN: immer der Store-Eintrag der
            // gerufenen Funktion, sonst der des Aufrufers (wie
            // `erster_rahmen`/`unteraufruf`).
            let k_neu = lauf
                .store
                .funktion_kontext
                .get(global as usize)
                .and_then(|s| s.as_ref())
                .map(Arc::clone)
                .unwrap_or_else(|| Arc::clone(&self.k));
            let oben_alt = std::mem::replace(&mut self.oben, p);
            let k_alt = std::mem::replace(&mut self.k, k_neu);
            self.rahmen.push(Rahmen {
                lokal: self.lokal,
                basis: self.basis,
                pc: self.pc,
                oben: oben_alt,
                k: k_alt,
            });
            self.stapel.truncate(lokal + n);
            for t in &self.oben.lokale {
                self.stapel.push(kod(Wert::standard(*t)));
            }
            self.lokal = lokal;
            self.basis = self.stapel.len();
            pruefe_tiefe(lauf, self.rahmen.len() + 1)?;
            lauf.zaehle(1);
            self.stapel.resize(self.basis + self.oben.feld, 0);
            self.pc = 0;
            return Ok(());
        }
        let f = Arc::clone(&lauf.store.funktionen[global as usize]);
        match f.as_ref() {
            Funktion::Fremd { typ, funktion: g } => {
                // Argumente typgerichtet dekodieren — der alte Pfad
                // slice-te nur (Werte-Array), die Hostwelt erwartet
                // `Wert`. Die Typen kommen aus dem Store-Eintrag.
                self.host_args.clear();
                for (i, t) in typ.parameter.iter().enumerate() {
                    let w = self.stapel[lokal + i];
                    self.host_args.push(dek(*t, w));
                }
                lauf.einbettung += 1;
                if lauf.einbettung as usize > lauf.grenzen.einbettung {
                    lauf.einbettung -= 1;
                    return Err(Fehler::trap(
                        T_EINBETTUNG,
                        format!(
                            "Hostaufruf tiefer als {} (Grenze)",
                            lauf.grenzen.einbettung
                        ),
                    ));
                }
                let r = g(lauf, host, &self.host_args);
                lauf.einbettung -= 1;
                let r = r?;
                self.stapel.truncate(lokal);
                for w in r {
                    self.stapel.push(kod(w));
                }
            }
            Funktion::Eigen { .. } => {}
        }
        Ok(())
    }

    /// Kopie anwenden; Rueckgabe ist die Hoehe NACH der Kopie (fuer
    /// das Rahmenende: Ergebnisbereich `fh - n .. fh`).
    fn anwenden(&mut self, k: u32, h: u32) -> usize {
        if k == KEIN {
            return h as usize;
        }
        let c = self.oben.kopien[k as usize];
        let von = self.basis + c.von as usize;
        let bis = von + c.anzahl as usize;
        let nach = self.basis + c.nach as usize;
        if c.anzahl > 0 {
            self.stapel.copy_within(von..bis, nach);
        }
        c.nach as usize + c.anzahl as usize
    }

    /// Rahmen beenden: die obersten `ergebnis_anzahl` Werte (bei Hoehe
    /// `h`) sind sein Ergebnis. `Some(v)` = der letzte Rahmen.
    ///
    /// Kein `ausnahme`-Fall: der Schnellpfad uebersetzt keine
    /// Exception-Op's, ein unbenannter Ausnahmezustand kann hier nie
    /// entstehen.
    fn rahmen_ende(
        &mut self,
        h: usize,
        ergebnis: &[Valtyp],
    ) -> Result<Option<Vec<Wert>>, Fehler> {
        let n = self.oben.ergebnis_anzahl.min(h);
        if n > 0 {
            let von = self.basis + h - n;
            self.stapel.copy_within(von..self.basis + h, self.lokal);
        }
        self.stapel.truncate(self.lokal + n);
        match self.rahmen.pop() {
            None => {
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let w = self.stapel[self.lokal + i];
                    let t = ergebnis.get(i).copied().unwrap_or(Valtyp::I32);
                    out.push(dek(t, w));
                }
                self.stapel.truncate(self.lokal);
                Ok(Some(out))
            }
            Some(r) => {
                self.lokal = r.lokal;
                self.basis = r.basis;
                self.pc = r.pc;
                self.oben = r.oben;
                self.k = r.k;
                self.stapel.resize(self.basis + self.oben.feld, 0);
                Ok(None)
            }
        }
    }
}
