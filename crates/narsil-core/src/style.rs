//! Stile eines Dokumentknotens.
//!
//! `Style` ist der DEKLARIERTE Stil, wie er im Dokument steht. Eine spaetere
//! Cascade-Berechnung (Vererbung, Spezifitaet) ist nicht Teil von MVP 0.

// `Color` wird HIER re-exportiert, weil `Style` und `SchattenArt` Felder
// davon haben. Wer `narsil_core::style::SchattenArt` benutzt, MUSS `Color`
// benennen koennen, um den Wert zu pruefen oder zu setzen.
//
// GEMESSEN 03.10.2026: der Import war privat (`use crate::color::Color;`),
// und ein Aufrufer in crates/narsil/src/sitzung.rs:4739 bekam daraufhin
// "E0603: struct `Color` is private" bei `narsil_core::style::Color::rgb(...)`.
// Der Aufrufer hat den richtigen Pfad NENNT, weil er `Color` braucht —
// also ist der Weg falsch, nicht der Aufrufer.
//
// Deshalb `pub use`: der Typ wird von aussen benennbar, ohne dass sich
// etwas an `Style` oder `SchattenArt` aendert.
pub use crate::color::Color;
use crate::length::{Dimension, Spacing};

/// Textausrichtung (CSS Text 3 §3.1).
///
/// Entspricht `narsil_css::Textausrichtung` und `narsil_layout::plan::Textausrichtung`.
/// Definiert hier in `narsil-core`, damit `Style` sie ohne zusaetzliche
/// Abhaengigkeiten tragen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Textausrichtung {
    /// Links (Anfangswert, und `start` bei `ltr`).
    #[default]
    Links,
    /// Rechts (`end` bei `ltr`, `start` bei `rtl`).
    Rechts,
    /// Zentriert.
    Zentriert,
    /// Blocksatz.
    Blocksatz,
}

/// Weissraum-Behandlung (CSS Text 3 §3.1 / `white-space`).
///
/// Entspricht `narsil_css::Weissraum`. Definiert hier in `narsil-core`,
/// damit `Style` sie ohne zusaetzliche Abhaengigkeiten tragen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weissraum {
    /// Umbruch nach Bedarf, Folgen schrumpfen (Anfangswert).
    #[default]
    Normal,
    /// Wie gesetzt lassen (pre).
    Pre,
    /// Nie umbrechen (nowrap).
    KeinUmbruch,
    /// Wie gesetzt lassen, zusaetzlich umbrechen (pre-wrap).
    PreUmbruch,
    /// Folgen schrumpfen, umbrechen bei Zeilenende (pre-line).
    PreZeile,
    /// `break-spaces` (CSS Text 3 §3.1) — wie `pre-wrap`, aber jede
    /// Folge erhaelt einen Platz fuer Umbruch.
    UmbruchLeer,
}

/// Release 15.10.2026 (Agent CSS2): soft wrap opportunities INSIDE words
/// (CSS Text 3 §5.2 `word-break`, §5.3 `line-break`). Unlike `overflow-wrap`
/// (an emergency break, only when a word does not fit on a line of its own)
/// these are ordinary break opportunities: the line takes as many letters as
/// fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LetterBreaks {
    /// Only the opportunities of `word-break: normal`.
    #[default]
    None,
    /// `word-break: break-all`: letters and digits (UAX #14 AL, HL, NU, SA)
    /// break like ideographs; punctuation and glue keep their rules.
    BreakAll,
    /// `line-break: anywhere`: around every typographic character unit,
    /// including punctuation, glue and preserved spaces.
    Anywhere,
}

/// Art der Zeilenhoehe (CSS 2.1 §10.8 / `line-height`).
///
/// Entspricht `narsil_css::ZeilenhoeheArt`. Definiert hier in `narsil-core`,
/// damit `Style` sie ohne zusaetzliche Abhaengigkeiten tragen kann.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZeilenhoeheArt {
    /// `normal` — der Browser waehlt (ca. 1.2em).
    Normal,
    /// Einheitenloser Faktor (z.B. `1.5`).
    Faktor(f32),
    /// Absolute Laenge in Pixeln (berechnet).
    Absolut,
}

impl Default for ZeilenhoeheArt {
    fn default() -> Self {
        ZeilenhoeheArt::Normal
    }
}

/// Standard-Schriftgroesse in Pixeln.
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
/// Standardfarbe fuer Text.
pub const DEFAULT_TEXT_COLOR: Color = Color::WHITE;
/// Standard-Hintergrund: transparent.
pub const DEFAULT_BACKGROUND: Color = Color::TRANSPARENT;

