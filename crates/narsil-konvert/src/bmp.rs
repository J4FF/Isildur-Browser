//! BMP SCHREIBEN (BI_RGB, 24 oder 32 Bit, unkomprimiert).
//!
//! Warum BMP ueberhaupt: es ist das Format, das **jedes** Programm auf der
//! Welt lesen kann — von GIMP ueber Word bis zum Explorer. Fuer einen
//! Nutzer, der ein Bild „irgendwohin" speichern will, ist es die
//! ehrlichste Wahl.
//!
//! Zwei Fallen, an denen BMP-Schreiber scheitern, und beide sind hier
//! ausdruecklich behandelt:
//!
//! 1. **Die Zeilen muessen auf 4 Byte aufgefuellt werden.** `((breite*3)+3)
//!    & !3`. Ohne die Polsterung ist die Datei bei den meisten Lesern
//!    um eine Zeile daneben — ein Fehler, der sich als „Bild schraeg"
//!    zeigt und nicht als „Datei kaputt".
//! 2. **BMP speichert von unten nach oben** (positive Hoehe) oder von oben
//!    nach unten (negative Hoehe). Der Leser in `narsil-image/src/bmp.rs`
//!    versteht beides; wir schreiben von unten nach oben, weil das der
//!    verbreitetere Fall ist.
//!
//! # Was BMP verliert
//!
//! **Alpha.** Das vierte Byte eines 32-Bit-BMP ist laut Spezifikation
//! „unbenutzt"; GDI laesst dort eine Null stehen und liest es NICHT als
//! Deckkraft. Der Leser des Projekts setzt deshalb Alpha auf 255
//! (`narsil-image/src/bmp.rs:143`). Wir SCHREIBEN das echte Alpha hinein —
//! dann koennen es die Leser benutzen, die es verstehen — aber die
//! Rundreise ueber `narsil-image` behaelt es nicht. Deshalb steht in
//! [`crate::Zielformat::verlustfrei`], dass die Farbkanaele exakt bleiben.

use crate::Bild;
use crate::fehler::KonvertFehler;
use crate::grenzen;

/// BITMAPFILEHEADER: 14 Byte (das `BM` plus Dateigroesse und Versatz).
const KOPF_GROESSE: usize = 14;
/// BITMAPINFOHEADER: 40 Byte, die kleinste verbreitete Variante.
const INFO_GROESSE: usize = 40;

/// Der benannte Fehler, wenn eine Rechnung in `usize` ueberlaeuft.
///
/// `bytes: u64::MAX` heisst „unbekannt, aber sicher zu gross" — der Wert
/// wird nur in die Meldung geschrieben, und `pruefe_laenge` haelt danach an.
fn ueberlauf() -> KonvertFehler {
    KonvertFehler::SpeicherZuGross {
        bytes: u64::MAX,
        grenze: grenzen::MAX_SPEICHER,
    }
}

/// In welchem Umfang soll geschrieben werden?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BmpArt {
    /// 24 Bit, keine Deckkraft. Das kompatibelste BMP ueberhaupt —
    /// auch von Programmen gelesen, die 32-Bit-BM veraechtigen.
    Rgb,
    /// 32 Bit mit Deckkraft im vierten Byte. Achtung: nur Leser, die das
    /// vierte Byte als Alpha verstehen, zeigen die Transparenz.
    #[default]
    Rgba,
    /// Der Enkoder waehlt: 24 Bit, wenn das Bild undurchsichtig ist,
    /// sonst 32 Bit.
    Auto,
}

/// Schreibt ein BMP.
///
/// # Fehler
/// Siehe [`crate::grenzen::pruefe_bild`]; dazu
/// [`KonvertFehler::UnbekannteOption`] fuer eine unbekannte [`BmpArt`].
pub fn schreiben(bild: &Bild, art: BmpArt) -> Result<Vec<u8>, KonvertFehler> {
    grenzen::pruefe_bild(bild)?;
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;

    let (bpp, vier_bytes) = match art {
        BmpArt::Rgb => (3usize, false),
        BmpArt::Rgba => (4, true),
        BmpArt::Auto => {
            let undurchsichtig = (0..breite * hoehe).all(|p| bild.rgba[p * 4 + 3] == 255);
            if undurchsichtig {
                (3, false)
            } else {
                (4, true)
            }
        }
    };

    // M4: Die Dateigroesse wird VOR der Allokation gerechnet und geprueft.
    // `checked_mul`/`checked_add` statt `*`, weil `usize::MAX * 1` stillschweigend
    // 0 ergaebe und der Enkader dann eine Datei ohne Pixel schriebe.
    let ungepolstert = breite * bpp;
    let zeilenbreite = (ungepolstert + 3) & !3usize;
    let pixel_start = KOPF_GROESSE + INFO_GROESSE;
    let pixelbytes = zeilenbreite.checked_mul(hoehe).ok_or(ueberlauf())?;
    let groesse = pixel_start.checked_add(pixelbytes).ok_or(ueberlauf())?;
    grenzen::pruefe_laenge(groesse, grenzen::MAX_SPEICHER, "BMP-Datei")?;

    let mut out: Vec<u8> = Vec::with_capacity(groesse);

    // --- BITMAPFILEHEADER ---
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(groesse as u32).to_le_bytes());
    // Reserviert 1 und 2: muessen 0 sein.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(pixel_start as u32).to_le_bytes());

    // --- BITMAPINFOHEADER ---
    out.extend_from_slice(&(INFO_GROESSE as u32).to_le_bytes());
    out.extend_from_slice(&(bild.breite as i32).to_le_bytes());
    out.extend_from_slice(&(bild.hoehe as i32).to_le_bytes()); // positiv = von unten
    out.extend_from_slice(&1u16.to_le_bytes()); // Ebenen
    // ACHTUNG: hier stehen BITS, nicht Bytes. `bpp` ist Byte je Pixel
    // (3 oder 4), das Feld erwartet aber 24 oder 32. Ohne die Multiplikation
    // meldet die Datei „3 Bit pro Pixel", und der Leser bricht mit
    // „nur BI_RGB mit 24 oder 32 Bit" ab.
    //
    // GEMESSEN 02.10.2026: `bmp_24bit_rundlauf` las an Offset 28 eine 3
    // statt einer 24. Gefunden vom Test, nicht vom Anschauen.
    out.extend_from_slice(&((bpp * 8) as u16).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB = keine Kompression
    out.extend_from_slice(&(pixelbytes as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // X-Aufloesung
    out.extend_from_slice(&0u32.to_le_bytes()); // Y-Aufloesung
    out.extend_from_slice(&0u32.to_le_bytes()); // Palettenfarben
    out.extend_from_slice(&0u32.to_le_bytes()); // wichtige Farben

    // --- Pixelzeilen, von der LETZTEN zur ersten (von unten nach oben) ---
    for y in (0..hoehe).rev() {
        let grund = y * breite * 4;
        let zeilen_start = out.len();
        for x in 0..breite {
            let i = grund + x * 4;
            // BMP reihenfolge ist B, G, R, A — nicht RGB.
            out.push(bild.rgba[i + 2]);
            out.push(bild.rgba[i + 1]);
            out.push(bild.rgba[i]);
            if vier_bytes {
                out.push(bild.rgba[i + 3]);
            }
        }
        // Auf 4 Byte auffuellen. Die Polsterbytes sind undefinierbar, aber
        // 0 ist der freundlichste Wert: manche Leser zeigen sie als Rand.
        while (out.len() - zeilen_start) < zeilenbreite {
            out.push(0);
        }
    }
    Ok(out)
}
