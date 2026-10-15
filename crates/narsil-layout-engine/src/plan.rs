//! DIE PLATZIERUNG: Flex, Raster, Position, Float (Rolle LAYOUT, W7 L1–L4).
//!
//! # WO DIESE PHASE EINGREIFT
//!
//! Die Messphase ([`crate::box_sizing::compute_sizes`]) rechnet fuer jeden
//! Knoten seine eigene Groesse — die bleibt, wie sie ist. Die
//! **Platzierung** dieser Phase setzt die Kinder eines Containers an ihre
//! Stelle. Und genau da unterscheiden sich die Anzeigearten:
//!
//! | `display`      | Kinder werden…                                   |
//! |----------------|--------------------------------------------------|
//! | Block (heute)  | linear mit einem Cursor (Column/Row)             |
//! | **flex** (L1)  | mit `lege_flex_zeilen` verteilt                  |
//! | **grid** (L2)  | mit `lege_raster` verteilt                       |
//!
//! Der Aufbau ist bewusst so geschnitten, dass **ohne Plan nichts
//! anders laeuft**: [`crate::layout`] ruft [`crate::layout_mit_plan`]
//! mit [`LayoutPlan::leer`], und ein leerer Plan ergibt fuer jeden
//! Knoten `display:block`. Das ist exakt der Zustand vom 30.09.2026,
//! vor diesem Modul.
//!
//! # DIE FUNKTIONEN SIND DIESELBEN WIE IM TEST
//!
//! `lege_flex`, `lege_raster`, `absolut_waagerecht`,
//! `absolut_senkrecht`, `relativ_versatz`, `float_x`, `clear_y`,
//! `beschnitt`, `enthaltender_block` und `mal_reihenfolge` kommen aus
//! `narsil-css`. Die von der Koordination geprueften Zahlen gelten damit
//! auch im echten Layout — es gibt keine zweite, abweichende Rechnung.
//! Das ist der ganze Grund, warum sie in `narsil-css` stehen.
//!
//! # SPEICHERREGELN (M3/M4)
//!
//! * Jede Schleife ueber Kinder ist an [`MAX_KINDER`] gedeckelt.
//! * Allokationen aus Eingabegroesse werden VOR dem Anlegen geprueft:
//!   `Vec::with_capacity(min(n, MAX))`.
//! * Keine Rekursion in diesem Modul (M5); der Aufrufer
//!   ([`crate::flow`]) iteriert.

use narsil_core::geometry::{Point, Rect, Size};
use narsil_layout::plan::{
    FlexRichtung, FloatArt, MAX_FLEX_KINDER, PositionArt, RaeumungArt, Spalte, UeberlaufArt,
};

use narsil_css::{
    FlexAusrichtung, FlexBuendig, FlexKind, Float, KindQuer, Raeumung, RasterKind, Rechteck,
    Schicht, SpaltenMass, Ueberlauf, Vorfahr, absolut_senkrecht, absolut_waagerecht,
    beschnitt_ueberlauf, clear_y, enthaltender_block, float_x, lege_flex_zeilen,
    lege_raster_spuren, mal_reihenfolge, relativ_versatz,
};

/// Die Plan-Spalte in die Raster-Spalte von `narsil-css`, auch fuer die
/// verschachtelte `Auto`-Spur.
///
/// L4 (08:4x): `Auto` traegt die Spur in demselben Typ, also muss der
/// Weg rekursiv sein. GEMESSENE ENTSCHEIDUNG, warum rekursiv und nicht
/// flach: `repeat(auto-fill, …)` fasst in CSS eine Spur auf (kein
/// `repeat(auto-fill, 1fr 2fr)`), aber die verschachtelte Spur kann
/// selbst eine `Auto` sein, wenn jemand
/// `repeat(auto-fill, repeat(auto-fill, 100px))` schreibt —
/// syntaktisch zulaessig. Die Rekursion faengt das fuer den Preis
/// einer Zeile.
fn bridge_spalte(s: &Spalte) -> SpaltenMass {
    match s {
        Spalte::Fest(v) => SpaltenMass::Fest(*v),
        Spalte::Anteil(f) => SpaltenMass::Anteil(*f),
        // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) — dieselbe
        // Form wie `Anteil`: eine Zahl, die unveraendert durchwandert.
        Spalte::Inhalt => SpaltenMass::Inhalt,
        // L4 (09:3x): §10.1 — der Faktor kommt an und wird im Rasterfluss
        // gegen die Containerbreite gerechnet.
        Spalte::Prozent(f) => SpaltenMass::Prozent(*f),
        Spalte::Auto { spur, fit } => SpaltenMass::Auto {
            spur: Box::new(bridge_spalte(spur)),
            fit: *fit,
        },
        // L4 (08:6x): `minmax` wandert wie `Auto` durch.
        Spalte::MinMax { min, max } => SpaltenMass::MinMax {
            min: *min,
            max: Box::new(bridge_spalte(max)),
        },
    }
}

// ===========================================================================
// GRENZEN (M3/M4)
// ===========================================================================

/// Hoechste Kinder je Container, die diese Phase anfasst. Begruendung:
/// ein Container hat in einer echten Seite 2–50 Kinder; 4 096 deckt jede
/// ab und haelt die Allokation einer Zeile bei 4 096 x 40 Byte =
/// 160 KiB. Ohne diese Zahl waere `Vec::with_capacity(kinder.len())`
/// aus der Eingabe eine Bombe.
pub const MAX_KINDER: usize = 4_096;

/// Hoechste px-Zahl, die diese Phase durchlaesst. Begruendung: siehe
/// `narsil_css::MAX_WERT_PX` — dieselbe Zahl, damit Kaskade und Layout
/// nicht an verschiedenen Grenzen arbeiten (M4).
pub const MAX_WERT_PX: f32 = 100_000.0;

/// Hoechste px-Zahl fuer eine **POSITION** (x/y eines Kastens).
///
/// P12 (05.10.2026, 14:22): `sauber()` deckelt auf `MAX_WERT_PX`
/// (100 000). Fuer Laengen ist das richtig (M4): eine Bombe darf keine
/// 1 000 000 000 px hohe Box bauen. Fuer POSITIONEN ist es falsch, und
/// es hat auf heise.de sichtbar zugeschlagen:
///
/// ```text
/// BAND #1429 [ 92.00,100000.00  97.87x16.80] eltern=(1428, 92.0, 105487.20)
/// BAND #1933 [108.00,100004.00 221.41x16.00] eltern=(1932, 104.0, 161645.80)
/// BAND #1972 [128.00,100000.00  21.72x28.80] eltern=(1971, 116.0, 162729.19)
/// BAND #2023 [ 92.00,100000.00 134.50x16.80] eltern=(2022, 92.0, 169027.19)
/// ```
///
/// Die Eltern stehen bei y = 105 487, 161 645, 162 729, 169 027, die
/// Kinder ALLE bei y = 100 000. Genau die Obergrenze. Die Seite ist
/// 169 027 px hoch (das ist selbst ein Befund, siehe
/// team/PROBLEME.txt 14:17), und alles unterhalb von 100 000 px wird
/// auf DIESELBE Zeile gelegt — mit Folgen, die man sieht: doppelt
/// gezeichneter Text und fuenf harte `text-ueber-text` auf heise.de.
///
/// Ein Deckel, der Inhalt UMSORTIERT, ist schlimmer als ein grosser
/// Wert: ein Kasten bei y = 4 000 000 ist harmlos (die Zahlen bleiben
/// genau, das Rasterisieren schneidet ohnehin den Ausschnitt), ein
/// Kasten, der auf 100 000 springt, liegt sichtbar falsch.
///
/// 64 x `MAX_WERT_PX` = 6 400 000 px. Das ist keine echte Seitenhöhe,
/// aber es ist genug, dass die Obergrenze nie in einem Lauf greift,
/// und f32 hat dort noch rund 0,5 px Aufloesung.
pub const MAX_LAGE_PX: f32 = MAX_WERT_PX * 64.0;

