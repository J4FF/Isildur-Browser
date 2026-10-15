//! Laufzeitwerte (Wasm 2.0 Core): i32/i64/f32/f64 und die beiden
//! Referenztypen. f32/f64 werden als BITMUSTER gespeichert, damit
//! NaN-Bitfaelle, -0.0 und Reinterpret bitgenau erhalten bleiben —
//! ohne das scheitern `float_misc.wast`/`float_exprs.wast`.

use crate::fehler::{Fehler, T_UNGUELTIGE_KONVERSION};
use crate::modell::{Reftyp, Valtyp};

/// Referenz auf eine Funktion (Index im gemeinsamen Indexraum) oder auf ein
/// Host-Objekt (`externref` = Index in der Hosttabelle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ref {
    /// `funcref`: Index einer WASM-Funktion im Store.
    Func(u32),
    /// `externref`: Index in der Tabelle der Hostobjekte (W4).
    Extern(u32),
    /// GC-Referenz (`arrayref`): Index im GC-Haufen des Stores.
    ///
    /// GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
    Gcref(u32),
}

/// Ein Wert auf dem Operandstapel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wert {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    /// `funcref`.
    Func(Option<u32>),
    /// `externref`.
    Extern(Option<u32>),
    /// GC-Referenz (Array, Wasm 3.0 GC). `None` = `null`.
    ///
    /// GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
    Gcref(Option<u32>),
}

impl Wert {
    /// Standardwert eines Typs (0 bzw. `ref.null`).
    pub fn standard(t: Valtyp) -> Wert {
        match t {
            Valtyp::I32 => Wert::I32(0),
            Valtyp::I64 => Wert::I64(0),
            Valtyp::F32 => Wert::F32(0),
            Valtyp::F64 => Wert::F64(0),
            // Ein konkreter Typ ist eine Funktionsreferenz; `null` ist nur
            // erlaubt, wenn der Typ nullable ist.
            Valtyp::Ref(Reftyp::Func) | Valtyp::Ref(Reftyp::Konkret { nullable: true, .. }) => {
                Wert::Func(None)
            }
            Valtyp::Ref(Reftyp::Extern) => Wert::Extern(None),
            // `(ref $arr)`/`(ref null $arr)`: ein Array-Verweis. Der
            // Typindex wird erst in `typ()` wieder gebraucht; als
            // Standardwert ist es `null`.
            Valtyp::Ref(Reftyp::Konkret {
                typ,
                nullable: false,
            }) if typ == crate::modell::JEDER_ARRAYTYP => Wert::Gcref(None),
            Valtyp::Ref(Reftyp::Konkret {
                nullable: false, ..
            }) => Wert::Func(None),
        }
    }

    /// Der Typ, den der Wert hat.
    pub fn typ(&self) -> Valtyp {
        match self {
            Wert::I32(_) => Valtyp::I32,
            Wert::I64(_) => Valtyp::I64,
            Wert::F32(_) => Valtyp::F32,
            Wert::F64(_) => Valtyp::F64,
            Wert::Func(_) => Valtyp::Ref(Reftyp::Func),
            Wert::Extern(_) => Valtyp::Ref(Reftyp::Extern),
            // Der genaue Array-Typ steht im GC-Objekt; fuer die
            // Typmaschine reicht der Array-Wildcard (der Validator
            // vergleicht die Typen ueber die Befehlstypen).
            Wert::Gcref(_) => Valtyp::Ref(Reftyp::Konkret {
                typ: crate::modell::JEDER_ARRAYTYP,
                nullable: true,
            }),
        }
    }

    pub fn ist_referenz(&self) -> bool {
        matches!(self, Wert::Func(_) | Wert::Extern(_) | Wert::Gcref(_))
    }

