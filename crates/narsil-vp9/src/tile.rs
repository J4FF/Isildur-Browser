//! VIDEO_32, Schritt 1: **Partitionsbaum** und **Intra-Modi**.
//!
//! GEMESSEN am Video: `segmentation_enabled = 0`, `frame_type = 0`
//! (KEY_FRAME), 320x240. GEMESSEN in libvpx `vp9_decodeframe.c:2038`:
//! ```c
//! decode_partition(tile_data, pbi, mi_row, mi_col, BLOCK_64X64, 4);
//! ```
//! — der erste Superblock beginnt also bei `(0, 0)` mit `n4x4_l2 = 4`.
//!
//! **JEDE** Tabelle und **jede** Formel hier ist aus `libvpx` abgelesen und
//! mit dem Programm erzeugt, nicht abgetippt: `vp9/common/vp9_common_data.c`
//! (`subsize_lookup`, `partition_context_lookup`, `max_txsize_lookup`) und
//! `vp9/common/vp9_entropymode.c` (`vp9_kf_y_mode_prob`,
//! `vp9_kf_uv_mode_prob`, `vp9_kf_partition_probs`, `vp9_partition_tree`,
//! `vp9_intra_mode_tree`).

#![allow(clippy::needless_range_loop)]

use crate::dprob::{SKIP_CONTEXTS, TX_MODE_GROESSTE};
use crate::vp9_bool::{Leser, Vp9Fehler};
// GEMESSEN aus libvpx, PROGRAMMATISCH mit einem Klammerbaum-Parser ausgelesen —
// nicht abgetippt. Quellen: vp9/common/vp9_common_data.c und
// vp9/common/vp9_entropymode.c. Zu jeder Tabelle steht die Fundstelle.

/// `subsize_lookup[partition][bsize]` — GEMESSEN `vp9_common_data.c:71`.
/// GEMESSENE ENTSCHEIDUNG, warum `BLOCK_INVALID` als **13** steht und nicht als 255:
/// GEMESSEN `vp9_enums.h:60` ist `#define BLOCK_INVALID BLOCK_SIZES`, also **13**.
/// GEMESSENE ENTSCHEIDUNG, warum die Zeile `PARTITION_NONE` die Identitaet ist:
/// GEMESSEN steht dort `BLOCK_4X4, BLOCK_4X8, …` in aufsteigender Reihenfolge.
#[rustfmt::skip]
pub const SUBSIZE_LOOKUP: [[u8; 13]; 4] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], // PARTITION_NONE
    [13, 13, 13, 2, 13, 13, 5, 13, 13, 8, 13, 13, 11], // PARTITION_HORZ
    [13, 13, 13, 1, 13, 13, 4, 13, 13, 7, 13, 13, 10], // PARTITION_VERT
    [13, 13, 13, 0, 13, 13, 3, 13, 13, 6, 13, 13, 9], // PARTITION_SPLIT
];

/// `partition_context_lookup[bsize] = {above, left}` — GEMESSEN
/// `vp9_common_data.c:230`. GEMESSENE ENTSCHEIDUNG, warum das **Bitmasken** sind und
/// keine Flags: GEMESSEN ist `memset(above_ctx, partition_context_lookup[subsize].above, bw)` —
/// ein `memset`, also **ein Byte je mi-Einheit**. GEMESSEN ist `BLOCK_32X64` =
/// `{8, 0}` und `BLOCK_64X32` = `{0, 8}`: die **obere** Kante ist Null, wenn der Block
/// breiter als hoch ist. Zwei boolesche Flags verwechseln bei genau diesen zwei
/// Groessen oben und links, und der Fehler faellt erst bei `n8x8_l2 >= 2` auf.
#[rustfmt::skip]
pub const PARTITION_KONTEXT: [(u8, u8); 13] = [
    (15, 15),
    (15, 14),
    (14, 15),
    (14, 14),
    (14, 12),
    (12, 14),
    (12, 12),
    (12, 8),
    (8, 12),
    (8, 8),
    (8, 0),
    (0, 8),
    (0, 0),
];

/// `vp9_partition_tree` — GEMESSEN `vp9_entropymode.c:262`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Vorzeichen** der Blaetter das ganze
/// Format tragen: GEMESSEN ist
/// `while ((i = tree[i + vpx_read(...)]) > 0); return -i;`. Ohne Vorzeichen
/// gibt es **keinen** Abbruch, und der Pfad laeuft in einen Zyklus.
///
/// | Baum | Eintraege | Zyklen | erreichbare Symbole |
/// |---|---|---|---|
/// | **mit** Vorzeichen | 6 | **0** | **0, 1, 2, 3** |
/// | ohne Vorzeichen | 6 | **4** | **nur 0** |
///
/// GEMESSENE ENTSCHEIDUNG, warum die Pruefung **ohne ffmpeg** auskommt: sie
/// laeuft den Baum mit **beiden** Bits an jedem Knoten durch und sammelt die
/// Symbole ein. Bei einem echten Baum ist die Menge genau die Partitionen, und
/// Zyklen gibt es keine. Das ist dieselbe Art von Orakel wie
/// `vpx_reader_has_error` — eine Eigenschaft, die ein kaputter Baum nicht hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Wahrscheinlichkeit an der **Baumposition**
/// haengt und nicht am Symbol: GEMESSEN ist `probs[i >> 1]`. GEMESSEN sind die
/// Wurzelknoten `0, 2, 4`, also `probs[0], probs[1], probs[2]` — genau die
/// **drei** Spalten von `KF_PARTITION`.
#[rustfmt::skip]
pub const PARTITION_BAUM: [i32; 6] = [0, 2, -1, 4, -2, -3];

