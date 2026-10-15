//! MD5 (RFC 1321) — fuer die Signatur im FLAC-STREAMINFO.
//!
//! # Wofuer das hier ist
//!
//! FLAC schreibt am Ende des STREAMINFO 16 Byte, die ueber den
//! **unkomprimierten** Inhalt gebildet sind: der interleavte Bytestrom,
//! jeder Wert in seiner vollen Bittiefe, little endian. Damit kann ein
//! Leser beim Dekodieren selbst nachrechnen, ob die Datei verlustfrei ist
//! — ohne den Originalton zu brauchen.
//!
//! Das ist kein Bonus. Es ist der Grund, warum FLAC ueberhaupt die
//! Zusicherung „verlustfrei" verdient: der Enkoder muss beim Schreiben
//! denselben Hash bilden wie der Leser beim Lesen. Ein Verdaechtigen
//! wuerde mein Programm in der Klammer bemerken.
//!
//! # Warum kein Krypto-Anspruch
//!
//! MD5 ist seit 2004 als kryptografisch gebrochen bekannt (kollisionsfaehig).
//! Hier ist das ohne Bedeutung: es geht darum, dass **derselbe Algorithmus
//! auf beiden Seiten laeuft**, nicht darum, dass niemand eine Datei findet,
//! die diese 16 Byte auch nur annaehernd nachbildet. Es wird nichts
//! authentifiziert, nichts verschluesselt, nichts uebertragen, dem man
//! nicht schon traut, indem man es gerade entschluesselt hat.
//!
//! # Geprueft
//!
//! gegen die drei Testvektoren aus RFC 1321 (`src/md5.rs`, `md5_testvektoren`).

/// Der MD5-Zustand: vier 32-Bit-Werte.
#[derive(Clone)]
pub struct Md5 {
    a0: u32,
    b0: u32,
    c0: u32,
    d0: u32,
    puffer: [u8; 64],
    puffer_len: usize,
    gesamt_bits: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::neu()
    }
}

/// Die runden Klassen aus RFC 1321. Der Wert ist die Drehung
/// `links(k) + Konstante`, der Index die Position im Block.
const K: [u32; 64] = [
    // Runde 1
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    // Runde 2
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    // Runde 3
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    // Runde 4
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// Die Bitverschiebungen aus RFC 1321.
const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, //
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, //
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, //
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

impl Md5 {
    /// Ein leerer Hash.
    #[must_use]
    pub const fn neu() -> Self {
        Md5 {
            a0: 0x6745_2301,
            b0: 0xefcd_ab89,
            c0: 0x98ba_dcfe,
            d0: 0x1032_5476,
            puffer: [0; 64],
            puffer_len: 0,
            gesamt_bits: 0,
        }
    }

    /// Fuettert Bytes.
    pub fn fuehre(&mut self, mut daten: &[u8]) {
        self.gesamt_bits = self.gesamt_bits.wrapping_add((daten.len() as u64) * 8);
        // Fuellstueck aus dem Puffer, falls vorhanden.
        if self.puffer_len > 0 {
            let nahm = (64 - self.puffer_len).min(daten.len());
            self.puffer[self.puffer_len..self.puffer_len + nahm].copy_from_slice(&daten[..nahm]);
            self.puffer_len += nahm;
            daten = &daten[nahm..];
            if self.puffer_len == 64 {
                let block = self.puffer;
                self.block(&block);
                self.puffer_len = 0;
            }
        }
        let mut rest = daten;
        while rest.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&rest[..64]);
            self.block(&block);
            rest = &rest[64..];
        }
        if !rest.is_empty() {
            self.puffer[..rest.len()].copy_from_slice(rest);
            self.puffer_len = rest.len();
        }
    }

    /// Fuettert ein 32-Bit-Wert little endian (die Bytereihenfolge, in der
    /// FLAC die Proben hasht).
    pub fn fuehre_u32_le(&mut self, wert: u32) {
        self.fuehre(&wert.to_le_bytes());
    }

    /// Fuettert einen 32-Bit-Wert, von dem nur die unteren `bit_tiefe`
    /// Bit gebraucht werden (FLAC hasht in voller Bittiefe).
    pub fn fuehre_bittiefe(&mut self, wert: i32, bit_tiefe: u8) {
        debug_assert!((4..=32).contains(&bit_tiefe));
        if bit_tiefe == 32 {
            self.fuehre(&wert.to_le_bytes());
        } else {
            // Negatives Zwei-Komplement ueber die Nutzbits hinaus ausdehnen,
            // damit -1 bei 24 Bit als FF FF FF FF gehasht wird.
            let schub = 32 - i32::from(bit_tiefe);
            let v = (wert << schub) >> schub;
            self.fuehre(&v.to_le_bytes());
        }
    }

    /// Der fertige Hash als 16 Byte (little endian, A B C D).
    #[must_use]
    pub fn abschluss(mut self) -> [u8; 16] {
        let bits = self.gesamt_bits;
        // 0x80, dann Nullen, dann die Laenge in Bit als 8 Byte LE.
        self.fuehre(&[0x80]);
        // `fuehre` hat die Bitzahl schon veraendert — wir brauchen die alte.
        while self.puffer_len != 56 {
            self.fuehre(&[0]);
        }
        self.gesamt_bits = bits; // irrelevant, aber korrekt halten
        let laenge = bits.to_le_bytes();
        self.puffer[56..64].copy_from_slice(&laenge);
        let block = self.puffer;
        self.block(&block);
        let mut raus = [0u8; 16];
        raus[0..4].copy_from_slice(&self.a0.to_le_bytes());
        raus[4..8].copy_from_slice(&self.b0.to_le_bytes());
        raus[8..12].copy_from_slice(&self.c0.to_le_bytes());
        raus[12..16].copy_from_slice(&self.d0.to_le_bytes());
        raus
    }

    /// Eine Runde ueber 64 Byte.
    fn block(&mut self, block: &[u8; 64]) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (self.a0, self.b0, self.c0, self.d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = temp;
        }
        self.a0 = self.a0.wrapping_add(a);
        self.b0 = self.b0.wrapping_add(b);
        self.c0 = self.c0.wrapping_add(c);
        self.d0 = self.d0.wrapping_add(d);
    }
}

