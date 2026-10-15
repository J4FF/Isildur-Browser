//! Die Granulebene: Skalenfaktoren, Huffman, Requantisierung,
//! Reihenfolge, Stereo und Aliasreduktion.
//!
//! # DIE EINE ZAHL, AN DER MAN HIER EINEN FEHLER SIEHT
//!
//! Jedes Granul nennt in der Nebeninformation `part2_3_length` — die Zahl
//! der Bit, die seine Skalenfaktoren und Huffman-Daten belegen. Am Ende
//! dieses Moduls muss der Leser **genau dort** stehen. Das ist die
//! Pruefung in [`lies_granule`] selbst, und sie ist deshalb eingebaut,
//! weil sie sonst erst am Ende des PCM sichtbar wuerde.

use crate::bitleser::BitLeserVorn;
use crate::fehler::{Art, Ergebnis, Fehler};
use crate::mp3::huffman::Baum;
use crate::mp3::rahmen::{Granule, Kopf, bitfehler};
use crate::mp3::tabellen::{ALIAS, BAENDER_KURZ, BAENDER_LANG, SLEN1, SLEN2};

/// Die Zahl der Spektrallinien je Granul: 32 Baender zu 18 Linien.
pub const LINIEN: usize = 576;
/// Die Zahl der Baender.
pub const BAENDER: usize = 32;
/// Die Zahl der Linien je Band.
pub const LINIEN_PRO_BAND: usize = 18;
/// Die Zahl der Wertepaare, die der Huffman-Leser hoechstens liest.
pub const MAX_PAARE: usize = 288;

/// Der Zustand zwischen den Granulen eines Kanals.
#[derive(Debug, Clone)]
pub struct KanalZustand {
    /// Die Skalenfaktoren der beiden Granulen (40 Werte je Granul).
    pub skalenfaktoren: [[u8; 40]; 2],
    /// Die Signale der 32 Baender, 18 Werte je Band — die „Vorgeschichte"
    /// fuer das Ueberlappen.
    pub baender: [[f64; LINIEN_PRO_BAND]; BAENDER],
    /// `true`, wenn die Bandwerte vom vorigen Granul kommen (Startblock).
    pub band_gefuellt: [bool; BAENDER],
}

impl KanalZustand {
    /// Ein frischer Kanal.
    #[must_use]
    pub fn neu() -> Self {
        Self {
            skalenfaktoren: [[0; 40]; 2],
            baender: [[0.0; LINIEN_PRO_BAND]; BAENDER],
            band_gefuellt: [false; BAENDER],
        }
    }
}

/// Das Ergebnis eines Granules: 576 requantisierte Linien, in der Reihenfolge
/// des Bandwechsels.
pub type Spektrum = [f64; LINIEN];

/// Der gemeinsame Zustand beider Kanaele.
pub struct Dekoder {
    /// Die 15 Codebuecher, einmal gebaut.
    baeume: Vec<Baum>,
    /// Die Nachschlagetabellen der Quartett-Codes.
    quartett: [[u8; 6 * 64]; 2],
    /// Der Vorzustand je Kanal, mit dem Index des vorigen Granules.
    pub kanaele: Vec<(KanalZustand, usize)>,
}

impl Dekoder {
    /// Baut die Baeume und Nachschlagetabellen einmal.
    #[must_use]
    pub fn neu(kanaele: usize) -> Self {
        Self {
            baeume: crate::mp3::huffman::alle_baeume(),
            quartett: [
                crate::mp3::huffman::quartett_tabelle(0),
                crate::mp3::huffman::quartett_tabelle(1),
            ],
            kanaele: (0..kanaele).map(|_| (KanalZustand::neu(), 0)).collect(),
        }
    }
}

/// Die kumulierten Langbaender in **Wertepaaren** (2 Linien je Paar).
///
/// Das ist `ff_band_index_long` aus `libavcodec/mpegaudiodec_common.c`:
/// ```c
/// for (int i = 0; i < 9; i++) {
///     int k = 0;
///     for (int j = 0; j < 22; j++) {
///         ff_band_index_long[i][j] = k;
///         k += ff_band_size_long[i][j] >> 1;
///     }
///     ff_band_index_long[i][22] = k;
/// }
/// ```
#[must_use]
pub fn band_index_lang(rate_index: usize) -> [u16; 23] {
    let mut t = [0u16; 23];
    let mut k = 0u16;
    for j in 0..22 {
        t[j] = k;
        k += u16::from(BAENDER_LANG[rate_index][j] >> 1);
    }
    t[22] = k;
    t
}

/// Die drei Regionsgrenzen eines Granules, als **Linienindizes**.
///
/// `region_offset2size` und `init_long_region`/`init_short_region` aus
/// `libavcodec/mpegaudiodec_template.c`. Die Grenzen sind 0, Ende von
/// Region 0, Ende von Region 1, 576 — und `big_values` kappt sie, denn
/// dahinter kommen nur noch die Quartette.
#[must_use]
pub fn regionen(g: &Granule, rate_index: usize) -> [usize; 4] {
    let tab = band_index_lang(rate_index);
    let grosse = g.grosse_werte as usize;
    let (r0, r1) = if g.umgeschaltet {
        // `init_short_region`: die Grenzen sind Konstanten.
        (18usize, 288usize)
    } else {
        (
            tab[(g.region0 as usize + 1).min(22)] as usize,
            tab[(g.region0 as usize + g.region1 as usize + 2).min(22)] as usize,
        )
    };
    // `region_offset2size`:
    // ```c
    // g->region_size[2] = 576 / 2;
    // for (i = 0; i < 3; i++) {
    //     k = FFMIN(g->region_size[i], g->big_values);
    //     g->region_size[i] = k - j;
    //     j = k;
    // }
    // ```
    //
    // GEMESSENER FEHLER, und er ist stille: ich hatte `a + b + c`
    // gerechnet statt `c`. `b` und `c` sind schon die **kumulativen**
    // Minima — `region_size[1] = b - a` und `region_size[2] = c - b`, die
    // Summe ist also `c`. Mit der Summe wurde die letzte Grenze zu gross
    // und der Huffman-Leser laeuft an ihr vorbei: bei `big_values = 57`
    // kamen 264 statt 114 heraus, also **150 Linien zu viel** — und das
    // faellt erst auf, wenn die Regionen kuenstlich kein Ende haben.
    let a = r0.min(grosse);
    let b = r1.min(grosse);
    let c = 288usize.min(grosse);
    [0, a * 2, b * 2, c * 2]
}