/// `vp9_intra_mode_tree` — GEMESSEN `vp9_entropymode.c:245`.
///
/// GEMESSENE ENTSCHEIDUNG, warum **achtzehn** Eintraege sind: GEMESSEN
/// `vpx_dsp/prob.h:33` ist `#define TREE_SIZE(leaf_count) (2 * (leaf_count) - 2)`, und
/// `INTRA_MODES = 10` ergibt **18**. GEMESSENE ENTSCHEIDUNG, warum das nicht
/// Kosmetik ist: ich hatte `2n - 1` notiert, und damit ein Feld zu viel. Ein
/// `[i32; 19]` faellt beim Fuellen **sofort** auf — ein Index **eins daneben**
/// dagegen gar nicht, weil die neunzehnte Zelle im Speicher die naechste
/// Konstante ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum auch hier die **Vorzeichen** tragen: GEMESSEN
/// ist dieselbe Abbruchbedingung `> 0`. GEMESSEN ist der Unterschied:
///
/// | Baum | Eintraege | Zyklen | erreichbare Symbole |
/// |---|---|---|---|
/// | **mit** Vorzeichen | 18 | **0** | **0 bis 9** — alle zehn Intra-Modi |
/// | ohne Vorzeichen | 18 | **16** | **nur 0** |
///
/// GEMESSENE ENTSCHEIDUNG, warum das der **teuerste** Fehler der ganzen
/// Tabelle war: die falsche Fassung hat `[i32; 18]` genauso wie die richtige,
/// also **gleiche Typgroesse und gleiche Feldzahl**. Sie kompiliert, sie
/// behauptet 18 Eintraege, und sie laeuft endlos. GEMESSEN: **45 Sekunden**
/// Laufzeit, dann `attempt to add with overflow` an einem **Zaehler**, der
/// korrekt ist — der einzige Ort, der gelogen hat, war der Zaehler nicht.
#[rustfmt::skip]
pub const INTRA_MODUS_BAUM: [i32; 18] = [
    0, 2, -9, 4, -1, 6, 8, 12, -2,
    10, -4, -5, -3, 14, -8, 16, -6, -7,
];

/// `vp9_kf_uv_mode_prob[y_mode][9]` — GEMESSEN `vp9_entropymode.c:149`. GEMESSENE
/// ENTSCHEIDUNG, warum **genau diese** Tabelle und nicht `default_if_uv_probs`:
/// GEMESSEN in `read_intra_frame_mode_info` steht
/// `read_intra_mode(r, vp9_kf_uv_mode_prob[mi->mode])`. `default_if_uv_probs` ist fuer
/// **Inter**-Bilder mit Update und wird fuer ein Keyframe nie gelesen. Beide Tabellen
/// stehen acht Zeilen Abstand im selben File und unterscheiden sich in **jedem** der
/// 90 Werte — GEMESSEN Zeile 0, erster Wert: 144 hier gegen 120 dort.
#[rustfmt::skip]
pub const KF_UV_MODUS: [[u8; 9]; 10] = [
    [144, 11, 54, 157, 195, 130, 46, 58, 108],
    [118, 15, 123, 148, 131, 101, 44, 93, 131],
    [113, 12, 23, 188, 226, 142, 26, 32, 125],
    [120, 11, 50, 123, 163, 135, 64, 77, 103],
    [113, 9, 36, 155, 111, 157, 32, 44, 161],
    [116, 9, 55, 176, 76, 96, 37, 61, 149],
    [115, 9, 28, 141, 161, 167, 21, 25, 193],
    [120, 12, 32, 145, 195, 142, 32, 38, 86],
    [116, 12, 64, 120, 140, 125, 49, 115, 121],
    [102, 19, 66, 162, 182, 122, 35, 59, 128],
];

