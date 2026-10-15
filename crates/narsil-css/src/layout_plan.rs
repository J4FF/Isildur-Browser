//! DER WEG VON DER KASKADE INS LAYOUT: `Berechnet -> LayoutStil`
//! (Rolle LAYOUT, Welle 7, L1–L4).
//!
//! Die Kaskade rechnet aus den Blaettern `[Berechnet]`. Das Layout
//! braucht daraus genau die Werte, die `narsil_core::style::Style` nicht
//! traegt: `display`, die Flex-Werte, die Raster-Spalten, `position`
//! mit seinen vier Seiten, `float`, `clear` und `overflow`.
//!
//! GEMESSENER AUSGANGSPUNKT (Bildschirmfoto vom 30.09.2026, 19:15, aus
//! `probe/layout/bilder/`): `display:flex` und `display:grid` kamen
//! nirgends an. Ein Dreispalten-Raster lag in DREI ZEILEN, ein
//! `position:absolute`-Badge fehlte ganz, und eine Navigation mit
//! `gap:16px` hatte keine Luecke. Grund: der Weg endete in
//! `narsil_core::style::Style`, und dort gibt es diese Felder nicht.
//!
//! # WER RUFT DAS AUF
//!
//! `narsil-page`, in `kaskade.rs::stil_von_css`, direkt nach der
//! Uebernahme der anderen Werte. Der Aufruf ist additiv und aendert
//! an keinem bestehenden Verhalten etwas: ein Knoten, fuer den kein
//! Plan gebaut wird, ist ein Block — also genau der Zustand von
//! heute. Der genaue Aufruf steht in `team/WUNSCH_LAYOUT.txt`.
//!
//! # GRENZEN (M3/M4)
//!
//! * Nicht-endliche Zahlen werden 0 (der Kaskade kann sie nicht
//!   liefern, aber `calc()` und `var()` muessen es nicht ausschliessen).
//! * Werte werden an [`MAX_WERT_PX`] gekappt, damit `left: 1e30px`
//!   keine Box aus dem Bild schiebt.
//! * Die Raster-Spalten kommen bereits an [`MAX_RASTER_SPALTEN`]
//!   begrenzt aus dem Parser; hier wird noch einmal geschnitten, weil
//!   [`LayoutStil::raster`] das auch tut und man sich auf EINE Grenze
//!   verlassen soll. L4 (02.10.2026 19:4x): dasselbe gilt fuer die
//!   Raster-ZEILEN (`plan_spuren`), und sie benutzen dieselbe Grenze —
//!   ein Raster mit mehr als 32 Spuren in einem 600-px-Kasten haelt je
//!   Spur keine ganze Zahl Pixel, das ist eine Rechengrenze und keine
//!   CSS-Grenze.

use narsil_layout::plan::{
    Ausrichtung, Buendig, FlexUmbruch, FloatArt, Gebiet, Grenzen, GrenzenProzent, LayoutPlan,
    LayoutStil, MAX_RASTER_SPALTEN, PositionArt, RaeumungArt, Richtung, Spalte, Textausrichtung,
    UeberlaufArt,
};

use crate::typen::{
    Anzeige, Berechnet, FlexAusrichtung, FlexBuendig, Float, Position, Raeumung, SchreibRichtung,
    Ueberlauf,
};

/// Der Plan-Wert fuer einen berechneten Knoten.
///
/// # WENN ICH DAS FUER JEDEN KNOTEN AUFRUFE
///
/// Kostet es? Ja: 12 Vergleiche und eine Kopie des `Vec<Spalte>` je
/// Knoten. Bei 20 000 Knoten sind das 20 000 Kopien einer Spaltenliste,
/// die im Normalfall leer ist — also fast nichts. Der Plan entsteht
/// nur beim Bau der Seite, nicht beim Zeichnen, und wird danach nur
/// noch gelesen.
/// Eine Rasterspalte in die Plan-Spalte, rekursiv fuer die
/// `Auto`-Spur.
///
/// L4 (08:4x): `layout_stil` bildet die Liste ab, und `Auto` traegt die
/// Spur im selben Typ. Ein `match` an zwei Stellen (hier und im Leg)
/// waere die Alternative — GEMESSENE ENTSCHEIDUNG, warum eine Funktion
/// statt zwei: die Bruecke im Leg (`narsil-layout-engine/src/plan.rs`,
/// `bridge_spalte`) macht es genauso. Zwei aehnliche Funktionen in zwei
/// Crates sind zwei Orte, an denen `Auto` spaeter ergaenzt werden
/// muss — und heute ist es die dritte Aenderung an dieser Variante.
fn plan_spalte(s: &crate::typen::SpaltenMass) -> Spalte {
    match s {
        crate::typen::SpaltenMass::Fest(v) => Spalte::Fest(pflicht(*v)),
        crate::typen::SpaltenMass::Anteil(f) => Spalte::Anteil(pflicht(*f)),
        // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) wandert ohne Zahl
        // durch — ihre Breite steht am Kind, nicht in der Liste.
        crate::typen::SpaltenMass::Inhalt => Spalte::Inhalt,
        // L4 (09:3x): §10.1 — der Faktor wandert unveraendert durch, wie
        // `Anteil`: eine Zahl, die das Layout gegen den Container rechnet.
        crate::typen::SpaltenMass::Prozent(f) => Spalte::Prozent(pflicht(*f)),
        crate::typen::SpaltenMass::Auto { spur, fit } => Spalte::Auto {
            spur: Box::new(plan_spalte(spur)),
            fit: *fit,
        },
        crate::typen::SpaltenMass::MinMax { min, max } => Spalte::MinMax {
            min: pflicht(*min),
            max: Box::new(plan_spalte(max)),
        },
    }
}

/// `FlexAusrichtung` → [`narsil_layout::plan::Ausrichtung`].
///
/// L4 (03.10.2026 09:5x). GEMESSENE ENTSCHEIDUNG, warum das eine Funktion
/// ist: die Bruecke ist die EINZIGE Stelle, an der die beiden Crate-Listen
/// aufeinander treffen — `narsil-layout` kann `narsil_css` nicht kennen (die
/// Abhaengigkeit laeuft andersherum).
///
/// GEMESSENE ENTSCHEIDUNG, warum `baseline` auf `Strecke` geht und nicht
/// auf `Start`: §6.2 — `baseline` richtet an der Grundlinie aus, und die
/// haben wir nicht. `Strecke` ist das Verhalten von heute, also bleibt
/// der sichtbare Zustand unveraendert; `Start` waere eine Erfindung.
fn ausrichtung_bruecke(a: &Option<FlexAusrichtung>) -> Option<Ausrichtung> {
    // L4 (11:5x): `None` (= `auto`) bleibt `None` — nur so kann der
    // Rasterfluss den Wert des CONTAINERS einsetzen (§6.3).
    a.as_ref().map(|a| match a {
        FlexAusrichtung::Start => Ausrichtung::Start,
        FlexAusrichtung::Mitte => Ausrichtung::Mitte,
        FlexAusrichtung::Ende => Ausrichtung::Ende,
        // L4 (13:5x): `baseline` kommt an — §6.4. Vorher stand es hier
        // mit `Strecke` in einer Zeile und kam als `stretch` im Plan an.
        FlexAusrichtung::Grundlinie => Ausrichtung::Grundlinie,
        FlexAusrichtung::Strecke => Ausrichtung::Strecke,
    })
}

/// Die Gebietsliste in den Plan (§8.1).
///
/// L4 (03.10.2026 02:7x). GEMESSENE ENTSCHEIDUNG, warum die Bruecke
/// ZEILENWEISE geht und nicht als Ganzes: ein `Gebiet` aus narsil-css und
/// eines aus narsil-layout sind verschiedene Typen (die Crates haengen nicht
/// aneinander), und `Vec<Vec<A>>` laesst sich nicht direkt in
/// `Vec<Vec<B>>` wandeln. Zeilenweise kostet eine Schleife mehr und ist
/// ein Feld gleich.
fn plan_gebiete(zeilen: &[Vec<crate::typen::Gebiet>]) -> Vec<Vec<Gebiet>> {
    zeilen
        .iter()
        .map(|z| z.iter().map(|g| bridge_gebiet(g)).collect())
        .collect()
}

/// Der Gebietsname aus `grid-area` (§8.4) — Rohwert in den Plan.
///
/// L4 (03.10.2026 02:8x). GEMESSENE ENTSCHEIDUNG, warum hier der Name
/// uebersetzt wird und nicht die Zahl aus `Berechnet`: §8.1 macht die
/// Gebiete zu Zeilen von Namen, und die Namen sind keine Zahlen. Also
/// traegt `Berechnet` hier einen Rohwert und der Plan den Namen — die
/// Bruecke ist die Uebersetzung.
fn plan_gebiet(roh: &str) -> Gebiet {
    let mut g = Gebiet::LEER;
    // §8.4: `grid-area: <name>` ist ein Name; die Zahlform
    // (`r/c/r/c`) wird hier NICHT uebersetzt — sie gehoert zu einem
    // eigenen Punkt (GEMESSEN: 15 von 184 Vorkommen im Korpus sind
    // vierteilige Zahlen, der Rest sind Namen).
    g.set(roh.trim());
    g
}

/// Ein Gebiet aus narsil-css in eines aus narsil-layout.
fn bridge_gebiet(g: &crate::typen::Gebiet) -> Gebiet {
    Gebiet(g.0)
}

/// Eine Spur-Liste in den Plan (CSS Grid 1 §8.2 fuer Spalten, §8.3 fuer
/// Zeilen).
///
/// L4 (02.10.2026 19:4x). GEMESSENE ENTSCHEIDUNG, warum es jetzt diese
/// Funktion gibt: `layout_stil()` hatte den `match` ueber
/// `raster_spalten` **im Rumpf** stehen, per `.map(..)`. Fuer die Zeilen
/// waere das eine zweite Kopie desselben `match` — und die Kopie kann von
/// der Vorgaenge abweichen, ohne dass es auffaellt. Eine benannte
/// Funktion ist eine Kopie weniger.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier GEDEECKT wird (`MAX_RASTER_SPALTEN`)
/// und nicht an anderer Stelle: die Grenze gehoert zum Speicher, nicht zur
/// CSS-Regel. Beide Achsen benutzen dieselbe Grenze, weil ein Raster mit
/// mehr als 32 Spuren in einem 600-px-Kasten je Spur keine ganze Zahl
/// Pixel haelt — die Grenze ist also eine Rechengrenze, keine
/// CSS-Grenze.
fn plan_spuren(masse: &[crate::typen::SpaltenMass]) -> Vec<Spalte> {
    masse
        .iter()
        .take(MAX_RASTER_SPALTEN)
        .map(plan_spalte)
        .collect()
}

