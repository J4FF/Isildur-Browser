//! Release 12.10.2026 (Agent C): implizites `@scope` (ohne Kopf, CSS Cascade 6
//! §2.5.1) — die Wurzel ist das Elternelement des `<style>`. Die Kaskade
//! kennt den Besitzer eines Blattes nicht; deshalb bekommt das Elternelement im
//! Seitenbaum ein internes Attribut mit einer Kennung des Blatttextes, und der
//! Blatttext nennt sie als Kopf:
//! `@scope { … }` -> `@scope ([data-narsil-bereich~="1a2b"]) { … }`.
//! Im lebenden DOM steht beides nicht (Skripte sehen den Text, wie er
//! geschrieben war); dort beantwortet `lebend::ArenaEl::attribut` das Attribut
//! aus den `<style>`-Kindern (`kennungen_der_kinder`).
//!
//! Nur `@scope` auf oberster Ebene oder in Gruppenregeln (`@media` …) wird so
//! umgeschrieben; in einer Stilregel ist die Wurzel deren Element (das macht
//! `narsil_css::scope`).
//!
//! GEMESSEN vorher (WPT css-cascade scope-implicit, scope-declarations,
//! scope-style-sharing-*): `@scope { … }` fiel ganz weg.

use narsil_fmt_html::HtmlNode;
use std::collections::HashMap;
use std::sync::Mutex;

/// Das interne Attribut am Elternelement.
pub(crate) const ATTRIBUT: &str = "data-narsil-bereich";

/// Hoechstens so viele Kennungen insgesamt (M3/M4: jede lebt bis zum Ende).
const MAX_KENNUNGEN: usize = 4_096;

/// Die Kennung eines Blatttextes (stabil: SipHash mit festem Schluessel).
fn kennung(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Der Kopf, der hinter `@scope` eingesetzt wird. Release 13.10.2026: inside
/// an outer `@scope` the root may also be the outer root itself (WPT
/// css-cascade scope-implicit "sandwiched right under an explicit scope");
/// `:scope` there is the outer root.
fn einschub(k: &str, nested: bool) -> String {
    if nested {
        format!(" ([{ATTRIBUT}~=\"{k}\"]:is(:scope, :scope *))")
    } else {
        format!(" ([{ATTRIBUT}~=\"{k}\"])")
    }
}

/// Den Blatttext umschreiben; `None` = kein implizites `@scope` darin.
fn text_umschreiben(text: &str, k: &str) -> Option<String> {
    let klein = text.to_ascii_lowercase();
    if !klein.contains("@scope") {
        return None;
    }
    let z: Vec<char> = text.chars().collect();
    let kz: Vec<char> = klein.chars().collect();
    if kz.len() != z.len() {
        return None;
    }
    let mut raus = String::with_capacity(text.len() + 48);
    // Je offenem Block: ist er eine @-Regel (Gruppe)? Is it an `@scope`?
    let mut bloecke: Vec<bool> = Vec::new();
    let mut in_scope: Vec<bool> = Vec::new();
    let mut kopf_start = 0usize;
    let mut geaendert = false;
    let mut i = 0usize;
    let mut in_text: Option<char> = None;
    while i < z.len() {
        let c = z[i];
        if let Some(q) = in_text {
            raus.push(c);
            if c == '\\' && i + 1 < z.len() {
                raus.push(z[i + 1]);
                i += 2;
                continue;
            }
            if c == q {
                in_text = None;
            }
            i += 1;
            continue;
        }
        if c == '/' && z.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j + 1 < z.len() && !(z[j] == '*' && z[j + 1] == '/') {
                j += 1;
            }
            let ende = (j + 2).min(z.len());
            raus.extend(&z[i..ende]);
            i = ende;
            continue;
        }
        match c {
            '"' | '\'' => {
                in_text = Some(c);
                raus.push(c);
                i += 1;
                continue;
            }
            '{' => {
                let kopf: String = z[kopf_start..i].iter().collect();
                let kopf = kopf.trim_start();
                bloecke.push(kopf.starts_with('@'));
                in_scope.push(kopf.get(..6).is_some_and(|k| k.eq_ignore_ascii_case("@scope")));
                kopf_start = i + 1;
            }
            '}' => {
                bloecke.pop();
                in_scope.pop();
                kopf_start = i + 1;
            }
            ';' => kopf_start = i + 1,
            '@' if kz[i..].starts_with(&['@', 's', 'c', 'o', 'p', 'e']) && bloecke.iter().all(|&g| g) => {
                let mut j = i + 6;
                while j < z.len() && z[j].is_whitespace() {
                    j += 1;
                }
                let name_ende = !z.get(i + 6).is_some_and(|n| n.is_alphanumeric() || *n == '-' || *n == '_');
                // Release 13.10.2026: also `@scope to (.b) { … }` (implicit
                // root with a limit; WPT scope-implicit "with limit").
                let only_limit = kz[j..].starts_with(&['t', 'o']) && z.get(j + 2).is_some_and(|n| n.is_whitespace() || *n == '(');
                if name_ende && (z.get(j) == Some(&'{') || only_limit) {
                    raus.push_str("@scope");
                    raus.push_str(&einschub(k, in_scope.iter().any(|&s| s)));
                    geaendert = true;
                    i += 6;
                    continue;
                }
            }
            _ => {}
        }
        raus.push(c);
        i += 1;
    }
    geaendert.then_some(raus)
}

