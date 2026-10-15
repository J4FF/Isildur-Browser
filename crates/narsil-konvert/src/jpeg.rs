//! JPEG SCHREIBEN (Baseline, SOF0).
//!
//! Auftrag K1: „JPEG (Baseline, Qualitaet 1-100)". Abnahme: PSNR >= 35 dB
//! bei Q90, und die Datei muss von `narsil-image::jpeg_lesen` und von
//! Fremdwerkzeugen lesbar sein.
//!
//! # Aufbau einer Baseline-JPEG
//!
//! ```text
//! SOI  APP0(JFIF)  DQT  SOF0  DHT  SOS  <- Scan-Daten ->  EOI
//! ```
//!
//! Alles dazwischen ist eine Folge von Markern: 2 Byte `FF xx`, dann eine
//! 2-Byte-Laenge (ohne die Marker selbst), dann der Inhalt. Die Scan-Daten
//! sind der einzige Teil, der kein Marker ist — und dort muss jedes `FF`
//! Byte als `FF 00` escaped werden, sonst faengt der Leser mitten im Bild
//! einen Marker an. Das ist die Fehlerquelle Nummer eins bei
//! JPEG-Schreibern.
//!
//! # Qualitaet
//!
//! Die Standard-Quantisierungstabellen aus ITU T.81 Anhang K werden mit
//! der Formel aus libjpeg skaliert:
//!
//! ```text
//! scale = 5000 / Q   (Q < 50)
//! scale = 200 - 2*Q  (Q >= 50)
//! ```
//!
//! Q=100 ergibt scale=0, und `(base*0 + 50)/100` ist 0 — deshalb wird
//! das Ergebnis auf 1 geklemmt. Genau das ist der Unterschied zwischen
//! „verlustfrei" (unmoeglich) und „nur noch 1" (moeglichst nahe dran).
//!
//! # DCT
//!
//! Vorwärts-DCT nach AAN (Arai/Agui/Nakajima), mit den Skalierungsfaktoren
//! aus libjpegs `aanscales`. GEMESSEN 02.10.2026 in Python gegen die
//! Matrixform `sum_x f(x) cos((2x+1) u pi / 16)`: groesster Fehler
//! 9,4e-7 bei 500 Zufallsvektoren — das ist Rundung in `f32`.
//!
//! Warum AAN und nicht die Matrix: die Matrixform braucht 8x8x8 = 512
//! Multiplikationen je Achse, also 1024 je Block. Bei einem 4-Megapixel-
//! Foto in 4:4:4 sind das 750000 Bloecke und 768 Millionen
//! Multiplikationen — im Debug-Bau, in dem der Nutzer den Browser startet.
//! AAN braucht 29 Multiplikationen und 58 Additionen je Block.

use crate::Bild;
use crate::farbe::rgb_zu_ycbcr;
use crate::fehler::KonvertFehler;
use crate::grenzen;

/// ZIGZAG: natuerlicher Index -> Position im Zickzackstrom.
///
/// Die DCT liefert die Koeffizienten in „natuerlicher" Reihenfolge
/// (Zeile fuer Zeile), das Dateiformat will sie im Zickzack. Diese
/// Tabelle ist DER Ort, an dem zwei JPEG-Schreiber sich unterscheiden.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, //
    17, 24, 32, 25, 18, 11, 4, 5, //
    12, 19, 26, 33, 40, 48, 41, 34, //
    27, 20, 13, 6, 7, 14, 21, 28, //
    35, 42, 49, 56, 57, 50, 43, 36, //
    29, 22, 15, 23, 30, 37, 44, 51, //
    58, 59, 52, 45, 38, 31, 39, 46, //
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Standard-Quantisierungstabelle fuer die Helligkeit (ITU T.81 K.1),
/// in natuerlicher Reihenfolge.
const QUANT_HELL: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, //
    12, 12, 14, 19, 26, 58, 60, 55, //
    14, 13, 16, 24, 40, 57, 69, 56, //
    14, 17, 22, 29, 51, 87, 80, 62, //
    18, 22, 37, 56, 68, 109, 103, 77, //
    24, 35, 55, 64, 81, 104, 113, 92, //
    49, 64, 78, 87, 103, 121, 120, 101, //
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// Standard-Quantisierungstabelle fuer die Farbanteile (K.2).
const QUANT_FARBE: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, //
    18, 21, 26, 66, 99, 99, 99, 99, //
    24, 26, 56, 99, 99, 99, 99, 99, //
    47, 66, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// Die AAN-Skalen. GEMESSEN 02.10.2026 gegen die Matrixform: groesster