/// Hoechste Schritte beim Float-Umbruch (M3). Begruendung: die Kanten
/// `unten_links`/`unten_rechts` wachsen monoton, und jeder Schritt setzt
/// die Position mindestens auf eine davon — die Schleife kommt also an
/// ein Ende. Sechs Schritte sind fuer jede echte Seite reichlich; danach
/// ist es ein Fehler in den Daten, und der bricht ab statt zu laufen.
// Release 12.10.2026 (Agent CSS2): 32 statt 6 — mit der Float-Liste rueckt
// jeder Schritt nur bis zur naechsten Unterkante im Band.
const MAX_UMBRUECHE: usize = 32;

/// Saeubert eine Laenge: nicht-endlich wird 0, Uebermass wird gekappt.
fn sauber(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, MAX_WERT_PX)
    } else {
        0.0
    }
}

/// Saeubert eine **POSITION** (x/y eines Kastens).
///
/// P12 (14:22): eigener Deckel `MAX_LAGE_PX` statt `MAX_WERT_PX`, weil
/// ein zu kleiner Deckel Inhalt umsortiert statt ihn zu begrenzen (die
/// Messung steht bei `MAX_LAGE_PX`). Nicht-endlich wird 0, wie bei
/// jeder anderen Zahl.
fn sauber_lage(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(-MAX_LAGE_PX, MAX_LAGE_PX)
    } else {
        0.0
    }
}

/// Saeubert eine Laenge, die negativ sein darf (`left`, `top`, …).
fn sauber_versatz(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(-MAX_WERT_PX, MAX_WERT_PX)
    } else {
        0.0
    }
}

// ===========================================================================
// EINE PLATZIERUNG
// ===========================================================================

/// Wo ein Kind steht, wenn die Phase fertig ist.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Platzierung {
    /// Linke obere Ecke des Rahmens (inklusive Padding, ohne Margin).
    pub x: f32,
    /// Obere Kante.
    pub y: f32,
    /// Breite. 0 = das Kind belegt nichts.
    pub breite: f32,
    /// Hoehe.
    pub hoehe: f32,
}

impl Platzierung {
    /// Alles Null: fuer Kinder, die nicht gebaut werden.
    pub const WEG: Platzierung = Platzierung {
        x: 0.0,
        y: 0.0,
        breite: 0.0,
        hoehe: 0.0,
    };

    /// Als `Rect` — die Form, die `LayoutBox::border_box` will.
    pub fn rect(self) -> Rect {
        Rect::from_origin_size(
            Point::new(self.x, self.y),
            Size::new(self.breite, self.hoehe),
        )
    }

    /// Die Groesse als `Size` (ohne Position).
    pub fn size(self) -> Size {
        Size::new(self.breite, self.hoehe)
    }

    /// Verschiebt die Platzierung um (dx, dy).
    pub fn verschiebe(&mut self, dx: f32, dy: f32) {
        self.x = sauber_versatz(self.x + dx);
        self.y = sauber_versatz(self.y + dy);
    }

    /// Setzt Breite und Hoehe (mit Kappung).
    pub fn setze_masse(&mut self, breite: f32, hoehe: f32) {
        self.breite = sauber(breite);
        self.hoehe = sauber(hoehe);
    }
}

// ===========================================================================
// L1 — FLEX
// ===========================================================================

/// Die Basis eines Flex-Kindes auf der Hauptachse.
///
/// Das ist `flex-basis`; sonst die eigene Breite (bei `row`) bzw. Hoehe
/// (bei `column`); sonst 0, weil ein Kind ohne Inhalt nichts belegt.
pub fn flex_basis(
    basis_px: Option<f32>,
    eigene_breite: f32,
    eigene_hoehe: f32,
    richtung: FlexRichtung,
) -> f32 {
    if let Some(b) = basis_px.filter(|v| v.is_finite()) {
        return sauber(b);
    }
    match richtung {
        FlexRichtung::Reihe => sauber(eigene_breite),
        FlexRichtung::Spalte => sauber(eigene_hoehe),
    }
}

/// Verteilt die Kinder eines Flex-Containers mit `lege_flex_zeilen`.
///
/// RUFT `narsil_css::lege_flex_zeilen` — dieselbe Funktion, die die
/// Koordination geprueft hat. Es gibt hier keinen zweiten Algorithmus.
///
/// # RUECKGABE
///
/// Pro Kind: eine Platzierung, in DOKUMENTREIHENFOLGE. Bei `wrap`
/// stehen die Kinder nach Zeilen — Zeile 0 links nach rechts, dann Zeile 1.
/// Das ist die Reihenfolge, in der sie auch im Baum stehen, damit
/// Zeichnen und Treffer zur Struktur passen.
///
/// # ACHSEN
///
/// `lege_flex_zeilen` rechnet in (haupt, quer). Bei `row` ist `haupt`
/// die Breite, bei `column` die Hoehe; die Drehung macht diese
/// Funktion, nicht `lege_flex_zeilen`.
pub fn verteile_flex(
    inhalt: Rect,
    richtung: FlexRichtung,
    buendig: FlexBuendig,
    ausrichtung: FlexAusrichtung,
    luecke: f32,
    // L3 (02.10.2026 04:5x): der Lueckenwert auf der QUERACHSE.
    // GEMESSENER BEFUND, warum der neue Parameter noetig ist: der
    // Aufruf reichte `sauber(luecke)` an BEIDEN Stellen — fuer
    // `luecke` und fuer `zeilen_luecke`. `lege_flex_zeilen` kann den
    // Zeilenabstand also nicht von null unterscheiden, und
    // `row-gap: 20px` ohne `column-gap` waere unsichtbar.
    quer_luecke: f32,
    umbrechen: bool,
    kinder: &[FlexKind],
) -> Vec<Platzierung> {
    // L4 (03.10.2026 14:2x): die Huelle.  `verteile_flex` ohne
    // Kind-Ausrichtungen delegiert an `verteile_flex_mit_selbst` mit
    // leerer Liste.
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine Huelle und kein zusaetzlicher
    // Parameter: `verteile_flex` wird in `tests/layout_echt.rs`
    // viermal gerufen (GEMESSEN mit `grep -rn "verteile_flex("
    // crates/narsil-layout-engine/tests`), und diese Datei gehoert einer
    // anderen Rolle.  Ein Parameter mehr hiesse dort vier Aenderungen
    // an fremdem Testtext.
    verteile_flex_mit_selbst(
        inhalt,
        richtung,
        buendig,
        ausrichtung,
        luecke,
        quer_luecke,
        umbrechen,
        kinder,
        &[],
    )
}

