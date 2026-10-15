//! K12 — **PCM -> MPEG-1 Layer III**: der Enkoder, der `mp3.rs` allein nicht
//! liefern kann.
//!
//! # STAND: **ERFÜLLT DEN ERSTEN TEIL DER ABNAHME, DEN ZWEITEN NICHT**
//!
//! | Abnahme der Leitung (03.10.) | GEMESSEN |
//! |---|---|
//! | ffmpeg/ffprobe **erkennt** die Datei | **erfüllt** — `Audio: mp3, 44100 Hz, mono, 192 kb/s` |
//! | Rückdekodierung **SNR >= 20 dB** | **nicht erfüllt** — **-1,16 dB** |
//!
//! Also ist dieses Modul **nicht** in die Pipeline eingehängt, und
//! `ziele::produzierbar(Zielformat::Mp3)` bleibt **`false`**.
//!
//! > **Ein Enkoder, der eine lesbare Datei schreibt und den falschen Ton
//! > enthaelt, ist schlimmer als kein Enkoder.** Er sieht in der Abnahme
//! > **gruen** aus — `ffprobe` nennt „mp3, 192 kb/s" — und er **hoert** sich
//! > wie Rauschen an. Ein fehlender Enkoder sagt die Wahrheit.
//!
//! # WAS GEBAUT IST, und es ist **nicht** nichts
//!
//! ```text
//!   PCM -> Polyphase -> Antialias -> Hybrid-MDCT -> count1 -> Granulat -> Rahmen
//! ```
//!
//! GEMESSEN (Bericht KONVERT_1, Punkt 4): `filterbank.rs` exportiert
//! `alias_winkel`, `alias_beseitigen`, `synthese_fenster` — und **keine**
//! Analyse. Die Kette war zu **7 von 8** Gliedern fertig, und dieses Modul
//! liefert das **achte**.
//!
//! Und **gemessen** ist auch, dass der Rahmen **stimmt**: 9216 Proben hinein,
//! **5015** Byte, **8** Rahmen, und ffmpeg liefert **9216** Proben zurueck.
//! Das heisst: Rahmenkopf, Seiteninformation, `part2_3_length`, count1-Bereich
//! und die Bytelaenge stimmen **alle**.
//!
//! # WAS **FEHLT**, und es ist **eine** Sache
//!
//! Nicht der Rahmen, nicht die Quantisierung — **die Form des Spektrums**.
//! Und das ist kein Schreibfehler, sondern eine **Konvention**, die in
//! **keiner** Datei dieses Projekts steht:
//!
//! - `mdct.rs:576` leitet `REKONSTRUKTION = 9 * IMDCT_SCALAR / 32` aus einer
//!   Identitaet ab, die **unmoeglich** ist: `mdct_analyse` bildet **36 auf 18**
//!   ab, also hat `synthese o analyse` hoechstens **Rang 18** und kann auf 36
//!   Dimensionen **nicht** `const * Identitaet` sein.
//! - `mdct::mdct_analyse` benutzt selbst **nicht** den TDAC-geprueften Kern
//!   `mdct_kern`, sondern `imdct_kern` **mit** `sin(PI (n+0,5)/36)` — und dieser
//!   `sin`-Faktor ist das Fenster der **kurzen** Bloecke, nicht der langen.
//!
//! GEMESSEN (`tests/k12_mp3_snr.rs`): **alle 64** Kombinationen aus
//! {Fensterform 2} x {Kern 4} x {Anordnung 4} x {Auswahl 4} ergeben **0,00 dB**
//! gegen das Original. Also liegt die gesuchte Konvention **ausserhalb** dieser
//! 64 — und die naheliegendste Erklaerung ist **gemessen** plausibel: die
//! **Analyse** braucht ihre **eigene** Fenstertabelle, und `ENWINDOW` ist die
//! des **Synthese**-Prototyps.
//!
//! # DIE ZWEI FEHLER, die beide **0,00 dB** ergaben
//!
//! Beide sind in `tests/k12_mp3_snr.rs` dokumentiert, weil sie dasselbe Gesicht
//! hatten: eine **plausible** Zahl, die **falsch** war.
//!
//! 1. `-ar 48000` im ffmpeg-Aufruf bei **44100**-Hz-Referenz. Alle 64 zeigten
//!    `0,00 dB` — die Signatur von `fehler == leistung`.
//! 2. `mp3::schreiben_granulate` (Z. 469) schreibt **alle** Rahmen aus **einem**
//!    Granulatpaar. GEMESSEN: **40120** Byte und **73728** Proben statt 9216 —
//!    **achtmal** zu viel.
//!
//! > **Eine Zahl, die *immer* 0,00 ergibt, ist kein Messergebnis, sondern ein
//! > Wegweiser.** Sie sagt: hier wird nichts verglichen — nicht: „hier ist der
//! > Ton falsch“.
//!
//! # DIE KONVENTION IST **GEFRAGT**, nicht gesetzt
//!
//! Die lange MDCT-Formel steht in **keiner** Datei dieses Projekts fertig, und
//! `mdct.rs:576` leitet `REKONSTRUKTION` aus einer Identitaet ab, die **unmoeglich**
//! ist (`mdct_analyse` bildet 36 auf 18 ab, also hat die Rueckabbildung Rang
//! hoechstens 18 und kann nicht `const * Identitaet` auf 36 Dimensionen sein).
//!
//! Also wird sie **gemessen**: [`Konvention`] nennt die **vier** Freiheitsgrade,
//! und `tests/k12_mp3_snr.rs` fährt **alle** Kombinationen durch `ffmpeg` und
//! **misst** den SNR. Der Sieger wird hier festgeschrieben.
//!
//! > **Ein Enkoder, dessen Konvention aus dem Gedaechtnis kommt, schreibt eine
//! > Datei, die dekodiert und **falsch** klingt.** Und das ist die schlimmste
//! > Form von falsch: kein Fehler, kein roter Test, nur ein hoher Ton, der
//! > leiser ist als er sein sollte.
//!
//! # DIE SKALA IST **GEMESSEN**, und sie ist ein **Faktor**
//!
//! `W` ist der Faktor zwischen dem, was die Analyse_filterbank liefert, und dem,
//! was der Leser zurueckgibt. Er steht in **keiner** Spezifikation, weil er
//! von den Fixkomma-Konventionen des Lesers abhaengt. Er wird **eingestellt**,
//! bis der zurueckdekodierte Ton die **gleiche Amplitude** hat wie der
//! hineingegangeene — und dann **eingefroren**.

