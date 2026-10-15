//! T5 — AES-128 und AES-256 (FIPS 197) und GCM (NIST SP 800-38D).
//!
//! Warum AES trotz ChaCha: TLS 1.3 muss beide Cipher Suites anbieten
//! koennen, weil die meisten Server heute nur AES-GCM sprechen. Ein
//! Browser, der nur ChaCha kann, erreicht einen grossen Teil des Webs
//! nicht.
//!
//! M6 — DIE ENTSCHEIDUNG, die hier wichtig ist: die S-Boxen werden als
//! KONSTANTE berechnet, nicht als Tabelle mit 4 KiB im Speicher.
//! poly1305.rs und sha2.rs sind tabellenfrei; AES bleibt es auch.
//!
//! M4: Schluessel und Laengen sind fest. `&[u8; 16]` und `&[u8; 32]`
//! erzwingen die Laenge im Typ.

// SubBytes: die S-Box als KONSTANTE aus FIPS 197, Tabelle 1.
//
// WARUM ALS TABELLE UND NICHT BERECHNET: Ich hatte sie zuerst aus dem
// mathematischen Definitionstext berechnet (Inverse im GF(2^8), dann die
// affine Abbildung) und die Rechnung war an drei Stellen falsch — beim
// Exponenten fuer das Inverse. GEMESSEN: meine Formel lieferte fuer die
// Eingabe 0x02 den Wert 0x63 statt 0x77, also war die S-Box schon an der
// dritten Stelle falsch und der AES-Block kam nicht heraus.
//
// Ein Rechner fuer eine S-Box ist 20 Zeilen und spart 256 Byte. Er ist
// aber nur dann besser, wenn er BEWEISEN richtig ist, und ein Beweis ist
// hier die Tabelle aus der Norm plus die Testvektoren aus Anhang C.
// Ein falsch gerechneter Wert, der aussieht wie ein richtiger, ist die
// teuerste Variante von beiden. Deshalb: die Tabelle, und die Tests aus
// FIPS 197 Anhang C als Beweis.
//
// M6: das ist eine KONSTANTE im Programmtext, kein globaler Zustand, der
// Speicher belegt. Sie wird nicht alloziert und nicht entladen.
const S_BOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// SubBytes auf einen Block anwenden.
fn sub_bytes_ein(block: &mut [u8; 16]) {
    for b in block.iter_mut() {
        *b = S_BOX[*b as usize];
    }
}

/// Multiplikation im GF(2^8) mit dem AES-Polynom 0x11b.
const fn gmul(a: u8, b: u8) -> u8 {
    let mut result = 0u8;
    let mut a = a;
    let mut b = b;
    let mut i = 0;
    while i < 8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let hoch = a & 0x80;
        a <<= 1;
        if hoch != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
        i += 1;
    }
    result
}

/// Die Rundenkonstanten nach FIPS 197: RCON[i] = x^(i-1) im GF(2^8).
///
/// Diese acht Werte sind als KONSTANTE eingetragen, aus demselben Grund
/// wie die S-Box: sie sind acht Byte, und eine falsche Berechnung faellt
/// erst im Testvektor auf, nicht beim Lesen.
const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// AES-128 oder AES-256, je nach Schluessellaenge.
///
/// Der Schluesselexpandiert-Schluesselplan ist hier ein `[u8; 240]`, feldgross
/// und damit M4-sicher: er haengt nicht von der Eingabe ab, sondern von der
/// Auswahl 128 oder 256.
#[derive(Clone)]
pub struct Aes {
    runden: usize,
    rk: [u8; 240],
}

impl Aes {
    /// Neuer Schluessel, 16 Byte (AES-128) oder 32 Byte (AES-256).
    pub fn neu(schluessel: &[u8]) -> Option<Self> {
        if schluessel.len() == 24 {
            return None;
        }
        Self::neu_web(schluessel)
    }

