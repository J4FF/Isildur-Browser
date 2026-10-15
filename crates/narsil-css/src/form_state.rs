//! Release 2026-10-15: the form-control pseudo-classes of HTML §4.16.3
//! (`:disabled`, `:enabled`, `:read-write`, `:read-only`, `:required`,
//! `:optional`, `:checked`, `:indeterminate`, `:default`, `:in-range`,
//! `:out-of-range`, `:valid`, `:invalid`, `:placeholder-shown`, `:open`,
//! `:any-link`), computed from the element tree through [`Element`].
//!
//! MEASURED before (WPT html/semantics/selectors/pseudo-classes, 107
//! failures): `:disabled`, `:enabled` and `:read-only` never matched,
//! `:read-write` always did, and `:valid`, `:required`, `:in-range` and
//! `:default` were unknown names that dropped the whole rule.
//!
//! The value of a control is the host's dirty value
//! ([`Element::form_value`]) or its content attribute; a `pattern` needs the
//! host's regular expressions ([`Element::pattern_matches`]), without one it
//! counts as satisfied. Every walk is bounded (`MAX_DEPTH`, `MAX_TREE`).

use crate::typen::Element;
use std::borrow::Cow;

const MAX_DEPTH: usize = 512;
const MAX_TREE: usize = 20_000;

/// The internal attributes of the host's control state (see
/// `narsil_fmt_html::dom::ATTR_ANGEHAKT` and siblings).
const ATTR_CHECKED: &str = " narsil-checked";
const ATTR_UNCHECKED: &str = " narsil-unchecked";
const ATTR_INDETERMINATE: &str = " narsil-indeterminate";

fn is<E: Element>(el: &E, tag: &str) -> bool {
    el.tag().eq_ignore_ascii_case(tag)
}

/// The state of an `<input>`'s `type` (lowercase; missing or unknown is
/// `text`).
fn input_type<E: Element>(el: &E) -> String {
    const TYPES: [&str; 22] = [
        "hidden", "text", "search", "tel", "url", "email", "password", "date", "month", "week", "time",
        "datetime-local", "number", "range", "color", "checkbox", "radio", "file", "submit", "image", "reset", "button",
    ];
    let t = el.attribut("type").map(str::to_ascii_lowercase).unwrap_or_default();
    if TYPES.contains(&t.as_str()) { t } else { "text".to_string() }
}

/// Pre-order walk over the element descendants of `root`; stops at the
/// first element for which `f` is true.
fn any_descendant<E: Element>(root: &E, f: &mut dyn FnMut(&E) -> bool) -> bool {
    let mut stack: Vec<E> = Vec::new();
    if let Some(c) = root.erstes_kind() {
        stack.push(c);
    }
    let mut n = 0;
    while let Some(x) = stack.pop() {
        n += 1;
        if n > MAX_TREE {
            return false;
        }
        if f(&x) {
            return true;
        }
        if let Some(s) = x.naechstes_geschwister() {
            stack.push(s);
        }
        if let Some(c) = x.erstes_kind() {
            stack.push(c);
        }
    }
    false
}

/// The nearest ancestor `<form>`.
fn ancestor_form<E: Element>(el: &E) -> Option<E> {
    let mut cur = el.eltern();
    let mut n = 0;
    while let Some(c) = cur {
        if is(&c, "form") {
            return Some(c);
        }
        n += 1;
        if n > MAX_DEPTH {
            return None;
        }
        cur = c.eltern();
    }
    None
}

/// The root of `el`'s tree; `None` when `el` is the root itself.
fn tree_root<E: Element>(el: &E) -> Option<E> {
    let mut cur = el.eltern()?;
    for _ in 0..MAX_DEPTH {
        match cur.eltern() {
            Some(p) => cur = p,
            None => return Some(cur),
        }
    }
    Some(cur)
}

// --------------------------------------------------------------- disabled

/// `el` is the first `<legend>` child of its parent.
fn first_legend<E: Element>(el: &E) -> bool {
    if !is(el, "legend") {
        return false;
    }
    let mut s = el.voriges_geschwister();
    let mut n = 0;
    while let Some(x) = s {
        if is(&x, "legend") {
            return false;
        }
        n += 1;
        if n > MAX_TREE {
            break;
        }
        s = x.voriges_geschwister();
    }
    true
}

