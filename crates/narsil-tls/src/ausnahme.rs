//! Release 10.10.2026: SITZUNGSAUSNAHMEN fuer Zertifikatsfehler — der Schritt
//! „Risiko akzeptieren und fortfahren" der Warnseite.
//!
//! # Die Regel (Entscheidung 10.10.2026)
//!
//! * Eine Ausnahme gilt fuer **einen Host und genau das Zertifikat**, das bei
//!   der Ablehnung vorlag (SHA-256 des Blatt-DER) — wie Firefox' Ausnahme. Ein
//!   ANDERES Zertifikat fuer denselben Host (ein zweiter Angreifer, ein
//!   Zertifikatswechsel) ist wieder gesperrt und zeigt wieder die Warnseite.
//! * Sie lebt **nur im Speicher** dieses Prozesses (die Sitzung) und wird nie
//!   auf die Platte geschrieben.
//! * Gesetzt wird sie nur vom Browser, nachdem der Benutzer auf der Warnseite
//!   geklickt hat (Einmal-Schluessel in `narsil::sicherheit_haken`); diese
//!   Crate merkt sich dafuer das zuletzt ABGELEHNTE Blatt je Host.
//!
//! Die Widerrufspruefung (OCSP) laeuft weiter: ein widerrufenes Zertifikat
//! bleibt auch mit Ausnahme gesperrt.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Hoechstens so viele Hosts je Liste (M4) — danach wird die Liste geleert.
const MAX_HOSTS: usize = 64;

#[derive(Default)]
struct Speicher {
    /// Host (klein) -> Fingerabdruck des erlaubten Blatts.
    erlaubt: HashMap<String, [u8; 32]>,
    /// Host (klein) -> Fingerabdruck des zuletzt abgelehnten Blatts.
    abgelehnt: HashMap<String, [u8; 32]>,
}

fn speicher() -> &'static Mutex<Speicher> {
    static S: OnceLock<Mutex<Speicher>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Speicher::default()))
}

fn mit<T>(f: impl FnOnce(&mut Speicher) -> T) -> T {
    let mut g = speicher().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g)
}

/// Der Fingerabdruck eines Zertifikats (SHA-256 ueber das DER).
pub fn fingerabdruck(der: &[u8]) -> [u8; 32] {
    crate::sha2::sha256(der)
}

/// Die Kettenpruefung fuer `host` ist mit dem Blatt `fp` gescheitert.
pub fn abgelehnt_merken(host: &str, fp: [u8; 32]) {
    mit(|s| {
        if s.abgelehnt.len() >= MAX_HOSTS {
            s.abgelehnt.clear();
        }
        s.abgelehnt.insert(host.to_ascii_lowercase(), fp);
    });
}

/// Das zuletzt abgelehnte Blatt fuer `host` (fuer die Freigabe).
pub fn zuletzt_abgelehnt(host: &str) -> Option<[u8; 32]> {
    mit(|s| s.abgelehnt.get(&host.to_ascii_lowercase()).copied())
}

/// Die Ausnahme setzen: `host` darf mit genau dem Blatt `fp` verbinden.
pub fn erlauben(host: &str, fp: [u8; 32]) {
    if host.is_empty() {
        return;
    }
    mit(|s| {
        if s.erlaubt.len() >= MAX_HOSTS {
            s.erlaubt.clear();
        }
        s.erlaubt.insert(host.to_ascii_lowercase(), fp);
    });
}

/// Gilt fuer `host` eine Ausnahme mit genau diesem Blatt?
pub fn erlaubt(host: &str, fp: &[u8; 32]) -> bool {
    mit(|s| s.erlaubt.get(&host.to_ascii_lowercase()) == Some(fp))
}

/// Alle Ausnahmen und Merker weg (Sitzungsende, Tests).
pub fn alle_weg() {
    mit(|s| {
        s.erlaubt.clear();
        s.abgelehnt.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eine_ausnahme_gilt_nur_fuer_genau_das_zertifikat() {
        let a = fingerabdruck(b"blatt-a");
        let b = fingerabdruck(b"blatt-b");
        abgelehnt_merken("Ausnahme.Test", a);
        assert_eq!(zuletzt_abgelehnt("ausnahme.test"), Some(a));
        assert!(!erlaubt("ausnahme.test", &a), "ohne Freigabe gesperrt");
        erlauben("ausnahme.test", a);
        assert!(erlaubt("AUSNAHME.test", &a));
        assert!(!erlaubt("ausnahme.test", &b), "ein anderes Blatt bleibt gesperrt");
        assert!(!erlaubt("andere.test", &a), "ein anderer Host bleibt gesperrt");
    }
}