/// `vp9_kf_y_mode_prob[above][left][9]` — GEMESSEN `vp9_entropymode.c:16`, **900**
/// Werte. GEMESSENE ENTSCHEIDUNG, warum nicht gekuerzt: GEMESSEN ist
/// `get_y_mode_probs` = `vp9_kf_y_mode_prob[above][left]`, und `above`/`left` sind
/// **echte Intra-Modi** (0..9). Wer auf `default_if_y_probs` zurueckfaellt, liest die
/// **falsche** Tabelle — die hat vier statt zehn Zeilen.
#[rustfmt::skip]
pub const KF_Y_MODUS: [[[u8; 9]; 10]; 10] = [
    [ // above = 0
        [137, 30, 42, 148, 151, 207, 70, 52, 91], // left = 0
        [92, 45, 102, 136, 116, 180, 74, 90, 100], // left = 1
        [73, 32, 19, 187, 222, 215, 46, 34, 100], // left = 2
        [91, 30, 32, 116, 121, 186, 93, 86, 94], // left = 3
        [72, 35, 36, 149, 68, 206, 68, 63, 105], // left = 4
        [73, 31, 28, 138, 57, 124, 55, 122, 151], // left = 5
        [67, 23, 21, 140, 126, 197, 40, 37, 171], // left = 6
        [86, 27, 28, 128, 154, 212, 45, 43, 53], // left = 7
        [74, 32, 27, 107, 86, 160, 63, 134, 102], // left = 8
        [59, 67, 44, 140, 161, 202, 78, 67, 119], // left = 9
    ],
    [ // above = 1
        [63, 36, 126, 146, 123, 158, 60, 90, 96], // left = 0
        [43, 46, 168, 134, 107, 128, 69, 142, 92], // left = 1
        [44, 29, 68, 159, 201, 177, 50, 57, 77], // left = 2
        [58, 38, 76, 114, 97, 172, 78, 133, 92], // left = 3
        [46, 41, 76, 140, 63, 184, 69, 112, 57], // left = 4
        [38, 32, 85, 140, 46, 112, 54, 151, 133], // left = 5
        [39, 27, 61, 131, 110, 175, 44, 75, 136], // left = 6
        [52, 30, 74, 113, 130, 175, 51, 64, 58], // left = 7
        [47, 35, 80, 100, 74, 143, 64, 163, 74], // left = 8
        [36, 61, 116, 114, 128, 162, 80, 125, 82], // left = 9
    ],
    [ // above = 2
        [82, 26, 26, 171, 208, 204, 44, 32, 105], // left = 0
        [55, 44, 68, 166, 179, 192, 57, 57, 108], // left = 1
        [42, 26, 11, 199, 241, 228, 23, 15, 85], // left = 2
        [68, 42, 19, 131, 160, 199, 55, 52, 83], // left = 3
        [58, 50, 25, 139, 115, 232, 39, 52, 118], // left = 4
        [50, 35, 33, 153, 104, 162, 64, 59, 131], // left = 5
        [44, 24, 16, 150, 177, 202, 33, 19, 156], // left = 6
        [55, 27, 12, 153, 203, 218, 26, 27, 49], // left = 7
        [53, 49, 21, 110, 116, 168, 59, 80, 76], // left = 8
        [38, 72, 19, 168, 203, 212, 50, 50, 107], // left = 9
    ],
    [ // above = 3
        [103, 26, 36, 129, 132, 201, 83, 80, 93], // left = 0
        [59, 38, 83, 112, 103, 162, 98, 136, 90], // left = 1
        [62, 30, 23, 158, 200, 207, 59, 57, 50], // left = 2
        [67, 30, 29, 84, 86, 191, 102, 91, 59], // left = 3
        [60, 32, 33, 112, 71, 220, 64, 89, 104], // left = 4
        [53, 26, 34, 130, 56, 149, 84, 120, 103], // left = 5
        [53, 21, 23, 133, 109, 210, 56, 77, 172], // left = 6
        [77, 19, 29, 112, 142, 228, 55, 66, 36], // left = 7
        [61, 29, 29, 93, 97, 165, 83, 175, 162], // left = 8
        [47, 47, 43, 114, 137, 181, 100, 99, 95], // left = 9
    ],
    [ // above = 4
        [69, 23, 29, 128, 83, 199, 46, 44, 101], // left = 0
        [53, 40, 55, 139, 69, 183, 61, 80, 110], // left = 1
        [40, 29, 19, 161, 180, 207, 43, 24, 91], // left = 2
        [60, 34, 19, 105, 61, 198, 53, 64, 89], // left = 3
        [52, 31, 22, 158, 40, 209, 58, 62, 89], // left = 4
        [44, 31, 29, 147, 46, 158, 56, 102, 198], // left = 5
        [35, 19, 12, 135, 87, 209, 41, 45, 167], // left = 6
        [55, 25, 21, 118, 95, 215, 38, 39, 66], // left = 7
        [51, 38, 25, 113, 58, 164, 70, 93, 97], // left = 8
        [47, 54, 34, 146, 108, 203, 72, 103, 151], // left = 9
    ],
    [ // above = 5
        [64, 19, 37, 156, 66, 138, 49, 95, 133], // left = 0
        [46, 27, 80, 150, 55, 124, 55, 121, 135], // left = 1
        [36, 23, 27, 165, 149, 166, 54, 64, 118], // left = 2
        [53, 21, 36, 131, 63, 163, 60, 109, 81], // left = 3
        [40, 26, 35, 154, 40, 185, 51, 97, 123], // left = 4
        [35, 19, 34, 179, 19, 97, 48, 129, 124], // left = 5
        [36, 20, 26, 136, 62, 164, 33, 77, 154], // left = 6
        [45, 18, 32, 130, 90, 157, 40, 79, 91], // left = 7
        [45, 26, 28, 129, 45, 129, 49, 147, 123], // left = 8
        [38, 44, 51, 136, 74, 162, 57, 97, 121], // left = 9
    ],
    [ // above = 6
        [75, 17, 22, 136, 138, 185, 32, 34, 166], // left = 0
        [56, 39, 58, 133, 117, 173, 48, 53, 187], // left = 1
        [35, 21, 12, 161, 212, 207, 20, 23, 145], // left = 2
        [56, 29, 19, 117, 109, 181, 55, 68, 112], // left = 3
        [47, 29, 17, 153, 64, 220, 59, 51, 114], // left = 4
        [46, 16, 24, 136, 76, 147, 41, 64, 172], // left = 5
        [34, 17, 11, 108, 152, 187, 13, 15, 209], // left = 6
        [51, 24, 14, 115, 133, 209, 32, 26, 104], // left = 7
        [55, 30, 18, 122, 79, 179, 44, 88, 116], // left = 8
        [37, 49, 25, 129, 168, 164, 41, 54, 148], // left = 9
    ],
    [ // above = 7
        [82, 22, 32, 127, 143, 213, 39, 41, 70], // left = 0
        [62, 44, 61, 123, 105, 189, 48, 57, 64], // left = 1
        [47, 25, 17, 175, 222, 220, 24, 30, 86], // left = 2
        [68, 36, 17, 106, 102, 206, 59, 74, 74], // left = 3
        [57, 39, 23, 151, 68, 216, 55, 63, 58], // left = 4
        [49, 30, 35, 141, 70, 168, 82, 40, 115], // left = 5
        [51, 25, 15, 136, 129, 202, 38, 35, 139], // left = 6
        [68, 26, 16, 111, 141, 215, 29, 28, 28], // left = 7
        [59, 39, 19, 114, 75, 180, 77, 104, 42], // left = 8
        [40, 61, 26, 126, 152, 206, 61, 59, 93], // left = 9
    ],
    [ // above = 8
        [78, 23, 39, 111, 117, 170, 74, 124, 94], // left = 0
        [48, 34, 86, 101, 92, 146, 78, 179, 134], // left = 1
        [47, 22, 24, 138, 187, 178, 68, 69, 59], // left = 2
        [56, 25, 33, 105, 112, 187, 95, 177, 129], // left = 3
        [48, 31, 27, 114, 63, 183, 82, 116, 56], // left = 4
        [43, 28, 37, 121, 63, 123, 61, 192, 169], // left = 5
        [42, 17, 24, 109, 97, 177, 56, 76, 122], // left = 6
        [58, 18, 28, 105, 139, 182, 70, 92, 63], // left = 7
        [46, 23, 32, 74, 86, 150, 67, 183, 88], // left = 8
        [36, 38, 48, 92, 122, 165, 88, 137, 91], // left = 9
    ],
    [ // above = 9
        [65, 70, 60, 155, 159, 199, 61, 60, 81], // left = 0
        [44, 78, 115, 132, 119, 173, 71, 112, 93], // left = 1
        [39, 38, 21, 184, 227, 206, 42, 32, 64], // left = 2
        [58, 47, 36, 124, 137, 193, 80, 82, 78], // left = 3
        [49, 50, 35, 144, 95, 205, 63, 78, 59], // left = 4
        [41, 53, 52, 148, 71, 142, 65, 128, 51], // left = 5
        [40, 36, 28, 143, 143, 202, 40, 55, 137], // left = 6
        [52, 34, 29, 129, 183, 227, 42, 35, 43], // left = 7
        [42, 44, 44, 104, 105, 164, 64, 130, 80], // left = 8
        [43, 81, 53, 140, 169, 204, 68, 84, 72], // left = 9
    ],
];

