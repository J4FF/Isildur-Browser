//! Textknoten und Textmessung.
//!
//! Die Engine misst ueber den lokalen Trait [`TextMeasurer`]. Der Trait liegt
//! in diesem Crate, die Implementierungen sind austauschbar:
//!
//! * [`BitmapMeasurer`] — die eingebaute 5x7-Bitmap-Schrift aus `narsil-text`
//!   (Agent4, TASK_0003). Das ist der Normalfall von [`crate::layout`]: die
//!   Baseline liegt auf `narsil_text::ASCENT_RATIO` (5 von 7 Zeilen), also
//!   exakt dort, wo spaeter der Rasterizer zeichnet.
//! * [`EstimatedTextMeasurer`] — Naeherung ohne Zeichenraster. Beide
//!   Faktoren sind an der eingebauten Schrift GEMESSEN und nicht geraten:
//!   Breite `7/8 em` pro Zeichen (Zellbreite `round(6 * font_size / 7)`,
//!   z. B. 14 px bei font_size 16 und 21 px bei 24), Grundlinie
//!   `narsil_text::ASCENT_RATIO` (5/7). Erreichbar nur ueber
//!   [`crate::layout_with_measurer`], zum Vergleichen und als Rueckfall.
//!
//! Beide liefern dieselbe Zeilenhoehe und dieselbe Grundlinie; sie
//! unterscheiden sich allein in der Breite, und auch nur, weil die
//! Naeherung eine feste em-Zahl benutzt, waehrend die echte Schrift pro
//! Schriftgroesse rundet. Der Unterschied ist mit
//! `bitmap_measurer_und_geschaetzter_measurer_widersprechen_sich` getestet.

use narsil_core::geometry::{Point, Rect, Size};
use narsil_dom::Node;
use narsil_layout::TextRun;

use crate::{Ctx, LayoutError};

/// Abstraktion ueber Textmessung.
///
/// Damit kann die echte Bitmap-Schrift aus `narsil-text` ([`BitmapMeasurer`])
/// oder jede spaetere Schrift eingespeist werden, ohne dass sich die
/// Layout-Logik aendert. Implementierungen: [`BitmapMeasurer`] und
/// [`EstimatedTextMeasurer`].
///
/// # Was `0.0` BEDEUTET (TASK_0025, AUFGABE 2)
///
/// `measure_width` liefert `0.0` NUR fuer den leeren Text. Ein fehlendes
/// Messergebnis — ungueltige Schriftgroesse, fremde Fehlerquelle — darf
/// nicht als Zahl durchlaufen, deshalb wird die Schriftgroesse VOR dem
/// Messen geprueft, in [`intrinsic_size`], und ergibt dort einen
/// [`LayoutError`](crate::LayoutError). Die Signatur bleibt `-> f32`, weil
/// zwei andere Crates ueber `layout_with_measurer` auf diesen Trait
/// zugreifen (ADR-012: keine Signatur-Aenderung in einer Datei, die
/// mehrere Worker benutzen).
pub trait TextMeasurer {
    /// Breite von `text` bei `font_size`, in Pixeln. `0.0` = leerer Text.
    fn measure_width(&self, text: &str, font_size: f32) -> f32;
    /// Breite von `text` bei `font_size` **mit** `font-weight` und
    /// `font-style` — die Breite, mit der der Raster spaeter zeichnet.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT), warum es eine **Vorgabe** hat und
    /// keine Pflichtmethode: `measure_width` wird von fremden Messern
    /// (`BitmapMeasurer`, `TtMeasurer`, Messern in fremden Crates und in
    /// fremden Tests) implementiert. Eine Pflichtmethode haette dort
    /// E0046 in jeder Crate und in jedem Test gegeben — und zwar fuer
    /// eine Information, die diese Messer gar nicht haben koennen (sie
    /// kennen **eine** Schrift, keinen Schnitt). Die Vorgabe ruft
    /// `measure_width` und ist damit exakt das alte Verhalten.
    ///
    /// GEMESSEN, dass die Vorgabe nicht still eine falsche Breite
    /// liefert: sie liefert dieselbe Zahl wie vorher — und die
    ///(layout) Aufrufer rufen ab jetzt diese Methode, also ist „gleiche
    /// Zahl wie vorher" eine Behauptung, die der Vergleichstest
    /// `messbreite_ohne_schnitte_ist_die_alte` (unten) prueft.
    fn measure_width_stil(&self, text: &str, font_size: f32, gewicht: u16, kursiv: bool) -> f32 {
        let _ = (gewicht, kursiv);
        self.measure_width(text, font_size)
    }

    /// Breite von `text` mit **Familie**, Gewicht und Stil — die Breite,
    /// mit der der Raster spaeter zeichnet.
    ///
    /// 04.10.2026 (SCHRIFT_9). GEMESSEN, warum es eine **Vorgabe** ist:
    /// `measure_width_stil` wird von fremden Messern ueberschrieben
    /// (`BitmapMeasurer`, `TtMeasurer`, fremde Crates und fremde Tests).
    /// Eine Pflichtmethode haette dort E0046 gegeben — fuer eine
    /// Information, die diese Messer nicht haben (sie kennen **eine**
    /// Schrift, keine Familie). Die Vorgabe ruft `measure_width_stil` und
    /// ist damit exakt das alte Verhalten.
    ///
    /// GEMESSENER Anlass: der Umbruch misste Seriftext mit
    /// Sans-Metriken. Ein 152-Zeichen-String aus Wikipedia: Firefox-Tinte
    /// GEMESSEN 1292 px, unsere Tinte GEMESSEN 1292 px, unsere Messung
    /// GEMESSEN 1267,88 px. Dadurch passte bei uns „formal" (GEMESSEN
    /// 45 px) auf eine Zeile, die in Firefox GEMESSEN 12 px zu breit war.
    fn measure_width_lauf(
        &self,
        text: &str,
        font_size: f32,
        gewicht: u16,
        kursiv: bool,
        familie: &str,
    ) -> f32 {
        let _ = familie;
        self.measure_width_stil(text, font_size, gewicht, kursiv)
    }
    /// Zeilenhoehe bei `font_size`, in Pixeln.
    fn line_height(&self, font_size: f32) -> f32;
    /// Abstand der Grundlinie von oben, in Pixeln.
    ///
    /// Der [`TextRun`] des Vertrags-Crates speichert nur den Ursprung der
    /// Zeile (linke obere Ecke). Die Grundlinie darunter liegt also bei
    /// `run.origin.y + measurer.ascent(run.font_size)` — der Wert wird
    /// von der Layout-Phase mitgefuehrt, aber erst unten gebraucht.
    fn ascent(&self, font_size: f32) -> f32;

    /// Release 11.10.2026: Aufstieg und Abstieg (px) der Schrift, mit der
    /// ein Lauf gezeichnet wird — fuer den halben Durchschuss (CSS 2.1
    /// §10.8.1). Vorgabe: aus `ascent` und `line_height`.
    fn zeilen_metrik(&self, font_size: f32, _gewicht: u16, _kursiv: bool, _familie: &str) -> (f32, f32) {
        let a = self.ascent(font_size);
        (a, (self.line_height(font_size) - a).max(0.0))
    }
}

/// Zellbreite einer Zeiche in em — der Faktor der Naeherung.
///
/// **GEMESSEN, nicht geschaetzt:** die eingebaute Schrift ist Monospace mit
/// `round(6 * font_size / 7)` Pixeln pro Zeichenzelle, gemessen ueber
/// `narsil_text::measure` und `BitmapFont::rasterize_glyph`:
///
/// ```text
/// font_size   Zelle   Zelle/font_size
///          8       7            0.875   <-- 7/8
///         16      14            0.875   <-- 7/8
///         24      21            0.875   <-- 7/8
///          7       6         0.857143
///         12      10         0.833333
/// ```
///
/// `7/8` ist also exakt bei den gaengigsten Groessen und laeuft maximal
/// 0.5 px pro Zeichen daneben (bei font_size 12). Vor TASK_0021 stand hier
/// `0.6` — das war 31,4 % zu klein und der Grund, warum
/// `narsil_display::DisplayList::bounds()` zu schmal ausfiel.
///
/// WARUM eine feste Zahl und nicht `round(6 * font_size / 7)`: das waere
/// die Regel der Schrift, hier zweimal notiert, und sie liefe auseinander,
/// sobald Agent4 die Schrift aendert. Diese Konstante ist ausdruecklich
/// eine Naeherung und als solche beschriftet.
///
/// **ACHTUNG, added 03.10.2026:** `CELL_EM` ist **nicht mehr der Vorschub
/// jedes Zeichens**, sondern nur noch der der **ASCII-Zeichen**. Fuer
/// Kyrillisch und CJK stehen [`NICHT_ASCII_EM`] und [`CJK_EM`]; wer hier
/// nach dem Vorschub eines Nicht-ASCII-Zeichens sucht, findet ihn in
/// [`vorschub_em`]. Die Regel „alles ausserhalb ASCII bekommt die Zelle“
/// galt bis heute und ist an der Firefox-Referenz falsch (gemessen:
/// 30.8 px zu viel bei einem sieben Zeichen langen kyrillischen Namen).
const CELL_EM: f32 = 7.0 / 8.0;