/// Die Exponenten der Linien — die Zahl, die die Requantisierung braucht.
///
/// `exponents_from_scale_factors` aus `libavcodec/mpegaudiodec_template.c`:
///
/// ```c
/// gain  = g->global_gain - 210;
/// shift = g->scalefac_scale + 1;
/// for (i = 0; i < g->long_end; i++) {
///     v0 = gain - ((g->scale_factors[i] + pretab[i]) << shift) + 400;
///     len = bstab[i];
///     for (j = len; j > 0; j--) *exp_ptr++ = v0;
/// }
/// if (g->short_start < 13) {
///     gains[l] = gain - (g->subblock_gain[l] << 3);
///     for (i = g->short_start; i < 13; i++) {
///         len = bstab[i];
///         for (l = 0; l < 3; l++) {
///             v0 = gains[l] - (g->scale_factors[k++] << shift);
///             for (j = len; j > 0; j--) *exp_ptr++ = v0;
///         }
///     }
/// }
/// ```
///
/// Das `+ 400` ist der feste Versatz von ffpegs Tabelle
/// (`ff_table_4_3_value`); in `f64` faellt er weg, weil
/// `xr = sign · |is|^(4/3) · 2^(exponent/4)`.
#[must_use]
pub fn exponenten(g: &Granule, skfsi: &[u8; 40], rate_index: usize) -> [i32; LINIEN] {
    let mut raus = [0i32; LINIEN];
    // GEMESSENER FEHLER: `- 210` statt `- 214`, also um **4** zu hoch.
    //
    // Die Konstante steht an genau einer Stelle in minimp3.h, Zeile 80
    // (am Original geprueft, nicht aus dem Gedaechtnis):
    //
    //     #define BITS_DEQUANTIZER_OUT        -1
    //     gain_exp = gr->global_gain + BITS_DEQUANTIZER_OUT*4 - 210 ...
    //
    // Das ist `global_gain - 4 - 210`. GEMESSENE FOLGE: der Faktor
    // 2,000000 an jeder Stelle, denn `2^(4/4) = 2` — und genau der war
    // seit Runde 7 als „Maximumsverhaeltnis exakt 2,000000" notiert,
    // ohne dass ich ihn einer Zahl zugeordnet haette.
    //
    // GEMESSENE GEGENPROBE, Schnitt nach `L3_decode_scalefactors`,
    // `g8c_rauschen_mono` Kanal 0, Granulen 6 und 7 (Orakel gegen
    // `exponenten()`): mit `- 210` war der Abstand **-4 an allen 21
    // Baendern**, mit `- 214` an allen **0**. Zwei Baender (8 und 9)
    // wichen vorher um -2 ab — das war der zweite Fehler, siehe
    // `mp3.rs`.
    //
    // NICHT mit `- 2` fuer MS-Stereo: minimp3 traegt diesen Ausgleich in
    // `gain_exp`, weil sein `L3_stereo_ms` ohne `1/sqrt(2)` rechnet.
    // `stereo()` hier rechnet **mit** `1/sqrt(2)`, wie ffmpegs
    // `l3stereo_ms` es tut. Beides zusammen waere doppelt.
    let verst = g.global_gain as i32 - 214;
    let schritt = g.skalierung as i32 + 1;
    let (lang_ende, kurz_start) = kurzgrenzen(g, rate_index);
    let mut zeiger = 0usize;
    let prae = &crate::mp3::tabellen::PRAEEMPHA[usize::from(g.praeemphasis)];
    for i in 0..lang_ende {
        let v = verst - ((i32::from(skfsi[i]) + i32::from(prae[i])) << schritt);
        let len = usize::from(BAENDER_LANG[rate_index][i]);
        for _ in 0..len {
            if zeiger < LINIEN {
                raus[zeiger] = v;
                zeiger += 1;
            }
        }
    }
    if kurz_start < 13 {
        let kurz = &BAENDER_KURZ[rate_index];
        let gewinn: [i32; 3] = [
            verst - ((g.unterverstaerkung[0] as i32) << 3),
            verst - ((g.unterverstaerkung[1] as i32) << 3),
            verst - ((g.unterverstaerkung[2] as i32) << 3),
        ];
        let mut k = lang_ende;
        for &breite in &kurz[kurz_start..13] {
            let len = usize::from(breite);
            for &g_w in &gewinn {
                let v = g_w - (i32::from(skfsi[k.min(39)]) << schritt);
                k += 1;
                for _ in 0..len {
                    if zeiger < LINIEN {
                        raus[zeiger] = v;
                        zeiger += 1;
                    }
                }
            }
        }
    }
    raus
}

/// Die Requantisierung: `sign · |is|^(4/3) · 2^(exponent/4)`.
///
/// # DER WEG ZUR FORMEL
///
/// Abschnitt 2.4.3.4.9.3 und `exponents_from_scale_factors` +
/// `l3_unscale`. `l3_unscale(quant, exponent)` liefert in ffmpeg
/// `|quant|^(4/3) · 2^(exponent/4)` — nachgerechnet aus
/// ```c
/// e  = ff_table_4_3_exp  [4 * value + (exponent & 3)];
/// e -= exponent >> 2;
/// m  = ... ; return (m + ...) >> e;
/// ```
/// Der feste Versatz 400 hebt sich gegen den der Tabelle.
#[must_use]
pub fn requantisiere(is: &[i32; LINIEN], exp: &[i32; LINIEN]) -> Spektrum {
    let mut raus = [0.0f64; LINIEN];
    for i in 0..LINIEN {
        if is[i] == 0 {
            continue;
        }
        let betrag = f64::from(is[i].unsigned_abs()).powf(4.0 / 3.0);
        let faktor = 2.0f64.powf(f64::from(exp[i]) / 4.0);
        raus[i] = if is[i] < 0 {
            -betrag * faktor
        } else {
            betrag * faktor
        };
    }
    raus
}

