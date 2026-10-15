//! Release 09.10.2026: das LEBENDE DOM — Operationen auf dem Arena-Baum
//! des Spec-Parsers ([`crate::dom::Dokument`]), die Skripte brauchen.
//!
//! # WARUM
//!
//! Seitenskripte sahen bisher ein Proxy-DOM aus flachen Tabellen; neue
//! Teilbaeume (`innerHTML =`, `cloneNode`, `DocumentFragment`, Kommentare)
//! kamen nie auf der Seite an, Geschwister (`nextSibling`) gab es nicht.
//! Der Arena-Baum des Parsers kann alles tragen (Kommentare, Doctype,
//! Template-Inhalt, Shadow Root) — er ist das lebende DOM, und die Seite
//! wird aus ihm gebaut ([`in_htmlnode_mit_indizes`]).
//!
//! # GRENZEN (M3/M4)
//!
//! Knotenzahl: [`crate::dom::MAX_KNOTEN`] — Operationen, die mehr erzeugen
//! wuerden, legen nichts an und melden `None`/`Err`. Tiefe beim Serialisieren
//! und Klonen: [`MAX_TIEFE_LEBEND`]. Ausgabe: [`MAX_SERIALISIERT`] Byte.

use crate::dom::{Art, Attribut, Dokument, Namensraum, ShadowRoot, ShadowRootMode, MAX_KNOTEN};
use crate::{HtmlNode, ShadowRootInfo};
use std::collections::HashMap;

/// Hoechste Tiefe beim Klonen, Serialisieren und Importieren.
pub const MAX_TIEFE_LEBEND: usize = 512;
/// Hoechstens so viele Byte liefert ein `innerHTML`/`outerHTML`.
pub const MAX_SERIALISIERT: usize = 32 * 1024 * 1024;

