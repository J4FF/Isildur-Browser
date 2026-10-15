//! WebP SCHREIBEN — verlustfrei (VP8L).
//!
//! Auftrag K1: „WebP verlustfrei (VP8L)". Die Abnahme ist wie bei PNG:
//! `webp_lesen` von `narsil-image` muss das Ergebnis **bitgleich**
//! zurueckgeben.
//!
//! # Warum der Enkeder gegen den PROJEKT-Dekoder gebaut ist
//!
//! VP8L hat eine Spezifikation (der IETF-Entwurf) und eine
//! Implementierung (libwebp), und an mehreren Stellen weichen die
//! voneinander ab. Der Leser in diesem Projekt ist ein Zeile-fuer-Zeile-
//! Port von libwebps `vp8l_dec.c` (siehe `narsil-image/src/vp8l.rs:1-25`),
//! und der Auftrag sagt „Rundreise Dekoder(Enkoder(x))". Also gilt
//! **libwebp**, nicht der Entwurf. Wo beide unterscheiden, steht das hier
//! und nicht irgendwo im Leser.
//!
//! # Der kleinste gueltige VP8L-Strom
//!
//! ```text
//! 0x2F                       Signatur (8 Bit)
//! 14 Bit  (Breite  - 1)      max 16384
//! 14 Bit  (Hoehe   - 1)      max 16384
//!  1 Bit  alpha_used         nur ein Hinweis
//!  3 Bit  Version = 0
//!  1 Bit  0                   keine Transformation
//!  1 Bit  0                   kein Farb-Cache
//!  1 Bit  0                   keine Meta-Praefixcodes
//!  5 x Huffman-Code           gruen, rot, blau, alpha, distanz
//!  Bildstrom                 Literal / Rueckverweis / Cache-Code
//! ```
//!
//! Fuenf Codes, kein Transform, kein Cache: das ist die kleinste Form, die
//! libwebp annimmt, und damit der Punkt, an dem man anfangen muss.
//!
//! # Warum der Bildstrom LSB zuerst schreibt
//!
//! VP8L-Bitstroeme sind **LSB zuerst** (`Bits::r` in
//! `narsil-image/src/vp8l.rs:110`), JPEG MSB zuerst, DEFLATE LSB zuerst mit
//! eigener Bytegruppe. Wer zwei davon vermischt, erzeugt Datenmüll, der
//! erst beim zweiten Bild auffaellt.

use crate::Bild;
use crate::fehler::KonvertFehler;
use crate::grenzen;
use crate::huffman::{self, Praefixcode};

/// Die RIFF-Signatur.
const RIFF: &[u8; 4] = b"RIFF";
/// Die WebP-Form-Signatur (an Offset 8).
const WEBP: &[u8; 4] = b"WEBP";
/// Die Chunk-Signatur von VP8L.
const VP8L: &[u8; 4] = b"VP8L";
/// Die VP8L-Signatur (erstes Byte der Nutzlast).
const VP8L_SIGNATUR: u8 = 0x2F;

/// Zahlen aus der Spezifikation (dieselben wie im Leser).
const LIT_LITERALE: usize = 256;
const LIT_LAENGEN: usize = 24;
const LIT_DISTANZEN: usize = 40;
/// Hoechste Laenge eines Rueckverweises (aus der Laengen-Codierung).
const MAX_LAENGE: usize = 4096;
/// Zahl der Leseplaetze fuer die „Code-Laengen-Codes".
const MAX_CODE_LAENGEN: usize = 19;
/// Hoechste Laenge eines Code-Laengen-Codes: die Felder sind 3 Bit breit,
/// also hoechstens 7.
const MAX_VORSTUFEN_LAENGE: u8 = 7;

/// Die Reihenfolge der Code-Laengen (`kCodeLengthCodeOrder`).
const CODE_LAENGEN_ORDNUNG: [usize; MAX_CODE_LAENGEN] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// `kCodeToPlane` aus libwebp — die Distanzcodes 4..39.
///
/// Wird von dieser Crate noch NICHT geschrieben (siehe
/// [`distanzcode_eins`]), aber hier vollstaendig abgelegt, damit die
/// naechste Stufe (echtes LZ77 ueber Zeilengrenzen) die Zahl nicht
/// aus libwebp tippen muss. Nicht verwendet -> `dead_code`; das ist
/// Absicht und im Test `vp8l_distanzcode_tabelle_stimmt_genau` belegt.
#[allow(dead_code)]
pub const K_CODE_TO_PLANE: [u8; 120] = [
    0x18, 0x07, 0x17, 0x19, 0x28, 0x06, 0x27, 0x29, 0x16, 0x1a, 0x26, 0x2a, 0x38, 0x05, 0x37, 0x39,
    0x15, 0x1b, 0x36, 0x3a, 0x25, 0x2b, 0x48, 0x04, 0x47, 0x49, 0x14, 0x1c, 0x35, 0x3b, 0x46, 0x4a,
    0x24, 0x2c, 0x58, 0x45, 0x4b, 0x34, 0x3c, 0x03, 0x57, 0x59, 0x13, 0x1d, 0x56, 0x5a, 0x23, 0x2d,
    0x44, 0x4c, 0x55, 0x5b, 0x33, 0x3d, 0x68, 0x02, 0x67, 0x69, 0x12, 0x1e, 0x66, 0x6a, 0x22, 0x2e,
    0x54, 0x5c, 0x43, 0x4d, 0x65, 0x6b, 0x32, 0x3e, 0x78, 0x01, 0x77, 0x79, 0x53, 0x5d, 0x11, 0x1f,
    0x64, 0x6c, 0x42, 0x4e, 0x76, 0x7a, 0x21, 0x2f, 0x75, 0x7b, 0x31, 0x3f, 0x63, 0x6d, 0x52, 0x5e,
    0x00, 0x74, 0x7c, 0x41, 0x4f, 0x10, 0x20, 0x62, 0x6e, 0x30, 0x73, 0x7d, 0x51, 0x5f, 0x40, 0x72,
    0x7e, 0x61, 0x6f, 0x50, 0x71, 0x7f, 0x60, 0x70,
];

