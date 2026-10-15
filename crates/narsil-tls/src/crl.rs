//! Sperrlisten (CRL), **nur das Lesen und das Prüfen**.
//!
//! added 02.10.2026 06:1x UTC (Projektlabel 02.10.2026), Rolle TLS.
//!
//! **WARUM ÜBERHAUPT.** Die Lücke ist in `tests/badssl_tabelle.rs` als
//! `revoked_ist_die_bekannte_luecke` benannt: wenn ein Server kein OCSP
//! stapelt, nehmen wir ein **widerrufenes** Zertifikat an. OCSP ist nur der
//! eine Weg, den RFC 5280 für den Widerrufsstatus vorsieht; der andere ist die
//! Sperrliste. Diese Datei schließt **die Prüfseite** — sie entscheidet, ob
//! eine Liste echt ist, ob sie vom richtigen Aussteller ist und ob sie den
//! Zeitpunkt noch deckt.
//!
//! **WAS HIER NICHT DRIN IST, und das ist eine Entscheidung, kein Versehen:**
//! das **Holen** der Liste. Dafür braucht es HTTP, und HTTP ist Sache von
//! `narsil-net`; `narsil-tls` darf diese Abhängigkeit nicht haben, sonst entsteht
//! ein Kreis. Die Nahtstelle ist deshalb schmal: der Aufrufer holt die Bytes
//! und ruft `beurteile`, und er bekommt kein `bool` zurück, sondern ein
//! `Urteil` mit Grund — so kann er „nicht widerrufen" von „nicht brauchbar"
//! unterscheiden. Das ist keine Feinheit: ein nicht abrufbares Widerrufsportal
//! und ein widerrufenes Zertifikat sind zwei verschiedene Tatsachen.
//!
//! DIE NORMSTELLEN, und keine davon ist abgetippt, sie stehen in RFC 5280:
//! * 5.1.1.2 — der `signatureAlgorithm` **muss** derselbe sein wie das
//!   `signature`-Feld im `tbsCertList`: "This field MUST contain the same
//!   algorithm identifier as the signature field in the sequence tbsCertList".
//! * 5.1.2.5 — "Conforming CRL issuers MUST include the nextUpdate field in
//!   all CRLs." Fehlt das Feld, ist die Liste bei uns **nicht** brauchbar.
//! * 5.1.2.6 — "When there are no revoked certificates, the revoked
//!   certificates list MUST be absent." Ein vorhandenes, aber leeres
//!   `SEQUENCE` ist also nicht die Regel; wir lesen es trotzdem, es schadet
//!   nicht und es zu verweigern wäre eine eigene Regel.
//! * 5.2.1 — `authorityKeyIdentifier` (2.5.29.35) vergleicht die Liste mit dem
//!   Schluessel des Ausstellers. Wir vergleichen stattdessen den Namen und die
//!   Signatur; siehe die Bemerkung bei [`pruefen`].
//! * 6.3.3 (a)(1) — "If the current time is after the value of the CRL next
//!   update field, then do one of the following" — eine Liste, deren
//!   `nextUpdate` vergangen ist, darf **nicht** benutzt werden.
//! * 6.3.3 (b)(1) — "Otherwise, verify that the CRL issuer matches the
//!   certificate issuer."
//! * 6.3.3 (b)(2), (c), (d) — der **Umfang** der Liste (`onlySomeReasons`,
//!   `cRLIssuer`, Delta-Listen).
//! * 6.3.3 (f) — "If a key usage extension is present in the CRL issuer's
//!   certificate, verify that the cRLSign bit is set."
//! * 6.3.3 (g) — "Validate the signature on the complete CRL using the public
//!   key validated in step (f)."
//! * 6.3.3 (j) — "search for the certificate on the complete CRL. If an entry
//!   is found that matches the certificate issuer and serial number … then set
//!   the cert_status variable to the indicated reason".
//!
//! **WAS WIR NICHT KÖNNEN, und warum wir dann ablehnen** (fail closed, nicht
//! „trotzdem prüfen"): `issuingDistributionPoint` (2.5.29.28) und
//! `deltaCRLIndicator` (2.5.29.27). Beide ändern den **Umfang** der Liste —
//! welche Aussteller und welche Gründe sie abdeckt. Ohne diese Auswertung
//! könnte eine Liste, die nur für einen Teil der Fälle gilt, als
//! vollständig missverstanden werden. Wir erkennen beide Erweiterungen und
//! geben [`CrlFehler::UmfangNichtUnterstuetzt`], statt sie zu übergehen.

use crate::der::{DerFehler, DerLeser, bit_string, integer_als_bytes, oid_als_punkte};
use crate::x509::{
    Name, SignaturAlgorithmus, X509Fehler, Zertifikat, algorithmus_lesen, key_usage_bit,
    key_usage_hat, name_lesen, zeit_seit_1970,
};

/// added 02.10.2026 06:1x: **Obergrenze für eine Sperrliste in Byte**, und die
/// hängt an einer Grenze, die schon existiert: `der::MAX_FELD` erlaubt kein
/// einzelnes Feld über 1 MiB, und das größte einzelne Feld einer Sperrliste ist
/// `revokedCertificates`. Deshalb ist 2 MiB die Grenze, **bei der die Liste
/// gerade noch lesbar ist** — 1 MiB Einträge plus Kopf, Erweiterungen und
/// Signatur. Wer eine größere Liste schickt, bekommt keinen Absturz, sondern
/// einen benannten Fehler.
pub const MAX_CRL: usize = 2 * 1024 * 1024;

/// added 02.10.2026 06:1x: **Obergrenze für die Zahl der Widerrufe**, gemessen
/// und nicht geraten: ein Eintrag in `testdaten/crl/widerrufen.der` ist **35
/// Byte** (`openssl asn1parse`, Offset 82, Länge 35 — Seriennummer 4, Datum
/// 15, Erweiterungen 14, Kopf 2). 1 MiB / 35 ≈ 29 900, deshalb 30 000. Für
/// mehr braucht es die Segmentierung nach 5.2.3, die wir nicht bauen.
///
/// added 02.10.2026 06:2x: UND DIE ABSICHTLICHE UNABHÄNGIGKEIT von
/// `der::MAX_ELEMENTE` (10 000): die Liste wird **nicht** mit
/// `DerLeser::container` gelesen, denn dessen Grenze würde die Zahl der
/// Einträge auf 10 000 begrenzen, ohne dass das hier jemand entschieden
/// hätte. Zwei Grenzen mit zwei Bedeutungen sind schlechter als eine.
pub const MAX_WIDERRUFE: usize = 30_000;

/// added 02.10.2026 06:2x: `id-ce-cRLReasons ::= { id-ce 21 }` (RFC 5280
/// Anhang A.1, Zeile 3871). Der Name im RFC hat ein **s**, der Wert ist 21.
pub const OID_WIDERRUFSGRUND: [u64; 4] = [2, 5, 29, 21];

/// added 02.10.2026 06:2x: `id-ce-issuingDistributionPoint ::= { id-ce 28 }`.
pub const OID_VERTEILUNGSSTELLE: [u64; 4] = [2, 5, 29, 28];

