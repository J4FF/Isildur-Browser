//! PNG LESEN.
//!
//! Drei Fehler aus diesem Projekt, die man beim Lesen von Bildern immer
//! wieder trifft und die deshalb hier ausdruecklich benannt sind:
//!
//! 1. Die Chunk-Kette muss in der Reihenfolge IHDR, [PLTE], IDAT, IEND
//!    kommen. Wir pruefen das, statt es anzunehmen.
//! 2. Der CRC ist nicht optional. Er ist das Einzige, was einen
//!    Bitfehler im Datei-Transport aufdeckt. Ohne ihn zeigt der Browser
//!    ein zerschrummeltes Bild und nennt es ein Bild.
//! 3. Die Groesse wird VOR der Allokation geprueft. Eine Datei, die
//!    "100000x100000" behauptet, darf keine 40 GB anfordern.

use crate::inflate::entpacke_in;
use crate::png::schreiben::paeth;
use crate::png::{
    FARBTYP_GRAU, FARBTYP_GRAU_ALPHA, FARBTYP_PALETTE, FARBTYP_RGB, FARBTYP_RGBA, SIGNATUR,
};
use crate::{Bild, BildFehler, MAX_BILDPUNKTE, MAX_KANTE};

/// Was der Header behauptet. Aus IHDR gerechnet, bevor irgendetwas
/// allokiert wird.
struct Kopfdaten {
    breite: u32,
    hoehe: u32,
    tiefe: u8,
    farbtyp: u8,
    /// Bytes je Pixel fuer die Filterrechnung, mindestens 1.
    bpp_filter: usize,
    /// Bytes einer ungefilterten Bildzeile.
    zeilenbreite: usize,
    /// Bytes der ganzen Entpackung inklusive Filterbyte je Zeile.
    erwarte_entpackt: usize,
    /// 0 = zeilenweise, 1 = Adam7 (RFC 2083 Abschnitt 2.6).
    ///
    /// B12-Nachmessung (02.10.2026): ohne Adam7 blieb das Logo von
    /// debian.org leer — `tests/daten/openlogo-50.png` (50x61, RGBA,
    /// IHDR-Byte 12 = 1) kam mit dem BENANNTEN Fehler „Interlace-Verfahren
    /// 1 (Adam7) nicht unterstuetzt“ zurueck. Andere Werte als 0 und 1 gibt
    /// es in PNG nicht; sie bleiben der Fehler `Interlace`.
    interlace: u8,
}

impl Kopfdaten {
    /// Rechnet alles aus dem IHDR. KEINE Allokation.
    fn aus_ihdr(
        breite: u32,
        hoehe: u32,
        tiefe: u8,
        farbtyp: u8,
        interlace: u8,
    ) -> Result<Kopfdaten, BildFehler> {
        if breite == 0 || hoehe == 0 {
            return Err(BildFehler::GroesseNull);
        }
        if breite > MAX_KANTE || hoehe > MAX_KANTE {
            return Err(BildFehler::ZuGross {
                breite,
                hoehe,
                kante: MAX_KANTE,
            });
        }
        let punkte = breite as u64 * hoehe as u64;
        if punkte > MAX_BILDPUNKTE {
            return Err(BildFehler::ZuGross {
                breite,
                hoehe,
                kante: MAX_BILDPUNKTE as u32,
            });
        }

        let (kanaele, erlaubte_tiefen): (usize, &[u8]) = match farbtyp {
            FARBTYP_GRAU => (1, &[1, 2, 4, 8, 16]),
            FARBTYP_RGB => (3, &[8, 16]),
            FARBTYP_PALETTE => (1, &[1, 2, 4, 8]),
            FARBTYP_GRAU_ALPHA => (2, &[8, 16]),
            FARBTYP_RGBA => (4, &[8, 16]),
            other => return Err(BildFehler::UnbekannterFarbtyp { wert: other }),
        };
        if !erlaubte_tiefen.contains(&tiefe) {
            return Err(BildFehler::UnbekannteBittiefe { farbtyp, tiefe });
        }

        let bpp_filter = ((kanaele * tiefe as usize) + 7) / 8;
        let zeilenbreite = (breite as usize * kanaele * tiefe as usize + 7) / 8;
        // M3/M4: die erwartete Entpackgroesse ist bei Adam7 NICHT
        // `zeilenbreite + 1` mal Hoehe, sondern die Summe ueber die sieben
        // Durchgaenge. Sie wird VOR dem Entpacken gebraucht (sie begrenzt
        // den zlib-Strom), also muss sie hier gerechnet werden und nicht
        // erst beim Entflechten.
        let erwarte_entpackt = if interlace == 1 {
            adam7_entpackt_gross(breite, hoehe, zeilenbreite, bpp_filter)
        } else {
            (zeilenbreite + 1)
                .checked_mul(hoehe as usize)
                .ok_or(BildFehler::ZuGross {
                    breite,
                    hoehe,
                    kante: MAX_KANTE,
                })?
        };

        Ok(Kopfdaten {
            breite,
            hoehe,
            tiefe,
            farbtyp,
            bpp_filter,
            zeilenbreite,
            erwarte_entpackt,
            interlace,
        })
    }
}