/// Elemente ohne Inhalt und Endtag (HTML §13.1.2).
pub const LEERE_ELEMENTE: [&str; 18] = [
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// Elemente, deren Text roh serialisiert wird (HTML §13.3).
const ROHTEXT: [&str; 7] = ["style", "script", "xmp", "iframe", "noembed", "noframes", "plaintext"];

impl Dokument {
    /// Gibt es den Knoten?
    #[must_use]
    pub fn gibt_es(&self, i: usize) -> bool {
        i < self.knoten.len()
    }

    /// Das naechste Geschwister (`nextSibling`).
    #[must_use]
    pub fn naechstes_geschwister(&self, i: usize) -> Option<usize> {
        let e = self.knoten.get(i)?.eltern?;
        let kinder = &self.knoten[e].kinder;
        let pos = kinder.iter().position(|&k| k == i)?;
        kinder.get(pos + 1).copied()
    }

    /// Das vorige Geschwister (`previousSibling`).
    #[must_use]
    pub fn voriges_geschwister(&self, i: usize) -> Option<usize> {
        let e = self.knoten.get(i)?.eltern?;
        let kinder = &self.knoten[e].kinder;
        let pos = kinder.iter().position(|&k| k == i)?;
        pos.checked_sub(1).and_then(|p| kinder.get(p).copied())
    }

    /// Ist der Knoten ein Element?
    #[must_use]
    pub fn ist_element_knoten(&self, i: usize) -> bool {
        matches!(self.knoten.get(i).map(|k| &k.art), Some(Art::Element { .. }))
    }

    /// Lokaler Name eines Elements (klein fuer HTML).
    #[must_use]
    pub fn element_name(&self, i: usize) -> Option<&str> {
        match &self.knoten.get(i)?.art {
            Art::Element { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    /// DOM-`nodeType` (1 Element, 3 Text, 8 Kommentar, 9 Dokument,
    /// 10 Doctype, 11 Fragment/Shadow Root, 7 PI).
    #[must_use]
    pub fn knoten_typ(&self, i: usize) -> u32 {
        match self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Element { .. }) => 1,
            Some(Art::Text(_)) if self.cdata.contains(&i) => 4,
            Some(Art::Text(_)) => 3,
            Some(Art::Pi { .. }) => 7,
            Some(Art::Kommentar(_)) => 8,
            Some(Art::Dokument) => 9,
            Some(Art::Doctype { .. }) => 10,
            Some(Art::Fragment) | Some(Art::ShadowRoot { .. }) => 11,
            None => 0,
        }
    }

    /// Die Wurzel des Baums, in dem `i` haengt (`getRootNode()`): der oberste
    /// Vorfahr — ein Dokument, ein Fragment, eine Shadow Root oder `i` selbst.
    #[must_use]
    pub fn wurzel_von(&self, i: usize) -> usize {
        let mut k = i;
        let mut schritte = 0;
        // Eine Shadow Root ist die Wurzel ihres Baums (DOM §4.8), auch wenn
        // sie intern ihren Wirt als Eltern traegt.
        while !matches!(self.knoten.get(k).map(|n| &n.art), Some(Art::ShadowRoot { .. }))
            && let Some(e) = self.knoten.get(k).and_then(|n| n.eltern)
        {
            k = e;
            schritte += 1;
            if schritte > MAX_KNOTEN {
                break;
            }
        }
        k
    }

    /// Ist `b` gleich `a` oder ein Nachfahre von `a` (`a.contains(b)`)?
    #[must_use]
    pub fn enthaelt(&self, a: usize, b: usize) -> bool {
        let mut k = Some(b);
        let mut schritte = 0;
        while let Some(x) = k {
            if x == a {
                return true;
            }
            // `contains` geht nicht ueber die Shadow-Grenze.
            if matches!(self.knoten.get(x).map(|n| &n.art), Some(Art::ShadowRoot { .. })) {
                return false;
            }
            k = self.knoten.get(x).and_then(|n| n.eltern);
            schritte += 1;
            if schritte > MAX_KNOTEN {
                return false;
            }
        }
        false
    }

    /// Neues Element (HTML-Namensraum, Name klein).
    pub fn neues_element(&mut self, name: &str) -> Option<usize> {
        self.neues_element_ns(Namensraum::Html, &name.to_ascii_lowercase())
    }

    /// Neues Element in einem Namensraum (`createElementNS`).
    pub fn neues_element_ns(&mut self, raum: Namensraum, name: &str) -> Option<usize> {
        if self.knoten.len() >= MAX_KNOTEN {
            return None;
        }
        let i = self.neuer_knoten(Art::Element {
            raum,
            name: name.to_string(),
            attribute: Vec::new(),
            template_inhalt: None,
            shadow_root: None,
        });
        if raum == Namensraum::Html && name == "template" {
            let inhalt = self.neuer_knoten(Art::Fragment);
            if let Art::Element { template_inhalt, .. } = &mut self.knoten[i].art {
                *template_inhalt = Some(inhalt);
            }
        }
        Some(i)
    }

    /// Neuer Textknoten.
    pub fn neuer_text(&mut self, text: &str) -> Option<usize> {
        (self.knoten.len() < MAX_KNOTEN).then(|| self.neuer_knoten(Art::Text(text.to_string())))
    }

    /// Neuer Kommentar.
    pub fn neuer_kommentar(&mut self, text: &str) -> Option<usize> {
        (self.knoten.len() < MAX_KNOTEN).then(|| self.neuer_knoten(Art::Kommentar(text.to_string())))
    }

    /// Neues `DocumentFragment`.
    pub fn neues_fragment(&mut self) -> Option<usize> {
        (self.knoten.len() < MAX_KNOTEN).then(|| self.neuer_knoten(Art::Fragment))
    }

    /// Der Inhalt eines `<template>` (`template.content`).
    #[must_use]
    pub fn template_inhalt(&self, i: usize) -> Option<usize> {
        match &self.knoten.get(i)?.art {
            Art::Element { template_inhalt, .. } => *template_inhalt,
            _ => None,
        }
    }

    /// Die Shadow Root eines Wirts (nur `open`, wie `element.shadowRoot`).
    #[must_use]
    pub fn shadow_wurzel(&self, wirt: usize) -> Option<usize> {
        let wurzel = self.shadow_wurzel_intern(wirt)?;
        match &self.knoten[wurzel].art {
            Art::ShadowRoot { mode: ShadowRootMode::Open, .. } => Some(wurzel),
            _ => None,
        }
    }

    /// Die Shadow Root eines Wirts, auch `closed` (fuer Custom Elements).
    pub fn shadow_wurzel_intern(&self, wirt: usize) -> Option<usize> {
        // Die Shadow Root ist ein eigener Knoten (`Art::ShadowRoot`), dessen
        // Eltern der Wirt ist, der aber NICHT in dessen `kinder` steht.
        // Release 09.10.2026: ohne Merker am Wirt gibt es keine Shadow Root —
        // sonst kostete jedes `el.shadowRoot` einen Lauf ueber die Arena.
        if !matches!(self.knoten.get(wirt).map(|n| &n.art), Some(Art::Element { shadow_root: Some(_), .. })) {
            return None;
        }
        self.knoten.iter().enumerate().find_map(|(i, k)| {
            (matches!(k.art, Art::ShadowRoot { .. }) && k.eltern == Some(wirt)).then_some(i)
        })
    }

    /// `attachShadow({mode})`: legt die Shadow Root an (hoechstens eine).
    pub fn shadow_anhaengen(&mut self, wirt: usize, offen: bool) -> Option<usize> {
        if !self.ist_element_knoten(wirt) || self.shadow_wurzel_intern(wirt).is_some() {
            return None;
        }
        if self.knoten.len() >= MAX_KNOTEN {
            return None;
        }
        let mode = if offen { ShadowRootMode::Open } else { ShadowRootMode::Closed };
        let w = self.neuer_knoten(Art::ShadowRoot { mode, children: Vec::new() });
        self.knoten[w].eltern = Some(wirt);
        self.tree_changed();
        if let Art::Element { shadow_root, .. } = &mut self.knoten[wirt].art {
            *shadow_root = Some(ShadowRoot { mode, children: Vec::new(), merkmale: 0 });
        }
        Some(w)
    }

    /// Release 12.10.2026: die Merkmale der Shadow Root eines Wirts (siehe
    /// [`crate::dom::SCHATTEN_KLONBAR`] u. a.; 0 ohne Wurzel).
    #[must_use]
    pub fn schatten_merkmale(&self, wirt: usize) -> u8 {
        match self.knoten.get(wirt).map(|n| &n.art) {
            Some(Art::Element { shadow_root: Some(sr), .. }) => sr.merkmale,
            _ => 0,
        }
    }

    /// Release 12.10.2026: die Merkmale setzen (nur an einem Wirt).
    pub fn schatten_merkmale_setzen(&mut self, wirt: usize, merkmale: u8) {
        if let Some(Art::Element { shadow_root: Some(sr), .. }) = self.knoten.get_mut(wirt).map(|n| &mut n.art) {
            sr.merkmale = merkmale;
        }
    }

    /// Haengt `kind` an `eltern` (`appendChild`). Ein Fragment gibt seine
    /// Kinder ab (DOM §4.2.3). `false`, wenn es ein Zyklus waere.
    pub fn kind_anhaengen(&mut self, eltern: usize, kind: usize) -> bool {
        self.kind_einfuegen(eltern, kind, None)
    }

    /// Fuegt `kind` vor `vor` ein (`insertBefore`; `None` = am Ende).
    ///
    /// Release 12.10.2026: mit den Schritten fuer lebende Bereiche (DOM
    /// §4.2.3 „insert"): erst gehen die Kinder eines Fragments, dann
    /// verschieben sich die Punkte hinter `vor` um die Zahl der Knoten —
    /// VOR dem Abtrennen der Knoten aus ihren alten Eltern (Schritt 7.1).
    pub fn kind_einfuegen(&mut self, eltern: usize, kind: usize, vor: Option<usize>) -> bool {
        if !self.gibt_es(eltern) || !self.gibt_es(kind) || self.enthaelt(kind, eltern) {
            return false;
        }
        let vor = vor.filter(|v| self.knoten.get(*v).and_then(|n| n.eltern) == Some(eltern));
        let kinder: Vec<usize> = if matches!(self.knoten[kind].art, Art::Fragment | Art::ShadowRoot { .. }) {
            self.bereiche_alle_kinder_weg(kind);
            let kinder = std::mem::take(&mut self.knoten[kind].kinder);
            self.tree_changed();
            // Release 12.10.2026: EIN Satz am Fragment (DOM §4.2.3 insert 4.2).
            if self.journal.is_some() && !kinder.is_empty() {
                self.protokoll(crate::dom::Aenderung::KinderSatz { eltern: kind, hinzu: Vec::new(), weg: kinder.clone(), vorher: None, nachher: None });
            }
            for &k in &kinder {
                self.knoten[k].eltern = None;
            }
            kinder
        } else {
            vec![kind]
        };
        // Release 12.10.2026: EIN Satz fuer alle eingefuegten Knoten (DOM
        // §4.2.3 insert Schritt 7.8); das Umhaengen aus dem alten Eltern hat
        // eigene Saetze.
        let vorher_geschwister = match vor {
            Some(v) => self.voriges_geschwister(v),
            None => self.knoten[eltern].kinder.last().copied(),
        };
        let gesammelt = kinder.len() > 1 && self.sammeln_an(eltern, vorher_geschwister, vor, false);
        if let Some(v) = vor
            && !kinder.is_empty()
            && !self.bereiche.ist_leer()
        {
            let index = self.stelle_vor(eltern, v) as u32;
            self.bereiche_vor_einfuegen(eltern, index, kinder.len() as u32);
        }
        for k in kinder {
            match vor {
                Some(v) => self.einfuegen_vor(eltern, k, v),
                None => self.anhaengen(eltern, k),
            }
        }
        if gesammelt {
            self.sammeln_aus();
        }
        true
    }

    /// Entfernt alle Kinder.
    pub fn kinder_leeren(&mut self, i: usize) {
        // Release 12.10.2026: lebende Bereiche (DOM §4.2.3 „replace all").
        self.bereiche_alle_kinder_weg(i);
        let kinder = std::mem::take(&mut self.knoten[i].kinder);
        self.tree_changed();
        for (n, &k) in kinder.iter().enumerate() {
            self.knoten[k].eltern = None;
            if self.journal.is_some() {
                // Alle gehen auf einmal: die Geschwister sind die der Liste.
                let vorher = n.checked_sub(1).map(|j| kinder[j]);
                let nachher = kinder.get(n + 1).copied();
                self.protokoll(crate::dom::Aenderung::Kinder { eltern: i, knoten: k, hinzu: false, vorher, nachher });
            }
        }
    }

    /// `textContent` lesen: Text/Kommentar = eigener Inhalt, Element und
    /// Fragment = aller Text der Nachfahren.
    #[must_use]
    pub fn text_inhalt(&self, i: usize) -> String {
        match self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Text(t)) | Some(Art::Kommentar(t)) => t.clone(),
            Some(Art::Pi { daten, .. }) => daten.clone(),
            Some(Art::Dokument) | Some(Art::Doctype { .. }) | None => String::new(),
            _ => {
                let mut raus = String::new();
                self.text_sammeln(i, &mut raus, 0);
                raus
            }
        }
    }

    fn text_sammeln(&self, i: usize, raus: &mut String, tiefe: usize) {
        if tiefe > MAX_TIEFE_LEBEND || raus.len() > MAX_SERIALISIERT {
            return;
        }
        for &k in &self.knoten[i].kinder {
            match &self.knoten[k].art {
                Art::Text(t) => raus.push_str(t),
                Art::Element { .. } | Art::Fragment => self.text_sammeln(k, raus, tiefe + 1),
                _ => {}
            }
        }
    }

    /// `textContent` setzen: Element/Fragment bekommen genau einen Text.
    pub fn text_inhalt_setzen(&mut self, i: usize, text: &str) {
        if self.journal.is_some()
            && let Art::Text(t) | Art::Kommentar(t) = &self.knoten[i].art
        {
            let alt = t.clone();
            self.protokoll(crate::dom::Aenderung::Daten { knoten: i, alt });
        }
        // Release 12.10.2026: „replace data" mit Versatz 0 ueber die ganze
        // Laenge (DOM §4.10) — jeder Grenzpunkt im Knoten geht auf 0.
        if matches!(self.knoten[i].art, Art::Text(_) | Art::Kommentar(_) | Art::Pi { .. }) {
            self.bereiche_daten(i, 0, u32::MAX, 0);
        }
        match &mut self.knoten[i].art {
            Art::Text(t) | Art::Kommentar(t) => *t = text.to_string(),
            Art::Pi { daten, .. } => *daten = text.to_string(),
            Art::Element { .. } | Art::Fragment | Art::ShadowRoot { .. } => {
                // Release 12.10.2026: „replace all" ist EIN Datensatz.
                let gesammelt = self.sammeln_an(i, None, None, true);
                self.kinder_leeren(i);
                if !text.is_empty()
                    && let Some(t) = self.neuer_text(text)
                {
                    self.anhaengen(i, t);
                }
                if gesammelt {
                    self.sammeln_aus();
                }
            }
            _ => {}
        }
    }

    /// Klont einen Knoten (`cloneNode(tief)`). Template-Inhalte werden mit
    /// kopiert, Shadow Roots nicht (DOM §4.5).
    pub fn klonen(&mut self, i: usize, tief: bool) -> Option<usize> {
        let journal = self.journal.take();
        let r = self.klonen_tiefe(i, tief, 0);
        self.journal = journal;
        r
    }

    fn klonen_tiefe(&mut self, i: usize, tief: bool, tiefe: usize) -> Option<usize> {
        if tiefe > MAX_TIEFE_LEBEND || self.knoten.len() >= MAX_KNOTEN || !self.gibt_es(i) {
            return None;
        }
        let art = match &self.knoten[i].art {
            Art::Element { raum, name, attribute, .. } => Art::Element {
                raum: *raum,
                name: name.clone(),
                attribute: attribute.clone(),
                template_inhalt: None,
                shadow_root: None,
            },
            Art::ShadowRoot { .. } => Art::Fragment,
            andere => andere.clone(),
        };
        let neu = self.neuer_knoten(art);
        // Release 11.10.2026: Namensraum, Praefix und CDATA wandern mit.
        if let Some(ns) = self.fremd_ns.get(&i).cloned() {
            self.fremd_ns.insert(neu, ns);
        }
        if self.cdata.contains(&i) {
            self.cdata.insert(neu);
        }
        if let Some(t) = self.xml_typ.get(&i).cloned() {
            self.xml_typ.insert(neu, t);
        }
        // Release 11.10.2026: der Haken-Zustand (HTML „cloning steps" von input).
        let haken = self.css_zustand_von(i) & crate::dom::HAKEN_BITS;
        if haken != 0 {
            self.css_zustand_setzen(neu, haken);
        }
        if let Some(inhalt) = self.template_inhalt(i) {
            let kopie = self.klonen_tiefe(inhalt, true, tiefe + 1)?;
            if let Art::Element { template_inhalt, .. } = &mut self.knoten[neu].art {
                *template_inhalt = Some(kopie);
            }
        }
        if tief {
            let kinder = self.knoten[i].kinder.clone();
            for k in kinder {
                if let Some(c) = self.klonen_tiefe(k, true, tiefe + 1) {
                    self.anhaengen(neu, c);
                }
            }
        }
        Some(neu)
    }

    /// Kopiert einen Knoten aus einem ANDEREN Dokument hierher
    /// (`importNode`; auch der Weg von `innerHTML =`).
    pub fn importieren(&mut self, fremd: &Dokument, i: usize) -> Option<usize> {
        // Release 09.10.2026: der Aufbau der Kopie (lose Knoten) gehoert
        // nicht ins Aenderungsprotokoll — erst ihr Einhaengen.
        let journal = self.journal.take();
        let r = self.importieren_tiefe(fremd, i, 0);
        self.journal = journal;
        r
    }

    fn importieren_tiefe(&mut self, fremd: &Dokument, i: usize, tiefe: usize) -> Option<usize> {
        if tiefe > MAX_TIEFE_LEBEND || self.knoten.len() >= MAX_KNOTEN {
            return None;
        }
        let quelle = fremd.knoten.get(i)?;
        let art = match &quelle.art {
            Art::Element { raum, name, attribute, .. } => Art::Element {
                raum: *raum,
                name: name.clone(),
                attribute: attribute.clone(),
                template_inhalt: None,
                shadow_root: None,
            },
            Art::ShadowRoot { .. } | Art::Dokument => Art::Fragment,
            andere => andere.clone(),
        };
        let neu = self.neuer_knoten(art);
        // Release 12.10.2026: eine deklarative Shadow Root des Fragments
        // (`setHTMLUnsafe`) kommt mit ihren Merkmalen mit.
        if let Some(sr) = fremd.shadow_wurzel_intern(i) {
            let offen = matches!(fremd.knoten[sr].art, Art::ShadowRoot { mode: ShadowRootMode::Open, .. });
            let merkmale = fremd.schatten_merkmale(i);
            if let Some(w) = self.shadow_anhaengen(neu, offen) {
                self.schatten_merkmale_setzen(neu, merkmale);
                for &k in &fremd.knoten[sr].kinder {
                    if let Some(c) = self.importieren_tiefe(fremd, k, tiefe + 1) {
                        self.anhaengen(w, c);
                    }
                }
            }
        }
        if let Some(inhalt) = fremd.template_inhalt(i) {
            if let Some(kopie) = self.importieren_tiefe(fremd, inhalt, tiefe + 1)
                && let Art::Element { template_inhalt, .. } = &mut self.knoten[neu].art
            {
                *template_inhalt = Some(kopie);
            }
        }
        for &k in &quelle.kinder {
            if let Some(c) = self.importieren_tiefe(fremd, k, tiefe + 1) {
                self.anhaengen(neu, c);
            }
        }
        Some(neu)
    }

    /// `innerHTML =`: parst `html` als Fragment im Kontext des Elements und
    /// ersetzt die Kinder (bei `<template>` den Inhalt).
    pub fn inner_html_setzen(&mut self, i: usize, html: &str) -> bool {
        self.inner_html_setzen_mit(i, html, false)
    }

    /// Release 13.10.2026: `document.write` — `html` im Kontext von `i`
    /// parsen und die Knoten AN DAS ENDE von `i` haengen (ohne die
    /// vorhandenen Kinder zu leeren). Liefert die neuen obersten Knoten.
    pub fn html_anhaengen(&mut self, i: usize, html: &str) -> Vec<usize> {
        let kontext = match &self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Element { raum: Namensraum::Svg, .. }) => "svg".to_string(),
            Some(Art::Element { raum: Namensraum::MathMl, .. }) => "math".to_string(),
            Some(Art::Element { name, .. }) => name.clone(),
            _ => return Vec::new(),
        };
        let Ok((fremd, wurzel)) = crate::treebuilder::parse_fragment_dsd(html, &kontext, true, false) else {
            return Vec::new();
        };
        let mut neu = Vec::new();
        let kinder = fremd.knoten[wurzel].kinder.clone();
        for k in kinder {
            if let Some(c) = self.importieren(&fremd, k) {
                if matches!(fremd.knoten[k].art, Art::Element { ref name, .. } if name == "html")
                    && fremd.knoten[wurzel].kinder.len() == 1
                {
                    let enkel = self.knoten[c].kinder.clone();
                    for e in enkel {
                        self.anhaengen(i, e);
                        neu.push(e);
                    }
                } else {
                    self.anhaengen(i, c);
                    neu.push(c);
                }
            }
        }
        neu
    }

    /// Release 12.10.2026: `setHTMLUnsafe` (`dsd`: deklarative Shadow Roots
    /// erlaubt, HTML §8.5).
    pub fn inner_html_setzen_mit(&mut self, i: usize, html: &str, dsd: bool) -> bool {
        let kontext = match &self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Element { raum: Namensraum::Svg, .. }) => "svg".to_string(),
            Some(Art::Element { raum: Namensraum::MathMl, .. }) => "math".to_string(),
            // Release 13.10.2026: ein Element in einem fremden Namensraum ist
            // kein HTML-Sonderkontext (`html`, `title`, `textarea` …).
            // GEMESSEN vorher (WPT createContextualFragment): ein `html` im
            // Namensraum `http://fake-namespace` erzeugte `<head>`/`<body>`.
            Some(Art::Element { raum: Namensraum::Fremd, .. }) => "div".to_string(),
            Some(Art::Element { name, .. }) => name.clone(),
            Some(Art::Fragment) | Some(Art::ShadowRoot { .. }) => "body".to_string(),
            _ => return false,
        };
        let ziel = self.template_inhalt(i).unwrap_or(i);
        let Ok((fremd, wurzel)) = crate::treebuilder::parse_fragment_dsd(html, &kontext, true, dsd) else {
            return false;
        };
        // Release 12.10.2026: „replace all" ist EIN Datensatz (DOM §4.2.3).
        let gesammelt = self.sammeln_an(ziel, None, None, true);
        self.kinder_leeren(ziel);
        let kinder = fremd.knoten[wurzel].kinder.clone();
        for k in kinder {
            // Der Fragment-Parser haengt die Kinder an ein `html`-Element.
            if let Some(c) = self.importieren(&fremd, k) {
                if matches!(fremd.knoten[k].art, Art::Element { ref name, .. } if name == "html")
                    && fremd.knoten[wurzel].kinder.len() == 1
                {
                    let enkel = self.knoten[c].kinder.clone();
                    for e in enkel {
                        self.anhaengen(ziel, e);
                    }
                } else {
                    self.anhaengen(ziel, c);
                }
            }
        }
        if gesammelt {
            self.sammeln_aus();
        }
        true
    }

    /// `innerHTML` lesen (HTML-Fragment-Serialisierung, HTML §13.3).
    #[must_use]
    pub fn inner_html(&self, i: usize) -> String {
        let mut raus = String::new();
        let quelle = self.template_inhalt(i).unwrap_or(i);
        let roh = self.element_name(i).is_some_and(|n| ROHTEXT.contains(&n) || n == "noscript");
        for &k in &self.knoten[quelle].kinder {
            self.serialisiere_knoten(k, roh, &mut raus, 0, None);
        }
        raus
    }

    /// Release 12.10.2026: `getHTML({serializableShadowRoots, shadowRoots})`
    /// (HTML §13.3 „serialize an HTML fragment" mit Shadow Roots): wie
    /// `innerHTML`, dazu jede Shadow Root als `<template shadowrootmode>`, die
    /// `serializable` ist (wenn `serialisierbar`) oder in `wurzeln` steht.
    #[must_use]
    pub fn get_html(&self, i: usize, serialisierbar: bool, wurzeln: &[usize]) -> String {
        let wahl = SchattenWahl { serialisierbar, wurzeln };
        let mut raus = String::new();
        // Ist der Knoten selbst ein Wirt, kommt seine Shadow Root zuerst.
        self.schatten_vorlage(i, &mut raus, 0, &wahl);
        let quelle = self.template_inhalt(i).unwrap_or(i);
        let roh = self.element_name(i).is_some_and(|n| ROHTEXT.contains(&n) || n == "noscript");
        for &k in &self.knoten[quelle].kinder {
            self.serialisiere_knoten(k, roh, &mut raus, 0, Some(&wahl));
        }
        raus
    }

    /// Die Shadow Root des Wirts `i` als `<template shadowrootmode=…>`, wenn
    /// `wahl` sie nennt (HTML §13.3 „serialize an HTML fragment" Schritt 3).
    fn schatten_vorlage(&self, i: usize, raus: &mut String, tiefe: usize, w: &SchattenWahl<'_>) {
        let Some(sr) = self.shadow_wurzel_intern(i) else { return };
        let m = self.schatten_merkmale(i);
        if !((w.serialisierbar && m & crate::dom::SCHATTEN_SERIALISIERBAR != 0) || w.wurzeln.contains(&sr)) {
            return;
        }
        let offen = matches!(self.knoten[sr].art, Art::ShadowRoot { mode: ShadowRootMode::Open, .. });
        raus.push_str(if offen { "<template shadowrootmode=\"open\"" } else { "<template shadowrootmode=\"closed\"" });
        if m & crate::dom::SCHATTEN_DELEGIERT != 0 {
            raus.push_str(" shadowrootdelegatesfocus=\"\"");
        }
        if m & crate::dom::SCHATTEN_SERIALISIERBAR != 0 {
            raus.push_str(" shadowrootserializable=\"\"");
        }
        if m & crate::dom::SCHATTEN_MANUELL != 0 {
            raus.push_str(" shadowrootslotassignment=\"manual\"");
        }
        if m & crate::dom::SCHATTEN_KLONBAR != 0 {
            raus.push_str(" shadowrootclonable=\"\"");
        }
        raus.push('>');
        for &k in &self.knoten[sr].kinder {
            self.serialisiere_knoten(k, false, raus, tiefe + 1, Some(w));
        }
        raus.push_str("</template>");
    }

    /// `outerHTML` lesen.
    #[must_use]
    pub fn outer_html(&self, i: usize) -> String {
        let mut raus = String::new();
        let roh = self.knoten[i]
            .eltern
            .and_then(|e| self.element_name(e))
            .is_some_and(|n| ROHTEXT.contains(&n));
        self.serialisiere_knoten(i, roh, &mut raus, 0, None);
        raus
    }

    fn serialisiere_knoten(&self, i: usize, roh: bool, raus: &mut String, tiefe: usize, wahl: Option<&SchattenWahl<'_>>) {
        if tiefe > MAX_TIEFE_LEBEND || raus.len() > MAX_SERIALISIERT {
            return;
        }
        match &self.knoten[i].art {
            Art::Text(t) => {
                if roh {
                    raus.push_str(t);
                } else {
                    for c in t.chars() {
                        match c {
                            '&' => raus.push_str("&amp;"),
                            '<' => raus.push_str("&lt;"),
                            '>' => raus.push_str("&gt;"),
                            '\u{a0}' => raus.push_str("&nbsp;"),
                            _ => raus.push(c),
                        }
                    }
                }
            }
            Art::Kommentar(t) => {
                raus.push_str("<!--");
                raus.push_str(t);
                raus.push_str("-->");
            }
            Art::Pi { target, daten } => {
                raus.push_str("<?");
                raus.push_str(target);
                raus.push(' ');
                raus.push_str(daten);
                raus.push('>');
            }
            Art::Doctype { name, .. } => {
                raus.push_str("<!DOCTYPE ");
                raus.push_str(name);
                raus.push('>');
            }
            Art::Dokument | Art::Fragment | Art::ShadowRoot { .. } => {
                for &k in &self.knoten[i].kinder {
                    self.serialisiere_knoten(k, false, raus, tiefe + 1, wahl);
                }
            }
            Art::Element { raum, name, attribute, .. } => {
                raus.push('<');
                raus.push_str(name);
                for a in attribute {
                    raus.push(' ');
                    raus.push_str(&attribut_qname(a));
                    raus.push_str("=\"");
                    for c in a.wert.chars() {
                        match c {
                            '&' => raus.push_str("&amp;"),
                            '"' => raus.push_str("&quot;"),
                            '\u{a0}' => raus.push_str("&nbsp;"),
                            _ => raus.push(c),
                        }
                    }
                    raus.push('"');
                }
                raus.push('>');
                if *raum == Namensraum::Html && LEERE_ELEMENTE.contains(&name.as_str()) {
                    return;
                }
                let kinder_roh = *raum == Namensraum::Html && ROHTEXT.contains(&name.as_str());
                // Release 12.10.2026: die Shadow Root zuerst (HTML §13.3 Schritt
                // 3, „getHTML"): `<template shadowrootmode=…>` mit den Merkmalen.
                if let Some(w) = wahl {
                    self.schatten_vorlage(i, raus, tiefe, w);
                }
                let quelle = self.template_inhalt(i).unwrap_or(i);
                for &k in &self.knoten[quelle].kinder {
                    self.serialisiere_knoten(k, kinder_roh, raus, tiefe + 1, wahl);
                }
                raus.push_str("</");
                raus.push_str(name);
                raus.push('>');
            }
        }
    }

    /// Attribute eines Elements (fuer `attributes`, `getAttributeNames`).
    #[must_use]
    pub fn attribute_von(&self, i: usize) -> Vec<(String, String)> {
        match self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Element { attribute, .. }) => {
                attribute.iter().map(|a| (attribut_qname(a), a.wert.clone())).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Entfernt ein Attribut (HTML-Namensraum).
    pub fn attribut_entfernen(&mut self, i: usize, name: &str) -> bool {
        let alt = match self.knoten.get(i).map(|k| &k.art) {
            Some(Art::Element { attribute, .. }) => attribute.iter().find(|a| attribut_qname(a) == name).map(|a| a.wert.clone()),
            _ => None,
        };
        if let Some(Art::Element { attribute, .. }) = self.knoten.get_mut(i).map(|k| &mut k.art) {
            let vorher = attribute.len();
            attribute.retain(|a| attribut_qname(a) != name);
            let weg = attribute.len() != vorher;
            if weg {
                self.attribute_changed(name.rsplit(':').next().unwrap_or(name));
            }
            if weg && self.journal.is_some() {
                self.protokoll(crate::dom::Aenderung::Attribut { knoten: i, name: name.to_string(), alt, ns: None });
            }
            return weg;
        }
        false
    }
}

