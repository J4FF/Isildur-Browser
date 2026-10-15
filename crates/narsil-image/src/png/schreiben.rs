//! PNG SCHREIBEN.
//!
//! Reihenfolge des Projekts (BACKLOG 0071): erst gueltig ohne Kompression,
//! dann mit. Beides ist hier gebaut, und beides erzeugt eine Datei, die
//! `png_lesen` wieder einliest — siehe `tests/rundlauf.rs`.
//!
//! Der Enkoder macht mehr als „Bytes hinschreiben", weil ein naiver Enkeder
//! PNG-Dateien produziert, die zwar gueltig, aber unbrauchbar sind:
//!   * Filterwahl je Zeile. Ohne Filter ist ein Foto unkomprimierbar; mit
//!     der kleinsten Summe der Absolutdifferenzen ist es rund 8x kleiner.
//!     Der Leser muss dafuer natuerlich Filter 0-4 koennen.
//!   * Die Farbwahl. Graustufen statt RGBA ist ein Drittel der Bytes.

use crate::deflate;
use crate::png::{FARBTYP_GRAU, FARBTYP_RGB, FARBTYP_RGBA, SIGNATUR};
use crate::pruefsummen::crc32_zwei;
use crate::zlib;
use crate::{Bild, Farbmodus};

/// PNG-Farbtyp fuer einen Modus.
fn farbtyp_von(modus: Farbmodus) -> u8 {
    match modus {
        Farbmodus::Graustufen => FARBTYP_GRAU,
        Farbmodus::Rgb => FARBTYP_RGB,
        Farbmodus::Rgba => FARBTYP_RGBA,
    }
}

/// Kanalanzahl eines Modus: Graustufen hat eins, RGB drei, RGBA vier.
fn kanaele_von(modus: Farbmodus) -> usize {
    match modus {
        Farbmodus::Graustufen => 1,
        Farbmodus::Rgb => 3,
        Farbmodus::Rgba => 4,
    }
}

/// Waehlt den kleinsten Modus, der das Bild verlustfrei darstellt.
///
/// Die Regel ist streng: RGBA nur, wenn wirklich ein Alpha-Kanal vorkommt
/// (`a != 255`). Sonst wuerde ein Foto mit einem einzigen leicht
/// durchsichtigen Randpixel viermal so gross wie noetig.
///
/// Graustufen nur, wenn r == g == b fuer JEDEN Pixel. Sonst wuerde
/// `png_lesen(png_schreiben(b)) == b` nicht gelten, und genau diese
/// Gleichheit ist das bindende Versprechen des Auftrags.
pub fn automatischer_modus(bild: &Bild) -> Farbmodus {
    let mut grau = true;
    let mut undurchsichtig = true;
    let px = bild.breite as usize * bild.hoehe as usize;
    for p in 0..px {
        let i = p * 4;
        let (r, g, b, a) = (
            bild.rgba.get(i).copied().unwrap_or(0),
            bild.rgba.get(i + 1).copied().unwrap_or(0),
            bild.rgba.get(i + 2).copied().unwrap_or(0),
            bild.rgba.get(i + 3).copied().unwrap_or(0),
        );
        if r != g || g != b {
            grau = false;
        }
        if a != 255 {
            undurchsichtig = false;
        }
        if !grau && !undurchsichtig {
            break;
        }
    }
    if !undurchsichtig {
        Farbmodus::Rgba
    } else if !grau {
        Farbmodus::Rgb
    } else {
        Farbmodus::Graustufen
    }
}

/// Schreibt ein PNG. Die ausgelieferte Fassung benutzt zlib-DEFLATE.
///
/// Der Auftrag sagt das ausdruecklich: „Ein gueltiges PNG ohne Kompression
/// ist 4x zu gross — und ESRICHT." Also wird hier komprimiert. Fuer die
/// Feld-fuer-Feld-Pruefung gibt es [`zlib_ohne_kompression`].
pub fn png_schreiben(bild: &Bild) -> Vec<u8> {
    png_schreiben_modus(bild, automatischer_modus(bild))
}

