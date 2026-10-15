//! Die **grosswertigen** Huffman-Tabellen von MP3 Layer III — die ersten fünf.
//!
//! # WARUM ÜBERHAupt
//!
//! Der count1-Bereich setzt je Komponente den Wert **1**. Das ist **eine**
//! Stufe, und eine Stufe ist keine Requantisierung. Für Werte 0 bis 14 braucht
//! es die Tabellen, die je **zwei** Spektrallinien als Paar kodieren — und
//! dort liegen auch die Stufen, an denen die Kurve `is^(4/3)` überhaupt sichtbar
//! wird.
//!
//! `requantisiert()` in [`crate::granulat`] rechnet diese Kurve, aber bis hierher
//! konnte **niemand** sie messen: count1 liefert nur `is = 1`.
//!
//! # DIE FÜNF TABELLEN, UND WAS SIE KÖNNEN
//!
//! | `table_select` | VLC | Symbole | Stufen | Einträge |
//! |---|---|---|---|---|
//! | 1 | 1 | x, y ∈ 0..1 | 0, 1 | 4 |
//! | 2 | 2 | x, y ∈ 0..2 | 0, 1, 2 | 9 |
//! | 3 | 3 | x, y ∈ 0..2 | 0, 1, 2 | 9 |
//! | 5 | 4 | x, y ∈ 0..3 | 0..3 | 16 |
//! | **6** | **5** | x, y ∈ 0..3 | **0..3** | 16 |
//!
//! `table_select = 6` ist die **größte** der hier gebauten Tabellen und damit die
//! interessanteste: sie hat alle 16 Kombinationen aus 0, 1, 2, 3.
//!
//! # DIE ZAHLEN SIND AUS DER QUELLE, DIE CODES SIND KANONISCH
//!
//! Aus `libavcodec/mpegaudiodec_common.c`, `mpegaudiodec_common_init_static()`:
//!
//! ```c
//! offset = 0;
//! for (int i = 0; i < 15;) {
//!     uint16_t tmp_symbols[256];
//!     int nb_codes_minus_one = mpa_huff_sizes_minus_one[i];
//!     for (int j = 0; j <= nb_codes_minus_one; j++) {
//!         uint8_t high = huff_sym[j] & 0xF0, low = huff_sym[j] & 0xF;
//!         tmp_symbols[j] = high << 1 | ((high && low) << 4) | low;
//!     }
//!     ff_vlc_init_from_lengths(&ff_huff_vlc[++i], 7, j, huff_lens, 1,
//!                              tmp_symbols, 2, 2, 0, ...);
//!     huff_lens += j;  huff_sym += j;
//! }
//! ```
//!
//! Zwei Daraus, und beide sind nicht selbstverständlich:
//!
//! 1. **Es gibt keine Codewörter in der Quelle**, nur Längen. ffmpeg baut die
//!    Codes **kanonisch** aus den Längen. Diese Crate tut dasselbe — nach der
//!    üblichen Regel: erst nach Länge aufsteigend, dann in der Reihenfolge des
//!    Symbolfelds. GEMESSEN 02.10.2026, dass das die richtige Regel ist, steht
//!    in `tests/k2_mp3_bigwerte.rs`.
//! 2. **Das Symbol ist gepackt.** Aus `high << 1 | ((high && low) << 4) | low`
//!    folgt für ein Paar `(x, y)`:
//!
//!    ```text
//!    symbol = x * 32 + (x != 0 && y != 0 ? 16 : 0) + y
//!    ```
//!
//!    Und im Leser:
//!    ```c
//!    if (y & 16) { x = y >> 5;  y = y & 0x0f; }   /* beide da */
//!    else       { x = y >> 5;  y = y & 0x0f; x += y; }
//!    ```
//!    Das Bit 4 ist also ein **Flag**, kein Wert — und bei `x = 0` oder
//!    `y = 0` steht es nicht. GEMESSEN an `0x11` (x = 1, y = 1):
//!    `1*32 + 16 + 1 = 49 = 0x31`, und `0x31 & 16 = 16` ✓. An `0x01`
//!    (x = 0, y = 1): `0 + 0 + 1 = 1`, und `1 & 16 = 0` ✓.
//!
//! # WAS NOCH FEHLT
//!
//! Die Tabellen 7 bis 15 (bis 256 Einträge) und die `linbits`-Tabellen 16 bis
//! 23. `linbits` ist die Zahl **zusätzlicher** Bit für Werte ab 15:
//!
//! ```c
//! x += get_bitsz(&s->gb, linbits);
//! v = l3_unscale(x, exponent);
//! ```
//!
//! Für die Abnahme der Stufenkurve reichen die fünf kleinen Tabellen.

use crate::bits::Bits;

/// `ff_mpa_huff_data[k]` — welches `ff_huff_vlc` und wie viele `linbits`.
///
/// GEMESSEN 02.10.2026: `table_select` 0, 4 und 14 sind **nicht belegt** (VLC 0),
/// und die Tabelle ist kein einfacher Index. Aus der Quelle:
///
/// ```c
/// const uint8_t ff_mpa_huff_data[32][2] = {
/// { 0, 0 }, { 1, 0 }, { 2, 0 }, { 3, 0 }, { 0, 0 }, { 4, 0 },
/// { 5, 0 }, { 6, 0 }, { 7, 0 }, { 8, 0 }, { 9, 0 }, {10, 0 },
/// {11, 0 }, {12, 0 }, { 0, 0 }, {13, 0 }, {14, 1 }, {14, 2 },
/// {14, 3 }, {14, 4 }, {14, 6 }, {14, 8 }, {14,10 }, {14,13 },
/// {15, 4 }, {15, 5 }, {15, 6 }, {15, 7 }, {15, 8 }, {15, 9 },
/// {15,11 }, {15,13 },
/// };
/// ```
pub const DATEN: [(u8, u8); 32] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (0, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 0),
    (12, 0),
    (0, 0),
    (13, 0),
    (14, 1),
    (14, 2),
    (14, 3),
    (14, 4),
    (14, 6),
    (14, 8),
    (14, 10),
    (14, 13),
    (15, 4),
    (15, 5),
    (15, 6),
    (15, 7),
    (15, 8),
    (15, 9),
    (15, 11),
    (15, 13),
];

