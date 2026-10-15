//! Der stapelte OCSP-Status — **ausgewertet**, nicht nur gelesen.
//!
//! added 01.10.2026 22:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
//! Vorgang: Koordinationsmeldung vom 02.10.2026 („Den Zustandsautomaten fuer
//! TLS 1.2 um CertificateStatus ergaenzen (optional nach Certificate)")
//! und `team/AUTONOM.txt` Zeile 636, S4 „OCSP-Stapling auswerten".
//!
//! DER AUFBAU, den wir lesen (RFC 6960 Abschnitt 4.2.1):
//!
//! ```text
//! OCSPResponse ::= SEQUENCE {
//!     responseStatus   OCSPResponseStatus,          ENUMERATED
//!     responseBytes    [0] EXPLICIT ResponseBytes OPTIONAL }
//! ResponseBytes ::= SEQUENCE {
//!     responseType   OBJECT IDENTIFIER,               id-pkix-ocsp-basic 1.3.6.1.5.5.7.48.1.1
//!     response       OCTET STRING }                  <- BasicOCSPResponse
//! BasicOCSPResponse ::= SEQUENCE {
//!     tbsResponseData      ResponseData,
//!     signatureAlgorithm   AlgorithmIdentifier,
//!     signature            BIT STRING,
//!     certs            [0] EXPLICIT SEQUENCE OF Certificate OPTIONAL }
//! ResponseData ::= SEQUENCE {
//!     responderID          [0] EXPLICIT ResponderID,
//!     producedAt           GeneralizedTime,
//!     responses            SEQUENCE OF SingleResponse,
//!     responseExtensions   [1] EXPLICIT Extensions OPTIONAL }
//! SingleResponse ::= SEQUENCE {
//!     certID           CertID, certStatus  CertStatus,
//!     thisUpdate       GeneralizedTime, nextUpdate [0] EXPLICIT GeneralizedTime OPTIONAL,
//!     singleExtensions [1] EXPLICIT Extensions OPTIONAL }
//! CertStatus ::= ENUMERATED { good (0), revoked (1), unknown (2), … }
//! ```
//!
//! WAS WIR **NICHT** TUN, und das ist der wichtigste Satz dieser Datei:
//! wir pruefen die **Signatur** des Responders nicht. EinOCSP-Antwort ohne
//! Signaturpruefung ist eine Behauptung des Servers ueber sein eigenes
//! Zertifikat. Wir lesen deshalb nur `certStatus` und benutzen es
//! ausschliesslich **einseitig**: `revoked` bricht ab, alles andere wird
//! als `Unbekannt` behandelt und **nicht** als „gueltig“ verwendet. Wer
//! eine belastbare Widerrufspruefung will, braucht die Signaturpruefung —
//! das ist eine eigene Runde und steht als Luecke im Bericht.

use crate::der::DerLeser;

/// added 01.10.2026 22:2x UTC (Projektlabel 02.10.2026): `responseStatus` aus RFC 6960 4.2.1.
mod status {
    /// `successful (0)` — der Rest der Felder ist befuellt.
    pub const ERFOLGREICH: u8 = 0;
}

/// `CertStatus`-Werte (RFC 6960 4.2.1).
mod cert_status {
    /// added 02.10.2026 23:8x: die Werte sind **unsere** Konvention, nicht
    /// ASN.1-Werte: `certStatus` ist ein CHOICE, und der Weg von dort zu einer
    /// Zahl ist `beweise`. Ein `0` hier heisst „Tag 0x80 gesehen", nicht
    /// „ENUMERATED 0".
    pub const GUT: u8 = 0;
    pub const WIDERRUFEN: u8 = 1;
    /// `unknown [2]` — der Responder weiss es nicht.
    pub const UNBEKANNT: u8 = 2;
}

// added 02.10.2026 12:5x (Projektlabel 02.10.2026), Rolle TLS: **AUS `///` GEWORDEN, UND DAS IST EINE KORREKTUR.** added 02.10.2026 23:cx UTC (Projektlabel 02.10.2026), S4. DER FALSCHE
// Dieser Block war als `///` geschrieben und haengte deshalb laut clippy an `pub struct Antwort` — an ein Item, das er nicht beschreibt. Die eigentliche
// Doku von `Antwort` steht weiter unten ab Zeile 97; diese Zeilen sind eine
// Erzaehlung ueber die Loeschung alter Tests und gehoeren als `//` daneben.
//
// added 02.10.2026 23:cx: `status_der_ocsp_tiefe` und `status_der_ocsp`
// (geschrieben 22:2x in derselben Runde, die die Regression behob) lasen
// `certStatus` als **ENUMERATED** (Tag `0x0A`) und liefen ueber
// `OCSPResponse > responseBytes > OCTET STRING > BasicOCSPResponse >
// tbsResponseData > responses > SingleResponse > certStatus`.
//
// GEMESSEN, warum das falsch war: die Vektoren von `openssl ocsp`
// (`werkzeuge/ocsp_szenarien.sh`) zeigen an Offset 125
// (``asn1parse -inform der -strparse 26``) ``125:d=4 hl=2 l= 0 cons: cont [ 0 ]``
// — ein Kontext-Tag 0 mit **leerem** Inhalt, weil RFC 6960 4.2.1
// `CertStatus ::= CHOICE { good [0] IMPLICIT NULL, … }` schreibt. Der alte
// Leser fand dort nie ein `0x0A` und lieferte bei **jeder** echten Antwort
// ``Unbekannt``.
//
// WAS DAS FUR DEN BERICHT VON HEUTE MORGEN HEISST: die Aussage „revoked
// bricht den Handshake ab" war an echten stapelten Antworten nie belegt —
// nur an selbstgebauten Vektoren, die dem RFC nicht entsprachen.
// Sicherheitsfolgen hatte das keine (``Unbekannt`` erzwang nichts), aber die
// Behauptung war zu gross. Neu ist `antwort_lesen` plus `beweise`.
//
// DIE ALTEN TESTS SIND MITGELOESCHT, und das ist Absicht: `good_wird_gut`,
// `revoked_wird_widerrufen` und `unknown_ist_unbekannt` bauten ihre
// Vektoren mit ``tlv(0x0A, &[wert])`` — also mit dem ENUMERATED, das es in
// keiner echten Antwort gibt. Sie haben den Fehler nicht gefunden, weil sie
// den Fehler enthielten. Ihre Ersetzung steht weiter unten und benutzt
// Dateien aus `testdaten/ocsp/`, die `openssl` geschrieben hat.
// ---------------------------------------------------------------------------
// Der vollstaendige Aufbau — aus einem Lesen wird eine Pruefung
// ---------------------------------------------------------------------------

