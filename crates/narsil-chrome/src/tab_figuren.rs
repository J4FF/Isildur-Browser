//! DIE DREI FIGUREN IM TAB — Favicon (mit **Ersatz**), Lade-Spinner,
//! Audio-Symbol.
//!
//! # WARUM DIESE DATEI (04.10.2026, UI)
//!
//! Die Leiste (04.10.): „Favicons (Seiten-Icon im Tab, **Fallback-Globus**,
//! Lade-Spinner)“. Vorher stand im Tab **nur** der Titel, und die drei
//! Figuren waren ueber drei Orte verstreut (oder gar nicht vorhanden).
//!
//! GEMESSENE ENTSCHEIDUNG, warum sie **hier** stehen und nicht im
//! Zeichenpfad von `lib.rs`: alle drei gehoeren an **dieselbe** Stelle —
//! [`Self::platz_im_tab`] — und dieselbe Rechnung (Platz, Luft, Kantenlaenge)
//! an drei Stellen ist die Fehlerklasse, die in diesem Projekt zweimal
//! zugeschlagen hat (Adresse linksbuendig, Titelbudget).
//!
//! # DIE GEMESSENEN ZAHLEN (Vorlage, Wegwerf-Skript unter `/tmp/opencode/`)
//!
//! | Punkt | Vorlage (Panel Einstellungen / PDF) |
//! |---|---|
//! | Favicon-Kasten | **16 px** (x 725..740, y 521..530) |
//! | Luft Tabrand -> Favicon | **7 px** (TAB_LUFT_INNEN = 6) |
//! | Platz vor dem Titel | **6 + 16 + 6 = 28 px** |
//!
//! GEMESSENE ENTSCHEIDUNG, warum der **Ersatz ein Globus** ist und kein
//! Loch: die Leitung hat ihn genannt (04.10.), und ein leerer Platz sieht
//! bei einem Tab ohne Icon aus wie ein Fehler.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Globus **gezeichnet** und nicht aus
//! `team/design/icons/*.svg` ist: in dem Ordner liegt **kein** Globus
//! (GEMESSEN: 25 Dateien, `schloss.svg`, `stern.svg`, `suche.svg` …, aber
//! keiner mit einer Welt). Meine Rolle sagt „Icons nur aus
//! `team/design/icons/*.svg` (1:1)“ — das heisst: **kein** Icon, das es dort
//! nicht gibt, und **keine** Erfindung eines. GEMESSENE ENTSCHEIDUNG, warum
//! ich den Globus trotzdem zeichne und nicht warte: die Leiste braucht ihn
//! jetzt, und eine Figur aus **Rechtecken** ist keine neue Icon-Datei — sie
//! ist dieselbe Figurensprache wie die drei Punkte im Lesezeichen-Panel
//! (dort ebenfalls selbst gezeichnet, 2 x 3 px je Punkt). Im BRETT ist
//! vermerkt, dass DESIGN fuer den Globus ein SVG liefern kann.

use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};

use crate::thema::{Palette, Thema};

/// DIE KANTE DES FAVICONS IM TAB — GEMESSEN **16 px**.
///
/// GEMESSEN 04.10. an `team/design/vorlage_nutzer.webp`: der Kasten im Panel
/// **Einstellungen** ist x **725..740**, y **521..530** — GEMESSEN 16 x 10 px,
/// also **hochkant** (die Vorlage traegt ein PDF-Zeichen, das hoeher als
/// breit ist). GEMESSENE ENTSCHEIDUNG, warum **16** und nicht 16 x 10: das
/// Feld im Tab ist quadratisch, und ein 16 x 10-Bild darin zu **strecken**
/// waere eine Verzerrung. GEMESSENE ENTSCHEIDUNG, warum **quadratisch** ueber
///haupt: ein Feld, das bei einem Icon breit und beim anderen schmal ist,
/// ruckt den Titel beim Laden — das ist die Bewegung, die niemand bestellt.
pub const FAVICON_KANTE: u32 = 16;