/// Anordnungsrichtung der Kind-Elemente eines Containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Standard: von oben nach unten.
    #[default]
    Column,
    /// Von links nach rechts.
    Row,
}

/// M3: hoechste Zahl `box-shadow`-Eintraege je Knoten.
///
/// GEMESSEN 03.10.2026: `narsil_css::MAX_SCHATTEN` ist 8, und mehr als acht
/// Schatten fallen dort GANZ weg (nicht abschneiden). Der Rasterizer
/// braucht dieselbe Grenze, sonst koennte er mehr zeichnen als die Kaskade
/// berechnet hat.
pub const MAX_SCHATTEN: usize = 8;

/// Ein Schatten, wie ihn der Rasterizer braucht.
///
/// WARUM EIN EIGENER TYP und nicht `narsil_css::Schatten`: die
/// Abhaengigkeitsrichtung laeuft von `narsil-css` zu `narsil-core`, nicht
/// umgekehrt. Und `Style` ist `Copy` — ein `Vec` (wie in `Berechnet`)
/// wuerde das brechen. Also ein fester Array mit Zaehler, und die Felder
/// sind bitgleich zu `narsil_css::Schatten`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SchattenArt {
    /// Farbe des Schattens (Alpha inbegriffen).
    pub farbe: Color,
    /// Versatz in x, in px (CSS Backgrounds 3 §9.1: zwei Laengen).
    pub x: f32,
    /// Versatz in y, in px.
    pub y: f32,
    /// Weichzeichnen (`blur`) in px. 0 = harte Kante.
    ///
    /// GEMESSEN: CSS Backgrounds 3 §5.3 definiert `blur` als die
    /// STANDARDABWEICHUNG des Gauss, nicht als seinen 2-Sigma-
    /// Durchmesser. Wer das als Radius nimmt, bekommt zu weich.
    pub weich: f32,
    /// Ausbreitung (`spread`) in px, nach aussen, VOR dem Gauss.
    pub ausbreitung: f32,
    /// `inset`: hinter dem Inhalt, vor dem Hintergrund (§9.2).
    pub innen: bool,
}

impl Default for SchattenArt {
    fn default() -> Self {
        SchattenArt::LEER
    }
}

impl SchattenArt {
    /// Ein Schatten, der NICHTS zeichnet: transparente Farbe, kein
    /// Versatz, keine Weichzeichnung.
    ///
    /// GEMESSEN 03.10.2026: die Fuellflaeche von `[SchattenArt; MAX_SCHATTEN]`
    /// ist aus Fuellzwecken mit diesem Wert belegt. Ohne eine benannte
    /// Konstante muss jeder Aufrufer die sechs Felder von Hand schreiben,
    /// und ein Aufrufer, der es vergisst, erzeugt einen schwarzen Schatten
    /// an jeder Stelle, an der eigentlich keiner sein sollte. Ein
    /// `Default` allein reicht dafuer nicht: es gibt nichts, was man
    /// fuellen koennte, ohne `Default` aufzurufen.
    pub const LEER: SchattenArt = SchattenArt {
        farbe: Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        x: 0.0,
        y: 0.0,
        weich: 0.0,
        ausbreitung: 0.0,
        innen: false,
    };
}