/// Wie [`verteile_flex`], aber mit der Ausrichtung JE KIND (§9.2
/// `align-self`, §6.3 `auto`).
///
/// `selbst[i] = Some(w)` heisst „Kind i will `w`", `None` heisst „der
/// Wert des Containers".  Die Liste darf kuerzer sein als `kinder` —
/// fehlende Eintraege zaehlen wie `None` (GEMESSENE ENTSCHEIDUNG: ein
/// fehlender Eintrag und `auto` sind dieselbe Sache, und beide sind
/// selbstaendig sinnvoll).
#[allow(clippy::too_many_arguments)]
pub fn verteile_flex_mit_selbst(
    inhalt: Rect,
    richtung: FlexRichtung,
    buendig: FlexBuendig,
    ausrichtung: FlexAusrichtung,
    luecke: f32,
    quer_luecke: f32,
    umbrechen: bool,
    kinder: &[FlexKind],
    kinder_quer: &[KindQuer],
) -> Vec<Platzierung> {
    // DIE EINE GRENZE, und sie ist die von `lege_flex_zeilen`:
    // `MAX_FLEX_KINDER` (256). GEMESSEN am 30.09.2026: mit `MAX_KINDER`
    // (4096) kam ein Vektor von 4096 zurueck, von denen 3840 leer waren —
    // 4 000 Kacheln fuer eine Navigation, die 256 zeigt.
    //
    // WARUM 256 und nicht `MAX_KINDER`: `lege_flex_zeilen` ist die Funktion,
    // die die Zahl bestimmt. Ein eigener, groesserer Wert hiesse, dass wir
    // mehr Kinder anbieten, als gelegt werden — und jedes dieser Kinder
    // bekaeme eine erfundene Position.
    let n = kinder.len().min(MAX_FLEX_KINDER);
    let mut raus: Vec<Platzierung> = Vec::with_capacity(n);
    if n == 0 {
        return raus;
    }
    let menge: Vec<FlexKind> = kinder.iter().take(n).cloned().collect();

    let (haupt, quer) = match richtung {
        FlexRichtung::Reihe => (sauber(inhalt.width), sauber(inhalt.height)),
        FlexRichtung::Spalte => (sauber(inhalt.height), sauber(inhalt.width)),
    };
    let zeilen = lege_flex_zeilen(
        haupt,
        quer,
        buendig,
        ausrichtung,
        sauber(luecke),
        sauber(quer_luecke),
        umbrechen,
        &menge,
        kinder_quer,
    );

    let mut naechster = 0usize;
    // GEMESSENER FEHLER 30.09.2026 (Bombentest): `inhalt.x + sauber(pos)`
    // gibt NaN, wenn `inhalt.x` selbst NaN ist — `sauber` laeuft nur auf
    // dem Summanden, nicht auf der Summe. Deshalb wird der Kasten EINMAL
    // am Anfang gesaeubert und danach nur noch addiert.
    let basis_x = sauber(inhalt.x);
    let basis_y = sauber(inhalt.y);
    for zeile in zeilen.iter() {
        // L3 (02.10.2026 03:5x): der Versatz der Zeile auf der
        // Querachse. GEMESSENE ENTSCHEIDUNG, warum `sauber` erst
        // HIER und nicht beim Lesen: `basis_x`/`basis_y` sind
        // gesaubert, `versatz_quer` kommt aus `lege_flex_zeilen`, das
        // selbst sauber rechnet. Die Summe wird wie die beiden
        // anderen gesaeubert — sonst waere ein NaN erst im Ergebnis.
        let zeilen_versatz = sauber(zeile.versatz_quer);
        for streifen in zeile.streifen.iter() {
            if naechster >= menge.len() {
                break;
            }
            let k = menge[naechster].clone();
            naechster += 1;
            let mut p = match richtung {
                // row: haupt -> x, quer -> y
                //
                // L3 (02.10.2026 03:5x): DER ZEILENVERSATZ FEHLTE.
                //
                // GEMESSENER BEFUND, `examples/l3_flex.rs` (Release,
                // 4 Kinder zu 150 px mit `flex-shrink: 0` in einem
                // 400-px-Kasten, `flex-wrap: wrap`):
                //
                //   vorher  1 Zeile, 0 Ueberlappungen
                //           x = 40 / 190 / 340 / 490  (alles eine Zeile)
                //   mit der Zeile `flex_umbruch` verdrahtet
                //           x = 40 / 40 / 190 / 190    (richtig umgebrochen)
                //           ABER: 1 Zeile und 2 Ueberlappungen
                //
                // Der Umbruch war also richtig auf der HAUPTACHSE und
                // falsch auf der QUERACHSE. `lege_flex_zeilen` liefert
                // `FlexZeile::versatz_quer` — den Versatz der Zeile auf
                // der Querachse, und er wird dort korrekt aufsummiert
                // (`flex.rs`: `z.versatz_quer = pos_q; pos_q += z.mass_quer + zgap;`).
                // Hier wurde er nie addiert: jede Zeile bekam denselben
                // Quer-Anfang, und die Kinder der zweiten Zeile landeten
                // auf denen der ersten.
                //
                // GEMESSENE ENTSCHEIDUNG, warum das eine fehlende
                // Addition ist und kein Konzeptfehler: `quer_pos` ist
                // laut `lege_flex_zeilen` „relativ zur Zeile (plus
                // versatz_quer absolut)". „Plus" stand in der Doku und
                // nicht im Code.
                FlexRichtung::Reihe => Platzierung {
                    x: sauber_lage(basis_x + streifen.haupt_pos),
                    y: sauber_lage(basis_y + zeilen_versatz + streifen.quer_pos),
                    breite: sauber(streifen.haupt_mass),
                    hoehe: sauber(streifen.quer_mass),
                },
                // column: haupt -> y, quer -> x
                FlexRichtung::Spalte => Platzierung {
                    x: sauber_lage(basis_x + zeilen_versatz + streifen.quer_pos),
                    y: sauber_lage(basis_y + streifen.haupt_pos),
                    breite: sauber(streifen.quer_mass),
                    hoehe: sauber(streifen.haupt_mass),
                },
            };
            // Ein Kind mit FESTER Querangabe gewinnt gegen den Streifen:
            // `align-items:stretch` soll ein `height:20px` nicht
            // ueberschreiben.
            if let Some(q) = k.quer {
                match richtung {
                    FlexRichtung::Reihe => p.hoehe = sauber(q),
                    FlexRichtung::Spalte => p.breite = sauber(q),
                }
            }
            raus.push(p);
        }
    }
    // Kinder ohne Streifen (Grenze in narsil-css) bekommen eine eigene
    // Zeile unter den gelegten — sichtbar, aber ohne Ueberlappung.
    while raus.len() < n {
        let i = raus.len();
        raus.push(Platzierung {
            x: basis_x,
            y: sauber_lage(basis_y + haupt * (i as f32)),
            breite: 0.0,
            hoehe: 0.0,
        });
    }
    raus
}

// ===========================================================================
// L2 — RASTER
// ===========================================================================

/// Verteilt die Kinder eines Raster-Containers mit `lege_raster`.
///
/// RUFT `narsil_css::lege_raster` — dieselbe Funktion, die die Koordination
/// an `1fr 2fr 100px bei 800 px, Luecke 10 = 226,7 / 453,3 / 100`
/// geprueft hat.
///
/// # RUECKGABE
///
/// Pro Kind: eine Platzierung in Dokumentreihenfolge.
///
/// **OHNE** `grid-template-columns` gibt es eine einzige Spalte ueber die
/// volle Breite — das ist, was ein Browser ohne Angabe tut, und es ist
/// der Grund, warum `display:grid` allein noch keine Spalten macht.
/// Der Stand vor L4: EIN Lueckenwert fuer beide Achsen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Weg weiter besteht: Fuenf
/// Testaufrufstellen in `tests/layout_echt.rs` benutzen ihn und
/// behaupten sein Verhalten. Der echte Weg ist
/// [`verteile_raster_bei`].
pub fn verteile_raster(
    inhalt: Rect,
    spalten: &[Spalte],
    luecke: f32,
    kind_hoehe: &[f32],
) -> Vec<Platzierung> {
    verteile_raster_bei(inhalt, spalten, luecke, luecke, kind_hoehe)
}

/// Mit ZWEI Lueckenwerten: `luecke_spalte` waagerecht,
/// `luecke_zeile` senkrecht (CSS Grid 1 §10.1).
/// Wie [`verteile_raster_bei_fluss`], aber in Zeilenrichtung.
///
/// L4 (02.10.2026 11:3x): Hülle, damit die Aufrufstelle in `flow.rs`
/// ohne Zusatzargument auskommt. Es gibt nur eine.
pub fn verteile_raster_bei(
    inhalt: Rect,
    spalten: &[Spalte],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kind_hoehe: &[f32],
) -> Vec<Platzierung> {
    verteile_raster_bei_autom(
        inhalt,
        spalten,
        &[],
        &[],
        &[],
        luecke_spalte,
        luecke_zeile,
        kind_hoehe,
        false,
    )
}

/// Raster verteilen; `spaltenfluss = true` heisst `grid-auto-flow: column`.
///
/// L4 (02.10.2026 11:3x): das Flag kommt an. GEMESSENER BEFUND, warum
/// es hier KEIN eigener Zweig ist: die Plazierung gehoert in
/// `lege_raster_bei_fluss` (narsil-css), das kennt die CSS-Regel. Hier
/// wird nur durchgereicht — dasselbe Muster wie bei den Achsluecken
/// (L4 08:1x) und bei `flex-wrap` (P19 00:14).
pub fn verteile_raster_bei_fluss(
    inhalt: Rect,
    spalten: &[Spalte],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kind_hoehe: &[f32],
    spaltenfluss: bool,
) -> Vec<Platzierung> {
    verteile_raster_bei_zeilen(
        inhalt,
        spalten,
        &[],
        luecke_spalte,
        luecke_zeile,
        kind_hoehe,
        spaltenfluss,
    )
}