/// added 02.10.2026 23:4x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// `OCSPResponse` mit allem, was fuer eine **Entscheidung** noetig ist.
///
/// added 02.10.2026 23:4x: BIS HIERHER LAS `status_der_ocsp` NUR DEN
/// `certStatus`. Das ist die Luecke, die `team/BERICHT_TLS_R8.txt` Punkt 3 als
/// Luecke genannt hat: „die **Signatur** des Responders wird nicht
/// geprueft — ohne sie ist die Antwort eine Behauptung des Servers". Diese
/// Struktur traegt jetzt die Felder, aus denen sich die Behauptung pruefen
/// laesst (RFC 6960 4.2.1):
///
/// ```text
/// BasicOCSPResponse ::= SEQUENCE {
///     tbsResponseData      ResponseData,        <- **das** wurde signiert
///     signatureAlgorithm   AlgorithmIdentifier,
///     signature            BIT STRING,
///     certs            [0] EXPLICIT SEQUENCE OF Certificate OPTIONAL }
/// CertID ::= SEQUENCE {
///     hashAlgorithm   AlgorithmIdentifier,      <- meist SHA-1
///     issuerNameHash  OCTET STRING,             <- Hash ueber den issuer
///     issuerKeyHash   OCTET STRING,             <- Hash ueber subjectPublicKey
///     serialNumber    CertificateSerialNumber }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Antwort {
    /// `responseStatus`: 0 = `successful`. Alles andere heisst, es gab keine
    /// OCSP-Antwort.
    pub code: u8,
    /// `certStatus`: 0 = `good`, 1 = `revoked`, 2 = `unknown`.
    pub cert_status: u8,
    /// Die OID aus `hashAlgorithm` der CertID, als Punkte.
    pub hash_oid: Vec<u64>,
    /// Die drei Felder der CertID.
    pub issuer_name_hash: Vec<u8>,
    pub issuer_key_hash: Vec<u8>,
    pub seriennummer: Vec<u8>,
    /// `thisUpdate` und `nextUpdate`, in Sekunden seit 1970.
    pub this_update: i64,
    pub next_update: Option<i64>,
    /// `producedAt`.
    pub erzeugt: i64,
    /// Die OID aus `signatureAlgorithm`, der Hash daraus und (bei PSS) die
    /// Salzlaenge.
    pub alg_oid: Vec<u64>,
    pub alg_hash: crate::x509::Hash,
    pub pss_salt: Option<u32>,
    /// Der Inhalt der `signature`-Bit-Zeichenkette.
    pub signatur: Vec<u8>,
    /// `certs`: die Zertifikate des Responders als DER. Leer, wenn der
    /// Responder die CA selbst ist.
    pub zertifikate: Vec<Vec<u8>>,
    /// `tbsResponseData` **mit** Tag — ueber genau diese Bytes ging die
    /// Signatur.
    pub tbs_roh: Vec<u8>,
}

/// added 02.10.2026 23:4x: die Fehlerformen, **benannt**.
///
/// added 02.10.2026 23:4x: `KeineAntwort` ist kein Syntaxfehler, sondern eine
/// Aussage des Responders: `responseStatus` 1 bis 6 heisst, es gab keine
/// OCSP-Antwort (RFC 6960 4.2.1). Das ist der haeufigste Fall in der
/// Wildbahn, und ein Handshake darf daran **nicht** sterben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcspFehler {
    /// Der Aufbau laesst sich nicht lesen.
    Aufbau(String),
    /// `responseStatus` ist nicht `successful (0)`.
    KeineAntwort(u8),
    /// Die Antwort ist laenger als `MAX_ANTWORT_BYTES`.
    ZuLang { ist: usize, erlaubt: usize },
    /// Mehr Zertifikate in `certs`, als wir lesen.
    ZuVieleZertifikate { ist: usize, erlaubt: usize },
}

impl std::fmt::Display for OcspFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcspFehler::Aufbau(w) => write!(f, "OCSP-Aufbau unlesbar: {w}"),
            OcspFehler::KeineAntwort(c) => {
                write!(f, "der Responder hat nicht geantwortet (Code {c})")
            }
            OcspFehler::ZuLang { ist, erlaubt } => {
                write!(f, "OCSP-Antwort {ist} Byte, erlaubt sind {erlaubt}")
            }
            OcspFehler::ZuVieleZertifikate { ist, erlaubt } => {
                write!(
                    f,
                    "{ist} Zertifikate in der Antwort, erlaubt sind {erlaubt}"
                )
            }
        }
    }
}

/// added 02.10.2026 23:4x: M4-Grenze fuer eine stapelte Antwort.
///
/// added 02.10.2026 23:4x: GEMESSEN an den stapelten Antworten von `xkcd.com`
/// und `www.netbsd.org` (02.10.2026 22:2x): 1486 Byte
/// (`CertificateStatus`-Laenge `05be`). Der Wert ist 256 KiB — gross genug
/// fuer jede gemessene und jede erdenkliche Norm-Antwort (die Zertifikate in
/// `certs` sind der Grund fuer den Spielraum) und klein genug, damit ein
/// Server nicht den Speicher des Clients bestimmen kann.
pub const MAX_ANTWORT_BYTES: usize = 256 * 1024;

/// added 02.10.2026 23:4x: M3/M4, hoechstens so viele Zertifikate in `certs`.
///
/// added 02.10.2026 23:4x: RFC 6960 4.2.2.2 nennt fuer `certs` keine Zahl,
/// sondern „a handful". Vier ist eine benannte Grenze, keine Zahl aus der
/// Luft: ein Delegierter braucht sein Zertifikat, und bis zu drei
/// Zwischenzertifikate, wenn die Kette laenger ist. Jedes Zertifikat ist
/// zusaetzlich auf `x509::MAX_ZERTIFIKAT` begrenzt, und die Schleife laeuft
/// ueber `MAX_ZERTIFIKATE_OCSP`, nicht ueber die Eingabe.
pub const MAX_ZERTIFIKATE_OCSP: usize = 4;

/// added 02.10.2026 23:5x: DIE HILFE GIBT DAS **ELEMENT** ZURUECK, nicht
/// einen Leser darueber.
///
/// added 02.10.2026 23:5x: der Umweg ist zweimal begangen worden.
/// `Element::inhalt` ist ein **eigener** `Vec` (siehe `der.rs`), also kann ein
/// Leser darueber die Funktion nicht verlassen; der Uebersetzer sagt dann
/// „cannot return value referencing local data `e.inhalt`" (`E0515`). Der
/// Aufrufer haelt das Element deshalb in einer **eigenen** Variablen im
/// selben Block und legt den Leser darauf an — so wie es
/// `status_der_ocsp_tiefe` oben schon macht.
fn sequenz(l: &mut DerLeser<'_>) -> Result<crate::der::Element, OcspFehler> {
    let e = l
        .element()
        .map_err(|e| OcspFehler::Aufbau(format!("SEQUENCE: {e:?}")))?;
    if e.tag != 0x30 {
        return Err(OcspFehler::Aufbau(format!(
            "SEQUENCE erwartet, Tag {:02x}",
            e.tag
        )));
    }
    Ok(e)
}

fn oktett(l: &mut DerLeser<'_>, was: &'static str) -> Result<Vec<u8>, OcspFehler> {
    let e = l
        .element()
        .map_err(|x| OcspFehler::Aufbau(format!("{was}: {x:?}")))?;
    if e.tag != 0x04 {
        return Err(OcspFehler::Aufbau(format!(
            "{was} ist kein OCTET STRING (Tag {:02x})",
            e.tag
        )));
    }
    Ok(e.inhalt)
}

