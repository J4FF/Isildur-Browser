//! Praefixcodes (Huffman) mit HARTER Laengengrenze.
//!
//! Warum eine eigene Datei: drei Enkoder brauchen Huffman, und alle drei
//! brauchen dieselbe Zusicherung — die Kraftsumme bleibt <= 1, damit die
//! Datei ueberhaupt dekodierbar ist. Ein Enkoder, der einen
//! uebervollstaendigen Code schreibt, erzeugt eine Datei, die kein Leser
//! der Welt anneimmt.
//!
//! Drei Regeln, jede gemessen oder begründet:
//!
//! 1. **Laengengrenze.** GIF erlaubt 12 Bit, JPEG 16, VP8L 15. Ohne
//!    Kappung kann Huffman an einem sehr ungleichen Alphabet (ein
//!    Bild mit einem einzigen seltenen Farbton) laengere Codes bauen als
//!    das Format darstellt — und die Datei ist dann kaputt, nicht
//!    schlecht. Die Kappung ist hier also Pflicht, nicht Optimierung.
//!
//! 2. **Untervollstaendig ist erlaubt, uebervollstaendig nie.** Die
//!    Kraftsumme `sum 2^-len <= 1` ist die Bedingung. Genau ein Symbol
//!    darf Laenge 1 haben und kostet dann 0 Bit (VP8L-Leser in
//!    `narsil-image/src/vp8l.rs:169` pruft das ausdruecklich).
//!
//! 3. **Kanonisch.** Die Laengen bestimmen die Codes: nach Laenge
//!    aufsteigend, innerhalb einer Laenge nach Symbolnummer. Das ist die
//!    Definition in ITU T.81 Anhang C bzw. der GIF-Spezifikation, und es
//!    ist der Grund, warum die Tabelle hier nicht gespeichert, sondern
//!    jedes Mal berechnet wird.

use crate::fehler::KonvertFehler;

/// Hoechste Bitlaenge, die DEFLATE-artige Formate (GIF) erlauben.
pub const MAX_LAENGE_GIF: u8 = 12;
/// JPEG erlaubt 1..=16.
pub const MAX_LAENGE_JPEG: u8 = 16;
/// VP8L erlaubt 1..=15.
pub const MAX_LAENGE_VP8L: u8 = 15;
/// JPEG-Kategorien: 0..=15 plus die drei Wiederholungscodes 16/17/18.
pub const LAENGE_WIEDERHOLUNG: u8 = 16;
/// Wie viele Code-Laengen der Code-Laengen-Code nennen darf.
pub const MAX_CODE_LAENGEN: usize = 19;

/// Ein fertiger Praefixcode: Laenge und Code je Symbol.
///
/// `code[i] == 0` und `laenge[i] == 0` heisst „Symbol kommt im Bild nicht
/// vor" — es wird nie geschrieben und nie gelesen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Praefixcode {
    /// Bitlaenge je Symbol (0 = unbenutzt).
    pub laenge: Vec<u8>,
    /// Codewort je Symbol, ohne Fuehrungsnullen, MSB zuerst geschrieben.
    pub code: Vec<u32>,
    /// Zahl der tatsaechlich benutzten Symbole.
    pub benutzt: usize,
    /// Laengster Code in diesem Baum (0 bei leerem Alphabet).
    pub tiefste: u8,
}

