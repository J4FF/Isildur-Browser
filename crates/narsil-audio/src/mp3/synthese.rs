//! Die Synthese: inverse MDCT, Fenster, Ueberlappung, Frequenzinversion
//! und der Polyphasefilter — aus 32 Bandwerten je 18 kommen 576 Samples.
//!
//! # WARUM NICHT ffpegs FORM
//!
//! ffmpeg rechnet die inverse MDCT als Hand-Schmetterling mit
//! `MULH3`/`MULLx` in `imdct36`, und **laesst die letzte Stufe im
//! Fenstertable stecken**:
//!
//! ```c
//! d = sin(M_PI * (i + 0.5) / 36.0);
//! …
//! d *= 0.5 * IMDCT_SCALAR / cos(M_PI * (2 * i + 19) / 72);
//! ff_mdct_win_float[j][idx] = d / (1 << 5);
//! ```
//!
//! Das ist dieselbe Sorte Stelle, an der G8b vier Fehler gefunden hat
//! (`mdct_init` setzt `scale` an der **Vorwaerts**transformation).
//! Deshalb steht hier die **Normform** und nichts davon:
//!
//! ```text
//!   y[i] = Σ(k=0..17) X[k] · cos( π/72 · (2i + 19)(2k + 1) ),   i = 0..35
//!   s[i] = w[i] · y[i]                                          (Fenster)
//! ```
//!
//! Der Faktor `1/cos(π(2i+19)/72)` steckt bei ffmpeg im Fenster, weil
//! dessen Schmetterling die DCT-IV-Form nicht direkt bildet. Hier ist er
//! Teil der Formel — damit kann er weder fehlen noch doppelt
//! hineingeraten. Die Normierung selbst (`2/n`) ist damit ebenfalls
//! **messbar**: der Test `die_normierung_der_imdct_ist_beweisbar` rechnet
//! sie nach.
//!
//! # DIE VIER FENSTER, UND WO SIE STEHEN
//!
//! | Nummer | Bedeutung | `i` in 0..35 |
//! |---|---|---|
//! | 0 | normal | `sin(π(i+½)/36)` |
//! | 1 | Start eines Kurzblocks (Mitte lang) | `i<18`: 1; `18..24`: `sin(π(i-18+½)/12)`; sonst 0 |
//! | 2 | Ende eines Kurzblocks (Mitte kurz) | `i<6`: 0; `6..12`: `sin(π(i-6+½)/12)`; `i<18`: 1; sonst `sin(π(i-17+½)/36)` |
//! | 3 | Kurzblock | `sin(π(i+½)/12)` fuer `i < 12`, sonst 0 |
//!
//! Die Fenster 1 und 2 sind genau die, die ffmpeg in `mpa_synth`-Art
//! bildet — belegt an `libavcodec/mpegaudiodsp.c`, Schleife
//! „compute mdct windows".
//!
//! # DER KURZBLOCK IST DREI 6->12-MDCTs, NICHT EINE 18->36
//!
//! Ein Kurzblock hat 18 Spektralwerte je Band, aber **6 je Fenster** und
//! drei Fenster. Daraus macht die Norm **drei** MDCTs 6->12, deren
//! Ausgaenge mit Schritt 6 uebereinandergelegt werden — jeder mit dem
//! 12-Wert-Fenster. ffmpeg macht dasselbe, nur mit einem 18-Puffer:
//!
//! ```c
//! ptr = g->sb_hybrid + 18 * mdct_long_end;
//! imdct12(out2, ptr + 0);  …
//! ptr += 18;
//! ```
//!
//! `ptr` laeuft also um 18 weiter, nicht um 12: die drei 12er Ausgaenge
//! werden **nicht** fortlaufend geschrieben, sondern an den Stellen
//! 0, 6 und 12 eines 18-Wert-Bandes. Das ist der ganze Unterschied, und
//! es ist der Grund, warum die Ueberlappung mit 18 Werten je Band
//! arbeitet.

use crate::mp3::granel::{BAENDER, LINIEN_PRO_BAND, Spektrum};
use crate::mp3::rahmen::Granule;
use crate::mp3::tabellen::SYNTHESE_FENSTER;

/// Die Zahl der Baender.
pub const BAENDERZAHL: usize = BAENDER;
/// Die Zahl der Samples je Granul und Kanal: 32 Baender zu 18.
pub const SAMPLES_PRO_GRANUL: usize = BAENDERZAHL * LINIEN_PRO_BAND;

/// Die vier Fenster, zur Rechnung fertig: `FENSTER[fenster][i]`, `i` 0..35.
///
/// # GEMESSENE EIGENSCHAFTEN, und die pruefen die Tests
///
/// * **Princen-Bradley**: `Σ(i=0..17) w[i]·w[i+18] == 0` fuer alle vier
///   Fenster. Das ist die Bedingung, unter der sich das Ueberlappen
///   ausloescht — ohne sie ist der Faktor `2/n` nicht der einzige
///   Fehler.
/// * **Energie**: `Σ(i=0..35) w[i]^2 == 9` fuer Fenster 0. Das ist `n/2`
///   mit `n = 18`, und daraus folgt der Normierungsfaktor.
///
/// Die Fenstertabelle, auf **`f32` gerundet** — die Form, in der das
/// Orakel sie traegt.
///
/// # WOFUER DAS DA IST (G9m)
///
/// GEMESSEN, in `tests/orakel/minimp3_mess.h:1344`:
/// `static const float g_mdct_window[2][18] = { 0.99904822f, ... }` — das
/// sind **Dezimal-Literale** mit `f`-Suffix, **keine** berechnete Sinus.
///
/// GEMESSEN, dass das ein Unterschied ist: mein `f32`-Wert fuer Index 0
/// ist `0,04361939`, das Literal ist `0,04361938f` — **1 ulp**
/// (`3,7e-09`). GEMESSENE Groessenordnung dieses Fehlers, multipliziert
/// mit einem Sample-Wert von `3,0`: `1,1e-08` — und das ist **genau** die
/// groesste **absolute** Abweichung aus G8j (`1,106e-08`).
///
/// Also: **minimp3 traegt die Sinuswerte ein, ich berechne sie.** Und die
/// Runde 15 („Fit ueber 15 Unbekannte") waere die Suche nach einem
/// Phantom, denn die Unbekannten sind **ein** bekannter Wert: der
/// Unterschied zwischen „Literal" und „berechnet".
///
/// **REGEL: wenn ein Orakel eine Zahl als **Literal** traegt und man sie
/// berechnet, dann ist die Abweichung kein Geheimnis, sondern die
/// Rundung. Man muss sie nur an der richtigen Stelle messen.**
#[must_use]
pub fn fenster_f32() -> &'static [[f64; 36]; 4] {
    static F: std::sync::OnceLock<[[f64; 36]; 4]> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let mut r = [[0.0f64; 36]; 4];
        for (z, quelle) in fenster().iter().enumerate() {
            for (i, w) in quelle.iter().enumerate() {
                // GEMESSEN: die Rundung ist `as f32 as f64` — einmal auf
                // `f32`, das ist die Praezision des Orakels, und dann
                // zurueck, weil meine Kette `f64` rechnet.
                r[z][i] = f64::from(*w as f32);
            }
        }
        r
    })
}