/// Release 2026-10-15: the namespace of a parsed attribute of a foreign
/// element from its qualified name (HTML §13.2.6.1 "adjust foreign
/// attributes"; the names are already case-adjusted).
fn fremder_attributname(elem_raum: Namensraum, q: &str) -> (Namensraum, String) {
    if !matches!(elem_raum, Namensraum::Svg | Namensraum::MathMl) {
        return (Namensraum::Html, q.to_string());
    }
    match q {
        "xlink:actuate" | "xlink:arcrole" | "xlink:href" | "xlink:role" | "xlink:show" | "xlink:title" | "xlink:type" => {
            (Namensraum::Xlink, q["xlink:".len()..].to_string())
        }
        "xml:base" | "xml:lang" | "xml:space" => (Namensraum::Xml, q["xml:".len()..].to_string()),
        "xmlns" => (Namensraum::Xmlns, "xmlns".to_string()),
        "xmlns:xlink" => (Namensraum::Xmlns, "xlink".to_string()),
        _ => (Namensraum::Html, q.to_string()),
    }
}

fn attribut_qname(a: &Attribut) -> String {
    match a.raum {
        Namensraum::Html | Namensraum::Svg | Namensraum::MathMl | Namensraum::Fremd => a.name.clone(),
        Namensraum::Xml => format!("xml:{}", a.name),
        Namensraum::Xmlns if a.name == "xmlns" => "xmlns".to_string(),
        Namensraum::Xmlns => format!("xmlns:{}", a.name),
        Namensraum::Xlink => format!("xlink:{}", a.name),
    }
}

