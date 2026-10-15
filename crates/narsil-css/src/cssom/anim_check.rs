//! Release 15.10.2026: style sheet declarations of transition and animation
//! longhands that their grammar rejects are dropped, so the earlier
//! declaration (or the initial value) applies (CSS Syntax 3 §8; WPT
//! css-transitions transition-duration-001: `-5s`, `foobar`). The cascade
//! keeps these properties as text and would otherwise hand the invalid text
//! on as their computed value.

/// The longhands this applies to (unprefixed; the cascade keeps them as
/// text).
const CHECKED: &[&str] = &[
    "transition-duration",
    "transition-delay",
    "transition-timing-function",
    "transition-behavior",
    "animation-duration",
    "animation-delay",
    "animation-timing-function",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "animation-composition",
];

/// Whether the declaration `name: value` is invalid and must be dropped.
/// Values with substitution functions (`var()`, `attr()`, …) are decided at
/// computed-value time and are kept.
pub fn invalid_declaration(name: &str, value: &str) -> bool {
    if !CHECKED.contains(&name) {
        return false;
    }
    let kv = super::syntax::komponenten(value);
    if super::block::hat_ersetzung(&kv) {
        return false;
    }
    let keyword = value.trim();
    if ["inherit", "initial", "unset", "revert", "revert-layer"].iter().any(|k| keyword.eq_ignore_ascii_case(k)) {
        return false;
    }
    super::lang_lesen(name, &kv).is_none()
}

#[cfg(test)]
mod tests {
    use super::invalid_declaration;

    #[test]
    fn invalid_times_are_dropped() {
        assert!(invalid_declaration("transition-duration", "-5s"));
        assert!(invalid_declaration("transition-duration", "foobar"));
        assert!(invalid_declaration("transition-delay", "foobar"));
        assert!(!invalid_declaration("transition-delay", "-5s"));
        assert!(!invalid_declaration("transition-duration", "1s, 0.1s, 10ms"));
        assert!(!invalid_declaration("transition-duration", "var(--d)"));
        assert!(!invalid_declaration("animation-duration", "auto"));
        assert!(!invalid_declaration("animation-iteration-count", "infinite"));
        assert!(!invalid_declaration("transition-timing-function", "cubic-bezier(.4,0,.2,1)"));
        assert!(!invalid_declaration("width", "foobar"));
    }
}
