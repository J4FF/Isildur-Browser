//! Release 12.10.2026 (Agent C): die Reihenfolge der Kaskadenschichten
//! (CSS Cascade 5 §6.4.3) als Baum: eine Schicht steht HINTER all ihren
//! Unterschichten (ihre eigenen Regeln gewinnen gegen `A.B`), Geschwister
//! in der Reihenfolge ihres ersten Auftretens. UA (Blatt 0) und Autor
//! zaehlen getrennt, wie in der Kaskade.
//!
//! GEMESSEN vorher (WPT css-cascade layer-basic A5/B5/B10/C3/C5/D4/E5,
//! layer-important D1–E2): die Nummern entstanden nach dem ersten Auftreten,
//! und `@layer A { x {} @layer B { x {} } }` liess `A.B` gewinnen.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::typen::{Regel, Stylesheet};

/// Hoechstens so viele Schichten je Herkunft (M3; unter `u16::MAX`, das
/// „ungeschichtet" heisst).
const MAX_SCHICHTEN: usize = 60_000;

type Karte = HashMap<(usize, usize), u16>;

thread_local! {
    /// (Adresse, Laenge, Regelzahl des Blattsatzes) und die Nummern je
    /// `(Blatt, Regel)` einer `Regel::Layer`.
    static ORDNUNG: RefCell<((usize, usize, usize), std::rc::Rc<Karte>)> =
        RefCell::new(((0, 0, 0), std::rc::Rc::new(HashMap::new())));
}

struct Knoten {
    kinder: Vec<usize>,
}

#[derive(Default)]
struct Baum {
    knoten: Vec<Knoten>,
    oben: Vec<usize>,
    namen: HashMap<String, usize>,
    stellen: Vec<((usize, usize), usize)>,
}

impl Baum {
    fn neu(&mut self, eltern: Option<usize>) -> Option<usize> {
        if self.knoten.len() >= MAX_SCHICHTEN {
            return None;
        }
        let i = self.knoten.len();
        self.knoten.push(Knoten { kinder: Vec::new() });
        match eltern {
            Some(e) => self.knoten[e].kinder.push(i),
            None => self.oben.push(i),
        }
        Some(i)
    }

    fn anmelden(&mut self, stelle: (usize, usize), name: Option<&str>) {
        let k = match name {
            None => self.neu(None),
            Some(n) => {
                let mut eltern: Option<usize> = None;
                let mut pfad = String::new();
                for teil in n.split('.') {
                    if !pfad.is_empty() {
                        pfad.push('.');
                    }
                    pfad.push_str(teil.trim());
                    let k = match self.namen.get(&pfad) {
                        Some(&k) => k,
                        None => {
                            let Some(k) = self.neu(eltern) else { return };
                            self.namen.insert(pfad.clone(), k);
                            k
                        }
                    };
                    eltern = Some(k);
                }
                eltern
            }
        };
        if let Some(k) = k {
            self.stellen.push((stelle, k));
        }
    }

    /// Nachordnung: erst die Unterschichten, dann die Schicht selbst. Returns
    /// the number of every node.
    fn nummern(&self, karte: &mut Karte) -> Vec<u16> {
        let mut nr = vec![0u16; self.knoten.len()];
        let mut zaehler: u16 = 0;
        let mut stapel: Vec<(usize, bool)> = self.oben.iter().rev().map(|&k| (k, false)).collect();
        while let Some((k, fertig)) = stapel.pop() {
            if fertig {
                nr[k] = zaehler;
                zaehler = zaehler.saturating_add(1);
                continue;
            }
            stapel.push((k, true));
            for &c in self.knoten[k].kinder.iter().rev() {
                stapel.push((c, false));
            }
        }
        for (stelle, k) in &self.stellen {
            karte.insert(*stelle, nr[*k]);
        }
        nr
    }
}

/// Release 14.10.2026: prefix of the front layers that carry `@property`
/// initial values per cascade layer (`crate::parse::property_layers`).
const PROPERTY_PREFIX: &str = "\u{1}@property/";

