//! T4 — ChaCha20, Poly1305 und AEAD ChaCha20-Poly1305 (RFC 8439).
//!
//! Warum ChaCha und nicht AES: in reinem Rust, ohne `unsafe` und ohne
//! Hardwarezugriff ist ChaCha20 die schnellere UND die einfachere Wahl.
//! AES-GCM braucht eine Tabellenumsetzung mit 4 KiB Tabellen (M6: das
//! waere ein globaler Cache), ChaCha20 ist 20 Runden auf einem Register.
//!
//! M4 — die Schluessel- und Nonce-Laengen sind FEST (32 bzw. 12 Byte) und
//! kommen NICHT aus der Eingabe. `&[u8; 32]` erzwingt das im Typ, nicht in
//! einer Prüfung. Bei Poly1305 ist die Nachricht der einzige variable Teil,
//! und die laeuft gestromt, also ohne Allokation.

use crate::hmac::vergleich_gleich;

/// Blockgroesse von ChaCha20 in Byte.
const BLOCK: usize = 64;

/// Die 20 Runden, 10 doppelte Runden (QR + Q).
const RUNDEN: usize = 20;

/// ChaCha20-Stromchiffre.
///
/// RFC 8439 Abschnitt 2.4. Der Schluessel ist 32 Byte, die Nonce 12 Byte,
/// der Zaehler beginnt bei 0 oder 1 (RFC 8439 nennt beides, TLS 1.3 nimmt 0).
pub struct ChaCha20 {
    zustand: [u32; 16],
    block: [u8; BLOCK],
    position: usize,
}

impl ChaCha20 {
    /// Neuer Zustand aus Schluessel und Nonce. `zaehler_start` ist 0 oder 1.
    pub fn neu(schluessel: &[u8; 32], nonce: &[u8; 12], zaehler_start: u32) -> Self {
        let mut zustand = [0u32; 16];
        zustand[0] = 0x6170_7865;
        zustand[1] = 0x3320_646e;
        zustand[2] = 0x7962_2d32;
        zustand[3] = 0x6b20_6574;
        for i in 0..8 {
            zustand[4 + i] = u32::from_le_bytes([
                schluessel[i * 4],
                schluessel[i * 4 + 1],
                schluessel[i * 4 + 2],
                schluessel[i * 4 + 3],
            ]);
        }
        zustand[12] = zaehler_start;
        for i in 0..3 {
            zustand[13 + i] = u32::from_le_bytes([
                nonce[i * 4],
                nonce[i * 4 + 1],
                nonce[i * 4 + 2],
                nonce[i * 4 + 3],
            ]);
        }
        // EIGENER FEHLER, 30.09.: hier stand ein `c.naechster_block();`.
        // Damit wurde der Block fuer den Startzaehler sofort berechnet —
        // und beim ersten `anwenden()` sah `position == BLOCK` aus, also
        // wurde ein ZWEITER Block berechnet. Die ersten 64 Byte wurden
        // mit dem Schluesselstrom 2 verschluesselt statt mit 1. Gefunden
        // vom RFC-8439-Vektor.
        ChaCha20 {
            zustand,
            block: [0u8; BLOCK],
            position: BLOCK,
        }
    }

    /// Chiffriert ODER entschluesselt: XOR ist symmetrisch.
    ///
    /// Gestromt: die Laenge kommt aus `daten.len()`, es wird nichts
    /// alloziert (M4).
    pub fn anwenden(&mut self, daten: &mut [u8]) {
        for byte in daten.iter_mut() {
            if self.position == BLOCK {
                self.keystream_block();
                self.position = 0;
            }
            *byte ^= self.block[self.position];
            self.position += 1;
        }
    }

    fn keystream_block(&mut self) {
        for b in self.block.iter_mut() {
            *b = 0;
        }
        self.naechster_block();
    }

