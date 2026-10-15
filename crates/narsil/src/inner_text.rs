//! Release 2026-10-15: the `innerText` getter (HTML §3.2.7 "rendered text
//! collection steps") on the computed styles of the live DOM.
//!
//! A rendered element's text follows its boxes: `display: none` and
//! `visibility: hidden` drop text, block-level boxes add required line
//! breaks (`<p>` two), table cells end with a tab and rows with a newline,
//! `<br>` is a newline, `white-space` collapses spaces across element
//! boundaries (CSS Text §4.1.1) and `text-transform` applies. The contents
//! of replaced elements and form controls are not rendered. An element that
//! is not rendered gives its descendant text content.
//!
//! MEASURED before (WPT html/dom/elements/the-innertext-and-outertext-
//! properties, getter 152 and innertext-with-white-spaces 45 failures):
//! `innerText` was `textContent`.

use crate::dom::Stand;
use narsil_css::{Anzeige, Berechnet, Weissraum};
use narsil_fmt_html::dom::{Art, Namensraum};
use narsil_layout::plan::TabellenRolle;

const MAX_NODES: usize = 200_000;

/// What the collection hands to the whitespace pass.
enum Tok {
    /// Text of one text node (transformed) and its `white-space`.
    Text(String, Weissraum),
    /// `<br>`: a newline, a line end for collapsing.
    Br,
    /// A required line break count at a block boundary.
    Block(u8),
    /// A literal string after a table cell (`\t`) or row (`\n`).
    Sep(&'static str),
    /// The start/end of an atomic inline or table cell: its own formatting
    /// context for collapsing.
    CtxStart,
    CtxEnd,
}

enum Item {
    Text(String),
    Req(u8),
}

fn html_name(dok: &narsil_fmt_html::dom::Dokument, k: usize) -> Option<&str> {
    match dok.knoten.get(k).map(|n| &n.art) {
        Some(Art::Element { raum: Namensraum::Html, .. }) => dok.element_name(k),
        _ => None,
    }
}

fn transform(text: &str, how: &str) -> String {
    match how.trim() {
        "uppercase" => text.to_uppercase(),
        "lowercase" => text.to_lowercase(),
        "capitalize" => {
            let mut out = String::with_capacity(text.len());
            let mut word_start = true;
            for c in text.chars() {
                if c.is_alphanumeric() {
                    if word_start {
                        out.extend(c.to_uppercase());
                    } else {
                        out.push(c);
                    }
                    word_start = false;
                } else {
                    out.push(c);
                    word_start = c.is_whitespace();
                }
            }
            out
        }
        _ => text.to_string(),
    }
}

struct Walk<'a> {
    l: &'a crate::lebend::Lebend,
    satz: &'a narsil_page::StilSatz,
    cache: &'a mut std::collections::HashMap<usize, Berechnet>,
    out: Vec<Tok>,
    budget: usize,
}

