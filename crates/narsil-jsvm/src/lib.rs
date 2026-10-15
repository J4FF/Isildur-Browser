//! Eigene JavaScript-Engine mit Bytecode-VM.
//!
//! Lexer, Parser und Compiler erzeugen Bytecode. Die VM verwaltet Aufrufe,
//! Heap, eingebaute Objekte und die Task-Schlangen fuer await, yield und DOM.
//!
//! # BEISPIEL
//!
//! ```rust
//! use narsil_jsvm::{Engine, Grenzen};
//! let mut e = Engine::neu(Grenzen::standard());
//! let w = e.ausfuehren("1 + 2 * 3").unwrap();
//! assert_eq!(w.als_text(), "7");
//! ```

mod ast;
mod asyncgen;
mod builtin;
mod builtin2;
mod builtin3;
mod compile;
pub mod debugger;
/// J28: der Debug-Haken (Haltepunkte, Schrittarten, Stapel, Locals).
mod debuglogik;
/// J1: DOM-API im Motor (ohne fremde Crates).
pub mod dom;
mod eigenschaften;
/// Release 15.10.2026: ES2022-2026 built-ins and conformance fixes (agent I).
mod es_modern;
mod es_regex;
/// JS2 (03.10.2026): die Regeln des `Array`-Objekts an **einer** Stelle —
/// Parameterzahl der eingebauten Funktionen (`Array.from.length === 1`) und
/// das generische Lesen array-ähnlicher Objekte (`every` auf einem `{length:0}`).
/// GEMESSEN, warum das nicht in `eigenschaften.rs` steht: die Zahl wird an
/// zwei Aufrufstellen gebraucht (`hole` und `getOwnPropertyDescriptor`), und
/// eine Zahl an zwei Stellen ist eine Zahl, die auseinanderläuft. Details und
/// Messung in der Datei; die Aenderung an `eigenschaften.rs` ist in
/// `team/WUNSCH_JS.txt` beantragt, nicht eigenmaechtig gemacht.
///
/// NUR die Deklaration. Diese Zeile aendert kein Verhalten.
mod eingebaut_array;
/// JS3 (Rolle CACHE, 04.10.2026): Intl-Minimum als eigene Datei
/// (DateTimeFormat/NumberFormat/PluralRules/Collator/RelativeTimeFormat,
/// de-DE/en-US-Grundformate, reine Funktionen + node-verglichene Tests).
/// NUR die Deklaration — kein Verhalten, Verdrahtung folgt mit Freigabe.
mod eingebaut_intl;
mod eingebaut_json;
mod eingebaut_menge;
/// JS3 (Rolle CACHE): die Property-Deskriptoren fuer `Object.create`,
/// `Object.defineProperty` und `Object.defineProperties`
/// (Spec 10.1.6). Die Logik steht in der Datei, hier nur der Pfad —
/// dieselbe Regel wie bei `eingebaut_array`.
mod eingebaut_object;
/// JS3 (Rolle CACHE, 04.10.2026): sprachnahes Web (Weisung CACHE_26) —
/// atob/btoa, crypto-Zufall. NUR die Deklaration, kein Verhalten.
/// `pub`, weil narsil/js.rs die Medienabfrage (matchMedia, CACHE_28)
/// wiederverwendet — reine Funktion, kein Engine-Zugriff.
pub mod eingebaut_web;
// WASM-Rolle, 03.10.2026 (NEUVERTEILUNG „HILFE BEI JS"): die
// RegExp-Rechenkerne (`source`, `flags`, `lastIndex`) fuer test262
// `built-ins/RegExp`. Verdrahtung in `builtin2.rs`/`builtin.rs` steht
// aus — siehe `team/BERICHT_WASM_HILFE_JS.txt`.
mod eingebaut_regexp;
/// JS2 (03.10.2026): die Schluesselgleichheit von `Map` und `Set`
/// (`SameValueZero`, ECMA-262 7.2.14). `schluessel_hash` (heap.rs) und
/// `schluessel_gleich` (builtin3.rs) widersprachen sich bei `-0`; die Regel
/// steht jetzt an **einer** Stelle. GEMESSEN: `node 22` sagt
/// `m.set(-0,42); m.get(+0)` -> `42`, der Motor `undefined`. Die zwei
/// Aufrufstellen sind gemeinsame Dateien — der Antrag steht in
/// `team/WUNSCH_JS.txt` (23:40). NUR die Deklaration, keine Verhaltensaenderung.
/// JS2 (02.10.2026): die Regeln von `JSON.parse`/`JSON.stringify`.
/// GEMESSEN: 5 rote Faelle in `JSON/parse`, weil `char::is_whitespace()` die
/// **Unicode**-Eigenschaft `White_Space` benutzt und JSON nur **vier**
/// Zeichen erlaubt (U+0020, U+0009, U+000A, U+000D). `node 22` bestaetigt
/// jeden der elf verbotenen Zeichen mit `SyntaxError`. NUR die Deklaration.
/// JS2 (02.10.2026, 01:25): die Regeln der `String.prototype`-Indexmethoden —
/// `ToIntegerOrInfinity` fuer die Position und `RequireObjectCoercible` fuer
/// `this`. GEMESSEN (node 22, 19 Proben): 10 davon falsch, alle aus diesen zwei
/// Regeln. NUR die Deklaration.
mod eingebaut_unicode;
mod eingebaut_text;
/// W16 (JS, 04.10.2026): ArrayBuffer / SharedArrayBuffer / DataView und die
/// Elementarten-Tabelle fuer die TypedArrays (Bericht JS_12).
mod eingebaut_typed;
/// A6: Ereignisse — Anmeldung im Skript, Ausloesen durch den Host.
pub mod ereignis;
mod fehler;
mod ganz;
mod grenzen;
mod heap;
/// W6B (J9): Host-Objekte mit Getter/Setter-Haken.
pub mod hostobjekt;
mod interp;
mod klassen;
mod lex;
/// J10: ES-Module (`import`/`export`, Modul-Lader).
mod module;
pub use module::{INLINE_MODUL_MARKE, MODUL_TYP_MARKE};
/// A4: `fetch()` — Herkunftsgrenze, Groessengrenze, Host-Anbindung.
pub mod netz;
// Release 10.10.2026: `Proxy` (ES2023 10.5 / 28.2).
mod proxy;
mod op;
mod ops;
mod parse;
mod regexmini;
mod regexp;
/// HTML §2.7 structured serialization (postMessage, workers, IndexedDB).
pub mod structured;
mod symbole;
mod text_einheiten;
/// J27: Unicode-Eigenschaften fuer den Regex (`\p{…}`).
mod unicode;
/// J27: die **generierten** Unicode-Daten (1714 Eigenschaftsnamen,
/// 53.172 Bereiche). Erzeugt von
/// `tests/t262_echt/unicode_daten_bauen.py` — nicht von Hand pflegen.
mod unicode_daten;
// QUIC, 04.10.2026 (Regel 15, minimale Fremdaenderung, im BRETT gemeldet):
// die **generierte** Normalisierungstabelle (2424/4678/544/699 Eintraege).
mod unicode_norm;
mod unicode_eigenschaften;
mod url;
mod vm;
/// Lazily loaded web platform APIs (WebAssembly, …) — 2026-10-15.
mod web_audio;
mod web_audio_render;
mod web_compression;
mod web_crypto;
mod web_media;
mod web_media_demux;
mod web_net;
mod web_platform;
mod web_wasm;
mod wert;
mod zone;
mod tzdata;
/// `%TypedArray%.prototype` methods — 2026-10-15.
mod typed_array_methods;

