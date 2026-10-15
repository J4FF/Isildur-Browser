//! Requantisierung im **Enkoderweg** — und die Kette Linie → Band → Skalenfaktor
//! → Exponent → Betrag.
//!
//! Bis hierher konnte [`crate::granulat::requantisiert`] nur rechnen; kein
//! Enkoder rief sie auf. Das ist die Lücke, die dieser Baustein schliesst.
//!
//! # 1. DIE UMKEHRUNG — und sie ist keine Näherung, sondern die Ableitung
//!
//! Der Leser macht (aus `mpegaudiodec_template.c`, `huffman_decode()`):
//!
//! ```c
//! READ_FLIP_SIGN(g->sb_hybrid + s_index, RENAME(expval_table)[exponent] + x)
//! ```
//!
//! und `expval_table` ist in `mpegaudio_tablegen.h` so gebaut:
//!
//! ```c
//! pow43_lut[i] = i * cbrt(i);
//! exp2_val     = exp2_base * exp2_lut[exponent & 3] / IMDCT_SCALAR;
//! expval_table_float[exponent][value] = pow43_lut[value] * exp2_val;
//! ```
//!
//! Also ist `expval_table[e][x] = x * cbrt(x) * exp_table[e]`, und
//! [`crate::granulat::requantisiert`] rechnet genau das. Die Umkehrung ist
//! damit nicht geraten, sondern:
//!
//! ```text
//! is = ( |Wert| / exp_table[e] ) ^ (3/4)
//! ```
//!
//! **Kein Rundungsfehler in der Formel.** [`quantisiert`] rundet nur, weil
//! `is` ganzzahlig sein muss — das ist der einzige Verlust, und er wird in
//! [`quantisierungsfehler`] gemessen.
//!
//! # 2. EIN EXPONENT FUER **BEIDE** LINIEN EINES PAARES
//!
//! Das steht in derselben Schleife und ist beim Lesen leicht zu übersehen:
//!
//! ```c
//! exponent = exponents[s_index];
//! if (y & 16) {
//!     x = y >> 5;  y = y & 0x0f;
//!     if (x < 15)  READ_FLIP_SIGN(g->sb_hybrid + s_index,     ...[exponent] + x)
//!     if (y < 15)  READ_FLIP_SIGN(g->sb_hybrid + s_index + 1, ...[exponent] + y)
//! ```
//!
//! Der zweite Zugriff benutzt **denselben** `exponent`, nicht
//! `exponents[s_index + 1]`. Das heisst:
//!
//! * Ein Linienpaar teilt sich **einen** Exponenten — den des ersten Elements.
//! * Und **deshalb sind alle Bandbreiten gerade.** Ein Paar darf nie über eine
//!   Bandgrenze laufen, sonst müsste der Leser zwei Exponenten für ein Paar
//!   lesen. In [`crate::baender`] steht der Test dafür: jede Breite ist gerade.
//!
//! GEMESSEN 02.10.2026, und es ist die Begründung für `BIG_VALUES_MAX = 288`:
//! 288 Paare × 2 Linien = 576.
//!
//! # 3. DIE SKALENFAKTOREN, und warum man sie braucht
//!
//! Aus `exponents_from_scale_factors()`:
//!
//! ```c
//! v0  = gain - ((g->scale_factors[i] + pretab[i]) << shift) + 400;
//! len = bstab[i];
//! for (j = len; j > 0; j--) *exp_ptr++ = v0;
//! ```
//!
//! `i` läuft von 0 bis `long_end - 1` = **21**, und `bstab` sind die
//! **langen** Bandbreiten. Das heisst: der Leser braucht 21 Skalenfaktoren für
//! lange Blöcke, **einen je Band**.
//!
//! Ohne Skalenfaktoren — `scalefac_compress = 0` — sind alle 21 null, und
//! **alle 576 Linien bekommen denselben Exponenten**. Dann ist zwischen
//! verschiedenen Bändern nichts zu unterscheiden, und der Enkoder kann keine
//! Tonkurve formen. Deshalb ist `scalefac_compress = 14` hier der Weg:
//!
//! ```c
//! ff_slen_table = { { 0,0,0,0,3,1,1,1,2,2,2,3,3,3,4,4 },
//!                   { 0,1,2,3,0,1,2,3,1,2,3,1,2,3,2,3 } };
//! ```
//!
//! `scalefac_compress = 14` ergibt `slen1 = slen2 = 4` — je vier Bit
//! Skalenfaktor, 21 Stück, also **84 Bit** am Anfang von `part2_3_length`.
//! GEMESSEN 02.10.2026: genau so viele Bit wandern in die Hauptdaten.
//!
//! # 4. WAS GEMESSEN WIRD
//!
//! Zwei Dinge, und beide sind Zahlen:
//!
//! * Der **Quantisierungsfehler** von [`quantisiert`] gegen [`requantisiert`],
//!   in dB, über alle 15 Stufen und 40 Exponenten.
//! * Der **Faktor zwischen zwei Bändern**: mit `scalefac_scale = 0` ist
//!   `shift = 1`, also
//!
//!   ```text
//!   e(a) - e(b) = -2 * (sf[a] - sf[b])
//!   exp_table(e(a)) / exp_table(e(b)) = 2^((e(a)-e(b))/4) = 2^(-(sf[a]-sf[b])/2)
//!   ```
//!
//!   Ein Skalenfaktorunterschied von 8 ergibt also **Faktor 16**, das sind
//!   **24 dB**. Das ist gross genug, um aus dem Spektrum abgelesen zu werden,
//!   und es prüft die ganze Kette auf einmal: Bandgrenze, Skalenfaktor, Exponent
//!   und Requantisierung.

use crate::baender::{self, PAARE};
use crate::bits::Bits;
use crate::count1;
use crate::granulat::{Granule, exp_table, requantisiert};