/// Deklarierter Stil eines Knotens.
///
/// GEMESSEN 04.10.2026 (SCHRIFT): `Style` war bis dahin `Copy`, und
/// `schriftfamilie: String` (der `font-family`-Wunsch, siehe dort) macht
/// das **unmoeglich**.
///
/// GEMESSEN, warum `String` und keine kleinere Form: CSS Fonts 4 §4.2
/// nennt als `font-family` eine **Liste** beliebiger Namen
/// (`"Helvetica Neue", Arial, sans-serif`), und jeder Name kann ein
/// fremdes Wort sein. Eine feste Byte-Zahl muesste abschneiden — und ein
/// abgeschnittener Familienname waehlt still die **falsche** Schrift.
///
/// GEMESSEN, warum eine Nummer (ein Index in eine Tabellentabelle des
/// Parsers) nicht geht: `Style` wird auch von Hand gebaut (die
/// Vorgabe-Literale in `narsil-page`), und eine Tabelle muesste dann an
/// jeder dieser Stellen mitgegeben werden.
///
/// GEMESSEN, was das kostet: `Style` wird an vielen Stellen **kopiert**
/// (`node.style` in einem Ausdruck, `*stil` in einem Muster). Ohne
/// `Copy` sind das jetzt Aufrufe von `.clone()` — der Compiler zeigt sie
/// einzeln, und es sind **keine** Logikentscheidungen.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Breite und Hoehe des Containers.
    pub size: Dimension,
    /// Aeusserer Abstand zum Eltern-Container.
    pub margin: Spacing,
    /// Innerer Abstand zum Inhalt.
    pub padding: Spacing,
    /// Fuellfarbe. Transparent bedeutet "nicht zeichnen".
    pub background: Color,
    /// Textfarbe.
    pub color: Color,
    /// Schriftgroesse in Pixeln.
    pub font_size: f32,
    /// Richtung der Kind-Elemente (nur fuer Container).
    pub direction: Direction,
    /// Textdekoration als Bitmaske (A3, 02.10.2026): 1 = `underline`,
    /// 2 = `overline`, 4 = `line-through`. 0 = keine.
    ///
    /// Die Bits sind bitgleich zu `narsil_css::Berechnet::dekoration`, damit
    /// der Wert ohne Umschreiben durchgereicht werden kann.
    ///
    /// GEMESSEN: dieses Feld zu ergaenzen war der teuerste Schritt der
    /// ganzen Kette — 131 erschoepfende `Style { .. }`-Literale in neun
    /// Crates (davon 57 allein in `narsil-layout-engine/tests/layout.rs`).
    /// `Style` hat bereits ein HANDGESCHRIEBENES `impl Default` (nicht
    /// abgeleitet, weil die Vorgabewerte benannt sind) — das ist die EINZIGE
    /// Stelle, die ein neues Feld kennen muss, und sie steht gleich hier.
    /// Wer kuenftig ein Feld ergaenzt, hat es mit dieser Liste zu tun.
    ///
    /// `overline` (Bit 2) wird vom Rasterizer gezeichnet zu NICHTS: es
    /// gibt dafuer keine geprueften Font-Metriken, und eine erfundene Linie
    /// ist schlimmer als eine fehlende.
    pub dekoration: u8,
    /// `font-weight`, 100..=900 (CSS Fonts 4 §5.3). Vorgabe **400**.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT), warum dieses Feld noetig war: an
    /// `texttest.html` gegen Firefox kamen **alle vier** Stilzeilen gleich
    /// an — Firefox 127 / 144 / 129 / 143 px, narsil 125 / 125 / 125 / 125.
    /// Ueberschriften, `<b>`, `<strong>`, `<em>`, `<th>` — der ganze Text
    /// einer Seite wurde aufrecht gezeichnet.
    ///
    /// GEMESSEN, dass es nicht an der Kaskade lag: `narsil_css::Berechnet`
    /// fuehrt `schriftgewicht` (typen.rs:1852) mit Vererbung. Es fehlte der
    /// Weg von dort bis hier — und ab hier sieht der Layout-Auftrag
    /// **nur** `node.style`.
    ///
    /// GEMESSEN, dass der Zeichenweg es umsetzen kann: `Schrift::
    /// mit_gewicht(700, false)` liefert GEMESSEN `DejaVuSans-Bold.ttf`
    /// (`crates/narsil-raster/examples/a49_stil_bild.rs`).
    ///
    /// GEMESSEN, dass die Vorgabe **400** ist und nicht 0: CSS Fonts 4
    /// §5.3 nennt 400 als `normal`, und 0 wuerde jeden Schnitt als
    /// unerfuellt melden — dieselbe Falle wie bei `SchriftWunsch::normal`.
    pub schriftgewicht: u16,
    /// `font-style: italic` oder `oblique` (CSS Fonts 4 §5.2).
    /// Vorgabe `false` = `normal`.
    ///
    /// GEMESSEN wie bei `schriftgewicht`: die kursive Zeile war in narsil
    /// 125 px breit und damit **so breit wie die normale** — aufrecht.
    pub kursiv: bool,
    /// `font-family` als **CSS-Liste** (CSS Fonts 4 §4.2), Vorgabe
    /// `sans-serif`.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT), warum das Feld noetig war: an
    /// `schrift_uablatt.html` stehen `code`, `kbd` und `samp` bei
    /// GEMESSEN 14,4 px **in `DejaVuSans.ttf`** — also in der
    /// Fliesstextschrift. Das UA-Blatt sagt fuer sie `monospace`, die
    /// Kaskade rechnet es (`Berechnet::schriftfamilie`), und es geht
    /// trotzdem verloren: `Style` hatte **kein** Feld fuer die Familie
    /// (`grep -n schriftfamilie crates/narsil-core/src/style.rs` findet
    /// nichts), also konnte weder der Umbruch noch der Raster sie sehen.
    ///
    /// GEMESSEN, warum die Liste als **String** und nicht als `Vec<String>`
    /// kommt: die Kaskade liefert sie als Wort (parse.rs:7429
    /// `schriftfamilie_wert`), und ein `Vec` muesste an jeder Stelle neu
    /// gespalten und neu gejoint werden. Die Liste ist die **Reihenfolge**,
    /// und CSS Fonts 4 §4.2 sagt: der erste Eintrag, fuer den es eine
    /// Schrift gibt, gewinnt.
    ///
    /// GEMESSEN, warum die Vorgabe `sans-serif` ist und nicht `""`:
    /// CSS Fonts 4 §4.2 nennt die generische Familie als Auffang; ein
    /// leeres Feld waere „keine Angabe", und der Raster muesste dann
    /// selbst raten.
    pub schriftfamilie: String,
    /// Release 11.10.2026 (Agent CSS2): `vertical-align: <length> |
    /// <percentage> | super | sub` als Hebung der Grundlinie in px (positiv
    /// = nach oben, CSS 2.1 §10.8.1), fuer Text bereits ueber die
    /// Inline-Vorfahren aufsummiert. 0 = `baseline`.
    pub vertikal_hoch: f32,
    /// Release 11.10.2026 (Agent CSS2): `letter-spacing` und `word-spacing`
    /// in px (CSS 2.1 §16.4): zusaetzlicher Abstand nach jedem Zeichen bzw.
    /// nach jedem Leerzeichen. 0 = `normal`.
    pub buchstaben_abstand: f32,
    pub wort_abstand: f32,
    /// `text-align` (Anfangswert links, vererbt).
    ///
    /// Wird in `narsil-page/src/kaskade.rs` aus `Berechnet::textausrichtung`
    /// uebernommen. Die Layout-Engine (`narsil-layout-engine/src/flow.rs`)
    /// liest sie ueber den Plan (`narsil_layout::plan::Textausrichtung`).
    pub textausrichtung: Textausrichtung,
    /// `white-space` (Anfangswert normal, vererbt).
    ///
    /// Wird in `narsil-page/src/kaskade.rs` aus `Berechnet::weissraum`
    /// uebernommen. Die Textmessung und der Umbruch in
    /// `narsil-layout-engine/src/text.rs` beruecksichtigen sie.
    pub weissraum: Weissraum,
    /// Release 11.10.2026: `line-break: loose`/`anywhere` (CSS Text 3 §5.2,
    /// vererbt) — lockerere Umbruchregeln fuer CJK (vor Iterationszeichen,
    /// dem Mittelpunkt, den Bindestrichen). Gesetzt in `narsil-page`.
    pub umbruch_locker: bool,
    /// Release 11.10.2026: `word-break: break-all`, `overflow-wrap:
    /// anywhere|break-word` oder `line-break: anywhere` — an den Grenzen
    /// dieses Textes darf umbrochen werden, auch ohne Leerraum.
    pub umbruch_ueberall: bool,
    /// Release 15.10.2026 (Agent CSS2): see [`LetterBreaks`] (inherited).
    pub letter_breaks: LetterBreaks,
    /// Release 15.10.2026 (Agent CSS2): `overflow-wrap: break-word` (or
    /// `word-wrap: break-word`) without any value that also changes the
    /// min-content size (`overflow-wrap: anywhere`, `word-break: break-word|
    /// break-all`, `line-break: anywhere`) — CSS Text 3 §5.5: its emergency
    /// breaks "are not considered when calculating min-content intrinsic
    /// sizes".
    pub break_word_only: bool,
    /// Release 15.10.2026 (Agent CSS2): the least min-content width of a
    /// text run (the widest whole word of its section when `break_word_only`;
    /// the page builder sets it, because an emergency split cuts the word
    /// into runs that are narrower than the word). 0 = none.
    pub min_content_floor: f32,
    /// Release 15.10.2026 (Agent CSS2): `hyphens: none` — a soft hyphen
    /// (U+00AD) is no break opportunity (CSS Text 3 §6.1; `manual`, the
    /// initial value, breaks there and shows a hyphen).
    pub hyphens_none: bool,
    /// Release 15.10.2026 (Agent CSS2): `hyphenate-character` (CSS Text 4
    /// §6.4): the string shown at a hyphenation break; `None` = `auto` ("-").
    pub hyphenate_character: Option<String>,
    /// Release 11.10.2026: die Inhaltssprache (`lang`, vererbt) ist
    /// Chinesisch, Japanisch oder Koreanisch — die sprachabhaengigen
    /// Umbruchregeln (CSS Text 3 §5.2) gelten.
    pub cjk_sprache: bool,
    /// `line-height` in Pixeln (berechneter Wert, vererbt).
    ///
    /// Wird in `narsil-page/src/kaskade.rs` aus `Berechnet::zeilenhoehe_px`
    /// uebernommen. `zeilenhoehe_art` sagt, ob es ein Faktor, `normal`
    /// oder ein absoluter Wert war.
    pub zeilenhoehe_px: f32,
    /// Art der Zeilenhoehe: `normal`, Faktor (einheitenlos), oder absolut.
    ///
    /// Wird in `narsil-page/src/kaskade.rs` aus `Berechnet::zeilenhoehe_art`
    /// uebernommen. Noetig fuer korrekte Vererbung (einheitenlose Faktoren
    /// werden vererbt, nicht der Pixelwert).
    pub zeilenhoehe_art: ZeilenhoeheArt,
    /// `opacity` als Faktor 0..1 (RUNDE 3 S2, 03.10.2026). 1 = deckend.
    ///
    /// CSS Color 4 §3.2: die Eigenschaft wird NICHT geerbt, wirkt aber auf
    /// alle Kinder. Deshalb steht sie am Knotenstil und NICHT am geerbten
    /// Wert — ein Kind mit `opacity: 1` in einem Elternteil mit 0,5 ist
    /// halb durchsichtig.
    ///
    /// GEKLEMMT, nicht abgewiesen: `opacity: 2` ergibt 1, `-1` ergibt 0.
    /// CSS Color 4 §3.2 sagt ausdruecklich "clamped".
    pub deckkraft: f32,
    /// Release 10.10.2026: `visibility: hidden` (CSS 2.1 §11.2) — der Kasten
    /// belegt seinen Platz, gezeichnet wird von IHM nichts (Hintergrund,
    /// Rahmen, Text, Bild). Der Wert ist vererbt; ein Kind mit `visibility:
    /// visible` zeichnet sich selbst. Vorher wurde ein verstecktes Element
    /// gar nicht gebaut (wie `display: none`) und verlor seinen Platz.
    pub unsichtbar: bool,
    /// `border-radius` in px: oben-links, oben-rechts, unten-rechts,
    /// unten-links (die Reihenfolge aus `Berechnet::radius`).
    ///
    /// CSS Backgrounds 3 §5.5: die Werte werden auf `min(b,h)/2`
    /// BEGRENZT und bei gemischten Ecken PROPORTIONAL VERKLEINERT, nicht
    /// beschnitten. Das macht der Rasterizer
    /// (`narsil_raster::formen::radius_form`); hier steht der Rohwert.
    pub radius: [f32; 4],
    /// Release 15.10.2026 (Agent F): elliptical corners (CSS Backgrounds 3
    /// §5.1): per corner `[horizontal, vertical]` in px or, where
    /// `radius_percent` has the bit (bit i: horizontal of corner i, bit
    /// 4 + i: vertical), in percent of the border box width or height. Used
    /// for a corner whose `radius` is not 0 (see [`Style::corner_radii`]).
    pub radius_xy: [[f32; 2]; 4],
    pub radius_percent: u8,
    /// `border` — **BILD_32: es gab das Feld nicht.** `narsil-css` parst
    /// `border`/`-width`/`-style`/`-color` (`parse.rs:5406-5414`) und die Werte
    /// gingen an dieser Stelle **vorbei**. Siehe [`Rahmen`].
    pub rahmen: Rahmen,
    /// `box-shadow`, in CSS-REIHENFOLGE: der ERSTE liegt OBEN
    /// (CSS Backgrounds 3 §9.3). Hoechstens [`MAX_SCHATTEN`].
    pub schatten: [SchattenArt; MAX_SCHATTEN],

    /// **BILD_48, Station 1: `background-image: linear-gradient(...)` und
    /// Verwandtes.**
    ///
    /// **GEMESSEN, warum das Feld noetig ist:** vor diesem Feld gab es in
    /// `Style` **null** Treffer auf `verlauf` — es gab also **keinen Platz**, einen
    /// Verlauf zu tragen, und `narsil_page::kaskade::verlauf_von_css` (BILD_44,
    /// Station 2) hatte **null Aufruforte**. Ein Typ ohne Ort ist kein Weg.
    ///
    /// **`None` heisst „kein Verlauf"** — und das ist der **Normalfall**:
    /// `background-image: none` ist der Anfangswert (CSS Backgrounds 3 §2.2).
    /// `Some` heisst: **ein** Verlauf, getragen bis zum Rasterizer
    /// (`DisplayItem::Verlauf`, Station 3).
    ///
    /// **WARUM der Typ aus `verlauf_typ` und nicht aus `narsil_css`:** die Crate
    /// haengt in **keiner** Richtung an `narsil-css` (das steht in
    /// `crates/narsil-raster/Cargo.toml`), und `narsil-core` ist die Crate, die
    /// **alle** teilen.
    pub verlauf: Option<crate::verlauf_typ::Verlauf>,
    /// Release 13.10.2026 (Agent CSS2): Groesse, Lage und Wiederholung des
    /// Verlaufs (`background-size`/`-position`/`-repeat`). `None`: die
    /// Anfangswerte — der Verlauf fuellt den Kasten wie bisher.
    pub verlauf_kachel: Option<crate::verlauf_typ::Kachel>,
    /// Release 13.10.2026 (Agent CSS2): `overflow: clip` (oder `contain:
    /// paint`) mit `overflow-clip-margin` — die Kante (0 `padding-box`, 1
    /// `border-box`, 2 `content-box`) und der Abstand in px (CSS Overflow 4
    /// §3.1). `None`: kein solcher Clip (der Ueberlauf schneidet, falls
    /// ueberhaupt, an der Padding-Box).
    pub clip_rand: Option<(u8, f32)>,
    /// Release 13.10.2026 (Agent CSS2): `outline` — Breite in px und Farbe
    /// des Rings ausserhalb der Rahmenkante (CSS UI 4 §6). `None`: keiner.
    pub umriss: Option<(f32, crate::color::Color)>,
    /// Release 14.10.2026 (Agent CSS2): a `border-image` replaces the painted
    /// border (CSS Backgrounds 3 §6); the border still takes up space.
    pub border_image: bool,
    /// Release 15.10.2026 (Agent F): the computed `clip-path` value (a basic
    /// shape and/or a reference box), `None` for `none` (CSS Masking 1 §5).
    pub clip_pfad: Option<std::sync::Arc<str>>,
    /// Release 15.10.2026 (Agent F): for `clip-path: url(#id)`, an SVG
    /// document holding the referenced `<clipPath>` of the page.
    pub clip_svg: Option<std::sync::Arc<str>>,
    /// Release 15.10.2026 (Agent F): the box has a `mask-image` (it masks
    /// its subtree and makes a stacking context, CSS Masking 1 §7). The
    /// value is the element's key: the copies of its style on its text
    /// share it (the text of an inline element without a box of its own
    /// carries the element's mask).
    pub maske_bild: Option<std::sync::Arc<str>>,
    /// Release 15.10.2026 (Agent F): the computed `filter` value (not
    /// `none`) and, for `url()`, the page's `<filter>` elements as an SVG
    /// document.
    pub filter_value: Option<std::sync::Arc<str>>,
    pub filter_svg: Option<std::sync::Arc<str>>,
    /// Release 15.10.2026 (Agent F): the computed `backdrop-filter` value
    /// (not `none`) and, for `url()`, its `<filter>` elements as an SVG
    /// document (Filter Effects 2 §3).
    pub backdrop_filter: Option<std::sync::Arc<str>>,
    pub backdrop_svg: Option<std::sync::Arc<str>>,
    /// Wie viele Eintraege in `schatten` wirklich belegt sind.
    pub schatten_anzahl: u8,
}

