//! `skalieren` und `erkenne` — die zwei Werkzeuge, die kein Dateiformat
//! kennen und deshalb immer gebraucht werden.

use crate::{Bild, BildFehler, Bildformat, MAX_KANTE, MAX_SPEICHER};

/// B9: Liest ein Bild **in Anzeigegroesse**.
///
/// Der Auftrag lautete: „grosse Bilder nur in Anzeigegroesse dekodieren/
/// skalieren“. Diese Funktion ist die Stelle, an der das entschieden wird.
///
/// GEMESSENER Weg pro Format (02.10.2026):
///
/// | Format | Weg | Beleg |
/// |---|---|---|
/// | JPEG ohne senkrechte Unterabtastung (4:4:4, 4:2:2, grau) und Ziel hoechstens ein Achtel | Gleichstrom, **kein** Volldekoder | `tests/jpeg_klein.rs`: 0 von 60 und 0 von 121500 Kanalwerten verschieden, groesste Abweichung 1 |
/// | JPEG mit senkrechter Unterabtastung (4:2:0, fast alle Fotos) | Volldekoder, dann Box-Filter | der Gleichstromweg weicht dort bis 37 von `djpeg -scale 1/8` ab (drei Filter durchgerechnet, keiner passt); siehe `src/jpeg/lesen.rs` |
/// | alle anderen Formate | Volldekoder, dann Box-Filter | PNG, WebP, GIF, BMP, PPM, SVG haben im Kopf keine Verkleinerungsangabe, aus der man beim Dekodieren folgen koennte; WebP-traegende verlustbehaftende Bilder haben sie auch nicht |
///
/// Der Box-Filter ist bewusst der guenstigste der beiden vorhandenen
/// (`crate::skalieren::Filter::Box` gegenueber `Lanczos3`): es geht um
/// Speicher und Zeit, und ein Verkleinern fuer die Anzeige verzeiht
/// Lanczos nicht, kostet aber dasselbe Speichern.
///
/// M3: die Zielmasse werden auf `MAX_KANTE` geklemmt, nie multipliziert
/// (es gibt kein Verhaeltnis, das hereingereicht wird), und ein Ziel von
/// 0 ergibt ein leeres Bild statt eines Absturzes.
pub fn lesen_klein(bytes: &[u8], ziel_breite: u32, ziel_hoehe: u32) -> Result<Bild, BildFehler> {
    let ziel_breite = ziel_breite.min(MAX_KANTE);
    let ziel_hoehe = ziel_hoehe.min(MAX_KANTE);
    if ziel_breite == 0 || ziel_hoehe == 0 {
        return Err(BildFehler::GroesseNull);
    }
    // 1) Der guenstige Weg: JPEG, Ziel hoechstens ein Achtel der Quelle.
    //
    // GEMESSENER Fehler beim ersten Wurf: ohne den Vergleich mit der Quelle
    // lieferte `lesen_klein` bei Ziel 2400x1800 ein 300x225-Bild. Das
    // Ziel wird darum mit `saturating_mul(8)` gegen den Kopf geprueft —
    // mit `saturating_mul`, weil ein Ziel von 2^29 sonst ueberlaeuft und
    // stillschweigend falsch verglichen wuerde.
    let achtel_reicht = match crate::jpeg::jpeg_masse(bytes) {
        Some((w, h)) => ziel_breite.saturating_mul(8) <= w && ziel_hoehe.saturating_mul(8) <= h,
        None => false,
    };
    // Release 08.10.2026 (Bild-Diaet): die groesste DCT-Stufe, die das Ziel
    // noch nicht unterschreitet — Viertel, sonst Halb. Danach wie bisher der
    // Kastenfilter auf das genaue Ziel. GEMESSEN vorher: alles zwischen
    // „halb so gross“ und „ein Achtel“ wurde VOLL dekodiert (bei 2400x1800
    // rund 17 MB RGBA plus 13 MB Koeffizienten) und erst dann verkleinert.
    let stufe = match crate::jpeg::jpeg_masse(bytes) {
        Some((w, h)) if !achtel_reicht => [crate::jpeg::Verkleinerung::Viertel, crate::jpeg::Verkleinerung::Halb]
            .into_iter()
            .find(|v| {
                let t = v.teiler();
                ziel_breite.saturating_mul(t) <= w && ziel_hoehe.saturating_mul(t) <= h
            }),
        _ => None,
    };
    if let (Some(Bildformat::Jpeg), Some(v)) = (erkenne(bytes), stufe) {
        match crate::jpeg::jpeg_lesen_mit(bytes, v) {
            Ok(bild) => {
                if bild.breite <= ziel_breite && bild.hoehe <= ziel_hoehe {
                    return Ok(bild);
                }
                return Ok(crate::skalieren::skalieren(
                    &bild,
                    ziel_breite,
                    ziel_hoehe,
                    crate::skalieren::Filter::Box,
                ));
            }
            Err(e) => return Err(e),
        }
    }
    if let Some(Bildformat::Jpeg) = erkenne(bytes) {
        if !achtel_reicht {
            let bild = lesen(bytes)?;
            if bild.breite <= ziel_breite && bild.hoehe <= ziel_hoehe {
                return Ok(bild);
            }
            return Ok(crate::skalieren::skalieren(
                &bild,
                ziel_breite,
                ziel_hoehe,
                crate::skalieren::Filter::Box,
            ));
        }
        match crate::jpeg::jpeg_lesen_mit(bytes, crate::jpeg::Verkleinerung::Achtel) {
            Ok(bild) => {
                // Das Ergebnis ist hoechstens ein Achtel; wenn das Ziel
                // noch kleiner ist, wird von dort aus verkleinert.
                if bild.breite <= ziel_breite && bild.hoehe <= ziel_hoehe {
                    return Ok(bild);
                }
                return Ok(crate::skalieren::skalieren(
                    &bild,
                    ziel_breite,
                    ziel_hoehe,
                    crate::skalieren::Filter::Box,
                ));
            }
            // GEMESSEN: bei 4:2:0 kommt genau dieser Fehler, und er ist
            // der Grund fuer den Volldekoder-Weg — kein Schlimmeres.
            Err(BildFehler::JpegVerkleinerungFalsch { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    // 2) Der allgemeine Weg: voll lesen, dann verkleinern.
    let bild = lesen(bytes)?;
    if bild.breite <= ziel_breite && bild.hoehe <= ziel_hoehe {
        return Ok(bild);
    }
    Ok(crate::skalieren::skalieren(
        &bild,
        ziel_breite,
        ziel_hoehe,
        crate::skalieren::Filter::Box,
    ))
}

/// Naechster Nachbar, kein Resampler./// Naechster Nachbar, kein Resampler.
///
/// Warum naechster Nachbar und nicht bilinear: der Browser zeigt eine
/// 4000-px-Seite in einem 800-px-Feld. Ein bilinearer Resampler braucht
/// Fuellraten ueber alle vier Nachbarn und macht bei einem 1-px-Text
/// farbige Ringe. Naechster Nachbar kann das nicht, kostet pro Pixel einen
/// Ganzzahl-Durchgang und liefert fuer ein Verkleinern exakt die
/// Originalfarben — kein Blenden, keine Ringe.
///
/// Faellt das Ziel groesser aus als das Bild, wird der naechste Nachbar
/// GLEICH genommen (es gibt keine Extrapolation). Das ist eine bewusste
/// Entscheidung: „Aufblaehen" mit Interpolation ist eine zweite
/// Interpolationsentscheidung, und der Auftrag sagt ausdruecklich, dass
/// diese Crate keine baut.
pub fn skalieren(bild: &Bild, breite: u32, hoehe: u32) -> Bild {
    let b = breite.min(MAX_KANTE);
    let h = hoehe.min(MAX_KANTE);
    if b == 0 || h == 0 {
        // Kein Fehler, weil die Signatur keinen Fehler zurueckgibt: ein
        // 0x0-Bild ist ein leeres Bild, kein Absturz.
        return Bild {
            breite: 0,
            hoehe: 0,
            rgba: Vec::new(),
        };
    }
    let punkte = (b as usize).saturating_mul(h as usize);
    if punkte.saturating_mul(4) > MAX_SPEICHER {
        // Auch hier: geklemmt statt paniked.
        return Bild::new(b, h);
    }

    let sb = bild.breite;
    let sh = bild.hoehe;
    let mut raus = Bild::new(b, h);
    if sb == 0 || sh == 0 {
        return raus;
    }

    // Ganzzahlig, damit es kein Rundungsflackern gibt: derselbe
    // Quellpixel deckt genau einen Zielbereich ab.
    if b <= sb && h <= sh {
        // Verkleinern: die Spalten-/Zeilenindizes werden vorberechnet,
        // damit pro Pixel nur noch zwei Additionen noetig sind.
        let mut xs = vec![0u32; b as usize];
        for x in 0..b as usize {
            xs[x] = ((x as u64 * sb as u64) / b as u64) as u32;
        }
        for y in 0..h {
            let sy = ((y as u64 * sh as u64) / h as u64) as u32;
            for x in 0..b {
                raus.setze_pixel(x as u32, y, bild.pixel(xs[x as usize], sy));
            }
        }
    } else {
        // Vergroessern (oder eine Achse vergroessern): hier gibt es nur
        // eine moegliche Antwort ohne Interpolation.
        for y in 0..h {
            let sy = ((y as u64 * sh as u64) / h as u64) as u32;
            for x in 0..b {
                let sx = ((x as u64 * sb as u64) / b as u64) as u32;
                raus.setze_pixel(x as u32, y, bild.pixel(sx, sy));
            }
        }
    }
    raus
}

/// Erkennt das Format an den ersten Bytes.
///
/// `None` heisst „unbekannt" und ist ein vollstaendiges Ergebnis, kein
/// Fehler: ein Browser fragt vor jedem Download, und die Antwort auf eine
/// Videodatei ist „kein Bild", nicht „Fehler".
pub fn erkenne(bytes: &[u8]) -> Option<Bildformat> {
    if bytes.len() >= 8 && bytes[..8] == crate::png::SIGNATUR {
        return Some(Bildformat::Png);
    }
    if bytes.len() >= 2 && bytes[0] == b'B' && bytes[1] == b'M' {
        return Some(Bildformat::Bmp);
    }
    if bytes.len() >= 2 && bytes[0] == b'P' {
        match bytes[1] {
            b'1'..=b'6' => return Some(Bildformat::Ppm),
            _ => return None,
        }
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        return Some(Bildformat::Jpeg);
    }
    // Klammer noetig: && bindet staerker als ||, sonst rutscht das
    // zweite "GIF89a" vor die Laenge und schneidet bei 5 Byte Panic.
    if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        return Some(Bildformat::Gif);
    }
    if bytes.len() >= 4
        && bytes[0] == 0x00
        && bytes[1] == 0x00
        && bytes[2] == 0x01
        && bytes[3] == 0x00
    {
        return Some(Bildformat::Ico);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Bildformat::Webp);
    }
    if ist_svg(bytes) {
        return Some(Bildformat::Svg);
    }
    if ist_avif(bytes) {
        return Some(Bildformat::Avif);
    }
    None
}

/// Erkennt AVIF an der `ftyp`-Box.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B8): `ffmpeg -c:v libaom-av1
/// -still-picture 1` liefert eine Datei, die mit `....ftypavif` beginnt
/// (683 Byte fuer 64x48 Pixel). Nach ISO/IEC 23000-22 steht die
/// Kompatibilitaetsliste direkt hinter der Hauptmarke; `avif` (Bilder)
/// und `avis` (Sequenzen) zaehlen beide.
///
/// M3: gelesen werden hoechstens 64 Byte — die Box steht am Anfang, und
/// eine Datei, die erst viel spaeter `ftyp` sagt, ist keine.
fn ist_avif(bytes: &[u8]) -> bool {
    const KOPF: usize = 12; // 4 Groesse, 4 Typ, 4 Hauptmarke
    if bytes.len() < KOPF + 4 {
        return false;
    }
    if &bytes[4..8] != b"ftyp" {
        return false;
    }
    let haupt = &bytes[8..12];
    if haupt == b"avif" || haupt == b"avis" {
        return true;
    }
    // Kompatibilitaetsmarken: alle 4 Byte bis hoechstens 64 Byte.
    let ende = bytes.len().min(64);
    let mut p = 12;
    while p + 4 <= ende {
        if &bytes[p..p + 4] == b"avif" || &bytes[p..p + 4] == b"avis" {
            return true;
        }
        p += 4;
    }
    false
}

/// Erkennt eine SVG-Datei an ihrem Anfang.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B-HN): `news.ycombinator.com`
/// liefert als erstes Bild `y18.svg`; vorher stand SVG in keiner
/// Formatliste, `erkenne` gab `None` und `lesen` „kein bekanntes
/// Bildformat“. Die Datei selbst ist 315 Byte Text mit `<svg …>`.
///
/// SVG hat keine Binaersignatur, also wird der Anfang gelesen: ein
/// optionales BOM, dann Leerraum, dann in beliebiger Reihenfolge
/// `<?xml …?>`, `<!DOCTYPE …>`, `<!-- … -->` und schliesslich `<svg`.
///
/// M3: hoechstens [`SVG_PROLOG_MAX`] Byte werden angesehen. Eine
/// Datei, die erst hinter dieser Grenze ein `<svg` bringt, gilt als kein
/// Bild — dann ist „kein bekanntes Bildformat“ die Antwort, und das ist
/// besser als ein Rasteren von Text, der wie eine Bilddatei beginnt.
const SVG_PROLOG_MAX: usize = 4096;

/// Gibt es ein `<svg` im erlaubten Anfang? Siehe [`SVG_PROLOG_MAX`].
fn ist_svg(bytes: &[u8]) -> bool {
    let n = bytes.len().min(SVG_PROLOG_MAX);
    let mut i = 0usize;
    // BOM
    if n >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        i = 3;
    }
    let s = match std::str::from_utf8(&bytes[..n]) {
        Ok(t) => t,
        // Abgeschnittenes UTF-8 am Fensterende (der Kasten endet mitten
        // in einem Zeichen): der gueltige Anfang ist trotzdem lesbar.
        // `from_utf8_unchecked` waere hier der falsche Weg, deshalb
        // schneiden wir am gueltigen Ende ab.
        Err(e) => match std::str::from_utf8(&bytes[..e.valid_up_to()]) {
            Ok(t) => t,
            Err(_) => return false,
        },
    };
    let b = s.as_bytes();
    let mut p = i;
    loop {
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= b.len() {
            return false;
        }
        if b[p] == b'<' {
            let rest = &b[p..];
            if rest.starts_with(b"<svg") {
                // `<svg` gefolgt von Leerraum, `>` oder `/` — sonst ist
                // es ein Element, das nur so beginnt (`<svgx`).
                let nach = rest.get(4).copied();
                if nach.is_none_or(|c| {
                    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b'>' || c == b'/'
                }) {
                    return true;
                }
                return false;
            }
            if rest.starts_with(b"<?") {
                match find(rest, b"?>") {
                    Some(k) => p += k + 2,
                    None => return false,
                }
                continue;
            }
            if rest.starts_with(b"<!--") {
                match find(rest, b"-->") {
                    Some(k) => p += k + 3,
                    None => return false,
                }
                continue;
            }
            if rest.starts_with(b"<!") {
                // DOCTYPE: schachtelbare spitze Klammern ([ ... ]).
                match rest.iter().position(|&c| c == b'>') {
                    Some(k) => p += k + 1,
                    None => return false,
                }
                continue;
            }
        }
        return false;
    }
}

/// Index des ersten Vorkommens von `nadel` ab `ab`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&k| &hay[k..k + needle.len()] == needle)
}