/// Schreibt ein PNG in einem bestimmten Farbmodus.
///
/// `modus == Farbmodus::Graustufen` auf einem Farbbild verliert Farbe
/// (nur der Rotkanal geht in die Datei). Das ist Absicht: manchmal
/// braucht man genau das, etwa um einen Screenshot als Maske zu schreiben,
/// und dafuer ist ein eigener Aufruf besser als eine stille Regel.
pub fn png_schreiben_modus(bild: &Bild, modus: Farbmodus) -> Vec<u8> {
    let roh = rohzeilen(bild, modus);
    komprimiert_schreiben(&roh, bild, modus, true)
}

/// Schreibt ein PNG mit gespeicherten (unkomprimierten) DEFLATE-Bloecken.
///
/// Das ist die Zwischenstufe aus Stufe 1 des Auftrags und zugleich das
/// kleinste moegliche gueltige zlib-Format: `78 01 <Blocktyp 00> ...`.
/// `png_lesen` muss das lesen koennen — und die Feld-fuer-Feld-Pruefung
/// in `tests/png_felder.rs` rechnet genau diese Bytes nach.
pub fn zlib_ohne_kompression(bild: &Bild, modus: Farbmodus) -> Vec<u8> {
    let roh = rohzeilen(bild, modus);
    komprimiert_schreiben(&roh, bild, modus, false)
}

fn komprimiert_schreiben(
    roh: &[u8],
    bild: &Bild,
    modus: Farbmodus,
    mit_kompression: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(roh.len() / 2 + 64);

    // --- Signatur ---
    out.extend_from_slice(&SIGNATUR);

    // --- IHDR, immer 13 Byte, immer zuerst ---
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&bild.breite.to_be_bytes());
    ihdr.extend_from_slice(&bild.hoehe.to_be_bytes());
    ihdr.push(8); // Bittiefe: diese Crate schreibt nur 8
    ihdr.push(farbtyp_von(modus));
    ihdr.push(0); // Kompressionsverfahren: 0 = DEFLATE
    ihdr.push(0); // Filtermethode: 0 = adaptiv (wir waehlen je Zeile)
    ihdr.push(0); // Interlace: 0 = keine
    chunk_anhaengen(&mut out, b"IHDR", &ihdr);

    // --- IDAT: der zlib-Strom ---
    let strom = if mit_kompression {
        zlib::rahme(&deflate::komprimiere(roh), roh)
    } else {
        zlib::rahme(&deflate::gespeichert(roh), roh)
    };
    chunk_anhaengen(&mut out, b"IDAT", &strom);

    // --- IEND: Laenge 0, keine Daten ---
    chunk_anhaengen(&mut out, b"IEND", &[]);

    out
}