/// Wie [`verteile_raster_bei_fluss`], aber mit den Zeilenspuren aus
/// `grid-template-rows` (CSS Grid 1 §8.3).
///
/// L4 (02.10.2026 19:4x). GEMESSENE ENTSCHEIDUNG, warum das eine neue
/// Funktion ist und kein weiterer Parameter: `verteile_raster_bei_fluss`
/// hat Aufrufstellen, die das alte Verhalten behaupten (Zeile = groesstes
/// Kind). Sie pruefen etwas anderes, also behaupten sie es weiterhin
/// ueber `&[]` — dieselbe Regel wie bei den Achsluecken (L4 08:1x).
pub fn verteile_raster_bei_zeilen(
    inhalt: Rect,
    spalten: &[Spalte],
    zeilen: &[Spalte],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kind_hoehe: &[f32],
    spaltenfluss: bool,
) -> Vec<Platzierung> {
    verteile_raster_bei_autom(
        inhalt,
        spalten,
        zeilen,
        &[],
        &[],
        luecke_spalte,
        luecke_zeile,
        kind_hoehe,
        spaltenfluss,
    )
}

/// Wie [`verteile_raster_bei_zeilen`], aber MIT `grid-auto-rows` (§7.5)
/// und `grid-auto-columns` (§7.3).
///
/// L4 (02.10.2026 21:6x). Dieselbe Begruendung wie in `narsil-css`: die
/// aeusseren Aufrufe behaupten das alte Verhalten ueber die impliziten
/// Spuren, und es sind viele Aufrufstellen.
pub fn verteile_raster_bei_autom(
    inhalt: Rect,
    spalten: &[Spalte],
    zeilen: &[Spalte],
    auto_spalten: &[Spalte],
    auto_zeilen: &[Spalte],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kind_hoehe: &[f32],
    spaltenfluss: bool,
) -> Vec<Platzierung> {
    let kinds: Vec<RasterKind> = kind_hoehe
        .iter()
        .map(|h| RasterKind {
            hoch: sauber(*h),
            ..RasterKind::default()
        })
        .collect();
    verteile_raster_bei_spann(
        inhalt,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        // L4 (03.10.2026 02:13x): die Huelle traegt LEERE Gebiete —
        // sie behauptet das alte Verhalten ohne Bereiche (§8.1).
        &[],
        luecke_spalte,
        luecke_zeile,
        &kinds,
        spaltenfluss,
        // L4 (04:4x): die aeusseren Aufrufe behaupten `sparse`, den
        // Anfangswert (§8.5).
        false,
    )
}