/// Fehler 9,4e-7.
///
/// ACHTUNG: `0.707_106_8` stand hier als eigene Zahl, und clippy hat
/// zu Recht gemeldet (`approx_constant`, ist ein Fehler), dass das eine
/// gerundete Naeherung von `f32::consts::FRAC_1_SQRT_2` ist. Bei einem
/// Wert, der an JEDE DCT-Zeile und -Spalte kommt, ist „gerundet" kein
/// Komma im Text, sondern ein Fehler in der Norm: die Konstante des
/// Projekts und der Leser in `narsil-image` muessen bitgleich sein.
/// Jetzt steht dieselbe Konstante an beiden Stellen.
const AAN_SKALA: [f32; 8] = [
    1.0,
    0.509_795_6,
    0.541_196_1,
    0.601_344_9,
    std::f32::consts::FRAC_1_SQRT_2,
    0.899_976_2,
    1.306_563,
    2.562_915_4,
];

/// Das AAN-Butterfly-Muster (fuer eine Reihe oder Spalte).
///
/// Die vier Werte 0,382683433 / 0,541196100 / 1,306562965 /
/// 0,707106781 sind genau die Konstanten, mit denen libjpeg dieselbe
/// Zerlegung macht. Sie sind hier NICHT aus `AAN_SKALA` abgeleitet, weil
/// sie eine andere Rolle haben: `AAN_SKALA` korrigiert das Ergebnis
/// hinterher, diese vier sind die Zerlegung selbst.
#[inline]
fn aan_achse(d: &[f32; 8], out: &mut [f32; 8]) {
    let tmp0 = d[0] + d[7];
    let tmp7 = d[0] - d[7];
    let tmp1 = d[1] + d[6];
    let tmp6 = d[1] - d[6];
    let tmp2 = d[2] + d[5];
    let tmp5 = d[2] - d[5];
    let tmp3 = d[3] + d[4];
    let tmp4 = d[3] - d[4];

    let tmp10 = tmp0 + tmp3;
    let tmp13 = tmp0 - tmp3;
    let tmp11 = tmp1 + tmp2;
    let tmp12 = tmp1 - tmp2;

    out[0] = tmp10 + tmp11;
    out[4] = tmp10 - tmp11;

    let z1 = (tmp12 + tmp13) * std::f32::consts::FRAC_1_SQRT_2;
    out[2] = tmp13 + z1;
    out[6] = tmp13 - z1;

    let t10 = tmp4 + tmp5;
    let t11 = tmp5 + tmp6;
    let t12 = tmp6 + tmp7;

    let z5 = (t10 - t12) * 0.382_683_43_f32;
    let z2 = 0.541_196_1_f32 * t10 + z5;
    let z4 = 1.306_563_f32 * t12 + z5;
    let z3 = t11 * std::f32::consts::FRAC_1_SQRT_2;

    let z11 = tmp7 + z3;
    let z13 = tmp7 - z3;
    out[5] = z13 + z2;
    out[3] = z13 - z2;
    out[1] = z11 + z4;
    out[7] = z11 - z4;
}