impl Walk<'_> {
    fn style(&mut self, k: usize) -> Option<Berechnet> {
        crate::lebend::berechnet(self.l, k, self.satz, self.cache)
    }

    fn children(&mut self, k: usize, parent: Option<&Berechnet>) {
        let kids: Vec<usize> = self.l.dok.kinder(k).to_vec();
        for c in kids {
            self.node(c, parent);
            if self.budget == 0 {
                return;
            }
        }
    }

    fn node(&mut self, k: usize, parent: Option<&Berechnet>) {
        if self.budget == 0 {
            return;
        }
        self.budget -= 1;
        let dok = &self.l.dok;
        match dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Text(t)) => {
                // Whitespace directly in a table, row group or row has no
                // box (CSS 2.1 §17.2.1).
                if parent.is_some_and(|p| {
                    matches!(
                        p.tabellen_rolle,
                        TabellenRolle::Tabelle | TabellenRolle::InlineTabelle | TabellenRolle::Rumpf | TabellenRolle::Kopf | TabellenRolle::Fuss | TabellenRolle::Zeile
                    )
                }) && t.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}'))
                {
                    return;
                }
                if let Some(p) = parent
                    && p.sichtbar
                {
                    let t = transform(t, &p.text_transform);
                    self.out.push(Tok::Text(t, p.weissraum));
                }
            }
            Some(Art::Element { .. }) => self.element(k),
            _ => {}
        }
    }

    fn element(&mut self, k: usize) {
        let Some(b) = self.style(k) else { return };
        if b.anzeige == Anzeige::Keins {
            return;
        }
        let name = html_name(&self.l.dok, k).map(str::to_string);
        let svg = matches!(self.l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Svg, .. }));
        if svg {
            // SVG: only rendered text content elements, not inside <defs>.
            let tag = self.l.dok.element_name(k).unwrap_or("");
            if matches!(tag, "defs" | "symbol" | "clipPath" | "mask" | "pattern" | "marker" | "linearGradient" | "radialGradient" | "stop" | "title" | "desc" | "metadata" | "style" | "script") {
                return;
            }
            let text_el = matches!(tag, "text" | "tspan" | "textPath" | "a");
            if text_el {
                self.children(k, Some(&b));
            } else {
                let kids: Vec<usize> = self.l.dok.kinder(k).to_vec();
                for c in kids {
                    if self.l.dok.ist_element_knoten(c) {
                        self.node(c, Some(&b));
                    }
                }
            }
            return;
        }
        let name = name.as_deref();
        if b.nur_inhalt {
            self.children(k, Some(&b));
            return;
        }
        let visible = b.sichtbar;
        match name {
            Some("br") => {
                if visible {
                    self.out.push(Tok::Br);
                }
                return;
            }
            // Replaced elements and controls: their contents are not
            // rendered; they sit in the line as atomic inlines (or blocks).
            Some(
                "img" | "input" | "textarea" | "iframe" | "video" | "audio" | "canvas" | "object" | "embed" | "meter"
                | "progress" | "picture" | "frame" | "frameset" | "noscript" | "template",
            ) => {
                let block = Self::block_level(&b);
                if block && visible {
                    self.out.push(Tok::Block(1));
                } else {
                    self.out.push(Tok::CtxStart);
                    self.out.push(Tok::CtxEnd);
                }
                if block && visible {
                    self.out.push(Tok::Block(1));
                }
                return;
            }
            Some("select") => {
                // The options are listed one per line (an empty select is
                // an atomic inline).
                let kids: Vec<usize> = self.l.dok.kinder(k).to_vec();
                let opts: Vec<usize> =
                    kids.into_iter().filter(|&c| matches!(html_name(&self.l.dok, c), Some("option" | "optgroup" | "hr"))).collect();
                if opts.is_empty() {
                    self.out.push(Tok::CtxStart);
                    self.out.push(Tok::CtxEnd);
                    return;
                }
                self.out.push(Tok::Block(1));
                for c in opts {
                    self.option_like(c);
                }
                self.out.push(Tok::Block(1));
                return;
            }
            _ => {}
        }
        // A closed <details> renders only its first <summary>.
        if name == Some("details") && self.l.dok.attribut(k, "open").is_none() {
            let summary = self.l.dok.kinder(k).iter().copied().find(|&c| html_name(&self.l.dok, c) == Some("summary"));
            if visible && Self::block_level(&b) {
                self.out.push(Tok::Block(1));
            }
            if let Some(sm) = summary {
                self.node(sm, Some(&b));
            }
            if visible && Self::block_level(&b) {
                self.out.push(Tok::Block(1));
            }
            return;
        }
        let block = visible && (Self::block_level(&b) || matches!(name, Some("p" | "optgroup")));
        let gap = if name == Some("p") { 2 } else { 1 };
        let atomic = visible && Self::atomic_inline(&b);
        let cell = visible && b.tabellen_rolle == TabellenRolle::Zelle;
        let row = visible && b.tabellen_rolle == TabellenRolle::Zeile;
        if block {
            self.out.push(Tok::Block(gap));
        }
        if atomic || cell {
            self.out.push(Tok::CtxStart);
        }
        self.children(k, Some(&b));
        if atomic || cell {
            self.out.push(Tok::CtxEnd);
        }
        if cell && self.has_later_cell(k) {
            self.out.push(Tok::Sep("\t"));
        }
        if row && self.has_later_row(k) {
            self.out.push(Tok::Sep("\n"));
        }
        if block {
            self.out.push(Tok::Block(gap));
        }
    }

    /// An `<option>`/`<optgroup>` of a `<select>`: a line each.
    fn option_like(&mut self, k: usize) {
        let Some(b) = self.style(k) else { return };
        self.out.push(Tok::Block(1));
        if html_name(&self.l.dok, k) == Some("optgroup") {
            let kids: Vec<usize> = self.l.dok.kinder(k).to_vec();
            for c in kids {
                if html_name(&self.l.dok, c) == Some("option") {
                    self.option_like(c);
                }
            }
        } else {
            self.children(k, Some(&b));
        }
        self.out.push(Tok::Block(1));
    }

    fn block_level(b: &Berechnet) -> bool {
        if b.inline_aussen {
            return false;
        }
        matches!(
            b.anzeige,
            Anzeige::Block | Anzeige::ListenEintrag | Anzeige::Flex | Anzeige::Raster | Anzeige::Tabelle | Anzeige::TabelleRand
        )
    }

    fn atomic_inline(b: &Berechnet) -> bool {
        matches!(b.anzeige, Anzeige::InlineBlock | Anzeige::InlineFlex) || b.inline_aussen
    }

    /// The siblings after `k`.
    fn later_siblings(&self, k: usize) -> Vec<usize> {
        let Some(p) = self.l.dok.eltern(k) else { return Vec::new() };
        let kids = self.l.dok.kinder(p);
        match kids.iter().position(|&c| c == k) {
            Some(i) => kids[i + 1..].iter().copied().filter(|&c| self.l.dok.ist_element_knoten(c)).collect(),
            None => Vec::new(),
        }
    }

    fn has_later_cell(&mut self, k: usize) -> bool {
        for x in self.later_siblings(k) {
            if self.style(x).is_some_and(|b| b.tabellen_rolle == TabellenRolle::Zelle) {
                return true;
            }
        }
        false
    }

    /// Another table row follows in the same table (also in a later row
    /// group).
    fn has_later_row(&mut self, k: usize) -> bool {
        for x in self.later_siblings(k) {
            if self.style(x).is_some_and(|b| b.tabellen_rolle == TabellenRolle::Zeile) {
                return true;
            }
        }
        let Some(group) = self.l.dok.eltern(k) else { return false };
        if !self.style(group).is_some_and(|b| matches!(b.tabellen_rolle, TabellenRolle::Rumpf | TabellenRolle::Kopf | TabellenRolle::Fuss)) {
            return false;
        }
        for x in self.later_siblings(group) {
            let rows: Vec<usize> = self.l.dok.kinder(x).iter().copied().filter(|&r| self.l.dok.ist_element_knoten(r)).collect();
            for r in rows {
                if self.style(r).is_some_and(|b| b.tabellen_rolle == TabellenRolle::Zeile) {
                    return true;
                }
            }
        }
        false
    }
}

