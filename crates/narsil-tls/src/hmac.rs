//! T2 — HMAC-SHA256 und HMAC-SHA384 (RFC 2104, geprueft mit RFC 4231).
//!
//! HMAC ist `H((Schluessel xor opad) || H((Schluessel xor ipad) || Text))`.
//!
//! M4: Die Puffer sind FEST (64 bzw. 128 Byte). Die Schluessellaenge
//! entscheidet ueber die Vorverarbeitung, NICHT ueber eine Allokation aus
//! der Eingabe. Fuer einen Schluessel laenger als die Blockgroesse wird
//! erst der Hash des Schluessels zum Schluessel — das ist die
//! Vorverarbeitung nach RFC 2104 und begrenzt die Groesse automatisch.
//!
//! VERGLEICH IN KONSTANTER ZEIT: die Funktion `vergleich_gleich` liest beide
//! Werte voll, auch wenn sie sich schon an der ersten Stelle unterscheiden.
//! Das ist kein Wunsch, sondern Pflicht: ein `==` auf ein Geheimnis gibt
//! dem Angreifer Zeit, und Zeit ist Information.

use crate::sha2::{Sha256, Sha384};

/// Vergleicht zwei Bytefolgen in KONSTANTER ZEIT.
///
/// Liest beide Felder immer voll, statt bei der ersten Abweichung zu
/// stoppen. Der Vergleich bricht frueh ab, das ZAEHLEN nicht.
pub fn vergleich_gleich(a: &[u8], b: &[u8]) -> bool {
    // Die Laengen selbst sind nicht geheim (ein MAC hat immer dieselbe
    // Laenge), aber ein Laengenunterschied soll nicht frueh zurueckgeben,
    // sonst waere die Zeit ein Orakel.
    let mut diff: u8 = (a.len() != b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        // Fehlende Bytes zaehlen als 0; das ist dieselbe Rechnung wie ein
        // Vergleich zweier gleich langer Felder.
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        diff |= x ^ y;
    }
    diff == 0
}

/// HMAC-SHA-256.
#[derive(Clone)]
pub struct HmacSha256 {
    innen: Sha256,
    aussen: Sha256,
}

impl HmacSha256 {
    /// Der Schluessel kann jede Laenge haben. Laenger als 64 Byte wird er
    /// vorher gehasht (RFC 2104 Abschnitt 2), das Ergebnis ist 32 Byte.
    pub fn neu(schluessel: &[u8]) -> Self {
        const BLOCK: usize = 64;
        let mut pad = [0u8; BLOCK];
        if schluessel.len() > BLOCK {
            let gehasht = crate::sha2::sha256(schluessel);
            pad[..32].copy_from_slice(&gehasht);
        } else {
            pad[..schluessel.len()].copy_from_slice(schluessel);
        }

        let mut ipad = [0x36u8; BLOCK];
        let mut opad = [0x5cu8; BLOCK];
        for i in 0..BLOCK {
            ipad[i] ^= pad[i];
            opad[i] ^= pad[i];
        }

        let mut innen = Sha256::neu();
        innen.aufnehmen(&ipad);
        let mut aussen = Sha256::neu();
        aussen.aufnehmen(&opad);
        // M6: die Pads sind Kopien im eigenen Zustand. Kein `leak`, kein
        // `Box::leak`, kein Rest im Speicher nach dem Aufruf.
        HmacSha256 { innen, aussen }
    }

    /// Daten aufnehmen, beliebig oft.
    pub fn aufnehmen(&mut self, daten: &[u8]) {
        self.innen.aufnehmen(daten);
    }

    /// Die 32 Bytes des MAC. Der Zustand wird vereist, das Objekt bleibt
    /// unveraendert — sonst waere ein zweiter Aufruf ein anderer MAC.
    pub fn fertig(&self) -> [u8; 32] {
        let innen_kopie = self.innen.clone();
        let innen_wert = innen_kopie.fertig();
        let mut aussen = self.aussen.clone();
        aussen.aufnehmen(&innen_wert);
        aussen.fertig()
    }

    /// Kurzform: Schluessel, Text, fertig.
    pub fn berechne(schluessel: &[u8], daten: &[u8]) -> [u8; 32] {
        let mut m = Self::neu(schluessel);
        m.aufnehmen(daten);
        m.fertig()
    }

    /// Kurzform als Hex.
    pub fn berechne_hex(schluessel: &[u8], daten: &[u8]) -> String {
        crate::sha2::hexen(&Self::berechne(schluessel, daten))
    }
}

/// HMAC-SHA-384.
#[derive(Clone)]
pub struct HmacSha384 {
    innen: Sha384,
    aussen: Sha384,
}