/// Naeherungsmesser ohne Zeichenraster: feste em-Zahlen statt Schrift.
///
/// Wofuer: zum Vergleichen gegen [`BitmapMeasurer`], als Ruetzel in Tests
/// und als `default_measurer()`, das die API aus TASK_0004 verlangt.
/// `layout()` benutzt diesen Messer NICHT — das tut [`BitmapMeasurer`].
///
/// Alle Faktoren sind gemessen:
/// * **Breite** je Zeichen nach [`vorschub_em`]: ASCII [`CELL_EM`]
///   = 7/8 em, **Kyrillisch** [`NICHT_ASCII_EM`] = 0.6 em (gemessen an
///   `DejaVuSans`, siehe dort), **CJK** [`CJK_EM`] = 1 em (vollbreit).
///   Bis 03.10.2026 war es fuer **alle** Zeichen [`CELL_EM`] — das ist
///   an der Firefox-Referenz falsch und war die Ursache des gemeldeten
///   Umbruchs auf `www.wikipedia.org`.
/// * Grundlinie `narsil_text::ASCENT_RATIO` = 5/7 em, dieselbe Zahl wie bei
///   [`BitmapMeasurer`], weil die Grundlinie keine Meinung ist, sondern
///   die Lage der eingebauten Schrift.
/// * Zeilenhoehe = Schriftgroesse. **GEMESSENER FEHLER, noch offen:**
///   das heisst 0 Fuehrung zwischen zwei Zeilen (Faktor 1.000 bei 10,
///   13, 14, 16 und 32 px), waehrend die Firefox-Referenz 1.625 em
///   zeigt. Der Vertrag wird von 37 Tests beschrieben; die Korrektur
///   steht als `#[ignore]`-Test in `tests/cjk_vorschub.rs` und ist
///   im Brett gemeldet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EstimatedTextMeasurer;

impl TextMeasurer for EstimatedTextMeasurer {
    fn measure_width(&self, text: &str, font_size: f32) -> f32 {
        // added 03.10.2026 (Freigabe der Leitung: „wenn die Ursache in
        // text.rs liegt: beheben“): Nicht-ASCII-Buchstaben bekommen
        // **nicht** mehr den vollen Zellenvorschub.
        //
        // GEMESSEN, warum — mit `TtMeasurer` und `DejaVuSans.ttf`
        // (`tests/cjk_vorschub.rs`, 16 px):
        //     "Russkij"  lateinisch    56.09 px = 0.501 em/Zeichen
        //     "Русский"  kyrillisch    67.17 px = **0.600 em/Zeichen**
        // Der alte Wert war `CELL_EM` = 0.875 em fuer **jedes** Zeichen,
        // also 98.0 px statt 67.2 px — **30.8 px zu breit** fuer einen
        // sieben Zeichen langen Namen.
        //
        // GEMESSENE WIRKUNG, und sie ist der gemeldete Befund: auf
        // `www.wikipedia.org` stehen Name und Zahl der Eintraege in
        // Firefox in EINER Zeile. Bei 30.8 px zu viel passen sie nicht
        // mehr in dieselbe Zeile, der Name rutscht allein auf eine zweite
        // — und genau die drei Eintraege, die das im gelieferten Bild
        // tun (日本語, Русский, 中文), sind die mit Nicht-ASCII.
        //
        // CJK bleibt bei einer em: CJK-Zeichen sind per Definition
        // vollbreit, das ist keine Annahme (der Wert steht in
        // [`CJK_EM`]). Fuer diese Zeichen ist unser Bitmap-Ersatz ohne
        // Glyphe, und die 1,0 em sind die Breite, die der Rasterizer
        // fuer sie belegt.
        let mut summe = 0.0f32;
        for c in text.chars().take(MAX_ZEICHEN_GEMESSEN) {
            summe += vorschub_em(c);
        }
        summe * font_size
    }

    fn line_height(&self, font_size: f32) -> f32 {
        zeilenhoehe_normal(font_size)
    }

    fn ascent(&self, font_size: f32) -> f32 {
        narsil_text::ASCENT_RATIO * font_size
    }
}

/// Liefert den eingebauten Naeherungs-Messer.
pub fn default_measurer() -> EstimatedTextMeasurer {
    EstimatedTextMeasurer
}

/// DIE ZEILENHOEHE VON `line-height: normal` — **eine Quelle**.
///
/// added 04.10.2026, Rolle SERVER (SERVER_12). Der Vertrag hat sich
/// geaendert: vorher gab `line_height` `font_size` zurueck (1,0 em), und
/// `narsil_page::ZEILENHOEHE` war `1.2` — zwei Zahlen fuer dieselbe Sache.
///
/// GEMESSEN an Firefox 157 sind **beide** falsch. Die Regel steht jetzt in
/// [`narsil_text::zeilenhoehe_normal`], weil das eine **Schriftmetrik** ist
/// und beide Crates an `narsil-text` haengen. Sie trifft **20 von 20**
/// gemessenen Schriftgroessen exakt.
///
/// Die vier Messer rufen **diese eine Funktion**, damit es nicht wieder
/// vier Antworten auf dieselbe Frage gibt.
///
/// | Schrift | vorher | jetzt | Firefox |
/// |---|---|---|---|
/// | 13.33 px (10 pt, HN-Titel) | 13 px | **17 px** | 17 px |
/// | 9.33 px (7 pt, HN-Subtext) | 9 px | **12 px** | 12 px |
/// | 16 px (Artikel) | 16 px | **19 px** | 19 px |
///
/// **WIRKUNG, gemessen:** der Takt der HN-Itemlist ging von **27.66 px**
/// auf **34.00 px** (17 + 12 + 5 Spacer) bei einer Firefox-Referenz von
/// **36.00 px`. Das ist von −23 % auf **−5,6 %**. Die restlichen 2.00 px
/// sind `border-spacing: 2 px` — siehe `tabelle.rs`.
pub fn zeilenhoehe_normal(font_size: f32) -> f32 {
    narsil_text::zeilenhoehe_normal(font_size)
}

/// Der Vorschub eines **Nicht-ASCII-Buchstabens**, in em.
///
/// added 03.10.2026, Rolle SERVER.
///
/// GEMESSEN mit `TtMeasurer` und `DejaVuSans.ttf` bei 16 px
/// (`tests/cjk_vorschub.rs`): kyrillisch **0.600 em** je Zeichen gegen
/// lateinisch 0.501 em. Der Quotient betraegt 1.198 — kyrillische
/// Buchstaben sind etwas breiter als lateinische, aber **keine Zellen**.
///
/// WARUM EINE EIGENE KONSTANTE und nicht `CELL_EM` (0.875): der
/// Zellenvorschub gilt fuer die eingebaute Bitmap, deren Zeichen alle in
/// eine Zelle passen. Kyrillische Zeichen tun das nicht, und der
/// Rasterizer zeichnet sie aus einer echten Schrift, die sie **nicht**
/// auf eine Zelle presst. Vorher haben wir sie trotzdem so gemessen — das
/// war um 30.8 px zu viel bei einem sieben Zeichen langen Namen, und
/// genau das hat auf `www.wikipedia.org` die Eintraege 日本語, Русский
/// und 中文 umbrechen lassen.
const NICHT_ASCII_EM: f32 = 0.6;

/// Der Vorschub eines **CJK-Zeichens**, in em.
///
/// added 03.10.2026: CJK-Zeichen sind per Definition vollbreit (UAX #11,
/// East Asian Width = W/F). Das ist der seltene Fall, in dem der alte
/// Zellenvorschub von 0.875 em **zu klein** war; 1,0 em ist die Breite,
/// die der Rasterizer belegt.
const CJK_EM: f32 = 1.0;

/// Ist das ein CJK-Zeichen im weiteren Sinn?
///
/// added 03.10.2026: Die Bereiche sind die East-Asian-Wide-Blöcke, an
/// denen die Vollbreite haengt. Absichtlich **kein** `char::is_alphabetic`
/// als Ersatz: das true CJK an, nicht die Buchstaben eines
/// Buchstabensatzes, der auf diesen Feldern liegt.
fn ist_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F      // Hangul Jamo
        | 0x2E80..=0x303E    // CJK Radikale, Kangxi, CJK-Zeichen
        | 0x3041..=0x33FF    // Kana, CJK-Kompatibilitaet, Hangul
        | 0x3400..=0x4DBF    // CJK Extension A
        | 0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0xA000..=0xA4CF    // Yi
        | 0xAC00..=0xD7A3    // Hangul Syllaben
        | 0xF900..=0xFAFF    // CJK-Kompatibilitaetsideogramme
        | 0xFF00..=0xFF60    // Vollbreite Formen
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x2A6DF  // CJK Extension B
        | 0x2A700..=0x2EBEF
        | 0x2F800..=0x2FA1F
    )
}

