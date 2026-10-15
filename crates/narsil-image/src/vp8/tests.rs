//! Prüfungen für den VP8-Bool-Dekoder und den Baum-Leser.
//!
//! Beides wird gegen die **Norm** geprüft, nicht gegen eine Vermutung:
//!   * der Bool-Dekoder gegen den Encoder aus RFC 6386, Abschnitt 7.3
//!     (Hin- und Zurückkodieren)
//!   * der Baum gegen die Kodierungen, die RFC 6386, Abschnitt 8.2
//!     wörtlich angibt
//!   * der ungepackte Kopf gegen eine echte cwebp-Datei, siehe
//!     `tests/vp8_kopf.rs`
//!
//! 01.10.2026.

use super::*;
use crate::vp8::BoolDekoder;

/// Der normative Encoder aus RFC 6386, 7.3.
///
/// `out` enthaelt NUR geschriebene Byte. GEMESSEN 01.10.2026: mit einer
/// Reserve am Pufferende ging `add_one` in die Polsterung statt in die
/// Daten, und nach `flush` war ein Byte zu viel abgeschnitten — beides
/// hat die Rundlaufpruefung sichtbar scheitern lassen (nur bei
/// `prob = 255`, wo der Uebertrag vier Byte weit zuruecklaeuft).
struct RfcEncoder {
    out: Vec<u8>,
    bott: u32,
    range: u32,
    bit_count: u32,
}

impl RfcEncoder {
    /// "init_bool_encoder": range 255, bottom 0, bit_count 24.
    fn new() -> Self {
        RfcEncoder {
            out: Vec::new(),
            bott: 0,
            range: 255,
            bit_count: 24,
        }
    }

    /// "void add_one_to_output(uint8 *q)":
    /// "while (*--q == 255) *q = 0; ++*q;"
    ///
    /// Zeigt `q` im C auf `output[-1]` und laeuft rueckwaerts, also
    /// greift es die zuletzt geschriebenen Byte. Laeuft es aus, traegt
    /// eine Eins am Anfang nach — der RFC sagt, dass das nie passiert.
    fn add_one(&mut self) {
        while let Some(x) = self.out.pop() {
            if x == 0xFF {
                self.out.push(0);
                continue;
            }
            self.out.push(x + 1);
            return;
        }
        self.out.push(1);
    }

    /// "void write_bool(bool_encoder *e, Prob prob, int bool_value)"
    fn write_bool(&mut self, prob: u8, wert: u32) {
        let split = 1 + (((self.range - 1) * prob as u32) >> 8);
        if wert != 0 {
            self.bott += split;
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bott & (1 << 31) != 0 {
                self.add_one();
            }
            self.bott <<= 1;
            self.bit_count -= 1;
            if self.bit_count == 0 {
                // "*e->output++ = (uint8)(e->bottom >> 24);"
                self.out.push((self.bott >> 24) as u8);
                self.bott &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }

    /// "void flush_bool_encoder" — genau einmal nach der letzten Bool.
    fn flush(&mut self) {
        let mut c = self.bit_count as i32;
        let mut v = self.bott;
        // "if (v & (1 << (32 - c)))". Bei c == 0 kann kein Uebertrag
        // anstehen, und `1 << 32` waere in Rust ein Panik, deshalb
        // die Bedingung.
        if c > 0 && c < 32 && v & (1u32 << (32 - c)) != 0 {
            self.add_one();
        }
        v <<= c & 7;
        c >>= 3;
        while {
            c -= 1;
            c >= 0
        } {
            v <<= 8;
        }
        // "write remaining data, possibly padded" — alle vier Byte sind
        // echt; das letzte ist die Polsterung. Nichts abschneiden.
        for _ in 0..4 {
            self.out.push((v >> 24) as u8);
            v <<= 8;
        }
    }

    /// Die kodierte Partition, gefuellt auf mindestens zwei Byte, damit
    /// der Dekoder seinen Startwert lesen kann.
    fn daten(&self) -> Vec<u8> {
        let mut v = self.out.clone();
        while v.len() < 2 {
            v.push(0);
        }
        v
    }
}

/// Ein fester, aber gemischter Bitstrom (xorshift), damit die Prüfung
/// nicht nur Klassen wie "alle Nullen" sieht.
fn gemischte_bits(n: usize) -> Vec<u32> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 33) & 1) as u32
        })
        .collect()
}