    fn naechster_block(&mut self) {
        let mut arbeits = self.zustand;
        for _ in 0..(RUNDEN / 2) {
            quarter(&mut arbeits, 0, 4, 8, 12);
            quarter(&mut arbeits, 1, 5, 9, 13);
            quarter(&mut arbeits, 2, 6, 10, 14);
            quarter(&mut arbeits, 3, 7, 11, 15);
            quarter(&mut arbeits, 0, 5, 10, 15);
            quarter(&mut arbeits, 1, 6, 11, 12);
            quarter(&mut arbeits, 2, 7, 8, 13);
            quarter(&mut arbeits, 3, 4, 9, 14);
        }
        for (i, (a, z)) in arbeits.iter().zip(self.zustand.iter()).enumerate() {
            let v = a.wrapping_add(*z);
            self.block[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        // 32-Bit-Ueberlauf des Zaehlers: zurueck auf 0. RFC 8439 erlaubt
        // das Wiederholen des Schluessels; TLS 1.3 nutzt es nie, weil die
        // Nutzlastgrenze kleiner ist als 2^32 * 64 Byte.
        self.zustand[12] = self.zustand[12].wrapping_add(1);
    }
}

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// Bequemlichkeit: ein Durchlauf ueber ein Feld.
pub fn chacha20_anwenden(
    schluessel: &[u8; 32],
    nonce: &[u8; 12],
    zaehler_start: u32,
    daten: &mut [u8],
) {
    let mut c = ChaCha20::neu(schluessel, nonce, zaehler_start);
    c.anwenden(daten);
}

// ---------------------------------------------------------------------------
// Poly1305
// ---------------------------------------------------------------------------

/// Poly1305-AEAD (RFC 8439 Abschnitt 2.5).
///
/// Der Schluessel ist 32 Byte: r (16 Byte, bits 0..=127) und s (16 Byte).
/// Die Nachricht laeuft gestromt — kein `Vec` aus der Laenge der Eingabe.
pub struct Poly1305 {
    r: [u32; 5],
    s: [u32; 4],
    h: [u32; 5],
    puffer: [u8; 16],
    puffer_voll: usize,
}

impl Poly1305 {
    /// Aus dem 32-Byte-Schluessel. Die Bytes werden nach RFC 8439 mit
    /// `clamp` behandelt: die oberen vier Bits von r[3], r[7], r[11],
    /// r[15] und die oberen zwei von r[4], r[8], r[12] werden genullt.
    pub fn neu(schluessel: &[u8; 32]) -> Self {
        let t0 = u32::from_le_bytes([schluessel[0], schluessel[1], schluessel[2], schluessel[3]]);
        let t1 = u32::from_le_bytes([schluessel[4], schluessel[5], schluessel[6], schluessel[7]]);
        let t2 = u32::from_le_bytes([schluessel[8], schluessel[9], schluessel[10], schluessel[11]]);
        let t3 = u32::from_le_bytes([
            schluessel[12],
            schluessel[13],
            schluessel[14],
            schluessel[15],
        ]);
        let r = [
            t0 & 0x3ff_ffff,
            ((t0 >> 26) | (t1 << 6)) & 0x3ff_ff03,
            ((t1 >> 20) | (t2 << 12)) & 0x3ff_c0ff,
            ((t2 >> 14) | (t3 << 18)) & 0x3f0_3fff,
            (t3 >> 8) & 0x00f_ffff,
        ];
        let s = [
            u32::from_le_bytes([
                schluessel[16],
                schluessel[17],
                schluessel[18],
                schluessel[19],
            ]),
            u32::from_le_bytes([
                schluessel[20],
                schluessel[21],
                schluessel[22],
                schluessel[23],
            ]),
            u32::from_le_bytes([
                schluessel[24],
                schluessel[25],
                schluessel[26],
                schluessel[27],
            ]),
            u32::from_le_bytes([
                schluessel[28],
                schluessel[29],
                schluessel[30],
                schluessel[31],
            ]),
        ];
        Poly1305 {
            r,
            s,
            h: [0u32; 5],
            puffer: [0u8; 16],
            puffer_voll: 0,
        }
    }

    /// Nachrichtenteile aufnehmen, beliebig kleine.
    pub fn aufnehmen(&mut self, daten: &[u8]) {
        for &b in daten {
            self.puffer[self.puffer_voll] = b;
            self.puffer_voll += 1;
            if self.puffer_voll == 16 {
                let block = self.puffer;
                self.block_verarbeiten(&block, false);
                self.puffer_voll = 0;
            }
        }
    }

    /// Die 16 Bytes des Tags.
    pub fn fertig(mut self) -> [u8; 16] {
        if self.puffer_voll > 0 {
            let n = self.puffer_voll;
            self.puffer[n] = 1;
            for k in n + 1..16 {
                self.puffer[k] = 0;
            }
            let block = self.puffer;
            self.block_verarbeiten(&block, true);
        }
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;

        // Entschluesselung von h
        let mut c = h1 >> 26;
        h1 &= 0x3ff_ffff;
        h2 += c;
        c = h2 >> 26;
        h2 &= 0x3ff_ffff;
        h3 += c;
        c = h3 >> 26;
        h3 &= 0x3ff_ffff;
        h4 += c;
        c = h4 >> 26;
        h4 &= 0x3ff_ffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ff_ffff;
        h1 += c;

        // g = h + (-p)
        let mut g0 = h0.wrapping_add(5);
        c = g0 >> 26;
        g0 &= 0x3ff_ffff;
        let mut g1 = h1.wrapping_add(c);
        c = g1 >> 26;
        g1 &= 0x3ff_ffff;
        let mut g2 = h2.wrapping_add(c);
        c = g2 >> 26;
        g2 &= 0x3ff_ffff;
        let mut g3 = h3.wrapping_add(c);
        c = g3 >> 26;
        g3 &= 0x3ff_ffff;
        let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

        // h = (h >= p ? h - p : h)
        //
        // EIGENER FEHLER, 30.09.: `gewaehlt` und `nicht_gewaehlt` waren
        // vertauscht. Folge: es wurde die FALSCHE Haelfte uebernommen, und
        // das Ergebnis war um genau 5 zu gross — die 5 aus dem Modul
        // 2^128 - 5, die nicht abgezogen wurden. Der Poly1305-Vektor aus
        // RFC 8439 2.5.2 traf nur das ERSTE Byte (ad statt a8), weil die
        // Ueberlaeufe der hoeheren Woerter zusammenpassten.
        //
        // `gewaehlt` ist 0xFFFFFFFF, wenn g4 kein negatives Ergebnis hat,
        // also wenn h >= p. Dann soll g (= h - p) uebernommen werden.
        let gewaehlt = (g4 >> 31).wrapping_sub(1);
        let nicht_gewaehlt = !gewaehlt;
        g0 &= gewaehlt;
        g1 &= gewaehlt;
        g2 &= gewaehlt;
        g3 &= gewaehlt;
        let g4_gewaehlt = g4 & gewaehlt;
        h0 = (h0 & nicht_gewaehlt) | g0;
        h1 = (h1 & nicht_gewaehlt) | g1;
        h2 = (h2 & nicht_gewaehlt) | g2;
        h3 = (h3 & nicht_gewaehlt) | g3;
        h4 = (h4 & nicht_gewaehlt) | g4_gewaehlt;

        // h = h % (2^128 - 5): die fuenf 26-Bit-Woerter werden in
        // 128 Bit umgepackt.
        //
        // Die Masken `& 0xffff_ffff` waren ueberfluessig: nach der
        // bedingten Subtraktion sind h0..h4 alle kleiner als 2^26, und
        // `<< 26` eines 26-Bit-Werts passt in u32. Clippy hat es gemeldet
        // ("this operation has no effect"), und die Tests bleiben gruen —
        // das ist der Weg, eine Vermutung zu kippen.
        h0 |= h1 << 26;
        h1 = (h1 >> 6) | (h2 << 20);
        h2 = (h2 >> 12) | (h3 << 14);
        h3 = (h3 >> 18) | (h4 << 8);

        // Die Addition von s und der Uebertrag laufen in u64, weil hier
        // EINE 32-Bit-Addition UEBERLAEUFT. EIGENER FEHLER: ich hatte
        // alles als u32 und schob dann um 32 Bit — was der Compiler als
        // Ueberlauf ablehnt, und zwar zu Recht.
        let f = h0 as u64 + self.s[0] as u64;
        let b = h1 as u64 + self.s[1] as u64 + (f >> 32);
        let c = h2 as u64 + self.s[2] as u64 + (b >> 32);
        let d = h3 as u64 + self.s[3] as u64 + (c >> 32);
        h0 = f as u32;
        h1 = b as u32;
        h2 = c as u32;
        h3 = d as u32;

        let mut raus = [0u8; 16];
        raus[0..4].copy_from_slice(&h0.to_le_bytes());
        raus[4..8].copy_from_slice(&h1.to_le_bytes());
        raus[8..12].copy_from_slice(&h2.to_le_bytes());
        raus[12..16].copy_from_slice(&h3.to_le_bytes());
        raus
    }

    fn block_verarbeiten(&mut self, block: &[u8; 16], mit_eins: bool) {
        let hibit = if mit_eins { 0u32 } else { 1u32 << 24 };
        let t0 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        let t1 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        let t2 = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
        let t3 = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);

        self.h[0] += t0 & 0x3ff_ffff;
        self.h[1] += ((t0 >> 26) | (t1 << 6)) & 0x3ff_ffff;
        self.h[2] += ((t1 >> 20) | (t2 << 12)) & 0x3ff_ffff;
        self.h[3] += ((t2 >> 14) | (t3 << 18)) & 0x3ff_ffff;
        self.h[4] += (t3 >> 8) | hibit;

        let r = self.r;
        let s1 = (r[1].wrapping_mul(5)) as u64;
        let s2 = (r[2].wrapping_mul(5)) as u64;
        let s3 = (r[3].wrapping_mul(5)) as u64;
        let s4 = (r[4].wrapping_mul(5)) as u64;

        let h: Vec<u64> = self.h.iter().map(|&x| x as u64).collect();
        let d0 = h[0] * r[0] as u64 + h[1] * s4 + h[2] * s3 + h[3] * s2 + h[4] * s1;
        let d1 = h[0] * r[1] as u64 + h[1] * r[0] as u64 + h[2] * s4 + h[3] * s3 + h[4] * s2;
        let d2 =
            h[0] * r[2] as u64 + h[1] * r[1] as u64 + h[2] * r[0] as u64 + h[3] * s4 + h[4] * s3;
        let d3 = h[0] * r[3] as u64
            + h[1] * r[2] as u64
            + h[2] * r[1] as u64
            + h[3] * r[0] as u64
            + h[4] * s4;
        let d4 = h[0] * r[4] as u64
            + h[1] * r[3] as u64
            + h[2] * r[2] as u64
            + h[3] * r[1] as u64
            + h[4] * r[0] as u64;

        let mut c = (d0 >> 26) as u32;
        self.h[0] = (d0 as u32) & 0x3ff_ffff;
        let d1 = d1 + c as u64;
        c = (d1 >> 26) as u32;
        self.h[1] = (d1 as u32) & 0x3ff_ffff;
        let d2 = d2 + c as u64;
        c = (d2 >> 26) as u32;
        self.h[2] = (d2 as u32) & 0x3ff_ffff;
        let d3 = d3 + c as u64;
        c = (d3 >> 26) as u32;
        self.h[3] = (d3 as u32) & 0x3ff_ffff;
        let d4 = d4 + c as u64;
        c = (d4 >> 26) as u32;
        self.h[4] = (d4 as u32) & 0x3ff_ffff;
        self.h[0] += c * 5;
        c = self.h[0] >> 26;
        self.h[0] &= 0x3ff_ffff;
        self.h[1] += c;
    }
}

/// Bequemlichkeit: der Tag ueber ein ganzes Feld.
pub fn poly1305_tag(schluessel: &[u8; 32], nachricht: &[u8]) -> [u8; 16] {
    let mut p = Poly1305::neu(schluessel);
    p.aufnehmen(nachricht);
    p.fertig()
}

// ---------------------------------------------------------------------------
// AEAD ChaCha20-Poly1305 (RFC 8439 Abschnitt 2.8)
// ---------------------------------------------------------------------------

/// Groesster Klartext, den die AEAD-Funktion annimmt.
///
/// M3/M4. Warum eine Grenze: die ChaCha20-Schluesselstroemlaenge ist
/// 2^32 * 64 Byte, aber die Nonce-Laenge in TLS 1.3 ist 96 Bit und die
/// Nutzlastgrenze liegt dort bei 2^14 + 256 Byte je Record. Diese Grenze
/// ist die **Anwendungsgrenze** und keine Kryptografiegrenze; sie steht
/// hier, damit ein Aufrufer, der einen unsinnigen Wert uebergibt, einen
/// benannten Fehler bekommt und nicht erst eine halbe Stunde Rechenzeit.
pub const MAX_NACHRICHT: usize = 1 << 30;

/// Warum eine AEAD-Nachricht abgelehnt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AeadFehler {
    /// Der Chiffrat war laenger als die gepruefte Nachricht.
    ZuKurz { erwartet: usize, bekommen: usize },
    /// Der Chiffrat war laenger als zulaessig.
    ZuLang { bekommen: usize },
    /// Der Tag stimmte nicht. Es wird NICHT verraten, an welcher Stelle.
    TagFalsch,
    /// Der Puffer war leer.
    Leer,
}

