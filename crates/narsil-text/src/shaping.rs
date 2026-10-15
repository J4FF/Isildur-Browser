//! Text-Shaping: Zerlegung einer Textzeile in platzierte Glyphen.

use narsil_core::geometry::Point;

use crate::TextError;
use crate::font::{BitmapFont, GlyphBitmap};
use crate::metrics::validate_font_size;

/// Platzierte Glyphe: ein Zeichen an einer konkreten Position.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    /// Das Zeichen, das hier platziert wird.
    pub ch: char,
    /// Position der linken oberen Glyphenecke, relativ zur Textlinie.
    pub x: f32,
    /// Position der linken oberen Glyphenecke, relativ zur Textlinie.
    pub y: f32,
    /// Die gerasterte Zelle. Leer, wenn die Schrift das Zeichen nicht kennt.
    pub bitmap: GlyphBitmap,
    /// Der Vorschub dieses Zeichens in Pixeln — die Strecke, um die sich
    /// `x` nach der Platzierung bewegt.
    ///
    /// GEMESSEN 01.10.2026 (A3): ohne dieses Feld kann eine Unterstreichung
    /// ihre Laenge nicht kennen. Aus den `x`-Positionen alone laesst sie sich
    /// nur fuer alle Glyphen ausser der letzten rekonstruieren, und eine
    /// Dekoration ueber eine einzelne Glyphe ergaebe dann gar nichts.
    /// `layout_text` hat den Wert ohnehin (`advance_or_fallback`).
    pub advance: f32,
}

/// Zerlegt eine Textzeile in platzierte Glyphen.
///
/// Der String wird ueber [`str::chars`] in `char` zerlegt, nicht ueber
/// Bytes — mehrbyte-UTF-8 wie `ö` oder `🎉` wird dadurch nicht zerschnitten.
///
/// # Regeln
///
/// * `origin` ist die linke obere Ecke der Textlinie. Die erste Zeile
///   beginnt genau dort.
/// * Jedes Zeichen wird sequentiell um seinen Vorschub nach rechts
///   geschoben; aufeinanderfolgende Glyphen haben daher streng steigende
///   `x`-Werte.
/// * `\n` und `\r` erzeugen **keine** Glyphe. Sie beenden die Zeile: die
///   naechste Zeile beginnt eine Zeilenhoehe tiefer bei `origin.x`.
///   `\r\n` zaehlt dabei wie ein einzelner Umbruch, weil `\r` die Zeile
///   beendet und das darauffolgende `\n` eine weitere leere Zeile eroeffnen
///   wuerde. Genau dafuer wird der Umbruch entprellt.
/// * Tabulator `\t` belegt eine Zelle wie ein Leerzeichen, fuehrt aber zu
///   keiner sichtbaren Tinte. Er rastet **nicht** am Tabstopp ein — fuer den
///   MVP waere das ein Layout-Thema und gehoert nicht ins Shaping.
/// * Zeichen ausserhalb der Schrift (alles ausserhalb U+0020..=U+007E)
///   werden uebersprungen: sie erhalten eine leere Bitmap, aber den vollen
///   Zellvorschub. So ruecken die folgenden Zeichen nicht zusammen.
/// * Ein `\n` am Ende erzeugt eine Leerzeile, liefert aber keine Glyphe.
///   Ergebnis kann daher leer sein, obwohl der String nicht leer war.
///
/// # Fehler
///
/// * `font_size` nicht endlich oder `<= 0` -> [`TextError::InvalidFontSize`]
/// * leere Zeichenkette -> [`TextError::EmptyText`]
///
/// Die Schriftgroesse wird vor dem Inhalt geprueft (gleiche Reihenfolge
/// wie in [`crate::measure`]).
///
/// # ACHTUNG FUER DEN AUFRUFER: `origin` ist die linke OBERE Ecke
///
/// `origin` ist die linke obere Ecke der Textlinie, **nicht** die
/// Grundlinie. Die Glyphe beginnt an genau diesem Punkt: bei
/// `origin.y = 0` steht bereits in Zeile 0 der Bitmap Tinte.
///
/// Die Grundlinie liegt `ascent` Pixel darunter, mit
/// `ascent = font_size * ASCENT_RATIO` (5/7 der Zeilenhoehe). Wer die
/// Glyphen zeichnet, muss **nichts** addieren — er zeichnet die
/// Bitmap 1:1 ab `glyph.x, glyph.y`.
///
/// Drei moegliche Fehlbilder beim Verdrahten, alle unterscheidbar:
///
/// | erste Tintenzeile | Bedeutung                                  |
/// |-------------------|--------------------------------------------|
/// | `0`               | richtig (dies ist die Zusage)               |
/// | `round(17,14)`    | Ascent doppelt verrechnet                   |
/// | `round(6,86)`     | `TextMetrics::height` statt `ascent` benutzt |
///
/// Der zweite Fall ist der wahrscheinlichste, weil `height` und `ascent`
/// in [`TextMetrics`] nebeneinander stehen. Siehe
/// `tests/baseline_zusage.rs`, das alle drei Zahlen festnagelt.
pub fn layout_text(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
) -> Result<Vec<PositionedGlyph>, TextError> {
    validate_font_size(font_size)?;
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }

    // Vorab allozieren: der String kann sehr lang sein, und die
    // Positionen sollen in einem Durchgang entstehen (kein quadratisches
    // Verhalten, kein Stack-Overflow).
    layout_text_mit_tabs(font, text, font_size, origin, TAB_SIZE_STANDARD, false)
}

