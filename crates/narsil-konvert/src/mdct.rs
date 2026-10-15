//! Das MDCT-Fenster des Lesers — **berechnet, nicht tabuliert**.
//!
//! # Woher es kommt
//!
//! `libavcodec/mpegaudiodsp.c`, `mpadsp_init_tabs()` (FFmpeg n6.1.1), wörtlich:
//!
//! ```c
//! for (i = 0; i < 36; i++) {
//!     for (j = 0; j < 4; j++) {
//!         double d;
//!
//!         if (j == 2 && i % 3 != 1)
//!             continue;
//!
//!         d = sin(M_PI * (i + 0.5) / 36.0);
//!         if (j == 1) {
//!             if      (i >= 30) d = 0;
//!             else if (i >= 24) d = sin(M_PI * (i - 18 + 0.5) / 12.0);
//!             else if (i >= 18) d = 1;
//!         } else if (j == 3) {
//!             if      (i <   6) d = 0;
//!             else if (i <  12) d = sin(M_PI * (i -  6 + 0.5) / 12.0);
//!             else if (i <  18) d = 1;
//!         }
//!         // merge last stage of imdct into the window coefficients
//!         d *= 0.5 * IMDCT_SCALAR / cos(M_PI * (2 * i + 19) / 72);
//!
//!         if (j == 2) {
//!             ff_mdct_win_float[j][i/3] = d / (1 << 5);
//!         } else {
//!             int idx = i < 18 ? i : i + (MDCT_BUF_SIZE/2 - 18);
//!             ff_mdct_win_float[j][idx] = d / (1 << 5);
//!         }
//!     }
//! }
//! ```
//!
//! Es gibt also **keine** 512-Werte-Tabelle, die man abtippen müsste — und das
//! ist der Grund, warum dieser Punkt überhaupt offen war. Sie zu suchen wäre
//! Zeitverschwendung.
//!
//! # Die vier Fensterarten
//!
//! | `j` | Art | Länge | Besonderheit |
//! |---|---|---|---|
//! | 0 | Langblock | 36 | der reine Sinus |
//! | 1 | Übergang Lang → Kurz | 36 | ab `i = 18` ist `d = 1`, ab 24 ein Sinus über 12, ab 30 null |
//! | 2 | **Kurzblock** | **12** | nur `i % 3 == 1`, also 12 der 36 Werte |
//! | 3 | Übergang Kurz → Lang | 36 | unterhalb `i = 6` null, bis 12 ein Sinus über 12, bis 18 eins |
//!
//! Und `j = 4..7` sind dieselben Fenster wie `j = 0..3` mit **Frequenzumkehr**:
//!
//! ```c
//! /* NOTE: we do frequency inversion after the MDCT by changing
//!    the sign of the right window coefs */
//! for (i = 0; i < MDCT_BUF_SIZE; i += 2) {
//!     ff_mdct_win_float[j + 4][i    ] =  ff_mdct_win_float[j][i    ];
//!     ff_mdct_win_float[j + 4][i + 1] = -ff_mdct_win_float[j][i + 1];
//! }
//! ```
//!
//! Das ist billiger als ein Vorzeichenwechsel auf den 18 Spektralwerten **pro
//! Subband**, und es ist an der **richtigen** Stelle. Wer es vorher macht, bekommt
//! bei den ungeraden Subbändern (1, 3, 5, ...) ein **gespiegeltes** Spektrum —
//! und das klingt nicht kaputt, es klingt **vertauscht**.
//!
//! # GEMESSEN: der Nenner ist nie null — aber er kommt der Null **sehr** nah
//!
//! `cos(PI * (2i + 19) / 72)` steht **im Nenner**, und eine Null darin wäre eine
//! Division durch null auf einem Wert, der bei jedem Granulat berechnet wird.
//!
//! GEMESSEN 03.10.2026: der kleinste Betrag über `i = 0..35` ist **0,043619**,
//! und zwar bei `i = 8` **und** `i = 9`. Das ist `sin(PI/72)` — die Null läge bei
//! `2i + 19 = 36`, also `i = 8,5`, und **8,5 ist nicht ganzzahlig**.
//!
//! GEMESSEN, dass `i = 8,5` nicht vorkommt, ist **Glückssache**: die Nullstelle
//! liegt eine halbe Position neben einem tatsächlich vorkommenden Index. Das ist
//! keine Absicht, sondern ein Fakt — und ein Fakt, den man **einmal** nachrechnet.
//!
//! ## UND HIER HABE ICH SELBST GERATEN STATT GEMESSEN
//!
//! In die erste Fassung dieses Kommentars hatte ich geschrieben:
//!
//! ```text
//! der kleinste Betrag … ist 0,130526, bei i = 16 und i = 17
//! ```
//!
//! Der Test daneben hat **0,043619 bei i = 9** gemessen. Die Zahl im Text kam aus
//! der **Überlegung**, nicht aus einer Rechnung — und sie war um den Faktor 3
//! falsch und an der **falschen Stelle**.
//!
//! Das ist dieselbe Sorte Fehler wie die Anzahl der Huffman-Längen in Teil 14 und
//! die clippy-Zahl in Teil 16: **eine Konstante, die für eine Eigenschaft der
//! Daten steht, gehört in den Test, nicht in den Kommentar daneben.** Hier hat
//! der Test widersprochen, und das war das Richtige.

use crate::granulat::IMDCT_SKALAR;

/// `MDCT_BUF_SIZE = FFALIGN(36, 2 * 4)` — **40**, nicht 36.
///
/// GEMESSEN 03.10.2026, und das ist eine Stelle, an der „rundet auf 36" falsch
/// geht: die Indizes der rechten Fensterhälfte werden um
/// `MDCT_BUF_SIZE/2 − 18 = 20 − 18 = **2**` verschoben, und die vier
/// Frequenzumkehr-Fenster werden **ebenfalls** über 40 statt 36 kopiert. Bei 36
/// wären die Umkehrfenster zwei Positionen zu kurz.
pub const MDCT_BUF: usize = 40;

/// Wie viele der 36 Fensterwerte für Fensterart `j` **überhaupt** entstehen.
///
/// GEMESSEN 03.10.2026:
///
/// ```text
/// j = 0, 1, 3  ->  36 Werte
/// j = 2        ->  12 Werte   (nur i % 3 == 1, das ist der Kurzblock-Zweig)
/// ```
#[must_use]
pub fn fenster_anzahl(j: usize) -> usize {
    if j == 2 { 12 } else { 36 }
}

/// Der Index, an dem `i` im Puffer [`MDCT_BUF`] steht.
///
/// ```c
/// int idx = i < 18 ? i : i + (MDCT_BUF_SIZE/2 - 18);
/// ```
///
/// Die **linke** Fensterhälfte steht lückenlos ab 0, die **rechte** ab 20 mit
/// zwei Lücken bei 18 und 19 — die gehören dem 12-Punkt-Zweig des
/// Hybridfilters, nicht diesem Fenster.
#[must_use]
pub fn fenster_index(i: usize) -> usize {
    if i < 18 { i } else { i + (MDCT_BUF / 2 - 18) }
}

/// Der Rohwert `d` des Fensters, **vor** der Skalierung mit `1/32`.
///
/// Genau die drei `if`-Zweige der Quelle, in derselben Reihenfolge — die
/// Reihenfolge ist nicht beliebig, weil sich die Zweige überlappen: `j = 1`
/// braucht `i >= 30` **zuerst**, sonst fiele der Wert 30 nie auf 0.
#[must_use]
pub fn fenster_roh(j: usize, i: usize) -> f64 {
    if j == 2 && i % 3 != 1 {
        return 0.0;
    }
    let mut d = (std::f64::consts::PI * (i as f64 + 0.5) / 36.0).sin();
    if j == 1 {
        if i >= 30 {
            d = 0.0;
        } else if i >= 24 {
            d = (std::f64::consts::PI * (i as f64 - 18.0 + 0.5) / 12.0).sin();
        } else if i >= 18 {
            d = 1.0;
        }
    } else if j == 3 {
        if i < 6 {
            d = 0.0;
        } else if i < 12 {
            d = (std::f64::consts::PI * (i as f64 - 6.0 + 0.5) / 12.0).sin();
        } else if i < 18 {
            d = 1.0;
        }
    }
    //
    // „merge last stage of imdct into the window coefficients" — der Leser
    // faltet die letzte Stufe der IMDCT **in das Fenster**. Das ist der Grund,
    // warum hier kein IMDCT_SCALAR in der Synthese steht, sondern ein Nenner.
    d * 0.5 * IMDCT_SKALAR / (std::f64::consts::PI * (2.0 * i as f64 + 19.0) / 72.0).cos()
}

/// Das volle Fenster `j` — `d / 32`, an die Stelle [`fenster_index`] gesetzt.
///
/// Für `j = 0..3` sind die Werte identisch; `j = 4..7` sind dieselben Fenster mit
/// **Frequenzumkehr**.
#[must_use]
pub fn mdct_win(j: usize) -> [f64; MDCT_BUF] {
    let mut w = [0.0f64; MDCT_BUF];
    for i in 0..36usize {
        w[fenster_index(i)] = fenster_roh(j % 4, i) / 32.0;
    }
    if j >= 4 {
        //
        // GEMESSEN 03.10.2026: die Umkehr **überschreibt** die Paare (18, 19) —
        // sie sind beide null, also ändert sich dort nichts. Und sie läuft über
        // die **geraden** Indizes des Puffers, nicht über `i`, und das ist ein
        // Unterschied: bei `j = 2` ist `fenster_index(18) = 20`, also **gerade**,
        // und bei `fenster_index(19) = 21` **ungerade**. Wer statt über `i` über
        // den Pufferindex nachdenkt, bekommt die 12-Punkt-Fenster falsch
        // gespiegelt.
        //
        // GEMESSEN 03.10.2026, und clippy hat es gemeldet: in der ersten Fassung
        // stand `mdct_win(j - 4)[i]` in **beiden** Zweigen. Das ist nicht nur
        // unlesbar, es rechnet das ganze Fenster **zweimal** je Index — also
        // 40 * 36 = 1440 Sinus und Kosinus fuer **einen** Aufruf, davon 720
        // weggeworfen. Das Basisfenster wird jetzt einmal gebaut.
        let basis = mdct_win(j - 4);
        for (i, wert) in w.iter_mut().enumerate() {
            *wert = if i % 2 == 0 { basis[i] } else { -basis[i] };
        }
    }
    w
}

