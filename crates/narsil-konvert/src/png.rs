//! PNG SCHREIBEN (verlustfrei).
//!
//! Auftrag K1: „PNG (verlustfrei, Deflate ueber narsil-compress)". Die
//! Abnahme ist der strengste Test der ganzen Crate: `png_lesen` von
//! `narsil-image` muss das Ergebnis **bitgleich** zurueckgeben. Kein
//! Filterverlust, keine Farbmodus-Verluste, kein stilles Abschneiden.
//!
//! Drei Dinge unterscheiden einen PNG-Enkoder, der benutzt wird, von einem,
//! der nur Bytes hinschreibt:
//!
//! 1. **Filterwahl je Zeile.** Ohne Filter ist ein Foto unkomprimierbar;
//!    die Summe der Absolutdifferenzen zu minimieren macht es rund 8x
//!    kleiner (das ist die Empfehlung aus RFC 2083 Abschnitt 6.2 und das,
//!    was `narsil-image/src/png/schreiben.rs` macht — hier aber mit dem
//!    Kompressor aus `narsil-compress`, wie K1 es vorschreibt).
//! 2. **Farbwahl.** Graustufen statt RGB ist ein Drittel der Bytes;
//!    indexed statt RGB ist eine Zehntel, wenn das Bild <= 256 Farben hat.
//! 3. **Kein stiller Verlust.** Wenn `Farbmodus` etwas verlangt, was das
//!    Bild nicht kann (Graustufen auf einem Farbbild), ist das ein Fehler
//!    mit Namen — nicht eine Datei, in der nur noch der Rotkanal steht.

use crate::Bild;
use crate::farbe::{self, Palette, Stichprobe};
use crate::fehler::KonvertFehler;
use crate::grenzen;

/// Die PNG-Signatur (RFC 2083 Abschnitt 5.2). 8 Byte, kein Fallback.
pub const SIGNATUR: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Farbtyp 0: Graustufen.
const FARBTYP_GRAU: u8 = 0;
/// Farbtyp 2: RGB.
const FARBTYP_RGB: u8 = 2;
/// Farbtyp 3: indexed (mit PLTE).
const FARBTYP_PALETTE: u8 = 3;
/// Farbtyp 4: Graustufe + Alpha.
const FARBTYP_GRAU_ALPHA: u8 = 4;
/// Farbtyp 6: RGBA.
const FARBTYP_RGBA: u8 = 6;

/// Die fuenf Filtertypen aus RFC 2083 Abschnitt 6.
pub const FILTER_NONE: u8 = 0;
pub const FILTER_SUB: u8 = 1;
pub const FILTER_UP: u8 = 2;
pub const FILTER_AVERAGE: u8 = 3;
pub const FILTER_PAETH: u8 = 4;

/// In welchem Farbmodus soll geschrieben werden?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PngModus {
    /// Der Enkoder waehlt: indexed (<= 256 Farben) > Graustufe > RGB/RGBA.
    ///
    /// Das ist die Voreinstellung des Untermenues. Der Nutzer, der „als PNG"
    /// waehlt, will eine Datei, die sein Bild vollstaendig zeigt — nicht
    /// eine, die moeglichst klein ist.
    #[default]
    Auto,
    /// Nur die Helligkeit. Verliert Farbe, wenn das Bild farbig ist.
    Graustufen,
    /// Helligkeit + Deckkraft.
    GraustufenAlpha,
    /// Rot/Gruen/Blau, undurchsichtig. Verliert Alpha.
    Rgb,
    /// Rot/Gruen/Blau + Deckkraft.
    Rgba,
    /// Indexed: hoechstens 256 Farben in einer Palette.
    Palette,
}

/// Wie viele Farben `Palette` bekommen darf.
///
/// Der Wert ist 256, weil indexed PNG bei Bittiefe 8 genau 256 Paletten-
/// eintraege hat. Eine kleinere Zahl macht die Datei nicht kleiner (die
/// Palette wird auf 3 Byte je Eintrag aufgerundet), nur schlechter.
pub const MAX_PNG_FARBEN: usize = 256;

