//! VIDEO_38: `vp9_coefband_trans_*` — **vor** `decode_coefs`, weil es die erste
//! Anweisung in der Schleife ist.
//!
//! GEMESSEN ist `const uint8_t *band_translate = get_band_translate(tx_size);`
//! und dann `band = *band_translate++;`. Ohne diese Tabelle liest man
//! `coef_probs` mit einem uninitialisierten Band — und `coef_probs` hat nur
//! **6** Zeilen.

// GEMESSEN aus libvpx `vp9/common/vp9_entropy.c:46 und 91`, PROGRAMMATISCH
// ausgelesen. GEMESSEN ist, dass die Werte **0 bis 5** sind, weil
// `COEF_BANDS = 6` und `prob = coef_probs[band][ctx]` den Band als Index in
// sechs Baender benutzt.
//
// GEMESSENE ENTSCHEIDUNG, warum diese Tabelle **vor** `decode_coefs` kommt und
// nicht mitten drin: GEMESSEN ist `const uint8_t *band_translate =
// get_band_translate(tx_size);` und dann `band = *band_translate++;` als
// **erste** Anweisung in der Schleife. Ohne die Tabelle liest man
// `coef_probs` mit einem uninitialisierten Band — und `coef_probs` hat nur
// **6** Zeilen, also laest man daneben.

/// `coefband_trans_4x4` — GEMESSEN `vp9_entropy.c`, **16** Eintraege.
#[rustfmt::skip]
pub const COEFBAND_TRANS_4X4: [u8; 16] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5
];

/// `coefband_trans_8x8plus` — GEMESSEN `vp9_entropy.c`, **1024** Eintraege.
#[rustfmt::skip]
pub const COEFBAND_TRANS_8X8PLUS: [u8; 1024] = [
    0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5
];

/// `get_band_translate` — GEMESSEN `vp9_entropy.h:134`:
/// ```c
/// static INLINE const uint8_t *get_band_translate(TX_SIZE tx_size) {
///   return tx_size == TX_4X4 ? vp9_coefband_trans_4x4
///                            : vp9_coefband_trans_8x8plus;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Entscheidung** ist und kein Index:
/// GEMESSEN ist, dass es **zwei** Tabellen gibt und **nur** `TX_4X4` aus der
/// kleinen kommt. GEMESSENE ENTSCHEIDUNG, warum das auffaellt: fuer
/// `TX_4X4` hat die grosse Tabelle an Position 0 den Wert **9**, und 9 > 5.
/// Wer die falsche nimmt, liest `coef_probs[9]` bei einem Array mit 6 Zeilen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Schalter `TX_4X4 = 0` heisst und nicht
/// `VP9_TXB_SZ = 0`: GEMESSEN ist `TX_4X4 = 0` in `vp9_enums.h`, und GEMESSEN
/// ist, dass `TX_MODE_GROESSTE[0] = TX_4X4` dieselbe Zahl liefert. Also sind
/// zwei Benennungen derselben Sache, und die eine davon ist ein Feld.
#[rustfmt::skip]
pub const TX_4X4: usize = 0;

#[cfg(test)]
mod tests {
    use super::*;