/// added 02.10.2026 06:2x: `id-ce-deltaCRLIndicator ::= { id-ce 27 }`.
pub const OID_DELTA: [u64; 4] = [2, 5, 29, 27];

/// added 02.10.2026 06:2x: `id-ce-cRLNumber ::= { id-ce 20 }`. Wird gelesen
/// und **nicht** ausgewertet: die Nummer beantwortet die Frage „ist dieses
/// Zertifikat widerrufen" nicht. Sie steht hier, damit man sie hat.
pub const OID_LISTENNUMMER: [u64; 4] = [2, 5, 29, 20];

/// added 02.10.2026 06:2x: `id-ce-authorityKeyIdentifier ::= { id-ce 35 }`.
/// added 02.10.2026 06:2x: **UND WARUM WIR IHN NICHT VERGLEICHEN:** 5.2.1
/// beschreibt ihn als „informational, nur bei mehreren Zertifikaten desselben
/// Ausstellers nötig". Die Signaturprüfung in [`pruefen`] ist strenger als ein
/// Schlüsselabgleich — sie kann nicht durch ein gleichnamiges Zertifikat
/// eines anderen Schlüssels bestehen.
pub const OID_SCHLUESSELKENNZEICHNUNG: [u64; 4] = [2, 5, 29, 35];

/// Warum eine Sperrliste nicht gebraucht werden kann.
///
/// added 02.10.2026 06:2x: **JEDER Fehler traegt seinen Grund**, weil der
/// Aufrufer sonst drei verschiedene „nicht brauchbar" nicht unterscheiden
/// kann: von einem anderen Server, von einer kaputten Kette und von einem
/// abgelaufenen Zeitfenster. Das ist der Grund, warum das kein `bool` ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrlFehler {
    /// Die Eingabe ist größer als [`MAX_CRL`].
    ZuGross { vorhanden: usize, erlaubt: usize },
    /// Mehr Widerrufe als [`MAX_WIDERRUFE`].
    ZuVieleWiderrufe { erlaubt: usize },
    /// Nach der letzten Sequenz stand noch etwas im Puffer.
    RestNachDerListe { uebrig: usize },
    /// Eine unbekannte `Version` (5.1.2.1 kennt v1 bis v3).
    VersionUnbekannt { wert: u8 },
    /// Der `tbsCertList`-Signaturalgorithmus weicht vom äußeren ab (5.1.1.2).
    AlgorithmusWeichtAb,
    /// Der Aussteller der Liste ist nicht der Aussteller des Zertifikats
    /// (6.3.3 (b)(1)).
    AusstellerPasstNicht,
    /// Die Liste trägt `issuingDistributionPoint` oder `deltaCRLIndicator`.
    UmfangNichtUnterstuetzt,
    /// `nextUpdate` fehlt (5.1.2.5).
    KeinNextUpdate,
    /// `jetzt` liegt außerhalb von `thisUpdate`..`nextUpdate` (6.3.3 (a)(1)).
    Zeitfenster { jetzt: i64, ab: i64, bis: i64 },
    /// Kein unterstützter Hash im Signaturalgorithmus.
    HashNichtUnterstuetzt,
    /// Das `cRLSign`-Bit ist nicht gesetzt (6.3.3 (f)).
    KeinCrlSign,
    /// Die Signatur passt nicht zum `tbsCertList` (6.3.3 (g)).
    SignaturPasstNicht,
    /// Der DER-Leser hat etwas abgelehnt.
    Der(DerFehler),
    /// `Name` oder Zeit sind im `x509`-Leser gescheitert.
    Zertifikat(X509Fehler),
}

impl std::fmt::Display for CrlFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrlFehler::ZuGross { vorhanden, erlaubt } => {
                write!(f, "Sperrliste {vorhanden} B, erlaubt {erlaubt} B")
            }
            CrlFehler::ZuVieleWiderrufe { erlaubt } => write!(f, "mehr Widerrufe als {erlaubt}"),
            CrlFehler::RestNachDerListe { uebrig } => {
                write!(f, "{uebrig} Byte hinter der Sperrliste")
            }
            CrlFehler::VersionUnbekannt { wert } => write!(f, "unbekannte CRL-Version {wert}"),
            CrlFehler::AlgorithmusWeichtAb => {
                write!(f, "Signaturalgorithmus weicht im tbsCertList ab (5.1.1.2)")
            }
            CrlFehler::AusstellerPasstNicht => write!(f, "Aussteller passt nicht zum Zertifikat"),
            CrlFehler::UmfangNichtUnterstuetzt => write!(
                f,
                "issuingDistributionPoint oder deltaCRLIndicator, Umfang nicht ausgewertet"
            ),
            CrlFehler::KeinNextUpdate => write!(f, "nextUpdate fehlt (5.1.2.5)"),
            CrlFehler::Zeitfenster { jetzt, ab, bis } => {
                write!(f, "Zeitfenster {ab}..{bis} deckt {jetzt} nicht")
            }
            CrlFehler::HashNichtUnterstuetzt => write!(f, "kein unterstützter Hash"),
            CrlFehler::KeinCrlSign => write!(f, "cRLSign nicht gesetzt (6.3.3 (f))"),
            CrlFehler::SignaturPasstNicht => write!(f, "Signatur der Liste passt nicht"),
            CrlFehler::Der(e) => write!(f, "DER: {e}"),
            CrlFehler::Zertifikat(e) => write!(f, "Zertifikat: {e}"),
        }
    }
}

impl std::error::Error for CrlFehler {}

impl From<DerFehler> for CrlFehler {
    fn from(wert: DerFehler) -> Self {
        CrlFehler::Der(wert)
    }
}

impl From<X509Fehler> for CrlFehler {
    fn from(wert: X509Fehler) -> Self {
        CrlFehler::Zertifikat(wert)
    }
}

/// Ein einzelner Widerruf aus `revokedCertificates` (5.1.2.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widerruf {
    /// `userCertificate`, als Bytes **ohne** führende Null.
    pub seriennummer: Vec<u8>,
    /// `revocationDate`, Sekunden seit 1970-01-01 00:00:00 UTC.
    pub am: i64,
    /// `reasonCode` aus den `crlEntryExtensions` (5.3.1), falls vorhanden.
    pub grund: Option<u8>,
}

impl Widerruf {
    /// added 02.10.2026 06:3x: **DER NAME DES GRUNDES**, und er ist nicht
    /// geraten, sondern die Aufzählung aus RFC 5280 5.3.1
    /// (`CRLReason ::= ENUMERATED`). Der Wert 7 ist laut RFC **nicht belegt**
    /// („value 7 is not used") — deshalb der eigene Arm statt einer Liste, in
    /// der er stillschweigend irgendeinen Namen bekäme.
    pub fn grund_name(&self) -> &'static str {
        match self.grund {
            None => "ohne Angabe",
            Some(0) => "unspecified",
            Some(1) => "keyCompromise",
            Some(2) => "cACompromise",
            Some(3) => "affiliationChanged",
            Some(4) => "superseded",
            Some(5) => "cessationOfOperation",
            Some(6) => "certificateHold",
            Some(7) => "Wert 7, nicht belegt (5.3.1)",
            Some(8) => "removeFromCRL",
            Some(9) => "privilegeWithdrawn",
            Some(10) => "aACompromise",
            Some(_) => "unbekannter Wert",
        }
    }
}

