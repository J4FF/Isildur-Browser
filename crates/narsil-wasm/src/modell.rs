//! Datenmodell des Moduls (WebAssembly 2.0 Core). W1 Punkt 1: der
//! Binaerformat-Parser baut genau diese Struktur, der Validator prueft sie,
//! der Interpreter laeuft darauf, der Debugger zeigt darauf.
//!
//! Indexraeume: importierte und eigene Definitionen liegen im JEWEILS
//! GEMEINSAMEN Vektor (Spec-Semantik: der Index zaehlt Imports zuerst).
//! `Func::Importiert` traegt dann keine Nutzlast, `Func::Eigen` schon.

use crate::fehler::{ART_REFTYP, Fehler, V_TYP};
use crate::op::Befehl;
use std::collections::HashSet;

/// Wertsatz einer Funktion oder eines Blocks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FuncTyp {
    pub parameter: Vec<Valtyp>,
    pub ergebnis: Vec<Valtyp>,
}

/// Werttyp (kein `v128`: SIMD ist Wasm 2.0 *inklusive*, aber der Auftrag
/// W2 nennt ausdruecklich nur MVP + sign-ext + sat-trunc + bulk-memory +
/// multi-value + reference-types. SIMD ist in `team/WUNSCH_WASM.txt`
/// als offener Punkt vermerkt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Valtyp {
    I32,
    I64,
    F32,
    F64,
    Ref(Reftyp),
}

/// Referenztyp (Wasm 2.0: `funcref`, `externref`; GC-Typen sind 3.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reftyp {
    Func,
    Extern,
    /// `(ref null $t)` / `(ref $t)`: eine Funktionsreferenz, die nur auf
    /// Funktionen vom Typindex `typ` zeigen darf (Wasm 3.0, Typed Function
    /// References). `nullable == false` heisst: `null` ist nicht erlaubt.
    ///
    /// GEMESSEN 02.10.: ohne diese Variante fiel in `br_table.wast` das
    /// ganze Modul (52 KiB, 161 Testfaelle) und in `elem.wast`/`table.wast`
    /// je ein Dutzend Faelle weg.
    Konkret {
        typ: u32,
        nullable: bool,
    },
}

impl Reftyp {
    /// Ist das eine Funktionsreferenz (auch mit konkretem Typ)?
    ///
    /// `(ref extern)` ist KEINE Funktionsreferenz, auch wenn es ein
    /// Typindex traegt (GEMESSEN 03.10., `table.wast` Z. 30).
    pub fn ist_funktion(&self) -> bool {
        match self {
            Reftyp::Func => true,
            // Jeder konkrete Typindex ist in unserem Modell eine
            // Funktion — ausser der externe Wildcard.
            //
            // GEMESSEN 03.10.: `br_table.wast` verlor 161 Faelle, als der
            // Wildcard hier zu eng gefasst war ((ref null $t) ist auch eine
            // Funktionsreferenz).
            Reftyp::Konkret { typ, .. } => {
                *typ != JEDER_EXTERNTYP
                    && *typ != JEDER_ARRAYTYP
                    // Die GC-Wildcards sind KEINE Funktionsreferenzen:
                    // `anyref` zaehlt nicht als `func`, `(ref nofunc)`
                    // bewohnt nur `null`.
                    //
                    // GEMESSEN 03.10. (`ref_null.wast`).
                    && !matches!(
                        *typ,
                        JEDER_TYP | JEDER_EXNTYP | JEDER_EQTYP | JEDER_I31TYP | JEDER_STRUKTYP
                            | KEIN_FUNKTIONSTYP | KEIN_TYP | KEIN_EXNTYP
                    )
            }
            Reftyp::Extern => false,
        }
    }

    /// Ist das eine externe Referenz (`externref`, auch mit Wildcard)?
    ///
    /// `(ref noextern)` zaehlt dazu: der Wert `null` ist ein
    /// `externref`-Wert.
    ///
    /// GEMESSEN 03.10. (`ref_null.wast`: `(assert_return (invoke
    /// "externref") (ref.null extern))` — der Wert stammt aus einem
    /// `nullexternref`-Global und wurde als `funcref`-null abgelegt:
    /// „null erwartet, null").
    pub fn ist_extern(&self) -> bool {
        match self {
            Reftyp::Extern => true,
            Reftyp::Konkret { typ, .. } => *typ == JEDER_EXTERNTYP || *typ == KEIN_EXTERNTYP,
            Reftyp::Func => false,
        }
    }

    /// Traegt der Typ den Array-Wildcard (`arrayref`, `(ref array)`)?
    ///
    /// Ein konkreter Array-Typindex (`(ref $arr)`) braucht das Modul —
    /// dafuer `ist_array_typ`.
    pub fn ist_array_wildcard(&self) -> bool {
        matches!(self, Reftyp::Konkret { typ, .. } if *typ == JEDER_ARRAYTYP)
    }

    /// Nimmt dieser Tabellentyp ein GC-Objekt auf (`ref.i31`,
    /// `struct.new`, `array.new`)?
    ///
    /// Das sind genau die Typen, die im GC-Lattice ueber oder gleich
    /// `structref` liegen: `anyref`, `eqref`, `structref`, `arrayref`,
    /// `i31ref`.
    ///
    /// GEMESSEN 03.10. (`extern.wast` Z. 16: `table.set` eines i31 in
    /// eine `anyref`-Tabelle war vorbei abgelehnt — „irgendein
    /// nicht-nuller externref erwartet, null").
    pub fn nimmt_gc(&self) -> bool {
        match self {
            Reftyp::Konkret { typ, .. } => matches!(
                *typ,
                JEDER_TYP | JEDER_EQTYP | JEDER_STRUKTYP | JEDER_ARRAYTYP | JEDER_I31TYP
            ),
            _ => false,
        }
    }

    /// Darf dieser Referenztyp `null` annehmen?
    pub fn ist_nullable(&self) -> bool {
        match self {
            Reftyp::Func | Reftyp::Extern => true,
            Reftyp::Konkret { nullable, .. } => *nullable,
        }
    }

    /// Der erlaubte Typindex (`None` = kein konkreter Typ).
    pub fn typindex(&self) -> Option<u32> {
        match self {
            Reftyp::Konkret { typ, .. } if !ist_wildcard(*typ) => Some(*typ),
            _ => None,
        }
    }

    /// Subtyp-Pruefung: `passt_als(self, supertyp)` heisst, jeder Wert
    /// dieses Referenztyps ist auch vom Typ `supertyp`.
    pub fn passt_als(&self, supertyp: &Reftyp) -> bool {
        match (self, supertyp) {
            (Reftyp::Func, Reftyp::Func) => true,
            (Reftyp::Extern, Reftyp::Extern) => true,
            // `(ref null $t)` erfuellt `(ref null func)`.
            (Reftyp::Konkret { .. }, Reftyp::Func) => true,
            // `(ref extern)`/`(ref null extern)` erfuellt `externref`
            // (die Umkehrung ist fuer `null` verboten).
            //
            // GEMESSEN 03.10. (`br_on_null` auf einem `(result externref)`
            // -Block: „externref erwartet, typreferenz auf dem Stapel").
            (
                Reftyp::Konkret {
                    typ: JEDER_EXTERNTYP,
                    ..
                },
                Reftyp::Extern,
            ) => true,
            // `externref` erfuellt `(ref null (ref extern))` — aber nicht
            // die Form OHNE `null`.
            (
                Reftyp::Extern,
                Reftyp::Konkret {
                    typ: JEDER_EXTERNTYP,
                    nullable: true,
                },
            ) => true,
            // Ebenso `funcref` und `(ref null func)`.
            (
                Reftyp::Func,
                Reftyp::Konkret {
                    typ: JEDER_FUNKTIONSTYP,
                    nullable: true,
                },
            ) => true,
            (
                Reftyp::Konkret {
                    typ: a,
                    nullable: na,
                },
                Reftyp::Konkret {
                    typ: b,
                    nullable: nb,
                },
            ) => a == b && (*na || !*nb),
            _ => false,
        }
    }

    pub fn byte(&self) -> u8 {
        match self {
            Reftyp::Func => 0x70,
            Reftyp::Extern => 0x6f,
            // `(ref null $t)` = 0x63, `(ref $t)` = 0x64 (Binaerform).
            Reftyp::Konkret { nullable, .. } => {
                if *nullable {
                    0x63
                } else {
                    0x64
                }
            }
        }
    }
    pub fn von_byte(b: u8) -> Option<Reftyp> {
        match b {
            0x70 => Some(Reftyp::Func),
            0x6f => Some(Reftyp::Extern),
            // Die Heaptypen des GC-Vorschlags (Wasm 3.0). Ohne sie liest
            // der Binaerpfad `ref.test`/`ref.cast`/`ref.null` nicht.
            //
            // GEMESSEN 03.10. (`ref_test.wast`, `ref_cast.wast`).
            0x6e => Some(Reftyp::Konkret {
                typ: JEDER_TYP,
                nullable: true,
            }), // any
            0x6d => Some(Reftyp::Konkret {
                typ: JEDER_EQTYP,
                nullable: true,
            }), // eq
            0x6c => Some(Reftyp::Konkret {
                typ: JEDER_I31TYP,
                nullable: true,
            }), // i31
            0x6b => Some(Reftyp::Konkret {
                typ: JEDER_STRUKTYP,
                nullable: true,
            }), // struct
            0x6a => Some(Reftyp::Konkret {
                typ: JEDER_ARRAYTYP,
                nullable: true,
            }), // array
            0x69 => Some(Reftyp::Konkret {
                typ: JEDER_EXNTYP,
                nullable: true,
            }), // exn
            0x71 => Some(Reftyp::Konkret {
                typ: KEIN_TYP,
                nullable: true,
            }), // none
            0x73 => Some(Reftyp::Konkret {
                typ: KEIN_FUNKTIONSTYP,
                nullable: true,
            }), // nofunc
            0x72 => Some(Reftyp::Konkret {
                typ: KEIN_EXTERNTYP,
                nullable: true,
            }), // noextern
            0x74 => Some(Reftyp::Konkret {
                typ: KEIN_EXNTYP,
                nullable: true,
            }), // noexn
            _ => None,
        }
    }
    pub fn text(&self) -> &'static str {
        match self {
            Reftyp::Func => "funcref",
            Reftyp::Extern => "externref",
            Reftyp::Konkret { .. } => "typreferenz",
        }
    }
}

/// `(ref func)` — die Form OHNE `null`: jeder Funktionstyp, aber der Wert
/// darf nie `null` sein. Als Typindex traegt sie diesen Wildcard.
pub const JEDER_FUNKTIONSTYP: u32 = u32::MAX;

/// `(ref extern)` — jeder `externref`, aber nie `null`.
/// (`ref null extern)` bleibt `Reftyp::Extern`.)
///
/// GEMESSEN 03.10. (`table.wast` Z. 30: `(assert_invalid (module (table 0
/// (ref extern))) "type mismatch")` — 1 Fall).
pub const JEDER_EXTERNTYP: u32 = u32::MAX - 1;

/// `(ref array)` / `arrayref`: jeder Array-Verweis.
///
/// GEMESSEN 03.10. (`table_init.wast` Z. 790).
pub const JEDER_ARRAYTYP: u32 = u32::MAX - 2;

/// `(ref eq)` / `eqref`: jeder `i31`-, `struct`- oder `array`-Verweis.
pub const JEDER_EQTYP: u32 = u32::MAX - 3;

/// `(ref any)` / `anyref`: JEDE Referenz (oberster Typ des Lattices).
pub const JEDER_TYP: u32 = u32::MAX - 4;

/// `(ref none)`: der LEERE Typ — nur `null` bewohnt ihn.
///
/// GEMESSEN 03.10. (`ref_null.wast` Z. 19/34: `nullref`).
pub const KEIN_TYP: u32 = u32::MAX - 5;

/// `(ref exn)` / `exnref`: eine Ausnahme (Wasm 3.0).
pub const JEDER_EXNTYP: u32 = u32::MAX - 6;

/// `(ref noexn)` / `nullexnref`: nur `null`.
pub const KEIN_EXNTYP: u32 = u32::MAX - 7;

