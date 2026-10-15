//! Granulate: die Seite, auf der in Layer III die eigentliche Information
//! steht.
//!
//! # Das Seiteninfo-Layout, gemessen statt geglaubt
//!
//! Quelle ist `libavcodec/mpegaudiodec_template.c`, Funktion
//! `mp_decode_layer3()`. Die Reihenfolge ist **nicht** die, die man errät:
//!
//! ```text
//! main_data_begin              9 Bit
//! privat                       5 Bit (mono) / 3 Bit (stereo)
//! scfsi Granulat 1             4 Bit je Kanal     <- VOR der Granulatrunde!
//! Granulat 0                   59 Bit je Kanal
//! Granulat 1                   59 Bit je Kanal
//! ```
//!
//! GEMESSEN: `9 + 5 + 4 + 59 + 59 = 136 Bit = 17 Byte` (mono) und
//! `9 + 3 + 8 + 4·59 = 256 Bit = 32 Byte` (stereo). Das sind genau die
//! Seiteninfolängen, die der Leser beim CRC erwartet
//! (`handle_crc(s, 8*17)` bzw. `8*32`).
//!
//! **Das `scfsi` steht vor der Granulatrunde, nicht darin.** Das ist die
//! Falle: wer es pro Granulat liest, verschiebt alle 59 Bit danach und
//! bekommt eine Datei, die gueltig aussieht und falsch klingt.
//!
//! Die 59 Bit je Granulat, in Lesereihenfolge:
//!
//! ```text
//! part2_3_length        12
//! big_values             9      (darf hoechstens 288 sein)
//! global_gain            8
//! scalefac_compress      4
//! blocksplit_flag        1
//!   0: table_select     3 x 5
//!      region_address1    4
//!      region_address2    3
//! preflag                1
//! scalefac_scale         1
//! count1table_select     1
//! ```
//!
//! # ZWEI MESSINSTRUMENTE, die ffmpeg selbst liefert
//!
//! Die beiden Fehlermeldungen des Lesers sind benennte Marken im Bitstrom:
//!
//! ```c
//! if (g->big_values > 288) { av_log(..., "big_values too big\n"); return AVERROR_INVALIDDATA; }
//! ...
//! if (g->block_type == 0) { av_log(..., "invalid block type\n"); return AVERROR_INVALIDDATA; }
//! ```
//!
//! Damit laesst sich die Bitlage **messen**, statt sie zu behaupten: man
//! schreibt `big_values = 511` an eine Kandidatenstelle und schaut, an
//! welcher Stelle die Meldung erscheint. Das ist die Uebertragung derselben
//! Technik, die bei FLAC die Partitionsschnitt und bei LPC die
//! Koeffizientenreihenfolge geklaert hat — diesmal gegen den Leser
//! selbst und nicht gegen die Spezifikation.
//!
//! # Skalierungsfaktoren: null Bit bei `scalefac_compress = 0`
//!
//! `ff_slen_table` liefert fuer `scalefac_compress = 0` sowohl `slen1 = 0`
//! als auch `slen2 = 0`, und der Leser schreibt dann keine
//! Skalierungsfaktorenbits, sondern setzt alle 21 auf null. Das heisst:
//! **der gesamte `part2_3_length` ist reiner Huffman-Datenbereich**.
//!
//! GEMESSEN 02.10.2026 am Exponenten, den der Leser daraus bildet:
//!
//! ```text
//! v0 = (global_gain - 210) - ((sf + pretab[i]) << (scalefac_scale+1)) + 400
//! ```
//!
//! mit `sf = 0` und `preflag = 0` bleibt `v0 = global_gain + 190`. Und der
//! Betrag eines Wertes `is = 1` im count1-Bereich ist `exp_table[v0]` mit
//! (`libavcodec/mpegaudio_tablegen.h`):
//!
//! ```text
//! exp2_lut  = [1, 1.18920711500272106672, sqrt(2), 1.68179283050742908606]
//! IMDCT_SCALAR = 1.759
//! exp2_base    = 2^(-72)
//! exp_table[e] = round( 2^(e/4 - 72) / 1.759 )
//! ```
//!
//! Also **berechenbar statt tabuliert** — fuer 512 Exponenten, und die
//! Tabelle waere 2 KB Zahlen, die man abtippen muesste.

use crate::bits::Bits;

/// Die Zahl der Spektralwerte je Kanal und Granulat: 32 Subbaender zu 18.
pub const WERTE_JA_GRANULAT: usize = 576;

