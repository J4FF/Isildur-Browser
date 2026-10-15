//! ICO SCHREIBEN.
//!
//! Eine ICO-Datei ist kein einzelnes Bild, sondern ein VERZEICHNIS von
//! mehreren. Das ist der Punkt, an dem die meisten Bildkonverter falsch
//! liegen: sie schreiben ein 256x256-Bild in eine ICO, und die Datei sieht
//! in der Explorer-Vorschau falsch aus, weil Windows das groesste Format
//! nimmt und kleiner anzeigt.
//!
//! Deshalb schreibt diese Crate die **Standardgroessen**:
//! 16, 24, 32, 48, 64, 128, 256. Das sind genau die, die Windows seit dem
//! Vista im Explorer anzeigt — und die alle Leser kennen.
//!
//! # Warum PNG-Eintraege und kein DIB
//!
//! Eine ICO-Eintrag kann Pixel entweder als **DIB** (ein BMP ohne Dateikopf
//! mit zusatzlicher AND-Maske) oder als **PNG** enthalten. Beides ist
//! gueltig. PNG hat zwei entscheidende Vorteile:
//!
//! * **Deckkraft**: DIB kann sie nur ueber die AND-Maske, und die ist
//!   1 Bit — keine Halbtransparenz. PNG hat einen echten Alphakanal.
//! * **Groesse**: DIB ist unkomprimiert, PNG nicht. Eine 256er-Icon als DIB
//!   ist 256 KiB, als PNG oft 4 KiB.
//!
//! Und ein Nachteil, den man kennen muss: DIB ist der einzige Weg fuer
//! Geraete, die vor Vista gebaut wurden. Für „Konvertiert herunterladen"
//! ist das keine Rolle.

use crate::Bild;
use crate::fehler::KonvertFehler;
use crate::grenzen;
use crate::png;

/// Die Groessen, die Windows im Explorer anzeigt (16, 24, 32, 48, 64, 128,
/// 256 Pixel). In dieser Reihenfolge stehen sie im Verzeichnis — die
/// Reihenfolge ist nicht bedeutungsschwer, aber sie liest sich gut.
pub const STANDARD_GROESSEN: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Schreibt eine ICO mit den Standardgroessen.
///
/// Der Nutzer will ein Symbol fuer seine Anwendung oder eine Datei. Welche
/// Groessen sinnvoll sind, entscheidet Windows — nicht der Konverter.
///
/// # Was passiert, wenn das Bild KLEINER ist als die groesste verlangte Groesse
///
/// Es wird hochskaliert (naechster Nachbar, keine Glattung — ein Icon soll
/// kraeftige Kanten haben, keine weichen).
///
/// ACHTUNG: hier stand frueher, es komme `IcoKanteFalsch`, „wenn das Bild
/// kleiner ist als die kleinste Standardgroesse (16)". Das war eine
/// Zusage, die der Code nicht einhaelt — er hat nie so geprueft, und ein
/// 1x1-Bild kam als 256x256-Icon heraus (GEMESSEN 02.10.2026 in
/// `bomben.rs::ein_pixel_breit_und_ein_pixel_hoch`). Ein Versprechen in
/// dieser Form ist schlimmer als keines: wer sich darauf verlaesst, wartet
/// auf einen Fehler, der nicht kommt.
///
/// Warum hochskaliert und nicht abgelehnt wird: die sieben
/// Standardgroessen sind das, was Windows anfragt; eine Datei mit drei
/// davon ist brauchbar, eine ohne Eintraege gar nicht. Und der haeufigste
/// Weg zu einem Icon — ein 32x32-Logo aus einer Webseite — waere mit
/// einem Fehler vollstaendig blockiert. Wer die Verkleinerung nicht will,
/// ruft [`schreiben_groessen`] mit Groessen auf, die zum Bild passen.
///
/// # Fehler
/// - Siehe [`crate::grenzen::pruefe_bild`] (Masse und Speicher).
pub fn schreiben(bild: &Bild) -> Result<Vec<u8>, KonvertFehler> {
    schreiben_groessen(bild, &STANDARD_GROESSEN)
}

/// Release 08.10.2026: das Bild mittig auf eine durchsichtige quadratische
/// Flaeche (Kante = laengere Seite). Ein quadratisches Bild bleibt, wie es
/// ist (eine Kopie, kein Umrechnen).
fn auf_quadrat(bild: &Bild) -> Bild {
    let (b, h) = (bild.breite, bild.hoehe);
    if b == h || b == 0 || h == 0 {
        return bild.clone();
    }
    let kante = b.max(h);
    // `Bild::new` ist durchsichtig gefuellt.
    let mut raus = Bild::new(kante, kante);
    let (dx, dy) = ((kante - b) / 2, (kante - h) / 2);
    for y in 0..h {
        let von = (y * b * 4) as usize;
        let nach = (((y + dy) * kante + dx) * 4) as usize;
        let laenge = (b * 4) as usize;
        if let (Some(q), Some(z)) = (bild.rgba.get(von..von + laenge), raus.rgba.get_mut(nach..nach + laenge)) {
            z.copy_from_slice(q);
        }
    }
    raus
}

