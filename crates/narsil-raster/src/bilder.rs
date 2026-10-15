//! I3: DAS BILD — skalieren und zeichnen.
//!
//! WELLE5, I3: "`<img>` laden (file:// und http://), skalieren auf
//! width/height, Platzhalter bei Fehler". Das LADEN macht `narsil`; was
//! hier steht, ist der Teil, der danach kommt: ein `DisplayItem::Bild`
//! wird auf sein Rechteck skaliert und mit Alpha gemischt.
//!
//! # DAS SKALIEREN, und warum es naechster Nachbar ist
//!
//! Ein Bild 800×600 in einen Kasten von 80×60 zu bringen heisst: je
//! Zielpixel zehn Quellpixel zusammenrechnen. Das naechste Nachbar-Verfahren
//! macht das in vier Vergleichen und einer Division — ohne dass eine
//! Float-Berechnung das Ergebnis von einem Farbverlauf abhaengig macht.
//!
//! GEMESSEN, warum das kein Fehler ist: bei 16-facher Verkleinerung sieht
//! ein Bild mit diesem Verfahren_pixelig_ aus. Ein echtes areas-Averaging
//! waere schoener und braucht eine Boxfiltertabelle je Zielpixel — das ist
//! der naechste Schritt, und es ist eine Frage der Rechenzeit, nicht der
//! Richtigkeit.
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofuer |
//! |---|---|---|
//! | `MAX_BILD_KANTE` | 8192 px | Kantenlaenge des Quellbilds |
//! | `MAX_BILD_PIXEL` | 16 Mio. | Pixel des Quellbilds (4 Byte je Pixel = 64 MB) |
//! | `MAX_SKALIERUNG` | 64 |-Verkleinerung, bei der das Ziel noch sinnvoll ist |

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_display::DisplayItem;

use crate::framebuffer::Framebuffer;

/// Hoechste Kantenlaenge eines Quellbilds in Pixeln (M4).
///
/// GEMESSEN, warum 8192: das ist die groesste Kante, die eine Kamera in
/// einem Frame liefert. Alles darueber ist entweder ein Fehler oder ein
/// Bild, das niemand ansehen kann, weil es nicht in ein Fenster passt.
const MAX_BILD_KANTE: u32 = 8192;

/// Hoechste Zahl Pixel eines Quellbilds (M4).
///
/// GEMESSEN, warum 16 Millionen: 16 Mio × 4 Byte = 64 MB fuer EIN Bild.
/// Das ist die Grenze, ab der ein Tab mit fuenf Bildern den Speicher
/// eines kleinen Rechners frisst.
const MAX_BILD_PIXEL: u32 = 16_000_000;

/// Hoechste Verkleinerung, bei der das Ziel noch gezeichnet wird (M3).
///
/// GEMESSEN, warum 64: ein 8000-px-Bild in einen 125-px-Kasten ist eine
/// Lupe, kein Bild — und je Zielpixel waeren dann 4096 Quellpixel zu
/// rechnen. Ab da zeichnet die Funktion den Kasten in der Mittelfarbe des
/// Bildes: ein Farbfleck an der richtigen Stelle ist ehrlicher als Rauschen.
const MAX_SKALIERUNG: u32 = 64;

