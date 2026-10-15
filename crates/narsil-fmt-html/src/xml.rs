//! Release 11.10.2026: ein kleiner XML-Parser (XML 1.0 + Namespaces in XML
//! 1.0), der direkt in die Arena eines [`Dokument`]s baut.
//!
//! GEMESSEN vorher: es gab keinen. XHTML wurde zu HTML umgeschrieben
//! (`xhtml::xhtml_als_html`), `DOMParser` las jeden Typ als HTML, und ein
//! `<iframe src="dummy.xml">` bekam ein HTML-Dokument (WPT dom/nodes
//! Document-createElementNS: 233 Teiltests, „Dummy XML document\n").
//!
//! Umfang: Elemente mit Namensraeumen (`xmlns`, `xmlns:p`), Attribute,
//! Text mit den fuenf vordefinierten Entitaeten und Zeichenreferenzen,
//! CDATA-Abschnitte, Kommentare, Verarbeitungsanweisungen, ein Doctype
//! (der interne Teil wird uebersprungen). Ein Fehler der Wohlgeformtheit
//! liefert `Err` — der Aufrufer baut dann das `parsererror`-Dokument.

use crate::dom::{Art, Dokument, MAX_KNOTEN, Namensraum};
use std::collections::HashMap;

const NS_XHTML: &str = "http://www.w3.org/1999/xhtml";
const NS_SVG: &str = "http://www.w3.org/2000/svg";
const NS_MATHML: &str = "http://www.w3.org/1998/Math/MathML";
const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";
const NS_XMLNS: &str = "http://www.w3.org/2000/xmlns/";
const NS_XLINK: &str = "http://www.w3.org/1999/xlink";
/// Hoechste Verschachtelung (M3).
const MAX_TIEFE: usize = 512;

