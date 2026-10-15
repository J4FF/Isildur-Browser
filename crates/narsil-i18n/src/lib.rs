//! Translations of Narsil's own user interface.
//!
//! # Model
//!
//! English is the source language. Every UI string in the code is written in
//! English and passed through [`tr`] (or [`trf!`] when it has arguments):
//!
//! ```
//! use narsil_i18n::{Language, tr, tr_in, trf};
//! let _guard = narsil_i18n::scoped(Language::English);
//! assert_eq!(tr("New tab"), "New tab");
//! assert_eq!(tr_in(Language::German, "New tab"), "Neuer Tab");
//! assert_eq!(trf!("{} of {}", 2, 5), "2 of 5");
//! ```
//!
//! Every other language has exactly one table in this crate (German:
//! `src/de.rs`) that maps the English source string to its translation. A
//! string without an entry falls back to English, so a missing translation is
//! visible but never breaks the UI.
//!
//! # The current language
//!
//! The language is process-wide ([`set_language`], English by default), so
//! every window and every thread shows the same language. Two overrides sit
//! on top of it, checked in this order by [`current`]:
//!
//! 1. A thread-local override installed with [`scoped`]. Tests use it, so a
//!    test that checks German text does not change the language of other
//!    tests running in parallel threads.
//! 2. The environment variable `NARSIL_LANGUAGE` (`en` or `de`), read once.
//!    It is meant for tests and diagnostics and wins over the user setting.
//!
//! # Adding a language
//!
//! 1. Add a variant to [`Language`] and extend [`Language::ALL`],
//!    [`Language::code`], [`Language::native_name`] and
//!    [`Language::from_code`].
//! 2. Add a table file next to `de.rs` (same shape:
//!    `pub static TABLE: &[(&str, &str)]`) and return it from [`table`].
//! 3. Run `cargo test -p narsil-i18n`: the key check lists every UI string of
//!    the workspace that the new table does not translate yet.
//!
//! # Keys that are not written at the call site
//!
//! When a key is stored in a constant or table and translated later with
//! `tr(value)`, wrap the literal in [`tr_key`]. It returns the key unchanged
//! but lets the key check find it.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::{Display, Write as _};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

mod de;

/// A language the user interface can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    /// The source language and the default.
    #[default]
    English,
    German,
}

impl Language {
    /// Every language, in the order the settings list shows them.
    pub const ALL: [Language; 2] = [Language::English, Language::German];

    /// The ISO 639-1 code, used in the settings file and in
    /// `NARSIL_LANGUAGE`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
        }
    }

    /// The name of the language in that language. The settings list shows
    /// these names untranslated, so a user can always find their language.
    #[must_use]
    pub const fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
        }
    }

    /// Parses a language code or name: `en`, `de`, `de-DE`, `de_DE.UTF-8`,
    /// `English`, `Deutsch`, `German` (case does not matter).
    #[must_use]
    pub fn from_code(code: &str) -> Option<Language> {
        let lower = code.trim().to_ascii_lowercase();
        let base = lower.split(['-', '_', '.', '@']).next().unwrap_or("");
        match base {
            "en" | "english" => Some(Language::English),
            "de" | "deutsch" | "german" => Some(Language::German),
            _ => None,
        }
    }

    const fn index(self) -> u8 {
        match self {
            Language::English => 0,
            Language::German => 1,
        }
    }

    const fn from_index(index: u8) -> Language {
        match index {
            1 => Language::German,
            _ => Language::English,
        }
    }
}

/// The process-wide language, as [`Language::index`].
static GLOBAL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// The thread-local override installed by [`scoped`].
    static OVERRIDE: Cell<Option<Language>> = const { Cell::new(None) };
}

/// The name of the environment override.
pub const ENV_VAR: &str = "NARSIL_LANGUAGE";

/// The language forced by `NARSIL_LANGUAGE`, if the variable is set to a
/// known code. Read once per process.
#[must_use]
pub fn env_language() -> Option<Language> {
    static ENV: OnceLock<Option<Language>> = OnceLock::new();
    *ENV.get_or_init(|| {
        std::env::var(ENV_VAR)
            .ok()
            .and_then(|v| Language::from_code(&v))
    })
}

/// The language the UI is shown in right now (see the module docs for the
/// order of the overrides).
#[must_use]
pub fn current() -> Language {
    if let Some(lang) = OVERRIDE.with(Cell::get) {
        return lang;
    }
    if let Some(lang) = env_language() {
        return lang;
    }
    Language::from_index(GLOBAL.load(Ordering::Relaxed))
}