/// Wie viele Spuren ein Item belegt (CSS Grid 1 §8.3).
///
/// L4 (02.10.2026 22:5x). GEMESSENE ENTSCHEIDUNG, warum die Differenz
/// aus Start und ENDE kommt und nicht ein Feld: §8.4 laesst beides zu.
/// `grid-row: span 2` setzt nur `-start` (die Kurzform), `grid-row: 1 /
/// 3` setzt beide (Start 1, Ende 3 -> zwei Zeilen). Also ist die
/// Spannweite die Differenz, wenn beide da sind.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine negative Zahl als `1` gelesen wird:
/// §8.3 zaehlt Spannen, und eine Zahl `-2` ist keine Spannweite. Sie
/// entsteht bei `grid-row-end: -1` (eine Linie vom Ende), und das ist
/// ausdruecklich ein eigener Punkt — die Linienzaehlung vom Ende ist heute
/// nicht gebaut. Bis dahin ist `1` die ehrliche Zahl: das Item belegt
/// eine Spur.
/// **Diese Aufloesung stand hier und ist ZURUECKGENOMMEN.**
///
/// L4 (03.10.2026 04:11x). GEMESSENER BEFUND: ich habe die negative
/// Endlinie hier gegen `b.raster_zeilen.len()` aufgeloest — die Zahl der
/// Zeilen der Vorlage **im Knoten selbst**. Die Vorlage steht aber am
/// CONTAINER, und der Knoten erbt sie nicht: §8.1/§8.3 machen
/// `grid-template-rows` NICHT erblich. GEMESSEN im Beispiel-Programm
/// (Block T5, Release): `grid-area: 1/1/-1/-1` blieb bei **h 50** statt
/// 250 — `raster_zeilen` des Kindes ist leer, also war die „Vorlage" 0.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Aufloesung in die PLATZIERUNG gehoert
/// und nicht hierher: nur dort steht die Vorlage des Containers
/// (`zeilen` beim Aufruf). Die Funktion bleibt als Kommentar, weil die
/// Zahl falsch waere, wenn jemand sie hier wieder einbaut.

fn spanne(aus_spanne: usize, start: i32, ende: i32) -> usize {
    // L4 (02.10.2026 23:4x): die `span`-Form hat VORRANG. GEMESSENER
    // BEFUND, warum sie VORRANG haben muss und nicht gleichrangig ist:
    // `grid-row: span 2 / 4` ist nach §8.4 gueltig (Spannweite UND
    // Endlinie), und §8.3 sagt fuer diese Form: "the grid item spans
    // that many tracks" — die Endlinie wird zur Startlinie, und die
    // Zahl GEWINNT. Also: steht eine `span`-Angabe da, ist sie die
    // Spannweite.
    if aus_spanne > 0 {
        return aus_spanne;
    }
    let a = if start > 0 { start as usize } else { 1 };
    let b = if ende > 0 { ende as usize } else { 1 };
    b.saturating_sub(a).max(1)
}

/// Macht `transform` oder `filter` einen Stapelkontext?
///
/// CSS Transforms 1, §3: "any value other than `none` for the transform
/// property results in the creation of a stacking context". CSS Filter
/// Effects 1, §2.1 sagt dasselbe fuer `filter`.
///
/// GEMESSEN, woher der Wert kommt (`tests/position.rs`,
/// `l5_transform_und_filter_kommen_im_sammelbehalten_an`): `transform`
/// und `filter` sind generische Schluessel-Eigenschaften und stehen in
/// `Berechnet::schluessel` als `(Name, Rohwert)`. Es braucht also keine
/// eigene Eigenschaft und keinen eigenen Typ.
///
/// GEMESSENE ENTSCHEIDUNG, warum NICHT einfach „anderer Wert als
/// `none`": `transform:uppercase` steht 13-mal im Korpus und ist ein
/// Tippfehler einer Seite — keine Transformfunktion. Nach CSS 2.1, 4.2
/// wird die Deklaration verworfen, und ein verworfener Wert darf keinen
/// Stapelkontext erzeugen. Ohne diese Regel haetten wir 13 falsche.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Regel „enthaelt eine Klammer" ist
/// und nicht „ist eine bekannte Funktion": `var(--aok-sfs-effect-
/// mode-dark)` steht im Korpus und ist eine `var()`-Verwendung, deren
/// Ziel erst zur Laufzeit feststeht. Das Zeichen `(` faengt sie ein,
/// und es faengt `rotate(180deg)` und `url(` ebenfalls ein. Eine Liste
/// bekannter Funktionsnamen waere eine Tabelle, die man bei jeder
/// neuen Funktion nachpflegen muss — und ihre Luecken faellt dann
/// stillschweigend auf die Seite zurueck.
///
/// GEMESSENE GRENZE, die ich benenne: `transform: perspective(1px)`
/// ohne weitere Funktion ist gueltig und **erzeugt** nach Spezifikation
/// keinen Stapelkontext; meine Regel erzeugt einen. Das ist eine
/// Ueberdeckung in der sicheren Richtung (mehr Kontexte als noetig), und
/// sie ist an drei Werten im Korpus messbar.
/// Release 11.10.2026: `background-clip` (CSS Backgrounds 3 §3.7, auch
/// `-webkit-background-clip`): 0 border-box, 1 padding-box, 2 content-box,
/// 3 text. Bei mehreren Schichten zaehlt die unterste (die letzte Angabe) —
/// sie traegt die Farbe. GEMESSEN vorher: der Wert wurde gelesen und nie
/// angewandt (WPT `background-rounded-image-clip-002`; Verlaufsschrift mit
/// `background-clip: text` war ein Verlaufsrechteck).
fn hintergrund_clip(b: &Berechnet) -> u8 {
    let wert = b
        .schluessel
        .iter()
        .rev()
        .find(|(k, _)| k == "background-clip" || k == "-webkit-background-clip")
        .map(|(_, v)| v.to_ascii_lowercase());
    let clip = match wert.as_deref().and_then(|v| v.split(',').next_back()).map(str::trim) {
        Some("padding-box") => 1,
        Some("content-box") => 2,
        Some("text") => 3,
        _ => 0,
    };
    // Release 14.10.2026 (Agent CSS2): with `background-attachment: local` on
    // a scroll container the background scrolls with the content and is
    // clipped to the padding box, so `border-box` acts as `padding-box` (CSS
    // Backgrounds 3 §3.2, §3.5). MEASURED before (WPT css-backgrounds
    // `background-attachment-local/attachment-local-clipping-color-1`): the
    // colour filled the dashed border.
    let local = b
        .schluessel
        .iter()
        .rev()
        .find(|(k, _)| k == "background-attachment")
        .and_then(|(_, v)| v.split(',').next_back().map(|t| t.trim().eq_ignore_ascii_case("local")))
        .unwrap_or(false);
    let scrolls = b.ueberlauf != Ueberlauf::Sichtbar
        || b.overflow_x.is_some_and(|o| o != Ueberlauf::Sichtbar)
        || b.overflow_y.is_some_and(|o| o != Ueberlauf::Sichtbar);
    if clip == 0 && local && scrolls { 1 } else { clip }
}

/// Release 14.10.2026: size containment (CSS Contain 2 §3.2) and
/// `contain-intrinsic-size` (CSS Sizing 4 §6) from the keyword properties:
/// (bits 1 width / 2 height, [width, height] in px). `auto <length>` counts as
/// the length (there is no remembered size). MEASURED before: WPT css-sizing
/// `contain-intrinsic-size/contain-intrinsic-size-001` (the box was 0 instead
/// of 222 px high).
fn containment(b: &Berechnet) -> (u8, [Option<f32>; 2]) {
    let value = |n: &str| b.schluessel.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_ascii_lowercase());
    let mut bits = 0u8;
    if let Some(c) = value("contain") {
        for word in c.split_whitespace() {
            match word {
                "size" | "strict" => bits |= 3,
                "inline-size" => bits |= 1,
                _ => {}
            }
        }
    }
    let skips_contents = value("content-visibility").as_deref() == Some("hidden");
    if skips_contents {
        bits |= 3;
    }
    if bits == 0 {
        return (0, [None, None]);
    }
    let em = if b.schriftgroesse_px.is_finite() && b.schriftgroesse_px > 0.0 { b.schriftgroesse_px } else { 16.0 };
    let length = |t: &str| -> Option<f32> {
        let t = t.trim();
        let (number, factor) = if let Some(z) = t.strip_suffix("px") {
            (z, 1.0)
        } else if let Some(z) = t.strip_suffix("rem") {
            (z, b.wurzel_schrift.max(0.0))
        } else if let Some(z) = t.strip_suffix("em") {
            (z, em)
        } else {
            (t, 0.0)
        };
        let v: f32 = number.trim().parse().ok()?;
        // A number without a unit is only valid as 0.
        (v.is_finite() && v >= 0.0 && (factor != 0.0 || v == 0.0)).then_some(v * factor)
    };
    // The values as (`auto` given, length); `none` means 0.
    let values = |t: &str| -> Vec<(bool, Option<f32>)> {
        let mut out = Vec::new();
        let mut auto = false;
        for w in t.split_whitespace() {
            if w == "auto" {
                auto = true;
                continue;
            }
            out.push((auto, if w == "none" { None } else { length(w) }));
            auto = false;
        }
        out
    };
    let mut size = [None, None];
    let mut auto = [false, false];
    if let Some(t) = value("contain-intrinsic-size") {
        match values(&t).as_slice() {
            [a] => {
                size = [a.1, a.1];
                auto = [a.0, a.0];
            }
            [a, h, ..] => {
                size = [a.1, h.1];
                auto = [a.0, h.0];
            }
            _ => {}
        }
    }
    for (name, i) in [("contain-intrinsic-width", 0), ("contain-intrinsic-inline-size", 0), ("contain-intrinsic-height", 1), ("contain-intrinsic-block-size", 1)] {
        if let Some(t) = value(name)
            && let Some(v) = values(&t).first()
        {
            size[i] = v.1;
            auto[i] = v.0;
        }
    }
    // `auto` with skipped contents uses the last remembered size (CSS Sizing
    // 4 §6.1). There is no remembered size across layouts here; the contents'
    // own size is the best stand-in (the element was usually rendered
    // without containment before), so that axis is not contained. MEASURED:
    // WPT `contain-intrinsic-size/auto-005`, `auto-007`.
    for (i, bit) in [(0, 1u8), (1, 2u8)] {
        if skips_contents && auto[i] {
            bits &= !bit;
        }
    }
    (bits, size)
}