fn integer(l: &mut DerLeser<'_>, was: &'static str) -> Result<Vec<u8>, OcspFehler> {
    let e = l
        .element()
        .map_err(|x| OcspFehler::Aufbau(format!("{was}: {x:?}")))?;
    if e.tag != 0x02 {
        return Err(OcspFehler::Aufbau(format!(
            "{was} ist kein INTEGER (Tag {:02x})",
            e.tag
        )));
    }
    Ok(e.inhalt)
}

fn enumeriert(l: &mut DerLeser<'_>, was: &'static str) -> Result<u8, OcspFehler> {
    let e = l
        .element()
        .map_err(|x| OcspFehler::Aufbau(format!("{was}: {x:?}")))?;
    if e.tag != 0x0A {
        return Err(OcspFehler::Aufbau(format!(
            "{was} ist kein ENUMERATED (Tag {:02x})",
            e.tag
        )));
    }
    match e.inhalt.first() {
        Some(w) => Ok(*w),
        None => Err(OcspFehler::Aufbau(format!("{was} ist leer"))),
    }
}

fn zeit(l: &mut DerLeser<'_>) -> Result<i64, OcspFehler> {
    let e = l
        .element()
        .map_err(|x| OcspFehler::Aufbau(format!("Zeit: {x:?}")))?;
    crate::x509::zeit_seit_1970(e.tag, &e.inhalt)
        .map_err(|x| OcspFehler::Aufbau(format!("Zeit: {x:?}")))
}