/// Liest `text` als XML-Dokument in den Dokumentknoten `doc`.
///
/// # Errors
/// Beschreibung des ersten Wohlgeformtheitsfehlers.
pub fn xml_parsen(dok: &mut Dokument, doc: usize, text: &str) -> Result<(), String> {
    let mut p = Leser { s: text.trim_start_matches('\u{feff}'), i: 0 };
    // Offene Elemente: (Knoten, qualifizierter Name, Namensraum-Bindungen).
    let mut offen: Vec<(usize, String, HashMap<String, String>)> = Vec::new();
    let mut wurzel_gesehen = false;
    // Release 13.10.2026: allgemeine Entitaeten aus dem internen Teil des
    // Doctypes (`<!ENTITY x "y">`, XML 1.0 §4.2).
    let mut eigene: HashMap<String, String> = HashMap::new();
    while p.i < p.s.len() {
        if dok.knoten.len() + 4 >= MAX_KNOTEN {
            return Err("zu viele Knoten".into());
        }
        let eltern = offen.last().map_or(doc, |o| o.0);
        if p.beginnt("<!--") {
            let ende = p.rest().find("-->").ok_or("Kommentar ohne Ende")?;
            let inhalt = &p.rest()[4..ende];
            if inhalt.contains("--") {
                return Err("'--' in einem Kommentar".into());
            }
            if let Some(k) = dok.neuer_kommentar(inhalt) {
                dok.anhaengen(eltern, k);
            }
            p.i += ende + 3;
        } else if p.beginnt("<![CDATA[") {
            if offen.is_empty() {
                return Err("CDATA ausserhalb des Wurzelelements".into());
            }
            let ende = p.rest().find("]]>").ok_or("CDATA ohne Ende")?;
            let inhalt = &p.rest()[9..ende];
            if let Some(k) = dok.neuer_text(inhalt) {
                dok.cdata.insert(k);
                dok.anhaengen(eltern, k);
            }
            p.i += ende + 3;
        } else if p.beginnt("<!DOCTYPE") {
            if wurzel_gesehen || !offen.is_empty() {
                return Err("Doctype nach dem Wurzelelement".into());
            }
            p.i += 9;
            p.leer();
            let name = p.name().ok_or("Doctype ohne Namen")?.to_string();
            let mut public_id = String::new();
            let mut system_id = String::new();
            p.leer();
            if p.beginnt("PUBLIC") {
                p.i += 6;
                p.leer();
                public_id = p.zitat().ok_or("PUBLIC ohne Kennung")?.to_string();
                p.leer();
                // Release 13.10.2026: nach PUBLIC ist die Systemkennung
                // Pflicht (XML 1.0 §4.2.2 ExternalID).
                system_id = p.zitat().ok_or("PUBLIC ohne Systemkennung")?.to_string();
            } else if p.beginnt("SYSTEM") {
                p.i += 6;
                p.leer();
                system_id = p.zitat().ok_or("SYSTEM ohne Kennung")?.to_string();
            }
            p.leer();
            if p.beginnt("[") {
                let ende = interner_teil(p.rest(), &mut eigene)?;
                p.i += ende + 1;
                p.leer();
            }
            if !p.beginnt(">") {
                return Err("Doctype ohne '>'".into());
            }
            p.i += 1;
            let d = dok.neuer_knoten(Art::Doctype { name, public_id, system_id });
            dok.anhaengen(doc, d);
        } else if p.beginnt("<?") {
            let ende = p.rest().find("?>").ok_or("Verarbeitungsanweisung ohne Ende")?;
            let inhalt = &p.rest()[2..ende];
            let (ziel, daten) = match inhalt.find(|c: char| c.is_ascii_whitespace()) {
                Some(n) => (&inhalt[..n], inhalt[n..].trim_start()),
                None => (inhalt, ""),
            };
            if ziel.is_empty() {
                return Err("Verarbeitungsanweisung ohne Ziel".into());
            }
            if !ziel.eq_ignore_ascii_case("xml") {
                let k = dok.neuer_knoten(Art::Pi { target: ziel.to_string(), daten: daten.to_string() });
                dok.anhaengen(eltern, k);
            } else if p.i != 0 {
                return Err("XML-Deklaration nicht am Anfang".into());
            }
            p.i += ende + 2;
        } else if p.beginnt("</") {
            p.i += 2;
            let name = p.name().ok_or("Endmarke ohne Namen")?.to_string();
            p.leer();
            if !p.beginnt(">") {
                return Err("Endmarke ohne '>'".into());
            }
            p.i += 1;
            match offen.pop() {
                Some((_, q, _)) if q == name => {}
                _ => return Err(format!("unerwartete Endmarke </{name}>")),
            }
        } else if p.beginnt("<") {
            if offen.is_empty() && wurzel_gesehen {
                return Err("zweites Wurzelelement".into());
            }
            if offen.len() >= MAX_TIEFE {
                return Err("zu tief verschachtelt".into());
            }
            p.i += 1;
            let q = p.name().ok_or("Starttag ohne Namen")?.to_string();
            if !qname_ok(&q) {
                return Err(format!("kein QName: <{q}>"));
            }
            // Attribute lesen.
            let mut attrs: Vec<(String, String)> = Vec::new();
            loop {
                let vorher = p.i;
                p.leer();
                if p.beginnt("/>") || p.beginnt(">") {
                    break;
                }
                if p.i == vorher {
                    return Err(format!("Leerraum vor dem Attribut in <{q}> fehlt"));
                }
                let an = p.name().ok_or_else(|| format!("kaputtes Attribut in <{q}>"))?.to_string();
                if !qname_ok(&an) {
                    return Err(format!("kein QName: Attribut {an}"));
                }
                p.leer();
                if !p.beginnt("=") {
                    return Err(format!("Attribut {an} ohne Wert"));
                }
                p.i += 1;
                p.leer();
                let roh = p.zitat().ok_or_else(|| format!("Attribut {an} ohne Anfuehrungszeichen"))?;
                if roh.contains('<') {
                    return Err("'<' in einem Attributwert".into());
                }
                let wert = entitaeten(roh, true, &eigene)?;
                if attrs.iter().any(|(n, _)| *n == an) {
                    return Err(format!("doppeltes Attribut {an}"));
                }
                attrs.push((an, wert));
            }
            let leer_tag = p.beginnt("/>");
            p.i += if leer_tag { 2 } else { 1 };
            // Namensraeume: die Bindungen des Elternteils plus die eigenen.
            let mut bindungen = offen.last().map(|o| o.2.clone()).unwrap_or_default();
            for (n, v) in &attrs {
                if n == "xmlns" {
                    bindungen.insert(String::new(), v.clone());
                } else if let Some(pf) = n.strip_prefix("xmlns:") {
                    if v.is_empty() {
                        return Err(format!("leere Bindung fuer {pf}"));
                    }
                    bindungen.insert(pf.to_string(), v.clone());
                }
            }
            let (praefix, lokal) = match q.split_once(':') {
                Some((a, b)) => (Some(a.to_string()), b.to_string()),
                None => (None, q.clone()),
            };
            let uri = match &praefix {
                Some(pf) if pf == "xml" => NS_XML.to_string(),
                Some(pf) => bindungen.get(pf).cloned().ok_or_else(|| format!("unbekanntes Praefix {pf}"))?,
                None => bindungen.get("").cloned().unwrap_or_default(),
            };
            let raum = match uri.as_str() {
                NS_XHTML => Namensraum::Html,
                NS_SVG => Namensraum::Svg,
                NS_MATHML => Namensraum::MathMl,
                _ => Namensraum::Fremd,
            };
            let e = dok.neuer_knoten(Art::Element {
                raum,
                name: lokal,
                attribute: Vec::new(),
                template_inhalt: None,
                shadow_root: None,
            });
            if raum == Namensraum::Fremd || praefix.is_some() {
                dok.fremd_ns.insert(e, (if raum == Namensraum::Fremd { uri } else { String::new() }, praefix));
            }
            for (n, v) in &attrs {
                let (araum, aname) = if n == "xmlns" {
                    (Namensraum::Xmlns, "xmlns".to_string())
                } else if let Some(pf) = n.strip_prefix("xmlns:") {
                    (Namensraum::Xmlns, pf.to_string())
                } else if let Some(r) = n.strip_prefix("xml:") {
                    (Namensraum::Xml, r.to_string())
                } else if let Some((pf, r)) = n.split_once(':') {
                    match bindungen.get(pf).map(String::as_str) {
                        Some(NS_XLINK) => (Namensraum::Xlink, r.to_string()),
                        Some(_) => (Namensraum::Html, n.clone()),
                        None => return Err(format!("unbekanntes Attribut-Praefix {pf}")),
                    }
                } else {
                    (Namensraum::Html, n.clone())
                };
                let _ = (NS_XMLNS, NS_XML);
                dok.setze_attribut(e, araum, &aname, v);
            }
            dok.anhaengen(eltern, e);
            if offen.is_empty() {
                wurzel_gesehen = true;
            }
            if !leer_tag {
                offen.push((e, q, bindungen));
            }
        } else {
            // Text bis zur naechsten Marke.
            let ende = p.rest().find('<').unwrap_or(p.rest().len());
            let roh = &p.rest()[..ende];
            p.i += ende;
            if offen.is_empty() {
                if !roh.trim().is_empty() {
                    return Err("Text ausserhalb des Wurzelelements".into());
                }
                continue;
            }
            if roh.contains("]]>") {
                return Err("']]>' im Text".into());
            }
            let t = entitaeten(roh, false, &eigene)?;
            if let Some(k) = dok.neuer_text(&t) {
                dok.anhaengen(eltern, k);
            }
        }
    }
    if let Some((_, q, _)) = offen.last() {
        return Err(format!("<{q}> nicht geschlossen"));
    }
    if !wurzel_gesehen {
        return Err("kein Wurzelelement".into());
    }
    Ok(())
}