#[test]
fn bool_dekoder_liest_zurueck_was_der_rfc_encoder_geschrieben_hat() {
    for (prob, n) in [(1u8, 60usize), (127, 400), (200, 300), (255, 60)] {
        let erwartet = gemischte_bits(n);
        let mut e = RfcEncoder::new();
        for &b in &erwartet {
            e.write_bool(prob, b);
        }
        e.flush();

        let daten = e.daten();
        let mut d = BoolDekoder::new(&daten, 0).unwrap_or_else(|err| panic!("prob {prob}: {err}"));
        for (i, &b) in erwartet.iter().enumerate() {
            let gelesen = d.flag(prob);
            assert_eq!(
                gelesen, b,
                "prob {prob}: Bit {i} ist {gelesen}, erwartet {b}"
            );
        }
    }
}

#[test]
fn bool_dekoder_schafft_lange_null_und_und_und_einsfolgen() {
    // GEMESSEN 01.10.2026: genau diese beiden Faelle haben den Dekoder
    // mit einem anderen Renormieren sichtbar scheitern lassen. Sie sind
    // darum nicht Beiwerk, sondern die Nagelprobe fuer die Grenzen von
    // `wert` und `weite`.
    for (prob, wert, anzahl) in [
        (255u8, 0u32, 300usize),
        (1, 1, 300),
        (255, 1, 40),
        (1, 0, 40),
    ] {
        let mut e = RfcEncoder::new();
        for _ in 0..anzahl {
            e.write_bool(prob, wert);
        }
        e.flush();
        let daten = e.daten();
        let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
        for i in 0..anzahl {
            assert_eq!(
                d.flag(prob),
                wert,
                "prob {prob}, wert {wert}: Bit {i} falsch"
            );
        }
    }
}

#[test]
fn literal_liest_breite_werte_hoch_nach_niedrig() {
    // "read_literal": "v = (v << 1) + read_bool(d, 128)", jedes Bit bei
    // Gleichstand. Also muessen die Bits in der Reihenfolge hoch nach
    // niedrig ankommen.
    for wert in [0u32, 1, 2, 0b1010_1010, 0b0101_0101, 127, 128, 65_535] {
        let mut e = RfcEncoder::new();
        for i in (0..16).rev() {
            e.write_bool(128, (wert >> i) & 1);
        }
        e.flush();
        let daten = e.daten();
        let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
        assert_eq!(d.literal(16), wert, "literal({wert})");
    }
}

/// Kodiert eine Bitfolge so, wie der Dekoder sie liest: der Baum wird
/// entlang der gewaehlten Bits abgeschritten und an jedem besuchten
/// Knoten die dort gueltige Wahrscheinlichkeit benutzt.
///
/// Ohne das muesste man mit Gleichstand (128) kodieren, waehrend der
/// Dekoder `KF_Y_MODE_PROBS` = 145, 156, 163, 128 liest — der Test
/// pruefte dann eine Uebereinstimmung, die es nicht gibt, und scheitert
/// an der falschen Stelle.
fn kodiere(baum: &[i8], probs: &[u8], bits: &[u32]) -> Vec<u8> {
    let mut e = RfcEncoder::new();
    let mut i: i32 = 0;
    for &b in bits {
        let p = probs[(i >> 1) as usize];
        e.write_bool(p, b);
        let v = baum[(i + b as i32) as usize];
        // v == 0 ist ein ganz normales Blatt: es ist der Wert 0
        // (DC_PRED). Nur ein positiver Eintrag ist ein Knoten.
        i = if v > 0 { v as i32 } else { -1 };
        if i < 0 {
            break;
        }
    }
    e.flush();
    e.daten()
}

#[test]
fn baum_liefert_die_kodierungen_aus_rfc_8_2() {
    // RFC 6386, 8.2 nennt fuer den Keyframe-Baum woertlich:
    //   B_PRED = "0", DC_PRED = "100", V_PRED = "101",
    //   H_PRED = "110", TM_PRED = "111"
    // Der Test kodiert jedes dieser Symbole ueber seinen echten Pfad
    // und laesst den Dekoder es wieder lesen.
    use super::t;
    for (name, wert, kodierung) in [
        ("B_PRED", 4u32, "0"),
        ("DC_PRED", 0, "100"),
        ("V_PRED", 1, "101"),
        ("H_PRED", 2, "110"),
        ("TM_PRED", 3, "111"),
    ] {
        let bits: Vec<u32> = kodierung.chars().map(|c| (c == '1') as u32).collect();
        let daten = kodiere(&t::KF_YMODE_TREE, &t::KF_Y_MODE_PROBS, &bits);
        let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
        let gelesen = y_modus(&mut d).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(gelesen, wert, "{name} = \"{kodierung}\" lieferte {gelesen}");
    }
}