/// `(ref nofunc)` / `nullfuncref`: nur `null`.
pub const KEIN_FUNKTIONSTYP: u32 = u32::MAX - 8;

/// `(ref noextern)` / `nullexternref`: nur `null`.
pub const KEIN_EXTERNTYP: u32 = u32::MAX - 9;

/// `(ref i31)` / `i31ref`: eine 31-Bit-Ganzzahl im GC-Heap.
pub const JEDER_I31TYP: u32 = u32::MAX - 10;

/// `(ref struct)` / `structref`: ein Struktur-Wert im GC-Heap.
pub const JEDER_STRUKTYP: u32 = u32::MAX - 11;

/// Ein Heaptyp-Wildcard (kein Typindex des Moduls)?
pub fn ist_heaptyp_wildcard(typ: u32) -> bool {
    matches!(
        typ,
        JEDER_FUNKTIONSTYP
            | JEDER_EXTERNTYP
            | JEDER_ARRAYTYP
            | JEDER_EQTYP
            | JEDER_TYP
            | KEIN_TYP
            | JEDER_EXNTYP
            | KEIN_EXNTYP
            | KEIN_FUNKTIONSTYP
            | KEIN_EXTERNTYP
            | JEDER_I31TYP
            | JEDER_STRUKTYP
    )
}

/// Die Knoten des GC-Lattices.
///
/// `Klasse::Konkret` heisst „ein Typindex des Moduls, der eine
/// Funktionssignatur ist" — Arrays und Strukturen werden ueber
/// `ist_array_typ` auf ihre eigenen Klassen abgebildet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Klasse {
    /// `none` — und damit auch `nofunc`, `noextern`, `noexn`: die
    /// Spezifikation definiert alle vier als DENSELBEN untersten Typ
    /// (`nullfuncref` = `nullexternref` = `nullref`); sein einziger Wert
    /// ist `null`, und `null` wohnt in jedem Referenztyp.
    ///
    /// GEMESSEN 03.10. (`ref_null.wast` Z. 74: `(global (ref null $t)
    /// (ref.null nofunc))` ist gueltig — also ist `nofunc` <: `$t`).
    Keine,
    /// Ein Typindex des Moduls mit Funktionssignatur (`(ref $t)`).
    Konkret,
    /// `func`.
    Funktion,
    /// `extern`.
    Extern,
    /// `exn`.
    Ausnahme,
    /// `struct`.
    Struktur,
    /// `array`.
    Array,
    /// `i31`.
    I31,
    /// `eq`: Obertyp von `struct`, `array` und `i31`.
    Gleich,
    /// `any`: jeder Referenzwert.
    Irgendwas,
}

impl Klasse {
    /// Gehoert der Quelltyp in den Zieltyp?
    ///
    /// `Klasse::Keine` (`none`) bewohnt nur `null`, und `null` wohnt in
    /// jedem Referenztyp — sie passt deshalb in ALLE. Umgekehrt passt in
    /// `none` gar nichts ausser `none` selbst.
    pub fn passt_in(self, ziel: Klasse) -> bool {
        use Klasse::*;
        match (self, ziel) {
            // Jeder Typ passt in sich selbst; ueber die `null`-Form
            // entscheidet der Aufrufer.
            (_, _) if self == ziel => true,
            (Keine, _) => true,
            (_, Keine) => false,
            // Zwei Typindizes: die Gleichheit entscheidet `typen_gleich`.
            (Konkret, Konkret) => true,
            (Konkret, Funktion | Irgendwas) => true,
            (Funktion, Funktion | Irgendwas) => true,
            (Extern, Extern | Irgendwas) => true,
            (Ausnahme, Ausnahme | Irgendwas) => true,
            (Struktur, Struktur | Gleich | Irgendwas) => true,
            (Array, Array | Gleich | Irgendwas) => true,
            (I31, I31 | Gleich | Irgendwas) => true,
            (Gleich, Gleich | Irgendwas) => true,
            (Irgendwas, Irgendwas) => true,
            _ => false,
        }
    }
}

/// Der Knoten eines Referenztyps im GC-Lattice. Fuer `Klasse::Konkret`
/// muss der Typindex des MODULS bekannt sein.
///
/// GEMESSEN 03.10. (`ref_null.wast`).
pub fn reftyp_klasse(m: &Modul, r: &Reftyp) -> Klasse {
    match r {
        Reftyp::Func => Klasse::Funktion,
        Reftyp::Extern => Klasse::Extern,
        Reftyp::Konkret { typ, .. } => match *typ {
            // `none`, `nofunc`, `noextern`, `noexn` sind derselbe
            // unterste Typ.
            KEIN_TYP | KEIN_FUNKTIONSTYP | KEIN_EXTERNTYP | KEIN_EXNTYP => Klasse::Keine,
            JEDER_TYP => Klasse::Irgendwas,
            JEDER_EXNTYP => Klasse::Ausnahme,
            JEDER_FUNKTIONSTYP => Klasse::Funktion,
            JEDER_EXTERNTYP => Klasse::Extern,
            JEDER_EQTYP => Klasse::Gleich,
            JEDER_I31TYP => Klasse::I31,
            JEDER_STRUKTYP => Klasse::Struktur,
            JEDER_ARRAYTYP => Klasse::Array,
            // Ein Typindex des Moduls: Array- und STRUKTURtypen sind
            // eigenstaendig, alles andere ist in unserem Modell eine
            // Funktionssignatur.
            //
            // GEMESSEN 03.10. (`extern.wast` Z. 4 + `struct.new_default`:
            // „Block0[0]: typreferenz erwartet, typreferenz auf dem
            // Stapel" — ein konkreter Strukturtyp fiel auf den
            // Funktionszweig und passte in kein `(ref struct)`).
            _ => {
                if ist_array_typ(m, *typ) {
                    Klasse::Array
                } else if ist_struct_typ(m, *typ) {
                    Klasse::Struktur
                } else {
                    Klasse::Konkret
                }
            }
        },
    }
}

/// Ist das ein Wildcard-Index (kein echter Typindex)?
pub fn ist_wildcard(typ: u32) -> bool {
    ist_heaptyp_wildcard(typ)
}

/// Ein GC-Objekt (Array, Wasm 3.0 GC).
///
/// GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
#[derive(Debug, Clone, PartialEq)]
pub struct GcObjekt {
    /// Typindex des GC-Typs (Array- oder Strukturtyp).
    pub typ: u32,
    /// Zahl der Elemente (Felder bei einer Struktur).
    pub laenge: u32,
    /// Der eigentliche Inhalt.
    ///
    /// GEMESSEN 03.10. (`extern.wast`): ohne Inhalt konnte ein Array
    /// keinen Wert zurueckgeben und `ref.i31` nicht erkannt werden.
    pub inhalt: GcInhalt,
}

/// Ein Struktur-Typ: `(struct)` oder `(struct <feld> …)`.
///
/// GEMESSEN 03.10. (`extern.wast` Z. 4: `(type $st (struct))`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructTyp {
    /// Die Felder in Reihenfolge.
    pub felder: Vec<StructFeld>,
    /// `(struct (mut …) …)` erlaubt `struct.set` auf JEDEM Feld.
    pub veraenderbar: bool,
}

/// EIN Feld einer Struktur: `(field $name <speichertyp>*)`.
///
/// GEMESSEN 03.10. (`struct.wast` Z. 4–11): ein Feld traegt einen
/// Bezeichner, ist gepackt (`i8`/`i16`) und ist veraenderbar. Ohne das
/// war nur `(struct <typ> …)` lesbar, und `struct.get_s`/`get_u` hatten
/// kein Feld, das sie benennen koennten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructFeld {
    /// `$name` des Feldes (`None`, wenn keiner dasteht).
    pub name: Option<String>,
    /// Der **Werttyp**: bei gepackten Feldern steht hier `i32` — die
    /// Bit-Zahl steht in `bits`, weil `i8`/`i16` im Typmodell gar nicht
    /// vorkommen (siehe `Valtyp::aus_text`).
    pub typ: Valtyp,
    /// 8 oder 16 fuer gepackte Felder, 0 fuer „ungepackt".
    pub bits: u32,
    /// `(field (mut …) …)` — nur dann darf `struct.set` schreiben.
    pub veraenderbar: bool,
}

impl StructFeld {
    /// Der Standardwert: `0` bei gepackten Feldern, sonst der des Typs.
    pub fn standard(&self) -> crate::Wert {
        if self.bits > 0 {
            return crate::Wert::I32(0);
        }
        crate::Wert::standard(self.typ)
    }

    /// Der Wert, wie er **abgelegt** wird: gepackte Felder werden auf die
    /// Bit-Zahl gekuerzt und vorzeichenbehaftet erweitert.
    ///
    /// GEMESSEN 03.10. (`struct.wast` Z. 55ff: `set_get_packed_g0_1`):
    /// so liefert `struct.get_s` den gespeicherten Wert direkt, und
    /// `struct.get_u` maskiert ihn (`-1` im `i8`-Feld wird 255).
    pub fn speicherwert(&self, w: crate::Wert) -> crate::Wert {
        if self.bits == 0 {
            return w;
        }
        crate::Wert::I32(kuerzen_und_vorzeichen(w.als_i32().unwrap_or(0), self.bits))
    }

    /// Aus dem Speicherwert lesen: `vorzeichen` = `struct.get_s`,
    /// sonst `struct.get_u`.
    pub fn lese(&self, w: crate::Wert, vorzeichen: bool) -> crate::Wert {
        if self.bits == 0 || vorzeichen {
            return w;
        }
        crate::Wert::I32(w.als_i32().unwrap_or(0).rem_euclid(1 << self.bits.min(31)))
    }
}

/// Auf `bits` kuerzen und **vorzeichenbehaftet** erweitern
/// (`bits` = 8: `0x7f` -> 127, `0x80` -> -128).
pub fn kuerzen_und_vorzeichen(w: i32, bits: u32) -> i32 {
    if bits == 0 || bits >= 32 {
        return w;
    }
    let b = bits as u32;
    let maske = (1i64 << b) - 1;
    let roh = (w as i64) & maske;
    // Das oberste Bit entscheidet ueber das Vorzeichen.
    if roh & (1i64 << (b - 1)) != 0 {
        (roh - (1i64 << b)) as i32
    } else {
        roh as i32
    }
}

impl StructTyp {
    /// Index des Feldes mit diesem Bezeichner (`None`, wenn es keins gibt).
    pub fn feld_index(&self, name: &str) -> Option<u32> {
        self.felder
            .iter()
            .position(|f| f.name.as_deref() == Some(name))
            .map(|i| i as u32)
    }

    /// Das Feld an `index` (falls es das gibt).
    pub fn feld(&self, index: u32) -> Option<&StructFeld> {
        self.felder.get(index as usize)
    }

    /// Die Feldtypen in Reihenfolge (fuer die Typpruefung).
    pub fn feld_typen(&self) -> Vec<Valtyp> {
        self.felder.iter().map(|f| f.typ).collect()
    }

    /// Die Standardwerte aller Felder — fuer `struct.new_default`.
    pub fn standard_werte(&self) -> Vec<crate::Wert> {
        self.felder.iter().map(|f| f.standard()).collect()
    }
}

/// Der Inhalt eines GC-Objekts.
///
/// GEMESSEN 03.10. (`extern.wast`): `ref.i31 7` muss wieder als „ein
/// i31-Wert" erkannt werden, und `(ref.array)`/`(ref.struct)` an der
/// Erwartungsseite.
#[derive(Debug, Clone, PartialEq)]
pub enum GcInhalt {
    /// Ein i31-Wert (31 Bit, vorzeichenbehaftet).
    I31(i32),
    /// Ein Struktur-Objekt: die Felder in Reihenfolge.
    Feld(Vec<crate::Wert>),
    /// Ein Array: die Elemente in Reihenfolge.
    Array(Vec<crate::Wert>),
}

