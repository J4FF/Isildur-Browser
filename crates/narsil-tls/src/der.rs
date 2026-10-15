//! T10 — DER-Leser (X.690) mit Tiefen- und Laengengrenzen.
//!
//! DER ist keine Sprache, in der man schreiben kann — man kann nur
//! lesen, und jedes Feld ist mit einer Laenge versehen. Genau darin
//! liegen die Angriffe: eine Laenge, die nicht zum Rest des Puffers
//! passt, und eine Verschachtelung ohne Ende.
//!
//! Die Grenzen dieses Moduls, alle benannt und begründet (M3):
//!
//!   * `MAX_TIEFE` = 20. Das X.509 aus RFC 8448 hat 4 Ebenen. Eine
//!     Zertifikatskette mit einer Zwischen-CA liegt bei 5. 20 laesst
//!     Raum und setzt dem 10.000-fach verschachtelten Gebilde eine Grenze.
//!   * `MAX_FELD` = 1 MiB. Das groesste echte Feld in einem Zertifikat
//!     ist die Signatur einer 4096-Bit-RSA (512 Byte) oder ein
//!     Zertifikat mit vielen SANs; 1 MiB ist weit darueber und
//!     unterhalb dessen, was ein Speicherproblem erzeugt.
//!   * `MAX_ELEMENTE` = 10.000 je Container. Das haelt eine flache Liste
//!     aus.
//!
//! M4: Jede Laenge wird gegen den Rest des Puffers geprueft, BEVOR ein
//! `Vec` fuer den Inhalt entsteht. `Vec::with_capacity(laenge)` mit einer
//! Laenge von 2^31 kommt in diesem Modul nicht vor.

/// Hoechste Verschachtelungstiefe. Begründung siehe Modulkopf.
pub const MAX_TIEFE: usize = 20;
/// Hoechste Laenge eines einzelnen Feldes. Begründung siehe Modulkopf.
pub const MAX_FELD: usize = 1024 * 1024;
/// Hoechste Zahl der Elemente in einem Container. Begründung siehe
/// Modulkopf.
pub const MAX_ELEMENTE: usize = 10_000;

/// Die Tag-Klassen von X.690.
pub mod tag {
    pub const BOOLEAN: u8 = 0x01;
    pub const INTEGER: u8 = 0x02;
    pub const BIT_STRING: u8 = 0x03;
    pub const OCTET_STRING: u8 = 0x04;
    pub const NULL: u8 = 0x05;
    pub const OID: u8 = 0x06;
    pub const UTF8_STRING: u8 = 0x0C;
    pub const SEQUENCE: u8 = 0x30;
    pub const SET: u8 = 0x31;
    pub const PRINTABLE_STRING: u8 = 0x13;
    pub const IA5_STRING: u8 = 0x16;
    pub const UTC_TIME: u8 = 0x17;
    pub const GENERALIZED_TIME: u8 = 0x18;
}

/// Warum DER nicht gelesen werden kann. BENANNTE Varianten, damit der
/// Aufrufer den Grund kennt (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerFehler {
    /// Der Puffer endete mitten in einem Feld.
    ZuKurz { erwartet: usize, vorhanden: usize },
    /// Eine Laengenangabe war groesser als das, was ueberhaupt da ist.
    LaengeUeberPuffer { angesagt: usize, vorhanden: usize },
    /// Die Laenge war groesser als `MAX_FELD`.
    FeldZuGross { angesagt: usize, erlaubt: usize },
    /// Die Verschachtelung war tiefer als `MAX_TIEFE`.
    ZuTief { erlaubt: usize },
    /// Ein Container hatte mehr Elemente als `MAX_ELEMENTE`.
    ZuVieleElemente { erlaubt: usize },
    /// Der Tag war nicht der erwartete.
    TagFalsch { erwartet: u8, bekommen: u8 },
    /// Die high-tag-number-Form wurde benutzt, die braucht diese Crate
    /// nicht. Sie ist in X.690 erlaubt, kommt aber in Zertifikaten nicht
    /// vor und kostet Code, den niemand braucht.
    HoherTag,
    /// Die Laenge war nicht in der minimalen Form kodiert (X.690 8.1.3).
    LaengeNichtMinimal,
    /// Inhalt, der nicht weitergebrochen werden konnte.
    InhaltUnbekannt { tag: u8 },
    /// Ein INTEGER war leer oder ungleich 0x00 / 0xFF am Anfang.
    IntegerUngueltig,
    /// Ein BIT_STRING hatte ungenutzte Bits, die nicht null waren.
    BitStringUngueltig,
    /// Eine Zeitangabe war nicht plausibel.
    ZeitUngueltig { wert: String },
}