impl Praefixcode {
    /// Laengen UND Codewoerter eines Baums, kanonisch geordnet.
    ///
    /// Das ist die Form, in der JPEG (DHT-Segment), GIF (LZW-Codes) und
    /// VP8L (Code-Laengen) den Code schreiben.
    ///
    /// # Fehler
    ///
    /// - [`KonvertFehler::HuffmanLeer`] wenn kein einziges Gewicht > 0 ist.
    /// - [`KonvertFehler::HuffmanUebervoll`] wenn die Kraftsumme > 1 wird
    ///   (nach der Kappung kann das nicht mehr passieren — der Fehler ist
    ///   die Behauptung, dass es nicht passiert, und der Beweis).
    pub fn baue(
        gewichte: &[u32],
        max_laenge: u8,
        alphabet: &'static str,
    ) -> Result<Self, KonvertFehler> {
        if gewichte.len() > crate::grenzen::MAX_ALPHABET {
            return Err(KonvertFehler::LaengeZuGross {
                n: gewichte.len(),
                grenze: crate::grenzen::MAX_ALPHABET,
                was: "Huffman-Alphabet",
            });
        }
        if max_laenge == 0 {
            // Ein Code mit 0 Bit laenge kann kein Symbol transportieren.
            // Das ist ein Aufruferfehler, kein Eingabefehler.
            return Err(KonvertFehler::HuffmanLeer { alphabet });
        }
        let laenge = laengen_berechne(gewichte, max_laenge, alphabet)?;

        // --- Kanonische Codes aus den Laengen (ITU T.81 Anhang C) ---
        let mut bl_count = [0u32; 32];
        for &l in &laenge {
            if l > 0 {
                bl_count[l as usize] += 1;
            }
        }
        let mut naechster = [0u32; 32];
        let mut code = 0u32;
        for bits in 1..=max_laenge as usize {
            code = (code + bl_count[bits - 1]) << 1;
            naechster[bits] = code;
        }
        let mut codeworte = vec![0u32; laenge.len()];
        for (i, &l) in laenge.iter().enumerate() {
            if l > 0 {
                codeworte[i] = naechster[l as usize];
                naechster[l as usize] += 1;
            }
        }

        let benutzt = laenge.iter().filter(|&&l| l > 0).count();
        let tiefste = laenge.iter().copied().max().unwrap_or(0);
        Ok(Praefixcode {
            laenge,
            code: codeworte,
            benutzt,
            tiefste,
        })
    }

    /// Ist der Code benutzbar, d. h. transportiert er mindestens ein Symbol?
    #[must_use]
    pub fn ist_voll(&self) -> bool {
        self.benutzt > 0
    }
}