    pub fn als_i32(&self) -> Option<i32> {
        match self {
            Wert::I32(v) => Some(*v),
            _ => None,
        }
    }
    pub fn als_i64(&self) -> Option<i64> {
        match self {
            Wert::I64(v) => Some(*v),
            _ => None,
        }
    }
    pub fn als_f32(&self) -> Option<f32> {
        match self {
            Wert::F32(b) => Some(f32::from_bits(*b)),
            _ => None,
        }
    }
    pub fn als_f64(&self) -> Option<f64> {
        match self {
            Wert::F64(b) => Some(f64::from_bits(*b)),
            _ => None,
        }
    }
    /// Bitmuster als `u32` (i32 oder f32) — fuer Speicherzugriffe.
    ///
    /// GEMESSEN 01.10. (`float_memory.wast`, `float_exprs.wast`): der
    /// Speicherzugriff holte den Wert mit `als_i32()`, das fuer `F32` `None`
    /// liefert — jeder `f32.store` schrieb also Nullen.
    pub fn als_bits32(&self) -> u32 {
        match self {
            Wert::I32(v) => *v as u32,
            Wert::F32(b) => *b,
            _ => 0,
        }
    }
    /// Bitmuster als `u64` (i64 oder f64) — siehe `als_bits32`.
    pub fn als_bits64(&self) -> u64 {
        match self {
            Wert::I64(v) => *v as u64,
            Wert::F64(b) => *b,
            _ => 0,
        }
    }
    pub fn als_ref(&self) -> Option<Ref> {
        match self {
            Wert::Func(Some(i)) => Some(Ref::Func(*i)),
            Wert::Extern(Some(i)) => Some(Ref::Extern(*i)),
            Wert::Gcref(Some(i)) => Some(Ref::Gcref(*i)),
            _ => None,
        }
    }
}

/// i32-Wert als u32 (fuer Adressen, Indizes, `br_table`).
pub fn als_u32(v: Wert) -> u32 {
    match v {
        Wert::I32(x) => x as u32,
        // Der Validator hat den Typ schon geprueft; fuer die vollstaendige
        // Abdeckung (Hostfunktionen) wird der Fehlerwert geliefert.
        _ => 0,
    }
}

/// Trunkiert `f32`/`f64` nach `i32`/`i64` mit Trap bei NaN/Overflow
/// (Wasm 2.0: `trunc`, nicht `trunc_sat`).
pub fn trunk(f: f64, nach_bits: u32, vorzeichen: bool) -> Result<i64, Fehler> {
    if f.is_nan() {
        return Err(Fehler::trap(
            T_UNGUELTIGE_KONVERSION,
            "trunc einer NaN in eine Ganzzahl",
        ));
    }
    let t = f.trunc();
    if vorzeichen {
        if t < i32::MIN as f64 || t > i32::MAX as f64 {
            // Fuer i64 ist der Vergleich in f64 ungenau -> ueber i128 pruefen.
            if nach_bits == 64 {
                // f64 kann i64::MIN nicht genau darstellen (-2^63 ist
                // exakt, aber der Vergleich `t < -2^63` ist bei f64
                // gerundet). Deshalb der Vergleich ueber `f64` >= 2^63.
                if !(I64_MIN_F64..I64_OBERGRENZE).contains(&t) {
                    return Err(Fehler::trap(
                        T_UNGUELTIGE_KONVERSION,
                        "trunc: Wert ausserhalb des i64-Bereichs",
                    ));
                }
                return Ok(t as i64);
            }
            return Err(Fehler::trap(
                T_UNGUELTIGE_KONVERSION,
                "trunc: Wert ausserhalb des i32-Bereichs",
            ));
        }
        Ok(t as i64)
    } else {
        // Ohne Vorzeichen: nur Werte in (-1, 0) sind erlaubt (sie werden zu 0
        // abgeschnitten). `-1.0` selbst ist AUSSERHALB und muss trapen
        // (GEMESSEN 02.10., `conversions.wast` Z. 61/109/155/202: die
        // Bedingung war `< -1.0` statt `<= -1.0`).
        if t < 0.0 {
            if t <= -1.0 {
                return Err(Fehler::trap(
                    T_UNGUELTIGE_KONVERSION,
                    "trunc_u: negativer Wert",
                ));
            }
            return Ok(0);
        }
        if nach_bits == 64 {
            if t >= 18_446_744_073_709_551_616.0 {
                return Err(Fehler::trap(
                    T_UNGUELTIGE_KONVERSION,
                    "trunc_u: Wert ausserhalb des u64-Bereichs",
                ));
            }
            return Ok(t as u64 as i64);
        }
        if t >= 4_294_967_296.0 {
            return Err(Fehler::trap(
                T_UNGUELTIGE_KONVERSION,
                "trunc_u: Wert ausserhalb des u32-Bereichs",
            ));
        }
        Ok(t as u64 as i64)
    }
}