/// Eine gelesene Sperrliste.
///
/// added 02.10.2026 06:3x: **die Rohbytes bleiben**, weil die Signatur über
/// das `tbsCertList` **mit** seinem Tag läuft (5.1.1.3, analog zu 4.1.1.3 für
/// das Zertifikat). Wer die Felder einzeln nachrechnet, hätte nicht dieselben
/// Bytes und könnte die Signatur nicht prüfen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crl {
    /// Das `tbsCertList` **mit** Tag und Länge, unverändert.
    pub tbs_der: Vec<u8>,
    /// Der Inhalt des `signatureValue`-BIT STRING.
    pub signatur: Vec<u8>,
    /// Der äußere `signatureAlgorithm` (5.1.1.2).
    pub algorithmus: SignaturAlgorithmus,
    /// Der `signature`-Algorithmus **im** `tbsCertList` (5.1.2.2). Er muss
    /// gleich `algorithmus` sein; das wird in [`pruefen`] geprüft, weil es eine
    /// Eigenschaft der **Verwendung** ist und nicht des Lesens.
    pub algorithmus_im_tbs: SignaturAlgorithmus,
    /// Der `issuer` der Liste (5.1.2.3).
    pub aussteller: Name,
    /// `thisUpdate`, Sekunden seit 1970 (5.1.2.4).
    pub ab: i64,
    /// `nextUpdate` (5.1.2.5). `None`, wenn das Feld fehlt — dann ist die
    /// Liste bei uns nicht brauchbar, siehe [`CrlFehler::KeinNextUpdate`].
    pub bis: Option<i64>,
    /// `crlNumber` (5.2.3), falls vorhanden. Nur zum Nachschlagen.
    pub nummer: Option<Vec<u8>>,
    /// Die OIDs der Erweiterungen der Liste (5.1.2.7), unverändert.
    pub erweiterungen: Vec<Vec<u64>>,
    /// `revokedCertificates` (5.1.2.6).
    pub widerrufe: Vec<Widerruf>,
}

impl Crl {
    /// Steht die Seriennummer in der Liste?
    ///
    /// added 02.10.2026 06:3x: **DER VERGLEICH**, und es ist ein
    /// Bytengleichheitstest, kein Zahlenvergleich: beide Seiten kommen aus
    /// `der::integer_als_bytes`, das eine führende Null entfernt. Damit sind
    /// `02 00 11` (Zertifikat) und `00 02 00 11` (Liste) gleich — und das ist
    /// richtig, denn DER verlangt minimale Kodierung (X.690 8.3.2), und
    /// `00 02 00 11` wäre eine **andere** Kodierung derselben Zahl. Wer hier
    /// roh vergleicht, fiele je nach Kodierung durch oder durch nicht.
    pub fn widerruft(&self, seriennummer: &[u8]) -> Option<&Widerruf> {
        self.widerrufe
            .iter()
            .find(|w| w.seriennummer == seriennummer)
    }

    /// Deckt das Zeitfenster `jetzt` ab?
    ///
    /// added 02.10.2026 06:3x: beide Grenzen sind **einschließlich** — 4.1.2.5
    /// sagt das ausdrücklich für `Validity`, und 6.3.3 (a)(1) fordert für
    /// `nextUpdate` nur, dass die Zeit **vor** dem Wert liegt; am Stichtag
    /// selbst ist die Liste also noch zu gebrauchen. Fehlt `nextUpdate`, gilt
    /// sie **nicht** als brauchbar; 5.1.2.5 macht das Feld zur Pflicht.
    pub fn gueltig(&self, jetzt: i64) -> bool {
        match self.bis {
            None => false,
            Some(bis) => jetzt >= self.ab && jetzt <= bis,
        }
    }
}

/// Liest eine Sperrliste aus DER-Bytes.
///
/// added 02.10.2026 06:3x: **DAS DREI-TEILIGE**, das der Struct nicht zeigt:
/// `CertificateList ::= SEQUENCE { tbsCertList, signatureAlgorithm,
/// signatureValue }` (5.1). Die `tbsCertList` wird **roh** übernommen
/// (`.roh`, mit Tag und Länge), weil die Signatur darüber läuft.
/// added 03.10.2026 23:az, R66. **`pub(crate)`, weil `x509::muster_pruefer` ihn
/// braucht** — und das ist **kein** Komfort, sondern der Grund, warum es den
/// Test gibt: der Musterprüfer vergleicht die **richtige** und die **falsche**
/// Form einer Sperrliste, und dafür braucht er den Bauer. Vorher war
/// `mod bauen` privat, also **pro Crate** — und `x509.rs` ist eine **andere**
/// Datei derselben Crate. `pub(crate)` ist die kleinste Sichtbarkeit, die das
/// erlaubt, und sie gilt nur unter `cfg(test)`.
#[cfg(test)]
pub(crate) mod bauen {
    use crate::x509::bauen::{
        erweiterung, gueltigkeit, integer, name, oid, sequenz, signiere, spki, tlv,
    };

    /// added 03.10.2026 19:3x, R62. **DER SPERRLISTEN-BUER, UND SEINE
    /// BEGRUENDUNG IST EIN FEHLENDER BEWEIS, NICHT EIN SCHOENERER BAU.**
    ///
    /// added 03.10.2026 19:3x: **WAS OHNE IHN NICHT GEHT.** R61 hat den harten
    /// Fehlerpfad gebaut (`NetError::ZertifikatWiderrufen`) und **zugleich
    /// gemeldet**, dass er **ungetestet** ist: `testdaten/crl/widerrufen.der`
    /// enthaelt genau **eine** Seriennummer, die zu `widerrufen.pem` gehoert —
    /// und die gehoert zu **fremden** Testdaten. Fuer den **Client-Weg** brauche
    /// ich aber ein Zertifikat, dessen Verteilungspunkt auf einen **lokalen**
    /// Server zeigt und dessen Seriennummer in **meiner** Liste steht. Beides
    /// zugleich geht nur mit einem Bauer.
    ///
    /// added 03.10.2026 19:3x: **UND `crl.rs` HAT NUR EINEN LESER.** GEMESSEN:
    /// `grep` nach `pub fn` in `crl.rs` findet `crl_lesen`, `pruefen`,
    /// `beurteile` und die drei `Urteil`-Hilfen — **keinen** Bauer. Das ist
    /// keine Schoenheitsfrage: R59 hatte `crl.rs` als „11 Felder, null
    /// Konsumenten“ gemessen, und R61 hat die Konsumenten gebaut. Jetzt fehlt
    /// die **Gegenseite**.
    ///
    /// added 03.10.2026 19:3x: **DASS ER `#[cfg(test)]` IST**, folgt dem
    /// Zertifikat-Bauer (R47/R48): er ist eine Beweishilfe, keine
    /// Bibliotheksleistung. Wer eine Sperrliste **erzeugen** will, hat OpenSSL.
    ///
    /// added 03.10.2026 19:3x: **DIE ZEIT ALS TEXT, NICHT ALS SEKUNDENZAHL —
    /// und der Grund ist die Rundprobe.** Fuer `UTCTime` (X.690 11.8, RFC 5280
    /// 4.1.2.5.1) braucht man eine Kalenderrechnung. Die gibt es in der
    /// Bibliothek **nicht**: `zeit_seit_1970` **liest**, es schreibt nicht
    /// (GEMESSEN: `grep "fn zeit"` findet genau eine Funktion). Eine
    /// Handrechnung von Tagen und Schaltjahren in einen Test zu schreiben, wäre
    /// eine **zweite** Kalenderimplementierung — genau die Form, die R60 als
    /// Fehler benannt hat („ein Aufbau, zweimal gebaut“). Also bekommt der
    /// Bauer den **Text** (`"260101000000Z"`), und die Rückrichtung prüft der
    /// Leser. Die **Sollwerte** sind unabhängig gemessen (GNU `date -u -d`):
    /// 2026-01-01 → **1767225600**, 2026-06-15 12:30:45 → **1781526645**,
    /// 2027-01-01 → **1798761600**.
    pub fn zeit(text: &str) -> Vec<u8> {
        tlv(0x17, text.as_bytes())
    }