/// Wandelt den lebenden Baum unter `wurzel` in `HtmlNode`s fuer den
/// Seitenbau um und liefert dazu fuer JEDEN erzeugten Knoten (Elemente und
/// Texte, in Vorordnung) seinen Arena-Index — so findet der Host zu jedem
/// Seitenknoten den lebenden Knoten zurueck.
///
/// Wie [`crate::parse_html`]: Kommentare, Doctype und PI fallen weg,
/// Template-Inhalte haengen als Kinder am `<template>`, die Shadow Root
/// steht in `shadow_root`.
/// Release 12.10.2026: die Arena-Indizes JE BAUM (0 = Licht, dann die
/// Shadow Trees). Die flache Liste ist Licht in Vorordnung, danach die
/// Shadow Trees in Breitenfolge (ein Wirt reiht seinen Baum beim ERSTEN
/// Besuch ein) — genau die Regel von `narsil_page::schatten::ords_gehen`.
/// GEMESSEN vorher: Shadow-Inhalte bekamen keine Indizes („stumm"), ihre
/// Knoten hatten keine Host-Id, Klicks darauf erreichten kein Element.
struct Baeume {
    listen: Vec<(Vec<usize>, Vec<usize>)>,
}

impl Baeume {
    fn neu() -> Self {
        Self { listen: vec![(Vec::new(), Vec::new())] }
    }
    fn merken(&mut self, baum: usize, k: usize) {
        self.listen[baum].0.push(k);
    }
    /// Ein neuer Shadow Tree in `baum` (Obergrenze M3: dann keiner mehr,
    /// seine Knoten landen im Licht-Baum am Ende und bekommen keine Nummer).
    fn neuer_baum(&mut self, baum: usize) -> usize {
        if self.listen.len() >= 100_000 {
            return baum;
        }
        let u = self.listen.len();
        self.listen.push((Vec::new(), Vec::new()));
        self.listen[baum].1.push(u);
        u
    }
    fn flach(mut self) -> Vec<usize> {
        let mut raus = std::mem::take(&mut self.listen[0].0);
        let mut schlange: std::collections::VecDeque<usize> = self.listen[0].1.iter().copied().collect();
        while let Some(b) = schlange.pop_front() {
            raus.append(&mut self.listen[b].0);
            schlange.extend(self.listen[b].1.iter().copied());
        }
        raus
    }
}