fn berechnen(blaetter: &[Stylesheet]) -> Karte {
    let mut ua = Baum::default();
    let mut autor = Baum::default();
    // Release 14.10.2026 (Agent C): the `@property` front layers of the
    // author sheets are not layers of their own: they come before every
    // author layer, ordered like the layers they belong to, the unlayered
    // registrations last. MEASURED before (WPT css-cascade
    // layer-cssom-order-reverse-at-property): their order followed only the
    // sheet that held them.
    let mut fronts: Vec<((usize, usize), String)> = Vec::new();
    for (bi, blatt) in blaetter.iter().take(crate::MAX_BLAETTER).enumerate() {
        let baum = if bi == 0 { &mut ua } else { &mut autor };
        for (ri, regel) in blatt.regeln.iter().enumerate() {
            if let Regel::Layer { name, .. } = regel {
                if bi > 0
                    && let Some(rest) = name.as_deref().and_then(|n| n.strip_prefix(PROPERTY_PREFIX))
                {
                    fronts.push(((bi, ri), rest.replace('\u{3}', ".")));
                    continue;
                }
                baum.anmelden((bi, ri), name.as_deref());
            }
        }
    }
    let mut karte = HashMap::new();
    ua.nummern(&mut karte);
    let mut autor_karte = HashMap::new();
    let nr = autor.nummern(&mut autor_karte);
    if fronts.is_empty() {
        karte.extend(autor_karte);
        return karte;
    }
    // Sort keys: (group, number): 0 = a known layer by its number, 1 = an
    // unknown layer by first appearance, 2 = unlayered.
    let mut keys: Vec<(u8, usize, String)> = Vec::new();
    for (_, name) in &fronts {
        if keys.iter().any(|(_, _, n)| n == name) {
            continue;
        }
        let key = if name == "\u{4}" {
            (2, 0)
        } else if let Some(&k) = autor.namen.get(name) {
            (0, usize::from(nr[k]))
        } else {
            (1, keys.len())
        };
        keys.push((key.0, key.1, name.clone()));
    }
    keys.sort();
    let shift = u16::try_from(keys.len()).unwrap_or(u16::MAX);
    for (stelle, n) in autor_karte {
        karte.insert(stelle, n.saturating_add(shift).min(u16::MAX - 1));
    }
    for (stelle, name) in fronts {
        if let Some(i) = keys.iter().position(|(_, _, n)| *n == name) {
            karte.insert(stelle, u16::try_from(i).unwrap_or(0));
        }
    }
    karte
}

/// Die Schichtnummer der `Regel::Layer` an `(bi, ri)`; `None` = unbekannt
/// (dann gilt die alte Zaehlung).
pub(crate) fn nummer(blaetter: &[Stylesheet], bi: usize, ri: usize) -> Option<u16> {
    let regeln: usize = blaetter.iter().map(|b| b.regeln.len()).sum();
    let schluessel = (blaetter.as_ptr() as usize, blaetter.len(), regeln);
    let karte = ORDNUNG.with(|o| {
        let o = o.borrow();
        (o.0 == schluessel).then(|| o.1.clone())
    });
    let karte = match karte {
        Some(k) => k,
        None => {
            let k = std::rc::Rc::new(berechnen(blaetter));
            ORDNUNG.with(|o| *o.borrow_mut() = (schluessel, k.clone()));
            k
        }
    };
    karte.get(&(bi, ri)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterschichten_zuerst() {
        let ua = crate::stylesheet_lesen("");
        let b = crate::stylesheet_lesen("@layer A { x { color: green } @layer B { x { color: red } } } @layer C { y { color: blue } }");
        let blaetter = vec![ua, b];
        let mut nr: Vec<(Option<String>, u16)> = Vec::new();
        for (ri, r) in blaetter[1].regeln.iter().enumerate() {
            if let Regel::Layer { name, .. } = r {
                nr.push((name.clone(), nummer(&blaetter, 1, ri).unwrap()));
            }
        }
        let von = |n: &str| nr.iter().find(|(m, _)| m.as_deref() == Some(n)).map(|x| x.1).unwrap();
        assert!(von("A.B") < von("A"), "{nr:?}");
        assert!(von("A") < von("C"), "{nr:?}");
    }
}