/// Ein Array-Typ: `(array <typ>)` oder `(array (mut <typ>))`.
///
/// GEMESSEN 03.10. (`table_init.wast` Z. 790: `(type $arr (array (mut
/// arrayref)))`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayTyp {
    /// Elementtyp.
    pub inhalt: Valtyp,
    /// `(mut …)` erlaubt `array.set`.
    pub veraenderbar: bool,
    /// 8 oder 16 fuer **gepackte** Arrays (`(array i8)`, `(array i16)`),
    /// 0 fuer ungepackte. `array.get` ist dann ungueltig und dafuer sind
    /// `array.get_s`/`array.get_u` da.
    ///
    /// GEMESSEN 03.10. (`array_copy.wast` Z. 17ff: „array types do not
    /// match" blieb aus, weil `(array (mut i8))` und `(array i16)` im
    /// Modell identisch waren).
    pub bits: u32,
}

impl ArrayTyp {
    /// Die Breite eines Elements in BYTE — `array.new_data` rechnet damit.
    ///
    /// GEMESSEN 03.10. (`array_new_data.wast`: `(data $d "abcd")` in ein
    /// `(array (mut i8))` ergibt vier Elemente).
    pub fn element_breite(&self) -> u32 {
        if self.bits > 0 {
            return self.bits / 8;
        }
        match self.inhalt {
            Valtyp::I32 | Valtyp::F32 => 4,
            Valtyp::I64 | Valtyp::F64 => 8,
            _ => 0,
        }
    }
}

/// Subtyping: passt ein Wert vom Typ `a` auch in `b`?
///
///Fuer die Zahlentypen ist das Gleichheit; bei Referenztypen gilt die
///Regel der Spezifikation: `(ref $t)` <: `(ref null $t)` <: `(ref null
///func)`, `(ref null $t)` <: `(ref null $u)` nur fuer `t == u`.
///
/// GEMESSEN 02.10. (`br_table.wast`, `elem.wast`, `table.wast`): mit
///Gleichheit fielen 185 Testfaelle weg.
pub fn valtyp_passt_als(m: &Modul, a: &Valtyp, b: &Valtyp) -> bool {
    match (a, b) {
        (Valtyp::Ref(ra), Valtyp::Ref(rb)) => reftyp_passt_als(m, ra, rb),
        _ => a == b,
    }
}

/// Der **abstrakte** Heap-Typ-Index eines Referenztyps, wenn er einer
/// ist — sonst `None`.
///
/// GEMESSEN 03.10.: `reftyp_klasse` allein kann das nicht sagen — `func`
/// (das abstrakte) und ein Modul-Typindex mit Funktionssignatur ergeben
/// dieselbe Klasse. Entscheidend ist der Index, nicht die Klasse. Fuer
/// `Reftyp::Func` / `Reftyp::Extern` wird der Wildcard-Index geliefert, so
/// dass `(ref func)` und `funcref` vergleichbar sind.
fn reftyp_abstrakt_index(r: &Reftyp) -> Option<u32> {
    match r {
        Reftyp::Func => Some(JEDER_FUNKTIONSTYP),
        Reftyp::Extern => Some(JEDER_EXTERNTYP),
        Reftyp::Konkret { typ, .. } if ist_wildcard(*typ) => Some(*typ),
        _ => None,
    }
}

/// Wie `valtyp_passt_als`, aber zwischen zwei **abstrakten** Heap-Typen
/// gilt nur die Gleichheit — diese beiden sollen sich nicht auffangen.
///
/// GEMESSEN 03.10. (`type-subtyping.wast` Z. 227ff, Abschnitt „Invalid
/// abstract subtyping", 12 Faelle): die Suite weist zurueck
///
/// ```text
/// (module (func (param (ref null nofunc))
///                 (result (ref null any)) (local.get 0)))
/// ```
///
/// und ebenso `noextern` -> `any`, `none` -> `func`, `nofunc` -> `none`.
///
/// /// Ein **durchweg** flaches abstraktes Gitter kostet dagegen 282 Faelle
/// (34983 -> 34701, davon 16 im 2.0-Kern): `ref_cast.wast`,
/// `br_on_cast.wast`, `ref_eq.wast`, `ref_test.wast`, `i31.wast`,
/// `struct.wast`, `array.wast` brauchen `struct -> eq -> any`,
/// `i31 -> eq -> any` und `eq -> any` sehr wohl. Die Suite verlangt das
/// Gitter also an zwei Stellen entgegengesetzt; diese Funktion ist der
/// Versuch, es auf **Funktionssignaturen** zu beschraenken (Aufrufstelle
/// in `pruefe.rs`: nur der Funktionsrahmen und `return`).
///
/// GEMESSEN: **negative Bilanz, siehe `BERICHT_WASM_ABSTRAKT_SIGNATUR.txt`.
pub fn valtyp_passt_als_signatur(m: &Modul, a: &Valtyp, b: &Valtyp) -> bool {
    if let (Valtyp::Ref(ra), Valtyp::Ref(rb)) = (a, b)
        && let (Some(ia), Some(ib)) = (reftyp_abstrakt_index(ra), reftyp_abstrakt_index(rb))
        && ia != ib
    {
        // Zwei **verschiedene** abstrakte Heap-Typen: das gehoert zu keinem
        // Modultyp, also gibt es keinen Untertyp. Die `null`-Frage entscheidet
        // danach der normale Vergleich.
        return false;
    }
    valtyp_passt_als(m, a, b)
}

/// **Normalform eines `(rec …)`-Typs** fuer den Link-Vergleich.
///
/// GEMESSEN 03.10. (`type-subtyping.wast` Z. 652/746, Module einzeln
/// gefahren als `m25_Z652` und `m34_Z746`): der Importvergleich braucht
/// mehr als die Mitglieds-Signaturen. Die beiden Gruppen sind als
/// Signaturen gleich (`[func, func]` bzw. `[func, struct]`, Position 0),
/// unterscheiden sich aber **rekursiv**: die `(sub …)`-Kante bzw. ein
/// Feldtyp zeigt **aus der Gruppe heraus**. Das ist der Unterschied, den
/// die Suite mit `assert_unlinkable` markiert.
///
/// Warum eine Normalform und nicht der Typindex: beim Linken liegt das
/// **Modul der exportierenden Funktion nicht mehr vor** (der Store kennt
/// nur Signaturen). Die Normalform muss also alles, was der Vergleich
/// braucht, in sich tragen — und **gruppenbezogen** sein, weil
/// Typindizes modul-lokal sind.
///
/// * `Intern(p)`  = Typ `p` **innerhalb derselben** Gruppe (Position)
/// * `Extern(i)`  = Typ `i` **ausserhalb** (Rohindex, nur fuer den
///                   Vergleich zweier Normalformen desselben Paars)
/// * `Abstrakt(i)`= ein abstrakter Heap-Typ (`any`, `func`, …)
#[derive(Debug, Clone, Eq)]
pub enum Ziel {
    Intern(u32),
    /// Verweis **nach aussen**: Rohindex und — solange die Tiefe reicht —
    /// die Normalform der fremden Gruppe.
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 402ff/414ff,
    /// `ref.test (ref $g1) (ref.func $g)` mit `$g : $g2`): beide Gruppen
    /// zeigen mit ihrem Obertyp **nach aussen** (`$f1` bzw. `$f2`), der
    /// Rohindex allein verglich sie aber als verschieden (0 gegen 2) und
    /// lieferte 0 statt 1. Die **mitgenommene Form** der fremden Gruppe
    /// ist in beiden Faellen gleich ([func, struct mit `(ref Intern(0))`]),
    /// und genau dann stimmen die Gruppen ueberein.
    ///
    /// Im **Link**-Vergleich aendert diese Fassung nichts (GEMESSEN
    /// 04.10., `BERICHT_WASM_EXTERN_NESTED.txt`: 35001, vier gueltige
    /// Importe bleiben abgewiesen); `Ziel::Extern` und `Ziel::Intern`
    /// bleiben verschiedene Varianten, damit `m25_Z652`/`m34_Z746` zu
    /// Recht unlinkbar bleiben.
    ///
    /// M3/M5: `MAX_ZIEL_TIEFE` ist die harte Grenze — ab dort gilt jeder
    /// Verweis nach aussen als gleich (`Extern(_, None)`), ein Zyklus
    /// haengt nicht.
    Extern(u32, Option<std::sync::Arc<Vec<Mitglied>>>),
    Abstrakt(u32),
}

/// Zwei Ziele sind gleich nach ihrem **Kind** und ihrem Wert — auch
/// `Extern` vergleicht den Rohindex.
///
/// GEMESSEN 03.10., **beide Fassungen**:
///   * `Extern` vergleicht **nicht** (zwei Verweise nach aussen gelten
///     immer als gleich): **34998** — drei `assert_unlinkable` werden
///     angenommen, weil genau diese Relaxation `m25_Z652`/`m34_Z746`
///     durchwinkt.
///   * `Extern` vergleicht den Rohindex: **35001** — dafuer wird ein
///     **gueltiger** Import abgewiesen (`type-subtyping.wast` #66,
///     `M.f1`), weil Typindizes modul-lokal sind und der Rohindex
///     zweier Module nichts bedeutet.
///
/// Ausgeliefert ist die **strikte** Fassung (+1 gegenueber dem Stand ohne
/// Normalform). Der bekannte Restfehler ist damit benannt: **die volle
/// Entfaltung** — ein externer Verweis muss *rekursiv* in die Gruppe
/// verglichen werden, statt nur an seiner Existenz zu hängen. Das ist
/// der naechste Schritt, kein Geheimnis.
impl PartialEq for Ziel {
    fn eq(&self, other: &Ziel) -> bool {
        // **ACHTUNG:** `self == other` waere ein endloser Aufruf — es
        // kompiliert, lintet sauber und bricht erst zur Laufzeit mit
        // „stack overflow" ab (GEMESSEN 03.10., genau so passiert).
        match (self, other) {
            (Ziel::Intern(a), Ziel::Intern(b)) => a == b,
            // **Der Rohindex wird nicht verglichen** — er ist modul-lokal.
            // Verglichen wird die mitgebrachte Form; fehlt sie (Tiefengrenze
            // erreicht), gilt jeder Verweis nach aussen als gleich.
            (Ziel::Extern(_, Some(a)), Ziel::Extern(_, Some(b))) => a == b,
            (Ziel::Extern(_, None), Ziel::Extern(_, None)) => true,
            (Ziel::Abstrakt(a), Ziel::Abstrakt(b)) => a == b,
            _ => false,
        }
    }
}

/// Die Normalform einer `(rec …)`-Gruppe: alle Mitglieder in
/// Reihenfolge und die **Position des eigenen Typs** darin (`None`
/// ausserhalb jeder Gruppe).
///
/// GEMESSEN 04.10.: `clippy` nannte den ausgeschriebenen Typ an vier
/// Stellen „very complex type" — der Alias sagt zugleich, **was**
/// verglichen wird: Position **und** Mitgliederliste.
pub type Gruppenform = Option<(Vec<Mitglied>, u32)>;

/// Ein Werttyp in Normalform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// `i32`, `i64`, `f32`, `f64` (der Zahlentyp selbst).
    Zahl(u8),
    /// `(ref null? …)` — `None` heisst: nicht nullbar.
    Ref(bool, Option<Ziel>),
}

/// Ein Mitglied einer `(rec …)`-Gruppe in Normalform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mitglied {
    /// 0 = Funktion, 1 = Struktur, 2 = Array.
    pub art: u8,
    /// Die `(sub …)`-Kette, normalisiert (`None` = final).
    pub ober: Option<Ziel>,
    /// Parameter/Ergebnisse einer Funktion.
    pub parameter: Vec<Form>,
    pub ergebnis: Vec<Form>,
    /// Felder einer Struktur: Werttyp und Veraenderbarkeit.
    pub felder: Vec<(Form, bool)>,
    /// Elementtyp und Veraenderbarkeit eines Arrays.
    pub element: Option<(Form, bool)>,
}

/// Die Normalform der Gruppe, in der `t` liegt, und die Position von `t`.
///
/// `None`, wenn `t` ausserhalb jeder Gruppe liegt (dann gilt weiter der
/// einfache Signaturvergleich).
///
/// M3/M5: `MAX_GRUPPEN_TIEFE` ist die harte Grenze fuer die Verschachtelung
/// der Normalform — ein zyklischer Typ darf nicht haengen.
pub fn gruppen_normalform(m: &Modul, t: u32) -> Option<(Vec<Mitglied>, u32, u32)> {
    gruppen_normalform_tief(m, t, MAX_ZIEL_TIEFE)
}

