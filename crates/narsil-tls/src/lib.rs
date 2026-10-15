//! narsil-tls — Kryptografie und TLS 1.3 in reinem Rust.
//!
//! Auftrag: team/WELLE4.txt Abschnitt 5, Rolle TLS.
//!
//! Grundsatz dieser Crate: handgeschriebene Kryptografie ist nur so gut
//! wie ihre Tests. Jede Primitive wird gegen die OFFIZIELLEN Vektoren aus
//! RFC/FIPS geprueft, woertlich im Test mit Quellenangabe. Kein Vektor = nicht
//! fertig. Vergleiche auf Geheimnisse (MAC/Tag) in konstanter Zeit.
//!
//! Nur `std`. Keine Fremd-Crates (ADR-030). Unsafe nur an der eng
//! begrenzten Windows-Systemzufalls-FFI, keine Aenderung der Kryptografie.
//!
//! Der Aufbau folgt den Phasen des Auftrags, und die Reihenfolge ist
//! Pflicht: Phase 1 (Primitive) muss vollstaendig gruen sein, bevor
//! Phase 2 (TLS) beginnt. Eine Schicht, die auf einer ungeprueften
//! Primitiv sitzt, ist schlimmer als gar keine.

pub mod aes;
/// Release 10.10.2026: Sitzungsausnahmen („Risiko akzeptieren“).
pub mod ausnahme;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_zufall;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_vertrauen;
pub mod chacha;
pub mod crl;
pub mod der;
pub mod ecdsa;
pub mod hkdf;
pub mod hmac;
pub mod info;
pub mod kette;
pub mod nachricht;
pub mod nachricht12;
pub mod ocsp;
pub mod rec12;
pub mod record;
pub mod schrittzeit;
pub mod server;
pub mod sha2;
pub mod signatur;
pub mod strom;
pub mod strom12;
pub mod ticket;
pub mod tls12;
pub mod trace;
pub mod vektor;
/// 2026-10-15: primitives for the Web Crypto API (narsil-jsvm).
pub mod web_crypto;
pub mod x25519;
pub mod x509;
pub mod zahl;
pub mod zufall;

pub use aes::{Aes, GcmFehler, gcm_chiffre, gcm_dechiffre};
pub use chacha::{
    AeadFehler, ChaCha20, Poly1305, aead_chiffre, aead_dechiffre, chacha20_anwenden, poly1305_tag,
};
pub use der::{DerFehler, DerLeser, Element};
pub use hkdf::{Fehler, expand_label_sha256, expand_label_sha384, hkdf_sha256, hkdf_sha384};
pub use hmac::{HmacSha256, HmacSha384, vergleich_gleich};
pub use info::{InfoFehler, VerbindungsInfo, ZertifikatInfo, verbindungsinfo};
pub use record::{MAX_CIPHERTEXT, MAX_EINLESEN, MAX_INNEN, MAX_NACHRICHT, NachrichtFehler};
pub use record::{Record, RecordFehler, Schluesselplan, Suite, nachrichten, records};
pub use sha2::{Sha256, Sha384, hexen, sha256, sha384, von_hex};
pub use vektor::{Leser, Schreiber, VektorFehler};
pub use x25519::{XFehler, gemeinsam, oeffentlich};
pub use zahl::{Zahl, ZahlFehler};
pub use zufall::{ZufallFehler, nonce12, schluessel32};

#[cfg(test)]
/// added 04.10.2026 03:2x, R68. **DER WÄCHTER ÜBER „GELIEFERT UND NIEMALS
/// BENUTZT“ — jetzt in `aufrufer_waechter.rs`, und er liest den ganzen, und er liest den ganzen
/// Workspace.** R67s Fassung stand hier und zählte nur diese Crate; sie
/// meldete `host_verwerfen` **falsch** (der Aufrufer steht in
/// `narsil-net/src/lib.rs:4530`) und musste die Ausnahmeliste **behaupten**.
/// Beide Lücken sind jetzt **gemessen**: der Wächter liest `crates/` zur
/// Laufzeit (GEMESSEN 04.10.: 1295 Dateien, 32,8 MB) und **belegt** jeden
/// Aufrufer, statt ihn zu behaupten.
///
/// added 04.10.2026 03:2x: **WARUM EINE EIGENE DATEI UND NICHT HIER:** der
/// Scanner ist der **eigentliche** Inhalt des Werkzeugs, und er ist der
/// Teil, der **getestet** werden muss (vier Tests). Alles in `lib.rs` zu
/// stapeln hieße, dass die Datei, die die Crate **beschreibt**, die Crate
/// **misst** — das war R66s Fehler (drei Klammerfehler in derselben Datei).
mod aufrufer_waechter;