    /// added 03.10.2026 19:3x: **EIN WIDERRUFSEINTRAG**, und die Form ist
    /// `SEQUENCE { userCertificate INTEGER, revocationDate Time,
    /// crlEntryExtensions OPTIONAL }` (RFC 5280 5.3). **OHNE** die
    /// Erweiterungen, und das ist Absicht: der Leser liest sie **optional**
    /// (`widerrufe_lesen`, Zeile 419), und ein Bauer, der sie immer schreibt,
    /// wuerde den Zweig „mit Grund“ **nie** pruefen.
    pub fn eintrag(seriennummer: &[u8], am: &str) -> Vec<u8> {
        sequenz(&[integer(seriennummer), zeit(am)])
    }

    /// added 03.10.2026 19:3x: **EINE LISTE MIT GENAU EINEM WIDERRUF**, und
    /// `v2` (5.1.2.1) wird **geschrieben**, weil der Leser es liest und eine
    /// Liste ohne Versionsfeld als v1 auffaesst — die Behauptung „v2“
    /// waere dann nicht in den Bytes.
    ///
    /// added 03.10.2026 19:4x: **DER SIGNATURRAHMEN IST DER GLEICHE WIE BEIM
    /// ZERTIFIKAT — und das ist gemessen, nicht angenommen.** `CertificateList`
    /// (5.1) und `Certificate` (4.1) haben **beide** die Form
    /// `SEQUENCE { tbs, signatureAlgorithm, signatureValue }`; der
    /// Unterschied steckt **ausschliesslich** im `tbs`. Also ruft der Bauer
    /// `x509::bauen::signiere` auf, statt die zweite Signatur zu schreiben:
    /// `SEQUENCE { [0] INTEGER 1, AlgorithmIdentifier, issuer Name,
    /// thisUpdate, nextUpdate, revokedCertificates }`.
    pub fn liste_mit_einem_widerruf(
        seriennummer: &[u8],
        aussteller: &str,
        ab: &str,
        bis: &str,
    ) -> Result<Vec<u8>, &'static str> {
        // ecdsa-with-SHA256, dieselbe OID wie im Zertifikat-Bauer
        let alg = sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]);
        let tbs = sequenz(&[
            // added 03.10.2026 19:8x, R62. **`version` IST HIER EIN PLAIN
            // INTEGER (Tag 0x02), NICHT `[0] EXPLICIT` (Tag 0x0a)** — und das ist
            // **GEMESSEN**, nicht geraten: der Leser meldete
            // `TagFalsch { erwartet: 48, bekommen: 10 }`, alsoexpects 0x30 und
            // bekam 0x0a, weil er nach `version` den `AlgorithmIdentifier` des
            // `tbsCertList` an der Stelle las, an der mein 0x0a stand.
            //
            // added 03.10.2026 19:8x: **UND DAS IST DIE ZWEITE KODIERUNG
            // DESSELBEN FELDNAMENS IN DEMSELBEN ASN.1-MODUL.** RFC 5280
            // 4.1.2.1 schreibt fuer das Zertifikat `version [0] EXPLICIT
            // Version DEFAULT v1`, 5.1.2.1 fuer die Liste `version Version
            // OPTIONAL` — und `Version ::= INTEGER`. Wer den einen Wert im
            // anderen Aufbau wiederverwendet, bekommt **beides** falsch: im
            // Zertifikat ein zusaetzliches Schalen-Byte, in der Liste ein
            // fehlendes. Der Zertifikat-Bauer in `x509::bauen` benutzt
            // darum `tlv(0xa0, &integer(..))`, dieser hier `integer(..)`.
            integer(&[0x01]), // version v2
            alg,
            name(aussteller),
            zeit(ab),
            zeit(bis),
            sequenz(&[eintrag(seriennummer, ab)]), // revokedCertificates
        ]);
        signiere(&tbs)
    }

    /// added 03.10.2026 19:4x: **DIE KETTE ZUSAMMEN, WEIL GETRENNT NICHTS
    /// BEWEIST.** Diese Funktion liefert `(blatt_der, crl_der)` und ist der
    /// Kern von R62: **beide** Teile muessen aus **einem** Schluessel stammen,
    /// sonst prueft der Test zwei fremde Dinge gegeneinander. Die Seriennummer
    /// ist ein Parameter, damit der Test **zwei** Zertifikate bauen kann, von
    /// denen nur eines in der Liste steht — der **Gegenfall**, ohne den der
    /// Test auch von einem Abruf erfuellt waere, der alles zurueckgibt.
    pub fn blatt_und_liste(
        seriennummer_blatt: &[u8],
        seriennummer_widerrufen: &[u8],
        verteilungsstellen_url: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
        let blatt = crate::x509::bauen::blatt_mit_verteilungsstellen_zu(
            seriennummer_blatt,
            verteilungsstellen_url,
        )?;
        let liste = liste_mit_einem_widerruf(
            seriennummer_widerrufen,
            crate::x509::bauen::CA_NAME,
            "260101000000Z",
            "270101000000Z",
        )?;
        Ok((blatt, liste))
    }

    /// added 03.10.2026 19:7x, R62. **DIE DAZUGEHOERIGE CA, und sie ist noetig,
    /// weil `beurteile` den Aussteller zum Pruefen der Signatur braucht**
    /// (R60, Befund 2). `x509::bauen` hat **mehrere** CA-Bauer
    /// (`ca_mit_dirname_auflage`, `zertifikat_ca_mit_auflage`), aber **keinen
    /// einfachen** — und R62 braucht genau einen: Subject = Aussteller =
    /// `CA_NAME`, `cA = TRUE`, **ohne** Auflagen, weil eine Auflage nichts zum
    /// Thema beitraegt und den Bau nur kompliziert.
    pub fn ca() -> Result<Vec<u8>, &'static str> {
        let tbs = sequenz(&[
            tlv(0xa0, &integer(&[0x02])),
            integer(&[0x01]),
            sequenz(&[oid(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02])]),
            name(crate::x509::bauen::CA_NAME),
            gueltigkeit(),
            name(crate::x509::bauen::CA_NAME), // selbstsigniert
            spki(),
            tlv(
                0xa3,
                &tlv(
                    0x30,
                    &erweiterung(&[0x55, 0x1d, 0x13], &sequenz(&[tlv(0x01, &[0xff])])),
                ),
            ),
        ]);
        signiere(&tbs)
    }
}