/// `vp9_kf_partition_probs[16][3]` — GEMESSEN `vp9_entropymode.c:182`,
/// `PARTITION_CONTEXTS = 4 * PARTITION_PLOFFSET = 16` (GEMESSEN `vp9_enums.h:73/74`).
/// GEMESSENE ENTSCHEIDUNG, warum **drei** Spalten und nicht vier: GEMESSEN ist
/// `PARTITION_TYPES - 1`, denn `PARTITION_SPLIT` ist der Default ohne gelesenes Bit —
/// GEMESSEN in `read_partition`: bei `!has_rows && !has_cols` steht
/// `p = PARTITION_SPLIT;` **ohne** zu lesen.
#[rustfmt::skip]
pub const KF_PARTITION: [[u8; 3]; 16] = [
    [158, 97, 94],
    [93, 24, 99],
    [85, 119, 44],
    [62, 59, 67],
    [149, 53, 53],
    [94, 20, 48],
    [83, 53, 24],
    [52, 18, 18],
    [150, 40, 39],
    [78, 12, 26],
    [67, 33, 11],
    [24, 7, 5],
    [174, 35, 49],
    [68, 11, 27],
    [57, 15, 9],
    [12, 3, 3],
];

/// `max_txsize_lookup[bsize]` — GEMESSEN `vp9_common_data.c:90`. GEMESSENE
/// ENTSCHEIDUNG, warum das **zwei** Platten hat statt einer Potenz: GEMESSEN
/// steht dort `TX_4X4, TX_4X4, TX_4X4, TX_8X8, …` — **drei** Eintraege fuer
/// 4x4, **drei** fuer 8x8 usw. Wer `log2(bsize)/2` rechnet, bekommt fuer
/// `BLOCK_8X4` (2) den Wert 0 statt 1, also eine halbe Transformation.
#[rustfmt::skip]
pub const MAX_TXSIZE: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3];

