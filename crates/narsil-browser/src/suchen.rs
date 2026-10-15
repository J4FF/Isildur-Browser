//! Suche auf der Seite: Treffer-Positionen im Text.
//!
//! Reine Textarbeit ohne Fenster: `suchen` liefert zu jedem Treffer die
//! Byte-Positionen (an Zeichengrenzen, also direkt schneidbar) und die
//! Zeilennummer (1-basiert). Ueberlappend wird nicht gesucht — der
//! naechste Treffer beginnt nach dem vorigen, wie bei wiederholtem
//! Weitersuchen im Browser.

// ---------------------------------------------------------------------------
// Modul-Intern
// ---------------------------------------------------------------------------

/// Hoechstens so viele Treffer je Suche.
///
/// BEGRUENDUNG (M3): Eine Seite mit mehr als 1000 Treffern braucht keine
/// Liste, sondern ein schmaleres Suchwort. Die Schleife endet hier,
/// egal wie oft das Muster vorkommt — `a` in 1 Mio `a` liefert 1000
/// Treffer in Millisekunden statt 1 Mio Eintraege.
pub const MAX_TREFFER: usize = 1000;
/// Hoechstens so viele Zeichen im Suchmuster (1024: jeder echte Such-
/// begriff passt, Bomben aus `a`-Wiederholungen bleiben draussen).
pub const MAX_MUSTER_ZEICHEN: usize = 1024;

/// EIN Treffer: wo er steht und in welcher Zeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Treffer {
    /// Byte-Offset des Anfangs im Originaltext (Zeichengrenze).
    pub start: usize,
    /// Byte-Offset des Endes im Originaltext (Zeichengrenze).
    pub ende: usize,
    /// Zeilennummer ab 1 (`\n` zaehlt).
    pub zeile: u32,
}

/// Sucht `muster` in `text`, von vorn, nicht ueberlappend.
///
/// - Leeres Muster oder Muster ueber [`MAX_MUSTER_ZEICHEN`]: keine
///   Treffer (statt Endlosschleife oder Sekunden-Arbeit).
/// - `ignoriere_gross_klein`: Kleinbuchstaben-Vergleich pro Zeichen
///   (ohne Locale-Sonderfaelle — tuerkisches `İ` faellt auf `i`
///   zurueck, dokumentiert statt geraten).
/// - Hoechstens [`MAX_TREFFER`] Treffer (M3).
/// - Positionen sind Byte-Offsets an Zeichengrenzen: `&text[s..e]` ist
///   der Treffer selbst und panikt nie.
pub fn suchen(text: &str, muster: &str, ignoriere_gross_klein: bool) -> Vec<Treffer> {
    if muster.is_empty() || muster.chars().count() > MAX_MUSTER_ZEICHEN {
        return Vec::new();
    }
    if !ignoriere_gross_klein {
        // Byte-exakt: `match_indices` auf str liefert nur Treffer an
        // Zeichengrenzen, in linearer Zeit (M3: frueher Abbruch bei
        // MAX_TREFFER, Zeilen werden je nur einmal gezaehlt).
        let mut raus: Vec<Treffer> = Vec::new();
        let mut zeile = 1u32;
        let mut pos = 0usize;
        for (start, teil) in text.match_indices(muster) {
            if raus.len() >= MAX_TREFFER {
                break;
            }
            zeile += text[pos..start].bytes().filter(|&b| b == b'\n').count() as u32;
            pos = start;
            raus.push(Treffer {
                start,
                ende: start + teil.len(),
                zeile,
            });
        }
        return raus;
    }
    // Gross/Klein egal: Text einmal klein falten und merken, welches
    // Klein-Byte zu welchem Original-Byte gehoert. Klein-Schreibung kann
    // die Byte-Laenge aendern (selten), darum die Karte statt Rechnens.
    let mut klein = String::with_capacity(text.len());
    let mut karte: Vec<(u32, u32)> = Vec::new();
    for (versatz, zeichen) in text.char_indices() {
        karte.push((klein.len() as u32, versatz as u32));
        for kleinbuchstabe in zeichen.to_lowercase() {
            klein.push(kleinbuchstabe);
        }
    }
    karte.push((klein.len() as u32, text.len() as u32));
    let nadel: String = muster.chars().flat_map(|c| c.to_lowercase()).collect();
    if nadel.is_empty() {
        return Vec::new();
    }
    let mut zeilenanfaenge: Vec<u32> = vec![0];
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            zeilenanfaenge.push(i as u32 + 1);
        }
    }
    let mut raus: Vec<Treffer> = Vec::new();
    for (klein_start, teil) in klein.match_indices(nadel.as_str()) {
        if raus.len() >= MAX_TREFFER {
            break;
        }
        let start = original_versatz(&karte, klein_start as u32) as usize;
        let ende = original_versatz(&karte, (klein_start + teil.len()) as u32) as usize;
        // Zeile = Zahl der Anfaenge <= start (binaer, nicht linear).
        let zeile = zeilenanfaenge.partition_point(|&z| z <= start as u32) as u32;
        raus.push(Treffer { start, ende, zeile });
    }
    raus
}

/// Klein-Offset zurueck in den Original-Offset (binaere Suche in der
/// Falt-Karte; faellt ein Treffer in eine mehr-zeichen-Faltung, gilt der
/// Original-Anfang — immer noch eine Zeichengrenze, nie ein Panic).
fn original_versatz(karte: &[(u32, u32)], klein: u32) -> u32 {
    let mut i = karte.partition_point(|&(k, _)| k <= klein);
    if i >= karte.len() {
        i = karte.len() - 1;
    }
    // Auf mehr-zeichen-Faltungen zurueckgehen, bis die Karte passt.
    while i > 0 && karte[i].0 > klein {
        i -= 1;
    }
    karte[i].1
}