    /// Like [`Aes::neu`], also with 24-byte keys (AES-192, which Web Crypto
    /// offers and TLS 1.3 does not use). Added 2026-10-15.
    pub fn neu_web(schluessel: &[u8]) -> Option<Self> {
        let (runden, nk) = match schluessel.len() {
            16 => (10usize, 4usize),
            24 => (12usize, 6usize),
            32 => (14usize, 8usize),
            _ => return None,
        };
        let mut rk = [0u8; 240];
        rk[..schluessel.len()].copy_from_slice(schluessel);

        // Der Schluesselplan nach FIPS 197 Abschnitt 5.2, wortweise gelesen
        // (die Bytes eines Wortes sind big endian im Speicher).
        let mut i = nk;
        while i < 4 * (runden + 1) {
            let mut temp = [
                rk[(i - 1) * 4],
                rk[(i - 1) * 4 + 1],
                rk[(i - 1) * 4 + 2],
                rk[(i - 1) * 4 + 3],
            ];
            if i % nk == 0 {
                temp = [
                    S_BOX[temp[1] as usize] ^ RCON[i / nk - 1],
                    S_BOX[temp[2] as usize],
                    S_BOX[temp[3] as usize],
                    S_BOX[temp[0] as usize],
                ];
            } else if nk > 6 && i % nk == 4 {
                temp = [
                    S_BOX[temp[0] as usize],
                    S_BOX[temp[1] as usize],
                    S_BOX[temp[2] as usize],
                    S_BOX[temp[3] as usize],
                ];
            }
            for k in 0..4 {
                rk[i * 4 + k] = rk[(i - nk) * 4 + k] ^ temp[k];
            }
            i += 1;
        }
        Some(Aes { runden, rk })
    }

    /// Ein Block (16 Byte) verschluesseln IN ORT.
    pub fn verschluessel(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, 0);
        for r in 1..self.runden {
            sub_bytes_ein(block);
            self.shift_rows(block);
            self.mix_columns(block);
            self.add_round_key(block, r);
        }
        sub_bytes_ein(block);
        self.shift_rows(block);
        self.add_round_key(block, self.runden);
    }

    /// Ein Block entschluesseln IN ORT (nur zum Testen der Sichtbarkeit).
    pub fn entschluessel(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, self.runden);
        for r in (1..self.runden).rev() {
            self.inv_shift_rows(block);
            self.inv_sub_bytes(block);
            self.add_round_key(block, r);
            self.inv_mix_columns(block);
        }
        self.inv_shift_rows(block);
        self.inv_sub_bytes(block);
        self.add_round_key(block, 0);
    }

    fn add_round_key(&self, block: &mut [u8; 16], runde: usize) {
        for (i, b) in block.iter_mut().enumerate() {
            *b ^= self.rk[runde * 16 + i];
        }
    }

    fn inv_sub_bytes(&self, block: &mut [u8; 16]) {
        for b in block.iter_mut() {
            *b = inv_s_box(*b);
        }
    }

    fn shift_rows(&self, block: &mut [u8; 16]) {
        let alt = *block;
        // Zeile 0 bleibt, Zeilen 1..3 wandern um 1, 2, 3 nach links.
        for zeile in 1..4 {
            for spalte in 0..4 {
                block[spalte * 4 + zeile] = alt[((spalte + zeile) % 4) * 4 + zeile];
            }
        }
    }

    fn inv_shift_rows(&self, block: &mut [u8; 16]) {
        let alt = *block;
        for zeile in 1..4 {
            for spalte in 0..4 {
                block[((spalte + zeile) % 4) * 4 + zeile] = alt[spalte * 4 + zeile];
            }
        }
    }

    fn mix_columns(&self, block: &mut [u8; 16]) {
        for s in 0..4 {
            let a0 = block[s * 4];
            let a1 = block[s * 4 + 1];
            let a2 = block[s * 4 + 2];
            let a3 = block[s * 4 + 3];
            // FIPS 197 Abschnitt 5.1.3, die Mischmatrix:
            //   [ 2 3 1 1 ]   [ 1 2 3 1 ]   [ 1 1 2 3 ]   [ 3 1 1 2 ]
            //   [ 1 2 3 1 ] . [ 3 1 1 2 ] . [ 2 3 1 1 ] . [ 1 1 2 3 ]
            //   [ 3 1 1 2 ]   [ 2 3 1 1 ]   [ 3 1 1 2 ]   [ 2 3 1 1 ]
            //   [ 1 1 2 3 ]   [ 1 2 3 1 ]   [ 1 1 2 3 ]   [ 3 1 1 2 ]
            //
            // EIGENER FEHLER, 30.09.: in der ersten und der vierten Zeile
            // stand 2 an der Stelle, an der 3 stehen muss. Ich hatte
            // `gmul2(a1)` statt `gmul3(a1)`. AES-128 lieferte daraufhin
            // 2ce43405331b9f7eab88e1c749de5319 statt des FIPS-Vektors
            // 69c4e0d86a7b0430d8cdb78070b4c55a — plausibel aussehende
            // Muell.
            block[s * 4] = gmul2(a0) ^ gmul3(a1) ^ a2 ^ a3;
            block[s * 4 + 1] = a0 ^ gmul2(a1) ^ gmul3(a2) ^ a3;
            block[s * 4 + 2] = a0 ^ a1 ^ gmul2(a2) ^ gmul3(a3);
            block[s * 4 + 3] = gmul3(a0) ^ a1 ^ a2 ^ gmul2(a3);
        }
    }

    fn inv_mix_columns(&self, block: &mut [u8; 16]) {
        for s in 0..4 {
            let a0 = block[s * 4];
            let a1 = block[s * 4 + 1];
            let a2 = block[s * 4 + 2];
            let a3 = block[s * 4 + 3];
            block[s * 4] = gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9);
            block[s * 4 + 1] = gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13);
            block[s * 4 + 2] = gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11);
            block[s * 4 + 3] = gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14);
        }
    }
}