#[test]
fn uv_baum_liefert_alle_vier_modi_und_nicht_den_fuenften() {
    // RFC 6386, 11.4: die Chroma-Modi sind eine echte Teilmenge der
    // Y-Modi. Der Baum hat vier Blaetter, kein fuenftes — ein Symbol 4
    // kann der UV-Modus also nicht liefern.
    use super::t;
    for (name, wert, kodierung) in [
        ("DC_PRED", 0u32, "0"),
        ("V_PRED", 1, "10"),
        ("H_PRED", 2, "110"),
        ("TM_PRED", 3, "111"),
    ] {
        let bits: Vec<u32> = kodierung.chars().map(|c| (c == '1') as u32).collect();
        let daten = kodiere(&t::KF_UVMODE_TREE, &t::KF_UV_MODE_PROBS, &bits);
        let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
        let gelesen = uv_modus(&mut d).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(gelesen, wert, "{name} = \"{kodierung}\" lieferte {gelesen}");
    }
}

#[test]
fn der_baum_kann_nicht_in_eine_schleife_laufen() {
    // M5: ein zyklischer Baum muss abbrechen, nicht endlos laufen.
    let baum: [i8; 2] = [2, 2];
    let probs = [128u8; 8];
    let mut e = RfcEncoder::new();
    for _ in 0..16 {
        e.write_bool(128, 1);
    }
    e.flush();
    let daten = e.daten();
    let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
    let r = d.baum(&baum, &probs);
    assert!(
        r.is_err(),
        "ein zyklischer Baum muss abbrechen, ergab {r:?}"
    );
}

#[test]
fn baum_aus_einer_zu_kurzen_tabelle_bricht_ab() {
    // Ein Blatt, das auf eine Position zeigt, die es nicht gibt, muss
    // ein benannter Fehler sein (M2) — kein Panic, kein stiller Wert.
    // Der 0-Zweig der Wurzel zeigt auf Knoten 4, den es nicht gibt.
    // Deshalb muessen Nullen kodiert werden.
    let baum: [i8; 2] = [4, -1];
    let probs = [128u8; 8];
    let mut e = RfcEncoder::new();
    for _ in 0..16 {
        e.write_bool(128, 0);
    }
    e.flush();
    let daten = e.daten();
    let mut d = BoolDekoder::new(&daten, 0).expect("Dekoder");
    let r = d.baum(&baum, &probs);
    assert!(
        r.is_err(),
        "ein Baum aus einer zu kurzen Tabelle muss abbrechen, ergab {r:?}"
    );
}

#[test]
fn bool_dekoder_verweigert_einen_start_hinter_ende() {
    // M2: kein Panic, kein laengstes Lesen ueber das Feld hinaus.
    let d = [1u8, 2, 3];
    assert!(BoolDekoder::new(&d, 0).is_ok());
    assert!(BoolDekoder::new(&d, 1).is_ok());
    assert!(
        BoolDekoder::new(&d, 2).is_err(),
        "Start 2 braucht zwei Byte"
    );
    assert!(BoolDekoder::new(&d, 3).is_err());
    assert!(BoolDekoder::new(&d, 99).is_err());
}

#[test]
fn rohkopf_liest_die_masse_aus_den_ersten_zehn_bytes() {
    // Ein handgebauter Kopf nach RFC 9.1 und 9.2: Breite 15, Hoehe 300,
    // Keyframe (Bit 0 = 0), Version 2, sichtbar (Bit 4 = 1), erste
    // Partition 20 Byte.
    let breite: u16 = 15;
    let hoehe: u16 = 300;
    let tag: u32 = 0 | (2 << 1) | (1 << 4) | (20 << 5);
    let mut d: Vec<u8> = Vec::new();
    d.push(tag as u8);
    d.push((tag >> 8) as u8);
    d.push((tag >> 16) as u8);
    d.extend_from_slice(&[0x9D, 0x01, 0x2A]);
    // Bytes 6,7: Breite in den unteren 14 Bit, Skala oben.
    d.extend_from_slice(&(breite | (1 << 14)).to_le_bytes());
    d.extend_from_slice(&(hoehe | (3 << 14)).to_le_bytes());
    d.extend_from_slice(&[0u8; 30]);

    let k = rohkopf_lesen(&d).expect("Kopf");
    assert_eq!(k.breite, 15, "Breite");
    assert_eq!(k.hoehe, 300, "Hoehe");
    assert_eq!(k.horiz_skala, 1, "horizontale Skala");
    assert_eq!(k.vert_skala, 3, "vertikale Skala");
    assert_eq!(k.erste_part, 20, "erste Partition");
    assert_eq!(
        k.makrobloecke,
        1 * 19,
        "15 breit braucht 1 Spalte, 300 hoch braucht 19 Zeilen (19 * 16 = 304)"
    );
}