/// Wie tief eine Normalform Verweise nach aussen **mitnimmt**.
///
/// GEMESSEN 04.10.: 3 reicht fuer die gemessenen Faelle und haelt die
/// Normalform klein. M3/M5: harte Grenze, ein Zyklus haengt nicht.
pub const MAX_ZIEL_TIEFE: usize = 3;

/// Wie `gruppen_normalform`, aber mit **Tiefenrest**: jeder Verweis nach
/// aussen nimmt die Normalform der fremden Gruppe mit, solange `tiefe > 0`.
fn gruppen_normalform_tief(m: &Modul, t: u32, tiefe: usize) -> Option<(Vec<Mitglied>, u32, u32)> {
    const MAX_GRUPPEN_TIEFE: usize = 256;
    let (start, ende) = m.gruppe_von.get(t as usize).copied().flatten()?;
    let ziel_von = |typ: u32, start: u32, ende: u32| -> Ziel {
        if ist_wildcard(typ) {
            Ziel::Abstrakt(typ)
        } else if typ >= start && typ < ende {
            Ziel::Intern(typ - start)
        } else {
            // Nach aussen: die fremde Gruppe **mitnehmen**, solange die
            // Tiefe reicht (M3/M5: `MAX_ZIEL_TIEFE`).
            let form = if tiefe == 0 {
                None
            } else {
                gruppen_normalform_tief(m, typ, tiefe - 1)
                    .map(|(glieder, _, _)| std::sync::Arc::new(glieder))
            };
            Ziel::Extern(typ, form)
        }
    };
    let form_von = |v: &Valtyp, start: u32, ende: u32| -> Form {
        match v {
            Valtyp::I32 => Form::Zahl(0),
            Valtyp::I64 => Form::Zahl(1),
            Valtyp::F32 => Form::Zahl(2),
            Valtyp::F64 => Form::Zahl(3),
            Valtyp::Ref(r) => match r {
                Reftyp::Konkret { typ, nullable } => {
                    Form::Ref(*nullable, Some(ziel_von(*typ, start, ende)))
                }
                andere => Form::Ref(
                    andere.ist_nullable(),
                    Some(ziel_von(
                        match andere {
                            Reftyp::Func => JEDER_FUNKTIONSTYP,
                            Reftyp::Extern => JEDER_EXTERNTYP,
                            _ => KEIN_TYP,
                        },
                        start,
                        ende,
                    )),
                ),
            },
        }
    };
    let mut mitglieder: Vec<Mitglied> = Vec::new();
    if (ende - start) as usize > MAX_GRUPPEN_TIEFE {
        return None;
    }
    for i in start..ende {
        let strukt = m.strukt.get(i as usize).and_then(|x| x.as_ref());
        let array = m.arrays.get(i as usize).and_then(|x| x.as_ref());
        let (art, parameter, ergebnis, felder, element) = match (strukt, array) {
            (Some(s), _) => (
                1u8,
                Vec::new(),
                Vec::new(),
                s.felder
                    .iter()
                    .map(|f| (form_von(&f.typ, start, ende), f.veraenderbar))
                    .collect(),
                None,
            ),
            (_, Some(a)) => (
                2u8,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some((form_von(&a.inhalt, start, ende), a.veraenderbar)),
            ),
            _ => {
                let ft = m.typen.get(i as usize);
                (
                    0u8,
                    ft.map(|f| {
                        f.parameter
                            .iter()
                            .map(|v| form_von(v, start, ende))
                            .collect()
                    })
                    .unwrap_or_default(),
                    ft.map(|f| {
                        f.ergebnis
                            .iter()
                            .map(|v| form_von(v, start, ende))
                            .collect()
                    })
                    .unwrap_or_default(),
                    Vec::new(),
                    None,
                )
            }
        };
        mitglieder.push(Mitglied {
            art,
            ober: m
                .sub_ober
                .get(i as usize)
                .copied()
                .flatten()
                .map(|o| ziel_von(o, start, ende)),
            parameter,
            ergebnis,
            felder,
            element,
        });
    }
    Some((mitglieder, t - start, start))
}

/// Ist der Typindex ein Array-Typ des Moduls?
///
/// GEMESSEN 03.10. (`table_init.wast` Z. 790).
pub fn ist_array_typ(m: &Modul, typ: u32) -> bool {
    m.arrays.get(typ as usize).is_some_and(|a| a.is_some())
}

/// Ist der Typindex ein STRUKTURtyp des Moduls?
///
/// GEMESSEN 03.10. (`extern.wast`).
pub fn ist_struct_typ(m: &Modul, typ: u32) -> bool {
    m.strukt.get(typ as usize).is_some_and(|a| a.is_some())
}

/// Der Struktur-Typ hinter einem Index (falls es einer ist).
pub fn struct_typ(m: &Modul, typ: u32) -> Option<&StructTyp> {
    m.strukt.get(typ as usize).and_then(|a| a.as_ref())
}

/// Der Array-Typ hinter einem Index (falls es einer ist).
pub fn array_typ(m: &Modul, typ: u32) -> Option<&ArrayTyp> {
    m.arrays.get(typ as usize).and_then(|a| a.as_ref())
}

/// Sind zwei Typindizes STRUKTURELL gleich? `(type (func))` und ein
/// Inline-Typ `(func)` sind zwei Eintraege, aber derselbe Typ.
///
/// GEMESSEN 02.10. (`br_table.wast`: `(type $t (func))` neben
/// `(func $tf)` — beide `(func)`, aber Index 11 und 0).
///
/// **Koinduktion (GEMESSEN 03.10., Absturz `type-equivalence.wast`):**
/// Seit der GC-Spezifikation darf ein Typ sich selbst referenzieren, auch
/// ohne `rec`:
///     (module (type $t1 (func (param i32 (ref $t1))))
///             (type $t2 (func (param i32 (ref $t2))))
///             (func $f1 (param $r (ref $t1)) (call $f2 (local.get $r))))
/// Der Vergleich von `$t1` und `$t2` stieg dabei ohne Ende in die
/// Tiefe: `typen_gleich` -> `valtyp_gleich` -> `typen_gleich` mit
/// `(t1,t2)`, `(t1,t2)`, … bis der Stapel ueberlief (`fatal runtime
/// error: stack overflow`). Die Spezifikation definiert Typgleichheit als
/// groessten Fixpunkt: liegt ein Paar schon als gleich angenommen, gilt es
/// als gleich. Genau das macht die Paarmenge unten.
pub fn typen_gleich(m: &Modul, a: u32, b: u32) -> bool {
    let mut angenommen = HashSet::new();
    typen_gleich_intern(m, a, b, &mut angenommen)
}

/// Hoechstzahl angenommenen Typpaaren (M3). Jedes Paar kommt hoechstens
/// einmal in die Menge, im Normalfall also `typen.len() * typen.len()`.
/// Die Grenze ist nur ein Sicherheitsnetz fuer kaputte Modelle; dann gilt
/// das Paar als UNGLEICH (Ablehnen ist der sichere Zweig).
const MAX_TYPPAARE: usize = 100_000;

/// Vergleich mit bereits angenommenen Paaren (Koinduktion).
fn typen_gleich_intern(m: &Modul, a: u32, b: u32, angenommen: &mut HashSet<(u32, u32)>) -> bool {
    if a == b {
        return true;
    }
    // `(ref func)` passt auf JEDEN Typindex (jeder Typindex ist in
    // unserem Modell ein Funktionstyp); die beiden Wildcards sind unter
    // sich gleich. `(ref extern)` passt dagegen auf keinen Typindex —
    // eine Funktion ist keine externe Referenz.
    //
    // GEMESSEN 03.10. (`table.wast` Z. 30: `(ref extern)`).
    if ist_wildcard(a) && ist_wildcard(b) {
        return true;
    }
    // `(ref func)` passt auf jeden FUNKTIONStyp, nicht auf einen Arraytyp.
    if a == JEDER_FUNKTIONSTYP {
        return !ist_array_typ(m, b);
    }
    if b == JEDER_FUNKTIONSTYP {
        return !ist_array_typ(m, a);
    }
    // `arrayref`/`(ref array)` passt auf jeden Arraytyp …
    //
    // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
    if a == JEDER_ARRAYTYP {
        return ist_array_typ(m, b);
    }
    if b == JEDER_ARRAYTYP {
        return ist_array_typ(m, a);
    }
    // … und `externref` auf nichts anderen (externe Referenzen haben in
    // unserem Modell keine Typindizes).
    if ist_wildcard(a) || ist_wildcard(b) {
        return false;
    }
    // Zwei Arraytypen: Inhalt und Veraenderbarkeit vergleichen. Die
    // Inhalte koennen wieder Typindizes tragen (verschachtelte Arrays).
    if let (Some(x), Some(y)) = (array_typ(m, a), array_typ(m, b)) {
        return x.veraenderbar == y.veraenderbar
            && valtyp_gleich_intern(m, &x.inhalt, &y.inhalt, angenommen);
    }
    // Koinduktionsschritt: liegt das Paar schon vor, gilt es als gleich.
    // Sonst wird es eingetragen und der Inhalt verglichen.
    if angenommen.len() >= MAX_TYPPAARE {
        return false;
    }
    if !angenommen.insert((a, b)) {
        return true;
    }
    match (m.typen.get(a as usize), m.typen.get(b as usize)) {
        (Some(x), Some(y)) => {
            x.parameter.len() == y.parameter.len()
                && x.ergebnis.len() == y.ergebnis.len()
                && x.parameter
                    .iter()
                    .zip(y.parameter.iter())
                    .all(|(p, q)| valtyp_gleich_intern(m, p, q, angenommen))
                && x.ergebnis
                    .iter()
                    .zip(y.ergebnis.iter())
                    .all(|(p, q)| valtyp_gleich_intern(m, p, q, angenommen))
        }
        _ => false,
    }
}

/// Werttyp-Gleichheit mit Typindizes (siehe `typen_gleich`).
pub fn valtyp_gleich(m: &Modul, a: &Valtyp, b: &Valtyp) -> bool {
    let mut angenommen = HashSet::new();
    valtyp_gleich_intern(m, a, b, &mut angenommen)
}

/// Werttyp-Gleichheit mit gemeinsamer Paarmenge (siehe `typen_gleich`).
fn valtyp_gleich_intern(
    m: &Modul,
    a: &Valtyp,
    b: &Valtyp,
    angenommen: &mut HashSet<(u32, u32)>,
) -> bool {
    match (a, b) {
        (
            Valtyp::Ref(Reftyp::Konkret { typ: x, .. }),
            Valtyp::Ref(Reftyp::Konkret { typ: y, .. }),
        ) => typen_gleich_intern(m, *x, *y, angenommen),
        _ => a == b,
    }
}

/// Typvergleich fuer den IMPORT einer globalen Variablen.
///
/// Regel der Spezifikation: der Import darf einen **allgemeineren** Typ
/// nennen als der Export anbietet — `export <: import`. Bei einer
/// veraenderbaren Variablen (der Platz wird geteilt) muessen die Typen
/// dagegen GLEICH sein.
///
/// GEMESSEN 03.10. (`linking.wast` Z. 112-200: 9 gueltige und 17
/// `assert_unlinkable`-Faelle). Die Tabelle dort, mit
/// `export -> import`:
///   (ref null func) -> (ref null func)   gueltig
///   (ref func)      -> (ref null func)   gueltig  (allgemeiner)
///   (ref $t)        -> (ref null func)   gueltig  (allgemeiner)
///   (ref null $t)   -> (ref null $t)     gueltig  (gleich)
///   (ref $t)        -> (ref $t)          gueltig  (gleich)
///   (ref null func) -> (ref func)        UNGUELTIG (spezifischer)
///   (ref func)      -> (ref null $t)     UNGUELTIG (spezifischer)
///   (ref null $t)   -> (ref $t)          UNGUELTIG (nicht nullable)
///   (mut (ref func))-> (mut (ref null func)) UNGUELTIG (geteilt!)
pub fn reftyp_import_passt_als(export: &Reftyp, import: &Reftyp, veraenderbar: bool) -> bool {
    use Reftyp::*;
    // Veraenderbare Variablen werden geteilt — dort gilt Gleichheit.
    if veraenderbar {
        return reftyp_gleich_als_import(export, import);
    }
    match (export, import) {
        (Func, Func) | (Extern, Extern) => true,
        (Konkret { .. }, Func) => {
            // `funcref` ist Obertyp jedes konkreten Funktionsverweises.
            !ist_array_typ_index(export)
        }
        (Konkret { .. }, Extern) => false,
        (
            Konkret {
                typ: x,
                nullable: nx,
            },
            Konkret {
                typ: y,
                nullable: ny,
            },
        ) => {
            if x == y {
                // Der Import darf nur „nullable" sein, wenn der Export es
                // auch ist — er darf NICHT das nullable gegen das nicht
                // Nullable tauschen.
                return *ny || !*nx;
            }
            // `(ref func)` (nicht nullable) deckt `(ref $t)` ohne null.
            if !*ny && (*y == JEDER_FUNKTIONSTYP || *y == JEDER_EXTERNTYP || *y == JEDER_ARRAYTYP) {
                return !*nx;
            }
            false
        }
        _ => false,
    }
}