const fn gmul2(a: u8) -> u8 {
    gmul(a, 2)
}

/// Multiplikation mit 3 im GF(2^8). Sie ist NICHT dasselbe wie eine
/// Verdopplung: 3 = 1 + 2, also `gmul(a, 3) == gmul(a, 2) ^ a`.
const fn gmul3(a: u8) -> u8 {
    gmul(a, 3)
}

/// Die inverse S-Box. Sie wird nicht selbst berechnet (das waere ein
/// Inversen-Wurzelziehen pro Byte), sondern aus `s_b_box` abgeleitet und
/// beim ersten Zugriff einmal gesetzt. Das ist ein `OnceLock`, KEIN
/// `Box::leak` (M6) und hat eine feste Groesse.
fn inv_s_box(x: u8) -> u8 {
    use std::sync::OnceLock;
    static TABELLE: OnceLock<[u8; 256]> = OnceLock::new();
    let t = TABELLE.get_or_init(|| {
        let mut t = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            t[S_BOX[i] as usize] = i as u8;
            i += 1;
        }
        t
    });
    t[x as usize]
}

// ---------------------------------------------------------------------------
// GHASH — die authentische Seite von GCM
// ---------------------------------------------------------------------------

/// Multiplikation im Polynomring GF(2^128) mit dem NIST-Polynom
/// x^128 + x^7 + x^2 + x + 1.
fn ghash_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;
    for i in 0..128 {
        if x[i / 8] >> (7 - (i % 8)) & 1 == 1 {
            for k in 0..16 {
                z[k] ^= v[k];
            }
        }
        // v = v >> 1, bei herausgeschobenem Einsbit mit R = 0xe10000...
        //
        // EIGENER FEHLER, 30.09.: die Bedingung war `v[0] & 1 == 1`. `v[0]`
        // ist das STARKSTE Byte, `v[15]` das SCHWACHSTE. Entscheidend ist
        // das schwache, denn es wird herausgeschoben und genau dann muss
        // reduziert werden. Mit `v[0]` wurde nur bei jedem 8. Bit
        // reduziert.
        //
        // Der Fehler fiel nur bei einem GHASH-Block auf, dessen erstes
        // Byte nicht null war — beim NIST-Testfall 1 (leere Nutzlast) ist
        // y die ganze Zeit null, da ist y = 0 x H = 0 und der Fehler
        // unsichtbar. Ein Test mit leerer Eingabe haette ihn nie gefunden.
        let lsb = v[15] & 1;
        for k in (1..16).rev() {
            v[k] = (v[k] >> 1) | ((v[k - 1] & 1) << 7);
        }
        v[0] >>= 1;
        if lsb == 1 {
            v[0] ^= 0xe1;
        }
    }
    z
}

/// GHASH-Zustand fuer eine Nachricht.
struct Ghash {
    h: [u8; 16],
    y: [u8; 16],
}

impl Ghash {
    fn neu(h: [u8; 16]) -> Self {
        Ghash { h, y: [0u8; 16] }
    }

    fn aufnehmen(&mut self, daten: &[u8]) {
        let mut rest = daten;
        while rest.len() >= 16 {
            let mut block = [0u8; 16];
            block.copy_from_slice(&rest[..16]);
            for (k, b) in self.y.iter_mut().zip(block.iter()) {
                *k ^= *b;
            }
            self.y = ghash_mul(&self.y, &self.h);
            rest = &rest[16..];
        }
        if !rest.is_empty() {
            let mut block = [0u8; 16];
            block[..rest.len()].copy_from_slice(rest);
            for (k, b) in self.y.iter_mut().zip(block.iter()) {
                *k ^= *b;
            }
            self.y = ghash_mul(&self.y, &self.h);
        }
    }