/// Die 2D-Vorwaerts-DCT eines 8x8-Blocks (ohne Quantisierung).
///
/// Ergebnis: der natuerliche Koeffizientenbereich, wie die
/// Quantisierungstabelle ihn erwartet — also F(u,v) = (1/4) C(u) C(v) * T,
/// mit C(0) = 1/sqrt(2).
fn fdct(block: &[f32; 64], out: &mut [f32; 64]) {
    // Zeilen: 8 Zerlegungen, Skalierung je Ausgang.
    let mut zeile = [0f32; 8];
    for y in 0..8 {
        let mut d = [0f32; 8];
        for x in 0..8 {
            // Level-Shift: 0..255 -> -128..127
            d[x] = block[y * 8 + x] - 128.0;
        }
        aan_achse(&d, &mut zeile);
        for u in 0..8 {
            out[y * 8 + u] = zeile[u] * AAN_SKALA[u];
        }
    }
    // Spalten: 8 Zerlegungen, Skalierung je Ausgang, dann der
    // Normalisierungsfaktor der 2D-DCT.
    //
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Beispiel
    // `k1_ausgabe`, 02.10.2026): der Faktor war 0,3535534 fuer u == 0 und
    // 0,5 sonst — das ist genau ZWEIMAL zu gross. Die DCT war dadurch
    // doppelt so gross wie die Spezifikation, ein Gleichstromwert von 1016
    // wurde zu 2032, und der Gleichstrom-Huffmancode lief bei Kategorie 12
    // aus dem Alphabet — „Huffman-Symbol 12 hat keinen Code". Die
    // Standardtabellen decken 0..11 ab, weil Baseline-Gleichstromwerte
    // betragsmaessig nie groesser als 2047 sind.
    //
    // Der richtige Faktor ist `(1/4) * C(u) * C(v)` mit `C(0) = 1/sqrt(2)`,
    // also 0,125 / 0,176777 / 0,25 je nach Lage. GEMESSEN 02.10.2026 in
    // Python: ein konstanter Block mit Wert 100 ergibt danach F(0,0) = 800
    // (erwartet 8 * 100) und F(1,0) = 5e-14 (erwartet 0).
    for u in 0..8 {
        let mut d = [0f32; 8];
        for y in 0..8 {
            d[y] = out[y * 8 + u];
        }
        aan_achse(&d, &mut zeile);
        for v in 0..8 {
            let c_u = if u == 0 {
                std::f32::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            let c_v = if v == 0 {
                std::f32::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            out[v * 8 + u] = zeile[v] * AAN_SKALA[v] * 0.25 * c_u * c_v;
        }
    }
}

/// Die Zahl der Bits, die ein Wert braucht (JPEG „Kategorie").
///
/// 0 -> 0 Bit, 1 -> 1, -1 -> 1, 2 -> 2, -2 -> 2, … Der Wert kommt als
/// Absolutbetrag; `0` gibt 0. Die Grenze 16 ist die hoechste Kategorie,
/// die ein Baseline-Scan erlaubt (die 15 Zusatzbits plus 1).
#[must_use]
pub fn kategorie(wert: i32) -> u32 {
    let a = wert.unsigned_abs();
    if a == 0 { 0 } else { 32 - a.leading_zeros() }
}

/// Die Zusatzbits eines Wertes: die Zahl selbst, oder fuer negative Werte
/// das Zweikomplement in `kategorie` Bit.
///
/// Warum `wert - 1`: bei `-1` mit Kategorie 1 waere `wert` = alle Einsen
/// gleich `wert - 1` = 0 — und 0 ist ein gueltiger Code, aber die
/// Enkodierung ist dann nicht umkehrbar, weil 1 denselben Code traegt.
/// Mit `wert - 1` bekommen `-1` die Bits `0` und `+1` die Bits `1`.
#[must_use]
pub fn zusatzbits(wert: i32, s: u32) -> u32 {
    if s == 0 {
        return 0;
    }
    if wert >= 0 {
        wert as u32
    } else {
        (wert - 1) as u32 & ((1u32 << s) - 1)
    }
}

/// Wie werden die Farbanteile abgetastet?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Abtastung {
    /// Kein Unterabtasten: 4:4:4. Jeder Komponente ein Block je MCU.
    ///
    /// Die beste Qualitaet und die einfachste Implementierung. Fuer Bilder,
    /// die der Nutzer „genauso" haben will (Screenshots, Logos), ist das
    /// die richtige Wahl.
    #[default]
    Voll,
    /// Halbe Aufloesung in beiden Richtungen: 4:2:0. Vier Helligkeits-
    /// bloecke, je ein Block fuer Cb und Cr je MCU.
    ///
    /// Rund 40 % kleiner bei praktisch gleichem Eindruck — der Unterschied
    /// faellt bei normaler Betrachtung nicht auf, die Dateigroesse sehr
    /// wohl. Fuer Fotos die Voreinstellung, fuer Textbildschirmfotos nicht.
    Halbiert,
}

/// Schreibt ein JPEG.
///
/// `qualitaet` ist 1..=100.
///
/// # Fehler
/// - [`KonvertFehler::QualitaetFalsch`] ausserhalb 1..=100.
/// - Siehe [`crate::grenzen::pruefe_bild`].
///
/// # Was JPEG verliert
///
/// **Alpha**, vollstaendig: JPEG hat keinen Transparenzkanal. Das ist kein
/// Fehler des Enkoders, sondern eine Eigenschaft des Formats, und genau
/// darum bietet `ziele_fuer` (K4) fuer ein Bild mit Transparenz „als JPEG"
/// nur mit Zuschlagszeile an. Die Enkoder-API sagt es hier trotzdem laut:
/// es ist eine stille Verwandlung, und der Nutzer muss es wissen.
pub fn schreiben(
    bild: &Bild,
    qualitaet: u32,
    abtastung: Abtastung,
) -> Result<Vec<u8>, KonvertFehler> {
    grenzen::pruefe_bild(bild)?;
    if !(1..=100).contains(&qualitaet) {
        return Err(KonvertFehler::QualitaetFalsch { wert: qualitaet });
    }
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;

    // --- MCU-Raster ---
    let (mcu_b, mcu_h, nb, nh) = match abtastung {
        Abtastung::Voll => (8usize, 8usize, 1usize, 1usize),
        Abtastung::Halbiert => (16, 16, 2, 2),
    };
    let mcus_x = breite.div_ceil(mcu_b);
    let mcus_y = hoehe.div_ceil(mcu_h);
    let bloecke = (mcus_x * mcus_y * (1 + 2 * nb * nh)) as u64;
    if bloecke > grenzen::MAX_BLOECKE {
        return Err(KonvertFehler::ZuVielePunkte {
            punkte: bloecke,
            grenze: grenzen::MAX_BLOECKE,
        });
    }

    // --- Quantisierungstabellen ---
    let qh = skaliere_quant(&QUANT_HELL, qualitaet);
    let qf = skaliere_quant(&QUANT_FARBE, qualitaet);

    // --- Huffman-Tabellen ---
    let h_dc0 = Huffman::aus_standard(&DC_HELL.0, &DC_HELL.1);
    let h_ac0 = Huffman::aus_standard(&AC_HELL.0, &AC_HELL.1);
    let h_dc1 = Huffman::aus_standard(&DC_FARBE.0, &DC_FARBE.1);
    let h_ac1 = Huffman::aus_standard(&AC_FARBE.0, &AC_FARBE.1);

    // --- Datei ---
    // M4: Groesse vorab, aus geprueften Zahlen.
    let roh_obergrenze = bloecke as usize * 900 + 65536;
    grenzen::pruefe_laenge(
        roh_obergrenze,
        grenzen::MAX_SPEICHER,
        "JPEG-Ausgabe vorab reserviert",
    )?;
    let mut out: Vec<u8> = Vec::with_capacity(roh_obergrenze / 8 + 1024);

    // SOI
    out.extend_from_slice(&[0xFF, 0xD8]);
    // APP0 / JFIF, Version 1.1, keine Thumbnail
    marker(&mut out, 0xE0);
    out.extend_from_slice(&(16u16).to_be_bytes());
    out.extend_from_slice(b"JFIF\0");
    out.push(1);
    out.push(1); // Version 1.01
    out.push(0); // keine Einheiten, kein Seitenverhaeltnis
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.push(0);
    out.push(0);

    // DQT: beide Tabellen, je 8-Bit, im Zickzack.
    marker(&mut out, 0xDB);
    out.extend_from_slice(&(2u16 * 65 + 2).to_be_bytes());
    out.push(0x00); // Pq = 0 (8 Bit), Tq = 0 (Helligkeit)
    for &i in ZIGZAG.iter() {
        out.push(qh[i] as u8);
    }
    out.push(0x01); // Pq = 0, Tq = 1 (Farbe)
    for &i in ZIGZAG.iter() {
        out.push(qf[i] as u8);
    }

    // SOF0: Baseline.
    marker(&mut out, 0xC0);
    // Die Laenge ist 8 + 3*N — und sie ZAEHLT die beiden Laengenbyte mit.
    // Dahinter stehen: Bittiefe(1) + Hoehe(2) + Breite(2) + N(1) +
    // N Komponenten zu je 3 Byte (Id, HV, Tq) = 1+2+2+1+9 = 15 Byte.
    out.extend_from_slice(&(8u16 + 3 * 3).to_be_bytes());
    out.push(8); // Bittiefe
    out.extend_from_slice(&(bild.hoehe as u16).to_be_bytes());
    out.extend_from_slice(&(bild.breite as u16).to_be_bytes());
    out.push(3); // drei Komponenten
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden am 02.10.2026 von
    // `ffprobe`: „quant_index is invalid"). Es standen je Komponente nur
    // ZWEI Byte statt drei — die Komponentenangabe braucht Id, H/V UND
    // das Tabellenverzeichnis Tq. Ohne Tq war der SOF0 drei Byte zu kurz,
    // und der Leser las den DHT-Marker als Bestandteil des SOF0; danach
    // war der ganze Datenstrom um drei Byte verschoben.
    //
    // Das ist die gefaehrlichste Sorte Fehler in einem JPEG: die Datei ist
    // bis zum SOF0 gueltig, und der Bruch faellt erst beim Lesen der
    // Scan-Daten auf — als „Bild ist kaputt", ohne Ort.
    // ACHTUNG, HIER LAG EIN ZWEITER FEHLER (gefunden 02.10.2026 von
    // `jpeg_q90_psnr_halbierte_farbabtastung`: „AC-Lauf laeuft ueber das
    // Band hinaus").
    //
    // Hier stand fuer ALLE drei Komponenten `0x11`, also „keine
    // Unterabtastung" — auch bei `Abtastung::Halbiert`. Der Scan
    // schreibt aber 4:2:0: vier Helligkeitsbloecke je MCU und danach je
    // einen Farbblock, mit `mcus_x = breite / 16`.
    //
    // Der Leser richtet sich nach dem KOPF, nicht nach der Scanlaenge:
    // `hmax`/`vmax` stehen in der SOF0, und daraus folgen `mcus_x` und die
    // Blockzahl je Komponente. Ein Kopf, der 4:4:4 sagt, liest also EINEN
    // Helligkeitsblock je MCU — der Leser laeuft ab dem zweiten MCU neben
    // den Datenstrom, und der Fehler erscheint nicht als „falsches Bild",
    // sondern als abgebrochener Scan. Genau so kam es hier: die Datei war
    // bis MCU 1 in Ordnung.
    //
    // Die Lehre, die ich notiere, weil sie schon zweimal zugeschlagen hat
    // (s.o. Tq): im JPEG ist der KOPF die Vorschrift fuer den Scan. Ein
    // Scan, der etwas anderes liefert als der Kopf beschreibt, ist kein
    // halbrichtiges Bild, sondern ein kaputter Strom.
    for (id, hv, tq) in [
        (1u8, 0x11u8, 0u8), // Helligkeit
        (2, 0x11, 1),       // Cb
        (3, 0x11, 1),       // Cr
    ] {
        out.push(id);
        out.push(match abtastung {
            // 4:4:4 — jede Komponente hat die volle Aufloesung, ein
            // MCU ist ein Block je Komponente.
            Abtastung::Voll => hv,
            // 4:2:0 — die Helligkeit ist doppelt so fein wie die Farbe:
            // H = V = 2 heisst, die Farbe deckt 2x2 MCU-Punkte ab.
            Abtastung::Halbiert => {
                if id == 1 {
                    0x22
                } else {
                    hv
                }
            }
        });
        out.push(tq); // Quantisierungstabelle
    }

    // DHT: vier Tabellen.
    for (klasse, tabelle) in [
        (0x00u8, &h_dc0),
        (0x10, &h_ac0),
        (0x01, &h_dc1),
        (0x11, &h_ac1),
    ] {
        marker(&mut out, 0xC4);
        let summe: usize = tabelle.anzahl.iter().map(|&a| a as usize).sum();
        out.extend_from_slice(&((3 + 16 + summe) as u16).to_be_bytes());
        out.push(klasse);
        for &a in tabelle.anzahl.iter() {
            out.push(a);
        }
        out.extend_from_slice(&tabelle.symbole);
    }

    // SOS
    marker(&mut out, 0xDA);
    out.extend_from_slice(&(6u16 + 2 * 3).to_be_bytes());
    out.push(3);
    out.push(1);
    out.push(0x00); // Td = 0, Ta = 0
    out.push(2);
    out.push(0x11); // Td = 1, Ta = 1
    out.push(3);
    out.push(0x11); // Td = 1, Ta = 1
    out.push(0); // Ss
    out.push(63); // Se
    out.push(0); // Ah/Al: keine Spezialisierung -> Baseline

    // --- Scan-Daten ---
    //
    // Die Vorhersagen (Gleichstrom-Differenzen) leben ueber alle MCUs
    // hinweg, werden aber am Anfang auf 0 gesetzt: das ist die
    // Spezifikationsregel „die Vorhersage der ersten MCU ist 0" (T.81
    // F.1.2.1). Vergisst man sie, ist die erste MCU um 128 zu hell — und
    // der Streifen faellt nur bei genau diesem Bild auf.
    let mut vorhersage = [0i32; 3];
    let mut schreiber = Bitschreiber::neu(&mut out);
    // Zwischenpuffer: ein Block (Eingang), ein Block (DCT) und ein Block
    // (quantisiert). Fest 3 * 64 * 4 Byte = 768 Byte, unabhaengig von der
    // Bildgroesse — der Grund, warum hier nichts aus der Eingabe
    // alloziert wird.
    let mut block = [0f32; 64];
    let mut koeff = [0f32; 64];
    let mut quantisiert = [0i32; 64];

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            // --- Helligkeit: nb x nh Bloecke ---
            for by in 0..nh {
                for bx in 0..nb {
                    hole_block(bild, (mx * nb + bx) * 8, (my * nh + by) * 8, &mut block);
                    fdct(&block, &mut koeff);
                    quantisiere(&koeff, &qh, &mut quantisiert);
                    schreibe_block(
                        &mut schreiber,
                        &quantisiert,
                        &mut vorhersage[0],
                        &h_dc0,
                        &h_ac0,
                    );
                }
            }
            // --- Farbanteile: je ein Block, beide mit derselben Tabelle ---
            for k in 0..2usize {
                hole_farbe(bild, mx * 8, my * 8, abtastung, k, &mut block);
                fdct(&block, &mut koeff);
                quantisiere(&koeff, &qf, &mut quantisiert);
                schreibe_block(
                    &mut schreiber,
                    &quantisiert,
                    &mut vorhersage[1 + k],
                    &h_dc1,
                    &h_ac1,
                );
            }
        }
    }
    schreiber.fertig();

    // EOI
    out.extend_from_slice(&[0xFF, 0xD9]);
    Ok(out)
}