/// Die sieben Durchgaenge nach RFC 2083 Abschnitt 2.6 (Adam7):
/// `(x_start, y_start, x_schritt, y_schritt)`.
///
/// GEMESSENE Herleitung an `openlogo-50.png` (50x61): Durchgang 1 hat
/// 7 x 8 Pixel, Durchgang 2 hat 6 x 8, Durchgang 3 hat 13 x 7 und so
/// weiter — und die Summe der Zeilenlaengen ist genau die Laenge des
/// zlib-Stroms nach dem Entpacken.
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Die Pixelzahl eines Durchgangs in einer Richtung: wie viele Werte von
/// `start` in Schritten von `schritt` bis `ende` passen.
fn adam7_achse(ende: u32, start: u32, schritt: u32) -> u32 {
    if ende <= start {
        return 0;
    }
    (ende - start).div_ceil(schritt)
}

/// Die erwartete Entpackgroesse bei Adam7.
///
/// M3/M4: `saturating_*` und ein Rueckgabewert `None`, wenn die Rechnung
/// die Grenze verlaesst — eine zerlegte Datei darf keine Allokation
/// erzwingen, die ihre eigene Angabe begruendet.
fn adam7_entpackt_gross(breite: u32, hoehe: u32, zeilenbreite: usize, bpp: usize) -> usize {
    let mut summe = 0usize;
    for &(xs, ys, xst, yst) in ADAM7.iter() {
        let w = adam7_achse(breite, xs, xst) as usize;
        let h = adam7_achse(hoehe, ys, yst) as usize;
        if w == 0 || h == 0 {
            continue;
        }
        let zeile = w.saturating_mul(bpp);
        summe = summe.saturating_add(h.saturating_mul(zeile.saturating_add(1)));
    }
    let _ = zeilenbreite;
    summe
}