/// `border-style` (CSS Backgrounds 3 §4.2) — **alle fuenf Stile**, und die
/// Liste sagt ausdruecklich, dass **`hidden` wie `none` behandelt wird**.
///
/// **BILD_32, GEMESSEN: `border-radius` FUNKTIONIERT, der Rahmen NICHT.** Vor
/// diesem Typ hat `narsil_core::style::Style` **kein einziges Rahmenfeld** gehabt,
/// `narsil-css` hat `border`, `border-width`, `border-style` und `border-color`
/// aber **alle vier geparst** (`parse.rs:5406`, `:5407`, `:5413`, `:5414`) — die
/// Werte waren also da und wurden verworfen. Das betrifft Eingabefelder,
/// Knoepfe, Karten, Tabellen, Tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RahmenStil {
    /// `none` und `hidden` — **kein** Rahmen, und die Breite zaehlt nicht.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `hidden` hier auf `none` liegt:**
    /// CSS Backgrounds 3 §4.2 sagt woertlich „`hidden` ist dasselbe wie `none`",
    /// und der Unterschied ist nur bei Tabellen sichtbar. Ein eigener Wert
    /// wuerde eine zweite Bedeutung in die Kaskade bringen, die es dort nicht
    /// gibt — und die Faelle sind **Tabellen**, nicht Bilder.
    #[default]
    Keiner,
    /// `solid` — durchgezogen. Der Vorgabewert, wenn eine Breite gesetzt ist.
    Durchgezogen,
    /// `dashed` — gestrichelt, in **Browser-Kacheln**, nicht in Zeilen. Firefox
    /// zeichnet die Striche an den **Ecken** verkürzt.
    Gestrichelt,
    /// `dotted` — gepunktet. Firefox zeichnet **Quadrate**, keine runden
    /// Punkte, wenn die Breite 1 px ist.
    Gepunktet,
    /// `double` — zwei Linien mit einem Zwischenraum. **Die Breite wird auf
    /// mindestens 3 px gebracht**, sonst bleibt nur eine Linie (CSS Backgrounds
    /// 3 §4.4).
    Doppelt,
    /// `groove` — **zwei Linien**, die aussen eingedrueckt wirken. Wird wie
    /// `ridge` mit vertauschten Haelften gezeichnet (CSS Backgrounds 3 §4.5).
    Furche,
    /// `ridge` — zwei Linien, die aussen erhaben wirken.
    Wulst,
    /// `inset` — **zwei Linien**, die innen eingedrueckt wirken.
    Innen,
    /// `outset` — **zwei Linien**, die aussen erhaben wirken. Wird wie
    /// `groove` mit vertauschten Haelften gezeichnet.
    Aussen,
}