/// Die acht **kleinen** Tabellen: Längen und gepackte Symbole `(x << 4) | y`.
///
/// Reihenfolge wie in `mpa_hufflens` / `mpa_huffsymbols`, also
/// `TABELLEN[0]` ist die Gruppe für `ff_huff_vlc[1]`.
///
/// GEMESSEN 02.10.2026, und die Prüfung, mit der die Daten hierher kamen, ist
/// derselbe Test, der sie jetzt festhält: der Akkumulator muss bei genau `2^32`
/// enden, und kein Codewort darf ein Präfix eines anderen sein. Eine
/// fehlerhafte Abschrift von 36 Zahlen fällt da **immer** auf — GEMESSEN ist
/// genau das passiert, ich hatte in Tabelle 8 **eine Länge zu viel** und es wurde
/// sofort sichtbar.
///
/// # Warum `Option`
///
/// Der Index ist `ff_huff_vlc[i] - 1`, und **die Gruppen 11, 12 und 13 fehlen**
/// in dieser Crate („table 13", „table 15", „table 16" — je 256 Einträge).
/// Ohne `Option` würde `gruppe(table_select = 15)` auf die Daten von
/// „table 24" zeigen, und der Enkoder schriebe gültige Bytes für die falsche
/// Tabelle. **Ein fehlender Eintrag muss fehlen**, nicht der nächste sein.
pub const TABELLEN: [Option<(&[u8], &[u8])>; 15] = [
    // Gruppe 1 — „Huffman table 1", [3, 3, 2, 1] Eintraege, x,y in 0..1
    Some((&[3, 3, 2, 1], &[0x11, 0x01, 0x10, 0x00])),
    // Gruppe 2 — „table 2", [6, 6, 5, 5, 5, 3, 3, 3, 1] Eintraege, x,y in 0..2
    Some((
        &[6, 6, 5, 5, 5, 3, 3, 3, 1],
        &[0x22, 0x02, 0x12, 0x21, 0x20, 0x11, 0x01, 0x10, 0x00],
    )),
    // Gruppe 3 — „table 3", [6, 6, 5, 5, 5, 3, 2, 2, 2] Eintraege, x,y in 0..2
    Some((
        &[6, 6, 5, 5, 5, 3, 2, 2, 2],
        &[0x22, 0x02, 0x12, 0x21, 0x20, 0x10, 0x11, 0x01, 0x00],
    )),
    // Gruppe 4 — „table 5", [8, 8, 7, 6, 7, 7, 7, 7, 6, 6, 6, 6, 3, 3, 3, 1] Eintraege, x,y in 0..3
    Some((
        &[8, 8, 7, 6, 7, 7, 7, 7, 6, 6, 6, 6, 3, 3, 3, 1],
        &[
            0x33, 0x23, 0x32, 0x31, 0x13, 0x03, 0x30, 0x22, 0x12, 0x21, 0x02, 0x20, 0x11, 0x01,
            0x10, 0x00,
        ],
    )),
    // Gruppe 5 — „table 6", [7, 7, 6, 6, 6, 5, 5, 5, 5, 4, 4, 4, 3, 2, 3, 3] Eintraege, x,y in 0..3
    Some((
        &[7, 7, 6, 6, 6, 5, 5, 5, 5, 4, 4, 4, 3, 2, 3, 3],
        &[
            0x33, 0x03, 0x23, 0x32, 0x30, 0x13, 0x31, 0x22, 0x02, 0x12, 0x21, 0x20, 0x01, 0x11,
            0x10, 0x00,
        ],
    )),
    // Gruppe 6 — „table 7", [10, 10, 10, 10, 9, 9, 9, 9, 8, 8, 9, 9, 8, 9, 9, 8, 8, 7, 7, 7, 8, 8, 8, 8, 7, 7, 7, 7, 6, 5, 6, 6, 4, 3, 3, 1] Eintraege, x,y in 0..5
    Some((
        &[
            10, 10, 10, 10, 9, 9, 9, 9, 8, 8, 9, 9, 8, 9, 9, 8, 8, 7, 7, 7, 8, 8, 8, 8, 7, 7, 7, 7,
            6, 5, 6, 6, 4, 3, 3, 1,
        ],
        &[
            0x55, 0x45, 0x54, 0x53, 0x35, 0x44, 0x25, 0x52, 0x15, 0x51, 0x05, 0x34, 0x50, 0x43,
            0x33, 0x24, 0x42, 0x14, 0x41, 0x40, 0x04, 0x23, 0x32, 0x03, 0x13, 0x31, 0x30, 0x22,
            0x12, 0x21, 0x02, 0x20, 0x11, 0x01, 0x10, 0x00,
        ],
    )),
    // Gruppe 7 — „table 8", [11, 11, 10, 9, 10, 10, 9, 9, 9, 8, 8, 9, 9, 9, 9, 8, 8, 8, 7, 8, 8, 8, 8, 8, 8, 8, 8, 6, 6, 6, 4, 4, 2, 3, 3, 2] Eintraege, x,y in 0..5
    Some((
        &[
            11, 11, 10, 9, 10, 10, 9, 9, 9, 8, 8, 9, 9, 9, 9, 8, 8, 8, 7, 8, 8, 8, 8, 8, 8, 8, 8,
            6, 6, 6, 4, 4, 2, 3, 3, 2,
        ],
        &[
            0x55, 0x54, 0x45, 0x53, 0x35, 0x44, 0x25, 0x52, 0x05, 0x15, 0x51, 0x34, 0x43, 0x50,
            0x33, 0x24, 0x42, 0x14, 0x41, 0x04, 0x40, 0x23, 0x32, 0x13, 0x31, 0x03, 0x30, 0x22,
            0x02, 0x20, 0x12, 0x21, 0x11, 0x01, 0x10, 0x00,
        ],
    )),
    // Gruppe 8 — „table 9", [9, 9, 8, 8, 9, 9, 8, 8, 8, 8, 7, 7, 7, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 5, 5, 6, 6, 5, 5, 4, 4, 4, 3, 3, 3, 3] Eintraege, x,y in 0..5
    Some((
        &[
            9, 9, 8, 8, 9, 9, 8, 8, 8, 8, 7, 7, 7, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 5, 5, 6, 6, 5, 5,
            4, 4, 4, 3, 3, 3, 3,
        ],
        &[
            0x55, 0x45, 0x35, 0x53, 0x54, 0x05, 0x44, 0x25, 0x52, 0x15, 0x51, 0x34, 0x43, 0x50,
            0x04, 0x24, 0x42, 0x33, 0x40, 0x14, 0x41, 0x23, 0x32, 0x13, 0x31, 0x03, 0x30, 0x22,
            0x02, 0x12, 0x21, 0x20, 0x11, 0x01, 0x10, 0x00,
        ],
    )),
    // Gruppe 9 — „table 10", [11, 11, 11, 11, 11, 11, 10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 9, 9, 10, 10, 9, 9, 10, 10, 9, 10, 10, 8, 8, 9, 9, 10, 10, 9, 9, 10, 10, 8, 8, 8, 9, 9, 9, 9, 9, 9, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 4, 3, 3, 1] Eintraege, x,y in 0..7
    Some((
        &[
            11, 11, 11, 11, 11, 11, 10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 9, 9, 10, 10, 9, 9, 10,
            10, 9, 10, 10, 8, 8, 9, 9, 10, 10, 9, 9, 10, 10, 8, 8, 8, 9, 9, 9, 9, 9, 9, 8, 8, 8, 8,
            8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 4, 3, 3, 1,
        ],
        &[
            0x77, 0x67, 0x76, 0x57, 0x75, 0x66, 0x47, 0x74, 0x56, 0x65, 0x37, 0x73, 0x46, 0x55,
            0x54, 0x63, 0x27, 0x72, 0x64, 0x07, 0x70, 0x62, 0x45, 0x35, 0x06, 0x53, 0x44, 0x17,
            0x71, 0x36, 0x26, 0x25, 0x52, 0x15, 0x51, 0x34, 0x43, 0x16, 0x61, 0x60, 0x05, 0x50,
            0x24, 0x42, 0x33, 0x04, 0x14, 0x41, 0x40, 0x23, 0x32, 0x03, 0x13, 0x31, 0x30, 0x22,
            0x12, 0x21, 0x02, 0x20, 0x11, 0x01, 0x10, 0x00,
        ],
    )),
    // Gruppe 10 — „table 11", [10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 10, 9, 9, 9, 10, 10, 10, 10, 8, 8, 9, 9, 7, 8, 8, 8, 8, 8, 9, 9, 9, 9, 8, 7, 8, 8, 7, 7, 8, 8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 7, 7, 6, 6, 7, 7, 6, 5, 4, 5, 5, 3, 3, 3, 2] Eintraege, x,y in 0..7
    Some((
        &[
            10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 10, 9, 9, 9, 10, 10, 10, 10, 8, 8, 9, 9, 7, 8,
            8, 8, 8, 8, 9, 9, 9, 9, 8, 7, 8, 8, 7, 7, 8, 8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 7, 7, 6, 6,
            7, 7, 6, 5, 4, 5, 5, 3, 3, 3, 2,
        ],
        &[
            0x77, 0x67, 0x76, 0x75, 0x66, 0x47, 0x74, 0x57, 0x55, 0x56, 0x65, 0x37, 0x73, 0x46,
            0x45, 0x54, 0x35, 0x53, 0x27, 0x72, 0x64, 0x07, 0x71, 0x17, 0x70, 0x36, 0x63, 0x60,
            0x44, 0x25, 0x52, 0x05, 0x15, 0x62, 0x26, 0x06, 0x16, 0x61, 0x51, 0x34, 0x50, 0x43,
            0x33, 0x24, 0x42, 0x14, 0x41, 0x04, 0x40, 0x23, 0x32, 0x13, 0x31, 0x03, 0x30, 0x22,
            0x21, 0x12, 0x02, 0x20, 0x11, 0x01, 0x10, 0x00,
        ],
    )),
    // Gruppe 11 — „table 12", [10, 10, 9, 9, 9, 9, 9, 9, 9, 8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 9, 9, 8, 8, 8, 8, 8, 9, 9, 7, 7, 7, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 8, 8, 7, 7, 7, 6, 6, 6, 6, 7, 7, 6, 5, 5, 5, 4, 4, 5, 5, 4, 3, 3, 3] Eintraege, x,y in 0..7
    Some((
        &[
            10, 10, 9, 9, 9, 9, 9, 9, 9, 8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 9, 9, 8, 8, 8, 8, 8, 9, 9,
            7, 7, 7, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 8, 8, 7, 7, 7, 6, 6, 6, 6, 7, 7, 6, 5, 5, 5, 4,
            4, 5, 5, 4, 3, 3, 3,
        ],
        &[
            0x77, 0x67, 0x76, 0x57, 0x75, 0x66, 0x47, 0x74, 0x65, 0x56, 0x37, 0x73, 0x55, 0x27,
            0x72, 0x46, 0x64, 0x17, 0x71, 0x07, 0x70, 0x36, 0x63, 0x45, 0x54, 0x44, 0x06, 0x05,
            0x26, 0x62, 0x61, 0x16, 0x60, 0x35, 0x53, 0x25, 0x52, 0x15, 0x51, 0x34, 0x43, 0x50,
            0x04, 0x24, 0x42, 0x14, 0x33, 0x41, 0x23, 0x32, 0x40, 0x03, 0x30, 0x13, 0x31, 0x22,
            0x12, 0x21, 0x02, 0x20, 0x00, 0x11, 0x01, 0x10,
        ],
    )),
    // Die Gruppen 12 bis 14 („table 13", „table 15", „table 16")
    // fehlen hier noch — je 256 Eintraege. Sie stehen als `None`, und
    // `tabelle()` gibt deshalb `None` statt der Daten der Nachbartabelle.
    None,
    None,
    None,
    // Gruppe 14 — „table 24", [8, 8, 8, 8, 8, 8, 8, 8, 7, 8, 8, 7, 7, 8, 8, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 9, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 4, 11, 11, 11, 11, 12, 12, 11, 10, 11, 11, 10, 10, 10, 10, 11, 11, 10, 10, 10, 10, 11, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 11, 11, 10, 9, 10, 10, 10, 10, 11, 11, 10, 9, 9, 10, 10, 9, 10, 10, 10, 10, 9, 9, 10, 10, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 10, 10, 9, 9, 9, 10, 10, 8, 9, 9, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 9, 9, 7, 8, 8, 7, 7, 7, 7, 7, 8, 8, 7, 7, 6, 6, 7, 7, 6, 5, 5, 6, 6, 4, 4, 4, 4] Eintraege, x,y in 0..15, **linbits = 4**
    //
    // GEMESSEN 02.10.2026: die erste Tabelle dieser Crate mit
    // `linbits > 0`. Damit ist der Weg ab Stufe 15 benutzbar, und mit
    // `linbits = 4` sind die Stufen **0 bis 30** erreichbar. Und `(15,15)` hat
    // das **kuerzeste** Codewort der ganzen Tabelle: `0011` mit 4 Bit — die
    // Fluchteintraege liegen am Anfang.
    Some((
        &[
            8, 8, 8, 8, 8, 8, 8, 8, 7, 8, 8, 7, 7, 8, 8, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8,
            9, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
            11, 11, 11, 11, 11, 11, 11, 4, 11, 11, 11, 11, 12, 12, 11, 10, 11, 11, 10, 10, 10, 10,
            11, 11, 10, 10, 10, 10, 11, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 10, 11, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 11, 11, 10, 11,
            11, 10, 9, 10, 10, 10, 10, 11, 11, 10, 9, 9, 10, 10, 9, 10, 10, 10, 10, 9, 9, 10, 10,
            9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
            9, 9, 9, 9, 9, 9, 9, 10, 10, 9, 9, 9, 10, 10, 8, 9, 9, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
            8, 8, 9, 9, 8, 8, 8, 8, 8, 8, 9, 9, 7, 8, 8, 7, 7, 7, 7, 7, 8, 8, 7, 7, 6, 6, 7, 7, 6,
            5, 5, 6, 6, 4, 4, 4, 4,
        ],
        &[
            0xef, 0xfe, 0xdf, 0xfd, 0xcf, 0xfc, 0xbf, 0xfb, 0xfa, 0xaf, 0x9f, 0xf9, 0xf8, 0x8f,
            0x7f, 0xf7, 0x6f, 0xf6, 0x5f, 0xf5, 0x4f, 0xf4, 0x3f, 0xf3, 0x2f, 0xf2, 0xf1, 0x1f,
            0xf0, 0x0f, 0xee, 0xde, 0xed, 0xce, 0xec, 0xdd, 0xbe, 0xeb, 0xcd, 0xdc, 0xae, 0xea,
            0xbd, 0xdb, 0xcc, 0x9e, 0xe9, 0xad, 0xda, 0xbc, 0xcb, 0x8e, 0xe8, 0x9d, 0xd9, 0x7e,
            0xe7, 0xac, 0xff, 0xca, 0xbb, 0x8d, 0xd8, 0x0e, 0xe0, 0x0d, 0xe6, 0x6e, 0x9c, 0xc9,
            0x5e, 0xba, 0xe5, 0xab, 0x7d, 0xd7, 0xe4, 0x8c, 0xc8, 0x4e, 0x2e, 0x3e, 0x6d, 0xd6,
            0xe3, 0x9b, 0xb9, 0xaa, 0xe2, 0x1e, 0xe1, 0x5d, 0xd5, 0x7c, 0xc7, 0x4d, 0x8b, 0xb8,
            0xd4, 0x9a, 0xa9, 0x6c, 0xc6, 0x3d, 0xd3, 0x2d, 0xd2, 0x1d, 0x7b, 0xb7, 0xd1, 0x5c,
            0xc5, 0x8a, 0xa8, 0x99, 0x4c, 0xc4, 0x6b, 0xb6, 0xd0, 0x0c, 0x3c, 0xc3, 0x7a, 0xa7,
            0x2c, 0xc2, 0x5b, 0xb5, 0x1c, 0x89, 0x98, 0xc1, 0x4b, 0xc0, 0x0b, 0x3b, 0xb0, 0x0a,
            0x1a, 0xb4, 0x6a, 0xa6, 0x79, 0x97, 0xa0, 0x09, 0x90, 0xb3, 0x88, 0x2b, 0x5a, 0xb2,
            0xa5, 0x1b, 0xb1, 0x69, 0x96, 0xa4, 0x4a, 0x78, 0x87, 0x3a, 0xa3, 0x59, 0x95, 0x2a,
            0xa2, 0xa1, 0x68, 0x86, 0x77, 0x49, 0x94, 0x39, 0x93, 0x58, 0x85, 0x29, 0x67, 0x76,
            0x92, 0x19, 0x91, 0x48, 0x84, 0x57, 0x75, 0x38, 0x83, 0x66, 0x28, 0x82, 0x18, 0x47,
            0x74, 0x81, 0x08, 0x80, 0x56, 0x65, 0x17, 0x07, 0x70, 0x73, 0x37, 0x27, 0x72, 0x46,
            0x64, 0x55, 0x71, 0x36, 0x63, 0x45, 0x54, 0x26, 0x62, 0x16, 0x61, 0x06, 0x60, 0x35,
            0x53, 0x44, 0x25, 0x52, 0x15, 0x05, 0x50, 0x51, 0x34, 0x43, 0x24, 0x42, 0x33, 0x14,
            0x41, 0x04, 0x40, 0x23, 0x32, 0x13, 0x31, 0x03, 0x30, 0x22, 0x12, 0x21, 0x02, 0x20,
            0x11, 0x01, 0x10, 0x00,
        ],
    )),
];

