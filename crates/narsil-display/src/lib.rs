//! Display List: die zeichnerische Repräsentation eines Dokuments.
//!
//> Die Display List ist die Schnittstelle zwischen Layout und Renderer.
//! Sie enthaelt ausschliesslich, WAS gezeichnet wird, in welcher Reihenfolge —
//! nie WIE. Dadurch kann:
//!   * `narsil-raster` die Liste direkt auf der CPU rastern,
//!   * ein spaeterer GPU-Renderer sie als Draw-Calls interpretieren,
//!   * ein Test sie serialisieren und pixelgenau vergleichen.
//!
//> ARCHITEKTURREGEL: Diese Datei kennt kein Narsil, keine CSS-Property-Namen
//! und keine Layout-Entscheidungen. Sie kennt nur `narsil_core`-Primitive.

use std::sync::Arc;

use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_core::style::{MAX_SCHATTEN, Rahmen, SchattenArt};
use narsil_text::{BitmapFont, measure};

/// Release 09.10.2026: die Marke fuer LEISTENTEXT (Tabs, Adressleiste,
/// Menues) in `DisplayItem::Text::schriftfamilie`.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Familie und kein neues Feld: ein Feld
/// in `Text` waere eine Aenderung an jeder der ueber 300 Stellen, die Text
/// bauen, in fuenf Crates. Die Familienliste laeuft schon bis zum Raster,
/// und CSS Fonts 4 §4.2 ueberspringt unbekannte Namen — die Schrift bleibt
/// `sans-serif`. Der Raster liest die Marke und darf dort Subpixel-Text
/// zeichnen (deckender Grund, Leiste); eine Seite kann sie nicht setzen,
/// ohne den Namen zu kennen, und dann zeichnet sie nur schaerfer.
pub const LEISTEN_MARKE: &str = "x-narsil-leiste";

/// Die Familienliste fuer Leistentext: die Marke, dann `sans-serif`.
pub const LEISTEN_SCHRIFT: &str = "x-narsil-leiste, sans-serif";

/// Eine erkannte Trefferziel-Taste bei einer Trefferpruefung.
///
/// Unterscheidet zwischen textueller Interaktion und klickbarem
/// Hintergrund. Eine Browser-Oberflaeche benoetigt beides: Klicks auf Text
/// duerfen zum Textknoten fuehren, Klicks auf leeren Hintergrund sollen
/// None zurueckliefern (nicht jeder Pixel ist anklickbar).
#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    /// Der Klick traf auf Text. Der enthaltene Origin zeigt, wo der Text
    /// im Dokument beginnt (bzw. wo er visuell positioniert ist).
    Text { origin: Point, text: String },
    /// Es traf nichts Aufklaerbares. Ein Klick auf leeren Hintergrund.
    None,
}

/// Durchsucht eine Display-Liste nach dem obersten Element an der gegebenen
/// Stelle. Die Reihenfolge der Liste wird beachnet: spaeter stehende Items
/// liegen ueber frueheren, das erste Treffer-Ergebnis ist also das obenste.
///
/// Diese Funktion hat **keine eigene Formel** fuer Breitenberechnungen.
/// Nutzen Sie `narsil_text::measure`, um die Breite eines Text-Items zu
/// ermitteln — genau der Fehler, den es bei `bounds()` gab, soll hier nicht
/// wiederholt werden.
///
/// Trefferpruefung:
///   - `DisplayItem::Rect`: Punkt liegt innerhalb des Rechtecks.
///   - `DisplayItem::Text`: Punkt liegt innerhalb der Text-Bounding-Box.
///     Die Breite wird ueber `narsil_text::measure` objektiv gemessen.
///   - `DisplayItem::Bild` (I3): Punkt liegt im Bildrechteck. Es trifft
///     wie ein Rechteck, denn ein Bild ist kein Ziel — ein `<a><img></a>`
///     traegt die Adresse am umgebenden Knoten, nicht am Bild.
/// Verschiebt ein Zeichenelement um `dy` Pixel.
///
/// BILD_29. **GEMESSENER GRUND FUER DIE EXHAUSTIVE FORM:** es gibt hier
/// **keinen** `_`-Zweig. Ein neuer Fall in `DisplayItem` macht damit den Build
/// **ROT**, statt still in eine Vorgabe zu laufen — das ist die bessere
/// Reihenfolge, und `narsil/src/sitzung.rs` hat sie inzwischen auch (dort mit
/// derselben Begruendung).
///
/// **UND DER PLAN, DEN ICH NICHT AUSFUELLE:** `narsil::verschiebe` macht
/// dasselbe. Solange beide existieren, ist das **zwei `match` ueber
/// `DisplayItem`** — und genau davor warnt der Kommentar in `narsil`. **Die
/// Entschraenkung ist der naechste Schritt** (`narsil::verschiebe` delegiert an
/// diese Funktion). Bis dahin ist der Fehler **beim Build** sichtbar und nicht
/// beim Bild.
pub fn schiebe(item: &DisplayItem, dy: f32) -> DisplayItem {
    match item {
        DisplayItem::Rect {
            rect,
            color,
            radius,
            deckkraft,
            schatten,
            schatten_anzahl,
        } => DisplayItem::Rect {
            rect: Rect {
                x: rect.x,
                y: rect.y + dy,
                width: rect.width,
                height: rect.height,
            },
            color: *color,
            radius: *radius,
            deckkraft: *deckkraft,
            schatten: *schatten,
            schatten_anzahl: *schatten_anzahl,
        },
        //
        // BILD_32: der Rahmen wandert **mit** — er liegt am Kasten, und ein
        // stehen gebliebener Rahmen waere ein Ring an der alten Stelle. Die
        // Dicke aendert sich nicht.
        DisplayItem::Rand {
            rect,
            rahmen,
            radius,
        } => DisplayItem::Rand {
            rect: Rect {
                x: rect.x,
                y: rect.y + dy,
                width: rect.width,
                height: rect.height,
            },
            rahmen: *rahmen,
            radius: *radius,
        },
        // 04.10.2026 (SCHRIFT): BAUFIX E0004 nach `DisplayItem::Verlauf`
        // (BILD_48 Station 3, fremde Variante). GEMESSEN, warum hier der
        // Zweig fehlte und der Build dadurch rot wurde: diese Funktion
        // ist **absichtlich** exhaustiv (BILD_29 im Kommentar darueber:
        // „ein neuer Fall in `DisplayItem` macht damit den Build ROT,
        // statt still in eine Vorgabe zu laufen"). Der Verlauf wandert
        // wie das Rechteck — er liegt am Kasten, und ein stehen
        // gebliebener Verlauf waere eine Farbflaeche an der alten Stelle.
        DisplayItem::Verlauf {
            rect,
            radius,
            verlauf,
        } => DisplayItem::Verlauf {
            rect: Rect {
                x: rect.x,
                y: rect.y + dy,
                width: rect.width,
                height: rect.height,
            },
            radius: *radius,
            verlauf: verlauf.clone(),
        },
        DisplayItem::Text {
            schriftfamilie,
            origin,
            text,
            color,
            font_size,
            deko,
            gewicht,
            kursiv,
        } => DisplayItem::Text {
            // Release 08.10.2026: die Familie WANDERT MIT. Hier stand
            // `String::from("sans-serif")` mit der Annahme, kein Aufrufer
            // trage eine eigene Familie — aber `narsil` verschiebt die ganze
            // Seitenliste um den Scroll-Versatz (`schiebe_alle`). GEMESSEN:
            // jede Seitenschrift (Serif, `monospace`, Georgia …) wurde in
            // Segoe UI gezeichnet und mit der echten Familie gemessen —
            // Code-Bloecke mit Luecken zwischen den Woertern, Inline-`code`
            // mit zu breiten Kaesten.
            schriftfamilie: schriftfamilie.clone(),
            origin: Point {
                x: origin.x,
                y: origin.y + dy,
            },
            text: text.clone(),
            color: *color,
            font_size: *font_size,
            deko: *deko,
            gewicht: *gewicht,
            kursiv: *kursiv,
        },
        DisplayItem::Bild {
            rect,
            breite,
            hoehe,
            pixels,
        } => DisplayItem::Bild {
            rect: Rect {
                x: rect.x,
                y: rect.y + dy,
                width: rect.width,
                height: rect.height,
            },
            breite: *breite,
            hoehe: *hoehe,
            pixels: Arc::clone(pixels),
        },
    }
}