/// Liest ein PNG. Kein Panic, egal was in den Bytes steht.
pub fn png_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    if daten.len() < 8 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 8,
        });
    }
    if daten[..8] != SIGNATUR {
        return Err(BildFehler::KeineSignatur);
    }

    let mut kopf: Option<Kopfdaten> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut trns_grau: Option<u16> = None;
    let mut trns_rgb: Option<[u16; 3]> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_idat = false;
    let mut saw_iend = false;

    let mut pos = 8usize;
    while pos < daten.len() {
        // Ein Chunk besteht aus 4 Byte Laenge, 4 Byte Typ, n Byte Daten,
        // 4 Byte CRC. Ueberlaeufe in Rust paniken nicht, aber sie wuerden
        // zu "abgeschnitten" fuehren, was hier FALSCH waere — deshalb der
        // genaue Vergleich statt `>=`.
        if pos + 8 > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: pos,
                gebraucht: 8,
            });
        }
        let laenge =
            u32::from_be_bytes([daten[pos], daten[pos + 1], daten[pos + 2], daten[pos + 3]])
                as usize;
        let typ = [
            daten[pos + 4],
            daten[pos + 5],
            daten[pos + 6],
            daten[pos + 7],
        ];

        for &z in typ.iter() {
            if !(z.is_ascii_alphanumeric() || z == b'_' || z == b'-') {
                return Err(BildFehler::UngueltigerChunkTyp { typ, position: pos });
            }
        }

        let laengen_pos = pos;
        let daten_pos = pos + 8;
        // `laenge` ist bis hierher maximal 4294967295, `daten_pos` ist
        // klein — die Addition passt in usize auf 64 Bit. Auf 32 Bit
        // wuerde der Vergleich trotzdem greifen, weil `laenge` dort schon
        // nicht mehr passt. Der Check ist deshalb `>` und nicht `+ 12`.
        if daten_pos + laenge + 4 > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: laengen_pos,
                gebraucht: laenge + 12,
            });
        }
        let inhalt = &daten[daten_pos..daten_pos + laenge];
        let crc_pos = daten_pos + laenge;
        let crc_datei = u32::from_be_bytes([
            daten[crc_pos],
            daten[crc_pos + 1],
            daten[crc_pos + 2],
            daten[crc_pos + 3],
        ]);
        let crc_gerechnet = crate::pruefsummen::crc32_zwei(&typ, inhalt);
        if crc_datei != crc_gerechnet {
            return Err(BildFehler::CrcFalsch {
                typ,
                position: laengen_pos,
                erwartet: crc_datei,
                gefunden: crc_gerechnet,
            });
        }

        match &typ {
            b"IHDR" => {
                if pos != 8 {
                    return Err(BildFehler::KeinIhdr); // nicht der erste Chunk
                }
                if laenge != 13 {
                    return Err(BildFehler::IhdrLaengeFalsch {
                        gefunden: laenge as u32,
                    });
                }
                let breite = u32::from_be_bytes([inhalt[0], inhalt[1], inhalt[2], inhalt[3]]);
                let hoehe = u32::from_be_bytes([inhalt[4], inhalt[5], inhalt[6], inhalt[7]]);
                let tiefe = inhalt[8];
                let farbtyp = inhalt[9];
                let kompression = inhalt[10];
                let filtermethode = inhalt[11];
                let interlace = inhalt[12];
                if kompression != 0 || filtermethode != 0 {
                    return Err(BildFehler::UnbekanntesVerfahren {
                        kompression,
                        filter: filtermethode,
                    });
                }
                if interlace > 1 {
                    // B12-Nachmessung: 1 ist Adam7 und wird gelesen (siehe
                    // `adam7_entflechten`). Alles ueber 1 gibt es in PNG
                    // nicht — das bleibt der BENANNTE Fehler, damit eine
                    // kaputte Datei nicht als Bild durchgeht.
                    return Err(BildFehler::Interlace {
                        verfahren: interlace,
                    });
                }
                // Release 15.10.2026: 16-bit samples are rounded to 8 bits
                // like every browser does (MEASURED before: WPT canvas
                // `red-16x16.png` was "broken", createPattern threw).
                kopf = Some(Kopfdaten::aus_ihdr(
                    breite, hoehe, tiefe, farbtyp, interlace,
                )?);
            }
            b"PLTE" => {
                if laenge == 0 || laenge % 3 != 0 || laenge / 3 > 256 {
                    return Err(BildFehler::PaletteFalsch {
                        gefunden: laenge / 3,
                    });
                }
                palette = inhalt.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => match kopf.as_ref().map(|k| k.farbtyp) {
                Some(FARBTYP_PALETTE) => {
                    trns = inhalt.to_vec();
                }
                Some(FARBTYP_GRAU) => {
                    if laenge != 2 {
                        return Err(BildFehler::TrnsFalsch { gefunden: laenge });
                    }
                    trns_grau = Some(u16::from_be_bytes([inhalt[0], inhalt[1]]));
                }
                Some(FARBTYP_RGB) => {
                    if laenge != 6 {
                        return Err(BildFehler::TrnsFalsch { gefunden: laenge });
                    }
                    trns_rgb = Some([
                        u16::from_be_bytes([inhalt[0], inhalt[1]]),
                        u16::from_be_bytes([inhalt[2], inhalt[3]]),
                        u16::from_be_bytes([inhalt[4], inhalt[5]]),
                    ]);
                }
                _ => return Err(BildFehler::TrnsFalsch { gefunden: laenge }),
            },
            b"IDAT" => {
                if kopf.is_none() {
                    return Err(BildFehler::KeinIhdr);
                }
                if !saw_idat {
                    // Alle IDAT muessen zusammenhaengen; ein Chunksprung
                    // dazwischen waere ein Fehler nach RFC 2083.
                    idat.clear();
                    saw_idat = true;
                }
                idat.extend_from_slice(inhalt);
            }
            b"IEND" => {
                if laenge != 0 {
                    return Err(BildFehler::Abgeschnitten {
                        position: laengen_pos,
                        gebraucht: laenge + 12,
                    });
                }
                saw_iend = true;
            }
            // Anciliary-Chunks (Kleinbuchstabe am Anfang) duerfen ueberlesen
            // werden — so steht es in RFC 2083, und tEXt/pHYs/gAMA/tIME
            // kommen in jeder zweiten echten Datei vor.
            _ => {
                if typ[0].is_ascii_uppercase() {
                    return Err(BildFehler::UnbekannterChunk {
                        typ,
                        position: laengen_pos,
                    });
                }
            }
        }

        pos = crc_pos + 4;
        if saw_iend {
            break;
        }
    }

    let kopf = kopf.ok_or(BildFehler::KeinIhdr)?;
    if !saw_iend {
        return Err(BildFehler::IendFehlt);
    }
    if !saw_idat {
        return Err(BildFehler::IdatFehlt);
    }
    if kopf.farbtyp == FARBTYP_PALETTE && palette.is_empty() {
        return Err(BildFehler::PaletteFalsch { gefunden: 0 });
    }

    // --- Entpacken. Die Grenze ist die aus dem Header gerechnete Groesse:
    // ein zlib-Strom darf nicht mehr liefern, als das Bild braucht. ---
    let mut roh = Vec::new();
    entpacke_in(&idat, &mut roh, kopf.erwarte_entpackt).map_err(BildFehler::Zlib)?;
    if roh.len() < kopf.erwarte_entpackt {
        return Err(BildFehler::ZuWenigDaten {
            erwartet: kopf.erwarte_entpackt,
            gefunden: roh.len(),
        });
    }
    roh.truncate(kopf.erwarte_entpackt);

    if kopf.interlace == 1 {
        // B12-Nachmessung: Adam7 entflechten, dann geht derselbe Weg
        // weiter wie bei einer nicht verflochtenen Datei. Das ist der
        // Grund fuer diese Bauart: `filter_rueckgaengig` und `expandieren`
        // bleiben unveraendert und damit auch unveraendert geprueft.
        roh = adam7_entflechten(&roh, &kopf)?;
    }
    filter_rueckgaengig(&mut roh, &kopf)?;

    let mut bild = Bild {
        breite: kopf.breite,
        hoehe: kopf.hoehe,
        rgba: Vec::with_capacity(kopf.breite as usize * kopf.hoehe as usize * 4),
    };
    expandieren(&roh, &kopf, &palette, &trns, trns_grau, trns_rgb, &mut bild);
    // B4 (02.10.2026): ein eingebettetes ICC-Profil aus dem `iCCP`-Chunk
    // wird ausgewertet, genau wie im JPEG. Ohne das bleibt ein P3- oder
    // AdobeRGB-Bild in seinem eigenen Farbraum stehen.
    crate::icc::auf_srgb(&mut bild, daten);
    Ok(bild)
}

