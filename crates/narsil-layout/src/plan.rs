//! DER LAYOUT-PLAN: was die Kaskade weiss, was das Layout braucht.
//!
//! # WARUM EIN PLAN UND KEIN FELD IN `Style`
//!
//! GEMESSEN am 30.09.2026: der einzige Weg von der Kaskade ins Layout
//! ist `narsil_core::style::Style`, und dort stehen nur Groesse, Margin,
//! Padding, Hintergrund, Farbe, Schriftgroesse und Richtung. Kein
//! `display`, kein `flex`, kein `grid`, kein `position`, kein `float`.
//! Deshalb stehen Navigationsleisten als senkrechte Listen und ein
//! `position:absolute`-Badge wird gar nicht gezeichnet (Bildschirmfoto
//! vom 30.09., 19:15: das Grid liegt in drei Zeilen statt in drei
//! Spalten, das Badge fehlt ganz).
//!
//! Der naheliegende Weg waere, `Style` um die Felder zu erweitern. DER
//! IST VERWORFEN, und gemessen begruendet: `Style { … }` steht als
//! Struct-Literal an 25 Stellen in FUENF fremden Crates
//! (narsil-page 21, narsil-fmt-text 2, narsil-dom 1, narsil-parser 2). Ein
//! neues Feld macht jeden dieser 25 Stellen ungueltig — der Build
//! bricht in Crates, die nicht meine sind, und die Rollen, die gerade
//! darin arbeiten, sehen ihren Fortschritt verschwinden.
//!
//! DER WEG, DEN ICH GEHE: ein Plan daneben. [`LayoutPlan`] ist eine
//! Nebentabelle `NodeId -> LayoutStil`. Sie wird von der Kaskade
//! (narsil-page) gefuellt und dem Layout uebergeben; ein Knoten, der
//! nicht im Plan steht, ist ein Block (das ist der Anfangswert, und es
//! ist der Zustand von heute). Damit bricht nichts, und
//! [`crate::LayoutPlan::leer`] ist genau das Verhalten ohne Flex.
//!
//! # SPEICHERREGELN
//!
//! * M3/M4: [`MAX_PLAN_KNOTEN`] begrenzt die Zahl der Eintraege,
//!   [`MAX_RASTER_SPALTEN`] die Spalten je Knoten. Beide werden VOR der
//!   Allokation geprueft, nie danach.
//! * Der Plan ist ein `Vec` mit sortierten Schluesseln und wird per
//!   Binärsuche gelesen — keine `HashMap`, also keine Hash-Bombe und
//!   keine Zufallsordnung.
//! * Keine globalen Zustände, kein `Box::leak`, kein `mem::forget`
//!   (M6). Der Plan gehört dem Aufrufer und faellt mit ihm.

use narsil_dom::NodeId;

// ===========================================================================
// GRENZEN — jede mit Begruendung (M3/M4).
// ===========================================================================

/// Hoechste Zahl der Eintraege im Plan. Begruendung: ein Knoten im Plan
/// ist ein Knoten im Dokument. Die groessten echten Seiten (Wikipedia,
/// GitHub) haben 5 000–20 000 Elemente; 200 000 ist das Zehnfache und
/// haelt die Allokation bei 200 000 x 72 Byte = 14 MiB. Ohne diese Zahl
/// waere `Vec::with_capacity(knoten.len())` aus der Eingabe eine Bombe.
pub const MAX_PLAN_KNOTEN: usize = 200_000;

/// Hoechste Zahl an Knoten mit Beschraenkung (P2, 01.10.2026).
///
/// HALB so gross wie [`MAX_PLAN_KNOTEN`], mit einem Grund: eine Seite,
/// bei der mehr als die Haelfte aller Kaesten eine `min-`/`max-`-Angabe
/// traegt, ist keine Seite, sondern ein Stylesheet mit
/// `* { min-width: 0 }`. Fuer den Normalfall reicht die Haelfte
/// bei weitem.
pub const MAX_GRENZEN: usize = 100_000;

/// Hoechste Spalten je `grid-template-columns`. Begruendung: echte Raster
/// haben ein Dutzend Spalten; 32 laesst viel Luft und haelt die
/// Spaltenrechnung auf einem Stapel, der in den Cache passt.
pub const MAX_RASTER_SPALTEN: usize = 32;

/// Hoechste Flex-Kinder je Flexzeile. Begruendung: eine Navigation hat
/// 2–20 Eintraege; 1024 deckt jede echte ab und haelt die
/// Zeilenschleife kurz. Ueberzaehlige Kinder fallen in eine weitere
/// Zeile (das ist `flex-wrap: wrap` im Default).
pub const MAX_FLEX_KINDER: usize = 1_024;

// ===========================================================================
// ENUMS — dieselben Werte, die die Kaskade berechnet, aber ohne
// Abhaengigkeit von narsil-css (die Kette laeuft sonst im Kreis).
// ===========================================================================

/// `display` in der Form, die das Layout braucht (L1/L2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anzeige {
    /// Normaler Blockfluss (der Fall von heute, der Standard).
    #[default]
    Block,
    /// Textfluss, wie ein `span` ohne Layoutwirksamkeit.
    Inline,
    /// P9 (05.10.2026): `display: inline-block`.
    ///
    /// CSS 2.1, 9.2.1.1: ein Kasten, der AUSSEHEN wie inline ist
    /// (er sitzt im Textfluss, eine Zeile mit den Geschwistern) und sich
    /// wie ein Block verhaelt (er hat eine Breite und eine Hoehe und
    /// bekommt Kinder untereinander).
    ///
    /// GEMESSENER FEHLER, den diese Variante behebt
    /// (`/tmp/opencode/p9/probe.html`):
    ///     .ib { display:inline-block; width:60px; height:30px }
    ///     <div><span class="ib">A</span></div>
    /// Vorher: der Kasten war 11x16 — 11 px ist die GEMESSENE
    /// TEXTBREITE von "A". `width` und `height` kamen nicht an, und es
    /// gab nichts, was einen 11-px-Kasten von einem Inline-Element
    /// unterscheidbar machte: `narsil-page` erzeugt aus einem Inline-Tag
    /// keinen Knoten, und `Anzeige::Inline` wurde wie `Block`
    /// behandelt (narsil-css, `layout_plan.rs:120-134`).
    ///
    /// WAS NOCH FEHLT, damit die Seite es erreicht: `narsil_css::layout_stil`
    /// muss `Anzeige::InlineBlock` hierher abbilden. Es tut es noch
    /// nicht, weil es diese Variante erst seit heute gibt — eine Zeile
    /// bei der Rolle CSS, gemeldet in team/PROBLEME.txt.
    InlineBlock,
    /// `display: flex` (L1).
    Flex,
    /// `display: grid` (L2).
    Raster,
    /// added 05.10.2026, Rolle SERVER (SERVER_13): **DIE TABELLEN-ROLLE.**
    ///
    /// GEMESSENER BEFUND, warum das fehlte: `narsil_css::kaskade` hat
    /// **alle zehn** Tabellenschluesselwoerter gelesen und auf
    /// `Anzeige::Tabelle` abgebildet — und `narsil-css/src/layout_plan.rs:773`
    /// hat daraus `Z::Block` gemacht, weil **dieses Enum keine
    /// Tabellen-Variante hatte**. Es war also nicht „wir haben es nicht
    /// gebaut“, sondern „wir konnten es nicht unterscheiden“. Genau das
    /// steht seit dem 03.10. als offene Luecke in `WUNSCH_CSS.txt` W10.
    ///
    /// **WARUM EINE VARIANTE MIT ROLLE und nicht sieben:** die sieben
    /// Schluesselwoerter (`table`, `table-row`, `table-cell`,
    /// `table-row-group`, `table-header-group`, `table-footer-group`,
    /// `table-caption`, `table-column`, `table-column-group`) beschreiben
    /// **eine Rolle im selben Raster**. Der Leg, der sie braucht, ist einer:
    /// `lege_tabelle` bekommt Zeilen und Zellen und rechnet daraus
    /// Spaltenbreiten und Zeilenhoehen (CSS 2.1, 17.6). Fuenfzehn
    /// Enum-Varianten wuerden eine Genauigkeit vortaeuschen, die es nicht
    /// gibt — der Raster-Leg unterscheidet `thead` von `tbody` nicht.
    ///
    /// `Tabelle` ist der Rahmen (er ruft `lege_tabelle`), `Zeile` und
    /// `Zelle` sind seine Kinder. `Gruppe` (row/header/footer) und
    /// `Spalte` werden wie `Zeile` behandelt: sie sammeln fuer den Rahmen.
    Tabelle,
    /// `display: table-row` (und `table-row-group`,
    /// `table-header-group`, `table-footer-group`).
    ///
    /// added 05.10.2026: `header` und `footer` sind **keine** eigene
    /// Rolle. CSS 2.1, 17.2 spricht bei ihnen ausdruecklich von den
    /// Boxen, die sie erzeugen, nicht von einer Sonderbehandlung; `thead`
    /// ist eine Zeilengruppe wie `tbody`, nur dass sie oben steht. Wer
    /// daraus spaeter etwas macht, muss es messen.
    TabelleZeile,
    /// `display: table-cell`.
    TabelleZelle,
    /// `display: table-caption`, `table-column`, `table-column-group`.
    ///
    /// added 05.10.2026: **bewusst zusammengefasst.** Die Caption ist ein
    /// Block **ueber** oder **unter** der Tabelle, die Spalten-
    /// beschreibung ist gar kein Kasten im Raster (CSS 2.1, 17.2: sie
    /// erzeugt keine Boxen). Der Rahmen braucht beide also nicht als
    /// Raster — was mit ihnen geschieht, steht in `flow.rs` an der
    /// Stelle, die sie behandelt.
    TabelleRand,
    /// `display: none` — wird gar nicht gebaut.
    Keins,
}

/// `flex-direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexRichtung {
    /// `row`: nebeneinander von links nach rechts.
    #[default]
    Reihe,
    /// `column`: untereinander von oben nach unten.
    Spalte,
}

/// `flex-wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexUmbruch {
    /// `nowrap` (Anfangswert): alles in eine Zeile, notfalls ueberlaufend.
    #[default]
    Keiner,
    /// `wrap`: umbrechen, wenn die Zeile voll ist.
    Umbruch,
    /// `wrap-reverse`: wie `wrap`, die Zeilen kommen in umgekehrter
    /// Reihenfolge von unten.
    Umgekehrt,
}

/// `justify-content` — Verteilung AUF der Hauptachse.
///
/// Die sechs Werte sind genau die, die die Kaskade berechnet
/// (`narsil_css::FlexBuendig`). Sechs, nicht fuenf: `space-evenly` gibt es
/// seit CSS Box Alignment und steht dort schon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Buendig {
    /// `flex-start` / `normal`: an den Anfang, Luecke hinten.
    #[default]
    Start,
    /// `center`: mittig, beidseitig gleich viel Luecke.
    Mitte,
    /// `flex-end` / `end`: ans Ende, Luecke vorn.
    Ende,
    /// `space-between`: erster am Anfang, letzter am Ende, dazwischen
    /// verteilt.
    Zwischen,
    /// `space-around`: jeder bekommt links und rechts gleich viel, die
    /// Raender halb so gross wie dazwischen.
    Ringsum,
    /// `space-evenly`: ueberall gleich viel, auch an den Raendern.
    Gleichmaessig,
}