pub fn hit_test(list: &DisplayList, point: Point) -> Option<HitTarget> {
    // Iteriere umgekehrt: spaeter liegende Items liegen ueber frueheren.
    for (index, item) in list.iter().enumerate().rev() {
        if let Some(clip) = list.clip_at(index) {
            if clip.is_empty()
                || point.x < clip.x
                || point.x > clip.right()
                || point.y < clip.y
                || point.y > clip.bottom()
            {
                continue;
            }
        }
        match item {
            DisplayItem::Rect { rect, .. }
            | DisplayItem::Bild { rect, .. }
            // BILD_48, Station 3: der Verlauf **fuellt** den Kasten wie `Rect`
            // (anders als der Rahmen, der nur ein Ring ist) — er ist also
            // genauso **anklickbar**.
            | DisplayItem::Verlauf { rect, .. }
            // BILD_32: der Rahmen ist Teil des Kastens und **anklickbar** — ein
            // Klick auf den Rand eines Eingabefeldes setzt den Fokus, das ist
            // die erwartete Bedienung.
            | DisplayItem::Rand { rect, .. } => {
                // NICHT-ENDLICHE RECHTECKE TREFFEN NICHTS.
                //
                // GEMESSEN am 30.09.2026, 08:40 (`tests/bomben.rs`):
                // `Rect { width: INFINITY }` traf JEDEN Punkt, weil
                // `point.x <= rect.x + rect.width + 1.0` bei `inf` immer
                // gilt. Ein Rechteck mit `NaN` war dagegen unsichtbar —
                // `NaN` vergleicht sich mit allem als `false`. Zwei
                // kaputte Werte, zwei entgegengesetzte Fehler: einer
                // schluckt jeden Klick der Seite, der andere keinen.
                //
                // Woher die Werte kommen: `width: calc(100% - 100%)` oder
                // ein `font-size: 1e40px` in einer fremden Crate. Der
                // Aufrufer kann sie nicht sehen, also faengt es diese
                // Crate ab — an der einen Stelle, an der sie entstehen.
                if !(rect.x.is_finite()
                    && rect.y.is_finite()
                    && rect.width.is_finite()
                    && rect.height.is_finite())
                {
                    continue;
                }
                // Punkt-gegen-Rechteck-Check (inklusive Randerweiterung)
                if point.x >= rect.x - 1.0
                    && point.x <= rect.x + rect.width + 1.0
                    && point.y >= rect.y - 1.0
                    && point.y <= rect.y + rect.height + 1.0
                {
                    return Some(HitTarget::None);
                }
            }
            DisplayItem::Text {
                schriftfamilie: _,
                origin,
                text,
                font_size,
                color: _,
                deko: _,
                // 04.10.2026 (SCHRIFT): `gewicht`/`kursiv` sind fuer die
                // Trefferfrage **ohne** Bedeutung — die Breite wird hier
                // mit der Bitmap-Schrift gemessen, und die ist fuer jeden
                // Schnitt dieselbe. Sie werden mit `_` genannt, weil das
                // Muster alle Felder nennen muss (E0027).
                    gewicht: _,
                    kursiv: _,
            } => {
                // Breite mit narsil_text::measure messen (keine 0.6-em-Formel!)
                let font = BitmapFont::builtin();
                let metrics = measure(&font, text, *font_size)
                    .expect("Messfehler: Text und Schriftgroesse sind gueltig");
                let text_width = metrics.width;
                let text_height = *font_size;

                // Check, ob der Punkt im Text-Bounding-Box liegt
                // Text beginnt bei origin und erstreckt sich right/down
                if point.x >= origin.x - 1.0
                    && point.x <= origin.x + text_width + 1.0
                    && point.y >= origin.y - 1.0
                    && point.y <= origin.y + text_height + 1.0
                {
                    return Some(HitTarget::Text {
                        origin: *origin,
                        text: text.clone(),
                    });
                }
            }
        }
    }
    None
}

/// Release 12.10.2026 (Agent CSS2): gehoert `c` zu einer verbundenen
/// (kursiven) Schrift — Arabisch, Syrisch, N'Ko, Mandaeisch, Mongolisch,
/// Phagspa, Adlam, Hanifi Rohingya? Dort bekommen die Zeichen KEINEN
/// Buchstabenabstand (CSS Text 3 §8.2: "UAs should not apply letter-spacing
/// to cursive scripts"). Layout (Breite) und Raster (Lage) fragen dasselbe.
pub fn ohne_buchstabenabstand(c: char) -> bool {
    matches!(
        c as u32,
        0x0600..=0x074F | 0x0750..=0x077F | 0x07C0..=0x07FF | 0x0840..=0x085F | 0x08A0..=0x08FF
            | 0x1800..=0x18AF | 0xA840..=0xA87F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF
            | 0x10D00..=0x10D3F | 0x1E900..=0x1E95F
    )
}

/// Release 12.10.2026 (Agent CSS2): haengt `c` an das Zeichen davor (dieselbe
/// typografische Einheit, CSS Text 3 §8.2: der Abstand faellt zwischen
/// Einheiten, nicht in ein Graphem)? Kombinierende Zeichen, Vokalzeichen und
/// Virama der indischen Schriften, Joiner und Variantenwaehler; nach einem
/// Virama oder ZWJ auch der Folgebuchstabe (Ligatur/Konjunkt). GEMESSEN vorher
/// (WPT `css-text/letter-spacing/letter-spacing-bengali-yaphala-001`): das
/// Ya-phala wurde von seinem Konsonanten getrennt gezeichnet.
pub fn bindet_an_vorheriges(vorher: char, c: char) -> bool {
    let indisch_zeichen = |c: char| {
        let u = c as u32;
        (0x0900..=0x0DFF).contains(&u) && {
            let o = (u - 0x0900) % 0x80;
            matches!(o, 0x00..=0x03 | 0x3A..=0x4F | 0x51..=0x57 | 0x62..=0x63)
        }
    };
    let virama = |c: char| {
        let u = c as u32;
        (0x0900..=0x0DFF).contains(&u) && (u - 0x0900) % 0x80 == 0x4D
    };
    matches!(c as u32, 0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05C7 | 0x064B..=0x065F | 0x0670
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C..=0x200D | 0x20D0..=0x20FF | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F | 0x1F3FB..=0x1F3FF | 0xE0100..=0xE01EF)
        || indisch_zeichen(c)
        || virama(vorher)
        || vorher == '\u{200d}'
}

/// Release 12.10.2026 (Agent CSS2): die Teile eines Laufs mit
/// `letter-spacing` (je Zeichen; verbundene Schriften bleiben zusammen) bzw.
/// nur `word-spacing` (je Wort bis einschliesslich des Leerzeichens) — als
/// Byte-Bereiche, und je Teil die Zugabe danach (CSS Text 3 §8: `ls` nach
/// jedem Zeichen ausser verbundenen, `ws` nach jedem Leerzeichen). Das Layout
/// rechnet daraus die Lage der Teile, der Raster zeichnet sie dort.
pub fn abstands_teile(text: &str, ls: f32, ws: f32) -> Vec<(usize, usize, f32)> {
    let ls = if ls.is_finite() { ls } else { 0.0 };
    let ws = if ws.is_finite() { ws } else { 0.0 };
    if text.is_empty() || (ls == 0.0 && ws == 0.0) {
        return Vec::new();
    }
    let mut raus = Vec::new();
    let mut anfang = 0usize;
    let mut zeichen = text.char_indices().peekable();
    while let Some((i, c)) = zeichen.next() {
        let ende = i + c.len_utf8();
        let leer = matches!(c, ' ' | '\u{a0}');
        let verbunden = zeichen.peek().is_some_and(|(_, n)| {
            (ohne_buchstabenabstand(c) && ohne_buchstabenabstand(*n)) || bindet_an_vorheriges(c, *n)
        });
        if (ls != 0.0 && !verbunden) || leer || ende == text.len() {
            let teil = &text[anfang..ende];
            // Eine typografische Einheit je Teil; verbundene Schrift ohne.
            let n = if teil.chars().last().is_some_and(ohne_buchstabenabstand) { 0.0 } else { 1.0 };
            let zugabe = ls * n + if teil.ends_with([' ', '\u{a0}']) { ws } else { 0.0 };
            raus.push((anfang, ende, zugabe));
            anfang = ende;
        }
    }
    raus
}