#[must_use]
pub fn in_htmlnode_mit_indizes(dok: &Dokument, wurzel: usize) -> (Vec<HtmlNode>, Vec<usize>) {
    let mut baeume = Baeume::neu();
    let mut raus = Vec::new();
    // Release 11.10.2026: Wirt -> Shadow Root EINMAL je Umwandlung (vorher
    // lief jedes Element ueber die ganze Arena; siehe `knoten_umwandeln`).
    let schatten: HashMap<usize, usize> = dok
        .knoten
        .iter()
        .enumerate()
        .filter_map(|(i, n)| match (&n.art, n.eltern) {
            (Art::ShadowRoot { .. }, Some(wirt)) => Some((wirt, i)),
            _ => None,
        })
        .collect();
    for &k in &dok.knoten[wurzel].kinder {
        if let Some(n) = knoten_umwandeln(dok, k, &mut baeume, 0, 0, &schatten) {
            raus.push(n);
        }
    }
    (raus, baeume.flach())
}

fn knoten_umwandeln(
    dok: &Dokument,
    k: usize,
    baeume: &mut Baeume,
    baum: usize,
    tiefe: usize,
    schatten: &HashMap<usize, usize>,
) -> Option<HtmlNode> {
    if tiefe > MAX_TIEFE_LEBEND {
        return None;
    }
    match &dok.knoten[k].art {
        Art::Text(t) => {
            baeume.merken(baum, k);
            Some(HtmlNode::Text(t.clone()))
        }
        Art::Element { name, attribute, template_inhalt, .. } => {
            baeume.merken(baum, k);
            // Der Shadow Tree wird beim ERSTEN Besuch eingereiht (vor den
            // Licht-Kindern, siehe `Baeume`).
            let schatten_baum = schatten.get(&k).map(|_| baeume.neuer_baum(baum));
            let mut m: HashMap<String, String> = HashMap::with_capacity(attribute.len());
            for a in attribute {
                m.insert(attribut_qname(a), a.wert.clone());
            }
            let zustand = dok.css_zustand_von(k);
            if zustand & 1 != 0 {
                m.insert(crate::dom::ATTR_POPOVER_OFFEN.to_string(), String::new());
            }
            if zustand & 2 != 0 {
                m.insert(crate::dom::ATTR_MODAL.to_string(), String::new());
            }
            if zustand & 4 != 0 {
                m.insert(crate::dom::ATTR_FOKUS.to_string(), String::new());
            }
            if zustand & 8 != 0 {
                m.insert(crate::dom::ATTR_ANGEHAKT.to_string(), String::new());
            }
            if zustand & 16 != 0 {
                m.insert(crate::dom::ATTR_NICHT_ANGEHAKT.to_string(), String::new());
            }
            if zustand & 32 != 0 {
                m.insert(crate::dom::ATTR_UNBESTIMMT.to_string(), String::new());
            }
            // Release 2026-10-15: the dirty value of a control, so the page's
            // selectors see what the user (or a script) typed.
            if let Some(v) = dok.css_wert_von(k) {
                if name.eq_ignore_ascii_case("input") {
                    m.insert("value".to_string(), v.to_string());
                } else if name.eq_ignore_ascii_case("textarea") {
                    m.insert(crate::dom::ATTR_WERT.to_string(), v.to_string());
                }
            }
            let mut kinder = Vec::new();
            for &c in &dok.knoten[k].kinder {
                if let Some(n) = knoten_umwandeln(dok, c, baeume, baum, tiefe + 1, schatten) {
                    kinder.push(n);
                }
            }
            if let Some(inhalt) = template_inhalt {
                for &c in &dok.knoten[*inhalt].kinder {
                    if let Some(n) = knoten_umwandeln(dok, c, baeume, baum, tiefe + 1, schatten) {
                        kinder.push(n);
                    }
                }
            }
            // Release 11.10.2026: aus der Wirt-Tabelle. GEMESSEN vorher: JEDES
            // Element lief ueber die ganze Arena — quadratisch (18 000 Knoten:
            // 239 ms je Umwandlung, nachher 6 ms; bei jedem Neubau nach einer
            // DOM-Aenderung).
            let shadow_root = schatten.get(&k).copied().and_then(|i| match &dok.knoten[i].art {
                Art::ShadowRoot { mode, .. } => Some((i, *mode)),
                _ => None,
            }).map(|(i, mode)| {
                let mut sr_kinder = Vec::new();
                let u = schatten_baum.unwrap_or(baum);
                for &c in &dok.knoten[i].kinder {
                    if let Some(n) = knoten_umwandeln(dok, c, baeume, u, tiefe + 1, schatten) {
                        sr_kinder.push(n);
                    }
                }
                ShadowRootInfo {
                    mode: match mode {
                        ShadowRootMode::Open => crate::ShadowRootMode::Open,
                        ShadowRootMode::Closed => crate::ShadowRootMode::Closed,
                    },
                    children: sr_kinder,
                    merkmale: dok.schatten_merkmale(k),
                }
            });
            Some(HtmlNode::Element { tag: name.clone(), attributes: m, children: kinder, shadow_root })
        }
        Art::Fragment | Art::Dokument => None,
        _ => None,
    }
}

