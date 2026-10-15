//! TLS 1.2 — die Recordschicht (RFC 5246 Abschnitt 6.2).
//!
//! added 30.09.2026, als zweiter Teil von T15. Diese Datei ist NICHT
//! `record.rs` wiederholt: TLS 1.3 raeumt die Kopfzeile eines Records
//! auf, TLS 1.2 nicht. In 1.2 ist der Kopfkoerper genau das, was
//! auch wirklich auf der Leitung steht, und **es gibt keine
//!-padding-Laenge wie in 1.3**.
//!
//! # Die zwei Stellen, an denen sich 1.2 von 1.3 unterscheidet
//!
//! 1. **Die Nonce.** In 1.3 ist sie `iv XOR Sequenznummer`, 12 Byte.
//!    In 1.2 mit AES-GCM (RFC 5288) ist sie `salt(4) || explizit(8)`:
//!    vier Byte kommen aus dem Schluesselblock, acht Byte stehen als
//!    Feld IM Record und werden vom Absender frei gewaehlt — in der
//!    Praxis die Sequenznummer. Fuer ChaCha20-Poly1305 (RFC 7905) ist
//!    es `iv XOR (0^32 || sequenz)`, ohne Feld im Record.
//! 2. **Das AAD.** In 1.3 sind es fuenf Byte (Typ, Version, Laenge).
//!    In 1.2 ist es **13 Byte**: zusaetzlich die 64-Bit-Sequenznummer
//!    am Anfang. Das ist kein Detail — AES-GCM rechnet das AAD in die
//!    Authentifizierung ein, und ein 5-Byte-AAD schlaegt bei jedem
//!    Record fehl.
//!
//! Beide sind in `strom.rs` (TLS 1.3) richtig und hier falsch
//! geschrieben, wenn man sie von dort kopiert. Deshalb steht beides
//! hier ausgeschrieben und nicht durch einen Aufruf ersetzt.

use std::fmt;

/// Hoechstgrenze fuer einen Recordinhalt.
///
/// RFC 5246 Abschnitt 6.2.1: `2^14` Byte Klartext, plus 2048 Byte
/// Kompression. `MAX_NACHRICHT` in `record.rs` ist derselbe Wert fuer
/// TLS 1.3; hier steht er noch einmal, weil die Datei ohne die andere
/// auskommen muss.
pub const MAX_KLAR: usize = 16384;

/// Plus 2048 fuer die Kompression (RFC 5246 Abschnitt 6.2.2).
pub const MAX_KOMPRESSION: usize = MAX_KLAR + 2048;

/// M3: die Obergrenze fuer die Kette. Ein Handshake besteht aus
/// mindestens drei Nachrichten, ein HTTP-GET aus einer Anfrage und
/// einer Antwort. 64 Records sind fuer beides reichlich und machen
/// eine Endlosschleife unmoglich.
pub const MAX_FLUG_RECORDS: usize = 64;

/// Inhaltstypen (RFC 5246 Abschnitt 6.2.1).
pub mod typen {
    pub const CHANGE_CIPHER_SPEC: u8 = 20;
    pub const ALERT: u8 = 21;
    pub const HANDSHAKE: u8 = 22;
    pub const APPLICATION_DATA: u8 = 23;
    pub const HEARTBEAT: u8 = 24;
}

/// Die AEAD einer TLS-1.2-Suite.
///
/// AES-GCM und ChaCha20-Poly1305. **Kein CBC**: die CBC-Suiten sind in
/// RFC 5246/5288 zwar definiert, aber sie sind der Grund fuer Lucky13
/// und POODLE, und der Auftrag schliesst sie aus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aead {
    Aes128Gcm,
    Aes256Gcm,
    ChaCha20Poly1305,
}

impl Aead {
    /// Laenge des Schluessels in Byte (M4: die Zieldlaenge ist fest und
    /// kommt aus keiner Eingabe).
    pub fn schluessel_laenge(self) -> usize {
        match self {
            Aead::Aes128Gcm => 16,
            Aead::Aes256Gcm => 32,
            Aead::ChaCha20Poly1305 => 32,
        }
    }