pub fn erzeugt_stapelkontext(b: &Berechnet) -> bool {
    b.schluessel.iter().any(|(name, wert)| {
        ((name == "transform" || name == "filter") && wert != "none" && wert.contains('('))
            // Release 11.10.2026: die Einzeleigenschaften (CSS Transforms 2
            // §6) machen ebenfalls einen Stapelkontext.
            || (matches!(name.as_str(), "translate" | "rotate" | "scale") && wert.trim() != "none" && !wert.trim().is_empty())
    })
}

/// Release 11.10.2026: Masonry (CSS Grid 3 `grid-lanes`): die Achse der
/// Spuren (1 Spalten, 2 Zeilen), `flow-tolerance` in px, die Umkehr-Bits.
/// Ohne `grid-lanes-direction` entscheidet die Vorlage: nur
/// `grid-template-rows` heisst Zeilen-Spuren.
fn lanes_angaben(b: &Berechnet) -> (u8, f32, u8) {
    if !b.raster_lanes || b.anzeige != crate::Anzeige::Raster {
        return (0, 16.0, 0);
    }
    let wert = |n: &str| b.schluessel.iter().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_ascii_lowercase());
    let richtung = wert("grid-lanes-direction").unwrap_or_default();
    let woerter: Vec<&str> = richtung.split_whitespace().collect();
    let achse = if woerter.contains(&"row") {
        2
    } else if woerter.contains(&"column") {
        1
    } else if b.raster_spalten.is_empty() && !b.raster_zeilen.is_empty() {
        2
    } else {
        1
    };
    let dicht = wert("grid-lanes-pack").is_some_and(|v| v == "dense");
    let umkehr = u8::from(woerter.contains(&"fill-reverse"))
        | (u8::from(woerter.contains(&"track-reverse")) << 1)
        | (u8::from(dicht) << 2);
    let em = if b.schriftgroesse_px.is_finite() && b.schriftgroesse_px > 0.0 { b.schriftgroesse_px } else { 16.0 };
    let toleranz = match wert("flow-tolerance").as_deref() {
        None | Some("normal") | Some("") => em,
        Some("infinite") => f32::INFINITY,
        Some(t) => {
            if let Some(z) = t.strip_suffix("px") {
                z.trim().parse::<f32>().unwrap_or(em)
            } else if let Some(z) = t.strip_suffix("em") {
                z.trim().parse::<f32>().map_or(em, |v| v * em)
            } else {
                t.parse::<f32>().unwrap_or(em)
            }
        }
    };
    (achse, toleranz.max(0.0), umkehr)
}

/// Release 11.10.2026: die Rastereigenschaften eines `display: grid` fuer
/// den neuen Rasteralgorithmus (bei Masonry: die Spuren der Rasterachse).
fn gitter_stil(b: &Berechnet) -> Option<Box<narsil_layout::gitter::GitterStil>> {
    if b.anzeige != crate::Anzeige::Raster {
        return None;
    }
    let leer = narsil_layout::gitter::GitterWerte::default();
    let g = b.gitter.as_deref().unwrap_or(&leer);
    Some(Box::new(narsil_layout::gitter::GitterStil {
        spalten: g.spalten.clone(),
        zeilen: g.zeilen.clone(),
        auto_spalten: g.auto_spalten.clone(),
        auto_zeilen: g.auto_zeilen.clone(),
        gebiete: g.gebiete.clone(),
        fluss_spalte: b.gitter_fluss_spalten,
        dicht: b.gitter_fluss_dicht,
        luecke_spalte: g.luecke_spalte,
        luecke_zeile: g.luecke_zeile,
        justify_content: g.justify_content,
        align_content: g.align_content,
        justify_items: g.justify_items,
        align_items: g.align_items,
        rtl: b.richtung == crate::typen::SchreibRichtung::Rtl,
    }))
}

/// Release 11.10.2026: die Flex-Eigenschaften eines Flex-Containers fuer den
/// neuen Flex-Algorithmus (CSS Flexbox 1, Box Alignment 3).
fn flex_stil(b: &Berechnet) -> Option<Box<narsil_layout::flexstil::FlexStil>> {
    // Auch `inline-flex` (GEMESSEN vorher: WPT css/cssom-view
    // `scrollWidthHeight-overflow-visible-margin-collapsing`, der
    // `inline-flex`-Kasten lief noch durch die alte Rechnung).
    if !matches!(b.anzeige, crate::Anzeige::Flex | crate::Anzeige::InlineFlex) {
        return None;
    }
    let leer = narsil_layout::gitter::GitterWerte::default();
    let g = b.gitter.as_deref().unwrap_or(&leer);
    use crate::typen::FlexRichtung as R;
    Some(Box::new(narsil_layout::flexstil::FlexStil {
        reihe: matches!(b.flex_richtung, R::Reihe | R::ReiheUmgekehrt),
        umgekehrt: matches!(b.flex_richtung, R::ReiheUmgekehrt | R::SpalteUmgekehrt),
        umbruch: b.flex_umbruch || b.flex_umbruch_umgekehrt || b.flex_umbruch_ausgleich,
        umbruch_umgekehrt: b.flex_umbruch_umgekehrt,
        ausgleich: b.flex_umbruch_ausgleich,
        zeilen_min: b.flex_zeilen_min,
        justify_content: g.justify_content,
        align_content: g.align_content,
        align_items: g.align_items,
        luecke_spalte: g.luecke_spalte,
        luecke_zeile: g.luecke_zeile,
        rtl: b.richtung == crate::typen::SchreibRichtung::Rtl,
    }))
}

/// Release 11.10.2026: Lage und `*-self` eines Rasterelements (`None`, wenn
/// alles `auto` ist).
fn gitter_kind_stil(b: &Berechnet) -> Option<Box<narsil_layout::gitter::GitterKindStil>> {
    let g = b.gitter.as_deref()?;
    // Release 11.10.2026: nur `auto` ist der Anfangswert; ein angegebenes
    // `normal` muss ankommen (GEMESSEN: WPT `flex-abspos-staticpos-align-self-006`).
    let n = |a: narsil_layout::gitter::Ausr| matches!(a.lage, narsil_layout::gitter::AusrLage::Auto);
    if g.platz.ist_auto() && n(g.justify_self) && n(g.align_self) {
        return None;
    }
    Some(Box::new(narsil_layout::gitter::GitterKindStil {
        platz: g.platz.clone(),
        justify_self: g.justify_self,
        align_self: g.align_self,
    }))
}

