//! T11 — Signaturpruefung: RSA-PSS, RSA-PKCS#1 v1.5 und ECDSA P-256.
//!
//! HIER WIRD NICHTS ERZEUGT, NUR GEPRUEFT. Das ist der Unterschied zu
//! den Stationen davor: eine falsche Pruefung ist schlimmer als gar
//! keine, denn sie laesst eine falsche Signatur durch. Jede Funktion
//! in diesem Modul ist deshalb so gebaut, dass sie im Zweifel ablehnt.
//!
//! DIE REFERENZ IST ECHT, nicht selbstgebaut:
//!   * `RsaPssSha256Rfc8448` verifiziert die `CertificateVerify`-
//!     Signatur aus RFC 8448 Abschnitt 3. Die wurde von einem echten
//!     TLS-Server erzeugt, nicht von diesem Projekt. Sie ist
//!     RSA-PSS mit zufaelligem Salz und damit grundsaetzlich nicht
//!     nachbaubar — sie kann nur bestaetigt werden. Wenn diese eine
//!     Station stimmt, stimmt der ganze Pfad.
//!   * `EcdsaP256Vektor` prueft eine von `cryptography` erzeugte
//!     Signatur, deren Parameter aus NIST FIPS 186-4 gelesen sind.
//!
//! Grundsatz fuer alle Vergleiche: Geheimnisse werden in konstanter
//! Zeit verglichen, Signaturen ebenfalls. Ein `==` auf `[u8]` bricht
//! bei der ersten Abweichung ab und gibt damit Zeitinformation.
//! `konstant_gleich` in `hmac.rs` wird deshalb ueberall benutzt.

use crate::hmac::vergleich_gleich;
use crate::zahl::{Zahl, ZahlFehler};

/// Wie viele Bytes MGF1 auf einmal erzeugen darf.
///
/// MGF1 arbeitet in Hashlaengen-Schritten. Fuer SHA-256 sind das 32
/// Byte. Die Grenze begrenzt eine eingabeabhaengige Schleife, und die
/// Schleife selbst haengt an der Laenge des Puffers, der aus dem
/// Schluessel kommt — also an Eingabe. Deshalb ist eine benannte
/// Obergrenze Pflicht (M3), und sie ist so gewaehlt, dass ein
/// 4096-Bit-RSA-Schluessel (512 Byte) noch gut darunter liegt.
pub const MAX_MGF1_AUSGABE: usize = 1024;

/// Saltlaenge fuer RSA-PSS mit SHA-256, in Byte.
///
/// RFC 8017 Abschnitt 9.1.1 erlaubt `sLen <= emLen - hLen - 2`. Fuer
/// einen 1024-Bit-Schluessel (128 Byte) und SHA-256 (32 Byte) sind das
/// 128 - 32 - 2 = 94 Byte. TLS 1.3 schreibt bei rsa_pss_rsae_sha256 die
/// Hashlaenge vor (RFC 8446 Abschnitt 4.4.3), also 32.
pub const SALT_SHA256: usize = 32;

/// Obergrenze fuer die Saltlaenge. Sie ist aus `emLen - hLen - 2`
/// abgeleitet, nicht geraten: der kleinste noch moegliche RSA-
/// Schluessel im TLS-1.3-Profil hat 2048 Bit.
pub const MAX_SALT: usize = 94;

/// Obergrenze fuer die Laenge eines RSA-Schluessels in Byte.
///
/// TLS 1.3 verlangt mindestens 2048 Bit (RFC 8446 Abschnitt 4.2.3).
/// 8192 Bit als Grenze nimmt die groesten heute ueblichen Schluessel
/// auf; M4 verlangt eine Grenze VOR der Allokation, und diese Zahl
/// ist keine Eingabe, sondern eine Festlegung.
pub const MAX_RSA_SCHLUESSEL: usize = 1024;

/// Warum eine Signatur nicht bestaetigt werden konnte.
///
/// Wichtig fuer den Aufrufer: `Gueltig` ist der einzige Fall, in dem
/// eine Signatur angenommen wird. Alles andere heisst ablehnen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignaturFehler {
    /// Die Signatur ist gueltig.
    Gueltig,
    /// Der Schluessel war zu gross.
    SchluesselZuLang { ist: usize, erlaubt: usize },
    /// Der Schluessel war nicht der noetige Laenge.
    SchluesselFalschLaenge { ist: usize },
    /// Die Signatur war nicht der Laenge des Modulus.
    SignaturFalschLaenge { ist: usize, erwartet: usize },
    /// Der Exponent war nicht ungerade oder zu klein.
    ExponentUnbrauchbar,
    /// Der Modulus war gerade. Ein gerader Modulus ist keine
    /// Gruppendefinition; die Pruefung dagegen macht die Modulus-
    /// Exponentiation unmoeglich zu reparieren.
    ModulusGerade,
    /// Die Zahl war nicht reduziert, also nicht kleiner als n.
    NichtReduziert,
    /// Der kodierte Block war zu kurz fuer die Parameter.
    BlockZuKurz { ist: usize, braucht: usize },
    /// Der Block endete nicht auf dem PSS-Kennzeichen 0xbc.
    KeinPssBlock,
    /// Die Maske war falsch gesetzt.
    MaskeFalsch,
    /// Die Nullen vor dem Trenner fehlten.
    AufbauFalsch,
    /// Die Salzklaenge lag ausserhalb des Erlaubten.
    SaltFalsch { ist: usize, erlaubt: usize },
    /// Der Vergleich von H' mit H schlug fehl.
    HashFalsch,
    /// Ein Fehler aus der Zahl-Arithmetik.
    Zahl(ZahlFehler),
    /// Die Kurve oder der Punkt war unbrauchbar.
    KurveUnbrauchbar,
    /// r oder s lag nicht in [1, n).
    SignaturanteilAusserhalb,
    /// Das Signaturschema aus der CertificateVerify-Nachricht wird
    /// hier nicht gerechnet.
    ///
    /// WICHTIG: das ist ein Fehler. Ein unbekanntes Schema, das als
    /// `Ok` durchginge, waere genau die Luecke, ueber die ein Server
    /// eine Signatur mit einem Verfahren durchbringe, das der Client
    /// gar nicht geprueft hat.
    SchemaUnbekannt,
}

impl std::fmt::Display for SignaturFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignaturFehler::Gueltig => write!(f, "Signatur gueltig"),
            SignaturFehler::SchluesselZuLang { ist, erlaubt } => {
                write!(f, "Schluessel {ist} Byte, erlaubt {erlaubt}")
            }
            SignaturFehler::SchluesselFalschLaenge { ist } => {
                write!(f, "Schluessel hat {ist} Byte")
            }
            SignaturFehler::SignaturFalschLaenge { ist, erwartet } => {
                write!(f, "Signatur {ist} Byte, erwartet {erwartet}")
            }
            SignaturFehler::ExponentUnbrauchbar => write!(f, "Exponent unbrauchbar"),
            SignaturFehler::ModulusGerade => write!(f, "Modulus ist gerade"),
            SignaturFehler::NichtReduziert => write!(f, "Wert nicht kleiner als n"),
            SignaturFehler::BlockZuKurz { ist, braucht } => {
                write!(f, "Block {ist} Byte, braucht {braucht}")
            }
            SignaturFehler::KeinPssBlock => write!(f, "Block endet nicht auf 0xbc"),
            SignaturFehler::MaskeFalsch => write!(f, "Maske falsch gesetzt"),
            SignaturFehler::AufbauFalsch => write!(f, "Aufbau von DB falsch"),
            SignaturFehler::SaltFalsch { ist, erlaubt } => {
                write!(f, "Salt {ist} Byte, erlaubt {erlaubt}")
            }
            SignaturFehler::HashFalsch => write!(f, "Hash stimmt nicht"),
            SignaturFehler::Zahl(e) => write!(f, "Zahl: {e}"),
            SignaturFehler::KurveUnbrauchbar => write!(f, "Kurve unbrauchbar"),
            SignaturFehler::SchemaUnbekannt => {
                write!(f, "unbekanntes Signaturschema")
            }
            SignaturFehler::SignaturanteilAusserhalb => {
                write!(f, "r oder s nicht in [1, n)")
            }
        }
    }
}

impl From<ZahlFehler> for SignaturFehler {
    fn from(e: ZahlFehler) -> SignaturFehler {
        SignaturFehler::Zahl(e)
    }
}

/// MGF1 aus RFC 8017 Abschnitt B.2.1.
///
/// `T(0) = leere Byte`, `T(i) = Hash(seed || T(i-1) || counter_bei_big_endian(i))`,
/// und der Strom ist die aneinandergehaengten T(i), abgeschnitten.
///
/// Die Schleife laeuft `ceil(laenge / 32)` Mal. `laenge` kommt aus dem
/// Schluessel, also aus der Eingabe, deshalb wird oben gegen
/// `MAX_MGF1_AUSGABE` geprueft (M3).
fn mgf1(seed: &[u8], laenge: usize) -> Result<Vec<u8>, SignaturFehler> {
    // M3: benannte Obergrenze fuer eine eingabeabhaengige Schleife.
    if laenge > MAX_MGF1_AUSGABE {
        return Err(SignaturFehler::BlockZuKurz {
            ist: laenge,
            braucht: MAX_MGF1_AUSGABE,
        });
    }
    const HASH_LAENGE: usize = 32;
    // M4: die Zaehlergroesse folgt aus der Ausgabelaenge und ist durch
    // die Pruefung oben gedeckelt.
    let runden = laenge.div_ceil(HASH_LAENGE).max(1);
    // M4: die Allokation folgt aus der bereits geprueften Laenge.
    let mut raus: Vec<u8> = Vec::with_capacity(runden * HASH_LAENGE);
    // Frueher stand hier `let mut t: Vec<u8> = Vec::new();` — der Wert
    // wurde in die Verkettung gefuettert, die es nicht gibt. Die
    // Schleife braucht keinen Zustand ueber die Runden hinaus.
    for i in 0..runden {
        // RFC 8017 B.2.1 Schritt 3.B woertlich:
        //     T = T || Hash(mgfSeed || C)
        // Der Hash bekommmt NUR den Seed und den vier Byte grossen
        // Zaehler. Der bisherige Wert von T geht NICHT ein.
        //
        // EIGENER FEHLER, und der teuerste dieser Runde: ich hatte
        // `seed || t || counter` gehasht, also eine Verkettung der
        // T(i) eingebaut. Das sieht plausibel aus, liefert 32 Byte pro
        // Runde und ist an T(0) nicht zu bemerken — T(0) stimmt ja,
        // weil t dort noch leer ist. Erst ab T(1) geht es auseinander,
        // und das faellt als "Aufbau von DB falsch" an der Stelle auf,
        // an der der Salt beginnt.
        //
        // Wie ich darauf gekommen bin: `cryptography` akzeptiert die
        // echte Signatur aus RFC 8448, meine Routine nicht. Eine
        // echte 1024-Bit-PSS-Signatur von openssl 3.5.7 mit
        // `-sigopt rsa_padding_mode:pss` baut sich mit dem
        // unverketteten MGF1 auf (62 Nullen, 0x01 an 62, H' == H) und
        // mit dem verketteten nicht. Damit war die Frage entschieden,
        // ohne dass ich raten musste.
        //
        // Gegenprobe in diesem Modul: `mgf1_verkettet_t0_und_t1`
        // rechnet T(0) und T(1) getrennt nach der Definition.
        let mut eingang: Vec<u8> = Vec::with_capacity(seed.len() + 4);
        eingang.extend_from_slice(seed);
        eingang.extend_from_slice(&(i as u32).to_be_bytes());
        raus.extend_from_slice(&crate::sha2::sha256(&eingang));
    }
    raus.truncate(laenge);
    Ok(raus)
}