    /// Laenge des IV aus dem Schluesselblock.
    ///
    /// AES-GCM: 4 Byte (der „Salt" in RFC 5288). ChaCha20: 12 Byte
    /// (RFC 7905). Das sind verschiedene Groessen fuer aehnlich
    /// benannte Dinge, und der Unterschied steht in genau einer
    /// Stelle dieser Datei: `nonce()`.
    pub fn iv_laenge(self) -> usize {
        match self {
            Aead::Aes128Gcm | Aead::Aes256Gcm => 4,
            Aead::ChaCha20Poly1305 => 12,
        }
    }

    /// Traegt der Record ein explizites Nonce-Feld?
    pub fn mit_explizitem_nonce(self) -> bool {
        matches!(self, Aead::Aes128Gcm | Aead::Aes256Gcm)
    }

    /// Laenge des Tags in Byte.
    pub fn tag_laenge(self) -> usize {
        16
    }

    /// Braucht die Suite einen MAC-Schluessel?
    ///
    /// NEIN, bei allen drei. In TLS 1.2 nimmt ein AEAD den Platz von
    /// MAC und Schluessel ein; es gibt keinen `mac_key` im
    /// Schluesselblock. Wer hier einen 20-Byte-Slot einplant, verschiebt
    /// jeden folgenden Schluessel um 20 Byte — und die Connection
    /// bricht beim ersten Record ab, ohne dass jemand weiss, warum.
    pub fn braucht_mac(self) -> bool {
        false
    }

    /// Laenge des gesamten Schluesselblocks fuer diese Suite.
    pub fn schluesselblock(self) -> usize {
        2 * self.schluessel_laenge() + 2 * self.iv_laenge()
    }

    /// Verschluesselt `klartext` als Recordinhalt.
    ///
    /// `salt` ist der IV des Schluessels aus dem Schluesselblock,
    /// `seq` die 64-Bit-Sequenznummer. Fuer AES-GAM wird die
    /// Sequenznummer als explizites Nonce vorangestellt (das ist die
    /// Wahl, die OpenSSL und Browser treffen); ein Server, der einen
    /// anderen Wert sendet, wird nicht erkannt — siehe `nonce()`.
    pub fn chiffre(
        &self,
        schluessel: &[u8],
        salt: &[u8],
        seq: u64,
        typ: u8,
        klartext: &[u8],
    ) -> Result<Vec<u8>, Rec12Fehler> {
        if klartext.len() > MAX_KOMPRESSION {
            return Err(Rec12Fehler::ZuLang {
                grenze: MAX_KOMPRESSION,
            });
        }
        // ECHTER FEHLER, und der ist beim Schreiben passiert: der
        // explizite Nonce stand ZUVOR im Klartext und wurde damit
        // mitverschluesselt. Er gehoert aber NICHT in den Chiffrat —
        // er steht im Klartext des Records, damit der Empfaenger die
        // Nonce nachrechnen kann.
        //
        // Deshalb wird hier nur `klartext` verschluesselt und der
        // Nonce NACHHER vorangestellt. Verschuesselt man ihn mit,
        // stimmt die Entschlusselung nicht: der Empfaenger schneidet
        // die ersten acht Byte ab und bekommt 300 Byte Chiffrat, die
        // 308 sein muessten.
        let nonce = self.nonce(salt, seq)?;
        // M7: die Laenge im AAD ist die des KLARTEXTS, nicht die des
        // Chiffrats. RFC 5246 Abschnitt 7.4.7.1: "the length of the
        // ... plaintext". Hier wird die Laenge VOR dem Anhängen von
        // Nonce und Tag genommen — und das ist die Stelle, an der
        // man sie sonst nachher nimmt.
        let aad = aad(seq, typ, klartext.len());
        // M4: die Zieldlaenge ist bekannt — Nonce(8) + Klartext +
        // Tag(16) bei AES-GCM, Klartext + Tag(16) bei ChaCha.
        let mut raus: Vec<u8> = Vec::with_capacity(klartext.len() + 24);
        match self {
            Aead::ChaCha20Poly1305 => {
                let k: [u8; 32] = schluessel
                    .try_into()
                    .map_err(|_| Rec12Fehler::SchluesselFalsch)?;
                let n: [u8; 12] = nonce.try_into().map_err(|_| Rec12Fehler::NonceFalsch)?;
                let c = crate::chacha::aead_chiffre(&k, &n, &aad, klartext)
                    .map_err(|_| Rec12Fehler::Aead)?;
                raus.extend_from_slice(&c);
            }
            _ => {
                let (c, tag) = crate::aes::gcm_chiffre(schluessel, &nonce, &aad, klartext)
                    .map_err(|_| Rec12Fehler::Aead)?;
                raus.extend_from_slice(&seq.to_be_bytes());
                raus.extend_from_slice(&c);
                raus.extend_from_slice(&tag);
            }
        }
        Ok(raus)
    }

