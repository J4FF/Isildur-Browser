//! Benannte Fehler der Engine (M8: jede boesartige Eingabe endet mit einem
//! dieser Fehler, nie mit Panic).

use std::fmt;

/// Benannter Fehler eines JS-Laufs. Jede Variante ist ein begrenztes
/// Ergebnis: Der Aufrufer erhaelt Text mit Zeile/Spalte, wo sinnvoll.
#[derive(Debug, Clone, PartialEq)]
pub enum JsFehler {
    /// Lexikalischer oder syntaktischer Fehler mit Position (1-basiert).
    Syntax {
        /// Zeile (1-basiert).
        zeile: u32,
        /// Spalte (1-basiert, in Zeichen).
        spalte: u32,
        /// Beschreibung (deutsch).
        nachricht: String,
    },
    /// Laufzeitfehler aus JS-Sicht (`TypeError: ...` u.ae.), inkl.
    /// nicht gefangener geworfener Werte (`Unbehandelt: ...`).
    Laufzeit(String),
    /// Zugriff auf nicht deklarierte Variable / TDZ-Verletzung.
    Referenz(String),
    /// Heap-Grenze (`max_heap_bytes`) oder Einzelstring-Grenze
    /// (`max_string_laenge`) ueberschritten.
    Speicher(String),
    /// Schrittgrenze (`max_schritte`) oder 2-s-Wanduhr ueberschritten.
    Zeitlimit(String),
    /// JS-Aufruftiefe (`max_aufruftiefe`) ueberschritten.
    AufrufZuTief(String),
}

impl JsFehler {
    /// Kurzer, stabiler Variantenname fuer Tests (M7: Ergebnis zusichern).
    pub fn art(&self) -> &'static str {
        match self {
            JsFehler::Syntax { .. } => "Syntax",
            JsFehler::Laufzeit(_) => "Laufzeit",
            JsFehler::Referenz(_) => "Referenz",
            JsFehler::Speicher(_) => "Speicher",
            JsFehler::Zeitlimit(_) => "Zeitlimit",
            JsFehler::AufrufZuTief(_) => "AufrufZuTief",
        }
    }
}

impl fmt::Display for JsFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsFehler::Syntax {
                zeile,
                spalte,
                nachricht,
            } => write!(f, "Syntaxfehler {zeile}:{spalte}: {nachricht}"),
            JsFehler::Laufzeit(m) => write!(f, "Laufzeitfehler: {m}"),
            JsFehler::Referenz(m) => write!(f, "Referenzfehler: {m}"),
            JsFehler::Speicher(m) => write!(f, "Speichergrenze: {m}"),
            JsFehler::Zeitlimit(m) => write!(f, "Zeitlimit: {m}"),
            JsFehler::AufrufZuTief(m) => write!(f, "Aufruftiefe: {m}"),
        }
    }
}

impl std::error::Error for JsFehler {}
