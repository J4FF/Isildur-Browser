//! Graphem-Cluster nach UAX #29 (Unicode 15.1).
//!
//! Erweiterte Graphem-Cluster, Regeln GB1..GB999, vollstaendig:
//!
//! * GB1/GB2 (Textanfang/-ende sind Grenzen),
//! * GB3 (CR x LF),
//! * GB4/GB5 (Bruch um Control, CR, LF),
//! * GB6/GB7/GB8 (Hangul-Silben),
//! * GB9/GB9a/GB9b (Extend, ZWJ, SpacingMark, Prepend),
//! * GB9c (`InCB=Consonant [InCB=Extend InCB=Linker]* InCB=Linker
//!   [InCB=Extend InCB=Linker]* x InCB=Consonant`),
//! * GB11 (`Extended_Pictographic Extend* ZWJ x Extended_Pictographic`),
//! * GB12/GB13 (Regional-Indicator-Paare),
//! * GB999 (sonst Bruch).
//!
//! Die Eigenschaften kommen aus
//! [`crate::unicode_daten::graphem_klassen`]; sie sind mit einem Skript
//! aus den offiziellen Unicode-15.1-Dateien erzeugt (nicht abgetippt).
//!
//! # Laufzeit, Speicher, Grenzen (M3)
//!
//! Alle Funktionen sind linear in der Textlaenge und benoetigen O(1)
//! Zustand. Kein Schritt springt zurueck: die Grenzen laufen aufsteigend
//! durch den Text, jedes Zeichen wird genau einmal angesehen.
//!
//! Die harte Obergrenze jeder Schleife ist `text.len() + 1`: eine Grenze
//! liegt immer auf einem Zeichenanfang, und ein Zeichen belegt mindestens
//! ein Byte. Deshalb ist die Runden- und Ergebniszahl ohne zusaetzlichen
//! Deckel begrenzt (ein Deckel wuerde hier ein FALSCHES Ergebnis liefern,
//! weil die Schnittstelle alle Grenzen verlangt). `graphem_grenzen`
//! allokiert hoechstens `8 * (text.len() + 1)` Byte fuer das Ergebnis —
//! proportional zur Eingabe, ohne Verstaerker.

use crate::unicode_daten::graphem_klassen::{
    Graphemklasse, Incb, graphem_klasse, incb, ist_extended_pictographic,
};

/// Zustand der InCB-Kette fuer GB9c.
///
/// * `Leer`: vor dem naechsten Zeichen ist keine Kette offen.
/// * `Nachkonsonant`: seit einem `InCB=Consonant` kamen nur `InCB=Extend`,
///   aber noch kein Linker.
/// * `MitLinker`: es kam mindestens ein `InCB=Linker`; nur dieser Zustand
///   haelt einen folgenden Consonant fest.
///
/// Es wird nur das Ergebnis der Kette gespeichert, nicht die Kette selbst:
/// der Zustand bleibt O(1), auch bei 100 000 Linkern hintereinander.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncbKette {
    Leer,
    Nachkonsonant,
    MitLinker,
}

/// Zustand zwischen zwei Zeichen — alles, was GB9c/GB11/GB12/GB13 brauchen.
#[derive(Debug, Clone, Copy)]
struct Zustand {
    /// Grapheme_Cluster_Break-Klasse des zuletzt gelesenen Zeichens.
    vorher: Graphemklasse,
    /// Laenge des Regional-Indicator-Laufs, der am letzten Zeichen endet
    /// (GB12/GB13 brauchen nur die Paritaet, nicht die Laenge selbst).
    ri_lauf: usize,
    /// GB11: endet der bisherige Text auf `Extended_Pictographic Extend*`?
    ep_kette: bool,
    /// GB11: endet er auf `Extended_Pictographic Extend* ZWJ`?
    ep_zwj: bool,
    /// GB9c: Zustand der InCB-Kette.
    incb_kette: IncbKette,
}

impl Zustand {
    /// Anfangszustand vor dem ersten Zeichen des Textes.
    fn neu() -> Zustand {
        Zustand {
            vorher: Graphemklasse::Other,
            ri_lauf: 0,
            ep_kette: false,
            ep_zwj: false,
            incb_kette: IncbKette::Leer,
        }
    }
}