/// `align-items` — Ausrichtung auf der Querachse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ausrichtung {
    /// `flex-start`.
    #[default]
    Start,
    /// `center`.
    Mitte,
    /// `flex-end` / `end`.
    Ende,
    /// `stretch`: die Kinder fuellen die Querachse.
    Strecke,
    /// `baseline` (css-align §6.4) — die Kinder werden nach ihrer
    /// Grundlinie ausgerichtet.
    ///
    /// L4 (03.10.2026 13:5x). GEMESSENER BEFUND, dass die Variante
    /// fehlte: `align-items: baseline` stand 25x in den Korpus-CSS, und
    /// `ausrichtung_bruecke` in `narsil-css` hat `Grundlinie` auf
    /// `Strecke` abgebildet — im Plan kam also nie ein `baseline` an.
    Grundlinie,
}

/// `position` (L3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionArt {
    /// Normalfluss (Anfangswert).
    #[default]
    Statisch,
    /// Versetzt gegen die Normalposition, ohne den Fluss zu aendern.
    Relativ,
    /// Gegen den enthaltenden Block, aus dem Fluss genommen.
    Absolut,
    /// Gegen die Ansicht.
    Fest,
    /// Wie `relative` (dokumentierte Naeherung).
    Klebend,
}

/// `float` (L4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FloatArt {
    /// Kein Float (Anfangswert).
    #[default]
    Keins,
    /// `left`.
    Links,
    /// `right`.
    Rechts,
}

/// `clear` (L4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RaeumungArt {
    /// Kein clear (Anfangswert).
    #[default]
    Keins,
    Links,
    Rechts,
    Beide,
}

/// `overflow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UeberlaufArt {
    /// `visible` (Anfangswert): nichts beschneiden.
    #[default]
    Sichtbar,
    /// `hidden`: beschneiden, kein Rollbalken.
    Versteckt,
    /// `scroll`: beschneiden, Rollbalken immer.
    Rollen,
    /// `auto`: beschneiden, Rollbalken nur bei Bedarf.
    Automatisch,
}

impl UeberlaufArt {
    /// Beschneidet dieser Wert? `hidden`, `scroll`, `auto` — alle drei
    /// beschneiden (CSS 2.1, 11.1.1); nur `visible` nicht.
    pub fn beschneidet(self) -> bool {
        !matches!(self, UeberlaufArt::Sichtbar)
    }
}

/// Eine Spalte im `grid-template-columns` (L2). Dieselbe Form wie in
/// `narsil_css::SpaltenMass`, aber ohne die Abhaengigkeit.
#[derive(Debug, Clone, PartialEq)]
pub enum Spalte {
    /// Feste Breite in px.
    Fest(f32),
    /// `fr`-Anteil am Rest nach allen festen Spalten und den Luecken.
    Anteil(f32),
    /// L4 (03.10.2026 09:3x): die PROZENT-SPUR (§10.1) — der Faktor
    /// wandert mit, weil der Container erst im Layout steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das dieselbe Form wie `Anteil` hat
    /// und nicht wie `Fest`: die Zahl ist relativ, und sie wird erst im
    /// Rasterfluss gegen die Containerbreite gerechnet.
    Prozent(f32),
    /// L4 (02.10.2026 08:4x): `repeat(auto-fill | auto-fit, spur)` —
    /// die Zahl der Wiederholungen steht erst hier im Layout fest, weil
    /// sie von der Containerbreite abhaengt (CSS Grid 1 §7.2.3).
    /// Siehe `narsil_css::typen::SpaltenMass::Auto` fuer den Grund.
    Auto { spur: Box<Spalte>, fit: bool },
    /// L4 (03.10.2026 05:4x): die `auto`-SPUR (CSS Grid 1 §7.2).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie OHNE Zahl durchwandert: die Breite
    /// steht am Kind (`narsil_css::RasterKind::breit`), nicht in der Spur.
    /// Eine Zahl hier waere in der Kaskade gerechnet — und die Kaskade
    /// sieht das Kind nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Name nicht `Auto` ist: `Auto`
    /// ist die WIEDERHOLUNG aus `repeat(auto-fill, spur)` (§7.2.3), und die
    /// traegt ihre Spur in sich. Siehe `narsil_css::typen::SpaltenMass`.
    Inhalt,
    /// `minmax(min, max)` (CSS Grid 1 §7.2.2). Siehe
    /// `narsil_css::typen::SpaltenMass::MinMax`.
    MinMax { min: f32, max: Box<Spalte> },
}

// ===========================================================================
/// Ein Gebietsname aus `grid-template-areas` (CSS Grid 1 §8.1).
///
/// L4 (03.10.2026 02:6x). GEMESSENE ENTSCHEIDUNG, warum ein fester
/// Bytering und kein `String`: `RasterKind` (narsil-css) ist `Copy` und
/// traegt den Namen des KINDES; ein `String` darin waere eine Allokation
/// je Kind. GEMESSEN im Korpus: 41 verschiedene Namen, der laengste
/// `trailingComponent` (17 Zeichen).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein zu langer Name leer bleibt und NICHT
/// abgeschnitten wird: `footer` und `footerBar` sind verschiedene Namen,
/// und ein Abschneiten macht beide gleich — dann legt ein Kind an der
/// Stelle des anderen. Also: passt der Name nicht in den Ring, ist er
/// LEER, und ein leerer Name trifft nie ein Gebiet (§8.3: `auto`).
///
/// GEMESSENE ENTSCHEIDUNG, warum 32 Byte: der laengste gemessene Name
/// hat 17 Zeichen. 32 fasst die Namen mit Reserve, und eine Grenze, die
/// an einem einzigen Korpusnamen ausgemessen waere, waere keine Regel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gebiet(pub [u8; 32]);

impl Gebiet {
    /// Der leere Name — `auto`.
    pub const LEER: Gebiet = Gebiet([0; 32]);

    /// Setzt den Namen aus `s`; passt er nicht, bleibt der Name leer.
    ///
    /// §8.1: „ident-strings" — ein Name ist eine Folge von Buchstaben,
    /// Ziffern und `-`/`_` (GEMESSEN im Korpus: `trailing-action` und
    /// `leading-action` kommen vor). Wir vergleichen in Kleinschreibung,
    /// weil CSS-Bezeichner case-insensitiv sind.
    pub fn set(&mut self, s: &str) {
        *self = Self::LEER;
        let bytes = s.as_bytes();
        if bytes.is_empty() || bytes.len() > 32 {
            return;
        }
        // GEMESSENE ENTSCHEIDUNG, warum alles ausser Buchstaben, Ziffern
        // und `-`/`_` verworfen wird: ein Name mit Anfuehrungszeichen oder
        // Klammern kommt aus einem zerbrochenen Werteparser, und ein
        // Name, den wir nicht lesen koennen, darf keinen Platz belegen.
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            return;
        }
        for (i, b) in bytes.iter().enumerate() {
            self.0[i] = b.to_ascii_lowercase();
        }
    }

    /// Ist der Name leer (`auto`)?
    pub fn ist_leer(&self) -> bool {
        *self == Self::LEER
    }

    /// Vergleicht mit einem Namen aus dem Quelltext (Kleinschreibung).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Rest des Rings geprueft wird:
    /// `footerBar` beginnt wie `footer`, und ohne die Restpruefung waeren
    /// beide gleich — dann legte ein Kind an der Stelle des anderen.
    pub fn gleich(&self, s: &str) -> bool {
        if s.is_empty() || s.len() > 32 {
            return false;
        }
        let bytes = s.as_bytes();
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            return false;
        }
        for (i, b) in bytes.iter().enumerate() {
            if self.0[i] != b.to_ascii_lowercase() {
                return false;
            }
        }
        self.0[bytes.len()..].iter().all(|b| *b == 0)
    }
}

// LAYOUT-STIL: die Werte EINES Knotens
// ===========================================================================

/// Was das Layout ueber einen Knoten wissen muss, was `Style` nicht traegt.
///
/// Kein `Default` per Hand: `standard()` sagt jede Vorgabe und ist
/// getestet. `Derive` waere kuerzer, aber eine Aenderung an der Reihenfolge
/// der Felder wuerde stillschweigend die Bedeutung aendern.
/// Release 11.10.2026: eine CSS-Transformation (CSS Transforms 1/2) als
/// 2D-Affinmatrix `lin = [a, b, c, d]` (`x' = a·x + c·y + e`,
/// `y' = b·x + d·y + f`). Die Verschiebung `e`/`f` ist `[px, Anteil der
/// Breite, Anteil der Hoehe]` der Rahmenbox, weil Prozente erst mit dem
/// Layout feststehen; `ursprung` (`transform-origin`) ist
/// `[x px, x Anteil der Breite, y px, y Anteil der Hoehe]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transformation {
    pub lin: [f32; 4],
    pub tx: [f32; 3],
    pub ty: [f32; 3],
    pub ursprung: [f32; 4],
}