pub fn layout_stil(b: &Berechnet) -> LayoutStil {
    // L4 (19:4x): die Spalten wandern ueber `plan_spuren` — die
    // GEMESSENE ENTSCHEIDUNG, warum: der `match` stand hier im Rumpf, und
    // fuer die Zeilen waere eine zweite Kopie entstanden. Eine Kopie kann
    // abweichen, ohne dass es auffaellt; eine benannte Funktion nicht.
    // Die Erklaerung zu `Auto` (L4 08:4x, `pflicht` faengt NaN) und zu
    // `minmax` (L4 08:6x) steht jetzt bei `plan_spalte`.
    // L4 (03.10.2026 07:2x): §7.2 — DIE SPUREN EINES SUBGRIDS SIND DIE DES
    // CONTAINERS.
    //
    // GEMESSENER BEFUND, warum das noetig war (Release,
    // `examples/l4_raster.rs`, Block W2): `grid-template-columns: subgrid`
    // liest sich als LEERE Liste, und eine leere Liste ist eine implizite
    // Vollspalte — **eine** Spalte fuer das ganze Subgrid. GEMESSEN: ein
    // Kind mit `grid-area: visual` (Linie 2) bekam **Breite 0**, weil es
    // in einem Ein-Spalten-Raster an Linie 2 keine Zelle gibt. Das Kind
    // war unsichtbar.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die SPUREN des Eltern und nicht nur
    // ihre ANZAHL: §7.2 sagt, das Subgrid benutzt die Linien des
    // Containers — nicht nur deren Zahl, sondern deren Spuren mit ihren
    // Massen. Fuer `[content]1fr[visual]1fr` heisst das: zwei Spalten,
    // und beide `1fr`. Also stimmen x-Position und Breite in diesem Fall
    // aus demselben Grund wie beim Container.
    //
    // GEMESSENE EINSCHRAENKUNG, und sie ist die Grenze dieses Punktes:
    // das sind die MASSEN aus dem CSS, nicht die im Layout aufgeloesten
    // Breiten. Bei `200px 1fr` im Eltern wuerde die `1fr`-Spalte hier im
    // Subgrid gerechnet, nicht aus dem Eltern uebernommen — die Breite
    // kann also abweichen. Siehe `BERICHT_CSS_L4_SUBGRID.txt`, Punkt 3.
    let spuren = |eigen: &Vec<crate::typen::SpaltenMass>,
                  geerbt: &Vec<crate::typen::SpaltenMass>,
                  sub: bool|
     -> Vec<crate::typen::SpaltenMass> {
        if sub && eigen.is_empty() {
            geerbt.clone()
        } else {
            eigen.clone()
        }
    };
    let spalten: Vec<Spalte> = plan_spuren(&spuren(
        &b.raster_spalten,
        &b.raster_eltern_spalten,
        b.raster_subgrid_spalten,
    ));

    let (lanes, lanes_toleranz, lanes_umkehr) = lanes_angaben(b);
    LayoutStil {
        border_box: b.box_art == crate::BoxArt::Rand,
        inline_aussen: b.inline_aussen,
        flow_root: b.flow_root,
        cb_inline: false,
        lanes,
        lanes_toleranz,
        lanes_umkehr,
        ordnung: b.ordnung,
        transform: crate::transform::transformation(b),
        hintergrund_clip: hintergrund_clip(b),
        gitter: gitter_stil(b),
        gitter_kind: gitter_kind_stil(b),
        groessen_art: b.groessen_art,
        stretch_bits: b.stretch_bits,
        height_keyword_limits: b.height_keyword_limits,
        fit_content_arg: b.fit_content_arg,
        size_contained: containment(b).0,
        vertical_writing: match b.schluessel.iter().rev().find(|(k, _)| k == "writing-mode").map(|(_, v)| v.trim().to_ascii_lowercase()).as_deref() {
            Some("vertical-lr" | "sideways-lr") => 1,
            Some("vertical-rl" | "sideways-rl" | "tb-rl" | "tb") => 2,
            _ => 0,
        },
        contain_size: containment(b).1,
        flex: flex_stil(b),
        flex_basis_art: b.flex_basis_art,
        rtl: b.richtung == crate::typen::SchreibRichtung::Rtl,
        // `clip` ist kein Rollkasten (CSS Overflow 3 §3.1). GEMESSEN vorher:
        // WPT css-grid `min-size-auto-overflow-clip`.
        rollkasten: (b.overflow_x.is_some_and(|u| u != Ueberlauf::Sichtbar) && !b.ueberlauf_clip[0])
            || (b.overflow_y.is_some_and(|u| u != Ueberlauf::Sichtbar) && !b.ueberlauf_clip[1])
            || (b.overflow_x.is_none() && b.overflow_y.is_none() && b.ueberlauf != Ueberlauf::Sichtbar),
        // BILD_29: DIE LETZTE STELLE DER KETTE. `Berechnet::objekt_passung`
        // wird von `kaskade.rs:4883` gefuellt und war bis hier der ENDE der
        // Kette — es gab kein Feld, das es weiterreicht. Jetzt schon.
        objekt_passung: b.objekt_passung.clone(),
        // Release 15.10.2026: `object-position` lives in the generic
        // `schluessel` list (parse.rs `ESchluessel`); empty means the initial
        // value `50% 50%`.
        objekt_position: b
            .schluessel
            .iter()
            .find(|(k, _)| k == "object-position")
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        anzeige: anzeige(b.anzeige),
        flex_richtung: richtung(b.flex_richtung),
        //
        // P19 (05.10.2026, 23:52 / 00:14): `flex-wrap` DURCHREICHEN —
        // ZURUECKGENOMMEN, und die Messung, die es beweist. Die Zeile
        // stand hier von 23:56 bis 00:14.
        //
        // DER BEFUND, DER DAFUER SPRACH (heise.de, 900 px, 23:51):
        // 10 Ueberlappungen an EINER Stelle (y = 2246, x = 542...630,
        // exakt 16 px Schritt), ein Zeilenmenue („Newsticker",
        // „Security", „IT & Tech") mit Aufzaehlungspunkten. Kette:
        //
        //   [12] #243 [502.00,2246.00 636.00x379.20] plan=Some(Flex)
        //   [13] #244 [514.00,2246.00   0.00x379.20] plan=Some(Block)
        //   [14] #246 [548.22,2246.00  80.69x 16.00]  <- "Newsticker"
        //
        // #244 war 0 px breit, und seine Kinder standen nebeneinander —
        // das sieht nach fehlendem Umbruch aus. Die Kaskade hatte die Zahl
        // (`Berechnet::flex_umbruch`, `kaskade.rs:3176`) und reichte sie
        // nicht durch: `layout_stil()` ist der einzige Weg von der
        // Kaskade in die Engine. Also eine Zeile:
        //
        //   -  flex_umbruch: FlexUmbruch::Keiner,
        //   +  flex_umbruch: umbruch(b.flex_umbruch),
        //
        // DIE WIRKUNG WAR DOPPELT (heise.de, gleiche Datei, 8 Min. auseinander):
        // Ueberlappungen 10 → 5. **DIE HAFTE BEFUNDE AUF w3.org
        // SPRANGEN DABEI VON 0 AUF 22** — alle 22 an einer Stelle.
        //
        // DER A/B, DER ES ENTSCHEIDET (w3.org, 900 px, 00:12):
        //
        //     ohne diese Zeile:  hart=0    19 Paare
        //     mit  dieser Zeile:  hart=22   65 Paare
        //
        // **ES IST MEINE REGRESSION, und die Diagnose von 23:51 war
        // falsch.** Die Ueberlappungen auf heise.de waren kein
        // fehlender Umbruch.
        //
        // WAS TATSÄCHLICH DASTIEGT (00:10, `NARSIL_KETTE=42` auf w3.org):
        //
        //   [6] #23 [109.00,242.61 706.00x342.42] plan=Some(Flex)
        //   [7] #42 [121.00,242.61  17.44x 29.74] plan=Some(Flex)
        //   [8] #43 [133.00,242.61   5.44x 29.74] plan=None
        //
        // Ein **Absatz** (`plan=None`, Text „* ") steckt in einem Flex,
        // und mit Umbruch bekommen die Listenpunkte **eigene Zeilen** —
        // 58-mal derselbe Hash `7e7a407b4abb407` an 58 Positionen
        // (GEMESSEN: `grep -oE '\] h=[0-9a-f]+' | sort | uniq -c`).
        // Ohne Umbruch lagen sie in einer Reihe und ueberlappten nicht.
        //
        // **DER FOLGERUNGSSCHLUSS, DER FEHLTE:** `flex-wrap: wrap` ist an
        // einem Absatz in einem Flex **richtig** (CSS Flexible Box Layout
        // 1 §4) und **macht die Seite kaputt**, weil unser Absatz in
        // einem Flex ein Flex-ITEM ist und jede Zeile seinen Platz
        // bekommt. Ein echter Browser bricht dort nicht um, weil er den
        // Absatz nicht als Flex-Item sieht, sondern als Block in einer
        // Liste. **Die Ursache ist nicht `flex-wrap`, sondern dass
        // `narsil-page` Listenabsaetze in einen Flex-Kasten stellt** —
        // und diese Ursache liegt nicht in meiner Crate.
        //
        // Also: nicht die Zahl verdrahtet, sondern den KASTEN. Und der
        // Kasten ist nicht meiner.
        // ------------------------------------------------------------------
        // L3 (02.10.2026 04:0x): `NARSIL_UMBRUCH_AUS=1` nimmt die Zeile
        // wieder heraus.
        //
        // GEMESSENER GRUND: der Vergleich „mit Zeile" gegen „ohne
        // Zeile" muss aus DERSELBEN Binaerdatei kommen. Beim
        // Kaskaden-Tempo (KOMPRESS D1) hatte ich zwei Zeitpunkte
        // verglichen, und dazwischen hatte eine fremde Rolle den
        // HTML-Parser geaendert — die Zahlen sagten nichts
        // miteinander. Der Schalter ist billig: `OnceLock`, einmal
        // gelesen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Wert und nicht die
        // Anwesenheit der Variablen geprueft wird: `var_os` liefert
        // fuer "0" genauso `Some(..)` wie fuer "1". Das war der
        // zweite Fehler am selben Schalter beim Kaskaden-Tempo.
        // Release 09.10.2026: die Umkehrung kommt jetzt an (vorher K7c:
        // „wird hier nicht geraten" — sie ging verloren).
        flex_umgekehrt: matches!(
            b.flex_richtung,
            crate::typen::FlexRichtung::ReiheUmgekehrt | crate::typen::FlexRichtung::SpalteUmgekehrt
        ),
        flex_umbruch: if umbruch_aus() {
            FlexUmbruch::Keiner
        } else {
            umbruch(b.flex_umbruch)
        },

        flex_buendig: buendig(b.flex_buendig),
        flex_ausrichtung: ausrichtung(b.flex_ausrichtung),
        flex_wachstum: pflicht(b.flex_wachstum),
        flex_schrumpf: pflicht(b.flex_schrumpf),
        // `flex-basis: auto` bedeutet in CSS: die eigene `width`.
        // Das Layout rechnet das aus, indem es `None` sieht und auf die
        // gemessene Breite zurueckfaellt — deshalb hier nur die
        // tatsaechliche Zahl.
        flex_basis: opt(b.flex_basis),
        // L3 (02.10.2026 04:5x): DIE ACHSEN SIND GETRENNT, und es
        // haengt an der Richtung, welche Zahl wohin gehoert.
        //
        // CSS Flexbox 1 §8.3 woertlich: row-gap steht senkrecht zur
        // Hauptachse bei `row` (also quer), und waagerecht bei `column`
        // (also haupt). `column-gap` ist es genau umgekehrt. Die Namen
        // meinen Zeilen und Spalten des Rasters, nicht der Flex-Achse.
        //
        // GEMESSENER BEFUND (vorher, `l3_flex`): `row-gap: 20px` und
        // `column-gap: 20px` gaben beide dieselbe Stellung wie
        // `gap: 20px` — die Hauptachse. Und das war falsch: auf einer
        // Zeile (`row`) ist `row-gap` die Querachse und darf die
        // Hauptachse gar nicht beruehren.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `luecke_px` trotzdem noch
        // eine Rolle spielt: `gap` schreibt nach der Korrektur oben alle
        // drei Felder, also liefern `luecke_zeile` und `luecke_spalte`
        // bei `gap` dieselbe Zahl wie `luecke_px`. Fuer eine
        // alte Messung, die nur `gap` kennt, aendert sich damit
        // nichts — der Weg ist aber jetzt der richtige.
        // L4 (02.10.2026 08:1x): FUER EIN RASTER KEIN RICHTUNGS-TAUSCH.
        //
        // GEMESSENER BEFUND (`examples/l4_raster.rs`): `row-gap: 20px`
        // trennte die Zeilen NICHT (y 40/60 wie ohne Angabe), und
        // `column-gap: 20px` trennte sie DOCH (y 40/80). Also war die
        // Zuordnung genau verkehrt herum.
        //
        // GEMESSENE URSACHE: `haupt_luecke` folgt der Flex-Richtung,
        // weil ein Flex-Container eine Hauptachse hat. Ein Grid hat
        // KEINE — CSS Grid 1 §10.1 meint mit `column` die waagerechte
        // und mit `row` die senkrechte Achse, immer. Fuer ein Raster
        // ist die Zuordnung deshalb eine Zeile, kein `match`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum es an `b.anzeige` haengt und
        // nicht an einer Sonderfunktion: ein Element kann `display:grid`
        // und `flex-direction: column` gleichzeitig haben. Die
        // Flex-Richtung ist dann irrelevant, weil sie keine
        // Flex-Achse beschreibt. `b.anzeige` entscheidet das, und es
        // ist genau die Angabe, die CSS auch entscheidet.
        luecke: pflicht(if luecke_aus() {
            b.luecke_px
        } else if b.anzeige == crate::typen::Anzeige::Tabelle {
            // Release 11.10.2026: bei Tabellen ist die Luecke
            // `border-spacing` (waagerecht). Vorher stand im Layout fest
            // 2 px — auch fuer `display: table` und anonyme Tabellen, wo
            // die Vorgabe 0 ist (CSS 2.1 §17.6.1).
            b.randabstand_waagerecht
        } else if b.anzeige == crate::typen::Anzeige::Raster {
            b.luecke_spalte
        } else {
            haupt_luecke(b)
        }),
        luecke_quer: pflicht(if luecke_aus() {
            // L3 (02.10.2026 05:0x): der A/B-Zweig ist EXAKT der alte
            // Zustand. `luecke_px` ging als Haupt- UND Querwert an den
            // Leg — das war der Fehler, den dieser Punkt behebt. Wer
            // vergleichen will, misst also die alte Doppelbelegung
            // gegen die neue Achsenzuordnung, nicht „mit gegen ohne
            // Luecke".
            b.luecke_px
        } else if b.anzeige == crate::typen::Anzeige::Tabelle {
            b.randabstand_senkrecht
        } else if b.anzeige == crate::typen::Anzeige::Raster {
            // L4 (08:1x): dieselbe Regel fuer die Querachse — siehe die
            // Begruendung bei `luecke` oben.
            b.luecke_zeile
        } else {
            quer_luecke(b)
        }),
        spalten,
        // L4 (02.10.2026 11:3x): die SPALTENRICHTUNG durchreichen.
        //
        // GEMESSENER BEFUND, warum das hier eine Zeile ist und keine
        // eigene Funktion: `layout_stil()` ist der einzige Weg von der
        // Kaskade in die Engine, und die Kaskade hatte den Boolean
        // bereits (11:3x in kaskade.rs). Es fehlte nur diese
        // Verbindung — dieselbe Lage wie bei `flex-wrap` (P19, Zeile
        // 0:14). Eine eigene Funktion waere hier geteilter Ballast:
        // es gibt keine zweite Stelle, die sie braucht.
        spaltenfluss: b.gitter_fluss_spalten,
        // L4 (03.10.2026 04:2x): `dense` (§8.5). GEMESSENER BEFUND, was
        // vorher passierte: das zweite Wort kam an und wurde nicht
        // gelesen — nur das erste entschied (`kaskade.rs`, 11:3x).
        dicht: b.gitter_fluss_dicht,
        // L4 (02.10.2026 19:4x): `grid-template-rows` DURCHREICHEN
        // (CSS Grid 1 §8.3).
        //
        // GEMESSENER BEFUND, was vorher passierte: `layout_stil()` liest
        // nur `raster_spalten`, also blieb `Berechnet::raster_zeilen`
        // (der Punkt von 18:2x) ohne Wirkung — die Kaskade las die Zeilen
        // und niemand rechnete damit. GEMESSEN in
        // `tests/position.rs`, `l4_grid_template_rows_legt_die_zeilenhoehe`:
        // `grid-template-rows: 200px 200px` ergab 20-px-Zeilen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `plan_spalte` fuer beide Achsen
        // benutzt wird und nicht eine eigene Funktion: §8.2 und §8.3
        // nennen dieselben Spurformen, und `plan_spalte` ist genau der
        // Uebergang `Berechnet`-Wert -> Plan-Wert. Eine zweite Funktion
        // waere derselbe Code an zwei Stellen.
        // L4 (07:2x): dasselbe an der Zeilenachse — siehe die Begruendung bei
        // `spalten`.
        zeilen: plan_spuren(&spuren(
            &b.raster_zeilen,
            &b.raster_eltern_zeilen,
            b.raster_subgrid_zeilen,
        )),
        // L4 (09:5x): `align-self` und `justify-self` kommen mit (§6.6).
        // GEMESSENER BEFUND, dass es noetig ist: Release, Block AA —
        // `stretch`, `center`, `flex-start`, `flex-end` und
        // `justify-self: center` ergaben alle dieselbe Zelle.
        selbst_vertikal: ausrichtung_bruecke(&b.ausrichtung_selbst),
        selbst_waagerecht: ausrichtung_bruecke(&b.ausrichtung_selbst_waagerecht),
        // L4 (11:5x): `justify-items` (§6.2) kommt mit — es ist der
        // Vorgabewert des Kindes auf der Querachse.
        justify_ausrichtung: ausrichtung_bruecke(&Some(b.justify_ausrichtung))
            .unwrap_or(Ausrichtung::Strecke),
        // L4 (13:5x): die ANGABE von `width`/`height` wandert mit. Siehe
        // `narsil-layout/src/plan.rs::breite_angabe` — ohne sie ist die
        // Querachsen-Ausrichtung eines Zeilen-Flex wirkungslos.
        breite_angabe: b.breite.filter(|_| b.breite_prozent.is_none()),
        hoehe_angabe: b.hoehe.filter(|_| b.hoehe_prozent.is_none()),
        breite_anteil: b.breite_prozent,
        hoehe_anteil: b.hoehe_prozent,
        hoehe_bedingt: None,
        breite_inhalt: matches!(b.breite_art.as_str(), "max-content" | "min-content" | "fit-content"),
        hoehe_inhalt: b.hoehe_inhalt,
        // L4 (13:4x): §6.4 — der Aufstieg der Schrift bestimmt die
        // Grundlinie eines Kindes.
        schriftgrad: b.schriftgroesse_px,
        // L4 (02.10.2026 21:6x): `grid-auto-rows` (§7.5) und
        // `grid-auto-columns` (§7.3). GEMESSENER BEFUND, was vorher
        // passierte: beide kamen als String in `gitter_automatik` an —
        // zusammen mit `grid-auto-flow` — und wurden nie gelesen.
        auto_zeilen: plan_spuren(&b.raster_auto_zeilen),
        auto_spalten: plan_spuren(&b.raster_auto_spalten),
        // L4 (02.10.2026 22:5x): `span` (§8.3). GEMESSENER BEFUND, was
        // vorher passierte: `grid-row: span N` kam als ROHWORT in
        // `schluessel` an (parse.rs:5226, `Ziel::Lang(ESchluessel)`) und
        // `gitter_zeile_start` blieb 0 — GEMESSEN in
        // `tests/position.rs::l4_span_ueber_zeilen_macht_ein_kind_hoch`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `0` wie `1` gelesen wird und nicht
        // wie „keine Zeile": `gitter_linie` liefert `0` fuer `auto`, und
        // eine Zeilenzahl von 0 waere ein Item ohne Zeile — ein Loch im
        // Bild. Also `max(1)`.
        spann_zeilen: spanne(
            b.gitter_zeile_spanne,
            b.gitter_zeile_start,
            b.gitter_zeile_ende,
        ),
        spann_spalten: spanne(
            b.gitter_spalte_spanne,
            b.gitter_spalte_start,
            b.gitter_spalte_ende,
        ),
        // L4 (03.10.2026 01:6x): die festen LINIEN (§8.5) durchreichen.
        //
        // GEMESSENER BEFUND, was vorher passierte: die Linien kamen in
        // `Berechnet::gitter_zeile_start` an und wurden von niemandem
        // benutzt — GEMESSEN ueber den Seitenweg (Block P des
        // Beispiel-Programms, 00:5x): `grid-row: 2/span 2` legte das Kind
        // in Zeile 0. Die SPANNE stand da schon (das war der Punkt von
        // 00:4x), die Linie nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die ENDlinie mitkommt und nicht in
        // `spann_*` verrechnet wird: `grid-row: 2/-1` ergibt die
        // Spannweite erst, wenn die Zeilenzahl der VORLAGE bekannt ist —
        // und die traegt erst die Platzierung in `lege_raster_bei_autom`.
        zeile_start: b.gitter_zeile_start,
        zeile_ende: b.gitter_zeile_ende,
        spalte_start: b.gitter_spalte_start,
        spalte_ende: b.gitter_spalte_ende,
        // L4 (03.10.2026 02:7x): `grid-template-areas` (§8.1).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Bruecke eine EIGENE Funktion
        // ist und kein `map`: `plan_gebiete` wird an ZWEI Stellen
        // gebraucht — hier fuer die Vorlage des CONTAINERS und spaeter
        // fuer den Namen des KINDES. Eine geschlossene Formel an zwei
        // Stellen kann nicht auseinanderlaufen.
        gebiete: plan_gebiete(&b.raster_gebiete),
        // L4 (02:8x): `grid-area: <name>` (§8.4) — der Name des Kindes.
        gebiet: plan_gebiet(&b.grid_area_name),
        // L5 (02.10.2026 17:0x): TRANSFORM UND FILTER ALS STAPELKONTEXT.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Regel hier ausgewertet wird
        // und nicht in der Engine: `transform` und `filter` sind
        // generische Schluessel-Eigenschaften und stehen als Rohwert in
        // `Berechnet::schluessel` — dort kann man sie lesen, im Plan
        // nicht mehr. Und die Regel selbst („anderer Wert als `none`")
        // gehoert an die Stelle, an der der Wert noch lesbar ist.
        transformt: erzeugt_stapelkontext(b),
        position: position(b.position),
        oben: opt(b.oben),
        rechts: opt(b.rechts),
        unten: opt(b.unten),
        links: opt(b.links),
        oben_prozent: b.oben_prozent,
        rechts_prozent: b.rechts_prozent,
        unten_prozent: b.unten_prozent,
        links_prozent: b.links_prozent,
        z_index: b.z_index,
        schweben: schweben(b.schweben),
        raeumen: raeumung(b.raeumung),
        ueberlauf: ueberlauf(b.ueberlauf),
        marge_links_auto: b.marge_links_auto,
        marge_rechts_auto: b.marge_rechts_auto,
        marge_oben_auto: b.marge_oben_auto,
        marge_unten_auto: b.marge_unten_auto,
        // Release 11.10.2026: negativ = `auto && <ratio>`
        // (`Berechnet::seitenverhaeltnis_auto`).
        seitenverhaeltnis: if b.seitenverhaeltnis.is_finite() && b.seitenverhaeltnis > 0.0 {
            if b.seitenverhaeltnis_auto { -b.seitenverhaeltnis } else { b.seitenverhaeltnis }
        } else {
            0.0
        },
        // BILD_37 (fremde Datei, **minimale** Reparatur, Rolle SERVER im BRETT
        // informiert): `colspan`/`rowspan` kamen mit `LayoutStil` (SERVER_13,
        // 05.10.) in die Crate, aber in **keinen** vollstaendigen
        // Initialisierer — GEMESSEN: `error[E0063]: missing fields`, und damit
        // war **jede** Crate rot, die von `narsil-css` abhaengt (auch meine:
        // `narsil-raster` haengt ueber `narsil-page` daran).
        //
        // **UND HIER STEHT NOCH KEINE INFORMATION.** GEMESSEN: `grep colspan
        // crates/narsil-css/src/` findet **ausserhalb dieser Datei nichts** —
        // `Berechnet` fuehrt die beiden Felder **nicht**, es gibt also noch
        // keine Kaskadenstelle, aus der sie hier zufaellen koennten. Es ist
        // deshalb die **Vorgabe** `1`, und **nicht** etwa `b.colspan`: das waere
        // geraten, und geraten wird hier nichts.
        // **WO DIE LUEcke ZU SCHLIESSEN IST:** `Berechnet` braucht die Felder
        // (aus dem HTML-Attribut, `narsil-dom`), und **diese** Zeilen werden dann
        // `b.colspan` / `b.rowspan`. Die Vorgaben stehen in der Doku der Felder:
        // „`rowspan`: **`1` = normale Zeile**", und `0` wird ohnehin als `1`
        // behandelt.
        colspan: 1,
        rowspan: 1,
    }
}