/// `trunc_sat`: wie `trunk`, aber statt Trap der naechste darstellbare Wert.
pub fn trunk_sat(f: f64, nach_bits: u32, vorzeichen: bool) -> i64 {
    if f.is_nan() {
        return 0;
    }
    let t = f.trunc();
    if vorzeichen {
        let (min, max) = if nach_bits == 64 {
            (i64::MIN as f64, 9_223_372_036_854_775_807.0)
        } else {
            (i32::MIN as f64, i32::MAX as f64)
        };
        // `max` wird auf die naechste darstellbare Ganzzahl gerundet.
        if t <= min {
            return if nach_bits == 64 {
                i64::MIN
            } else {
                i32::MIN as i64
            };
        }
        if t >= max {
            return if nach_bits == 64 {
                i64::MAX
            } else {
                i32::MAX as i64
            };
        }
        t as i64
    } else {
        let max = if nach_bits == 64 {
            u64::MAX as f64
        } else {
            u32::MAX as f64
        };
        if t <= 0.0 {
            return 0;
        }
        if t >= max {
            return if nach_bits == 64 {
                u64::MAX as i64
            } else {
                u32::MAX as i64
            };
        }
        t as u64 as i64
    }
}

/// -2^63 als f64 (exakt darstellbar).
const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
/// 2^63 als f64: die erste Zahl, die ausserhalb des i64-Bereichs liegt.
const I64_OBERGRENZE: f64 = 9_223_372_036_854_775_808.0;

/// kuerzestes `round-half-to-even` (Wasm `nearest`).
pub fn nearest(x: f64) -> f64 {
    // `round_ties_even` ist die Regel der Spezifikation („round to
    // nearest, ties to even"). Die eigene Nachrechnung (`r - signum`)
    // verlor das Vorzeichen der Null: GEMESSEN 02.10. (`f64.wast`:
    // `nearest(-0.5)` muss `-0.0` liefern, kam `+0.0`).
    x.round_ties_even()
}

/// kuerzestes `round-half-to-even` fuer f32.
pub fn nearest32(x: f32) -> f32 {
    // siehe `nearest`: `round_ties_even` statt eigener Nachrechnung.
    x.round_ties_even()
}

/// Kanonische NaN: Bitmuster mit ruhendem Bit. Wasm erlaubt alle
/// „arithmetischen NaN" (Bit 22 gesetzt, Nutzlast beliebig); die
/// Testsuite prueft meist auf „canonical" oder „arithmetic". Wir erzeugen
/// das kanonische Bitmuster — das erfuellt beide.
pub fn nan32() -> u32 {
    0x7fc0_0000
}
pub fn nan64() -> u64 {
    0x7ff8_0000_0000_0000
}

/// Ist ein f32-Bitmuster eine NaN?
pub fn ist_nan32(b: u32) -> bool {
    (b & 0x7f80_0000) == 0x7f80_0000 && (b & 0x007f_ffff) != 0
}
pub fn ist_nan64(b: u64) -> bool {
    (b & 0x7ff0_0000_0000_0000) == 0x7ff0_0000_0000_0000 && (b & 0x000f_ffff_ffff_ffff) != 0
}

/// Wert als kurzer Text (Debugger-Anzeige, Disassembly, Fehlermeldungen).
pub fn als_text(w: &Wert) -> String {
    match w {
        Wert::I32(x) => format!("i32:{x}"),
        Wert::I64(x) => format!("i64:{x}"),
        Wert::F32(b) => format!("f32:{}", als_f32_text(*b)),
        Wert::F64(b) => format!("f64:{}", als_f64_text(*b)),
        Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None) => "null".to_string(),
        Wert::Func(Some(f)) => format!("funcref:{f}"),
        Wert::Extern(Some(e)) => format!("externref:{e}"),
        Wert::Gcref(Some(g)) => format!("arrayref:{g}"),
    }
}