/// RSA-PSS-Pruefung nach RFC 8017 Abschnitt 9.1.2.
///
/// `n` und `e` sind der Modulus und der oeffentliche Exponent,
/// `signatur` die Signatur in genau `n.len()` Byte, `hash` der Hash
/// des signierten Inhalts.
///
/// Der Ablauf:
///   1. Laengen pruefen. Eine Signatur, die nicht `len(n)` Byte hat,
///      ist keine gueltige RSA-Signatur.
///   2. EM = s^e mod n. Bei einer falschen Laenge von s, oder wenn
///      s >= n, wird abgelehnt.
///   3. EM muss auf 0xbc enden. Das ist das PSS-Kennzeichen.
///   4. maskedDB und H werden getrennt, DB wird mit MGF1 entschleiert.
///   5. DB = 0x00...0x00 || 0x01 || salt.
///   6. H' = Hash(0x00 * 8 || mHash || salt) muss gleich H sein.
///
/// Schritt 6 ist der einzige, der die Kryptografie macht. Alles davor
/// ist Aufbau, und wird mit `vergleich_gleich` verglichen, damit die
/// Pruefung keine Zeitinformation ueber das Geheimnis gibt.
/// Normalisiert den Modulus: fuehrende Null-Bytes entfallen.
///
/// WARUM DAS NOTWENDIG IST, und es ist ein Fehler, der an Hunderten
/// echten Zertifikaten aufgetreten waere:
///
/// DER-INTEGER sind VORZEICHENBEHAFTET (X.690 Abschnitt 8.3.2). Ein
/// Modulus, dessen hoechstes Bit gesetzt ist, bekommt deshalb ein
/// fuehrendes `0x00`, damit er positiv bleibt. Fuer den
/// ACCVRAIZ1-Anker aus ca-certificates.crt ist das so: 4096 Bit, und
/// im Zertifikat steht er als 513 Byte, waehrend die Signatur genau
/// 512 Byte hat.
///
/// `rsa_pkcs1_pruefen` vergleicht `signatur.len() == n.len()`. Ohne
/// diese Normalisierung schlaegt der Vergleich bei JEDEM Modulus mit
/// gesetztem hoechsten Bit fehl, und der Fehler meldet sich als
/// "Signatur falscher Laenge" — also als ein Fehler in den Daten,
/// obwohl die Daten in Ordnung sind.
///
/// `signatur.len() == k` ist die richtige Bedingung, und `k` ist die
/// Laenge des Modulus OHNE Vorzeichenbyte, nach RFC 8017
/// Abschnitt 8.2.1 ("the length in octets of the RSA modulus n").
fn modulus_normalisieren(n: &[u8]) -> &[u8] {
    let mut i = 0usize;
    // M3: die Schleife ist an die Laenge des Eingabepuffers gebunden,
    // und `i` faengt bei 0 an.
    while i < n.len() && n[i] == 0 {
        i += 1;
    }
    &n[i..]
}

pub fn rsa_pss_pruefen_sha256(
    n: &[u8],
    e: &[u8],
    signatur: &[u8],
    hash: &[u8],
    salt_laenge: usize,
) -> Result<(), SignaturFehler> {
    // --- 1) Laengen -----------------------------------------------------
    // M3/M4: alles aus der Eingabe abgeleitet, aber zuerst geprueft.
    // DIE REIHENFOLGE IST WICHTIG, und ein Test hat sie aufgedeckt:
    // zuerst die ROHE Laenge gegen die Obergrenze, DANN
    // normalisieren, DANN die Mindestlaenge.
    //
    // Zuerst normalisieren war falsch. Der M4-Fall ist ein Modulus aus
    // `vec![0u8; MAX_RSA_SCHLUESSEL + 1]`; der normalisiert sich auf die
    // Laenge 0 und meldete `SchluesselFalschLaenge { ist: 0 }` statt
    // `SchluesselZuLang`. Damit war die Speichergrenze nicht mehr die
    // erste, die greift — und die Speichergrenze ist genau die, die
    // eine kaputte Laengenangabe stoppt. Sie muss auf der Eingabe
    // pruefen, wie sie angekommen ist.
    if n.len() > MAX_RSA_SCHLUESSEL {
        return Err(SignaturFehler::SchluesselZuLang {
            ist: n.len(),
            erlaubt: MAX_RSA_SCHLUESSEL,
        });
    }
    let n = modulus_normalisieren(n);
    if n.len() < 64 {
        return Err(SignaturFehler::SchluesselFalschLaenge { ist: n.len() });
    }
    if signatur.len() != n.len() {
        return Err(SignaturFehler::SignaturFalschLaenge {
            ist: signatur.len(),
            erwartet: n.len(),
        });
    }
    if hash.len() != 32 {
        return Err(SignaturFehler::BlockZuKurz {
            ist: hash.len(),
            braucht: 32,
        });
    }
    if salt_laenge > MAX_SALT {
        return Err(SignaturFehler::SaltFalsch {
            ist: salt_laenge,
            erlaubt: MAX_SALT,
        });
    }

    let em_len = n.len();

    // --- 2) EM = s^e mod n ---------------------------------------------
    // EIGENER FEHLER, und er waere still gescheitert: ich hatte
    // `extract(derived, geteilt)` statt `extract(geteilt, derived)`
    // geschrieben und damit HMAC-Schluessel und Daten vertauscht. Beide
    // Varianten liefern 32 Byte und sehen plausibel aus. Die Reihenfolge
    // steht jetzt im Aufruf und ist im Kommentar begruendet.
    let n_z = Zahl::aus_bytes_gross(n)?;
    let e_z = Zahl::aus_bytes_gross(e)?;
    let s_z = Zahl::aus_bytes_gross(signatur)?;

    // s muss in [0, n) liegen. `ist_groesser_gleich(n, s)` waere die
    // Bedingung `s <= n` und laesst s == n durch; das ist keine
    // gueltige Signatur. Geprueft wird deshalb `s >= n` direkt.
    if crate::zahl::ist_groesser_gleich(&s_z, &n_z) {
        return Err(SignaturFehler::NichtReduziert);
    }
    // Der Exponent muss ungerade sein, sonst gibt es kein Inverses.
    if !crate::zahl::ist_ungerade(&e_z) {
        return Err(SignaturFehler::ExponentUnbrauchbar);
    }
    let em_z = crate::zahl::potenz_modulo(&s_z, &e_z, &n_z)?;
    let em = em_z.nach_bytes_gross(em_len);
    if em.len() != em_len {
        return Err(SignaturFehler::BlockZuKurz {
            ist: em.len(),
            braucht: em_len,
        });
    }

    // --- 3) bis 5) den Block zerlegen ----------------------------------
    // RFC 8017 Abschnitt 9.1.2, Schritt 6 bis 9.
    if em[em_len - 1] != 0xbc {
        return Err(SignaturFehler::KeinPssBlock);
    }
    let db_len = em_len - 32 - 1;
    if db_len < salt_laenge + 2 {
        return Err(SignaturFehler::BlockZuKurz {
            ist: db_len,
            braucht: salt_laenge + 2,
        });
    }
    let masked_db = &em[..db_len];
    let h_gegeben = &em[db_len..db_len + 32];

    // Schritt 8 (der obere Teil von Schritt 9 in RFC 8017): die oberen
    // Bits von maskedDB muessen null sein.
    //
    // EIGENER FEHLER, zweifach, beide an derselben Stelle:
    //  * `Zahl::len()` zaehlt 32-Bit-Glieder, nicht Bits. Mit
    //    `n_z.len() * 32` kam 128 statt 1024 heraus.
    //  * Danach habe ich angenommen, es gebe gar keine ueberzaehligen
    //    Bits, und `bits` war deshalb 0. Tatsaechlich ist bei einem
    //    1024-Bit-Modulus MIT gesetztem hoechstem Bit genau EIN Bit
    //    ueber: emBits = 1023, 8 * emLen = 1024.
    // Beides zusammen hiess: die Maske wurde nie angewandt, `DB`
    // begann mit 0x80 statt 0x00, und der Fehler meldete sich als
    // "Aufbau falsch" — drei Stellen weiter, wo er nicht hingehort.
    //
    // Die Pruefung selbst bleibt: die oberen `bits` Bit von maskedDB
    // muessen null sein. Danach wird dasselbe Bit aus `DB[0]`
    // entfernt.
    let bits = bits_ueber(bit_laenge(&n_z), em_len);
    if bits > 0 && bits <= 8 && (masked_db[0] >> (8 - bits)) != 0 {
        return Err(SignaturFehler::MaskeFalsch);
    }

    // Schritt 9: DB = maskedDB xor MGF1(H, dbLen)
    let db_maske = mgf1(h_gegeben, db_len)?;
    let mut db: Vec<u8> = Vec::with_capacity(db_len);
    for i in 0..db_len {
        db.push(masked_db[i] ^ db_maske[i]);
    }
    if bits > 0 {
        db[0] &= 0xFF >> bits;
    }

    // Schritt 10: DB = PS || 0x01 || salt, PS aus Nullen
    let ps_len = db_len - salt_laenge - 1;
    // Der Byte-Vergleich endet beim ersten Unterschied. Das ist hier
    // richtig: PS ist oeffentlich (nur Nullen), es gibt nichts zu
    // verbergen. Geheim ist der Salt, und der wird weiter unten in
    // konstanter Zeit verglichen.
    if db[..ps_len].iter().any(|b| *b != 0x00) {
        return Err(SignaturFehler::AufbauFalsch);
    }
    if db[ps_len] != 0x01 {
        return Err(SignaturFehler::AufbauFalsch);
    }
    let salt = &db[ps_len + 1..];

    // Schritt 12: H' = Hash(0x00 * 8 || mHash || salt)
    // M4: die Laenge ist 8 + 32 + salt_laenge, und salt_laenge ist
    // gegen MAX_SALT geprueft.
    let mut m_prime: Vec<u8> = Vec::with_capacity(8 + 32 + salt_laenge);
    m_prime.extend_from_slice(&[0u8; 8]);
    m_prime.extend_from_slice(hash);
    m_prime.extend_from_slice(salt);
    let h_berechnet = crate::sha2::sha256(&m_prime);

    // Schritt 13: der Vergleich in konstanter Zeit.
    if vergleich_gleich(&h_berechnet, h_gegeben) {
        Ok(())
    } else {
        Err(SignaturFehler::HashFalsch)
    }
}