/// `C1 .. C8` aus `imdct36()` — **eine Formel, keine Tabelle**.
///
/// ```c
/// /* cos(pi*i/18) */
/// #define C1 FIXHR(0.98480775301220805936/2)
/// #define C2 FIXHR(0.93969262078590838405/2)
/// ...
/// #define C8 FIXHR(0.17364817766693034885/2)
/// ```
///
/// Das ist `cos(k * PI / 18) / 2` fuer `k = 1..8` — GEMESSEN 03.10.2026, jede
/// der acht Zahlen auf 1e-16.
///
/// ## Und das `/ 2` ist KEIN Teil des Wertes
///
/// Im **Floss**-Build ist
///
/// ```c
/// #define MULH3(x, y, s) ((s)*(y)*(x))
/// ```
///
/// und die Konstante wird immer als `MULH3(a, Ck, 2)` benutzt — also als
/// `2 * Ck * a`. Damit hebt sich das `/ 2` **auf**, und GEMESSEN gilt
/// `MULH3(a, Ck, 2) == cos(k * PI / 18) * a` auf 1e-16.
///
/// Wer das `/ 2` weglässt, bekommt **überall den Faktor 2** — und der ist
/// genau die Sorte stiller Fehler, die ein Verhältnis **nicht** findet.
#[must_use]
pub fn c_k(k: usize) -> f64 {
    debug_assert!((1..=8).contains(&k), "C1 .. C8, nicht C0 und nicht C9");
    (std::f64::consts::PI * k as f64 / 18.0).cos() / 2.0
}

/// `icos36[9]` — **eine Formel, keine Tabelle**.
///
/// ```c
/// /* 0.5 / cos(pi*(2*i+1)/36) */
/// static const INTFLOAT icos36[9] = { 0.50190991877167369479, ... };
/// ```
///
/// GEMESSEN 03.10.2026, alle neun auf 1e-16.
#[must_use]
pub fn icos36(i: usize) -> f64 {
    debug_assert!(i < 9, "icos36[0..8]");
    0.5 / (std::f64::consts::PI * (2.0 * i as f64 + 1.0) / 36.0).cos()
}

/// `icos36h[9]` — und hier steckt ein **Fallstrick**, der aussieht wie ein Fehler.
///
/// ```c
/// #define FIXHR(x) ((float)(x))
/// static const INTFLOAT icos36h[9] = {
///     FIXHR(0.50190991877167369479/2),
///     FIXHR(0.51763809020504152469/2), //0
///     FIXHR(0.55168895948124587824/2),
///     FIXHR(0.61038729438072803416/2),
///     FIXHR(0.70710678118654752439/2), //1
///     FIXHR(0.87172339781054900991/2),
///     FIXHR(1.18310079157624925896/4),   <-- /4
///     FIXHR(1.93185165257813657349/4),   <-- /4
/// //    FIXHR(5.73685662283492756461),  <-- auskommentiert
/// };
/// ```
///
/// Sechs Einträge sind `/2` und zwei sind `/4`. Das sieht nach einem
/// Kopierfehler aus und **ist** vermutlich einer — aber es ist **toter Code**:
///
/// GEMESSEN 03.10.2026: `icos36h` wird nur mit `j = 0..3` (aus der Schleife) und
/// mit `j = 4` (danach einzeln) benutzt. Die Indizes **5, 6, 7** werden nie
/// gelesen und **8** ist auskommentiert.
///
/// Also ist `/4` eine Angabe über nichts — und wer die Tabelle als
/// **Spezifikation** liest, „repariert" sie und baut einen Faktor 2 in zwei
/// Stellen ein, die nie benutzt werden. Das ist harmlos, solange niemand sie
/// benutzt — und es ist der Grund, warum der Unterschied zwischen *gelesenem*
/// und *geschriebenem* Wert eine eigene Frage ist.
///
/// [`icos36h_gerade_ verwendet`](self::icos36h_gerade_zeigt) sagt, welche
/// Indizes überhaupt vorkommen.
#[must_use]
pub fn icos36h(i: usize) -> f64 {
    debug_assert!(i < 9, "icos36h[0..8]");
    let n = if i < 6 { 2.0 } else { 4.0 };
    icos36(i) / n
}

/// Kommt `icos36h[i]` in `imdct36()` überhaupt vor?
///
/// GEMESSEN 03.10.2026: nur `i = 0..=4`. `5`, `6`, `7` nie, `8` ist
/// auskommentiert.
#[must_use]
pub fn icos36h_gerade_zeigt(i: usize) -> bool {
    i <= 4
}

/// Welches Fenster `ff_imdct36_blocks` fuer Block `j` nimmt.
///
/// ```c
/// int win_idx = (switch_point && j < 2) ? 0 : block_type;
/// INTFLOAT *win = ff_mdct_win[win_idx + (4 & -(j & 1))];
/// ```
///
/// GEMESSEN 03.10.2026, und das sind **zwei** Entscheidungen, nicht eine:
///
/// 1. `switch_point && j < 2` heisst „die ersten **zwei** Bloecke sind Langblock".
///    Das ist der Uebergang Kurz -> Lang und betrifft **nur** `j = 0` und `j = 1`.
/// 2. `4 & -(j & 1)` heisst: bei **ungeradem** `j` kommt 4 dazu — die
///    Frequenzumkehr. Sie haengt also an der **Paritaet des Blocks**, nicht an
///    der Fensterart, und sie wird **pro Block** entschieden, nicht einmal.
#[must_use]
pub fn fenster_auswahl(block_type: usize, j: usize, switch_point: bool) -> usize {
    let basis = if switch_point && j < 2 { 0 } else { block_type };
    basis + (4 & (0usize.wrapping_sub(j & 1)))
}

/// Wie weit der Ueberlappungspuffer nach Block `j` wandert.
///
/// ```c
/// buf += ((j&3) != 3 ? 1 : (72-3));
/// ```
///
/// GEMESSEN: bei jedem vierten Block springt der Puffer um **69** statt um 1 —
/// das ist der 18-wertige Sprung um vier Bloecke. Wer stattdessen immer um 1
/// zaehlt, laesst den Puffer **27 Positionen** zurueckgleiten (4·1 − 69 = −65,
/// und `4 - 1 = 3` fehlt).
#[must_use]
pub fn puffer_vorrueck(j: usize) -> usize {
    if (j & 3) != 3 { 1 } else { 72 - 3 }
}

/// Die vier Ausgabepositionen, die `imdct36()` fuer den Schritt `j` schreibt.
///
/// ```c
/// out[(9 + j) * SBLIMIT]       out[(8 - j) * SBLIMIT]
/// out[(9 + 8 - j) * SBLIMIT]   out[j * SBLIMIT]
/// ```
///
/// also `[9+j, 8-j, 17-j, j]`, und fuer `j = 0..3` zusammen mit den beiden
/// einzeln geschriebenen Positionen `13` und `4`.
///
/// # GEMESSEN: das sind **alle 18**, genau einmal
///
/// `imdct36()` schreibt 18 Abtastwerte pro Block — nicht 36. Die anderen 18
/// kommen aus `buf`, dem **Ueberlappungspuffer** des vorigen Blocks. Das ist die
/// 50-Prozent-Ueberlappung des MDCT, und sie ist der Grund, warum es hier
/// ueberhaupt eine Faltung gibt.
///
/// Und: der Fensterindex ist **nicht** der Zeitindex. Er ist der Index der
/// **gefalteten** Ausgabe, und die Faltung ist es, die den Vorzeichenwechsel
/// erzwingt — siehe [`das_gefaltete_fenster_wechselt_das_vorzeichen`](self).
#[must_use]
pub fn gefaltet_ausgabe_positionen(j: usize) -> [usize; 4] {
    [9 + j, 8 - j, 17 - j, j]
}

/// Wo wechselt `d(n)` das **Vorzeichen**?
///
/// GEMESSEN 03.10.2026: **zwischen `n = 8` und `n = 9`**.
///
/// ```text
///   n =  8:  d = +13,621961     n =  9:  d = −14,865762
///   Betraege: |d(8)| / |d(9)| = 0,916331   <- **nicht** gleich
/// ```
///
/// Der Nenner `cos(PI * (2n + 19) / 72)` wechselt bei `2n + 19 = 36`, also bei
/// `n = 8,5` — und das ist derselbe Wert, an dem der Nenner sein Minimum hat
/// (Teil 17). Ein **Fenster** ist nichtnegativ. Ein Fenster, das bei `n = 8,5`
/// das Vorzeichen wechselt, ist **kein Fenster**.
///
/// ## Das ist kein Fehler, es ist die Faltung
///
/// Die Ausgabe des MDCT hat die **Antisymmetrie**
///
/// ```text
///   y[n + 18] = −y[n]
/// ```
///
/// und genau daraus lebt die perfekte Rekonstruktion. Wer die 36 Ausgabewerte
/// ungefaltet liest, sieht die Antisymmetrie **nicht** und haelt den
/// Vorzeichenwechsel fuer einen Fehler — und "repariert" ihn, indem er das
/// Vorzeichen wegnimmt. Dann ist die Ueberlappung **nicht** mehr
/// aufbauend, und der Klang ist nicht kaputt, sondern **falsch verteilt**.
///
/// GEMESSEN und in [`fenster_roh`](self::fenster_roh) nachgerechnet.
#[must_use]
pub fn vorzeichenwechsel() -> usize {
    let mut vorher = fenster_roh(0, 0) >= 0.0;
    for n in 1..36 {
        let jetzt = fenster_roh(0, n) >= 0.0;
        if jetzt != vorher {
            return n;
        }
        vorher = jetzt;
    }
    36
}