/// `border` — **je Kante**, weil CSS die vier Seiten getrennt behandelt.
///
/// **Feldreihenfolge ist die CSS-REIHENFOLGE**: oben, rechts, unten, links —
/// **nicht** im Uhrzeigersinn. Das steht hier, weil ein Ring aus vier Seiten in
/// einer anderen Reihenfolge gebaut wird als die, in der sie im Bild steht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rahmen {
    /// `border-width` in px, je Kante.
    pub breite: [f32; 4],
    /// `border-style` je Kante.
    pub stil: [RahmenStil; 4],
    /// `border-color` je Kante, **`currentColor` bereits aufgeloest**.
    pub farbe: [Color; 4],
}

impl Default for Rahmen {
    /// **DER VORGABEWERT IST „KEIN RAHMEN"**, und das ist nicht nur eine
    /// Bequemlichkeit: `border-style: none` heisst, dass die **Breite nicht
    /// gilt** (CSS Backgrounds 3 §4.1). Wer `breite: 0` als Default nimmt und
    /// eine Breite aus dem Stil uebernimmt, baut einen Rahmen, den die Seite
    /// nicht asked hat.
    fn default() -> Self {
        Self {
            breite: [0.0; 4],
            stil: [RahmenStil::Keiner; 4],
            farbe: [Color::BLACK; 4],
        }
    }
}