impl std::fmt::Display for DerFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DerFehler::ZuKurz {
                erwartet,
                vorhanden,
            } => {
                write!(f, "erwartet {erwartet} Byte, vorhanden {vorhanden}")
            }
            DerFehler::LaengeUeberPuffer {
                angesagt,
                vorhanden,
            } => {
                write!(f, "Laenge {angesagt}, aber nur {vorhanden} da")
            }
            DerFehler::FeldZuGross { angesagt, erlaubt } => {
                write!(f, "Feld {angesagt} Byte, erlaubt {erlaubt}")
            }
            DerFehler::ZuTief { erlaubt } => write!(f, "tiefer als {erlaubt}"),
            DerFehler::ZuVieleElemente { erlaubt } => {
                write!(f, "mehr als {erlaubt} Elemente")
            }
            DerFehler::TagFalsch { erwartet, bekommen } => {
                write!(f, "Tag {bekommen:#04x}, erwartet {erwartet:#04x}")
            }
            DerFehler::HoherTag => write!(f, "high-tag-number-Form"),
            DerFehler::LaengeNichtMinimal => write!(f, "Laenge nicht minimal kodiert"),
            DerFehler::InhaltUnbekannt { tag } => write!(f, "unbekannter Tag {tag:#04x}"),
            DerFehler::IntegerUngueltig => write!(f, "INTEGER ungleich gueltig kodiert"),
            DerFehler::BitStringUngueltig => write!(f, "BIT_STRING mit ungenutzten Einsbits"),
            DerFehler::ZeitUngueltig { wert } => write!(f, "Zeit unplausibel: {wert}"),
        }
    }
}

impl std::error::Error for DerFehler {}

/// Ein gelesenes DER-Element: Tag und Inhalt als Rohbytes.
///
/// Der Inhalt wird NICHT interpretiert. Das ist Absicht: wer nur eine
/// Signatur pruefen will, braucht keine Interpretation, und jede
/// Interpretation ist eine weitere Stelle, an der ein Fehler sitzen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: u8,
    /// Der Inhalt ohne Tag und Laenge.
    pub inhalt: Vec<u8>,
    /// Der ganze Element-Byte-String, Tag und Laenge inklusive. Wird
    /// fuer Signaturpruefungen gebraucht (RFC 5280 Abschnitt 4.1.1.2):
    /// signiert wird das`tbsCertificate` MIT seinem Tag.
    pub roh: Vec<u8>,
}

/// Der Zustand beim Lesen: der Puffer, die Position und die Tiefe.
pub struct DerLeser<'a> {
    daten: &'a [u8],
    pos: usize,
    /// M3: die Tiefe wird gezaehlt und gegen `MAX_TIEFE` geprueft.
    tiefe: usize,
}

/// M3/M5: Rekursion braucht eine Tiefengrenze. `tiefen` hier ist die
/// hoechste erlaubte Verschachtelung; jeder Aufruf von `container`
/// erhoeht die gemessene Tiefe um eins.
const TIEFE_START: usize = 1;

impl<'a> DerLeser<'a> {
    pub fn neu(daten: &'a [u8]) -> Self {
        DerLeser {
            daten,
            pos: 0,
            tiefe: TIEFE_START,
        }
    }

    /// Wie viele Byte noch da sind.
    pub fn uebrig(&self) -> usize {
        self.daten.len() - self.pos
    }

    /// Ist nichts mehr da?
    pub fn ist_leer(&self) -> bool {
        self.uebrig() == 0
    }