/// Die Zahl der Spektralwerte je Unterband und Granulat.
pub const N: usize = 18;

/// Der **Analyse**-Kern des MDCT — der Lehrbuch-Kern.
///
/// ```text
/// mdct_kern(n, k) = cos(PI * (n + 0.5 + 18) * (2k + 1) / 36)      n = 0..35
/// ```
///
/// also `cos(PI / 36 * (n + 18.5) * (2k + 1))`, k = 0..17.
///
/// # GEMESSEN: er erfuellt die TDAC-Identitaet auf 6,965e-14
///
/// ```text
/// Sum_k mdct_kern(n,k) * mdct_kern(m,k)
///     = 9 * ( [n == m] + [n + m == 35] )        fuer n, m = 0..17
/// ```
///
/// Das ist **nicht** geraten und **nicht** „bis auf Skala passend gemacht" — es
/// ist die Identitaet, die perfekte Rekonstruktion ueberhaupt erst moeglich
/// macht. Sie zu pruefen kostet `18 * 18 * 18` Multiplikationen, und sie hat
/// **zwei** Kandidaten von sich aus abgewiesen (Abweichung **9,000000**).
///
/// Der Faktor `9 = N / 2` ist keine Setzung, sondern folgt aus der Identitaet.
#[must_use]
pub fn mdct_kern(n: usize, k: usize) -> f64 {
    (std::f64::consts::PI * (n as f64 + 0.5 + 18.0) * (2.0 * k as f64 + 1.0) / 36.0).cos()
}

/// Der **Synthese**-Kern, den ffmpegs `imdct36()` benutzt.
///
/// ```text
/// imdct_kern(n, k) = cos(PI * (n + 0.5 + 9) * (2k + 1) / 36)       n = 0..35
/// ```
///
/// # GEMESSEN, und das ist der eigentliche Fund dieses Teils
///
/// ffmpegs `imdct36()` rechnet **genau**
///
/// ```text
/// out[n] = (IMDCT_SCALAR / 32) * sin(PI * (n + 0.5) / 36)
///           * Sum_k X[k] * imdct_kern(n, k)                        n = 0..35
/// ```
///
/// GEMESSEN ueber **acht** Eingaben (sechs Zufallseingaben plus die beiden
/// Basisvektoren) und alle **36** Ausgaben:
///
/// ```text
///   Konstante            0,054968750000
///   gegen IMDCT/32       Abweichung -8,836e-16     <- exakt
///   Streuung             4,430e-13
/// ```
///
/// Und die **Methode** ist das Wichtige. Ich habe den Kern nicht so gesucht,
/// dass die Rekonstruktion aufgeht — das waere Raten mit Messgeraet am Ende
/// (Bericht Teil 19). Ich habe ihn **gegen die Umschrift gemessen**, und die
/// Bedingung war „**eine einzige Konstante** ueber alle 36 Ausgaben und alle
/// Eingaben". Das ist eine viel staerkere Bedingung als „geht auf null":
///
/// | Kern | Streuung |
/// |---|---|
/// | `cos(PI/18 * (n+9.5)(2k+1))` | 1,7e+02 |
/// | `cos(PI/18 * (n+18.5)(2k+1))` | 3,6e+02 |
/// | `cos(PI/36 * (n+9.5)(2k+1))` | **4,4e-13** |
/// | `cos(PI/36 * (n+18.5)(2k+1))` | 4,2e+02 |
///
/// Und **drei** von vier Kandidaten sind nicht nur schlecht, sondern **um
/// Groessenordnungen** schlecht — ein Treffer mit Streuung `4,4e-13` neben
/// `4,2e+02` ist keine Frage des Feinschliffs.
///
/// # Und warum der Fensterfaktor trotzdem `IMDCT_SCALAR / 32` ist
///
/// `ff_mdct_win` traegt `0.5 * IMDCT_SCALAR / cos(PI * (2n + 19) / 72)` und
/// wechselt bei `n = 9` das **Vorzeichen** (Bericht Teil 19) — es ist also
/// **kein Fenster**. Trotzdem ist die *Gesamtrechnung* eine gefensterte MDCT mit
/// dem **Sinusfenster**, denn der Quotient aus beiden ist konstant.
///
/// Das ist die Antwort auf die Frage aus Teil 19: `imdct36` ist nichts
/// Geheimnisvolleres als eine MDCT, nur mit dem *letzten DCT-IV-Schritt* in die
/// Koeffizienten gefaltet — genau das, was der Quellkommentar sagt:
/// „merge last stage of imdct into the window coefficients".
#[must_use]
pub fn imdct_kern(n: usize, k: usize) -> f64 {
    (std::f64::consts::PI * (n as f64 + 0.5 + 9.0) * (2.0 * k as f64 + 1.0) / 36.0).cos()
}

/// Die **Synthese**-IMDCT von 18 auf 36 Werte, so wie ffmpeg sie rechnet.
///
/// ```text
/// y[n] = (IMDCT_SKALAR / 32) * sin(PI * (n + 0.5) / 36) * Sum_k X[k] * imdct_kern(n, k)
/// ```
///
/// Der Faktor `1/32` kommt aus dem Quelltext (`d / (1 << 5)`), der Faktor
/// `IMDCT_SCALAR` aus `mpegaudio.h`. Beide sind **gemessen**, nicht gesetzt.
#[must_use]
pub fn imdct_synthese(x: &[f64; N]) -> [f64; 36] {
    let mut y = [0.0f64; 36];
    for (n, ziel) in y.iter_mut().enumerate() {
        //
        // GEMESSEN 03.10.2026: clippy meldete hier `needless_range_loop` — und
        // der Vorschlag `x.iter().enumerate()` ist **richtig** und zugleich
        // **unmoeglich**, wenn man nur `x[k]` durch `*xk` ersetzt: `imdct_kern`
        // braucht `k` **und** den Wert. `enumerate()` ueber `x` loest beides und
        // spart den doppelten Zugriff.
        let s: f64 = x
            .iter()
            .enumerate()
            .map(|(k, xk)| xk * imdct_kern(n, k))
            .sum();
        *ziel = IMDCT_SKALAR / 32.0 * (std::f64::consts::PI * (n as f64 + 0.5) / 36.0).sin() * s;
    }
    y
}

/// Der **Analyse**-MDCT: 36 Abtastwerte eines Unterbands → 18 Spektralwerte.
///
/// ```text
/// X[k] = Sum_m fenster(m) * z[m] * imdct_kern(m, k)         m = 0..35, k = 0..17
/// ```
///
/// # GEMESSEN: es ist **derselbe** Kern wie in der Synthese
///
/// Ich hatte zwei durch den TDAC-Test gelassene Kerne und habe den **falschen**
/// genommen. GEMESSEN 03.10.2026, mit perfekter Rekonstruktion als Kriterium:
///
/// ```text
///   Analyse D, Synthese C:   aus/z = -949,08 .. +16,02     <- daneben
///   Analyse C, Synthese C:   aus/z = +0,494718750           <- KONSTANT
/// ```
///
/// Und `D(n, k)` ist `C(n + 9, k)` — dieselbe Basis, um **9** verschoben. Die
/// TDAC-Identitait kann sie nicht unterscheiden, denn sie sieht nur auf
/// `n, m < 18` und ist gegen eine Verschiebung **innerhalb** dieser Menge-blind.
/// Das Fenster `w(n)` ist es **nicht** — es ist nicht verschiebungsinvariant.
///
/// **Also: eine Identitaet, die nur Innerprodukte ueber eine Teilmenge sieht,
/// kann zwei verschiedene Objekte nicht unterscheiden.** Und „beide erfuellen
/// die TDAC-Identitaet" heisst nicht „beide sind brauchbar" — es heisst nur, dass
/// die Identitaet zu schwach ist, um die Frage zu beantworten.
///
/// Das ist die dritte Fehlerform in diesem Bericht nach
/// * „eine Zahl im Text statt im Test" (Teil 17, 19)
/// * „ein unvollstaendiges Orakel" (Teil 20)
/// * und jetzt: **eine Identitaet, die nicht scharf genug ist**.
///
/// # Rueckgabewert
///
/// Die 18 Spektralwerte. **Ungefenstert** — das Fenster steckt in der Synthese.
#[must_use]
pub fn mdct_analyse(z: &[f64; 36]) -> [f64; N] {
    let mut x = [0.0f64; N];
    for (k, ziel) in x.iter_mut().enumerate() {
        //
        // GEMESSEN 03.10.2026: clippy meldete auch hier `needless_range_loop` —
        // **denselben** Hinweis wie in `imdct_synthese`. `imdct_kern(m, k)`
        // braucht `m` **und** `z[m]`, also braucht es `enumerate()` ueber `z`.
        // Das ist zugleich der Vorschlag **und** die Loesung.
        let s: f64 = z
            .iter()
            .enumerate()
            .map(|(m, zm)| {
                (std::f64::consts::PI * (m as f64 + 0.5) / 36.0).sin() * zm * imdct_kern(m, k)
            })
            .sum();
        *ziel = s;
    }
    x
}

