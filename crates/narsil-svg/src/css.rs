//! Style sheets inside SVG documents: rules with selectors (type, class,
//! id, attribute, universal, the structural pseudo-classes that matter,
//! and the four combinators) and declarations, cascaded by specificity.

/// A parsed style sheet.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
    /// Position in the document (later wins at equal specificity).
    pub order: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    pub important: bool,
}

/// A complex selector: compounds joined by combinators, right to left
/// (`parts[0]` is the subject).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selector {
    pub parts: Vec<(Compound, Combinator)>,
    pub specificity: (u32, u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    /// The subject (no combinator to its right).
    None,
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, AttrOp, String)>,
    pub pseudo: Vec<Pseudo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Exists,
    Equals,
    Includes,
    Prefix,
    Suffix,
    Substring,
    Dash,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    Root,
    NthChild(i32, i32),
    Not(Box<Compound>),
    /// Unsupported pseudo-classes never match.
    Never,
}

/// What a selector needs from an element.
pub trait Matchable: Sized {
    fn tag(&self) -> &str;
    fn attr(&self, name: &str) -> Option<&str>;
    fn parent(&self) -> Option<Self>;
    fn previous_sibling(&self) -> Option<Self>;
    fn next_sibling(&self) -> Option<Self>;
    /// 1-based index among element siblings.
    fn index(&self) -> usize;
}

/// Removes `/* comments */`.
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        rest = match rest[p + 2..].find("*/") {
            Some(e) => &rest[p + 2 + e + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Parses `name: value; ...` (a style attribute or a rule body).
#[must_use]
pub fn parse_declarations(body: &str) -> Vec<Declaration> {
    let body = strip_comments(body);
    let mut out = Vec::new();
    for part in split_top(&body, ';') {
        let Some(colon) = part.find(':') else { continue };
        let name = part[..colon].trim().to_ascii_lowercase();
        let mut value = part[colon + 1..].trim().to_string();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let mut important = false;
        let lower = value.to_ascii_lowercase();
        if let Some(p) = lower.rfind("!important") {
            if lower[p + 10..].trim().is_empty() {
                value = value[..p].trim().to_string();
                important = true;
            }
        }
        out.push(Declaration { name, value, important });
    }
    out
}

/// Splits at `sep` outside parentheses and quotes.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0usize);
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => depth -= 1,
                _ if c == sep && depth <= 0 => {
                    out.push(&s[start..i]);
                    start = i + c.len_utf8();
                }
                _ => {}
            },
        }
    }
    out.push(&s[start..]);
    out
}

/// Parses a style sheet; `@media` blocks are read as if they matched,
/// other at-rules are skipped.
#[must_use]
pub fn parse_sheet(text: &str, order_base: usize) -> Sheet {
    let text = strip_comments(text);
    let mut sheet = Sheet::default();
    parse_block(&text, &mut sheet, order_base);
    sheet
}

fn parse_block(text: &str, sheet: &mut Sheet, order_base: usize) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Prelude up to `{` or `;` (for statement at-rules).
        let start = i;
        while i < b.len() && b[i] != b'{' && b[i] != b';' {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let prelude = text[start..i].trim();
        if b[i] == b';' {
            i += 1;
            continue;
        }
        // Body with nesting.
        let body_start = i + 1;
        let mut depth = 0;
        while i < b.len() {
            match b[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let body = &text[body_start..i.min(text.len())];
        i += 1;
        if let Some(at) = prelude.strip_prefix('@') {
            if at.to_ascii_lowercase().starts_with("media") || at.to_ascii_lowercase().starts_with("supports") {
                parse_block(body, sheet, order_base);
            }
            continue;
        }
        let declarations = parse_declarations(body);
        if declarations.is_empty() {
            continue;
        }
        for sel in split_top(prelude, ',') {
            if let Some(selector) = parse_selector(sel.trim()) {
                if sheet.rules.len() < 4096 {
                    let order = order_base + sheet.rules.len();
                    sheet.rules.push(Rule { selector, declarations: declarations.clone(), order });
                }
            }
        }
    }
}

/// Parses one complex selector.
#[must_use]
pub fn parse_selector(s: &str) -> Option<Selector> {
    if s.is_empty() {
        return None;
    }
    let mut compounds: Vec<(Compound, Combinator)> = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut pending = Combinator::None;
    let mut spec = (0u32, 0u32, 0u32);
    while i < chars.len() {
        // Combinator.
        let mut saw_space = false;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
            saw_space = true;
        }
        if i >= chars.len() {
            break;
        }
        match chars[i] {
            '>' => {
                pending = Combinator::Child;
                i += 1;
                continue;
            }
            '+' => {
                pending = Combinator::Adjacent;
                i += 1;
                continue;
            }
            '~' => {
                pending = Combinator::Sibling;
                i += 1;
                continue;
            }
            _ => {}
        }
        if !compounds.is_empty() && pending == Combinator::None && saw_space {
            pending = Combinator::Descendant;
        }
        let (compound, used, cspec) = parse_compound(&chars[i..])?;
        if used == 0 {
            return None;
        }
        i += used;
        spec = (spec.0 + cspec.0, spec.1 + cspec.1, spec.2 + cspec.2);
        if let Some(last) = compounds.last_mut() {
            last.1 = pending;
        }
        compounds.push((compound, Combinator::None));
        pending = Combinator::None;
    }
    if compounds.is_empty() {
        return None;
    }
    // Store right to left: the subject first, each with the combinator
    // that links it to the next (leftward) compound.
    let n = compounds.len();
    let mut parts = Vec::with_capacity(n);
    for k in (0..n).rev() {
        let comb = if k == 0 { Combinator::None } else { compounds[k - 1].1 };
        parts.push((compounds[k].0.clone(), comb));
    }
    Some(Selector { parts, specificity: spec })
}

