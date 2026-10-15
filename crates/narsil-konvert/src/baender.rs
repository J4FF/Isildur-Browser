//! Die Bandgrenzen von MP3 Layer III — `ff_band_size_long` und
//! `ff_band_index_long`.
//!
//! # WOZU DAS EIN ENKODER BRAUCHT
//!
//! `exponent()` in [`crate::granulat`] nimmt bereits einen **Band**-Index, nicht
//! einen Linien-Index. Das ist keine Feinheit: der Exponent einer Linie hängt
//! über `pretab[band]` am Band, in dem sie liegt. Wer die Bandgrenzen nicht
//! kennt, kann die Skalenfaktoren nicht an die richtigen Linien setzen.
//!
//! Aus `mpegaudiodec_template.c`, `exponents_from_scale_factors()`:
//!
//! ```c
//! if (band_idx[0 + 3 * ch] == 0 && ...)
//! ...
//! exponents[i] = v;   /* v haengt an band_idx */
//! ```
//!
//! # DIE EINHEIT IST **LINIENPAARE**, NICHT LINIEN
//!
//! GEMESSEN: `ff_band_size_long` gibt **Anzahlen von Linien** an — jede Zeile
//! summiert sich zu **576**. Der gross_values-Bereich zählt aber
//! **Linienpaare**, und genau deswegen baut ffmpeg `ff_band_index_long` mit
//! `>> 1`:
//!
//! ```c
//! for (int i = 0; i < 9; i++) {
//!     int k = 0;
//!     for (int j = 0; j < 22; j++) {
//!         ff_band_index_long[i][j] = k;
//!         k += ff_band_size_long[i][j] >> 1;
//!     }
//!     ff_band_index_long[i][22] = k;
//! }
//! ```
//!
//! Also ist `band_index[i][j]` der Paar-Index, an dem Band `j` beginnt, und
//! `band_index[i][22]` sind die Paare insgesamt — **288**.
//!
//! Das ist keine Vermutung, es ist nachrechenbar: 576 / 2 = 288, und der Test
//! prüft genau das für alle neun Abtastraten.
//!
//! # DIE NEUN ABTASTRATEN
//!
//! Die Zeilen sind **nicht** 11, 12 und 16 kHz — das sind die MPEG-Versionen.
//! Sie stehen in dieser Reihenfolge in ffmpeg:
//!
//! ```text
//! 44100  48000  32000  22050  24000  16000  11025  12000  8000
//! ```
//!
//! Und sie sind nicht aufsteigend sortiert: 32000 steht an Stelle 3, obwohl
//! es grösser ist als 22050. Wer sie nach der Zahl sucht, findet die falsche
//! Zeile. Deshalb wird hier über den **Zeilenindex** ausgewählt, und
//! [`zeile_fuer_abtastrate`] lehnt eine unbekannte Rate ab, statt still die
//! nächste zu nehmen.

/// Die neun Abtastraten in der Reihenfolge der Tabelle.
pub const ABTASTRATEN: [u32; 9] = [
    44_100, 48_000, 32_000, 22_050, 24_000, 16_000, 11_025, 12_000, 8_000,
];

/// `ff_band_size_long`: Bandbreiten in **Linien** je Abtastrate.
///
/// Jede Zeile hat 22 Bänder, und jede Zeile summiert sich zu 576 Linien.
pub const BANDGROESSEN: [[u8; 22]; 9] = [
    // 44100
    [
        4, 4, 4, 4, 4, 4, 6, 6, 8, 8, 10, 12, 16, 20, 24, 28, 34, 42, 50, 54, 76, 158,
    ],
    // 48000
    [
        4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 10, 12, 16, 18, 22, 28, 34, 40, 46, 54, 54, 192,
    ],
    // 32000
    [
        4, 4, 4, 4, 4, 4, 6, 6, 8, 10, 12, 16, 20, 24, 30, 38, 46, 56, 68, 84, 102, 26,
    ],
    // 22050
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    // 24000
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 18, 22, 26, 32, 38, 46, 54, 62, 70, 76, 36,
    ],
    // 16000
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    // 11025
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    // 12000
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    // 8000
    [
        12, 12, 12, 12, 12, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 76, 90, 2, 2, 2, 2, 2,
    ],
];

