//! # narsil-wasm — WebAssembly-Engine in reinem Rust (nur std, kein `unsafe`)
//!
//! Rolle WASM, Punkte W1–W5 aus `team/AUTONOM.txt`, Abschnitt NEUE ROLLEN.
//! Grundlage: `team/ANFORDERUNGEN.txt` Z. 12 — die Entwicklerwerkzeuge des
//! Browsers verlangen WebAssembly-Debugging (Module, Funktionen,
//! Disassembly, Haltepunkte, Schritt, Stack/Locals/Speicher).
//!
//! ## Aufbau
//!
//! | Modul | Punkt | Inhalt |
//! |---|---|---|
//! | [`bytes`] | W1 | Lesekopf, LEB128 mit Byte-Obergrenze, Laengen vor Allokation |
//! | [`binaer`] | W1 | Binarformat-Parser, alle Sektionen, hart begrenzt |
//! | [`op`] | W1/W5 | Befehlssatz: lesen, binaer schreiben, WAT-Text |
//! | [`modell`] | W1 | Typen, Funktionen, Speicher, Tabellen, Export, Custom |
//! | [`pruefe`] | W1 | Validator nach WebAssembly 2.0 Core |
//! | [`wat`] | W3 | Textformat (WAT) lesen und schreiben |
//! | [`wast`] | W3 | `.wast`-Skriptleser (assert_*), Abnahme gegen die Testsuite |
//! | [`abnahme`] | W3 | Abnahmelauf gegen die offizielle Testsuite (`spectest`, `register`) |
//! | [`speicher`], [`tabelle`] | W2 | Speicher/Tabellen mit Grenzen |
//! | [`lauf`] | W2 | Store, Instanz, Fremdfunktionen, Brennstoff |
//! | [`interp`] | W2 | Befehls-Interpreter (alle MVP + 2.0-Erweiterungen) |
//! | [`js`] | W4 | JS-Anbindung (Merkmals-Haken) |
//! | [`debug`] | W5 | Haltepunkte, Einzelschritt, Stapel, Locals, Disassembly, DWARF |
//!
//! ## Beispiel
//!
//! ```rust
//! use narsil_wasm::{Grenzen, binaer, pruefe};
//! let bytes: Vec<u8> = vec![0x00, 0x61, 0x73, 0x6d, 1, 0, 0, 0];
//! let m = binaer::lies_modul(&bytes, &Grenzen::standard())?;
//! pruefe::modul(&m, &Grenzen::standard())?;
//! # Ok::<(), narsil_wasm::Fehler>(())
//! ```

pub mod abnahme;
pub mod binaer;
pub mod bytes;
pub mod debug;
pub mod dwarf;
pub mod fehler;
pub mod grenzen;
pub mod interp;
pub mod js;
pub mod lauf;
pub mod modell;
pub mod op;
pub mod pruefe;
pub mod schnell;
pub mod speicher;
pub mod tabelle;
pub mod wast;
pub mod wat;
pub mod wert;

pub use fehler::Fehler;
pub use grenzen::Grenzen;
pub use lauf::{Instanz, Lauf, Store};
pub use modell::Modul;
pub use wert::Wert;

/// Kurzfassung: Bytes -> geprueftes Modul (W1 in einem Aufruf).
pub fn modul_pruefen(bytes: &[u8], g: &Grenzen) -> Result<Modul, Fehler> {
    let m = binaer::lies_modul(bytes, g)?;
    pruefe::modul(&m, g)?;
    Ok(m)
}