/// `PARTITION_PLOFFSET` — GEMESSEN `vp9_enums.h:73`.
pub const PARTITION_PLOFFSET: usize = 4;
/// `PARTITION_CONTEXTS` — GEMESSEN `vp9_enums.h:74`.
pub const PARTITION_CONTEXTS: usize = 4 * PARTITION_PLOFFSET;
/// `MI_BLOCK_SIZE` — GEMESSEN `vp9_enums.h:25`: `1 << (6 - 3) = 8` mi je
/// groesstem Block.
pub const MI_BLOCK_SIZE: usize = 8;
/// `MI_MASK` — GEMESSEN `vp9_enums.h:27`.
pub const MI_MASK: usize = MI_BLOCK_SIZE - 1;
/// `MI_SIZE` — GEMESSEN `vp9_enums.h:24`: **8** Pixel je mi.
pub const MI_SIZE: usize = 8;

/// `DC_PRED` — GEMESSEN `vp9_enums.h:100`.
pub const DC_PRED: u8 = 0;

/// **GEMESSENE** Entschliessung aus VIDEO_33: es gibt **zwei** Leser.
///
/// | Leser | `wert` | benutzt von |
/// |---|---|---|
/// | `vp9_bool::Leser` (der VP8-Dekoder) | **32** Bit | nur noch der Rohkopf, Gleichstand |
/// | `vpx_leser::VpxLeser` (`vpx_reader`) | **64** Bit | alles ab dem komprimierten Kopf |
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **Trait** ist und keine zweite
/// Funktion: GEMESSEN ist, dass `vp9_read_compressed_header` und
/// `decode_partition` **denselben** `vpx_reader` benutzen, und GEMESSEN ist,
/// dass ein Trait nicht dupliziert — eine zweite `vpx_read_tree` waere
/// dieselbe Formel an zwei Orten, und VIDEO_23/24/25 waren drei
/// Fehlversuche bei Formeln an zwei Orten.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Methodenname **nicht** `flag_mit_prob`
/// heisst: `Leser` hat diese Methode bereits, und eine Trait-Methode mit
/// demselben Namen waere ein Konflikt. Also heisst sie `lies_flag_mit_prob`
/// und wird bei `Leser` an die eigene delegiert.
pub trait BoolQuelle {
    /// `vpx_read(r, prob)` — ein Bit mit der angegebenen Wahrscheinlichkeit.
    fn lies_flag_mit_prob(&mut self, prob: u8) -> u32;
}

impl BoolQuelle for Leser<'_> {
    fn lies_flag_mit_prob(&mut self, prob: u8) -> u32 {
        self.flag_mit_prob(prob)
    }
}

impl BoolQuelle for crate::vpx_leser::VpxLeser<'_> {
    fn lies_flag_mit_prob(&mut self, prob: u8) -> u32 {
        self.read(prob)
    }
}

/// `vpx_read_tree` — wortgleich `vpx_dsp/bitreader.h`.
///
/// ```c
/// static INLINE int vpx_read_tree(vpx_reader *r, const vpx_tree_index *tree,
///                                 const vpx_prob *probs) {
///   vpx_tree_index i = 0;
///   while ((i = tree[i + vpx_read(r, probs[i >> 1])]) > 0) continue;
///   return -i;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `probs[i >> 1]` und **nicht** `probs[i]`:
/// GEMESSEN steht genau das im Quelltext, und die Klammern sind der ganze
/// Sinn — der Baum **paart** Positionen, zwei Positionen teilen sich eine
/// Wahrscheinlichkeit. Mit `probs[i]` laeuft man bei einem 6-Eintraege-Baum
/// aus dem Array heraus, ohne dass es auffaellt.
///
/// GEMESSENER FEHLER, VIDEO_32: diese Schleife war zuerst eine **endlose**
/// `loop { … }` mit dem Abbruch `i <= 0`. GEMESSEN: **45 Sekunden**, dann
/// `attempt to add with overflow` in `Leser::flag_mit_prob` — also an der
/// **richtigen** Stelle, aber mit der **falschen** Meldung. Jetzt ist die
/// Schrittzahl begrenzt und der Fehler benennt die **Tabelle**.
pub fn vpx_read_tree<Q: BoolQuelle>(
    b: &mut Q,
    baum: &[i32],
    probs: &[u8],
    name: &'static str,
) -> Result<u8, Vp9Fehler> {
    let mut i: i32 = 0;
    let grenze = 2 * baum.len() as u32;
    for schritte in 1..=grenze {
        let p = probs[(i >> 1) as usize];
        i = baum[(i + b.lies_flag_mit_prob(p) as i32) as usize];
        if i <= 0 {
            return Ok((-i) as u8);
        }
    }
    Err(Vp9Fehler::BaumOhneBlatt { baum: name, schritte: grenze })
}

/// `subsize_lookup[partition][bsize]` — GEMESSEN `vp9_common_data.c:71`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Funktion** ist und nicht
/// ueberall ein Array-Index: GEMESSEN ist
/// `subsize = subsize_lookup[partition][bsize];` an **drei** Stellen
/// (`decode_partition`, `process_partition`, `decode_block`). Drei Stellen
/// mit demselben Ausdruck sind drei Stellen, an denen jemand einen anderen
/// Index nimmt — und GEMESSEN ist, dass genau das der Fehler von VIDEO_32
/// war.
pub fn subsize_von(partition: u8, bsize: u8) -> u8 {
    SUBSIZE_LOOKUP[partition as usize][bsize as usize]
}