/// Die groesste zulaessige `big_values`. Ueber 288 weist der Leser die Datei
/// zurueck.
pub const BIG_VALUES_MAX: u16 = 288;

/// Ein Granulat: die Seiteninformation und die zugehoerigen Hauptdaten.
///
/// Der Standardwert ist ein **leeres** Granulat — `part2_3_length = 0`, und
/// das ist ein vollstaendig gueltiges Granulat, das Stille ergibt. Deshalb
/// ist `Default` hier nicht `None`, sondern ein brauchbarer Wert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Granule {
    /// Wie viele Bit der Leser fuer dieses Granulat liest, 0..4095.
    ///
    /// Das ist **die** Zahl, an der sich alles andere messen laesst: steht
    /// sie falsch, liest der Leser in den naechsten Rahmen hinein oder
    /// laesst Daten liegen, und in beiden Faellen ohne Fehlermeldung.
    pub part2_3_laenge: u16,
    /// Anzahl der Werte im grosswertigen Bereich, 0..288.
    pub big_values: u16,
    /// Globaler Verstärkungsfaktor, 0..255.
    ///
    /// In den Exponenten geht er als `global_gain + 190` ein.
    pub global_gain: u8,
    /// Kompression der Skalierungsfaktoren, 0..15.
    ///
    /// **0 heisst: keine Skalierungsfaktorenbits im Datenbereich.** Das ist
    /// der bequemste Wert fuer den ersten hörbaren Schritt, weil dann der
    /// ganze `part2_3_length` Huffman-Daten sind.
    pub scalefac_compress: u8,
    /// Die drei Huffman-Tabellen der Regionen, je 0..31.
    pub table_select: [u8; 3],
    /// `region_address1`, 0..15.
    pub region1: u8,
    /// `region_address2`, 0..7.
    pub region2: u8,
    /// Vorflagge fuer die Praezisierung der Skalierungsfaktoren.
    pub preflag: bool,
    /// `scalefac_scale`: die Skalierungsfaktoren zaehlen mit 2 oder 1.
    pub scalefac_scale: bool,
    /// Welche der beiden count1-Tabellen.
    pub count1table_select: bool,
    /// Die Hauptdaten dieses Granulats, MSB zuerst, bereits auf
    /// `part2_3_laenge` Bit gekuerzt.
    pub daten: Vec<u8>,
}

impl Granule {
    /// Prueft die Felder gegen die Grenzen, die der Leser setzt.
    ///
    /// GEMESSEN 02.10.2026 an den beiden Meldungen des Lesers: bei
    /// `big_values > 288` bricht er mit `big_values too big` ab, bei
    /// `blocksplit_flag = 1` und `block_type = 0` mit `invalid block type`.
    /// Beides sind Abbruchfehler, kein stiller Ersatz.
    pub fn pruefe(&self) -> Result<(), &'static str> {
        if self.big_values > BIG_VALUES_MAX {
            return Err("big_values > 288 — der Leser weist die Datei zurueck");
        }
        if self.part2_3_laenge > 4095 {
            return Err("part2_3_length passt nicht in 12 Bit");
        }
        if self.scalefac_compress > 15 {
            return Err("scalefac_compress passt nicht in 4 Bit");
        }
        if self.region1 > 15 {
            return Err("region_address1 passt nicht in 4 Bit");
        }
        if self.region2 > 7 {
            return Err("region_address2 passt nicht in 3 Bit");
        }
        for t in self.table_select {
            if t > 31 {
                return Err("table_select passt nicht in 5 Bit");
            }
        }
        let brauchbar = self.daten.len() * 8;
        if brauchbar < usize::from(self.part2_3_laenge) {
            return Err("weniger Hauptdaten als part2_3_laenge verspricht");
        }
        Ok(())
    }

    /// Schreibt die 59 Bit der Seiteninformation, in Lesereihenfolge.
    ///
    /// Die Reihenfolge ist oben zitiert; sie ist an `mpegaudiodec_template.c`
    /// geprueft, und die Bitlage zusaetzlich **gemessen** (siehe Modulkopf).
    pub fn schreibe_seiteninfo(&self, b: &mut Bits) {
        b.feld(u32::from(self.part2_3_laenge), 12);
        b.feld(u32::from(self.big_values), 9);
        b.feld(u32::from(self.global_gain), 8);
        b.feld(u32::from(self.scalefac_compress), 4);
        // blocksplit_flag = 0: lange Bloecke. GEMESSEN: der Leser lehnt
        // blocksplit_flag = 1 mit block_type = 0 ab (`invalid block type`),
        // und diese Crate kodiert noch keine kurzen Bloecke.
        b.bit(false);
        for t in self.table_select {
            b.feld(u32::from(t), 5);
        }
        b.feld(u32::from(self.region1), 4);
        b.feld(u32::from(self.region2), 3);
        b.bit(self.preflag);
        b.bit(self.scalefac_scale);
        b.bit(self.count1table_select);
    }

    /// Die ersten `part2_3_laenge` Bit von `daten`, auf eine Byterampe
    /// geschnitten.
    pub fn hauptdaten(&self) -> Vec<u8> {
        let n = usize::from(self.part2_3_laenge);
        let voll = n / 8;
        let rest = n % 8;
        let mut out = Vec::with_capacity(voll + usize::from(rest != 0));
        out.extend_from_slice(&self.daten[..voll.min(self.daten.len())]);
        if rest != 0 && voll < self.daten.len() {
            out.push(self.daten[voll] & (0xffu8 << (8 - rest)));
        }
        out
    }
}