/// Schreibt ein verlustfreies WebP.
///
/// # Fehler
/// - [`KonvertFehler::Vp8lKanteZuGross`] wenn eine Kante nicht in 14 Bit passt.
/// - [`KonvertFehler::Vp8lZuGross`] wenn das Bild mehr Pixel hat als der
///   Dekoder erlaubt.
/// - Siehe [`crate::grenzen::pruefe_bild`].
pub fn schreiben_verlustfrei(bild: &Bild) -> Result<Vec<u8>, KonvertFehler> {
    grenzen::pruefe_bild(bild)?;
    if bild.breite > 16384 || bild.hoehe > 16384 {
        return Err(KonvertFehler::Vp8lKanteZuGross {
            breite: bild.breite,
            hoehe: bild.hoehe,
        });
    }
    let pixel = bild.breite as u64 * bild.hoehe as u64;
    if pixel > grenzen::MAX_VP8L_PIXEL {
        return Err(KonvertFehler::Vp8lZuGross {
            pixel,
            grenze: grenzen::MAX_VP8L_PIXEL,
        });
    }

    // --- Alpha-Benutzergrenze: nur ein Hinweis, aber der wird von
    // --- Werkzeugen benutzt, um „hat Alpha" zu melden. Also korrekt setzen.
    let mut alpha_benutzt = false;
    let n = bild.breite as usize * bild.hoehe as usize;
    for p in 0..n {
        if bild.rgba[p * 4 + 3] != 255 {
            alpha_benutzt = true;
            break;
        }
    }

    // --- Bildstrom: erst die Symbole zaehlen, dann die Codes bauen ---
    //
    // Zwei Durchgaenge ueber die Pixel: der erste zaehlt, der zweite
    // schreibt. Das ist die einzige Moeglichkeit, einen Huffman-Code aus
    // den Daten zu bauen — und der Unterschied zu festen Tabellen ist bei
    // Bildern mit wenigen Farben gross (ein Logo mit 8 Farben kostet so
    // fast nichts).
    let (symbole, rueckverweise) = symbole_zaehlen(bild);

    let g_alphabet = LIT_LITERALE + LIT_LAENGEN; // 280
    let mut g_gewichte = vec![0u32; g_alphabet];
    let mut r_gewichte = vec![0u32; LIT_LITERALE];
    let mut b_gewichte = vec![0u32; LIT_LITERALE];
    let mut a_gewichte = vec![0u32; LIT_LITERALE];
    let mut d_gewichte = vec![0u32; LIT_DISTANZEN];
    for s in &symbole {
        match s.art {
            SymbolArt::Literal(argb) => {
                // Nur die Helligkeit zaehlt in den GRUEN-Baum; Rot, Blau
                // und Alpha haben eigene Baeume (Leser `gruppe_lesen`,
                // vp8l.rs:337 — dort ist RED=1, BLUE=2, ALPHA=3).
                g_gewichte[((argb >> 8) & 0xFF) as usize] += 1;
                r_gewichte[((argb >> 16) & 0xFF) as usize] += 1;
                b_gewichte[(argb & 0xFF) as usize] += 1;
                a_gewichte[((argb >> 24) & 0xFF) as usize] += 1;
            }
            SymbolArt::Rueckverweis {
                laenge,
                distanz_code,
            } => {
                g_gewichte[LIT_LITERALE + laenge_code(laenge).0 as usize] += 1;
                d_gewichte[distanz_code as usize] += 1;
            }
        }
    }
    // Der Rueckverweiszaehler gehoert in den Bericht, nicht in die Datei —
    // aber er ist die Zahl, die die Groesse erklaert.
    let _ = rueckverweise;
    // Der Distanzbaum muss mindestens EIN Symbol haben, auch wenn das Bild
    // keinen einzigen Rueckverweis enthaelt (etwa 1x1 oder 2x1 Pixel).
    //
    // GEMESSEN 02.10.2026: `k1_ausgabe` auf ein 1x1-Bild brach ab mit
    // „Huffman-Alphabet 'vp8l distanz' hat kein einziges Symbol". Der
    // Leser verlangt aber fuenf Baeume, und `Code::neu` weist einen Baum
    // ohne Symbol zurueck. Also wird der Distanzcode 0 (Distanz 1) als
    // vorbereitet eingetragen — er wird nie geschrieben, kostet aber
    // 2 Bit im Kopf.
    if d_gewichte.iter().all(|&g| g == 0) {
        d_gewichte[0] = 1;
    }

    let h_g = Praefixcode::baue(&g_gewichte, huffman::MAX_LAENGE_VP8L, "vp8l gruen")?;
    let h_r = Praefixcode::baue(&r_gewichte, huffman::MAX_LAENGE_VP8L, "vp8l rot")?;
    let h_b = Praefixcode::baue(&b_gewichte, huffman::MAX_LAENGE_VP8L, "vp8l blau")?;
    let h_a = Praefixcode::baue(&a_gewichte, huffman::MAX_LAENGE_VP8L, "vp8l alpha")?;
    let h_d = Praefixcode::baue(&d_gewichte, huffman::MAX_LAENGE_VP8L, "vp8l distanz")?;

    // --- Nutzlast ---
    // M4: Groesse vorab. Vier Literale je Pixel (gruen 15 + 3 x 8 Bit im
    // guenstigsten Fall) ist die harte obere Grenze.
    let mut grenze = 32 + grenzen::MAX_VP8L_KOPF_BYTES;
    grenze = grenze.checked_add(pixel as usize * 5).ok_or(ueberlauf())?;
    grenzen::pruefe_laenge(
        grenze,
        grenzen::MAX_SPEICHER,
        "VP8L-Ausgabe vorab reserviert",
    )?;
    let mut b = BitSchreiber::neu(grenze);
    b.ohm(VP8L_SIGNATUR as u32, 8);
    b.ohm(bild.breite - 1, 14);
    b.ohm(bild.hoehe - 1, 14);
    b.bit(u32::from(alpha_benutzt));
    b.ohm(0, 3); // Version
    // Keine Transformation.
    b.bit(0);
    // Kein Farb-Cache.
    b.bit(0);
    // Keine Meta-Praefixcodes.
    b.bit(0);
    // Die fuenf Codes, in dieser Reihenfolge: gruen, rot, blau, alpha,
    // distanz. GEMESSEN: eine andere Reihenfolge verschiebt den ganzen
    // Bildstrom um ein Bit je Code (Leser `gruppe_lesen`, vp8l.rs:337).
    //
    // `NARSIL_KONVERT_VP8L_DUMP=1` schreibt die Kopfdaten auf stderr: der
    // Enkoder sagt, wie viele Bit er fuer welchen Baum geschrieben hat.
    // Ohne das muss man beim Fehlernachstellen raten, an welcher Stelle
    // der Datenstrom auseinanderlaeuft — und raten ist bei einem
    // Bitformat genau die Sache, die man nicht tun soll.
    let dump = std::env::var_os("NARSIL_KONVERT_VP8L_DUMP").is_some();
    let mut stand = b.bit_stand();
    code_schreiben(&mut b, &h_g, g_alphabet)?;
    if dump {
        eprintln!(
            "VP8L gruen  : alphabet={g_alphabet:3} {} (Bit {stand}..{})",
            kopf_text(&h_g),
            b.bit_stand()
        );
    }
    for (name, h, alphabet) in [
        ("rot   ", &h_r, LIT_LITERALE),
        ("blau  ", &h_b, LIT_LITERALE),
        ("alpha ", &h_a, LIT_LITERALE),
        ("distanz", &h_d, LIT_DISTANZEN),
    ] {
        stand = b.bit_stand();
        code_schreiben(&mut b, h, alphabet)?;
        if dump {
            eprintln!(
                "VP8L {name}: alphabet={alphabet:3} {} (Bit {stand}..{})",
                kopf_text(h),
                b.bit_stand()
            );
        }
    }

    // --- Die Symbole schreiben ---
    for s in &symbole {
        match s.art {
            SymbolArt::Literal(argb) => {
                // Reihenfolge: GRUEN, ROT, BLAU, ALPHA. GEMESSEN: rot zuerst
                // zu lesen zerlegt jede Datei ab dem zweiten Pixel.
                b.code(&h_g, ((argb >> 8) & 0xFF) as u16);
                b.code(&h_r, ((argb >> 16) & 0xFF) as u16);
                b.code(&h_b, (argb & 0xFF) as u16);
                b.code(&h_a, ((argb >> 24) & 0xFF) as u16);
            }
            SymbolArt::Rueckverweis {
                laenge,
                distanz_code,
            } => {
                schreibe_rueckverweis(&mut b, &h_g, &h_d, laenge, distanz_code);
            }
        }
    }
    let nutzlast = b.fertig()?;

    // --- RIFF-Rahmen ---
    let chunk_gerade = nutzlast.len() + (nutzlast.len() & 1);
    let riff_groesse = 4 + 8 + chunk_gerade; // "WEBP" + Chunkkopf + Nutzlast
    grenzen::pruefe_laenge(riff_groesse + 8, grenzen::MAX_SPEICHER, "WebP-Datei")?;
    let mut out: Vec<u8> = Vec::with_capacity(riff_groesse + 8);
    out.extend_from_slice(RIFF);
    out.extend_from_slice(&(riff_groesse as u32).to_le_bytes());
    out.extend_from_slice(WEBP);
    out.extend_from_slice(VP8L);
    out.extend_from_slice(&(nutzlast.len() as u32).to_le_bytes());
    out.extend_from_slice(&nutzlast);
    // RIFF verlangt eine gerade Chunkgroesse: ein ungerader Chunk wird
    // mit einem Fuellbyte abgeschlossen. Fehlt er, verschiebt sich bei
    // mehreren Chunks alles um eins Byte — und libwebp lehnt die Datei ab.
    if nutzlast.len() & 1 == 1 {
        out.push(0);
    }
    Ok(out)
}

