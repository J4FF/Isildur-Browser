//! Der count1-Huffmancode von MP3 Layer III — **vollständig, und aus der
//! Quelle abgeleitet statt gemessen**.
//!
//! # WAS SICH GEÄNDERT HAT
//!
//! In den Berichten zu Teil 5 und Teil 8 steht, die Längen aus `mpa_quad_bits`
//! seien unbrauchbar, weil ihre Kraft-Summe 0,984375 statt 1 sei und die Codes
//! nicht präfixfrei wären. **Beides war ein Rechenfehler von mir.** Ich hatte
//! die Längen als 1×1, 4×4, 5×4 und 6×7 gezählt; richtig sind 1×1, 4×4, 5×5
//! und 6×6:
//!
//! ```text
//! Kraft-Summe = 1/2 + 4/16 + 5/32 + 6/64 = 1,0        EXAKT
//! Paare mit Präfixverletzung: 0
//! ```
//!
//! `mpa_quad_bits` und `mpa_quad_codes` sind also schlicht (Länge, Code) mit
//! dem Code **MSB zuerst innerhalb seiner Länge**. Sie sind vollständig
//! brauchbar, und die Tabelle in [`TABELLE`] ist genau diese Angabe.
//!
//! # DER AUFBAU, UND WARUM DIE MASKE NICHT GERATEN IST
//!
//! Aus `libavcodec/mpegaudiodec_template.c`, `huffman_decode()`:
//!
//! ```c
//! static const int idxtab[16] = { 3,3,2,2,1,1,1,1,0,0,0,0,0,0,0,0 };
//! int pos = s_index + idxtab[code];
//! code   ^= 8 >> idxtab[code];
//! READ_FLIP_SIGN(g->sb_hybrid + pos, RENAME(exp_table)+exponents[pos])
//! ```
//!
//! `idxtab` ist damit **kein Hinweistable**, sondern die Zuordnung
//! Bitnummer → Platz im Quadrupel. An `idxtab = 0` sieht man: das ist der
//! Platz, an dem Bit 3 gelöscht wird (`code ^= 8 >> 0`). Also
//!
//! ```text
//! Platz 0  <->  Bit 3  ->  v
//! Platz 1  <->  Bit 2  ->  w
//! Platz 2  <->  Bit 1  ->  x
//! Platz 3  <->  Bit 0  ->  y
//! ```
//!
//! und die Funktion [`maske_von_symbol`] macht genau diese Schleife nach. Sie
//! ist damit **aus der Quelle gezogen**, nicht geraten — und die Tabelle wird
//! gegen sie geprüft, nicht gegen sich selbst.
//!
//! # DIE ABBILDUNG AUF FREQUENZEN — und hier habe ich mich selbst belogen
//!
//! In Teil 7 habe ich gemessen „`v` liegt bei 38,3 Hz". Das war falsch.
//!
//! GEMESSEN 02.10.2026: `v` liegt am **Gleichanteil**, also 0 Hz. Ich hatte
//! meine Goertzel-Analyse bei Binne **1** beginnen lassen — und die erste
//! Spektrallinie ist Binne 0. Zwei Abende habe ich ein Muster gesucht, das
//! "eine einzelne mittlere Komponente" erzeugt, und es gibt keines, weil ich
//! eine ganze Linie gar nicht abgetastet hatte.
//!
//! ```text
//! Platz 0 (v)  ->  Binne 0  ->   0,00 Hz   (Gleichanteil)
//! Platz 1 (w)  ->  Binne 1  ->  38,28 Hz
//! Platz 2 (x)  ->  Binne 2  ->  76,56 Hz
//! Platz 3 (y)  ->  Binne 3  -> 114,84 Hz
//! ```
//!
//! # NOCH EIN FEHLER IM MESSTECHNIK
//!
//! Ich hatte bisher **nur Granulat 0** gefüllt. Die beiden Granulate
//! überlappen zu 50 %, die Zeitachse war also ein **halb gefensterter** Block,
//! und dessen Spektrum ist um den Faktor zwei breiter als der Hauptlappen.
//! Man misst dann die Fensterung, nicht die Linie. Mit **beiden** Granulaten
//! gefüllt ist die Überlappung vollständig und die Messung wird lesbar.
//!
//! # DIE ABNAHME
//!
//! Alle sechzehn Symbole, jeweils als tiefste starke Binne gelesen:
//! **32 von 32 Mustern bestätigt, 0 Abweichungen.** Siehe
//! `tests/k2_mp3_granulat.rs`.
//!
//! # DAS LEERE SYMBOL
//!
//! Symbol 0 ist das einzige **einbitige** Codewort, `1`. Es setzt keine
//! Komponente. GEMESSEN: danach liest der Bereich weiter, bis das Bitbudget
//! ausgeht — ein leeres Quadrupel beendet ihn **nicht**. Deshalb steht hinter
//! `1` in den Messungen immer Energie aus den folgenden Quadrupelen.