/// Schreibt ein PNG.
///
/// # Fehler
/// - [`KonvertFehler::GroesseNull`] bei Breite oder Hoehe 0.
/// - [`KonvertFehler::KanteZuGross`] / [`KonvertFehler::ZuVielePunkte`] /
///   [`KonvertFehler::SpeicherZuGross`] bei zu grossen Bildern.
/// - [`KonvertFehler::PufferZuKurz`] wenn `rgba` zu kurz ist.
/// - [`KonvertFehler::UnbekannteOption`] wenn [`PngModus::Palette`] verlangt
///   wird und das Bild mehr als 256 Farben hat UND keine Farbe verloren
///   werden darf.
pub fn schreiben(bild: &Bild, modus: PngModus) -> Result<Vec<u8>, KonvertFehler> {
    grenzen::pruefe_bild(bild)?;
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;

    // --- Farbmodus bestimmen ---
    let (farbtyp, tiefe, palette) = bestimme_modus(bild, modus)?;

    let mut out: Vec<u8> = Vec::new();
    // Reservierung aus einer GEPRUEFTEN Zahl: 8 Signatur + 25 IHDR +
    // 1024 PLTE (256*3) + 1024 tRNS + 12 IEND + IDAT-Kopf.
    let mut kapazitaet = 8 + 25 + 12 + 1050;
    kapazitaet += breite * hoehe * 4 + hoehe;
    grenzen::pruefe_laenge(
        kapazitaet,
        grenzen::MAX_SPEICHER,
        "PNG-Ausgabe vorab reserviert",
    )?;
    out.reserve(kapazitaet);

    // --- Signatur ---
    out.extend_from_slice(&SIGNATUR);

    // --- IHDR: immer 13 Byte, immer zuerst (RFC 2083 Abschnitt 5.3) ---
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&bild.breite.to_be_bytes());
    ihdr[4..8].copy_from_slice(&bild.hoehe.to_be_bytes());
    ihdr[8] = tiefe;
    ihdr[9] = farbtyp;
    ihdr[10] = 0; // Kompressionsverfahren 0 = DEFLATE
    ihdr[11] = 0; // Filtermethode 0 = adaptiv (wir waehlen je Zeile)
    ihdr[12] = 0; // Interlace 0 = keine (Adam7 waere hier pure Verschwendung)
    chunk(&mut out, b"IHDR", &ihdr);

    // --- PLTE: nur bei indexed ---
    if farbtyp == FARBTYP_PALETTE {
        let pal = palette
            .as_ref()
            .ok_or(KonvertFehler::QuantStuck { stufe: 0 })?;
        let mut plte = Vec::with_capacity(pal.len() * 3);
        for f in &pal.farben {
            plte.push(f[1]); // R
            plte.push(f[2]); // G
            plte.push(f[3]); // B
        }
        chunk(&mut out, b"PLTE", &plte);

        // --- tRNS: nur wenn ein Alpha < 255 vorkommt ---
        if pal.mit_alpha && pal.farben.iter().any(|f| f[0] != 255) {
            let mut trns = Vec::with_capacity(pal.len());
            for f in &pal.farben {
                trns.push(f[0]);
            }
            chunk(&mut out, b"tRNS", &trns);
        }
    }

    // --- IDAT: die Zeilen mit Filterbyte, als zlib-Strom ---
    // Die Farbindex-Tabelle wird EINMAL gebaut, nicht je Zeile: sie ist
    // 32 KiB, und 480 Zeilen wuerden sie 480 Mal aufbauen.
    let argb = palette.as_ref().is_some_and(|p| p.mit_alpha);
    let index = palette.as_ref().map(|pal| {
        // Der exakte Weg lohnt sich nur, wenn die Palette genau die
        // Bildfarben enthaelt — das ist der indexed-Fall von [`PngModus::Auto`]
        // und der verlustfreie Fall von [`PngModus::Palette`].
        let mut schluessel: Vec<u32> = Vec::with_capacity(bild.breite as usize);
        for p in 0..bild.breite as usize * bild.hoehe as usize {
            let i = p * 4;
            schluessel.push(if argb {
                ((bild.rgba[i + 3] as u32) << 24)
                    | ((bild.rgba[i] as u32) << 16)
                    | ((bild.rgba[i + 1] as u32) << 8)
                    | bild.rgba[i + 2] as u32
            } else {
                ((bild.rgba[i] as u32) << 16)
                    | ((bild.rgba[i + 1] as u32) << 8)
                    | bild.rgba[i + 2] as u32
            });
        }
        schluessel.sort_unstable();
        schluessel.dedup();
        if argb {
            farbe::Farbindex::neu(pal.clone(), Some(&schluessel))
        } else {
            farbe::Farbindex::neu_rgb(pal.clone(), Some(&schluessel))
        }
    });
    let roh = zeilen_mit_filter(bild, farbtyp, index.as_ref(), argb);
    let strom = zlib_einpacken(&roh);
    chunk(&mut out, b"IDAT", &strom);

    // --- IEND: Laenge 0, keine Daten ---
    chunk(&mut out, b"IEND", &[]);

    Ok(out)
}