/// Wie viele Bytes die Datei wird (fuer die Menuebeschriftung in K4).
pub fn groesse(bild: &Bild) -> Result<usize, KonvertFehler> {
    Ok(schreiben_verlustfrei(bild)?.len())
}

// =========================================================================
// Die Bildstrom-Symbole
// =========================================================================

/// Was ein Eintrag im Bildstromtransport ist.
///
/// Der Code im GRUEN-Baum (0..255 Literal, 256..279 Laengenpraefix) wird
/// beim Schreiben aus `art` berechnet, nicht gespeichert: zwei
/// Darstellungen derselben Sache waeren eine Fehlerquelle.
#[derive(Debug, Clone, Copy)]
struct Symbol {
    art: SymbolArt,
}

#[derive(Debug, Clone, Copy)]
enum SymbolArt {
    /// Ein Farbpixel: AARRGGBB.
    Literal(u32),
    /// Ein Rueckverweis: die Laenge in Pixeln und der Distanzcode.
    ///
    /// Die Laenge wird gespeichert, nicht der daraus berechnete
    /// Laengen-Code: so benutzen Zaehl- und Schreibphase dieselbe
    /// Funktion ([`laenge_code`]) und koennen nicht auseinanderlaufen.
    Rueckverweis { laenge: usize, distanz_code: u16 },
}