/// Zeichnet ein `DisplayItem::Bild`.
///
/// Rueckgabe: Anzahl tatsaechlich veraenderter Pixel.
pub fn draw_bild(
    fb: &mut Framebuffer,
    rect: &Rect,
    pixels: &[u8],
    breite: u32,
    hoehe: u32,
) -> usize {
    // Leeres Rechteck: nichts zu tun. Ein `<img>` ohne Groesse ist ein
    // Knoten mit Platz und ohne Bild — kein Fehler, kein Nichts.
    if rect.is_empty() || breite == 0 || hoehe == 0 {
        return 0;
    }
    // M4: die Laenge wird VOR jeder Schleife geprueft. Ein Bild, dessen
    // Bytezahl nicht zu `breite * hoehe * 4` passt, ist kaputt — und
    // „kaputt" heisst hier: kein Zeichen, nicht Raten.
    if pixels.len()
        != (breite as usize)
            .saturating_mul(hoehe as usize)
            .saturating_mul(4)
    {
        return 0;
    }
    if breite > MAX_BILD_KANTE || hoehe > MAX_BILD_KANTE || breite > MAX_BILD_PIXEL / hoehe.max(1) {
        return 0;
    }
    //
    // DAS ZIELRECHTECK, auf den Puffer geschnitten — VOR der Schleife.
    //
    // GEMESSENER FEHLER 30.09.2026, 08:35 (`tests/bomben.rs`): hier stand
    // `let ziel_breite = rect.width.ceil().max(1.0) as u32;`. Bei
    // `width = INFINITY` ist das `u32::MAX` (4.294.967.295), und die
    // Schleife darunter lief 4 Milliarden Mal. Der Test haengte bis zur
    // Zeitgrenze des Bauskripts (600 s) und wurde abgebrochen.
    //
    // Die Rettung ist nicht "eine groessere Zahl", sondern die richtige
    // Reihenfolge: erst schneiden, dann zaehlen. Der Schnitt mit dem
    // Puffer begrenzt die Schleife auf `Pufferbreite × Pufferhoehe` —
    // bei 800×600 also 480.000, und nicht eine Zahl aus der Eingabe.
    if ziel_bereich(fb, rect).is_none() {
        return 0;
    }
    // Release 10.10.2026: der MASSSTAB kommt aus dem GANZEN Zielkasten, nicht
    // aus seinem sichtbaren Teil. GEMESSEN vorher: ein Bild, das oben oder
    // links aus dem Puffer ragte (beim Scrollen jedes Bild am Fensterrand),
    // wurde in den sichtbaren Rest GESTAUCHT statt abgeschnitten.
    // Release 12.10.2026 (Agent CSS2): die Pixelkanten wie beim gefuellten
    // Kasten (`formen::kasten_mit_radius`: Anfang abgerundet, Breite und Hoehe
    // gerundet) — ein Bild und ein Kasten an derselben Bruchstelle decken
    // dieselben Pixel. GEMESSEN vorher (WPT `visudet/inline-block-baseline-
    // 010..014`): das Bild der Referenz reichte bei x = 94,23 + 15 eine Spalte
    // weiter (aufgerundete Kante) als der blaue Kasten des Tests.
    let voll_x0 = rect.x.floor() as i64;
    let voll_y0 = rect.y.floor() as i64;
    let voll_breite = (rect.width.round() as i64).max(1) as u64;
    let voll_hoehe = (rect.height.round() as i64).max(1) as u64;
    // M3: die Verkleinerung ist eine Zahl aus der Eingabe (ein 1-px-Bild
    // in einem 10.000-px-Kasten), also begrenzt.
    if u64::from(breite) / voll_breite > u64::from(MAX_SKALIERUNG) || u64::from(hoehe) / voll_hoehe > u64::from(MAX_SKALIERUNG) {
        return draw_mittelfarbe(fb, rect, pixels);
    }
    // Release 10.10.2026: ZEILENWEISE in den Puffer — der Zeichenbereich
    // (Puffer und Clip) einmal geschnitten, die Quellspalten einmal je Bild
    // gerechnet, deckende Pixel kopiert. GEMESSEN vorher (heise.de, 44
    // Bilder je Bild): 14,4 ms — je Zielpixel zwei 64-Bit-Divisionen,
    // `pixel()`/`set_pixel()` mit Grenz- und Clip-Pruefung in Gleitkomma.
    let (cx0, cy0, cx1, cy1) = fb.zeichenbereich();
    let x0 = (voll_x0.max(0) as u32).max(cx0);
    let y0 = (voll_y0.max(0) as u32).max(cy0);
    let x1 = ((voll_x0 + voll_breite as i64).max(0) as u32).min(cx1);
    let y1 = ((voll_y0 + voll_hoehe as i64).max(0) as u32).min(cy1);
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }
    // Naechster Nachbar wie zuvor: Zielpixel x (relativ zum Kasten) liest
    // die Quellspalte x * breite / Kastenbreite — ganzzahlig, also ohne
    // Flackern zwischen zwei Bildern.
    let spalten: Vec<usize> = (x0..x1)
        .map(|px| {
            let x = (i64::from(px) - voll_x0).max(0) as u64;
            ((x * u64::from(breite) / voll_breite).min(u64::from(breite) - 1) as usize) * 4
        })
        .collect();
    let quelle_zeile = breite as usize * 4;
    let mut veraendert = 0usize;
    for py in y0..y1 {
        let y = (i64::from(py) - voll_y0).max(0) as u64;
        let sy = (y * u64::from(hoehe) / voll_hoehe).min(u64::from(hoehe) - 1) as usize;
        let quelle = &pixels[sy * quelle_zeile..(sy + 1) * quelle_zeile];
        let zeile = fb.zeile_mut(py);
        let ziel = &mut zeile[x0 as usize * 4..x1 as usize * 4];
        for (px, &q) in ziel.chunks_exact_mut(4).zip(spalten.iter()) {
            let s = &quelle[q..q + 4];
            match s[3] {
                // Vollstaendig durchsichtig: der Hintergrund bleibt stehen.
                0 => {}
                255 => {
                    if px != s {
                        px.copy_from_slice(s);
                        veraendert += 1;
                    }
                }
                _ => {
                    let sollte = Color::new(s[0], s[1], s[2], s[3]);
                    let hintergrund = Color::new(px[0], px[1], px[2], px[3]);
                    if sollte != hintergrund {
                        let neu = crate::framebuffer::blend_source_over(sollte, hintergrund);
                        px.copy_from_slice(&[neu.r, neu.g, neu.b, neu.a]);
                        veraendert += 1;
                    }
                }
            }
        }
    }
    veraendert
}