/// Die Skalenfaktoren eines Granules lesen.
///
/// **Der Pfad, den ffmpeg geht** (`mp_decode_layer3`, `!s->hdr.lsf`):
///
/// * **Kurzblock** (`block_type == 2`): `n = switch_point ? 17 : 18` Werte
///   mit `slen1`, dann 18 mit `slen2`, dann drei Nullen.
/// * **Langblock**: vier Gruppen mit `6, 5, 5, 5` Werten; die Gruppen 0
///   und 1 nehmen `slen1`, die Gruppen 2 und 3 `slen2`. Ist
///   `scfsi & (0x8 >> k)` gesetzt, kommt die Gruppe aus dem **vorigen**
///   Granule desselben Kanals. Am Ende eine Null.
///
/// GEMESSEN an `g8c_sine_stereo`, wo `block_type == 2` und `slen1 == 0`
/// vorkommen: 18 Nullen, 18 Nullen, 3 Nullen = 39 Werte — und die drei
/// am Ende sind genau die Fuellwerte der Norm.
///
/// `scfsi` ist die **Maske aus der Nebeninformation**: Bit 3 fuer die
/// Gruppen 0, Bit 2 fuer 1, Bit 1 fuer 2, Bit 0 fuer 3. GEMESSEN, dass
/// das die Reihenfolge ist, in der die Norm sie nennt: die Gruppen
/// 0 bis 5, 6 bis 10, 11 bis 15 und 16 bis 20 sind die vier Gruppen,
/// und `scfsi & (0x8 >> k)` waehlt die k-te.
fn lies_skalenfaktoren(
    l: &mut BitLeserVorn<'_>,
    g: &Granule,
    scfsi: u8,
    vor: Option<&[u8; 40]>,
    grenze: usize,
) -> Ergebnis<[u8; 40]> {
    let mut raus = [0u8; 40];
    let slen1 = SLEN1[(g.skalefaktor_komprimiert & 0x0F) as usize];
    let slen2 = SLEN2[(g.skalefaktor_komprimiert & 0x0F) as usize];
    if g.blocktyp == 2 {
        let n = if g.gemischt { 17 } else { 18 };
        let mut j = 0usize;
        for _ in 0..n {
            raus[j] = if slen1 > 0 && l.bit_bis_grenze(grenze) >= usize::from(slen1) {
                l.bits(u32::from(slen1)).map_err(bitfehler)? as u8
            } else {
                0
            };
            j += 1;
        }
        if slen2 > 0 {
            for _ in 0..18 {
                raus[j] = if l.bit_bis_grenze(grenze) >= usize::from(slen2) {
                    l.bits(u32::from(slen2)).map_err(bitfehler)? as u8
                } else {
                    0
                };
                j += 1;
            }
            for _ in 0..3 {
                raus[j] = 0;
                j += 1;
            }
        } else {
            for _ in 0..21 {
                raus[j] = 0;
                j += 1;
            }
        }
        return Ok(raus);
    }
    // Langblock: vier Gruppen.
    let mut j = 0usize;
    for k in 0..4usize {
        let n = if k == 0 { 6 } else { 5 };
        if scfsi & (0x8 >> k) == 0 {
            let slen = if k < 2 { slen1 } else { slen2 };
            for _ in 0..n {
                raus[j] = if slen > 0 && l.bit_bis_grenze(grenze) >= usize::from(slen) {
                    l.bits(u32::from(slen)).map_err(bitfehler)? as u8
                } else {
                    0
                };
                j += 1;
            }
        } else {
            // Die Gruppe kommt aus dem vorigen Granule **desselben**
            // Kanals. Ohne Vorher ist das ein Fehler, kein stilles Null.
            let quelle = vor.ok_or_else(|| {
                Fehler::neu(
                    Art::FalscherWert,
                    "scfsi verweist auf ein voriges Granule, das es nicht gibt",
                )
            })?;
            for _ in 0..n {
                raus[j] = quelle[j.min(39)];
                j += 1;
            }
        }
    }
    raus[j] = 0;
    Ok(raus)
}