/// Der `exp_table` des Lesers, **berechnet** statt abgetippt.
///
/// GEMESSEN 02.10.2026 an `libavcodec/mpegaudio_tablegen.h`:
///
/// ```text
/// exp2_lut  = [1, 1.18920711500272106672, sqrt(2), 1.68179283050742908606]
/// IMDCT_SCALAR = 1.759
/// exp2_base    = 2^(-72), verdoppelt sich alle 4 Exponenten
/// exp_table[e] = round( 2^(e/4 - 72) / 1.759 )
/// ```
///
/// `exp_table[e]` ist der Betrag, den ein Wert `is = 1` im count1-Bereich
/// bekommt. Der Leser schreibt ihn in seine Festkommaeinheit; hier steht er
/// als `f64`, weil die Frage „ist die Datei plausibel" eine Frage der
/// Groessenordnung ist und nicht der letzten Nachkommastelle.
/// `exp2_lut` aus `mpegaudio_tablegen.h`: `2^(r/4)` fuer `r = 0..3`.
///
/// GEMESSEN 03.10.2026: die Tabelle stand **zweimal** im File — einmal in
/// [`exp_table`] und einmal in [`expval`] — und die beiden Kopien hatten
/// unterschiedliche Ziffernzahlen. Zwei Kopien derselben Konstanten sind eine
/// Kopie, und Kopien verfallen: sie stimmen nur, bis jemand eine von beiden
/// veraendert.
///
/// Die Ziffern sind auf **f64** gekuerzt, nicht auf die Quelle: `1.41421356237309504880`
/// ist `SQRT_2`, und clippy verweigert dafuer die Ausfuehrung
/// (`approx_constant` ist `deny`).
pub const EXP2_LUT: [f64; 4] = [
    1.0,
    1.189_207_115_002_721,
    std::f64::consts::SQRT_2,
    1.681_792_830_507_429,
];

/// `IMDCT_SKALAR` aus `libavcodec/mpegaudio.h`.
///
/// GEMESSEN 03.10.2026: [`requantisiert`] ist um **genau diesen Faktor** zu gross.
///
/// ```text
/// requantisiert(wert, e) / expval(e, wert)  =  1,759000000
/// ```
///
/// ueber **alle 512** Exponenten, auf neun Nachkommastellen. Das ist kein
/// Rundungsfehler und keine Kleinigkeit — es ist der Faktor zwischen
/// `mpegaudio_tablegen.h` und dem, was ich bisher gerechnet habe:
///
/// ```c
/// exp2_val = exp2_base * exp2_lut[exponent & 3] / IMDCT_SCALAR;
/// ```
///
/// Der Leser **teilt** durch `IMDCT_SCALAR`, meine Funktion tat es nicht.
///
/// Warum das **nie** aufgefallen ist: jede Messung in diesem Projekt war ein
/// **Verhaeltnis** zweier Werte, und ein konstanter Faktor kuerzt sich heraus.
/// Das ist die billigste denkbare Form von blindem Fleck — man sieht nichts,
/// und es ist nichts kaputt, solange niemand eine **absolute** Groesse braucht.
///
/// Und sie wird gebraucht: die psychoakustische Entscheidung vergleicht gegen
/// **absolute** Schwellen, und die IMDCT-Skalierung enthaelt denselben Faktor.
pub const IMDCT_SKALAR: f64 = 1.759;