    /// Entschluesselt den Inhalt eines Records.
    ///
    /// M4: `chiffrat` ist schon auf seine Laenge geprueft, bevor hier
    /// etwas wächst. Fuer AES-GCM muss er mindestens Nonce (8) plus
    /// Tag (16) haben.
    pub fn dechiffre(
        &self,
        schluessel: &[u8],
        salt: &[u8],
        seq: u64,
        typ: u8,
        chiffrat: &[u8],
    ) -> Result<Vec<u8>, Rec12Fehler> {
        let explizit = if self.mit_explizitem_nonce() { 8 } else { 0 };
        if chiffrat.len() < explizit + self.tag_laenge() {
            return Err(Rec12Fehler::ZuKurz {
                gebraucht: explizit + self.tag_laenge(),
                war: chiffrat.len(),
            });
        }
        // DER EXPLIZITE NONCE AUS DEM RECORD, nicht die Sequenznummer.
        //
        // Das ist der Unterschied zu TLS 1.3 und der bequemste Fehler
        // daneben: in 1.3 ist die Nonce `iv XOR seq`, und wenn man das
        // hier so schreibt, sieht der Code richtig aus und scheitert an
        // jedem Record. Der Absender darf den expliziten Nonce frei
        // waehlen (RFC 5288 Abschnitt 3), und `tfo`-Server tun das
        // auch. Also: der Wert aus dem Record ist massgeblich.
        let nonce = if self.mit_explizitem_nonce() {
            let mut n = Vec::with_capacity(12);
            n.extend_from_slice(&salt[..4]);
            n.extend_from_slice(&chiffrat[..8]);
            n
        } else {
            self.nonce(salt, seq)?
        };
        let aad = aad(seq, typ, chiffrat.len() - explizit - self.tag_laenge());
        let innen: Vec<u8> = match self {
            Aead::ChaCha20Poly1305 => {
                let k: [u8; 32] = schluessel
                    .try_into()
                    .map_err(|_| Rec12Fehler::SchluesselFalsch)?;
                let n: [u8; 12] = nonce.try_into().map_err(|_| Rec12Fehler::NonceFalsch)?;
                crate::chacha::aead_dechiffre(&k, &n, &aad, chiffrat)
                    .map_err(|_| Rec12Fehler::Aead)?
            }
            _ => {
                let rest = &chiffrat[explizit..];
                let c = &rest[..rest.len() - self.tag_laenge()];
                let tag = &rest[rest.len() - self.tag_laenge()..];
                let t: [u8; 16] = tag.try_into().map_err(|_| Rec12Fehler::Aead)?;
                crate::aes::gcm_dechiffre(schluessel, &nonce, &aad, c, &t)
                    .map_err(|_| Rec12Fehler::Aead)?
            }
        };
        // Der entschluesselte Text ist der Klartext ohne weiteren
        // Abzug: der explizite Nonce stand im KLARTEXT des Records,
        // nicht im Chiffrat. `innen[explizit..]` abzuziehen waere der
        // Rest desselben Fehlers von der Sendeseite.
        Ok(innen)
    }

    /// Die 12-Byte-Nonce.
    ///
    /// * AES-GCM: `salt(4) || seq(8)`.
    /// * ChaCha20: `iv(12) XOR (0^32 || seq(8))` — RFC 7905 Abschnitt 2.
    fn nonce(&self, salt: &[u8], seq: u64) -> Result<Vec<u8>, Rec12Fehler> {
        match self {
            Aead::ChaCha20Poly1305 => {
                if salt.len() != 12 {
                    return Err(Rec12Fehler::NonceFalsch);
                }
                let mut n = [0u8; 12];
                n.copy_from_slice(salt);
                for i in 0..8 {
                    // `salt[4+i] XOR Byte 7-i von seq` — die Sequenz
                    // steht hinten, mit dem HOHEN Byte zuerst.
                    n[4 + i] ^= (seq >> (8 * (7 - i))) as u8;
                }
                Ok(n.to_vec())
            }
            _ => {
                if salt.len() != 4 {
                    return Err(Rec12Fehler::NonceFalsch);
                }
                let mut n = Vec::with_capacity(12);
                n.extend_from_slice(salt);
                n.extend_from_slice(&seq.to_be_bytes());
                Ok(n)
            }
        }
    }
}