#[cfg(test)]
mod bauen_tests {
    use super::bauen::{blatt_und_liste, ca, liste_mit_einem_widerruf, zeit};
    use crate::crl::{beurteile, crl_lesen};
    use crate::x509::zertifikat_lesen;

    /// added 03.10.2026 19:6x, R62. **DER BAU LÄSST SICH WIEDER LESEN — und das
    /// ist die **erste** und wichtigste Probe.** Ein Bauer, dessen Ausgabe der
    /// eigene Leser nicht liest, beweist gar nichts: dann waere der Test „der
    /// Leser kann die Liste“ und nicht „der Bauer kann eine Liste“.
    #[test]
    fn die_gebaute_liste_liest_sich_wieder() {
        let der = liste_mit_einem_widerruf(
            &[0x2a, 0x03, 0x04],
            "Test-CA",
            "260101000000Z",
            "270101000000Z",
        )
        .unwrap_or_else(|e| panic!("bauen: {e}"));
        let liste = crl_lesen(&der).unwrap_or_else(|e| panic!("lesen: {e:?}"));
        assert_eq!(
            liste.widerrufe.len(),
            1,
            "die Liste hat {} Eintraege",
            liste.widerrufe.len()
        );
        assert_eq!(liste.widerrufe[0].seriennummer, vec![0x2a, 0x03, 0x04]);
    }

    /// added 03.10.2026 19:6x, R62. **DIE ZEITEN, MIT **UNABHÄNGIG** GEMESSENEN
    /// SOLLWERTEN.** Die Sollwerte stammen aus GNU `date -u -d`, **nicht** aus
    /// der Bibliothek: 2026-01-01 → 1767225600, 2026-06-15 12:30:45 →
    /// 1781526645, 2027-01-01 → 1798761600. Ein Sollwert aus der eigenen
    /// Rechnung waere ein Zirkelschluss — der Leser wuerde sich selbst bestaetigen.
    #[test]
    fn die_zeiten_stimmen_mit_den_gemessenen_werten() {
        for (text, soll) in [
            ("260101000000Z", 1_767_225_600_i64),
            ("260615123045Z", 1_781_526_645_i64),
            ("270101000000Z", 1_798_761_600_i64),
        ] {
            let wert = crate::x509::zeit_seit_1970(0x17, &zeit(text)[2..])
                .unwrap_or_else(|e| panic!("{text}: {e:?}"));
            assert_eq!(wert, soll, "{text} wurde {wert}, erwartet {soll}");
        }
    }

    /// added 03.10.2026 19:6x, R62. **DER GEGENFALL, UND ER IST DER WICHTIGSTE
    /// TEST DER RUNDE.** Es werden **zwei** Listen aus **einem** Schluessel und
    /// **einem** Blatt gebaut; sie unterscheiden sich **nur** darin, welche
    /// Seriennummer sie nennen. Die eine muss **durchfallen** lassen, die andere
    /// **durchlassen**.
    ///
    /// added 03.10.2026 19:6x: **WARUM BEIDES IN EINEM TEST.** R59s
    /// `verteilungsstellen_als_texte` lieferte **immer** leer und kein Test sah
    /// es, weil jeder Test nur eine Richtung geprueft hatte. Zwei Richtungen aus
    /// **einem** Aufbau sind der Preis dafuer, dass „durchgelassen“ und
    /// „durchgefallen“ **verschiedene** Aussagen sind.
    #[test]
    fn nur_der_widerrufene_faellt_durch() {
        const SERIE: &[u8] = &[0x2a, 0x03, 0x04];
        const ANDERE: &[u8] = &[0x09, 0x09, 0x09];
        let (blatt_a, liste_trifft) = blatt_und_liste(SERIE, SERIE, "http://127.0.0.1:1/x.crl")
            .unwrap_or_else(|e| panic!("bauen A: {e}"));
        let (blatt_b, liste_verfehlt) = blatt_und_liste(SERIE, ANDERE, "http://127.0.0.1:1/x.crl")
            .unwrap_or_else(|e| panic!("bauen B: {e}"));
        assert_eq!(
            blatt_a, blatt_b,
            "die beiden Blaetter muessen identisch sein — sonst vergleicht der Test zwei Dinge"
        );

        let blatt = zertifikat_lesen(&blatt_a).unwrap_or_else(|e| panic!("blatt: {e:?}"));
        let ca_der = ca().unwrap_or_else(|e| panic!("ca bauen: {e}"));
        let aussteller = zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("ca lesen: {e:?}"));

        // added 03.10.2026 19:6x: **DIE ZEIT IST GEMESSEN, NICHT GEWAEHLT:**
        // 1781526645 ist `date -u -d "2026-06-15 12:30:45" +%s`, und es liegt
        // **zwischen** `ab` (1767225600) und `bis` (1798761600) der gebauten
        // Liste. Ein `jetzt` ausserhalb wuerde `Zeitfenster` liefern und der
        // Test wuerde die **falsche** Sache messen.
        let jetzt = 1_781_526_645_i64;

        let getroffen = beurteile(&liste_trifft, &blatt, &aussteller, jetzt);
        assert!(
            getroffen.ist_widerrufen(),
            "die Liste nennt die Nummer des Blatts und es fiel nicht durch: {:?}",
            getroffen.grund()
        );
        let verfehlt = beurteile(&liste_verfehlt, &blatt, &aussteller, jetzt);
        assert_eq!(
            verfehlt,
            crate::crl::Urteil::NichtWiderrufen,
            "die Liste nennt eine andere Nummer und liess das Blatt fallen"
        );
    }
}

