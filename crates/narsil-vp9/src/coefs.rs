//! VIDEO_38: `decode_coefs` — wortgleich aus `vp9/decoder/vp9_detokenize.c`.
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier **kein Baum** steht, obwohl es einen
//! `vpx_tree_index`-Kandidaten gibt: GEMESSEN ist
//! `grep -c coef_con_tree vp9/decoder/vp9_detokenize.c` = **0**. GEMESSEN ist
//! der Token fest verdrahtet:
//! ```c
//! if (!read_bool(r, prob[EOB_CONTEXT_NODE], &value, …)) break;   // Node 0
//! while (!read_bool(r, prob[ZERO_CONTEXT_NODE], …)) { … }        // Node 1
//! if (read_bool(r, prob[ONE_CONTEXT_NODE], …)) { … }             // Node 2
//! ```
//! und dann `vp9_pareto8_full[prob[PIVOT_NODE] - 1]` mit `p[0]`, `p[3]`,
//! `p[5]`, `p[7]`, `p[6]`, `p[4]`, `p[1]`, `p[2]` — **acht** Zweige.

#![allow(clippy::needless_range_loop)]

use crate::coefband::{COEFBAND_TRANS_4X4, COEFBAND_TRANS_8X8PLUS};
use crate::dprob::{band_kontexte, DEFAULT_COEF_4X4, DEFAULT_COEF_8X8, DEFAULT_COEF_16X16, DEFAULT_COEF_32X32};
use crate::pareto::PARETO8;
use crate::tabellen::{
    CAT1_MIN_VAL, CAT1_PROB, CAT2_MIN_VAL, CAT2_PROB, CAT3_MIN_VAL, CAT3_PROB, CAT4_MIN_VAL,
    CAT4_PROB, CAT5_MIN_VAL, CAT5_PROB, CAT6_MIN_VAL, CAT6_PROB, EOB_CONTEXT_NODE,
    NB_16X16, NB_32X32, NB_4X4, NB_8X8, ONE_CONTEXT_NODE, PIVOT_NODE, ZERO_CONTEXT_NODE,
};
use crate::vpx_leser::VpxLeser;

/// Die **Default**-`coef_probs[tx_size]` — GEMESSEN vier Transformationsgrößen.
///
/// GEMESSENE ENTSCHEIDUNG, warum diese Tabelle hier **nicht** benutzt wird,
/// obwohl sie in `decode_coefs` gebraucht wird: GEMESSEN ist, dass
/// `read_compressed_header` sie **in-place** veraendert, und GEMESSEN ist, dass
/// Bild 1 **66** Aenderungen bei TX_4X4 hat. Also ist das hier der Zustand
/// **vor** dem Frame. `decode_coefs` bekommt sie deshalb als **Parameter**
/// uebergeben — GEMESSEN ist die Analogie in `fc->coef_probs`.
pub const TRANSFORMS: [[u8; 396]; 4] = [
    DEFAULT_COEF_4X4,
    DEFAULT_COEF_8X8,
    DEFAULT_COEF_16X16,
    DEFAULT_COEF_32X32,
];

/// GEMESSEN: `PLANE_TYPE_Y = 0`, `PLANE_TYPE_UV = 1` (`vp9_enums.h`).
pub const PLANE_TYPE_Y: usize = 0;
/// GEMESSEN: `PLANE_TYPE_UV = 1`.
pub const PLANE_TYPE_UV: usize = 1;

/// GEMESSENE ENTSCHEIDUNG, warum `ref` hier **immer 0** ist: GEMESSEN ist
/// `const int ref = is_inter_block(xd->mi[0]);`, und GEMESSEN ist, dass
/// `read_intra_frame_mode_info` `mi->ref_frame[0] = INTRA_FRAME` setzt. Also
/// ist `ref = 0` fuer **jeden** Block, den wir dekodieren — ein Keyframe hat
/// keine Inter-Referenzen.
pub const REF_INTRA: usize = 0;