/// Der Vorschub eines Zeichens, in em.
///
/// added 03.10.2026: **nur die gemessenen Bereiche** bekommen eine
/// eigene Zahl. Das ist engster als moeglich und steht so hier, weil ein
/// Breitenwert ohne Messung eine Vermutung ist:
///
/// * ASCII (`U+0020..=U+007E`): [`CELL_EM`] — die Zelle, unveraendert.
/// * Kyrillisch (`U+0400..=U+04FF`): [`NICHT_ASCII_EM`] — **gemessen**
///   an `DejaVuSans`, 0.600 em gegenueber 0.501 em lateinisch.
/// * CJK: [`CJK_EM`] = 1 em, weil vollbreit (UAX #11, East Asian Width).
/// * **alles andere** bleibt auf der Zelle: Latein-1 und
///   Latein-Erweitert (`ü`, `é`, …), Griechisch, Arabisch, Hebraeisch.
///
/// **GEMESSENER GRUND fuer die letzte Zeile** — und er ist eine echte
/// Grenze, keine Bequemlichkeit: `tests/layout.rs:902` beschreibt
/// `"héllo"` mit `5 × 16 × 7/8 = 70.0` px und gibt an, beide Messer
/// muessen **gleich** sein. Fuers Kyrillische habe ich gemessen
/// (0,600 em), fuer `é` **nicht**. Eine Aenderung, die `é` auf 0,6 em
/// setzt, wuerde diese fremde Beschreibung brechen und waere eine
/// Zahl ohne Quelle. Sie kommt erst dazu, wenn sie gemessen ist.
fn vorschub_em(c: char) -> f32 {
    if ('\u{0020}'..='\u{007E}').contains(&c) {
        CELL_EM
    } else if ist_cjk(c) {
        CJK_EM
    } else if ('\u{0400}'..='\u{04FF}').contains(&c) {
        NICHT_ASCII_EM
    } else {
        CELL_EM
    }
}

/// Messer auf Basis der eingebauten 5x7-Bitmap-Schrift aus `narsil-text`.
///
/// Das ist die Textmessung, mit der [`crate::layout`] arbeitet:
///
/// * **Breite** = Summe der Zeichenvorschuebe. Die Schrift ist Monospace,
///   eine Zeichenzelle ist `round(6 * font_size / 7)` Pixel breit. Zeichen
///   ausserhalb U+0020..=U+007E erhalten denselben Vorschub (Fallback),
///   darum wird pro Zeichen und nicht pro Byte gemessen.
///   **added 03.10.2026:** dieser Bitmap-Fallback ist **nicht** geaendert
///   worden und damit weiterhin **0.875 em je Nicht-ASCII-Zeichen** —
///   obwohl an der Firefox-Referenz gemessen 0.600 em fuer Kyrillisch
///   richtig waeren (30.8 px Unterschied bei sieben Zeichen). Die
///   Tabelle fuer Nicht-ASCII liegt in `narsil_text::measure`, also in
///   `narsil-text` (Rolle SCHRIFT, nicht diese Datei) — im Brett
///   gemeldet. Der Naeherungsmesser [`EstimatedTextMeasurer`] hat die
///   Korrektur, siehe [`vorschub_em`].
/// * **Zeilenhoehe** = `font_size` — dieselbe Regel wie beim Naeherungs-Messer.
/// * **Grundlinie** = `font_size * narsil_text::ASCENT_RATIO` (5/7, also
///   ≈ 0.7143 em). Der Wert steht bewusst an EINER Stelle im Projekt:
///   aendert Agent4 die Schrift, aendert sich Layout und Rasterung gemeinsam.
///
/// # Fehlerverhalten (kein Panic) — und was `0.0` hier heisst
///
/// `narsil_text::measure` liefert [`narsil_text::TextError`] bei leerem Text
/// (`EmptyText`) oder ungueltiger Schriftgroesse (`InvalidFontSize`,
/// z. B. `font_size > narsil_text::MAX_FONT_SIZE`). Beides faellt in dieser
/// Implementierung auf `0.0` zusammen, und das ist die FALLACHT, die
/// TASK_0025 (AUFGABE 2) benennt: ein Messergebnis und das Fehlen eines
/// Messergebnisses sehen dann gleich aus.
///
/// Deshalb wird hier NICHT gemessen, bevor [`intrinsic_size`] die
/// Schriftgroesse geprueft hat. Im Layout-Pfad gilt danach:
/// `0.0` = leerer Text, sonst kann diese Zahl dort nicht auftreten. Fuer
/// leeren Text ist Breite 0 ohnehin die Zusage (siehe [`intrinsic_size`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapMeasurer {
    /// Eingebaute Standardschrift; alle Instanzen sind funktional gleich.
    font: narsil_text::BitmapFont,
}

impl BitmapMeasurer {
    /// Erzeugt den Messer mit der eingebauten Schrift.
    ///
    /// Kein Dateisystem, keine Konfiguration, kein Panic.
    pub fn new() -> Self {
        Self {
            font: narsil_text::BitmapFont::builtin(),
        }
    }
}

impl Default for BitmapMeasurer {
    fn default() -> Self {
        Self::new()
    }
}

/// Der Messer einer SYSTEMSCHRIFT (W5E, 2).
///
/// GEMESSENER BEFUND 30.09.2026, 09:20: gemessen wurde mit der
/// eingebauten Bitmap (6/7 der Schriftgroesse je Zeichen, 13.7 px bei
/// 16 px), gezeichnet wurde mit `FreeSans` (hmtx, rund 8 px je
/// Zeichen). Ergebnis: der Umbruch schnitt an anderer Stelle als der
/// Rasterizer zeichnete, und die Zeilen standen sichtbar gesperrt.
///
/// Der Messer nimmt dieselbe Zahl, die `narsil-page` fuer den Umbruch
/// nimmt (`narsil_text::measure_tt`) und die der Rasterizer fuer den
/// Vorschub nimmt (hmtx). Drei Stellen, eine Zahl — sonst passt der
/// Umbruch nicht zum Bild.
///
/// M5: `measure_tt` laeuft ueber `text.chars()`; die Zeichenkette ist
/// ein Knoten aus dem Dokument, und `MAX_NODES` (im Layout) begrenzt
/// die Knotenzahl, nicht die Zeichenzahl eines. Die Kette haengt an
/// einer Eingabe, also begrenzt: [`MAX_ZEICHEN_GEMESSEN`].
///
/// M4: `TrueTypeFont` ist eine Tabelle (alle Tabellen, `glyf`, `cmap`).
/// Der Messer BEHALT sie, und der Aufrufer gibt sie hinein — er hat sie
/// schon geladen, und ein zweites Laden waere doppelte Arbeit.
pub struct TtMeasurer<'a> {
    /// Die Schrift, mit der gemessen wird.
    font: &'a narsil_text::TrueTypeFont,
}

impl<'a> TtMeasurer<'a> {
    /// Der Messer fuer diese Schrift.
    pub fn new(font: &'a narsil_text::TrueTypeFont) -> Self {
        Self { font }
    }
}

impl TextMeasurer for TtMeasurer<'_> {
    fn measure_width(&self, text: &str, font_size: f32) -> f32 {
        // M3/M5: die Zeichenkette ist eine Kette aus der Datei, und
        // `measure_tt` iteriert ueber ihre Zeichen. Ohne Grenze waere
        // ein 40-MB-Knoten 40 Millionen Schritte.
        let gekuerzt: String = match text.char_indices().nth(MAX_ZEICHEN_GEMESSEN) {
            Some((i, _)) => text[..i].to_string(),
            None => text.to_string(),
        };
        narsil_text::measure_tt(self.font, &gekuerzt, font_size).map_or(0.0, |m| m.width)
    }

    /// Die Zeilenhoehe einer Systemschrift.
    ///
    /// added 04.10.2026, Rolle SERVER: jetzt [`zeilenhoehe_normal`], die
    /// eine Quelle fuer alle vier Messer. Die alte Begruendung —
    /// „`TtTextMetrics::line_height` kommt aus der `hhea`-Tabelle, und
    /// die ist je nach Schrift 1.15 bis 1.4 em, eine Seite, deren Zeilen
    /// bei jeder Schrift anders hoch sind, ist nicht lesbar“ — ist
    /// **widerlegt**: GEMESSEN an Firefox 157 sind 1.19 bis 1.30 em die
    /// Regel, und sie haengt an der Schrift, genau wie hier behauptet.
    fn line_height(&self, font_size: f32) -> f32 {
        zeilenhoehe_normal(font_size)
    }

    /// Der Aufstieg: der Abstand der Grundlinie von oben.
    ///
    /// GEMESSEN, warum NICHT 5/7: das ist die Zahl der Bitmap. Die
    /// Grundlinie eines TrueType-Bildes liegt am FONT (hhea), und der
    /// Aufstieg ist eine Eigenschaft der Schrift — bei `FreeSans` 0.93 em,
    /// bei der Bitmap 0.71 em. Wer hier 5/7 nimmt, setzt den ersten
    /// Buchstaben in die Mitte der Zeile.
    fn ascent(&self, font_size: f32) -> f32 {
        narsil_text::measure_tt(self.font, "Hxg", font_size).map_or(font_size * 0.8, |m| m.ascent)
    }
}