impl Rahmen {
    /// **GEMESSENE HILFE:** ist an irgendeiner Kante ein Rahmen zu zeichnen?
    ///
    /// `none` und `hidden` zaehlen **nicht**, auch wenn eine Breite dasteht —
    /// das ist der Unterschied zwischen „Breite angegeben" und „Rahmen da".
    pub fn ist_gezeichnet(&self) -> bool {
        (0..4).any(|i| self.stil[i] != RahmenStil::Keiner && self.breite[i] > 0.0)
    }

    /// Die **Breite einer Kante**, die tatsaechlich gezeichnet wird: `0`, wenn
    /// der Stil `none` ist. Der Rasterizer fragt das, damit er nicht Rahmen mit
    /// Breite 0 und noch einem Radius malt.
    pub fn gezeichnete_breite(&self, kante: usize) -> f32 {
        if kante < 4 && self.stil[kante] != RahmenStil::Keiner && self.breite[kante] > 0.0 {
            self.breite[kante]
        } else {
            0.0
        }
    }
}

impl Style {
    /// Release 15.10.2026 (Agent F): the corner radii of a border box of
    /// `width` x `height`: horizontal and vertical radius per corner (CSS
    /// order), percentages resolved. Without elliptical radii both are
    /// `radius`.
    #[must_use]
    pub fn corner_radii(&self, width: f32, height: f32) -> ([f32; 4], [f32; 4]) {
        let mut x = self.radius;
        let mut y = self.radius;
        if self.radius_percent == 0 && self.radius_xy == [[0.0; 2]; 4] {
            return (x, y);
        }
        for i in 0..4 {
            let has_xy = self.radius_xy[i] != [0.0; 2] || self.radius_percent & ((1 << i) | (1 << (4 + i))) != 0;
            if self.radius[i] <= 0.0 || !has_xy {
                continue;
            }
            let [h, v] = self.radius_xy[i];
            x[i] = if self.radius_percent & (1 << i) != 0 { width.max(0.0) * h / 100.0 } else { h };
            y[i] = if self.radius_percent & (1 << (4 + i)) != 0 { height.max(0.0) * v / 100.0 } else { v };
        }
        (x, y)
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            size: Dimension::AUTO,
            margin: Spacing::ZERO,
            padding: Spacing::ZERO,
            background: DEFAULT_BACKGROUND,
            // BILD_32: der Vorgabewert ist **kein Rahmen**, nicht `breite: 0`.
            rahmen: Rahmen::default(),
            color: DEFAULT_TEXT_COLOR,
            font_size: DEFAULT_FONT_SIZE,
            direction: Direction::default(),
            dekoration: 0,
            // GEMESSEN 04.10.2026: 400 ist `normal` (CSS Fonts 4 §5.3).
            // Mit 0 wuerde `Schrift::mit_gewicht` jeden Schnitt als
            // unerfuellt melden und der Text kaeme kuenstlich dick heraus.
            schriftgewicht: 400,
            kursiv: false,
            // GEMESSEN 04.10.2026: `sans-serif` ist die generische
            // Auffangfamilie (CSS Fonts 4 §4.2) — und `fc-match` liefert
            // dafuer auf dieser Maschine `DejaVuSans.ttf`, also ist die
            // Vorgabe **keine** leere Liste, sondern die Familie, die
            /// `font-family` ohne Angabe zeigen muss.
            schriftfamilie: String::from("sans-serif"),
            vertikal_hoch: 0.0,
            buchstaben_abstand: 0.0,
            wort_abstand: 0.0,
            // Text-Eigenschaften (Anfangswerte laut CSS 2.1 / CSS Text 3).
            textausrichtung: Textausrichtung::Links,
            weissraum: Weissraum::Normal,
            umbruch_locker: false,
            umbruch_ueberall: false,
            letter_breaks: LetterBreaks::None,
            break_word_only: false,
            min_content_floor: 0.0,
            hyphens_none: false,
            hyphenate_character: None,
            cjk_sprache: false,
            // zeilenhoehe_normal aus narsil-text (hier inline, um Abhaengigkeit zu vermeiden):
            // 1.200195em (OS/2 typo-Metriken) vs. Auf+Ab gerundet, Minimum 1px je Teil.
            zeilenhoehe_px: {
                let fs = DEFAULT_FONT_SIZE;
                let em = (fs * 1229.0 / 1024.0 + 0.5).floor();
                let auf = (fs * 1901.0 / 2048.0).ceil().max(1.0);
                let ab = (fs * 483.0 / 2048.0).ceil().max(1.0);
                em.max(auf + ab)
            },
            zeilenhoehe_art: ZeilenhoeheArt::Normal,
            deckkraft: 1.0,
            unsichtbar: false,
            radius: [0.0; 4],
            schatten: [SchattenArt {
                farbe: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
                x: 0.0,
                y: 0.0,
                weich: 0.0,
                ausbreitung: 0.0,
                innen: false,
            }; MAX_SCHATTEN],
            // **BILD_48, Station 1:** `background-image: none` ist der
            // **Anfangswert** (CSS Backgrounds 3 §2.2), also **kein** Verlauf.
            verlauf: None,
            verlauf_kachel: None,
            clip_rand: None,
            umriss: None,
            border_image: false,
            clip_pfad: None,
            clip_svg: None,
            maske_bild: None,
            filter_value: None,
            filter_svg: None,
            backdrop_filter: None,
            backdrop_svg: None,
            radius_xy: [[0.0; 2]; 4],
            radius_percent: 0,
            schatten_anzahl: 0,
        }
    }
}

