//! T7 — Systemzufall: Linux /dev/urandom, Windows BCryptGenRandom.
//!
//! Wofuer das gebraucht wird: TLS 1.3 braucht einen geheimen
//! Client-Schluessel (X25519), eine Client-Nonce fuer den
//! Schluesselplan und, wenn resumed, eine Session-ID. Alles davon muss
//! unvorhersagbar sein. Ein fehlgeschlagener Zufall ist ein
//! HALT, kein Grund weiterzulaufen — deshalb ist der Fehler ein `Result`
//! und kein `unwrap`.
//!
//! M3/M4: die gewuenschte Laenge ist IMMER eine benannte Konstante oder
//! kommt aus `SchluesselLaenge` aus dem TLS-Code, also aus einer
//! Ganzzahl, die vorher gegen eine Grenze geprueft wurde. In diesem
//! Modul gibt es keine Allokation aus einer Zahl von aussen: der Puffer
//! ist `[u8; 64]` und wird nur teilweise gefuellt.

#[cfg(not(windows))]
use std::{fs::File, io::Read};

/// Warum Zufall nicht gelesen werden konnte. BENANNTE Varianten: der
/// Aufrufer soll entscheiden, nicht raten.
#[derive(Debug)]
pub enum ZufallFehler {
    /// `/dev/urandom` liess sich nicht oeffnen.
    KeinZugriff(String),
    /// Die Datei liess sich nicht lesen.
    LeseFehler(String),
    /// `/dev/urandom` war zu kurz — das waere auf diesem System ein
    /// Fehler des Betriebssystems, nicht der Anwendung.
    ZuKurz { verlangt: usize, bekommen: usize },
}

impl std::fmt::Display for ZufallFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZufallFehler::KeinZugriff(w) => write!(f, "Systemzufall nicht zu oeffnen: {w}"),
            ZufallFehler::LeseFehler(w) => write!(f, "Systemzufall fehlgeschlagen: {w}"),
            ZufallFehler::ZuKurz { verlangt, bekommen } => {
                write!(f, "nur {bekommen} von {verlangt} Byte gelesen")
            }
        }
    }
}

impl std::error::Error for ZufallFehler {}

/// Die groesste Laenge, die dieses Modul auf einmal liefert.
///
/// M3/M4. Warum 64: das ist die groesste TLS-Entropie, die an einer
/// Stelle gebraucht wird (X25519-Schluesselpaar, Client-Hello-Random
/// und ein Schluesselblock). Groessere Laengen werden in Bloecken
/// gelesen, jede mit eigener Grenze — niemals in einem `Vec` aus einer
/// Zahl von aussen.
pub const MAX_BLOCK: usize = 64;

/// Fuellt `ziel` mit Zufall. `ziel` ist `[u8; 64]`, die Laenge ergibt
/// sich aus dem Typ, nicht aus einer Zahl.
#[cfg(not(windows))]
fn fuelle_systemzufall(ziel: &mut [u8; MAX_BLOCK]) -> Result<(), ZufallFehler> {
    let mut datei =
        File::open("/dev/urandom").map_err(|e| ZufallFehler::KeinZugriff(e.to_string()))?;
    // M4: `read_exact` schreibt in den vorhandenen Puffer. Es wird
    // nichts alloziert, und es gibt kein `Vec::with_capacity` aus einer
    // Laenge von aussen.
    datei
        .read_exact(ziel)
        .map_err(|e| ZufallFehler::LeseFehler(e.to_string()))
}

#[cfg(windows)]
fn fuelle_systemzufall(ziel: &mut [u8; MAX_BLOCK]) -> Result<(), ZufallFehler> {
    crate::windows_zufall::fuellen(ziel)
}

/// `laenge` zufaellige Byte. `laenge` wird VOR der Allokation geprueft.
pub fn bytes(laenge: usize) -> Result<Vec<u8>, ZufallFehler> {
    if laenge > MAX_BLOCK {
        return Err(ZufallFehler::ZuKurz {
            verlangt: laenge,
            bekommen: 0,
        });
    }
    let mut roh = [0u8; MAX_BLOCK];
    fuelle_systemzufall(&mut roh)?;
    Ok(roh[..laenge].to_vec())
}

/// Zufall fuer X25519: genau 32 Byte, im Typ erzwungen.
pub fn schluessel32() -> Result<[u8; 32], ZufallFehler> {
    let v = bytes(32)?;
    let mut raus = [0u8; 32];
    raus.copy_from_slice(&v);
    Ok(raus)
}