/// Das AAD von TLS 1.2: 64-Bit-Sequenznummer, Typ, Version, Laenge.
///
/// 13 Byte. Die Laenge ist die des **Klartexts** (ohne expliziten
/// Nonce und ohne Tag) — RFC 5246 Abschnitt 7.4.7.1.
pub fn aad(seq: u64, typ: u8, klartext_laenge: usize) -> Vec<u8> {
    // M4: 13 Byte sind fest.
    let mut v: Vec<u8> = Vec::with_capacity(13);
    v.extend_from_slice(&seq.to_be_bytes());
    v.push(typ);
    v.extend_from_slice(&[0x03, 0x03]);
    v.extend_from_slice(&(klartext_laenge as u16).to_be_bytes());
    v
}

/// Fehler der TLS-1.2-Recordschicht.
///
/// Jede Variante traegt die Zahl, an der es liegen muss. Ein AEAD-Fehler
/// ohne Laenge sagt dem Aufrufer nicht, ob sein Schluessel zu kurz war
/// oder die Nachricht zu lang.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Rec12Fehler {
    ZuLang {
        grenze: usize,
    },
    ZuKurz {
        gebraucht: usize,
        war: usize,
    },
    SchluesselFalsch,
    NonceFalsch,
    Aead,
    UnbekannterTyp {
        bekommen: u8,
    },
    /// Die Entschluesselung schlug fehl. Getrennt von `Aead`, weil das
    /// ein BEFUND ist (falscher Schluessel, manipulierte Nachricht) und
    /// kein Rechenfehler.
    Authentifizierung,
}

impl fmt::Display for Rec12Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rec12Fehler::ZuLang { grenze } => {
                write!(f, "Record zu lang: ueber {grenze} Byte")
            }
            Rec12Fehler::ZuKurz { gebraucht, war } => {
                write!(f, "Record zu kurz: {gebraucht} Byte noetig, {war} bekommen")
            }
            Rec12Fehler::SchluesselFalsch => write!(f, "Schluessel hat die falsche Laenge"),
            Rec12Fehler::NonceFalsch => write!(f, "IV hat die falsche Laenge"),
            Rec12Fehler::Aead => write!(f, "Rechenfehler der AEAD"),
            Rec12Fehler::UnbekannterTyp { bekommen } => {
                write!(f, "unbekannter Inhaltstyp {bekommen}")
            }
            Rec12Fehler::Authentifizierung => {
                write!(f, "die Authentifizierung des Records schlug fehl")
            }
        }
    }
}

