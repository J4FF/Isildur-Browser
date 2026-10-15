//! Release 12.10.2026: LEBENDE BEREICHE (DOM §5 „live ranges").
//!
//! # WARUM
//!
//! Ein `Range` muss jede Aenderung am Baum mitgehen (DOM §5.5): Einfuegen
//! und Entfernen von Knoten (§4.2.3), „replace data" (§4.10), `splitText`
//! (§4.11) und `normalize` (§4.4). GEMESSEN vorher (WPT dom/ranges, Commit
//! 147859dd): 8 von 72 Dateien, 8.829 von 43.906 Teiltests — `Range` war ein
//! Polyfill im Vorspann ohne Live-Aktualisierung, `isPointInRange` war immer
//! `false`. Eine Umhuellung der DOM-Methoden in JS kann das nicht leisten:
//! die Host-Methoden an `Element`/`Document` gewinnen gegen JS-Prototypen,
//! und `innerHTML`, `textContent` oder der Parser laufen ganz in Rust.
//!
//! # WIE
//!
//! Die Grenzpunkte aller lebenden Bereiche liegen hier, neben der Arena
//! ([`Dokument::bereiche`]). Die Mutationsprimitive der Arena rufen die
//! Schritte der Spec (`abtrennen` → „remove", `kind_einfuegen` → „insert",
//! `kinder_leeren` → „replace all", `text_inhalt_setzen`/[`Dokument::daten_ersetzen`]
//! → „replace data", [`Dokument::text_teilen`], [`Dokument::normalisieren`]).
//! Der Vorspann haelt je `Range` nur den Platz und dessen Generation.
//!
//! Ohne einen einzigen lebenden Bereich kostet das nichts (ein Vergleich je
//! Primitive). Mit Bereichen weist die Zaehlung der Behaelter
//! ([`Bereiche::behaelter`]) die meisten Aenderungen in O(1) ab: nur wenn ein
//! betroffener Knoten Behaelter eines Grenzpunkts ist, laufen die Punkte.
//!
//! # GRENZEN (M3)
//!
//! Hoechstens [`MAX_BEREICHE`] lebende Bereiche. Die Engine kennt keine
//! Finalisierung (ein toter `Range` meldet sich nie ab), also verdraengt ein
//! neuer Bereich den aeltesten; der verdraengte behaelt im Skript seine
//! letzten Grenzpunkte und wird damit statisch. Die Ahnenkette wird
//! hoechstens [`MAX_KETTE`] Schritte weit verfolgt.

use crate::dom::{Art, Dokument, MAX_KNOTEN};
use std::collections::{HashMap, HashSet, VecDeque};

/// Hoechstens so viele lebende Bereiche (M3). GEMESSEN: WPT `Range-set.html`
/// legt rund 11.000 an (`cloneRange` je Teiltest).
pub const MAX_BEREICHE: usize = 1 << 17;
/// Hoechstens so viele Schritte die Ahnenkette hinauf.
pub const MAX_KETTE: usize = 4096;

/// Ein Grenzpunkt (DOM §5.1 „boundary point"): Knoten und Versatz — bei
/// Zeichendaten in UTF-16-Einheiten, sonst die Zahl der Kinder davor.
pub type Punkt = (usize, u32);

/// Die lebenden Bereiche eines Dokuments (siehe Modulkopf).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bereiche {
    /// Je Platz Start und Ende, `None` = frei.
    plaetze: Vec<Option<[Punkt; 2]>>,
    /// Generation je Platz (ein verdraengter Platz zaehlt hoch).
    generation: Vec<u32>,
    frei: Vec<u32>,
    /// Belegte Plaetze in der Reihenfolge ihres Anlegens (Verdraengung).
    reihe: VecDeque<(u32, u32)>,
    /// Behaelter -> Zahl der Grenzpunkte, die in ihm liegen.
    behaelter: HashMap<usize, u32>,
}

impl Bereiche {
    /// Gibt es keinen lebenden Bereich?
    #[must_use]
    pub fn ist_leer(&self) -> bool {
        self.reihe.is_empty()
    }

    /// Zahl der lebenden Bereiche.
    #[must_use]
    pub fn anzahl(&self) -> usize {
        self.reihe.len()
    }

    /// Liegt mindestens ein Grenzpunkt in `k`?
    #[must_use]
    pub fn ist_behaelter(&self, k: usize) -> bool {
        self.behaelter.contains_key(&k)
    }