/// Haengt einen Chunk an: Laenge, Typ, Daten, CRC.
///
/// Der CRC laeuft ueber TYP + DATEN. Wer ihn nur ueber die DATEN rechnet,
/// bekommt eine Datei, die jeder `pngcheck` ablehnt.
fn chunk_anhaengen(out: &mut Vec<u8>, typ: &[u8; 4], daten: &[u8]) {
    out.extend_from_slice(&(daten.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(daten);
    let crc = crc32_zwei(typ, daten);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Baut die Zeilen fuer den Enkoder: Filterbyte + Nutzbytes, gewaehlter
/// Filter je Zeile.
fn rohzeilen(bild: &Bild, modus: Farbmodus) -> Vec<u8> {
    let bpp = kanaele_von(modus);
    let b = bild.breite as usize;
    let h = bild.hoehe as usize;
    let zeilenbreite = b * bpp;
    if zeilenbreite == 0 || h == 0 {
        return Vec::new();
    }
    let mut roh = vec![0u8; h * (zeilenbreite + 1)];
    // Drei Puffer, alle genau einmal gebucht: die ROHE Zeile, die
    // GEFILTERTE Zeile und die ROHE Zeile DARUEBER. Der dritte wird
    // gebraucht, weil der Filter die rohen Nachbarn braucht (RFC 2083
    // Abschnitt 6.2 sagt `Raw(...)`) und weil der Filter nicht in-place
    // arbeiten darf.
    let mut zeile_roh = vec![0u8; zeilenbreite];
    let mut zeile_filtered = vec![0u8; zeilenbreite];
    let mut vorzeile_roh = vec![0u8; zeilenbreite];
    for y in 0..h {
        zeile_roh_bauen(bild, y as u32, modus, &mut zeile_roh);
        // `vorzeile_roh` ist die ROHE Zeile darueber. Filter 2, 3 und 4
        // brauchen sie in genau dieser Form.
        let vor: &[u8] = if y == 0 { &[] } else { &vorzeile_roh };
        let typ = bester_filter(&zeile_roh, vor, bpp);
        let start = y * (zeilenbreite + 1);
        roh[start] = typ;
        filter_anwenden(&zeile_roh, vor, bpp, typ, &mut zeile_filtered);
        roh[start + 1..start + 1 + zeilenbreite].copy_from_slice(&zeile_filtered);
        vorzeile_roh.copy_from_slice(&zeile_roh);
    }
    roh
}

/// Fuellt `ziel` mit den Nutzbytes der Zeile `y`, im gewaehlten Modus.
///
/// `ziel` wird mit der Laenge der Zeile (nicht des Bildes) geschrieben, und
/// ueber [`Bild::pixel`] geklemmt gelesen: ein `Bild` mit zu kurzem `rgba`
/// liefert Nullen und keine Panik.
fn zeile_roh_bauen(bild: &Bild, y: u32, modus: Farbmodus, ziel: &mut [u8]) {
    let breite = bild.breite as usize;
    let bpp = kanaele_von(modus);
    for x in 0..breite {
        let p = bild.pixel(x as u32, y);
        let i = x * bpp;
        match modus {
            Farbmodus::Graustufen => ziel[i] = p[0],
            Farbmodus::Rgb => {
                ziel[i] = p[0];
                ziel[i + 1] = p[1];
                ziel[i + 2] = p[2];
            }
            Farbmodus::Rgba => {
                ziel[i..i + 4].copy_from_slice(&p);
            }
        }
    }
}

/// Die fuenf Filtertypen aus RFC 2083 Abschnitt 6.
pub const FILTER_NONE: u8 = 0;
pub const FILTER_SUB: u8 = 1;
pub const FILTER_UP: u8 = 2;
pub const FILTER_AVERAGE: u8 = 3;
pub const FILTER_PAETH: u8 = 4;

/// Paeth-Vorhersager (RFC 2083 Abschnitt 6.6). Der ganze Filtertyp 4
/// besteht aus dieser Formel — sie ist der Grund, warum PNG bei
/// Bildrauschen so gut ist.
///
/// `a` links, `b` oben, `c` oben links; man waehlt das, was dem linearen
/// Verlauf am naechsten kommt.
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filtert `roh` nach `out`: aus Rohbytes werden gefilterte Bytes.
///
/// `vorzeile` ist die ROHE (ungefilterte) Zeile darueber — und das ist
/// der Punkt, an dem sich zwei Implementierungen um Tage unterscheiden.
///
/// RFC 2083 Abschnitt 6.2 definiert die Nachbarn ausdruecklich als
/// `a = Raw(x - bpp, y)`, `b = Raw(x, y - bpp)`, `c = Raw(x - bpp, y - bpp)`
/// — "Raw" heisst: der UNGEFILTERTE Wert. Die Zeile darueber wird fuer
/// Filter 2, 3 und 4 also in ihrer ROHEN Form gebraucht.
///
/// ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test `rundlauf_graustufen`,
/// 30.09.2026, bei Pixel (2,1): soll 15, ist 18). Es wurde die bereits
/// GEFILTERTE Zeile darueber uebergeben. Das fiesst sich nicht sofort aus:
/// Bild (0,0) stimmt, (1,1) stimmt, und ab irgendeiner Stelle kippt es —
/// ein „fast" richtiges Bild, wie der Auftrag sagt.
///
/// `roh` und `out` sind VERSCHIEDENE Puffer. Das ist zwingend, nicht bequem:
///
/// ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test
/// `filter_enkoder_und_dekoder_sind_invers`, 30.09.2026). Die Fassung davor
/// filterte IN PLACE und las dabei `a` aus demselben Puffer, in den sie
/// gerade schrieb — also den schon GEFILTERTEN Nachbarn statt des ROHEN.
/// Beispiel `[11, 48, 85]`, Filter Sub, bpp 1:
///   i=1: a = 11 (roh, i-1 noch unberuehrt)  -> 48 - 11 = 37
///   i=2: a = 37 (bereits GEFILTERT!)        -> 85 - 37 = 48   <-- falsch
/// Zurueckgerechnet: 11, 48, 96 statt 11, 48, 85.
///
/// Die Formel der Spezifikation (RFC 2083 Abschnitt 6) lautet
/// `x = roh[i] - f(roh[i-bpp], vor[i], vor[i-bpp])`: der Nachbar kommt
/// IMMER aus dem Rohpuffer. Deshalb zwei Puffer, auch wenn es eine
/// Kopie kostet.
///
/// `vorzeile` ist die bereits GEFILTERTE Zeile darueber — nicht die rohe.
/// Das ist der Punkt, an dem sich zwei Implementierungen unterscheiden.
/// Wuerde hier die ROHE Zeile genommen, wuerden Filter 2 und 4 ein „fast"
/// richtiges Bild liefern, und „fast" faellt beim Anschauen nicht auf.
/// Nur fuer den Test in `deflate.rs`: legt den privaten Enkoder offen.
/// `#[cfg(test)]` heisst: im ausgelieferten Code existiert die Funktion nicht.
#[cfg(test)]
pub fn filter_testbar_anwenden(roh: &[u8], vorzeile: &[u8], bpp: usize, typ: u8, out: &mut [u8]) {
    filter_anwenden(roh, vorzeile, bpp, typ, out);
}

fn filter_anwenden(roh: &[u8], vorzeile: &[u8], bpp: usize, typ: u8, out: &mut [u8]) {
    let n = roh.len().min(out.len());
    for i in 0..n {
        let a = if i >= bpp { roh[i - bpp] } else { 0 };
        let b = if i < vorzeile.len() { vorzeile[i] } else { 0 };
        let c = if i >= bpp && i < vorzeile.len() {
            vorzeile[i - bpp]
        } else {
            0
        };
        out[i] = match typ {
            FILTER_NONE => roh[i],
            FILTER_SUB => roh[i].wrapping_sub(a),
            FILTER_UP => roh[i].wrapping_sub(b),
            FILTER_AVERAGE => roh[i].wrapping_sub((((a as u16) + (b as u16)) / 2) as u8),
            FILTER_PAETH => roh[i].wrapping_sub(paeth(a, b, c)),
            _ => roh[i],
        };
    }
}

/// Waehlt den Filter mit der kleinsten Summe der Absolutdifferenzen.
///
/// Das ist die Empfehlung der Spezifikation („minimum sum of absolute
/// differences") und funktioniert erstaunlich gut, weil die Summe der
/// Signedness-Messung der Entropie des Bytewerts naech kommt: viele kleine
/// Abweichungen (typisch fuer ein Foto) werden billiger kodiert als wenige
/// grosse (typisch fuer Kanten).
fn bester_filter(zeile: &[u8], vorzeile: &[u8], bpp: usize) -> u8 {
    let mut bester = FILTER_NONE;
    let mut beste_summe = u64::MAX;
    for typ in FILTER_NONE..=FILTER_PAETH {
        let s = filter_summe(zeile, vorzeile, bpp, typ);
        if s < beste_summe {
            beste_summe = s;
            bester = typ;
        }
    }
    bester
}

fn filter_summe(zeile: &[u8], vorzeile: &[u8], bpp: usize, typ: u8) -> u64 {
    let mut summe = 0u64;
    for i in 0..zeile.len() {
        let a = if i >= bpp { zeile[i - bpp] } else { 0 };
        let b = if i < vorzeile.len() { vorzeile[i] } else { 0 };
        let c = if i >= bpp && i < vorzeile.len() {
            vorzeile[i - bpp]
        } else {
            0
        };
        let d = match typ {
            FILTER_NONE => zeile[i],
            FILTER_SUB => zeile[i].wrapping_sub(a),
            FILTER_UP => zeile[i].wrapping_sub(b),
            FILTER_AVERAGE => zeile[i].wrapping_sub((((a as u16) + (b as u16)) / 2) as u8),
            FILTER_PAETH => zeile[i].wrapping_sub(paeth(a, b, c)),
            _ => zeile[i],
        };
        // Als signed: 0x80 als -128 zaehlt 128, nicht 0. Genau das macht
        // die Heuristik brauchbar.
        summe += (d as i8).unsigned_abs() as u64;
    }
    summe
}