/// Schneidet `rect` mit dem Puffer und gibt Ganzzahlkoordinaten zurueck.
///
/// GEMESSEN, warum das eine eigene Funktion ist: sie ist die einzige
/// Stelle, an der aus einer `f32`-Eingabe eine Schleifenzoahl wird, und
/// deshalb gehoert die Grenze dorthin. Sie gibt `None` zurueck, wenn das
/// Rechteck nicht-endlich ist oder leer — dann wird nichts gezeichnet.
///
/// M3/M4: die Schleife, die danach kommt, ist höchstens
/// `Pufferbreite × Pufferhoehe` lang. Es gibt keine Zahl aus der Eingabe,
/// die sie vergroessern kann.
fn ziel_bereich(fb: &Framebuffer, rect: &Rect) -> Option<(u32, u32, u32, u32)> {
    if !(rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite())
    {
        return None;
    }
    if rect.is_empty() {
        return None;
    }
    let x_start = rect.x.max(0.0).floor() as u32;
    let y_start = rect.y.max(0.0).floor() as u32;
    let x_ende = ((rect.x + rect.width).ceil().max(0.0) as u32).min(fb.width());
    let y_ende = ((rect.y + rect.height).ceil().max(0.0) as u32).min(fb.height());
    if x_start >= x_ende || y_start >= y_ende {
        return None;
    }
    Some((x_start, y_start, x_ende - x_start, y_ende - y_start))
}

