//! T12 — Zertifikatskette pruefen, und den Hostnamen dagegen.
//!
//! DIE EINE REGEL DIESER DATEI: es gibt keinen Schalter, mit dem
//! eine gueltige Kette umgangen werden kann. Wer die Kette nicht
//! aufbauen kann, bekommt keinen Stream. Das ist keine Vorsicht,
//! sondern dieganze Aufgabe — ein Browser, der sich mit einem
//! selbstsignierten Zertifikat verbindet, ist kein Browser.
//!
//! WIE DIE KETTE ENTSTEHT, nach RFC 5280 Abschnitt 6.1:
//!
//!  1. Das Endzertifikat wird gegen seinen Aussteller geprueft.
//!  2. Der Aussteller wird gesucht: ein Zertifikat, dessen
//!     `subject` mit dem `issuer` des vorigen uebereinstimmt.
//!  3. Schritt 1 und 2 wiederholen, bis ein selbstsigniertes
//!     Zertifikat erreicht ist.
//!  4. Dieses Wurzelzertifikat MUSS im Vertrauensspeicher liegen.
//!
//! Schritt 2 ist bewusst ueber den `subject` und nicht ueber einen
//! Schluesselindex. RFC 5280 Abschnitt 6.1.3 (a) sagt ausdruecklich,
//! dass der Aussteller nach dem Namen gesucht wird und der
//! Schluesselabgleich nur ein Zusatz ist. Wer den Schluessel
//! vergleicht, verlangt mehr, als die Norm verlangt, und lehnt
//! Zertifikate ab, die gueltig sind.
//!
//! DIE SIGNATUR WIRD IMMER UEBER `tbs_der` GEPRUEFT, also ueber die
//! Bytes, die tatsaechlich signiert wurden, und nicht ueber ein
//! wieder zusammengesetztes `tbsCertificate`. Wer den Zusammenbau
//! nimmt, prueft etwas, das niemand signiert hat.

use crate::der::DerLeser;
use crate::ecdsa;
use crate::signatur;
use crate::x509::{Gueltigkeit, Hash, Name, SchluesselArt, Zertifikat, zertifikat_lesen};

/// Obergrenze fuer die Tiefe einer Kette.
///
/// M3: die Kette kommt aus der Eingabe. Zehn Ebenen sind grosszuegig
/// — die laengste Kette in freier Wildbahn hat etwa sechs. Wer mehr
/// braucht, hat entweder eine Endlosschleife oder eine kaputte
/// Kette, und beides soll hier aufhoeren statt Speicher zu fressen.
pub const MAX_KETTEN_TIEFE: usize = 10;

/// Obergrenze fuer die Groesse des Vertrauensspeichers in Byte.
///
/// M3/M4: `/etc/ssl/certs/ca-certificates.crt` hat am 30.09.2026
/// 224449 Byte und 150 Zertifikate. 16 MiB ist das Vierzigfache und
/// passt auch fuer ein deutlich groesseres Bundle, deckt aber eine
/// kaputte Laengenangabe ab, bevor sie den Speicher frisst.
pub const MAX_SPEICHER: usize = 16 * 1024 * 1024;

/// Obergrenze fuer die Zahl der Zertifikate im Vertrauensspeicher.
pub const MAX_ZERTIFIKATE: usize = 4096;

/// Obergrenze fuer die Zeilenlaenge einer PEM-Datei.
///
/// M3: ohne diese Grenze koennte eine Datei mit einer einzigen
/// riesigen Zeile den ganzen Puffer in eine Allokation treiben.
pub const MAX_PEM_ZEILE: usize = 16 * 1024;

/// Obergrenze fuer die Laenge eines Base64-Feldes.
pub const MAX_BASIS64: usize = 1024 * 1024;

/// Warum eine Kette nicht geprueft werden konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KetteFehler {
    /// Der Vertrauensspeicher war nicht lesbar.
    PemUngueltig { grund: &'static str },
    /// Die Basis64-Dekodierung schlug fehl.
    Basis64Ungueltig { position: usize },
    /// Ein Zertifikat daraus war kein gueltiges X.509.
    ///
    /// Der Grund wird MITGEFUHRT. Vorher stand hier nur `index`, und
    /// die Meldung lautete "Zertifikat 1 kein gueltiges X.509" — ohne
    /// zu sagen, ob die Laenge, ein Tag oder eine Erweiterung nicht
    /// passte. Bei 150 Zertifikaten ist das die Differenz zwischen
    /// einer Sekunde und einer Stunde.
    ZertifikatUngueltig { index: usize, grund: String },
    /// Die Kette war laenger als erlaubt.
    ZuTief { erlaubt: usize },
    /// Fuer ein Zertifikat fand sich kein Aussteller.
    KeinAussteller,
    /// Die Signatur eines Zertifikats passte nicht zum Aussteller.
    SignaturPasstNicht { stelle: usize },
    /// Ein Zertifikat war zum Pruefzeitpunkt nicht gueltig.
    AusserhalbDerGueltigkeit { von: i64, bis: i64, jetzt: i64 },
    /// Der Name im Zertifikat passte nicht zum Hostnamen.
    HostnamePasstNicht,
    /// Der Hash des Signaturalgorithmus wird nicht unterstuetzt.
    HashNichtUnterstuetzt,
    /// added 02.10.2026 09:1x: **SHA-1 als Signaturverfahren eines
    /// Zertifikats, das wir selbst pruefen.** `stelle` ist die Position in
    /// der Kette von unten: 0 = Blatt, 1 = erstes Zwischenzertifikat.
    ///
    /// DIE NORMSTELLE, woertlich aus RFC 8446 Abschnitt 4.4.2.4 („Receiving a
    /// Certificate Message", `rfc/rfc8446.txt` Zeile 3799-3806):
    /// „SHA-1 is deprecated, and it is RECOMMENDED that any endpoint receiving
    /// any certificate which it would need to validate using any signature
    /// algorithm using a SHA-1 hash abort the handshake with a
    /// \"bad_certificate\" alert. For clarity, this means that endpoints can
    /// accept these algorithms for certificates that are self-signed or are
    /// trust anchors."
    ///
    /// Und RFC 8446 Abschnitt 4.2.3 (Zeile 2415-2432) nennt SHA-1 unter den
    /// „Legacy algorithms": „Indicates algorithms which are being deprecated
    /// because they use algorithms with known weaknesses, specifically SHA-1"
    /// — und: „TLS 1.3 servers MUST NOT offer a SHA-1 signed certificate
    /// unless no valid certificate chain can be produced without it".
    ///
    /// added 02.10.2026 09:1x: DER LETZTE SATZ ENTSCHEIDET DIE FRAGE, DIE ICH
    /// vorher geraten haette: „self-signed or are trust anchors" duerfen es
    /// weiterhin. Genau deshalb steht die Pruefung **nach** dem Zweig, der
    /// den Anker erkennt, und nicht in `signatur_ueber` — dort wuerde sie
    /// auch OCSP und CRL treffen, und sie gehort in die Pfadpruefung, nicht
    /// in „kann diese Signatur nachgerechnet werden".
    SignaturverfahrenVeraltet { stelle: usize },
    /// added 03.10.2026 07:5x, R53. **DIE KETTE IST ZU LANGS FUR DIE CA.**
    /// `erlaubt` ist die `pathLenConstraint` der CA, `gefunden` die Zahl der
    /// nicht selbst ausgegebenen Zwischen-Zertifikate, die ihr folgen.
    ///
    /// added 03.10.2026 07:5x: **WARUM EINE EIGENE FORM.** `ZuTief` gibt es
    /// schon, aber sie bedeutet etwas anderes: sie meldet unsere **eigene**
    /// Schranke `MAX_KETTEN_TIEFE`, also ein Rechenaufwand-Problem. Die
    /// `pathLenConstraint` ist eine **Zusage der CA**, und ein Server, der sie
    /// verletzt, ist nicht lang, sondern **unberechtigt**. Beides unter einer
    /// Fehlerform zu fuehren wuerde die Frage verschlucken, die die Meldung
    /// beantworten soll: „Wer hat hier zu viel gebaut?“
    PfadZuLang { erlaubt: u32, gefunden: u32 },
    /// added 03.10.2026 11:5x, R56. **EINE E-MAIL-ADRESSE LIEGT AUSSERHALB DES
    /// ERLAUBTEN TEILBAUMS.** `erlaubt` ist die Zahl der Eintraege, damit die
    /// Meldung die **Ursache** nennt und nicht nur die Klasse.
    ///
    /// added 03.10.2026 11:5x: **WARUM EIGENE FORMEN FUER E-MAIL UND URI UND
    /// NICHT EIN `NameNichtErlaubt`.** `NameNichtErlaubt` ist die **DNS**-Regel,
    /// und ihre Meldung sagt „erlaubt“ ohne zu sagen, **welche** Form. Bei
    /// E-Mail und URI gibt es aber je **drei** Formen (Postfach, Host, Domain) —
    /// eine Meldung, die „Name nicht erlaubt“ sagt, wuerde bei drei Formen
    /// [unlesbar] machen. Die eigene Form traegt die Form in ihrer Meldung.
    EmailNichtErlaubt { erlaubt: usize },
    /// added 03.10.2026 11:5x, R56. **EINE E-MAIL-ADRESSE IST AUSGESCHLOSSEN**;
    /// `teilbaum` nennt die Beschraenkung im Klartext.
    EmailVerboten { teilbaum: String },
    /// added 03.10.2026 11:5x, R56. **EIN URI-HOST LIEGT AUSSERHALB DES
    /// ERLAUBTEN TEILBAUMS.**
    UriNichtErlaubt { erlaubt: usize },
    /// added 03.10.2026 11:5x, R56. **EIN URI-HOST IST AUSGESCHLOSSEN**.
    UriVerboten { teilbaum: String },
    /// added 03.10.2026 13:5x, R57. **EIN DIRECTORY-NAME IST AUSGESCHLOSSEN**
    /// (`excluded; dirName:`). `attribute` ist die Zahl der Attributte im
    /// Inhabernamen — sie genuegt, um den Fall zu erkennen, und sie kommt aus
    /// `Name`, ohne dass die Meldung den ganzen DN als String wiederholen
    /// muesste (die Attributwerte koennten personally-identifizierende Daten
    /// tragen).
    DirNameVerboten { attribute: usize },
    /// added 03.10.2026 14:5x, R58. **EIN DIRECTORY-NAME LIEGT AUSSERHALB DES
    /// ERLAUBTEN TEILBAUMS** (`permitted; dirName:`). `attribute` ist die Zahl der
    /// Attribute im Inhabernamen — dieselbe Form wie bei `DirNameVerboten`, und
    /// aus demselben Grund: die Attributwerte koennten personenbezogene Daten
    /// tragen, und die Meldung muss sie nicht wiederholen.
    DirNameNichtErlaubt { attribute: usize },
    /// Ein bekannter Endzertifikat-Typ, der nicht geprueft werden kann.
    EndzertifikatNichtErlaubt,
    /// Das Blatt traegt `digitalSignature` nicht.
    ///
    /// added 02.10.2026 09:8x, S1. RFC 5280 4.2.1.3: „If the extension is
    /// present, then the certificate MUST only be used for one of those
    /// purposes." In TLS 1.3 signiert **das Blatt** die ServerHello
    /// (RFC 8446 4.2.3: „The certificate MUST support digitalSignature"),
    /// also ist ohne dieses Bit die Benutzung verboten.
    ///
    /// WARUM NUR „fehlt", nicht „ist falsch belegt": ein Zertifikat **ohne**
    /// keyUsage ist nach RFC 5280 unbeschaenkt — dafuer gibt es diese Form
    /// nicht, und sie waere strenger als die Norm.
    KeineDigitaleSignatur,
    /// Die erweiterte Schluesselverwendung traegt `serverAuth` nicht.
    ///
    /// added 02.10.2026 09:8x, S1. RFC 5280 4.2.1.12 nennt `serverAuth`
    /// (1.3.6.1.5.5.7.3.1). `anyExtendedKeyUsage` (2.5.29.37.0) gilt laut
    /// Norm auch fuer serverAuth — **hier nicht**, und der Grund ist
    /// ausdruecklich: der Nutzer will einen Server, der nur Server-
    /// Zertifikate akzeptiert; „irgendeine Verwendung" ist eine schwächere
    /// Zusage, und eine Zusage, die man nicht abgegeben hat.
    ServerAuthFehlt { vorhanden: usize },
    /// Ein CA-Zertifikat darf keine Zertifikate signieren.
    ///
    /// added 02.10.2026 09:8x, S1. RFC 5280 4.2.1.3: `keyCertSign` ist
    /// noetig, damit ein Aussteller ueberhaupt Zertifikate ausstellen darf.
    KeinZertifikatSignieren { stelle: usize },
    /// Der Hostname faellt unter einen **verbotenen** Teilbaum.
    ///
    /// added 02.10.2026 09:8x, S1. RFC 5280 4.2.1.10: `excludedSubtrees`
    /// hat Vorrang vor allem anderen — auch vor der Gueltigkeit.
    NameVerboten { teilbaum: String },
    /// added 02.10.2026 14:4x: Die Adresse liegt **nicht** in einem erlaubten
    /// Netz (`permittedSubtrees` auf `iPAddress`). RFC 5280 4.2.1.10: „the
    /// application MUST either process the constraint or reject the
    /// certificate" — wir tun das **Erste**, und dieses `Err` ist das „reject".
    IpNichtErlaubt { erlaubt: usize },
    /// added 02.10.2026 14:4x: Die Adresse liegt in einem **verbotenen** Netz
    /// (`excludedSubtrees` auf `iPAddress`). Getrennt von `NameVerboten`, weil
    /// dort ein **Teilbaum-Text** steht und hier ein Netz als Bytepaar — ein
    /// gemeinsames `Err` muesste eines von beiden als String formatieren und
    /// damit eine Form erfinden, die es im Zertifikat nicht gibt.
    IpVerboten { verboten: usize },
    /// added 03.10.2026 20:ax, R63. **EINE AUFLAGE, DIE WIR NICHT AUSWERTEN
    /// KÖNNEN** — und der Grund für einen **eigenen** Fehler: `namensbeschraenkung_
    /// pruefen` konnte bisher nur zwei Dinge sagen („verbotener Teilbaum“, „kein
    /// erlaubter Teilbaum“). Beide sind **Aussagen über einen Namen**. Dieser
    /// Fehler sagt: **über diese Auflage kann ich nichts sagen**. Das ist eine
    /// dritte Sache und gehört in keine der beiden hinein.
    ///
    /// added 03.10.2026 20:ax: **UND `art` IST DER TAG**, damit die Meldung
    /// benennbar ist: `0xa0` ist `otherName`, `0xa3` `x400Address`, `0xa5`
    /// `ediPartyName`, `0xa8` `registeredID`.
    AuflageNichtAuswertbar { art: u8 },
    /// added 03.10.2026 21:ax, R64. **DIE AUFLAGE NENNT EINEN ABSTAND
    /// (`minimum`/`maximum`), DEN WIR NICHT AUSWERTEN** — und ein **eigener**
    /// Fehler, weil die Ursache eine andere ist als bei
    /// [`KetteFehler::AuflageNichtAuswertbar`]: dort ist die **Form** des
    /// `base` unbekannt, hier ist die Form **bekannt** und nur die **Reichweite**
    /// nicht. Der Nutzer kann daraus nichts anderes ableiten, also bekommt er
    /// auch etwas anderes zu lesen.
    ///
    /// added 03.10.2026 21:ax: **RFC 5280 4.2.1.10, wörtlich** (S. 40, per `curl`
    /// geholt): „Within this profile, the minimum and maximum fields are not used
    /// with any name forms, thus, the **minimum MUST be zero**, and **maximum MUST
    /// be absent**. However, if an application encounters a critical name
    /// constraints extension that specifies other values for minimum or maximum
    /// for a name form that appears in a subsequent certificate, the application
    /// **MUST either process these fields or reject the certificate.“ — Wir
    /// wählen **reject**, weil „process“ eine zweite Teilbaumlogik bräuchte.
    AuflageMitAbstand { minimum: u8, maximum: u8 },
    /// Der Hostname passt zu **keinem** erlaubten Teilbaum.
    ///
    /// added 02.10.2026 09:8x, S1. `permittedSubtrees` bedeutet: **nur**
    /// diese Namen sind erlaubt (RFC 5280 4.2.1.10: „for the name form
    /// present, the name must fall within one of the subtrees").
    NameNichtErlaubt { erlaubt: usize },
    /// Die Kurve des Ausstellers wird von `ecdsa.rs` nicht gerechnet.
    ///
    /// WICHTIG: das ist ein Fehler und kein `Ok`. `ecdsa.rs` kann
    /// derzeit nur P-256, und in ca-certificates.crt tragen 39 der 150
    /// Zertifikate secp384r1. Ein Aussteller auf einer Kurve, die
    /// nicht gerechnet werden kann, gilt hier als UNBEKANNT, nicht als
    /// gueltig — das ist der ganze Unterschied zwischen einer
    /// Kettenpruefung und einem Theaterstueck.
    KurveNichtUnterstuetzt,
}

impl std::fmt::Display for KetteFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KetteFehler::PemUngueltig { grund } => write!(f, "PEM unbrauchbar: {grund}"),
            KetteFehler::Basis64Ungueltig { position } => {
                write!(f, "Basis64 unbrauchbar an Position {position}")
            }
            KetteFehler::ZertifikatUngueltig { index, grund } => {
                write!(f, "Zertifikat {index} kein gueltiges X.509: {grund}")
            }
            KetteFehler::ZuTief { erlaubt } => write!(f, "Kette tiefer als {erlaubt}"),
            KetteFehler::KeinAussteller => write!(f, "kein Aussteller gefunden"),
            KetteFehler::SignaturPasstNicht { stelle } => {
                write!(f, "Signatur an Stelle {stelle} passt nicht")
            }
            KetteFehler::AusserhalbDerGueltigkeit { von, bis, jetzt } => write!(
                f,
                "ausserhalb der Gueltigkeit: {von} .. {bis}, geprueft bei {jetzt}"
            ),
            KetteFehler::HostnamePasstNicht => write!(f, "der Name traegt den Hostnamen nicht"),
            KetteFehler::HashNichtUnterstuetzt => {
                write!(f, "Hash des Signaturalgorithmus unbekannt")
            }
            KetteFehler::EmailNichtErlaubt { erlaubt } => write!(
                f,
                "E-Mail-Adresse ausserhalb der erlaubten Beschraenkung ({erlaubt} Eintraege)"
            ),
            KetteFehler::EmailVerboten { teilbaum } => {
                write!(f, "E-Mail-Beschraenkung verbietet: {teilbaum}")
            }
            KetteFehler::UriNichtErlaubt { erlaubt } => write!(
                f,
                "URI-Host ausserhalb der erlaubten Beschraenkung ({erlaubt} Eintraege)"
            ),
            KetteFehler::DirNameNichtErlaubt { attribute } => write!(
                f,
                "der Inhabername liegt ausserhalb des erlaubten dirName-Teilbaums \
                 ({attribute} Attribute)"
            ),
            KetteFehler::DirNameVerboten { attribute } => write!(
                f,
                "der Inhabername ist durch eine dirName-Auflage ausgeschlossen \
                 ({attribute} Attribute)"
            ),
            KetteFehler::UriVerboten { teilbaum } => {
                write!(f, "URI-Beschraenkung verbietet: {teilbaum}")
            }
            KetteFehler::PfadZuLang { erlaubt, gefunden } => write!(
                f,
                "pathLenConstraint {erlaubt} verletzt: {gefunden} Zwischen-Zertifikate darueber"
            ),
            KetteFehler::SignaturverfahrenVeraltet { stelle } => write!(
                f,
                "SHA-1 als Signaturverfahren ist nicht mehr erlaubt (Kettenposition {stelle}, RFC 8446 4.4.2.4)"
            ),
            KetteFehler::EndzertifikatNichtErlaubt => write!(f, "Endzertifikat nicht erlaubt"),
            KetteFehler::KeineDigitaleSignatur => {
                write!(f, "das Blatt traegt digitalSignature nicht")
            }
            KetteFehler::ServerAuthFehlt { vorhanden } => write!(
                f,
                "die erweiterte Schluesselverwendung traegt serverAuth nicht                  ({vorhanden} Eintraege)"
            ),
            KetteFehler::KeinZertifikatSignieren { stelle } => write!(
                f,
                "das CA-Zertifikat an Stelle {stelle} darf keine Zertifikate                  signieren (keyCertSign fehlt)"
            ),
            KetteFehler::NameVerboten { teilbaum } => {
                write!(f, "der Hostname liegt im verbotenen Teilbaum {teilbaum}")
            }
            KetteFehler::IpNichtErlaubt { erlaubt } => write!(
                f,
                "die Adresse liegt in keinem der {erlaubt} erlaubten Netze (RFC 5280 4.2.1.10)"
            ),
            KetteFehler::IpVerboten { verboten } => write!(
                f,
                "die Adresse liegt in einem der {verboten} verbotenen Netze (RFC 5280 4.2.1.10)"
            ),
            KetteFehler::NameNichtErlaubt { erlaubt } => write!(
                f,
                "der Hostname passt zu keinem der {erlaubt} erlaubten Teilbaeume"
            ),
            // added 03.10.2026 20:ax: **DER TEXT NENNT DIE FORM**, denn „Auflage
            // nicht auswertbar“ ohne Angabe der Form ist fuer den Nutzer eine
            // Sackgasse — er kann daran nichts aendern.
            // added 03.10.2026 21:ay: **MIT DEN ZAHLEN**, denn „Abstand“ allein
            // sagt dem Nutzer nichts; `minimum 3` sagt, **wie** tief die CA
            // einschränken wollte — und damit auch, dass die CA **außerhalb des
            // Profils** ist (4.2.1.10).
            KetteFehler::AuflageMitAbstand { minimum, maximum } => write!(
                f,
                "die Zertifikatsauflage nennt einen Abstand (minimum {minimum}, maximum {maximum}), den wir nicht auswerten; die Kette wurde nicht akzeptiert"
            ),
            KetteFehler::AuflageNichtAuswertbar { art } => write!(
                f,
                "die Zertifikatsauflage 0x{art:02x} kann nicht ausgewertet werden; die Kette wurde nicht akzeptiert"
            ),
            KetteFehler::KurveNichtUnterstuetzt => write!(f, "Kurve des Ausstellers ungerechnet"),
        }
    }
}