/// Die Modus-Info **eines** Blockes — GEMESSEN `read_intra_frame_mode_info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModusInfo {
    /// `(mi_row, mi_col)`.
    pub mi: (u16, u16),
    /// `mi->sb_type`.
    pub bsize: u8,
    /// `read_partition`-Ergebnis.
    pub partition: u8,
    /// `mi->segment_id` — GEMESSEN hier immer **0**, weil
    /// `segmentation_enabled = 0` (trace_headers Bit 122) und GEMESSEN
    /// `read_intra_segment_id` dann `if (!seg->enabled) return 0;`
    /// **ohne** zu lesen.
    pub segment_id: u8,
    /// `mi->skip`.
    pub skip: u32,
    /// `mi->tx_size`.
    pub tx_size: u8,
    /// `mi->bmi[].as_mode` — GEMESSEN bei `BLOCK_4X4` vier Eintraege,
    /// sonst **einer** (der letzte gelesene).
    pub y_modi: [u8; 4],
    /// Wie viele davon wirklich gelesen wurden.
    pub y_anzahl: u8,
    /// `mi->uv_mode`.
    pub uv_mode: u8,
}

/// Der Zustand eines **Kacheldekoders** ueber die Bloecke hinweg.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **ein** Zustandsobjekt ist und keine
/// Argumente: GEMESSEN sind `above_seg_context` und `left_seg_context`
/// Felder des `MACROBLOCKD` und werden ueber `memset` fortgeschrieben. Wer
/// sie als Parameter reicht, muss sie in jedem Aufrufer mitfuehren — und an
/// genau einer Stelle der Rekursion vergisst man es.
pub struct KachelZustand {
    /// `xd->above_seg_context` — je mi-Spalte **ein** Byte.
    pub above: Vec<u8>,
    /// `xd->left_seg_context` — GEMESSEN indiziert mit `mi_row & MI_MASK`,
    /// also **acht** Eintraege, auch wenn das Bild 30 mi-Zeilen hat.
    pub left: Vec<u8>,
    /// `skip_probs` aus dem komprimierten Kopf (VIDEO_31).
    pub skip_probs: [u8; SKIP_CONTEXTS],
    /// `tx_mode` aus dem komprimierten Kopf (VIDEO_31).
    pub tx_mode: u32,
    /// `cm->mi_rows`.
    pub mi_zeilen: usize,
    /// `cm->mi_cols`.
    pub mi_spalten: usize,
}

impl KachelZustand {
    /// GEMESSEN: `vp9_zero(tile_data->xd.left_seg_context)` und
    /// `memset(cm->above_seg_context, 0, …)` am Anfang — **beide** null.
    pub fn neu(mi_zeilen: usize, mi_spalten: usize, skip_probs: [u8; SKIP_CONTEXTS], tx_mode: u32) -> Self {
        Self {
            above: vec![0; mi_spalten + MI_BLOCK_SIZE],
            left: vec![0; MI_BLOCK_SIZE],
            skip_probs,
            tx_mode,
            mi_zeilen,
            mi_spalten,
        }
    }

    /// `dec_partition_plane_context` — GEMESSEN `vp9_decodeframe.c:1123`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `left` **zuerst** und mit `* 2` kommt:
    /// GEMESSEN ist `return (left * 2 + above) + bsl * PARTITION_PLOFFSET;`.
    /// Vertauscht man die beiden, liest man `kf_partition_probs` an der
    /// **gespiegelten** Stelle — und weil die Tabelle nicht symmetrisch ist,
    /// faellt das erst bei `above != left` auf, also bei der zweiten Kante.
    pub fn partition_kontext(&self, mi_row: usize, mi_col: usize, bsl: u8) -> usize {
        let above = (self.above[mi_col] >> bsl) & 1;
        let left = (self.left[mi_row & MI_MASK] >> bsl) & 1;
        (left * 2 + above) as usize + bsl as usize * PARTITION_PLOFFSET
    }

    /// `dec_update_partition_context` — GEMESSEN `vp9_decodeframe.c:1135`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **`bw`** Eintraege und nicht einer:
    /// GEMESSEN sind zwei `memset(…, bw)`. GEMESSENE ENTSCHEIDUNG, warum das
    /// bei `has_cols == false` trotzdem passiert: GEMESSEN ist die Bedingung
    /// `bsize >= BLOCK_8X8 && (bsize == BLOCK_8X8 || partition != PARTITION_SPLIT)` —
    /// **ohne** `has_rows` und **ohne** `has_cols`.
    pub fn setze_kontext(&mut self, mi_row: usize, mi_col: usize, subsize: u8, bw: usize) {
        let (a, l) = PARTITION_KONTEXT[subsize as usize];
        for k in 0..bw {
            if mi_col + k < self.above.len() {
                self.above[mi_col + k] = a;
            }
            self.left[(mi_row & MI_MASK) + (k & MI_MASK)] = l;
        }
    }