/// Ein zu zeichnendes Primitiv.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    /// Gefuelltes Rechteck (Hintergrund, Rahmen, Block).
    /// Ein gefuelltes Rechteck (Hintergrund, Rahmen, Block).
    ///
    /// S2 Station 3: `radius`, `deckkraft` und `schatten` kommen aus der
    /// Kaskade (`narsil_core::Style`) und laufen unveraendert mit. Sie
    /// stehen HIER und nicht in einem eigenen Listen-Eintrag, weil ein
    /// Hintergrund, eine Ecke und ein Schatten dieselbe Form haben: sie
    /// gehoeren an dasselbe Rechteck. Ein zweiter Eintrag muesste vom
    /// Zeichner wieder zusammengesucht werden, und die Reihenfolge der
    /// Ebenen (Hintergrund, dann Schatten, dann Inhalt) waere dann eine
    /// Sache der Liste statt eine Sache des Zeichners.
    Rect {
        rect: Rect,
        color: Color,
        /// `border-radius` in px: oben-links, oben-rechts, unten-rechts,
        /// unten-links. 0 = eckig.
        radius: [f32; 4],
        /// `opacity` in [0,1]. 1 = deckend.
        ///
        /// Wirkt auf die ENGEFARBE dieses Rechtecks, nicht auf die Kinder —
        /// CSS 2.1 §11.13: eine Gruppen-Deckkraft wird erst beim Wurzel-
        /// element auf eine ebene Gruppe angewendet. Das Umrechnen der
        /// Gruppenform macht `narsil-layout-engine`, sobald es sie braucht.
        deckkraft: f32,
        /// `box-shadow`, hoechstens `MAX_SCHATTEN` Eintraege. Der Rest
        /// von `schatten` ist Fuellflaeche und wird nie gelesen.
        ///
        /// Der Typ ist `narsil_core::style::SchattenArt` und nicht
        /// `narsil_css::Schatten`: `narsil-core` darf `narsil-css` nicht kennen.
        schatten: [SchattenArt; MAX_SCHATTEN],
        /// Wie viele Eintraege wirklich belegt sind (0 bis `MAX_SCHATTEN`).
        schatten_anzahl: u8,
    },
    /// Der **Rahmen** eines Kastens, als eigener Eintrag (BILD_32, Schritt 4).
    ///
    /// **WARUM EIN EIGENER EINTRAG UND KEIN FELD AN `Rect`:** `Rect` traegt
    /// `radius`, `deckkraft` und `schatten` — **die drei Werte, die die Form
    /// AENDERN** (die Ecken runden, das Rechteck wird durchsichtig, ausserhalb
    /// liegt ein Schatten). **Der Rahmen aendert die Form nicht**, er liegt
    /// **darum herum**: Er hat eine **Dicke**, die am Kasten abgezogen werden
    /// muss, bevor die Fuellung gerechnet wird, und bei `border-radius` ist er
    /// ein **Ring**, nicht ein Rechteck. Ein Fuell-`Rect` und ein Ring sind
    /// **zwei Formen**, und sie gehoeren in **zwei Eintraege** — sonst muesste
    /// der Zeichner an einem Item entscheiden, ob er Fuellung oder Ring malt,
    /// und diese Entscheidung gehoert in die Liste, nicht in den Zeichner.
    ///
    /// **GEMESSENER VORGANG:** `narsil-core::style::Rahmen` wurde in BILD_32
    /// Schritt 1 gebaut, `stil_von_css` fuellt es (Schritt 2a). Es gab **vorher
    /// keinen** Rahmenpfad: `narsil-css` parst `border`/`-width`/`-style`/
    /// `-color`, `Style` hatte **kein** Feld, und `push_box_items`
    /// (`narsil-layout-engine/src/display.rs:358`) erzeugte **ausschliesslich**
    /// Hintergrund, Bilder und Text.
    ///
    /// **DIE `radius`-ANGABE STEHT HIER, WEIL DER RAHMEN EIN RING IST:**
    /// `border-radius: 24px` mit `border: 8px solid` malt **einen Ring mit
    /// denselben 24 px** — die Radiuskanten liegen an der **AUSSEN**kante des
    /// Rings, nicht am Fuellrechteck.
    Rand {
        /// Der **Rahmenkasten** (`border-box`), in Dokumentkoordinaten.
        rect: Rect,
        /// Die vier Kanten in CSS-Reihenfolge (oben, rechts, unten, links).
        rahmen: Rahmen,
        /// `border-radius` in px, dieselbe Reihenfolge wie in `Rect`.
        radius: [f32; 4],
    },

    /// **BILD_48, Station 3: der Verlauf als eigener Eintrag — `NEBEN` dem
    /// Hintergrund, nicht darin.**
    ///
    /// **WARUM EIN EIGENER EINTRAG, und nicht im Hintergrund:** das ist genau
    /// der Grund, aus dem auch [`DisplayItem::Rand`] ein eigener Eintrag ist
    /// (siehe dessen Doku). `background-color: transparent;
    /// background-image: linear-gradient(...)` ist eine **ganz normale**
    /// Kombination — und bei **transparentem** Grund ist der Hintergrund
    /// **gar nicht** da (`flow.rs`: `background` ist `None`). Haette der Verlauf
    /// **am** Hintergrund gehaengen, waere er **genau dann weg** — also bei
    /// **jedem** Verlauf ueber transparentem Grund.
    ///
    /// **GEMESSENER VORGANG, genau wie beim Rahmen:** `narsil-core` hat den Typ
    /// (`verlauf_typ.rs`, BILD_44), `narsil-page` fuellt `Style::verlauf`
    /// (BILD_48 Station 2), und es gab **null** Stellen, die daraus einen
    /// Eintrag machen — **0** Aufrufe von `verlauf::zeichne` ausserhalb der
    /// Messkiste (BILD_47).
    Verlauf {
        /// Der Kasten, auf den der Verlauf gezeichnet wird — der **Füllrechteck**,
        /// und der Verlauf **füllt** ihn (anders als der Rahmen, der ein Ring ist).
        rect: Rect,
        /// **Wichtig, weil der Verlauf eine Füllung ist:** `border-radius` schneidet
        /// den Verlauf an **denselben** Kanten ab wie den Hintergrund — es steht
        /// hier, damit der Zeichner es **anwenden** muss.
        radius: [f32; 4],
        /// Der Verlauf selbst, aus `narsil_core::verlauf_typ::Verlauf`.
        verlauf: narsil_core::verlauf_typ::Verlauf,
    },
    /// Text an einer Basisposition.
    Text {
        origin: Point,
        text: String,
        color: Color,
        font_size: f32,
        /// Textdekoration als Bitmaske (A3): 1 = `underline`,
        /// 2 = `overline`, 4 = `line-through`. 0 = keine.
        ///
        /// Sie kommt aus der Kaskade und laeuft unveraendert mit. Der
        /// Rasterizer zeichnet 1 und 4; 2 wird bewusst nicht gezeichnet.
        deko: u8,
        /// `font-weight`, 100..=900 (CSS Fonts 4 §5.3), Vorgabe 400.
        ///
        /// GEMESSEN 04.10.2026 (SCHRIFT, `texttest.html` gegen Firefox),
        /// warum das Feld noetig ist und nicht nur bequem: **alle vier**
        /// Stilzeilen einer Seite kamen im Browser gleich breit an —
        /// Firefox 127 / 144 / 129 / 143 px fuer normal / fett / kursiv /
        /// fett-kursiv, narsil **125 / 125 / 125 / 125**. Ueberschriften,
        /// `<b>`, `<strong>`, `<em>`, `<th>` — der ganze Text einer Seite
        /// wurde aufrecht gezeichnet, weil die Angabe nirgends stand.
        ///
        /// GEMESSEN, dass die Kaskade sie **hat**: `narsil_css::Berechnet`
        /// fuehrt `schriftgewicht` (typen.rs:1852) und `schriftstil`
        /// (1853), mit Vererbung. Es fehlte der Weg ab dort.
        ///
        /// GEMESSEN, dass der Zeichenweg sie **umsetzen kann**:
        /// `Schrift::mit_gewicht(700, false)` liefert GEMESSEN
        /// `DejaVuSans-Bold.ttf`, und 700/false/700-true liefern
        /// Bold/Oblique/BoldOblique — Beispiel `a49_stil_bild.rs`.
        gewicht: u16,
        /// `font-style: italic` oder `oblique` (CSS Fonts 4 §5.2).
        /// Vorgabe `false` = `normal`.
        ///
        /// GEMESSEN wie bei `gewicht`: Firefox 129 px fuer die kursive
        /// Zeile, narsil 125 — die Zeile war aufrecht, weil `italic` nicht
        /// ankam. Und GEMESSEN, dass die echten Schnitte existieren und
        /// greifen (`DejaVuSans-Oblique.ttf`, `erfuellt = true`).
        kursiv: bool,
        /// `font-family` als CSS-Liste (CSS Fonts 4 §4.2), Vorgabe
        /// `sans-serif`.
        ///
        /// GEMESSEN 04.10.2026 (SCHRIFT, (c)): `code`, `kbd` und `samp`
        /// standen bei GEMESSEN 14,4 px in `DejaVuSans.ttf` — der
        /// Fliesstextschrift — obwohl das UA-Blatt fuer sie
        /// `monospace` sagt (`narsil-css/src/ua.rs`) und die Kaskade die
        /// Familie berechnet (`Berechnet::schriftfamilie`). Der Verlust
        /// hatte **fuenf** Stellen, und `DisplayItem::Text` war die letzte
        /// davon: gewichtet und kursiv kamen an, die Familie nicht.
        ///
        /// GEMESSEN, warum eine **Liste** und kein einzelner Name: CSS
        /// Fonts 4 §4.2 nennt die Liste ausdruecklich, und der erste
        /// Eintrag, fuer den es eine Schrift gibt, gewinnt. Wer hier
        /// vorzeitig einen Namen waehlt, nimmt dem Raster die Fallback-
        /// Kette.
        schriftfamilie: String,
    },
    /// Ein Bild, auf `rect` skaliert (I3).
    ///
    /// # WARUM DIE PIXEL IN DER LISTE UND NICHT IM DOM
    ///
    /// Ein `<img>`-Knoten traegt im Dokument seine Adresse, nicht seine
    /// Pixel: die Pixel entstehen beim LADEN, und das Laden ist Sache des
    /// Browsers (`narsil`), nicht des Layouts. Der Layout-Baum kennt die
    /// Groesse (H6 reserviert Platz), der Browser die Farben. Wer beides
    /// im Dokument zusammenhaelt, baut eine Kopie: einmal im Knoten fuer
    /// das Layout und einmal in der Liste fuer das Zeichnen. Bei einem
    /// 2-MB-Bild sind das 4 MB, und die Kopie ist der Grund, warum der
    /// Speicher nicht mit der Seite waechst.
    ///
    /// `Arc` ist hier kein Optimismus, sondern die Bedingung: ein Bild
    /// wird fuer JEDEN Frame an die Liste gehaengt. Ohne `Arc` waere das
    /// eine Kopie pro Frame — bei 30 Bildern und 60 Frames sind das 1800
    /// Kopien pro Sekunde.
    Bild {
        rect: Rect,
        /// Die Bildbreite in Pixeln (`pixels` hat `breite * hoehe * 4`).
        breite: u32,
        /// Die Bildhoehe in Pixeln.
        hoehe: u32,
        /// RGBA8, vier Byte je Pixel, Zeile fuer Zeile.
        pixels: std::sync::Arc<Vec<u8>>,
    },
}

impl DisplayItem {
    /// Kurzer Typname, nuetzlich fuer Debug-Ausgaben.
    pub const fn kind_name(&self) -> &'static str {
        match self {
            DisplayItem::Rect { .. } => "rect",
            // BILD_32: **deutsch wie die anderen** — `rand`, nicht `rahmen`,
            // damit die Debug-Ausgaben eine Sprache sprechen.
            DisplayItem::Rand { .. } => "rand",
            // BILD_48, Station 3: **deutsch wie die anderen** — `verlauf`.
            DisplayItem::Verlauf { .. } => "verlauf",
            DisplayItem::Text { .. } => "text",
            DisplayItem::Bild { .. } => "bild",
        }
    }

    /// **DER KONSTRUKTOR, DEN 244 INITIALISIERER GEBRAUCHT HAETTEN.** *06.10. 14:49 — Ich habe
    /// `schatten` und `schatten_anzahl` in die Variante `Rect` eingefuegt, und damit ist
    /// **jedes** `DisplayItem::Rect { .. }` in diesem Baum zum Compilefehler geworden: 28
    /// Dateien mit 244 Fundstellen, und am Ende 9 Stellen in
    /// `narsil-chrome/src/audio_leiste.rs`, die meinen Lauf seit 14:10 blockieren. Es gab keinen
    /// Konstruktor — jede Stelle schrieb alle sechs Felder von Hand, also musste jede Stelle
    /// nachgezogen werden, und 14 Dateien mussten es.
    ///
    /// ***Der Fehler war nicht das Hinzufuegen des Feldes, sondern das Fehlen dieses Aufrufs:
    /// eine gemeinsam benutzte Struktur ohne Konstruktor zahlt bei jeder Aenderung jede
    /// Fundstelle einzeln. `DisplayItem` wird von `narsil`, `narsil-chrome`, `narsil-devtools`,
    /// `narsil-raster` und `narsil-probe` gebaut — diese Crates duerfen nicht alle angefasst
    /// werden, also ist die Abhaengigkeit einseitig, und eine einseitige Abhaengigkeit
    /// braucht einen Konstruktor auf der gebenden Seite.** *Mit `radius: [0; 4]`,
    /// `deckkraft: 1.0` und `schatten_anzahl: 0` ist das genau der Wert, den die 14
    /// nachgezogenen Dateien geschrieben haben (z. B. `adressleiste.rs:427-430`).
    ///
    /// *Ein Muster darf nicht mehr bedeuten als es soll: der bequeme Wert ist der haeufige,
    /// und das Abweichen von ihm ist eine Entscheidung, die ihren Platz im Aufruf haben soll.*
    /// Wer Radius, Deckkraft oder Schatten braucht, schreibt weiter das Literal.
    pub fn rechteck(rect: Rect, color: Color) -> Self {
        DisplayItem::Rect {
            rect,
            color,
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [SchattenArt::LEER; MAX_SCHATTEN],
            schatten_anzahl: 0,
        }
    }
}