pub fn crl_lesen(daten: &[u8]) -> Result<Crl, CrlFehler> {
    if daten.len() > MAX_CRL {
        return Err(CrlFehler::ZuGross {
            vorhanden: daten.len(),
            erlaubt: MAX_CRL,
        });
    }
    let mut leser = DerLeser::neu(daten);
    let liste = leser.element_mit_tag(0x30)?;
    // added 02.10.2026 06:4x: **KEIN BYTE ZU VIEL.** Nach der letzten
    // Sequenz muss Schluss sein. Ohne diese Prüfung nähme ein Leser auch eine
    // Liste an, an die etwas angehängt wurde — und weil die Signatur nur über
    // den `tbsCertList`-Teil läuft, wäre das nicht einmal sichtbar.
    if !leser.ist_leer() {
        return Err(CrlFehler::RestNachDerListe {
            uebrig: leser.uebrig(),
        });
    }

    let mut l = DerLeser::neu(&liste.inhalt);
    let tbs = l.element_mit_tag(0x30)?;
    let algorithmus = algorithmus_lesen(&mut l)?;
    // added 02.10.2026 06:4x: DER BIT STRING braucht eine eigene Variable:
    // `bit_string` leiht sich die Bytes aus (`fn bit_string(inhalt: &[u8]) ->
    // Result<(u8, &[u8])>`), und ein `&temporaer[..]` wird am Zeilenende
    // freigegeben (E0716). Deshalb steht das Element hier eine Zeile ueber
    // seinem Benutzer — nicht aus Pedanterie, sondern weil der Uebersetzer
    // sonst genau das meldet, was er gemeldet hat.
    let sig_element = l.element_mit_tag(0x03)?;
    let (_ungenutzt, signatur) = bit_string(&sig_element.inhalt)?;
    // added 02.10.2026 06:4x: und nach der Signatur ist Schluss — sonst
    // wären vier Felder erlaubt, wo die Norm drei nennt.
    if !l.ist_leer() {
        return Err(CrlFehler::RestNachDerListe { uebrig: l.uebrig() });
    }
    let tbs_der = tbs.roh;

    let mut t = DerLeser::neu(&tbs.inhalt);
    // 5.1.2.1 Version, OPTIONAL. Fehlt es, ist es v1 (Wert 0). Wir verlangen
    // **kein** v2: `openssl` schreibt hier `01`, andere Listen lassen das Feld
    // weg, und 5.1.2.1 sagt nur, was der Aussteller **muss**, wenn er es
    // schreibt. Einen Wert, den es nicht gibt, lehnen wir ab.
    let mut version = 0u8;
    if t.naechste_tag() == Some(0x02) {
        let v = t.element_mit_tag(0x02)?;
        let bytes = integer_als_bytes(&v.inhalt)?;
        version = bytes.first().copied().unwrap_or(0);
    }
    if version > 2 {
        return Err(CrlFehler::VersionUnbekannt { wert: version });
    }
    let algorithmus_im_tbs = algorithmus_lesen(&mut t)?;
    let aussteller = name_lesen(&mut t)?;
    let tu = t.element()?;
    let ab = zeit_seit_1970(tu.tag, &tu.inhalt)?;

    // added 02.10.2026 06:4x: DIE DREI OPTIONALEN FELDER, und ihre Reihenfolge
    // ist die DER-Reihenfolge aus 5.1.2.5–5.1.2.7, nicht eine Wunschliste:
    // `nextUpdate`, dann `revokedCertificates`, dann `crlExtensions [0]`.
    let mut bis = None;
    let mut widerrufe = Vec::new();
    let mut nummer = None;
    let mut erweiterungen = Vec::new();
    while !t.ist_leer() {
        match t.naechste_tag() {
            Some(0x17) | Some(0x18) => {
                let n = t.element()?;
                bis = Some(zeit_seit_1970(n.tag, &n.inhalt)?);
            }
            Some(0x30) => {
                let block = t.element_mit_tag(0x30)?;
                widerrufe = widerrufe_lesen(&block.inhalt)?;
            }
            Some(0xA0) => {
                let klammer = t.element_mit_tag(0xA0)?;
                (nummer, erweiterungen) = crl_erweiterungen_lesen(&klammer.inhalt)?;
            }
            _ => {
                let feld = t.element()?;
                return Err(DerFehler::InhaltUnbekannt { tag: feld.tag }.into());
            }
        }
    }

    Ok(Crl {
        tbs_der,
        signatur: signatur.to_vec(),
        algorithmus,
        algorithmus_im_tbs,
        aussteller,
        ab,
        bis,
        nummer,
        erweiterungen,
        widerrufe,
    })
}

/// Liest `revokedCertificates` (5.1.2.6).
fn widerrufe_lesen(inhalt: &[u8]) -> Result<Vec<Widerruf>, CrlFehler> {
    let mut leser = DerLeser::neu(inhalt);
    let mut raus: Vec<Widerruf> = Vec::new();
    while !leser.ist_leer() {
        // added 02.10.2026 06:5x: DIE GRENZE, und sie wird **vor** dem Lesen
        // geprüft: eine Liste mit zehn Millionen Einträgen soll an dieser
        // Zeile scheitern und nicht daran, dass der Speicher vollgeht.
        if raus.len() >= MAX_WIDERRUFE {
            return Err(CrlFehler::ZuVieleWiderrufe {
                erlaubt: MAX_WIDERRUFE,
            });
        }
        let eintrag = leser.element_mit_tag(0x30)?;
        let mut e = DerLeser::neu(&eintrag.inhalt);
        let seriennummer = integer_als_bytes(&e.element_mit_tag(0x02)?.inhalt)?;
        let d = e.element()?;
        let am = zeit_seit_1970(d.tag, &d.inhalt)?;
        let mut grund = None;
        if !e.ist_leer() {
            // crlEntryExtensions (5.3)
            let ext = e.element_mit_tag(0x30)?;
            if erweiterungs_oids(&ext.inhalt)?
                .iter()
                .any(|o| *o == OID_WIDERRUFSGRUND)
            {
                grund = widerrufsgrund_lesen(&ext.inhalt)?;
            }
        }
        raus.push(Widerruf {
            seriennummer,
            am,
            grund,
        });
    }
    Ok(raus)
}

/// added 02.10.2026 06:5x: DER WIDERRUFSGRUND (5.3.1). Der Weg ist
/// `Extensions` -> `Extension` -> `extnValue` (OCTET STRING), und **im**
/// OCTET STRING steht noch einmal DER — diesmal `CRLReason ::= ENUMERATED`,
/// also Tag 0x0A. Wer das letzte DER übersieht, hält den OCTET-STRING selbst
/// für den Grund. GEMESSEN in `testdaten/crl/widerrufen.der` (`asn1parse`,
/// Offset 114): der OCTET STRING enthält `0A 01 01`, also Grund 1.
fn widerrufsgrund_lesen(inhalt: &[u8]) -> Result<Option<u8>, CrlFehler> {
    let mut leser = DerLeser::neu(inhalt);
    while !leser.ist_leer() {
        let ext = leser.element_mit_tag(0x30)?;
        let mut e = DerLeser::neu(&ext.inhalt);
        let oid = oid_als_punkte(&e.element_mit_tag(0x06)?.inhalt)?;
        // `critical BOOLEAN DEFAULT FALSE` darf zwischen OID und Wert stehen.
        if e.naechste_tag() == Some(0x01) {
            let _ = e.element()?;
        }
        let wert = e.element_mit_tag(0x04)?;
        if oid == OID_WIDERRUFSGRUND {
            let mut w = DerLeser::neu(&wert.inhalt);
            let v = w.element_mit_tag(0x0A)?;
            let bytes = integer_als_bytes(&v.inhalt)?;
            return Ok(bytes.first().copied());
        }
    }
    Ok(None)
}

