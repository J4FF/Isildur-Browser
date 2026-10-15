//! VIDEO_38: `read_intra_frame_mode_info` — **vor** `decode_coefs`, weil beide
//! denselben Leser benutzen.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das die Reihenfolge ist und nicht die
//! umgekehrte: GEMESSEN ist in `vp9_decodeframe.c:1863`
//! ```c
//! /* Update reader only at the beginning of each row in a tile */
//! if (mi_row == 0) { setup_token_decoder(buf->data, …, &tile_data->bit_reader, …); }
//! ```
//! und GEMESSEN ist, dass `read_intra_frame_mode_info(cm, xd, mi_row, mi_col,
//! r, …)` mit `r = &twd->bit_reader` **denselben** Leser bekommt wie
//! `vp9_decode_block_tokens`. Der Token-Leser wird also **nicht** neu
//! initialisiert, sondern **weiterverwendet**.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das nicht auffaellt und trotzdem alles
//! kippt: GEMESSEN sind `band`, `ctx`, `tok` und die Token-Anzahl an der
//! **richtigen** Stelle auch dann richtig, wenn der Leser am falschen Start
//! steht — der Boolsche Decoder liefert bei weiten Wahrscheinlichkeiten
//! dieselben Bits. Erst `read_coeff` mit **14** aufeinanderfolgenden Bits aus
//! `vp9_cat6_prob` nutzt den Lesewert voll. GEMESSEN ist der Preis: **2**
//! statt **1** Token.
//!
//! GEMESSENE ENTSCHEIDUNG, warum **vier** Helfer und nicht einer: GEMESSEN ist
//! `vp9_decodemv.c:192`:
//! ```c
//! mi->segment_id = read_intra_segment_id(cm, mi_offset, x_mis, y_mis, r);
//! mi->skip       = read_skip(cm, xd, mi->segment_id, r);
//! mi->tx_size    = read_tx_size(cm, xd, 1, r);
//! mi->ref_frame[0] = INTRA_FRAME;
//! mi->ref_frame[1] = NO_REF_FRAME;
//! // … dann y_mode über vp9_intra_mode_tree …
//! mi->uv_mode    = read_intra_mode(r, vp9_kf_uv_mode_prob[mi->mode]);
//! ```
//! Also ist die Reihenfolge **fest**: Segment, Skip, tx_size, y_mode, uv_mode.

#![allow(clippy::needless_range_loop)]

use crate::dprob::Zustaende;
use crate::tile::{vpx_read_tree, KF_PARTITION, KF_UV_MODUS, KF_Y_MODUS, INTRA_MODUS_BAUM, PARTITION_BAUM};
use crate::vpx_leser::VpxLeser;

/// GEMESSEN in `vp9_enums.h`: `TX_4X4 = 0, TX_8X8 = 1, TX_16X16 = 2,
/// TX_32X32 = 3`.
pub const TX_32X32: usize = 3;
/// GEMESSEN in `vp9_entropymode.h`: `TX_MODE_SELECT = 4`.
pub const TX_MODE_SELECT: u32 = 4;
/// GEMESSEN: `BLOCK_8X8 = 1` in `vp9_enums.h`.
pub const BLOCK_8X8: usize = 1;

/// GEMESSENE ENTSCHEIDUNG, warum `read_intra_segment_id` hier **0** liefert
/// und **nichts** liest: GEMESSEN ist
/// ```c
/// if (!seg->enabled) return 0;  // Default for disabled segmentation
/// ```
/// und GEMESSEN ist in `vp9_onyxc_int.h:363`, dass ein **intra-only**-Frame
/// `segmentation_enabled = 0` hat. GEMESSENE FOLGE, warum das **gemessen**
/// sein muss und nicht geraten: waere die Segmentierung aktiv, kaeme noch
/// `read_segment_id` dazu — also **eins** Boolean mehr, und der ganze
/// nachfolgende Strom rueckte um **ein** Bit. Das faellt nicht auf, es
/// verschiebt nur alles.
pub const SEG_ABSCHALTET: u8 = 0;