/// Der Gruppenindex für ein `table_select` aus der Seiteninfo.
///
/// `None` für 0, 4 und 14 — die sind in `ff_mpa_huff_data` **nicht belegt**, und
/// ein belegter Rückgabewert wäre geraten.
pub fn gruppe(table_select: u8) -> Option<usize> {
    let &(vlc, _) = DATEN.get(table_select as usize)?;
    if vlc == 0 {
        return None;
    }
    let g = usize::from(vlc) - 1;
    if g < TABELLEN.len() { Some(g) } else { None }
}

/// Die Tabelle der Gruppe `g`, oder `None`.
///
/// # Warum das nicht `TABELLEN[g].unwrap()` ist
///
/// GEMESSEN 02.10.2026: die Gruppen **12 bis 14** fehlen (je 256 Einträge).
/// Ohne diese Funktion zeigte [`gruppe`] für `table_select = 15` auf die
/// Gruppe 14 — und damit auf die Daten von „table 24". Der Enkoder hätte
/// gültige Bytes für die falsche Tabelle geschrieben, und der Leser hätte
/// daraus plausiblen Klang gemacht.
///
/// **Ein fehlender Eintrag muss fehlen.** Das ist die ganze Aufgabe dieser
/// Funktion.
pub fn tabelle(g: usize) -> Option<(&'static [u8], &'static [u8])> {
    TABELLEN.get(g).copied().flatten()
}