/// Sets the UI language.
///
/// Normally this changes the process-wide language. If the calling thread
/// has a [`scoped`] override, only that override changes: a test that clicks
/// the language setting then stays isolated from other tests.
pub fn set_language(lang: Language) {
    let scoped = OVERRIDE.with(|o| {
        if o.get().is_some() {
            o.set(Some(lang));
            true
        } else {
            false
        }
    });
    if !scoped {
        GLOBAL.store(lang.index(), Ordering::Relaxed);
    }
}

/// Sets a language for the calling thread only, without a guard (it ends
/// with the thread). Meant for test processes, where every test runs on its
/// own thread; the app itself uses [`set_language`].
pub fn set_thread_language(lang: Language) {
    OVERRIDE.with(|o| o.set(Some(lang)));
}

/// Does the calling thread have its own language ([`scoped`] or
/// [`set_thread_language`])?
#[must_use]
pub fn has_thread_language() -> bool {
    OVERRIDE.with(|o| o.get().is_some())
}

/// Restores the previous thread-local override when dropped.
#[must_use = "the override ends when the guard is dropped"]
#[derive(Debug)]
pub struct ScopedLanguage {
    previous: Option<Language>,
}

impl Drop for ScopedLanguage {
    fn drop(&mut self) {
        let previous = self.previous;
        OVERRIDE.with(|o| o.set(previous));
    }
}

/// Shows the UI in `lang` on the calling thread until the guard is dropped.
/// Wins over `NARSIL_LANGUAGE` and over [`set_language`] calls from other
/// threads.
pub fn scoped(lang: Language) -> ScopedLanguage {
    let previous = OVERRIDE.with(|o| o.replace(Some(lang)));
    ScopedLanguage { previous }
}

/// The translation table of a language: pairs of (English source, translation).
/// English has no table.
#[must_use]
pub fn table(lang: Language) -> &'static [(&'static str, &'static str)] {
    match lang {
        Language::English => &[],
        Language::German => de::TABLE,
    }
}

fn map(lang: Language) -> Option<&'static HashMap<&'static str, &'static str>> {
    static GERMAN: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let cell = match lang {
        Language::English => return None,
        Language::German => &GERMAN,
    };
    Some(cell.get_or_init(|| table(lang).iter().copied().collect()))
}

/// Translates an English UI string into the current language. Strings
/// without a translation come back unchanged.
#[must_use]
pub fn tr(key: &str) -> &str {
    tr_in(current(), key)
}

/// Translates an English UI string into `lang`.
#[must_use]
pub fn tr_in(lang: Language, key: &str) -> &str {
    match map(lang).and_then(|m| m.get(key).copied()) {
        Some(translation) => translation,
        None => key,
    }
}

/// Does `lang` have its own entry for `key`? (English always does.)
#[must_use]
pub fn has_translation(lang: Language, key: &str) -> bool {
    match map(lang) {
        None => true,
        Some(m) => m.contains_key(key),
    }
}