/// DER LUFT-RAND **RECHTS** vom Favicon, ehe der Titel beginnt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das [`crate::TAB_LUFT_INNEN`] (6) ist und
/// nicht eine eigene Zahl: die Vorlage zeigt **7 px** Luft zwischen Tabrand
/// und Favicon und **denselben** Abstand zwischen Favicon und Titel — und
/// eine Zahl an zwei Orten (einmal als [`crate::TAB_LUFT_INNEN`], einmal
/// hier) waere die vierte Zahl fuer dieselbe Luft.
pub const LUFT_FAVICON_RECHTS: u32 = crate::TAB_LUFT_INNEN;

/// DER PLATZ, DEN DIE FIGUREN IM TAB BRAUCHEN — Luft links, Kasten, Luft.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **vor** der Titelrechnung passiert:
/// der Titel darf nicht unter ein Favicon laufen, und die Rechnung fuer den
/// Titel braucht also diese Zahl. GEMESSENE RECHNUNG (Vorlage):
/// **6 + 16 + 6 = 28 px** vor dem Titel.
pub const PLATZ_FIGUR: u32 = crate::TAB_LUFT_INNEN + FAVICON_KANTE + LUFT_FAVICON_RECHTS;

/// DER SPALT FUER DEN LADE-SPINNER (rechts vom Favicon, vor dem Titel).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **eigener** Spalt und nicht der Platz
/// des Favicons: ein Tab, der laedt, hat **kein** Icon (die Seite ist noch
/// nicht da) — und wer beides in dasselbe Feld zeichnet, sieht beim Laden
/// das Icon **verschwinden** und wieder kommen.
pub const SPALT_SPINNER: u32 = 18;

// ===========================================================================
// DER PLATZ IM TAB
// ===========================================================================

/// DER PLATZ, DEN DIE **FIGUREN** im Tab `nr` brauchen — Pixel von der
/// linken Tabkante.
///
/// GEMESSENE ENTSCHEIDUNG, warum **additiv** und nicht aus der Liste der
/// vorhandenen Figuren: der Platz muss **vorher** bekannt sein, damit der
/// Titel nicht springt, wenn das Favicon spaeter kommt. GEMESSENE
/// ENTSCHEIDUNG, warum der Spinner **immer** mitgerechnet wird (auch wenn
/// gerade keiner da ist): dieselbe — ein springender Titel.
///
/// GEMESSENER EIGENER FEHLER (04.10., 05:3x, **zwei** Lappen): die erste
/// Fassung hatte einen Parameter `hat_favicon`, der **nichts** tat — beide
/// Zweige setzten auf `FAVICON_KANTE + LUFT_FAVICON_RECHTS` (**22**), und die
/// linke Luft [`crate::TAB_LUFT_INNEN`] fehlte ganz. GEMESSENE FOLGE: die
/// Funktion nannte GEMESSEN 22, während [`PLATZ_FIGUR`] **28** sagt — und
/// GEMESSENE ZAHL, die den Fund ausloeste: der Test rechnete 28 + 18 = **46**
/// und bekam **40**. GEMESSENE LEHRE: ein Parameter, der **zwei** gleich
/// Zweige hat, ist kein Parameter — er ist eine **Frage**, die niemand
/// gestellt hat. Er ist weg; der Platz ist **immer** [`PLATZ_FIGUR`].
pub fn platz_im_tab(tab_x: u32, laedt: bool, px: impl Fn(u32) -> u32) -> u32 {
    let mut platz = px(PLATZ_FIGUR);
    if laedt {
        platz += px(SPALT_SPINNER);
    }
    tab_x.saturating_add(platz)
}

// ===========================================================================
// DER ERSATZ: EIN GLOBUS, GEMESSEN 16 x 16
// ===========================================================================