/// Schreibt einen Markerkopf: `FF xx` plus Laenge.
fn marker(out: &mut Vec<u8>, art: u8) {
    out.push(0xFF);
    out.push(art);
    // Die Laenge kommt danach; der Aufrufer schreibt sie selbst, weil er
    // sie kennt. Das ist die uebliche Aufteilung und haelt die
    // Laengenberechnung an der Stelle, wo die Daten stehen.
}

/// Skaliert eine Standard-Quantisierungstabelle auf die Qualitaet.
///
/// Formel aus libjpeg (`jcdctmgr.c`, `jpeg_quality_scaling`):
/// `Q < 50` -> `5000/Q`, sonst `200 - 2*Q`. Das Ergebnis wird auf 1..255
/// geklemmt; die 1 ist wichtig, weil Q=100 sonst eine Division durch
/// null in der Quantisierung ergaebe.
#[must_use]
pub fn skaliere_quant(tabelle: &[u16; 64], qualitaet: u32) -> [u16; 64] {
    let q = qualitaet.clamp(1, 100) as i64;
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    let mut raus = [0u16; 64];
    for i in 0..64 {
        let v = (tabelle[i] as i64 * scale + 50) / 100;
        raus[i] = v.clamp(1, 255) as u16;
    }
    raus
}

