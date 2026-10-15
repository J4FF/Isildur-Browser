//! A small, forgiving XML reader for SVG documents: elements, attributes,
//! text, CDATA, comments, processing instructions and the doctype (whose
//! internal subset may declare entities). Namespace prefixes are kept in
//! the names (`xlink:href`); the SVG namespace itself is implied.

use std::collections::HashMap;

/// Upper bounds against hostile input.
const MAX_NODES: usize = 200_000;
const MAX_DEPTH: usize = 256;

/// One element of the document tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    /// The qualified name as written (`svg`, `xlink:href` style prefixes
    /// stay; an `svg:` prefix is dropped).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// An attribute by name (`href` also finds `xlink:href`).
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        if let Some((_, v)) = self.attrs.iter().find(|(k, _)| k == name) {
            return Some(v.as_str());
        }
        if name == "href" {
            return self.attrs.iter().find(|(k, _)| k == "xlink:href").map(|(_, v)| v.as_str());
        }
        None
    }

    /// Child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The concatenated text content (for `<style>` and `<text>`).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }
}

struct Reader<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    entities: HashMap<String, String>,
    nodes: usize,
    /// The document ended inside a tag.
    broken: bool,
}

/// Parses a document and returns its root element (`None` when there is
/// none).
#[must_use]
pub fn parse(source: &str) -> Option<Element> {
    let mut r = Reader { s: source, b: source.as_bytes(), i: 0, entities: HashMap::new(), nodes: 0, broken: false };
    // Skip a byte order mark.
    if r.s.starts_with('\u{feff}') {
        r.i = 3;
    }
    loop {
        r.skip_ws();
        if r.i >= r.b.len() {
            return None;
        }
        if r.starts("<?") {
            r.skip_past("?>");
        } else if r.starts("<!--") {
            r.skip_past("-->");
        } else if r.starts("<!DOCTYPE") || r.starts("<!doctype") {
            r.doctype();
        } else if r.starts("<") {
            let root = r.element(0);
            return if r.broken { None } else { root };
        } else {
            r.i += 1;
        }
    }
}