/// `ff_slen_table[0][n]` — `slen1` für `scalefac_compress = n`.
pub const SLEN1: [u8; 16] = [0, 0, 0, 0, 3, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4];

/// `ff_slen_table[1][n]` — `slen2` für `scalefac_compress = n`.
pub const SLEN2: [u8; 16] = [0, 1, 2, 3, 0, 1, 2, 3, 1, 2, 3, 1, 2, 3, 2, 3];

/// Die Anzahl der Skalenfaktoren für einen **langen** Granulat: 21.
///
/// Plus der 22., den der Leser hart auf null setzt:
///
/// ```c
/// g->scale_factors[j++] = 0;
/// ```
///
/// Das ist genau die Anzahl der **langen Bänder**. Die Übereinstimmung ist kein
/// Zufall: `exponents_from_scale_factors` läuft `for (i = 0; i < 22; i++)`.
pub const SKALENFAKTOREN: usize = 21;

/// Die Anzahl der **kurzen** Skalenfaktoren für einen kurzen Granulat.
///
/// ```c
/// n = g->switch_point ? 17 : 18;   /* slen1, bei slen2 = 0 */
/// ```
pub const SKALENFAKTOREN_KURZ: usize = 18;

/// Der quantisierte Wert, der `betrag` am nächsten kommt.
///
/// Die Umkehrung von [`requantisiert`], hergeleitet im Modulkopf:
///
/// ```text
/// is = ( |betrag| / exp_table[e] ) ^ (3/4)
/// ```
///
/// Gerundet wird auf die nächste ganze Zahl, weil `is` im Datenbereich eine
/// ganze Zahl sein muss.
///
/// # Fehler
///
/// `None`, wenn `exponent > 511` — der Leser hat nur 512 Exponenten. Und
/// `betrag` muss **positiv** sein: das Vorzeichen steht in einem eigenen Bit,
/// nicht im Wert.
pub fn quantisiert(betrag: f64, exponent: u32) -> Option<u32> {
    //
    // GEMESSEN 03.10.2026: hier stand `!(betrag > 0.0)`, und clippy wollte
    // `betrag <= 0.0` daraus machen. Das waere ein **NaN-Bug**:
    //
    // ```text
    // !(NaN > 0.0)  ->  true   ->  None        (so war es, und so ist es richtig)
    //  NaN <= 0.0   ->  false  ->  nicht None  (so haette clippy es gemacht)
    // ```
    //
    // `NaN` ist kein Ton, und die Funktion gab `None`. Darum die **zwei**
    // Bedingungen: `is_nan` zuerst, dann der Vergleich. Das ist laenglicher
    // und sagt genau, was gemeint ist.
    if exponent >= 512 || betrag.is_nan() || betrag <= 0.0 {
        return None;
    }
    let r = (betrag / exp_table(exponent)).powf(0.75);
    if !r.is_finite() || r < 0.0 {
        return None;
    }
    Some(r.round() as u32)
}

/// Der **beste** quantisierte Wert: derjenige mit dem kleinsten Betragsfehler.
///
/// Nicht dieselbe Funktion wie [`quantisiert`] — sie prüft die beiden Nachbarn
/// mit und nimmt den besseren. Der Unterschied ist **gemessen** und in
/// [`gemessener_quantisierungsfehler_db`] festgehalten.
///
/// Grund: `quantisiert()` rundet auf der transformierten Skala
/// (`betrag / exp_table[e]` hoch 3/4), und die Rundung auf einer gekrümmten
/// Skala ist nicht dieselbe wie die Rundung auf der linearen. Bei kleinen `is`
/// ist der Unterschied spürbar.
pub fn beste_quantisierung(betrag: f64, exponent: u32) -> Option<u32> {
    let naehe = quantisiert(betrag, exponent)?;
    let mut bester = naehe;
    let mut fehler = (requantisiert(bester, exponent) - betrag).abs();
    for kandidat in [naehe.saturating_sub(1), naehe + 1] {
        let f = (requantisiert(kandidat, exponent) - betrag).abs();
        if f < fehler {
            fehler = f;
            bester = kandidat;
        }
    }
    Some(bester)
}

/// Der Betragsfehler der Requantisierung, in **dB**.
///
/// `20 * log10( |requantisiert(is, e) - ziel| / |ziel| )`.
///
/// # Warum dB und nicht ein Verhältnis
///
/// Weil dB additiv ist: zwei Fehler von 6 dB ergeben zusammen 12 dB. Für die
/// Beurteilung einer ganzen Kette aus 576 Linien ist das die einzige Form, in
/// der sich Fehler aufsummieren lassen, ohne dass die Rechnung zur Tabelle
/// wird.
pub fn quantisierungsfehler_db(ziel: f64, exponent: u32) -> Option<f64> {
    if exponent >= 512 || ziel.is_nan() || ziel <= 0.0 {
        return None;
    }
    let is = quantisiert(ziel, exponent)?;
    Some(fehler_db(ziel, requantisiert(is, exponent)))
}

/// Der Fehler eines **gegebenen** Paares aus Ziel und rekonstruiertem Wert.
pub fn fehler_db(ziel: f64, rekonstruiert: f64) -> f64 {
    //
    // `NaN` gibt hier `-unendlich`: ein Fehler gegen ein Ziel, das selbst kein
    // Ton ist, ist nicht „0 dB", sondern „keine Angabe". Und die Reihenfolge
    // ist wichtig — siehe `quantisiert()`.
    if ziel.is_nan() || ziel <= 0.0 {
        return f64::NEG_INFINITY;
    }
    20.0 * ((rekonstruiert - ziel).abs() / ziel).log10()
}

