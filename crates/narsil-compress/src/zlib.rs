//! zlib-Rahmen (RFC 1950).

// W5F, 30.09.2026: die unbenutzten Importe am Dateikopf entfernt — der
// `FehlerZlib` und `STANDARD_GRENZE` wurden entfernt: der Code
// schreibt `crate::FehlerZlib` ausgeschrieben. `adler32` und
// `komprimiere` wurde auch entfernt: der Code ruft es ueber
// `crate::deflate::komprimiere(...)` auf (zlib.rs:29), nicht ueber den
// Import — ein Import, den niemand liest, ist ein Kommentar, der etwas
// behauptet, was nicht stattfindet.
use crate::pruefsummen::adler32;

/// Baut den zlib-Rahmen um DEFLATE-Daten.
pub fn rahme(deflate_bytes: &[u8], rohdaten: &[u8]) -> Vec<u8> {
    const CMF: u8 = 0x78;
    const FLG: u8 = 0x01;
    debug_assert_eq!(((CMF as u32) << 8 | FLG as u32) % 31, 0);
    let mut out = Vec::with_capacity(deflate_bytes.len() + 6);
    out.push(CMF);
    out.push(FLG);
    out.extend_from_slice(deflate_bytes);
    out.extend_from_slice(&adler32(rohdaten).to_be_bytes());
    out
}

/// Entpackt einen zlib-Strom in einen Puffer (mit Grenze).
pub fn entpacke_in(
    daten: &[u8],
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), crate::FehlerZlib> {
    crate::inflate::entpacke_in(daten, out, grenze)
}

/// Komprimiert und rahmt ein (zlib).
///
/// VORUEBERGEHEND: stillgelegt, added 30.09.2026 bei der Uebernahme von
/// `narsil-compress`. Die Funktion ist richtig (sie baut den
/// DEFLATE-Teil mit `deflate::komprimiere` und rahmt ihn mit
/// `rahme`), aber ungenutzt: `narsil-compress` wird bisher nur zum
/// **Entpacken** gebraucht.
///
/// `rahme` selbst wird von `narsil-image` beim PNG-Schreiben gebraucht
/// und bleibt deshalb oeffentlich — deshalb ist es hier auch nicht
/// stillgelegt.
#[allow(dead_code)]
pub fn komprimiere_und_rahme(rohdaten: &[u8]) -> Vec<u8> {
    let deflate = crate::deflate::komprimiere(rohdaten);
    rahme(&deflate, rohdaten)
}