/// A descendant of a `<fieldset disabled>` outside its first `<legend>`.
fn in_disabled_fieldset<E: Element>(el: &E) -> bool {
    let mut below_legend = first_legend(el);
    let mut cur = el.eltern();
    let mut n = 0;
    while let Some(c) = cur {
        if is(&c, "fieldset") && c.attribut("disabled").is_some() && !below_legend {
            return true;
        }
        below_legend = first_legend(&c);
        n += 1;
        if n > MAX_DEPTH {
            break;
        }
        cur = c.eltern();
    }
    false
}

/// `:disabled` (HTML §4.16.3): actually disabled controls, disabled
/// `<optgroup>`/`<option>` and disabled fieldsets.
pub fn is_disabled<E: Element>(el: &E) -> bool {
    let t = el.tag();
    if ["button", "input", "select", "textarea", "fieldset"].iter().any(|x| t.eq_ignore_ascii_case(x)) {
        return el.attribut("disabled").is_some() || in_disabled_fieldset(el);
    }
    if t.eq_ignore_ascii_case("optgroup") {
        return el.attribut("disabled").is_some();
    }
    if t.eq_ignore_ascii_case("option") {
        return el.attribut("disabled").is_some()
            || el.eltern().is_some_and(|p| is(&p, "optgroup") && p.attribut("disabled").is_some());
    }
    false
}

/// `:enabled`: the same elements when they are not disabled.
pub fn is_enabled<E: Element>(el: &E) -> bool {
    let t = el.tag();
    ["button", "input", "select", "textarea", "fieldset", "optgroup", "option"].iter().any(|x| t.eq_ignore_ascii_case(x))
        && !is_disabled(el)
}

// ------------------------------------------------------ read-write, required

fn readonly_applies(t: &str) -> bool {
    matches!(
        t,
        "text" | "search" | "url" | "tel" | "email" | "password" | "date" | "month" | "week" | "time" | "datetime-local"
            | "number"
    )
}

fn required_applies(t: &str) -> bool {
    !matches!(t, "hidden" | "range" | "color" | "submit" | "image" | "reset" | "button")
}

fn editable_state(v: &str) -> Option<bool> {
    if v.is_empty() || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("plaintext-only") {
        Some(true)
    } else if v.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// An editing host or editable element (`contenteditable`, inherited).
fn is_editable<E: Element>(el: &E) -> bool {
    if let Some(s) = el.attribut("contenteditable").and_then(editable_state) {
        return s;
    }
    let mut cur = el.eltern();
    let mut n = 0;
    while let Some(c) = cur {
        if let Some(s) = c.attribut("contenteditable").and_then(editable_state) {
            return s;
        }
        n += 1;
        if n > MAX_DEPTH {
            break;
        }
        cur = c.eltern();
    }
    false
}

/// `:read-write`: mutable text-like `<input>`s and `<textarea>`s, editing
/// hosts and editable elements; `:read-only` is everything else.
pub fn is_read_write<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        return readonly_applies(&input_type(el)) && el.attribut("readonly").is_none() && !is_disabled(el);
    }
    if is(el, "textarea") {
        return el.attribut("readonly").is_none() && !is_disabled(el);
    }
    is_editable(el)
}

/// `:required`.
pub fn is_required<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        return required_applies(&input_type(el)) && el.attribut("required").is_some();
    }
    (is(el, "select") || is(el, "textarea")) && el.attribut("required").is_some()
}

/// `:optional`: the controls `required` applies to, when they lack it.
pub fn is_optional<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        return required_applies(&input_type(el)) && el.attribut("required").is_none();
    }
    (is(el, "select") || is(el, "textarea")) && el.attribut("required").is_none()
}

// ------------------------------------------------- checked, indeterminate

/// Checkedness of a checkbox/radio, selectedness of an `<option>`: the
/// host's dirty state, otherwise the content attribute.
pub fn is_checked<E: Element>(el: &E) -> bool {
    if el.attribut(ATTR_CHECKED).is_some() {
        return true;
    }
    if el.attribut(ATTR_UNCHECKED).is_some() {
        return false;
    }
    if is(el, "input") {
        let t = input_type(el);
        return (t == "checkbox" || t == "radio") && el.attribut("checked").is_some();
    }
    is(el, "option") && (el.attribut("selected").is_some() || is_default_selected_option(el))
}