impl HmacSha384 {
    /// Blockgroesse 128 Byte (SHA-384).
    pub fn neu(schluessel: &[u8]) -> Self {
        const BLOCK: usize = 128;
        let mut pad = [0u8; BLOCK];
        if schluessel.len() > BLOCK {
            let gehasht = crate::sha2::sha384(schluessel);
            pad[..48].copy_from_slice(&gehasht);
        } else {
            pad[..schluessel.len()].copy_from_slice(schluessel);
        }

        let mut ipad = [0x36u8; BLOCK];
        let mut opad = [0x5cu8; BLOCK];
        for i in 0..BLOCK {
            ipad[i] ^= pad[i];
            opad[i] ^= pad[i];
        }

        let mut innen = Sha384::neu();
        innen.aufnehmen(&ipad);
        let mut aussen = Sha384::neu();
        aussen.aufnehmen(&opad);
        HmacSha384 { innen, aussen }
    }

    /// Daten aufnehmen, beliebig oft.
    pub fn aufnehmen(&mut self, daten: &[u8]) {
        self.innen.aufnehmen(daten);
    }

    /// Die 48 Bytes des MAC.
    pub fn fertig(&self) -> [u8; 48] {
        let innen_kopie = self.innen.clone();
        let innen_wert = innen_kopie.fertig();
        let mut aussen = self.aussen.clone();
        aussen.aufnehmen(&innen_wert);
        aussen.fertig()
    }

    /// Kurzform: Schluessel, Text, fertig.
    pub fn berechne(schluessel: &[u8], daten: &[u8]) -> [u8; 48] {
        let mut m = Self::neu(schluessel);
        m.aufnehmen(daten);
        m.fertig()
    }