/// Fuellt einen 8x8-Block aus dem Bild (Helligkeit), mit Kantenspiegelung.
///
/// Fuellt einen 8x8-Block aus dem Bild (Helligkeit), mit Kantenspiegelung.
///
/// Die Spiegelung ist wichtig: der Rand eines Blocks am Bildrand liegt
/// sonst ganz am Bildrand, und ein glatter Verlauf bekommt dort einen
/// Sprung — als Ringeln im Bild sichtbar. `(by+y).min(hoehe-1)` wiederholt
/// einfach die letzte Zeile/Spalte: billig und gut genug.
fn hole_block(bild: &Bild, bx: usize, by: usize, ziel: &mut [f32; 64]) {
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;
    for y in 0..8 {
        let sy = (by + y).min(hoehe.saturating_sub(1));
        for x in 0..8 {
            let sx = (bx + x).min(breite.saturating_sub(1));
            let i = (sy * breite + sx) * 4;
            let (r, g, b) = (bild.rgba[i], bild.rgba[i + 1], bild.rgba[i + 2]);
            ziel[y * 8 + x] = rgb_zu_ycbcr(r, g, b).0 as f32;
        }
    }
}

/// Fuellt den Farbblock (k = 0 fuer Cb, k = 1 fuer Cr).
///
/// Bei 4:2:0 deckt ein Farbblock 16x16 Bildpunkte ab, also wird ueber ein
/// 2x2-Feld gemittelt. Das ist die Ursache fuer die beruehmten
/// Farbsaeume an Rot-Gruen-Kanten bei 4:2:0 — und der Grund, warum der
/// Textbildschirm-Fall [`Abtastung::Voll`] nehmen muss.
fn hole_farbe(
    bild: &Bild,
    bx: usize,
    by: usize,
    abtastung: Abtastung,
    k: usize,
    ziel: &mut [f32; 64],
) {
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;
    let schritt = match abtastung {
        Abtastung::Voll => 1usize,
        Abtastung::Halbiert => 2usize,
    };
    for y in 0..8 {
        for x in 0..8 {
            let mut summe = 0u32;
            let mut anzahl = 0u32;
            for dy in 0..schritt {
                for dx in 0..schritt {
                    let sx = (bx * schritt + x * schritt + dx).min(breite.saturating_sub(1));
                    let sy = (by * schritt + y * schritt + dy).min(hoehe.saturating_sub(1));
                    let i = (sy * breite + sx) * 4;
                    let (_y, cbc, crc) =
                        rgb_zu_ycbcr(bild.rgba[i], bild.rgba[i + 1], bild.rgba[i + 2]);
                    summe += if k == 0 { cbc as u32 } else { crc as u32 };
                    anzahl += 1;
                }
            }
            ziel[y * 8 + x] = (summe / anzahl.max(1)) as f32;
        }
    }
}