/// Entflechtet eine Adam7-Datei in einen normalen Zeilenpuffer.
///
/// # WAS ADAM7 IST
///
/// Ein Adam7-PNG schreibt das Bild in sieben Durchgaenge, jeder ein eigenes
/// Teilbild mit eigenen Zeilen und eigenen Filterbytes (RFC 2083
/// Abschnitt 2.6). Wer die Datei wie eine normale liest, sieht sieben
/// Zeilenfragmente statt eines Bildes — bei `openlogo-50.png` (50x61)
///
/// ```text
/// Durchgang 1: 7 x 8 Pixel     Durchgang 2: 6 x 8
/// Durchgang 3: 13 x 7          Durchgang 4: 12 x 8
/// Durchgang 5: 25 x 6          Durchgang 6: 25 x 8
/// Durchgang 7: 50 x 4
/// ```
///
/// GEMESSENE SUMME der Zeilenbytes: 9*13 + ... = die Laenge, die
/// `adam7_entpackt_gross` vorher berechnet hat. Ein Test vergleicht die
/// Zahl mit der des zlib-Stroms (`adam7_groesse_stimmt_mit_dem_strom`).
///
/// # WIE
///
/// Je Durchgang wird jede Zeile fuer sich entfiltert (`filter_zeile`, die
/// benutzt derselbe Weg wie der normale) und danach Pixel fuer Pixel in
/// den Zielpuffer gelegt. **Filterbyte 0** wird vor jede Zielzeile
/// geschrieben, damit `filter_rueckgaengig` danach nichts zu tun hat und
/// `expandieren` an der gewohnten Stelle liest.
///
/// M3/M4: jeder Zugriff laeuft ueber `get`/`checked_add`; ein Durchgang,
/// der laenger als der Reststrom ist, ist `ZuWenigDaten` und kein Panic.
fn adam7_entflechten(roh: &[u8], kopf: &Kopfdaten) -> Result<Vec<u8>, BildFehler> {
    let breite = kopf.breite as usize;
    let hoehe = kopf.hoehe as usize;
    let n = kopf.zeilenbreite;
    let bpp = kopf.bpp_filter.max(1);
    // Ziel: `hoehe` Zeilen zu je `1 + n` Byte, erstes Byte Filtertyp 0.
    let mut ziel = vec![0u8; hoehe.saturating_mul(n.saturating_add(1))];
    let mut pos = 0usize;
    for &(xs, ys, xst, yst) in ADAM7.iter() {
        let w = adam7_achse(kopf.breite, xs, xst) as usize;
        let h = adam7_achse(kopf.hoehe, ys, yst) as usize;
        if w == 0 || h == 0 {
            continue;
        }
        let zeilenbytes = w.saturating_mul(bpp);
        let mut vorzeile: Vec<u8> = vec![0u8; zeilenbytes];
        for r in 0..h {
            let typ = roh.get(pos).copied().ok_or(BildFehler::ZuWenigDaten {
                erwartet: kopf.erwarte_entpackt,
                gefunden: pos,
            })?;
            let anfang = pos.saturating_add(1);
            let mut zeile: Vec<u8> = roh
                .get(anfang..anfang.saturating_add(zeilenbytes))
                .ok_or(BildFehler::ZuWenigDaten {
                    erwartet: kopf.erwarte_entpackt,
                    gefunden: roh.len(),
                })?
                .to_vec();
            if typ > 4 {
                return Err(BildFehler::FilterUnbekannt { typ, zeile: r });
            }
            filter_zeile(&mut zeile, &vorzeile, bpp, typ);
            vorzeile.clone_from(&zeile);
            // Streuen: Pixel `c` des Durchgangs sitzt bei
            // (xs + c*xst, ys + r*yst).
            let y = (ys as usize).saturating_add(r.saturating_mul(yst as usize));
            let zielzeile = y.saturating_mul(n.saturating_add(1)).saturating_add(1);
            for c in 0..w {
                let x = xs as usize + c.saturating_mul(xst as usize);
                let ziel_index = zielzeile.saturating_add(x.saturating_mul(bpp));
                let quelle = c.saturating_mul(bpp);
                for k in 0..bpp {
                    if let Some(ziel_byte) = ziel.get_mut(ziel_index.saturating_add(k)) {
                        *ziel_byte = zeile.get(quelle.saturating_add(k)).copied().unwrap_or(0);
                    }
                }
            }
            pos = anfang.saturating_add(zeilenbytes);
        }
    }
    let _ = breite;
    Ok(ziel)
}