/// Schreibt ein PNG und misst dabei die Groesse (fuer K4 / Bericht).
pub fn groesse(bild: &Bild, modus: PngModus) -> Result<usize, KonvertFehler> {
    Ok(schreiben(bild, modus)?.len())
}

/// Haengt einen Chunk an: Laenge, Typ, Daten, CRC.
///
/// Der CRC laeuft ueber TYP + DATEN, nicht ueber die Daten allein. Wer ihn
/// nur ueber die Daten rechnet, erzeugt eine Datei, die jeder `pngcheck`
/// ablehnt — und, schlimmer, die manche Leser stillschweigend annehmen.
fn chunk(out: &mut Vec<u8>, typ: &[u8; 4], daten: &[u8]) {
    out.extend_from_slice(&(daten.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(daten);
    let crc = narsil_compress::crc32_zwei(typ, daten);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Baut den zlib-Strom: 2 Byte Kopf, DEFLATE, Adler-32.
///
/// WARUM NICHT `narsil_image::zlib::rahme`: der Auftrag K1 sagt „Deflate ueber
/// narsil-compress", und `narsil-compress` hat genau einen oeffentlichen
/// Kompressor — [`narsil_compress::deflate`]. Deren `zlib`-Modul ist
/// `mod zlib;` (privat), nur `rahme` ist nicht erreichbar. Der Kopf ist
/// darum selbst geschrieben: CMF = 0x78 (DEFLATE, 32-KiB-Fenster), FLG = 0x01
/// (FLEVEL 0, kein Dictionary). `0x7801 = 30721`, `30721 % 31 == 0` — die
/// FCHECK-Bedingung des zlib-Rahmens ist damit erfuellt, und genau das
/// prueft der Leser zuerst.
///
/// Adler-32 kommt aus demselben Modul (`narsil_compress::adler32`), also
/// steht die Pruefsumme nicht noch einmal hier.
fn zlib_einpacken(roh: &[u8]) -> Vec<u8> {
    let mut strom = Vec::with_capacity(roh.len() / 2 + 16);
    strom.push(0x78); // CMF
    strom.push(0x01); // FLG
    strom.extend_from_slice(&narsil_compress::deflate(roh));
    strom.extend_from_slice(&narsil_compress::adler32(roh).to_be_bytes());
    strom
}

/// Entscheidet den Farbmodus.
///
/// Der `Auto`-Zweig ist der interesting one, weil er die Reihenfolge
/// nach Dateigroesse optimiert:
///
/// 1. **Indexed**, wenn das Bild hoechstens 256 Farben hat. Das ist der Fall
///    fuer Logos, Screenshots, Diagramme — also fuer genau die Bilder, die
///    ein Nutzer aus einer Webseite speichert.
/// 2. **Graustufe**, wenn r == g == b fuer jeden Pixel. Ein Drittel.
/// 3. **Graustufe+Alpha**, wenn es Alpha gibt, aber die Farben gleich sind.
/// 4. **RGB** bzw. **RGBA**, sonst.
///
/// Die Grenze fuer indexed ist nicht geraten: `PLTE` fasst 256*3 Byte, und
/// `tRNS` 256 Byte. Bei 257 Farben waere indexed schlechter als RGB —
/// deshalb die harte Zahl.
fn bestimme_modus(
    bild: &Bild,
    modus: PngModus,
) -> Result<(u8, u8, Option<Palette>), KonvertFehler> {
    let n = bild.breite as usize * bild.hoehe as usize;
    let (mut grau, mut undurchsichtig) = (true, true);
    // Nur die linke Spalte pruefen ist falsch; aber alle Pixel twice geht
    // auch nicht. Kompromiss: wir brechen ab, sobald beide Flags fallen —
    // dann ist die Entscheidung sowieso gefallen.
    for p in 0..n {
        let i = p * 4;
        let (r, g, b, a) = (
            bild.rgba[i],
            bild.rgba[i + 1],
            bild.rgba[i + 2],
            bild.rgba[i + 3],
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

    match modus {
        PngModus::Auto => {
            // Alpha muss als Teil der Farbe gezaehlt werden, SONST verliert
            // indexed PNG die Transparenz: zwei Pixel mit gleichem RGB und
            // verschiedenem Alpha bekommen denselben Index.
            //
            // GEMESSEN 02.10.2026: `png_indexed_mit_transparenz` schlug fehl,
            // und die erste Abweichung lag genau in der Ecke, die
            // durchsichtig gemacht worden war.
            let mit_alpha = !undurchsichtig;
            let probe = Stichprobe::von_bild(bild, mit_alpha);
            if probe.ist_vollstaendig() && probe.anzahl() <= MAX_PNG_FARBEN {
                // Indexed ist moeglich: die Palette ist genau die Menge der
                // (ARGB-)Farben des Bildes, 1:1 und verlustfrei.
                let (f, _g) = probe.teile();
                let palette = Palette {
                    farben: f.to_vec(),
                    mit_alpha,
                    // Die Palette IST die Menge der Bildfarben: exakt.
                    verlustfrei: true,
                };
                return Ok((FARBTYP_PALETTE, 8, Some(palette)));
            }
            Ok(match (grau, undurchsichtig) {
                (true, true) => (FARBTYP_GRAU, 8, None),
                (true, false) => (FARBTYP_GRAU_ALPHA, 8, None),
                (false, true) => (FARBTYP_RGB, 8, None),
                (false, false) => (FARBTYP_RGBA, 8, None),
            })
        }
        PngModus::Graustufen => {
            if !grau {
                // Kein stiller Verlust: der Aufrufer hat ausdruecklich
                // Graustufen verlangt, und das Bild ist farbig.
                return Err(KonvertFehler::ZielNichtSinnvoll {
                    ziel: crate::Zielformat::Png,
                    grund: "Graustufen verlangt, das Bild hat Farbe",
                });
            }
            Ok((FARBTYP_GRAU, 8, None))
        }
        PngModus::GraustufenAlpha => {
            if !grau {
                return Err(KonvertFehler::ZielNichtSinnvoll {
                    ziel: crate::Zielformat::Png,
                    grund: "Graustufen verlangt, das Bild hat Farbe",
                });
            }
            Ok((FARBTYP_GRAU_ALPHA, 8, None))
        }
        PngModus::Rgb => Ok((FARBTYP_RGB, 8, None)),
        PngModus::Rgba => Ok((FARBTYP_RGBA, 8, None)),
        PngModus::Palette => {
            let probe = Stichprobe::von_bild(bild, !undurchsichtig);
            let (f, _g) = probe.teile();
            if f.len() > MAX_PNG_FARBEN {
                return Err(KonvertFehler::PaletteZuGross {
                    gebraucht: f.len(),
                    grenze: MAX_PNG_FARBEN,
                });
            }
            Ok((
                FARBTYP_PALETTE,
                8,
                Some(Palette {
                    farben: f.to_vec(),
                    mit_alpha: !undurchsichtig,
                    // Auch hier: es wurde nichts zusammengefasst, sonst waere
                    // der Fehler `PaletteZuGross` schon geflogen.
                    verlustfrei: true,
                }),
            ))
        }
    }
}

/// Baut die Zeilen fuer den Enkoder: Filterbyte + Nutzbytes, Filter je
/// Zeile nach dem Kleinsten-Absolutdifferenz-Mass.
///
/// Die drei Puffer (roh, gefiltert, rohe Vorzeile) sind jeweils genau
/// einmal gebucht. Der dritte wird gebraucht, weil Filter 2, 3 und 4 die
/// ROHEN Nachbarn brauchen (RFC 2083 Abschnitt 6.2 sagt ausdruecklich
/// `Raw(...)`) — die Zeile DARUEBER in ihrer ungefilterten Form.
fn zeilen_mit_filter(
    bild: &Bild,
    farbtyp: u8,
    index: Option<&farbe::Farbindex>,
    argb: bool,
) -> Vec<u8> {
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;
    let (bpp, kanaele) = match farbtyp {
        FARBTYP_GRAU => (1usize, 1usize),
        FARBTYP_GRAU_ALPHA => (2, 2),
        FARBTYP_RGB => (3, 3),
        FARBTYP_RGBA => (4, 4),
        // Indexed: Bittiefe 8 => ein Byte je Pixel. Fuer tiefere Tiefen
        // waere bpp = 1, aber dann sind „Pixel" mehrere Nachbarn fuer den
        // Filter — die Spezifikation sagt dafuer bpp = 1, und das stimmt
        // auch, weil der Filter auf dem ENTPACKTEN Strom arbeitet.
        FARBTYP_PALETTE => (1, 1),
        _ => (1, 1),
    };
    let zeilenbreite = breite * kanaele;
    if zeilenbreite == 0 || hoehe == 0 {
        return Vec::new();
    }
    let mut roh = vec![0u8; hoehe * (zeilenbreite + 1)];
    let mut zeile_roh = vec![0u8; zeilenbreite];
    let mut zeile_filtered = vec![0u8; zeilenbreite];
    let mut vorzeile_roh = vec![0u8; zeilenbreite];
    for y in 0..hoehe {
        zeile_bauen(bild, y, farbtyp, index, argb, &mut zeile_roh);
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

/// Fuellt `ziel` mit den Nutzbytes der Zeile `y` im gewaehlten Modus.
///
/// Ueber `bild.rgba` direkt indiziert statt ueber [`Bild::pixel`]: hier
/// ist bereits geprueft, dass `rgba` lang genug ist
/// ([`grenzen::pruefe_bild`]), und der Klemmpfad von `pixel` wuerde einen
/// Randfehler stillschweigend schwarz malen.
fn zeile_bauen(
    bild: &Bild,
    y: usize,
    farbtyp: u8,
    index: Option<&farbe::Farbindex>,
    argb: bool,
    ziel: &mut [u8],
) {
    let breite = bild.breite as usize;
    let grund = y * breite * 4;
    match farbtyp {
        FARBTYP_GRAU => {
            for (x, z) in ziel[..breite].iter_mut().enumerate() {
                *z = bild.rgba[grund + x * 4];
            }
        }
        FARBTYP_GRAU_ALPHA => {
            for x in 0..breite {
                ziel[x * 2] = bild.rgba[grund + x * 4];
                ziel[x * 2 + 1] = bild.rgba[grund + x * 4 + 3];
            }
        }
        FARBTYP_RGB => {
            // Drei Bytes je Pixel aus vier: der Index wird gebraucht,
            // weil das Ziel Three-Bytes-Schritte macht, die Quelle
            // Four-Byte-Schritte. Da hilft kein `iter()`.
            #[allow(clippy::needless_range_loop)]
            for x in 0..breite {
                ziel[x * 3] = bild.rgba[grund + x * 4];
                ziel[x * 3 + 1] = bild.rgba[grund + x * 4 + 1];
                ziel[x * 3 + 2] = bild.rgba[grund + x * 4 + 2];
            }
        }
        FARBTYP_RGBA => {
            ziel[..breite * 4].copy_from_slice(&bild.rgba[grund..grund + breite * 4]);
        }
        FARBTYP_PALETTE => {
            let tab = index.expect("Farbindex bei indexed gesetzt");
            // Wie `FARBTYP_RGB`: Quelle Four-Byte, Ziel Ein-Byte.
            #[allow(clippy::needless_range_loop)]
            for x in 0..breite {
                let i = grund + x * 4;
                ziel[x] = if argb {
                    // Alpha ist Teil der Palette: der Schluessel ist ARGB.
                    tab.index(
                        bild.rgba[i + 3],
                        bild.rgba[i],
                        bild.rgba[i + 1],
                        bild.rgba[i + 2],
                    )
                } else {
                    tab.index_rgb_exakt(bild.rgba[i], bild.rgba[i + 1], bild.rgba[i + 2])
                };
            }
        }
        _ => {}
    }
}

/// Filtert `roh` nach `out`.
///
/// `roh` und `out` sind VERSCHIEDENE Puffer — zwingend, nicht bequem. Die
/// Formel der Spezifikation lautet `x = roh[i] - f(roh[i-bpp], vor[i],
/// vor[i-bpp])`: der Nachbar kommt IMMER aus dem Rohpuffer. Filtert man in
/// Place, laesst man den schon GEFILTERTEN Nachbarn einrechnen und bekommt
/// ein Bild, das „fast" richtig aussieht und in der dritten Zeile kippt.
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
            FILTER_SUB => roh[i].wrapping_sub(a),
            FILTER_UP => roh[i].wrapping_sub(b),
            FILTER_AVERAGE => roh[i].wrapping_sub((((a as u16) + (b as u16)) / 2) as u8),
            FILTER_PAETH => roh[i].wrapping_sub(paeth(a, b, c)),
            _ => roh[i],
        };
    }
}

/// Paeth-Vorhersager (RFC 2083 Abschnitt 6.6). `a` links, `b` oben,
/// `c` oben links.
#[must_use]
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

/// Waehlt den Filter mit der kleinsten Summe der Absolutdifferenzen.
///
/// Als signed gerechnet: 0x80 zaehlt als -128, also 128. Genau das macht
/// die Heuristik brauchbar — eine Zeile voller kleiner Vorzeichen ist
/// billiger zu kodieren als eine mit wenigen grossen, und genau das sieht
/// die Heuristik.
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
            FILTER_SUB => zeile[i].wrapping_sub(a),
            FILTER_UP => zeile[i].wrapping_sub(b),
            FILTER_AVERAGE => zeile[i].wrapping_sub((((a as u16) + (b as u16)) / 2) as u8),
            FILTER_PAETH => zeile[i].wrapping_sub(paeth(a, b, c)),
            _ => zeile[i],
        };
        summe += (d as i8).unsigned_abs() as u64;
    }
    summe
}