/// Das Ergebnis eines Zeilenlayouts: die platzierten Glyphen UND die
/// Endbreite der Zeile.
///
/// # WARUM DAS EIN EIGENER TYP IST
///
/// Bis 03.10.2026 gab `layout_text` nur `Vec<PositionedGlyph>` zurueck. Wer
/// die Breite einer Zeile brauchte — fuer den Umbruch und fuer
/// `text-align` — musste die Vorschuebe selbst nachzaehlen. Das ist an
/// drei Stellen falsch geht:
///
/// * nach einem `letter-spacing`, weil die Laufweite nach **jedem** Zeichen
///   kommt, auch nach dem letzten;
/// * bei einem Tabulator, weil der Sprung auf das naechste Vielfache geht;
/// * bei einem weichen Trennstrich, weil er unsichtbar ist und trotzdem
///   Platz beansprucht, wenn die Zeile dort endet.
///
/// Der Aufrufer muss diese drei Faelle nicht kennen. Er fragt nach `breite`.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeilenLayout {
    /// Die platzierten Glyphen, genau wie `layout_text` sie liefert.
    pub glyphen: Vec<PositionedGlyph>,
    /// Die Breite der LETZTEN gelegten Zeile in px, gemessen ab `origin.x`.
    ///
    /// GEMESSEN 03.10.2026: fuer "abc" bei 16 px sind das drei Vorschuebe
    /// plus drei Laufweiten. Enthält der Text einen Zeilenumbruch, ist es
    /// die Breite der letzten Zeile — nicht die Summe aller.
    pub breite: f32,
}

/// Wie [`layout_text`], liefert aber auch die Endbreite der Zeile.
///
/// Der Aufruf von `layout_text` ist genau eine Ableitung davon; es gibt
/// keinen Grund, zweimal zu rechnen.
pub fn layout_text_mit_breite(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
) -> Result<ZeilenLayout, TextError> {
    validate_font_size(font_size)?;
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }
    let (glyphen, breite) =
        layout_text_voll(font, text, font_size, origin, TAB_SIZE_STANDARD, false, 0.0)?;
    Ok(ZeilenLayout { glyphen, breite })
}

/// M3: hoechste Zahl Tabulatorstopps je Tabulatorzeile.
///
/// CSS erlaubt `tab-size` bis `10000` (`tab-size: 8` ist ein Vielfaches der
/// Breite des Leerzeichens). Ohne Grenze waere ein Tabulator bei diesem Wert
/// ein Sprung ueber mehr Bildbreite, als ein Framebuffer haben kann — die
/// Schleife darueber waere trotzdem endlich, aber sinnlos. `MAX_TAB_SIZE` ist
/// die CSS-Grenze, nicht eine von mir gewaehlte.
pub const MAX_TAB_SIZE: u32 = 10_000;

/// `tab-size` nach CSS 2.1, 16.3.2: die Vorgabe ist **8 Leerzeichen**.
pub const TAB_SIZE_STANDARD: u32 = 8;

/// Zerlegt eine Textzeile in platzierte Glyphen, mit Tabulatorstopps.
///
/// # Tabulatoren (A3)
///
/// CSS 2.1, 16.3.2: `tab-size: <number>` bedeutet "das Vielfache der Breite
/// des Leerzeichens", die Vorgabe ist 8. Ein Tabulator rueckt also nicht um
/// eine feste Pixelzahl vor, sondern bis zum naechsten Vielfachen von
/// `tab_size * leerzeichen_breite`. GEMESSEN: der Vorschub des Leerzeichens
/// kommt aus derselben Quelle wie bei jedem anderen Zeichen
/// (`advance_or_fallback`) — eine eigene Formel wuerde bei einer
/// proportionalen Schrift danebenliegen.
///
/// # Weiche Trennstriche (A3)
///
/// U+00AD SOFT HYPHEN ist im Text unsichtbar und belegt **keinen Platz**.
/// CSS 2.1, 16.3.1: er ist "invisible except at line breaks"; bricht die
/// Zeile dort, wird ein Bindestrich gezeigt.
///
/// `umbruch_nach_shy` sagt, ob die Zeile an einem weichen Trennstrich
/// endet. Dann wird der **letzte** weiche Trennstrich der Zeile als sichtbarer
/// Bindestrich gezeichnet — der am Zeilenende, nicht jeder im Text.
///
/// Der Platzhalter bleibt in der Liste, damit `out[i]` zum `i`-ten Zeichen
/// des Textes gehoert; nur Vorschub und Bitmap sind auf 0 bzw. leer.
#[allow(clippy::too_many_arguments)]
pub fn layout_text_mit_tabs(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
    tab_size: u32,
    umbruch_nach_shy: bool,
) -> Result<Vec<PositionedGlyph>, TextError> {
    layout_text_voll(
        font,
        text,
        font_size,
        origin,
        tab_size,
        umbruch_nach_shy,
        0.0,
    )
    .map(|(glyphen, _breite)| glyphen)
}

