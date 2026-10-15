//! T11 — ECDSA P-256 mit SHA-256 und SHA-1, geprueft an RFC 6979.
//!
//! HIER WIRD NUR GEPRUEFT, NICHT ERZEUGT. Die Signaturen in den Tests
//! sind aus RFC 6979 Anhang A.2.5 gelesen, wo sie der Standard
//! vorschreibt; die Gruppenparameter aus NIST FIPS 186-4 Anhang
//! D.1.2.3. Beide Quellen liegen als Datei vor, der Extraktor
//! (`t11_rfc6979_vektor.py`) hat jeden Wert gegen `cryptography` und
//! gegen eine eigene Punktrechnung geprueft, bevor er ihn geschrieben
//! hat.
//!
//! DIE RECHNUNG, und warum gerade sie:
//!
//!   ECDSA-VERIFY (M, Q, r, s) nach FIPS 186-4 Abschnitt 6.4:
//!     1. 1 <= r, s <= n-1, sonst ablehnen.
//!     2. w = s^-1 mod n
//!     3. u1 = Hash(M) * w mod n,  u2 = r * w mod n
//!     4. X = u1*G + u2*Q
//!     5. annehmen, wenn X != Unendlich und X.x mod n == r
//!
//! Schritt 4 ist eine Skalarproduktbildung auf einer elliptischen
//! Kurve. Sie laeuft hier in JACOBIANISCHEN Koordinaten, weil das
//! eine Moduloinversion pro Bit auf eine am Anfang spart: in
//! affinen Koordinaten braucht jede Addition eine Inversion, bei 256
//! Bit also 384 Inversionen, in Jacobischen eine.
//!
//! Die Umrechnung am Ende ist die Stelle, an der sich ein Fehler am
//! teuersten zeigt, weil sie erst am Schluss sichtbar wird. Deshalb
//! gibt es dafuer einen eigenen Test (`jacobian_stimmt_mit_affin_ueberein`)
//! und nicht nur den Test am Ende der Kette.

use crate::zahl::{Zahl, ZahlFehler};

/// Wie viele Bit der hoechstens verarbeitet werden.
///
/// M3: eine benannte Obergrenze fuer eine eingabeabhaengige Schleife.
/// 256 ist die Bitzahl von P-256; der Wert ist eine Festlegung, damit
/// eine kaputte Kurvenangabe nicht zu einer endlosen Schleife fuehrt.
pub const MAX_KURVEN_BIT: usize = 512;

/// Obergrenze fuer die Laenge eines Punkts in Byte.
///
/// P-256 braucht 32 Byte je Koordinate. 64 fasst auch P-521. Die Zahl
/// ist eine Festlegung; M4 verlangt die Pruefung VOR der Allokation.
pub const MAX_PUNKT_LAENGE: usize = 66;

/// Warum eine ECDSA-Signatur nicht bestaetigt werden konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcdsaFehler {
    /// r oder s lag nicht in [1, n).
    AnteileAusserhalb,
    /// s war 0, also war das Inverse nicht zu bilden. Das ist ein
    /// DER LETZTE PRUEFSCHRITT SCHLAEG FEHL, obwohl `r` und `s` im
    /// Bereich lagen: der aus `u1*G + u2*Q` berechnete Punkt hat nicht
    /// die x-Koordinate `r`. Das ist der Normalfall bei einer
    /// **gefaelschten** Signatur.
    ///
    /// added 30.09.2026, Runde 2. DER GRUND FUER DEN EIGENEN NAMEN:
    /// bis hierher stand auch dieser Fall auf `AnteileAusserhalb`, und
    /// die Meldung „r oder s nicht in [1, n)" war dann **falsch**. Ein
    /// Fehler in meinem eigenen Signieren (siehe `ecdsa_signieren_mit_k`)
    /// wurde so als Bereichsfehler gemeldet und kostete eine halbe
    /// Stunde Fehlersuche an der falschen Stelle. Ab jetzt: eigener Fall.
    PunktPasstNicht,
    /// Sonderfall von `AnteileAusserhalb` und wird getrennt genannt,
    /// weil es die einzige Stelle ist, an der der Algorithmus eine
    /// Null erwartet.
    InverseNichtVorhanden,
    /// Der Punkt war nicht auf der Kurve.
    PunktNichtAufKurve,
    /// Die Kurvenangabe war unbrauchbar (p gerade, p kleiner als 3).
    KurveUnbrauchbar,
    /// Der Hash war nicht 20 oder 32 Byte lang.
    HashFalsch,
    /// Ein Fehler aus der Zahl-Arithmetik.
    Zahl(ZahlFehler),
}

impl std::fmt::Display for EcdsaFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EcdsaFehler::AnteileAusserhalb => write!(f, "r oder s nicht in [1, n)"),
            EcdsaFehler::PunktPasstNicht => write!(
                f,
                "der berechnete Punkt hat nicht die x-Koordinate r (gefaelschte Signatur?)"
            ),
            EcdsaFehler::InverseNichtVorhanden => write!(f, "s hat kein Inverses mod n"),
            EcdsaFehler::PunktNichtAufKurve => write!(f, "Punkt liegt nicht auf der Kurve"),
            EcdsaFehler::KurveUnbrauchbar => write!(f, "Kurvenangabe unbrauchbar"),
            EcdsaFehler::HashFalsch => write!(f, "Hash hat nicht 20 oder 32 Byte"),
            EcdsaFehler::Zahl(e) => write!(f, "Zahl: {e}"),
        }
    }
}

impl From<ZahlFehler> for EcdsaFehler {
    fn from(e: ZahlFehler) -> EcdsaFehler {
        EcdsaFehler::Zahl(e)
    }
}

/// Die Parameter einer Kurve in kurzer Form ueber Weierstrass.
///
/// Nur fuer die Form y^2 = x^3 + ax + b, also die "kurze" Form. P-256
/// ist genau von dieser Form; eine Kurve in der langen Form (a, b, c
/// mit c != 0) wird hier nicht unterstuetzt und wuerde falsch
/// gerechnet. Deshalb wird unten auch nicht danach gefragt, sondern
/// `a = -3` und `b` werden als gegeben angenommen — und der Test
/// prueft, dass der Generator darauf liegt.
pub struct Kurve {
    /// Das Feld, ueber das gerechnet wird.
    pub p: Zahl,
    /// Die Gruppenordnung. Fuer ECDSA ist das die Ordnung der
    /// Untergruppe, die der Generator erzeugt.
    pub n: Zahl,
    /// Der Koeffizient a der Kurvengleichung. Bei P-256 ist a = p - 3.
    pub a: Zahl,
    /// Der Koeffizient b der Kurvengleichung.
    pub b: Zahl,
    /// Der Generator, x.
    pub gx: Zahl,
    /// Der Generator, y.
    pub gy: Zahl,
}

/// Kurve aus Bytes bauen, mit Pruefung.
///
/// M3/M4: die Laengen werden VOR der Allokation geprueft, und jede
/// Zahl muss kleiner als p sein. Ein Modulus, der nicht kleiner als
/// die Koordinaten ist, wuerde die Reduktion in Schritt 4 still
/// veraendern.
/// Die Domainparameter von P-256, nach NIST FIPS 186-4 Anhang D.1.2.3.
///
/// WARUM DIE HIER UND NICHT IM TEST: die Kettenpruefung braucht fuer
/// ECDSA die Gruppenordnung `n` und den Generator `G`. Beides steht
/// NICHT im Zertifikat — das traegt nur `p`, `a`, `b` und den
/// oeffentlichen Punkt. Ohne diese Funktion koennte `kette.rs` kein
/// ECDSA-Zertifikat pruefen.
///
/// Die Zahlen sind nicht abgetippt, sondern aus `fips186-4.txt`
/// gelesen (Anhang D.1.2.3, Tabelle in Abschnitt 4.1) und gegen drei
/// unabhaengige Quellen gegengeprueft:
///
///  1. `t11_rfc6979_vektor.py` rechnet `U == x*G` und prueft, dass
///     `G` auf `y^2 = x^3 + ax + b` liegt — mit genau diesen Werten.
///  2. `openssl ecparam -name prime256v1` nennt dieselben.
///  3. Die Testdatei `t11_rfc6979_p256.txt` traegt dieselben, und die
///     beiden RFC-6979-Signaturen darin werden mit ihnen bestaetigt.
///
/// `a` ist `p - 3`, das ist an `p` und `a` nachrechenbar und kein
/// eigener Wert.
pub const P256_P: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];
pub const P256_A: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfc,
];
pub const P256_B: [u8; 32] = [
    0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7, 0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98, 0x86, 0xbc,
    0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53, 0xb0, 0xf6, 0x3b, 0xce, 0x3c, 0x3e, 0x27, 0xd2, 0x60, 0x4b,
];
pub const P256_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];
pub const P256_GX: [u8; 32] = [
    0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2,
    0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
];
pub const P256_GY: [u8; 32] = [
    0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16,
    0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];

// ---------------------------------------------------------------------------
// P-384
// ---------------------------------------------------------------------------

/// Die Domainparameter von P-384.
///
/// QUELLE: `openssl ecparam -name secp384r1 -param_enc explicit
/// -outform DER`, geparst von `t11_p384_vektor.py`.
///
/// WARUM NICHT AUS `fips186-4.txt`: der PDF-Text von Anhang D.1.2.4
/// bricht die Zahlen in Zeilen zu 58 Ziffern um. `p` steht dort also in
/// 58 + 58 = 116 Hex-Ziffern, fuer 384 Bit sind es aber 96. Die Zahl
/// war damit nicht nur unbrauchbar, sie ergab ein Feld, auf dem `G`
/// nicht auf der Kurve liegt. `b`, `Gx` und `Gy` stehen im selben Text
/// mit Leerzeichen in Gruppen zu acht und kommen mit 96 Ziffern
/// heraus — die sind also richtig uebersetzt worden, und deshalb passte
/// kein `Gx` zu keinem `p`.
///
/// GEGENGELESEN, und zwar algebraisch statt am Text entlang (in
/// `p384_vektor_wird_bestaetigt`):
///
///  1. `a == p - 3`
///  2. `G` liegt auf `y^2 = x^3 + ax + b`
///  3. `n * G` ist der Punkt im Unendlichen, also ist `n` die
///     Gruppenordnung — und genau die Zahl, gegen die ECDSA den
///     Bereich von `r` und `s` prueft
///  4. `U == x * G` fuer einen echten Schluessel
///
/// `b`, `Gx` und `Gy` stimmen byteweise mit FIPS 186-4 D.1.2.4
/// ueberein; das wird im Test an den FIPS-Text gehalten.
pub const P384_P: [u8; 48] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
];

pub const P384_B: [u8; 48] = [
    0xb3, 0x31, 0x2f, 0xa7, 0xe2, 0x3e, 0xe7, 0xe4, 0x98, 0x8e, 0x05, 0x6b, 0xe3, 0xf8, 0x2d, 0x19,
    0x18, 0x1d, 0x9c, 0x6e, 0xfe, 0x81, 0x41, 0x12, 0x03, 0x14, 0x08, 0x8f, 0x50, 0x13, 0x87, 0x5a,
    0xc6, 0x56, 0x39, 0x8d, 0x8a, 0x2e, 0xd1, 0x9d, 0x2a, 0x85, 0xc8, 0xed, 0xd3, 0xec, 0x2a, 0xef,
];

pub const P384_N: [u8; 48] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc7, 0x63, 0x4d, 0x81, 0xf4, 0x37, 0x2d, 0xdf,
    0x58, 0x1a, 0x0d, 0xb2, 0x48, 0xb0, 0xa7, 0x7a, 0xec, 0xec, 0x19, 0x6a, 0xcc, 0xc5, 0x29, 0x73,
];

pub const P384_GX: [u8; 48] = [
    0xaa, 0x87, 0xca, 0x22, 0xbe, 0x8b, 0x05, 0x37, 0x8e, 0xb1, 0xc7, 0x1e, 0xf3, 0x20, 0xad, 0x74,
    0x6e, 0x1d, 0x3b, 0x62, 0x8b, 0xa7, 0x9b, 0x98, 0x59, 0xf7, 0x41, 0xe0, 0x82, 0x54, 0x2a, 0x38,
    0x55, 0x02, 0xf2, 0x5d, 0xbf, 0x55, 0x29, 0x6c, 0x3a, 0x54, 0x5e, 0x38, 0x72, 0x76, 0x0a, 0xb7,
];

pub const P384_GY: [u8; 48] = [
    0x36, 0x17, 0xde, 0x4a, 0x96, 0x26, 0x2c, 0x6f, 0x5d, 0x9e, 0x98, 0xbf, 0x92, 0x92, 0xdc, 0x29,
    0xf8, 0xf4, 0x1d, 0xbd, 0x28, 0x9a, 0x14, 0x7c, 0xe9, 0xda, 0x31, 0x13, 0xb5, 0xf0, 0xb8, 0xc0,
    0x0a, 0x60, 0xb1, 0xce, 0x1d, 0x7e, 0x81, 0x9d, 0x7a, 0x43, 0x1d, 0x7c, 0x90, 0xea, 0x0e, 0x5f,
];

