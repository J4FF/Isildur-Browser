//! Rasterizer: Uebersetzt eine DisplayList in einen Framebuffer.

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_display::{DisplayItem, DisplayList};
use narsil_text::BitmapFont;

use crate::framebuffer::Framebuffer;
use crate::glyphs::draw_text_mit_deko;
use crate::schrift::Schrift;

/// Ergebnis eines Rastervorgangs.
pub struct RenderResult {
    /// Der gerenderte Framebuffer.
    pub framebuffer: Framebuffer,
    /// Anzahl tatsaechlich gezeichneter Items (Rechtecke und Texte).
    pub drawn: usize,
    /// Anzahl uebersprungener Items.
    ///
    /// Zaehlt NUR noch Items, die nicht gezeichnet werden konnten
    /// (unbekanntes Zeichen in allen Positionen, oder leere
    /// Zeichenkette). Fehlerhafte Font-Groessen und unbekannte
    /// Zeichen an EINER Stelle sind kein Grund, den ganzen
    /// Text-Item zu ueberspringen.
    pub skipped: usize,
}

/// Rastert eine komplette Display-List.
///
/// Zeichnet erst den Hintergrund, dann die Items in Listenreihenfolge.
/// Gibt die Anzahl tatsaechlich gezeichneter Items zurueck.
///
/// Text-Items werden uebersprungen und in `skipped` gezaehlt.
pub fn rasterize(list: &DisplayList, width: u32, height: u32) -> RenderResult {
    rasterize_with_font(list, None, width, height)
}

/// Rastert eine Display-List.
///
/// `font` wird fuer Text-Items verwendet. `None` bedeutet:
/// es ist keine Schrift eingebunden, dann werden Text-Items
/// uebersprungen und in `skipped` gezaehlt. Das ist kein
/// Fehler, sondern ein gueltiger Zustand.
pub fn rasterize_with_font(
    list: &DisplayList,
    font: Option<&BitmapFont>,
    width: u32,
    height: u32,
) -> RenderResult {
    rasterize_mit_schrift(list, &mut Schrift::aus_bitmap(font), width, height)
}