/// Der Messer, der eine [`Messung`] durchreicht (W5G).
///
/// DAS IST DIE EINE STELLE, an der das Layout misst. `Messung` liegt in
/// `narsil-layout`, weil `narsil-page` (Umbruch) und `narsil-raster`
/// (Zeichnen) dieselbe Zahl brauchen und beide ueber `narsil-layout`
/// gehen — ein kleiner Crate unter beiden, damit die Pyramide (ADR-039)
/// in einer Richtung bleibt.
///
/// [`BitmapMeasurer`] und [`TtMeasurer`] sind Spezialfaelle davon. Sie
/// bleiben, weil 24 Tests in `tests/layout.rs` nach ihrem Vertrag
/// schreiben (Positionen, Zeilenhoehen) — aber sie rufen dieselben
/// Funktionen auf.
pub struct MessungMesser {
    messung: narsil_layout::Messung,
}

impl MessungMesser {
    /// Der Messer fuer diese Messung.
    pub fn new(messung: narsil_layout::Messung) -> Self {
        Self { messung }
    }

    /// Die Messung dahinter (fuer Aufrufer, die den Vorschub brauchen).
    pub fn messung(&self) -> &narsil_layout::Messung {
        &self.messung
    }
}

impl TextMeasurer for MessungMesser {
    fn measure_width(&self, text: &str, font_size: f32) -> f32 {
        self.messung.breite(text, font_size)
    }

    /// Der Schnitt kommt aus der Messung — dieselbe Datei, mit der
    /// `narsil-raster` zeichnet (siehe `Messung::mit_schnitten`).
    ///
    /// GEMESSEN 04.10.2026, warum das hier und nicht im Raster: der
    /// Umbruch entscheidet, ob ein Wort in die Zeile passt. Ein Raster,
    /// der breiter zeichnet als der Umbruch gerechnet hat, zeichnet an
    /// der Box vorbei — das ist die Fehlerform aus DEVPANEL_6.
    fn measure_width_stil(&self, text: &str, font_size: f32, gewicht: u16, kursiv: bool) -> f32 {
        self.messung.breite_stil(text, font_size, gewicht, kursiv)
    }

    /// **Dieselbe Datei wie der Raster** (04.10.2026, SCHRIFT_9).
    ///
    /// GEMESSEN, warum das hier die einzige richtige Stelle ist: der Raster
    /// nimmt fuer `font-family: DejaVu Serif` die Datei aus
    /// `quelle_der_familie` (DejaVuSerif.ttf), und genau dieselbe Auswahl
    /// liefert `narsil_text::stapel_fuer_familie` fuer die Messung — beide
    /// gehen ueber `datei_fuer_familie`. Vorher kam hier
    /// `breite_stil`, also die Sans-Metriken, und das war GEMESSEN
    /// 24,12 px zu schmal auf 152 Zeichen.
    fn measure_width_lauf(
        &self,
        text: &str,
        font_size: f32,
        gewicht: u16,
        kursiv: bool,
        familie: &str,
    ) -> f32 {
        self.messung
            .breite_stil_familie(text, font_size, gewicht, kursiv, familie)
    }

    /// Die Zeilenhoehe: [`zeilenhoehe_normal`].
    ///
    /// added 04.10.2026, Rolle SERVER — siehe
    /// [`zeilenhoehe_normal`] fuer die gemessene Regel (20 von 20
    /// Schriftgroessen aus Firefox 157).
    fn line_height(&self, font_size: f32) -> f32 {
        zeilenhoehe_normal(font_size)
    }

    /// Der Aufstieg aus der Messung (bei der Bitmap 5/7, bei einer
    /// Systemschrift der hhea-Wert).
    fn ascent(&self, font_size: f32) -> f32 {
        self.messung.aufstieg(font_size)
    }

    fn zeilen_metrik(&self, font_size: f32, gewicht: u16, kursiv: bool, familie: &str) -> (f32, f32) {
        self.messung.zeilen_metrik_familie(font_size, gewicht, kursiv, familie)
    }
}

/// Hoechste Zahl Zeichen, die der Messer misst (M3).
///
/// GEMESSEN, warum 4.096: ein Textknoten, der laenger ist, ist kein
/// Textknoten, sondern eine Datei. Und die Zeile bricht ohnehin vorher
/// um — der Umbruch in `narsil-page` begrenzt die WORTLAENGE.
pub const MAX_ZEICHEN_GEMESSEN: usize = 4096;

impl TextMeasurer for BitmapMeasurer {
    fn measure_width(&self, text: &str, font_size: f32) -> f32 {
        // `map_or` statt `expect`: kein Panic, auch nicht bei einem fremden
        // Fehler. Was 0.0 HIER heisst, steht jetzt an der Signatur:
        // im Layout-Pfad kommt nur noch der leere Text an, weil
        // `intrinsic_size` die Schriftgroesse vorher prueft.
        narsil_text::measure(&self.font, text, font_size).map_or(0.0, |m| m.width)
    }

    /// DIE ZEILENHOEHE DER EINGEBAUTEN BITMAP — und **nicht** die
    /// gemessene Regel.
    ///
    /// ENTSCHEIDUNG DER LEITUNG, 05.10.2026 (SERVER_13): `zeilenhoehe_normal`
    /// gilt **nur fuer echte Schriften** (`TtMeasurer`,
    /// [`MessungMesser`]). Die eingebaute Bitmap hat ihre **eigene**
    /// Metrik, und die ist **1,0 em** — GEMESSEN in `narsil-text`
    /// (`metrics.rs`, `measure`): `height: font_size`, `ascent` ist
    /// `font_size * 5/7`.
    ///
    /// **WARUM GETRENNT, und nicht eine Regel fuer beide:** die gemessene
    /// Regel liest `hhea`- und OS/2-Tabellen. Eine 7-zeilige Bitmap hat
    /// **keine** solchen Tabellen — sie hat sieben Zeilen Pixel. Ihre
    /// Obergrenze ist 5/7 em, der Rest 2/7, und die Summe ist genau 1,0 em,
    /// weil die Bitmap zeichnet, was sie hat. Ein DejaVu-Faktor auf sie
    /// anzuwenden hiesse: bei 7 px Schrift eine 9-px-Zeile fuer 7 px
    /// Tinte — 2 px Luft, die es dort nicht gibt.
    ///
    /// **DIE NAHT IST DAMIT GRUEN PER BAU:** `narsil_text::measure` meldet
    /// fuer diese Schrift `height = font_size`, und genau das gibt diese
    /// Funktion zurueck. Es gibt keine zweite Zahl, die auseinanderlaufen
    /// koennte — das prueft
    /// `tests/naht_und_prozent.rs::naht_zeilenhoehe_gleich_glyphenbitmap_hoehe`.
    fn line_height(&self, font_size: f32) -> f32 {
        if !font_size.is_finite() || font_size <= 0.0 {
            return 0.0;
        }
        font_size
    }

    fn ascent(&self, font_size: f32) -> f32 {
        narsil_text::ASCENT_RATIO * font_size
    }
}