/// Wie [`verteile_raster_bei_autom`], aber mit den SPANNEN aus §8.3.
///
/// L4 (02.10.2026 22:5x). GEMESSENE ENTSCHEIDUNG, warum es eine neue
/// Funktion ist und kein weiterer Parameter: `RasterKind` traegt die
/// Spannweite, und `kind_hoehe: &[f32]` traegt nur die Hoehe. Die
/// aeusseren Aufrufe behaupten das alte Verhalten und behaupten es
/// weiterhin ueber ihre Huelle.
pub fn verteile_raster_bei_spann(
    inhalt: Rect,
    spalten: &[Spalte],
    zeilen: &[Spalte],
    auto_spalten: &[Spalte],
    auto_zeilen: &[Spalte],
    gebiete: &[Vec<narsil_layout::plan::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinds: &[RasterKind],
    spaltenfluss: bool,
    // L4 (03.10.2026 04:4x): `dense` (§8.5). GEMESSENE ENTSCHEIDUNG,
    // warum das ein PARAMETER ist und keine neue Funktion: `dense`
    // aendert die Packung derselben Platzierung, nicht die Daten — es
    // kommt keine neue Liste und kein neuer Typ dazu. Eine neue Funktion
    // waere eine Huelle mit einem Flag, das nur eine Stelle weiterwandert.
    dicht: bool,
) -> Vec<Platzierung> {
    verteile_raster_mit_spuren(
        inhalt,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        gebiete,
        luecke_spalte,
        luecke_zeile,
        kinds,
        spaltenfluss,
        dicht,
        None,
    )
    .0
}

/// Wie [`verteile_raster_bei_spann`], dazu die Spalten-Spuren `(x, Breite)`
/// in absoluten Koordinaten (Release 11.10.2026, fuer Masonry; siehe
/// `narsil_css::lege_raster_spuren`).
#[allow(clippy::too_many_arguments)]
pub fn verteile_raster_mit_spuren(
    inhalt: Rect,
    spalten: &[Spalte],
    zeilen: &[Spalte],
    auto_spalten: &[Spalte],
    auto_zeilen: &[Spalte],
    gebiete: &[Vec<narsil_layout::plan::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinds: &[RasterKind],
    spaltenfluss: bool,
    dicht: bool,
    // Release 11.10.2026: siehe `narsil_css::lege_raster_spuren`.
    zeilen_platz: Option<f32>,
) -> (Vec<Platzierung>, Vec<(f32, f32)>, Vec<(f32, f32)>) {
    let kind_hoehe: Vec<f32> = kinds.iter().map(|k| k.hoch).collect();
    let n = kind_hoehe.len().min(MAX_KINDER);
    let mut raus: Vec<Platzierung> = Vec::with_capacity(n);
    if n == 0 {
        return (raus, Vec::new(), Vec::new());
    }
    // GEMESSENER FEHLER 30.09.2026 (Bombentest): dieselbe NaFalle wie in
    // `verteile_flex` — der Kasten wird einmal am Anfang gesaeubert.
    let basis_x = sauber(inhalt.x);
    let basis_y = sauber(inhalt.y);
    let volle_breite = sauber(inhalt.width);
    // L4 (02.10.2026 19:5x): OHNE SPALTEN IST EINE IMPLIZITE VOLLE SPALTE
    // — und die Regel steht in `lege_raster_bei_zeilen` (narsil-css, §7.2.1),
    // nicht mehr hier.
    //
    // GEMESSENER BEFUND, der das entschieden hat: als die Ersetzung hier
    // stand (19:4x), gab der CSS-Aufruf bei leeren Spalten **keine**
    // Zelle zurueck (`if ncol == 0 { return }`). Zwei Orte, eine Regel,
    // zwei Antworten — GEMESSEN in `narsil-css/tests/position.rs`,
    // `l4_minmax_auf_der_zeilenachse`: `index out of bounds: the len is
    // 0 but the index is 0`. Der leere `Vec` geht deshalb hier weg, und
    // `ncol == 0` kann nicht mehr eintreten.
    let mass: Vec<SpaltenMass> = spalten
        .iter()
        .cloned()
        .take(MAX_KINDER)
        .map(|s| bridge_spalte(&s))
        .collect();

    // Bruecke: die Plan-Spalte in die Raster-Spalte von narsil-css. Die
    // beiden Typen sind gleich, aber sie gehoeren verschiedenen Crates,
    // und narsil-css darf nicht von narsil-layout abhaengen (der Weg
    // laeuft andersrum). Sechs Zeilen sind billiger als ein Zyklus.
    //
    // L4 (19:4x): die Bruecke steht ueber dem leeren-Spalten-Fall, weil
    // sie jetzt auch die implizite volle Spalte erzeugt.
    // L4 (22:5x): die KINDS kommen durch, mit ihren Spannweiten (§8.3) —
    // sie sind aus der Kaskade gelesen, nicht hier erfunden.
    let arten: Vec<RasterKind> = kinds.iter().take(n).cloned().collect();
    // L4 (02.10.2026 19:4x): dasselbe fuer die ZEILEN (CSS Grid 1 §8.3).
    // GEMESSENE ENTSCHEIDUNG, warum die Bruecke eine eigene Funktion ist
    // und kein `map` im Aufruf: sie wird an ZWEI Stellen gebraucht — hier
    // fuer die Liste und in `bridge_zeilen_hoehe` fuer eine einzelne
    // Spur. Eine geschlossene Formel an zwei Stellen kann nicht
    // auseinanderlaufen.
    let zeilen_mass: Vec<SpaltenMass> = zeilen.iter().cloned().map(|s| bridge_spalte(&s)).collect();
    // L4 (02.10.2026 21:6x): dieselbe Bruecke fuer die beiden
    // Auto-Listen (§7.3, §7.5).
    let auto_mass: Vec<SpaltenMass> = auto_spalten
        .iter()
        .cloned()
        .map(|s| bridge_spalte(&s))
        .collect();
    let auto_zeilen_mass: Vec<SpaltenMass> = auto_zeilen
        .iter()
        .cloned()
        .map(|s| bridge_spalte(&s))
        .collect();
    // L4 (08:1x): beide Achsluecken durchgereicht. Vorher stand hier
    // `sauber(luecke)` fuer beide — siehe `lege_raster_bei` in narsil-css.
    // L4 (03.10.2026 02:10x): die Gebiete (§8.1) wandern mit. Sie sind
    // Zeilen von NAMEN, also kann der Aufrufer sie nicht in Spurmasse
    // uebersetzen — die Rechtecke brauchen die Vorlage.
    let gebiete_css: Vec<Vec<narsil_css::Gebiet>> = gebiete
        .iter()
        .map(|z| z.iter().map(|g| narsil_css::Gebiet(g.0)).collect())
        .collect();
    let (zellen, spuren, zeilen_spuren) = lege_raster_spuren(
        sauber(inhalt.width),
        &mass,
        &zeilen_mass,
        &auto_mass,
        &auto_zeilen_mass,
        &gebiete_css,
        sauber(luecke_spalte),
        sauber(luecke_zeile),
        &arten,
        spaltenfluss,
        dicht,
        zeilen_platz,
    );
    for i in 0..n {
        raus.push(match zellen.get(i) {
            Some(z) => Platzierung {
                x: sauber_lage(basis_x + z.x),
                y: sauber_lage(basis_y + z.y),
                breite: sauber(z.breite),
                hoehe: sauber(z.hoehe),
            },
            // Sollte nicht vorkommen (die Grenzen sind dieselben), aber
            // ein Kind ohne Zelle waere ein Loch im Bild.
            None => Platzierung::WEG,
        });
    }
    let spuren = spuren
        .into_iter()
        .map(|(x, b)| (sauber_lage(basis_x + x), sauber(b)))
        .collect();
    let zeilen_spuren = zeilen_spuren
        .into_iter()
        .map(|(y, h)| (sauber_lage(basis_y + y), sauber(h)))
        .collect();
    (raus, spuren, zeilen_spuren)
}

// ===========================================================================
// L3 — POSITION
// ===========================================================================

/// Versatz einer `position: relative` Box (CSS 2.1, 9.4.3).
///
/// RUFT `narsil_css::relativ_versatz` — dieselbe Funktion, die die
/// Koordination geprueft hat (`top` gewinnt `bottom`, `left` gewinnt
/// `right`, `auto` zaehlt 0).
pub fn versatz_relativ(
    oben: Option<f32>,
    rechts: Option<f32>,
    unten: Option<f32>,
    links: Option<f32>,
) -> (f32, f32) {
    relativ_versatz(
        oben.map(sauber_versatz),
        rechts.map(sauber_versatz),
        unten.map(sauber_versatz),
        links.map(sauber_versatz),
    )
}

/// Platz einer absoluten Box gegen ihren enthaltenden Block
/// (CSS 2.1, 10.3.7 waagerecht, 10.6.4 senkrecht).
///
/// RUFT `absolut_waagerecht` und `absolut_senkrecht` aus `narsil-css`.
///
/// * `behaelter` — die Content-Box des enthaltenden Blocks, in
///   ABSOLUTEN Koordinaten.
/// * `statisch` — wo die Box im Fluss stuende.
/// * `inhalt` — ihre Schrumpfgroesse (fuer `width:auto`).
// 9 Argumente sind hier richtig und nicht bequem: CSS 2.1 hat fuer
// 10.3.7 vier Seiten und fuer 10.6.4 zwei Achsen, und ein
// Parameter-Bundle waere eine Artikulation, die man an drei Stellen
// aufpacken und auspacken muesste.
#[allow(clippy::too_many_arguments)]
pub fn platz_absolut(
    behaelter: Rect,
    links: Option<f32>,
    oben: Option<f32>,
    breite: Option<f32>,
    hoehe: Option<f32>,
    rechts: Option<f32>,
    unten: Option<f32>,
    statisch: Platzierung,
    inhalt: Size,
) -> Platzierung {
    let (x, b) = absolut_waagerecht(
        sauber(behaelter.width),
        links.map(sauber_versatz),
        breite.map(sauber),
        rechts.map(sauber_versatz),
        sauber_versatz(statisch.x - behaelter.x),
        sauber(inhalt.width),
    );
    let (y, h) = absolut_senkrecht(
        sauber(behaelter.height),
        oben.map(sauber_versatz),
        hoehe.map(sauber),
        unten.map(sauber_versatz),
        sauber_versatz(statisch.y - behaelter.y),
        sauber(inhalt.height),
    );
    Platzierung {
        x: sauber_versatz(behaelter.x + x),
        y: sauber_lage(behaelter.y + y),
        breite: b,
        hoehe: h,
    }
}

/// Der enthaltende Block fuer eine absolute Box (CSS 2.1, 10.1).
///
/// RUFT `narsil_css::enthaltender_block`. Die Kette kommt von aussen nach
/// innen; gesucht wird von innen.
pub fn enthaltend(ansicht: Rechteck, vorfahren: &[Vorfahr]) -> Rechteck {
    enthaltender_block(ansicht, vorfahren)
}

/// Die Mal-Reihenfolge fuer die Stapelkontexte (CSS 2.1, Anhang E).
///
/// RUFT `narsil_css::mal_reihenfolge`.
pub fn reihenfolge(schichten: &[Schicht]) -> Vec<usize> {
    mal_reihenfolge(schichten)
}

/// Die Schicht eines Knotens nach Anhang E.
///
/// `z_index: None` heisst `auto`. Bei `position: static` ist `auto` ein
/// gewöhnlicher Fluss-Block (`Schicht::Block`), sonst `Schicht::Positioniert`
/// (die Reihenfolge unter den positionierten Boxen entscheidet dann die
/// Dokumentreihenfolge, weil beide z-index `auto` haben).
///
/// L5 (02.10.2026 13:0x): DIE PRUEFUNG AUF `position` VORAN.
///
/// GEMESSENER BEFUND, was vorher passierte
/// (`tests/mal_reihenfolge.rs`, `l5_z_index_an_einem_statischen_
/// kasten_bleibt_ohne_wirkung`): `z-index: 9` an einem statischen
/// Kasten ergab die Farbfolge `[20, 30, 10]` — der 10er hinten.
/// Richtig ist `[10, 20, 30]`.
///
/// GEMESSENE URSACHE: der `z_index`-Zweig stand VOR der `position`-
/// Pruefung. `Some(z) if z > 0 => Positiv(z)` traf also auch dann zu,
/// wenn `position` `Statisch` war, und der `position`-Zweig war fuer
/// diese Kaesten unerreichbar.
///
/// CSS 2.1, 9.9.1 woertlich: "this property applies to positioned
/// elements". Das ist keine Formulierungsfrage, sondern die Regel, an
/// der die Reihenfolge haengt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Pruefung VORAN steht und nicht
/// im `match`: mit `PositionArt::Statisch` als erstem Fall liest man
/// die Regel an der Reihenfolge des Codes ab — die `position`-Frage
/// kommt zuerst, weil sie die uebergreifende ist. Ein `match` mit
/// `z_index` zuerst laesst den naechsten Leser wieder genau an der
/// Stelle anfangen, an der es schiefgegangen ist.
pub fn schicht_von(position: PositionArt, z_index: Option<i32>) -> Schicht {
    schicht_von_als(position, z_index, false)
}

/// Wie [`schicht_von`], aber mit der Ausnahme fuer Flex-Items.
///
/// `flex_item = true` heisst: der Elternknoten ist ein Flex-Container.
/// CSS Flexbox 1 §5.4: `z-index` gilt fuer Flex-Items auch bei
/// `position: static`.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine ZWEITE Funktion und kein
/// zusaetzlicher Parameter an `schicht_von`: `schicht_von` ist
/// exportiert und wird von `tests/layout_echt.rs` aufgerufen. Ein
/// Parameter `false` an drei Aufrufstellen waere eine Aenderung ohne
/// Aussage — die Regel „was, wenn kein Flex-Item" ist dann an jedem
/// Aufrufer festgeschrieben statt einmal hier. So steht die
/// Voreinstellung an EINER Stelle.
pub fn schicht_von_als(position: PositionArt, z_index: Option<i32>, flex_item: bool) -> Schicht {
    if matches!(position, PositionArt::Statisch) && !flex_item {
        return Schicht::Block;
    }
    match z_index {
        Some(z) if z < 0 => Schicht::Negativ(z),
        Some(z) if z > 0 => Schicht::Positiv(z),
        _ => Schicht::Positioniert,
    }
}

// ===========================================================================
// L4 — FLOAT UND CLEAR
// ===========================================================================

/// Der Float-Zustand beim Platzieren der Kinder EINES Containers.
///
/// GEMESSEN wichtig: es gibt EINEN Zustand je Container, nicht einen
/// globalen. Zwei Container nebeneinander haben je einen, und ihre
/// Floats stossen nicht aneinander — das ist der Unterschied zwischen
/// `float` und einem Float-Stapel ueber die ganze Seite.
///
/// `unten_links`/`unten_rechts` sind RELATIV zum Anfang des
/// Containers (nicht absolut), damit `clear_y` dieselben Koordinaten
/// sieht wie die y-Position, die es vergleicht.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FloatZustand {
    /// Wie weit links zuletzt belegt war (Innenkante plus rechter Rand
    /// plus Gap) — der zuletzt gesetzte linke Float.
    pub belegt_links: f32,
    /// Wie weit rechts zuletzt belegt war.
    pub belegt_rechts: f32,
    /// Die tiefste Unterkante der linken Floats, relativ zum Anfang.
    pub unten_links: f32,
    /// Die tiefste Unterkante der rechten Floats, relativ zum Anfang.
    pub unten_rechts: f32,
    /// Der tiefste belegte Punkt, relativ zum Anfang. Das ist die
    /// eigene Mindesthoehe des Containers (CSS 2.1, 9.5).
    pub tiefste: f32,
    /// Release 12.10.2026 (Agent CSS2): JEDER gesetzte Float mit seinem
    /// senkrechten Band (CSS 2.1 §9.5.1, Regeln 2, 3, 5 und 7 vergleichen mit
    /// ALLEN frueheren Floats, nicht nur dem letzten). GEMESSEN vorher (WPT
    /// `floats-wrap-top-below-inline-001l/r`, `floats-wrap-bfc-with-margin-
    /// 008/009`): zwei Floats untereinander (`clear: left`) — der zweite, 100 px
    /// breite belegte schon die Zeile neben dem ersten, 50 px breiten. Fest
    /// begrenzt (M3), damit der Zustand `Copy` bleibt; voll verdraengt ein
    /// neuer Float den mit der hoechsten Unterkante.
    liste: [FloatBand; MAX_FLOAT_BAENDER],
    anzahl: usize,
    /// Die Oberkante des zuletzt gesetzten Floats (Regel 5: ein spaeterer
    /// steht nicht hoeher).
    letzte_oben: f32,
}

/// Release 12.10.2026 (Agent CSS2): siehe `FloatZustand::liste`.
const MAX_FLOAT_BAENDER: usize = 24;

/// Release 12.10.2026 (Agent CSS2): ein gesetzter Float — seine belegte
/// Kante (links: rechte Randkante plus Gap ab der linken Innenkante; rechts:
/// Abstand der linken Randkante plus Gap von der rechten Innenkante) und sein
/// senkrechtes Band `[oben, unten)`, relativ zum Container.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct FloatBand {
    rechts: bool,
    kante: f32,
    oben: f32,
    unten: f32,
}