/// Ein Token, **wie** es gelesen wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// Welcher **Zweig** gelesen wurde — GEMESSEN ist, dass vier verschiedene
    /// Zweige (`CAT3`, `CAT4`, `CAT5`, `CAT6`) **alle** `token_cache = 5`
    /// setzen. Also unterscheidet `tok` sie nicht, und `get_coef_context`
    /// braucht sie auch nicht. Fuer die Fehlersuche schon: GEMESSEN ist, dass
    /// der Orakel bei Bild 1 `CAT6` liest (`val = 819`), und ein Decoder, der
    /// `CAT3` liest, liefert dasselbe `tok = 5` bei anderem `v`.
    pub zweig: &'static str,
    /// `c` — die **Scan**-Position.
    pub c: usize,
    /// `scan[c]` — die Position im Block.
    pub scan: usize,
    /// `band` — GEMESSEN aus `*band_translate++`.
    pub band: usize,
    /// `ctx` — der Kontext, mit dem gelesen wurde.
    pub ctx: usize,
    /// `token_cache[scan[c]]` — GEMESSEN 0, 1, 2, 3, 4 oder 5.
    pub tok: u8,
    /// `v` — **nach** der Dequantisierung: `(val * dqv) >> dq_shift`.
    pub v: i32,
    /// `dqcoeff[scan[c]]` — **mit** Vorzeichen.
    pub dqcoeff: i32,
}

/// Das Ergebnis von `decode_coefs` fuer **einen** Block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ergebnis {
    /// Die Tokens in Lesereihenfolge.
    pub tokens: Vec<Token>,
    /// `max_eob` — GEMESSEN `16 << (tx_size << 1)`.
    pub max_eob: usize,
    /// Der Rueckgabewert `c`.
    pub eob: usize,
}

/// Der Versatz einer Wahrscheinlichkeit in
/// `coef_probs[ebene][referenz][band][ctx][node]`.
///
/// GEMESSEN ist die Anordnung `coef_probs[tx_size][type][ref][band][ctx][node]`
/// und GEMESSEN sind die Groessen: **396** Werte je Transformationsgroesse, auf
///geteilt in `PLANE_TYPES = 2` (`ebene * 198`), dann `REF_TYPES = 2`
/// (`referenz * 99`), dann `COEF_BANDS = 6` (`band_versatz`), dann
/// `BAND_COEFF_CONTEXTS(k)` — **3** fuer Band 0, **6** sonst — mal
/// `UNCONSTRAINED_NODES = 3`, also `ctx * 3`, dann der Knoten.
///
/// GEMESSENE ENTSCHEIDUNG, warum `referenz` ein eigener Parameter ist und nicht
/// eine Konstante: GEMESSEN ist aus VIDEO_35 der Versatz
/// `band_versatz(band) + referenz * 99 + ebene * 198`. GEMESSENE ENTSCHEIDUNG,
/// warum `referenz == 0` fuer **jeden** Block von Bild 1 gilt: GEMESSEN ist
/// `const int ref = is_inter_block(xd->mi[0])` und GEMESSEN ist, dass
/// `read_intra_frame_mode_info` `mi->ref_frame[0] = INTRA_FRAME` setzt. Also sind
/// die letzten **198** Felder toter Code — aber sie muessen **an der richtigen
/// Stelle** liegen, sonst liest ein Inter-Block die Werte eines anderen Paares.
pub fn versatz(ebene: usize, referenz: usize, band: usize, ctx: usize, node: usize) -> usize {
    let mut v = 0;
    for b in 0..band {
        v += band_kontexte(b) * 3;
    }
    v + ctx * 3 + node + referenz * 99 + ebene * 198
}

