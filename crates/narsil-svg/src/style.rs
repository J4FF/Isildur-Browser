//! The document tree with computed styles: presentation attributes, style
//! sheets, `style` attributes and `!important`, cascaded and inherited for
//! the SVG properties.

use crate::css::{self, Declaration, Matchable, Sheet};
use crate::xml::{Element, Node};
use std::collections::HashMap;

/// The properties the renderer reads, with whether they inherit and the
/// initial value.
pub const PROPERTIES: &[(&str, bool, &str)] = &[
    ("fill", true, "black"),
    ("fill-opacity", true, "1"),
    ("fill-rule", true, "nonzero"),
    ("stroke", true, "none"),
    ("stroke-width", true, "1"),
    ("stroke-opacity", true, "1"),
    ("stroke-linecap", true, "butt"),
    ("stroke-linejoin", true, "miter"),
    ("stroke-miterlimit", true, "4"),
    ("stroke-dasharray", true, "none"),
    ("stroke-dashoffset", true, "0"),
    ("paint-order", true, "normal"),
    ("vector-effect", false, "none"),
    ("color", true, "black"),
    ("opacity", false, "1"),
    ("display", false, "inline"),
    ("visibility", true, "visible"),
    ("overflow", false, "visible"),
    ("transform", false, "none"),
    ("transform-origin", false, "0 0"),
    ("transform-box", false, "view-box"),
    ("clip-path", false, "none"),
    ("clip-rule", true, "nonzero"),
    ("mask", false, "none"),
    ("mask-type", false, "luminance"),
    ("filter", false, "none"),
    ("mix-blend-mode", false, "normal"),
    ("isolation", false, "auto"),
    ("marker", true, "none"),
    ("marker-start", true, "none"),
    ("marker-mid", true, "none"),
    ("marker-end", true, "none"),
    ("stop-color", false, "black"),
    ("stop-opacity", false, "1"),
    ("flood-color", false, "black"),
    ("flood-opacity", false, "1"),
    ("lighting-color", false, "white"),
    ("color-interpolation-filters", true, "linearrgb"),
    ("color-interpolation", true, "srgb"),
    ("font-family", true, "sans-serif"),
    ("font-size", true, "16px"),
    ("font-weight", true, "normal"),
    ("font-style", true, "normal"),
    ("font", true, ""),
    ("text-anchor", true, "start"),
    ("dominant-baseline", true, "auto"),
    ("alignment-baseline", false, "auto"),
    ("letter-spacing", true, "normal"),
    ("word-spacing", true, "normal"),
    ("text-decoration", false, "none"),
    ("direction", true, "ltr"),
    ("unicode-bidi", false, "normal"),
    ("writing-mode", true, "horizontal-tb"),
    ("image-rendering", true, "auto"),
    ("shape-rendering", true, "auto"),
    // SVG 2 geometry properties.
    ("x", false, ""),
    ("y", false, ""),
    ("width", false, ""),
    ("height", false, ""),
    ("cx", false, ""),
    ("cy", false, ""),
    ("r", false, ""),
    ("rx", false, ""),
    ("ry", false, ""),
    ("d", false, ""),
];

fn property_index(name: &str) -> Option<usize> {
    static INDEX: std::sync::OnceLock<HashMap<&'static str, usize>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| PROPERTIES.iter().enumerate().map(|(k, (n, _, _))| (*n, k)).collect()).get(name).copied()
}

/// Computed values by property index (`None` = the initial value).
#[derive(Clone, Debug, Default)]
pub struct Computed {
    values: Vec<Option<String>>,
}

impl Computed {
    /// The computed value text of a property (initial value if unset).
    #[must_use]
    pub fn get(&self, name: &str) -> &str {
        let Some(i) = property_index(name) else { return "" };
        match self.values.get(i).and_then(|v| v.as_deref()) {
            Some(v) => v,
            None => PROPERTIES[i].2,
        }
    }

    /// Whether the property was set on the element or inherited.
    #[must_use]
    pub fn is_set(&self, name: &str) -> bool {
        property_index(name).is_some_and(|i| self.values.get(i).is_some_and(Option::is_some))
    }
}

/// One element of the indexed tree.
pub struct Item<'a> {
    pub el: &'a Element,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub style: Computed,
    /// The element's own declarations in cascade order (presentation
    /// attributes, matching rules, `style`), kept for restyling.
    decls: Vec<Declaration>,
}

/// The indexed document with computed styles.
pub struct Doc<'a> {
    pub items: Vec<Item<'a>>,
    pub ids: HashMap<String, usize>,
    /// Index of each element by address (for walking the xml tree).
    by_ptr: HashMap<usize, usize>,
}