/// Entscheidet, ob zwischen dem vorigen Zeichen (nur seine Klasse zaehlt)
/// und dem Zeichen mit den uebergebenen Eigenschaften ein Bruch liegt.
///
/// Die Reihenfolge ist die der Regeln GB3..GB999; die erste zutreffende
/// Regel entscheidet. `ist_extended_pictographic` und `incb` sind fuer
/// das neue Zeichen schon ausgerechnet (eine Suche je Zeichen).
fn ist_bruch(zustand: &Zustand, klasse: Graphemklasse, incb_wert: Incb, ep: bool) -> bool {
    use Graphemklasse::*;

    // GB3: CR x LF. Muss VOR GB4/GB5 stehen, sonst faellt das Paar
    // auseinander.
    if zustand.vorher == Cr && klasse == Lf {
        return false;
    }
    // GB4: (Control | CR | LF) ÷
    if matches!(zustand.vorher, Control | Cr | Lf) {
        return true;
    }
    // GB5: ÷ (Control | CR | LF)
    if matches!(klasse, Control | Cr | Lf) {
        return true;
    }
    // GB6: L x (L | V | LV | LVT)
    if zustand.vorher == L && matches!(klasse, L | V | Lv | Lvt) {
        return false;
    }
    // GB7: (LV | V) x (V | T)
    if matches!(zustand.vorher, Lv | V) && matches!(klasse, V | T) {
        return false;
    }
    // GB8: (LVT | T) x T
    if matches!(zustand.vorher, Lvt | T) && klasse == T {
        return false;
    }
    // GB9: x (Extend | ZWJ)
    if matches!(klasse, Extend | Zwj) {
        return false;
    }
    // GB9a: x SpacingMark
    if klasse == SpacingMark {
        return false;
    }
    // GB9b: Prepend x
    if zustand.vorher == Prepend {
        return false;
    }
    // GB9c: Consonant (Extend|Linker)* Linker (Extend|Linker)* x Consonant
    if incb_wert == Incb::Consonant && zustand.incb_kette == IncbKette::MitLinker {
        return false;
    }
    // GB11: Extended_Pictographic Extend* ZWJ x Extended_Pictographic
    if ep && zustand.ep_zwj {
        return false;
    }
    // GB12/GB13: (sot | [^RI]) (RI RI)* RI x RI. Der Lauf endet am
    // vorigen Zeichen; ist er ungerade, bildet das neue RI ein Paar.
    // BEIDE Zeichen muessen Regionale Indikatoren sein — sonst wuerde
    // ein einzelnes RI jede folgende Grenze verschlucken.
    if klasse == RegionalIndicator
        && zustand.vorher == RegionalIndicator
        && zustand.ri_lauf % 2 == 1
    {
        return false;
    }
    // GB999: sonst Bruch.
    true
}

/// Schreibt das Zeichen in den Zustand fort. Muss NACH [`ist_bruch`]
/// aufgerufen werden, weil die Regeln den Zustand VOR dem neuen Zeichen
/// brauchen.
fn fortschreiben(zustand: &mut Zustand, klasse: Graphemklasse, incb_wert: Incb, ep: bool) {
    use Graphemklasse::*;

    // GB12/GB13: RI-Lauf. Nur Regionale Indikatoren verlaengern ihn.
    if klasse == RegionalIndicator {
        zustand.ri_lauf += 1;
    } else {
        zustand.ri_lauf = 0;
    }

    // GB11: `EP Extend*`. Beide neuen Werte werden aus dem ALTEN
    // `ep_kette` gebildet; `neu_ep_zwj` also vor dem Ueberschreiben.
    let neu_ep_kette = ep || (klasse == Extend && zustand.ep_kette);
    let neu_ep_zwj = klasse == Zwj && zustand.ep_kette;
    zustand.ep_kette = neu_ep_kette;
    zustand.ep_zwj = neu_ep_zwj;

    // GB9c: InCB-Kette. `Extend` verlaengert eine offene Kette, `None`
    // loescht sie (auch ein GCB=Extend-Zeichen ohne InCB=Extend gehoert
    // nicht zur Kette — GB9 haelt es trotzdem am Cluster).
    zustand.incb_kette = match incb_wert {
        Incb::Consonant => IncbKette::Nachkonsonant,
        Incb::Linker => match zustand.incb_kette {
            IncbKette::Nachkonsonant | IncbKette::MitLinker => IncbKette::MitLinker,
            IncbKette::Leer => IncbKette::Leer,
        },
        Incb::Extend => zustand.incb_kette,
        Incb::None => IncbKette::Leer,
    };

    zustand.vorher = klasse;
}