/// Release 09.10.2026 (Animationen): ein Item um `anker` mit `s` skaliert und
/// um `(dx, dy)` verschoben — Rechtecke, Radien, Rahmen, Schatten, Schrift.
/// Fuer das Aufgehen eines Menues (0,96 → 1) und das Hereingleiten eines
/// Panels; `s = 1`, `dx = dy = 0` gibt das Item unveraendert zurueck.
#[must_use]
pub fn transformiere(item: &DisplayItem, anker: Point, s: f32, dx: f32, dy: f32) -> DisplayItem {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    if s == 1.0 && dx == 0.0 && dy == 0.0 {
        return item.clone();
    }
    let p = |x: f32, y: f32| (anker.x + (x - anker.x) * s + dx, anker.y + (y - anker.y) * s + dy);
    let r = |r: &Rect| {
        let (x, y) = p(r.x, r.y);
        Rect { x, y, width: r.width * s, height: r.height * s }
    };
    let rad = |r: &[f32; 4]| [r[0] * s, r[1] * s, r[2] * s, r[3] * s];
    match item {
        DisplayItem::Rect { rect, color, radius, deckkraft, schatten, schatten_anzahl } => {
            let mut sch = *schatten;
            for x in sch.iter_mut() {
                x.x *= s;
                x.y *= s;
                x.weich *= s;
                x.ausbreitung *= s;
            }
            DisplayItem::Rect {
                rect: r(rect),
                color: *color,
                radius: rad(radius),
                deckkraft: *deckkraft,
                schatten: sch,
                schatten_anzahl: *schatten_anzahl,
            }
        }
        DisplayItem::Rand { rect, rahmen, radius } => {
            let mut rh = *rahmen;
            for b in rh.breite.iter_mut() {
                *b *= s;
            }
            DisplayItem::Rand { rect: r(rect), rahmen: rh, radius: rad(radius) }
        }
        DisplayItem::Verlauf { rect, radius, verlauf } => {
            DisplayItem::Verlauf { rect: r(rect), radius: rad(radius), verlauf: verlauf.clone() }
        }
        DisplayItem::Text { schriftfamilie, origin, text, color, font_size, deko, gewicht, kursiv } => {
            let (x, y) = p(origin.x, origin.y);
            DisplayItem::Text {
                schriftfamilie: schriftfamilie.clone(),
                origin: Point { x, y },
                text: text.clone(),
                color: *color,
                font_size: font_size * s,
                deko: *deko,
                gewicht: *gewicht,
                kursiv: *kursiv,
            }
        }
        DisplayItem::Bild { rect, breite, hoehe, pixels } => DisplayItem::Bild {
            rect: r(rect),
            breite: *breite,
            hoehe: *hoehe,
            pixels: Arc::clone(pixels),
        },
    }
}

/// Release 09.10.2026: eine LIQUID-GLASS-Flaeche (Oberflaeche, nicht Seite).
///
/// Der Rasterer zeichnet sie in einem Durchgang: weicher Schatten aussen,
/// weichgezeichneter, gesaettigter und getoenter Untergrund mit feinem
/// Rauschen, leiser Glanz, innen eine helle und aussen eine dunkle
/// Lichtkante — alles ueber dieselbe Abstandsfunktion der gerundeten Form,
/// also kantengeglaettet.
///
/// GEMESSENE ENTSCHEIDUNG, warum das kein `DisplayItem` ist: eine neue
/// Variante waere an jedem der ueber 40 `match`-Orte in zwoelf Crates
/// nachzutragen (Layout, Seite, Probe, Druck …), und keiner davon braucht
/// Glas. Glas steht in einer Nebentabelle der Liste ([`DisplayList::glas`]),
/// VOR dem Item, das als naechstes kommt — wie der Clip je Item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glas {
    /// Die Form (Bildschirmpixel).
    pub rect: Rect,
    /// Eckradien: oben-links, oben-rechts, unten-rechts, unten-links (CSS).
    pub radius: [f32; 4],
    /// Weichzeichnung des Untergrunds (Gauss-Sigma in Pixeln, 0 = keine).
    pub unschaerfe: f32,
    /// Saettigung des Untergrunds (1,0 = unveraendert).
    pub saettigung: f32,
    /// Toenung: Farbe, Alpha = Staerke der Mischung.
    pub toenung: Color,
    /// Rauschen gegen Banding, in 8-Bit-Stufen (±).
    pub rauschen: f32,
    /// Glanz: Weiss-Anteil oben (0..1), bis zur halben Hoehe auslaufend.
    pub glanz: f32,
    /// Innere helle Lichtkante oben (0..1); unten bleibt knapp ein Fuenftel.
    pub kante_hell: f32,
    /// Aeussere dunkle Kante (0..1).
    pub kante_dunkel: f32,
    /// Schatten: Deckung (0 = keiner).
    pub schatten: f32,
    /// Schatten: Gauss-Sigma in Pixeln.
    pub schatten_weite: f32,
    /// Schatten: Versatz nach unten in Pixeln.
    pub schatten_y: f32,
    /// Breite der Lichtkanten in Pixeln (die Skalierung, mindestens 1).
    pub linie: f32,
}

impl Glas {
    /// Das Glas der schwebenden Felder (Menues, Aufklappfelder): kraeftig
    /// weichgezeichnet, dunkel getoent, mit Schatten. `s` = Skalierung.
    #[must_use]
    pub fn schwebend(rect: Rect, radius: [f32; 4], s: f32) -> Self {
        let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        Self {
            rect,
            radius,
            unschaerfe: 18.0 * s,
            saettigung: 1.45,
            toenung: Color::new(24, 25, 31, 204),
            rauschen: 2.0,
            glanz: 0.055,
            kante_hell: 0.30,
            kante_dunkel: 0.45,
            schatten: 0.75,
            schatten_weite: 14.0 * s,
            schatten_y: 6.0 * s,
            linie: s.round().max(1.0),
        }
    }

    /// Das Glas einer Flaeche AUF der Oberflaeche (Kategorienliste, aktiver
    /// Tab, Karten): leiser, ohne grossen Schatten.
    #[must_use]
    pub fn flach(rect: Rect, radius: [f32; 4], s: f32) -> Self {
        let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        Self {
            unschaerfe: 10.0 * s,
            toenung: Color::new(255, 255, 255, 14),
            saettigung: 1.3,
            glanz: 0.04,
            kante_hell: 0.22,
            kante_dunkel: 0.30,
            schatten: 0.22,
            schatten_weite: 5.0 * s,
            schatten_y: 2.0 * s,
            ..Self::schwebend(rect, radius, s)
        }
    }

    /// Nur Glanz und doppelte Lichtkante auf einer Flaeche, die ihren Grund
    /// schon hat (aktiver Tab, Adressfeld, Knopf): kein Weichzeichnen, keine
    /// Toenung, kein Schatten.
    #[must_use]
    pub fn kanten(rect: Rect, radius: [f32; 4], s: f32, glanz: f32, hell: f32, dunkel: f32) -> Self {
        let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        Self {
            rect,
            radius,
            unschaerfe: 0.0,
            saettigung: 1.0,
            toenung: Color::new(0, 0, 0, 0),
            rauschen: 0.0,
            glanz,
            kante_hell: hell,
            kante_dunkel: dunkel,
            schatten: 0.0,
            schatten_weite: 0.0,
            schatten_y: 0.0,
            linie: s.round().max(1.0),
        }
    }
}

/// Release 09.10.2026: ein Effekt in der Nebentabelle der Liste.
#[derive(Debug, Clone, PartialEq)]
pub enum Effekt {
    /// Eine Glasflaeche, gezeichnet vor dem Item mit diesem Index.
    Glas(Glas),
    /// Gruppen-Deckkraft (CSS `opacity` einer Gruppe, fuer Ein-/Ausblenden):
    /// die Items von hier bis `bis` (exklusiv) werden gezeichnet und danach
    /// im `bereich` mit `deckkraft` ueber den Stand davor gemischt.
    Gruppe {
        /// Erstes Item NACH der Gruppe.
        bis: usize,
        /// Der Bereich, den die Gruppe beruehrt (Bildschirmpixel).
        bereich: Rect,
        /// 0 = unsichtbar, 1 = voll.
        deckkraft: f32,
        /// Release 15.10.2026: the corner radii of `bereich` (CSS order
        /// top-left, top-right, bottom-right, bottom-left); the group is
        /// masked to that rounded rectangle. All zero = no mask.
        radius: [f32; 4],
        /// Release 15.10.2026 (Agent F): a coverage mask (`clip-path`,
        /// `mask`) the group is multiplied with; outside it the group does
        /// not paint.
        maske: Option<Maske>,
        /// Release 15.10.2026 (Agent F): a `filter` (CSS Filter Effects 1):
        /// the group is painted alone, filtered, then composited.
        filter: Option<std::sync::Arc<FilterSpec>>,
        /// Release 15.10.2026 (Agent F): the vertical radii of elliptical
        /// corners (`radius` holds the horizontal ones); `None`: circular.
        radius_y: Option<[f32; 4]>,
    },
    /// Release 15.10.2026 (Agent F): `backdrop-filter` (Filter Effects 2
    /// §3): what is painted behind the box within its border box (with
    /// `radius`) is filtered and composited over itself, before the box's
    /// own items.
    Backdrop {
        spec: std::sync::Arc<FilterSpec>,
        radius: [f32; 4],
        /// The vertical radii (equal to `radius` for circular corners).
        radius_y: [f32; 4],
    },
}