/// Zerlegt das Bild in Symbole und zaehlt sie dabei.
///
/// Drei Schritte, und jeder ist eine Entscheidung:
///
/// 1. **Lauflangen zuerst.** Ein Streifen gleicher Farbe wird ein
///    Rueckverweis. Das ist der Grund, warum WebP verlustfrei klein wird:
///    ein 640x480-Screenshot mit grossen Flaechen kostet fast nichts.
/// 2. **Rueckverweise pruefen.** Nur wenn die gewuenschte Distanz im
///    Bild auch wirklich erreichbar ist (der Leser lehnt
///    `distanz > pos` ab), wird sie geschrieben.
/// 3. **Alles andere ist ein Literal.** Vier Huffman-Codes, ein Pixel.
fn symbole_zaehlen(bild: &Bild) -> (Vec<Symbol>, u64) {
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;
    let n = breite * hoehe;
    let mut raus: Vec<Symbol> = Vec::with_capacity(n / 2 + 16);
    let mut rueckverweise = 0u64;
    let mut i = 0usize;
    let mut vorletztes: Option<u32> = None;
    while i < n {
        let p = i * 4;
        let argb = ((bild.rgba[p + 3] as u32) << 24)
            | ((bild.rgba[p] as u32) << 16)
            | ((bild.rgba[p + 1] as u32) << 8)
            | bild.rgba[p + 2] as u32;

        // --- Lauflange, wenn dasselbe Bild schon kam ---
        if vorletztes == Some(argb) && i > 0 {
            let mut lauf = 1usize;
            while i + lauf < n {
                let q = (i + lauf) * 4;
                let a = ((bild.rgba[q + 3] as u32) << 24)
                    | ((bild.rgba[q] as u32) << 16)
                    | ((bild.rgba[q + 1] as u32) << 8)
                    | bild.rgba[q + 2] as u32;
                if a != argb {
                    break;
                }
                lauf += 1;
            }
            // Distanz 1 = „das Pixel direkt davor". Code 0 ergibt nach
            // `aus_code` genau diese Distanz, bei jeder Bildbreite.
            let mut rest = lauf;
            while rest > 0 {
                let l = rest.min(MAX_LAENGE);
                raus.push(Symbol {
                    art: SymbolArt::Rueckverweis {
                        laenge: l,
                        distanz_code: distanzcode_eins(),
                    },
                });
                rueckverweise += 1;
                rest -= l;
            }
            i += lauf;
            vorletztes = Some(argb);
            continue;
        }
        raus.push(Symbol {
            art: SymbolArt::Literal(argb),
        });
        vorletztes = Some(argb);
        i += 1;
    }
    (raus, rueckverweise)
}

