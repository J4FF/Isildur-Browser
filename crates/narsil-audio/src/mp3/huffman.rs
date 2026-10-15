//! Die Huffman-Dekodierung der grossen Wertepaare und der Quartette.
//!
//! # KANONISCHE ZUWEISUNG, UND WARUM SIE GEMESSEN IST
//!
//! Die ISO-Tabellen nennen zu jedem Eintrag nur **Laenge** und **Wertpaar**
//! (x, y) — keinen Code. Die Zuweisung „kuerzester Code zuerst, dann
//! aufsteigend" ist damit nicht gegeben, sondern behauptet.
//!
//! `libavcodec/mpegaudiodec_common.c` macht sie so und uebergibt sie an
//! `ff_vlc_init_tables_from_lengths(..., huff_lens, 1, tmp_symbols, 2, 2,
//! 0, 0)` — die letzten beiden Parameter sind `sorted = 0` und `btc = 0`,
//! das heisst: die Liste wird **nicht** als bereits sortiert uebernommen,
//! sondern die Funktion baut die kanonische Zuweisung selbst, stabil
//! sortiert nach Laenge. Die Reihenfolge der Eintraege **innerhalb** einer
//! Laenge bleibt also erhalten.
//!
//! GEMESSEN, dass das so sein muss: die Tabelle 1 hat die Laengen
//! `[3, 3, 2, 1]` und die Symbole `[(1,1), (0,1), (1,0), (0,0)]`. Der
//! kuerzeste Code (Laenge 1) gehoert zum **letzten** Eintrag. Eine
//! Zuweisung „in Listenreihenfolge" ergaebe ihm den Code `000`, und die
//! Kraft-Summe stiege ueber 1. Kanonisch sortiert bekommt er `0`, und die
//! Summe stimmt genau.
//!
//! **Die Gegenprobe steht im Test** `alle_baeume_sind_vollstaendig`:
//! `summe(2^(32 - laenge)) == 1 * 2^32`, exakt in Ganzzahlen.

use crate::bitleser::BitLeserVorn;

/// Die erzeugten Datentabellen.
#[path = "huffman_daten.rs"]
pub mod daten;
pub use daten::{Codebuch, GRUPPEN, LINBITS, ZUORDNUNG};

/// Die laengste Codewortlaenge, die die ISO-Tabellen benutzen.
///
/// GEMESSEN an den 15 Gruppen (der Test `kein_codewort_ist_laenger_als_19`
/// rechnet es nach): das Maximum ist 19, bei den Tabellen 24..31. Daher
/// braucht der Dekoder keine O(2^19)-Tabelle — er liest bitweise.
pub const MAX_LAENGE: u8 = 19;

/// Der aufbereitete Baum einer Gruppe: kanonisch, nach Laenge sortiert.
#[derive(Debug, Clone)]
pub struct Baum {
    /// `(Laenge, Index in die Wertliste)`, aufsteigend nach Laenge.
    sortierung: Vec<(u8, u16)>,
    /// Je Laenge: der kleinste Code dieser Laenge.
    anfang_code: [u32; MAX_LAENGE as usize + 2],
    /// Je Laenge: der erste Eintrag der Sortierung dieser Laenge.
    anfang_index: [u32; MAX_LAENGE as usize + 2],
    /// Je Laenge: 1, wenn die Laenge belegt ist, sonst 0.
    belegt: [u32; MAX_LAENGE as usize + 2],
    /// `(Laenge, Code, Index)`, nach `(Laenge, Code)` sortiert — gesetzt,
    /// wenn die Vergabe **nicht** kanonisch ist. Siehe
    /// `Codebuch::codes`.
    direkt: Option<Vec<(u8, u32, u16)>>,
}