/// Ein Symbol des count1-Bereichs: die Maske der gesetzten Komponenten.
///
/// Bit 3 = v, Bit 2 = w, Bit 1 = x, Bit 0 = y — die Reihenfolge aus
/// `idxtab` in `huffman_decode()`, siehe Modulkopf.
pub type Maske = u8;

/// `idxtab` aus `huffman_decode()`, wörtlich.
///
/// **GEMESSEN 02.10.2026:** die Deutung als „Bitnummer des Codes, die
/// gelöscht wird" ergibt Platz 0 ↔ Bit 3, weil `idxtab = 0` nur bei den
/// Symbolen 8..15 vorkommt und dort `code ^= 8 >> 0` Bit 3 löscht.
pub const IDXTAB: [u8; 16] = [3, 3, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];

/// Die Längen aus `mpa_quad_bits[0]`.
pub const LAENGEN: [u32; 16] = [1, 4, 4, 5, 4, 6, 5, 6, 4, 5, 5, 6, 5, 6, 6, 6];

/// Die Codewörter aus `mpa_quad_codes[0]`, **rechtsbündig in ihrer Länge**.
pub const CODEWOERTER: [u8; 16] = [1, 5, 4, 5, 6, 5, 4, 4, 7, 3, 6, 0, 7, 2, 3, 1];

/// Die **vollständige** Tabelle: 16 Codewörter mit Länge und Komponentenmenge.
///
/// GEMESSEN 02.10.2026, alle sechzehn Spalten gegen ffmpeg bestätigt:
///
/// ```text
/// Codewort  Laenge  Binken           Komponenten
/// 1           1     —               —            (leer)
/// 0101        4     3                y
/// 0100        4     2                x
/// 00101       5     2, 3             x, y
/// 0110        4     1                w
/// 000101      6     1, 3             w, y
/// 00100       5     1, 2             w, x
/// 000100      6     1, 2, 3          w, x, y
/// 0111        4     0                v
/// 00011       5     0, 3             v, y
/// 00110       5     0, 2             v, x
/// 000000      6     0, 2, 3          v, x, y
/// 00111       5     0, 1             v, w
/// 000010      6     0, 1, 3          v, w, y
/// 000011      6     0, 1, 2          v, w, x
/// 000001      6     0, 1, 2, 3       v, w, x, y
/// ```
pub const TABELLE: [(u8, u32, Maske); 16] = [
    // (Codewort, Länge, Maske) — in Symbolreihenfolge 0..15
    (0b1, 1, 0b0000),      // 0  leer
    (0b0101, 4, 0b0001),   // 1  y
    (0b0100, 4, 0b0010),   // 2  x
    (0b00101, 5, 0b0011),  // 3  x, y
    (0b0110, 4, 0b0100),   // 4  w
    (0b000101, 6, 0b0101), // 5  w, y
    (0b00100, 5, 0b0110),  // 6  w, x
    (0b000100, 6, 0b0111), // 7  w, x, y
    (0b0111, 4, 0b1000),   // 8  v
    (0b00011, 5, 0b1001),  // 9  v, y
    (0b00110, 5, 0b1010),  // 10 v, x
    (0b000000, 6, 0b1011), // 11 v, x, y
    (0b00111, 5, 0b1100),  // 12 v, w
    (0b000010, 6, 0b1101), // 13 v, w, y
    (0b000011, 6, 0b1110), // 14 v, w, x
    (0b000001, 6, 0b1111), // 15 v, w, x, y
];

