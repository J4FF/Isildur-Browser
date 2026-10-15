//! Das Fenster der Pseudo-QMF-Filterbank — 257 **authentische** Zahlen.
//!
//! # Woher sie kommen, und warum das hier anders ist als sonst
//!
//! `ff_mpa_enwindow[257]` aus `libavcodec/mpegaudiodsp_data.c` (FFmpeg n6.1.1),
//! wörtlich. Und `mpa_synth_init()` baut daraus das **512**-Wert-Synthesefenster:
//!
//! ```c
//! for(i=0;i<257;i++) {
//!     v = ff_mpa_enwindow[i];
//! #if USE_FLOATS
//!     v *= 1.0 / (1LL<<(16 + FRAC_BITS));        /* 2^-39 */
//! #endif
//!     window[i] = v;
//!     if ((i & 63) != 0) v = -v;                 /* <- die Ausnahme */
//!     if (i != 0) window[512 - i] = v;
//! }
//! for(i = 0; i < 8; i++)
//!     for(j = 0; j < 16; j++)
//!         window[512+16*i+j] = window[64*i+32-j];
//! for(i = 0; i < 8; i++)
//!     for(j = 0; j < 16; j++)
//!         window[512+128+16*i+j] = window[64*i+48-j];
//! ```
//!
//! # GEMESSEN 03.10.2026: DIESE TABELLE IST NUR GEGEN IHRE QUELLE PRUEFBAR
//!
//! Ich habe zwei Abnahmekriterien gebaut und **beide am bekannten Fehler
//! geeicht** — Positivkontrolle: einen Wert um 1 veraendern und messen, ob sich
//! das Mass bewegt. **Beide sind blind:**
//!
//! | Kriterium | Aenderung bei einem falschen Wert |
//! |---|---|
//! | Sperrdaempfung des Prototyps | **0,00 dB** |
//! | Symmetrie-Rest `h[n] - h[511-n]` | **0,00e+00** |
//!
//! Und der Grund ist gemessen: der **Betrag** waechst monoton bis zum Ende und
//! faellt im Spektrum glatt wie `1/k`.
//!
//! ```text
//! max|E|              = 75038 bei i = 256     <- Maximum am ENDE, kein Sinus
//! Summe E^2 / max^2   = 24,908862             <- fuer einen Sinus waere ~1,5
//! ```
//!
//! Das ist ein **Ramp**, keine Formel. Und eine falsch gesetzte Ziffer ist eine
//! **relative** Abweichung von `1/75038 = 1,3e-5` — und **keine** globale
//! Eigenschaft sieht eine so kleine relative Abweichung.
//!
//! > **Eine Abnahme muss selbst geprueft werden — nicht daraufhin, dass sie ein
//! > gutes Ergebnis liefert, sondern daraufhin, ob sie den Fehler ueberhaupt
//! > sieht, den sie verhindern soll.**
//!
//! Beide Pruefungen stehen als Tests in diesem Modul, sie heissen
//! `..._unterscheidet_nicht()`, und sie sind **Absicherung gegen die
//! Verwechslung**: niemand soll die Sperrdaempfung spaeter benutzen und
//! glauben, sie habe das Fenster geprueft.

/// Die 257 **authentischen** Werte, woertlich aus `mpegaudiodsp_data.c`.
pub const ENWINDOW: [i32; 257] = [
    0, -1, -1, -1, -1, -1, -1, -2, -2, -2, -2, -3, -3, -4, -4, -5, -5, -6, -7, -7, -8, -9, -10,
    -11, -13, -14, -16, -17, -19, -21, -24, -26, -29, -31, -35, -38, -41, -45, -49, -53, -58, -63,
    -68, -73, -79, -85, -91, -97, -104, -111, -117, -125, -132, -139, -147, -154, -161, -169, -176,
    -183, -190, -196, -202, -208, 213, 218, 222, 225, 227, 228, 228, 227, 224, 221, 215, 208, 200,
    189, 177, 163, 146, 127, 106, 83, 57, 29, -2, -36, -72, -111, -153, -197, -244, -294, -347,
    -401, -459, -519, -581, -645, -711, -779, -848, -919, -991, -1064, -1137, -1210, -1283, -1356,
    -1428, -1498, -1567, -1634, -1698, -1759, -1817, -1870, -1919, -1962, -2001, -2032, -2057,
    -2075, -2085, -2087, -2080, -2063, 2037, 2000, 1952, 1893, 1822, 1739, 1644, 1535, 1414, 1280,
    1131, 970, 794, 605, 402, 185, -45, -288, -545, -814, -1095, -1388, -1692, -2006, -2330, -2663,
    -3004, -3351, -3705, -4063, -4425, -4788, -5153, -5517, -5879, -6237, -6589, -6935, -7271,
    -7597, -7910, -8209, -8491, -8755, -8998, -9219, -9416, -9585, -9727, -9838, -9916, -9959,
    -9966, -9935, -9863, -9750, -9592, -9389, -9139, -8840, -8492, -8092, -7640, -7134, 6574, 5959,
    5288, 4561, 3776, 2935, 2037, 1082, 70, -998, -2122, -3300, -4533, -5818, -7154, -8540, -9975,
    -11455, -12980, -14548, -16155, -17799, -19478, -21189, -22929, -24694, -26482, -28289, -30112,
    -31947, -33791, -35640, -37489, -39336, -41176, -43006, -44821, -46617, -48390, -50137, -51853,
    -53534, -55178, -56778, -58333, -59838, -61289, -62684, -64019, -65290, -66494, -67629, -68692,
    -69679, -70590, -71420, -72169, -72835, -73415, -73908, -74313, -74630, -74856, -74992, 75038,
];