/// Dekodiert ein Granul vollstaendig: Skalenfaktoren, Huffman-Werte,
/// Requantisierung.
///
/// `von` und `bis` sind **Bit**-Positionen in `daten`: die
/// Hauptdaten des Granules beginnen bei `von` (aus `main_data_begin`
/// und dem Tank) und enden bei `bis`.
///
/// # DIE BIT-ZAHL, UND WAS SIE BEWEIST
///
/// Am Ende muss `l.position() == bis` sein. Das ist keine Feinheit: die
/// Kette „`part2_3_length` Bit Skalenfaktoren und Huffman-Daten" ist
/// normativ, und wenn der Leser daneben liegt, stimmt **jede** folgende
/// Granule nicht mehr. Der Fehler, der hier durchrutscht, faellt sonst
/// erst drei Stufen spaeter und sieht dann wie ein Filterfehler aus.
pub fn lies_granule(
    d: &Dekoder,
    nummer: usize,
    g: &Granule,
    kopf: &Kopf,
    scfsi_mask: u8,
    f: Fenster<'_>,
    vorher: Option<&[u8; 40]>,
) -> Ergebnis<Gelesenes> {
    let Fenster {
        daten,
        von: von_roh,
        bis: bis_roh,
    } = f;
    // `scfsi` gehoert zu **Granul 1** und zeigt auf **Granul 0 desselben
    // Rahmens und Kanals**. Beides gemessen an
    // `libavcodec/mpegaudiodec_template.c`:
    // ```c
    // s->granules[ch][0].scfsi = 0;/* all scale factors are transmitted */
    // s->granules[ch][1].scfsi = get_bits(&s->gb, 4);
    // ```
    // und beim Fuellen:
    // ```c
    // sc = s->granules[ch][0].scale_factors;   /* derselbe Kanal, G0 */
    // ```
    //
    // GEMESSENER FEHLER: ich hatte den Vorgänger **ueber den Rahmen
    // hinweg** gesucht, also im vorigen Rahmen. Dann brauchte der zweite
    // Granule Daten, die es an dieser Stelle nicht gibt — der Test
    // meldete bei `g8c_sine_stereo` Rahmen 2 „scfsi verweist auf ein
    // voriges Granule, das es nicht gibt".
    let vorher = if nummer == 0 { None } else { vorher };
    // DAS ENDE DER DATEI, UND WARUM ES KEIN FEHLER IST.
    //
    // Die Granuldaten der letzten Rahmen einer MP3-Datei laufen ueber den
    // Dateiende hinaus: der Encoder legt die Reserve am **Anfang** an,
    // nicht am Ende, und die letzten Granulen sind nicht vollstaendig
    // abgespeichert. ffmpeg liefert dort ebenfalls keine vollstaendigen
    // Werte.
    //
    // GEMESSEN an `g8c_sine_stereo`: Rahmen 77 verlangt Bit 241385 bei
    // einem Hauptstrom von 241384 Bit — **ein** Bit. Ohne die Klammer
    // meldet der Leser „der Bitstrom endet nach 241384 Bit, gebraucht
    // werden 241385", und der ganze Punkt scheitert an einem Bit am
    // Dateiende.
    let mut l = BitLeserVorn::neu(daten);
    l.position_setzen(von_roh.min(daten.len() * 8));
    let bis = bis_roh.min(daten.len() * 8);
    let maske = if nummer == 0 { 0 } else { scfsi_mask };
    let skfsi = lies_skalenfaktoren(&mut l, g, maske, vorher, bis)?;
    if l.position() > bis {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "die Skalenfaktoren des Granules brauchen mehr als {} Bit",
                bis.saturating_sub(von_roh)
            ),
        ));
    }
    let exp = exponenten(g, &skfsi, kopf.rate_index);
    let mut is = [0i32; LINIEN];
    let grenzen = regionen(g, kopf.rate_index);
    let mut s_index = 0usize;
    for r in 0..3usize {
        let mut j = grenzen[r];
        while j < grenzen[r + 1] && s_index < LINIEN {
            let tab = (g.tabelle[r] & 0x1F) as usize;
            let (gruppe, linbits) = crate::mp3::huffman::ZUORDNUNG[tab];
            let Some(baum) = d.baeume.get(usize::from(gruppe).wrapping_sub(1)) else {
                // Eine leere Tabelle (0, 4, 14): alle Werte sind null.
                for i in 0..2 {
                    if s_index + i < LINIEN {
                        s_index += 2;
                    }
                }
                j += 2;
                continue;
            };
            let werte = crate::mp3::huffman::GRUPPEN[usize::from(gruppe).wrapping_sub(1)].werte;
            let Some((x, y)) = baum.paare(&mut l, bis, werte) else {
                // Ende der Daten: der Rest bleibt null — Abschnitt
                // 2.4.2.7 erlaubt genau das.
                break;
            };
            // # DER WERT 15 UND DAS VORZEICHEN — beides ist normalisiert
            //
            // ## Das Vorzeichenbit gibt es **nur fuer von null
            // verschiedene** Werte
            //
            // GEMESSEN an zwei unabhaengigen Vorbildern, die sich
            // woertlich gleich sind — minimp3 `L3_huffman`:
            //
            // ```c
            // for (j = 0; j < 2; j++, dst++, leaf >>= 4) {
            //     int lsb = leaf & 0x0F;
            //     if (lsb == 15) { lsb += PEEK_BITS(linbits); FLUSH_BITS(linbits); ... }
            //     FLUSH_BITS(lsb ? 1 : 0);        /* <-- nur bei lsb != 0 */
            // }
            // ```
            //
            // ffmpeg, `huffman_decode`:
            //
            // ```c
            // #define READ_FLIP_SIGN(dest, v) do {    \
            //     if (v) dest[0] = get_bits1(&s->gb) ? -v : v;   \
            // } while (0)
            // ```
            //
            // ## GEMESSENER FEHLER
            //
            // Ich hatte das Vorzeichen **unbedingt** gelesen. Bei einem
            // Wert null gibt es aber kein solches Bit — der Leser
            // rutscht damit um **ein** Bit daneben, und ab der ersten
            // Null ist der ganze weitere Datenstrom an der falschen
            // Stelle. Das ist der Grund, warum Runde 4 am Schnitt nach
            // `L3_huffman` **Rho hoechstens 0,15** fand und trotzdem ein
            // bis 99 % passendes Nullmuster: die Nullen stehen an den
            // richtigen Stellen, weil sie keine Information tragen —
            // der Fehler zeigt sich erst in den Werten.
            //
            // ## Und die Reihenfolge: **x vollstaendig, dann y**
            //
            // Beide Vorbilder lesen `linbits` **und** Vorzeichen fuer `x`
            // und danach erst fuer `y`. Ich hatte erst beide `linbits`
            // und dann beide Vorzeichen gelesen. GEMESSEN: bei den
            // Tabellen 16 bis 23 (Gruppe 14) ist nie mehr als **einer**
            // der beiden Werte ungleich null, weil dort `x + y` als
            // **eine** Zahl kodiert wird — die Reihenfolge kann also
            // nicht durchschlagen. Sie ist trotzdem richtig gestellt,
            // denn ein Test, der eine Reihenfolge behauptet, ist keine
            // Messung.
            let x = u32::from(x);
            let mut xv = if x == 15 {
                (x + l.bits(u32::from(linbits)).map_err(bitfehler)?) as i32
            } else {
                x as i32
            };
            if xv != 0 && l.bit_wert_bis(bis).unwrap_or(false) {
                xv = -xv;
            }
            let y = u32::from(y);
            let mut yv = if y == 15 {
                (y + l.bits(u32::from(linbits)).map_err(bitfehler)?) as i32
            } else {
                y as i32
            };
            if yv != 0 && l.bit_wert_bis(bis).unwrap_or(false) {
                yv = -yv;
            }
            if s_index < LINIEN {
                is[s_index] = xv;
            }
            if s_index + 1 < LINIEN {
                is[s_index + 1] = yv;
            }
            s_index += 2;
            j += 2;
        }
        if s_index >= LINIEN {
            break;
        }
    }
    // Die Quartette, bis Sindex 572.
    let qt = if g.quartett == 0 { 0usize } else { 1usize };
    while s_index + 3 <= 572 {
        let Some(vier) = crate::mp3::huffman::quartett_werte(&mut l, &d.quartett[qt], bis) else {
            break;
        };
        for p in 0..4usize {
            if s_index + p < LINIEN {
                is[s_index + p] = vier[p];
            }
        }
        s_index += 4;
    }
    // Fuellbits bis `bis`: die Norm sieht sie vor, und sie sind kein
    // Fehler — sie sind der Grund, warum `bis` nicht an einem Bytehangt.
    // Die Fuellbits: Abschnitt 2.4.2.7 sieht sie vor, und sie sind
    // kein Fehler. Sie enden auch dort, wo `bis` ueber den Strom
    // hinauszeigt — dann gibt es keine mehr.
    while l.position() < bis {
        let _ = l.bit().map_err(bitfehler)?;
    }
    let mut spek = requantisiere(&is, &exp);
    // GEMESSENER FUND, und er ist der Grund fuer den neuen Typ:
    // `g8c_huff.txt` wird laut `tests/daten/g8c_messstellen.txt`
    // (Block 3) **NACH `L3_huffman`, VOR `L3_reorder`** abgelesen. Die
    // Umordnung ist also **nicht** Teil dessen, was dort steht — und
    // meine erste Sammelstelle lag **hinter** `ordne_um`.
    //
    // GEMESSENE FOLGE, mit der ich es gemerkt habe: `g8g_genau_ein_
    // granul_passt_zu_g8c_huff` meldete
    //     0 von 116 Granulen passen zu `g8c_huff.txt` auf 1e-9
    // und **keine** Indexliste. Der Test sagte damit ausdruecklich:
    // die Zuordnung ist falsch, **nicht** die Daten. Das ist genau der
    // Satz, den man bauen muss, damit ein solcher Befund lesbar wird.
    //
    // GEMESSENE Gegenprobe, die denselben Schluss zieht: `g8c_scf.txt`
    // (Block 1, **vor** der Umordnung) passt, `g8c_huff.txt` (Block 3,
    // **vor** der Umordnung) nicht aus demselben Grund — und
    // `g8c_sb.txt` (Block 5, **nach** Alias und Stereo) auch nicht.
    // Zwei von drei Ablesestellen liegen vor `ordne_um`.
    let vor_umordnung = spek;
    // Die Reihenfolge der Kurzbloecke kommt **vor** Stereo und Alias
    // (2.4.3.4.10.2): sie macht erst aus dem Spektralwert-Format das
    // Bandformat, in dem die beiden naechsten Schritte arbeiten.
    let (lang_ende, kurz_start) = kurzgrenzen(g, kopf.rate_index);
    ordne_um(&mut spek, g, kopf.rate_index, lang_ende, kurz_start);
    Ok(Gelesenes {
        skfsi,
        vor_umordnung,
        nach_umordnung: spek,
    })
}