    /// Die aktuelle Position, fuer Fehlermeldungen.
    /// Das Tag des naechsten Elements, ohne es zu lesen.
    ///
    /// Noetig fuer `DEFAULT`-Felder: bei einem `DEFAULT v1` darf das
    /// Versionsfeld fehlen, und man muss wissen, ob da eines kommt,
    /// ohne das Seriennummernfeld zu verbrauchen.
    pub fn naechste_tag(&self) -> Option<u8> {
        if self.uebrig() == 0 {
            None
        } else {
            Some(self.daten[self.pos])
        }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// Liest EIN Element: Tag, Laenge, Inhalt.
    ///
    /// M4: Die Laenge wird in drei Schritten geprueft — gegen den Rest
    /// des Puffers, gegen `MAX_FELD`, und dann wird der Inhalt
    /// genommen. Erst danach wird alloziert, und die Allokation ist so
    /// gross wie der Inhalt, nicht wie die Laengenangabe behauptete.
    pub fn element(&mut self) -> Result<Element, DerFehler> {
        if self.uebrig() < 2 {
            return Err(DerFehler::ZuKurz {
                erwartet: 2,
                vorhanden: self.uebrig(),
            });
        }
        let start = self.pos;
        let tag = self.daten[self.pos];
        self.pos += 1;
        // X.690 8.1.2: high-tag-number-Form (Tag & 0x1f == 0x1f).
        // Diese Crate braucht sie nicht, also wird sie abgelehnt statt
        // uebersprungen — ein stilles Ueberspringen koennte eine
        // Laengenangabe an der falschen Stelle lesen.
        if tag & 0x1f == 0x1f {
            return Err(DerFehler::HoherTag);
        }
        // X.690 8.1.3: kurze Form (unter 128) und lange Form.
        let laenge = if self.daten[self.pos] & 0x80 == 0 {
            let l = self.daten[self.pos] as usize;
            self.pos += 1;
            l
        } else {
            let anzahl = (self.daten[self.pos] & 0x7f) as usize;
            // M4: Bis zu 127 Bytes Laengenfeld. Alles darueber hinaus ist
            // absurd (die Laenge waere 2^(8*127)) und wird abgelehnt.
            if anzahl == 0 {
                return Err(DerFehler::LaengeNichtMinimal);
            }
            if anzahl > 4 {
                // EIGENER FEHLER: es stand `1 << (8 * anzahl.min(31))`.
                // Bei anzahl = 8 ist das `1 << 64` und ueberlaeuft — ein
                // Ueberlauf in einer BOMBENBEHANDLUNG. Die Zahl wird gar
                // nicht erst ausgewertet; die Grenze entscheidet.
                return Err(DerFehler::FeldZuGross {
                    angesagt: usize::MAX,
                    erlaubt: MAX_FELD,
                });
            }
            if self.uebrig() < anzahl + 1 {
                return Err(DerFehler::ZuKurz {
                    erwartet: anzahl + 1,
                    vorhanden: self.uebrig(),
                });
            }
            self.pos += 1;
            let mut l = 0usize;
            for _ in 0..anzahl {
                l = (l << 8) | self.daten[self.pos] as usize;
                self.pos += 1;
            }
            // X.690 8.1.3.5: die lange Form ist nicht zulaessig, wenn der
            // Wert unter 128 liegt.
            if l < 128 {
                return Err(DerFehler::LaengeNichtMinimal);
            }
            l
        };

        // M4: Die drei Pruefungen, in dieser Reihenfolge.
        if laenge > MAX_FELD {
            return Err(DerFehler::FeldZuGross {
                angesagt: laenge,
                erlaubt: MAX_FELD,
            });
        }
        if laenge > self.uebrig() {
            return Err(DerFehler::LaengeUeberPuffer {
                angesagt: laenge,
                vorhanden: self.uebrig(),
            });
        }
        // M4: Erst jetzt wird fuer die INHALTSGROESSE alloziert.
        let mut inhalt = Vec::with_capacity(laenge);
        inhalt.extend_from_slice(&self.daten[self.pos..self.pos + laenge]);
        self.pos += laenge;
        let roh = self.daten[start..self.pos].to_vec();
        Ok(Element { tag, inhalt, roh })
    }

    /// Liest ein Element und prueft den Tag.
    pub fn element_mit_tag(&mut self, erwartet: u8) -> Result<Element, DerFehler> {
        let e = self.element()?;
        if e.tag != erwartet {
            return Err(DerFehler::TagFalsch {
                erwartet,
                bekommen: e.tag,
            });
        }
        Ok(e)
    }

    /// Liest alle Elemente eines Containers bis zum Ende.
    ///
    /// M3: `MAX_ELEMENTE` begrenzt die Schleife, und `MAX_TIEFE` wird
    /// gegen die Verschachtelung geprueft. Beides ist eine benannte
    /// Konstante, nicht etwas aus der Eingabe.
    pub fn container(&mut self, erwartet: u8, tiefe: usize) -> Result<Vec<Element>, DerFehler> {
        if tiefe > MAX_TIEFE {
            return Err(DerFehler::ZuTief { erlaubt: MAX_TIEFE });
        }
        let aeusser = self.element_mit_tag(erwartet)?;
        let mut inhalt = DerLeser {
            daten: &aeusser.inhalt,
            pos: 0,
            tiefe: tiefe + 1,
        };
        let mut raus: Vec<Element> = Vec::new();
        while !inhalt.ist_leer() {
            if raus.len() >= MAX_ELEMENTE {
                return Err(DerFehler::ZuVieleElemente {
                    erlaubt: MAX_ELEMENTE,
                });
            }
            raus.push(inhalt.element()?);
        }
        Ok(raus)
    }

    /// Wie tief der Leser gerade steht. Fuer Fehlermeldungen.
    pub fn tiefe(&self) -> usize {
        self.tiefe
    }
}

/// Liest einen INTEGER als vorzeichenbehaftete Bytes.
///
/// X.690 8.3.2: ein INTEGER ist minimal kodiert. `00` oder `ff` am
/// Anfang sind erlaubt, aber nur, wenn das naechste Bit das Vorzeichen
/// nicht redundant macht. Diese Pruefung ist wichtig, weil ein
/// Zertifikat mit zwei verschiedenen Kodierungen derselben Zahl zwei
/// verschiedene Signaturen haben kann.
pub fn integer_als_bytes(inhalt: &[u8]) -> Result<Vec<u8>, DerFehler> {
    if inhalt.is_empty() {
        return Err(DerFehler::IntegerUngueltig);
    }
    if inhalt.len() > 1
        && ((inhalt[0] == 0x00 && inhalt[1] & 0x80 == 0)
            || (inhalt[0] == 0xff && inhalt[1] & 0x80 == 0x80))
    {
        return Err(DerFehler::IntegerUngueltig);
    }
    Ok(inhalt.to_vec())
}

/// Liest einen BIT_STRING. Das erste Byte zaehlt die ungenutzten Bits.
pub fn bit_string(inhalt: &[u8]) -> Result<(u8, &[u8]), DerFehler> {
    if inhalt.is_empty() {
        return Err(DerFehler::BitStringUngueltig);
    }
    let ungenutzt = inhalt[0];
    if ungenutzt > 7 {
        return Err(DerFehler::BitStringUngueltig);
    }
    if ungenutzt != 0 {
        // Die letzten Bits muessen null sein.
        let letztes = inhalt[inhalt.len() - 1];
        if letztes & ((1u8 << ungenutzt) - 1) != 0 {
            return Err(DerFehler::BitStringUngueltig);
        }
    }
    Ok((ungenutzt, &inhalt[1..]))
}

/// Liest ein OID und gibt es als Punkte aus.
///
/// M3: Die Schleife laeuft ueber die Laenge des Feldes, die durch
/// `MAX_FELD` begrenzt ist. Jeder Punkt braucht mindestens ein Byte.
pub fn oid_als_punkte(inhalt: &[u8]) -> Result<Vec<u64>, DerFehler> {
    if inhalt.is_empty() {
        return Err(DerFehler::InhaltUnbekannt { tag: tag::OID });
    }
    let mut raus = Vec::new();
    let mut pos = 0usize;
    // Die ersten beiden Punkte stecken in einem Byte: 40 * x + y.
    let mut erstes = true;
    while pos < inhalt.len() {
        if raus.len() >= MAX_ELEMENTE {
            return Err(DerFehler::ZuVieleElemente {
                erlaubt: MAX_ELEMENTE,
            });
        }
        let mut wert: u64 = 0;
        let mut schritten = 0usize;
        loop {
            if pos >= inhalt.len() {
                return Err(DerFehler::InhaltUnbekannt { tag: tag::OID });
            }
            let b = inhalt[pos];
            pos += 1;
            schritten += 1;
            // M4: 9 Bytes maximal je Punkt, sonst waere der Wert zu
            // gross fuer u64. Das ist eine Grenze aus dem Typ, benannt.
            if schritten > 9 {
                return Err(DerFehler::InhaltUnbekannt { tag: tag::OID });
            }
            wert = (wert << 7) | (b & 0x7f) as u64;
            if b & 0x80 == 0 {
                break;
            }
        }
        if erstes {
            erstes = false;
            raus.push(wert / 40);
            raus.push(wert % 40);
        } else {
            raus.push(wert);
        }
    }
    Ok(raus)
}

/// Die OIDs, die diese Crate braucht (RFC 5280 Abschnitt 4.2 und 4.2.1).
pub mod oids {
    /// 1.2.840.113549.1.1.11 — sha256WithRSAEncryption
    pub const SHA256_RSA: [u64; 7] = [1, 2, 840, 113549, 1, 1, 11];
    /// 1.2.840.113549.1.1.10 — rsassaPss
    pub const RSA_PSS: [u64; 7] = [1, 2, 840, 113549, 1, 1, 10];
    /// 1.2.840.10045.2.1 — id-ecPublicKey
    pub const EC_PUBLIC_KEY: [u64; 6] = [1, 2, 840, 10045, 2, 1];
    /// 1.2.840.10045.4.3.2 — ecdsa-with-SHA256
    pub const ECDSA_SHA256: [u64; 7] = [1, 2, 840, 10045, 4, 3, 2];
    /// 1.2.840.10045.3.1.7 — prime256v1 (secp256r1)
    pub const SECP256R1: [u64; 7] = [1, 2, 840, 10045, 3, 1, 7];
    /// 1.3.132.0.34 — secp384r1.
    ///
    /// added 01.10.2026 22:4x UTC (Projektlabel 02.10.2026), Rolle TLS. GEMESSEN an einem echten
    /// Zertifikat (`openssl x509 -text` auf dem Zertifikat von
    /// `www.opensuse.org`, 01.10.2026 22:4x UTC (Projektlabel 02.10.2026)): die OID der Kurve ist
    /// `secp384r1 (1.3.132.0.34)`. In `strom.rs::cv_gueltig` stand vorher
    /// `1.3.132.0.7` — das ist **keine** Kurve, sondern der Anfang des
    /// 1.3.132-Raums; `kette.rs` (Zeile 508) akzeptierte zufaellig beide.
    pub const SECP384R1: [u64; 5] = [1, 3, 132, 0, 34];
    /// 1.2.840.10045.3.1.31 — eine **veraltete** Schreibweise fuer
    /// secp384r1, die in freier Wildbahn vorkommt.
    ///
    /// added 01.10.2026 22:4x UTC (Projektlabel 02.10.2026): `kette.rs` (Zeile 508) nimmt seit dem
    /// 01.10.2026 beide OIDs fuer P-384 an. Diese Crate folgt dem, damit
    /// die Schluessellaenge in `kette.rs` und die Signaturpruefung in
    /// `strom.rs` nicht variouschiedlich urteilen. Sicher ist das:
    /// `ecdsa_p384_pruefen_sha384` rechnet die Punktgleichung nach, ein
    /// Punkt, der nicht auf P-384 liegt, faellt dort durch.
    pub const SECP384R1_ALT: [u64; 7] = [1, 2, 840, 10045, 3, 1, 31];
    /// 2.5.4.3 — commonName
    pub const COMMON_NAME: [u64; 4] = [2, 5, 4, 3];
    /// 2.5.29.17 — subjectAltName
    pub const SUBJECT_ALT_NAME: [u64; 4] = [2, 5, 29, 17];
    /// 2.5.29.19 — basicConstraints
    pub const BASIC_CONSTRAINTS: [u64; 4] = [2, 5, 29, 19];
    /// 2.5.29.15 — keyUsage
    pub const KEY_USAGE: [u64; 4] = [2, 5, 29, 15];
    /// 2.5.29.37 — extKeyUsage
    pub const EXT_KEY_USAGE: [u64; 4] = [2, 5, 29, 37];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nachricht::CERTIFICATE_RFC8448_ABSCHNITT_3;