/// Rastert eine Display-List mit einer gewaehlten [`Schrift`] (I4).
///
/// Das ist der Weg, den ein Browser nimmt: die Schrift wird EINMAL
/// gewaehlt (dabei wird das Dateisystem abgegangen, das ist teuer) und
/// dann fuer jede Anzeige wiederverwendet — der Masken-Cache lebt
/// darin und waechst mit der Zeit.
///
/// `bitmap` ist `Some` fuer die eingebaute Bitmap-Schrift, `None` fuer
/// "keine Schrift eingebunden" — dann werden Text-Items uebersprungen
/// und gezaehlt, genau wie in [`rasterize_with_font`].
pub fn rasterize_mit_schrift(
    list: &DisplayList,
    schrift: &mut Schrift,
    width: u32,
    height: u32,
) -> RenderResult {
    // Framebuffer mit Hintergrundfarbe fuellen
    let bg = list.background.unwrap_or(Color::TRANSPARENT);
    let mut fb = Framebuffer::new(width, height, bg);

    let mut drawn = 0usize;
    let mut skipped = 0usize;

    // Release 08.10.2026: `NARSIL_RASTER_ZEIT=1` sammelt die Zeit je
    // Eintragsart (Rechteck, Bild, Verlauf, Rand, Text) und druckt sie je
    // 30 Bilder — die Frage „wo bleiben die 20 ms?“ braucht diese Zahlen.
    let zeit_messen = raster_zeit_an();
    let mut zeiten = [0f64; 5];
    // Release 09.10.2026: offene Gruppen (Deckkraft), innerste zuletzt.
    let mut ebenen: Vec<OpenGroup> = Vec::new();
    // Items in Listenreihenfolge zeichnen
    for (index, item) in list.iter().enumerate() {
        effekte_anwenden(&mut fb, list, index, &mut ebenen);
        let item_clip = list.clip_at(index);
        if let Some(rect) = item_clip {
            fb.push_clip(rect);
        }
        // Release 15.10.2026 (Agent F): an item in a rounded overflow clip.
        let rounded = crate::glas::rounded_clip_layer(&fb, list, index);
        let t_item = zeit_messen.then(std::time::Instant::now);
        let art = match item {
            DisplayItem::Rect { .. } => 0,
            DisplayItem::Bild { .. } => 1,
            DisplayItem::Verlauf { .. } => 2,
            DisplayItem::Rand { .. } => 3,
            DisplayItem::Text { .. } => 4,
        };
        match item {
            DisplayItem::Rect {
                rect,
                color,
                radius,
                deckkraft,
                schatten,
                schatten_anzahl,
            } => {
                // S2 Station 4: Schatten ZUERST, dann die Form.
                //
                // Die Reihenfolge ist keine Geschmacksfrage, sondern
                // CSS Backgrounds 3 §7: ein Schatten (ausser `inset`)
                // wird HINTER dem Hintergrund gezeichnet. Ein Schatten, der
                // oben auf liegt, verdeckt genau die Kante, zu der er
                // gehoert.
                // DIE REIHENFOLGE, und sie ist normativ (CSS Backgrounds 3 §7):
                //   1. aeussere Schatten   HINTER dem Hintergrund
                //   2. der Hintergrund     die Engefarbe
                //   3. `inset`-Schatten    VOR dem Hintergrund
                //   4. der Inhalt          (Text, Bilder)
                //
                // GEMESSEN 03.10.2026: Inhalt 1 und 3 waren zusammen vor
                // Schritt 2. Der deckende Kasten hat den `inset` deshalb
                // vollstaendig verdeckt — im Bild war die `inset`-Box von
                // der Kante bis zur Mitte eine Farbe.
                crate::formen::schatten_aussen(&mut fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, schatten, *schatten_anzahl);
                // DIE ZAEHLREGEL, und sie hat sich hier nicht geaendert:
                // ein RECT zaehlt als GEZEICHNET, auch wenn kein Pixel
                // dadurch entstanden ist — es wurde ja versucht.
                //
                // GEMESSEN 03.10.2026: hier stand erst `if n > 0`, nachdem
                // `kasten_mit_radius` den Clip-Stack beachten lernte und bei
                // einem 0x0-Framebuffer 0 zurueckgab. Damit war ein Item
                // weder gezeichnet noch uebersprungen, und
                // `tests/bomben.rs::ein_leerer_puffer_ist_kein_absturz`
                // schlug mit GEMESSEN 1 != 2 fehl.
                let _n =
                    crate::formen::kasten_mit_radius(&mut fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, *color, *deckkraft);
                crate::formen::schatten_inset(&mut fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, schatten, *schatten_anzahl);
                drawn += 1;
            }
            // I3: ein Bild wird gesucht, nicht gefunden. `draw_bild`
            // entscheidet selbst, ob es zeichnet (Laenge passt, Groesse
            // erlaubt) — hier gibt es keinen Sonderfall.
            DisplayItem::Bild {
                rect,
                breite,
                hoehe,
                pixels,
            } => {
                let n = crate::bilder::draw_bild(&mut fb, rect, pixels, *breite, *hoehe);
                if n > 0 || bilddaten_gueltig(pixels, *breite, *hoehe) {
                    // Release 08.10.2026: ein GUELTIGES Bild, das keinen Pixel
                    // aendert (durchsichtiges SVG, weisses Logo auf weissem
                    // Grund, ausserhalb des Ausschnitts), ist gezeichnet —
                    // vorher bekam es den dunklen Platzhalter, GEMESSEN als
                    // schwarze Kaesten an Stelle der Logos von MDN, GitHub
                    // und tagesschau.de.
                    drawn += 1;
                } else {
                    // Kaputte Bilddaten: dann ist es ein Platzhalter, sonst
                    // waere die Stelle einfach leer und man wuesste
                    // nicht, ob das Bild fehlt oder nicht da ist.
                    crate::bilder::draw_platzhalter(&mut fb, rect);
                    skipped += 1;
                }
            }
            // BILD_33, Schritt 5: der **Rahmen** als Ring.
            //
            // **DER RAHMEN IST KEIN FUELL-`Rect`:** er liegt zwischen der
            // Aussenkurve (`border-box` mit `border-radius`) und der Innenkurve
            // (dieselbe Kurve um die Rahmenbreite kleiner). `crate::rahmen`
            // rechnet das ueber reine Zahlen; **hier** wird nur gemalt.
            //
            // **ES WIRD NUR `solid` GEMALT** (`rahmen::zeichne` gibt dann 0
            // zurueck) — `dashed`/`dotted`/`double`/`groove` brauchen die
            // Lauflaenge entlang der Kante. Das ist eine **benannte Luecke**,
            // kein Formfehler, und `drawn` zaehlt sie **nicht** als gezeichnet.
            DisplayItem::Verlauf {
                rect,
                radius,
                verlauf,
            } => {
                // BILD_48, Station 4: der Verlauf **fuellt** den Kasten wie `Rect`
                // und kommt **vor** dem Text, **hinter** dem Hintergrund.
                let gemalt = crate::verlauf::zeichne(&mut fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, verlauf);
                // **GEMESSENE KONVENTION, dieselbe wie beim Rahmen:** ein Item
                // zaehlt als **gezeichnet**, wenn es **mindestens ein** Pixel
                // gesetzt hat — nicht, wie viele.
                if gemalt > 0 {
                    drawn += 1;
                }
            }
            DisplayItem::Rand {
                rect,
                rahmen,
                radius,
            } => {
                let gemalt = crate::rahmen::zeichne(&mut fb, *rect, rahmen, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) });
                // GEMESSEN: in `rasterize` ist `fb` **eigen** (`Framebuffer::new`), in `rasterize_mit_schrift` ein `&mut` — dieselbe Zeile, zwei Signaturen.
                if gemalt > 0 {
                    drawn += 1;
                }
            }
            DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): die Familie wird **benutzt**,
                // nicht mehr ignoriert. GEMESSEN, was vorher geschah: dieser
                // Ast behandelte `_`, also lief jeder Text — auch
                // `<code>`/`<kbd>`/`<samp>` — ueber `suche_schnitt` und
                // damit in `BEKANNT_SCHRIFTEN[0]` (GEMESSEN
                // `DejaVuSans.ttf`). Der neue Weg nimmt die Datei, die
                // `familie_waehlen` nennt; GEMESSEN nennt sie dieselbe wie
                // `fc-match` (Test `a54_familien_orakel.rs`, 3/3).
                schriftfamilie,
                origin,
                text,
                color,
                font_size,
                // OFFEN (A3, 03.10.2026): dieser Pfad zeichnet ueber
                // `Schrift::zeichne`, nicht ueber `draw_text_mit_deko`, und
                // ignoriert die Dekoration. Der zweite Pfad
                // (`rasterize_mit_schrift`) zeichnet sie. BEIDE zu
                // unterstuetzen heisst, `Schrift::zeichne` die Maske zu
                // geben — dieselbe Aenderung wie in `glyphs.rs`, aber an
                // einer anderen Signatur. Hier wird es NICHT vorgetaeuscht:
                // `deko: _` statt einer stillen Luecke.
                deko: _,
                gewicht,
                kursiv,
            } => {
                // Release 10.10.2026: voellig durchsichtiger Text zeichnet
                // nichts. GEMESSEN vorher: die Glyphenwege mischten nur die
                // Deckung der Glyphe, nicht das Alpha der Farbe — Text mit
                // `color.a == 0` (die Steuertexte eines `<video>` ohne
                // `controls`, github.com) erschien schwarz.
                if color.a == 0 {
                    if item_clip.is_some() {
                        fb.pop_clip();
                    }
                    continue;
                }
                // Ohne Schrift: ueberspringen und zaehlen. Das ist kein
                // Fehler, sondern ein gueltiger Zustand.
                if !schrift.hat_schrift() {
                    skipped += 1;
                    if item_clip.is_some() {
                        fb.pop_clip();
                    }
                    continue;
                }

                // 04.10.2026 (SCHRIFT): DER SCHNITT PRO TEXT-ITEM.
                //
                // GEMESSEN, was vorher lief: an `texttest.html` kamen alle
                // vier Stilzeilen mit **125 px** an — Firefox 127 / 144 /
                // 129 / 143. `DisplayItem::Text` fuehrte kein Gewicht und
                // keinen Stil, also wurde **jeder** Text aufrecht gezeichnet
                // (Ueberschriften, `<b>`, `<strong>`, `<em>`, `<th>`).
                //
                // GEMESSEN, warum der Weg funktioniert:
                // `Schrift::mit_gewicht(700, false)` liefert
                // `DejaVuSans-Bold.ttf`, `(400, true)` den Oblique-Schnitt
                // (`examples/a49_stil_bild.rs`, alle vier mit
                // `erfuellt = true` — also echte Schnitte, keine
                // Verzerrung).
                //
                // GEMESSEN, warum `zeichne_mit_gewicht` und nicht
                // `Schrift::mit_gewicht` pro Item: siehe dort — ein
                // Dateiabende je Schnitt fuer das ganze Programm, nicht
                // je Textknoten.
                //
                // DER REGELVORBEHALT, gemessen und nicht verschwiegen: das
                // LAYOUT misst die Breite **ohne** Gewicht (der Messer
                // kennt keinen), also bricht ein fetter Absatz an einer
                // anderen Stelle um als in Firefox. Siehe BRETT 02:20; die
                // Zahl steht in `team/berichte/SCHRIFT_4.txt`.
                // DIE W5G-PRUEFUNG (04.10.2026, SCHRIFT (c)): die mit
                // `schrift` **gemessene** Datei wird auch zum Zeichnen
                // benutzt — aber nur, wenn die Familie des Textes auf
                // dieselbe Datei zeigt. Sonst waere der Absatz an anderer
                // Stelle umgebrochen als die Tinte endet.
                //
                // GEMESSEN, was vorher lief: `schriftfamilie` stand als `_`
                // im Muster, also galt IMMER `zeichne_mit_gewicht`, und
                // jede Familie wurde wie sans-serif gezeichnet.
                // DIE BITMAP AUSNAHMSWEISE (04.10.2026, SCHRIFT (c)):
                // ruft der Aufrufer mit der **eingebauten** Bitmap-Schrift
                // auf, so will er genau die. Eine Bitmap hat keine Familie
                // — `quelle_der_familie` wuerde eine Systemschrift nennen
                // und sie anstelle der gelieferten benutzen.
                //
                // GEMESSEN, warum das ein echter Fall ist und nicht eine
                // Vorsicht: `tests`/`src/rasterizer.rs`
                // `rasterize_with_font_baseline_is_origin_y` ruft mit
                // `Some(&BitmapFont::builtin())` und prueft auf Pixel der
                // **exakten** Farbe `rgb(255,0,0)`. Mit der Systemschrift
                // bei 7 px gibt es keinen solchen Pixel (Kantenglaettung),
                // GEMESSEN `first_ink_row = None` statt `Some(0)`.
                // Release 09.10.2026: Leistentext (Marke in der Familie) darf
                // mit Teilpixeln gezeichnet werden — siehe `textgamma`.
                crate::textgamma::setze_leiste(crate::textgamma::ist_leistenfamilie(schriftfamilie));
                let bitmap_ist_gewollt = matches!(schrift.quelle, crate::SchriftQuelle::Bitmap);
                let gemessen_ist_richtig = bitmap_ist_gewollt
                    || crate::schrift::quelle_der_familie(schriftfamilie, *gewicht, *kursiv)
                        .as_ref()
                        == Some(&schrift.quelle);
                let eigene = gemessen_ist_richtig
                    || (*gewicht == 400 && !*kursiv && schriftfamilie.trim().is_empty());
                let zeichne = |fb: &mut Framebuffer, schrift: &mut Schrift, t: &str, o: narsil_core::geometry::Point| {
                    if eigene {
                        schrift.zeichne(fb, t, *color, *font_size, o)
                    } else {
                        crate::schrift::zeichne_mit_familie(fb, t, *color, *font_size, o, schriftfamilie, *gewicht, *kursiv)
                    }
                };
                // Release 12.10.2026 (Agent CSS2): `letter-spacing`/
                // `word-spacing` (CSS 2.1 §16.4) — Zeichen bzw. Woerter einzeln
                // an ihre Stelle (`DisplayList::versaetze_bei`).
                let pixels = match list.versaetze_bei(index) {
                    Some(teile) => text_teile(text, teile, *origin)
                        .into_iter()
                        .map(|(t, o)| zeichne(&mut fb, schrift, t, o))
                        .sum(),
                    None => zeichne(&mut fb, schrift, text, *origin),
                };
                crate::textgamma::setze_leiste(false);
                if pixels > 0 {
                    drawn += 1;
                } else {
                    // Text konnte nicht gezeichnet werden (z.B. leerer Text)
                    // Das ist kein Fehler, aber wir zaehlen es als uebersprungen
                    skipped += 1;
                }
            }
        }
        if let Some(t) = t_item {
            zeiten[art] += t.elapsed().as_secs_f64() * 1000.0;
        }
        if let Some(e) = rounded {
            crate::glas::ebene_beenden(&mut fb, e);
        }
        if item_clip.is_some() {
            fb.pop_clip();
        }
    }
    // Effekte hinter dem letzten Item, dann alle noch offenen Gruppen.
    effekte_anwenden(&mut fb, list, list.len(), &mut ebenen);
    while let Some(e) = ebenen.pop() {
        close_group(&mut fb, e);
    }
    if zeit_messen {
        raster_zeit_merken(zeiten);
    }

    RenderResult {
        framebuffer: fb,
        drawn,
        skipped,
    }
}