impl Transformation {
    /// Die Matrix `[a, b, c, d, e, f]` in Seitenkoordinaten fuer die
    /// Rahmenbox `(x, y, breite, hoehe)`: um den Ursprung gelegt
    /// (`p' = M·(p - o) + o`).
    #[must_use]
    pub fn in_seite(&self, x: f32, y: f32, breite: f32, hoehe: f32) -> [f32; 6] {
        let [a, b, c, d] = self.lin;
        let e = self.tx[0] + self.tx[1] * breite + self.tx[2] * hoehe;
        let f = self.ty[0] + self.ty[1] * breite + self.ty[2] * hoehe;
        let ox = x + self.ursprung[0] + self.ursprung[1] * breite;
        let oy = y + self.ursprung[2] + self.ursprung[3] * hoehe;
        [a, b, c, d, e + ox - a * ox - c * oy, f + oy - b * ox - d * oy]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStil {
    /// CSS box-sizing: width/height include padding and border.
    pub border_box: bool,
    /// `display`.
    pub anzeige: Anzeige,
    /// `flex-direction`.
    pub flex_richtung: FlexRichtung,
    /// Release 09.10.2026: `row-reverse` / `column-reverse` — die
    /// Hauptachse wird von hinten belegt (CSS Flexbox 1 §5.1). Die Achse
    /// selbst steht in `flex_richtung`.
    pub flex_umgekehrt: bool,
    /// `flex-wrap`.
    pub flex_umbruch: FlexUmbruch,
    /// `justify-content`.
    pub flex_buendig: Buendig,
    /// `align-items`.
    pub flex_ausrichtung: Ausrichtung,
    /// `object-fit` als **Zeichenkette**, wie die Kaskade sie liefert
    /// (`narsil_css::typen::Berechnet::objekt_passung`).
    ///
    /// BILD_29, GEMESSEN: `object-fit` wurde **lange** geparst
    /// (`narsil-css/src/parse.rs:5208`) und in `Berechnet` geschrieben
    /// (`kaskade.rs:4883`), und stand dann in **keinem** weiteren Typ — nicht
    /// in `Style`, nicht hier. Ein `<img object-fit:cover>` verhielt sich wie
    /// `fill`. **Dieses Feld ist die fehlende Kette.**
    ///
    /// **WARUM EINE ZEICHENKETTE UND NICHT EIN EIGENER TYP:** die Kaskade
    /// liefert genau das, und `narsil-layout` darf `narsil-image` nicht kennen.
    /// **WER LIEST, PARST:** `narsil_image::passung_lesen` — dieselbe
    /// Firefox-gemessene Funktion, die schon in `platzierung.rs` steht.
    /// Leer = keine Angabe = `fill` (der Anfangswert, Firefox-gemessen).
    pub objekt_passung: String,
    /// `object-position` als Zeichenkette.
    ///
    /// BILD_29, GEMESSEN — **UND HIER IST DIE LÜCKE WEITER VORN:**
    /// `object-position` wird geparst (`narsil-css/src/parse.rs:5311`,
    /// `"object-position" => Ziel::Lang(ESchluessel)`), hat aber **keine
    /// `Eigenschaft`-Variante** in `typen.rs` und **kein Feld** in `Berechnet`
    /// — der Wert faellt also **vor** `kaskade.rs:4883` weg. **Deshalb ist
    /// dieses Feld hier noch IMMER LEER**, und der Aufrufer bekommt den
    /// Firefox-Anfangswert `50% 50%`. Das ist **richtig**, nicht geraten: die
    /// Vorgabe ist gemessen. Es ist **unvollstaendig**, und genau das ist der
    /// naechste Schritt in `narsil-css`.
    pub objekt_position: String,
    /// `flex-grow` (Faktor, >= 0).
    pub flex_wachstum: f32,
    /// `flex-shrink` (Faktor, >= 0).
    pub flex_schrumpf: f32,
    /// `flex-basis` in px. `None` = die eigene Breite/Hoehe, sonst 0.
    pub flex_basis: Option<f32>,
    /// Der Lueckenwert auf der HAUPTACHSE in px (CSS Flexbox 1 §8.3: fuer
    /// `row` das `column-gap`, fuer `column` das `row-gap`).
    ///
    /// L3 (02.10.2026 04:5x): das war vorher „eine Zahl fuer Zeilen und
    /// Spalten, wie im Auftrag". GEMESSENER BEFUND, warum das nicht
    /// reichte: `l3_flex` zeigte fuer `row-gap: 20px` dieselbe
    /// Stellung wie fuer `gap: 20px` und fuer `column-gap: 20px`
    /// dieselbe wie ohne Angabe. Die Kaskade hatte `row-gap` und
    /// `column-gap` sauber getrennt in `Berechnet` (`luecke_zeile`,
    /// `luecke_spalte`) — und der Plan las nur `luecke_px`, das nur
    /// `gap` schrieb.
    pub luecke: f32,
    /// Der Lueckenwert auf der QUERACHSE in px (der andere der beiden).
    /// Ohne Umbruch ist er gegenstandslos: er steht zwischen den Zeilen,
    /// und eine Zeile hat keinen Nachbarn.
    pub luecke_quer: f32,
    /// `grid-template-columns` (leer = kein Raster mit Spalten).
    pub spalten: Vec<Spalte>,
    /// `grid-auto-flow: column` — die Kinder laufen in SPALTEN.
    ///
    /// L4 (02.10.2026 11:3x). `false` = Zeilenrichtung = der Anfangswert
    /// `row` (CSS Grid 1 §8.5).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das am `LayoutStil` steht und
    /// nicht am Rasterzell-Typ: es ist eine Eigenschaft des CONTAINERS
    /// (welche Achse die Reihenfolge nimmt), nicht einer Spur. Eine
    /// Spur weiss nichts von ihren Nachbarn in der anderen Achse.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Boolean und kein Winkel oder
    /// kein `Fluss`: §8.5 kennt genau zwei Richtungen, und `dense` ist
    /// ein ZUSATZ darauf (nicht gebaut, siehe Kaskade in narsil-css).
    pub spaltenfluss: bool,
    /// `grid-auto-flow: … dense` (§8.5) — die dichte Packung.
    ///
    /// L4 (03.10.2026 04:2x). `false` = `sparse` = der Anfangswert.
    /// GEMESSENER BEFUND, dass `dense` bis eben wirkungslos war: nur das
    /// ERSTE Wort des Werts wurde gelesen (11:3x), also verhielt sich
    /// `row dense` wie `row`.
    pub dicht: bool,
    /// `grid-template-rows` (leer = implizite Zeilen, also `auto`).
    ///
    /// L4 (02.10.2026 19:4x), CSS Grid 1 §8.3.
    ///
    /// GEMESSENER BEFUND, was vorher passierte: es gab kein Feld, also
    /// war die Zeilenhoehe immer das groesste Kind der Zeile. GEMESSEN in
    /// `narsil-css/tests/position.rs` (`l4_grid_template_rows_legt_die_
    /// zeilenhoehe`): `grid-template-rows: 200px 200px` mit 20 px
    /// Inhalt ergab 20 px statt 200 px.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das am `LayoutStil` steht und nicht
    /// irgendwo im Rasterzell-Typ: es ist eine Eigenschaft des
    /// CONTAINERS (die Groessen seiner Zeilen), und `LayoutStil` ist
    /// genau der Container-Beschreibungssatz. Der Typ `Spalte` traegt
    /// beides — er steht fuer „eine Spur", und §8.3 macht fuer Zeilen
    /// dieselbe Formeln wie §8.2 fuer Spalten.
    /// L4 (03.10.2026 09:5x): `align-self` des Kindes (§6.6, §10.4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das im PLAN steht und nicht als Zahl
    /// im `RasterKind`: es ist ein Wert aus dem CSS, kein Laufzeitmass —
    /// dieselbe Form wie `spalten` und `zeilen`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum [`Ausrichtung`] und kein eigener Typ:
    /// der existiert hier schon (fuer `align-items` im Flex), und §6.6
    /// nennt fuer beide dieselbe Wertemenge. Ein zweiter Typ waere eine
    /// zweite Liste, die auseinanderlaufen kann. `baseline` hat hier keine
    /// Entsprechung und wird auf `Strecke` abgebildet (dokumentiert in
    /// `layout_plan.rs`).
    ///
    /// L4 (03.10.2026 11:5x): `Option`, weil `None` das `auto` bedeutet
    /// — §6.3: „auto bedeutet, den Wert des Containers zu benutzen".
    pub selbst_vertikal: Option<Ausrichtung>,
    /// `justify-self` des Kindes (§6.2, §10.3), dieselbe Form.
    pub selbst_waagerecht: Option<Ausrichtung>,
    /// `justify-items` des Containers (§6.2) — der Vorgabewert fuer
    /// `justify-self`. `align-items` steht schon in `flex_ausrichtung`.
    pub justify_ausrichtung: Ausrichtung,
    /// Die **ANGABE** von `width` in px, `None` = `auto`.
    ///
    /// L4 (03.10.2026 13:5x). GEMESSENER BEFUND, dass es fehlte: die
    /// Querachsen-Ausrichtung eines Zeilen-Flex braucht die Hoehe, die
    /// das Kind verlangt hat — und die MESSUNG ist die falsche Quelle
    /// (L3 06:2x: 19,2 px statt 20 px, weil ein gestrecktes Kind die
    /// Zeilenhoehe bekommen hat). Ohne dieses Feld war `FlexKind::quer`
    /// immer `None`, und `quermass_zeile` lieferte fuer `Start`,
    /// `Ende`, `Mitte` und `Strecke` denselben Versatz 0.
    ///
    /// GEMESSEN (`examples/l3_flex.rs`, Block H): `align-items:
    /// flex-start`, `center` und `flex-end` ergaben in einem
    /// Zeilen-Flex alle `y 48`.
    pub breite_angabe: Option<f32>,
    /// Die **ANGABE** von `height` in px, `None` = `auto`. Siehe
    /// [`LayoutStil::breite_angabe`].
    pub hoehe_angabe: Option<f32>,
    /// Prozentangaben werden erst gegen den tatsaechlichen Container aufgeloest.
    pub breite_anteil: Option<f32>,
    pub hoehe_anteil: Option<f32>,
    /// Release 11.10.2026: `height: P%` eines Kindes eines Flex- oder
    /// Rasterelements, dessen Hoehe erst das Layout definit macht (gedehnt,
    /// CSS Flexbox 1 §9.4 Schritt 11 und §9.8; Rasterbereich, CSS Grid 1
    /// §6.6). Die Kaskade setzt `height` dann auf `auto`; `box.rs` loest den
    /// Anteil auf, sobald der Eltern eine definite Hoehe gesetzt bekommt
    /// (`Ctx::hoehe_definit`).
    pub hoehe_bedingt: Option<f32>,
    /// Release 11.10.2026 (Agent CSS2): `width`/`height` als
    /// `min-content | max-content | fit-content` (CSS Sizing 3 §2): ein
    /// absoluter Kasten dehnt sich dann nicht zwischen `left`/`right` bzw.
    /// `top`/`bottom` (der UA-Stil von `<dialog>`).
    pub breite_inhalt: bool,
    pub hoehe_inhalt: bool,
    /// Die Schriftgroesse in px (§6.4 — sie bestimmt den Aufstieg, also
    /// die Grundlinie eines Kindes).
    ///
    /// L4 (03.10.2026 13:4x). GEMESSENE ENTSCHEIDUNG, warum gerade die
    /// und nicht die gemessene Hoehe: ein Kind mit `height: 40px` und
    /// einer Zeile 16-px-Schrift hat einen Aufstieg von 16 * 5/7 =
    /// 11,4 px. Aus der Kastenhoehe (40 * 5/7 = 28,6) waere die
    /// Grundlinie um 17 px daneben.
    pub schriftgrad: f32,
    pub zeilen: Vec<Spalte>,
    /// `grid-auto-rows` (leer = implizite Zeilen sind `auto`).
    ///
    /// L4 (02.10.2026 21:6x), CSS Grid 1 §7.5.
    pub auto_zeilen: Vec<Spalte>,
    /// `grid-auto-columns` (leer = implizite Spalten sind `auto`).
    ///
    /// L4 (02.10.2026 21:6x), CSS Grid 1 §7.3.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum dieses Feld den Wert aus Bericht
    /// 21:4x ersetzt und nicht neben ihm steht: dort war die Breite einer
    /// impliziten Spalte die breiteste explizite Spur, weil `auto` als
    /// Hoechstwert `max-content` ist und `RasterKind` nur die Hoehe
    /// traegt. Mit diesem Feld ist der Fall „der Autor hat es
    /// angegeben" gedeckt; die Schranke bleibt nur fuer die Faelle, in
    /// denen die Liste leer ist.
    pub auto_spalten: Vec<Spalte>,
    /// Wie viele Zeilen dieses Item belegt (CSS Grid 1 §8.3, `0`/`1` = 1).
    ///
    /// L4 (02.10.2026 22:5x). GEMESSENER BEFUND, warum die Zahl hier und
    /// nicht beim Raster liegt: §8.3 ist eine Eigenschaft des ITEMS, und
    /// die Platzierung entscheidet pro Kind. `RasterKind` (narsil-css)
    /// traegt sie deshalb auch pro Kind.
    pub spann_zeilen: usize,
    /// Wie viele Spalten dieses Item belegt (§8.3, `0`/`1` = 1).
    ///
    /// L4 (22:5x): dasselbe fuer die Querachse.
    pub spann_spalten: usize,
    /// Feste Startlinie auf der Zeilenachse (§8.5), 1-basiert, `0` = auto.
    ///
    /// L4 (03.10.2026 01:6x). GEMESSENE ENTSCHEIDUNG, warum hier der
    /// ROHWERT steht und nicht die aufgeloeste Zeile: eine negative Linie
    /// (`-1` = die letzte) laesst sich erst aufloesen, wenn die Zeilenzahl
    /// bekannt ist — und die kennt erst die Platzierung in narsil-css.
    pub zeile_start: i32,
    /// Feste Endlinie auf der Zeilenachse (§8.5), `0` = auto.
    pub zeile_ende: i32,
    /// Feste Startlinie auf der Spaltenachse (§8.5), `0` = auto.
    pub spalte_start: i32,
    /// Feste Endlinie auf der Spaltenachse (§8.5), `0` = auto.
    pub spalte_ende: i32,
    /// `grid-template-areas` (§8.1): Zeilen von Gebietsnamen.
    ///
    /// L4 (03.10.2026 02:7x). **ERSETZT im `Berechnet` den Rohwert**
    /// (dort `gitter_gebiete: String`, der von niemandem gelesen wurde).
    pub gebiete: Vec<Vec<Gebiet>>,
    /// `grid-area: <name>` (§8.4) — der Name, den dieses Kind will.
    ///
    /// L4 (03.10.2026 02:8x). Leer = `auto`. GEMESSENE ENTSCHEIDUNG,
    /// warum der Name hier am KIND steht und nicht das Rechteck: das
    /// Rechteck entsteht aus der Vorlage des CONTAINERS, und die kennt
    /// der Plan des Kindes nicht.
    pub gebiet: Gebiet,
    /// `position`.
    pub position: PositionArt,
    /// `top` in px (bereits gegen die Ansicht aufgeloest). `None` = auto.
    pub oben: Option<f32>,
    /// `right`.
    pub rechts: Option<f32>,
    /// `bottom`.
    pub unten: Option<f32>,
    /// `left`.
    pub links: Option<f32>,
    /// Rohanteile fuer absolute/relative Offsets; Basis ist der enthaltende Block.
    pub oben_prozent: Option<f32>,
    pub rechts_prozent: Option<f32>,
    pub unten_prozent: Option<f32>,
    pub links_prozent: Option<f32>,
    /// `z-index`. `None` = auto.
    pub z_index: Option<i32>,
    /// `transform` oder `filter` mit einem anderen Wert als `none`
    /// erzeugt einen Stapelkontext.
    ///
    /// L5 (02.10.2026 17:0x). CSS Transforms 1 §3, CSS Filter Effects
    /// 1 §2.1. GEMESSENE ENTSCHEIDUNG, warum EIN Boolean fuer beides
    /// und nicht zwei: die Regel ist fuer beide Eigenschaften woertlich
    /// gleich („jeder Wert ausser `none`"), und zwei Felder wuerden zwei
    /// Felder sein, die man beim Setzen beide setzen muss. Die Regel
    /// steht in `narsil_css::erzeugt_stapelkontext` und wird dort
    /// ausgewertet — hier steht nur das Ergebnis.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das am `LayoutStil` steht und
    /// nicht am Stapelkontext-Aufruf: dasselbe Muster wie `position`
    /// und `ist_flex_container` — `build_display_list` bekommt nur Baum
    /// und Dokument, nicht den Plan.
    pub transformt: bool,
    /// `float`.
    pub schweben: FloatArt,
    /// `clear`.
    pub raeumen: RaeumungArt,
    /// `overflow`.
    pub ueberlauf: UeberlaufArt,
    /// Erhaelt `margin-left/right: auto` nach der numerischen CSS-Kaskade.
    pub marge_links_auto: bool,
    pub marge_rechts_auto: bool,
    /// Release 11.10.2026: `margin-top`/`margin-bottom: auto` (fuer Flex
    /// und Raster; im Blockfluss bedeutungslos).
    pub marge_oben_auto: bool,
    pub marge_unten_auto: bool,
    /// CSS `aspect-ratio`; 0 bedeutet `auto`.
    pub seitenverhaeltnis: f32,
    /// added 05.10.2026, Rolle SERVER (SERVER_13): **`colspan` und
    /// `rowspan` aus den HTML-Attributen** — die Zahlen, ohne die
    /// `lege_tabelle` keine Zelle legen kann.
    ///
    /// GEMESSENER BEFUND, warum es dieses Feld geben musste: `narsil_dom::
    /// NodeKind` hat **drei** Varianten (`Page`, `Container`, `Text`) —
    /// **keinen** Tabellen-, Zeilen- oder Zellenknoten, und `narsil_core::
    /// Style` hatte weder `colspan` noch `rowspan`. Es gab also **keinen
    /// Ort**, an dem diese beiden Zahlen untergebracht werden konnten;
    /// `lege_tabelle` bekam sie nie.
    ///
    /// **WARUM ZWEI `u16` UND NICHT EIN `TabellenRolle`-Feld:** die
    /// Rollen stehen in `anzeige`. Diese beiden Zahlen sind etwas
    /// anderes — sie stehen in keinem `display`-Schluesselwort, sondern
    /// im HTML-Attribut, und sie sind **Zahlen**, keine Rolle.
    ///
    /// **WIE GROESS:** HTML parst `colspan`/`rowspan` als nichtnegative
    /// ganze Zahl und **begrenzt auf 1000** (die Spezifikation sagt, dass
    /// groessere Werte auf 1000 begrenzt werden — Firefox macht das
    /// ebenfalls). `0` bedeutet „1“, weil HTML `rowspan="0"` als „bis zum
    /// Ende der Tabelle“ liest; das ist **nicht** implementiert, und der
    /// Wert 0 wird hier als 1 behandelt. GEMESSEN nicht, entschieden nach
    /// HTML 4.01, §11.2.1 — wenn das jemand braucht, ist es eine eigene
    /// Messung.
    pub colspan: u16,
    /// added 05.10.2026, siehe `colspan`. `1` = normale Zeile.
    pub rowspan: u16,
    /// Release 08.10.2026: `inline-flex`/`inline-grid` — der Kasten ist
    /// AUSSEN inline (steht in der Zeile wie `InlineBlock`), innen
    /// `anzeige` (Flex/Raster).
    pub inline_aussen: bool,
    /// Release 11.10.2026 (Agent CSS2): `display: flow-root` — ein Block
    /// mit eigenem Formatierungskontext (meidet Floats, umschliesst seine,
    /// laesst keine Raender durch).
    pub flow_root: bool,
    /// Release 12.10.2026 (Agent CSS2): der enthaltende Block dieses absolut
    /// positionierten Kastens ist ein positioniertes INLINE-Element ohne
    /// eigenen Kasten im Seitenbau (CSS 2.1 §10.1, Punkt 4) — genaehert als
    /// Rechteck der Breite und Hoehe 0 an der statischen Lage.
    pub cb_inline: bool,
    /// Release 11.10.2026: Masonry (`display: grid-lanes`, CSS Grid 3):
    /// 0 = kein, 1 = Spalten-Spuren (die Elemente stapeln sich senkrecht),
    /// 2 = Zeilen-Spuren (waagerecht).
    pub lanes: u8,
    /// `flow-tolerance` in px (`normal` = 1em): Spuren, deren Ende hoechstens
    /// so weit hinter der kuerzesten liegt, gelten als gleich kurz.
    pub lanes_toleranz: f32,
    /// `grid-lanes-direction`: Bit 1 `fill-reverse`, Bit 2 `track-reverse`;
    /// Bit 4 `grid-lanes-pack: dense` (Luecken ueber den Spuren fuellen).
    pub lanes_umkehr: u8,
    /// Release 11.10.2026: `order` (CSS Display 3) — die Lage des Kastens in
    /// der Reihenfolge seines Flex-/Raster-Containers.
    pub ordnung: i32,
    /// Release 11.10.2026: `transform` (und `translate`/`rotate`/`scale`),
    /// `None` ohne.
    pub transform: Option<Transformation>,
    /// Release 11.10.2026: `background-clip` — 0 border-box, 1 padding-box,
    /// 2 content-box, 3 text.
    pub hintergrund_clip: u8,
    /// Release 11.10.2026: die Rastereigenschaften eines `display: grid`
    /// (CSS Grid 1/2) fuer den neuen Rasteralgorithmus. `None` = kein Raster
    /// oder Masonry (`grid-lanes`, dort gilt weiter die alte Form).
    pub gitter: Option<Box<crate::gitter::GitterStil>>,
    /// Release 11.10.2026: Lage und `*-self` als Rasterelement — `None`, wenn
    /// alles `auto` ist.
    pub gitter_kind: Option<Box<crate::gitter::GitterKindStil>>,
    /// Release 11.10.2026: `width`/`min-width`/`max-width`/`height` als
    /// Schluesselwort (CSS Sizing 3 §2.1): 0 keines, 1 `min-content`,
    /// 2 `max-content`, 3 `fit-content`.
    pub groessen_art: [u8; 4],
    /// Release 14.10.2026: `stretch` (also `-webkit-fill-available`,
    /// `-moz-available`; CSS Sizing 4 §3.1) as a bit mask: 1 `width`,
    /// 2 `min-width`, 4 `max-width`, 8 `height`, 16 `min-height`,
    /// 32 `max-height`; 64 `width` and 128 `height` in the cross axis of a
    /// flex item (the size stays `auto`, the item is stretched to its line
    /// whatever `align-self` says, see `flexbox.rs`).
    pub stretch_bits: u8,
    /// Release 14.10.2026: `min-height` (bit 1) / `max-height` (bit 2) as
    /// `min-content`/`max-content`/`fit-content`: for a block the content
    /// height (CSS Sizing 3 §2.1, §5.1).
    pub height_keyword_limits: u8,
    /// Release 14.10.2026: the argument of `fit-content(<length-percentage>)`
    /// for `width`, `min-width`, `max-width` (CSS Sizing 3 §2.1; the keyword
    /// itself is `groessen_art` 3).
    pub fit_content_arg: [Option<crate::gitter::LaengeProzent>; 3],
    /// Release 14.10.2026: size containment (CSS Contain 2 §3.2; `contain:
    /// size`/`strict`/`inline-size`, `content-visibility: hidden`): bit 1 the
    /// width, bit 2 the height. The contents then do not contribute to the size.
    pub size_contained: u8,
    /// Release 14.10.2026: `writing-mode` of the box, only as far as the
    /// self-alignment of absolutely positioned boxes needs it: 0 horizontal,
    /// 1 `vertical-lr`/`sideways-lr`, 2 `vertical-rl`/`sideways-rl`. Layout
    /// itself stays horizontal.
    pub vertical_writing: u8,
    /// Release 14.10.2026: `contain-intrinsic-size` (CSS Sizing 4 §6) as a
    /// content-box size (width, height) in px; `None` = `none` (0).
    pub contain_size: [Option<f32>; 2],
    /// Release 11.10.2026: die Flex-Eigenschaften eines `display: flex`
    /// (CSS Flexbox 1) fuer den neuen Flex-Algorithmus (`flexbox.rs`).
    pub flex: Option<Box<crate::flexstil::FlexStil>>,
    /// Release 11.10.2026: `flex-basis` mit Prozent und Schluesselwoertern.
    pub flex_basis_art: crate::flexstil::FlexBasis,
    /// Release 11.10.2026: ein Rollkasten (`overflow-x` ODER `overflow-y`
    /// nicht `visible`, CSS Overflow 3 §3) — dann gilt keine automatische
    /// Mindestgroesse (CSS Flexbox 1 §4.5, CSS Grid 1 §6.6).
    pub rollkasten: bool,
    /// Release 11.10.2026: `direction: rtl` des Elements selbst (fuer
    /// `self-start`/`self-end`, CSS Box Alignment 3 §4.2).
    pub rtl: bool,
}
impl Default for LayoutStil {
    fn default() -> Self {
        Self::standard()
    }
}

impl LayoutStil {
    /// Release 15.10.2026: the default style as a shared reference (the
    /// same value as [`LayoutStil::standard`]), for lookups that borrow
    /// instead of cloning.
    pub fn default_ref() -> &'static LayoutStil {
        &STANDARD
    }
}

impl LayoutStil {
    /// Alle Anfangswerte — das ist ein Block im Normalfluss.
    ///
    /// GEMESSEN wichtig: `zwei` gleiche Anfangswerte sind nicht dasselbe
    /// wie `null`. Der Plan muss unterscheiden koennen zwischen "der
    /// Knoten ist ein Flex-Container" und "der Knoten hat kein Layout".
    pub fn standard() -> LayoutStil {
        LayoutStil {
            border_box: false,
            inline_aussen: false,
            flow_root: false,
            cb_inline: false,
            lanes: 0,
            lanes_toleranz: 16.0,
            lanes_umkehr: 0,
            ordnung: 0,
            transform: None,
            hintergrund_clip: 0,
            gitter: None,
            gitter_kind: None,
            groessen_art: [0; 4],
            stretch_bits: 0,
            height_keyword_limits: 0,
            fit_content_arg: [None; 3],
            size_contained: 0,
            vertical_writing: 0,
            contain_size: [None, None],
            flex: None,
            flex_basis_art: crate::flexstil::FlexBasis::Auto,
            rollkasten: false,
            rtl: false,
            // L4 (03.10.2026 09:5x): §6.6 — `stretch` ist der Anfangswert
            // fuer `align-self`/`justify-self`, und es ist das Verhalten,
            // das die Platzierung ohnehin rechnet.
            // L4 (11:5x): `None` = `auto` = „den Wert des Containers".
            // BILD_29: beide Felder sind **Zeichenketten**, und die Vorgabe ist
            // die LEERE — das ist `fill` bzw. `50% 50%`, beides der
            // Firefox-Anfangswert (gemessen). `Default` ruft `standard()`
            // auf, also deckt das eine Aenderung beide Wege.
            objekt_passung: String::new(),
            objekt_position: String::new(),
            selbst_vertikal: None,
            selbst_waagerecht: None,
            justify_ausrichtung: Ausrichtung::Strecke,
            // L4 (13:5x): keine Angabe = `auto`.
            breite_angabe: None,
            hoehe_angabe: None,
            breite_anteil: None,
            hoehe_anteil: None,
            hoehe_bedingt: None,
            breite_inhalt: false,
            hoehe_inhalt: false,
            // added 05.10.2026, SERVER_13: keine Angabe = **eine** Spalte
            // und **eine** Zeile. Das ist der HTML-Anfangswert: `<td>` ohne
            // `colspan`/`rowspan` ist eine Zelle in einer Zeile. (HTML
            // erlaubt `colspan="0"` als „bis zum Ende“ — das ist nicht
            // implementiert, siehe das Feld.)
            colspan: 1,
            rowspan: 1,
            // L4 (13:4x): der Anfangswert der Schrift, 16 px.
            schriftgrad: 16.0,
            anzeige: Anzeige::Block,
            flex_richtung: FlexRichtung::Reihe,
            flex_umgekehrt: false,
            flex_umbruch: FlexUmbruch::Keiner,
            flex_buendig: Buendig::Start,
            flex_ausrichtung: Ausrichtung::Start,
            flex_wachstum: 0.0,
            flex_schrumpf: 1.0,
            flex_basis: None,
            luecke: 0.0,
            luecke_quer: 0.0,
            spalten: Vec::new(),
            spaltenfluss: false,
            // L4 (04:2x): `grid-auto-flow: … dense` (§8.5).
            dicht: false,
            zeilen: Vec::new(),
            auto_zeilen: Vec::new(),
            auto_spalten: Vec::new(),
            spann_zeilen: 0,
            spann_spalten: 0,
            // L4 (01:6x): die festen Linien (§8.5), roh und 1-basiert.
            zeile_start: 0,
            zeile_ende: 0,
            spalte_start: 0,
            spalte_ende: 0,
            // L4 (02:7x): `grid-template-areas` (§8.1) und der Name des
            // Kindes (§8.4).
            gebiete: Vec::new(),
            gebiet: Gebiet::LEER,
            transformt: false,
            position: PositionArt::Statisch,
            oben: None,
            rechts: None,
            unten: None,
            links: None,
            oben_prozent: None,
            rechts_prozent: None,
            unten_prozent: None,
            links_prozent: None,
            z_index: None,
            schweben: FloatArt::Keins,
            raeumen: RaeumungArt::Keins,
            ueberlauf: UeberlaufArt::Sichtbar,
            marge_links_auto: false,
            marge_rechts_auto: false,
            marge_oben_auto: false,
            marge_unten_auto: false,
            seitenverhaeltnis: 0.0,
        }
    }