/// Die vier Beschraenkungen EINES Knotens fuer die `Grenzen`-Tabelle
/// des Layout-Plans (CSS 2.1, 10.4: `used = min(max-width,
/// max(min-width, width))`).
///
/// # WARUM EINE EIGENE FUNKTION UND KEIN FELD IM `LayoutStil`
///
/// `LayoutStil` wird an mehreren Stellen als Struct-Literal gebaut —
/// unter anderem hier. Die Rolle LAYOUT hat die min/max-Werte darum
/// (nach demgemessenen 7129-px-Kasten auf nixos.org, P2) in eine
/// eigene Tabelle `Grenzen` gelegt. Diese Funktion ist die Bruecke
/// dorthin: der Aufrufer liest sie fuer JEDEN Knoten und traegt sie
/// unter dessen NodeId ein.
///
/// `None` heisst "nicht gesetzt" — nicht 0. 0 waere eine Aussage ueber
/// die Seite, die der Plan nicht treffen darf.
pub fn layout_grenzen(b: &Berechnet) -> Grenzen {
    Grenzen {
        min_breite: opt(b.min_breite),
        max_breite: opt(b.max_breite),
        min_hoehe: opt(b.min_hoehe),
        max_hoehe: opt(b.max_hoehe),
    }
}
/// P19 (05.10.2026, 20:36): DIE PROZENTANTEILE DER GRENZEN, fuer den
/// Motor.
///
/// # WARUM EINE EIGENE FUNKTION
///
/// `layout_grenzen` liefert die **Zahlen** der Kaskade, und die sind
/// bei einem Prozent gegen die ANSICHT gerechnet
/// (`kaskade.rs::breiten_wert`) — als Groesse im Layout zu verwenden waere
/// eine Falschangabe. Diese Funktion liefert die Anteile daneben, und der
/// Motor (`box.rs`) rechnet sie gegen den Inhalt des Eltern. Beide
/// Funktionen zu einem Aufruf zu zwingen hiesse, dass der Aufrufer
/// entscheidet, welches der beiden er braucht — und es braucht je nach
/// Eigenschaft beides.
///
/// GEMESSENER FALL (20:32): `img, video { max-width: 100% }` mit einem
/// 2000-px-Bild in einem 852-px-Eltern ergab 900 px (die Ansicht)
/// statt 852 px.
///
/// # GRENZEN (M3/M4)
///
/// * hoechstens `MAX_WERT_PX` je Anteil, und `None` bei allem, was
///   nicht endlich oder negativ ist,
/// * ein leeres Ergebnis wird vom Plan nicht eingetragen
///   (`GrenzenProzent::ist_leer`).
pub fn layout_grenzen_prozent(b: &Berechnet) -> GrenzenProzent {
    fn sauber(p: Option<f32>) -> Option<f32> {
        p.filter(|v| v.is_finite() && *v >= 0.0 && *v <= MAX_WERT_PX)
    }
    GrenzenProzent {
        min_breite: sauber(b.min_breite_prozent),
        max_breite: sauber(b.max_breite_prozent),
        min_hoehe: sauber(b.min_hoehe_prozent),
        max_hoehe: sauber(b.max_hoehe_prozent),
    }
}