/// Liest eine stapelte OCSP-Antwort vollstaendig.
///
/// added 02.10.2026 23:4x: `Ok(Antwort)` heisst **nicht** „gueltig", sondern
/// nur „lesbar". Die Entscheidung faellt `beweise`, weil sie das Blatt, den
/// Aussteller und die Uhrzeit braucht — drei Dinge, die nicht in der Antwort
/// stehen.
pub fn antwort_lesen(antwort: &[u8]) -> Result<Antwort, OcspFehler> {
    if antwort.len() > MAX_ANTWORT_BYTES {
        return Err(OcspFehler::ZuLang {
            ist: antwort.len(),
            erlaubt: MAX_ANTWORT_BYTES,
        });
    }
    let mut aussen = DerLeser::neu(antwort);
    // added 02.10.2026 23:5x: `sequenz` gibt bereits einen **Leser** zurueck,
    // nicht das Element. Die erste Fassung holte noch einmal `.inhalt` — der
    // Uebersetzer meldete „no field `inhalt` on type `DerLeser`".
    let ocsp_e = sequenz(&mut aussen)?;
    let mut ocsp = DerLeser::neu(&ocsp_e.inhalt);
    let code = enumeriert(&mut ocsp, "responseStatus")?;
    if code != status::ERFOLGREICH {
        return Err(OcspFehler::KeineAntwort(code));
    }
    let huelle_e = ocsp
        .element()
        .map_err(|e| OcspFehler::Aufbau(format!("responseBytes: {e:?}")))?;
    if huelle_e.tag != 0xA0 {
        return Err(OcspFehler::Aufbau(format!(
            "responseBytes ist nicht [0] EXPLICIT (Tag {:02x})",
            huelle_e.tag
        )));
    }
    let mut innen = DerLeser::neu(&huelle_e.inhalt);
    let rb_e = sequenz(&mut innen)?;
    let mut rb = DerLeser::neu(&rb_e.inhalt);
    // `responseType`: uebersprungen, nicht geprueft (siehe Modulkopf).
    let _ = rb.element();
    let oktett_e = oktett(&mut rb, "response")?;
    let mut oktett_l = DerLeser::neu(&oktett_e);
    let basic_e = sequenz(&mut oktett_l)?;
    let mut basic = DerLeser::neu(&basic_e.inhalt);

    // --- tbsResponseData, als Erstes -------------------------------------
    let tbs_e = basic
        .element()
        .map_err(|e| OcspFehler::Aufbau(format!("tbsResponseData: {e:?}")))?;
    if tbs_e.tag != 0x30 {
        return Err(OcspFehler::Aufbau(format!(
            "tbsResponseData ist kein SEQUENCE (Tag {:02x})",
            tbs_e.tag
        )));
    }
    // added 02.10.2026 23:4x: `roh`, **nicht** wieder zusammengesetzt. Ueber
    // diese Bytes ging die Signatur; eine neu kodierte SEQUENCE koennte an
    // einer Laengenkodierung abweichen (Langform ab 128 Byte) und die
    // Pruefung faelle dann ohne sichtbaren Grund durch.
    let tbs_roh = tbs_e.roh.clone();
    let mut daten = DerLeser::neu(&tbs_e.inhalt);

    // --- ResponderID [0] EXPLICIT, producedAt ----------------------------
    // added 02.10.2026 23:4x: der Responder wird **nicht** ueber seinen Namen
    // gebunden. Die Bindung entsteht ueber das Zertifikat in `certs` und
    // dessen Signatur durch die CA; ein Namensvergleich waere eine zweite
    // Normalisierung (RDN-Reihenfolge, Leerzeichen, Gross/Klein) und damit
    // eine neue Fehlerquelle an einer Stelle, die die Kette schon kennt.
    let _responder = daten
        .element()
        .map_err(|e| OcspFehler::Aufbau(format!("responderID: {e:?}")))?;
    let _erzeugt = zeit(&mut daten)?;

    // --- responses: SEQUENCE OF SingleResponse ---------------------------
    let liste_e = sequenz(&mut daten)?;
    let mut liste = DerLeser::neu(&liste_e.inhalt);
    if liste.ist_leer() {
        return Err(OcspFehler::Aufbau("responses ist leer".into()));
    }
    let single_e = sequenz(&mut liste)?;
    let mut single = DerLeser::neu(&single_e.inhalt);
    let certid_e = sequenz(&mut single)?;
    let mut certid = DerLeser::neu(&certid_e.inhalt);
    let hash_oid = {
        // added 02.10.2026 23:4x: `algorithmus_lesen` aus `x509.rs`, weil die
        // Frage „OID plus Parametersatz" dort schon einmal beantwortet wird.
        // Eine eigene OID-Sichtbarkeits-Regel hier waere die zweite.
        let alg = crate::x509::algorithmus_lesen(&mut certid)
            .map_err(|e| OcspFehler::Aufbau(format!("hashAlgorithm: {e:?}")))?;
        alg.oid
    };
    let issuer_name_hash = oktett(&mut certid, "issuerNameHash")?;
    let issuer_key_hash = oktett(&mut certid, "issuerKeyHash")?;
    let seriennummer = integer(&mut certid, "serialNumber")?;
    // --- certStatus: ein CHOICE, kein ENUMERATED -------------------------
    // added 02.10.2026 23:8x: DAS WAR EIN FEHLER, UND ER IST MIR AUFGEFALLEN,
    // WEIL DIE NEUEN VECTOREN VON `openssl` DEN GEGENTEIL BEHAUPTEN. RFC 6960
    // 4.2.1:
    //
    //     CertStatus ::= CHOICE {
    //         good    [0] IMPLICIT NULL,
    //         revoked [1] IMPLICIT RevokedInfo,
    //         unknown [2] IMPLICIT UnknownInfo }
    //
    // added 02.10.2026 23:8x: GEMESSEN an `gut_ecdsa.der` mit
    // `openssl asn1parse -inform der -strparse 26` (Offset 125):
    // `125:d=4 hl=2 l= 0 cons: cont [ 0 ]` — ein Kontext-Tag 0 mit **leerem**
    // Inhalt. Ein ENUMERATED (0x0A) kommt dort in keiner echten Antwort vor.
    //
    // added 02.10.2026 23:8x: WAS DAS FUER DIE LETZTE RUNDE BEDEUTET, und es
    // ist der ehrlichste Satz dieses Berichts: `status_der_ocsp` (R8) las ein
    // ENUMERATED und bekam bei echten stapelten Antworten **immer** `None`,
    // also `Unbekannt`. Das heisst: die Aussage „revoked bricht den Handshake
    // ab" war an echten Antworten **nie** belegt — nur an selbstgebauten
    // Vektoren, die dem RFC nicht entsprachen. Sicherheitsfolgen hatte das
    // keine (`Unbekannt` erzwang nichts), aber die Behauptung war zu gross.
    let cert_status = {
        let e = single
            .element()
            .map_err(|x| OcspFehler::Aufbau(format!("certStatus: {x:?}")))?;
        match e.tag {
            // good [0] IMPLICIT NULL — der Inhalt muss leer sein (DER
            // erzwingt das; ein Inhalt waere ein Fehler des Responders).
            0x80 => {
                if !e.inhalt.is_empty() {
                    return Err(OcspFehler::Aufbau(format!(
                        "good hat {} Byte Inhalt statt keinem",
                        e.inhalt.len()
                    )));
                }
                cert_status::GUT
            }
            // revoked [1] IMPLICIT RevokedInfo
            //
            // added 02.10.2026 23:9x: DER TAG IST `0xA1`, nicht `0x81` — und
            // das ist keine Eigenheit von `openssl`. GEMESSEN an
            // `widerrufen.der` (`asn1parse -strparse 26`, Offset 125):
            // `125:d=4 hl=2 l= 22 cons: cont [ 1 ]`. IMPLICIT verdraengt nur
            // die **Nummer**, nicht das Constructed-Bit, und `RevokedInfo` ist
            // ein SEQUENCE — also konstruiert. Fuer `good` (NULL) bleibt es
            // `0x80`, fuer `unknown` waere es `0xA2`. Die erste Fassung
            // kannte nur `0x81` und meldete „certStatus mit unbekanntem Tag
            // a1".
            //   RevokedInfo ::= SEQUENCE { revocationTime GeneralizedTime,
            //                             revocationReason [0] EXPLICIT
            //                                          CRLReason OPTIONAL }
            // added 02.10.2026 23:8x: die Zeit wird **gelesen und geprueft**,
            // nicht uebersprungen. Eine Widerrufsmeldung ohne Zeitangabe ist
            // nicht auswertbar, und ein Server, der sie weglässt, soll
            // daran erkennbar sein — nicht still durchgehen.
            0xA1 => {
                let mut ri = DerLeser::neu(&e.inhalt);
                zeit(&mut ri)?;
                cert_status::WIDERRUFEN
            }
            // unknown [2] IMPLICIT UnknownInfo — konstruiert, wie revoked.
            0xA2 => cert_status::UNBEKANNT,
            // added 02.10.2026 23:9x: die **primitiven** Formen sind
            // aufgenommen, obwohl DER sie nicht erzeugt. Ein Responder, der
            // `revoked` als `0x81` schickt, ist nicht normgerecht — aber die
            // *Bedeutung* ist eindeutig, und ein Widerruf darf nicht an einer
            // Formalie hängen. GEMESSEN ist der Fall `0xA1`; die anderen
            // Zweige sind Zugabe mit derselben Bedeutung.
            0x81 => cert_status::WIDERRUFEN,
            0x82 => cert_status::UNBEKANNT,
            other => {
                return Err(OcspFehler::Aufbau(format!(
                    "certStatus mit unbekanntem Tag {:02x}",
                    other
                )));
            }
        }
    };
    let this_update = zeit(&mut single)?;
    // added 02.10.2026 23:4x: `nextUpdate` ist **optional** (RFC 6960 4.2.1)
    // und kommt in zwei Formen: als GeneralizedTime oder als [0] EXPLICIT
    // darum. `naechste_tag` schaut nur, damit ein fehlendes Feld nichts
    // verbraucht — `element()` wuerde es sonst.
    let next_update = match single.naechste_tag() {
        Some(0x18) => Some(zeit(&mut single)?),
        Some(0xA0) => {
            let h = single
                .element()
                .map_err(|x| OcspFehler::Aufbau(format!("nextUpdate: {x:?}")))?;
            let mut hi = DerLeser::neu(&h.inhalt);
            Some(zeit(&mut hi)?)
        }
        _ => None,
    };

    // --- Der Rest von BasicOCSPResponse ----------------------------------
    let alg = crate::x509::algorithmus_lesen(&mut basic)
        .map_err(|e| OcspFehler::Aufbau(format!("signatureAlgorithm: {e:?}")))?;
    let alg_hash = match alg.hash {
        Some(h) => h,
        // added 02.10.2026 23:4x: `None` heisst „Hash unbekannt", und das ist
        // **kein** Grund, einen zu raten. `beweise` meldet dann
        // `Unbekannt`, statt SHA-256 zu unterstellen.
        None => return Err(OcspFehler::Aufbau("signatureAlgorithm ohne Hash".into())),
    };
    let sig_e = basic
        .element()
        .map_err(|e| OcspFehler::Aufbau(format!("signature: {e:?}")))?;
    if sig_e.tag != 0x03 {
        return Err(OcspFehler::Aufbau(format!(
            "signature ist keine BIT STRING (Tag {:02x})",
            sig_e.tag
        )));
    }
    let signatur = {
        let (_, inhalt) = crate::der::bit_string(&sig_e.inhalt)
            .map_err(|e| OcspFehler::Aufbau(format!("signature: {e:?}")))?;
        inhalt.to_vec()
    };

    // --- certs [0] EXPLICIT SEQUENCE OF Certificate OPTIONAL -------------
    let mut zertifikate = Vec::new();
    if basic.naechste_tag() == Some(0xA0) {
        let h = basic
            .element()
            .map_err(|e| OcspFehler::Aufbau(format!("certs: {e:?}")))?;
        let mut hi = DerLeser::neu(&h.inhalt);
        let liste_e = sequenz(&mut hi)?;
        let mut l = DerLeser::neu(&liste_e.inhalt);
        // M3/M4: die Schleife laeuft ueber `MAX_ZERTIFIKATE_OCSP`, nicht ueber
        // die Eingabe.
        while !l.ist_leer() {
            if zertifikate.len() >= MAX_ZERTIFIKATE_OCSP {
                return Err(OcspFehler::ZuVieleZertifikate {
                    ist: zertifikate.len() + 1,
                    erlaubt: MAX_ZERTIFIKATE_OCSP,
                });
            }
            let e = l
                .element()
                .map_err(|e| OcspFehler::Aufbau(format!("certs: {e:?}")))?;
            if e.roh.len() > crate::x509::MAX_ZERTIFIKAT {
                return Err(OcspFehler::ZuLang {
                    ist: e.roh.len(),
                    erlaubt: crate::x509::MAX_ZERTIFIKAT,
                });
            }
            zertifikate.push(e.roh);
        }
    }
    Ok(Antwort {
        code,
        cert_status,
        hash_oid,
        issuer_name_hash,
        issuer_key_hash,
        seriennummer,
        this_update,
        next_update,
        erzeugt: _erzeugt,
        alg_oid: alg.oid,
        alg_hash,
        pss_salt: alg.pss_salt,
        signatur,
        zertifikate,
        tbs_roh,
    })
}