/// Der Distanzcode fuer „eine Distanz zurueck" — bei JEDER Bildbreite 1.
///
/// # ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
/// `webp_verlustfrei_bitgleich`: „Rueckverweis zeigt vor den Bildanfang")
///
/// Hier stand vorher 0. Das ist plausibel — die Formel `aus_code` gibt fuer
/// die Codes 0..3 einfach `code + 1` zurueck, und man denkt „Code 0 heisst
/// Distanz 1". Der Leser macht aber ZWEI Schritte:
///
/// ```text
///   distanz = distanz_in_punkte(aus_code(dsymbol), breite)
///                        ^^^^^^^^^^^^^^  0..3 -> 1..4
///                                     ^^^^^^^^^^^^^^^^^^^^^^
///                 K_CODE_TO_PLANE[plane - 1], dann (x)*breite + (8 - y)
/// ```
///
/// GEMESSEN (unabhaengig nachgerechnet, Bildbreiten 1/2/4/37/320):
///
///   dsymbol 0 -> plane 1 -> K_CODE_TO_PLANE[0] = 0x18 -> 1*breite + 0
///              = die GANZE ZEILENBREITE
///   dsymbol 1 -> plane 2 -> K_CODE_TO_PLANE[1] = 0x07 -> 0*breite + 1
///              = 1, bei jeder Breite
///
/// Also ist 1 der Code fuer Distanz 1, und 0 ist die Distanz „eine Zeile
/// zurueck" — fuer LZ77 ueber Zeilengrenzen genau richtig, fuer einen
/// Lauflauf vollkommen falsch.
///
/// DER FEHLER HAT SICH GETARNT: bei einem 1 Pixel breiten Bild ist „die
/// ganze Zeilenbreite" = 1, also stimmte es. Genau darum haben 1x1, 2x1
/// und 1x9 funktioniert und 37x29 nicht.
#[must_use]
pub fn distanzcode_eins() -> u16 {
    1
}

/// Der Distanzcode fuer „eine Zeile zurueck" (also `breite` Punkte).
///
/// GEMESSEN: dsymbol 0. Nur sinnvoll, wenn `pos >= breite`, also fruehestens
/// nach der zweiten Zeile — der Leser lehnt sonst `distanz > pos` ab
/// (`vp8l.rs:452`).
#[must_use]
pub fn distanzcode_zeile() -> u16 {
    0
}

/// Der Laengen-Code fuer eine Laenge von 1..=4096, mit seinen Zusatzbits.
///
/// Umkehrung von `aus_code` (vp8l.rs:544):
/// `code < 4 -> code+1`; sonst `extra = (code-2)>>1` und
/// `Laenge = ((2 + (code&1)) << extra) + bits(extra) + 1`.
///
/// Weil `LIT_LAENGEN = 24`, sind nur die Codes 0..23 zulaessig, und die
/// groesste Laenge ist 4096 (bei `extra = 10`).
#[must_use]
pub fn laenge_code(laenge: usize) -> (u16, u32, u32) {
    let l = laenge.clamp(1, MAX_LAENGE);
    if l <= 4 {
        return ((l - 1) as u16, 0, 0);
    }
    let n = (l - 1) as u32;
    // Das groesste `extra` mit `2 * 2^extra <= n` ist `floor(log2(n)) - 1`.
    let e = (31 - n.leading_zeros()).max(2) - 1;
    let basis2 = 2u32 << e;
    let basis3 = 3u32 << e;
    let (p, versatz) = if n >= basis3 {
        (1u32, basis3)
    } else {
        (0u32, basis2)
    };
    let code = 2 * e + 2 + p;
    debug_assert!(
        (code as usize) < LIT_LAENGEN,
        "Laengencode {code} passt nicht in LIT_LAENGEN = {LIT_LAENGEN}"
    );
    (code as u16, e, n - versatz)
}

/// Schreibt einen Rueckverweis.
///
/// ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
/// `webp_verlustfrei_bitgleich`: „Rueckverweis zeigt vor den Bildanfang").
/// Die Reihenfolge der drei Schreibvorgaenge war: Laengencode,
/// Distanzcode, ZUSAETZBITS der Laenge. Der Leser macht es anders
/// (`narsil-image/src/vp8l.rs:440`):
///
/// ```text
///   s        = gruen.dec()                       // Laengenpraefix
///   laenge   = aus_code(s - 256, b)             // liest die ZUSAETZBITS
///   dsymbol  = gr[4].dec()                       // ERST DANACH der Distanzcode
///   distanz  = distanz_in_punkte(aus_code(dsymbol, b), breite)
/// ```
///
/// Die Zusatzbits der Laenge stehen also ZWISCHEN den beiden Codes.
/// Beide Codes sind bei Distanz 1 einbitig, deshalb hat es bei einem
/// Lauflauf mit Laenge 1 (keine Zusatzbits) nicht aufgefallen — und
/// `webp_lauflangen_als_rueckverweis` war der einzige Test mit einem
/// echten Lauf und kam durch. Der 4096er-Lauf mit Zusatzbits traf es.
fn schreibe_rueckverweis(
    b: &mut BitSchreiber,
    h_g: &Praefixcode,
    h_d: &Praefixcode,
    laenge: usize,
    distanz_code: u16,
) {
    let (code, _e, wert) = laenge_code(laenge);
    // 1. Der Laengenpraefix im gruenen Baum.
    b.code(h_g, LIT_LITERALE as u16 + code);
    // 2. Die Zusatzbits der Laenge.
    let extra = if code < 4 { 0 } else { (code - 2) >> 1 };
    if extra > 0 {
        b.ohm(wert, u32::from(extra));
    }
    // 3. Der Distanzcode.
    b.code(h_d, distanz_code);
}