impl FloatBand {
    /// Ueberschneidet das Band `[y, y + h)` (bei `h <= 0` der Punkt `y`)?
    fn trifft(&self, y: f32, h: f32) -> bool {
        if h > 0.0 { self.oben < y + h && self.unten > y } else { self.oben <= y && y < self.unten }
    }
}

impl FloatZustand {
    /// Beginn bei Null — es gibt noch keinen Float.
    pub fn neu() -> FloatZustand {
        FloatZustand::default()
    }

    /// Die y-Position eines Kindes, nachdem `clear` gewirkt hat.
    ///
    /// RUFT `narsil_css::clear_y`. Der Rueckgabewert ist RELATIV zum
    /// Anfang des Containers.
    pub fn clear_y(&self, raeumung: RaeumungArt, y_relativ: f32) -> f32 {
        clear_y(
            match raeumung {
                RaeumungArt::Keins => Raeumung::Keins,
                RaeumungArt::Links => Raeumung::Links,
                RaeumungArt::Rechts => Raeumung::Rechts,
                RaeumungArt::Beide => Raeumung::Beide,
            },
            sauber_versatz(y_relativ),
            self.unten_links,
            self.unten_rechts,
        )
    }

    /// Die x-Position eines schwebenden Kindes.
    ///
    /// RUFT `narsil_css::float_x`.
    pub fn float_x(&self, container_breite: f32, eigene_breite: f32, seite: FloatArt) -> f32 {
        float_x(
            sauber(container_breite),
            sauber(eigene_breite),
            match seite {
                FloatArt::Keins => Float::Keins,
                FloatArt::Links => Float::Links,
                FloatArt::Rechts => Float::Rechts,
            },
            self.belegt_links,
            self.belegt_rechts,
        )
    }

    /// Trägt ein Kind ein und liefert seine Stelle RELATIV zum Anfang
    /// des Containers.
    ///
    /// Das ist die einzige Stelle, an der der Zustand wächst — damit
    /// kann niemand versehentlich zwei Floats auf dieselbe Stelle
    /// legen, und die Reihenfolge ist eindeutig.
    pub fn platziere(
        &mut self,
        container_breite: f32,
        eigene_breite: f32,
        eigene_hoehe: f32,
        y_relativ: f32,
        seite: FloatArt,
        gap: f32,
    ) -> Platzierung {
        let b = sauber(eigene_breite);
        let h = sauber(eigene_hoehe);
        let y = sauber_versatz(y_relativ);
        // DIE REGEL, die nach dem Umbruch gilt (CSS 2.1, 9.5.1): es zaehlen
        // die Floats, deren Band sich mit dem des neuen ueberschneidet — ein
        // Float aus der Zeile drueber begrenzt nichts mehr.
        let (links_belegt, rechts_belegt) = self.kanten_im_band(y, h);
        match seite {
            FloatArt::Keins => {
                self.tiefste = self.tiefste.max(y + h);
                Platzierung {
                    x: 0.0,
                    y,
                    breite: b,
                    hoehe: h,
                }
            }
            FloatArt::Links => {
                let x = float_x(
                    sauber(container_breite),
                    b,
                    Float::Links,
                    links_belegt,
                    rechts_belegt,
                );
                self.belegt_links = sauber(x + b + sauber(gap));
                self.unten_links = self.unten_links.max(y + h);
                self.tiefste = self.tiefste.max(y + h);
                // Release 12.10.2026 (Agent CSS2): ein Float ohne Breite belegt
                // kein Band (WPT `floats/zero-width-floats-positioning`).
                if b > 0.0 {
                    self.band_eintragen(FloatBand { rechts: false, kante: self.belegt_links, oben: y, unten: y + h });
                }
                Platzierung {
                    x,
                    y,
                    breite: b,
                    hoehe: h,
                }
            }
            FloatArt::Rechts => {
                let x = float_x(
                    sauber(container_breite),
                    b,
                    Float::Rechts,
                    links_belegt,
                    rechts_belegt,
                );
                // GEMESSENER FEHLER 30.09.2026 (Test
                // `a2_umbruch_bei_rechtsfloat`): hier stand
                // `breite - x + b + gap` — die Breite des Floats zaehlte
                // einmal zu viel. Der Abstand von `x` bis zum Rand ist es.
                self.belegt_rechts = sauber(sauber(container_breite) - x + sauber(gap));
                self.unten_rechts = self.unten_rechts.max(y + h);
                self.tiefste = self.tiefste.max(y + h);
                // Release 12.10.2026 (Agent CSS2): ein Float ohne Breite belegt
                // kein Band (WPT `floats/zero-width-floats-positioning`).
                if b > 0.0 {
                    self.band_eintragen(FloatBand { rechts: true, kante: self.belegt_rechts, oben: y, unten: y + h });
                }
                Platzierung {
                    x,
                    y,
                    breite: b,
                    hoehe: h,
                }
            }
        }
    }