    /// Baut einen Flex-Container (`display: flex`).
    pub fn flex(richtung: FlexRichtung) -> LayoutStil {
        LayoutStil {
            anzeige: Anzeige::Flex,
            flex_richtung: richtung,
            ..LayoutStil::standard()
        }
    }

    /// Baut ein Raster (`display: grid`) mit diesen Spalten.
    ///
    /// M3/M4: mehr als [`MAX_RASTER_SPALTEN`] Spalten werden hier
    /// GESCHNITTEN (nicht abgelehnt) — ein Raster mit 40 Spalten soll
    /// die ersten 32 zeigen, nicht gar nichts.
    pub fn raster(spalten: &[Spalte]) -> LayoutStil {
        LayoutStil {
            anzeige: Anzeige::Raster,
            spalten: spalten.iter().cloned().take(MAX_RASTER_SPALTEN).collect(),
            ..LayoutStil::standard()
        }
    }

    /// Ist dieser Knoten ein Flex-Container?
    pub fn ist_flex(&self) -> bool {
        self.anzeige == Anzeige::Flex
    }

    /// Ist dieser Knoten ein Raster?
    pub fn ist_raster(&self) -> bool {
        self.anzeige == Anzeige::Raster
    }

    /// Nimmt dieser Knoten am Fluss teil, oder wird er herausgenommen?
    ///
    /// Absolut und fixed stehen NICHT mehr im Fluss (CSS 2.1, 9.5): sie
    /// belegen keinen Platz fuer die Geschwister. Das ist der ganze
    /// Unterschied zu `relative`.
    pub fn ist_im_fluss(&self) -> bool {
        !matches!(self.position, PositionArt::Absolut | PositionArt::Fest)
    }