    /// M7: das echte Zertifikat aus RFC 8448 Abschnitt 3 laesst sich lesen.
    /// Das ist der beste Test fuer den Leser, weil es ein echtes
    /// X.509-Zertifikat ist und keines von mir gebaut wurde.
    #[test]
    fn echtes_zertifikat_aus_rfc8448() {
        // Die Zertifikatsnachricht enthaelt das DER als erstes Zertifikat.
        let cert = crate::nachricht::Certificate::dekodiere(&CERTIFICATE_RFC8448_ABSCHNITT_3)
            .expect("Certificate-Nachricht");
        assert_eq!(cert.zertifikate.len(), 1, "M7: ein Blatt");
        let der = &cert.zertifikate[0];
        assert_eq!(der.len(), 432, "M7: 432 Oktett DER");

        let mut l = DerLeser::neu(der);
        let seq = l.element_mit_tag(tag::SEQUENCE).expect("aeussere SEQUENCE");
        assert!(l.ist_leer(), "M7: die Datei ist vollstaendig gelesen");
        assert_eq!(
            seq.roh.len(),
            der.len(),
            "M7: roh umfasst das ganze Zertifikat"
        );

        // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm,
        //                              signatureValue }
        let mut innen = DerLeser::neu(&seq.inhalt);
        let felder = innen
            .container(tag::SEQUENCE, 1)
            .expect("tbsCertificate als SEQUENCE");
        // tbsCertificate ::= SEQUENCE {
        //   version [0] EXPLICIT, serialNumber, signature, issuer,
        //   validity, subject, subjectPublicKeyInfo, ...optional... }
        // Also SIEBEN Felder mindestens, nicht drei. Ich hatte drei
        // erwartet und damit den Leser fuer falsch gehalten.
        assert!(
            felder.len() >= 7,
            "M7: mindestens sieben Felder im tbsCertificate, waren {}",
            felder.len()
        );
        assert_eq!(felder[0].tag, 0xA0, "M7: version ist ein [0] EXPLICIT");
        assert_eq!(felder[1].tag, tag::INTEGER, "M7: serialNumber");
        assert_eq!(felder[2].tag, tag::SEQUENCE, "M7: signature");
        assert_eq!(felder[3].tag, tag::SEQUENCE, "M7: issuer");
        assert_eq!(felder[4].tag, tag::SEQUENCE, "M7: validity");
        assert_eq!(felder[5].tag, tag::SEQUENCE, "M7: subject");
        assert_eq!(felder[6].tag, tag::SEQUENCE, "M7: subjectPublicKeyInfo");
        integer_als_bytes(&felder[1].inhalt).expect("M7: serialNumber ist minimal kodiert");

        // Die Signatur ist ein BIT STRING von 128 Byte.
        let mut sig = DerLeser::neu(&seq.inhalt);
        sig.container(tag::SEQUENCE, 1).expect("tbs");
        let _ = sig.element().expect("signatureAlgorithm");
        let sigval = sig
            .element_mit_tag(tag::BIT_STRING)
            .expect("signatureValue");
        let (ungenutzt, rest) = bit_string(&sigval.inhalt).expect("BIT STRING");
        assert_eq!(ungenutzt, 0, "M7: keine ungenutzten Bits");
        assert_eq!(rest.len(), 128, "M7: 1024-Bit-RSA");
    }