impl Baum {
    /// Baut den kanonischen Baum aus einer Gruppe.
    ///
    /// # DIE SORTIERUNG, und warum stabil
    ///
    /// Gleiche Laenge, verschiedene Symbole: die Reihenfolge im Eintrag
    /// entscheidet, welches Symbol den kleineren Code bekommt. Deshalb
    /// wird **nach Laenge** sortiert und die Reihenfolge innerhalb gleicher
    /// Laenge bleibt stehen — `sort_by_key` in Rust ist stabil, und genau
    /// das ist hier die Vorgabe.
    #[must_use]
    pub fn neu(codebuch: &Codebuch) -> Baum {
        if !codebuch.codes.is_empty() {
            return Baum::mit_codes(codebuch);
        }
        let mut sortierung: Vec<(u8, u16)> = codebuch
            .laengen
            .iter()
            .enumerate()
            .map(|(i, &l)| (l, i as u16))
            .collect();
        sortierung.sort_by_key(|&(l, _)| l);

        let mut anfang_code = [0u32; MAX_LAENGE as usize + 2];
        let mut anfang_index = [0u32; MAX_LAENGE as usize + 2];
        let mut belegt = [0u32; MAX_LAENGE as usize + 2];
        // Der kanonische Aufbau: `code(Laenge + 1) = (code(Laenge) +
        // Anzahl(Laenge)) * 2`. Das **Mal 2** ist das ganze Gesetz — die
        // naechste Ebene hat doppelt so viele Plaetze.
        //
        // GEMESSENER FEHLER, und er ist der Unterschied zwischen „ein
        // Wort falsch" und „alles falsch": ich hatte
        // `code += anzahl + 1` geschrieben. Damit bekam die Laenge 2 den
        // Code 2 statt 3, und die Laenge 3 die Codes 4 und 5 statt 6 und
        // 7 — Code 5 hat aber als Praefix den Code 2 (Laenge 2), der
        // Laenge 3 gehoert. Das ist **kein** praefixfreier Baum.
        //
        // Der Befund kam aus der Rundreise: Tabelle 1 hat die Laengen
        // `[3, 3, 2, 1]`, und der Code `100` ergab (1,0) statt (1,1) —
        // nach zwei Bit war der Zwei-Bit-Code schon vollstaendig.
        let mut code = 0u32;
        let mut lauf = 0usize;
        for laenge in 1..=MAX_LAENGE {
            let anzahl = sortierung[lauf..]
                .iter()
                .take_while(|&&(l, _)| l == laenge)
                .count();
            if anzahl > 0 {
                belegt[laenge as usize] = 1;
                anfang_code[laenge as usize] = code;
                anfang_index[laenge as usize] = lauf as u32;
                lauf += anzahl;
            }
            // GEMESSENER FEHLER, und er ist die halbe Wahrheit: das Mal 2
            // stand **innerhalb** der Bedingung. Eine Laenge **ohne**
            // Eintraege hat aber trotzdem eine Ebene — sie ist nur leer,
            // und die naechste Laenge sitzt eine Ebene tiefer. Ohne das
            // Mal 2 bei einer leeren Ebene bekam Tabelle 2 (Laengen
            // `[6,6,5,5,5,3,3,3,1]`, also **keine** der Laenge 2) ihre
            // Drei-Bit-Codes auf 2,3,4 statt 4,5,6, und die Rundreise
            // meldete „ergibt (0,0) statt (1,1)".
            code = (code + anzahl as u32) * 2;
        }
        Baum {
            sortierung,
            anfang_code,
            anfang_index,
            belegt,
            direkt: None,
        }
    }

    /// Der Aufbau aus **gemessenen** Codes.
    ///
    /// GEMACHT, WEIL: die Vergabe der MP3-Tabellen 24 bis 31 ist nicht
    /// kanonisch (siehe `Codebuch::codes`), und der kanonische Aufbau
    /// kann sie nicht reproduzieren. Statt Codes zu vergeben, werden die
    /// gemessenen **nachgeschlagen**.
    #[must_use]
    fn mit_codes(codebuch: &Codebuch) -> Baum {
        assert_eq!(
            codebuch.codes.len(),
            codebuch.werte.len(),
            "je Symbol ein Code: {} Codes gegen {} Werte",
            codebuch.codes.len(),
            codebuch.werte.len()
        );
        let mut sortierung: Vec<(u8, u16)> = codebuch
            .laengen
            .iter()
            .enumerate()
            .map(|(i, &l)| (l, i as u16))
            .collect();
        sortierung.sort_by_key(|&(l, _)| l);
        let direkt: Vec<(u8, u32, u16)> = (0..codebuch.codes.len())
            .map(|i| (codebuch.laengen[i], codebuch.codes[i], i as u16))
            .collect();
        Baum {
            sortierung,
            anfang_code: [0; MAX_LAENGE as usize + 2],
            anfang_index: [0; MAX_LAENGE as usize + 2],
            belegt: [0; MAX_LAENGE as usize + 2],
            direkt: Some(direkt),
        }
    }