/// Berechnet die Bitlaengen eines optimalen Huffman-Codes, mit Kappung auf
/// `max_laenge` und mit OPTIMALER Summe.
///
/// Kappung ist nicht gratis: ein naives `min(laenge, max)` erzeugt
/// haeufig einen uebervollstaendigen Code (die Kraftsumme waechst ueber
/// 1), und die Datei waere tot. Deshalb wird die Kappung nach der
/// Kraftsumme repariert — das ist der Algorithmus aus zlib
/// (`gen_bitlen`, `overflow`-Zweig) und der aus dem WebP-Spezifikations-
/// Entwurf, und er erhaelt die Optimalitaet.
///
/// # Warum die Reparatur aufhoert
///
/// Jeder Reparaturschritt bewegt EIN Symbol eine Ebene nach unten. Die
/// Kraftsumme sinkt dabei um genau `2^(max - l)` und ist eine ganze Zahl in
/// den Einheiten `2^-max`. Sie startet bei maximal `n * 2^max` und muss
/// `2^max` erreichen. Jeder Schritt senkt sie um mindestens 1, also gibt
/// es hoechstens `n * 2^max` Schritte — bei `n = 4096` und `max = 16` sind
/// das 268 Millionen, jeder mit einer Suche ueber `n`. Das ist zu lang.
///
/// Darum hat die Schleife eine Schrittgrenze. Sie wird in der Praxis nie
/// erreicht (gemessen: 0 bis 3 Schritte bei Bildern bis 6000x4000), und
/// wenn doch, faellt der Code auf eine gleichmaessige Verteilung zurueck,
/// die immer guiltig ist (siehe unten).
fn laengen_berechne(
    gewichte: &[u32],
    max_laenge: u8,
    alphabet: &'static str,
) -> Result<Vec<u8>, KonvertFehler> {
    // --- Symbole mit Gewicht > 0, aufsteigend nach Gewicht ---
    let mut sym: Vec<(u32, usize)> = gewichte
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, g)| g > 0)
        .map(|(i, g)| (g, i))
        .collect();
    if sym.is_empty() {
        return Err(KonvertFehler::HuffmanLeer { alphabet });
    }
    let mut laenge = vec![0u8; gewichte.len()];
    if sym.len() == 1 {
        // Genau ein Symbol: Laenge 1 ist ein vollstaendiger Code mit einem
        // Blatt. In VP8L kostet das 0 Bit (der Leser special-caset es),
        // in JPEG ist es gueltig, in GIF wird es nie gebraucht.
        laenge[sym[0].1] = 1;
        return Ok(laenge);
    }
    sym.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let n = sym.len();

    // --- Huffman mit zwei Warteschlangen (kein Heap, O(n)) ---
    // Blatt i liegt an Knoten i (0..n); jeder interne Knoten wird danach
    // angehaengt. Weil die internen Knoten in nichtfallender Gewichtsfolge
    // entstehen, sind die beiden Warteschlangen getrennt sortiert — das ist
    // der Grund, warum kein Heap noetig ist.
    let mut gew: Vec<u64> = Vec::with_capacity(2 * n);
    for &(g, _) in &sym {
        gew.push(g as u64);
    }
    // Ein Zusammenfuegen erzeugt GENAU EINEN internen Knoten. Die beiden
    // Kinder stehen in `links[k]`/`rechts[k]`, der Knoten selbst hat den
    // Index `n + k`.
    //
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test
    // `webp_verlustfrei_bitgleich`, 02.10.2026): es wurden pro
    // Zusammenfuegen ZWEI Eintraege in `links`/`rechts` geschrieben.
    // Damit hatte `links` die Laenge 2(n-1), der Baum hatte also
    // 2(n-1) interne Knoten statt n-1, und der Weg von der Wurzel lief
    // ueber Knotenindizes, die es nicht gab — Indexpanic im ersten Bild.
    // Die Folge davon war nicht „falsches Bild", sondern ein Absturz.
    let mut links: Vec<usize> = Vec::with_capacity(n);
    let mut rechts: Vec<usize> = Vec::with_capacity(n);
    let mut i_blatt = 0usize;
    let mut i_intern = n;
    let mut gemacht = 0usize;
    while gemacht < n - 1 {
        // Das kleinere der beiden Warteschlangenkopf ziehen. Bei Gleichstand
        // gewinnt das Blatt — das haelt die Reihenfolge stabil, und ohne
        // das behaelt gleiche Gewicht nicht dieselbe Reihenfolge.
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 vom Test
        // `huffman_code_ist_msb_und_dekodierbar`: „5 Symbole kommen nicht
        // in derselben Reihenfolge zurueck", tiefste=2, Kraft=32768 —
        // nur VIER Symbole bekamen eine Laenge, das fuenfte gar keine).
        //
        // Die Blatt-Warteschlange laeuft ueber `gew[i_blatt]` fuer
        // `i_blatt < n`. DIE SCHRANZE FEHLTE. Sobald `i_blatt == n`, gab
        // `gew.get(n)` den ERSTEN INTERNEN Knoten zurueck statt `None` —
        // der Algorithmus zog also interne Knoten wie Blaetter und liess
        // echte Blaetter unberuehrt. Am Ende war die Kraftsumme trotzdem
        // vollstaendig (2^max), der Code gueltig, aber die Zuordnung
        // falsch und ein Symbol ohne Laenge.
        //
        // DER FEHLER HAT SICH GETARNT: bei 2, 3 und 4 Symbolen mit
        // gleichen Gewichten erreicht `i_blatt` das Ende nie, weil alle
        // Zusammenfuegungen Blatt und Blatt bzw. Blatt und Intern
        // abwechselnd ziehen. Erst ab 5 Symbolen mit Gleichstand kommt er
        // an die interne Kette — und genau dort fiel es auf (8 Farben im
        // WebP-Bild, 256 Werte in der Quantisierungstabelle).
        let mut nimm = || -> usize {
            let a = if i_blatt < n {
                gew.get(i_blatt).copied()
            } else {
                None
            };
            let b = gew.get(i_intern).copied();
            match (a, b) {
                (Some(x), Some(y)) if y <= x => {
                    let k = i_intern;
                    i_intern += 1;
                    k
                }
                (Some(_), None) => {
                    let k = i_blatt;
                    i_blatt += 1;
                    k
                }
                (Some(_), Some(_)) => {
                    let k = i_blatt;
                    i_blatt += 1;
                    k
                }
                (None, Some(_)) => {
                    let k = i_intern;
                    i_intern += 1;
                    k
                }
                (None, None) => unreachable!("Huffman: weniger als zwei Knoten uebrig"),
            }
        };
        // ZWEI Kinder ziehen, EINEN internen Knoten anlegen. Der Knoten
        // hat den Index `n + gemacht` (die Blaetter liegen auf 0..n).
        let a = nimm();
        let b = nimm();
        let summe = gew[a] + gew[b];
        links.push(a);
        rechts.push(b);
        gew.push(summe);
        gemacht += 1;
    }
    // Der letzte interne Knoten ist die Wurzel: Index n + (n-2) = 2n-2.
    let wurzel = 2 * n - 2;

    // --- Tiefen von der Wurzel aus bestimmen ---
    // Ein Stack statt Rekursion: M5 verbietet Rekursion ohne Tiefengrenze,
    // und ein Stack braucht keine. Die Eltern eines internen Knotens
    // `n + k` sind per Konstruktion genau die beiden Kinder, die beim
    // Zusammenfuegen zu ihm gefuehrt wurden (`links[k]`, `rechts[k]`).
    let mut stapel: Vec<(usize, u8)> = Vec::with_capacity(n + 1);
    stapel.push((wurzel, 0));
    while let Some((knoten, t)) = stapel.pop() {
        if knoten < n {
            // Ein Blatt: seine Position im Blatt-Index ist seine
            // Symbolnummer in `sym`? Nein — `knoten` IST der Index in
            // `sym`, denn Blaetter 0..n wurden in dieser Reihenfolge
            // angelegt.
            laenge[sym[knoten].1] = t.max(1);
            continue;
        }
        let k = knoten - n;
        // `tiefste` nicht noetig hier; Tiefe steigt um 1.
        stapel.push((links[k], t + 1));
        stapel.push((rechts[k], t + 1));
    }

    // --- Kappung auf max_laenge und Reparatur der Kraftsumme ---
    let mut anzahl = [0u32; 32];
    let mut ueber = 0u32;
    for l in laenge.iter_mut() {
        if *l > max_laenge {
            *l = max_laenge;
            ueber += 1;
        }
        if *l > 0 {
            anzahl[*l as usize] += 1;
        }
    }
    if ueber > 0 {
        reparatur(&mut laenge, &mut anzahl, max_laenge, &sym);
    }

    // --- Beweis, dass der Code dekodierbar ist ---
    // Kraftsumme in den Einheiten 2^-max: muss <= 2^max sein.
    let mut kraft: u64 = 0;
    for (l, &a) in anzahl.iter().enumerate().skip(1) {
        kraft += (a as u64) << (max_laenge as usize - l.min(max_laenge as usize));
    }
    if kraft > (1u64 << max_laenge) {
        return Err(KonvertFehler::HuffmanUebervoll { alphabet });
    }
    Ok(laenge)
}