/// The whitespace pass (CSS Text §4.1.1, simplified to the line ends that
/// innerText can see) and the final assembly of the rendered text.
fn assemble(toks: Vec<Tok>) -> String {
    let mut items: Vec<Item> = Vec::new();
    let mut cur = String::new();
    let mut line_start = true;
    let mut pending = false;
    let flush = |cur: &mut String, items: &mut Vec<Item>| {
        if !cur.is_empty() {
            items.push(Item::Text(std::mem::take(cur)));
        }
    };
    for t in toks {
        match t {
            Tok::Text(s, ws) => match ws {
                Weissraum::Pre | Weissraum::PreUmbruch | Weissraum::UmbruchLeer => {
                    if s.is_empty() {
                        continue;
                    }
                    if pending && !line_start {
                        cur.push(' ');
                    }
                    pending = false;
                    cur.push_str(&s.replace("\r\n", "\n").replace('\r', "\n"));
                    line_start = s.ends_with('\n');
                }
                Weissraum::PreZeile => {
                    for c in s.chars() {
                        match c {
                            '\n' | '\r' => {
                                pending = false;
                                cur.push('\n');
                                line_start = true;
                            }
                            ' ' | '\t' | '\u{c}' => pending = true,
                            _ => {
                                if pending && !line_start {
                                    cur.push(' ');
                                }
                                pending = false;
                                cur.push(c);
                                line_start = false;
                            }
                        }
                    }
                }
                Weissraum::Normal | Weissraum::KeinUmbruch => {
                    for c in s.chars() {
                        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                            pending = true;
                        } else {
                            if pending && !line_start {
                                cur.push(' ');
                            }
                            pending = false;
                            cur.push(c);
                            line_start = false;
                        }
                    }
                }
            },
            Tok::Br => {
                pending = false;
                cur.push('\n');
                line_start = true;
            }
            Tok::Block(n) => {
                pending = false;
                line_start = true;
                flush(&mut cur, &mut items);
                items.push(Item::Req(n));
            }
            Tok::Sep(s) => {
                pending = false;
                cur.push_str(s);
                line_start = true;
            }
            Tok::CtxStart => {
                if pending && !line_start {
                    cur.push(' ');
                }
                pending = false;
                line_start = true;
            }
            Tok::CtxEnd => {
                pending = false;
                line_start = false;
            }
        }
    }
    flush(&mut cur, &mut items);
    // Remove leading/trailing required line breaks; a run of them becomes
    // as many newlines as its largest count.
    let mut out = String::new();
    let mut run: u8 = 0;
    let mut started = false;
    for it in items {
        match it {
            Item::Req(n) => run = run.max(n),
            Item::Text(t) => {
                if started && run > 0 {
                    for _ in 0..run {
                        out.push('\n');
                    }
                }
                run = 0;
                out.push_str(&t);
                started = true;
            }
        }
    }
    out
}

