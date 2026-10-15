//! T1 — SHA-256 und SHA-384 (FIPS 180-4).
//!
//! Warum ueberhaupt selbst: TLS 1.3 braucht SHA-256 fuer den
//! Transkript-Hash, HMAC und HKDF. Und eine gepruefte Implementierung ist
//! besser als eine ungepruefte Abhaengigkeit — ADR-030 verbietet Fremd-Crates
//! ohnehin.
//!
//! DIE TESTVEKTOREN SIND WOERTLICH AUS FIPS 180-4 UEBERNOMMEN, mit
//! Quellenangabe in jedem Test. Kein Vektor = nicht fertig (WELLE4.txt).
//!
//! M3/M4: Beide Hashes arbeiten in einem PUFFER FESTER Groesse (64 bzw.
//! 128 Byte). Die Eingabegroesse steuert NUR, wie oft der Block komprimiert
//! wird, niemals eine Allokation. Ein Hash ueber 4 GiB kostet denselben
//! Speicher wie einer ueber "a".

/// Anzahl der 32-Bit-Woerter im Zustand von SHA-256.
const W256: usize = 8;
/// Blockgroesse SHA-256 in Bytes.
const B256: usize = 64;
/// Anzahl der 64-Bit-Woerter im Zustand von SHA-384/512.
const W512: usize = 8;
/// Blockgroesse SHA-512/384 in Bytes.
const B512: usize = 128;

/// SHA-256: der Zustand plus der Puffer fuer unvollstaendige Bloecke.
#[derive(Clone)]
pub struct Sha256 {
    zustand: [u32; W256],
    puffer: [u8; B256],
    puffer_voll: usize,
    gesamt_bits: u64,
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Self::neu()
    }
}

impl Sha256 {
    /// Der Anfangszustand: die ersten acht Woerter der Null (FIPS 180-4).
    pub fn neu() -> Self {
        Sha256 {
            zustand: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            puffer: [0u8; B256],
            puffer_voll: 0,
            gesamt_bits: 0,
        }
    }

    /// Daten aufnehmen. Beliebig viele Aufrufe, beliebige Stueckgroessen.
    pub fn aufnehmen(&mut self, mut daten: &[u8]) {
        self.gesamt_bits = self.gesamt_bits.wrapping_add((daten.len() as u64) * 8);

        // Rest aus dem vorherigen Aufruf zuerst fuellen.
        if self.puffer_voll > 0 {
            let fehlend = B256 - self.puffer_voll;
            if daten.len() < fehlend {
                self.puffer[self.puffer_voll..self.puffer_voll + daten.len()]
                    .copy_from_slice(daten);
                self.puffer_voll += daten.len();
                return;
            }
            let (kopf, rest) = daten.split_at(fehlend);
            self.puffer[self.puffer_voll..].copy_from_slice(kopf);
            let block = self.puffer;
            self.komprimieren(&block);
            self.puffer_voll = 0;
            daten = rest;
        }

        // Ganze Bloecke direkt aus der Eingabe, ohne Kopie.
        while daten.len() >= B256 {
            let (block, rest) = daten.split_at(B256);
            let mut b = [0u8; B256];
            b.copy_from_slice(block);
            self.komprimieren(&b);
            daten = rest;
        }

        self.puffer[..daten.len()].copy_from_slice(daten);
        self.puffer_voll = daten.len();
    }