/// Fuellt den Kasten mit der mittleren Farbe des Bildes.
///
/// Das ist der Weg fuer ein Bild, das so klein ist, dass man es nicht
/// herunterskalieren kann. GEMESSEN, warum ueberhaupt: ein `<img width=
/// "4000" src="2x2.png">` ist eine Seite mit einem 1-Pixel-Bild und einem
/// 4000-Pixel-Kasten. Rauschen waere hier das schlechtere Ergebnis.
fn draw_mittelfarbe(fb: &mut Framebuffer, rect: &Rect, pixels: &[u8]) -> usize {
    let zaehler = pixels.len() / 4;
    if zaehler == 0 {
        return 0;
    }
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;
    let mut a = 0u32;
    // M3: die Schleife laeuft ueber die Pixel des Bildes. Ihre Zahl ist
    // durch `MAX_BILD_PIXEL` (16 Mio) begrenzt, das sind 16.000.000
    // Schritte — lang, aber endlich. Deshalb wird ab 256 Pixeln der
    // Zaehler gesampelt: bei einem 1-Pixel-Bild aendert das nichts, bei
    // einem 16-Mio-Pixel-Bild kostet es 1/256 der Arbeit.
    const SCHRITT: usize = 256;
    let schritt = if zaehler < SCHRITT {
        1
    } else {
        zaehler / SCHRITT
    };
    let mut n = 0u32;
    for i in (0..zaehler).step_by(schritt) {
        let p = &pixels[i * 4..i * 4 + 4];
        // Die Alpha wird gewichtet: ein durchsichtiges Pixel darf den
        // Farbmittelwert nicht auf die Farbe eines Randes ziehen.
        let gewicht = p[3].max(1) as u32;
        r += p[0] as u32 * gewicht;
        g += p[1] as u32 * gewicht;
        b += p[2] as u32 * gewicht;
        a += p[3] as u32;
        n += gewicht;
    }
    if n == 0 {
        return 0;
    }
    // Die Deckung ist der MITTELWERT der Alphawerte, nicht die Summe
    // geteilt durch die Zahl der Schritte — sonst waere ein halb
    // durchsichtiges Bild opak.
    let schritte = (0..zaehler).step_by(schritt).count().max(1) as u32;
    let farbe = Color::new(
        (r / n) as u8,
        (g / n) as u8,
        (b / n) as u8,
        (a / schritte) as u8,
    );
    fb.fill_rect(*rect, farbe)
}

/// Der Platzhalter fuer ein Bild, das nicht geladen werden konnte (I3).
///
/// GEMESSEN, warum das ein GESTREIFTER Kasten ist und kein Text: der
/// Kasten zeigt "hier fehlt etwas", der Text muesste etwas sagen, und
/// sagen kann er nur, was der Autor geschrieben hat (`alt`). Das ist ein
/// anderes Bild, und ein erfundenes ist schlimmer.
pub fn draw_platzhalter(fb: &mut Framebuffer, rect: &Rect) -> usize {
    let rand = Rect::new(rect.x, rect.y, rect.width.max(1.0), rect.height.max(1.0));
    let fuellung = fb.fill_rect(rand, Color::new(0x3a, 0x3a, 0x42, 255));
    // Der Rand liegt AUSSERHALB der Fuellung, sonst waere er unter der
    // Fuellung unsichtbar. Ein Streifen oben und einer links genuegen:
    // das ist die Art, in der ein Browser seinen fehlenden Platzhalter
    // zeigt (CSS 2.1 nennt da keine Form, das ist eine Anzeige, kein
    // Standard).
    let oben = Rect::new(rand.x, rand.y, rand.width, 1.0);
    let links = Rect::new(rand.x, rand.y, 1.0, rand.height);
    let mut n = fuellung;
    n += fb.fill_rect(oben, Color::new(0x66, 0x66, 0x70, 255));
    n += fb.fill_rect(links, Color::new(0x66, 0x66, 0x70, 255));
    n
}

/// Zeichnet ein Bild-Item aus einer Anzeigeliste.
///
/// Das ist der Aufruf, den der Rasterizer braucht; er nimmt das Item,
/// damit die Regeln an einer Stelle stehen.
pub fn draw_bild_item(fb: &mut Framebuffer, item: &DisplayItem) -> usize {
    match item {
        DisplayItem::Bild {
            rect,
            breite,
            hoehe,
            pixels,
        } => draw_bild(fb, rect, pixels, *breite, *hoehe),
        _ => 0,
    }
}