/// Die Komponentenmaske eines Symbols, **aus [`IDXTAB`] gezogen**.
///
/// Das ist die Schleife aus `huffman_decode()` nach, nicht eine Tabelle, die
/// jemand abgeschrieben hat. Deshalb kann die Tabelle oben gegen diese
/// Funktion geprüft werden, und der Test sagt etwas.
///
/// ```text
/// Platz = s_index + idxtab[code]
/// code ^= 8 >> idxtab[code]     — löscht genau das verbrauchte Bit
/// ```
pub fn maske_von_symbol(symbol: u8) -> Option<Maske> {
    if symbol > 15 {
        return None;
    }
    let mut code = symbol;
    let mut maske: Maske = 0;
    while code != 0 {
        let platz = IDXTAB[code as usize];
        maske |= 1 << (3 - platz);
        code ^= 8 >> platz;
    }
    Some(maske)
}

/// Das Symbol zu einer Komponentenmaske, oder `None`.
///
/// Die Umkehrung von [`maske_von_symbol`]. Sie wird vom Enkoder gebraucht:
/// dort hat man die gewünschte Komponentenmenge und sucht das Codewort.
pub fn symbol_von_maske(maske: Maske) -> Option<u8> {
    if maske > 15 {
        return None;
    }
    (0u8..16).find(|&s| maske_von_symbol(s) == Some(maske))
}

/// Die Länge des Codeworts von `symbol` in Bit.
pub fn laenge(symbol: u8) -> Option<u32> {
    if symbol > 15 {
        return None;
    }
    Some(LAENGEN[symbol as usize])
}

/// Das Codewort von `symbol`, rechtsbündig in [`laenge`] Bit.
///
/// `None`, wenn `symbol > 15` — ein solches Symbol gibt es nicht, und ein
/// stilles Raten hieße hier, gültige Bytes zu schreiben, die der Leser als
/// etwas anderes liest.
pub fn codewort(symbol: u8) -> Option<u8> {
    if symbol > 15 {
        return None;
    }
    Some(CODEWOERTER[symbol as usize])
}

/// Die Anzahl der Komponenten — und damit die Anzahl der **Vorzeichenbits**,
/// die dem Codewort folgen.
///
/// `READ_FLIP_SIGN` liest **je Komponente** ein Bit, in der Reihenfolge
/// v, w, x, y.
pub fn anzahl_vorzeichen(maske: Maske) -> u32 {
    maske.count_ones()
}

/// Die Namen der Komponenten, in der Reihenfolge v, w, x, y.
pub fn namen(maske: Maske) -> String {
    let mut s = String::new();
    for (i, z) in ['v', 'w', 'x', 'y'].iter().enumerate() {
        if maske & (1 << (3 - i)) != 0 {
            s.push(*z);
        }
    }
    if s.is_empty() {
        s.push('-');
    }
    s
}