/// Durchlaeuft die Clustergrenzen von vorn, ohne sie zwischenzuspeichern.
///
/// Liefert `0`, dann jede Bruchstelle (Byte-Offset des Zeichens, mit dem
/// der naechste Cluster beginnt) und zuletzt `text.len()`. Fuer leeren
/// Text ist das genau ein Wert: `0` (= `text.len()`).
struct GrenzenIter<'a> {
    text: &'a str,
    zeichen: std::str::CharIndices<'a>,
    zustand: Zustand,
    /// Ist das naechste Zeichen das erste? Dann gibt es keine Regel
    /// anzuwenden (GB1 hat die Grenze 0 schon gemeldet).
    erstes_zeichen: bool,
    start_gemeldet: bool,
    ende_gemeldet: bool,
}

impl<'a> GrenzenIter<'a> {
    fn neu(text: &'a str) -> GrenzenIter<'a> {
        GrenzenIter {
            text,
            zeichen: text.char_indices(),
            zustand: Zustand::neu(),
            erstes_zeichen: true,
            start_gemeldet: false,
            ende_gemeldet: false,
        }
    }
}

impl Iterator for GrenzenIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if !self.start_gemeldet {
            self.start_gemeldet = true;
            if self.text.is_empty() {
                // Anfang und Ende fallen zusammen; nur EIN Wert.
                self.ende_gemeldet = true;
            }
            return Some(0);
        }
        if self.ende_gemeldet {
            return None;
        }

        for (offset, ch) in self.zeichen.by_ref() {
            let klasse = graphem_klasse(ch);
            let incb_wert = incb(ch);
            let ep = ist_extended_pictographic(ch);

            let bruch = if self.erstes_zeichen {
                false
            } else {
                ist_bruch(&self.zustand, klasse, incb_wert, ep)
            };
            self.erstes_zeichen = false;
            fortschreiben(&mut self.zustand, klasse, incb_wert, ep);

            if bruch {
                return Some(offset);
            }
        }

        // GB2: das Textende ist immer eine Grenze.
        self.ende_gemeldet = true;
        Some(self.text.len())
    }
}

/// Liefert die Byte-Offsets aller Graphem-Clustergrenzen.
///
/// Der erste Wert ist immer 0, der letzte immer `text.len()`; die Werte
/// sind streng aufsteigend. Fuer leeren Text ist das Ergebnis `[0]`.
///
/// Harte Obergrenze (M3): hoechstens `text.len() + 1` Eintraege, weil
/// jede Grenze auf einem Zeichenanfang liegt und ein Zeichen mindestens
/// ein Byte belegt. Der Vergleich `grenzen.len() >= max_grenzen` ist die
/// zugesicherte Abbruchbedingung; er greift nachweislich nie, weil der
/// Iterator genau diese Grenze einhaelt.
pub fn graphem_grenzen(text: &str) -> Vec<usize> {
    let max_grenzen = text.len().saturating_add(1); // harte Obergrenze (M3)
    let mut grenzen = Vec::new();
    for grenze in GrenzenIter::neu(text) {
        if grenzen.len() >= max_grenzen {
            break;
        }
        grenzen.push(grenze);
    }
    grenzen
}

/// Naechste Clustergrenze strikt nach `byte_pos` (hoechstens `text.len()`).
///
/// `byte_pos` darf mitten in einem Zeichen oder hinter dem Textende
/// liegen; die Funktion panickt nie. Ab dem Textende gibt es keine
/// Grenze mehr, dann kommt `text.len()` zurueck.
///
/// Laufzeit: linear bis zur gesuchten Stelle (hoechstens
/// `text.len() + 1` Zeichen), ohne Allokation.
pub fn naechste_position(text: &str, byte_pos: usize) -> usize {
    let ziel = byte_pos.min(text.len());
    for grenze in GrenzenIter::neu(text) {
        if grenze > ziel {
            return grenze;
        }
    }
    text.len()
}

/// Vorige Clustergrenze strikt vor `byte_pos` (mindestens 0).
///
/// Fuer `byte_pos == 0` und mitten im ersten Cluster ist das Ergebnis 0;
/// `byte_pos` darf auch hinter dem Textende liegen (dann zaehlt das
/// Textende als Obergrenze). Kein Panic.
///
/// Laufzeit: linear bis zur gesuchten Stelle (hoechstens
/// `text.len() + 1` Zeichen), ohne Allokation.
pub fn vorige_position(text: &str, byte_pos: usize) -> usize {
    let ziel = byte_pos.min(text.len());
    let mut letzte = 0usize;
    for grenze in GrenzenIter::neu(text) {
        if grenze >= ziel {
            break;
        }
        letzte = grenze;
    }
    letzte
}