// =========================================================================
// Bit-Schreiber (LSB zuerst)
// =========================================================================

/// Schreibt Bits **LSB zuerst** in Bytes.
///
/// Nicht zu verwechseln mit dem JPEG-Schreiber (MSB zuerst). Der
/// Unterschied ist genau eine Zeile — und der haeufigste Grund, warum ein
/// VP8L-Bild „schwarze Muellstreifen" bekommt.
struct BitSchreiber {
    out: Vec<u8>,
    bitpuffer: u32,
    bitzahl: u32,
}

impl BitSchreiber {
    fn neu(grenze: usize) -> Self {
        BitSchreiber {
            out: Vec::with_capacity(grenze.min(1 << 20)),
            bitpuffer: 0,
            bitzahl: 0,
        }
    }

    /// Wie viele Bit sind bisher geschrieben? Nur fuer die Diagnose.
    fn bit_stand(&self) -> usize {
        self.out.len() * 8 + self.bitzahl as usize
    }

    fn ohm(&mut self, wert: u32, anzahl: u32) {
        for i in 0..anzahl {
            self.bit((wert >> i) & 1);
        }
    }

    fn bit(&mut self, wert: u32) {
        self.bitpuffer |= (wert & 1) << self.bitzahl;
        self.bitzahl += 1;
        if self.bitzahl == 8 {
            self.out.push(self.bitpuffer as u8);
            self.bitpuffer = 0;
            self.bitzahl = 0;
        }
    }

    /// Schreibt das Codewort eines Symbols.
    ///
    /// Die Vorsicht `einzeln` ist wichtig: hat ein Baum nur EIN Symbol,
    /// kostet das Lesen **null** Bit (`BuildHuffmanTable` in
    /// `huffman_utils.c`, Leser vp8l.rs:169). Wer hier trotzdem ein Bit
    /// schreibt, verschiebt den ganzen Bildstrom um ein Bit je Pixel —
    /// das Bild ist dann ab der zweiten Zeile Muell.
    /// Schreibt das Codewort eines Symbols.
    ///
    /// **MSB ZUERST.** Der Leser sammelt den Code als `c = c * 2 + bit`
    /// (`narsil-image/src/vp8l.rs:213`), das erste gelesene Bit ist also
    /// das hoechstwertige. Ein Code „10" wird also als Bit 1 dann Bit 0
    /// geschrieben.
    ///
    /// ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 mit
    /// `NARSIL_KONVERT_VP8L_DUMP=1` und einem Python-Abbild des Lesers).
    /// Hier stand vorher `self.ohm(wert, laenge)`, und `ohm` schreibt
    /// **LSB zuerst**. Das ist fuer die EINFACHEN Felder richtig — der
    /// Leser liest ein Feld mit `Bits::r(n)` und legt das erste Bit in das
    /// NIEDRIGSTE Bit (`vp8l.rs:110`) —, fuer Huffman-Codes aber genau
    /// verkehrt herum.
    ///
    /// GEMESSEN: Bei Laenge 1 faellt der Unterschied nicht auf (ein
    /// einbitiges Codewort hat kein „letztes" Bit), deshalb haben 1x1,
    /// 2x1 und 1x9 funktioniert. Ab der ersten Laenge 2 wurde jedes
    /// Codewort gespiegelt gelesen: der Leser bekam „01" statt „10" und
    /// traf das Nachbar-Symbol. Beobachtet als eine Kette von
    /// Bitversetzungen: der Alpha-Baum endete beim Leser bei Bit 1215,
    /// beim Enkoder bei 1216.
    ///
    /// Die Lehre, die ich notiere, weil sie viermal im Projekt
    /// vorkommt: `ohm` und `code` sind VERSCHIEDENE Reihenfolgen, und
    /// eine Datei kann richtig und trotzdem unlesbar sein. Bei
    /// Laenge 1 faellt der Unterschied nicht auf — das ist die Falle.
    ///
    /// Und die andere Vorsicht `einzeln`: hat ein Baum nur EIN Symbol,
    /// kostet das Lesen **null** Bit (`BuildHuffmanTable` in
    /// `huffman_utils.c`, Leser vp8l.rs:169). Wer hier trotzdem ein Bit
    /// schreibt, verschiebt den ganzen Bildstrom um ein Bit je Pixel —
    /// das Bild ist dann ab der zweiten Zeile Muell.
    fn code(&mut self, code: &Praefixcode, symbol: u16) {
        if code.benutzt == 1 {
            return;
        }
        let laenge = code.laenge[symbol as usize];
        if laenge == 0 {
            // Darf nie vorkommen: alle benutzten Symbole haben Laenge > 0.
            return;
        }
        let wert = code.code[symbol as usize];
        // MSB zuerst: von der letzten Stelle zur ersten.
        for i in (0..u32::from(laenge)).rev() {
            self.bit((wert >> i) & 1);
        }
    }