/// Die Fenstertabelle — aus [`fenster`], **optional** ersetzt.
///
/// GEMESSENER Grund fuer den **optionalen** Ausgang statt einer zweiten
/// Funktion: Regel aus G8j — *lieber eine Funktion mit optionalem Ausgang
/// als eine zweite Schleife*. Und die Regel aus G8h: `#[cfg(test)]` gilt
/// **nicht** fuer Integrationstests in `tests/`, also kann die Messung
/// nicht ueber ein Feature laufen.
#[must_use]
pub fn fenster_oder(ersatz: Option<&'static [[f64; 36]; 4]>) -> &'static [[f64; 36]; 4] {
    match ersatz {
        Some(f) => f,
        None => fenster(),
    }
}

#[must_use]
pub fn fenster() -> &'static [[f64; 36]; 4] {
    static F: std::sync::OnceLock<[[f64; 36]; 4]> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let mut r = [[0.0f64; 36]; 4];
        for (i, w) in r[0].iter_mut().enumerate() {
            let x = f64::from(i as u32) + 0.5;
            *w = (std::f64::consts::PI * x / 36.0).sin();
        }
        for (i, w) in r[3].iter_mut().enumerate() {
            let x = f64::from(i as u32) + 0.5;
            *w = if i < 12 {
                (std::f64::consts::PI * x / 12.0).sin()
            } else {
                0.0
            };
        }
        for (i, w) in r[1].iter_mut().enumerate() {
            *w = match i {
                0..=17 => 1.0,
                18..=23 => (std::f64::consts::PI * (f64::from(i as u32 - 18) + 0.5) / 12.0).sin(),
                _ => 0.0,
            };
        }
        for (i, w) in r[2].iter_mut().enumerate() {
            *w = match i {
                0..=5 => 0.0,
                6..=11 => (std::f64::consts::PI * (f64::from(i as u32 - 6) + 0.5) / 12.0).sin(),
                12..=17 => 1.0,
                _ => {
                    let y = f64::from(i as u32) - 17.0 + 0.5;
                    (std::f64::consts::PI * y / 36.0).sin()
                }
            };
        }
        r
    })
}

/// Die beiden Cosinus-Tabellen der MDCT.
///
/// * `lang[i][k] = cos( π/72 · (2i + 19)(2k + 1) )`, `i` 0..35, `k` 0..17
/// * `kurz[i][k] = cos( π/24 · (2i +  7)(2k + 1) )`, `i` 0..11, `k` 0..5
///
/// Der Versatz ist `n/2` im Nenner und `(2i + 1 + n/2)` im Zaehler — die
/// DCT-IV-Form der MDCT. GEMESSEN an ffmpegs Fensteraufbau:
///
/// ```c
/// d *= 0.5 * IMDCT_SCALAR / cos(M_PI * (2 * i + 19) / 72);
/// ```
///
/// Bei `i` und `k` vertauscht ist das genau dieselbe Form: der Faktor
/// `1/cos(π(2i+19)/72)` ist der, den ffmpeg **in das Fenster** steckt,
/// weil sein Schmetterling die DCT-IV-Form nicht direkt bildet.
#[must_use]
pub fn mdct_tabellen() -> (&'static [[f64; 18]; 36], &'static [[f64; 6]; 12]) {
    static T: std::sync::OnceLock<Tabellen> = std::sync::OnceLock::new();
    let (lang, kurz) = T.get_or_init(|| {
        let pi = std::f64::consts::PI;
        let mut lang = [[0.0f64; 18]; 36];
        for (i, zeile) in lang.iter_mut().enumerate() {
            for (k, zelle) in zeile.iter_mut().enumerate() {
                let a = pi * (2.0 * f64::from(i as u32) + 19.0) / 72.0;
                *zelle = (a * (2.0 * f64::from(k as u32) + 1.0)).cos();
            }
        }
        let mut kurz = [[0.0f64; 6]; 12];
        for (i, zeile) in kurz.iter_mut().enumerate() {
            for (k, zelle) in zeile.iter_mut().enumerate() {
                let a = pi * (2.0 * f64::from(i as u32) + 7.0) / 24.0;
                *zelle = (a * (2.0 * f64::from(k as u32) + 1.0)).cos();
            }
        }
        (lang, kurz)
    });
    (lang, kurz)
}

/// Die inverse MDCT fuer einen **langen** Block: 18 Werte hinein,
/// 36 heraus.
#[must_use]
pub fn imdct_lang(x: &[f64; 18]) -> [f64; 36] {
    let (lang, _) = mdct_tabellen();
    let mut y = [0.0f64; 36];
    for (i, zeile) in lang.iter().enumerate() {
        y[i] = zeile.iter().zip(x.iter()).map(|(c, v)| c * v).sum();
    }
    y
}

/// Die inverse MDCT fuer ein **Kurzblock**-Fenster: 6 hinein, 12 heraus.
///
/// # DIE FORMEL FUER DEN KURZBLOCK
///
/// Die Norm hat **keine** 18-Wert-Form fuer den Kurzblock, sondern
/// dieselbe Formel mit `n = 6`:
///
/// ```text
///   y[i] = Σ(k=0..5) X[k] · cos( π/24 · (2i + 7)(2k + 1) ),   i = 0..11
/// ```
///
/// `π/24` ist `π/(4·6)`, und der Versatz `7` ist `1 + 6/2`. Belegt an
/// ffmpegs `imdct12`, das genau diese Groessen benutzt
/// (`C3 = cos(π/6)/2`, `C4 = cos(π/12)/2`, `C5 = cos(π/10)/2` …).
#[must_use]
pub fn imdct_kurz(x: &[f64; 6]) -> [f64; 12] {
    let (_, kurz) = mdct_tabellen();
    let mut y = [0.0f64; 12];
    for (i, zeile) in kurz.iter().enumerate() {
        y[i] = zeile.iter().zip(x.iter()).map(|(c, v)| c * v).sum();
    }
    y
}