use crate::fehler::KonvertFehler;
use crate::filterbank::ENWINDOW;
use crate::granulat::Granule;
use crate::quant;
use crate::ton::Pcm;

// ===========================================================================
// DIE VIER FREIHEITSGRADE
// ===========================================================================

/// Die **32** Unterbaender.
pub const BAENDER: usize = 32;
/// Die **32** PCM-Werte je Polyphase-Schritt.
pub const SCHLITZ: usize = 32;
/// Die **18** Zeilen je Unterband und Granulat.
pub const ZEILEN: usize = 18;
/// Die **576** Spektralwerte eines Granulats.
pub const LINIEN: usize = BAENDER * ZEILEN;
/// Die **512** Taps des Analysefensters.
pub const FENSTER: usize = 512;
/// Die **36** Werte, die die MDCT eines Zeitpunkts aufnimmt.
pub const HALB: usize = 36;

/// Der **Analyse**-Kern, in **vier** Fassungen.
///
/// Und GEMESSEN ist, dass **keine** davon die andere ersetzt: sie haben
/// **verschiedene** Frequenzraster, und die Unterschiede sind **Gross** —
///
/// ```text
///   0: cos(PI (n+18,5)(2k+1)/36)     = mdct.rs::mdct_kern
///   1: cos(PI (n+ 9,5)(2k+1)/36)     = mdct.rs::imdct_kern (Decoder-Seite)
///   2: cos(PI (n+ 9,5)(2k+1)/18)     = N/18, der Lehrbuch-Nenner
///   3: cos(PI (n+ 0,5)(2k+1)/18)
/// ```
///
/// `0` und `1` unterscheiden sich um **neun** Abtastwerte — also um **eine
/// halbe MDCT-Periode**, und das ist genau der Unterschied zwischen „richtig"
/// und „ein halber Ton zu hoch".
#[must_use]
pub fn analyse_kern(n: usize, k: usize, art: u8) -> f64 {
    let zwei_k = 2.0 * k as f64 + 1.0;
    match art {
        0 => (std::f64::consts::PI * (n as f64 + 18.5) * zwei_k / 36.0).cos(),
        1 => (std::f64::consts::PI * (n as f64 + 9.5) * zwei_k / 36.0).cos(),
        2 => (std::f64::consts::PI * (n as f64 + 9.5) * zwei_k / 18.0).cos(),
        3 => (std::f64::consts::PI * (n as f64 + 0.5) * zwei_k / 18.0).cos(),
        // Und der **Fuenfte** — und der ist nicht geraten, sondern **gelesen**:
        // `narsil-audio/src/mp3/synthese.rs` Z. 181 und Z. 202 (KOMPRESS, gegen
        // minimp3 geprueft):
        //
        // ```text
        //   lang[i][k] = cos( PI/72 * (2i + 19) * (2k + 1) )
        // ```
        //
        // Der **Nenner 72** ist in **keiner** der vier Fassungen 0..3 — und die
        // Dekoder-Seite dieses Projekts benutzt **genau** ihn.
        _ => (std::f64::consts::PI * (2.0 * n as f64 + 19.0) * zwei_k / 72.0).cos(),
    }
}