    fn fertig(&self) -> [u8; 16] {
        self.y
    }
}

/// Hoechste Nutzlast fuer einen GCM-Aufruf in dieser Crate.
///
/// M3/M4. Warum eine Grenze und welcher Wert: NIST SP 800-38D Abschnitt
/// 6.2 erlaubt pro Schluessel 2^32 Bloecke, das waere 2^36 Byte. TLS 1.3
/// laesst 2^14 + 256 Byte je Record zu (RFC 8446 Abschnitt 5.1). Diese
/// Grenze ist also die ANWENDUNGSgrenze, nicht die Kryptografiegrenze.
/// Sie wird VOR der Allokation geprueft, damit ein Aufrufer mit einer
/// unsinnigen Laenge einen benannten Fehler bekommt statt einer
/// halben Stunde Rechenzeit.
pub const MAX_NUTZLAST_GCM: usize = 1 << 24;

/// Warum eine AEAD-Operation abgelehnt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcmFehler {
    /// Schluessellaenge war nicht 16 oder 32 Byte.
    SchluesselFalsch { ist: usize },
    /// Nonce war nicht 12 Byte. TLS 1.3 benutzt genau 12.
    NonceFalsch { ist: usize },
    /// Der Tag stimmte nicht.
    TagFalsch,
    /// Die Nutzlast war laenger als die gepruefte Grenze.
    ZuLang { ist: usize },
}

impl std::fmt::Display for GcmFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GcmFehler::SchluesselFalsch { ist } => {
                write!(f, "Schluessel hat {ist} Byte, erwartet 16 oder 32")
            }
            GcmFehler::NonceFalsch { ist } => write!(f, "Nonce hat {ist} Byte, erwartet 12"),
            GcmFehler::TagFalsch => write!(f, "der Tag stimmt nicht"),
            GcmFehler::ZuLang { ist } => {
                write!(f, "Nutzlast hat {ist} Byte, ueber der Grenze")
            }
        }
    }
}

impl std::error::Error for GcmFehler {}

/// AES-GCM: Verschluesselung mit authentischem Klartext.
pub fn gcm_chiffre(
    schluessel: &[u8],
    nonce: &[u8],
    aad: &[u8],
    klartext: &[u8],
) -> Result<(Vec<u8>, [u8; 16]), GcmFehler> {
    let aes = Aes::neu(schluessel).ok_or(GcmFehler::SchluesselFalsch {
        ist: schluessel.len(),
    })?;
    if nonce.len() != 12 {
        return Err(GcmFehler::NonceFalsch { ist: nonce.len() });
    }
    if klartext.len() > MAX_NUTZLAST_GCM {
        return Err(GcmFehler::ZuLang {
            ist: klartext.len(),
        });
    }

    // H = AES_K(0^128)
    let mut h_block = [0u8; 16];
    aes.verschluessel(&mut h_block);
    let mut g = Ghash::neu(h_block);

    // J0 = Nonce || 0^31 || 1 (12-Byte-Nonce, 96-Bit-Zaehler)
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;

    // Der Schluesselstrom: E(J0) und danach E(J0+1), E(J0+2) ...
    let mut e0 = j0;
    aes.verschluessel(&mut e0);
    // DER TAG WIRD UEBER DAS CHIFFRAT GEBILDET, nicht ueber den Klartext
    // (NIST SP 800-38D Abschnitt 6.1, Schritt 5).
    //
    // EIGENER FEHLER, 30.09.: hier stand vorher `klartext`. Bei einem
    // Klartext aus lauter Nullen — genau NIST-Testfall 2 — faellt das
    // nicht auf, weil 0 x H = 0 ist und beide Wege gleich aussehen.

    // M4: die Ausgabe wird exakt so gross wie die Eingabe alloziert, und
    // die Eingabe ist vorher gegen MAX_NUTZLAST_GCM geprueft.
    let mut out = vec![0u8; klartext.len()];
    gcm_strom(&aes, &j0, klartext, &mut out);
    let tag = ghash_zwischen(&mut g, aad, &out, &e0);
    Ok((out, tag))
}

