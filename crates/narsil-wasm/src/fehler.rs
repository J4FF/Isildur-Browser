//! Fehler der Engine. `team/REGELN.txt` M8 verlangt fuer boesartige Eingaben
//! einen *BENANNTEN* Fehler, keinen Panic. Deshalb ist jeder Fehler eine
//! Variante mit einem statischen Namen (`art()`), auf den Tests pruefen.
//!
//! Die Namen sind stabil: `tests/bomben.rs` und der Testsuite-Lauf in
//! `tests/w3_testsuite.rs` vergleichen sie mit `==`, nicht mit `contains`.

use std::fmt;

/// Binarform nicht lesbar. `Fehler::Form(ART_.., ..)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Decoder: unbekannte/magische Bytes, LEB128-Ueberlaenge, Grenze.
    Form(&'static str, String),
    /// Validator nach WebAssembly 2.0 Core.
    Ungueltig(&'static str, String),
    /// Linken/Instantiieren: fehlender Import, doppelter Export, Start-Fehler.
    Link(&'static str, String),
    /// Laufzeit (Trap) nach Ausfuehrung eines Befehls.
    Trap(&'static str, String),
    /// Brennstoff aufgebraucht (Endlosschleife im Modul, M3).
    Brennstoff { verbraucht: u64, grenze: u64 },
}

// ---- Namen der Formfehler (Modul-Binaerformat) ------------------------------
/// `magic` stimmt nicht (`\0asm`).
pub const ART_MAGIE: &str = "form:magie";
/// Version ist nicht 1.
pub const ART_VERSION: &str = "form:version";
/// Unbekannte Sektionsart.
pub const ART_SEKTION: &str = "form:sektion";
/// LEB128 laeuft ueber die Bytegrenze hinaus (5 Byte fuer u32, 10 fuer u64).
pub const ART_LEB: &str = "form:leb128";
/// Unbekannter Opcode.
pub const ART_OPCODE: &str = "form:opcode";
/// Unbekannter Opcode mit Praefix (0xFC/0xFD/0xFE).
pub const ART_OPCODE_PRAEFIX: &str = "form:opcode-praefix";
/// Ende der Eingabe mitten im Feld.
pub const ART_UNVOLLSTAENDIG: &str = "form:unvollstaendig";
/// Hartgrenze ueberschritten (M3/M4).
pub const ART_GRENZE: &str = "form:grenze";
/// Laengenfeld kleiner als Mindestgroesse (`size`).
pub const ART_LAENGE: &str = "form:laenge";
/// Sektionen kommen in verlangter Reihenfolge nicht.
pub const ART_REIHENFOLGE: &str = "form:reihenfolge";
/// Sektion ohne Abhaengigkeit (Code vor Typen, Daten vor Speicher).
pub const ART_ABHAENGIG: &str = "form:abhaengig";
/// Zaehler passt nicht zur Zahl der Eintraege (DataCount, Element-Offsets).
pub const ART_ZAEHLER: &str = "form:zaehler";
/// Index (Funktion, Typ, Global, ...) liegt ausserhalb des Indexraums.
pub const ART_INDEX: &str = "form:index";
/// Name ist kein gueltiges UTF-8.
pub const ART_UTF8: &str = "form:utf8";
/// Datensektion zu gross fuer den Speicher / passt nicht hinein.
pub const ART_DATEN: &str = "form:daten";
/// Speichermindestgroesse (Seiten) verletzt oder > eine Seite.
pub const ART_SPEICHER: &str = "form:speicher";
/// Referenztyp-Byte unbekannt (0x70/0x6f erlaubt).
pub const ART_REFTYP: &str = "form:reftyp";
/// Typform oder Typindex unzulaessig.
pub const ART_TYP: &str = "form:typ";
/// Tabellentyp/-groesse unzulaessig.
pub const ART_TABELLE: &str = "form:tabelle";
/// Nummernform eines LEB128 zu lang/kurz kodiert (Spec verbietet
/// ueberlaengliche Kodierung, `binary-leb128.wast`).
pub const ART_LEB_KANONISCH: &str = "form:leb-kanonisch";
/// `block`-Typ ist unzulaessig.
pub const ART_BLOCKTYP: &str = "form:blocktyp";
/// Funktionsindex doppelt belegt (`ref.func` braucht Deklaration).
pub const ART_DOPPELT: &str = "form:doppelt";
/// Nach dem Ende der Sektion stehen Bytes.
pub const ART_REST: &str = "form:rest";

// ---- Namen der Validatorfehler ---------------------------------------------
/// Unbekannter Typindex in einem Signatur/Sprung.
pub const V_TYP: &str = "ungueltig:typ";
/// Unbekannter Funktionsindex (`call`).
pub const V_FUNKTION: &str = "ungueltig:funktion";
/// Unbekannter Tabellenindex.
pub const V_TABELLE: &str = "ungueltig:tabelle";
/// Unbekannter Speicherindex (nur Index 0 ohne Multi-Memory).
pub const V_SPEICHER: &str = "ungueltig:speicher";
/// Unbekannter Globalindex.
pub const V_GLOBAL: &str = "ungueltig:global";
/// Unbekannter Elementindex.
pub const V_ELEMENT: &str = "ungueltig:element";
/// Unbekannter Datenindex.
pub const V_DATEN: &str = "ungueltig:daten";
/// Unbekannter lokaler Index.
pub const V_LOKAL: &str = "ungueltig:lokal";
/// Unbekanntes Label (`br`/`br_if`/`br_table`).
pub const V_LABEL: &str = "ungueltig:label";
/// Falscher Operandtyp auf dem Stapel.
pub const V_TYP_OPERAND: &str = "ungueltig:operand";
/// Falscher Operandtyp beim Ergebnis einer Instruktion.
pub const V_TYP_ERGEBNIS: &str = "ungueltig:ergebnis";
/// Falscher Ergebnistyp eines Blocks.
pub const V_TYP_BLOCK: &str = "ungueltig:block";
/// Falscher Sprungtyp (`br_if`-Bedingung, `call_indirect`).
pub const V_TYP_SPRUNG: &str = "ungueltig:sprung";
/// Falscher Ergebnistyp einer globalen Variablen (Konstante).
pub const V_TYP_GLOBAL: &str = "ungueltig:global-typ";
/// Speicher zu klein fuer die Mindestgroesse.
pub const V_SPEICHER_MIN: &str = "ungueltig:speicher-min";
/// `if` ohne `else`, aber mit Ergebnissen.
pub const V_IF: &str = "ungueltig:if";
/// `else` ohne `if`.
pub const V_ELSE: &str = "ungueltig:else";
/// `end` ohne offene Struktur.
pub const V_END: &str = "ungueltig:end";
/// Unerreichbarer Code nach unbedingtem Sprung (im Wasm 2.0 erlaubt, aber
/// der Stapeltyp-Zustand wird als `unreachable` markiert - hier nur, wenn
/// die Struktur nicht stimmt).
pub const V_UNERREICHBAR: &str = "ungueltig:unerreichbar";
/// Startfunktion hat falsche Signatur.
pub const V_START: &str = "ungueltig:start";
/// Exportname doppelt belegt (Spec: „duplicate export name").
pub const V_EXPORT: &str = "ungueltig:export";
/// Ref-Typ passt nicht (Referenztypen-Regeln).
pub const V_REFTYP: &str = "ungueltig:reftyp";
/// Falscher Werttyp in `select` mit Typen.
pub const V_SELECT: &str = "ungueltig:select";
/// Import einer globalen Variablen mit unzulaessiger Mutabilitaet (2.0:
/// nur `global.get` importieren).
pub const V_IMPORT_GLOBAL: &str = "ungueltig:import-global";
/// `memory.init`/`table.init` mit falschem Elementtyp.
pub const V_BULK: &str = "ungueltig:bulk";
/// Stapel unterlaufen (Pop aus leerem Stapel).
pub const V_STAPEL: &str = "ungueltig:stapel";
/// Hartgrenze (M3/M4/M5).
pub const V_GRENZE: &str = "ungueltig:grenze";

// ---- Linkfehler --------------------------------------------------------------
pub const L_IMPORT: &str = "link:import";
pub const L_EXPORT: &str = "link:export";
pub const L_START: &str = "link:start";
pub const L_SPEICHER: &str = "link:speicher";
pub const L_TABELLE: &str = "link:tabelle";
pub const L_GLOBAL: &str = "link:global";
pub const L_SIGNATUR: &str = "link:signatur";
pub const L_GRENZE: &str = "link:grenze";

// ---- Traps -------------------------------------------------------------------
pub const T_UNERREICHBAR: &str = "trap:unerreichbar";
pub const T_SPEICHER_LESEN: &str = "trap:speicher-lesen";
pub const T_SPEICHER_SCHREIBEN: &str = "trap:speicher-schreiben";
pub const T_SPEICHER_GROESSER: &str = "trap:speicher-groesser";
pub const T_DIVISION: &str = "trap:division";
pub const T_OVERFLOW: &str = "trap:overflow";
pub const T_UNGUELTIGE_KONVERSION: &str = "trap:konversion";
pub const T_TABELLE_LESEN: &str = "trap:tabelle-lesen";
pub const T_TABELLE_SCHREIBEN: &str = "trap:tabelle-schreiben";
pub const T_TABELLE_GROESSER: &str = "trap:tabelle-groesser";
pub const T_UNINITIALISIERTE_TABELLE: &str = "trap:tabelle-uninitialisiert";
pub const T_INDIREKT: &str = "trap:indirekt";
pub const T_BRENNSTOFF: &str = "trap:brennstoff";
pub const T_AUFRUFTIEFE: &str = "trap:aufruftiefe";
pub const T_FREMD: &str = "trap:fremdfunktion";
pub const T_EINBETTUNG: &str = "trap:einbettung";
pub const T_DIVERSE: &str = "trap:sonstiges";

impl Fehler {
    /// Benannte Fehlerart - DER Testhaken (M8).
    pub fn art(&self) -> &'static str {
        match self {
            Fehler::Form(a, _) => a,
            Fehler::Ungueltig(a, _) => a,
            Fehler::Link(a, _) => a,
            Fehler::Trap(a, _) => a,
            Fehler::Brennstoff { .. } => T_BRENNSTOFF,
        }
    }

    /// Kurz: `art: text`.
    pub fn kurz(&self) -> String {
        match self {
            Fehler::Form(a, t)
            | Fehler::Ungueltig(a, t)
            | Fehler::Link(a, t)
            | Fehler::Trap(a, t) => {
                if t.is_empty() {
                    (*a).to_string()
                } else {
                    format!("{a}: {t}")
                }
            }
            Fehler::Brennstoff { verbraucht, grenze } => {
                format!("{T_BRENNSTOFF}: {verbraucht} Befehle verbraucht, Grenze {grenze}")
            }
        }
    }

    /// Ist das ein Trap aus der Laufzeit?
    pub fn ist_trap(&self) -> bool {
        matches!(self, Fehler::Trap(..) | Fehler::Brennstoff { .. })
    }

    pub fn form(a: &'static str, t: impl Into<String>) -> Self {
        Fehler::Form(a, t.into())
    }
    pub fn ungueltig(a: &'static str, t: impl Into<String>) -> Self {
        Fehler::Ungueltig(a, t.into())
    }
    pub fn link(a: &'static str, t: impl Into<String>) -> Self {
        Fehler::Link(a, t.into())
    }
    pub fn trap(a: &'static str, t: impl Into<String>) -> Self {
        Fehler::Trap(a, t.into())
    }
}

impl fmt::Display for Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kurz())
    }
}

impl std::error::Error for Fehler {}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Wert sichern (Fehlerart + Text).
    #[test]
    fn art_ist_stabil() {
        let e = Fehler::form(ART_MAGIE, "kein \\0asm");
        assert_eq!(e.art(), "form:magie");
        assert_eq!(e.kurz(), "form:magie: kein \\0asm");
        assert!(!e.ist_trap());
        let b = Fehler::Brennstoff {
            verbraucht: 7,
            grenze: 5,
        };
        assert_eq!(b.art(), "trap:brennstoff");
        assert!(b.ist_trap());
        assert!(b.kurz().contains('7'));
        assert_eq!(Fehler::form(ART_LEB, "").kurz(), "form:leb128");
    }
}