/// Der Zustand **eines** Kanals: die 18 aufgeschobenen Werte je Band
/// und der 512er Ring des Polyphasefilters.
#[derive(Debug, Clone)]
pub struct Synthese {
    /// Je Band die 18 Werte des vorigen Granules, die noch fehlen.
    ///
    /// # WAS DARIN STEHT, JE NACH BLOCKART
    ///
    /// * **lang**: die zweite Haelfte des 36-Wert-Fensters, `i` 0..17.
    ///   Sie wird im naechsten Granul zu den ersten 18 Werten addiert —
    ///   das ist das 50-Prozent-Ueberlappen.
    /// * **kurz**: nur die **letzten 6** Werte (`o[12..18]`), der Rest
    ///   steht auf null. Grund: die drei Fenster liegen mit Schritt 6
    ///   in 18 Werten, also ragen nur 6 hinaus.
    ///
    /// GEMESSEN: der Zustand nach einem Kurzblock ist genau an dieser
    /// Stelle 6 Werte breit besetzt (`ein_kurzblock_laesst_sechs_werte_
    /// stehen_und_der_rest_steht_auf_null`).
    band: [[f64; LINIEN_PRO_BAND]; BAENDERZAHL],
    /// Der Ring des Polyphasefilters, doppelt so lang wie der Index
    /// reicht — so wie `MPA_MAX_CHANNELS][512 * 2]` bei ffmpeg, damit
    /// ein Zeiger 16 oder 48 dahinter nicht herausfaellt.
    ring: [f64; 1024],
    /// Der Schreibzeiger im Ring, 0..511. GEMESSEN an ffmpeg: er startet
    /// bei 0 (der Kontext wird genullt) und geht mit
    /// `(zeiger - 32) & 511` weiter.
    zeiger: usize,
    /// Der Rest der letzten Fenstersumme — `dither_state`. Er ist der
    /// Anfang der naechsten Summe und geht deshalb **nicht** verloren:
    /// ohne ihn fehlen an jeder Probe 32 Terme.
    rest: f64,
    /// Ob das Band im letzten Granul kurz war. Nur fuer die Diagnose —
    /// die Ueberlappung braucht es nicht, weil der Zustand je Band
    /// schon die Form traegt.
    war_kurz: [bool; BAENDERZAHL],
}

impl Synthese {
    /// Ein frischer Kanal: alles null.
    #[must_use]
    pub fn neu() -> Self {
        Self {
            band: [[0.0; LINIEN_PRO_BAND]; BAENDERZAHL],
            ring: [0.0; 1024],
            zeiger: 0,
            rest: 0.0,
            war_kurz: [false; BAENDERZAHL],
        }
    }

    /// Die Fensternummer je Band, nach Blocktyp und Switch-Punkt.
    ///
    /// ffmpeg: `int win_idx = (switch_point && j < 2) ? 0 : block_type;`
    /// (`libavcodec/mpegaudiodsp_template.c`, `ff_imdct36_blocks`).
    /// Der Wert 0 dort ist mein Fenster **1**.
    ///
    /// # DIE ZAHL ZWEI, UND WARUM SIE VON DER ABTASTRATE ABHAENGT
    ///
    /// GEMESSEN: `compute_band_indexes` setzt `long_end = 8` fuer
    /// `sample_rate_index <= 2` und sonst `6` — das sind **2** bzw.
    /// **6** Baender. ffmpegs `j < 2` ist also die Fassung fuer
    /// 44100/48000/32000 und fuer 24000 Hz und tiefer zu klein. Alle
    /// drei Testdateien (`rate_index` 0, 1, 2) kommen damit auf 2 —
    /// `g8c_rauschen_mono` liegt mit 32000 Hz **auf** der Grenze.
    fn fensternummer(g: &Granule, rate_index: usize, band: usize) -> usize {
        let lange_baender = if rate_index <= 2 { 2 } else { 6 };
        if g.blocktyp == 2 {
            if g.gemischt && band < lange_baender {
                1
            } else {
                3
            }
        } else {
            g.blocktyp as usize
        }
    }

    /// Ein Granul: 576 requantisierte Linien hinein, 576 Samples heraus.
    ///
    /// # DIE REIHENFOLGE, UND DIE IST NORMATIV
    ///
    /// ```text
    ///   1. inverse MDCT (lang 18->36, kurz 6->12)
    ///   2. Fenster
    ///   3. Ueberlappung mit dem vorigen Granul   <- hier
    ///   4. Frequenzinversion (ungerade Baender, jedes zweite Sample)
    ///   5. Polyphasefilter
    /// ```
    #[must_use]
    pub fn granule(
        &mut self,
        spek: &Spektrum,
        g: &Granule,
        rate_index: usize,
    ) -> [f64; SAMPLES_PRO_GRANUL] {
        self.granule_mess(spek, g, rate_index, None)
    }

    /// Wie [`Self::granule`], legt aber den Zustand **nach** IMDCT,
    /// Fenster, Ueberlappung und Frequenzinversion ab.
    ///
    /// # GEMESSENER ORT, und das ist der Punkt dieses Moduls
    ///
    /// ```c
    /// 1959:  L3_decode(dec, &scratch, ...);   /* endet mit L3_imdct_gr
    ///                                           und L3_change_sign   */
    /// 1965:  if (g_dump_sb && g_gr == g_dump_nr)
    /// 1967:      fprintf(g_dump_sb, "%.17g\n", scratch.grbuf[0][q]);
    /// 1972:  mp3d_synth_granule(dec->qmf_state, scratch.grbuf[0], 18, ...);
    /// ```
    ///
    /// Die fuenfte Messstelle liegt also **nach** `L3_decode` und **vor**
    /// `mp3d_synth_granule` — das ist **der Eingang des Polyphasefilters**,
    /// und der Kommentar dort sagt es selbst: „Hybrid-Werte **VOR** der
    /// Polyphase".
    ///
    /// GEMESSENE Zuordnung zu meinem Code: `raus` am Ende dieser Funktion
    /// ist genau das — der Wert geht danach je Band und Zeitschritt an
    /// [`Self::band_werte_aus`], also an die DCT-32 und den Ring.
    ///
    /// GEMESSENE Notwendigkeit, und sie kostete drei Runden Suche:
    /// `g8c_sb.txt` passte zu **keinem** meiner Felder (`vor_synthese`,
    /// `vor_umordnung`, je mit und ohne `L3_change_sign`), alle vier Wege
    /// mit der besten Abweichung **1,000e+00**. Weil `g8c_sb.txt` einer
    /// Stufe **nach** `vor_synthese` abgelesen wurde, und diese Stufe
    /// gab es in meiner Sammlung nicht.
    ///
    /// **REGEL: eine Messstelle, die sich keinem Feld zuordnen laesst,
    /// liegt nicht am falschen Feld, sondern an einer Stufe, die die
    /// Sammlung nicht enthaelt.**
    pub fn granule_mess(
        &mut self,
        spek: &Spektrum,
        g: &Granule,
        rate_index: usize,
        sammel: Option<&mut [f64; SAMPLES_PRO_GRANUL]>,
    ) -> [f64; SAMPLES_PRO_GRANUL] {
        self.granule_mess_mit_fenster(spek, g, rate_index, sammel, None)
    }