/// added 02.10.2026 06:5x: **nur die OIDs** der Erweiterungen. Der Grund
/// wird beim Lesen einer **Liste** nicht gebraucht — er steht am **Eintrag**,
/// nicht an der Liste —, aber die Liste selbst muss erkennen können, ob sie
/// eine `issuingDistributionPoint`-Erweiterung trägt: das entscheidet in
/// [`pruefen`] über „können wir das".
fn erweiterungs_oids(inhalt: &[u8]) -> Result<Vec<Vec<u64>>, CrlFehler> {
    let mut leser = DerLeser::neu(inhalt);
    let mut raus = Vec::new();
    while !leser.ist_leer() {
        let ext = leser.element_mit_tag(0x30)?;
        let mut e = DerLeser::neu(&ext.inhalt);
        raus.push(oid_als_punkte(&e.element_mit_tag(0x06)?.inhalt)?);
    }
    Ok(raus)
}

/// added 02.10.2026 12:5x (Projektlabel 02.10.2026), Rolle TLS: **DER TYP ALS
/// ALIAS**, weil clippy `type_complexity` meldet und die Meldung berechtigt ist:
/// `(Option<Vec<u8>>, Vec<Vec<u64>>)` sagt an der Rueckgabe **nichts** darueber,
/// welche Zahl welche Bedeutung hat. `CrlErweiterungen` benennt die Form und
/// laesst die beiden Anteile benennbar, ohne die Signatur zu verbiegen.
type CrlErweiterungen = (Option<Vec<u8>>, Vec<Vec<u64>>);

/// Liest `crlExtensions` [0] EXPLICIT (5.1.2.7): die OIDs und — wenn die
/// `crlNumber` dabei ist (5.2.3) — deren Wert.
fn crl_erweiterungen_lesen(inhalt: &[u8]) -> Result<CrlErweiterungen, CrlFehler> {
    // added 02.10.2026 06:5x: die `[0]`-Klammer ist ein Element, das die
    // `Extensions`-SEQUENCE umschließt; deshalb wird es hier abgenommen und
    // sein Inhalt weitergereicht.
    let mut leser = DerLeser::neu(inhalt);
    let innen = leser.element_mit_tag(0x30)?;
    let mut nummer = None;
    let mut oids: Vec<Vec<u64>> = Vec::new();
    let mut l = DerLeser::neu(&innen.inhalt);
    while !l.ist_leer() {
        let ext = l.element_mit_tag(0x30)?;
        let mut e = DerLeser::neu(&ext.inhalt);
        let oid = oid_als_punkte(&e.element_mit_tag(0x06)?.inhalt)?;
        if e.naechste_tag() == Some(0x01) {
            let _ = e.element()?;
        }
        let wert = e.element_mit_tag(0x04)?;
        oids.push(oid.clone());
        if oid == OID_LISTENNUMMER {
            let mut w = DerLeser::neu(&wert.inhalt);
            let n = w.element_mit_tag(0x02)?;
            nummer = Some(integer_als_bytes(&n.inhalt)?);
        }
    }
    Ok((nummer, oids))
}

/// **Prüft eine Liste gegen ein Zertifikat** — die einzige Frage, die dieses
/// Modul beantwortet.
///
/// added 02.10.2026 06:6x: DIE REIHENFOLGE, und sie folgt 6.3.3, nicht dem
/// Aufwand: erst der **Aussteller** (b)(1), dann der **Umfang** (b)(2)/(c),
/// dann das **Zeitfenster** (a)(1), dann der `cRLSign`-Schlüsselgebrauch (f),
/// dann die **Signatur** (g). Warum gerade so: ein Fehler an einer frühen
/// Stelle ist billiger zu erkennen und sagt mehr als ein `SignaturPasstNicht`
/// am Ende — wenn die Liste gar nicht von dieser CA ist, ist ihre Signatur mit
/// dem Schlüssel dieser CA zu prüfen sinnlos.
/// added 03.10.2026 16:f0, R60. **`pruefen` BRAUCHT ZWEI ZERTIFIKATE, UND DIE
/// EINZIGE BISHERIGE FORM KONNTE IN DER WILD NICHT FUNKTIONIEREN.** GEMESSEN
/// (16:eg, `xkcd.com`): `beurteile` lieferte `Unbrauchbar(AusstellerPasstNicht)`
/// fuer eine CRL, deren Aussteller **exakt** der Aussteller des Blattes ist
/// (`openssl`: „C=BE, O=GlobalSign nv-sa, CN=GlobalSign Atlas R3 DV TLS CA 2025 Q4“
/// in Blatt und CRL gleich).
///
/// added 03.10.2026 16:f1: **DIE URSACHE, und sie ist ein Verwechslung von
/// „Aussteller“ mit „dem Zertifikat“.** Der Vergleich war
/// `crl.aussteller != zertifikat.aussteller`, und `beurteile` uebergab den
/// **Aussteller** als `zertifikat`. Damit wurde die CRL der Zwischen-CA gegen
/// den **Aussteller der Zwischen-CA** geprueft — das ist die Wurzel, und die
/// stimmt natuerlich nie.
///
/// added 03.10.2026 16:f2: **DIE REGEL DER NORMSPRACHE.** RFC 5280 6.3.3 (b)(1)
/// sagt woertlich „verify that the **CRL issuer** matches the **certificate
/// issuer**“. Das „certificate“ ist das Zertifikat, dessen Status geprueft wird —
/// das **Blatt**. Also (b)(1) gehoert **immer** gegen das Blatt.
///
/// added 03.10.2026 16:f3: **UND (f)/(g) GEHOEREN GEGEN DIE CA.** (f) fragt das
/// `cRLSign`-Bit „**in the CRL issuer's certificate**“ und (g) prueft die
/// Signatur mit dem Schluessel **derselben** CA. Das ist der Aussteller. Die
/// beiden Regeln brauchen **zwei** Zertifikate — und die alte Form konnte nur
/// eine davon richtig bedienen, je nachdem was der Aufrufer uebergab. Das ist
/// keine Signaturfrage, sondern eine **Frage nach dem Wortlaut**.
pub fn pruefen(
    crl: &Crl,
    blatt: &Zertifikat,
    aussteller: &Zertifikat,
    jetzt: i64,
) -> Result<(), CrlFehler> {
    // 6.3.3 (b)(1): „verify that the CRL issuer matches the certificate issuer“ —
    // der Aussteller des **Blattes**, verglichen nach 7.1 (R57/R58), also mit
    // StringPrep und `caseIgnoreMatch`, **nicht** byteweise. added 03.10.2026
    // 16:f4: **WARUM NICHT `!=`.** Ein CRL-Aussteller wird von der CA geschrieben
    // und steht ein zweites Mal im Zertifikat, das diese CA ausgestellt hat —
    // beide Male koennen unterschiedliche Zeichenkodierungen (PrintableString
    // neben UTF8String) oder Gross-/Kleinschreibung im Weg stehen. GEMESSEN ist
    // der Byte-Vergleich hier **nicht** gescheitert, aber er ist damit
    // **unbewiesen**, und der Nachweis waere eine CA mit abweichender
    // Schreibweise. `namen_gleich_7_1` ist die Regel, die 7.1 sagt; sie zu
    // benutzen kostet nichts und ersetzt eine Vermutung.
    if !crate::kette::namen_gleich_7_1(&crl.aussteller, &blatt.aussteller) {
        return Err(CrlFehler::AusstellerPasstNicht);
    }
    // 5.1.1.2: der Algorithmus muss innen wie außen sein.
    if crl.algorithmus != crl.algorithmus_im_tbs {
        return Err(CrlFehler::AlgorithmusWeichtAb);
    }
    // added 02.10.2026 06:6x: DER UMFANG, und hier wird **abgelehnt** statt
    // übergangen: 6.3.3 (b)(2) verlangt bei `issuingDistributionPoint` eine
    // Prüfung von `distributionPointName` und `onlyContains…`, und (c)/(d)
    // verlangen dasselbe für Delta-Listen. Ohne diese Auswertung würde eine
    // Liste, die nur für einen Teil der Fälle gilt, als vollständig gelesen —
    // und genau daran hängen `certificateHold` und `removeFromCRL` (5.3.1),
    // die nicht dasselbe bedeuten wie ein endgültiger Widerruf.
    for oid in &crl.erweiterungen {
        if *oid == OID_VERTEILUNGSSTELLE || *oid == OID_DELTA {
            return Err(CrlFehler::UmfangNichtUnterstuetzt);
        }
    }
    // 5.1.2.5 und 6.3.3 (a)(1).
    let bis = match crl.bis {
        None => return Err(CrlFehler::KeinNextUpdate),
        Some(b) => b,
    };
    if jetzt < crl.ab || jetzt > bis {
        return Err(CrlFehler::Zeitfenster {
            jetzt,
            ab: crl.ab,
            bis,
        });
    }
    // 6.3.3 (f): "If a key usage extension is present in the CRL issuer's
    // certificate, verify that the cRLSign bit is set."
    if let Some(ku) = aussteller.key_usage {
        if !key_usage_hat(ku, key_usage_bit::WIDERRUF_SIGNIEREN) {
            return Err(CrlFehler::KeinCrlSign);
        }
    }
    // 6.3.3 (g): die Signatur über das `tbsCertList`.
    let hash = match crl.algorithmus.hash {
        None => return Err(CrlFehler::HashNichtUnterstuetzt),
        Some(h) => h,
    };
    crate::kette::signatur_ueber(
        &crl.tbs_der,
        &crl.signatur,
        &crl.algorithmus.oid,
        hash,
        crl.algorithmus.pss_salt,
        &aussteller.schluessel,
    )
    .map_err(|_| CrlFehler::SignaturPasstNicht)
}