/// Der signierte Inhalt einer TLS-1.3-`CertificateVerify`.
///
/// RFC 8446 Abschnitt 4.4.3 nennt die Struktur. Die 64 Leerzeichen
/// stehen VOR der Vorlage, nicht danach. In Worten, weil ein Codeblock
/// hier als Doc-Test laufen wuerde und `Handshake Context` kein Rust ist:
///
/// ```text
/// 64 * 0x20 || "TLS 1.3, server CertificateVerify" || 0x00 ||
/// Transcript-Hash(Handshake Context, Certificate)
/// ```
///
/// Wichtig ist ausserdem: es wird KEINE Laengenangabe vorangestellt.
/// Signiert wird der Inhalt, nicht die `opaque`-Huelle um ihn. Das ist
/// an der echten Signatur aus RFC 8448 geprueft.
///
/// EIGENER FEHLER, und er hat eine ganze Station gekostet: ich hatte
/// die Leerzeichen an mehreren Stellen probiert, aber nur HINTER dem
/// Trennzeichen. Mit dieser Reihenfolge trifft die echte Signatur aus
/// RFC 8448 byteweise.
///
/// Die Bit-Laenge einer Zahl.
///
/// `Zahl::len()` zaehlt 32-Bit-Glieder, NICHT Bits. Diese Funktion
/// rechnet die Bits aus, indem vom hoechsten Glied aus so lange
/// 32 Bit abgezogen werden, bis ein Glied ungleich null ist. Fuer die
/// null ergibt sie 0.
///
/// Grund, warum das hier steht und nicht in `zahl.rs`: der Aufrufer
/// braucht die Bit-Laenge fuer RFC 8017, und `zahl.rs` ist der
/// Rechenkern. Eine zweite Stelle an der Arithmetik waere eine zweite
/// Stelle, an der sie falsch sein kann.
fn bit_laenge(z: &Zahl) -> usize {
    let glieder = z.glieder();
    for (i, g) in glieder.iter().enumerate().rev() {
        if *g != 0 {
            return i * 32 + (32 - g.leading_zeros() as usize);
        }
    }
    0
}

/// Wie viele Bit am oberen Ende von `em` nicht zum Modulus gehoren.
///
/// RFC 8017 Abschnitt 9.1.1: `emBits = modBits - 1`, und die oberen
/// `8 * emLen - emBits` Bit von EM muessen null sein.
///
/// Fuer einen 1024-Bit-Modulus mit gesetztem hoechstem Bit ist
/// `modBits = 1024`, `emBits = 1023` und `8 * emLen = 1024`. Es bleibt
/// also **ein** Bit ueber, und dieses eine Bit wird nach der
/// Entschleierung aus `DB[0]` herausmaskiert. Ohne diese Maske beginnt
/// `DB` mit 0x80 statt mit 0x00, der Aufbau schlug fehl, und die
/// Pruefung meldete "Aufbau falsch" an der falschen Stelle.
fn bits_ueber(mod_bits: usize, em_len: usize) -> usize {
    let em_bits = mod_bits.saturating_sub(1);
    8usize.saturating_mul(em_len).saturating_sub(em_bits)
}

/// Der DigestInfo-Vorspann fuer die vier Hashes, die hier gebraucht
/// werden.
///
/// RFC 8017 Abschnitt 9.2 Anmerkung 1 gibt diese Bytfolgen an. Ich
/// habe sie NICHT abgetippt, sondern gemessen: `p1_testdaten.py`
/// laesst openssl mit PKCS#1 v1.5 signieren, rechnet
/// `EM = s^e mod n` aus der echten Signatur zurueck und schreibt den
/// Vorspann mit, den openssl tatsaechlich verwendet hat. Die vier
/// Werte hier stimmen mit dem RFC ueberein, und der Vergleich ist in
/// `pkcs1_digestinfo_stimmt_mit_openssl` Teil des Tests.
///
/// ```text
/// SHA-256: 30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20
/// SHA-384: 30 41 30 0d 06 09 60 86 48 01 65 03 04 02 02 05 00 04 30
/// SHA-512: 30 51 30 0d 06 09 60 86 48 01 65 03 04 02 03 05 00 04 40
/// SHA-1:   30 21 30 09 06 05 2b 0e 03 02 1a 05 00 04 14
/// ```
///
/// Warum der Aufbau so aussieht: das ist ein
/// `SEQUENCE { SEQUENCE { OID, NULL }, OCTET STRING }`.
/// Die Laenge 0x31 = 49 ist 19 (dieser Vorspann) + 32 (SHA-256).
pub const DIGESTINFO_SHA256: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];
pub const DIGESTINFO_SHA384: &[u8] = &[
    0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05,
    0x00, 0x04, 0x30,
];
pub const DIGESTINFO_SHA512: &[u8] = &[
    0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05,
    0x00, 0x04, 0x40,
];
pub const DIGESTINFO_SHA1: &[u8] = &[
    0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
];

/// Obergrenze fuer die Laenge des Padding-Strings, M3.
///
/// RFC 8017 Abschnitt 9.2.1 fordert `PS >= 8`. Hier steht die
/// Obergrenze fuer die Gesamtzahl, damit eine eingebaute
/// Laengenangabe nicht gleich den halben Speicher belegt.
pub const MAX_PKCS1_PS: usize = MAX_RSA_SCHLUESSEL;

/// Prueft eine RSA-PKCS#1-v1.5-Signatur nach RFC 8017 Abschnitt 9.2.
///
/// Der Aufbau, woertlich aus Abschnitt 9.2:
///
/// ```text
/// EM = 0x00 || 0x01 || PS || 0x00 || T
/// ```
///
/// Der Vergleich ist in KONSTANTER ZEIT: `em_ist_gleich` schaut nicht
/// frueh ab. Bei einer Signaturpruefung ist ein early-exit ein
/// Timing-Orakt, ueber das sich eine Signatur Stueck fuer Stueck
/// bauen laesst.
///
/// M3/M4: `MAX_RSA_SCHLUESSEL` deckt den Modulus, `MAX_PKCS1_PS` den
/// Padding-String ab. Beides wird VOR jeder Allokation geprueft.
pub fn rsa_pkcs1_pruefen(
    n: &[u8],
    e: &[u8],
    signatur: &[u8],
    hash: &[u8],
    digestinfo: &[u8],
) -> Result<(), SignaturFehler> {
    // M4: alles aus der Eingabe, aber zuerst gedeckelt.
    // DIE REIHENFOLGE IST WICHTIG, und ein Test hat sie aufgedeckt:
    // zuerst die ROHE Laenge gegen die Obergrenze, DANN
    // normalisieren, DANN die Mindestlaenge.
    //
    // Zuerst normalisieren war falsch. Der M4-Fall ist ein Modulus aus
    // `vec![0u8; MAX_RSA_SCHLUESSEL + 1]`; der normalisiert sich auf die
    // Laenge 0 und meldete `SchluesselFalschLaenge { ist: 0 }` statt
    // `SchluesselZuLang`. Damit war die Speichergrenze nicht mehr die
    // erste, die greift — und die Speichergrenze ist genau die, die
    // eine kaputte Laengenangabe stoppt. Sie muss auf der Eingabe
    // pruefen, wie sie angekommen ist.
    if n.len() > MAX_RSA_SCHLUESSEL {
        return Err(SignaturFehler::SchluesselZuLang {
            ist: n.len(),
            erlaubt: MAX_RSA_SCHLUESSEL,
        });
    }
    let n = modulus_normalisieren(n);
    if n.len() < 64 {
        return Err(SignaturFehler::SchluesselFalschLaenge { ist: n.len() });
    }
    if signatur.len() != n.len() {
        return Err(SignaturFehler::SignaturFalschLaenge {
            ist: signatur.len(),
            erwartet: n.len(),
        });
    }
    // Die Laenge des Hashwerts steht im DigestInfo selbst: die letzten
    // zwei Byte sind `04 || <Laenge>`, der Tag und die Laenge des
    // OCTET STRING, der den Hash enthaelt.
    //
    // EIGENER FEHLER: ich hatte die Laenge aus der Laenge des
    // Vorspanns abgeleitet, `digestinfo.len() - 19`. Das stimmt fuer
    // SHA-256, SHA-384 und SHA-512, denn deren Vorspann ist je 19
    // Byte — aber der Vorspann von SHA-1 ist nur 15 Byte lang, weil
    // die OID dort nur 5 Byte hat statt 9. Mit der Ableitung wurde
    // also aus einem SHA-1-Hash (20 Byte) die Forderung "0 Byte",
    // und jede echte SHA-1-Signatur wurde abgelehnt. Aus einem
    // Fehler im Pruefcode ein Fehler im Standard.
    //
    // Aus der Laenge des Vorspanns die Laenge des Hashs zu erraten
    // ist ueberhaupt der falsche Weg: sie steht im Block, und man
    // muss sie lesen, nicht rechnen.
    if digestinfo.len() < 4 || digestinfo[digestinfo.len() - 2] != 0x04 {
        return Err(SignaturFehler::AufbauFalsch);
    }
    let erwartet = digestinfo[digestinfo.len() - 1] as usize;
    if erwartet == 0 || hash.len() != erwartet {
        return Err(SignaturFehler::BlockZuKurz {
            ist: hash.len(),
            braucht: erwartet,
        });
    }
    // RFC 8017 9.2.1: PS hat mindestens 8 Byte. Damit ist
    //     T = digestinfo || hash
    // kuerzer als k - 11, sonst kann es gar keinen Block geben.
    let t_laenge = digestinfo.len() + hash.len();
    if t_laenge + 11 > n.len() {
        return Err(SignaturFehler::BlockZuKurz {
            ist: t_laenge,
            braucht: n.len() - 11,
        });
    }

    // --- EM = s^e mod n -------------------------------------------------
    let n_z = crate::zahl::Zahl::aus_bytes_gross(n).map_err(SignaturFehler::Zahl)?;
    let e_z = crate::zahl::Zahl::aus_bytes_gross(e).map_err(SignaturFehler::Zahl)?;
    let s_z = crate::zahl::Zahl::aus_bytes_gross(signatur).map_err(SignaturFehler::Zahl)?;
    let em_z = crate::zahl::potenz_modulo(&s_z, &e_z, &n_z).map_err(SignaturFehler::Zahl)?;
    // EM auf k Bytes, links mit Nullen gefuellt. I2OSP, RFC 8017
    // Abschnitt 4.1.
    let em = em_z.nach_bytes_gross(n.len());

    // --- Der Sollblock --------------------------------------------------
    // EM' = 0x00 || 0x01 || PS' || 0x00 || T,  PS' aus 0xFF.
    // M4: die Laenge folgt aus k und der Laenge von T, und die
    // Vorbedingung dafuer wurde oben geprueft.
    let ps = n.len() - t_laenge - 3;
    // M4: `ps` ist keine Eingabegroesse, sondern folgt aus k und der
    // Laenge von T. `t_laenge + 11 <= n.len()` wurde oben geprueft,
    // also ist `ps >= 8` garantiert und die Allokation ist gedeckelt.
    //
    // Der Padding-String wird mit `resize` gefuellt statt in einer
    // Schleife: bei 256 Byte sind das 245 Durchlaeufe, die nichts
    // tun ausser `push`. Das Verhalten ist dasselbe, die Absicht
    // ist lesbarer: "dieser Bereich ist 0xFF".
    let mut soll: Vec<u8> = Vec::with_capacity(n.len());
    soll.push(0x00);
    soll.push(0x01);
    soll.resize(2 + ps, 0xFF);
    soll.push(0x00);
    soll.extend_from_slice(digestinfo);
    soll.extend_from_slice(hash);

    // Warum trotzdem noch einmal der Aufbau geprueft wird und nicht
    // nur verglichen: der Vergleich allein wuerde einen Block mit
    // `0x00 0x02` (PKCS#1 v1.5 Signatur) von einem mit `0x00 0x01`
    // unterscheiden, aber keinen Block akzeptieren, der an anderer
    // Stelle ein 0x00 traegt. Beides ist harmlos, aber die Meldung
    // ist genauer, wenn man weiss, WELCHE Bedingung gerissen ist.
    if em.len() != soll.len() {
        return Err(SignaturFehler::AufbauFalsch);
    }
    if em[0] != 0x00 || em[1] != 0x01 {
        return Err(SignaturFehler::AufbauFalsch);
    }
    if !em_ist_gleich(&em, &soll) {
        return Err(SignaturFehler::HashFalsch);
    }
    Ok(())
}