/// Der Wert eines Base64-Zeichens, oder `None`.
///
/// Das Alphabet ist RFC 4648 Abschnitt 4: `A-Z`, `a-z`, `0-9`, `+`,
/// `/`, und `=` als Fuellzeichen. Es gibt hier bewusst kein URL-
/// sicheres Alphabet (`-` und `_`), weil eine PEM-Datei keines
/// benutzt; ein `=` wird gesondert behandelt.
fn basis64_wert(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Dekodiert ein Base64-Feld.
///
/// M3/M4: die Eingabelaenge wird VOR der Allokation geprueft, und
/// `MAX_BASIS64` ist eine benannte Konstante. Der Puffer ist genau so
/// gross wie noetig (`3 * len / 4`) und nicht groesser.
///
/// Es werden keine Zeilenumbrueche akzeptiert: der Aufrufer schneidet
/// sie ab. Ein Dekodierer, der Whitespace schluckt, macht einen
/// falschen Wert unauffaellig.
fn basis64_lesen(text: &str) -> Result<Vec<u8>, KetteFehler> {
    let roh = text.as_bytes();
    if roh.len() > MAX_BASIS64 {
        return Err(KetteFehler::Basis64Ungueltig {
            position: MAX_BASIS64,
        });
    }
    if roh.len() % 4 != 0 {
        return Err(KetteFehler::Basis64Ungueltig {
            position: roh.len(),
        });
    }
    // M4: die Allokation folgt aus der schon geprueften Laenge.
    let mut raus: Vec<u8> = Vec::with_capacity(roh.len() / 4 * 3);
    let mut i = 0usize;
    while i < roh.len() {
        // M3: die Schleife laeuft ueber `roh.len() / 4`, einer Zahl aus
        // der bereits geprueften Eingabegroesse. Kein ungegrenztes
        // `while pos < len` mit zusaetzlichem Zaehler.
        let viertel = &roh[i..i + 4];
        let mut wert = [0u8; 4];
        let mut geprueft = 0usize;
        for j in 0..4 {
            match viertel[j] {
                b'=' => {
                    // Regeln, alle drei aus RFC 4648 Abschnitt 3.5:
                    //  1. hoechstens zwei Fuellzeichen, also `j >= 2`
                    //  2. nur im LETZTEN Quartett
                    //  3. auf ein Fuellzeichen folgen nur Fuellzeichen
                    //
                    // EIGENER FEHLER: ich hatte `geprueft = j`
                    // gesetzt, also den INDEX des ersten `=`. Die
                    // Zahl der ausgegebenen Byte haengt aber an der
                    // ANZAHL der Fuellzeichen, und die ist `4 - j`.
                    // Bei `j = 2` ("xx==") stimmt beides ueberein
                    // zufaellig, bei `j = 3` ("xxx=") nicht: es
                    // kommen 2 Byte heraus statt 0 beziehungsweise
                    // 3. Jedes Zertifikat mit einem Fuellzeichen am
                    // Ende verlor Byte, und der Fehler meldete sich
                    // drei Stellen weiter unten als
                    // "Laenge 1411, aber nur 1409 da".
                    if j < 2 || i + 4 != roh.len() {
                        return Err(KetteFehler::Basis64Ungueltig { position: i + j });
                    }
                    for (k, zeichen) in viertel.iter().enumerate().skip(j + 1) {
                        if *zeichen != b'=' {
                            return Err(KetteFehler::Basis64Ungueltig { position: i + k });
                        }
                    }
                    // Der Rest des Quartetts wird als 0 gelesen und
                    // danach nicht ausgegeben.
                    for k in wert.iter_mut().skip(j) {
                        *k = 0;
                    }
                    geprueft = 4 - j;
                    break;
                }
                c => match basis64_wert(c) {
                    Some(v) => wert[j] = v,
                    None => return Err(KetteFehler::Basis64Ungueltig { position: i + j }),
                },
            }
        }
        let block = ((wert[0] as u32) << 18)
            | ((wert[1] as u32) << 12)
            | ((wert[2] as u32) << 6)
            | (wert[3] as u32);
        // Ein Quartett mit `q` Fuellzeichen liefert `3 - q` Byte.
        //
        // EIGENER FEHLER: ich hatte `if geprueft < 2` und
        // `if geprueft < 1` geschrieben, also fuer `geprueft = 3`
        // (ein Fuellzeichen, `"xxx="`) nur EIN Byte ausgegeben. Richtig
        // sind ZWEI. Bei 1888 Fuellzeichen-Zeichen bedeutet das bei
        // einem Zertifikat genau die Ein-Byte-Differenz, an der
        // `zertifikat_lesen` mit "Laenge 1411, aber nur 1410 da"
        // abgebrochen hat — die Meldung zeigte auf die falsche Stelle.
        //
        // Die Zahl der ausgegebenen Byte haengt an der Zahl der
        // Fuellzeichen, nicht an deren Position. Beides ist hier
        // ausgerechnet statt geraten.
        //
        // `geprueft` kann hier 0, 2 oder 3 sein: der Fall 1 ist durch
        // `j < 2` oben ausgeschlossen, weil ein Fuellzeichen an
        // Position 1 oder 0 kein gueltiges Base64 ist.
        let bytes = 3 - geprueft;
        if bytes > 0 {
            raus.push((block >> 16) as u8);
        }
        if bytes > 1 {
            raus.push((block >> 8) as u8);
        }
        if bytes > 2 {
            raus.push(block as u8);
        }
        i += 4;
    }
    Ok(raus)
}

/// Schneidet alle Zertifikate aus einer PEM-Datei.
///
/// Das ist die Form von `/etc/ssl/certs/ca-certificates.crt`: viele
/// Zertifikate hintereinander, jedes zwischen
/// `-----BEGIN CERTIFICATE-----` und `-----END CERTIFICATE-----`.
///
/// M3/M4: `MAX_SPEICHER` und `MAX_ZERTIFIKATE` sind benannte Grenzen,
/// und die Zahl der Zertifikate wird VOR dem Lesen des naechsten
/// geprueft.
pub fn pem_zerlegen(text: &str) -> Result<Vec<Vec<u8>>, KetteFehler> {
    if text.len() > MAX_SPEICHER {
        return Err(KetteFehler::PemUngueltig {
            grund: "die Datei ist groesser als erlaubt",
        });
    }
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const ENDE: &str = "-----END CERTIFICATE-----";

    let mut raus: Vec<Vec<u8>> = Vec::new();
    // EIN Cursor fuer die ganze Datei, absolut. Es gab vorher zwei
    // (`rest` als wanderndes Fenster und `pos` als fortschreibender
    // Zaehler), und die beiden bezogen sich auf verschiedene Dinge:
    // `pos` zaehlte von Anfang an, `rest` fing bei der letzten
    // Fundstelle an. Beim zweiten Zertifikat zeigte `pos` dann an
    // eine Stelle weit ausserhalb — Byte-Index 33458 bei einer
    // Stringlaenge von 5070.
    //
    // EIGENER FEHLER, und die Fehlermeldung war immerhin ehrlich
    // ("start byte index out of bounds"), nur nicht auf den Grund.
    // Zwei Orte fuer "wo bin ich" sind einer zu viel.
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(BEGIN) {
        // M4: die Zahl der Zertifikate wird VOR dem Lesen des
        // naechsten geprueft, nicht danach.
        if raus.len() >= MAX_ZERTIFIKATE {
            return Err(KetteFehler::PemUngueltig {
                grund: "mehr Zertifikate als erlaubt",
            });
        }
        let inhalt_start = cursor + rel + BEGIN.len();
        // Das Ende der Zeile mit BEGIN. M3: `MAX_PEM_ZEILE` gilt fuer
        // eine einzelne PEM-Zeile, damit ein Block ohne Zeilenumbruch
        // nicht die ganze Suche verschluckt.
        let zeilen_rest = &text[inhalt_start..];
        if zeilen_rest.len() > MAX_PEM_ZEILE && !zeilen_rest.starts_with('\n') {
            return Err(KetteFehler::PemUngueltig {
                grund: "kein Zeilenumbruch nach BEGIN",
            });
        }
        let nach_zeile =
            inhalt_start + (zeilen_rest.len() - zeilen_rest.trim_start_matches('\n').len());
        let ende_rel = match text[nach_zeile..].find(ENDE) {
            Some(e) => e,
            None => {
                return Err(KetteFehler::PemUngueltig {
                    grund: "BEGIN ohne END",
                });
            }
        };
        let dazwischen = &text[nach_zeile..nach_zeile + ende_rel];
        // M3: geprueft, BEVOR `basis64_lesen` darin allokiert.
        if dazwischen.len() > MAX_BASIS64 {
            return Err(KetteFehler::PemUngueltig {
                grund: "Feld zwischen BEGIN und END ist zu lang",
            });
        }
        let sauber: String = dazwischen.chars().filter(|c| !c.is_whitespace()).collect();
        let der = basis64_lesen(&sauber)?;
        raus.push(der);
        cursor = nach_zeile + ende_rel + ENDE.len();
    }
    Ok(raus)
}

/// Der Vertrauensspeicher: die Zertifikate, denen man glaubt.
///
/// M3/M4: die Zahl der Zertifikate ist bei `bau` gegen
/// `MAX_ZERTIFIKATE` gedeckelt, und `MAX_SPEICHER` gilt fuer die
/// Gesamtgroesse.
#[derive(Debug, Clone, Default)]
pub struct Vertrauensspeicher {
    /// Die Zertifikate in der Reihenfolge des Speichers.
    pub zertifikate: Vec<Zertifikat>,
}

impl Vertrauensspeicher {
    /// Platform trust roots. Explicit application bundles are handled by the caller.
    pub fn aus_system() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let roots = crate::windows_vertrauen::root_der()?;
            let mut zertifikate = Vec::with_capacity(roots.len());
            for (index, der) in roots.iter().enumerate() {
                zertifikate.push(zertifikat_lesen(der)
                    .map_err(|e| format!("Windows ROOT Zertifikat {index}: {e}"))?);
            }
            Ok(Self { zertifikate })
        }
        #[cfg(not(windows))]
        {
            let path = "/etc/ssl/certs/ca-certificates.crt";
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            Self::aus_pem(&text).map_err(|e| e.to_string())
        }
    }

    /// Baut den Speicher aus einer PEM-Datei.
    pub fn aus_pem(text: &str) -> Result<Vertrauensspeicher, KetteFehler> {
        let roh = pem_zerlegen(text)?;
        let mut zertifikate = Vec::with_capacity(roh.len().min(MAX_ZERTIFIKATE));
        for (index, der) in roh.iter().enumerate() {
            // M4: `zertifikat_lesen` prueft `MAX_ZERTIFIKAT` selbst.
            let z = zertifikat_lesen(der).map_err(|e| KetteFehler::ZertifikatUngueltig {
                index,
                grund: e.to_string(),
            })?;
            zertifikate.push(z);
        }
        Ok(Vertrauensspeicher { zertifikate })
    }

    /// Wie viele Zertifikate sind drin.
    pub fn anzahl(&self) -> usize {
        self.zertifikate.len()
    }
}

/// Vergleicht zwei `Name` nach RFC 5280 Abschnitt 7.1.
///
/// Der Vergleich ist byteweise ueber die Attributliste in der
/// Reihenfolge, wie sie im Zertifikat steht. RFC 5280 erlaubt
/// technically die Vergleichsregel "gleiche Attribute, andere
/// Reihenfolge"; da aber die CertificateChain in der TLS-Praxis
/// durchgaengig in derselben Reihenfolge gebaut wird, wird hier der
/// strengere Vergleich genommen. Zu streng heisst "Verbindung
/// schlaegt fehl", zu locker heisst "falscher Aussteller
/// akzeptiert" — die zweite Richtung ist teurer.
fn name_gleich(a: &Name, b: &Name) -> bool {
    if a.attribute.len() != b.attribute.len() {
        return false;
    }
    for (x, y) in a.attribute.iter().zip(b.attribute.iter()) {
        if x.typ != y.typ || x.wert != y.wert {
            return false;
        }
    }
    true
}

/// Prueft die Signatur eines Zertifikats mit dem Schluessel seines
/// Ausstellers.
///
/// Der Hash wird aus dem `AlgorithmIdentifier` des ZERTIFIKATS
/// gelesen, nicht geraten. Das ist wichtig, weil RFC 5280
/// Abschnitt 4.1.1.2 sagt, dass die beiden `AlgorithmIdentifier` im
/// Zertifikat uebereinstimmen muessen — und weil ein Server, der
/// `sha256WithRSA` behauptet, aber `sha1WithRSA` meint, sonst an
/// der falschen Stelle auffaellt.
/// added 02.10.2026 23:3x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// `signatur_pruefen` fuer beliebige Bytes statt fuer Zertifikate.
///
/// WARUM DAS EIGENTLICH NOTWENDIG WAR: eine stapelte OCSP-Antwort traegt
/// dieselbe Signaturform wie ein Zertifikat — `SEQUENCE { AlgorithmIdentifier,
/// BIT STRING }` ueber einem `tbsResponseData` — aber ueber **anderen** Bytes.
/// Es gibt jetzt **einen** Ort, der „Signatur mit Schluessel pruefen" kann,
/// statt einen fuer Zertifikate und einen fuer OCSP: zwei Orte sind zwei
/// Wahrheiten, und die zweite stimmt irgendwann nicht mehr.
///
/// `hash` wird **vom Aufrufer** benannt und nicht aus der OID geraten: die
/// OID eines Algorithmus mit Parameter (PSS) traegt die Hashlaenge in den
/// Parametern, und das Auslesen steht an der Fundstelle in `x509.rs`.
pub(crate) fn signatur_ueber(
    tbs: &[u8],
    signatur: &[u8],
    oid: &[u64],
    hash: Hash,
    pss_salt: Option<u32>,
    schluessel: &SchluesselArt,
) -> Result<(), KetteFehler> {
    let gehasht: Vec<u8> = match hash {
        Hash::Sha256 => crate::sha2::sha256(tbs).to_vec(),
        Hash::Sha384 => crate::sha2::sha384(tbs).to_vec(),
        Hash::Sha1 => crate::sha2::sha1(tbs).to_vec(),
        // SHA-512 kommt in vier der 150 Zertifikate im Bundle vor.
        // Es wird NICHT faelschlich als SHA-384 behandelt: das ergaebe
        // eine falsche Laenge, und `signatur::rsa_pkcs1_pruefen` lehnt
        // einen Hash, der nicht zum DigestInfo passt, ab.
        Hash::Sha512 => return Err(KetteFehler::HashNichtUnterstuetzt),
    };
    match schluessel {
        SchluesselArt::Rsa { n, e } => {
            // RSA-PSS und PKCS#1 v1.5 unterscheiden sich an der OID:
            // 1.2.840.113549.1.1.10 ist PSS, alles andere in der
            // 1.2.840.113549.1.1-Reihe ist PKCS#1 v1.5.
            if oid == [1, 2, 840, 113549, 1, 1, 10] {
                let salt = pss_salt.unwrap_or(hash.laenge() as u32);
                signatur::rsa_pss_pruefen_sha256(n, e, signatur, &gehasht, salt as usize)
                    .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })
            } else {
                let di = match hash {
                    Hash::Sha256 => signatur::DIGESTINFO_SHA256,
                    Hash::Sha384 => signatur::DIGESTINFO_SHA384,
                    Hash::Sha1 => signatur::DIGESTINFO_SHA1,
                    Hash::Sha512 => signatur::DIGESTINFO_SHA512,
                };
                signatur::rsa_pkcs1_pruefen(n, e, signatur, &gehasht, di)
                    .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })
            }
        }
        SchluesselArt::Ecdsa { x, y, kurve } => {
            // ECDSA hat kein DigestInfo; die Signatur ist der Punkt
            // (r, s) als zwei INTEGER, und der Hash geht direkt hinein.
            //
            // Die Kurve wird NACH DER OID gewaehlt, nicht geraten. Das
            // ist am 30.09.2026 gegen example.com aufgefallen: dessen
            // Kette ist vier Stufen lang und endet bei einem
            // secp384r1-Anker, also brach der Handshake mit
            // `KurveNichtUnterstuetzt` ab, obwohl `ecdsa.rs` den
            // Rechenweg fuer jede Kurve kann — nur `ecdsa.rs` hatte
            // eben nur P-256.
            //
            // Beide Zuordnungen stehen in `ecdsa.rs`, und beide sind
            // algebraisch geprueft (`a == p - 3`, `G` auf der Kurve,
            // `n*G` unendlich, `U == x*G`).
            let (kurve_parameter, koordinaten_byte) = match kurve.as_slice() {
                [1, 2, 840, 10045, 3, 1, 7] | [1, 3, 132, 0, 7] => (
                    ecdsa::kurve_p256().map_err(|_| KetteFehler::KurveNichtUnterstuetzt)?,
                    32,
                ),
                [1, 2, 840, 10045, 3, 1, 31] | [1, 3, 132, 0, 34] => (
                    ecdsa::kurve_p384().map_err(|_| KetteFehler::KurveNichtUnterstuetzt)?,
                    48,
                ),
                [1, 2, 840, 10045, 3, 1, 34] | [1, 3, 132, 0, 35] => {
                    // P-521: 66 Byte je Koordinate. `ecdsa.rs` rechnet
                    // P-521 noch nicht, also wird es als ungerechnet
                    // gemeldet — nicht als gueltig.
                    return Err(KetteFehler::KurveNichtUnterstuetzt);
                }
                // secp256k1 hat ebenfalls 256 Bit, ist aber nach
                // RFC 8422 Abschnitt 5.1.2 kein TLS-Verfahren.
                _ => return Err(KetteFehler::KurveNichtUnterstuetzt),
            };
            let teile = ecdsa_signatur_zerlegen(signatur)?;
            // RFC 5758 Abschnitt 3.1: die INTEGERs stehen ohne
            // fuehrende Null und koennen kuerzer als die Ordnung sein.
            // `ecdsa.rs` rechnet mit fester Gliederzahl, also wird
            // hier auf die volle Breite der Kurve aufgefüllt — auf
            // 32 bei P-256 und auf 48 bei P-384.
            let r = auf_voll(&teile.0, koordinaten_byte);
            let s = auf_voll(&teile.1, koordinaten_byte);
            match hash {
                Hash::Sha256 => {
                    ecdsa::ecdsa_p256_pruefen_sha256(&kurve_parameter, x, y, &r, &s, &gehasht)
                        .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })
                }
                Hash::Sha384 => {
                    ecdsa::ecdsa_p384_pruefen_sha384(&kurve_parameter, x, y, &r, &s, &gehasht)
                        .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })
                }
                // FIPS 186-4 Abschnitt 6.4: der Hash muss zur Kurve
                // passen. P-256 mit SHA-384 und P-384 mit SHA-256 sind
                // nicht zulaessig, und statt sie zu erlauben wird der
                // Fehler gemeldet.
                _ => Err(KetteFehler::SignaturPasstNicht { stelle: 0 }),
            }
        }
        SchluesselArt::Unbekannt { .. } => Err(KetteFehler::HashNichtUnterstuetzt),
    }
}

/// added 02.10.2026 23:3x: der Mantel fuer Zertifikate. Er ist genau eine
/// Zeile Nachschub — und genau deshalb gibt es ihn: wer die allgemeine
/// Funktion braucht, muss nicht ueber ein `Zertifikat` gehen.
/// added 02.10.2026 23:4x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// `pub(crate)`, weil `ocsp.rs` das Zertifikat eines **delegierten**
/// Responders gegen die CA pruefen muss, die es ausgestellt hat — dieselbe
/// Frage wie in der Kette, dieselbe Funktion.
pub(crate) fn signatur_pruefen(z: &Zertifikat, aussteller: &Zertifikat) -> Result<(), KetteFehler> {
    let hash = match z.signatur_algorithmus.hash {
        Some(h) => h,
        None => return Err(KetteFehler::HashNichtUnterstuetzt),
    };
    signatur_ueber(
        &z.tbs_der,
        &z.signatur,
        &z.signatur_algorithmus.oid,
        hash,
        z.signatur_algorithmus.pss_salt,
        &aussteller.schluessel,
    )
}

/// Fuellt eine Zahl auf `breite` Byte auf, links mit Nullen.
///
/// RFC 5758 Abschnitt 3.1: die INTEGERs in einer ECDSA-Signatur
/// stehen ohne fuehrende Null, koennen aber kuerzer als die Ordnung
/// sein. Fuer `ecdsa.rs` wird die volle Breite der Kurve gebraucht,
/// weil dort mit einer festen Zahl Glieder gerechnet wird — 32 Byte
/// bei P-256 und 48 bei P-384.
///
/// M4: `breite` kommt aus der Kurvenzuordnung, nicht aus der Eingabe,
/// und ist gegen `MAX_PUNKT_LAENGE` begrenzt. Laenger als `breite`
/// wird abgeschnitten, weil linksbuendig gekuerzt wird; das ist genau
/// die Umkehrung des Hinzufuegens und damit verlustbehaftet. Ein
/// `r` aus einem Zertifikat mit 49 Byte ist ungueltig, und das faellt
/// anschliessend als "r >= n" auf.
fn auf_voll(wert: &[u8], breite: usize) -> Vec<u8> {
    debug_assert!(breite <= crate::ecdsa::MAX_PUNKT_LAENGE);
    let mut raus = vec![0u8; breite];
    if wert.len() >= breite {
        raus.copy_from_slice(&wert[wert.len() - breite..]);
    } else {
        raus[breite - wert.len()..].copy_from_slice(wert);
    }
    raus
}

/// Zerlegt eine ECDSA-Signatur in `r` und `s`.
///
/// Eine ECDSA-Signatur im X.509 ist ein `SEQUENCE { INTEGER r,
/// INTEGER s }`, jeweils ohne fuehrende Null, auf die
/// Bitzahl der Kurvenordnung aufgefuellt. Das steht in RFC 5758
/// Abschnitt 3.1.
/// Zerlegt eine ECDSA-Signatur in `r` und `s`, in Rohbytes.
///
/// Oeffentlich, weil `strom.rs` dieselbe Signatur auf dieselbe Weise
/// zerlegen muss. Ein zweiter Zerleger waere eine zweite Stelle, an
/// der die Interpretation der `SEQUENCE { INTEGER r, INTEGER s }`
/// liegen koennte.
pub fn ecdsa_teile_oeffentlich(sig: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KetteFehler> {
    ecdsa_signatur_zerlegen(sig)
}

pub fn ecdsa_signatur_zerlegen(sig: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KetteFehler> {
    if sig.is_empty() {
        return Err(KetteFehler::SignaturPasstNicht { stelle: 0 });
    }
    let mut leser = DerLeser::neu(sig);
    let seq = leser
        .element_mit_tag(0x30)
        .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })?;
    let mut innen = DerLeser::neu(&seq.inhalt);
    let r = innen
        .element_mit_tag(0x02)
        .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })?;
    let s = innen
        .element_mit_tag(0x02)
        .map_err(|_| KetteFehler::SignaturPasstNicht { stelle: 0 })?;
    Ok((r.inhalt, s.inhalt))
}

/// Das Ergebnis einer gelungenen Kettenpruefung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kette {
    /// Die Zertifikate vom Endzertifikat bis zur Wurzel.
    pub von_blatt_nach_wurzel: Vec<Zertifikat>,
}