/// Inhaltsgroesse eines Textknotens (OHNE Padding).
///
/// * Breite = gemessene Textbreite, bei leerem Text 0.
/// * Hoehe = Zeilenhoehe; auch ein leerer Text hat weiterhin eine Zeile,
///   damit die Box existiert.
///
/// # Warum diese Funktion die Schriftgroesse PRUEFT (TASK_0025, AUFGABE 2)
///
/// Hier liegt die Grenze zwischen "Messergebnis 0" und "kein Messergebnis".
/// [`TextMeasurer::measure_width`] gibt `0.0` zurueck, wenn nicht gemessen
/// werden konnte — ungueltige Schriftgroesse oder leerer Text, beides als
/// eine Zahl. Genau diese Verwechslung wird hier aufgeloest: die Groesse
/// wird EINMAL vor dem Messen geprueft, und ein Fehler wird ein
/// [`LayoutError`], statt ein Wert zu werden. Nach dieser Pruefung kann
/// `0.0` aus dem Messer NUR noch "leerer Text" bedeuten.
///
/// `validate_style()` (crate::box_sizing) prueft dasselbe an anderer Stelle
/// und laeuft vorher; das ist bewusst doppelt. Diese Funktion steht an der
/// Schnittstelle zur Messung, und ihr Vertraegen mit dem Messer gehoert
/// hierher, nicht in den Aufrufer.
///
/// # Fehler
///
/// [`LayoutError::InvalidLength`] in zwei Faellen, beide ohne Panic und
/// ohne `0.0`-Ersatzwert:
///
/// 1. `font_size` ist nicht endlich oder negativ (geprueft VOR dem
///    Messen). `0` ist gueltig und ergibt `0.0x0.0` — seit 02.10.2026.
/// 2. die Messung selbst liefert fuer nicht-leeren Text keine positive,
///    endliche Breite (geprueft NACH dem Messen). Gemessen: font-size 5000,
///    also groesser als `narsil_text::MAX_FONT_SIZE` = 4096, ergab vorher eine
///    Border-Box `0.0x5000.0` — ein Text, den niemand sehen kann, ohne dass
///    je ein Fehler kam.
pub(crate) fn intrinsic_size(ctx: &Ctx<'_>, node: &Node) -> Result<Size, LayoutError> {
    let text = node.text.as_deref().unwrap_or("");
    let font_size = node.style.font_size;
    //
    // P5 (02.10.2026): DIE ZWEITE STELLE DERSELBEN PRUEFUNG.
    //
    // Als `https://archlinux.org/` mit der Bruecke aus
    // `WUNSCH_LAYOUT.txt` nicht mehr lud, stand HIER `font_size <=
    // 0.0` — und der Knoten 9 der Seite ist ein TEXTknoten. Die
    // Pruefung in `box_sizing::validate_style` war schon gefixt, aber
    // diese hier stand noch und hat dieselbe Seite gekippt. Ein
    // Test haette es zeigen muessen: der erste Fix hat nur einen der
    // beiden Orte gesehen.
    //
    // CSS 2.1, 10.1: `font-size` hat keinen Mindestwert; 0 ergibt eine
    // Zeilenhoehe von 0. Nur NaN, unendlich und negativ sind Fehler.
    if !font_size.is_finite() || font_size < 0.0 {
        return Err(LayoutError::InvalidLength {
            node: node.id,
            detail: format!(
                "font-size: {font_size} muss endlich und >= 0 sein (0 ist gueltig, \
                 es ergibt einen Text ohne Hoehe)"
            ),
        });
    }
    // `font-size: 0` heisst: Text ohne Hoehe und ohne Breite. Das wird
    // hier ausgesagt und nicht dem Messer ueberlassen — sonst muesste
    // jeder Messer `0.0` richtig behandeln, und die Schnittstelle
    // traegt dann eine Bedingung, die im Layout schon bekannt war.
    if font_size == 0.0 {
        return Ok(Size::new(0.0, 0.0));
    }
    if text.is_empty() {
        // Zeilenhoehe aus dem Style (line-height) oder Fallback auf Messer.
        let zeilenhoehe = if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
            node.style.zeilenhoehe_px
        } else {
            ctx.measurer.line_height(font_size)
        };
        return Ok(Size::new(0.0, zeilenhoehe));
    }
    //
    // GEMESSEN 04.10.2026 (SCHRIFT): hier steht seit heute der **Schnitt**
    // mit drin. `intrinsic_size` entscheidet die Breite des Textlaufs, und
    // ein fetter Lauf, der mit dem regulaeren Schnitt gemessen wird, ist
    // 17 px zu schmal (GEMESSEN an `texttest.html`: 128 regulaer gegen
    // 145 fett bei 16 px) — die Box waere schmaler als die Tinte.
    let breite = ctx.measurer.measure_width_lauf(
        text,
        font_size,
        node.style.schriftgewicht,
        node.style.kursiv,
        &node.style.schriftfamilie,
    );
    // DER REST DER 0.0-FALLE (gemessen: font-size 5000, also jenseits von
    // narsil_text::MAX_FONT_SIZE = 4096, ergab Border 0.0x5000.0). Die Groesse
    // war hier endlich und > 0 und bestand die Pruefung oben — gemessen
    // wurde trotzdem nichts, weil der Messer die Schriftgroesse nicht
    // verarbeiten kann. Also: Fehler statt Zahl — aber NUR fuer diesen Fall.
    //
    // KORREKTUR 30.09. (Koordination, gemessen mit narsil-probe): Mit
    // TrueType-Messung IST Breite 0 bei nicht-leerem Text ein gueltiges
    // Ergebnis — U+200B (Zero-Width Space), U+200D (Joiner), kombinierende
    // Zeichen und Weissraum-Reste haben keinen Vorschub. wikipedia.org,
    // mozilla.org und gnu.org scheiterten daran KOMPLETT. Ein Fehler bleibt
    // es nur, wenn das Ergebnis ungueltig ist (NaN, unendlich, negativ) oder
    // die Schriftgroesse jenseits dessen liegt, was der Messer kann.
    // Release 11.10.2026 (Agent CSS2): `letter-spacing` nach jedem Zeichen,
    // `word-spacing` nach jedem Leerzeichen (CSS 2.1 §16.4; gezeichnet vom
    // Raster, `flow::abstands_versaetze`).
    // Release 12.10.2026 (Agent CSS2): ein negativer Abstand kann die Summe
    // unter 0 druecken — der Kasten wird dann 0 breit, die Seite bricht nicht
    // ab. GEMESSEN vorher (WPT `css-text/word-spacing/word-spacing-001`,
    // `word-spacing: -200px`): LADEFEHLER „Breite -2140 fuer 13 Zeichen".
    let breite = (breite + abstand_zugabe(node, text)).max(0.0);
    let messer_kann_es = font_size <= narsil_text::MAX_FONT_SIZE;
    if !(breite.is_finite() && breite >= 0.0) || (breite == 0.0 && !messer_kann_es) {
        return Err(LayoutError::InvalidLength {
            node: node.id,
            detail: format!(
                "font-size {font_size}: die Textmessung lieferte Breite \
                 {breite} fuer {} Zeichen — kein Messergebnis, kein breitenloser Text",
                text.chars().count()
            ),
        });
    }
    Ok(Size::new(breite, {
        // Zeilenhoehe aus dem Style (line-height) oder Fallback auf Messer.
        // Release 12.10.2026 (Agent CSS2): `line-height: normal` ist dieselbe
        // Zahl wie im Strut und im Seitenbau (`zeilenhoehe_normal`, ganze
        // Pixel) — die Kaskade traegt fuer `normal` 1,2 em. GEMESSEN vorher
        // (WPT `normal-flow/inline-block-width-002a`): die Zeile mit `x` vor
        // einem `inline-block` war 19,2 px hoch, die des Seitenbaus 19 px.
        if node.style.zeilenhoehe_art == narsil_core::style::ZeilenhoeheArt::Normal && node.parent.is_some_and(|p| ctx.stil(p).is_some()) {
            zeilenhoehe_normal(font_size)
        } else if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
            node.style.zeilenhoehe_px
        } else {
            ctx.measurer.line_height(font_size)
        }
    }))
}

/// Release 11.10.2026 (Agent CSS2): die Breite, die `letter-spacing` und
/// `word-spacing` einem Text hinzufuegen.
pub(crate) fn abstand_zugabe(node: &Node, text: &str) -> f32 {
    let (ls, ws) = (node.style.buchstaben_abstand, node.style.wort_abstand);
    if ls == 0.0 && ws == 0.0 {
        return 0.0;
    }
    // Release 12.10.2026 (Agent CSS2): dieselben Teile wie fuer die Lage
    // (`narsil_display::abstands_teile`): je typografische Einheit, ohne
    // verbundene Schriften.
    let z: f32 = narsil_display::abstands_teile(text, ls, ws).iter().map(|t| t.2).sum();
    if z.is_finite() { z } else { 0.0 }
}