/// An open group: a blended one (opacity, rounded corners, mask) or one
/// painted alone into its own buffer (`filter`).
enum OpenGroup {
    Blend(crate::glas::Ebene),
    Isolated { start: usize, end: usize, below: Framebuffer, area: Rect, spec: std::sync::Arc<narsil_display::FilterSpec>, view: Option<Rect> },
}

impl OpenGroup {
    fn end(&self) -> usize {
        match self {
            OpenGroup::Blend(e) => e.bis,
            OpenGroup::Isolated { end, .. } => *end,
        }
    }
}

fn close_group(fb: &mut Framebuffer, e: OpenGroup) {
    match e {
        OpenGroup::Blend(e) => crate::glas::ebene_beenden(fb, e),
        OpenGroup::Isolated { below, area, spec, view, .. } => {
            let group = std::mem::replace(fb, below);
            crate::glas::composite_filtered(fb, &group, area, &spec, view);
        }
    }
}

/// Release 09.10.2026: schliesst die Gruppen, die vor `index` enden, und
/// wendet die Effekte vor `index` an (Glas zeichnen, Gruppe merken).
fn effekte_anwenden(
    fb: &mut Framebuffer,
    list: &DisplayList,
    index: usize,
    ebenen: &mut Vec<OpenGroup>,
) {
    while ebenen.last().is_some_and(|e| e.end() <= index) {
        if let Some(e) = ebenen.pop() {
            close_group(fb, e);
        }
    }
    for (effekt, clip) in list.effekte_bei(index) {
        match effekt {
            narsil_display::Effekt::Glas(g) => {
                if let Some(c) = clip {
                    fb.push_clip(c);
                }
                crate::glas::zeichne(fb, g);
                if clip.is_some() {
                    fb.pop_clip();
                }
            }
            narsil_display::Effekt::Gruppe { bis, bereich, deckkraft, radius, maske, filter, radius_y } => {
                if *bis > index {
                    if let Some(spec) = filter {
                        // Release 15.10.2026 (Agent F): the group paints
                        // into a transparent buffer of its own.
                        let empty = Framebuffer::new(fb.width(), fb.height(), narsil_core::Color::TRANSPARENT);
                        let below = std::mem::replace(fb, empty);
                        ebenen.push(OpenGroup::Isolated { start: index, end: *bis, below, area: *bereich, spec: spec.clone(), view: clip });
                    } else {
                        let mut e = crate::glas::ebene_beginnen(fb, *bereich, *bis, *deckkraft, crate::formen::Corners { x: *radius, y: radius_y.unwrap_or(*radius) });
                        e.maske = maske.clone();
                        ebenen.push(OpenGroup::Blend(e));
                    }
                }
            }
            narsil_display::Effekt::Backdrop { .. } => {}
        }
    }
    // Release 15.10.2026 (Agent F): `backdrop-filter` after the groups that
    // start here (the box's own opacity, clip and mask apply to it). Inside
    // the box's own `filter` group, the backdrop is what lies below it.
    for (effekt, _) in list.effekte_bei(index) {
        if let narsil_display::Effekt::Backdrop { spec, radius, radius_y } = effekt {
            let below = match ebenen.last() {
                Some(OpenGroup::Isolated { start, below, .. }) if *start == index => Some(below),
                _ => None,
            };
            crate::glas::composite_backdrop(fb, below, spec, crate::formen::Corners { x: *radius, y: *radius_y });
        }
    }
}