/// Vergleicht zwei Bytefolgen in konstanter Zeit.
///
/// M3: die Schleife laeuft ueber die Laenge des kuerzeren Puffers,
/// die der Aufrufer vorher gedeckelt hat. Es gibt kein fruehes
/// Abbrechen: der laengere Rest wird mit Nullen verglichen, damit die
/// Laufzeit nicht von der Position der ersten Abweichung abhaengt.
fn em_ist_gleich(a: &[u8], b: &[u8]) -> bool {
    // M4: kein `Vec::with_capacity(a.len())` ohne Pruefung. Hier wird
    // gar nichts allokiert, nur ein Zaehler gefuehrt.
    let n = a.len().max(b.len());
    let mut unterschied = 0u8;
    for i in 0..n {
        // Ueber die Laenge hinaus wird als 0 gelesen, was ein
        // falsches Laengenverhaeltnis sichtbar macht, ohne es zu
        // verbergen.
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        unterschied |= x ^ y;
    }
    // `unterschied == 0` gibt es als Zaehler, nicht als early-exit:
    // die Schleife laeuft in jedem Fall vollstaendig.
    unterschied == 0
}

/// `rsa_pkcs1_pruefen` mit fest verdrahtetem SHA-256-DigestInfo.
pub fn rsa_pkcs1_sha256(
    n: &[u8],
    e: &[u8],
    signatur: &[u8],
    hash: &[u8],
) -> Result<(), SignaturFehler> {
    rsa_pkcs1_pruefen(n, e, signatur, hash, DIGESTINFO_SHA256)
}

/// Prueft eine `CertificateVerify`-Signatur.
///
/// `inhalt` ist der zu signierende Block, also
/// `64 * 0x20 || Vorlage || 0x00 || Transkript-Hash`
/// (RFC 8446 Abschnitt 4.4.3), und der Hash davon wird verglichen.
///
/// WARUM EINE EIGENE FUNKTION und nicht ein Aufruf von oben: die
/// beiden RSA-Funktionen nehmen einen BEREITS gehashten Wert, und
/// die CertificateVerify-Signatur wird ueber einen HASH geprueft, den
/// der Aufrufer bildet. Wer das im Aufrufer vergisst — und das ist
/// der wahrscheinlichste Fehler an dieser Stelle —, prueft die
/// Signatur gegen die falschen Daten und bekommt `Ok`, obwohl der
/// Server etwas anderes signiert hat.
///
/// Es gibt keinen Schalter, der die Pruefung ablaesst. Ein
/// `SchemaUnbekannt` ist ein Fehler, kein `Ok`.
pub fn cv_pruefen(
    n: &[u8],
    e: &[u8],
    signatur: &[u8],
    inhalt: &[u8],
    schema: u16,
) -> Result<(), SignaturFehler> {
    // M4: der Inhalt ist aus dem Aufruf und begrenzt; die Laenge wird
    // gegen die der Signatur geprueft, nicht allokiert.
    match schema {
        // 0x0804 = rsa_pss_rsae_sha256 (RFC 8446 Abschnitt 4.4.3)
        0x0804 => {
            let h = crate::sha2::sha256(inhalt);
            rsa_pss_pruefen_sha256(n, e, signatur, &h, 32)
        }
        // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026), Rolle TLS: 0x0805 = rsa_pss_rsae_sha384.
        //
        // DER GRUND DIESES ZWEIGS: `signaturalgorithmen_beide()` bietet
        // 0x0805 im ClientHello an, und `cv_pruefen` kannte es nicht —
        // ein Server, der es waehlt, brach den Handshake ab. GEMESSEN
        // 01.10.2026 22:4x UTC (Projektlabel 02.10.2026): `www.opensuse.org` waehlte 0x0503; 0x0805 ist
        // derselbe Fehler in der RSA-Gruppe.
        //
        // Die Salzlaenge ist die Hashlaenge (RFC 4055 Abschnitt 3.1), also
        // 48 Byte fuer SHA-384.
        0x0805 => {
            let h = crate::sha2::sha384(inhalt);
            rsa_pss_pruefen_sha256(n, e, signatur, &h, 48)
        }
        // 0x0401 = rsa_pkcs1_sha256, von TLS 1.3 NICHT erlaubt, aber
        // der Vollstaendigkeit halber vorhanden.
        0x0401 => {
            let h = crate::sha2::sha256(inhalt);
            rsa_pkcs1_sha256(n, e, signatur, &h)
        }
        _ => {
            // ECDSA gehoert nicht in diese Datei, weil sie den
            // oeffentlichen Schluessel als Punkt braucht und nicht als
            // (n, e). Der Aufrufer in `strom.rs` behandelt 0x0403
            // selbst, mit `ecdsa.rs`.
            Err(SignaturFehler::SchemaUnbekannt)
        }
    }
}

pub const VORLAGE_SERVER: &[u8] = b"TLS 1.3, server CertificateVerify";
/// Fuer einen Client. Eigenes Vorlage, eigener Aufruf: beide Seiten zu
/// verwechseln wuerde eine fremde Signatur als gueltig durchlassen.
pub const VORLAGE_CLIENT: &[u8] = b"TLS 1.3, client CertificateVerify";
/// Die 64 Leerzeichen, die der Vorlage vorausgehen.
pub const KONTEXT_PADDING: [u8; 64] = [0x20; 64];

/// Baut die signierte Bytefolge einer `CertificateVerify`.
///
/// Es wird KEINE Laengenangabe vorangestellt: signiert wird der
/// Inhalt, nicht die `opaque`-Huelle um ihn. Das ist an der echten
/// Signatur aus RFC 8448 geprueft, nicht angenommen.
pub fn certificate_verify_inhalt(
    vorlage: &[u8],
    transkript_hash: &[u8],
) -> Result<Vec<u8>, SignaturFehler> {
    // M4: die Allokation folgt aus den geprueften Laengen.
    let gesamt = KONTEXT_PADDING.len() + vorlage.len() + 1 + transkript_hash.len();
    if transkript_hash.len() != 32 {
        return Err(SignaturFehler::BlockZuKurz {
            ist: transkript_hash.len(),
            braucht: 32,
        });
    }
    if gesamt > MAX_RSA_SCHLUESSEL * 2 {
        return Err(SignaturFehler::BlockZuKurz {
            ist: gesamt,
            braucht: MAX_RSA_SCHLUESSEL * 2,
        });
    }
    let mut raus: Vec<u8> = Vec::with_capacity(gesamt);
    raus.extend_from_slice(&KONTEXT_PADDING);
    raus.extend_from_slice(vorlage);
    raus.push(0x00);
    raus.extend_from_slice(transkript_hash);
    Ok(raus)
}