/// An option of a single-selection `<select>` counts as selected when it
/// is the first enabled option and no option is selected (HTML §4.10.7
/// "selectedness setting algorithm").
fn is_default_selected_option<E: Element>(el: &E) -> bool {
    if is_disabled(el) {
        return false;
    }
    let Some(parent) = el.eltern() else { return false };
    let in_group = is(&parent, "optgroup");
    let select = if in_group {
        match parent.eltern() {
            Some(g) if is(&g, "select") => g,
            _ => return false,
        }
    } else if is(&parent, "select") {
        parent
    } else {
        return false;
    };
    if select.attribut("multiple").is_some()
        || select.attribut("size").is_some_and(|s| s.trim().parse::<u32>().is_ok_and(|n| n > 1))
    {
        return false;
    }
    let selected = |o: &E| {
        o.attribut(ATTR_CHECKED).is_some() || (o.attribut("selected").is_some() && o.attribut(ATTR_UNCHECKED).is_none())
    };
    let enabled_option = |o: &E| is(o, "option") && !is_disabled(o);
    // No option of the select is selected.
    let mut n = 0;
    let mut c = select.erstes_kind();
    while let Some(x) = c {
        if is(&x, "option") && selected(&x) {
            return false;
        }
        if is(&x, "optgroup") {
            let mut o = x.erstes_kind();
            while let Some(y) = o {
                if is(&y, "option") && selected(&y) {
                    return false;
                }
                n += 1;
                if n > MAX_TREE {
                    return false;
                }
                o = y.naechstes_geschwister();
            }
        }
        n += 1;
        if n > MAX_TREE {
            return false;
        }
        c = x.naechstes_geschwister();
    }
    // No enabled option comes before `el`.
    let before = |start: Option<E>| {
        let mut s = start;
        let mut n = 0;
        while let Some(x) = s {
            if enabled_option(&x) {
                return true;
            }
            if is(&x, "optgroup") && x.attribut("disabled").is_none() {
                let mut o = x.erstes_kind();
                while let Some(y) = o {
                    if enabled_option(&y) {
                        return true;
                    }
                    o = y.naechstes_geschwister();
                }
            }
            n += 1;
            if n > MAX_TREE {
                return true;
            }
            s = x.voriges_geschwister();
        }
        false
    };
    if before(el.voriges_geschwister()) {
        return false;
    }
    !(in_group && el.eltern().is_some_and(|g| before(g.voriges_geschwister())))
}

/// Calls `f` for the members of `el`'s radio button group (same name, same
/// form owner, same tree; `el` included) until it returns true.
fn radio_group_any<E: Element>(el: &E, f: &mut dyn FnMut(&E) -> bool) -> bool {
    if f(el) {
        return true;
    }
    let Some(name) = el.attribut("name").filter(|n| !n.is_empty()).map(str::to_string) else { return false };
    let form_attr = el.attribut("form").map(str::to_string);
    let form = ancestor_form(el);
    let in_form = form.is_some();
    let mut member = |x: &E| {
        is(x, "input")
            && input_type(x) == "radio"
            && x.attribut("name") == Some(name.as_str())
            && x.attribut("form").map(str::to_string) == form_attr
            && (in_form || ancestor_form(x).is_none())
            && f(x)
    };
    match form {
        Some(fm) => any_descendant(&fm, &mut member),
        None => tree_root(el).is_some_and(|r| any_descendant(&r, &mut member)),
    }
}

/// `:indeterminate`: a checkbox with the `indeterminate` IDL flag, a radio
/// whose group has no checked button, a `<progress>` without `value`.
pub fn is_indeterminate<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        return match input_type(el).as_str() {
            "checkbox" => el.attribut(ATTR_INDETERMINATE).is_some(),
            "radio" => !radio_group_any(el, &mut |x| is_checked(x)),
            _ => false,
        };
    }
    is(el, "progress") && el.attribut("value").is_none()
}

// ----------------------------------------------------------------- default