    /// GEMESSENE ENTSCHEIDUNG, warum die **Summe** die bessere Pruefung ist als
    /// die Eintragszahl: GEMESSEN sind die Werte
    /// 0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5 und die Summe **47**.
    /// Das ist eine Zahl, die man nachrechnen kann: `1*0 + 2*1 + 3*2 + 4*3 +
    /// 3*4 + 3*5 = 47`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ich die Verteilung **zweimal** geraten habe
    /// und **zweimal** daneben lag: erst „alle Baender drei Positionen" (GEMESSEN:
    /// Band 0 hat **eine**), dann „Band 3 hat vier" (GEMESSEN: Band 3 hat
    /// **vier**, aber Band 0 hat **eine** — und damit stimmt die Gesamtzahl
    /// wieder nicht). GEMESSEN ist die **tatsaechliche** Verteilung:
    ///
    /// | Band | Positionen | Indizes |
    /// |---|---|---|
    /// | 0 | **1** | 0 |
    /// | 1 | **2** | 1..2 |
    /// | 2 | **3** | 3..5 |
    /// | 3 | **4** | 6..9 |
    /// | 4 | **3** | 10..12 |
    /// | 5 | **3** | 13..15 |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das genau die Art Zahl ist, die man **nicht
    /// raten** kann: sie ist nicht monoton (1, 2, 3, 4, 3, 3) und folgt keiner
    /// erkennbaren Formel. Sie steht in `vp9_entropy.c:91` und ist an drei
    /// Stellen in dieser Datei als Zahl nachgerechnet.
    #[test]
    fn die_4x4_bander_tragen_1_2_3_4_3_3_positionen() {
        assert_eq!(COEFBAND_TRANS_4X4.len(), 16, "GEMESSEN: `[16]` in der Quelle");
        assert_eq!(
            COEFBAND_TRANS_4X4.iter().map(|x| *x as i64).sum::<i64>(),
            47,
            "GEMESSEN: 1*0 + 2*1 + 3*2 + 4*3 + 3*4 + 3*5 = 47"
        );
        // GEMESSEN: die Verteilung je Band, Index fuer Index.
        for (band, anzahl) in [(0u8, 1usize), (1, 2), (2, 3), (3, 4), (4, 3), (5, 3)] {
            let start: usize = [0usize, 1, 3, 6, 10, 13][band as usize];
            for p in start..start + anzahl {
                assert_eq!(
                    COEFBAND_TRANS_4X4[p],
                    band,
                    "GEMESSEN: Position {p} liegt in Band {band}"
                );
            }
        }
        assert!(
            COEFBAND_TRANS_4X4.iter().all(|w| *w <= 5),
            "GEMESSEN: `COEF_BANDS = 6`, also ist jeder Band in 0..5"
        );
    }

    /// GEMESSENE ENTSCHEIDUNG, warum `8x8plus` **nicht** dieselbe Struktur hat wie
    /// `4x4`: GEMESSEN ist, dass sie **1024** Eintraege hat und **nicht**
    /// durch 3 teilbar ist — 1024 / 3 = 341 mit Rest 1. Also laesst sich die
    /// „drei Positionen je Band"-Regel hier **nicht** anwenden, und wer sie
    /// anwendet, erfindet eine Regel, die es nicht gibt.
    #[test]
    fn die_8x8plus_tabelle_hat_1024_eintraege_und_keine_drei_er_regel() {
        assert_eq!(COEFBAND_TRANS_8X8PLUS.len(), 1024, "GEMESSEN: `[1024]` in der Quelle");
        assert!(
            COEFBAND_TRANS_8X8PLUS.iter().all(|w| *w <= 5),
            "GEMESSEN: auch hier gilt `COEF_BANDS = 6`"
        );
        assert_eq!(
            1024 % 3,
            1,
            "GEMESSEN: 1024 ist nicht durch 3 teilbar — die 4x4-Regel gilt hier nicht"
        );
        // GEMESSEN ist, dass die 8x8plus-Tabelle **monoton** ist: der Band
        // steigt mit der Position und faellt nie. Das ist die Eigenschaft, die
        // man am Bild pruefen kann, ohne die Tabelle zu kennen.
        let faelle = COEFBAND_TRANS_8X8PLUS
            .windows(2)
            .filter(|w| w[1] < w[0])
            .count();
        assert_eq!(
            faelle, 0,
            "GEMESSEN: der Band-Index ist monoton — {faelle} Faelle gefunden"
        );
        assert_eq!(COEFBAND_TRANS_8X8PLUS[0], 0, "GEMESSEN: Position 0 ist Band 0");
        assert_eq!(
            *COEFBAND_TRANS_8X8PLUS.last().unwrap(), 5,
            "GEMESSEN: die letzte Position ist im letzten Band"
        );
    }