    /// Release 12.10.2026 (Agent CSS2): ist kein Float eingetragen?
    pub(crate) fn ist_leer(&self) -> bool {
        self.anzahl == 0
    }

    /// Release 12.10.2026 (Agent CSS2): wie viele Baender eingetragen sind.
    pub(crate) fn anzahl(&self) -> usize {
        self.anzahl
    }

    /// Release 12.10.2026 (Agent CSS2): derselbe Zustand im Rahmen eines
    /// Kindes ohne eigenen Formatierungskontext, dessen Inhaltskante `dl`
    /// rechts der linken, `dr` links der rechten und `dy` unter der oberen
    /// Inhaltskante liegt (CSS 2.1 §9.5: die Floats eines Kontexts gelten fuer
    /// alle seine Nachfahren). Baender, die dort nichts mehr belegen, fallen
    /// weg.
    pub(crate) fn verschoben(&self, dl: f32, dr: f32, dy: f32) -> FloatZustand {
        let mut z = FloatZustand::default();
        for b in &self.liste[..self.anzahl] {
            let kante = b.kante - if b.rechts { dr } else { dl };
            let unten = b.unten - dy;
            // Eine Kante <= 0 liegt ausserhalb des Inhalts — fuer Zeilen ohne
            // Wirkung, aber ein Kasten mit eigenem Kontext, der ueber seinen
            // Block hinausragt, darf den Float trotzdem nicht ueberlappen.
            if !(unten > 0.0 && kante.is_finite() && unten.is_finite()) {
                continue;
            }
            z.band_roh(FloatBand { rechts: b.rechts, kante, oben: b.oben - dy, unten });
        }
        z.letzte_oben = self.letzte_oben - dy;
        z
    }

    /// Release 12.10.2026 (Agent CSS2): die Baender ab `ab` (die eigenen eines
    /// Kindes, ohne die geerbten) zurueck im Rahmen des Eltern — die Umkehrung
    /// von [`Self::verschoben`].
    pub(crate) fn zurueck(&self, ab: usize, dl: f32, dr: f32, dy: f32) -> FloatZustand {
        let mut z = FloatZustand::default();
        for b in self.liste[..self.anzahl].iter().skip(ab) {
            let kante = b.kante + if b.rechts { dr } else { dl };
            z.band_roh(FloatBand { rechts: b.rechts, kante, oben: b.oben + dy, unten: b.unten + dy });
        }
        z.letzte_oben = self.letzte_oben + dy;
        z
    }

    /// Release 12.10.2026 (Agent CSS2): nimmt die Baender eines anderen
    /// Zustands dazu (Floats aus Nachfahren ohne eigenen Kontext).
    pub(crate) fn uebernehme(&mut self, andere: &FloatZustand) {
        for b in &andere.liste[..andere.anzahl] {
            self.band_roh(*b);
        }
        if andere.anzahl > 0 {
            self.letzte_oben = self.letzte_oben.max(andere.letzte_oben);
        }
    }

    /// Release 12.10.2026 (Agent CSS2): ein Band mit allen Kennzahlen.
    fn band_roh(&mut self, b: FloatBand) {
        if b.rechts {
            self.belegt_rechts = b.kante;
            self.unten_rechts = self.unten_rechts.max(b.unten);
        } else {
            self.belegt_links = b.kante;
            self.unten_links = self.unten_links.max(b.unten);
        }
        self.tiefste = self.tiefste.max(b.unten);
        let oben = self.letzte_oben;
        self.band_eintragen(b);
        self.letzte_oben = oben;
    }

    /// Release 12.10.2026 (Agent CSS2): traegt ein Band ein (voll: ersetzt
    /// das mit der hoechsten Unterkante).
    fn band_eintragen(&mut self, band: FloatBand) {
        self.letzte_oben = self.letzte_oben.max(band.oben);
        if self.anzahl < MAX_FLOAT_BAENDER {
            self.liste[self.anzahl] = band;
            self.anzahl += 1;
            return;
        }
        if let Some(stelle) = self
            .liste
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.unten.total_cmp(&b.1.unten))
            .map(|(i, _)| i)
        {
            self.liste[stelle] = band;
        }
    }

    /// Die belegten Kanten, die fuer eine Box bei `y` GELTEN.
    ///
    /// Liegt `y` unterhalb der Unterkante der linken Floats, ist die
    /// linke Kante frei (und umgekehrt) — so lautet es CSS 2.1, 9.5.1,
    /// und so entsteht der Umbruch ueberhaupt.
    pub(crate) fn kanten_bei(&self, y: f32) -> (f32, f32) {
        self.kanten_im_band(y, 0.0)
    }

    /// Release 12.10.2026 (Agent CSS2): die belegten Kanten (links, rechts)
    /// fuer das Band `[y, y + h)` — das Maximum ueber alle Floats, die es
    /// schneiden (fuer einen Float seine Hoehe, fuer eine Zeilenbox ihre).
    /// Release 12.10.2026 (Agent CSS2): wie [`Self::kanten_im_band`], aber
    /// roh (auch negativ) und `None`, wo kein Float der Seite das Band schneidet.
    pub(crate) fn kanten_im_band_roh(&self, y: f32, h: f32) -> (Option<f32>, Option<f32>) {
        let mut links: Option<f32> = None;
        let mut rechts: Option<f32> = None;
        for band in &self.liste[..self.anzahl] {
            if band.trifft(y, h) {
                let z = if band.rechts { &mut rechts } else { &mut links };
                *z = Some(z.map_or(band.kante, |v| v.max(band.kante)));
            }
        }
        (links, rechts)
    }

    pub(crate) fn kanten_im_band(&self, y: f32, h: f32) -> (f32, f32) {
        let mut links = 0.0f32;
        let mut rechts = 0.0f32;
        for band in &self.liste[..self.anzahl] {
            if band.trifft(y, h) {
                if band.rechts {
                    rechts = rechts.max(band.kante);
                } else {
                    links = links.max(band.kante);
                }
            }
        }
        (links, rechts)
    }

    /// Release 12.10.2026 (Agent CSS2): die naechste Unterkante eines Floats,
    /// der das Band `[y, y + h)` schneidet — dorthin rueckt, was daneben
    /// nicht passt (CSS 2.1 §9.5.1 Regel 7, §9.5: Zeilenboxen). `None`, wenn
    /// keiner es schneidet.
    pub(crate) fn naechste_unterkante(&self, y: f32, h: f32) -> Option<f32> {
        self.liste[..self.anzahl]
            .iter()
            .filter(|b| b.trifft(y, h) && b.unten > y)
            .map(|b| b.unten)
            .min_by(f32::total_cmp)
    }

    /// Wie tief reicht der Inhalt dieses Containers? (CSS 2.1, 9.5:
    /// der Container umschliesst seine Floats.)
    pub fn mindest_hoehe(&self) -> f32 {
        sauber(self.tiefste)
    }

    /// Platziert ein Kind und LAESST ES NOTFALLS UMBRUCHEN
    /// (CSS 2.1, 9.5.1).
    ///
    /// DIE REGEL, die [`Self::platziere`] nicht hat: passt ein Float
    /// nicht mehr neben die bereits schwebenden, rueckt er so weit nach
    /// unten, bis er passt — jeweils an die naechste Unterkante eines
    /// Floats in seinem Band. Release 12.10.2026 (Agent CSS2): und nie
    /// hoeher als der zuletzt gesetzte Float (Regel 5).
    ///
    /// # DIE SCHLEIFE, und warum sie begrenzt ist
    ///
    /// Jeder Durchlauf senkt die Position auf eine Unterkante eines der
    /// bisherigen Floats. Nach spaetestens `MAX_UMBRUECHE` Schritten ist
    /// entweder Platz oder der Zustand bewegt sich nicht mehr (M3).
    ///
    /// # RUECKGABE
    ///
    /// Die Platzierung. `rueckte` sagt, ob ueberhaupt umgebrochen wurde
    /// — das braucht der Aufrufer, um den Zeilencursor nicht doppelt
    /// weiterzuzaehlen.
    pub fn platziere_mit_umbruch(
        &mut self,
        container_breite: f32,
        eigene_breite: f32,
        eigene_hoehe: f32,
        y_relativ: f32,
        seite: FloatArt,
        gap: f32,
    ) -> (Platzierung, bool) {
        let b = sauber(eigene_breite);
        let c = sauber(container_breite);
        let h = sauber(eigene_hoehe);
        // Ohne Float gibt es nichts umzubrechen.
        if seite == FloatArt::Keins {
            let p = self.platziere(c, b, eigene_hoehe, y_relativ, seite, gap);
            return (p, false);
        }
        let start = sauber_versatz(y_relativ);
        let mut y = start.max(self.letzte_oben);
        let mut schritte = 0usize;
        while schritte < MAX_UMBRUECHE && !self.passt(c, b, h, y, seite) {
            // Runter bis an die naechste Unterkante der schwebenden Boxen,
            // die im Weg sind (CSS 2.1, 9.5.1: "move down until it fits").
            let Some(naechste) = self.naechste_unterkante(y, h) else {
                break;
            };
            let ziel = sauber_versatz(y.max(naechste + gap));
            if ziel <= y {
                // Kein Fortschritt: abbrechen, statt endlos zu laufen.
                break;
            }
            y = ziel;
            schritte += 1;
        }
        let p = self.platziere(c, b, eigene_hoehe, y, seite, gap);
        (p, y > start)
    }

    /// Passt ein Float der Breite `b` und Hoehe `h` bei `y` neben die
    /// belegten Kanten seines Bandes?
    /// ACHTUNG, die Seite ist nicht symmetrisch (CSS 2.1, 9.5.1):
    /// * links: der Float passt, wenn rechts von ihm Platz bleibt, also
    ///   `belegt_links + b <= container_breite - belegt_rechts`.
    /// * rechts: der Float passt, wenn links von ihm genug Platz bleibt.
    ///
    /// GEMESSENER FEHLER 30.09.2026 (Test `a2_umbruch_bei_rechtsfloat`):
    /// die Pruefung lief ueber `float_x`, das bei 0 kappt — gerechnet wird
    /// darum an den echten Kanten.
    fn passt(&self, container_breite: f32, b: f32, h: f32, y: f32, seite: FloatArt) -> bool {
        let c = sauber(container_breite);
        let (links_belegt, rechts_belegt) = self.kanten_im_band(y, h);
        match seite {
            // Release 12.10.2026 (Agent CSS2): beide Seiten zaehlen (Regel 7:
            // ein linker Float darf einen rechten nicht ueberlappen).
            FloatArt::Links | FloatArt::Rechts => {
                // Ein Float, der allein schon breiter ist als der Container,
                // passt nur ohne andere im Band (sonst nie).
                links_belegt + sauber(b) + rechts_belegt <= c + 0.01 || (links_belegt <= 0.0 && rechts_belegt <= 0.0)
            }
            // Ohne Float gibt es nichts, wogegen es zu passen geht.
            FloatArt::Keins => true,
        }
    }
}