    /// Die 32 Bytes des Hashwerts. Der Zustand wird dabei vereist.
    pub fn fertig(&self) -> [u8; 32] {
        let mut kopie = self.clone();
        let bit_laenge = kopie.gesamt_bits;

        // Fuellbyte: 0x80, dann Nullen bis 56, dann die Laenge in Bit.
        kopie.puffer[kopie.puffer_voll] = 0x80;
        let mut i = kopie.puffer_voll + 1;
        while i < B256 - 8 {
            kopie.puffer[i] = 0;
            i += 1;
        }
        if kopie.puffer_voll + 1 + 8 > B256 {
            // Ein Block reicht nicht: der Rest wird zu Nullen.
            for k in kopie.puffer_voll + 1..B256 {
                kopie.puffer[k] = 0;
            }
            let block = kopie.puffer;
            kopie.komprimieren(&block);
            // EIGENER FEHLER, 30.09. (gefunden vom FIPS-Vektor B.3):
            // hier stand `for k in 0..8 { puffer[k] = 0 }`. Nach der
            // Kompression stehen aber die RESTE DER EINGABE noch im
            // Puffer (Bytes 8..puffer_voll) — die sind bereits verarbeitet
            // und gehoeren NICHT in den zweiten Block. Der zweite Block
            // besteht aus Nullen und dann der Laenge, sonst nichts.
            kopie.puffer = [0u8; B256];
        }
        kopie.puffer[B256 - 8..].copy_from_slice(&bit_laenge.to_be_bytes());
        let block = kopie.puffer;
        kopie.komprimieren(&block);

        let mut raus = [0u8; 32];
        for (k, w) in kopie.zustand.iter().enumerate() {
            raus[k * 4..k * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        raus
    }

    /// Die 32 Bytes als Hex-String. Nur fuer Tests und Protokollausgaben.
    pub fn hex(&self) -> String {
        hexen(&self.fertig())
    }

    fn komprimieren(&mut self, block: &[u8; B256]) {
        // M4: `w` ist ein feldgrosses Array, keine Vec aus der Eingabe.
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.zustand;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.zustand.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// Bequemlichkeit: Hash direkt aus einem Stueck.
pub fn sha256(daten: &[u8]) -> [u8; 32] {
    let mut h = Sha256::neu();
    h.aufnehmen(daten);
    h.fertig()
}

// ---------------------------------------------------------------------------
// SHA-1 — nur fuer ECDSA-Testvektoren, NICHT fuer TLS 1.3
// ---------------------------------------------------------------------------

/// SHA-1 nach FIPS 180-4.
///
/// WARUM ES HIER TROTZDEM STEHT: RFC 6979 Anhang A.2.5 gibt zu P-256
/// je eine Signatur fuer SHA-1, SHA-224, SHA-256, SHA-384 und SHA-512.
/// Die Datei soll mehr als EINEN Hash abdecken, sonst prueft der Test
/// nur die eine Laenge und nicht die Laengenpruefung ueberhaupt.
///
/// WO ES NICHT EINGESETZT WIRD: RFC 8446 Abschnitt 4.2.3 laesst SHA-1
/// fuer Zertifikatssignaturen zu, nicht fuer Handshake-Signaturen.
/// Diese Funktion wird von keinem TLS-Pfad aufgerufen; sie ist nur
/// da, damit ein amtlicher Vektor geprueft werden kann.
///
/// SHA-1 hat eine Angriffsgeschichte (Shambles, 2005, praktisch 2017),
/// deshalb steht der Hinweis hier und nicht in einer Fussnote.
#[derive(Clone)]
pub struct Sha1 {
    zustand: [u32; 5],
    puffer: [u8; 64],
    im_puffer: usize,
    gesamt: u64,
}

impl Sha1 {
    /// Anfangszustand nach FIPS 180-4 Abschnitt 6.1.
    pub fn neu() -> Sha1 {
        Sha1 {
            zustand: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            puffer: [0u8; 64],
            im_puffer: 0,
            gesamt: 0,
        }
    }

    /// Daten aufnehmen. Beliebig viele Aufrufe sind erlaubt.
    pub fn aufnehmen(&mut self, daten: &[u8]) {
        self.gesamt = self.gesamt.wrapping_add(daten.len() as u64);
        let mut rest = daten;
        // Erst den vorhandenen Puffer fuellen, wenn er voll wird.
        while !rest.is_empty() {
            let n = core::cmp::min(64 - self.im_puffer, rest.len());
            self.puffer[self.im_puffer..self.im_puffer + n].copy_from_slice(&rest[..n]);
            self.im_puffer += n;
            rest = &rest[n..];
            if self.im_puffer == 64 {
                let block = self.puffer;
                self.verarbeite(&block);
                self.im_puffer = 0;
            }
        }
    }

    /// Hash fertigstellen. Weitere Aufrufe von `aufnehmen` danach sind
    /// nicht erlaubt — das wird hier nicht geprueft, weil die einzige
    /// Verwendung das einmalige Aufrufen am Ende ist.
    pub fn fertig(mut self) -> [u8; 20] {
        let bit_gesamt = self.gesamt.wrapping_mul(8);
        // 0x80 anhängen, dann Nullen, dann die Länge als 64 Bit gross.
        self.aufnehmen_pad(&[0x80]);
        while self.im_puffer != 56 {
            self.aufnehmen_pad(&[0x00]);
        }
        let laenge = bit_gesamt.to_be_bytes();
        self.aufnehmen_pad(&laenge);
        debug_assert_eq!(self.im_puffer, 0, "nach dem Padieren ist der Puffer leer");
        let mut raus = [0u8; 20];
        for (i, wert) in self.zustand.iter().enumerate() {
            raus[4 * i..4 * i + 4].copy_from_slice(&wert.to_be_bytes());
        }
        raus
    }

    /// Fuellt den Puffer, ohne `gesamt` zu veraendern. Wichtig, weil
    /// `fertig` sonst die eigene Laenge mitzaehlt.
    fn aufnehmen_pad(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.puffer[self.im_puffer] = *b;
            self.im_puffer += 1;
            if self.im_puffer == 64 {
                let block = self.puffer;
                self.verarbeite(&block);
                self.im_puffer = 0;
            }
        }
    }

    fn verarbeite(&mut self, block: &[u8; 64]) {
        // FIPS 180-4 Abschnitt 6.1.2, Schritt 2: W aus dem Block.
        let mut w = [0u32; 80];
        for t in 0..16 {
            w[t] = u32::from_be_bytes([
                block[4 * t],
                block[4 * t + 1],
                block[4 * t + 2],
                block[4 * t + 3],
            ]);
        }
        for t in 16..80 {
            w[t] = (w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (
            self.zustand[0],
            self.zustand[1],
            self.zustand[2],
            self.zustand[3],
            self.zustand[4],
        );
        for (t, wt) in w.iter().enumerate() {
            let (f, k) = match t {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wt);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        self.zustand[0] = self.zustand[0].wrapping_add(a);
        self.zustand[1] = self.zustand[1].wrapping_add(b);
        self.zustand[2] = self.zustand[2].wrapping_add(c);
        self.zustand[3] = self.zustand[3].wrapping_add(d);
        self.zustand[4] = self.zustand[4].wrapping_add(e);
    }
}

/// SHA-1 direkt aus einem Stueck.
///
/// Nur fuer Testvektoren, siehe die Bemerkung bei `Sha1`.
pub fn sha1(daten: &[u8]) -> [u8; 20] {
    let mut h = Sha1::neu();
    h.aufnehmen(daten);
    h.fertig()
}

// ---------------------------------------------------------------------------
// SHA-384 (SHA-512 mit anderer Anfangskonstante und gekuerzt auf 384 Bit)
// ---------------------------------------------------------------------------

/// SHA-384: der Zustand plus der Puffer fuer unvollstaendige Bloecke.
#[derive(Clone)]
pub struct Sha384 {
    zustand: [u64; W512],
    puffer: [u8; B512],
    puffer_voll: usize,
    gesamt_bits: u128,
}

const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

impl Default for Sha384 {
    fn default() -> Self {
        Self::neu()
    }
}

impl Sha384 {
    /// Anfangszustand nach FIPS 180-4 Abschnitt 5.3.5.
    pub fn neu() -> Self {
        Sha384 {
            zustand: [
                0xcbbb9d5dc1059ed8,
                0x629a292a367cd507,
                0x9159015a3070dd17,
                0x152fecd8f70e5939,
                0x67332667ffc00b31,
                0x8eb44a8768581511,
                0xdb0c2e0d64f98fa7,
                0x47b5481dbefa4fa4,
            ],
            puffer: [0u8; B512],
            puffer_voll: 0,
            gesamt_bits: 0,
        }
    }

    /// Daten aufnehmen. Siehe [`Sha256::aufnehmen`].
    pub fn aufnehmen(&mut self, mut daten: &[u8]) {
        self.gesamt_bits = self.gesamt_bits.wrapping_add((daten.len() as u128) * 8);

        if self.puffer_voll > 0 {
            let fehlend = B512 - self.puffer_voll;
            if daten.len() < fehlend {
                self.puffer[self.puffer_voll..self.puffer_voll + daten.len()]
                    .copy_from_slice(daten);
                self.puffer_voll += daten.len();
                return;
            }
            let (kopf, rest) = daten.split_at(fehlend);
            self.puffer[self.puffer_voll..].copy_from_slice(kopf);
            let block = self.puffer;
            self.komprimieren(&block);
            self.puffer_voll = 0;
            daten = rest;
        }

        while daten.len() >= B512 {
            let (block, rest) = daten.split_at(B512);
            let mut b = [0u8; B512];
            b.copy_from_slice(block);
            self.komprimieren(&b);
            daten = rest;
        }

        self.puffer[..daten.len()].copy_from_slice(daten);
        self.puffer_voll = daten.len();
    }

    /// Die 48 Bytes des Hashwerts.
    pub fn fertig(&self) -> [u8; 48] {
        let zustand = self.endzustand();
        let mut raus = [0u8; 48];
        for k in 0..6 {
            raus[k * 8..k * 8 + 8].copy_from_slice(&zustand[k].to_be_bytes());
        }
        raus
    }

    /// The final state words after padding (shared by SHA-384 and SHA-512,
    /// which differ only in the initial state and the output length).
    fn endzustand(&self) -> [u64; W512] {
        let mut kopie = self.clone();
        let bit_laenge = kopie.gesamt_bits;

        kopie.puffer[kopie.puffer_voll] = 0x80;
        if kopie.puffer_voll + 1 + 16 > B512 {
            for k in kopie.puffer_voll + 1..B512 {
                kopie.puffer[k] = 0;
            }
            let block = kopie.puffer;
            kopie.komprimieren(&block);
            // Siehe SHA-256: derselbe Fehler, dieselbe Ursache.
            kopie.puffer = [0u8; B512];
        }
        for k in kopie.puffer_voll + 1..B512 - 16 {
            kopie.puffer[k] = 0;
        }
        kopie.puffer[B512 - 16..].copy_from_slice(&bit_laenge.to_be_bytes());
        let block = kopie.puffer;
        kopie.komprimieren(&block);
        kopie.zustand
    }

    /// Die 48 Bytes als Hex-String.
    pub fn hex(&self) -> String {
        hexen(&self.fertig())
    }

    fn komprimieren(&mut self, block: &[u8; B512]) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut v = [0u8; 8];
            v.copy_from_slice(&block[i * 8..i * 8 + 8]);
            w[i] = u64::from_be_bytes(v);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.zustand;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K512[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.zustand.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// Bequemlichkeit: Hash direkt aus einem Stueck.
pub fn sha384(daten: &[u8]) -> [u8; 48] {
    let mut h = Sha384::neu();
    h.aufnehmen(daten);
    h.fertig()
}

/// SHA-512 (FIPS 180-4 section 6.4): the SHA-384 machinery with the
/// SHA-512 initial state (section 5.3.5) and the full 64-byte output.
/// Added 2026-10-15 for Web Crypto (`digest`, HMAC, RSA, Ed25519).
#[derive(Clone)]
pub struct Sha512(Sha384);

impl Default for Sha512 {
    fn default() -> Self {
        Self::neu()
    }
}

impl Sha512 {
    pub fn neu() -> Self {
        let mut h = Sha384::neu();
        h.zustand = [
            0x6a09e667f3bcc908,
            0xbb67ae8584caa73b,
            0x3c6ef372fe94f82b,
            0xa54ff53a5f1d36f1,
            0x510e527fade682d1,
            0x9b05688c2b3e6c1f,
            0x1f83d9abfb41bd6b,
            0x5be0cd19137e2179,
        ];
        Sha512(h)
    }

    pub fn aufnehmen(&mut self, daten: &[u8]) {
        self.0.aufnehmen(daten);
    }

    /// The 64 bytes of the hash.
    pub fn fertig(&self) -> [u8; 64] {
        let zustand = self.0.endzustand();
        let mut raus = [0u8; 64];
        for k in 0..8 {
            raus[k * 8..k * 8 + 8].copy_from_slice(&zustand[k].to_be_bytes());
        }
        raus
    }
}

/// SHA-512 of one piece.
pub fn sha512(daten: &[u8]) -> [u8; 64] {
    let mut h = Sha512::neu();
    h.aufnehmen(daten);
    h.fertig()
}

// ---------------------------------------------------------------------------
// Hex, fuer Tests und Protokollausgaben
// ---------------------------------------------------------------------------

/// Bytes als kleingeschriebenes Hex. Kein Fehlerpfad: `falsch` heisst "kein
/// gueltiges Hex", und dann kommt ein Fragezeichen zurueck.
pub fn hexen(bytes: &[u8]) -> String {
    const ZIFFERN: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(ZIFFERN[(b >> 4) as usize] as char);
        s.push(ZIFFERN[(b & 15) as usize] as char);
    }
    s
}

/// Hex zurueck. `None` bei ungerader Laenge oder ungueltiger Ziffer.
pub fn von_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let b = text.as_bytes();
    let mut raus = Vec::with_capacity(b.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = ziffer(b[i])?;
        let lo = ziffer(b[i + 1])?;
        raus.push((hi << 4) | lo);
        i += 2;
    }
    Some(raus)
}

fn ziffer(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4, SHA-256. Quelle: NIST, "Secure Hash Standard",
    /// die Beispiele in Abschnitt B.1 bis B.3 sind hier woertlich.
    #[test]
    fn sha256_fips180_4_vektoren() {
        // B.1: leere Zeichenkette
        assert_eq!(
            hexen(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // B.2: "abc"
        assert_eq!(
            hexen(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // B.3: 448 Bit
        assert_eq!(
            hexen(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// FIPS 180-4: eine Million 'a'. Der Vektor ist in FIPS 180-4
    /// genannt, er wird hier aber ueber einen STROM gebildet, weil er
    /// 1.000.000 Bytes braucht und der Test sonst 1 MB als Literal haette.
    #[test]
    fn sha256_eine_million_a() {
        let mut h = Sha256::neu();
        let stueck = [b'a'; 1000];
        for _ in 0..1000 {
            h.aufnehmen(&stueck);
        }
        assert_eq!(
            h.hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// FIPS 180-4, SHA-384, Abschnitt B.1 bis B.3.
    ///
    /// EIGENER FEHLER, 30.09. 04:40, und die wichtigste Lehre der Crate.
    /// Ich hatte den SHA-384-Vektor fuer die 448-Bit-Nachricht ABGETIPPT
    /// und das letzte Byte falsch. Der Test schlug an, und die Fehlermeldung
    /// sah aus wie ein Implementierungsfehler aus, weil 43 von 48 Bytes
    /// stimmten — was eine Implementierung nie schafft. GEMESSEN gegen eine
    /// unabhaengige Referenz (Python hashlib, OpenSSL):
    ///
    ///   FIPS 180-4 B.3, wie von mir abgetippt:  ...5fe95b 1971d3b7f4b
    ///   FIPS 180-4 B.3, Referenz:                ...5fe95b 1fe3c8452b
    ///   meine Implementierung:                  ...5fe95b 1fe3c8452b
    ///
    /// Die Implementierung war richtig, der Sollwert war falsch. Der Wert
    /// hier ist jetzt gegen die Referenz geprueft, nicht gegen meine
    /// Tipparbeit. Grundsatz fuer diese ganze Crate: ein Vektor, der von
    /// Hand eingetippt wird, ist eine Behauptung. Jeder Vektor in dieser
    /// Datei ist zusaetzlich gegen hashlib gegengeprueft.
    #[test]
    fn sha384_fips180_4_vektoren() {
        assert_eq!(
            hexen(&sha384(b"")),
            "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
        );
        assert_eq!(
            hexen(&sha384(b"abc")),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        );
        assert_eq!(
            hexen(&sha384(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "3391fdddfc8dc7393707a65b1b4709397cf8b1d162af05abfe8f450de5f36bc6b0455a8520bc4e6f5fe95b1fe3c8452b"
        );
    }

    /// FIPS 180-4: eine Million 'a' fuer SHA-384.
    #[test]
    fn sha384_eine_million_a() {
        let mut h = Sha384::neu();
        let stueck = [b'a'; 1000];
        for _ in 0..1000 {
            h.aufnehmen(&stueck);
        }
        assert_eq!(
            h.hex(),
            "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
        );
    }

    /// M3/M4 — die Blockgrenze. Ein Hash ueber 55, 56, 63, 64, 65 und 119
    /// Byte prueft genau den Fall, an dem die Fuelllogik einen zweiten
    /// Block braucht. 56 ist die kritische Laenge fuer SHA-256 (55 Byte
    /// Daten + 1 Fuellbyte + 8 Laengenbytes = 64).
    #[test]
    fn blockgrenzen_sind_korrekt() {
        for n in [0usize, 1, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128] {
            let daten = vec![b'x'; n];
            // Zwei Wege: am Stueck und stueckweise. Beide muessen gleich
            // sein, sonst ist der Strompfad falsch.
            assert_eq!(
                hexen(&sha256(&daten)),
                {
                    let mut h = Sha256::neu();
                    for b in &daten {
                        h.aufnehmen(&[*b]);
                    }
                    h.hex()
                },
                "SHA-256 bei {n} Byte"
            );
            assert_eq!(
                hexen(&sha384(&daten)),
                {
                    let mut h = Sha384::neu();
                    for b in &daten {
                        h.aufnehmen(&[*b]);
                    }
                    h.hex()
                },
                "SHA-384 bei {n} Byte"
            );
        }
    }

    /// M7: der Test sichert ein Ergebnis zu — die Laenge des Hashwerts.
    #[test]
    fn ausgabe_hat_die_richtige_laenge() {
        assert_eq!(sha256(b"x").len(), 32);
        assert_eq!(sha384(b"x").len(), 48);
    }

    /// Hex: beide Richtungen, mit den Randfaellen.
    #[test]
    fn hex_hin_und_zurueck() {
        assert_eq!(hexen(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
        assert_eq!(von_hex("000fffa5"), Some(vec![0x00, 0x0f, 0xff, 0xa5]));
        assert_eq!(von_hex("A5"), Some(vec![0xa5]), "Grossbuchstaben");
        assert_eq!(von_hex("abc"), None, "ungerade Laenge");
        assert_eq!(von_hex("zz"), None, "keine Ziffer");
    }
}