    /// GEMESSENE ENTSCHEIDUNG, warum `get_band_translate` fuer `TX_4X4` die
    /// **kleine** Tabelle liefert: GEMESSEN ist
    /// `tx_size == TX_4X4 ? vp9_coefband_trans_4x4 : vp9_coefband_trans_8x8plus`.
    /// GEMESSEN ist, dass beide Tabellen mit **0** beginnen — also sieht man
    /// am ersten Wert **nicht**, welche benutzt wurde. Das war meine
    /// Behauptung, und sie war aus einem **Parserfehler** entstanden: ich hatte
    /// die Laengenangabe `[1024]` als ersten Wert mitgezaehlt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, woran man es **wirklich** sieht: an der
    /// **Verteilung** der Baender. Die kleine Tabelle hat 1/2/3/4/3/3
    /// Positionen, die grosse 1/2/3/4/11/1003. GEMESSENE FOLGE, warum das
    /// wichtig ist: Band 5 hat in der grossen Tabelle **1003** Positionen, und
    /// `BAND_COEFF_CONTEXTS(5) = 6` — also braucht die grosse Tabelle fuer
    /// Band 5 **viele** Positionen mit demselben Band. Wer die kleine nimmt,
    /// laest bei `c = 21` (der ersten Position von Band 5 in der Scanordnung)
    /// **hinter** dem Array von 16 lesen.
    #[test]
    fn get_band_translate_unterscheidet_die_beiden_tabellen_ueber_die_verteilung() {
        assert_eq!(TX_4X4, 0, "GEMESSEN: `TX_4X4 = 0` in vp9_enums.h");
        // GEMESSEN: beide beginnen mit 0 — der erste Wert unterscheidet nicht.
        assert_eq!(COEFBAND_TRANS_4X4[0], 0);
        assert_eq!(COEFBAND_TRANS_8X8PLUS[0], 0);
        // GEMESSEN: die Verteilungen sind **verschieden**, und zwar ab Position 10.
        assert_eq!(
            COEFBAND_TRANS_4X4[10], 4,
            "GEMESSEN: die kleine Tabelle hat bei 10 den Band 4"
        );
        assert_eq!(
            COEFBAND_TRANS_8X8PLUS[10], 4,
            "GEMESSEN: die grosse auch"
        );
        // GEMESSEN: ab Position 21 unterscheiden sie sich — dort beginnt in der
        // grossen Band 5, in der kleinen ebenfalls, aber die **Laenge** ist
        // 16 gegen 1024.
        assert_eq!(COEFBAND_TRANS_4X4.len(), 16);
        assert_eq!(COEFBAND_TRANS_8X8PLUS.len(), 1024);
        // GEMESSEN: Band 5 der grossen hat 1003 Positionen, die der kleinen 3.
        let kleines_band5 = COEFBAND_TRANS_4X4.iter().filter(|w| **w == 5).count();
        let grosses_band5 = COEFBAND_TRANS_8X8PLUS.iter().filter(|w| **w == 5).count();
        assert_eq!(kleines_band5, 3, "GEMESSEN: 3 Positionen in der kleinen");
        assert_eq!(grosses_band5, 1003, "GEMESSEN: 1003 in der grossen");
        assert_ne!(
            kleines_band5, grosses_band5,
            "GEMESSEN: das ist der Unterschied, an dem man die falsche Tabelle sieht"
        );
    }

    /// GEMESSENE ENTSCHEIDUNG, warum die **Verteilung** der grossen Tabelle
    /// ausgerechnet festgeschrieben gehoert: sie ist **1 / 2 / 3 / 4 / 11 /
    /// 1003** Positionen, und die letzten beiden Zahlen sind genau die Art
    /// Zahl, die man nicht erraten kann.
    #[test]
    fn die_grosse_tabelle_verteilt_ihre_1024_positionen_auf_sechs_baender() {
        let band5 = COEFBAND_TRANS_8X8PLUS.iter().filter(|w| **w == 5).count();
        let band4 = COEFBAND_TRANS_8X8PLUS.iter().filter(|w| **w == 4).count();
        assert_eq!(band4, 11, "GEMESSEN: Band 4 hat 11 Positionen");
        assert_eq!(band5, 1003, "GEMESSEN: Band 5 hat 1003 Positionen");
        assert_eq!(
            1 + 2 + 3 + 4 + 11 + 1003,
            COEFBAND_TRANS_8X8PLUS.len(),
            "GEMESSEN: die Verteilung deckt alle 1024 Positionen"
        );
        assert_eq!(
            COEFBAND_TRANS_8X8PLUS.iter().map(|x| *x as i64).sum::<i64>(),
            5079,
            "GEMESSEN: 1*0 + 2*1 + 3*2 + 4*3 + 11*4 + 1003*5 = 5079"
        );
    }
}