    /// Wird dieser Knoten ueberhaupt gebaut?
    pub fn wird_gezeichnet(&self) -> bool {
        self.anzeige != Anzeige::Keins
    }
}

// ===========================================================================
// PLAN: die Nebentabelle
// ===========================================================================

/// Die vier Beschraenkungen EINES Knotens (CSS 2.1, 10.4).
///
/// # WOHER UND WOZU
///
/// * `used = min(max-width, max(min-width, width))` — CSS 2.1, 10.4.
/// * Gefuellt wird es von `narsil_css::layout_grenzen(&Berechnet)`,
///   gelesen wird es von `narsil-layout-engine`.
///
/// # WARUM EIGENER TYP UND KEINE VIER FELDER IM `LayoutStil`
///
/// `LayoutStil` wird an mehreren Stellen als Struct-Literal gebaut,
/// unter anderem in `narsil-css/src/layout_plan.rs`. Vier Felder
/// hinzuzufuegen wuerde jeden dieser Aufrufe ungueltig machen — und
/// `narsil-css` gehoert seit dem 01.10.2026 der Rolle CSS, nicht der
/// Rolle LAYOUT. Die Tabelle ist dasselbe Muster wie `LayoutPlan`
/// selbst: sortierte Liste mit Binaersuche.
///
/// `None` heisst "nicht gesetzt" — nicht 0. 0 waere eine Aussage ueber
/// die Seite, die der Plan nicht treffen darf.
/// Schreibrichtung eines Knotens (K17b, CSS Writing Modes 3 §2.4).
///
/// # WARUM EIN EIGENER TYP
///
/// `narsil_css::SchreibRichtung` und `Richtung` sind VERSCHIEDENE Typen,
/// genau wie `narsil_css::Anzeige` und `plan::Anzeige`. Die Bruecke
/// wandelt um. Das ist kein Zufall: eine Crate soll nicht von der
/// anderen abhaengen, nur weil zwei Werte gleich heissen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Richtung {
    /// links nach rechts (Anfangswert).
    Ltr,
    /// rechts nach links.
    Rtl,
}

/// Textausrichtung (K17b, CSS Text 3 §3.1).
///
/// GEMESSENER BEFUND, der dieses Feld noetig macht: `grep -rn
/// "textausrichtung" crates/ | grep -v narsil-css/src` findet — ausser
/// in Tests — **keinen** Leser. Die Kaskade rechnet `text-align` seit
/// Jahren richtig, und es hat niemanden erreicht. `direction: rtl`
/// allein aendert daran nichts: ohne Ausrichtung im Plan gibt es keine
/// Zeile, die man nach rechts schieben koennte.
///
/// `narsil_css::Textausrichtung` und `Ausrichtung` sind VERSCHIEDENE
/// Typen, genau wie `Anzeige`. Die Bruecke wandelt um.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Textausrichtung {
    /// links (Anfangswert; `start` bei `ltr`).
    Links,
    /// rechts (`end` bei `ltr`, `start` bei `rtl`).
    Rechts,
    /// zentriert.
    Zentriert,
    /// Blocksatz.
    Blocksatz,
}

/// Hoechste Knoten mit `Rtl` (K17b). Begruendung: nur `Rtl` wird
/// gespeichert, `Ltr` ist der Default — auf einer arabischen Seite ist
/// also JEDER Knoten ein Eintrag, auf einer deutschen keiner. Die Zahl
/// ist so hoch wie bei [`MAX_GRENZEN`], damit es hier keine neue
/// Engstelle gibt.
pub const MAX_RICHTUNGEN: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Grenzen {
    /// `min-width` in px.
    pub min_breite: Option<f32>,
    /// `max-width` in px.
    pub max_breite: Option<f32>,
    /// `min-height` in px.
    pub min_hoehe: Option<f32>,
    /// `max-height` in px.
    pub max_hoehe: Option<f32>,
}

/// P19 (05.10.2026, 20:35): DIE PROZENTANTEILE DER VIER GRENZEN.
///
/// # WARUM EINE NEBENTABELLE UND KEINE VIER FELDER IN `Grenzen`
///
/// `Grenzen` wird an mehreren Stellen als **Struct-Literal** gebaut, und
/// nicht alle sind in dieser Crate: `grep -rn "Grenzen {" crates/` findet
/// `narsil-devtools/src/grenzen.rs:312` (`STANDARDAUSGABE`) — eine fremde
/// Crate. Vier Felder mehr wuerden sie brechen. Dasselbe Muster hat
/// `Richtung` (P9) schon getragen, und der Grund steht dort: "Ein neues
/// Feld dort bricht diese Crate und damit den ganzen Bau."
///
/// # WAS DRINSTEHT
///
/// `min-width`/`max-width`/`min-height`/`max-height` als **Prozentzahl**.
/// `Some(100.0)` heisst "`max-width: 100%`".
///
/// GEMESSENER FEHLER, der sie noetig macht (20:32,
/// `file:///tmp/opencode/bruecke_test.html`, Fenster 900 px, Eltern
/// 852 px):
///
/// ```text
/// img, video { max-width: 100% }     (nixos.org, woertlich)
/// <img width="2000" height="900">
/// ->  explicit=Some(900.0)xSome(404.99997)  verfuegbar=852.0
/// ```
///
/// GEMESSENER FEHLER IM WERKZEUG: eine eingerueckte Zeile in einem
/// Doc-Kommentar ist fuer rustdoc ein **Codeblock**, und der wird als
/// Doctest gebaut. `<img width="2000">` ist kein Rust — der Test
/// scheiterte an einem Kommentar.
///
/// 900 ist die ANSICHTSbreite, 852 der Eltern: die Klamme sitzt 48 px zu
/// weit aussen. Der Weg, warum: die Kaskade rechnet `100%` sofort gegen
/// die Ansicht (`kaskade.rs::breiten_wert`), `Grenzen::max_breite`
/// traegt nur die Zahl, und `box.rs::grenzen` nimmt sie unveraendert.
/// Die Prozentbasis geht an der ersten Stelle verloren.
///
/// `100 %` ist der haeufigste Wert in echten Stylesheets
/// (`img, video { max-width: 100% }`, `.os-img-container img
/// { max-width: 100% }` auf debian.org).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GrenzenProzent {
    pub min_breite: Option<f32>,
    pub max_breite: Option<f32>,
    pub min_hoehe: Option<f32>,
    pub max_hoehe: Option<f32>,
}