/// Macht die Zeilenfilter rueckgaengig, Zeile fuer Zeile.
///
/// Die Formeln stehen woertlich in RFC 2083 Abschnitt 6. Die Falle: `b` ist
/// die bereits ENTFILTERTE Zeile darueber, `a` ist das ENTFILTERTE Byte
/// links im selben Ziel. Wer stattdessen die Rohbytes nimmt, bekommt bei
/// Filter 2 und 4 ein Bild, das "fast" richtig aussieht — und genau das ist
/// der Fehler, den man ohne Gegenbeweis nicht findet.
fn filter_rueckgaengig(roh: &mut [u8], kopf: &Kopfdaten) -> Result<(), BildFehler> {
    let n = kopf.zeilenbreite;
    let bpp = kopf.bpp_filter.max(1);
    for y in 0..kopf.hoehe as usize {
        let start = y * (n + 1);
        let typ = roh[start];
        // `split_at_mut` trennt die vorige Zeile (unveraendert gelesen)
        // von der aktuellen (veraendert). Ohne das kann der Borrowschecker
        // nicht beweisen, dass die beiden Bereiche sich nicht ueberlappen.
        let (oben, unten) = roh.split_at_mut(start);
        let (zeile, _rest) = unten.split_at_mut(n + 1);
        let zeile = &mut zeile[1..n + 1];
        let vorzeile: &[u8] = if y == 0 {
            &[]
        } else {
            let v = (y - 1) * (n + 1) + 1;
            &oben[v..v + n]
        };
        // Der Aufruf geht an dieselbe Funktion, die auch der Test in
        // `deflate.rs` aufruft. Zwei Kopien waeren zwei Wahrheiten, und
        // die zweite wird irgendwann vergessen.
        if typ > 4 {
            return Err(BildFehler::FilterUnbekannt { typ, zeile: y });
        }
        filter_zeile(zeile, vorzeile, bpp, typ);
    }
    Ok(())
}