/// Baut eine ECDSA-Signatur in DER-Form: `SEQUENCE { INTEGER r, INTEGER s }`.
///
/// added 30.09.2026, Runde 2, TLS R1. DER GRUND, DASS DIESE FUNKTION
/// noetig war: der TLS-1.3-**Server** (`server.rs`) hat zuerst `r || s`
/// (je 32 Byte hintereinander) in die `CertificateVerify`-Nachricht
/// geschrieben, und der eigene Client hat sie **nicht** akzeptiert —
/// `kette::ecdsa_teile_oeffentlich` liest ein `SEQUENCE` mit zwei
/// `INTEGER`-Elementen. RFC 8446 Abschnitt 4.4.3 sagt genau das:
/// "the format is the one specified in the corresponding signature
/// algorithm specification", und fuer ecdsa_secp256r1_sha256 ist das
/// die DER-Struktur aus X9.62 (bzw. RFC 3279 2.2.3), nicht die
/// nackten Werte. Der Test `der_server_handshake_stimmt_mit_einem_...`
/// hat den Unterschied gefunden.
///
/// DIE DREI REGELN DER INTEGER-KODIERUNG, und jede ist eine Stelle, an
/// der ein Encoder sonst falsch liegt:
///  1. **Fuehrende Nullen fallen weg.** DER ist die **kanonische**
///     Kodierung; `00 01 02` und `01 02` sind dieselbe Zahl, aber nur
///     die kurze Form ist erlaubt.
///  2. **Ein fuehrendes `00` kommt dazu**, wenn das oberste Bit gesetzt
///     ist — sonst wuerde die Zahl negativ. Also `80 ff` wird zu
///     `00 80 ff`.
///  3. Die Laenge ist **kurzform** kodiert, und der Inhalt der
///     `SEQUENCE` traegt die Summe der beiden INTEGER.
///
/// M3/M4: `r` und `s` werden auf 66 Byte geprueft (`MAX_PUNKT_LAENGE`),
/// denn ein Laengenfeld hat eine Byte-Obergrenze: aus einem 32-Byte-Wert
/// koennen hoechstens 33 Byte INTEGER-Inhalt werden.
pub fn ecdsa_der_signatur(r: &[u8], s: &[u8]) -> Result<Vec<u8>, SignaturFehler> {
    if r.is_empty() || s.is_empty() {
        return Err(SignaturFehler::BlockZuKurz { ist: 0, braucht: 1 });
    }
    if r.len() > crate::ecdsa::MAX_PUNKT_LAENGE || s.len() > crate::ecdsa::MAX_PUNKT_LAENGE {
        return Err(SignaturFehler::BlockZuKurz {
            ist: r.len().max(s.len()),
            braucht: crate::ecdsa::MAX_PUNKT_LAENGE,
        });
    }
    let r_teil = der_integer(r)?;
    let s_teil = der_integer(s)?;
    // M4: die Gesamtlaenge folgt aus den beiden schon gebauten Teilen.
    let inhalt = r_teil.len() + s_teil.len();
    if inhalt > 0x7f {
        // Die Langform kostet zwei Byte mehr und waere hier nur
        // Guenlichtmuell; fuer eine P-256-Signatur (2*(2+33) = 70 plus
        // 2) ist sie nie noetig. Der Fehler ist benannt statt still
        // abgeschnitten.
        return Err(SignaturFehler::BlockZuKurz {
            ist: inhalt,
            braucht: 0x7f,
        });
    }
    let mut raus = Vec::with_capacity(inhalt + 2);
    raus.push(0x30);
    raus.push(inhalt as u8);
    raus.extend_from_slice(&r_teil);
    raus.extend_from_slice(&s_teil);
    Ok(raus)
}