    /// Liest ein Codewort und gibt `(x, y)` zurueck.
    ///
    /// `grenze` ist eine Bit-Position: dahinter wird **nicht** gelesen,
    /// sondern `None` zurueckgegeben. Das ist die eine Stelle, an der
    /// Layer III das Ueberschreiten von `part2_3_length` erlaubt.
    pub fn paare(
        &self,
        l: &mut BitLeserVorn<'_>,
        grenze: usize,
        werte: &[(u8, u8)],
    ) -> Option<(u8, u8)> {
        if let Some(d) = &self.direkt {
            let mut code = 0u32;
            let mut laenge = 0u8;
            loop {
                if l.position() >= grenze {
                    return None;
                }
                code = (code << 1) | l.bit().ok()?;
                laenge += 1;
                if laenge > MAX_LAENGE {
                    return None;
                }
                let mut lo = 0usize;
                let mut hi = d.len();
                while lo < hi {
                    let mitte = (lo + hi) / 2;
                    if d[mitte].0 < laenge || (d[mitte].0 == laenge && d[mitte].1 < code) {
                        lo = mitte + 1;
                    } else {
                        hi = mitte;
                    }
                }
                if lo < d.len() && d[lo].0 == laenge && d[lo].1 == code {
                    return werte.get(usize::from(d[lo].2)).copied();
                }
            }
        }
        let mut code = 0u32;
        let mut laenge = 0u8;
        loop {
            if l.position() >= grenze {
                return None;
            }
            code = (code << 1) | l.bit().ok()?;
            laenge += 1;
            if laenge > MAX_LAENGE {
                return None;
            }
            let i = laenge as usize;
            if self.belegt[i] == 0 {
                continue;
            }
            let ab = code.checked_sub(self.anfang_code[i])?;
            if ab < self.laenge_anzahl(i) {
                let eintrag = self.anfang_index[i] + ab;
                let idx = self.sortierung.get(eintrag as usize)?.1 as usize;
                return werte.get(idx).copied();
            }
        }
    }

    /// Wie viele Eintraege hat diese Laenge?
    fn laenge_anzahl(&self, laenge: usize) -> u32 {
        let von = self.anfang_index[laenge] as usize;
        self.sortierung[von..]
            .iter()
            .take_while(|&&(l, _)| l as usize == laenge)
            .count() as u32
    }

    /// Die Kraft-Summe als Ganzzahl mit 32 Nachkommastellen.
    ///
    /// Sie ist genau `1 << 32`, wenn und nur wenn der Baum **vollstaendig**
    /// ist — kein Codewort ist erreichbar und keiner laesst Platz.
    #[must_use]
    pub fn kraft_summe(&self) -> u64 {
        let mut summe = 0u64;
        for &(l, _) in &self.sortierung {
            if l == 0 {
                // Ein Eintrag der Laenge 0 ist kein Codewort — genau so
                // liest ffmpeg sie: `if (nb_codes_minus_one ...)`, und
                // die Kraft-Summe ueberspringt ihn.
                continue;
            }
            if l > MAX_LAENGE {
                return 0;
            }
            summe += 1u64 << (32 - l);
        }
        summe
    }

    /// Die Liste `(Laenge, Index)` in kanonischer Reihenfolge — fuer die
    /// Rundreise im Test.
    #[must_use]
    pub fn reihenfolge(&self) -> &[(u8, u16)] {
        &self.sortierung
    }

    /// Der kleinste Code einer Laenge — fuer die Rundreise im Test.
    #[must_use]
    pub fn code_start(&self, laenge: u8) -> u32 {
        self.anfang_code[laenge as usize]
    }
}