/// Gleichheit fuer den Import einer veraenderbaren Variablen.
fn reftyp_gleich_als_import(export: &Reftyp, import: &Reftyp) -> bool {
    use Reftyp::*;
    match (export, import) {
        (Func, Func) | (Extern, Extern) => true,
        (
            Konkret {
                typ: x,
                nullable: nx,
            },
            Konkret {
                typ: y,
                nullable: ny,
            },
        ) => x == y && nx == ny,
        _ => false,
    }
}

/// Ein konkreter Referenztyp, der auf einen Array-Typ zeigt?
fn ist_array_typ_index(r: &Reftyp) -> bool {
    match r {
        Reftyp::Konkret { typ, .. } => *typ == JEDER_ARRAYTYP,
        _ => false,
    }
}

/// Subtyping fuer Referenztypen mit Modulbezug (Typindizes werden
/// strukturell verglichen).
pub fn reftyp_passt_als(m: &Modul, a: &Reftyp, b: &Reftyp) -> bool {
    let mut angenommen = HashSet::new();
    reftyp_passt_als_intern(m, a, b, &mut angenommen)
}

/// Subtyping mit gemeinsamer Paarmenge (siehe `typen_gleich`).
/// Laeuft die Kette der ausdruecklich deklarierten **Obertypen** von
/// `a` ab und liefert sie `b`? (`(type $a (sub $b …))`)
///
/// GEMESSEN 03.10. (`type-subtyping.wast`). `schritte` ist die
/// M3/M4-Grenze: die Kette ist zyklisch, wenn das Modul sie zulaesst,
/// und darf dann nicht haengen (MAX_SUBKETTE).
/// Gehoert der Typindex `a` ueber die `(sub …)`-Kette zum Typindex `b`?
/// (`a == b` zaehlt dazu.)
///
/// GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, Abschnitt „Runtime
/// types", 7 `assert_trap`-/`assert_return`-Faelle): `call_indirect` muss
/// den **Typindex** der aufgerufenen Funktion pruefen, und
/// `$t1 ⊑ $t0` gilt ueber `(type $t1 (sub $t0 …))`. Der Vergleich der
/// Signaturen allein verlangt in diesen Faellen faelschlich einen Trap.
pub fn subtyp_ueber_kette(m: &Modul, a: u32, b: u32) -> bool {
    sub_kette_erreicht(m, a, b, 0)
}

/// Wie `subtyp_ueber_kette`, aber nur mit der Kettentabelle — das braucht
/// der **Interpreter**, der das Modul nicht mehr hat, sondern nur den
/// Kontext (`ModulKontext::sub_ober`).
///
/// M3/M5: `MAX_SUBKETTE` ist die harte Grenze, eine zyklische Kette
/// haengt nicht.
pub fn subtyp_ueber_kette_in(sub_ober: &[Option<u32>], a: u32, b: u32) -> bool {
    const MAX_SUBKETTE: usize = 256;
    let mut aktuell = a;
    for _ in 0..MAX_SUBKETTE {
        if aktuell == b {
            return true;
        }
        match sub_ober.get(aktuell as usize).copied().flatten() {
            Some(naechste) => aktuell = naechste,
            None => return false,
        }
    }
    false
}

/// Die **Definition** von `a` ist ein Subtyp der Definition von `b`?
///
/// Das ist die Regel fuer eine `(sub $b (…))`-Deklaration: der deklarierte
/// Typ muss zum deklarierten **Obertyp** passen (Wasm 3.0, „sub type").
///
/// GEMESSEN 04.10. (`type-subtyping.wast` Z. 780–950, 19 `assert_invalid`).
/// Die Suite verlangt in einem Zug:
///
/// * **gleiche Klasse** — `(sub $a0 (struct))` ueber einem `(array i32)`
///   ist ungueltig, ebenso func ueber struct und array ueber func.
/// * **Funktionen**: gleiche **Arity**, Parameter **kontravariant**,
///   Ergebnisse **kovariant** — `(func (param i32))` passt **nicht** zu
///   `(func)`, obwohl es sie funktional erweitert.
/// * **Strukturen**: **Breite** — ein Typ mit MEHR Feldern ist ein Subtyp
///   eines mit weniger (Z. 168ff: ein`(sub $s1 (struct …5 Felder)))`).
///   Veraenderbarkeit muss feldweise gleich sein.
/// * **Arrays**: Veraenderbarkeit muss gleich sein, und der Elementtyp ist
///   bei `(mut …)` **invariant**, sonst **kovariant** (Z. 880–902).
///
/// Die vier `(ref none)`/`(ref any)`-Faelle dieser Liste entscheidet das
/// vorhandene GC-Gitter (`Klasse::passt_in`).
pub fn def_subtyp(m: &Modul, a: u32, b: u32) -> bool {
    const MAX_DEF_TIEFE: usize = 32;
    def_subtyp_tief(m, a, b, MAX_DEF_TIEFE)
}

/// Die harte Tiefengrenze von `def_subtyp` (M3/M5): ein zyklischer Typ
/// darf die Pruefung nicht haengen lassen.
pub const MAX_DEF_TIEFE: usize = 32;

fn def_subtyp_tief(m: &Modul, a: u32, b: u32, tiefe: usize) -> bool {
    if tiefe == 0 {
        return false;
    }
    // Gleiche Art: struct/array/func. Alles, was nicht in `strukt` oder
    // `arrays` steht, ist im Modell eine Funktionssignatur.
    let a_array = ist_array_typ(m, a);
    let b_array = ist_array_typ(m, b);
    let a_strukt = ist_struct_typ(m, a);
    let b_strukt = ist_struct_typ(m, b);
    if a_array != b_array || a_strukt != b_strukt {
        return false;
    }
    if a_array {
        let (Some(x), Some(y)) = (
            m.arrays.get(a as usize).and_then(|x| x.as_ref()),
            m.arrays.get(b as usize).and_then(|x| x.as_ref()),
        ) else {
            return false;
        };
        // Veraenderbarkeit muss uebereinstimmen; der Elementtyp ist bei
        // `(mut …)` invariant, sonst kovariant.
        if x.veraenderbar != y.veraenderbar || x.bits != y.bits {
            return false;
        }
        if x.veraenderbar {
            valtyp_subtyp(m, &x.inhalt, &y.inhalt, tiefe - 1)
                && valtyp_subtyp(m, &y.inhalt, &x.inhalt, tiefe - 1)
        } else {
            valtyp_subtyp(m, &x.inhalt, &y.inhalt, tiefe - 1)
        }
    } else if a_strukt {
        let (Some(x), Some(y)) = (
            m.strukt.get(a as usize).and_then(|x| x.as_ref()),
            m.strukt.get(b as usize).and_then(|x| x.as_ref()),
        ) else {
            return false;
        };
        // Breite: `a` braucht mindestens so viele Felder wie `b`.
        if x.felder.len() < y.felder.len() {
            return false;
        }
        x.felder.iter().zip(y.felder.iter()).all(|(xf, yf)| {
            xf.veraenderbar == yf.veraenderbar
                && xf.bits == yf.bits
                && valtyp_subtyp(m, &xf.typ, &yf.typ, tiefe - 1)
        })
    } else {
        let (Some(x), Some(y)) = (m.typen.get(a as usize), m.typen.get(b as usize)) else {
            return false;
        };
        if x.parameter.len() != y.parameter.len() || x.ergebnis.len() != y.ergebnis.len() {
            return false;
        }
        // Parameter kontravariant, Ergebnisse kovariant.
        x.parameter
            .iter()
            .zip(y.parameter.iter())
            .all(|(xt, yt)| valtyp_subtyp(m, yt, xt, tiefe - 1))
            && x.ergebnis
                .iter()
                .zip(y.ergebnis.iter())
                .all(|(xt, yt)| valtyp_subtyp(m, xt, yt, tiefe - 1))
    }
}

/// `a` (als Werttyp) ist ein Subtyp von `b`?
fn valtyp_subtyp(m: &Modul, a: &Valtyp, b: &Valtyp, tiefe: usize) -> bool {
    match (a, b) {
        (Valtyp::I32, Valtyp::I32)
        | (Valtyp::I64, Valtyp::I64)
        | (Valtyp::F32, Valtyp::F32)
        | (Valtyp::F64, Valtyp::F64) => true,
        (Valtyp::Ref(ra), Valtyp::Ref(rb)) => {
            let null_a = reftyp_nullbar(ra);
            let null_b = reftyp_nullbar(rb);
            // `(ref $t)` passt nicht auf `(ref null $t)` herunter.
            if null_a && !null_b {
                return false;
            }
            let ka = reftyp_klasse(m, ra);
            let kb = reftyp_klasse(m, rb);
            if !ka.passt_in(kb) {
                return false;
            }
            match (ka, kb) {
                // Zwei Typindizes: gleiche Definition genuegt (die
                // Koinduktion aus `typen_gleich`).
                //
                // GEMESSEN 04.10. (`type-subtyping.wast` Z. 989ff:
                // „Transitive equivalent subtyping" — `$b3 <: $a3` ueber
                // `$b1`/`$a1`, zwei **verschiedene**, aber strukturgleiche
                // Ketten; der Indexvergleich haette sie verworfen).
                (Klasse::Konkret, Klasse::Konkret) => {
                    if tiefe == 0 {
                        return false;
                    }
                    match (reftyp_index(ra), reftyp_index(rb)) {
                        (Some(ia), Some(ib)) => typen_gleich_intern(m, ia, ib, &mut HashSet::new()),
                        _ => false,
                    }
                }
                _ => true,
            }
        }
        _ => false,
    }
}

/// Darf bei diesem Referenztyp `null` stehen?
fn reftyp_nullbar(r: &Reftyp) -> bool {
    match r {
        Reftyp::Konkret { nullable, .. } => *nullable,
        // `funcref`/`externref` sind die nullbaren Abstrakttypen.
        Reftyp::Func | Reftyp::Extern => true,
    }
}

/// Der Typindex eines Referenztyps, falls er einen traegt.
fn reftyp_index(r: &Reftyp) -> Option<u32> {
    match r {
        Reftyp::Konkret { typ, .. } => Some(*typ),
        Reftyp::Func | Reftyp::Extern => None,
    }
}

/// Der deklarierte Typindex einer Funktion (Wasm-3.0-(sub …)-Ketten).
pub fn funktion_typindex(f: &Func) -> u32 {
    match f {
        Func::Eigen { typ, .. } | Func::Importiert { typ, .. } => *typ,
    }
}

fn sub_kette_erreicht(m: &Modul, a: u32, b: u32, schritte: usize) -> bool {
    const MAX_SUBKETTE: usize = 256;
    if schritte > MAX_SUBKETTE {
        return false;
    }
    if a == b {
        return true;
    }
    match m.sub_ober.get(a as usize).copied().flatten() {
        Some(ober) => sub_kette_erreicht(m, ober, b, schritte + 1),
        None => false,
    }
}