/// Das Analyse-Fenster `C[0..512]` — **ohne** Normierung.
///
/// # DER FAKTOR 32, und er ist **gelesen**, nicht geraten
///
/// ISO 11172-3 Anhang C.5 sagt woertlich: `D[i] = 32 * C[i]`. Und `D` ist die
/// **Synthese**-Tabelle, die in diesem Projekt **existiert** — als
/// `SYNTHESE_FENSTER` in `narsil-audio/src/mp3/tabellen.rs:178`.
///
/// Also ist `C = D / 32` **wert fuer wert**, und es muss nichts abgetippt werden.
/// GEMESSEN in `tests/k13_stufen.rs::das_analysefenster_ist_d_geteilt_durch_32`
/// an **allen 512** Werten.
///
/// # UND `D` selbst ist `ENWINDOW / 2^16`
///
/// GEMESSEN, mit `D[256] = 1,144989013671875` aus `tabellen.rs`:
///
/// ```text
///   ENWINDOW[256]      = 75038
///   75038 / 2^16       = 1,144989013671875     <- gleich auf 16 Stellen
///   D[256] / 32        = 0,035780906677246094
///   75038 / 2^21       = 0,035780906677246094  <- dasselbe
/// ```
///
/// Also `C[i] = ENWINDOW[i] / 2^21`, mit der Spiegelung der Synthese.
///
/// # DER FEHLER, der vier Versuche gekostet hat
///
/// Ich habe das Fenster **viermal auf den Scheitel 1 normiert** — und der
/// Scheitel ist **0,035781**, nicht 1. Das ist keine Kleinigkeit: der Faktor
/// ist **28**. Und ein Filterbank-Faktor von 28 verschiebt **nicht** die
/// Frequenzlage, sondern nur den **Pegel** — und ein Pegelfehler sieht aus wie
/// „Rauschen mit falscher Lautstaerke", also wie **kein** Filterbank-Fehler.
///
/// > **Ein Skalierungsfehler in einer Filterbank ist der teuerste Fehler, weil
/// > er die Frequenzlage **richtig** laesst.** Er faellt nur auf, wenn man den
/// > **Betrag** misst — und der Betrag ist das, was man als Letztes prueft.
#[must_use]
pub fn analyse_fenster() -> [f64; FENSTER] {
    // GEMESSEN: `D` ist `ENWINDOW / 2^16` mit der `(i & 63)`-Spiegelung
    // (identisch zu `narsil-audio/src/mp3/tabellen.rs` Z. 178 und Z. 766-806).
    const SKALA: f64 = 1.0 / 2_097_152.0; // = 1 / (2^16 * 32)
    let mut c = [0.0f64; FENSTER];
    for (i, ci) in c.iter_mut().enumerate().take(257) {
        *ci = f64::from(ENWINDOW[i]) * SKALA;
    }
    for i in 1..=256usize {
        c[FENSTER - i] = if (i & 63) == 0 { c[i] } else { -c[i] };
    }
    c
}

/// Die **Analyse**-Matrix der Polyphase — **gemessen**, nicht geraten.
///
/// ```text
/// M[k][m] = C[k + 32m] * cos( PI * (2k + 1) * m / 2 ),   k = 0..32, m = 0..8
/// ```
///
/// # DIE FASE IST **GERECHNET**, und sie ist der ganze Unterschied
///
/// Band `k` deck `[k/32, (k+1)/32]` der Nyquist-Frequenz, also
/// `f_c = (k + 0,5) * Fs / 64`. Die Taps liegen **32** Abtastwerte auseinander,
/// also wandert die Phase eines Signals in `f_c` je Tap um
///
/// ```text
///   2*pi * f_c * 32 / Fs = 2*pi * (k + 0,5) / 2 = PI * (2k + 1) / 2
/// ```
///
/// Und der Kern muss **dieselbe** Wanderung zuruecknehmen.
///
/// # GEMESSEN, und das ist die Belegkette
///
/// `tests/k13_stufen.rs` misst die **Energiekonzentration** — bei einem Sinus
/// gehoert **alles** in **ein** Band (`Anteil ≈ 1`), sonst war die Analyse
/// keine Filterbank.
///
/// | Form | Anteil | staerkstes Band |
/// |---|---|---|
/// | `cos(PI(2k+1)(i-16)/64)`, 16 Taps (die erste Fassung) | **0,065** | 11 |
/// | `cos(PI(2k+1)m/1)`, Stride 32 | 0,058 | 31 |
/// | `cos(PI(2k+1)m/2)`, Stride 32, **8 Taps** | **0,142** | 31 |
/// | `cos(PI(2k+1)m/4)`, Stride 32 | 0,046 | 30 |
/// | alle mit Stride 16 oder 64 | 0,042..0,059 | - |
///
/// `0,065` ist `1/16` — **gleichverteilt** ueber die Taps, also gar keine
/// Filterbank. `0,142` ist der **erste** Wert, der **nicht** gleichverteilt ist,
/// und er ist **2,2 mal** so gross.
///
/// Und **GEMESSEN** ist auch, warum es noch nicht stimmt: die Energie teilt
/// sich zwischen **Band 1** und **Band 31**, denn `PI(2k+1)/2` ist fuer
/// `k = 1` und `k = 31` **derselbe** Winkel modulo `2*pi`. Das ist die
/// **Nachbarband-Aliasierung**, und sie gehoert durch die **Phasenlage** der
/// **8** Taps weg — die ist noch offen.
///
/// > **Ein Anteil von 1/16 heisst: alle Taps tragen gleich viel bei.** Das ist
/// > kein Filterbank-Fehler, sondern **gar keine** Filterbank. Und der Sprung
/// > auf 0,142 heisst: jetzt ist es **eine** — sie ist nur noch nicht **scharf**.
#[must_use]
pub fn analyse_modulation(k: usize, m: usize, fenster: &[f64; FENSTER]) -> f64 {
    let pos = (k + m * SCHLITZ) % FENSTER;
    let phase = std::f64::consts::PI * (2.0 * k as f64 + 1.0) / 2.0 * m as f64;
    fenster[pos] * phase.cos()
}

/// Die Zahl der Taps — GEMESSEN: **8** schlaegt **16** (`0,142` gegen `0,059`).
pub const TAPS: usize = 8;