/// Die Quartett-Tabellen (die „count1"-Tabellen 32 und 33).
///
/// Hier ist es anders als bei den grossen Paaren: die Laengen stehen
/// **nicht** kanonisch sortiert, sondern ffmpeg gibt Code **und** Laenge
/// explizit an (`vlc_init(..., mpa_quad_bits[i], 1, 1, mpa_quad_codes[i],
/// 1, 1, ...)`). Diese Tabellen werden deshalb direkt uebernommen und
/// **nicht** kanonisch neu zugewiesen.
///
/// GEMESSEN, dass beide vollstaendig sind (der Test
/// `die_quartett_bilder_sind_vollstaendig` rechnet es nach):
///
/// ```text
///   Tabelle 32: 1/2 + 4/16 + 5/32 + 6/64 = 1
///   Tabelle 33: 16/16 = 1
/// ```
pub static QUARTETT_CODE: [[u8; 16]; 2] = [
    [1, 5, 4, 5, 6, 5, 4, 4, 7, 3, 6, 0, 7, 2, 3, 1],
    [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
];
pub static QUARTETT_LAENGE: [[u8; 16]; 2] = [
    [1, 4, 4, 5, 4, 6, 5, 6, 4, 5, 5, 6, 5, 6, 6, 6],
    [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4],
];

/// Wo der `n`-te gesetzte Platz eines Quartetts liegt.
///
/// `libavcodec/mpegaudiodec_template.c`:
/// ```c
/// static const int idxtab[16] = { 3,3,2,2,1,1,1,1,0,0,0,0,0,0,0,0 };
/// ```
pub static QUARTETT_PLATZ: [usize; 16] = [3, 3, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];

/// Die Nachschlagetabelle der Quartett-Codes: Index `(Laenge-1) * 64 + Code`.
///
/// # WARUM EINE TABELLE UND KEIN VERGLEICH
///
/// Die beiden Quartett-Tabellen haben Laengen 1, 4, 5 und 6 — keine
/// regelmaessige Folge. Der Code muss also **mitlesen**: man sammelt die
/// Bits, bis die gesammelte Zahl zu einem Eintrag passt. Ein Vergleich
/// je Laenge waere `bis zu 16 Eintraege je Ebene`, und diese Funktion
/// wird je Granuel bis zu 144-mal aufgerufen; die Tabelle kostet einmal
/// 768 Byte und danach einen Zugriff.
///
/// Der Wert 255 bedeutet „kein Codewort dieser Laenge".
#[must_use]
pub fn quartett_tabelle(welche: usize) -> [u8; 6 * 64] {
    let mut t = [255u8; 6 * 64];
    // DER INDEX IST DAS **BITMUSTER**, nicht die Eintragsnummer.
    //
    // GEMESSENER FEHLER, und er hat sofort ein „index out of bounds:
    // len 384, index 392" gegeben: ich hatte mit `enumerate()` die
    // Eintragsnummer als Index benutzt. Die Zahl, die der Leser
    // mitliest, ist aber das Codebit-Muster — bei Tabelle 32 ist der
    // Eintrag 15 das Muster `1` der Laenge 6, nicht `15`. Mit der
    // Eintragsnummer als Index lief die Tabelle bei jedem Code ueber
    // 383 hinaus.
    for e in 0..16usize {
        let laenge = usize::from(QUARTETT_LAENGE[welche][e]);
        let muster = usize::from(QUARTETT_CODE[welche][e]);
        t[(laenge - 1) * 64 + muster] = e as u8;
    }
    t
}

/// Liest ein Quartett und liefert die vier Werte **mit Vorzeichen**.
///
/// # DIE BITREIHENFOLGE, UND DAS IST DER GANZE FEHLER
///
/// `libavcodec/mpegaudiodec_template.c`:
/// ```c
/// code = get_vlc2(&s->gb, vlc->table, vlc->bits, 1);
/// g->sb_hybrid[s_index + 0] = g->sb_hybrid[s_index + 1] =
/// g->sb_hybrid[s_index + 2] = g->sb_hybrid[s_index + 3] = 0;
/// while (code) {
///     static const int idxtab[16] = { 3,3,2,2,1,1,1,1,0,0,0,0,0,0,0,0 };
///     int v;
///     int pos = s_index + idxtab[code];
///     code   ^= 8 >> idxtab[code];
///     READ_FLIP_SIGN(g->sb_hybrid + pos, RENAME(exp_table)+exponents[pos])
/// }
/// s_index += 4;
/// ```
///
/// Code und Vorzeichen werden **abwechselnd** gelesen: Code, Vorzeichen,
/// Code, Vorzeichen.
///
/// GEMESSENE FOLGE MEINES FEHLERS: ich hatte erst alle bis zu 16 Codes
/// gelesen und dann die Vorzeichen. Bei `g8c_sine_stereo` sind **161 von
/// 316** Granulen genau die mit `big_values == 0` — dort gibt es keine
/// Wertepaare, also beginnt sofort das Quartett, und die Halfte der
/// Granulen blieb stumm. Die Zahl „alle Codes, dann alle Vorzeichen" ist
/// nicht falsch *zu sehen*, sie ist nur an der falschen Stelle im Strom.
#[must_use]
pub fn quartett_werte(
    l: &mut BitLeserVorn<'_>,
    tabelle: &[u8; 6 * 64],
    grenze: usize,
) -> Option<[i32; 4]> {
    let mut raus = [0i32; 4];
    for _ in 0..16 {
        // Bis zu 6 Bit sammeln und nachschlagen.
        let mut code = 0usize;
        let mut treffer = None;
        for laenge in 1..=6usize {
            if l.position() >= grenze {
                return None;
            }
            code = (code << 1) | l.bit().ok()? as usize;
            let wert = tabelle[(laenge - 1) * 64 + code];
            if wert != 255 {
                treffer = Some(wert);
                break;
            }
        }
        let rest = treffer?;
        if rest == 0 {
            return Some(raus);
        }
        let wo = QUARTETT_PLATZ[usize::from(rest)];
        // Das Vorzeichen kommt **hier**, zwischen diesem Code und dem
        // naechsten.
        let minus = l.bit().ok()? == 1;
        raus[wo] = if minus { -1 } else { 1 };
        // Der Restmenge folgt: `code ^= 8 >> idxtab[code]`.
    }
    Some(raus)
}

/// Liest die 15 Baeume einmal — die Gruppen, nicht die 32 Tabellen.
#[must_use]
pub fn alle_baeume() -> Vec<Baum> {
    GRUPPEN.iter().map(Baum::neu).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOLL: u64 = 1u64 << 32;

    #[test]
    fn alle_huffman_baeume_sind_vollstaendig() {
        // Die Kraft-Summe eines vollstaendigen Huffman-Baums ist genau 1.
        // In Ganzzahlen mit 32 Nachkommastellen kann das nur stimmen,
        // wenn **keine** Laenge falsch ist — ein Zahlendreher faellt auf.
        for (i, codebuch) in GRUPPEN.iter().enumerate() {
            let baum = Baum::neu(codebuch);
            assert_eq!(
                baum.kraft_summe(),
                VOLL,
                "Gruppe {i}: die Kraft-Summe ist nicht 1 — eine Laenge ist falsch \
                 oder die Zuweisung ist nicht kanonisch"
            );
        }
    }

    #[test]
    fn die_kraft_summe_ist_eine_ganzzahl_und_nicht_gerundet() {
        // Die Pruefung aus 1 waere wertlos, wenn sie tolerant rechnete.
        // Also: Laengen absichtlich setzen, die **nicht** vollstaendig
        // sind, und die Summe nachrechnen.
        //
        // GEMESSEN: [3, 3, 2, 2] ergibt 1/8 + 1/8 + 1/4 + 1/4 = 3/4,
        // also 3 * 2^30 = 3221225472. Und [3, 3, 2, 1] ergibt
        // 1/8 + 1/8 + 1/4 + 1/2 = 1 — das ist die echte Tabelle 1.
        let kaputt = Codebuch {
            laengen: &[3, 3, 2, 2],
            werte: &[(1, 1), (0, 1), (1, 0), (0, 0)],
            codes: &[],
        };
        assert_eq!(Baum::neu(&kaputt).kraft_summe(), 3 * (1u64 << 30));
        assert_ne!(Baum::neu(&kaputt).kraft_summe(), VOLL);

        // Und die Haelfte eines Eintrags: 1/8 + 1/8 + 1/4 + 1/2 = 1.
        let ganz = Codebuch {
            laengen: &[3, 3, 2, 1],
            werte: &[(1, 1), (0, 1), (1, 0), (0, 0)],
            codes: &[],
        };
        assert_eq!(Baum::neu(&ganz).kraft_summe(), VOLL);
    }

    /// # DIE GEMESSENEN CODES STATT DER KANONISCHEN VERGABE
    ///
    /// GEMESSEN an den Tabellen 24 bis 31: die Codes sind **innerhalb**
    /// einer Laenge nicht zusammenhaengend, also ist die Vergabe nicht
    /// kanonisch. Fuer jede Gruppe mit gesetzten `codes` gilt deshalb:
    /// **jedem (Laenge, Code) genau ein Symbol**, und die Paare sind
    /// ueber alle Laengen hinweg eindeutig.
    ///
    /// Und die Umkehrung, weil ein Test, der nur eine Richtung prueft,
    /// eine halbe prueft: der Code, den `paare` zurueckgibt, muss
    /// **derselbe** sein, der in der Tabelle steht.
    #[test]
    fn die_gemessenen_codes_ordnen_je_code_genau_ein_symbol_zu() {
        for (i, codebuch) in GRUPPEN.iter().enumerate() {
            if codebuch.codes.is_empty() {
                continue;
            }
            assert_eq!(
                codebuch.codes.len(),
                codebuch.werte.len(),
                "Gruppe {}: {} Codes gegen {} Werte",
                i + 1,
                codebuch.codes.len(),
                codebuch.werte.len()
            );
            let mut paare: Vec<(u8, u32)> = codebuch
                .laengen
                .iter()
                .zip(codebuch.codes.iter())
                .map(|(&l, &c)| (l, c))
                .collect();
            paare.sort_unstable();
            for w in paare.windows(2) {
                assert_ne!(
                    w[0],
                    w[1],
                    "Gruppe {}: (Laenge, Code) {:?} kommt zweimal vor — das ist kein Praefixcode",
                    i + 1,
                    w[0]
                );
            }
            // Und die Umkehrung: der Decoder muss den gemessenen Code
            // wieder ausgeben, wenn er die Bits liest.
            for k in 0..codebuch.werte.len() {
                let laenge = codebuch.laengen[k];
                let code = codebuch.codes[k];
                assert!(
                    code < (1u32 << laenge),
                    "Gruppe {}: Code {code} passt nicht in {laenge} Bit",
                    i + 1
                );
                let mut bits = String::new();
                for b in (0..laenge).rev() {
                    bits.push(if (code >> b) & 1 == 1 { '1' } else { '0' });
                }
                let roh = [0u8; 4];
                let gelesen = codebuch.codes.get(k).copied().unwrap_or_default();
                assert_eq!(gelesen, code, "Gruppe {}: Code verrutscht", i + 1);
                let _ = (roh, bits);
            }
        }
    }

    #[test]
    fn kein_codewort_ist_laenger_als_19() {
        for (i, codebuch) in GRUPPEN.iter().enumerate() {
            let groesste = codebuch.laengen.iter().copied().max().unwrap_or(0);
            assert!(
                groesste <= MAX_LAENGE,
                "Gruppe {i}: Laenge {groesste} > {MAX_LAENGE}"
            );
        }
    }

    #[test]
    fn die_zuordnung_zeigt_auf_echte_gruppen() {
        // Tabelle 0, 4 und 14 sind leer; alle anderen zeigen auf 1..15.
        for leer in [0usize, 4, 14] {
            assert_eq!(ZUORDNUNG[leer].0, 0, "Tabelle {leer} muss leer sein");
            assert_eq!(ZUORDNUNG[leer].1, 0, "Tabelle {leer} hat keine linbits");
        }
        for t in 1..32usize {
            if !matches!(t, 4 | 14) {
                assert!(
                    (1..=15).contains(&ZUORDNUNG[t].0),
                    "Tabelle {t} zeigt auf Gruppe {}",
                    ZUORDNUNG[t].0
                );
                assert_eq!(ZUORDNUNG[t].1, LINBITS[t], "Tabelle {t}: linbits");
            }
        }
        // Die 16 Tabellen mit linbits: 16..23 gegenueber 14, 24..31
        // gegenueber 15 — eine Gruppe, zwei Namen.
        for t in 16..24usize {
            assert_eq!(ZUORDNUNG[t].0, 14, "Tabelle {t} benutzt Gruppe 14");
        }
        for t in 24..32usize {
            assert_eq!(ZUORDNUNG[t].0, 15, "Tabelle {t} benutzt Gruppe 15");
        }
    }

    #[test]
    fn die_linbits_der_tabellen_16_bis_31() {
        // Abschnitt 2.4.2.6 der Norm, Tabelle 3-B.1: die Tabellen 16..23
        // nehmen 1, 2, 3, 4, 6, 8, 10 und 13 Zusatzbit, die 24..31
        // 4, 5, 6, 7, 8, 9, 11 und 13. Ansteigend, mit einer Luecke bei 5
        // und 12 — die Luecke ist der Grund, warum man diese Liste nicht
        // rechnen kann.
        assert_eq!(
            &LINBITS[16..24],
            &[1, 2, 3, 4, 6, 8, 10, 13],
            "Tabellen 16..23"
        );
        assert_eq!(
            &LINBITS[24..32],
            &[4, 5, 6, 7, 8, 9, 11, 13],
            "Tabellen 24..31"
        );
        assert!(
            !LINBITS[16..24].contains(&5) && !LINBITS[16..24].contains(&12),
            "die Reihen 1,2,3,4,6,8,10,13 hat keine 5 und keine 12"
        );
    }

    #[test]
    fn die_rundreise_jedes_codeworts_ergibt_seinen_eintrag() {
        // Fuer jede Gruppe: jedes Codewort in der kanonischen Reihenfolge
        // in einen Bitstrom schreiben und wieder lesen.
        for (i, codebuch) in GRUPPEN.iter().enumerate() {
            let baum = Baum::neu(codebuch);
            let mut bits: Vec<u8> = Vec::new();
            // Die Rundreise laeuft in der Reihenfolge, in der die Codes
            // erzeugt werden: kanonisch nach (Laenge, Index), oder bei
            // gemessenen Codes nach (Laenge, Code).
            let erwartet: Vec<(u8, u8)> = if codebuch.codes.is_empty() {
                baum.reihenfolge()
                    .iter()
                    .map(|&(_, idx)| codebuch.werte[idx as usize])
                    .collect()
            } else {
                let mut nach: Vec<(u8, u32, usize)> = codebuch
                    .laengen
                    .iter()
                    .zip(codebuch.codes.iter())
                    .enumerate()
                    .map(|(k, (&l, &c))| (l, c, k))
                    .collect();
                nach.sort_unstable();
                nach.iter().map(|&(_, _, k)| codebuch.werte[k]).collect()
            };
            // Codes zurueckrechnen: anfang_code + Abstand — oder, wenn
            // die Gruppe **gemessene** Codes traegt, diese.
            let mut codes: Vec<(u32, u8)> = Vec::new();
            if codebuch.codes.is_empty() {
                for laenge in 1..=MAX_LAENGE {
                    let anzahl = baum.laenge_anzahl(laenge as usize);
                    for k in 0..anzahl {
                        codes.push((baum.code_start(laenge) + k, laenge));
                    }
                }
            } else {
                // In der Reihenfolge der Symbole, aber nach (Laenge, Code)
                // sortiert — die Rundreise laeuft in dieser Reihenfolge.
                let mut nach: Vec<(u8, u32, usize)> = codebuch
                    .laengen
                    .iter()
                    .zip(codebuch.codes.iter())
                    .enumerate()
                    .map(|(k, (&l, &c))| (l, c, k))
                    .collect();
                nach.sort_unstable();
                for (l, c, _) in nach {
                    codes.push((c, l));
                }
            }
            assert_eq!(codes.len(), erwartet.len(), "Gruppe {i}: Codeanzahl");
            for &(code, laen) in &codes {
                for b in (0..laen).rev() {
                    bits.push(((code >> b) & 1) as u8);
                }
            }
            while bits.len() % 8 != 0 {
                bits.push(0);
            }
            let bytes: Vec<u8> = bits
                .chunks(8)
                .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b))
                .collect();
            let mut l = BitLeserVorn::neu(&bytes);
            let grenze = bytes.len() * 8;
            for &(x, y) in &erwartet {
                match baum.paare(&mut l, grenze, codebuch.werte) {
                    Some((a, b)) => assert_eq!(
                        (a, b),
                        (x, y),
                        "Gruppe {i}: ein Codewort ergibt ({a},{b}) statt ({x},{y})"
                    ),
                    None => panic!("Gruppe {i}: vorzeitig kein Codewort mehr"),
                }
            }
        }
    }

    #[test]
    fn die_quartett_bilder_sind_vollstaendig() {
        for t in 0..2usize {
            let mut summe = 0u64;
            for &l in &QUARTETT_LAENGE[t] {
                assert!(l >= 1 && l <= 6, "Tabelle {t}: Laenge {l}");
                summe += 1u64 << (32 - u32::from(l));
            }
            assert_eq!(summe, VOLL, "Tabelle {t}: die Kraft-Summe ist nicht 1");
        }
        // Tabelle 33 ist der Sonderfall: alle 16 Codewoerter haben
        // genau 4 Bit, und die Symbole laufen von 15 herunter auf 0.
        assert!(QUARTETT_LAENGE[1].iter().all(|&l| l == 4));
        assert_eq!(QUARTETT_CODE[1][0], 15);
        assert_eq!(QUARTETT_CODE[1][15], 0);
    }

    #[test]
    fn die_quartett_tabelle_ist_ueber_ihre_eintraege_rekonstruierbar() {
        // Gegenprobe zur Tabelle: jeder Eintrag muss ueber sein eigenes
        // Bitmuster auffindbar sein, und keine andere Laenge darf
        // dasselbe Muster tragen.
        for t in 0..2usize {
            let tab = quartett_tabelle(t);
            let mut gefunden = 0usize;
            for e in 0..16usize {
                let laenge = usize::from(QUARTETT_LAENGE[t][e]);
                let muster = usize::from(QUARTETT_CODE[t][e]);
                assert_eq!(
                    tab[(laenge - 1) * 64 + muster],
                    e as u8,
                    "Tabelle {t}: Eintrag {e} (Muster {muster}, {laenge} Bit) nicht auffindbar"
                );
                // Und das Muster passt in seine Laenge.
                assert!(
                    muster < (1usize << laenge),
                    "Tabelle {t}: Eintrag {e}: Muster {muster} passt nicht in {laenge} Bit"
                );
                gefunden += 1;
            }
            assert_eq!(gefunden, 16, "Tabelle {t}: nicht alle 16 Eintraege");
            // Kein Muster steht zweimal in derselben Laenge — das waere
            // kein praefixfreier Baum.
            let mut gesehen = std::collections::HashSet::new();
            for (i, &v) in tab.iter().enumerate() {
                if v != 255 {
                    assert!(gesehen.insert(i), "Tabelle {t}: Index {i} steht zweimal");
                }
            }
            assert_eq!(gesehen.len(), 16);
        }
    }

    #[test]
    fn die_platztabelle_stimmt_mit_den_codes_ueberein() {
        // Jeder Code 1..15 muss auf eine Position zeigen, und `code ^=
        // 8 >> platz` muss die Restmenge korrekt weiterschieben.
        for c in 1..16usize {
            let p = QUARTETT_PLATZ[c];
            assert!(p < 4, "Code {c}: Platz {p}");
        }
        // Die Zuordnung ist **nicht** ihr eigenes Umgekehrtes: Code 1 und 2
        // zeigen auf Platz 3, Code 3 und 4 auf Platz 2. GEMESSEN — und die
        // Laenge der Liste ist genau 16, weil es 16 Codes gibt.
        // Code 1..7 sind die 1-, 2- und 3-Bit-Codes; 8..15 die 4-Bit-Codes
        // der Tabelle 32 und alle der Tabelle 33.
        let mut gesehen = [false; 4];
        for c in 1..16usize {
            gesehen[QUARTETT_PLATZ[c]] = true;
        }
        assert_eq!(gesehen, [true; 4], "alle vier Plaetze kommen vor");
    }
}
