//! Release 08.10.2026: DER GEPARSTE BAUM fuer die Features 1, 2, 4 und 5.
//!
//! Die Vorlauf-Nummern (`ord`) zaehlen hier genau wie `narsil_page::zaehle`:
//! jeder Knoten in Dokumentreihenfolge, ohne die Texte in Steuerelementen
//! und ohne `option`/`optgroup`, bis `MAX_BAU_TIEFE`. Mit derselben Zahl
//! arbeiten der DOM-Stand und der `Patch`.

use narsil_fmt_html::HtmlNode;

const STEUER: [&str; 6] = ["input", "button", "select", "textarea", "option", "optgroup"];

/// Ein gezaehlter Knoten.
#[derive(Debug, Clone, Copy)]
pub struct Stelle<'a> {
    pub knoten: &'a HtmlNode,
    pub ord: usize,
    /// Die erste Nummer hinter dem Teilbaum (Vorlauf: der Teilbaum ist
    /// `ord..ende`).
    pub ende: usize,
    pub tiefe: usize,
    pub eltern: Option<usize>,
}

/// Alle gezaehlten Knoten in Vorlauf-Reihenfolge (Index = `ord`).
pub fn stellen(knoten: &[HtmlNode]) -> Vec<Stelle<'_>> {
    fn lauf<'a>(
        liste: &'a [HtmlNode],
        tiefe: usize,
        im_steuer: bool,
        eltern: Option<usize>,
        raus: &mut Vec<Stelle<'a>>,
    ) {
        for k in liste {
            if tiefe >= narsil_page::MAX_BAU_TIEFE {
                return;
            }
            if im_steuer {
                match k {
                    HtmlNode::Text(_) => continue,
                    HtmlNode::Element { tag, .. }
                        if matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup") =>
                    {
                        continue;
                    }
                    _ => {}
                }
            }
            let me = raus.len();
            raus.push(Stelle { knoten: k, ord: me, ende: me + 1, tiefe, eltern });
            if let HtmlNode::Element { tag, children, .. } = k {
                let klein = tag.to_ascii_lowercase();
                let tiefer = im_steuer || STEUER.contains(&klein.as_str());
                lauf(children, tiefe + 1, tiefer, Some(me), raus);
            }
            let ende = raus.len();
            raus[me].ende = ende;
        }
    }
    let mut raus = Vec::new();
    lauf(knoten, 0, false, None, &mut raus);
    raus
}