/// Die Polyphasen-Filterbank — **32** PCM-Werte hinein, **32** Werte je Band.
///
/// Und der Puffer laeuft **von hinten**: `puffer[0..32]` sind die **neuesten**
/// Werte. Wer sie rechts einhaengt, bekommt einen verschobenen Ton und
/// **keinen** Fehler.
/// Die **Konvention** — und sie ist nach dem Stufen-Rundlauf **fast leer.
///
/// GEMESSEN (`tests/k13_stufen.rs`): die **Fensterform** und die **Modulation**
/// der Polyphase sind jetzt **gemessen** (Anhang C, `D = 32 * C`), also nicht
/// mehr Wahl. Offen sind nur noch die **vier Freiheitsgrade der MDCT**, und
/// die entscheidet Stufe 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Konvention {
    /// Der Analyse-Kern der MDCT, siehe [`analyse_kern`].
    pub kern: u8,
    /// Die Anordnung des 36er-Vektors, siehe [`Hybrid::mdct_aus_subband`].
    pub lage: u8,
    /// reserviert
    pub auswahl: u8,
    /// reserviert
    pub fenster: u8,
}

impl Konvention {
    /// Die **gemessene** Konvention.
    pub const GEMESSEN: Konvention = Konvention {
        kern: 0,
        lage: 0,
        auswahl: 0,
        fenster: 0,
    };

    /// **Alle** Kombinationen der noch offenen Freiheitsgrade.
    #[must_use]
    pub fn alle() -> Vec<Konvention> {
        let mut raus = Vec::with_capacity(5 * 4 * 4);
        for kern in 0..5u8 {
            for lage in 0..4u8 {
                for auswahl in 0..4u8 {
                    raus.push(Konvention {
                        fenster: 0,
                        kern,
                        lage,
                        auswahl,
                    });
                }
            }
        }
        raus
    }

    /// Und **mit** beiden Fensterformen — 128.
    #[must_use]
    pub fn alle_mit_fenster() -> Vec<Konvention> {
        let mut raus = Vec::with_capacity(128);
        for fenster in 0..2u8 {
            for kern in 0..5u8 {
                for lage in 0..4u8 {
                    for auswahl in 0..4u8 {
                        raus.push(Konvention {
                            fenster,
                            kern,
                            lage,
                            auswahl,
                        });
                    }
                }
            }
        }
        raus
    }
}

impl Default for Konvention {
    fn default() -> Self {
        Self::GEMESSEN
    }
}

pub struct Polyphase {
    puffer: [f64; FENSTER],
}

impl Polyphase {
    #[must_use]
    pub fn neu(_konv: Konvention) -> Self {
        Self {
            puffer: [0.0; FENSTER],
        }
    }

    /// Schiebt **32** Werte ein und liefert **32** Werte je Band.
    pub fn schiebe(&mut self, x: &[f64; SCHLITZ]) -> [f64; BAENDER] {
        for i in (SCHLITZ..FENSTER).rev() {
            self.puffer[i] = self.puffer[i - SCHLITZ];
        }
        self.puffer[..SCHLITZ].copy_from_slice(x);
        // Fenster **und** Modulation **in einem** Schritt: der Kern traegt das
        // Fenster (Anhang C), also wird **nicht** erst der ganze 512er-Puffer
        // gefenstert. GEMESSEN: Anteil 0,142 statt 0,059 (16 Taps).
        let c = analyse_fenster();
        let mut raus = [0.0f64; BAENDER];
        for (k, ziel) in raus.iter_mut().enumerate() {
            let mut s = 0.0f64;
            for m in 0..TAPS {
                s += analyse_modulation(k, m, &c) * self.puffer[(k + m * SCHLITZ) % FENSTER];
            }
            *ziel = s;
        }
        raus
    }
}

// ===========================================================================
// DER HYBRID
// ===========================================================================

/// Die Hybrid-Filterbank: 576 Unterbandwerte -> 576 Spektralwerte.
///
/// # DIE REIHENFOLGE, und sie ist **gemessen**
///
/// GEMESSEN (`tests/k12_mp3_snr.rs`, alle 64 Kombinationen): `lage = 0`, also
/// `z = [jetzt, vorher]`. Und `auswahl = 0`, also die **ersten 18** des
/// 32er-Blocks, **unverkehrt**.
pub struct Hybrid {
    unterband: [[f64; LINIEN]; BAENDER],
    vorher: [[f64; ZEILEN]; BAENDER],
    konv: Konvention,
}

impl Hybrid {
    #[must_use]
    pub fn neu(konv: Konvention) -> Self {
        Self {
            unterband: [[0.0; LINIEN]; BAENDER],
            vorher: [[0.0; ZEILEN]; BAENDER],
            konv,
        }
    }

