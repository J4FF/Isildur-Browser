//! Release 12.10.2026: Shadow DOM im Seitenbau (DOM §4.2.2 „shadow tree",
//! HTML §4.12.4 `<slot>`, CSS Scoping §2 „flat tree").
//!
//! GEMESSEN vorher (WPT shadow-dom 59/208, narsil-page schattenwurzel):
//! der Seitenbau las `HtmlNode::shadow_root` nirgends — der Inhalt jeder
//! Schattenwurzel (deklarativ wie per `attachShadow`) fehlte im Bild,
//! dafuer standen die Licht-Kinder da, als gaebe es keine Wurzel.
//!
//! DER WEG: vor dem Bau wird der „flache Baum" gebaut ([`zusammensetzen`]):
//! ein Wirt hat statt seiner Licht-Kinder die Kinder seiner Shadow Root,
//! ein `<slot>` traegt die ihm zugeordneten Licht-Knoten (oder seinen
//! Ersatzinhalt), nicht zugeordnete Licht-Kinder fallen weg.
//!
//! DIE NUMMERN: der Bauer vergibt Ordinalzahlen in Besuchsreihenfolge, und
//! die Bruecke (`narsil::lebend`) ordnet sie Arena-Knoten zu. Im flachen
//! Baum besucht der Bauer Licht-Knoten an anderer Stelle — deshalb haengt
//! die Nummer am KNOTEN, nicht an der Reihenfolge: [`ords_gehen`] legt
//! sie fest (Licht in Vorordnung wie bisher, danach die Shadow Trees in
//! Breitenfolge, jeder in Vorordnung), und BEIDE Seiten benutzen genau
//! diese eine Funktion.

use narsil_fmt_html::{HtmlNode, ShadowRootInfo};
use std::collections::{HashMap, VecDeque};

use crate::{MAX_BAU_TIEFE, STEUERELEMENTE};

/// Internes Attribut: der Bereich eines Elements IM Shadow Tree (und eines
/// `<style>` darin). Der Schluessel haengt am Stilinhalt des Baums, nicht an
/// der Instanz: hundert gleiche Komponenten teilen EIN umgeschriebenes Blatt.
pub const ATTR_BEREICH: &str = " narsil-bereich";
/// Internes Attribut: der Wirt traegt den Bereich seines Shadow Trees (`:host`).
pub const ATTR_WIRT: &str = " narsil-wirt";
/// Internes Attribut: ein zugeordneter Licht-Knoten traegt den Bereich des
/// Slots, in dem er steht (`::slotted()`).
pub const ATTR_SLOT: &str = " narsil-slot";

/// Hoechstzahl Shadow Trees je Seite (M3).
const MAX_SCHATTENBAEUME: usize = 100_000;
/// Hoechste Verschachtelung beim Zusammensetzen (M5).
const MAX_SETZ_TIEFE: usize = 512;

fn adresse(n: &HtmlNode) -> usize {
    n as *const HtmlNode as usize
}

/// Hat irgendein Element eine Shadow Root?
pub fn hat_schatten(liste: &[HtmlNode]) -> bool {
    fn gehen(liste: &[HtmlNode], tiefe: usize) -> bool {
        if tiefe > MAX_SETZ_TIEFE {
            return false;
        }
        liste.iter().any(|n| match n {
            HtmlNode::Element { shadow_root: Some(_), .. } => true,
            HtmlNode::Element { children, .. } => gehen(children, tiefe + 1),
            HtmlNode::Text(_) => false,
        })
    }
    gehen(liste, 0)
}

/// Die Zaehlregel des Bauers fuer ALLE Baeume einer Seite.
///
/// `melde(knoten, ord, pos)` kommt fuer jeden gezaehlten Knoten: `ord` ist
/// seine Ordinalzahl, `pos` seine Stelle in der Indexliste der Bruecke
/// (Licht in Vorordnung, danach die Shadow Trees in Breitenfolge, jeder in
/// Vorordnung; JEDER Knoten hat eine Stelle, auch ein nicht gezaehlter).
///
/// Die Regel ist die von `zaehle`: ab `MAX_BAU_TIEFE` und in Steuerelementen
/// (Text, `option`, `optgroup`) zaehlt ein Knoten nicht, und dann auch
/// nichts darunter. Ein Shadow Tree beginnt bei Tiefe 0. Wirte werden beim
/// ERSTEN Besuch eingereiht (auch in nicht gezaehlten Teilbaeumen).
pub fn ords_gehen<'a>(knoten: &'a [HtmlNode], melde: &mut dyn FnMut(&'a HtmlNode, usize, usize)) {
    let mut ord = 0usize;
    let mut pos = 0usize;
    let mut schlange: VecDeque<&'a ShadowRootInfo> = VecDeque::new();
    let mut baeume = 0usize;
    gehen(knoten, 0, false, true, &mut ord, &mut pos, &mut schlange, &mut baeume, melde);
    while let Some(sr) = schlange.pop_front() {
        gehen(&sr.children, 0, false, true, &mut ord, &mut pos, &mut schlange, &mut baeume, melde);
    }
}