/// Wie `sub_kette_erreicht`, aber jeder Kettenpunkt zaehlt auch dann als
/// Treffer, wenn er **strukturell gleich** zu `b` ist.
///
/// GEMESSEN 03.10. (`type-subtyping.wast`, Modul aus Z. 90ff — die Suite
/// fuehrt es):
///
///   (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
///   (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
///   (rec (type $g1 (sub $f1 (func))) (type (struct)))
///   (rec (type $g2 (sub $f2 (func))) (type (struct)))
///   (func $g (type $g2))
///   (global (ref $g1) (ref.func $g))
///
/// `$g2` muss zu `$g1` passen: die Ketten fuehren auf `$f2` bzw. `$f1`, und
/// die beiden sind **strukturell gleich** (beide `(func)`). Mit der
/// Index-Vergleich fand die Kette nichts, und der Global-Initialisierer
/// wurde abgewiesen: „Globale 0: Typ Ref(6) statt Ref(2)".
///
/// M3/M5: `schritte` ist die harte Grenze (`MAX_SUBKETTE`), die Kette darf
/// zyklisch sein und darf nicht haengen.
#[allow(dead_code)] // GEMESSEN zu permissiv (34969 statt 34983) — Beleg, nicht benutzt.
fn sub_kette_gleich(
    m: &Modul,
    a: u32,
    b: u32,
    schritte: usize,
    _angenommen: &mut HashSet<(u32, u32)>,
) -> bool {
    const MAX_SUBKETTE: usize = 256;
    if schritte > MAX_SUBKETTE {
        return false;
    }
    if a == b {
        return true;
    }
    let angenommen = &mut HashSet::new();
    // Die beiden Fassungen mit struktureller Gleichheit sind gemessen und
    // widerlegt (siehe Aufrufstelle); diese Funktion bleibt als Beleg, mit
    // dem einfacheren „strukturgleich"-Zweig fuer den Vergleich.
    if typen_gleich_intern(m, a, b, angenommen) {
        return true;
    }
    match m.sub_ober.get(a as usize).copied().flatten() {
        Some(ober) => sub_kette_gleich(m, ober, b, schritte + 1, angenommen),
        None => false,
    }
}

fn reftyp_passt_als_intern(
    m: &Modul,
    a: &Reftyp,
    b: &Reftyp,
    // GEMESSEN 03.10.: im ausgelieferten Weg wird die Paarmenge hier nicht
    // benutzt — die Koinduktion ist aus dem Matching verschwunden
    // (Index-Identitaet plus `(sub …)`-Kette), sie lebt in `typen_gleich`.
    // Der Beleg `sub_kette_gleich` unten nimmt sie wieder auf; er ist
    // gemessen zu permissiv (34969 statt 34983).
    _angenommen: &mut HashSet<(u32, u32)>,
) -> bool {
    // Das GC-Lattice entscheidet zuerst die KLASSE, dann die `null`-
    // Zulaessigkeit. Die alte Fassung kannte nur `func`/`extern` und
    // stufte jeden Typindex als Funktion ein — `anyref`, `nullref` und
    // die leeren Typen fehlten.
    //
    // GEMESSEN 03.10. (`ref_null.wast`): 32 Faelle mit `anyref`,
    // `nullref`, `exnref`, `nullfuncref`, `nullexternref`.
    let ka = reftyp_klasse(m, a);
    let kb = reftyp_klasse(m, b);
    if !ka.passt_in(kb) {
        return false;
    }
    if ka == Klasse::Konkret && kb == Klasse::Konkret {
        // Zwei Typindizes muessen STRUKTURELL gleich sein (Koinduktion,
        // siehe `typen_gleich`).
        //
        // WICHTIG: kein `return` — die `null`-Pruefung unten gilt auch
        // hier. `(ref null $t)` erfuellt `(ref $t)` nicht.
        //
        // GEMESSEN 03.10. (`ref_as_non_null.wast` Z. 33ff:
        // `(func $g (param (ref $t)) …) (func (call $g (ref.null $t)))`
        // wurde mit dem fruehen `return` akzeptiert).
        let (ia, ib) = match (a, b) {
            (Reftyp::Konkret { typ: x, .. }, Reftyp::Konkret { typ: y, .. }) => (*x, *y),
            _ => (0, 0),
        };
        // GEMESSEN 03.10. (`type-rec.wast` Z. 51ff): die **Identitaet** der
        // Typindizes entscheidet, nicht ihre strukturelle Gleichheit.
        // Die Suite fuehrt als `assert_invalid`:
        //
        //   (rec (type $f1 (func)) (type (struct (field (ref $f1)))))
        //   (rec (type $f2 (func)) (type (struct (field (ref $f1)))))
        //   (func $f (type $f2))
        //   (global (ref $f1) (ref.func $f))
        //
        // `$f1` und `$f2` sind beide `(func)` — strukturell gleich, aber
        // der Global-Initialisierer verlangt **denselben** Typindex. Mit der
        // Koinduktion an dieser Stelle wurde das Modul angenommen.
        //
        // Die Koinduktion bleibt fuer die **Aequivalenz** (`typen_gleich`,
        // `type-equivalence.wast`) unveraendert — dort ist sie richtig und
        // wird weiter benutzt.
        let gleich = ia == ib;
        if !gleich {
            // **(sub …)-Deklarationen** (Wasm 3.0 GC): `$a` passt zu `$b`,
            // wenn die Obertypen-Kette von `$a` bei `$b` vorbeikommt.
            //
            // GEMESSEN 03.10. (`type-subtyping.wast` 33/74 mit 249
            // `(sub …)`-Vorkommen; ohne diese Kette fielen die
            // `assert_return`-Faelle, weil jeder deklarierte Untertyp als
            // fremd galt).
            // GEMESSEN 03.10.: eine Kette, deren Punkte auch **strukturell**
            // gleich zum Ziel zaehlen (`sub_kette_gleich`, bleibt im
            // Quelltext als Beleg), verliert **14 Faelle** — 34983 -> 34969.
            // Sie ist damit zu permissiv; massgeblich bleibt der
            // Index-Vergleich. `type-subtyping.wast` fuehrt Module, in denen
            // genau das noetig waere (`$g2 (sub $f2)` zu `$g1 (sub $f1)` mit
            // strukturell gleichen `$f2`/`$f1`) — sie bleiben deshalb rot,
            // und der Grund ist hier benannt statt kaschiert.
            // GEMESSEN 03.10., zwei Versuche, beide **widerlegt**:
            // (1) jeder Kettenpunkt zaehlt strukturell gleich  -> 34969 (-14)
            // (2) nur wenn er zusaetzlich **final** ist         -> 34979 (-4)
            // Beide sind zu permissiv. Massgeblich bleibt der Index-Vergleich.
            if !sub_kette_erreicht(m, ia, ib, 0) {
                return false;
            }
        }
    }
    // `null`: `(ref X)` (ohne `null`) erfuellt `(ref null X)`, aber nicht
    // umgekehrt.
    b.ist_nullable() || !a.ist_nullable()
    /* ALT ---------------------------------------------------------------
    match (a, b) {
        (Reftyp::Func, Reftyp::Func) => true,
        (Reftyp::Extern, Reftyp::Extern) => true,
        // `(ref null $t)` erfuellt `(ref null func)`.
        (Reftyp::Konkret { .. }, Reftyp::Func) => true,
        // `(ref extern)` erfuellt `externref`; umgekehrt erfuellt
        // `externref` NUR die nullbare Form.
        //
        // GEMESSEN 03.10. (`br_on_null` auf einem `(result externref)`
        // -Block: „externref erwartet, typreferenz auf dem Stapel").
        (
            Reftyp::Konkret {
                typ: JEDER_EXTERNTYP,
                ..
            },
            Reftyp::Extern,
        ) => true,
        (
            Reftyp::Extern,
            Reftyp::Konkret {
                typ: JEDER_EXTERNTYP,
                nullable: true,
            },
        ) => true,
        (
            Reftyp::Func,
            Reftyp::Konkret {
                typ: JEDER_FUNKTIONSTYP,
                nullable: true,
            },
        ) => true,
        // `(ref $t)` <: `(ref null $t)` (der Quelltyp darf strenger sein),
        // aber `(ref null $t)` ist KEIN Subtyp von `(ref $t)` — dort ist
        // `null` nicht erlaubt.
        (
            Reftyp::Konkret {
                typ: x,
                nullable: nx,
            },
            Reftyp::Konkret {
                typ: y,
                nullable: ny,
            },
        ) => typen_gleich_intern(m, *x, *y, angenommen) && (!*nx || *ny),
        _ => false,
    }
    --------------------------------------------------------------- ALT */
}

impl Valtyp {
    /// Darf ein Wert dieses Typs `null` sein?
    pub fn ist_nullable(&self) -> bool {
        match self {
            Valtyp::Ref(r) => r.ist_nullable(),
            _ => false,
        }
    }

    pub fn byte(&self) -> u8 {
        match self {
            Valtyp::I32 => 0x7f,
            Valtyp::I64 => 0x7e,
            Valtyp::F32 => 0x7d,
            Valtyp::F64 => 0x7c,
            Valtyp::Ref(r) => r.byte(),
        }
    }
    pub fn von_byte(b: u8) -> Option<Valtyp> {
        match b {
            0x7f => Some(Valtyp::I32),
            0x7e => Some(Valtyp::I64),
            0x7d => Some(Valtyp::F32),
            0x7c => Some(Valtyp::F64),
            // 2026-10-15: also the abstract heap types of Wasm 3.0 (GC,
            // exceptions) as value types — `Reftyp::von_byte` knew them, the
            // value-type reader did not. MEASURED: every assertion wrapper of
            // the WPT `wasm/core` suite imports `spectest.hostref`
            // (`(i32) -> anyref`) and `spectest.eq_ref` (`(eqref eqref) ->
            // i32`), so without them no `assert_return` could compile.
            0x70 | 0x6f | 0x6e | 0x6d | 0x6c | 0x6b | 0x6a | 0x69 | 0x71 | 0x72 | 0x73 | 0x74 => {
                Some(Valtyp::Ref(Reftyp::von_byte(b)?))
            }
            _ => None,
        }
    }
    pub fn text(&self) -> &'static str {
        match self {
            Valtyp::I32 => "i32",
            Valtyp::I64 => "i64",
            Valtyp::F32 => "f32",
            Valtyp::F64 => "f64",
            Valtyp::Ref(r) => r.text(),
        }
    }
    /// Wird der Typ auf dem Stapel benutzt? (Referenztypen brauchen
    /// `ref.is_null`, sonst nicht - `select` ohne Typen zaehlt nicht.)
    pub fn ist_referenz(&self) -> bool {
        matches!(self, Valtyp::Ref(_))
    }
    pub fn aus_text(t: &str) -> Option<Valtyp> {
        match t {
            "i32" => Some(Valtyp::I32),
            // `i8`/`i16` sind die PACKED Typen des GC-Vorschlags: sie
            // werden in einem Array gespeichert und NICHT als Wert
            // benutzt. Fuer das Modell sind sie `i32` — der Unterschied
            // zeigt sich erst beim Lesen und Schreiben (`array.get_u`).
            //
            // GEMESSEN 03.10. (`extern.wast` Z. 5: `(type $at (array i8))`).
            "i8" | "i16" => Some(Valtyp::I32),
            "i64" => Some(Valtyp::I64),
            "f32" => Some(Valtyp::F32),
            "f64" => Some(Valtyp::F64),
            "funcref" => Some(Valtyp::Ref(Reftyp::Func)),
            "externref" => Some(Valtyp::Ref(Reftyp::Extern)),
            // `arrayref` = `(ref null array)` (Wasm 3.0 GC).
            // GEMESSEN 03.10. (`table_init.wast` Z. 790).
            "arrayref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_ARRAYTYP,
                nullable: true,
            })),
            // Die GC-Kurzformen (Wasm 3.0 GC): `(ref null …)` zu jedem
            // Heaptyp des Lattices.
            //
            // GEMESSEN 03.10. (`ref_null.wast`: 32 Faelle).
            "anyref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_TYP,
                nullable: true,
            })),
            "nullref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: KEIN_TYP,
                nullable: true,
            })),
            "eqref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_EQTYP,
                nullable: true,
            })),
            "i31ref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_I31TYP,
                nullable: true,
            })),
            "structref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_STRUKTYP,
                nullable: true,
            })),
            "exnref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: JEDER_EXNTYP,
                nullable: true,
            })),
            "nullexnref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: KEIN_EXNTYP,
                nullable: true,
            })),
            "nullfuncref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: KEIN_FUNKTIONSTYP,
                nullable: true,
            })),
            "nullexternref" => Some(Valtyp::Ref(Reftyp::Konkret {
                typ: KEIN_EXTERNTYP,
                nullable: true,
            })),
            _ => None,
        }
    }
}