/// Release 15.10.2026 (Agent F): a rounded clip (CSS Backgrounds 3 §5.3):
/// the items of the boxes inside an `overflow` clip with rounded corners
/// are masked to `rect` with these corner radii (CSS order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundedClip {
    pub rect: Rect,
    pub radius_x: [f32; 4],
    pub radius_y: [f32; 4],
}

/// Release 15.10.2026 (Agent F): the `filter` of a CSS box.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterSpec {
    /// The computed `filter` value.
    pub value: String,
    /// The `<filter>` elements `url()` names, as an SVG document.
    pub svg: Option<std::sync::Arc<str>>,
    /// The border box (the filter's user space and bounding box).
    pub border_box: Rect,
    /// `currentColor`.
    pub current_color: [u8; 3],
    /// The clip of the box's ancestors (`overflow`): the filtered result
    /// stays inside it.
    pub clip: Option<Rect>,
}

/// Release 15.10.2026 (Agent F): an alpha coverage bitmap placed at
/// (`x`, `y`) in list coordinates, one byte per pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct Maske {
    pub x: f32,
    pub y: f32,
    pub breite: u32,
    pub hoehe: u32,
    pub alpha: std::sync::Arc<Vec<u8>>,
}

impl Maske {
    /// The coverage (0..=1) at a pixel centre in list coordinates.
    #[must_use]
    pub fn bei(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = ((x - self.x).floor(), (y - self.y).floor());
        if fx < 0.0 || fy < 0.0 || fx >= self.breite as f32 || fy >= self.hoehe as f32 {
            return 0.0;
        }
        f32::from(self.alpha[fy as usize * self.breite as usize + fx as usize]) / 255.0
    }

    /// The rectangle the mask covers.
    #[must_use]
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.breite as f32, self.hoehe as f32)
    }
}

impl Effekt {
    fn geschoben(&self, dx: f32, dy: f32, versatz: usize) -> Self {
        let r = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.width, r.height);
        match self {
            Effekt::Glas(g) => Effekt::Glas(Glas { rect: r(g.rect), ..*g }),
            Effekt::Gruppe { bis, bereich, deckkraft, radius, maske, filter, radius_y } => Effekt::Gruppe {
                bis: bis + versatz,
                bereich: r(*bereich),
                deckkraft: *deckkraft,
                radius: *radius,
                radius_y: *radius_y,
                maske: maske.as_ref().map(|m| Maske { x: m.x + dx, y: m.y + dy, ..m.clone() }),
                filter: filter.as_ref().map(|f| std::sync::Arc::new(FilterSpec { border_box: r(f.border_box), clip: f.clip.map(r), ..(**f).clone() })),
            },
            Effekt::Backdrop { spec, radius, radius_y } => Effekt::Backdrop {
                spec: std::sync::Arc::new(FilterSpec { border_box: r(spec.border_box), clip: spec.clip.map(r), ..(**spec).clone() }),
                radius: *radius,
                radius_y: *radius_y,
            },
        }
    }
}

/// Geordnete Liste von Zeichenoperationen fuer ein Dokument.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayList {
    items: Vec<DisplayItem>,
    /// Effektiver Clip je Item; `None` erlaubt die ganze Zeichenflaeche.
    clips: Vec<Option<Rect>>,
    clip_stack: Vec<Rect>,
    /// Farbe, die vor allen Items den gesamten Canvas fuellt.
    pub background: Option<Color>,
    /// Release 09.10.2026: Effekte (Glas, Gruppen) — `(vor Item, Clip, Effekt)`,
    /// nach dem Index geordnet.
    effekte: Vec<(usize, Option<Rect>, Effekt)>,
    /// Release 11.10.2026: CSS `transform` — die Abbildung `p -> s·p +
    /// (dx, dy)`, in der ab jetzt gesetzte Clips gelten (`setze_abbildung`).
    abbildung: Option<(f32, f32, f32)>,
    /// Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing` je
    /// Text-Item — `(Item, [(Byte-Anfang des Teils, x-Versatz vom Ursprung)])`,
    /// nach dem Index geordnet, nur fuer Items mit Abstand (CSS 2.1 §16.4).
    /// Der Lauf bleibt EIN `DisplayItem::Text` (Textsuche, Kopieren, Tests
    /// lesen ihn am Stueck); der Raster zeichnet die Teile an den Stellen, die
    /// das Layout mit SEINER Messung gerechnet hat. Ein Feld am Item haette
    /// 164 Literale in 50 Dateien geaendert.
    abstaende: Vec<(usize, Box<[(u32, f32)]>)>,
    /// Release 15.10.2026 (Agent F): the vertical corner radii of items with
    /// elliptical corners (`border-radius: 50%` on a box that is not
    /// square, `10px / 20px`) — `(Item, radii)`, ordered by index. The
    /// item's `radius` holds the horizontal radii; items without an entry
    /// have circular corners. A side table like `abstaende`: a field on the
    /// item would change every literal of it.
    vertical_radii: Vec<(usize, [f32; 4])>,
    /// Release 15.10.2026 (Agent F): the rounded clip of items inside a
    /// rounded `overflow` clip (see [`RoundedClip`]), ordered by index.
    rounded_clips: Vec<(usize, RoundedClip)>,
}

impl DisplayList {
    /// Leere Liste.
    pub fn new() -> Self {
        Self::default()
    }

    /// Leere Liste mit Hintergrundfarbe.
    pub fn with_background(color: Color) -> Self {
        Self {
            items: Vec::new(),
            clips: Vec::new(),
            clip_stack: Vec::new(),
            background: Some(color),
            effekte: Vec::new(),
            abbildung: None,
            abstaende: Vec::new(),
            vertical_radii: Vec::new(),
            rounded_clips: Vec::new(),
        }
    }

    /// Release 12.10.2026 (Agent CSS2): die Teile des zuletzt angehaengten
    /// Text-Items mit ihren x-Versaetzen (siehe Feld `abstaende`). Ohne Item
    /// oder ohne Teile geschieht nichts.
    pub fn versaetze_fuer_letztes(&mut self, teile: Box<[(u32, f32)]>) {
        if self.items.is_empty() || teile.is_empty() {
            return;
        }
        let i = self.items.len() - 1;
        if self.abstaende.last().is_some_and(|(j, _)| *j == i) {
            self.abstaende.pop();
        }
        self.abstaende.push((i, teile));
    }

    /// Release 15.10.2026 (Agent F): the last pushed item has elliptical
    /// corners with the vertical radii `y` (its `radius` holds the
    /// horizontal ones).
    pub fn set_vertical_radii_for_last(&mut self, y: [f32; 4]) {
        if self.items.is_empty() {
            return;
        }
        let i = self.items.len() - 1;
        if self.vertical_radii.last().is_some_and(|(j, _)| *j == i) {
            self.vertical_radii.pop();
        }
        self.vertical_radii.push((i, y));
    }

    /// Release 15.10.2026 (Agent F): the items `start..end` (the last ones
    /// pushed) lie inside the rounded clip `clip`.
    pub fn set_rounded_clip(&mut self, start: usize, end: usize, clip: RoundedClip) {
        let end = end.min(self.items.len());
        let start = start.max(self.rounded_clips.last().map_or(0, |(i, _)| i + 1));
        for i in start..end {
            self.rounded_clips.push((i, clip));
        }
    }

    /// Release 15.10.2026 (Agent F): the rounded clip of item `index`.
    #[must_use]
    pub fn rounded_clip_at(&self, index: usize) -> Option<&RoundedClip> {
        self.rounded_clips.binary_search_by_key(&index, |(i, _)| *i).ok().map(|k| &self.rounded_clips[k].1)
    }

    /// Release 15.10.2026 (Agent F): the area item `index` paints (without
    /// its shadows), if known.
    #[must_use]
    pub fn item_bounds_at(&self, index: usize) -> Option<Rect> {
        self.items.get(index).and_then(item_bounds)
    }

    /// Release 15.10.2026 (Agent F): the vertical corner radii of item
    /// `index`, if its corners are elliptical.
    #[must_use]
    pub fn vertical_radii_at(&self, index: usize) -> Option<[f32; 4]> {
        self.vertical_radii.binary_search_by_key(&index, |(i, _)| *i).ok().map(|k| self.vertical_radii[k].1)
    }

    /// Release 12.10.2026 (Agent CSS2): die Teile des Items `index` (Byte-
    /// Anfang, x-Versatz vom Ursprung), wenn es Buchstaben-/Wortabstand hat.
    pub fn versaetze_bei(&self, index: usize) -> Option<&[(u32, f32)]> {
        self.abstaende
            .binary_search_by_key(&index, |(i, _)| *i)
            .ok()
            .map(|k| &*self.abstaende[k].1)
    }

    /// Release 09.10.2026: eine Glasflaeche — gezeichnet VOR dem naechsten
    /// Item (also ueber allem, was bis jetzt in der Liste steht), mit dem
    /// aktiven Clip.
    pub fn glas(&mut self, g: Glas) {
        let clip = self.clip_stack.last().copied();
        self.effekte.push((self.items.len(), clip, Effekt::Glas(g)));
    }