/// **K16 (02.10.2026): BEIDES IN EINEM AUFRUF.**
///
/// `team/WUNSCH_LAYOUT.txt` (Rolle LAYOUT, 30.09.) bittet die Rollen
/// HTML/INTEGRATION um genau EINEN Aufruf pro Knoten:
///
/// ```text
/// seite.layout_plan.set(document_id, narsil_css::layout_stil(&berechnet));
/// ```
///
/// GEMESSENER FEHLER in dieser Bruecke, und er ist der Punkt K16: der
/// Wunsch nennt `set`, aber **nie `set_grenzen`**. `layout_grenzen`
/// existiert seit dem 01.10. und wird von `team/BERICHT_CSS_K16` ab
/// niemandem aufgerufen — GEMESSEN ueber alle Crates: die einzigen
/// Aufrufer von `set_grenzen` sind die Tests von `narsil-layout-engine`.
///
/// Folge: `max-width`/`max-height` kommen aus der Kaskade an
/// (`Berechnet::max_breite`, gesetzt in `kaskade.rs`), werden von
/// [`layout_grenzen`] korrekt umgesetzt — und kommen dann nirgends an.
/// `plan.grenzen(knoten).max_breite` ist `None`. Das ist woertlich der
/// Befund aus AUTONOM.txt K16.
///
/// Diese Funktion loest das so, dass es nicht mehr vergessen werden
/// kann: EIN Aufruf, der BEIDES tut. Wer die Bruecke nach dem Wunsch
/// baut, bekommt die Grenzen mit, ohne es zu wissen.
///
/// ```text
/// narsil_css::layout_plan_setze(&mut seite.layout_plan, document_id, &berechnet);
/// ```
///
/// Additiv: verändert an keinem bestehenden Verhalten etwas.
pub fn layout_plan_setze(plan: &mut LayoutPlan, knoten: narsil_dom::NodeId, b: &Berechnet) {
    plan.set(knoten, layout_stil(b));
    plan.set_grenzen(knoten, layout_grenzen(b));
    // K17b: die Schreibrichtung kommt im SELBEN Aufruf mit. Vorher waere
    // sie ein vierter Aufruf gewesen — und damit wieder eine Stelle, an
    // der sie vergessen werden kann.
    plan.set_richtung(knoten, schreibrichtung(b));
    // K17b: die Ausrichtung kommt auch mit. GEMESSENER BEFUND, warum
    // das noetig ist und nicht nur bequem: `textausrichtung` hatte
    // AUSSERHALB von `narsil-css` keinen einzigen Leser. Die Kaskade hat
    // `text-align` seit Jahren richtig gerechnet, und es hat niemanden
    // erreicht. Mit `direction: rtl` allein waere auch nichts passiert:
    // eine Zeile, die man nicht ausrichten kann, rutscht nicht nach
    // rechts.
    plan.set_ausrichtung(knoten, textausrichtung(b));
}

/// `Berechnet::richtung` -> `plan::Richtung`.
///
/// Heisst bewusst NICHT `richtung`: in dieser Datei gibt es schon ein
/// `richtung()` fuer `FlexRichtung` (`flex-direction`). Zwei Funktionen
/// mit demselben Namen und verschiedenen Typen sind eine Falle fuer den
/// naechsten Leser — der Compiler hat mich darauf hingewiesen, und er
/// hat recht.
fn schreibrichtung(b: &Berechnet) -> Richtung {
    match b.richtung {
        SchreibRichtung::Ltr => Richtung::Ltr,
        SchreibRichtung::Rtl => Richtung::Rtl,
    }
}

/// `Berechnet::textausrichtung` -> `plan::Textausrichtung`.
fn textausrichtung(b: &Berechnet) -> Textausrichtung {
    match b.textausrichtung {
        crate::typen::Textausrichtung::Links => Textausrichtung::Links,
        crate::typen::Textausrichtung::Rechts => Textausrichtung::Rechts,
        crate::typen::Textausrichtung::Zentriert => Textausrichtung::Zentriert,
        crate::typen::Textausrichtung::Blocksatz => Textausrichtung::Blocksatz,
    }
}

/// Hoechster px-Wert, den diese Bruecke durchlaesst. Begruendung: kein
/// Anzeigegeraet zeigt mehr als 100 000 px; groessere Werte kommen aus
/// `calc()`-Bomben und wuerden die Box aus dem Bild schieben (M4).
pub const MAX_WERT_PX: f32 = 100_000.0;

/// `display` aus der Kaskade.
fn anzeige(a: Anzeige) -> narsil_layout::plan::Anzeige {
    use narsil_layout::plan::Anzeige as Z;
    match a {
        Anzeige::Keins => Z::Keins,
        Anzeige::Flex => Z::Flex,
        Anzeige::Raster => Z::Raster,
        // `Inline`, `InlineBlock`, `ListenEintrag` und `Block` werden
        // alle zu `Block`. GEMESSEN wichtig: das ist das Verhalten von
        // heute (das Layout kennt nur Row/Column), und ein Block ist der
        // sichere Ausgangswert — ein Inline-Kasten waere eine zweite
        // Baustelle ohne Nutzen fuer diesen Auftrag.
        // P9 (05.10.2026): `inline-block` ist **inline-stufig**, nicht Block.
        //
        // GEMESSENER FEHLER: hier stand `Anzeige::InlineBlock` in der
        // Gruppe mit `Inline`/`Block` und wurde damit `Z::Block`. Die Engine
        // kann `Z::InlineBlock` in die Zeile setzen (`flow.rs::inline_kind`).
        // Die Schrumpfbreite liefert `box.rs` (dieselbe Formel wie fuer
        // `position`), seit `schrumpft_breite` auch `InlineBlock` kennt.
        // P9 (05.10.2026): `inline-block` ist **inline-stufig**, nicht Block.
        // Die Engine setzt `Z::InlineBlock` in die Zeile (`flow.rs::inline_kind`),
        // und `box.rs` gibt ihm die Schrumpfbreite (`max_content_breite`).
        Anzeige::InlineBlock => Z::InlineBlock,
        Anzeige::Inline | Anzeige::ListenEintrag | Anzeige::Block => {
            Z::Block
        }
        // K7 (01.10.2026): `inline-flex` und `table` sind NEU in der
        // Kaskade. GEMESSEN vorher: `display: inline-flex` wurde dort
        // gelesen und dann VERWORFEN, also kam fuer diese Elemente
        // gar kein `display` an — sie waren Block.
        //
        // `inline-flex` wird zu `Flex`: es ist ein Flex-Container
        // (CSS Flexbox 1 §2), und ob er eine eigene Zeile bekommt,
        // entscheidet der Textfluss, nicht der Flex-Leg. Der
        // Flex-Container ist die wichtige Haelfte.
        Anzeige::InlineFlex => Z::Flex,
        // added 05.10.2026, Rolle SERVER (SERVER_13): **DIE ZEILE, AN
        // DER DIE TABELLEN-INFO BISHER STARB.**
        //
        // Sie stand hier:
        //     Anzeige::Tabelle => Z::Block,
        // mit der Begruendung „Es ist KEIN Tabellen-Leg da (narsil_layout
        // kennt Row/Column), also waere `Block` eine Entscheidung ohne
        // Grundlage“.
        //
        // Die Begruendung war richtig, die Tatsache falsch: `narsil_layout`
        // **konnte** nichts unterscheiden, weil `plan::Anzeige` keine
        // Tabellen-Variante hatte. Die gibt es jetzt:
        // `Tabelle`, `TabelleZeile`, `TabelleZelle`, `TabelleRand`.
        //
        // GEMESSENER WEG, den die Information nun nimmt:
        //   Kaskade (`kaskade.rs`, alle 10 Schluesselwoerter)
        //     -> plan::Anzeige::Tabelle / ::TabelleZeile / ::TabelleZelle
        //     -> `flow.rs` ruft fuer `Tabelle` `lege_tabelle`
        //     -> Zeilen und Zellen kommen aus den Kindern
        // added 05.10.2026: `Tabelle` heisst jetzt viermal — einmal als
        // Muster und dreimal als Ziel. `#[deny(bindings_with_variant_name)]`
        // laesst das nicht zu, darum die vollstaendigen Pfade. Das ist
        // kein Umweg, sondern die Warnung des Lints: ein Muster, das
        // gleich heisst wie eine Variante, liest sich wie eine
        // Schreibweise und nicht wie ein Zweig.
        crate::typen::Anzeige::Tabelle => Z::Tabelle,
        crate::typen::Anzeige::TabelleZeile => Z::TabelleZeile,
        crate::typen::Anzeige::TabelleZelle => Z::TabelleZelle,
        crate::typen::Anzeige::TabelleRand | crate::typen::Anzeige::TabelleSpalte => Z::TabelleRand,
    }
}