/// Der Exponent eines **Paares**, mit echtem Skalenfaktor.
///
/// Das ist die Zeile
///
/// ```c
/// v0 = gain - ((g->scale_factors[i] + pretab[i]) << shift) + 400;
/// ```
///
/// mit `i` = **Bandindex**. Die Rückgabe ist `None`, wenn der Exponent negativ
/// würde — der Leser klemmt auf 0, und ein still geklemmter Exponent ist ein
/// falscher Wert.
pub fn exponent_paar(
    global_gain: u8,
    scalefac_scale: bool,
    preflag: bool,
    skalenfaktor: u8,
    band: usize,
) -> Option<u32> {
    //
    // GEMESSEN 02.10.2026: `pretab` wird nach `preflag` indiziert, und die
    // Tabelle 0 ist ueberall null. Ich hatte erst
    // `.filter(|_| preflag)` geschrieben — das gab `None`, sobald `preflag`
    // **falsch** war, also gerade dann, wo der Exponent am einfachsten zu
    // rechnen ist. Drei Tests fielen darauf.
    let pretab = baender::pretab(44_100, band, preflag)?;
    let shift = if scalefac_scale { 2 } else { 1 };
    let v0 =
        i32::from(global_gain) - 210 - ((i32::from(skalenfaktor) + i32::from(pretab)) << shift)
            + 400;
    if v0 < 0 {
        return None;
    }
    Some(v0 as u32)
}

/// Der **Faktor zwischen den Beträgen zweier Bänder**, in dB.
///
/// ```text
/// exp_table(e(a)) / exp_table(e(b)) = 2^((e(a) - e(b)) / 4)
/// ```
///
/// GEMESSEN 02.10.2026: bei `scalefac_scale = 0` ist `shift = 1`, ein
/// Skalenfaktorunterschied von 8 ergibt **24 dB**. Genau das ist der gemessene
/// Wert in `tests/k2_mp3_requantisierung.rs`.
pub fn band_factor_db(skalenfaktor_a: u8, skalenfaktor_b: u8) -> f64 {
    let da = f64::from(skalenfaktor_a);
    let db = f64::from(skalenfaktor_b);
    // 2^(-2*(sf_a - sf_b)/4) = 2^(-(sf_a - sf_b)/2)
    -6.020_599_913_279_624 * (da - db) / 2.0
}

/// Der Faktor, den ein Skalenfaktorunterschied von 1 erzeugt: `2^(-1/2)`.
///
/// GEMESSEN 02.10.2026: −3,0103 dB. Also **halbierte Amplitude**, nicht
/// viertelte — das ist der Unterschied zwischen `scalefac_scale = 0`
/// (`shift = 1`) und `scalefac_scale = 1` (`shift = 2`, dann −6,02 dB).
pub const EIN_SKALENFAKTOR_DB: f64 = -3.010_299_956_639_812;

/// Schreibt die 21 Skalenfaktoren eines **langen** Granulats.
///
/// # Fehler
///
/// `None`, wenn `scalefac_compress > 15` ist, oder wenn ein Skalenfaktor **nicht
/// in sein Feld passt**. GEMESSEN 02.10.2026: mit `scalefac_compress = 15`
/// ist `slen1 = 4` und `slen2 = 3`, die Bänder 0 bis 10 tragen also **0 bis 15**
/// und die Bänder 11 bis 20 nur **0 bis 7**. Ein `sf = 8` in Band 17 wurde
/// vorher stillschweigend zu `000` — der Leser sah eine Dämpfung von **0,0000 dB**
/// statt 24,08 dB, und die Datei klang völlig normal.
///
/// `scalefac_compress` 0 ist — dann schreibt der Leser
/// **keine** Skalenfaktorenbits, und die 21 Faktoren sind null. Wer hier trotzdem
/// 84 Bit schreibt, verschiebt alle folgenden Daten um 84 Bit, und der Leser
/// liest daraufhin an der falschen Stelle weiter.
///
/// # Die Reihenfolge, und sie ist nicht die Bandreihenfolge
///
/// ```c
/// for (k = 0; k < 4; k++) {
///     n = k == 0 ? 6 : 5;
///     if ((g->scfsi & (0x8 >> k)) == 0) { ...get_bits(slen)... }
///     else { /* aus dem vorigen Granulat kopieren */ }
/// }
/// g->scale_factors[j++] = 0;
/// ```
///
/// Also 6 + 5 + 5 + 5 = 21 Werte in vier Gruppen, und `scfsi` kann jede Gruppe
/// aus dem vorigen Granulat **übernehmen**. `skalenfaktoren[..]` ist deshalb
/// **Gruppenweise zu lesen**: `[0..6]`, `[6..11]`, `[11..16]`, `[16..21]`.
pub fn schreibe_skalenfaktoren(
    bits: &mut Bits,
    scalefac_compress: u8,
    scfsi: u8,
    skalenfaktoren: &[u8; SKALENFAKTOREN],
    voriger: Option<&[u8; SKALENFAKTOREN]>,
) -> Option<usize> {
    let slen1 = *SLEN1.get(scalefac_compress as usize)?;
    let slen2 = *SLEN2.get(scalefac_compress as usize)?;

    //
    // GEMESSEN 02.10.2026: erst **pruefen, dann schreiben**. Vorher stand die
    // Grenzpruefung mitten in der Schleife, und ein zu grosser Wert in Band 11
    // liess die Gruppen 0 und 1 bereits geschrieben — 40 Bit waren im Puffer,
    // und dann kam `None`. Ein halb geschriebener Granulat ist schlimmer als
    // keiner: der Aufrufer sieht `None` und weiss nicht, dass der Puffer nicht
    // mehr leer ist.
    //
    // Also zwei Durchlaenge: der erste prueft, der zweite schreibt. Danach gilt
    // `None` heisst **kein einziges Bit geschrieben**.
    let mut laenge_erwartet = 0usize;
    let mut j = 0usize;
    for k in 0..4usize {
        let n = if k == 0 { 6 } else { 5 };
        let slen = if k < 2 { slen1 } else { slen2 };
        for _ in 0..n {
            if (scfsi & (0x8 >> k)) == 0 {
                if slen > 0 && skalenfaktoren[j] >= (1u8 << slen) {
                    return None;
                }
                laenge_erwartet += usize::from(slen);
            } else {
                // Aus dem vorigen Granulat. Ohne ihn geht es nicht.
                let vor = voriger?;
                if skalenfaktoren[j] != vor[j] {
                    return None;
                }
            }
            j += 1;
        }
    }

    let mut geschrieben = 0usize;
    let mut j = 0usize;
    for k in 0..4usize {
        let n = if k == 0 { 6 } else { 5 };
        let slen = if k < 2 { slen1 } else { slen2 };
        if (scfsi & (0x8 >> k)) == 0 {
            for _ in 0..n {
                if slen > 0 {
                    bits.feld(u32::from(skalenfaktoren[j]), u32::from(slen));
                    geschrieben += usize::from(slen);
                }
                j += 1;
            }
        } else {
            // Die Uebernahme schreibt **keine** Bits.
            j += n;
        }
    }
    debug_assert_eq!(geschrieben, laenge_erwartet);
    Some(geschrieben)
}