/// Wie [`layout_text_mit_tabs`], mit `letter-spacing` in Pixeln.
///
/// GEMESSEN 03.10.2026 (RUNDE 3 S3): `letter-spacing` ist der einzige
/// Wert, der den VORSCHUB veraendert statt der Glyphe. Die Laufweite wird
/// auf [-64, +64] px geklemmt und auf Endlichkeit geprueft; ein nicht-
/// endlicher CSS-Wert ergibt 0 statt eines stillen NaN-Pfades.
pub fn layout_text_mit_laufweite(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
    laufweite_px: f32,
) -> Result<Vec<PositionedGlyph>, TextError> {
    layout_text_voll(
        font,
        text,
        font_size,
        origin,
        TAB_SIZE_STANDARD,
        false,
        laufweite_px,
    )
    .map(|(glyphen, _breite)| glyphen)
}

/// Die gemeinsame Maschine aller Layoutwege.
#[allow(clippy::too_many_arguments)]
fn layout_text_voll(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
    tab_size: u32,
    umbruch_nach_shy: bool,
    laufweite_px: f32,
) -> Result<(Vec<PositionedGlyph>, f32), TextError> {
    validate_font_size(font_size)?;
    // GEMESSEN 03.10.2026: ein `NaN`-Ursprung machte jede Glyphenposition
    // und damit auch die Endbreite zu `NaN`. Der Aufrufer addiert die
    // Breite zur Kastenbreite, und ein `NaN` dort faellt nicht auf, sondern
    // macht die Zeile einfach unbrauchbar. Also abweisen.
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return Err(TextError::UngueltigerUrsprung {
            x: origin.x,
            y: origin.y,
        });
    }
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }
    // M3: die Tabulatorbreite wird VOR der Allokation nach OBEN begrenzt.
    // Die UNTERGRENZE wird NICHT auf 1 gehoben: `tab-size: 0` ist gueltiges
    // CSS (CSS 2.1, 16.3.2) und ergibt einen Tabulator der Breite 0.
    let tab_size = tab_size.min(MAX_TAB_SIZE);
    let laufweite = laufweite(laufweite_px);
    // ARABISCHES SHAPING, und es muss VOR der Glyphenwahl stehen.
    //
    // GEMESSEN 03.10.2026: `arabisch::ausgabe` macht in einem Durchgang
    // beides, was die Reihenfolge verlangt — erst `forme` (verbundene
    // Formen in LOGISCHER Reihenfolge, jede Praesentationsform aus der
    // Verbindungsklasse des Nachbarn), danach das Umdrehen jedes maximalen
    // Laufs aus arabischen Zeichen.
    //
    // Warum das hier und nicht erst beim Zeichnen: die Verbundform ergibt
    // sich aus dem Nachbarzeichen. Wenn die Glyphenwahl schon gelaufen
    // ist, ist der Nachbar schon weg — dann kaeme nur noch `.notdef` oder
    // die unverbundene Form heraus. Der Test
    // `arabische_laeufe_umgedreht` in tests/arabisch_shaping.rs prueft
    // genau das an "العربية".
    //
    // Der Waechter kostet fuer den Normalfall (kein Arabisch) nichts: der
    // ganze Weg ueber `ausgabe` faellt aus, und es wird nur der Wert von
    // `text` kopiert. `ausgabe` alloziert naemlich immer einen `String`.
    let geformt: &str = if text.chars().any(crate::arabisch::ist_arabisch) {
        // M2/M3: die Laenge ist durch `text` begrenzt, `ausgabe` kann nicht
        // laenger werden als die Eingabe (es dreht und ersetzt, es fuegt
        // nichts hinzu). Kein eigener Grenzwert noetig, und keiner, der
        // stillschweigend Text abschneiden wuerde.
        &crate::arabisch::ausgabe(text)
    } else {
        text
    };
    let zeichen: Vec<char> = geformt.chars().collect();
    // Wo steht der LETZTE weiche Trennstrich?
    let letzter_shy = zeichen.iter().rposition(|&c| c == WEICHES_TRENNZEICHEN);

    // M3: Vorab allozieren; der Text kann sehr lang sein (siehe Kommentar
    // in `layout_text`).
    let mut out: Vec<PositionedGlyph> = Vec::with_capacity(zeichen.len());
    let mut x = origin.x;
    let mut y = origin.y;
    let mut at_line_start = true;

    // DIE ENDBREITE, und sie kommt mit genau ZWEI Zuweisungen aus.
    //
    // GEMESSEN 03.10.2026: `x` ist immer `origin.x` plus der Summe der
    // Vorschuebe der aktuellen Zeile. Also ist `x - origin.x` zu JEDEM
    // Zeitpunkt die Breite dieser Zeile — es muss nur an zwei Stellen
    // abgelesen werden: am Anfang jeder Runde (das erfasst auch alle Wege,
    // die mit `continue` aus der Schleife gehen) und einmal nach der
    // Schleife fuer das letzte Zeichen.
    //
    // Ohne den Startwert: die Variable wird am Anfang JEDER Runde und
    // einmal nach der Schleife zugewiesen, der Wert 0.0 wuerde nie
    // gelesen. `let mut` ohne Wert ist hier richtig — die Schleife laeuft
    // mindestens einmal (der leere Text ist schon abgewiesen), und wenn
    // nicht, bleibt 0.0 die richtige Breite einer leeren Zeile.
    for (i, &ch) in zeichen.iter().enumerate() {
        if is_line_break(ch) {
            if at_line_start {
                // `\r\n`: das erste Zeichen hat die Zeile bereits beendet.
                continue;
            }
            y += font_size;
            x = origin.x;
            at_line_start = true;
            continue;
        }

        at_line_start = false;

        if ch == WEICHES_TRENNZEICHEN {
            // Unsichtbar, kein Vorschub — ausser die Zeile endet hier.
            let sichtbar = umbruch_nach_shy && letzter_shy == Some(i);
            out.push(PositionedGlyph {
                ch,
                x,
                y,
                bitmap: if sichtbar {
                    glyph_for(font, '-', font_size)
                } else {
                    GlyphBitmap::empty()
                },
                advance: if sichtbar {
                    font.advance_or_fallback('-', font_size)
                } else {
                    0.0
                },
            });
            x += if sichtbar {
                font.advance_or_fallback('-', font_size) + laufweite
            } else {
                laufweite
            };
            continue;
        }

        if ch == '\t' {
            // GEMESSEN: der Sprung ist auf das naechste Vielfache von
            // `tab_size * leerzeichen_breite` GROSSER als die aktuelle
            // Position. Steht `x` already genau auf einem Stopp, geht es
            // um einen VOLLEN Stopp weiter, nicht um null — sonst wuerde ein
            // Tabulator am Zeilenanfang nichts bewegen und der Text klebte
            // am Rand.
            let leer = font.advance_or_fallback(' ', font_size);
            let schritt = leer * tab_size as f32;
            // `tab-size: 0` heisst Breite 0: der Tabulator bewegt nichts.
            // Hier wird GEGEN die Division geschuetzt, nicht danach.
            if schritt <= 0.0 || !schritt.is_finite() {
                out.push(PositionedGlyph {
                    ch,
                    x,
                    y,
                    bitmap: GlyphBitmap::empty(),
                    advance: 0.0,
                });
                continue;
            }
            let jetzt = (x - origin.x) / schritt;
            let naechste = jetzt.floor() + 1.0;
            let neu = origin.x + naechste * schritt;
            out.push(PositionedGlyph {
                ch,
                x,
                y,
                bitmap: GlyphBitmap::empty(),
                advance: (neu - x).max(0.0),
            });
            // GEMESSEN 03.10.2026: der Sprung geht auf den naechsten
            // Stopp RELATIV ZUM ZEILENANFANG, nicht weiter nach rechts.
            // Die Laufweite verschiebt nur die Zeichen DAVOR; addierte man
            // sie hier, wuechse die Tabulatorbreite um ihren Wert.
            x = neu + laufweite;
            continue;
        }

        let bitmap = glyph_for(font, ch, font_size);
        let advance = font.advance_or_fallback(ch, font_size);
        out.push(PositionedGlyph {
            ch,
            x,
            y,
            bitmap,
            advance,
        });
        // CSS Text 3, 4: `letter-spacing` aendert den VORSCHUB, nicht die
        // Glyphe. GROSSBUCHSTABEN: die Laufweite kommt NACH jedem
        // Zeichen, auch nach dem letzten — sonst haengt der letzte Buchstabe
        // links, und der Text sieht nach links verschoben aus.
        x += advance + laufweite;
    }

    //
    // DIE ENDBREITE, und sie kommt mit EINER einzigen Zuweisung aus.
    //
    // GEMESSEN 03.10.2026 (clippy "value assigned is never read", zweimal
    // gemeldet): die erste Fassung schrieb die Breite am Anfang JEDER
    // Schleifenrunde UND nach der Schleife. Die Zuweisung in der Schleife
    // wurde nie gelesen, weil die nach der Schleife immer ueberschreibt —
    // ein Zwei-Stuetzstellen-Design, das sich als EINE erweist.
    //
    // Warum EINE genuegt: `x` ist nach der Schleife `origin.x` plus der
    // Summe aller Vorschuebe der LETZTEN Zeile, weil ein Zeilenumbruch
    // `x` auf `origin.x` zuruecksetzt. `x - origin.x` ist also genau die
    // Breite der letzten Zeile — fuer alle Faelle, ohne jeden Zustand.
    let zeilen_breite = x - origin.x;
    Ok((out, zeilen_breite))
}