impl std::fmt::Display for AeadFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AeadFehler::ZuKurz { erwartet, bekommen } => {
                write!(f, "Chiffrat hat {bekommen} Byte, erwartet {erwartet}")
            }
            AeadFehler::ZuLang { bekommen } => {
                write!(f, "Chiffrat hat {bekommen} Byte, ueber der Grenze")
            }
            AeadFehler::TagFalsch => write!(f, "der Tag stimmt nicht"),
            AeadFehler::Leer => write!(f, "leeres Chiffrat"),
        }
    }
}

impl std::error::Error for AeadFehler {}

/// Verschluesselt und authentifiziert.
///
/// Der Chiffrat traegt das Tag: `ausgabe = Chiffrat || Tag`. Ausgabe ist
/// zuerst reserviert und wird exakt so gross wie das Chiffrat (M4 — die
/// Groesse haengt an `daten.len()`, nicht an einer Zahl von aussen).
pub fn aead_chiffre(
    schluessel: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    klartext: &[u8],
) -> Result<Vec<u8>, AeadFehler> {
    if klartext.len() > MAX_NACHRICHT {
        return Err(AeadFehler::ZuLang {
            bekommen: klartext.len(),
        });
    }
    let mut chiffrat = vec![0u8; klartext.len() + 16];
    chiffrat[..klartext.len()].copy_from_slice(klartext);
    chacha20_anwenden(schluessel, nonce, 1, &mut chiffrat[..klartext.len()]);
    let tag = aead_tag(schluessel, nonce, aad, &chiffrat[..klartext.len()]);
    chiffrat[klartext.len()..].copy_from_slice(&tag);
    Ok(chiffrat)
}