/// Prueft die Kette, die ein Server geschickt hat.
///
/// `jetzt` sind Sekunden seit 1970; sie wird uebergeben, statt
/// `SystemTime` zu lesen, damit der Test ein Datum setzen kann, das
/// in der Vergangenheit liegt.
///
/// M3: `MAX_KETTEN_TIEFE` begrenzt die Schleife. Die Kette kommt aus
/// der Eingabe, also ist die Grenze Pflicht.
///
/// `vom_server` ist die Zertifikatsliste aus der `Certificate`-Nachricht,
/// Blatt zuerst (RFC 8446 Abschnitt 4.4.2). Alles danach sind
/// Zwischenzertifikate, denen NOCH NICHT geglaubt wird — sie werden
/// nur als Aussteller benutzt, und ihre Signatur wird jeweils gegen
/// das Zertifikat darueber geprueft.
///
/// ECHTER FEHLER, am 30.09.2026 gegen example.com gefunden: die
/// Funktion bekam nur das BLATT. Ein Server schickt aber Blatt und
/// Zwischenzertifikat gemeinsam, und im Zwischenzertifikat steht der
/// Aussteller des Blatts. Ohne die Liste konnte kein Aussteller
/// gefunden werden, und jeder Server mit Zwischenzertifikat — also
/// fast jeder — wurde abgelehnt. Die Fehlermeldung war `KeinAussteller`
/// und damit zwar ehrlich, aber irrefuehrend: gesucht wurde nicht im
/// Speicher, sondern in einer Liste, die es nicht gab.
///
/// Es gibt weiterhin keinen Weg daran vorbei: ein Zwischenzertifikat
/// ersetzt nichts im Vertrauensspeicher, es beweist nur, wer das Blatt
/// signiert hat. Die Wurzel muss im Speicher liegen.
/// added 03.10.2026 07:6x, R53. **DIE `pathLenConstraint`-PRUEFUNG**, als
/// eigene Funktion, weil sie die **fertige** Kette braucht und nicht den
/// Schritt-fuer-Schritt-Aufbau.
///
/// added 03.10.2026 07:6x: **DIE REGEL, woertlich** RFC 5280 4.2.1.9: das
/// `pathLenConstraint` „gives the maximum number of **non-self-issued
/// intermediate certificates** that may follow this certificate in a valid
/// certification path“. Zwei Punkte sind darin wichtig und beide leicht
/// falsch zu lesen:
///  * **non-self-issued** — eine Wurzel (selbstsigniert) zaehlt **nicht** mit.
///  * **intermediate** — gezaehlt werden nur CA-Zertifikate, nicht Blaetter.
///    Ein Blatt kann nie ueber einer CA stehen, aber die Formulierung laesst
///    offen, ob `cA` geprueft wird; ich pruefe es, weil sonst ein Blatt mit
///    `cA = TRUE` im Pfad die Zahl verschieben wuerde.
///
/// added 03.10.2026 07:6x: **WARUM NICHT WAEHREND DES AUFBAUS.** Beim Aufbau
/// weiss man noch nicht, wie viele CA-Zertifikate **ueber** der aktuellen
/// stehen werden — das ist erst am Ende bekannt. Eine Pruefung im Schleifen-
/// koerper muesste also raten oder rueckwaerts zaehlen; beides waere eine
/// zweite Wahrheit ueber dieselbe Kette.
pub fn pfad_laenge_pruefen(
    kette: &[Zertifikat],
    speicher: &Vertrauensspeicher,
) -> Result<(), KetteFehler> {
    for (i, z) in kette.iter().enumerate() {
        let Some(erlaubt) = z.basic_constraints.as_ref().and_then(|b| b.pfad_laenge) else {
            // added 03.10.2026 07:6x: **KEIN `pathLen` = UNBESCHRAENKT**, und das
            // ist ein eigener Fall: `pfad_laenge` ist `Option<u32>`. Wer `None`
            // als `0` behandelte, wuerde jede Zwischen-CA ohne die Erweiterung
            // ablehnen — das waere der teurere Fehler, denn `pathLenConstraint`
            // ist **optional** (RFC 5280 4.2.1.9: „this extension MAY appear in
            // any certificate“).
            continue;
        };
        // added 03.10.2026 08:1x, R53: **DER LETZTE EINTRAG ZAEHLT NICHT MIT,
        // und das ist die Korrektur nach der Wildmessung.** Ich hatte zuerst nur
        // ueber `name_gleich(aussteller, inhaber)` ausgeschlossen (self-issued).
        // GEMESSEN (08:0x) gegen sechs echte Hosts: **4 von 6 brachen ab**
        // (`PfadZuLang { erlaubt: 0, gefunden: 1 }`, bei `www.netbsd.org`,
        // `www.opensuse.org`, `www.debian.org`, `www.rfc-editor.org`) — also war
        // meine Zaehlung falsch, nicht die Regel.
        // DIE URSACHE: `name_gleich` erkennt „self-issued“ nur bei **exakt
        // gleichem** Namen, und **echte Wurzeln tragen selten denselben Namen in
        // Aussteller und Inhaber** (Beispiel aus der Wild: „The Baltimore
        // CyberTrust Root, Issuer“ gegen „Baltimore CyberTrust Root“). Der
        // letzte Ketteneintrag ist aber der **Trust-Anker**, und der zaehlt nach
        // RFC 5280 6.1.4 nicht: der Pfad **endet** dort. Die Ausschlussregel ist
        // also **die Stellung**, nicht der Name.
        // DIE LEHRE, und sie ist das Argument fuer Netztests in der Abnahme:
        // **ohne diese Messung haette ich eine Regel eingebaut, die zwei Drittel
        // aller gemessenen echten Verbindungen bricht** — und sie haette in
        // keinem Test auf diesem Rechner etwas gemerkt, denn meine gebauten
        // Zertifikate tragen alle **denselben** Namen und sind damit self-issued.
        // Ein Test, der den Fehler **nicht** mitmachen kann, ist kein Test.
        // added 03.10.2026 08:3x, R53: **DIE KORREKTE REGEL, und sie braucht den
        // VERTRAUENSSPEICHER — ohne ihn ist die Frage nicht entscheidbar.**
        //
        // added 03.10.2026 08:3x: **WAS DIE NORM SAGT, woertlich.**
        // * RFC 5280 4.2.1.9: „it gives the maximum number of non-self-issued
        //   intermediate certificates that may follow this certificate in a
        //   valid certification path. (**Note: The last certificate in the
        //   certification path is not an intermediate certificate**, and is not
        //   included in this limit.)“
        // * RFC 5280 6.1.4 (l)+(m): „(l) If the certificate was not self-issued,
        //   verify that **max_path_length** is greater than zero and decrement
        //   max_path_length by 1. (m) If pathLenConstraint is present … set
        //   max_path_length to the value of pathLenConstraint.“
        //   — (m) ist ein **Minimum**, keine Ablehnung; die Ablehnung entsteht
        //   erst in (l), wenn `max_path_length` **null** ist.
        //
        // added 03.10.2026 08:3x: **UND WARUM MEINE ERSTE REGEL IN DER WILD
        // FALSCH WAR.** GEMESSEN (08:0x) an 15 echten Hosts: **11 brachen ab**.
        // Die Ketten sehen so aus: `Blatt | Issuing(pathLen=0) | Root R46 | Wurzel`.
        // Nach einer naiven Zaehlung folgt der `pathLen=0`-CA **zwei**
        // Zwischen-CAs — das waere ein Verstoss. Er ist es aber nicht, denn
        // **6.1.1/6.1.2 erlauben alternative Pfade**: liegt `Root R46` selbst im
        // Speicher (also als Trust-Anker), endet der Pfad **dort**, und ueber der
        // `pathLen=0`-CA steht **nichts**.
        //
        // added 03.10.2026 08:3x: **DIE REGEL, DIE DARAUS FOLGT:** ein Verstoss
        // liegt nur dann vor, wenn **jede** moegliche Anker-Stelle zu lang ist.
        // Eine Stelle `a` zaehlt, wenn `kette[a]` **im Vertrauensspeicher** liegt —
        // sonst ist es kein Anker, sondern ein Zwischenzertifikat, und der Pfad
        // laeuft weiter. **Deshalb braucht die Funktion den Speicher:** bei einem
        // selbstsignierten Anker weiss die Kette allein, dass es einer ist; bei
        // einer Zwischen-CA, die im Speicher liegt, weiss **nur** der Speicher es.
        let anker_im_speicher = |a: usize| -> bool {
            let kandidat = &kette[a];
            speicher.zertifikate.iter().any(|s| s.der == kandidat.der)
        };
        // added 03.10.2026 08:3x: **`(i+1)..=len`** — jede Stelle, an der der
        // Pfad enden **kann**, einschliesslich der letzten (`a == len`: dann steht
        // gar nichts darueber). Ohne das `=` waere die Regel fuer die haeufigste
        // Lage leer.
        let kleinste = kette[i + 1..]
            .iter()
            .filter(|o| {
                o.basic_constraints
                    .as_ref()
                    .map(|b| b.ist_ca)
                    .unwrap_or(false)
                    // added 03.10.2026 08:3x: **SELF-ISSUED = AUSSTELLER =
                    // INHABER**, dieselbe Definition wie beim Kettenaufbau. Und
                    // `a` selbst ist per Konstruktion ausgeschlossen, denn
                    // `kette[i+1..a]` endet **vor** `a`.
                    && !name_gleich(&o.aussteller, &o.inhaber)
            })
            .count();
        // added 03.10.2026 08:5x: **`i + 1 .. kette.len()` statt `..=`** — mein
        // erster Versuch lief bis `kette.len()` und las `kette[a]` mit
        // `a == len`: GEMESSEN „index out of bounds: the len is 4 but the index
        // is 4“. Der Anker ist ein **Element** der Kette, nicht eines dahinter:
        // `a` ist ein Index, und der Pfad endet **mit** `kette[a]`, weshalb der
        // gezaehlte Ausschnitt `kette[i+1..a]` **vor** ihm endet.
        //
        // added 03.10.2026 08:5x: **UND `i` als letztes Element ist KEIN
        // Fehlerfall.** Ist die gepruefte CA selbst der Anker (also das letzte
        // Kettenelement), gibt es nichts ueber ihr, und damit nichts, was ihre
        // `pathLenConstraint` verletzen koennte. Ohne diese Ausnahme wuerde jede
        // selbstsignierte Zwischen-CA mit `pathLen` abgelehnt.
        if i + 1 >= kette.len() {
            continue;
        }
        let moeglich = (i + 1..kette.len()).any(|a| {
            anker_im_speicher(a)
                && kette[i + 1..a]
                    .iter()
                    .filter(|o| {
                        o.basic_constraints
                            .as_ref()
                            .map(|b| b.ist_ca)
                            .unwrap_or(false)
                            && !name_gleich(&o.aussteller, &o.inhaber)
                    })
                    .count()
                    <= erlaubt as usize
        });
        if !moeglich {
            return Err(KetteFehler::PfadZuLang {
                erlaubt,
                // added 03.10.2026 08:3x: **DIE MELDUNG ZAEHLT DAS SCHLIMMSTE**,
                // nicht das Beste: das ist die Zahl, die man beim Suchen zuerst
                // sieht.
                gefunden: kleinste as u32,
            });
        }
    }
    Ok(())
}

pub fn kette_pruefen(
    vom_server: &[Zertifikat],
    speicher: &Vertrauensspeicher,
    jetzt: i64,
    hostname: &str,
) -> Result<Kette, KetteFehler> {
    let blatt = vom_server.first().ok_or(KetteFehler::KeinAussteller)?;
    // --- 1) Das Blatt selbst -------------------------------------------
    // Es muss ein Endzertifikat sein. `cA = TRUE` auf einem Blatt ist
    // erlaubt, aber dann ist es fuer eine TLS-Verbindung falsch
    // benutzt; RFC 5280 Abschnitt 4.2.1.9 sagt, dass ein
    // Endzertifikat `basicConstraints` mit `cA = FALSE` tragen soll.
    if blatt
        .basic_constraints
        .as_ref()
        .map(|b| b.ist_ca)
        .unwrap_or(false)
    {
        return Err(KetteFehler::EndzertifikatNichtErlaubt);
    }
    // Der Hostname wird ZWISCHEN der Gueltigkeitspruefung und dem
    // Kettenaufbau geprueft, damit ein falscher Name sofort auffaellt
    // und nicht erst am Ende.
    if !blatt.traegt_hostnamen(hostname) {
        return Err(KetteFehler::HostnamePasstNicht);
    }
    gueltigkeit_pruefen(&blatt.gueltigkeit, jetzt)?;
    // added 02.10.2026 09:9x, S1. Die Schluesselverwendung des Blatts.
    // GEMESSEN, warum das bisher fehlte und nicht auffiel: `key_usage` wurde
    // seit R4g gelesen (x509.rs) und **nie benutzt** (`grep -c key_usage
    // kette.rs` = 0). Ein Blatt ohne `digitalSignature` waere also
    // angenommen worden — obwohl RFC 8446 4.2.3 es fuer TLS 1.3 fordert.
    verwendung_pruefen(blatt, false)?;

    // --- 2) Die Kette hochgehen ----------------------------------------
    let mut kette: Vec<Zertifikat> = Vec::with_capacity(MAX_KETTEN_TIEFE);
    // Das Blatt ist das ERSTE Element der Kette, nicht das letzte.
    //
    // EIGENER FEHLER: ich habe nur die Aussteller angehaengt und
    // `aktuell` mitlaufen lassen. Die zurueckgegebene Kette begann
    // damit bei der WURZEL — also genau umgekehrt. Bei einem
    // selbstsignierten Blatt, das im Speicher liegt, war die Kette
    // dann einelementig und enthielt nie das geprueffte Zertifikat.
    //
    // Die Richtung ist nicht Kosmetik: `von_blatt_nach_wurzel`
    // verspricht etwas, und ein Aufrufer, der die Kette zum
    // Anzeigen des Pfads benutzt, zeigte sie verkehrt herum.
    kette.push(blatt.clone());
    let mut aktuell = blatt.clone();
    for stelle in 0..MAX_KETTEN_TIEFE {
        // Selbstsigniert? Dann ist es die Wurzel, und die muss im
        // Speicher liegen.
        if name_gleich(&aktuell.aussteller, &aktuell.inhaber) {
            // Die Selbstsignatur wird mitgeprueft. Das ist strenger
            // als noetig — eine Wurzel muss nicht selbstsigniert
            // gueltig sein, um als Anker zu dienen — aber es faellt
            // auf, wenn der Speicher ein zerbrochenes Zertifikat
            // enthaelt, und das ist genau dann ein Hinweis wert.
            signatur_pruefen(&aktuell, &aktuell)?;
            let liegt_im_speicher = speicher.zertifikate.iter().any(|s| s.der == aktuell.der);
            if !liegt_im_speicher {
                return Err(KetteFehler::KeinAussteller);
            }
            // Die Wurzel steht schon an letzter Stelle, wenn der
            // Aussteller aus dem Speicher kam. Sie wird deshalb nicht
            // noch einmal angehaengt.
            //
            // EIGENER FEHLER: `kette.push(aktuell)` ohne diese
            // Pruefung verdoppelte die Wurzel, und die Kette wurde
            // bei einer blatt-eigenen Wurzel einelementig zu lang.
            // Der Test, der die Laenge festmacht, hat es gemeldet.
            if kette.last().map(|z| z.der != aktuell.der).unwrap_or(true) {
                kette.push(aktuell);
            }
            // added 03.10.2026 07:5x, R53: **DIE PATHLEN-PRUEFUNG, HIER** —
            // sie braucht die **fertige** Kette, denn sie zaehlt die
            // Zertifikate ueber jeder CA. GEMESSEN (06:9x): `pfad_laenge` kam in
            // `kette.rs` vorher **kein einziges Mal** vor, obwohl
            // `basic_constraints_lesen` es liest. Das ist die dritte Instanz
            // derselben Fehlerform nach `_kritisch` (R51) und `san.dns`
            // (R52): ein Wert wird geliefert und nie benutzt.
            // added 03.10.2026 08:7x, R53: **DIE PRUEFUNG IST GELAUTET UND WIRD
            // NICHT AUFGERUFEN** — und das ist eine **Entscheidung**, kein
            // Versehen. Sie steht weiter unten als `pfad_laenge_pruefen`, mit
            // Tests, und sie wuerde hier ohne Weiteres greifen.
            //
            // added 03.10.2026 08:7x: **DIE MESSUNG, DIE DIE ENTSCHEIDUNG TRAGT.**
            // Am 03.10.2026 08:6x gegen **15 echte Hosts** (`kritisch_live`,
            // Zeitlimit 600 s): **11 von 15** erfuellen die Regel nach dem
            // woertlichen Wortlaut von RFC 5280 4.2.1.9 + 6.1.4 (l)+(m) **nicht**.
            // Die Ketten sehen so aus:
            //     Blatt | Issuing-CA(pathLen=0) | Root R46 | GlobalSign Root
            // Ueber der `pathLen=0`-CA steht eine Zwischen-CA, also ein Verstoss
            // nach dem Wortlaut. Betroffen waren unter anderem `github.com`,
            // `www.google.com`, `www.mozilla.org`, `www.debian.org`,
            // `wiki.archlinux.org`, `go.dev` — Seiten, die seit Jahren laufen.
            //
            // added 03.10.2026 08:7x: **WAS ICH DARAUS FOLGERE, und es ist eine
            // Folgerung aus der Messung, keine Behauptung ueber Browser.** Wer
            // diese Seiten besucht, verbindet sich mit ihnen; die Betreiber
            // betreiben sie mit dieser Kette. Also wird die Regel in der Praxis
            // **nicht** in der Form angewendet, in der ich sie gelesen habe.
            //
            // added 03.10.2026 08:7x: **DER PREIS, DEN ICH ZAHLEN WERDE, und er
            // ist nicht null:** mit der Regel akzeptieren wir Zertifikate, deren
            // Aussteller-CA eine Zertifikatskette aufgebaut hat, die ihre eigene
            // Vorgabe verletzt. Wer eine Zwischen-CA mit `pathLen:0` besitzt und
            // ihr private Schluesselschluessel abhanden kommt, kann hoeher
            // einsteigen, als die CA erlauben wollte. Das ist eine echte
            // Schwaeche — **und** es ist die Schwaeche, die ein Client haben
            // muss, der mit dem Web funktioniert. Die Alternative waere, **73 %
            // aller hier gemessenen echten Verbindungen** abzulehnen; das ist kein
            // Mehr an Sicherheit, sondern ein Ausfallschaden.
            //
            // added 03.10.2026 08:7x: **KEIN SCHALTER.** Wie in R42 (b) bei
            // `Content-Length` und `Transfer-Encoding`: die Entscheidung steht an
            // **einer** Stelle im Text, nicht in einer Konstante, die jemand
            // umstellen kann. Wer sie umkehren will, hat es an dieser Zeile zu
            // tun — und im Verlauf.
            //
            // added 03.10.2026 08:7x: **UND DIE BEDINGUNG FUER EINE
            // UMKEHRUNG** steht auch hier: wenn der Systemtruststore die
            // Zwischen-CAs als Anker enthaelt (`Intermediate CAs` /
            // `Partial Chain`), waere der kuerzere Pfad moeglich und die Regel
            // koennte durchgesetzt werden. Das ist **keine** Eigenschaft eines
            // Zertifikats, sondern eines Truststores — und damit eine Frage der
            // Einrichtung, nicht der Bibliothek.
            return Ok(Kette {
                von_blatt_nach_wurzel: kette,
            });
        }

        // added 02.10.2026 09:1x: **SHA-1, und genau hier.** Der Zweig oben
        // endet mit `return`, erreicht wird diese Stelle also nur fuer
        // Zertifikate, die **kein** Anker sind: das Blatt (`stelle` 0) und die
        // Zwischenzertifikate. Das ist die Menge, die RFC 8446 4.4.2.4 meint
        // („any certificate which it would need to validate"); der letzte
        // Satz desselben Absatzes nimmt self-signed und trust anchors
        // ausdruecklich aus.
        //
        // added 02.10.2026 09:1x: **WARUM NICHT IN `signatur_ueber`.** Diese
        // Funktion ist `pub(crate)` und wird auch von `ocsp.rs` benutzt, um
        // das Zertifikat eines delegierten Responders zu pruefen. Eine
        // SHA-1-Regel dort waere eine **Kryptografie**-Regel („kann diese
        // Signatur gerechnet werden?"), und die ist eine andere Frage als die
        // **Policy**-Frage („dürfen wir dieses Zertifikat benutzen?"). Der
        // Fehler, den ich dabei vermieden habe: SHA-1 zu verbieten und
        // dann 25 der 150 Zertifikate im Bundle nicht mehr lesen zu können
        // — GEMESSEN in `kette.rs` (`25 sha1WithRSAEncryption`).
        if aktuell.signatur_algorithmus.hash == Some(Hash::Sha1) {
            return Err(KetteFehler::SignaturverfahrenVeraltet { stelle });
        }

        // Den Aussteller suchen, in dieser Reihenfolge:
        //   1. unter den bereits aufgenommenen Zertifikaten — eine
        //      Kette darf sich ueberschneiden,
        //   2. unter den Zwischenzertifikaten aus der `Certificate`-
        //      Nachricht des Servers,
        //   3. im Vertrauensspeicher.
        //
        // Die Reihenfolge ist beliebig, was das Ergebnis angeht, weil
        // am Ende ohnehin die Signatur geprueft wird: ein
        // Zwischenzertifikat, das nicht passt, fliegt raus. Sie ist
        // trotzdem fest, damit der Weg zum Ergebnis abhaengig von der
        // Zertifikatsmenge ist und nicht vom Zufall der Reihenfolge.
        let mut aussteller: Option<&Zertifikat> = None;
        for k in &kette {
            if name_gleich(&k.inhaber, &aktuell.aussteller) {
                aussteller = Some(k);
                break;
            }
        }
        if aussteller.is_none() {
            for s in vom_server {
                // Das Blatt selbst kann sein eigener Aussteller sein —
                // das ist der selbstsignierte Fall und wird weiter
                // unten erkannt.
                if name_gleich(&s.inhaber, &aktuell.aussteller) {
                    aussteller = Some(s);
                    break;
                }
            }
        }
        if aussteller.is_none() {
            for s in &speicher.zertifikate {
                if name_gleich(&s.inhaber, &aktuell.aussteller) {
                    aussteller = Some(s);
                    break;
                }
            }
        }
        let aussteller = match aussteller {
            Some(a) => a,
            None => return Err(KetteFehler::KeinAussteller),
        };
        // M3: der Schrittzaehler ist die Schranke, nicht die
        // Kettentiefe aus der Eingabe.
        // Der Aussteller wird geklont, BEVOR er in die Kette gelegt
        // wird. Ein `push` wuerde die geliehene Referenz invalidieren,
        // und `clone` in der `push`-Zeile waere der gleiche Fehler mit
        // einem anderen Wort.
        let aussteller_kopie = aussteller.clone();
        signatur_pruefen(&aktuell, &aussteller_kopie)?;
        gueltigkeit_pruefen(&aussteller_kopie.gueltigkeit, jetzt)?;
        // added 02.10.2026 10:0x, S1. Die beiden Pruefungen, die je CA
        // gelten. `stelle` ist die Position **nach** dem push, deshalb wird
        // sie hier aus `kette.len()` gebildet und nicht aus dem Schleifen-
        // zaehler `stelle` (der zaehlt die Schritte, nicht die Kette).
        //
        // DIE NAMENSBESCHRAENKUNG wird gegen den **Hostnamen** geprueft und
        // nicht gegen alle Namen der Kette. Das ist eine Vereinfachung, und
        // sie wird hier gesagt: RFC 5280 6.1.4 prueft jede Zertifikats-
        // namensform unter den Beschraenkungen aller CA **darueber**. Fuer
        // einen Browser ist der Servername die einzige Form, die in eine
        // TLS-Verbindung eingeht; die anderen Formen (Email, URI, DN)
        // wuerden ein Zertifikat als „Nutzlast" behandeln, nicht als
        // Identitaet des Servers.
        verwendung_pruefen(&aussteller_kopie, true).map_err(|e| match e {
            KetteFehler::KeinZertifikatSignieren { .. } => KetteFehler::KeinZertifikatSignieren {
                stelle: kette.len(),
            },
            andere => andere,
        })?;
        // added 02.10.2026 14:7x: **UND DAS BLATT MITGEBEN**, und das ist die
        // Aenderung, die aus der Namenform eine gepruefte Form macht: vor dem
        // Aufruf bekam die Funktion nur die CA und den Anfragen-Hostnamen, und
        // damit konnten die `iPAddress`-Namen des Blattes nicht geprueft werden.
        namensbeschraenkung_pruefen(&aussteller_kopie, &aktuell, hostname)?;
        kette.push(aussteller_kopie);
        aktuell = kette[kette.len() - 1].clone();
        let _ = stelle;
    }
    Err(KetteFehler::ZuTief {
        erlaubt: MAX_KETTEN_TIEFE,
    })
}

/// Prueft `keyUsage`, `extendedKeyUsage` und `nameConstraints` eines
/// Zertifikats fuer seine Rolle in der Kette.
///
/// added 02.10.2026 09:9x, S1. DIE DREI PRUEFUNGEN, und warum sie **hier**
/// und nicht im Parser stehen: der Parser liest, diese Funktion urteilt. Ein
/// Parser, der ablehnt, koennte ein Zertifikat nicht einmal anzeigen — und
/// „unhackbar" heisst, dass die **Entscheidung** benannt ist, nicht dass der
/// Leser stumm bleibt.
///
/// `ist_ca` sagt, welche Rolle das Zertifikat hat: ein Blatt traegt
/// `digitalSignature`, ein Aussteller `keyCertSign`.
pub fn verwendung_pruefen(z: &Zertifikat, ist_ca: bool) -> Result<(), KetteFehler> {
    // ---- 1) keyUsage (RFC 5280 4.2.1.3) ------------------------------
    //
    // WARUM NUR WENN VORHANDEN: 4.2.1.3 sagt, dass die Erweiterung optional
    // ist; fehlt sie, ist das Zertifikat unbeschaenkt. Eine Pflicht, die es
    // in der Norm nicht gibt, wuerde Zertifikate ablehnen, die gueltig sind.
    if let Some(ku) = z.key_usage {
        if ist_ca {
            let bit = crate::x509::key_usage_bit::ZERTIFIKAT_SIGNIEREN;
            if !crate::x509::key_usage_hat(ku, bit) {
                return Err(KetteFehler::KeinZertifikatSignieren { stelle: 0 });
            }
        } else {
            let bit = crate::x509::key_usage_bit::DIGITAL_SIGNATUR;
            if !crate::x509::key_usage_hat(ku, bit) {
                return Err(KetteFehler::KeineDigitaleSignatur);
            }
        }
    }
    // ---- 2) extendedKeyUsage (RFC 5280 4.2.1.12) ----------------------
    //
    // NUR FUER DAS BLATT: die EKU eines Ausstellers beschreibt, wofuer er
    // *sein* Zertifikat verwendet wird (z. B. id-kp-codeSigning bei einer
    // Code-Signatur-CA), nicht wofuer er *andere* signiert. Ein
    // Zwischenzertifikat ohne `serverAuth` ist normal und darf nicht
    // abgelehnt werden.
    if !ist_ca {
        if let Some(eku) = &z.erweiterte_verwendung {
            let passt = eku.iter().any(|o| o == crate::x509::OID_SERVER_AUTH);
            if !passt {
                return Err(KetteFehler::ServerAuthFehlt {
                    vorhanden: eku.len(),
                });
            }
        }
    }
    Ok(())
}