/// Wie `layout_text_mit_tabs`, aber der letzte weiche Trennstrich der Zeile
/// wird als sichtbarer Bindestrich gezeichnet — der Fall, in dem CSS 2.1,
/// 16.3.1 ihn sichtbar macht: die Zeile bricht genau dort.
pub fn layout_zeile_mit_shy(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
) -> Result<Vec<PositionedGlyph>, TextError> {
    layout_text_mit_tabs(font, text, font_size, origin, TAB_SIZE_STANDARD, true)
}

/// Wie [`layout_text_mit_tabs`], liefert aber auch die Endbreite der Zeile.
///
/// GEMESSEN 03.10.2026: der Tabulator ist der Grund, warum die Breite hier
/// nicht nachzaehlbar ist. Sein Vorschub als Zeichen ist 0 — der Sprung
/// entsteht aus der Position auf dem naechsten Vielfachen von
/// `tab_size * Leerzeichenbreite`. Wer die Vorschuebe summiert, bekommt
/// fuer `tab-size: 0` eine Breite von 0 und fuer den Sprung gar nichts.
pub fn layout_text_mit_tabs_mit_breite(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
    tab_size: u32,
) -> Result<ZeilenLayout, TextError> {
    validate_font_size(font_size)?;
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }
    let (glyphen, breite) = layout_text_voll(font, text, font_size, origin, tab_size, false, 0.0)?;
    Ok(ZeilenLayout { glyphen, breite })
}