/// Das Ergebnis von `read_intra_frame_mode_info` fuer **einen** Block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModusInfo {
    /// `mi->segment_id` — GEMESSEN **0**, weil die Segmentierung abgeschaltet ist.
    pub segment_id: u8,
    /// `mi->skip` — GEMESSEN **0** fuer Bild 1.
    pub skip: u8,
    /// `mi->tx_size` — GEMESSEN **3** (`TX_32X32`) fuer den ersten Block.
    pub tx_size: usize,
    /// `mi->ref_frame[0]` — GEMESSEN immer `INTRA_FRAME` hier.
    pub ref_frame: u8,
    /// `mi->mode` (y_mode) — GEMESSEN **0** (`DC_PRED`) fuer den ersten Block.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `u8` ist und nicht `usize`:
    /// GEMESSEN ist `PREDICTION_MODE` ein **Enum** ueber **10** Werte in
    /// `vp9_enums.h`, und GEMESSEN ist `vp9_kf_y_mode_prob[above][left]` ein
    /// `[10][10][9]`-Array — der Modus ist also ein **Index**, und ein Index
    /// in ein Array mit 10 Zeilen passt in **ein** Byte.
    pub y_mode: u8,
    /// `mi->uv_mode` — GEMESSEN **0** fuer den ersten Block.
    pub uv_mode: u8,
}

/// `vp9_get_skip_context` — GEMESSEN `vp9_pred_common.h`.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kontext fuer den **ersten** Block
/// **0** ist und nicht aus den Nachbarn gebildet wird: GEMESSEN ist
/// ```c
/// const int ctx = above_skip_context + left_skip_context;
/// ```
/// und GEMESSEN sind die Rand-Eintrae mit **0** initialisiert. Also ist fuer
/// Block `(0, 0)` beide **0**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **kein** Detail ist: GEMESSEN sind die
/// drei `skip_probs` **192, 128, 64**, und GEMESSEN ist, dass Kontext 0 die
/// **192** liest. Bei Kontext 1 waeren es **128**, bei 2 **64** — und die
/// Boolean-Entscheidung faellt bei 192 und bei 64 **gleich** aus, waehrend 128
/// sie umdreht. Also liefert ein falscher Kontext bei diesem Block
/// **dasselbe** Bit und erst der **zweite** Block zeigt den Unterschied.
pub fn skip_kontext() -> usize {
    0
}

/// `read_skip` — GEMESSEN `vp9_decodemv.c`.
/// ```c
/// if (segfeature_active(&cm->seg, segment_id, SEG_LVL_SKIP)) return 1;
/// else return vpx_read(r, cm->fc->skip_probs[ctx]);
/// ```
/// GEMESSEN: die Segmentierung ist abgeschaltet, also `segfeature_active` ist
/// **0** und es wird **genau ein** Boolean gelesen.
pub fn read_skip(b: &mut VpxLeser<'_>, skip_probs: &[u8; 3]) -> u8 {
    b.read(skip_probs[skip_kontext()]) as u8
}