/// Die Zahl **zusätzlicher** Bit für Werte ab 15 — `linbits`.
///
/// `None` für ein `table_select`, das es nicht gibt oder nicht belegt ist.
pub fn linbits(table_select: u8) -> Option<u8> {
    let &(vlc, lb) = DATEN.get(table_select as usize)?;
    if vlc == 0 {
        return None;
    }
    Some(lb)
}

/// Die höchste Stufe, die `(x, y)` in dieser Gruppe erreichen kann.
///
/// Also der grösste Wert, für den die Tabelle ein Codewort hat — und damit die
/// Grenze, an der `linbits` ins Spiel käme.
pub fn hoechste_stufe(g: usize) -> Option<u8> {
    let (_, werte) = tabelle(g)?;
    werte.iter().map(|w| (w >> 4).max(w & 0x0f)).max()
}

/// Das gepackte Symbol eines Paares `(x, y)`.
///
/// Aus `tmp_symbols[j] = high << 1 | ((high && low) << 4) | low`:
///
/// ```text
/// symbol = x * 32 + (x != 0 && y != 0 ? 16 : 0) + y
/// ```
pub fn symbol(x: u8, y: u8) -> u32 {
    u32::from(x) * 32 + if x != 0 && y != 0 { 16 } else { 0 } + u32::from(y)
}

/// Das Codewort und seine Länge für `(x, y)` in Gruppe `g`.
///
/// **Kanonische Zuordnung**, wie `ff_vlc_init_from_lengths` sie macht: erst nach
/// Länge aufsteigend, dann in der Reihenfolge des Symbolfelds.
///
/// # Fehler
///
/// `None`, wenn die Gruppe nicht existiert, `(x, y)` kein Codewort hat, oder ein
/// Wert **negativ** ist. Es entsteht dann kein Bit.
pub fn codewort(g: usize, x: u8, y: u8) -> Option<(u32, u32)> {
    let (laengen, werte) = tabelle(g)?;
    //
    // DIE REGEL, und sie ist **nicht** die kanonische.
    //
    // `ff_vlc_init_from_lengths()` in `libavcodec/vlc.c` **sortiert gar nicht**.
    // Es ist ein fortlaufender 32-Bit-Akkumulator **in Feldreihenfolge**:
    //
    // ```c
    // j = code = 0;
    // for (int i = 0; i < nb_codes; i++, lens += lens_wrap) {
    //     int len = *lens;
    //     if (len > 0) { buf[j].bits = len; ...; buf[j++].code = code; }
    //     else if (len < 0) len = -len;
    //     else continue;
    //     if (len > len_max || code & ((1U << (32 - len)) - 1)) { /* Fehler */ }
    //     code += 1U << (32 - len);
    // }
    // ```
    //
    // Der code eines Eintrags ist also `acc >> (32 - len)`, und danach waechst
    // `acc` um `2^(32 - len)`.
    //
    // GEMESSEN 02.10.2026, und das war ein **Fehler von mir**: ich hatte zuerst
    // die kanonische Regel mit Sortierung nach Symbolwert gebaut, weil die
    // Tabellen nach Länge aufsteigend **aussehen** — sie sind es aber nicht.
    // Die Folge war, dass die Stufen **nicht monoton** herauskamen:
    //
    // ```text
    //   x = 0 -> 1339    x = 1 -> 1226    x = 2 -> 367    x = 3 -> 1205
    // ```
    //
    // Eine Stufenkurve, bei der Stufe 2 leiser ist als Stufe 1, ist unmöglich —
    // und so ist die Regel gefunden worden. Zweimal falsch war ich vorher:
    // einmal mit der Feldreihenfolge, einmal mit der Sortierung nach Symbolwert.
    //
    // GEMESSEN, und die Regel ist **gültig**: der Akkumulator endet bei genau
    // `2^32`, die Kraft-Summe ist 1, und kein Codewort ist ein Präfix eines
    // anderen. Das ist fuer alle fuenf Gruppen nachgerechnet und als Test
    // festgehalten.
    let mut acc: u64 = 0;
    for (i, &l8) in laengen.iter().enumerate() {
        let l = u32::from(l8);
        let schritt = 1u64 << (32 - l);
        let code = acc >> (32 - l);
        let w = werte[i];
        if u32::from(w >> 4) == u32::from(x) && u32::from(w & 0x0f) == u32::from(y) {
            return Some((code as u32, l));
        }
        acc += schritt;
    }
    None
}