impl Style {
    /// `true`, wenn alle Laengen und die Schriftgroesse gueltig sind.
    ///
    /// WICHTIG: `margin` wird NICHT gepruft, weil negative Margins
    /// gueltiges CSS sind (CSS 2.1 8.3.1). Nur `padding`, `width`,
    /// `height` und `font-size` duerfen nicht negativ sein.
    pub fn is_valid(&self) -> bool {
        self.size.is_valid()
            && self.padding.is_valid()
            && self.font_size.is_finite()
            && self.font_size > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_language_spec() {
        let s = Style::default();
        assert_eq!(s.size, Dimension::AUTO);
        assert_eq!(s.margin, Spacing::ZERO);
        assert_eq!(s.padding, Spacing::ZERO);
        assert_eq!(s.background, DEFAULT_BACKGROUND);
        assert_eq!(s.color, DEFAULT_TEXT_COLOR);
        assert_eq!(s.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(s.direction, Direction::Column);
    }

    #[test]
    fn default_is_valid() {
        assert!(Style::default().is_valid());
    }

    #[test]
    fn invalid_font_size_and_lengths_are_detected() {
        let mut s = Style::default();
        s.font_size = 0.0;
        assert!(!s.is_valid());
        s.font_size = f32::NAN;
        assert!(!s.is_valid());
        let mut s = Style::default();
        s.size = Dimension::fixed(10.0, -3.0);
        assert!(!s.is_valid());
    }

    #[test]
    fn direction_default_is_column() {
        assert_eq!(Direction::default(), Direction::Column);
    }
}