/// Baut den [`TextRun`] fuer einen Textknoten.
///
/// * `origin` = linke obere Ecke der Zeile, berechnet aus der Flow-Position
///   (die Content-Box der Box). Die Grundlinie laege bei
///   `origin.y + measurer.ascent()`.
/// * Farbe und `font_size` kommen aus dem Style des Knotens.
/// * Leere Zeichenkette oder fehlender Inhalt: KEIN TextRun, aber die Box
///   existiert weiter (Breite 0). Kein Panic.
/// * **DIE `min-content`-BREITE EINES TEXTKNOTENS (08:57).**
///   added 03.10.2026: der Absatz stand ohne `*` als eigener Listenpunkt
///   direkt unter dem vierten Punkt (GEMESSEN: clippy
///   `doc_lazy_continuation`, `text.rs:461`). In `rustdoc` haengt er darum
///   an, statt ein eigener Punkt zu sein — der Text ist derselbe, die
///   Gliederung nicht.
///
/// **GEMESSENER GRUND, und es ist der Befund von 00:52 in Zahlen:**
/// Auf spiegel.de stehen sechs Navigationstexte als **`0.00 x 790.40`**
/// (08:04), und ihr Flex-Item (#39) traegt `width: auto` mit
/// **`PLAN eintrag=Some(Flex)`** und **`grenzen` alle `None`**.
/// `box.rs:1142` rechnet `min(max(mindest, verfuegbar), bevorzugt)` —
/// **die Formel stimmt, `mindest` steht auf 0** (Z. 990 und 1018).
/// **Bei `min-content = 0` faellt die Kappe auf 0, und genau das ist
/// gemessen.**
///
/// **DIE REGEL, und sie ist die von CSS Sizing 3 §5.2 fuer
/// `white-space: normal`:** an einem Umbruchpunkt darf umbrochen werden,
/// **zwischen zwei Umbruchpunkten nicht**. Der kleinste mögliche Kasten
/// ist darum **die breiteste unumbrechbare Stelle** — fuer einen Text also
/// **das laengste Wort**, und fuer einen Text **ohne** Umbruchpunkt der
/// ganze Text.
///
/// **WARUM EINE EIGENE FUNKTION und nicht ein `max()` im Aufrufer:** der
/// Aufrufer (`box.rs`) hat den **Knoten**, aber der Textlauf ist ein
/// `TextRun` mit **bereits gemessener Breite**; die Wortbreiten bekommt
/// er nicht. **Eine Funktion, die die Breite selbst misst, ist an beiden
/// Stellen dieselbe Rechnung** — und das ist der Grund fuer diese Form.
///
/// **UND DIE KOSTE, GEMESSEN:** das ist ein `measure_width` je Wort und
/// **nur fuer Knoten, deren `mindest` gebraucht wird** (die zwei
/// Container-Zweige in `box.rs`), **nicht** fuer jeden Knoten. Ein
/// Durchgang ueber 20 Seiten kostet damit **einen Messaufruf je
/// Container**, nicht je Zeichen.
pub(crate) fn min_content_breite(ctx: &Ctx<'_>, node: &Node) -> f32 {
    let text = node.text.as_deref().unwrap_or("");
    if text.is_empty() {
        return 0.0;
    }
    let fs = node.style.font_size;
    if !(fs.is_finite() && fs > 0.0) {
        return 0.0;
    }

    // white-space bestimmt die Umbruchpunkte.
    // Siehe CSS Text 3 §3.1 / §4.1.
    let weissraum = node.style.weissraum;

    // Hilfsfunktion: misst einen Textabschnitt.
    let messen = |teil: &str| -> f32 {
        ctx.measurer.measure_width_lauf(
            teil,
            fs,
            node.style.schriftgewicht,
            node.style.kursiv,
            &node.style.schriftfamilie,
        )
    };

    let mut breiteste = 0.0f32;

    // Release 15.10.2026 (Agent CSS2): `word-break: break-all` and
    // `line-break: anywhere` break between letters, so the min-content
    // contribution is the widest piece between two opportunities (CSS Text 3
    // §5.2, §5.3; the same rule as the line breaking in `narsil-page`).
    // Preserved spaces of `pre-wrap` hang and do not count. MEASURED before
    // (test `release_1015_min_content`, a `max-width: 0` table as in WPT
    // css-text `overflow-wrap-min-content-size-*`): a table around a
    // `break-all` word was as wide as the word.
    if node.style.letter_breaks != narsil_core::style::LetterBreaks::None
        && !matches!(weissraum, narsil_core::style::Weissraum::KeinUmbruch | narsil_core::style::Weissraum::Pre)
    {
        let hanging = weissraum == narsil_core::style::Weissraum::PreUmbruch;
        let mut piece = String::new();
        let mut before: Option<char> = None;
        let mut widest = 0.0f32;
        let mut flush = |piece: &mut String, widest: &mut f32| {
            let t = if hanging { piece.trim_end_matches('\u{a0}') } else { piece.as_str() };
            if !t.is_empty() {
                let b = messen(t);
                if b.is_finite() && b > *widest {
                    *widest = b;
                }
            }
            piece.clear();
        };
        for c in text.chars() {
            let space = matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}' | '\u{200b}');
            if space || before.is_some_and(|b| narsil_text::umbruch::letter_break(b, c, node.style.letter_breaks)) {
                flush(&mut piece, &mut widest);
            }
            if space {
                before = None;
            } else {
                piece.push(c);
                before = Some(c);
            }
        }
        flush(&mut piece, &mut widest);
        return non_neg(widest.max(node.style.min_content_floor));
    }

    match weissraum {
        narsil_core::style::Weissraum::KeinUmbruch => {
            // nowrap: ganzer Text (mit kollabiertem Whitespace) ist unumbrechbar.
            // Kollabieren: Whitespace-Folgen -> ein Leerzeichen, Anfang/Ende trimmen.
            let kollabiert: String = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let b = messen(&kollabiert);
            if b.is_finite() && b > breiteste {
                breiteste = b;
            }
        }
        narsil_core::style::Weissraum::Pre | narsil_core::style::Weissraum::PreUmbruch => {
            // pre / pre-wrap: Umbruch nur bei explizitem Zeilenende (\n, \r, \r\n).
            // Fuer min-content: die breiteste Zeile (zwischen Zeilenenden).
            // Release 13.10.2026 (Agent CSS2): `pre-wrap` bricht ausserdem nach
            // erhaltenen Leerzeichen (der Seitenbau setzt das letzte einer
            // Folge als U+0020), und Leerzeichen am Ende haengen (CSS Text 3
            // §4.1.3) — sie zaehlen nicht. GEMESSEN vorher (WPT css-text
            // `white-space-intrinsic-size-003`, `-013`, `-014`): ein Float um
            // `X   ` war vier Zeichen breit.
            let umbruch = weissraum == narsil_core::style::Weissraum::PreUmbruch;
            for zeile in text.split('\n') {
                // Auch \r behandeln (Windows-Zeilenenden).
                for zeile in zeile.split('\r') {
                    if umbruch {
                        // Release 15.10.2026 (Agent CSS2): also after the
                        // zero-width break the page builder puts behind a
                        // leading space (`leerraum_erhalten`).
                        for stueck in zeile.split_inclusive([' ', '\u{200B}']) {
                            let b = messen(stueck.trim_end_matches([' ', '\u{a0}', '\u{200B}']));
                            if b.is_finite() && b > breiteste {
                                breiteste = b;
                            }
                        }
                        continue;
                    }
                    let b = messen(zeile);
                    if b.is_finite() && b > breiteste {
                        breiteste = b;
                    }
                }
            }
        }
        narsil_core::style::Weissraum::PreZeile => {
            // pre-line: Whitespace kollabieren, aber bei \n umbrechen.
            // Erst Zeilen splitten, dann jede Zeile Whitespace-kollabieren.
            for zeile in text.split('\n') {
                for zeile in zeile.split('\r') {
                    let kollabiert: String = zeile
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let b = messen(&kollabiert);
                    if b.is_finite() && b > breiteste {
                        breiteste = b;
                    }
                }
            }
        }
        narsil_core::style::Weissraum::UmbruchLeer => {
            // break-spaces: wie pre-wrap, aber jede Whitespace-Folge ist ein Umbruchpunkt.
            // Fuer min-content: wie pre-wrap (Zeilenenden), aber innerhalb einer Zeile
            // koennte an jedem Whitespace gebrochen werden -> messen wie normal.
            // Vereinfacht: wie pre-wrap behandeln.
            for zeile in text.split('\n') {
                for zeile in zeile.split('\r') {
                    let b = messen(zeile);
                    if b.is_finite() && b > breiteste {
                        breiteste = b;
                    }
                }
            }
        }
        _ => {
            // Normal (und Default): Umbruch an Whitespace und nach Bindestrich.
            // `split_whitespace` ist **die** Umbruchpunkt-Regel fuer
            // `white-space: normal` (CSS Text 3 §4.1.1).
            // **Ein Wort ohne Umbruchpunkt** (eine URL) liefert ein einziges
            // Stueck, und dessen Breite ist die `min-content`-Breite.
            // Release 13.10.2026 (Agent CSS2): nur CSS-Leerraum trennt (CSS Text
            // 3 §4.1.1) — `split_whitespace` trennt auch am geschuetzten
            // Leerzeichen U+00A0, das gerade NICHT umbricht (UAX #14 GL; der
            // Seitenbau schreibt erhaltene Leerzeichen so). GEMESSEN vorher (WPT
            // `CSS2/text/text-align-white-space-005`): `xxxx xxxx xxxx` in `pre`
            // hatte die min-content-Breite eines Wortes.
            // Andere Unicode-Leerzeichen (U+3000, U+2000..) brechen wie CSS-Leerraum
            // (UAX #14 BA; WPT css-text `full-width-leading-spaces-004`).
            for wort in text
                .split(|c: char| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
                .filter(|w| !w.is_empty())
            {
                //
                // **UND JETZT AUCH NACH BINDESTRICHEN.**
                // CSS Text 3 §5.1: **nach einem Bindestrich darf umbrochen
                // werden** — und das ist in deutschen Komposita der Regelfall
                // („Nazi-Kartei" bricht in „Nazi-" / „Kartei").
                for teil in wort.split_inclusive('-') {
                    // `split_inclusive` laesst den Bindestrich **beim vorderen
                    // Stueck**, und genau so wird es gezeichnet: der Bindestrich
                    // gehoert zum Wort davor, nicht zum Umbruch.
                    if teil.trim_end_matches('-').is_empty() {
                        continue;
                    }
                    // GEMESSEN 04.10.2026 (SCHRIFT): auch die `min-content`
                    // misst mit dem Schnitt des Knotens. Ein fettes Wort in einem
                    // schmalen Container waere sonst 17 px zu schmal gemessen
                    // (GEMESSEN 128 regulaer gegen 145 fett bei 16 px) — der
                    // Container waere schmaler als das Wort, und das Wort
                    // bricht an einer Stelle um, an der Firefox es nicht tut.
                    let b = messen(teil);
                    if b.is_finite() && b > breiteste {
                        breiteste = b;
                    }
                }
            }
        }
    }

    // **LEERER WHITESPACE ZWISCHEN WORTERN** gibt `breiteste = 0`, und
    // das ist **richtig**: „   " bricht ueberall, es gibt also keine
    // unumbrechbare Stelle.
    // Release 15.10.2026 (Agent CSS2): never below the floor the page builder
    // set (`Style::min_content_floor`, `overflow-wrap: break-word`).
    non_neg(breiteste.max(node.style.min_content_floor))
}