impl<'a> Doc<'a> {
    /// Builds the index, reads every `<style>` and cascades.
    #[must_use]
    pub fn new(root: &'a Element, extra_css: &str) -> Doc<'a> {
        let mut doc = Doc { items: Vec::new(), ids: HashMap::new(), by_ptr: HashMap::new() };
        doc.add(root, None);
        let mut sheet = css::parse_sheet(extra_css, 0);
        for i in 0..doc.items.len() {
            if doc.items[i].el.name == "style" {
                let ty = doc.items[i].el.attr("type").unwrap_or("text/css");
                if ty.is_empty() || ty.eq_ignore_ascii_case("text/css") {
                    let s = css::parse_sheet(&doc.items[i].el.text(), sheet.rules.len());
                    sheet.rules.extend(s.rules);
                }
            }
        }
        doc.cascade(&sheet);
        doc
    }

    fn add(&mut self, el: &'a Element, parent: Option<usize>) -> usize {
        let idx = self.items.len();
        self.items.push(Item { el, parent, children: Vec::new(), style: Computed::default(), decls: Vec::new() });
        self.by_ptr.insert(el as *const Element as usize, idx);
        if let Some(id) = el.attr("id") {
            self.ids.entry(id.to_string()).or_insert(idx);
        }
        if self.items.len() > 200_000 {
            return idx;
        }
        for c in &el.children {
            if let Node::Element(e) = c {
                let ci = self.add(e, Some(idx));
                self.items[idx].children.push(ci);
            }
        }
        idx
    }

    /// The index of an element of this document.
    #[must_use]
    pub fn index_of(&self, el: &Element) -> Option<usize> {
        self.by_ptr.get(&(el as *const Element as usize)).copied()
    }

    /// The element an `#id` reference (or `url(#id)`) points to.
    #[must_use]
    pub fn lookup(&self, reference: &str) -> Option<usize> {
        let r = reference.trim();
        let r = r.strip_prefix("url(").map(|x| x.trim_end_matches(')').trim().trim_matches(|c| c == '"' || c == '\'')).unwrap_or(r);
        let id = r.rsplit_once('#').map_or(r, |(_, id)| id);
        if id.is_empty() {
            return None;
        }
        self.ids.get(id).copied()
    }

    /// The computed style of element `i` under a parent style.
    fn compute(&self, i: usize, parent: Option<&Computed>) -> Computed {
        let mut values: Vec<Option<String>> = vec![None; PROPERTIES.len()];
        if let Some(p) = parent {
            for (k, (_, inherits, _)) in PROPERTIES.iter().enumerate() {
                if *inherits {
                    values[k] = p.values.get(k).cloned().flatten();
                }
            }
        }
        for d in &self.items[i].decls {
            apply(&mut values, &d.name, &d.value, parent);
        }
        Computed { values }
    }

    /// The styles of the subtree at `root` when it inherits from `parent`
    /// (a `use` element's instance tree): selectors still match the
    /// originals, only inheritance follows the instance.
    #[must_use]
    pub fn restyle(&self, root: usize, parent: &Computed) -> HashMap<usize, Computed> {
        let mut out = HashMap::new();
        let mut stack = vec![(root, parent.clone())];
        while let Some((i, p)) = stack.pop() {
            if out.len() > 50_000 {
                break;
            }
            let st = self.compute(i, Some(&p));
            for &c in &self.items[i].children {
                stack.push((c, st.clone()));
            }
            out.insert(i, st);
        }
        out
    }

    fn cascade(&mut self, sheet: &Sheet) {
        for i in 0..self.items.len() {
            let el = self.items[i].el;
            // Collected declarations: (important, origin rank, specificity, order).
            let mut decls: Vec<((bool, u8, (u32, u32, u32), usize), Declaration)> = Vec::new();
            for (name, value) in &el.attrs {
                if property_index(name).is_some() && !is_geometry_attr_only(&el.name, name) && !matches!(name.as_str(), "marker" | "font") {
                    decls.push(((false, 0, (0, 0, 0), 0), Declaration { name: name.clone(), value: value.clone(), important: false }));
                }
            }
            let node = Node2 { doc: self, i };
            for rule in &sheet.rules {
                if css::matches(&rule.selector, &node) {
                    for d in &rule.declarations {
                        decls.push(((d.important, 1, rule.selector.specificity, rule.order), d.clone()));
                    }
                }
            }
            if let Some(st) = el.attr("style") {
                for (n, d) in css::parse_declarations(st).into_iter().enumerate() {
                    decls.push(((d.important, 2, (0, 0, 0), n), d));
                }
            }
            decls.sort_by(|a, b| a.0.cmp(&b.0));
            let mut list: Vec<Declaration> = decls.into_iter().map(|(_, d)| d).collect();
            // SMIL `<set>` that applies from the start (no `begin`, or 0)
            // overrides the other values (SMIL Animation §3.3.6: the
            // animation sandwich is above the override style sheet).
            for c in &el.children {
                let Node::Element(a) = c else { continue };
                if a.name != "set" {
                    continue;
                }
                let begin = a.attr("begin").map(str::trim).unwrap_or("0");
                let from_start = matches!(begin, "0" | "0s" | "0ms" | "");
                let lasts = a.attr("dur").is_none_or(|d| d.trim() == "indefinite") || a.attr("fill").map(str::trim) == Some("freeze");
                if let (true, true, Some(name), Some(to)) = (from_start, lasts, a.attr("attributeName"), a.attr("to")) {
                    if property_index(name).is_some() {
                        list.push(Declaration { name: name.to_string(), value: to.to_string(), important: false });
                    }
                }
            }
            self.items[i].decls = list;
            let parent = self.items[i].parent.map(|p| self.items[p].style.clone());
            self.items[i].style = self.compute(i, parent.as_ref());
        }
    }
}

/// Geometry presentation attributes only count on the elements they
/// belong to (`x` on a `<rect>`, not on a `<g>`); the renderer reads them
/// directly, so they are left out of the cascade except via CSS.
fn is_geometry_attr_only(tag: &str, name: &str) -> bool {
    let _ = tag;
    matches!(name, "x" | "y" | "width" | "height" | "cx" | "cy" | "r" | "rx" | "ry" | "d")
}

fn apply(values: &mut [Option<String>], name: &str, value: &str, parent: Option<&Computed>) {
    let v = value.trim();
    if name == "font" {
        apply_font_shorthand(values, v);
        return;
    }
    if name == "marker" {
        for n in ["marker-start", "marker-mid", "marker-end"] {
            apply(values, n, v, parent);
        }
        return;
    }
    let Some(k) = property_index(name) else { return };
    let lower = v.to_ascii_lowercase();
    let inherited = || parent.and_then(|p| p.values.get(k).cloned().flatten());
    values[k] = match lower.as_str() {
        "inherit" => inherited(),
        "initial" => None,
        "unset" if PROPERTIES[k].1 => inherited(),
        "unset" => None,
        _ => Some(v.to_string()),
    };
}

fn apply_font_shorthand(values: &mut [Option<String>], v: &str) {
    // [style] [weight] size[/line-height] family
    let mut style = "normal".to_string();
    let mut weight = "normal".to_string();
    let words: Vec<&str> = v.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        let w = words[i].to_ascii_lowercase();
        if matches!(w.as_str(), "italic" | "oblique") {
            style = w;
        } else if matches!(w.as_str(), "bold" | "bolder" | "lighter") || w.parse::<u32>().is_ok() {
            weight = w;
        } else if w != "normal" && !matches!(w.as_str(), "small-caps") {
            break;
        }
        i += 1;
    }
    if i >= words.len() {
        return;
    }
    let size = words[i].split('/').next().unwrap_or("16px").to_string();
    let family = words[i + 1..].join(" ");
    let set = |values: &mut [Option<String>], n: &str, val: String| {
        if let Some(k) = property_index(n) {
            values[k] = Some(val);
        }
    };
    set(values, "font-style", style);
    set(values, "font-weight", weight);
    set(values, "font-size", size);
    if !family.is_empty() {
        set(values, "font-family", family);
    }
}