fn is_submit_button<E: Element>(el: &E) -> bool {
    if is(el, "button") {
        return !el.attribut("type").is_some_and(|t| t.eq_ignore_ascii_case("reset") || t.eq_ignore_ascii_case("button"));
    }
    is(el, "input") && matches!(input_type(el).as_str(), "submit" | "image")
}

/// `el` is its form's default button: the first submit button of the form
/// in tree order (form owner by ancestry; a `form` attribute opts out).
fn is_default_button<E: Element>(el: &E) -> bool {
    if el.attribut("form").is_some() || ancestor_form(el).is_none() {
        return false;
    }
    let submit = |x: &E| is_submit_button(x) && x.attribut("form").is_none();
    let mut prev = el.voriges_geschwister();
    let mut up = el.eltern();
    let mut n = 0;
    loop {
        while let Some(s) = prev {
            if submit(&s) || any_descendant(&s, &mut |x| submit(x)) {
                return false;
            }
            n += 1;
            if n > MAX_TREE {
                return false;
            }
            prev = s.voriges_geschwister();
        }
        let Some(p) = up else { return false };
        if is(&p, "form") {
            return true;
        }
        if submit(&p) {
            return false;
        }
        prev = p.voriges_geschwister();
        up = p.eltern();
    }
}

/// `:default`: the form's default button, checkboxes/radios with
/// `checked`, options with `selected`.
pub fn is_default<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        return match input_type(el).as_str() {
            "checkbox" | "radio" => el.attribut("checked").is_some(),
            "submit" | "image" => is_default_button(el),
            _ => false,
        };
    }
    if is(el, "option") {
        return el.attribut("selected").is_some();
    }
    is(el, "button") && is_submit_button(el) && is_default_button(el)
}

// ------------------------------------------------------------ the value

/// A control's value: the host's dirty value, else the `value` attribute.
fn control_value<E: Element>(el: &E) -> Cow<'_, str> {
    if let Some(v) = el.form_value() {
        return v;
    }
    Cow::Borrowed(el.attribut("value").unwrap_or(""))
}