/// Die wirksamen Texte des Patches: ein Eintrag auf einem ELEMENT gilt fuer
/// alle seine direkten Textkinder (wie `narsil_page::zaehle_und_erweitere` —
/// `el.textContent = "x"` steht im Patch unter der Nummer des Elements).
pub fn texte(st: &[Stelle<'_>], patch: &narsil_page::Patch) -> std::collections::HashMap<usize, String> {
    let mut raus = patch.text.clone();
    for (ord, neu) in &patch.text {
        if !matches!(st.get(*ord).map(|s| s.knoten), Some(HtmlNode::Element { .. })) {
            continue;
        }
        raus.remove(ord);
        let s = st[*ord];
        for x in &st[(s.ord + 1).min(st.len())..s.ende.min(st.len())] {
            if x.eltern == Some(*ord) && matches!(x.knoten, HtmlNode::Text(_)) {
                raus.insert(x.ord, neu.clone());
            }
        }
    }
    raus
}

/// Leerraum zusammenziehen.
pub fn zusammen(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Der Text des Teilbaums `ord` — mit den Texten des Patches, wenn da.
pub fn text_von(st: &[Stelle<'_>], ord: usize, patch: Option<&narsil_page::Patch>) -> String {
    let Some(s) = st.get(ord) else {
        return String::new();
    };
    let wirksam = patch.map(|p| texte(st, p));
    let mut t = String::new();
    for x in &st[s.ord..s.ende.min(st.len())] {
        if let HtmlNode::Element { tag, .. } = x.knoten
            && matches!(tag.to_ascii_lowercase().as_str(), "script" | "style" | "noscript" | "template")
        {
            continue;
        }
        let in_skript = {
            let mut e = x.eltern;
            let mut ja = false;
            while let Some(i) = e {
                if i < s.ord {
                    break;
                }
                if let HtmlNode::Element { tag, .. } = st[i].knoten
                    && matches!(tag.to_ascii_lowercase().as_str(), "script" | "style" | "noscript" | "template")
                {
                    ja = true;
                    break;
                }
                e = st[i].eltern;
            }
            ja
        };
        if in_skript {
            continue;
        }
        if let HtmlNode::Text(text) = x.knoten {
            match wirksam.as_ref().and_then(|w| w.get(&x.ord)) {
                Some(neu) => t.push_str(neu),
                None => t.push_str(text),
            }
            t.push(' ');
        }
    }
    zusammen(&narsil_fmt_html::decode_entities(&t))
}

/// Das naechste Element (der Knoten selbst oder sein Eltern).
pub fn element_von(st: &[Stelle<'_>], ord: usize) -> Option<usize> {
    let mut o = Some(ord);
    while let Some(i) = o {
        if matches!(st.get(i)?.knoten, HtmlNode::Element { .. }) {
            return Some(i);
        }
        o = st[i].eltern;
    }
    None
}

/// Der Pfad zu einem Element: `tag[n]/tag[n]/…` mit `n` = Stelle unter den
/// gleichnamigen Geschwister-Elementen. Ein Element mit `id` beginnt den
/// Pfad neu (`#id/…`), weil die `id` robuster ist als die Zaehlung.
pub fn pfad_zu(knoten: &[HtmlNode], ziel: &HtmlNode) -> Option<String> {
    fn lauf(liste: &[HtmlNode], ziel: &HtmlNode, weg: &mut Vec<String>) -> bool {
        let mut zaehler: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for k in liste {
            if let HtmlNode::Element { tag, attributes, children, .. } = k {
                let t = tag.to_ascii_lowercase();
                let n = zaehler.entry(t.clone()).or_insert(0);
                let teil = match attributes.get("id") {
                    Some(id) if !id.trim().is_empty() && !id.contains('/') => format!("#{id}"),
                    _ => format!("{t}[{n}]"),
                };
                *n += 1;
                weg.push(teil);
                if std::ptr::eq(k, ziel) || lauf(children, ziel, weg) {
                    return true;
                }
                weg.pop();
            }
        }
        false
    }
    let mut weg = Vec::new();
    if !lauf(knoten, ziel, &mut weg) {
        return None;
    }
    // Ab der letzten `id` beginnen.
    let start = weg.iter().rposition(|t| t.starts_with('#')).unwrap_or(0);
    Some(weg[start..].join("/"))
}

/// Ein Element ueber seinen Pfad finden.
pub fn finde<'a>(knoten: &'a [HtmlNode], pfad: &str) -> Option<&'a HtmlNode> {
    let teile: Vec<&str> = pfad.split('/').filter(|t| !t.is_empty()).collect();
    let (start, rest): (Option<&'a HtmlNode>, &[&str]) = match teile.first() {
        Some(t) if t.starts_with('#') => (Some(nach_id(knoten, &t[1..])?), &teile[1..]),
        _ => (None, &teile[..]),
    };
    let mut kinder: &'a [HtmlNode] = match start {
        Some(HtmlNode::Element { children, .. }) => children,
        _ => knoten,
    };
    let mut gefunden = start;
    for t in rest {
        let (tag, n) = if let Some(id) = t.strip_prefix('#') {
            gefunden = Some(nach_id(knoten, id)?);
            if let Some(HtmlNode::Element { children, .. }) = gefunden {
                kinder = children;
            }
            continue;
        } else {
            let (tag, nr) = t.split_once('[')?;
            (tag.to_string(), nr.trim_end_matches(']').parse::<usize>().ok()?)
        };
        let k = kinder
            .iter()
            .filter(|k| matches!(k, HtmlNode::Element { tag: x, .. } if x.eq_ignore_ascii_case(&tag)))
            .nth(n)?;
        gefunden = Some(k);
        if let HtmlNode::Element { children, .. } = k {
            kinder = children;
        }
    }
    gefunden
}

fn nach_id<'a>(knoten: &'a [HtmlNode], id: &str) -> Option<&'a HtmlNode> {
    for k in knoten {
        if let HtmlNode::Element { attributes, children, .. } = k {
            if attributes.get("id").map(String::as_str) == Some(id) {
                return Some(k);
            }
            if let Some(f) = nach_id(children, id) {
                return Some(f);
            }
        }
    }
    None
}

/// Der Text eines Knotens ohne Patch (fuer den Hintergrund-Abruf).
pub fn text_des_knotens(k: &HtmlNode) -> String {
    fn lauf(k: &HtmlNode, t: &mut String) {
        match k {
            HtmlNode::Text(x) => {
                t.push_str(x);
                t.push(' ');
            }
            HtmlNode::Element { tag, children, .. } => {
                if matches!(tag.to_ascii_lowercase().as_str(), "script" | "style" | "noscript" | "template") {
                    return;
                }
                for c in children {
                    lauf(c, t);
                }
            }
        }
    }
    let mut t = String::new();
    lauf(k, &mut t);
    zusammen(&narsil_fmt_html::decode_entities(&t))
}