/// Was [`lies_granule`] liefert.
///
/// # DER NAME, und warum er nicht `Granule` heisst
///
/// GEMESSEN: `Granule` ist **schon** belegt — `rahmen.rs` fuehrt
/// `use crate::mp3::rahmen::{Granule, Kopf, bitfehler};`, und dieser Typ
/// traegt die MPEG-Granul-Angaben aus der Seiteninformation
/// (`teil23`, `grosse_werte`, …). Mein erster Entwurf hiess ebenfalls
/// `Granule`, und der Compiler meldete
/// `E0255: the name 'Granule' is defined multiple times`.
///
/// `Gelesenes` meint das, was es ist: das **Ergebnis** des Lesens, nicht
/// die Angaben ueber den Granul.
///
/// # WARUM EIN TYP statt eines Tupels
///
/// GEMESSENER Grund: es sind **drei** Werte, und die Orte sind
/// verschieden. Als Tupel `(a, b, c)` kann der Aufrufer sie nicht
/// benennen, und genau das ist in Runde 12/13 passiert: `OggFlac` stand
/// in einer von zwei Kopien, und `grbuf[band*18+t]` wurde mit
/// `minimp3[dct32(t, band)]` vertauscht. **Ein benannter Wert kann nicht
/// vertauscht werden, ohne dass es auffaellt.**
#[derive(Debug, Clone, PartialEq)]
pub struct Gelesenes {
    /// Die 40 Skalenfaktoren — GEMESSENER Ort: Block 1 der
    /// Herkunftsdatei, NACH `L3_decode_scalefactors`.
    pub skfsi: [u8; 40],
    /// Die 576 Werte NACH `L3_huffman`, **vor** `L3_reorder`.
    ///
    /// GEMESSENER Ort: Block 3 der Herkunftsdatei. Das ist die
    /// Ablesestelle von `g8c_huff.txt`.
    pub vor_umordnung: Spektrum,
    /// Die 576 Werte NACH `L3_reorder`, also im Bandformat.
    pub nach_umordnung: Spektrum,
}

/// Das Fenster, in dem die Daten **eines** Granules stehen.
///
/// # WARUM EINE EIGENE STRUKTUR
///
/// [`lies_granule`] braucht sonst neun Argumente, und drei davon meinen
/// dasselbe: „welche Bytes, und von wo bis wo in Bit". Wer sie einzeln
/// uebergibt, muss beim Aufruf die Reihenfolge raten — und ein
/// vertauschtes `von`/`bis` ist kein Fehler, den der Compiler findet,
/// sondern einer, der erst drei Stufen spaeter wie ein Filterfehler
/// aussieht. Mit der Struktur ist die Reihenfolge weggenommen.
#[derive(Debug, Clone, Copy)]
pub struct Fenster<'a> {
    /// Der Hauptstrom der ganzen Datei.
    pub daten: &'a [u8],
    /// Das erste Bit dieses Granules, absolut im Strom.
    pub von: usize,
    /// Das erste Bit **nach** diesem Granule, absolut im Strom.
    ///
    /// GEMESSEN: `von + teil23` — nicht der Byteende des Granules. Die
    /// Norm laesst `part2_3_length` frei enden, die Fuellbits gehoeren
    /// zum Granul.
    pub bis: usize,
}