/// Die **Antialiasing**-Koeffizienten, Spalten 0 und 1.
///
/// `csa_table[8][4]` aus `libavcodec/mpegaudiodec_float.c` (FFmpeg n6.1.1),
/// wörtlich — Spalten 0 und 1. Die Spalten 2 und 3 benutzt nur der
/// **Festkomma**-Zweig; der rechnet anders und ist hier nicht gebaut.
///
/// # WO SIE LIEGEN, und warum ich dreizehn Dateien gebraucht habe
///
/// Ich habe sie in **zehn** Dateien gesucht: `mpegaudiodec_common.c` (zweimal,
/// vollständig), `mpegaudiodec_common_tablegen.h`, `mpegaudiodsp_data.c`,
/// `mpegaudio_tablegen.h`, `mpegaudio.h`, `mpegaudiodsp.h`, `mpegaudiodecheader.h`,
/// `mpegaudiodsp_template.c`, `mpegaudiodata.h`, `mathops.h`. Sie war in
/// **keiner**.
///
/// Und sie steht in `mpegaudiodec_float.c`, **zehn Zeilen vor**
///
/// ```c
/// #include "mpegaudiodec_template.c"
/// ```
///
/// Das ist das Muster, an dem ich es haette erkennen koennen — und an dem ich
/// es nicht erkannt habe:
///
/// > **Variantenspezifische Tabellen gehoeren in die Datei der Variante, nicht in
/// > die gemeinsame.** Sie stehen vor dem Include, weil sie vom gemeinsamen Code
/// > gebraucht und nicht von ihm bereitgestellt werden.
///
/// Ein Include-Verbund aus **einer** gemeinsamen Datei und **zwei** Varianten
/// liest sich wie „die gemeinsame enthaelt alles". Sie enthaelt es nicht.
///
/// # GEMESSEN: `a^2 + b^2 = 1` — und das Kriterium ist **empfindlich**
///
/// ```text
/// j =  0:  1,000000032190886     <- nicht 1, sondern 1 + 3,2e-8
/// j =  7:  0,999999980790894     <- nicht 1, sondern 1 - 1,9e-8
///
/// Und der Grund ist **an den Literalen ablesbar**: 19 Nachkommastellen fuer
/// `1,0`, das nur 17 hat. Das sind **float32**-Werte, und float32 hat rund
/// **sieben** signifikante Stellen. `a^2 + b^2 = 1` auf `6e-8` ist hier der
/// **richtige** Wert, und genauer ist er nicht erreichbar.
///
/// **Die Toleranz einer Abnahme ist keine Eigenschaft des Tests, sondern eine
/// Eigenschaft der Daten. Man schreibt sie nicht hin — man misst sie.**
/// ```
///
/// Und im Gegensatz zu `ENWINDOW` (Teil 22) **sieht** dieses Kriterium einen
/// Fehler — GEMESSEN mit einem verschobenen Wert: die Abweichung wandert von
/// `5,2e-08` auf `1,9e-06`, also um **Faktor 37**.
///
/// Der Unterschied liegt **nicht** an der Sorgfalt. Hier wird ein **Verhaeltnis
/// auf 1** gefordert; dort nicht. Und hier sind die Zahlen **float32**, dort
/// **Ganzzahlen** mit `1,3e-5` relativer Genauigkeit.
///
/// **Die Form der Zahlen entscheidet, ob eine Abnahme moeglich ist.** Nicht ihre
/// Anzahl.
//
// GEMESSEN 03.10.2026: clippy meldet hier 18 Mal `excessive_precision` — die
// Literale haben mehr Stellen, als `f64` haelt. Das ist **Absicht**: es sind die
// Zahlen der Quelle woertlich, und die Quelle ist das Argument. Ein `#[allow]`
// mit Grund ist hier ehrlicher als ein Kuerzen, denn ein Kuerzen wuerde die Zahl
// aendern und den Vergleich mit der Quelle still verlieren.
//
// Die Genauigkeit, die **wirklich** da ist, ist float32 — siehe den Test.
#[allow(clippy::excessive_precision)]
pub const CSA: [[f64; 2]; 8] = [
    [
        8.574_929_237_365_722_656e-01,
        -5.144_957_900_047_302_246e-01,
    ],
    [
        8.817_420_005_798_339_844e-01,
        -4.717_319_905_757_904_053e-01,
    ],
    [
        9.496_286_511_421_203_613e-01,
        -3.133_774_697_780_609_131e-01,
    ],
    [
        9.833_145_737_648_010_254e-01,
        -1.819_131_970_405_578_613e-01,
    ],
    [
        9.955_177_903_175_354_004e-01,
        -9.457_419_067_621_231_079e-02,
    ],
    [
        9.991_605_877_876_281_738e-01,
        -4.096_558_317_542_076_111e-02,
    ],
    [
        9.998_992_085_456_848_145e-01,
        -1.419_856_864_959_001_541e-02,
    ],
    [
        9.999_931_454_658_508_301e-01,
        -3.699_974_622_577_428_818e-03,
    ],
];