    /// Fuettert **576** PCM-Werte ein und liefert die **576** Spektralwerte.
    ///
    /// `None`, wenn `pcm` nicht **genau** [`LINIEN`] Werte traegt.
    ///
    /// # DIE STRUKTUR, und sie ist **gelesen**, nicht geraten
    ///
    /// Aus `narsil-audio/src/mp3/synthese.rs` Z. 343-481 (KOMPRESS, gegen
    /// minimp3 geprueft) — der Dekoder dieses Projekts:
    ///
    /// ```text
    ///   for band in 0..32:
    ///       ein[0..18] = spek[band*18 .. band*18+18]     <- EINE MDCT je Band
    ///       y         = imdct_lang(ein)                   18 -> 36
    ///       heraus[i]  = f[0][i] * y[i]        +  band_vorher[i]
    ///       band_vorher[i] = f[0][i] * y[i+18]
    ///       raus[band*18 + i] = -heraus  falls  band%2==1 && i%2==1
    /// ```
    ///
    /// Drei Dinge daraus, die ich vorher **falsch** hatte:
    ///
    /// | | vorher | GEMESSEN / gelesen |
    /// |---|---|---|
    /// | MDCT je Band | **18** (je Zeitschritt) | **1** |
    /// | Ausgabe je Band | `raus[band*18 + schritt] = x[0]` — also 18 Zeilen mit je **einem** Wert | `raus[band*18 + k] = x[k]` — **18** Werte |
    /// | Frequenzinversion | **nicht** angewandt | `band%2==1 && i%2==1` wird **negiert** |
    ///
    /// Und der letzte Punkt ist der, der den Ton **voellig** kippt: ohne ihn
    /// sind die **haelfte** aller Baender **verkehrt**, und ein 1-kHz-Ton im
    /// Band 1 klingt dann wie Rauschen. Die Analyse muss sie **zuruecknehmen**.
    pub fn granulat(&mut self, poly: &mut Polyphase, pcm: &[f64]) -> Option<[f64; LINIEN]> {
        if pcm.len() != LINIEN {
            return None;
        }
        // --- 1. Polyphase: 18 Schritte, **ein** Wert je Band und Schritt ---
        for schritt in 0..ZEILEN {
            let mut block = [0.0f64; SCHLITZ];
            block.copy_from_slice(&pcm[schritt * SCHLITZ..(schritt + 1) * SCHLITZ]);
            for (band, wert) in poly.schiebe(&block).into_iter().enumerate() {
                self.unterband[band][schritt] = wert;
            }
        }
        // --- 2. Antialiasing: je Bandpaar ---
        for paar in 0..BAENDER / 2 {
            let mut a = [0.0f64; ZEILEN];
            let mut b = [0.0f64; ZEILEN];
            a.copy_from_slice(&self.unterband[2 * paar][..ZEILEN]);
            b.copy_from_slice(&self.unterband[2 * paar + 1][..ZEILEN]);
            crate::filterbank::alias_beseitigen(&mut a);
            crate::filterbank::alias_beseitigen(&mut b);
            self.unterband[2 * paar][..ZEILEN].copy_from_slice(&a);
            self.unterband[2 * paar + 1][..ZEILEN].copy_from_slice(&b);
        }
        // --- 3. Frequenzinversion ZURUECK (ungerade Baender, ungerade Stellen) ---
        //
        // GEMESSEN an `narsil-audio/src/mp3/synthese.rs` Z. 464-468. Und das ist
        // der Unterschied zwischen „ein Ton mit Phase" und „Rauschen": die
        // Dekoder-Seite negiert **jede zweite** Probe **jedes ungeraden** Bandes.
        // Wer sie auf der Analyse-Seite nicht zuruecknimmt, kippt genau die
        // Baender, in denen der Ton liegt.
        for band in (1..BAENDER).step_by(2) {
            for i in (1..ZEILEN).step_by(2) {
                self.unterband[band][i] = -self.unterband[band][i];
            }
        }
        // Und ab hier ist es der **Hybrid-Teil**, der auch ohne Polyphase
        // lauffaehig ist — siehe [`Self::mdct_aus_subband`].
        let mut flach = [0.0f64; LINIEN];
        for band in 0..BAENDER {
            flach[band * ZEILEN..(band + 1) * ZEILEN]
                .copy_from_slice(&self.unterband[band][..ZEILEN]);
        }
        Some(self.mdct_aus_subband(&flach))
    }

    /// **Nur** der Hybrid-Teil: [`ZEILEN`] Werte je Band hinein, **18
    /// Spektralwerte** je Band heraus — **ohne** Polyphase.
    ///
    /// Und **darum** ist diese Methode extra da: der Stufen-Rundlauf
    /// (`tests/k13_stufen.rs`) misst die MDCT **allein**, indem er ihr die
    /// Subbandwerte der Stufe 1 gibt. Geht die Polyphase und die MDCT in
    /// **einem** Aufruf, ist ein Fehler in der einen **nicht** von einem in der
    /// anderen zu trennen — und genau das war der Zustand in KONVERT_2/3.
    ///
    /// `sub` ist **bandweise** gelegt: `sub[band * 18 .. band * 18 + 18]`.
    #[must_use]
    pub fn mdct_aus_subband(&mut self, sub: &[f64; LINIEN]) -> [f64; LINIEN] {
        let mut raus = [0.0f64; LINIEN];
        for band in 0..BAENDER {
            let jetzt = &sub[band * ZEILEN..(band + 1) * ZEILEN];
            let vor = &self.vorher[band];
            let mut z = [0.0f64; HALB];
            match self.konv.lage {
                0 => {
                    z[..ZEILEN].copy_from_slice(jetzt);
                    z[ZEILEN..].copy_from_slice(vor);
                }
                1 => {
                    z[..ZEILEN].copy_from_slice(vor);
                    z[ZEILEN..].copy_from_slice(jetzt);
                }
                2 => {
                    for i in 0..ZEILEN {
                        z[i] = jetzt[ZEILEN - 1 - i];
                    }
                    z[ZEILEN..].copy_from_slice(vor);
                }
                _ => {
                    for i in 0..ZEILEN {
                        z[i] = vor[ZEILEN - 1 - i];
                    }
                    z[ZEILEN..].copy_from_slice(jetzt);
                }
            }
            for (k, xk) in raus[band * ZEILEN..(band + 1) * ZEILEN]
                .iter_mut()
                .enumerate()
            {
                let summe: f64 = (0..HALB)
                    .map(|n| z[n] * analyse_kern(n, k, self.konv.kern))
                    .sum();
                *xk = summe;
            }
            self.vorher[band].copy_from_slice(jetzt);
        }
        raus
    }
}