struct Leser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Leser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }
    fn beginnt(&self, t: &str) -> bool {
        self.rest().starts_with(t)
    }
    fn leer(&mut self) {
        let r = self.rest();
        self.i += r.len() - r.trim_start_matches([' ', '\t', '\n', '\r']).len();
    }
    /// Ein XML-Name (mit `:`).
    fn name(&mut self) -> Option<&'a str> {
        let r = self.rest();
        let mut ende = 0;
        for (n, c) in r.char_indices() {
            let erlaubt = c.is_alphabetic() || c == '_' || c == ':' || (n > 0 && (c.is_alphanumeric() || c == '-' || c == '.' || c == '\u{b7}'))
                || (c as u32) >= 0xC0;
            if !erlaubt {
                break;
            }
            ende = n + c.len_utf8();
        }
        if ende == 0 {
            return None;
        }
        self.i += ende;
        Some(&r[..ende])
    }
    /// Ein Wert in `"…"` oder `'…'`.
    fn zitat(&mut self) -> Option<&'a str> {
        let r = self.rest();
        let q = r.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let ende = r[1..].find(q)?;
        self.i += ende + 2;
        Some(&r[1..=ende])
    }
}

/// Ersetzt die fuenf Entitaeten und Zeichenreferenzen (in Attributen auch
/// die Normalisierung von Leerraum zu Leerzeichen).
fn entitaeten(roh: &str, attribut: bool, eigene: &HashMap<String, String>) -> Result<String, String> {
    let mut raus = String::with_capacity(roh.len());
    let mut rest = roh;
    while let Some(n) = rest.find('&') {
        let teil = &rest[..n];
        if attribut {
            raus.extend(teil.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }));
        } else {
            raus.push_str(&teil.replace("\r\n", "\n"));
        }
        let nach = &rest[n + 1..];
        let ende = nach.find(';').ok_or("'&' ohne ';'")?;
        let name = &nach[..ende];
        let c = match name {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32).ok_or("kaputte Zeichenreferenz")?,
            _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32).ok_or("kaputte Zeichenreferenz")?,
            _ => match eigene.get(name) {
                // Der Ersatztext einer eigenen Entitaet (ohne weitere
                // Verschachtelung; Leerraum bleibt wie deklariert).
                Some(w) => {
                    raus.push_str(w);
                    rest = &nach[ende + 1..];
                    continue;
                }
                None => return Err(format!("unbekannte Entitaet &{name};")),
            },
        };
        raus.push(c);
        rest = &nach[ende + 1..];
    }
    if attribut {
        raus.extend(rest.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }));
    } else {
        raus.push_str(&rest.replace("\r\n", "\n"));
    }
    Ok(raus)
}