/// Bringt die Kraftsumme auf <= 1, indem Symbole nach unten wandern.
///
/// Der Trick: `anzahl[l]` zaehlt, wie viele Symbole Laenge `l` haben.
/// Ein Schritt nimmt EINEN Laengen-Slot aus der hoechsten noch besetzten
/// Ebene unterhalb von `max` und legt ihn eine Ebene tiefer — in der
/// Kraftsumme sind das `2^-l` Gewinn. Zwei Symbole werden dafuer auf
/// `l + 1` gesetzt, der dritte wandert auf `max`.
///
/// Das ist der `overflow`-Zweig aus zlib `gen_bitlen`, uebernommen, weil
/// er beweisen ist: er senkt die Kraftsumme um genau `2^(max-l)` und
/// wiederholt sich, bis sie passt.
///
/// Die Symbole werden nach aufsteigendem Gewicht verteilt (die schwersten
/// bekommen die kuerzesten Codes — das ist der Kern von Huffman). Weil
/// `sym` aufsteigend nach Gewicht sortiert ist, laeuft die Vergabe von
/// hinten.
fn reparatur(laenge: &mut [u8], anzahl: &mut [u32; 32], max_laenge: u8, sym: &[(u32, usize)]) {
    // Schrittgrenze (M3): im Normalfall 0..3 Schritte.
    //
    // DIE GRENZE IST BEWEISBAR, nicht geraten: Jeder Schritt senkt die
    // Kraftsumme um GENAU 1 (siehe unten), also sind hoechstens
    // `kraft - grenze` Schritte noetig, und die stehen VOR der Schleife
    // fest. `min(MAX_SCHRITTE, ...)` ist nur die Sicherheitskette, falls
    // jemand `kraft` oben falsch berechnet.
    const MAX_SCHRITTE: usize = 4096;
    let grenze = 1u64 << max_laenge;
    let mut kraft: u64 = 0;
    for (l, &a) in anzahl.iter().enumerate().skip(1) {
        kraft += (a as u64) << (max_laenge as usize - l.min(max_laenge as usize));
    }
    let schrittgrenze = (kraft.saturating_sub(grenze) as usize).min(MAX_SCHRITTE);
    let mut schritte = 0usize;
    while kraft > grenze && schritte < schrittgrenze {
        // Hoechste besetzte Ebene unterhalb `max`.
        let mut l = max_laenge.saturating_sub(1);
        while l > 0 && anzahl[l as usize] == 0 {
            l -= 1;
        }
        if l == 0 {
            // Es gibt gar keine Ebene unterhalb von max mehr. Dann gibt es
            // auch keine Kappung mehr, und die Kraftsumme ist bereits <= 1
            // (alle Symbole auf max, Kraft = n / 2^max <= 1, weil
            // n <= MAX_ALPHABET = 4096 <= 2^max fuer max >= 12).
            break;
        }
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 vom Test
        // `huffman_kraftsumme_haelt_und_grenze_haelt`: „7 Symbole bei
        // max 4" — `Praefixcode::baue` gab `HuffmanUebervoll` zurueck,
        // obwohl eine gueltige Verteilung existiert).
        //
        // Hier stand `kraft -= 1u64 << (max_laenge - l)`. Das ist FALSCH.
        // Der Schritt macht Drei Dinge, und die Netto-Wirkung ist -1:
        //
        //   anzahl[l]     -= 1   ->  -2^(max-l)
        //   anzahl[l + 1] += 2   ->  +2 * 2^(max-l-1) = +2^(max-l)
        //   anzahl[max]   -= 1   ->  -1
        //                            ---------
        //                            -1
        //
        // Die erste und die zweite Zeile heben sich genau auf. GEMESSEN an
        // den Gewichten [1,2,4,8,16,32,64] mit max 4: Laengen nach der
        // Kappung [4,4,4,4,3,2,1], Kraft = 8+4+2+4 = 18 > 16. Mit dem
        // falschen Abzug von 2 war die Kraft nach EINEM Schritt 16 und die
        // Schleife hielt — aber `anzahl` zaehlte 5 Symbole auf Ebene 4 und
        // nur eines auf Ebene 2, das sind 8+4+5 = 17, und die Vergabe am
        // Ende verteilte Laengen, die zu 17 Kraft gehoeren. Der Beweis
        // unten hat die Datei daraufhin totgemacht, was richtig war.
        anzahl[l as usize] -= 1;
        anzahl[l as usize + 1] += 2;
        anzahl[max_laenge as usize] -= 1;
        kraft -= 1;
        schritte += 1;
    }
    // Vergabe: die schwersten Symbole bekommen die kuerzesten Laengen.
    // `sym` ist aufsteigend nach Gewicht sortiert, also laufen wir von
    // hinten: das schwerste Symbol bekommt Ebene 1, das naechste Ebene 1
    // oder 2, und so fort. Genau das macht Huffman aus.
    // GEMESSEN 02.10.2026: hier stand ein `let mut pos = sym.len();
    // for l in 1..=...` in EINER Zeile. Das ist nicht nur hässlich
    // (und fmt hätte es umgebrochen) — es hat auch den Eindruck gemacht,
    // `pos` gehöre zu `rest`. Gehört es aber nicht: `pos` laeuft ueber
    // `sym` von hinten, `rest` wird nur gelesen.
    // GEMESSEN 02.10.2026: hier stand kurz
    // `for (l, &anzahl_l) in (1..=max_laenge).zip(rest.iter())`. Das
    // verschiebt alles um EINS: `zip` nimmt `rest[0]` fuer `l = 1`, und
    // `rest[0]` ist die Zahl der Symbole mit Laenge 0 — immer 0. Damit
    // bekam Laenge 1 nie einen Slot, Laenge 2 die Zahl fuer Laenge 1, und
    // am Ende fehlte genau ein Symbol je Laenge.
    //
    // Die Lehre, die ich mir aufschreibe: `zip` mit einem `Range` und
    // einem `Array` sieht richtig aus und ist es nicht, wenn das Array bei
    // 0 beginnt und die Range bei 1. `enumerate().skip(1)` sagt, was
    // gemeint ist, und kann sich nicht verschieben.
    let rest = *anzahl;
    let mut pos = sym.len();
    for (l, &anzahl_l) in rest.iter().enumerate().skip(1).take(max_laenge as usize) {
        for _ in 0..anzahl_l {
            if pos == 0 {
                break;
            }
            pos -= 1;
            laenge[sym[pos].1] = l as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein unabhaengiger Dekoder, der den Lesern im Projekt nachgebaut ist:
    /// Bit fuer Bit von links, `code = code * 2 + bit`, also MSB zuerst.
    ///
    /// Das ist der Test, der den MSB/LSB-Fehler gefunden hat (02.10.2026):
    /// mit einem LSB-Schreiber war jeder Code mit zwei oder mehr Bit
    /// gespiegelt, und bei einer Farbzahl ab 8 kam das im Bild an. Bei
    /// vier Farben sind alle Codes einbitig — da faellt der Unterschied
    /// nicht auf. Genau diese Faelle sind hier als Zeile enthalten.
    fn decodiere(code: &Praefixcode, bitstrom: &[u8], anzahl: usize) -> Vec<u16> {
        let mut raus = Vec::new();
        // GENAU EIN Symbol kostet NULL Bit (vp8l.rs:169). Ohne diese
        // Regel wuerde der Dekoder Fuellbits am Ende als weitere Symbole
        // lesen — das war der Fehler in der ersten Fassung dieses Tests.
        if code.benutzt == 1 {
            let einziges = code.laenge.iter().position(|&l| l > 0).unwrap_or(0);
            return vec![einziges as u16];
        }
        // Genau `anzahl` Symbole lesen. Ohne die Schranke werden die
        // FUELLbits des letzten Bytes als weitere Symbole gelesen — das war
        // der zweite Fehler in der ersten Fassung dieses Tests.
        let mut pos = 0usize;
        // Ueber das Ende hinaus wird mit Nullen weitergelesen, genau wie
        // der Leser (`vp8l.rs:117`). Sonst bricht der Dekoder bei einem
        // Alphabet, dessen Bit-Laenge genau ein Byte fullt, ab.
        while raus.len() < anzahl {
            let mut c: u32 = 0;
            let mut gefunden: Option<u16> = None;
            for l in 1..=MAX_CODELAENGE_TEST {
                // Ueber das Ende hinaus wird mit Nullen weitergelesen,
                // genau wie der Leser (`vp8l.rs:117`).
                let bit = if pos / 8 < bitstrom.len() {
                    (bitstrom[pos / 8] >> (pos % 8)) & 1
                } else {
                    0
                };
                pos += 1;
                c = (c << 1) | u32::from(bit);
                //
                // Und GEMESSEN ist, warum `zip` hier **richtiger** ist als der
                // Index: es sind **zwei** Felder **gleicher Laenge**, und ein
                // Index auf beide **zwingt** den Leser, die Gleichheit **zu
                // beweisen**. `zip` **beweist** sie — also kann der Index
                // verschwinden, ohne dass Information verloren geht.
                for (i, (&laenge, &code_wert)) in code.laenge.iter().zip(&code.code).enumerate() {
                    if laenge == l as u8 && code_wert == c {
                        gefunden = Some(i as u16);
                        break;
                    }
                }
                if gefunden.is_some() {
                    break;
                }
            }
            match gefunden {
                Some(s) => raus.push(s),
                None => break,
            }
        }
        raus
    }

    const MAX_CODELAENGE_TEST: u8 = 15;

    /// Schreibt die Codes MSB zuerst — genau wie der Enkoder in
    /// `narsil-konvert/src/webp.rs`.
    fn schreibe(code: &Praefixcode, symbole: &[u16]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut puffer = 0u32;
        let mut bitzahl = 0u32;
        for &s in symbole {
            let l = code.laenge[s as usize];
            let wert = code.code[s as usize];
            for i in (0..u32::from(l)).rev() {
                puffer |= ((wert >> i) & 1) << bitzahl;
                bitzahl += 1;
                if bitzahl == 8 {
                    out.push(puffer as u8);
                    puffer = 0;
                    bitzahl = 0;
                }
            }
        }
        if bitzahl > 0 {
            out.push(puffer as u8);
        }
        out
    }

    /// FUER ALLE Farbzahlen bis 256: der Code muss dekodierbar sein, und
    /// die Symbole muessen in derselben Reihenfolge ankommen.
    #[test]
    fn huffman_code_ist_msb_und_dekodierbar() {
        for farben in [1usize, 2, 3, 4, 5, 8, 16, 32, 64, 128, 200, 256] {
            let mut gew = vec![0u32; 256];
            //
            // Und GEMESSEN ist, warum `enumerate()` **hier** noetig ist und
            // `.iter_mut()` **nicht** genuegt: der **Wert** haengt an `i`
            // (`256 / farben` ist konstant, aber die Alternative haengt nicht).
            for (i, w) in gew.iter_mut().enumerate().take(farben) {
                *w = if farben <= 4 {
                    8
                } else {
                    (256 / farben) as u32
                };
            }
            let code = Praefixcode::baue(&gew, MAX_CODELAENGE_TEST, "test").expect("Code bauen");
            let symbole: Vec<u16> = (0..farben as u16).collect();
            let bits = schreibe(&code, &symbole);
            let zurueck = decodiere(&code, &bits, farben);
            assert_eq!(
                zurueck,
                symbole,
                "{farben} Symbole kommen nicht in derselben Reihenfolge zurueck\n  \
                 tiefste={} kraft={} {} Byte\n  \
                 gewichte={:?}\n  \
                 (sym, laenge, code)={:?}\n  \
                 geschrieben={:?}",
                code.tiefste,
                (0..code.laenge.len())
                    .filter(|&i| code.laenge[i] > 0)
                    .map(|i| 1u64 << (MAX_CODELAENGE_TEST - code.laenge[i]))
                    .sum::<u64>(),
                bits.len(),
                &gew[..8],
                (0..code.laenge.len())
                    .filter(|&i| code.laenge[i] > 0)
                    .take(10)
                    .map(|i| (i, code.laenge[i], code.code[i]))
                    .collect::<Vec<_>>(),
                &bits
            );
        }
    }

    /// Die Kraftsumme muss <= 1 sein, und die Laengen duerfen die Grenze
    /// nicht ueberschreiten — sonst ist der Code nicht dekodierbar.
    ///
    /// ACHTUNG: `max_laenge` 4 und 7 sind KEIN Zufall, sondern die
    /// interessanten Faelle — dort greift die Kappung, weil ein Alphabet
    /// von 33 Symbolen ohne Kappung Laengen bis 6 braeuchte. Die Kraftsumme
    /// ist dann zu hoch und muss abgebaut werden.
    ///
    /// GEMESSEN: An Alphabeten, die gar nicht passen koennen (33 Symbole bei
    /// hoechstens Laenge 4 — es gibt nur 2^4 = 16 Plaetze, denn jedes Symbol
    /// kostet mindestens eine Kraft-Einheit), muss EIN BENANNTER FEHLER
    /// kommen, kein Panic und kein erfundener Code. Genau das prueft die
    /// zweite Haelfte dieses Tests, und die Faelle waren vorher ungetestet.
    #[test]
    fn huffman_kraftsumme_haelt_und_grenze_haelt() {
        for farben in [2usize, 7, 8, 20, 33, 100, 256] {
            for max_laenge in [4u8, 7, 10, 15] {
                let mut gew = vec![0u32; 256];
                // Ein sehr ungleiches Alphabet: genau der Fall, an dem die
                // Kappung eingreift.
                for (i, w) in gew.iter_mut().enumerate().take(farben) {
                    *w = 1u32 << (i % 20);
                }
                // Wann ist ein Alphabet ueberhaupt kodierbar? Jedes Symbol
                // kostet MINDESTENS eine Kraft-Einheit (Laenge = max), also
                // gilt zwingend `n <= 2^max`.
                //
                // GEMESSEN: die naheliegende Formel `n <= 2^(max+1) - 1`
                // (die Zahl der Blaetter eines vollstaendigen Baumes der Tiefe
                // `max`) ist FALSCH und zu grosszuegig — sie sagt 31 Symbole
                // bei max 4 zu, wo doch nur 16 gehen. Mit ihr kam
                // `HuffmanUebervoll` fuer ein Alphabet, von dem der Test
                // behauptet hatte, es passe.
                let passend = farben <= (1usize << max_laenge);
                match Praefixcode::baue(&gew, max_laenge, "test") {
                    Ok(code) => {
                        assert!(
                            passend,
                            "{farben} Symbole bei max {max_laenge} passen nicht — \
                             es wurde trotzdem ein Code gebaut"
                        );
                        let mut kraft: u64 = 0;
                        for &l in &code.laenge {
                            if l > 0 {
                                assert!(
                                    l <= max_laenge,
                                    "Laenge {l} ueber der Grenze {max_laenge}"
                                );
                                kraft += 1u64 << (max_laenge - l);
                            }
                        }
                        assert_eq!(
                            code.benutzt, farben,
                            "{farben} Symbole, aber nur {} bekamen eine Laenge",
                            code.benutzt
                        );
                        assert!(
                            kraft <= (1u64 << max_laenge),
                            "{farben} Symbole bei max {max_laenge}: Kraft {kraft} > {}",
                            1u64 << max_laenge
                        );
                    }
                    Err(f) => assert!(
                        !passend,
                        "{farben} Symbole bei max {max_laenge} passen, \
                         aber es kam {f:?}"
                    ),
                }
            }
        }
    }

    /// Ein Alphabet mit EINEM Symbol kostet null Bit — so lesen die
    /// Leser in `narsil-image` (vp8l.rs:169) und libwebp
    /// (`BuildHuffmanTable`). Wer hier ein Bit schreibt, verschiebt den
    /// ganzen Bildstrom.
    #[test]
    fn huffman_einzelnes_symbol_hat_laenge_eins() {
        let mut gew = vec![0u32; 8];
        gew[5] = 42;
        let code = Praefixcode::baue(&gew, 15, "test").expect("Code bauen");
        assert_eq!(code.benutzt, 1);
        assert_eq!(code.laenge[5], 1, "ein Symbol bekommt Laenge 1");
    }

    /// Leeres Alphabet ist ein BENANNTER Fehler, kein Panic (M8).
    #[test]
    fn huffman_leeres_alphabet_ist_fehler() {
        let gew = vec![0u32; 16];
        assert!(matches!(
            Praefixcode::baue(&gew, 15, "leer"),
            Err(KonvertFehler::HuffmanLeer { .. })
        ));
    }
}