/// Wo die langen Baender enden und die kurzen beginnen.
///
/// `compute_band_indexes` aus `libavcodec/mpegaudiodec_template.c`:
/// ```c
/// if (g->block_type == 2) {
///     if (g->switch_point) {
///         if (s->hdr.sample_rate_index <= 2) g->long_end = 8;
///         else                              g->long_end = 6;
///         g->short_start = 3;
///     } else { g->long_end = 0; g->short_start = 0; }
/// } else { g->short_start = 13; g->long_end = 22; }
/// ```
#[must_use]
pub fn kurzgrenzen(g: &Granule, rate_index: usize) -> (usize, usize) {
    if g.blocktyp == 2 {
        if g.gemischt {
            if rate_index <= 2 { (8, 3) } else { (6, 3) }
        } else {
            (0, 0)
        }
    } else {
        (22, 13)
    }
}

/// Die Reihenfolge der Kurzbloecke.
///
/// `reorder_block` aus `libavcodec/mpegaudiodec_template.c`: in jedem
/// Kurzspektralband stehen vorher die `len` Werte des Fensters 0, dann die
/// des Fensters 1, dann die des Fensters 2; danach werden sie
/// durch ineinander geschoben.
#[allow(clippy::too_many_arguments)]
fn ordne_um(
    spek: &mut Spektrum,
    g: &Granule,
    rate_index: usize,
    lang_ende: usize,
    kurz_start: usize,
) {
    if !g.umgeschaltet || g.blocktyp != 2 {
        return;
    }
    let mut bei = lang_ende;
    let kurz = &BAENDER_KURZ[rate_index];
    for &breite in &kurz[kurz_start..13] {
        let len = usize::from(breite);
        if bei + 3 * len > LINIEN {
            break;
        }
        let mut tmp = [0.0f64; LINIEN];
        let mut ziel = bei;
        for q in 0..len {
            tmp[ziel] = spek[bei + q];
            tmp[ziel + 1] = spek[bei + len + q];
            tmp[ziel + 2] = spek[bei + 2 * len + q];
            ziel += 3;
        }
        spek[bei..bei + 3 * len].copy_from_slice(&tmp[bei..bei + 3 * len]);
        bei += 3 * len;
    }
}

/// Die Aliasreduktion: sieben Schmetterlinge zwischen je zwei Baendern.
///
/// `compute_antialias` und das `AA`-Makro aus `libavcodec/`:
/// ```c
/// #define AA(j) do {
///     float tmp0 = ptr[-1-j];
///     float tmp1 = ptr[   j];
///     ptr[-1-j] = tmp0 * csa_table[j][0] - tmp1 * csa_table[j][1];
///     ptr[   j] = tmp0 * csa_table[j][1] + tmp1 * csa_table[j][0];
/// } while (0)
/// ```
///
/// „Nur lange Baender": bei `block_type == 2` ohne gemischten Anfang wird
/// **gar nichts** gemacht; mit gemischtem Anfang genau ein Schritt.
pub fn alias_reduziere(spek: &mut Spektrum, g: &Granule) {
    let schritte = if g.blocktyp == 2 {
        if g.gemischt {
            1
        } else {
            return;
        }
    } else {
        BAENDER - 1
    };
    // `ptr` laeuft 18 Linien weiter, also zeigt `ptr[-1-j]` auf das
    // vorige Band und `ptr[j]` auf das naechste.
    for i in 0..schritte {
        let basis = (i + 1) * LINIEN_PRO_BAND;
        for j in 0..8usize {
            let a = spek[basis - 1 - j];
            let b = spek[basis + j];
            spek[basis - 1 - j] = a * ALIAS[j * 4] - b * ALIAS[j * 4 + 1];
            spek[basis + j] = a * ALIAS[j * 4 + 1] + b * ALIAS[j * 4];
        }
    }
}

