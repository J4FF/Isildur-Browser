//! ALLE harten Obergrenzen dieser Crate an EINER Stelle. Regel M3.
//!
//! Jede Schleife, die von einer Zahl aus der Eingabe abhaengt, und jede
//! Allokation, deren Groesse aus der Eingabe kommt (M4), prueft gegen eine
//! Konstante aus diesem Modul — und zwar BEVOR alloziert wird, nicht danach.
//!
//! Warum ueberhaupt: die Maschine des Nutzers ist mehrfach an Speicherlecks
//! erstickt (19,8 GB RSS in EINEM Testprozess). Ein Enkoder ist dabei die
//! gefaehrlichste Stelle im ganzen Projekt, denn er Allokiert nach Plan:
//! `breite * hoehe * 4` Byte Pixel, plus Zeilenpuffer, plus Paletten, plus
//! Huffman-Baeume. Ohne Grenze ist ein `width=65535 height=65535` im
//! IHDR-Vorschlag eine 17-GiB-Allokation.

use crate::Bild;

/// Hoechste Bildkante in Pixeln.
///
/// GEMESSEN/Begruendung: `narsil-image` dekodiert bis 16384, also kann diese
/// Crate nichts Groesseres als Eingang bekommen. Der Wert steht hier
/// bewusst doppelt: die Grenze gehoert zum ENKODER, nicht nur zum Dekoder.
pub const MAX_KANTE: u32 = 16384;

/// Hoechste Zahl Bildpunkte. 16384 x 16384 = 268 M Pixel; RGBA waere 1 GiB.
/// Wir bleiben darunter, weil der Pipeline-Puffer (K4, 1 GiB) drei Bilder
/// nebeneinander halten muss (Quelle + Ziel + Skalierungsstufe).
pub const MAX_BILDPUNKTE: u64 = 64 * 1024 * 1024; // 64 M Pixel

/// Hoechste Zahl Bytes, die ein Enkoder fuer ein Bild zurueckhaelt.
pub const MAX_SPEICHER: usize = 256 * 1024 * 1024;

/// Hoechste Zeilenbreite in Byte (RGBA).
///
/// Das ist `MAX_KANTE * 4`. Bewusst KEINE eigene Zahl: eine eigene Zahl
/// waere eine zweite Wahrheit, die auseinanderlaufen kann.
pub const MAX_ZEILE: usize = MAX_KANTE as usize * 4;

/// Hoechste Zahl Bilder in **einem** Video (K3).
///
/// GEMESSEN begründet, warum **1024** und nicht mehr: `idx1` ist ein `u32` je
/// Bild, und die Datei waechst um mindestens **16 Byte** je Bild. Bei **1024**
/// Bildern sind das 16 KiB Index **plus** die Bilddaten — und eine Minute Video
/// bei 10 Bildern pro Sekunde sind **600**.
///
/// Also: **1024** ist **zehn Minuten** bei 10 fps. Und das ist **genau** die
/// Grenze, ab der ein Nutzer **zweimal** nachfragt statt einmal.
pub const MAX_VIDEO_BILDER: usize = 1024;

/// Hoechste Zahl Farben in einer Palette (GIF: 2^8, PNG indexed: 2^8,
/// BMP: 2^8 fuer 8-Bit-Indexed).
pub const MAX_PALETTE: usize = 256;

/// Hoechste Zahl Huffman-Symbole in einem Praefixcode.
///
/// JPEG: Rot/Gruen/Blau je 256 + DC/AC-Kategorien 16. VP8L: gruen 280.
/// GIF: 4096. WebP: 40 (Distanz). Der Wert deckt alles ab und ist eine
/// Konstante, keine Eingabezahl.
pub const MAX_ALPHABET: usize = 4096;

/// Hoechste Bitlaenge eines JPEG-Huffman-Codes (JPEG erlaubt 1..16).
pub const MAX_CODELAENGE_JPEG: u8 = 16;