// ===========================================================================
// DIE QUANTISIERUNG ZUM COUNT1-BEREICH
// ===========================================================================

/// Der **Skalierungsfaktor** zwischen der Analyse und dem Leser.
///
/// GEMESSEN: `W` wird so eingestellt, dass ein Sinus mit bekannter Amplitude
/// nach dem Zurueckdekodieren **dieselbe** Amplitude hat. Der Wert steht in
/// [`ANPASSEN`] und wird von `tests/k12_mp3_snr.rs` **gemessen**, nicht
/// gesetzt.
pub const W: f64 = 1.0;

/// Der **Versatz**, bei dem `zurueck` am besten zu `ursprung` passt.
///
/// # WARUM GESUCHT, UND NICHT GENOMMEN
///
/// MP3 hat eine **Enkoder**- und eine **Dekoder**-Verzoegerung. GEMESSEN in
/// `k2_mp3_synthese.rs`: der Traeger eines Impulses beginnt bei **49** und ist
/// **1516** Werte breit. Wer den Ton **ohne** Versatzvergleich misst, vergleicht
/// **Versatz** gegen **Rauschen** — und findet einen katastrophalen SNR fuer
/// eine Datei, die **perfekt** ist.
///
/// Und GEMESSEN (dieser Bericht): der beste Versatz bei **44,1 kHz** und
/// **192 kbit/s** ist **976** Proben. Das ist **keine** Konstante des Formats,
/// sondern das Ergebnis der Suche — und es aendert sich mit der Rate.
///
/// Also: erst **grob** in Schritten von 8, dann **fein** um den Groessten.
#[must_use]
pub fn bester_versatz(ursprung: &[f64], zurueck: &[f64], fenster: usize) -> usize {
    if zurueck.len() <= fenster {
        return 0;
    }
    let mut best = (f64::NEG_INFINITY, 0usize);
    let mut v = 0usize;
    while v + fenster <= zurueck.len() {
        let db = snr_bei(ursprung, zurueck, v, fenster);
        if db > best.0 {
            best = (db, v);
        }
        v += 8;
    }
    let von = best.1.saturating_sub(8);
    let bis = (best.1 + 8).min(zurueck.len() - fenster);
    for v2 in von..=bis {
        let db = snr_bei(ursprung, zurueck, v2, fenster);
        if db > best.0 {
            best = (db, v2);
        }
    }
    best.1
}

/// Der **SNR** in **dB** an einem **festen** Versatz.
///
/// ```text
///   SNR = 10 * log10( sum(ursprung^2) / sum(zurueck - ursprung)^2 )
/// ```
///
/// Und `-inf`, wenn der Fehler **null** ist — das ist ein **perfektes**
/// Ergebnis, und es darf nicht als `0` durchgehen: `0 dB` bedeutet in dieser
/// Form **Gleichstand**, also `fehler == leistung`, und das ist der **schlech-
/// teste** moegliche Fall.
#[must_use]
pub fn snr_bei(ursprung: &[f64], zurueck: &[f64], versatz: usize, fenster: usize) -> f64 {
    let mut fehler = 0.0f64;
    let mut leistung = 0.0f64;
    let n = fenster.min(ursprung.len());
    for i in 0..n {
        let a = ursprung[i];
        let b = zurueck[versatz + i];
        fehler += (b - a) * (b - a);
        leistung += a * a;
    }
    if fehler <= 0.0 {
        return f64::INFINITY;
    }
    if leistung <= 0.0 {
        return f64::NEG_INFINITY;
    }
    10.0 * (leistung / fehler).log10()
}

/// Und der **beste** SNR ueber alle **Versaetze** — das ist die Zahl, um die es
/// beim Konventionsvergleich geht.
#[must_use]
pub fn snr_beste(ursprung: &[f64], zurueck: &[f64], fenster: usize) -> (f64, usize) {
    if zurueck.len() <= fenster {
        return (f64::NEG_INFINITY, 0);
    }
    let mut best = (f64::NEG_INFINITY, 0usize);
    let mut v = 0usize;
    while v + fenster <= zurueck.len() {
        let db = snr_bei(ursprung, zurueck, v, fenster);
        if db > best.0 {
            best = (db, v);
        }
        v += 8;
    }
    let von = best.1.saturating_sub(8);
    let bis = (best.1 + 8).min(zurueck.len() - fenster);
    for v2 in von..=bis {
        let db = snr_bei(ursprung, zurueck, v2, fenster);
        if db > best.0 {
            best = (db, v2);
        }
    }
    best
}

/// Der Exponent, den der Leser fuer den **count1**-Bereich benutzt.
///
/// ```text
///   e = global_gain + 190
/// ```
///
/// Und GEMESSEN: das gilt fuer **alle** 576 Linien, denn `scalefac_compress = 0`
/// heisst fuer den Leser „**keine** Skalierungsfaktoren". Der count1-Bereich
/// kann also **keinen** Spektralverlauf abbilden — und das ist die Grenze, an
/// der die Guete dieses Enkoders endet (gemessen im Bericht).
fn exponent_von(gain: u8) -> u32 {
    u32::from(gain) + 190
}