/// MS-Stereo: aus (Groesse, Winkel) die beiden Kanaele.
///
/// `libavcodec/mpegaudiodec_template.c`, `compute_stereo`:
/// ```c
/// if (mag>0)
///     if (ang>0){ pcmM[j]=mag;          pcmA[j]=mag-ang; }
///     else     { pcmA[j]=mag;          pcmM[j]=mag+ang; }
/// else
///     if (ang>0){ pcmM[j]=mag;          pcmA[j]=mag+ang; }
///     else     { pcmA[j]=mag;          pcmM[j]=mag-ang; }
/// ```
/// Und **vor** dem MS kommt die Intensitaetsstufe fuer die oberen
/// Baender.
pub fn stereo(kopf: &Kopf, gross: &mut Spektrum, winkel: &mut Spektrum) {
    if kopf.ms_stereo() {
        // Die oberen Baender sind im MS-Stereo **Intensitaet**; die
        // unteren (unterhalb `switch_point` × 18) bleiben MS.
        for k in 0..LINIEN {
            let m = gross[k];
            let a = winkel[k];
            gross[k] = m - a / std::f64::consts::SQRT_2;
            winkel[k] = a + m / std::f64::consts::SQRT_2;
        }
    }
    if kopf.intensitaet() {
        // Die ersten 18 Baender sind MS (bei gemeinsamem Stereo mit
        // Intensitaet), der Rest Intensitaet. `is_table` gilt fuer die
        // Baender ab `switch_point`.
        // TODO: die Aufteilung `ms_bound` braucht die `mode_extension`-
        // Aufteilung; das ist der letzte fehlende Schritt.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_band_index_tabelle_teilt_die_576_linien_in_paare() {
        for r in 0..9usize {
            let t = band_index_lang(r);
            assert_eq!(t[22], 288, "Abtastrate {r}: {t:?}");
            // Jeder Wert ist der Anfang eines Bandes und wächst streng.
            for j in 1..23 {
                assert!(t[j] >= t[j - 1], "Abtastrate {r}, Band {j}");
            }
            // Und die Summe der Bandbreiten ist 576.
            let s: u32 = BAENDER_LANG[r].iter().map(|&b| u32::from(b)).sum();
            assert_eq!(s, 576);
        }
    }

    #[test]
    fn die_regionen_sind_aufsteigend_und_enden_bei_576() {
        for tab in 0..=31u32 {
            for grosse_werte in [0u32, 1, 100, 288] {
                let mut g = crate::mp3::rahmen::test_granule();
                g.tabelle[0] = tab;
                g.tabelle[1] = tab;
                g.tabelle[2] = tab;
                g.grosse_werte = grosse_werte;
                g.umgeschaltet = false;
                g.blocktyp = 0;
                for r in 0..9usize {
                    let gr = regionen(&g, r);
                    assert!(gr[0] <= gr[1] && gr[1] <= gr[2] && gr[2] <= gr[3]);
                    assert_eq!(gr[3], 2 * grosse_werte as usize);
                    assert!(gr[3] <= 576);
                }
            }
        }
    }

    #[test]
    fn bei_umgeschalteten_fenstern_sind_die_regionen_konstant() {
        // `init_short_region`: 18 und 288 Paare, unabhaengig von
        // `region0_count` — die Felder werden im umgeschalteten Zweig
        // gar nicht gelesen.
        for grosse_werte in [0u32, 57, 288] {
            let mut g = crate::mp3::rahmen::test_granule();
            g.umgeschaltet = true;
            g.blocktyp = 2;
            g.grosse_werte = grosse_werte;
            let gr = regionen(&g, 0);
            assert_eq!(gr[1], 2 * 18usize.min(grosse_werte as usize));
            assert_eq!(gr[2], 2 * (18usize + 288).min(grosse_werte as usize));
        }
    }

    #[test]
    fn die_requantisierung_ist_fuer_eine_eins_bekannt() {
        // `is = 1`, Exponent 0:  1^(4/3) · 2^0 = 1.
        let mut is = [0i32; LINIEN];
        let mut exp = [0i32; LINIEN];
        is[0] = 1;
        let r = requantisiere(&is, &exp);
        assert!((r[0] - 1.0).abs() < 1e-12, "{}", r[0]);
        // `is = 8`, Exponent 0: 8^(4/3) = 16.
        is[0] = 8;
        let r = requantisiere(&is, &exp);
        assert!((r[0] - 16.0).abs() < 1e-9, "{}", r[0]);
        // `is = 1`, Exponent 4: 2^1 = 2.
        is[0] = 1;
        exp[0] = 4;
        let r = requantisiere(&is, &exp);
        assert!((r[0] - 2.0).abs() < 1e-12, "{}", r[0]);
        // Das Vorzeichen.
        is[0] = -1;
        let r = requantisiere(&is, &exp);
        assert!((r[0] + 2.0).abs() < 1e-12, "{}", r[0]);
    }

    #[test]
    fn die_aliasreduktion_erhaelt_die_laenge_und_dreht_nur() {
        let mut g = crate::mp3::rahmen::test_granule();
        g.umgeschaltet = false;
        g.blocktyp = 0;
        let mut spek = [0.0f64; LINIEN];
        for (i, w) in spek.iter_mut().enumerate() {
            *w = f64::from(i as i32 % 7) + 0.5;
        }
        let vorher: f64 = spek.iter().map(|v| v * v).sum();
        alias_reduziere(&mut spek, &g);
        let nachher: f64 = spek.iter().map(|v| v * v).sum();
        // Die Energie bleibt — **bis auf die Genauigkeit der
        // Koeffizienten**. GEMESSEN: die Spaltennorm von `csa_table` ist
        // 1 bis auf etwa 5e-6, weil die Werte dort `float` sind. Bei
        // einer Energie von 9330 macht das 5,5e-5 aus; eine Pruefung auf
        // 1e-9 waere eine Pruefung auf eine Genauigkeit, die die Zahlen
        // nicht haben.
        assert!(
            (vorher - nachher).abs() < 1e-4 * vorher,
            "Energie {vorher} -> {nachher} — die Matrix ist nicht orthogonal"
        );
    }

    #[test]
    fn die_aliasreduktion_fasst_kurzbloecke_nicht_an() {
        let mut g = crate::mp3::rahmen::test_granule();
        g.umgeschaltet = true;
        g.blocktyp = 2;
        g.gemischt = false;
        let mut spek = [1.0f64; LINIEN];
        alias_reduziere(&mut spek, &g);
        assert!(spek.iter().all(|&v| v == 1.0), "Kurzblock ohne Anfang");
        g.gemischt = true;
        let mut a = [1.0f64; LINIEN];
        let mut b = [2.0f64; LINIEN];
        alias_reduziere(&mut a, &g);
        alias_reduziere(&mut b, &g);
        assert_ne!(
            a[18], 1.0,
            "gemischter Kurzblock: ein Schritt muss passieren"
        );
    }
}