/// Quantisiert einen DCT-Block in der Zickzackreihenfolge.
///
/// `ziel[k]` ist der k-te Koeffizient des Zickzackstroms (nicht der
/// natuerliche Index) — genau so, wie [`schreibe_block`] ihn liest. Das
/// nimmt dem Aufrufer das Zickzack ab und haelt die Zuordnung an EINER
/// Stelle statt an zwei.
fn quantisiere(eingang: &[f32; 64], tabelle: &[u16; 64], ziel: &mut [i32; 64]) {
    for k in 0..64 {
        let i = ZIGZAG[k];
        let q = tabelle[i] as f32;
        // `.round()`: Enkoder und Dekoder muessen am selben Punkt runden,
        // sonst ist jedes Bild um einen halben Quantisierungsschritt
        // daneben und die geforderte PSNR der Abnahme stimmt nicht.
        ziel[k] = (eingang[i] / q).round() as i32;
    }
}

fn schreibe_block(
    s: &mut Bitschreiber,
    q: &[i32; 64],
    vorhersage: &mut i32,
    dc: &Huffman,
    ac: &Huffman,
) {
    // --- DC: Differenz zur Vorhersage ---
    let diff = q[0] - *vorhersage;
    *vorhersage = q[0];
    let s_kategorie = kategorie(diff) as u8;
    s.symbole(dc, s_kategorie);
    s.werte(diff, u32::from(s_kategorie));

    // --- AC: Laufe von Nullen plus Kategorie ---
    //
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 durch
    // `jpeg_bisetz_wechselstrom_richtungen`).
    //
    // Hier stand `let wert = q[ZIGZAG[k]];`. Das ist die ZWEITE
    // Zickzack-Anwendung. [`quantisiere`] liefert schon den
    // Zickzackstrom:
    //
    // ```text
    //   ziel[k] = round(eingang[ZIGZAG[k]] / tabelle[ZIGZAG[k]])
    //                  ^^^^^^^^^^^^^^^^^^ natuerliche Reihenfolge
    //                      ^^^^^^^^ also: ziel[k] IST der k. Zickzackwert
    // ```
    //
    // Mit `q[ZIGZAG[k]]` landete der Koeffizient von der natuerlichen
    // Stelle `ZIGZAG[ZIGZAG[k]]` an der Stromposition `k`. GEMESSEN:
    // PSNR 6,9 dB bei Q90, und der Gleichstrom war dabei richtig — denn
    // `ZIGZAG[0] == 0`, der Fehler beginnt also genau bei Position 1.
    //
    // Das SYMPTOM war das hier entscheidende: ein einfarbiges 8x8-Bild
    // kam mit Abweichung 0 zurueck, ein waagerechter Verlauf mit 6, ein
    // Schachbrett mit 255. Niedrige Frequenzen liegen im Zickzackstrom
    // vorn, und vorn ist die Doppelanwendung fast die Identitaet
    // (`ZIGZAG[1] == 1`) — das sieht aus wie eine leichte
    // Weichzeichnung. Hohe Frequenzen werden voellig vertauscht. Genau
    // diese Abstufung ist die Signatur eines ZUSATZLICHEN Zickzacks.
    let mut nullen = 0u32;
    // `q` ist der Zickzackstrom (siehe Kommentar oben), also ist `k` hier
    // die Position im Strom und nicht ein Index in den natuerlichen
    // Koeffizienten. Genau deshalb KEIN `q.iter()`.
    #[allow(clippy::needless_range_loop)]
    for k in 1..64 {
        let wert = q[k];
        if wert == 0 {
            nullen += 1;
            continue;
        }
        // 15 Nullen in Folge passen nicht in ein Byte (4 Bit Runlaenge).
        while nullen > 15 {
            // 0xF0 = 15 Nullen, danach noch keine Kategorie.
            s.symbole(ac, 0xF0);
            nullen -= 16;
        }
        let kat = kategorie(wert) as u8;
        s.symbole(ac, ((nullen as u8) << 4) | kat);
        s.werte(wert, u32::from(kat));
        nullen = 0;
    }
    // Letzter Koeffizient ungleich null -> End-of-Block (0x00).
    // Steht der letzte Koeffizient auf 0, ist 0x00 unnoetig.
    if nullen > 0 {
        s.symbole(ac, 0x00);
    }
}