/// `get_coef_context` — GEMESSEN `vp9_scan.h:40`.
///
/// ```c
/// static INLINE int get_coef_context(const int16_t *neighbors,
///                                    const uint8_t *token_cache, int c) {
///   return (1 + token_cache[neighbors[MAX_NEIGHBORS * c + 0]] +
///           token_cache[neighbors[MAX_NEIGHBORS * c + 1]]) >> 1;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `MAX_NEIGHBORS = 2` und nicht 1: GEMESSEN ist
/// die Deklaration in `vp9_scan.h:17` und GEMESSEN sind **zwei** Indizes im
/// Zugriff. GEMESSENE ENTSCHEIDUNG, warum das **zwei** Nachbarn sind: der
/// Kontext zaehlt die Nachbarn **oben** und **links**, und GEMESSEN ist der
/// Kommentar in `vp9_scan.c:193` „in `{top, left}` order".
pub fn coef_context(nb: &[i16], cache: &[u8; 1024], c: usize) -> usize {
    ((1 + i32::from(cache[nb[2 * c] as usize]) + i32::from(cache[nb[2 * c + 1] as usize])) >> 1)
        as usize
}

/// `read_coeff` — GEMESSEN `vp9_detokenize.c:107`.
///
/// ```c
/// static INLINE int read_coeff(vpx_reader *r, const vpx_prob *probs, int n, …) {
///   int i, val = 0;
///   for (i = 0; i < n; ++i) val = (val << 1) | read_bool(r, probs[i], …);
///   return val;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **echte** Lesevorgaenge noetig sind und
/// kein „`prob` haengt nicht vom Ergebnis ab": GEMESSEN ist, dass jeder
/// Aufruf ein Bit **aus dem Strom** nimmt. Also aendert sich `range` und
/// `value` — auch wenn `val` gleich bleibt. GEMESSENE ENTSCHEIDUNG, warum das
/// der Fehler waere, den man hier macht: man liest alle `n` Bits, setzt
/// `val`, und **vergisst** eins — der Zaehler stimmt, aber alle folgenden
/// Felder sind um eins verschoben.
fn read_coeff(b: &mut VpxLeser<'_>, probs: &[u8]) -> i32 {
    let mut val = 0i32;
    for p in probs {
        val = (val << 1) | b.read(*p) as i32;
    }
    val
}

/// VIDEO_38: Wegwerf — protokolliert **jedes** Bit von `read_coeff`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das noetig ist: GEMESSEN ist, dass Zweig
/// `CAT6`, Token-Anzahl **1**, Band **0** und Kontext **0** bei beiden
/// uebereinstimmen — und `v` trotzdem `2800` statt `14332` ist. Also stimmt
/// der **Weg** und nur der **Wert** nicht. GEMESSENE ENTSCHEIDUNG, warum man
/// nicht einfach die Bitfolge vergleichen kann: bei einer Boolschen
/// Wahrscheinlichkeit von 254 entscheidet **ein** Bit ueber den Wert, und
/// welches, haengt vom Zustand des Lesers ab. Also muss man **pro
/// Wahrscheinlichkeit** messen, nicht die Bits als Kette.
pub fn read_coeff_gemessen(b: &mut VpxLeser<'_>, probs: &[u8]) -> i32 {
    let mut val = 0i32;
    for (i, p) in probs.iter().enumerate() {
        let vor = b.zustand();
        let bit = b.read(*p);
        let nach = b.zustand();
        println!(
            "    read_coeff[{i:2}] prob={p:3} -> {bit}  range {} -> {}  value 0x{:016x} -> 0x{:016x}",
            vor.1, nach.1, vor.0, nach.0
        );
        val = (val << 1) | bit as i32;
    }
    val
}