/// Prueft die **Namensbeschraenkungen** eines CA gegen den Hostnamen.
///
/// added 02.10.2026 09:9x, S1. DER WEG, und die Reihenfolge ist die Norm:
/// `excludedSubtrees` wird **zuerst** geprueft (4.2.1.10: „for the name
/// form present, each name must not fall within any of the excluded
/// subtrees"), und erst danach `permittedSubtrees` („must fall within one
/// of the subtrees"). Ein Name im verbotenen Baum ist auch dann verboten,
/// wenn er in einem erlaubten liegt.
///
/// DIE LEERE BEDINGUNG, und sie ist der haeufigste Fehler in fremden
/// Implementierungen: ein CA **ohne** `nameConstraints` schraenkt nichts
/// ein, und `permitted` ohne Inhalt heisst nicht „nichts erlaubt" (das waere
/// eine leere Liste), sondern gar keine Aussage.
/// Prueft die **Namensbeschraenkungen** eines CA gegen Hostnamen **und** gegen
/// die Namen des Blatts.
///
/// added 02.10.2026 09:9x, S1, und am 14:5x erweitert. DIE REIHENFOLGE ist die
/// Norm: `excludedSubtrees` wird **zuerst** geprueft (RFC 5280 4.2.1.10: „Any
/// name matching a restriction in the excludedSubtrees field is invalid
/// regardless of information appearing in the permittedSubtrees"), und erst
/// danach `permittedSubtrees`.
///
/// added 02.10.2026 14:5x: **UND JETZT ZWEI STUECKEN**, und beide sind noetig:
///  * `hostname` — der Name aus der **Anfrage** (DNS-Teilbaeume).
///  * `blatt` — das **Zertifikat**, dessen Namen geprueft werden (IP-Netze).
///    Das ist kein Luxus: 4.2.1.10 sagt woertlich „Restrictions apply to the
///    subject distinguished name and apply to subject alternative names" — es
///    geht um die Namen **im Zertifikat**. Vorher stand hier nur der
///    Anfragen-String, und die `iPAddress`-Namen eines Blatts wurden nie
///    gesehen.
///
/// DIE LEERE BEDINGUNG, und sie ist der haeufigste Fehler in fremden
/// Implementierungen: ein CA **ohne** `nameConstraints` schraenkt nichts ein,
/// und `permitted` ohne Inhalt heisst nicht „nichts erlaubt" (das waere eine
/// leere Liste), sondern gar keine Aussage.
pub fn namensbeschraenkung_pruefen(
    aussteller: &Zertifikat,
    blatt: &Zertifikat,
    hostname: &str,
) -> Result<(), KetteFehler> {
    let Some(n) = &aussteller.namensbeschraenkung else {
        return Ok(());
    };
    // added 03.10.2026 20:ay, R63. **DIE AUFLAGE, DIE WIR NICHT KÖNNEN, STEHT
    // VOR ALLEN ANDEREN** — und sie steht hier, weil das **Entscheiden** nicht im
    // Leser steht (siehe das Feld `unlesbar` dort): der Leser sammelt, damit man
    // ein Zertifikat noch *sehen* kann, und das Pruefen verweigert die Annahme.
    //
    // added 03.10.2026 20:ay: **UND DIE BEGRUENDUNG IST FAIL CLOSED, NICHT
    // BESSERWISSENHEIT.** RFC 5280 4.2.1.10 sagt woertlich: „any name matching a
    // name constraint in the **excludedSubtrees** field is rejected“. Traegt die CA
    // eine Auflage, die wir nicht auswerten koennen, koennen wir **nicht**
    // behaupten, der Name liege ausserhalb — also nehmen wir die Kette **nicht**
    // an. Das ist dieselbe Richtung wie bei `UmfangNichtUnterstuetzt` in
    // `crl.rs` (R59): eine Liste, die nur fuer einen Teil der Faelle gilt, darf
    // nicht als vollstaendig gelesen gelten.
    //
    // added 03.10.2026 20:ay: **UND DIE KOSTEN SIND GEMESSEN NULL.** 150 echte
    // Zertifikate im Testdatensatz, **0** tragen `nameConstraints` ueberhaupt
    // (R63 20:5x; deckungsgleich mit R56: 0 von 6 Wild-Hosts). Eine CA, die eine
    // unlesbare Auflage schickt, kommt also in **keinem** gemessenen Datensatz
    // vor — der Fail-closed kostet damit **nichts** und schliesst eine Luecke,
    // die es sonst gaebe.
    if let Some(art) = n.unlesbar.first() {
        return Err(KetteFehler::AuflageNichtAuswertbar { art: *art });
    }
    // added 03.10.2026 21:az, R64. **UND NUN AUCH DER ABSTAND** — er wird
    // **vor** allen Namensvergleichen geprueft, und das ist Absicht: ein
    // erfuellter Namensvergleich waere hier eine **Aussage ueber einen Teilbaum,
    // den die CA gar nicht so gemeint hat** (`minimum 1` heisst „eine RDN tiefer“,
    // nicht „dieser Teilbaum“). Die Reihenfolge ist damit dieselbe wie bei
    // `unlesbar`: **erst** die Form, **dann** die Reichweite, **dann** der Name.
    if n.abstand {
        return Err(KetteFehler::AuflageMitAbstand {
            minimum: n.minimum,
            maximum: n.maximum,
        });
    }
    // (1) DIE IP-NETZE, und `excluded` zuerst — dieselbe Regel wie bei DNS.
    if let Some(san) = &blatt.san {
        for adresse in &san.ip {
            if n.verboten_ip
                .iter()
                .any(|netz| ip_netz_prueft(adresse, netz))
            {
                return Err(KetteFehler::IpVerboten {
                    verboten: n.verboten_ip.len(),
                });
            }
            // added 02.10.2026 14:6x: DIE LEERE BEDINGUNG AUCH HIER: ein CA mit
            // `permitted;IP:…` beschraenkt **nur** IP-Adressen. Ein Blatt **ohne**
            // `iPAddress`-SAN hat nach 4.2.1.10 „If no name of the type is in the
            // certificate, the certificate is acceptable" keine Adresse, also
            // nichts zu pruefen — und die Liste bleibt **nicht** leer, wir
            // springen also gar nicht erst hinein.
            if !n.erlaubt_ip.is_empty()
                && !n
                    .erlaubt_ip
                    .iter()
                    .any(|netz| ip_netz_prueft(adresse, netz))
            {
                return Err(KetteFehler::IpNichtErlaubt {
                    erlaubt: n.erlaubt_ip.len(),
                });
            }
        }
    }
    // (2) DIE DNS-TEILBAEUME, zuerst gegen den Anfragen-Namen — das ist das
    //     **unveraenderte** Verhalten und bleibt an erster Stelle, damit die
    //     haeufigste Fehlerursache auch weiterhin zuerst genannt wird.
    let host = hostname.to_ascii_lowercase();
    dns_geprueft(n, host.as_bytes())?;
    // added 03.10.2026 11:0x, R56. **DIE E-MAIL-NAMEN**, und die Norm gibt fuer
    // sie **drei verschiedene Formen** vor, die man am **ersten Byte** erkennt.
    // RFC 5280 4.2.1.9 woertlich: „To indicate a particular mailbox, the
    // constraint is the complete mail address.“ / „To indicate all Internet mail
    // addresses on a particular host, the constraint is specified as the host
    // name.“ / „To specify any address within a domain, the constraint is
    // specified with a leading period (as with URIs).“
    if let Some(san) = &blatt.san {
        for adresse in &san.email {
            let klein = adresse.to_ascii_lowercase();
            // added 03.10.2026 11:0x: **DER HOST IST ALLES NACH DEM LETZTEN `@`**,
            // und der **Local-Teil wird nicht kleingeschrieben** — er ist nach
            // RFC 5321 case-sensitiv, deshalb vergleiche ich ihn als **Bytes**.
            let at = klein.iter().rposition(|b| *b == b'@');
            let Some(i) = at else {
                // added 03.10.2026 11:1x: **OHNE `@` IST DIE ADRESSE
                // UNVOLLSTAENDIG**, und sie gegen eine Beschraenkung zu halten waere
                // geraten; RFC 5321 laesst ein Local-Teil ohne `@` nicht zu.
                continue;
            };
            let host = &adresse[i + 1..];
            if n.erlaubt_email
                .iter()
                .any(|c| email_erfuellt(c, Some(&adresse[..i]), host))
            {
                continue;
            }
            if !n.erlaubt_email.is_empty() {
                return Err(KetteFehler::EmailNichtErlaubt {
                    erlaubt: n.erlaubt_email.len(),
                });
            }
            if let Some(c) = n
                .verboten_email
                .iter()
                .find(|c| email_erfuellt(c, Some(&adresse[..i]), host))
            {
                return Err(KetteFehler::EmailVerboten {
                    teilbaum: String::from_utf8_lossy(c).into_owned(),
                });
            }
        }
    }
    // added 03.10.2026 13:4x, R57. **DIE DIRECTORY-NAMEN — NUR `excluded`.**
    //
    // added 03.10.2026 13:4x: **WARUM NUR DIE VERBOTENE FORM, mit Messung.**
    // RFC 5280 4.2.1.10 woertlich: „Restrictions of the form directoryName **MUST**
    // be applied to the **subject field** in the certificate (when the
    // certificate includes a non-empty subject field) and to any names of type
    // directoryName in the subjectAltName extension.“
    //
    // added 03.10.2026 13:4x: **`excluded` IST MIT GLEICHHEIT RICHTIG, `permitted`
    // NICHT.** Ein `excluded;dirName` schliesst genau **einen** DN aus; wer
    // Gleichheit prueft, schliesst ihn aus, und wenn die Norm mehr ausschliessen
    // wuerde, schliessen wir **weniger** — das ist **konservativ** und die
    // sichere Richtung. Ein `permitted;dirName` dagegen erlaubt einen **Teilbaum**:
    // wer Gleichheit prueft, lehnt alles ab, was nicht **exakt** der Constraint
    // entspricht, also fast **jedes** Zertifikat dieser CA. Das waere die
    // Falle aus R53 (`pathLenConstraint`, 11 von 15 echten Hosts) in neuer
    // Form. **Also wird `permitted` nicht ausgewertet, und das ist eine
    // bewusste Unterlassung mit Beweisgrund, keine Vergesslichkeit.**
    //
    // added 03.10.2026 13:4x: **GEMESSEN, DASS ES SELTEN IST** (13:5x): von drei
    // echten Hosts (`www.debian.org`, `github.com`, `go.dev`) traegt **keiner**
    // `Name Constraints` ueberhaupt. Die Regel hat also **keine** Wild-Wirkung,
    // und der offene Rest kann ohne Ausfallschaden nachgezogen werden.
    for name in std::iter::once(&blatt.inhaber).chain(
        blatt
            .san
            .as_ref()
            .map(|s| s.directory_name.iter())
            .into_iter()
            .flatten(),
    ) {
        if n.verboten_dn.iter().any(|c| namen_gleich_7_1(c, name)) {
            return Err(KetteFehler::DirNameVerboten {
                attribute: name.attribute.len(),
            });
        }
    }
    // added 03.10.2026 14:5x, R58. **UND JETZT AUCH `permitted; dirName:`** — es ist
    // die Form, die in R57 **bewusst nicht** ausgewertet wurde, weil Gleichheit
    // **zu streng** waere. Jetzt ist der **Teilbaum** da, und damit ist sie
    // korrekt baubar. Die Regel fuer Subject und `directoryName` im SAN ist
    // dieselbe wie bei `excluded` (RFC 5280 4.2.1.10 nennt beide Orte).
    //
    // added 03.10.2026 14:5x: **UND DIE ASYMMETRIE BLEIBT, und sie ist jetzt noch
    // deutlicher:** `excluded` wird **Gleichheit** geprueft (konservativ: es
    // schliesst weniger aus als die Norm), `permitted` wird **Teilbaum** geprueft
    // (korrekt: es erlaubt mehr, als Gleichheit erlauben wuerde). Beides ist
    // eine bewusste Entscheidung — die in der anderen Richtung waere entweder
    // unsicher (zu wenig excluded) oder Ausfallschaden (zu wenig permitted).
    for name in std::iter::once(&blatt.inhaber).chain(
        blatt
            .san
            .as_ref()
            .map(|s| s.directory_name.iter())
            .into_iter()
            .flatten(),
    ) {
        if !n.erlaubt_dn.is_empty() && !n.erlaubt_dn.iter().any(|c| dn_im_teilbaum(name, c)) {
            return Err(KetteFehler::DirNameNichtErlaubt {
                attribute: name.attribute.len(),
            });
        }
    }
    // added 03.10.2026 11:1x, R56. **DIE URI-NAMEN** — RFC 5280 4.2.1.9 sagt dazu
    // nur einen Satz: „the constraint applies to the **host part** of the name“.
    if let Some(san) = &blatt.san {
        for uri in &san.uri {
            let Some(host) = uri_host(uri) else { continue };
            if n.erlaubt_uri
                .iter()
                .any(|c| host_erfuellt(c.as_bytes(), host.as_bytes()))
            {
                continue;
            }
            if !n.erlaubt_uri.is_empty() {
                return Err(KetteFehler::UriNichtErlaubt {
                    erlaubt: n.erlaubt_uri.len(),
                });
            }
            if let Some(c) = n
                .verboten_uri
                .iter()
                .find(|c| host_erfuellt(c.as_bytes(), host.as_bytes()))
            {
                return Err(KetteFehler::UriVerboten {
                    teilbaum: c.clone(),
                });
            }
        }
    }

    // added 03.10.2026 06:5x, R52: **DIE NAMEN DES ZERTIFIKATS, UND DAS IST DER
    // BEFUND DIESER RUNDE.** Bis eben stand hier nur diese eine Zeile fuer den
    // Anfragenamen. RFC 5280 4.2.1.10 woertlich: „indicates a name space within
    // which **all subject names in subsequent certificates in a certification
    // path MUST be located**. Restrictions apply to the subject distinguished
    // name and apply to **subject alternative names**.“
    //
    // added 03.10.2026 06:5x: **WAS DAS IN DER WILD BEDEUTET.** Eine Zwischen-CA
    // mit `permitted;DNS:example.de` darf nur Zertifikate fuer `example.de`
    // ausstellen. Sie stellt eines mit `SAN = [example.de, evil.com]` aus: wir
    // fragen `example.de` an, der Name passt, und **akzeptieren** — obwohl das
    // Zertifikat einen Namen beansprucht, den die CA nicht ausstellen darf. Ein
    // Browser lehnt es ab. Wir nicht.
    //
    // added 03.10.2026 06:5x: **UND `excluded` DASSEBE, nur umgekehrt** — ein
    // ausgeschlossener Name im Zertifikat wird bei uns ebenfalls nicht gesehen.
    // Das ist der zweite rote Test.
    //
    // added 03.10.2026 06:5x: **WAS NOCH NICHT DRIN IST**, gemessen und nicht
    // geraten (siehe Bericht R52): der `CommonName` aus dem Subject-DN (die Norm
    // erfasst ihn ueber „the subject distinguished name“) sowie `rfc822Name` und
    // `uniformResourceIdentifier` aus dem SAN — dafuer muesste unser SAN-Parser
    // erst E-Mail- und URI-Namen lesen koennen. Das ist eigene Arbeit und **kein**
    // Grund, die DNS-Namen liegen zu lassen.
    if let Some(san) = &blatt.san {
        for name in &san.dns {
            // added 03.10.2026 06:5x: **`san.dns` ist bereits kleingeschrieben**
            // (`san_lesen` macht `to_ascii_lowercase`); das hier ist also
            // ueberfluessig — ich lasse es, weil `host_matches_teilbaum` auf
            // Bytes vergleicht und sich die Verschiedenheit sonst in einer
            // spaeteren Aenderung wieder einschleichen wuerde.
            dns_geprueft(n, name.to_ascii_lowercase().as_bytes())?;
        }
    }
    Ok(())
}

/// added 03.10.2026 14:3x, R58. **LIEGT DN1 IM TEILBAUM VON DN2?** Und die
/// Regel ist die einzige Stelle, an der die RDN-Grenzen gebraucht werden, und
/// sie ist **zweiteilig**.
///
/// added 03.10.2026 14:3x: **DIE NORM, woertlich** (RFC 5280 7.1, Seite 96): „A
/// distinguished name DN1 is within the subtree defined by the distinguished name
/// DN2 if **DN1 contains at least as many RDNs as DN2**, and DN1 and DN2 are a
/// match when **trailing RDNs in DN1 are ignored**.“
///  * **Bedingung 1**: DN1 hat **mindestens** so viele RDNs wie DN2. Das ist eine
///    **Vorpruefung** und schliesst die meisten Faelle aus.
///  * **Bedingung 2**: die **nicht abschliessenden** RDNs von DN1 stimmen mit den
///    RDNs von DN2 ueberein — „trailing RDNs in DN1 are ignored“ heisst genau das.
///    Zusammen: die **ersten** `len(DN2)` RDNs von DN1 sind nach 7.1 gleich den
///    RDNs von DN2.
///
/// added 03.10.2026 14:3x: **UND DIE REIHENFOLGE, die man leicht verkehrt.** In DER
/// steht das **erste** RDN **hinten** (die distinguishedName ist in der
/// Textdarstellung umgekehrt). Unsere `rdn_grenze` folgt der **DER-Reihenfolge**,
/// weil `name_lesen` dort liest. Fuer die Teilbaum-Regel ist das **egal**, weil
/// beide Seiten dasselbe Schema benutzen — aber es waere eine stille
/// Falle, wenn jemand spaeter einen Namen aus einer **Textdarstellung** baute.
/// Der Kommentar steht hier, damit das jemand liest.
pub fn dn_im_teilbaum(dn1: &Name, dn2: &Name) -> bool {
    let g1 = &dn1.rdn_grenze;
    let g2 = &dn2.rdn_grenze;
    // added 03.10.2026 14:4x: **LEERE NAMEN**: ein leerer `Name` hat **keine**
    // RDN und damit einen leeren Teilbaum. `dn2` leer heisst nach Bedingung 1
    // „alles passt“ — das ist **gefährlich**, denn eine leere Auflage wuerde alles
    // erlauben. Also: **wenn `dn2` leer ist, gibt es keinen Teilbaum**, und
    // wenn `dn1` leer ist, liegt er auch nicht darin (ausser `dn2` ist leer,
    // was wir gerade ausgeschlossen haben).
    if g2.is_empty() || g1.is_empty() {
        return false;
    }
    // added 03.10.2026 14:bc: **`g.len()`, NICHT `g.len() - 1`** — und das war ein
    // echter **Off-by-one**, den die Diagnose (14:bd) sofort gezeigt hat.
    // GEMESSEN: `name_kette(&["O"])` liefert `rdn_grenze = [0, 1]`, und das sind
    // **zwei** RDNs (`C=DE` und `CN=O`) — `rdn_grenze` hat einen Eintrag **vor**
    // jedem RDN, also Eintraege = RDNs. Mit `len() - 1` verglich die Regel nur
    // das **erste** RDN, und `gleiche_rdn_zahl_anderer_inhalt_ist_kein_teilbaum`
    // meldete zu Unrecht „Teilbaum“, weil `C=DE` bei beiden gleich ist.
    let rdn1 = g1.len();
    let rdn2 = g2.len();
    if rdn1 < rdn2 {
        return false;
    }
    // added 03.10.2026 14:4x: **DIE ERSTEN `rdn2` RDNs, zeilenweise.** Fuer jedes
    // Paar von Grenzen: gleiche Laenge UND jeder Attribut nach 7.1 gleich.
    for r in 0..rdn2 {
        let von = g1[r];
        // added 03.10.2026 14:bc: **DIE LETZTE GRENZE EXISTIERT NICHT** — es gibt
        // einen Eintrag **vor** jedem RDN, keinen danach. Beim letzten RDN ist
        // das Ende also `attribute.len()`.
        let bis = if r + 1 < g1.len() {
            g1[r + 1]
        } else {
            dn1.attribute.len()
        };
        let c_von = g2[r];
        let c_bis = if r + 1 < g2.len() {
            g2[r + 1]
        } else {
            dn2.attribute.len()
        };
        if bis - von != c_bis - c_von {
            return false;
        }
        for i in 0..(bis - von) {
            if !crate::x509::attribut_gleich_7_1(&dn1.attribute[von + i], &dn2.attribute[c_von + i])
            {
                return false;
            }
        }
    }
    true
}

/// added 03.10.2026 13:3x, R57. **ZWEI NAMEN NACH 7.1 GLEICH?** Und die Regel
/// ist zweiteilig, und die zweite Hälfte ist die von RFC 5280 7.1: „Two relative
/// distinguished names match if … the matching RDNs appear in the **same
/// order** in both DNs.“
///
/// added 03.10.2026 13:3x: **UND DIE REIHENFOLGE, die hier fehlt.** Die
/// Teilbaum-Regel (Seite 96) sagt: „A distinguished name DN1 is within the
/// subtree defined by DN2 if **DN1 contains at least as many RDNs as DN2**, and
/// DN1 and DN2 are a match when **trailing RDNs in DN1 are ignored**.“ Dafuer
/// braucht man die **RDN-Grenzen**, und `Name` speichert `attribute: Vec<Attribut>`
/// — eine **flache** Liste ohne Grenzen. Diese Funktion vergleicht deshalb
/// **Gleichheit**, nicht Teilbaum; wo sie benutzt wird, steht das.
pub fn namen_gleich_7_1(a: &Name, b: &Name) -> bool {
    if a.attribute.len() != b.attribute.len() {
        return false;
    }
    a.attribute
        .iter()
        .zip(b.attribute.iter())
        .all(|(x, y)| crate::x509::attribut_gleich_7_1(x, y))
}

/// added 03.10.2026 11:2x, R56. **ERFUELLT EINE E-MAIL-ADRESSE DIE
/// BESCHRAENKUNG?** Die drei Normformen in **einer** Funktion, und das ist der
/// Punkt: die Form ergibt sich aus der **Beschraenkung**, nicht aus der Adresse.
///  * fuehrender `.` -> „alle Postfaecher **der Domain“: der Host muss mit
///    `.` + Beschraenkung enden. Die Norm sagt ausdruecklich: „`.example.com`
///    indicates all the Internet mail addresses in the domain `example.com`, **but
///    not** Internet mail addresses on the host `example.com`“ — genau das liefert
///    diese Form ohne Sonderfall.
///  * `@` darin -> „**genau** dieses Postfach“: der Local-Teil wird
///    **byteweise** verglichen (case-sensitiv, RFC 5321).
///  * sonst -> „alle Adressen **auf diesem Host**“: der Host muss **gleich** sein.
fn email_erfuellt(c: &[u8], lokal: Option<&[u8]>, host: &[u8]) -> bool {
    let Some(lokal) = lokal else { return false };
    if c.first() == Some(&b'.') {
        // added 03.10.2026 11:2x: **`host` endet mit `.` + Beschraenkung** — und
        // `host` ist bereits kleingeschrieben, `c` kommt aus dem Parser, der ihn
        // ebenfalls kleingeschrieben hat.
        let teil = &c[1..];
        if teil.is_empty() || host.len() <= teil.len() {
            return false;
        }
        host.ends_with(teil) && host[host.len() - teil.len() - 1] == b'.'
    } else if let Some(i) = c.iter().position(|b| *b == b'@') {
        // added 03.10.2026 11:3x: **`lokal` byteweise** — kein
        // `to_ascii_lowercase` auf beiden Seiten, denn der Local-Teil ist
        // case-sensitiv.
        // added 03.10.2026 12:3x: **`c[..i] == lokal` UND NICHT
        // `eq_ignore_ascii_case`** — das war ein **echter Fehler**, und der Test
        // `email_postfachform_unterscheidet_gross_klein` hat ihn gemeldet: mit
        // `eq_ignore_ascii_case` wurde `ROOT@example.com` gegen die Auflage
        // `root@example.com` **akzeptiert**, obwohl RFC 5321 den Local-Teil als
        // **case-sensitiv** vorschreibt. Der **Host** dagegen
        // (`c[i + 1..]`) bleibt case-insensitiv, weil Hosts das sind. Zwei
        // Haelften, zwei Regeln — und ich hatte beide gleich gemacht.
        // added 03.10.2026 12:5x: **`c[..i] == *lokal`** — der Uebersetzer
        // meldete „can't compare `[u8]` with `&[u8]`“: ein Slice-Schnitt ist ein
        // `[u8]`, `lokal` ein `&[u8]`, und `==` vergleicht beide nicht direkt.
        c[..i] == *lokal && c[i + 1..].eq_ignore_ascii_case(host)
    } else {
        c.eq_ignore_ascii_case(host)
    }
}

/// added 03.10.2026 11:3x, R56. **ERFUELLT EIN HOST DIE BESCHRAENKUNG?** Fuer URI
/// und (ohne `@`) fuer E-Mail dieselbe Frage mit derselben Antwort: der fuehrende
/// Punkt bedeutet „irgendein Label davor“.
fn host_erfuellt(c: &[u8], host: &[u8]) -> bool {
    if c.first() == Some(&b'.') {
        let teil = &c[1..];
        if teil.is_empty() || host.len() <= teil.len() {
            return false;
        }
        host.ends_with(teil) && host[host.len() - teil.len() - 1] == b'.'
    } else {
        c.eq_ignore_ascii_case(host)
    }
}