#[test]
fn rohkopf_weist_scheissige_ingaenge_ab() {
    // M2/M8: jeder Fehler traegt einen Namen, kein Absturz, kein leeres Bild.
    assert!(rohkopf_lesen(&[]).is_err());
    assert!(rohkopf_lesen(&[0u8; 9]).is_err(), "zehn Byte sind noetig");

    // Interframe: Bit 0 des Rahmentags ist 1. Der Tag wird LSB zuerst
    // gelesen, also sitzt Bit 0 im ERSTEN Byte.
    //
    // B13 (02.10.2026): das war bis eben der BENANNTE Fehler
    // "VP8-Interframe ist nicht implementiert", und der Test hier
    // behauptete genau das. Jetzt wird die Art GEMELDET statt
    // abgewiesen — `Rohkopf::interframe` sagt sie, und der Aufrufer
    // nimmt den passenden Weg.
    //
    // GEMESSEN an `g7b_testsrc.webm`: Bild 1 beginnt mit `b1 0a 00 0b
    // 10 ac` — Bit 0 des Tags ist 1, und die Sync-Bytes an Position 3
    // sind `0b 10 ac` statt `9d 01 2a`. Deshalb wird der Sync-Code nur
    // noch bei einem SCHLUESSELbild geprueft.
    let d: Vec<u8> = vec![0x11, 0, 0, 0x0b, 0x10, 0xac, 15, 0, 15, 0, 0, 0];
    let r = rohkopf_lesen(&d).expect("Interframe wird jetzt gelesen");
    assert!(r.interframe, "Bit 0 des Rahmentags ist 1");

    // Und ein Schluesselbild mit falschem Sync wird weiter abgewiesen.
    let d2: Vec<u8> = vec![0x10, 0, 0, 0x9D, 0x01, 0x2B, 15, 0, 15, 0, 0, 0];
    let e2 = rohkopf_lesen(&d2).expect_err("falsches Sync muss abgewiesen werden");
    assert!(format!("{e2}").contains("Sync"), "Meldung: {e2}");

    // Version 5. GEMESSEN 01.10.2026: das Versionsfeld ist nach
    // RFC 9.1 genau drei Bit breit, eine Version 9 laesst sich gar nicht
    // eintragen — sie wuerde als 1 gelesen. Abgewiesen wird 4..7.
    let tag3: u32 = (1 << 4) | (5 << 1);
    let d3: Vec<u8> = vec![
        tag3 as u8,
        (tag3 >> 8) as u8,
        (tag3 >> 16) as u8,
        0x9D,
        0x01,
        0x2A,
        15,
        0,
        15,
        0,
        0,
        0,
    ];
    let e3 = rohkopf_lesen(&d3).expect_err("Version 9 muss abgewiesen werden");
    assert!(format!("{e3}").contains("Version"), "Meldung: {e3}");

    // Breite 0.
    let d4: Vec<u8> = vec![0x10, 0, 0, 0x9D, 0x01, 0x2A, 0, 0, 15, 0, 0, 0];
    let e4 = rohkopf_lesen(&d4).expect_err("Breite 0 muss abgewiesen werden");
    assert!(format!("{e4}").contains("ohne Masse"), "Meldung: {e4}");

    // Als nicht sichtbar markiert (Bit 4 = 0).
    let d5: Vec<u8> = vec![0, 0, 0, 0x9D, 0x01, 0x2A, 15, 0, 15, 0, 0, 0];
    let e5 = rohkopf_lesen(&d5).expect_err("nicht sichtbar muss abgewiesen werden");
    assert!(format!("{e5}").contains("sichtbar"), "Meldung: {e5}");

    // Erste Partition hinter dem Chunk.
    // Rahmentag: Keyframe (Bit 0 = 0), sichtbar (Bit 4 = 1), erste
    // Partition 900 Byte bei nur 10 Byte Nutzlast.
    let tag6: u32 = (1 << 4) | (900 << 5);
    let d6: Vec<u8> = vec![
        tag6 as u8,
        (tag6 >> 8) as u8,
        (tag6 >> 16) as u8,
        0x9D,
        0x01,
        0x2A,
        15,
        0,
        15,
        0,
    ];
    let e6 = rohkopf_lesen(&d6).expect_err("Partition hinter dem Ende muss abgewiesen werden");
    assert!(
        format!("{e6}").contains("hinter dem Chunk"),
        "Meldung: {e6}"
    );
}