/// HTML §2.3.4.3 "valid floating-point number" (the whole string).
fn parse_float_strict(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'-') {
        i = 1;
    }
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > s
    };
    let int = digits(&mut i);
    let mut frac = false;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        frac = digits(&mut i);
        if !frac {
            return None;
        }
    }
    if !int && !frac {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if !digits(&mut i) {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// HTML §2.3.4.3 "rules for parsing floating-point number values" (a
/// valid prefix; trailing text is ignored).
fn parse_float_lenient(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\x0c' | b'\r') {
        i += 1;
    }
    let start = i;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut end = i;
    let mut any = i > ds;
    if i < b.len() && b[i] == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        end = i;
        any = true;
    }
    if !any {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let es = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > es {
            end = j;
        }
    }
    s[start..end].parse::<f64>().ok().filter(|v| v.is_finite())
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn num(s: &str, len: std::ops::RangeInclusive<usize>) -> Option<i64> {
    (len.contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// `YYYY-MM` -> (year, month).
fn parse_month_parts(s: &str) -> Option<(i64, i64)> {
    let (y, m) = s.rsplit_once('-')?;
    let (y, m) = (num(y, 4..=9)?, num(m, 2..=2)?);
    (y >= 1 && (1..=12).contains(&m)).then_some((y, m))
}

/// `YYYY-MM-DD` -> days since 1970-01-01.
fn parse_date(s: &str) -> Option<f64> {
    let (ym, d) = s.rsplit_once('-')?;
    let (y, m) = parse_month_parts(ym)?;
    let d = num(d, 2..=2)?;
    (d >= 1 && d <= days_in_month(y, m)).then(|| days_from_civil(y, m, d) as f64)
}

/// `HH:MM[:SS[.s{1,3}]]` -> milliseconds.
fn parse_time(s: &str) -> Option<f64> {
    let mut it = s.splitn(3, ':');
    let h = num(it.next()?, 2..=2)?;
    let m = num(it.next()?, 2..=2)?;
    let mut ms = 0.0;
    if let Some(sec) = it.next() {
        let (whole, frac) = match sec.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (sec, None),
        };
        let w = num(whole, 2..=2)?;
        if w > 59 {
            return None;
        }
        ms = w as f64 * 1000.0;
        if let Some(f) = frac {
            let v = num(f, 1..=3)?;
            ms += v as f64 * 10f64.powi(3 - f.len() as i32);
        }
    }
    (h <= 23 && m <= 59).then(|| (h * 3_600_000 + m * 60_000) as f64 + ms)
}

/// A value of a date/time/number `<input>` type as a number (days for
/// `date`, months for `month`, weeks for `week`, ms for `time` and
/// `datetime-local`).
fn parse_typed(t: &str, s: &str, strict: bool) -> Option<f64> {
    match t {
        "number" | "range" => {
            if strict {
                parse_float_strict(s)
            } else {
                parse_float_lenient(s)
            }
        }
        "date" => parse_date(s),
        "month" => parse_month_parts(s).map(|(y, m)| ((y - 1970) * 12 + m - 1) as f64),
        "week" => {
            let (y, w) = s.split_once("-W")?;
            let (y, w) = (num(y, 4..=9)?, num(w, 2..=2)?);
            (y >= 1 && (1..=53).contains(&w)).then(|| (y * 53 + w) as f64)
        }
        "time" => parse_time(s),
        "datetime-local" => {
            let (d, t) = s.split_once(['T', ' '])?;
            Some(parse_date(d)? * 86_400_000.0 + parse_time(t)?)
        }
        _ => None,
    }
}

fn has_datalist_ancestor<E: Element>(el: &E) -> bool {
    let mut cur = el.eltern();
    let mut n = 0;
    while let Some(c) = cur {
        if is(&c, "datalist") {
            return true;
        }
        n += 1;
        if n > MAX_DEPTH {
            break;
        }
        cur = c.eltern();
    }
    false
}

/// A candidate for constraint validation (HTML §4.10.20.2: not barred).
fn is_candidate<E: Element>(el: &E) -> bool {
    if is(el, "input") {
        let t = input_type(el);
        if matches!(t.as_str(), "hidden" | "reset" | "button") || (readonly_applies(&t) && el.attribut("readonly").is_some()) {
            return false;
        }
    } else if is(el, "textarea") {
        if el.attribut("readonly").is_some() {
            return false;
        }
    } else if is(el, "button") {
        if !is_submit_button(el) {
            return false;
        }
    } else if !is(el, "select") {
        return false;
    }
    !is_disabled(el) && !has_datalist_ancestor(el)
}

/// (underflow, overflow) of an `<input>` with range limitations.
fn range_state<E: Element>(el: &E) -> Option<(bool, bool)> {
    if !is(el, "input") {
        return None;
    }
    let t = input_type(el);
    if !matches!(t.as_str(), "number" | "range" | "date" | "month" | "week" | "time" | "datetime-local") {
        return None;
    }
    let min = el.attribut("min").and_then(|v| parse_typed(&t, v, false));
    let max = el.attribut("max").and_then(|v| parse_typed(&t, v, false));
    if t == "range" {
        // The range state always has limits, and sanitization clamps.
        return Some((false, false));
    }
    if min.is_none() && max.is_none() {
        return None;
    }
    let Some(v) = parse_typed(&t, control_value(el).trim_matches(['\n', '\r']), true) else { return Some((false, false)) };
    if t == "time" && let (Some(lo), Some(hi)) = (min, max) && lo > hi {
        // A reversed range: out of range in the gap between max and min.
        let gap = v < lo && v > hi;
        return Some((gap, gap));
    }
    Some((min.is_some_and(|m| v < m), max.is_some_and(|m| v > m)))
}

/// `:in-range` / `:out-of-range` (`want_in`).
pub fn matches_range<E: Element>(el: &E, want_in: bool) -> bool {
    if !is_candidate(el) {
        return false;
    }
    match range_state(el) {
        Some((u, o)) => (u || o) != want_in,
        None => false,
    }
}

fn valid_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    if local.is_empty()
        || !local.bytes().all(|c| c.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&c))
        || domain.is_empty()
    {
        return false;
    }
    domain.split('.').all(|l| {
        let b = l.as_bytes();
        !b.is_empty()
            && b.len() <= 63
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
            && b[0] != b'-'
            && b[b.len() - 1] != b'-'
    })
}