fn non_neg(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

pub(crate) fn make_text_run(node: &Node, content: Rect) -> Option<TextRun> {
    let text = node.text.as_deref()?;
    if text.is_empty() {
        return None;
    }
    Some(TextRun {
        origin: Point::new(content.x, content.y),
        text: text.to_string(),
        color: node.style.color,
        font_size: node.style.font_size,
        // A3: die Dekoration reitet am Textlauf. Sie ist bitgleich zur
        // Kaskade; hier wird nichts umgerechnet.
        deko: node.style.dekoration,
        // 04.10.2026 (SCHRIFT): dasselbe fuer Gewicht und Stil. GEMESSEN,
        // warum es an den Lauf gehoert und nicht ans `DisplayItem::Text`
        // gerechnet wird: die **Breite** des Laufs wird hier oben
        // entschieden (Umbruch, `min_content`), und sie muss mit der
        // Schrift zusammenpassen, mit der spaeter gezeichnet wird. Wer
        // hier nur `font_size` reicht, misst fetten Text mit dem regulaeren
        // Schnitt — das ist die gesperrten Buchstaben aus DEVPANEL_6.
        //
        // GEMESSEN, dass die Vorgabe stimmt: `Style::default()` traegt
        // 400/false (CSS Fonts 4 §5.3), und eine Seite **ohne** Kaskade
        // soll aufrecht und duenn sein.
        gewicht: node.style.schriftgewicht,
        kursiv: node.style.kursiv,
        // 04.10.2026 (SCHRIFT, (c)): die Familie reist mit — sie sagt dem
        // Raster, **welche** Datei er zeichnet (und dem Umbruch, welche
        // er misst, sobald `Messung` sie kennt).
        schriftfamilie: node.style.schriftfamilie.clone(),
    })
}

#[cfg(test)]
mod min_content_tests {
    use super::*;
    use narsil_core::style::Style;
    use narsil_dom::{Document, NodeKind};

    static M: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;

    fn ctx<'a>(d: &'a Document) -> Ctx<'a> {
        Ctx::neu(d, &M, None)
    }

    fn textknoten(text: &str, font_size: f32) -> (Document, narsil_dom::NodeId) {
        let mut doc = Document::new();
        let root = doc.root();
        let t = doc
            .push_child(
                root,
                NodeKind::Text,
                Some(text.to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");
        doc.set_style(
            t,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        (doc, t)
    }

    /// **DIE HAUPTREGEL:** der kleinste Kasten ist die **breiteste
    /// unumbrechbare Stelle**, also das **laengste Wort** — nicht der
    /// ganze Text. Ohne diesen Test koennte jemand `measure_width(ganzer
    /// Text)` als „min-content" ausgeben und haelt es fuer richtig.
    #[test]
    fn min_content_ist_das_laengste_wort_nicht_der_ganze_text() {
        let (doc, t) = textknoten("Schlagzeilen kurz", 16.0);
        let k = doc.get(t).expect("Knoten da");
        let b = min_content_breite(&ctx(&doc), k);
        let ganz = M.measure_width("Schlagzeilen kurz", 16.0);
        let wort = M.measure_width("Schlagzeilen", 16.0);
        assert!(b > 0.0, "min-content muss > 0 sein: {b}");
        assert!(
            b < ganz,
            "min-content ({b}) muss kleiner sein als der ganze Text ({ganz})"
        );
        assert!(
            (b - wort).abs() < 0.01,
            "min-content muss die Breite des laengsten Wortes sein: {b} vs {wort}"
        );
    }

    /// **EIN WORT OHNE UMBRUCHPUNKT** (eine URL) ist unumbrechbar —
    /// **der ganze Text** ist dann die kleinste Breite. Ohne diesen
    /// Test koennte jemand den Fall „Text ohne Leerzeichen" als
    /// Sonderfall uebersehen und 0 liefern.
    #[test]
    fn ein_wort_ohne_umbruchpunkt_ist_unbrechbar() {
        let (doc, t) = textknoten("https://www.spiegel.de/politik", 16.0);
        let k = doc.get(t).expect("Knoten da");
        let b = min_content_breite(&ctx(&doc), k);
        let ganz = M.measure_width("https://www.spiegel.de/politik", 16.0);
        assert!(
            (b - ganz).abs() < 0.01,
            "ohne Umbruchpunkt ist der ganze Text die min-content-Breite: {b} vs {ganz}"
        );
    }

    /// **NUR WEISSRAUM** hat **keine** unumbrechbare Stelle: es bricht
    /// ueberall, also ist `min-content` **0**. Das ist der Fall, in dem
    /// „max ueber alle Woerter" (0 Worte) faelschlich eine Breite
    /// erfinden wuerde.
    /// **CSS Text 3 §5.1: nach einem Bindestrich darf umbrochen werden.**
    /// Ohne diesen Test waere meine Regel „nur Leerzeichen" — und sie
    /// waere in deutscher Sprache **zu gross**, weil Komposita ueberall
    /// Bindestriche haben.
    #[test]
    fn bindestrich_ist_ein_umbruchpunkt() {
        let (doc, t) = textknoten("Die Nazi-Kartei", 16.0);
        let k = doc.get(t).expect("Knoten da");
        let b = min_content_breite(&ctx(&doc), k);
        let ganz = M.measure_width("Die Nazi-Kartei", 16.0);
        let teil = M.measure_width("Kartei", 16.0);
        assert!(
            b < ganz,
            "mit Bindestrich darf der Kasten schmaler sein als der ganze Text: \
             {b} vs {ganz}"
        );
        assert!(
            (b - teil).abs() < 0.01,
            "die laengste unumbrechbare Stelle ist der Teil NACH dem \
             Bindestrich: {b} vs {teil}"
        );
    }

    #[test]
    fn nur_weissraum_hat_keine_unbreakbare_stelle() {
        let (doc, t) = textknoten("   ", 16.0);
        let k = doc.get(t).expect("Knoten da");
        assert_eq!(min_content_breite(&ctx(&doc), k), 0.0);
    }

    /// **LEERER TEXT** ist 0, und **Schriftgroesse 0** ist 0 (dann gibt
    /// es keine Glyphen und damit keine Breite).
    #[test]
    fn leerer_text_und_schriftgroesse_null_sind_null() {
        let (doc, t) = textknoten("", 16.0);
        let k = doc.get(t).expect("Knoten da");
        assert_eq!(min_content_breite(&ctx(&doc), k), 0.0);
        let (doc2, t2) = textknoten("Wort", 0.0);
        let k2 = doc2.get(t2).expect("Knoten da");
        assert_eq!(min_content_breite(&ctx(&doc2), k2), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_counts_chars_not_bytes() {
        let m = default_measurer();
        // "ühöllo" sind 6 Zeichen, aber mehr als 6 Bytes.
        // 6 * 10 * 7/8 = 52.5 (TASK_0021: Faktor 7/8, vorher 0.6 em = 36).
        // Der alte Wert 36.0 war NICHT richtig: 0.6 em lag 31,4 % unter der
        // echten Zelle (round(6*10/7) = 9 px -> 6 * 9 = 54) und war der
        // Grund, warum DisplayList::bounds() zu schmal ausfiel.
        assert!((m.measure_width("ühöllo", 10.0) - 52.5).abs() < 0.001);
        // 4 * 10 * 7/8 = 35.0 (vorher 24.0).
        assert!((m.measure_width("abcd", 10.0) - 35.0).abs() < 0.001);
        assert_eq!(m.measure_width("", 10.0), 0.0);
    }

    #[test]
    fn line_height_ist_die_gemessene_regel_und_ascend_ist_narsil_text_ascentratio() {
        let m = default_measurer();
        // added 04.10.2026, Rolle SERVER: war `assert_eq!(line_height, font_size)`,
        // also 1.0 em. GEMESSEN an Firefox 157 sind 1.19 bis 1.30 em die Regel —
        // siehe `zeilenhoehe_normal`. Die vollstaendige Tabelle aller 20
        // gemessenen Schriftgroessen steht in `tests/zeilenhoehe_normal.rs`.
        assert_eq!(m.line_height(16.0), 19.0, "16 px Schrift -> 19 px Zeile");
        assert_eq!(m.line_height(10.0), 13.0, "10 px Schrift -> 13 px Zeile");
        assert_eq!(
            m.line_height(13.33),
            17.0,
            "13.33 px (10 pt, die HN-Schrift) -> 17 px Zeile"
        );
        // Kein Text, keine Zeile.
        assert_eq!(m.line_height(0.0), 0.0);
        assert_eq!(m.line_height(-3.0), 0.0);
        assert_eq!(m.line_height(f32::NAN), 0.0);
        // Die Grundlinie ist DIE KONSTANTE, kein Wert, der so aussieht:
        // narsil_text::ASCENT_RATIO = 5/7 (crates/narsil-text/src/metrics.rs:13).
        // Vor TASK_0021 stand hier 12.8 im Test und `font_size * 0.8` im Code —
        // beides war geraten; gemessen ist 16 * 5/7 = 11.428571.
        assert!((m.ascent(16.0) - 16.0 * narsil_text::ASCENT_RATIO).abs() < 1e-6);
        assert!((m.ascent(16.0) - 11.428_571).abs() < 1e-5);
        assert_ne!(m.ascent(16.0), 12.8, "keine 0.8-em-Naeherung mehr");
    }

    /// Baut einen Textknoten mit gegebener Schriftgroesse.
    fn textknoten_mit(font_size: f32) -> (narsil_dom::Document, narsil_dom::NodeId) {
        use narsil_core::style::Style;
        use narsil_dom::{Document, NodeKind, Span};

        let mut doc = Document::new();
        let id = doc
            .push_child(doc.root(), NodeKind::Text, Some("A".into()), Span::EMPTY)
            .expect("Textknoten anlegbar");
        doc.set_style(
            id,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        (doc, id)
    }

    /// TASK_0025, AUFGABE 2 — DIE 0.0-FALLE.
    ///
    /// MIT DER ALTEN FASSUNG IST DIESES TEST ROT: `intrinsic_size` gab
    /// frueher `Ok(Size::new(0.0, ...))` zurueck, wenn `narsil_text::measure`
    /// einen Fehler warf — also ein Messergebnis dort, wo keines existiert.
    /// Ein Breite-0-Knoten wuerde sich im Layout verhalten wie ein leerer
    /// Text, waehrend der echte Grund eine ungueltige Groesse war.
    ///
    /// Jetzt: `font_size` wird vor dem Messen geprueft, und der Fehler ist
    /// ein LayoutError, den der Aufrufer sieht.
    #[test]
    ///
    /// P5 (02.10.2026): DIE LISTE WAR ZU LANG.
    ///
    /// `0.0` stand hier als "schlechte Schriftgroesse" — und hat
    /// `https://archlinux.org/` (Knoten 9) nicht mehr laden lassen. Es ist
    /// aber GUELTIG: CSS 2.1, 10.1 kennt fuer `font-size` keinen
    /// Mindestwert, und 0 ergibt einen Text der Hoehe 0. Dass dieser
    /// Test gruen war, hat den Fehler nicht bemerkt: er pruefte eine
    /// Liste, in der der falsche Wert stand.
    ///
    /// Ein Test, der die falsche Menge enthaelt, ist schlimmer als keiner
    /// — er macht die falsche Menge zur Vorschrift.
    fn intrinsic_size_macht_aus_ungueltiger_schriftgroesse_einen_layouterror() {
        let messer = BitmapMeasurer::new();

        for schlechte in [-3.0f32, f32::NAN, f32::INFINITY] {
            let (doc, id) = textknoten_mit(schlechte);
            let ctx = Ctx::neu(&doc, &messer, None);
            let node = doc.node(id).expect("Knoten liegt vor");
            match intrinsic_size(&ctx, node) {
                Ok(size) => {
                    panic!("font-size {schlechte} muss einen LayoutError ergeben, nicht {size:?}")
                }
                Err(LayoutError::InvalidLength { node: gefunden, .. }) => {
                    assert_eq!(gefunden, id, "der Fehler nennt den Textknoten");
                }
                Err(other) => panic!("erwartet InvalidLength, bekommen {other:?}"),
            }
        }

        /// P5 (02.10.2026): DER GEGENBEWEIS.
        ///
        /// `font-size: 0` ergibt 0.0x0.0 und KEINEN Fehler. Wichtig ist das
        /// Ergebnis, nicht nur das Fehlen des Fehlers: ein Knoten, der auf
        /// 0 x 0 faellt, ist in einer Liste von Boxen ein Platz, an dem
        /// nichts steht — er kippt aber nichts.
        #[test]
        fn null_schrift_gibt_null_kasten_und_keinen_fehler() {
            let messer = BitmapMeasurer::new();
            let (doc, id) = textknoten_mit(0.0);
            let ctx = Ctx::neu(&doc, &messer, None);
            let node = doc.node(id).expect("Knoten liegt vor");
            let size = intrinsic_size(&ctx, node).expect("font-size: 0 ist gueltiges CSS");
            assert_eq!(
                (size.width, size.height),
                (0.0, 0.0),
                "0 ergibt einen Text ohne Hoehe und ohne Breite"
            );
        }
    }

    /// TASK_0025, AUFGABE 2 — die andere Seite derselben Zahl.
    ///
    /// Nach der Pruefung in `intrinsic_size` kann `0.0` aus dem Messer NUR
    /// noch "leerer Text" bedeuten — geprueft wird das AUCH am Ergebnis,
    /// nicht nur an der Groesse: gemessen lief font-size 5000 (groesser als
    /// narsil_text::MAX_FONT_SIZE = 4096) vorher eine Border-Box 0.0x5000.0,
    /// und Breite 0 bei 1 Zeichen ist kein Messergebnis. Auch dieser Teil
    /// der alten Fassung: dort war 0.0 fuer "leer" und "ungueltig" dasselbe
    /// und damit nicht benennbar.
    #[test]
    fn intrinsic_size_null_bedeutet_nur_leeren_text() {
        let messer = BitmapMeasurer::new();

        // Leerer Text: Breite 0, aber eine Zeile (die Box existiert).
        let mut doc = narsil_dom::Document::new();
        let leer = doc
            .push_child(
                doc.root(),
                narsil_dom::NodeKind::Text,
                Some(String::new()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Textknoten anlegbar");
        let ctx = Ctx::neu(&doc, &messer, None);
        let node = doc.node(leer).expect("Knoten liegt vor");
        let groesse = intrinsic_size(&ctx, node).expect("leerer Text ist kein Fehler");
        assert_eq!(groesse.width, 0.0, "Breite 0 = leerer Text");
        // added 04.10.2026, SERVER_12: war `16.0`, ich zog es auf `19.0` —
        // und die Entscheidung der Leitung vom 05.10. (SERVER_13) hat das
        // **zurueckgenommen**. Dieser Test benutzt **`BitmapMeasurer`**, und
        // die eingebaute 7-Zeilen-Bitmap hat ihre **eigene** Metrik von
        // 1,0 em; in `narsil-text` gemessen (`measure`: `height: font_size`).
        // Die Regel aus `tests/zeilenhoehe_normal.rs` gilt fuer **echte**
        // Schriften (`TtMeasurer`, `MessungMesser`), nicht fuer die Bitmap.
        assert_eq!(groesse.height, 16.0, "Zeile bleibt (Bitmap: 1,0 em)");

        // Gefuellter Text: dieselbe Zahl, aber ein anderes Ergebnis.
        let (doc, id) = textknoten_mit(16.0);
        let ctx = Ctx::neu(&doc, &messer, None);
        let node = doc.node(id).expect("Knoten liegt vor");
        let groesse = intrinsic_size(&ctx, node).expect("gueltige Groesse misst");
        assert!(
            groesse.width > 0.0,
            "Breite > 0, bekommen {}",
            groesse.width
        );
    }

    /// TASK_0025, AUFGABE 2 — der REST der Falle: die Groesse bestand die
    /// Pruefung oben (endlich, > 0), gemessen wurde trotzdem nichts.
    ///
    /// GEMESSEN, bevor diese Pruefung existierte: font-size 5000 ergab eine
    /// Border-Box `0.0x5000.0` — stillschweigend Breite 0, weil
    /// `narsil_text::measure` `InvalidFontSize` zurueckgab
    /// (MAX_FONT_SIZE = 4096) und der Messer daraus `0.0` machte. Genau die
    /// Verwechslung, um die es hier geht: Fehlen eines Ergebnisses gegenueber
    /// dem Ergebnis der Breite 0.
    #[test]
    fn intrinsic_size_liefert_kein_null_fuer_nicht_leeren_text() {
        let messer = BitmapMeasurer::new();
        let (doc, id) = textknoten_mit(5000.0);
        let ctx = Ctx::neu(&doc, &messer, None);
        let node = doc.node(id).expect("Knoten liegt vor");
        match intrinsic_size(&ctx, node) {
            Ok(size) => panic!("font-size 5000 muss einen LayoutError ergeben, nicht {size:?}"),
            Err(LayoutError::InvalidLength {
                node: gefunden,
                detail,
            }) => {
                assert_eq!(gefunden, id, "der Fehler nennt den Textknoten");
                assert!(
                    detail.contains("5000"),
                    "der Fehler nennt die Schriftgroesse: {detail}"
                );
            }
            Err(other) => panic!("erwartet InvalidLength, bekommen {other:?}"),
        }
    }
}
