//! Release 10.10.2026: der BERECHNETE Wert einer Eigenschaft als CSS-Text —
//! das, was `getComputedStyle(el).getPropertyValue(name)` liefert (CSSOM §6.7
//! „resolved value", soweit die Kaskade ihn kennt).
//!
//! GEMESSEN vorher (Web-Platform-Tests): `getComputedStyle` war ein Stummel
//! im Vorspann (Inline-Stil plus feste Vorgaben); 631 Testdateien scheiterten
//! an `expected "…" but got ""`.
//!
//! Mehrdeutige Anfangswerte folgen Chrome: `justify-content`/`align-items`
//! ohne Angabe sind `normal`, `align-self` ist `auto`, `text-align` ist
//! `start`. Eigenschaften, die die Kaskade nur als Rohtext kennt (der
//! generische Eimer, z. B. `cursor`), kommen roh zurueck. `None` heisst „kenne
//! ich nicht" — der Aufrufer faellt dann auf seinen eigenen Weg zurueck.

use crate::typen::{
    Anzeige, Berechnet, BoxArt, Farbe, FlexAusrichtung, FlexBuendig, FlexRichtung, Float, Position, Raeumung,
    Rahmenstil, SchreibRichtung, Schriftstil, Textausrichtung, Ueberlauf, Weissraum, ZeilenhoeheArt,
};

/// Eine Zahl wie CSSOM sie schreibt: ohne ueberfluessige Nachkommastellen.
pub fn zahl(f: f32) -> String {
    if !f.is_finite() {
        return String::from("0");
    }
    if (f - f.round()).abs() < 0.00005 {
        return format!("{}", f.round() as i64);
    }
    let mut s = format!("{f:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

fn px(f: f32) -> String {
    format!("{}px", zahl(f))
}

/// `rgb(r, g, b)` bzw. `rgba(r, g, b, a)` (CSS Color 4 §15.2).
pub fn farbe(f: Farbe) -> String {
    if f.a == 255 {
        return format!("rgb({}, {}, {})", f.r, f.g, f.b);
    }
    let a = f.a as f32 / 255.0;
    // Ein in CSS geschriebenes Alpha (0.5) kommt als 128/255 an; auf zwei
    // Stellen zurueckgerundet, wenn es so nahe liegt.
    let a2 = (a * 100.0).round() / 100.0;
    let a = if (a - a2).abs() < 0.003 { a2 } else { a };
    format!("rgba({}, {}, {}, {})", f.r, f.g, f.b, zahl(a))
}

fn laenge_oder(px_wert: Option<f32>, prozent: Option<f32>, sonst: &str) -> String {
    match (prozent, px_wert) {
        (Some(p), _) => format!("{}%", zahl(p)),
        (None, Some(v)) => px(v),
        (None, None) => String::from(sonst),
    }
}

/// Release 11.10.2026: CSS Overflow 3 §3.1 — `visible` wird zu `auto`,
/// wenn die andere Achse weder `visible` noch `clip` ist. GEMESSEN vorher:
/// `overflow-y: scroll` lieferte `overflowX` = "visible" (Chrome: "auto").
fn ueberlauf_achsen(b: &Berechnet) -> (Ueberlauf, Ueberlauf) {
    let x = b.overflow_x.unwrap_or(b.ueberlauf);
    let y = b.overflow_y.unwrap_or(b.ueberlauf);
    match (x, y) {
        (Ueberlauf::Sichtbar, y) if y != Ueberlauf::Sichtbar => (Ueberlauf::Auto, y),
        (x, Ueberlauf::Sichtbar) if x != Ueberlauf::Sichtbar => (x, Ueberlauf::Auto),
        p => p,
    }
}

fn ueberlauf(u: Ueberlauf) -> &'static str {
    match u {
        Ueberlauf::Sichtbar => "visible",
        Ueberlauf::Versteckt => "hidden",
        Ueberlauf::Scroll => "scroll",
        Ueberlauf::Auto => "auto",
    }
}

fn rahmenstil(s: Rahmenstil) -> &'static str {
    match s {
        Rahmenstil::Kein => "none",
        Rahmenstil::Durchgehend => "solid",
        Rahmenstil::Gemustert | Rahmenstil::Gestrichelt => "dashed",
        Rahmenstil::Gepunktet => "dotted",
        Rahmenstil::Doppelt => "double",
        Rahmenstil::Furche => "groove",
        Rahmenstil::Wulst => "ridge",
        Rahmenstil::Innen => "inset",
        Rahmenstil::Aussen => "outset",
        Rahmenstil::Versteckt => "hidden",
    }
}