/// `ff_mpa_pretab[0]` — der Prefaktor je Band, wenn `preflag` **nicht**
/// gesetzt ist. Also überall null.
///
/// Der Name steht hier historisch, weil ffmpeg die Tabelle über den
/// `preflag`-Schalter indiziert und nicht über die Blockart. Siehe
/// [`pretab`].
pub const PRETAB_LANG: [u8; 22] = [0; 22];

/// `ff_mpa_pretab[1]` — der Prefaktor je Band, wenn `preflag` gesetzt ist.
///
/// ```text
/// Band    0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21
/// pretab  0  0  0  0  0  0  0  0  0  0  0  1  1  1  1  2  2  3  3  3  2  0
/// ```
///
/// GEMESSEN 02.10.2026: es ist **null**, nicht `0,1,2,3,...` — die Reihenfolge
/// fällt am Ende auf 2 und dann auf 0. Wer sie als `Band` liest, bekommt für
/// Band 17 den Prefaktor 3 und für Band 21 den Prefaktor 0.
pub const PRETAB_KURZ: [u8; 22] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 3, 2, 0,
];

/// Die Anzahl der Bänder — 22, und bei 8 kHz sind die hintersten fünf nur zwei
/// Linien breit.
pub const BAENDER: usize = 22;

/// Die Anzahl der Spektrallinien je Granulat.
pub const LINIEN: usize = 576;

/// Die Anzahl der **Linienpaare** — der gross_values-Bereich zählt Paare.
pub const PAARE: usize = LINIEN / 2;

/// [`PAARE`] als Schleifenobergrenze.
pub const PAARES_A: u32 = PAARE as u32;

/// Die Summe der **kurzen** Bandgressen: 192.
///
/// GEMESSEN 02.10.2026: nicht 576. Ein kurzer Granulat hat drei Fenster, und
/// die Bandzerlegung gilt **pro Fenster**, also je 192 Linien.
pub const KURZE_BAENDER_SUMME: u32 = 192;

/// Der Zeilenindex der Tabelle für eine Abtastrate.
///
/// `None`, wenn die Rate nicht in der Tabelle steht.
///
/// # Warum nicht „die nächste passende"
///
/// GEMESSEN 02.10.2026: die Tabelle ist **nicht aufsteigend sortiert** —
/// 32000 steht an Stelle 3, zwischen 48000 und 22050. Wer die Zeile über eine
/// Suche nach der nächstliegenden Rate bestimmt, landet bei 32000 auf der
/// Zeile für 22050. Eine falsche Zeile liefert keine Fehlermeldung, sondern
/// falsche Bandgrenzen.
pub fn zeile_fuer_abtastrate(rate: u32) -> Option<usize> {
    ABTASTRATEN.iter().position(|&r| r == rate)
}

/// Die Bandbreiten in Linien für eine Abtastrate.
pub fn bandgroessen(rate: u32) -> Option<&'static [u8; 22]> {
    BANDGROESSEN.get(zeile_fuer_abtastrate(rate)?)
}

/// `ff_band_index_long[i][j]` — der **Paar**-Index, an dem Band `j` beginnt.
///
/// `None`, wenn die Abtastrate nicht in der Tabelle steht oder `band > 22`.
pub fn band_index(rate: u32, band: usize) -> Option<u32> {
    // `>=`, nicht `>`: bei `band == 22` liefert `.take(22)` zwar das richtige
    // Ergebnis, aber der Aufrufer hat ein Band 22 angefragt, das es nicht gibt.
    // Siehe die Bemerkung bei [`pretab`].
    if band > BAENDER {
        return None;
    }
    let sizes = bandgroessen(rate)?;
    let mut k = 0u32;
    for &s in sizes.iter().take(band) {
        k += u32::from(s) >> 1;
    }
    Some(k)
}

/// Das **Ende** eines Bandes, in Paaren — also [`band_index`] des nächsten
/// Bandes.
///
/// Für das letzte Band (21) ist das [`PAARE`], 288.
pub fn band_ende(rate: u32, band: usize) -> Option<u32> {
    // Ebenso: `band >= BAENDER`, denn es gibt kein Ende für Band 21 als
    // „nächstes Band" — dafür steht [`PAARE`].
    if band >= BAENDER {
        return None;
    }
    band_index(rate, band + 1)
}