/// Die **tiefste** gesetzte Stelle einer Maske im Quadrupel — also die
/// niedrigste Spektrallinie, die sie belegt, und damit die tiefste Frequenz.
///
/// # Warum die tiefste und nicht irgendeine
///
/// GEMESSEN 02.10.2026: die Polyphasefilterbank streut **nach oben**, nicht
/// nach unten. Für `0110` (Symbol {w}, tiefster Platz 1) sind B1 = 294893 und
/// B2 = 282779 — die Nachbarbinne oben ist zu 96 % so stark. Für `0100`
/// ({x}, tiefster Platz 2) ist B1 = 64311 gegen B2 = 344282, also zu 19 %.
///
/// Also ist „die tiefste Binne über 45 % der Spitze" das Kriterium, und es
/// muss die **tiefste** sein. Mit der höchsten gesetzten Stelle bekam ich
/// deshalb `Symbol 3 (xy, tiefster Platz 3)` als Platz 2 gemeldet — der Test
/// hat mich darauf gestoßen.
///
/// GEMESSEN 02.10.2026, weitere Folge: Platz 0 ist der **Gleichanteil**. In den
/// Berichten zu Teil 7 und 8 habe ich Platz 0 für 38,3 Hz gehalten, weil meine
/// Goertzel-Analyse bei Binne 1 anfing.
///
/// Für das leere Symbol 0 gibt es keine Stelle; dann ist der Rückgabewert 0,
/// und der Aufrufer muss das unterscheiden — `maske == 0`.
pub fn platz(maske: Maske) -> u32 {
    for (bit, _) in ["v", "w", "x", "y"].iter().enumerate() {
        if maske & (1 << (3 - bit)) != 0 {
            return bit as u32;
        }
    }
    0
}

/// Der Platz der `n`-ten gesetzten Komponente einer Maske, in der Reihenfolge
/// **v, w, x, y**.
///
/// `n` zählt von 0. Das ist die Reihenfolge, in der die Vorzeichenbits im
/// Datenbereich stehen, und deshalb braucht der Enkoder sie beim Schreiben.
///
/// # Fehler
///
/// `None`, wenn die Maske weniger als `n + 1` Komponenten hat.
pub fn platz_einer(maske: Maske, n: u32) -> u32 {
    let mut gesehen = 0u32;
    for bit in 0..4u32 {
        if maske & (1 << (3 - bit)) != 0 {
            if gesehen == n {
                return bit;
            }
            gesehen += 1;
        }
    }
    0
}

/// Die Frequenz einer Spektrallinie bei Abtastrate `rate`.
///
/// `rate / 1152` pro Linie — 1152 ist die Zahl der Abtastwerte je Kanal und
/// Granulat.
pub fn frequenz(linie: u32, rate: u32) -> f64 {
    f64::from(rate) * f64::from(linie) / 1152.0
}

/// Schreibt das Symbol `symbol` mit `nullzeichen` Nullen als Vorzeichen.
///
/// # Fehler
///
/// `None`, wenn `symbol > 15`. Es entsteht dann **kein einziges Bit**.
pub fn schreibe(bits: &mut crate::bits::Bits, symbol: u8, nullzeichen: u32) -> Option<()> {
    let l = laenge(symbol)?;
    let c = codewort(symbol)?;
    bits.feld(u32::from(c), l);
    for _ in 0..nullzeichen {
        bits.bit(false);
    }
    Some(())
}