impl GrenzenProzent {
    /// `true`, wenn keine der vier Angaben ein Prozent ist.
    ///
    /// Genau fuer den Fall, in dem der Plan **keinen** Eintrag bekommt:
    /// eine leere Zeile im Plan ist eine Zeile ohne Aussage.
    pub fn ist_leer(&self) -> bool {
        self.min_breite.is_none()
            && self.max_breite.is_none()
            && self.min_hoehe.is_none()
            && self.max_hoehe.is_none()
    }
}

/// Hoechstens so viele Knoten tragen Prozentgrenzen (M3/M4).
pub const MAX_GRENZEN_PROZENT: usize = 100_000;

/// P9 (05.10.2026): `vertical-align` (CSS Inline 3, §6).
///
/// # WARUM EINE EIGENE TABELLE UND KEIN FELD IM `LayoutStil`
///
/// `narsil_css::layout_stil` baut `LayoutStil` als VOLLSTAENDIGES
/// Struct-Literal (layout_plan.rs:63-76, ohne `..standard()`). Ein
/// neues Feld dort bricht diese Crate und damit den ganzen Bau. Also
/// dasselbe Muster wie bei [`Grenzen`]: eine Nebentabelle im Plan.
///
/// # WAS DIE VIER WERTE BEDEUTEN
///
/// CSS Inline 3, §6 richtet die Zeile an der Grundlinie aus; das
/// `vertical-align` eines Kindes sagt, woran es haengt.
///
/// * `Grundlinie` — der Anfangswert. Der Text eines Kastens sitzt auf
///   der Grundlinie, ein Kasten mit Inhalt darunter traegt seine
///   Unterkante.
/// * `Mitte` — die Mitte des Kastens auf der Mitte der Zeile.
/// * `Oben` / `Unten` — die Kanten aneinander.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vertikal {
    /// `vertical-align: baseline` — der Anfangswert.
    #[default]
    Grundlinie,
    /// `vertical-align: middle`.
    Mitte,
    /// `vertical-align: top`.
    Oben,
    /// `vertical-align: bottom`.
    Unten,
}

impl Vertikal {
    /// Alles `Grundlinie`? Solche Eintraege werden nicht gespeichert.
    pub fn ist_standard(&self) -> bool {
        *self == Vertikal::Grundlinie
    }
}

/// Release 11.10.2026 (Agent CSS2): die genaue Rolle eines Tabellenteils
/// (CSS 2.1 §17.2). `Anzeige` fasst Zeile und Zeilengruppen in
/// `TabelleZeile` und Beschriftung und Spalten in `TabelleRand` zusammen;
/// das Tabellen-Layout braucht die Unterscheidung (Kopf- vor Rumpf- vor
/// Fussgruppe, Spaltenbreiten aus `<col>`, Beschriftung ausserhalb des
/// Rasters). Nebentabelle aus demselben Grund wie [`Vertikal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabellenRolle {
    /// Kein Tabellenteil (oder unbekannt — dann gilt die Struktur).
    #[default]
    Keine,
    /// `display: table`.
    Tabelle,
    /// `display: inline-table`.
    InlineTabelle,
    /// `display: table-row-group`.
    Rumpf,
    /// `display: table-header-group`.
    Kopf,
    /// `display: table-footer-group`.
    Fuss,
    /// `display: table-row`.
    Zeile,
    /// `display: table-cell`.
    Zelle,
    /// `display: table-caption`.
    Beschriftung,
    /// `display: table-column`.
    Spalte,
    /// `display: table-column-group`.
    Spaltengruppe,
}

/// Release 11.10.2026 (Agent CSS2): die Tabellen-Angaben eines Knotens —
/// Rolle und die Tabellen-Eigenschaften aus CSS 2.1 Kapitel 17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TabellenAngabe {
    /// Die genaue Rolle (siehe [`TabellenRolle`]).
    pub rolle: TabellenRolle,
    /// `table-layout: fixed` (§17.5.2.1).
    pub fest: bool,
    /// `border-collapse: collapse` (§17.6.2).
    pub zusammen: bool,
    /// `caption-side: bottom` (§17.4.1).
    pub unten: bool,
    /// `empty-cells: hide` (§17.6.1.1).
    pub leer_verbergen: bool,
    /// `span` einer Spalte/Spaltengruppe (HTML `<col span>`), 0 = keine Angabe.
    pub spann: u16,
}

impl TabellenAngabe {
    /// Nichts gesetzt? Solche Eintraege werden nicht gespeichert.
    pub fn ist_standard(&self) -> bool {
        *self == TabellenAngabe::default()
    }
}

impl Grenzen {
    /// Sind alle vier Werte `None`? Solche Eintraege werden nicht
    /// gespeichert — die allermeisten Knoten haben keine Beschraenkung,
    /// und eine Tabelle aus 200 000 leeren Eintraegen waere Speicher
    /// fuer nichts.
    pub fn ist_leer(&self) -> bool {
        self.min_breite.is_none()
            && self.max_breite.is_none()
            && self.min_hoehe.is_none()
            && self.max_hoehe.is_none()
    }
}

/// Die Nebentabelle: `NodeId -> LayoutStil`, nach `NodeId` sortiert.
///
/// SORTIERT statt `HashMap`, und der Grund ist nicht Aesthetik: der
/// Plan wird je Knoten genau einmal gelesen, in Dokumentreihenfolge. Ein
/// sortierter `Vec` gibt das mit einer Binaersuche in O(log n) ohne
/// Hash-Bombe und ohne Zufallsordnung; eine `HashMap` muesste man
/// aufbauen, was dieselbe Arbeit ohne den Vorteil taete.
///
/// # DIE ZWEITE TABELLE
///
/// `grenzen: Vec<(u32, Grenzen)>` traegt `min-width`/`max-width`/
/// `min-height`/`max-height` — dieselbe Form, derselbe Grund. Sie ist
/// getrennt von `eintraege`, weil `LayoutStil` an mehreren Stellen als
/// Struct-Literal gebaut wird (siehe [`Grenzen`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayoutPlan {
    /// Nach `NodeId.0` sortiert.
    eintraege: Vec<(u32, LayoutStil)>,
    /// Nach `NodeId.0` sortiert; leer = keine Beschraenkung irgendwo.
    ///
    /// M3/M4: `MAX_GRENZEN` begrenzt die Zahl, und `verworfen` zaehlt
    /// die, die nicht mehr hineinpassten (M7: mitzaehlen).
    grenzen: Vec<(u32, Grenzen)>,
    /// `vertical-align` je Knoten (P9). Eigene Tabelle aus demselben
    /// Grund wie `Grenzen`: `layout_stil` baut ein vollstaendiges
    /// Struct-Literal.
    ///
    /// Nur Knoten mit einem ABWEICHENDEN Wert stehen hier. Die
    /// allermeisten Elemente erben die Grundlinie, und eine Tabelle aus
    /// 200 000 Grundlinien waere Speicher fuer nichts.
    vertikal: Vec<(u32, Vertikal)>,
    /// Die Knoten mit `Rtl` (K17b). NUR die Ausnahmen: `Ltr` ist der
    /// Default und steht hier nicht. Auf einer deutschen Seite ist diese
    /// Tabelle leer, auf einer arabischen hat sie einen Eintrag je Knoten
    /// — gemessen ist das die kleinste moegliche Darstellung.
    richtung: Vec<(u32, Richtung)>,
    /// P19 (20:35): die Prozentanteile der vier Grenzen, je Knoten.
    grenzen_prozent: Vec<(u32, GrenzenProzent)>,
    /// Die Knoten mit einer Ausrichtung ungleich `Links` (K17b). Wie
    /// `richtung` eine Ausnahmeliste: `Links` ist der Default.
    ausrichtung: Vec<(u32, Textausrichtung)>,
    /// Release 11.10.2026 (Agent CSS2): die Tabellenteile mit ihrer genauen
    /// Rolle (siehe [`TabellenAngabe`]). Ausnahmeliste wie `vertikal`.
    tabellen: Vec<(u32, TabellenAngabe)>,
    /// Wie viele Eintraege verworfen wurden (M7: mitzaehlen, nicht
    /// schweigend schlucken).
    verworfen: usize,
    /// Release 15.10.2026: see [`NodeIndex`].
    index: NodeIndex,
}

/// Release 15.10.2026: the position of each node's entry in
/// `LayoutPlan::eintraege` (`u32::MAX`: none), built once for a finished
/// plan ([`LayoutPlan::build_index`], at the start of a layout) and dropped
/// by every insertion. The layout reads the plan for every node in every
/// pass (`Ctx::stil`); MEASURED before: a binary search per call
/// (`LayoutPlan::hole`) was among the hottest frames of the layout of
/// https://www.chip.de/ (cdb samples, `tabellen_leg::ist_tabelle` from the
/// intrinsic width passes). Plans compare equal whether or not it is built.
#[derive(Debug, Clone, Default)]
struct NodeIndex(std::sync::OnceLock<Vec<u32>>);

impl PartialEq for NodeIndex {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl LayoutPlan {
    /// Ein leerer Plan: jeder Knoten ist ein Block. Das ist das Verhalten
    /// ohne Flex/Grid/Position und der Ausgangspunkt fuer Tests.
    pub fn leer() -> LayoutPlan {
        LayoutPlan {
            eintraege: Vec::new(),
            grenzen: Vec::new(),
            vertikal: Vec::new(),
            richtung: Vec::new(),
            grenzen_prozent: Vec::new(),
            ausrichtung: Vec::new(),
            tabellen: Vec::new(),
            verworfen: 0,
            index: NodeIndex::default(),
        }
    }

    /// Release 11.10.2026 (Agent CSS2): traegt die Tabellen-Angaben eines
    /// Knotens ein (M3: hoechstens [`MAX_GRENZEN`] Knoten).
    pub fn set_tabelle(&mut self, knoten: NodeId, t: TabellenAngabe) {
        match self.tabellen.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => {
                if t.ist_standard() {
                    self.tabellen.remove(stelle);
                } else {
                    self.tabellen[stelle].1 = t;
                }
            }
            Err(stelle) => {
                if t.ist_standard() {
                    return;
                }
                if self.tabellen.len() >= MAX_GRENZEN {
                    self.verworfen += 1;
                    return;
                }
                self.tabellen.insert(stelle, (knoten.0, t));
            }
        }
    }

    /// Release 11.10.2026 (Agent CSS2): die Tabellen-Angaben eines Knotens
    /// (Standard: keine Rolle).
    pub fn tabelle(&self, knoten: NodeId) -> TabellenAngabe {
        self.tabellen
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
            .map(|i| self.tabellen[i].1)
            .unwrap_or_default()
    }