fn valid_absolute_url(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else { return false };
    let mut b = scheme.bytes();
    b.next().is_some_and(|c| c.is_ascii_alphabetic())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        && !rest.contains([' ', '\t', '\n', '\r'])
        && !(matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "ftp" | "ws" | "wss") && rest.trim_start_matches('/').is_empty())
}

/// The value fails `step` (HTML §4.10.5.3.8).
fn step_mismatch<E: Element>(el: &E, t: &str, v: f64) -> bool {
    let (default_step, scale) = match t {
        "number" => (1.0, 1.0),
        "date" | "month" | "week" => (1.0, 1.0),
        "time" | "datetime-local" => (60.0, 1000.0),
        _ => return false,
    };
    let step = match el.attribut("step") {
        Some(s) if s.trim().eq_ignore_ascii_case("any") => return false,
        Some(s) => parse_float_lenient(s).filter(|x| *x > 0.0).unwrap_or(default_step),
        None => default_step,
    } * scale;
    if t == "week" {
        return false;
    }
    let base = el
        .attribut("min")
        .and_then(|m| parse_typed(t, m, false))
        .or_else(|| el.attribut("value").and_then(|m| parse_typed(t, m, true)))
        .unwrap_or(0.0);
    let q = (v - base) / step;
    (q - q.round()).abs() > 1e-7 * q.abs().max(1.0)
}

/// A `<select required>` without a selected option, or with the
/// placeholder label option selected.
fn select_missing<E: Element>(sel: &E) -> bool {
    let multiple = sel.attribut("multiple").is_some();
    let single = !multiple && sel.attribut("size").is_none_or(|s| s.trim().parse::<u32>().map_or(true, |n| n <= 1));
    let (mut index, mut placeholder, mut selected, mut first_enabled) = (0usize, false, None, None);
    let mut visit = |o: &E, direct: bool| {
        if index == 0 && direct {
            placeholder = o.attribut("value").map_or_else(|| o.ist_leer(), str::is_empty);
        }
        if is_checked(o) {
            selected = Some(index);
        }
        if first_enabled.is_none() && !is_disabled(o) {
            first_enabled = Some(index);
        }
        index += 1;
    };
    let mut c = sel.erstes_kind();
    let mut n = 0;
    while let Some(x) = c {
        if is(&x, "option") {
            visit(&x, true);
        } else if is(&x, "optgroup") {
            let mut o = x.erstes_kind();
            while let Some(y) = o {
                if is(&y, "option") {
                    visit(&y, false);
                }
                n += 1;
                if n > MAX_TREE {
                    break;
                }
                o = y.naechstes_geschwister();
            }
        }
        n += 1;
        if n > MAX_TREE {
            break;
        }
        c = x.naechstes_geschwister();
    }
    let chosen = selected.or(if single { first_enabled } else { None });
    chosen.is_none() || (single && placeholder && chosen == Some(0))
}

/// The control suffers from a validity problem it can be checked for here
/// (missing value, type mismatch, pattern, range, step).
fn suffers<E: Element>(el: &E) -> bool {
    if is(el, "textarea") {
        let empty = el.form_value().map_or_else(|| el.ist_leer(), |v| v.is_empty());
        return el.attribut("required").is_some() && empty;
    }
    if is(el, "select") {
        return el.attribut("required").is_some() && select_missing(el);
    }
    if !is(el, "input") {
        return false;
    }
    let t = input_type(el);
    match t.as_str() {
        "checkbox" => return el.attribut("required").is_some() && !is_checked(el),
        "radio" => {
            return radio_group_any(el, &mut |x| x.attribut("required").is_some())
                && !radio_group_any(el, &mut |x| is_checked(x));
        }
        "submit" | "image" | "reset" | "button" | "hidden" | "range" | "color" => return false,
        _ => {}
    }
    let raw = control_value(el);
    let value: Cow<'_, str> = if matches!(t.as_str(), "text" | "search" | "tel" | "password") {
        Cow::Owned(raw.replace(['\n', '\r'], ""))
    } else {
        Cow::Owned(raw.trim_matches(|c: char| c.is_ascii_whitespace()).replace(['\n', '\r'], ""))
    };
    if value.is_empty() {
        return el.attribut("required").is_some();
    }
    let multiple = el.attribut("multiple").is_some();
    match t.as_str() {
        "email" => {
            let ok = if multiple {
                value.split(',').all(|p| valid_email(p.trim_matches(|c: char| c.is_ascii_whitespace())))
            } else {
                valid_email(&value)
            };
            if !ok {
                return true;
            }
        }
        "url" if !valid_absolute_url(&value) => return true,
        _ => {}
    }
    if matches!(t.as_str(), "text" | "search" | "tel" | "url" | "email" | "password")
        && let Some(p) = el.attribut("pattern")
    {
        let parts: Vec<&str> = if t == "email" && multiple { value.split(',').collect() } else { vec![&value] };
        for part in parts {
            if el.pattern_matches(p, part.trim()) == Some(false) {
                return true;
            }
        }
    }
    if matches!(t.as_str(), "number" | "date" | "month" | "week" | "time" | "datetime-local") {
        let Some(v) = parse_typed(&t, &value, true) else { return false };
        if range_state(el).is_some_and(|(u, o)| u || o) || step_mismatch(el, &t, v) {
            return true;
        }
    }
    false
}