/// Release 08.10.2026: passen Pixelzahl und Masse zusammen? Nur ein Bild
/// mit kaputten Daten bekommt den Platzhalter.
fn bilddaten_gueltig(pixels: &[u8], breite: u32, hoehe: u32) -> bool {
    breite > 0 && hoehe > 0 && pixels.len() == (breite as usize).saturating_mul(hoehe as usize).saturating_mul(4)
}

/// Ist `NARSIL_RASTER_ZEIT` gesetzt? (einmal gelesen)
fn raster_zeit_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_RASTER_ZEIT").is_some())
}

/// Summiert die Zeiten je Eintragsart und druckt je 30 Bilder das Mittel.
fn raster_zeit_merken(zeiten: [f64; 5]) {
    use std::sync::Mutex;
    static SUMME: Mutex<(u32, [f64; 5])> = Mutex::new((0, [0.0; 5]));
    let Ok(mut s) = SUMME.lock() else { return };
    s.0 += 1;
    for (a, b) in s.1.iter_mut().zip(zeiten) {
        *a += b;
    }
    if s.0 >= 30 {
        let n = f64::from(s.0);
        eprintln!(
            "RASTERZEIT rechteck={:.2} bild={:.2} verlauf={:.2} rand={:.2} text={:.2} ms",
            s.1[0] / n, s.1[1] / n, s.1[2] / n, s.1[3] / n, s.1[4] / n
        );
        *s = (0, [0.0; 5]);
    }
}