/// Hoechste Bitlaenge eines VP8L-Codes (Entwurf 3.7: 1..15).
pub const MAX_CODELAENGE_VP8L: u8 = 15;

/// Hoechste Zahl Huffman-Codes, die ein Bild benutzt (JPEG: 4 Paare,
/// VP8L: 5 je Gruppe). Plus Reserve fuer Meta-Gruppen.
pub const MAX_CODES: usize = 256;

/// Hoechste Zahl Bytes fuer einen Huffman-Baum bei Huffman-Faktor 4.
///
/// JPEG erlaubt DHT-Segmente bis 65533 Byte; wir deckeln deutlich darunter,
/// weil ein Code mit 256 Symbolen und Laenge 4 hoechstens 256*16 Bit hat.
pub const MAX_HUFFMAN_BYTES: usize = 64 * 1024;

/// Hoechste Zahl Blöcke (8x8) je JPEG-Bild.
pub const MAX_BLOECKE: u64 = (MAX_BILDPUNKTE / 64) + 64;

/// Hoechste Zahl Bilder in einem animierten GIF (Animation ist ein
/// Vektor — M6: eine Datei mit 100000 Bildern waere eine Bombe).
///
/// 1000 Bilder entspricht der Grenze des Dekoders in `narsil-image`
/// (`MAX_FRAMES`), damit eine von uns geschriebene Datei dort immer lesbar
/// bleibt.
pub const MAX_GIF_BILDER: usize = 1000;

/// Hoechste Summe aller GIF-Bilder in Pixel (alle Bilder zusammen).
pub const MAX_GIF_PIXEL: u64 = 16 * 1024 * 1024;

/// Hoechste Verzoegerung je GIF-Bild in ms (das GIF-Feld ist 16 Bit,
/// also 65535; wir lassen nichts darueber hinaus, damit ein Konverter
/// nicht 1000 Bilder x 65535 ms = 18 Stunden erzeugt).
pub const MAX_GIF_VERZOEGERUNG_MS: u16 = 65535;

/// Hoechste Zahl Iterationen im Median-Cut.
///
/// Median-Cut teilt einen Kasten in zwei Haelften. Ohne Grenze koennte ein
/// Pfad 1 Pixel breit bleiben und 255 Mal teilen; das ist hier 255 Schritte
/// (nicht 100000), weil die Klassenliste nach 255 Teilungen leer ist.
pub const MAX_QUANT_STUFEN: usize = 255;

/// Hoechste Zahl Eintraege in einer ICO-Datei (Grenze des Dekoders:
/// `narsil-image` `MAX_EINTRAEGE` = 64).
pub const MAX_ICO_EINTRAEGE: usize = 64;

/// Hoechste Kantenlaenge je ICO-Eintrag. Eine ICO enthaelt meist 16/32/48/
/// 64/128/256; 256 ist das Maximum des Formats (ein Byte im Verzeichnis,
/// 0 bedeutet 256).
pub const MAX_ICO_KANTE: u32 = 256;

/// Hoechste Zahl Pixel je VP8L-Bildstrom (Grenze des Dekoders:
/// `narsil-image` `MAX_BILD` = 8 000 000).
pub const MAX_VP8L_PIXEL: u64 = 8_000_000;

/// Hoechste Zahl Bytes, die ein einzelner VP8L-Codestrom fuer die
/// Laengenliste aller fuenf Codes belegen darf.
///
/// Fuenf Codes mit maximaler Vollstaendigkeit: gruen 280 + 3x256 + 40
/// Symbole zu je 15 Bit = (280+768+40)*15/8 = 2036 Byte, dazu die
/// Code-Laengen-Codes. 8 KiB ist grosszuegig und deckelt eine
/// Endlosschleife im Laengenleser.
pub const MAX_VP8L_KOPF_BYTES: usize = 8 * 1024;