// ===========================================================================
// OVERFLOW (C14, fuer A3 vorbereitet)
// ===========================================================================

/// Schneidet einen Kindkasten gegen den Inhaltskasten, wenn `overflow`
/// es verlangt (CSS 2.1, 11.1.1).
///
/// RUFT `narsil_css::beschnitt_ueberlauf` — dieselbe Funktion, die der
/// C14-Test geprueft hat. `None` heisst „nicht beschneiden"
/// (`overflow:visible`), `Some` heisst „auf diese Groesse beschneiden".
pub fn beschnitt_kasten(inhalt: Rect, kind: Rect, ueberlauf: UeberlaufArt) -> Option<Rect> {
    let b = Rechteck::neu(inhalt.x, inhalt.y, inhalt.width, inhalt.height);
    let k = Rechteck::neu(kind.x, kind.y, kind.width, kind.height);
    beschnitt_ueberlauf(
        &b,
        &k,
        match ueberlauf {
            UeberlaufArt::Sichtbar => Ueberlauf::Sichtbar,
            UeberlaufArt::Versteckt => Ueberlauf::Versteckt,
            UeberlaufArt::Rollen => Ueberlauf::Scroll,
            UeberlaufArt::Automatisch => Ueberlauf::Auto,
        },
    )
    .map(|r| Rect::from_origin_size(Point::new(r.x, r.y), Size::new(r.breite, r.hoehe)))
}

/// Wie weit kann IN EINEM KastEN gescrollt werden? (A3)
/// Das Ergebnis eines Scrollbereichs: wie weit, und ob ein Rollbalken
/// sichtbar sein MUSS (`scroll`) oder nur bei Bedarf da sein DARF
/// (`auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollBereich {
    /// Wie weit nach unten gescrollt werden kann (px). 0 = kein Scrollen.
    pub max_versatz: u32,
    /// `true` = der Rollbalken ist immer da (`overflow:scroll`).
    pub balken_immer: bool,
    /// `true` = der Inhalt passt nicht hinein, es gibt also etwas zu
    /// rollen (`overflow:auto` mit Inhalt).
    pub balken_noetig: bool,
}

/// Der Scrollbereich EINES Kastens (CSS 2.1, 11.1.3).
///
/// * `kasten` — die Groesse, die der Kasten im Bild einnimmt (also das,
///   was man sieht).
/// * `inhalt_hoehe` — wie hoch sein Inhalt wirklich ist (die Kinder
///   summiert, inklusive Padding).
/// * `ueberlauf` — `visible` und `hidden` rollen nicht, `auto` und
///   `scroll` schon.
///
/// # WARUM EINE EIGENE RECHNUNG UND NICHT `max_versatz` aus narsil
///
/// `narsil::Angezeigt::max_versatz` rechnet fuer die SEITE (die ganze
/// Ansicht). Das hier rechnet fuer einen KASTEN — eine Navigationsleiste
/// mit `overflow:auto` rollt sich selbst, ohne dass sich das Blatt
/// bewegt. Beides gibt es, und sie duerfen sich nicht mischen.
///
/// # DIE ZAHLEN
///
/// `max_versatz = max(0, inhalt_hoehe - kasten_hoehe)`. Kein Zeichen
/// drumherum: `auto` bei Inhalt, der 10 px zu hoch ist, ergibt 10, und
/// ein Browser zeigt einen Rollbalken, der genau diesen 10 Pixeln
/// entspricht.
pub fn scroll_bereich(kasten: Rect, inhalt_hoehe: f32, ueberlauf: UeberlaufArt) -> ScrollBereich {
    match ueberlauf {
        // Kein Scrollen. Auch nicht, wenn der Inhalt herausragt: das ist
        // genau der Unterschied zu `hidden`.
        UeberlaufArt::Sichtbar | UeberlaufArt::Versteckt => ScrollBereich::default(),
        UeberlaufArt::Rollen | UeberlaufArt::Automatisch => {
            let sichtbar = sauber(kasten.height);
            let inhalt = sauber(inhalt_hoehe);
            let ueber = inhalt - sichtbar;
            if ueber <= 0.0 {
                // Passt alles: bei `scroll` ist der Balken trotzdem da
                // (CSS 2.1, 11.1.3: "the scroll bar is always visible"),
                // bei `auto` nicht.
                ScrollBereich {
                    max_versatz: 0,
                    balken_immer: ueberlauf == UeberlaufArt::Rollen,
                    balken_noetig: false,
                }
            } else {
                ScrollBereich {
                    max_versatz: ueber.min(MAX_WERT_PX) as u32,
                    balken_immer: ueberlauf == UeberlaufArt::Rollen,
                    balken_noetig: true,
                }
            }
        }
    }
}

/// Klemmt einen Scrollversatz auf den Bereich, den
/// [`scroll_bereich`] berechnet hat.
///
/// GEMESSEN wichtig: ein ungeklemmter Versatz zeigt Inhalt, der gar
/// nicht mehr da ist (leere Flaeche), und ein Klick auf einen Link, den
/// es nicht gibt. Beides ist der Bug, den die Seite am 29.09. einmal
/// hatte (im Modul-Doc von `Angezeigt::max_versatz` beschrieben).
pub fn klemme_versatz(versatz: u32, bereich: &ScrollBereich) -> u32 {
    versatz.min(bereich.max_versatz)
}