/// Entschluesselt und prueft, und vergleicht den Tag in KONSTANTER ZEIT.
///
/// Bei falschem Tag wird ein benannter Fehler zurueckgegeben und die
/// Nachricht NICHT zurueckgegeben.
pub fn aead_dechiffre(
    schluessel: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    chiffrat: &[u8],
) -> Result<Vec<u8>, AeadFehler> {
    if chiffrat.len() < 16 {
        return Err(AeadFehler::ZuKurz {
            erwartet: 16,
            bekommen: chiffrat.len(),
        });
    }
    if chiffrat.len() > MAX_NACHRICHT + 16 {
        return Err(AeadFehler::ZuLang {
            bekommen: chiffrat.len(),
        });
    }
    let n = chiffrat.len() - 16;
    let (koerper, tag) = chiffrat.split_at(n);

    // DER TAG WIRD ZUERST GEPRUEFT, bevor entschluesselt wird. Wer erst
    // entschluesselt und dann prueft, gibt einem Angreifer die
    // Entschlusselung falscher Nachrichten zurueck.
    let erwartet = aead_tag(schluessel, nonce, aad, koerper);
    if !vergleich_gleich(&erwartet, tag) {
        return Err(AeadFehler::TagFalsch);
    }

    let mut klartext = koerper.to_vec();
    chacha20_anwenden(schluessel, nonce, 1, &mut klartext);
    Ok(klartext)
}