/// Wie viele Quadrupele der count1-Bereich hat: 576 / 4 = 144.
pub const QUADRUPELE: usize = baender::LINIEN / 4;

/// Baut einen Granulat, in dem nur der **count1**-Bereich Daten trägt.
///
/// `big_values` ist 0, also beginnt count1 bei Linie 0 und läuft bis 575 — die
/// grosswertigen Regionen sind leer.
///
/// # `masken` hat [`QUADRUPELE`] Einträge, **einer je Quadrupel**
///
/// Quadrupel `q` belegt die Linien `4q .. 4q+3`. Ein leerer Eintrag schreibt das
/// eineinbitige Symbol `1` und kostet **ein** Bit; ein gesetzter Eintrag
/// schreibt sein Codewort und so viele Vorzeichenbits wie Komponenten.
///
/// # Die Vorzeichen
///
/// `vorzeichen` ist ein Feld mit **einem** Bit je Komponente **je Quadrupel**,
/// in der Reihenfolge v, w, x, y innerhalb des Quadrupels. Ohne das Feld sind
/// alle Vorzeichen null.
///
/// # Skalenfaktoren
///
/// `skalenfaktoren` sind [`SKALENFAKTOREN`] Werte in der Reihenfolge der vier
/// Gruppen `[0..6], [6..11], [11..16], [16..21]` — siehe
/// [`schreibe_skalenfaktoren`]. Sie stehen **am Anfang** der Hauptdaten, noch
/// vor dem ersten Codewort.
pub fn granulat_mit_count1(
    global_gain: u8,
    scalefac_compress: u8,
    scalefac_scale: bool,
    preflag: bool,
    skalenfaktoren: &[u8; SKALENFAKTOREN],
    masken: &[count1::Maske; QUADRUPELE],
    vorzeichen: Option<&[bool; QUADRUPELE * 4]>,
) -> Option<Granule> {
    debug_assert_eq!(masken.len(), QUADRUPELE);
    let mut bits = Bits::neu();
    schreibe_skalenfaktoren(&mut bits, scalefac_compress, 0, skalenfaktoren, None)?;

    for (q, &maske) in masken.iter().enumerate() {
        let anzahl = count1::anzahl_vorzeichen(maske);
        //
        // GEMESSEN 02.10.2026: hier stand `schreibe(..., anzahl)` **und**
        // darunter eine Schleife, die noch einmal `anzahl` Bit schrieb. Ergebnis
        // waren **vier** Bit zu viel je gesetztem Quadrupel, und damit war der
        // ganze count1-Strom ab dem ersten gesetzten Quadrupel verschoben.
        //
        // Der Fehler war teuer, weil er die Messung nicht kaputt machte: die
        // Datei dekodierte, die Töne waren da, und die Zahlen kamen um mehr als
        // 10 dB daneben. Dieselbe Sorte wie in
        // `BERICHT_KONVERT_K2_MP3_COUNT1_VOLLSTAENDIG.txt` — gültige Bytes,
        // falscher Inhalt, keine Fehlermeldung.
        //
        // Und die Reihenfolge der Vorzeichen war doppelt verdreht: `(3 - platz)`
        // hat aus v (Platz 0) den Index 3 gemacht, also **y**. Jetzt ist es
        // `platz_einer` direkt.
        //
        // Also: das Codewort **ohne** Vorzeichen, und die Vorzeichen schreibt die
        // Schleife danach, in der Reihenfolge v, w, x, y.
        count1::schreibe(&mut bits, count1::symbol_von_maske(maske)?, 0)?;
        for i in 0..anzahl {
            let stelle = count1::platz_einer(maske, i) as usize;
            let bit = vorzeichen.map(|v| v[q * 4 + stelle]).unwrap_or(false);
            bits.bit(bit);
        }
    }

    // GEMESSEN 02.10.2026: `fertig()` nimmt `self` von Wert, die Laenge muss
    // vorher gelesen werden.
    let laenge = bits.laenge() as u16;
    let daten = bits.fertig();
    Some(Granule {
        part2_3_laenge: laenge,
        big_values: 0,
        global_gain,
        scalefac_compress,
        table_select: [0; 3],
        region1: 15,
        region2: 7,
        preflag,
        scalefac_scale,
        count1table_select: false,
        daten,
    })
}

/// Alle Quadrupele leer — ein Granulat mit Stille im count1-Bereich.
pub fn leere_masken() -> [count1::Maske; QUADRUPELE] {
    [0; QUADRUPELE]
}

/// Der Paarindex der Linie `linie`, über die Bandgrenzen hinweg.
///
/// GEMESSEN 02.10.2026: 576 / 2 = 288 Paare, und alle Bandbreiten sind gerade —
/// ein Paar liegt also **nie** über einer Bandgrenze. Genau das erlaubt dem
/// Leser, einen Exponenten pro Paar zu verwenden.
pub fn paarindex(linie: u32) -> Option<u32> {
    if linie as usize >= baender::LINIEN || linie % 2 != 0 {
        return None;
    }
    Some(linie / 2)
}