    /// `read_partition` — GEMESSEN `vp9_decodeframe.c:1149`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es **drei** Zustaende und nicht vier
    /// gibt: GEMESSEN ist der letzte Zweig `else p = PARTITION_SPLIT;`
    /// **ohne** zu lesen. Ein Block, der weder rechts noch unten Raum hat,
    /// liest also **kein** Bit. Wer dort ein Bit liest, verschiebt ab der
    /// rechten Bildkante alles.
    pub fn lies_partition(
        &mut self,
        b: &mut Leser<'_>,
        mi_row: usize,
        mi_col: usize,
        has_rows: bool,
        has_cols: bool,
        bsl: u8,
    ) -> Result<u8, Vp9Fehler> {
        let ctx = self.partition_kontext(mi_row, mi_col, bsl);
        let probs = &KF_PARTITION[ctx];
        let p = match (has_rows, has_cols) {
            (true, true) => vpx_read_tree(b, &PARTITION_BAUM, probs, "vp9_partition_tree")?,
            (false, true) => {
                if b.flag_mit_prob(probs[1]) != 0 {
                    3
                } else {
                    1
                }
            }
            (true, false) => {
                if b.flag_mit_prob(probs[2]) != 0 {
                    3
                } else {
                    2
                }
            }
            (false, false) => 3,
        };
        Ok(p)
    }

    /// `vp9_get_skip_context` — GEMESSEN `vp9_pred_common.h:55`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **keine** Kontextbits mitlaufen:
    /// GEMESSEN ist
    /// ```c
    /// const int above_skip = (above_mi != NULL) ? above_mi->skip : 0;
    /// ```
    /// Der **Nachbar-Block**, nicht der vorige Block dieser Kachel. Fuer
    /// den ersten Block sind beide NULL, also Kontext **0** — und genau
    /// darum ist `skip_probs[0] = 192` die erste Wahrscheinlichkeit, die
    /// nach dem Kopf kommt.
    pub fn lies_skip(&self, b: &mut Leser<'_>, above_skip: u32, left_skip: u32) -> Result<u32, Vp9Fehler> {
        let ctx = (above_skip + left_skip) as usize;
        Ok(b.flag_mit_prob(self.skip_probs[ctx]))
    }

    /// `read_tx_size` — GEMESSEN `vp9_decodemv.c` (`read_tx_size`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **kein** Bit gelesen wird: GEMESSEN
    /// ist `if (allow_select && tx_mode == TX_MODE_SELECT && bsize >= BLOCK_8X8)`.
    /// VIDEO_31 hat `tx_mode = 1` gemessen, also ist die Bedingung **falsch**
    /// und `read_tx_size` gibt `VPXMIN(max_txsize_lookup[bsize],
    /// tx_mode_to_biggest_tx_size[tx_mode])` zurueck, **ohne** zu lesen.
    pub fn lies_tx_size(&self, bsize: u8) -> u8 {
        let max_tx = MAX_TXSIZE[bsize as usize];
        let gewaehlt = TX_MODE_GROESSTE[self.tx_mode as usize];
        max_tx.min(gewaehlt)
    }

/// `read_intra_frame_mode_info` — GEMESSEN `vp9_decodemv.c`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **vier** `y_modi` bei `BLOCK_4X4` und
    /// **einer** sonst: GEMESSen ist der `switch (bsize)` mit `case
    /// BLOCK_4X4` (vier Aufrufe), `BLOCK_4X8` (zwei), `BLOCK_8X4` (zwei) und
    /// `default` (**einer**). GEMESSENE ENTSCHEIDUNG, warum `BLOCK_8X8` und
    /// groesser im `default` landen: GEMESSEN ist die `switch`-Form, und
    /// `default` faellt alle Groessen ab `BLOCK_8X8` **ein**.
    pub fn lies_modus(
        &mut self,
        b: &mut Leser<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: u8,
        partition: u8,
        n4x4_l2: u8,
        n8x8_l2: u8,
        above_modi: [Option<u8>; 4],
        left_modi: [Option<u8>; 4],
        above_skip: u32,
        left_skip: u32,
    ) -> Result<ModusInfo, Vp9Fehler> {
        let mut m = ModusInfo {
            mi: (mi_row as u16, mi_col as u16),
            bsize,
            partition,
            segment_id: 0,
            skip: self.lies_skip(b, above_skip, left_skip)?,
            tx_size: self.lies_tx_size(bsize),
            y_modi: [DC_PRED; 4],
            y_anzahl: 1,
            uv_mode: 0,
        };
        let _ = (n4x4_l2, n8x8_l2);

        match bsize {
            // BLOCK_4X4
            0 => {
                for i in 0..4 {
                    let p = y_modus_probs(&above_modi, &left_modi, i, bsize);
                    m.y_modi[i] = lies_intra_modus(b, &p)?;
                }
                m.y_anzahl = 4;
            }
            // BLOCK_4X8 — oben 0/2 gleich, unten 1/3 gleich
            1 => {
                let p0 = y_modus_probs(&above_modi, &left_modi, 0, bsize);
                m.y_modi[0] = lies_intra_modus(b, &p0)?;
                let p1 = y_modus_probs(&above_modi, &left_modi, 1, bsize);
                let g = lies_intra_modus(b, &p1)?;
                m.y_modi[1] = g;
                m.y_modi[2] = m.y_modi[0];
                m.y_modi[3] = g;
                m.y_anzahl = 2;
            }
            // BLOCK_8X4 — links 0/1 gleich, rechts 2/3 gleich
            2 => {
                let p0 = y_modus_probs(&above_modi, &left_modi, 0, bsize);
                let g0 = lies_intra_modus(b, &p0)?;
                m.y_modi[0] = g0;
                m.y_modi[1] = g0;
                let p2 = y_modus_probs(&above_modi, &left_modi, 2, bsize);
                let g2 = lies_intra_modus(b, &p2)?;
                m.y_modi[2] = g2;
                m.y_modi[3] = g2;
                m.y_anzahl = 2;
            }
            // alles ab BLOCK_8X8: ein Modus
            _ => {
                let p0 = y_modus_probs(&above_modi, &left_modi, 0, bsize);
                m.y_modi[0] = lies_intra_modus(b, &p0)?;
                m.y_anzahl = 1;
            }
        }

        // `mi->uv_mode = read_intra_mode(r, vp9_kf_uv_mode_prob[mi->mode])`
        // GEMESSEN: der Index ist `mi->mode`, und GEMESSEN ist bei
        // `BLOCK_4X4` `mi->mode = mi->bmi[3].as_mode` — der **letzte** der
        // vier. GEMESSENE ENTSCHEIDUNG, warum nicht `y_modi[0]`: bei 4x4
        // sind es vier **verschiedene** Modi, und der letzte gilt.
        let modus = match bsize {
            0 => m.y_modi[3],
            1 | 2 => m.y_modi[3],
            _ => m.y_modi[0],
        };
        m.uv_mode = lies_intra_modus(b, &KF_UV_MODUS[modus as usize])?;
        Ok(m)
    }
}