/// `:valid` / `:invalid` (`want_valid`): candidates by their own validity,
/// `<form>`s by the controls they own, `<fieldset>`s by their descendants.
pub fn matches_validity<E: Element>(el: &E, want_valid: bool) -> bool {
    let form = is(el, "form");
    if form || is(el, "fieldset") {
        let id = el.id().map(str::to_string);
        let bad = any_descendant(el, &mut |x| {
            (!form || x.attribut("form").is_none_or(|f| Some(f) == id.as_deref())) && is_candidate(x) && suffers(x)
        });
        return bad != want_valid;
    }
    is_candidate(el) && suffers(el) != want_valid
}

/// `:placeholder-shown`: an `<input>`/`<textarea>` showing its placeholder
/// (a non-empty `placeholder` and an empty value).
pub fn is_placeholder_shown<E: Element>(el: &E) -> bool {
    if el.attribut("placeholder").is_none_or(str::is_empty) {
        return false;
    }
    if is(el, "input") {
        let t = input_type(el);
        if !matches!(t.as_str(), "text" | "search" | "url" | "tel" | "email" | "password" | "number") {
            return false;
        }
        return control_value(el).is_empty();
    }
    is(el, "textarea") && el.form_value().map_or_else(|| el.ist_leer(), |v| v.is_empty())
}

/// `:open`: `<details>`/`<dialog>` with `open`.
pub fn is_open<E: Element>(el: &E) -> bool {
    (is(el, "details") || is(el, "dialog")) && el.attribut("open").is_some()
}

/// `:link` / `:any-link`: `<a>`/`<area>` with `href` (nothing is visited).
pub fn is_link<E: Element>(el: &E) -> bool {
    (is(el, "a") || is(el, "area")) && el.attribut("href").is_some()
}

/// HTML "rules for parsing integers" (leading whitespace, sign, digits).
fn parse_integer(s: &str) -> Option<i64> {
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).take(12).collect();
    if digits.is_empty() {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// Release 2026-10-15: the heading level of an h1-h6 (HTML headingoffset,
/// whatwg/html#11086): its number plus the `headingoffset` values (each at
/// least 0) of its inclusive ancestors up to one with `headingreset` or a
/// modal dialog, at most 9.
pub fn heading_level<E: Element>(el: &E) -> Option<i32> {
    let t = el.tag().as_bytes();
    let base = match t {
        [b'h' | b'H', d @ b'1'..=b'6'] => i64::from(d - b'0'),
        _ => return None,
    };
    let off = |x: &E| x.attribut("headingoffset").and_then(parse_integer).map_or(0, |v| v.clamp(0, 9));
    let stops = |x: &E| x.attribut("headingreset").is_some() || x.attribut(" narsil-modal").is_some();
    let mut offset = off(el);
    if !stops(el) {
        let mut cur = el.eltern();
        let mut n = 0;
        while let Some(c) = cur {
            offset += off(&c);
            if stops(&c) || offset >= 9 {
                break;
            }
            n += 1;
            if n > MAX_DEPTH {
                break;
            }
            cur = c.eltern();
        }
    }
    Some((base + offset).min(9) as i32)
}