/// Lesen/Schreiben eines Werttyps aus einem Lesekopf (`bytes::Leser`).
pub fn valtyp_lesen(b: u8) -> Result<Valtyp, Fehler> {
    Valtyp::von_byte(b).ok_or(Fehler::form(
        ART_REFTYP,
        format!("Werttyp-Byte {b:#04x} unbekannt"),
    ))
}

/// Der Werttyp als Zahl, wie ihn `ART_REFTYP` nennt.
pub fn reftyp_ungueltig(t: impl std::fmt::Display) -> Fehler {
    Fehler::form(
        ART_REFTYP,
        format!("Referenztyp {t} unbekannt (erlaubt: funcref, externref)"),
    )
}

/// Der Typindex-Fehler des Validators.
pub fn typ_ungueltig(i: u32) -> Fehler {
    Fehler::ungueltig(V_TYP, format!("Typindex {i} unbekannt"))
}

/// Limits (min/max) fuer Speicher oder Tabelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min: u64,
    pub max: Option<u64>,
}

impl Limits {
    /// Passt `min <= max`? Der Decoder ruft das fuer jede Tabelle/Speicher.
    pub fn passt(&self, _was: &str) -> bool {
        match self.max {
            None => true,
            Some(m) => self.min <= m,
        }
    }
    pub fn text(&self) -> String {
        match self.max {
            None => self.min.to_string(),
            Some(m) => format!("{} {}", self.min, m),
        }
    }
}

/// Tabellentyp: Reftyp + Limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tabellentyp {
    pub reftyp: Reftyp,
    pub grenzen: Limits,
}

/// Speicherdefinition (nur ein Speicher je Modul ohne Multi-Memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speichertyp {
    pub grenzen: Limits,
}

/// Eine Funktion im gemeinsamen Indexraum.
#[derive(Debug, Clone, PartialEq)]
pub enum Func {
    /// Import aus einem anderen Modul.
    Importiert {
        modul: String,
        feld: String,
        typ: u32,
    },
    /// Eigene Funktion: Typindex, Lokaltypen, Code als Befehlsfolge.
    Eigen {
        typ: u32,
        lokale: Vec<Valtyp>,
        koerper: Vec<Befehl>,
        befehl_offset: usize,
    },
}

/// `use` beim Debugger: Woerterbuchfunktion -> Typindex.
pub type Namen = std::collections::BTreeMap<String, u32>;

/// Globale Variable.
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub typ: Valtyp,
    pub veraenderbar: bool,
    /// Startwert (nur fuer eigene Globale als Konstante ausgewertet).
    pub start: Option<Vec<crate::op::Befehl>>,
    pub import: Option<(String, String)>,
}

/// Export.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub name: String,
    pub art: Art,
}

/// Was exportiert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Func(u32),
    Tabelle(u32),
    Speicher(u32),
    Global(u32),
    /// Tag (Exception Handling, Wasm 3.0). GEMESSEN 02.10. (`imports.wast`).
    Tag(u32),
}

impl Art {
    pub fn byte(&self) -> u8 {
        match self {
            Art::Func(_) => 0,
            Art::Tabelle(_) => 1,
            Art::Speicher(_) => 2,
            Art::Global(_) => 3,
            Art::Tag(_) => 4,
        }
    }
    pub fn text(&self) -> &'static str {
        match self {
            Art::Func(_) => "func",
            Art::Tabelle(_) => "table",
            Art::Speicher(_) => "memory",
            Art::Global(_) => "global",
            Art::Tag(_) => "tag",
        }
    }
    pub fn index(&self) -> u32 {
        match self {
            Art::Func(i) | Art::Tabelle(i) | Art::Speicher(i) | Art::Global(i) | Art::Tag(i) => *i,
        }
    }
}

/// Importart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importart {
    Func(u32),
    Tabelle(Tabellentyp),
    Speicher(Speichertyp),
    Global(Valtyp, bool),
}

impl Importart {
    pub fn byte(&self) -> u8 {
        match self {
            Importart::Func(_) => 0,
            Importart::Tabelle(_) => 1,
            Importart::Speicher(_) => 2,
            Importart::Global(_, _) => 3,
        }
    }
    pub fn text(&self) -> &'static str {
        match self {
            Importart::Func(_) => "func",
            Importart::Tabelle(_) => "table",
            Importart::Speicher(_) => "memory",
            Importart::Global(_, _) => "global",
        }
    }
}

/// Import (Modulname, Feldname, Art).
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub modul: String,
    pub feld: String,
    pub art: Importart,
}

/// Ein Elementabschnitt (Wasm 2.0: 8 Formen, Bit 0..2 = Form).
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// Aktiv: bei der Instanziierung in die Tabelle kopieren.
    Aktiv {
        tabelle: u32,
        offset: Vec<crate::op::Befehl>,
        reftyp: Reftyp,
        eintraege: Vec<Vec<crate::op::Befehl>>,
    },
    /// Passiv: nur mit `table.init` benutzt.
    Passiv {
        reftyp: Reftyp,
        eintraege: Vec<Vec<crate::op::Befehl>>,
    },
    /// Deklarativ: macht Funktionen fuer `ref.func` benutzbar.
    Deklarativ {
        reftyp: Reftyp,
        eintraege: Vec<Vec<crate::op::Befehl>>,
    },
}

impl Element {
    pub fn reftyp(&self) -> Reftyp {
        match self {
            Element::Aktiv { reftyp, .. }
            | Element::Passiv { reftyp, .. }
            | Element::Deklarativ { reftyp, .. } => *reftyp,
        }
    }
    /// Die Eintragsliste des Abschnitts (alle drei Formen).
    pub fn eintraege(&self) -> &[Vec<crate::op::Befehl>] {
        match self {
            Element::Aktiv { eintraege, .. }
            | Element::Passiv { eintraege, .. }
            | Element::Deklarativ { eintraege, .. } => eintraege,
        }
    }
    /// Nur ein Abschnitt mit konstantem Versatz darf aktiven Speicher/
    /// Tabellenspeicher in der Vorstufe anfassen (Spec: constant expressions).
    pub fn form(&self) -> &'static str {
        match self {
            Element::Aktiv { .. } => "aktiv",
            Element::Passiv { .. } => "passiv",
            Element::Deklarativ { .. } => "deklarativ",
        }
    }
}

/// Datenabschnitt (zwei Formen: aktiv mit Versatz, passiv).
#[derive(Debug, Clone, PartialEq)]
pub enum Daten {
    Aktiv {
        speicher: u32,
        offset: Vec<crate::op::Befehl>,
        inhalt: Vec<u8>,
    },
    Passiv {
        inhalt: Vec<u8>,
    },
}

/// Benutzerdefinierte Sektion. W5 liest daraus die DWARF-Zeilen
/// (Sektionen namens `<funktionsindex>` bzw. `.debug_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct Custom {
    pub name: String,
    pub inhalt: Vec<u8>,
    /// Position im Modul, fuer Fehlermeldungen.
    pub abschnitt: u8,
}

/// Ein vollstaendig gelesenes Modul.
#[derive(Debug, Clone, PartialEq)]
pub struct Modul {
    pub typen: Vec<FuncTyp>,
    pub funktionen: Vec<Func>,
    pub tabellen: Vec<ImportiertTab>,
    pub speicher: Vec<ImportiertSpeicher>,
    pub globale: Vec<Global>,
    /// Tags (Exception Handling). Ohne `try`/`catch` ist ein Tag nur eine
    /// Signatur im Indexraum — genug fuer Import, Export und
    /// Link-Pruefung. GEMESSEN 02.10. (`imports.wast` Z. 0/10).
    pub tags: Vec<Tag>,
    pub elemente: Vec<Element>,
    pub daten: Vec<Daten>,
    pub exporte: Vec<Export>,
    pub start: Option<u32>,
    pub custom: Vec<Custom>,
    /// `DataCount`-Sektion, falls vorhanden (Pflicht vor `memory.init`).
    pub daten_anzahl: Option<u32>,
    /// Funktionen, die `ref.func` benutzen (aus der Deklarationsliste).
    pub deklarierte_funktionen: Vec<u32>,
    /// `use`-Woerterbuch (Eigennamen-Feld `names`), falls vorhanden.
    pub namen: Option<Namen>,
    /// Array-Typen (Index wie `typen`; `None` = Funktionstyp).
    ///
    /// GEMESSEN 03.10. (`table_init.wast` Z. 790).
    pub arrays: Vec<Option<ArrayTyp>>,
    /// Struktur-Typen (Index wie `typen`; `None` = kein Strukturtyp).
    ///
    /// GEMESSEN 03.10. (`extern.wast` Z. 4: `(type $st (struct))`).
    pub strukt: Vec<Option<StructTyp>>,
    /// Je Typindex: der ausdruecklich deklarierte **Obertyp** aus
    /// `(type $a (sub $b …))` — `None`, wenn keiner dasteht (dann ist der
    /// Rumpf selbst final).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` 33/74 mit 249
    /// `(sub …)`-Vorkommen). Die Liste ist so lang wie `typen`.
    pub sub_ober: Vec<Option<u32>>,
    /// Je Typindex: war der Typ als `(sub **final** $s (…))` deklariert?
    /// Ein finaler Typ darf **nie** Obertyp eines anderen sein — der
    /// Validator lehnt eine solche Deklaration ab.
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 780ff, 19 Faelle
    /// `assert_invalid`): `(type $t (func))` / `(type $s (sub $t (func)))`
    /// und `(type $t (sub final (func)))` sind ungueltig („sub type").
    /// Die Liste ist so lang wie `sub_ober`.
    pub sub_final: Vec<bool>,
    /// Je Typindex: wurde der Typ ueberhaupt als `(sub …)` deklariert?
    /// **Finalitaet haengt an dieser Klammer, nicht am Obertyp**:
    /// `(type $st (sub (struct)))` ist ein *nicht-finaler* Typ ganz ohne
    /// Obertyp, `(type $t (struct))` ist final.
    ///
    /// GEMESSEN 04.10. (`ref_eq.wast` Z. 1ff: `(type $st-sub1 (sub $st
    /// (struct)))` mit `$st (sub (struct))` — die erste Fassung der
    /// Finalitaetsregel las „kein Obertyp heisst final" und liess die
    /// ganze Datei fallen: 0/87, Gesamt **34928 (-73)**).
    pub sub_ge_deklariert: Vec<bool>,
    /// Je Typindex: die `(rec …)`-Gruppe, in der er liegt
    /// `(Gruppenanfang, Gruppenende)`; `None` ausserhalb einer Gruppe.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast`: 2x „unknown type"). Ein
    /// `(ref N)` darf ausserhalb einer Gruppe nur **vorher**
    /// deklarierte Typen nennen, innerhalb nur Mitglieder derselben
    /// Gruppe.
    pub gruppe_von: Vec<Option<(u32, u32)>>,
    /// Je Typindex: die `(rec …)`-Gruppe, in der er liegt
    /// `(Gruppenanfang, Gruppenende)`; `None` ausserhalb einer Gruppe.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast`: 2x „unknown type"). Ein
    /// `(ref N)` darf ausserhalb einer Gruppe nur **vorher**
    /// deklarierte Typen nennen, innerhalb nur Mitglieder derselben
    /// Gruppe.    /// Je Typindex: die `(rec …)`-Gruppe, in der er liegt
    /// `(Gruppenanfang, Gruppenende)`; `None` ausserhalb einer Gruppe.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast`: 2x „unknown type"). Ein
    /// `(ref N)` darf ausserhalb einer Gruppe nur **vorher**
    /// deklarierte Typen nennen, innerhalb nur Mitglieder derselben
    /// Gruppe.    /// Je Typindex: der ausdruecklich deklarierte **Obertyp** aus
    /// `(type $a (sub $b …))` — `None`, wenn keiner dasteht (dann ist der
    /// Rumpf selbst final).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` 33/74 mit 249
    /// `(sub …)`-Vorkommen). Die Liste ist so lang wie `typen`.    /// Groesse der Eingabe in Byte (fuer Berichte/Pruefungen).
    pub byte_groesse: usize,
    /// The imports in the order of the import section, each as its kind and
    /// its index in that kind's index space (`Art::Func(3)` = function 3).
    /// The per-kind lists above lose this order; the JS API
    /// (`WebAssembly.Module.imports`, reading the import object) needs it.
    /// Empty for modules that did not come from the binary reader.
    pub import_reihe: Vec<Art>,
}