pub use debugger::{HakenZustand, Haltepunkt, NamWert, RahmenInfo, SchrittArt, SkriptTabelle};
pub use fehler::JsFehler;
pub use grenzen::Grenzen;
/// W6B (J9): Host-Objekte mit Getter/Setter-Haken.
pub use hostobjekt::{
    Closures, Closures as _Closures2, HOST_MERKE, HostGetter, HostObjektHaken, HostSetter, METHODE,
    haken, host_id,
};
mod namespace;
pub use namespace::{attribute_name, attribute_namespace_name, element_name, namespace_name, prefix_ok};
pub use interp::{Engine, PREVENT_DEFAULT, ereignis_als_wert};
pub use wert::Wert;
/// Release 10.10.2026: die lokale Zone vom Host (Windows: `narsil-window`).
pub use zone::{lokale_zone_setzen, windows_zone_iana};

/// Release 2026-10-15: HTML §4.10.5.3.6 `pattern` for the host's selector
/// matching (`:valid`/`:invalid`): whether `text` matches `^(?:pattern)$`
/// compiled with the `v` flag. `None` when the pattern does not compile
/// (then the attribute is ignored).
#[must_use]
pub fn pattern_matches(pattern: &str, text: &str) -> Option<bool> {
    let anchored = format!("^(?:{pattern})$");
    let (knoten, gruppen, _) = regexmini::kompilieren_mit_flags(&anchored, "v").ok()?;
    let schalter = regexmini::schalter_aus("v").ok()?;
    let units = regexmini::als_einheiten(text);
    Some(regexmini::suchen_einheiten(&knoten, gruppen, schalter, true, &units, 0).is_some())
}