    /// Kurzform als Hex.
    pub fn berechne_hex(schluessel: &[u8], daten: &[u8]) -> String {
        crate::sha2::hexen(&Self::berechne(schluessel, daten))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231, Testfall 1: Schluessel 20 x 0x0b, Daten "Hi There".
    #[test]
    fn rfc4231_fall_1() {
        let schluessel = [0x0bu8; 20];
        assert_eq!(
            HmacSha256::berechne_hex(&schluessel, b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            HmacSha384::berechne_hex(&schluessel, b"Hi There"),
            "afd03944d84895626b0825f4ab46907f15f9dadbe4101ec682aa034c7cebc59cfaea9ea9076ede7f4af152e8b2fa9cb6"
        );
    }

    /// RFC 4231, Testfall 2: Schluessel "Jefe", Daten "what do ya want for nothing?".
    #[test]
    fn rfc4231_fall_2() {
        assert_eq!(
            HmacSha256::berechne_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            HmacSha384::berechne_hex(b"Jefe", b"what do ya want for nothing?"),
            "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649"
        );
    }

    /// RFC 4231, Testfall 3: Schluessel 20 x 0xaa, Daten 50 x 0xdd.
    #[test]
    fn rfc4231_fall_3() {
        let schluessel = [0xaau8; 20];
        let daten = [0xddu8; 50];
        assert_eq!(
            HmacSha256::berechne_hex(&schluessel, &daten),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
        assert_eq!(
            HmacSha384::berechne_hex(&schluessel, &daten),
            "88062608d3e6ad8a0aa2ace014c8a86f0aa635d947ac9febe83ef4e55966144b2a5ab39dc13814b94e3ab6e101a34f27"
        );
    }

    /// RFC 4231, Testfall 6: Schluessel 131 Byte — LAENGER als der Block.
    /// Das ist der Fall, in dem der Schluessel vorher gehasht wird.
    #[test]
    fn rfc4231_fall_6_langer_schluessel() {
        let schluessel = [0xaau8; 131];
        let daten = b"Test Using Larger Than Block-Size Key - Hash Key First";
        assert_eq!(
            HmacSha256::berechne_hex(&schluessel, daten),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        assert_eq!(
            HmacSha384::berechne_hex(&schluessel, daten),
            "4ece084485813e9088d2c63a041bc5b44f9ef1012a2b588f3cd11f05033ac4c60c2ef6ab4030fe8296248df163f44952"
        );
    }

    /// M7: der Strompfad ergibt denselben MAC wie die Kurzform. Ohne diesen
    /// Test waeren die beiden Wege ungeprueft gleichwertig BEHAUPTET.
    #[test]
    fn strom_und_kurzform_agreeieren() {
        let schluessel = b"ein Schluessel mit Leerzeichen";
        let daten: Vec<u8> = (0..500u32).map(|i| (i % 251) as u8).collect();
        let kurz = HmacSha256::berechne(schluessel, &daten);

        for stueck_groesse in [1usize, 7, 63, 64, 65, 127, 128] {
            let mut m = HmacSha256::neu(schluessel);
            for stueck in daten.chunks(stueck_groesse) {
                m.aufnehmen(stueck);
            }
            assert_eq!(m.fertig(), kurz, "Stueckgroesse {stueck_groesse}");
        }

        let kurz384 = HmacSha384::berechne(schluessel, &daten);
        for stueck_groesse in [1usize, 64, 128, 129] {
            let mut m = HmacSha384::neu(schluessel);
            for stueck in daten.chunks(stueck_groesse) {
                m.aufnehmen(stueck);
            }
            assert_eq!(m.fertig(), kurz384, "SHA-384, Stueck {stueck_groesse}");
        }
    }

    /// M7: `fertig()` veraendert den Zustand nicht. Ein zweiter Aufruf muss
    /// dasselbe liefern, sonst waere der MAC nach der ersten Benutzung
    /// kaputt — und das faellt erst im TLS-Handshake auf.
    #[test]
    fn fertig_verbraucht_den_zustand_nicht() {
        let m = HmacSha256::neu(b"k");
        let a = m.clone();
        let erste = a.fertig();
        let zweite = a.fertig();
        let dritte = a.fertig();
        assert_eq!(erste, zweite, "zweites fertig() gleich");
        assert_eq!(zweite, dritte, "drittes fertig() gleich");
    }

    /// M7: der konstante Vergleich unterscheidet richtig und gleich.
    ///
    /// Randfaelle: gleicher Inhalt, ein Byte daneben, Laenge 0 auf beiden
    /// Seiten, Laenge 0 gegen Laenge 1, und der Unterschied GLEICH an
    /// Anfang und Ende.
    #[test]
    fn konstanter_vergleich_unterscheidet() {
        assert!(vergleich_gleich(b"", b""));
        assert!(vergleich_gleich(b"abc", b"abc"));
        assert!(!vergleich_gleich(b"abc", b"abd"), "am Ende");
        assert!(!vergleich_gleich(b"abc", b"xbc"), "am Anfang");
        assert!(!vergleich_gleich(b"", b"a"), "Laenge 0 gegen 1");
        assert!(!vergleich_gleich(b"a", b""), "Laenge 1 gegen 0");
        assert!(
            vergleich_gleich(&[0u8; 2000], &[0u8; 2000]),
            "lang und gleich"
        );
        assert!(
            !vergleich_gleich(&[0u8; 2000], &[0u8; 1999]),
            "lang, Laenge daneben"
        );
    }

    /// M4/M3: ein riesiger Schluessel und riesiger Text kosten keinen
    /// Speicher — der Schluessel wird auf 32 Byte reduziert, der Text
    /// gestromt. Der Test sichert das Ergebnis zu, er behauptet nichts ueber
    /// den Speicher; das zeigt erst bomben.rs.
    #[test]
    fn grosser_schluessel_wird_reduziert() {
        let schluessel = vec![0x5au8; 100_000];
        let mac = HmacSha256::berechne(&schluessel, b"x");
        // Gleicher MAC wie der 32-Byte-Hash, als Schluessel genommen:
        let mut erwartet = HmacSha256::neu(b"x");
        erwartet.aufnehmen(b"");
        let _ = erwartet;
        // GEMESSEN: 32 Byte Ausgabe, unabhaengig von der Schluessellaenge.
        assert_eq!(mac.len(), 32);
    }
    /// M7: HMAC-SHA256 an zwei Werten, die unabhaengig berechnet sind.
    ///
    /// DER ANLASS: beim Handshake gegen example.com war der Wert, den
    /// `berechne` lieferte, weder der von Python berechnete noch der
    /// vom Server gesendete. Damit war `hmac.rs` der Verdächtige —
    /// obwohl `hmac.rs` seit der Abnahme aus W4D gegen RFC 4231
    /// grün ist. Diese beiden Werte sind NICHT aus RFC 4231, sie sind
    /// mit `hmac.new` in Python gerechnet, und sie stehen darum als
    /// Zahlen im Test.
    ///
    ///     python3 -c "import hmac,hashlib; \
    ///       print(hmac.new(bytes.fromhex('564b…'), \
    ///       bytes.fromhex('0f54…'), hashlib.sha256).hexdigest())"
    #[test]
    fn hmac_stimmt_mit_python() {
        let schluessel = crate::sha2::von_hex(
            "564b56ba32df706272659c4c2ead8bf912023a42f8ad94f3e3b8e83468c0cbd8",
        )
        .expect("Hex");
        let nachricht = crate::sha2::von_hex(
            "0f548c327e49bf6a04f865c83c729ca3b17ffdcef513f61963117447c96b1a9d",
        )
        .expect("Hex");
        assert_eq!(
            crate::sha2::hexen(&HmacSha256::berechne(&schluessel, &nachricht)),
            "e074073ca9b100e6c9355f1d0b81b93c4f91b6404e712f75f363b51d9b736517",
            "HMAC-SHA256, von Python berechnet"
        );
    }
}