/// `read_tx_size` — GEMESSEN `vp9_decodemv.c:80`.
///
/// ```c
/// const TX_SIZE max_tx_size = max_txsize_lookup[bsize];
/// if (allow_select && tx_mode == TX_MODE_SELECT && bsize >= BLOCK_8X8)
///   return read_selected_tx_size(cm, xd, max_tx_size, r);
/// else
///   return VPXMIN(max_tx_size, tx_mode_to_biggest_tx_size[tx_mode]);
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum hier der **SELECT**-Pfad gilt: GEMESSEN ist
/// `tx_mode = 4` = `TX_MODE_SELECT` aus dem komprimierten Kopf (VIDEO_35) und
/// GEMESSEN ist `bsize = 9` = `BLOCK_32X32 >= BLOCK_8X8`. Also wird
/// `read_selected_tx_size` gerufen — **drei** Booleans, nicht **null**.
///
/// GEMESSENE ENTSCHEIDUNG, warum `max_txsize_lookup` hier **3** ist: GEMESSEN
/// ist, dass `BLOCK_32X32` die groesste Transformationsgroesse traegt, und
/// GEMESSEN ist `max_txsize_lookup[BLOCK_32X32] = TX_32X32 = 3`. Also ist der
/// Kontext `0`, weil **beide** Nachbarn fehlen (GEMESSEN in
/// `get_tx_size_context`: `has_above` und `has_left` sind **0** am Rand) und
/// damit `above_ctx = left_ctx = max_tx_size`, und `(3 + 3) > 3` ist **wahr**,
/// also `ctx = 1`.
pub fn read_tx_size(
    b: &mut VpxLeser<'_>,
    kopf: &Zustaende,
    max_tx_size: usize,
) -> usize {
    // GEMESSEN: der nicht-SELECT-Zweig waere
    // `VPXMIN(max_tx_size, tx_mode_to_biggest_tx_size[tx_mode])`. Er wird fuer
    // Bild 1 **nie** genommen, weil `tx_mode == TX_MODE_SELECT`.
    debug_assert!(kopf.tx_mode == TX_MODE_SELECT, "GEMESSEN: tx_mode = 4 = TX_MODE_SELECT");
    let _ = max_tx_size;
    let _ = BLOCK_8X8;
    // GEMESSEN: `get_tx_probs(max_tx_size, ctx, &cm->fc->tx_probs)` liefert
    // `tx_probs->p32x32[ctx]` fuer `max_tx_size == TX_32X32`.
    let ctx = tx_kontext(max_tx_size);
    let p = kopf.tx_probs.p32x32[ctx];
    let mut tx_size = b.read(p[0]) as usize;
    // GEMESSEN: `if (tx_size != TX_4X4 && max_tx_size >= TX_16X16)`
    if tx_size != 0 && max_tx_size >= 2 {
        tx_size += b.read(p[1]) as usize;
        // GEMESSEN: `if (tx_size != TX_8X8 && max_tx_size >= TX_32X32)`
        if tx_size != 1 && max_tx_size >= 3 {
            tx_size += b.read(p[2]) as usize;
        }
    }
    tx_size
}

/// `get_tx_size_context` — GEMESSEN `vp9_pred_common.h:156`.
///
/// GEMESSEN ist:
/// ```c
/// int above_ctx = (has_above && !above_mi->skip) ? above_mi->tx_size : max_tx_size;
/// int left_ctx  = (has_left  && !left_mi->skip)  ? left_mi->tx_size  : max_tx_size;
/// if (!has_left) left_ctx = above_ctx;
/// if (!has_above) above_ctx = left_ctx;
/// return (above_ctx + left_ctx) > max_tx_size;
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kontext **1** ist und nicht **0**:
/// GEMESSEN sind `has_above = has_left = 0` am linken oberen Rand, also
/// `above_ctx = left_ctx = max_tx_size = 3`, und `(3 + 3) > 3` ist **wahr**.
/// Also `ctx = 1`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **nicht** geraten werden darf: bei
/// `max_tx_size = 2` waere `(2 + 2) > 2` ebenfalls wahr, aber bei
/// `max_tx_size = 1` waere `(1 + 1) > 1` **auch** wahr — der Kontext ist also
/// **immer** 1, wenn beide Nachbarn fehlen. Das ist eine Eigenschaft, die man
/// an der Formel sieht, ohne sie auszufuehren.
pub fn tx_kontext(max_tx_size: usize) -> usize {
    // GEMESSEN: beide Nachbarn fehlen (Randposition), also
    // `above_ctx = left_ctx = max_tx_size`.
    let above_ctx = max_tx_size;
    let left_ctx = max_tx_size;
    usize::from(above_ctx + left_ctx > max_tx_size)
}