/// Schreibt ein Wertepaar mit `table_select`.
///
/// # DIE ZAHL DER VORZEICHENBITS, und sie ist nicht „je Komponente"
///
/// GEMESSEN 02.10.2026, und das war ein Fehler, der die Messung **stauchte**,
/// ohne zu melden. Der Leser:
///
/// ```c
/// y = get_vlc2(...);
/// if (!y) { sb_hybrid[s_index] = sb_hybrid[s_index+1] = 0; s_index += 2; continue; }
/// if (y & 16) {
///     x = y >> 5;  y = y & 0x0f;
///     if (x < 15) READ_FLIP_SIGN(g->sb_hybrid + s_index,     ...[exponent] + x)
///     if (y < 15) READ_FLIP_SIGN(g->sb_hybrid + s_index + 1, ...[exponent] + y)
/// } else {
///     x = y >> 5;  y = y & 0x0f;  x += y;
///     if (x < 15) READ_FLIP_SIGN(g->sb_hybrid + s_index + !!y, ...[exponent] + x)
///     g->sb_hybrid[s_index + !y] = 0;
/// }
/// ```
///
/// Also:
///
/// ```text
///   x == 0 && y == 0   ->  0 Vorzeichenbit   (`if (!y)` nimmt den Zweig)
///   genau eine != 0    ->  1 Vorzeichenbit   (ein READ_FLIP_SIGN)
///   beide != 0         ->  2 Vorzeichenbit
/// ```
///
/// Ich hatte „je gesetzter Komponente" angenommen, also **0** im Fall einer.
/// Folge: **ein** Bit zu wenig, der ganze Strom danach um ein Bit verschoben —
/// und GEMESSEN wurden die Stufenverhältnisse **1 : 1,418 : 1,915** statt
/// `1 : 2,5198 : 4,3267`. Sie waren **monoton**, aber **gestaucht**, und das hat
/// wie ein Amplitudenproblem ausgesehen.
///
/// Das ist dieselbe Sorte Fehler wie die doppelten Vorzeichen in
/// `BERICHT_KONVERT_K2_MP3_REQUANTISIERUNG.txt`: gültige Bytes, falscher Inhalt,
/// keine Fehlermeldung.
///
/// # `linbits` — AB WERT 15, UND DIE REGEL IST NICHT DIE, DIE ICH HATTE
///
/// Ab Wert **15** steht im Codewort eine **15**, und der wahre Wert folgt in
/// `linbits` Zusatzbit. GEMESSEN 02.10.2026 am Leser
/// (`libavcodec/mpegaudiodec_template.c`, `huffman_decode()`):
///
/// ```c
/// if (y & 16) {                          // beide Komponenten im Symbol
///     x = y >> 5;  y = y & 0x0f;
///     if (x < 15) READ_FLIP_SIGN(..., x)          // nur EINE Komponente
///     else { x += get_bitsz(&s->gb, linbits);     // *** 15 PLUS *** Zusatz
///            v = l3_unscale(x, exponent);
///            if (get_bits1(&s->gb)) v = -v; }
///     if (y < 15) READ_FLIP_SIGN(..., y)
///     else { y += get_bitsz(&s->gb, linbits);
///            v = l3_unscale(y, exponent);
///            if (get_bits1(&s->gb)) v = -v; }
/// } else {                              // nur EINE Komponente im Symbol
///     x = y >> 5;  y = y & 0x0f;  x += y;
///     if (x < 15) READ_FLIP_SIGN(..., x)
///     else { x += get_bitsz(&s->gb, linbits); ... }
///     g->sb_hybrid[s_index + !y] = 0;
/// }
/// ```
///
/// Daraus folgen **drei** Regeln, und ich hatte vorher **keine** davon richtig:
///
/// **R1 — der Zusatzwert ist `wert - 15`, nicht `wert`.**
/// `x += get_bitsz(...)` addiert zum Symbolwert, und der Symbolwert ist 15.
/// Mein Kommentar behauptete das Gegenteil („der Wert steht **voll** in den
/// Zusatzbit"). Das ist die Form, die man auswendig erwartet, und sie ist
/// falsch. Der Fehler war schlafend, weil der Weg vorher unerreichbar war.
///
/// **R2 — sind BEIDE Komponenten geflüchtet, kommen ZWEI Fluchtwege.**
/// Ich hatte `if x >= 15 { (15, y) } else { (x, 15) }` — ein einziger Weg für
/// zwei Komponenten. Der Leser liest aber zwei. Der Strom wäre ab der ersten
/// geflüchteten Komponente um `linbits + 1` Bit verschoben.
///
/// **R3 — eine geflüchtete und eine nicht geflüchtete Komponente: es kommt ein
/// ZWEITES Vorzeichenbit.** Weil die nicht geflüchtete im selben `if (y & 16)`
/// Zweig ihr eigenes `READ_FLIP_SIGN` hat. Ich hatte nur ein Bit geschrieben.
///
/// Und `(0, 0)` hat **kein** Vorzeichenbit — das ist der Kurzschluss
/// `if (!y) { beide null; continue; }` ganz oben, und es war die einzige der
/// vier Regeln, die ich richtig hatte.
///
/// # Wertebereich
///
/// `x` und `y` sind `u32`, weil der Weg bis `15 + (2^13 - 1) = 8206` geht
/// (`linbits` haengt an `table_select`, nicht an der Tabelle — Maximum 13 bei
/// `table_select = 31`).
///
/// # Rückgabe
///
/// Die Zahl der geschriebenen Bit, oder `None` — und dann ist **kein** Bit
/// geschrieben.
pub fn schreibe_paar(
    bits: &mut Bits,
    table_select: u8,
    x: u32,
    y: u32,
    vorzeichen: (bool, bool),
) -> Option<u32> {
    let g = gruppe(table_select)?;
    let linbits = linbits(table_select)?;

    //
    // PRUEFEN, DANN SCHREIBEN. Erst das Codewort holen, dann beide Zusatzwege
    // und ihre Werte gegen `linbits` messen — und **erst danach** das erste
    // Bit setzen. Sonst bleibt bei einem abgewiesenen Paar ein halbes Codewort
    // im Strom, und der Leser findet keine Fehlermeldung.
    //
    // GEMESSEN 02.10.2026: `codewort(g, 15, y)` gibt `None` fuer eine Tabelle
    // ohne 15 — das ist der Weg, ueber den die elf kleinen Tabellen eine Anfrage
    // ab 15 **ablehnen**. Bei den 256-Eintraege-Tabellen gibt es ihn.
    //
    //
    // GEMESSEN 02.10.2026: `codewort(g, x as u8, y as u8)` waere hier ein
    // **Stolperstein**. `x = 260` wird von `as u8` zu `4`, und `codewort(g, 4,
    // y)` gibt es — der Enkoder haette den Codewort eines Wertes 4 geschrieben
    // und dann 245 Zusatzbit angehaengt. Also wird **immer** die 15 eingesetzt,
    // genau wie im Leser.
    let sx = if x >= 15 { 15 } else { x as u8 };
    let sy = if y >= 15 { 15 } else { y as u8 };
    let (code, laenge) = codewort(g, sx, sy)?;

    // Welche Komponenten brauchen den Zusatzweg? `beide` entscheidet es:
    // `symbol(x, y)` setzt Bit 4 genau dann, wenn `x != 0 && y != 0` — und
    // nur dieser Zweig liest **zwei** Komponenten.
    let beide = x != 0 && y != 0;
    let zusatz: &[(u32, bool)] = if beide {
        &[(x, vorzeichen.0), (y, vorzeichen.1)]
    } else if x != 0 {
        &[(x, vorzeichen.0)]
    } else {
        &[(y, vorzeichen.1)]
    };
    for &(wert, _) in zusatz {
        if wert >= 15 {
            // R1: `x += get_bitsz(linbits)` — der Symbolwert 15 steckt schon
            // im Codewort, geschrieben wird also `wert - 15`.
            let rest = wert - 15;
            if linbits == 0 || rest >= (1u32 << linbits) {
                return None;
            }
        }
    }

    // Und jetzt schreiben.
    bits.feld(code, laenge);
    let mut n = laenge;
    //
    // DER KURZSCHLUSS, und er steht **ganz oben** in `huffman_decode()`, vor
    // jeder Komponentenlogik:
    //
    // ```c
    // if (!y) { g->sb_hybrid[s_index] = g->sb_hybrid[s_index + 1] = 0;
    //           s_index += 2; continue; }
    // ```
    //
    // `(0, 0)` hat also **kein** Vorzeichenbit.
    //
    // GEMESSEN 02.10.2026: als ich das beim Umschreiben des `linbits`-Weges
    // verloren habe, gab ein leeres Paar **2582** statt **0** — und die ganze
    // Stufenkurve war hin, weil `(0, 0)` der Nullpunkt ist, gegen den alle
    // anderen Stufen gemessen werden. Ein Fehler an **einem** Randwert zerlegt
    // die **gesamte** Kurve, weil sie dort normiert wird.
    if x == 0 && y == 0 {
        return Some(n);
    }
    for &(wert, neg) in zusatz {
        if wert >= 15 {
            bits.feld(wert - 15, u32::from(linbits));
            n += u32::from(linbits);
        }
        bits.bit(neg);
        n += 1;
    }
    Some(n)
}