/// Das Ergebnis einer Prüfung, **ohne** Wahrheitswert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Urteil {
    /// Die Liste ist echt, aktuell, von der richtigen CA — und das Zertifikat
    /// steht **nicht** darin.
    NichtWiderrufen,
    /// Die Liste ist echt und aktuell, und das Zertifikat steht mit diesem
    /// Grund darin.
    Widerrufen(Box<Widerruf>),
    /// Die Liste lässt sich nicht gebrauchen; der Grund steht drin.
    Unbrauchbar(CrlFehler),
}

impl Urteil {
    /// added 02.10.2026 06:7x: DIE DREI FRAGEN, die ein Aufrufer stellt, und
    /// die drei Antworten, die er braucht. Bewusst keine `bool`-Methoden:
    /// „ist es widerrufen" und „weiß ich es überhaupt" sind verschiedene Fragen,
    /// und wer sie verwechselt, hält eine fehlende Sperrliste für ein gültiges
    /// Zertifikat.
    pub fn ist_widerrufen(&self) -> bool {
        matches!(self, Urteil::Widerrufen(_))
    }
    /// Konnte die Liste benutzt werden?
    pub fn ist_brauchbar(&self) -> bool {
        !matches!(self, Urteil::Unbrauchbar(_))
    }
    /// Warum nicht, wenn nicht.
    pub fn grund(&self) -> Option<&CrlFehler> {
        match self {
            Urteil::Unbrauchbar(f) => Some(f),
            _ => None,
        }
    }
}

/// added 02.10.2026 06:7x: **DER WEG IN EINEM AUFRUF**, und er ist kurz genug,
/// dass er in die Kette gehört statt in jeden Aufrufer: Bytes lesen, gegen die
/// CA prüfen, und wenn beides gelingt, die Seriennummer des Blattes nachsehen.
///
/// added 02.10.2026 08:1x: **ZWEI ZERTIFIKATE, UND DAS WAR EIN FEHLER:**
/// die erste Fassung nahm **eines** und benutzte dessen Schlüssel für die
/// Signaturprüfung. Damit prüfte sie die Liste mit dem Schlüssel des **Blattes**
/// statt mit dem der CA — richtig wäre nur, wenn das Blatt selbstsigniert ist.
/// GEMESSEN: `beurteile(gut.der, widerrufen.pem, …)` lieferte
/// `Unbrauchbar(SignaturPasstNicht)`, obwohl dieselbe Datei mit `ca.pem`
/// `Ok(())` ergibt. Der Test `das_widerrufene_blatt_wird_als_widerrufen_
/// erkannt` hat es gemeldet, nicht der Entwickler.
///
/// added 02.10.2026 08:1x: DIE REIHENFOLGE DER ARGUMENTE ist Absicht und wird
/// hier noch einmal benannt, weil ein Vertauschen nicht knallt, sondern nur
/// immer `SignaturPasstNicht` liefert: erst das **Blatt** (dessen Seriennummer
/// wird gesucht), dann der **Aussteller** (dessen Schlüssel die Liste
/// signiert haben muss). Beide gleich zu setzen ist nur dann richtig, wenn das
/// Blatt selbstsigniert ist.
///
/// added 02.10.2026 06:7x: **WAS DIESER AUFRUF NICHT TUT**, und das ist der
/// Punkt, an dem die Lücke nicht kleiner wird: er **holt** nichts. Wer eine
/// Sperrliste aus dem `crlDistributionPoints` des Zertifikats laden muss,
/// braucht HTTP — also `narsil-net` — und dann das Caching und die Frage, was
/// ohne erreichbare Liste gilt (RFC 5280 6.3.3 sagt dafür nichts, das ist
/// `soft-fail` oder `hard-fail` und damit eine **Entscheidung der
/// Koordination**, keine des Lesers). Diese Datei liefert die Prüfseite, und
/// sie behauptet nicht mehr.
pub fn beurteile(bytes: &[u8], blatt: &Zertifikat, aussteller: &Zertifikat, jetzt: i64) -> Urteil {
    let crl = match crl_lesen(bytes) {
        Ok(c) => c,
        Err(f) => return Urteil::Unbrauchbar(f),
    };
    // added 03.10.2026 16:f5: **BEIDE, UND IN DIESER ROLLE** — das Blatt fuer
    // (b)(1) (sein Aussteller ist der CRL-Aussteller), die CA fuer (f) und (g)
    // (ihr Schluessel hat signiert). Die alte Form uebergab nur die CA und
    // verglich sie mit deren **Aussteller** (16:f1).
    if let Err(f) = pruefen(&crl, blatt, aussteller, jetzt) {
        return Urteil::Unbrauchbar(f);
    }
    match crl.widerruft(&blatt.seriennummer) {
        Some(w) => Urteil::Widerrufen(Box::new(w.clone())),
        None => Urteil::NichtWiderrufen,
    }
}