    /// Wie viele Eintraege der Plan hat (ohne die verworfenen).
    pub fn len(&self) -> usize {
        self.eintraege.len()
    }

    /// Leer? (`ist_leer` ist die Projekt-Konvention, `is_empty` der
    /// Beiname, den clippy und die std verlangen — beide sagen dasselbe.)
    pub fn ist_leer(&self) -> bool {
        self.eintraege.is_empty()
    }

    /// Leer? — die Form, die die `std` und clippy erwarten.
    pub fn is_empty(&self) -> bool {
        self.ist_leer()
    }

    /// Wie viele Eintraege die Grenze [`MAX_PLAN_KNOTEN`] gerissen hat.
    ///
    /// GEMESSEN wichtig: 0 heisst "nichts ging verloren", und das ist ein
    /// Unterschied, den der Benutzer sehen muss. Ein stillschweigend
    /// gekuerzter Plan sieht aus wie "diese Seite hat kein Flex".
    pub fn verworfen(&self) -> usize {
        self.verworfen
    }

    /// Traegt die vier Beschraenkungen eines Knotens ein.
    ///
    /// Ein leeres [`Grenzen`] wird NICHT eingetragen (siehe
    /// [`Grenzen::ist_leer`]).
    /// P9: `vertical-align` eines Knotens setzen.
    ///
    /// Nur ABWEICHENDE Werte werden gespeichert (`Vertikal::
    /// ist_standard`). Der Anfangswert ist damit "nicht in der
    /// Tabelle" — das ist sparsamer als eine Tabelle aus Grundlinien
    /// und verlangt beim Lesen keinen Default.
    pub fn set_vertikal(&mut self, knoten: NodeId, v: Vertikal) {
        if v.ist_standard() {
            self.entferne_vertikal(knoten);
            return;
        }
        match self.vertikal.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => self.vertikal[stelle].1 = v,
            Err(stelle) => {
                if self.vertikal.len() >= MAX_GRENZEN {
                    self.verworfen += 1;
                    return;
                }
                self.vertikal.insert(stelle, (knoten.0, v));
            }
        }
    }

    /// `vertical-align` eines Knotens, oder `Grundlinie`.
    pub fn vertikal(&self, knoten: NodeId) -> Vertikal {
        self.vertikal
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
            .map(|i| self.vertikal[i].1)
            .unwrap_or_default()
    }

    fn entferne_vertikal(&mut self, knoten: NodeId) {
        if let Ok(stelle) = self.vertikal.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            self.vertikal.remove(stelle);
        }
    }

    pub fn set_grenzen(&mut self, knoten: NodeId, g: Grenzen) {
        if g.ist_leer() {
            return;
        }
        match self.grenzen.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => self.grenzen[stelle].1 = g,
            Err(stelle) => {
                if self.grenzen.len() >= MAX_GRENZEN {
                    self.verworfen += 1;
                    return;
                }
                self.grenzen.insert(stelle, (knoten.0, g));
            }
        }
    }

    /// Die Beschraenkungen eines Knotens; `Grenzen::default()` heisst
    /// "keine".
    pub fn grenzen(&self, knoten: NodeId) -> Grenzen {
        match self.grenzen.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => self.grenzen[stelle].1,
            Err(_) => Grenzen::default(),
        }
    }

    /// Wie viele Knoten eine Beschraenkung tragen (nach `set_grenzen`).
    pub fn grenzen_anzahl(&self) -> usize {
        self.grenzen.len()
    }

    /// P19 (20:35): die Prozentanteile der vier Grenzen eines Knotens.
    ///
    /// Ein leeres Ergebnis heisst „nichts eingetragen" — der Normalfall.
    pub fn grenzen_prozent(&self, knoten: NodeId) -> GrenzenProzent {
        match self
            .grenzen_prozent
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
        {
            Ok(stelle) => self.grenzen_prozent[stelle].1,
            Err(_) => GrenzenProzent::default(),
        }
    }

    /// P19 (20:35): traegt die Prozentanteile fuer einen Knoten ein.
    ///
    /// Ist `p` leer (keine der vier Angaben ist ein Prozent), wird der
    /// Eintrag **entfernt** statt angelegt: eine leere Zeile im Plan ist
    /// eine Zeile ohne Aussage, und der Plan wird fuer jede Seite
    /// gebaut. M4: nach [`MAX_GRENZEN_PROZENT`] wird nichts mehr
    /// angenommen und gezaehlt — dieselbe Regel wie bei `set_grenzen`.
    pub fn set_grenzen_prozent(&mut self, knoten: NodeId, p: GrenzenProzent) {
        if p.ist_leer() {
            if let Ok(stelle) = self
                .grenzen_prozent
                .binary_search_by_key(&knoten.0, |(k, _)| *k)
            {
                self.grenzen_prozent.remove(stelle);
            }
            return;
        }
        match self
            .grenzen_prozent
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
        {
            Ok(stelle) => self.grenzen_prozent[stelle].1 = p,
            Err(stelle) => {
                if self.grenzen_prozent.len() >= MAX_GRENZEN_PROZENT {
                    self.verworfen += 1;
                    return;
                }
                self.grenzen_prozent.insert(stelle, (knoten.0, p));
            }
        }
    }

    /// Wie viele Knoten Prozentgrenzen tragen (P19, 20:35).
    pub fn grenzen_prozent_anzahl(&self) -> usize {
        self.grenzen_prozent.len()
    }

    /// Setzt die Schreibrichtung eines Knotens (K17b).
    ///
    /// NUR `Rtl` wird gespeichert. `Ltr` ist der Default, und ein
    /// Eintrag dafuer waere eine Zeile ohne Aussage: der Plan wird fuer
    /// jede Seite gebaut, und 99 % der Knoten sind `ltr`.
    ///
    /// M4: nach [`MAX_RICHTUNGEN`] wird nichts mehr angenommen und
    /// gezaehlt — dieselbe Regel wie bei `set_grenzen`.
    pub fn set_richtung(&mut self, knoten: NodeId, r: Richtung) {
        match r {
            Richtung::Ltr => {
                // Ein ausdrueckliches `ltr` auf einem `rtl`-Knoten
                // muss die Angabe wieder entfernen, sonst gewaenne der
                // Plan nach einem `dir="ltr"`-Kind weiter nach rechts.
                if let Ok(stelle) = self.richtung.binary_search_by_key(&knoten.0, |(k, _)| *k) {
                    self.richtung.remove(stelle);
                }
            }
            Richtung::Rtl => match self.richtung.binary_search_by_key(&knoten.0, |(k, _)| *k) {
                Ok(stelle) => self.richtung[stelle].1 = r,
                Err(stelle) => {
                    if self.richtung.len() >= MAX_RICHTUNGEN {
                        self.verworfen += 1;
                        return;
                    }
                    self.richtung.insert(stelle, (knoten.0, r));
                }
            },
        }
    }

    /// Die Schreibrichtung eines Knotens. `Richtung::Ltr` heisst
    /// „nichts eingetragen" — das ist der Normalfall.
    pub fn richtung(&self, knoten: NodeId) -> Richtung {
        match self.richtung.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => self.richtung[stelle].1,
            Err(_) => Richtung::Ltr,
        }
    }

    /// Wie viele Knoten `Rtl` tragen (nach `set_richtung`).
    pub fn richtung_anzahl(&self) -> usize {
        self.richtung.len()
    }

    /// Setzt die Textausrichtung eines Knotens (K17b).
    ///
    /// NUR Ausrichtungen ungleich `Links` werden gespeichert — dieselbe
    /// Ausnahmeliste wie bei `richtung`. `Links` auf einem previously
    /// gesetzten Knoten entfernt den Eintrag wieder.
    pub fn set_ausrichtung(&mut self, knoten: NodeId, a: Textausrichtung) {
        if a == Textausrichtung::Links {
            if let Ok(stelle) = self
                .ausrichtung
                .binary_search_by_key(&knoten.0, |(k, _)| *k)
            {
                self.ausrichtung.remove(stelle);
            }
            return;
        }
        match self
            .ausrichtung
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
        {
            Ok(stelle) => self.ausrichtung[stelle].1 = a,
            Err(stelle) => {
                if self.ausrichtung.len() >= MAX_RICHTUNGEN {
                    self.verworfen += 1;
                    return;
                }
                self.ausrichtung.insert(stelle, (knoten.0, a));
            }
        }
    }

    /// Die Textausrichtung eines Knotens; `Links` heisst „nichts
    /// eingetragen".
    pub fn ausrichtung(&self, knoten: NodeId) -> Textausrichtung {
        match self
            .ausrichtung
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
        {
            Ok(stelle) => self.ausrichtung[stelle].1,
            Err(_) => Textausrichtung::Links,
        }
    }

    /// Wie viele Knoten eine Ausrichtung ungleich `Links` tragen.
    pub fn ausrichtung_anzahl(&self) -> usize {
        self.ausrichtung.len()
    }

    /// Traegt einen Knoten ein.
    ///
    /// M3/M4: nach [`MAX_PLAN_KNOTEN`] wird nichts mehr angenommen und
    /// gezaehlt. Ein spaeterer Eintrag fuer DASSELBE Knoten ersetzt den
    /// frueheren (letztes Gewinnt) — das ist die Reihenfolge, in der die
    /// Kaskade arbeitet.
    pub fn set(&mut self, knoten: NodeId, stil: LayoutStil) {
        // M3: die Suche ist auf die vorhandene Zahl begrenzt, ein
        // `Vec::binary_search_by_key` ueber 200 000 Eintraege ist schnell,
        // aber die Zahl der AUFRUFE ist es, die begrenzt werden muss.
        match self.eintraege.binary_search_by_key(&knoten.0, |(k, _)| *k) {
            Ok(stelle) => self.eintraege[stelle].1 = stil,
            Err(stelle) => {
                if self.eintraege.len() >= MAX_PLAN_KNOTEN {
                    self.verworfen += 1;
                    return;
                }
                self.eintraege.insert(stelle, (knoten.0, stil));
                self.index = NodeIndex::default();
            }
        }
    }

    /// Release 15.10.2026: builds the node index of the finished plan (see
    /// [`NodeIndex`]); `hole` uses it from then on. Idempotent.
    pub fn build_index(&self) {
        self.index.0.get_or_init(|| {
            let len = self.eintraege.last().map_or(0, |(k, _)| *k as usize + 1);
            let mut ix = vec![u32::MAX; len];
            for (stelle, (k, _)) in self.eintraege.iter().enumerate() {
                ix[*k as usize] = stelle as u32;
            }
            ix
        });
    }

    /// Release 11.10.2026: alle eingetragenen Stile (ohne Knotennummer).
    pub fn eintraege(&self) -> impl Iterator<Item = &LayoutStil> {
        self.eintraege.iter().map(|(_, s)| s)
    }

    /// Der Stil eines Knotens, oder `None` (dann: Block).
    pub fn hole(&self, knoten: NodeId) -> Option<&LayoutStil> {
        if let Some(ix) = self.index.0.get() {
            let stelle = *ix.get(knoten.0 as usize)?;
            return self.eintraege.get(stelle as usize).map(|(_, s)| s);
        }
        self.eintraege
            .binary_search_by_key(&knoten.0, |(k, _)| *k)
            .ok()
            .map(|stelle| &self.eintraege[stelle].1)
    }

    /// Der Stil eines Knotens, oder der Standard (Block im Fluss).
    ///
    /// Das ist die Form, die das Layout braucht: 99,9 % aller Knoten
    /// stehen nicht im Plan, und fuer die soll kein `match` im Code
    /// stehen, sondern ein einziger Aufruf.
    pub fn stil_oder_standard(&self, knoten: NodeId) -> &LayoutStil {
        self.hole(knoten).unwrap_or(&STANDARD)
    }
}