/// Das Band, in dem der Paar-Index `paar` liegt.
///
/// Das ist die Umkehrung von [`band_index`] und die Funktion, die ein Enkoder
/// für die Skalenfaktoren braucht.
///
/// # Fehler
///
/// `None`, wenn die Abtastrate unbekannt ist oder `paar` ausserhalb von 0..288
/// liegt. Es wird **nicht** auf das letzte Band geklemmt — ein Klemmen würde ein
/// falsches Band liefern statt eines Fehlers, und der Fehler fällt später an
/// der falschen Stelle auf.
pub fn band_fuer_paar(rate: u32, paar: u32) -> Option<usize> {
    if paar as usize >= PAARE {
        return None;
    }
    let sizes = bandgroessen(rate)?;
    let mut k = 0u32;
    for (band, &s) in sizes.iter().enumerate() {
        let breite = u32::from(s) >> 1;
        if paar < k + breite {
            return Some(band);
        }
        k += breite;
    }
    None
}

/// Das Band, in dem die **Spektrallinie** `linie` liegt, 0..575.
///
/// # Warum das nicht dasselbe ist wie [`band_fuer_paar`]
///
/// GEMESSEN 02.10.2026: bei 44100 Hz sind die Bänder 6 bis 11 sechs Linien
/// breit, also **drei Paare** — eine ungerade Zahl. Deshalb liegen die
/// Bandgrenzen nicht auf geraden Paarindizes:
///
/// ```text
///   44100 Hz, Bandanfang in Linienpaaren:  0  2  4  6  8  10  12  15  18  22 ...
///                                                        ^^  ungerade ab hier
/// ```
///
/// Ein Band kann also eine ungerade Paarzahl umfassen. Was **nicht** passieren
/// kann, ist ein zerschnittenes Paar — und genau das ist die Eigenschaft, auf
/// die es ankommt, weil der Leser für beide Linien **einen** Exponenten
/// verwendet (`exponent = exponents[s_index]`, auch für `s_index + 1`).
///
/// Die Gegenprobe steht in `quant::tests::kein_paar_wird_zerschnitten`.
///
/// # Fehler
///
/// `None`, wenn die Abtastrate unbekannt ist oder `linie` ausserhalb von 0..576
/// liegt.
pub fn band_fuer_linie(rate: u32, linie: u32) -> Option<usize> {
    if linie as usize >= LINIEN {
        return None;
    }
    // Die Grenzen liegen per Konstruktion auf geraden **Linien**indizes, weil
    // der Paarindex das Doppelte ist. Also genuegt die Halbierung.
    band_fuer_paar(rate, linie / 2)
}