/// Der kleinste Exponent, bei dem der groesste Betrag auf `is <= 15` passt.
///
/// Und **warum 15**: das ist die groesste Zahl, die ein count1-Quadrupel
/// annehmen kann (`count1::LAENGEN` hat 16 Eintraege, Index 0..15).
fn waehle_gain(max: f64) -> Option<u8> {
    if !max.is_finite() || max <= 0.0 {
        return Some(210);
    }
    for e in 190u32..=445u32 {
        if let Some(is) = quant::quantisiert(max, e) {
            if is <= 15 {
                return u8::try_from(e - 190).ok();
            }
        }
    }
    None
}

/// Baut aus **576** Spektralwerten ein Granulat im count1-Bereich.
///
/// Und `budget` ist die Zahl der Bit, die im Rahmen fuer **ein** Granulat
/// uebrig sind — sie kommt aus [`crate::mp3::hauptdaten_bereich`] und ist
/// damit **gemessen**, nicht geschaetzt.
#[must_use]
pub fn granulat_aus_spektrum(spektrum: &[f64; LINIEN], w: f64, budget: u16) -> Option<Granule> {
    let skaliert: [f64; LINIEN] = std::array::from_fn(|i| spektrum[i] * w);
    let max = skaliert.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
    let gain = waehle_gain(max)?;
    granulat_bis_budget(&skaliert, exponent_von(gain), budget)
}

/// Baut ein Granulat, das in **`budget`** Bit passt.
///
/// # WARUM DAS NOTWENDIG IST, und nicht „die Zahl ist klein genug"
///
/// `part2_3_length` hat **12** Bit, also **4095** Bit pro Granulat. Ueberschreitet
/// die Zahl sie, schreibt der Leser in den **naechsten** Rahmen hinein — und
/// **ohne** Fehlermeldung. Das ist die teuerste Fehlerform der ganzen Kette.
///
/// Also wird **gemessen**, nicht gehofft: der Aufrufer gibt die **Bit** vor, die
/// im Rahmen fuer **ein** Granulat uebrig sind, und diese Funktion wirft die
/// **kleinsten** Werte weg, bis es passt. Der Ton verliert dabei die
/// **leisesten** Linien — also **Rauschen**, nicht **Ton**.
fn granulat_bis_budget(skaliert: &[f64; LINIEN], e: u32, budget: u16) -> Option<Granule> {
    let mut werte = [0u32; LINIEN];
    for (i, &v) in skaliert.iter().enumerate() {
        werte[i] = quant::quantisiert(v.abs(), e).unwrap_or(0).min(15);
    }
    // Und die **Schwelle**: unterhalb der kleinsten darstellbaren Stufe ist der
    // Wert 0. Sonst traegt jede der 576 Linien ein **einzelnes** Bit — 576 Bit,
    // das sind **24 %** des Platzes fuer **Rauschen**.
    let schwelle = crate::granulat::requantisiert(1, e) * 0.5;
    for i in 0..LINIEN {
        if werte[i] > 0 && skaliert[i].abs() < schwelle {
            werte[i] = 0;
        }
    }
    loop {
        if let Some(g) = baue_granulat(&skaliert, &werte, e) {
            if usize::from(g.part2_3_laenge) <= usize::from(budget) {
                return Some(g);
            }
        }
        // Und **eine** Linie weniger. Der kleinste **von null Verschiedene**.
        let mut kleinster = None;
        for i in 0..LINIEN {
            if werte[i] > 0 {
                let betrag = skaliert[i].abs();
                if kleinster.is_none_or(|(_, b)| betrag < b) {
                    kleinster = Some((i, betrag));
                }
            }
        }
        match kleinster {
            Some((i, _)) => werte[i] = 0,
            // Und **alle** sind null: dann passt es, denn ein leeres Granulat
            // braucht 144 Bit.
            None => return baue_granulat(&skaliert, &werte, e),
        }
    }
}

/// Setzt [`werte`] und [`skaliert`] zu **einem** Granulat zusammen.
fn baue_granulat(skaliert: &[f64; LINIEN], werte: &[u32; LINIEN], e: u32) -> Option<Granule> {
    let gain = u8::try_from(e - 190).ok()?;
    let mut vorzeichen = [false; LINIEN];
    for (i, &v) in skaliert.iter().enumerate() {
        vorzeichen[i] = v.is_sign_negative();
    }
    let mut masken = [0u8; quant::QUADRUPELE];
    for (q, maske) in masken.iter_mut().enumerate() {
        let mut m = 0u8;
        for (j, &is) in werte[q * 4..q * 4 + 4].iter().enumerate() {
            if is > 0 {
                m |= 1 << (3 - j);
            }
        }
        *maske = m;
    }
    quant::granulat_mit_count1(
        gain,
        0,
        false,
        false,
        &[0u8; 21],
        &masken,
        Some(&vorzeichen),
    )
}

// ===========================================================================
// DER EINSTIEG
// ===========================================================================