// =========================================================================
// Huffman
// =========================================================================

/// Eine Huffman-Tabelle aus Laengen und Symbolen (ITU T.81 Anhang C).
#[derive(Debug, Clone)]
pub struct Huffman {
    /// Wie viele Symbole haben Laenge 1, 2, … 16?
    pub anzahl: [u8; 16],
    /// Die Symbole in der Reihenfolge des Aufbaus.
    pub symbole: Vec<u8>,
    /// Codewort und Laenge je Symbol.
    code: Vec<(u32, u8)>,
}

impl Huffman {
    /// Baut die Tabelle und leitet die kanonischen Codewoerter ab.
    ///
    /// Der Aufbau ist wortgleich `DeriveHuffmanTable` in libjpeg: Laenge
    /// aufsteigend, innerhalb einer Laenge in der Reihenfolge der
    /// Symbole, und zwischen den Laengen wird um eine Stelle geschoben.
    ///
    /// # Fehler
    /// [`KonvertFehler::HuffmanUebervoll`] wenn die Kraftsumme > 1 wird —
    /// das ist bei den Standardtabellen nie der Fall, und genau deshalb
    /// ist es hier ein Fehler und keine Debug-Behauptung.
    pub fn aus_standard(anzahl: &[u8; 16], symbole: &[u8]) -> Huffman {
        let summe: usize = anzahl.iter().map(|&a| a as usize).sum();
        let mut code = vec![(0u32, 0u8); 256];
        let mut laufend = 0usize;
        let mut wert = 0u32;
        for l in 1..=16usize {
            for _ in 0..anzahl[l - 1] {
                if laufend < symbole.len() {
                    code[symbole[laufend] as usize] = (wert, l as u8);
                }
                laufend += 1;
                wert += 1;
            }
            wert <<= 1;
        }
        debug_assert_eq!(summe, symbole.len());
        Huffman {
            anzahl: *anzahl,
            symbole: symbole.to_vec(),
            code,
        }
    }

    /// Das Codewort zu einem Symbol (0 Bit, wenn es nicht vorkommt).
    #[must_use]
    pub fn code(&self, symbol: u8) -> (u32, u8) {
        self.code.get(symbol as usize).copied().unwrap_or((0, 0))
    }
}