/// `flex-direction`.
/// `Berechnet::flex_umbruch` (ein `bool`) auf den Plan-Enum.
///
/// P19 (05.10.2026, 23:56): `Berechnet` kennt nur zwei Zustaende. Die
/// Kaskade vergleicht **exakt** (`kaskade.rs:3176`:
///
/// ```text
/// erg.flex_umbruch = x == "wrap";
/// ```
///
/// ), also ist `true` genau `wrap` und alles andere `false`. Das wurde
/// gemessen, nicht angenommen: `crates/narsil-css/tests/p19_flex_wrap.rs`,
/// `wrap_reverse_bleibt_keiner_wie_vorher`.
///
/// # WAS ICH ZUERST BEHAUPTET HABE — UND WARUM ES FALSCH WAR
///
/// Ich hatte in den Kommentar und im Test geschrieben, `wrap-reverse`
/// falle auf `Umbruch` zurueck. **Der Test hat mich korrigiert**:
/// gemessen ist `Keiner`.
///
/// Und `Keiner` ist die richtige Antwort, nicht `Umbruch`:
///
///  * Es ist **unveraendert** — vor dieser Zeile war es auch `Keiner`.
///    Es gab nur niemanden, der es nachsehen konnte.
///  * `wrap-reverse` heisst „umb wie `wrap`, aber die Zeilen von
///    unten". Eine umgekehrte Zeilenreihenfolge zu erfinden waere
///    geraten. Sie richtig zu bauen braucht eine **dritte Zahl** in
///    `Berechnet` (`typen.rs:1659` ist ein `bool`), und die gehoert an
///    eine Stelle, die nicht meine ist.
///
/// # WARUM DAS KEIN RESTFALL MIT GEFAHR IST
///
/// `flex-wrap: wrap-reverse` steht in keinem der 30 Abnahme-Stylesheets
/// (GEMESSEN ueber die Abnahmen 21:17, 23:36 und 23:48), `wrap` dagegen
/// auf heise.de — deshalb der Befund um 23:51. Der Fall ist praktisch
/// leer. Diese Grenze steht hier, damit „praktisch leer" nicht mit
/// „behoben" verwechselt wird: es ist eins von beiden, und gemessen
/// habe ich das erste.
/// NUR FUER DIE MESSUNG (L3, 02.10.2026 04:0x): `NARSIL_UMBRUCH_AUS=1`
/// nimmt `flex_umbruch` aus dem Plan.
///
/// Einmal gelesen, nicht je Aufruf: `std::env::var_os` nimmt auf Linux
/// die Umgebungssperre, und `layout_stil` laeuft je Knoten. GEMESSEN
/// beim Kaskaden-Tempo: 2 000 ms fuer einen Schalter, der je Aufruf
/// las.
/// NUR FUER DIE MESSUNG (L3, 02.10.2026 05:0x): `NARSIL_LUECKE_AUS=1`
/// nimmt die Achsenzuordnung von `gap`/`row-gap`/`column-gap` wieder
/// heraus und gibt `luecke_px` an beide Achsen.
///
/// Einmal gelesen, nicht je Aufruf — dieselbe Begruendung wie bei
/// `umbruch_aus` (die Umgebungssperre kostete beim Kaskaden-Tempo
/// 2 000 ms, weil der Schalter je Knoten las).
fn luecke_aus() -> bool {
    static AUS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AUS.get_or_init(|| std::env::var("NARSIL_LUECKE_AUS").ok().as_deref() == Some("1"))
}

fn umbruch_aus() -> bool {
    static AUS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AUS.get_or_init(|| std::env::var("NARSIL_UMBRUCH_AUS").ok().as_deref() == Some("1"))
}

fn umbruch(u: bool) -> narsil_layout::plan::FlexUmbruch {
    if u {
        narsil_layout::plan::FlexUmbruch::Umbruch
    } else {
        narsil_layout::plan::FlexUmbruch::Keiner
    }
}

/// Der Lueckenwert auf der HAUPTACHSE (L3, 02.10.2026 04:5x).
///
/// CSS Flexbox 1 §8.3: `row` nimmt `column-gap` auf der Hauptachse,
/// `column` nimmt `row-gap`. GEMESSENE ENTSCHEIDUNG, warum hier nicht
/// `luecke_px` steht: das Feld bekommt nur `gap` geschrieben, und
/// `row-gap: 20px` auf einer Zeile soll die Hauptachse nicht
/// beruehren. Siehe `luecke_quer` fuer die andere Achse.
fn haupt_luecke(b: &Berechnet) -> f32 {
    // GEMESSENE ENTSCHEIDUNG, warum `umgekehrt_ist_spalte` gilt und
    // nicht ein eigener `match` mit vier Armen: `row-reverse` und
    // `column-reverse` drehen die Reihenfolge, NICHT die Achse. Die
    // Hauptachse von `row-reverse` ist weiter die Waagerechte, also
    // bleibt es beim `column-gap`. `richtung` unten macht aus beiden
    // Umkehrungen je eine `Reihe`/`Spalte` — dieselbe Entscheidung,
    // und sie steht dort schon mit Begruendung (K7c).
    if umgekehrt_ist_spalte(b.flex_richtung) {
        b.luecke_zeile
    } else {
        b.luecke_spalte
    }
}

/// `column` und `column-reverse` sind die Spaltenrichtung, der Rest
/// die Zeilenrichtung (CSS Flexbox 1 §5.1).
fn umgekehrt_ist_spalte(r: crate::typen::FlexRichtung) -> bool {
    matches!(
        r,
        crate::typen::FlexRichtung::Spalte | crate::typen::FlexRichtung::SpalteUmgekehrt
    )
}

/// Der Lueckenwert auf der QUERACHSE (L3, 02.10.2026 04:5x) — der
/// andere der beiden, und ohne Umbruch gegenstandslos.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Standardwert 0 ist und nicht
/// `luecke_px`: Beide Felder fangen bei 0 an (`typen.rs`, Anfangswerte),
/// und ein Kasten ohne `row-gap` hat auch keinen Zeilenabstand. Ein
/// Wert aus dem anderen Achsen-Feld waere eine erfundene Luecke.
fn quer_luecke(b: &Berechnet) -> f32 {
    if umgekehrt_ist_spalte(b.flex_richtung) {
        b.luecke_spalte
    } else {
        b.luecke_zeile
    }
}

fn richtung(r: crate::typen::FlexRichtung) -> narsil_layout::plan::FlexRichtung {
    match r {
        crate::typen::FlexRichtung::Reihe => narsil_layout::plan::FlexRichtung::Reihe,
        crate::typen::FlexRichtung::Spalte => narsil_layout::plan::FlexRichtung::Spalte,
        // K7c: die umgekehrten Richtungen (CSS Flexbox 1 §5.1).
        //
        // `narsil_layout` kennt kein `reverse`. Die Umkehrung ist eine
        // EIGENE Frage des Legs (die Hauptachse von hinten zaehlen),
        // und sie wird hier NICHT geraten: sie kommt als `Reihe`/`Spalte`
        // an, und der Leg entscheidet nichts darueber.
        //
        // Das ist eine dokumentierte Naeherung und ein offener Punkt
        // (WUNSCH_CSS.txt W11) — 18 Deklarationen auf den echten
        // Stylesheets sind davon betroffen.
        crate::typen::FlexRichtung::ReiheUmgekehrt => narsil_layout::plan::FlexRichtung::Reihe,
        crate::typen::FlexRichtung::SpalteUmgekehrt => narsil_layout::plan::FlexRichtung::Spalte,
    }
}

/// `justify-content`.
fn buendig(b: FlexBuendig) -> Buendig {
    match b {
        FlexBuendig::Start => Buendig::Start,
        FlexBuendig::Mitte => Buendig::Mitte,
        FlexBuendig::Ende => Buendig::Ende,
        FlexBuendig::Zwischen => Buendig::Zwischen,
        FlexBuendig::Ringsum => Buendig::Ringsum,
        FlexBuendig::Gleichmaessig => Buendig::Gleichmaessig,
        // K7c: `stretch`, `left`, `right` (CSS Box Alignment 3 §8).
        // `narsil_layout` kennt nur die sechs klassischen Werte —
        // `stretch` ist wie `start` (§8.3), `left`/`right` wie
        // `start`/`end` ohne `direction: rtl` (§8.2). Beides ist in
        // WUNSCH_CSS.txt W11 als offene Frage vermerkt.
        FlexBuendig::Strecke | FlexBuendig::Links => Buendig::Start,
        FlexBuendig::Rechts => Buendig::Ende,
    }
}

/// `align-items`.
fn ausrichtung(a: FlexAusrichtung) -> Ausrichtung {
    match a {
        FlexAusrichtung::Start => Ausrichtung::Start,
        FlexAusrichtung::Mitte => Ausrichtung::Mitte,
        FlexAusrichtung::Ende => Ausrichtung::Ende,
        FlexAusrichtung::Strecke => Ausrichtung::Strecke,
        // K7c: `baseline` (CSS Box Alignment 3 §6.3) — die Grundlinie
        // liegt in der Schrift, nicht im Kasten (§6.3 verweist auf
        // CSS Inline 3 §4.1). Fuer die GROSSRICHTUNG ist `baseline`
        // wie `start`; die Feinrichtung braucht Grundlinien, die
        // `narsil_layout` nicht hatte. WUNSCH_CSS.txt W11.
        //
        // L4 (03.10.2026 13:6x): der Plan KENNT die Grundlinie jetzt
        // (`Ausrichtung::Grundlinie`), also geht der Wert durch statt
        // zu `Start` zu werden.  GEMESSENER BEWEIS: der Test
        // `l4_baseline_bleibt_baseline_im_plan` sah vorher links
        // `Start`.  GEMESSENE ENTSCHEIDUNG, warum das keine
        // Verhaltensaenderung ist: `quermass_zeile` behandelt
        // `Grundlinie` wie `Start` ((0, wunsch)) — die Zahl bleibt, der
        // Wert wird nur unterscheidbar.
        FlexAusrichtung::Grundlinie => Ausrichtung::Grundlinie,
    }
}