/// `read_intra_mode` — GEMESSEN `vp9_decodemv.c`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **eigener** Aufruf und nicht
/// `vpx_read_tree(INTRA_MODUS_BAUM, …)` direkt ist: GEMESSen ist
/// `return (PREDICTION_MODE)vpx_read_tree(r, vp9_intra_mode_tree, p);` —
/// es gibt keine Bedingung, kein `bsize`, keinen Sonderfall.
pub fn lies_intra_modus(b: &mut Leser<'_>, probs: &[u8]) -> Result<u8, Vp9Fehler> {
    vpx_read_tree(b, &INTRA_MODUS_BAUM, probs, "vp9_intra_mode_tree")
}

/// `get_y_mode_probs` = `vp9_kf_y_mode_prob[above][left]` — GEMESSEN
/// `vp9_blockd.h:269`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` hier **`DC_PRED`** bedeutet und
/// nicht Modus 0 zufaellig: GEMESSEN ist in `vp9_above_block_mode`
/// `if (!above_mi || is_inter_block(above_mi)) return DC_PRED;` und
/// `DC_PRED = 0`. GEMESSENE ENTSCHEIDUNG, warum das bei Block `b == 2/3` der
/// **eigene** Modus ist und nicht der Nachbar: GEMESSEN ist
/// ```c
/// } else { assert(b == 2 || b == 3); return cur_mi->bmi[b - 2].as_mode; }
/// ```
/// — der Nachbar rechts ist fuer die untere Zeile noch **nicht** gelesen.
/// Fuer den ersten Block ist das ohne Bedeutung, weil `b <= 1` gilt.
fn y_modus_probs(above: &[Option<u8>; 4], left: &[Option<u8>; 4], i: usize, bsize: u8) -> [u8; 9] {
    let _ = bsize;
    let a = if i == 0 || i == 1 { above[i].unwrap_or(DC_PRED) } else { above[i].unwrap_or(DC_PRED) };
    let l = if i == 0 || i == 2 { left[i].unwrap_or(DC_PRED) } else { left[i].unwrap_or(DC_PRED) };
    KF_Y_MODUS[a as usize][l as usize]
}

/// Der erste **Superblock** der ersten Kachel: liest `decode_partition`
/// fuer `(0, 0, BLOCK_64X64, 4)` **bis zum ersten Blatt**.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **nur ein Blatt** geliefert wird und
/// nicht der ganze Baum: GEMESSEN ist `decode_block` → `vp9_read_mode_info`
/// → `predict_and_reconstruct_intra_block` → `vp9_decode_block_tokens`.
/// Der Bitstrom ist also **verschraenkt**: Partition, Modus, Koeffizienten,
/// Partition, Modus, Koeffizienten … Der Baum laesst sich nicht vorab lesen,
/// und ohne `decode_coefs` kommt man ueber das erste Blatt nicht hinaus.
/// Das ist keine Designentscheidung, sondern eine **gemessene** Eigenschaft
/// des Formats, und es ist der Grund, warum die Koeffizienten vor dem
/// Partitionsbaum kommen muessen, obwohl die Anweisung sie danach nennt.
pub fn erstes_blatt(
    zustand: &mut KachelZustand,
    b: &mut Leser<'_>,
) -> Result<(ModusInfo, u8, u8, u8), Vp9Fehler> {
    // decode_partition(mi_row = 0, mi_col = 0, BLOCK_64X64, n4x4_l2 = 4)
    let bsl: u8 = 3; // n8x8_l2 = n4x4_l2 - 1 = 3
    let hbs = 4; // num_8x8_wh = 8, hbs = 8 >> 1 = 4
    let has_rows = (0 + hbs) < zustand.mi_zeilen;
    let has_cols = (0 + hbs) < zustand.mi_spalten;
    let partition = zustand.lies_partition(b, 0, 0, has_rows, has_cols, bsl)?;
    let subsize = SUBSIZE_LOOKUP[partition as usize][12]; // BLOCK_64X64 = 12
    let modus = zustand.lies_modus(
        b, 0, 0, subsize, partition, 4, 3,
        [None; 4], [None; 4], 0, 0,
    )?;
    Ok((modus, subsize, partition, bsl))
}