/// f32-Bitmuster als kurzer Text (fuer Disassembly und Fehlermeldungen).
pub fn als_f32_text(bits: u32) -> String {
    let f = f32::from_bits(bits);
    if ist_nan32(bits) {
        return if bits & 0x8000_0000 != 0 {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }
    if f.is_infinite() {
        return if bits & 0x8000_0000 != 0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    format!("{f:?}")
}

/// f64-Bitmuster als kurzer Text.
pub fn als_f64_text(bits: u64) -> String {
    let f = f64::from_bits(bits);
    if ist_nan64(bits) {
        return if bits & 0x8000_0000_0000_0000 != 0 {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }
    if f.is_infinite() {
        return if bits & 0x8000_0000_0000_0000 != 0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    format!("{f:?}")
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Ergebnisse und Fehlerarten sichern.
    #[test]
    fn standard_und_typ() {
        for t in [Valtyp::I32, Valtyp::I64, Valtyp::F32, Valtyp::F64] {
            assert_eq!(Wert::standard(t).typ(), t);
        }
        assert_eq!(Wert::Func(None).typ(), Valtyp::Ref(Reftyp::Func));
        assert_eq!(Wert::Extern(Some(3)).als_ref(), Some(Ref::Extern(3)));
        assert!(Wert::Func(None).ist_referenz());
    }

    #[test]
    fn trunk_traps() {
        assert_eq!(trunk(3.9, 32, true).unwrap(), 3);
        assert_eq!(trunk(-3.9, 32, true).unwrap(), -3);
        assert_eq!(trunk(3.9, 32, false).unwrap(), 3);
        assert_eq!(trunk(-0.5, 32, false).unwrap(), 0);
        assert_eq!(trunk(3.9, 64, true).unwrap(), 3);
        assert_eq!(
            trunk(1e30, 64, false).unwrap_err().art(),
            T_UNGUELTIGE_KONVERSION
        );
        assert_eq!(
            trunk(1e30, 32, true).unwrap_err().art(),
            T_UNGUELTIGE_KONVERSION
        );
        assert_eq!(
            trunk(f64::NAN, 32, true).unwrap_err().art(),
            T_UNGUELTIGE_KONVERSION
        );
    }

    #[test]
    fn trunk_sat_klemmt() {
        assert_eq!(trunk_sat(f64::NAN, 32, true), 0);
        assert_eq!(trunk_sat(1e30, 32, true), i32::MAX as i64);
        assert_eq!(trunk_sat(-1e30, 32, true), i32::MIN as i64);
        assert_eq!(trunk_sat(1e30, 32, false), u32::MAX as i64);
        assert_eq!(trunk_sat(1e30, 64, false), u64::MAX as i64);
    }

    #[test]
    fn nearest_haelt_gerade() {
        assert_eq!(nearest(0.5), 0.0);
        assert_eq!(nearest(1.5), 2.0);
        assert_eq!(nearest(2.5), 2.0);
        assert_eq!(nearest(-0.5), -0.0);
        assert_eq!(nearest32(3.5), 4.0);
        assert_eq!(nearest32(0.5), 0.0);
    }

    /// M7: Bitmuster -> Text.
    #[test]
    fn text_aus_bitmuster() {
        assert_eq!(als_f32_text(0x7f80_0000), "inf");
        assert_eq!(als_f32_text(0xff80_0000), "-inf");
        assert_eq!(als_f32_text(nan32()), "nan");
        assert_eq!(als_f64_text(0x7ff0_0000_0000_0000), "inf");
        assert_eq!(als_f64_text(1.0f64.to_bits()), "1.0");
        assert!(als_f32_text((-0.0f32).to_bits()).starts_with("-0"));
    }

    #[test]
    fn nan_bitmuster() {
        assert!(ist_nan32(nan32()));
        assert!(ist_nan64(nan64()));
        assert!(!ist_nan32(1.0f32.to_bits()));
        assert!(!ist_nan64(1.0f64.to_bits()));
    }
}