    /// Schliesst den Strom ab: die letzten Bit mit Nullen fuellen.
    ///
    /// Nullen, nicht Einsen wie bei JPEG: der VP8L-Leser liest ueber das
    /// Ende hinaus mit Nullen weiter (vp8l.rs:117), und ein Fuellen mit
    /// Einsen wuerde die letzten Codes verschieben.
    fn fertig(mut self) -> Result<Vec<u8>, KonvertFehler> {
        if self.bitzahl > 0 {
            self.out.push(self.bitpuffer as u8);
        }
        grenzen::pruefe_laenge(self.out.len(), grenzen::MAX_SPEICHER, "VP8L-Nutzlast")?;
        Ok(self.out)
    }
}

// =========================================================================
// Die Code-Laengen schreiben
// =========================================================================

/// Schreibt die Laengen eines Praefixcodes in der Form, die der Leser
/// erwartet.
///
/// Das ist der Teil, an dem VP8L-Enkoder am haeufigsten scheitern, weil
/// er drei Ebenen hat:
///
/// 1. `1 Bit = 0` → „normaler Code" (der einfache Code waere nur 1 oder 2
///    Symbole und taeugt hier nichts).
/// 2. `4 Bit` → Zahl der geschriebenen Laengen minus 4 (4..19).
/// 3. Die Laengen der „Code-Laengen-Codes" selbst, je 3 Bit, in der
///    Reihenfolge [`CODE_LAENGEN_ORDNUNG`].
///
/// Danach kommt der eigentliche Laengenstrom, kodiert mit dem
/// Code-Laengen-Code und mit den drei Wiederholungscodes 16/17/18.
fn code_schreiben(
    b: &mut BitSchreiber,
    code: &Praefixcode,
    alphabet: usize,
) -> Result<(), KonvertFehler> {
    // --- Welche Symbole braucht der Code-Laengen-Code? ---
    // Symbole, die im Laengenstrom tatsaechlich vorkommen. Plus die
    // Wiederholungscodes, falls sie noetig werden (das entscheidet die
    // Aufraeumlogik unten).
    let plan = laengen_plan(&code.laenge, alphabet);
    let mut benutzt = [0u32; MAX_CODE_LAENGEN];
    for &c in &plan {
        benutzt[c.symbol as usize] += 1;
    }
    // Der Code-Laengen-Code selbst: gewichtet nach Haeufigkeit, Laenge
    // hoechstens 7 (die Felder sind 3 Bit).
    let mut vorstufe_gewichte = benutzt;
    if vorstufe_gewichte.iter().sum::<u32>() == 0 {
        vorstufe_gewichte[0] = 1;
    }
    let mut vorstufe = Praefixcode::baue(
        &vorstufe_gewichte,
        MAX_VORSTUFEN_LAENGE,
        "vp8l code-laengen",
    )?;
    if vorstufe.benutzt == 1 {
        // GEFAHR: ein Baum mit einem Symbol kostet beim Lesen NULL Bit.
        // Wenn wir jetzt mehr als ein Code schreiben, verschiebt sich der
        // Strom. Also ein zweites Symbol mit Laenge 1 dazugeben — die
        // Kraftsumme 2 * 2^-1 = 1 ist genau vollstaendig, also gueltig.
        let einziges = vorstufe.laenge.iter().position(|&l| l > 0).unwrap_or(0);
        let zweites = if einziges == 0 { 1 } else { 0 };
        let mut w = vorstufe_gewichte;
        w[einziges] = 1;
        w[zweites] = 1;
        vorstufe = Praefixcode::baue(&w, MAX_VORSTUFEN_LAENGE, "vp8l code-laengen")?;
    }

    // --- Wie viele Laengen-Felder schreiben? ---
    // Mindestens 4 (das ist die Mindestzahl im Format) und genau so viele,
    // wie die benutzten Symplice brauchen. `CODE_LAENGEN_ORDNUNG` sagt,
    // an welcher Position ein Symbol steht.
    let mut letzte_pos = 3usize; // -> mindestens 4 Felder
    for s in 0..MAX_CODE_LAENGEN {
        if vorstufe.laenge[s] > 0 {
            if let Some(pos) = CODE_LAENGEN_ORDNUNG.iter().position(|&o| o == s) {
                letzte_pos = letzte_pos.max(pos);
            }
        }
    }
    let anzahl = (letzte_pos + 1).min(MAX_CODE_LAENGEN);

    // --- Kopf schreiben ---
    b.bit(0); // kein einfacher Code
    b.ohm(u32::try_from(anzahl - 4).unwrap_or(0), 4);
    // Die Reihenfolge ist die Spezifikation (`CODE_LAENGEN_ORDNUNG`), nicht
    // die aufsteigende Symbolnummer — deshalb der Index und kein `iter()`.
    #[allow(clippy::needless_range_loop)]
    for i in 0..anzahl {
        b.ohm(u32::from(vorstufe.laenge[CODE_LAENGEN_ORDNUNG[i]]), 3);
    }
    // Diagnose: der Enkoder sagt, was er schreibt. Ohne das muss man beim
    // Fehlernachstellen raten, an welcher Stelle der Strom auseinander-
    // laeuft — und raten ist bei einem Bitformat genau die Sache, die man
    // nicht tun soll.
    let dump = std::env::var_os("NARSIL_KONVERT_VP8L_DUMP").is_some();
    if dump {
        eprintln!(
            "        anzahl={anzahl} vorstufe={:?} plan={}",
            (0..MAX_CODE_LAENGEN)
                .map(|i| (i, vorstufe.laenge[i], vorstufe.code[i]))
                .filter(|&(_, l, _)| l > 0)
                .collect::<Vec<_>>(),
            plan.len()
        );
        eprintln!(
            "        baum={:?}",
            (0..code.laenge.len())
                .filter(|&i| code.laenge[i] > 0)
                .take(10)
                .map(|i| (i, code.laenge[i], code.code[i]))
                .collect::<Vec<_>>()
        );
    }
    // `max_symbol`: 0-Bit = „so viele Symbole wie das Alphabet hat".
    // Das ist die VORGABE des Lesers (vp8l.rs:265) — der Entwurf
    // schreibt hier `num_code_lengths` und verschiebt damit alles.
    b.bit(0);

    // --- Laengenstrom ---
    for schritt in &plan {
        if schritt.symbol < 16 {
            b.code(&vorstufe, u16::from(schritt.symbol));
        } else {
            b.code(&vorstufe, u16::from(schritt.symbol));
            let extra = match schritt.symbol {
                16 => 2,
                17 => 3,
                _ => 7,
            };
            b.ohm(schritt.extra, extra);
        }
    }
    Ok(())
}