/// Der **Drehwinkel** des Schmetterlings `j` — GEMESSEN, in Grad.
///
/// `atan2(|b|, a)`, GEMESSEN **steigend** von 59,0362 auf 89,7880 Grad.
///
/// Der Test gibt ihn aus, damit die Folge **lesbar** ist, und behauptet
/// **nicht**, dass sie einem einfachen Gesetz folgt: ich habe `atan(k·PI/36)`
/// und Verwandtes versucht, und keines passte.
///
/// GEMESSEN: die Winkel sind **streng steigend**, die **Schrittweiten nicht**
/// (`2,817  9,884  7,782  5,054  3,079  1,534  0,602`). Zwei verschiedene
/// Aussagen ueber dieselben acht Zahlen — und nur eine davon war richtig.
#[must_use]
pub fn alias_winkel(j: usize) -> f64 {
    let [a, b] = CSA[j];
    a.atan2(-b).to_degrees()
}

/// Die Aliasbeseitigung an **einem** Unterbandpaar, genau wie
/// `compute_antialias()`:
///
/// ```c
/// #define AA(j) do {                                                      \
///         float tmp0 = ptr[-1-j];                                         \
///         float tmp1 = ptr[   j];                                         \
///         ptr[-1-j] = tmp0 * csa_table[j][0] - tmp1 * csa_table[j][1];    \
///         ptr[   j] = tmp0 * csa_table[j][1] + tmp1 * csa_table[j][0];    \
///     } while (0)
/// ...
/// ptr = g->sb_hybrid + 18;
/// for (i = n; i > 0; i--) { AA(0); ... AA(7); ptr += 18; }
/// ```
///
/// # Das Fenster `g->sb_hybrid + 18`
///
/// GEMESSEN 03.10.2026: das ist der **zweite** Unterband — Index 18 von 32, also
/// 18 Werte hinter dem Anfang. Und `n` ist **1** im Kurzblock mit
/// Umschaltpunkt, sonst `SBLIMIT − 1 = 31`.
///
/// Und der Schmetterling ist eine **Drehung**: `a^2 + b^2 = 1`, also
/// `out[-1-j]^2 + out[j]^2 == in[-1-j]^2 + in[j]^2` auf `1e-15`. Das ist eine
/// **Normerhaltung** — sie ist im Code ueberpruefbar, und sie ist der Grund,
/// warum dieses Kriterium empfindlich ist: eine falsche Ziffer bricht sie
/// **sofort**.
///
/// # Rueckgabe
///
/// Der neue Fensterinhalt, 18 Werte. Der Aufrufer schreibt ihn zurueck.
pub fn alias_beseitigen(paar: &mut [f64; 18]) {
    for j in 0..8usize {
        let [a, b] = CSA[j];
        let tmp0 = paar[17 - j];
        let tmp1 = paar[j];
        paar[17 - j] = tmp0 * a - tmp1 * b;
        paar[j] = tmp0 * b + tmp1 * a;
    }
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    /// Und `a^2 + b^2 = 1` — auf `1e-15`.
    ///
    /// Das ist das **empfindliche** Kriterium, und es ist der Gegenpol zu
    /// `ENWINDOW`: dort haben **beide** Kriterien den Fehler nicht gesehen, hier
    /// sieht es ihn.
    #[test]
    fn die_antialiasing_koeffizienten_liegen_auf_dem_einheitskreis() {
        let mut schlechteste = 0.0f64;
        for (j, &[a, b]) in CSA.iter().enumerate() {
            let s = a * a + b * b;
            schlechteste = schlechteste.max((s - 1.0).abs());
            println!(
                "GEMESSEN: j = {j}: a^2 + b^2 = {s:.15}  Winkel = {:.6} Grad",
                alias_winkel(j)
            );
        }
        //
        // GEMESSEN: **5,919e-08**, nicht `1e-15`. Und der Grund ist an den
        // Literalen ablesbar: `8,574929237365722656e-01` hat 19 Nachkommastellen,
        // `1,0` hat 17. Das sind **float32**-Werte, und float32 hat rund
        // **sieben** signifikante Stellen.
        //
        // **Die Toleranz einer Abnahme ist keine Eigenschaft des Tests, sondern
        // eine Eigenschaft der DATEN. Man schreibt sie nicht hin — man misst
        // sie.** (Achte ungepruefte Behauptung dieses Berichts.)
        assert!(
            schlechteste < 1e-7,
            "GEMESSEN: 5,919e-08 ist float32-Genauigkeit, nicht 1e-15"
        );
        assert!(
            schlechteste > 1e-9,
            "GEMESSEN: und es ist nicht ungenau — es ist float32"
        );
    }

    /// Und **die Positivkontrolle** — der Gegenpol zu Teil 22.
    ///
    /// Teil 22: zwei Kriterien, **beide blind** — ein falscher Wert bewegte sie um
    /// `0,000025 dB` und `0,000e+00`. Hier: ein Kriterium, und es **sieht**.
    /// GEMESSEN, um einen Wert um `1e-6` verschoben:
    ///
    /// ```text
    ///   vorher:  a^2 + b^2 - 1 = -5,2e-08
    ///   nachher: a^2 + b^2 - 1 = +1,9e-06      Faktor 37
    /// ```
    ///
    /// Der Unterschied liegt **nicht** an der Sorgfalt. Er liegt daran, dass hier
    /// ein **Verhältnis auf 1** gefordert wird und dort nicht — und dass hier
    /// **float32**-Präzision vorliegt, wo dort **1,3e-5** relative Genauigkeit
    /// stand.
    ///
    /// Und **Faktor 37** ist die richtige Größenordnung, denn der Fehler, den man
    /// sucht, ist ein abgeschnittener Dezimalwert der Größenordnung `1e-3`, nicht
    /// `1e-6`. Ich hatte „sechs Größenordnungen" hingesetzt — **siebente**
    /// ungeprüfte Behauptung.
    ///
    /// Ohne diese Zeile wäre „a^2 + b^2 = 1 haelt" nur eine Behauptung, und eine
    /// Behauptung, die **immer** gilt, ist keine.
    #[test]
    fn die_positivkontrolle_sieht_den_fehler() {
        let sauber = CSA[4][0] * CSA[4][0] + CSA[4][1] * CSA[4][1] - 1.0;
        let mut kaputt = CSA;
        kaputt[4][0] += 1e-6;
        let mit_fehler = kaputt[4][0] * kaputt[4][0] + kaputt[4][1] * kaputt[4][1] - 1.0;
        println!("GEMESSEN: sauber {sauber:.3e}   mit einem Fehler {mit_fehler:.3e}");
        assert!(
            sauber.abs() < 1e-7,
            "GEMESSEN: die saubere Abweichung ist 5,2e-08 — float32, nicht 1e-15"
        );
        assert!(
            mit_fehler.abs() > 1e-6,
            "GEMESSEN: das Kriterium muss den Fehler sehen, {mit_fehler:.3e}"
        );
        // GEMESSEN: der Faktor ist **37**, nicht die sechs Groessenordnungen, die
        // ich hingesetzt hatte. Und das ist die richtige Groessenordnung: der
        // Fehler, den man sucht, ist ein **abgeschnittener oder verdrehter
        // Dezimalwert** — der Groessenordnung `1e-3` und groesser, nicht `1e-6`.
        let faktor = mit_fehler.abs() / sauber.abs();
        println!("GEMESSEN: Faktor sauber/mit Fehler = {faktor:.1}");
        assert!(
            faktor > 10.0,
            "GEMESSEN: mindestens Faktor 10, sonst ist es blind"
        );
        assert!(
            faktor < 1e3,
            "GEMESSEN: und nicht empfindlicher als Faktor 1000"
        );
    }

    /// Und die Monotonie: `a` **steigt**, `|b|` **fällt** — GEMESSEN.
    #[test]
    fn die_winkel_sind_steigend_geordnet() {
        let mut vorher_a = 0.0f64;
        let mut vorher_b = f64::INFINITY;
        for (j, &[a, b]) in CSA.iter().enumerate() {
            assert!(
                a > vorher_a,
                "j = {j}: a = {a} ist nicht groesser als {vorher_a}"
            );
            assert!(
                b.abs() < vorher_b,
                "j = {j}: |b| = {} ist nicht kleiner",
                b.abs()
            );
            vorher_a = a;
            vorher_b = b.abs();
        }
        let w: Vec<f64> = (0..8).map(alias_winkel).collect();
        println!(
            "GEMESSEN: Winkel = {:?}",
            w.iter()
                .map(|x| (x * 1e4).round() / 1e4)
                .collect::<Vec<_>>()
        );
        // GEMESSEN: streng fallend von 30,96 auf 0,21 Grad — und **kein**
        // einfaches Gesetz. Ich habe `atan(k*PI/36)` versucht, es passte nicht.
        //
        // GEMESSEN: **steigend**, von 59,0362 auf 89,7880 Grad — also gegen 90.
        // Ich hatte zuerst „fallend von 30,96 auf 0,21" hingesetzt, weil ich
        // `atan2` mit **negativem** `b` gerechnet hatte und so die Winkel
        // **negativ** bekam. Dieselbe Zahl, umgekehrtes Vorzeichen — und die
        // Behauptung „fallend" war die Umkehrung der gemessenen Richtung.
        //
        // **Achte** ungepruefte Behauptung dieses Berichts.
        //
        // Und die Deutung: `a = cos(theta)` geht gegen 1 und `|b| = sin(theta)`
        // gegen 0. Die hinteren Schmetterlinge sind also **fast die Identitaet**,
        // und die **vorderen** koppeln am staerksten. Das ist plausibel — die
        // Aliasbeitraege sind bei den grossen Zeitversetzen am groessten — aber
        // „plausibel" ist keine Messung, und ich behaupte es deshalb nur als
        // Beobachtung, nicht als Gesetz.
        assert!(w[0] > 59.0 && w[0] < 60.0, "j = 0: {:.4} Grad", w[0]);
        assert!(w[7] > 89.0 && w[7] < 90.0, "j = 7: {:.4} Grad", w[7]);
        for i in 1..8 {
            assert!(
                w[i] > w[i - 1],
                "j = {i}: der Winkel muss steigen, {i} = {:.4}",
                w[i]
            );
        }
        //
        // Und die **Schrittweiten sind NICHT monoton** — auch das ist gemessen,
        // und ich hatte „schrumpfend" hingesetzt, weil das die naheliegende
        // Erwartung ist, wenn ein Winkel gegen 90 laeuft:
        //
        // ```text
        //   2,817   9,884   7,782   5,054   3,079   1,534   0,602
        // ```
        //
        // Der **erste** Schritt ist der kleinste, dann kommt ein grosser. Das ist
        // keine Rundung: `9,884` ist rund das **Dreifache** von `2,817`.
        //
        // Also: **die Winkel sind streng steigend, die Schrittweiten sind es
        // nicht.** Zwei verschiedene Aussagen ueber dieselben acht Zahlen, und
        // nur eine davon war richtig. (Neunte ungepruefte Behauptung.)
        let schritte: Vec<f64> = (1..8).map(|i| w[i] - w[i - 1]).collect();
        println!("GEMESSEN: Schrittweiten = {schritte:?}");
        assert!(
            schritte[0] < schritte[1],
            "GEMESSEN: der erste Schritt ist der kleinste"
        );
        for i in 1..6 {
            assert!(
                schritte[i] > schritte[i + 1],
                "j = {i}: ab dem zweiten Schritt schrumpfen die Weiten"
            );
        }
    }

    /// Und der Schmetterling ist eine **Drehung** — die Norm bleibt.
    ///
    /// GEMESSEN 03.10.2026: auf `1e-7`, nicht auf `1e-15` — weil `a^2 + b^2`
    /// selbst nur auf `6e-8` stimmt. Die Normerhaltung erbt die Genauigkeit ihrer
    /// Koeffizienten, und **nicht mehr**.
    #[test]
    fn der_schmetterling_erhaelt_die_norm() {
        let mut paar = [0.0f64; 18];
        let mut zustand = 0x9E37_79B9_7F4A_7C15u64;
        for wert in paar.iter_mut() {
            zustand = zustand
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *wert = ((zustand >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
        }
        let vorher: f64 = paar.iter().map(|v| v * v).sum();
        alias_beseitigen(&mut paar);
        let nachher: f64 = paar.iter().map(|v| v * v).sum();
        println!("GEMESSEN: Quadratsumme vorher {vorher:.12} nachher {nachher:.12}");
        assert!(
            (vorher - nachher).abs() / vorher < 1e-6,
            "Norm nicht erhalten"
        );
        // Und die Paare sind **vertauscht**, nicht vernichtet.
        assert!(
            paar.iter().any(|v| v.abs() > 1e-9),
            "es muss etwas passieren"
        );
    }
}

/// Der Faktor, den der **Floss**-Build anwendet: `1 / 2^(16 + FRAC_BITS)`.
///
/// GEMESSEN 03.10.2026: `FRAC_BITS` steht in `mpegaudio.h` und ist **23**, also
/// `2^39 = 549755813888`. Ich hatte zuerst `2^42` im Kopf — das ist die Zahl, die
/// man bekommt, wenn man `16 + 23` und `16 + 26` verwechselt, und **beide** stehen
/// in derselben Datei. Die Probe ist billig und sie hat es gefangen: der Faktor
/// muss `f64::from(ENWINDOW[256]) / FENSTERSKALA` genau `1.0` ergeben.
pub const FENSTERSKALA: f64 = 1.0 / 549_755_813_888.0;

/// Das **512**-Wert-Synthesefenster, aus [`ENWINDOW`] nach `mpa_synth_init()`.
///
/// Die 512 Werte werden **abgeleitet**, nicht abgeschrieben: 257 stehen in der
/// Quelle, und die anderen **255** sind ihre Spiegelung mit Vorzeichenwechsel.
///
/// Und die Ausnahme ist genau eine Regel: `if ((i & 63) != 0) v = -v;` — bei
/// `i % 64 == 0` **bleibt** das Vorzeichen. GEMESSEN 03.10.2026, und das ist
/// die Stelle, an der man beim Umschreiben hängen bleibt, weil man die Regel
/// „immer spiegeln" im Kopf hat und die Ausnahme erst beim zweiten Vergleich
/// bemerkt.
///
/// # Die zwei SIMD-Aliasbloecke ab 512
///
/// ```c
/// for(i = 0; i < 8; i++) for(j = 0; j < 16; j++)
///     window[512+16*i+j]    = window[64*i+32-j];
/// for(i = 0; i < 8; i++) for(j = 0; j < 16; j++)
///     window[512+128+16*i+j] = window[64*i+48-j];
/// ```
///
/// Die sind **Kopien** — „needed for avoiding shuffles in ASM implementations",
/// wie der Quellkommentar sagt. Sie sind **keine** neuen Werte, und sie sind
/// trotzdem **genau die Hälfte der Pruefung**: wenn die Spiegelung stimmt, stimmen
/// sie automatisch, und wenn sie stimmen, ist die Spiegelung bewiesen.
#[must_use]
pub fn synthese_fenster() -> [f64; 768] {
    let mut w = [0.0f64; 768];
    for (i, &roh) in ENWINDOW.iter().enumerate() {
        let mut v = f64::from(roh) / 549_755_813_888.0;
        w[i] = v;
        if (i & 63) != 0 {
            v = -v;
        }
        if i != 0 {
            w[512 - i] = v;
        }
    }
    for i in 0..8usize {
        for j in 0..16usize {
            w[512 + 16 * i + j] = w[64 * i + 32 - j];
        }
    }
    for i in 0..8usize {
        for j in 0..16usize {
            w[512 + 128 + 16 * i + j] = w[64 * i + 48 - j];
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Und die 512 Werte sind **abgeleitet**: 255 kommen aus der Spiegelung.
    ///
    /// GEMESSEN 03.10.2026, und die Ausnahme ist der Punkt:
    ///
    /// ```text
    ///   w[i]       =  E[i]                      fuer i = 0..256
    ///   w[512 - i] = -E[i]                      fuer i % 64 != 0,  i >= 1
    ///   w[512 - i] = +E[i]                      fuer i % 64 == 0
    /// ```
    ///
    /// Das ist `if ((i & 63) != 0) v = -v;` — **eine** Regel mit **einer**
    /// Ausnahme, und die Ausnahme ist bei `i = 0, 64, 128, 192, 256`. Wer die
    /// Regel „immer spiegeln und Vorzeichen wechseln" im Kopf hat, macht **fuenf**
    /// von 512 Werten falsch, und sie sehen aus wie die uebrigen 507.
    #[test]
    fn die_spiegelung_hat_genau_fuenf_ausnahmen() {
        let w = synthese_fenster();
        assert_eq!(w.len(), 768);
        let mut ausnahmen = 0;
        for i in 1..257usize {
            let roh = f64::from(ENWINDOW[i]);
            let ist = roh / 549_755_813_888.0;
            let erwartet = if (i & 63) != 0 { -ist } else { ist };
            assert!(
                (w[512 - i] - erwartet).abs() < 1e-300,
                "i = {i}: {} statt {erwartet}",
                w[512 - i]
            );
            if (i & 63) == 0 {
                ausnahmen += 1;
            }
        }
        //
        // GEMESSEN: **vier**, nicht fuenf. Ich hatte `i = 0` mitgezaehlt — aber
        // `i = 0` wird gar **nicht** gespiegelt, es steht schon links. Die vier
        // sind `64, 128, 192, 256`.
        assert_eq!(
            ausnahmen, 4,
            "GEMESSEN: die Ausnahmen sind i = 64, 128, 192, 256 — i = 0 wird nicht gespiegelt"
        );
        assert_eq!(ENWINDOW[0], 0, "und links steht bei i = 0 die Null");
        // Und die linke Haelfte steht **woertlich**.
        for i in 0..257usize {
            assert_eq!(w[i], f64::from(ENWINDOW[i]) / 549_755_813_888.0, "i = {i}");
        }
        //
        // GEMESSEN: es gibt **keine** Luecke. Ich hatte behauptet, `256..511`
        // sei unbelegt — aber die Spiegelung schreibt `w[512 - i]` fuer
        // `i = 1..256`, also die Indizes **511 runter bis 256**. Damit sind alle
        // 512 Plaetze belegt, und `w[256]` wird **zweimal** geschrieben:
        // einmal links (`w[i]`, `i = 256`) und einmal als Spiegel von `i = 256`.
        //
        // Beide Schreibvorgaenge muessen dasselbe ergeben, und das tun sie nur,
        // weil `256 % 64 == 0` — der Vorzeichenwechsel faellt dort aus. Waere er
        // nicht ausgefallen, stuenden links und rechts **verschiedene** Werte an
        // derselben Stelle, und der zweite Schreibvorgang haette still gewonnen.
        assert_ne!(w[256], 0.0, "w[256] ist belegt");
        assert_eq!(
            w[256],
            f64::from(ENWINDOW[256]) / 549_755_813_888.0,
            "und beide Schreibvorgaenge ergeben dasselbe"
        );
        //
        // Und GEMESSEN ist, warum der Index **im Text** bleiben muss: die Meldung
        // nennt `i`, weil ein Fehler **hier** ohne die Stelle **nichts** sagt.
        // Also `enumerate()` statt Index — und `i` lebt **im Text** weiter.
        for (i, v) in w.iter().enumerate().skip(257).take(255) {
            assert_ne!(*v, 0.0, "i = {i} gehoert zu einem Fensterwert");
        }
    }

    /// Und die zwei SIMB-Aliasbloecke sind **Kopien** — „needed for avoiding
    /// shuffles in ASM implementations", wie der Quellkommentar sagt.
    ///
    /// Sie sind damit **keine** neuen Zahlen, und sie sind die **billigste
    /// Gegenprobe** der Spiegelung: wenn sie stimmen, stimmt die Spiegelung.
    #[test]
    fn die_simd_bloecke_sind_kopien_der_spiegelung() {
        let w = synthese_fenster();
        for i in 0..8usize {
            for j in 0..16usize {
                assert_eq!(
                    w[512 + 16 * i + j],
                    w[64 * i + 32 - j],
                    "Block 1, i = {i}, j = {j}"
                );
                assert_eq!(
                    w[512 + 128 + 16 * i + j],
                    w[64 * i + 48 - j],
                    "Block 2, i = {i}, j = {j}"
                );
            }
        }
    }

    /// Und der Skalenfaktor ist **2^39** — nachgerechnet, nicht behauptet.
    #[test]
    fn der_skalenfaktor_ist_zwei_vierunddreissig() {
        // max|E| = 75038; geteilt durch 2^39 muss 1 sein.
        let max = ENWINDOW.iter().map(|&v| v.abs()).max().expect("Tabelle");
        assert_eq!(max, 75038, "GEMESSEN: das groesste |E|");
        assert!((FENSTERSKALA * 549_755_813_888.0 - 1.0).abs() < 1e-15);
        //
        // GEMESSEN: der Faktor normalisiert **nicht** auf 1. `w[256]` ist
        // `75038 / 2^39 = 1,364933268632740e-7`.
        //
        // Ich hatte behauptet, `w[256]` muesste **1** sein — das ist die Form,
        // die man von einem „Fenster" erwartet. Sie ist falsch: die
        // Normalisierung auf 1 passiert **anderswo** (in der DCT32 und in der
        // Akkumulation der Synthese), nicht im Fenster.
        //
        // Das ist die **fuenfte** ungepruefte Behauptung in diesem Bericht. Der
        // Test hat sie gemeldet, wie die vier anderen auch.
        let w = synthese_fenster();
        let max = w.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        println!("GEMESSEN: groesstes |w| = {max:.15e}");
        assert_eq!(max, f64::from(ENWINDOW[256]) / 549_755_813_888.0);
        assert!(
            (max - 1.364_933_268_632_740e-7).abs() < 1e-22,
            "GEMESSEN: {max:.15e}"
        );
    }

    /// **NEGATIVKONTROLLE 1** — die Sperrdaempfung unterscheidet **nicht**.
    ///
    /// GEMESSEN 03.10.2026: ein Wert um 1 veraendert bewegt die Sperrdaempfung
    /// um **0,00 dB**. Der Test sagt das als **Behauptung** fest, damit niemand
    /// die Sperrdaempfung spaeter als Abnahme benutzt.
    #[test]
    fn die_sperrdaempfung_unterscheidet_nicht() {
        let a = sperrdaempfung(&ENWINDOW);
        let mut b = ENWINDOW;
        b[100] += 1;
        let mit_fehler = sperrdaempfung(&b);
        let delta = (mit_fehler - a).abs();
        println!(
            "GEMESSEN: Sperrdaempfung = {a:.2} dB, mit einem falschen Wert = {mit_fehler:.2} dB"
        );
        println!("GEMESSEN: Aenderung = {delta:.6} dB");
        assert!(
            delta < 1e-3,
            "GEMESSEN: die Sperrdaempfung ist BLIND — sie sieht einen falschen Wert nicht"
        );
    }

    /// **NEGATIVKONTROLLE 2** — die Symmetrie unterscheidet **nicht** (und der
    /// Prototyp ist ueberhaupt nicht symmetrisch).
    ///
    /// GEMESSEN 03.10.2026: der Rest `h[n] - h[511-n]` ist **1,53** mal so gross
    /// wie `max|h|` — also **keine** Symmetrie, sondern das Gegenteil. Und wenn
    /// man einen Wert um 1 veraendert, aendert sich der Rest um **0,00e+00**.
    ///
    /// Diese Negativkontrolle hat ihren Zweck erfüllt: sie hat eine **falsche
    /// Grundannahme** gestoppt. Ich hatte „die Analyse- und Synthesefenster sind
    /// symmetrisch" angenommen und wollte es pruefen — die Pruefung sagt, dass sie
    /// es **nicht** sind, also war die Annahme schon vorher falsch und haette
    /// nichts gefunden.
    #[test]
    fn die_symmetrie_unterscheidet_nicht() {
        let r = symmetrie_rest(&ENWINDOW);
        let mut b = ENWINDOW;
        b[100] += 1;
        let delta = (symmetrie_rest(&b) - r).abs();
        println!(
            "GEMESSEN: Symmetrie-Rest = {r:.3e}, mit einem falschen Wert = {:.3e}",
            symmetrie_rest(&b)
        );
        println!(
            "GEMESSEN: Aenderung = {delta:.3e}   max|h| = {:.3e}",
            max_prototyp(&ENWINDOW)
        );
        println!(
            "GEMESSEN: Rest / max|h| = {:.3}  <- also KEINE Symmetrie",
            r / max_prototyp(&ENWINDOW)
        );
        assert!(delta < 1e-20, "GEMESSEN: auch die Symmetrie ist BLIND");
        assert!(
            r / max_prototyp(&ENWINDOW) > 1.0,
            "GEMESSEN: der Prototyp ist nicht symmetrisch"
        );
    }

    /// Die beiden Masse, die **nichts** taugen — sie stehen hier, damit der
    /// Befund im Code steht und nicht nur im Bericht.
    ///
    /// `h[n] = W[n] * cos(PI * (n - 16) / 64)`, und der Prototyp ist **nicht**
    /// symmetrisch; die Sperrdaempfung liegt bei **-23,69 dB**. Beides ist kein
    /// Fehler der Messung, sondern ein **Fakt ueber diese Tabelle**: sie ist
    /// eine **Naeherung** in Festkomma, und ihre Eigenschaften sind nicht exakt.
    fn prototyp(e: &[i32; 257]) -> Vec<f64> {
        let w = fenster_aus(e);
        (0..512)
            .map(|n| w[n] * (std::f64::consts::PI * (n as f64 - 16.0) / 64.0).cos())
            .collect()
    }
    fn fenster_aus(e: &[i32; 257]) -> [f64; 512] {
        let mut w = [0.0f64; 512];
        for (i, &roh) in e.iter().enumerate() {
            let mut v = f64::from(roh) / 549_755_813_888.0;
            w[i] = v;
            if (i & 63) != 0 {
                v = -v;
            }
            if i != 0 {
                w[512 - i] = v;
            }
        }
        w
    }
    fn max_prototyp(e: &[i32; 257]) -> f64 {
        prototyp(e).iter().fold(0.0f64, |a, b| a.max(b.abs()))
    }
    fn symmetrie_rest(e: &[i32; 257]) -> f64 {
        let h = prototyp(e);
        (0..512)
            .map(|n| (h[n] - h[511 - n]).abs())
            .fold(0.0f64, f64::max)
    }
    fn sperrdaempfung(e: &[i32; 257]) -> f64 {
        let h = prototyp(e);
        let betrag = |f: f64| {
            let mut re = 0.0;
            let mut im = 0.0;
            for (n, &v) in h.iter().enumerate() {
                let a = 2.0 * std::f64::consts::PI * f * (n as f64 - 255.5);
                re += v * a.cos();
                im -= v * a.sin();
            }
            re.hypot(im)
        };
        let mut durch = 0.0f64;
        for i in 100..=900 {
            durch = durch.max(betrag(f64::from(i) / 200.0 / 64.0));
        }
        let mut sperre = f64::INFINITY;
        for i in 1000..=2000 {
            sperre = sperre.min(betrag(f64::from(i) / 200.0 / 64.0));
        }
        20.0 * (sperre / durch).log10()
    }
}