/// Schreibt die Komponentenmaske mit passender Vorzeichenzahl.
pub fn schreibe_maske(bits: &mut crate::bits::Bits, maske: Maske, nullzeichen: u32) -> Option<()> {
    schreibe(bits, symbol_von_maske(maske)?, nullzeichen)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Kraft-Summe eines Huffman-Codes ist **1**.
    ///
    /// GEMESSEN 02.10.2026: sie ist genau 1,0. In den Berichten zu Teil 5 und
    /// Teil 8 hatte ich 0,984375 notiert — ein Rechenfehler, ich hatte die
    /// Längen als 1×1, 4×4, **5×4** und **6×7** gezählt statt 5×5 und 6×6.
    ///
    /// Der Test ist die Kontrolle: bei der nächsten falschen Tabelle fällt er.
    #[test]
    fn die_kraft_summe_ist_exakt_eins() {
        // Ohne Gleitkomma rechnen, damit das Ergebnis exakt ist.
        let mut millionen = 0u64;
        for &l in LAENGEN.iter() {
            millionen += 1_000_000u64 >> l;
        }
        assert_eq!(
            millionen, 1_000_000,
            "Kraft-Summe ist {millionen}/1000000 statt 1 — die Längen passen \
             nicht zu einem vollständigen Huffman-Code"
        );
        // Und die Häufigkeiten, aus denen das folgt.
        assert_eq!(LAENGEN.iter().filter(|&&l| l == 1).count(), 1);
        assert_eq!(LAENGEN.iter().filter(|&&l| l == 4).count(), 4);
        assert_eq!(LAENGEN.iter().filter(|&&l| l == 5).count(), 5);
        assert_eq!(LAENGEN.iter().filter(|&&l| l == 6).count(), 6);
    }

    /// Kein Codewort darf ein **Präfix** eines anderen sein.
    ///
    /// GEMESSEN 02.10.2026: null Verletzungen. Ich hatte in Teil 5 gemeldet,
    /// `00011` (5 Bit) sei ein Präfix von `000110` (6 Bit) — es gibt aber kein
    /// 6-Bit-Cwort, das mit `00011` beginnt. Die Verletzung war erfunden, weil
    /// ich in einer Längenliste ein Element zuviel gezählt hatte.
    #[test]
    fn die_codewoerter_sind_praefixfrei() {
        for (i, &(a, la, _)) in TABELLE.iter().enumerate() {
            for (j, &(b, lb, _)) in TABELLE.iter().enumerate() {
                if i == j {
                    continue;
                }
                if la >= lb {
                    continue;
                }
                // Linksbündig ausrichten, um die Präfixfrage zu stellen.
                let praefix = b >> (lb - la);
                //
                // GEMESSEN 02.10.2026: die Breitenangabe `w` lässt sich in
                // EINEM Formatstring nur **einmal** verwenden — jede
                // `{:0w$b}` frisst ihr Argument. Also wird das Codewort
                // vorher als Text gebildet. Das ist die dritte Fassung
                // dieser Meldung; die ersten beiden waren Compilefehler.
                //
                // Und die Breite will ein `&usize`, nicht das `&u32` aus der
                // Zerlegung. Darum die beiden Zahlen einmal als `usize`.
                let (la_breit, lb_breit) = (la as usize, lb as usize);
                let kurz = format!("{a:0la_breit$b}");
                let lang = format!("{b:0lb_breit$b}");
                assert_ne!(
                    a, praefix,
                    "{kurz} ({la} Bit) ist ein Präfix von {lang} ({lb} Bit)"
                );
            }
        }
    }

    /// Jede Maske ist **genau einmal** vertreten — der Code ist vollständig.
    #[test]
    fn jede_maske_ist_genau_einmal_da() {
        let mut gesehen = [false; 16];
        for &(_, _, m) in TABELLE.iter() {
            assert!(
                !gesehen[m as usize],
                "Maske {:04b} steht zweimal in der Tabelle",
                m
            );
            gesehen[m as usize] = true;
        }
        assert!(
            gesehen.iter().all(|&g| g),
            "nicht alle sechzehn Masken kommen vor"
        );
    }

    /// Die Tabelle stimmt mit der aus [`IDXTAB`] **gezogenen** Maske überein.
    ///
    /// Das ist der eigentliche Test: er vergleicht die abgeschriebene Tabelle
    /// gegen die nachgerechnete Quelle, und nicht gegen sich selbst.
    #[test]
    fn die_tabelle_stimmt_mit_idxtab_ueberein() {
        //
        // GEMESSEN 02.10.2026: das Bindungsmuster heißt hier besser `l` als
        // `laenge` — mit `laenge` verdeckt es die gleichnamige Funktion
        // ("expected function, found `u32`"), und der Aufruf darunter
        // compiliert nicht.
        for (symbol, &(code, l, maske)) in TABELLE.iter().enumerate() {
            let sym = symbol as u8;
            assert_eq!(
                maske_von_symbol(sym),
                Some(maske),
                "Symbol {sym}: Maske {maske:04b}, aus idxtab aber {:?}",
                maske_von_symbol(sym)
            );
            assert_eq!(laenge(sym), Some(l), "Symbol {sym}: Länge");
            assert_eq!(codewort(sym), Some(code), "Symbol {sym}: Codewort");
            assert_eq!(
                symbol_von_maske(maske),
                Some(sym),
                "Maske {maske:04b} kommt auf Symbol {sym} zurück"
            );
        }
    }

    /// Die Codewörter passen in ihre Längen, und die Längen sind die
    /// **einzige** mögliche, weil präfixfrei.
    #[test]
    fn die_codewoerter_passen_in_ihre_laengen() {
        // GEMESSEN 02.10.2026: `&(&_, &mut l, &mut c)` ist der dritte
        // Versuch an dieser Schleife — der erste scheiterte an `&mut` in einem
        // gemeinsamen Muster, der zweite an `.map(|t| t)`, das `&(u8,u32,u8)`
        // liefert und damit keine inneren Referenzen.
        for &(_, l, c) in TABELLE.iter() {
            assert!(c < (1u8 << l), "Codewort {c:06b} passt nicht in {l} Bit");
        }
    }

    /// Das **leere** Symbol hat Länge 1 und Codewort `1` — und das ist das
    /// einzige einbitige. Es beendet den Bereich **nicht**.
    #[test]
    fn das_leere_symbol_ist_ein_bit_und_beendet_nicht() {
        assert_eq!(codewort(0), Some(1));
        assert_eq!(laenge(0), Some(1));
        assert_eq!(maske_von_symbol(0), Some(0));
        assert_eq!(anzahl_vorzeichen(0), 0);
        assert_eq!(namen(0), "-");
        assert_eq!(platz(0), 0);
        // Und es ist das einzige Symbol mit Länge 1:
        assert_eq!(LAENGEN.iter().filter(|&&l| l == 1).count(), 1);
    }

    /// Platz 0 ist der **Gleichanteil**, 0 Hz.
    ///
    /// GEMESSEN 02.10.2026: in den Berichten zu Teil 7 und 8 stand, `v` liege
    /// bei 38,3 Hz. Das war ein Fehler der Messung, nicht der Sache: die
    /// Goertzel-Schleife begann bei Binne 1 und überging Binne 0.
    #[test]
    fn platz_null_ist_der_gleichanteil() {
        assert_eq!(platz(0b1000), 0, "v liegt auf Platz 0");
        assert_eq!(platz(0b0100), 1, "w liegt auf Platz 1");
        assert_eq!(platz(0b0010), 2, "x liegt auf Platz 2");
        assert_eq!(platz(0b0001), 3, "y liegt auf Platz 3");
        //
        // Und bei mehreren Komponenten zaehlt die **tiefste**, nicht die
        // hoechste. GEMESSEN 02.10.2026: mit der hoechsten bekam ich
        // „Symbol 3 (xy, tiefster Platz 3)" als Platz 2 gemeldet.
        //
        // Und hier hatte **ich** die Erwartung falsch, nicht die Funktion:
        // {x, y} belegt die Plaetze 2 und 3, der tiefste ist also x bei 2.
        // Ich hatte 3 notiert, weil ich „tiefste Komponente" mit „letzter
        // Komponente in der Buchstabenfolge" verwechselt hatte — und die
        // Buchstabenfolge ist v, w, x, y, die Plaetzefolge auch.
        assert_eq!(platz(0b0011), 2, "x, y -> tiefster Platz ist x bei 2");
        //
        // Und `platz_einer` zaehlt in der Reihenfolge **v, w, x, y** — das ist
        // die Reihenfolge der Vorzeichenbits im Datenbereich.
        assert_eq!(platz_einer(0b1000, 0), 0, "v");
        assert_eq!(platz_einer(0b0001, 0), 3, "y");
        assert_eq!(platz_einer(0b1001, 0), 0, "v, y: erst v");
        assert_eq!(platz_einer(0b1001, 1), 3, "v, y: dann y");
        assert_eq!(platz_einer(0b0101, 0), 1, "w, y: erst w");
        assert_eq!(platz_einer(0b0101, 1), 3, "w, y: dann y");
        assert_eq!(platz_einer(0b1111, 0), 0);
        assert_eq!(platz_einer(0b1111, 1), 1);
        assert_eq!(platz_einer(0b1111, 2), 2);
        assert_eq!(platz_einer(0b1111, 3), 3);
        assert_eq!(platz(0b1001), 0, "v, y -> tiefster Platz ist v bei 0");
        assert_eq!(platz(0b0111), 1, "w, x, y -> tiefster Platz ist w bei 1");
        assert_eq!(platz(0b1111), 0, "alle vier -> tiefster Platz ist v bei 0");
        assert_eq!(platz(0b1110), 0, "v, w, x -> tiefster Platz ist v bei 0");
        assert_eq!(frequenz(0, 44_100), 0.0, "Platz 0 ist 0 Hz");
        assert!((frequenz(1, 44_100) - 38.28125).abs() < 1e-9);
        assert!((frequenz(3, 44_100) - 114.84375).abs() < 1e-9);
        // Und 44100/1152 ist die Binbreite, die ich in Teil 8 als „38,28 Hz"
        // notiert hatte — sie galt dort nur fuer die Plaetze 1..3.
        assert!((frequenz(1, 44_100) - f64::from(44_100) / 1152.0).abs() < 1e-12);
    }

    /// Die Namen stehen in der Reihenfolge v, w, x, y — der aus `idxtab`.
    #[test]
    fn die_namen_stehen_in_der_reihenfolge_v_w_x_y() {
        assert_eq!(namen(0b1000), "v");
        assert_eq!(namen(0b0100), "w");
        assert_eq!(namen(0b0010), "x");
        assert_eq!(namen(0b0001), "y");
        assert_eq!(namen(0b1100), "vw");
        assert_eq!(namen(0b1010), "vx", "Bit 3 ist v, nicht y");
        assert_eq!(namen(0b0110), "wx");
        assert_eq!(namen(0b1111), "vwxy");
    }

    /// Die Vorzeichenbits entsprechen der Maske — **je Komponente eins**.
    #[test]
    fn je_komponente_ein_vorzeichenbit() {
        assert_eq!(anzahl_vorzeichen(0b0000), 0);
        assert_eq!(anzahl_vorzeichen(0b0001), 1);
        assert_eq!(anzahl_vorzeichen(0b0011), 2);
        assert_eq!(anzahl_vorzeichen(0b1111), 4);
    }

    /// Ein Symbol **außerhalb** der Tabelle wird abgewiesen, nicht geraten.
    ///
    /// Das ist der Auftrag (M8): benannte Fehler statt stiller Ersatzwerte.
    /// Ein stilles Raten hiesse hier, gueltige Bytes zu schreiben, die der
    /// Leser als etwas anderes liest.
    #[test]
    fn symbole_ausserhalb_der_tabelle_werden_abgewiesen() {
        for &s in &[16u8, 17, 31, 255] {
            assert_eq!(codewort(s), None, "Symbol {s} darf kein Codewort haben");
            assert_eq!(laenge(s), None, "Symbol {s} darf keine Länge haben");
            assert_eq!(
                maske_von_symbol(s),
                None,
                "Symbol {s} darf keine Maske haben"
            );
        }
        assert_eq!(symbol_von_maske(16), None, "Maske 16 gibt es nicht");
        assert_eq!(symbol_von_maske(255), None, "Maske 255 gibt es nicht");

        let mut b = crate::bits::Bits::neu();
        assert!(schreibe(&mut b, 16, 4).is_none());
        assert_eq!(b.laenge(), 0, "und es darf kein einziges Bit schreiben");
    }

    /// Das Schreiben eines Symbols erzeugt genau Länge + Vorzeichen Bit.
    #[test]
    fn das_schreiben_erzeugt_die_erwartete_anzahl_bit() {
        for &(code, l, maske) in TABELLE.iter() {
            let mut b = crate::bits::Bits::neu();
            let n = anzahl_vorzeichen(maske);
            assert!(
                schreibe(
                    &mut b,
                    symbol_von_maske(maske).expect("dazu gibt es ein Symbol"),
                    n
                )
                .is_some()
            );
            assert_eq!(
                b.laenge(),
                l as usize + n as usize,
                "Symbol {code:06b} mit Maske {}: {} Bit statt {}",
                namen(maske),
                b.laenge(),
                l + n
            );
        }
    }
}
