//! Lexer und Parser der Narsil-Sprache (Version 0).
//!
//! Diese Datei ist VERTRAG und gehoert Agent1. Sie wurde bewusst so
//! aufgeteilt, dass zwei Worker unabhaengig an ihr arbeiten koennen:
//!
//!   src/token.rs   — Vertrag, FERTIG. Nicht aendern.
//!   src/lexer.rs   — TASK_0007 (Lexer)
//!   src/parser.rs  — TASK_0008 (Parser, baut den Document-Baum)
//!   tests/         — TASK_0008
//!
//! STAND 03:15: TASK_0008 ist angenommen. `parse_tokens` ist von Agent4
//! als zusaetzliche, oeffentliche Stufe eingefuehrt worden, damit der
//! Parser ohne Lexer testbar war. Sie gehoert jetzt zur oeffentlichen
//! API, weil `narsil-app` sie fuer Fehlermeldungen ohne Lexer nutzen
//! kann.
//!
//! Die Modul-Deklarationen stehen hier, damit BEIDE Teile sofort fuer sich
//! allein bauen und testen koennen. Ohne das wuerde der Lexer-Worker nicht
//! bauen koennen, solange `parser.rs` noch fehlt.
//!
//! Sprachspezifikation: docs/NARSIL_LANGUAGE_SPEC_v0.txt

pub mod lexer;
pub mod parser;
pub mod token;

pub use lexer::{LexError, Lexer, tokenize};
pub use parser::{parse, parse_tokens};
pub use token::{Token, TokenKind};

use narsil_dom::{Position, Span, position};

/// Fehler beim Umwandeln von Quelltext in einen Dokumentbaum.
///
/// Trägt immer eine Position, damit der Aufrufer die Stelle im Quelltext
/// markieren kann. `message` ist menschenlesbar und auf Deutsch.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// Was war falsch.
    pub message: String,
    /// Wo im Quelltext es falsch war.
    pub span: Span,
}

impl ParseError {
    /// Erzeugt einen Fehler an einer Stelle.
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// Die Stelle als Zeile und Spalte.
    ///
    /// DER EINGEBRAUCH, und er ist der ganze Grund fuer diese Methode:
    /// `span` ist ein Byte-Offset, und ein Byte-Offset ist fuer einen
    /// Menschen keine Stelle, auf die man springen kann. Der Aufrufer
    /// hat den Quelltext — der Parser haelt ihn nicht — und rechnet
    /// damit die Position.
    ///
    /// Die Byte-Angabe bleibt daneben erhalten. Sie ist der Anker fuer
    /// alles Programmatische, und die Zeile ist die Angabe fuer den
    /// Menschen. Es kommt etwas dazu, es faellt nichts weg.
    pub fn position_in(&self, quelle: &str) -> Position {
        position(quelle, self.span.start)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Ohne den Quelltext gibt es keine Zeile — dann bleibt es beim
        // Byte. EIN Formular an zwei Orten waere eine Stelle, an der
        // sie auseinanderlaufen.
        write!(
            f,
            "Narsil-Fehler an Byte {} (Laenge {}): {}",
            self.span.start, self.span.len, self.message
        )
    }
}

/// Schreibt eine Fehlermeldung mit Zeile und Spalte.
///
/// DAS ist der Weg, den ein Aufrufer nimmt, der dem Menschen mehr als
/// "Byte 239" zeigen will. Es steht als FREIE Funktion und nicht als
/// Methode, weil sie den Quelltext braucht und der Fehler ihn nicht hat.
///
/// GEMESSEN, warum nicht in `Display`: `Display` hat nur `&self`, und
/// fuer Zeile und Spalte braucht es den Quelltext. Ein Fehler, der sich
/// seinen Quelltext merkt, waere ein zweites, kopierbares Exemplar
/// jeder Datei im Speicher.
pub fn meldung(fehler: &dyn std::fmt::Display, quelle: &str, byte: u32, laenge: u32) -> String {
    let p = position(quelle, byte);
    format!("{fehler} — {p} (Laenge {laenge})")
}

impl std::error::Error for ParseError {}