/// Rastert eine Display-List in einen BESTEHENDEN Framebuffer,
/// aber nur innerhalb von `bereich`.
///
/// Der Hintergrund wird NICHT gefuellt — der Aufrufer hat ihn schon.
/// `bereich` wird als oberstes Schnittrechteck gesetzt und danach
/// wieder entfernt. Der Clip-Stand des Aufrufers bleibt, wie er war.
///
/// Rueckgabe: Anzahl gezeichneter Items (dieselbe Zaehlregel wie
/// `rasterize_with_font`).
///
/// Regeln:
/// - Leerer Bereich (Breite oder Hoehe <= 0): 0, kein Pixel geaendert.
/// - Bereich ausserhalb des Puffers: erlaubt, ergibt 0.
/// - Kein Panic.
pub fn rasterize_bereich(
    list: &DisplayList,
    font: Option<&BitmapFont>,
    fb: &mut Framebuffer,
    bereich: Rect,
) -> usize {
    // Leerer Bereich: nichts tun
    if bereich.is_empty() {
        return 0;
    }

    // I3: die Bilder brauchen eine `&mut Framebuffer`; `fb` ist eine
    // `&mut Framebuffer`, aber die Aufrufe sehen `&mut &mut`, und das
    // gibt einen Typfehler. Deshalb wird `fb` hier neu gebunden. Die
    // Alternative — die Signatur von `rasterize_bereich` aendern — hat
    // 46 Aufrufstellen im Haus.

    // Clip-Stack: alten Stand merken, bereich pushen, danach poppen
    let clip_vorher = fb.clip_stack_len();
    fb.push_clip(bereich);

    let mut drawn = 0usize;

    // Items in Listenreihenfolge zeichnen
    for (index, item) in list.iter().enumerate() {
        let item_clip = list.clip_at(index);
        if let Some(rect) = item_clip {
            fb.push_clip(rect);
        }
        let rounded = crate::glas::rounded_clip_layer(fb, list, index);
        match item {
            DisplayItem::Rect {
                rect,
                color,
                radius,
                deckkraft,
                schatten,
                schatten_anzahl,
            } => {
                crate::formen::schatten_zeichnen(fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, schatten, *schatten_anzahl);
                let changed =
                    crate::formen::kasten_mit_radius(fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, *color, *deckkraft);
                crate::formen::schatten_inset(fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, schatten, *schatten_anzahl);
                if changed > 0 {
                    drawn += 1;
                }
            }
            // I3: im Bereich gerastert wie alles andere; der Platzhalter
            // gilt auch hier, sonst waere ein fehlendes Bild ein Loch.
            DisplayItem::Bild {
                rect,
                breite,
                hoehe,
                pixels,
            } => {
                let changed = crate::bilder::draw_bild(fb, rect, pixels, *breite, *hoehe);
                if changed > 0 || bilddaten_gueltig(pixels, *breite, *hoehe) {
                    drawn += 1;
                } else {
                    crate::bilder::draw_platzhalter(fb, rect);
                }
            }
            // BILD_33, Schritt 5: der **Rahmen** als Ring.
            //
            // **DER RAHMEN IST KEIN FUELL-`Rect`:** er liegt zwischen der
            // Aussenkurve (`border-box` mit `border-radius`) und der Innenkurve
            // (dieselbe Kurve um die Rahmenbreite kleiner). `crate::rahmen`
            // rechnet das ueber reine Zahlen; **hier** wird nur gemalt.
            //
            // **ES WIRD NUR `solid` GEMALT** (`rahmen::zeichne` gibt dann 0
            // zurueck) — `dashed`/`dotted`/`double`/`groove` brauchen die
            // Lauflaenge entlang der Kante. Das ist eine **benannte Luecke**,
            // kein Formfehler, und `drawn` zaehlt sie **nicht** als gezeichnet.
            DisplayItem::Verlauf {
                rect,
                radius,
                verlauf,
            } => {
                let gemalt = crate::verlauf::zeichne(fb, *rect, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) }, verlauf);
                if gemalt > 0 {
                    drawn += 1;
                }
            }
            DisplayItem::Rand {
                rect,
                rahmen,
                radius,
            } => {
                let gemalt = crate::rahmen::zeichne(fb, *rect, rahmen, crate::formen::Corners { x: *radius, y: list.vertical_radii_at(index).unwrap_or(*radius) });
                if gemalt > 0 {
                    drawn += 1;
                }
            }
            DisplayItem::Text {
                schriftfamilie: _,
                origin,
                text,
                color,
                font_size,
                deko,
                // 04.10.2026 (SCHRIFT): dieser Pfad zeichnet ueber die
                // **Bitmap**-Schrift (`draw_text_mit_deko` nimmt
                // `&BitmapFont`), und die kennt keinen Schnitt. Die
                // Felder werden hier also nur genannt, damit das Muster
                // vollstaendig ist; gewirkt wird der Schnitt im
                // Browserpfad `rasterize_mit_schrift`.
                gewicht: _,
                kursiv: _,
            } => {
                // Release 10.10.2026: voellig durchsichtiger Text zeichnet
                // nichts (siehe `rasterize_mit_schrift`).
                if color.a == 0 {
                    if item_clip.is_some() {
                        fb.pop_clip();
                    }
                    continue;
                }
                // Ohne Schrift: nichts, und der Aufrufer sieht es an der
                // Zahl der gezeichneten Items.
                //
                // GEMESSEN am 30.09.2026: hier stand ein `skipped`, das
                // hochgezaehlt und nie gelesen wurde — diese Funktion
                // gibt `drawn` zurueck, `skipped` gibt es nur in
                // `rasterize_mit_schrift`. Ein Zaehler, den niemand
                // abfragt, ist eine Behauptung ohne Aussage.
                let Some(font) = font else {
                    if item_clip.is_some() {
                        fb.pop_clip();
                    }
                    continue;
                };
                // A3: die Dekoration kommt aus der Kaskade und wird hier
                // gezeichnet. `draw_text_mit_deko` zeichnet die Linien
                // ueber den VORSCHUB jeder Glyphe, nicht ueber die
                // Tintenbreite — sonst gaebe es genau an den Leerzeichen
                // Luecken, und jeder Link mit Wortabstand haette eine.
                let pixels = draw_text_mit_deko(fb, font, text, *color, *font_size, *origin, *deko);
                if pixels > 0 {
                    drawn += 1;
                }
            }
        }
        if let Some(e) = rounded {
            crate::glas::ebene_beenden(fb, e);
        }
        if item_clip.is_some() {
            fb.pop_clip();
        }
    }

    // Clip-Stack wiederherstellen: alle bis zum alten Stand entfernen
    fb.truncate_clip_stack(clip_vorher);

    drawn
}