/// Schreibt eine ICO mit selbst gewaehlten Kanten.
///
/// `groessen` werden aufsteigend sortiert und dedupliziert; jede muss
/// zwischen 1 und [`grenzen::MAX_ICO_KANTE`] liegen (das Verzeichnis hat
/// genau ein Byte fuer die Kante, und 0 bedeutet dort 256).
///
/// # Fehler
/// - [`KonvertFehler::ZuVieleEintraege`] wenn mehr als
///   [`grenzen::MAX_ICO_EINTRAEGE`] Groessen kommen.
/// - [`KonvertFehler::IcoKanteFalsch`] bei 0 oder ueber 256.
pub fn schreiben_groessen(bild: &Bild, groessen: &[u32]) -> Result<Vec<u8>, KonvertFehler> {
    grenzen::pruefe_bild(bild)?;
    // Die Zahl der Eintraegge ist die erste Sache, die man prueft: ein
    // Verzeichnis mit 100 Eintraegen passt nicht ins 16-Bit-Feld und
    // wuerde den Leser zum Abschneiden zwingen. Eigener benannter Fehler
    // statt der allgemeinen Laengenpruefung, weil die Zahl hier eine
    // BEDEUTUNG hat (das ICO-Verzeichnis) und nicht nur eine Laenge.
    if groessen.len() > grenzen::MAX_ICO_EINTRAEGE {
        return Err(KonvertFehler::ZuVieleEintraege {
            anzahl: groessen.len(),
            grenze: grenzen::MAX_ICO_EINTRAEGE,
        });
    }
    if groessen.is_empty() {
        return Err(KonvertFehler::ZuVieleEintraege {
            anzahl: 0,
            grenze: grenzen::MAX_ICO_EINTRAEGE,
        });
    }
    let mut kanten: Vec<u32> = groessen.to_vec();
    kanten.sort_unstable();
    kanten.dedup();
    for &k in &kanten {
        if k == 0 || k > grenzen::MAX_ICO_KANTE {
            return Err(KonvertFehler::IcoKanteFalsch { kante: k });
        }
    }
    if kanten.len() > grenzen::MAX_ICO_EINTRAEGE {
        return Err(KonvertFehler::ZuVieleEintraege {
            anzahl: kanten.len(),
            grenze: grenzen::MAX_ICO_EINTRAEGE,
        });
    }

    // --- Bildpunkte einmal skalieren, nicht je Eintrag neu lesen ---
    // Die Skalierung ist der teuerste Teil (Kastenfilter). Jeder Eintrag
    // wird aus demselben Quellbild berechnet, damit alle Groessen
    // dieselbe Farbqualitaet haben.
    //
    // Release 08.10.2026: ein nicht quadratisches Bild wird zuerst mittig
    // auf eine durchsichtige quadratische Flaeche gesetzt. GEMESSEN vorher
    // (`narsil-chrome/tests/konvert_matrix.rs`): ein 40x30-JPEG kam als
    // GESTRECKTES 256x256-Symbol heraus — das Seitenverhaeltnis war weg.
    let quadrat = auf_quadrat(bild);
    let bild = &quadrat;
    let mut nutzlast: Vec<(u32, Vec<u8>)> = Vec::with_capacity(kanten.len());
    for &k in &kanten {
        // Quadratisch: eine Icon ist quadratisch (das Bild ist es jetzt auch,
        // also wird nichts verzerrt).
        let klein =
            narsil_image::skalieren::skalieren(bild, k, k, narsil_image::skalieren::Filter::Box);
        // Jeder Eintrag ist ein vollstaendiges, verlustfreies PNG.
        let bytes = png::schreiben(&klein, png::PngModus::Auto)?;
        // M4: Die Nutzlast ist gegen die Speichergrenze geprueft, und das
        // schon beim Wachsen — nicht erst am Ende.
        let bisher: usize = nutzlast.iter().map(|(_, v)| v.len()).sum();
        grenzen::pruefe_laenge(bisher + bytes.len(), grenzen::MAX_SPEICHER, "ICO-Nutzlast")?;
        nutzlast.push((k, bytes));
    }

    // --- Groesse vorab: 6 + 16*n + Summe der Nutzlasten ---
    let kopf = 6 + 16 * nutzlast.len();
    let summe: usize = nutzlast.iter().map(|(_, v)| v.len()).sum();
    let gesamt = kopf
        .checked_add(summe)
        .ok_or(KonvertFehler::SpeicherZuGross {
            bytes: u64::MAX,
            grenze: grenzen::MAX_SPEICHER,
        })?;
    grenzen::pruefe_laenge(gesamt, grenzen::MAX_SPEICHER, "ICO-Datei")?;

    let mut out: Vec<u8> = Vec::with_capacity(gesamt);
    // --- ICONDIR: reserviert 0, Typ 1 (Icon), Anzahl ---
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(nutzlast.len() as u16).to_le_bytes());

    // --- ICONDIRENTRY je Bild ---
    // Die Breite und Hoehe stehen in je EINEM Byte, und 0 bedeutet dort
    // 256. Das ist der Grund, warum 256 hier nicht als 256 geschrieben
    // werden kann.
    let mut versatz = kopf;
    for (k, bytes) in &nutzlast {
        let kbyte = if *k == 256 { 0u8 } else { *k as u8 };
        out.push(kbyte);
        out.push(kbyte);
        // Farbanzahl 0 heisst laut Spezifikation „256 oder mehr". Die
        // PNG-Eintraege haben keine Farbanzahl; 0 ist hier die ehrliche
        // Antwort.
        out.push(0);
        out.push(0); // reserviert
        out.extend_from_slice(&1u16.to_le_bytes()); // Farbebenen
        out.extend_from_slice(&32u16.to_le_bytes()); // Bit pro Pixel (RGBA)
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(versatz as u32).to_le_bytes());
        versatz += bytes.len();
    }
    // --- Nutzlasten ---
    for (_, bytes) in &nutzlast {
        out.extend_from_slice(bytes);
    }
    Ok(out)
}