/// Marks a string as a translation key without translating it. Use it where
/// the key is stored first and translated later with `tr(value)`.
#[must_use]
pub const fn tr_key(key: &'static str) -> &'static str {
    key
}

/// Fills the placeholders of a (translated) template.
///
/// `{}` takes the next argument, `{0}` .. `{9}` take an argument by position
/// (so a translation can reorder them), `{{` and `}}` are literal braces. A
/// placeholder without an argument stays as written.
#[must_use]
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0usize;
    let mut rest = template;
    while let Some(pos) = rest.find(['{', '}']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if tail.starts_with("{{") {
            out.push('{');
            rest = &tail[2..];
        } else if tail.starts_with("}}") {
            out.push('}');
            rest = &tail[2..];
        } else if tail.starts_with('{') {
            let Some(end) = tail.find('}') else {
                out.push_str(tail);
                rest = "";
                break;
            };
            let inner = &tail[1..end];
            let index = if inner.is_empty() {
                let i = next;
                next += 1;
                Some(i)
            } else {
                inner.parse::<usize>().ok()
            };
            match index.and_then(|i| args.get(i)) {
                Some(arg) => {
                    let _ = write!(out, "{arg}");
                }
                None => out.push_str(&tail[..=end]),
            }
            rest = &tail[end + 1..];
        } else {
            out.push('}');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Translates a template and fills its placeholders:
/// `trf!("Tab {} of {}", nr, count)`.
#[macro_export]
macro_rules! trf {
    ($key:expr $(, $arg:expr)* $(,)?) => {
        $crate::fill($crate::tr($key), &[$(&$arg as &dyn ::std::fmt::Display),*])
    };
}

/// The number of placeholders in a template (`{}` and `{N}`), used by the
/// table checks.
#[must_use]
pub fn placeholders(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(pos) = rest.find(['{', '}']) {
        let tail = &rest[pos..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
        } else if tail.starts_with('{') {
            match tail.find('}') {
                Some(end) => {
                    found.push(tail[1..end].to_string());
                    rest = &tail[end + 1..];
                }
                None => break,
            }
        } else {
            rest = &tail[1..];
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_default_and_returns_the_key() {
        let _g = scoped(Language::English);
        assert_eq!(tr("New tab"), "New tab");
        assert_eq!(tr("A string nobody translated"), "A string nobody translated");
    }

    #[test]
    fn german_lookup_and_fallback() {
        let _g = scoped(Language::German);
        assert_eq!(tr("New tab"), "Neuer Tab");
        assert_eq!(tr("Settings"), "Einstellungen");
        // No entry: the English source is shown.
        assert_eq!(tr("A string nobody translated"), "A string nobody translated");
        assert!(has_translation(Language::German, "New tab"));
        assert!(!has_translation(Language::German, "A string nobody translated"));
    }

    #[test]
    fn tr_in_ignores_the_current_language() {
        let _g = scoped(Language::German);
        assert_eq!(tr_in(Language::English, "New tab"), "New tab");
        assert_eq!(tr_in(Language::German, "New tab"), "Neuer Tab");
    }

    #[test]
    fn scoped_override_is_restored_and_nested() {
        let outer = scoped(Language::German);
        assert_eq!(current(), Language::German);
        {
            let _inner = scoped(Language::English);
            assert_eq!(current(), Language::English);
        }
        assert_eq!(current(), Language::German);
        drop(outer);
        assert_eq!(OVERRIDE.with(Cell::get), None);
    }

    #[test]
    fn set_language_inside_a_scope_stays_on_the_thread() {
        let _g = scoped(Language::English);
        let before = GLOBAL.load(Ordering::Relaxed);
        set_language(Language::German);
        assert_eq!(current(), Language::German);
        assert_eq!(GLOBAL.load(Ordering::Relaxed), before, "the global value must not change");
        let other = std::thread::spawn(|| OVERRIDE.with(Cell::get)).join().unwrap();
        assert_eq!(other, None, "other threads do not see the override");
    }

    #[test]
    fn language_codes() {
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code(" DE "), Some(Language::German));
        assert_eq!(Language::from_code("de_DE.UTF-8"), Some(Language::German));
        assert_eq!(Language::from_code("en-US"), Some(Language::English));
        assert_eq!(Language::from_code("Deutsch"), Some(Language::German));
        assert_eq!(Language::from_code("fr"), None);
        assert_eq!(Language::from_code(""), None);
        for lang in Language::ALL {
            assert_eq!(Language::from_code(lang.code()), Some(lang));
            assert_eq!(Language::from_code(lang.native_name()), Some(lang));
            assert_eq!(Language::from_index(lang.index()), lang);
        }
    }

    #[test]
    fn fill_placeholders() {
        assert_eq!(fill("{} of {}", &[&1, &3]), "1 of 3");
        assert_eq!(fill("{1} before {0}", &[&"a", &"b"]), "b before a");
        assert_eq!(fill("{{literal}} {}", &[&7]), "{literal} 7");
        assert_eq!(fill("missing {}", &[]), "missing {}");
        assert_eq!(fill("no placeholders", &[&1]), "no placeholders");
        assert_eq!(fill("odd } brace", &[]), "odd } brace");
        assert_eq!(fill("open { brace", &[]), "open { brace");
    }

    #[test]
    fn trf_translates_then_fills() {
        let _g = scoped(Language::German);
        assert_eq!(trf!("{} of {}", 2, 5), "2 von 5");
        let _e = scoped(Language::English);
        assert_eq!(trf!("{} of {}", 2, 5), "2 of 5");
    }

    #[test]
    fn placeholder_listing() {
        assert_eq!(placeholders("a {} b {0} {{x}}"), vec!["".to_string(), "0".to_string()]);
        assert!(placeholders("plain").is_empty());
    }
}