/// Release 13.10.2026: Namespaces in XML §3 — ein QName ist `NCName` oder
/// `NCName:NCName` (kein leeres Praefix, kein zweiter Doppelpunkt). GEMESSEN
/// vorher (WPT DOMParser-parseFromString-xml-parsererror): `:test`, `::`,
/// `xmlns:` liefen ohne Fehler durch.
fn qname_ok(q: &str) -> bool {
    match q.split_once(':') {
        None => !q.is_empty(),
        Some((a, b)) => !a.is_empty() && !b.is_empty() && !b.contains(':') && !b.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.'),
    }
}

/// Release 13.10.2026: der interne Teil des Doctypes ab `[` — liefert den
/// Index der schliessenden `]` und sammelt `<!ENTITY name "wert">`
/// (Parameter-Entitaeten `%` und andere Deklarationen werden uebersprungen).
fn interner_teil(r: &str, eigene: &mut HashMap<String, String>) -> Result<usize, String> {
    let b = r.as_bytes();
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b']' => return Ok(i),
            b'"' | b'\'' => {
                let q = b[i];
                let e = r[i + 1..].find(q as char).ok_or("Zeichenkette im Doctype ohne Ende")?;
                i += e + 2;
            }
            b'<' if r[i..].starts_with("<!--") => {
                let e = r[i..].find("-->").ok_or("Kommentar im Doctype ohne Ende")?;
                i += e + 3;
            }
            b'<' if r[i..].starts_with("<!ENTITY") => {
                let mut l = Leser { s: r, i: i + 8 };
                l.leer();
                if l.beginnt("%") {
                    i += 8;
                    continue;
                }
                if let Some(name) = l.name() {
                    l.leer();
                    if let Some(w) = l.zitat()
                        && eigene.len() < 1024
                        && w.len() <= 64 * 1024
                    {
                        eigene.entry(name.to_string()).or_insert_with(|| w.to_string());
                    }
                }
                let e = r[l.i..].find('>').ok_or("ENTITY ohne Ende")?;
                i = l.i + e + 1;
            }
            _ => i += 1,
        }
    }
    Err("interner Doctype-Teil ohne Ende".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namensraeume_cdata_und_fehler() {
        let (mut dok, doc) = Dokument::neu();
        xml_parsen(&mut dok, doc, "<?xml version=\"1.0\"?>\n<a xmlns=\"urn:x\" xmlns:s=\"http://www.w3.org/2000/svg\"><s:rect w=\"1\"/>x &amp; <![CDATA[<y>]]></a>").unwrap();
        let a = dok.kinder(doc)[0];
        assert_eq!(dok.fremd_ns.get(&a).map(|x| x.0.as_str()), Some("urn:x"));
        let r = dok.kinder(a)[0];
        assert!(dok.ist_element(r, Namensraum::Svg, "rect"));
        assert_eq!(dok.fremd_ns.get(&r).and_then(|x| x.1.as_deref()), Some("s"));
        assert_eq!(dok.kinder(a).len(), 3);
        assert!(dok.cdata.contains(&dok.kinder(a)[2]));
        let (mut d2, w2) = Dokument::neu();
        assert!(xml_parsen(&mut d2, w2, "<a><b></a>").is_err());
        assert!(xml_parsen(&mut d2, w2, "<a/><b/>").is_err());
    }

    #[test]
    fn qnamen_und_interner_doctype() {
        let (mut d, w) = Dokument::neu();
        assert!(xml_parsen(&mut d, w, "<span :test=\"x\">3</span>").is_err());
        let (mut d, w) = Dokument::neu();
        assert!(xml_parsen(&mut d, w, "<span xmlns:=\"urn:x\">12</span>").is_err());
        let (mut d, w) = Dokument::neu();
        assert!(xml_parsen(&mut d, w, "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\"><html/>").is_err());
        let (mut d, w) = Dokument::neu();
        xml_parsen(&mut d, w, "<!DOCTYPE foo [ <!ENTITY x \"y\"> ]><foo>&x;</foo>").unwrap();
        let foo = d.kinder(w)[1];
        assert_eq!(d.text_inhalt(foo), "y");
    }
}