/// AES-GCM: Entschlusselung mit Pruefung.
pub fn gcm_dechiffre(
    schluessel: &[u8],
    nonce: &[u8],
    aad: &[u8],
    chiffrat: &[u8],
    tag: &[u8; 16],
) -> Result<Vec<u8>, GcmFehler> {
    let aes = Aes::neu(schluessel).ok_or(GcmFehler::SchluesselFalsch {
        ist: schluessel.len(),
    })?;
    if nonce.len() != 12 {
        return Err(GcmFehler::NonceFalsch { ist: nonce.len() });
    }
    if chiffrat.len() > MAX_NUTZLAST_GCM {
        return Err(GcmFehler::ZuLang {
            ist: chiffrat.len(),
        });
    }

    let mut h_block = [0u8; 16];
    aes.verschluessel(&mut h_block);
    let mut g = Ghash::neu(h_block);
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;
    let mut e0 = j0;
    aes.verschluessel(&mut e0);
    let erwartet = ghash_zwischen(&mut g, aad, chiffrat, &e0);

    // DER TAG ZUERST, wie bei ChaCha20-Poly1305.
    if !crate::hmac::vergleich_gleich(&erwartet, tag) {
        return Err(GcmFehler::TagFalsch);
    }

    let mut out = vec![0u8; chiffrat.len()];
    gcm_strom(&aes, &j0, chiffrat, &mut out);
    Ok(out)
}

/// Der Schluesselstrom von GCM: E(J0+1), E(J0+2), ... (NIST SP 800-38D
/// Abschnitt 6.2).
///
/// M3: die Schleife laeuft ueber `chunks(16)`, also ueber die Laenge der
/// EINGABE. Die Grenze `MAX_NUTZLAST_GCM` ist vorher geprueft, die
/// Schleife kann also hoechstens so oft laufen, wie die Grenze erlaubt.
///
/// Der letzte Block darf kuerzer als 16 Byte sein. Er wird mit Nullen
/// ergaenzt, verschluesselt und nur auf die echten Bytes angewendet —
/// genau so, wie GHASH denselben Rest mit Nullen fuellt.
fn gcm_strom(aes: &Aes, j0: &[u8; 16], ein: &[u8], aus: &mut [u8]) {
    // EIGENER FEHLER, 30.09.: es stand `ctr = zaehler_plus(ctr, i + 1)`.
    // Damit wurde J0+1 genommen, darauf J0+3, darauf J0+6 — der Zaehler
    // HAEUFT sich an, statt bei jedem Block von J0 aus zu laufen. Der
    // erste Block stimmte, alle folgenden nicht. Bei einer 16-Byte-
    // Nutzlast faellt das nie auf, deshalb ist der NIST-Testfall 2
    // (16 Byte) gruen und Testfall 4 (60 Byte) rot.
    for (i, block) in ein.chunks(16).enumerate() {
        let ctr = zaehler_plus(*j0, i as u32 + 1);
        let mut e = ctr;
        aes.verschluessel(&mut e);
        for (k, &b) in block.iter().enumerate() {
            aus[i * 16 + k] = b ^ e[k];
        }
    }
}

/// Addiert `n` auf die letzten 32 Bit von `block` — GCM zaehlt dort weiter.
///
/// EIGENER FEHLER, 30.09.: es stand `block[15] = block[15].wrapping_add(n)`
/// und damit ging der Uebertrag in Byte 14 verloren. Bei einer
/// TLS-Nutzlast (hoechstens 2^14 + 256 Byte, also 1040 Bloecke) faellt das
/// nie auf, es ist trotzdem falsch.
fn zaehler_plus(mut block: [u8; 16], n: u32) -> [u8; 16] {
    let letzte = u32::from_be_bytes([block[12], block[13], block[14], block[15]]);
    let neu = letzte.wrapping_add(n);
    block[12..].copy_from_slice(&neu.to_be_bytes());
    block
}