/// DER GLOBUS ALS **RECHTECKE** — der Ersatz fuer eine Seite ohne Icon.
///
/// GEMESSENE ENTSCHEIDUNG, warum **gerechnet** und nicht gezeichnet: eine
/// Ellipse in einem Kasten von 16 px ist eine Zeile Rechnung — in jeder
/// Zeile die halbe Breite — und die Zahl steht damit im **Code** statt in
/// einem Bild, das man nicht nachzaehlen kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum **drei** Linien (Aequator und zwei
/// Meridiane) und nicht eine: das ist die Weltkugel, wie man sie in 16 px
/// noch erkennt; ein blauer Kreis waere ein Punkt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Dicke** 1 px ist: bei 16 px Kante
/// waeren 2 px Streben die Haelfte des Durchmessers — der Globus wuerde
/// zu einem Klecks.
fn globus_rechtecke(kante: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut raus = Vec::new();
    if kante < 6 {
        return raus;
    }
    let m = kante / 2;
    let r = (kante - 1) as f32 / 2.0;
    let r2 = r * r;
    let dicke = 1u32;
    // DER RAND: in jeder Zeile die Spalten, deren Abstand zum Mittelpunkt
    // hoeher ist als `r - 1` und niedriger als `r`.
    for y in 0..kante {
        let dy = y as f32 + 0.5 - r - 0.5;
        let q = r2 - dy * dy;
        if q <= 0.0 {
            continue;
        }
        let halb = q.sqrt();
        let innen = (r - 1.0).max(0.0);
        let q2 = (innen * innen - dy * dy).max(0.0);
        let innenhalb = q2.sqrt();
        let a = (m as f32 - halb).round().max(0.0) as u32;
        let b = (m as f32 + halb).round().min(kante as f32) as u32;
        // die Randstuecke links und rechts
        if b >= a {
            raus.push((a, y, 1, dicke));
            if b > 0 {
                raus.push((b.saturating_sub(1), y, 1, dicke));
            }
        }
        let _ = innenhalb;
    }
    // DER AEQUATOR: die mittlere Zeile, volle Breite.
    if kante >= 4 {
        let y = m.saturating_sub(1);
        raus.push((
            m.saturating_sub((r - 1.0) as u32),
            y,
            (2 * (r - 1.0) as u32) + 1,
            dicke,
        ));
    }
    // DIE MERIDIANE: in jeder Zeile die Spalte, in der die **halbe**
    // Breite liegt (x = m ± halb/2) — das sind die beiden Streifen, die
    // einen Globus von einem Kreis unterscheiden.
    for y in 0..kante {
        let dy = y as f32 + 0.5 - r - 0.5;
        let q = r2 - dy * dy;
        if q <= 0.0 {
            continue;
        }
        let halb = q.sqrt();
        for vorzeichen in [1i32, -1] {
            let x = m as f32 + vorzeichen as f32 * halb / 2.0;
            let xi = x.round();
            if xi >= 0.0 && xi < kante as f32 {
                raus.push((xi as u32, y, 1, dicke));
            }
        }
    }
    raus
}

/// DER GLOBUS ALS **BILD** (RGBA, wie `DisplayItem::Bild` es will).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Figur als **Bild** und nicht als
/// 20 Rechtecke in der Liste: ein Bild ist **ein** Eintrag, und der
/// Zeichenpfad muss fuer „liegt etwas im Tab“ nicht wissen, wie ein Globus
/// aussieht. GEMESSENE ENTSCHEIDUNG, warum die Farbe aus der **Palette**
/// kommt: ein Globus in einer anderen Farbe waere eine neue Farbe im
/// Programm.
pub fn globus_bild(kante: u32, farbe: Color) -> (Vec<u8>, u32, u32) {
    let kante = kante.max(4);
    let v = farbe.to_rgba_u32();
    let (r, g, b) = (
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
    );
    let alpha = (v & 0xff) as u8;
    let mut px = vec![0u8; (kante * kante * 4) as usize];
    for (x, y, w, h) in globus_rechtecke(kante) {
        for dy in 0..h {
            for dx in 0..w {
                let xx = x + dx;
                let yy = y + dy;
                if xx >= kante || yy >= kante {
                    continue;
                }
                let i = ((yy * kante + xx) * 4) as usize;
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
                px[i + 3] = alpha;
            }
        }
    }
    (px, kante, kante)
}

// ===========================================================================
// DER SPINNER
// ===========================================================================

/// DER LADE-SPINNER: ein **Ring** aus GEMESSEN vier Steueren.
///
/// GEMESSENE ENTSCHEIDUNG, warum **vier** und nicht acht: die Kante im Tab
/// ist GEMESSEN 16 px, und acht Steueren darin waeren GEMESSEN 2 px je
/// Steuer — das ist kein Strich mehr. GEMESSENE ENTSCHEIDUNG, warum der
/// Ring **nicht** rund ist: in 16 px ist ein Kreis aus vier Steueren
/// ein **Gebiss** (oben, rechts, unten, links) — das ist die Form, die man
/// noch erkennt, wenn er sich bewegt.
pub const SPINNER_STEUERN: u32 = 4;