/// Baut ein lebendes Dokument aus einem `HtmlNode`-Baum (dem Baum, aus dem
/// die Seite gebaut wurde) und liefert die Arena-Indizes in DERSELBEN
/// Vorordnung wie [`in_htmlnode_mit_indizes`] — Hin- und Rueckweg passen
/// also Knoten fuer Knoten zueinander.
///
/// Elemente ohne Namensraum-Angabe gelten als HTML; `svg`/`math` und ihre
/// Nachfahren bekommen ihren Namensraum. Inhalte eines `<template>` kommen
/// in den Template-Inhalt.
#[must_use]
pub fn aus_htmlnode(nodes: &[HtmlNode]) -> (Dokument, usize, Vec<usize>) {
    let (mut dok, wurzel) = Dokument::neu();
    let mut baeume = Baeume::neu();
    for n in nodes {
        if let Some(k) = htmlnode_einbauen(&mut dok, n, Namensraum::Html, &mut baeume, 0, 0) {
            dok.anhaengen(wurzel, k);
        }
    }
    (dok, wurzel, baeume.flach())
}

fn htmlnode_einbauen(
    dok: &mut Dokument,
    n: &HtmlNode,
    raum: Namensraum,
    baeume: &mut Baeume,
    baum: usize,
    tiefe: usize,
) -> Option<usize> {
    if tiefe > MAX_TIEFE_LEBEND || dok.knoten.len() >= MAX_KNOTEN {
        return None;
    }
    match n {
        HtmlNode::Text(t) => {
            let k = dok.neuer_knoten(Art::Text(t.clone()));
            baeume.merken(baum, k);
            Some(k)
        }
        HtmlNode::Element { tag, attributes, children, shadow_root } => {
            let klein = tag.to_ascii_lowercase();
            let raum = match klein.as_str() {
                "svg" => Namensraum::Svg,
                "math" => Namensraum::MathMl,
                _ => raum,
            };
            let name = if raum == Namensraum::Html { klein } else { tag.clone() };
            // Release 2026-10-15: on SVG/MathML elements the parser put
            // xlink:*, xml:* and xmlns attributes into their namespaces
            // ("adjust foreign attributes"); the page tree keeps only the
            // qualified name, so split it again. MEASURED before (WPT
            // dom/nodes/Attr-prefix): `g.getAttributeNodeNS(XLINK, "href")`
            // was null for a parsed `<g xlink:href>`.
            let mut attr: Vec<Attribut> = attributes
                .iter()
                .map(|(k, v)| {
                    let (araum, aname) = fremder_attributname(raum, k);
                    Attribut { raum: araum, name: aname, wert: v.clone() }
                })
                .collect();
            // HashMap hat keine Reihenfolge: sortiert, damit outerHTML stabil ist.
            attr.sort_by_key(attribut_qname);
            let ist_template = raum == Namensraum::Html && name == "template";
            let k = dok.neuer_knoten(Art::Element {
                raum,
                name,
                attribute: attr,
                template_inhalt: None,
                shadow_root: None,
            });
            baeume.merken(baum, k);
            let schatten_baum = shadow_root.as_ref().map(|_| baeume.neuer_baum(baum));
            let ziel = if ist_template {
                let inhalt = dok.neuer_knoten(Art::Fragment);
                if let Art::Element { template_inhalt, .. } = &mut dok.knoten[k].art {
                    *template_inhalt = Some(inhalt);
                }
                inhalt
            } else {
                k
            };
            let kind_raum = if raum == Namensraum::Svg
                && matches!(tag.as_str(), "foreignObject" | "desc" | "title")
            {
                Namensraum::Html
            } else {
                raum
            };
            for c in children {
                if let Some(ck) = htmlnode_einbauen(dok, c, kind_raum, baeume, baum, tiefe + 1) {
                    dok.anhaengen(ziel, ck);
                }
            }
            if let Some(sr) = shadow_root {
                let offen = matches!(sr.mode, crate::ShadowRootMode::Open);
                // Ohne gueltigen Wirt ein loses Fragment: die Indizes muessen
                // trotzdem in derselben Ordnung stehen wie in `ords_gehen`.
                let w = dok.shadow_anhaengen(k, offen).unwrap_or_else(|| dok.neuer_knoten(Art::Fragment));
                dok.schatten_merkmale_setzen(k, sr.merkmale);
                let u = schatten_baum.unwrap_or(baum);
                for c in &sr.children {
                    if let Some(ck) = htmlnode_einbauen(dok, c, Namensraum::Html, baeume, u, tiefe + 1) {
                        dok.anhaengen(w, ck);
                    }
                }
            }
            Some(k)
        }
    }
}