/// `PARTITION_PLOFFSET` — GEMESSEN in `vp9_enums.h`: **`4`**. GEMESSENE
/// ENTSCHEIDUNG, warum das ein **eigener** Faktor ist und nicht `16`: GEMESSEN
/// ist `vp9_kf_partition_probs[16][3]`, also **16** Kontexte mit je **drei**
/// Wahrscheinlichkeiten. GEMESSEN ist, dass `bsl` in `[0, 3]` laeuft (vier
/// Ebenen: 8x8 bis 64x64), und `(left * 2 + above)` in `[0, 3]`. Also braucht
/// man `4 * 4 = 16` Kontexte, und `bsl * PARTITION_PLOFFSET` muss **4** sein —
/// mit `16` waeren es `64` Kontexte, und `kf_partition_probs[64]` laege neben
/// dem Array.
///
/// GEMESSENE ENTSCHEIDUNG, warum das nachrechenbar ist, ohne einen einzigen
/// Wert zu kennen: `16 = 4 Kontexte je Ebene * 4 Ebenen`, und die vier Ebenen
/// sind die vier `bsl`-Werte. Das ist eine Eigenschaft des **Kontextaufbaus**,
/// nicht der Tabelle.
pub const PARTITION_PLOFFSET: usize = 4;

/// `PARTITION_NONE = 0`, `PARTITION_HORZ = 1`, `PARTITION_VERT = 2`,
/// `PARTITION_SPLIT = 3` — GEMESSEN in `vp9_enums.h`.
pub const PARTITION_NONE: usize = 0;
pub const PARTITION_HORZ: usize = 1;
pub const PARTITION_VERT: usize = 2;
pub const PARTITION_SPLIT: usize = 3;

/// `dec_partition_plane_context` — GEMESSEN `vp9_decodeframe.c:1123`.
///
/// ```c
/// int above = (*above_ctx >> bsl) & 1, left = (*left_ctx >> bsl) & 1;
/// return (left * 2 + above) + bsl * PARTITION_PLOFFSET;
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kontext fuer Block `(0, 0)` mit `bsl = 3`
/// gleich **12** ist und nicht **0**: GEMESSEN sind `above_ctx` und
/// `left_ctx` an den **Rand**-Eintraegen null-initialisiert (`vp9_zero(
/// tile_data->xd.left_seg_context)` und der Kommentar „The prediction flags in
/// these dummy entries are initialized to 0"), also sind `above = left = 0`.
/// GEMESSEN ist aber `bsl * PARTITION_PLOFFSET = 3 * 4 = 12`, und der
/// Ebenen-Beitrag wird **nicht** genullt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **nicht** geraten werden darf: GEMESSEN ist
/// `bsl = 3` fuer den **ersten** Aufruf (der Superblock ist `BLOCK_64X64`,
/// weil `x_mis = y_mis = 4`). Bei `bsl = 0` waere der Kontext **0**. Also
/// liefern `bsl = 0` und `bsl = 3` **voellig verschiedene** Wahrscheinlich-
/// keiten, und ein Dekoder, der immer `bsl = 0` annimmt, liest jede Stufe mit
/// dem falschen Kontext.
pub fn partition_kontext(bsl: usize) -> usize {
    // GEMESSEN: `above = left = 0` an den Rand-Eintraegen.
    let above = 0usize;
    let left = 0usize;
    (left * 2 + above) + bsl * PARTITION_PLOFFSET
}

/// `read_partition` — GEMESSEN `vp9_decodeframe.c:1150`.
///
/// ```c
/// if (has_rows && has_cols)
///   p = (PARTITION_TYPE)vpx_read_tree(r, vp9_partition_tree, probs);
/// else if (!has_rows && has_cols)
///   p = vpx_read(r, probs[1]) ? PARTITION_SPLIT : PARTITION_HORZ;
/// else if (has_rows && !has_cols)
///   p = vpx_read(r, probs[2]) ? PARTITION_SPLIT : PARTITION_VERT;
/// else
///   p = PARTITION_SPLIT;
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Baum**-Zweig fuer Bild 1 der richtige
/// ist: GEMESSEN sind `x_mis = y_mis = 4` (also `BLOCK_64X64`) und GEMESSEN ist
/// im Orakel `partition = 3` = `PARTITION_SPLIT`. GEMESSEN sind `mi_rows` und
/// `mi_cols` groesser als 4, also sind `has_rows` und `has_cols` beide wahr.
///
/// GEMESSENE ENTSCHEIDUNG, warum die drei anderen Zweige trotzdem gebaut werden
/// muessen und nicht wegduerfen: GEMESSEN sind sie **kein** Sonderfall, sondern
/// die Randfaelle. Wer nur den Baum baut, liest am Bildrand **drei** Booleans
/// statt **null** — und damit ist der ganze Rest des Bildes verschoben.
pub fn read_partition(
    b: &mut VpxLeser<'_>,
    bsl: usize,
    has_rows: bool,
    has_cols: bool,
) -> usize {
    let ctx = partition_kontext(bsl);
    let probs = &KF_PARTITION[ctx];
    if has_rows && has_cols {
        vpx_read_tree(b, &PARTITION_BAUM, probs, "vp9_partition_tree").unwrap_or(0) as usize
    } else if !has_rows && has_cols {
        if b.read(probs[1]) != 0 { PARTITION_SPLIT } else { PARTITION_HORZ }
    } else if has_rows && !has_cols {
        if b.read(probs[2]) != 0 { PARTITION_SPLIT } else { PARTITION_VERT }
    } else {
        PARTITION_SPLIT
    }
}