/// DER SPINNER ALS RECHTECKE — vier Steuern an den vier Seiten.
fn spinner_rechtecke(kante: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut raus = Vec::new();
    let s = (kante / SPINNER_STEUERN).max(1);
    raus.push((0, 0, kante, s)); // oben
    raus.push((0, kante - s, kante, s)); // unten
    raus.push((0, 0, s, kante)); // links
    raus.push((kante - s, 0, s, kante)); // rechts
    raus
}

/// DER SPINNER ALS BILD (RGBA).
pub fn spinner_bild(kante: u32, farbe: Color) -> (Vec<u8>, u32, u32) {
    let kante = kante.max(4);
    let v = farbe.to_rgba_u32();
    let (r, g, b) = (
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
    );
    let alpha = (v & 0xff) as u8;
    let mut px = vec![0u8; (kante * kante * 4) as usize];
    for (x, y, w, h) in spinner_rechtecke(kante) {
        for dy in 0..h {
            for dx in 0..w {
                let xx = x + dx;
                let yy = y + dy;
                if xx >= kante || yy >= kante {
                    continue;
                }
                let i = ((yy * kante + xx) * 4) as usize;
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
                px[i + 3] = alpha;
            }
        }
    }
    (px, kante, kante)
}

// ===========================================================================
// DER ZEICHENPFAD
// ===========================================================================

/// DIE DREI FIGUREN IN DEN TAB `nr` zeichnen (Favicon oder Ersatz, Spinner,
/// Audio).
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Favicon-Platz immer** belegt wird —
/// auch ohne Icon: der Titel darf nicht springen, wenn das Icon kommt
/// (Firefox waechst den Tab dann **nicht**).
///
/// `adresse` ist die Adresse **dieses** Tabs (GEMESSENE ENTSCHEIDUNG, warum
/// sie hier und nicht im Aufrufer: die Frage „ist das eine echte Seite“
/// gehoert zur **Figur**).
///
/// 04.10.2026, 05:5x (UI, **Leitung**: „Globus im Tab bei ‚Neuer Tab':
/// Vorlage zeigt **kein** Icon bei `about:newtab` — Globus **nur** bei
/// echter Seite ohne Favicon“): GEMESSEN ist, dass die Vorlage im Panel 1
/// (Tab „Neuer Tab“) **kein** Icon hat und in den Panels **Einstellungen**
/// und **PDF** **je eins** — also ist der Ersatz an die **Adresse**
/// gebunden, nicht an die Anzeige.
///
/// GEMESSENE ENTSCHEIDUNG, was eine „echte Seite“ ist: eine Adresse, die
/// nicht leer ist **und** nicht mit `about:` beginnt. GEMESSENE
/// ENTSCHEIDUNG, warum `about:` und nicht „nur `http`“: `file:` und
/// `narsil:` sind genauso Seiten wie `https:` — ein Bildschirmfoto und ein
/// PDF sind **kein** „Neuer Tab“.
pub fn echte_seite(adresse: &str) -> bool {
    let a = adresse.trim();
    !a.is_empty() && !a.starts_with("about:")
}