/// Das lebende Dokument einer Seite parsen (wie [`crate::parse_html`], aber
/// der Arena-Baum bleibt erhalten). Mit `scripting` (Seiten mit JS:
/// `<noscript>` ist dann Rohtext).
#[must_use]
pub fn lebendes_dokument(html: &str, scripting: bool) -> (Dokument, usize) {
    crate::treebuilder::parse_dokument_mit_fehler(html, scripting).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> (Dokument, usize) {
        lebendes_dokument(html, true)
    }

    fn finde(d: &Dokument, name: &str) -> usize {
        (0..d.knoten.len()).find(|&i| d.element_name(i) == Some(name)).expect(name)
    }

    #[test]
    fn geschwister_und_kommentare_bleiben() {
        let (d, _) = doc("<div id=a><span>x</span><!--k--><b>y</b></div>");
        let a = finde(&d, "div");
        let span = d.kinder(a)[0];
        let k = d.naechstes_geschwister(span).unwrap();
        assert_eq!(d.knoten_typ(k), 8);
        assert_eq!(d.element_name(d.naechstes_geschwister(k).unwrap()), Some("b"));
        assert_eq!(d.text_inhalt(a), "xy");
    }

    #[test]
    fn inner_html_lesen_und_schreiben() {
        let (mut d, _) = doc("<div id=a></div>");
        let a = finde(&d, "div");
        assert!(d.inner_html_setzen(a, "<p class=\"x\">1 &amp; 2</p><!--c--><br>"));
        assert_eq!(d.inner_html(a), "<p class=\"x\">1 &amp; 2</p><!--c--><br>");
        assert_eq!(d.kinder(a).len(), 3);
    }

    #[test]
    fn template_hat_inhalt() {
        let (mut d, _) = doc("<template id=t><b>q</b></template>");
        let t = finde(&d, "template");
        let inhalt = d.template_inhalt(t).unwrap();
        assert_eq!(d.kinder(inhalt).len(), 1);
        let k = d.klonen(inhalt, true).unwrap();
        assert_eq!(d.inner_html(k), "<b>q</b>");
    }

    #[test]
    fn fragment_gibt_kinder_ab() {
        let (mut d, _) = doc("<div></div>");
        let div = finde(&d, "div");
        let f = d.neues_fragment().unwrap();
        let a = d.neues_element("i").unwrap();
        let b = d.neuer_text("t").unwrap();
        d.kind_anhaengen(f, a);
        d.kind_anhaengen(f, b);
        assert!(d.kind_anhaengen(div, f));
        assert_eq!(d.kinder(div).len(), 2);
        assert!(d.kinder(f).is_empty());
        assert!(!d.kind_anhaengen(a, div), "Zyklus muss abgelehnt werden");
    }

    #[test]
    fn umwandlung_liefert_indizes_in_vorordnung() {
        let (d, w) = doc("<p>a<b>c</b></p>");
        let (baum, idx) = in_htmlnode_mit_indizes(&d, w);
        assert!(!baum.is_empty());
        let namen: Vec<String> = idx
            .iter()
            .map(|&i| d.element_name(i).map(str::to_string).unwrap_or_else(|| d.text_inhalt(i)))
            .collect();
        let p = namen.iter().position(|n| n == "p").unwrap();
        assert_eq!(&namen[p..p + 4], &["p", "a", "b", "c"]);
    }

    #[test]
    fn hin_und_zurueck_passt_knoten_fuer_knoten() {
        let quelle = crate::parse_html("<div id=a>t<template><b>x</b></template><svg><rect/></svg><p>q</p></div>");
        let (d, w, hin) = aus_htmlnode(&quelle);
        let (zurueck, idx) = in_htmlnode_mit_indizes(&d, w);
        assert_eq!(hin, idx, "Vorordnung hin und zurueck");
        assert_eq!(crate::serialisiere(&quelle), crate::serialisiere(&zurueck));
    }

    #[test]
    fn shadow_wurzel() {
        let (mut d, _) = doc("<div></div>");
        let div = finde(&d, "div");
        let s = d.shadow_anhaengen(div, true).unwrap();
        assert_eq!(d.shadow_wurzel(div), Some(s));
        assert!(d.shadow_anhaengen(div, true).is_none());
        let x = d.neues_element("x-y").unwrap();
        d.kind_anhaengen(s, x);
        assert_eq!(d.wurzel_von(x), s);
    }
}