impl Reader<'_> {
    fn starts(&self, p: &str) -> bool {
        self.b[self.i..].starts_with(p.as_bytes())
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn skip_past(&mut self, end: &str) {
        match self.s[self.i..].find(end) {
            Some(p) => self.i += p + end.len(),
            None => self.i = self.b.len(),
        }
    }

    /// `<!DOCTYPE ... [ <!ENTITY name "value"> ... ]>`.
    fn doctype(&mut self) {
        let start = self.i;
        let mut depth = 0i32;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                b'>' if depth <= 0 => {
                    self.i += 1;
                    break;
                }
                _ => {}
            }
            self.i += 1;
        }
        let text = &self.s[start..self.i];
        let mut rest = text;
        while let Some(p) = rest.find("<!ENTITY") {
            rest = &rest[p + 8..];
            let t = rest.trim_start();
            let name: String = t.chars().take_while(|c| !c.is_whitespace()).collect();
            let after = t[name.len()..].trim_start();
            let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
            let body = &after[1..];
            if let Some(end) = body.find(q) {
                if self.entities.len() < 256 {
                    self.entities.insert(name, body[..end].to_string());
                }
            }
        }
    }

    fn name(&mut self) -> String {
        let start = self.i;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_whitespace() || matches!(c, b'/' | b'>' | b'=') {
                break;
            }
            self.i += 1;
        }
        self.s[start..self.i].to_string()
    }

    fn element(&mut self, depth: usize) -> Option<Element> {
        // At '<'.
        self.i += 1;
        let raw = self.name();
        let mut el = Element { name: local_name(&raw), attrs: Vec::new(), children: Vec::new() };
        self.nodes += 1;
        // Attributes.
        loop {
            self.skip_ws();
            if self.i >= self.b.len() {
                // The document ends inside a start tag: not well-formed.
                self.broken = true;
                return Some(el);
            }
            if self.starts("/>") {
                self.i += 2;
                return Some(el);
            }
            if self.b[self.i] == b'>' {
                self.i += 1;
                // HTML void elements (an HTML serializer writes `<link ...>`
                // for foreign `h:link` inside inline SVG) have no content.
                if is_html_void(&el.name) {
                    return Some(el);
                }
                break;
            }
            let name = self.name();
            if name.is_empty() {
                self.i += 1;
                continue;
            }
            self.skip_ws();
            let mut value = String::new();
            if self.i < self.b.len() && self.b[self.i] == b'=' {
                self.i += 1;
                self.skip_ws();
                if self.i < self.b.len() && (self.b[self.i] == b'"' || self.b[self.i] == b'\'') {
                    let q = self.b[self.i];
                    self.i += 1;
                    let start = self.i;
                    while self.i < self.b.len() && self.b[self.i] != q {
                        self.i += 1;
                    }
                    value = self.decode(&self.s[start..self.i]);
                    self.i = (self.i + 1).min(self.b.len());
                } else {
                    let start = self.i;
                    while self.i < self.b.len() && !self.b[self.i].is_ascii_whitespace() && self.b[self.i] != b'>' {
                        self.i += 1;
                    }
                    value = self.decode(&self.s[start..self.i]);
                }
            }
            if el.attrs.len() < 256 {
                el.attrs.push((attr_name(&name), value));
            }
        }
        if depth >= MAX_DEPTH {
            return Some(el);
        }
        // Content.
        let mut text = String::new();
        while self.i < self.b.len() {
            if self.starts("</") {
                self.i += 2;
                let end = local_name(&self.name());
                self.skip_past(">");
                // A stray end tag of a void element does not close this one.
                if end != el.name && is_html_void(&end) {
                    continue;
                }
                break;
            }
            if self.starts("<!--") {
                self.skip_past("-->");
                continue;
            }
            if self.starts("<![CDATA[") {
                self.i += 9;
                let start = self.i;
                match self.s[self.i..].find("]]>") {
                    Some(p) => {
                        text.push_str(&self.s[start..start + p]);
                        self.i = start + p + 3;
                    }
                    None => {
                        text.push_str(&self.s[start..]);
                        self.i = self.b.len();
                    }
                }
                continue;
            }
            if self.starts("<?") {
                self.skip_past("?>");
                continue;
            }
            if self.b[self.i] == b'<' {
                if !text.is_empty() {
                    el.children.push(Node::Text(std::mem::take(&mut text)));
                }
                if self.nodes >= MAX_NODES {
                    self.i = self.b.len();
                    break;
                }
                if let Some(child) = self.element(depth + 1) {
                    el.children.push(Node::Element(child));
                }
                continue;
            }
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'<' {
                self.i += 1;
            }
            let decoded = self.decode(&self.s[start..self.i]);
            text.push_str(&decoded);
        }
        if !text.is_empty() {
            el.children.push(Node::Text(text));
        }
        Some(el)
    }

    /// Character and entity references.
    fn decode(&self, t: &str) -> String {
        if !t.contains('&') {
            return t.to_string();
        }
        let mut out = String::with_capacity(t.len());
        let mut rest = t;
        while let Some(p) = rest.find('&') {
            out.push_str(&rest[..p]);
            rest = &rest[p..];
            let Some(end) = rest.find(';').filter(|e| *e < 32) else {
                out.push('&');
                rest = &rest[1..];
                continue;
            };
            let name = &rest[1..end];
            let rep = match name {
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "amp" => Some("&".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" => Some("'".to_string()),
                _ if name.starts_with("#x") || name.starts_with("#X") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32).map(String::from),
                _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32).map(String::from),
                _ => self.entities.get(name).cloned(),
            };
            match rep {
                Some(r) => {
                    out.push_str(&r);
                    rest = &rest[end + 1..];
                }
                None => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// HTML elements that never have content.
fn is_html_void(name: &str) -> bool {
    matches!(name, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr")
}

/// Element names: an `svg:` prefix is the default namespace.
fn local_name(raw: &str) -> String {
    raw.strip_prefix("svg:").unwrap_or(raw).to_string()
}

/// Attribute names keep `xlink:`/`xml:` prefixes; other prefixes for the
/// SVG namespace are dropped.
fn attr_name(raw: &str) -> String {
    raw.strip_prefix("svg:").unwrap_or(raw).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_text_and_entities() {
        let doc = r##"<?xml version="1.0"?>
<!DOCTYPE svg [ <!ENTITY red "#f00"> ]>
<svg xmlns="http://www.w3.org/2000/svg" width="10"><!-- c -->
  <rect fill="&red;" width='5'/>
  <style><![CDATA[ rect { stroke: blue } ]]></style>
  <text>a &amp; b &#x41;</text>
</svg>"##;
        let root = parse(doc).expect("root");
        assert_eq!(root.name, "svg");
        assert_eq!(root.attr("width"), Some("10"));
        let kids: Vec<&Element> = root.elements().collect();
        assert_eq!(kids[0].attr("fill"), Some("#f00"));
        assert_eq!(kids[1].text().trim(), "rect { stroke: blue }");
        assert_eq!(kids[2].text(), "a & b A");
    }
}