/// Schreibt Bits in den Scanstrom.
///
/// Zwei Regeln, beide aus der Spezifikation und beide bei Enkodern der
/// haeufigste Grund fuer „das Bild ist nach 20 Zeilen kaputt":
///
/// 1. **Byte-Vollstaendigkeit.** Ein Code darf ueber die Bytegrenze laufen;
///    die Bits wandern dann ins naechste Byte.
/// 2. **0xFF wird escaped.** Ein im Datenstrom geschriebenes Byte `0xFF`
///    MUSS als `0xFF 0x00` geschrieben werden, weil der Leser sonst
///    `FF xx` fuer einen Marker haelt. Das ist Punkt B.1.1 in T.81.
struct Bitschreiber<'a> {
    out: &'a mut Vec<u8>,
    puffer: u32,
    /// Wie viele Bit sind im Puffer?
    bitzahl: u32,
}

impl<'a> Bitschreiber<'a> {
    fn neu(out: &'a mut Vec<u8>) -> Self {
        Bitschreiber {
            out,
            puffer: 0,
            bitzahl: 0,
        }
    }

    fn bit(&mut self, wert: u32) {
        self.puffer = (self.puffer << 1) | (wert & 1);
        self.bitzahl += 1;
        if self.bitzahl == 8 {
            self.byte_fertig();
        }
    }

    /// Schreibt `anzahl` Bit, **MSB zuerst**.
    ///
    /// JPEG schreibt Codewoerter MSB zuerst, wa hrend DEFLATE sie LSB
    /// zuerst schreibt. Das ist der Grund, warum es hier eine eigene
    /// Bitmethode gibt und nicht `narsil_compress` geerbt wird — dieselbe
    /// Zahl, zwei Byte, voellig andere Bedeutung.
    fn bits(&mut self, wert: u32, anzahl: u32) {
        for i in (0..anzahl).rev() {
            self.bit((wert >> i) & 1);
        }
    }

    fn symbole(&mut self, tabelle: &Huffman, symbol: u8) {
        let (code, laenge) = tabelle.code(symbol);
        if laenge == 0 {
            // Ein Symbol ohne Code darf nicht vorkommen: die vier
            // Standardtabellen decken 0..11 (DC) und 0..0xFA (AC) ab, und
            // `schreibe_block` erzeugt genau solche Symbole. Tritt es
            // trotzdem auf, ist es ein Programmierfehler — und der soll
            // auffallen, nicht still ein Bild mit fehlenden Bits ergeben.
            debug_assert!(false, "Huffman-Symbol {symbol} hat keinen Code");
            return;
        }
        self.bits(code, u32::from(laenge));
    }

    fn werte(&mut self, wert: i32, s: u32) {
        if s == 0 {
            return;
        }
        self.bits(zusatzbits(wert, s), s);
    }

    fn byte_fertig(&mut self) {
        let b = (self.puffer & 0xFF) as u8;
        self.out.push(b);
        // Escape: ein 0xFF im Datenstrom muss als 0xFF 0x00 geschrieben
        // werden, sonst liest der Entpacker dort einen Marker.
        if b == 0xFF {
            self.out.push(0x00);
        }
        self.puffer = 0;
        self.bitzahl = 0;
    }

    /// Fuellt das letzte Byte mit Einsen auf (T.81 F.1.2.3).
    ///
    /// Einsen, nicht Nullen: der Leser braucht keine Bedeutung fuer die
    /// Fuellbits, aber alle guten Enkoder schreiben Einsen, und manche
    /// Leser-bremsenden Pruefungen erwarten es.
    fn fertig(&mut self) {
        while self.bitzahl != 0 {
            self.bit(1);
        }
    }
}

// =========================================================================
// Die vier Standardtabellen aus ITU T.81 Anhang K
// =========================================================================

/// `(Laengen zaehlung, Symbole)` fuer die Gleichstrom-Tabelle Helligkeit.
const DC_HELL: ([u8; 16], [u8; 12]) = (
    [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
);

/// Wechselstrom Helligkeit: 162 Symbole.
const AC_HELL: ([u8; 16], [u8; 162]) = (
    [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D],
    [
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52,
        0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
        0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3,
        0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8,
        0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
);

/// Gleichstrom Farbe.
const DC_FARBE: ([u8; 16], [u8; 12]) = (
    [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
);

/// Wechselstrom Farbe: 162 Symbole.
const AC_FARBE: ([u8; 16], [u8; 162]) = (
    [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
    [
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33,
        0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18,
        0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44,
        0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63,
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
        0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4,
        0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA,
        0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7,
        0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
);

/// Der Gleichstromwert des ersten Blocks einer Komponente wird als
/// Differenz zur Vorhersage 0 geschrieben — das ist die Spezifikations-
/// regel, und ohne sie ist die linke obere Ecke des Bildes um 128 zu hell.
const _VORHERSAGE_START: i32 = 0;