/// Der Prefaktor eines Bandes, aus [`PRETAB_LANG`] oder [`PRETAB_KURZ`].
///
/// # Der zweite Schalter heisst `preflag`, **nicht** „kurze Bloecke"
///
/// GEMESSEN 02.10.2026: ffmpeg schreibt
///
/// ```c
/// pretab = ff_mpa_pretab[g->preflag];
/// ```
///
/// Der Schalter ist also `preflag` aus der Seiteninfo — **nicht**, ob der
/// Granulat kurze Bloecke hat. Die beiden Dinge fallen nur zusammen: `preflag`
/// wird nur dann ausgewertet, wenn der Block kurz ist.
///
/// Ich hatte den Parameter `kurz` genannt, und damit zwei Dinge verwechselt.
/// `preflag = 0` heisst in der Tat „Tabelle 0, alles null" — aber aus dem
/// Grund, dass das Flag nicht gesetzt ist, nicht weil der Block lang ist.
/// Darum der Test unten: beide Schalter kommen bei denselben Werten heraus,
/// und nur einer davon ist gemeint.
pub fn pretab(rate: u32, band: usize, preflag: bool) -> Option<u8> {
    //
    // GEMESSEN 02.10.2026: `band > BAENDER` war falsch. Die Bänder sind
    // 0..=21, es gibt also 22 Stück, und `22 > 22` ist falsch — der Wächter
    // liess `band = 22` durch und `PRETAB_KURZ[22]` lief über das Array
    // hinaus. Richtig ist `>=`.
    if band >= BAENDER {
        return None;
    }
    //
    // Nur zur Pruefung, ob es diese Abtastrate ueberhaupt gibt. Der Wert wird
    // nicht gebraucht — deshalb `as_ref()?` statt `is_none()`.
    bandgroessen(rate).as_ref()?;
    Some(if preflag {
        PRETAB_KURZ[band]
    } else {
        PRETAB_LANG[band]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jede Zeile muss **576 Linien** ergeben.
    ///
    /// GEMESSEN 02.10.2026: das ist die Bedingung, unter der die Tabelle
    /// überhaupt eine Bandzerlegung des Spektrums ist. Und die 8-kHz-Zeile
    /// sieht danach aus, als fehlten fünf Bänder — sie sind nur zwei Linien
    /// breit, was als `2, 2, 2, 2, 2` dasteht und zusammen 10 ergibt.
    #[test]
    fn jede_zeile_summe_576() {
        for (i, zeile) in BANDGROESSEN.iter().enumerate() {
            let summe: u32 = zeile.iter().map(|&b| u32::from(b)).sum();
            assert_eq!(
                summe, LINIEN as u32,
                "Zeile {i} ({} Hz) ergibt {summe} statt 576",
                ABTASTRATEN[i]
            );
        }
    }

    /// Und die Paarindizes ergeben **288**.
    ///
    /// `band_index(rate, 22)` ist das Ende des letzten Bandes, also die
    /// Gesamtzahl der Paare.
    #[test]
    fn der_paarindex_endet_bei_288() {
        for &rate in ABTASTRATEN.iter() {
            assert_eq!(
                band_index(rate, BAENDER),
                Some(PAARE as u32),
                "{rate} Hz: der Paarindex am Ende ist nicht 288"
            );
        }
    }

    /// Jede Bandbreite ist **gerade** — sonst ginge beim `>> 1` etwas verloren.
    #[test]
    fn jede_bandbreite_ist_gerade() {
        for (i, zeile) in BANDGROESSEN.iter().enumerate() {
            for (band, &b) in zeile.iter().enumerate() {
                assert_eq!(
                    b % 2,
                    0,
                    "Zeile {i} ({} Hz), Band {band}: Breite {b} ist ungerade — \\
                     `>> 1` verliert dann eine halbe Linie",
                    ABTASTRATEN[i]
                );
            }
        }
    }

    /// Die gemessene Regel für die Bandbreiten — und sie ist **schwächer**, als
    /// man sie sich wünschen würde.
    ///
    /// GEMESSEN 02.10.2026, alle neun Zeilen:
    ///
    /// ```text
    ///    44100: Summe 576  nicht fallend in Band 1..18: nein  letzte 158 vs 76
    ///    48000: Summe 576  nicht fallend in Band 1..18: nein  letzte 192 vs 54
    ///    32000: Summe 576  nicht fallend in Band 1..18: nein  letzte  26 vs 102
    ///    22050: Summe 576  nicht fallend in Band 1..18: nein  letzte  54 vs 58
    ///    24000: Summe 576  nicht fallend in Band 1..18: nein  letzte  36 vs 76
    ///    16000: Summe 576  nicht fallend in Band 1..18: nein  letzte  54 vs 58
    ///    11025: Summe 576  nicht fallend in Band 1..18: nein  letzte  54 vs 58
    ///    12000: Summe 576  nicht fallend in Band 1..18: nein  letzte  54 vs 58
    ///     8000: Summe 576  nicht fallend in Band 1..18: nein  letzte   2 vs 2
    /// ```
    ///
    /// Also: **die Bänder 0 bis 16 sind in allen neun Zeilen nicht fallend,
    /// ab Band 17 variieren sie.** Bei 8000 Hz bricht es bei Band 17
    /// (90 → 2), bei 22050/16000/11025/12000 bei Band 20 (68 → 58), und das
    /// letzte Band ist bei 32000, 24000 und 8000 schmaler als das vorletzte.
    ///
    /// Ich hatte zuerst „nicht fallend über alle Bänder" behauptet und es war
    /// an **drei** Zeilen falsch. Die Regel, die wirklich stimmt, ist die
    /// schwächere — und die steht hier, statt dass man sie sich zusammenbaut.
    #[test]
    fn die_bander_0_bis_16_sind_in_allen_zeilen_nicht_fallend() {
        for (i, zeile) in BANDGROESSEN.iter().enumerate() {
            for band in 1..=16 {
                assert!(
                    zeile[band] >= zeile[band - 1],
                    "Zeile {i} ({} Hz): Band {band} ({}) ist schmaler als Band {} ({})",
                    ABTASTRATEN[i],
                    zeile[band],
                    band - 1,
                    zeile[band - 1]
                );
            }
        }
    }

    /// Und ab Band 18 ist die Regel **nicht** allgemein — das ist gemessen und
    /// wird festgehalten, damit niemand die stärkere Behauptung wieder aufstellt.
    #[test]
    fn ab_band_18_gibts_gegenbeispiele() {
        // 32000: 102 -> 26
        assert!(
            BANDGROESSEN[2][21] < BANDGROESSEN[2][20],
            "32000 letztes Band"
        );
        // 22050: 68 -> 58
        assert!(BANDGROESSEN[3][20] < BANDGROESSEN[3][19], "22050 Band 20");
        // 8000: 90 -> 2
        assert!(BANDGROESSEN[8][17] < BANDGROESSEN[8][16], "8000 Band 17");
        // Und 44100 waechst bis zum Schluss:
        assert!(BANDGROESSEN[0][21] > BANDGROESSEN[0][20], "44100 waechst");
    }

    /// [`band_index`] und [`band_fuer_paar`] sind Umkehrungen voneinander.
    ///
    /// Das ist die eigentliche Abnahme: die Tabelle ist dann richtig, wenn beide
    /// Richtungen übereinstimmen — und das wird für **alle** 288 Paare und
    /// alle neun Raten geprüft, nicht für ein Beispiel.
    #[test]
    fn band_index_und_band_fuer_paar_sind_umkehrungen() {
        for &rate in ABTASTRATEN.iter() {
            for paar in 0..PAARE as u32 {
                let band = band_fuer_paar(rate, paar).expect("Paar liegt in einem Band");
                assert!(
                    band_index(rate, band).expect("Anfang") <= paar
                        && paar < band_ende(rate, band).expect("Ende"),
                    "{rate} Hz, Paar {paar}: Band {band} umfasst ihn nicht"
                );
            }
        }
    }

    /// Jeder Paarindex gehört zu **genau einem** Band, und die Bänder decken 0
    /// bis 288 lückenlos ab.
    #[test]
    fn die_bänder_decken_288_paare_lueckenlos_ab() {
        for &rate in ABTASTRATEN.iter() {
            let mut erwartet = 0u32;
            for band in 0..BAENDER {
                assert_eq!(
                    band_index(rate, band),
                    Some(erwartet),
                    "{rate} Hz, Band {band}: beginnt nicht bei {erwartet}"
                );
                erwartet = band_ende(rate, band).expect("Ende");
            }
            assert_eq!(erwartet, PAARE as u32, "{rate} Hz endet nicht bei 288");
        }
    }

    /// Die Tabelle ist **nicht aufsteigend sortiert**, und das ist eine Falle.
    ///
    /// GEMESSEN 02.10.2026: 32000 steht an Stelle 3, zwischen 48000 und 22050.
    /// Wer nach der nächstliegenden Rate sucht, nimmt bei 32000 die Zeile für
    /// 22050 — und bekommt keine Fehlermeldung, sondern falsche Bandgrenzen.
    #[test]
    fn die_reihenfolge_ist_nicht_aufsteigend() {
        assert_eq!(ABTASTRATEN[0], 44_100);
        assert_eq!(ABTASTRATEN[1], 48_000);
        assert_eq!(ABTASTRATEN[2], 32_000, "32000 steht an dritter Stelle");
        assert_eq!(ABTASTRATEN[3], 22_050, "und 22050 erst an vierter");
        assert!(
            ABTASTRATEN[2] > ABTASTRATEN[3],
            "die Reihenfolge ist aufsteigend — dann ist dieser Test falsch"
        );
        // Und die Zeilen sind wirklich verschieden:
        assert_ne!(BANDGROESSEN[2], BANDGROESSEN[3], "32000 und 22050 gleich?");
    }

    /// Eine unbekannte Abtastrate wird abgewiesen, nicht geraten.
    #[test]
    fn unbekannte_abtastraten_werden_abgewiesen() {
        for &rate in &[0u32, 1, 8_001, 11_024, 22_051, 96_000, 192_000] {
            assert_eq!(zeile_fuer_abtastrate(rate), None, "{rate} Hz");
            assert_eq!(bandgroessen(rate), None, "{rate} Hz");
            assert_eq!(band_index(rate, 0), None, "{rate} Hz");
            assert_eq!(band_fuer_paar(rate, 0), None, "{rate} Hz");
            assert_eq!(pretab(rate, 0, false), None, "{rate} Hz");
        }
    }

    /// Paare ausserhalb von 0..288 werden abgewiesen — **nicht** geklemmt.
    ///
    /// GEMESSEN: ein Klemmen liefert ein falsches Band statt eines Fehlers, und
    /// der Fehler fällt dann an der falschen Stelle auf.
    #[test]
    fn paare_ausserhalb_werden_abgewiesen_und_nicht_geklemmt() {
        for &paar in &[288u32, 289, 1000, u32::MAX] {
            assert_eq!(band_fuer_paar(44_100, paar), None, "Paar {paar}");
        }
        assert_eq!(band_fuer_paar(44_100, 287), Some(21), "der letzte Paar");
        assert_eq!(band_ende(44_100, 21), Some(288), "Ende des letzten Bandes");
        assert_eq!(band_ende(44_100, 22), None, "es gibt kein Band 22 als Ende");
        assert_eq!(band_index(44_100, 23), None, "es gibt kein Band 23");
    }

    /// `pretab` unterscheidet lang und kurz.
    ///
    /// GEMESSEN 02.10.2026: bei langen Blöcken ist `pretab` überall 0. Das ist
    /// der Grund, warum `scalefac_compress == 0` in einem langen Granulat
    /// überhaupt funktioniert: der Bandindex fällt heraus.
    #[test]
    fn pretab_unterscheidet_lang_und_kurz() {
        assert!(PRETAB_LANG.iter().all(|&p| p == 0), "lang ist ueberall 0");
        assert_eq!(PRETAB_KURZ[0], 0);
        assert_eq!(PRETAB_KURZ[11], 1, "Band 11 hat Prefaktor 1");
        assert_eq!(PRETAB_KURZ[17], 3, "Band 17 hat Prefaktor 3");
        assert_eq!(PRETAB_KURZ[21], 0, "das letzte Band ist wieder 0");
        assert_eq!(pretab(44_100, 17, true), Some(3));
        assert_eq!(pretab(44_100, 17, false), Some(0));
        assert_eq!(pretab(44_100, 22, true), None, "es gibt kein Band 22");
        assert_eq!(pretab(44_100, 23, true), None, "es gibt kein Band 23");
        assert_eq!(
            pretab(8_000, 17, true),
            Some(3),
            "pretab haengt nicht an der Rate"
        );
        //
        // GEMESSEN: die Tabelle faellt am Ende, sie steigt nicht.
        assert_eq!(PRETAB_KURZ[20], 2, "Band 20 hat wieder 2");
        assert_eq!(PRETAB_KURZ[21], 0, "und Band 21 ist wieder 0");
    }

    /// Und die **kurzen** Bandgrenzen, damit die Zahl 13 nicht unverstanden
    /// bleibt.
    ///
    /// GEMESSEN 02.10.2026: `ff_band_size_short` hat **13** Spalten für 576
    /// Linien, und jede Zeile summiert sich wieder zu 576.
    #[test]
    fn die_kurzen_bandgressen_ergeben_auch_576() {
        const KURZ: [[u8; 13]; 9] = [
            [4, 4, 4, 4, 6, 8, 10, 12, 14, 18, 22, 30, 56],
            [4, 4, 4, 4, 6, 6, 10, 12, 14, 16, 20, 26, 66],
            [4, 4, 4, 4, 6, 8, 12, 16, 20, 26, 34, 42, 12],
            [4, 4, 4, 6, 6, 8, 10, 14, 18, 26, 32, 42, 18],
            [4, 4, 4, 6, 8, 10, 12, 14, 18, 24, 32, 44, 12],
            [4, 4, 4, 6, 8, 10, 12, 14, 18, 24, 30, 40, 18],
            [4, 4, 4, 6, 8, 10, 12, 14, 18, 24, 30, 40, 18],
            [4, 4, 4, 6, 8, 10, 12, 14, 18, 24, 30, 40, 18],
            [8, 8, 8, 12, 16, 20, 24, 28, 36, 2, 2, 2, 26],
        ];
        for (i, zeile) in KURZ.iter().enumerate() {
            let summe: u32 = zeile.iter().map(|&b| u32::from(b)).sum();
            //
            // GEMESSEN 02.10.2026: kurze Bänder summieren sich auf **192**,
            // nicht auf 576. Ich hatte 576 behauptet, weil ich die langen
            // Bänder im Kopf hatte. 192 x 3 = 576: ein kurzer Granulat hat
            // drei Fenster, und die Bandzerlegung gilt pro Fenster.
            assert_eq!(
                summe, KURZE_BAENDER_SUMME,
                "kurze Zeile {i} ({} Hz) ergibt {summe} statt 192",
                ABTASTRATEN[i]
            );
        }
    }
}