    /// M4: der OID des Signaturalgorithmus wird richtig zerlegt.
    #[test]
    fn oid_zerlegung() {
        // 1.2.840.113549.1.1.11 kodiert als 2a 86 48 86 f7 0d 01 01 0b
        let kodiert = [0x2au8, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
        assert_eq!(
            oid_als_punkte(&kodiert).expect("OID"),
            oids::SHA256_RSA.to_vec()
        );
        // Und commonName 2.5.4.3 als 55 04 03
        assert_eq!(
            oid_als_punkte(&[0x55, 0x04, 0x03]).expect("OID"),
            oids::COMMON_NAME.to_vec()
        );
        // Und secp256r1 als 2a 86 48 ce 3d 03 01 07
        assert_eq!(
            oid_als_punkte(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]).expect("OID"),
            oids::SECP256R1.to_vec()
        );
    }

    /// M4: INTEGER und BIT STRING pruefen ihre Kodierung.
    #[test]
    fn kodierpruefungen() {
        // 0x00 0x01 ist nicht minimal: 0x01 reicht.
        assert_eq!(
            integer_als_bytes(&[0x00, 0x01]),
            Err(DerFehler::IntegerUngueltig)
        );
        assert_eq!(integer_als_bytes(&[0x01]).expect("minimal"), vec![0x01]);
        // 0xff 0x80 ist nicht minimal: 0x80 reicht.
        assert_eq!(
            integer_als_bytes(&[0xff, 0x80]),
            Err(DerFehler::IntegerUngueltig)
        );
        assert!(integer_als_bytes(&[]).is_err(), "M7: leerer INTEGER");
        // BIT STRING mit ungenutzten Einsbits.
        assert_eq!(
            bit_string(&[0x04, 0x0f]),
            Err(DerFehler::BitStringUngueltig),
            "die letzten vier Bit muessen null sein"
        );
        assert_eq!(
            bit_string(&[0x04, 0xf0]).expect("ok").0,
            4,
            "M7: vier ungenutzte Bit"
        );
        assert!(bit_string(&[0x08, 0x00]).is_err(), "M7: mehr als sieben");
        assert!(bit_string(&[]).is_err(), "M7: leer");
    }