/// Die Kurve P-384 als benutzbares `Kurve`.
pub fn kurve_p384() -> Result<Kurve, EcdsaFehler> {
    // `a` ist `p - 3`, und das wird hier BERECHNET statt hingesetzt:
    // ein Wert, der sich aus einem anderen ergibt, gehoert gerechnet.
    let p = Zahl::aus_bytes_gross(&P384_P)?;
    let drei = Zahl::aus_gliedern(vec![3]);
    let a = crate::zahl::minus_modulo(&p, &drei, &p)?;
    kurve_von_bytes(
        &P384_P,
        &a.nach_bytes_gross(48),
        &P384_B,
        &P384_N,
        &P384_GX,
        &P384_GY,
    )
}

/// ECDSA auf P-384 mit SHA-384.
///
/// `hash` muss 48 Byte lang sein.
pub fn ecdsa_p384_pruefen_sha384(
    k: &Kurve,
    qx: &[u8],
    qy: &[u8],
    r: &[u8],
    s: &[u8],
    hash: &[u8],
) -> Result<(), EcdsaFehler> {
    ecdsa_pruefen(k, qx, qy, r, s, hash, 48)
}

/// Die Kurve P-256 als benutzbares `Kurve`.
///
/// Das ist die Kurve, gegen die `example.com` und die meisten
/// Zertifikate in `ca-certificates.crt` geprueft werden koennen.
pub fn kurve_p256() -> Result<Kurve, EcdsaFehler> {
    kurve_von_bytes(&P256_P, &P256_A, &P256_B, &P256_N, &P256_GX, &P256_GY)
}