/// Der MD5 einer Bytefolge (die haeufigste Benutzung).
#[must_use]
pub fn md5(daten: &[u8]) -> [u8; 16] {
    let mut h = Md5::neu();
    h.fuehre(daten);
    h.abschluss()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die drei Testvektoren aus RFC 1321, Abschnitt „Test suite".
    ///
    /// Das ist der Grund, warum hier kein „sollte stimmen" steht: MD5 ist
    /// ein festes Verfahren, und ein Verfahren, das man nicht gegen den
    /// Normtext prueft, ist eine Behauptung.
    #[test]
    fn md5_testvektoren() {
        let faelle: [(&str, &str); 7] = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (eingabe, soll) in faelle {
            let h = md5(eingabe.as_bytes());
            let text: String = h.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(text, soll, "MD5 von {eingabe:?}");
        }
    }

    /// Zerteilte Zufuehrung muss dasselbe ergeben wie am Stueck — der
    /// Pufferwechsel in `fuehre` ist die Stelle, an der das schiefgeht.
    #[test]
    fn md5_zerteilte_zufuehrung_ist_gleich() {
        let daten: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let ganz = md5(&daten);
        for stueck in [1usize, 7, 63, 64, 65, 128, 199] {
            let mut h = Md5::neu();
            for teil in daten.chunks(stueck) {
                h.fuehre(teil);
            }
            assert_eq!(h.abschluss(), ganz, "Stueckgroesse {stueck}");
        }
    }

    /// Bittiefe: -1 bei 24 Bit muss als viermal FF gehasht werden, weil
    /// FLAC in voller Bittiefe hasht.
    #[test]
    fn md5_bittiefe_streckt_das_vorzeichen() {
        let mut h = Md5::neu();
        h.fuehre_bittiefe(-1, 24);
        let mut erwartet = Md5::neu();
        erwartet.fuehre(&(-1i32).to_le_bytes());
        assert_eq!(h.abschluss(), erwartet.abschluss());
    }
}