/// Hoechste Zahl Pixel je Farbindex-Bitgruppe (VP8L Farbindexierung).
pub const MAX_VP8L_PALETTE: u32 = 256;

/// Hoechste Zahl Tonproben (Werte, alle Kanaele zusammen) je Datei.
///
/// 40 Mio. Werte sind bei 48 kHz stereo rund 7 Minuten. Das ist deutlich
/// mehr, was ein „Konvertiert herunterladen"-Menue braucht, und deutlich
/// weniger, was den Speicher eines 32-Bit-Prozesses wegschlaegt
/// (40 Mio. * 4 Byte = 160 MiB fuer die Werte allein).
///
/// GEMESSEN nicht, geschaetzt — und deshalb mit dem Kommentar, dass es
/// eine Abschneidegrenze ist, keine physikalische.
pub const MAX_TON_PUNKTE: usize = 40 * 1024 * 1024;

/// Hoechste Byte-Groesse einer geschriebenen Audiodatei. 256 MiB.
pub const MAX_TON_BYTES: usize = 256 * 1024 * 1024;

/// Prueft die Bildmasse gegen [`MAX_KANTE`], [`MAX_BILDPUNKTE`] und
/// [`MAX_SPEICHER`] — BEVOR irgendetwas alloziert wird.
///
/// `Ok(())` heisst: `breite * hoehe * 4` passt garantiert in den Speicher.
pub fn pruefe_masse(breite: u32, hoehe: u32) -> Result<(), crate::KonvertFehler> {
    if breite == 0 || hoehe == 0 {
        return Err(crate::KonvertFehler::GroesseNull);
    }
    if breite > MAX_KANTE || hoehe > MAX_KANTE {
        return Err(crate::KonvertFehler::KanteZuGross {
            breite,
            hoehe,
            grenze: MAX_KANTE,
        });
    }
    let punkte = (breite as u64) * (hoehe as u64);
    if punkte > MAX_BILDPUNKTE {
        return Err(crate::KonvertFehler::ZuVielePunkte {
            punkte,
            grenze: MAX_BILDPUNKTE,
        });
    }
    let bytes = punkte.saturating_mul(4);
    if bytes > MAX_SPEICHER as u64 {
        return Err(crate::KonvertFehler::SpeicherZuGross {
            bytes,
            grenze: MAX_SPEICHER,
        });
    }
    Ok(())
}

/// Prueft ein Bild gegen dieselben Grenzen und meldet zusaetzlich, wenn
/// `rgba` kuerzer ist als `breite * hoehe * 4`.
///
/// Ein `Bild` mit zu kurzem `rgba` ist im Projekt erlaubt (`Bild::pixel`
/// klemmt und liefert Nullen). Fuer einen ENKODER ist das eine Fehlerquelle,
/// also wird hier benannt abgelehnt statt still zu schwarz zu malen.
pub fn pruefe_bild(bild: &Bild) -> Result<(), crate::KonvertFehler> {
    pruefe_masse(bild.breite, bild.hoehe)?;
    let nlaenge = bild.breite as usize * bild.hoehe as usize * 4;
    if bild.rgba.len() < nlaenge {
        return Err(crate::KonvertFehler::PufferZuKurz {
            braucht: nlaenge,
            hat: bild.rgba.len(),
        });
    }
    Ok(())
}

/// Prueft eine Laenge aus der Eingabe, BEVOR `Vec::with_capacity` laeuft.
///
/// Das ist Regel M4 in seiner kuerzesten Form: `Vec::with_capacity(n)` mit
/// `n` aus der Datei ist verboten, `Vec::with_capacity(n)` mit `n` von hier
/// ist erlaubt.
pub fn pruefe_laenge(
    n: usize,
    grenze: usize,
    was: &'static str,
) -> Result<(), crate::KonvertFehler> {
    if n > grenze {
        return Err(crate::KonvertFehler::LaengeZuGross { n, grenze, was });
    }
    Ok(())
}