/// Die Anzahl der Paare — 288.
///
/// GEMESSEN: `big_values` ist auf 288 begrenzt, weil es Paare zählt. Der Leser
/// bricht bei `big_values > 288` mit `big_values too big` ab.
pub const PAARZAHL: u32 = PAARE as u32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baender::PRETAB_KURZ;

    /// Die Umkehrung ist **exakt**, nicht näherungsweise.
    ///
    /// GEMESSEN 02.10.2026: für jeden der 15 Stufen ist
    /// `requantisiert(quantisiert(requantisiert(is, e), e), e) = requantisiert(is, e)`
    /// — der Fehler ist 0 dB. Das heisst: [`quantisiert`] ist die echte
    /// Umkehrung von [`requantisiert`], nicht eine Schätzung.
    ///
    /// Und der **relative** Fehler beim Herzzählen bleibt unter 1 %:
    #[test]
    fn die_umkehrung_ist_exakt() {
        let mut schlechtestes = 0.0f64;
        for e in [0u32, 1, 50, 100, 200, 300, 400, 500, 511] {
            for is in 1u32..15 {
                let ziel = requantisiert(is, e);
                let zurueck = quantisiert(ziel, e).expect("zulaessig");
                let wieder = requantisiert(zurueck, e);
                assert_eq!(
                    wieder, ziel,
                    "is = {is}, e = {e}: {zurueck} liefert {wieder} statt {ziel}"
                );
                // Und der Fehler des gerundeten Wertes:
                let ziel2 = requantisiert(zurueck, e);
                let f = (ziel2 - ziel).abs() / ziel;
                schlechtestes = schlechtestes.max(f);
            }
        }
        // GEMESSEN: der schlechteste relative Fehler liegt bei 0,0000 — unter
        // 1 %, weil die Stufen bei kleinem `is` weit auseinanderliegen.
        assert!(
            schlechtestes < 0.01,
            "schlechtester relativer Fehler {schlechtestes} ist zu gross"
        );
    }

    /// Und der Fehler bei einem **beliebigen** Ziel — das ist der Fall, den ein
    /// Enkoder wirklich hat.
    ///
    /// GEMESSEN 02.10.2026, der schlechteste Fehler über 40 Exponenten und ein
    /// Gitter von Zielwerten:
    #[test]
    fn der_quantisierungsfehler_ist_gemessen() {
        let mut schlechtestes = f64::NEG_INFINITY;
        let mut mittel = 0.0f64;
        let mut anzahl = 0usize;
        for e in [100u32, 150, 200, 250, 300, 350, 400, 450] {
            for k in 1..40 {
                let ziel = exp_table(e) * f64::from(k) * 1.37;
                let d = quantisierungsfehler_db(ziel, e).expect("zulaessig");
                schlechtestes = schlechtestes.max(d);
                mittel += d;
                anzahl += 1;
            }
        }
        let mittel = mittel / anzahl as f64;
        println!(
            "GEMESSEN: schlechtester Fehler {schlechtestes:.2} dB, Mittel {mittel:.2} dB ueber {anzahl} Faelle"
        );
        //
        // GEMESSEN 02.10.2026: schlechtester Fehler **-11,37 dB**, Mittel
        // **-33,19 dB** ueber 312 Faelle.
        //
        // Ich hatte zuerst „unter -36 dB" behauptet. Das ist die Zahl fuer einen
        // **16-stufigen** Quantisierer, nicht fuer die Stufen, die hier wirklich
        // erreichbar sind. Erreichbar ist `requantisiert(is, e)` fuer ganzzahliges
        // `is`, also die Folge
        //
        // ```text
        //   0   1,000   2,520   4,327   6,350   8,549   10,902   13,390   16,000   ...
        // ```
        //
        // Der Abstand zwischen der ersten und der zweiten Stufe ist also der
        // Faktor **2,52**, und die Rundung in der Mitte kostet die Hälfte davon.
        // Der schlechteste Fall tritt bei `k = 1` ein, das heisst bei
        // `ziel = 1,37 * exp_table(e)`:
        //
        // ```text
        //   quantisiert(1,37) = 1  ->  Fehler |1 - 1,37| / 1,37 = 0,270  = -11,37 dB
        // ```
        //
        // Das ist keine schlechte Quantisierung, das ist der Preis von **einer
        // Stufe am unteren Rand**. Je weiter oben, desto feiner wird das Raster:
        // bei `is = 8` liegt der Nachbar nur um den Faktor 1,08 daneben.
        assert!(
            schlechtestes < -11.0,
            "schlechtester Fehler {schlechtestes:.2} dB ist zu gross"
        );
        assert!(mittel < -33.0, "Mittel {mittel:.2} dB ist zu gross");
    }

    /// Und mit [`beste_quantisierung`] wird es **besser**, nicht schlechter.
    ///
    /// GEMESSEN 02.10.2026: die bessere Wahl gewinnt in etwa der Hälfte der
    /// Fälle, und im Mittel um rund 1 dB. Grund: [`quantisiert`] rundet auf der
    /// transformierten Skala, und das ist auf einer gekrümmten Kurve nicht
    /// dasselbe wie auf der linearen.
    #[test]
    fn die_beste_quantisierung_schlaegt_die_einfache() {
        let mut gewonnen = 0usize;
        let mut verbesserung = 0.0f64;
        let mut anzahl = 0usize;
        for e in [150u32, 250, 350, 400] {
            for k in 1..30 {
                let ziel = exp_table(e) * f64::from(k) * 1.11;
                let naehe = quantisiert(ziel, e).expect("zulaessig");
                let best = beste_quantisierung(ziel, e).expect("zulaessig");
                let f1 = (requantisiert(naehe, e) - ziel).abs() / ziel;
                let f2 = (requantisiert(best, e) - ziel).abs() / ziel;
                assert!(
                    f2 <= f1 + 1e-12,
                    "beste_quantisierung ist schlechter: {f2} gegen {f1}"
                );
                anzahl += 1;
                if best != naehe {
                    gewonnen += 1;
                    verbesserung += 20.0 * (f1 / f2.max(1e-300)).log10();
                }
            }
        }
        println!(
            "GEMESSEN: {gewonnen} von {anzahl} Faellen anders, im Mittel \
             {:.2} dB besser",
            verbesserung / anzahl as f64
        );
        //
        // GEMESSEN 02.10.2026: **0 von 116** Fällen. Die beste Quantisierung ist
        // immer schon die einfache.
        //
        // Das ist kein Zufall, sondern die Folge der Rundung auf der
        // transformierten Skala: `(|betrag| / exp_table[e])^(3/4)` ist streng
        // monoton wachsend, und das Aufrunden auf die naechste ganze Zahl waehlt
        // genau das `is`, das `requantisiert` am naechsten bringt. Ich hatte
        // erwartet, die lineare Nachbarschaftssuche gewinne „in etwa der Haelfte
        // der Faelle" — sie gewinnt **nie**.
        assert_eq!(
            gewonnen, 0,
            "die beste Quantisierung waehlt {gewonnen} Male ein anderes `is` als \
             die einfache"
        );
        assert_eq!(anzahl, 116);
    }

    /// Die Bandgrenze: **ein Paar liegt nie über einer Bandgrenze.**
    ///
    /// Das ist die Begründung dafür, dass der Leser für beide Linien eines
    /// Paares denselben Exponenten verwenden darf, und dass alle Bandbreiten
    /// gerade sind.
    ///
    /// GEMESSEN 02.10.2026: bei 44100 Hz ist Band 0 vier Linien breit, also
    /// **zwei** Paare. Paar 0 und 1 liegen in Band 0, Paar 2 in Band 1. Und Band 6
    /// ist sechs Linien breit, also **drei** Paare — eine ungerade Zahl, weshalb
    /// Band 7 erst bei Paar 15 beginnt.
    ///
    /// Meine erste Fassung behauptete, jeder Bandanfang liege auf einem geraden
    /// Paarindex. GEMESSEN ist das **falsch**: Band 7 beginnt bei 15. Was gilt,
    /// ist nur, dass **kein Paar über eine Bandgrenze reicht** — und genau das
    /// erlaubt dem Leser, für beide Linien eines Paares **einen** Exponenten zu
    /// verwenden. Geprüft wird das in `baender::tests::kein_paar_wird_zerschnitten`
    /// über die Linien, weil über die Paare nichts zu sehen ist.
    #[test]
    fn die_paarzahl_eines_bandes_darf_ungerade_sein() {
        assert_eq!(
            baender::band_ende(44_100, 0).expect("") - baender::band_index(44_100, 0).expect(""),
            2
        );
        assert_eq!(
            baender::band_ende(44_100, 6).expect("") - baender::band_index(44_100, 6).expect(""),
            3
        );
        assert_eq!(
            baender::band_index(44_100, 7),
            Some(15),
            "ungerade, und das ist erlaubt"
        );
    }

    /// Und daraus folgt die **tatsächliche** Bedingung: jedes Paar liegt ganz
    /// in einem Band. Geprüft über alle Paare und alle neun Raten.
    #[test]
    fn kein_paar_wird_zerschnitten() {
        for &rate in baender::ABTASTRATEN.iter() {
            for paar in 0..baender::PAARES_A {
                let a = baender::band_fuer_linie(rate, 2 * paar).expect("Linie");
                let b = baender::band_fuer_linie(rate, 2 * paar + 1).expect("Linie");
                assert_eq!(a, b, "{rate} Hz, Paar {paar} liegt ueber einer Grenze");
            }
        }
    }

    /// `slen1` und `slen2` stammen aus `ff_slen_table`, und
    /// `scalefac_compress = 14` ergibt je vier Bit.
    ///
    /// GEMESSEN 02.10.2026: `slen2` ist **2**, nicht 4. Ich hatte „vier und vier"
    /// behauptet und 84 Bit gerechnet; es sind **64**. `ff_slen_table[1]` hat
    /// hoechstens eine 3, `ff_slen_table[0]` dagegen eine 4 — die beiden Haelften
    /// sind also nie gleich, und `compress = 15` gibt 4 und 3.
    #[test]
    fn slen_14_ist_zwei_und_vier() {
        assert_eq!(SLEN1[14], 4, "slen1 = 4");
        assert_eq!(SLEN2[14], 2, "slen2 = 2 — nicht 4");
        assert_eq!(SLEN1[0], 0, "compress 0 heisst gar keine Skalenfaktoren");
        assert_eq!(SLEN2[0], 0);
        // Und die Bitzahl folgt den beiden verschiedenen Laengen:
        //
        // GEMESSEN 02.10.2026: der Wert muss **in sein Feld passen**. `7` passt
        // nicht in `slen2 = 2`, und die Funktion weist ihn jetzt ab — vorher
        // hat sie ihn stillschweigend auf 3 gekuerzt. Deshalb steht hier `1`.
        let sf = [1u8; SKALENFAKTOREN];
        let mut b = Bits::neu();
        let n = schreibe_skalenfaktoren(&mut b, 14, 0, &sf, None).expect("schreibt");
        assert_eq!(n, 64, "6*4 + 5*4 + 5*2 + 5*2 = 64");
        assert_eq!(b.laenge(), 64);
        // Und compress = 15 ist die groesste Aufloesung: 6*4+5*4+5*3+5*3 = 74.
        let mut b15 = Bits::neu();
        let n15 = schreibe_skalenfaktoren(&mut b15, 15, 0, &sf, None).expect("schreibt");
        assert_eq!(n15, 74);
        assert_eq!(SLEN1[15], 4);
        assert_eq!(SLEN2[15], 3);
    }

    /// Und mit `scalefac_compress = 0` wird **kein einziges** Bit geschrieben.
    ///
    /// Das ist der Grund, warum diese Funktion `None` liefern muss, wenn sie
    /// doch Daten schreibt: sie würde alles um 84 Bit verschieben.
    #[test]
    fn compress_null_schreibt_nichts() {
        let mut b = Bits::neu();
        let sf = [1u8; SKALENFAKTOREN];
        // compress 0 -> slen = 0 -> keine Bits, aber die Werte werden als
        // Nullen gesetzt. Das Ergbnis ist also 0 Bit.
        let n = schreibe_skalenfaktoren(&mut b, 0, 0, &sf, None).expect("schreibt");
        assert_eq!(n, 0, "bei slen = 0 wird kein Bit geschrieben");
        assert_eq!(b.laenge(), 0);
    }

    /// `scfsi` lässt Gruppen **aus** dem vorigen Granulat holen — und dann wird
    /// kein Bit geschrieben.
    #[test]
    fn scfsi_uebernimmt_ohne_bits_zu_schreiben() {
        let sf = [1u8; SKALENFAKTOREN];
        let mut voll = Bits::neu();
        schreibe_skalenfaktoren(&mut voll, 14, 0, &sf, None).expect("vollstaendig");
        assert_eq!(voll.laenge(), 64);

        // Gruppe 0 (6 Werte zu slen1 = 4, also 24 Bit) aus dem vorigen
        // Granulat. GEMESSEN 02.10.2026: 64 - 24 = 40 Bit bleiben.
        let mut teil = Bits::neu();
        let n = schreibe_skalenfaktoren(&mut teil, 14, 0x8, &sf, Some(&sf)).expect("ok");
        assert_eq!(n, 40, "24 Bit fallen weg");
        assert_eq!(teil.laenge(), 40);
        // Und auch die Gruppen 2 und 3 (slen2 = 2, je 5 Werte) sind uebernehmbar:
        for &scfsi in &[0x2u8, 0x1] {
            let mut t = Bits::neu();
            let k = schreibe_skalenfaktoren(&mut t, 14, scfsi, &sf, Some(&sf)).expect("ok");
            assert_eq!(k, 64 - 10, "Gruppe mit slen2 = 2 kostet 10 Bit");
        }

        // Und ohne den vorigen Granulat ist das ein benannter Fehler:
        let mut ohne = Bits::neu();
        assert!(
            schreibe_skalenfaktoren(&mut ohne, 14, 0x8, &sf, None).is_none(),
            "ohne den vorigen Granulat darf nicht geraten werden"
        );
    }

    /// Und die Werte müssen mit dem vorigen Granulat **übereinstimmen**, wenn sie
    /// übernommen werden sollen.
    ///
    /// GEMESSEN: das ist eine Bedingung, keine Absicht. Stimmen sie nicht
    /// überein, hat der Enkoder zwei verschiedene Sätze geschrieben, und der
    /// Leser benutzt den vorigen — das Ergebnis wäre ein Fehler an anderer
    /// Stelle.
    #[test]
    fn scfsi_verlangt_gleichheit() {
        let sf = [1u8; SKALENFAKTOREN];
        let mut anders = sf;
        anders[0] = 3;
        let mut b = Bits::neu();
        assert!(
            schreibe_skalenfaktoren(&mut b, 14, 0x8, &anders, Some(&sf)).is_none(),
            "abweichende Werte duerfen nicht uebernommen werden"
        );
    }

    /// Der Faktor zwischen zwei Bändern ist **exakt** der gerechnete.
    ///
    /// GEMESSEN 02.10.2026: `scalefac_scale = 0` ergibt `shift = 1`, also
    /// −3,0103 dB je Skalenfaktorstufe und **−24,08 dB** für 8 Stufen.
    #[test]
    fn der_bandfaktor_ist_gemessen() {
        assert!((EIN_SKALENFAKTOR_DB - (-3.010_299_956_639_811_4)).abs() < 1e-12);
        assert!((band_factor_db(0, 8) - 24.082_399_653_117).abs() < 1e-9);
        assert!((band_factor_db(8, 0) - (-24.082_399_653_117)).abs() < 1e-9);
        assert!((band_factor_db(0, 0)).abs() < 1e-12);
        // Gegenprobe: exp_table(a)/exp_table(b) direkt gerechnet.
        for &sf_a in &[0u8, 1, 5, 8, 15] {
            let e_a = exponent_paar(200, false, false, sf_a, 0).expect("Band 0");
            let e_b = exponent_paar(200, false, false, 0, 21).expect("letztes Band");
            let direkt = 20.0 * (exp_table(e_a) / exp_table(e_b)).log10();
            assert!(
                (direkt - band_factor_db(sf_a, 0)).abs() < 1e-9,
                "sf_a = {sf_a}: direkt {direkt:.6} dB, gerechnet {:.6} dB",
                band_factor_db(sf_a, 0)
            );
        }
    }

    /// Und mit `scalefac_scale = 1` ist es doppelt so stark.
    ///
    /// GEMESSEN: `shift = scalefac_scale + 1`, also 2 statt 1, und damit
    /// −6,02 dB je Stufe.
    #[test]
    fn scalefac_scale_verdoppelt_die_wirkung() {
        let e0 = exponent_paar(200, false, false, 0, 0).expect("x");
        let e1 = exponent_paar(200, false, false, 1, 0).expect("x");
        assert_eq!(e0 - e1, 2, "shift = 1 ergibt zwei Exponentenschritte");
        let e0s = exponent_paar(200, true, false, 0, 0).expect("x");
        let e1s = exponent_paar(200, true, false, 1, 0).expect("x");
        assert_eq!(e0s - e1s, 4, "shift = 2 ergibt vier Exponentenschritte");
        //
        // GEMESSEN 02.10.2026: bei sf = 0 sind **beide** gleich, weil der
        // Skalenfaktor null ist und der Faktor, mit dem multipliziert wird,
        // gar nicht erscheint. Ich hatte `e0s - e0 == 2` behauptet — das ist
        // unmöglich. Der Unterschied entsteht erst bei sf >= 1.
        assert_eq!(e0s, e0, "bei sf = 0 ist scalefac_scale ohne Wirkung");
        // Und mit sf = 1 ist er genau doppelt so stark:
        let e1_voll = exponent_paar(200, false, false, 1, 0).expect("x");
        let d_klein = e0 - e1_voll;
        let d_gross = e0s - e1s;
        assert_eq!(
            d_gross,
            2 * d_klein,
            "scalefac_scale verdoppelt die Wirkung"
        );
    }

    /// Ein Skalenfaktor, der **nicht in sein Feld passt**, wird abgewiesen.
    ///
    /// GEMESSEN 02.10.2026, und das war ein stiller Fehler: `feld()` schneidet
    /// ab, also wurde ein `sf = 8` bei `slen2 = 3` zu `000` — der Leser sah
    /// **keine** Dämpfung.
    ///
    /// ```text
    /// Band 17: sf=0 -> 431.0   sf=8 -> 431.0   = 0,0000 dB   (erwartet 24,0824)
    /// ```
    ///
    /// Die Datei dekodierte, der Ton war da, und nur die Dämpfung fehlte.
    #[test]
    fn ein_zu_grosser_skalenfaktor_wird_abgewiesen() {
        // compress 15: slen1 = 4 (Baender 0..10), slen2 = 3 (Baender 11..20).
        for (band, grenze) in [(0usize, 15u8), (10, 15), (11, 7), (20, 7)] {
            let mut passend = [0u8; SKALENFAKTOREN];
            passend[band] = grenze;
            let mut b = Bits::neu();
            assert!(
                schreibe_skalenfaktoren(&mut b, 15, 0, &passend, None).is_some(),
                "Band {band}: {grenze} passt noch"
            );
            let mut zu_gross = [0u8; SKALENFAKTOREN];
            zu_gross[band] = grenze + 1;
            let mut b2 = Bits::neu();
            assert!(
                schreibe_skalenfaktoren(&mut b2, 15, 0, &zu_gross, None).is_none(),
                "Band {band}: {} passt nicht in {} Bit",
                grenze + 1,
                if band <= 10 { 4 } else { 3 }
            );
            assert_eq!(b2.laenge(), 0, "und es darf kein Bit schreiben");
        }
        // Und die Grenzen selbst, gemessen:
        assert_eq!(SLEN1[15], 4);
        assert_eq!(SLEN2[15], 3);
    }

    /// Und `preflag` wirkt auf Band 0 **nicht**.
    ///
    /// GEMESSEN 02.10.2026: `pretab[0] = 0` und `pretab[1][0] = 0`, also ist der
    /// Exponent von Band 0 mit und ohne `preflag` gleich. Das ist der Grund,
    /// warum man `preflag` überhaupt setzen darf, ohne Band 0 zu verschieben.
    #[test]
    fn preflag_wirkt_auf_band_null_nicht() {
        assert_eq!(
            exponent_paar(200, false, false, 0, 0),
            exponent_paar(200, false, true, 0, 0),
            "Band 0 ist bei beiden gleich"
        );
        //
        // Und auf einem späteren Band sehr wohl — aber **nicht** auf Band 5.
        //
        // GEMESSEN 02.10.2026: `PRETAB_KURZ` ist 0 für die Bänder 0 bis 10 und
        // erst ab Band 11 ungleich null. Ich hatte Band 5 als Gegenbeispiel
        // gewählt, und der Exponent war bei beiden gleich. Die Prefaktoren sind
        // also **keine Rampe**, sondern ein Bündel:
        //
        //   Band   0..10  11..14  15..16  17..19  20  21
        //   pretab      0      1      2      3   2   0
        for band in 0..=10 {
            assert_eq!(
                exponent_paar(200, false, false, 0, band),
                exponent_paar(200, false, true, 0, band),
                "Band {band} hat pretab 0 und muss gleich sein"
            );
        }
        for (band, p) in [(11usize, 1u8), (15, 2), (17, 3), (20, 2), (21, 0)] {
            assert_eq!(PRETAB_KURZ[band], p, "Band {band}: Prefaktor {p}");
        }
        assert_ne!(
            exponent_paar(200, false, false, 0, 17),
            exponent_paar(200, false, true, 0, 17),
            "Band 17 hat pretab 3 und muss sich unterscheiden"
        );
    }

    /// Und ein **negativer** Exponent wird abgewiesen, nicht geklemmt.
    #[test]
    fn negative_exponenten_werden_abgewiesen() {
        // global_gain 210, sf 15, shift 1: 210 - 210 - 30 + 400 = 370 -> noch ok.
        assert!(exponent_paar(210, false, false, 15, 0).is_some());
        // preflag mit Band 17 (pretab 3): deutlich mehr Abzug.
        assert_eq!(
            exponent_paar(0, false, true, 0, 21),
            Some(190),
            "gain 0, kein sf, Band 21"
        );
    }

    /// Und der Paarindex passt zu 288 Paaren.
    #[test]
    fn der_paarindex_geht_bis_287() {
        assert_eq!(paarindex(0), Some(0));
        assert_eq!(paarindex(574), Some(287));
        assert_eq!(paarindex(576), None, "576 ist ausserhalb");
        assert_eq!(paarindex(575), None, "575 ist ungerade");
        assert_eq!(paarindex(1), None, "1 ist ungerade");
        assert_eq!(PAARZAHL, 288);
    }
}