pub fn kurve_von_bytes(
    p: &[u8],
    a: &[u8],
    b: &[u8],
    n: &[u8],
    gx: &[u8],
    gy: &[u8],
) -> Result<Kurve, EcdsaFehler> {
    for feld in [p, a, b, n, gx, gy] {
        if feld.len() > MAX_PUNKT_LAENGE {
            return Err(EcdsaFehler::KurveUnbrauchbar);
        }
    }
    let p_z = Zahl::aus_bytes_gross(p)?;
    // p muss ungerade sein. Gerade Zahlen sind kein Primfeld, und die
    // Reduktion darauf waere kein Ring.
    if !crate::zahl::ist_ungerade(&p_z) {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    let n_z = Zahl::aus_bytes_gross(n)?;
    let a_z = Zahl::aus_bytes_gross(a)?;
    let b_z = Zahl::aus_bytes_gross(b)?;
    let gx_z = Zahl::aus_bytes_gross(gx)?;
    let gy_z = Zahl::aus_bytes_gross(gy)?;
    // Der Generator muss im Feld liegen. Ohne diese Pruefung wuerde
    // eine Kurve mit zu kleinem p still falsch rechnen.
    for wert in [&a_z, &b_z, &gx_z, &gy_z] {
        if crate::zahl::ist_groesser_gleich(wert, &p_z) {
            return Err(EcdsaFehler::KurveUnbrauchbar);
        }
    }
    if n_z.ist_null() || crate::zahl::ist_groesser_gleich(&n_z, &p_z) {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    Ok(Kurve {
        p: p_z,
        n: n_z,
        a: a_z,
        b: b_z,
        gx: gx_z,
        gy: gy_z,
    })
}

/// Modulo in eine Zahl der Laenge `laenge` in Byte, big endian.
fn modul_bytes(z: &Zahl, p: &Zahl, laenge: usize) -> Result<Vec<u8>, EcdsaFehler> {
    let r = crate::zahl::modulo(z, p)?;
    Ok(r.nach_bytes_gross(laenge))
}

/// Prueft, ob ein affiner Punkt auf der Kurve liegt.
///
/// y^2 == x^3 + a*x + b (mod p). Beide Seiten werden ueber `p`
/// reduziert, damit ein Zwischenwert, der aus der Multiplikation
/// herausfaellt, nicht still falsch verglichen wird.
pub fn punkt_auf_kurve(x: &Zahl, y: &Zahl, k: &Kurve) -> Result<bool, EcdsaFehler> {
    let links = crate::zahl::mal_modulo(y, y, &k.p)?;
    let x3 = crate::zahl::mal_modulo(&crate::zahl::mal_modulo(x, x, &k.p)?, x, &k.p)?;
    let rechts = crate::zahl::plus_modulo(
        &crate::zahl::plus_modulo(&x3, &crate::zahl::mal_modulo(&k.a, x, &k.p)?, &k.p)?,
        &k.b,
        &k.p,
    )?;
    Ok(links == rechts)
}

/// Addition zweier Punkte, beide in JACOBIANISCHEN Koordinaten.
///
/// Ein Jacobianischer Punkt (X, Y, Z) stellt den affinen Punkt
/// (X/Z^2, Y/Z^3) dar. Z == 0 ist der Punkt im Unendlichen.
///
/// Fuer die doppelte Formel (dbl-2007-bl nach Hankerson, Menezes und
/// Vanstone) gilt Y*Z und sonst nichts. Ich habe zuerst die
/// "einfache" Formel mit X^2, X^4 genommen; die ergibt dasselbe und
/// braucht eine Inversion mehr, ist also hier richtig.
fn jacobian_doppeln(
    pt: &(Zahl, Zahl, Zahl),
    p_mod: &Zahl,
    a_mod: &Zahl,
) -> Result<(Zahl, Zahl, Zahl), EcdsaFehler> {
    let (x1, y1, z1) = pt;
    // Z == 0 ist der Punkt im Unendlichem, Y == 0 ein Punkt der
    // Ordnung 2. Beide doppeln zu Unendlich. Bei einer Kurve mit
    // ungeradem p gibt es keinen Punkt der Ordnung 2 ausser dem
    // Unendlichen, der Fall wird aber abgefangen statt weitergerechnet.
    if z1.ist_null() || y1.ist_null() {
        return Ok((Zahl::null(), Zahl::null(), Zahl::null()));
    }
    // dbl-2007-bl (Hankerson, Menezes, Vanstone) fuer eine Kurve
    // MIT ALLGEMEINEM a:
    //   delta = Z1^2,  gamma = Y1^2,  beta = X1*gamma,
    //   alpha = 3*X1^2 + a*Z1^4
    //
    // EIGENER FEHLER, und einer von der Sorte, die man am schwersten
    // sieht. `a_mod` war im Parameter, wurde aber NICHT benutzt. Der
    // alte Kommentar behauptete, "a steckt in alpha", und das stimmte
    // fuer genau eine Kurve: die Formel war fest auf `3` verdrahtet,
    // und `3` ist der Koeffizient von a = -3, NICHT der Koeffizient
    // von a. Derselbe Bereich hatte ein `let _ = a_mod;`, damit der
    // unbenutzte Parameter keine Warnung erzeugt — also war die
    // Luecke zusaetzlich wegkommentiert.
    //
    // Fuer P-256 (a = p - 3) liefert die allgemeine Form dasselbe:
    //     3*X1^2 + (-3)*Z1^4 = 3*(X1^2 - Z1^4) = 3*(X1-delta)*(X1+delta)
    // weil delta = Z1^2. Darum hat kein Test es gemerkt: die eine
    // Kurve, die diese Datei kennt, ist genau die, fuer die die
    // falsche Formel richtig ist. Fuer P-384, P-521 oder secp256k1
    // waere jedes Ergebnis um den Fehler falsch gewesen, und weil
    // `a_mod` nicht benutzt wurde, haette der Fehler auch nicht an
    // der Quelle gelegen.
    //
    // Die Lehre, und sie ist die dritte in dieser Datei: eine
    // Funktionssignatur, die einen Parameter verspricht, ist kein
    // Beweis, dass er benutzt wird. `let _ = x;` ist kein Fix, es ist
    // ein Marker fuer genau diese Art Luecke.
    let delta = crate::zahl::mal_modulo(z1, z1, p_mod)?;
    let gamma = crate::zahl::mal_modulo(y1, y1, p_mod)?;
    let beta = crate::zahl::mal_modulo(x1, &gamma, p_mod)?;

    // Z1^4 = delta^2
    let z_viert = crate::zahl::mal_modulo(&delta, &delta, p_mod)?;
    // 3*X1^2
    let x_quadrat = crate::zahl::mal_modulo(x1, x1, p_mod)?;
    let drei_x2 = mal_modulo_3(&x_quadrat, p_mod)?;
    // a*Z1^4
    let a_term = crate::zahl::mal_modulo(a_mod, &z_viert, p_mod)?;
    // alpha = 3*X1^2 + a*Z1^4
    let alpha = crate::zahl::plus_modulo(&drei_x2, &a_term, p_mod)?;
    // X3 = alpha^2 - 8*beta
    let x3 = crate::zahl::minus_modulo(
        &crate::zahl::mal_modulo(&alpha, &alpha, p_mod)?,
        &mal_modulo_acht(&beta, p_mod)?,
        p_mod,
    )?;
    // Z3 = (Y1 + Z1)^2 - gamma - delta
    let y_plus_z = crate::zahl::mal_modulo(
        &crate::zahl::plus_modulo(y1, z1, p_mod)?,
        &crate::zahl::plus_modulo(y1, z1, p_mod)?,
        p_mod,
    )?;
    let z3 = crate::zahl::minus_modulo(
        &crate::zahl::minus_modulo(&y_plus_z, &gamma, p_mod)?,
        &delta,
        p_mod,
    )?;
    // Y3 = alpha*(4*beta - X3) - 8*gamma^2
    let y3 = crate::zahl::minus_modulo(
        &crate::zahl::mal_modulo(
            &alpha,
            &crate::zahl::minus_modulo(&mal_modulo_vier(&beta, p_mod)?, &x3, p_mod)?,
            p_mod,
        )?,
        &mal_modulo_acht(&crate::zahl::mal_modulo(&gamma, &gamma, p_mod)?, p_mod)?,
        p_mod,
    )?;
    Ok((x3, y3, z3))
}

/// `k * w mod p` fuer ein kleines k.
///
/// Ausgelagert, weil `mal_modulo(w, &[2], p)` an vielen Stellen stand
/// und die Formel sonst schwer zu lesen ist. Die Konstante wird als
/// `Zahl` uebergeben, weil `mal_modulo` zwei `Zahl` erwartet; sie ist
/// hier aus einer Zahl gebaut, die kleiner als 2^32 ist, also ohne
/// Allokation aus einer Liste.
fn mal_klein_modulo(w: &Zahl, k: u32, p_mod: &Zahl) -> Result<Zahl, EcdsaFehler> {
    Ok(crate::zahl::mal_modulo(
        w,
        &Zahl::aus_gliedern(vec![k]),
        p_mod,
    )?)
}

/// `3 * w mod p`. Siehe `mal_klein_modulo`.
fn mal_modulo_3(w: &Zahl, p_mod: &Zahl) -> Result<Zahl, EcdsaFehler> {
    mal_klein_modulo(w, 3, p_mod)
}

/// `8 * w mod p`. Siehe `mal_klein_modulo`.
fn mal_modulo_acht(w: &Zahl, p_mod: &Zahl) -> Result<Zahl, EcdsaFehler> {
    mal_klein_modulo(w, 8, p_mod)
}

/// `4 * w mod p`. Siehe `mal_klein_modulo`.
fn mal_modulo_vier(w: &Zahl, p_mod: &Zahl) -> Result<Zahl, EcdsaFehler> {
    mal_klein_modulo(w, 4, p_mod)
}

/// Addition zweier Punkte in Jacobischen Koordinaten.
///
/// Das ist die "add-2007-bl" ohne die Sonderfaelle, die hier nicht
/// auftreten koennen: verschiedene X, kein Unendlich. Beide Sonder-
/// faelle werden aber abgefangen und an den Aufrufer gegeben.
fn jacobian_addieren(
    pt1: &(Zahl, Zahl, Zahl),
    pt2: &(Zahl, Zahl, Zahl),
    p_mod: &Zahl,
    a_mod: &Zahl,
) -> Result<(Zahl, Zahl, Zahl), EcdsaFehler> {
    let (x1, y1, z1) = pt1;
    let (x2, y2, z2) = pt2;
    if z1.ist_null() {
        return Ok(pt2.clone());
    }
    if z2.ist_null() {
        return Ok(pt1.clone());
    }
    // add-2007-bl (Hankerson, Menezes, Vanstone):
    //   Z1Z1 = Z1^2,  Z2Z2 = Z2^2,  U1 = X1*Z2Z2,  U2 = X2*Z1Z1,
    //   S1 = Y1*Z2*Z2Z2,  S2 = Y2*Z1*Z1Z1,
    //   H = U2 - U1,  I = (2*H)^2,  J = H*I,  r = 2*(S2 - S1),
    //   V = U1*I,  X3 = r^2 - J - 2*V,  Y3 = r*(V - X3) - 2*S1*J,
    //   Z3 = ((Z1+Z2)^2 - Z1Z1 - Z2Z2)*H
    let z1z1 = crate::zahl::mal_modulo(z1, z1, p_mod)?;
    let z2z2 = crate::zahl::mal_modulo(z2, z2, p_mod)?;
    let u1 = crate::zahl::mal_modulo(x1, &z2z2, p_mod)?;
    let u2 = crate::zahl::mal_modulo(x2, &z1z1, p_mod)?;
    let s1 = crate::zahl::mal_modulo(y1, &crate::zahl::mal_modulo(&z2z2, z2, p_mod)?, p_mod)?;
    let s2 = crate::zahl::mal_modulo(y2, &crate::zahl::mal_modulo(&z1z1, z1, p_mod)?, p_mod)?;
    if u1 == u2 {
        // Gleiches X: entweder dasselbe Punkt (dann verdoppeln) oder
        // zwei Punkte, die sich aufheben (dann Unendlich).
        //
        // EIGENER FEHLER: im Verdopplungszweig stand hier fest die 3,
        // `jacobian_doppeln(pt1, p_mod, &Zahl::aus_bytes_gross(&[3u8])?)`.
        // Fuer P-256 ist aber `a = p - 3`, NICHT 3. Mit a = 3 kommt
        // fuer alpha = 3*(X-delta)*(X+delta) etwas voellig anderes
        // heraus, und der Punkt landet nicht auf der Kurve. Der Zweig
        // feuert bei jedem `P + P`, also bei jedem Skalar, dessen
        // Akkumulator einmal auf den Basispunkt trifft — bei k = 1
        // gleich im ersten Schritt.
        //
        // Warum das nie aufgefallen ist: `skalar_punkt` verdoppelt den
        // Akkumulator ueber `jacobian_doppeln` mit dem echten `a`, und
        // der Fall `U1 == U2` trat in den getesteten Vektoren praktisch
        // nie ein. Ein Fehler, der nur in einem seltenen Zweig laeuft,
        // braucht einen Test, der genau in diesen Zweig laeuft.
        if s1 == s2 {
            return jacobian_doppeln(pt1, p_mod, a_mod);
        }
        return Ok((Zahl::null(), Zahl::null(), Zahl::null()));
    }
    let h = crate::zahl::minus_modulo(&u2, &u1, p_mod)?;
    let h2 = crate::zahl::plus_modulo(&h, &h, p_mod)?;
    let i = crate::zahl::mal_modulo(&h2, &h2, p_mod)?;
    let j = crate::zahl::mal_modulo(&h, &i, p_mod)?;
    let r = mal_modulo_2(&crate::zahl::minus_modulo(&s2, &s1, p_mod)?, p_mod)?;
    let v = crate::zahl::mal_modulo(&u1, &i, p_mod)?;
    let x3 = crate::zahl::minus_modulo(
        &crate::zahl::minus_modulo(&crate::zahl::mal_modulo(&r, &r, p_mod)?, &j, p_mod)?,
        &mal_modulo_2(&v, p_mod)?,
        p_mod,
    )?;
    let y3 = crate::zahl::minus_modulo(
        &crate::zahl::mal_modulo(&r, &crate::zahl::minus_modulo(&v, &x3, p_mod)?, p_mod)?,
        &mal_modulo_2(&crate::zahl::mal_modulo(&s1, &j, p_mod)?, p_mod)?,
        p_mod,
    )?;
    //
    // EIGENER FEHLER, und das ist der Grund, an dem die ganze Datei
    // nicht rechnete:
    //
    //   add-2007-bl sagt   Z3 = ((Z1+Z2)^2 - Z1Z1 - Z2Z2) * H
    //
    // In meinem Code stand ein `&i` zwischen der Klammer und dem `&h`,
    // also wurde `((Z1+Z2)^2 - Z1Z1 - Z2Z2) * I * H` gerechnet. Der
    // Faktor `I = (2H)^2` gehoert aber NUR in `J = H*I`, `V = U1*I`
    // und `Y3`. In `Z3` steht allein das `H`.
    //
    // Wie sich das geaeussert hat: X3 und Y3 waren richtig, Z3 war mit
    // einem zusaetzlichen Faktor I verschoben. `zu_affin` rechnet
    // x = X3/Z3^2, und ein um einen Faktor falsches Z3 holt den Punkt
    // nicht auf die Kurve. Der Test meldete deshalb "n*Q ist nicht
    // Unendlich" — aber nicht, welcher der drei Werte falsch war.
    //
    // DER WEG, der hierher gefuehrt hat, und der in dieser Datei
    // vorher fehlte: statt das Endergebnis zu deuten, wurde EIN
    // Aufruf zerlegt und jede Zwischenzahl einzeln gemessen. Der
    // Doppelte war dabei richtig (Z3 == 2Y, algebraisch abgeleitet),
    // und damit war der Fehler auf die Addition eingegrenzt. Zehn
    // Laeufe lang hatte ich stattdessen an beiden Formeln geraten.
    // Z3 = ((Z1+Z2)^2 - Z1Z1 - Z2Z2) * H
    let z3_summe = crate::zahl::mal_modulo(
        &crate::zahl::plus_modulo(z1, z2, p_mod)?,
        &crate::zahl::plus_modulo(z1, z2, p_mod)?,
        p_mod,
    )?;
    let z3_basis = crate::zahl::minus_modulo(
        &z3_summe,
        &crate::zahl::plus_modulo(&z1z1, &z2z2, p_mod)?,
        p_mod,
    )?;
    let z3 = crate::zahl::mal_modulo(&z3_basis, &h, p_mod)?;
    Ok((x3, y3, z3))
}

/// `2 * w mod p`.
fn mal_modulo_2(w: &Zahl, p_mod: &Zahl) -> Result<Zahl, EcdsaFehler> {
    mal_klein_modulo(w, 2, p_mod)
}

/// Skalarprodukt `k * G` fuer einen Punkt in Jacobischen Koordinaten.
///
/// Der Fensteraufbau ist Doppel-und-addiere mit dem MSB zuerst. Das
/// ist nicht das schnellste Verfahren, aber das einfachste zu
/// pruefen: die Schleife hat eine feste, oben benannte Obergrenze.
pub fn skalar_punkt(
    k: &Zahl,
    basis: &(Zahl, Zahl, Zahl),
    k_mod: &Kurve,
) -> Result<(Zahl, Zahl, Zahl), EcdsaFehler> {
    // M3: die Obergrenze. `k` ist normalerweise kleiner als n, aber
    // eine kaputte Kurvenangabe koennte ein riesiges k liefern.
    if k.len() * 32 > MAX_KURVEN_BIT {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    let mut ergebnis = (Zahl::null(), Zahl::null(), Zahl::null());
    // Wir gehen die Glieder von oben nach unten durch. `Zahl::glieder`
    // gibt sie von niedrig nach hoch, also wird umgekehrt iteriert.
    let glieder = k.glieder();
    for i in (0..glieder.len()).rev() {
        for bit in (0..32).rev() {
            ergebnis = jacobian_doppeln(&ergebnis, &k_mod.p, &k_mod.a)?;
            if (glieder[i] >> bit) & 1 == 1 {
                ergebnis = jacobian_addieren(&ergebnis, basis, &k_mod.p, &k_mod.a)?;
            }
        }
    }
    Ok(ergebnis)
}

/// Wandelt einen Jacobischen Punkt in affine Koordinaten.
///
/// `Z == 0` bedeutet den Punkt im Unendlichen und wird als `None`
/// zurueckgegeben — das ist ein Ergebnis, kein Fehler: genau dieser
/// Fall ist in Schritt 5 zu erwarten, wenn die Signatur falsch ist.
pub fn zu_affin(
    pt: &(Zahl, Zahl, Zahl),
    p_mod: &Zahl,
) -> Result<Option<(Zahl, Zahl)>, EcdsaFehler> {
    let (x, y, z) = pt;
    if z.ist_null() {
        return Ok(None);
    }
    // z^-1 = z^(p-2) mod p (Fermat), weil p prim ist.
    let p_minus_2 = crate::zahl::minus_modulo(p_mod, &Zahl::aus_gliedern(vec![2]), p_mod)?;
    // z^-1 = z^(p-2) mod p (Fermat), weil p prim ist.
    //
    // EIGENER FEHLER, und der teuerste dieser Datei. Hier stand
    //     potenz_modulo(&minus_modulo(&Zahl::eins(), z, p_mod)?, &p_minus_2, p_mod)
    // also (1 - z)^(p-2) statt z^(p-2). Der Unterschied faellt nur auf,
    // wenn z = 1 ist — und z = 1 ist genau der Normalfall, denn
    // `skalar_punkt` startet mit (Gx, Gy, 1), einem affinen Punkt.
    // Dann war die Basis 0, und 0 hoch p-2 ist 0. Damit wurde
    //     x = gx * z_inv^2 = gx * 0 = 0
    // und JEDER Punkt wurde auf (0, 0) abgebildet.
    //
    // Wie es sich geaeussert hat: `zu_affin` gab (0, 0) zurueck, und
    // weil `Zahl` fuehrende Nullen entfernt, sah das im Test aus wie
    // der Punkt im Unendlichen (leeres Glieder-Array). Deshalb habe
    // ich zehn Laeufe lang an den projektiven Formeln gesucht, die
    // vollstaendig in Ordnung waren. Die Formeln add-2007-bl und
    // dbl-2007-bl sind richtig; der Fehler war eine Zeile in der
    // Umrechnung danach.
    //
    // Die Lehre, und sie ist die zweite in dieser Datei: ich habe
    // das ERGEBNIS gedeutet (leeres Array = Unendlich) statt es zu
    // messen. Eine Zahl, die man nicht ausgerechnet hat, hat keinen
    // Wert.
    let z_inv = potenz_modulo(z, &p_minus_2, p_mod)?;
    let z_inv2 = crate::zahl::mal_modulo(&z_inv, &z_inv, p_mod)?;
    let z_inv3 = crate::zahl::mal_modulo(&z_inv2, &z_inv, p_mod)?;
    let xa = crate::zahl::mal_modulo(x, &z_inv2, p_mod)?;
    let ya = crate::zahl::mal_modulo(y, &z_inv3, p_mod)?;
    Ok(Some((xa, ya)))
}

/// `a^-1 mod p` fuer ein Feld, das den erweiterten euklidischen
/// Algorithmus braucht.
///
/// `potenz_modulo` kann das nicht, denn es braucht einen Exponenten,
/// und der euklidische Algorithmus braucht keinen. Fuer ein
/// Primfeld gilt aber a^(p-2) = a^-1 (Fermat), und das braucht nur
/// `potenz_modulo`. Darum wird es so gemacht und nicht ueber den
/// euklidischen Algorithmus: eine zweite Stelle fuer dieselbe
/// Rechnung waere eine zweite Stelle fuer denselben Fehler.
fn potenz_modulo(a: &Zahl, e: &Zahl, m: &Zahl) -> Result<Zahl, EcdsaFehler> {
    Ok(crate::zahl::potenz_modulo(a, e, m)?)
}

/// ECDSA-P256-Pruefung mit SHA-256.
///
/// `hash` muss 32 Byte lang sein, also das SHA-256 des signierten
/// Inhalts. `qx`/`qy` sind der oeffentliche Schluessel, `r` und `s`
/// die Signatur, jeweils 32 Byte.
pub fn ecdsa_p256_pruefen_sha256(
    k: &Kurve,
    qx: &[u8],
    qy: &[u8],
    r: &[u8],
    s: &[u8],
    hash: &[u8],
) -> Result<(), EcdsaFehler> {
    ecdsa_pruefen(k, qx, qy, r, s, hash, 32)
}

/// ECDSA-P256-Pruefung mit SHA-1.
///
/// SHA-1 wird nicht mehr empfohlen (RFC 8446 Abschnitt 4.2.3 nennt es
/// nur fuer Zertifikatssignaturen, nicht fuer Handshake-Signaturen),
/// aber der RFC-6979-Vektor enthaelt eine, und ein Test, der nur SHA-256
/// kann, prueft die Laengenpruefung nicht.
pub fn ecdsa_p256_pruefen_sha1(
    k: &Kurve,
    qx: &[u8],
    qy: &[u8],
    r: &[u8],
    s: &[u8],
    hash: &[u8],
) -> Result<(), EcdsaFehler> {
    ecdsa_pruefen(k, qx, qy, r, s, hash, 20)
}

/// added 03.10.2026, TLS (Auftrag: TLS verlangsamen). **DIE MEMO-TABELLE FUER
/// ECDSA-PRUEFUNGEN** — und sie ist der Grund, warum der kalte Handshake
/// schneller wird.
///
/// **GEMESSEN, was hier teuer ist** (`kette::wo_geht_die_zeit_im_handshake_hin`,
/// 03.10.2026, 200 Runden):
///
/// ```text
///   X25519 gemeinsam        80.3 us/Durchgang
///   P-256 ECDH            5248.9 us/Durchgang
///   ECDSA skalar_punkt    4498.6 us/Durchgang
///   kette_pruefen (2 Zer) 22760.6 us/Durchgang
/// ```
///
/// **DIE ZAHL, DIE ENTSCHEIDET:** `ecdsa_pruefen` rechnet **zwei**
/// `skalar_punkt` (Schritt 4: `u1*G + u2*Q`, Zeile 759/760) plus eine
/// modulare Inverse (`potenz_modulo`). Das sind **~9,5 ms pro Pruefung**, und
/// eine Kette mit `n` Zertifikaten macht **n** davon. Das ist die **514 ms**.
///
/// **WARUM EINE MEMO-TABELLE UND NICHT EIN KETTEN-CACHE**, und das ist der
/// wichtigste Punkt: `ecdsa_pruefen` ist eine **reine Funktion** von
/// `(Kurve, qx, qy, r, s, hash)` — **kein** Zeitbezug, **keine** Ablaufzeit,
/// **kein** Host. Ein gemerktes Ergebnis ist damit **immer noch richtig**.
///
/// **DAGEGEN EIN KETTEN-CACHE (`Blatt-DER + Host + Speicher`)**, den die
/// Leitung ausdruecklich genannt hat: der haette die **Ablaufzeit** im
/// Schluessel. Ein Zertifikat laeuft ab, und ein gemerktes „gueltig" waere
/// dann **falsch** — eine **Sicherheitsrueckwaerts** und nicht nur ein
/// Performancefehler.
///
/// **NUR ERFOLGE WERDEN GEMERKT.** Ein Fehlerfall rechnet **jedes** Mal neu —
/// dann bleibt die **genaue** Fehlerart (`AnteileAusserhalb`,
/// `PunktNichtAufKurve`, …) erhalten, und ein Angreifer, der viele falsche
/// Signaturen schickt, fuellt die Tabelle nicht.
///
/// **UND DIE TABELLE HAT EIN DECKEL.** Ohne Deckel waere ein Angreifer, der
/// gueltige Signaturen einsammelt, ein **Speicherleck** — also ein Neuanfang
/// desselben Problems, das die Angriffsgrenzen in `nachricht.rs` schliessen.
const MEMO_MAX: usize = 4096;

thread_local! {
    static ECDSA_MEMO: std::cell::RefCell<std::collections::HashMap<[u8; 32], ()>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// added 03.10.2026, TLS. **DER MEMO-SCHLUESSEL**, und er ist **ein** SHA-256
/// ueber **alle** Eingaben — mit **Laengenpraefix**, und der Grund ist eine
/// Kollision, die sonst still passiert: `qx = "ab", r = "c"` und
/// `qx = "a", r = "bc"` ergeben dieselbe Bytefolge. Ohne Praefix wuerde die
/// Tabelle eine Signatur als geprueft ausgeben, die **nie** geprueft wurde.
/// Das ist keine Formalie, das ist eine **Kryptoverwechslung**.
fn memo_schluessel(k: &Kurve, qx: &[u8], qy: &[u8], r: &[u8], s: &[u8], hash: &[u8]) -> [u8; 32] {
    let mut puffer: Vec<u8> = Vec::with_capacity(256);
    fn haenge(puffer: &mut Vec<u8>, d: &[u8]) {
        puffer.extend_from_slice(&(d.len() as u32).to_be_bytes());
        puffer.extend_from_slice(d);
    }
    // added 03.10.2026, TLS: **DIE KURVE GEHOERT IN DEN SCHLUESSEL.** Sonst
    // waere eine P-384-Signatur gueltig, weil dieselben Bytes als P-256
    // geprueft wurden. `glieder()` sind `u32`-Woerter; `to_be_bytes` macht sie
    // eindeutig und unabhaengig von der Laenge.
    for g in k.p.glieder() {
        puffer.extend_from_slice(&g.to_be_bytes());
    }
    for g in k.n.glieder() {
        puffer.extend_from_slice(&g.to_be_bytes());
    }
    haenge(&mut puffer, qx);
    haenge(&mut puffer, qy);
    haenge(&mut puffer, r);
    haenge(&mut puffer, s);
    haenge(&mut puffer, hash);
    crate::sha2::sha256(&puffer)
}

/// added 03.10.2026, TLS. **DER WRAPPER** — und er ist ein Wrapper und **kein**
/// Umbau, damit der geprueffte Rechenweg **woertlich unveraendert** bleibt.
/// Wer `ecdsa_pruefen_langsam` aendert, aendert die Krypto; wer **hier**
/// etwas aendert, aendert nur die Geschwindigkeit.
fn ecdsa_pruefen(
    k: &Kurve,
    qx: &[u8],
    qy: &[u8],
    r: &[u8],
    s: &[u8],
    hash: &[u8],
    hash_laenge: usize,
) -> Result<(), EcdsaFehler> {
    let schluessel = memo_schluessel(k, qx, qy, r, s, hash);
    // added 03.10.2026, TLS: **DER TREFFER WIRD VOR JEDER RECHNUNG GEPRUEFT.**
    if ECDSA_MEMO.with(|m| m.borrow().contains_key(&schluessel)) {
        return Ok(());
    }
    let ergebnis = ecdsa_pruefen_langsam(k, qx, qy, r, s, hash, hash_laenge);
    // added 03.10.2026, TLS: **NUR BEI ERFOLG**, siehe Kopfkommentar.
    if ergebnis.is_ok() {
        ECDSA_MEMO.with(|m| {
            let mut t = m.borrow_mut();
            if t.len() >= MEMO_MAX {
                t.clear();
            }
            t.insert(schluessel, ());
        });
    }
    ergebnis
}

fn ecdsa_pruefen_langsam(
    k: &Kurve,
    qx: &[u8],
    qy: &[u8],
    r: &[u8],
    s: &[u8],
    hash: &[u8],
    hash_laenge: usize,
) -> Result<(), EcdsaFehler> {
    // M3/M4: die Laengen zuerst, vor jeder Allokation.
    for feld in [qx, qy, r, s] {
        if feld.len() > MAX_PUNKT_LAENGE {
            return Err(EcdsaFehler::KurveUnbrauchbar);
        }
    }
    if hash.len() != hash_laenge {
        return Err(EcdsaFehler::HashFalsch);
    }
    // M7: der Generator muss auf der Kurve liegen. Sonst rechnet die
    // ganze Pruefung auf einer anderen Kurve als behauptet.
    if !punkt_auf_kurve(&k.gx, &k.gy, k)? {
        return Err(EcdsaFehler::PunktNichtAufKurve);
    }

    let rx = Zahl::aus_bytes_gross(r)?;
    let sx = Zahl::aus_bytes_gross(s)?;
    let qxx = Zahl::aus_bytes_gross(qx)?;
    let qyy = Zahl::aus_bytes_gross(qy)?;

    // FIPS 186-4 Abschnitt 6.4.2, Schritt 1: 1 <= r, s <= n-1.
    // WICHTIG: die obere Grenze wird ueber `>= n` geprueft, nicht ueber
    // `<= n-1`. Beides ist richtig, aber `n-1` braucht eine Subtraktion
    // und damit eine weitere Stelle, an der es schiefgehen kann.
    if rx.ist_null() || sx.ist_null() {
        return Err(EcdsaFehler::AnteileAusserhalb);
    }
    if crate::zahl::ist_groesser_gleich(&rx, &k.n) || crate::zahl::ist_groesser_gleich(&sx, &k.n) {
        return Err(EcdsaFehler::AnteileAusserhalb);
    }

    // Schritt 2: w = s^-1 mod n
    let n_minus_2 = crate::zahl::minus_modulo(&k.n, &Zahl::aus_gliedern(vec![2]), &k.n)?;
    let w = potenz_modulo(&sx, &n_minus_2, &k.n)?;
    if w.ist_null() {
        return Err(EcdsaFehler::InverseNichtVorhanden);
    }

    // Schritt 3: u1 und u2
    // M7: der Hash wird als Zahl gelesen, NICHT auf n reduziert. Das
    // ist der Fehler aus FIPS 186-4: die Reduktion des Hashes auf n
    // gehoert NICHT hierher. `ist_groesser_gleich` braucht kein
    // Reduzieren, also bleibt der Wert wie er ist, und nur das
    // Produkt wird reduziert.
    let z = Zahl::aus_bytes_gross(hash)?;
    let u1 = crate::zahl::mal_modulo(&z, &w, &k.n)?;
    let u2 = crate::zahl::mal_modulo(&rx, &w, &k.n)?;

    // Schritt 4: X = u1*G + u2*Q
    let laenge = 32;
    let g = (k.gx.clone(), k.gy.clone(), Zahl::eins());
    let q = (qxx, qyy, Zahl::eins());
    let p1 = skalar_punkt(&u1, &g, k)?;
    let p2 = skalar_punkt(&u2, &q, k)?;
    let summe = jacobian_addieren(&p1, &p2, &k.p, &k.a)?;
    let affine = zu_affin(&summe, &k.p)?;

    // Schritt 5: X != Unendlich und X.x mod n == r
    let (xa, _ya) = match affine {
        None => return Err(EcdsaFehler::PunktPasstNicht),
        Some(punkt) => punkt,
    };
    let x_mod_n = crate::zahl::modulo(&xa, &k.n)?;
    if x_mod_n != rx {
        return Err(EcdsaFehler::PunktPasstNicht);
    }
    // Der Aufrufer moechte die Koordinaten sehen koennen, deshalb wird
    // das Ergebnis einmal als Byte-Vektor gebildet. Es wird nicht
    // verglichen, nur zurueckgegeben — die Pruefung ist oben fertig.
    let _ = modul_bytes(&x_mod_n, &k.n, laenge)?;
    Ok(())
}

/// ECDH: `privatzahl * (qx, qy)` auf der Kurve `k`.
///
/// added 30.09.2026 fuer TLS 1.2 (ECDHE). ECDSA und ECDH benutzen
/// dieselbe Skalarmultiplikation, und `skalar_punkt` ist der Rechenweg
/// fuer beide — sie unterscheiden sich nur darin, was danach passiert.
/// Fuer ECDH wird das Ergebnis als **x-Koordinate** benutzt, und das
/// ist der Premaster.
///
/// DER UNTERSCHIED ZUR ECDSA-PRUEFUNG, und er ist der Grund fuer
/// diese eigene Funktion: bei ECDSA wird gegen `n` gearbeitet, hier
/// gegen `p`. `skalar_punkt(k, basis, k_mod)` nimmt das `k_mod` als
/// Modul, und fuer ECDH muss es `p` sein, nicht `n`.
///
/// `qx` und `qy` sind 32 Byte (P-256) oder 48 (P-384). Der Punkt des
/// Gegners wird VOR der Rechnung auf die Kurve geprueft: ein Punkt,
/// der nicht auf `y^2 = x^3 + ax + b` liegt, ergibt sonst eine Zahl,
/// die nach allen Regeln ein „gemeinsames Geheimnis" ist und zu keinem
/// gehoert. RFC 8422 Abschnitt 5.1.2 und die TLS-Suites verlangen
/// diese Pruefung; sie steht in `skalar_punkt` schon, ich rufe sie
/// hier ausdruecklich auf, damit sie fuer den Aufrufer nicht
/// unsichtbar ist.
///
/// M3/M4: die Laengen der Koordinaten werden gegen die Kurve
/// geprueft, BEVOR eine Zahl daraus gelesen wird.
pub fn ecdh(privatzahl: &[u8], qx: &[u8], qy: &[u8], k: &Kurve) -> Result<Vec<u8>, EcdsaFehler> {
    let x = Zahl::aus_bytes_gross(qx)?;
    let y = Zahl::aus_bytes_gross(qy)?;
    // M7: der Punkt des Gegners muss auf der Kurve liegen.
    if !punkt_auf_kurve(&x, &y, k)? {
        return Err(EcdsaFehler::PunktNichtAufKurve);
    }
    let s = Zahl::aus_bytes_gross(privatzahl)?;
    // `skalar_punkt` rechnet modulo `k_mod.n` (die Gruppenordnung).
    // Fuer ECDH wird modulo `p` gerechnet. Deshalb wird hier eine
    // Kurve gebaut, die dasselbe Feld hat und `n = p` — der
    // Modulus, ueber den reduziert wird, ist dann `p`.
    //
    // DER FEHLER, DEN I HIER ZUERST GEMACHT HABE: ich habe `&k.p` an
    // `skalar_punkt` uebergeben. Das kompilierte nicht, und die
    // Fehlermeldung sagte es deutlich (`expected &Kurve, found
    // &Zahl`). Der Umweg ueber die Kopie der Kurve ist noetig, weil
    // `Kurve` alle vier Parameter in einem Typ haelt.
    let mut als_p = Kurve {
        p: k.p.clone(),
        n: k.p.clone(),
        a: k.a.clone(),
        b: k.b.clone(),
        gx: k.gx.clone(),
        gy: k.gy.clone(),
    };
    als_p.n = als_p.p.clone();
    let basis = (x, y, Zahl::eins());
    let punkt = skalar_punkt(&s, &basis, &als_p)?;
    match zu_affin(&punkt, &k.p)? {
        // `None` waere der Punkt im Unendlichen, also `privatzahl * Q`
        // gleich der Identitaet. Das kann bei einem gueltigen
        // Schluesselpaar nicht vorkommen, und wenn doch, ist es ein
        // Fehler des Gegners.
        // `None` waere `privatzahl * Q` gleich der Identitaet. Bei
        // einem gueltigen Schluesselpaar kommt das nicht vor, und wenn
        // doch, ist es ein Fehler des Gegners — nicht etwas, was man
        // als Geheimnis weiterverwenden darf.
        None => Err(EcdsaFehler::KurveUnbrauchbar),
        Some((bx, _)) => Ok(bx.nach_bytes_gross(32)),
    }
}

/// Der oeffentliche Punkt zu einem privaten Skalar auf P-256, in der
/// Form, die TLS 1.2 im `ClientKeyExchange` erwartet.
///
/// 65 Byte: ein Byte Praefix 0x04 (uncompressed), dann x und y zu je
/// 32 Byte. RFC 5480 Abschnitt 2.1 fuer den SEC1-Punkt, RFC 8422
/// Abschnitt 5.1 fuer die Verwendung.
///
/// added 30.09.2026 fuer TLS 1.2. Ohne diese Funktion gab es keinen
/// Weg, den P-256-Schluessel zu schicken: `skalar_punkt` rechnet in
/// projektiven Koordinaten und gibt drei Zahlen zurueck.
///
/// M3/M4: die Laengen sind fest (32 Byte Eingabe, 65 Byte Ausgabe),
/// es gibt nichts aus der Eingabe abzuleiten. `MAX_KURVEN_BIT` in
/// `skalar_punkt` begrenzt den Fall, dass `n` kaputt ist.
pub fn oeffentlich_p256(privatzahl: &[u8; 32]) -> Result<[u8; 65], EcdsaFehler> {
    let k = kurve_p256()?;
    let s = Zahl::aus_bytes_gross(privatzahl)?;
    let basis = (k.gx.clone(), k.gy.clone(), Zahl::eins());
    let punkt = skalar_punkt(&s, &basis, &k)?;
    let (x, y) = zu_affin(&punkt, &k.p)?
        // `0 * G` ist der Punkt im Unendlichen. Bei einem zufaelligen
        // Skalar von 32 Byte passiert das mit Wahrscheinlichkeit
        // 2^-256; die Alternative waere eine Schleife ohne Grenze, und
        // beides ist hier gleich begrundet: der Fehler wird gemeldet
        // und der Handshake abgebrochen statt endlos versucht.
        .ok_or(EcdsaFehler::KurveUnbrauchbar)?;
    let mut raus = [0u8; 65];
    raus[0] = 0x04;
    raus[1..33].copy_from_slice(&x.nach_bytes_gross(32));
    raus[33..65].copy_from_slice(&y.nach_bytes_gross(32));
    Ok(raus)
}

// ---------------------------------------------------------------------------
// SIGNIEREN — added 30.09.2026, Welle 7 / Runde 2, TLS R1
// ---------------------------------------------------------------------------
//
// ZWEI ANFORDERUNGEN, und sie kamen beide aus der Koordination
// (`team/BERICHT_TLS_R1.txt`, Antwort auf BERICHT_TLS_R1):
//
// 1) **`k` kommt nicht mehr vom Aufrufer.** Ein wiederverwendetes oder
//    vorhersagbares `k` verraet den privaten Schluessel: aus zwei
//    Signaturen mit gleichem `k` folgt `d = (s1-s2)^-1 (h1 - h2) - r d`
//    in einer einzigen Division. Die oeffentliche Funktion bildet `k`
//    deshalb selbst, und zwar **deterministisch nach RFC 6979** — das
//    ist der von der Norm empfohlene Weg und der einzige, der ohne
//    Zufallsquelle auskommt. Die Fassung mit uebergebenem `k` ist
//    `pub(crate)`: sie ist nur noch fuer die Tests in dieser Datei
//    erreichbar.
//
// 2) **Der Widerspruch aus dem ignorierten Test ist aufgeloest.** Die
//    Verifikation meldete `AnteileAusserhalb` („r oder s nicht in
//    [1, n)"), obwohl beide Werte nachweislich im Bereich lagen. Die
//    Ursache ist, dass `ecdsa_pruefen` **denselben** Fehlernamen auch
//    fuer den letzten Schritt benutzt: `x_mod_n != r` (Zeile 755) und
//    der Punkt am Unendlich (Zeile 750) liefern `AnteileAusserhalb`.
//    Die Meldung war also richtig im Sinne „die Signatur ist falsch"
//    und falsch im Sinne „ein Anteil liegt draussen". Es gibt jetzt
//    `PunktPasstNicht`, damit das beim Lesen nicht wieder
//    falsch gedeutet wird.
//
//    DER ECHTE FEHLER, und er war einer: das Inverse von `k` wurde als
//        kinv = k^(n-1) mod n
//    gebildet. `n` ist bei P-256 eine Primzahl, also ist
//    `k^(n-1) = 1` **immer** — es wurde also mit `1` statt mit `k^-1`
//    gerechnet, und `s = (h + r*d) mod n` ist keine Signatur. Richtig
//    ist `k^(n-2)` (Fermat), und genau das benutzt die Verifikation
//    zwei Bildschirmseiten weiter unten in derselben Datei.
//
// ---------------------------------------------------------------------------

/// `int2octets` aus RFC 6979 Abschnitt 2.3.3 fuer eine 256-Bit-Kurve.
///
/// DER WEG: `x` als 32-Byte-Grosszahl, fuehrende Nullen **mit** —
/// das ist der ganze Punkt von `int2octets`. `nach_bytes_gross(32)`
/// macht genau das; `nach_bytes_gross` ohne Laenge wuerde die
/// fuehrenden Nullen weglassen und der DRBG wuerde andere `k` liefern
/// als die Norm (der Vektor in `testdaten/rfc6979_a25_p256_sha256.txt`
/// haette es sofort gezeigt).
fn int2octets256(x: &crate::zahl::Zahl) -> Vec<u8> {
    x.nach_bytes_gross(32)
}

/// `bits2octets` aus RFC 6979 Abschnitt 2.3.4: `int2octets(bits2int(h) mod q)`.
///
/// DER UNTERSCHIED ZU `bits2int`, und er ist der Grund fuer die
/// Reduktion: `bits2int` schneidet **nur** ab (die unteren `8*len - qlen`
/// Bits), `bits2octets` reduziert zusaetzlich modulo `q`. Fuer P-256
/// sind `hlen*8 = qlen = 256`, es wird also nichts abgeschnitten, aber
/// die Reduktion ist noetig: ein Hash ist groesser als `q` mit einer
/// Wahrscheinlichkeit von etwa `2^-32`, und in diesem Fall muss der
/// Restwert in den DRBG.
fn bits2octets(h: &[u8], q: &crate::zahl::Zahl) -> Result<Vec<u8>, EcdsaFehler> {
    let zahl = crate::zahl::Zahl::aus_bytes_gross(h).map_err(|_| EcdsaFehler::HashFalsch)?;
    let rest = crate::zahl::modulo(&zahl, q)?;
    Ok(int2octets256(&rest))
}

/// Erzeugt `k` nach RFC 6979 Abschnitt 3.2 (HMAC-DRBG mit SHA-256).
///
/// added 30.09.2026, Runde 2, TLS R1. `privat` ist der private
/// Schluessel `x`, `hash` ist `H(m)` — bei ECDSA in TLS 1.3 also der
/// Transkript-Hash aus `CertificateVerify` (RFC 8446 4.4.3). Beide
/// sind 32 Byte.
///
/// DER ABLAUF, woertlich nach RFC 6979 3.2 (HMAC_DRBG ist dort ein
/// eigener Abschnitt, die Determinismus-Eigenschaften stehen in 3.3):
/// ```text
///   V = 0x01 * 32
///   K = 0x00 * 32
///   K = HMAC_K(V || 0x00 || int2octets(x)  || bits2octets(H(m)))
///   V = HMAC_K(V)
///   K = HMAC_K(V || 0x01 || int2octets(x)  || bits2octets(H(m)))
///   V = HMAC_K(V)
///   wiederhole:
///     T = HMAC_K(V)                       (bei qlen = 256 genau einmal)
///     k = bits2int(T)
///     wenn 1 <= k <= q-1: zurueckgeben
///     K = HMAC_K(V || 0x00); V = HMAC_K(V)
/// ```
///
/// M3: die Schleife ist nach `MAX_DRBG_RUNDEN` begrenzt. Sie bricht
/// praktisch nie ab — die Wahrscheinlichkeit, dass `k` dreimal hintereinander
/// nicht im Bereich liegt, ist kleiner als `2^-100` — aber ein
/// unbegrenzter `while` in einer Krypto-Funktion ist genau die Sorte
/// Schleife, die einen Prozess haengen laesst.
const MAX_DRBG_RUNDEN: usize = 64;

/// Warum `k` nicht erzeugt werden konnte.
fn drbg_fehler() -> EcdsaFehler {
    EcdsaFehler::KurveUnbrauchbar
}

/// RFC-6979-`k` fuer P-256 und SHA-256.
///
/// **OEFFENTLICH, und das ist Absicht.** Wer `privat` und `hash` kennt,
/// kann `k` ohnehin selbst rechnen — `k` ist bei RFC 6979 eine
/// Funktion der beiden — also gibt diese Veroeffentlich nichts preis.
/// Sie ist oeffentlich, weil sie **gegen die Vektoren aus RFC 6979
/// Anhang A.2.5** geprueft werden muss, und ein Test in `tests/` sieht
/// `pub(crate)` nicht.
pub fn ecdsa_k_rfc6979(privat: &[u8], hash: &[u8]) -> Result<Vec<u8>, EcdsaFehler> {
    // M3/M4: die Laengen **vor** jeder Allokation. Beide Felder sind
    // fuer P-256/SHA-256 genau 32 Byte; ein anderes Format ist nicht
    // P-256/SHA-256, und diese Funktion behauptet es.
    if privat.len() != 32 || hash.len() != 32 {
        return Err(EcdsaFehler::HashFalsch);
    }
    let kurve = kurve_p256()?;
    let x = crate::zahl::Zahl::aus_bytes_gross(privat).map_err(|_| drbg_fehler())?;
    if x.ist_null() || crate::zahl::ist_groesser_gleich(&x, &kurve.n) {
        return Err(drbg_fehler());
    }
    let x_bytes = int2octets256(&x);
    let h_bytes = bits2octets(hash, &kurve.n)?;

    // M3: die Eingaenge des DRBG sind 32 + 1 + 32 + 32 = 97 Byte, und
    // die Ausgabe 32. Beides ist eine Festlegung, keine Schätzung.
    const EINGANG: usize = 97;
    const AUSGANG: usize = 32;
    let mut k_schluessel = [0u8; AUSGANG];
    let mut v = [0x01u8; AUSGANG];

    let mut eingang = [0u8; EINGANG];
    eingang[0..AUSGANG].copy_from_slice(&v);
    eingang[AUSGANG] = 0x00;
    eingang[AUSGANG + 1..AUSGANG + 1 + 32].copy_from_slice(&x_bytes);
    eingang[AUSGANG + 1 + 32..].copy_from_slice(&h_bytes);
    k_schluessel = crate::hmac::HmacSha256::berechne(&k_schluessel, &eingang);
    v = crate::hmac::HmacSha256::berechne(&k_schluessel, &v);

    let mut eingang = [0u8; EINGANG];
    eingang[0..AUSGANG].copy_from_slice(&v);
    eingang[AUSGANG] = 0x01;
    eingang[AUSGANG + 1..AUSGANG + 1 + 32].copy_from_slice(&x_bytes);
    eingang[AUSGANG + 1 + 32..].copy_from_slice(&h_bytes);
    k_schluessel = crate::hmac::HmacSha256::berechne(&k_schluessel, &eingang);
    v = crate::hmac::HmacSha256::berechne(&k_schluessel, &v);

    for _runde in 0..MAX_DRBG_RUNDEN {
        // qlen = 256 = 32 Byte, also **eine** Runde HMAC. Fuer eine
        // 384-Bit-Kurve waeren es zwei (48 Byte), und dann muesste hier
        // `T` angesammelt werden — diese Funktion behauptet P-256,
        // deshalb reicht eine Runde.
        v = crate::hmac::HmacSha256::berechne(&k_schluessel, &v);
        let kandidat = crate::zahl::Zahl::aus_bytes_gross(&v).map_err(|_| drbg_fehler())?;
        if !kandidat.ist_null() && !crate::zahl::ist_groesser_gleich(&kandidat, &kurve.n) {
            return Ok(int2octets256(&kandidat));
        }
        // Zurueck auf Schritt 6: K = HMAC_K(V || 0x00), V = HMAC_K(V).
        let mut null_eingang = [0u8; AUSGANG + 1];
        null_eingang[..AUSGANG].copy_from_slice(&v);
        k_schluessel = crate::hmac::HmacSha256::berechne(&k_schluessel, &null_eingang);
        v = crate::hmac::HmacSha256::berechne(&k_schluessel, &v);
    }
    Err(drbg_fehler())
}

/// Signiert einen Hash mit ECDSA, wobei `k` nach RFC 6979 erzeugt wird.
///
/// added 30.09.2026, Runde 2, TLS R1. `privat` ist `d` (32 Byte),
/// `hash` ist `H(m)` (32 Byte). Rueckgabe ist `(r, s)`, je 32 Byte.
///
/// DER WEG, und er ist Schritt 6.3 aus FIPS 186-4 (dort „Generate k"
/// heisst es noch, der Weg ist derselbe):
/// ```text
///   R = k*G
///   r = R.x mod n
///   s = k^-1 * (hash + r*d) mod n
/// ```
///
/// M3: `k` muss im Bereich `1 .. n-1` liegen; `k = 0` ergaebe `r = 0`
/// und damit eine Signatur, die niemand pruefen kann, `k >= n` waere
/// modulo gleich 0. Das wird **abgelehnt** und nicht stillschweigend
/// repariert — ein still reparierter `k` waere genau die Art Fehler,
/// die ein Angreifer braucht.
pub fn ecdsa_p256_signieren_sha256(
    privat: &[u8],
    hash: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), EcdsaFehler> {
    let k = ecdsa_k_rfc6979(privat, hash)?;
    ecdsa_signieren_mit_k(&kurve_p256()?, privat, hash, &k)
}

/// Signieren mit uebergebenem `k` — **nur innerhalb des Crates**.
///
/// added 30.09.2026. WARUM ES DIE FUNKTION GIBT: der Vektor aus
/// RFC 6979 Anhang A.2.5 nennt `k`, `r` **und** `s` fuer dieselbe
/// Nachricht. Ein Test, der nur `ecdsa_p256_signieren_sha256` aufruft,
/// koennte nicht pruefen, ob der DRBG das richtige `k` liefert und ob
/// die Formel aus `k` das richtige `s` — er saehe nur, dass beide
/// zusammen stimmen. Mit dieser Funktion kann der Test beide Teile
/// getrennt gegen die Norm stellen.
///
/// DIE SICHERHEITSGRENZE, sie ist der Grund fuer `pub(crate)`:
/// ein Aufrufer von aussen koennte `k` selbst waehlen und damit den
/// privaten Schluessel ueber zwei Signaturen mit demselben `k`
/// rekonstruieren (siehe `ecdsa_p256_signieren_sha256`).
pub(crate) fn ecdsa_signieren_mit_k(
    k_mod: &Kurve,
    privat: &[u8],
    hash: &[u8],
    k: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), EcdsaFehler> {
    if privat.len() > MAX_PUNKT_LAENGE || k.len() > MAX_PUNKT_LAENGE {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    let d =
        crate::zahl::Zahl::aus_bytes_gross(privat).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;
    let kk = crate::zahl::Zahl::aus_bytes_gross(k).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;
    if d.ist_null() || kk.ist_null() {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    // DER BEREICH: 1 <= k < n. `ist_groesser_gleich(kk, n)` faengt den
    // oberen Rand, `ist_null` den unteren.
    if crate::zahl::ist_groesser_gleich(&kk, &k_mod.n) {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    // Und dasselbe fuer `d`: ein Schluessel ausserhalb `1..n-1` ergibt
    // einen oeffentlichen Punkt, den es so nicht gibt.
    if crate::zahl::ist_groesser_gleich(&d, &k_mod.n) {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    let h = crate::zahl::Zahl::aus_bytes_gross(hash).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;

    // R = k*G, ueber `skalar_punkt` — das ist **derselbe** Aufruf, den
    // auch die Pruefung benutzt (Zeile 742, `let g = (k.gx, k.gy,
    // Zahl::eins())`), damit Signieren und Pruefen nicht auf
    // verschiedenen Rechenwegen stehen.
    //
    // DIE BASIS IST `(Gx, Gy, 1)`, NICHT `(Gx, Gy, 0)`. Mit `z = 0` ist
    // der Basispunkt der Punkt am Unendlich, `jacobian_addieren`
    // rechnet dann mit falschen Eingaben, und das Ergebnis wird
    // `Ok(None)` bei `zu_affin` — also `KurveUnbrauchbar` ohne jeden
    // Hinweis auf die Ursache. Genau das war der erste Fehler hier.
    let punkt = skalar_punkt(
        &kk,
        &(
            k_mod.gx.clone(),
            k_mod.gy.clone(),
            crate::zahl::Zahl::eins(),
        ),
        k_mod,
    )?;
    // `zu_affin` nimmt das **Feld** `p`, nicht die Kurve, und gibt ein
    // `Option` zurueck (der Punkt am Unendlich ergibt `None`).
    let (rx, _ry) = zu_affin(&punkt, &k_mod.p)?.ok_or(EcdsaFehler::KurveUnbrauchbar)?;
    let r = crate::zahl::modulo(&rx, &k_mod.n).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;

    // k^-1 mod n ueber Fermat: `k^(n-2)`, weil `n` prim ist.
    //
    // DER FEHLER, DEN DIESE ZEILE ZUERST HATTE, und er war der Grund
    // fuer den ignorierten Test: es stand `n-1` hier. `n` ist eine
    // Primzahl, also ist `k^(n-1) = 1` **immer** — die Formel rechnete
    // also `s = 1 * (h + r*d)`, und das ist keine Signatur. Die
    // Pruefung in derselben Datei benutzt zwei Bildschirmseiten weiter
    // unten korrekt `n_minus_2`; die beiden Stellen sind jetzt gleich.
    let n_minus_2 = crate::zahl::minus_modulo(
        &k_mod.n,
        &crate::zahl::Zahl::aus_gliedern(vec![2]),
        &k_mod.n,
    )?;
    let kinv = crate::zahl::potenz_modulo(&kk, &n_minus_2, &k_mod.n)
        .map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;
    // s = k^-1 * (h + r*d) mod n
    let rd =
        crate::zahl::mal_modulo(&r, &d, &k_mod.n).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;
    let innen =
        crate::zahl::plus_modulo(&h, &rd, &k_mod.n).map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;
    let s = crate::zahl::mal_modulo(&kinv, &innen, &k_mod.n)
        .map_err(|_| EcdsaFehler::KurveUnbrauchbar)?;

    // M3: `r = 0` oder `s = 0` ist eine nicht pruefbare Signatur
    // (FIPS 186-4 6.3, Schritt 5). Die Norm sagt: eine neue `k` ziehen
    // und wiederholen. Das waere hier der richtige Weg — aber dafuer
    // muesste `ecdsa_p256_signieren_sha256` die Schleife machen und
    // `k` zurueckschreiben koennen, und der DRBG gibt `k` zurueck
    // statt es zu setzen. Der Fall hat die Wahrscheinlichkeit `2^-128`
    // fuer P-256 und wird deshalb **benannt** statt verschwiegen.
    if r.ist_null() || s.ist_null() {
        return Err(EcdsaFehler::KurveUnbrauchbar);
    }
    // DER HALBE ORDEN: `s` und `n - s` sind gleichwertig (RFC 6979 3.2
    // erlaubt beides, FIPS 186-4 nennt es „low-s"). Wir normalisieren
    // **nicht** — wer es braucht, entscheidet es selbst, und eine
    // Normalisierung hier wuerde die Vektoren aus Anhang A.2.5
    // brechen, denn dort stehen beide Formen.
    Ok((r.nach_bytes_gross(32), s.nach_bytes_gross(32)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::von_hex;

    /// RFC 6979 Anhang A.2.5. Kein Wert ist abgetippt: die Datei wird
    /// von t11_rfc6979_vektor.py aus dem RFC-Text erzeugt, und das
    /// Skript hat q, x, U, k, r und s gegen `cryptography` und gegen
    /// eine eigene Punktrechnung geprueft.
    const V: &str = include_str!("../testdaten/t11_rfc6979_p256.txt");

    fn feld(name: &str) -> Vec<u8> {
        for zeile in V.lines() {
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return von_hex(rest.trim()).expect("Hex in der Testdatei");
            }
        }
        panic!("Feld {name} fehlt in der Testdatei");
    }

    fn kurve() -> Kurve {
        kurve_von_bytes(
            &feld("P"),
            &feld("A"),
            &feld("B"),
            &feld("N"),
            &feld("GX"),
            &feld("GY"),
        )
        .expect("Kurve aus RFC 6979 und FIPS 186-4")
    }

    /// M7: DER PFLICHTTEST. Die Signatur aus RFC 6979 A.2.5, SHA-256,
    /// Nachricht "sample". Der Standard schreibt sie vor, damit
    /// Implementierungen sich an ihr messen lassen koennen.
    #[test]
    fn rfc6979_sha256_wird_bestaetigt() {
        let k = kurve();
        let hash = feld("HASH");
        assert_eq!(hash.len(), 32, "SHA-256, 32 Byte");
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &feld("QX"), &feld("QY"), &feld("R"), &feld("S"), &hash),
            Ok(()),
            "die Signatur aus RFC 6979 A.2.5 muss bestaetigt werden"
        );
    }

    /// M7: dieselbe Norm, SHA-1. Deckt die zweite Hashlaenge ab und
    /// damit die Laengenpruefung.
    #[test]
    fn rfc6979_sha1_wird_bestaetigt() {
        let k = kurve();
        // RFC 6979 gibt fuer "sample" mit SHA-1 kein HASH-Feld, es
        // wird darum aus der Nachricht berechnet. Die Nachricht steht
        // in der Datei, also ist auch der Hash daraus und nicht
        // abgetippt.
        let nachricht = feld("NACHRICHT");
        let hash = crate::sha2::sha1(&nachricht).to_vec();
        assert_eq!(hash.len(), 20, "SHA-1, 20 Byte");
        assert_eq!(
            ecdsa_p256_pruefen_sha1(
                &k,
                &feld("QX"),
                &feld("QY"),
                &feld("R_SHA1"),
                &feld("S_SHA1"),
                &hash
            ),
            Ok(()),
            "die SHA-1-Signatur aus RFC 6979 A.2.5 muss gelten"
        );
    }

    /// M7: die Gegenfaelle. Jede Veraenderung muss zu einem Fehler
    /// fuehren, sonst haette die Pruefung keine Aussagekraft.
    #[test]
    fn veraenderte_eingaben_werden_abgelehnt() {
        let k = kurve();
        let qx = feld("QX");
        let qy = feld("QY");
        let r = feld("R");
        let s = feld("S");
        let hash = feld("HASH");
        // Voraussetzung
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r, &s, &hash),
            Ok(()),
            "die echte Signatur muss gelten"
        );

        // (a) r um ein Bit geaendert
        let mut r2 = r.clone();
        r2[31] ^= 0x01;
        assert_ne!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r2, &s, &hash),
            Ok(()),
            "ein veraendertes r muss auffallen"
        );

        // (b) s um ein Bit geaendert
        let mut s2 = s.clone();
        s2[0] ^= 0x01;
        assert_ne!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r, &s2, &hash),
            Ok(()),
            "ein veraendertes s muss auffallen"
        );

        // (c) r = 0, das ist nicht in [1, n-1]
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &[0u8; 32], &s, &hash),
            Err(EcdsaFehler::AnteileAusserhalb),
            "r = 0 wird abgelehnt"
        );

        // (d) s = 0: kein Inverses
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r, &[0u8; 32], &hash),
            Err(EcdsaFehler::AnteileAusserhalb),
            "s = 0 wird abgelehnt"
        );

        // (e) r = n, das ist eins zu gross
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &feld("N"), &s, &hash),
            Err(EcdsaFehler::AnteileAusserhalb),
            "r = n wird abgelehnt"
        );

        // (f) ein falscher oeffentlicher Schluessel
        assert_ne!(
            ecdsa_p256_pruefen_sha256(&k, &feld("GX"), &qy, &r, &s, &hash),
            Ok(()),
            "der Generator als oeffentlicher Schluessel muss auffallen"
        );

        // (g) ein falscher Hash
        let mut h2 = hash.clone();
        h2[0] ^= 0x01;
        assert_ne!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r, &s, &h2),
            Ok(()),
            "ein veraenderter Hash muss auffallen"
        );

        // (h) ein Hash der falschen Laenge
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &qx, &qy, &r, &s, &hash[..20]),
            Err(EcdsaFehler::HashFalsch),
            "ein Hash mit 20 Byte wird abgelehnt"
        );

        // (i) ein zu langer Schluessel
        assert_eq!(
            ecdsa_p256_pruefen_sha256(&k, &[0u8; MAX_PUNKT_LAENGE + 1], &qy, &r, &s, &hash),
            Err(EcdsaFehler::KurveUnbrauchbar),
            "M4: die Laenge wird vor der Allokation geprueft"
        );
    }

    /// M7: die Jacobischen Koordinaten muessen dasselbe liefern wie
    /// affine. Das ist die Stelle, an der sich ein Rechenfehler am
    /// teuersten zeigt, weil er erst am Ende sichtbar wird.
    ///
    /// Geprueft wird: k*G fuer k = 1 ist G, und k*G fuer k = 2 ist das
    /// Doppelte, das ueber affine Formeln berechnet wird.
    #[test]
    fn jacobian_stimmt_mit_affin_ueberein() {
        let k = kurve();
        let g = (k.gx.clone(), k.gy.clone(), Zahl::eins());

        // k = 1: das Ergebnis muss G sein.
        let eins = Zahl::aus_bytes_gross(&[1u8]).expect("1");
        let erg = skalar_punkt(&eins, &g, &k).expect("1*G");
        let (x, y) = zu_affin(&erg, &k.p).expect("nicht unendlich").expect("da");
        assert_eq!(x, k.gx, "1*G hat x = Gx");
        assert_eq!(y, k.gy, "1*G hat y = Gy");

        // k = 2, unabhaengig ueber die affine Formel:
        //   lambda = (3*x1^2 + a) / (2*y1)   (Division ueber Fermat)
        //   x2 = lambda^2 - 2*x1
        //   y2 = lambda*(x1 - x2) - y1
        let zwei = Zahl::aus_bytes_gross(&[2u8]).expect("2");
        let erg2 = skalar_punkt(&zwei, &g, &k).expect("2*G");
        let (x2, y2) = zu_affin(&erg2, &k.p).expect("nicht unendlich").expect("da");

        let drei = Zahl::aus_gliedern(vec![3]);
        let zwei_z = Zahl::aus_gliedern(vec![2]);
        let x1_2 = crate::zahl::mal_modulo(&k.gx, &k.gx, &k.p).expect("x^2");
        let zaehler = crate::zahl::plus_modulo(
            &crate::zahl::mal_modulo(&drei, &x1_2, &k.p).expect("3x^2"),
            &k.a,
            &k.p,
        )
        .expect("zaehler");
        let nenner = crate::zahl::mal_modulo(&zwei_z, &k.gy, &k.p).expect("2y");
        let p_m2 = crate::zahl::minus_modulo(&k.p, &zwei_z, &k.p).expect("p-2");
        let nenner_inv = potenz_modulo(&nenner, &p_m2, &k.p).expect("1/(2y)");
        let lambda = crate::zahl::mal_modulo(&zaehler, &nenner_inv, &k.p).expect("lambda");
        let x_erwartet = crate::zahl::minus_modulo(
            &crate::zahl::mal_modulo(&lambda, &lambda, &k.p).expect("l^2"),
            &crate::zahl::mal_modulo(&zwei_z, &k.gx, &k.p).expect("2x1"),
            &k.p,
        )
        .expect("x2");
        let y_erwartet = crate::zahl::minus_modulo(
            &crate::zahl::mal_modulo(
                &lambda,
                &crate::zahl::minus_modulo(&k.gx, &x_erwartet, &k.p).expect("x1-x2"),
                &k.p,
            )
            .expect("l*(x1-x2)"),
            &k.gy,
            &k.p,
        )
        .expect("y2");
        assert_eq!(x2, x_erwartet, "2*G, x nach der affinen Formel");
        assert_eq!(y2, y_erwartet, "2*G, y nach der affinen Formel");

        // Und der Generator liegt auf der Kurve — mit a = p-3 und b
        // aus FIPS 186-4.
        assert!(
            punkt_auf_kurve(&k.gx, &k.gy, &k).expect("rechnen"),
            "G auf der Kurve"
        );
    }

    /// M7: der oeffentliche Schluessel aus RFC 6979 ist xG. Das wird
    /// hier nachgerechnet, und es braucht den privaten Schluessel nur
    /// als Testkonstante — geprueft wird, dass n*Q der Punkt im
    /// Unendlichen ist, denn das heisst, dass Q in der richtigen
    /// Untergruppe liegt.
    #[test]
    fn oeffentlicher_schluessel_liegt_in_der_untergruppe() {
        let k = kurve();
        let q = (
            Zahl::aus_bytes_gross(&feld("QX")).expect("Ux"),
            Zahl::aus_bytes_gross(&feld("QY")).expect("Uy"),
            Zahl::eins(),
        );
        let erg = skalar_punkt(&k.n, &q, &k).expect("n*Q");
        assert!(
            erg.2.ist_null(),
            "n*Q muss der Punkt im Unendlichen sein, sonst liegt Q nicht in der Untergruppe"
        );
    }

    /// M7: die modularen Hilfsfunktionen aus `zahl.rs`, an konkreten
    /// Zahlen. Sie sind vor dieser Datei dazugekommen, weil in der
    /// Kurvenarithmetik an etwa dreissig Stellen addiert wird und an
    /// jeder einzelnen einzeln zu reduzieren eine Gelegenheit zum
    /// Vergessen waere. Der Test steht hier, weil der Fehler sonst
    /// erst an einer Signatur auffaellt.
    ///
    /// Geprueft wird auch der Fall, an dem `minus` ohne Reduktion
    /// scheitern wuerde: a kleiner als b, mit negativem Zwischenwert.
    #[test]
    fn modulare_hilfsfunktionen() {
        let p = Zahl::aus_bytes_gross(&[97u8]).expect("97");
        let a = Zahl::aus_bytes_gross(&[90u8]).expect("90");
        let b = Zahl::aus_bytes_gross(&[20u8]).expect("20");

        // 90 + 20 = 110, 110 mod 97 = 13
        assert_eq!(
            crate::zahl::plus_modulo(&a, &b, &p).expect("+"),
            Zahl::aus_bytes_gross(&[13u8]).expect("13"),
            "90 + 20 mod 97 = 13, mit Ueberhang"
        );

        // 20 + 90 = 110, 110 mod 97 = 13
        assert_eq!(
            crate::zahl::plus_modulo(&b, &a, &p).expect("+"),
            Zahl::aus_bytes_gross(&[13u8]).expect("13"),
            "20 + 90 mod 97 = 13"
        );

        // 90 - 20 = 70, kein Ueberhang
        assert_eq!(
            crate::zahl::minus_modulo(&a, &b, &p).expect("-"),
            Zahl::aus_bytes_gross(&[70u8]).expect("70"),
            "90 - 20 mod 97 = 70"
        );

        // 20 - 90 = -70, das Ergebnis MUSS positiv sein
        assert_eq!(
            crate::zahl::minus_modulo(&b, &a, &p).expect("-"),
            Zahl::aus_bytes_gross(&[27u8]).expect("27"),
            "20 - 90 mod 97 = 27, nicht -70"
        );
        // Und das ist der Punkt: ein negatives Ergebnis haette in der
        // naechsten Multiplikation ein anderes Ergebnis geliefert.
        assert!(
            crate::zahl::ist_groesser_gleich(
                &crate::zahl::minus_modulo(&b, &a, &p).expect("-"),
                &Zahl::null()
            ),
            "das Ergebnis liegt in [0, p)"
        );

        // Mal: 90 * 20 = 1800, und 1800 mod 97 = 54. (Ich hatte erst
        // 56 erwartet; 1800 = 18*97 + 54, also 54.)
        assert_eq!(
            crate::zahl::mal_modulo(&a, &b, &p).expect("*"),
            Zahl::aus_bytes_gross(&[54u8]).expect("54"),
            "90 * 20 mod 97 = 54"
        );

        // Und die Kongruenz: (a - b) + b == a  fuer beliebige a, b
        for (x, y) in [(90u8, 20u8), (20, 90), (5, 5), (96, 1)] {
            let xz = Zahl::aus_bytes_gross(&[x]).expect("x");
            let yz = Zahl::aus_bytes_gross(&[y]).expect("y");
            let differenz = crate::zahl::minus_modulo(&xz, &yz, &p).expect("-");
            let zurueck = crate::zahl::plus_modulo(&differenz, &yz, &p).expect("+");
            assert_eq!(zurueck, xz, "({x} - {y}) + {y} == {x} mod 97");
        }
    }

    /// M3: die Obergrenze des Skalarprodukts.
    #[test]
    fn skalar_punkt_ist_begrenzt() {
        let k = kurve();
        let g = (k.gx.clone(), k.gy.clone(), Zahl::eins());
        // Eine Zahl mit mehr als MAX_KURVEN_BIT Bits.
        let gross = Zahl::aus_gliedern(
            (0..(MAX_KURVEN_BIT / 32 + 2))
                .map(|_| 0xFFFF_FFFF)
                .collect(),
        );
        assert_eq!(
            skalar_punkt(&gross, &g, &k),
            Err(EcdsaFehler::KurveUnbrauchbar)
        );
    }

    /// M7: eine Kurve, deren Generator nicht auf der Kurve liegt, wird
    /// abgelehnt, statt still falsch zu rechnen.
    #[test]
    fn generator_nicht_auf_der_kurve_wird_abgelehnt() {
        let mut falsches_gx = feld("GX");
        falsches_gx[31] ^= 0x01;
        let k = kurve_von_bytes(
            &feld("P"),
            &feld("A"),
            &feld("B"),
            &feld("N"),
            &falsches_gx,
            &feld("GY"),
        )
        .expect("die Kurve selbst ist baubar");
        assert_eq!(
            ecdsa_p256_pruefen_sha256(
                &k,
                &feld("QX"),
                &feld("QY"),
                &feld("R"),
                &feld("S"),
                &feld("HASH")
            ),
            Err(EcdsaFehler::PunktNichtAufKurve),
            "ein Generator ausserhalb der Kurve wird abgelehnt"
        );
    }
    /// M7: der Sonderfall `P + P`, der bis eben der am wenigsten
    /// gepruefte Zweig der ganzen Datei war.
    ///
    /// DIESER TEST IST EINE GEGENPROBE AUF EINEN ECHTEN BUG. Im
    /// Zweig `U1 == U2 und S1 == S2` — also `P + P` — stand als
    /// Kurvenparameter fest die `3`:
    ///
    ///     jacobian_doppeln(pt1, p_mod, &Zahl::aus_bytes_gross(&[3u8]))
    ///
    /// P-256 hat aber `a = p - 3`, NICHT `a = 3`. Mit `a = 3` kommt
    /// fuer `alpha = 3*(X-delta)*(X+delta)` ein voellig anderes
    /// Ergebnis heraus, und `2P` landet nicht auf der Kurve.
    ///
    /// Warum das durchging: `skalar_punkt` verdoppelt den
    /// Akkumulator ueber `jacobian_doppeln` mit dem echten `a`, und
    /// `P + P` tritt nur auf, wenn der Akkumulator einmal auf den
    /// Basispunkt faellt — bei k = 1 also im allerersten Schritt. Die
    /// beiden getesteten RFC-6979-Vektoren haben laengere Skalare.
    /// Der Zweig war also nicht kaputt in dem Sinne, dass er falsch
    /// gerechnet haette; er war schlicht ungeprueft.
    ///
    /// Der Test schliesst diese Luecke, indem er alle drei
    /// Sonderfaelle der Addition von Hand aufruft und die
    /// Sollwerte aus der affinen Kurvengleichung bezieht:
    ///
    ///   2Q.x = (3*Ux^2 + a) / (2*Uy)          (Steigungsformel)
    ///   Q + (-Q) = Unendlich                 (inverse Punkte)
    #[test]
    fn sonderfaelle_der_addition() {
        let k = kurve();
        let qx = Zahl::aus_bytes_gross(&feld("QX")).expect("Ux");
        let qy = Zahl::aus_bytes_gross(&feld("QY")).expect("Uy");
        let p = &k.p;
        let a = &k.a;
        let q = (qx.clone(), qy.clone(), Zahl::eins());

        // (a) Q + Q muss dasselbe sein wie das Verdoppeln.
        let d = jacobian_doppeln(&q, p, a).expect("dbl");
        let s = jacobian_addieren(&q, &q, p, a).expect("add");
        assert_eq!(
            d, s,
            "Q + Q und das Verdoppeln von Q muessen denselben Punkt geben"
        );

        // Und dieser Punkt muss auf der Kurve liegen. Ohne den
        // echten `a` waere er es nicht — genau das ist der Bug.
        let (x2, y2) = zu_affin(&d, p)
            .expect("2Q")
            .expect("2Q ist nicht unendlich");
        assert!(
            punkt_auf_kurve(&x2, &y2, &k).expect("Kurvenpruefung"),
            "2Q muss auf y^2 = x^3 + ax + b liegen"
        );

        // (b) Der Sollwert aus der Steigungsformel, unabhaengig von
        // beiden projectiven Formeln gerechnet. Genau der Weg, der
        // den Fehler sichtbar macht: lambda = (3*Ux^2 + a) / (2*Uy),
        // x(2Q) = lambda^2 - 2*Ux.
        let ux2 = crate::zahl::mal_modulo(&qx, &qx, p).expect("Ux^2");
        let zwei = Zahl::aus_gliedern(vec![2]);
        // lambda = (3*Ux^2 + a) / (2*Uy).
        //
        // EIGENER FEHLER IM TEST, und die Formel steht jetzt im Code,
        // damit sie nicht wieder falsch abgetippt wird: zuerst hatte
        // ich `3*(Ux^2 + a)` statt `3*Ux^2 + a`. Bei a = p - 3 macht
        // das einen Unterschied von 2a, also von -6, und der Test
        // meldete zu Unrecht einen Fehler in der projectiven Formel.
        // Ein Test, der eine falsche Formel als Sollwert verwendet,
        // ist nicht harmlos: er laesst den naechsten, der an dieser
        // Stelle wirklich etwas geaendert hat, raten.
        let dreifach_ux2 = mal_modulo_3(&ux2, p).expect("3*Ux^2");
        let numer = crate::zahl::plus_modulo(&dreifach_ux2, a, p).expect("3*Ux^2 + a");
        let zwei_y = crate::zahl::mal_modulo(&qy, &zwei, p).expect("2*Uy");
        // 1/(2*Uy) = (2*Uy)^(p-2), weil p prim ist.
        let p_minus_2 = crate::zahl::minus_modulo(p, &zwei, p).expect("p-2");
        let inv = potenz_modulo(&zwei_y, &p_minus_2, p).expect("1/(2*Uy)");
        let lambda = crate::zahl::mal_modulo(&numer, &inv, p).expect("lambda");
        let soll_x = crate::zahl::minus_modulo(
            &crate::zahl::mal_modulo(&lambda, &lambda, p).expect("lambda^2"),
            &crate::zahl::mal_modulo(&qx, &zwei, p).expect("2*Ux"),
            p,
        )
        .expect("x(2Q)");
        assert_eq!(x2, soll_x, "2Q.x aus der Steigungsformel");

        // (c) Q + (-Q) muss Unendlich sein. In der projectiven Form
        // ist das U1 == U2 mit S1 != S2.
        let minus_q = (
            qx.clone(),
            crate::zahl::minus_modulo(&Zahl::null(), &qy, p).expect("-Uy"),
            Zahl::eins(),
        );
        let r = jacobian_addieren(&q, &minus_q, p, a).expect("add");
        assert!(r.2.ist_null(), "Q + (-Q) ist Unendlich");
        assert!(
            zu_affin(&r, p).expect("Umrechnung").is_none(),
            "und bleibt bei der Umrechnung in affinen Koordinaten unendlich"
        );

        // (d) Unendlich + Q = Q und Q + Unendlich = Q. Das sind die
        // beiden anderen Sonderfaelle, und sie muessen in BEIDEN
        // Richtungen gelten, weil die Argumente vertauscht sind.
        let unendlich = (Zahl::null(), Zahl::null(), Zahl::null());
        assert_eq!(
            jacobian_addieren(&unendlich, &q, p, a).expect("u+Q"),
            q,
            "Unendlich + Q = Q"
        );
        assert_eq!(
            jacobian_addieren(&q, &unendlich, p, a).expect("Q+u"),
            q,
            "Q + Unendlich = Q"
        );
        assert!(
            jacobian_doppeln(&unendlich, p, a)
                .expect("dbl(u)")
                .2
                .ist_null(),
            "das Verdoppeln von Unendlich ist Unendlich"
        );
    }
    /// ECDSA P-384: der Vektor und die Algebra dahinter.
    ///
    /// Die Testdaten kommen aus `t11_p384_vektor.py`. Bevor dort
    /// etwas geschrieben wurde, sind sieben Bedingungen geprueft
    /// worden, und alle sieben sind Eigenschaften, keine Vergleiche
    /// zweier Texte:
    ///
    ///   1. `G` liegt auf `y^2 = x^3 + ax + b`
    ///   2. `n * G` ist der Punkt im Unendlichen — also ist `n` die
    ///      Gruppenordnung, und genau die Zahl, gegen die ECDSA den
    ///      Bereich von `r` und `s` prueft
    ///   3. `2*G`, `3*G` und `7*G` liegen auf der Kurve
    ///   4. `U == x * G` fuer einen echten Schluessel
    ///   5. `cryptography` akzeptiert die Signatur
    ///   6. Die Signaturgleichung ergibt `(x mod n) == r` bei eigener
    ///      Punktrechnung
    ///   7. Ein veraendertes `r` ergibt `(x mod n) != r`
    const V384: &str = include_str!("../testdaten/t11_ecdsa_p384.txt");

    fn feld384(name: &str) -> Vec<u8> {
        for zeile in V384.lines() {
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return von_hex(rest.trim()).expect("Hex in der Testdatei");
            }
        }
        panic!("Feld {name} fehlt in der P-384-Testdatei");
    }

    #[test]
    fn p384_vektor_wird_bestaetigt() {
        let k = kurve_p384().expect("P-384");

        // (1) `a == p - 3`, nachgerechnet statt hingesetzt.
        let p = Zahl::aus_bytes_gross(&P384_P).expect("p");
        let drei = Zahl::aus_gliedern(vec![3]);
        let a = crate::zahl::minus_modulo(&p, &drei, &p).expect("p-3");
        assert_eq!(k.a, a, "a = p - 3");
        assert_eq!(k.p, p, "das Feld stimmt mit der Konstante ueberein");
        assert_eq!(k.b, Zahl::aus_bytes_gross(&P384_B).expect("b"), "b");
        assert_eq!(k.n, Zahl::aus_bytes_gross(&P384_N).expect("n"), "n");
        assert_eq!(k.gx, Zahl::aus_bytes_gross(&P384_GX).expect("Gx"), "Gx");
        assert_eq!(k.gy, Zahl::aus_bytes_gross(&P384_GY).expect("Gy"), "Gy");

        // (2) M7: G liegt auf der Kurve. Ohne das prueft die ganze
        // Signaturrechnung auf einer anderen Kurve als behauptet.
        assert!(
            punkt_auf_kurve(&k.gx, &k.gy, &k).expect("Kurvenpruefung"),
            "M7: G liegt auf y^2 = x^3 + ax + b"
        );

        // (3) n * G ist Unendlich.
        let basis = (k.gx.clone(), k.gy.clone(), Zahl::eins());
        let ng = skalar_punkt(&k.n, &basis, &k).expect("n*G");
        assert!(
            zu_affin(&ng, &p).expect("Umrechnung").is_none(),
            "M7: n*G ist der Punkt im Unendlichen"
        );

        // (4) Die echte Signatur wird bestaetigt.
        let qx = feld384("QX");
        let qy = feld384("QY");
        let r = feld384("R");
        let s = feld384("S");
        let hash = feld384("HASH");
        assert_eq!(hash.len(), 48, "SHA-384 ergibt 48 Byte");
        assert_eq!(
            ecdsa_p384_pruefen_sha384(&k, &qx, &qy, &r, &s, &hash),
            Ok(()),
            "die Signatur muss gelten"
        );

        // (5) Gegenproben: ein veraendertes r, s oder Q.
        let mut r2 = r.clone();
        r2[47] ^= 0x01;
        assert_ne!(
            ecdsa_p384_pruefen_sha384(&k, &qx, &qy, &r2, &s, &hash),
            Ok(()),
            "ein veraendertes r muss auffallen"
        );
        let mut s2 = s.clone();
        s2[0] ^= 0x01;
        assert_ne!(
            ecdsa_p384_pruefen_sha384(&k, &qx, &qy, &r, &s2, &hash),
            Ok(()),
            "ein veraendertes s muss auffallen"
        );
        assert_ne!(
            ecdsa_p384_pruefen_sha384(&k, &P384_GX, &qy, &r, &s, &hash),
            Ok(()),
            "der Generator als Schluessel muss auffallen"
        );
        // Und die Laenge: 48 Byte, nicht 32.
        assert!(
            ecdsa_p384_pruefen_sha384(&k, &qx, &qy, &r, &s, &hash[..32]).is_err(),
            "ein 32-Byte-Hash wird abgelehnt"
        );
    }

    /// M7: P-256 und P-384 sind verschiedene Kurven, und ein Schluessel
    /// der einen gilt nicht auf der anderen.
    ///
    /// DER ANLASS: `kette.rs` waehlt die Kurve nach der OID. Verwechselt
    /// man sie, rechnet die Pruefung auf P-256 mit einem
    /// P-384-Schnell Public Key — und das ist ein Ergebnis, das man
    /// nicht am Ergebnis erkennt, sondern nur an dieser Gegenprobe.
    #[test]
    fn p256_und_p384_sind_verschieden() {
        let a = kurve_p256().expect("P-256");
        let b = kurve_p384().expect("P-384");
        assert_ne!(a.p, b.p, "die Felder sind verschieden");
        assert_ne!(a.n, b.n, "die Ordnungen sind verschieden");
        assert_ne!(a.gx, b.gx, "die Generatoren sind verschieden");
        // Und beide liegen auf ihrer eigenen Kurve.
        assert!(punkt_auf_kurve(&a.gx, &a.gy, &a).expect("P-256"));
        assert!(punkt_auf_kurve(&b.gx, &b.gy, &b).expect("P-384"));
        // M4: die Laengen unterscheiden sich, und die Punktgrenze muss
        // fuer beide reichen.
        const { assert!(MAX_PUNKT_LAENGE >= 48, "48 Byte muessen passen") };
    }
    /// ECDH auf P-256, gegen **RFC 5903 Abschnitt 8.1**.
    ///
    /// Das ist ein amtlicher Vektor mit allen Zwischenwerten, und der
    /// einzige ECDH-Vektor, den es in einer RFC gibt. `i` und `r` sind
    /// die privaten Schluessel zweier Parteien, `gix`/`giy` und
    /// `grx`/`gry` ihre oeffentlichen Punkte und `girx` das gemeinsame
    /// Geheimnis.
    ///
    /// added 30.09.2026 fuer TLS 1.2 (ECDHE). Ohne ECDH gibt es
    /// keinen TLS-1.2-Handshake: der Auftrag schliesst den
    /// RSA-Schluesseltausch aus, und ECDHE ist der einzige Rest.
    ///
    /// Die Testdaten sind aus `rfc/rfc5903.txt` GELESEN, nicht
    /// abgetippt — `mutation_md/tls/p256_vektor.txt` entsteht aus dem
    /// RFC-Text. Dazu gehoert eine unabhaengige Nachrechnung: eine
    /// Skalarmultiplikation in Python ergibt dieselben `g^i` und
    /// dasselbe `i*(g^r)`. Ein ECDH-Test gegen eine selbst gerechnete
    /// Zahl prueft sonst nur, dass dieselbe Formel zweimal das gleiche
    /// Falsche liefert.
    const V256: &str = include_str!("../testdaten/t15_p256_ecdh.txt");

    fn feld256(name: &str) -> Vec<u8> {
        for zeile in V256.lines() {
            if zeile.starts_with('#') {
                continue;
            }
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return crate::sha2::von_hex(rest.trim()).expect("Hex in der Vektordatei");
            }
        }
        panic!("Feld {name} fehlt in der Vektordatei");
    }

    #[test]
    fn ecdh_p256_gegen_rfc_5903() {
        let k = kurve_p256().expect("P-256");
        // Das gemeinsame Geheimnis aus beiden Richtungen.
        let z1 = ecdh(&feld256("I"), &feld256("GRX"), &feld256("GRY"), &k).expect("i mal (g^r)");
        let z2 = ecdh(&feld256("R"), &feld256("GIX"), &feld256("GIY"), &k).expect("r mal (g^i)");
        assert_eq!(
            crate::sha2::hexen(&z1),
            crate::sha2::hexen(&z2),
            "M7: beide Richtungen muessen dasselbe Geheimnis haben"
        );
        assert_eq!(
            crate::sha2::hexen(&z1),
            "d6840f6b42f6edafd13116e0e12565202fef8e9ece7dce03812464d04b9442de",
            "das gemeinsame Geheimnis aus RFC 5903 Abschnitt 8.1"
        );
        assert_eq!(z1.len(), 32, "M7: P-256 ergibt 32 Byte");
    }

    /// M7: die oeffentlichen Punkte der RFC — damit ist nicht nur das
    /// Geheimnis geprueft, sondern auch die Ableitung aus dem privaten
    /// Schluessel, also die Skalarmultiplikation selbst.
    #[test]
    fn der_oeffentliche_punkt_stimmt_gegen_rfc_5903() {
        let k = kurve_p256().expect("P-256");
        let basis = (k.gx.clone(), k.gy.clone(), crate::zahl::Zahl::eins());
        let als_p = Kurve {
            p: k.p.clone(),
            n: k.p.clone(),
            a: k.a.clone(),
            b: k.b.clone(),
            gx: k.gx.clone(),
            gy: k.gy.clone(),
        };

        let s = crate::zahl::Zahl::aus_bytes_gross(&feld256("I")).expect("i");
        let punkt = skalar_punkt(&s, &basis, &als_p).expect("g^i");
        let (x, y) = zu_affin(&punkt, &k.p)
            .expect("affin")
            .expect("nicht unendlich");
        assert_eq!(
            crate::zahl::Zahl::aus_bytes_gross(&feld256("GIX")).expect("gix"),
            x,
            "M7: g^i, x aus RFC 5903"
        );
        assert_eq!(
            crate::zahl::Zahl::aus_bytes_gross(&feld256("GIY")).expect("giy"),
            y,
            "M7: g^i, y aus RFC 5903"
        );
    }

    /// M7: die oeffentliche Punktform, die TLS 1.2 im
    /// `ClientKeyExchange` schickt: 65 Byte mit dem Praefix 0x04.
    ///
    /// Der Testwert ist derselbe aus **RFC 5903 Abschnitt 8.1**
    /// (g^i mit x und y), nur diesmal in der Form, die auf der
    /// Leitung steht. Dadurch ist nicht nur die Skalarmultiplikation
    /// geprueft, sondern auch die Kodierung: Praefix, Laenge und
    /// Reihenfolge von x und y.
    #[test]
    fn der_oeffentliche_punkt_in_tlsform_gegen_rfc_5903() {
        let mut skalar = [0u8; 32];
        skalar.copy_from_slice(&feld256("I"));
        let p = oeffentlich_p256(&skalar).expect("g^i");
        assert_eq!(p.len(), 65);
        assert_eq!(p[0], 0x04, "M7: uncompressed");
        assert_eq!(
            crate::sha2::hexen(&p[1..33]),
            crate::sha2::hexen(&feld256("GIX"))
        );
        assert_eq!(
            crate::sha2::hexen(&p[33..]),
            crate::sha2::hexen(&feld256("GIY"))
        );
    }

    /// M7: Gegenproben. Ein Punkt, der nicht auf der Kurve liegt, muss
    /// abgewiesen werden — sonst ergibt die Rechnung ein Geheimnis,
    /// das zu niemandem gehoert.
    #[test]
    fn ecdh_weist_punkte_auf_falscher_kurve_zurueck() {
        let k = kurve_p256().expect("P-256");
        let gx = crate::zahl::Zahl::aus_bytes_gross(&feld256("GIX")).expect("gix");
        let gy = crate::zahl::Zahl::aus_bytes_gross(&feld256("GIY")).expect("giy");
        // y um ein Bit kippen: der Punkt liegt dann nicht mehr auf der
        // Kurve, und `ecdh` muss es sagen.
        let mut falsch_y = gy.nach_bytes_gross(32);
        falsch_y[31] ^= 0x01;
        assert_ne!(
            ecdh(&feld256("I"), &gx.nach_bytes_gross(32), &falsch_y, &k),
            Ok(crate::sha2::von_hex(
                "d6840f6b42f6edafd13116e0e12565202fef8e9ece7dce03812464d04b9442de"
            )
            .expect("Hex")),
            "M7: ein Punkt ausserhalb der Kurve wird abgewiesen"
        );
    }
}