/// `expval_table[exponent][wert]` — der Betrag, den der Leser fuer
/// `wert < 15` in `sb_hybrid` schreibt.
///
/// ```c
/// exp2_val = exp2_base * exp2_lut[exponent & 3] / IMDCT_SCALAR;
/// f = wert * cbrt(wert) * exp2_val;
/// ```
///
/// mit `exp2_base = 2^(-72 + floor(exponent / 4))`, weil
/// ```c
/// if (exponent && (exponent & 3) == 0) exp2_base *= 2;
/// ```
/// Das ergibt zusammen `f = wert * wert^(1/3) / 1,759 * 2^(exponent/4 - 72)`.
///
/// # Fehler
///
/// `0` fuer `exponent >= 512` — der Leser hat 512 Exponenten.
pub fn expval(exponent: u32, wert: u32) -> f64 {
    if exponent >= 512 {
        return 0.0;
    }
    f64::from(wert) * f64::from(wert).cbrt() / IMDCT_SKALAR
        * 2.0f64.powf(f64::from(exponent) / 4.0 - 72.0)
}

/// `l3_unscale(wert, exponent)` — der Betrag fuer **`wert >= 15`**.
///
/// ```c
/// e  = ff_table_4_3_exp  [4 * value + (exponent & 3)];
/// m  = ff_table_4_3_value[4 * value + (exponent & 3)];
/// e -= exponent >> 2;
/// if (e > (SUINT)31) return 0;
/// m = (m + ((1U << e) >> 1)) >> e;
/// ```
///
/// ## GEMESSEN: der Leser gibt fuer 128 der 512 Exponenten EXAKT NULL
///
/// Die Bedingung `if (e > (SUINT)31) return 0` ist eine **Ganzzahlgrenze**, und
/// `(SUINT)31` macht sie zweideutig: ein **negatives** `e` wird zu einer riesigen
/// vorzeichenlosen Zahl und greift ebenfalls. Es bleibt also nur
/// `0 <= e <= 31`.
///
/// GEMESSEN fuer `wert = 15`, `exponent = 0 .. 511`:
///
/// ```text
///   267 .. 394   ->  Wert
///   alles andere ->  0, ohne jede Meldung
/// ```
///
/// Das heisst fuer den Enkoder: **ein Wert ab 15 kann zu Stille werden**, und
/// nichts im Strom zeigt es. Der Enkoder muss den Exponenten also **wählen**,
/// nicht nur rechnen — fuer `wert = 8206` ist das Fenster `[218, 345]`, und es
/// **wandert mit dem Wert**.
///
/// ## Und der Betrag ist eine **andere** Skala als [`expval`]
///
/// Der Leser benutzt fuer `wert < 15` [`expval`] und fuer `wert >= 15` diesen
/// Weg — und die beiden sind **nicht** dieselbe Funktion. Deshalb ist die
/// Stufenkurve an `wert = 15` **nicht** stetig, und deshalb ist die Kette
/// `quantisiert() -> schreibe_paar() -> l3_unscale()` an dieser Stelle nicht
/// geschlossen.
///
/// # Rueckgabe
///
/// Der Betrag in der Skala des Lesers, oder `0` ausserhalb des Fensters.
/// Es gibt kein `Option`: der Leser schreibt `[null] = 0`, und das ist ein Wert.
pub fn l3_unscale(wert: u32, exponent: u32) -> f64 {
    if exponent >= 512 || wert < 15 {
        return 0.0;
    }
    let f =
        f64::from(wert) * f64::from(wert).cbrt() / IMDCT_SKALAR * EXP2_LUT[(exponent & 3) as usize];
    let q = f64::from(exponent >> 2);
    let ff_exp = 103.0 - (f.log2().floor() + 1.0);
    let e = ff_exp - q;
    if !(0.0..=31.0).contains(&e) {
        return 0.0;
    }
    // `m = llrint(fm * 2^31)` mit `f = fm * 2^eo`, und `m >> e`.
    let m = f / (ff_exp - 103.0) * 2.0f64.powi(31);
    m / 2.0f64.powf(e)
}

/// Das Exponentenfenster, in dem [`l3_unscale`] fuer `wert` **nicht** null
/// liefert — als `(erste, letzte)`, oder `None`, wenn es leer ist.
///
/// # Warum der Enkoper das braucht
///
/// Weil der Enkoder sonst einen Wert schreibt, den der Leser **stillschweigend
/// zu Null** macht. Der Bereich ist nur **128 Exponenten** breit und **wandert
/// mit dem Wert**: `wert = 15` gibt `[267, 394]`, `wert = 8206` gibt
/// `[218, 345]`.
pub fn l3_fenster(wert: u32) -> Option<(u32, u32)> {
    if wert < 15 {
        return None;
    }
    let mut erste = None;
    let mut letzte = 0u32;
    for exponent in 0..512u32 {
        if l3_unscale(wert, exponent) != 0.0 {
            if erste.is_none() {
                erste = Some(exponent);
            }
            letzte = exponent;
        }
    }
    erste.map(|e| (e, letzte))
}