/// `innerText` of host `id`, or `None` when the live DOM cannot answer
/// (then the caller's text content stays). Not rendered elements give the
/// descendant text content.
pub(crate) fn inner_text(w: &mut Stand, id: u32) -> Option<String> {
    crate::dom::refresh_styles(w);
    let Stand { lebend, stil_satz, stil_cache, .. } = w;
    let (l, satz) = (lebend.as_ref()?, stil_satz.as_ref()?);
    let k = l.knoten(id)?;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    // Being rendered: connected, and neither it nor an ancestor is
    // `display: none` (`display: contents` has no box either).
    let mut rendered = l.ist_verbunden(k);
    if rendered {
        let mut x = Some(k);
        let mut n = 0;
        while let Some(e) = x {
            if l.dok.ist_element_knoten(e) {
                match crate::lebend::berechnet(l, e, satz, stil_cache) {
                    Some(b) if b.anzeige == Anzeige::Keins => {
                        rendered = false;
                        break;
                    }
                    Some(_) => {}
                    None => {
                        rendered = false;
                        break;
                    }
                }
            }
            n += 1;
            if n > 512 {
                break;
            }
            x = l.dok.eltern(e);
        }
    }
    if !rendered {
        return Some(l.dok.text_inhalt(k));
    }
    let b = crate::lebend::berechnet(l, k, satz, stil_cache)?;
    let mut walk = Walk { l, satz, cache: stil_cache, out: Vec::new(), budget: MAX_NODES };
    match html_name(&l.dok, k) {
        // The element's own contents count even for controls whose
        // contents are not rendered inside a parent's text.
        Some("select") => {
            let kids: Vec<usize> = l.dok.kinder(k).to_vec();
            for c in kids {
                if matches!(html_name(&l.dok, c), Some("option" | "optgroup")) {
                    walk.option_like(c);
                }
            }
        }
        Some("textarea" | "input" | "img" | "iframe" | "video" | "audio" | "canvas" | "object" | "embed" | "br") => {}
        _ => walk.children(k, Some(&b)),
    }
    Some(assemble(walk.out))
}