/// Der Hash, den die CertID verlangt, oder `None`, wenn wir ihn nicht rechnen.
///
/// added 02.10.2026 23:4x: SHA-1 ist hier **kein** Versehen. RFC 6960 4.1.1
/// schreibt fuer `CertID` den SHA-1 vor, und der Responder **muss** dann auch
/// SHA-1 rechnen. Wer nur SHA-256 anbietet, wuerde jede Antwort als
/// „unbekannter Hash" abweisen — und die haeufigste Antwort der Welt ist
/// genau die.
/// added 02.10.2026 12:5x (Projektlabel 02.10.2026), Rolle TLS: **DER
/// FUNCTIONSZEIGER ALS ALIAS**, aus demselben Grund wie `CrlErweiterungen`:
/// `Option<fn(&[u8]) -> Vec<u8>>` ist an drei Stellen gleich und an keiner lesbar.
type Hashfunktion = fn(&[u8]) -> Vec<u8>;

fn hash_der_certid(oid: &[u64]) -> Option<Hashfunktion> {
    match oid {
        [1, 3, 14, 3, 2, 26] => Some(|d| crate::sha2::sha1(d).to_vec()),
        [2, 16, 840, 1, 101, 3, 4, 2, 1] => Some(|d| crate::sha2::sha256(d).to_vec()),
        _ => None,
    }
}

/// Das Ergebnis einer **vollstaendigen** Pruefung.
///
/// added 02.10.2026 23:4x: `Unbekannt` traegt den **Grund**. Ohne Grund ist
/// die Meldung wertlos: „unbekannt" allein sagt niemandem, ob der Responder
/// kaputt ist, die Uhrzeit verrutscht ist oder die Signatur nicht passt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Urteil {
    /// Die Antwort sagt `good`, **und** CertID, Zeitfenster und Signatur
    /// stimmen.
    Gut,
    /// Die Antwort sagt `revoked`. Dazu muss man sonst nichts wissen.
    Widerrufen,
    /// Etwas stimmt nicht oder wir koennen es nicht nachrechnen.
    Unbekannt(&'static str),
}

/// added 02.10.2026 23:4x: HIER WIRD AUS EINER BEHAUPTUNG EINE AUSSAGE.
///
/// DIE REIHENFOLGE, und sie ist nicht beliebig:
/// 1. `revoked` zuerst. Ein klarer Widerruf ist auch dann einer, wenn die
///    Signatur daneben kaputt ist.
/// 2. CertID: die Antwort muss **ueber unser Zertifikat** sein.
/// 3. Zeitfenster: `thisUpdate <= jetzt <= nextUpdate`.
/// 4. Signatur: mit dem Schluessel des Ausstellers oder eines von ihm
///    delegierten Responders.
///
/// added 02.10.2026 23:4x: `Gut` ist jetzt eine **Aussage ueber eine
/// gepruefte Antwort**, nicht mehr ueber eine gelesene Zahl. Das ist der
/// Unterschied zwischen „der Server sagt, sein Zertifikat sei gueltig" und
/// „die Signatur der CA ueber diese Aussage ist in Ordnung".
pub fn beweise(
    a: &Antwort,
    blatt: &crate::x509::Zertifikat,
    aussteller: &crate::x509::Zertifikat,
    jetzt: i64,
) -> Urteil {
    if a.cert_status == cert_status::WIDERRUFEN {
        return Urteil::Widerrufen;
    }
    if a.cert_status != cert_status::GUT {
        return Urteil::Unbekannt("der Responder sagt weder gut noch widerrufen");
    }
    // --- 2) Die CertID muss unsere sein ----------------------------------
    let rechne = match hash_der_certid(&a.hash_oid) {
        Some(f) => f,
        None => return Urteil::Unbekannt("die CertID verlangt einen Hash, den wir nicht rechnen"),
    };
    let name = match crate::x509::subject_der(aussteller) {
        Ok(v) => v,
        Err(_) => return Urteil::Unbekannt("der Aussteller laesst sich nicht lesen"),
    };
    let key = match crate::x509::spki_bits(aussteller) {
        Ok(v) => v,
        Err(_) => {
            return Urteil::Unbekannt("der Schluessel des Ausstellers laesst sich nicht lesen");
        }
    };
    if rechne(&name) != a.issuer_name_hash {
        return Urteil::Unbekannt("die CertID nennt einen anderen Aussteller");
    }
    if rechne(&key) != a.issuer_key_hash {
        return Urteil::Unbekannt("die CertID nennt einen anderen Schluessel");
    }
    if a.seriennummer != blatt.seriennummer {
        return Urteil::Unbekannt("die CertID nennt eine andere Seriennummer");
    }
    // --- 3) Das Zeitfenster ----------------------------------------------
    if jetzt < a.this_update {
        return Urteil::Unbekannt("die Antwort ist noch nicht gueltig");
    }
    if let Some(ende) = a.next_update {
        if jetzt > ende {
            return Urteil::Unbekannt("die Antwort ist abgelaufen");
        }
    }
    // --- 4) Die Signatur -------------------------------------------------
    // added 02.10.2026 23:5x: `SchluesselArt` als **Wert**, weil der
    // delegierte Responder erst hier entsteht und sein Schluessel nicht von
    // einer aeusseren Struktur geliehen werden kann.
    let eigener = if a.zertifikate.is_empty() {
        None
    } else {
        match responder_schluessel(a, blatt, aussteller) {
            Ok(k) => Some(k),
            Err(grund) => return Urteil::Unbekannt(grund),
        }
    };
    let schluessel: &crate::x509::SchluesselArt = match &eigener {
        Some(k) => k,
        None => &aussteller.schluessel,
    };
    // added 02.10.2026 23:5x: DER NAME. Ich hatte `signatur_ueber_pruefen`
    // geschrieben und der Uebersetzer meldete „no function
    // `signatur_ueber_pruefen` in module `crate::kette`" — die Funktion
    // heisst `signatur_ueber` (kette.rs Zeile 463) und ist `pub(crate)`.
    match crate::kette::signatur_ueber(
        &a.tbs_roh,
        &a.signatur,
        &a.alg_oid,
        a.alg_hash,
        a.pss_salt,
        schluessel,
    ) {
        Ok(()) => Urteil::Gut,
        Err(_) => Urteil::Unbekannt("die Signatur des Responders passt nicht"),
    }
}