    fn zaehlen(&mut self, k: usize, plus: bool) {
        if plus {
            *self.behaelter.entry(k).or_insert(0) += 1;
        } else if let Some(n) = self.behaelter.get_mut(&k) {
            *n -= 1;
            if *n == 0 {
                self.behaelter.remove(&k);
            }
        }
    }

    /// Legt einen Bereich an, Start und Ende bei `p`. Liefert Platz und
    /// Generation. Bei [`MAX_BEREICHE`] faellt der aelteste weg.
    pub fn anlegen(&mut self, p: Punkt) -> (u32, u32) {
        while self.reihe.len() >= MAX_BEREICHE {
            let Some((i, g)) = self.reihe.pop_front() else { break };
            self.freigeben(i, g);
        }
        let i = match self.frei.pop() {
            Some(i) => i,
            None => {
                self.plaetze.push(None);
                self.generation.push(0);
                (self.plaetze.len() - 1) as u32
            }
        };
        let g = self.generation[i as usize];
        self.plaetze[i as usize] = Some([p, p]);
        self.zaehlen(p.0, true);
        self.zaehlen(p.0, true);
        self.reihe.push_back((i, g));
        (i, g)
    }

    fn freigeben(&mut self, i: u32, g: u32) {
        let Some(slot) = self.plaetze.get_mut(i as usize) else { return };
        if self.generation[i as usize] != g {
            return;
        }
        if let Some([a, b]) = slot.take() {
            self.zaehlen(a.0, false);
            self.zaehlen(b.0, false);
        }
        self.generation[i as usize] = g.wrapping_add(1);
        self.frei.push(i);
    }

    /// Start und Ende eines Bereichs (`None`: verdraengt oder unbekannt).
    #[must_use]
    pub fn lesen(&self, i: u32, g: u32) -> Option<[Punkt; 2]> {
        if self.generation.get(i as usize).copied() != Some(g) {
            return None;
        }
        self.plaetze.get(i as usize).copied().flatten()
    }

    /// Setzt Start und Ende. `false`: der Bereich ist verdraengt.
    pub fn setzen(&mut self, i: u32, g: u32, start: Punkt, ende: Punkt) -> bool {
        let Some(alt) = self.lesen(i, g) else { return false };
        self.zaehlen(alt[0].0, false);
        self.zaehlen(alt[1].0, false);
        self.zaehlen(start.0, true);
        self.zaehlen(ende.0, true);
        self.plaetze[i as usize] = Some([start, ende]);
        true
    }

    /// Wendet `f` auf jeden Grenzpunkt an und haelt die Behaelter-Zaehlung.
    fn alle(&mut self, mut f: impl FnMut(&mut Punkt)) {
        let mut wechsel: Vec<(usize, usize)> = Vec::new();
        for slot in self.plaetze.iter_mut().flatten() {
            for p in slot.iter_mut() {
                let alt = p.0;
                f(p);
                if p.0 != alt {
                    wechsel.push((alt, p.0));
                }
            }
        }
        for (alt, neu) in wechsel {
            self.zaehlen(alt, false);
            self.zaehlen(neu, true);
        }
    }
}

/// Eine kleine Menge von Knoten (linear bis acht, sonst gehasht).
enum Menge {
    Klein(Vec<usize>),
    Gross(HashSet<usize>),
}

impl Menge {
    fn aus(v: Vec<usize>) -> Self {
        if v.len() > 8 { Menge::Gross(v.into_iter().collect()) } else { Menge::Klein(v) }
    }
    fn hat(&self, k: usize) -> bool {
        match self {
            Menge::Klein(v) => v.contains(&k),
            Menge::Gross(s) => s.contains(&k),
        }
    }
    fn ist_leer(&self) -> bool {
        match self {
            Menge::Klein(v) => v.is_empty(),
            Menge::Gross(s) => s.is_empty(),
        }
    }
}