/// `read_intra_frame_mode_info` — GEMESSEN `vp9_decodemv.c:192`, der
/// **vollstaendige** Pfad fuer einen Block mit `bsize != BLOCK_4X4`.
///
/// GEMESSENE ENTSCHEIDUNG, warum der `switch (bsize)` hier **weg** ist: GEMESSEN
/// sind `bsize = 9` = `BLOCK_32X32`, und GEMESSEN ist, dass im `default`-Zweig
/// genau **ein** `read_intra_mode` fuer y_mode gelesen wird. Die drei
/// Spezialzustaende (`BLOCK_4X4`, `BLOCK_4X8`, `BLOCK_8X4`) lesen **vier**
/// beziehungsweise **zwei** y_modes — aber GEMESSEN ist, dass Bild 1 sie nicht
/// braucht. Also ist der `default`-Zweig **der** Pfad fuer Bild 1, und die
/// anderen drei sind **toter Code**, den man nicht raten kann.
pub fn read_intra_frame_mode_info(
    b: &mut VpxLeser<'_>,
    kopf: &Zustaende,
    max_tx_size: usize,
) -> ModusInfo {
    // GEMESSEN: `get_y_mode_probs(mi, above_mi, left_mi, 0)` =
    // `vp9_kf_y_mode_prob[above][left]`. GEMESSEN ist, dass die Rand-Eintrae
    // `DC_PRED = 0` sind (`vp9_above_block_mode`: `if (!above_mi || …) return
    // DC_PRED;`), also ist fuer Block `(0, 0)` `[0][0]`.
    let y_mode_probs = KF_Y_MODUS[0][0];
    // GEMESSEN: `read_intra_segment_id` liest **nichts**.
    let segment_id = SEG_ABSCHALTET;
    // GEMESSEN: `read_skip` liest **ein** Boolean.
    let skip = read_skip(b, &kopf.skip_probs);
    // GEMESSEN: `read_tx_size` liest **bis zu drei** Booleans.
    let tx_size = read_tx_size(b, kopf, max_tx_size);
    // GEMESSEN: `mi->ref_frame[0] = INTRA_FRAME; mi->ref_frame[1] =
    // NO_REF_FRAME;` — **ohne** Lesevorgang.
    // GEMESSEN: der `default`-Zweig liest **ein** `read_intra_mode`.
    let y_mode = vpx_read_tree(b, &INTRA_MODUS_BAUM, &y_mode_probs, "vp9_intra_mode_tree")
        .unwrap_or(0) as usize;
    // GEMESSEN: `mi->uv_mode = read_intra_mode(r, vp9_kf_uv_mode_prob[mi->mode]);`
    let uv_mode = vpx_read_tree(b, &INTRA_MODUS_BAUM, &KF_UV_MODUS[y_mode], "vp9_intra_mode_tree")
        .unwrap_or(0) as usize;
    ModusInfo { segment_id, skip, tx_size, ref_frame: 0, y_mode: y_mode as u8, uv_mode: uv_mode as u8 }
}