fn ident(chars: &[char], mut i: usize) -> (String, usize) {
    let start = i;
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_' || chars[i] == '\\' || (chars[i] as u32) > 127) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

fn parse_compound(chars: &[char]) -> Option<(Compound, usize, (u32, u32, u32))> {
    let mut c = Compound::default();
    let mut i = 0;
    let mut spec = (0, 0, 0);
    if i < chars.len() && chars[i] == '*' {
        i += 1;
    } else if i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
        let (t, n) = ident(chars, i);
        c.tag = Some(t.to_ascii_lowercase());
        i = n;
        spec.2 += 1;
    }
    while i < chars.len() {
        match chars[i] {
            '#' => {
                let (t, n) = ident(chars, i + 1);
                c.id = Some(t);
                i = n;
                spec.0 += 1;
            }
            '.' => {
                let (t, n) = ident(chars, i + 1);
                c.classes.push(t);
                i = n;
                spec.1 += 1;
            }
            '[' => {
                let end = chars[i..].iter().position(|x| *x == ']')? + i;
                let inner: String = chars[i + 1..end].iter().collect();
                i = end + 1;
                spec.1 += 1;
                let ops = [("~=", AttrOp::Includes), ("^=", AttrOp::Prefix), ("$=", AttrOp::Suffix), ("*=", AttrOp::Substring), ("|=", AttrOp::Dash), ("=", AttrOp::Equals)];
                let mut done = false;
                for (tok, op) in ops {
                    if let Some(p) = inner.find(tok) {
                        let name = inner[..p].trim().to_ascii_lowercase();
                        let val = inner[p + tok.len()..].trim().trim_matches(|q| q == '"' || q == '\'').to_string();
                        c.attrs.push((name, op, val));
                        done = true;
                        break;
                    }
                }
                if !done {
                    c.attrs.push((inner.trim().to_ascii_lowercase(), AttrOp::Exists, String::new()));
                }
            }
            ':' => {
                let mut j = i + 1;
                if j < chars.len() && chars[j] == ':' {
                    // Pseudo-elements never match an element.
                    j += 1;
                    let (_, n) = ident(chars, j);
                    i = n;
                    c.pseudo.push(Pseudo::Never);
                    continue;
                }
                let (name, n) = ident(chars, j);
                j = n;
                let lower = name.to_ascii_lowercase();
                let mut arg = String::new();
                if j < chars.len() && chars[j] == '(' {
                    let mut depth = 0;
                    let start = j + 1;
                    while j < chars.len() {
                        if chars[j] == '(' {
                            depth += 1;
                        } else if chars[j] == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                    arg = chars[start..j.min(chars.len())].iter().collect();
                    j += 1;
                }
                i = j;
                spec.1 += 1;
                c.pseudo.push(match lower.as_str() {
                    "first-child" => Pseudo::FirstChild,
                    "last-child" => Pseudo::LastChild,
                    "only-child" => Pseudo::OnlyChild,
                    "root" => Pseudo::Root,
                    "nth-child" => match nth(&arg) {
                        Some((a, b)) => Pseudo::NthChild(a, b),
                        None => Pseudo::Never,
                    },
                    "not" => {
                        let inner: Vec<char> = arg.trim().chars().collect();
                        match parse_compound(&inner) {
                            Some((cc, used, _)) if used == inner.len() => Pseudo::Not(Box::new(cc)),
                            _ => Pseudo::Never,
                        }
                    }
                    _ => Pseudo::Never,
                });
            }
            _ => break,
        }
    }
    Some((c, i, spec))
}

fn nth(arg: &str) -> Option<(i32, i32)> {
    let a = arg.trim().to_ascii_lowercase().replace(' ', "");
    match a.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(p) = a.find('n') {
        let coef = match &a[..p] {
            "" | "+" => 1,
            "-" => -1,
            x => x.parse().ok()?,
        };
        let rest = &a[p + 1..];
        let off = if rest.is_empty() { 0 } else { rest.parse().ok()? };
        Some((coef, off))
    } else {
        Some((0, a.parse().ok()?))
    }
}

fn compound_matches<M: Matchable>(c: &Compound, e: &M) -> bool {
    if let Some(t) = &c.tag {
        if !e.tag().eq_ignore_ascii_case(t) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if e.attr("id") != Some(id.as_str()) {
            return false;
        }
    }
    for cl in &c.classes {
        if !e.attr("class").is_some_and(|v| v.split_whitespace().any(|x| x == cl)) {
            return false;
        }
    }
    for (name, op, val) in &c.attrs {
        let Some(v) = e.attr(name) else { return false };
        let ok = match op {
            AttrOp::Exists => true,
            AttrOp::Equals => v == val,
            AttrOp::Includes => v.split_whitespace().any(|x| x == val),
            AttrOp::Prefix => !val.is_empty() && v.starts_with(val.as_str()),
            AttrOp::Suffix => !val.is_empty() && v.ends_with(val.as_str()),
            AttrOp::Substring => !val.is_empty() && v.contains(val.as_str()),
            AttrOp::Dash => v == val || v.starts_with(&format!("{val}-")),
        };
        if !ok {
            return false;
        }
    }
    for p in &c.pseudo {
        let ok = match p {
            Pseudo::FirstChild => e.previous_sibling().is_none(),
            Pseudo::LastChild => e.next_sibling().is_none(),
            Pseudo::OnlyChild => e.previous_sibling().is_none() && e.next_sibling().is_none(),
            Pseudo::Root => e.parent().is_none(),
            Pseudo::NthChild(a, b) => {
                let n = e.index() as i32;
                if *a == 0 { n == *b } else { (n - b) % a == 0 && (n - b) / a >= 0 }
            }
            Pseudo::Not(inner) => !compound_matches(inner, e),
            Pseudo::Never => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Whether `sel` matches `e`.
#[must_use]
pub fn matches<M: Matchable + Clone>(sel: &Selector, e: &M) -> bool {
    match_from(sel, 0, e)
}

fn match_from<M: Matchable + Clone>(sel: &Selector, k: usize, e: &M) -> bool {
    let Some((compound, comb)) = sel.parts.get(k) else { return true };
    if !compound_matches(compound, e) {
        return false;
    }
    if k + 1 == sel.parts.len() {
        return true;
    }
    match comb {
        Combinator::None => true,
        Combinator::Child => e.parent().is_some_and(|p| match_from(sel, k + 1, &p)),
        Combinator::Descendant => {
            let mut cur = e.parent();
            while let Some(p) = cur {
                if match_from(sel, k + 1, &p) {
                    return true;
                }
                cur = p.parent();
            }
            false
        }
        Combinator::Adjacent => e.previous_sibling().is_some_and(|p| match_from(sel, k + 1, &p)),
        Combinator::Sibling => {
            let mut cur = e.previous_sibling();
            while let Some(p) = cur {
                if match_from(sel, k + 1, &p) {
                    return true;
                }
                cur = p.previous_sibling();
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct E<'a> {
        tree: &'a [(Option<usize>, &'a str, &'a str)],
        i: usize,
    }

    impl Matchable for E<'_> {
        fn tag(&self) -> &str {
            self.tree[self.i].1
        }
        fn attr(&self, name: &str) -> Option<&str> {
            (name == "class").then_some(self.tree[self.i].2)
        }
        fn parent(&self) -> Option<Self> {
            self.tree[self.i].0.map(|p| E { tree: self.tree, i: p })
        }
        fn previous_sibling(&self) -> Option<Self> {
            let p = self.tree[self.i].0;
            (0..self.i).rev().find(|j| self.tree[*j].0 == p).map(|j| E { tree: self.tree, i: j })
        }
        fn next_sibling(&self) -> Option<Self> {
            let p = self.tree[self.i].0;
            (self.i + 1..self.tree.len()).find(|j| self.tree[*j].0 == p).map(|j| E { tree: self.tree, i: j })
        }
        fn index(&self) -> usize {
            let p = self.tree[self.i].0;
            (0..=self.i).filter(|j| self.tree[*j].0 == p).count()
        }
    }

    #[test]
    fn selectors_match_with_combinators() {
        let tree = [(None, "svg", ""), (Some(0), "g", "a"), (Some(1), "rect", "x y"), (Some(1), "circle", "")];
        let rect = E { tree: &tree, i: 2 };
        let circle = E { tree: &tree, i: 3 };
        assert!(matches(&parse_selector("svg rect.x").unwrap(), &rect));
        assert!(matches(&parse_selector("g.a > rect").unwrap(), &rect));
        assert!(!matches(&parse_selector("svg > rect").unwrap(), &rect));
        assert!(matches(&parse_selector("rect + circle").unwrap(), &circle));
        assert!(matches(&parse_selector("circle:last-child").unwrap(), &circle));
        assert!(matches(&parse_selector("g :not(rect)").unwrap(), &circle));
        assert_eq!(parse_selector("#a .b c").unwrap().specificity, (1, 1, 1));
    }

    #[test]
    fn sheets_and_declarations() {
        let s = parse_sheet("/* x */ rect, .a { fill: red !important; stroke : blue } @media screen { g { opacity: .5 } }", 0);
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[0].declarations[0], Declaration { name: "fill".into(), value: "red".into(), important: true });
        assert_eq!(s.rules[2].declarations[0].value, ".5");
    }
}