/// DER GEPATCHTE BAUM fuer Schnappschuss und Einfrieren: Texte, Attribute
/// (inklusive `classList`/`style`), entfernte und neue Knoten des Patches
/// eingearbeitet; `<script>` und die eingesetzten Knoten der Features
/// fallen weg (der Stand soll ohne Skripte wieder erscheinen).
pub fn gepatcht(knoten: &[HtmlNode], patch: &narsil_page::Patch) -> Vec<HtmlNode> {
    fn attribute(
        alt: &std::collections::HashMap<String, String>,
        ord: usize,
        patch: &narsil_page::Patch,
    ) -> std::collections::HashMap<String, String> {
        let mut anweisungen: Vec<(&String, &String)> = patch
            .attribut
            .iter()
            .filter(|((o, _), _)| *o == ord)
            .map(|((_, n), w)| (n, w))
            .collect();
        if anweisungen.is_empty() {
            return alt.clone();
        }
        anweisungen.sort_by(|a, b| a.0.cmp(b.0));
        let mut eigene = alt.clone();
        for (name, wert) in anweisungen {
            if name == narsil_page::W6_KLASSEN_MARKER {
                let k = eigene.get("class").cloned().unwrap_or_default();
                eigene.insert("class".into(), narsil_page::klassen_wenden(&k, wert));
            } else if name == narsil_page::W6_STIL_MARKER {
                let s = eigene.get("style").cloned().unwrap_or_default();
                let neu = if s.trim().is_empty() { wert.clone() } else { format!("{};{wert}", s.trim_end_matches(';')) };
                eigene.insert("style".into(), neu);
            } else if name.starts_with('\u{1}') {
                // andere interne Marker: nicht ins HTML schreiben
            } else if name == "checked" && wert.is_empty() {
                eigene.remove("checked");
            } else {
                eigene.insert(name.clone(), wert.clone());
            }
        }
        eigene
    }
    fn neue_kinder(eltern: usize, lage: &narsil_page::NeuLage, patch: &narsil_page::Patch) -> Vec<HtmlNode> {
        patch
            .neu
            .iter()
            .filter(|n| {
                &n.lage == lage
                    && (n.eltern_ord == eltern
                        || matches!(lage, narsil_page::NeuLage::Vor(_) | narsil_page::NeuLage::Nach(_)))
            })
            .map(|n| HtmlNode::Element {
                tag: n.tag.clone(),
                attributes: Default::default(),
                children: n.text.clone().map(HtmlNode::Text).into_iter().collect(),
                shadow_root: None,
            })
            .collect()
    }
    fn lauf(
        liste: &[HtmlNode],
        tiefe: usize,
        im_steuer: bool,
        ord: &mut usize,
        eltern: Option<usize>,
        patch: &narsil_page::Patch,
        wirksam: &std::collections::HashMap<usize, String>,
    ) -> Vec<HtmlNode> {
        let mut raus = Vec::new();
        if let Some(e) = eltern {
            raus.extend(neue_kinder(e, &narsil_page::NeuLage::Anfang, patch));
        }
        for k in liste {
            if tiefe >= narsil_page::MAX_BAU_TIEFE {
                break;
            }
            if im_steuer {
                match k {
                    HtmlNode::Text(_) => {
                        raus.push(k.clone());
                        continue;
                    }
                    HtmlNode::Element { tag, .. }
                        if matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup") =>
                    {
                        raus.push(k.clone());
                        continue;
                    }
                    _ => {}
                }
            }
            let me = *ord;
            *ord += 1;
            match k {
                HtmlNode::Text(t) => {
                    if !patch.weg.contains(&me) {
                        raus.push(HtmlNode::Text(wirksam.get(&me).cloned().unwrap_or_else(|| t.clone())));
                    }
                }
                HtmlNode::Element { tag, attributes, children, shadow_root } => {
                    let klein = tag.to_ascii_lowercase();
                    let tiefer = im_steuer || STEUER.contains(&klein.as_str());
                    let mut kinder = lauf(children, tiefe + 1, tiefer, ord, Some(me), patch, wirksam);
                    if patch.weg.contains(&me)
                        || klein == "script"
                        || attributes.contains_key(super::ruhe::KENNUNG)
                    {
                        continue;
                    }
                    raus.extend(neue_kinder(me, &narsil_page::NeuLage::Vor(me), patch));
                    let attrs = attribute(attributes, me, patch);
                    // Ein Textfeld traegt seinen Wert als Inhalt.
                    if klein == "textarea"
                        && let Some(w) = patch.attribut.get(&(me, "value".to_string()))
                    {
                        kinder = vec![HtmlNode::Text(w.clone())];
                    }
                    kinder.extend(neue_kinder(me, &narsil_page::NeuLage::Ende, patch));
                    raus.push(HtmlNode::Element {
                        tag: tag.clone(),
                        attributes: attrs,
                        children: kinder,
                        shadow_root: shadow_root.clone(),
                    });
                    raus.extend(neue_kinder(me, &narsil_page::NeuLage::Nach(me), patch));
                }
            }
        }
        raus
    }
    let wirksam = texte(&stellen(knoten), patch);
    let mut ord = 0usize;
    lauf(knoten, 0, false, &mut ord, None, patch, &wirksam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_zaehlung_ist_die_des_bauers() {
        let html = narsil_page::html_lesen(
            "<html><body><h1>Titel</h1><button>Knopf</button><p>Text <span id=\"z\">19,99 €</span></p></body></html>",
        );
        let st = stellen(&html);
        let seite = narsil_page::seite_aus_knoten_mit_messung(
            &html,
            &narsil_page::Patch::default(),
            800,
            "file:///t.html",
            &narsil_layout::Messung::bitmap(),
        );
        // Die Nummer der id `z` aus dem Bauer ist die unsere.
        let ord_z = *seite.ids.get("z").expect("id z");
        assert!(matches!(st[ord_z].knoten, HtmlNode::Element { tag, .. } if tag == "span"));
        assert_eq!(text_von(&st, ord_z, None), "19,99 €");
        let mut patch = narsil_page::Patch::default();
        patch.text.insert(ord_z + 1, "17,49 €".into());
        assert_eq!(text_von(&st, ord_z, Some(&patch)), "17,49 €");
    }

    #[test]
    fn der_pfad_findet_das_element_wieder() {
        let a = narsil_page::html_lesen("<html><body><div><p>a</p><p>b <b>x</b></p></div><div id=\"k\"><ul><li>1</li><li>2</li></ul></div></body></html>");
        let st = stellen(&a);
        for s in &st {
            if let HtmlNode::Element { .. } = s.knoten {
                let p = pfad_zu(&a, s.knoten).expect("pfad");
                let f = finde(&a, &p).expect("finde");
                assert!(std::ptr::eq(f, s.knoten), "{p}");
            }
        }
        let li = st.iter().find(|s| text_des_knotens(s.knoten) == "2" && matches!(s.knoten, HtmlNode::Element { tag, .. } if tag == "li")).expect("li 2");
        assert_eq!(pfad_zu(&a, li.knoten).as_deref(), Some("#k/ul[0]/li[1]"));
        // In einer neu geladenen Fassung mit anderem Inhalt davor:
        let b = narsil_page::html_lesen("<html><body><div><p>neu</p></div><div id=\"k\"><ul><li>1</li><li>3</li></ul></div></body></html>");
        assert_eq!(finde(&b, "#k/ul[0]/li[1]").map(text_des_knotens).as_deref(), Some("3"));
    }

    #[test]
    fn der_gepatchte_baum_traegt_texte_werte_und_keine_skripte() {
        let html = narsil_page::html_lesen(
            "<html><body><p id=\"a\">vorher</p><input name=\"q\" value=\"\"><textarea name=\"t\"></textarea><script>alert(1)</script><p class=\"x\">weg</p></body></html>",
        );
        let st = stellen(&html);
        let ord_a = st.iter().find(|s| matches!(s.knoten, HtmlNode::Element { attributes, .. } if attributes.get("id").map(String::as_str) == Some("a"))).expect("a").ord;
        let ord_in = st.iter().find(|s| matches!(s.knoten, HtmlNode::Element { tag, .. } if tag == "input")).expect("input").ord;
        let ord_ta = st.iter().find(|s| matches!(s.knoten, HtmlNode::Element { tag, .. } if tag == "textarea")).expect("ta").ord;
        let ord_weg = st.iter().rev().find(|s| matches!(s.knoten, HtmlNode::Element { tag, .. } if tag == "p")).expect("p").ord;
        let mut patch = narsil_page::Patch::default();
        patch.text.insert(ord_a + 1, "nachher".into());
        patch.attribut.insert((ord_in, "value".into()), "getippt".into());
        patch.attribut.insert((ord_ta, "value".into()), "langer Text".into());
        patch.attribut.insert((ord_a, narsil_page::W6_KLASSEN_MARKER.into()), "+neu".into());
        patch.weg.insert(ord_weg);
        let text = narsil_fmt_html::serialisiere(&gepatcht(&html, &patch));
        assert!(text.contains("nachher") && !text.contains("vorher"), "{text}");
        assert!(text.contains("value=\"getippt\""), "{text}");
        assert!(text.contains("langer Text"), "{text}");
        assert!(text.contains("class=\"neu\""), "{text}");
        assert!(!text.contains("alert(1)") && !text.contains(">weg<"), "{text}");
    }
}