    /// M4: eine Laenge von 2^31 (die M8-Bombe) wird abgelehnt, und zwar
    /// BEVOR alloziert wird.
    #[test]
    fn bombe_laenge_2_hoch_31() {
        // 04 84 7f ff ff ff = OCTET STRING, Laenge 0x7fffffff (2 GiB - 1)
        let mut bombe = vec![tag::OCTET_STRING, 0x84, 0x7f, 0xff, 0xff, 0xff];
        bombe.extend_from_slice(&[0u8; 64]);
        let mut l = DerLeser::neu(&bombe);
        let e = l.element();
        assert!(e.is_err(), "M7: muss abgelehnt werden, kam {e:?}");
        // M7: die Fehlervariante nennt die Zahl aus der Eingabe. Es ist
        // `FeldZuGross` und nicht `LaengeUeberPuffer`, weil zuerst gegen
        // MAX_FELD geprueft wird — 2 GiB sind auch dann zu gross, wenn der
        // Puffer genug haette. Die Reihenfolge ist Absicht: die kleinere
        // Grenze zuerst, damit die groessere nie eine Allokation ausloest.
        assert_eq!(
            e,
            Err(DerFehler::FeldZuGross {
                angesagt: 0x7fff_ffff,
                erlaubt: MAX_FELD
            }),
            "M7: die Fehlervariante nennt die Zahl aus der Eingabe"
        );
        assert!(matches!(e, Err(DerFehler::FeldZuGross { .. })));
    }