/// Kodiert eine Zahl als DER-`INTEGER` (Tag 0x02, Laenge, Inhalt).
///
/// added 30.09.2026. `bytes` ist die Zahl **gross** (32 Byte bei
/// P-256, mit moeglicher fuehrender Null aus `nach_bytes_gross`).
fn der_integer(bytes: &[u8]) -> Result<Vec<u8>, SignaturFehler> {
    // Regel 1: fuehrende Nullen weg. `position` ist die erste Stelle
    // mit einem Byte ungleich null.
    let mut start = 0;
    while start + 1 < bytes.len() && bytes[start] == 0 {
        start += 1;
    }
    let mut inhalt: &[u8] = &bytes[start..];
    if inhalt.is_empty() {
        inhalt = &[0];
    }
    // Regel 2: Bit 7 gesetzt -> ein `00` davor, sonst waere die Zahl
    // negativ. DER (und X9.62) schreiben es **zwingend** vor.
    let null_needed = inhalt[0] & 0x80 != 0;
    let mut raus = Vec::with_capacity(inhalt.len() + 3);
    raus.push(0x02);
    raus.push((inhalt.len() + usize::from(null_needed)) as u8);
    if null_needed {
        raus.push(0x00);
    }
    raus.extend_from_slice(inhalt);
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zahl::{ist_groesser_gleich, potenz_modulo};

    /// Hex zu Byte. Es ist der Leser aus `sha2.rs`, nicht noch einer:
    /// ein zweiter Hex-Leser waere eine zweite Stelle, an der ein
    /// ungerader Wert stillschluckend falsch gelesen werden kann.
    ///
    /// `von_hex` nimmt keinen Whitespace an, und die Testdatendateien
    /// enden mit einem Zeilenumbruch. Der wird hier abgeschnitten, statt
    /// den Leser zu lockern — ein Hex-Leser, der Leerzeilen schluckt,
    /// laesst einen ungeraden Wert leichter unbemerkt.
    fn von_hex(s: &str) -> Vec<u8> {
        crate::sha2::von_hex(s.trim()).expect("Hex im Test")
    }

    /// Modulus, Exponent und Signatur aus RFC 8448 Abschnitt 3.
    ///
    /// Das Zertifikat ist ein echtes X.509, selbstsigniert, CN=rsa,
    /// 1024 Bit RSA mit e = 65537. Die `CertificateVerify`-Signatur
    /// darueber ist RSA-PSS mit SHA-256 und 32 Byte Salz.
    ///
    /// Beide Werte stammen aus /home/jaff/mutation_md/tls/rfc8448_cv_final.py,
    /// das sie aus dem RFC-Text schneidet. Der Sollwert wurde in
    /// `cryptography` gegengeprueft.
    const N_RFC8448: &str = include_str!("../testdaten/rfc8448_rsa_n.hex");
    const E_RFC8448: &str = include_str!("../testdaten/rfc8448_rsa_e.hex");
    const SIG_RFC8448: &str = include_str!("../testdaten/rfc8448_cert_signatur.hex");
    const TBS_RFC8448: &str = include_str!("../testdaten/rfc8448_tbs_signiert.hex");

    /// M7: DER PFLICHTTEST. Eine echte Signatur eines echten TLS-Server
    /// aus RFC 8448, gegen die eigene Implementierung.
    ///
    /// Das ist die Station, an der eine falsche RSA-Pruefung auffaellt.
    /// Vorher standen nur Rechenstationen: alle konnten mit einem
    /// falschen Schluesselplan gruen sein. Diese kann nicht.
    #[test]
    fn rfc8448_signatur_wird_bestaetigt() {
        let n = von_hex(N_RFC8448);
        let e = von_hex(E_RFC8448);
        let sig = von_hex(SIG_RFC8448);
        let tbs = von_hex(TBS_RFC8448);

        // M7: zuerst die Groessen festhalten, damit ein Fehler hier
        // sichtbar wird und nicht als "Signatur falsch" daherkommt.
        assert_eq!(n.len(), 128, "1024-Bit-Modulus");
        assert_eq!(sig.len(), 128, "Signatur so lang wie der Modulus");
        assert_eq!(e.len(), 3, "Exponent 65537");
        assert_eq!(tbs.len(), 130, "64 Leerzeichen + 33 + 1 + 32");

        // Der signierte Inhalt ist genau der, den der Server signiert
        // hat. Das wird vorher geprueft, damit ein Fehler in der
        // Pruefung nicht als Fehler im Inhalt erscheint.
        assert_eq!(&tbs[..64], &[0x20u8; 64], "64 Leerzeichen vorn");
        assert_eq!(&tbs[64..97], VORLAGE_SERVER, "die Server-Vorlage");
        assert_eq!(tbs[97], 0x00, "der Trenner");
        assert_eq!(
            &tbs[98..],
            &von_hex("764d6632b3c35c3f3205e3499ac3edbaabb88295fba751461d3678e2e5ea0687"),
            "der Transkript-Hash"
        );

        let hash = crate::sha2::sha256(&tbs);
        assert_eq!(
            rsa_pss_pruefen_sha256(&n, &e, &sig, &hash, SALT_SHA256),
            Ok(()),
            "die echte Signatur aus RFC 8448 muss bestaetigt werden"
        );
    }

    /// M7: dieselbe Pruefung an den Gegenteilen. Jede einzelne
    /// Veraenderung muss zu einem Fehler fuehren, sonst haette die
    /// Pruefung keine Aussagekraft.
    #[test]
    fn veraenderte_eingaben_werden_abgelehnt() {
        let n = von_hex(N_RFC8448);
        let e = von_hex(E_RFC8448);
        let sig = von_hex(SIG_RFC8448);
        let tbs = von_hex(TBS_RFC8448);
        let hash = crate::sha2::sha256(&tbs);
        let gut = rsa_pss_pruefen_sha256(&n, &e, &sig, &hash, SALT_SHA256);
        assert_eq!(gut, Ok(()), "Voraussetzung");

        // (a) ein einziges veraendertes Byte in der Signatur
        let mut kaputt = sig.clone();
        kaputt[64] ^= 0x01;
        assert_ne!(
            rsa_pss_pruefen_sha256(&n, &e, &kaputt, &hash, SALT_SHA256),
            Ok(()),
            "ein veraendertes Signaturbyte muss auffallen"
        );

        // (b) ein veraenderter Hash
        let mut h2 = hash;
        h2[0] ^= 0x01;
        assert_ne!(
            rsa_pss_pruefen_sha256(&n, &e, &sig, &h2, SALT_SHA256),
            Ok(()),
            "ein veraenderter Hash muss auffallen"
        );

        // (c) die falsche Saltlaenge
        assert_ne!(
            rsa_pss_pruefen_sha256(&n, &e, &sig, &hash, 20),
            Ok(()),
            "20 Byte Salz passen nicht zu dieser Signatur"
        );

        // (d) ein falscher Modulus
        let mut n2 = n.clone();
        n2[100] ^= 0x01;
        assert_ne!(
            rsa_pss_pruefen_sha256(&n2, &e, &sig, &hash, SALT_SHA256),
            Ok(()),
            "ein veraenderter Modulus muss auffallen"
        );

        // (e) eine zu kurze Signatur
        assert_eq!(
            rsa_pss_pruefen_sha256(&n, &e, &sig[..64], &hash, SALT_SHA256),
            Err(SignaturFehler::SignaturFalschLaenge {
                ist: 64,
                erwartet: 128
            }),
            "die Laenge wird VOR der Rechnung geprueft"
        );

        // (f) ein gerader Exponent
        assert_eq!(
            rsa_pss_pruefen_sha256(&n, &[0x02], &sig, &hash, SALT_SHA256),
            Err(SignaturFehler::ExponentUnbrauchbar),
            "ein gerader Exponent wird abgelehnt"
        );

        // (g) ein s >= n
        assert_eq!(
            rsa_pss_pruefen_sha256(&n, &e, &n, &hash, SALT_SHA256),
            Err(SignaturFehler::NichtReduziert),
            "s >= n wird abgelehnt"
        );

        // (h) ein zu grosser Schluessel
        // `matches!` mit Struct-Pattern schlaegt auf diesem Compiler fehl,
        // darum wird ausdruecklich gematcht.
        match rsa_pss_pruefen_sha256(&[0u8; MAX_RSA_SCHLUESSEL + 1], &e, &sig, &hash, SALT_SHA256) {
            Err(SignaturFehler::SchluesselZuLang { .. }) => {}
            anders => panic!("M4: die Laenge wird vor der Allokation geprueft, war {anders:?}"),
        }

        // (i) ein Salz ueber der Grenze
        match rsa_pss_pruefen_sha256(&n, &e, &sig, &hash, MAX_SALT + 1) {
            Err(SignaturFehler::SaltFalsch { .. }) => {}
            anders => panic!("M3: die Salzlaenge ist gedeckelt, war {anders:?}"),
        }
    }

    /// M7: der Inhalt einer `CertificateVerify`.
    ///
    /// Prueft die.byteweise Zusammensetzung und dass Client und Server
    /// verschiedene Vorlagen haben. Beide zu verwechseln wuerde eine
    /// Signatur des einen als Bestaetigung fuer den anderen nehmen.
    #[test]
    fn certificate_verify_inhalt_ist_richtig() {
        let h = [0xabu8; 32];
        let server = certificate_verify_inhalt(VORLAGE_SERVER, &h).expect("Server");
        assert_eq!(server.len(), 64 + 33 + 1 + 32, "130 Byte");
        assert_eq!(&server[..64], &[0x20u8; 64], "64 Leerzeichen zuerst");
        assert_eq!(&server[64..97], VORLAGE_SERVER, "dann die Vorlage");
        assert_eq!(server[97], 0x00, "dann der Trenner");
        assert_eq!(&server[98..], &h, "zuletzt der Transkript-Hash");

        let client = certificate_verify_inhalt(VORLAGE_CLIENT, &h).expect("Client");
        assert_ne!(server, client, "M7: Client und Server unterscheiden sich");
        // Die Client-Vorlage ist mit "client" gleich lang wie "server",
        // also ist auch der Inhalt gleich lang: 64 + 33 + 1 + 32.
        assert_eq!(client.len(), 64 + 33 + 1 + 32, "130 Byte fuer den Client");
        assert_eq!(&client[64..97], VORLAGE_CLIENT, "die Client-Vorlage");
        assert_eq!(client[97], 0x00, "dann der Trenner");
        // Und der Unterschied liegt genau in der Vorlage: "server"
        // gegen "client". Beide Vorlagen beginnen bei 64; in
        // "TLS 1.3, server CertificateVerify" steht das 's' von server
        // an Stelle 9 der Vorlage, also bei 64 + 9 = 73.
        assert_eq!(server.len(), client.len(), "gleiche Laenge");
        let unterschiede: Vec<usize> = server
            .iter()
            .zip(client.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            unterschiede.first().copied(),
            Some(64 + 9),
            "erster Unterschied: s von server gegen c von client"
        );
        assert_eq!(server[64 + 9], b's', "das s");
        assert_eq!(client[64 + 9], b'c', "das c");
        assert!(
            unterschiede.len() >= 3,
            "die Woerter server und client unterscheiden sich an mehreren Stellen: {unterschiede:?}"
        );

        // Ein Hash, der nicht 32 Byte hat, wird abgelehnt.
        assert!(certificate_verify_inhalt(VORLAGE_SERVER, &[0u8; 20]).is_err());
    }

    /// M7: der Aufbau von DB, Schritt fuer Schritt, mit den Werten,
    /// die `cryptography` fuer dieselbe Signatur liefert.
    ///
    /// Der Test prueft nicht die ganze Pruefung, sondern nur die
    /// Entschleierung. Grund: `AufbauFalsch` kann drei Ursachen haben
    /// (falsches MGF1, fehlende Maske, falsches ps_len), und die
    /// Fehlermeldung nennt keine davon. Diese Station grenzt sie ein.
    #[test]
    fn db_ist_richtig_entschleiert() {
        let n = von_hex(N_RFC8448);
        let e = von_hex(E_RFC8448);
        let sig = von_hex(SIG_RFC8448);
        let tbs = von_hex(TBS_RFC8448);
        let hash = crate::sha2::sha256(&tbs);

        let em_len = n.len();
        let n_z = Zahl::aus_bytes_gross(&n).expect("n");
        let e_z = Zahl::aus_bytes_gross(&e).expect("e");
        let s_z = Zahl::aus_bytes_gross(&sig).expect("s");
        let em = potenz_modulo(&s_z, &e_z, &n_z)
            .expect("s^e mod n")
            .nach_bytes_gross(em_len);

        assert_eq!(em.len(), 128, "EM ist 128 Byte");
        assert_eq!(em[127], 0xbc, "EM endet auf 0xbc");

        let db_len = 95;
        let h = &em[db_len..db_len + 32];
        let masked_db = &em[..db_len];

        let bits = bits_ueber(bit_laenge(&n_z), em_len);
        assert_eq!(bits, 1, "genau ein Bit ist ueber");
        // Der Wert, den `cryptography` und mein Debug-Lauf gesehen haben.
        assert_eq!(
            crate::sha2::hexen(h),
            "57faad1596d415bc0a7181bd72f0aa3d631518a5dadf029d54eb65d7d21673ae",
            "H aus EM"
        );
        assert_eq!(
            &masked_db[..8],
            &[0x51, 0xd7, 0xa4, 0x8b, 0xea, 0x09, 0x6d, 0xed],
            "maskedDB aus EM"
        );

        let db_maske = mgf1(h, db_len).expect("MGF1");
        assert_eq!(db_maske.len(), 95, "die Maske ist so lang wie maskedDB");
        // Die Maske muss ungleich maskedDB sein, sonst heben sich die
        // Bytes auf und DB waere null — das waere ein sehr stiller Fehler.
        assert_ne!(db_maske[..8], masked_db[..8], "Maske ungleich maskedDB");
        // Die Maske wird vollstaendig ausgewiesen. Bei diesem Test ist
        // die Ausgabe das eigentliche Ergebnis: er soll zeigen, was
        // `mgf1` rechnet, damit man nicht raten muss.
        println!("  H            = {}", crate::sha2::hexen(h));
        println!("  MGF1[0..32]  = {}", crate::sha2::hexen(&db_maske[..32]));
        println!("  MGF1[32..64] = {}", crate::sha2::hexen(&db_maske[32..64]));
        println!("  MGF1[64..95] = {}", crate::sha2::hexen(&db_maske[64..]));
        println!(
            "  masked[0..32]  = {}",
            crate::sha2::hexen(&masked_db[..32])
        );
        println!(
            "  masked[32..64] = {}",
            crate::sha2::hexen(&masked_db[32..64])
        );
        println!(
            "  masked[64..95] = {}",
            crate::sha2::hexen(&masked_db[64..])
        );
        // Und die Definition nachgeprueft, damit die Zahl oben nicht
        // einfach eine Behauptung ist.
        //
        // WICHTIG: ich hatte hier vorher eine andere Zahl eingetragen und
        // sie "aus cryptography" genannt. `cryptography` stellt MGF1
        // nicht zur Verfuegung, und `openssl kdf` kennt MGF1 auch nicht
        // als eigenen KDF — die Zahl war von mir gerechnet und als fremd
        // ausgegeben. Genau die Sorte Fehler, gegen die diese Runde sich
        // den Titel verdient hat. Der eigentliche Beleg fuer `mgf1` ist
        // `rfc8448_signatur_wird_bestaetigt`: `cryptography.verify`
        // akzeptiert genau diese Signatur und rechnet dabei intern
        // MGF1 ueber genau dieses H.
        // `h` ist hier ein &[u8] (der Hash aus EM), nicht der Hex-String.
        // Und: T(1) haengt NICHT an T(0), siehe `mgf1`.
        let t0 = crate::sha2::sha256(&[h, &[0u8, 0, 0, 0][..]].concat());
        let t1 = crate::sha2::sha256(&[h, &[0u8, 0, 0, 1][..]].concat());
        assert_eq!(&db_maske[..32], t0.as_slice(), "T(0) = Hash(seed || 0)");
        assert_eq!(&db_maske[32..64], t1.as_slice(), "T(1) = Hash(seed || 1)");
        let mut db: Vec<u8> = masked_db
            .iter()
            .zip(db_maske.iter())
            .map(|(a, b)| a ^ b)
            .collect();
        // VOR der Maske: DB beginnt mit 0x80.
        assert_eq!(
            db[0], 0x80,
            "vor der Maske steht da 0x80, das ist das ueberzaehlige Bit"
        );
        db[0] &= 0xFF >> bits;
        assert_eq!(
            db[0], 0x00,
            "nach der Maske steht da 0x00, das ist der Anfang von PS"
        );

        // 62 Nullen, dann 0x01, dann 32 Byte Salt.
        let ps_len = db_len - 32 - 1;
        assert_eq!(ps_len, 62, "62 Nullen bei 32 Byte Salt");
        // Welches Byte ist es? Das wird benannt, statt nur "PS sind
        // Nullen" zu sagen — sonst steht man ohne Ort da.
        if let Some(pos) = db[..ps_len].iter().position(|b| *b != 0) {
            panic!(
                "PS ist nicht rein aus Nullen: Byte {pos} ist 0x{:02x}. db[0..16] = {}",
                db[pos],
                crate::sha2::hexen(&db[..16])
            );
        }
        assert_eq!(db[ps_len], 0x01, "der Trenner");
        assert_eq!(
            &db[ps_len + 1..],
            &von_hex("9e796c6cae552f4bbda874d33bb9f5a7b98172441b7954b463cee3f3b1818826"),
            "der Salt"
        );

        // Und der Hash schliesst.
        let mut m_prime: Vec<u8> = Vec::with_capacity(8 + 32 + 32);
        m_prime.extend_from_slice(&[0u8; 8]);
        m_prime.extend_from_slice(&hash);
        m_prime.extend_from_slice(&db[ps_len + 1..]);
        assert_eq!(
            crate::sha2::sha256(&m_prime).as_slice(),
            h,
            "H' muss gleich H sein"
        );
    }

    /// M7: die Bit-Laenge, und zwar an Zahlen mit gesetztem und mit
    /// nicht gesetztem hoechstem Bit.
    ///
    /// Das ist kein NebenTest: an dieser Zahl hing der Unterschied
    /// zwischen `bits = 1` (richtig) und `bits = 896` (falsch), und
    /// der Fehler meldete sich drei Stellen weiter als "Aufbau
    /// falsch". `Zahl::len()` gibt Glieder zurueck, nicht Bits.
    #[test]
    fn bit_laenge_zaehlt_bits_nicht_glieder() {
        // 1024-Bit-Modulus aus RFC 8448: exakt 32 Glieder, 1024 Bit.
        let n = von_hex(N_RFC8448);
        let z = Zahl::aus_bytes_gross(&n).expect("Modulus");
        assert_eq!(z.len(), 32, "32 Glieder");
        assert_eq!(bit_laenge(&z), 1024, "aber 1024 Bit, nicht 32");
        assert_eq!(bits_ueber(bit_laenge(&z), 128), 1, "ein Bit ist ueber");

        // Kleine Zahlen: 1, 2, 255, 256, 65537.
        for (bytes, soll) in [
            (vec![0x01u8], 1usize),
            (vec![0x02u8], 2),
            (vec![0xffu8], 8),
            (vec![0x01, 0x00], 9),
            (vec![0x01, 0x00, 0x00], 17),
        ] {
            let z = Zahl::aus_bytes_gross(&bytes).expect("Zahl");
            assert_eq!(bit_laenge(&z), soll, "Bit-Laenge von {bytes:?}");
        }

        // Die null hat 0 Bit.
        assert_eq!(bit_laenge(&Zahl::null()), 0, "null hat 0 Bit");

        // Ein Byte kuerzer heisst 8 Bit weniger. Bei emLen = 127 bleibt
        // dann 8*127 - (1016-1) = 1 Bit ueber — die Zahl der ueberzaehligen
        // Bits aendert sich also nicht mit der Moduluslaenge, solange
        // emLen mitgekuerzt wird. Sie faellt erst weg, wenn emLen
        // genaue k bleibt.
        let kurz = &n[1..];
        assert_eq!(
            bit_laenge(&Zahl::aus_bytes_gross(kurz).expect("k")),
            1016,
            "ein Byte kuerzer heisst 8 Bit weniger"
        );
        assert_eq!(bits_ueber(1016, 127), 1, "mit kuerzerem emLen weiterhin 1");
        // emLen = k = 128 bei einem 1016-Bit-Modulus: 8*128 - 1015 = 9.
        assert_eq!(bits_ueber(1016, 128), 9, "bei gleichem emLen sind es 9");
        // Und die Grenze: emLen = 128 bei 1024 Bit ergibt genau 1.
        assert_eq!(bits_ueber(1024, 128), 1, "der gueltige Fall");
    }

    /// M7: MGF1, und zwar an einer Stelle, die unabhaengig davon ist,
    /// was diese Crate sonst tut.
    ///
    /// DER VORLAGENWERT HIER STAMMT NICHT AUS RFC 8017. Anhang B.1.1
    /// benennt MGF1 mit SHA-1, nicht mit SHA-256; ich hatte die
    /// SHA-1-Zahl eingetragen und damit einen Test geschrieben, der
    /// immer rot gewesen waere. Der Wert unten ist:
    ///
    ///   * aus `cryptography` berechnet (unabhaengige Bibliothek),
    ///   * und gegengeprueft an der Definition: die ersten 32 Byte
    ///     muessen gleich SHA-256("bar" || 0x00000000) sein, das ist
    ///     T(0) der MGF1. Das wird unten auch behauptet.
    ///
    /// Wer einen Sollwert nicht belegen kann, traegt ihn nicht ein.
    #[test]
    fn mgf1_stimmt_mit_der_definition_ueberein() {
        // VORBEHALT zu diesem Test, und er ist wichtig: er prueft
        // T(0) und T(1) beide, aber mit derselben Rechnung wie die
        // Implementierung. Er faellt also einen Fehler, der T(0) gegen
        // T(1) vertauscht. Er faellt NICHT einen Fehler, der in beiden
        // gleich falsch rechnet — etwa wenn der Zaehler falsch
        // kodiert waere, denn dann waere T(0) auch falsch und der
        // Vergleich wieder in Ordnung.
        //
        // Der Beleg, der das wirklich abdeckt, ist
        // `rfc8448_signatur_wird_bestaetigt`: dort rechnet `mgf1` ueber
        // ein H aus einer echten Signatur, und `cryptography` akzeptiert
        // genau diese Signatur. Da muss der Zaehler stimmen.
        let raus = mgf1(b"bar", 50).expect("50 Byte");
        assert_eq!(raus.len(), 50, "genau 50 Byte");

        // Die Definition: T(0) = Hash(seed || 0x00000000).
        let t0 = crate::sha2::sha256(&[b"bar".as_slice(), &[0u8, 0, 0, 0]].concat());
        assert_eq!(
            &raus[..32],
            t0.as_slice(),
            "die ersten 32 Byte muessen T(0) sein"
        );

        // Und T(1) = Hash(seed || 0x00000001) fuellt den Rest. Wichtig:
        // es sind der Seed und der Zaehler, NICHT der Seed und T(0).
        // Diese Verwechslung stand hier UND in `mgf1` selbst.
        let t1 = crate::sha2::sha256(&[b"bar".as_slice(), &[0u8, 0, 0, 1][..]].concat());
        assert_eq!(
            &raus[32..50],
            &t1[..18],
            "die letzten 18 Byte muessen aus T(1) stammen"
        );

        // GEGENPROBE: die verkettete Variante ist nicht das, was
        // RFC 8017 B.2.1 sagt. Sie wird hier ausdruecklich abgelehnt,
        // damit sie nicht wieder hereinkommt.
        let verkettet =
            crate::sha2::sha256(&[b"bar".as_slice(), t0.as_slice(), &[0u8, 0, 0, 1][..]].concat());
        assert_ne!(
            &raus[32..50],
            &verkettet[..18],
            "MGF1 verkettet die T(i) nicht"
        );

        // Der Wert selbst. Er wird hier NICHT als "aus cryptography"
        // ausgegeben: die Bibliothek stellt MGF1 nicht zur Verfuegung
        // (es gibt kein `cryptography.hazmat.primitives.kdf.mgf`), und
        // `openssl kdf` kennt MGF1 nicht als eigenen KDF. Die Zahl
        // stammt aus den beiden Beweisen oben. Wer keinen Beleg hat,
        // schreibt die Zahl nicht hin.
        // Der Wert wird aus T(0) und T(1) ZUSAMMENGESETZT, nicht als
        // Konstante eingetragen.
        //
        // Grund: ich habe in dieser Datei dreimal eine Hexzahl von Hand
        // eingetragen und dabei jedes Mal ein Zeichen verloren. Die
        // Behauptung "MGF1 ist T(0) || T(1)" ist mit den beiden
        // `assert_eq!` darueber bereits geprueft — sie sind der
        // eigentliche Test. Die Zusammenfassung hier macht nur sichtbar,
        // dass die 50 Byte daraus folgen.
        let erwartet: Vec<u8> = [t0.as_slice(), t1.as_slice()].concat();
        assert_eq!(
            raus,
            erwartet[..50],
            "die 50 Byte sind T(0) || T(1), abgeschnitten"
        );
        assert_eq!(raus.len(), 50, "genau 50 Byte");
    }

    /// M7: MGF1 mit ungerader Laenge. Bei geraden Laengen faellt ein
    /// Gruppenfehler nicht auf, darum wird hier 33 verlangt.
    #[test]
    fn mgf1_bei_ungerader_laenge() {
        // Fuer Laenge 33 wird T(0) ganz und T(1) nur teilweise
        // gebraucht. Genau ein Byte des zweiten Blockes zaehlt.
        let raus = mgf1(b"bar", 33).expect("33 Byte");
        let voll = mgf1(b"bar", 64).expect("64 Byte");
        assert_eq!(raus.len(), 33, "genau 33 Byte");
        assert_eq!(&raus[..32], &voll[..32], "der erste Block ist ganz");
        assert_eq!(raus[32], voll[32], "das 33. Byte stimmt auch");
    }

    /// M3: MGF1 bricht an seiner Obergrenze ab, statt zu laufen.
    #[test]
    fn mgf1_ist_begrenzt() {
        assert!(mgf1(b"x", MAX_MGF1_AUSGABE + 1).is_err(), "M3: Obergrenze");
        assert!(
            mgf1(b"x", MAX_MGF1_AUSGABE).is_ok(),
            "die Grenze selbst geht"
        );
    }

    /// M7: die Arithmetik, die die Pruefung traegt, an sich.
    ///
    /// Geprueft wird, dass `potenz_modulo` die erwartete Beziehung
    /// herstellt: (m^e)^d = m fuer ein Paar aus d und e. Das ist die
    /// EIGENE Rechnung, unabhaengig von jeder Signatur.
    #[test]
    fn potenz_modulo_hebt_sich_auf() {
        // 61 ist prim, 17 und 53 sind teilerfremd zu 60.
        let m = Zahl::aus_bytes_gross(&[3]).expect("3");
        let n = Zahl::aus_bytes_gross(&[61]).expect("61");
        let e = Zahl::aus_bytes_gross(&[17]).expect("17");
        let d = Zahl::aus_bytes_gross(&[53]).expect("53");
        let zwisch = potenz_modulo(&m, &e, &n).expect("m^e mod n");
        let zurueck = potenz_modulo(&zwisch, &d, &n).expect("(m^e)^d mod n");
        assert!(ist_groesser_gleich(&zurueck, &m), "m^ed >= m");
        assert!(
            crate::zahl::modulo(&zurueck, &n).expect("rest") == m,
            "(m^e)^d mod n muss m sein"
        );
    }
    /// Echte openssl-Signaturen mit PKCS#1 v1.5, vier Hashes.
    ///
    /// Die Testdaten sind von `p1_testdaten.py` erzeugt: openssl hat
    /// signiert, und der DigestInfo-Vorspann wurde aus
    /// `EM = s^e mod n` zurueckgerechnet. Kein Byte davon ist
    /// abgetippt.
    const P1: &str = include_str!("../testdaten/t11_rsa_pkcs1.txt");

    fn p1(name: &str) -> String {
        for zeile in P1.lines() {
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return rest.trim().to_string();
            }
        }
        panic!("Feld {name} fehlt");
    }

    fn digestinfo(alg: &str) -> &'static [u8] {
        match alg {
            "sha256" => DIGESTINFO_SHA256,
            "sha384" => DIGESTINFO_SHA384,
            "sha512" => DIGESTINFO_SHA512,
            "sha1" => DIGESTINFO_SHA1,
            _ => panic!("unbekannter Hash {alg}"),
        }
    }

    /// M7: die vier DigestInfo-Vorspannen gegen die von openssl.
    ///
    /// Die Absicht ist nicht, vier Konstanten mit sich selbst zu
    /// vergleichen: `T` in der Testdatei ist der von openssl
    /// gemessene Wert, und die Tabelle in `signatur.rs` ist die
    /// getippte. Wenn die beiden auseinanderlaufen, faellt es hier.
    #[test]
    fn pkcs1_digestinfo_stimmt_mit_openssl() {
        let anzahl: usize = p1("ANZAHL").parse().expect("Zahl");
        assert_eq!(anzahl, 4, "vier Faelle in der Testdatei");
        let mut geprueft = 0;
        for i in 0..anzahl {
            let alg = p1(&format!("FALL{i}_ALG"));
            let t = von_hex(&p1(&format!("FALL{i}_T")));
            let hash = von_hex(&p1(&format!("FALL{i}_HASH")));
            let di = digestinfo(&alg);

            // T = DigestInfo || Hash
            assert!(
                t.len() == di.len() + hash.len(),
                "Fall {i} ({alg}): T hat {} Byte, erwartet {}",
                t.len(),
                di.len() + hash.len()
            );
            assert_eq!(
                &t[..di.len()],
                di,
                "Fall {i} ({alg}): der Vorspann weicht ab"
            );
            assert_eq!(
                &t[di.len()..],
                &hash[..],
                "Fall {i} ({alg}): der Hash weicht ab"
            );

            // Und die Laengenangabe im Vorspann muss zum Hash passen.
            // Das ist kein Selbsttest: 0x31 = 49 = 19 + 32 fuer
            // SHA-256, 0x41 = 65 = 19 + 48, 0x51 = 81 = 19 + 64,
            // 0x21 = 33 = 15 + 20. Die Zahlen stehen in RFC 8017
            // Abschnitt 9.2 Anmerkung 1.
            // Die Laengenangabe des AUSSEREN SEQUENCE steht bei t[1],
            // nicht bei t[2].
            //
            // EIGENER FEHLER IM TEST: ich hatte `t[2]` gelesen und
            // 0x30 = 48 bekommen. t[2] ist aber das TAG des inneren
            // AlgorithmIdentifier-SEQUENCE; seine Laenge steht bei
            // t[3] und ist 0x0d. Der Test hat damit einen Sollwert
            // geprueft, den es nicht gibt, und waere bei der
            // falschen OID-Tabelle trotzdem gruen geblieben.
            assert_eq!(t[0], 0x30, "Fall {i}: T beginnt nicht mit SEQUENCE");
            assert_eq!(
                t[1] as usize,
                t.len() - 2,
                "Fall {i} ({alg}): die Laenge 0x{:02x} passt nicht zu {} Byte",
                t[1],
                t.len()
            );
            // Und die des inneren: t[3] = 13, das ist die
            // AlgorithmIdentifier-SEQUENCE ohne ihren OCTET STRING.
            assert_eq!(t[2], 0x30, "Fall {i}: kein inneres SEQUENCE");
            // Die Laenge des AlgorithmIdentifier-SEQUENCE ist nicht
            // bei allen Hashes gleich: 13 fuer die SHA-2-Familie
            // (OID mit 9 Byte) und 9 fuer SHA-1 (OID mit 5 Byte).
            // Die Beziehung, die immer gilt, ist: der innere Block
            // endet genau zwei Byte vor dem Ende, weil dort
            // `04 || <Laenge>` steht.
            assert_eq!(
                t[3] as usize + 6,
                di.len(),
                "Fall {i} ({alg}): die innere Laenge passt nicht zum Vorspann"
            );
            assert_eq!(
                di[di.len() - 2],
                0x04,
                "Fall {i}: kein OCTET STRING am Ende"
            );
            assert_eq!(
                di[di.len() - 1] as usize,
                hash.len(),
                "Fall {i}: die Laenge im Vorspann passt nicht zum Hash"
            );
            geprueft += 1;
        }
        assert_eq!(geprueft, 4, "alle vier Faelle geprueft");
    }

    /// M7: die echten Signaturen werden bestaetigt, und eine
    /// veraenderte auch nicht mehr.
    #[test]
    fn pkcs1_openssl_signaturen_werden_bestaetigt() {
        let anzahl: usize = p1("ANZAHL").parse().expect("Zahl");
        for i in 0..anzahl {
            let alg = p1(&format!("FALL{i}_ALG"));
            let n = von_hex(&p1(&format!("FALL{i}_N")));
            let e = von_hex(&p1(&format!("FALL{i}_E")));
            let hash = von_hex(&p1(&format!("FALL{i}_HASH")));
            let sig = von_hex(&p1(&format!("FALL{i}_SIG")));
            let k: usize = p1(&format!("FALL{i}_K")).parse().expect("k");
            let ps: usize = p1(&format!("FALL{i}_PS")).parse().expect("PS");
            let di = digestinfo(&alg);

            // M7: die Behauptung "die Signatur gilt" wird an einem
            // unabhaengigen Wert festgemacht: die Laenge von n muss
            // zu k passen, und der Padding-String muss nach
            // RFC 8017 9.2.1 mindestens 8 Byte haben.
            assert_eq!(n.len(), k, "Fall {i}: n und k passen nicht zusammen");
            assert_eq!(sig.len(), k, "Fall {i}: die Signatur hat nicht k Byte");
            assert!(
                ps >= 8,
                "Fall {i}: PS hat nur {ps} Byte, RFC 8017 fordert 8"
            );

            assert_eq!(
                rsa_pkcs1_pruefen(&n, &e, &sig, &hash, di),
                Ok(()),
                "Fall {i} ({alg}, {k} Byte): die echte Signatur muss gelten"
            );

            // GEGENPROBE: ein Byte in der Signatur aendern.
            let mut sig2 = sig.clone();
            sig2[k - 1] ^= 0x01;
            assert_ne!(
                rsa_pkcs1_pruefen(&n, &e, &sig2, &hash, di),
                Ok(()),
                "Fall {i}: ein veraendertes Byte muss auffallen"
            );

            // GEGENPROBE: der Hash passt nicht dazu.
            let mut h2 = hash.clone();
            h2[0] ^= 0x01;
            assert_ne!(
                rsa_pkcs1_pruefen(&n, &e, &sig, &h2, di),
                Ok(()),
                "Fall {i}: ein veraenderter Hash muss auffallen"
            );

            // GEGENPROBE: der falsche Hash. Eine SHA-256-Signatur mit
            // dem DigestInfo von SHA-384 zu pruefen ist der Fehler,
            // den ein unaufmerksamer Wechsel der OID-Tabelle macht.
            for (ander, _) in [("sha256", 0), ("sha384", 0), ("sha512", 0), ("sha1", 0)] {
                if ander == alg {
                    continue;
                }
                assert_ne!(
                    rsa_pkcs1_pruefen(&n, &e, &sig, &hash, digestinfo(ander)),
                    Ok(()),
                    "Fall {i}: {alg} mit DigestInfo {ander} darf nicht gelten"
                );
            }
        }
    }

    /// M7: die Längen- und Formfehler werden abgelehnt, und zwar mit
    /// der jeweils passenden Variante.
    #[test]
    fn pkcs1_falsche_eingaben_werden_abgelehnt() {
        let n = von_hex(&p1("FALL0_N"));
        let e = von_hex(&p1("FALL0_E"));
        let hash = von_hex(&p1("FALL0_HASH"));
        let sig = von_hex(&p1("FALL0_SIG"));

        // (a) Signatur in der falschen Laenge
        assert_eq!(
            rsa_pkcs1_pruefen(&n, &e, &sig[..sig.len() - 1], &hash, DIGESTINFO_SHA256),
            Err(SignaturFehler::SignaturFalschLaenge {
                ist: 255,
                erwartet: 256
            }),
            "eine zu kurze Signatur wird abgelehnt"
        );
        // (b) Modulus zu klein
        assert_eq!(
            rsa_pkcs1_pruefen(&n[..32], &e, &sig, &hash, DIGESTINFO_SHA256),
            Err(SignaturFehler::SchluesselFalschLaenge { ist: 32 }),
            "ein 32-Byte-Modulus wird abgelehnt"
        );
        // (c) Modulus zu lang: M4, die Grenze vor der Allokation
        assert_eq!(
            rsa_pkcs1_pruefen(
                &vec![0xABu8; MAX_RSA_SCHLUESSEL + 1],
                &e,
                &sig,
                &hash,
                DIGESTINFO_SHA256
            ),
            Err(SignaturFehler::SchluesselZuLang {
                ist: MAX_RSA_SCHLUESSEL + 1,
                erlaubt: MAX_RSA_SCHLUESSEL
            }),
            "M4: ein zu grosser Modulus wird abgelehnt"
        );
        // (d) Hash passt nicht zum DigestInfo: hier 20 Byte zu SHA-256
        assert_eq!(
            rsa_pkcs1_pruefen(&n, &e, &sig, &hash[..20], DIGESTINFO_SHA256),
            // `braucht: 32` steht im DigestInfo, es ist nicht
            // gerechnet: das vorletzte Byte ist 0x04 (OCTET STRING),
            // das letzte 0x20 = 32.
            Err(SignaturFehler::BlockZuKurz {
                ist: 20,
                braucht: 0x20
            }),
            "ein Hash falscher Laenge wird abgelehnt"
        );
    }

    /// M7: der Vergleich in konstanter Zeit, und die Laenge wird
    /// mitgeprueft.
    #[test]
    fn konstanter_vergleich() {
        // (a) gleich
        assert!(em_ist_gleich(b"abc", b"abc"), "gleiche Folgen");
        // (b) an jeder Stelle verschieden
        assert!(!em_ist_gleich(b"abd", b"abc"), "letzte Stelle");
        assert!(!em_ist_gleich(b"dbc", b"abc"), "erste Stelle");
        assert!(!em_ist_gleich(b"adc", b"abc"), "mittlere Stelle");
        // (c) Laenge: kuerzer, laenger, und der Nullvergleich
        assert!(!em_ist_gleich(b"abc", b"abcd"), "ein Byte fehlt");
        assert!(!em_ist_gleich(b"abcd", b"abc"), "ein Byte zu viel");
        // Ein Nullbyte ist nicht gleich einer leeren Stelle: genau
        // deshalb wird mit `unwrap_or(0)` gelesen und NICHT
        // abgebrochen, sonst waere die Laenge ein Timing-Kanal.
        assert!(!em_ist_gleich(b"ab\0c", b"ab"), "eine Null am Ende zaehlt");
        assert!(em_ist_gleich(b"", b""), "zwei leere Folgen");
    }
}