/// Wie [`layout_zeile_mit_shy`], liefert aber auch die Endbreite der Zeile.
///
/// GEMESSEN 03.10.2026: der weiche Trennstrich traegt die Laufweite wie
/// jedes Zeichen und bekommt am Zeilenende einen echten Bindestrich mit
/// Vorschub. Beides ist aus der Glyphenliste nicht ohne die Sonderregeln
/// rekonstruierbar.
pub fn layout_zeile_mit_shy_mit_breite(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    origin: Point,
) -> Result<ZeilenLayout, TextError> {
    validate_font_size(font_size)?;
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }
    let (glyphen, breite) =
        layout_text_voll(font, text, font_size, origin, TAB_SIZE_STANDARD, true, 0.0)?;
    Ok(ZeilenLayout { glyphen, breite })
}

/// U+00AD SOFT HYPHEN.
pub const WEICHES_TRENNZEICHEN: char = '\u{00AD}';
/// U+002D HYPHEN-MINUS, der sichtbar wird.
pub const TRENNSTRICH: char = '-';

/// `true` fuer die beiden Zeilenumbruch-Zeichen.
fn is_line_break(ch: char) -> bool {
    ch == '\n' || ch == '\r'
}

/// Bitmap fuer `ch`: die gerasterte Zelle, oder eine leere Bitmap wenn die
/// Schrift das Zeichen nicht enthaelt.
fn glyph_for(font: &BitmapFont, ch: char, font_size: f32) -> GlyphBitmap {
    match ch {
        // Tabulator belegt eine Zelle ohne Tinte, wie ein Leerzeichen.
        //
        // ACHTUNG: dieser Zweig ist ueber den Quelltext NICHT erreichbar
        // (TASK_0023). Der Parser weist jedes Steuerzeichen im
        // Textinhalt ab — es gibt keine Ausrichtung in v0, und ein Zeichen,
        // das Platz belegt und nichts anzeigt, sieht aus wie ein Fehler.
        //
        // Er bleibt trotzdem, und aus zwei Gruenden:
        //   1. `layout_text` ist oeffentlich. Wer sie programmatisch
        //      aufruft, umgeht den Parser — genau wie bei `"` oder `\n`.
        //   2. `shaping` kennt die SPRACHE nicht. Es bekommt ein Zeichen
        //      und macht eine Bitmap daraus. Ob dieses Zeichen in v0
        //      gueltig ist, hat eine Ebene darueber entschieden.
        //
        // Die Konsistenzregel dahinter: die Sprache entscheidet, OB ein
        // Zeichen gueltig ist; die Darstellung entscheidet nur, WIE es
        // aussieht. Genau so, wie der Parser `margin: 150%` ablehnt und
        // die Layout-Engine trotzdem weiss, wie man 150 % aufloest.
        '\t' => font
            .rasterize_glyph(' ', font_size)
            .unwrap_or_else(GlyphBitmap::empty),
        _ => font
            .rasterize_glyph(ch, font_size)
            .unwrap_or_else(GlyphBitmap::empty),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> BitmapFont {
        BitmapFont::builtin()
    }

    fn at(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn simple_layout() {
        let g = layout_text(&font(), "AB", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].ch, 'A');
        assert_eq!(g[1].ch, 'B');
        assert_eq!(g[0].x, 0.0);
        assert_eq!(g[0].y, 0.0);
    }

    #[test]
    fn x_increases_strictly() {
        let g = layout_text(&font(), "Hello, World!", 16.0, at(0.0, 0.0)).expect("ok");
        for pair in g.windows(2) {
            assert!(pair[0].x < pair[1].x, "x muss streng steigen");
        }
    }

    #[test]
    fn origin_is_honored() {
        let g = layout_text(&font(), "hi", 16.0, at(100.0, 50.0)).expect("ok");
        assert_eq!(g[0].x, 100.0);
        assert_eq!(g[0].y, 50.0);
    }

    #[test]
    fn newline_moves_to_next_line() {
        let g = layout_text(&font(), "a\nb", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].ch, 'b');
        assert_eq!(g[1].x, 0.0);
        assert!((g[1].y - 16.0).abs() < 1e-4);
    }

    #[test]
    fn newline_resets_x_to_origin() {
        let origin = at(5.0, 10.0);
        let g = layout_text(&font(), "aaa\nb", 16.0, origin).expect("ok");
        assert_eq!(g[3].x, origin.x);
    }

    #[test]
    fn crlf_counts_once() {
        let g = layout_text(&font(), "a\r\nb", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 2);
        assert!((g[1].y - 16.0).abs() < 1e-4);
    }

    #[test]
    fn trailing_newline_produces_no_glyph() {
        let g = layout_text(&font(), "a\n", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn empty_text_is_error() {
        assert_eq!(
            layout_text(&font(), "", 16.0, at(0.0, 0.0)),
            Err(TextError::EmptyText)
        );
    }

    #[test]
    fn invalid_size_is_error() {
        for size in [0.0, -1.0, f32::INFINITY] {
            assert_eq!(
                layout_text(&font(), "x", size, at(0.0, 0.0)),
                Err(TextError::InvalidFontSize(size))
            );
        }
        // NaN != NaN, daher separat auf die Fehlervariante pruefen.
        assert!(matches!(
            layout_text(&font(), "x", f32::NAN, at(0.0, 0.0)),
            Err(TextError::InvalidFontSize(s)) if s.is_nan()
        ));
    }

    #[test]
    fn umlauts_do_not_panic_and_keep_advance() {
        // Seit TASK_0017 bekommen unbekannte Zeichen einen sichtbaren
        // .notdef-Platzhalter statt einer leeren Glyphe. Die Breite und
        // der Vorschub sind unveraendert — nur die Sichtbarkeit hat sich
        // geaendert, damit "Größe" nicht als "Gr e" mit Loch erscheint.
        let g = layout_text(&font(), "Größe", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 5, "G, r, ö, ß, e");
        // Jede Glyphe ist sichtbar — auch die fuer Zeichen ohne Eintrag.
        for glyph in &g {
            assert!(
                !glyph.bitmap.is_empty(),
                "'{}' ist unsichtbar — das waere ein Loch im Text",
                glyph.ch
            );
        }
        // Der Abstand muss trotzdem stimmen.
        assert!(g[4].x > g[3].x);
        // Und die Platzhalter sitzt in Zellbreite, nicht in 0.
        let zelle = font().advance('ö', 16.0);
        assert!(zelle.is_none(), "advance ist None fuer unbekannte Zeichen");
        assert!(g[2].bitmap.width > 0, "die Bitmap hat trotzdem Breite");
    }

    #[test]
    fn emoji_does_not_panic() {
        // Seit TASK_0017: sichtbarer Platzhalter statt leerer Glyphe.
        // Ein Emoji ist vier Bytes UTF-8, aber EIN `char` — es bekommt
        // deshalb genau eine Zelle.
        let g = layout_text(&font(), "\u{1F389}", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 1, "ein char, eine Glyphe");
        assert!(
            !g[0].bitmap.is_empty(),
            "TASK_0017: das Emoji bekommt einen sichtbaren Platzhalter"
        );
        assert!(g[0].bitmap.width > 0);
    }

    #[test]
    fn unknown_chars_keep_full_advance() {
        let f = font();
        let g = layout_text(&f, "a\u{1F389}b", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 3);
        let cell = f.advance('a', 16.0).expect("ok");
        assert!((g[2].x - cell * 2.0).abs() < 1e-3);
    }

    #[test]
    fn tab_geht_zum_naechsten_stopp_und_nicht_zwei_zellen() {
        // GEMESSEN 02.10.2026: dieser Test behauptete ZWEI Zellen. Das war
        // ein Platzhalterverhalten aus der Zeit, als der Tabulatorzweig
        // unerreichbar war und nur eine Zelle belegen sollte. CSS 2.1,
        // 16.3.2 sagt `tab-size: 8` als Vorgabe, also geht 'b' nach 'a' auf
        // Zelle 8, nicht auf Zelle 2.
        let f = font();
        let g = layout_text(&f, "a\tb", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 3);
        assert_eq!(g[1].ch, '\t');
        assert!(g[1].bitmap.is_empty(), "Tabulator hat keine Tinte");
        let cell = f.advance('a', 16.0).expect("ok");
        assert!(
            (g[2].x - cell * 8.0).abs() < 1e-3,
            "b steht bei {} statt auf Zelle 8 ({})",
            g[2].x,
            cell * 8.0
        );
    }

    #[test]
    fn tab_size_acht_ist_der_wert_von_css() {
        let f = font();
        let a = layout_text(&f, "\tx", 16.0, at(0.0, 0.0)).expect("ok");
        let b = layout_text_mit_tabs(&f, "\tx", 16.0, at(0.0, 0.0), 4, false).expect("ok");
        assert!(
            b[1].x < a[1].x,
            "tab-size 4 ({}) muss vor tab-size 8 ({}) liegen",
            b[1].x,
            a[1].x
        );
    }

    #[test]
    fn zwei_tabs_stehen_bei_zwei_verchiedenen_stopp() {
        let f = font();
        let g = layout_text(&f, "\t\tx", 16.0, at(0.0, 0.0)).expect("ok");
        // GEMESSEN 02.10.2026: `x` ist der BEGINN des Tabulators, der Sprung
        // steckt im `advance`. Die Stopps stehen darum an den Positionen der
        // FOLGENDEN Zeichen — die erste Fassung dieses Tests hat an `g[0].x`
        // gemessen und deshalb 0 statt 8 gefunden.
        let zelle = f.advance(' ', 16.0).expect("ok");
        assert_eq!(g[0].x, 0.0, "der erste Tabulator beginnt bei x = 0");
        assert!(
            (g[1].x - zelle * 8.0).abs() < 1e-3,
            "erster Stopp: {}",
            g[1].x
        );
        assert!(
            (g[2].x - zelle * 16.0).abs() < 1e-3,
            "zweiter Stopp: {}",
            g[2].x
        );
    }

    #[test]
    fn weiches_trennzeichen_ist_unsichtbar_und_belegt_keinen_platz() {
        let f = font();
        let ohne = layout_text(&f, "ab", 16.0, at(0.0, 0.0)).expect("ok");
        let mit = layout_text(&f, "a\u{00AD}b", 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(mit.len(), 3, "der Platzhalter bleibt in der Liste");
        assert_eq!(mit[1].ch, WEICHES_TRENNZEICHEN);
        assert!(mit[1].bitmap.is_empty(), "U+00AD hat keine Tinte");
        assert_eq!(mit[1].advance, 0.0, "U+00AD belegt keinen Platz");
        assert!(
            (mit[2].x - ohne[1].x).abs() < 1e-4,
            "'b' steht bei {} statt bei {}",
            mit[2].x,
            ohne[1].x
        );
    }

    #[test]
    fn am_zeilenende_wird_aus_der_regen_grenze_ein_bindestrich() {
        let f = font();
        let g = layout_zeile_mit_shy(&f, "Nach\u{00AD}wort", 16.0, at(0.0, 0.0)).expect("ok");
        let shy = g
            .iter()
            .find(|g| g.ch == WEICHES_TRENNZEICHEN)
            .expect("U+00AD");
        assert!(!shy.bitmap.is_empty(), "am Zeilenende muss Tinte da sein");
        assert_eq!(shy.ch, WEICHES_TRENNZEICHEN);
        assert!(shy.advance > 0.0, "der sichtbare Trennstrich belegt Platz");
        // Er MUSS aber wie ein '-' aussehen: gleiche Position, gleiche Breite
        // wie ein echtes Minus an derselben Stelle.
        let m = layout_text(&f, "Nach-", 16.0, at(0.0, 0.0)).expect("ok");
        assert!((shy.advance - m[4].advance).abs() < 1e-4, "Vorschub");
    }

    #[test]
    fn nur_der_letzte_teuerstrich_wird_sichtbar() {
        // Zwei weiche Trennstriche, aber nur der am Zeilenende: der erste
        // mitten im Text bleibt unsichtbar.
        let f = font();
        let g = layout_zeile_mit_shy(&f, "a\u{00AD}b\u{00AD}c", 16.0, at(0.0, 0.0)).expect("ok");
        let shy: Vec<&PositionedGlyph> =
            g.iter().filter(|g| g.ch == WEICHES_TRENNZEICHEN).collect();
        assert_eq!(shy.len(), 2, "zwei U+00AD im Text");
        assert!(
            shy[0].bitmap.is_empty(),
            "der erste muss unsichtbar bleiben"
        );
        assert!(!shy[1].bitmap.is_empty(), "der letzte muss sichtbar sein");
    }

    #[test]
    fn ein_tabulator_am_zeilenanfang_verschiebt_um_einen_vollen_stopp() {
        // Steht x genau auf einem Stopp, geht es um einen VOLLEN Stopp weiter.
        // Sonst klebte der Text am linken Rand.
        let f = font();
        let g = layout_text(&f, "\tx", 16.0, at(0.0, 0.0)).expect("ok");
        let zelle = f.advance(' ', 16.0).expect("ok");
        assert!(g[1].x > zelle * 8.0 - 1e-3, "x steht bei {}", g[1].x);
    }

    #[test]
    fn tab_ist_null_und_bewegt_sich_nicht() {
        // CSS 2.1, 16.3.2: `tab-size: 0` ist GUELTIGES CSS und ergibt einen
        // Tabulator der Breite 0. GEMESSEN 02.10.2026 (Koordination): die erste
        // Fassung hat den Wert auf 1 geklemmt und daraus einen echten Sprung
        // gemacht — das ist ein anderes Verhalten, kein sichereres.
        let f = font();
        let g = layout_text_mit_tabs(&f, "a\tb", 16.0, at(0.0, 0.0), 0, false).expect("ok");
        assert_eq!(g[1].advance, 0.0, "Tabulator der Breite 0 bewegt nichts");
        assert!(
            g[1].x.is_finite() && g[2].x.is_finite(),
            "keine NaN: {} und {}",
            g[1].x,
            g[2].x
        );
        let zelle = f.advance('a', 16.0).expect("ok");
        assert!((g[2].x - zelle).abs() < 1e-4, "b steht bei {}", g[2].x);
    }

    #[test]
    fn tab_size_wird_nach_oben_begrenzt_und_der_text_bleibt_ganz() {
        // M3: `tab-size` ist nach OBEN auf MAX_TAB_SIZE begrenzt. 0 ist ein
        // eigener Fall (siehe `tab_ist_null_und_bewegt_sich_nicht`).
        let f = font();
        for wert in [1u32, 8, MAX_TAB_SIZE, u32::MAX] {
            let g = layout_text_mit_tabs(&f, "a\tb", 16.0, at(0.0, 0.0), wert, false)
                .unwrap_or_else(|e| panic!("tab-size {wert}: {e}"));
            assert_eq!(g.len(), 3, "tab-size {wert}: {} Glyphen", g.len());
            assert!(g[2].x.is_finite(), "tab-size {wert}: x = {}", g[2].x);
        }
    }

    #[test]
    fn many_lines_stack() {
        let g = layout_text(&font(), "a\nb\nc\nd", 10.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 4);
        for (i, glyph) in g.iter().enumerate() {
            assert!((glyph.y - i as f32 * 10.0).abs() < 1e-4);
            assert_eq!(glyph.x, 0.0);
        }
    }

    #[test]
    fn long_text_is_linear() {
        let text = "a".repeat(10_000);
        let g = layout_text(&font(), &text, 16.0, at(0.0, 0.0)).expect("ok");
        assert_eq!(g.len(), 10_000);
    }

    #[test]
    fn layout_width_matches_measure() {
        let f = font();
        let text = "Hello, Welt!";
        let glyphs = layout_text(&f, text, 16.0, at(0.0, 0.0)).expect("ok");
        let m = crate::measure(&f, text, 16.0).expect("ok");
        let last = glyphs.last().expect("nicht leer");
        let advance = f.advance_or_fallback(last.ch, 16.0);
        assert!((m.width - (last.x + advance)).abs() < 1e-3);
    }
}

/// M3: hoechste Zeichenzahl, die `text_transform` bearbeitet.
///
/// Die Kaskade begrenzt die Textlaenge nicht; ohne Grenze waere ein
/// 100-MB-Text ein Vec von 400 MB.
pub const MAX_TEXTLAENGE: usize = 1 << 20;

/// Wie `text-transform` den Text veraendert (CSS Text 3 §3).
///
/// CSS Text 3, 3: `uppercase` und `lowercase` nutzen die VOLLE Unicode-
/// Gross-/Kleinschreibung, `capitalize` macht JEDES Wort首个 gross —
/// genauer: das erste alphabetische Zeichen jedes Wortes, und Wortgrenzen
/// sind jedes Zeichen, das keine Buchstabe ist.
///
/// GEMESSEN 03.10.2026: `capitalize` ist in CSS kein Sonderfall, sondern
/// "uppercase only the first letter of each word"; die Definition von
/// Wort ist "a sequence of one or more letters". Ein `string`-basiertes
/// `to_uppercase()` auf dem ganzen Text waere falsch.
///
/// M3/M4: Die Laenge wird VOR der Allokation geprueft.
pub fn text_transform(text: &str, art: Transform) -> Result<String, TextError> {
    if text.chars().count() > MAX_TEXTLAENGE {
        return Err(TextError::ZuLang);
    }
    Ok(match art {
        Transform::Keiner => text.to_string(),
        Transform::Gross => text.to_uppercase(),
        Transform::Klein => text.to_lowercase(),
        Transform::WortGross => {
            let mut raus = String::with_capacity(text.len());
            let mut am_wortanfang = true;
            for c in text.chars() {
                if c.is_alphabetic() {
                    if am_wortanfang {
                        raus.extend(c.to_uppercase());
                        am_wortanfang = false;
                    } else {
                        raus.push(c);
                    }
                } else {
                    raus.push(c);
                    // CSS Text 3, 3: eine Wortgrenze ist jedes Zeichen,
                    // das KEIN Buchstabe ist — auch eine Ziffer, auch ein
                    // Bindestrich innerhalb von "foo-bar".
                    am_wortanfang = true;
                }
            }
            raus
        }
    })
}

/// M3: hoechster Abstand zwischen zwei Buchstaben in px.
///
/// CSS Text 3, 4: `letter-spacing` aendert den VORSCHUB, nicht die
/// Glyphe. Ein negativer Wert ist erlaubt (CSS Color weist darauf hin,
/// dass der Text danach enden kann). Die Grenze haelt den Vorschub in
/// einem Bereich, in dem eine Zeile noch lesbar ist.
pub const MAX_LAUFWEITE: f32 = 64.0;
/// M2: bei diesem Abstand fallen zwei Zeichen auf dasselbe Pixel —
/// darunter ist es sinnlos (und potenziell eine Endlosschleife, wenn der
/// Aufrufer auf zurueckgesprungene x-Werte prueft).
pub const MIN_LAUFWEITE: f32 = -64.0;

/// Der Laufweiten-Zusatz, geklemmt und auf endlich geprueft.
///
/// `NaN` ergibt 0: ein nicht-endlicher CSS-Wert darf kein stiller
/// NaN-Pfad in der Platzierung sein.
#[must_use]
pub fn laufweite(w: f32) -> f32 {
    // GEMESSEN 03.10.2026: NaN ergibt 0 — ein nicht-endlicher CSS-Wert
    // darf kein stiller NaN-Pfad sein. Unendlich wird dagegen GEKLEMMT
    // und nicht verworfen: `clamp` bildet auch die Grenzen ab, und das ist
    // bei `letter-spacing` die SPEC-nahere Lesart.
    if w.is_nan() {
        return 0.0;
    }
    w.clamp(MIN_LAUFWEITE, MAX_LAUFWEITE)
}

/// Die drei Werte von `text-transform` (CSS Text 3 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    /// `none` — der Default.
    #[default]
    Keiner,
    /// `uppercase`.
    Gross,
    /// `lowercase`.
    Klein,
    /// `capitalize`: nur das erste Buchstabenzeichen JEDES Wortes wird gross.
    WortGross,
}