/// Nur fuer den Test in `deflate.rs`: macht die Rueckrechnung EINE Zeile
/// aufrufbar, ohne ein Bild drumherum zu bauen. `#[cfg(test)]` heisst: im
/// ausgelieferten Code existiert die Funktion nicht.
#[cfg(test)]
pub fn filter_testbar(zeile: &mut [u8], vorzeile: &[u8], bpp: usize, typ: u8) {
    filter_zeile(zeile, vorzeile, bpp, typ);
}

/// Rechnet eine Zeile Filter rueckgaengig. Ausgelagert aus
/// [`filter_rueckgaengig`], damit der Test sie einzeln aufrufen kann.
///
/// `vorzeile` ist die REKONSTRUIERTE Zeile darueber; fuer die ERSTE Zeile
/// ist sie leer (`&[]`), und dann gilt fuer `b` und `c` der Wert 0.
///
/// ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test
/// `zwei_zeilen_uebereinander_sind_konsistent`, 30.09.2026): Filter 2 und 3
/// griffen mit `vorzeile[i]` ohne Laengenpruefung zu. Eine PNG, deren ERSTE
/// Zeile Filter 2 benutzt, liest mit `vorzeile = &[]` dann ueber die
/// Slice-Grenze hinaus — und PANIKT. Das ist ein Absturz an einer
/// vollkommen gueltigen Datei, nicht an einer boesartigen.
fn filter_zeile(zeile: &mut [u8], vorzeile: &[u8], bpp: usize, typ: u8) {
    let n = zeile.len();
    match typ {
        0 => {}
        1 => {
            for i in bpp..n {
                zeile[i] = zeile[i].wrapping_add(zeile[i - bpp]);
            }
        }
        2 => {
            for i in 0..n {
                let b = if i < vorzeile.len() { vorzeile[i] } else { 0 };
                zeile[i] = zeile[i].wrapping_add(b);
            }
        }
        3 => {
            for i in 0..n {
                let a = if i >= bpp { zeile[i - bpp] as u16 } else { 0 };
                let b = if i < vorzeile.len() {
                    vorzeile[i] as u16
                } else {
                    0
                };
                zeile[i] = zeile[i].wrapping_add(((a + b) / 2) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let a = if i >= bpp { zeile[i - bpp] } else { 0 };
                let b = if i < vorzeile.len() { vorzeile[i] } else { 0 };
                let c = if i >= bpp && i < vorzeile.len() {
                    vorzeile[i - bpp]
                } else {
                    0
                };
                zeile[i] = zeile[i].wrapping_add(paeth(a, b, c));
            }
        }
        _ => {}
    }
}

/// Skaliert einen Grauwert der Tiefe `d` auf 0..=255.
///
/// Nicht `v << (8-d)`: das waere bei 2 Bit `v*64` und damit falsch (0, 64,
/// 128, 192 statt 0, 85, 170, 255). Der Maximalwert muss auf 255 laufen,
/// sonst sind 4-Bit-Graustufen im Browser zu dunkel.
fn skalieren_grau(v: u8, tiefe: u8) -> u8 {
    match tiefe {
        1 => {
            if v != 0 {
                255
            } else {
                0
            }
        }
        2 => (v as u16 * 255 / 3) as u8,
        4 => (v as u16 * 255 / 15) as u8,
        _ => v,
    }
}

/// Liest bei Tiefe < 8 den `i`-ten Wert einer Zeile (0-basiert).
fn bitwert(zeile: &[u8], index: usize, tiefe: u8) -> u8 {
    match tiefe {
        1 => (zeile[index / 8] >> (7 - (index % 8))) & 1,
        2 => (zeile[index / 4] >> (6 - 2 * (index % 4))) & 3,
        4 => (zeile[index / 2] >> (4 - 4 * (index % 2))) & 15,
        _ => zeile[index],
    }
}

/// Entpackt die entfilterten Zeilen in RGBA8.
#[allow(clippy::too_many_arguments)]
fn expandieren(
    roh: &[u8],
    kopf: &Kopfdaten,
    palette: &[[u8; 3]],
    trns_palette: &[u8],
    trns_grau: Option<u16>,
    trns_rgb: Option<[u16; 3]>,
    bild: &mut Bild,
) {
    let breite = kopf.breite as usize;
    let n = kopf.zeilenbreite;
    if kopf.tiefe == 16 {
        expandieren_16(roh, kopf, trns_grau, trns_rgb, bild);
        return;
    }
    for y in 0..kopf.hoehe as usize {
        let zeile = &roh[y * (n + 1) + 1..y * (n + 1) + 1 + n];
        for x in 0..breite {
            let p = match kopf.farbtyp {
                FARBTYP_GRAU => {
                    let roh_wert = if kopf.tiefe == 8 {
                        zeile[x] as u16
                    } else {
                        skalieren_grau(bitwert(zeile, x, kopf.tiefe), kopf.tiefe) as u16
                    };
                    // tRNS nennt den Grauwert, der durchsichtig wird.
                    let a = match trns_grau {
                        Some(t) if roh_wert == t => 0,
                        _ => 255,
                    };
                    [roh_wert as u8, roh_wert as u8, roh_wert as u8, a]
                }
                FARBTYP_GRAU_ALPHA => {
                    let g = zeile[x * 2];
                    [g, g, g, zeile[x * 2 + 1]]
                }
                FARBTYP_PALETTE => {
                    let idx = bitwert(zeile, x, kopf.tiefe) as usize;
                    // Ein Index ausserhalb der Palette ist ein Fehler, aber
                    // hier ist der Fehler schon behandelt: `bitwert` liefert
                    // hoechstens 255, und `palette.get` liefert None. Statt
                    // zu panicen: schwarz, wie es die Spezifikation fuer
                    // Fehlindizes nahelegt.
                    let rgb = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    let a = trns_palette.get(idx).copied().unwrap_or(255);
                    [rgb[0], rgb[1], rgb[2], a]
                }
                FARBTYP_RGB => {
                    let a = match trns_rgb {
                        Some([tr, tg, tb]) => {
                            let passt = zeile[x * 3] as u16 == tr
                                && zeile[x * 3 + 1] as u16 == tg
                                && zeile[x * 3 + 2] as u16 == tb;
                            if passt { 0 } else { 255 }
                        }
                        None => 255,
                    };
                    [zeile[x * 3], zeile[x * 3 + 1], zeile[x * 3 + 2], a]
                }
                _ => [
                    zeile[x * 4],
                    zeile[x * 4 + 1],
                    zeile[x * 4 + 2],
                    zeile[x * 4 + 3],
                ],
            };
            bild.rgba.extend_from_slice(&p);
        }
    }
}

/// 16-bit samples (big endian) rounded to 8 bits; `tRNS` compares the full
/// 16-bit values.
fn expandieren_16(roh: &[u8], kopf: &Kopfdaten, trns_grau: Option<u16>, trns_rgb: Option<[u16; 3]>, bild: &mut Bild) {
    let breite = kopf.breite as usize;
    let n = kopf.zeilenbreite;
    let kanaele = match kopf.farbtyp {
        FARBTYP_GRAU => 1,
        FARBTYP_GRAU_ALPHA => 2,
        FARBTYP_RGB => 3,
        _ => 4,
    };
    let acht = |v: u16| ((u32::from(v) * 255 + 32767) / 65535) as u8;
    for y in 0..kopf.hoehe as usize {
        let zeile = &roh[y * (n + 1) + 1..y * (n + 1) + 1 + n];
        for x in 0..breite {
            let s = |k: usize| {
                let i = (x * kanaele + k) * 2;
                u16::from_be_bytes([zeile[i], zeile[i + 1]])
            };
            let p = match kopf.farbtyp {
                FARBTYP_GRAU => {
                    let g = s(0);
                    let a = if trns_grau == Some(g) { 0 } else { 255 };
                    [acht(g), acht(g), acht(g), a]
                }
                FARBTYP_GRAU_ALPHA => {
                    let g = acht(s(0));
                    [g, g, g, acht(s(1))]
                }
                FARBTYP_RGB => {
                    let (r, g, b) = (s(0), s(1), s(2));
                    let a = if trns_rgb == Some([r, g, b]) { 0 } else { 255 };
                    [acht(r), acht(g), acht(b), a]
                }
                _ => [acht(s(0)), acht(s(1)), acht(s(2)), acht(s(3))],
            };
            bild.rgba.extend_from_slice(&p);
        }
    }
}