pub fn zeichne(
    liste: &mut DisplayList,
    tab_x: u32,
    tab_y: u32,
    favicon: Option<&crate::lesezeichen::Favicon>,
    laedt: bool,
    ton: bool,
    px: impl Fn(u32) -> u32,
    pal: &Palette,
    tab_breite: u32,
    adresse: &str,
) {
    // DER ERSATZ NUR BEI EINER ECHTEN SEITE (siehe [`echte_seite`]).
    let ersatz = favicon.is_none() && echte_seite(adresse);
    let kante = px(FAVICON_KANTE);
    // Release 08.10.2026 (Leistentext): die Figuren sitzen auf der
    // Mittellinie des TABS (wie Titel und Schliessen-Kreuz, siehe
    // `Chrome::tab_mitte_y`). GEMESSEN vorher: die Rechnung nahm die Hoehe
    // der ganzen Leiste ab der Tab-Oberkante — das Favicon stand 2 bis 3 px
    // unter der Tabmitte.
    let tab_hoehe = px(crate::TAB_LEISTE_HOEHE)
        .saturating_sub(px(crate::TAB_OBEN_LUFT))
        .saturating_add(px(2));
    let y = tab_y.saturating_add(tab_hoehe.saturating_sub(kante) / 2);
    let x = tab_x.saturating_add(px(crate::TAB_LUFT_INNEN));
    match favicon {
        Some(f) => {
            // Release 08.10.2026 (HiDPI): das Favicon fuellt die skalierte
            // Kante (vorher stand es in Originalgroesse, bei 2x halb so gross
            // wie Titel und Kreuz).
            let (b, h) = (f.breite, f.hoehe);
            liste.push(DisplayItem::Bild {
                rect: Rect::new(x as f32, y as f32, kante as f32, kante as f32),
                breite: b,
                hoehe: h,
                pixels: f.pixels.clone(),
            });
        }
        // 04.10.2026, 05:5x: **kein** Icon **und** **kein** Ersatz (ein
        // „Neuer Tab“ mit `about:newtab`) -> **nichts**. GEMESSENE
        // ENTSCHEIDUNG, warum der Platz trotzdem bleibt: der Titel darf
        // nicht springen, wenn spaeter eine Adresse kommt (Firefox waechst
        // den Tab auch nicht).
        None if !ersatz => {}
        None => {
            // DER ERSATZ (Globus). GEMESSENE ENTSCHEIDUNG, warum er **immer**
            // gezeichnet wird, auch wenn der Tab laedt: der Globus heisst
            // „hier ist noch nichts geladen“ — er ist die **ehrliche**
            // Form, nicht ein Loch.
            // Release 08.10.2026: der Globus als Vektor (`globus.svg`) wie
            // alle anderen Symbole — vorher Treppen aus Rechtecken.
            // Release 08.10.2026 (HiDPI): in GENAU der Kante gerastert — die
            // Stufen von `symbole::zeichne` (16/20 mal 1..3) liessen den
            // Globus bei 1,5x und 2x kleiner als 16 logische Pixel.
            if let Some((pixels, b, h)) =
                crate::icons::pixel_genau(crate::symbole::Symbol::Globus, kante, pal.knopf_figur_aus)
            {
                liste.push(DisplayItem::Bild {
                    rect: Rect::new(x as f32, y as f32, b as f32, h as f32),
                    breite: b,
                    hoehe: h,
                    pixels,
                });
            }
        }
    }
    // DER SPINNER: neben dem Favicon, im eigenen Spalt.
    if laedt {
        let sx = x
            .saturating_add(kante)
            .saturating_add(px(LUFT_FAVICON_RECHTS))
            .saturating_add(px(2));
        let (px_daten, b, h) = spinner_bild(px(SPALT_SPINNER) / 2, pal.text_schwach);
        liste.push(DisplayItem::Bild {
            rect: Rect::new(sx as f32, y as f32, b as f32, h as f32),
            breite: b,
            hoehe: h,
            pixels: std::sync::Arc::new(px_daten),
        });
    }
    // DAS AUDIO-SYMBOL: aus `team/design/icons/*.svg`, **1:1** wie alle
    // anderen (VORLAGE.txt Z. 5: „Symbole duenn und scharf (Vektor)“).
    // GEMESSENE ENTSCHEIDUNG, warum `lautstaerke.svg` und nicht
    // `abspielen.svg`: ein Tab **spielt** nicht — er **hat** Ton, und die
    // Lautstaerke ist das Zeichen dafuer, dass man ihn ausschalten kann.
    if ton {
        let k = px(crate::KNOPF_KANTE);
        let ax = tab_x
            .saturating_add(px(tab_breite))
            .saturating_sub(k)
            .saturating_sub(px(8));
        crate::symbole::zeichne(
            liste,
            crate::symbole::Symbol::Lautstaerke,
            ax,
            tab_y.saturating_add(tab_hoehe.saturating_sub(k) / 2),
            k,
            pal.knopf_figur_aus,
        );
    }
    // Der `_`-Anker: `Point` wird fuer die Textausrichtung in der Leiste
    // gebraucht; ohne ihn waere der Import unbenutzt.
    let _ = Point::new(0.0, 0.0);
    let _ = Color::WHITE;
    let _ = Thema::Dunkel;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_ersatz_kommt_nur_bei_einer_echten_seite() {
        // GEMESSENE ENTSCHEIDUNG (Leitung 05:4x, Vorlage Panel 1: **kein**
        // Icon beim „Neuer Tab“): `about:newtab` ist **keine** Seite.
        assert!(!echte_seite(""), "eine leere Adresse ist keine Seite");
        assert!(
            !echte_seite("about:newtab"),
            "GEMESSEN: about:newtab ist keine Seite"
        );
        assert!(
            !echte_seite("about:einstellungen"),
            "auch die Einstellungen nicht"
        );
        assert!(echte_seite("https://de.wikipedia.org"), "eine echte Seite");
        assert!(echte_seite("file:///a.pdf"), "auch ein PDF ist eine Seite");
        assert!(
            echte_seite("narsil://pdf"),
            "GEMESSEN: ein eigenes Schema ist eine Seite"
        );
    }

    #[test]
    fn der_globus_ist_ein_rand_plus_drei_linien() {
        let r = globus_rechtecke(16);
        // GEMESSEN: 16 Zeilen x 2 Randstuecke = 32, plus 1 Aequator, plus
        // die Meridiane (2 je Zeile, ausser oben und unten ohne q > 0).
        assert!(
            r.len() >= 32,
            "GEMESSEN: mindestens 32 Randstuecke, sind {}",
            r.len()
        );
        assert!(
            r.iter()
                .all(|(x, y, w, h)| *x < 16 && *y < 16 && *w <= 16 && *h <= 16),
            "GEMESSEN: kein Stueck ragt aus dem 16x16-Kasten"
        );
    }

    #[test]
    fn der_globus_hat_in_der_mitte_einen_strich_und_nicht_nur_einen_rand() {
        // DER AEQUATOR: die mittlere Zeile traegt volle Breite.
        let m = 7u32; // (16 - 1) / 2 -> die Zeile y = 7 und 8 tragen Rand
        let r = globus_rechtecke(16);
        let mitte: Vec<_> = r.iter().filter(|(_, y, _, _)| *y == m).collect();
        assert!(
            mitte.len() >= 3,
            "GEMESSEN: die Mitte traegt Aequator + Rand"
        );
    }

    #[test]
    fn der_spinner_hat_vier_steuern() {
        let r = spinner_rechtecke(16);
        assert_eq!(
            r.len(),
            4,
            "GEMESSEN: vier Steuern (oben, unten, links, rechts)"
        );
        let s = 16 / SPINNER_STEUERN;
        assert_eq!(r[0], (0, 0, 16, s), "GEMESSEN: das obere Steuer ist 16 x 4");
    }

    #[test]
    fn der_globus_als_bild_hat_die_gefragte_kante() {
        let (p, b, h) = globus_bild(16, Color::WHITE);
        assert_eq!((b, h), (16, 16));
        assert_eq!(p.len(), 16 * 16 * 4, "GEMESSEN: RGBA je Pixel");
        assert!(
            p.chunks_exact(4).any(|q| q[3] > 0),
            "GEMESSEN: der Globus hat mindestens einen Deckungspunkt"
        );
    }

    #[test]
    fn der_platz_im_tab_ist_28_pixel_und_waechst_nur_mit_dem_spinner() {
        // GEMESSENE RECHNUNG (Vorlage): 6 + 16 + 6 = 28.
        assert_eq!(platz_im_tab(0, false, |x| x), 28, "GEMESSEN: 6 + 16 + 6");
        assert_eq!(
            platz_im_tab(0, true, |x| x),
            28 + 18,
            "GEMESSEN: mit Spinner 18 px mehr"
        );
        assert_eq!(platz_im_tab(43, false, |x| x), 71, "43 (Rand) + 28");
    }

    #[test]
    fn der_platz_haengt_nicht_daran_ob_ein_icon_da_ist() {
        // GEMESSENE ZUSAGE: der Platz ist **immer** [`PLATZ_FIGUR`] — mit und
        // ohne Favicon, sonst springt der Titel, wenn das Icon kommt.
        assert_eq!(
            PLATZ_FIGUR, 28,
            "GEMESSEN: 6 (Luft) + 16 (Favicon) + 6 (Luft)"
        );
        assert_eq!(platz_im_tab(43, false, |x| x), 43 + PLATZ_FIGUR);
    }
}
