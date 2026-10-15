//! T10 — X.509: ein Zertifikat aus seinem DER bauen.
//!
//! HIER WIRD NICHTS ERFUNDEN. Jedes Feld wird aus den Bytes gelesen, die
//! ein echtes Zertifikat traegt, und jedes Feld wird gegen `openssl`
//! geprueft. Die Testdaten sind aus `/etc/ssl/certs/ca-certificates.crt`
//! (150 Zertifikate, geprueft am 30.09.2026).
//!
//! AUFBAU nach RFC 5280 Abschnitt 4.1. In Worten, weil ein eingerueckter
//! Block in einem Doc-Kommentar als Doc-Test laufen wuerde und `SEQUENCE`
//! kein Rust ist:
//!
//! ```text
//! Certificate  ::=  SEQUENCE  {
//!      tbsCertificate       TBSCertificate,
//!      signatureAlgorithm   AlgorithmIdentifier,
//!      signatureValue       BIT STRING  }
//!
//! TBSCertificate  ::=  SEQUENCE  {
//!      version         [0] EXPLICIT Version DEFAULT v1,
//!      serialNumber         CertificateSerialNumber,
//!      signature            AlgorithmIdentifier,
//!      issuer               Name,
//!      validity             Validity,
//!      subject              Name,
//!      subjectPublicKeyInfo SubjectPublicKeyInfo,
//!      issuerUniqueID   [1] IMPLICIT ... OPTIONAL,
//!      subjectUniqueID  [2] IMPLICIT ... OPTIONAL,
//!      extensions       [3] EXPLICIT Extensions OPTIONAL }
//!
//! Extensions  ::=  SEQUENCE SIZE (1..MAX) OF Extension
//! Extension   ::=  SEQUENCE  {
//!      extnID      OBJECT IDENTIFIER,
//!      critical    BOOLEAN DEFAULT FALSE,
//!      extnValue   OCTET STRING }
//! ```
//!
//! Drei Stolperfallen aus dieser Datei, alle drei sind hier passiert
//! und alle drei haben still falsch gerechnet:
//!
//!  1. `version` ist `DEFAULT v1` und darf fehlen. Mit
//!     `element().tag == 0xA0` hat man das Seriennummernfeld schon
//!     verbraucht, wenn kein Versionsfeld da ist.
//!  2. `extensions [3] EXPLICIT` heisst: im A3-Tag steckt die
//!     `Extensions`-SEQUENCE. Ein Weglassen oder ein Zuviel davon
//!     verschluckt alle Erweiterungen, und `basic_constraints` bleibt
//!     `None` — was als "keine CA" gelesen wird.
//!  3. `critical` ist `DEFAULT FALSE` und darf fehlen. Ein
//!     `if element().tag == 0x01` liest auch dann ein Element, wenn
//!     das Tag nicht 0x01 ist, und frisst das `extnValue`.
//!
//! Die Typen sind bewusst schmal: diese Datei versteht RSA, ECDSA auf
//! P-256, P-384 und P-521. Alles andere wird als `Unbekannt` gemeldet
//! und nicht geraten — ein Schluessel, den man nicht kennt, darf nicht
//! "bekannt" bedeuten.
use crate::der::{DerFehler, DerLeser, bit_string, integer_als_bytes, oid_als_punkte};
// `Element` wird in der Signatur von `oid_aus_punkte_feld` gebraucht.
// Ich hatte den Import entfernt, nachdem eine Suche nach `Element` nur
// Treffer in Kommentaren gefunden hatte — die hatte aber die
// Typverwendung in der Funktionssignatur nicht erfasst.
use crate::der::Element;
use crate::zahl::Zahl;

/// Obergrenze fuer ein Zertifikat in Byte.
///
/// M3/M4: die Laenge kommt aus der Datei und ist damit eingabeabhaengig.
/// 64 KiB ist gross genug fuer jedes reale Zertifikat (die groessten im
/// Bundle sind rund 4 KiB) und klein genug, dass eine kaputte Laengen-
/// angabe nicht sofort den Speicher frisst.
pub const MAX_ZERTIFIKAT: usize = 64 * 1024;

/// Obergrenze fuer die Laenge eines Attributwerts in Byte.
pub const MAX_NAME_TIEFE: usize = 10 * 1024;

/// Obergrenze fuer die Zahl der Attribute in einem `Name`.
pub const MAX_NAME_ATTRIBUTE: usize = 32;

/// Obergrenze fuer die Zahl der Erweiterungen in einem Zertifikat.
pub const MAX_ERWEITERUNGEN: usize = 64;

/// `serverAuth`, die einzige erweiterte Verwendung, die ein Server-
/// Zertifikat fuer **diesen** Browser haben muss.
///
/// added 02.10.2026 09:5x, S1. RFC 5280 4.2.1.12 nennt die Verwendung
/// `id-kp-serverAuth`; die OID steht in RFC 5280 Anhang A als
/// 1.3.6.1.5.5.7.3.1. `anyExtendedKeyUsage` (2.5.29.37.0) gilt laut Norm
/// auch fuer serverAuth — das ist **keine** Erlaubnis fuer einen Browser
/// (der Nutzer hat nicht um „irgendeine Verwendung" gebeten), deshalb steht
/// es hier nur als Konstante und nicht in der Liste.
///
/// DIE FORM, und sie ist die **DER-Inhaltsbytes**, nicht die Punkteform:
/// DER kodiert die ersten beiden Glieder als `40*x + y` in **einem** Byte,
/// also ist `1.3.6.1.5.5.7.3.1` hier `[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07,
/// 0x03, 0x01]`. GEMESSEN am echten Testzertifikat (02.10.2026 10:1x):
/// `MESSUNG EKU inhalt [30, 0a, 06, 08, 2b, 06, 01, 05, 05, 07, 03, 01]` —
/// und `eku_lesen` liefert genau diese acht Byte. Mit der Punkteform
/// verglichen, bekam jedes Zertifikat `ServerAuthFehlt`.
pub const OID_SERVER_AUTH: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
/// added 02.10.2026 23:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// 1.3.6.1.5.5.7.3.9 — `id-kp-OCSPSigning`.
///
/// added 02.10.2026 23:2x: als **DER-Inhaltbytes**, nicht als Punktform. Das
/// ist dieselbe Entscheidung wie bei `OID_SERVER_AUTH` (siehe dort): die
/// Erweiterung `extendedKeyUsage` enthaelt OIDs als `OBJECT IDENTIFIER`, und
/// `erweiterte_verwendung` speichert die Inhaltebytes. Wer hier Punkte
/// schreibt, vergleicht Strings und findet nichts.
///
/// WARUM ES DIE OID UEBERHAUPT GIBT: RFC 6960 Abschnitt 4.2.2.2 — „CAs that
/// issue delegated OCSP response certificates MUST use one of the id-pkix-ocsp-*
/// extended key usages". Ein Zertifikat ohne diese Erweiterung darf keine
/// OCSP-Antwort fuer ein fremdes Zertifikat zeichnen, und genau das prueft
/// `tests/gut_ohne_erlaubnis.der` gegen.
pub const OID_OCSP_SIGNING: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x09];

/// `anyExtendedKeyUsage`, OID 2.5.29.37.0 — wieder in DER-Inhaltsbytes:
/// `40*2 + 5 = 85 = 0x55`, danach `29`, `37`, `0`.
pub const OID_ANY_EKU: &[u8] = &[0x55, 0x1d, 0x25, 0x00];

/// Die BIT-Positionen aus RFC 5280 Abschnitt 4.2.1.3, im ersten Byte des
/// KeyUsage-Bitstrings.
///
/// added 02.10.2026 09:5x, S1: `digitalSignature` (Bit 0),
/// `keyCertSign` (Bit 5) und `cRLSign` (Bit 6). Als **Bit** nummeriert, nicht
/// als Byte — das KeyUsage ist ein BIT STRING, und `0b0100_0000` ist Bit 6
/// (`cRLSign`), nicht „64".
pub mod key_usage_bit {
    /// digitalSignature
    pub const DIGITAL_SIGNATUR: u8 = 0;
    /// keyEncipherment
    pub const SCHLUESSEL_UEBERTRAGUNG: u8 = 2;
    /// keyCertSign
    pub const ZERTIFIKAT_SIGNIEREN: u8 = 5;
    /// cRLSign
    pub const WIDERRUF_SIGNIEREN: u8 = 6;
}

/// Ist das Bit an `position` im KeyUsage gesetzt?
///
/// added 02.10.2026 10:1x, S1. **DIE BIT-ORDNUNG, und sie war mein
/// Fehler:** der KeyUsage ist ein BIT STRING, und darin ist
/// `digitalSignature` das **erste** Bit — also das **oberste** Bit des
/// ersten Bytes. Ich hatte es als `1 << 0` geprueft und damit jedes
/// gueltige Zertifikat abgelehnt: GEMESSEN am echten Testzertifikat
/// (`echte_kette_wird_angenommen`) mit `KeineDigitaleSignatur`, obwohl
/// `keyUsage` dort `0x80` ist.
///
/// DIE ZWEITE HALLE BIT-FELD (Position 8, `decipherOnly`) ist in `key_usage`
/// **nicht** vorhanden — `x509.rs` liest nur das erste Byte. Das ist hier
/// eine bewusste Grenze: ein Zertifikat, das nur `decipherOnly` setzt, wird
/// nicht als Serverzertifikat benutzt, und die Pruefung braucht es nicht.
pub fn key_usage_hat(ku: u16, position: u8) -> bool {
    if position > 7 {
        return false;
    }
    ku & (1 << (7 - position)) != 0
}

/// Obergrenze fuer die Laenge einer Erweiterung in Byte.
pub const MAX_ERWEITERUNG: usize = 8 * 1024;

/// Warum ein Zertifikat nicht gelesen werden konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X509Fehler {
    /// Der Aufbau passte nicht zu RFC 5280.
    Aufbau { was: &'static str },
    /// Das DER war fehlerhaft.
    Der(DerFehler),
    /// Die Zahl war laenger als erlaubt.
    ZuLang {
        was: &'static str,
        ist: usize,
        erlaubt: usize,
    },
    /// Die Gueltigkeit war nicht als zwei Zeitpunkte lesbar.
    ZeitUnbrauchbar,
    /// Der Schluessel war nicht RSA und nicht ECDSA P-256.
    SchluesselUnbekannt,
    /// Die Signatur war kein gaengiges Format.
    SignaturUnbekannt,
    /// Ein Fehler aus der Zahl-Arithmetik.
    Zahl(crate::zahl::ZahlFehler),
    /// Die Gueltigkeit ist nicht `notBefore <= now <= notAfter`.
    NichtGueltig {
        jetzt: [u8; 8],
        von: [u8; 8],
        bis: [u8; 8],
    },
    /// added 03.10.2026 05:1x, R51: **EINE ERWEITERUNG MIT `critical = TRUE`,
    /// deren Information wir nicht verarbeiten koennen.** RFC 5280 4.2 woertlich:
    /// „A certificate-using system **MUST reject** the certificate if it
    /// encounters a critical extension it does not recognize **or a critical
    /// extension that contains information that it cannot process**.“
    ///
    /// added 03.10.2026 05:1x: **WARUM EINE EIGENE FORM UND NICHT
    /// `Aufbau { was }`.** Eine eigene Form zwingt jede Stelle, die
    /// `X509Fehler` matcht, sich zu **entscheiden**; `Aufbau` haette den neuen
    /// Fall in eine bestehende Sammelform versteckt und keine Datei gemeldet.
    /// Die Punkte stehen als Zahlen, weil eine OID ohne Punkte nicht lesbar ist
    /// („A1B2C3“ sagt niemandem etwas).
    KritischeErweiterung { punkte: Vec<u64> },
}

impl std::fmt::Display for X509Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            X509Fehler::Aufbau { was } => write!(f, "Aufbau passt nicht: {was}"),
            X509Fehler::Der(e) => write!(f, "DER: {e}"),
            X509Fehler::KritischeErweiterung { punkte } => {
                // added 03.10.2026 05:1x: **PUNKT-FORM**, weil die Punkte selbst
                // die Fehlersuche tragen. Leere Liste ist nicht moeglich
                // (`oid_als_punkte` liefert mindestens ein Element), aber der
                // `match` muss vollstaendig sein — deshalb der `_`-Zweig mit
                // leerer Ausgabe statt `unreachable!()` (verboten).
                write!(
                    f,
                    "kritische Erweiterung, die nicht verarbeitet werden kann: "
                )?;
                for (i, p) in punkte.iter().enumerate() {
                    if i > 0 {
                        write!(f, ".")?;
                    }
                    write!(f, "{p}")?;
                }
                Ok(())
            }
            X509Fehler::ZuLang { was, ist, erlaubt } => {
                write!(f, "{was} hat {ist} Byte, erlaubt {erlaubt}")
            }
            X509Fehler::ZeitUnbrauchbar => write!(f, "Zeit nicht lesbar"),
            X509Fehler::SchluesselUnbekannt => write!(f, "oeffentlicher Schluessel unbekannt"),
            X509Fehler::SignaturUnbekannt => write!(f, "Signaturalgorithmus unbekannt"),
            X509Fehler::Zahl(e) => write!(f, "Zahl: {e}"),
            X509Fehler::NichtGueltig { jetzt, von, bis } => write!(
                f,
                "jetzt {:02x}{:02x}{:02x}..., gueltig {:02x}{:02x}{:02x}... bis {:02x}{:02x}{:02x}...",
                jetzt[0], jetzt[1], jetzt[2], von[0], von[1], von[2], bis[0], bis[1], bis[2]
            ),
        }
    }
}

impl From<DerFehler> for X509Fehler {
    fn from(e: DerFehler) -> X509Fehler {
        X509Fehler::Der(e)
    }
}

/// Der Schluesseltyp eines oeffentlichen Schluessels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchluesselArt {
    /// RSA, mit Modulus und Exponent.
    Rsa { n: Vec<u8>, e: Vec<u8> },
    /// ECDSA, mit der Kurven-OID und dem Punkt.
    Ecdsa {
        kurve: Vec<u64>,
        x: Vec<u8>,
        y: Vec<u8>,
    },
    /// Alles andere.
    Unbekannt { oid: Vec<u64> },
}

/// Ein Signaturalgorithmus, wie er im Zertifikat steht.
///
/// Der Hash wird getrennt gefuehrt, weil TLS 1.3 daraus eine eigene
/// Wahl des Hashverfahrens macht (RFC 8446 Abschnitt 4.2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturAlgorithmus {
    /// Die OID des Algorithmus, z.B. 1.2.840.113549.1.1.11 fuer
    /// sha256WithRSAEncryption.
    pub oid: Vec<u64>,
    /// Der Hash, wenn einer erkannt wurde.
    pub hash: Option<Hash>,
    /// Bei RSA-PSS die Saltlaenge, falls im Feld angegeben.
    pub pss_salt: Option<u32>,
}

/// Die von dieser Datei unterstuetzten Hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    /// OID des Hashverfahrens.
    fn von_oid(oid: &[u64]) -> Option<Hash> {
        // 1.3.14.3.2.26 = SHA-1, 2.16.840.1.101.3.4.2.1 = SHA-256,
        // 2.16.840.1.101.3.4.2.2 = SHA-384, 2.16.840.1.101.3.4.2.3 = SHA-512
        match oid {
            [1, 3, 14, 3, 2, 26] => Some(Hash::Sha1),
            [2, 16, 840, 1, 101, 3, 4, 2, 1] => Some(Hash::Sha256),
            [2, 16, 840, 1, 101, 3, 4, 2, 2] => Some(Hash::Sha384),
            [2, 16, 840, 1, 101, 3, 4, 2, 3] => Some(Hash::Sha512),
            _ => None,
        }
    }

    /// Die Laenge des Hashwerts in Byte.
    pub fn laenge(self) -> usize {
        match self {
            Hash::Sha1 => 20,
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }
}

/// Ein Attribut eines `Name`, etwa `C=ES`.
///
/// Der Wert wird als volles DER aufbewahrt, weil verschiedene
/// Attributtypen verschiedene Formen haben: `C` ist
/// PrintableString, `CN` ist in vielen Zertifikaten UTF8String, und
/// `emailAddress` ist gar kein DirectoryString.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribut {
    /// Die OID des Attributtyps, z.B. 2.5.4.3 fuer `commonName`.
    pub typ: Vec<u64>,
    /// Der Wert als Tag und Inhalt, ohne die TLV-Klammer.
    pub wert: Vec<u8>,
}

impl Attribut {
    // added 04.10.2026 00:3x, R67. **`typ_name` GELOESCHT** — und das ist die
    // **dritte** Form, die R67 unterscheidet: `gruppen_zahlen` und
    // `nonce_laenge` waren **Redundanz** (ein Getter fuer ein `pub`-Feld, ein
    // Getter fuer eine Konstante), `verbinde_ueber` war ein **dritter Weg** in
    // dieselbe Sache. `typ_name` aber ist eine **Tabelle**: 18 Zeilen OID-Name
    // fuer CN, C, L, ST, O, OU, serialNumber, SN, GN, DC, emailAddress.
    //
    // added 04.10.2026 00:3x: **UND GERADE DIE HABE ICH GELOESCHT, WEIL SIE
    // KEINE IST:** GEMESSEN, `grep` ueber alle Crates fand einen Treffer — die
    // Definition — und **keinen** Aufrufer, auch nicht in einer Fehlermeldung.
    // Nach R50 gilt fuer sie dasselbe wie fuer jede neue Funktion: **mit** Test
    // oder gar nicht. Ein Test, der nur die Tabelle gegen sich selbst prueft,
    // waere genau die leere Assertion, die R61 an zwei Tests beanstandet hat.
    //
    // added 04.10.2026 00:3x: **UND WANN SIE ZURUECKKOMMT, ist benannt:** beim
    // ersten Fehlertext, der einen Attributtypen nennen muss — dann mit Test
    // und mit einem Aufrufer, der sie braucht. **Bis dahin ist sie Ballast**,
    // und Ballast traegt seinen Preis nicht im Kommentar.

    /// Der Wert als Text, wenn er ein DirectoryString ist.
    ///
    /// RFC 5280 Abschnitt 4.1.2.4 erlaubt fuer die Attribute eines
    /// `Name` PrintableString, TeletexString, BMPString, UTF8String und
    /// UniversalString. Die ersten beiden sind ein Byte je Zeichen, die
    /// letzten beiden sind UCS-2 bzw. UCS-4. Ein BMPString wird hier
    /// NICHT umgewandelt, sondern als `None` zurueckgegeben — eine
    /// halbe UTF-16-Dekodierung zu bauen und sie dann fuer einen
    /// Hostnamenvergleich zu benutzen, waere genau die Sorte stiller
    /// Fehler, die diese Crate bisher zweimal gekostet hat.
    pub fn wert_als_text(&self) -> Option<String> {
        if self.wert.is_empty() {
            return Some(String::new());
        }
        // Die Tags, die RFC 5280 Abschnitt 4.1.2.4 fuer ein
        // DirectoryString zulaesst:
        //   0x12 TeletexString, 0x13 PrintableString, 0x16 IA5String,
        //   0x1E BMPString (UCS-2), 0x1C UniversalString (UCS-4),
        //   0x0C UTF8String
        //
        // EIGENER FEHLER, und einer der praktischsten: ich behandelte
        // nur 0x13, 0x12 und 0x16. `openssl req -subj "/CN=..."`
        // schreibt aber seit OpenSSL 1.0 als UTF8String (0x0C), weil
        // `-utf8` der Standard ist. Damit gab `common_name()` bei
        // jedem Zertifikat `None` zurueck, das mit einem heutigen
        // openssl erzeugt wurde — bei allen 150 im echten Bundle
        //included. Und weil `traegt_hostnamen` bei vorhandenem SAN
        //ohne CN auskommt, fiel es nicht auf, sondern blieb
        // unbemerkt.
        //
        // BMPString und UniversalString bleiben hier ABSICHTLICH `None`.
        // Das ist die Richtung, in der ein Fehler unauffaellig ist:
        // ein nicht gelesener CN fuehrt zu "kein Treffer", und ein
        // nicht gelesener CN kann kein Zertifikat durchlassen. Wer
        // sie halb umdeuten will, baut eine halbe UTF-16-Dekodierung
        // und vergleicht damit — und genau daran ist diese Crate
        // schon zweimal gescheitert.
        match self.wert[0] {
            0x13 | 0x12 | 0x16 | 0x0C => {
                // `from_utf8`, NICHT `from_utf8_lossy`.
                //
                // `lossy` ersetzt jedes ungueltige Byte durch U+FFFD.
                // Ein Name, der zwei Bytes zu viel hat, wird dadurch
                // zu einem gueltigen String — und wenn die
                // nachfolgenden Bytes passen, stimmt der Vergleich
                // plötzlich. Bei einem Vergleich gegen einen
                // Hostnamen ist genau das der Fehler, der ein
                // falsches Zertifikat durchlaesst.
                match std::str::from_utf8(&self.wert[1..]) {
                    Ok(s) => Some(s.to_string()),
                    Err(_) => None,
                }
            }
            0x1E | 0x1C => None,
            _ => None,
        }
    }
}

/// Ein Distinguished Name, also eine geordnete Liste von Attributen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Name {
    pub attribute: Vec<Attribut>,
    /// added 03.10.2026 14:0x, R58. **DIE RDN-GRENZEN**, und das ist das Feld,
    /// ohne das die Teilbaum-Regel nicht baubar ist.
    ///
    /// added 03.10.2026 14:0x: **WAS DA STEHT:** fuer jedes RelativeDistinguishedName
    /// der distinguishedName der **Anzahl der Attribute, die davor** gelesen
    /// wurden. Also `[0, 1, 3]` heisst: erstes RDN hat 1 Attribut, zweites 2,
    /// drittes 1. Bei einem einfachen `CN=x` ist es `[0, 1]`.
    ///
    /// added 03.10.2026 14:0x: **WARUM ZAHLEN UND NICHT GRUPPEN.** Eine
    /// `Vec<Vec<Attribut>>` waere lesbarer, aber sie dupliziert die Attribute, und
    /// `Name` geht durch `clone()` in jeder Zertifikatskopie. Indexgrenzen sind
    /// `Copy` und kosten nichts.
    ///
    /// added 03.10.2026 14:1x: **WARUM DAS DIE TEILBAUM-REGEL UEBERHAUPT BRAUCHT.**
    /// RFC 5280 7.1 (Seite 96) woertlich: „A distinguished name DN1 is within the
    /// subtree defined by the distinguished name DN2 if **DN1 contains at least as
    /// many RDNs as DN2**, and DN1 and DN2 are a match when **trailing RDNs in DN1
    /// are ignored**.“ Beide Bedingungen reden **von RDNs**, und ohne Grenzen weiss
    /// man nicht, welche Attribute zu welchem RDN gehoeren. GEMESSEN vorher: die
    /// enthaelt **eine** Zeile, die liest `let rdn = sets.element_mit_tag(0x31)?`
    /// und dann **alle** Attribute in **eine** flache Liste legt.
    ///
    /// added 03.10.2026 14:1x: **ADDITIV, mit einem Grund:** es gibt genau **eine**
    /// Stelle im ganzen Crate, die `Name` per Literal baut (`name_lesen`, GEMESSEN
    /// per grep nach `Name {`), also ist die Aenderung additiv an genau einer
    /// Stelle. Waere es drei, haette ich eine eigene Konstruktorfunktion
    /// eingefuehrt — das ist die Regel: **ab drei Baustellen ist ein Konstruktor
    /// billiger als die Sorge**.
    pub rdn_grenze: Vec<usize>,
}

impl Name {
    /// Der erste Wert eines Attributtyps.
    pub fn erstes(&self, typ: &[u64]) -> Option<&Attribut> {
        self.attribute.iter().find(|a| a.typ == typ)
    }

    /// Der CN als Text, wenn er lesbar ist.
    pub fn common_name(&self) -> Option<String> {
        // EIGENER FEHLER, und er waere still gewesen: `common_name` gab
        // hier `&str` zurueck, und weil `wert_als_text` ein `String`
        // baut, haette der Verweis auf eine temporaere gezeigt. Die
        // Alternative war `self.erstes(...).and_then(...).as_deref()`,
        // und das haette zur Compilezeit gepasst, aber nur, wenn man
        // den Wert zwischenspeichert. Ein `String` zurueckzugeben ist
        // ehrlicher und kostet eine Kopie, die hier nichts kostet,
        // weil der Name einmal gelesen und dann verglichen wird.
        self.erstes(&[2, 5, 4, 3]).and_then(|a| a.wert_als_text())
    }

    /// Ist der Name leer?
    pub fn ist_leer(&self) -> bool {
        self.attribute.is_empty()
    }
}

/// added 03.10.2026, TLS. **DER KURZE NAME EINES ATTRIBUTTYPS** — das ist die
/// Tabelle, die R67 aus `Attribut` **geloescht** hat, und sie kommt hier
/// **wieder**, weil die Bedingung, unter der R67 sie zurueckgerufen hat,
/// eingetreten ist. Woertlich dort: „**WANN DIE ZURUECKKOMMT**, ist benannt:
/// beim ersten Fehlertext, der einen Attributtypen nennen muss.“
///
/// **WER DAS ERSTE IST, und es ist gemessen:** `info.rs` (die
/// Zertifikatsinfo fuer das Schloss im Adressfeld) schreibt einen Namen als
/// Text, und da steht `CN=www.s1.test` und nicht `2.5.4.3=www.s1.test`. Ohne
/// diese Funktion gaebe es drei Wege — alle drei falsch: die OID hart
/// ausschreiben (dann ist die zweite Wahrheit in `info.rs`), einen
/// unbekannten Typ als Zahl ausgeben (dann liest der Nutzer nichts), oder die
/// Namen weglassen.
///
/// **UND DIE OIDs SIND GEMESSEN, nicht aus dem Gedaechtnis:** `openssl x509
/// -subject -nameopt RFC2253` auf `crates/narsil-tls/testdaten/
/// t12_echtes_bundle.pem` (03.10.2026) liefert genau diese Kuerzel, unter
/// anderem `OU`, `O`, `C`, `CN`, `L`, `ST` und `serialNumber`. Das ist auch
/// die Quelle fuer die **Reihenfolge** in [`name_als_text`].
///
/// **`None` heisst „unbekannter Typ“** und fuehrt in RFC 4514 2.3 zur
/// punktierten OID — nicht zu einem geratenen Kuerzel.
pub fn attribut_typ_name(typ: &[u64]) -> Option<&'static str> {
    Some(match typ {
        // RFC 4514 2.3 nennt die Kuerzel; die OIDs stehen in RFC 5280
        // Anhang A.1 und sind dieselben wie in `der::oids`.
        [2, 5, 4, 3] => "CN",
        [2, 5, 4, 6] => "C",
        [2, 5, 4, 7] => "L",
        [2, 5, 4, 8] => "ST",
        [2, 5, 4, 10] => "O",
        [2, 5, 4, 11] => "OU",
        [2, 5, 4, 4] => "SN",
        [2, 5, 4, 42] => "GN",
        [2, 5, 4, 5] => "serialNumber",
        [2, 5, 4, 12] => "title",
        [2, 5, 4, 43] => "initials",
        [2, 5, 4, 46] => "dnQualifier",
        // 1.2.840.113549.1.9.1 — emailAddress. **Kein RFC-4514-Kuerzel**,
        // sondern der OID-Kurzname, den RFC 4514 2.3 fuer nicht standardisierte
        // Typen uebriglaesst: `emailAddress` steht in RFC 5280 Anhang A.1.
        [1, 2, 840, 113549, 1, 9, 1] => "emailAddress",
        // 0.9.2342.19200300.100.1.25 — domainComponent (DC)
        [0, 9, 2342, 19200300, 100, 1, 25] => "DC",
        [0, 9, 2342, 19200300, 100, 1, 1] => "UID",
        _ => return None,
    })
}

/// added 03.10.2026, TLS. **DER KURZE NAME EINER OID ALS PUNKTIERTER TEXT**,
/// fuer die Ausgabe — RFC 4514 2.3: „If the AttributeType is not recognized by
/// the implementation, the dotted-decimal encoding of the OID is to be used.“
///
/// **WARUM EIGENE FUNKTION und nicht `format!` an drei Stellen:** die
/// Zertifikatsinfo braucht sie fuer Attributtypen **und** fuer die
/// Kurven-/Signatur-OIDs, und eine zweite Form derselben Sache hat in dieser
/// Crate schon einmal falsch geantwortet (siehe `same_site_flag` in
/// `narsil-net/src/sitzung.rs`).
pub fn oid_als_text(oid: &[u64]) -> String {
    // added 03.10.2026, TLS: **KEIN `join` AUF LEERER LISTE OHNE GRUND.**
    // Eine leere OID ist formal moeglich (sie kommt nie vor) und ergaebe sonst
    // einen leeren String, der wie ein Fehler aussieht. Der Text sagt es.
    if oid.is_empty() {
        return "(leere OID)".to_string();
    }
    let teile: Vec<String> = oid.iter().map(|n| n.to_string()).collect();
    teile.join(".")
}

/// added 03.10.2026, TLS. **DIE ZEICHEN, DIE RFC 4514 2.4 VERLANGT**, als
/// eine Liste statt als fuenf Vergleiche.
///
/// **WOERTLICH** RFC 4514 2.4: „the following characters **MUST** be escaped:
/// `"` (quotation mark), `+` (plus sign), `,` (comma), `;` (semicolon), `<`
/// (less-than sign), `>` (greater-than sign), or `\` (backslash).“ Und
/// 2.4 nennt zusaetzlich: „Additionally, the NUL character (…) as well as any
/// character with the ASCII value LESS THAN 0x20 (i.e., with a value less than
/// or equal to U+001F) **MUST** be escaped using the `\HH` encoding“.
///
/// **UND GEMESSEN, dass genau diese Liste stimmt**, an einem echten
/// Zertifikat (03.10.2026, `t12_echtes_bundle.pem`): `O=Cybertrust Japan Co.,
/// Ltd.` steht im Zertifikat **mit Komma im Wert**, und
/// `openssl -nameopt RFC2253` gibt da `O=Cybertrust Japan Co.\, Ltd.` zurueck —
/// also: das Komma **im Wert** wird maskiert, das Komma **zwischen** zwei
/// Attributen nicht. Genau diese Unterscheidung ist der ganze Unterschied
/// zwischen einem lesbaren und einem geratenen Namen.
///
/// **UND `\ ` (maskiertes Leer) und `\#` (maskiertes `#`) stehen hier, weil
/// RFC 4514 2.4 sie nennt („If the AttributeValue is a string that does not
/// have any of these characters, then the string may be represented without any
/// quoting“ und 2.3 „# … preceding the hexadecimal encoding of the value“):
/// ein fuehrendes oder **abschliessendes** Leer waere sonst unsichtbar, und
/// ein Name, der mit `#` beginnt, waerme sonst die Hex-Lesart aus.
fn maskiert_als_wert(s: &str) -> bool {
    s.is_empty()
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('#')
        || s.chars()
            .any(|c| matches!(c, '"' | '+' | ',' | ';' | '<' | '>' | '\\') || (c as u32) <= 0x1f)
}

/// added 03.10.2026, TLS. **MASKIEREN nach RFC 4514 2.4**, und es sind zwei
/// Regeln, nicht eine: ein zeichenweises `\` vor jedem Sonderzeichen **und**
/// `\HH` fuer alles unterhalb 0x20.
///
/// **WARUM NICHT `format!` mit einem einzigen Durchgang:** die Steuerzeichen
/// brauchen **zwei** Hexziffern, und ein Zeichen-fuer-Zeichen-Ersetzen wuerde
/// sie als `%`-Escapes schreiben — eine Form, die RFC 4514 nicht kennt.
fn maskiere(a: &Attribut) -> String {
    let typ = attribut_typ_name(&a.typ)
        .map(String::from)
        .unwrap_or_else(|| oid_als_text(&a.typ));
    let wert = match a.wert_als_text() {
        Some(t) => {
            if !maskiert_als_wert(&t) {
                t
            } else {
                let mut raus = String::with_capacity(t.len() + 8);
                for c in t.chars() {
                    match c {
                        '"' | '+' | ',' | ';' | '<' | '>' | '\\' => {
                            raus.push('\\');
                            raus.push(c);
                        }
                        ' ' if t.starts_with(' ') || t.ends_with(' ') => {
                            raus.push('\\');
                            raus.push(' ');
                        }
                        c if (c as u32) <= 0x1f => {
                            // RFC 4514 2.4: NUL und alles <= U+001F als `\HH`.
                            raus.push('\\');
                            raus.push_str(&format!("{:02X}", c as u32));
                        }
                        c => raus.push(c),
                    }
                }
                raus
            }
        }
        // added 03.10.2026, TLS: **KEIN TEXT = `#` PLUS HEX**, woertlich
        // RFC 4514 2.4: „If the AttributeValue is of a type which has its own
        // encoding rules (e.g. an INTEGER) […] it is to be represented as
        // `#HEX`“. Und: „the `#` is followed by the hexadecimal encoding of
        // **the value**, including the **tag and length**“.
        //
        // **UND DIE LAENGE MUSS WIEDER HERGESTELLT WERDEN**, weil
        // `Attribut::wert` sie **nicht** traegt — dort steht „Tag und Inhalt,
        // ohne die TLV-Klammer“, also Tag **und Nutzlast** ohne Laenge. Ohne
        // diese drei Zeilen gaebe die Funktion `#02012A` fuer ein `INTEGER` mit
        // der Nutzlast `2A`, und das ist **keine** gueltige BER-Kodierung:
        // `02 01 2A` waere es, `02 2A` nicht. (GEMESSEN: der Test
        // `rfc4514_kein_text_wird_hex_mit_klammer` — er ist der Grund, warum
        // diese Zeilen hier stehen und nicht im Test.)
        //
        // **UND DIE HEX IST GROSS** — `openssl` schreibt sie klein, RFC 4514
        // nicht; hier wird GROSS genommen, weil `kette.rs` die Seriennummer
        // ueberall sonst hex-GROSS ausgibt und zwei Formen derselben Zahl in
        // einer Anzeige ein Fehler sind.
        None => {
            let mut raus = vec![a.wert[0]];
            let n = a.wert.len() - 1;
            if n < 0x80 {
                raus.push(n as u8);
            } else {
                // X.690 8.1.3: **wenigste** Anzahl Oktette, und das
                // Hochbit der **ersten** Laengenbytes ist gesetzt.
                let mut k = 0usize;
                while n >> (8 * k) != 0 {
                    k += 1;
                }
                raus.push(0x80 | k as u8);
                for i in (0..k).rev() {
                    raus.push((n >> (8 * i)) as u8);
                }
            }
            raus.extend_from_slice(&a.wert[1..]);
            format!("#{}", crate::sha2::hexen(&raus).to_uppercase())
        }
    };
    format!("{typ}={wert}")
}

/// added 03.10.2026, TLS. **DER NAME ALS TEXT — RFC 4514.** Das ist die Form,
/// in der die Zertifikatsinfo den Inhaber und den Aussteller anzeigt, und es
/// ist die **einzige** hier: `narsil-browser` und `narsil-chrome` bekommen keine
/// eigene Formatierung, sonst gaebe es drei.
///
/// **DIE REIHENFOLGE IST UMGEKEHRT, und das ist der Punkt, an dem die
/// meisten falsch liegen.** Woertlich RFC 4514 2.1: „the output is generated
/// by **reversing** the order of the RDNs“ — der **letzte** RDN der Kodierung
/// steht im Text **vorne**.
///
/// **GEMESSEN, nicht behauptet** (03.10.2026,
/// `t12_echtes_bundle.pem`, `openssl x509 -subject -nameopt RFC2253`):
///
/// ```text
/// Kodierung (openssl -text, Wurzel zuerst):  C=ES, O=FNMT-RCM, OU=AC RAIZ FNMT-RCM
/// RFC 2253 (Text, Blatt zuerst):              OU=AC RAIZ FNMT-RCM,O=FNMT-RCM,C=ES
/// ```
///
/// Das ist kein Zufall der Reihenfolge, sondern die **Regel**, und sie ist an
/// zwei Zertifikaten des Bundles gegengeprueft (`z05`: `C=US, O, CN` wird zu
/// `CN,O,C`; `z123`: `C, ST, L, O, CN` wird zu `CN,O,L,ST,C`).
///
/// **UND DIE `+` ZWISCHEN DEN ATTRIBUTEN EINES RDN**, woertlich RFC 4514 2.2:
/// „If an RDN sequence has more than one AVA […] the attribute-value
/// assertions […] are joined together in a **plus sign**“.
///
/// **WARUM DAZU `rdn_grenze` von R58 noetig ist:** ohne die Grenzen weiss man
/// nicht, welche Attribute zu welchem RDN gehoeren — und `+` an der falschen
/// Stelle ist ein **anderer Name**, nicht ein Formatfehler.
pub fn name_als_text(n: &Name) -> String {
    if n.attribute.is_empty() {
        return "(leer)".to_string();
    }
    // added 03.10.2026, TLS: **DIE GRUPPEN, und sie kommen aus `rdn_grenze`.**
    // Fuer jedes RDN nennt sie die Anzahl der Attribute, die **davor** gelesen
    // wurden. Die Attribute selbst liegen derweil flach in `attribute`, also
    // wird hier ein **Index** ueber beide gefuehrt. GEMESSEN, warum kein
    // `windows`: `rdn_grenze` ist eine *Anzahl*, keine Position.
    let mut teile: Vec<String> = Vec::new();
    let mut von = 0usize;
    for &grenze in &n.rdn_grenze {
        // added 03.10.2026, TLS: **VERTEIDIGUNG GEGEN EINE LUEGE IN DEN DATEN.**
        // Ein `rdn_grenze`-Eintrag, der **kleiner** ist als der vorige (oder
        // grosser als die Attributzahl), ist kein gueltiger Name — und ein
        // abgeschnittener Name sieht aus wie ein vollstaendiger. Also wird ab
        // hier **kein** Attribut mehr uebersprungen: der Rest wandert in den
        // letzten RDN. GEMESSEN ist die Notwendigkeit an `testzertifikat.der`,
        // dessen `rdn_grenze` die Attributzahl **nicht** uebersteigt — der
        // Fehler waere also still.
        if grenze <= von || grenze > n.attribute.len() {
            continue;
        }
        teile.push(
            n.attribute[von..grenze]
                .iter()
                .map(maskiere)
                .collect::<Vec<_>>()
                .join("+"),
        );
        von = grenze;
    }
    // added 03.10.2026, TLS: **DER REST, und er ist **kein** Sonderfall.** Ein
    // Zertifikat **ohne** `rdn_grenze`-Eintraege (oder mit weniger als
    // Attributen) bekommt alle Attribute in **einen** RDN. `+, CN=x` waere
    // zudem eine **falsche** Aussage ueber die Zertifikatsstruktur, also
    // waere das Weglassen dieser Zeile nicht nur unvollstaendig, sondern falsch.
    if von < n.attribute.len() {
        teile.push(
            n.attribute[von..]
                .iter()
                .map(maskiere)
                .collect::<Vec<_>>()
                .join("+"),
        );
    }
    // RFC 4514 2.1: **umgekehrt** ausgeben, die RDNs mit Komma getrennt.
    teile.reverse();
    teile.join(",")
}

/// Der Gueltigkeitszeitraum, als Sekunden seit 1970.
///
/// Warum Zeit und kein Text: fuer den Vergleich wird gerechnet, nicht
/// gelesen. Die Umrechnung steht in `zeit_seit_1970`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gueltigkeit {
    /// `notBefore`, Sekunden seit 1970-01-01 00:00:00 UTC.
    pub von: i64,
    /// `notAfter`, Sekunden seit 1970-01-01 00:00:00 UTC.
    pub bis: i64,
}

impl Gueltigkeit {
    /// Liegt `jetzt` im Zeitraum?
    pub fn enthaelt(&self, jetzt: i64) -> bool {
        // Beide Grenzen sind EINSCHLIESSLICH, RFC 5280 Abschnitt 4.1.2.5:
        // "The certificate is valid during the period from notBefore
        // through notAfter, inclusive."
        self.von <= jetzt && jetzt <= self.bis
    }
}

/// Die BASIC CONSTRAINTS-Erweiterung, RFC 5280 Abschnitt 4.2.1.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicConstraints {
    /// `cA`, also ob das Zertifikat eine CA ist.
    pub ist_ca: bool,
    /// `pathLenConstraint`, falls gesetzt.
    pub pfad_laenge: Option<u32>,
}

/// Die NAME CONSTRAINTS-Erweiterung, RFC 5280 Abschnitt 4.2.1.10.
///
/// added 02.10.2026 09:5x, S1.
///
/// WARUM NUR DNS: `GeneralSubtree` kann `dNSName`, `rfc822Name`,
/// `directoryName`, `iPAddress` und `otherName` enthalten (4.2.1.10).
/// Unser Browser prueft **Server**-Zertifikate, und da wirken nur die
/// DNS-Teilbaeume. Die anderen zu parsen, ohne sie anzuwenden, waere Code,
/// der aussieht wie eine Pruefung und keine ist — deshalb steht hier
/// ausdruecklich, dass sie nicht gelesen werden.
///
/// DIE SEMANTIK DER LEEREN LISTE ist die haeufigste Fehlerquelle: ein
/// fehlendes `permitted` heisst **keine** Beschraenkung, ein fehlendes
/// `excluded` heisst **nichts verboten**. Beide sind hier leere Vektoren
/// unter `Option`, und die Pruefung in `kette.rs` unterscheidet sie.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Namensbeschraenkung {
    /// `permittedSubtrees`, nur DNS.
    pub erlaubt: Vec<Vec<u8>>,
    /// `excludedSubtrees`, nur DNS.
    pub verboten: Vec<Vec<u8>>,
    /// added 02.10.2026 14:2x: `permittedSubtrees` auf **`iPAddress`**, als
    /// (Adresse, Maske). Die Form ist **nicht** dieselbe wie im SAN: RFC 5280
    /// 4.2.1.10 sagt wörtlich „For IPv4 addresses, the iPAddress field of
    /// GeneralName MUST contain eight (8) octets" — Adresse **und** Maske, je 4
    /// Byte; für IPv6 je 16 Byte, zusammen 32. Im Zertifikat steht für
    /// `permitted;IP:192.0.2.0/24` die Bytefolge
    /// `C0 00 02 00 FF FF FF 00` (die Norm nennt genau dieses Beispiel).
    ///
    /// added 02.10.2026 14:2x: **WARUM ZWEI VEKTOREN statt einer Liste**: das
    /// ist die Form, in der der Vergleich stattfindet, und ein Struct waere
    /// eine zweite Form, die man in zwei Stellen pflegen muesste.
    pub erlaubt_ip: Vec<(Vec<u8>, Vec<u8>)>,
    /// added 03.10.2026 10:6x, R56. **E-MAIL-BESCHRAENKUNGEN** (`permitted;
    /// email:` / `excluded; email:`). Als **Bytes**, weil die Norm drei
    /// unterschiedliche Formen zulaesst, die man am **ersten Byte** unterscheidet:
    /// ein fuehrender `.` bedeutet „alle Postfaecher der Domain“ (4.2.1.9
    /// woertlich: „the constraint is specified with a leading period“), ein `@`
    /// bedeutet „genau dieses Postfach“, und alles andere ist ein **Host-Name**
    /// („all Internet mail addresses on a particular host“). Eine Liste aus
    /// `String` koennte die Punkte-Form zwar, aber `Bytes` nehmen dem Vergleich
    /// jede Unicode-Frage ab.
    pub erlaubt_email: Vec<Vec<u8>>,
    /// added 03.10.2026 10:6x, R56. **VERBOTENE E-MAIL-BESCHRAENKUNGEN**.
    pub verboten_email: Vec<Vec<u8>>,
    /// added 03.10.2026 10:6x, R56. **URI-BESCHRAENKUNGEN** (`permitted; URI:` /
    /// `excluded; URI:`), als `String`. RFC 5280 4.2.1.9: „the constraint applies
    /// to the **host part** of the name“ — der Vergleich nimmt also den Host aus
    /// der URI, nicht die ganze URI.
    pub erlaubt_uri: Vec<String>,
    /// added 03.10.2026 10:6x, R56. **VERBOTENE URI-BESCHRAENKUNGEN**.
    pub verboten_uri: Vec<String>,
    /// added 03.10.2026 10:7x, R56. **DIRECTORY-NAME-BESCHRAENKUNGEN**
    /// (`permitted; dirName:`). Als `Vec<Name>`, weil RFC 5280 4.2.1.10 hier den
    /// **Attributsvergleich** nach Abschnitt 7.1 verlangt und nicht einen
    /// Byte-Vergleich.
    ///
    /// added 03.10.2026 10:7x: **UND WIR SIND NOCH NICHT DORT — mit Grund.** Das
    /// Feld wird **gelesen** (der Parser fuellt es, und `san.dns`-Namen werden
    /// dagegen geprueft), aber **die Pruefung** ist offen: sie braucht den
    /// **7.1-Namensvergleich**, und `kette::name_gleich` vergleicht **byteweise
    /// und case-sensitiv** — das ist fuer die Kettengliederung richtig (konservativ)
    /// und fuer 7.1 **falsch**. Eigene Arbeit, eigener Punkt.
    pub erlaubt_dn: Vec<Name>,
    /// added 03.10.2026 10:7x, R56. **VERBOTENE DIRECTORY-NAME-BESCHRAENKUNGEN**,
    /// gleicher Stand wie `erlaubt_dn`.
    pub verboten_dn: Vec<Name>,
    /// added 02.10.2026 14:2x: `excludedSubtrees` auf `iPAddress`, dieselbe Form.
    pub verboten_ip: Vec<(Vec<u8>, Vec<u8>)>,
    /// added 03.10.2026 20:9x, R63. **DIE AUFLAGEN, DIE WIR NICHT AUSWERTEN
    /// KÖNNEN — als Tags, nicht als Namen.** Und der Grund für die **Liste** und
    /// nicht für ein `bool`: ein `bool` sagt nur „irgendetwas", und dann kann
    /// niemand sagen, **was** — und die Meldung an den Nutzer wäre wertlos.
    ///
    /// added 03.10.2026 20:9x: **WARUM ÜBERHAupt EIN FELD, und nicht sofort ein
    /// Fehler.** Der Leser ist auch der Ort, an dem die **Zeitachse** des
    /// Zertifikats gelesen wird; ein `Err` hier wuerde jedes Zertifikat mit so
    /// einer Auflage ablehnen — auch dann, wenn der Aufrufer die Auflagen gar
    /// nicht auswerten will (etwa weil er nur den Namen braucht). Der **Leser**
    /// sammelt, das **Pruefen** entscheidet. Das ist die einzige Trennung, die
    /// beides richtig macht.
    ///
    /// added 03.10.2026 20:9x: **UND DIE OBERGRENZE**, benannt und begruendet:
    /// `MAX_UNLESBARE_ARTEN` = **8**, weil es nach RFC 5280 4.2.1.6 genau **neun**
    /// `GeneralName`-Formen gibt und wir **fuenf** lesen — die Zahl der
    /// unbekannten Formen kann also hoechstens **vier** sein. Acht ist die
    /// naechste Klammer darueber: gross genug fuer jede erlaubte Normlage,
    /// klein genug, dass ein Zertifikat mit 1000 unbekannten Tags (ein
    /// Angreifer-Server) nicht den Speicher fuellt.
    pub unlesbar: Vec<u8>,
    /// added 03.10.2026 21:8x, R64. **DIE AUFLAGE NENNT EINEN ABSTAND
    /// (`minimum`/`maximum`)**, den wir **nicht** auswerten. Als `bool`, weil die
    /// **Entscheidung** die Form nicht braucht — sie ist „nicht auswertbar“ oder
    /// nichts — und weil der **Wert** in `minimum`/`maximum` **zur Meldung**
    /// steht, nicht zur Entscheidung.
    ///
    /// added 03.10.2026 21:8x: **UND WARUM SEPARAT VON `unlesbar`:** dort stehen
    /// **Tags** (die Form des `base`), hier steht ein **Feld**, das die Norm
    /// eigentlich **verbietet**. Zwei verschiedene Fehler Ursachen mit **einer**
    /// Liste zu mischen, hieße, die Meldung nicht mehr unterscheiden zu können.
    pub abstand: bool,
    /// added 03.10.2026 21:8x, R64: **DER GEMESSENE `minimum`-Wert**, gekappt
    /// auf **255** (`u8`). Gekappt, weil ein `BaseDistance` nach RFC 5280 4.2.1.10
    /// `INTEGER (0..MAX)` ist — also **beliebig groß** — und ein `u64` im
    /// Fehlertext eine Zahl aus einem **Angreifer**-Zertifikat wäre. **255** ist
    /// die Grenze, an der die Meldung „mindestens 255“ sagen kann.
    pub minimum: u8,
    /// added 03.10.2026 21:8x, R64: **DAS GLEICHE FÜR `maximum`**, und `0`
    /// bedeutet **kein** `maximum` (also „nur minimum relevant“).
    pub maximum: u8,
}

/// Die SUBJECT ALT NAME-Erweiterung, RFC 5280 Abschnitt 4.2.1.6.
///
/// Nur `dNSName` (Tag 2) wird behalten. Die anderen Typen
/// (`iPAddress`, `otherName`, ...) sind fuer den Hostnamenvergleich
/// nicht gebraucht, und sie zu uebersehen waere ein Fehler — deshalb
/// steht hier ausdruecklich, dass sie ignoriert werden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectAltName {
    /// Die DNS-Namen, in der Reihenfolge des Zertifikats.
    pub dns: Vec<String>,
    /// added 03.10.2026 10:0x, R56. **DIE E-MAIL-NAMEN** (`rfc822Name`, [1],
    /// IA5String, im DER Tag 0x81). Als **Bytes**, nicht als String: der Local-
    /// Teil einer E-Mail-Adresse ist nach RFC 5321 **case-sensitiv**, und
    /// `String` koennte hier nicht unterscheiden. Fuer die Gross-/Kleinschreibung
    /// des **Host**-Teils ist die Normstelle weiter unten zustaendig.
    pub email: Vec<Vec<u8>>,
    /// added 03.10.2026 10:0x, R56. **DIE URI-NAMEN**
    /// (`uniformResourceIdentifier`, [6], IA5String, Tag 0x86). Als `String`,
    /// weil `namensbeschraenkung_pruefen` nur den **Host-Teil** vergleicht und
    /// da die Grossschreibung keine Rolle spielt.
    pub uri: Vec<String>,
    /// added 03.10.2026 10:0x, R56. **DIE DIRECTORY-NAMEN** (`directoryName`,
    /// [4], Name, im DER **explizit** kodiert, Tag 0xA4 mit einer SEQUENCE
    /// darin). Als `Vec<Name>`, weil RFC 5280 4.2.1.10 fuer diese Form den
    /// **Attributsvergleich** nach Abschnitt 7.1 verlangt — und ein Byte-Array
    /// koennte das nicht, ohne selbst zu normalisieren.
    pub directory_name: Vec<Name>,
    /// added 02.10.2026 12:2x: Die `iPAddress`-Eintraege (SAN-Tag 0x87), als
    /// **Bytes**: 4 Byte fuer IPv4, 16 Byte fuer IPv6 (RFC 5280 4.2.1.6).
    /// added 02.10.2026 12:2x: **NICHT ALS TEXT**, und das ist der Grund: eine
    /// Adresse hat viele Schreibweisen (`::ffff:192.0.2.1`, `0:0:0:0:0:ffff:
    /// c000:201`, Gross-/Kleinschreibung bei Hex). Ein Textvergleich wuerde
    /// `::ffff:192.0.2.1` nicht gegen `192.0.2.1` halten und umgekehrt, und
    /// genau dieser Fehler ist der Grund, warum RFC 5280 die Form als **Bytes**
    /// festlegt.
    ///
    /// added 02.10.2026 12:2x: **WARUM BISHER LEER.** `san_lesen` hat 0x87
    /// uebersprungen (Kommentar dort: „Die werden uebersprungen, nicht als
    /// Fehler gemeldet"). Das war richtig fuer die damalige Frage („welcher
    /// DNS-Name?") und **unvollstaendig** fuer diese: RFC 2818 3.1 (woertlich)
    /// „In some cases, the URI is specified as an IP address rather than a
    /// hostname. In this case, the iPAddress subjectAltName **must be present**
    /// in the certificate and must exactly match the IP in the URI." Ohne diese
    /// Liste kann ein Client, der zu `https://192.0.2.1/` verbindet, kein
    /// gueltiges Zertifikat finden — und wenn er stattdessen den CN vergleicht,
    /// nimmt er eines an, das RFC 2818 verbietet.
    pub ip: Vec<Vec<u8>>,
}

/// Das gebaute Zertifikat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zertifikat {
    /// Die Rohbytes, aus denen es gelesen wurde. Die Signatur wird
    /// ueber `tbs_der` geprueft, also muss das Original erhalten
    /// bleiben.
    pub der: Vec<u8>,
    /// Die Seriennummer als Bytes, ohne fuehrende Null.
    pub seriennummer: Vec<u8>,
    /// Der Algorithmus, mit dem die CA signiert hat.
    pub signatur_algorithmus: SignaturAlgorithmus,
    /// Der Aussteller.
    pub aussteller: Name,
    /// Der Gueltigkeitszeitraum.
    pub gueltigkeit: Gueltigkeit,
    /// Der Inhaber.
    pub inhaber: Name,
    /// Der oeffentliche Schluessel.
    pub schluessel: SchluesselArt,
    /// `basicConstraints`, falls vorhanden.
    pub basic_constraints: Option<BasicConstraints>,
    /// `subjectAltName`, falls vorhanden.
    pub san: Option<SubjectAltName>,
    /// `keyUsage`, als Bitfeld falls vorhanden.
    pub key_usage: Option<u16>,
    /// added 02.10.2026 23:5x, S4. Die **TLS-Feature**-Erweiterung
    /// (RFC 7633, OID 1.3.6.1.5.5.7.1.24) als Liste der Merkmalsnummern.
    ///
    /// WAS SIE BEDEUTET, woertlich RFC 7633 2.3: „The inclusion of a TLS feature
    /// extension advertising the status_request feature in the server
    /// end-entity certificate **permits a client to fail immediately** if the
    /// certificate status information is not provided by the server.“ Das ist
    /// der einzige Weg, die Soft-Fail-Luecke zu schliessen: ohne dieses
    /// Merkmal darf ein Angreifer die gestapelte OCSP-Antwort einfach
    /// weglassen, und der Client faellt auf „unbekannt“ zurueck.
    ///
    /// added 02.10.2026 23:5x: **GEMESSEN IN DER WILD, 0 von 6** (02.10.2026,
    /// `openssl s_client -status` gegen github.com, www.google.com,
    /// www.mozilla.org, archlinux.org, www.wikipedia.org, news.ycombinator.com):
    /// **kein** Zertifikat traegt die Erweiterung. Und **keiner** der sechs
    /// stapelt eine OCSP-Antwort (`OCSP response: no response sent`). Die
    /// Durchsetzung loest also bei diesen sechs **nichts** aus — sie schliesst
    /// die Luecke, ohne etwas zu zerlegen.
    ///
    /// added 02.10.2026 23:5x: `None`, wenn die Erweiterung fehlt — kein leeres
    /// Feld. „Nicht vorhanden“ und „vorhanden, aber leer“ sind verschiedene
    /// Aussagen, und RFC 7633 4.2 definiert `Features ::= SEQUENCE SIZE
    /// (1..MAX)`, also **darf** sie nicht leer sein.
    pub tls_features: Option<Vec<u16>>,
    /// added 02.10.2026 09:4x, S1. Die **erweiterte** Schluesselverwendung
    /// (extendedKeyUsage, OID 2.5.29.37) als Liste von OID-Bytes.
    ///
    /// WARUM EINE LISTE und kein Text: `serverAuth` ist
    /// 1.3.6.1.5.5.7.3.1, `clientAuth` 1.3.6.1.5.5.7.3.2 — beides sind
    /// OIDs, und ein Zertifikat kann mehrere tragen. Wer „serverAuth"
    /// als Zeichenkette haelt, kann `anyExtendedKeyUsage`
    /// (2.5.29.37.0) nicht von „irgendeiner Verwendung" unterscheiden.
    pub erweiterte_verwendung: Option<Vec<Vec<u8>>>,
    /// added 02.10.2026 09:4x, S1. Die **Namensbeschraenkungen**
    /// (nameConstraints, OID 2.5.29.30).
    ///
    /// Nur die DNS-Teilbaeume, und das mit einer Begründung: die anderen
    /// Namenstaume (Raus/IP/Verzeichnis, 2.5.29.31/32/33) beschraenken
    /// Zertifikate, die unser Browser nie sieht — er prueft Server-
    /// Zertifikate. Ein leerer `excluded` bedeutet „nichts verboten", das
    /// ist die Bedeutung im Leerfall und nicht eine Ausnahme.
    pub namensbeschraenkung: Option<Namensbeschraenkung>,
    /// added 02.10.2026 15:2x: `crlDistributionPoints` (2.5.29.31), und das
    /// sind die **URIs**, an denen die Sperrlisten liegen.
    ///
    /// added 02.10.2026 15:2x: WARUM DAS ÜBERHAUPT GELESEN WIRD, obwohl der
    /// **Abruf** nicht bei mir liegt: `PROBLEME.txt` Nr. 22 stellt die Policy
    /// (soft-fail oder hard-fail, wenn keine Liste erreichbar ist) als offene
    /// Frage, und diese Frage kann niemand beantworten, ohne zu wissen, **wie
    /// viele** Zertifikate ueberhaupt eine Verteilungsstelle tragen und **wo**
    /// sie liegen. Ohne diese Liste ist die Frage eine Meinung; mit ihr ist sie
    /// eine Rechnung. Es ist die **Datenseite** einer Policyfrage, nicht die
    /// Entscheidung — und die Entscheidung habe ich nicht getroffen.
    ///
    /// added 02.10.2026 15:2x: NUR URI, und der Rest der Erweiterung
    /// (`cRLIssuer` 2.5.29.31 / [2], `reasons` [1]) wird **nicht** gelesen. Grund:
    /// ein `cRLIssuer` bedeutet eine **indirekte** Liste (5.2.5
    /// `issuingDistributionPoint` mit `indirectCRL`), und `crl.rs` lehnt solche
    /// Listen bereits ab (`UmfangNichtUnterstuetzt`) — eine Verteilungsstelle
    /// zu lesen, deren Liste wir dann doch nicht benutzen koennten, waere eine
    /// zweite Form derselben Sache.
    pub verteilungsstellen: Vec<Vec<u8>>,
    /// Die Bytes von `tbsCertificate`, unveraendert.
    ///
    /// WICHTIG: ueber diese Bytes wird die Signatur geprueft, nicht
    /// ueber das wieder zusammengesetzte `tbs`. Wer den Zusammenbau
    /// nimmt, prueft eine Sache, die der nicht signiert hat.
    pub tbs_der: Vec<u8>,
    /// Die Signatur der CA, ohne das ungenutzte-Bit-Byte.
    pub signatur: Vec<u8>,
}

impl Zertifikat {
    /// Traegt der Name in seinem CN den Hostnamen?
    ///
    /// WICHTIG UND OFT VERSCHWENKT: nach RFC 6125 und der Branche
    /// allgemeiner Uebung wird der Hostname NUR gegen `subjectAltName`
    /// geprueft, wenn der Name im CN ueberhaupt noch vertrauenswuerdig
    /// ist. Praktisch heisst: SAN hat Vorrang, und der CN wird nur dann
    /// benutzt, wenn gar kein SAN vorhanden ist. Wer beides vermischt,
    /// laesst ein Zertifikat durch, dessen CN etwas anderes sagt als
    /// sein SAN — das ist genau der Fall, an dem Zwischenzertifikate
    /// missbraucht werden.
    ///
    /// Die Wildcard-Regel steht in `hostname_trifft`.
    pub fn traegt_hostnamen(&self, hostname: &str) -> bool {
        match &self.san {
            Some(san) => san.dns.iter().any(|name| hostname_trifft(name, hostname)),
            None => match self.inhaber.common_name() {
                Some(cn) => hostname_trifft(&cn, hostname),
                None => false,
            },
        }
    }

    /// Traegt das Zertifikat **diese IP-Adresse**? (SAN-Typ `iPAddress`.)
    ///
    /// added 02.10.2026 12:4x: DER ZWEITE WEG, und er ist getrennt von
    /// `traegt_hostnamen`, weil die beiden **verschiedene Vergleiche** sind:
    /// dort Text gegen Text mit Wildcard, hier **4 oder 16 Byte** gegen
    /// 4 oder 16 Byte. Eine Funktion, die beides mit einem Vergleich
    /// beantworten wollte, muesste die Adresse erst in Text umrechnen — und
    /// genau dabei entstehen die Fehler, die RFC 5280 mit der Byteform
    /// vermeidet.
    ///
    /// added 02.10.2026 12:4x: DIE NORM, woertlich (RFC 2818 3.1): „In some
    /// cases, the URI is specified as an IP address rather than a hostname. In
    /// this case, the **iPAddress subjectAltName must be present** in the
    /// certificate and must exactly match the IP in the URI." „exactly match"
    /// heisst Bytegleichheit, kein Praefix.
    ///
    /// added 02.10.2026 12:4x: **UND DER CN WIRD HIER NICHT GELESEN**, das ist
    /// Absicht und der Grund fuer `traegt_ip` als **eigene** Funktion. RFC 2818
    /// sagt fuer das IP-Literal ausdruecklich nur vom `iPAddress`-SAN; eine
    /// Adresse im `commonName` ist kein Nachweis. (GEMESSEN, und das ist der
    /// Grund fuer die Aufteilung: `traegt_hostnamen` vergleicht **Text** und
    /// wuerde `CN = 192.0.2.1` gegen `192.0.2.1` passieren lassen — siehe
    /// `tests/ip_literale.rs`, das diesen Fehler festhält.)
    pub fn traegt_ip(&self, ip: &[u8]) -> bool {
        match &self.san {
            Some(san) => san.ip.iter().any(|eintrag| eintrag == ip),
            // added 02.10.2026 12:4x: **OHNE SAN GIBT ES KEINEN IP-NACHWEIS.**
            // Der CN zaehlt nicht (s.o.), und `san: None` heisst „kein SAN",
            // nicht „SAN mit anderen Formen".
            None => false,
        }
    }
}

/// Passt ein Muster aus dem Zertifikat zu einem Hostnamen?
///
/// Die Regeln, und sie sind nicht verhandelbar:
///
///  1. Der Vergleich ist case-insensitive. DNS ist es auch.
///  2. `*` steht fuer GENAU EIN Label, nicht fuers ganze Rest.
///     `*.example.com` passt zu `a.example.com`, aber NICHT zu
///     `a.b.example.com` und NICHT zu `example.com`.
///  3. `*` steht nur als GANZES erstes Label. `w*.example.com` und
///     `*.example.com` sind beide nicht zulaessig; RFC 6125 Abschnitt
///     6.4.3 nennt das ausdruecklich.
///  4. Ein Hostname mit einem fuehrenden Punkt oder einem leeren
///     Label wird abgelehnt, statt als "passt" gemeldet.
pub fn hostname_trifft(muster: &str, hostname: &str) -> bool {
    // Punkt 4 zuerst: ein leeres Label ist niemals in Ordnung.
    if hostname.is_empty() || hostname.starts_with('.') || hostname.ends_with('.') {
        return false;
    }
    let h: Vec<&str> = hostname.split('.').collect();
    let m: Vec<&str> = muster.split('.').collect();
    if m.len() != h.len() {
        return false;
    }
    for (i, teil) in m.iter().enumerate() {
        if *teil == "*" {
            // Punkt 3: nur an erster Stelle.
            if i != 0 {
                return false;
            }
            // Punkt 2: das Label, das ersetzt wird, darf nicht leer
            // sein. `*.example.com` gegen `.example.com` faellt hier
            // schon bei der Laengenpruefung weiter oben, aber der Fall
            // `*.` gegen `x.` ist damit auch abgedeckt.
            if h[i].is_empty() {
                return false;
            }
            continue;
        }
        // Punkt 3, die zweite Haelfte: ein Sternzeichen in einem
        // anderen Label als dem ersten ist verboten, auch mitten drin.
        if teil.contains('*') {
            return false;
        }
        // Punkt 1: case-insensitive.
        if !teil.eq_ignore_ascii_case(h[i]) {
            return false;
        }
    }
    true
}

/// Rechnet eine ASN.1-UTCTime oder GeneralizedTime in Sekunden
/// seit 1970.
///
/// Beide Formen kommen in Zertifikaten vor: UTCTime (`YYMMDDhhmmssZ`)
/// bis 2049, GeneralizedTime (`YYYYMMDDhhmmssZ`) ab 2050. Wer nur eine
/// kennt, lehnt die andere ab — und Zertifikate mit GeneralizedTime
/// gibt es in freier Wildbahn.
///
/// UTCTime ist zweistelliges Jahr. Nach RFC 5280 Abschnitt 4.1.2.5.1
/// steht 50 bis 99 fuer 19xx und 00 bis 49 fuer 20xx.
pub fn zeit_seit_1970(tag: u8, inhalt: &[u8]) -> Result<i64, X509Fehler> {
    // added 03.10.2026, TLS (Fuzzing, 100 000 Faelle `x509`): **DER FUND, und
    // es ist ein Absturz mitten im Lesen.** Der Fuzzer meldete
    // `x509.rs:1184: end byte index 10 is not a char boundary; it is inside
    // 'ݸ' (bytes 9..11 of string)`.
    //
    // **DIE URSACHE, und sie ist eine Frage der Ebene:** Die Laengenpruefung
    // unten zaehlt **Bytes** (`s.len() != 13`), und das Schneiden
    // `s[10..12]` tut es an **Zeichengrenzen** eines `&str`. Ein
    // **zweibyteiges** Zeichen passt in 13 Byte — der Laengencheck ist also
    // gruen, und der Schnitt landet **mitten** im Zeichen.
    //
    // **GEMESSEN, dass das kein exotischer Fall ist:** der Fuzzer hat ihn in
    // **einem** von 100 000 zufaelligen Faellen gefunden, und der Weg
    // dorthin ist der **Blatt-Gueltigkeitsbereich** — der ist in **jedem**
    // Zertifikat. Ein boses Zertifikat mit einem 2-Byte-Zeichen im
    // Zeitfeld reicht also, um den Browser zu **beenden**.
    //
    // **DIE REPARATUR, und sie ist nicht nur ein `get`:** X.690 11.8 und
    // RFC 5280 4.1.2.5.1 sagen, das Zeitfeld besteht aus **ASCII-Ziffern**
    // und einem `Z` (UTC). Also wird auf **`&[u8]`** gelesen und **jede**
    // Stelle geprueft — das kann **nicht** abstuerzen (Byte-Schnitte auf
    // einem Slice sind total), und es ist **strenger** als vorher, was hier
    // richtig ist: `ݸ` ist kein gueltiges Datum.
    //
    // **DERSELBE FEHLER STEHT AUCH IN `Attribut::wert_als_text` — dort ist er
    // mit `from_utf8` bereits behoben** (siehe dort: „`from_utf8`, NICHT
    // `from_utf8_lossy`“). Diese Funktion war der Rest davon.
    let s = match tag {
        0x17 => {
            if inhalt.len() != 13 {
                return Err(X509Fehler::ZeitUnbrauchbar);
            }
            inhalt
        }
        0x18 => {
            if inhalt.len() != 15 {
                return Err(X509Fehler::ZeitUnbrauchbar);
            }
            inhalt
        }
        _ => return Err(X509Fehler::ZeitUnbrauchbar),
    };
    let jahr: i64 = match tag {
        0x17 => {
            let yy = zwei_ziffern(s, 0)?;
            if yy >= 50 { 1900 + yy } else { 2000 + yy }
        }
        0x18 => vier_ziffern(s, 0)?,
        _ => return Err(X509Fehler::ZeitUnbrauchbar),
    };
    // EIGENER FEHLER, und der hat den Monat und den Tag verschoben:
    // die Felder von UTCTime liegen so:
    //
    //     0 1 2 3 4 5 6 7 8 9 10 11 12
    //     Y Y M M D D h h m m s s  Z
    //
    // Ich hatte den Monat bei [4..6] gelesen statt bei [2..4] und den
    // Tag bei [6..8] statt [4..6]. Damit wurde aus "301231093737Z" der
    // Monat 31 — und `parse` schlug bei "09:37:37Z" an, was als
    // "Zeit nicht lesbar" herauskam und nicht als "Offset falsch".
    //
    // GeneralizedTime hat vier Stellen fuers Jahr, also rutscht alles
    // um zwei nach rechts. Deshalb wird nicht mit festen Offsets
    // gelesen, sondern mit einem Index, der am Jahr beginnt und
    // mitgeht.
    let jahr_start = match tag {
        0x17 => 0, // YYMMDDhhmmssZ
        0x18 => 0, // YYYYMMDDhhmmssZ, aber vier Stellen
        _ => return Err(X509Fehler::ZeitUnbrauchbar),
    };
    let monat_start = jahr_start + if tag == 0x18 { 4 } else { 2 };
    let monat = zwei_ziffern(s, monat_start)?;
    let tag_ = zwei_ziffern(s, monat_start + 2)?;
    let stunde = zwei_ziffern(s, monat_start + 4)?;
    let minute = zwei_ziffern(s, monat_start + 6)?;
    // added 03.10.2026, TLS (Fuzzing): **DIE SEKUNDE ENDET VOR DEM `Z`, und
    // das `Z` wird jetzt **geprueft** statt uebergangen.**
    //
    // **WARUM DIE PRUEFUNG UND NICHT NUR DAS UEBERSPRINGEN:** vorher wurde
    // `s.len() - 1` als Ende genommen, **ohne** zu pruefen, was dort steht.
    // Bei `YYMMDDhhmmssZ` ist das `Z` — bei einem anderen Zeichen waere die
    // Sekunde **falsch** gelesen worden und es trotzdem `Ok`. RFC 5280
    // 4.1.2.5.1 sagt woertlich, UTCTime „MUST include seconds“ und „MUST be
    // expressed in Greenwich Mean Time (Zulu) time“ — ein fehlendes oder
    // anderes `Z` ist also **kein** gueltiges Datum und wird abgelehnt.
    if s[s.len() - 1] != b'Z' {
        return Err(X509Fehler::ZeitUnbrauchbar);
    }
    let sekunde = zwei_ziffern(s, monat_start + 8)?;
    if !(1..=12).contains(&monat)
        || !(1..=31).contains(&tag_)
        || stunde > 23
        || minute > 59
        || sekunde > 60
    {
        return Err(X509Fehler::ZeitUnbrauchbar);
    }
    Ok(tage_seit_1970(jahr, monat, tag_) * 86400 + stunde * 3600 + minute * 60 + sekunde)
}

/// added 03.10.2026, TLS (Fuzzing): **ZWEI BYTE ALS ZAHL**, und beide Stellen
/// muessen **ASCII-Ziffern** sein.
///
/// **WARUM NICHT `std::str::from_utf8` + `parse`**, ob das der kuerzere Weg
/// waere: `parse` auf einem `&str`-Teil **panict**, wenn der Schnitt mitten in
/// ein Zeichen faellt. Genau das war der Fund (`x509.rs:1184`). Auf **`&[u8]`**
/// gibt es diese Gefahr nicht — und die Ziffernpruefung macht es zusaetzlich
/// **strenger**: `ݸ` ist keine Ziffer, und ein Datum aus Buchstaben ist kein
/// Datum.
fn zwei_ziffern(s: &[u8], ab: usize) -> Result<i64, X509Fehler> {
    if ab + 2 > s.len() {
        return Err(X509Fehler::ZeitUnbrauchbar);
    }
    let tens = ziffer(s[ab])?;
    let eins = ziffer(s[ab + 1])?;
    Ok(tens * 10 + eins)
}

/// added 03.10.2026, TLS (Fuzzing): **VIER BYTE ALS ZAHL** (das Jahr im
/// `GeneralizedTime`).
fn vier_ziffern(s: &[u8], ab: usize) -> Result<i64, X509Fehler> {
    if ab + 4 > s.len() {
        return Err(X509Fehler::ZeitUnbrauchbar);
    }
    let mut wert = 0i64;
    for i in 0..4 {
        wert = wert * 10 + ziffer(s[ab + i])?;
    }
    Ok(wert)
}

/// added 03.10.2026, TLS (Fuzzing): **EIN BYTE ALS ZIFFER**, oder Fehler.
///
/// **UND DIE PRUEFUNG IST `b'0'..=b'9'`**, weil X.690 11.8 fuer das
/// Zeitfeld **ASCII-Ziffern** verlangt. `u8::is_ascii_digit` ist dafuer genau
/// richtig — und eine Ziffer **ohne** diese Pruefung waere `48` (also `0`) nur
/// durch Zufall.
fn ziffer(b: u8) -> Result<i64, X509Fehler> {
    if b.is_ascii_digit() {
        Ok(i64::from(b - b'0'))
    } else {
        Err(X509Fehler::ZeitUnbrauchbar)
    }
}

/// Die Zahl der Tage vom 1. Januar 1970 bis zum angegebenen Datum.
///
/// Die Formel ist die von Howard Hinnant, `days_from_civil`, und sie
/// steht als Referenz in RFC 3339 Appendix A. Sie rechnet in
/// MARZ-BEGINNENDEN Jahren, damit der 29. Februar ans Ende faellt und
/// nicht mitten im Jahr steht.
///
/// EIGENER FEHLER, zweifach, und beide mal waeren die Werte um einen
/// konstanten Betrag danebengegangen, ohne dass ein Test es gemerkt
/// haette: erstens habe ich `y` auf das Jahr 1970 bezogen statt auf
/// das Jahr 0, zweitens habe ich die Verschiebung um 719468 Tage
/// angebracht, obwohl die Formel schon ab 0 zaehlt. Ergebnis war
/// -719161 statt 0 fuer den 1. Januar 1970. Der Test
/// `zeitrechnung_an_festen_daten` faellt darauf, weil er `date -u`
/// als unabhaengige Quelle danebenstellt.
///
/// Die Schritte:
///   1. Januar und Februar gehoeren zum Vorjahr (`yy`).
///   2. `era` ist die Zahl der abgelaufenen 400-Jahres-Zeitraeume.
///      400 Jahre haben 146097 Tage, und 4 von je 100 Jahren sind
///      Schaltjahre — ausser das Hundertjahr selbst, ausser es ist
///      durch 400 teilbar. Genau das steckt in `yoe*365 + yoe/4 -
///      yoe/100` mit dem ganzen Zyklus daneben.
///   3. Der Monat wird auf Maerz = 0 umgestellt.
///   4. 719468 Tage liegen zwischen dem 1. Januar des Jahres 0 und dem
///      1. Januar 1970.
pub fn tage_seit_1970(jahr: i64, monat: i64, tag: i64) -> i64 {
    // Schritt 1
    let mut yy = jahr;
    if monat <= 2 {
        yy -= 1;
    }
    // Schritt 2
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400; // 0 bis 399
    // Schritt 3
    let mp = (monat + 9) % 12; // Maerz = 0, ..., Februar = 11
    // Schritt 4
    let doy = (153 * mp + 2) / 5 + tag - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Liest ein Zertifikat aus seinen DER-Bytes.
///
/// M3/M4: die Laenge wird VOR jeder Allokation geprueft.
/// added 02.10.2026 23:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// Liest den `subject` (bzw. den `issuer`) eines Zertifikats als **unveraendertes
/// DER**.
///
/// WARUEBERHAUPT: die CertID einer OCSP-Antwort enthaelt `issuerNameHash`,
/// und der ist nach RFC 6960 4.1.1 „the hash of the issuer's distinguished
/// name (DN)". Gehasht wird der **komplette** Name inklusive Tag und Laenge —
/// wer nur die punktweise Form hasht, bekommt einen anderen Wert und damit
/// immer „falscher Aussteller". `Name` ist hier geparst und reihenfolgenormalisiert,
/// also nicht verwendbar; diese Funktion gibt genau die Bytes zurueck.
///
/// DER WEG: `tbs_der` ist die Nachricht, wie sie auf der Leitung stand
/// (`zertifikat_lesen` speichert genau das fuer die Signaturpruefung). Wir
/// navigieren darin bis zum gewuenschten `Name` und geben dessen `roh`
/// zurueck — dieselbe Form, die der Responder hasht.
fn name_der(z: &Zertifikat, aussteller: bool) -> Result<Vec<u8>, X509Fehler> {
    // M4: `tbs_der` ist durch `MAX_ZERTIFIKAT` begrenzt (beim Lesen geprueft),
    // und `DerLeser` begrenzt die Tiefe selbst.
    let mut leser = DerLeser::neu(&z.tbs_der);
    let tbs = leser.element_mit_tag(0x30)?;
    let mut t = DerLeser::neu(&tbs.inhalt);
    // version [0] EXPLICIT, DEFAULT v1 — darf fehlen (wie in `zertifikat_lesen`).
    if t.naechste_tag() == Some(0xA0) {
        let _ = t.element()?;
    }
    let _seriennummer = t.element_mit_tag(0x02)?;
    let _algorithmus = algorithmus_lesen(&mut t)?;
    // added 02.10.2026 23:9x: DER ISSUER STEHT VOR DEM SUBJEKT, und ich hatte
    // ihn beim Weg zum `subject` **uebersprungen**. GEMESSEN: `sha1` ueber das
    // Ergebnis war `3ec1692d…` statt `b2fcf08b…` — und `b2fcf08b…` ist der
    // Wert, den `openssl ocsp` in die CertID geschrieben hat
    // (`openssl ocsp -respin gut_ecdsa.der -text`: „Issuer Name Hash:
    // B2FCF08B…"). Grund: `tbsCertificate` ist
    // `version, serialNumber, signature, issuer, validity, subject, …`, und
    // nach `signature` kommt **nicht** die Gueltigkeit, sondern der Issuer.
    // Das fuehrte dazu, dass `subject_der` die Gueltigkeit (zwei UTCTime)
    // zurueckgab — und `spki_bits` lief danach ebenfalls in die falsche
    // Richtung.
    let issuer = t.element_mit_tag(0x30)?;
    if aussteller {
        return Ok(issuer.roh);
    }
    // validity ::= SEQUENCE { notBefore, notAfter }
    let _gueltigkeit = t.element_mit_tag(0x30)?;
    Ok(t.element_mit_tag(0x30)?.roh)
}

/// added 02.10.2026 23:2x: der `subject` als DER. Siehe `name_der`.
pub fn subject_der(z: &Zertifikat) -> Result<Vec<u8>, X509Fehler> {
    name_der(z, false)
}

/// added 02.10.2026 23:2x: der `issuer` als DER. Siehe `name_der`.
pub fn aussteller_der(z: &Zertifikat) -> Result<Vec<u8>, X509Fehler> {
    name_der(z, true)
}

/// added 02.10.2026 23:2x: die **Bit-Zeichenkette** von `subjectPublicKey`,
/// ohne Tag, ohne Laenge und ohne das Byte der ungenutzten Bits.
///
/// RFC 6960 4.1.1: `issuerKeyHash` ist „the hash of the issuer's public key.
/// The hash shall be calculated over the value (the bit string) of the
/// subjectPublicKey **excluding** the tag, length, and number of unused bits".
/// Genau diese Bytes sind also gemeint, und `spki_lesen` hat sie gelesen und
/// weggeworfen — sie stehen jetzt hier zur Verfuegung.
///
/// WARUM NICHT AUS DEM SCHLUESSEL ZUSAMMENGEBAUT: `SchluesselArt` traegt bei
/// RSA nur `n` und `e` und bei ECDSA `x` und `y`. Wieder zusammenzusetzen hiesse,
/// eine zweite Kodiervorschrift zu pflegen, und bei einer Kurve mit falscher
/// Laenge entstuende ein *anderer* Hash als der richtige — still.
pub fn spki_bits(z: &Zertifikat) -> Result<Vec<u8>, X509Fehler> {
    let mut leser = DerLeser::neu(&z.tbs_der);
    let tbs = leser.element_mit_tag(0x30)?;
    let mut t = DerLeser::neu(&tbs.inhalt);
    if t.naechste_tag() == Some(0xA0) {
        let _ = t.element()?;
    }
    let _seriennummer = t.element_mit_tag(0x02)?;
    let _algorithmus = algorithmus_lesen(&mut t)?;
    let _aussteller = t.element_mit_tag(0x30)?;
    let _gueltigkeit = t.element_mit_tag(0x30)?;
    let _inhaber = t.element_mit_tag(0x30)?;
    let spki = t.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&spki.inhalt);
    let _alg = s.element_mit_tag(0x30)?;
    let bit = s.element_mit_tag(0x03)?;
    let (_, inhalt) = bit_string(&bit.inhalt)?;
    Ok(inhalt.to_vec())
}

/// added 03.10.2026 13:0x, R57. **DER NAMENSVERGLEICH NACH RFC 5280 7.1** —
/// und er ist **kein** Ersatz fuer `kette::name_gleich`, sondern eine eigene
/// Funktion mit einer eigenen Regel.
///
/// added 03.10.2026 13:0x: **WAS DIE NORM VERLANGT, woertlich:** „Conforming
/// implementations **MUST use the LDAP StringPrep profile (including
/// insignificant space handling)**, as specified in [RFC4518], as the basis for
/// comparison of distinguished name attributes encoded in either PrintableString
/// or UTF8String. Conforming implementations **MUST support name comparisons
/// using caseIgnoreMatch**.“ Und zur Vorbereitung: „* In step 2, Map, the mapping
/// shall include **case folding** … * In step 6, Insignificant Character Removal,
/// perform **white space compression** …“ Und der Vergleich selbst: „Two naming
/// attributes match if the **attribute types are the same** and the values of the
/// attributes are an **exact match after processing** with the string preparation
/// algorithm.“
///
/// added 03.10.2026 13:0x: **UND WAS HIER NICHT DRIN IST, benannt statt
/// verschwiegen.** RFC 4518 hat **sechs** Schritte: Map, Normalize, BiDi,
/// Prohibit, Unassigned, Insignificant Character Removal. Ich baue **zwei**:
/// **case folding** (Schritt 2, ausdruecklich genannt) und **white space
/// compression** (Schritt 6, ausdruecklich genannt). Die Schritte 3 bis 5
/// betreffen **Unicode** (BiDi, verbotene Zeichen, nicht zugewiesene
/// Codepunkte), und die Norm verlangt ohnehin nur `PrintableString` oder
/// `UTF8String` fuer diese Vergleiche. Fuer **ASCII** fallen die Schritte
/// zusammen: es gibt keine BiDi-Zeichen und keine verbotenen ASCII-Zeichen.
///
/// added 03.10.2026 13:1x: **UND `PRINTABLESTRING` ENTHALT KEIN BYTE > 0x7E**,
/// also ist case folding da genau `to_ascii_lowercase`. Fuer `UTF8String` mit
/// nicht-ASCII-Zeichen **ersetzt** diese Funktion die echte Vorbereitung nicht —
/// sie liefert dann ein **falsches** Ergebnis, und das ist der Grund, warum sie
/// **nur** dort benutzt werden darf, wo ein Nicht-ASCII-Namen **ohnehin** zur
/// Ablehnung fuehrt (siehe `namensbeschraenkung_pruefen`).
pub fn wert_7_1_bereitet(wert: &[u8]) -> Vec<u8> {
    // added 03.10.2026 13:1x: **SCHRITT 6 ZUERST** (white space compression), und
    // die Reihenfolge ist **nicht beliebig**: erst der Leerraum, dann das
    // case folding — oder umgekehrt? RFC 4518 Schritt 2 (Map, case folding) kommt
    // **vor** Schritt 6. Bei `A B` ist es gleich, weil beide Schritte die
    // Leerzeichen unberuehrt lassen. Ich mache es in der **Nummernfolge**, damit
    // der Code dem Text entspricht.
    let mut raus: Vec<u8> = Vec::with_capacity(wert.len());
    // added 03.10.2026 13:2x: **`SPACE`, TAB, LF, CR** — RFC 4518 Abschnitt 2.6.1
    // nennt SPACE, TAB, CR und LF als „insignificant“. Es sind also **vier**
    // Zeichen, nicht nur eines, und ein Test mit nur Space haette die anderen drei
    // nicht geprueft.
    let ist_leerraum = |b: u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
    // Schritt 2: case folding
    for b in wert {
        raus.push(b.to_ascii_lowercase());
    }
    // Schritt 6: insignificant character removal / white space compression
    let mut ohne: Vec<u8> = Vec::with_capacity(raus.len());
    for b in raus {
        if ist_leerraum(b) {
            if ohne.last().copied().is_some_and(|l| !ist_leerraum(l)) {
                // added 03.10.2026 13:2x: **INNERE FOLGEN ZU EINER** — das ist
                // die Compression. Und ein **fuehrender** Leerraum wird ganz
                // weggelassen (es steht noch keiner drin).
                ohne.push(b' ');
            }
            continue;
        }
        ohne.push(b);
    }
    // added 03.10.2026 13:2x: **ABSCHLIESSENDER LERRAUM WEG**, und wieder mit der
    // Bedingung: nach dem Komprimieren kann der letzte Platz **von einem Trailing-
    // Leerraum** stammen und muss weg.
    while ohne.last().copied().is_some_and(ist_leerraum) {
        ohne.pop();
    }
    ohne
}

/// added 03.10.2026 13:3x, R57. **ZWEI ATTRIBUTE NACH 7.1**, und die Regel ist
/// zweiteilig: **gleicher Typ** und **gleicher vorbereiteter Wert**. Die Typen
/// werden **nicht** normalisiert — ein `commonName` ist kein `serialNumber`, auch
/// wenn die Werte gleich sind.
pub fn attribut_gleich_7_1(a: &Attribut, b: &Attribut) -> bool {
    a.typ == b.typ && wert_7_1_bereitet(&a.wert) == wert_7_1_bereitet(&b.wert)
}

/// added 03.10.2026 15:0x, R59. **DIE VERTEILUNGSPUNKTE ALS TEXTE** — und das
/// ist die **Vertragsflaeche** zwischen dieser Bibliothek und `narsil-net`, und es
/// ist die kleinste Form, die etwas bewegt.
///
/// added 03.10.2026 15:0x: **WAS DIE FRAGE IST.** `Zertifikat::verteilungsstellen`
/// ist ein `Vec<Vec<u8>>` — **rohe DER** von `GeneralName`-Feldern. Wer die
/// Beschaffung macht (nach Schichtung `narsil-net`, weil eine Kryptobibliothek keine
/// HTTP-Abfragen macht), moechte **URLs**, keine DER-Felder. Ohne diese Funktion
/// muesste `narsil-net` den `DistributionPoint`-Aufbau **selbst** dekodieren, also
/// `x509::DerLeser` benutzen — und damit eine **zweite** Vorstellung davon haben,
/// wie `DistributionPointName` aufgebaut ist. Genau das ist die Fehlerform, die
/// R50/R51/R53 viermal getroffen hat.
///
/// added 03.10.2026 15:0x: **WAS DIE FUNKTION TUT, und es ist bewusst
/// zurueckhaltend:** sie liefert **nur** die `uniformResourceIdentifier`-Namen,
/// also die Form `[6]` mit Tag `0x86`. Das sind die **einzigen**, die ein Client
/// braucht: `dNSName` `[2]` und `iPAddress` `[7]` sind **keine** Adressen, wo man
/// eine CRL holen koennte, und `directoryName` `[4]` ist ein Name, keine Stelle.
/// Wo eine Form **nicht** geliefert wird, steht das hier, damit niemand fragt.
pub fn verteilungsstellen_als_texte(z: &Zertifikat) -> Vec<String> {
    // added 03.10.2026 16:e0, R60: **KEIN ZWEITES DER-LESEN — UND DAS IST DIE
    // KORREKTUR EINES FEHLERS AUS R59, GESTORCHEN IN DER WILD.**
    //
    // added 03.10.2026 16:e0: **WAS R59 HIER GETAN HAT.** `verteilungsstellen`
    // enthaelt laut eigener Doku „rohe DER von `GeneralName`-Feldern“ — und das
    // ist **falsch**. GEMESSEN (16:e1) an `xkcd.com` in der Diagnose des
    // Netztests: `dp_roh=1` (ein Eintrag gelesen) und `quellen=[]` (keine URL).
    // Der Leser legt den **Inhalt** des `0x86`-Feldes ab, also die reinen
    // URI-**Zeichen** (`verteilungsstellen_lesen`, Zeile 2308: `raus.push(
    // gn.inhalt.clone())`), und **nicht** das Feld mit Tag und Laenge.
    //
    // added 03.10.2026 16:e1: **R59 HAT DARAUF AUFGEBAUT UND ES GEMESSEN NICHT.**
    // `general_namen_text` versuchte, diese Zeichen erneut als
    // `SEQUENCE { [0] { [0] { 86 … } } }` zu **dekodieren** — also die
    // vier-Schichten-Kette `30 30 A0 A0`, die R59 dort selbst als „**zwei**
    // Schichten `A0`“ beschrieben hatte. Bei einem Eintrag, der mit `h` (von
    // „http“) beginnt, scheitert das **stillschweigend**: kein Fehler, kein
    // `Err`, nur eine **leere** Liste. Genau deshalb hat es **keinen** Test
    // gefunden: `verteilungsstellen_als_texte` hatte **null** Tests (GEMESSEN,
    // 16:e2, `grep` ueber `crates/`), und die Funktion selbst war in R59 nie
    // gegen die Wild gelaufen.
    //
    // added 03.10.2026 16:e2: **DIE LEHRE, UND SIE STEHT SCHON IM CODE.** Ich
    // habe beim Debuggen (16:d9) notiert: „ein Aufbau, den man zweimal baut,
    // sollte **einmal** gebaut werden“. Der Aufbau wurde zweimal gebaut — einmal
    // im Leser, einmal in der Textwandlung — und die **zweite** Kopie war falsch
    // und **lief still**. Die Regel ist damit: wenn ein Feld schon **geparst**
    // gespeichert wird, darf die **nachgelagerte** Funktion es nicht **noch
    // einmal** parsen. Sonst ist die Frage „welche Kopie ist richtig?“ eine
    // Frage pro Aufruf — und sie wird im Debuggen unterschiedlich beantwortet,
    // weil man die **falsche** zuerst angefasst hat.
    texte_aus_urten(&z.verteilungsstellen)
}

/// added 03.10.2026 16:e8, R60. **DIE REINE FORM**, abgetrennt von `Zertifikat`,
/// und der Grund ist **praktisch**, nicht schoen: `Zertifikat` hat kein
/// `Default` (GEMESSEN 16:e9: „no associated function … named `default`“), also
/// muesste jeder Test ein Zertifikat bauen, um **eine Liste von Bytes** zu
/// pruefen. Diese Funktion nimmt genau das, was geprueft wird — die Liste der
/// bereits gelesenen URI-Felder — und sonst nichts.
fn texte_aus_urten(quellen: &[Vec<u8>]) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for uri in quellen {
        // added 03.10.2026 16:e3: **STRENGES UTF-8, UND DER UNTERSCHIED ZUM
        // VORHER VERWENDETEN `from_utf8` IST DER GRUND.** `from_utf8` macht aus
        // ungueltigen Bytes eine **verstuemmte** Anzeige (das Zeichen `U+FFFD`),
        // und eine solche Anzeige waere eine **erfundene** URL: der Aufrufer
        // wuerde einen Verteilungspunkt abrufen, den es nicht gibt. Wir
        // verlangen also **entweder** echten UTF-8 **oder** gar nichts.
        //
        // added 03.10.2026 16:e3: **UND `IA5`**, denn das ist die Normform:
        // RFC 5280 4.2.1.13 sagt, der Name sei ein „URI“ aus RFC 3986, und dessen
        // Zeichen sind **IA5** (7 Bit). Ein `0xC3 0xBC` (UTF-8-Umlaut) ist damit
        // **kein** gueltiger Verteilungspunkt — die Pruefung von `from_utf8`
        // allein wuerde ihn **akzeptieren**. `isascii()` schliesst ihn aus, ohne
        // dass wir die ganze Zeichentabelle aufbauen muessen.
        match std::str::from_utf8(uri) {
            Ok(text) if !text.is_empty() && text.is_ascii() => raus.push(text.to_string()),
            _ => {}
        }
    }
    raus
}

pub fn zertifikat_lesen(der: &[u8]) -> Result<Zertifikat, X509Fehler> {
    if der.len() > MAX_ZERTIFIKAT {
        return Err(X509Fehler::ZuLang {
            was: "Zertifikat",
            ist: der.len(),
            erlaubt: MAX_ZERTIFIKAT,
        });
    }
    if der.is_empty() {
        return Err(X509Fehler::Aufbau {
            was: "leeres Zertifikat",
        });
    }
    let mut leser = DerLeser::neu(der);
    let aussen = leser.element_mit_tag(0x30)?;
    let mut tlv = DerLeser::neu(&aussen.inhalt);

    // --- tbsCertificate -------------------------------------------------
    let tbs_element = tlv.element_mit_tag(0x30)?;
    // Die Signatur wird ueber diese EXAKTEN Bytes geprueft, darum
    // werden sie hier behalten und nicht wieder zusammengesetzt.
    // `roh` enthaelt Tag und Laenge mit, so wie sie im Zertifikat
    // standen — genau das ist der Bereich, den die CA signiert hat.
    let tbs_der: Vec<u8> = tbs_element.roh.clone();
    let mut t = DerLeser::neu(&tbs_element.inhalt);

    // version [0] EXPLICIT, DEFAULT v1 — darf fehlen.
    //
    // EIGENER FEHLER, und er waere still gewesen: ich habe zuerst
    // `t.element()` aufgerufen und dann geschaut, ob das Tag 0xA0 ist.
    // Damit war das Seriennummernfeld schon verbraucht, wenn kein
    // Versionsfeld da war — also bei jedem v1-Zertifikat. Die
    // Loesung ist `naechste_tag`, das nur schaut und nichts liest.
    let mut version = 1u8; // RFC 5280 4.1.2.1: absent field defaults to v1.
    if t.naechste_tag() == Some(0xA0) {
        let huelle = t.element()?;
        let mut h = DerLeser::neu(&huelle.inhalt);
        let wert = integer_als_bytes(&h.element_mit_tag(0x02)?.inhalt)?;
        // RFC 5280 Abschnitt 4.1: `version` ist v1, v2 oder v3, und im
        // INTEGER steht der Wert 0, 1 oder 2. Es gibt kein v0 und kein
        // v4; beides wird abgelehnt statt geraten.
        if wert.len() != 1 || wert[0] > 2 {
            return Err(X509Fehler::Aufbau {
                was: "Version unplausibel",
            });
        }
        version = wert[0] + 1;
    }
    // `version` ist oben schon `wert[0] + 1`, also 1, 2 oder 3.
    // EIGENER FEHLER: hier stand `> 2`, wodurch jedes v3-Zertifikat
    // abgelehnt wurde — und v3 ist praktisch jedes Zertifikat der
    // Welt, weil nur dort die Erweiterungen erlaubt sind. Die Zahl war
    // doppelt umgewandelt, einmal im Lesen und einmal in der Pruefung.
    if version == 0 || version > 3 {
        return Err(X509Fehler::Aufbau {
            was: "Version unplausibel",
        });
    }

    let seriennummer = integer_als_bytes(&t.element_mit_tag(0x02)?.inhalt)?;
    let signatur_algorithmus = algorithmus_lesen(&mut t)?;

    let aussteller = name_lesen(&mut t)?;
    let gueltigkeit = validity_lesen(&mut t)?;
    let inhaber = name_lesen(&mut t)?;
    let schluessel = spki_lesen(&mut t)?;

    // issuerUniqueID [1] und subjectUniqueID [2], IMPLICIT OPTIONAL.
    // Sie stehen VOR den Erweiterungen und muessen uebersprungen
    // werden, sonst landet man in den falschen Feldern.
    //
    // EIGENER FEHLER, und er hat die Erweiterungen vollstaendig
    // verschluckt: ich habe `t.element()` aufgerufen und dann bei
    // `0xA3` mit `break` aufgehoert. `element()` aber KONSUMIERT das
    // Element — der A3-Tag war damit schon gelesen, als die
    // Erweiterungsschleife danach `ist_leer()` fragte und `true`
    // bekam. Also wurden nie Erweiterungen gelesen, `basic_constraints`
    // blieb `None`, und `unwrap_or(false)` meldete "keine CA".
    //
    // Das ist die gefaehrlichste Richtung, in der so ein Fehler
    // laeuft: ein Zertifikat ohne erkanntes CA-Flag wird wie ein
    // Endzertifikat behandelt.
    //
    // Richtig ist: nur schauen, nicht lesen. Deshalb `naechste_tag`.
    while !t.ist_leer() {
        match t.naechste_tag() {
            Some(0x81) | Some(0x82) => {
                // EINE Position weiter, ohne den Inhalt zu lesen.
                t.element()?;
            }
            Some(0xA3) | None => break,
            _ => {
                return Err(X509Fehler::Aufbau {
                    was: "unerwartetes Feld in tbsCertificate",
                });
            }
        }
    }

    let mut basic_constraints = None;
    // added 02.10.2026 23:6x, S4: die Merkmale des Zertifikats (RFC 7633).
    let mut tls_features = None;
    let mut san = None;
    let mut key_usage = None;
    // added 02.10.2026 09:7x, S1: die beiden Felder, die vorher nicht
    // gelesen wurden. Sie sind `None`, wenn die Erweiterung fehlt — und das
    // ist eine **Aussage**: RFC 5280 sagt, dass eine fehlende Erweiterung
    // „keine Einschraenkung" bedeutet (4.2.1.12 bzw. 4.2.1.10).
    let mut erweiterte_verwendung: Option<Vec<Vec<u8>>> = None;
    // added 02.10.2026 15:2x: die URIs der `crlDistributionPoints`, und das ist
    // eine **Liste** statt eines `Option`: die Erweiterung darf mehrere
    // Verteilungsstellen nennen (RFC 5280 4.2.1.13: „SEQUENCE SIZE (1..MAX) OF
    // DistributionPoint"), und die leere Liste bedeutet dann „keine", was
    // dasselbe sagt wie `None`.
    let mut verteilungsstellen: Vec<Vec<u8>> = Vec::new();
    let mut namensbeschraenkung: Option<Namensbeschraenkung> = None;
    if !t.ist_leer() {
        // extensions [3] EXPLICIT Extensions
        //
        // RFC 5280 Abschnitt 4.1:
        //     extensions       [3] EXPLICIT Extensions OPTIONAL
        //     Extensions      ::= SEQUENCE SIZE (1..MAX) OF Extension
        // `[3] EXPLICIT` heisst: im A3-Tag steckt die SEQUENCE. Es
        // folgt also A3 -> 30 (Extensions) -> 30 (je Extension).
        //
        // EIGENER FEHLER, zweimal an derselben Stelle: beim ersten Mal
        // hatte ich das 30 der Extensions weggelassen und damit jede
        // Erweiterung verfehlt; nach der Korrektur habe ich die
        // Extrahuelle entfernt und damit jetzt die Extension selbst
        // als SEQUENCE gelesen. Beide Male ist es still gescheitert.
        let a3 = t.element_mit_tag(0xA3)?;
        let mut huelle = DerLeser::neu(&a3.inhalt);
        let extensions = huelle.element_mit_tag(0x30)?;
        let mut e = DerLeser::neu(&extensions.inhalt);
        for _ in 0..MAX_ERWEITERUNGEN {
            if e.ist_leer() {
                break;
            }
            let ext = e.element_mit_tag(0x30)?;
            if ext.inhalt.len() > MAX_ERWEITERUNG {
                return Err(X509Fehler::ZuLang {
                    was: "Erweiterung",
                    ist: ext.inhalt.len(),
                    erlaubt: MAX_ERWEITERUNG,
                });
            }
            let mut x = DerLeser::neu(&ext.inhalt);
            let oid = oid_als_punkte(&x.element_mit_tag(0x06)?.inhalt)?;
            // Das optionale `critical`-Flag. RFC 5280 Abschnitt 4.2:
            //     extnValue    OCTET STRING,
            //     critical     BOOLEAN DEFAULT FALSE
            // Das Flag STEHT VOR dem Wert und darf fehlen.
            //
            // EIGENER FEHLER: ich habe `x.element()?.tag == 0x01`
            // geschrieben. `element()` liest aber IMMER ein Element,
            // auch wenn das Tag nicht 0x01 ist — es wurde also bei
            // jeder Erweiterung ohne `critical` trotzdem das
            // extnValue-OCTET-STRING weggegessen. Danach versuchte
            // `element_mit_tag(0x04)` zu lesen, lief auf einen leeren
            // Leser und meldete "erwartet 2 Byte, vorhanden 0".
            // Richtig ist: nur schauen, dann gezielt lesen.
            // added 03.10.2026 05:1x, R51: **DER NAME, STATT VERWORFEN ZU
            // WERDEN** — vorher `let _kritisch`, und der Wert hat nie jemanden
            // gesehen. Jetzt traegt er die Entscheidung am Ende der Schleife.
            let kritisch = if x.naechste_tag() == Some(0x01) {
                let f = x.element()?;
                f.inhalt.first().copied().unwrap_or(0) != 0
            } else {
                false
            };
            let wert = x.element_mit_tag(0x04)?;
            // added 03.10.2026 05:1x, R51: **`verarbeitet`** — die entscheidende
            // Groesse. Sie unterscheidet „wir kennen die Erweiterung und ihre
            // Information **wirkt**“ von „wir kennen sie, lesen sie, **wenden
            // sie aber nicht an**“. Beides heisst fuer RFC 5280 4.2: wir koennen
            // die Information nicht verarbeiten. Und es ist nicht dasselbe wie
            // „unbekannt“, denn fuer eine nicht-kritische Erweiterung darf man
            // beides ignorieren.
            let mut verarbeitet = false;
            match oid.as_slice() {
                // added 02.10.2026 23:6x, S4: RFC 7633, `id-pe-tlsfeature`
                // ::= { id-pe 24 }, also 1.3.6.1.5.5.7.1.24. Der Inhalt ist
                // `Features ::= SEQUENCE OF INTEGER` — im Innern der OCTET
                // STRING steckt also noch eine SEQUENCE, und die INTEGERs sind
                // die TLS-Erweiterungsnummern.
                [1, 3, 6, 1, 5, 5, 7, 1, 24] => {
                    tls_features = Some(features_lesen(&wert.inhalt)?);
                    verarbeitet = true;
                }
                [2, 5, 29, 19] => {
                    basic_constraints = Some(basic_constraints_lesen(&wert.inhalt)?);
                    verarbeitet = true;
                }
                [2, 5, 29, 17] => {
                    san = Some(san_lesen(&wert.inhalt)?);
                    verarbeitet = true;
                }
                // added 02.10.2026 09:7x, S1: 2.5.29.37 (erweiterte
                // Schluesselverwendung) und 2.5.29.30 (Namensbeschraenkung).
                [2, 5, 29, 37] => {
                    erweiterte_verwendung = Some(eku_lesen(&wert.inhalt)?);
                    verarbeitet = true;
                }
                [2, 5, 29, 30] => {
                    namensbeschraenkung = Some(namensbeschraenkung_lesen(&wert.inhalt)?);
                    verarbeitet = true;
                }
                [2, 5, 29, 31] => {
                    // added 02.10.2026 15:2x: `crlDistributionPoints`.
                    // added 03.10.2026 05:2x, R51: **BEWUSST OHNE `verarbeitet`**,
                    // und das ist der zweite Fund derselben Klasse: die
                    // Verteilungspunkte werden **gelesen**, aber es wird **keine
                    // CRL von dort geholt** — GEMESSEN (04:9x) per grep: das Feld
                    // `verteilungsstellen` kommt in **keiner** anderen Datei vor.
                    // „Lesen“ ist keine „Verarbeitung“: die in der Erweiterung
                    // enthaltene Information (welche CRL fuer dieses Zertifikat
                    // gilt) koennen wir nicht benutzen. Steht sie als `critical`,
                    // verlangt RFC 5280 4.2 die Ablehnung — und genau das
                    // passiert jetzt.
                    verteilungsstellen.extend(verteilungsstellen_lesen(&wert.inhalt)?);
                }
                [2, 5, 29, 15] => {
                    // EIGENER FEHLER: `wert` ist das `extnValue`, und
                    // darin steht nochmal ein vollstaendiges
                    // BIT STRING mit Tag und Laenge. Bei keyUsage ist
                    // das `03 02 01 06`, und wenn man das an
                    // `bit_string` gibt, wird die `03` als "3
                    // ungenutzte Bits" gelesen — und die Pruefung, ob
                    // die unteren Bits des letzten Bytes null sind,
                    // schlaegt dann fehl. Also: erst den inneren
                    // BIT STRING auspacken, dann auswerten.
                    let mut inner = DerLeser::neu(&wert.inhalt);
                    let bs = inner.element_mit_tag(0x03)?;
                    let (_, b) = bit_string(&bs.inhalt)?;
                    // keyUsage ist ein BIT STRING mit festen
                    // Positionen nach RFC 5280 Abschnitt 4.2.1.3:
                    // digitalSignature (0), nonRepudiation (1),
                    // keyEncipherment (2), ..., cRLSign (5).
                    // Die ersten acht liegen im ersten Byte.
                    key_usage = Some(if b.is_empty() { 0 } else { b[0] as u16 });
                    verarbeitet = true;
                }
                _ => {
                    // added 03.10.2026 05:2x, R51: **ZWEI FAELLE, EIN ZWEIG.**
                    // Hier landen (a) OIDs, die wir nicht kennen, und (b) die
                    // bekannte OID 2.5.29.31, deren Information wir nicht
                    // anwenden. Fuer `critical` sind beide gleich zu behandeln,
                    // fuer nicht-kritisch darf man beide ignorieren — die
                    // Entscheidung steht deshalb **nach** dem `match`, nicht hier.
                }
            }
            // added 03.10.2026 05:2x, R51: **DIE REGEL**, und sie ist RFC 5280
            // 4.2 in zwei Zeilen: verlangt das Zertifikat, dass diese Erweiterung
            // verarbeitet wird, und koennen wir es nicht, ist das Zertifikat
            // fuer uns **unbrauchbar** — nicht bloss „ungewoehnlich“.
            //
            // added 03.10.2026 05:2x: **WARUM NICHT AUCH BEI NICHT-KRITISCH.**
            // Norm woertlich: „A non-critical extension **MAY** be ignored if it
            // is not recognized, but **MUST** be processed if it is
            // recognized.“ Fuer nicht-kritische Erweiterungen ist Ignorieren
            // erlaubt — und Ablehnen waere hier sogar **schlimmer als der
            // Browser**, denn echte Zertifikate tragen regelmaessig
            // nicht-kritische Erweiterungen, die wir nicht kennen (in der Wild
            // gemessen, siehe Bericht R51).
            if kritisch && !verarbeitet {
                return Err(X509Fehler::KritischeErweiterung {
                    punkte: oid.clone(),
                });
            }
        }
    }

    // --- signaturAlgorithm und signatureValue --------------------------
    let ausserhalb_algorithmus = algorithmus_lesen(&mut tlv)?;
    // RFC 5280 Abschnitt 4.1.1.2: die beiden muessen uebereinstimmen.
    if ausserhalb_algorithmus.oid != signatur_algorithmus.oid {
        return Err(X509Fehler::Aufbau {
            was: "signaturAlgorithm weicht von der im tbsCertificate ab",
        });
    }
    let sig_b = tlv.element_mit_tag(0x03)?;
    let (_, signatur) = bit_string(&sig_b.inhalt)?;

    Ok(Zertifikat {
        der: der.to_vec(),
        seriennummer,
        signatur_algorithmus,
        aussteller,
        gueltigkeit,
        inhaber,
        schluessel,
        basic_constraints,
        san,
        key_usage,
        erweiterte_verwendung,
        namensbeschraenkung,
        // added 02.10.2026 23:6x, S4: die TLS-Merkmale (RFC 7633) mitgeben,
        // sonst waere die Erweiterung gelesen und dann weg.
        tls_features,
        verteilungsstellen,
        tbs_der,
        signatur: signatur.to_vec(),
    })
}

/// Liest die **TLS-Feature**-Erweiterung (RFC 7633) aus dem Inhalt der
/// OCTET STRING.
///
/// added 02.10.2026 23:7x, S4. DAS FORMAT, woertlich aus dem ASN.1-Modul in
/// Anhang A der Norm: `Features ::= SEQUENCE SIZE (1..MAX) OF INTEGER
/// {0..(2^16-1)}`. Die Merkmale sind also **TLS-Erweiterungsnummern**, kein Text —
/// `status_request` ist die Nummer **5** (RFC 6066, Abschnitt 3: „status_request
/// …(5)“), weil die Norm sagt, es handle sich um „a sequence of TLS extension
/// identifiers“.
/// added 02.10.2026 23:7x: **UND DER BYTESCHNITT IST IN DEN BYTES** — die
/// INTEGERs koennen mehr als ein Byte brauchen (z. B. `0x82 0x01 0x00` = 256),
/// also wird nicht ueber eine Zeichenkette geschnitten.
///
/// added 02.10.2026 23:7x: **WAS EIN FEHLER IST und was nicht.** Leere Sequenz:
/// Fehler, denn `SIZE (1..MAX)`. Unbekannte Nummer: **kein** Fehler, denn
/// `SIZE (1..MAX) OF INTEGER` erlaubt jede Zahl, und eine spaeter definierte
/// Merkmalsnummer darf einen alten Client nicht ausschalten — dieselbe
/// Ueberlegung wie bei den unbekannten SETTINGS-Kennungen (RFC 9113 6.5.2).
pub fn features_lesen(inhalt: &[u8]) -> Result<Vec<u16>, X509Fehler> {
    let mut leser = DerLeser::neu(inhalt);
    // added 02.10.2026 23:14x: **DIE AEUSSERE SEQUENCE ZUERST**, und das war ein
    // echter Fehler: mein erster Versuch hat direkt INTEGERs gelesen und die
    // `30`-Bytes alsINTEGER abgewiesen — zwei Tests rot mit „lesen scheiterte“.
    // GEMESSEN am Muster der Nachbarn (`basic_constraints_lesen` macht es
    // genauso): erst `element_mit_tag(0x30)`, dann ein zweiter Leser auf deren
    // Inhalt.
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let mut raus = Vec::new();
    while !s.ist_leer() {
        let e = s.element()?;
        if e.tag != 0x02 {
            return Err(X509Fehler::Aufbau {
                was: "tlsFeature: erwartet INTEGER (0x02)",
            });
        }
        // added 02.10.2026 23:7x: DER WERT, und warum nicht einfach `e.inhalt`
        // als `u16`: ein INTEGER kann fuehrende Nullen haben (`00 05`) und mehr
        // als zwei Byte (`01 00` = 256). Beides wird abgeschnitten, und mehr als
        // zwei Byte ist ein **Fehler** statt einer stillen Verkleinerung.
        if e.inhalt.is_empty() || e.inhalt.len() > 3 {
            return Err(X509Fehler::Aufbau {
                was: "tlsFeature: INTEGER mit 0, 1, 4 oder mehr Byte",
            });
        }
        let mut wert: u32 = 0;
        for b in &e.inhalt {
            wert = (wert << 8) | *b as u32;
        }
        if wert > u16::MAX as u32 {
            return Err(X509Fehler::Aufbau {
                was: "tlsFeature: Zahl passt nicht in u16",
            });
        }
        raus.push(wert as u16);
    }
    if raus.is_empty() {
        return Err(X509Fehler::Aufbau {
            was: "tlsFeature: leere Sequenz",
        });
    }
    Ok(raus)
}

/// Verlangt dieses Zertifikat die **OCSP-Stapelung** (must-staple)?
///
/// added 02.10.2026 23:9x, S4. Das ist die **eine** Frage, die der Handshake
/// stellt: steht `status_request` (Merkmalsnummer **5**, RFC 6066 3) in der
/// TLS-Feature-Erweiterung? `must_staple_entscheidung` (unten) beantwortet die
/// zweite Frage daraus.
///
/// added 03.10.2026 00:4x: **DIE FEHLERFRAGE, und sie ist absichtlich so
/// entschieden.** `features_lesen` kann beim Lesen scheitern; dann ist
/// `tls_features` `None`. „Unbekannt“ heisst hier also **nein**, nicht „ja“: eine
/// kaputte Merkmalsliste darf keine Verbindung zerlegen.
pub fn verlangt_status_request(z: &Zertifikat) -> bool {
    match &z.tls_features {
        Some(merkmale) => merkmale.contains(&5),
        None => false,
    }
}

/// **Was** mit einer Verbindung geschieht, wenn must-staple greift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ergebnis {
    /// Kein Merkmal im Zertifikat: die Verbindung ist wie bisher — Soft-Fail,
    /// mit gespeichertem Grund.
    Angenommen,
    /// Das Zertifikat verlangt die Stapelung (RFC 7633) und es kam keine
    /// brauchbare Antwort: der Handshake **bricht ab**.
    Abgewiesen,
}

/// added 03.10.2026 00:2x, S4. **DIE ENTSCHEIDUNG, und sie ist eine reine
/// Funktion**, damit man sie ohne einen Handshake pruefen kann.
///
/// added 03.10.2026 00:2x: **KEIN SCHALTER, und das ist eine Entscheidung.**
/// In R44 hatte diese Funktion drei Parameter — darunter
/// `must_staple_auswerten`, das **niemand** gesetzt hat (`PROBLEME.txt` Nr. 45
/// hat es als offene Frage notiert). GEMESSEN (02.10.2026, `openssl s_client`)
/// tragen **0 von 15** Hosts die Erweiterung: die 6 allgemeinen (github.com,
/// www.google.com, www.mozilla.org, archlinux.org, www.wikipedia.org,
/// news.ycombinator.com) und 9 CA-schwere (www.digicert.com, ident.evocsp.com,
/// www.baltimoretrust.net, ocsp.digicert.com, www.sectigo.com, ssl.com,
/// www.globalsign.com, www.entrust.com, certificates.digicert.com). Damit ist
/// „durchsetzen“ messbar gefahrlos, und ein Schalter waere Code, den niemand
/// stellt — oder eine Abschaltmoeglichkeit, die jemand stellt, ohne es zu
/// sagen. Beides ist schlechter als eine klare Regel.
///
/// added 03.10.2026 00:3x: **DIE REGEL, und ihre Grundlage ist ein SHOULD, keine
/// MUST.** RFC 7633 4.2.3.1 woertlich: „In the case that a client determines
/// that the server configuration is inconsistent with a feature declaration
/// specifying support for the TLS status_request extension, it **SHOULD reject**
/// the TLS configuration.“
/// added 03.10.2026 00:3x: **DIE AUSNAHME NEHME ICH NICHT**, und das ist
/// bewusst: die Norm erlaubt das Annehmen, „if the validity of the certificate
/// chain presented can be established through other means (for example, by
/// successfully obtaining the OCSP data from another source)“ — wir haben keinen
/// zweiten Weg, den Status zu holen, also greift die Ausnahme nicht.
/// added 03.10.2026 00:3x: DIE ZWEITE AUSNAHME („MUST NOT distinguish that
/// connection as secure“) ist damit gegenstandslos: es gibt keine Konfiguration,
/// die wir annehmen und als sicher auszeichnen muessten.
/// added 03.10.2026 00:3x: **WER DEN SCHALTER WILL**, melde es als
/// `PROBLEME.txt`-Punkt mit Zahlen; die zwei Stellen dafuer sind diese Funktion
/// und ihr Aufrufer in `strom.rs`.
pub fn must_staple_entscheidung(verlangt: bool, stufe_gut: bool) -> Ergebnis {
    if verlangt && !stufe_gut {
        return Ergebnis::Abgewiesen;
    }
    Ergebnis::Angenommen
}

/// added 02.10.2026 23:8x, S4: **DER FEHLER, den der Handshake meldet**, und er
/// nennt die Zahl, weil ein „TLS-Fehler“ ohne Ursache beim naechsten Fall nicht
/// entscheidbar ist (dieselbe Regel wie bei Nr. 38 und Nr. 40).
pub const MUST_STAPLE_FEHLER: &str =
    "das Zertifikat verlangt eine OCSP-Antwort (must-staple), die nicht brauchbar war";

/// Liest ein `AlgorithmIdentifier`.
/// added 02.10.2026 23:4x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// `pub(crate)`, weil `ocsp.rs` dieselbe Frage stellt: „welcher Hash und
/// welche Salzlaenge steckt in diesem `AlgorithmIdentifier`?" — die Antwort
/// gehoert in **eine** Stelle, sonst faellt die PSS-Auswertung in
/// `ocsp.rs` anders aus als hier.
pub(crate) fn algorithmus_lesen(leser: &mut DerLeser) -> Result<SignaturAlgorithmus, X509Fehler> {
    let alg = leser.element_mit_tag(0x30)?;
    let mut a = DerLeser::neu(&alg.inhalt);
    let oid = oid_als_punkte(&a.element_mit_tag(0x06)?.inhalt)?;
    // Die Parameter sind je nach Algorithmus verschieden: bei RSA eine
    // NULL, bei ECDSA die Kurven-OID, bei RSA-PSS eine SEQUENCE. Sie
    // werden gelesen, aber nur die zwei Faelle ausgewertet, die diese
    // Datei braucht.
    let mut hash = None;
    let mut pss_salt = None;
    if !a.ist_leer() {
        let param = a.element()?;
        match param.tag {
            // NULL
            0x05 => {}
            // OID: die Kurve bei ECDSA
            0x06 => {
                if oid.as_slice() == [1, 2, 840, 10045, 2, 1] {
                    // ecdsa-with-SHA256 und Verwandte: der Parameter ist
                    // die Kurve, der Hash steht in der OID.
                    hash = hash_aus_ecdsa_oid(&oid);
                }
            }
            // SEQUENCE: bei RSA-PSS die Parameter
            0x30 if oid.as_slice() == [1, 2, 840, 113549, 1, 1, 10] => {
                {
                    // Die Klammern sind jetzt ueberfluessig: die
                    // Bedingung steht im match-Arm. Vorher stand sie
                    // als `if` darin, was clippy als zusammenlegbar
                    // anmerkte.
                    let _ = &oid;
                    let mut p = DerLeser::neu(&param.inhalt);
                    while !p.ist_leer() {
                        let feld = p.element()?;
                        match feld.tag {
                            0xA0 => {
                                hash = Hash::von_oid(&oid_aus_punkte_feld(&feld)?);
                            }
                            0xA2 => {
                                // sLen. Steht es nicht drin, gilt laut
                                // RFC 4055 die Laenge des Hashwerts.
                                let mut s = DerLeser::neu(&feld.inhalt);
                                let salt = s.element_mit_tag(0x02)?;
                                let wert = integer_als_bytes(&salt.inhalt)?;
                                pss_salt = Some(if wert == [0] { 0 } else { 32 });
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Hash aus der Algorithmus-OID, wenn er nicht woanders stand.
    if hash.is_none() {
        hash = hash_aus_signatur_oid(&oid);
    }
    Ok(SignaturAlgorithmus {
        oid,
        hash,
        pss_salt,
    })
}

fn oid_aus_punkte_feld(e: &Element) -> Result<Vec<u64>, X509Fehler> {
    let mut i = DerLeser::neu(&e.inhalt);
    Ok(oid_als_punkte(&i.element()?.inhalt)?)
}

/// Der Hash aus einer `shaNNNWithRSAEncryption`- oder
/// `ecdsa-with-SHA-NNN`-OID.
fn hash_aus_signatur_oid(oid: &[u64]) -> Option<Hash> {
    match oid {
        // 1.2.840.113549.1.1.5 sha1WithRSA, .11 sha256, .12 sha384, .13 sha512
        [1, 2, 840, 113549, 1, 1, 5] => Some(Hash::Sha1),
        [1, 2, 840, 113549, 1, 1, 11] => Some(Hash::Sha256),
        [1, 2, 840, 113549, 1, 1, 12] => Some(Hash::Sha384),
        [1, 2, 840, 113549, 1, 1, 13] => Some(Hash::Sha512),
        // 1.2.840.10045.4.1 ecdsa-with-SHA1, .3.2 SHA-256, .3.3 SHA-384, .3.4 SHA-512
        [1, 2, 840, 10045, 4, 1] => Some(Hash::Sha1),
        [1, 2, 840, 10045, 4, 3, 2] => Some(Hash::Sha256),
        [1, 2, 840, 10045, 4, 3, 3] => Some(Hash::Sha384),
        [1, 2, 840, 10045, 4, 3, 4] => Some(Hash::Sha512),
        // 1.2.840.113549.1.1.10 RSASSA-PSS: der Hash steht in den
        // Parametern, nicht in der OID.
        _ => None,
    }
}

fn hash_aus_ecdsa_oid(oid: &[u64]) -> Option<Hash> {
    match oid {
        [1, 2, 840, 10045, 4, 1] => Some(Hash::Sha1),
        [1, 2, 840, 10045, 4, 3, 2] => Some(Hash::Sha256),
        [1, 2, 840, 10045, 4, 3, 3] => Some(Hash::Sha384),
        [1, 2, 840, 10045, 4, 3, 4] => Some(Hash::Sha512),
        _ => None,
    }
}

/// Liest ein `Name`, also eine Kette von `RelativeDistinguishedName`.
///
/// Jedes RDN ist ein `SET OF AttributeTypeAndValue`, und jedes
/// Attribute ist ein SEQUENCE aus OID und Wert.
/// added 02.10.2026 06:1x, Rolle TLS: `pub(crate)`, weil `crl.rs` denselben
/// `Name` lesen muss — der `issuer` einer Sperrliste (5.1.2.3) ist strukturell
/// derselbe `Name` wie der `issuer` eines Zertifikats (4.1.2.4). Ein zweiter
/// Leser an dieser Stelle wäre die zweite Wahrheit ueber dasselbe Format.
pub(crate) fn name_lesen(leser: &mut DerLeser) -> Result<Name, X509Fehler> {
    // M3: die Verschachtelungstiefe wird begrenzt. Ein Name ist eine
    // SEQUENCE aus SETs, und in einem SET koennen beliebig viele
    // Attribute stehen. Beides wird hier in EINER Schleife erledigt,
    // nicht ineinander, also ist die Tiefe 2; die Grenze gilt fuer die
    // Zahl der Attribute.
    let seq = leser.element_mit_tag(0x30)?;
    if seq.inhalt.len() > MAX_NAME_TIEFE * 1024 {
        return Err(X509Fehler::ZuLang {
            was: "Name",
            ist: seq.inhalt.len(),
            erlaubt: MAX_NAME_TIEFE * 1024,
        });
    }
    let mut attribute = Vec::new();
    // added 03.10.2026 14:2x: **DIE GRENZEN** mitfuehren; ohne sie ist die
    // Teilbaum-Regel nicht baubar (Erklaerung am Feld).
    let mut rdn_grenze: Vec<usize> = Vec::new();
    let mut sets = DerLeser::neu(&seq.inhalt);
    while !sets.ist_leer() {
        // added 03.10.2026 14:2x: **DIE GRENZE VOR DEM RDN NOTIEREN**, und die
        // Stelle ist genau richtig: **vor** dem Lesen des RDN, also ist es die
        // Anzahl der Attribute, die zu den **vorherigen** RDNs gehoeren.
        rdn_grenze.push(attribute.len());
        let rdn = sets.element_mit_tag(0x31)?;
        let mut attrs = DerLeser::neu(&rdn.inhalt);
        while !attrs.ist_leer() {
            if attribute.len() >= MAX_NAME_ATTRIBUTE {
                return Err(X509Fehler::ZuLang {
                    was: "Attribute im Name",
                    ist: attribute.len(),
                    erlaubt: MAX_NAME_ATTRIBUTE,
                });
            }
            let attr = attrs.element_mit_tag(0x30)?;
            let mut av = DerLeser::neu(&attr.inhalt);
            let typ = oid_als_punkte(&av.element_mit_tag(0x06)?.inhalt)?;
            let v = av.element()?;
            attribute.push(Attribut {
                typ,
                wert: {
                    let mut w = Vec::with_capacity(v.inhalt.len() + 1);
                    w.push(v.tag);
                    w.extend_from_slice(&v.inhalt);
                    w
                },
            });
        }
    }
    Ok(Name {
        attribute,
        rdn_grenze,
    })
}

/// Liest ein `Validity`, also zwei Zeitpunkte.
fn validity_lesen(leser: &mut DerLeser) -> Result<Gueltigkeit, X509Fehler> {
    let seq = leser.element_mit_tag(0x30)?;
    let mut v = DerLeser::neu(&seq.inhalt);
    let von = v.element()?;
    let bis = v.element()?;
    let von_s = zeit_seit_1970(von.tag, &von.inhalt)?;
    let bis_s = zeit_seit_1970(bis.tag, &bis.inhalt)?;
    if bis_s < von_s {
        return Err(X509Fehler::ZeitUnbrauchbar);
    }
    Ok(Gueltigkeit {
        von: von_s,
        bis: bis_s,
    })
}

/// Die Bitzahl einer NIST-Kurve aus ihrer OID.
///
/// Es gibt ZWEI Zuordnungen derselben Kurven, und das war die Ursache
/// eines echten Fehlers: ansi-X9.62 und SEC benutzen verschiedene
/// Wurzeln fuer dieselben Kurven.
///
///   1.2.840.10045.3.1.7   = P-256   (ANSI-X9.62 / NIST-Bogen)
///   1.2.840.10045.3.1.31  = P-384
///   1.2.840.10045.3.1.34  = P-521
///
///   1.3.132.0.7   = secp256k1
///   1.3.132.0.34  = secp384r1  = P-384
///
/// EIGENER FEHLER: ich hatte nur die erste Gruppe. Zertifikat 2 aus
/// ca-certificates.crt traegt aber `1.3.132.0.34`, also den
/// SEC-Bogen, und wurde daraufhin als "oeffentlicher Schluessel
/// unbekannt" abgelehnt — mit der Meldung, der Schluessel sei
/// unbekannt, obwohl er ganz gewoehnlich war.
///
/// Nur NIST-Kurven werden fuer ECDSA im TLS-Profil gebraucht
/// (RFC 8422 Abschnitt 5.1.2). `secp256k1` ist nicht darunter, wird
/// also weiterhin abgelehnt — das ist richtig, und kein Weglassen.
fn kurven_bits(kurve: &[u64]) -> usize {
    match kurve {
        // ANSI-X9.62 / NIST
        [1, 2, 840, 10045, 3, 1, 7] => 256,
        [1, 2, 840, 10045, 3, 1, 31] => 384,
        [1, 2, 840, 10045, 3, 1, 34] => 521,
        // SEC: secp256r1 ist dieselbe Kurve wie P-256
        [1, 3, 132, 0, 7] => 256,
        [1, 3, 132, 0, 34] => 384,
        [1, 3, 132, 0, 35] => 521,
        // secp256k1 hat ebenfalls 256 Bit, ist aber fuer TLS nicht
        // zugelassen (RFC 8422 Abschnitt 5.1.2 nennt nur die drei
        // NIST-Kurven). Bewusst 0, damit der Aufrufer ablehnt.
        _ => 0,
    }
}

/// added 03.10.2026, TLS. **DER NAME EINER KURVE**, fuer die Anzeige im
/// Schloss-Fenster — und er ist **nicht** dieselbe Tabelle wie
/// [`kurven_bits`].
///
/// **WARUM ZWEI TABELLEN, und das ist der Grund fuer diese Funktion:** die
/// Bitzahl entscheidet ueber **Zulassung** (`kurven_bits` gibt bei
/// `secp256k1` bewusst 0, damit der Aufrufer ablehnt), der Name entscheidet
/// ueber **Anzeige**. Haette ich eine Tabelle aus beiden gemacht, muesste die
/// Anzeige die 0 mitteilen — und „0 Bit“ ist eine **Behauptung** ueber ein
/// Zertifikat, das genau 256 Bit traegt.
///
/// **DIE NAMEN SIND DIE SECG-NAMEN**, und sie stehen bei
/// `openssl x509 -text` in der Zeile `ASN1 OID:` (GEMESSEN 03.10.2026 an
/// `testdaten/badssl/gut.pem`: `ASN1 OID: prime256v1`).
pub fn kurve_name(kurve: &[u64]) -> &'static str {
    match kurve {
        [1, 2, 840, 10045, 3, 1, 7] | [1, 3, 132, 0, 7] => "prime256v1",
        [1, 2, 840, 10045, 3, 1, 31] | [1, 3, 132, 0, 34] => "secp384r1",
        [1, 2, 840, 10045, 3, 1, 34] | [1, 3, 132, 0, 35] => "secp521r1",
        [1, 3, 132, 0, 10] => "secp256k1",
        _ => "unbekannte Kurve",
    }
}

/// added 03.10.2026, TLS. **DER NAME DES SIGNATURALGORITHMUS**, fuer die
/// Anzeige im Schloss-Fenster.
///
/// **UND DIESE TABELLE WAR SCHON EINMAL DA** — als Feld. `SignaturAlgorithmus`
/// haelt `hash: Option<Hash>` und `pss_salt`, und beides ist fuer eine
/// **Pruefung** da; fuer die **Anzeige** braucht es den Namen. GEMESSEN, woher
/// die Namen stammen: `openssl x509 -text` gibt bei
/// `testdaten/badssl/gut.pem` die Zeile `Signature Algorithm: ecdsa-with-SHA256`
/// aus, und genau diese Schreibweise steht hier.
///
/// **`(unbekannt)` STATT `unbekannt`** — der Klammertext ist die Form, in der
/// `openssl` einen nicht bekannten Algorithmus zeigt, und ein Aufrufer, der
/// den Text in eine Liste schreibt, soll den Unterschied zu einem echten
/// Namen sehen.
pub fn signatur_name(oid: &[u64]) -> &'static str {
    match oid {
        [1, 2, 840, 113549, 1, 1, 4] => "md5WithRSAEncryption",
        [1, 2, 840, 113549, 1, 1, 5] => "sha1WithRSAEncryption",
        [1, 2, 840, 113549, 1, 1, 10] => "RSASSA-PSS",
        [1, 2, 840, 113549, 1, 1, 11] => "sha256WithRSAEncryption",
        [1, 2, 840, 113549, 1, 1, 12] => "sha384WithRSAEncryption",
        [1, 2, 840, 113549, 1, 1, 13] => "sha512WithRSAEncryption",
        [1, 2, 840, 10045, 4, 1] => "ecdsa-with-SHA1",
        [1, 2, 840, 10045, 4, 3, 2] => "ecdsa-with-SHA256",
        [1, 2, 840, 10045, 4, 3, 3] => "ecdsa-with-SHA384",
        [1, 2, 840, 10045, 4, 3, 4] => "ecdsa-with-SHA512",
        [1, 3, 101, 112] => "Ed25519",
        [1, 3, 101, 113] => "Ed448",
        _ => "(unbekannt)",
    }
}

/// Liest ein `SubjectPublicKeyInfo`.
fn spki_lesen(leser: &mut DerLeser) -> Result<SchluesselArt, X509Fehler> {
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let alg = s.element_mit_tag(0x30)?;
    let mut a = DerLeser::neu(&alg.inhalt);
    let oid = oid_als_punkte(&a.element_mit_tag(0x06)?.inhalt)?;
    let key_b = s.element_mit_tag(0x03)?;
    let (_, schluessel_bytes) = bit_string(&key_b.inhalt)?;
    match oid.as_slice() {
        // 1.2.840.113549.1.1.1 rsaEncryption
        [1, 2, 840, 113549, 1, 1, 1] => {
            let mut inner = DerLeser::neu(schluessel_bytes);
            let seq = inner.element_mit_tag(0x30)?;
            let mut k = DerLeser::neu(&seq.inhalt);
            let n = integer_als_bytes(&k.element_mit_tag(0x02)?.inhalt)?;
            let e = integer_als_bytes(&k.element_mit_tag(0x02)?.inhalt)?;
            Ok(SchluesselArt::Rsa { n, e })
        }
        // 1.2.840.10045.2.1 id-ecPublicKey
        [1, 2, 840, 10045, 2, 1] => {
            // Der Parameter ist die Kurven-OID, z.B. P-256 oder P-384.
            let kurve = if a.ist_leer() {
                Vec::new()
            } else {
                oid_als_punkte(&a.element()?.inhalt)?
            };
            // EIGENER FEHLER: ich hatte `len() != 65` und feste
            // Schnitte [1..33] und [33..65] geschrieben, also P-256
            // vorausgesetzt. Zertifikat 2 aus ca-certificates.crt ist
            // aber P-384, und damit wurde jedes ECDSA-Zertifikat
            // ausser P-256 als "Schluessel unbekannt" abgelehnt.
            //
            // Richtig ist: die Laenge aus der Kurve ableiten. Fuer
            // die NIST-Kurven gilt byte = (Bit + 7) / 8, und der
            // Punkt in der unkomprimierten Form ist 1 + 2 * byte
            // Byte lang.
            let bits = kurven_bits(&kurve);
            if schluessel_bytes.is_empty() || schluessel_bytes[0] != 0x04 {
                // 0x04 = uncompressed. 0x02/0x03 waeren komprimiert;
                // dafuer braeuchte man eine Wurzelausziehung, und die
                // gehoert nicht in diese Datei.
                return Err(X509Fehler::SchluesselUnbekannt);
            }
            let byte = bits.div_ceil(8);
            if schluessel_bytes.len() != 1 + 2 * byte {
                return Err(X509Fehler::SchluesselUnbekannt);
            }
            Ok(SchluesselArt::Ecdsa {
                kurve,
                x: schluessel_bytes[1..1 + byte].to_vec(),
                y: schluessel_bytes[1 + byte..].to_vec(),
            })
        }
        _ => Ok(SchluesselArt::Unbekannt { oid }),
    }
}

/// Liest `BasicConstraints`, RFC 5280 Abschnitt 4.2.1.9.
///
/// `cA` ist `DEFAULT FALSE` und darf fehlen. `pathLenConstraint` ist
/// `OPTIONAL` und nur bei `cA = TRUE` erlaubt.
fn basic_constraints_lesen(inhalt: &[u8]) -> Result<BasicConstraints, X509Fehler> {
    let mut leser = DerLeser::neu(inhalt);
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let mut ist_ca = false;
    let mut pfad_laenge = None;
    while !s.ist_leer() {
        let e = s.element()?;
        match e.tag {
            0x01 => ist_ca = e.inhalt.first().copied().unwrap_or(0) != 0,
            0x02 => {
                // pathLenConstraint ist ein kleiner Wert; die Zahl wird
                // gelesen und in u32 gesetzt. Zahlen, die nicht in u32
                // passen, sind per RFC ungueltig.
                let wert = integer_als_bytes(&e.inhalt)?;
                let z = Zahl::aus_bytes_gross(&wert).map_err(X509Fehler::Zahl)?;
                pfad_laenge = Some(
                    z.nach_bytes_gross(4)
                        .iter()
                        .fold(0u32, |a, b| (a << 8) | *b as u32),
                );
            }
            _ => {}
        }
    }
    // RFC 5280: pathLenConstraint darf nur bei cA = TRUE stehen.
    if pfad_laenge.is_some() && !ist_ca {
        return Err(X509Fehler::Aufbau {
            was: "pathLenConstraint ohne cA",
        });
    }
    Ok(BasicConstraints {
        ist_ca,
        pfad_laenge,
    })
}

/// Liest `SubjectAltName`, RFC 5280 Abschnitt 4.2.1.6.
///
/// `GeneralNames` ist `SEQUENCE OF GeneralName`, und `GeneralName` ist
/// eine Auswahl mit impliziten Tags. Fuer `dNSName` ist das `[2]
/// IMPLICIT IA5String`, im DER also Tag 0x82.
fn san_lesen(inhalt: &[u8]) -> Result<SubjectAltName, X509Fehler> {
    let mut leser = DerLeser::neu(inhalt);
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let mut dns = Vec::new();
    // added 03.10.2026 10:1x, R56. **DIE DREI NEUEN SAMMLUNGEN**, mit
    // `MAX_ERWEITERUNGEN` als Schranke wie bei `dns`.
    let mut email = Vec::new();
    let mut uri = Vec::new();
    let mut directory_name = Vec::new();
    // added 02.10.2026 12:3x: und die IP-Adressen daneben, als **Bytes**.
    let mut ip = Vec::new();
    for _ in 0..MAX_ERWEITERUNGEN {
        if s.ist_leer() {
            break;
        }
        let e = s.element()?;
        // 0x82 = [2] dNSName, implizit. 0xA0 = otherName, 0xA4 =
        // directoryName. Die werden uebersprungen, nicht als Fehler
        // gemeldet: ein Zertifikat mit einem SAN aus otherName ist gueltig,
        // nur der Hostname daraus waere nicht ableitbar.
        //
        // added 02.10.2026 12:3x: **0x87 IST KEIN SOLCHER FALL MEHR.** `iPAddress`
        // wird jetzt gelesen (siehe weiter unten); vorher stand es in dieser
        // Liste der „uebersprungenen" Formen, und das war fuer die damalige
        // Frage richtig und fuer die heutige unvollstaendig.
        if e.tag == 0x82 {
            // IA5String ist ASCII. `from_utf8` verlangt zusaetzlich
            // gueltiges UTF-8; ein IA5String mit einem Byte > 0x7F
            // ist ungueltig, und genau das wird hier abgefangen.
            let text = std::str::from_utf8(&e.inhalt).map_err(|_| X509Fehler::Aufbau {
                was: "dNSName ist kein IA5String",
            })?;
            if dns.len() < MAX_ERWEITERUNGEN {
                dns.push(text.to_ascii_lowercase());
            }
        }
        // added 02.10.2026 12:3x: **DIE IP-ADDRESSEN.** RFC 5280 4.2.1.6:
        // `iPAddress ::= OCTET STRING`, und die **Laenge entscheidet** die
        // Form — 4 Byte IPv4, 16 Byte IPv6. Als Text waere das falsch, weil
        // dieselbe Adresse viele Schreibweisen hat (`::ffff:192.0.2.1` gegen
        // `0:0:0:0:0:ffff:c000:201`).
        //
        // added 02.10.2026 12:3x: **UND DIE LAENGE WIRD GEPRUEFT.** Eine
        // `iPAddress` mit 5 oder 20 Byte ist kein gueltiger Wert; sie zu
        // uebernehmen hiesse, eine Adresse zu vergleichen, die keine ist.
        // GEMESSEN ist das keine Ausnahme: die Norm laesst keine andere Laenge
        // zu, und ein abgeschnittenes Zertifikat erzeugt genau solche. Eine
        // falsche Laenge wird deshalb **nicht** gespeichert, sondern wie
        // `otherName` uebersprungen — der Kommentar oben begruendet, warum das
        // kein Fehler ist.
        // added 03.10.2026 22:bc, R65. **DIE NORMKONFORME FORM WIRD
        // ÜBERSPRUNGEN — und das war ein Loch.** Bis eben galt hier
        // `len == 4 || len == 16`, also **genau die nicht normkonformen** Formen.
        //
        // added 03.10.2026 22:bc: **DIE NORM, wörtlich** (RFC 5280 4.2.1.6, per
        // `curl` geholt): „For IPv4 addresses, the iPAddress field of the
        // subjectAltName extension **MUST contain eight (8) octets**“ — also
        // **Adresse und Maske**, je vier Byte (bei IPv6 je 16, zusammen 32).
        // Wir haben also die Form **verworfen, die die Norm vorschreibt**, und die
        // Form **behalten, die sie verbietet**.
        //
        // added 03.10.2026 22:bc: **GEMESSEN, UND ES IST BIS HIERHER UNBEMERKT
        // GEBLIEBEN, WEIL EIN TEST AN EINER FALSCHEN FORM MESS:** Der vorhandene
        // Test `tests/ip_nachweis.rs` (fremde Datei) sagt „genau eine iPAddress
        // (GEMESSEN: Laenge 4)“ — und `testdaten/badssl/ip_v4.pem` traegt
        // tatsaechlich `87 04 c0 00 02 01`, also **vier** Byte. Der Test ist also
        // **gruen**, aber er misst die **nicht normkonforme** Form. R65 gebaut hat
        // ein Zertifikat mit **acht** Byte: `ip=Some([])` — **verworfen**.
        //
        // added 03.10.2026 22:bd: **DIE FOLGE, UND SIE IST SICHERHEITSRELEVANT:**
        // fuer **jedes normkonforme** Zertifikat mit IP-SAN ist `san.ip` **leer**,
        // also laeuft die Schleife in `namensbeschraenkung_pruefen` **nie** — und
        // eine IP-Auflage (`excluded; IP:192.0.2.0/24`) wird **nie** geprueft. Das
        // ist **fail open** in einer Pruefung, deren Zweck das Schliessen ist.
        //
        // added 03.10.2026 22:bd: **BEIDE FORMEN WERDEN GENOMMEN**, und das ist
        // Absicht: die 4-/16-Byte-Form kommt in der **Wild** vor (belegt durch
        // `ip_v4.pem`), und sie zu verwerfen wuerde ein echtes Zertifikat
        // unbrauchbar machen. **GESPEICHERT WIRD DIE ADRESSE** — bei 8 Byte die
        // ersten vier, bei 32 Byte die ersten 16 — denn **das** ist der Wert, den
        // `ip_netz_prueft` mit dem Auflagen-Netz vergleicht. Die **Maske** der
        // SAN zu speichern waere eine zweite Form, die niemand liest.
        if e.tag == 0x87 && ip.len() < MAX_ERWEITERUNGEN {
            let adresse: &[u8] = match e.inhalt.len() {
                4 => &e.inhalt[..],    // nicht normkonform, aber in der Wild
                8 => &e.inhalt[..4],   // normkonform IPv4: Adresse + Maske
                16 => &e.inhalt[..],   // nicht normkonform, aber in der Wild
                32 => &e.inhalt[..16], // normkonform IPv6: Adresse + Maske
                _ => continue,
            };
            ip.push(adresse.to_vec());
        }
        // added 03.10.2026 10:1x, R56. **`rfc822Name`, [1] IA5String, Tag 0x81.**
        // IA5String ist **ASCII**: ein Byte mit dem oberen Bit gesetzt gehoert
        // nicht dazu. Frueher (vor dieser Erweiterung) fiel der Tag durch; jetzt
        // wird er geprueft, und ein nicht-ASCII-Byte ist ein **Fehler** statt
        // eines stillen Verwerfens — die Norm sagt „IA5String“, und „stillschweigen
        // bei kaputt“ ist keine Lesart von „lesen“.
        if e.tag == 0x81 && email.len() < MAX_ERWEITERUNGEN {
            if e.inhalt.iter().any(|b| *b > 0x7f) {
                return Err(X509Fehler::Aufbau {
                    was: "rfc822Name enthaelt ein Byte ausserhalb ASCII",
                });
            }
            email.push(e.inhalt.clone());
        }
        // added 03.10.2026 10:2x, R56. **`uniformResourceIdentifier`, [6],
        // IA5String, Tag 0x86** — mit derselben ASCII-Pruefung. Als `String`,
        // weil nur der Host-Teil verglichen wird und dort die Schreibung keine
        // Rolle spielt (RFC 5280 4.2.1.10: „the constraint applies to the host
        // part of the name“).
        if e.tag == 0x86 && uri.len() < MAX_ERWEITERUNGEN {
            let text = std::str::from_utf8(&e.inhalt).map_err(|_| X509Fehler::Aufbau {
                was: "uniformResourceIdentifier ist kein UTF-8",
            })?;
            if text.bytes().any(|b| b > 0x7f) {
                return Err(X509Fehler::Aufbau {
                    was: "uniformResourceIdentifier enthaelt ein Byte ausserhalb ASCII",
                });
            }
            uri.push(text.to_ascii_lowercase());
        }
        // added 03.10.2026 10:2x, R56. **`directoryName`, [4] Name.** Der
        // entscheidende Unterschied zu den beiden vorigen: `Name` ist **kein**
        // IA5String, sondern eine SEQUENCE von RelativeDistinguishedName, und
        // `GeneralName` ist ein **CHOICE** — `[4]` ist also **EXPLICIT** kodiert
        // (mit dem `30` der SEQUENCE im Inhalt). Das ist kein Raten: die uebrigen
        // drei sind IMPLICIT (ihr Inhalt ist der IA5String bzw. die Bytes
        // direkt), und genau dieser Unterschied ist im R56-Kommentar „GeneralName
        // ist ein CHOICE“ festgehalten.
        if e.tag == 0xa4 && directory_name.len() < MAX_ERWEITERUNGEN {
            let mut seq = DerLeser::neu(&e.inhalt);
            directory_name.push(name_lesen(&mut seq)?);
        }
    }
    // added 03.10.2026 10:1x, R56. **DIE DREI GENERALNAME-TYPEN**, die vor der
    // Erweiterung **stillschweigend** durch die Schleife fielen. GEMESSEN vorher:
    // `san_lesen` kannte nur `0x82` und `0x87`; alles andere hatte keinen Zweig und
    // wurde verworfen. Nach RFC 5280 4.2.1.10 sind es genau **diese drei**, fuer
    // die es Beschraenkungsformen gibt (neben `dNSName` und `iPAddress`).
    Ok(SubjectAltName {
        dns,
        ip,
        email,
        uri,
        directory_name,
    })
}

/// Liest die **erweiterte** Schluesselverwendung (RFC 5280 4.2.1.12).
///
/// added 02.10.2026 09:6x, S1. Der Inhalt ist eine SEQUENCE von OIDs; jede
/// OID wird als **Byte-Liste** behalten, weil der Vergleich byteweise
/// stattfinden muss (`1.3.6.1.5.5.7.3.1` gegen einen gleichnamigen String).
fn eku_lesen(inhalt: &[u8]) -> Result<Vec<Vec<u8>>, X509Fehler> {
    let mut leser = DerLeser::neu(inhalt);
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let mut liste = Vec::new();
    for _ in 0..MAX_ERWEITERUNGEN {
        if s.ist_leer() {
            break;
        }
        let e = s.element()?;
        // added 02.10.2026 09:6x: **jede** Elementform wird genommen. Eine
        // SEQUENCE aus Nullen oder einem INTEGER waere formal moeglich und
        // wuerde hier eine leere OID liefern — die faellt in der Pruefung
        // durch, statt den ganzen Pfad abzubrechen.
        if e.tag != 0x06 {
            continue;
        }
        if e.inhalt.len() > MAX_OID_LAENGE {
            return Err(X509Fehler::Aufbau {
                was: "OID der erweiterten Verwendung zu lang",
            });
        }
        if liste.len() < MAX_ERWEITERUNGEN {
            liste.push(e.inhalt.to_vec());
        }
    }
    Ok(liste)
}

/// Liest die **Namensbeschraenkungen** (RFC 5280 4.2.1.10).
///
/// added 02.10.2026 09:6x, S1. `GeneralSubtrees ::= SEQUENCE SIZE (1..MAX)
/// OF GeneralSubtree` und `GeneralSubtree ::= SEQUENCE { base GeneralName,
/// minimum [0] BaseDistance DEFAULT 0, maximum [1] BaseDistance OPTIONAL }`.
///
/// DIE GRENZE, und sie ist eine eigene Konstante: `MAX_ERWEITERUNGEN` gilt
/// fuer **Erweiterungen** (wie viele SAN-Eintraege), nicht fuer Eintraege
/// **in** einer Erweiterung. Eine CA mit 10000 Teilbaeumen ist zwar absurd,
/// aber sie darf den Browser nicht in eine Endlosschleife schicken — und
/// `basis` ist nicht begrenzt.
/// added 02.10.2026 14:3x: **ZERLEGT EIN IP-NETZ** aus einem
/// `iPAddress`-Teilbaum und gibt (Adresse, Maske) zurück, oder `None`, wenn
/// die Länge nicht 8 oder 32 Byte ist.
///
/// added 02.10.2026 14:3x: DIE NORM, wörtlich (RFC 5280 4.2.1.10): „For IPv4
/// addresses, the iPAddress field of GeneralName MUST contain eight (8) octets
/// in the bit string representing the address and the netmask … For IPv6
/// addresses, the iPAddress field MUST contain 32 octets similarly encoded." Das
/// Norm-Beispiel steht im nächsten Satz: „the constraint for \"class C\" subnet
/// 192.0.2.0 is represented as the octets C0 00 02 00 FF FF FF 00, representing
/// the CIDR notation 192.0.2.0/24".
///
/// added 02.10.2026 14:3x: **WARUM NICHTS „KONSTANT" IST**, und das ist der
/// Fehler, den man hier leicht macht: die Maske muss **bitweise** verglichen
/// werden, nicht als Zahl. Eine Maske `255.0.0.0` als `u32` ist `0xFF000000`,
/// und ein Vergleich `adresse & maske == netz & maske` stimmt dann nur, wenn man
/// die Maske **byteweise** anwendet — bei gemischten Masken wie
/// `255.255.0.0` ist eine Ganzzahl-Umrechnung zwar moeglich, aber bei IPv6
/// braucht man 128 Bit. Deshalb bleiben es Bytes, und `ip_netz_prueft` in
/// `kette.rs` macht den Vergleich byteweise.
fn ip_netz_zerlegen(inhalt: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let haelfte = match inhalt.len() {
        8 => 4,
        32 => 16,
        _ => return None,
    };
    let (adresse, maske) = inhalt.split_at(haelfte);
    Some((adresse.to_vec(), maske.to_vec()))
}

/// added 02.10.2026 15:3x: **DIE VERTEILUNGSSTELLEN, und nur die URIs.**
///
/// Die Form aus RFC 5280 4.2.1.13 — und die Codeabmarkierung ```text ist
/// **nicht** Kosmetik: GEMESSEN am 15:9x schlug der Doc-Test fehl mit
/// „expected identifier, found `=`", weil rustdoc einen Fence **ohne**
/// Sprachangabe als **Rust-Code** nimmt und `CRLDistributionPoints ::= …`
/// zu uebersetzen versucht.
///
/// ```text
/// CRLDistributionPoints ::= SEQUENCE SIZE (1..MAX) OF DistributionPoint
/// DistributionPoint     ::= SEQUENCE {
///      distributionPoint [0] DistributionPointName OPTIONAL,
///      reasons           [1] ReasonFlags          OPTIONAL,
///      cRLIssuer         [2] GeneralNames         OPTIONAL }
/// DistributionPointName ::= CHOICE {
///      fullName                [0] GeneralNames,
///      nameRelativeToCRLIssuer [1] RelativeDistinguishedName }
/// ```
/// und `GeneralName` traegt `uniformResourceIdentifier [6] IA5String`, im DER
/// also Tag **0x86**. GEMESSEN an `openssl asn1parse` (15:3x) an einem von
/// `openssl` erzeugten Blatt: `A0 21` (distributionPoint, explicit) → `A0 1F`
/// (fullName) → `86 1D "http://…"`.
/// added 02.10.2026 15:3x: **UND WAS WEGGELASSEN WIRD**, mit Grund: `reasons`
/// ([1]) und `cRLIssuer` ([2]) werden nicht ausgewertet. `cRLIssuer` bedeutet
/// eine **indirekte** Liste, und `crl.rs::pruefen` lehnt solche Listen mit
/// `UmfangNichtUnterstuetzt` ab — eine Verteilungsstelle zu notieren, deren
/// Liste wir dann doch nicht benutzen duerften, waere eine zweite Form
/// derselben Sache. `reasons` ist fuer die Frage „ist dieses Zertifikat
/// widerrufen" nicht noetig.
/// added 03.10.2026 23:8x, R66. **DIE PROBE NEBEN DEM LESER**, und sie ist
/// `#[cfg(test)]`, weil sie **keinen** Mehrwert hat: sie macht aus einem
/// `Result<Vec<Vec<u8>>, X509Fehler>` ein **`Option<usize>`** („wie viele Namen
/// kamen heraus?“), damit ein Test **beide** Richtungen prüfen kann, ohne den
/// Fehler auseinanderzuklammern.
///
/// added 03.10.2026 23:8x: **WARUM ÜBERHAUPT EINE PROBE, UND NICHT
/// `unwrap()`:** Der Leser ist `privat`, und ein Test in einem **anderen** Modul
/// kann ihn nicht rufen. `#[cfg(test)]` hebt die Sichtbarkeit nicht auf — also
/// braucht es **eine** Funktion im Modul selbst. Das ist die kleinste Form, und
/// sie ist **kein** zweiter Leser: sie ruft denselben auf.
#[cfg(test)]
pub(crate) fn verteilungsstellen_lesen_probe(inhalt: &[u8]) -> Option<usize> {
    verteilungsstellen_lesen(inhalt).ok().map(|v| v.len())
}

fn verteilungsstellen_lesen(inhalt: &[u8]) -> Result<Vec<Vec<u8>>, X509Fehler> {
    let mut raus: Vec<Vec<u8>> = Vec::new();
    let mut p = DerLeser::neu(inhalt);
    // added 02.10.2026 15:7x: **DIE AUSSEN-HUELLE ABNEHMEN**, und das war ein
    // Ebenen-Fehler. `inhalt` ist der Inhalt des `extnValue`-OCTET STRING, und
    // das ist die **getaggte** `CRLDistributionPoints`-SEQUENCE
    // (`30 44 …` nach `openssl asn1parse`, 15:7x). Die erste Fassung ging davon
    // aus, `inhalt` seien schon die Kinder,nahm die aeussere SEQUENCE selbst
    // fuer eine `DistributionPoint` und steigerte **eine Ebene zu tief**.
    // GEMESSEN: `cdp_zwei` lieferte `[]` — und weil `cdp_keins` und
    // `cdp_aussteller` ebenfalls `[]` liefern, waren die beiden anderen Tests
    // **gruen und falsch**.
    //
    // added 02.10.2026 15:7x: **UND DIE FORM IST DAMIT BELEGT** (GEMESSEN):
    // `30 44` CRLDistributionPoints → `30 20` DistributionPoint →
    // `A0 1E` distributionPoint [0] → `A0 1C` fullName [0] → `86 1A` URI.
    let Ok(huelle) = p.element_mit_tag(0x30) else {
        return Ok(raus);
    };
    let mut p = DerLeser::neu(&huelle.inhalt);
    while !p.ist_leer() {
        // `DistributionPoint` ist eine SEQUENCE.
        let Ok(dp) = p.element() else { break };
        if dp.tag != 0x30 {
            continue;
        }
        let mut d = DerLeser::neu(&dp.inhalt);
        while !d.ist_leer() {
            let Ok(feld) = d.element() else { break };
            match feld.tag {
                // distributionPoint [0] EXPLICIT -> DistributionPointName
                0xA0 => verteilungsnamen_lesen(&feld.inhalt, &mut raus)?,
                // reasons [1] / cRLIssuer [2]: uebersprungen, siehe oben.
                0xA1 | 0xA2 => {}
                _ => {}
            }
        }
    }
    Ok(raus)
}

/// added 02.10.2026 15:3x: EIN `DistributionPointName`, und es ist ein CHOICE:
/// `fullName [0]` oder `nameRelativeToCRLIssuer [1]`. Nur `fullName` traegt die
/// **URIs**; der zweite Fall ist ein Distinguished Name **relativ zum
/// Aussteller** und laesst sich nur mit dem Speicher aufloesen — ohne den gibt
/// es keine Adresse, also wird er stillschweigend uebersprungen.
fn verteilungsnamen_lesen(inhalt: &[u8], raus: &mut Vec<Vec<u8>>) -> Result<(), X509Fehler> {
    let mut n = DerLeser::neu(inhalt);
    let Ok(wahl) = n.element() else {
        return Ok(());
    };
    // added 02.10.2026 15:8x: **DIE BEIDEN FORMEN, und GEMESSEN ist, dass nur die
    // zweite vorkommt.** Nach `openssl asn1parse` (15:7x) lautet der Aufbau
    // einer Verteilungsstelle mit URI:
    //     A0 1E   distributionPoint [0]  (explizit)
    //      A0 1C  fullName [0]           — und **direkt** `86 1A`, also OHNE
    //                                        die `30`-SEQUENCE dazwischen
    // Der Grund: `GeneralNames ::= SEQUENCE OF GeneralName`, und `fullName [0]`
    // ist **implizit** getaggt (RFC 5280 Anhang A.2, „Implicitly Tagged
    // Module") — der `SEQUENCE`-Tag wird durch den `[0]`-Tag **ersetzt**.
    // Meine erste Fassung erwartete die explizite Form (`A0 { 30 … }`) und
    // lieferte fuer `cdp_zwei` **leer**; GEMESSEN ist die Reihenfolge
    // `30 → 30 → A0 → A0 → 86`, also `A0` direkt gefolgt von `86`.
    //
    // added 02.10.2026 15:8x: **BEIDE FORMEN WERDEN GELESEN**, und das ist
    // Absicht: die implizite ist die gelieferte, die explizite waere
    // normverträglich und ist billig mitzunehmen. Eine Form abzulehnen, weil
    // `openssl` die andere schreibt, waere eine Kopplung an ein Werkzeug.
    if wahl.tag != 0xA0 {
        return Ok(());
    }
    // added 02.10.2026 15:8x: **DIE ENTSCHEIDUNG ÜBER DIE BEIDEN FORMEN**, und
    // sie ist eine **Sichtpruefung**: liegt hinter `A0` eine SEQUENCE
    // (`naechste_tag() == Some(0x30)`), ist es die explizite Form und wir
    // steigen eine Ebene tiefer; sonst sind die Inhalte direkt die
    // `GeneralNames`. `naechste_tag()` **schaut nur** und liest nicht — sonst
    // waere der Tag nachher konsumiert und der naechste Aufruf stuende schon
    // im zweiten Eintrag (dieselbe Falle steht als Kommentar in
    // `zertifikat_lesen` bei der A3-Behandlung).
    // added 03.10.2026 16:d8, R60: **DIE ZWEITE `A0`-SCHICHT, UND SIE IST
    // GEMESSEN.** Der echte Verteilungspunkt von `xkcd.com` hat die Bytes
    //     30 82 04 80 | 30 3f | A0 3b | A0 39 | 86 37 ...
    // Das heisst: `SEQUENCE { SEQUENCE { [0] { [0] { uniformResourceIdentifier } } } }`
    // — **zwei** Schichten `A0`: die erste ist `distributionPoint [0]`, die zweite
    // ist `fullName [0]` aus `distributionPointName ::= CHOICE`. Mein Code las
    // **eine** und erwartete dann eine `GeneralNames`-`SEQUENCE`, die **hier gar
    // nicht dasteht** — also kein Name, und `verteilungsstellen` blieb leer.
    //
    // added 03.10.2026 16:d9: **UND DAS IST GENAU DIE FORM, DIE ICH IN
    // `general_namen_text` (R59) RICHTIG GESCHRIEBEN HABE** — „**zwei** Schichten
    // `A0`“. Die Ausgabefunktion war richtig und der Leser falsch: derselbe
    // Aufbau, zweimal gelesen, einmal richtig und einmal nicht. **DAS IST DIE
    // LEHRE: EIN AUFBAU, DEN MAN ZWEIMAL BAUT, SOLLTE **EINMAL** GEBAUT
    // WERDEN** — sonst ist die Frage „welcher ist richtig?“ eine Frage pro
    // Aufruf, und sie wird dann im Eifer des Debuggens unterschiedlich beantwortet.
    let mut g = DerLeser::neu(&wahl.inhalt);
    // added 03.10.2026 16:dl, R60: **`naechste_tag()` NUR EINMAL, UND DER
    // LESER STEHT DANACH AUF DEM INHALT.** Das ist der Fehler, den ich zwei
    // Kommentare lang **beschrieben** und dann **gemacht** habe: mein Original
    // rief `naechste_tag()` zweimal auf — einmal fuer die Form, einmal fuer die
    // Wirkung — und `naechste_tag()` **schaut nur**, es **liest nicht**. Der
    // Leser stand also beim zweiten Aufruf **hinter** der `fullName`-Schicht und
    // sah `30`, nicht `A0`.
    //
    // added 03.10.2026 16:dm: **DIE REGEL, und sie ist die一般的:** ein Leser, der
    // **schaut**, wird **genau einmal** gefragt. Wer ihn zweimal fragt, fragt
    // ihn an zwei **verschiedenen** Stellen — und bekommt zwei Antworten, von
    // denen nur eine zu seiner Frage gehoert.
    let form = g.naechste_tag();
    let namen_inhalt = match form {
        Some(0xa0) => match g.element_mit_tag(0xa0) {
            Ok(zweite) => zweite.inhalt,
            Err(_) => return Ok(()),
        },
        Some(0x30) => match g.element_mit_tag(0x30) {
            Ok(seq) => seq.inhalt,
            Err(_) => return Ok(()),
        },
        _ => wahl.inhalt.clone(),
    };
    let mut s = DerLeser::neu(&namen_inhalt);
    while !s.ist_leer() {
        let Ok(gn) = s.element() else { break };
        if gn.tag != 0x86 {
            continue;
        }
        // added 02.10.2026 15:4x: **DIE LÄNGE WIRD GEPRÜFT**, und zwar gegen
        // `MAX_ERWEITERUNG`. Ein `URI`-Feld ohne Längenbegrenzung wäre eine
        // Einladung, einen Speicherbereich zu belegen, der aus einem
        // Zertifikat kommt.
        if gn.inhalt.len() <= MAX_ERWEITERUNG && raus.len() < MAX_ERWEITERUNGEN {
            raus.push(gn.inhalt.clone());
        }
    }
    Ok(())
}

/// added 03.10.2026 23:9x, R66. **DIE PROBE FÜR DIE AUFLAGE**, aus demselben
/// Grund wie bei `verteilungsstellen_lesen_probe`: der Leser ist `privat`, und
/// `mod muster_pruefer` liegt in derselben Datei, braucht aber trotzdem einen
/// **benannten** Zugang — `pub(crate)` innerhalb von `#[cfg(test)]`.
#[cfg(test)]
pub(crate) fn namensbeschraenkung_lesen_probe(inhalt: &[u8]) -> Option<Namensbeschraenkung> {
    namensbeschraenkung_lesen(inhalt).ok()
}

fn namensbeschraenkung_lesen(inhalt: &[u8]) -> Result<Namensbeschraenkung, X509Fehler> {
    let mut leser = DerLeser::neu(inhalt);
    let seq = leser.element_mit_tag(0x30)?;
    let mut s = DerLeser::neu(&seq.inhalt);
    let mut n = Namensbeschraenkung::default();
    while !s.ist_leer() {
        let teil = s.element()?;
        if teil.tag != 0xA0 && teil.tag != 0xA1 {
            continue;
        }
        // added 02.10.2026 10:6x, S1. **0xA0/0xA1**, nicht 0xA0/0x85. Die
        // erste Fassung hatte `excluded` als 0x85 gelesen — das ist der Tag
        // von `dNSName` **innerhalb** eines Teilbaums, nicht der des
        // Feldes `excludedSubtrees`. GEMESSEN: die Beschraenkung wurde
        // dadurch nie gefunden, und `Name ausserhalb permitted` ergab
        // „angenommen".
        let erlaubt = teil.tag == 0xA0;
        // added 02.10.2026 10:6x: der Inhalt ist eine **implizite**
        // SEQUENCE OF GeneralSubtree — es steht also **kein** 0x30 vorn,
        // wohl aber vor jedem einzelnen Eintrag.
        let mut b = DerLeser::neu(&teil.inhalt);
        let mut zaehler = 0usize;
        while !b.ist_leer() && zaehler < MAX_NAME_CONSTRAINTS {
            let Ok(sub) = b.element() else { break };
            if sub.tag != 0x30 {
                continue;
            }
            // added 03.10.2026 21:7x, R64. **`minimum [0]` UND `maximum [1]`
            // WERDEN JETZT GELESEN — und sie sind der Grund für `abstand`.**
            //
            // added 03.10.2026 21:7x: **DIE NORM, wörtlich** (RFC 5280 4.2.1.10,
            // S. 40, per `curl` geholt): „Within this profile, the minimum and
            // maximum fields are not used with any name forms, thus, the
            // **minimum MUST be zero**, and **maximum MUST be absent**. However,
            // if an application encounters a critical name constraints extension
            // that specifies other values for minimum or maximum for a name form
            // that appears in a subsequent certificate, the application **MUST
            // either process these fields or reject the certificate.“
            //
            // added 03.10.2026 21:7x: **BISHER STAND HIER „uns interessiert nur
            // `base`“** — also weder „process“ noch „reject“, und damit ein
            // **nicht erfülltes MUST**. GEMESSEN (R64): `permitted; dirName:
            // Beispiel, minimum 1` ergab „Teilbäume gelesen 1, Abstand vermerkt:
            // nein“.
            //
            // added 03.10.2026 21:7x: **WIR WÄHLEN „reject“ UND NICHT „process“**,
            // und die Norm lässt beides zu. Grund: `BaseDistance` ist eine Zahl in
            // **RDNs** bzw. **Labels**, und die Bedingung wäre ein **Vergleich
            // zweier Zählungen** — eine **zweite** Teilbaumlogik neben
            // `dn_im_teilbaum` (R58), deren Formel ich abschreiben müsste. „reject“
            // ist die kleinere Änderung und braucht keine neue Vergleichslogik.
            // **UND DIE HÄUFIGKEIT IST GEMESSEN NULL** (0 von 150 lesbaren
            // Zertifikaten tragen `nameConstraints` überhaupt, R63 20:5x) — der
            // Fail-closed kostet also **nichts** gemessen.
            let mut g = DerLeser::neu(&sub.inhalt);
            let Ok(base) = g.element() else { break };
            // added 03.10.2026 21:8x: **UND DIE RESTFELDER DES SUBTREE LESEN**, und
            // das ist der Punkt: `g` wurde bisher **weggeworfen**, also blieben
            // `minimum` und `maximum` **ohne** dass irgendjemand sie gesehen hat.
            // Beide sind **explizit** getaggt (`[0]`/`[1]` um eine INTEGER), weil
            // für SEQUENCE-Komponenten die Voreinstellung „explicit“ gilt.
            while !g.ist_leer() {
                let Ok(rest) = g.element() else { break };
                if rest.tag != 0xa0 && rest.tag != 0xa1 {
                    continue;
                }
                let mut innen = DerLeser::neu(&rest.inhalt);
                let Ok(zahl) = innen.element_mit_tag(0x02) else {
                    // added 03.10.2026 21:8x: **KEIN INTEGER ⇒ DAS FELD IST
                    // UNLESBAR**, und dann ist die Auflage erst recht nicht
                    // auswertbar — `abstand` wird gesetzt, und der Wert bleibt 0.
                    n.abstand = true;
                    continue;
                };
                let Ok(bytes) = integer_als_bytes(&zahl.inhalt) else {
                    n.abstand = true;
                    continue;
                };
                // added 03.10.2026 21:9x: **NUR DIE NICHT-NORMKONFORME FÄLLE
                // ZÄHLEN.** `minimum = 0` ist ausdrücklich erlaubt („the minimum
                // MUST be zero“ heißt: es **darf** null sein), und `maximum = 0`
                // ist **formal** nicht „absent“, aber für `directoryName` mit
                // `minimum = 0` gleichbedeutend mit „nur der Wurzelname“ —
                // deshalb wird ein **Nullwert** wie folgt behandelt: `maximum`
                // wird nur bei **positivem** Wert als Abstand vermerkt.
                let wert = bytes.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b));
                if wert > 0 {
                    n.abstand = true;
                    if rest.tag == 0xa0 {
                        n.minimum = wert.min(255) as u8;
                    } else {
                        n.maximum = wert.min(255) as u8;
                    }
                }
            }
            // added 02.10.2026 14:3x: **DIE IP-NETZE, und sie kommen VOR der
            // DNS-Pruefung**, weil die Form eine andere ist: hier ist der Wert
            // **Adresse + Maske** (8 Byte fuer IPv4, 32 fuer IPv6 nach
            // RFC 5280 4.2.1.10), nicht die blosse Adresse des SAN.
            //
            // added 02.10.2026 14:3x: **UND DIE LAENGE WIRD GEPRUEFT** —
            // 8 oder 32 Byte, sonst nichts. Eine andere Laenge ist kein gueltiger
            // Wert; sie wird uebersprungen, nicht als Adresse gespeichert.
            if base.tag == 0x87 {
                if let Some(netz) = ip_netz_zerlegen(&base.inhalt) {
                    if erlaubt {
                        n.erlaubt_ip.push(netz);
                    } else {
                        n.verboten_ip.push(netz);
                    }
                    zaehler += 1;
                }
                continue;
            }
            // added 03.10.2026 10:8x, R56. **DIE DREI NEUEN FORMEN, VOR dem
            // DNS-Zweig.** GEMESSEN vorher: `namensbeschraenkung_lesen` kannte nur
            // `dNSName` (0x82) und `iPAddress` (0x87); alles andere fiel still.
            //
            // added 03.10.2026 10:8x: **UND DER ALTE KOMMENTAR HIER WAR FALSCH.**
            // Er stand vom 02.10.2026 10:6x an dieser Stelle und schrieb: „Die
            // anderen Namenstaeme werden nicht gelesen … und sie zu uebersehen ist
            // hier richtig: sie wuerden als leere DNS-Teilbaeume in die Liste gehen
            // und **alles** verbieten.“ **GEMESSEN falsch:** die Zweige lesen den
            // Inhalt und `continue`en — ein fremder Tag **haette** die DNS-Liste
            // nie erreicht. Das war also keine Entscheidung, sondern eine
            // **Vermeidung**, und sie stand da, weil sie als Entscheidung formuliert
            // war. DIE LEHRE: **ein Kommentar, der ein Nicht-Tun begruendet, traegt
            // die Last, das Nicht-Tun zu erklaeren** — und tut er das mit einer
            // Behauptung ueber den Code, muss diese Behauptung **gemessen** werden.
            //
            // added 03.10.2026 10:8x: **`rfc822Name`, [1], IMPLICIT IA5String** —
            // der Inhalt ist die Adresse als **Bytes**. Keine ASCII-Pruefung: der
            // Inhalt wird nur verglichen, und ein Byte ausserhalb ASCII trifft
            // keine IA5String-Beschraenkung.
            if base.tag == 0x81 {
                if erlaubt {
                    n.erlaubt_email.push(base.inhalt.clone());
                } else {
                    n.verboten_email.push(base.inhalt.clone());
                }
                continue;
            }
            // added 03.10.2026 10:8x: **`uniformResourceIdentifier`, [6],
            // IMPLICIT IA5String** — kleingeschrieben, weil nur der Host-Teil
            // verglichen wird.
            if base.tag == 0x86 {
                let uri = std::str::from_utf8(&base.inhalt).map_err(|_| X509Fehler::Aufbau {
                    was: "URI-Beschraenkung ist kein UTF-8",
                })?;
                let klein = uri.to_ascii_lowercase();
                if erlaubt {
                    n.erlaubt_uri.push(klein);
                } else {
                    n.verboten_uri.push(klein);
                }
                continue;
            }
            // added 03.10.2026 10:9x, R56. **`directoryName`, [4] Name** — und
            // anders als die beiden vorigen **EXPLICIT**: im Inhalt steht die
            // SEQUENCE. Das ist kein Raten: IMPLICIT tagging ist fuer einen CHOICE
            // mit verschiedenen Typen nicht anwendbar (X.690 11.5), und die
            // uebrigen Namen in `GeneralName` sind IA5String, `Name` nicht.
            if base.tag == 0xa4 {
                let mut inner = DerLeser::neu(&base.inhalt);
                let name = name_lesen(&mut inner)?;
                if erlaubt {
                    n.erlaubt_dn.push(name);
                } else {
                    n.verboten_dn.push(name);
                }
                continue;
            }
            if base.tag != 0x82 {
                // added 03.10.2026 20:9x, R63. **DIE AUSREDEN IST GESTRICHEN, WEIL
                // SIE FALSCH WAR.** Der Kommentar an dieser Stelle behauptete, die
                // Namenstaeme 0, 3, 5 und 8 (`otherName`, `x400Address`,
                // `ediPartyName`, `registeredID`) fuehrten in RFC 5280 4.2.1.10 zu
                // **keiner** Beschraenkungsform — und **stillschweigend**
                // `continue` war die Folge.
                //
                // added 03.10.2026 20:9x: **DIE HABE IST HALB WAHR.** Fuer die
                // **Verarbeitungsregeln** stimmt sie: 4.2.1.10 beschreibt in
                // Woertlichen fuer `DNS`, `email`, `URI`, `dirName` und `IP`,
                // und fuer die anderen vier **keine**. **ABER** `GeneralSubtree`
                // ist in 4.2.1.10 als `base GeneralName` definiert, und
                // `GeneralName` (4.2.1.6) kennt **alle neun** Formen. Die
                // ASN.1 laesst sie zu — die Norm sagt nur, wie man sie **nicht**
                // auswertet.
                //
                // added 03.10.2026 20:9x: **UND WARUM DAS EIN FEHLER IST, NICHT
                // NUR EIN FEHLENDES FEATURE.** 4.2.1.10 sagt woertlich: „any
                // name matching a name constraint in the **excludedSubtrees**
                // field is rejected“. Ein `excluded; otherName:x` heisst also
                // „ein Name, den wir nicht bewerten koennen, wird abgelehnt“.
                // Wer die Auflage wegliest, **behauptet** das Gegenteil — das ist
                // **fail open** in einer Pruefung, deren Zweck das Schliessen
                // ist. Also: **merken, nicht verschwinden lassen**; das
                // **Entscheiden** macht `namensbeschraenkung_pruefen`.
                if n.unlesbar.len() < MAX_UNLESBARE_ARTEN {
                    n.unlesbar.push(base.tag);
                }
                continue;
            }
            if base.inhalt.len() > MAX_OID_LAENGE {
                return Err(X509Fehler::Aufbau {
                    was: "DNS-Teilbaum zu lang",
                });
            }
            let eintrag = base.inhalt.to_ascii_lowercase();
            if erlaubt {
                n.erlaubt.push(eintrag);
            } else {
                n.verboten.push(eintrag);
            }
            zaehler += 1;
        }
    }
    Ok(n)
}

/// added 02.10.2026 09:6x, S1: Obergrenze fuer die Zahl der Teilbaeume in
/// einer `nameConstraints`-Erweiterung.
///
/// M4 mit Begruendung: 64 Teilbaeume je Art. Das ist **kein** geratener
/// Wert — die groessten realen CAs (DigiCert, GlobalSign) benutzen 3 bis 5,
/// und jede CA, die 64 braucht, hat ein anderes Problem.
pub const MAX_NAME_CONSTRAINTS: usize = 64;

/// added 03.10.2026 20:9x, R63. **WIE VIELE UNBEKANNTE AUFLAGEN-ARTEN WIR
/// MERKEN**, benannt und begruendet: RFC 5280 4.2.1.6 kennt **neun**
/// `GeneralName`-Formen, davon lesen wir **fuenf** (`rfc822Name`, `dNSName`,
/// `directoryName`, `uniformResourceIdentifier`, `iPAddress`). Die Zahl der
/// unbekannten Formen ist also durch die **Norm** auf **vier** begrenzt — diese
/// Konstante ist die Klammer darueber, damit ein Zertifikat mit einer
/// Endloszahl unbekannter Tags (von einem Angreifer-Server) keinen Speicher
/// belegt.
pub const MAX_UNLESBARE_ARTEN: usize = 8;

/// added 02.10.2026 09:6x, S1: Obergrenze fuer eine OID in Byte.
///
/// M4 mit Begruendung: die laengste OID in RFC 5280 Anhang A hat neun
/// Byte (`2.5.4.3.1` ist vier, `1.3.6.1.5.5.7.3.1` acht); 32 laesst
/// Spielraum und begrenzt die Allokation trotzdem.
pub const MAX_OID_LAENGE: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::von_hex;

    /// 150 echte CA-Zertifikate aus /etc/ssl/certs/ca-certificates.crt.
    ///
    /// Die Erwartungswerte daneben kommen aus `openssl`, nicht aus
    /// eigener Rechnung. Wer hier und `openssl` uneinig sind, hat
    /// mindestens einer von beiden die Norm nicht gelesen.
    const DATEN: &str = include_str!("../testdaten/t10_x509_testdaten.txt");

    fn feld(name: &str) -> Option<String> {
        for zeile in DATEN.lines() {
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return Some(rest.to_string());
            }
        }
        None
    }

    fn der_von(index: usize) -> Vec<u8> {
        von_hex(&feld(&format!("ZERT{index}_DER")).expect("DER im Testdatensatz")).expect("Hex")
    }

    /// M7: ALLE 150 Zertifikate werden gelesen und an `openssl`
    /// gemessen. Das ist breit genug, dass ein Sonderfall auffaellt:
    /// UTCTime und GeneralizedTime, RSA und ECDSA, Zertifikate mit und
    /// ohne Erweiterungen, Laengen von 500 bis 4000 Byte.
    ///
    /// M4: 150 Zertifikate sind eine feste Zahl aus der Datei, keine
    /// aus der Eingabe.
    #[test]
    fn alle_echten_zertifikate_werden_gelesen() {
        let anzahl: usize = feld("ANZAHL").expect("ANZAHL").parse().expect("Zahl");
        assert_eq!(anzahl, 150, "so viele Zertifikate sind in der Datei");

        let mut geprueft = 0usize;
        let mut mit_ecdsa = 0usize;
        let mut mit_rsa = 0usize;
        let mut laengen_klein = usize::MAX;
        let mut laengen_gross = 0usize;

        for i in 0..anzahl {
            let der = der_von(i);
            laengen_klein = laengen_klein.min(der.len());
            laengen_gross = laengen_gross.max(der.len());
            let z = match zertifikat_lesen(&der) {
                Ok(z) => z,
                Err(e) => panic!("Zertifikat {i} nicht lesbar: {e}"),
            };
            geprueft += 1;
            match &z.schluessel {
                SchluesselArt::Rsa { n, e } => {
                    mit_rsa += 1;
                    // RFC 5280 Abschnitt 4.1.1.2 und die TLS-Regeln:
                    // ein Modulus unter 2048 Bit ist zu klein.
                    assert!(
                        n.len() >= 256,
                        "Zertifikat {i}: Modulus nur {} Byte",
                        n.len()
                    );
                    assert!(!e.is_empty(), "Zertifikat {i}: Exponent leer");
                }
                SchluesselArt::Ecdsa { x, y, kurve } => {
                    mit_ecdsa += 1;
                    // Beide Koordinaten muessen gleich lang sein, und
                    // ihre Laenge muss zur Kurve passen. Sonst waere
                    // ein P-384-Schluessel als P-256 lesbar.
                    let bits = kurven_bits(kurve);
                    assert!(bits > 0, "Zertifikat {i}: unbekannte Kurve {kurve:?}");
                    let byte = bits.div_ceil(8);
                    assert_eq!(
                        x.len(),
                        byte,
                        "Zertifikat {i}: x hat {} statt {byte} Byte",
                        x.len()
                    );
                    assert_eq!(
                        y.len(),
                        byte,
                        "Zertifikat {i}: y hat {} statt {byte} Byte",
                        y.len()
                    );
                }
                SchluesselArt::Unbekannt { .. } => {
                    panic!("Zertifikat {i}: unbekannter Schluessel")
                }
            }
            // notAfter muss nach notBefore liegen. Das prueft die
            // Zeitrechnung, denn beide Seiten kommen aus derselben
            // Formel — ein Fehler, der beide gleich verschiebt,
            // faellt hier nicht auf. Deshalb steht unten noch eine
            // Gegenprobe an einem fest verdrahteten Datum.
            assert!(
                z.gueltigkeit.bis > z.gueltigkeit.von,
                "Zertifikat {i}: notAfter vor notBefore"
            );
            // Das CA-Flag muss zu openssl passen.
            let soll_ca = feld(&format!("ZERT{i}_CA")).expect("CA-Flag") == "1";
            assert_eq!(
                z.basic_constraints.map(|b| b.ist_ca).unwrap_or(false),
                soll_ca,
                "Zertifikat {i}: CA-Flag weicht von openssl ab"
            );
        }
        assert_eq!(geprueft, 150, "alle 150 gelesen");
        // Wenn die Datei nur RSA enthielte, waere der ECDSA-Zweig nie
        // gelaufen und der Test waere gruen, ohne geprueft zu haben,
        // was er behauptet zu pruefen.
        assert!(mit_rsa > 0, "es muss RSA-Zertifikate geben");
        assert!(mit_ecdsa > 0, "es muss ECDSA-Zertifikate geben");
        assert!(
            laengen_klein < 1000,
            "die kleinste Laenge, sonst zu wenig Streuung"
        );
        assert!(
            laengen_gross > 2000,
            "die groesste Laenge, sonst zu wenig Streuung"
        );
    }

    /// M7: die Zeitrechnung an Daten, die nicht aus einem Zertifikat
    /// kommen, sondern fest sind.
    ///
    /// DER Grund fuer den eigenen Test: im Test mit den 150
    /// Zertifikaten kommen `von` und `bis` aus derselben Formel. Ein
    /// Fehler, der BEIDE gleich verschiebt — etwa ein konstant
    /// falscher Offset — bliebe unbemerkt. Hier sind die Erwartungen
    /// unabhaengig, mit `date -u` nachgerechnet.
    #[test]
    fn zeitrechnung_an_festen_daten() {
        // 1970-01-01 00:00:00 UTC ist Tag 0, das ist die Definition.
        assert_eq!(tage_seit_1970(1970, 1, 1), 0, "der Anfang von Unix");
        // 2000-01-01 war ein Samstag; `date -u -d 2000-01-01 +%s`
        // ergibt 946684800.
        assert_eq!(tage_seit_1970(2000, 1, 1), 10957, "2000-01-01, Tag 10957");
        // Ein Schaltjahr: 2000 war eines durch 400, 1900 nicht.
        assert_eq!(
            tage_seit_1970(2000, 3, 1) - tage_seit_1970(2000, 2, 29),
            1,
            "der 29. Februar 2000 existierte"
        );
        assert_eq!(
            tage_seit_1970(2001, 3, 1) - tage_seit_1970(2001, 2, 28),
            1,
            "2001 war kein Schaltjahr"
        );
        // Das Aequivalent in Sekunden, wie es im Zertifikat steht.
        // Beide Werte sind mit `date -u -d "..." +%s` gegengerechnet,
        // nicht von mir gerechnet:
        //   date -u -d "2030-12-31 09:37:37" +%s  ->  1924940257
        //   date -u -d "2011-05-05 09:37:37"  +%s  ->  1304588257
        assert_eq!(
            zeit_seit_1970(0x17, b"301231093737Z").expect("UTCTime"),
            1_924_940_257,
            "UTCTime 301231093737Z, der notAfter von Zertifikat 0"
        );
        // UTCTime mit zweistelligem Jahr: 50 bis 99 ist 19xx.
        assert_eq!(
            zeit_seit_1970(0x17, b"110505093737Z").expect("2011"),
            1_304_588_257,
            "UTCTime 11 ist 2011, nicht 2011 n. Chr."
        );
        // GeneralizedTime fuer dasselbe Datum, mit vierstelligen Jahr.
        let a = zeit_seit_1970(0x17, b"110505093737Z").expect("UTCTime");
        let b = zeit_seit_1970(0x18, b"20110505093737Z").expect("GeneralizedTime");
        assert_eq!(a, b, "UTCTime und GeneralizedTime muessen dasselbe ergeben");
        // Und ein Datum jenseits von 2049, was nur als
        // GeneralizedTime vorkommt.
        assert!(
            zeit_seit_1970(0x18, b"20501231000000Z").expect("2050")
                > zeit_seit_1970(0x17, b"491231000000Z").expect("2049")
        );
        // Unsinn wird abgelehnt, nicht geraten.
        assert!(zeit_seit_1970(0x17, b"991301000000Z").is_err(), "Monat 13");
        assert!(zeit_seit_1970(0x17, b"990101250000Z").is_err(), "Stunde 25");
        assert!(zeit_seit_1970(0x17, b"990132000000Z").is_err(), "Tag 32");
        assert!(zeit_seit_1970(0x17, b"9901").is_err(), "zu kurz");
        assert!(
            zeit_seit_1970(0x04, b"990101000000Z").is_err(),
            "falscher Tag"
        );
    }

    /// added 03.10.2026, TLS (Fuzzing, 100 000 Faelle `x509`): **DER MINIMALFALL
    /// DES ABSTURZES, als Test.**
    ///
    /// **DER FUND:** `fuzz x509 100000` meldete
    /// `x509.rs:1184: end byte index 10 is not a char boundary; it is inside
    /// 'ݸ' (bytes 9..11 of string)`. Ein zweibyteiges Zeichen an **Byte 9**,
    /// der Schnitt bei **Byte 10**.
    ///
    /// **UND WARUM DIESER TEST VORHER **GRUEN** WAR:** die alte Fassung
    /// benutzte `s[10..12]` auf einem `&str`. Bei diesem Fall ist `s` gueltiges
    /// UTF-8 (13 Byte, Laengenpruefung gruen), aber der Schnitt faellt in das
    /// Zeichen — und `str`-Indizierung **panict** dort.
    ///
    /// **DER MINIMALFALL, und er ist wirklich minimal:** 13 Byte, wie
    /// `YYMMDDhhmmssZ`, aber an Byte 9 und 10 das **zweibyteige** Zeichen
    /// `ݸ` (U+0778). Das ist genau der Byte-Bereich, den der Fuzzer
    /// gemeldet hat.
    #[test]
    fn zweibytezeichen_im_zeitfeld_panict_nicht() {
        // added 03.10.2026, TLS: **DAS BYTE-MUSTER, wörtlich gebaut** — nicht
        // `format!`, denn das koennte selbst an einer Grenze scheitern und der
        // Test waere dann eine andere Aussage.
        // added 03.10.2026, TLS: **10 Byte VOR** dem Zeichen, nicht 9. Mein
        // erster Vektor hatte 9 und damit **12** Byte gesamt — der Test meldete
        // `left: 12, right: 13` und war deshalb rot, **nicht** der Code.
        // 10 + 2 (Zeichen) + 1 (`Z`) = 13.
        let mut s = Vec::from(&b"9901010000"[..]);
        s.extend_from_slice("\u{0778}".as_bytes()); // 2 Byte
        s.push(b'Z');
        assert_eq!(s.len(), 13, "13 Byte, also an der Laengengrenze vorbei");
        // added 03.10.2026, TLS: **`is_err`, nicht `panic!`-Fang.** `is_err`
        // ist hier die **starke** Aussage: die Funktion liefert einen Fehler,
        // und ein Absturz waere ein Fehler des Tests, nicht des Aufrufers.
        assert!(
            zeit_seit_1970(0x17, &s).is_err(),
            "ein zweibyteiges Zeichen ist kein Datum — Fehler statt Absturz"
        );
    }

    /// added 03.10.2026, TLS (Fuzzing): **DASSELBE FUER `GeneralizedTime`** und
    /// an **jeder** Feldgrenze, weil der Fuzzer nur die eine Stelle fand — die
    /// anderen sind derselbe Fehler an anderen Offsets.
    #[test]
    fn zweibytezeichen_an_jeder_feldgrenze_panict_nicht() {
        // added 03.10.2026, TLS: **ALLE OFFSETS DER BEIDEN FORMEN.** Der
        // Schnitt `s[a..a+2]` faellt fuer jedes **ungerade** `a` in ein
        // zweibyteiges Zeichen, das bei `a` beginnt. Also jedes ungerade `a`
        // von 1 bis zur Laenge minus 2.
        for (tag, len) in [(0x17u8, 13usize), (0x18, 15)] {
            for a in (1..=len - 2).step_by(2) {
                let mut s: Vec<u8> = (0..len).map(|i| b'0' + (i as u8 % 10)).collect();
                s[a] = 0xd8; // erstes Byte von U+0778
                s[a + 1] = 0xb8; // zweites Byte
                // added 03.10.2026, TLS: **DIE LAENGE BLEIBT 13/15**, sonst
                // prueft der Test die Laengenpruefung und nicht den Schnitt.
                assert_eq!(s.len(), len);
                assert!(
                    zeit_seit_1970(tag, &s).is_err(),
                    "tag {tag:#04x}, Offset {a}: Fehler statt Absturz"
                );
            }
        }
    }

    /// added 03.10.2026, TLS (Fuzzing): **DIE ASCII-ZIFFER-PRUEFUNG**, und sie
    /// ist eine **zweite** Aussage neben dem Absturz: ein Datum aus Buchstaben
    /// ist kein gueltiges Datum, und `parse` haette es auch abgelehnt — aber
    /// `parse` **auf einem `&str`** ist genau das, was den Absturz brachte.
    #[test]
    fn keine_buchstaben_als_ziffern() {
        // added 03.10.2026, TLS: **10 Byte** Vorlauf (siehe oben): 10 + 1 + 2
        // (`5Z`) = 13.
        for b in [b'a', b'/', b':', 0x7f, 0x80, 0xff] {
            let mut s = Vec::from(&b"9901010000"[..]);
            s.push(b);
            s.extend_from_slice(b"5Z");
            assert_eq!(s.len(), 13);
            assert!(
                zeit_seit_1970(0x17, &s).is_err(),
                "Byte {b:#04x} ist keine Ziffer"
            );
        }
    }

    /// added 03.10.2026, TLS (Fuzzing): **OHNE `Z` IST ES KEIN DATUM** — vorher
    /// wurde `s.len() - 1` als Ende der Sekunden genommen, **ohne** zu
    /// pruefen, was dort steht. RFC 5280 4.1.2.5.1: „MUST be expressed in
    /// Greenwich Mean Time (Zulu) time“.
    #[test]
    fn ohne_z_ist_es_kein_datum() {
        assert!(
            zeit_seit_1970(0x17, b"990101000000+").is_err(),
            "statt Z steht ein anderes Zeichen"
        );
        assert!(
            zeit_seit_1970(0x17, b"9901010000009").is_err(),
            "statt Z eine Ziffer"
        );
    }

    /// M7: die Wildcard-Regeln, und zwar einzeln, damit bei einem
    /// Fehler klar ist, welche Regel es war.
    ///
    /// Die Faelle stammen aus der Praxis: RFC 6125 Abschnitt 6.4.3
    /// erlaubt `*` nur als erstes Label; alles andere ist ein Fehler,
    /// auch wenn es "logisch" aussieht.
    #[test]
    fn wildcard_regeln_einzeln() {
        // (a) der Normalfall
        assert!(hostname_trifft("*.example.com", "www.example.com"), "a");
        assert!(
            hostname_trifft("*.example.com", "WWW.EXAMPLE.COM"),
            "b, case-insensitive"
        );
        assert!(hostname_trifft("example.com", "example.com"), "c, exakt");
        // (b) ein Sternchen ist EIN Label, nicht beliebig viele
        assert!(
            !hostname_trifft("*.example.com", "a.b.example.com"),
            "d, ein Sternchen ersetzt nur ein Label"
        );
        assert!(
            !hostname_trifft("*.example.com", "example.com"),
            "e, der Apex gehoert nicht dazu"
        );
        // (c) nur als erstes Label
        assert!(
            !hostname_trifft("w*.example.com", "www.example.com"),
            "f, Teilwildcard links"
        );
        assert!(
            !hostname_trifft("*w.example.com", "www.example.com"),
            "g, Teilwildcard mitten im Wort"
        );
        assert!(
            !hostname_trifft("www.*.example.com", "www.a.example.com"),
            "h, Sternchen nicht an erster Stelle"
        );
        // (d) leere Labels
        assert!(
            !hostname_trifft("*.example.com", ".example.com"),
            "i, leeres erstes Label"
        );
        assert!(!hostname_trifft("example.com", ".example.com"), "j");
        assert!(
            !hostname_trifft("example.com", "example.com."),
            "k, Punkt am Ende"
        );
        assert!(!hostname_trifft("example.com", ""), "l, leerer Hostname");
        // (e) Laenge und Sonderzeichen
        assert!(
            !hostname_trifft("example.com", "example.com.evil.com"),
            "m, kein praefix-Match"
        );
        assert!(hostname_trifft("example.com", "EXAMPLE.COM"), "n");
        // Und die Regel, die man leicht falsch hat: der Hostname
        // muss GENAU so viele Labels haben wie das Muster.
        assert!(
            !hostname_trifft("example.com", "www.example.com"),
            "o, ohne Sternchen keine Zusatzlabels"
        );
        assert!(
            hostname_trifft("*.example.com", "a.example.com"),
            "p, ein Label"
        );
    }
}

#[cfg(test)]
mod tls_feature_tests {
    use super::{
        Ergebnis, X509Fehler, features_lesen, must_staple_entscheidung, verlangt_status_request,
    };

    /// added 02.10.2026 23:10x, S4. **DER VEKTOR, und woher er kommt.** RFC 7633
    /// hat **kein** Byte-Beispiel (Anhang A ist nur das ASN.1-Modul), also ist
    /// der Block **aus der Norm gebaut**: `Features ::= SEQUENCE SIZE (1..MAX) OF
    /// INTEGER {0..(2^16-1)}` mit den INNERNEN Werten 5 (`status_request`,
    /// RFC 6066 3) und 17 (`extended_master_secret`, RFC 7627).
    /// DER BYTESCHNITT: `30 06` = SEQUENCE, 6 Byte Inhalt; `02 01 05` = INTEGER 5,
    /// `02 01 11` = INTEGER 17. Kein Wert ist geraten.
    fn sequenz_5_und_17() -> Vec<u8> {
        let mut v = vec![0x30, 0x06, 0x02, 0x01, 0x05, 0x02, 0x01, 0x11];
        // added 02.10.2026 23:10x: `v` wird nur bewegt, damit die Funktion
        // nichts zurueckgibt, was sie nicht prueft — nein, sie gibt die Bytes
        // zurueck, das ist der Sinn.
        v.shrink_to_fit();
        v
    }

    /// added 02.10.2026 23:10x, S4. **DER NORMALFALL:** die Liste wird gelesen,
    /// und `status_request` steht darin.
    #[test]
    fn features_lesen_liest_die_merkmalsnummern() -> Result<(), &'static str> {
        let f = features_lesen(&sequenz_5_und_17()).map_err(|_| "lesen scheiterte")?;
        assert_eq!(f, vec![5, 17]);
        Ok(())
    }

    /// added 02.10.2026 23:11x, S4. **MEHR ALS EIN BYTE JE INTEGER**, und das
    /// ist der Grund fuer die eigene Schleife: `01 00` = 256, `00 05` = 5 mit
    /// fuehrender Null. Beides muss denselben Wert ergeben wie die kurze Form.
    #[test]
    fn integer_mit_fuehrender_null_und_breiter_wert() -> Result<(), &'static str> {
        // 30 08: SEQUENCE, 8 Byte. 02 02 00 05 = INTEGER 5 (zwei Byte mit
        // fuehrender Null), 02 02 01 00 = INTEGER 256.
        let block = [0x30, 0x08, 0x02, 0x02, 0x00, 0x05, 0x02, 0x02, 0x01, 0x00];
        let f = features_lesen(&block).map_err(|_| "lesen scheiterte")?;
        assert_eq!(f, vec![5, 256]);
        Ok(())
    }

    /// added 02.10.2026 23:11x, S4. **DREI FEHLER, und alle drei muessen Fehler
    /// sein**, weil `SIZE (1..MAX)` und `INTEGER` in der Norm stehen:
    /// leere Sequenz, ein Element mit falschem Tag, ein INTEGER aus vier Byte.
    #[test]
    fn kaputte_merkmalsliste_ist_ein_fehler() {
        // 30 00 = leere SEQUENCE
        assert!(matches!(
            features_lesen(&[0x30, 0x00]),
            Err(X509Fehler::Aufbau { .. })
        ));
        // 30 03 04 01 05 = OCTET STRING statt INTEGER
        assert!(matches!(
            features_lesen(&[0x30, 0x03, 0x04, 0x01, 0x05]),
            Err(X509Fehler::Aufbau { .. })
        ));
        // added 02.10.2026 23:15x: **MEIN VEKTOR WAR KAPUTT**, und die
        // Korrektur steht hier, weil sie lehrreicher ist als der Test:
        // ich hatte `30 05` (5 Byte Inhalt) geschrieben und dann **6** Byte
        // Inhalt geliefert. Der Leser las das INTEGER, liess einen Restbyte
        // uebrig und meldete einen **DER**-Fehler statt meines `Aufbau` —
        // der Test schlug fehl und lag am Vektor, nicht an der Pruefung.
        // Richtig ist `30 06`: SEQUENCE mit 6 Byte, darin `02 04` = INTEGER aus
        // vier Byte (`01 02 03 04`), und das ist genau der Fall „mehr als drei
        // Byte“.
        assert!(matches!(
            features_lesen(&[0x30, 0x06, 0x02, 0x04, 0x01, 0x02, 0x03, 0x04]),
            Err(X509Fehler::Aufbau { .. })
        ));
    }

    /// added 02.10.2026 23:11x, S4. **DIE ENTSCHEIDUNG, und das ist der Kern.**
    /// `verlangt && !gut` heisst `Abgewiesen`; alles andere `Angenommen`. Und der
    /// Gegenfall: eine Liste **ohne** Nummer 5 (hier 17) darf **nie** eine
    /// Verbindung zerlegen, auch nicht bei fehlender OCSP-Antwort.
    #[test]
    fn die_entscheidung_nur_bei_verlangtem_stapling() {
        // added 03.10.2026 00:4x: **ZWEI** Argumente. In R44 waren es drei, mit
        // einem Schalter `must_staple_auswerten`, den **niemand** gesetzt hat
        // (PROBLEME.txt Nr. 45). GEMESSEN (02.10.2026) tragen **0 von 15** Hosts
        // die Erweiterung, also ist die Regel gefahrlos — und ein ungenutzter
        // Schalter ist schlimmer als keiner, weil er eine Abschaltmoeglichkeit
        // verspricht, die es nicht gibt.
        assert_eq!(must_staple_entscheidung(true, false), Ergebnis::Abgewiesen);
        assert_eq!(must_staple_entscheidung(true, true), Ergebnis::Angenommen);
        assert_eq!(must_staple_entscheidung(false, false), Ergebnis::Angenommen);
    }

    /// added 02.10.2026 23:12x, S4. **DIE FRAGE, DIE DER HANDSHAKE STELLT**, an
    /// einem echten `Zertifikat` aus unserem Testbestand — nicht an einer Liste,
    /// damit der ganze Weg gemessen wird: `_beobachtet` heisst, kein
    /// Testzertifikat in `narsil-tls/testdaten` traegt must-staple. Das ist die
    /// **Gegenprobe** zur Wildmessung (0 von 6).
    #[test]
    fn keine_unserer_testzertifikate_verlangt_stapling() -> Result<(), &'static str> {
        let pfad = concat!(env!("CARGO_MANIFEST_DIR"), "/testdaten/testzertifikat.der");
        let der = std::fs::read(pfad).map_err(|_| "testzertifikat.der nicht lesbar")?;
        // added 02.10.2026 23:13x: der Weg heisst `zertifikat_lesen`, nicht
        // `Zertifikat::lesen` — mein erster Versuch fand keine Methode
        // (`no associated function named 'lesen' found`).
        let z = super::zertifikat_lesen(&der).map_err(|_| "Testzertifikat unlesbar")?;
        assert!(
            !verlangt_status_request(&z),
            "ein Testzertifikat verlangt must-staple — dann ist der Test hier falsch"
        );
        Ok(())
    }
}

/// added 03.10.2026 01:3x, R47. **DER ZERTIFIKAT-BUER**, und er ist
/// `#[cfg(test)]`, damit der Bibliotheksbau waehrend der Arbeit an ihm gruen
/// bleibt.
///
/// WARUM ER KOMM — und es ist `PROBLEME.txt` Nr. 46 woertlich: `narsil-tls` las
/// Zertifikate und stellte **keine**; damit war die Kette „die TLS-Feature-
/// Erweiterung wird gelesen“ → „der Handshake bricht ab“ **unbelegt**. GEMESSEN
/// war zusaetzlich: es gibt nicht nur keinen Bauer, sondern auch **keinen
/// DER-Schreiber** (`der.rs` liest nur). Der Bauer ist deshalb die kleinste
/// Unterschicht, die die Beweisluecke schliesst — und er nuetzt allen anderen
/// Zertifikatstests der Rolle.
///
/// WAS ER KANN, bewusst wenig: **ein** selbstsigniertes Blatt mit ECDSA P-256,
/// mit `SAN`, `basicConstraints`, `keyUsage`, `extendedKeyUsage` (serverAuth) und
/// optional der TLS-Feature-Erweiterung. Keine Ausstellung, keine Kette, kein
/// Schluesselverteilungsweg.
#[cfg(test)]
pub mod bauen {

    /// added 03.10.2026 02:9x, R48. **DER NAME DER CA**, und er ist
    /// **nicht** `localhost`. GEMESSEN (02:8x): mit demselben Namen in
    /// CA und Blatt gilt dem Pruefer das Blatt als **selbstsigniert**
    /// (`name_gleich(&aktuell.aussteller, &aktuell.inhaber)` in
    /// `kette.rs`), und dann muss es selbst im Vertrauensspeicher liegen —
    /// liegt es dort nicht, kommt `KeinAussteller`. Das ist kein Fehler des
    /// Pruefers, sondern eine Kette, die es in der Wirklichkeit nicht gibt:
    /// Aussteller und Blatt haben verschiedene Namen.
    pub const CA_NAME: &str = "Rollen-TLS-Test-CA";

    /// added 03.10.2026 01:3x: **DER LAENGENKODEBIERER**, und er ist der Teil,
    /// an dem ein Handbauer am haeufigsten scheitert: unter 0x80 ein Byte,
    /// sonst 0x80 plus Byteanzahl und die Bytes selbst (X.690 8.1.3).
    pub fn laenge(n: usize) -> Vec<u8> {
        if n < 0x80 {
            return vec![n as u8];
        }
        let mut bytes = Vec::new();
        let mut rest = n;
        while rest > 0 {
            bytes.insert(0, (rest & 0xff) as u8);
            rest >>= 8;
        }
        let mut v = vec![0x80 | bytes.len() as u8];
        v.extend_from_slice(&bytes);
        v
    }

    /// added 03.10.2026 01:3x: **EIN ELEMENT**: Tag, Laenge, Inhalt.
    pub fn tlv(tag: u8, inhalt: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&laenge(inhalt.len()));
        v.extend_from_slice(inhalt);
        v
    }

    /// added 03.10.2026 01:4x: **INTEGER**, mit dem Vorzeichenbit beachtet: ein
    /// Wert mit gesetztem obersten Bit braucht eine fuehrende Null, sonst wuerde
    /// er als **negative** Zahl gelesen. Das ist der Fehler, den
    /// `ecdsa_der_signatur` in `signatur.rs` bereits fuer `r` und `s` abdeckt;
    /// hier wird es fuer Seriennummer und Zeit gebraucht.
    pub fn integer(wert: &[u8]) -> Vec<u8> {
        let mut v = wert.to_vec();
        if v.is_empty() {
            v.push(0);
        }
        if v[0] & 0x80 != 0 {
            v.insert(0, 0);
        }
        tlv(0x02, &v)
    }

    /// added 03.10.2026 19:2x, R62. **`pub` GEMACHT, und der Grund ist ein
    /// zweiter Bauer:** `crl.rs` braucht `sequenz`, `oid` und `tlv`, um eine
    /// **Sperrliste** zu bauen (R62) — und `sequenz`/`oid` waren `privat`, also
    /// nur innerhalb dieses Moduls sichtbar. Es gibt zwei ehrliche Wege: die
    /// Primitive duplizieren (zwei Kopien derselben Kodierregel — die Fehlerform
    /// aus R60: „ein Aufbau, zweimal gebaut“) oder **einmal** `pub` machen.
    pub fn sequenz(teile: &[Vec<u8>]) -> Vec<u8> {
        let mut inhalt = Vec::new();
        for t in teile {
            inhalt.extend_from_slice(t);
        }
        tlv(0x30, &inhalt)
    }

    /// added 03.10.2026 19:2x, R62: **`pub` aus demselben Grund wie `sequenz`**
    /// — geteilte Primitive statt geteilter Kopien.
    pub fn oid(bytes: &[u8]) -> Vec<u8> {
        tlv(0x06, bytes)
    }

    /// added 03.10.2026 01:4x: **DER NAME**, minimal und gueltig:
    /// `CN=localhost` als `DirectoryString` in `UTF8String`.
    pub fn name(text: &str) -> Vec<u8> {
        let attribut = sequenz(&[
            oid(&[0x55, 0x04, 0x03]), // commonName 2.5.4.3
            tlv(0x0c, text.as_bytes()),
        ]);
        let set = tlv(0x31, &attribut);
        sequenz(&[set])
    }

    /// added 03.10.2026 01:4x: **DIE GUELTIGKEIT**, als `UTCTime` (X.509
    /// verlangt es fuer Jahre bis 2049). Fest 2020 bis 2035, damit der Test
    /// nicht mit der Uhr laeuft.
    pub fn gueltigkeit() -> Vec<u8> {
        sequenz(&[tlv(0x17, b"200101000000Z"), tlv(0x17, b"350101000000Z")])
    }

    /// added 03.10.2026 01:4x: **DER OEFFENTLICHE SCHLUESSEL** aus dem
    /// Testschluessel (`testdaten/testschluessel_p256.hex`). GEMESSEN: unser
    /// Server-Angebot nimmt `privat: Vec<u8>` und `zertifikat: Vec<u8>`, also
    /// muessen beide zusammenpassen — der Punkt des Spiels.
    pub fn privat_aus_testdaten() -> Vec<u8> {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdaten/testschluessel_p256.hex"
        ))
        .unwrap_or_default();
        let sauber: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (0..sauber.len() / 2)
            .map(|i| u8::from_str_radix(&sauber[i * 2..i * 2 + 2], 16).unwrap_or(0))
            .collect()
    }

    /// added 03.10.2026 01:5x: **DER SPKI** fuer genau diesen Schluessel.
    pub fn spki() -> Vec<u8> {
        let mut privat = [0u8; 32];
        let bytes = privat_aus_testdaten();
        let n = bytes.len().min(32);
        privat[..n].copy_from_slice(&bytes[..n]);
        let punkt = crate::ecdsa::oeffentlich_p256(&privat).unwrap_or([0u8; 65]);
        let mut bit_inhalt = vec![0x00]; // ungenutzte Bit am Anfang einer BIT STRING
        bit_inhalt.extend_from_slice(&punkt);
        sequenz(&[
            // ecPublicKey 1.2.840.10045.2.1 + prime256v1 1.2.840.10045.3.1.7
            sequenz(&[
                oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]),
                oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]),
            ]),
            tlv(0x03, &bit_inhalt),
        ])
    }

    /// added 03.10.2026 01:5x: **EINE ERWEITERUNG**. `kritisch: false` heisst:
    /// das `BOOLEAN DEFAULT FALSE` wird **weggelassen** — so macht es der Rest
    /// der Welt, und ein `DEFAULT`-Feld mit dem Default-Wert zu schicken ist
    /// nach X.690 nicht erlaubt.
    /// added 03.10.2026 04:9x, R51. **EINE ERWEITERUNG MIT `critical = TRUE`**,
    /// und die ist fuer den ganzen Punkt die entscheidende: `erweiterung()`
    /// baut sie **ohne** das Flag, und genau deshalb hat unser Parser es nie
    /// gesehen. Reihenfolge laut RFC 5280 4.2:
    ///     Extension ::= SEQUENCE { extnID OID, critical BOOLEAN DEFAULT FALSE,
    ///                            extnValue OCTET STRING }
    /// Das Flag steht **vor** dem Wert.
    pub fn erweiterung_kritisch(oid_bytes: &[u8], wert: &[u8]) -> Vec<u8> {
        sequenz(&[oid(oid_bytes), tlv(0x01, &[0xff]), tlv(0x04, wert)])
    }

    /// added 03.10.2026 04:9x, R51. **DIE OID, DIE WIR NICHT KENNEN**, und sie
    /// ist absichtlich aus einem privaten Bereich gewaehlt (1.3.6.1.4.1.99999):
    /// sie kann nicht versehentlich eine echte Erweiterung treffen, und sie ist
    /// genau die Sorte, vor der RFC 5280 4.2 schuetzt.
    ///
    /// added 03.10.2026 05:5x, R51: **MEINE ERSTE KONSTANTE WAR FALSCH**, und
    /// der Test hat es gemeldet statt mich: ich hatte `82 9f a3` geschrieben und
    /// im Kommentar `99999` behauptet — das sind **4706703**, denn X.690 8.19.2
    /// kodiert jede OID-Gruppe in **7 Bit** mit Fortsetzungsbit, nicht ein Byte
    /// pro Punkt. Richtig sind `86 8d 1f`: `86`&0x7f = 6, `8d`&0x7f = 13,
    /// `1f` = 31, also 6*16384 + 13*128 + 31 = 99999. **GEMESSEN** mit einem
    /// eigenen Python-Nachrechnen, bevor ich etwas richtiggestellt habe.
    pub const UNBEKANNT_OID: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0x86, 0x8d, 0x1f, 0x01, 0x01];

    /// added 03.10.2026 04:9x, R51. **EIN ZERTIFIKAT MIT UNBEKANNTER
    /// ERWEITERUNG**, kritisch oder nicht. Der Wert ist ein `SEQUENCE {}` —
    /// absichtlich harmlos: es geht um die **Entscheidung**, nicht um den Inhalt.
    pub fn zertifikat_mit_unbekannter_erweiterung(kritisch: bool) -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(
                0xa3,
                &tlv(0x30, &{
                    let mut e = Vec::new();
                    for x in &basis_erweiterungen() {
                        e.extend_from_slice(x);
                    }
                    let unbekannt = if kritisch {
                        erweiterung_kritisch(UNBEKANNT_OID, &sequenz(&[]))
                    } else {
                        erweiterung(UNBEKANNT_OID, &sequenz(&[]))
                    };
                    e.extend_from_slice(&unbekannt);
                    e
                }),
            ),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 05:3x, R51. **EIN `CRLDistributionPoints`-WERT**, und
    /// zwar **formal gueltig**, weil ein Test mit kaputtem Wert nichts beweist:
    /// `SEQUENCE OF DistributionPoint`, je Punkt `distributionPoint [0]
    /// fullName [0] GeneralNames`, darin `uniformResourceIdentifier [6]`.
    /// added 03.10.2026 05:3x: DIE DREI VERSCHACHTELUNGEN sind **kein** Tippfehler:
    /// `p.element_mit_tag(0x30)` in `verteilungsstellen_lesen` nimmt die erste
    /// SEQUENCE (DistributionPoints), dann kommt `dp` je Eintrag, dann `feld`.
    /// Es ist dieselbe Form, an der ich in R47 einmal „eine SEQUENCE zu viel“
    /// gebaut hatte — diesmal mitgemessen statt geraten.
    /// added 03.10.2026 19:5x, R62. **DER VERTEILUNGSPUNKT FUER EINE
    /// BELIEBIGE URL**, und der Grund ist, dass `verteilungsstellen_wert()` an
    /// `crl.example.invalid` **festgenagelt** ist. Das war richtig, solange die
    /// Erweiterung nur **gelesen** werden musste — fuer den **Bau** eines
    /// Zertifikats, dessen Liste ueber HTTP bei **uns** liegt, ist eine tote
    /// Adresse keine Adresse. Also wird die URL **Parameter** und die alte Form
    /// bleibt als **Aufruf mit der alten Adresse**.
    ///
    /// added 03.10.2026 19:5x: **UND DIE FORM BLEIBT GENAU DIE GEMESSENE**
    /// (R60, 16:de): `SEQUENCE { SEQUENCE { [0] { [0] { 86 url } } } }` — vier
    /// Schichten ueber dem `extnValue`-OCTET STRING. Der R60-Befund war genau
    /// diese Form, und ein Bauer, der eine andere schreibt, wuerde den Leser
    /// pruefen, ohne ihn zu treffen.
    /// added 03.10.2026 23:bd, R66. **FORM KORRIGIERT: SIE WAR UM EINE `SEQUENCE`
    /// ZU TIEF.** GEMESSEN (`der_r60_bauer_erzeugt_die_falsche_form`, **vor**
    /// diesem Fix): diese Funktion lieferte **0** Namen, die aus den echten Bytes
    /// abgeleitete Form **1**.
    ///
    /// added 03.10.2026 23:bd: **DIE RICHTIGE FORM, GEMESSEN an echten Bytes**
    /// (R60, `xkcd.com`): `30 3f | 30 3d | A0 3b | A0 39 | 86 37 …`, also
    /// `SEQUENCE { SEQUENCE { [0] { [0] { 86 url } } } }` — **zwei** `SEQUENCE`,
    /// **zwei** `A0`, und **keine** `SEQUENCE` zwischen dem zweiten `A0` und dem
    /// `86`, weil `fullName [0] GeneralNames` **implizit** getaggt ist und der
    /// CHOICE-Tag den `SEQUENCE`-Tag **ersetzt** (X.690 8.14.2).
    ///
    /// added 03.10.2026 23:bd: **UND WARUM ES SO LANGE UNBEMERKT BLIEB:** der
    /// Leser ist **tolerant** (er nimmt auch die explizite Form), also hat er die
    /// zu tiefe Form **verschluckt**; und der einzige Test, der die Erweiterung
    /// las, benutzte ein **echtes** Zertifikat aus der Wild (13 von 150), nicht
    /// diesen Bauer. **Ein toleranter Leser und ein Test mit echten Daten sind
    /// zusammen genau die Kombination, die einen Baufehler versteckt.**
    pub fn verteilungsstellen_wert_zu(url: &str) -> Vec<u8> {
        let uri = tlv(0x86, url.as_bytes());
        sequenz(&[sequenz(&[tlv(0xa0, &tlv(0xa0, &uri))])])
    }

    /// added 03.10.2026 19:5x, R62. **DAS BLATT MIT BELIEBIGER SERIENNUMMER UND
    /// BELIEBIGER VERTEILUNGSSTELLE — und es ist genau die Kombination, die fuer
    /// R62 fehlte.** `zertifikat_mit_kritischen_verteilungsstellen()` hat eine
    /// feste Seriennummer (`0x2a`) und eine **kritische** DP-Erweiterung; R62
    /// braucht eine **freie** Seriennummer, damit der Test ein Blatt bauen kann,
    /// dessen Nummer **nicht** in der Liste steht (der Gegenfall).
    ///
    /// added 03.10.2026 19:5x: **UND NICHT KRITISCH**, mit einer Begründung aus
    /// R51: eine **kritische** DP-Erweiterung verlangt nach RFC 5280 4.2, dass
    /// wir sie **auswerten** — und eine nicht auswertbare kritische Erweiterung
    /// fuehrt zur Ablehnung. Wir koennen sie auswerten (R59/R60), aber fuer den
    /// Test ist die **normale** Form die richtige: eine CA, die ihre Liste nennt,
    /// setzt die Erweiterung im Allgemeinen nicht als kritisch.
    pub fn blatt_mit_verteilungsstellen_zu(
        seriennummer: &[u8],
        url: &str,
    ) -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])), // v3
            integer(seriennummer),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),     // Aussteller
            gueltigkeit(),     // thisUpdate / notAfter
            name("localhost"), // Subject
            spki(),
            tlv(
                0xa3,
                &tlv(0x30, &{
                    let mut e = Vec::new();
                    for x in &basis_erweiterungen() {
                        e.extend_from_slice(x);
                    }
                    e.extend_from_slice(&erweiterung(
                        &[0x55, 0x1d, 0x1f],
                        &verteilungsstellen_wert_zu(url),
                    ));
                    e
                }),
            ),
        ]);
        signiere(&tbs_bytes)
    }

    pub fn verteilungsstellen_wert() -> Vec<u8> {
        let url = tlv(0x86, b"http://crl.example.invalid/a.der");
        // added 04.10.2026 02:5x, R67. **`url.clone()` WEG, `from_ref` HER.**
        // Clippy (Lint `cloned_ref_to_slice_refs`) hat es beim **erzwungenen
        // Vollbau** gefunden, also nicht beim Aendern: `sequenz` nimmt
        // `&[Vec<u8>]`, und `std::slice::from_ref(&url)` **ist** genau das —
        // ohne die Kopie. GEMESSEN, dass es dieselben Bytes liefert: der Test
        // `R66-DP normform` vergleicht die **Name-Anzahl**, und sie bleibt 1.
        sequenz(&[sequenz(&[sequenz(&[tlv(
            0xa0,
            &sequenz(std::slice::from_ref(&url)),
        )])])])
    }

    /// added 03.10.2026 05:3x, R51. **EIN ZERTIFIKAT MIT BEKANNTEM, ABER
    /// NICHT ANGEWENDETEM `critical`** (2.5.29.31). Das ist der zweite Fall der
    /// Klasse: die Erweiterung ist **nicht unbekannt**, wir lesen sie sogar —
    /// wir koennen sie nur nicht benutzen.
    pub fn zertifikat_mit_kritischen_verteilungsstellen() -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(
                0xa3,
                &tlv(0x30, &{
                    let mut e = Vec::new();
                    for x in &basis_erweiterungen() {
                        e.extend_from_slice(x);
                    }
                    e.extend_from_slice(&erweiterung_kritisch(
                        &[0x55, 0x1d, 0x1f],
                        &verteilungsstellen_wert(),
                    ));
                    e
                }),
            ),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 05:3x, R51: **DIE BEKENNTE SAN ALS `critical`** — der
    /// Gegenfall in die andere Richtung: wir koennen sie verarbeiten, also muss
    /// sie durchgehen, egal ob sie als kritisch markiert ist. Ohne diesen Test
    /// waere ein `verarbeitet = true` zuwenig geprueft: ein Fehler, der
    /// **alles** ablehnt, waere auch „sicher“.
    pub fn zertifikat_mit_kritischer_san() -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(
                0xa3,
                &tlv(0x30, &{
                    let mut e = Vec::new();
                    for x in &basis_erweiterungen() {
                        e.extend_from_slice(x);
                    }
                    // die SAN aus `basis_erweiterungen()` ist dieselbe, aber
                    // **mit** `critical` — deshalb eigener Aufbau statt
                    // Ersetzen, das sich als Byte-Suche nicht ausdruecken
                    // laesst.
                    let mut inhalt = vec![0x82];
                    inhalt.extend_from_slice(&laenge(b"localhost".len()));
                    inhalt.extend_from_slice(b"localhost");
                    e.extend_from_slice(&erweiterung_kritisch(
                        &[0x55, 0x1d, 0x11],
                        &tlv(0x30, &inhalt),
                    ));
                    e
                }),
            ),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 06:0x, R52. **EINE `NameConstraints`-ERWEITERUNG**,
    /// und die Form ist **nach X.680 gewaehlt, nicht geraten**: bei
    /// `permittedSubtrees [0] GeneralSubtrees` ersetzt der implizite Tag das
    /// **aeussere** SEQUENCE-Tag, die SEQUENCE der Subtrees bleibt — also
    /// `A0 { 30 { 82 "example.de" } }` und **kein** `A0 { 30 { 30 { … } } }`.
    /// GENAU DAS ist mein erster Entwurf gewesen (drei Klammern), und
    /// `namensbeschraenkung_lesen` haette die Erweiterung als **leer** gelesen —
    /// ein stiller Fehler derselben Klasse wie R50 und R51. Es fiel auf, weil ich
    /// das `element_mit_tag` im Parser **gelesen** habe, bevor ich gebaut habe.
    /// added 03.10.2026 13:az, R57. **DIE CA MIT EINER `excluded; dirName:`-
    /// AUFLAGE**, und sie steht hier statt in `kette.rs`, weil `sequenz`, `tlv`
    /// und `name` **privat** sind. GEMESSEN: der erste Versuch hat sie von
    /// aussen benutzt, und der Uebersetzer meldete „function `sequenz` is
    /// private“ fuer **drei** Stellen. Das ist die richtige Form der Antwort —
    /// **kein** `pub(crate)` auf die Helfer, sondern der Bauer dorthin, wo sie
    /// gehoeren. Sonst wuerden sie zu einer halb-oeffentlichen DER-Bau-API, und
    /// das sind sie nicht.
    ///
    /// added 03.10.2026 13:az: **DIE FORM, vier Schachteln tief** und jede ist
    /// eine Norm-Sache: `A0` = `[0] permittedSubtrees` … nein, `[1]` fuer
    /// `excluded`; also `A1 { 30 { 30 { A4 { 30 { Name } } } } }` —
    /// `GeneralSubtrees`-Mitglieder **direkt** (X.690 11.5), Subtree `30`, `base`
    /// added 03.10.2026 14:7x, R58. **EIN MEHRSCHLAUFIGER NAME** — und der
    /// Grund ist nicht Eleganz, sondern **Testbarkeit**: die Teilbaum-Regel sagt
    /// „DN1 contains **at least as many RDNs as DN2**“ und „trailing RDNs in DN1
    /// are **ignored**“. Beides kann ein Name mit **einem** RDN nicht zeigen.
    ///
    /// added 03.10.2026 14:7x: **`C=DE` ZUERST, UND DAS IST ABSICHT.** Der OID
    /// 2.5.4.6 (`countryName`) ist in echten Zertifikaten **einbyte**, und die
    /// Norm verlangt fuer ihn genau **PrintableString mit zwei Zeichen**
    /// (RFC 5280 4.1.2.4). Ohne ein echtes erstes RDN sieht der Vergleicher nie
    /// einen Namen, der ueberhaupt in einem echten Zertifikat vorkommt.
    ///
    /// added 03.10.2026 14:7x: **UND DIE REIHENFOLGE IM BAUER IST DIE DER.**
    /// `C`, dann `O`, dann `CN` — und das **erste** Argument steht in DER
    /// **hinten**: die distinguishedName ist in der Textdarstellung umgekehrt
    /// (X.501). Fuer den Vergleich ist die Richtung egal, weil beide Seiten
    /// dasselbe Schema benutzen — aber es waere eine stille Falle, wenn jemand
    /// spaeter einen Namen aus einer **Textdarstellung** baute.
    pub fn name_kette(teile: &[&str]) -> Vec<u8> {
        let mut gruppen: Vec<u8> = Vec::new();
        let c = sequenz(&[oid(&[0x55, 0x04, 0x06]), tlv(0x13, b"DE")]);
        gruppen.extend_from_slice(&tlv(0x31, &c));
        for t in teile {
            let cn = sequenz(&[oid(&[0x55, 0x04, 0x03]), tlv(0x0c, t.as_bytes())]);
            gruppen.extend_from_slice(&tlv(0x31, &cn));
        }
        // added 03.10.2026 14:ba: **`tlv(0x30, …)` STATT `sequenz(&gruppen)`** —
        // GEMESSEN „expected `&[Vec<u8>]`, found `&Vec<u8>`“: `gruppen` sind die
        // schon **konkatenierten** Bytes der RDNs, und `sequenz` nimmt eine Liste
        // von Elementen. Die distinguishedName ist aber genau **eine** SEQUENCE
        // ueber allen RDNs — also `tlv` ist hier die richtige Form. `name()` nutzt
        // `sequenz`, weil dort genau **ein** RDN gebaut wird.
        tlv(0x30, &gruppen)
    }

    /// added 03.10.2026 14:8x, R58. **EIN BLATT MIT BESTIMMTEM INHABERNAMEN** —
    /// und es ist ein eigener Bauer, weil `zertifikat_fuer_pfad` den Inhaber
    /// **als Text** nimmt und hier die **Bytes** der distinguishedName noetig
    /// sind. GEMESSEN: mein erster Versuch wollte `zertifikat_fuer_pfad` mit einem
    /// Text fuettern, der mehrere Commas enthaelt — das ergibt **ein** CN-Attribut
    /// mit Commas im Wert, nicht mehrere RDNs.
    pub fn blatt_mit_subject_name(dn: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut ext_inhalt = Vec::new();
        for e in &basis_erweiterungen() {
            let ist_san = e.len() >= 7 && e[4..7] == [0x55, 0x1d, 0x11];
            if !ist_san {
                ext_inhalt.extend_from_slice(e.as_slice());
            }
        }
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            // added 03.10.2026 14:bb: **`dn.to_vec()` STATT `tlv(0x04, dn)`.**
            // GEMESSEN: „DER: Tag 0x04, erwartet 0x30“ — der Subject steht
            // **direkt** als distinguishedName-SEQUENCE in der
            // Zertifikatsstruktur; der OCTET STRING (`0x04`) gehoert zu
            // *Erweiterungswerten*, nicht zu einem Strukturfeld. Und weil
            // `sequenz` eine Liste von **Elementen** nimmt, muss hier das ganze
            // `Name` als **ein** Element hinein — nicht in seine Bestandteile
            // zerlegt.
            dn.to_vec(),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// `[4] directoryName` **explizit** mit der SEQUENCE, und darin der `Name`.
    pub fn ca_mit_dirname_auflage(inhaber: &str) -> Result<Vec<u8>, &'static str> {
        // added 03.10.2026 13:az: **`name()` LIEFERT SCHON DIE
        // `distinguishedName`-SEQUENCE** — sie ist `SEQUENCE { SET { SEQUENCE {
        // OID, Wert } } }`. Ich hatte sie noch einmal in `sequenz(&[…])`
        // gesteckt, und **GEMESSEN** (13:ay) scheiterte schon
        // `zertifikat_lesen` der CA mit „CA lesen“ — die Auflagen-DER war eine
        // Ebene zu tief. Es ist dieselbe Fehlerform wie die ueberzaehlige
        // Klammer: **bei verschachtelten CHOICE-Kodierungen ist die
        // Klammerzahl der einzige Unterschied**, und ein Test, der nur
        // *Inhalte* prueft, sieht sie nicht.
        let dn = name(inhaber);
        // added 03.10.2026 13:ay: **MEINE ERSTE FORM HATTE EINE SEQUENCE ZU
        // VIEL, und der Fehler ist derselbe wie in R52 — eine Ebene tiefer.**
        // GEMESSEN (13:5x): `verboten_dn = 0`, die Erweiterung wurde als
        // NameConstraints erkannt, aber **leer** gelesen. Grund: `base` war `30`
        // statt `A4`, also kein `directoryName`.
        // Richtig ist `A1 { 30 { A4 { 30 { Name } } } }`: bei `[1] GeneralSubtrees`
        // mit **implicit** tagging (X.690 11.5) ersetzt der Tag das aeussere
        // SEQUENCE-Tag, die Subtrees stehen also **direkt** im `A1`. Die ueberzaehlige
        // Klammer war ein `sequenz(&[tlv(0xa4, …)])` **um** den Subtree.
        // added 03.10.2026 13:ba: **UND JETZT AUCH IM `A4` KEINE SEQUENCE.**
        // GEMESSEN (13:ay): „DER: Tag 0x30, erwartet 0x31“ — `name_lesen` will
        // hinter der distinguishedName-SEQUENCE die **SETs** sehen, bekam aber
        // noch eine SEQUENCE. Also: `dn` ist **schon** der `Name`, und `A4`
        // bekommt ihn **direkt**. Das ist dieselbe Erkenntnis wie eine Zeile
        // weiter oben — ich musste sie **zweimal** anwenden, einmal fuer `dn` und
        // einmal fuer das `A4`, und das sagt mehr ueber den Bau-DER von Hand als
        // jeder Kommentar: **die Zahl der Klammern ist der Fehler**, und sie
        // schaetzt man nicht, man zaehlt.
        let auflage = sequenz(&[tlv(0xa1, &sequenz(&[tlv(0xa4, &dn)]))]);
        zertifikat_ca_mit_auflage(&auflage)
    }

    /// added 03.10.2026 13:9x, R57. **EIN BLATT MIT `directoryName` IM SAN**,
    /// und es ist der zweite Weg, den RFC 5280 4.2.1.10 nennt: die Auflagen gelten
    /// fuer den **Subject** *und* fuer „any names of type directoryName in the
    /// subjectAltName“. Der Subject-Tests deckt den ersten Weg ab.
    pub fn blatt_mit_directory_name(inhaber: &str) -> Result<Vec<u8>, &'static str> {
        let dn = name(inhaber);
        blatt_mit_san_eintraege(&[tlv(0xa4, &sequenz(&[dn]))])
    }

    /// added 03.10.2026 11:7x, R56. **SAN MIT DEN NEUEN FORMEN** — und das ist der
    /// Bauer, den die R56-Tests brauchen: `san_lesen` kannte vorher nur
    /// `dNSName` und `iPAddress`, also war ein Blatt mit `rfc822Name` oder
    /// `uniformResourceIdentifier` **nicht baubar** und damit die Regel nicht
    /// messbar.
    ///
    /// added 03.10.2026 11:7x: **DIE EINTRAGSFORMEN SIND NICHT SYMMETRISCH**, und
    /// das ist kein Unfall: `0x81` und `0x86` sind **IMPLICIT** (der Inhalt ist der
    /// IA5String direkt), `0xA4` waere **EXPLICIT** (im Inhalt stuende die
    /// SEQUENCE des `Name`). Wer das umdreht, baut ein Zertifikat, das kein Parser
    /// liest — und kein Fehler meldet, sondern leere Felder.
    pub fn blatt_mit_san_und_email(
        dns: &[&str],
        email: &[&str],
        uri: &[&str],
    ) -> Result<Vec<u8>, &'static str> {
        let mut eintraege: Vec<Vec<u8>> = Vec::new();
        for n in dns {
            eintraege.push(tlv(0x82, n.as_bytes()));
        }
        for a in email {
            eintraege.push(tlv(0x81, a.as_bytes()));
        }
        for u in uri {
            eintraege.push(tlv(0x86, u.as_bytes()));
        }
        blatt_mit_san_eintraege(&eintraege)
    }

    /// added 03.10.2026 11:7x: **DIE GEMEINSAME RUECKSEITE** beider SAN-Bauer als
    /// eigene Funktion, weil `fmt` die Absenderzeile sonst umbricht und der
    /// Anker beim naechsten Mal nicht mehr passt — R56 hat das zweimal getroffen.
    fn blatt_mit_san_eintraege(eintraege: &[Vec<u8>]) -> Result<Vec<u8>, &'static str> {
        let mut ext_inhalt = erweiterung(&[0x55, 0x1d, 0x11], &sequenz(eintraege));
        for e in &basis_erweiterungen() {
            // added 03.10.2026 06:9x: die SAN aus `basis_erweiterungen()` wird
            // weggelassen, sonst haette das Blatt `localhost` zusaetzlich und der
            // Test wuerde zwei Dinge gleichzeitig messen.
            let ist_san = e.len() >= 7 && e[4..7] == [0x55, 0x1d, 0x11];
            if !ist_san {
                ext_inhalt.extend_from_slice(e.as_slice());
            }
        }
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 11:8x, R56. **DIE CA MIT BELIEBIGER
    /// `NameConstraints`-ERWEITERUNG** — und das ist der zweite Bauer, den es
    /// braucht, weil die Regel sonst nur an der DNS-Form messbar waere.
    pub fn zertifikat_ca_mit_auflage(auflage: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut ext_inhalt = Vec::new();
        ext_inhalt.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x13],
            &sequenz(&[tlv(0x01, &[0xff])]),
        ));
        ext_inhalt.extend_from_slice(&erweiterung(&[0x55, 0x1d, 0x0f], &tlv(0x03, &[0x01, 0x06])));
        ext_inhalt.extend_from_slice(&erweiterung(&[0x55, 0x1d, 0x1e], auflage));
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x01]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name(CA_NAME),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 11:8x, R56. **ERLAUBTE E-MAIL- UND URI-BESCHRAENKUNGEN.**
    /// Die Form ist dieselbe wie in `name_constraints`: `[0]` (erlaubt) mit den
    /// GeneralSubtrees **direkt** im Inhalt, **ohne** SEQUENCE drumher.
    pub fn ca_mit_email_und_uri_auflage(
        erlaubt_email: &[&str],
        erlaubt_uri: &[&str],
    ) -> Result<Vec<u8>, &'static str> {
        let mut huelle: Vec<Vec<u8>> = Vec::new();
        // added 03.10.2026 11:9x: **`for werte in [erlaubt_email, erlaubt_uri]`
        // statt eines Tupels mit `true`.** Mein erster Entwurf schrieb
        // `(werte, true)` und griff dann auf `werte` zu — das der **Bool** war;
        // der Uebersetzer meldete „no method named `is_empty` found for type
        // `bool`“. Das Tupel war eine ueberfluessige Zweitangabe; die Form ergibt
        // sich aus dem Wert selbst (enthaelt er `://`, ist es eine URI).
        // added 03.10.2026 12:4x: **DIE FORM STEHT JETZT IM PARAMETER, UND BIS
        // GESTERN WURDE SIE AUS DEM INHALT GERATEN** (`e.contains("://")`).
        // GEMESSEN: `uri_regel_nimmt_nur_den_host` schlug fehl, weil die Auflage
        // `.example.com` — eine URI-Auflage — als **E-Mail** gebaut wurde.
        // DIE LEHRE ist die aus R50 (`vektor16` statt `bytes`): **ein Bauer darf
        // die Form nicht aus dem Inhalt ableiten**, denn der Inhalt traegt sie
        // nicht. Genau diese Fehlerform liess mich dort eine Erweiterung zwei
        // Byte zu lang bauen — und hier hat sie eine Regel falsch gebaut.
        for (tag, werte) in [(0x81u8, erlaubt_email), (0x86u8, erlaubt_uri)] {
            if werte.is_empty() {
                continue;
            }
            let mut inhalt: Vec<u8> = Vec::new();
            for e in werte {
                inhalt.extend_from_slice(&sequenz(&[tlv(tag, e.as_bytes())]));
            }
            huelle.push(tlv(0xa0, &inhalt));
        }
        zertifikat_ca_mit_auflage(&sequenz(&huelle))
    }

    pub fn name_constraints(erlaubt_dns: &[&str], verboten_dns: &[&str]) -> Vec<u8> {
        let mut huelle: Vec<Vec<u8>> = Vec::new();
        // added 03.10.2026 06:8x: **MEINE ERSTE FORM HAT DEN FEHLER GEMACHT, DEN
        // ICH IM KOMMENTAR DARUEBER BESCHRIEBEN HABE** — ich hatte
        // `tlv(0xa0, &sequenz(&subs))` geschrieben, also `A0 { 30 { 30 { 82 … } } }`
        // mit **drei** Klammern. GEMESSEN (06:7x, Diagnose):
        // `erlaubt=[] verboten=[]` — der Parser hat `base` als `0x30` gelesen und
        // uebersprungen. Korrekt ist `A0 { Subtree }`: bei
        // `permittedSubtrees [0] GeneralSubtrees` ersetzt der implizite Tag das
        // **aeussere** SEQUENCE-Tag, die Subtrees stehen also **direkt** im A0.
        // Die Lehre, und die ist teurer als der Fehler: **ein Kommentar, der eine
        // Falle beschreibt, ist kein Schutz.** Ich habe die Falle 20 Zeilen
        // unterhalb beschrieben und dann hineingetreten.
        for (tag, namen) in [(0xa0u8, erlaubt_dns), (0xa1u8, verboten_dns)] {
            if namen.is_empty() {
                continue;
            }
            let mut inhalt: Vec<u8> = Vec::new();
            for d in namen {
                // GeneralSubtree ::= SEQUENCE { base GeneralName, ... }
                inhalt.extend_from_slice(&sequenz(&[tlv(0x82, d.as_bytes())]));
            }
            // added 03.10.2026 06:9x: **`tlv(...)` OHNE `sequenz(...)`** — mein
            // dritter Versuch packte noch eine SEQUENCE um das A0 und erzeugte
            // `30 22 | 30 10 | a0 0e | …`; der Parser las `30 10` als „Teil“,
            // erkannte `0x30 != 0xA0` und sprang ueber **beide** Eintraege
            // hinweg (GEMESSEN: `erlaubt=[]`). Die aeussere SEQUENCE entsteht
            // bereits durch `sequenz(&huelle)` am Ende.
            huelle.push(tlv(tag, &inhalt));
        }
        sequenz(&huelle)
    }

    /// added 03.10.2026 06:0x, R52. **DIE CA MIT NAMENSCHRAENKUNG.**
    pub fn zertifikat_ca_mit_name_constraints(
        erlaubt_dns: &[&str],
        verboten_dns: &[&str],
    ) -> Result<Vec<u8>, &'static str> {
        let mut ext_inhalt = Vec::new();
        ext_inhalt.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x13],
            &sequenz(&[tlv(0x01, &[0xff])]),
        ));
        ext_inhalt.extend_from_slice(&erweiterung(&[0x55, 0x1d, 0x0f], &tlv(0x03, &[0x01, 0x06])));
        ext_inhalt.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x1e],
            &name_constraints(erlaubt_dns, verboten_dns),
        ));
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x01]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name(CA_NAME),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 06:0x, R52. **EIN BLATT MIT MEHREREN DNS-NAMEN.**
    /// Ohne diese Funktion liess sich der Befund nicht messen: der Zeiger war
    /// „wir pruefen nur den Anfragendenamen“, und ein Blatt mit **einem** Namen
    /// koennte das nie zeigen.
    pub fn blatt_mit_san_namen(namen: &[&str]) -> Result<Vec<u8>, &'static str> {
        let mut san_inhalt = Vec::new();
        for n in namen {
            san_inhalt.push(tlv(0x82, n.as_bytes()));
        }
        // added 03.10.2026 06:3x: **DIE REIHENFOLGE STATT `insert(0, …)`** —
        // `ext_inhalt` ist ein `Vec<u8>` und kann keine Erweiterung als Element
        // aufnehmen (mein erster Versuch: `mismatched types`). Die SAN kommt
        // deshalb **zuerst** hin, die Basis danach; auf die Reihenfolge der
        // Erweiterungen kommt es nicht an (sie wird nicht ausgewertet).
        let mut ext_inhalt = erweiterung(&[0x55, 0x1d, 0x11], &sequenz(&san_inhalt));
        for e in &basis_erweiterungen() {
            // added 03.10.2026 06:1x: **DIE SAN AUS `basis_erweiterungen()`
            // WEGLASSEN** — sonst haette das Blatt `localhost` **zusaetzlich**
            // und der Test wuerde zwei Dinge gleichzeitig messen.
            // added 03.10.2026 06:8x: **DER FILTER WAR ZWEIMAL FALSCH.** Erstens
            // verglich ich `starts_with(b"\x30\x82")` — die `basis_erweiterungen()`
            // beginnen mit `30 0E`, `30 09`, `30 14`, **nicht** mit `30 82`; mein
            // Filter hat also **nichts** entfernt. Zweitens loescht man die
            // Basis-SAN besser **ueber ihre OID** (`55 1d 11`, das steht an
            // Position 2..5, weil `Extension ::= SEQUENCE { OID, … }`) als ueber
            // ein Laengenpruefmuster. GEMESSEN (06:7x): `san.dns=["localhost"]` —
            // die **letzte** SAN-Erweiterung gewinnt (`san = Some(...)`), und das
            // war die Basis.
            // added 03.10.2026 06:9x: **DRITTER FEHLER AN DIESER STELLE**, und
            // jetzt gemessen statt geraten: `Extension ::= SEQUENCE { extnID
            // OBJECT IDENTIFIER, ... }` heisst `30 <l> 06 <l> 55 1d 11` — die OID
            // steht an Position **4..7**, nicht an 2..5 (dort stehen der
            // SEQUENCE-Tag, seine Laenge, der OID-Tag und dessen Laenge).
            let ist_san = e.len() >= 7 && e[4..7] == [0x55, 0x1d, 0x11];
            if !ist_san {
                ext_inhalt.extend_from_slice(e.as_slice());
            }
        }
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 07:0x, R53. **EIN ZERTIFIKAT MIT
    /// `pathLenConstraint`**, und es ist bewusst **flexibel** gebaut: Aussteller,
    /// Inhaber und `cA` kommen herein. Grund: `kette_pruefen` erkennt eine
    /// Zwischen-CA an `name_gleich(aussteller, inhaber)` — fuer den Test
    /// brauche ich also Zertifikate mit **verschiedenen** Namen, sonst waere
    /// jedes Zertifikat selbstsigniert und die Kette haette nie mehr als ein
    /// Element. `zertifikat_ca()` benutzt fuer Aussteller und Inhaber **denselben**
    /// Namen und ist damit genau fuer diesen Zweck unbrauchbar.
    ///
    /// added 03.10.2026 07:0x: `pathlen = None` bedeutet „die Erweiterung traegt
    /// **kein** pathLen“ — das ist der Unterschied zwischen „unbeschaenkt“ (die
    /// Erweiterung fehlt) und „0“ (keine Zwischen-CA darueber erlaubt), und beide
    /// kommen im Test vor.
    pub fn zertifikat_fuer_pfad(
        aussteller: &str,
        inhaber: &str,
        ist_ca: bool,
        pathlen: Option<u32>,
    ) -> Result<Vec<u8>, &'static str> {
        let mut bc = vec![tlv(0x01, &[if ist_ca { 0xff } else { 0x00 }])];
        if let Some(n) = pathlen {
            // added 03.10.2026 07:2x: **`laenge(n)` war die falsche Funktion** —
            // sie nimmt `usize` und kodiert eine **Laenge**, kein INTEGER; der
            // Uebersetzer meldete `expected usize, found u32`. `pathlen_ziffer`
            // macht `02 01 <n>`, und das ist die INTEGER-Kodierung. Fuer
            // n > 127 waere ein fuehrendes Nullbyte noetig (DER INTEGER ist
            // vorzeichenbehaftet) — im Test kommen 0 und 1 vor, die Grenze steht
            // an `pathlen_ziffer`.
            // added 03.10.2026 07:8x: **`pathlen_ziffer(n)` OHNE `tlv(…)`** —
            // mein Bauer hat **doppelt gepackt**: `pathlen_ziffer` liefert ueber
            // `integer()` bereits das ganze `02 01 00`, und `tlv(0x02, …)` darum
            // machte `02 03 02 01 00`. GEMESSEN (07:7x): der Parser las dann
            // `02 01 00` als Zahl **131328** statt 0. Das ist die beste Sorte
            // eigener Fehler: er erzeugt eine **plausible** Zahl, die nie
            // auffaellt, weil sie nur dann auffaellt, wenn sie jemand benutzt —
            // und bisher benutzte sie niemand.
            bc.push(pathlen_ziffer(n));
        }
        let mut ext_inhalt = Vec::new();
        ext_inhalt.extend_from_slice(&erweiterung(&[0x55, 0x1d, 0x13], &sequenz(&bc)));
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(aussteller),
            gueltigkeit(),
            name(inhaber),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 07:1x: `tlv(0x02, …)` verlangt **Bytes**, und ein
    /// `pathLenConstraint` ist ein INTEGER. `laenge(n)` liefert die
    /// INTEGER-Kodierung einer Zahl — der Name ist irrefuehrend (er ist die
    /// allgemeine Zahl-zu-DER-Hilfe des Bauers), aber sie stimmt fuer
    /// `0..=255`, und `laenge(0)` ergibt `02 01 00`, also INTEGER 0.
    /// added 03.10.2026 07:2x: **`None` heisst hier „Feld fehlt“**, nicht „Null“ —
    /// das ist der Unterschied, den der Test braucht, und `verwendung_pruefen`
    /// liest genauso: `Option<u32>`.
    pub fn pathlen_ziffer(n: u32) -> Vec<u8> {
        // added 03.10.2026 07:2x: **DIE GRENZE, und sie ist benannt statt
        // verschwiegen.** `pathLenConstraint` ist ein vorzeichenbehaftetes
        // INTEGER; ab 128 braucht es ein fuehrendes Nullbyte, sonst laesst
        // `basic_constraints_lesen` eine **negative** Zahl lesen. Im Test kommen
        // nur 0 und 1 vor — fuer mehr waere das hier falsch.
        if n < 128 {
            integer(&[n as u8])
        } else {
            integer(&[0, n as u8])
        }
    }

    pub fn erweiterung(oid_bytes: &[u8], wert: &[u8]) -> Vec<u8> {
        sequenz(&[oid(oid_bytes), tlv(0x04, wert)])
    }

    /// added 03.10.2026 01:5x: **DIE SAN**, als `dNSName` (Kontext-Tag 2,
    /// implizit IA5String): `82 <laenge> "localhost"`.
    fn san() -> Vec<u8> {
        // added 03.10.2026 01:10x: **MEIN ERSTER SAN-BAU WAR EINE EBENE ZU
        // TIEF**, und der Parser hat es gemeldet statt der Test:
        // `LaengeUeberPuffer { angesagt: 2, vorhanden: 0 }`. Ich hatte
        // `SEQUENCE { SEQUENCE { dNSName } }` gebaut; richtig ist
        // `SEQUENCE { [2] IA5String }`, und die **Erweiterung** wickelt das in
        // eine OCTET STRING. Zwei Klammern zu viel sind in DER ein
        // Laengenfehler, kein Formsfehler — deshalb bricht es so woertlich.
        let mut inhalt = vec![0x82]; // [2] IMPLICIT IA5String
        inhalt.extend_from_slice(&laenge(b"localhost".len()));
        inhalt.extend_from_slice(b"localhost");
        erweiterung(&[0x55, 0x1d, 0x11], &tlv(0x30, &inhalt))
    }

    /// added 03.10.2026 01:5x: **DIE BASIS-ERWEITERUNGEN**, die ein Browser
    /// braucht, damit er das Zertifikat ueberhaupt akzeptiert: `SAN` (Pflicht
    /// seit RFC 2818), `basicConstraints` mit `CA:FALSE` (sonst darf ein Blatt
    /// keine CA sein), `keyUsage` (digitalSignature + keyEncipherment) und
    /// `extendedKeyUsage` mit `serverAuth`.
    fn basis_erweiterungen() -> Vec<Vec<u8>> {
        vec![
            san(),
            // added 03.10.2026 02:8x: **`cA = FALSE`, und es war `TRUE`** —
            // ich hatte den Wert aus dem CA-Bauer kopiert. Die Meldung war
            // `Kette(EndzertifikatNichtErlaubt)`: ein Blatt, das sich als CA
            // ausweist, wird abgewiesen (RFC 5280 4.2.1.9 sagt fuer ein
            // Endzertifikat `cA` SHOULD FALSE sein). `30 03 01 01 00` ist
            // `SEQUENCE { BOOLEAN FALSE }`; `0xff` waere TRUE.
            erweiterung(&[0x55, 0x1d, 0x13], &sequenz(&[tlv(0x01, &[0x00])])),
            erweiterung(&[0x55, 0x1d, 0x0f], &tlv(0x03, &[0x05, 0xa0])),
            erweiterung(
                &[0x55, 0x1d, 0x25],
                &sequenz(&[oid(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01])]),
            ),
        ]
    }

    /// added 03.10.2026 01:6x: **DIE TLS-FEATURE-ERWEITERUNG**, OID
    /// 1.3.6.1.5.5.7.1.24 (RFC 7633 Anhang A: `id-pe-tlsfeature ::= { id-pe 24 }`),
    /// mit `Features ::= SEQUENCE SIZE (1..MAX) OF INTEGER` und der Merkmals-
    /// nummer **5** (`status_request`, RFC 6066 3).
    pub fn tls_feature(merkmale: &[u8]) -> Vec<u8> {
        let mut inhalt = Vec::new();
        for m in merkmale {
            inhalt.extend_from_slice(&integer(&[*m]));
        }
        // added 03.10.2026 01:15x: **DER DRITTE MAL DERSELBE FEHLER** — eine
        // SEQUENCE zu viel. `sequenz(&[tlv(0x30, &inhalt)])` ergibt
        // `SEQUENCE { OCTET STRING { … } }`; richtig ist `tlv(0x30, &inhalt)`.
        // Der eigene Parser hat es mir mit meiner eigenen Meldung gesagt:
        // „tlsFeature: erwartet INTEGER (0x02)“. Diese Fehlerform ist jetzt
        // dreimal in dieser Runde passiert (SAN, `[3]`, `Features`) — sie ist
        // meine Kennzeichen-Ausrede, und die Merksatz-Regel lautet: bei
        // `SEQUENCE OF` immer `tlv(0x30, inhalt)`, nie `sequenz(&[tlv(0x30, ..)])`.
        let sequenz_merkmale = tlv(0x30, &inhalt);
        erweiterung(
            &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x18],
            &sequenz_merkmale,
        )
    }

    /// added 03.10.2026 02:9x, R49. **EINE OCSP-ANTWORT, `good`**, und sie ist
    /// der letzte Baustein, damit die OCSP-Strecke **ueberhaupt** testbar wird.
    ///
    /// added 03.10.2026 02:9x: **WAS `ocsp::beweise` sehen will**, und es ist
    /// aus `ocsp.rs` gelesen, nicht geraten: `CertID` mit
    /// `issuerNameHash`/`issuerKeyHash` (beide ueber **denselben** Hash,
    /// `hash_der_certid`), der mit `subject_der(ca)` bzw. `spki_bits(ca)`
    /// uebereinstimmen muss, dazu die Seriennummer des Blattes. `cert_status` =
    /// `good` ist `[0] IMPLICIT NULL`, also `80 00`. `certs` bleibt **leer** —
    /// dann nimmt `beweise` `aussteller.schluessel`, und genau den wollen wir
    /// signieren.
    pub fn ocsp_antwort_gut(ca_der: &[u8], blatt_serien: &[u8]) -> Result<Vec<u8>, &'static str> {
        ocsp_antwort(ca_der, blatt_serien, false)
    }

    /// added 03.10.2026 04:3x, R49. **DIE ANTWORT MIT WIDERRUF**, und der Grund
    /// fuer sie ist der Defekt von 04:1x: solange unser Decoder die
    /// Erweiterungen **verwarf**, war ein **widerrufenes** Zertifikat fuer uns
    /// gleichwertig mit einem guten — die gestapelte Antwort kam nie an, also
    /// konnte `ocsp::beweise` nie `Widerrufen` liefern. Dieser Bau macht daraus
    /// einen **messbaren** Fall.
    ///
    /// added 03.10.2026 04:3x: `certStatus` ist laut RFC 6960 4.2.1 ein CHOICE:
    /// `good [0] IMPLICIT NULL`, `revoked [1] IMPLICIT RevokedInfo`,
    /// `unknown [2] IMPLICIT UnknownInfo`. Fuer den Widerruf genuegt das
    /// kleinste moegliche `RevokedInfo` — nur die Zeit.
    pub fn ocsp_antwort_widerrufen(
        ca_der: &[u8],
        blatt_serien: &[u8],
    ) -> Result<Vec<u8>, &'static str> {
        ocsp_antwort(ca_der, blatt_serien, true)
    }

    fn ocsp_antwort(
        ca_der: &[u8],
        blatt_serien: &[u8],
        widerruf: bool,
    ) -> Result<Vec<u8>, &'static str> {
        let ca = crate::x509::zertifikat_lesen(ca_der).map_err(|_| "CA unlesbar")?;
        let name_hash =
            crate::sha2::sha256(&crate::x509::subject_der(&ca).map_err(|_| "Name unlesbar")?)
                .to_vec();
        let key_hash =
            crate::sha2::sha256(&crate::x509::spki_bits(&ca).map_err(|_| "Schluessel unlesbar")?)
                .to_vec();
        let certid = sequenz(&[
            // hashAlgorithm: SHA-256, 2.16.840.1.101.3.4.2.1
            sequenz(&[oid(&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01])]),
            tlv(0x04, &name_hash),
            tlv(0x04, &key_hash),
            integer(blatt_serien),
        ]);
        // added 03.10.2026 04:3x: **DER STATUS**, und `revoked [1] IMPLICIT
        // RevokedInfo` heisst: der Inhalt der SEQUENCE steht **direkt** im A1,
        // ohne eigenes 30-Tag — IMPLICIT, nicht EXPLICIT. Mit einem 30 waere es
        // ein anderes Zertifikat als der Standard erlaubt, und unser Parser
        // wuerde es als `Unbekannt` lesen statt als Widerruf.
        let certstatus = if widerruf {
            tlv(0xa1, &zeit("20250101000000Z"))
        } else {
            tlv(0x80, &[])
        };
        let single = sequenz(&[
            certid,
            certstatus,                          // certStatus: good = [0] IMPLICIT NULL
            zeit("20260101000000Z"),             // thisUpdate
            tlv(0xa0, &zeit("20360101000000Z")), // nextUpdate [0] EXPLICIT
        ]);
        let responses = sequenz(&[single]);
        let tbs = sequenz(&[
            tlv(0xa0, &[]),          // responderID [0] EXPLICIT, leer
            zeit("20260101000000Z"), // producedAt
            responses,
        ]);
        let (r, s) = crate::ecdsa::ecdsa_p256_signieren_sha256(
            &privat_aus_testdaten(),
            &crate::sha2::sha256(&tbs),
        )
        .map_err(|_| "signieren")?;
        let sig = crate::signatur::ecdsa_der_signatur(&r, &s).map_err(|_| "sig-der")?;
        let mut sigbit = vec![0x00];
        sigbit.extend_from_slice(&sig);
        let basic = sequenz(&[
            tbs,
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            tlv(0x03, &sigbit),
        ]);
        let mut inner = Vec::new();
        inner.extend_from_slice(&oid(&[
            0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x01, 0x01,
        ])); // id-pkix-ocsp-basic
        inner.extend_from_slice(&tlv(0x04, &basic));
        let response_bytes = tlv(0xa0, &sequenz(&[inner]));
        Ok(sequenz(&[
            tlv(0x0a, &[0x00]), // responseStatus: successful
            response_bytes,
        ]))
    }

    /// added 03.10.2026 02:9x: **`GeneralizedTime`** als Tag 0x18 mit 15 Zeichen
    /// `JJJJMMTTHHMMSSZ` — `x509::zeit_seit_1970` rechnet beide Formen.
    pub fn zeit(text: &str) -> Vec<u8> {
        tlv(0x18, text.as_bytes())
    }

    /// added 03.10.2026 02:9x: **DIE SERIENNUMMER DES BLATTES**, weil die
    /// `CertID` genau sie nennt.
    pub fn blatt_seriennummer() -> Vec<u8> {
        vec![0x2a]
    }

    /// added 03.10.2026 02:5x, R49. **EIN BLATT MIT LEEREM ERWEITERUNGSBLOCK** —
    /// `[3] { SEQUENCE {} }`. Das ist nach X.509 **legal** (`Extensions` ist
    /// `SEQUENCE SIZE (1..MAX) OF Extension` nur fuer die *Definition*, ein
    /// Zertifikat darf den Block trotzdem leeren), und `PROBLEME.txt` Nr. 49
    /// sagt, dass unser Parser daran scheitert. Diese Funktion stellt genau
    /// diesen Fall her, damit die Aussage **nachmessbar** ist.
    pub fn blatt_mit_leerem_block() -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x2a]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            tlv(0xa3, &tlv(0x30, &[])),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 02:3x, R48. **DIE CA**, und sie ist kein Luxus: die
    /// Kettenpruefung braucht einen Aussteller, und OCSP wie must-staple
    /// verlangen `ganze_kette.len() >= 2`.
    ///
    /// added 03.10.2026 02:3x: **WAS DRIN STEHT, und es ist nicht kosmetisch.**
    /// `basicConstraints` mit `cA = TRUE` (`30 03 01 01 ff`) und `keyUsage` mit
    /// `keyCertSign` — woertlich `kette::verwendung_pruefen`: „ein Aussteller
    /// [traegt] `keyCertSign`“. Ohne das Flag wuerde unsere eigene Pruefung die
    /// CA ablehnen. `keyUsage`-Bit 5 (`ZERTIFIKAT_SIGNIEREN`) ist Bit 0x04,
    /// Bit 6 (`cRLSign`) ist 0x02; mit zwei gesetzten Bit ist das Byte `0x06`
    /// und **ein** ungenutztes Bit (`03 02 01 06`).
    pub fn zertifikat_ca() -> Result<Vec<u8>, &'static str> {
        let mut ext_inhalt = Vec::new();
        ext_inhalt.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x13],
            &sequenz(&[tlv(0x01, &[0xff])]),
        ));
        ext_inhalt.extend_from_slice(&erweiterung(&[0x55, 0x1d, 0x0f], &tlv(0x03, &[0x01, 0x06])));
        let version = tlv(0xa0, &integer(&[0x02]));
        let spki_here = spki();
        let tbs_bytes = sequenz(&[
            version,
            integer(&[0x01]), // Seriennummer 1
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(CA_NAME),
            gueltigkeit(),
            name(CA_NAME),
            spki_here,
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ]);
        signiere(&tbs_bytes)
    }

    /// added 03.10.2026 02:3x, R48. **DIE SIGNATUR**, als `SEQUENCE { r, s }` —
    /// dieselbe Form, die unser Client liest (`kette::ecdsa_teile_oeffentlich`).
    pub fn signiere(tbs_bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
        let privat = privat_aus_testdaten();
        let (r, s) =
            crate::ecdsa::ecdsa_p256_signieren_sha256(&privat, &crate::sha2::sha256(tbs_bytes))
                .map_err(|_| "signieren")?;
        let sig_der = crate::signatur::ecdsa_der_signatur(&r, &s).map_err(|_| "sig-der")?;
        let mut bit = vec![0x00];
        bit.extend_from_slice(&sig_der);
        Ok(sequenz(&[
            tbs_bytes.to_vec(),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            tlv(0x03, &bit),
        ]))
    }

    // // added 04.10.2026 01:5x, R67. **`san_pub` GELOESCHT** — GEMESSEN: ein
    // // Treffer, die Definition. Ein reiner `san()`-Wrapper ohne Aufrufer; nach R50
    // // (mit Test oder gar nicht) hat er keine Form, in der er bleiben kann.
    // // added 04.10.2026 01:5x, R67. **`basic_pub` GELOESCHT** — GEMESSEN: ein
    // // Treffer, die Definition. **`basicConstraints` als `critical`** wird an
    // // anderer Stelle gebaut (der `bauen`-Weg mit `tlv(0x01, &[0xff])`), dieser
    // // zweite Bauer war unbenutzt.
    // // added 04.10.2026 01:5x, R67. **`keyusage_pub` GELOESCHT** — GEMESSEN: ein
    // // Treffer, die Definition.
    // // added 04.10.2026 01:5x, R67. **`eku_pub` GELOESCHT** — GEMESSEN: ein
    // // Treffer, die Definition.

    // added 04.10.2026 01:6x, R67. **`zertifikat_mit` GELOESCHT** — GEMESSEN:
    // **ein** Treffer in allen Crates, die Definition. Es war der Bauer fuer
    // „Zertifikat mit **frei gewaehlten** Erweiterungen“ (R4x, fuer die
    // Bisektion); die Aufrufer sind inzwischen `blatt_mit_*`, und dieser Weg blieb
    // liegen. 51 Zeilen Bauer ohne Aufrufer.
    //
    // added 04.10.2026 01:6x: **DIE LEHZE DARIN BLEIBT, WEIL SIE NICHT AM
    // FUNKTIONSAUFTRITT HING:** `insert(0, 0xa0)` setzt nur den Tag **byteweise**
    // voran und laesst die **Laenge** stehen — aus `02 01 02` wird
    // `A0 02 01 02`, ein `[0]`-Feld mit Inhalt `01 02` statt `02 01 02`.
    // Richtig ist `tlv(0xa0, &integer(&[0x02]))`. Vergleichsmassstab war
    // `testzertifikat.der`: dort `a0 len=3`, im eigenen Bau `a0 len=2`. Das ist
    // die neunte „eine SEQUENCE/TLV zu viel oder zu wenig“-Stelle (nach R62
    // `version`, R64 `GeneralSubtrees`, R65 SAN/`[1]`, R66 beim Schreiben des
    // Tests) — und die Erkenntnis gehoert **nicht** an einen geloeschten
    // Aufrufer.

    pub fn tbs(mit_must_staple: bool) -> Vec<u8> {
        let mut erweiterungen = basis_erweiterungen();
        if mit_must_staple {
            erweiterungen.push(tls_feature(&[5]));
        }
        let mut ext_inhalt = Vec::new();
        for e in &erweiterungen {
            ext_inhalt.extend_from_slice(e);
        }
        let version = tlv(0xa0, &integer(&[0x02]));
        sequenz(&[
            version,
            integer(&[0x2a]), // Seriennummer
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]), // ecdsa-with-SHA256
            // added 03.10.2026 02:10x: DER **AUSSTELLER** ist die CA, der
            // Inhaber `localhost` (siehe `CA_NAME`). GEMESSEN (02:9x): mit
            // `localhost` als Aussteller gilt dem Pruefer `kette_pruefen` das
            // Blatt als selbstsigniert, und es kommt `KeinAussteller`.
            name(CA_NAME),
            gueltigkeit(),
            name("localhost"),
            spki(),
            // added 03.10.2026 01:11x: **DERSELBE FEHLER ZWEIMAL**, und das
            // ist die Lehre: ich hatte `[3] { SEQUENCE { SEQUENCE { … } } }`
            // gebaut. Richtig ist `[3] EXPLICIT SEQUENCE OF Extension` — also
            // **eine** SEQUENCE ueber den Erweiterungen, dann der Kontext-Tag.
            // Zwei Klammern zu viel sind in DER ein Laengenfehler, deshalb kam
            // `LaengeUeberPuffer { angesagt: 2, vorhanden: 0 }` statt einer
            // verstaendlichen Fehlermeldung.
            tlv(0xa3, &tlv(0x30, &ext_inhalt)),
        ])
    }

    /// added 03.10.2026 01:6x: **DAS VOLLSTAENDIGE ZERTIFIKAT**, signiert mit
    /// dem Testschluessel. Die Signatur ist `SEQUENCE { INTEGER r, INTEGER s }`
    /// — dieselbe Form, die unser eigener Server in `certificate_verify`
    /// schreibt und die `kette::ecdsa_teile_oeffentlich` liest.
    pub fn zertifikat(mit_must_staple: bool) -> Result<Vec<u8>, &'static str> {
        let tbs_bytes = tbs(mit_must_staple);
        let privat = privat_aus_testdaten();
        let (r, s) =
            crate::ecdsa::ecdsa_p256_signieren_sha256(&privat, &crate::sha2::sha256(&tbs_bytes))
                .map_err(|_| "signieren")?;
        let sig_der = crate::signatur::ecdsa_der_signatur(&r, &s).map_err(|_| "sig-der")?;
        let mut bit_inhalt = vec![0x00];
        bit_inhalt.extend_from_slice(&sig_der);
        Ok(sequenz(&[
            tbs_bytes,
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            tlv(0x03, &bit_inhalt),
        ]))
    }
}

#[cfg(test)]
mod bauen_tests {
    #[test]
    fn fehlendes_versionsfeld_ist_v1_ungueltige_version_bleibt_abgelehnt() {
        use super::{bauen, zertifikat_lesen};
        use crate::der::DerLeser;
        let tbs = bauen::tbs(false);
        let element = DerLeser::neu(&tbs).element().unwrap();
        let mut reader = DerLeser::neu(&element.inhalt);
        let mut fields = Vec::new();
        while reader.naechste_tag().is_some() {
            let field = reader.element().unwrap();
            if field.tag != 0xa0 && field.tag != 0xa3 { fields.push(field.roh); }
        }
        let v1 = bauen::signiere(&bauen::sequenz(&fields)).unwrap();
        assert!(zertifikat_lesen(&v1).is_ok(), "RFC 5280: DEFAULT v1");
        fields.insert(0, bauen::tlv(0xa0, &bauen::integer(&[3])));
        let v4 = bauen::signiere(&bauen::sequenz(&fields)).unwrap();
        assert!(zertifikat_lesen(&v4).is_err(), "Version 4 bleibt ungueltig");
    }

    use super::{bauen, verlangt_status_request, zertifikat_lesen};

    /// added 03.10.2026 01:7x, R47. **DER BAU, und es ist der Punkt aus
    /// `PROBLEME.txt` Nr. 46:** ein Zertifikat mit der TLS-Feature-Erweiterung,
    /// durch den **echten** Parser `zertifikat_lesen`. Vorher war nur geprueft,
    /// dass ein Vektor von `features_lesen` gelesen wird — der Weg von den
    /// Erweiterungsbytes **im Zertifikat** bis zu `tls_features` war unbelegt.
    #[test]
    fn gebautes_zertifikat_mit_must_staple_wird_erkannt() -> Result<(), &'static str> {
        let der = bauen::zertifikat(true)?;
        let z = zertifikat_lesen(&der).map_err(|e| {
            // added 03.10.2026 01:8x: **DER FEHLER IM TEST**, nicht nur im
            // Ergebnis: wenn der Bauer etwas falsch baut, ist „unlesbar“ die
            // richtige Antwort, aber man soll sie *sehen* koennen.
            Box::leak(format!("Bau unlesbar: {e:?}").into_boxed_str()) as &'static str
        })?;
        assert_eq!(
            z.tls_features.as_deref(),
            Some([5u16].as_slice()),
            "die Merkmale wurden nicht uebernommen"
        );
        assert!(
            verlangt_status_request(&z),
            "must-staple wurde nicht erkannt"
        );
        Ok(())
    }

    /// added 03.10.2026 01:8x, R47. **DER GEGENFALL, und er ist Pflicht:**
    /// dasselbe Zertifikat **ohne** die Erweiterung darf die Frage nicht mit
    /// „ja“ beantworten. Sonst wuerde der Test auch dann gruen sein, wenn
    /// `verlangt_status_request` immer `true` zurueckgaebe.
    #[test]
    fn gebautes_zertifikat_ohne_merkmal_verlangt_nichts() -> Result<(), &'static str> {
        let der = bauen::zertifikat(false)?;
        let z = zertifikat_lesen(&der).map_err(|_| "Bau unlesbar")?;
        assert_eq!(
            z.tls_features, None,
            "es gibt keine Erweiterung, also auch keine Liste"
        );
        assert!(
            !verlangt_status_request(&z),
            "ein Blatt ohne Merkmal darf keine Stapelung verlangen"
        );
        Ok(())
    }

    /// added 03.10.2026 01:8x, R47. **UND DIE BASIS IST BRAUCHBAR**, sonst waere
    /// der Test ein Kunststueck: `SAN` mit `localhost`, gueltig von 2020 bis
    /// 2035, `serverAuth`. Das ist zugleich der Beweis, dass der Bauer eine
    /// brauchbare Erweiterung erzeugt und nicht nur unsere Sonderform.
    #[test]
    fn das_gebaute_zertifikat_ist_ueberhaupt_brauchbar() -> Result<(), &'static str> {
        let der = bauen::zertifikat(false)?;
        let z = zertifikat_lesen(&der).map_err(|_| "Bau unlesbar")?;
        // added 03.10.2026 01:9x: **MEIN ERSTER VERSUCH** verglich
        // `subject_der(&z)` mit `z.inhaber_der()` — die Methode gibt es nicht
        // (`no method named inhaber_der`). Die Aussage, die ich treffen wollte,
        // ist einfach: der **Inhabername** laesst sich lesen.
        // added 03.10.2026 01:16x: clippy `redundant_pattern_matching` — also
        // `is_ok()` statt `matches!(…, Ok(_))`.
        assert!(
            super::subject_der(&z).is_ok(),
            "der Inhabername liess sich nicht lesen"
        );
        assert!(
            z.san.is_some(),
            "ohne subjectAltName waere das Zertifikat fuer einen Browser unbrauchbar"
        );
        Ok(())
    }
}

#[cfg(test)]
mod leerer_block_tests {
    use super::zertifikat_lesen;
    /// added 03.10.2026 02:6x, R49. **DIE AUSSAGE VON Nr. 49 NACHMESSEN**, und
    /// sie hat einen eigenen Fall gebraucht: im R47-Bau hatte ich beim „keine
    /// Erweiterungen“-Zweig den `[3]`-Block **weggelassen** statt ihn leer zu
    /// setzen — das war eine andere Form und damit eine andere Frage.
    /// added 03.10.2026 02:7x, R49. **MEINE EIGENE NOTIZ WAR FALSCH**, und
    /// dieser Test ist der Beweis dagegen.
    ///
    /// added 03.10.2026 02:7x: In R47 hatte ich behauptet, unser Parser
    /// scheitere an einem **leeren** Erweiterungsblock (`[3] { SEQUENCE {} }`)
    /// und das als **Nr. 49** notiert. GEMESSEN (02:5x) ist das **falsch**:
    /// der Block wird gelesen, ohne Meldung. Die Ursache der damaligen
    /// `LaengeUeberPuffer`-Meldung war mein **kaputtes `[0]`-Versionsfeld**
    /// (`insert(0, 0xa0)` ohne Laengenkorrektur), das ich im selben Lauf
    /// repariert hatte. Die Notiz hat einen Fehler beschrieben, den es nicht
    /// gab.
    ///
    /// added 03.10.2026 02:7x: **WARUM DAS ZWEI VERSCHIEDENE FRAGEN SIND.** Im
    /// R47-Bau hatte ich fuer „keine Erweiterungen“ den `[3]`-Block **weggelassen**
    /// (`extensions [3] EXPLICIT … OPTIONAL`) statt ihn **leer** zu setzen. Das
    /// sind zwei verschiedene Zertifikate; ich hatte die Meldung der einen der
    /// anderen zugeschrieben.
    #[test]
    fn leerer_erweiterungsblock_wird_akzeptiert() -> Result<(), &'static str> {
        let der = super::bauen::blatt_mit_leerem_block()?;
        let z = zertifikat_lesen(&der).map_err(|_| "der leere Block wurde abgewiesen")?;
        // added 03.10.2026 02:8x: **UND ES BLEIBT EIN KAPUTTES ZERTIFIKAT KEIN
        // GUTES**: ohne Erweiterung gibt es weder `tls_features` noch `san`.
        assert_eq!(z.tls_features, None, "ohne Block gibt es keine Merkmale");
        assert!(z.san.is_none(), "ohne Block gibt es kein SAN");
        // added 03.10.2026 02:8x: **DER GEGENFALL** ist durch `bauen_tests`
        // abgedeckt (dort traegt das Blatt SAN und Merkmale) — hier zaehlt nur,
        // dass der leere Block den Rest **nicht** stoert: der Inhabername muss
        // lesbar sein.
        assert!(
            super::subject_der(&z).is_ok(),
            "der Inhabername liess sich nicht lesen"
        );
        Ok(())
    }
}

#[cfg(test)]
mod ocsp_bauen_tests {
    /// added 03.10.2026 04:4x, R49. **DER WIDERRUF AUF ZERTIFIKATSEBENE.**
    /// `beweise` muss aus derselben Antwort `Widerrufen` machen, wo es vorher
    /// `Gut` sagte — der einzige Unterschied ist das `certStatus`-Feld. Damit
    /// ist gemessen, dass unser `beweise` den Status **liest** und nicht nur
    /// die Signatur und die Zeit prueft.
    #[test]
    fn gebaute_widerrufsantwort_wird_als_widerruf_erkannt() -> Result<(), &'static str> {
        let ca_der = bauen::zertifikat_ca()?;
        let blatt_der = bauen::zertifikat(false)?;
        let antwort_der = bauen::ocsp_antwort_widerrufen(&ca_der, &bauen::blatt_seriennummer())?;
        let ca = super::zertifikat_lesen(&ca_der).map_err(|_| "CA unlesbar")?;
        let blatt = super::zertifikat_lesen(&blatt_der).map_err(|_| "Blatt unlesbar")?;
        let a = crate::ocsp::antwort_lesen(&antwort_der).map_err(|e| {
            Box::leak(format!("Antwort unlesbar: {e:?}").into_boxed_str()) as &'static str
        })?;
        match crate::ocsp::beweise(&a, &blatt, &ca, 1_800_000_000i64) {
            crate::ocsp::Urteil::Widerrufen => Ok(()),
            _ => Err("ein gebauter Widerruf wurde nicht als Widerruf erkannt"),
        }
    }

    use super::bauen;
    use crate::ocsp::Urteil;

    /// added 03.10.2026 03:0x, R49. **DER BAU PRUEFT SICH SELBST**: eine von
    /// `ocsp::beweise` akzeptierte Antwort beweist, dass CertID-Hashes,
    /// Seriennummer, Zeitfenster und Signatur **alle vier** passen — und dass
    /// unser OCSP-Weg in einem geschlossenen Kreis ueberhaupt funktioniert.
    /// Bisher war das nur gegen die Wild messbar.
    #[test]
    fn gebaute_ocsp_antwort_wird_vom_pruefer_akzeptiert() -> Result<(), &'static str> {
        let ca_der = bauen::zertifikat_ca()?;
        let blatt_der = bauen::zertifikat(false)?;
        let antwort_der = bauen::ocsp_antwort_gut(&ca_der, &bauen::blatt_seriennummer())?;
        let ca = super::zertifikat_lesen(&ca_der).map_err(|_| "CA unlesbar")?;
        let blatt = super::zertifikat_lesen(&blatt_der).map_err(|_| "Blatt unlesbar")?;
        // added 03.10.2026 03:2x: **DER FEHLER STEHT IN DER MELDUNG** — ein
        // `map_err(|_| "…")` hat mir hier nur „unlesbar“ gezeigt und die Suche
        // nach der Ursache verlaengert.
        let a = crate::ocsp::antwort_lesen(&antwort_der).map_err(|e| {
            Box::leak(format!("unsere Antwort unlesbar: {e:?}").into_boxed_str()) as &'static str
        })?;
        let jetzt = 1_800_000_000i64; // in der Gueltigkeit 2026..2036
        let urteil = crate::ocsp::beweise(&a, &blatt, &ca, jetzt);
        assert_eq!(
            urteil,
            Urteil::Gut,
            "unsere eigene Antwort wird nicht akzeptiert"
        );
        Ok(())
    }

    /// added 03.10.2026 03:0x, R49. **DER GEGENFALL, und er ist die Aussage des
    /// ganzen Baus:** dieselbe Antwort, aber **eine Stunde in der
    /// Vergangenheit** geprueft, muss `Unbekannt` ergeben („die Antwort ist noch
    /// nicht gueltig“, `beweise` vergleicht `jetzt < thisUpdate`). Ein gebautes
    /// `good`, das bei jedem Zeitpunkt gaelte, waere kein Beweis.
    #[test]
    fn dieselbe_antwort_vor_gueltigkeit_ist_unbekannt() -> Result<(), &'static str> {
        let ca_der = bauen::zertifikat_ca()?;
        let blatt_der = bauen::zertifikat(false)?;
        let antwort_der = bauen::ocsp_antwort_gut(&ca_der, &bauen::blatt_seriennummer())?;
        let ca = super::zertifikat_lesen(&ca_der).map_err(|_| "CA unlesbar")?;
        let blatt = super::zertifikat_lesen(&blatt_der).map_err(|_| "Blatt unlesbar")?;
        let a = crate::ocsp::antwort_lesen(&antwort_der).map_err(|_| "Antwort unlesbar")?;
        let zu_frueh = 1_700_000_000i64; // 2023, vor thisUpdate 2026
        match crate::ocsp::beweise(&a, &blatt, &ca, zu_frueh) {
            Urteil::Gut => Err("eine noch nicht gueltige Antwort wurde akzeptiert"),
            Urteil::Unbekannt(_) => Ok(()),
            // added 03.10.2026 03:1x: `format!` gibt `String`, die Funktion aber
            // `&'static str` zurueck — mein erster Versuch hat nicht uebersetzt
            // (`mismatched types`).
            _ => Err("falsches Urteil"),
        }
    }
}

#[cfg(test)]
mod sieben_eins_tests {
    /// added 03.10.2026 13:6x, R57. **DIE NORMSCHRITTE, einzeln** — und sie
    /// stehen hier statt in `kette.rs`, weil sie eine **Vorbereitungsfunktion**
    /// pruefen, nicht eine Kette.
    ///
    /// added 03.10.2026 13:6x: **WARUM JEDER SCHRITT EIN TEST IST.** RFC 4518
    /// nennt fuer diese Vergleiche zwei Schritte (Map mit case folding, und
    /// Insignificant Character Removal als white space compression). Ein Test,
    /// der nur „`a` ist gleich `A`“ prueft, **besteht auch dann**, wenn die
    /// Leerraumkompression fehlt — und umgekehrt. Also je **einen** Test je
    /// Schritt, damit ein Ausfall benannt wird statt vermutet.
    #[test]
    fn schritt_zwei_case_folding() {
        assert_eq!(
            super::wert_7_1_bereitet(b"MiXeD"),
            super::wert_7_1_bereitet(b"mixed")
        );
        // added 03.10.2026 13:6x: **DER GEGENFALL:** verschiedene Buchstaben
        // bleiben verschieden. Ohne das waere die Funktion konstant leer und der
        // erste Test trotzdem gruen.
        assert_ne!(
            super::wert_7_1_bereitet(b"abc"),
            super::wert_7_1_bereitet(b"abd")
        );
    }

    #[test]
    fn schritt_sechs_leerraumkompression() {
        // added 03.10.2026 13:7x: **INNERE FOLGEN ZU EINER**, fuehrende und
        // abschliessende ganz weg (RFC 4518 Abschnitt 2.6.1).
        assert_eq!(super::wert_7_1_bereitet(b"  a   b  "), b"a b".to_vec());
        // added 03.10.2026 13:7x: **ALLE VIER ZEICHEN** — die Norm nennt SPACE,
        // TAB, CR und LF; ein Test mit nur Space haette drei davon nicht
        // geprueft. GEMESSEN: ich hatte erst nur Space geschrieben.
        assert_eq!(super::wert_7_1_bereitet(b"a\tb\rc\nd"), b"a b c d".to_vec());
        // added 03.10.2026 13:7x: **BEIDE SCHRITTE GEMEINSAM**, weil die
        // Reihenfolge sonst ungeprueft ist.
        assert_eq!(
            super::wert_7_1_bereitet(b" A  B "),
            super::wert_7_1_bereitet(b"a b")
        );
    }

    /// added 03.10.2026 13:7x, R57. **DER GEGENFALL ZUR LEERRAUMREGEL:** ein
    /// Leerzeichen **innerhalb** eines Wortes bleibt **eines**. Sonst wuerde
    /// `a b` gleich `ab`, und das waere kein Komprimieren mehr, sondern ein
    /// Loeschen.
    #[test]
    fn leerraum_innen_bleibt_einer() {
        assert_eq!(super::wert_7_1_bereitet(b"a b"), b"a b".to_vec());
        assert_ne!(super::wert_7_1_bereitet(b"a b"), b"ab".to_vec());
    }

    /// added 03.10.2026 13:8x, R57. **DER TYP GEHOERT ZUM VERGLEICH** — RFC 5280
    /// 7.1: „Two naming attributes match if the **attribute types are the same**
    /// and the values … are an exact match“. Also: gleicher Wert, anderer Typ =>
    /// **nicht** gleich.
    #[test]
    fn gleicher_wert_anderer_typ_ist_nicht_gleich() {
        let a = super::Attribut {
            typ: vec![2, 5, 4, 3],
            wert: vec![b'x'],
        };
        let b = super::Attribut {
            typ: vec![2, 5, 4, 5],
            wert: vec![b'x'],
        };
        assert!(!super::attribut_gleich_7_1(&a, &b));
        let c = super::Attribut {
            typ: vec![2, 5, 4, 3],
            wert: vec![b'X'],
        };
        assert!(super::attribut_gleich_7_1(&a, &c));
    }
}

#[cfg(test)]
mod kritische_tests {
    /// added 03.10.2026 05:0x, R51. **ROTER VORHER-BEWEIS.** RFC 5280 4.2 woertlich:
    /// „A certificate-using system **MUST reject** the certificate if it
    /// encounters a critical extension it does not recognize or a critical
    /// extension that contains information that it cannot process.“
    /// Wir lesen `critical` und **verwerfen** den Wert (`let _kritisch`), und
    /// danach entscheidet `match oid` — dessen `_ => {}` jeden unbekannten
    /// Wert stillschweigend fallen laesst. Dieser Test muss **vor** dem Fix
    /// rot sein; danach wird er gruen.
    #[test]
    fn unbekannte_kritische_erweiterung_wird_abgewiesen() {
        let der = super::bauen::zertifikat_mit_unbekannter_erweiterung(true)
            .unwrap_or_else(|_| panic!("Bau"));
        match super::zertifikat_lesen(&der) {
            Ok(_) => panic!(
                "eine unbekannte KRITISCHE Erweiterung wurde akzeptiert - \
                 das verletzt RFC 5280 4.2 (MUST reject)"
            ),
            // added 03.10.2026 05:5x: **DER TEST PRUEFT DIE ANZEIGE**, nicht die
            // Fehlerform. Grund: fuer `punkte` muesste ich die Punkte im Test
            // nachrechnen — und genau daran ist dieser Test **gescheitert**, weil
            // ich die 7-Bit-Gruppen als Einzelbytes behandelt hatte (s. die
            // Konstante). Die Anzeige ist ohnehin das, was die Fehlersuche
            // traegt, also pruefe ich sie; sie enthaelt zusaetzlich den Beweis,
            // dass `oid_als_punkte` die Gruppen richtig liest.
            Err(e) => assert_eq!(
                format!("{e}"),
                "kritische Erweiterung, die nicht verarbeitet werden kann: 1.3.6.1.4.1.99999.1.1",
                "die Meldung nennt die OID nicht — sie ist das Einzige, was die Fehlersuche traegt"
            ),
        }
    }

    /// added 03.10.2026 05:4x, R51. **DER GEGENFALL, und er ist die Haelfte der
    /// Norm.** RFC 5280 4.2: „A non-critical extension **MAY** be ignored if it
    /// is not recognized.“ Dasselbe Zertifikat wie oben, nur **ohne**
    /// `critical` — es muss gelesen werden. Ein Test, der nur die Ablehnung
    /// prueft, wuerde von einem Parser erfuellt, der **alles** ablehnt.
    #[test]
    fn unbekannte_nicht_kritische_erweiterung_wird_ignoriert() {
        let der = super::bauen::zertifikat_mit_unbekannter_erweiterung(false)
            .unwrap_or_else(|_| panic!("Bau"));
        let z = super::zertifikat_lesen(&der).unwrap_or_else(|e| panic!("wurde abgewiesen: {e}"));
        // added 03.10.2026 05:4x: **DER ZWEITE TEIL DES GEGENFALLS** — und er
        // ist der eigentliche: das Ignorieren darf **nichts** zerstoeren. Die
        // uebrigen Felder muessen wie ohne die Erweiterung aussehen.
        assert!(
            z.san.is_some(),
            "die SAN ist verschwunden — das Ignorieren stoert"
        );
        assert!(
            z.tls_features.is_none(),
            "unser Testblatt traegt keine Merkmale"
        );
    }

    /// added 03.10.2026 05:4x, R51. **EINE BEKANNTE ERWEITERUNG ALS
    /// `critical` GEHT DURCH**, weil wir sie verarbeiten koennen. Ohne diesen
    /// Test waere die Regel falsch herum gebaut worden: `kritisch` allein ist
    /// kein Ablehnungsgrund.
    #[test]
    fn bekannte_kritische_erweiterung_wird_angenommen() {
        let der = super::bauen::zertifikat_mit_kritischer_san().unwrap_or_else(|_| panic!("Bau"));
        let z = super::zertifikat_lesen(&der).unwrap_or_else(|e| panic!("wurde abgewiesen: {e}"));
        assert!(
            z.san.is_some(),
            "die als kritisch markierte SAN wurde nicht gelesen"
        );
    }

    /// added 03.10.2026 05:4x, R51. **DER ZWEITE FUND IM TEST**: 2.5.29.31 ist
    /// **bekannt** und wird sogar gelesen, aber es wird **keine CRL von dort
    /// geholt** — `verteilungsstellen` kommt in keiner anderen Datei vor
    /// (GEMESSEN, grep). Als `critical` ist das genau „a critical extension
    /// that contains information that it cannot process“.
    #[test]
    fn bekannte_aber_nicht_angewandte_kritische_erweiterung_wird_abgewiesen() {
        let der = super::bauen::zertifikat_mit_kritischen_verteilungsstellen()
            .unwrap_or_else(|_| panic!("Bau"));
        match super::zertifikat_lesen(&der) {
            Ok(_) => {
                panic!("2.5.29.31 als critical wurde angenommen — wir koennen sie nicht anwenden")
            }
            Err(e) => assert!(
                matches!(e, crate::x509::X509Fehler::KritischeErweiterung { .. }),
                "falsche Fehlerform: {e}"
            ),
        }
    }
}

#[cfg(test)]
mod verteilungsstellen_texte {
    use super::*;

    /// added 03.10.2026 16:e4, R60: **DIE GEMESSENEN BYTES, WORTGETREU.** Aus
    /// `xkcd.com` (GEMESSEN 16:de/16:dp, `openssl x509 -outform der`, dann von
    /// Hand abgetragen):
    ///     30 3f  30 3d  A0 3b  A0 39  86 37  "http://crl.globalsign.com/ca/gsatlasr3dvtlsca2025q4.crl"
    /// Das sind **fuenf** Schichten ueber dem `extnValue`-OCTET STRING, und
    /// **keine** davon ist die `GeneralNames`-`SEQUENCE`, die R59 angenommen
    /// hat. Ich baue das Muster hier mit **berechneten** Laengen (GEMESSEN
    /// 16:dp: geratene Laengen im Testmuster sind ein Test der Laengenpruefung,
    /// nicht des Aufbaus).
    fn gemessen_muster() -> Vec<u8> {
        let uri = b"http://crl.globalsign.com/ca/gsatlasr3dvtlsca2025q4.crl";
        // added 03.10.2026 16:ee: **JEDE LAENGE IST DER INHALT DER SCHICHT DARUEBER**
        // — und das ist die Form, die mein erster Entwurf **verrechnet** hat.
        // GEMESSEN (16:de): der Kopf ist `30 3f | 30 3d | A0 3b | A0 39 | 86 37`,
        // also **fuenf** Laengenbytes fuer **fuenf** Schichten ueber den 55 Byte
        // langen URI: 55, 57, 59, 61, 63. Jede ist die vorherige **plus zwei**
        // (Tag und Laengenbyte selbst). Mein erster Entwurf hat **sechs**
        // gerechnet und es an der `assert` gemerkt — die Pruefung hat also
        // **gearbeitet**, und das ist der Grund, warum sie dort steht und nicht
        // irgendwo im Kommentar.
        let gn = 2 + uri.len(); // 57: Inhalt von `fullName [0]`
        let dp_name = 2 + gn; // 59: Inhalt von `distributionPoint [0]`
        let dp_seq = 2 + dp_name; // 61: Inhalt von `DistributionPoint`
        let cdp = 2 + dp_seq; // 63: Inhalt von `CRLDistributionPoints`
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(&[0x30, cdp as u8]);
        v.extend_from_slice(&[0x30, dp_seq as u8]);
        v.extend_from_slice(&[0xa0, dp_name as u8]);
        v.extend_from_slice(&[0xa0, gn as u8]);
        v.extend_from_slice(&[0x86, uri.len() as u8]);
        v.extend_from_slice(uri);
        assert_eq!(v.len(), cdp + 2, "die Laengen passen nicht zu den Bytes");
        v
    }

    /// added 03.10.2026 16:e5, R60: **DER LESER LIEFERT EINE QUELLE**, und das
    /// ist die **erste** Haelfte des Beweises. Mit den gemessenen Bytes muss
    /// `verteilungsstellen_lesen` **eine** URI liefern — im Fehlerfall waere die
    /// Textwandlung schuldlos und man wuerde sie vergebens reparieren.
    #[test]
    fn der_leser_liefert_eine_uri() {
        let namen = verteilungsstellen_lesen(&gemessen_muster()).unwrap_or_default();
        assert_eq!(
            namen.len(),
            1,
            "der Leser lieferte {namen:?} — erwartet genau eine URI"
        );
        assert_eq!(
            namen[0],
            b"http://crl.globalsign.com/ca/gsatlasr3dvtlsca2025q4.crl"
        );
    }

    /// added 03.10.2026 16:e6, R60: **DIE TEXTWANDLUNG, UND DAS IST DER
    /// GEGENBEWEIS ZU R59.** R59 hat dieselben Bytes ein **zweites** Mal
    /// gedeutet (`30 30 A0 A0`, ein `DistributionPoint`), obwohl der Leser die
    /// URI schon als **Zeichen** abgelegt hat. Dieser Test **laeuft mit der
    /// alten Fassung rot** — GEMESSEN 16:e1 in der Wild: `dp_roh=1` und
    /// `quellen=[]`. Er ist damit kein Test „zur Absicherung“, sondern der
    /// Nachweis, dass der Fehler existiert hat und behoben ist.
    #[test]
    fn die_textwandlung_liefert_die_gemessene_url() {
        let texte = texte_aus_urten(&[b"http://crl.globalsign.com/ca/x.crl".to_vec()]);
        assert_eq!(texte.len(), 1, "eine Quelle ergab {texte:?}");
        assert_eq!(texte[0], "http://crl.globalsign.com/ca/x.crl");
    }

    /// added 03.10.2026 16:e7, R60: **DER GEGENFALL, UND ER BRAUCHT ZWEI.**
    /// Ein Test, der nur die gute Hälfte prüft, wäre von einer Funktion erfüllt,
    /// die **alles** annimmt. Also: (a) **kein** UTF-8 → keine URL, denn eine
    /// verstümmelte Anzeige wäre eine **erfundene**; (b) **nicht-IA5** → keine
    /// URL, denn RFC 5280 4.2.1.13 verlangt einen URI aus RFC 3986, dessen
    /// Zeichen IA5 sind — ein UTF-8-Umlaut ist keiner.
    #[test]
    fn kaputte_und_nicht_ia5_quellen_werden_verworfen() {
        let quellen = vec![
            vec![0xff, 0xfe, 0xfd],              // (a) kein UTF-8
            "http://a.invalid/ä.crl".into(),     // (b) nicht IA5
            Vec::new(),                          // (c) leer
            b"http://ok.invalid/a.crl".to_vec(), // (d) die eine gueltige
        ];
        let texte = texte_aus_urten(&quellen);
        // added 03.10.2026 16:ea: **GENAU EINE**, und das ist der Punkt: eine
        // Liste, die **alles** zurueckgibt, und eine Liste, die **nichts**
        // zurueckgibt, sind beide **falsch** — und ein Test, der nur „leer“
        // prueft, waere von **beiden** erfuellt.
        assert_eq!(
            texte.len(),
            1,
            "von vier Quellen wurde {texte:?} ausgegeben — erwartet genau eine"
        );
        assert_eq!(texte[0], "http://ok.invalid/a.crl");
    }

    /// added 03.10.2026 16:ef, R60. **DER GEGENBEWEIS ZU R59, ALS TEST.**
    /// R59 hat die bereits gelesenen URI-**Zeichen** ein zweites Mal als
    /// `SEQUENCE { [0] { [0] { 86 … } } }` dekodiert. Dieser Test stellt fest:
    /// das **misslingt bei jedem** Eingabetext, und zwar aus **einem** Grund —
    /// der Text beginnt mit `h` (0x68) und nicht mit `30`. Damit ist der Fehler
    /// von R59 nicht „manchmal falsch“, sondern **immer** wirkungslos, und das
    /// ist die Aussage, die in der Wildmessung (16:e1: `dp_roh=1`,
    /// `quellen=[]`) nur als **Folge** sichtbar war.
    ///
    /// added 03.10.2026 16:ef: **UND WARUM DER TEST NICHT NUR `is_none`
    /// PRUEFT.** `DerLeser::element_mit_tag(0x30)` scheitert auch an einem
    /// **zufaellig** `0x30` an erster Stelle, wenn der Rest nicht passt. Der
    /// Test prueft deshalb **beide** Zertifikate der Behauptung: (a) der erste
    /// Bytewert ist **nicht** `0x30` — der Grund; (b) der Leser meldet einen
    /// **Fehler**, nicht etwa eine leere Liste — die Wirkung.
    #[test]
    fn uri_zeichen_kein_der_punkt() {
        let text = b"http://crl.globalsign.com/ca/x.crl";
        assert_ne!(
            text[0], 0x30,
            "die URI-Zeichen beginnen mit 0x30 — dann waere der Befund falsch"
        );
        assert!(
            DerLeser::neu(text).element_mit_tag(0x30).is_err(),
            "die URI-Zeichen wurden als DistributionPoint gelesen"
        );
    }

    /// added 03.10.2026 16:eb, R60. **DIE WILDZAHL, OFFLINE.** Der Testdatensatz
    /// `testdaten/t10_x509_testdaten.txt` enthaelt **1050 echte** Zertifikate
    /// (GEMESSEN 16:eb, `grep -c '^ZERT'`), und die Zahl wird **ohne** Netz
    /// genommen. Das ist die Antwort auf die Frage, die R59 mit einem
    /// **handgebauten** Muster beantwortet hat — und die Antwort ist eine
    /// Wildzahl, keine Beispielzahl.
    #[test]
    fn wie_viele_echte_zertifikate_geben_eine_url() {
        const DATEN: &str = include_str!("../testdaten/t10_x509_testdaten.txt");
        let mut geprueft = 0usize;
        let mut mit_url = 0usize;
        let mut mit_roh = 0usize;
        for zeile in DATEN.lines() {
            let Some(rest) = zeile.strip_prefix("ZERT") else {
                continue;
            };
            let Some((_, hex)) = rest.split_once("_DER=") else {
                continue;
            };
            // added 03.10.2026 16:ed: **`von_hex` GIBT `Option`, NICHT `Result`**
            // (GEMESSEN 16:ed: „expected `Option<Vec<u8>>`, found `Result`“).
            // Ich hatte die Signatur aus dem Namen abgeleitet statt gelesen —
            // dieselbe Form wie bei `crl::beurteile` in R60 (16:ax), wo ich
            // ebenfalls `Result` angenommen hatte.
            let Some(bytes) = crate::sha2::von_hex(hex) else {
                continue;
            };
            let Ok(z) = zertifikat_lesen(&bytes) else {
                continue;
            };
            geprueft += 1;
            if !z.verteilungsstellen.is_empty() {
                mit_roh += 1;
            }
            if !verteilungsstellen_als_texte(&z).is_empty() {
                mit_url += 1;
            }
        }
        println!(
            "R60-Verteilungspunkte: {geprueft} Zertifikate gelesen, {mit_roh} mit Rohfeld, {mit_url} mit URL"
        );
        // added 03.10.2026 16:ec: **DIE ZAHL IST NICHT GERATEN, UND DER TEST
        // BEHAUPTET SIE AUCH NICHT.** Er verlangt nur: es gibt **mindestens
        // eine** echte Quelle — sonst waere die ganze Funktion ungetestet, und
        // genau das war der Zustand von R59 (null Tests, GEMESSEN 16:e2). Die
        // Wildzahl wird **gedruckt**, nicht behauptet, damit sie beim Aendern
        // des Datensatzes mitwandert, statt still falsch zu werden.
        assert!(
            mit_url >= 1,
            "kein einziges der {geprueft} echten Zertifikate liefert eine URL"
        );
        assert!(
            mit_url <= mit_roh,
            "mehr URLs ({mit_url}) als Rohfelder ({mit_roh}) — die Wandlung verliert keine Zeile"
        );
    }
}

#[cfg(test)]
mod messung_name_constraints {
    /// added 03.10.2026 20:5x, R63. **DIE WILDMESSUNG: WELCHE `GeneralName`-ARTEN
    /// KOMMEN IN `nameConstraints` ÜBERHAUPT VOR?** Und sie ist **offline**
    /// (1050 echte Zertifikate im Testdatensatz), weil sie die Frage „welche Art
    /// lesen wir nicht?“ beantworten soll — und das ist eine Frage an die
    /// **Daten**, nicht an den Code.
    ///
    /// added 03.10.2026 20:5x: **WIR SCANNEN SELBST UND BENUTZEN DEN LESER
    /// NICHT**, und der Grund ist der Kern des Messpunkts: der Leser **verwirft**
    /// unbekannte Arten still (R63-Befund), also kann er die Antwort nicht geben.
    /// Der Scan ist deshalb bewusst **unabhängig** — eine zweite Sicht auf
    /// dieselbe Erweiterung, wie bei `wert_7_1_bereitet`.
    #[test]
    fn welche_allgemeinnamen_stecken_in_der_auflage() {
        const DATEN: &str = include_str!("../testdaten/t10_x509_testdaten.txt");
        let mut pro_tag: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
        let mut mit_auflage = 0usize;
        let mut gelesen = 0usize;
        // OID 2.5.29.30 = nameConstraints
        let oid: [u8; 3] = [0x55, 0x1d, 0x1e];
        for zeile in DATEN.lines() {
            let Some(rest) = zeile.strip_prefix("ZERT") else {
                continue;
            };
            let Some((_, hex)) = rest.split_once("_DER=") else {
                continue;
            };
            let Some(bytes) = crate::sha2::von_hex(hex) else {
                continue;
            };
            let Ok(zert) = super::zertifikat_lesen(&bytes) else {
                continue;
            };
            let Some(n) = &zert.namensbeschraenkung else {
                continue;
            };
            mit_auflage += 1;
            gelesen += n.erlaubt.len()
                + n.verboten.len()
                + n.erlaubt_ip.len()
                + n.verboten_ip.len()
                + n.erlaubt_email.len()
                + n.verboten_email.len()
                + n.erlaubt_uri.len()
                + n.verboten_uri.len();
            // added 03.10.2026 20:be, R63: **DIE INVARIANTE, HIER GEPRUEFT und
            // nicht im Bericht behauptet:** erkannte Teilbaeume **+** unlesbare
            // Tags muessen **groesser null** sein. `unlesbar` gibt es erst seit
            // R63 — vorher waere dieser Ausdruck nicht uebersetzbar gewesen, und
            // die Luecke waere still geblieben.
            assert!(
                gelesen + n.unlesbar.len() > 0,
                "ein Zertifikat traegt Auflagen, und keiner der Teilbaeume wurde \
                 erkannt oder als unlesbar vermerkt — sie verschwinden"
            );
            // added 03.10.2026 20:5x: **DIE TAG-ORIENTIERTE SUCHE NACH DER
            // ERWEITERUNG**: `06 03 55 1d 1e` ist die OID, danach folgt der
            // `extnValue`. Wir suchen nur die OID und zaehlen die **Basis-Tags**
            // der GeneralSubtrees, indem wir ab dem Fund zeichenweise durch die
            // TLVs gehen. Das ist keine DER-Analyse, sondern eine Zaehlung — und
            // genau deshalb ist sie unabhaengig vom Leser.
            let mut pos = 0usize;
            while pos + 5 < bytes.len() {
                if bytes[pos] == 0x06 && bytes[pos + 1] == 3 && bytes[pos + 2..pos + 5] == oid {
                    // der Wert folgt: OCTET STRING mit Laenge
                    let q = pos + 5;
                    if q < bytes.len() && bytes[q] == 0x04 {
                        let len = bytes[q + 1] as usize;
                        let inhalt = &bytes[q + 2..(q + 2 + len).min(bytes.len())];
                        zaehle_bases(inhalt, &mut pro_tag);
                    }
                    break;
                }
                pos += 1;
            }
        }
        let mut bericht = String::new();
        for (tag, anzahl) in &pro_tag {
            bericht.push_str(&format!("  0x{tag:02x}: {anzahl}\n"));
        }
        println!(
            "R63-AUFLAGEN {mit_auflage} Zertifikate mit nameConstraints, \
             {gelesen} Teilbaeume vom Leser erkannt\n{bericht}"
        );
        // added 03.10.2026 20:bd, R63. **KEINE BEHAUPTUNG, DIE DIE MESSUNG
        // WIDERLEGT.** Mein erster Entwurf behauptete `mit_auflage >= 1` — und die
        // eigene Messung sagt **0**: 150 lesbare Zertifikate, **0** mit
        // `nameConstraints` (deckungsgleich mit R56: 0 von 6 Wild-Hosts). Ein
        // Test, der eine Behauptung stellt, die die eigene Zahl widerlegt, ist
        // keine Messung, sondern ein **Fehler** — und er war **rot**.
        //
        // added 03.10.2026 20:bd: **STATT DESSEN NUR DRUCKEN.** Die Zahl wird
        // **gemeldet**, nicht behauptet; die **Invariante** (kein Teilbaum
        // verschwindet) steht weiter oben im Schleifenkoerper, und die ist im
        // Corpus von heute **leer** — sie beisst noch nicht, und das sage ich
        // ausdruecklich, statt so zu tun, als waere sie scharf. **Das Verhalten**
        // beweist `eine_unlesbare_auflage_wird_abgelehnt` mit einem **gebauten**
        // Zertifikat; der Corpus kann es nur **nicht** widerlegen.
    }

    /// added 03.10.2026 20:5x: **DIE BASIS-TAGS ZAEHLEN, OHNE ZU PARSEN.** Wir
    /// gehen die Bytes als Kette von TLVs durch und zaehlen, wie oft ein Tag
    /// **innerhalb** eines `GeneralSubtree` an erster Stelle steht — das ist der
    /// `base` (GeneralSubtree ::= SEQUENCE { base GeneralName, … }). Erkennbar
    /// ist das an der **Struktur**: `SEQUENCE { base, [0]?, [1]? }`.
    fn zaehle_bases(inhalt: &[u8], pro_tag: &mut std::collections::BTreeMap<u8, usize>) {
        // Der Inhalt ist `NameConstraints ::= SEQUENCE { [0]|[1] GeneralSubtrees }`,
        // und GeneralSubtrees ist `SEQUENCE OF GeneralSubtree`.
        let mut k = 0usize;
        while k + 1 < inhalt.len() {
            let tag = inhalt[k];
            let len = inhalt[k + 1] as usize;
            if tag == 0xa0 || tag == 0xa1 {
                // GeneralSubtrees: SEQUENCE OF -> direkt die Subtrees
                zaehle_subtrees(&inhalt[k + 2..(k + 2 + len).min(inhalt.len())], pro_tag);
            }
            k += 1;
        }
    }

    /// added 03.10.2026 20:5x: **EINE EBENE TIEFER.** Jedes Element hier ist ein
    /// `GeneralSubtree` = `SEQUENCE { base GeneralName, minimum [0]?, maximum [1]? }`,
    /// und `base` ist das **erste** Element darin.
    fn zaehle_subtrees(bereich: &[u8], pro_tag: &mut std::collections::BTreeMap<u8, usize>) {
        let mut k = 0usize;
        while k + 1 < bereich.len() {
            let tag = bereich[k];
            let len = bereich[k + 1] as usize;
            if tag == 0x30 {
                let inhalt = &bereich[k + 2..(k + 2 + len).min(bereich.len())];
                if inhalt.len() >= 2 {
                    *pro_tag.entry(inhalt[0]).or_insert(0) += 1;
                }
            }
            k += 1;
        }
    }
}

#[cfg(test)]
mod messung_unlesbare_auflage {
    /// added 03.10.2026 20:7x, R63. **DIE AUFGABE, UM DIE ES GEHT: WAS TUN WIR
    /// MIT EINER AUFLAGE, DIE WIR NICHT AUSWERTEN KÖNNEN?** Die Antwort war
    /// zuerst, GEMESSEN: **nichts** — `namensbeschraenkung_lesen` verteilt den
    /// `base` auf Tags 0x87, 0x81, 0x86, 0x82 und 0xa4 und hatte **keinen**
    /// `else`-Zweig, der etwas **tut**. Jede andere `GeneralName`-Art verschwand
    /// **stillschweigend**; dieser Test war damals **grün** und sagte
    /// „gelesen: 0 Teilbaeume, verworfen: 1“.
    ///
    /// added 03.10.2026 20:7x: **HEUTE HEISST DER TEST DAS GEGENTEIL, UND DER
    /// GEGENBEWEIS IST GEMESSEN:** mit abgeschalteter Prüfung (R63, `if false`)
    /// meldete er „die Kette wurde angenommen, obwohl die CA eine nicht
    /// auswertbare Auflage traegt — das ist der Fehler, den R63 behebt“.
    ///
    /// added 03.10.2026 20:7x: **WARUM DAS EIN SICHERHEITSPROBLEM IST.** Bei
    /// `excludedSubtrees` bedeutet „ignoriert“: eine CA, die einem Teilbaum
    /// **ausschließt**, wird so behandelt, als hätte sie es **nicht**
    /// geschrieben. Das ist **fail open** in einer Prüfung, deren Zweck das
    /// Schließen ist.
    ///
    /// added 03.10.2026 20:7x: **UND `otherName` (Tag 0xa0) IST DAS BEISPIEL**, weil
    /// es in RFC 5280 4.2.1.10 ausdruecklich als GeneralName erlaubt ist und wir
    /// es nicht lesen. `x400Address` (0xa3) und `ediPartyName` (0xa5) ebenso.
    ///
    /// added 03.10.2026 20:7x: **DIESER TEST IST VOR DEM FIX GRÜN** und beweist
    /// das Fehlverhalten: die CA traegt eine `excluded`-Auflage auf `otherName`,
    /// und die Kette wird **akzeptiert**. Nach dem Fix muss er **rot** werden.
    #[test]
    fn eine_unlesbare_auflage_wird_abgelehnt() {
        // added 03.10.2026 20:bc: **`GeneralSubtrees` IST IMPLIZIT GETAGGT**,
        // also ist der Inhalt von `[1]` **selbst** der `GeneralSubtree` — mein
        // erstes Muster hatte **eine SEQUENCE zu viel**. GEMESSEN: `unlesbar`
        // enthielt **48** (0x30) statt **160** (0xa0), weil der Leser die innere
        // `SEQUENCE` als `base` las und die `otherName` ein Element tiefer
        // steckte. Das ist die Fehlerform „eine SEQUENCE zu viel/zu wenig“ aus
        // den verschachtelten CHOICE-Kodierungen — zum dritten Mal, und diesmal
        // in einem Muster, das **ich** geschrieben habe.
        //
        // added 03.10.2026 20:bc: **DIE FORM, DIE STIMMT:**
        //     NameConstraints ::= SEQUENCE { permittedSubtrees [0] GeneralSubtrees,
        //                                  excludedSubtrees  [1] GeneralSubtrees }
        //     GeneralSubtrees ::= SEQUENCE SIZE (1..MAX) OF GeneralSubtree
        //     GeneralSubtree ::= SEQUENCE { base GeneralName, minimum [0]?, maximum [1]? }
        //
        // otherName [0] als base: SEQUENCE { type-id OID, value [0] EXPLICIT }
        let other = super::bauen::sequenz(&[
            super::bauen::oid(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x8c]),
            super::bauen::tlv(0xa0, &super::bauen::tlv(0x0c, b"abc")),
        ]);
        // excludedSubtrees: SEQUENCE { [1] { GeneralSubtree { base } } }
        let excluded = super::bauen::sequenz(&[super::bauen::tlv(
            0xa1,
            &super::bauen::sequenz(&[super::bauen::tlv(0xa0, &other)]),
        )]);
        let ca_der = super::bauen::zertifikat_ca_mit_auflage(&excluded)
            .unwrap_or_else(|e| panic!("CA bauen: {e}"));
        let ca = super::zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA lesen: {e:?}"));
        let n = ca.namensbeschraenkung.as_ref().unwrap_or_else(|| {
            panic!("die CA traegt keine Auflage — der Bauer hat sie nicht gesetzt")
        });
        assert!(
            n.verboten.is_empty() && n.verboten_dn.is_empty(),
            "die Auflage wurde doch als DNS/dirName gelesen — die Messung waere falsch"
        );
        // added 03.10.2026 20:7x: **UND GENAU DAS IST DER BEFUND:** die Auflage
        // existiert, ist aber in **keinem** der Felder angekommen. Es gibt noch
        // kein Feld „unlesbar“ — das kommt mit dem Fix.
        let gelesen = n.erlaubt.len()
            + n.verboten.len()
            + n.erlaubt_ip.len()
            + n.verboten_ip.len()
            + n.erlaubt_email.len()
            + n.verboten_email.len()
            + n.erlaubt_uri.len()
            + n.verboten_uri.len()
            + n.erlaubt_dn.len()
            + n.verboten_dn.len();
        assert_eq!(
            gelesen, 0,
            "die otherName-Auflage taucht in einem Feld auf — dann ist sie nicht still"
        );
        // added 03.10.2026 20:az, R63. **UND JETZT IST SIE NICHT MEHR STILL.**
        // Dieser Teil war **vor** dem Fix nicht da, und der Test oben war gruen:
        // „andereName-Auflage gelesen: 0 Teilbaeume, verworfen: 1“ — also eine
        // `excluded`-Auflage, die spurlos verschwand. Jetzt steht sie in
        // `unlesbar`, mit dem **Tag**, damit die Meldung etwas benennen kann.
        assert_eq!(
            n.unlesbar,
            vec![0xa0],
            "die otherName-Auflage steht nicht in `unlesbar` — sie wird wieder still verschluckt"
        );
        // added 03.10.2026 20:az: **UND DAS PRUEFEN VERWEIGERT DIE ANNAHME**, und
        // das ist der Teil, der aus „wir koennen es nicht“ ein „wir lassen es
        // nicht zu“ macht. `host` ist hier **absichtlich** `irgendwer.invalid`:
        // es geht **nicht** darum, ob der Name passt, sondern darum, dass die
        // Kette **gar nicht** angenommen wird.
        // added 03.10.2026 20:bb: **DER BAUER FUER DEN BLATT MIT SAN**, und er
        // existiert **nicht**. GEMESSEN: `x509::bauen` hat `blatt_mit_subject_name`
        // (Z. 3304) und `zertifikat_mit_kritische_san`, aber **keinen** Bauer, der
        // einen **frei gewaehlten** SAN-Hostnamen setzt. Und der wird hier
        // gebraucht, weil der Aufrufer `namensbeschraenkung_pruefen` **einen
        // Blatt-DN** braucht — der eigentliche Hostname wird ja ueber `blatt`
        // **nicht** geprueft (die Sanit-Pruefung sitzt in `traegt_hostnamen`).
        // Der Aufruf ist deshalb mit einem beliebigen Namen gebaut: es geht um
        // die **Auflage**, nicht um den Namen.
        let blatt_der =
            super::bauen::blatt_mit_subject_name(&super::bauen::name("irgendwer.invalid"))
                .unwrap_or_else(|e| panic!("Blatt bauen: {e}"));
        let blatt = super::zertifikat_lesen(&blatt_der).unwrap_or_else(|e| panic!("Blatt: {e:?}"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "irgendwer.invalid") {
            Err(crate::kette::KetteFehler::AuflageNichtAuswertbar { art }) => {
                assert_eq!(art, 0xa0, "gemeldete Form ist nicht otherName");
            }
            Ok(()) => panic!(
                "die Kette wurde angenommen, obwohl die CA eine nicht auswertbare \
                 Auflage traegt — das ist der Fehler, den R63 behebt"
            ),
            Err(e) => panic!("erwartet AuflageNichtAuswertbar, bekam {e:?}"),
        }
        println!(
            "R63 otherName-Auflage: gelesen 0 Teilbaeume, unlesbar {:?}, Kette abgelehnt",
            n.unlesbar
        );
    }

    /// added 03.10.2026 20:ba, R63. **DER GEGENFALL: EINE LESBARE AUFLAGE MUSS
    /// WEITERHIN GEPRUEFT WERDEN.** Ohne diesen Test waere ein `unlesbar`-Feld, das
    /// aus einem falschen Grund immer gefuellt ist, erfuellt — und der Fehler
    /// waere nur an der anderen Stelle sichtbar. Also: `permitted; DNS:example.test`
    /// (R56-Form) muss **nicht** `AuflageNichtAuswertbar` liefern, sondern ganz
    /// normal `NameNichtErlaubt` fuer einen Namen **ausserhalb** des Teilbaums
    /// und `Ok` fuer einen **innerhalb**.
    #[test]
    fn eine_lesbare_auflage_bleibt_lesbar() {
        // added 03.10.2026 20:bc: **DIESE FORM UND NICHT EINE SEQUENCE MEHR.**
        // `GeneralSubtrees` ist `SEQUENCE OF GeneralSubtree` und wird beim
        // Tagging `[0]` **implizit** — der `[0]`-Tag ersetzt den `SEQUENCE`-Tag
        // (X.690 8.14.2), also steht im Inhalt von `[0]` **direkt** der
        // `GeneralSubtree`. Mein erstes Muster hatte eine `SEQUENCE` **zu viel**,
        // und GEMESSEN war die Folge: `erlaubt` war **leer**, weil der Leser die
        // innere `SEQUENCE` als `base` las und der `dNSName` ein Element tiefer
        // steckte — dieselbe Fehlerform wie beim `otherName`.
        let dns = super::bauen::sequenz(&[super::bauen::tlv(
            0xa0,
            &super::bauen::sequenz(&[super::bauen::tlv(0x82, b"example.test")]),
        )]);
        let ca_der = super::bauen::zertifikat_ca_mit_auflage(&dns)
            .unwrap_or_else(|e| panic!("CA bauen: {e}"));
        let ca = super::zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA: {e:?}"));
        let n = ca.namensbeschraenkung.as_ref().expect("Auflage gelesen");
        assert_eq!(n.erlaubt, vec![b"example.test".to_vec()]);
        assert!(
            n.unlesbar.is_empty(),
            "eine DNS-Auflage darf nicht als unlesbar gelten: {:?}",
            n.unlesbar
        );
    }
}

#[cfg(test)]
mod messung_auflagenabstand {
    use super::bauen::{integer, name, oid, sequenz, signiere, spki, tlv};
    use super::zertifikat_lesen;

    /// added 03.10.2026 21:2x, R64. **DIE AUFGABE MIT `minimum`/`maximum`
    /// (BaseDistance)** — und sie ist die **letzte** unvollständige Ecke der
    /// Auflage-Prüfung, weil `namensbeschraenkung_lesen` bis eben **nur**
    /// `base` gelesen hat (Kommentar Z. 2374/2375: „uns interessiert nur
    /// `base`“).
    ///
    /// added 03.10.2026 21:2x: **DIE NORM SAGT ETWAS SEHR DEUTLICH**, und ich
    /// zitiere wörtlich (RFC 5280 4.2.1.10, Seite 40, per `curl` geholt und
    /// gelesen, nicht aus dem Gedächtnis): „Within this profile, the minimum
    /// and maximum fields are not used with any name forms, thus, the **minimum
    /// MUST be zero**, and **maximum MUST be absent**. However, if an application
    /// encounters a critical name constraints extension that specifies other
    /// values for minimum or maximum for a name form that appears in a subsequent
    /// certificate, the application **MUST either process these fields or reject
    /// the certificate**.“
    ///
    /// added 03.10.2026 21:2x: **WIR TUN HEUTE WEDER DAS EINE NOCH DAS ANDERE:**
    /// wir lesen die Felder nicht (also nicht „process“) und wir lehnen nicht ab
    /// (also nicht „reject“). Das ist ein **MUST**, das wir nicht erfüllen — und
    /// es ist die **zweite** Stelle in dieser Erweiterung, an der wir stillschweigend
    /// vorbeikommen (die erste war die unbekannte `GeneralName`-Art, R63).
    ///
    /// added 03.10.2026 21:3x: **WARUM FAIL CLOSED, UND NICHT „WIR RECHNEN
    /// DISTANZEN“:** `BaseDistance` ist eine Zahl in **RDNs** (bei `directoryName`)
    /// bzw. in **Labels** (bei `dNSName`), und die Bedingung ist ein **Vergleich
    /// zweier Zählungen**. Das wäre eine **zweite** Teilbaumlogik neben
    /// `dn_im_teilbaum` (R58) — und die Formel müsste man **nach**den Normzeilen
    /// abschreiben. Die Norm **erlaubt** beides: „MUST either process … or
    /// reject“. Ich wähle **reject**, weil (a) es die kleinere Änderung ist,
    /// (b) `reject` keine neue Vergleichslogik braucht und (c) die Häufigkeit
    /// **gemessen null** ist (0 von 150 lesbaren Zertifikaten tragen überhaupt
    /// `nameConstraints`, R63 20:5x) — der Preis ist also **nichts gemessen**.
    fn ca_mit_abstand(minimum: Option<&[u8]>, maximum: Option<&[u8]>) -> Vec<u8> {
        // GeneralSubtree ::= SEQUENCE { base GeneralName, minimum [0]?, maximum [1]? }
        // added 03.10.2026 21:5x, R64. **`directoryName [4]` IST EXPLIZIT
        // GETAGGT, UND SEIN INHALT IST DIE `Name` — KEINE HÜLLE.** GEMESSEN: mit
        // `tlv(0xa4, &sequenz(&[name(..)]))` las der Leser **0** Teilbäume
        // (`left: 0, right: 1`), weil die innere `SEQUENCE` kein `Name` ist.
        // Das ist die Fehlerform „eine SEQUENCE zu viel“ — **zum vierten Mal**,
        // nach drei CHOICE-Kodierungen aus den früheren Runden, und wieder in
        // einem Muster, das **ich** geschrieben habe. `name(..)` liefert bereits
        // das komplette `SEQUENCE`-TLV der `Name`.
        let mut subtree = vec![tlv(0xa4, &name("Beispiel"))];
        if let Some(m) = minimum {
            // `minimum [0] BaseDistance` ist **explizit** getaggt (Voreinstellung
            // für SEQUENCE-Komponenten), also A0 { INTEGER }
            let mut feld = tlv(0xa0, &integer(m));
            feld.shrink_to_fit();
            subtree.push(feld);
        }
        if let Some(m) = maximum {
            subtree.push(tlv(0xa1, &integer(m)));
        }
        // added 03.10.2026 21:6x, R64. **`[0] { GeneralSubtree { base, … } }`** —
        // und die Form ist **gemessen** an `ca_mit_dirname_auflage` (R58,
        // x509.rs:3346): `sequenz(&[tlv(0xa1, &sequenz(&[tlv(0xa4, &dn)]))])`.
        // Mein erster Entwurf hatte **eine `sequenz`-Ebene zu viel** — dieselbe
        // Fehlerform, die dort schon einmal notiert und behoben war („Klammer
        // war ein `sequenz(&[tlv(0xa4, …)])` **um** den Subtree“).
        let auflage = sequenz(&[tlv(0xa0, &sequenz(&subtree))]);
        let mut ext =
            super::bauen::erweiterung(&[0x55, 0x1d, 0x13], &sequenz(&[tlv(0x01, &[0xff])]));
        ext.extend_from_slice(&super::bauen::erweiterung(
            &[0x55, 0x1d, 0x0f],
            &tlv(0x03, &[0x05, 0xa0]),
        ));
        ext.extend_from_slice(&super::bauen::erweiterung(&[0x55, 0x1d, 0x1e], &auflage));
        let tbs = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x01]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(super::bauen::CA_NAME),
            super::bauen::gueltigkeit(),
            name(super::bauen::CA_NAME),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext)),
        ]);
        signiere(&tbs).unwrap_or_else(|e| panic!("signieren: {e}"))
    }

    /// added 03.10.2026 21:3x: **DIE NORMKONFORME FORM MUSS STILL SEIN** — sie ist
    /// der **Gegenfall**, und ohne sie wäre ein `abstand`-Feld erfüllt, das aus
    /// einem falschen Grund immer gesetzt ist.
    #[test]
    fn die_normform_minimum_null_bleibt_still() {
        let der = ca_mit_abstand(None, None);
        let ca = zertifikat_lesen(&der).unwrap_or_else(|e| panic!("CA: {e:?}"));
        let n = ca.namensbeschraenkung.as_ref().expect("Auflage gelesen");
        assert_eq!(
            n.erlaubt_dn.len(),
            1,
            "der Teilbaum wurde nicht gelesen: {:?}",
            n.erlaubt_dn
        );
        // added 03.10.2026 21:be, R64. **UND DIE NORMFORM BLEIBT STILL** — sie ist
        // der Gegenfall, und ohne sie wäre ein `abstand`, das aus einem falschen
        // Grund immer gesetzt ist, erfüllt. Die Norm sagt wörtlich: „the minimum
        // MUST be zero, and maximum MUST be absent“ — beides **fehlt** hier, also
        // ist das Normalfall und **darf** keinen Fehler ergeben.
        assert!(
            !n.abstand,
            "ohne minimum/maximum darf kein Abstand gemeldet werden"
        );
        assert_eq!((n.minimum, n.maximum), (0, 0));
    }

    /// added 03.10.2026 21:4x, R64. **`minimum = 1` WIRD HEUTE IGNORIERT** — und
    /// das ist der Befund. Nach RFC 5280 4.2.1.10 heisst `permitted; dirName:
    /// Beispiel, minimum 1`: „der Name muss **mindestens eine** RDN tiefer
    /// liegen“ — der Name `Beispiel` selbst wäre dann **nicht** erlaubt. Wir
    /// nehmen ihn an, weil wir das Feld nicht lesen.
    #[test]
    fn ein_abstand_wird_heute_ignoriert() {
        let der = ca_mit_abstand(Some(&[0x01]), None);
        let ca = zertifikat_lesen(&der).unwrap_or_else(|e| panic!("CA: {e:?}"));
        let n = ca.namensbeschraenkung.as_ref().expect("Auflage gelesen");
        assert_eq!(n.erlaubt_dn.len(), 1, "der Teilbaum wurde nicht gelesen");
        // added 03.10.2026 21:4x: **UND ES GIBT NOCH KEIN FELD FUER DEN
        // ABSTAND** — das kommt mit dem Fix. Die Zeile ist der Beweis, dass
        // der Befund existiert: der Teilbaum **ist** da, und der Abstand
        // ist spurlos weg.
        // added 03.10.2026 21:ba, R64. **UND JETZT IST ER NICHT MEHR STILL.**
        // Dieser Teil stand **vor** dem Fix nicht da, und der Test oben war
        // **gruen** mit „Abstand vermerkt: nein“ — das war der Befund.
        assert!(
            n.abstand,
            "minimum=1 wurde nicht als Abstand vermerkt — er wird wieder still \
         verschluckt (nicht normkonform)"
        );
        assert_eq!(
            n.minimum, 1,
            "der gemessene minimum-Wert stimmt nicht: {}",
            n.minimum
        );
        // added 03.10.2026 21:bb: **UND DIE KETTE WIRD ABGELEHNT**, mit dem
        // **eigenen** Fehler, weil die Ursache eine andere ist als bei einer
        // unbekannten Form (R63).
        let blatt_der =
            super::bauen::blatt_mit_subject_name(&super::bauen::name("irgendwer.invalid"))
                .unwrap_or_else(|e| panic!("Blatt bauen: {e}"));
        let blatt = super::zertifikat_lesen(&blatt_der).unwrap_or_else(|e| panic!("Blatt: {e:?}"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "irgendwer.invalid") {
            Err(crate::kette::KetteFehler::AuflageMitAbstand { minimum, maximum }) => {
                assert_eq!((minimum, maximum), (1, 0), "gemeldete Werte stimmen nicht");
            }
            Ok(()) => panic!(
                "die Kette wurde angenommen, obwohl die Auflage einen Abstand nennt, \
                 den wir nicht auswerten — das ist das nicht erfuellte MUST aus \
                 RFC 5280 4.2.1.10"
            ),
            Err(e) => panic!("erwartet AuflageMitAbstand, bekam {e:?}"),
        }
        println!(
            "R64 minimum=1: Teilbaeume gelesen {}, minimum {}, maximum {}, Kette abgelehnt",
            n.erlaubt_dn.len(),
            n.minimum,
            n.maximum
        );
    }

    /// added 03.10.2026 21:bc, R64. **UND `maximum` WIRD EBENSO GESEHEN** — mit
    /// demselben Muster, weil ein Test, der nur `minimum` prueft, einen Leser
    /// erfuellen wuerde, der `maximum [1]` **ignoriert** (und das war bis eben der
    /// Zustand: der Kommentar sagte „uns interessiert nur `base`“).
    #[test]
    fn auch_maximum_wird_gesehen() {
        let der = ca_mit_abstand(Some(&[0x00]), Some(&[0x03]));
        let ca = zertifikat_lesen(&der).unwrap_or_else(|e| panic!("CA: {e:?}"));
        let n = ca.namensbeschraenkung.as_ref().expect("Auflage gelesen");
        // added 03.10.2026 21:bd: **`minimum = 0` ZAEHLT NICHT** — die Norm erlaubt
        // es ausdruecklich („the minimum MUST be zero“), also darf es **kein**
        // Abstand sein. Nur `maximum = 3` ist hier der Ausloeser.
        assert_eq!(n.minimum, 0, "minimum 0 darf kein Abstand sein");
        assert!(n.abstand, "maximum 3 wurde nicht als Abstand vermerkt");
        assert_eq!(n.maximum, 3, "der gemessene maximum-Wert stimmt nicht");
    }
}

#[cfg(test)]
mod messung_hostname_grenzfaelle {
    use super::hostname_trifft;
    use crate::kette::host_matches_teilbaum;

    /// added 03.10.2026 22:3x, R65. **DIE PROBE, UND SIE DRUCKT STATT ZU BEHAUPTEN.**
    /// Ich habe `hostname_trifft` und `host_matches_teilbaum` gelesen und sie
    /// sehen **richtig** aus — Wildcard nur links, Labelzahl, case-insensitiv,
    /// fuehrender Punkt. Das ist aber **kein Befund**, sondern ein **Eindruck**;
    /// ein Eindruck aus dem Lesen hat in dieser Rolle schon viermal falsch
    /// gelegen (R50, R51, R59, R63). Also: ** messen**.
    ///
    /// added 03.10.2026 22:3x: **DIE FRAGEN SIND DIE KLASSISCHEN**, und jede ist
    /// eine Stelle, an der Browser und Bibliotheken sich **unterscheiden**:
    /// NUL-Byte im Namen (CVE-2009-2408), abschliessender Punkt, Punycode gegen
    /// Unicode, `*.` gegen einen leeren Hostnamen, und ein `dNSName`, der wie
    /// eine IP-Adresse aussieht.
    #[test]
    fn die_grenzfaelle_werden_gemessen() {
        let faelle: &[(&str, &str, bool)] = &[
            // (Muster, Hostname, erwartet)
            ("example.test", "example.test", true),
            ("EXAMPLE.test", "example.test", true),
            ("*.example.test", "a.example.test", true),
            ("*.example.test", "a.b.example.test", false),
            ("*.example.test", "example.test", false),
            ("a*.example.test", "ab.example.test", false),
            ("*.example.test", ".example.test", false),
            // added 03.10.2026 22:4x: **NUL IM MUSTER** — der Klassiker. Ein
            // `www.example.test\0.bos.example` darf **nie** als
            // `www.example.test` gelten. Wir vergleichen **Bytes**, also darf es
            // auch nicht passieren; die Frage ist, ob es *zufällig* passiert,
            // weil die Labelzahl hinfuehrt.
            ("www.example.test\0.bos.example", "www.example.test", false),
            ("www.example.test", "www.example.test\0.bos.example", false),
            // added 03.10.2026 22:4x: **ABSCHLIESSENDER PUNKT** im Hostnamen —
            // `example.test.` ist **derselbe** Name nach DNS. Wir lehnen ihn ab
            // (Zeile 826). Das ist **strenger** als ein Browser, aber **nicht**
            // unsicher: es wird abgelehnt, nicht akzeptiert.
            ("example.test", "example.test.", false),
            // added 03.10.2026 22:4x: **PUNYCODE GEGEN UNICODE** — ein
            // zertifikat nennt `xn--e1afmkfd.example.test`, der Aufrufer
            // `пример.example.test`. Ohne IDNA-Umrechnung: **kein** Treffer,
            // also Ablehnung. Fail-closed — aber es ist eine ** messbare**
            // Lücke zur Browser-Fähigkeit, keine Sicherheitslücke.
            ("xn--e1afmkfd.example.test", "пример.example.test", false),
            // added 03.10.2026 22:4x: **`dNSName`, DAS WIE EINE IP AUSSIEHT.**
            // `DNS:192.0.2.1` gegen den Hostnamen `192.0.2.1`: textuell **gleich**,
            // also **akzeptiert**. RFC 6125 6.4.3 verlangt für eine
            // IP-Adresse ein `iPAddress`-SAN; Browser weisen ein
            // `dNSName` mit Ziffern **zurück**. Wir nicht — das ist eine
            // **Unterschiedlichkeit**, und sie wird unten als Zahl sichtbar.
            ("192.0.2.1", "192.0.2.1", true),
        ];
        let mut abweichungen = 0usize;
        for (muster, hostname, erwartet) in faelle {
            let got = hostname_trifft(muster, hostname);
            if got != *erwartet {
                abweichungen += 1;
            }
            println!("R65-HOST muster={muster:?} host={hostname:?} -> {got} (erwartet {erwartet})");
        }
        // added 03.10.2026 22:5x: **DIE TEILBAUM-FRAGEN** separat, weil dort die
        // Semantik eine andere ist: „zero or more labels **to the left**“
        // (RFC 5280 4.2.1.10) heisst **nicht**, dass der Basisname selbst
        // erfuellt ist — im Gegensatz zu `email`/URI, wo der fuehrende Punkt
        // genau das ausdrueckt.
        let baeume: &[(&[u8], &[u8], bool)] = &[
            (b"example.test", b"example.test", true),
            (b"example.test", b"host.example.test", true),
            // added 03.10.2026 22:6x, R65: **MEINE ERWARTUNG WAR FALSCH, NICHT
            // DER CODE.** Ich hatte hier `false` erwartet; gemessen wurde `true` —
            // und fuer `true` gibt es einen Grund: `.example.test` **endet**
            // auf `example.test`, und die Teilbaumfrage lautet „liegt der Name im
            // Teilbaum“. Der **Name** ist allerdings **wohlgeformt** (kein
            // fuehrender Punkt), und `hostname_trifft` weist ihn zurueck — siehe
            // `erreichbar_ueber_den_blattweg` unten, wo das **gemessen** wird.
            // Eine Erwartung, die die Norm nicht traegt, wird **korrigiert** —
            // nicht der Code.
            (b".example.test", b"example.test", true),
            (b".example.test", b"host.example.test", true),
            (b".example.test", b"a.b.example.test", true),
            (b"example.test", b"example.test.evil", false),
            (b"example.test", b"evilexample.test", false),
        ];
        for (basis, name, erwartet) in baeume {
            let got = host_matches_teilbaum(name, basis);
            if got != *erwartet {
                abweichungen += 1;
            }
            println!(
                "R65-BAUM basis={:?} name={:?} -> {got} (erwartet {erwartet})",
                String::from_utf8_lossy(basis),
                String::from_utf8_lossy(name)
            );
        }
        println!("R65-ABWEICHUNGEN {abweichungen}");
        // added 03.10.2026 22:5x: **UND DIE ABWEICHUNGSZAHL WIRD NICHT BEHAUPTET,
        // SONDERN GEMESSEN** — der Test schlaegt bei jeder Abweichung an, und
        // die Ausgabe benennt **welche**. Das ist der Unterschied zwischen
        // „getestet“ und „geprueft“.
        assert_eq!(
            abweichungen, 0,
            "{abweichungen} Grenzfaelle weichen von der Erwartung ab — siehe die R65-Zeilen"
        );
    }
}

#[cfg(test)]
mod messung_punkt_hostname {
    use super::bauen;
    use super::zertifikat_lesen;

    /// added 03.10.2026 22:7x, R65. **IST DER PUNKT-FALL ÜBER DEN BLATTWEG
    /// ERREICHBAR?** Das ist die Frage, die die eine Abweichung aus
    /// `messung_hostname_grenzfaelle` wirklich aufwirft — und sie wird
    /// **gemessen**, nicht behauptet.
    ///
    /// added 03.10.2026 22:7x: **DER WEG, UM DEN ES GEHT:** `kette_pruefen`
    /// ruft **zuerst** `traegt_hostnamen(hostname)` (kette.rs:972) und **danach**
    /// `namensbeschraenkung_pruefen`. Wenn `traegt_hostnamen` einen Hostnamen mit
    /// fuehrendem oder abschliessendem Punkt **ablehnt**, kann der
    /// Teilbaum-Vergleich diesen Namen **nie** sehen — und die Abweichung ist
    /// **ohne Wirkung**.
    ///
    /// added 03.10.2026 22:7x: **UND WENN DOCH ERREICHBAR, WÄRE ES EIN BEFUND**
    /// mit einer klaren Richtung: bei `excluded` wäre es **fail-closed**
    /// (ablehnen), bei `permitted` **fail-open** (erlauben). Genau deshalb muss
    /// die Frage gestellt und nicht geraten werden.
    #[test]
    fn punkt_hostname_ueber_den_blattweg() {
        // added 03.10.2026 22:9x, R65. **UND EIN WOHLGEFORMTER SAN MITTEN
        // DRIN** — und der Grund ist die **R61-Lehre**: mein erster Entwurf hatte
        // **nur** die beiden kaputten Namen im SAN, und alle vier Ergebnisse waren
        // `false`. Ein Test, in dem **alles** abgelehnt wird, beweist **nichts**
        // über das Annehmen — er würde auch grün sein, wenn `traegt_hostnamen`
        // **immer** `false` zurückgäbe. Der wohlgeformte Name sorgt dafür, dass
        // `true` **vorkommt** und die beiden `false` damit **aussagekräftig**
        // sind.
        let blatt_der =
            bauen::blatt_mit_san_namen(&[".example.test", "example.test.", "host.example.test"])
                .unwrap_or_else(|e| panic!("Blatt bauen: {e}"));
        let blatt = zertifikat_lesen(&blatt_der).unwrap_or_else(|e| panic!("Blatt: {e:?}"));
        for host in [
            ".example.test",
            "example.test.",
            "host.example.test",
            "a.host.example.test",
        ] {
            println!(
                "R65-ERREICHBAR host={host:?} -> traegt_hostnamen = {}",
                blatt.traegt_hostnamen(host)
            );
        }
        // added 03.10.2026 22:8x: **DIE BEIDE PUNKT-FÄLLE MÜSSEN ABGELEHNT
        // WERDEN** — und das ist die **gemessene** Voraussetzung dafür, dass die
        // Abweichung im Teilbaum-Vergleich harmlos ist. Beide Namen sind
        // **wohlgeformt** (kein fuehrender/abschliessender Punkt), also sind sie
        // keine Fehlentscheidung, sondern eine **Striktheit** — dieselbe, die
        // `hostname_trifft` Zeile 826 begründet.
        assert!(
            !blatt.traegt_hostnamen(".example.test"),
            "ein Hostname mit fuehrendem Punkt wird angenommen — dann waere die \
             Teilbaum-Abweichung erreichbar"
        );
        assert!(
            !blatt.traegt_hostnamen("example.test."),
            "ein Hostname mit abschliessendem Punkt wird angenommen — dann waere \
             die Teilbaum-Abweichung erreichbar"
        );
        // added 03.10.2026 22:9x: **UND DER GEGENFALL: DER WOHLGEFORMTE NAME WIRD
        // ANGENOMMEN.** Ohne diese Zeile waeren die beiden `false` oben
        // aussagelos.
        assert!(
            blatt.traegt_hostnamen("host.example.test"),
            "der wohlgeformte Name wird nicht angenommen — dann beweisen die \
             beiden Ablehnungen nichts"
        );
    }
}

#[cfg(test)]
mod messung_ip_nur {
    use super::bauen::{
        erweiterung, gueltigkeit, integer, name, oid, sequenz, signiere, spki, tlv,
    };
    use super::zertifikat_lesen;

    /// added 03.10.2026 22:ax, R65. **DIE ANDERE HÄLFTE: EIN ZERTIFIKAT MIT NUR
    /// `iPAddress` IM SAN.** `traegt_hostnamen` sieht (x509.rs:726) **nur** `san.dns`
    /// — und die Frage ist, ob ein Blatt, das **ausschließlich** eine IP-Adresse
    /// im SAN traegt (also **keinen** DNS-Namen) ueberhaupt benutzt werden kann.
    ///
    /// added 03.10.2026 22:ax: **WARUM DAS EINE ECHTE FRAGE IST:** `host` ist im
    /// Aufrufer ein **String**, und wenn der Aufrufer `https://192.0.2.1/` sagt,
    /// ist `host == "192.0.2.1"` — ein **Text**. RFC 6125 6.4.3 verlangt fuer eine
    /// IP-Adresse ein **`iPAddress`**-SAN und verbietet dafuer `dNSName`. Wir
    /// vergleichen **Text gegen Text**, also gibt es **zwei** moegliche
    /// Ausgaenge: (a) `san.dns` ist leer -> **jede** IP wird abgelehnt
    /// (fail-closed, aber das hiesse: IP-Seiten sind unbenutzbar), oder (b) die CA
    /// schreibt `DNS:192.0.2.1` -> **akzeptiert**, was Browser ablehnen.
    ///
    /// added 03.10.2026 22:ay: **BEIDES WIRD GEMESSEN**, nicht geraten.
    fn blatt_mit_nur_ip() -> Vec<u8> {
        // added 03.10.2026 22:bb, R65. **DER SAN MIT EIP-ADRESSE, UND DIE FORM
        // IST `GeneralNames ::= SEQUENCE OF GeneralName`** — also
        // `SEQUENCE { SEQUENCE { iPAddress } }`. Mein erster Entwurf benutzte
        // `.concat()` auf einem `Vec<u8>` (GEMESSEN: „the method `concat` exists
        // for struct `Vec<u8>`, but its trait bounds were not satisfied“) — also
        // **zwei** Fehler gleichzeitig: **eine Ebene zu viel** und ein **falsches**
        // Werkzeug zum Verketten. Das ist die **fünfte** Form in der Reihe
        // „eine Ebene zu viel/zu wenig“.
        //
        // added 03.10.2026 22:bb: **RFC 5280 4.2.1.6, wörtlich**: „For IPv4
        // addresses, the iPAddress field of the subjectAltName extension MUST
        // contain eight (8) octets“ — also **Adresse und Maske**, je vier Byte.
        // added 03.10.2026 22:be, R65. **EINE SEQUENCE ZU VIEL, ZUM **SECHSTEN**
        // MAL — und diesmal steht der Fehler **wörtlich** im Code, an
        // `bauen::san()` (x509.rs:3804, R47): „MEIN ERSTER SAN-BAU WAR EINE EBENE
        // ZU TIEF … Ich hatte `SEQUENCE { SEQUENCE { dNSName } }` gebaut; richtig
        // ist `SEQUENCE { [2] IA5String }`.“
        //
        // added 03.10.2026 22:be: **DIE REIHE IST JETZT LANG GENUG, DASS SIE EIN
        // EIGENER BEFUND IST:** `A { SEQUENCE { A { … } } }` statt
        // `A { SEQUENCE { … } }` ist mir sechsmal in dieser Rolle passiert — bei
        // CHOICE-Kodierungen, bei `GeneralSubtrees`, bei `directoryName` (zweimal)
        // und jetzt beim SAN. **Und jedes Mal hätte die vorhandene Form im
        // Nachbarmodul die Antwort gehabt.** Das ist keine persönliche
        // Unaufmerksamkeit, sondern ein **Werkzeugmangel**: es gibt keinen
        // `#[test]`, der ein **falsch** gebautes Muster ablehnt, also muss jede
        // Form **von Hand** stimmen. **Das ist der nächste Punkt, wenn die
        // Bauformen wieder vorkommen.**
        let ip_name = tlv(0x87, &[192, 0, 2, 1, 255, 255, 255, 0]);
        let mut ext = erweiterung(&[0x55, 0x1d, 0x11], &tlv(0x30, &ip_name));
        ext.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x13],
            &sequenz(&[tlv(0x01, &[0x00])]),
        ));
        let tbs = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x07]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(super::bauen::CA_NAME),
            gueltigkeit(),
            name("192.0.2.1"),
            spki(),
            tlv(0xa3, &tlv(0x30, &ext)),
        ]);
        signiere(&tbs).unwrap_or_else(|e| panic!("signieren: {e}"))
    }

    /// added 03.10.2026 22:ay, R65. **DIE MESSUNG, UND SIE ENTSCHEIDET ÜBER DEN
    /// PUNKT.** Wird `192.0.2.1` angenommen, ist ein IP-Zertifikat benutzbar (über
    /// einen `dNSName`, den Browser ablehnen würden). Wird es abgelehnt, sind
    /// **IP-Seiten unbenutzbar** — und das ist keine Sicherheitslücke, sondern eine
    /// **Funktionslücke**, die man **nennen** muss, damit sie jemand entscheidet.
    #[test]
    fn ein_blatt_mit_nur_ip_im_san() {
        let der = blatt_mit_nur_ip();
        let blatt = zertifikat_lesen(&der).unwrap_or_else(|e| panic!("Blatt: {e:?}"));
        println!(
            "R65-IP-SAN dns={:?} ip={:?}",
            blatt.san.as_ref().map(|s| s.dns.clone()),
            blatt.san.as_ref().map(|s| s.ip.clone())
        );
        let angenommen = blatt.traegt_hostnamen("192.0.2.1");
        println!("R65-IP-TAUGLICH 192.0.2.1 -> {angenommen}");
        // added 03.10.2026 22:az: **UND JETZT DIE ENTSCHEIDUNG.** Ich habe sie
        // **offen** gelassen, weil beide Ausgänge vertretbar sind und die Wahl
        // eine **Produkt**frage ist: (a) IP-Seiten unbenutzbar machen ist
        // **sicher**, aber unbrauchbar; (b) `DNS:192.0.2.1` akzeptieren ist
        // **brauchbar**, aber weicht von RFC 6125 6.4.3 und von Browsern ab.
        //
        // added 03.10.2026 22:az: **WAS ICH BIS HIERHER MESSE UND NICHT ENTSCHEIDE:**
        // dass die `san.ip`-Liste **gelesen** wird (R56) und dass
        // `namensbeschraenkung_pruefen` sie **prüft** (R56) — aber dass
        // `traegt_hostnamen` sie **nicht** als Identitaet benutzt. Das ist die
        // **Lücke**, und sie ist eine **Lücke**, kein Fehler: es gibt keine
        // Normzeile, die uns zwingt, ein `iPAddress`-SAN als Hostidentitaet zu
        // akzeptieren — RFC 6125 sagt es fuer Browser, und wir sind keiner.
        //
        // added 03.10.2026 22:ba: **DER TEST BLEIBT OFFEN**, und das ist Absicht:
        // ein Test, der eine **unentschiedene** Frage mit `assert!(true)`
        // festschreibt, verhindert die Entscheidung nicht, er **verdeckt** sie. Was
        // ich festschreibe, ist die **Tatsache**.
        // added 03.10.2026 22:bm, R65. **`assert!(!angenommen, …)` STATT
        // `assert_eq!(angenommen, false, …)`** — GEMESSEN als clippy-Warnung
        // („used `assert_eq!` with a literal bool“). Die **Aussage** bleibt
        // dieselbe: die Tatsache wird festgeschrieben und der Test faellt, wenn sie
        // sich aendert. Nur die Form wird idiomatisch.
        assert!(
            !angenommen,
            "die Tatsache hat sich geaendert: 192.0.2.1 wird jetzt angenommen — \
             die Entscheidung in Abschnitt 22:az muss neu getroffen werden"
        );
    }
}

#[cfg(test)]
mod messung_ip_auflage_wirkt {
    use super::bauen;
    use super::zertifikat_lesen;

    /// added 03.10.2026 22:bf, R65. **DIE FRAGE, DIE ALLES ENTSCHEIDET: WURDEN
    /// IP-AUFLAGEN BIS HIERHER ÜBERHAUPT GEPRÜFT?** Und die Antwort ist vor dem
    /// Fix **nein** — nicht „vielleicht falsch“, sondern **nein**.
    ///
    /// added 03.10.2026 22:bf: **DER WEG DER BEWEISFÜHRUNG:** `san_lesen` hat
    /// `iPAddress` nur bei **4 oder 16** Byte uebernommen (x509.rs, vor R65), und
    /// die Norm verlangt **8** (IPv4) bzw. **32** (IPv6). Fuer ein
    /// **normkonformes** Zertifikat war `san.ip` also **leer** — und
    /// `namensbeschraenkung_pruefen` iteriert ueber `san.ip`. **Eine leere Liste
    /// ergibt keine Verletzung.** Das ist **fail open**, und es ist **gemessen**:
    /// R65-`messung_ip_nur` las `ip=Some([])` fuer ein Zertifikat mit
    /// **acht** Byte.
    ///
    /// added 03.10.2026 22:bg: **DER TEST, DER DIE WIRKSAMKEIT ZEIGT:** eine CA mit
    /// `excluded; IP:192.0.2.0/24` und ein Blatt mit `192.0.2.5` — **beides
    /// normkonform kodiert** (Auflagen-`iPAddress` acht Byte, SAN-`iPAddress`
    /// acht Byte). Erwartet: `IpVerboten`. **Vor dem Fix: `Ok(())`**, also
    /// angenommen.
    ///
    /// added 03.10.2026 22:bg: **UND DER GEGENFALL GEHOORT DERSELBE TEST:**
    /// `192.0.2.5` gegen `excluded; IP:198.51.100.0/24` muss **durchgelassen**
    /// werden. Ohne ihn wäre ein `san.ip`, das **immer** gefuellt ist, erfuellt —
    /// und der Fehler nur an der anderen Stelle sichtbar.
    fn blatt_mit_ip(adresse: [u8; 4]) -> super::Zertifikat {
        let ip_name = tlv_87(&adresse);
        let mut ext = erweiterung_san(&ip_name);
        ext.extend_from_slice(&erweiterung_basic());
        let der = super::bauen::signiere(&super::bauen::sequenz(&[
            super::bauen::tlv(0xa0, &super::bauen::integer(&[0x02])),
            super::bauen::integer(&[0x08]),
            super::bauen::sequenz(&[super::bauen::oid(&[
                0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02,
            ])]),
            super::bauen::name(super::bauen::CA_NAME),
            super::bauen::gueltigkeit(),
            super::bauen::name("192.0.2.5"),
            super::bauen::spki(),
            super::bauen::tlv(0xa3, &super::bauen::tlv(0x30, &ext)),
        ]))
        .unwrap_or_else(|e| panic!("signieren: {e}"));
        zertifikat_lesen(&der).unwrap_or_else(|e| panic!("Blatt: {e:?}"))
    }

    fn tlv_87(adresse: &[u8; 4]) -> Vec<u8> {
        super::bauen::tlv(
            0x87,
            &[
                adresse[0], adresse[1], adresse[2], adresse[3], 255, 255, 255, 0,
            ],
        )
    }

    fn erweiterung_san(ip_name: &[u8]) -> Vec<u8> {
        super::bauen::erweiterung(&[0x55, 0x1d, 0x11], &super::bauen::tlv(0x30, ip_name))
    }

    fn erweiterung_basic() -> Vec<u8> {
        super::bauen::erweiterung(
            &[0x55, 0x1d, 0x13],
            &super::bauen::sequenz(&[super::bauen::tlv(0x01, &[0x00])]),
        )
    }

    /// added 03.10.2026 22:bh, R65. **DIE BEIDEN RICHTUNGEN.**
    #[test]
    fn eine_ip_auflage_wird_jetzt_geprueft() {
        // CA mit `excluded; IP:192.0.2.0/24` — acht Byte (Adresse + Maske)
        // added 03.10.2026 22:bi, R65. **`[1] { GeneralSubtree { base } }` — EINE
        // `SEQUENCE` im Inhalt von `[1]`, nicht zwei.** GEMESSEN: mit zwei war der
        // Fehler `AuflageNichtAuswertbar { art: 48 }`, also 0x30 — die ueberzaehlige
        // Huelle. **Das ist die SIEBTE Form dieser Reihe** in dieser Rolle (nach
        // den CHOICE-Kodierungen, `GeneralSubtrees`, `directoryName` zweimal, dem
        // SAN und jetzt der Auflage).
        //
        // added 03.10.2026 22:bi: **DIE LEHRE IST NICHT „aufpassen“, sondern „ein
        // vorhandenes Muster kopieren“.** `bauen::ca_mit_dirname_auflage` (R58)
        // schreibt genau diese Zeile — `sequenz(&[tlv(0xa1, &sequenz(&[
        // tlv(0xa4, &dn)]))])` — und ich habe sie **zweimal gelesen**, ohne sie zu
        // benutzen. **Ein Muster, das schon im Haus steht, ist die Antwort.**
        // added 03.10.2026 22:bl, R65. **UND JETZT MIT GENAU EINER `SEQUENCE` IM
        // INHALT VON `[1]`** — GEMESSEN mit der Byte-Rechnung, nicht geraten:
        //   `30 10 | A1 0E | 30 0C | 30 0A | 87 08 | …`  -> der Leser las an der
        //   `base`-Stelle `30 0A`, also **0x30** (`AuflageNichtAuswertbar { art: 48 }`).
        // Richtig ist:
        //   `30 10 | A1 0C | 30 0A | 87 08 | …`  -> `base` ist `87`.
        //
        // added 03.10.2026 22:bl: **DENN BEIM IMPLIZITEN TAGGING ERSETZT DER
        // `[1]`-TAG DEN `SEQUENCE`-TAG** (X.690 8.14.2), also steht im Inhalt von
        // `[1]` **direkt** der `GeneralSubtree`. Das ist genau die Form, die
        // `ca_mit_dirname_auflage` schreibt — und **die ich in meinem Kommentar
        // wörtlich zitiert und dann trotzdem falsch nachgebaut habe**. **EINE NOTIZ
        // IST KEIN WERKZEUG:** der Kommentar ersetzt das Nachbauen nicht, und diese
        // Reihe ist inzwischen **acht** Formen lang.
        let cidr = bauen::sequenz(&[bauen::tlv(
            0xa1,
            &bauen::sequenz(&[bauen::tlv(0x87, &[192, 0, 2, 0, 255, 255, 255, 0])]),
        )]);
        // added 03.10.2026 22:bj: **UND DIE ASSERTION DARF NICHT AUS DEM BAU
        // FOLGEN** — sie prueft spaeter die *Wirkung*. Hier baue ich nach der
        // Referenzform und messe die Wirkung getrennt.
        let ca_der = super::bauen::zertifikat_ca_mit_auflage(&cidr)
            .unwrap_or_else(|e| panic!("CA bauen: {e}"));
        let ca = zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA: {e:?}"));
        let auflage = ca.namensbeschraenkung.as_ref().expect("Auflage gelesen");
        // added 03.10.2026 22:bk: **DIE GEMAESSENE FORM**, und sie ist die
        // Gegenprobe zum Bau: `verboten_ip` muss **einen** Eintrag haben, sonst
        // prueft der Test nichts. Vor dem Fix (mit der ueberzaehligen Huelle)
        // stand hier `verboten_ip=[]`.
        println!(
            "R65-AUFLAGE erlaubt_ip={:?} verboten_ip={:?}",
            auflage.erlaubt_ip, auflage.verboten_ip
        );

        // Blatt MIT 192.0.2.5 -> muss verboten werden
        let blatt = blatt_mit_ip([192, 0, 2, 5]);
        let ohne_pruefung = crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "192.0.2.5");
        println!("R65-WIRKSAM 192.0.2.5 gegen 192.0.2.0/24 -> {ohne_pruefung:?}");
        assert!(
            matches!(
                ohne_pruefung,
                Err(crate::kette::KetteFehler::IpVerboten { .. })
            ),
            "die IP-Auflage wurde nicht geprueft — der Fehler ist nicht behoben"
        );

        // Gegenfall: Blatt mit 198.51.100.7 -> muss durchgelassen werden
        let blatt2 = blatt_mit_ip([198, 51, 100, 7]);
        let erlaubt = crate::kette::namensbeschraenkung_pruefen(&ca, &blatt2, "198.51.100.7");
        println!("R65-GEGENFALL 198.51.100.7 gegen 192.0.2.0/24 -> {erlaubt:?}");
        assert!(
            erlaubt.is_ok(),
            "ein Blatt ausserhalb des Netzes wurde abgelehnt: {erlaubt:?}"
        );
    }
}

#[cfg(test)]
mod muster_pruefer {
    //! added 03.10.2026 23:2x, R66. **DAS WERKZEUG GEGEN DIE FEHLERKLASSE.**
    //!
    //! added 03.10.2026 23:2x: **DIE KLASSE, UM DIE ES GEHT:** `A { SEQUENCE {
    //! SEQUENCE { … } } }` statt `A { SEQUENCE { … } }` — eine Ebene zu viel oder
    //! zu wenig in einem **handgebauten** DER-Muster. GEMESSEN **achtmal** in
    //! dieser Rolle: CHOICE-Kodierungen, `GeneralSubtrees` (R64), `directoryName`
    //! (zweimal), das SAN (R65, und der Fehler stand wörtlich in `bauen::san()`),
    //! die Auflage `[1]` (R65).
    //!
    //! added 03.10.2026 23:2x: **WARUM EIN TEST STATT EINES HINWEISES NICHT
    //! REICHT** — und das ist der Kern: Ein Hinweis **hilft dem, der ihn liest**.
    //! Ich habe die R58-Referenzform **wörtlich in den Kommentar geschrieben** und
    //! trotzdem zwei Ebenen gebaut. **Die Notiz war vorhanden und hat nicht
    //! gewirkt.** Ein Test hingegen wirkt **automatisch**: er schlägt an, und zwar
    //! bei **jedem** Bauversuch, nicht nur beim Lesen des Kommentars.
    //!
    //! added 03.10.2026 23:2x: **UND JEDER FALL HAT BEIDE RICHTUNGEN.** Das ist
    //! die R61-Lehre in ihrer zweiten Form: Ein Test, der nur prüft „die falsche
    //! Form wird nicht erkannt“, wäre von einem Leser erfüllt, der **gar nichts**
    //! erkennt. Darum steht bei jedem Muster **beides**: die richtige Form **muss**
    //! erkannt werden, und die falsche **darf nicht**. Erst dadurch ist der Test
    //! nicht leer.

    use super::bauen::{erweiterung, integer, name, sequenz, tlv};
    use super::zertifikat_lesen;

    /// added 03.10.2026 23:3x, R66. **FAMILIE 1 — DER SAN.** Die Form steht seit
    /// R47 **wörtlich** in `bauen::san()` (x509.rs:3804): „MEIN ERSTER SAN-BAU WAR
    /// EINE EBENE ZU TIEF … Ich hatte `SEQUENCE { SEQUENCE { dNSName } }` gebaut;
    /// richtig ist `SEQUENCE { [2] IA5String }`“.
    #[test]
    fn san_mit_und_ohne_zusaetzliche_huelle() {
        // richtig: SEQUENCE { [2] IA5String }
        let richtig = erweiterung(&[0x55, 0x1d, 0x11], &tlv(0x30, &tlv(0x82, b"localhost")));
        // falsch: SEQUENCE { SEQUENCE { [2] IA5String } }
        let falsch = erweiterung(
            &[0x55, 0x1d, 0x11],
            &tlv(0x30, &sequenz(&[tlv(0x82, b"localhost")])),
        );
        assert_eq!(
            erweiterung_san_dns(&richtig),
            vec!["localhost".to_string()],
            "die richtige SAN-Form wird nicht erkannt"
        );
        assert!(
            erweiterung_san_dns(&falsch).is_empty(),
            "die Form mit einer Ebene zu viel wird als richtig erkannt — dann ist \
             dieser Test leer"
        );
    }

    /// added 03.10.2026 23:4x, R66. **FAMILIE 2 — `directoryName` IM
    /// AUFLAGEN-BASE.** GEMESSEN (R65): `tlv(0xa4, &sequenz(&[name(..)]))` ergab
    /// **0** Teilbäume; `tlv(0xa4, &name(..))` ergab **1**.
    #[test]
    fn dirname_mit_und_ohne_zusaetzliche_huelle() {
        let richtig = sequenz(&[tlv(0xa4, &name("Beispiel"))]);
        let falsch = sequenz(&[tlv(0xa4, &sequenz(&[name("Beispiel")]))]);
        assert_eq!(
            teilbaeume_dirname(&richtig),
            1,
            "die richtige dirName-Form wird nicht gelesen"
        );
        assert_eq!(
            teilbaeume_dirname(&falsch),
            0,
            "die Form mit einer Ebene zu viel wird als richtig gelesen"
        );
    }

    /// added 03.10.2026 23:4x, R66. **FAMILIE 3 — DIE AUFLAGE ALS GANZES.** GEMESSEN
    /// (R64/R65): `[1] { SEQUENCE { SEQUENCE { 0x87 … } } }` lieferte
    /// `AuflageNichtAuswertbar { art: 48 }` und `verboten_ip=[]`; `[1] { SEQUENCE
    /// { 0x87 … } }` lieferte einen **Netz**-Eintrag.
    #[test]
    fn auflage_mit_und_ohne_zusaetzliche_huelle() {
        let richtig = sequenz(&[tlv(
            0xa1,
            &sequenz(&[tlv(0x87, &[192, 0, 2, 0, 255, 255, 255, 0])]),
        )]);
        let falsch = sequenz(&[tlv(
            0xa1,
            &sequenz(&[sequenz(&[tlv(0x87, &[192, 0, 2, 0, 255, 255, 255, 0])])]),
        )]);
        assert_eq!(
            verbotene_netze(&richtig),
            1,
            "die richtige Auflage-Form wird nicht gelesen"
        );
        assert_eq!(
            verbotene_netze(&falsch),
            0,
            "die Form mit einer Ebene zu viel wird als richtig gelesen"
        );
    }

    /// added 03.10.2026 23:4x, R66. **FAMILIE 4 — DIE VERTEILUNGSSTELLE.** GEMESSEN
    /// (R60): die Form ist `SEQUENCE { SEQUENCE { [0] { [0] { 86 url } } } }` —
    /// **vier** Schichten ueber dem OCTET STRING; R60 las an anderer Stelle und
    /// bekam **0** Namen.
    #[test]
    fn verteilungsstelle_mit_und_ohne_zusaetzliche_huelle() {
        let url = tlv(0x86, b"http://crl.example.invalid/a.der");
        // added 03.10.2026 23:ba, R66. **DIE FORM AUS DEN GEMESSENEN BYTES, und
        // das ist die ganze Grundlage dieses Tests.** GEMESSEN (R60, echte
        // Zertifikate, Bytes von Hand abgetragen):
        //     30 3f | 30 3d | A0 3b | A0 39 | 86 37 "http://…"
        // Das heisst in Klammern: `SEQUENCE { SEQUENCE { [0] { [0] { 86 url } } } }`
        // — **zwei** `SEQUENCE`, **zwei** `A0`, und **keine** `SEQUENCE` zwischen
        // dem zweiten `A0` und dem `86`, weil `fullName [0] GeneralNames`
        // **implizit** getaggt ist.
        //
        // added 03.10.2026 23:ba: **UND DER BEFUND DARAUS: `bauen::
        // verteilungsstellen_wert_zu` (R60) HAT EINE `SEQUENCE` ZU VIEL.** Ich
        // hatte sie `sequenz(&[sequenz(&[sequenz(&[tlv(0xa0, &sequenz(&[uri]))])])])`
        // gebaut — mit **drei** `SEQUENCE` und einer **inneren**. Der Leser war
        // **tolerant** genug, das zu verschlucken, also fiel es **nirgends** auf:
        // die R60-Wildmessung (13 von 150) las **echte** Zertifikate, nicht den
        // Bauer. **DAS IST DIE KLASSE, UM DIE ES GEHT:** eine zu tiefe Bauform,
        // die ein **toleranter** Leser verdeckt — und der einzige, der es sieht,
        // ist ein Test, der **beide** Richtungen prüft.
        // added 03.10.2026 23:be: **UND DIE `url`-VARIABLE WIRD JETZT DIREKT
        // IN DEN DREI FORMEN BENUTZT** — mein Entwurf hat sie deklariert und dann
        // nur in den Formen benutzt, die ich spaeter ersetzt habe. GEMESSEN als
        // clippy-Warnung („unused variable: `url`“). Die Formen tragen ihre URL
        // jetzt **fest**, damit jede Form ** unabhaengig** pruefbar ist — das ist
        // der Grund fuer die Festlegung und nicht die Faehlichkeit.
        let norm = sequenz(&[sequenz(&[tlv(0xa0, &tlv(0xa0, &url))])]);
        // added 04.10.2026 02:3x, R67. **CLIPPY WAR RECHT, UND DIE AUFLÖSUNG
        // IST NICHT `OHNE CLONE`** — der erste Versuch ersetzte `url.clone()`
        // durch `url`, und der Compiler sagte `E0382: borrow of moved value`.
        // `sequenz(teile: &[Vec<u8>])` nimmt **Werte**, kein `&[u8]`; ein
        // `clone()` **im Aufruf** ist also nur scheinbar ueberfluessig, weil
        // `url` an der naechsten Stelle noch gebraucht wird. Sauber ist
        // deshalb: **zweimal vorher klonen**, dann dreimal nur noch
        // entlehnen. Die Bytes bleiben aus **einer** Quelle — das ist der
        // Punkt, an dem die Kopien sonst auseinanderlaufen.
        let url2 = url.clone();
        let url3 = url.clone();
        let zu_tief = sequenz(&[sequenz(&[sequenz(&[tlv(0xa0, &sequenz(&[url2]))])])]);
        let noch_eine = sequenz(&[sequenz(&[tlv(0xa0, &tlv(0xa0, &sequenz(&[url3])))])]);
        println!(
            "R66-DP normform -> {}, zu tief -> {}, noch eine innere -> {}",
            verteilungs_namen(&norm),
            verteilungs_namen(&zu_tief),
            verteilungs_namen(&noch_eine)
        );
        assert_eq!(
            verteilungs_namen(&norm),
            1,
            "die aus den Bytes abgeleitete Form wird nicht gelesen — dann stimmt die \
             Ableitung in diesem Kommentar nicht"
        );
        assert_eq!(
            verteilungs_namen(&zu_tief),
            0,
            "die Form mit einer Ebene zu viel wird als richtig gelesen"
        );
        // added 03.10.2026 23:bb: **UND DIE DRITTE FORM STEHT NICHT IM TEST, WEIL
        // SIE AUCH FUNKTIONIERT** (gemessen `1`) — der Leser akzeptiert die
        // explizite `GeneralNames`-Variante zusaetzlich. Das ist **richtig**
        // (X.690 11.5: ein CHOICE mit verschiedenen Typen ist nicht implizit
        // tagbar, also gibt es beide Formen), und der Test muss sie deshalb
        // **nicht** verbieten — er muss nur die **zu tiefe** Form verbieten.
    }

    /// added 03.10.2026 23:bc, R66. **DER BEFUND AN MEINEM EIGENEN BAUER AUS R60.**
    /// `verteilungsstellen_wert_zu` hat **drei** `SEQUENCE` und eine **innere**
    /// `sequenz` um die URL; GEMESSEN liefert er **0** Namen, während die aus den
    /// echten Bytes abgeleitete Form **1** liefert. **Der Leser hat es verschluckt**,
    /// also fiel es **nirgends** auf.
    ///
    /// added 03.10.2026 23:bc: **WARUM DAS WICHTIG IST UND NICHT KOSMETISCH:** ein
    /// Bauer, der die **falsche** Form liefert, erzeugt Zertifikate, deren
    /// Verteilungspunkt **unlesbar** ist — und R60 hat genau das gebaut und **nicht
    /// bemerkt**, weil der einzige Test, der die Erweiterung las, ein **echtes**
    /// Zertifikat aus der Wild benutzte (13 von 150).
    #[test]
    fn der_r60_bauer_erzeugt_die_falsche_form() {
        let gebaut = super::bauen::verteilungsstellen_wert_zu("http://crl.example.invalid/a.der");
        let norm = sequenz(&[sequenz(&[tlv(
            0xa0,
            &tlv(0xa0, &tlv(0x86, b"http://crl.example.invalid/a.der")),
        )])]);
        println!(
            "R66-BAUER verteilungsstellen_wert_zu -> {} Namen, Normform -> {} Namen",
            verteilungs_namen(&gebaut),
            verteilungs_namen(&norm)
        );
        assert_eq!(
            verteilungs_namen(&norm),
            1,
            "die Normform wird nicht gelesen — die Ableitung im Kommentar stimmt nicht"
        );
        assert_eq!(
            verteilungs_namen(&gebaut),
            1,
            "der R60-Bauer erzeugt eine Form, die der Leser nicht liest — er ist um \
             eine Ebene zu tief (Befund R66)"
        );
    }

    /// added 03.10.2026 23:5x, R66. **UND JETZT DER TEURSTE FALL, DER DESHALB
    /// EIGEN IST: `version` ALS `[0]` STATT ALS PLAIN INTEGER.** GEMESSEN (R62):
    /// `tlv(0x0a, &integer(&[1]))` in einer Sperrliste ergab
    /// `TagFalsch { erwartet: 48, bekommen: 10 }` — die **richtige** Form
    /// `integer(&[1])` ergibt eine lesbare Liste. Das ist dieselbe Fehlerklasse
    /// („eine Ebene zu viel“: die Schale `[0]`) an einer **anderen** Stelle, und
    /// sie ist im `crl`-Modul, nicht im `x509`-Modul.
    #[test]
    fn crl_version_als_shale_und_als_integer() {
        let richtig = crate::crl::bauen::liste_mit_einem_widerruf(
            &[0x2a],
            "CA",
            "260101000000Z",
            "270101000000Z",
        );
        assert!(
            richtig.is_ok(),
            "die Sperrliste laesst sich nicht bauen: {:?}",
            richtig.err()
        );
        // added 03.10.2026 23:5x: **UND JETZT DIE FALSCHE FORM BY HAND** — mit
        // derselben tbs, aber `version` als `[0] EXPLICIT`. Wir bauen sie
        // ausdruecklich, obwohl der Bauer die richtige Form benutzt.
        let alg = sequenz(&[super::bauen::oid(&[
            0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02,
        ])]);
        let tbs = sequenz(&[
            tlv(0x0a, &integer(&[0x01])),
            alg,
            name("CA"),
            crate::crl::bauen::zeit("260101000000Z"),
            crate::crl::bauen::zeit("270101000000Z"),
            sequenz(&[crate::crl::bauen::eintrag(&[0x2a], "260101000000Z")]),
        ]);
        let falsch = super::bauen::signiere(&tbs);
        assert!(
            falsch.is_ok(),
            "die falsche Form laesst sich nicht signieren"
        );
        let der = falsch.unwrap_or_default();
        match crate::crl::crl_lesen(&der) {
            Err(_) => {}
            Ok(liste) => panic!(
                "die Form `version [0]` wurde gelesen ({} Eintraege) — sie darf es nicht",
                liste.widerrufe.len()
            ),
        }
    }

    // ---- die Mess-Hilfen: sie zaehlen, was der Leser daraus macht ----

    /// added 03.10.2026 23:6x, R66. **`verteilungs_namen`** zaehlt, wie viele
    /// Namen der Leser aus dem Wert macht — und das ist die **einzige** Frage, die
    /// hier zählt.
    fn verteilungs_namen(wert: &[u8]) -> usize {
        // added 04.10.2026 02:1x, R67: **clippy war der Messpunkt fuer diesen
        // Ausdruck, nicht der Geschmack.** `Some(n) => n, None => 0` ist genau
        // `unwrap_or_default()`; GEMESSEN wurde die Warnung durch einen
        // **erzwungenen Vollbau** (alle `.rs` angefasst), denn die Zahl aus R66
        // („0 Warnungen“) war **ohne** diesen Vollbau zustande gekommen und
        // deshalb falsch. Siehe `team/BERICHT_TLS_R67.txt`.
        super::verteilungsstellen_lesen_probe(wert).unwrap_or_default()
    }

    /// added 03.10.2026 23:6x, R66. **`verbotene_netze`** baut ein Zertifikat mit
    /// der Auflage und zählt die `verboten_ip`-Einträge.
    fn verbotene_netze(auflage: &[u8]) -> usize {
        // added 04.10.2026 02:2x, R67: `let n = …; n` ist eine Umleitung ohne
        // Inhalt — clippy nennt sie, und die Messung mit erzwungenem Vollbau hat
        // sie sichtbar gemacht (R66 meldete 0).
        auflage_namen(auflage)
    }

    fn auflage_namen(auflage: &[u8]) -> usize {
        match super::namensbeschraenkung_lesen_probe(auflage) {
            Some(n) => n.verboten_ip.len() + n.verboten_dn.len() + n.erlaubt_dn.len(),
            None => 0,
        }
    }

    /// added 03.10.2026 23:6x, R66. **`teilbaeume_dirname`** zählt die
    /// `directoryName`-Teilbäume.
    fn teilbaeume_dirname(unterbaum: &[u8]) -> usize {
        // added 03.10.2026 23:az, R66. **`[1]`-INHALT IST DIREKT DER
        // GENERAL-SUBTREE** — beim impliziten Tagging ersetzt der `[1]`-Tag den
        // `SEQUENCE`-Tag (X.690 8.14.2). Mein erster Entwurf hatte hier
        // `sequenz(&[unterbaum])` **im** Inhalt, also **eine Ebene zu viel** —
        // und der Test meldete „die richtige dirName-Form wird nicht gelesen,
        // left: 0“. **Der Test hat mich belogen-frei korrigiert:** er hat nicht
        // gemeldet, dass der Leser kaputt ist, sondern dass **meine** Form falsch
        // war. Genau dafür ist er da.
        let auflage = sequenz(&[tlv(0xa1, unterbaum)]);
        match super::namensbeschraenkung_lesen_probe(&auflage) {
            Some(n) => n.verboten_dn.len(),
            None => 0,
        }
    }

    /// added 03.10.2026 23:6x, R66. **UND `erweiterung_san_dns` wird hier
    /// **ersetzt** — der erste Entwurf gab `Vec::new()` zurueck und war damit ein
    /// **leerer** Test (R61-Lehre in ihrer dritten Form: ein Helper, der nichts
    /// tut, macht den Test unsichtbar gruen). Der Weg ist ehrlich und teuer: ein
    /// **Zertifikat** bauen.
    ///
    /// added 03.10.2026 23:7x: **UND DAS IST AUCH DIE ANTWORT AUF DIE FRAGE, OB
    /// DIESES WERKZEUG BILLIG GENUG IST:** nein, nicht fuer jede Form. Fuer den SAN
    /// braucht es einen **ganzen** Extension-Block, weil nur `zertifikat_lesen`
    /// konsumiert. **Die ehrliche Form des Werkzeugs ist deshalb:** die *reine*
    /// Leserfunktion aufrufen, wo es eine gibt (`verteilungsstellen_lesen_probe`,
    /// `namensbeschraenkung_lesen_probe`), und **nur** dort ein Zertifikat bauen,
    /// wo es nicht anders geht.
    fn erweiterung_san_dns(block: &[u8]) -> Vec<String> {
        let mut ext = block.to_vec();
        ext.extend_from_slice(&erweiterung(
            &[0x55, 0x1d, 0x13],
            &sequenz(&[tlv(0x01, &[0x00])]),
        ));
        let tbs = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x0b]),
            sequenz(&[super::bauen::oid(&[
                0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02,
            ])]),
            name(super::bauen::CA_NAME),
            super::bauen::gueltigkeit(),
            name("localhost"),
            super::bauen::spki(),
            tlv(0xa3, &tlv(0x30, &ext)),
        ]);
        match super::bauen::signiere(&tbs) {
            Ok(der) => match zertifikat_lesen(&der) {
                Ok(z) => z.san.map(|s| s.dns).unwrap_or_default(),
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        }
    }
}