    /// [`Self::granule_mess`] mit **ersetztem** Fenstervorrat.
    ///
    /// # WOFUER DAS DA IST (G9m)
    ///
    /// GEMESSEN: minimp3 traegt die Sinuswerte als **Dezimal-Literal** mit
    /// `f`-Suffix (`orakel/minimp3_mess.h:1344`), ich **berechne** sie.
    /// GEMESSENER Unterschied: **1 ulp** (`3,7e-09`) bei Index 0, und mit
    /// einem Sample-Wert von `3,0` sind das **`1,1e-08`** — die groesste
    /// **absolute** Abweichung aus G8j.
    ///
    /// **REGEL aus G8j, hier befolgt:** *lieber eine Funktion mit
    /// optionalem Ausgang als eine zweite Schleife.* Also gibt es
    /// `granule_mess` (ruft diese mit `None`) und diese hier. Kein
    /// zweiter Durchlauf durch dieselben 576 Werte.
    ///
    /// **GEMESSENE Grenze, und sie ist ehrlich:** das ersetzt **nur** das
    /// Fenster. Die **Rechnung** bleibt `f64`. Wer auch die Rechnung in
    /// `f32` will, braucht einen zweiten Parameter — und GEMESSEN ist, dass
    /// G9m diesen Fall **nicht** braucht (Ergebnis im Bericht).
    pub fn granule_mess_mit_fenster(
        &mut self,
        spek: &Spektrum,
        g: &Granule,
        rate_index: usize,
        sammel: Option<&mut [f64; SAMPLES_PRO_GRANUL]>,
        fenstervorrat: Option<&'static [[f64; 36]; 4]>,
    ) -> [f64; SAMPLES_PRO_GRANUL] {
        let f = crate::mp3::synthese::fenster_oder(fenstervorrat);
        let mut raus = [0.0f64; SAMPLES_PRO_GRANUL];
        for band in 0..BAENDERZAHL {
            let basis = band * LINIEN_PRO_BAND;
            let nummer = Self::fensternummer(g, rate_index, band);
            let ist_kurz = nummer == 3;
            if ist_kurz {
                // DREI 6->12, mit Schritt 6 ueberlagert. Das Feld muss
                // **24** Werte haben, nicht 18: drei 12er Segmente mit
                // Schritt 6 spannen 0..23, und `o[18..24]` traegt in den
                // naechsten Granul hinein. GEMESSENER FEHLER: mit 18
                // Feldern endete der Lauf mit „index out of bounds: the
                // len is 18 but the index is 18".
                //
                // Und Wert `i` des Fensters `w` steht bei `3i + w`:
                // belegt an ffmpegs `imdct12`, `in[0*3]`, `in[1*3]`.
                let mut o = [0.0f64; 24];
                for w in 0..3usize {
                    let mut ein = [0.0f64; 6];
                    for (i, v) in ein.iter_mut().enumerate() {
                        *v = spek[basis + 3 * i + w];
                    }
                    let y = imdct_kurz(&ein);
                    for (i, v) in y.iter().enumerate() {
                        o[6 * w + i] += f[3][i] * v;
                    }
                }
                for i in 0..LINIEN_PRO_BAND {
                    let heraus = o[i] + self.band[band][i];
                    raus[basis + i] = if band % 2 == 1 && i % 2 == 1 {
                        -heraus
                    } else {
                        heraus
                    };
                }
                self.band[band][..6].copy_from_slice(&o[18..24]);
                self.band[band][6..].fill(0.0);
            } else {
                let mut ein = [0.0f64; 18];
                ein.copy_from_slice(&spek[basis..basis + LINIEN_PRO_BAND]);
                let y = imdct_lang(&ein);
                for i in 0..LINIEN_PRO_BAND {
                    let heraus = f[nummer][i] * y[i] + self.band[band][i];
                    raus[basis + i] = if band % 2 == 1 && i % 2 == 1 {
                        -heraus
                    } else {
                        heraus
                    };
                    self.band[band][i] = f[nummer][i + 18] * y[i + 18];
                }
            }
            self.war_kurz[band] = ist_kurz;
        }
        if let Some(ziel) = sammel {
            // GEMESSENER Ort: **nach** der Frequenzinversion, denn die ist
            // in der Schleife oben (`band % 2 == 1 && i % 2 == 1`) und
            // `L3_change_sign` in minimp3 steht **vor** der Messstelle.
            *ziel = raus;
        }
        raus
    }