/// Der Standardstil als Konstante, damit [`LayoutPlan::stil_oder_standard`]
/// nichts alloziert. `OnceLock` ist hier erlaubt (es ist keinwachsender
/// Cache, sondern ein einmal gesetzter unabhaengiger Wert) — aber ein
/// `static` mit `Sync`-Anforderung ist unnoetig: der Wert ist `Copy`-bar
/// klein genug fuer eine Funktion, die ihn zurueckgibt.
static STANDARD: LayoutStil = LayoutStil {
    border_box: false,
    inline_aussen: false,
    flow_root: false,
    cb_inline: false,
    lanes: 0,
    lanes_toleranz: 16.0,
    lanes_umkehr: 0,
    ordnung: 0,
    transform: None,
    hintergrund_clip: 0,
    gitter: None,
    gitter_kind: None,
    groessen_art: [0; 4],
    stretch_bits: 0,
    height_keyword_limits: 0,
    fit_content_arg: [None; 3],
    size_contained: 0,
    vertical_writing: 0,
    contain_size: [None, None],
    flex: None,
    flex_basis_art: crate::flexstil::FlexBasis::Auto,
    rollkasten: false,
    rtl: false,
    // L4 (03.10.2026 09:5x): §6.6 — der Anfangswert ist `stretch`, und
    // `stretch` ist das, was die Platzierung ohnehin rechnet.
    selbst_vertikal: None,
    selbst_waagerecht: None,
    justify_ausrichtung: Ausrichtung::Strecke,
    breite_angabe: None,
    hoehe_angabe: None,
            breite_anteil: None,
            hoehe_anteil: None,
            hoehe_bedingt: None,
            breite_inhalt: false,
            hoehe_inhalt: false,
    schriftgrad: 16.0,
    anzeige: Anzeige::Block,
    flex_richtung: FlexRichtung::Reihe,
    spaltenfluss: false,
    // L4 (04:2x): `grid-auto-flow: … dense` (§8.5).
    dicht: false,
    // L4 (19:4x): `grid-template-rows` (CSS Grid 1 §8.3). Leer =
    // implizite Zeilen, also `auto` (der Anfangswert, §7.5).
    zeilen: Vec::new(),
    // L4 (02.10.2026 21:6x): `grid-auto-rows` (§7.5) und
    // `grid-auto-columns` (§7.3). Leer = `auto`, also der Inhalt.
    auto_zeilen: Vec::new(),
    auto_spalten: Vec::new(),
    // L4 (02.10.2026 22:5x): `span` (§8.3). `0` bedeutet eine Spur —
    // `gitter_linie` liefert `0` fuer `auto`, und eine Zeilenzahl von 0
    // waere ein Item ohne Zeile.
    spann_zeilen: 0,
    spann_spalten: 0,
    // L4 (01:6x): die festen Linien (§8.5), roh und 1-basiert.
    zeile_start: 0,
    zeile_ende: 0,
    spalte_start: 0,
    spalte_ende: 0,
    // L4 (02:7x): `grid-template-areas` (§8.1).
    gebiete: Vec::new(),
    // L4 (02:8x): der Name des Kindes (§8.4).
    gebiet: Gebiet::LEER,
    transformt: false,
    flex_umgekehrt: false,
    flex_umbruch: FlexUmbruch::Keiner,
    flex_buendig: Buendig::Start,
    flex_ausrichtung: Ausrichtung::Start,
    flex_wachstum: 0.0,
    flex_schrumpf: 1.0,
    flex_basis: None,
    luecke: 0.0,
    luecke_quer: 0.0,
    spalten: Vec::new(),
    position: PositionArt::Statisch,
    oben: None,
    rechts: None,
    unten: None,
    links: None,
    oben_prozent: None,
    rechts_prozent: None,
    unten_prozent: None,
    links_prozent: None,
    z_index: None,
    schweben: FloatArt::Keins,
    raeumen: RaeumungArt::Keins,
    ueberlauf: UeberlaufArt::Sichtbar,
    marge_links_auto: false,
    marge_rechts_auto: false,
    marge_oben_auto: false,
    marge_unten_auto: false,
    seitenverhaeltnis: 0.0,
    // BILD_29: dieselben Vorgaben wie in `standard()`. `String::new()` ist
    // `const`-stabil, also geht es in einem `static`.
    objekt_passung: String::new(),
    objekt_position: String::new(),
    // BILD_37 (fremde Datei, **minimale** Reparatur, Rolle SERVER im BRETT
    // informiert): `colspan` und `rowspan` waren am 05.10. **zwei Tage vor
    // BILD_37** (SERVER_13) zu `LayoutStil` **hinzugefuegt** worden, ohne dass
    // dieser `static` mitwanderte — GEMESSEN: `error[E0063]: missing fields
    // \`colspan\` and \`rowspan\``, und damit war **jede** Crate rot, die von
    // `narsil-layout` abhaengt (auch meine: `narsil-raster` haengt ueber
    // `narsil-page` daran, also blockierte der Fehler **meinen** Testlauf).
    // Die Vorgaben stehen in der Doku der Felder: „`rowspan`: **`1` =
    // normale Zeile**", und `0` wird ohnehin als `1` behandelt.
    colspan: 1,
    rowspan: 1,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ref_and_node_index_agree_with_the_plain_lookups() {
        // Release 15.10.2026: the borrowed default is the default style, and
        // the node index gives the same entries as the binary search, also
        // after a later insertion drops it.
        assert_eq!(*LayoutStil::default_ref(), LayoutStil::standard());
        let mut p = LayoutPlan::leer();
        p.set(NodeId(9), LayoutStil::flex(FlexRichtung::Spalte));
        p.set(NodeId(3), LayoutStil::flex(FlexRichtung::Reihe));
        p.build_index();
        assert_eq!(p.hole(NodeId(9)).map(|s| s.flex_richtung), Some(FlexRichtung::Spalte));
        assert_eq!(p.hole(NodeId(3)).map(|s| s.flex_richtung), Some(FlexRichtung::Reihe));
        assert!(p.hole(NodeId(4)).is_none() && p.hole(NodeId(400)).is_none());
        p.set(NodeId(5), LayoutStil::flex(FlexRichtung::Spalte));
        assert_eq!(p.hole(NodeId(9)).map(|s| s.flex_richtung), Some(FlexRichtung::Spalte));
        assert_eq!(p.hole(NodeId(5)).map(|s| s.flex_richtung), Some(FlexRichtung::Spalte));
        p.build_index();
        assert_eq!(p.hole(NodeId(3)).map(|s| s.flex_richtung), Some(FlexRichtung::Reihe));
    }

    #[test]
    fn leerer_plan_ist_standard() {
        let p = LayoutPlan::leer();
        assert!(p.ist_leer());
        assert_eq!(p.len(), 0);
        assert_eq!(p.verworfen(), 0);
        let s = p.stil_oder_standard(NodeId(7));
        assert_eq!(s.anzeige, Anzeige::Block);
        assert!(s.ist_im_fluss());
        assert!(s.wird_gezeichnet());
    }

    #[test]
    fn set_und_hole_rund_um() {
        let mut p = LayoutPlan::leer();
        p.set(NodeId(5), LayoutStil::flex(FlexRichtung::Reihe));
        p.set(NodeId(1), LayoutStil::flex(FlexRichtung::Spalte));
        p.set(
            NodeId(9),
            LayoutStil::raster(&[Spalte::Fest(100.0), Spalte::Anteil(1.0)]),
        );
        assert_eq!(p.len(), 3);
        assert!(!p.ist_leer());
        // Unsortiert eingetragen, trotzdem richtige Antwort.
        assert!(p.hole(NodeId(1)).unwrap().ist_flex());
        assert_eq!(
            p.hole(NodeId(1)).unwrap().flex_richtung,
            FlexRichtung::Spalte
        );
        assert!(p.hole(NodeId(9)).unwrap().ist_raster());
        assert_eq!(p.hole(NodeId(9)).unwrap().spalten.len(), 2);
        // Ein Knoten, der nicht drin ist.
        assert!(p.hole(NodeId(4)).is_none());
        assert_eq!(p.stil_oder_standard(NodeId(4)).anzeige, Anzeige::Block);
    }

    #[test]
    fn letztes_gewinnt_beim_doppeleintrag() {
        let mut p = LayoutPlan::leer();
        p.set(NodeId(3), LayoutStil::flex(FlexRichtung::Reihe));
        p.set(NodeId(3), LayoutStil::flex(FlexRichtung::Spalte));
        assert_eq!(p.len(), 1, "derselbe Knoten darf nicht doppelt drin sein");
        assert_eq!(
            p.hole(NodeId(3)).unwrap().flex_richtung,
            FlexRichtung::Spalte
        );
    }

    #[test]
    fn plan_grenze_zaehlt_statt_verschluckt() {
        // M7: die Zahl der verworfenen Eintraege steht zur Verfuegung.
        let mut p = LayoutPlan::leer();
        for i in 0..MAX_PLAN_KNOTEN {
            p.set(NodeId(i as u32), LayoutStil::standard());
        }
        assert_eq!(p.len(), MAX_PLAN_KNOTEN);
        assert_eq!(p.verworfen(), 0);
        p.set(NodeId(999_999), LayoutStil::standard());
        p.set(NodeId(999_998), LayoutStil::standard());
        assert_eq!(p.verworfen(), 2, "beide muessen gezaehlt sein");
        assert_eq!(p.len(), MAX_PLAN_KNOTEN);
    }

    #[test]
    fn raster_schneidet_ueber_die_spaltengrenze() {
        // M3/M4: 100 Spalten -> 32. Nicht 0 (dann waere die Seite leer).
        let viele: Vec<Spalte> = (0..100).map(|_| Spalte::Anteil(1.0)).collect();
        let s = LayoutStil::raster(&viele);
        assert_eq!(s.spalten.len(), MAX_RASTER_SPALTEN);
        assert!(s.ist_raster());
    }

    #[test]
    fn absolute_und_fest_verlassen_den_fluss() {
        // CSS 2.1, 9.5: sie belegen keinen Platz fuer die Geschwister.
        for p in [PositionArt::Absolut, PositionArt::Fest] {
            let s = LayoutStil {
                position: p,
                ..LayoutStil::standard()
            };
            assert!(!s.ist_im_fluss(), "{p:?} muss aus dem Fluss");
        }
        for p in [
            PositionArt::Statisch,
            PositionArt::Relativ,
            PositionArt::Klebend,
        ] {
            let s = LayoutStil {
                position: p,
                ..LayoutStil::standard()
            };
            assert!(s.ist_im_fluss(), "{p:?} muss im Fluss bleiben");
        }
    }

    #[test]
    fn ueberlauf_beschneidet_dreimal_von_vier() {
        assert!(!UeberlaufArt::Sichtbar.beschneidet());
        assert!(UeberlaufArt::Versteckt.beschneidet());
        assert!(UeberlaufArt::Rollen.beschneidet());
        assert!(UeberlaufArt::Automatisch.beschneidet());
    }

    #[test]
    fn standard_ist_gleich_default() {
        assert_eq!(LayoutStil::standard(), LayoutStil::default());
    }
}