/// added 02.10.2026 23:4x: der Schluessel eines **delegierten** Responders.
///
/// added 02.10.2026 23:4x: RFC 6960 4.2.2.2 verlangt fuer ein delegiertes
/// Zertifikat dreierlei, und alle drei werden hier geprueft:
/// 1. es ist von **derselben** CA signiert, die auch unser Blatt signiert hat
///    (sonst koennte ein fremder Responder eine Antwort fuer unsere Zertifikat
///    beilegen);
/// 2. es traegt die Erweiterung `id-kp-OCSPSigning` (1.3.6.1.5.5.7.3.9);
/// 3. seine Signatur wird von dieser CA verifiziert.
///
/// added 02.10.2026 23:4x: DER NAMENSVERGLEICH IST BYTE-GLEICH, und nicht
/// „RDN fuer RDN". `aussteller_der` liefert die unveraenderten DER-Bytes des
/// Ausstellers. Zwei CA, die denselben Namen in verschiedener Schreibweise
/// schreiben, gelten uns als verschieden — das ist **strenger** als RFC 5280
/// und damit die sichere Richtung: im Zweifel `Unbekannt`, nie `Gut`.
/// added 02.10.2026 23:5x: DER RUECKGABETYP IST EIGENTUM, nicht eine
/// Referenz. Das Zertifikat des Responders wird **hier** gelesen und waere am
/// Ende dieser Funktion weg; eine Referenz auf seinen Schluessel zeigte dann
/// auf nichts („cannot return value referencing local data `z.schluessel`",
/// `E0515`).
fn responder_schluessel(
    a: &Antwort,
    blatt: &crate::x509::Zertifikat,
    aussteller: &crate::x509::Zertifikat,
) -> Result<crate::x509::SchluesselArt, &'static str> {
    let blatt_aussteller = match crate::x509::aussteller_der(blatt) {
        Ok(v) => v,
        Err(_) => return Err("der Aussteller unseres Blatts laesst sich nicht lesen"),
    };
    // added 02.10.2026 23:9x: DIE REIHENFOLGE DER ZWEI DURCHGAENGE, und sie
    // ist gemessen, nicht gewaehlt.
    //
    // added 02.10.2026 23:9x: BEFUND 1 — `openssl ocsp` legt die **CA selbst**
    // in `certs`, auch wenn der Responder die CA ist. GEMESSEN an
    // `gut_ecdsa.der` mit `openssl asn1parse -inform der -strparse 26`:
    // nach der `signature` steht bei Offset 250 ein `cont [ 0 ]` mit 428 Byte,
    // darin ein SEQUENCE von 424 Byte — das ist `wurzel_ecdsa.der`. Ohne diese
    // Beachtung ging **jede** gueltige Antwort den Weg „delegierter Responder"
    // und wurde an der fehlenden OCSPSigning-Erweiterung abgelehnt: 6 von 9
    // Tests waren rot, alle mit demselben Grund.
    //
    // added 02.10.2026 23:9x: BEFUND 2 — bei einem delegierten Responder kann
    // die Reihenfolge in `certs` unscheinbar sein. Deshalb der **Delegierte
    // zuerst** und die CA erst im zweiten Durchgang: nimmt man das erste
    // passende Zertifikat, kann das die CA sein, und dann passt die Signatur
    // nicht (sie hat der Delegierte gemacht).
    //
    // added 02.10.2026 23:9x: „die CA selbst" heisst hier **bytegleich** mit
    // dem Zertifikat, das wir aus der Kette haben. Kein Namensvergleich, kein
    // Schluesselvergleich: dieselbe Datei, dieselben Bytes.
    // added 02.10.2026 23:9x: die Erinnerung an „gesehen, aber ohne Erlaubnis".
    let mut gesehen_ohne_erlaubnis = false;
    for der in &a.zertifikate {
        if der.as_slice() == aussteller.der.as_slice() {
            continue;
        }
        let z = match crate::x509::zertifikat_lesen(der) {
            Ok(z) => z,
            Err(_) => continue,
        };
        if crate::x509::aussteller_der(&z) != Ok(blatt_aussteller.clone()) {
            continue;
        }
        let erlaubt = match &z.erweiterte_verwendung {
            Some(liste) => liste
                .iter()
                .any(|oid| oid.as_slice() == crate::x509::OID_OCSP_SIGNING),
            None => false,
        };
        if !erlaubt {
            // added 02.10.2026 23:9x: `continue`, aber der Grund wird
            // **gemerkt**. Die Datei koennte ein Zertifikat derselben CA ohne
            // die Erweiterung sein, neben einem mit — dann zaehlt das mit.
            // Steht am Ende kein Delegierter da, ist die fehlende Erweiterung
            // immer noch die naheliegendste Erklaerung, und der Bericht
            // soll das sagen statt „irgendwas passte nicht".
            if !gesehen_ohne_erlaubnis {
                gesehen_ohne_erlaubnis = true;
            }
            continue;
        }
        if crate::kette::signatur_pruefen(&z, aussteller).is_err() {
            return Err("das Responder-Zertifikat ist nicht von der CA signiert");
        }
        return Ok(z.schluessel.clone());
    }
    // --- Zweiter Durchgang: die CA selbst --------------------------------
    for der in &a.zertifikate {
        if der.as_slice() == aussteller.der.as_slice() {
            return Ok(aussteller.schluessel.clone());
        }
    }
    if gesehen_ohne_erlaubnis {
        return Err("der Responder hat keine OCSPSigning-Erweiterung");
    }
    Err("kein passendes Responder-Zertifikat in der Antwort")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// added 01.10.2026 22:3x UTC (Projektlabel 02.10.2026): haengt Bytefelder aneinander.
    ///
    /// added 02.10.2026 23:6x: die PEMs und DERs als Bytes.
    mod vektor {
        pub const GUT_ECDSA: &[u8] = include_bytes!("../testdaten/ocsp/gut_ecdsa.der");
        pub const GUT_RSA: &[u8] = include_bytes!("../testdaten/ocsp/gut_rsa.der");
        pub const GUT_DELEGIERT: &[u8] = include_bytes!("../testdaten/ocsp/gut_delegiert.der");
        pub const GUT_FREMD: &[u8] = include_bytes!("../testdaten/ocsp/gut_fremd.der");
        pub const GUT_OHNE_ERLAUBNIS: &[u8] =
            include_bytes!("../testdaten/ocsp/gut_ohne_erlaubnis.der");
        pub const WIDERRUFEN: &[u8] = include_bytes!("../testdaten/ocsp/widerrufen.der");
        pub const WURZEL_ECDSA: &[u8] = include_bytes!("../testdaten/ocsp/wurzel_ecdsa.der");
        pub const WURZEL_RSA: &[u8] = include_bytes!("../testdaten/ocsp/wurzel_rsa.der");
        pub const BLATT: &[u8] = include_bytes!("../testdaten/ocsp/blatt.der");
        pub const BLATT_RSA: &[u8] = include_bytes!("../testdaten/ocsp/rsablatt.der");
        pub const FREMD: &[u8] = include_bytes!("../testdaten/ocsp/fremd.der");
        pub const WIDERRUF_BLATT: &[u8] = include_bytes!("../testdaten/ocsp/widerruf.der");
    }

    /// added 02.10.2026 23:7x: DIE ZERTIFIKATE LIEGEN ALS **DER** VOR, nicht als
    /// PEM. Die erste Fassung dieses Tests hatte einen Base64-Leser im
    /// Testmodul, um aus dem PEM zu kommen — `base64_decode` gibt es in der
    /// Crate nicht. Ein Base64-Leser im Test ist eine zweite Sache, die
    /// falsch sein kann, und sie hat mit OCSP nichts zu tun. `openssl x509
    /// -outform DER` (im Skript) macht die Umwandlung mit dem Werkzeug, das
    /// die Zertifikate erzeugt hat.
    fn zertifikat(der: &[u8]) -> crate::x509::Zertifikat {
        crate::x509::zertifikat_lesen(der).expect("Zertifikat")
    }

    /// added 02.10.2026 23:6x: eine Zeit **innerhalb** des Fensters jedes
    /// Vektors, ohne Uhrzeit. `jetzt = thisUpdate + 1` liegt immer drin, weil
    /// `thisUpdate <= jetzt` gelten muss und `nextUpdate` bei allen Vektoren
    /// sieben Tage weiter ist.
    fn jetzt_in(a: &Antwort) -> i64 {
        a.this_update + 1
    }

    /// added 02.10.2026 23:6x, NORMWERTE: die CertID einer echten Antwort
    /// enthaelt `issuerNameHash` und `issuerKeyHash`, die **openssl** gerechnet
    /// hat. Wir rechnen dieselben zwei Hashes ueber den Aussteller nach und
    /// verlangen Gleichheit.
    ///
    /// added 02.10.2026 23:6x: das ist kein Selbstvergleich. Der Erwartungswert
    /// kommt aus der Datei, die `openssl ocsp` geschrieben hat; wenn unsere
    /// Zaehldefinition (welche Bytes in den Hash gehen) falsch waere, faellt
    /// genau dieser Test. Und weil openssl es ist, ist die Gegenprobe
    /// unabhaengig — ein Fehler, der in beiden Implementierungen gleich
    /// steckt, ist ausgeschlossen, weil die Definition im RFC 6960 4.1.1
    /// steht und nicht in einem von uns.
    #[test]
    fn die_certid_stimmt_mit_openssl_ueberein() {
        for (name, antwort_pem, wurzel_pem) in [
            ("ECDSA", vektor::GUT_ECDSA, vektor::WURZEL_ECDSA),
            ("RSA", vektor::GUT_RSA, vektor::WURZEL_RSA),
        ] {
            let a = antwort_lesen(antwort_pem).expect("Antwort liest");
            let w = zertifikat(wurzel_pem);
            let name_hash = crate::sha2::sha1(&crate::x509::subject_der(&w).expect("subject"));
            let key_hash = crate::sha2::sha1(&crate::x509::spki_bits(&w).expect("spki"));
            assert_eq!(
                a.issuer_name_hash,
                name_hash.to_vec(),
                "{name}: issuerNameHash weicht von openssl ab"
            );
            assert_eq!(
                a.issuer_key_hash,
                key_hash.to_vec(),
                "{name}: issuerKeyHash weicht von openssl ab"
            );
            assert_eq!(
                a.hash_oid,
                vec![1u64, 3, 14, 3, 2, 26],
                "{name}: openssl rechnet fuer die CertID SHA-1"
            );
        }
    }

    /// added 02.10.2026 23:6x: die Seriennummer in der CertID ist die des
    /// Blatts — bei beiden Vektoren byteweise.
    #[test]
    fn die_certid_nennt_die_seriennummer_des_blatts() {
        // added 02.10.2026 23:ex: das Tupel hat **zwei** Glieder, nicht drei.
        // Die Fassung davor nahm `(antwort, wurzel, blatt)` und las kein
        // `w_pem` — GEMESSEN: „unused variable: `w_pem`". Die Wurzel wird hier
        // nicht gebraucht: die Seriennummer steht im Blatt, und die
        // Issuer-Hashes prueft `die_certid_stimmt_mit_openssl_ueberein`.
        let faelle = [
            (vektor::GUT_ECDSA, vektor::BLATT),
            (vektor::GUT_RSA, vektor::BLATT_RSA),
        ];
        for (a_der, b_der) in faelle {
            let a = antwort_lesen(a_der).expect("Antwort liest");
            let b = zertifikat(b_der);
            assert_eq!(a.seriennummer, b.seriennummer, "Seriennummer");
        }
    }

    /// added 02.10.2026 23:6x: `Gut` nur, wenn **alles** stimmt. Der
    /// ECDSA-Vektor traegt eine echte Signatur der CA ueber `tbsResponseData`.
    #[test]
    fn gute_antwort_mit_kettensignatur_ist_gut() {
        let a = antwort_lesen(vektor::GUT_ECDSA).expect("Antwort liest");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(beweise(&a, &b, &w, jetzt_in(&a)), Urteil::Gut);
    }

    /// added 02.10.2026 23:6x: derselbe Weg mit einer **RSA**-Signatur
    /// (`sha256WithRSAEncryption`), weil die ECDSA- und die RSA-Rechnung in
    /// `signatur.rs`/`ecdsa.rs` getrennt laufen und ein gemeinsamer Fehler
    /// sonst unentdeckt bliebe.
    #[test]
    fn gute_antwort_mit_rsa_signatur_ist_gut() {
        let a = antwort_lesen(vektor::GUT_RSA).expect("Antwort liest");
        let b = zertifikat(vektor::BLATT_RSA);
        let w = zertifikat(vektor::WURZEL_RSA);
        assert_eq!(beweise(&a, &b, &w, jetzt_in(&a)), Urteil::Gut);
    }

    /// added 02.10.2026 23:6x: ein **delegierter** Responder. Die Antwort
    /// traegt das Responder-Zertifikat in `certs`, es ist von derselben CA
    /// signiert und traegt `id-kp-OCSPSigning` — damit ist sie nach RFC 6960
    /// 4.2.2.2 auswertbar.
    #[test]
    fn delegierter_responder_ist_gut() {
        let a = antwort_lesen(vektor::GUT_DELEGIERT).expect("Antwort liest");
        assert_eq!(a.zertifikate.len(), 1, "der Vektor traegt ein Zertifikat");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(beweise(&a, &b, &w, jetzt_in(&a)), Urteil::Gut);
    }

    /// added 02.10.2026 23:6x: GEGENPROBE zum Delegierten. Derselbe Weg, aber
    /// das Responder-Zertifikat traegt `digitalSignature` statt
    /// `OCSPSigning`. Die Meldung muss **nennen**, woran es liegt — ein
    /// `Unbekannt` ohne Grund waere hier fast so gut wie gar nichts.
    #[test]
    fn responder_ohne_ocspsigning_wird_abgelehnt() {
        let a = antwort_lesen(vektor::GUT_OHNE_ERLAUBNIS).expect("Antwort liest");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(
            beweise(&a, &b, &w, jetzt_in(&a)),
            Urteil::Unbekannt("der Responder hat keine OCSPSigning-Erweiterung")
        );
    }

    /// added 02.10.2026 23:6x: GEGENPROBE zur CertID. Die Antwort ist **gut**
    /// und von der CA signiert — sie handelt aber vom `fremd.pem`, nicht von
    /// `blatt.pem`. Genau das ist der Fall, den ein Server nachmachen koennte,
    /// um eine gueltige Antwort fuer ein beliebiges Zertifikat beizulegen.
    #[test]
    fn antwort_ueber_ein_anderes_zertifikat_wird_abgelehnt() {
        let a = antwort_lesen(vektor::GUT_FREMD).expect("Antwort liest");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(
            beweise(&a, &b, &w, jetzt_in(&a)),
            Urteil::Unbekannt("die CertID nennt eine andere Seriennummer")
        );
        // UND mit dem **richtigen** Blatt muss derselbe Vektor durchgehen:
        // der Unterschied liegt allein an der Seriennummer, nicht an der
        // Antwort.
        let fremd = zertifikat(vektor::FREMD);
        assert_eq!(beweise(&a, &fremd, &w, jetzt_in(&a)), Urteil::Gut);
    }

    /// added 02.10.2026 23:6x: GEGENPROBEN zum Zeitfenster. Beide Vektoren
    /// sind gueltig; nur die Uhr steht falsch.
    #[test]
    fn das_zeitfenster_wird_geprueft() {
        let a = antwort_lesen(vektor::GUT_ECDSA).expect("Antwort liest");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(
            beweise(&a, &b, &w, a.this_update - 1),
            Urteil::Unbekannt("die Antwort ist noch nicht gueltig")
        );
        let ende = a.next_update.expect("nextUpdate");
        assert_eq!(
            beweise(&a, &b, &w, ende + 1),
            Urteil::Unbekannt("die Antwort ist abgelaufen")
        );
        // DIE RANDEN selbst muessen durchgehen: `thisUpdate` und `nextUpdate`
        // sind nach RFC 6960 4.2.1 **eingeschlossen** („the time at which the
        // status being indicated is known to be correct" bzw. „on or after").
        assert_eq!(beweise(&a, &b, &w, a.this_update), Urteil::Gut);
        assert_eq!(beweise(&a, &b, &w, ende), Urteil::Gut);
    }

    /// added 02.10.2026 23:6x: GEGENPROBE zur Signatur. Ein einziges
    /// geaendertes Byte in `tbsResponseData` muss die Antwort kippen — sonst
    /// prueft `beweise` gar nichts.
    #[test]
    fn ein_geaendertes_byte_in_der_signatur_wird_abgelehnt() {
        let mut kaputt = vektor::GUT_ECDSA.to_vec();
        // added 02.10.2026 23:6x: die Stelle ist **gemessen**, nicht geraten:
        // das ist der Anfang von `thisUpdate` (GENERALIZEDTIME, ASCII). Eine
        // Ziffer dort aendert die signierten Bytes und sonst nichts.
        let mut gefunden = false;
        for i in 0..kaputt.len() {
            if kaputt.len() > i && &kaputt[i..i + 4] == b"2026" {
                kaputt[i] = b'2';
                kaputt[i + 1] = b'0';
                kaputt[i + 2] = b'2';
                kaputt[i + 3] = b'7';
                gefunden = true;
                break;
            }
        }
        assert!(gefunden, "im Vektor steht kein Jahresanfang 2026");
        // added 02.10.2026 23:9x: die erste Fassung las die Antwort **vor**
        // der Manipulation und band sie an `a`; die wurde nie gelesen, und der
        // Uebersetzer meldete „this value is reassigned later and never used".
        let a = antwort_lesen(&kaputt).expect("die kaputte Antwort liest immer noch");
        let b = zertifikat(vektor::BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        // added 02.10.2026 23:6x: das Jahr steht im `thisUpdate`, **nicht** im
        // `tbs_roh` (die Zeit steht im `SingleResponse`, das ist aber
        // signiert — deshalb schlaegt die Signatur an, nicht der Aufbau).
        assert_eq!(a.cert_status, 0, "der Status ist unveraendert");
        assert!(
            !matches!(beweise(&a, &b, &w, jetzt_in(&a)), Urteil::Gut),
            "ein geaendertes Byte darf keine gueltige Antwort mehr ergeben"
        );
    }

    /// added 02.10.2026 23:6x: GEGENPROBE zu `responseStatus != successful`.
    /// `OcspFehler::KeineAntwort` ist **kein** Absturzgrund: der haeufigste
    /// Fall in der Wildbahn ist genau dieser.
    #[test]
    fn kein_responder_code_ist_ein_benannter_fehler() {
        for code in [1u8, 2, 3, 5, 6] {
            let mut v = vektor::GUT_ECDSA.to_vec();
            // added 02.10.2026 23:9x: DIE POSITION IST GEMESSEN, und die erste
            // Fassung dieses Tests nahm 2 und 3 — GEMESSEN: „left: 2, right:
            // 10". Grund: die Vektoren sind 708 Byte lang, ihre aeussere
            // SEQUENCE braucht also die **lange** Laengenkodierung
            // (`30 82 02 A6 …`, vier Byte Kopf nach dem Tag). `responseStatus`
            // steht damit bei 4, nicht bei 2. Gegenprobe in diesem Test: die
            // beiden Bytes werden **geprueft**, bevor sie veraendert werden.
            assert_eq!(&v[..3], &[0x30, 0x82, 0x02], "der Kopf ist lang kodiert");
            assert_eq!(v[4], 0x0A, "Position 4 ist das ENUMERATED");
            assert_eq!(v[5], 0x01, "und genau ein Byte lang");
            v[6] = code;
            assert_eq!(
                antwort_lesen(&v),
                Err(OcspFehler::KeineAntwort(code)),
                "responseStatus {code}"
            );
        }
    }

    /// added 02.10.2026 23:7x: `revoked` schlaegt **jedes** andere Ergebnis —
    /// auch wenn die Signatur kaputt waere. Der Reihenfolge-Vorrang ist
    /// ausdruecklich so gebaut (siehe `beweise`).
    #[test]
    fn widerruf_bleibt_widerruf() {
        let a = antwort_lesen(vektor::WIDERRUFEN).expect("Antwort liest");
        let b = zertifikat(vektor::WIDERRUF_BLATT);
        let w = zertifikat(vektor::WURZEL_ECDSA);
        assert_eq!(a.cert_status, 1, "der Vektor sagt revoked");
        // Mit falscher Uhr: trotzdem `Widerrufen`.
        assert_eq!(beweise(&a, &b, &w, 0), Urteil::Widerrufen);
        // Mit dem **falschen** Blatt: trotzdem `Widerrufen`.
        let anderes = zertifikat(vektor::BLATT);
        assert_eq!(beweise(&a, &anderes, &w, jetzt_in(&a)), Urteil::Widerrufen);
    }
}