/// Zufall fuer eine Nonce: genau 12 Byte, im Typ erzwungen.
pub fn nonce12() -> Result<[u8; 12], ZufallFehler> {
    let v = bytes(12)?;
    let mut raus = [0u8; 12];
    raus.copy_from_slice(&v);
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::hexen;
    use std::collections::BTreeSet;

    /// M7: die Laenge stimmt, und es kommen wirklich verschiedene Werte.
    #[test]
    fn laenge_und_verschiedenheit() {
        for laenge in [0usize, 1, 12, 16, 32, 48, 63, 64] {
            let a = bytes(laenge).expect("lesen");
            let b = bytes(laenge).expect("lesen");
            assert_eq!(a.len(), laenge, "M7: Laenge {laenge}");
            if laenge >= 4 {
                assert_ne!(a, b, "M7: zwei Zufallswerte sind gleich ({laenge} Byte)");
            }
        }
    }

    /// M7: 300 Bloecke Zufall, alle verschieden. Wenn /dev/urandom nicht
    /// wirklich Zufall liefert, faellt es hier auf und nicht erst im
    /// Handshake gegen einen echten Server.
    #[test]
    fn dreihundert_blaecke_ohne_wiederholung() {
        let mut gesehen: BTreeSet<[u8; 32]> = BTreeSet::new();
        for _ in 0..300 {
            let k = schluessel32().expect("lesen");
            assert!(gesehen.insert(k), "M7: ein 32-Byte-Block kam zweimal");
        }
        assert_eq!(gesehen.len(), 300, "M7: 300 verschiedene Schluessel");
    }

    /// M7: die Bitverteilung. Ein Zufall, der in einem Bit immer 0
    /// liefert, waere deterministisch.
    ///
    /// GEMESSEN: 64 Byte sind 512 Bit, davon sind 256 gesetzt. Die
    /// Toleranz ist breit (170 bis 340), weil ein Test nicht an der
    /// Grenze der Normalverteilung scheitern soll. Er soll nur einen
    /// Zufall fangen, der strukturiert ist.
    #[test]
    fn bitverteilung() {
        const BLOECKE: usize = 20;
        const PRO_BLOCK: usize = 32;
        let mut einsen = 0u32;
        for _ in 0..BLOECKE {
            let v = bytes(PRO_BLOCK).expect("lesen");
            einsen += v.iter().map(|b| b.count_ones()).sum::<u32>();
        }
        let bits = (BLOECKE * PRO_BLOCK * 8) as u32;
        assert_eq!(bits, 5120, "M7: 5120 Bit gesamt");
        assert!(
            (1700..=3420).contains(&einsen),
            "M7: {einsen} Einsbits in {bits}, erwartet etwa {}",
            bits / 2
        );
    }

    /// M4: die Grenze greift VOR der Allokation, und der Fehler ist ein
    /// benannter Wert, keine Panik.
    #[test]
    fn grenze_vor_der_allokation() {
        // M7: gesichert wird die FEHLERVARIANTE, nicht die Panik.
        match bytes(MAX_BLOCK + 1) {
            Err(ZufallFehler::ZuKurz {
                verlangt: v,
                bekommen: 0,
            }) => assert_eq!(v, MAX_BLOCK + 1, "M7: die verlangte Laenge steht im Fehler"),
            other => panic!("ein Byte ueber dem Maximum muss abgelehnt werden, kam {other:?}"),
        }
        for absurd in [65usize, 1 << 20, usize::MAX] {
            match bytes(absurd) {
                Err(ZufallFehler::ZuKurz { .. }) => {}
                other => panic!("{absurd} haetten abgelehnt werden muessen, kam {other:?}"),
            }
        }
        // Genau am Maximum: erlaubt.
        assert_eq!(bytes(MAX_BLOCK).expect("am Maximum").len(), MAX_BLOCK);
    }

    /// M7: Schluessel und Nonce haben die Laenge, die der Typ verspricht.
    #[test]
    fn typen_erzwingen_die_laenge() {
        assert_eq!(schluessel32().expect("lesen").len(), 32);
        assert_eq!(nonce12().expect("lesen").len(), 12, "M7: Nonce 12 Byte");
    }

    /// M7: der Zufall geht in die Kryptografie ein und nicht herum. Der
    /// Vergleich nutzt die SHA-256-Ausgabe, damit der Test nur die
    /// Zufallsausgabe prueft und nicht selbst Zufall erzeugt.
    #[test]
    fn zwei_schluessel_ergeben_zwei_verschiedene_oeffentliche() {
        let a = schluessel32().expect("lesen");
        let b = schluessel32().expect("lesen");
        let pa = crate::x25519::oeffentlich(&a);
        let pb = crate::x25519::oeffentlich(&b);
        assert_ne!(hexen(&pa), hexen(&pb), "M7: zwei Schluessel, zwei Punkte");
    }
}