fn stil_text(kinder: &[HtmlNode]) -> String {
    kinder
        .iter()
        .filter_map(|c| match c {
            HtmlNode::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// Steht in irgendeinem `<style>` ueberhaupt `@scope`? (Vor dem Kopieren.)
pub(crate) fn kommt_vor(liste: &[HtmlNode]) -> bool {
    fn gehen(liste: &[HtmlNode], tiefe: usize) -> bool {
        tiefe < 512
            && liste.iter().any(|k| match k {
                HtmlNode::Element { tag, children, shadow_root, .. } => {
                    (tag.eq_ignore_ascii_case("style") && stil_text(children).to_ascii_lowercase().contains("@scope"))
                        || gehen(children, tiefe + 1)
                        || shadow_root.as_ref().is_some_and(|s| gehen(&s.children, tiefe + 1))
                }
                _ => false,
            })
    }
    gehen(liste, 0)
}

/// Alle Kinder von `liste` durchgehen; `eltern` sammelt die Kennungen der
/// `<style>`-Kinder des Elternelements.
fn gehen(liste: &mut [HtmlNode], eltern: &mut Vec<String>, tiefe: usize) {
    if tiefe > 512 {
        return;
    }
    for knoten in liste.iter_mut() {
        let HtmlNode::Element { tag, attributes, children, shadow_root } = knoten else { continue };
        if tag.eq_ignore_ascii_case("style") {
            let text = stil_text(children);
            let k = kennung(&text);
            if let Some(neu) = text_umschreiben(&text, &k) {
                if !eltern.contains(&k) {
                    eltern.push(k);
                }
                *children = vec![HtmlNode::Text(neu)];
            }
            continue;
        }
        let mut eigene: Vec<String> = Vec::new();
        gehen(children, &mut eigene, tiefe + 1);
        if let Some(sr) = shadow_root.as_mut() {
            // Im Shadow Tree ist der Wirt die Wurzel.
            gehen(&mut sr.children, &mut eigene, tiefe + 1);
        }
        if !eigene.is_empty() {
            attributes.insert(String::from(ATTRIBUT), eigene.join(" "));
        }
    }
}

/// Die impliziten `@scope` eines Seitenbaums umschreiben (siehe oben).
/// Rueckgabe: ob etwas geaendert wurde.
pub(crate) fn umschreiben(baum: &mut [HtmlNode]) -> bool {
    if !kommt_vor(baum) {
        return false;
    }
    let mut oben: Vec<String> = Vec::new();
    gehen(baum, &mut oben, 0);
    true
}

/// Die eingesetzten Koepfe aus einem Text wieder heraus.
fn einschuebe_entfernen(text: &str) -> String {
    let vorn = format!(" ([{ATTRIBUT}~=\"");
    let mut raus = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(&vorn) {
        raus.push_str(&rest[..i]);
        let nach = &rest[i + vorn.len()..];
        let end = nach.find("\"]").and_then(|j| {
            let r = &nach[j + 2..];
            let r = r.strip_prefix(":is(:scope, :scope *)").unwrap_or(r);
            r.strip_prefix(')')
        });
        match end {
            Some(r) => rest = r,
            None => {
                raus.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    raus.push_str(rest);
    raus
}

/// Release 12.10.2026: das lebende DOM entsteht aus demselben Seitenbaum —
/// Attribut und eingesetzte Koepfe wieder heraus.
pub(crate) fn aus_dom_entfernen(dok: &mut narsil_fmt_html::dom::Dokument) {
    for k in 0..dok.knoten.len() {
        if dok.attribut(k, ATTRIBUT).is_some() {
            dok.attribut_entfernen(k, ATTRIBUT);
        }
        if dok.attribut(k, "type") == Some(TYP_AUS) {
            dok.attribut_entfernen(k, "type");
        }
        if !dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")) {
            continue;
        }
        let kinder: Vec<usize> = dok.kinder(k).to_vec();
        for c in kinder {
            let t = dok.text_inhalt(c);
            if t.contains(ATTRIBUT) {
                dok.text_inhalt_setzen(c, &einschuebe_entfernen(&t));
            }
        }
    }
}

/// Fuer `ArenaEl::attribut`: die Kennungen der `<style>`-Kinder mit
/// implizitem `@scope` (dauerhaft abgelegt, gedeckelt).
pub(crate) fn kennungen_der_kinder(dok: &narsil_fmt_html::dom::Dokument, k: usize) -> Option<&'static str> {
    static ABLAGE: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let mut liste: Vec<String> = Vec::new();
    for &c in dok.kinder(k).iter().take(4_096) {
        if !dok.element_name(c).is_some_and(|n| n.eq_ignore_ascii_case("style")) {
            continue;
        }
        let text = dok.text_inhalt(c);
        if !text.to_ascii_lowercase().contains("@scope") {
            continue;
        }
        let kn = kennung(&text);
        if text_umschreiben(&text, &kn).is_some() && !liste.contains(&kn) {
            liste.push(kn);
        }
    }
    if liste.is_empty() {
        return None;
    }
    let wert = liste.join(" ");
    let mut g = ABLAGE.lock().ok()?;
    let karte = g.get_or_insert_with(HashMap::new);
    if let Some(s) = karte.get(&wert) {
        return Some(s);
    }
    if karte.len() >= MAX_KENNUNGEN {
        return None;
    }
    let s: &'static str = Box::leak(wert.clone().into_boxed_str());
    karte.insert(wert, s);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nur_oben_und_in_gruppen() {
        let t = text_umschreiben("@scope { .a { color: red } } @media screen { @scope{ .b {} } } .x { @scope { .c {} } }", "k4").unwrap();
        assert_eq!(t.matches("[data-narsil-bereich~=\"k4\"]").count(), 2, "{t}");
        assert!(t.contains(".x { @scope { .c {} } }"), "{t}");
        assert!(text_umschreiben("@scope (.a) { }", "k").is_none());
        let t = text_umschreiben("@scope to (.b) { .a {} } @scope (#o) { @scope { :scope {} } }", "k").unwrap();
        assert!(t.contains("@scope ([data-narsil-bereich~=\"k\"]) to (.b)"), "{t}");
        assert!(t.contains("@scope ([data-narsil-bereich~=\"k\"]:is(:scope, :scope *)) {"), "{t}");
        assert_eq!(einschuebe_entfernen(&t), "@scope to (.b) { .a {} } @scope (#o) { @scope { :scope {} } }");
        assert!(text_umschreiben("p { content: \"@scope {\" }", "k").is_none());
        let roh = "@scope { .a {} } @media x { @scope{ } }";
        assert_eq!(einschuebe_entfernen(&text_umschreiben(roh, "ab12").unwrap()), roh);
    }
}

/// Release 12.10.2026: der Typ, mit dem ein nicht bevorzugtes Blatt im
/// Seitenbaum ausgeschaltet wird (`narsil-page` liest nur `text/css`).
pub(crate) const TYP_AUS: &str = "text/x-narsil-alternativ";

/// Release 14.10.2026: `<link disabled>` (HTML §4.6.7 "disabled") is switched
/// off in the page tree the same way (`narsil-page` skips a link whose `type`
/// is not `text/css`). MEASURED before (WPT cssom HTMLLinkElement-disabled-001):
/// the sheet of a disabled link applied.
/// An alternate style sheet with a title (`rel="alternate stylesheet"
/// title=…`) does not apply either, unless a script enabled it explicitly
/// (`enabled`, by position in the preorder walk; HTML §4.6.7). MEASURED
/// before (WPT cssom HTMLLinkElement-disabled-006): the alternate sheet
/// applied.
pub(crate) fn switch_off_disabled_links(tree: &mut [HtmlNode], enabled: &dyn Fn(usize) -> bool) {
    fn walk(list: &mut [HtmlNode], enabled: &dyn Fn(usize) -> bool, pos: &mut usize, depth: usize) {
        if depth > 512 {
            return;
        }
        for node in list.iter_mut() {
            let mine = *pos;
            *pos += 1;
            let HtmlNode::Element { tag, attributes, children, .. } = node else { continue };
            if tag.eq_ignore_ascii_case("link") && link_is_off(attributes, || enabled(mine)) {
                attributes.insert(String::from("type"), String::from(TYP_AUS));
            }
            walk(children, enabled, pos, depth + 1);
        }
    }
    let mut pos = 0;
    walk(tree, enabled, &mut pos, 0);
}

fn link_is_off(attributes: &std::collections::HashMap<String, String>, enabled: impl Fn() -> bool) -> bool {
    if attributes.contains_key("disabled") {
        return true;
    }
    let rel = attributes.get("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
    let alternate = rel.split_whitespace().any(|r| r == "alternate") && rel.split_whitespace().any(|r| r == "stylesheet");
    alternate && attributes.get("title").is_some_and(|t| !t.is_empty()) && !enabled()
}

/// Is there a `<link>` that `switch_off_disabled_links` would switch off?
/// (Before copying the tree.)
pub(crate) fn has_disabled_link(list: &[HtmlNode], depth: usize) -> bool {
    depth < 512
        && list.iter().any(|node| match node {
            HtmlNode::Element { tag, attributes, children, .. } => {
                (tag.eq_ignore_ascii_case("link") && link_is_off(attributes, || false)) || has_disabled_link(children, depth + 1)
            }
            HtmlNode::Text(_) => false,
        })
}

/// Release 12.10.2026: die bevorzugte Blattmenge (CSSOM §6.2 „preferred CSS
/// style sheet set name"): das erste `<style title>` mit nicht leerem Titel
/// bestimmt sie; `<style>` mit einem anderen Titel gelten nicht. GEMESSEN
/// vorher (WPT cssom stylesheet-title, preferred-stylesheet-order): beide
/// Blaetter galten, das spaetere gewann.
pub(crate) fn titel_auswahl(baum: &mut [HtmlNode], bevorzugt: Option<&str>) {
    fn sammeln(liste: &[HtmlNode], erster: &mut Option<String>, gibt_andere: &mut bool, tiefe: usize) {
        if tiefe > 512 {
            return;
        }
        for k in liste {
            let HtmlNode::Element { tag, attributes, children, .. } = k else { continue };
            if tag.eq_ignore_ascii_case("style")
                && let Some(t) = attributes.get("title").filter(|t| !t.is_empty())
            {
                match erster {
                    None => *erster = Some(t.clone()),
                    Some(e) if e != t => *gibt_andere = true,
                    _ => {}
                }
            }
            sammeln(children, erster, gibt_andere, tiefe + 1);
        }
    }
    fn ausschalten(liste: &mut [HtmlNode], titel: &str, tiefe: usize) {
        if tiefe > 512 {
            return;
        }
        for k in liste.iter_mut() {
            let HtmlNode::Element { tag, attributes, children, .. } = k else { continue };
            if tag.eq_ignore_ascii_case("style")
                && attributes.get("title").is_some_and(|t| !t.is_empty() && t != titel)
            {
                attributes.insert(String::from("type"), String::from(TYP_AUS));
            }
            ausschalten(children, titel, tiefe + 1);
        }
    }
    let mut erster = None;
    let mut andere = false;
    sammeln(baum, &mut erster, &mut andere, 0);
    // Der Name kommt vom ZUERST hinzugefuegten Blatt (CSSOM §6.2 „add a CSS
    // style sheet"), nicht vom ersten im Baum (WPT preferred-stylesheet-
    // reversed-order); ohne Angabe gilt die Baumordnung (Parser).
    let titel = bevorzugt.map(str::to_string).or(erster);
    if andere && let Some(t) = titel {
        ausschalten(baum, &t, 0);
    }
}