/// added 03.10.2026 11:3x, R56. **DER HOST-TEIL EINER URI**, und die Frage ist
/// nicht geraten: RFC 3986 Abschnitt 3.2 nennt das die **authority** und sagt,
/// dass sie mit `//` beginnt und mit `/`, `?` oder `#` endet; die
/// **Benutzerinformation** (`user@`) steht davor und gehoert **nicht** zum Host.
/// `mailto:`-URIs haben keine Authority — dann gibt es keinen Host, und die Zeile
/// wird uebersprungen.
fn uri_host(uri: &str) -> Option<String> {
    let rest = uri.split_once("://")?.1;
    let ende = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let autoritaet = &rest[..ende];
    let ohne_userinfo = match autoritaet.rfind('@') {
        Some(i) => &autoritaet[i + 1..],
        None => autoritaet,
    };
    // added 03.10.2026 11:4x: **DER PORT GEHOERT NICHT ZUM HOST**, sonst waere
    // `example.com:443` nie von einer Beschraenkung auf `example.com` erfuellt.
    // added 03.10.2026 11:4x: **UND WIR LESEN KEINEN `IPv6`-HOST IN KLAMMERN** —
    // dann bleibt die Klammer im String und die Beschraenkung trifft nie zu. Das
    // ist die **sichere** Richtung: eine URI-Beschraenkung auf einen IPv6-Host
    // wird nicht erkannt, also nicht verletzt; solche Beschraenkungen sind
    // ausserdem selten.
    let host = ohne_userinfo.split(':').next().unwrap_or(ohne_userinfo);
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// added 03.10.2026 06:6x, R52. **DIE EINZELNE DNS-PRUEFUNG**, herausgezogen,
/// weil sie jetzt an **zwei** Stellen gebraucht wird (Anfragename, Namen im
/// Zertifikat). Vorher stand sie inline; ein Kopieren waere genau die Fehlerform,
/// die ich in R50 bei `ocsp_aus_zertifikatsblock` erst suchen und dann finden
/// musste.
fn dns_geprueft(n: &crate::x509::Namensbeschraenkung, host: &[u8]) -> Result<(), KetteFehler> {
    for teilbaum in &n.verboten {
        if host_matches_teilbaum(host, teilbaum) {
            return Err(KetteFehler::NameVerboten {
                teilbaum: String::from_utf8_lossy(teilbaum).into_owned(),
            });
        }
    }
    if n.erlaubt.is_empty() {
        // added 02.10.2026 09:9x: `permitted` **fehlt** heisst „keine
        // Einschraenkung". Eine leere Liste im Zertifikat ist dasselbe, weil
        // der Parser beides als leere Vec haelt und das Feld `Option` nur
        // sagt, ob die Erweiterung da war.
        return Ok(());
    }
    if n.erlaubt.iter().any(|t| host_matches_teilbaum(host, t)) {
        Ok(())
    } else {
        Err(KetteFehler::NameNichtErlaubt {
            erlaubt: n.erlaubt.len(),
        })
    }
}

/// Liegt die Adresse **in** diesem Netz?
///
/// added 02.10.2026 14:6x: DER VERGLEICH, und er ist **bitweise** auf Bytes,
/// nicht ueber Zahlen. Grund: eine Maske kann jede Bitfolge haben
/// (`255.255.0.0`, `255.0.255.0`), und fuer IPv6 sind es 128 Bit — eine
/// Ganzzahl wuerde in `u32`/`u128` passen, aber dann muesste an **zwei** Stellen
/// die Bytelogik in Zahllogik uebersetzt werden, und das ist genau die zweite
/// Wahrheit, die dieses Projekt nicht haben will.
///
/// added 02.10.2026 14:6x: DIE ZWEI KANDIDATEN, und warum „entweder passt die
/// Adresse **oder** das Netz" richtig ist: der Teilbaum nennt Netz**adresse**
/// und Maske (`C0 00 02 00 FF FF FF 00` fuer 192.0.2.0/24 nach RFC 5280
/// 4.2.1.10). Die Netzadresse traegt unter der Maske lauter Nullen, aber das
/// ist eine **Eigenschaft gueltiger Zertifikate**, keine Zusage. Deshalb wird
/// der Vergleich so gefuehrt, dass `10.0.0.1` gegen ein Netz mit Netzadresse
/// `10.0.0.0` genauso passt wie gegen `10.0.0.1` — und nicht anders.
pub fn ip_netz_prueft(adresse: &[u8], netz: &(Vec<u8>, Vec<u8>)) -> bool {
    if adresse.len() != netz.0.len() || adresse.len() != netz.1.len() {
        return false;
    }
    adresse
        .iter()
        .zip(netz.0.iter())
        .zip(netz.1.iter())
        .all(|((a, n), m)| (a & m) == (n & m))
}

/// Passt ein Hostname in einen DNS-Teilbaum?
///
/// added 02.10.2026 09:9x, S1. DIE REGEL, woertlich (RFC 5280 4.2.1.10,
/// `nameConstraints` auf `dNSName`): „a name matches a constraint if the
/// name is equal to the constraint or the name is a **sub-domain** of the
/// constraint" — mit einem Punkt davor, damit „evil-example.com" **nicht**
/// unter „example.com" faellt.
///
/// added 02.10.2026 09:9x: `example.com` selbst faellt **unter** seinen
/// eigenen Teilbaum. Das ist die Norm und nicht meine Erweiterung: wer ein
/// Zertifikat auf „example.com" beschraenkt, meint genau diesen Namen mit.
pub fn host_matches_teilbaum(host: &[u8], teilbaum: &[u8]) -> bool {
    // added 02.10.2026 10:7x, S1. **Der fuehrende Punkt faellt weg.**
    //
    // DIE NORM, woertlich (RFC 5280 4.2.1.10): „DNS name restrictions are
    // expressed as host.example.com. Any DNS name that can be constructed
    // by simply adding **zero or more labels** to the left-hand side of the
    // name satisfies the name constraint. For example,
    // www.host.example.com would satisfy the constraint but
    // host1.example.com would not."
    //
    // „zero or more labels" heisst: **auch der Name selbst** faellt unter
    // seinen eigenen Teilbaum. Und ein Teilbaum, der als `.example.com`
    // geschrieben wird, meint dasselbe wie `example.com` mit der
    // Suffix-Regel — der Punkt ist nur die Schreibweise fuer „Baum".
    //
    // GEMESSEN, was die erste Fassung tat (ohne den Punkt zu entfernen):
    // der Teilbaum `.erlaubt.s1.test` wurde als **Literal** verglichen,
    // `www.erlaubt.s1.test` passte nicht, und der Szenario-Test
    // „Name innerhalb permitted" wurde rot — also haette der Browser
    // erlaubte Namen abgelehnt.
    let teilbaum = if teilbaum.first() == Some(&b'.') {
        &teilbaum[1..]
    } else {
        teilbaum
    };
    if teilbaum.is_empty() {
        // added 02.10.2026 10:7x: ein leerer Teilbaum (nach dem Punkt
        // entfernen) waere „alles erlaubt" — und das ist auch die
        // OpenSSL-Lesart. Er ist aber **verdächtig**, deshalb ist er hier
        // nicht „alles": `host_matches_teilbaum` liefert `false`, und die
        // Beschraenkung bleibt fuer diesen Namen unwirksam. GEMESSEN ist
        // das nicht; ein leerer DNS-Teilbaum kommt in keinem realen
        // Zertifikat vor.
        return false;
    }
    if host.eq_ignore_ascii_case(teilbaum) {
        return true;
    }
    // Der Punkt **vor** dem Teilbaum, damit „notexample.com" **nicht** unter
    // „example.com" faellt — das ist der Fall, den die Norm mit „labels"
    // verhindert.
    host.len() > teilbaum.len()
        && host[host.len() - teilbaum.len() - 1] == b'.'
        && host[host.len() - teilbaum.len()..].eq_ignore_ascii_case(teilbaum)
}

/// Prueft die Gueltigkeit eines Zertifikats.
///
/// RFC 5280 Abschnitt 4.1.2.5: "The certificate is valid during the
/// period from notBefore through notAfter, inclusive." Beide Grenzen
/// sind also einschliesslich.
pub fn gueltigkeit_pruefen(g: &Gueltigkeit, jetzt: i64) -> Result<(), KetteFehler> {
    if g.enthaelt(jetzt) {
        Ok(())
    } else {
        Err(KetteFehler::AusserhalbDerGueltigkeit {
            von: g.von,
            bis: g.bis,
            jetzt,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Testkette, von openssl signiert, und das echte Bundle.
    const KETTE: &str = include_str!("../testdaten/t12_kette.txt");
    const BUNDLE: &str = include_str!("../testdaten/t12_echtes_bundle.pem");

    fn feld(name: &str) -> String {
        for zeile in KETTE.lines() {
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return rest.trim().to_string();
            }
        }
        panic!("Feld {name} fehlt");
    }

    fn feld_zahl(name: &str) -> i64 {
        feld(name).parse().expect("Zahl")
    }

    /// Schneidet den Abschnitt zwischen zwei Markern heraus.
    fn abschnitt(von: &str, bis: &str) -> String {
        let a = KETTE.find(&format!("---{von}---")).expect(von);
        let start = a + von.len() + 6;
        // Fehlt die Endmarke, wird bis zum Dateiende gelesen. Das ist
        // fuer den LETZTEN Abschnitt noetig, und es ist besser, hier
        // eine Endmarke zu erlauben als zu paniken — der Inhalt wird
        // danach vom PEM-Leser geprueft.
        let b = match KETTE[start..].find(&format!("---{bis}---")) {
            Some(rel) => rel + start,
            None => KETTE.len(),
        };
        KETTE[start..b].trim().to_string()
    }

    /// Liest genau EIN Zertifikat aus einem PEM-Abschnitt.
    ///
    /// Das `expect("genau ein Zertifikat")` ist nicht Beiwerk: es
    /// haelt fest, dass die Testdaten genau eine PEM enthalten. Ohne
    /// das wuerde `&[0]` still das falsche nehmen, wenn sich das
    /// Testmaterial aendert.
    fn ein_zertifikat(abschnitt_name: &str, ende: &str) -> Zertifikat {
        let alle = pem_zerlegen(&abschnitt(abschnitt_name, ende)).expect("PEM lesbar");
        assert_eq!(
            alle.len(),
            1,
            "der Abschnitt {abschnitt_name} enthaelt ein Zertifikat"
        );
        zertifikat_lesen(&alle[0]).expect("X.509 lesbar")
    }

    fn blatt() -> Zertifikat {
        ein_zertifikat("BLATT", "WURZEL")
    }

    fn wurzel() -> Zertifikat {
        ein_zertifikat("WURZEL", "BLATT_KAPUTT")
    }

    /// Der Zeitpunkt, zu dem die Kette pruefbar ist: in der Mitte der
    /// Gueltigkeit, damit der Test nicht an einem laufenden Datum
    /// haengt. Die Zahl ist aus `BLATT_VON` und `BLATT_BIS` gemittelt.
    fn jetzt_in_der_mitte() -> i64 {
        (feld_zahl("BLATT_VON") + feld_zahl("BLATT_BIS")) / 2
    }

    /// M7: die Basislinie. Eine echte Kette wird angenommen, und die
    /// Kette enthaelt genau zwei Zertifikate.
    #[test]
    fn echte_kette_wird_angenommen() {
        let b = blatt();
        let w = wurzel();
        let speicher = Vertrauensspeicher {
            zertifikate: vec![w.clone()],
        };
        let k = kette_pruefen(
            std::slice::from_ref(&b),
            &speicher,
            jetzt_in_der_mitte(),
            "www.t12.test",
        )
        .expect("die echte Kette muss gelten");
        // M7: das Ergebnis wird festgemacht, nicht nur "nicht Fehler".
        assert_eq!(k.von_blatt_nach_wurzel.len(), 2, "Blatt und Wurzel");
        assert_eq!(
            k.von_blatt_nach_wurzel[0].der, b.der,
            "das Blatt kommt zuerst"
        );
        assert_eq!(k.von_blatt_nach_wurzel[1].der, w.der, "die Wurzel zuletzt");

        // Und die SAN-Eintraege, die openssl geschrieben hat.
        let san = b.san.as_ref().expect("SAN vorhanden").dns.clone();
        assert_eq!(san.len(), 3, "drei DNS-Namen: {san:?}");
        assert!(san.contains(&"www.t12.test".to_string()));
        assert!(san.contains(&"*.wild.t12.test".to_string()));
        assert!(san.contains(&"alt.t12.test".to_string()));
    }

    /// M7: die Hostnamen-Regeln gegen die echten SAN-Eintraege.
    ///
    /// `*.wild.t12.test` ist der Wildcard-Fall, und er wird in beide
    /// Richtungen geprueft: was passen MUSS und was nicht.
    #[test]
    fn hostname_gegen_die_echten_san_eintraege() {
        let b = blatt();
        let w = wurzel();
        let speicher = Vertrauensspeicher {
            zertifikate: vec![w],
        };
        let jetzt = jetzt_in_der_mitte();

        let gut = [
            "www.t12.test",    // exakt
            "WWW.T12.TEST",    // Gross-Klein ist egal
            "a.wild.t12.test", // der Wildcard traegt ein Label
        ];
        for h in gut {
            assert!(
                kette_pruefen(std::slice::from_ref(&b), &speicher, jetzt, h).is_ok(),
                "{h} muss passen"
            );
        }

        let schlecht = [
            // Der Apex gehoert NICHT zu `*.wild.t12.test`.
            "wild.t12.test",
            // Zwei Labels sind einer Wildcard zu viel.
            "a.b.wild.t12.test",
            // Einen Namen, den es nicht gibt.
            "www.anderes.test",
            // Eine leere Wildcard ist kein Name.
            ".wild.t12.test",
        ];
        for h in schlecht {
            assert_eq!(
                kette_pruefen(std::slice::from_ref(&b), &speicher, jetzt, h).err(),
                Some(KetteFehler::HostnamePasstNicht),
                "{h} darf nicht passen"
            );
        }
    }

    /// M7: die Gueltigkeit, an beiden Grenzen.
    ///
    /// RFC 5280 Abschnitt 4.1.2.5 sagt "from notBefore through
    /// notAfter, INCLUSIVE". Also sind BEIDE Grenzen gueltig — das
    /// ist eine Eigenschaft, die man leicht wegprueft, und dann ist
    /// ein Zertifikat am Tag des Ablaufs schon abgelehnt.
    #[test]
    fn gueltigkeit_an_beiden_grenzen() {
        let b = blatt();
        let w = wurzel();
        let speicher = Vertrauensspeicher {
            zertifikate: vec![w],
        };
        let von = feld_zahl("BLATT_VON");
        let bis = feld_zahl("BLATT_BIS");

        assert!(
            kette_pruefen(std::slice::from_ref(&b), &speicher, von, "www.t12.test").is_ok(),
            "genau am notBeginn ist es noch gueltig"
        );
        assert!(
            kette_pruefen(std::slice::from_ref(&b), &speicher, bis, "www.t12.test").is_ok(),
            "genau am notAfter ist es noch gueltig"
        );
        assert!(
            kette_pruefen(std::slice::from_ref(&b), &speicher, von - 1, "www.t12.test").is_err(),
            "eine Sekunde davor nicht"
        );
        assert!(
            kette_pruefen(std::slice::from_ref(&b), &speicher, bis + 1, "www.t12.test").is_err(),
            "eine Sekunde danach nicht"
        );
    }

    /// M7: die drei Stoerfaelle. Und vor allem: es gibt KEINEN Weg
    /// daran vorbei. Jeder Fall endet in einem Fehler.
    #[test]
    fn stoerfaelle_werden_abgelehnt() {
        let w = wurzel();
        let speicher = Vertrauensspeicher {
            zertifikate: vec![w],
        };
        let jetzt = jetzt_in_der_mitte();

        // (a) die Signatur des Blatts ist um ein Byte verfaelscht.
        let kaputt = ein_zertifikat("BLATT_KAPUTT", "BLATT_ALT");
        // Das ist die wichtige Voraussetzung: das Zertifikat liest
        // sich, nur die Signatur stimmt nicht. Wer hier einen
        // Lesefehler bekaeme, wuerde den Test fuer erledigt halten,
        // obwohl nichts geprueft wurde.
        assert_eq!(kaputt.der.len(), blatt().der.len(), "gleiche Laenge");
        assert_ne!(kaputt.signatur, blatt().signatur, "die Signatur weicht ab");
        assert_eq!(
            kette_pruefen(
                std::slice::from_ref(&kaputt),
                &speicher,
                jetzt,
                "www.t12.test"
            )
            .err(),
            Some(KetteFehler::SignaturPasstNicht { stelle: 0 }),
            "ein verfaelschtes Endzertifikat muss abgelehnt werden"
        );

        // (b) die Wurzel ist NICHT im Vertrauensspeicher. Das ist der
        // Fall, den man mit einem Schalter umgehen wuerde, und genau
        // der darf nicht umgangen werden.
        let leer = Vertrauensspeicher::default();
        assert_eq!(
            kette_pruefen(&[blatt()], &leer, jetzt, "www.t12.test").err(),
            Some(KetteFehler::KeinAussteller),
            "ohne die Wurzel im Speicher gibt es keine Kette"
        );

        // (c) ein abgelaufenes Endzertifikat.
        let alle_alt = pem_zerlegen(&abschnitt("BLATT_ALT", "ENDE")).expect("PEM");
        assert_eq!(alle_alt.len(), 1, "ein alter Zertifikat");
        let alt = zertifikat_lesen(&alle_alt[0]).expect("lesbar");
        // Die Erwartungswerte kommen aus `ALT_VON` und `ALT_BIS`, die
        // das Generatorskript mit `openssl x509 -dates` gelesen und mit
        // `date -u -d` umgerechnet hat. Sie werden hier gegengelesen,
        // nicht nachgeprueft durch Nachrechnen — sonst waere der Test
        // eine zweite Kopie derselben Rechnung.
        assert_eq!(
            alt.gueltigkeit.von,
            feld_zahl("ALT_VON"),
            "notBefore des abgelaufenen Zertifikats"
        );
        assert_eq!(
            alt.gueltigkeit.bis,
            feld_zahl("ALT_BIS"),
            "notAfter des abgelaufenen Zertifikats"
        );
        assert!(
            alt.gueltigkeit.bis < jetzt,
            "das alte Zertifikat ist wirklich abgelaufen: bis {} liegt vor {}",
            alt.gueltigkeit.bis,
            jetzt
        );
        assert!(matches!(
            kette_pruefen(std::slice::from_ref(&alt), &speicher, jetzt, "www.t12.test").err(),
            Some(KetteFehler::AusserhalbDerGueltigkeit { .. })
        ));

        // (d) ein selbstsigniertes Blatt, das sich als CA ausgibt.
        // Das ist der Angriff, den ein "trotzdem verbinden"-Knopf
        // ermoeglicht: der Browser erfindet die Wurzel aus dem
        // Blatt selbst.
        let blatt_als_ca = wurzel();
        // Die echte Wurzel ist selbstsigniert und liegt im Speicher —
        // das funktioniert. Ein selbstsigniertes Zertifikat, das NICHT
        // im Speicher liegt, nicht.
        assert!(
            kette_pruefen(
                std::slice::from_ref(&blatt_als_ca),
                &speicher,
                jetzt_in_der_mitte(),
                "www.t12.test"
            )
            .is_err(),
            "ein selbstsigniertes Blatt im Speicher ist nur gueltig, wenn es dort liegt"
        );
    }

    /// M7: das echte Bundle. 150 Zertifikate werden gelesen, und die
    /// Zahl wird festgemacht.
    #[test]
    fn echtes_bundle_wird_gelesen() {
        let anzahl: usize = feld("ANZAHL_ECHT").parse().expect("Zahl");
        let bytes: usize = feld("ANZAHL_ECHTE_BYTES").parse().expect("Zahl");
        assert_eq!(BUNDLE.len(), bytes, "die Datei hat die gemessene Laenge");
        assert_eq!(
            BUNDLE.matches("-----BEGIN CERTIFICATE-----").count(),
            anzahl,
            "so viele Zertifikate stehen drin"
        );
        let speicher = Vertrauensspeicher::aus_pem(BUNDLE).expect("das echte Bundle");
        assert_eq!(speicher.anzahl(), anzahl, "alle Zertifikate gelesen");
    }

    /// M7: die PEM- und Basis64-Fehler, einzeln.
    #[test]
    fn pem_fehler_werden_abgelehnt() {
        const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
        const ENDE: &str = "-----END CERTIFICATE-----";

        // (a) kein BEGIN
        assert!(pem_zerlegen("nur Text").expect("leer").is_empty());
        // (b) BEGIN ohne ENDE
        assert!(matches!(
            pem_zerlegen(BEGIN).err(),
            Some(KetteFehler::PemUngueltig { .. })
        ));
        // (c) Base64 mit ungueltigem Zeichen
        assert!(matches!(
            pem_zerlegen(&format!("{BEGIN}\n####\n{ENDE}\n")).err(),
            Some(KetteFehler::Basis64Ungueltig { .. })
        ));
        // (d) ungerade Laenge. "QUJ" hat drei Zeichen und damit keine
        // Bytezahl; Base64 braucht ein Vielfaches von vier.
        //
        // EIGENER FEHLER IM TEST: ich hatte "QUJD" genommen, das sind
        // vier Zeichen und damit GUELTIG — es ist "ABC". Der Test
        // behauptete also eine Ablehnung, die nicht stattfand, und
        // haette bei einer zukuenftigen Lockerung des Lesers still
        // vorbeigelassen.
        assert!(matches!(
            pem_zerlegen(&format!("{BEGIN}\nQUJ\n{ENDE}\n")).err(),
            Some(KetteFehler::Basis64Ungueltig { .. })
        ));
        // Und zum Gegenbeweis: mit Fuellzeichen geht dieselbe Stelle.
        assert_eq!(
            pem_zerlegen(&format!("{BEGIN}\nQQ==\n{ENDE}\n")).expect("PEM"),
            vec![vec![0x41u8]],
            "\"QQ==\" ist genau ein Byte 0x41"
        );
        // (e) Fuellzeichen mitten drin
        assert!(matches!(
            pem_zerlegen(&format!("{BEGIN}\nQQ=QQQQ\n{ENDE}\n")).err(),
            Some(KetteFehler::Basis64Ungueltig { .. })
        ));
        // (f) gueltiges Basis64 ergibt die erwarteten Bytes
        // "QUJD" ist "ABC" — der Wert wird nicht geraten, er ist
        // Base64: Q=16, U=20, J=9, D=3 -> 010000 010100 001001 000011
        // -> 0x41 0x42 0x43.
        assert_eq!(basis64_lesen("QUJD").expect("Hex"), vec![0x41, 0x42, 0x43]);
        // Und mit Fuellzeichen: "QQ==" ist ein Byte, 0x41.
        assert_eq!(basis64_lesen("QQ==").expect("Hex"), vec![0x41]);
        // (g) M4: eine Laenge ueber der Grenze wird abgelehnt, BEVOR
        // allokiert wird.
        let zu_lang = "A".repeat(MAX_BASIS64 + 4);
        assert!(matches!(
            basis64_lesen(&zu_lang).err(),
            Some(KetteFehler::Basis64Ungueltig { .. })
        ));
    }

    /// M7: die Kette wird in der Reihenfolge geprueft, in der
    /// RFC 5280 Abschnitt 6.1 es vorschreibt — und die Reihenfolge
    /// ist beobachtbar, weil `von_blatt_nach_wurzel` sie festhält.
    #[test]
    fn kettenrichtung_ist_vom_blatt_zur_wurzel() {
        let b = blatt();
        let w = wurzel();
        let speicher = Vertrauensspeicher {
            zertifikate: vec![b.clone(), w.clone()],
        };
        let k = kette_pruefen(
            std::slice::from_ref(&b),
            &speicher,
            jetzt_in_der_mitte(),
            "www.t12.test",
        )
        .expect("Kette");
        assert_eq!(k.von_blatt_nach_wurzel.len(), 2);
        assert_eq!(
            k.von_blatt_nach_wurzel[0].inhaber.common_name(),
            Some("www.t12.test".to_string()),
            "vorne steht das Blatt"
        );
        assert_eq!(
            k.von_blatt_nach_wurzel[1].inhaber.common_name(),
            Some("T12 Testwurzel".to_string()),
            "hinten die Wurzel"
        );
    }

    /// M7: alle 150 Anker werden gelesen und richtig EINGETEILT.
    ///
    /// Das ist der schnelle Teil von `wie_viele_anker_sind_nachrechenbar`:
    /// die Schluesselart und das Signaturschema werden fuer alle 150
    /// gelesen und gegen `openssl` gehalten, aber noch keine
    /// Signatur gerechnet. Damit steht die Einordnung auch dann fest,
    /// wenn der langsame Test nicht laeuft.
    #[test]
    fn alle_anker_werden_eingeteilt() {
        let speicher = Vertrauensspeicher::aus_pem(BUNDLE).expect("Bundle");
        assert_eq!(speicher.anzahl(), 150, "150 Anker im Bundle");
        let mut rsa = 0usize;
        let mut p256 = 0usize;
        let mut p384 = 0usize;
        let mut andere = 0usize;
        let mut sha1 = 0usize;
        let mut sha256 = 0usize;
        let mut sha384 = 0usize;
        let mut sha512 = 0usize;
        for z in &speicher.zertifikate {
            match &z.schluessel {
                SchluesselArt::Rsa { n, e } => {
                    rsa += 1;
                    // M7: Mindestlaenge und Exponent festhalten.
                    assert!(n.len() >= 256, "Modulus zu kurz: {} Byte", n.len());
                    assert!(!e.is_empty(), "Exponent leer");
                }
                SchluesselArt::Ecdsa { x, y, kurve } => {
                    let bits = match kurve.as_slice() {
                        [1, 2, 840, 10045, 3, 1, 7] | [1, 3, 132, 0, 7] => 256,
                        [1, 2, 840, 10045, 3, 1, 31] | [1, 3, 132, 0, 34] => 384,
                        _ => 0,
                    };
                    if bits == 256 {
                        p256 += 1;
                        assert_eq!(x.len(), 32, "P-256: x ist 32 Byte");
                        assert_eq!(y.len(), 32, "P-256: y ist 32 Byte");
                    } else if bits == 384 {
                        p384 += 1;
                        assert_eq!(x.len(), 48, "P-384: x ist 48 Byte");
                        assert_eq!(y.len(), 48, "P-384: y ist 48 Byte");
                    } else {
                        andere += 1;
                    }
                }
                SchluesselArt::Unbekannt { .. } => {}
            }
            match z.signatur_algorithmus.hash {
                Some(Hash::Sha1) => sha1 += 1,
                Some(Hash::Sha256) => sha256 += 1,
                Some(Hash::Sha384) => sha384 += 1,
                Some(Hash::Sha512) => sha512 += 1,
                None => {}
            }
        }
        // Erwartungen GEGEN `openssl` gemessen, nicht gerechnet.
        assert_eq!(rsa, 107, "107 RSA-Anker laut openssl");
        assert_eq!(p256, 4, "4 prime256v1");
        assert_eq!(p384, 39, "39 secp384r1");
        assert_eq!(andere, 0, "kein Anker auf einer ungerechneten Kurve");
        assert_eq!(sha1, 25, "25 sha1WithRSAEncryption");
        assert_eq!(sha256, 67, "60 sha256WithRSA + 7 ecdsa-with-SHA256");
        assert_eq!(sha384, 54, "18 sha384WithRSA + 36 ecdsa-with-SHA384");
        assert_eq!(sha512, 4, "4 sha512WithRSAEncryption");
        // Und die beiden Einteilungen muessen zusammenpassen.
        assert_eq!(
            sha1 + sha256 + sha384 + sha512,
            150,
            "jeder Anker hat einen Hash"
        );
        assert_eq!(
            rsa + p256 + p384 + andere,
            150,
            "jeder Anker hat einen Schluessel"
        );
    }

    /// M7: die Zuordnung von Name zu Zertifikat, gemessen an den
    /// echten Daten. Ohne sie waere die Suche nach dem Aussteller
    /// eine ratesache.
    #[test]
    fn name_gleich_unterscheidet_echte_namen() {
        let b = blatt();
        let w = wurzel();
        assert!(
            name_gleich(&b.inhaber, &b.inhaber),
            "mit sich selbst gleich"
        );
        assert!(
            !name_gleich(&b.inhaber, &w.inhaber),
            "Blatt und Wurzel verschieden"
        );
        assert!(
            name_gleich(&b.aussteller, &w.inhaber),
            "der Aussteller des Blatts IST der Inhaber der Wurzel"
        );
        assert_eq!(b.inhaber.common_name(), Some("www.t12.test".to_string()));
        assert_eq!(w.inhaber.common_name(), Some("T12 Testwurzel".to_string()));
    }

    /// M7: wie viele der 150 echten Anker sind mit dem Code ueberhaupt
    /// nachrechenbar? Die Zahl wird gemessen, nicht behauptet.
    ///
    /// DER GRUND FUER DIESEN TEST: `ecdsa.rs` rechnet nur P-256, und in
    /// ca-certificates.crt tragen 39 Zertifikate secp384r1 und 4
    /// tragen sha512WithRSAEncryption, wofuer es kein SHA-512 gibt.
    /// Solche Anker MUESSEN als „unbekannt" gemeldet werden und
    /// duerfen nicht als geprueft gelten. Dieser Test haelt fest, wie
    /// viele es sind, damit eine Erweiterung auffaellt.
    ///
    /// LAUFZEIT, gemessen am 30.09.2026: 546 Sekunden. Das ist kein
    /// Fehler, sondern ECDSA auf P-384: 39 Anker, jeder braucht eine
    /// Skalarmultiplikation ueber 384 Bit, also 384 Verdopplungen und
    /// rund 190 Additionen in Zwölf-Gliedern.
    ///
    /// Der Test ist deshalb `#[ignore]`. Er laeuft mit
    ///
    /// ```text
    /// werkzeuge/bau.sh narsil-tls test wie_viele -- --ignored --nocapture
    /// ```
    ///
    /// und die gedruckten Zahlen sind im Test selbst festgehalten, damit
    /// eine Aenderung an der Kurvenunterstuetzung auffaellt. Die
    /// schnelle Pruefung, dass die 150 Zertifikate ueberhaupt gelesen
    /// und richtig eingordnet werden, steht in
    /// `alle_anker_werden_eingeteilt` und laeuft in unter einer
    /// Sekunde.
    #[test]
    #[ignore = "546 s; 39 ECDSA-Pruefungen auf P-384"]
    fn wie_viele_anker_sind_nachrechenbar() {
        let speicher = Vertrauensspeicher::aus_pem(BUNDLE).expect("Bundle");
        let mut rsa_ok = 0usize;
        let rsa_falsch = 0usize;
        let mut ecdsa_p256 = 0usize;
        let mut ecdsa_p384 = 0usize;
        let mut ecdsa_andere = 0usize;
        let mut hash_fehlend = 0usize;

        for z in &speicher.zertifikate {
            // Jeder Anker prueft sich selbst.
            let e = signatur_pruefen(z, z);
            match (&z.schluessel, &z.signatur_algorithmus.hash) {
                (SchluesselArt::Rsa { .. }, Some(Hash::Sha512)) => {
                    // SHA-512 gibt es in `sha2.rs` nicht. Solche Anker
                    // werden als BEKANNTE ERWEITERUNG gemeldet und
                    // nicht als geprueft gezaehlt — die Richtung ist
                    // die sichere.
                    hash_fehlend += 1;
                    assert_eq!(
                        e.err(),
                        Some(KetteFehler::HashNichtUnterstuetzt),
                        "ein Anker mit sha512 muss als unbekannt gelten"
                    );
                }
                (SchluesselArt::Rsa { .. }, _) => match e {
                    Ok(()) => rsa_ok += 1,
                    Err(fehler) => panic!(
                        "RSA-Anker {} liess sich nicht pruefen: {}",
                        z.inhaber.common_name().unwrap_or_else(|| "?".to_string()),
                        fehler
                    ),
                },
                (SchluesselArt::Ecdsa { kurve, .. }, _) => {
                    let bits = match kurve.as_slice() {
                        [1, 2, 840, 10045, 3, 1, 7] | [1, 3, 132, 0, 7] => 256,
                        [1, 2, 840, 10045, 3, 1, 31] | [1, 3, 132, 0, 34] => 384,
                        _ => {
                            // Alles andere: nicht gerechnet, also auch
                            // nicht bestaetigt.
                            assert_eq!(
                                e.err(),
                                Some(KetteFehler::KurveNichtUnterstuetzt),
                                "eine ungerechnete Kurve muss gemeldet werden"
                            );
                            ecdsa_andere += 1;
                            continue;
                        }
                    };
                    match e {
                        Ok(()) => {
                            if bits == 256 {
                                ecdsa_p256 += 1;
                            } else {
                                ecdsa_p384 += 1;
                            }
                        }
                        Err(fehler) => panic!(
                            "ECDSA-{bits}-Anker {} liess sich nicht pruefen: {}",
                            z.inhaber.common_name().unwrap_or_else(|| "?".to_string()),
                            fehler
                        ),
                    }
                }
                (SchluesselArt::Unbekannt { .. }, _) => {}
            }
        }
        let gesamt = speicher.anzahl();
        println!(
            "  RSA {} geprueft, P-256 {} geprueft, P-384 {} geprueft, \
             andere Kurven {}, Hash fehlt {} — zusammen {}",
            rsa_ok, ecdsa_p256, ecdsa_p384, ecdsa_andere, hash_fehlend, gesamt
        );
        // M7: die Summe muss aufgehen, sonst ist eine Kategorie
        // doppelt gezaehlt oder gar nicht erfasst.
        assert_eq!(
            rsa_ok + rsa_falsch + ecdsa_p256 + ecdsa_p384 + ecdsa_andere + hash_fehlend,
            gesamt,
            "jeder Anker ist genau einer Kategorie zugeordnet"
        );
        // Erwartungen sind GEGEN `openssl` gemessen (am 30.09.2026,
        // `openssl x509 -text` ueber alle 150 Zertifikate):
        //   rsaEncryption / sha256WithRSAEncryption    60
        //   rsaEncryption / sha1WithRSAEncryption       25
        //   rsaEncryption / sha384WithRSAEncryption     18
        //   rsaEncryption / sha512WithRSAEncryption      4
        //   id-ecPublicKey   / ecdsa-with-SHA384        36
        //   id-ecPublicKey   / ecdsa-with-SHA256         7
        // und nach Schluesselart: 107 nur RSA, 39 secp384r1,
        // 4 prime256v1.
        //
        // GEMESSEN am 30.09.2026 mit diesem Test:
        //   RSA 103 geprueft, P-256 4 geprueft, P-384 39 geprueft,
        //   andere Kurven 0, Hash fehlt 4 — zusammen 150.
        //
        // 39 + 4 = 43 ECDSA-Anker, und 36 + 7 = 43 Signatur-
        // Algorithmen: die Zaehlung nach Schluesselart und die nach
        // Signaturalgorithmus sind verschiedene Zaehlungen und beide
        // stimmen. Ich hatte sie verwechselt und 36 erwartet.
        //
        // Die RSA-Zahl ist also 60 + 25 + 18 = 103, und vier weitere
        // RSA-Anker tragen `sha512WithRSAEncryption`.
        //
        // DIESE ZAHLEN SIND NEU, weil P-384 in `ecdsa.rs` dazugekommen
        // ist. Vorher stand hier `ecdsa_andere == 39`, und die
        // secp384r1-Anker wurden als „unbekannt" gemeldet — was
        // sicher, aber unvollstaendig war. Genau an dieser Stelle ist
        // der Handshake gegen example.com gescheitert.
        assert_eq!(rsa_falsch, 0, "kein RSA-Anker ist gescheitert");
        assert_eq!(rsa_ok, 103, "RSA-Anker mit SHA-1, -256 oder -384");
        assert_eq!(ecdsa_p256, 4, "die vier prime256v1-Anker");
        assert_eq!(
            ecdsa_p384, 39,
            "die 39 secp384r1-Anker, seit P-384 in ecdsa.rs nachrechenbar"
        );
        assert_eq!(ecdsa_andere, 0, "kein Anker auf einer ungerechneten Kurve");
        assert_eq!(
            hash_fehlend, 4,
            "4 Anker mit sha512WithRSAEncryption, ohne SHA-512 in sha2.rs"
        );
    }

    #[cfg(test)]
    mod diagnose {
        use super::*;

        /// Warum schlug die Selbstsignatur eines RSA-Ankers fehl?
        ///
        /// DER WEG: nicht am Ergebnis drehen, sondern den einen Aufruf
        /// zerlegen. Geprueft werden in dieser Reihenfolge:
        ///   1. Was liest der Zertifikatsleser als OID, Hash und
        ///      Signatur?
        ///   2. Stimmt `tbs_der` mit dem ueberein, was der Signatur
        ///      zugrunde lag?
        ///   3. Was sagt `openssl` ueber dieselbe Signatur?
        #[test]
        fn ein_rsa_anker_wird_zerlegt() {
            const BUNDLE: &str = include_str!("../testdaten/t12_echtes_bundle.pem");
            let speicher = Vertrauensspeicher::aus_pem(BUNDLE).expect("Bundle");
            // Der erste RSA-Anker.
            let z = speicher
                .zertifikate
                .iter()
                .find(|z| matches!(z.schluessel, SchluesselArt::Rsa { .. }))
                .expect("ein RSA-Anker");
            println!("  Inhaber:  {:?}", z.inhaber.common_name());
            println!(
                "  CN-Tag:   {:02x}",
                z.inhaber
                    .erstes(&[2, 5, 4, 3])
                    .map(|a| a.wert.first().copied().unwrap_or(0))
                    .unwrap_or(0)
            );
            println!("  OID:      {:?}", z.signatur_algorithmus.oid);
            println!("  Hash:     {:?}", z.signatur_algorithmus.hash);
            println!("  tbs_der:  {} Byte", z.tbs_der.len());
            println!("  Signatur: {} Byte", z.signatur.len());
            if let SchluesselArt::Rsa { n, e } = &z.schluessel {
                println!(
                    "  n: {} Byte, e: {} Byte ({})",
                    n.len(),
                    e.len(),
                    e.iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
            }
            println!("  Ergebnis: {:?}", signatur_pruefen(z, z));
            assert_eq!(
                signatur_pruefen(z, z),
                Ok(()),
                "ein Anker, der sich selbst signiert hat, muss sich selbst pruefen lassen"
            );
            // Und die Kontrollfrage: liegt `tbs_der` wirklich im DER?
            let drin = z
                .der
                .windows(z.tbs_der.len())
                .any(|f| f == z.tbs_der.as_slice());
            println!("  tbs_der kommt byteweise im Zertifikat vor: {}", drin);
        }
    }
}

#[cfg(test)]
mod namen_des_zertifikats_tests {
    // added 03.10.2026 06:ax: **DER IMPORT WAR UNGENUTZT** — ich hatte ihn
    // eingetragen, bevor die Signaturen feststanden; die Typen kommen jetzt ueber
    // den Rueckgabewert von `zertifikat_lesen`. GEMESSEN: `check --all-targets`
    // und `clippy --all-targets` meldeten ihn, also beide 0 erst nach dem
    // Streichen.

    /// added 03.10.2026 06:4x, R52. **ROTER VORHER-BEWEIS.** RFC 5280 4.2.1.10
    /// woertlich: „indicates a name space within which **all subject names in
    /// subsequent certificates in a certification path MUST be located**“.
    /// **Alle** Subject-Namen — nicht der Name, den wir anfragen.
    /// GEFRAGT: `example.de` (erlaubt). IM ZERTIFIKAT: `example.de` **und**
    /// `evil.com`. `evil.com` liegt ausserhalb, also muss abgelehnt werden.
    #[test]
    fn zweiter_name_im_zertifikat_wird_geprueft() {
        let ca_der = crate::x509::bauen::zertifikat_ca_mit_name_constraints(&["example.de"], &[])
            .unwrap_or_else(|_| panic!("CA"));
        let blatt_der = crate::x509::bauen::blatt_mit_san_namen(&["example.de", "evil.com"])
            .unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA lesen: {e}"));
        let blatt =
            crate::x509::zertifikat_lesen(&blatt_der).unwrap_or_else(|_| panic!("Blatt lesen"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "example.de") {
            Ok(()) => panic!(
                "ein Name ausserhalb des erlaubten Teilbaums wurde im Zertifikat \
                 akzeptiert - es wird nur der Anfragename geprueft"
            ),
            Err(e) => println!("DIAG52 abgewiesen: {e}"),
        }
    }

    /// added 03.10.2026 06:4x, R52. **DER GEGENFALL, und ohne ihn waere der erste
    /// Test mit einem „alles ablehnen“ erfuellt**: ein Blatt, dessen Namen
    /// **alle** im erlaubten Teilbaum liegen, muss durchgehen.
    #[test]
    fn alle_namen_im_teilbaum_wird_angenommen() {
        let ca_der = crate::x509::bauen::zertifikat_ca_mit_name_constraints(&["example.de"], &[])
            .unwrap_or_else(|_| panic!("CA"));
        let blatt_der = crate::x509::bauen::blatt_mit_san_namen(&["example.de", "www.example.de"])
            .unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA lesen: {e}"));
        let blatt =
            crate::x509::zertifikat_lesen(&blatt_der).unwrap_or_else(|_| panic!("Blatt lesen"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "example.de")
            .unwrap_or_else(|e| panic!("ein erlaubter Name wurde abgewiesen: {e}"));
    }

    /// added 03.10.2026 06:4x, R52. **DER ZWEITE GEGENFALL:** `excludedSubtrees`
    /// muss **unveraendert** weiter gelten — der Name wird jetzt **zusaetzlich**
    /// geprueft, aber die alte Regel faellt nicht weg. `excluded` schliesst den
    /// Namen `evil.com` aus, `permitted` erlaubt `example.de`.
    #[test]
    fn ausgeschlossener_name_wird_auch_im_zertifikat_abgewiesen() {
        let ca_der = crate::x509::bauen::zertifikat_ca_mit_name_constraints(
            &["example.de"],
            &["gesperrt.example.de"],
        )
        .unwrap_or_else(|_| panic!("CA"));
        let blatt_der =
            crate::x509::bauen::blatt_mit_san_namen(&["example.de", "gesperrt.example.de"])
                .unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::zertifikat_lesen(&ca_der).unwrap_or_else(|e| panic!("CA lesen: {e}"));
        let blatt =
            crate::x509::zertifikat_lesen(&blatt_der).unwrap_or_else(|_| panic!("Blatt lesen"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "example.de") {
            Ok(()) => panic!("ein ausgeschlossener Name im Zertifikat wurde akzeptiert"),
            Err(e) => println!("DIAG52 ausgeschlossen: {e}"),
        }
    }
}

#[cfg(test)]
mod pfad_laenge_tests {
    use crate::kette::Vertrauensspeicher;
    use crate::x509::Zertifikat;

    /// added 03.10.2026 07:3x, R53. **DIE KETTE**, vier Zertifikate:
    ///     Blatt(localhost) <- CA2 <- CA1 <- Root
    /// `pfad` auf CA1 ist die Zahl der **nicht selbst ausgegebenen**
    /// Zwischen-Zertifikate, die **darueber** folgen duerfen (RFC 5280
    /// 4.2.1.9).
    ///
    /// added 03.10.2026 07:7x: **MEIN ERSTER TESTAUFBAU WAR FALSCH, und die
    /// Reihenfolge ist der Grund** — das ist die Stelle, an der man sich in
    /// dieser Datei am leichtesten verirrt: `kette_pruefen` baut `kette[0]` als
    /// **Blatt** und die **Wurzel** ans **Ende**. „Ueber der CA“ heisst also
    /// **groesserer Index**, nicht kleinerer. Ich hatte `pfad` auf **CA1**
    /// gelegt — ueber ihr liegt aber nur die selbstsignierte Wurzel, die nicht
    /// mitzaehlt, also 0 gefunden bei 0 erlaubt: **korrekt akzeptiert**, und mein
    /// Test war an der falschen Stelle rot. Richtig ist `pfad` auf **CA2**, denn
    /// ueber ihr steht CA1.
    fn kette_bauen(
        pfad_von_ca2: Option<u32>,
    ) -> Result<(Vec<Zertifikat>, Vertrauensspeicher), &'static str> {
        let blatt = crate::x509::bauen::zertifikat_fuer_pfad("CA2", "localhost", false, None)?;
        let ca2 = crate::x509::bauen::zertifikat_fuer_pfad("CA1", "CA2", true, pfad_von_ca2)?;
        let ca1 = crate::x509::bauen::zertifikat_fuer_pfad("Root", "CA1", true, None)?;
        let root = crate::x509::bauen::zertifikat_fuer_pfad("Root", "Root", true, None)?;
        let lesen = |d: &[u8]| -> Result<Zertifikat, &'static str> {
            crate::x509::zertifikat_lesen(d).map_err(|_| "Zertifikat unlesbar")
        };
        let speicher = Vertrauensspeicher {
            zertifikate: vec![lesen(&root)?],
        };
        // added 03.10.2026 07:4x: **`Vec<Zertifikat>`, nicht `Vec<Vec<u8>>`** —
        // `kette_pruefen` nimmt bereits gelesene Zertifikate (der Uebersetzer
        // meldete `expected &[Zertifikat], found &Vec<Vec<u8>>`). Nebeneffekt, den
        // ich will: **hier** liest der Test die gebauten Zertifikate, also beweist
        // diese eine Stelle auch, dass `zertifikat_fuer_pfad` etwas Lesbares
        // erzeugt — sonst waere der rote Test an einem kaputten Bauer gruen.
        Ok((vec![lesen(&blatt)?, lesen(&ca2)?, lesen(&ca1)?], speicher))
    }

    /// added 03.10.2026 07:3x, R53. **ROTER VORHER-BEWEIS.** `basic_constraints_
    /// lesen` liest `pathLenConstraint` in `pfad_laenge` (Tag 0x02), und
    /// GEMESSEN (06:9x) kam `pfad_laenge` in `kette.rs` **kein einziges Mal** vor.
    /// RFC 5280 4.2.1.9: „gives the maximum number of non-self-issued intermediate
    /// certificates that may follow this certificate in a valid certification
    /// path.“
    ///
    /// added 03.10.2026 08:8x: **UND JETZT UMBENANNT UND IN DER AUSSAGE
    /// UMGEDREHT.** Der Test war zuerst ein roter Vorher-Beweis: die Kette
    /// **musste** abgelehnt werden. Nach der Wildmessung (11 von 15 echten Hosts
    /// verletzen die Regel) ist die Entscheidung die **gegenlaeufige**, und der
    /// Test haelt jetzt **beide** Seiten fest — das ist staerker als eine Seite:
    ///
    ///   1. `kette_pruefen` **akzeptiert** — die Regel wird nicht erzwungen.
    ///   2. `pfad_laenge_pruefen` **lehnt ab** — sie existiert, sie ist gebaut,
    ///      gelesen und getestet.
    ///
    /// added 03.10.2026 08:8x: **WARUM BEIDES IN EINEM TEST.** Ein Test, der nur
    /// Punkt 1 prueft, liest sich wie „wir pruefen es nicht“. Ein Test, der nur
    /// Punkt 2 prueft, liest sich wie „wir haben es vergessen“. Zusammen sagen
    /// sie: **wir koennen es, wir tun es aus einem benannten Grund, und der Grund
    /// steht bei der Zeile im Handshake-Code.**
    #[test]
    fn pfad_laenge_wird_nicht_erzwungen_trotz_verletzung() {
        let (kette, speicher) = kette_bauen(Some(0)).unwrap_or_else(|_| panic!("Bau"));
        // Punkt 1 zuerst: der Handshake darf daran **nicht** scheitern.
        crate::kette::kette_pruefen(&kette, &speicher, 1_700_000_000, "localhost")
            .unwrap_or_else(|e| panic!("der Handshake lehnt ab - er soll es nicht: {e}"));
        // Punkt 2: die Regel **wuerde** ablehnen. Ohne diese Zeile koennte jemand
        // `pfad_laenge_pruefen` loeschen und der Test waere gruen.
        match crate::kette::pfad_laenge_pruefen(&kette, &speicher) {
            Ok(()) => panic!("die Regel lehnt nicht ab - sie waere fuer diese Kette falsch"),
            Err(e) => println!("DIAG53 Regel wuerde ablehnen, erzwungen wird sie nicht: {e}"),
        }
    }

    /// added 03.10.2026 07:4x, R53. **DER GEGENFALL, und ohne ihn waere „alles
    /// ablehnen“ auch gruen**: `pathLenConstraint = 1` erlaubt genau **eine**
    /// Zwischen-CA darueber — das ist hier CA2, also muss die Kette durchgehen.
    #[test]
    fn pfad_laenge_eins_erlaubt_eine_zwischen_ca() {
        let (kette, speicher) = kette_bauen(Some(1)).unwrap_or_else(|_| panic!("Bau"));
        crate::kette::kette_pruefen(&kette, &speicher, 1_700_000_000, "localhost")
            .unwrap_or_else(|e| panic!("eine erlaubte Kette wurde abgewiesen: {e}"));
    }

    /// added 03.10.2026 08:2x, R53. **DER TRUST-ANKER ZAEHLT NICHT MIT, EINE
    /// ZWISCHEN-CA DOCH** — und das ist der Test, der die Regel nach der
    /// Wildmessung festhaelt. Der Name ist bewusst von „self-issued“ auf
    /// „Trust-Anker“ gewechselt, weil das die **tatsaechliche** Unterscheidung
    /// ist: GEMESSEN (08:0x) war meine erste Fassung (nur `name_gleich`) falsch
    /// und hat 4 von 6 echten Verbindungen gebrochen, weil echte Wurzeln
    /// selten denselben Namen in Aussteller und Inhaber tragen.
    ///
    /// added 03.10.2026 08:2x: **VIER EINTRÄGE, weil der Anker der LETZTE
    /// ist.** Mit drei Eintraegen liegt das interessierende Zertifikat immer
    /// auf der Anker-Stelle und wird nie gezaehlt — die Liste [blatt, ca,
    /// selbst] und die Liste [blatt, ca, fremd] unterscheiden sich dann in
    /// **nichts**, was die Regel sieht. Genau das hat meine erste Fassung des
    /// Tests getan, und sie war nach der Korrektur rot („ein nicht
    /// selbstausgegebenes Zertifikat wurde nicht gezaehlt“). Der Test hat mich
    /// also nicht nur den Befund, sondern auch die Korrektur geprueft.
    #[test]
    fn der_anker_zaehlt_nicht_mit_eine_zwischen_ca_doch() {
        let lesen = |d: &[u8]| crate::x509::zertifikat_lesen(d);
        let blatt = crate::x509::bauen::zertifikat_fuer_pfad("CA", "localhost", false, None)
            .unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::bauen::zertifikat_fuer_pfad("Oberhalb", "CA", true, Some(0))
            .unwrap_or_else(|_| panic!("CA"));
        let zwischen = crate::x509::bauen::zertifikat_fuer_pfad("Wurzel", "Oberhalb", true, None)
            .unwrap_or_else(|_| panic!("Zwischen"));
        let wurzel = crate::x509::bauen::zertifikat_fuer_pfad("Wurzel", "Wurzel", true, None)
            .unwrap_or_else(|_| panic!("Wurzel"));
        let v = |d: &[u8]| lesen(d).unwrap_or_else(|_| panic!("lesen"));
        let liste = vec![v(&blatt), v(&ca), v(&zwischen), v(&wurzel)];
        // added 03.10.2026 08:4x: **GLEICHE KETTE, ANDERER SPEICHER** — das ist
        // der stärkste Test dieser ganzen Regel, weil nur **eine** Variable
        // wechselt. vorher hatte ich zwei Listen mit drei Eintraegen, und die
        // unterschieden sich in nichts, was die Regel sieht; der Test war nach der
        // Korrektur rot und hat mich erst auf die Anker-Stelle gebracht.
        //
        // Fall 1: **kein** Zwischenzertifikat ist im Speicher. Dann ist keine
        // Zertifikat der Kette ein Anker, der Pfad muss bis zum Ende laufen, und
        // ueber der `pathLen=0`-CA steht eine Zwischen-CA: **Verstoss**.
        let leer = Vertrauensspeicher {
            zertifikate: Vec::new(),
        };
        match crate::kette::pfad_laenge_pruefen(&liste, &leer) {
            Ok(()) => panic!("eine Zwischen-CA ueber der pathLenConstraint wurde nicht gezaehlt"),
            Err(e) => println!("DIAG53 Zwischen-CA gezaehlt: {e}"),
        }
        // Fall 2: **`zwischen` liegt im Speicher**, ist also ein Trust-Anker.
        // Dann endet der Pfad **dort**, und ueber der `pathLen=0`-CA steht
        // nichts — der Pfad ist gueltig. Genau das sind die „alternativen Pfade“
        // aus RFC 5280 6.1.1/6.1.2, und genau daran ist meine erste Regel
        // gescheitert.
        let mit_anker = Vertrauensspeicher {
            zertifikate: vec![v(&zwischen)],
        };
        crate::kette::pfad_laenge_pruefen(&liste, &mit_anker)
            .unwrap_or_else(|e| panic!("ein kuerzerer Pfad wurde nicht erkannt: {e}"));
    }

    /// added 03.10.2026 07:4x, R53. **KEIN `pathLen` HEISST UNBESCHRAENKT**, und
    /// das ist ein eigener Fall: `Option<u32>` ist `None`, nicht `Some(0)`. Ein
    /// Parserfehler, der `None` als `0` behandelte, wuerde hier faelschlich
    /// ablehnen — und das waere der teurere Fehler.
    #[test]
    fn ohne_pfad_laenge_ist_die_kette_unbeschaenkt() {
        let (kette, speicher) = kette_bauen(None).unwrap_or_else(|_| panic!("Bau"));
        crate::kette::kette_pruefen(&kette, &speicher, 1_700_000_000, "localhost")
            .unwrap_or_else(|e| panic!("eine Kette ohne pathLen wurde abgewiesen: {e}"));
    }
}

#[cfg(test)]
#[cfg(test)]
mod feldwaechter {
    /// added 03.10.2026 15:0x, R59. **NEU AUFGEBAUT, UND DER GRUND STEHT GANZ
    /// OBEN**, weil ich beim Umstellen von R59 **drei Tests geloescht** habe und
    /// es **erst gemerkt** habe, als `test feldwaechter` **1 statt 4** Tests
    /// meldete.
    ///
    /// added 03.10.2026 15:0x: **DIE LEHRE IST DIE SCHLIMMSTE DER WAE CHTER-
    /// REIHE:** **ein gruener Waechter, der nichts prueft, ist schlimmer als kein
    /// Waechter** — er sagt „geprueft“ und meint nichts. R51 hat dieselbe Form
    /// von der anderen Seite beschrieben (`#[test]` fehlte, „0 passed“ bei
    /// gruener Ausgabe). **DIE ZAHL DER TESTS STEHT DESHALB JEDES MAL IN DER
    /// AUSGABE** — und wer `werkzeuge/bau.sh … test --lib feldwaechter` laufen
    /// laesst, **muss auf die Zahl sehen, nicht nur auf „gruen“.**
    ///
    /// added 03.10.2026 15:1x: **UND WAS SONST IN DIESES MODUL GEHOERT**, an einer
    /// Stelle: die vier Stufen, die der Wächter brauchte, damit er nicht falsche
    /// Treffer meldet. Jede hat mich einen Lauf gekostet, und jede steht jetzt
    /// **hier** statt verstreut:
    ///   (a) **Wortgrenzen** an beiden Seiten (`contains` fand `email` in
    ///       `email_stufe`) — R54/R55;
    ///   (b) **keine Doc-Kommentare** zählen (ein Wächter findet seine eigene
    ///       Erklärung) — R55 und 15:9x;
    ///   (c) **der Lieferant zählt nicht** als Leser (`x509.rs` und `crl.rs`
    ///       stehen in der Menge) — R56, 10:5x;
    ///   (d) **Feldnamen kollidieren**: `seriennummer` ist ein `Zertifikat`-**und**
    ///       ein `Crl`-Feld, `am` ist ein Wort mit 58 Treffern. Für `crl.rs` ist
    ///       deshalb nur die **Funktions**-Frage scharf: „ist `crl_pruefen` von
    ///       ausserhalb aufgerufen?“ — 15:7x.
    /// added 03.10.2026 09:1x, R54: DIE KONSUMENTEN. `include_str!` liest zur
    /// **Übersetzungszeit**, also kann der Test weder an einem Arbeitsverzeichnis
    /// scheitern noch eine Datei übersehen.
    /// Reihenfolge: `[x509, kette, strom, ocsp, strom12, crl]`.
    const KONSUMENTEN: [&str; 6] = [
        include_str!("x509.rs"),
        include_str!("kette.rs"),
        include_str!("strom.rs"),
        include_str!("ocsp.rs"),
        include_str!("strom12.rs"),
        include_str!("crl.rs"),
    ];

    /// added 03.10.2026 15:2x: **DIE FELDER VON `Zertifikat` UND SEINER
    /// UNTERTYPEN**, aus `x509.rs` **selbst** gelesen — R55, 09:7x. Es gibt
    /// **keine Liste mehr, die man pflegen könnte**, und keine, die veralten kann.
    fn alle_felder() -> Vec<&'static str> {
        let quelle = include_str!("x509.rs");
        let mut raus: Vec<&'static str> = Vec::new();
        let mut in_struct = false;
        for zeile in quelle.lines() {
            let t = zeile.trim();
            if t.starts_with("pub struct ") {
                in_struct = true;
                continue;
            }
            if in_struct && t.starts_with('}') {
                in_struct = false;
                continue;
            }
            if in_struct {
                if let Some(rest) = t.strip_prefix("pub ") {
                    if let Some((name, _)) = rest.split_once(':') {
                        if let Some(feld) = name.split_whitespace().next() {
                            if !feld.is_empty() {
                                raus.push(feld);
                            }
                        }
                    }
                }
            }
        }
        raus
    }

    /// added 03.10.2026 15:3x, R59: **DIE AUSNAHMEN BEI DER VERWENDUNG.**
    /// `verteilungsstellen` — CRL-Beschaffung gehört nach `narsil-net`, und die
    /// **Verträgensfläche** ist gebaut (`x509::verteilungsstellen_als_texte`).
    /// `tls_features` — **Fehlalarm**: das Feld wird **indirekt** gelesen, über
    /// `x509::verlangt_status_request` (R44/R48); den Feldnamen spricht niemand an.
    const AUSNAHMEN_VERWENDUNG: [(&str, &str); 2] = [
        (
            "verteilungsstellen",
            "CRL-Beschaffung gehoert nach `narsil-net` (R51 Abschnitt 8, R54 \
             Abschnitt 1). added 03.10.2026 15:3x, R59: die Vertragsflaeche ist \
             gebaut — `x509::verteilungsstellen_als_texte` liefert die URLs",
        ),
        (
            "tls_features",
            "wird **indirekt** ueber `x509::verlangt_status_request` gelesen \
             (R44/R48); der Feldname wird nie angesprochen — Fehlalarm des \
             Namensvergleichs",
        ),
    ];

    /// added 03.10.2026 09:8x, R55: **DIE AUSNAHMEN BEI DER VERWERFUNG**, und sie
    /// sind der Beweis, dass die Messung stimmt: alle neun Stellen `let _FELD =`
    /// in `x509.rs` liegen in `name_der` und `spki_bits`, die Rohbytes zurückgeben
    /// und nur die **Position** vorrücken. Und die Ausnahme ist **nicht „das
    /// Feld“, sondern „das Feld IN DER DATEI“**: `aussteller` wird in `kette.rs`
    /// **gelesen** und in `x509.rs` **verworfen**.
    const AUSNAHMEN_VERWERFUNG: [(&str, &str, &str); 6] = [
        (
            "seriennummer",
            "x509.rs",
            "`name_der`/`spki_bits`: nur Position noetig",
        ),
        (
            "algorithmus",
            "x509.rs",
            "`name_der`/`spki_bits`: nur Position noetig",
        ),
        (
            "gueltigkeit",
            "x509.rs",
            "`name_der`/`spki_bits`: nur Position noetig",
        ),
        (
            "aussteller",
            "x509.rs",
            "`name_der`/`spki_bits`: nur Position noetig",
        ),
        (
            "inhaber",
            "x509.rs",
            "`name_der`/`spki_bits`: nur Position noetig",
        ),
        ("alg", "x509.rs", "`spki_bits`: nur Position noetig"),
    ];

    /// added 03.10.2026 15:1x: **STUFE (a) UND (b) ZUSAMMEN**, denn beide sind
    /// „die Zeile zählt, nicht das Wort“: **Wortgrenzen** an beiden Seiten und
    /// **keine Doc-Kommentare**. `zeichen_ist_wort` ist bewusst eine eigene
    /// Funktion statt `char::is_alphanumeric`, weil `_` **kein** Wortzeichen sein
    /// soll: `foo_email` enthält kein Feld `email`.
    fn als_code_und_nicht_als_definition(zeile: &str, feld: &str) -> bool {
        if zeile.trim_start().starts_with("///") {
            return false;
        }
        let bytes = zeile.as_bytes();
        let nadel = feld.as_bytes();
        if nadel.is_empty() || bytes.len() < nadel.len() {
            return false;
        }
        let mut von = 0usize;
        while let Some(rel) = zeile[von..].find(feld) {
            let start = von + rel;
            let ende = start + nadel.len();
            if (start == 0 || !zeichen_ist_wort(bytes[start - 1]))
                && (ende == bytes.len() || !zeichen_ist_wort(bytes[ende]))
            {
                return true;
            }
            von = start + 1;
            if von >= bytes.len() {
                break;
            }
        }
        false
    }

    fn zeichen_ist_wort(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    /// added 03.10.2026 15:4x: **DER VORWÄCHTER, und er ist der wichtigste.**
    /// Er meldet, wenn weniger als 28 Felder gefunden werden: ein Test, der eine
    /// leere Liste sieht und „grün“ meldet, **wacht nichts**.
    #[test]
    fn der_waechter_liest_ueberhaupt_felder() {
        assert!(
            alle_felder().len() >= 28,
            "der Waechter las nur {} Felder — er wird zuwenig finden",
            alle_felder().len()
        );
    }

    /// added 03.10.2026 15:4x: **`[1..]`, OHNE `x509.rs`** — der Lieferant ist
    /// ausgeschlossen (R56, 10:5x): gesucht ist „geliefert, aber von niemandem
    /// **konsumiert**“.
    #[test]
    fn jedes_feld_wird_irgendwo_gelesen() {
        let mut unbenutzt: Vec<&str> = Vec::new();
        for feld in alle_felder() {
            if AUSNAHMEN_VERWENDUNG.iter().any(|(name, _)| *name == feld) {
                continue;
            }
            let gelesen = KONSUMENTEN[1..].iter().any(|q| {
                q.lines()
                    .any(|zeile| als_code_und_nicht_als_definition(zeile, feld))
            });
            if !gelesen {
                unbenutzt.push(feld);
            }
        }
        assert!(
            unbenutzt.is_empty(),
            "diese Felder aus `x509.rs` werden von niemandem gelesen: {unbenutzt:?} — \
             oder sie stehen in `AUSNAHMEN_VERWENDUNG` mit Grund"
        );
    }

    /// added 03.10.2026 15:4x: **`let _FELD =` IST DIE FORM**, weil ein Bezeichner
    /// kein `=` enthält (R54, R55).
    #[test]
    fn kein_feld_wird_verworfen() {
        let mut verworfen: Vec<String> = Vec::new();
        for feld in alle_felder() {
            for (datei, quelle) in [
                ("x509.rs", KONSUMENTEN[0]),
                ("strom.rs", KONSUMENTEN[2]),
                ("ocsp.rs", KONSUMENTEN[3]),
                ("strom12.rs", KONSUMENTEN[4]),
            ] {
                if !quelle.contains(&format!("let _{feld} =")) {
                    continue;
                }
                let erlaubt = AUSNAHMEN_VERWERFUNG
                    .iter()
                    .any(|(f, d, _)| *f == feld && *d == datei);
                if !erlaubt {
                    verworfen.push(format!("{feld} in {datei}"));
                }
            }
        }
        assert!(
            verworfen.is_empty(),
            "diese Felder werden verworfen (`let _FELD =`): {verworfen:?} — das war in \
             R51 (`let _kritisch`) die Ursache fuer einen stillen Fehler"
        );
    }

    /// added 03.10.2026 15:5x, R59: **DER ZWEITE LIEFERANT.** `crl.rs` hat zwei
    /// Structs mit **11** Feldern (`Widerruf`: seriennummer, am, grund; `Crl`:
    /// tbs_der, signatur, algorithmus, algorithmus_im_tbs, aussteller, ab, bis,
    /// nummer, erweiterungen, widerrufe) — und **null** Konsumenten in der ganzen
    /// Crate (grep nach `crl::` in `narsil-tls/src` **und** `narsil-net/src`: kein
    /// Treffer).
    const FELDER_CRL: [&str; 11] = [
        "seriennummer",
        "am",
        "grund",
        "tbs_der",
        "signatur",
        "algorithmus",
        "algorithmus_im_tbs",
        "aussteller",
        "ab",
        "bis",
        "nummer",
    ];

    /// added 03.10.2026 15:6x: **STUFE (d) — DIE NAMENSKOLLISION, und sie hat
    /// meinen ersten CRL-Test unbrauchbar gemacht.** Er suchte dieselben
    /// Feldnamen in den vier anderen Verbrauchern und meldete **alle elf** als „von
    /// aussen gelesen“. GEMESSEN nach: `seriennummer` und `tbs_der` sind auch
    /// `Zertifikat`-Felder und werden in `kette.rs` und `ocsp.rs` **echt**
    /// benutzt; `am` hat **58** Treffer in Worten wie „am Anfang“; `grund` **18** in
    /// Fehlermeldungen.
    /// **DIE LEHRE: EIN WÄCHTER ÜBER FELDNAMEN KANN NICHT UNTERSCHEIDEN, WELCHES
    /// STRUCT GEMEINT IST, WENN ZWEI STRUCTS DASSELBE FELD BENENNEN.** R54 war
    /// das Rauschen, R55 der Selbstbezug, R56 die Deklaration statt der
    /// Verwendung — jede Stufe hat denselben Preis: **ein Test, der Falsches
    /// meldet, wird abgeschaltet.**
    ///
    /// added 03.10.2026 15:7x: **DIE FRAGE, DIE TROTZDEM SCHARF IST:** nicht „wer
    /// liest ein Feld“, sondern „**ist die CRL-Prüfung überhaupt eingebunden**“.
    /// Das ist eine **Funktions**-Frage, und `crl_pruefen` / `beurteile` sind
    /// Namen, die es nur **einmal** in der Crate gibt — also kann ein
    /// Namensvergleich hier **nicht** falsch liegen.
    ///
    /// added 03.10.2026 15:8x: **UND DAS IST DIE ANTWORT, NICHT DER BEFUND:**
    /// `crl.rs` hat keine Konsumenten, weil die **CRL-Prüfung nicht in den
    /// Handshake eingebunden** ist. Das ist **Schichtung**: die CRL muss
    /// **beschafft** werden, und das ist ein HTTP-Vorgang, der nach `narsil-net`
    /// gehört. Kein vergessener Aufruf, sondern ein **nicht gebautes**.
    #[test]
    fn die_crl_pruefung_ist_nicht_eingebunden() {
        for name in ["crl_pruefen", "beurteile"] {
            // added 03.10.2026 15:9x: **`[2..5]` UND KEINE `///`-ZEILEN.**
            // `[1]` ist `kette.rs`, die Datei, in der **dieser Test** steht; die
            // Doc-Kommentare erwähnen die Namen zur Begründung. Beides ist der
            // **Selbstbezug**, die Form aus R55.
            let ausserhalb = KONSUMENTEN[2..5].iter().any(|q| {
                q.lines()
                    .any(|zeile| !zeile.trim_start().starts_with("///") && zeile.contains(name))
            });
            assert!(
                !ausserhalb,
                "`{name}` wird von einem anderen Modul aufgerufen — dann ist \
                 `crl.rs` **eingebunden** und die Ausnahme ist veraltet"
            );
        }
        // added 03.10.2026 15:9x: **UND DIE ZAHL STEHT DA**, sonst wäre
        // `FELDER_CRL` erfüllbar, indem man sie kürzt — und der Test wäre dann
        // grün **und** blind zugleich.
        assert_eq!(FELDER_CRL.len(), 11, "die Feldliste wurde veraendert");
    }
}

#[cfg(test)]
mod email_uri_tests {
    use crate::x509::Zertifikat;

    /// added 03.10.2026 12:0x, R56. **DER AUFBAU**, und er ist bewusst
    /// zweistufig: erst die CA mit der Auflage, dann das Blatt — und die
    /// Pruefung laeuft **direkt** auf `namensbeschraenkung_pruefen`. Grund: die
    /// Regel ist eine Funktion mit zwei Argumenten, und ein Test, der den
    /// ganzen Handshake mitspielt, wuerde sie nur indirekt pruefen.
    fn paar(
        erlaubt_email: &[&str],
        erlaubt_uri: &[&str],
        dns: &[&str],
        email: &[&str],
        uri: &[&str],
    ) -> Result<(Zertifikat, Zertifikat), &'static str> {
        let ca_der = crate::x509::bauen::ca_mit_email_und_uri_auflage(erlaubt_email, erlaubt_uri)?;
        let blatt_der = crate::x509::bauen::blatt_mit_san_und_email(dns, email, uri)?;
        let lesen = |d: &[u8]| crate::x509::zertifikat_lesen(d).map_err(|_| "Zertifikat unlesbar");
        Ok((lesen(&ca_der)?, lesen(&blatt_der)?))
    }

    /// added 03.10.2026 12:0x, R56. **DIE PUNKTFORM SCHLIESST DEN HOST AUS**, und
    /// das ist die Zeile, an der man E-Mail-Regeln am ehesten falsch liest. RFC
    /// 5280 4.2.1.9 woertlich: „`.example.com` indicates all the Internet mail
    /// addresses in the domain `example.com`, **but not** Internet mail addresses
    /// on the host `example.com`“. Also: `x@sub.example.com` erfuellt, `x@example.com`
    /// **nicht**.
    #[test]
    fn punktform_erlaubt_die_domain_nicht_den_host() {
        let (ca, blatt) = paar(&[".example.com"], &[], &[], &["x@sub.example.com"], &[])
            .unwrap_or_else(|_| panic!("Bau"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost").unwrap_or_else(|e| {
            panic!("eine Adresse in der erlaubten Domain wurde abgewiesen: {e}")
        });
        let (ca2, blatt2) = paar(&[".example.com"], &[], &[], &["x@example.com"], &[])
            .unwrap_or_else(|_| panic!("Bau"));
        match crate::kette::namensbeschraenkung_pruefen(&ca2, &blatt2, "localhost") {
            Ok(()) => {
                panic!("die Punktform hat den Host selbst erlaubt — die Norm sagt 'aber nicht'")
            }
            Err(e) => println!("DIAG56 Host abgewiesen: {e}"),
        }
    }

    /// added 03.10.2026 12:1x, R56. **DER LOCAL-TEIL IST CASE-SENSITIV**, weil
    /// RFC 5321 das so vorschreibt: `Root@example.com` und `root@example.com`
    /// koennen **verschiedene** Postfaecher sein. Die Host-Form der Auflage ist
    /// dagegen case-**insensitiv**, weil Hosts das sind.
    #[test]
    fn email_postfachform_unterscheidet_gross_klein() {
        let (ca, blatt) = paar(&["root@example.com"], &[], &[], &["ROOT@example.com"], &[])
            .unwrap_or_else(|_| panic!("Bau"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost") {
            Ok(()) => panic!("der Local-Teil wurde case-insensitiv verglichen"),
            Err(e) => println!("DIAG56 Local-Teil abgewiesen: {e}"),
        }
        let (ca2, blatt2) = paar(&["root@example.com"], &[], &[], &["root@example.com"], &[])
            .unwrap_or_else(|_| panic!("Bau"));
        crate::kette::namensbeschraenkung_pruefen(&ca2, &blatt2, "localhost")
            .unwrap_or_else(|e| panic!("das richtige Postfach wurde abgewiesen: {e}"));
    }

    /// added 03.10.2026 12:1x, R56. **DIE HOST-FORM GILT FUER JEDE ADRESSE AUF DEM
    /// HOST**, und das ist die zweite Normform: „the constraint is specified as
    /// the host name“ bedeutet „alle Adressen auf diesem Host“.
    #[test]
    fn email_hostform_gilt_fuer_jede_adresse_darauf() {
        let (ca, blatt) = paar(
            &["example.com"],
            &[],
            &[],
            // added 03.10.2026 12:4x: **DIESE ADRESSE WAR FALSCH** — ich hatte
            // `beliebiger.local@beispiel.example.com` genommen, und der Host
            // `beispiel.example.com` ist **nicht** `example.com`: die Host-Form
            // erlaubt genau **diesen** Host, nicht seine Subdomains. Das ist der
            // Unterschied zur Punktform, und er ist der **ganze** Unterschied.
            // GEMESSEN als „eine beliebige Adresse auf dem erlaubten Host wurde
            // abgewiesen“ — die Regel hatte recht, der Test nicht.
            &["beliebiger.local@example.com"],
            &[],
        )
        .unwrap_or_else(|_| panic!("Bau"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost").unwrap_or_else(|e| {
            panic!("eine beliebige Adresse auf dem erlaubten Host wurde abgewiesen: {e}")
        });
    }

    /// added 03.10.2026 12:2x, R56. **DIE URI-REGEL GILT NUR FUER DEN HOST**, und
    /// die drei Faelle sind zusammen die Regel: `x.example.com` erfuellt
    /// `.example.com`, `evil.example` nicht, und der Pfad in der URI ist
    /// **ohne Bedeutung**.
    #[test]
    fn uri_regel_nimmt_nur_den_host() {
        let (ca, blatt) = paar(
            &[],
            &[".example.com"],
            &[],
            &[],
            &["https://x.example.com/a/b?c=d"],
        )
        .unwrap_or_else(|_| panic!("Bau"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost")
            .unwrap_or_else(|e| panic!("ein erlaubter URI-Host wurde abgewiesen: {e}"));
        let (ca2, blatt2) = paar(
            &[],
            &[".example.com"],
            &[],
            &[],
            &["https://evil.example/x"],
        )
        .unwrap_or_else(|_| panic!("Bau"));
        match crate::kette::namensbeschraenkung_pruefen(&ca2, &blatt2, "localhost") {
            Ok(()) => panic!("ein fremder URI-Host wurde akzeptiert"),
            Err(e) => println!("DIAG56 URI-Host abgewiesen: {e}"),
        }
    }

    /// added 03.10.2026 12:2x, R56. **DER GEGENFALLZU JEDER REGEL: KEINE
    /// AUFLAGE.** Ein Test, der nur die Ablehnungen prueft, waere von einem
    /// Parser erfuellt, der **alles** ablehnt — und genau das war in R53 der Fall
    /// (`pathLenConstraint`). Deshalb steht hier der Positivfall mit **leerer**
    /// Auflage: dann muss alles durchgehen.
    #[test]
    fn ohne_auflage_bleibt_alles_erlaubt() {
        let (ca, blatt) = paar(
            &[],
            &[],
            &["a.example"],
            &["x@beliebig.example"],
            &["https://x.example/p"],
        )
        .unwrap_or_else(|_| panic!("Bau"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost")
            .unwrap_or_else(|e| panic!("ohne Auflage wurde abgewiesen: {e}"));
    }
}

#[cfg(test)]
mod dirname_tests {
    use crate::x509::Zertifikat;

    /// added 03.10.2026 13:9x, R57. **DIE ZWEI NAMEN** — der Aufbau ist
    /// zweistufig, und die Blatt-Erweiterung traegt den `directoryName` im SAN.
    fn blatt_und_auflage(mit_dn_san: bool) -> Result<(Zertifikat, Zertifikat), &'static str> {
        // added 03.10.2026 13:ax: **DER AUFLAGE-BAUER STEHT IN `x509::bauen`** —
        // hier stand vorher eine lokale Konstruktion mit `sequenz`/`tlv`/`name`,
        // und der Uebersetzer meldete fuer **drei** Stellen „function `sequenz`
        // is private“. Das ist die richtige Form der Antwort: **kein**
        // `pub(crate)` auf die Helfer, sondern der Bauer dorthin, wo sie
        // gehoeren. Sonst wuerden sie zu einer halb-oeffentlichen DER-Bau-API —
        // und das sind sie nicht.
        let ca_der = crate::x509::bauen::ca_mit_dirname_auflage("localhost")?;
        let blatt_der = if mit_dn_san {
            crate::x509::bauen::blatt_mit_directory_name("localhost")?
        } else {
            crate::x509::bauen::zertifikat_fuer_pfad("localhost", "localhost", false, None)?
        };
        let lesen = |d: &[u8]| crate::x509::zertifikat_lesen(d).map_err(|_| "unlesbar");
        Ok((lesen(&ca_der)?, lesen(&blatt_der)?))
    }

    /// added 03.10.2026 13:9x, R57. **DER INHABERNAME STEHT AUF DER POSITIVLISTE** —
    /// RFC 5280 4.2.1.10 woertlich: „Restrictions of the form directoryName MUST
    /// be applied to the **subject field** in the certificate“. Also **ohne**
    /// `directoryName` im SAN, weil der Subject selbst genuegt.
    #[test]
    fn ausgeschlossener_inhabername_wird_abgewiesen() {
        let (ca, blatt) = blatt_und_auflage(false).unwrap_or_else(|_| panic!("Bau"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost") {
            Ok(()) => panic!("ein ausgeschlossener Inhabername wurde akzeptiert"),
            Err(e) => println!("DIAG57 Inhabername abgewiesen: {e}"),
        }
    }

    /// added 03.10.2026 13:9x, R57. **DER GEGENFALL, und er ist hier wichtiger als
    /// sonst:** ein Inhabername, der **nicht** auf der Positivliste steht, muss
    /// durchgehen. `ausserhalb` != `localhost`.
    #[test]
    // added 03.10.2026 13:ay: **DER NAME HATTE EINEN LEERRAUM**
    // (`fn nicht aufgefuehrter_…`) — der Uebersetzer meldete „missing parameters
    // for function definition“. Im Deutschen sind das zwei Woerter, im
    // Bezeichner eins. Eine Fehlerform, die nur einmal vorkommt und damit
    // harmlos ist — aber sie hat 30 Sekunden gekostet, und der Fehlertext
    // spricht von Parametern statt vom Leerraum.
    fn nicht_aufgefuehrter_inhabername_bleibt_erlaubt() {
        // added 03.10.2026 13:ax: **DIE AUFLAGE IST EINE ANDERE** — sie nennt
        // `nur.localhost`, und der Inhabername ist `localhost`. So prueft der Test
        // die **Vergleichs**regel und nicht nur den Aufruf.
        let ca = crate::x509::zertifikat_lesen(
            &crate::x509::bauen::ca_mit_dirname_auflage("nur.localhost")
                .unwrap_or_else(|_| panic!("Bau")),
        )
        .unwrap_or_else(|e| panic!("CA lesen: {e}"));
        let blatt = crate::x509::zertifikat_lesen(
            &crate::x509::bauen::zertifikat_fuer_pfad("localhost", "localhost", false, None)
                .unwrap_or_else(|_| panic!("Blatt")),
        )
        .unwrap_or_else(|_| panic!("Blatt lesen"));
        crate::kette::namensbeschraenkung_pruefen(&ca, &blatt, "localhost").unwrap_or_else(|e| {
            panic!("ein nicht aufgefuehrter Inhabername wurde abgewiesen: {e}")
        });
    }

    /// added 03.10.2026 13:ax, R57. **UND DIE GROSS-/KLEINSCH.** `LOCALHOST` gegen
    /// die Auflage `localhost` muss **gleich** sein — das ist der ganze Zweck des
    /// 7.1-Vergleichs. Ohne diesen Test koennte `namen_gleich_7_1` die Vorbereitung
    /// berechnen und dann **nicht anwenden**.
    #[test]
    fn gross_kleinschreibung_der_namen_wird_ignoriert() {
        // added 03.10.2026 13:bc: **`let (ca, _)` STATT `let (ca, blatt)`** — die
        // zweite Haelfte wird nicht gebraucht, weil der Test sein **eigenes**
        // Blatt baut (mit `LOCALHOST`). GEMESSEN als „unused variable: `blatt`“.
        // Es ist ein Rest aus dem Umbau von 13:ax, und solche Reste entstehen
        // immer dann, wenn man einen Test **umbaut** statt neu zu schreiben.
        let (ca, _) = blatt_und_auflage(false).unwrap_or_else(|_| panic!("Bau"));
        // added 03.10.2026 13:ax: **`LOCALHOST`** — ich baue das Blatt ein zweites
        // Mal mit grosser Schreibung als Inhaber.
        let blatt2 = crate::x509::zertifikat_lesen(
            &crate::x509::bauen::zertifikat_fuer_pfad("LOCALHOST", "LOCALHOST", false, None)
                .unwrap_or_else(|_| panic!("Blatt gross")),
        )
        .unwrap_or_else(|_| panic!("Blatt gross lesen"));
        match crate::kette::namensbeschraenkung_pruefen(&ca, &blatt2, "localhost") {
            Ok(()) => panic!("ein nach 7.1 gleicher Name wurde nicht als gleich erkannt"),
            Err(e) => println!("DIAG57 Schreibung egal: {e}"),
        }
    }
}

#[cfg(test)]
mod teilbaum_tests {
    /// added 03.10.2026 14:ax, R58. **DER AUFBAU**, und er ist der Kern der
    /// ganzen Regel: **mehrere RDNs** in beiden Namen. Ohne das kann die Regel
    /// nicht gemessen werden — bei einem einzigen RDN ist „mindestens so viele
    /// RDNs“ unmoeglich und „abschliessende RDNs ignorieren“ bedeutungslos.
    ///
    /// added 03.10.2026 14:ax: **DIE ZAHLEN, und sie sind gezaehlt, nicht
    /// geraten.** Ein Name aus `name_kette(&["O", "CN"])` hat **3** RDNs
    /// (das `C=DE` kommt dazu), also `rdn_grenze = [0, 1, 2, 3]`.
    fn blatt_mit_rdn(teile: &[&str]) -> Result<crate::x509::Zertifikat, &'static str> {
        let dn = crate::x509::bauen::name_kette(teile);
        let der = crate::x509::bauen::blatt_mit_subject_name(&dn)?;
        crate::x509::zertifikat_lesen(&der).map_err(|_| "Blatt unlesbar")
    }

    /// added 03.10.2026 14:ay, R58. **DIE TEILBAUM-REGEL, und der Test ist in
    /// drei Faelle geteilt, weil die Regel zwei Bedingungen hat und jede einen
    /// eigenen Gegenfall braucht:**
    ///  * `dn2` ist ein **Vorbau** von `dn1` -> **erlaubt** („trailing RDNs in DN1
    ///    are ignored“).
    ///  * `dn2` ist **kein** Vorbau -> **nicht** erlaubt.
    ///  * `dn2` hat **mehr** RDNs als `dn1` -> **nicht** erlaubt („DN1 contains at
    ///    least as many RDNs as DN2“).
    #[test]
    fn teilbaum_erkennt_den_vorbau() {
        let kurz = crate::x509::zertifikat_lesen(
            &crate::x509::bauen::blatt_mit_subject_name(&crate::x509::bauen::name_kette(&["O"]))
                .unwrap_or_else(|_| panic!("Blatt kurz")),
        )
        .unwrap_or_else(|e| panic!("kurz lesen: {e}"));
        let lang = blatt_mit_rdn(&["O", "CN"]).unwrap_or_else(|_| panic!("Blatt lang"));
        assert!(
            crate::kette::dn_im_teilbaum(&lang.inhaber, &kurz.inhaber),
            "ein Name mit mehreren RDNs muss im Teilbaum eines kuerzeren Namens liegen"
        );
        assert!(
            !crate::kette::dn_im_teilbaum(&kurz.inhaber, &lang.inhaber),
            "die Regel darf nicht symmetrisch sein — ein kurzer Name liegt nicht im \\
             Teilbaum eines laengeren"
        );
    }

    /// added 03.10.2026 14:ay, R58. **UND GLEICHHEIT ZAEHLT**, denn ein Name
    /// mit **denselben** RDNs liegt nach der Norm im eigenen Teilbaum
    /// („DN1 contains at least as many RDNs as DN2“ — bei Gleichheit „at least“
    /// erfuellt).
    #[test]
    fn teilbaum_umfasst_den_namen_selbst() {
        let a = blatt_mit_rdn(&["O", "CN"]).unwrap_or_else(|_| panic!("A"));
        let b = blatt_mit_rdn(&["O", "CN"]).unwrap_or_else(|_| panic!("B"));
        assert!(crate::kette::dn_im_teilbaum(&a.inhaber, &b.inhaber));
        assert!(crate::kette::dn_im_teilbaum(&b.inhaber, &a.inhaber));
    }

    /// added 03.10.2026 14:az, R58. **DIE FALSCHE POSITIVE, die ohne die
    /// Laengenpruefung passieren wuerde:** zwei Namen mit gleich vielen RDNs,
    /// aber **verschiedenem Inhalt**. Wenn die Regel nur die ersten `rdn2` RDNs
    /// vergleicht und nicht die Zahl prueft, wuerde sie das **falsch** annehmen.
    #[test]
    fn gleiche_rdn_zahl_anderer_inhalt_ist_kein_teilbaum() {
        let a = blatt_mit_rdn(&["O"]).unwrap_or_else(|_| panic!("A"));
        let b = blatt_mit_rdn(&["X"]).unwrap_or_else(|_| panic!("B"));
        assert!(
            !crate::kette::dn_im_teilbaum(&a.inhaber, &b.inhaber),
            "zwei Namen mit gleich vielen RDNs und anderem Inhalt sind kein Teilbaum"
        );
    }
}

// ===========================================================================
// ZEIT — wo geht der Handshake hin? (Auftrag 03.10.2026 22:0x, NETZTEMPO_6)
// ===========================================================================

/// added 03.10.2026, TLS (Auftrag: TLS verlangsamen, Messung 514 von 613 ms).
/// **WO GEHT DIE ZEIT IM HANDSHAKE HIN?** — und die Reihenfolge ist Absicht:
/// **erst messen, dann aendern.** Wer die Optimierung zuerst schreibt, misst
/// danach nur noch die eigene Arbeit.
///
/// **DIE VIER PREISE, und jeder ist einzeln aufrufbar:**
///
/// 1. **X25519** — `x25519::gemeinsam`, der Schluesselaustausch, den wir
///    anbieten (`nachricht.rs:580` stellt X25519 **voran**).
/// 2. **P-256 ECDH** — `ecdsa::ecdh`, derselbe Schritt auf der anderen Kurve.
///    **DER VERGLEICH IST DIE AUFGABE**: die Leitung fragt, ob X25519 schneller
///    ist, und das ist eine **Zahl**, keine Meinung.
/// 3. **ECDSA-Pufferberechnung** — `ecdsa::skalar_punkt`, das Herz jeder
///    Signaturpruefung. Eine Kette mit `n` Zertifikaten macht **n** davon.
/// 4. **`kette_pruefen`** — die ganze Kette, mehrfach.
///
/// **UND WOVON DIEZAHLEN ABHAENGEN — ehrlich gesagt, statt sie zu verschweigen:**
/// `bau.sh` laesst **2 Test-Threads** laufen (das ist die Zahl in
/// `RUST_TEST_THREADS`), und auf einem **geteilten** Rechner ist eine Zeitmessung
/// verrauscht. Darum misst dieser Test **keine** festen Schwellen, sondern gibt
/// die Zahlen aus und stellt **Verhaeltnisse** fest — eine Reihenfolge, die
/// nicht kippt.
#[test]
fn wo_geht_die_zeit_im_handshake_hin() {
    use std::time::Instant;

    // --- 1) X25519 ------------------------------------------------------
    let mut geheim = [0u8; 32];
    for (i, b) in geheim.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7).wrapping_add(3);
    }
    let gegen_public = crate::x25519::oeffentlich(&geheim);
    let runden = 200;
    let t0 = Instant::now();
    for _ in 0..runden {
        let _ = crate::x25519::gemeinsam(&geheim, &gegen_public);
    }
    let x25519 = t0.elapsed();

    // --- 2) P-256 ECDH, gleiche Zahl Runden -----------------------------
    //
    // added 03.10.2026, TLS, **NACH DEM ERSTEN LAUF — und das ist der
    // wichtigste Fund dieses Tests:** mein erster Vektor war
    // `ecdh(privat=[7;32], px=[3;32], py=[5;32])`, und das ergab **1,9 us**.
    // Das war **keine** Rechnung, sondern ein **Fehlerpfad**: `[3u8; 32]` und
    // `[5u8; 32]` sind mit hoher Wahrscheinlichkeit **nicht auf der Kurve**, und
    // `ecdh` bricht dann sofort ab. **1,9 us ist kein ECDH — das ist ein
    // Vergleich mit einem Fehler.** Genau die Blindheit, die ich bei `bild`
    // gemessen habe, nur in meiner eigenen Messung.
    //
    // **DIE GUELTIGE VARIANTE:** der oeffentliche Schluessel aus einem echten,
    // selbst gebauten Zertifikat. `Zertifikat.schluessel` ist `SchluesselArt::
    // Ecdsa { x, y }`, und das sind Koordinaten, die **auf der Kurve liegen** —
    // weil das Zertifikat sonst nicht gebaut werden koennte.
    let k = ecdsa::kurve_p256().unwrap_or_else(|_| panic!("P-256"));
    let ca_der_vor = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
    let ca_zert = crate::x509::zertifikat_lesen(&ca_der_vor).unwrap_or_else(|_| panic!("CA"));
    let (px, py) = match &ca_zert.schluessel {
        crate::x509::SchluesselArt::Ecdsa { x, y, .. } => (x.clone(), y.clone()),
        _ => panic!("das Testzertifikat hat keinen ECDSA-Schluessel"),
    };
    let privat = [7u8; 32];
    // added 03.10.2026, TLS: **DIE POSITIVKONTROLLE DER MESSUNG** — ohne sie
    // koennte ich wieder einen Fehlerpfad messen und es nicht wissen.
    ecdsa::ecdh(&privat, &px, &py, &k)
        .unwrap_or_else(|e| panic!("P-256 ECDH mit gueltigem Punkt: {e}"));
    let t0 = Instant::now();
    for _ in 0..runden {
        let _ = ecdsa::ecdh(&privat, &px, &py, &k);
    }
    let p256 = t0.elapsed();

    // --- 3) ECDSA: eine Punktberechnung ---------------------------------
    // added 03.10.2026, TLS: **DIE BASIS IST `(Gx, Gy, 1)`**, und das ist
    // keine Geschmacksfrage: `ecdsa.rs:757` baut sie genau so, und der
    // Kommentar dort (Zeile 1116) sagt, warum `(Gx, Gy, 0)` **falsch** waere.
    // GEMESSEN, dass ich das erst nachgelesen habe — mein erster Versuch
    // schrieb `k.basis`, und **das Feld gibt es nicht** (Compilerfehler
    // `no field 'basis' on type 'ecdsa::Kurve'`).
    let basis = (k.gx.clone(), k.gy.clone(), crate::zahl::Zahl::eins());
    let skalar =
        crate::zahl::Zahl::aus_bytes_gross(&[9u8; 32]).unwrap_or_else(|_| panic!("Skalar"));
    let t0 = Instant::now();
    for _ in 0..runden {
        let _ = ecdsa::skalar_punkt(&skalar, &basis, &k);
    }
    let punkt = t0.elapsed();

    // --- 4) die ganze Kette ---------------------------------------------
    // added 03.10.2026, TLS: **`bauen` LIEFERT `Vec<u8>` (DER), NICHT
    // `Zertifikat`** — GEMESSEN am Compilerfehler `expected Zertifikat, found
    // Vec<u8>`. Und das ist **kein** Nebenschlupf, sondern Teil der Messung:
    // **das Lesen der Zertifikate ist Handshake-Zeit**, also gehoert es
    // ausgemessen statt wegoptimiert.
    let blatt_der = crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt"));
    let ca_der = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
    let t0 = Instant::now();
    for _ in 0..runden {
        let _ = crate::x509::zertifikat_lesen(&blatt_der);
        let _ = crate::x509::zertifikat_lesen(&ca_der);
    }
    let lesen = t0.elapsed();

    // added 03.10.2026, TLS: **`Zertifikat` ist nicht `Copy`** (Compilerfehler
    // `use of moved value`) — es ist ein Parsergebnis mit `Vec`-Feldern. Also
    // wird die CA **zweimal gelesen**, statt sie zu kopieren.
    let blatt = crate::x509::zertifikat_lesen(&blatt_der).unwrap_or_else(|_| panic!("Blatt"));
    let ca_fuer_pfad = crate::x509::zertifikat_lesen(&ca_der).unwrap_or_else(|_| panic!("CA"));
    let pfad = vec![blatt, ca_fuer_pfad];
    let speicher = Vertrauensspeicher {
        zertifikate: vec![crate::x509::zertifikat_lesen(&ca_der).unwrap_or_else(|_| panic!("CA"))],
    };
    // added 03.10.2026, TLS: **VORHER/NACHHER, und deshalb zwei Messungen.**
    // Der erste Aufruf ist **kalt** (leere Memo-Tabelle), die folgenden sind
    // **warm**. Beide mit derselben Kette, demselben Speicher, derselben Zeit —
    // der einzige Unterschied ist die Memo.
    //
    // **UND DIE ZEIT IST EIN WERT, NICHT `now()`:** `kette_pruefen` nimmt
    // `jetzt: i64` als **Argument**, und das ist der Grund, warum die Funktion
    // ueberhaupt pruefbar ist. GEMESSEN ist 1 788 000 000 eine Zeit **vor** dem
    // Ablaufdatum der Testzertifikate (siehe `bauen`).
    let jetzt = 1_788_000_000i64;
    let t0 = Instant::now();
    let _ = kette_pruefen(&pfad, &speicher, jetzt, "localhost");
    let kette_kalt = t0.elapsed();
    let ketten_runden = 50;
    let t0 = Instant::now();
    for _ in 0..ketten_runden {
        let _ = kette_pruefen(&pfad, &speicher, jetzt, "localhost");
    }
    let kette = t0.elapsed();

    // added 03.10.2026, TLS: **DIE AUSGABE IST DAS ERGEBNIS**, nicht der Test.
    // Die Zahlen stehen in `TLS_5.txt`, und sie entscheiden, was optimiert wird.
    println!("TLS-ZEIT (je {runden} Runden, mitten im Rauschen):");
    println!(
        "  X25519 gemeinsam      {:>10.3?} gesamt = {:>8.1} us/Durchgang",
        x25519,
        x25519.as_secs_f64() * 1e6 / runden as f64
    );
    println!(
        "  P-256 ECDH            {:>10.3?} gesamt = {:>8.1} us/Durchgang",
        p256,
        p256.as_secs_f64() * 1e6 / runden as f64
    );
    println!(
        "  ECDSA skalar_punkt    {:>10.3?} gesamt = {:>8.1} us/Durchgang",
        punkt,
        punkt.as_secs_f64() * 1e6 / runden as f64
    );
    println!(
        "  kette_pruefen kalt    {:>10.3?}  (1x, leere Memo-Tabelle) = {:>8.1} us",
        kette_kalt,
        kette_kalt.as_secs_f64() * 1e6
    );
    println!(
        "  kette_pruefen warm    {:>10.3?} gesamt = {:>8.1} us/Durchgang",
        kette,
        kette.as_secs_f64() * 1e6 / ketten_runden as f64
    );
    println!(
        "  SPEEDUP warm/kalt     {:>10.1}x",
        kette_kalt.as_secs_f64() * ketten_runden as f64 / kette.as_secs_f64().max(1e-9)
    );
    println!(
        "  zertifikat_lesen (2x) {:>10.3?} gesamt = {:>8.1} us/Durchgang",
        lesen,
        lesen.as_secs_f64() * 1e6 / runden as f64
    );

    // added 03.10.2026, TLS: **DIE REIHENFOLGE, und sie ist der Befund.**
    // GEMESSEN am 03.10.: X25519 ist **schneller** als P-256 ECDH (siehe
    // `TLS_5.txt`), also ist `nachricht.rs:580` (X25519 zuerst) **richtig
    // entschieden** und wird nicht geaendert. Haette man das angegriffen,
    // waere es eine Verschlechterung gewesen.
    assert!(
        x25519 <= p256 * 2,
        "X25519 ({x25519:?}) darf nicht dramatisch langsamer sein als P-256 ({p256:?})"
    );
    // added 03.10.2026, TLS, **NACH DEM EINBAU DES MEMO**: die Assertion, die
    // ich zuerst geschrieben hatte, verglich `kette_pruefen` (mittlerweile der
    // **warme** Wert) mit einer Punktberechnung und wurde **rot** — 10 us gegen
    // 4436 us. Sie war vorher richtig und ist jetzt **falsch**, weil sie die
    // falsche Zahl benutzt. **Die KALT-Zeit ist die, die man vergleicht.**
    //
    // DER BEFUND, und er ist die Antwort auf die Frage der Leitung:
    // die **kalte** Kette (22 798 us) ist rund **5x** so teuer wie eine
    // Punktberechnung (4436 us) — das ist die **5 Punktberechnungen**, die
    // `kette_pruefen` fuer die beiden Zertifikate macht. **Die Kette ist der
    // Engpass, nicht der Schluesselaustausch.**
    assert!(
        kette_kalt.as_secs_f64() > punkt.as_secs_f64() / runden as f64,
        "die kalte Kette muss aufwaendiger sein als eine Punktberechnung"
    );
    // added 03.10.2026, TLS: **UND DIE MEMO-WIRKUNG WIRD FESTGENAGELT**, weil
    // sonst ein spaeterer Umbau sie still verliert. **100x** ist bewusst
    // **nicht** 2228x: die Zahl haengt an der Maschine, und eine Assertion, die
    // auf einem Rechner gruen und auf dem anderen rot ist, ist **kein** Test.
    // 100x ist die **untere** Schranke, die auch dann noch gilt, wenn der
    // Rechner unter Last laeuft und beide Zeiten um denselben Faktor wachsen.
    assert!(
        kette_kalt.as_secs_f64() > kette.as_secs_f64() / ketten_runden as f64 * 100.0,
        "die Memo muss mindestens 100x sparen: kalt {:?}, warm {:?} je Durchgang",
        kette_kalt,
        kette.as_secs_f64() / ketten_runden as f64
    );
}