/// Liest ein Bild in einem beliebigen der unterstuetzten Formate.
///
/// Praktisch fuer den Browser: `erkenne` sagt, welches es ist, und dann
/// wird genau das gelesen. Der Fehler bleibt BENANNT, auch wenn der
/// Aufrufer das Format geraten hat und danebenlag.
pub fn lesen(bytes: &[u8]) -> Result<Bild, BildFehler> {
    match erkenne(bytes) {
        Some(Bildformat::Png) => crate::png_lesen(bytes),
        Some(Bildformat::Bmp) => crate::bmp_lesen(bytes),
        Some(Bildformat::Ppm) => crate::ppm_lesen(bytes),
        Some(Bildformat::Jpeg) => crate::jpeg_lesen(bytes),
        Some(Bildformat::Gif) => crate::gif_lesen(bytes),
        Some(Bildformat::Ico) => crate::ico_lesen(bytes),
        Some(Bildformat::Webp) => crate::webp_lesen(bytes),
        Some(Bildformat::Avif) => Err(BildFehler::AvifNichtGelesen {
            // GEMESSEN 02.10.2026 (Rolle BILD, B8): der Container wird an
            // der `ftyp`-Box erkannt (Marke `avif` oder `avis`, dazu die
            // Kompatibilitaetsmarken), der Bildstrom ist AV1-Intra. Der
            // Dekoder dafuer ist nicht da — der Aufrufer soll aber
            // „Format nicht unterstuetzt“ sagen und nicht „kein
            // bekanntes Bildformat“, das liest sich wie eine kaputte Datei.
            //
            // GEMESSEN ist auch, dass der Container selbst lesbar ist:
            // `crate::avif::angaben(bytes)` liefert Groesse, Bittiefe und
            // Farbraum, ohne einen Bildpunkt zu dekodieren. Der Fehler
            // bleibt `&'static str`, deshalb steht die Beschreibung nicht
            // hier, sondern der Aufrufer holt sie sich bei Bedarf selbst.
            grund: "AV1-Intra-Dekoder fehlt",
        }),
        Some(Bildformat::Svg) => {
            // SVG ist Text. `from_utf8_lossy` ersetzt ungueltige Bytes
            // durch U+FFFD, statt die ganze Datei zu verwerfen — genau
            // so, wie ein Browser eine als UTF-8 deklarierte Seite
            // behandelt.
            let quelle = String::from_utf8_lossy(bytes);
            let s = crate::svg::svg_lesen(&quelle)?;
            crate::svg::svg_rasteren(&s)
        }
        None => Err(BildFehler::KeinBildformat),
    }
}