/// Ob die Tabelle `table_select` den `linbits`-Weg überhaupt kennt.
///
/// GEMESSEN 02.10.2026: `linbits` steht **nicht** bei null für die Tabellen ohne
/// den Weg — es steht bei null, weil diese Tabellen nur Stufen bis 14 kodieren.
/// Für sie ist ein Wert ab 15 also **grundsaetzlich** unmoeglich, und nicht nur
/// ungebaut.
pub fn hat_linbits(table_select: u8) -> bool {
    linbits(table_select).is_some_and(|l| l > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jede Tabelle ist ein **vollständiger** Code: Kraft-Summe 1.
    ///
    /// GEMESSEN 02.10.2026: alle fünf ergeben **1,0** ganzzahlig. Das ist
    /// dieselbe Bedingung, die in `BERICHT_KONVERT_K2_MP3_COUNT1_VOLLSTAENDIG.txt`
    /// für den count1-Code gestanden hat — und damals habe ich 0,984375
    /// behauptet, weil ich ein Element zu viel gezählt hatte.
    ///
    /// Der Test rechnet **ohne Gleitkomma**, damit das Ergebnis exakt ist —
    /// und die Skala ist eine **Zweierpotenz**.
    ///
    /// GEMESSEN 02.10.2026, und das war ein Fehler im **Test**, nicht in der
    /// Tabelle: mit der Skala `1 000 000` ergibt Gruppe 4 (`laenge = 8`)
    /// `999 997` statt `1 000 000`, weil `1 000 000 >> 7 = 7812` und nicht
    /// `7812,5` — und `>> 7` schneidet ab.
    ///
    /// ```text
    ///   Gruppe 4: Laengen bis 8   ->  mit Skala 1 000 000: 999 997
    ///   Gruppe 5: Laengen bis 7   ->  mit Skala 1 000 000: 999 999
    /// ```
    ///
    /// Beide Tabellen sind **vollständig** — mit einer Zweierpotenz als Skala
    /// ergeben sie genau 1. Die Lehre: eine Ganzzahlrechnung, die ein exaktes
    /// Ergebnis **behauptet**, muss ihre Skala so waehlen, dass die Division
    /// aufgeht. Sonst beweist der Test die Skala und nicht die Tabelle.
    const SKALA: u64 = 1u64 << 40;
    #[test]
    fn jede_tabelle_hat_die_kraft_summe_eins() {
        for (g, eintrag) in TABELLEN.iter().enumerate() {
            let (laengen, werte) = match eintrag {
                Some(t) => *t,
                None => continue,
            };
            assert_eq!(laengen.len(), werte.len(), "Gruppe {g}: Laengen und Werte");
            let mut summe = 0u64;
            for &l in laengen.iter() {
                assert!((1..32).contains(&l), "Gruppe {g}: Laenge {l}");
                summe += SKALA >> l;
            }
            assert_eq!(
                summe, SKALA,
                "Gruppe {g}: Kraft-Summe ist {summe}/{SKALA} statt 1"
            );
        }
    }

    /// Und kein Codewort ist ein **Präfix** eines anderen.
    #[test]
    fn die_cwoerter_sind_praefixfrei() {
        for g in 0..TABELLEN.len() {
            //
            // GEMESSEN 02.10.2026: mit den Luecken 12..14 ruft `hoechste_stufe`
            // `None`, und `.expect("Hoechststufe")` paniked. Eine Luecke ist kein
            // Fehler — sie ist eine **Abwesenheit**, und ein Test ueber einen
            // Bereich, der nicht existiert, ist kein Test.
            if tabelle(g).is_none() {
                continue;
            }
            //
            // GEMESSEN 02.10.2026: der Test lief ueber `0..4` — fuer die
            // Tabellen mit Stufen bis 7 war das nur eine **Teilmenge**, und
            // eine Teilmenge kann keine Praefixverletzung finden, die in ihrem
            // Komplement steckt. Also jetzt ueber den ganzen Wertebereich.
            let hoechst = hoechste_stufe(g).expect("Hoechststufe");
            let mut alle: Vec<(u32, u32)> = Vec::new();
            for x in 0..=hoechst {
                for y in 0..=hoechst {
                    if let Some(c) = codewort(g, x, y) {
                        alle.push(c);
                    }
                }
            }
            for &(c1, l1) in alle.iter() {
                for &(c2, l2) in alle.iter() {
                    if l1 >= l2 {
                        continue;
                    }
                    let praefix = c2 >> (l2 - l1);
                    //
                    // GEMESSEN 02.10.2026: die Breitenangabe laesst sich in
                    // EINEM Formatstring nur einmal verwenden — jede
                    // `{:0l$b}` frisst ihr Argument. Also erst als Text.
                    let (w1, w2) = (l1 as usize, l2 as usize);
                    let kurz = format!("{c1:0w1$b}");
                    let lang = format!("{c2:0w2$b}");
                    assert_ne!(c1, praefix, "Gruppe {g}: {kurz} ist ein Präfix von {lang}");
                }
            }
        }
    }

    /// Und jedes Symbol kommt **genau einmal** vor.
    #[test]
    fn jedes_symbol_kommt_einmal_vor() {
        for (g, eintrag) in TABELLEN.iter().enumerate() {
            let (_, werte) = match eintrag {
                Some(t) => *t,
                None => continue,
            };
            let mut gesehen = [false; 256];
            for &w in werte.iter() {
                assert!(
                    !gesehen[usize::from(w)],
                    "Gruppe {g}: Symbol {:02x} kommt zweimal vor",
                    w
                );
                gesehen[usize::from(w)] = true;
            }
        }
    }

    /// Das gepackte Symbol ist die **Umkehrung** von ffmpegs Transformation.
    ///
    /// GEMESSEN 02.10.2026 an `tmp_symbols[j] = high << 1 | ((high && low) << 4)
    /// | low`. Ohne diese Umkehrung wäre Bit 4 ein Wert und kein Flag.
    #[test]
    fn symbol_ist_die_umkehrung_von_ffmpeg() {
        for x in 0..4u8 {
            for y in 0..4u8 {
                let s = symbol(x, y);
                // ffmpeg: high = (x<<4) & 0xF0, low = y & 0x0F
                let high = x << 4;
                let low = y & 0x0f;
                let erwartet = u32::from(high) * 2
                    + if high != 0 && low != 0 { 16 } else { 0 }
                    + u32::from(low);
                assert_eq!(s, erwartet, "x = {x}, y = {y}");
                // Und der Leser:
                if s & 16 != 0 {
                    assert_ne!(x, 0, "Flag gesetzt, aber x = 0");
                    assert_ne!(y, 0, "Flag gesetzt, aber y = 0");
                    assert_eq!(s >> 5, u32::from(x));
                    assert_eq!(s & 0x0f, u32::from(y));
                } else {
                    assert!(
                        x == 0 || y == 0,
                        "Flag nicht gesetzt, aber x = {x} und y = {y}"
                    );
                }
            }
        }
        // Und die zwei Belege, die ich im Kopf nachgerechnet habe:
        assert_eq!(symbol(1, 1), 0x31, "0x11 wird zu 49");
        assert_eq!(symbol(0, 1), 0x01, "0x01 bleibt 1");
        assert_eq!(symbol(0, 0), 0x00, "und 0x00 bleibt 0");
    }

    /// Und die Codewörter sind die **aus der Quelle** — mit dem Akkumulator,
    /// nicht mit einer Sortierung.
    ///
    /// GEMESSEN 02.10.2026, Gruppe 1 (`ff_huff_vlc[1]`, Längen `3,3,2,1`):
    ///
    /// ```text
    ///   (1,1) -> 000      (0,1) -> 001      (1,0) -> 01      (0,0) -> 1
    /// ```
    ///
    /// Nachgerechnet: `acc` startet bei 0, und jeder Schritt addiert `2^(32-len)`
    /// — also `000`, dann `001`, dann `01`, dann `1`. Das ist **präfixfrei**, und
    /// die Kraft-Summe ist 1.
    #[test]
    fn die_cwoerter_sind_aus_dem_akkumulator() {
        let erwartet: [((u8, u8), (u32, u32)); 4] = [
            ((1, 1), (0b000, 3)),
            ((0, 1), (0b001, 3)),
            ((1, 0), (0b01, 2)),
            ((0, 0), (0b1, 1)),
        ];
        for &(xy, cw) in erwartet.iter() {
            assert_eq!(codewort(0, xy.0, xy.1), Some(cw), "Gruppe 1, {xy:?}");
        }
        // Und Gruppe 5, „table 6", Längen `7,7,6,6,6,5,5,5,5,4,4,4,3,2,3,3`:
        //
        // ```text
        //   (3,3) -> 0000000   (0,3) -> 0000001   (2,3) -> 000001   (3,2) -> 000010
        //   (3,0) -> 000011    (1,3) -> 00010     (3,1) -> 00011    (2,2) -> 00100
        //   (0,2) -> 00101     (1,2) -> 0011      (2,1) -> 0100     (2,0) -> 0101
        //   (0,1) -> 011       (1,1) -> 10        (1,0) -> 110      (0,0) -> 111
        // ```
        let g5: [((u8, u8), (u32, u32)); 16] = [
            ((3, 3), (0b0000000, 7)),
            ((0, 3), (0b0000001, 7)),
            ((2, 3), (0b000001, 6)),
            ((3, 2), (0b000010, 6)),
            ((3, 0), (0b000011, 6)),
            ((1, 3), (0b00010, 5)),
            ((3, 1), (0b00011, 5)),
            ((2, 2), (0b00100, 5)),
            ((0, 2), (0b00101, 5)),
            ((1, 2), (0b0011, 4)),
            ((2, 1), (0b0100, 4)),
            ((2, 0), (0b0101, 4)),
            ((0, 1), (0b011, 3)),
            ((1, 1), (0b10, 2)),
            ((1, 0), (0b110, 3)),
            ((0, 0), (0b111, 3)),
        ];
        for &(xy, cw) in g5.iter() {
            assert_eq!(codewort(4, xy.0, xy.1), Some(cw), "Gruppe 5, {xy:?}");
        }
    }

    /// Und die **Höchststufe** passt zu dem, was in der Tabelle steht.
    #[test]
    fn die_hoechste_stufe_ist_aus_der_tabelle() {
        assert_eq!(hoechste_stufe(0), Some(1), "Gruppe 1: x,y in 0..1");
        assert_eq!(hoechste_stufe(1), Some(2), "Gruppe 2: x,y in 0..2");
        assert_eq!(hoechste_stufe(2), Some(2), "Gruppe 3: x,y in 0..2");
        assert_eq!(hoechste_stufe(3), Some(3), "Gruppe 5: x,y in 0..3");
        assert_eq!(hoechste_stufe(4), Some(3), "Gruppe 6: x,y in 0..3");
        assert_eq!(hoechste_stufe(11), None, "Gruppe 11 gibt es nicht");
        // Und alles darueber ist ein Fehler, kein Ersatzwert:
        assert_eq!(codewort(4, 4, 0), None, "Stufe 4 gibt es in Gruppe 5 nicht");
        assert_eq!(codewort(0, 2, 0), None, "Stufe 2 gibt es in Gruppe 1 nicht");
    }

    /// Und `table_select` 0, 4 und 14 sind **nicht belegt**.
    ///
    /// GEMESSEN 02.10.2026: `ff_mpa_huff_data` hat dort VLC 0, und VLC 0 ist
    /// nicht gebaut. Der Leser macht dann `memset` — der ganze Bereich ist
    /// stumm. Ein Enkoder, der dort einen Wert annimmt, erzeugt Stille und
    /// bekommt keine Fehlermeldung.
    #[test]
    fn nicht_belegte_tabellen_werden_abgewiesen() {
        for &k in &[0u8, 4, 14] {
            assert_eq!(gruppe(k), None, "table_select {k} ist nicht belegt");
            assert_eq!(linbits(k), None, "table_select {k}");
        }
        // Und die belegten, kleine Gruppen:
        assert_eq!(gruppe(1), Some(0));
        assert_eq!(gruppe(2), Some(1));
        assert_eq!(gruppe(3), Some(2));
        assert_eq!(gruppe(5), Some(3));
        assert_eq!(gruppe(6), Some(4));
        // Und `linbits` steht in der Quelle, nicht geraten:
        assert_eq!(linbits(1), Some(0));
        assert_eq!(linbits(6), Some(0));
        assert_eq!(linbits(16), Some(1));
        assert_eq!(linbits(23), Some(13));
        assert_eq!(linbits(31), Some(13));
    }

    /// Und `schreibe_paar` schreibt **genau** Länge plus die Zahl der
    /// Vorzeichenbits — und die ist **nicht** „je Komponente".
    ///
    /// GEMESSEN 02.10.2026: bei **einer** Komponente liest der Leser **ein**
    /// Vorzeichenbit (`READ_FLIP_SIGN` im Zweig ohne Flag), nicht null. Ich hatte
    /// null geschrieben, der Strom war um ein Bit verschoben, und die
    /// Stufenverhältnisse kamen **gestaucht** heraus: GEMESSEN `1 : 1,418 : 1,915`
    /// statt `1 : 2,5198 : 4,3267`.
    #[test]
    fn schreiben_erzeugt_die_erwartete_bitzahl() {
        for g in 0..TABELLEN.len() {
            if tabelle(g).is_none() {
                continue;
            }
            let ts = table_select_fuer(g);
            let lb = linbits(ts).expect("linbits");
            let hoechst = u32::from(hoechste_stufe(g).expect("Stufe"));
            for x in 0..=hoechst {
                for y in 0..=hoechst {
                    let (code, laenge) = codewort(g, x as u8, y as u8).expect("Paar");
                    //
                    // DIE ERWARTUNG, abgeschrieben aus `huffman_decode()`:
                    //
                    // ```c
                    // if (!y) { beide null; continue; }              // 0 Bit
                    // if (y & 16) { beide Komponenten;
                    //     x < 15 ? READ_FLIP_SIGN : linbits + Bit   // je 1 (+lb)
                    //     y < 15 ? READ_FLIP_SIGN : linbits + Bit }
                    // else       { eine Komponente;                   // 1 (+lb)
                    //     x < 15 ? READ_FLIP_SIGN : linbits + Bit }
                    // ```
                    //
                    // GEMESSEN 02.10.2026: `symbol(x, y)` setzt Bit 4 genau dann,
                    // wenn `x != 0 && y != 0` — das ist der `y & 16` Zweig, und
                    // **er** ist der, der zwei Komponenten liest. Drei Fehler von
                    // mir waren alle „ein Weg genuegt" (siehe `schreibe_paar`).
                    let beide = x != 0 && y != 0;
                    //
                    // Und ganz oben, **vor** allem anderen:
                    //
                    // ```c
                    // if (!y) { g->sb_hybrid[s_index    ] = 0;
                    //           g->sb_hybrid[s_index + 1] = 0;
                    //           s_index += 2; continue; }
                    // ```
                    //
                    // `(0, 0)` hat **kein** Vorzeichenbit, und der Leser kommt nie
                    // bis `READ_FLIP_SIGN`. GEMESSEN 02.10.2026: als ich das beim
                    // Umschreiben des `linbits`-Weges verlor, gab ein leeres Paar
                    // **2582** statt **0** — und weil `(0, 0)` der Nullpunkt
                    // ist, gegen den die ganze Stufenkurve normiert wird, war
                    // damit **die gesamte Kurve** hin, nicht ein Randwert.
                    if x == 0 && y == 0 {
                        let mut b = Bits::neu();
                        let n = schreibe_paar(&mut b, ts, x, y, (false, false))
                            .unwrap_or_else(|| panic!("Gruppe {g}: (0, 0) muss gehen"));
                        assert_eq!(n, laenge, "Gruppe {g}: (0, 0) hat kein Vorzeichenbit");
                        assert_eq!(b.laenge() as u32, laenge);
                        continue;
                    }
                    let liste: &[(u32, bool)] = if beide {
                        &[(x, false), (y, false)]
                    } else if x != 0 {
                        &[(x, false)]
                    } else {
                        &[(y, false)]
                    };
                    let mut erwartet = laenge;
                    for &(wert, _) in liste {
                        if wert >= 15 {
                            erwartet += u32::from(lb);
                        }
                        erwartet += 1;
                    }
                    let mut b = Bits::neu();
                    let n = schreibe_paar(&mut b, ts, x, y, (false, false))
                        .unwrap_or_else(|| panic!("Gruppe {g}, x={x}, y={y}"));
                    assert_eq!(n, erwartet, "Gruppe {g}, x={x}, y={y}");
                    assert_eq!(b.laenge() as u32, erwartet);
                    if laenge <= 8 {
                        assert_eq!(
                            b.fertig()[0] >> (8 - laenge as usize),
                            code as u8,
                            "Codewort von (x={x}, y={y})"
                        );
                    }
                }
            }
        }
    }

    /// Und der Zusatzwert ist **`wert - 15`** — GEMESSEN aus dem Leser:
    /// `x += get_bitsz(&s->gb, linbits)` addiert zum Symbolwert, und der ist 15.
    ///
    /// Das ist die Form, die man auswendig erwartet (`ist` selbst), und sie ist
    /// falsch. Der Fehler war schlafend, weil bis heute keine Tabelle den Weg
    /// erreicht hat — jetzt `table_select = 24`.
    #[test]
    fn der_zusatzwert_ist_wert_minus_15() {
        let ts = 24u8;
        let lb = u32::from(linbits(ts).expect("linbits"));
        assert_eq!(
            lb, 4,
            "table_select 24 hat linbits 4 (Quelle, nicht geraten)"
        );
        for (x, erwartetes_feld) in [(15u32, 0u32), (16, 1), (20, 5), (30, 15)] {
            let mut b = Bits::neu();
            let n = schreibe_paar(&mut b, ts, x, 0, (false, false)).expect("Paar");
            let bytes = b.fertig();
            let laenge = codewort(huffbig_gruppe(ts), 15, 0).expect("Codewort").1 as usize;
            // Das Feld steht **unmittelbar** nach dem Codewort.
            let mut nach = 0u32;
            for i in 0..lb as usize {
                let pos = laenge + i;
                let z = if pos < 8 { bytes[0] } else { bytes[pos / 8] };
                let sch = 7 - (pos % 8);
                nach = nach * 2 + u32::from((z >> sch) & 1);
            }
            assert_eq!(nach, erwartetes_feld, "x = {x}: das Feld muss x-15 sein");
            // Und die Bitzahl: Codewort + 4 + 1 Vorzeichen.
            assert_eq!(
                n,
                u32::try_from(laenge).expect("Laenge") + lb + 1,
                "x = {x}"
            );
        }
    }

    /// Und was **nicht** hineinpasst, wird abgewiesen — **ohne** ein einziges
    /// geschriebenes Bit.
    ///
    /// GEMESSEN 02.10.2026: `table_select = 24` hat 4 `linbits`, der Weg geht also
    /// bis `15 + 15 = 30`. Ein Wert 31 passt nicht in vier Bit, und der alte Code
    /// hatte das Codewort **schon geschrieben**, bevor er `passt` prüfte.
    #[test]
    fn ein_zu_grosser_zusatzwert_wird_ohne_bit_abgewiesen() {
        let mut b = Bits::neu();
        assert!(
            schreibe_paar(&mut b, 24, 31, 0, (false, false)).is_none(),
            "31 braucht 5 Bit, es gibt 4"
        );
        assert_eq!(b.laenge(), 0, "und es darf kein Bit schreiben");
        // Und beide Komponenten gefluechtet: derselbe Weg, zweimal.
        let mut b = Bits::neu();
        assert!(schreibe_paar(&mut b, 24, 30, 30, (false, false)).is_some());
        let n = b.laenge();
        let mut c = Bits::neu();
        let m = schreibe_paar(&mut c, 24, 30, 30, (false, false)).expect("zwei Wege");
        assert_eq!(n as u32, m, "und die Bitzahl stimmt");
        // Eine gefluechtete, eine nicht: **zwei** Vorzeichenbit.
        let laenge = codewort(huffbig_gruppe(24), 15, 3).expect("Codewort").1;
        let k = schreibe_paar(&mut c, 24, 20, 3, (false, false)).expect("gemischt");
        assert_eq!(k, laenge + 4 + 2, "R3: das zweite Vorzeichenbit");
    }

    fn huffbig_gruppe(table_select: u8) -> usize {
        gruppe(table_select).expect("Gruppe")
    }

    /// Und ein Paar, das die Tabelle nicht hat, erzeugt **kein** Bit.
    #[test]
    fn ein_zu_grosses_paar_wird_abgewiesen() {
        let mut b = Bits::neu();
        assert!(schreibe_paar(&mut b, 6, 4, 0, (false, false)).is_none());
        assert_eq!(b.laenge(), 0, "und es darf kein Bit schreiben");
        assert!(schreibe_paar(&mut b, 0, 0, 0, (false, false)).is_none());
        assert_eq!(b.laenge(), 0);
        assert!(schreibe_paar(&mut b, 32, 0, 0, (false, false)).is_none());
        assert_eq!(b.laenge(), 0);
    }

    fn table_select_fuer(g: usize) -> u8 {
        DATEN
            .iter()
            .position(|&(v, _)| usize::from(v) == g + 1)
            .expect("table_select") as u8
    }
}