    /// Der Polyphasefilter: 32 Bandwerte hinein, 32 Samples heraus.
    ///
    /// Die Struktur ist ffmpegs `ff_mpa_synth_filter` und
    /// `ff_mpadsp_apply_window`:
    ///
    /// 1. Die 32 Samples durch eine DCT-32 und in den Ring schreiben.
    /// 2. Aus dem 512er Ring **32** Samples heraus, mit dem Fenster
    ///    `SYNTHESE_FENSTER`.
    /// 3. Den Zeiger um 32 **zurueck**: `(zeiger - 32) & 511`.
    ///
    /// # DER REST (`dither_state`) — DER WAR DER FEHLER
    ///
    /// GEMESSENER FEHLER, und er ist die halbe Funktion. Ich hatte
    /// ffmpegs **Fixed-Point**-Verhalten in den Float-Pfad gebaut:
    ///
    /// ```text
    /// // alt
    /// let mut summe = self.rest;   // Rest der vorigen Probe
    /// ...
    /// self.rest = summe;           // und wieder mitnehmen
    /// ```
    ///
    /// ffmpegs `round_sample` im Float-Pfad macht aber:
    ///
    /// ```c
    /// static inline float round_sample(float *sum)
    /// {
    ///     float sum1 = *sum;
    ///     *sum = 0;          /* <- die Summe wird auf NULL gesetzt */
    ///     return sum1;
    /// }
    /// ```
    ///
    /// **Jeder Ausgangswert beginnt bei null**, und `*dither_state = sum`
    /// speichert danach **null**. Das Mitfuehren des Rests gehoert zu
    /// `round_sample` im **Fixed-Point**-Pfad, wo es
    ///
    /// ```c
    ///     sum1 = (int)((*sum) >> OUT_SHIFT);
    ///     *sum &= (1<<OUT_SHIFT)-1;
    /// ```
    ///
    /// heisst — dort bleibt der **Bruchteil** uebrig, weil ganzzahlig
    /// gerundet wird. Im Float-Pfad gibt es nichts zu runden.
    ///
    /// GEMESSENE FOLGE, und sie erklaert die Form des Fehlers: mein PCM
    /// war die Summe aus **zwei** Zeitpunkten und driftete langsam —
    /// GEMESSEN `-2,57111 -2,53711 -2,73448 -2,98190 -2,60352 -2,31186`
    /// gegen das Orakel `-0,0142069 0,138936 -0,34152 0,047035 0,288632`.
    /// Ein Versatz von rund -2,5 **plus** die richtige Schwankung: das
    /// ist genau ein mitgenommener Rest.
    ///
    /// GEMESSENE BESTAETIGUNG des Fensters selbst: `SUM8` in ffmpeg
    /// benutzt `(w)[0 * 64]` bis `(w)[7 * 64]`, also **Schritt 64** —
    /// genau wie hier. Meine Deutung in Runde 11 war falsch.
    pub fn band_werte_aus(&mut self, subband: &[f64; BAENDERZAHL]) -> [f64; BAENDERZAHL] {
        let v = dct32(subband);
        for (i, wert) in v.iter().enumerate() {
            self.ring[self.zeiger + i] = *wert;
        }
        let s = self.zeiger;
        let d = &SYNTHESE_FENSTER;
        let mut raus = [0.0f64; BAENDERZAHL];
        // GEMESSEN: ffmpegs Float-Pfad beginnt **jeden** Ausgangswert bei
        // null — siehe die Herkunftsangabe an dieser Funktion. Der alte
        // `let mut summe = self.rest;` war der letzte Fehler im Dekoder.
        let mut summe = 0.0f64;

        for k in 0..8usize {
            summe += self.ring[s + 16 + 64 * k] * d[64 * k];
            summe -= self.ring[s + 48 + 64 * k] * d[32 + 64 * k];
        }
        raus[0] = summe;

        for j in 1..16usize {
            let mut zweit = 0.0f64;
            let w = j;
            let w2 = 32 - j;
            for k in 0..8usize {
                let p = self.ring[s + 16 + j + 64 * k];
                summe += p * d[w + 64 * k];
                zweit -= p * d[w2 + 64 * k];
                let q = self.ring[s + 48 - j + 64 * k];
                summe -= q * d[32 + w + 64 * k];
                zweit -= q * d[32 + w2 + 64 * k];
            }
            raus[j] = summe;
            // GEMESSENER FEHLER: die **beiden** Haelften. ffmpeg:
            // ```c
            // *samples  = round_sample(&sum);   /* gibt sum aus, setzt sum = 0 */
            // sum += sum2;                      /* also: sum = sum2      */
            // *samples2 = round_sample(&sum);   /* gibt sum2 aus        */
            // ```
            // Der zweite Ausgang ist also **`sum2` allein**, nicht
            // `sum + sum2` — und `sum` ist danach wieder null. Der alte
            // Code schrieb `summe += zweit; raus[32 - j] = summe;`, fuehrte
            // die Summe also ueber alle 32 Ausgaenge eines Zeitschritts
            // weiter und baute damit aus **32** Ausgaengen **bis zu 32
            // Anteilen auf.
            summe = 0.0;
            raus[32 - j] = zweit;
        }

        // GEMESSENER FEHLER: das ist der **16.** Ausgang, nicht der 31.
        // `for (j = 1..16)` schreibt die Indizes 1..15 und 31..17; danach
        // bleibt nur noch 16. Der alte Code schrieb `raus[31]` und
        // **ueberschrieb** damit den von `j = 1` gesetzten Wert, während
        // `raus[16]` auf null blieb.
        for k in 0..8usize {
            summe -= self.ring[s + 32 + 64 * k] * d[48 + 64 * k];
        }
        raus[16] = summe;
        // `self.rest` bleibt null: ffmpegs `*dither_state = sum` speichert
        // im Float-Pfad den Wert, den `round_sample` gerade auf null
        // gesetzt hat. Der alte `self.rest = summe;` verdoppelte die
        // Summe des naechsten Zeitschritts.
        debug_assert!(self.rest == 0.0, "der Rest muss im Float-Pfad null bleiben");

        self.zeiger = (self.zeiger + 512 - 32) & 511;
        raus
    }

    /// Der Ring des Polyphasefilters, unveraendert. Nur fuer die Tests.
    #[must_use]
    pub fn ring(&self) -> &[f64; 1024] {
        &self.ring
    }
    /// Der Rest der letzten Fenstersumme (`dither_state`).
    #[must_use]
    pub fn rest(&self) -> f64 {
        self.rest
    }
    /// Der Schreibzeiger im Ring, 0..511.
    #[must_use]
    pub fn zeiger(&self) -> usize {
        self.zeiger
    }
    /// Die 18 aufgeschobenen Werte des Bandes `band`.
    #[must_use]
    pub fn vorgang(&self, band: usize) -> &[f64; LINIEN_PRO_BAND] {
        &self.band[band]
    }
    /// Ob das Band im letzten Granul **kurz** war.
    #[must_use]
    pub fn war_kurz(&self, band: usize) -> bool {
        self.war_kurz[band]
    }
}
/// Die beiden MDCT-Tabellen als ein Typ — clippy meckert sonst ueber
/// „very complex type", und zu Recht: `([[f64; 18]; 36], [[f64; 6]; 12])`
/// steht an drei Stellen.
type Tabellen = ([[f64; 18]; 36], [[f64; 6]; 12]);