#[derive(Clone)]
struct Node2<'d, 'a> {
    doc: &'d Doc<'a>,
    i: usize,
}

impl Matchable for Node2<'_, '_> {
    fn tag(&self) -> &str {
        &self.doc.items[self.i].el.name
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.doc.items[self.i].el.attr(name)
    }
    fn parent(&self) -> Option<Self> {
        self.doc.items[self.i].parent.map(|p| Node2 { doc: self.doc, i: p })
    }
    fn previous_sibling(&self) -> Option<Self> {
        let p = self.doc.items[self.i].parent?;
        let sib = &self.doc.items[p].children;
        let pos = sib.iter().position(|c| *c == self.i)?;
        (pos > 0).then(|| Node2 { doc: self.doc, i: sib[pos - 1] })
    }
    fn next_sibling(&self) -> Option<Self> {
        let p = self.doc.items[self.i].parent?;
        let sib = &self.doc.items[p].children;
        let pos = sib.iter().position(|c| *c == self.i)?;
        sib.get(pos + 1).map(|n| Node2 { doc: self.doc, i: *n })
    }
    fn index(&self) -> usize {
        let Some(p) = self.doc.items[self.i].parent else { return 1 };
        self.doc.items[p].children.iter().position(|c| *c == self.i).map_or(1, |x| x + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_orders_attributes_sheets_and_style() {
        let root = crate::xml::parse(r#"<svg fill="red"><style>.a { fill: green } rect { stroke: blue !important }</style><g><rect class="a" fill="yellow" style="stroke: black"/></g></svg>"#).unwrap();
        let doc = Doc::new(&root, "");
        let rect = doc.items.iter().find(|i| i.el.name == "rect").unwrap();
        assert_eq!(rect.style.get("fill"), "green");
        assert_eq!(rect.style.get("stroke"), "blue");
        let g = doc.items.iter().find(|i| i.el.name == "g").unwrap();
        assert_eq!(g.style.get("fill"), "red");
        assert_eq!(g.style.get("opacity"), "1");
    }
}