/// Baut einen Klartext-Record: Kopf und Inhalt.
pub fn baue_klartext(typ: u8, inhalt: &[u8]) -> Result<Vec<u8>, Rec12Fehler> {
    if inhalt.len() > MAX_KLAR {
        return Err(Rec12Fehler::ZuLang { grenze: MAX_KLAR });
    }
    // M4: 5 + inhalt.len(), beide bekannt.
    let mut raus: Vec<u8> = Vec::with_capacity(5 + inhalt.len());
    raus.push(typ);
    raus.extend_from_slice(&[0x03, 0x03]);
    raus.extend_from_slice(&(inhalt.len() as u16).to_be_bytes());
    raus.extend_from_slice(inhalt);
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M7: das AAD hat 13 Byte und die ANGEKUENDIGTE Laenge ist die des
    /// Klartexts.
    ///
    /// DER FEHLER, DEN DIESER TEST VERHINDERT: das 5-Byte-AAD aus
    /// `strom.rs` (TLS 1.3) hier zu benutzen. Es ist drei Zeilen
    /// kuerzer und sieht richtig aus; AES-GCM rechnet es in die
    /// Authentifizierung ein und schlaegt bei jedem Record fehl.
    #[test]
    fn das_aad_hat_13_byte_und_die_klartextlaenge() {
        let a = aad(0, typen::APPLICATION_DATA, 100);
        assert_eq!(
            a.len(),
            13,
            "M7: Sequenznummer(8) + Typ(1) + Version(2) + Laenge(2)"
        );
        assert_eq!(&a[8..9], &[typen::APPLICATION_DATA]);
        assert_eq!(&a[9..11], &[0x03, 0x03]);
        assert_eq!(&a[11..13], &100u16.to_be_bytes());
    }

    /// M7: die Sequenznummer steht vorne, gross zuerst — die
    /// umgekehrte Reihenfolge ergaebe ein anderes AAD und damit eine
    /// falsche Authentifizierung.
    #[test]
    fn die_sequenznummer_steht_vorn() {
        let a = aad(1, typen::HANDSHAKE, 0);
        assert_eq!(&a[..8], &1u64.to_be_bytes());
        let b = aad(256, typen::HANDSHAKE, 0);
        assert_eq!(&b[..8], &256u64.to_be_bytes());
        assert_ne!(&a[..8], &b[..8]);
    }

    /// M7: die Nonce-Regeln beider Verfahren, gegen die Norm.
    ///
    /// * AES-GCM (RFC 5288 Abschnitt 3): `salt(4) || explizit(8)`.
    /// * ChaCha20 (RFC 7905 Abschnitt 2): `iv(12) XOR (0^32 || seq)`.
    #[test]
    fn die_nonce_regeln_der_norm() {
        // AES-128-GCM: der Salt kommt aus dem Schluesselblock, der
        // Rest ist die Sequenznummer.
        let gcm = Aead::Aes128Gcm;
        let n = gcm
            .nonce(&[0xaa, 0xbb, 0xcc, 0xdd], 0x0102_0304_0506_0708)
            .expect("GCM-Nonce");
        assert_eq!(n.len(), 12);
        assert_eq!(&n[..4], &[0xaa, 0xbb, 0xcc, 0xdd], "der Salt steht vorn");
        assert_eq!(&n[4..], &0x0102_0304_0506_0708u64.to_be_bytes());

        // ChaCha20: der ganze IV wird mit der hinten eingefuegten
        // Sequenznummer verrechnet. seq = 1 heisst: nur das letzte
        // Byte des IV kippt.
        let cc = Aead::ChaCha20Poly1305;
        let n = cc.nonce(&[0u8; 12], 1).expect("ChaCha-Nonce");
        assert_eq!(n.len(), 12);
        assert_eq!(n[11], 1, "das letzte Byte ist die Sequenznummer");
        assert_eq!(&n[..11], &[0u8; 11], "der Rest bleibt 0 bei seq = 1");
        // Und seq = 0x0102_0304_0506_0708 steht mit dem hohen Byte
        // zuerst an Position 4.
        let n = cc
            .nonce(&[0u8; 12], 0x0102_0304_0506_0708)
            .expect("ChaCha-Nonce");
        assert_eq!(&n[4..], &0x0102_0304_0506_0708u64.to_be_bytes());
    }

    /// M7: der Schluesselblock hat KEINEN MAC-Slot.
    ///
    /// DER FEHLER, DEN DIESER TEST VERHINDERT: `schluesselblock()`
    /// mit `+ 2 * 20` fuer die beiden MAC-Schluessel. Bei einer
    /// AEAD-Suite gibt es keine; die 40 Byte verschieben jeden
    /// Schluessel und die Verbindung bricht beim ersten Record ab.
    #[test]
    fn kein_mac_slot_im_schluesselblock() {
        // client_write_key, server_write_key, client_write_IV,
        // server_write_IV — vier Felder, sonst nichts.
        assert_eq!(Aead::Aes128Gcm.schluesselblock(), 2 * 16 + 2 * 4);
        assert_eq!(Aead::Aes256Gcm.schluesselblock(), 2 * 32 + 2 * 4);
        assert_eq!(Aead::ChaCha20Poly1305.schluesselblock(), 2 * 32 + 2 * 12);
        for a in [Aead::Aes128Gcm, Aead::Aes256Gcm, Aead::ChaCha20Poly1305] {
            assert!(
                !a.braucht_mac(),
                "M7: eine AEAD-Suite hat keinen MAC-Schluessel"
            );
        }
    }

    /// M7: ein Record laeuft durch Verschluesseln und Entschluesseln.
    ///
    /// Und die Gegenprobe: ein veraenderter Chiffrat scheitert an der
    /// Authentifizierung. Ohne das haette ein Test wie dieser auch dann
    /// gruen ausgesehen, wenn `dechiffre` jeden beliebigen Muell als
    /// Klartext zurueckgaebe.
    #[test]
    fn hin_und_zurueck_und_gegenprobe() {
        let a = Aead::Aes128Gcm;
        let schluessel = [0x11u8; 16];
        let salt = [0x22u8; 4];
        let klartext: Vec<u8> = (0..300u32).map(|i| i as u8).collect();

        for seq in [0u64, 1, 2, 1_000_000] {
            let c = a
                .chiffre(&schluessel, &salt, seq, typen::APPLICATION_DATA, &klartext)
                .expect("chiffre");
            // 8 Byte explizite Nonce + Klartext + 16 Byte Tag.
            assert_eq!(c.len(), 8 + klartext.len() + 16, "M7: Recordlaenge");
            let p = a
                .dechiffre(&schluessel, &salt, seq, typen::APPLICATION_DATA, &c)
                .expect("dechiffre");
            assert_eq!(p, klartext, "M7: der Klartext kommt zurueck");
        }

        // Die Gegenprobe: ein Byte kippen.
        let mut c = a
            .chiffre(&schluessel, &salt, 0, typen::APPLICATION_DATA, &klartext)
            .expect("chiffre");
        c[100] ^= 0x01;
        assert_eq!(
            a.dechiffre(&schluessel, &salt, 0, typen::APPLICATION_DATA, &c),
            Err(Rec12Fehler::Aead),
            "M7: ein veraendertes Byte wird bemerkt"
        );
        // Und die falsche Sequenzzahl ebenso.
        let c = a
            .chiffre(&schluessel, &salt, 0, typen::APPLICATION_DATA, &klartext)
            .expect("chiffre");
        assert!(
            a.dechiffre(&schluessel, &salt, 1, typen::APPLICATION_DATA, &c)
                .is_err(),
            "M7: die Sequenzzahl gehoert zum AAD"
        );
    }

    /// M7: ChaCha20-Poly1305 genauso, und der Ciphertext traegt KEIN
    /// explizites Nonce-Feld.
    #[test]
    fn chacha_ohne_explizites_nonce() {
        let a = Aead::ChaCha20Poly1305;
        assert!(!a.mit_explizitem_nonce());
        let schluessel = [0x33u8; 32];
        let iv = [0x44u8; 12];
        let klartext = b"Hallo, TLS 1.2";
        let c = a
            .chiffre(&schluessel, &iv, 3, typen::APPLICATION_DATA, klartext)
            .expect("chiffre");
        assert_eq!(c.len(), klartext.len() + 16, "M7: nur der Tag kommt dazu");
        assert_eq!(
            a.dechiffre(&schluessel, &iv, 3, typen::APPLICATION_DATA, &c)
                .expect("dechiffre"),
            klartext
        );
    }

    /// M4/M3: die Grenzen.
    #[test]
    fn die_grenzen_werden_eingehalten() {
        // Zu lang zum Chiffrieren.
        let zu_lang = vec![0u8; MAX_KOMPRESSION + 1];
        assert_eq!(
            Aead::Aes128Gcm.chiffre(&[0u8; 16], &[0u8; 4], 0, typen::APPLICATION_DATA, &zu_lang),
            Err(Rec12Fehler::ZuLang {
                grenze: MAX_KOMPRESSION
            })
        );
        // Zu kurz zum Dechiffrieren: Nonce(8) + Tag(16) = 24 Byte.
        assert_eq!(
            Aead::Aes128Gcm.dechiffre(
                &[0u8; 16],
                &[0u8; 4],
                0,
                typen::APPLICATION_DATA,
                &[0u8; 23]
            ),
            Err(Rec12Fehler::ZuKurz {
                gebraucht: 24,
                war: 23
            })
        );
        // Und die Kopfgrenze im Klartextbau.
        assert_eq!(
            baue_klartext(typen::HANDSHAKE, &vec![0u8; MAX_KLAR + 1]),
            Err(Rec12Fehler::ZuLang { grenze: MAX_KLAR })
        );
        // Genau an der Grenze geht es noch.
        assert!(baue_klartext(typen::HANDSHAKE, &vec![0u8; MAX_KLAR]).is_ok());
    }
}