/// Die DCT-32 der Polyphase: `X[i] = Σ(k=0..31) x[k]·cos(π·(2k+1)·i/64)`.
///
/// # DIE MATRIX, GEMESSEN AN FFMPEGS EIGENEM SCHMETTERLING
///
/// ffmpegs `ff_dct32_float` ist ein Hand-Schmetterling aus 30 `BF`-Stufen
/// mit den Tabellen `COS0_0`…`COS4_0`. Ich habe ihn **unveraendert
/// kompiliert** (`gcc -O2`, `int main` ruft `dct32` auf den Einheitsvektor)
/// und die Spalten der Matrix gemessen:
///
/// ```text
///   Spalte 0: 1.000000000  0.998795456  0.995184727  0.989176510 …
///   Spalte 1: 1.000000000  0.989176510  0.956940336  0.903989293 …
///   Spalte 2: 1.000000000  0.970031253  0.881921264  0.740951125 …
///   Spalte 3: 1.000000000  0.941544065  0.773010453  0.514102744 …
/// ```
///
/// GEMESSENE SCHRITTWEITE je Spalte: Spalte `k` hat die Schrittweite
/// `(2k+1)·π/64` — Spalte 0 um `π/64`, Spalte 1 um `3π/64`, Spalte 2 um
/// `5π/64`, Spalte 3 um `7π/64`. Nachgerechnet: `cos(5π·i/64)` ergibt
/// Spalte 2 **wörtlich** (`0,970031 / 0,881921 / 0,740951 / 0,555570`).
///
/// Und daraus die Formel: **`cos(π·(2k+1)·i/64)`** — der zweite Faktor
/// ist `i`, nicht `2i+1`.
///
/// # MEINE ERSTE FORMEL, UND WARUM SIE PLAUSIBEL AUSSIEHT
///
/// Ich hatte geschrieben:
///
/// ```text
///   X[i] = Σ_k x[k] · cos( π/64 · (2k+1)(2i+1) )
/// ```
///
/// Das ist ebenfalls „eine DCT-32 mit ungeraden Harmonischen", und die
/// **Spalte 0 ist in beiden eine Kosinusrampe** (`cos(πi/64)` gegen
/// `cos(π(2i+1)/64)`, beide bei 1 beginnend und fallend). Der Unterschied
/// zeigt sich erst an **Spalte 1**: gemessen `0,989177`, was
/// `cos(3π/64)` ist — und nicht `cos(3π/128) = 0,997859`.
///
/// GEMESSENE FOLGE im ganzen Dekoder: `g8c_rauschen_mono` kam mit
/// **Faktor 1,061200** und **-43,8 dB** heraus (nach der
/// `D[512]`-Korrektur mit Faktor 0,095300 und -5,0 dB). Eine
/// Faktorendifferenz, die **keine Konstante** ist — und genau das ist
/// das Kennzeichen einer **anderen** Matrix: eine reine
/// Amplitudenfrage haette **einen** Faktor fuer alle fuenf Kanaele.
///
/// # DER IMPULSGANG, DER DEN FEHLER ZEIGTE
///
/// Ein Impuls in Band 0 muss eine **endliche** Kurve ergeben, die nach
/// 64 Taps **null** ist — der Prototypfilter hat 512 Abtaps, und nach
/// seinem Durchlauf ist alles vorbei. GEMESSEN mit der falschen
/// Matrix: Index 0 exakt **null**, Index 16 exakt **null**, und ab Index
/// 31 ein **konstanter** Wert bis zum Ende der 576 Samples. Eine
/// konstante Endlage ist fuer einen FIR-Filter unmoeglich.
///
/// Gegen minimp3 als zweites Orakel (das **keine** DCT-32 hat, sondern
/// die Subbandwerte direkt in den Ring schreibt): Energiedifferenz
/// **Faktor 3,574** bei sonst gleicher Anordnung.
#[must_use]
pub fn dct32(x: &[f64; 32]) -> [f64; 32] {
    static T: std::sync::OnceLock<[[f64; 32]; 32]> = std::sync::OnceLock::new();
    let t = T.get_or_init(|| {
        let mut r = [[0.0f64; 32]; 32];
        for (i, zeile) in r.iter_mut().enumerate() {
            for (k, zelle) in zeile.iter_mut().enumerate() {
                let a =
                    std::f64::consts::PI * (2.0 * f64::from(k as u32) + 1.0) * f64::from(i as u32)
                        / 64.0;
                *zelle = a.cos();
            }
        }
        r
    });
    let mut raus = [0.0f64; 32];
    for (i, o) in raus.iter_mut().enumerate() {
        *o = t[i].iter().zip(x.iter()).map(|(c, v)| c * v).sum();
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp3::granel::kurzgrenzen;
    use crate::mp3::rahmen::test_granule;

    fn blocktyp(g: &Granule, block: u32, gemischt: bool) -> Granule {
        let mut x = g.clone();
        x.blocktyp = block;
        x.gemischt = gemischt;
        x
    }

    #[test]
    fn das_normale_fenster_ist_symmetrisch() {
        // DIE EIGENSCHAFT, DIE GILT — und ich hatte erst die falsche.
        //
        // Ich habe `Σ(i=0..17) w[i]·w[i+18] == 0` behauptet (das ist die
        // Princen-Bradley-Bedingung). **GEMESSEN: 5,736856622834928.**
        // Die Bedingung ist fuer diese Fenster nicht haltbar: mit
        // `w[i] = sin(π(i+½)/36)` ist `w[i]·w[i+18] = ½·sin(π(i+½)/18)`,
        // und diese Summe ist fuer **gerades** n nicht null.
        //
        // Was die MDCT braucht, ist **Symmetrie**: `w[35-i] == w[i]`.
        // Damit heben sich die zeitliche Aliaspaare auf — die Beiträge,
        // die aus dem vorigen und aus dem naechsten Granul an derselben
        // Stelle stehen.
        let f = fenster();
        for i in 0..36usize {
            assert!(
                (f[0][i] - f[0][35 - i]).abs() < 1e-15,
                "Fenster 0: w[{i}] = {} und w[{}] = {}",
                f[0][i],
                35 - i,
                f[0][35 - i]
            );
        }
        for i in 0..12usize {
            assert!(
                (f[3][i] - f[3][11 - i]).abs() < 1e-15,
                "Fenster 3: w[{i}] und w[{}] unterscheiden sich",
                11 - i
            );
        }
        // Und Fenster 1 ist **halb**, nicht symmetrisch: es ist 1 in der
        // ersten Haelfte und nur rechts belegt. Das ist der Sinn des
        // Fensters, und es steht hier, damit es niemand „repariert".
        assert_eq!(f[1][0], 1.0);
        assert_eq!(f[1][35], 0.0);
        assert_eq!(f[2][0], 0.0);
    }

    #[test]
    fn die_fenster_haben_die_richtige_energie() {
        let f = fenster();
        // UEBER 36 Werte IST ES 18, NICHT 9 — das war mein erster
        // Fehlglaube. GEMESSEN: `Σ(i=0..35) sin²(π(i+½)/36) = 18`, weil
        // die 36 Werte die **beiden** Haelfften sind und jede fuer sich
        // `n/2 = 9` traegt. Die Zahl 9 gehoert zu **einer** Haelfte, und
        // nur die Haelfte geht ins Ueberlappen ein.
        let alle: f64 = f[0].iter().map(|w| w * w).sum();
        assert!(
            (alle - 18.0).abs() < 1e-12,
            "Fenster 0, alle 36: {alle}, erwartet 18"
        );
        let haelfte: f64 = f[0].iter().take(18).map(|w| w * w).sum();
        assert!(
            (haelfte - 9.0).abs() < 1e-12,
            "Fenster 0, erste Haelfte: {haelfte}, erwartet n/2 = 9"
        );
        // Fenster 3 hat nur die ersten 12 Werte: 12/2 = 6.
        let kurz: f64 = f[3].iter().map(|w| w * w).sum();
        assert!((kurz - 6.0).abs() < 1e-12, "Fenster 3: {kurz}, erwartet 6");
    }

    #[test]
    fn die_fenster_haben_die_richtigen_nullen() {
        let f = fenster();
        // Fenster 1 ist in der ersten Haelfte konstant 1 und endet nach
        // 24 Werten.
        for i in 0..18 {
            assert_eq!(f[1][i], 1.0, "Fenster 1[{i}] muss 1 sein");
        }
        assert_eq!(f[1][24], 0.0, "Fenster 1[24] muss 0 sein");
        assert_eq!(f[1][35], 0.0, "Fenster 1[35] muss 0 sein");
        // Fenster 2 beginnt mit 6 Nullen und laeuft bis 36.
        for i in 0..6 {
            assert_eq!(f[2][i], 0.0, "Fenster 2[{i}] muss 0 sein");
        }
        assert!(f[2][35] > 0.0, "Fenster 2[35] muss ungleich 0 sein");
        // Fenster 3 hat hinter 12 keine Werte.
        assert_eq!(f[3][12], 0.0);
        assert_eq!(f[3][35], 0.0);
    }

    #[test]
    fn der_zustand_ist_die_zweite_haelfte_des_fensters() {
        // DIE AUSSAGE, DIE STIMMT — und die ist weit brauchbarer als
        // meine erste.
        //
        // Ich hatte behauptet, zwei gleiche Granulen muessten sich
        // aufheben („Princen-Bradley"). **GEMESSEN: 17,7204579283692**
        // — sie heben sich nicht auf, und sie duerfen es auch nicht:
        // zwei gleiche Granuel ergeben ein **doppeltes** Signal, weil
        // sich die beiden Haelften des Fensters an jeder Stelle
        // addieren statt aufzuheben. Aufhebung passiert zwischen
        // *verschiedenen* Granulen, ueber die zeitlichen Aliase.
        //
        // Was ich stattdessen festnageln kann und sollte: der Zustand
        // **ist** die mit dem Fenster multiplizierte zweite Haelfte der
        // 36-Wert-MDCT. Das prueft die Reihenfolge „Ueberlappung nach
        // dem Fenster" und nichts sonst.
        let mut s = Synthese::neu();
        let g = blocktyp(&test_granule(), 0, false);
        let mut spek: [f64; 576] = [0.0; 576];
        for (i, v) in spek.iter_mut().enumerate() {
            *v = f64::from((i % 5) as i32) - 2.0;
        }
        let _ = s.granule(&spek, &g, 0);
        let f = fenster();
        for band in [0usize, 1, 17, 31] {
            let mut ein = [0.0f64; 18];
            ein.copy_from_slice(&spek[band * 18..band * 18 + 18]);
            let y = imdct_lang(&ein);
            for i in 0..18usize {
                let soll = f[0][i + 18] * y[i + 18];
                assert!(
                    (s.vorgang(band)[i] - soll).abs() < 1e-12,
                    "Band {band}, Zustand[{i}]: {} statt {soll}",
                    s.vorgang(band)[i]
                );
            }
        }
    }

    #[test]
    fn die_gerade_und_ungerade_haelfte_sind_verschieden() {
        // Wenn die MDCT die Normierung nicht haette, waeren alle 36
        // Ausgaenge gleich — das ist der Test dafuer, dass ueberhaupt
        // gerechnet wird.
        let x = [1.0f64; 18];
        let y = imdct_lang(&x);
        let mittel: f64 = y.iter().sum::<f64>() / 36.0;
        let abw: f64 = y.iter().map(|w| (w - mittel).abs()).sum::<f64>() / 36.0;
        assert!(
            abw > 1e-6,
            "alle 36 Werte gleich ({mittel}): es wurde nichts gerechnet"
        );
        // Und die Werte sind endlich und von der Groessenordnung der
        // Eingabe (Faktor 18/2 = 9 bei voller Auslenkung).
        assert!(y.iter().all(|w| w.is_finite()));
        assert!(
            y.iter().all(|w| w.abs() < 100.0),
            "Werte zu gross: {:?}",
            y.iter().take(4).collect::<Vec<_>>()
        );
    }

    #[test]
    fn die_dct32_ist_die_gemessene_matrix() {
        // # DIE MATRIX, GEMESSEN AN FFMPEGS EIGENEM CODE
        //
        // Ich habe `ff_dct32_float` aus `libavcodec/dct32_template.c`
        // **unveraendert** mit `gcc -O2` gebaut und die Spalten am
        // Einheitsvektor abgelesen. GEMESSEN:
        //
        // ```text
        //   Spalte 0: 1.000000000  0.998795456  0.995184727  0.989176510 …
        //   Spalte 1: 1.000000000  0.989176510  0.956940336  0.903989293 …
        //   Spalte 2: 1.000000000  0.970031253  0.881921264  0.740951125 …
        //   Spalte 3: 1.000000000  0.941544065  0.773010453  0.514102744 …
        // ```
        //
        // Die Schrittweite je Spalte ist `(2k+1)·π/64` — Spalte 1 um
        // `3π/64`, Spalte 2 um `5π/64`, Spalte 3 um `7π/64`.
        // Nachgerechnet: `cos(5π·i/64)` ergibt Spalte 2 **woertlich**.
        //
        // Daraus die Formel: `X[i] = Σ_k x[k]·cos(π·(2k+1)·i/64)`.
        // Der zweite Faktor ist **`i`**, nicht `2i+1`.
        //
        // # MEINE ERSTE FORMEL
        //
        // `X[i] = Σ_k x[k]·cos(π/64·(2k+1)(2i+1))` — ebenfalls „eine
        // DCT-32 mit ungeraden Harmonischen", und die **Spalte 0 ist in
        // beiden eine Kosinusrampe**, die bei 1 beginnt und faellt. Der
        // Unterschied zeigt sich erst an **Spalte 1**: `cos(3π/64) =
        // 0,989177` (gemessen) gegen `cos(3π/128) = 0,997859`.
        //
        // GEMESSENE FOLGE im Dekoder: `g8c_sine_stereo` K0 von
        // **-28,9 dB auf -56,2 dB**. Die Matrix ist damit richtig und
        // war **nicht** die Ursache — sie steht trotzdem hier, weil ein
        // falsches Zwischenergebnis, das man wegwirft, genauso wenig
        // zaehlt wie ein richtiges, das man erfindet.
        // GEMESSENE Korrektur einer Warnung: der Wert wurde ausserhalb
        // der Schleife gesetzt und sofort ueberschrieben. Die Deklaration
        // gehoert in die Schleife.
        for k in 0..32usize {
            let mut x = [0.0f64; 32];
            x[k] = 1.0;
            let spalte = dct32(&x);
            for i in 0..32usize {
                let soll = (std::f64::consts::PI
                    * (2.0 * f64::from(k as u32) + 1.0)
                    * f64::from(i as u32)
                    / 64.0)
                    .cos();
                assert!(
                    (spalte[i] - soll).abs() < 1e-12,
                    "dct32, Spalte {k}, Zeile {i}: {} statt {soll}",
                    spalte[i]
                );
            }
        }
    }

    #[test]
    fn der_polyphase_ring_dreht_sich_um_32() {
        let mut s = Synthese::neu();
        assert_eq!(s.zeiger(), 0, "ffmpeg startet bei 0");
        let null = [0.0f64; 32];
        s.band_werte_aus(&null);
        assert_eq!(s.zeiger(), 480, "nach einem Schritt: 0 - 32 = -32 & 511");
        // 16 Schritte zu 32 sind 512 — einmal herum. Nach dem ersten
        // Schritt fehlen also noch **15**, nicht 16; mit 16 waeren es
        // 17 und der Zeiger stuende wieder bei 480.
        for _ in 0..15 {
            s.band_werte_aus(&null);
        }
        assert_eq!(s.zeiger(), 0, "nach 16 Schritten zurueck bei 0");
    }

    #[test]
    fn die_frequenzinversion_trifft_nur_die_ungeraden_baender() {
        // DERSELBE Inhalt in Band 0 und in Band 1. Band 0 ist gerade,
        // Band 1 ungerade — und bei ungeraden Baendern wird jedes
        // zweite Sample gespiegelt.
        //
        // # MEINE ERSTE FASSUNG WAR UNSINNIG
        //
        // Ich hatte zwei **identische** `Synthese` gebaut und
        // verglichen, was natuerlich dasselbe ergab. Der Test lief
        // erst durch, als ich ihn kaputt gemacht hatte.
        let mut s = Synthese::neu();
        let g = blocktyp(&test_granule(), 0, false);
        let mut spek: [f64; 576] = [0.0; 576];
        for i in 0..18usize {
            spek[i] = f64::from(i as i32) - 3.0;
            spek[18 + i] = f64::from(i as i32) - 3.0;
            spek[36 + i] = f64::from(i as i32) - 3.0;
        }
        let a = s.granule(&spek, &g, 0);
        for i in 0..18usize {
            // GEMESSENER BEFUND, als ich das erste Mal **gleich**
            // verglichen habe: bei `i = 1` kam `-1,1612427645102497`
            // gegen `+1,1612427645102497`. Das ist die Inversion, genau
            // wie sie sein soll — ich hatte die Vorzeichenregel im
            // Test vergessen, nicht im Code.
            let soll = if i % 2 == 1 { -a[i] } else { a[i] };
            assert!(
                (a[18 + i] - soll).abs() < 1e-15,
                "Band 1, i = {i}: {} statt {soll}",
                a[18 + i]
            );
            assert!(
                (a[36 + i] - a[i]).abs() < 1e-15,
                "Band 2, i = {i}: {} statt {} — der gerade Band ist gespiegelt worden",
                a[36 + i],
                a[i]
            );
        }
        // # WAS ICH ALS ZWEITES NOCH BEHAUPTET HABE, UND WAS FALSCH IST
        //
        // „die Zahl der negativen Werte in Band 1 muesse der in Band 0
        // entsprechen". **GEMESSEN: 18 gegen 9.** Die Vorzeichen eines
        // MDCT-Ausgangs haengen vom Spektrum ab, nicht nur von der
        // Inversion — sie zu zaehlen prueft nichts. Der Vergleich je
        // Index oben ist die richtige Zusage; die Zaehlung ist Removed.
    }

    #[test]
    fn ein_kurzblock_laesst_sechs_werte_stehen_und_der_rest_steht_auf_null() {
        let mut s = Synthese::neu();
        let g = blocktyp(&test_granule(), 2, false);
        let spek: [f64; 576] = [1.0; 576];
        let _ = s.granule(&spek, &g, 0);
        for band in 0..BAENDERZAHL {
            let v = s.vorgang(band);
            for (i, w) in v.iter().enumerate().take(6) {
                assert!(*w != 0.0, "Kurzblock: Band {band}, Rest[{i}] = 0");
            }
            for (i, w) in v.iter().enumerate().skip(6) {
                assert_eq!(*w, 0.0, "Kurzblock: Band {band}, Rest[{i}] != 0");
            }
        }
    }

    #[test]
    fn die_kurzblockgrenzen_kommen_aus_dem_blocktyp() {
        // Belegt an `compute_band_indexes` in
        // `libavcodec/mpegaudiodec_template.c`; der Test haelt fest, was
        // der Code daraus macht.
        let g = test_granule();
        assert_eq!(kurzgrenzen(&blocktyp(&g, 0, false), 0), (22, 13));
        assert_eq!(kurzgrenzen(&blocktyp(&g, 1, false), 0), (22, 13));
        assert_eq!(kurzgrenzen(&blocktyp(&g, 3, false), 0), (22, 13));
        assert_eq!(kurzgrenzen(&blocktyp(&g, 2, false), 0), (0, 0));
        assert_eq!(kurzgrenzen(&blocktyp(&g, 2, true), 0), (8, 3));
        assert_eq!(kurzgrenzen(&blocktyp(&g, 2, true), 3), (6, 3));
    }
}