/// Die Laenge einer Zeichenkette in UTF-16-Einheiten (DOM-Laenge).
#[must_use]
pub fn laenge16(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

/// Der Byte-Index zur UTF-16-Stelle `stelle` (geklemmt; mitten in einem
/// Ersatzpaar: davor).
#[must_use]
pub fn byte_index16(s: &str, stelle: u32) -> usize {
    let mut n = 0u32;
    for (b, c) in s.char_indices() {
        if n >= stelle {
            return b;
        }
        n += c.len_utf16() as u32;
        if n > stelle {
            return b;
        }
    }
    s.len()
}

impl Dokument {
    /// Ist `c` ein inklusiver Nachfahre von `k` (ohne die Shadow-Grenze zu
    /// ueberschreiten, wie DOM §4.2 „descendant")?
    #[must_use]
    pub fn ist_inklusiver_nachfahre(&self, c: usize, k: usize) -> bool {
        let mut n = c;
        for _ in 0..MAX_KETTE {
            if n == k {
                return true;
            }
            match self.knoten.get(n) {
                Some(x) if matches!(x.art, Art::ShadowRoot { .. }) => return false,
                Some(x) => match x.eltern {
                    Some(e) => n = e,
                    None => return false,
                },
                None => return false,
            }
        }
        false
    }

    /// Die Behaelter von Grenzpunkten, die inklusive Nachfahren von `k` sind.
    fn behaelter_in(&self, k: usize) -> Menge {
        let blatt = self.knoten.get(k).is_none_or(|n| n.kinder.is_empty());
        if blatt {
            return Menge::Klein(if self.bereiche.ist_behaelter(k) { vec![k] } else { Vec::new() });
        }
        let v: Vec<usize> = self
            .bereiche
            .behaelter
            .keys()
            .copied()
            .filter(|&c| self.ist_inklusiver_nachfahre(c, k))
            .collect();
        Menge::aus(v)
    }

    /// DOM §4.2.3 „remove", Schritte 4–7: `kind` (an der Stelle `index` von
    /// `eltern`) geht gleich weg.
    pub(crate) fn bereiche_vor_entfernen(&mut self, kind: usize, eltern: usize, index: u32) {
        if self.bereiche.ist_leer() {
            return;
        }
        let innen = self.behaelter_in(kind);
        if innen.ist_leer() && !self.bereiche.ist_behaelter(eltern) {
            return;
        }
        self.bereiche.alle(|p| {
            if innen.hat(p.0) {
                *p = (eltern, index);
            } else if p.0 == eltern && p.1 > index {
                p.1 -= 1;
            }
        });
    }

    /// DOM §4.2.3 „insert", Schritt 5: `anzahl` Knoten kommen an die Stelle
    /// `index` von `eltern` (vor das Kind, das jetzt dort steht).
    pub(crate) fn bereiche_vor_einfuegen(&mut self, eltern: usize, index: u32, anzahl: u32) {
        if self.bereiche.ist_leer() || !self.bereiche.ist_behaelter(eltern) {
            return;
        }
        self.bereiche.alle(|p| {
            if p.0 == eltern && p.1 > index {
                p.1 += anzahl;
            }
        });
    }

    /// DOM §4.2.3 „replace all" (alle Kinder von `k` gehen): jeder
    /// Grenzpunkt in `k` oder darunter landet bei `(k, 0)`.
    pub(crate) fn bereiche_alle_kinder_weg(&mut self, k: usize) {
        if self.bereiche.ist_leer() {
            return;
        }
        let innen = self.behaelter_in(k);
        if innen.ist_leer() {
            return;
        }
        self.bereiche.alle(|p| {
            if innen.hat(p.0) {
                *p = (k, 0);
            }
        });
    }

    /// DOM §4.10 „replace data", Schritte 8–11 (nach dem Ersetzen).
    pub(crate) fn bereiche_daten(&mut self, k: usize, versatz: u32, anzahl: u32, neu: u32) {
        if self.bereiche.ist_leer() || !self.bereiche.ist_behaelter(k) {
            return;
        }
        let bis = versatz.saturating_add(anzahl);
        self.bereiche.alle(|p| {
            if p.0 != k {
                return;
            }
            if p.1 > versatz && p.1 <= bis {
                p.1 = versatz;
            } else if p.1 > bis {
                p.1 = p.1 - anzahl + neu;
            }
        });
    }

    /// Die Stelle von `k` unter seinen Geschwistern.
    #[must_use]
    pub fn index_von(&self, k: usize) -> Option<usize> {
        let e = self.knoten.get(k)?.eltern?;
        self.knoten[e].kinder.iter().position(|&x| x == k)
    }

    /// DOM §4.10 „replace data": ersetzt `anzahl` UTF-16-Einheiten ab
    /// `versatz` durch `daten` (mit Aenderungsprotokoll und lebenden
    /// Bereichen). `Err`: `versatz` liegt hinter dem Ende (IndexSizeError).
    pub fn daten_ersetzen(&mut self, k: usize, versatz: u32, anzahl: u32, daten: &str) -> Result<(), u32> {
        let alt = match self.knoten.get(k).map(|n| &n.art) {
            Some(Art::Text(t)) | Some(Art::Kommentar(t)) => t.clone(),
            Some(Art::Pi { daten, .. }) => daten.clone(),
            _ => return Ok(()),
        };
        let laenge = laenge16(&alt);
        if versatz > laenge {
            return Err(laenge);
        }
        let anzahl = anzahl.min(laenge - versatz);
        let von = byte_index16(&alt, versatz);
        let bis = byte_index16(&alt, versatz + anzahl);
        let mut neu = String::with_capacity(alt.len() - (bis - von) + daten.len());
        neu.push_str(&alt[..von]);
        neu.push_str(daten);
        neu.push_str(&alt[bis..]);
        if self.journal.is_some() && !matches!(self.knoten[k].art, Art::Pi { .. }) {
            self.protokoll(crate::dom::Aenderung::Daten { knoten: k, alt });
        }
        match &mut self.knoten[k].art {
            Art::Text(t) | Art::Kommentar(t) => *t = neu,
            Art::Pi { daten: d, .. } => *d = neu,
            _ => {}
        }
        self.bereiche_daten(k, versatz, anzahl, laenge16(daten));
        Ok(())
    }

    /// DOM §4.11 `splitText(offset)`: teilt den Textknoten `k` an der
    /// UTF-16-Stelle `versatz`; liefert den neuen Knoten (hinter `k`, wenn
    /// `k` Eltern hat). `Err(laenge)`: `versatz` liegt hinter dem Ende.
    pub fn text_teilen(&mut self, k: usize, versatz: u32) -> Result<Option<usize>, u32> {
        let Some(Art::Text(alt)) = self.knoten.get(k).map(|n| &n.art) else { return Ok(None) };
        let laenge = laenge16(alt);
        if versatz > laenge {
            return Err(laenge);
        }
        if self.knoten.len() >= MAX_KNOTEN {
            return Ok(None);
        }
        let rest = alt[byte_index16(alt, versatz)..].to_string();
        let neu = self.neuer_knoten(Art::Text(rest));
        if self.cdata.contains(&k) {
            self.cdata.insert(neu);
        }
        if let Some(eltern) = self.knoten[k].eltern
            && let Some(index) = self.index_von(k)
        {
            let vor = self.knoten[eltern].kinder.get(index + 1).copied();
            self.kind_einfuegen(eltern, neu, vor);
            // Schritte 7.2–7.5: Punkte hinter der Stelle wandern in den neuen
            // Knoten; Punkte direkt hinter `k` bleiben hinter dem Paar.
            if !self.bereiche.ist_leer()
                && (self.bereiche.ist_behaelter(k) || self.bereiche.ist_behaelter(eltern))
            {
                let hinter = index as u32 + 1;
                self.bereiche.alle(|p| {
                    if p.0 == k && p.1 > versatz {
                        *p = (neu, p.1 - versatz);
                    } else if p.0 == eltern && p.1 == hinter {
                        p.1 += 1;
                    }
                });
            }
        }
        let anzahl = laenge - versatz;
        let _ = self.daten_ersetzen(k, versatz, anzahl, "");
        Ok(Some(neu))
    }

    /// Ist `k` ein „exclusive Text node" (Text, aber kein CDATA)?
    fn ist_exklusiver_text(&self, k: usize) -> bool {
        matches!(self.knoten.get(k).map(|n| &n.art), Some(Art::Text(_))) && !self.cdata.contains(&k)
    }

    /// DOM §4.4 `normalize()`: leere Textknoten weg, benachbarte Textknoten
    /// zusammen — mit den Bereichsschritten 6.1–6.4 der Spec.
    pub fn normalisieren(&mut self, wurzel: usize) {
        // Die Text-Nachfahren in Baumordnung (ohne Vorlagen-/Schatteninhalt).
        let mut texte = Vec::new();
        let mut stapel = vec![wurzel];
        while let Some(n) = stapel.pop() {
            if n != wurzel && self.ist_exklusiver_text(n) {
                texte.push(n);
            }
            if texte.len() >= MAX_KNOTEN {
                break;
            }
            stapel.extend(self.knoten[n].kinder.iter().rev().copied());
        }
        for t in texte {
            // Schon zusammengelegt (oder sonst nicht mehr darunter)?
            if self.knoten[t].eltern.is_none() || !self.ist_inklusiver_nachfahre(t, wurzel) {
                continue;
            }
            let laenge = match &self.knoten[t].art {
                Art::Text(s) => laenge16(s),
                _ => continue,
            };
            if laenge == 0 {
                self.abtrennen(t);
                continue;
            }
            let Some(eltern) = self.knoten[t].eltern else { continue };
            let mut folgende = Vec::new();
            let mut daten = String::new();
            let mut i = self.index_von(t).map_or(usize::MAX, |i| i + 1);
            while let Some(&c) = self.knoten[eltern].kinder.get(i) {
                if !self.ist_exklusiver_text(c) {
                    break;
                }
                if let Art::Text(s) = &self.knoten[c].art {
                    daten.push_str(s);
                }
                folgende.push((c, i as u32));
                i += 1;
            }
            if folgende.is_empty() {
                continue;
            }
            let _ = self.daten_ersetzen(t, laenge, 0, &daten);
            let mut laenge = laenge;
            for &(c, index) in &folgende {
                if !self.bereiche.ist_leer() {
                    let l = laenge;
                    self.bereiche.alle(|p| {
                        if p.0 == c {
                            *p = (t, p.1 + l);
                        } else if p.0 == eltern && p.1 == index {
                            *p = (t, l);
                        }
                    });
                }
                if let Art::Text(s) = &self.knoten[c].art {
                    laenge += laenge16(s);
                }
            }
            for (c, _) in folgende {
                self.abtrennen(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Dokument;

    fn baum() -> (Dokument, usize, usize, usize) {
        let (mut d, w) = Dokument::neu();
        let p = d.neues_element("p").expect("p");
        d.anhaengen(w, p);
        let a = d.neuer_text("abc").expect("a");
        let b = d.neues_element("b").expect("b");
        d.anhaengen(p, a);
        d.anhaengen(p, b);
        (d, p, a, b)
    }

    #[test]
    fn entfernen_und_einfuegen() {
        let (mut d, p, a, b) = baum();
        let (i, g) = d.bereiche.anlegen((p, 0));
        assert!(d.bereiche.setzen(i, g, (a, 1), (p, 2)));
        d.abtrennen(a);
        assert_eq!(d.bereiche.lesen(i, g), Some([(p, 0), (p, 1)]));
        let c = d.neuer_text("x").expect("x");
        d.kind_einfuegen(p, c, Some(b));
        assert_eq!(d.bereiche.lesen(i, g), Some([(p, 0), (p, 2)]));
        d.kinder_leeren(p);
        assert_eq!(d.bereiche.lesen(i, g), Some([(p, 0), (p, 0)]));
    }

    #[test]
    fn daten_und_teilen() {
        let (mut d, p, a, _) = baum();
        let (i, g) = d.bereiche.anlegen((a, 2));
        assert!(d.bereiche.setzen(i, g, (a, 2), (a, 3)));
        assert!(d.daten_ersetzen(a, 0, 1, "XY").is_ok());
        assert_eq!(d.bereiche.lesen(i, g), Some([(a, 3), (a, 4)]));
        assert_eq!(d.daten_ersetzen(a, 9, 0, ""), Err(4));
        let neu = d.text_teilen(a, 3).expect("teilen").expect("neu");
        assert_eq!(d.bereiche.lesen(i, g), Some([(a, 3), (neu, 1)]));
        assert_eq!(d.knoten[p].kinder.len(), 3);
        d.normalisieren(p);
        assert_eq!(d.knoten[p].kinder.len(), 2);
        assert_eq!(d.bereiche.lesen(i, g), Some([(a, 3), (a, 4)]));
    }

    #[test]
    fn utf16_stellen() {
        assert_eq!(laenge16("a\u{1F600}b"), 4);
        assert_eq!(byte_index16("a\u{1F600}b", 3), 5);
        assert_eq!(byte_index16("abc", 9), 3);
    }
}