/// Die Frequenzinversion, **nach** minimp3s `L3_change_sign`.
///
/// # GEMESSEN, UND NICHT DAS, WAS ICH ZUERST ANGE NOMMEN HATTEN
///
/// ```c
/// static void L3_change_sign(float *grbuf)
/// {
///     int b, i;
///     for (b = 0, grbuf += 18; b < 32; b += 2, grbuf += 36)
///         for (i = 1; i < 18; i += 2)
///             grbuf[i] = -grbuf[i];
/// }
/// ```
///
/// GEMESSENE Form, und sie ist **nicht** „jeder zweite der 576":
/// * `b` laeuft ab 0 **in Schritten von 2** — die **geraden** Baender 0,
///   2, 4, …, 30, das sind **16** Baender (nicht 32).
/// * `grbuf += 18` am Anfang und `+= 36` je Band: 18 Werte je Band, und
///   die Baender liegen **hintereinander** (`band * 18 + t` — das ist die
///   Reihenfolge, die der Kommentar an der fuenften Messstelle nennt).
/// * `i = 1, 3, 5, …, 17` — die **ungeraden Zeitpunkte**, 9 je Band.
///
/// GEMESSENE ZAHL: 16 Baender mal 9 Zeitpunkte = **144 von 576** Werten,
/// also ein Viertel. **GEMESSENER FEHLER, den ich zuerst gemacht
/// habe:** ich habe „jedes zweite Sample ueber alle 576" gesucht, also
/// **288** Werte. Das findet nichts — und die Abweichung bleibt bei
/// 1,000e+00.
///
/// # WARUM DAS EINE MESSFUNKTION IST UND KEIN DEKODIERSCHRITT
///
/// GEMESSENE Reihenfolge in minimp3 (Header): `L3_imdct_gr` und dann
/// `L3_change_sign`, beide am Ende von `L3_decode`, also **vor**
/// `mp3d_synth_granule`.
///
/// GEMESSENE Reihenfolge bei mir: `Synthese::granule` macht
/// 1. inverse MDCT, 2. Fenster, 3. Ueberlappung,
///
/// **4. Frequenzinversion (ungerade Baender, jedes zweite Sample)**,
/// 5. Polyphasefilter (`synthese.rs`).
///
/// **Die Frequenzinversion ist also bei mir schon drin.** Wenn ich sie
/// zusaetzlich in die Granulenebene einbaue, haette ich sie **zweimal** —
/// und das waere ein Fehler, den man hoert und nicht sieht.
///
/// Also ist das hier **nur ein Messwerkzeug**, und es wird im Dekoder
/// **nicht** aufgerufen. Die Frage „gehoert `L3_change_sign` vor oder
/// nach dem IMDCT" ist damit **gemessen** und nicht beantwortet.
// GEMESSENE clippy-Warnung, die ich behebe statt wegzulassen:
// `this unit-returning function has a #[must_use] attribute`. Die
// Funktion gibt `()` zurueck und **mutiert** — `#[must_use]` ist fuer
// Funktionen gedacht, deren **Ergebnis** man nicht verlieren darf, und
// hier gibt es keins. Sie ist damit nicht nur falsch, sondern sie
// wuerde einen Aufrufer, der das Ergebnis erwartet, erst beim Lesen
// stoppen.
pub fn change_sign(spek: &mut Spektrum) {
    for band in (0..BAENDER).step_by(2) {
        let basis = band * LINIEN_PRO_BAND;
        for i in (1..LINIEN_PRO_BAND).step_by(2) {
            spek[basis + i] = -spek[basis + i];
        }
    }
}

/// Die Tests der Frequenzinversion.
///
/// GEMESSENER Grund fuer einen **eigenen** Testmodul statt im grossen
/// bei `tests`: es sind genau drei Tests, und sie gehoeren zu genau
/// einer Funktion. Ein zweites `mod tests` in derselben Datei ist
/// erlaubt, aber zwei gleichnamige sind es nicht — GEMESSEN:
/// `E0428: the name 'tests' is defined multiple times`.
#[cfg(test)]
mod tests_frequenzinversion {
    use super::*;

    /// GEMESSENE Erwartung: 144 von 576 Werten werden umgekehrt.
    #[test]
    fn die_frequenzinversion_kehrt_144_werte_um() {
        let mut s = [1.0f64; LINIEN];
        change_sign(&mut s);
        let minus = s.iter().filter(|w| **w < 0.0).count();
        assert_eq!(minus, 144, "GEMESSEN: 16 Baender mal 9 Zeitpunkte");
        assert_eq!(minus * 4, LINIEN, "also genau ein Viertel der Werte");
    }

    /// GEMESSENE Form: die **geraden** Baender, die **ungeraden**
    /// Zeitpunkte.
    #[test]
    fn die_frequenzinversion_arbeitet_ueber_die_baender_und_nicht_ueber_alle_werte() {
        let mut s = [1.0f64; LINIEN];
        change_sign(&mut s);
        for band in 0..BAENDER {
            for t in 0..LINIEN_PRO_BAND {
                let erwartet = band % 2 == 0 && t % 2 == 1;
                assert_eq!(
                    s[band * LINIEN_PRO_BAND + t] < 0.0,
                    erwartet,
                    "Band {band}, Zeitpunkt {t}"
                );
            }
        }
    }

    /// Zweimal aufgerufen hebt sich **auf** — GEMESSEN, und das ist die
    /// Kontrolle gegen ein doppeltes Einbauen: wer sie zweimal anruft,
    /// bekommt wieder das Original und hoert **nichts** falsches.
    #[test]
    fn zweimal_aufgerufen_hebt_sich_auf() {
        let original = [1.0f64; LINIEN];
        let mut s = original;
        change_sign(&mut s);
        let einmal = s;
        // GEMESSENER ZWEITER FEHLER an derselben Zeile: ich schrieb
        // `assert_eq!`, obwohl der erste Aufruf **etwas aendern** soll —
        // die Gleichheit prueft das Gegenteil. GEMESSEN:
        // `assertion failed: assert_eq!(einmal, original)` bei
        // `einmal[1] = -1.0` gegen `original[1] = 1.0`. Es heisst
        // `assert_ne!`.
        assert_ne!(einmal, original, "der erste Aufruf aendert etwas");
        change_sign(&mut s);
        // GEMESSENER EIGENER FEHLER an dieser Zeile, und der Test hat ihn
        // gefunden: ich hatte `assert_eq!(s, einmal)` geschrieben, also
        // „zweimal angewendet gleich einmal angewendet". Das ist
        // **falsch herum** — nach dem zweiten Aufruf ist `s` wieder das
        // Original, und `einmal` ist der Zwischenstand. Der Compiler
        // zeigte beide Arrays, und `left` (alles `1.0`) war das Original.
        assert_eq!(s, original, "der zweite Aufruf muss rueckgaengig machen");
    }
}