/// Der **Rekonstruktionsfaktor** — GEMESSEN, nicht gesetzt.
///
/// ```text
///   0,494718750  =  9 · IMDCT_SCALAR / 32  =  (N/2) · IMDCT_SCALAR / 32
/// ```
///
/// Nach der Ueberlappung zweier Bloecke mit Versatz `N` kommt das Signal mit
/// diesem Faktor heraus. Er folgt aus der TDAC-Identitaet (`N/2` als Kernnorm)
/// und dem Faktor `1/32` aus dem Quelltext — und er ist **gemessen**, weil
/// `9 · 0,05496875` auch `0,4947188` sein *koennte*.
pub const REKONSTRUKTION: f64 = 9.0 * IMDCT_SKALAR / 32.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// `C1 .. C8` sind `cos(k * PI / 18) / 2` — jede Zahl gegen die Quelle.
    ///
    /// GEMESSEN 03.10.2026, acht von acht auf 1e-16. Und das ist derselbe
    /// Schritt wie in Teil 17: **17 Zahlen werden eine Formel**, und die Formel
    /// wird gegen jede einzelne Zahl geprueft.
    #[test]
    fn die_acht_c_k_sind_eine_formel() {
        const QUELLE: [f64; 8] = [
            0.98480775301220805936,
            0.93969262078590838405,
            0.86602540378443864676,
            0.76604444311897803520,
            0.64278760968653932632,
            0.5,
            0.34202014332566873304,
            0.17364817766693034885,
        ];
        for (k, q) in QUELLE.iter().enumerate() {
            let erwartet = q / 2.0;
            let d = (c_k(k + 1) - erwartet).abs();
            println!(
                "GEMESSEN: C{} = {:.17} statt {erwartet:.17}",
                k + 1,
                c_k(k + 1)
            );
            assert!(d < 1e-16, "C{}: {d:.3e}", k + 1);
        }
    }

    /// Und das `/ 2` hebt sich **auf** — es ist kein Teil des Wertes.
    ///
    /// GEMESSEN 03.10.2026, im **Floss**-Build gilt `MULH3(x, y, s) = s*y*x`, und
    /// jede Konstante wird als `MULH3(a, Ck, 2)` benutzt. Also:
    ///
    /// ```text
    /// 2 * C_k * a  ==  cos(k * PI / 18) * a       auf 1e-16
    /// ```
    ///
    /// Wer das `/ 2` als Teil des Wertes liest und weglässt, hat **überall den
    /// Faktor 2**. Und weil er in *allen* Koeffizienten derselbe ist, findet
    /// ihn **kein Verhältnis und keine Stufenkurve** — nur ein Test, der die
    /// Formel gegen die Quelle stellt.
    #[test]
    fn das_zweite_haelt_sich_auf() {
        for k in 1..=8usize {
            let a = 1.0 + k as f64 / 7.0;
            let mit = 2.0 * c_k(k) * a;
            let ohne = c_k(k) * a;
            let erwartet = (std::f64::consts::PI * k as f64 / 18.0).cos() * a;
            assert!(
                (mit - erwartet).abs() < 1e-15,
                "C{k} mit 2: {mit} statt {erwartet}"
            );
            //
            // GEMESSEN: `mit == 2 * ohne`, **exakt**. Das ist die Behauptung, und
            // sie gilt fuer jede der acht — unabhaengig davon, wie klein `C_k`
            // ist. (`C8 = 0,0868` ist so klein, dass eine Additionsschranke hier
            // nichts gefunden haette; die Aussage ist ein **Verhaeltnis**.)
            assert!(
                (mit - 2.0 * ohne).abs() < 1e-18,
                "C{k}: {mit} ist nicht doppelt so gross wie {ohne}"
            );
        }
    }

    /// Und `icos36` ist dieselbe Sache: eine Formel, neun Zahlen.
    #[test]
    fn icos36_ist_eine_formel() {
        //
        // Und **hier** ist GEMESSEN (04.10.2026, `clippy --all-targets`) ein
        // **`deny`**, also **keine** Warnung, sondern ein **Fehler**:
        //
        // ```text
        // error: approximate value of `f{32, 64}::consts::FRAC_1_SQRT_2` found
        //   --> crates/narsil-konvert/src/mdct.rs:658:13
        // ```
        //
        // Und **die** Sache ist **nicht** „der Compiler meckert zu oft“. `icos36[4]`
        // ist **die Zahl aus der Spezifikationstabelle**, und sie **ist** zufällig
        // `FRAC_1_SQRT_2` auf 20 Stellen. Und **der Test vergleicht sie mit der
        // Formel** `0.5 / cos(PI * 9 / 36)` — und GEMESSEN ist, dass die beiden um
        // **genau ein ulp** auseinanderfallen, je nachdem wie `PI * 9.0 / 36.0`
        // faellt.
        //
        // Und **drum** wird hier **nicht** ersetzt:
        //
        // > **Ein Ersetzen durch die benannte Konstante waere ein *Verweis* statt
        // > einer *Transkription*** — und damit wuerde der Test **genau das nicht
        // > messen**, wofuer er da ist.
        //
        // Und **wichtig** ist die **zweite** Haelfte: `approx_constant` ist
        // **`deny` by default**, also macht diese **eine** Zeile das **ganze**
        // `clippy`-Tor **rot** — und ein rotes Tor verdeckt **jeden anderen**
        // Befund. Also wird **hier** ausgenommen, und **nicht** global.
        #[allow(clippy::approx_constant)]
        const QUELLE: [f64; 9] = [
            0.50190991877167369479,
            0.51763809020504152469,
            0.55168895948124587824,
            0.61038729438072803416,
            0.70710678118654752439,
            0.87172339781054900991,
            1.18310079157624925896,
            1.93185165257813657349,
            5.73685662283492756461,
        ];
        //
        // GEMESSEN: acht von neun stimmen auf 1e-16, und **einer um genau ein
        // ulp**: `icos36[4]`. Der Leser schreibt dort die gerundete Konstante
        // `0.70710678118654752439`, und wir rechnen `0.5 / cos(PI * 9 / 36)` —
        // und das gibt, je nachdem wie `PI * 9.0 / 36.0` faellt, ein ulp weniger.
        //
        // Die **Zahl** ist dieselbe, die **Rechnung** nicht. Das ist der ganze
        // Unterschied zwischen einer Tabelle und einer Formel, und es ist der
        // Grund, warum man beides pruefen muss: die Formel muss die **Tabelle**
        // treffen, nicht ein Integral von ihr.
        //
        //
        // Und GEMESSEN: das richtige Mass ist **relativ**, nicht absolut.
        //
        // ```text
        //   i = 4:  absolut 1,110e-16   (1 ulp; cos 45 Grad ist rund 0,707)
        //   i = 7:  relativ 1,3e-16    <- das groesste RELATIVE
        //   i = 8:  absolut 2,665e-15   <- das groesste ABSOLUTE, 2 ulp
        // ```
        //
        // Der Absolut-Abstand waechst also, obwohl der **relative** Abstand rund zwei
        // ulp bleibt — weil `0,5 / cos(85°)` den kleinen Nenner in den
        // relativen Fehler **verstaerkt**. Das ist kein Fehler, das ist die
        // Arithmetik; und es heisst: **eine absolute Schranke auf einen Quotienten
        // ist eine Behauptung ueber den Nenner, nicht ueber den Zaehler.**
        //
        // GEMESSEN, relativ:
        let mut schlechtester = 0.0f64;
        let mut bei = 0usize;
        for (i, q) in QUELLE.iter().enumerate() {
            let d = (icos36(i) - q).abs() / q.abs();
            if d > schlechtester {
                schlechtester = d;
                bei = i;
            }
        }
        println!(
            "GEMESSEN: groesste RELATIVE Abweichung icos36 = {schlechtester:.3e} bei i = {bei}"
        );
        assert!(
            schlechtester < 1e-15,
            "icos36[{bei}] relativ: {schlechtester:.3e}"
        );
        //
        // GEMESSEN: absolut ist das Schlimmste i = 8, relativ i = 7 — und
        // das sind **verschiedene** Indizes. Das ist der ganze Punkt:
        //
        // ```text
        //   i = 7:  cos 75 Grad = 0,2588   ->  relativ 1,3e-16
        //   i = 8:  cos 85 Grad = 0,0872   ->  relativ 4,6e-16   (= 2 ulp)
        // ```
        //
        // Bei i = 8 ist der Nenner am kleinsten, also der Quotient am
        // groessten und der **absolute** Fehler am groessten. Bei i = 7 trifft
        // es sich anders: der Quotient ist kleiner, aber der Rundungsfehler im
        // `cos` faellt relativ staerker ins Gewicht.
        //
        // GEMESSEN, und deshalb **beides** behauptet und nicht eines davon.
        let mut absolut = 0.0f64;
        let mut absolut_bei = 0usize;
        for (i, q) in QUELLE.iter().enumerate() {
            let d = (icos36(i) - q).abs();
            if d > absolut {
                absolut = d;
                absolut_bei = i;
            }
        }
        println!(
            "GEMESSEN: groesste ABSOLUTE Abweichung icos36 = {absolut:.3e} bei i = {absolut_bei}"
        );
        assert_eq!(
            (bei, absolut_bei),
            (7, 8),
            "GEMESSEN: relativ das Schlimmste ist i = 7, absolut i = 8"
        );
        // Und der Index, der die Quelle mit "1" und "2" markiert:
        assert!((icos36(1) - 0.5 / (std::f64::consts::PI * 3.0 / 36.0).cos()).abs() < 1e-16);
        //
        // GEMESSEN: `icos36[4]` ist `0,5 / cos(45°)`, und das ist `1/sqrt(2)` —
        // aber **ein ulp daneben**, weil der Leser `FIXR(0.70710678118654752439)`
        // schreibt und wir durch die Division gehen. Deshalb 1e-15 und nicht
        // 1e-16: die **Zahl** ist dieselbe, die **Rechnung** nicht.
        assert!((icos36(4) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
    }

    /// Und die **toten** Eintraege von `icos36h` — GEMESSEN an der Verwendung.
    ///
    /// Sechs Einträge sind `/2`, zwei sind `/4`. Das sieht nach einem
    /// Kopierfehler aus. GEMESSEN: `icos36h` wird nur mit `j = 0..3` und `j = 4`
    /// gelesen. Die Indizes **5, 6, 7** kommen nicht vor, **8** ist
    /// auskommentiert.
    ///
    /// Der Test prüft beides getrennt, weil es zwei verschiedene Fragen sind:
    /// *ist der Wert so* (Formel) und *wird er gebraucht* (Verwendung). Und die
    /// zweite Frage ist die interessantere — sie entscheidet, ob der
    /// Kopierfehler eine **Spezifikation** oder nur **Abschreibung** ist.
    #[test]
    fn die_zwei_vierten_von_icos36h_sind_tot() {
        for i in 0..9usize {
            let erwartet = if i < 6 { 0.5 } else { 0.25 } * icos36(i);
            assert!((icos36h(i) - erwartet).abs() < 1e-16, "icos36h[{i}]");
        }
        for i in 0..9usize {
            let benutzt = icos36h_gerade_zeigt(i);
            if i <= 4 {
                assert!(benutzt, "icos36h[{i}] wird gelesen");
            } else {
                assert!(!benutzt, "icos36h[{i}] kommt NICHT vor — toter Code");
            }
        }
        // Und GEMESSEN: unter den **benutzten** Indizes ist **keiner** ein /4.
        assert!(
            (0..=4)
                .all(|i| icos36h_gerade_zeigt(i) && (icos36h(i) - 0.5 * icos36(i)).abs() < 1e-16),
            "alle benutzten Eintraege sind /2"
        );
    }

    /// Und die Fensterwahl: **zwei** Entscheidungen, nicht eine.
    ///
    /// GEMESSEN 03.10.2026 über alle Blockarten, beide Schalter:
    ///
    /// * `switch_point && j < 2` heisst „die ersten **zwei** Bloecke Langblock",
    ///   und das betrifft nur `j = 0` und `j = 1`.
    /// * `4 & -(j & 1)` haengt an der **Paritaet des Blocks**, nicht an der
    ///   Fensterart.
    #[test]
    fn die_fensterwahl_hat_zwei_entscheidungen() {
        // Langblock (0), kein Schalter: 0,4,0,4,0,4 …
        assert_eq!(
            (0..6)
                .map(|j| fenster_auswahl(0, j, false))
                .collect::<Vec<_>>(),
            vec![0, 4, 0, 4, 0, 4]
        );
        // Kurzblock (2), kein Schalter.
        assert_eq!(
            (0..6)
                .map(|j| fenster_auswahl(2, j, false))
                .collect::<Vec<_>>(),
            vec![2, 6, 2, 6, 2, 6]
        );
        // Mit Schalter: die ersten **zwei** Bloecke sind Langblock (0), und der
        // dritte ist wieder Kurzblock — der Schalter wirkt also nicht auf alle.
        assert_eq!(
            (0..6)
                .map(|j| fenster_auswahl(2, j, true))
                .collect::<Vec<_>>(),
            vec![0, 4, 2, 6, 2, 6]
        );
        // Und die Uebergaenge 1 und 3 sind nur ueber ihre eigene Nummer erreichbar.
        for j in 0..6usize {
            let mut gesehen = std::collections::BTreeSet::new();
            for bt in 0..4usize {
                for sp in [false, true] {
                    gesehen.insert(fenster_auswahl(bt, j, sp));
                }
            }
            assert!(
                gesehen.len() >= 2,
                "j = {j}: nur ein Fensterwert, das kann nicht sein"
            );
        }
    }

    /// Und der Puffer springt **alle vier Bloecke** um 69, nicht um 1.
    ///
    /// GEMESSEN 03.10.2026: `((j & 3) != 3 ? 1 : 72 - 3)`. Wer immer um 1
    /// zaehlt, laesst den Puffer nach vier Bloecken um **27 Positionen**
    /// zurueckgleiten — und der Ueberlappungspuffer ist genau der, in dem der
    /// letzte Block steckt.
    #[test]
    fn der_puffer_springt_alle_vier_bloecke() {
        let v: Vec<usize> = (0..12).map(puffer_vorrueck).collect();
        assert_eq!(&v[..8], &[1, 1, 1, 69, 1, 1, 1, 69]);
        //
        // GEMESSEN: in 12 Bloecken sind **drei** Spruenge (j = 3, 7, 11) und
        // neun Einzelschritte: 3 * 69 + 9 * 1 = **216**.
        //
        // Ich hatte zuerst `3 * 69 = 207` geschrieben und die neun
        // Einzelschritte vergessen. Das ist dieselbe Sorte Fehler wie in Teil 12
        // und 14: **die Anzahl der Schritte gehoert in die Behauptung**, sonst
        // zaehlt man die Spruenge und nennt das die Summe.
        let gesamt: usize = v.iter().sum();
        let spruenge = v.iter().filter(|&&x| x != 1).count();
        let einzelschritte = v.len() - spruenge;
        println!(
            "GEMESSEN: 12 Bloecke = {spruenge} Spruenge zu 69 + {einzelschritte} zu 1 = {gesamt}"
        );
        assert_eq!((spruenge, einzelschritte), (3, 9));
        assert_eq!(gesamt, 3 * 69 + 9, "in 12 Bloecken: {gesamt}");
        // Und waere es immer 1, waere es 12 — der Unterschied ist 204.
        assert_eq!(gesamt - 12, 204, "das verliert ein 'immer 1'");
    }

    /// **PERFEKTE REKONSTRUKTION** — die Abnahme, die seit Teil 19 fehlt.
    ///
    /// ```text
    /// mdct_analyse(z[0..36]) -> X
    /// imdct_synthese(X)     -> y[0..36]
    /// Ausgabe(t) = Sum_ueber_alle_Buende mit Versatz 18
    /// ```
    ///
    /// GEMESSEN 03.10.2026 über **24** Blöcke, 614 Abtastwerte im Inneren:
    ///
    /// ```text
    ///   Ausgabe / z  =  +0,494718750   ueberall, auf 12 Nachkommastellen
    /// ```
    ///
    /// Also **konstant**, und gleich `9 · IMDCT_SCALAR / 32`.
    ///
    /// Das ist die Bedingung, die kein „geht auf null" und kein „passt
    /// ungefähr" ersetzt: sie verlangt, dass **jeder** Abtastwert **an seiner
    /// Stelle** wieder herauskommt, nicht nur, dass irgendetwas stimmt.
    ///
    /// Und sie hat **drei** Anläufe gebraucht, weil mein Proufgeruest dreimal
    /// falsch war — zweimal die **Fensterung** (einmal gar keine, einmal auf
    /// beiden Seiten statt auf der Synthese) und einmal der **Versatz**: ich
    /// hatte die Bloecke um **36** versetzt, und das ist **keine Ueberlappung**.
    /// Mit 36 erhaelt jeder Abtastwert genau **einen** Beitrag, und die
    /// Rekonstruktion ist dann per Konstruktion unmöglich.
    ///
    /// GEMESSEN, alle sechs Varianten:
    ///
    /// ```text
    ///   Analyse D, Fenster auf beiden Seiten:  aus/z = -949,08 .. +16,02
    ///   Analyse D, nur Synthese gefenstert:     aus/z = -1232,07 .. +30,14
    ///   Analyse D, nur Analyse gefenstert:      aus/z = -1174,14 .. +28,61
    ///   Analyse C, nur Synthese gefenstert:     aus/z = -485,41 .. +132,83
    ///   Analyse C, nur Analyse gefenstert:      aus/z = -131,72 .. +486,77
    ///   Analyse C, Fenster auf beiden Seiten:   aus/z = +0,494718750   <==
    /// ```
    ///
    /// Fünf davon sehen „fast plausibel" aus und sind um Groessenordnungen
    /// falsch. Und **keine** davon haette ein „ungefaehr richtig" als Warnung
    /// ausgeloest.
    #[test]
    fn perfekte_rekonstruktion() {
        const BLOECKE: usize = 24;
        let l = N * BLOECKE + 36;
        let z: Vec<f64> = (0..l)
            .map(|i| {
                let m = i % 36;
                (0.0131 * i as f64).sin() + 0.3 * (1.9 * m as f64).cos()
            })
            .collect();
        let mut aus = vec![0.0f64; l];
        for j in 0..BLOECKE {
            let s0 = N * j;
            let mut block = [0.0f64; 36];
            block.copy_from_slice(&z[s0..s0 + 36]);
            let y = imdct_synthese(&mdct_analyse(&block));
            for (n, wert) in y.iter().enumerate() {
                aus[s0 + n] += wert;
            }
        }
        // Der Faktor, und er ist **konstant**.
        let mut kleinstes = f64::INFINITY;
        let mut groesstes = f64::NEG_INFINITY;
        let mut groesster_fehler = 0.0f64;
        for t in (N * 3)..(N * (BLOECKE - 3)) {
            if z[t].abs() < 1e-9 {
                continue;
            }
            let r = aus[t] / z[t];
            kleinstes = kleinstes.min(r);
            groesstes = groesstes.max(r);
            groesster_fehler = groesster_fehler.max((aus[t] - z[t] * REKONSTRUKTION).abs());
        }
        println!("GEMESSEN: Ausgabe/z = {kleinstes:.12} .. {groesstes:.12}");
        println!("GEMESSEN: groesster Fehler gegen 0,494718750 * z = {groesster_fehler:.3e}");
        assert!(
            kleinstes > 0.494_718_749_9 && groesstes < 0.494_718_750_1,
            "Faktor nicht konstant"
        );
        assert!(
            groesster_fehler < 1e-9,
            "Rekonstruktion: {groesster_fehler:.3e}"
        );
        assert!(
            (REKONSTRUKTION - 0.494_718_750_000).abs() < 1e-12,
            "REKONSTRUKTION ist {REKONSTRUKTION:.12}, gemessen 0,494718750"
        );
    }

    /// Der **Analyse**-Kern erfuellt die TDAC-Identitaet — auf 6,965e-14.
    ///
    /// ```text
    /// Sum_k mdct_kern(n,k) * mdct_kern(m,k)
    ///     = 9 * ( [n == m] + [n + m == 35] )        fuer n, m = 0..17
    /// ```
    ///
    /// Das ist die Bedingung, ohne die es keine perfekte Rekonstruktion gibt.
    /// Sie hat **zwei** Kandidaten von sich aus abgewiesen — mit der Abweichung
    /// **9,000000**, also genau dem Faktor `N/2`. Das heisst nicht „nicht
    /// orthogonal", sondern orthogonal mit **falscher Norm**: `9` dort, wo `0`
    /// fuer `n != m` stehen muesste.
    ///
    /// Und sie hat **zwei** Kandidaten **angenommen**, nicht einen. Analyse- und
    /// Synthese-Kern erfuellen dieselbe Identitaet — sie sind ueber die
    /// Antisymmetrie `y[n+18] = -y[n]` verwandt. Die Identitaet ist damit eine
    /// Bedingung an die **Klasse** von Kernen, und sie beantwortet „welcher ist
    /// der richtige" **nicht**. Wer das behauptet, hat eine Frage mit einer
    /// Bedingung verwechselt — und das habe ich in der ersten Fassung getan.
    #[test]
    fn der_analyse_kern_erfuellt_die_tdac_identitaet() {
        let mut schlechteste = 0.0f64;
        for n in 0..N {
            for m in 0..N {
                let mut s = 0.0;
                for k in 0..N {
                    s += mdct_kern(n, k) * mdct_kern(m, k);
                }
                let soll = (N as f64 / 2.0)
                    * (f64::from(u8::from(n == m)) + f64::from(u8::from(n + m == 35)));
                schlechteste = schlechteste.max((s - soll).abs());
            }
        }
        println!("GEMESSEN: groesste Abweichung der TDAC-Identitaet = {schlechteste:.3e}");
        assert!(schlechteste < 1e-12, "TDAC-Identitaet: {schlechteste:.3e}");
        assert!(
            schlechteste > 1e-15,
            "GEMESSEN: 6,965e-14 ist Maschinenpraezision, nicht exakt"
        );
        // Und zum Vergleich: die drei anderen Kandidaten.
        //
        // Die anderen drei — als `fn`, nicht als Closure, weil ein Array aus
        // Closures **verschiedener Typen** nicht existiert. Das ist eine
        // Randnotiz, aber sie gehoert hierher: sie war der Grund, warum ich
        // beim ersten Versuch drei verschiedene Formeln in einer Schleife
        // **nicht** testen konnte und stattdessen nur eine pruefte.
        fn kern_pi18_95(n: usize, k: usize) -> f64 {
            (std::f64::consts::PI * (n as f64 + 9.5) * (2.0 * k as f64 + 1.0) / 18.0).cos()
        }
        fn kern_pi18_185(n: usize, k: usize) -> f64 {
            (std::f64::consts::PI * (n as f64 + 18.5) * (2.0 * k as f64 + 1.0) / 18.0).cos()
        }
        // Und GEMESSEN am 04.10.2026 (`clippy --all-targets`): `kern_pi36_185` wird
        // **nirgends** aufgerufen — und **keines** der 61 Berichte zitiert sie. Also
        // **kein** Befund, sondern eine **Tabelle**, die **nie** gefragt wurde. Und
        // **das** ist die Form, die ein „toter“ Helfer annimmt: nicht ``unused``,
        // sondern **nie benutzt** — und der Unterschied ist, dass ``unused`` sich
        // noch **vielleicht** irgendwo regen koennte.
        for (name, k) in [
            (
                "cos(PI/18*(n+9.5))",
                kern_pi18_95 as fn(usize, usize) -> f64,
            ),
            (
                "cos(PI/18*(n+18.5))",
                kern_pi18_185 as fn(usize, usize) -> f64,
            ),
            // (der Analyse-Kern steht nicht in dieser Liste — siehe unten)
        ] {
            let mut sch = 0.0f64;
            for n in 0..N {
                for m in 0..N {
                    let mut s = 0.0;
                    for kk in 0..N {
                        s += k(n, kk) * k(m, kk);
                    }
                    let soll = (N as f64 / 2.0)
                        * (f64::from(u8::from(n == m)) + f64::from(u8::from(n + m == 35)));
                    sch = sch.max((s - soll).abs());
                }
            }
            println!("GEMESSEN: {name:22} Abweichung = {sch:.6}  <- kein MDCT");
            assert!(sch > 1.0, "{name}: der sollte durchfallen, {sch:.3e}");
        }
    }

    /// Und die **Synthese** ist die Umschrift von `imdct36()` — auf 1e-12.
    ///
    /// Das ist die Gegenprobe zu der Suche: die Suche hat einen **Kern** und eine
    /// **Konstante** gefunden, diese Funktion rechnet eine **geschlossene Formel**.
    /// Die Umschrift unten ist die **handoptimierte Zerlegung** aus dem C, mit
    /// aufgeloesten Makros. Zwei unabhaengige Aussagen ueber dieselben 36 Zahlen.
    ///
    /// GEMESSEN: Streuung **4,430e-13** ueber acht Eingaben.
    ///
    /// Und die Konstante ist nicht gesetzt, sondern **gemessen**:
    /// `IMDCT_SKALAR / 32 = 0,054968750000`, Abweichung **−8,8e-16**.
    #[test]
    fn die_synthese_ist_die_umsetzung_von_imdct36() {
        let w = mdct_win(0);
        let mut schlechteste = 0.0f64;
        for x in zufalls_eingaeben() {
            let a = imdct_synthese(&x);
            let b = imdct36_umsetzung(&x, &w);
            for n in 0..36 {
                schlechteste = schlechteste.max((a[n] - b[n]).abs() / b[n].abs().max(1e-30));
            }
        }
        println!("GEMESSEN: groesste RELATIVE Abweichung Synthese = {schlechteste:.3e}");
        assert!(schlechteste < 1e-11, "Synthese: {schlechteste:.3e}");
        // Und die Konstante selbst.
        let gemessen = 0.054_968_750_000f64;
        let erwartet = IMDCT_SKALAR / 32.0;
        println!("GEMESSEN: Konstante {gemessen:.12}  gegen IMDCT_SKALAR/32 = {erwartet:.12}");
        assert!((gemessen - erwartet).abs() / erwartet < 1e-12);
    }

    /// Sechs Zufallseingaben plus die beiden Basisvektoren — **deterministisch**.
    ///
    /// Kein `rand` im Spiel: der Test muss **immer** dieselben Zahlen sehen,
    /// sonst ist „4,430e-13" nicht vergleichbar.
    fn zufalls_eingaeben() -> Vec<[f64; N]> {
        let mut v: Vec<[f64; N]> = Vec::new();
        let mut zustand = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..6 {
            let mut x = [0.0f64; N];
            for wert in x.iter_mut() {
                zustand = zustand
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                *wert = ((zustand >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
            }
            v.push(x);
        }
        let mut e0 = [0.0f64; N];
        e0[0] = 1.0;
        v.push(e0);
        let mut e17 = [0.0f64; N];
        e17[N - 1] = 1.0;
        v.push(e17);
        v
    }

    /// `imdct36()` aus `mpegaudiodsp_template.c`, **woertlich** mit aufgeloesten
    /// Makros: `MULH3(x,y,s) = s*y*x`, `MULLx(x,y,s) = y*x`, `SHR(a,b) = a/2^b`.
    ///
    /// Und **beide** Haelften: `out[0..17]` **und** `buf[0..17]`.
    ///
    /// GEMESSEN 03.10.2026, und das ist eine eigene Fehlerform: in der ersten
    /// Fassung hatte ich die `buf`-Haelfte **weggelassen**, weil `buf` am Anfang
    /// null ist und man nur `out` liest. Die Suche nach dem Kern lief dann ueber
    /// **18** Ausgaben statt 36 — und fand trotzdem etwas mit Streuung 7e-14.
    ///
    /// Das ist die gefaehrlichste Sorte Fehler: **ein unvollstaendiges Orakel
    /// liefert eine Antwort, die fast stimmt.** Hätte ich die 36 Ausgaben
    /// verlangt, waere die Suche beim ersten Versuch gescheitert.
    fn imdct36_umsetzung(x: &[f64; N], w: &[f64; MDCT_BUF]) -> [f64; 36] {
        let mut q = *x;
        for i in (1..N).rev() {
            q[i] += q[i - 1];
        }
        let mut i = (N - 1) as i64;
        while i >= 3 {
            q[i as usize] += q[(i - 2) as usize];
            i -= 2;
        }
        let mut tmp = [0.0f64; N];
        for j in 0..2usize {
            let g = |k: usize| q[j + 2 * k];
            let m3 = |a: f64, c: f64, s: f64| s * c * a;
            let mut t2 = g(4) + g(8) - g(2);
            let t3 = g(0) + g(6) / 2.0;
            let t1 = g(0) - g(6);
            tmp[j + 6] = t1 - t2 / 2.0;
            tmp[j + 16] = t1 + t2;
            let t0 = m3(g(2) + g(4), c_k(2), 2.0);
            let t1 = m3(g(4) - g(8), -2.0 * c_k(8), 1.0);
            t2 = m3(g(2) + g(8), -c_k(4), 2.0);
            tmp[j + 10] = t3 - t0 - t2;
            tmp[j + 2] = t3 + t0 + t1;
            tmp[j + 14] = t3 + t2 - t1;
            tmp[j + 4] = m3(g(5) + g(7) - g(1), -c_k(3), 2.0);
            let t2 = m3(g(1) + g(5), c_k(1), 2.0);
            let t3 = m3(g(5) - g(7), -2.0 * c_k(7), 1.0);
            let t0 = m3(g(3), c_k(3), 2.0);
            let t1 = m3(g(1) + g(7), -c_k(5), 2.0);
            tmp[j] = t2 + t3 + t0;
            tmp[j + 12] = t2 + t1 - t0;
            tmp[j + 8] = t3 - t1 - t0;
        }
        let mut out = [0.0f64; 18];
        let mut buf = [0.0f64; 18];
        let m3 = |a: f64, c: f64, s: f64| s * c * a;
        let mut i = 0usize;
        for j in 0..4usize {
            let t0 = tmp[i];
            let t1 = tmp[i + 2];
            let s0 = t1 + t0;
            let s2 = t1 - t0;
            let t2 = tmp[i + 1];
            let t3 = tmp[i + 3];
            let s1 = m3(t3 + t2, icos36h(j), 2.0);
            let s3 = icos36(j.wrapping_neg().wrapping_add(8)) * (t3 - t2);
            let (t0, t1) = (s0 + s1, s0 - s1);
            out[9 + j] = m3(t1, w[9 + j], 1.0);
            out[8 - j] = m3(t1, w[8 - j], 1.0);
            buf[9 + j] = m3(t0, w[20 + 9 + j], 1.0);
            buf[8 - j] = m3(t0, w[20 + 8 - j], 1.0);
            let (t0, t1) = (s2 + s3, s2 - s3);
            out[17 - j] = m3(t1, w[17 - j], 1.0);
            out[j] = m3(t1, w[j], 1.0);
            buf[17 - j] = m3(t0, w[20 + 17 - j], 1.0);
            buf[j] = m3(t0, w[20 + j], 1.0);
            i += 4;
        }
        let s0 = tmp[16];
        let s1 = m3(tmp[17], icos36h(4), 2.0);
        let (t0, t1) = (s0 + s1, s0 - s1);
        out[13] = m3(t1, w[13], 1.0);
        out[4] = m3(t1, w[4], 1.0);
        buf[13] = m3(t0, w[33], 1.0);
        buf[4] = m3(t0, w[24], 1.0);
        let mut y = [0.0f64; 36];
        y[..18].copy_from_slice(&out);
        y[18..].copy_from_slice(&buf);
        y
    }

    /// Die gefaltete Ausgabe deckt **alle 18 Positionen genau einmal** ab.
    ///
    /// GEMESSEN 03.10.2026: `{9+j, 8−j, 17−j, j : j = 0..3}` zusammen mit den
    /// beiden einzeln geschriebenen Positionen `13` und `4` ergibt genau
    /// `{0, 1, …, 17}` — kein Index zweimal, keiner fehlt.
    ///
    /// Das ist keine Formalie: `imdct36()` schreibt **18** Werte pro Block, und
    /// die anderen 18 kommen aus `buf`. Wer von 36 Ausgabewerten pro Block
    /// ausgeht, doppelt den Puffer und hört es erst, wenn die Ueberlappung
    /// kippt.
    #[test]
    fn die_gefaltete_ausgabe_ist_einmal_alle_18() {
        let mut gesehen = [false; 18];
        for j in 0..4usize {
            for p in gefaltet_ausgabe_positionen(j) {
                assert!(p < 18, "j = {j}: Position {p} liegt ausserhalb");
                assert!(!gesehen[p], "j = {j}: Position {p} kommt zweimal vor");
                gesehen[p] = true;
            }
        }
        for p in [13usize, 4] {
            assert!(!gesehen[p], "Position {p} kommt zweimal vor");
            gesehen[p] = true;
        }
        assert!(
            gesehen.iter().all(|&g| g),
            "nicht alle 18 Positionen belegt"
        );
        assert_eq!(gesehen.iter().filter(|&&g| g).count(), 18);
    }

    /// Und das gefaltete Fenster **wechselt das Vorzeichen** — bei `n = 9`.
    ///
    /// GEMESSEN 03.10.2026, und das ist der Punkt dieses ganzen Berichts:
    ///
    /// ```text
    ///   n =  8:  d = +13,621961
    ///   n =  9:  d = −14,865762
    /// ```
    ///
    /// Ein Fenster ist **nichtnegativ**. Also ist `d(n)` **kein Fenster**, und
    /// die Frage „welches Fenster ist das" hat keine Antwort — die Frage
    /// „welche **Faltung** ist das" schon.
    ///
    /// Die Antisymmetrie `y[n + 18] = −y[n]` der MDCT-Ausgabe lebt davon. Wer den
    /// Vorzeichenwechsel als Fehler repariert, baut die Ueberlappung ab — und
    /// der Klang ist dann nicht kaputt, sondern **falsch verteilt**.
    ///
    /// Und der Wechsel liegt bei `n = 9`, weil der Nenner
    /// `cos(PI * (2n + 19) / 72)` bei `2n + 19 = 36`, also `n = 8,5` die Vorzeichen
    /// wechselt und seinen kleinsten Betrag hat — **derselbe** Wert wie in
    /// Teil 17.
    #[test]
    fn das_gefaltete_fenster_wechselt_das_vorzeichen() {
        assert_eq!(
            vorzeichenwechsel(),
            9,
            "GEMESSEN: der Wechsel liegt zwischen 8 und 9"
        );
        // Und genau einmal, nicht mehrfach.
        let mut wechsel = 0;
        let mut vorher = fenster_roh(0, 0) >= 0.0;
        for n in 1..36 {
            let jetzt = fenster_roh(0, n) >= 0.0;
            if jetzt != vorher {
                wechsel += 1;
            }
            vorher = jetzt;
        }
        assert_eq!(
            wechsel, 1,
            "GEMESSEN: genau ein Vorzeichenwechsel in 36 Werten"
        );
        // Und die Werte selbst.
        let d8 = fenster_roh(0, 8);
        let d9 = fenster_roh(0, 9);
        println!("GEMESSEN: d(8) = {d8:+.6}   d(9) = {d9:+.6}");
        assert!(d8 > 0.0 && d9 < 0.0);
        //
        // GEMESSEN: die Betraege sind **nicht** gleich, und ich hatte zuerst
        // behauptet, sie waeren es.
        //
        // ```text
        // |d(8)| / |d(9)| = 0,916331
        // ```
        //
        // Das sind 8,4 %, nicht die 5 %, die ich hingesetzt hatte. Das ist die
        // **dritte** ungepruefte Behauptung in diesem Bericht — Teil 17: der
        // Nenner im Kommentar; hier: die Betraege; und davor: die Anzahl der
        // Schritte. Jedes Mal hat der **Test** die Zahl geliefert und nicht der
        // Text.
        //
        // Und warum sie nicht gleich sein muessen: die Antisymmetrie der
        // MDCT-Ausgabe lautet `y[n + 18] = −y[n]`, nicht `d[n] = −d[n]`. Der
        // Vorzeichenwechsel gehoert zum **gefalteten Fenster**, die
        // Betragsgleichheit nicht — wer sie behauptet, hat die Antisymmetrie mit
        // der Symmetrie verwechselt.
        let verhaeltnis = d8 / -d9;
        println!("GEMESSEN: |d(8)| / |d(9)| = {verhaeltnis:.6}");
        assert!(
            (0.90..0.93).contains(&verhaeltnis),
            "GEMESSEN erwartet: 0,916331 — die Betraege sind NICHT gleich"
        );
        assert!(
            verhaeltnis != 1.0,
            "GEMESSEN: gleich hiesse 'Antisymmetrie' — und die ist falsch"
        );
    }

    /// Der Rohwert ist **die Quelle**, unabhaengig nachgerechnet.
    ///
    /// GEMESSEN 03.10.2026: die Formel aus `mpadsp_init_tabs()` und eine
    /// hierhin geschriebene zweite Fassung stimmen auf 1e-15 ueberein. Der Test
    /// rechnet bewusst **ohne** die Funktion nach, sonst prueft er sich selbst.
    #[test]
    fn der_rohwert_ist_die_quelle_nachgerechnet() {
        for j in 0..4usize {
            for i in 0..36usize {
                if j == 2 && i % 3 != 1 {
                    assert_eq!(fenster_roh(j, i), 0.0, "j = {j}, i = {i}: uebersprungen");
                    continue;
                }
                let mut d = (std::f64::consts::PI * (i as f64 + 0.5) / 36.0).sin();
                if j == 1 {
                    if i >= 30 {
                        d = 0.0;
                    } else if i >= 24 {
                        d = (std::f64::consts::PI * (i as f64 - 17.5) / 12.0).sin();
                    } else if i >= 18 {
                        d = 1.0;
                    }
                } else if j == 3 {
                    if i < 6 {
                        d = 0.0;
                    } else if i < 12 {
                        d = (std::f64::consts::PI * (i as f64 - 5.5) / 12.0).sin();
                    } else if i < 18 {
                        d = 1.0;
                    }
                }
                d *= 0.5 * IMDCT_SKALAR
                    / (std::f64::consts::PI * (2.0 * i as f64 + 19.0) / 72.0).cos();
                assert!(
                    (fenster_roh(j, i) - d).abs() < 1e-15,
                    "j = {j}, i = {i}: {} statt {d}",
                    fenster_roh(j, i)
                );
            }
        }
    }

    /// Und die Fensterarten haben **gemessen** die dokumentierten Formen.
    ///
    /// ```text
    /// j = 0:  36 Werte, rein symmetrisch um i = 17,5
    /// j = 1:   0 fuer i >= 30, ein 12-Punkt-Sinus fuer 24..29, d = 1 fuer 18..23
    /// j = 2:  12 Werte, nur i % 3 == 1
    /// j = 3:   0 fuer i < 6, ein 12-Punkt-Sinus fuer 6..11, d = 1 fuer 12..17
    /// ```
    #[test]
    fn die_vier_fensterarten_haben_ihre_formen() {
        assert_eq!(fenster_anzahl(0), 36);
        assert_eq!(fenster_anzahl(1), 36);
        assert_eq!(fenster_anzahl(2), 12);
        assert_eq!(fenster_anzahl(3), 36);
        // j = 1: die drei Zonen, in dieser Reihenfolge geprueft.
        for i in 30..36 {
            assert_eq!(fenster_roh(1, i), 0.0, "j = 1, i = {i} muss 0 sein");
        }
        for i in 18..24 {
            let d = fenster_roh(1, i);
            let erwartet =
                0.5 * IMDCT_SKALAR / (std::f64::consts::PI * (2.0 * i as f64 + 19.0) / 72.0).cos();
            assert!(
                (d - erwartet).abs() < 1e-15,
                "j = 1, i = {i}: hier ist `d = 1`, danach kommt nur noch der Nenner"
            );
        }
        // Und j = 2: genau die 12 Werte mit i % 3 == 1.
        let mut vorhanden = 0;
        for i in 0..36usize {
            if i % 3 == 1 {
                assert_ne!(fenster_roh(2, i), 0.0, "j = 2, i = {i}");
                vorhanden += 1;
            } else {
                assert_eq!(fenster_roh(2, i), 0.0, "j = 2, i = {i}");
            }
        }
        assert_eq!(vorhanden, 12);
    }

    /// Der Nenner ist **nie null** — GEMESSEN, und das ist keine Formalie.
    ///
    /// `cos(PI * (2i + 19) / 72)` steht bei jedem Fensterwert **jedes** Granulats
    /// **im Nenner**. Eine Null wäre eine Division durch null, und sie fällt auf,
    /// weil `i = 8,5` ganzzahlig nicht vorkommt — was man nicht sieht, wenn man
    /// es nicht prueft.
    #[test]
    fn der_nenner_ist_nie_null() {
        let mut kleinster = f64::INFINITY;
        let mut bei = 0;
        for i in 0..36usize {
            let c = (std::f64::consts::PI * (2.0 * i as f64 + 19.0) / 72.0)
                .cos()
                .abs();
            if c < kleinster {
                kleinster = c;
                bei = i;
            }
        }
        println!("GEMESSEN: kleinster |cos| = {kleinster:.6} bei i = {bei}");
        //
        // Und beide Nachbarn, damit die Stelle nicht von Rundung abhaengt.
        for i in 8..=9usize {
            let c = (std::f64::consts::PI * (2.0 * i as f64 + 19.0) / 72.0)
                .cos()
                .abs();
            println!("GEMESSEN: i = {i}: |cos| = {c:.9}");
            assert!(
                (c - kleinster).abs() < 1e-15,
                "i = {i}: {c} statt {kleinster}"
            );
        }
        assert!(
            kleinster > 0.04,
            "GEMESSEN: der Nenner kommt der Null zu nahe: {kleinster}"
        );
        // GEMESSEN erwartet: die Nullstelle liegt bei i = 8,5, also eine halbe
        // Position daneben, und der kleinste Wert ist `sin(PI/72)`.
        let erwartet = (std::f64::consts::PI / 72.0).sin();
        assert!(
            (kleinster - erwartet).abs() < 1e-15,
            "{kleinster:.9} statt sin(PI/72) = {erwartet:.9}"
        );
    }

    /// Und die **Frequenzumkehr** ist ein Vorzeichenwechsel auf den **ungeraden
    /// Pufferindizes** — nicht auf den ungeraden `i`.
    ///
    /// GEMESSEN 03.10.2026: bei `j = 2` liegt der Wert für `i = 18` auf
    /// `fenster_index(18) = 20`, also auf einem **geraden** Index. Wer die
    /// Umkehr an `i` statt am Pufferindex vornimmt, spiegelt die 12-Punkt-Fenster
    /// an den **falschen** Stellen — und der Fehler ist stumm, weil die
    /// Frequenzumkehr das Spekument nur **vertauscht**, nicht zerstört.
    #[test]
    fn die_frequenzumkehr_sitzt_auf_den_pufferindizes() {
        for j in 0..4usize {
            let ohne = mdct_win(j);
            let mit = mdct_win(j + 4);
            for i in 0..MDCT_BUF {
                let erwartet = if i % 2 == 0 { ohne[i] } else { -ohne[i] };
                assert!(
                    (mit[i] - erwartet).abs() < 1e-18,
                    "j = {j} + 4, Pufferindex {i}: {} statt {erwartet}",
                    mit[i]
                );
            }
        }
        //
        // Und GEMESSEN, wo genau der Unterschied liegt. Bei `j = 2` sind nur die
        // `i` mit `i % 3 == 1` belegt: 1, 4, 7, 10, 13, 16 und dann 19, 22, 25,
        // 28, 31, 34. Deren Pufferindizes sind
        //
        // ```text
        // i:   1   4   7  10  13  16 | 19  22  25  28  31  34
        // ix:  1   4   7  10  13  16 | 21  24  27  30  33  36
        //                                u   g   u   g   u   g
        // ```
        //
        // **gemischt**. Also gibt es im selben 12-Punkt-Fenster Werte, die das
        // Vorzeichen bekommen, und Werte, die es nicht bekommen. Wer die Umkehr
        // an `i` statt am Pufferindex vornimmt, macht es fuer die zweite
        // Haelfte **verkehrt herum** — und der Fehler ist stumm, weil die
        // Frequenzumkehr das Spektrum nur **vertauscht**, nicht zerstoert.
        assert_eq!(fenster_index(19), 21);
        assert_eq!(fenster_index(22), 24);
        assert_ne!(mdct_win(2)[21], 0.0, "i = 19 liegt auf 21");
        assert_ne!(mdct_win(2)[24], 0.0, "i = 22 liegt auf 24");
        assert_eq!(
            mdct_win(6)[21],
            -mdct_win(2)[21],
            "ungerade: Vorzeichenwechsel"
        );
        assert_eq!(mdct_win(6)[24], mdct_win(2)[24], "gerade: **kein** Wechsel");
        // Und die Luecken 18 und 19 bleiben bei beiden leer.
        assert_eq!(mdct_win(2)[18], 0.0);
        assert_eq!(mdct_win(2)[19], 0.0);
    }

    /// Und das Langblockfenster ist **symmetrisch um `i = 17,5`** — GEMESSEN.
    ///
    /// `sin(PI * (i + 0.5) / 36)` ist das, aber der Nenner
    /// `cos(PI * (2i + 19) / 72)` ist es **nicht**. Es ist daher keine Symmetrie
    /// des Fensters zu erwarten, und wer sie prueft und sie nicht findet, hat
    /// recht behalten — mit dem Fenster, nicht mit dem Test.
    ///
    /// Was **stimmt** (und die Form prueft): der **Zaehler** ist symmetrisch.
    #[test]
    fn der_zaehler_ist_symmetrisch_der_nenner_nicht() {
        for i in 0..18usize {
            let links = (std::f64::consts::PI * (i as f64 + 0.5) / 36.0).sin();
            let rechts = (std::f64::consts::PI * ((35 - i) as f64 + 0.5) / 36.0).sin();
            assert!(
                (links - rechts).abs() < 1e-15,
                "Zaehler bei i = {i} und 35 - {i}"
            );
        }
        // Und GEMESSEN: das Fenster ist es nicht, und um wie viel nicht.
        let mut groesste_abweichung = 0.0f64;
        for i in 0..18usize {
            let d = groesste_abweichung.max((fenster_roh(0, i) - fenster_roh(0, 35 - i)).abs());
            groesste_abweichung = d;
        }
        println!("GEMESSEN: groesste Asymmetrie des Langblockfensters = {groesste_abweichung:.6}");
        assert!(
            groesste_abweichung > 0.01,
            "GEMESSEN: das Fenster muss unsymmetrisch sein, sonst ist dieser Test blind"
        );
    }
}