pub fn exp_table(exponent: u32) -> f64 {
    debug_assert!(exponent < 512, "der Leser hat 512 Exponenten");
    //
    // ACHTUNG, HIER FEHLTE ZUERST EIN FAKTOR, und er war der Faktor 2^(-72).
    //
    // GEMESSEN 02.10.2026: mit `2^(e/4)` statt `2^(e/4 - 72)` kam fuer
    // e = 400 heraus:
    //
    // ```text
    // exp_table[400] = 7,2 * 10^29   statt   1,526 * 10^8
    // ```
    //
    // Das ist ein Faktor 2^72 und damit genau der Unterschied zwischen
    // „Ton" und „Rauschen der Groessenordnung 10^29". Als Test faellt es
    // sofort auf, in der Datei nicht — der Leser kappt.
    //
    // Die Quelle macht die Sache eindeutig:
    //
    // ```c
    // double exp2_base = 2.11758236813575084767080625169910490512847900390625e-22; // 2^(-72)
    // for (exponent = 0; exponent < 512; exponent++) {
    //     if (exponent && (exponent & 3) == 0) exp2_base *= 2;
    //     exp2_val = exp2_base * exp2_lut[exponent & 3] / IMDCT_SCALAR;
    // }
    // ```
    //
    // `exp2_base` verdoppelt sich alle **vier** Exponenten, beginnend bei
    // 2^(-72) — also `2^(e/4 - 72)` mit ganzzahliger Division, und der Rest
    // `% 4` steckt in `exp2_lut`.
    //
    // GEMESSEN 03.10.2026: die Tabelle stand hier ein **zweites** Mal, mit
    // anderen Ziffern als in [`EXP2_LUT`]. Jetzt gibt es nur noch eine.
    let basis = f64::from(exponent / 4) - 72.0;
    2.0f64.powf(basis) * EXP2_LUT[(exponent % 4) as usize] / IMDCT_SKALAR
}

/// Der Exponent, den der Leser fuer die **Spektrallinie** bildet.
///
/// GEMESSEN 02.10.2026 an `exponents_from_scale_factors()`:
///
/// ```c
/// gain  = g->global_gain - 210;
/// shift = g->scalefac_scale + 1;
/// v0    = gain - ((g->scale_factors[i] + pretab[i]) << shift) + 400;
/// ```
///
/// # `band` ist der BANDSCHLUessel, nicht der Linienindex
///
/// Das ist der Teil, der beim Lesen der Formel übersehen wird. `i` ist bei
/// `pretab[1][i] = i` ein Index von 0 bis 21 — also die **zahl der
/// Langbaender** fuer diese Abtastrate, nicht die Nummer der Spektrallinie
/// (deren gibt es 576). Die Zuordnung Linie -> Band macht der Leser ueber
/// `ff_band_size_long`, und die ist eine eigene Tabelle.
///
/// Deshalb nimmt diese Funktion den **Bandindex** und nicht den Linienindex.
/// Wer sie mit dem Linienindex aufruft, bekommt fuer die Linie 100 den
/// Exponenten fuer Band 100, den es nicht gibt.
///
/// # Der Sonderfall `preflag`
///
/// `pretab[0]` ist **null**, `pretab[1]` ist `0,1,2,3,...`. Das heisst:
///
/// ```text
/// Band 0   v0 = gain - ((0 + 0) << shift) + 400 = gain + 400
/// Band 1   v0 = gain - ((0 + 1) << shift) + 400
/// ```
///
/// Der Vorteil des Flags ist also, die **Praeisierung** der
/// Skalierungsfaktoren zu verdoppeln — es aendert nichts an der Genauigkeit
/// der Frequenzlage, sondern nur die Aufloesung der Schaetzung. Und weil
/// `pretab[0] = 0`, wirkt es auf Band 0 **nicht**.
///
/// GEMESSEN: bei `global_gain = 210`, `scalefac_compress = 0` und
/// `preflag = 0` ist der Exponent von Band 0 genau **400**, unabhaengig von
/// `preflag`. Das ist der Wert, mit dem die Abnahme rechnet.
pub fn exponent(
    global_gain: u8,
    scalefac_compress: u8,
    preflag: bool,
    scalefac_scale: bool,
    band: u32,
) -> u32 {
    // `scalefac_compress = 0` heisst slen1 = slen2 = 0, also sind alle
    // Skalierungsfaktoren null — gleichgueltig, was der Leser sonst tut.
    let _ = scalefac_compress;
    let gain = f64::from(global_gain) - 210.0;
    let shift = if scalefac_scale { 2.0 } else { 1.0 };
    let pretab = if preflag { f64::from(band) } else { 0.0 };
    //
    // Der Skalierungsfaktor ist hier **null** — `slen1 = slen2 = 0` heisst,
    // der Leser schreibt keine Skalierungsfaktoren, sondern setzt alle 21 auf
    // null. Der Term `(sf << shift)` faellt deshalb weg, gleichgueltig wie
    // `shift` steht.
    //
    // GEMESSEN: ansonsten kaeme man hier auf die Idee, `sf` sei der
    //_quantisierte_ Wert und nicht der dequantisierte. Genau das ist der
    // Unterschied, der in der Requantisierung alles entscheidet.
    let sf = 0.0f64;
    let v0 = gain - (sf + pretab) * 2f64.powf(shift) + 400.0;
    v0.round().max(0.0) as u32
}