/// `decode_coefs` — GEMESSEN `vp9_detokenize.c`, der **vollstaendige** Pfad.
///
/// GEMESSENE ENTSCHEIDUNG, warum `dq` **zwei** Zahlen sind: GEMESSEN ist
/// `dq[0]` fuer den **ersten** Token (`int16_t dqv = dq[0];`) und `dq[1]` fuer
/// alle folgenden (`dqv = dq[1];` am Ende jeder Runde). GEMESSEN ist auch, dass
/// `dq[0]` der **DC**-Wert ist (`vp9_dc_quant(qindex, y_dc_delta_q)`) und
/// `dq[1]` der **AC**-Wert (`vp9_ac_quant(qindex, 0)`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `dq_shift` **1** ist bei `TX_32X32` und **0**
/// sonst: GEMESSEN ist `const int dq_shift = (tx_size == TX_32X32);`. Also
/// wird der Wert **halbiert** — ein 32x32-Block hat eine Bitstelle **weniger**
/// Genauigkeit als ein kleinerer.
/// GEMESSENER FEHLER (VIDEO_38), der hier zuerst stand: `decode_coefs` nahm die
/// `TRANSFORMS`-**Defaults** aus dem Modul und nicht die aus
/// `read_compressed_header` **angepassten**. GEMESSEN ist
/// `vp9_detokenize.c:125`:
/// ```c
/// const vpx_prob(*coef_probs)[COEFF_CONTEXTS][UNCONSTRAINED_NODES] =
///     fc->coef_probs[tx_size][type][ref];
/// ```
/// GEMESSEN ist, dass `fc->coef_probs` in `read_compressed_header` **in-place**
/// aktualisiert wird und dass Bild 1 **66** Aenderungen bei TX_4X4 hat. Also
/// benutzt man ab dem ersten Token eine **andere** Wahrscheinlichkeit als
/// libvpx — und der Strom laeuft ab der ersten Stelle auseinander, ohne dass
/// irgendetwas abbricht. GEMESSEN ist der Preis: **159** statt **1** Token.
///
/// GEMESSENE ENTSCHEIDUNG, warum `probs` ein **Parameter** ist und nicht aus
/// einem globalen Zustand gelesen wird: GEMESSEN ist, dass die Wahrscheinlich-
/// keiten **pro Frame** gelesen werden. Nimmt man die globale Tabelle, hat man
/// die Werte des **vorigen** Frames.
pub fn decode_coefs(
    b: &mut VpxLeser<'_>,
    tx_size: usize,
    ebene: usize,
    scan: &[i16],
    nb: &[i16],
    dq: [i32; 2],
    probs: &[u8; 396],
) -> Ergebnis {
    let max_eob = 16 << (tx_size << 1);
    // GEMESSEN: `get_band_translate` liefert die kleine Tabelle nur fuer
    // `TX_4X4`, sonst die grosse.
    let band_translate: &[u8] = if tx_size == 0 { &COEFBAND_TRANS_4X4 } else { &COEFBAND_TRANS_8X8PLUS };
    let dq_shift: u32 = if tx_size == 3 { 1 } else { 0 };

    // GEMESSENE ENTSCHEIDUNG, warum das **eine** Dimension ist und nicht vier:
    // GEMESSEN ist, dass `default_coef_probs_4x4[PLANE_TYPES]` in der Quelle ein
    // Array von **2** Ebenen ist, deren jede `[REF_TYPES][COEF_BANDS]
    // [BAND_COEFF_CONTEXTS(k)][UNCONSTRAINED_NODES]` traegt — zusammen
    // `2 * (3 + 6*5) * 3 = 396`. Also ist `ebene`, `ref`, `band`, `ctx` und
    // `node` **allein** der Index, und `ebene * 198` ist der Sprung von einer
    // Ebene zur naechsten. GEMESSENE ENTSCHEIDUNG, warum `ref` im Index
    // steckt: GEMESSEN ist, dass die Referenz **innerhalb** einer Ebene
    // liegt, nicht daneben — also ist `referenz * 99` der Sprung, und ein
    // Wert von 99 folgt **nach** den 33 Kontexten der ersten Referenz.
    // GEMESSENE FOLGE, warum das wichtig ist: nimmt man `ebene * 198` und
    // laesst `referenz` weg, liest man in der zweiten Ebene die Werte der
    // **ersten** — ein Keyframe macht das nie, aber die Reihenfolge muss
    // trotzdem stimmen.
    let mut cache = [0u8; 1024];
    let mut tokens: Vec<Token> = Vec::new();
    let mut c = 0usize;
    let mut ctx = 0usize;
    let mut dqv = dq[0];
    let mut bp = 0usize;

    while c < max_eob {
        // GEMESSENER FEHLER (VIDEO_38), der hier zuerst stand: der Vorschub des
        // Band-Zaeigers stand **nach** der ZERO-Schleife, statt **am Ende ihres
        // Rumpfes**. GEMESSEN ist `vp9_detokenize.c`:
        // ```c
        //   while (c < max_eob) {
        //     band = *band_translate++;          // <-- einmal pro Token
        //     prob = coef_probs[band][ctx];
        //     if (!read_bool(r, prob[EOB_CONTEXT_NODE], …)) break;
        //     while (!read_bool(r, prob[ZERO_CONTEXT_NODE], …)) {
        //       token_cache[scan[c]] = 0;
        //       ++c;
        //       if (c >= max_eob) return c;      // zero tokens at the end
        //       ctx = get_coef_context(nb, token_cache, c);
        //       band = *band_translate++;        // <-- **im** Rumpf
        //       prob = coef_probs[band][ctx];
        //     }
        //     if (read_bool(r, prob[ONE_CONTEXT_NODE], …)) { … }
        // ```
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Unterschied **nicht** kosmetisch
        // ist: steht `prob[ZERO_CONTEXT_NODE] == 1` schon beim **ersten** Lesen,
        // wird die ZERO-Schleife **nie** betreten — und dann wird der Zaeiger
        // **kein** Mal weitergeholt. Mein Code holte ihn trotzdem und las damit
        // schon beim **ersten** Token `band_translate[1]` statt `[0]`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das bei Bild 1 genau **einen** Token
        // kippt und nicht nur den Wert: GEMESSEN ist, dass der Orakel bei
        // `c = 0` `band = 0` meldet und mein Dekoder `band = 1`. GEMESSEN ist,
        // dass `COEFBAND_TRANS_8X8PLUS[0] = 0` und `[1] = 1` — also war der
        // Wert **falsch**, und mit dem falschen Band die falschen drei
        // Wahrscheinlichkeiten. Aus **einem** Token wurden **54**.
        let mut band = band_translate[bp] as usize;
        bp += 1;

        // ---- EOB
        if b.read(probs[versatz(ebene, REF_INTRA, band, ctx, EOB_CONTEXT_NODE)]) == 0 {
            break;
        }
        // ---- ZERO: GEMESSEN als `while`, also **mehrfach** moeglich
        loop {
            if b.read(probs[versatz(ebene, REF_INTRA, band, ctx, ZERO_CONTEXT_NODE)]) != 0 {
                break;
            }
            cache[scan[c] as usize] = 0;
            tokens.push(Token { c, scan: scan[c] as usize, band, ctx, zweig: "ZERO", tok: 0, v: 0, dqcoeff: 0 });
            c += 1;
            if c >= max_eob {
                // GEMESSEN: `return c;  // zero tokens at the end (no eob token)`
                return Ergebnis { tokens, max_eob, eob: c };
            }
            ctx = coef_context(nb, &cache, c);
            // GEMESSEN: **hier** und nur hier, am Ende des Rumpfes.
            band = band_translate[bp] as usize;
            bp += 1;
        }

        // ---- ONE / TWO / … / CAT6
        let (tok, v, zweig) = if b.read(probs[versatz(ebene, REF_INTRA, band, ctx, ONE_CONTEXT_NODE)]) == 0 {
            // GEMESSEN: `token_cache[scan[c]] = 1; v = dqv >> dq_shift;`
            cache[scan[c] as usize] = 1;
            (1u8, dqv >> dq_shift, "ONE")
        } else {
            // GEMESSEN: `vp9_pareto8_full[prob[PIVOT_NODE] - 1]`
            let p = &PARETO8[(probs[versatz(ebene, REF_INTRA, band, ctx, PIVOT_NODE)] - 1) as usize];
            if b.read(p[0]) != 0 {
                if b.read(p[3]) != 0 {
                    cache[scan[c] as usize] = 5;
                    if b.read(p[5]) != 0 {
                        if b.read(p[7]) != 0 {
                            // GEMESSEN: CAT6_MIN_VAL + read_coeff(cat6_prob, 14)
                            (5u8, ((CAT6_MIN_VAL + read_coeff(b, &CAT6_PROB)) * dqv) >> dq_shift, "CAT6")
                        } else {
                            (5u8, ((CAT5_MIN_VAL + read_coeff(b, &CAT5_PROB)) * dqv) >> dq_shift, "CAT5")
                        }
                    } else if b.read(p[6]) != 0 {
                        (5u8, ((CAT4_MIN_VAL + read_coeff(b, &CAT4_PROB)) * dqv) >> dq_shift, "CAT4")
                    } else {
                        (5u8, ((CAT3_MIN_VAL + read_coeff(b, &CAT3_PROB)) * dqv) >> dq_shift, "CAT3")
                    }
                } else {
                    cache[scan[c] as usize] = 4;
                    if b.read(p[4]) != 0 {
                        (4u8, ((CAT2_MIN_VAL + read_coeff(b, &CAT2_PROB)) * dqv) >> dq_shift, "CAT2")
                    } else {
                        (4u8, ((CAT1_MIN_VAL + read_coeff(b, &CAT1_PROB)) * dqv) >> dq_shift, "CAT1")
                    }
                }
            } else if b.read(p[1]) != 0 {
                // GEMESSEN: `token_cache[scan[c]] = 3; v = ((3 + read_bool(p[2])) *
                // dqv) >> dq_shift;` — also das Bit **im Wert**, nicht separat.
                cache[scan[c] as usize] = 3;
                let vier = 3 + b.read(p[2]) as i32;
                (3u8, (vier * dqv) >> dq_shift, "DREI")
            } else {
                cache[scan[c] as usize] = 2;
                (2u8, (2 * dqv) >> dq_shift, "ZWEI")
            }
        };

        // ---- Vorzeichen: GEMESSEN `read_bool(r, 128, …) ? -v : v`
        let negativ = b.read(128) != 0;
        let pos = scan[c] as usize;
        tokens.push(Token {
            c,
            scan: pos,
            band,
            ctx,
            zweig,
            tok,
            v,
            dqcoeff: if negativ { -v } else { v },
        });
        cache[pos] = tok;

        c += 1;
        ctx = coef_context(nb, &cache, c);
        dqv = dq[1];
    }
    Ergebnis { tokens, max_eob, eob: c }
}

/// Der Scan und die Nachbarn fuer eine Transformationsgroesse.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **nur 32x32** steht: GEMESSEN ist, dass
/// `NB_4X4`, `NB_8X8` und `NB_16X16` **kleinere** Nachbar-Tabellen sind, und
/// GEMESSEN ist der erste Block `tx_size = TX_32X32`. Eine
/// All-In-Eine-Variante waere geraten, also sind es drei.
pub fn scan_und_nachbarn(tx_size: usize) -> (&'static [i16], &'static [i16]) {
    match tx_size {
        0 => (&crate::scan::SCAN_4X4, &NB_4X4),
        1 => (&crate::scan::SCAN_8X8, &NB_8X8),
        2 => (&crate::scan::SCAN_16X16, &NB_16X16),
        _ => (&crate::scan::SCAN_32X32, &NB_32X32),
    }
}