/// Release 12.10.2026: welche Shadow Roots `getHTML` serialisiert.
struct SchattenWahl<'a> {
    serialisierbar: bool,
    wurzeln: &'a [usize],
}

/// Release 2026-10-15: the source order of the attributes of a parsed page.
/// `HtmlNode` keeps attributes in a `HashMap`, so the live DOM built from it
/// (`aus_htmlnode`) had them sorted by name; this table remembers, for every
/// element with two or more attributes, the permutation from the name order
/// back to the source order. The key is the element name with its sorted
/// (name, value) pairs — elements the page pipeline adds or replaces in
/// between find no entry and simply keep the name order; equal elements
/// take their entries in tree order. MEASURED before (WPT dom/collections
/// namednodemap-, domstringmap-supported-property-names):
/// `<div id class>` listed `class, id`.
#[derive(Debug, Default)]
pub struct AttributeOrder {
    queues: HashMap<u64, std::collections::VecDeque<Vec<u32>>>,
}

impl AttributeOrder {
    fn key<'a>(tag: &str, sorted: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        tag.to_ascii_lowercase().hash(&mut h);
        for (n, v) in sorted {
            n.hash(&mut h);
            v.hash(&mut h);
        }
        h.finish()
    }

    /// Records the attributes of one element in source order.
    pub fn record(&mut self, tag: &str, source: &[(String, String)]) {
        let n = source.len();
        if n < 2 || self.queues.len() >= 1_000_000 {
            return;
        }
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| source[a].0.cmp(&source[b].0));
        let mut perm = vec![0u32; n];
        for (j, &i) in idx.iter().enumerate() {
            perm[i] = j as u32;
        }
        let identity = perm.iter().enumerate().all(|(i, &p)| p as usize == i);
        let key = Self::key(tag, idx.iter().map(|&i| (source[i].0.as_str(), source[i].1.as_str())));
        self.queues.entry(key).or_default().push_back(if identity { Vec::new() } else { perm });
    }

    /// Puts the attributes of one live-DOM element back into source order
    /// (they arrive in any order; the name order is the reference).
    pub fn apply(&mut self, tag: &str, attrs: &mut Vec<Attribut>) {
        let n = attrs.len();
        if n < 2 || self.queues.is_empty() {
            return;
        }
        let namen: Vec<String> = attrs.iter().map(attribut_qname).collect();
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| namen[a].cmp(&namen[b]));
        let key = Self::key(tag, idx.iter().map(|&i| (namen[i].as_str(), attrs[i].wert.as_str())));
        let Some(perm) = self.queues.get_mut(&key).and_then(|q| q.pop_front()) else { return };
        let sorted: Vec<Attribut> = idx.iter().map(|&i| attrs[i].clone()).collect();
        if perm.is_empty() {
            *attrs = sorted;
        } else if perm.len() == n {
            *attrs = perm.iter().map(|&p| sorted[p as usize].clone()).collect();
        }
    }
}

thread_local! {
    static PAGE_ATTRIBUTE_ORDER: std::cell::RefCell<Option<AttributeOrder>> = const { std::cell::RefCell::new(None) };
}

/// Release 2026-10-15: the attribute order of the page parsed last on this
/// thread, for the live DOM built next (`narsil_page::html_lesen`).
pub fn remember_page_attribute_order(order: AttributeOrder) {
    PAGE_ATTRIBUTE_ORDER.with(|o| *o.borrow_mut() = Some(order));
}

/// Takes the remembered page attribute order (once).
pub fn take_page_attribute_order() -> Option<AttributeOrder> {
    PAGE_ATTRIBUTE_ORDER.with(|o| o.borrow_mut().take())
}