/// GHASH ueber AAD und Chiffrat, dann der Schluesselstrom von E(J0).
fn ghash_zwischen(g: &mut Ghash, aad: &[u8], daten: &[u8], e0: &[u8; 16]) -> [u8; 16] {
    // `aufnehmen` fuellt einen Rest selbst mit Nullen auf 16 Byte und
    // verarbeitet ihn. Deshalb hier KEINE zweite Fuellung: die wuerde
    // einen zusaetzlichen Nullblock erzeugen, und genau der ist in den
    // Tag eingerechnet.
    g.aufnehmen(aad);
    g.aufnehmen(daten);
    let mut laengen = [0u8; 16];
    laengen[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    laengen[8..].copy_from_slice(&((daten.len() as u64) * 8).to_be_bytes());
    g.aufnehmen(&laengen);
    let mut tag = g.fertig();
    for k in 0..16 {
        tag[k] ^= e0[k];
    }
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::{hexen, von_hex};

    fn h(s: &str) -> Vec<u8> {
        von_hex(s).expect("Hex im Test")
    }

    /// FIPS 197 Anhang C.1: AES-128, Schluessel und Klartext "00112233...".
    #[test]
    fn fips197_anhang_c1_aes128() {
        let k = h("000102030405060708090a0b0c0d0e0f");
        let mut block = [0u8; 16];
        block.copy_from_slice(&h("00112233445566778899aabbccddeeff"));
        let aes = Aes::neu(&k).expect("16 Byte Schluessel");
        assert_eq!(aes.runden, 10, "M7: 10 Runden");
        aes.verschluessel(&mut block);
        assert_eq!(hexen(&block), "69c4e0d86a7b0430d8cdb78070b4c55a");
    }

    /// FIPS 197 Anhang C.2: AES-128-Entschluesselung, derselbe Vektor.
    #[test]
    fn fips197_anhang_c2_rueckwaerts() {
        let k = h("000102030405060708090a0b0c0d0e0f");
        let mut block = [0u8; 16];
        block.copy_from_slice(&h("69c4e0d86a7b0430d8cdb78070b4c55a"));
        let aes = Aes::neu(&k).expect("16 Byte");
        aes.entschluessel(&mut block);
        assert_eq!(hexen(&block), "00112233445566778899aabbccddeeff");
    }

    /// FIPS 197 Anhang C.3: AES-256.
    #[test]
    fn fips197_anhang_c3_aes256() {
        let k = h("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let mut block = [0u8; 16];
        block.copy_from_slice(&h("00112233445566778899aabbccddeeff"));
        let aes = Aes::neu(&k).expect("32 Byte Schluessel");
        assert_eq!(aes.runden, 14, "M7: 14 Runden");
        aes.verschluessel(&mut block);
        assert_eq!(hexen(&block), "8ea2b7ca516745bfeafc49904b496089");
    }

    /// M7: Schluessellaengen, die es nicht gibt, ergeben None — kein Panic.
    #[test]
    fn falsche_schluessellaenge_ist_none() {
        assert!(Aes::neu(&[0u8; 0]).is_none());
        assert!(Aes::neu(&[0u8; 15]).is_none());
        assert!(Aes::neu(&[0u8; 17]).is_none());
        assert!(Aes::neu(&[0u8; 24]).is_none());
        assert!(Aes::neu(&[0u8; 31]).is_none());
        assert!(Aes::neu(&[0u8; 33]).is_none());
        assert!(Aes::neu(&[0u8; 64]).is_none());
        assert!(Aes::neu(&[0u8; 16]).is_some());
        assert!(Aes::neu(&[0u8; 32]).is_some());
    }

    /// NIST GCM-Testfall 1 (ohne AAD, 0 Byte Nutzlast): leerer Chiffrat, ein
    /// vorgegebener Tag.
    #[test]
    fn nist_gcm_testfall_1() {
        let k = [0u8; 16];
        let n = [0u8; 12];
        let (c, tag) = gcm_chiffre(&k, &n, b"", b"").expect("leer");
        assert_eq!(c.len(), 0, "M7: leerer Chiffrat");
        // GEMESSEN: E(J0) = AES_K(0^00...01) und GHASH ueber den
        // Nullblock; der Tag ist genau E(J0), weil y die ganze Zeit null
        // bleibt. Mit dieser Beobachtung sieht man, warum ein leerer
        // Testfall einen GHASH-Fehler NIE findet.
        assert_eq!(hexen(&tag), "58e2fccefa7e3061367f1d57a4e7455a");
    }

    /// NIST GCM-Testfall 2: 16 Byte Nutzlast, kein AAD.
    #[test]
    fn nist_gcm_testfall_2() {
        let k = [0u8; 16];
        let n = [0u8; 12];
        let pt = [0u8; 16];
        let (c, tag) = gcm_chiffre(&k, &n, b"", &pt).expect("16 Byte");
        assert_eq!(hexen(&c), "0388dace60b6a392f328c2b971b2fe78");
        assert_eq!(hexen(&tag), "ab6e47d42cec13bdf53a67b21257bddf");

        let kette = gcm_dechiffre(&k, &n, b"", &c, &tag).expect("Entschluesselung");
        assert_eq!(kette, pt.to_vec(), "Rundlauf");
    }

    /// NIST GCM-Testfall 4: mit AAD.
    #[test]
    fn nist_gcm_testfall_4_mit_aad() {
        let k = [
            0xfe, 0xff, 0xe9, 0x92, 0x86, 0x65, 0x73, 0x1c, 0x6d, 0x6a, 0x8f, 0x94, 0x67, 0x30,
            0x83, 0x08,
        ];
        let n = [
            0xca, 0xfe, 0xba, 0xbe, 0xfa, 0xce, 0xdb, 0xad, 0xde, 0xca, 0xf8, 0x88,
        ];
        // SECHSZIG Byte, nicht vierundsechzig: ich hatte zwoelf Byte
        // zusaetzlich abgetippt. GEMESSEN gegen Python `cryptography`:
        // 60 Byte sind drei volle Bloecke und ein 12-Byte-Rest, und der
        // Rest ist genau der Fall, den dieser Aufbau vorher mit einem
        // Fehler abgewiesen hat.
        let pt = h(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = [
            0xfe, 0xed, 0xfa, 0xce, 0xde, 0xad, 0xbe, 0xef, 0xfe, 0xed, 0xfa, 0xce, 0xde, 0xad,
            0xbe, 0xef, 0xab, 0xad, 0xda, 0xd2,
        ];
        assert_eq!(
            pt.len(),
            60,
            "M7: der Klartext hat 60 Byte, kein Vielfaches von 16"
        );
        let (c, tag) = gcm_chiffre(&k, &n, &aad, &pt).expect("60 Byte mit Rest");
        assert_eq!(c.len(), 60, "M7: Chiffrat hat dieselbe Laenge");
        // GEMESSEN gegen Python `cryptography`, 30.09.
        assert_eq!(
            hexen(&c),
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e\
             21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091"
        );
        assert_eq!(hexen(&tag), "5bc94fbc3221a5db94fae95ae7121a47");
        let kette = gcm_dechiffre(&k, &n, &aad, &c, &tag).expect("Rundlauf");
        assert_eq!(kette, pt.to_vec());
    }

    /// M7: ein gekipptes Byte wird erkannt — bei Chiffrat, Tag und AAD.
    #[test]
    fn veraenderung_wird_erkannt() {
        let k = [0u8; 16];
        let n = [0u8; 12];
        let pt = [7u8; 32];
        let (c, tag) = gcm_chiffre(&k, &n, b"aad", &pt).expect("32 Byte");

        let mut kaputt = c.clone();
        kaputt[0] ^= 1;
        assert_eq!(
            gcm_dechiffre(&k, &n, b"aad", &kaputt, &tag),
            Err(GcmFehler::TagFalsch)
        );

        let mut kaputt2 = tag;
        kaputt2[0] ^= 1;
        assert_eq!(
            gcm_dechiffre(&k, &n, b"aad", &c, &kaputt2),
            Err(GcmFehler::TagFalsch)
        );

        assert_eq!(
            gcm_dechiffre(&k, &n, b"anders", &c, &tag),
            Err(GcmFehler::TagFalsch)
        );
    }

    /// M3/M7: EIN TEST ueber jede Laenge von 0 bis 80 Byte. Damit ist
    /// jede Lage des letzten Blocks abgedeckt: 0, 1, 15, 16, 17, 31, 32,
    /// 33, 47, 48, 63, 64, 65, 79, 80. Gesichert werden Laenge UND Inhalt
    /// des Rundlaufs.
    ///
    /// M3: die Schleife laeuft ueber eine BENANNTE Konstante, nicht ueber
    /// eine Zahl von aussen.
    #[test]
    fn jede_laenge_von_0_bis_80() {
        const MAX: usize = 80;
        let k = [0x11u8; 16];
        let n = [0x22u8; 12];
        for laenge in 0..=MAX {
            let pt: Vec<u8> = (0..laenge).map(|i| (i * 3 + 1) as u8).collect();
            let (c, tag) = gcm_chiffre(&k, &n, b"aad", &pt).expect("Verschluesselung");
            assert_eq!(c.len(), laenge, "M7: Chiffratlaenge {laenge}");
            let zurueck = gcm_dechiffre(&k, &n, b"aad", &c, &tag).expect("Entschluesselung");
            assert_eq!(zurueck, pt, "M7: Rundlauf bei {laenge} Byte");
            // Und der Inhalt haengt wirklich an der Laenge: zwei
            // verschiedene Laengen duerfen nicht denselben Chiffrat geben.
            if laenge > 0 {
                assert_ne!(
                    &c[..1],
                    &pt[..1].to_vec(),
                    "M7: {laenge} Byte wurden veraendert"
                );
            }
        }
    }

    /// M3/M7: 5000 Byte = 312 Bloecke. Damit laeuft der 32-Bit-Zaehler von
    /// GCM ueber die Bytegrenze 255 hinaus — genau an der Stelle, an der
    /// `zaehler_plus` den Uebertrag in Byte 14 braucht.
    ///
    /// GEMESSEN am 30.09. mit Python `cryptography`, gleiche Eingaben.
    #[test]
    fn langer_text_ueber_die_zaehlergrenze() {
        const LAENGE: usize = 5000;
        let k = crate::sha2::von_hex("2b7e151628aed2a6abf7158809cf4f3c").expect("Hex");
        let n = crate::sha2::von_hex("000102030405060708090a0b").expect("Hex");
        let aad = b"TLS 1.3, 5.2 Record";
        let pt: Vec<u8> = (0..LAENGE).map(|i| (i * 7 + 3) as u8).collect();

        let (c, tag) = gcm_chiffre(&k, &n, aad, &pt).expect("5000 Byte");
        assert_eq!(c.len(), LAENGE, "M7: Laenge bleibt erhalten");
        assert_eq!(
            hexen(&c[..32]),
            "58c52a4ea74ea376db9a5dd6f5d7d1fb7840807ec24fc976aeb82c67fbf880eb",
            "Anfang"
        );
        assert_eq!(
            hexen(&c[LAENGE - 32..]),
            "f3236633ffbf970d9c86813d2353f50e8e3d2c43a84f6e04560ebc27a1fd9e9a",
            "Ende"
        );
        assert_eq!(hexen(&tag), "b6680c0fa785023dda48e941063392a4");
        assert_eq!(
            gcm_dechiffre(&k, &n, aad, &c, &tag).expect("Rundlauf"),
            pt,
            "M7: 5000 Byte zurueck"
        );
    }

    /// M7: ein Schluessel mit zwei verschiedenen Nonces gibt zwei
    /// verschiedene Chiffrate. Sonst waere der Nonce wirkungslos.
    #[test]
    fn derselbe_schluessel_zwei_nonces() {
        let k = [0x33u8; 16];
        let n1 = [0u8; 12];
        let mut n2 = [0u8; 12];
        n2[11] = 1;
        let pt = b"derselbe plaintext";
        let (c1, t1) = gcm_chiffre(&k, &n1, b"", pt).expect("Nonce 1");
        let (c2, t2) = gcm_chiffre(&k, &n2, b"", pt).expect("Nonce 2");
        assert_ne!(c1, c2, "M7: Chiffrate unterscheiden sich");
        assert_ne!(t1, t2, "M7: Tags unterscheiden sich");
        assert_eq!(c1.len(), pt.len());
        assert_eq!(c2.len(), pt.len());
        // Und die Verwechslung wird bemerkt.
        assert_eq!(
            gcm_dechiffre(&k, &n1, b"", &c2, &t2),
            Err(GcmFehler::TagFalsch),
            "falsche Nonce wird erkannt"
        );
    }

    /// M7: Randfaelle sind benannte Fehler, keine Panik.
    #[test]
    fn randfaelle_sind_benannte_fehler() {
        let k = [0u8; 16];
        assert_eq!(
            gcm_chiffre(&[0u8; 20], &[0u8; 12], b"", &[0u8; 16]),
            Err(GcmFehler::SchluesselFalsch { ist: 20 })
        );
        assert_eq!(
            gcm_chiffre(&k, &[0u8; 16], b"", &[0u8; 16]),
            Err(GcmFehler::NonceFalsch { ist: 16 })
        );
        // 17 Byte sind KEIN Fehler mehr: der letzte Block wird teilgefuellt.
        // M7: Laenge und Inhalt werden gesichert.
        let (c, _) = gcm_chiffre(&k, &[0u8; 12], b"", &[0u8; 17]).expect("17 Byte sind erlaubt");
        assert_eq!(c.len(), 17, "M7: unveraenderte Laenge");
        assert_eq!(
            gcm_dechiffre(&k, &[0u8; 12], b"", &c, &[0u8; 16]),
            Err(GcmFehler::TagFalsch),
            "der falsche Tag wird erkannt"
        );
        // Und die Grenze greift, BEVOR alloziert wird (M4).
        assert_eq!(
            gcm_chiffre(&k, &[0u8; 12], b"", &vec![0u8; MAX_NUTZLAST_GCM + 1]),
            Err(GcmFehler::ZuLang {
                ist: MAX_NUTZLAST_GCM + 1
            })
        );
    }
}