#[allow(clippy::too_many_arguments)]
fn gehen<'a>(
    liste: &'a [HtmlNode],
    tiefe: usize,
    im_steuer: bool,
    erlaubt: bool,
    ord: &mut usize,
    pos: &mut usize,
    schlange: &mut VecDeque<&'a ShadowRootInfo>,
    baeume: &mut usize,
    melde: &mut dyn FnMut(&'a HtmlNode, usize, usize),
) {
    for k in liste {
        let mein_pos = *pos;
        *pos += 1;
        let steuer_aus = im_steuer
            && match k {
                HtmlNode::Text(_) => true,
                HtmlNode::Element { tag, .. } => matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup"),
            };
        let zaehlt = erlaubt && tiefe < MAX_BAU_TIEFE && !steuer_aus;
        if zaehlt {
            melde(k, *ord, mein_pos);
            *ord += 1;
        }
        if let HtmlNode::Element { tag, children, shadow_root, .. } = k {
            if let Some(sr) = shadow_root
                && *baeume < MAX_SCHATTENBAEUME
            {
                *baeume += 1;
                schlange.push_back(sr);
            }
            if tiefe > MAX_SETZ_TIEFE {
                continue;
            }
            let klein = tag.to_ascii_lowercase();
            let tiefer = im_steuer || STEUERELEMENTE.contains(&klein.as_str());
            gehen(children, tiefe + 1, tiefer, zaehlt, ord, pos, schlange, baeume, melde);
        }
    }
}

/// Der Zuordnungsstand eines Shadow Trees: Slot (Adresse) -> Licht-Knoten.
struct Kontext<'a, 'b> {
    zugeordnet: HashMap<usize, Vec<&'a HtmlNode>>,
    aussen: Option<&'b Kontext<'a, 'b>>,
    /// Der Bereich dieses Shadow Trees (siehe [`ATTR_BEREICH`]).
    bereich: String,
}