    /// Release 09.10.2026: haengt `andere` an — Items, Clips UND Effekte.
    ///
    /// GEMESSEN, warum es das braucht: die Oberflaeche baut Menues und
    /// Panels als eigene Listen und kopierte sie mit `for item in … push` —
    /// Effekte (Glas) gingen dabei verloren.
    pub fn anhaengen(&mut self, andere: &DisplayList) {
        let versatz = self.items.len();
        let aussen = self.clip_stack.last().copied();
        for (i, item) in andere.items.iter().enumerate() {
            self.push_mit_clip(item.clone(), andere.clip_at(i));
        }
        self.abstaende.extend(andere.abstaende.iter().map(|(i, t)| (i + versatz, t.clone())));
        self.vertical_radii.extend(andere.vertical_radii.iter().map(|(i, y)| (i + versatz, *y)));
        self.rounded_clips.extend(andere.rounded_clips.iter().map(|(i, c)| (i + versatz, *c)));
        for (i, clip, e) in &andere.effekte {
            let clip = match (aussen, *clip) {
                (Some(a), Some(b)) => Some(a.intersect(b).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))),
                (a, b) => a.or(b),
            };
            self.effekte.push((i + versatz, clip, e.geschoben(0.0, 0.0, versatz)));
        }
    }

    /// Release 09.10.2026: wie [`DisplayList::anhaengen`], die angehaengten
    /// Items aber als GRUPPE mit `deckkraft` (Ein- und Ausblenden). Bei
    /// voller Deckkraft ist das genau `anhaengen`.
    pub fn anhaengen_mit_deckkraft(&mut self, andere: &DisplayList, deckkraft: f32, bereich: Rect) {
        if deckkraft < 1.0 && !andere.is_empty() {
            let bis = self.items.len() + andere.items.len();
            self.effekte.push((
                self.items.len(),
                None,
                Effekt::Gruppe { bis, bereich, deckkraft: deckkraft.clamp(0.0, 1.0), radius: [0.0; 4], maske: None, filter: None, radius_y: None },
            ));
        }
        self.anhaengen(andere);
    }

    /// Release 15.10.2026: the items from `anfang` to the current end form a
    /// group with `deckkraft`, masked to `bereich` with the corner `radius`
    /// (the image of a replaced element and the background images of a box
    /// with `opacity` or `border-radius`). Nothing happens for a full,
    /// square group or an empty range.
    pub fn gruppe_ab(&mut self, anfang: usize, bereich: Rect, deckkraft: f32, radius: [f32; 4]) {
        self.group_from_with_corners(anfang, bereich, deckkraft, radius, radius);
    }

    /// Release 15.10.2026 (Agent F): like [`Self::gruppe_ab`] with
    /// elliptical corners: horizontal radii `radius`, vertical `radius_y`.
    pub fn group_from_with_corners(&mut self, anfang: usize, bereich: Rect, deckkraft: f32, radius: [f32; 4], radius_y: [f32; 4]) {
        let rund = radius.iter().zip(radius_y.iter()).any(|(x, y)| *x > 0.0 && *y > 0.0);
        if anfang >= self.items.len() || (deckkraft >= 1.0 && !rund) {
            return;
        }
        self.effekte.push((
            anfang,
            None,
            Effekt::Gruppe {
                bis: self.items.len(),
                bereich,
                deckkraft: deckkraft.clamp(0.0, 1.0),
                radius: radius.map(|r| if r.is_finite() { r.max(0.0) } else { 0.0 }),
                maske: None,
                filter: None,
                radius_y: (radius_y != radius).then(|| radius_y.map(|r| if r.is_finite() { r.max(0.0) } else { 0.0 })),
            },
        ));
    }

    /// Release 15.10.2026 (Agent F): the items from `start` to the current
    /// end form a group painted alone and filtered (`filter` on a CSS box);
    /// `reach` is how far the filter can move content beyond the items.
    pub fn filter_group(&mut self, start: usize, spec: FilterSpec, reach: f32) {
        if start > self.items.len() {
            return;
        }
        let mut area = spec.border_box;
        for index in start..self.items.len() {
            if let Some(b) = item_bounds(&self.items[index]) {
                let b = match self.clip_at(index) {
                    Some(c) => b.intersect(c).unwrap_or(b),
                    None => b,
                };
                area = area.union(b).unwrap_or(area);
            }
        }
        let reach = reach.max(16.0);
        let area = Rect::new(area.x - reach, area.y - reach, area.width + 2.0 * reach, area.height + 2.0 * reach);
        self.effekte.push((start, None, Effekt::Gruppe { bis: self.items.len(), bereich: area, deckkraft: 1.0, radius: [0.0; 4], maske: None, filter: Some(std::sync::Arc::new(spec)), radius_y: None }));
    }

    /// Release 15.10.2026 (Agent F): `backdrop-filter` of the box whose
    /// items come next (see [`Effekt::Backdrop`]).
    pub fn push_backdrop(&mut self, spec: FilterSpec, radius: [f32; 4], radius_y: [f32; 4]) {
        self.effekte.push((self.items.len(), None, Effekt::Backdrop { spec: std::sync::Arc::new(spec), radius, radius_y }));
    }

    /// Release 15.10.2026 (Agent F): the items from `anfang` to the current
    /// end form a group masked by `maske` (`clip-path` on a CSS box). The
    /// group's area is what its items touch and the mask, with room for
    /// shadows and text the item bounds only estimate.
    pub fn gruppe_mit_maske(&mut self, anfang: usize, maske: Maske) {
        if anfang >= self.items.len() {
            return;
        }
        let mut bereich = maske.rect();
        for index in anfang..self.items.len() {
            if let Some(b) = item_bounds(&self.items[index]) {
                let b = match self.clip_at(index) {
                    Some(c) => b.intersect(c).unwrap_or(b),
                    None => b,
                };
                bereich = bereich.union(b).unwrap_or(bereich);
            }
        }
        const RAND: f32 = 64.0;
        let bereich = Rect::new(bereich.x - RAND, bereich.y - RAND, bereich.width + 2.0 * RAND, bereich.height + 2.0 * RAND);
        self.effekte.push((anfang, None, Effekt::Gruppe { bis: self.items.len(), bereich, deckkraft: 1.0, radius: [0.0; 4], maske: Some(maske), filter: None, radius_y: None }));
    }

    /// Release 09.10.2026: die Effekte vor dem Item `index` (in der
    /// Reihenfolge, in der sie eingetragen wurden), mit ihrem Clip.
    pub fn effekte_bei(&self, index: usize) -> impl Iterator<Item = (&Effekt, Option<Rect>)> {
        self.effekte
            .iter()
            .filter(move |(i, _, _)| *i == index)
            .map(|(_, c, e)| (e, *c))
    }

    /// Release 09.10.2026: alle Effekte `(vor Item, Clip, Effekt)`.
    pub fn effekte(&self) -> &[(usize, Option<Rect>, Effekt)] {
        &self.effekte
    }

    /// Release 09.10.2026 (Animationen): die ganze Liste um `anker` mit `s`
    /// skaliert und um `(dx, dy)` verschoben — Items, Clips und Glas.
    #[must_use]
    pub fn transformiert(&self, anker: Point, s: f32, dx: f32, dy: f32) -> DisplayList {
        let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        let r = |c: Rect| {
            Rect::new(anker.x + (c.x - anker.x) * s + dx, anker.y + (c.y - anker.y) * s + dy, c.width * s, c.height * s)
        };
        DisplayList {
            items: self.items.iter().map(|i| transformiere(i, anker, s, dx, dy)).collect(),
            clips: self.clips.iter().map(|c| c.map(r)).collect(),
            clip_stack: Vec::new(),
            abbildung: None,
            abstaende: self
                .abstaende
                .iter()
                .map(|(i, t)| (*i, t.iter().map(|(b, x)| (*b, x * s)).collect()))
                .collect(),
            vertical_radii: self.vertical_radii.iter().map(|(i, y)| (*i, y.map(|v| v * s))).collect(),
            rounded_clips: self
                .rounded_clips
                .iter()
                .map(|(i, c)| (*i, RoundedClip { rect: r(c.rect), radius_x: c.radius_x.map(|v| v * s), radius_y: c.radius_y.map(|v| v * s) }))
                .collect(),
            background: self.background,
            effekte: self
                .effekte
                .iter()
                .map(|(i, c, e)| {
                    let e = match e {
                        Effekt::Glas(g) => Effekt::Glas(Glas {
                            rect: r(g.rect),
                            radius: g.radius.map(|x| x * s),
                            schatten_weite: g.schatten_weite * s,
                            schatten_y: g.schatten_y * s,
                            ..*g
                        }),
                        Effekt::Gruppe { bis, bereich, deckkraft, radius, maske, filter, radius_y } => Effekt::Gruppe {
                            bis: *bis,
                            bereich: r(*bereich),
                            deckkraft: *deckkraft,
                            radius: radius.map(|x| x * s),
                            radius_y: radius_y.map(|y| y.map(|v| v * s)),
                            filter: filter.as_ref().map(|f| std::sync::Arc::new(FilterSpec { border_box: r(f.border_box), clip: f.clip.map(r), ..(**f).clone() })),
                            // A scaled list keeps the mask's position; its
                            // pixels stay (they are re-made with the layout).
                            maske: maske.as_ref().map(|m| {
                                let p = r(m.rect());
                                Maske { x: p.x, y: p.y, ..m.clone() }
                            }),
                        },
                        Effekt::Backdrop { spec, radius, radius_y } => Effekt::Backdrop {
                            spec: std::sync::Arc::new(FilterSpec { border_box: r(spec.border_box), clip: spec.clip.map(r), ..(**spec).clone() }),
                            radius: radius.map(|x| x * s),
                            radius_y: radius_y.map(|y| y * s),
                        },
                    };
                    (*i, c.map(r), e)
                })
                .collect(),
        }
    }

    /// Release 09.10.2026: der Bereich, den die Liste beruehrt — Items und
    /// Glas samt Schatten (fuer Gruppen-Deckkraft und Ueberblendungen).
    #[must_use]
    pub fn bereich_mit_effekten(&self) -> Option<Rect> {
        let mut acc = self.bounds();
        for (_, _, e) in &self.effekte {
            if let Effekt::Glas(g) = e {
                let w = 3.0 * g.schatten_weite + g.schatten_y.abs() + g.linie + 1.0;
                let r = Rect::new(g.rect.x - w, g.rect.y - w, g.rect.width + 2.0 * w, g.rect.height + 2.0 * w);
                acc = match acc {
                    Some(a) => a.union(r),
                    None => Some(r),
                };
            }
        }
        acc
    }

    /// Release 09.10.2026: eine Liste nur mit den Items, fuer die `behalten`
    /// gilt — Clips und Effekte wandern mit (ein Effekt haengt am naechsten
    /// behaltenen Item, eine Gruppe endet am naechsten behaltenen danach).
    pub fn behalte(&self, behalten: impl Fn(&DisplayItem) -> bool) -> DisplayList {
        let mut raus = DisplayList { background: self.background, ..DisplayList::default() };
        // neu[i] = Index in der neuen Liste, den das alte Item i bekommt
        // (bzw. bekaeme), auch fuer i == len.
        let mut neu = Vec::with_capacity(self.items.len() + 1);
        for (i, item) in self.items.iter().enumerate() {
            neu.push(raus.items.len());
            if behalten(item) {
                raus.items.push(item.clone());
                raus.clips.push(self.clip_at(i));
            }
        }
        neu.push(raus.items.len());
        for (i, t) in &self.abstaende {
            if let Some(item) = self.items.get(*i)
                && behalten(item)
            {
                raus.abstaende.push((neu[*i], t.clone()));
            }
        }
        for (i, y) in &self.vertical_radii {
            if let Some(item) = self.items.get(*i)
                && behalten(item)
            {
                raus.vertical_radii.push((neu[*i], *y));
            }
        }
        for (i, c) in &self.rounded_clips {
            if let Some(item) = self.items.get(*i)
                && behalten(item)
            {
                raus.rounded_clips.push((neu[*i], *c));
            }
        }
        for (i, clip, e) in &self.effekte {
            let ni = neu[(*i).min(self.items.len())];
            let e = match e {
                Effekt::Gruppe { bis, bereich, deckkraft, radius, maske, filter, radius_y } => Effekt::Gruppe {
                    bis: neu[(*bis).min(self.items.len())],
                    bereich: *bereich,
                    deckkraft: *deckkraft,
                    radius: *radius,
                    radius_y: *radius_y,
                    maske: maske.clone(),
                    filter: filter.clone(),
                },
                andere => andere.clone(),
            };
            raus.effekte.push((ni, *clip, e));
        }
        raus
    }

    /// Haengt ein Item an (Reihenfolge = Zeichenreihenfolge = Painter's Algorithm).
    pub fn push(&mut self, item: DisplayItem) {
        self.push_mit_clip(item, None);
    }

    /// Uebernimmt ein Item mitsamt seinem Clip aus einer anderen Liste.
    /// Der aktive Clip dieser Liste wird zusaetzlich angewendet.
    /// Verschiebt **alle** Items und **alle** Clips um `dy` Pixel.
    ///
    /// BILD_29. **WARUM DIESE METHODE ES GEBEN MUSS, GEMESSEN:**
    /// `narsil/src/sitzung.rs` (die Scroll-Schleife, um 9471) tat
    /// `for item in liste.iter() { verschoben.push(verschiebe(item, dy)); }` —
    /// **`push` ohne Clip.** `liste = verschoben` **verwirft damit jeden Clip**,
    /// den die Liste vorher hatte. Und es gab keine andere Moeglichkeit:
    /// `clips` ist ein eigenes `Vec`, und es gab keine Methode, es zu
    /// verschieben (gemessen: keine `pub fn (verschiebe|schiebe|translate)` in
    /// dieser Datei). **Ein Item, das einen Clip braucht — ein `object-fit:cover`,
    /// das auf seinen Kasten geschnitten wird — waere beim ersten Scroll-Frame
    /// ungeschnitten.**
    ///
    /// **UND DIE ARBEIT IST KAPUTT, WENN MAN NUR DIE ITEMS VERSCHIEBT:** ein
    /// stehengebliebener Clip schneidet an der **alten** Stelle, das Bild
    /// wandert — das ist kein Flackern, das ist ein falsches Bild an der
    /// falschen Stelle.
    ///
    /// `dy == 0.0` ist ein **No-Op ohne Arbeit**: dann wird weder kopiert noch
    /// gerechnet. GEMESSENE ENTSCHEIDUNG, warum nicht `skip` ueber `!= 0.0`:
    /// `f32`-Gleichheit auf `0.0` ist hier exakt (die Differenz zweier
    /// Bildschirmkoordinaten ganzer Pixel), und ein `NaN` ist nicht `0.0`.
    pub fn schiebe_alle(&mut self, dy: f32) {
        if dy == 0.0 {
            return;
        }
        for item in self.items.iter_mut() {
            *item = schiebe(item, dy);
        }
        for clip in self.clips.iter_mut() {
            // GEMESSEN: `narsil_core::geometry::Rect` hat **kein** `translate`
            // (nur `Point`, `geometry.rs:43`) — deshalb aus den Feldern, und
            // `width`/`height` bleiben unangetastet: ein Versatz verschiebt,
            // er skaliert nicht.
            *clip = clip.map(|c| Rect {
                x: c.x,
                y: c.y + dy,
                width: c.width,
                height: c.height,
            });
        }
        // Release 09.10.2026: die Effekte wandern mit (Glas, Gruppenbereich).
        for (_, clip, e) in self.effekte.iter_mut() {
            *clip = clip.map(|c| Rect::new(c.x, c.y + dy, c.width, c.height));
            *e = e.geschoben(0.0, dy, 0);
        }
        for (_, c) in self.rounded_clips.iter_mut() {
            c.rect.y += dy;
        }
    }

    /// Release 09.10.2026: nur die Eintraege `von..bis` (samt ihren Clips
    /// und Effekten) senkrecht verschieben — fuer `position: fixed`, das
    /// beim Rollen stehen bleibt.
    pub fn schiebe_bereich(&mut self, von: usize, bis: usize, dy: f32) {
        if dy == 0.0 || von >= bis {
            return;
        }
        let bis = bis.min(self.items.len());
        for item in self.items[von.min(bis)..bis].iter_mut() {
            *item = schiebe(item, dy);
        }
        let cbis = bis.min(self.clips.len());
        for clip in self.clips[von.min(cbis)..cbis].iter_mut() {
            *clip = clip.map(|c| Rect::new(c.x, c.y + dy, c.width, c.height));
        }
        for (i, clip, e) in self.effekte.iter_mut() {
            if *i >= von && *i < bis {
                *clip = clip.map(|c| Rect::new(c.x, c.y + dy, c.width, c.height));
                *e = e.geschoben(0.0, dy, 0);
            }
        }
        for (i, c) in self.rounded_clips.iter_mut() {
            if *i >= von && *i < bis {
                c.rect.y += dy;
            }
        }
    }

    /// Release 11.10.2026: CSS `transform` — die Items `von..bis` mit `s`
    /// (um den Nullpunkt) skaliert und um `(dx, dy)` verschoben. Die Clips
    /// bleiben: sie stehen schon in Bildschirmlage (der Clip eines
    /// transformierten Kastens wird beim Sammeln mitbewegt).
    pub fn transformiere_bereich(&mut self, von: usize, bis: usize, s: f32, dx: f32, dy: f32) {
        if von >= bis || (s == 1.0 && dx == 0.0 && dy == 0.0) {
            return;
        }
        let bis = bis.min(self.items.len());
        let anker = Point::new(0.0, 0.0);
        for item in self.items[von.min(bis)..bis].iter_mut() {
            *item = transformiere(item, anker, s, dx, dy);
        }
        for (i, t) in self.abstaende.iter_mut() {
            if *i >= von && *i < bis {
                for (_, x) in t.iter_mut() {
                    *x *= s;
                }
            }
        }
        for (i, y) in self.vertical_radii.iter_mut() {
            if *i >= von && *i < bis {
                *y = y.map(|v| v * s);
            }
        }
        for (i, _clip, e) in self.effekte.iter_mut() {
            if *i >= von && *i < bis {
                *e = e.geschoben(dx, dy, 0);
            }
        }
    }

    /// Release 11.10.2026: CSS `transform` — Clips, die ab jetzt gesetzt
    /// werden (`push_clip`, `push_mit_clip`), gelten in der Lage
    /// `p -> s·p + (dx, dy)`; die Items selbst bildet danach
    /// [`DisplayList::transformiere_bereich`] ab. `None` beendet das.
    /// GEMESSEN vorher (bbc.com): der Platzhalter im Suchfeld wanderte, sein
    /// Clip nicht — sichtbar blieb „Sea".
    pub fn setze_abbildung(&mut self, abbildung: Option<(f32, f32, f32)>) {
        self.abbildung = abbildung;
    }

    fn abgebildet(&self, r: Rect) -> Rect {
        match self.abbildung {
            Some((s, dx, dy)) => Rect::new(s * r.x + dx, s * r.y + dy, s * r.width, s * r.height),
            None => r,
        }
    }

    pub fn push_mit_clip(&mut self, item: DisplayItem, clip: Option<Rect>) {
        let clip = clip.map(|c| self.abgebildet(c));
        let effektiv = match (self.clip_stack.last().copied(), clip) {
            (Some(a), Some(b)) => Some(a.intersect(b).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        self.items.push(item);
        self.clips.push(effektiv);
    }

    /// Begrenzt die folgenden Items auf das Rechteck und alle aeusseren Clips.
    pub fn push_clip(&mut self, rect: Rect) {
        let rect = self.abgebildet(rect);
        let clip = self.clip_stack.last().copied().map_or(rect, |outer| {
            outer
                .intersect(rect)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        });
        self.clip_stack.push(clip);
    }

    /// Beendet die letzte Begrenzung.
    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    /// Effektiver Clip des Items an dieser Position.
    pub fn clip_at(&self, index: usize) -> Option<Rect> {
        self.clips.get(index).copied().flatten()
    }

    /// Anzahl der Items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Ob die Liste leer ist.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iteration ueber die Items in Zeichenreihenfolge.
    pub fn iter(&self) -> std::slice::Iter<'_, DisplayItem> {
        self.items.iter()
    }

    /// Zugriff auf ein Item per Index.
    pub fn get(&self, index: usize) -> Option<&DisplayItem> {
        self.items.get(index)
    }

    /// Bounding-Box ueber alle Items (ohne Hintergrund).
    /// `None`, wenn keine Items vorhanden sind.
    pub fn bounds(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        for (index, item) in self.items.iter().enumerate() {
            let r = match (item_bounds(item), self.clip_at(index)) {
                (Some(bounds), Some(clip)) => bounds.intersect(clip),
                (bounds, None) => bounds,
                (None, _) => None,
            };
            acc = match (acc, r) {
                (None, Some(rr)) => Some(rr),
                (Some(a), Some(rr)) => a.union(rr),
                (a, None) => a,
            };
        }
        acc
    }

    /// Zaehlt Items eines Typs.
    pub fn count_of(&self, kind: &str) -> usize {
        self.items.iter().filter(|i| i.kind_name() == kind).count()
    }
}