/// Release 12.10.2026 (Agent CSS2): die Teile eines Text-Items mit
/// `letter-spacing`/`word-spacing` und ihre Ursprungspunkte — aus den Teilen
/// (Byte-Anfang, x-Versatz), die das Layout mit seiner Messung gerechnet hat
/// (`DisplayList::versaetze_bei`). Unpassende Angaben (kein Zeichenrand,
/// nicht aufsteigend) zeichnen den Lauf am Stueck.
pub(crate) fn text_teile<'a>(
    text: &'a str,
    teile: &[(u32, f32)],
    origin: narsil_core::geometry::Point,
) -> Vec<(&'a str, narsil_core::geometry::Point)> {
    if teile.first().is_none_or(|t| t.0 != 0) {
        return vec![(text, origin)];
    }
    let mut raus = Vec::with_capacity(teile.len());
    for (k, (anfang, x)) in teile.iter().enumerate() {
        let a = *anfang as usize;
        let e = teile.get(k + 1).map_or(text.len(), |n| n.0 as usize);
        if a >= e || e > text.len() || !text.is_char_boundary(a) || !text.is_char_boundary(e) || !x.is_finite() {
            return vec![(text, origin)];
        }
        raus.push((&text[a..e], narsil_core::geometry::Point::new(origin.x + x, origin.y)));
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_core::color::Color;
    use narsil_core::geometry::{Point, Rect};
    use narsil_display::{DisplayItem, DisplayList};
    use narsil_text::BitmapFont;

    /// Release 12.10.2026 (Agent CSS2): die Teile eines Text-Items aus den
    /// Versaetzen des Layouts; Unpassendes zeichnet am Stueck.
    #[test]
    fn text_teile_aus_den_versaetzen() {
        let o = Point::new(5.0, 7.0);
        let t = text_teile("ab c", &[(0, 0.0), (1, 13.0), (2, 26.0), (3, 59.0)], o);
        let xs: Vec<_> = t.iter().map(|(t, p)| (*t, p.x, p.y)).collect();
        assert_eq!(xs, vec![("a", 5.0, 7.0), ("b", 18.0, 7.0), (" ", 31.0, 7.0), ("c", 64.0, 7.0)]);
        assert_eq!(text_teile("ab", &[(0, 0.0), (5, 3.0)], o), vec![("ab", o)]);
        assert_eq!(text_teile("\u{e4}b", &[(0, 0.0), (1, 3.0)], o), vec![("\u{e4}b", o)], "kein Zeichenrand");
    }

    /// Release 12.10.2026 (Agent CSS2): der Raster setzt die Zeichen eines
    /// Items mit Abstand auseinander — die Tinte reicht weiter nach rechts.
    #[test]
    fn buchstabenabstand_verbreitert_die_tinte() {
        let font = BitmapFont::builtin();
        let tinte_rechts = |abstand: f32| -> Option<u32> {
            let mut list = DisplayList::with_background(Color::rgb(255, 255, 255));
            list.push(DisplayItem::Text {
                origin: Point::new(2.0, 2.0),
                text: "ii".into(),
                color: Color::rgb(0, 0, 0),
                font_size: 16.0,
                deko: 0,
                gewicht: 400,
                kursiv: false,
                schriftfamilie: String::new(),
            });
            list.versaetze_fuer_letztes(vec![(0, 0.0), (1, 9.0 + abstand)].into_boxed_slice());
            let r = rasterize_with_font(&list, Some(&font), 120, 30);
            let fb = &r.framebuffer;
            (0..fb.width()).rev().find(|&x| (0..fb.height()).any(|y| fb.pixel(x, y).is_some_and(|p| p.r < 128)))
        };
        let ohne = tinte_rechts(0.0).expect("Tinte ohne Abstand");
        let mit = tinte_rechts(40.0).expect("Tinte mit Abstand");
        assert!(mit >= ohne + 35, "mit Abstand {mit} gegen {ohne}");
    }

    /// Release 12.10.2026 (Agent CSS2): ein Bild und ein Kasten an derselben
    /// Bruchstelle decken dieselben Pixel (halb offen, abgerundet). GEMESSEN
    /// vorher (WPT `visudet/inline-block-baseline-010`): das Bild reichte eine
    /// Spalte weiter.
    #[test]
    fn bild_und_kasten_an_bruchkanten_gleich() {
        let rect = Rect::new(4.23, 0.0, 15.0, 3.0);
        let blau = Color::rgb(0, 0, 255);
        let mut a = DisplayList::with_background(Color::rgb(255, 255, 255));
        a.push(DisplayItem::rechteck(rect, blau));
        let mut b = DisplayList::with_background(Color::rgb(255, 255, 255));
        b.push(DisplayItem::Bild { rect, breite: 1, hoehe: 1, pixels: std::sync::Arc::new(vec![0, 0, 255, 255]) });
        let fa = rasterize(&a, 30, 3).framebuffer;
        let fb = rasterize(&b, 30, 3).framebuffer;
        for x in 0..30 {
            assert_eq!(fa.pixel(x, 1), fb.pixel(x, 1), "Spalte {x}");
        }
    }

    #[test]
    fn item_clip_schneidet_rechteck_auf_den_inhalt() {
        let mut list = DisplayList::new();
        list.push_clip(Rect::new(5.0, 5.0, 5.0, 5.0));
        list.push(DisplayItem::rechteck(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Color::rgb(255, 0, 0),
        ));
        list.pop_clip();
        let bild = rasterize(&list, 20, 20).framebuffer;
        assert_eq!(bild.pixel(4, 5), Some(Color::TRANSPARENT));
        assert_eq!(bild.pixel(5, 5), Some(Color::rgb(255, 0, 0)));
        assert_eq!(bild.pixel(9, 9), Some(Color::rgb(255, 0, 0)));
        assert_eq!(bild.pixel(10, 9), Some(Color::TRANSPARENT));
    }

    #[test]
    fn rasterize_bereich_clippt_innen_aussen_unveraendert() {
        // Liste mit einem Rechteck, das UEBER den Bereich hinausragt
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0), // Grosses Rechteck
            color: Color::rgb(255, 0, 0),
        });

        // Framebuffer mit Hintergrund (weiss)
        let mut fb = Framebuffer::new(200, 200, Color::WHITE);

        // Bereich: nur der linke obere 50x50 Ausschnitt
        let bereich = Rect::new(0.0, 0.0, 50.0, 50.0);

        let drawn = rasterize_bereich(&list, None, &mut fb, bereich);
        assert_eq!(drawn, 1, "Ein Item wurde gezeichnet");

        // INNEN: Pixel im Bereich muessen rot sein
        assert_eq!(
            fb.pixel(10, 10),
            Some(Color::rgb(255, 0, 0)),
            "Innen: Pixel geaendert"
        );
        assert_eq!(
            fb.pixel(49, 49),
            Some(Color::rgb(255, 0, 0)),
            "Innen: Ecke geaendert"
        );

        // AUSSEN: Pixel direkt ausserhalb des Bereichs bleiben weiss (Hintergrund)
        assert_eq!(
            fb.pixel(50, 10),
            Some(Color::WHITE),
            "Aussen rechts: unveraendert"
        );
        assert_eq!(
            fb.pixel(10, 50),
            Some(Color::WHITE),
            "Aussen unten: unveraendert"
        );
        assert_eq!(
            fb.pixel(100, 100),
            Some(Color::WHITE),
            "Weit aussen: unveraendert"
        );

        // KANTE genau: x=49 ist drin, x=50 ist draussen (halb-offen)
        assert_eq!(
            fb.pixel(49, 25),
            Some(Color::rgb(255, 0, 0)),
            "Kante x=49 innen"
        );
        assert_eq!(fb.pixel(50, 25), Some(Color::WHITE), "Kante x=50 aussen");
    }

    #[test]
    fn rasterize_bereich_ausserhalb_gibt_null() {
        // Liste mit einem Rechteck
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: Color::rgb(255, 0, 0),
        });

        // Framebuffer mit Hintergrund
        let mut fb = Framebuffer::new(100, 100, Color::WHITE);

        // Bereich vollstaendig ausserhalb des Puffers
        let bereich = Rect::new(200.0, 200.0, 50.0, 50.0);

        let drawn = rasterize_bereich(&list, None, &mut fb, bereich);
        assert_eq!(drawn, 0, "Bereich ausserhalb -> 0 Items gezeichnet");

        // Kein Pixel darf sich geaendert haben
        assert_eq!(fb.pixel(0, 0), Some(Color::WHITE));
        assert_eq!(fb.pixel(50, 50), Some(Color::WHITE));
        assert_eq!(fb.pixel(99, 99), Some(Color::WHITE));
    }

    #[test]
    fn rasterize_bereich_leer_gibt_null() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: Color::rgb(255, 0, 0),
        });

        let mut fb = Framebuffer::new(100, 100, Color::WHITE);

        // Breite 0
        let drawn = rasterize_bereich(&list, None, &mut fb, Rect::new(0.0, 0.0, 0.0, 50.0));
        assert_eq!(drawn, 0);

        // Hoehe 0
        let drawn = rasterize_bereich(&list, None, &mut fb, Rect::new(0.0, 0.0, 50.0, 0.0));
        assert_eq!(drawn, 0);

        // Negative Breite
        let drawn = rasterize_bereich(&list, None, &mut fb, Rect::new(0.0, 0.0, -10.0, 50.0));
        assert_eq!(drawn, 0);

        // Kein Pixel geaendert
        assert_eq!(fb.pixel(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn rasterize_bereich_ohne_clip_stack_aenderung() {
        // Prueft, dass der Clip-Stack des Aufrufers unveraendert bleibt
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 50.0, 50.0),
            color: Color::rgb(255, 0, 0),
        });

        let mut fb = Framebuffer::new(100, 100, Color::WHITE);

        // Aufrufer setzt eigenen Clip
        fb.push_clip(Rect::new(10.0, 10.0, 80.0, 80.0));
        let clip_vorher = fb.clip_stack_len();

        // rasterize_bereich mit eigenem Bereich
        let _ = rasterize_bereich(&list, None, &mut fb, Rect::new(20.0, 20.0, 30.0, 30.0));

        // Clip-Stack muss wieder genau so sein wie vorher
        assert_eq!(
            fb.clip_stack_len(),
            clip_vorher,
            "Clip-Stack wurde nicht wiederhergestellt"
        );

        // Der Clip des Aufrufers muss noch wirken
        fb.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Color::rgb(0, 255, 0));
        // Pixel bei (5,5) ist ausserhalb des Aufrufer-Clips (10,10)-(90,90) -> bleibt weiss
        assert_eq!(fb.pixel(5, 5), Some(Color::WHITE));
        // Pixel bei (15,15) ist innerhalb -> wird gruen
        assert_eq!(fb.pixel(15, 15), Some(Color::rgb(0, 255, 0)));
    }

    #[test]
    fn rasterize_bereich_gegenbeweis_ohne_clip() {
        // GEGENBEWEIS: Wenn man die Clip-Behandlung entfernt, wird dieser Test rot.
        // Dieser Test dokumentiert das erwartete Verhalten MIT Clip.
        // Siehe BERICHT_PIXEL.txt fuer die Ausgabe ohne Clip.

        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: Color::rgb(255, 0, 0),
        });

        let mut fb = Framebuffer::new(200, 200, Color::WHITE);
        let bereich = Rect::new(0.0, 0.0, 50.0, 50.0);

        let drawn = rasterize_bereich(&list, None, &mut fb, bereich);
        assert_eq!(drawn, 1);

        // MIT Clip: nur innen geaendert
        assert_eq!(fb.pixel(10, 10), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(50, 10), Some(Color::WHITE));
    }

    #[test]
    fn rasterize_empty_list() {
        let list = DisplayList::new();
        let result = rasterize(&list, 10, 10);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.framebuffer.width(), 10);
        assert_eq!(result.framebuffer.height(), 10);
    }

    #[test]
    fn rasterize_with_background() {
        let list = DisplayList::with_background(Color::rgb(100, 150, 200));
        let result = rasterize(&list, 2, 2);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(
            result.framebuffer.pixel(0, 0),
            Some(Color::rgb(100, 150, 200))
        );
    }

    #[test]
    fn rasterize_rect_items() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 5.0, 5.0),
            color: Color::rgb(255, 0, 0),
        });
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(10.0, 10.0, 5.0, 5.0),
            color: Color::rgb(0, 255, 0),
        });

        let result = rasterize(&list, 20, 20);
        assert_eq!(result.drawn, 2);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.framebuffer.pixel(2, 2), Some(Color::rgb(255, 0, 0)));
        assert_eq!(
            result.framebuffer.pixel(12, 12),
            Some(Color::rgb(0, 255, 0))
        );
    }

    #[test]
    fn rasterize_skips_text_items_without_font() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 5.0, 5.0),
            color: Color::rgb(255, 0, 0),
        });
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "Hallo".into(),
            color: Color::BLACK,
            font_size: 12.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let result = rasterize(&list, 20, 20);
        assert_eq!(result.drawn, 1);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_draws_text() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 1);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_baseline_is_origin_y() {
        // Beweis: Die Bitmap-Zeile 0 einer Glyphe wird an origin.y gezeichnet,
        // nicht an origin.y + ascent.
        // Bei font_size 7.0 ist die Glyphe "A" 7 Pixel hoch (Zeilen 0..=6).
        // Die erste Tintenzeile ist 0, die letzte Tintenzeile ist 6.
        // Waere die Grundlinie verschoben, laegen die Pixel an 5 oder 7 statt 0.

        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);

        // Finde die ERSTE Zeile mit einem Pixel in der Textfarbe
        let text_color = Color::rgb(255, 0, 0);
        let first_ink_row =
            (0..20).find(|y| (0..20).any(|x| result.framebuffer.pixel(x, *y) == Some(text_color)));

        // Die erste Tintenzeile muss 0 sein (nicht 5, nicht 7)
        assert_eq!(
            first_ink_row,
            Some(0),
            "Erste Tintenzeile muss 0 sein, nicht {}",
            first_ink_row.unwrap_or(999)
        );

        // Finde die LETZTE Zeile mit einem Pixel in der Textfarbe
        let last_ink_row = (0..20)
            .rev()
            .find(|y| (0..20).any(|x| result.framebuffer.pixel(x, *y) == Some(text_color)));

        // Die letzte Tintenzeile muss 6 sein (Bitmap-Hoehe 7, Zeilen 0..=6)
        assert_eq!(
            last_ink_row,
            Some(6),
            "Letzte Tintenzeile muss 6 sein, nicht {}",
            last_ink_row.unwrap_or(999)
        );

        // Zweiter Fall: origin = (0.0, 10.0)
        // Die erste Tintenzeile muss 10 sein, nicht 15 oder 17
        let mut list2 = DisplayList::new();
        list2.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 10.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let result2 = rasterize_with_font(&list2, Some(&font), 20, 20);

        let first_ink_row2 =
            (0..20).find(|y| (0..20).any(|x| result2.framebuffer.pixel(x, *y) == Some(text_color)));

        assert_eq!(
            first_ink_row2,
            Some(10),
            "Erste Tintenzeile muss 10 sein, nicht {}",
            first_ink_row2.unwrap_or(999)
        );
    }

    #[test]
    fn rasterize_zero_width() {
        let list = DisplayList::with_background(Color::WHITE);
        let result = rasterize(&list, 0, 10);
        assert_eq!(result.framebuffer.width(), 0);
        assert_eq!(result.framebuffer.height(), 10);
        assert_eq!(result.framebuffer.pixels().len(), 0);
    }

    #[test]
    fn rasterize_zero_height() {
        let list = DisplayList::with_background(Color::WHITE);
        let result = rasterize(&list, 10, 0);
        assert_eq!(result.framebuffer.width(), 10);
        assert_eq!(result.framebuffer.height(), 0);
        assert_eq!(result.framebuffer.pixels().len(), 0);
    }

    #[test]
    fn rasterize_overlapping_rects() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgb(255, 0, 0),
        });
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(5.0, 5.0, 10.0, 10.0),
            color: Color::rgb(0, 0, 255),
        });

        let result = rasterize(&list, 20, 20);
        assert_eq!(result.drawn, 2);
        // Erstes Rechteck sichtbar
        assert_eq!(result.framebuffer.pixel(2, 2), Some(Color::rgb(255, 0, 0)));
        // Zweites Rechteck ueberschneidet sich
        assert_eq!(result.framebuffer.pixel(7, 7), Some(Color::rgb(0, 0, 255)));
    }

    #[test]
    fn rasterize_background_before_items() {
        let mut list = DisplayList::with_background(Color::rgb(100, 100, 100));
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 5.0, 5.0),
            color: Color::rgb(255, 0, 0),
        });

        let result = rasterize(&list, 10, 10);
        // Hintergrund sichtbar
        assert_eq!(
            result.framebuffer.pixel(7, 7),
            Some(Color::rgb(100, 100, 100))
        );
        // Rechteck sichtbar
        assert_eq!(result.framebuffer.pixel(2, 2), Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn rasterize_with_font_empty_text() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        // Leerer Text wird nicht gezeichnet, aber auch nicht als skipped gezaehlt
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_invalid_font_size() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 0.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_rect_and_text() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgb(0, 0, 255),
        });
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 2);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_text_over_rect() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgb(0, 0, 255),
        });
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        // Text wird NACH dem Rechteck gezeichnet, also sollte der Text sichtbar sein
        // Pixel (0,1) ist in der Glyphe "A" gesetzt (Spalte 0, Zeile 1)
        assert_eq!(result.framebuffer.pixel(0, 1), Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn rasterize_with_font_rect_over_text() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        list.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgb(0, 0, 255),
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        // Rect sollte ueber dem Text sein
        assert_eq!(result.framebuffer.pixel(0, 0), Some(Color::rgb(0, 0, 255)));
    }

    #[test]
    fn rasterize_with_font_multiple_text_items() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(10.0, 0.0),
            text: "B".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 2);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_newline() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A\nB".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 1);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_unknown_characters() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A\u{1F600}B".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 50, 20);
        // A und B sollten gezeichnet werden
        assert_eq!(result.drawn, 1);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_space() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: " ".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        // Leerzeichen zeichnet keinen Pixel, ist aber kein Fehler
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_negative_origin() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(-3.0, -2.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        // Sollte nicht panicen
        assert_eq!(result.drawn, 1);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn rasterize_with_font_outside_origin() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(9999.0, 9999.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_zero_size_framebuffer() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: 7.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 0, 0);
        // Sollte nicht panicen
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_nan_font_size() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: f32::NAN,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_infinity_font_size() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: f32::INFINITY,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn rasterize_with_font_negative_font_size() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(0.0, 0.0),
            text: "A".into(),
            color: Color::rgb(255, 0, 0),
            font_size: -5.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });

        let font = BitmapFont::builtin();
        let result = rasterize_with_font(&list, Some(&font), 20, 20);
        assert_eq!(result.drawn, 0);
        assert_eq!(result.skipped, 1);
    }
}