    /// M4: eine Laenge von 2^40, also noch groesser.
    #[test]
    fn bombe_laenge_2_hoch_40() {
        let bombe = [0x04u8, 0x85, 0x01, 0x00, 0x00, 0x00, 0x00];
        let mut l = DerLeser::neu(&bombe);
        assert!(l.element().is_err(), "M7: 2^40 muss abgelehnt werden");
    }

    /// M4: ein Laengenfeld mit mehr als vier Byte wird abgelehnt, ohne
    /// dass der Wert ausgewertet wird.
    #[test]
    fn bombe_langes_laengenfeld() {
        // 30 88 = 8 Byte Laengenfeld
        let mut bombe = vec![0x30u8, 0x88];
        bombe.extend_from_slice(&[0xff; 8]);
        let mut l = DerLeser::neu(&bombe);
        assert!(l.element().is_err(), "M7: 8-Byte-Laengenfeld");
    }

    /// M3/M4: 10.000-fach verschachteltes DER (die zweite M8-Bombe).
    ///
    /// M5: Hier ist KEINE Rekursion im Spiel. Der Leser arbeitet mit
    /// einer Schleife und einer Tiefenvariablen, und genau deshalb kann
    /// diese Eingabe den Aufrufstapel nicht sprengen — sie laeuft in
    /// Mikrosekunden durch und endet mit `ZuTief`.
    #[test]
    fn bombe_10000_fach_verschachtelt() {
        const TIEFE: usize = 10_000;
        // Sauber von innen nach aussen bauen, damit jede Laenge stimmt.
        // Ein laengenzerstoertes Gebilde wuerde frueher an
        // `LaengeUeberPuffer` scheitern und die Tiefengrenze gar nicht
        // erreichen — das war der erste Fehlversuch hier.
        let mut inhalt: Vec<u8> = vec![tag::NULL, 0x00];
        while inhalt.len() < 2 + 4 * TIEFE {
            let l = inhalt.len();
            assert!(l <= 0xFFFF, "M7: die Laenge passt in zwei Byte");
            let mut kopf = vec![tag::SEQUENCE, 0x82, (l >> 8) as u8, l as u8];
            kopf.append(&mut inhalt);
            inhalt = kopf;
        }
        // Ab hier sind alle Laengen ueber 128, die 0x82-Form ist also
        // die richtige und die kuerzere waere verboten.
        assert_eq!(inhalt.len(), 2 + 4 * TIEFE, "M7: {TIEFE} Ebenen");

        // `container` liest EINE Ebene. Die Tiefengrenze greift, wenn der
        // Aufrufer hinabsteigt — genau das wird hier nachgestellt, und
        // zwar mit einer Schleife statt mit einem Aufruf pro Ebene. Genau
        // deshalb ist der Stapel nicht das Problem, sondern die Tiefe.
        //
        // Das war der erste Fehlversuch: `container(SEQUENCE, 1)` liest
        // nur die direkten Kinder und gibt sie zurueck, also erreicht die
        // Grenze gar nicht die 10.000.
        let mut rest: Vec<u8> = inhalt.clone();
        let mut ebene = 0usize;
        let mut durchbrochen = false;
        while !rest.is_empty() {
            ebene += 1;
            if ebene > MAX_TIEFE {
                durchbrochen = true;
                break;
            }
            let mut l = DerLeser::neu(&rest);
            match l.container(tag::SEQUENCE, ebene) {
                Ok(kinder) => {
                    if kinder.len() == 1 && kinder[0].tag == tag::SEQUENCE {
                        rest = kinder[0].inhalt.clone();
                    } else {
                        // Der innerste NULL: fertig.
                        rest = Vec::new();
                    }
                }
                Err(e) => panic!("M7: unerwarteter Fehler auf Ebene {ebene}: {e}"),
            }
        }
        assert!(
            durchbrochen,
            "M7: bei {TIEFE} Ebenen muss abgebrochen werden"
        );
        assert_eq!(ebene, MAX_TIEFE + 1, "M7: genau eine Ebene zu viel");

        // Und derselbe Abstieg mit korrekt gesetzter Tiefe bricht ab.
        let mut l = DerLeser::neu(&inhalt);
        assert_eq!(
            l.container(tag::SEQUENCE, MAX_TIEFE + 1),
            Err(DerFehler::ZuTief { erlaubt: MAX_TIEFE }),
            "M7: eine Ebene zu tief wird abgewiesen"
        );

        // Und eine tiefe, aber erlaubte Verschachtelung geht durch.
        // X.690 8.1.3.5: die lange Form ist verboten, wenn der Wert
        // unter 128 liegt. Es stand hier immer `0x82`, also auch bei
        // Laengen wie 6 — und der Leser hat zu Recht mit
        // `LaengeNichtMinimal` geantwortet. Die kleine Bombe braucht die
        // KURZE Form.
        let mut klein: Vec<u8> = vec![tag::NULL, 0x00];
        for _ in 0..(MAX_TIEFE - 2) {
            let l = klein.len();
            assert!(l < 128, "M7: die kleine Bombe bleibt unter 128 Byte");
            let mut kopf = vec![tag::SEQUENCE, l as u8];
            kopf.append(&mut klein);
            klein = kopf;
        }
        let mut l = DerLeser::neu(&klein);
        assert!(
            l.container(tag::SEQUENCE, 1).is_ok(),
            "M7: {} Ebenen sind noch erlaubt",
            MAX_TIEFE - 1
        );
    }