/// Ungefaehre Bildflaeche eines Items, ohne Font-Abhaengigkeit.
/// Fuer Text wird die Breite nur geschaetzt; der Rasterizer rechnet sie
/// spaeter praezise aus. Fuer die Bounding-Box reicht eine Naeherung.
/// Nutzt `narsil_text::measure` mit der eingebauten Bitmap-Schrift, um
/// die Breite objektiv zu messen anstatt mit der alten 0.6-em-Naeherung.
fn item_bounds(item: &DisplayItem) -> Option<Rect> {
    match item {
        // BILD_32: der Ring liegt **im** `border-box`, also sind seine Grenzen
        // die des Kastens — dieselbe wie beim Fuell-`Rect`. **GEMESSENE
        // Ueberlegung:** ein Ring *ausserhalb* des Kastens waere an den vier
        // Seiten nicht gleich breit, und genau das waere der Fehler.
        DisplayItem::Rect { rect, .. }
        | DisplayItem::Bild { rect, .. }
| DisplayItem::Rand { rect, .. }
        // 05.10.2026, SERVER_16: **der Verlauf fuellt den Kasten wie `Rect`**
        // (so auch der Kommentar in `hit_test` derselben Datei) — die
        // Bounding-Box ist die **seines Hintergrunds**, also gehoert er in
        // dieselbe Gruppe. Ohne diesen Zweig gab es fuer einen Verlauf
        // **keine** Grenze zurueck, und die Box des Kastens fiel aus dem
        // Rechteck der ganzen Liste.
        | DisplayItem::Verlauf { rect, .. } => {
            if rect.is_empty() {
                None
            } else {
                Some(*rect)
            }
        }
        DisplayItem::Text {
            schriftfamilie: _,
            origin,
            text,
            font_size,
            color: _,
            deko: _,
            // 04.10.2026 (SCHRIFT): die beiden Felder sind fuer die
            // Trefferfrage **ohne** Bedeutung — die Breite wird hier mit
            // der Bitmap-Schrift gemessen, und die ist fuer jeden Schnitt
            // dieselbe. Sie werden mit `_` genannt, weil das Muster alle
            // Felder nennen muss.
            gewicht: _,
            kursiv: _,
        } => {
            if *font_size <= 0.0 || text.is_empty() {
                return None;
            }
            // Messe die Breite mit der eingebauten Bitmap-Schrift statt
            // mit der approximativen 0.6-em-Formel.
            let font = BitmapFont::builtin();
            let metrics = measure(&font, text, *font_size)
                .expect("Messfehler: Text und Schriftgroesse sind gueltig");
            let width = metrics.width;
            Some(Rect::new(origin.x, origin.y, width, *font_size))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_begrenzt_items_treffer_und_bounds() {
        let mut dl = DisplayList::new();
        dl.push(DisplayItem::rechteck(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Color::WHITE,
        ));
        dl.push_clip(Rect::new(5.0, 5.0, 5.0, 5.0));
        dl.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            origin: Point::new(15.0, 15.0),
            text: "X".into(),
            color: Color::BLACK,
            font_size: 10.0,
            deko: 0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        dl.pop_clip();
        assert_eq!(dl.clip_at(0), None);
        assert_eq!(dl.clip_at(1), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert!(matches!(
            hit_test(&dl, Point::new(16.0, 16.0)),
            Some(HitTarget::None)
        ));
        assert_eq!(dl.bounds(), Some(Rect::new(0.0, 0.0, 20.0, 20.0)));
    }

    #[test]
    fn empty_list() {
        let dl = DisplayList::new();
        assert!(dl.is_empty());
        assert_eq!(dl.len(), 0);
        assert!(dl.bounds().is_none());
    }

    /// S2 Station 3: ein Hintergrund OHNE Kaskadenteile. Diese Felder
    /// kommen in den Tests der Anzeigepfade vor, die sich nicht mit
    /// Ecken, Deckkraft oder Schatten beschaeftigen — dort zu schreiben
    /// waere eine Behauptung ueber einen Wert, den es nicht gibt.
    ///
    /// 14:50 — **dieser Helfer war `DisplayItem::rechteck` zum zweiten Mal, an einer Stelle,
    /// wo es niemand findet.** *Zeile fuer Zeile derselbe Aufruf, nur ohne den Weg in die
    /// Fremdcrates. Die Notwendigkeit war also bekannt — sie stand nur im Testmodul. Genau
    /// das ist der Befund: **ein Konstruktor, den man nur im eigenen Test braucht, gehoert in
    /// die API und nicht in den Test.** Die Zeilen 354-363 sind jetzt weg und `ecklos`
    /// ruft `rechteck`. Die Vorgaben selbst haelt `rechteck_ist_das_von_hand_geschriebene`.
    fn ecklos(rect: Rect, color: Color) -> DisplayItem {
        DisplayItem::rechteck(rect, color)
    }

    /// 14:50 — **DIE VORGABEN FESTSCHREIBEN, DAMIT SIE NIEMAND STILL AEENDERT.** *`rechteck`
    /// ist bequem, und bequem heisst: niemand schreibt mehr hin, was er bekommt. Wenn jemand
    /// `deckkraft` auf 0.8 aendert, faellt kein Test und alle 244 Aufrufer aendern sich
    /// unbemerkt. Deshalb steht hier das Literal daneben, und der Test vergleicht beide — *
    /// ***der Test liest nicht die Ausgabe von `rechteck`, sondern zwei unabhaengige
    /// Definitionen, und sagt, ob sie dasselbe sind. Das ist der Unterschied zwischen einer
    /// Pruefung und einem Abgleich mit sich selbst.*** * (`ecklos` waere genau das nicht:
    /// es waere derselbe Aufruf.)
    #[test]
    fn rechteck_ist_das_von_hand_geschriebene() {
        let r = Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let c = Color::rgb(1, 2, 3);
        let von_hand = DisplayItem::Rect {
            rect: r,
            color: c,
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [SchattenArt::LEER; MAX_SCHATTEN],
            schatten_anzahl: 0,
        };
        assert_eq!(DisplayItem::rechteck(r, c), von_hand);
        // und die Vorgaben einzeln, damit die Fehlermeldung sagt, WELCHE sich aenderte:
        match DisplayItem::rechteck(r, c) {
            DisplayItem::Rect {
                radius,
                deckkraft,
                schatten_anzahl,
                ..
            } => {
                assert_eq!(radius, [0.0; 4], "Vorgabe: eckig");
                assert_eq!(deckkraft, 1.0, "Vorgabe: deckend");
                assert_eq!(schatten_anzahl, 0, "Vorgabe: kein Schatten");
            }
            anderes => panic!("rechteck lieferte {}", anderes.kind_name()),
        }
    }

    #[test]
    fn push_preserves_order() {
        let mut dl = DisplayList::new();
        dl.push(ecklos(Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE));
        dl.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "hi".into(),
            color: Color::BLACK,
            font_size: 10.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        assert_eq!(dl.len(), 2);
        assert_eq!(dl.get(0).unwrap().kind_name(), "rect");
        assert_eq!(dl.get(1).unwrap().kind_name(), "text");
        assert_eq!(dl.count_of("text"), 1);
        assert_eq!(dl.count_of("rect"), 1);
    }

    #[test]
    fn bounds_covers_rects() {
        let mut dl = DisplayList::new();
        dl.push(ecklos(Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE));
        dl.push(ecklos(Rect::new(20.0, 30.0, 5.0, 5.0), Color::BLACK));
        assert_eq!(dl.bounds(), Some(Rect::new(0.0, 0.0, 25.0, 35.0)));
    }

    #[test]
    fn background_is_stored_but_not_in_bounds() {
        let dl = DisplayList::with_background(Color::BLACK);
        assert_eq!(dl.background, Some(Color::BLACK));
        assert!(dl.bounds().is_none());
    }

    #[test]
    fn text_bounds_estimate() {
        let mut dl = DisplayList::new();
        dl.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(5.0, 5.0),
            text: "abcd".into(),
            color: Color::WHITE,
            font_size: 10.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        let b = dl.bounds().unwrap();
        assert_eq!(b.x, 5.0);
        assert_eq!(b.y, 5.0);
        assert_eq!(b.height, 10.0);
        assert!(b.width > 0.0);
    }
}