/// `position`.
fn position(p: Position) -> PositionArt {
    match p {
        Position::Statisch => PositionArt::Statisch,
        Position::Relativ => PositionArt::Relativ,
        Position::Absolut => PositionArt::Absolut,
        Position::Fest => PositionArt::Fest,
        // `sticky` ist wie `relative` (dokumentiert im Auftrag W6 C13).
        Position::Klebend => PositionArt::Klebend,
    }
}

/// `float`.
fn schweben(f: Float) -> FloatArt {
    match f {
        Float::Keins => FloatArt::Keins,
        Float::Links => FloatArt::Links,
        Float::Rechts => FloatArt::Rechts,
    }
}

/// `clear`.
fn raeumung(r: Raeumung) -> RaeumungArt {
    match r {
        Raeumung::Keins => RaeumungArt::Keins,
        Raeumung::Links => RaeumungArt::Links,
        Raeumung::Rechts => RaeumungArt::Rechts,
        Raeumung::Beide => RaeumungArt::Beide,
    }
}

/// `overflow`.
fn ueberlauf(u: Ueberlauf) -> UeberlaufArt {
    match u {
        Ueberlauf::Sichtbar => UeberlaufArt::Sichtbar,
        Ueberlauf::Versteckt => UeberlaufArt::Versteckt,
        Ueberlauf::Scroll => UeberlaufArt::Rollen,
        Ueberlauf::Auto => UeberlaufArt::Automatisch,
    }
}

/// Eine optionale px-Zahl aus der Kaskade: nicht-endlich wird `None`
/// (= auto), endlich wird an [`MAX_WERT_PX`] gekappt.
fn opt(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite())
        .map(|x| x.clamp(-MAX_WERT_PX, MAX_WERT_PX))
}

/// Eine Pflichtzahl aus der Kaskade: nicht-endlich wird 0.
fn pflicht(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, MAX_WERT_PX)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b() -> Berechnet {
        Berechnet::standard()
    }

    #[test]
    fn standard_ist_ein_block() {
        let s = layout_stil(&b());
        assert_eq!(s.anzeige, narsil_layout::plan::Anzeige::Block);
        assert_eq!(s.position, PositionArt::Statisch);
        assert_eq!(s.schweben, FloatArt::Keins);
        assert_eq!(s.raeumen, RaeumungArt::Keins);
        assert_eq!(s.ueberlauf, UeberlaufArt::Sichtbar);
        assert!(s.spalten.is_empty());
        assert_eq!(s.luecke, 0.0);
        assert!(s.ist_im_fluss());
        assert!(s.wird_gezeichnet());
    }

    #[test]
    fn display_none_wird_keins() {
        let mut x = b();
        x.anzeige = Anzeige::Keins;
        let s = layout_stil(&x);
        assert_eq!(s.anzeige, narsil_layout::plan::Anzeige::Keins);
        assert!(!s.wird_gezeichnet());
    }

    #[test]
    fn flex_und_grid_kommen_an() {
        let mut x = b();
        x.anzeige = Anzeige::Flex;
        x.flex_richtung = crate::typen::FlexRichtung::Spalte;
        x.luecke_px = 12.0;
        // L3 (02.10.2026 05:4x): ALLE DREI. `luecke_px` allein genuegt
        // nicht mehr — der Plan liest seit dieser Aenderung die beiden
        // Achsenfelder, weil `gap` sie schreibt und `row-gap`/
        // `column-gap` je eines davon. Ein Aufbau, der nur `luecke_px`
        // setzt, bekommt deshalb 0.
        //
        // GEMESSENER BEFUND, woher das kam: genau so stand es hier, und
        // `flex_und_grid_kommen_an` wurde rot mit `left: 0.0`. Das ist
        // die richtige Reihenfolge: erst der Plan liest die Achsen,
        // dann merkt der Test, dass seine Aufbauten ein Feld zu wenig
        // setzen. Ein Test, der `luecke_px` weiter allein setzen darf,
        // wuerde genau den Fehler verdecken, den die Kuerze macht.
        x.luecke_spalte = 12.0;
        x.luecke_zeile = 12.0;
        x.flex_buendig = FlexBuendig::Zwischen;
        x.flex_ausrichtung = FlexAusrichtung::Mitte;
        let s = layout_stil(&x);
        assert!(s.ist_flex());
        assert_eq!(s.flex_richtung, narsil_layout::plan::FlexRichtung::Spalte);
        assert_eq!(s.luecke, 12.0);
        assert_eq!(s.flex_buendig, Buendig::Zwischen);
        assert_eq!(s.flex_ausrichtung, Ausrichtung::Mitte);

        let mut y = b();
        y.anzeige = Anzeige::Raster;
        y.raster_spalten = vec![
            crate::typen::SpaltenMass::Fest(100.0),
            crate::typen::SpaltenMass::Anteil(1.0),
            crate::typen::SpaltenMass::Anteil(2.0),
        ];
        let s = layout_stil(&y);
        assert!(s.ist_raster());
        assert_eq!(s.spalten.len(), 3);
        assert_eq!(s.spalten[0], Spalte::Fest(100.0));
        assert_eq!(s.spalten[2], Spalte::Anteil(2.0));
    }

    #[test]
    fn position_und_seiten_kommen_an() {
        let mut x = b();
        x.position = Position::Absolut;
        x.links = Some(30.0);
        x.oben = Some(10.0);
        x.z_index = Some(5);
        let s = layout_stil(&x);
        assert_eq!(s.position, PositionArt::Absolut);
        assert_eq!(s.links, Some(30.0));
        assert_eq!(s.oben, Some(10.0));
        assert_eq!(s.z_index, Some(5));
        assert!(!s.ist_im_fluss(), "absolut steht nicht im Fluss");
    }

    #[test]
    fn float_und_clear_kommen_an() {
        let mut x = b();
        x.schweben = Float::Links;
        x.raeumung = Raeumung::Beide;
        x.ueberlauf = Ueberlauf::Versteckt;
        let s = layout_stil(&x);
        assert_eq!(s.schweben, FloatArt::Links);
        assert_eq!(s.raeumen, RaeumungArt::Beide);
        assert_eq!(s.ueberlauf, UeberlaufArt::Versteckt);
        assert!(s.ueberlauf.beschneidet());
    }

    #[test]
    fn alle_ueberlauf_werte() {
        for (von, nach) in [
            (Ueberlauf::Sichtbar, UeberlaufArt::Sichtbar),
            (Ueberlauf::Versteckt, UeberlaufArt::Versteckt),
            (Ueberlauf::Scroll, UeberlaufArt::Rollen),
            (Ueberlauf::Auto, UeberlaufArt::Automatisch),
        ] {
            let mut x = b();
            x.ueberlauf = von;
            assert_eq!(layout_stil(&x).ueberlauf, nach, "{von:?}");
        }
    }

    #[test]
    fn alle_float_werte() {
        for (von, nach) in [
            (Float::Keins, FloatArt::Keins),
            (Float::Links, FloatArt::Links),
            (Float::Rechts, FloatArt::Rechts),
        ] {
            let mut x = b();
            x.schweben = von;
            assert_eq!(layout_stil(&x).schweben, nach, "{von:?}");
        }
    }

    #[test]
    fn alle_richtungen_kommen_an() {
        for (von, nach) in [
            (
                crate::typen::FlexRichtung::Reihe,
                narsil_layout::plan::FlexRichtung::Reihe,
            ),
            (
                crate::typen::FlexRichtung::Spalte,
                narsil_layout::plan::FlexRichtung::Spalte,
            ),
        ] {
            let mut x = b();
            x.flex_richtung = von;
            assert_eq!(layout_stil(&x).flex_richtung, nach, "{von:?}");
            assert!(!layout_stil(&x).flex_umgekehrt, "{von:?}");
        }
        // Umgekehrte Richtungen: gleiche Achse, Spiegelung im Layout.
        for (von, nach) in [
            (
                crate::typen::FlexRichtung::ReiheUmgekehrt,
                narsil_layout::plan::FlexRichtung::Reihe,
            ),
            (
                crate::typen::FlexRichtung::SpalteUmgekehrt,
                narsil_layout::plan::FlexRichtung::Spalte,
            ),
        ] {
            let mut x = b();
            x.flex_richtung = von;
            assert_eq!(layout_stil(&x).flex_richtung, nach, "{von:?}");
            assert!(layout_stil(&x).flex_umgekehrt, "{von:?}");
        }
    }

    #[test]
    fn nan_und_inf_werden_0_oder_auto() {
        // M8: nicht-endliche Werte duerfen keine Position vergiften.
        let mut x = b();
        x.links = Some(f32::NAN);
        x.oben = Some(f32::INFINITY);
        x.luecke_px = f32::NAN;
        // L3 (05:4x): auch hier beide Achsen — sonst prueft der Fall
        // nur noch `luecke_px` und nicht mehr den Weg, den der Plan
        // nimmt. `assert_eq!(s.luecke, 0.0)` waere sonst vakuum gruen.
        x.luecke_spalte = f32::NAN;
        x.luecke_zeile = f32::NAN;
        x.flex_wachstum = f32::INFINITY;
        x.raster_spalten = vec![crate::typen::SpaltenMass::Fest(f32::NAN)];
        let s = layout_stil(&x);
        assert_eq!(s.links, None, "NaN -> auto");
        assert_eq!(s.oben, None, "inf -> auto");
        assert_eq!(s.luecke, 0.0);
        assert_eq!(s.flex_wachstum, 0.0);
        assert_eq!(s.spalten[0], Spalte::Fest(0.0));
        // Und nichts davon ist NaN.
        assert!(!s.luecke.is_nan() && !s.flex_wachstum.is_nan());
    }

    #[test]
    fn riesige_werte_werden_gekappt() {
        // M4: `left: 1e30px` darf die Box nicht aus dem Bild schieben.
        let mut x = b();
        x.links = Some(1.0e30);
        x.oben = Some(-1.0e30);
        x.luecke_px = 1.0e30;
        x.luecke_spalte = 1.0e30;
        x.luecke_zeile = 1.0e30;
        let s = layout_stil(&x);
        assert_eq!(s.links, Some(MAX_WERT_PX), "links gekappt");
        assert_eq!(s.oben, Some(-MAX_WERT_PX), "oben nach unten gekappt");
        assert_eq!(s.luecke, MAX_WERT_PX, "Luecke gekappt");
        assert!(s.links.unwrap().is_finite() && s.oben.unwrap().is_finite());
    }
}