/// Der Bereichsschluessel eines Shadow Trees: der Stilinhalt seiner
/// `<style>`-Elemente und `<link>`-Adressen (ohne verschachtelte Baeume und
/// Template-Inhalte).
fn bereich_schluessel(sr: &ShadowRootInfo) -> String {
    use std::hash::{Hash, Hasher};
    fn stile(liste: &[HtmlNode], h: &mut std::collections::hash_map::DefaultHasher, tiefe: usize) {
        if tiefe > MAX_SETZ_TIEFE {
            return;
        }
        for n in liste {
            if let HtmlNode::Element { tag, children, attributes, .. } = n {
                let klein = tag.to_ascii_lowercase();
                if klein == "template" {
                    continue;
                }
                if klein == "style" {
                    for c in children {
                        if let HtmlNode::Text(t) = c {
                            t.hash(h);
                        }
                    }
                } else if klein == "link" {
                    attributes.get("href").hash(h);
                }
                stile(children, h, tiefe + 1);
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    stile(&sr.children, &mut h, 0);
    format!("b{:x}", h.finish())
}

/// Die Slots eines Shadow Trees in Baumordnung (ohne verschachtelte
/// Shadow Trees und ohne Template-Inhalte), je Name der ERSTE.
fn slots_sammeln<'a>(liste: &'a [HtmlNode], raus: &mut HashMap<String, usize>, tiefe: usize) {
    if tiefe > MAX_SETZ_TIEFE {
        return;
    }
    for n in liste {
        if let HtmlNode::Element { tag, attributes, children, .. } = n {
            let klein = tag.to_ascii_lowercase();
            if klein == "template" {
                continue;
            }
            if klein == "slot" {
                let name = attributes.get("name").cloned().unwrap_or_default();
                raus.entry(name).or_insert(adresse(n));
            }
            slots_sammeln(children, raus, tiefe + 1);
        }
    }
}

/// HTML §4.12.4 „assign slottables": jedes Licht-Kind (Element oder Text)
/// geht in den ersten Slot seines Namens (`slot`-Attribut, sonst "").
fn zuordnen<'a>(sr: &'a ShadowRootInfo, licht: &'a [HtmlNode]) -> HashMap<usize, Vec<&'a HtmlNode>> {
    let mut slots = HashMap::new();
    slots_sammeln(&sr.children, &mut slots, 0);
    let mut raus: HashMap<usize, Vec<&'a HtmlNode>> = HashMap::new();
    if slots.is_empty() {
        return raus;
    }
    for c in licht {
        let name = match c {
            HtmlNode::Element { attributes, .. } => attributes.get("slot").cloned().unwrap_or_default(),
            HtmlNode::Text(_) => String::new(),
        };
        if let Some(&s) = slots.get(&name) {
            raus.entry(s).or_default().push(c);
        }
    }
    raus
}

fn setzen<'a, 'b>(
    n: &'a HtmlNode,
    ctx: Option<&'b Kontext<'a, 'b>>,
    karte: &HashMap<usize, usize>,
    ords: &mut Vec<Option<usize>>,
    tiefe: usize,
) -> HtmlNode {
    setzen_mit(n, ctx, karte, ords, tiefe, None)
}

/// `slot_bereich`: der Bereich des Slots, in den `n` zugeordnet ist (nur fuer
/// die zugeordneten Licht-Knoten selbst, siehe [`ATTR_SLOT`]).
fn setzen_mit<'a, 'b>(
    n: &'a HtmlNode,
    ctx: Option<&'b Kontext<'a, 'b>>,
    karte: &HashMap<usize, usize>,
    ords: &mut Vec<Option<usize>>,
    tiefe: usize,
    slot_bereich: Option<&str>,
) -> HtmlNode {
    ords.push(karte.get(&adresse(n)).copied());
    match n {
        HtmlNode::Text(t) => HtmlNode::Text(t.clone()),
        HtmlNode::Element { tag, attributes, children, shadow_root } => {
            let mut attr = attributes.clone();
            if let Some(c) = ctx {
                attr.insert(ATTR_BEREICH.to_string(), c.bereich.clone());
            }
            if let Some(b) = slot_bereich {
                attr.insert(ATTR_SLOT.to_string(), b.to_string());
            }
            let kinder: Vec<HtmlNode> = if tiefe > MAX_SETZ_TIEFE {
                Vec::new()
            } else if let Some(sr) = shadow_root {
                let bereich = bereich_schluessel(sr);
                attr.insert(ATTR_WIRT.to_string(), bereich.clone());
                let neu = Kontext { zugeordnet: zuordnen(sr, children), aussen: ctx, bereich };
                sr.children.iter().map(|c| setzen(c, Some(&neu), karte, ords, tiefe + 1)).collect()
            } else if tag.eq_ignore_ascii_case("slot")
                && let Some(c) = ctx
                && let Some(liste) = c.zugeordnet.get(&adresse(n))
                && !liste.is_empty()
            {
                liste
                    .iter()
                    .map(|a| setzen_mit(a, c.aussen, karte, ords, tiefe + 1, Some(&c.bereich)))
                    .collect()
            } else {
                children.iter().map(|c| setzen(c, ctx, karte, ords, tiefe + 1)).collect()
            };
            HtmlNode::Element {
                tag: tag.clone(),
                attributes: attr,
                children: kinder,
                shadow_root: None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// STILE: die Blaetter eines Shadow Trees gelten nur dort (CSS Scoping §3)
// ---------------------------------------------------------------------------

fn attr_gleich(name: &str, wert: &str) -> narsil_css::AttrBed {
    narsil_css::AttrBed {
        name: name.to_string(),
        bedingung: Some((narsil_css::AttrOp::Gleich, wert.to_string(), Some(false))),
    }
}

fn selektoren_aendern(sheet: &mut narsil_css::Stylesheet, f: &mut dyn FnMut(&mut narsil_css::Selektor) -> bool) {
    use narsil_css::Regel;
    for r in &mut sheet.regeln {
        match r {
            Regel::Stil(s) => s.selektoren.retain_mut(|x| f(x)),
            Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => {
                for s in regeln {
                    s.selektoren.retain_mut(|x| f(x));
                }
            }
            _ => {}
        }
    }
}

/// Ein Blatt aus einem Shadow Tree mit dem Bereich `bereich`: jede Stufe
/// verlangt den Bereich, `:host(…)` wird zum Wirt-Attribut, `::slotted(…)`
/// zum Slot-Attribut (der Rest einer `::slotted`-Stufe ist der Slot selbst,
/// sein Kind ist der zugeordnete Knoten). `:host`/`::slotted` verlieren gegen
/// das Dokument (CSS Scoping §3.3 „outer context wins"): Spezifitaet 0, und
/// die Blaetter stehen VOR denen des Dokuments.
pub fn blatt_einschraenken(sheet: &mut narsil_css::Stylesheet, bereich: &str) {
    use narsil_css::{Einfach, KettenGlied, Kombinator, Pseudo};
    selektoren_aendern(sheet, &mut |sel| {
        let mut neu: Vec<KettenGlied> = Vec::with_capacity(sel.kette.len() + 1);
        let mut aussen = false;
        for g in sel.kette.drain(..) {
            let mut e = g.einfach;
            let wirt = e.pseudos.iter().position(|p| matches!(p, Pseudo::Wirt(_)));
            let slot = e.pseudos.iter().position(|p| matches!(p, Pseudo::Eingesetzt(_)));
            if let Some(i) = wirt {
                let Pseudo::Wirt(liste) = e.pseudos.remove(i) else { continue };
                e.attribute.push(attr_gleich(ATTR_WIRT, bereich));
                if !liste.is_empty() {
                    e.pseudos.push(Pseudo::Ist(liste));
                }
                aussen = true;
                neu.push(KettenGlied { komb: g.komb, einfach: e });
            } else if let Some(i) = slot {
                let Pseudo::Eingesetzt(liste) = e.pseudos.remove(i) else { continue };
                // Der Rest der Stufe (Tag, Klassen ...) gehoert dem Slot.
                let hat_rest = e.tag.is_some()
                    || e.id.is_some()
                    || !e.klassen.is_empty()
                    || !e.attribute.is_empty()
                    || !e.pseudos.is_empty();
                let mut kind = Einfach::default();
                kind.attribute.push(attr_gleich(ATTR_SLOT, bereich));
                kind.pseudos.push(Pseudo::Ist(liste));
                if hat_rest {
                    e.attribute.push(attr_gleich(ATTR_BEREICH, bereich));
                    neu.push(KettenGlied { komb: g.komb, einfach: e });
                    neu.push(KettenGlied { komb: Kombinator::Kind, einfach: kind });
                } else {
                    neu.push(KettenGlied { komb: g.komb, einfach: kind });
                }
                aussen = true;
            } else {
                e.attribute.push(attr_gleich(ATTR_BEREICH, bereich));
                neu.push(KettenGlied { komb: g.komb, einfach: e });
            }
        }
        if neu.is_empty() || neu.len() > 32 {
            return false;
        }
        sel.kette = neu;
        if aussen {
            sel.spezifitaet = (0, 0, 0);
        }
        true
    });
}

/// Ein Blatt des Dokuments auf einer Seite MIT Shadow Trees: sein Subjekt
/// darf kein Element in einem Shadow Tree sein (`:not([ narsil-bereich])`).
pub fn blatt_dokument(sheet: &mut narsil_css::Stylesheet) {
    use narsil_css::{AttrBed, Einfach, KettenGlied, Kombinator, Pseudo, Selektor};
    let nicht = Pseudo::Nicht(vec![Selektor {
        kette: vec![KettenGlied {
            komb: Kombinator::Keiner,
            einfach: Einfach {
                attribute: vec![AttrBed { name: ATTR_BEREICH.to_string(), bedingung: None }],
                ..Einfach::default()
            },
        }],
        spezifitaet: (0, 0, 0),
    }]);
    selektoren_aendern(sheet, &mut |sel| {
        if let Some(letzt) = sel.kette.last_mut() {
            letzt.einfach.pseudos.push(nicht.clone());
        }
        true
    });
}

/// Der flache Baum und die Ordinalzahl jedes seiner Knoten (Adresse ->
/// Ord). `None`, wenn die Seite keine Shadow Root hat (dann bleibt alles
/// wie bisher, ohne Kosten).
pub fn zusammensetzen(knoten: &[HtmlNode]) -> Option<(Vec<HtmlNode>, HashMap<usize, usize>)> {
    if !hat_schatten(knoten) {
        return None;
    }
    let mut karte: HashMap<usize, usize> = HashMap::new();
    ords_gehen(knoten, &mut |n, ord, _| {
        karte.insert(adresse(n), ord);
    });
    let mut ords: Vec<Option<usize>> = Vec::new();
    let flach: Vec<HtmlNode> = knoten.iter().map(|n| setzen(n, None, &karte, &mut ords, 0)).collect();
    // Die Adressen des flachen Baums (in derselben Vorordnung wie `setzen`).
    let mut neu: HashMap<usize, usize> = HashMap::with_capacity(ords.len());
    let mut i = 0usize;
    fn vorordnung(liste: &[HtmlNode], ords: &[Option<usize>], i: &mut usize, neu: &mut HashMap<usize, usize>) {
        for n in liste {
            if let Some(Some(o)) = ords.get(*i) {
                neu.insert(adresse(n), *o);
            }
            *i += 1;
            if let HtmlNode::Element { children, .. } = n {
                vorordnung(children, ords, i, neu);
            }
        }
    }
    vorordnung(&flach, &ords, &mut i, &mut neu);
    Some((flach, neu))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(tag: &str, attr: &[(&str, &str)], kinder: Vec<HtmlNode>, sr: Option<Vec<HtmlNode>>) -> HtmlNode {
        HtmlNode::Element {
            tag: tag.to_string(),
            attributes: attr.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            children: kinder,
            shadow_root: sr.map(|c| ShadowRootInfo { mode: narsil_fmt_html::ShadowRootMode::Open, children: c, merkmale: 0 }),
        }
    }
    fn t(s: &str) -> HtmlNode {
        HtmlNode::Text(s.to_string())
    }
    fn texte(liste: &[HtmlNode], raus: &mut Vec<String>) {
        for n in liste {
            match n {
                HtmlNode::Text(s) => raus.push(s.clone()),
                HtmlNode::Element { children, .. } => texte(children, raus),
            }
        }
    }

    #[test]
    fn slots_verteilen_und_nummern_bleiben() {
        // <div>[shadow: <b>A</b><slot name=x></slot><slot>ersatz</slot>]<i slot=x>X</i>licht</div><p>nach</p>
        let html = vec![
            el(
                "div",
                &[],
                vec![el("i", &[("slot", "x")], vec![t("X")], None), t("licht")],
                Some(vec![
                    el("b", &[], vec![t("A")], None),
                    el("slot", &[("name", "x")], vec![], None),
                    el("slot", &[], vec![t("ersatz")], None),
                ]),
            ),
            el("p", &[], vec![t("nach")], None),
        ];
        let (flach, karte) = zusammensetzen(&html).expect("Schatten");
        let mut tx = Vec::new();
        texte(&flach, &mut tx);
        assert_eq!(tx, vec!["A", "X", "licht", "nach"]);
        // Licht-Nummern wie ohne Schatten: div 0, i 1, X 2, licht 3, p 4, nach 5;
        // dann der Shadow Tree: b 6, A 7, slot 8, slot 9, ersatz 10.
        let HtmlNode::Element { children: div_kinder, .. } = &flach[0] else { panic!() };
        let HtmlNode::Element { children: b_kinder, .. } = &div_kinder[0] else { panic!() };
        assert_eq!(karte.get(&adresse(&flach[0])), Some(&0));
        assert_eq!(karte.get(&adresse(&div_kinder[0])), Some(&6));
        assert_eq!(karte.get(&adresse(&b_kinder[0])), Some(&7));
        let HtmlNode::Element { children: slot1, .. } = &div_kinder[1] else { panic!() };
        assert_eq!(karte.get(&adresse(&slot1[0])), Some(&1));
        let HtmlNode::Element { children: slot2, .. } = &div_kinder[2] else { panic!() };
        assert_eq!(karte.get(&adresse(&slot2[0])), Some(&3));
        assert_eq!(karte.get(&adresse(&flach[1])), Some(&4));
    }

    #[test]
    fn ohne_schatten_kein_umbau() {
        let html = vec![el("p", &[], vec![t("x")], None)];
        assert!(zusammensetzen(&html).is_none());
    }
}