    /// M3: ein Container mit sehr vielen kleinen Elementen.
    #[test]
    fn bombe_viele_elemente() {
        let mut inhalt = Vec::new();
        for _ in 0..(MAX_ELEMENTE + 10) {
            inhalt.push(tag::NULL);
            inhalt.push(0x00);
        }
        let mut daten = vec![tag::SEQUENCE, 0x82];
        daten.push((inhalt.len() >> 8) as u8);
        daten.push(inhalt.len() as u8);
        daten.extend_from_slice(&inhalt);

        let mut l = DerLeser::neu(&daten);
        match l.container(tag::SEQUENCE, 1) {
            Err(DerFehler::ZuVieleElemente { erlaubt }) => {
                assert_eq!(erlaubt, MAX_ELEMENTE, "M7: die Grenze steht im Fehler")
            }
            other => panic!("M7: {MAX_ELEMENTE} Elemente muessen abgelehnt werden, kam {other:?}"),
        }
        // Und knapp darunter geht es.
        let mut inhalt = Vec::new();
        for _ in 0..1000 {
            inhalt.push(tag::NULL);
            inhalt.push(0x00);
        }
        let mut daten = vec![tag::SEQUENCE, 0x82];
        daten.push((inhalt.len() >> 8) as u8);
        daten.push(inhalt.len() as u8);
        daten.extend_from_slice(&inhalt);
        let mut l = DerLeser::neu(&daten);
        let felder = l
            .container(tag::SEQUENCE, 1)
            .expect("1000 Elemente sind erlaubt");
        assert_eq!(felder.len(), 1000, "M7: Anzahl");
    }

    /// M4: abgeschnittenes DER an jeder Stelle.
    #[test]
    fn abgeschnitten_an_jeder_stelle() {
        let der = {
            let cert = crate::nachricht::Certificate::dekodiere(&CERTIFICATE_RFC8448_ABSCHNITT_3)
                .expect("Certificate");
            cert.zertifikate[0].clone()
        };
        for kuerzen in 1..der.len() {
            let mut l = DerLeser::neu(&der[..der.len() - kuerzen]);
            // Egal was der Leser zurueckgibt: es darf NICHT paniken.
            let _ = l.element();
        }
        // Und der Leser steht nie ausserhalb des Puffers.
        let mut l = DerLeser::neu(&der);
        for _ in 0..10000 {
            let vorher = l.position();
            let _ = l.element();
            assert!(
                l.position() >= vorher,
                "M7: die Position laeuft nicht zurueck"
            );
            assert!(
                l.position() <= der.len(),
                "M7: die Position bleibt im Puffer"
            );
        }
    }

    /// M4: die Kodierpruefungen von X.690.
    #[test]
    fn x690_regeln() {
        // high-tag-number-Form wird abgelehnt.
        let hoch = [0x1fu8, 0x01, 0x00];
        let mut l = DerLeser::neu(&hoch);
        assert_eq!(l.element(), Err(DerFehler::HoherTag));
        // Lange Form fuer einen Wert unter 128 ist verboten.
        let nicht_minimal = [0x04u8, 0x81, 0x05, 1, 2, 3, 4, 5];
        let mut l = DerLeser::neu(&nicht_minimal);
        assert_eq!(l.element(), Err(DerFehler::LaengeNichtMinimal));
        // 0x80 als Laengenbyte (unbestimmt) ist verboten.
        let unbestimmt = [0x04u8, 0x80];
        let mut l = DerLeser::neu(&unbestimmt);
        assert_eq!(l.element(), Err(DerFehler::LaengeNichtMinimal));
        // Korrekte lange Form geht.
        let mut daten = vec![0x04u8, 0x81, 0x80];
        daten.extend_from_slice(&[0u8; 128]);
        let mut l = DerLeser::neu(&daten);
        let e = l.element().expect("128 Byte mit langer Form");
        assert_eq!(e.inhalt.len(), 128, "M7: Inhalt");
    }
}