/// Ein Schritt im Laengenstrom: ein Symbol und seine Zusatzbits.
#[derive(Debug, Clone, Copy)]
struct LaengenSchritt {
    symbol: u8,
    extra: u32,
}

/// Der Laengenstrom als Liste von Schritten — OHNE Wiederholungscodes.
///
/// # Warum vorerst ohne Wiederholungscodes
///
/// Die Codes 16/17/18 sind eine Optimierung: sie machen aus 280
/// geschriebenen Laengen (gruener Baum) rund 40 Codewoerter. Der Preis ist
/// ein zweiter Praefixcode mit bis zu 19 Symbolen und bis zu 7 Bit Tiefe —
/// und genau dort weicht die Auslegung von libwebp von der des
/// Projekt-Lesers ab.
///
/// GEMESSEN 02.10.2026 an ffmpeg/libwebp: mit Wiederholungscodes lehnte
/// libwebp die Datei schon im KOPF ab („invalid symbol 252 + repeat 51 >
/// alphabet size 280") — der Bildstrom wurde nie erreicht. Mit
/// Wiederholungscodes werden nur die Laengen 0, L und 16/17/18 benutzt,
/// der Code-Laengen-Code braucht also bis zu vier Symbole; ohne sie werden
/// nur die Laengen 0 bis 15 benutzt. Das ist die kleinere
/// Fehlerflaeche, und sie ist der naechste Schritt, wenn die
/// libwebp-Auslegung der Wiederholungscodes geklaert ist.
///
/// Der Preis sind rund 250 bis 400 Byte Kopfkosten je Bild. Bei Bildern ab
/// etwa 100 x 100 Pixeln ist das unbemerkbar; bei einem 16x16-Symbol sind
/// es rund ein Drittel der Datei. Deshalb ist es eine bewusste
/// Entscheidung und kein Versehen, und es steht als offener Punkt im
/// Bericht.
fn laengen_plan(laengen: &[u8], alphabet: usize) -> Vec<LaengenSchritt> {
    let n = laengen.len().min(alphabet);
    let mut raus: Vec<LaengenSchritt> = Vec::with_capacity(n);
    for &symbol in &laengen[..n] {
        raus.push(LaengenSchritt { symbol, extra: 0 });
    }
    raus
}

/// Ein Einzeiler ueber die Kopfdaten eines Baums: Zahl der Symbole,
/// tiefster Code, Kraftsumme. Nur fuer `NARSIL_KONVERT_VP8L_DUMP`.
fn kopf_text(code: &Praefixcode) -> String {
    let kraft: u64 = (0..code.laenge.len())
        .filter(|&i| code.laenge[i] > 0)
        .map(|i| 1u64 << (huffman::MAX_LAENGE_VP8L - code.laenge[i]))
        .sum();
    format!(
        "Symbole={:3} tiefste={} Kraft={kraft}/{}",
        code.benutzt,
        code.tiefste,
        1u64 << huffman::MAX_LAENGE_VP8L
    )
}

fn ueberlauf() -> KonvertFehler {
    KonvertFehler::SpeicherZuGross {
        bytes: u64::MAX,
        grenze: grenzen::MAX_SPEICHER,
    }
}