fn buendig(f: FlexBuendig) -> &'static str {
    match f {
        FlexBuendig::Start => "normal",
        FlexBuendig::Mitte => "center",
        FlexBuendig::Ende => "flex-end",
        FlexBuendig::Zwischen => "space-between",
        FlexBuendig::Ringsum => "space-around",
        FlexBuendig::Gleichmaessig => "space-evenly",
        FlexBuendig::Strecke => "stretch",
        FlexBuendig::Links => "left",
        FlexBuendig::Rechts => "right",
    }
}

fn ausrichtung(f: FlexAusrichtung, anfang: &'static str) -> &'static str {
    match f {
        FlexAusrichtung::Start => "flex-start",
        FlexAusrichtung::Mitte => "center",
        FlexAusrichtung::Ende => "flex-end",
        FlexAusrichtung::Strecke => anfang,
        FlexAusrichtung::Grundlinie => "baseline",
    }
}

/// Der berechnete Wert von `name` (Kleinschreibung, mit Bindestrich).
pub fn berechneter_wert(b: &Berechnet, name: &str) -> Option<String> {
    let s = |t: &str| Some(String::from(t));
    let rand_breite = |w: f32, st: Rahmenstil| {
        if matches!(st, Rahmenstil::Kein | Rahmenstil::Versteckt) { px(0.0) } else { px(w) }
    };
    Some(match name {
        "display" => {
            if b.nur_inhalt {
                return s("contents");
            }
            String::from(match b.anzeige {
                Anzeige::Block => "block",
                Anzeige::Inline => "inline",
                Anzeige::InlineBlock => "inline-block",
                Anzeige::ListenEintrag => "list-item",
                Anzeige::Flex => "flex",
                Anzeige::InlineFlex => "inline-flex",
                Anzeige::Tabelle => "table",
                Anzeige::TabelleZeile => "table-row",
                Anzeige::TabelleZelle => "table-cell",
                Anzeige::TabelleRand => "table-caption",
                Anzeige::TabelleSpalte => "table-column",
                Anzeige::Raster if b.raster_lanes && b.inline_aussen => "inline-grid-lanes",
                Anzeige::Raster if b.raster_lanes => "grid-lanes",
                Anzeige::Raster => "grid",
                Anzeige::Keins => "none",
            })
        }
        "position" => String::from(match b.position {
            Position::Statisch => "static",
            Position::Relativ => "relative",
            Position::Absolut => "absolute",
            Position::Fest => "fixed",
            Position::Klebend => "sticky",
        }),
        "float" => String::from(match b.schweben {
            Float::Keins => "none",
            Float::Links => "left",
            Float::Rechts => "right",
        }),
        "clear" => String::from(match b.raeumung {
            Raeumung::Keins => "none",
            Raeumung::Links => "left",
            Raeumung::Rechts => "right",
            Raeumung::Beide => "both",
        }),
        "visibility" => String::from(if b.sichtbar { "visible" } else { "hidden" }),
        "overflow-x" => String::from(ueberlauf(ueberlauf_achsen(b).0)),
        "overflow-y" => String::from(ueberlauf(ueberlauf_achsen(b).1)),
        "overflow" => {
            let (x, y) = ueberlauf_achsen(b);
            let (x, y) = (ueberlauf(x), ueberlauf(y));
            if x == y { String::from(x) } else { format!("{x} {y}") }
        }
        "box-sizing" => String::from(match b.box_art {
            BoxArt::Inhalt => "content-box",
            BoxArt::Rand => "border-box",
        }),
        "direction" => String::from(match b.richtung {
            SchreibRichtung::Ltr => "ltr",
            SchreibRichtung::Rtl => "rtl",
        }),
        "white-space" => String::from(match b.weissraum {
            Weissraum::Normal => "normal",
            Weissraum::Pre => "pre",
            Weissraum::KeinUmbruch => "nowrap",
            Weissraum::PreUmbruch => "pre-wrap",
            Weissraum::PreZeile => "pre-line",
            Weissraum::UmbruchLeer => "break-spaces",
        }),
        // Release 15.10.2026 (Agent CSS2): a logical value stays logical
        // (`start`/`end`, also `match-parent` on the root, WPT css-text
        // `text-align-match-parent-002`).
        "text-align" if b.ausrichtung_logisch.is_some() => {
            String::from(if b.ausrichtung_logisch == Some(true) { "start" } else { "end" })
        }
        "text-align" => String::from(match b.textausrichtung {
            Textausrichtung::Links => "start",
            Textausrichtung::Rechts => "right",
            Textausrichtung::Zentriert => "center",
            Textausrichtung::Blocksatz => "justify",
        }),
        "font-style" => String::from(match b.schriftstil {
            Schriftstil::Normal => "normal",
            Schriftstil::Kursiv => "italic",
        }),
        "font-weight" => b.schriftgewicht.to_string(),
        "font-size" => px(b.schriftgroesse_px),
        "font-family" => b.schriftfamilie.clone(),
        "line-height" => match b.zeilenhoehe_art {
            ZeilenhoeheArt::Normal => String::from("normal"),
            _ => px(b.zeilenhoehe_px),
        },
        "color" => farbe(b.farbe),
        "background-color" => farbe(b.hintergrund),
        "opacity" => zahl(b.deckkraft),
        "z-index" => b.z_index.map_or_else(|| String::from("auto"), |z| z.to_string()),
        "flex-direction" => String::from(match b.flex_richtung {
            FlexRichtung::Reihe => "row",
            FlexRichtung::Spalte => "column",
            FlexRichtung::ReiheUmgekehrt => "row-reverse",
            FlexRichtung::SpalteUmgekehrt => "column-reverse",
        }),
        "flex-wrap" => String::from(if b.flex_umbruch { "wrap" } else { "nowrap" }),
        "flex-grow" => zahl(b.flex_wachstum),
        "flex-shrink" => zahl(b.flex_schrumpf),
        "flex-basis" => b.flex_basis.map_or_else(|| String::from("auto"), px),
        "justify-content" => String::from(buendig(b.flex_buendig)),
        "align-items" => String::from(ausrichtung(b.flex_ausrichtung, "normal")),
        "align-self" => b
            .ausrichtung_selbst
            .map_or_else(|| String::from("auto"), |a| String::from(ausrichtung(a, "stretch"))),
        "order" => b.ordnung.to_string(),
        "row-gap" => if b.luecke_zeile > 0.0 { px(b.luecke_zeile) } else { String::from("normal") },
        "column-gap" => if b.luecke_spalte > 0.0 { px(b.luecke_spalte) } else { String::from("normal") },
        "margin-top" => px(b.marge_oben),
        "margin-right" => px(b.marge_rechts),
        "margin-bottom" => px(b.marge_unten),
        "margin-left" => px(b.marge_links),
        "padding-top" => px(b.fuellung_oben),
        "padding-right" => px(b.fuellung_rechts),
        "padding-bottom" => px(b.fuellung_unten),
        "padding-left" => px(b.fuellung_links),
        "border-top-width" => rand_breite(b.rahmen_oben_breite, b.rahmen_oben_stil),
        "border-right-width" => rand_breite(b.rahmen_rechts_breite, b.rahmen_rechts_stil),
        "border-bottom-width" => rand_breite(b.rahmen_unten_breite, b.rahmen_unten_stil),
        "border-left-width" => rand_breite(b.rahmen_links_breite, b.rahmen_links_stil),
        "border-top-style" => String::from(rahmenstil(b.rahmen_oben_stil)),
        "border-right-style" => String::from(rahmenstil(b.rahmen_rechts_stil)),
        "border-bottom-style" => String::from(rahmenstil(b.rahmen_unten_stil)),
        "border-left-style" => String::from(rahmenstil(b.rahmen_links_stil)),
        "border-top-color" => farbe(b.rahmen_oben_farbe),
        "border-right-color" => farbe(b.rahmen_rechts_farbe),
        "border-bottom-color" => farbe(b.rahmen_unten_farbe),
        "border-left-color" => farbe(b.rahmen_links_farbe),
        "border-top-left-radius" => px(b.radius[0]),
        "border-top-right-radius" => px(b.radius[1]),
        "border-bottom-right-radius" => px(b.radius[2]),
        "border-bottom-left-radius" => px(b.radius[3]),
        "top" => laenge_oder(b.oben, b.oben_prozent, "auto"),
        "right" => laenge_oder(b.rechts, b.rechts_prozent, "auto"),
        "bottom" => laenge_oder(b.unten, b.unten_prozent, "auto"),
        "left" => laenge_oder(b.links, b.links_prozent, "auto"),
        "width" => laenge_oder(b.breite, b.breite_prozent, "auto"),
        "height" => laenge_oder(b.hoehe, b.hoehe_prozent, "auto"),
        "min-width" => laenge_oder(b.min_breite, b.min_breite_prozent, "auto"),
        "min-height" => laenge_oder(b.min_hoehe, b.min_hoehe_prozent, "auto"),
        "max-width" => laenge_oder(b.max_breite, b.max_breite_prozent, "none"),
        "max-height" => laenge_oder(b.max_hoehe, b.max_hoehe_prozent, "none"),
        "letter-spacing" => if b.buchstaben_abstand == 0.0 { String::from("normal") } else { px(b.buchstaben_abstand) },
        "word-spacing" => px(b.wort_abstand),
        // Release 12.10.2026 (Agent CSS2): Prozent bleibt Prozent (CSS Text 3
        // §8.1: der berechnete Wert ist die Prozentangabe), `tab-size` als
        // Laenge mit Einheit.
        "text-indent" => match b.text_einzug_prozent {
            Some(p) if b.text_einzug == 0.0 => format!("{}%", zahl(p)),
            _ => px(b.text_einzug),
        },
        "tab-size" => if b.tabulator_px { px(b.tabulator_groesse) } else { zahl(b.tabulator_groesse) },
        "vertical-align" => if b.vertikal.is_empty() { String::from("baseline") } else { b.vertikal.clone() },
        "outline-width" => px(b.outline_breite),
        "outline-color" => farbe(b.outline_farbe),
        "text-transform" => if b.text_transform.is_empty() { String::from("none") } else { b.text_transform.clone() },
        "text-overflow" => if b.text_ueberlauf.is_empty() { String::from("clip") } else { b.text_ueberlauf.clone() },
        "overflow-wrap" | "word-wrap" => if b.wort_umbruch.is_empty() { String::from("normal") } else { b.wort_umbruch.clone() },
        "word-break" => if b.wort_trennung.is_empty() { String::from("normal") } else { b.wort_trennung.clone() },
        "object-fit" => if b.objekt_passung.is_empty() { String::from("fill") } else { b.objekt_passung.clone() },
        "aspect-ratio" => if b.seitenverhaeltnis > 0.0 && b.seitenverhaeltnis_auto {
            // Release 11.10.2026: `auto && <ratio>`.
            format!("auto {} / 1", zahl(b.seitenverhaeltnis))
        } else if b.seitenverhaeltnis > 0.0 {
            format!("{} / 1", zahl(b.seitenverhaeltnis))
        } else {
            String::from("auto")
        },
        _ => {
            let name = name.trim_start_matches("-webkit-");
            return b.schluessel.iter().rev().find(|(n, _)| n == name).map(|(_, w)| w.trim().to_string());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zahlen_und_farben() {
        assert_eq!(zahl(16.0), "16");
        assert_eq!(zahl(13.333333), "13.3333");
        assert_eq!(zahl(0.5), "0.5");
        assert_eq!(farbe(Farbe { r: 0, g: 128, b: 0, a: 255 }), "rgb(0, 128, 0)");
        assert_eq!(farbe(Farbe { r: 0, g: 0, b: 0, a: 0 }), "rgba(0, 0, 0, 0)");
        assert_eq!(farbe(Farbe { r: 255, g: 0, b: 0, a: 128 }), "rgba(255, 0, 0, 0.5)");
    }
}