/// Ein Tag (Exception Handling): ein Typindex, aus dem ein Import entstehen
/// kann. Ohne `try`/`catch` gibt es keinen Code dahinter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Typindex der Tag-Signatur (nur Parameter, keine Ergebnisse).
    pub typ: u32,
    /// Import aus einem anderen Modul (`(tag (import "m" "n") …)`).
    pub import: Option<(String, String)>,
}

/// Tabelle im gemeinsamen Indexraum.
///
/// Kein `Eq`: der Startwert enthaelt `Befehl`, das nur `PartialEq` hat.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportiertTab {
    Importiert {
        modul: String,
        feld: String,
        typ: Tabellentyp,
    },
    Eigen {
        typ: Tabellentyp,
        /// Startwert der Tabelle (Wasm 3.0, „table initializer“): er fuellt
        /// den GANZEN Bereich. `None` heisst `ref.null ht`.
        ///
        /// GEMESSEN 02.10. (`table.wast` Z. 34-39: 4 Faelle).
        startwert: Option<Vec<crate::op::Befehl>>,
    },
}

/// Speicher im gemeinsamen Indexraum.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportiertSpeicher {
    Importiert {
        modul: String,
        feld: String,
        typ: Speichertyp,
    },
    Eigen {
        typ: Speichertyp,
    },
}

impl Modul {
    pub fn leer() -> Modul {
        Modul {
            typen: Vec::new(),
            funktionen: Vec::new(),
            tabellen: Vec::new(),
            speicher: Vec::new(),
            globale: Vec::new(),
            tags: Vec::new(),
            elemente: Vec::new(),
            daten: Vec::new(),
            exporte: Vec::new(),
            start: None,
            custom: Vec::new(),
            daten_anzahl: None,
            deklarierte_funktionen: Vec::new(),
            namen: None,
            arrays: Vec::new(),
            strukt: Vec::new(),
            sub_ober: Vec::new(),
            sub_final: Vec::new(),
            sub_ge_deklariert: Vec::new(),
            gruppe_von: Vec::new(),
            byte_groesse: 0,
            import_reihe: Vec::new(),
        }
    }

    /// Signatur einer Funktion im Indexraum (Import = Typ des Imports).
    pub fn signatur(&self, idx: u32) -> Option<&FuncTyp> {
        let f = self.funktionen.get(idx as usize)?;
        let t = match f {
            Func::Importiert { typ, .. } => *typ,
            Func::Eigen { typ, .. } => *typ,
        };
        self.typen.get(t as usize)
    }

    /// Alle eigenen Funktionen (fuer WAT-Ausgabe/Disassembly).
    pub fn eigene_funktionen(&self) -> Vec<u32> {
        self.funktionen
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f, Func::Eigen { .. }))
            .map(|(i, _)| i as u32)
            .collect()
    }

    pub fn koerper(&self, idx: u32) -> Option<&[crate::op::Befehl]> {
        match self.funktionen.get(idx as usize)? {
            Func::Eigen { koerper, .. } => Some(koerper),
            Func::Importiert { .. } => None,
        }
    }

    /// Anzahl importierter Funktionen (Startindex der eigenen Funktionen).
    pub fn importierte_funktionen(&self) -> usize {
        self.funktionen
            .iter()
            .filter(|f| matches!(f, Func::Importiert { .. }))
            .count()
    }
    /// Anzahl importierter Tabellen.
    pub fn importierte_tabellen(&self) -> usize {
        self.tabellen
            .iter()
            .filter(|t| matches!(t, ImportiertTab::Importiert { .. }))
            .count()
    }
    /// Anzahl importierter Speicher.
    pub fn importierte_speicher(&self) -> usize {
        self.speicher
            .iter()
            .filter(|s| matches!(s, ImportiertSpeicher::Importiert { .. }))
            .count()
    }
    /// Anzahl importierter globaler Variablen.
    pub fn importierte_globale(&self) -> usize {
        self.globale.iter().filter(|g| g.import.is_some()).count()
    }

    /// Alle Exporte unter einem Namen (Doppelexport ist ein Linkfehler).
    pub fn export(&self, name: &str) -> Option<&Export> {
        self.exporte.iter().find(|e| e.name == name)
    }

    /// Der ANZEIGEBARNAME einer Funktion: Exportname, sonst die
    /// `name`-Sektion (im Binaerformat) bzw. der Bezeichner aus dem
    /// Textformat, sonst `func#<index>`.
    ///
    /// EINE Regel fuer alle Anzeigen des Reiters „WebAssembly": Aufrufstapel,
    /// Haltepunktliste und Disassembly-Kopf muessen dieselbe Schreibweise
    /// zeigen, sonst sieht ein Haltepunkt in der Liste anders aus als im
    /// Stapel. `modell::Namen` fuehrt den Namen OHNE `$` und im selben
    /// Indexraum wie `Modul::funktionen` (Importe mitgezaehlt).
    ///
    /// GEMESSEN 03.10. (team/rollen/WASM.txt: „Aufrufstapel mit
    /// Funktionsnamen (name-Section)").
    pub fn funktions_name(&self, funktion: u32) -> String {
        for e in &self.exporte {
            if matches!(e.art, Art::Func(_)) && e.art.index() == funktion {
                return e.name.clone();
            }
        }
        if let Some(namen) = &self.namen {
            if let Some((name, _)) = namen.iter().find(|(_, v)| **v == funktion) {
                return name.clone();
            }
        }
        format!("func#{funktion}")
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Bytes + Namen sichern.
    #[test]
    fn werttypen_hin_und_zurueck() {
        for t in [Valtyp::I32, Valtyp::I64, Valtyp::F32, Valtyp::F64] {
            assert_eq!(Valtyp::von_byte(t.byte()), Some(t));
            assert_eq!(Valtyp::aus_text(t.text()), Some(t));
        }
        for r in [Reftyp::Func, Reftyp::Extern] {
            let v = Valtyp::Ref(r);
            assert_eq!(Valtyp::von_byte(v.byte()), Some(v));
            assert!(v.ist_referenz());
        }
        assert_eq!(Valtyp::von_byte(0x00), None);
        assert_eq!(Valtyp::aus_text("v128"), None);
        assert_eq!(reftyp_ungueltig("anyref").art(), ART_REFTYP);
        assert_eq!(typ_ungueltig(7).art(), crate::fehler::V_TYP);
    }

    /// M7: die Kettenpruefung, die jetzt auch **zur Laufzeit** laeuft
    /// (`call_indirect`, `ref.cast`), sichert Kette, Reflexivitaet und die
    /// harte Zyklusgrenze (M3/M5).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime types").
    #[test]
    fn kettenpruefung_laeuft_und_haust_nicht() {
        // `sub_ober[a] = b` heisst: a ist ein Untertyp von b. Also
        // `2 -> 1 -> 0`: Typ 2 erfuellt Typ 0.
        let kette = vec![None, Some(0u32), Some(1u32), None];
        assert!(subtyp_ueber_kette_in(&kette, 0, 0));
        assert!(subtyp_ueber_kette_in(&kette, 1, 0));
        assert!(subtyp_ueber_kette_in(&kette, 2, 0));
        assert!(!subtyp_ueber_kette_in(&kette, 0, 1));
        assert!(!subtyp_ueber_kette_in(&kette, 0, 2));
        // Index ausserhalb der Tabelle: kein Treffer, kein Absturz.
        assert!(!subtyp_ueber_kette_in(&kette, 99, 0));
        // Zyklus: 1 -> 2 -> 1 muss enden (MAX_SUBKETTE), nicht haengen.
        let zyklus = vec![None, Some(2u32), Some(1u32)];
        // Ziel 0 wird nie erreicht: die Suche endet nach MAX_SUBKETTE mit
        // `false` (M3/M5) — sie darf nicht haengen.
        assert!(!subtyp_ueber_kette_in(&zyklus, 1, 0));
        // Im Zyklus sind 1 und 2 durchaus verwandt (2 -> 1 trifft zu).
        assert!(subtyp_ueber_kette_in(&zyklus, 2, 1));
    }

    /// M7: der Signatur-Vergleich sichert beide Seiten der Regel —
    /// abstrakte Heap-Typen sind untereinander **flach**, und der
    /// Normalfall gilt fuer konkrete Typen unveraendert.
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` Z. 227ff, 12 Faelle;
    /// `ref_null.wast` und `unreached-valid.wast` je 100 %).
    #[test]
    fn signatur_vergleich_abstrakt_flach() {
        let m = Modul::leer();
        let f = |t: u32| {
            Valtyp::Ref(Reftyp::Konkret {
                typ: t,
                nullable: true,
            })
        };
        // Zwei verschiedene abstrakte Heap-Typen: kein Untertyp.
        assert!(!valtyp_passt_als_signatur(
            &m,
            &f(KEIN_FUNKTIONSTYP),
            &f(JEDER_TYP)
        ));
        assert!(!valtyp_passt_als_signatur(
            &m,
            &f(KEIN_TYP),
            &f(JEDER_FUNKTIONSTYP)
        ));
        // Derselbe abstrakte Typ: passt (die `null`-Frage entscheidet der
        // normale Vergleich).
        assert!(valtyp_passt_als_signatur(&m, &f(JEDER_TYP), &f(JEDER_TYP)));
        // `(ref func)` und `funcref` sind derselbe abstrakte Typ.
        assert!(valtyp_passt_als_signatur(
            &m,
            &Valtyp::Ref(Reftyp::Func),
            &f(JEDER_FUNKTIONSTYP)
        ));
        // Ohne den Signatur-Zweig ist dieselbe Kombination erlaubt — das
        // ist der Grund, warum es zwei Funktionen gibt.
        assert!(valtyp_passt_als(&m, &f(KEIN_FUNKTIONSTYP), &f(JEDER_TYP)));
    }

    /// M7: die Sichtbarkeit fuer **Feldtypen** wirkt auch fuer Mitglieder
    /// einer `rec`-Gruppe: ein Feld, das in eine *spaetere* Gruppe zeigt,
    /// macht das Modul ungueltig.
    ///
    /// GEMESSEN 03.10. (`/tmp/opencode/rec/f11b.wast`; die Suite selbst
    /// fuehrt diese Regel in `type-rec.wast` Z. 5ff als „unknown type").
    #[test]
    fn feldtyp_zeigt_nicht_in_spaetere_gruppe() {
        let g = crate::Grenzen::standard();
        let gut = crate::wat::lies_text(
            "(module (rec (type $a (struct (field (ref $a))))) (rec (type $b (struct))))",
            &g,
        );
        assert!(gut.is_ok(), "Rueckverweis in die eigene Gruppe ist gueltig");
        let schlecht = crate::wat::lies_text(
            "(module (rec (type $a (struct (field (ref $x))))) (rec (type $x (struct))))",
            &g,
        );
        let m = schlecht.expect("Modul lesbar");
        assert!(
            crate::pruefe::modul(&m, &g).is_err(),
            "Feldtyp auf einen Typ der naechsten Gruppe muss ungueltig sein"
        );
    }

    #[test]
    fn limits_und_leer_modul() {
        let l = Limits {
            min: 2,
            max: Some(1),
        };
        assert!(!l.passt("mem"));
        assert_eq!(l.text(), "2 1");
        let m = Modul::leer();
        assert_eq!(m.typen.len(), 0);
        assert_eq!(m.signatur(0), None);
        assert!(m.koerper(0).is_none());
        assert!(m.eigene_funktionen().is_empty());
    }
}