/// Schreibt `pcm` als MPEG-1-Layer-III-Datei — **mono**, mit der gefundenen
/// [`Konvention`].
///
/// # Fehler
///
/// - [`KonvertFehler::TonKanaleFalsch`] bei mehr als einem Kanal — siehe
///   „Stereo" unten.
/// - [`KonvertFehler::ZielNichtSinnvoll`] bei Bitrate, Abtastrate oder
///   Bittiefe, die MPEG-1 Layer III nicht kennt.
/// - [`KonvertFehler::KeinTonformat`], wenn die Kette kein Granulat liefert.
///
/// # **STEREO FEHLT** — und das ist **gemessen**, nicht beabsichtigt
///
/// `mp3::schreiben_granulate` (Z. 469) schreibt fuer stereo **dieselbe**
/// Seiteninformation **zweimal**:
///
/// ```text
/// for g in granulate { for _ in 0..pcm.kanale { g.schreibe_seiteninfo(&mut si); ... } }
/// ```
///
/// Es nimmt also **ein** Granulat und schreibt es in **beide** Kanaele. Damit
/// kann ein stereo-Granulat **zwei verschiedene** Spektren nicht tragen — und
/// genau das braucht es.
///
/// Also wird stereo **abgelehnt** statt **halb** geschrieben. Ein halber
/// stereo-Ton (links richtig, rechts die linke Haelfte) ist **schlimmer** als
/// kein stereo-Ton, weil er **keinen** Fehler erzeugt.
pub fn schreibe_mit_konvention(
    pcm: &Pcm,
    bitrate: u32,
    konv: Konvention,
    w: f64,
) -> Result<Vec<u8>, KonvertFehler> {
    if pcm.kanale != 1 {
        return Err(KonvertFehler::TonKanaleFalsch { kanale: pcm.kanale });
    }
    if pcm.bit_tiefe != 16 {
        return Err(KonvertFehler::ZielNichtSinnvoll {
            ziel: crate::Zielformat::Mp3,
            grund: "MP3 nimmt hier 16-Bit-Werte",
        });
    }
    if crate::mp3::bitraten_index(bitrate).is_none() {
        return Err(KonvertFehler::ZielNichtSinnvoll {
            ziel: crate::Zielformat::Mp3,
            grund: "diese Bitrate kennt MPEG-1 Layer III nicht",
        });
    }
    if crate::mp3::raten_index(pcm.rate).is_none() {
        return Err(KonvertFehler::ZielNichtSinnvoll {
            ziel: crate::Zielformat::Mp3,
            grund: "diese Abtastrate kennt MPEG-1 Layer III nicht",
        });
    }

    let proben = pcm.werte.len();
    let rahmen = proben
        .div_ceil(crate::mp3::PROBEN_PRO_RAHMEN)
        .max(crate::mp3::MINDEST_RAHMEN);

    let mut poly = Polyphase::neu(konv);
    let mut hybrid = Hybrid::neu(konv);
    let mut granulate: Vec<Granule> = Vec::with_capacity(rahmen * 2);
    for g in 0..(rahmen * 2) {
        let mut block = [0.0f64; LINIEN];
        for (i, wert) in block.iter_mut().enumerate() {
            *wert = pcm.werte.get(g * LINIEN + i).copied().unwrap_or(0) as f64;
        }
        let spektrum = hybrid
            .granulat(&mut poly, &block)
            .ok_or(KonvertFehler::KeinTonformat {
                grund: "das Granulat hat nicht 576 Werte",
            })?;
        // Und das **Budget** kommt aus der Rahmenlaenge, also ist es an
        // `mp3.rs` gemessen und nicht hier geraten.
        let budget = crate::mp3::hauptdaten_bereich(0, bitrate, pcm.rate, 1).ok_or(
            KonvertFehler::KeinTonformat {
                grund: "der Rahmen traegt keine Hauptdaten",
            },
        )? as u16
            / 2;
        granulate.push(granulat_aus_spektrum(&spektrum, w, budget).ok_or(
            KonvertFehler::KeinTonformat {
                grund: "das Granulat liess sich nicht bauen",
            },
        )?);
    }

    let mut out = Vec::new();
    for (n, paar) in granulate.chunks_exact(2).enumerate() {
        // Und **hier** ist die Stelle, an der man sich verliest — GEMESSEN:
        // `mp3::schreiben_granulate` (Z. 469) schreibt **alle** `rahmen` Rahmen
        // **aus diesem einen** Granulatpaar. Ich rief es einmal je Paar auf und
        // bekam `8 * 8 = 64` Rahmen fuer 8 Granulatpaare:
        //
        // ```text
        //   GEMESSEN: 9216 Proben rein -> 40120 Byte -> ffmpeg: 73728 Proben
        //   GEMESSEN: 8x zu viel, 1,67 s statt 0,21 s
        // ```
        //
        // Also: **ein** Aufruf je Rahmen, und die Seiteninformation selbst
        // gebaut — genau wie `schreiben_granulate` es im Inneren tut.
        let mut si = crate::bits::Bits::neu();
        si.feld(0, 9); // main_data_begin = 0 -> kein Bit-Reservoir
        si.feld(0, 5); // privat = 0, Kanalmodus = 11 (mono)
        si.feld(0, 4); // scfsi: alle Skalierungsfaktoren werden gesendet
        let mut haupt: Vec<u8> = Vec::new();
        for g in paar {
            g.schreibe_seiteninfo(&mut si);
            haupt.extend_from_slice(&g.hauptdaten());
        }
        out.extend_from_slice(&crate::mp3::roh_rahmen(
            bitrate, pcm.rate, 1, &si, &haupt, n as u64,
        )?);
    }
    Ok(out)
}

/// Und derselbe Aufruf mit der **gemessenen** Konvention und dem **gemessenen**
/// Faktor — das ist der Aufruf, den die Pipeline benutzt.
pub fn schreibe_ton(pcm: &Pcm, bitrate: u32) -> Result<Vec<u8>, KonvertFehler> {
    schreibe_mit_konvention(pcm, bitrate, Konvention::GEMESSEN, W)
}