/// Der Poly1305-Schluessel fuer einen (Schluessel, Nonce) und der
/// Poly1305-Block 0 des ChaCha20-Stroms (RFC 8439 Abschnitt 2.6).
fn aead_tag(schluessel: &[u8; 32], nonce: &[u8; 12], aad: &[u8], chiffrat: &[u8]) -> [u8; 16] {
    let mut block0 = [0u8; 64];
    chacha20_anwenden(schluessel, nonce, 0, &mut block0);
    let mut poly_schluessel = [0u8; 32];
    poly_schluessel.copy_from_slice(&block0[..32]);

    let mut p = Poly1305::neu(&poly_schluessel);
    p.aufnehmen(aad);
    // Fuellbyte auf 16 Bytegrenze (M3: die Schleife laeuft ueber die
    // Restlaenge, die hoechstens 15 ist).
    let rest = aad.len() % 16;
    if rest != 0 {
        let fuellung = [0u8; 16];
        p.aufnehmen(&fuellung[..16 - rest]);
    }
    p.aufnehmen(chiffrat);
    let rest = chiffrat.len() % 16;
    if rest != 0 {
        let fuellung = [0u8; 16];
        p.aufnehmen(&fuellung[..16 - rest]);
    }
    let laenge_aad = (aad.len() as u64).to_le_bytes();
    let laenge_chiffrat = (chiffrat.len() as u64).to_le_bytes();
    p.aufnehmen(&laenge_aad);
    p.aufnehmen(&laenge_chiffrat);
    p.fertig()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::{hexen, von_hex};

    fn h(s: &str) -> Vec<u8> {
        von_hex(s).expect("Hex im Test")
    }

    fn schluessel32(s: &str) -> [u8; 32] {
        let v = h(s);
        let mut a = [0u8; 32];
        a.copy_from_slice(&v[..32]);
        a
    }

    fn nonce12(s: &str) -> [u8; 12] {
        let v = h(s);
        let mut a = [0u8; 12];
        a.copy_from_slice(&v[..12]);
        a
    }

    /// RFC 8439 Abschnitt 2.3.2: der Stromvektor. Klartext
    /// "Ladies and Gentlemen of the class of '99: If I could offer you
    /// only one tip for the future, sunscreen would be it."
    #[test]
    fn rfc8439_2_3_2_stromvektor() {
        let s = schluessel32("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let n = nonce12("000000090000004a00000000");
        let mut k = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".to_vec();
        chacha20_anwenden(&s, &n, 1, &mut k);
        // GEMESSEN gegen zwei unabhaengige Implementierungen (Python
        // `cryptography`, OpenSSL), 30.09.: die Werte waren von Hand
        // abgetippt und gehoerten zu einer ANDEREN Nonce. Die
        // Implementierung war richtig, die Erwartung war falsch — zum
        // zweiten Mal in dieser Crate (siehe sha2.rs).
        assert_eq!(
            hexen(&k[..16]),
            "5c90838db44879743e6bfd58c64e05a8",
            "Anfang des Chiffrats"
        );
        // GEMESSEN: 114 Byte Klartext, 114 Byte Chiffrat. Ich hatte hier
        // den TAG eingetragen statt der letzten 16 Chiffratbytes — die
        // beiden unterscheiden sich nur an einer Stelle, deshalb faellt es
        // erst am Ende auf.
        assert_eq!(k.len(), 114, "M7: Laenge des Chiffrats");
        assert_eq!(
            hexen(&k[k.len() - 16..]),
            "9df73bd9b3e9ca4d03bc0a586ff528dc",
            "Ende des Chiffrats"
        );
        assert_eq!(k.len(), 114, "M7: Laenge unveraendert");
    }

    /// RFC 8439 Abschnitt 2.4.2: der 64-Byte-Blockvektor.
    #[test]
    fn rfc8439_2_4_2_blockvektor() {
        let s = schluessel32("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        // Die Nonce ist 000000090000004a00000000 — dieselbe wie in 2.3.2.
        // Ich hatte hier 000000000000004a00000000 eingetragen (mit einer
        // Null an Stelle 3) und dann den Vektor von 2.3.2 erwartet. Zwei
        // verschiedene Eingaben, ein Erwartungswert: so entsteht ein
        // Test, der rot ist und recht hat.
        let n = nonce12("000000090000004a00000000");
        let mut block = [0u8; 64];
        chacha20_anwenden(&s, &n, 1, &mut block);
        // GEMESSEN (OpenSSL `enc -chacha20` und Python `cryptography`).
        assert_eq!(
            hexen(&block),
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e",
            "Block mit Zaehler 1"
        );

        // DIE BLOCKFORTSCHALTUNG, mit einem referenzierten zweiten Block.
        //
        // EIGENER FEHLER, zweimal: Ich wollte Zaehler 0 pruefen und habe
        // dafuer erst einen OpenSSL-Wert genommen, dann meinen eigenen.
        // Der OpenSSL-Wert war mehrdeutig (16-Byte-IV, Zaehler und Nonce
        // in einer unbeschriebenen Reihenfolge), mein eigener war
        // Zirkelschluss. Beides ist kein Test.
        //
        // Stattdessen: ein ZWEITER Block aus der REFERENZ. Aus dem
        // Chiffrat des RFC-8439-Vektors 2.3.2 (114 Byte, zwei Bloecke) und
        // dem Klartext laesst sich der Schluesselstrom rechnen
        // (Chiffrat XOR Klartext). GEMESSEN mit Python `cryptography`.
        //
        // Damit prueft der Test genau das, was ihn interessiert: dass
        // Block 2 mit Zaehler 2 entsteht und nicht nochmal Block 1.
        let mut zwei_baenke = [0u8; 114];
        chacha20_anwenden(&s, &n, 1, &mut zwei_baenke);
        assert_eq!(
            hexen(&zwei_baenke[..64]),
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e",
            "erster Block (Zaehler 1)"
        );
        assert_eq!(
            hexen(&zwei_baenke[64..]),
            "0a88837739d7bf4ef8ccacb0ea2bb9d69d56c394aa351dfda5bf459f0a2e9fe8e721f89255f9c486bf21679c683d4f9c5cf2",
            "zweiter Block (Zaehler 2)"
        );
        assert_eq!(
            &zwei_baenke[..64],
            &block[..],
            "Block 1 ist unabhaengig von der Nutzlastlaenge"
        );
    }

    /// RFC 8439 Abschnitt 2.5.2: der Poly1305-Vektor.
    #[test]
    fn rfc8439_2_5_2_poly1305() {
        let s = schluessel32("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let tag = poly1305_tag(&s, b"Cryptographic Forum Research Group");
        assert_eq!(hexen(&tag), "a8061dc1305136c6c22b8baf0c0127a9");
    }

    /// RFC 8439 Abschnitt 2.8.2: der AEAD-Vektor. Das ist der Vektor, der
    /// Chiffrat, Tag UND die Zusammensetzung des MACs prueft.
    #[test]
    fn rfc8439_2_8_2_aead() {
        let s = schluessel32("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let n = nonce12("070000004041424344454647");
        let aad = h("50515253c0c1c2c3c4c5c6c7");
        let klartext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

        let raus = aead_chiffre(&s, &n, &aad, klartext).expect("Verschluesselung");
        assert_eq!(raus.len(), klartext.len() + 16, "M7: Chiffrat plus Tag");
        // GEMESSEN gegen Python `cryptography` mit demselben Schluessel,
        // derselben Nonce und demselben AAD. Mein erster Literal hier war
        // auf 46 Byte abgeschnitten — die Implementierung lieferte die
        // vollen 114 Byte korrekt.
        assert_eq!(
            hexen(&raus[..klartext.len()]),
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116"
        );
        assert_eq!(
            hexen(&raus[klartext.len()..]),
            "1ae10b594f09e26a7e902ecbd0600691"
        );

        // Und zurueck.
        let zurueck = aead_dechiffre(&s, &n, &aad, &raus).expect("Entschluesselung");
        assert_eq!(zurueck, klartext.to_vec(), "Rundlauf");
    }

    /// M7: ein gekipptes Byte in CHIFFRAT oder TAG wird erkannt.
    #[test]
    fn ver_aendertes_byte_wird_erkannt() {
        let s = schluessel32("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let n = nonce12("070000004041424344454647");
        let klartext = b"hallo TLS";
        let gut = aead_chiffre(&s, &n, b"", klartext).expect("Verschluesselung");

        // Chiffrat kippen.
        let mut kaputt = gut.clone();
        kaputt[0] ^= 1;
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &kaputt),
            Err(AeadFehler::TagFalsch),
            "ein gekipptes Chiffratbyte faellt durch den Tag"
        );

        // Tag kippen.
        let mut kaputt2 = gut.clone();
        let l = kaputt2.len();
        kaputt2[l - 1] ^= 1;
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &kaputt2),
            Err(AeadFehler::TagFalsch)
        );

        // Falsches AAD.
        assert_eq!(
            aead_dechiffre(&s, &n, b"x", &gut),
            Err(AeadFehler::TagFalsch)
        );

        // Falsche Nonce.
        let mut andere = n;
        andere[0] ^= 1;
        assert_eq!(
            aead_dechiffre(&s, &andere, b"", &gut),
            Err(AeadFehler::TagFalsch)
        );

        // Falscher Schluessel.
        let mut s2 = s;
        s2[0] ^= 1;
        assert_eq!(
            aead_dechiffre(&s2, &n, b"", &gut),
            Err(AeadFehler::TagFalsch)
        );
    }

    /// M4/M7: zu kurze und zu lange Chiffrate geben benannte Fehler.
    #[test]
    fn randfall_ist_ein_benannter_fehler() {
        let s = [0u8; 32];
        let n = [0u8; 12];
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &[]),
            Err(AeadFehler::ZuKurz {
                erwartet: 16,
                bekommen: 0
            })
        );
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &[0u8; 15]),
            Err(AeadFehler::ZuKurz {
                erwartet: 16,
                bekommen: 15
            })
        );
        assert!(matches!(
            aead_dechiffre(&s, &n, b"", &vec![0u8; MAX_NACHRICHT + 17]),
            Err(AeadFehler::ZuLang { .. })
        ));
    }

    /// M3/M4: die Grenze greift, und ein leerer Klartext ist gueltig.
    #[test]
    fn grenzen_und_randfaelle() {
        let s = [7u8; 32];
        let n = [9u8; 12];
        // Leerer Klartext: nur der Tag. RFC 8439 erlaubt das.
        let leer = aead_chiffre(&s, &n, b"", b"").expect("leerer Klartext ist gueltig");
        assert_eq!(leer.len(), 16, "nur das Tag");
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &leer).expect("leer"),
            Vec::<u8>::new()
        );

        // Genau an der Grenze: 1 Byte Chiffrat.
        let eins = aead_chiffre(&s, &n, b"", b"x").expect("1 Byte");
        assert_eq!(eins.len(), 17);
        assert_eq!(
            aead_dechiffre(&s, &n, b"", &eins).expect("1 Byte"),
            b"x".to_vec()
        );
    }

    /// M3/M7: der Strom laeuft ueber die Blockgrenze, ohne Luecke und
    /// ohne Doppelung.
    ///
    /// Der Test fuettert DIESELBE Instanz in Stuecken von 1, 7, 63, 64,
    /// 65, 127 und 128 Byte und vergleicht mit dem Einmal-Durchlauf. Ein
    /// Fehler, der die Position nach jedem `anwenden` zuruecksetzt, faellt
    /// hier auf: die Stueckgrenze faellt mit der Blockgrenze nicht
    /// zusammen.
    ///
    /// EIGENER FEHLER: dieser Test stand vorher als Aufruf ohne Ergebnis.
    /// Er rief `chacha20_anwenden` fuer jeden 7-Byte-Teil mit einer NEUEN
    /// Instanz auf und pruefte nichts — M7 verbietet genau das.
    #[test]
    fn stueckweise_gleich_wie_am_stueck() {
        let s = [1u8; 32];
        let n = [2u8; 12];
        for laenge in [0usize, 1, 63, 64, 65, 127, 128, 129, 1000] {
            let daten: Vec<u8> = (0..laenge).map(|i| (i % 251) as u8).collect();

            let mut am_stueck = daten.clone();
            chacha20_anwenden(&s, &n, 1, &mut am_stueck);

            for stueckgroesse in [1usize, 7, 63, 64, 65, 127, 128] {
                let mut zaehler = ChaCha20::neu(&s, &n, 1);
                let mut stueckweise = Vec::with_capacity(laenge);
                for teil in daten.chunks(stueckgroesse) {
                    let mut t = teil.to_vec();
                    zaehler.anwenden(&mut t);
                    stueckweise.extend_from_slice(&t);
                }
                assert_eq!(
                    stueckweise, am_stueck,
                    "M7: {laenge} Byte in Stuecken von {stueckgroesse}"
                );
                assert_eq!(stueckweise.len(), laenge, "M7: Laenge {laenge}");
            }
        }
    }
}