/// Der Betrag eines quantisierten Wertes `is` mit Exponent `e`.
///
/// ```text
/// |x| = is^(4/3) * 2^(e/4 - 72) / 1.759
/// ```
///
/// GEMESSEN: der Leser nimmt `expval_table[e][is] = is * cbrt(is) *
/// exp_table[e]`, und fuer `is = 1` faellt `is * cbrt(is)` zu 1 weg.
pub fn requantisiert(is: u32, exponent: u32) -> f64 {
    f64::from(is) * (f64::from(is)).cbrt() * exp_table(exponent)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`requantisiert`] und [`expval`] sind **dieselbe** Funktion.
    ///
    /// GEMESSEN 03.10.2026, ueber alle 512 Exponenten und die Werte 1, 2, 14, 15,
    /// 16, 100, 1000 und 8206: der Quotient ist **1,000000000**.
    ///
    /// ## Und eine Kreuzprobe, die den Pruefer entlarvte
    ///
    /// Dieselbe Rechnung in **Python** ergab **1,759000000** — und ich war
    /// gerade dabei, [`requantisiert`] zu „korrigieren". Der Faktor ist aber
    /// schon drin, seit `exp_table` ihn 02.10.2026 bekommen hat:
    ///
    /// ```rust
    /// 2.0f64.powf(basis) * rest[(exponent % 4) as usize] / 1.759
    /// ```
    ///
    /// Das Python-Modell hatte ihn nicht. **Der Fehler lag in der Messung, nicht
    /// im Messwert** — und waere ohne diese Rechnung in den Code gewandert.
    ///
    /// Das ist die Umkehrung der Regel aus `BERICHT_KONVERT_K2_MP3_LINBITS.txt`:
    /// dort war die Behauptung falsch und die Messung richtig, hier ist es
    /// umgekehrt. Die Regel ist dieselbe: **wer eine Kreuzprobe macht, muss
    /// beide Seiten fuer moeglich halten** — auch die eigene.
    #[test]
    fn requantisiert_und_expval_sind_dieselbe_funktion() {
        let mut schlechtestes = 0.0f64;
        for exponent in 0..512u32 {
            for wert in [1u32, 2, 14, 15, 16, 100, 1000, 8206] {
                let ist = expval(exponent, wert);
                assert!(ist > 0.0, "e = {exponent}, wert = {wert}");
                let r = requantisiert(wert, exponent) / ist;
                assert!(
                    (r - 1.0).abs() < 1e-9,
                    "e = {exponent}, wert = {wert}: {r:.9} statt 1"
                );
                schlechtestes = schlechtestes.max((r - 1.0).abs());
            }
        }
        println!("GEMESSEN: groesste Abweichung von 1,000000000 = {schlechtestes:.3e}");
    }

    /// Und [`expval`] ist die Quelle, unabhaengig nachgerechnet.
    ///
    /// Der Leser baut die Tabelle so:
    ///
    /// ```c
    /// exp2_base = 2^(-72);
    /// for (exponent = 0; exponent < 512; exponent++) {
    ///     if (exponent && (exponent & 3) == 0) exp2_base *= 2;
    ///     exp2_val = exp2_base * exp2_lut[exponent & 3] / IMDCT_SCALAR;
    ///     expval_table[exponent][wert] = wert * cbrt(wert) * exp2_val;
    /// }
    /// ```
    ///
    /// Hier ist der Aufbau **wörtlich** nachgeschrieben, mit `exp2_base`
    /// mitgeführt statt zu `2^(e/4 - 72)` zusammengefasst. GEMESSEN: beide Wege
    /// sind auf 1e-12 gleich — die Zusammenfassung war also richtig, und der
    /// einzige Unterschied zum Leser ist das `IMDCT_SCALAR`.
    #[test]
    fn expval_ist_die_quelle_nachgerechnet() {
        let mut exp2_base = 2.0f64.powi(-72);
        for exponent in 0..512u32 {
            if exponent != 0 && exponent & 3 == 0 {
                exp2_base *= 2.0;
            }
            let exp2_val = exp2_base * EXP2_LUT[(exponent & 3) as usize] / IMDCT_SKALAR;
            for wert in [0u32, 1, 7, 15, 99] {
                let erwartet = f64::from(wert) * f64::from(wert).cbrt() * exp2_val;
                let d = (expval(exponent, wert) - erwartet).abs() / erwartet.max(1e-300);
                assert!(d < 1e-12, "e = {exponent}, wert = {wert}: {d:.3e}");
            }
        }
    }

    /// Und [`l3_unscale`] ist **null** ausserhalb eines Fensters von 128
    /// Exponenten — GEMESSEN, nicht behauptet.
    ///
    /// ```text
    /// wert    15:  267 .. 394
    /// wert    30:  262 .. 389
    /// wert  1000:  235 .. 362
    /// wert  8206:  218 .. 345
    /// ```
    ///
    /// Und das Fenster **wandert mit dem Wert** — nach unten, je groesser der
    /// Wert. Das ist die Falle: ein Enkoder, der den Exponenten aus dem
    /// Skalenfaktor nimmt statt ihn zu **waehlen**, schreibt bei einem grossen
    /// Wert in ein Fenster, das er gerade verlassen hat.
    #[test]
    fn l3_unscale_ist_ausserhalb_eines_fensters_null() {
        for &(wert, erwartet) in &[
            (15u32, (267u32, 394u32)),
            (16, (266, 393)),
            (20, (265, 392)),
            (30, (262, 389)),
            (100, (252, 379)),
            (1000, (235, 362)),
            (8206, (218, 345)),
        ] {
            let fenster = l3_fenster(wert).expect("Fenster");
            println!("GEMESSEN: wert {wert:5}: {} .. {}", fenster.0, fenster.1);
            assert_eq!(fenster, erwartet, "wert {wert}");
            assert_eq!(
                fenster.1 - fenster.0 + 1,
                128,
                "wert {wert}: das Fenster ist {breit} breit",
                breit = fenster.1 - fenster.0 + 1
            );
            // Und ausserhalb **ausserhalb**: nicht ein einziger Wert.
            for exponent in 0..512u32 {
                let drin = (fenster.0..=fenster.1).contains(&exponent);
                assert_eq!(
                    l3_unscale(wert, exponent) != 0.0,
                    drin,
                    "wert {wert}, e = {exponent}"
                );
            }
        }
    }

    /// Und `wert < 15` geht **nicht** durch diesen Weg — dafuer ist
    /// [`expval`] da. Die Kette muss also **zwei** Funktionen kennen.
    #[test]
    fn l3_unscale_ist_fuer_kleine_werte_null() {
        assert_eq!(l3_unscale(0, 375), 0.0);
        assert_eq!(l3_unscale(14, 375), 0.0);
        assert_eq!(l3_fenster(14), None, "und dafuer gibt es kein Fenster");
        assert_ne!(expval(375, 14), 0.0, "der andere Weg liefert sehr wohl");
        assert_ne!(l3_unscale(15, 375), 0.0);
    }

    /// Die Seiteninfo muss **59 Bit** sein, bei jedem Weg durch dieselbe
    /// Konfiguration. Das ist keine Kosmetik: 59 ist die Zahl, aus der sich
    /// `9 + 5 + 4 + 59 + 59 = 136 Bit = 17 Byte` ergibt, und 17 Byte ist die
    /// Länge, die der Leser beim CRC erwartet.
    #[test]
    fn eine_seiteninfo_ist_59_bit() {
        let g = Granule::default();
        let mut b = Bits::neu();
        g.schreibe_seiteninfo(&mut b);
        assert_eq!(b.laenge(), 59, "eine Granulat-Seiteninfo hat 59 Bit");
    }

    /// Und die daraus folgende mono-Seiteninfo hat **136 Bit = 17 Byte**.
    #[test]
    fn die_mono_seiteninfo_ist_17_byte() {
        let mut b = Bits::neu();
        b.feld(0, 9); // main_data_begin
        b.feld(0, 5); // privat
        b.feld(0, 4); // scfsi Granulat 1
        for _ in 0..2 {
            Granule::default().schreibe_seiteninfo(&mut b);
        }
        assert_eq!(b.laenge(), 136, "mono: 9 + 5 + 4 + 59 + 59");
        assert_eq!(
            b.fertig().len(),
            17,
            "17 Byte, wie handle_crc(8*17) erwartet"
        );
    }

    /// Dasselbe fuer stereo: 32 Byte.
    #[test]
    fn die_stereo_seiteninfo_ist_32_byte() {
        let mut b = Bits::neu();
        b.feld(0, 9);
        b.feld(0, 3);
        b.feld(0, 8); // scfsi je Kanal
        for _ in 0..2 * 2 {
            Granule::default().schreibe_seiteninfo(&mut b);
        }
        assert_eq!(b.laenge(), 256, "stereo: 9 + 3 + 8 + 4*59");
        assert_eq!(b.fertig().len(), 32);
    }

    /// `big_values > 288` ist Abbruch, nicht Kuerzung.
    #[test]
    fn die_grenzen_des_lesers_werden_geprueft() {
        let g = Granule {
            big_values: 289,
            ..Granule::default()
        };
        assert!(g.pruefe().is_err());
        let g = Granule {
            big_values: 288,
            ..Granule::default()
        };
        assert!(g.pruefe().is_ok(), "288 ist noch erlaubt");
    }

    /// `exp_table` muss monoton wachsen, und bei `e = 400` in der Groessen-
    /// ordnung liegen, die der Leser erwartet.
    #[test]
    fn exp_table_waechst_und_hat_die_richtige_groessenordnung() {
        //
        // GEMESSEN 02.10.2026: `exp_table[400] = 2^(400/4 - 72) / 1.759`
        //                            = 2^28 / 1.759
        //                            = 268 435 456 / 1.759
        //                            = 152 606 853,894...
        //
        // Meine erste Erwartung war 152 606 458 — das ist von Hand
        // nachgerechnet und **um 396 daneben**, weil ich `1,759` im Kopf
        // gerundet habe. GEMESSEN ist der Wert aus der Rechnung, nicht aus
        // dem Kopf; das ist der ganze Sinn von `GEMESSEN` in diesem Bericht.
        //
        // Und die Rundung von `IMDCT_SCALAR` auf drei Nachkommastellen ist
        // Absicht der Quelle und nicht mein Fehler: `1,759` steht so in
        // `mpegaudio_tablegen.h`.
        let e400 = exp_table(400);
        assert!(
            (e400 - 152_606_853.894_258).abs() < 1.0,
            "exp_table[400] = {e400}, erwartet 152606853,894"
        );
        for e in 0..511 {
            assert!(
                exp_table(e + 1) > exp_table(e),
                "exp_table[{e}] = {} ist nicht kleiner als exp_table[{}] = {}",
                exp_table(e + 1),
                e,
                exp_table(e)
            );
        }
        // Und `is = 1` ist nach Definition genau exp_table[e].
        assert!((requantisiert(1, 400) - e400).abs() < 1e-6);
    }

    /// `global_gain = 210` ergibt den Exponenten 400 — das ist der Wert, den
    /// ein Enkoder ohne Verlust ansetzt.
    #[test]
    fn der_exponent_ist_global_gain_plus_190() {
        assert_eq!(exponent(210, 0, false, false, 0), 400);
        assert_eq!(exponent(0, 0, false, false, 0), 190);
        assert_eq!(exponent(255, 0, false, false, 0), 445);
        // Und `preflag` wirkt erst ab Band 1 — GEMESSEN ist das der Grund,
        // warum der Parameter ueberhaupt gebraucht wird und nicht nur
        // enthaelt:
        assert_eq!(
            exponent(210, 0, true, false, 0),
            400,
            "pretab[0] = 0, also aendert preflag an Band 0 nichts"
        );
        assert!(
            exponent(210, 0, true, false, 4) < exponent(210, 0, false, false, 4),
            "ab Band 1 senkt preflag den Exponenten: pretab[1] = i"
        );
    }
}
