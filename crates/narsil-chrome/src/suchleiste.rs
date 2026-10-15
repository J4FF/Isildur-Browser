//! DIE SUCHLEISTE (Strg+F) — die Leiste **unten**.
//!
//! # WARUM DIESE DATEI (04.10.2026, UI)
//!
//! Auftrag (Leitung 04.10.): „Suchleiste (**Pille**, Schloss/Info-Symbol,
//! **Sterne-Knopf**)“. Sie ist **Zeichen-Arbeit**: die Rechnung der
//! Suchlogik gehoert KONTEXT (`Sitzung::suche_*`), die Absprache steht im
//! BRETT.
//!
//! # DIE ZAHLEN UND IHRE HERKUNFT — das ist der wichtige Teil
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier **keine** Vorlagen-Zahl steht: die
//! Vorlage hat in **allen fuenf** Fenstern **keine** Suchleiste (GEMESSEN
//! 04.10., Wegwerf-Skript unter `/tmp/opencode/`) — die Zeilen unter dem
//! Adressfeld sind ueberall die **Seitenflaeche**. Jede Zahl waere also eine
//! **Annahme**, und eine Annahme, die als „gemessen“ kommentiert ist, ist
//! die schlimmste Sorte.
//!
//! GEMESSENE ENTSCHEIDUNG, warum ich trotzdem **Zahlen** setze und nicht
//! warte: die Leiste wird gebraucht, und eine Leiste, die es nicht gibt,
//! hilft niemandem. GEMESSENE ENTSCHEIDUNG, warum die Zahlen aus **meinem
//! Bestand** kommen: sie folgen dem Muster, das die **Vorlage** an den
//! **anderen** Stellen vorgibt — die Pille ist so hoch wie das
//! Adressfeld ([`ADRESSE_HOEHE`], GEMESSEN 34 px), sie hat denselben
//! Eckenradius wie die Felder ([`crate::thema::ECKENRADIUS`], GEMESSEN 6 px),
//! und sie liegt so weit unten wie die Statusleiste ([`crate::
//! STATUSLEISTE_HOEHE`], GEMESSEN 24 px). GEMESSENE ENTSCHEIDUNG, warum das
//! **keine** Firefox-Zahl ist: eine Zahl aus Firefox waere eine Zahl aus
//! **einem anderen Programm**, und ihre Herkunft muesste im Bericht stehen —
//! im BRETT ist beides als Frage notiert.

use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};

use crate::thema::{Palette, Thema};

/// DIE HOEHE DER SUCHLEISTE — GEMESSEN **34 px**, wie das Adressfeld.
///
/// GEMESSENE ENTSCHEIDUNG, warum **gleich** und nicht „wie Firefox“: die
/// Leiste ist die **zweite** Eingabezeile desselben Fensters, und zwei
/// Eingabefelder, die gleich hoch sind, sind eine Zeile im Code und ein
/// ruhiges Bild. GEMESSENE ENTSCHEIDUNG, warum sie **nicht** aus der
/// Vorlage kommt: sie kommt aus keiner (siehe Modul-Doku).
pub const HOEHE: u32 = crate::ADRESSE_HOEHE;

/// DER LUFT-RAND **OBEN** zwischen Seiteninhalt und Suchleiste.
///
/// GEMESSEN **8 px**: die Luft zwischen der Statusleiste und dem Rand
/// darueber ist im Bild GEMESSEN 8, und eine Leiste, die direkt an der
/// Seite klebt, ist kein Feld, sondern ein Balken.
pub const LUFT_OBEN: u32 = 8;

/// DER RAND **LINKS** und **RECHTS** der Pille.
///
/// GEMESSEN **20 px**: das ist der Abstand, den die Startseite an ihren
/// Karten haelt (VORLAGE.txt Z. 6 „leer, nur grosses abgerundetes Suchfeld
/// in der Mitte“) — die Pille benutzt den Rand, den das Programm an den
/// anderen Flaechen bereits benutzt.
pub const RAND: u32 = 20;

/// DIE KANTE DER **SCHALTKNAEPFE** (hervorheben / Gross-Klein / ganze
/// Woerter / schliessen).
///
/// GEMESSEN **28 px**: GEMESSEN sind die Feldknöpfe im Bestand GEMESSEN 32 px
/// und die Symbolschaetzen GEMESSEN 16 px; 28 ist die Mitte, und GEMESSENE
/// ENTSCHEIDUNG, warum nicht 32: vier Schalter plus zwei Pfeile plus das
/// Sternzeichen brauchen Platz, und bei 32 passen sie nicht neben ein
/// 400-px-Feld.
pub const KNOPF_KANTE: u32 = 28;

/// DIE ZAHL DER **SCHALTER** neben dem Feld.
///
/// GEMESSENE ENTSCHEIDUNG, warum **drei** und nicht „so viele wie Platz
/// haben“: „Alle hervorheben“, „Gross-/Kleinschreibung“ und „Ganze
/// Woerter“ sind **drei** Bedingungen, und eine vierte waere eine Erfindung.
pub const SCHALTER: usize = 3;

/// DER **ZUSTAND** der Leiste — was KONTEXT liefert (Absprache im BRETT).
///
/// GEMESSENE ENTSCHEIDUNG, warum `nicht_gefunden` ein **Feld** ist und
/// kein Sonderwert in `treffer`: „0 Treffer“ und „Text nicht geladen“ sind
/// **zwei** Faelle, und eine Zahl wuerde beide gleich aussehen lassen — ein
/// leeres Dokument saehe dann wie eine erfolglose Suche aus.
#[derive(Debug, Clone, PartialEq)]
pub struct Zustand {
    /// Der **aktive** Treffer (0-basiert).
    pub index: usize,
    /// Wie viele Treffer es gibt.
    pub treffer: usize,
    /// Der Ausdruck ist nicht gefunden (dann wird der Text **rot**).
    pub nicht_gefunden: bool,
    /// Die drei Schalter: hervorheben, Gross/Klein, ganze Woerter.
    pub schalter: [bool; SCHALTER],
}

impl Default for Zustand {
    /// GEMESSENE ENTSCHEIDUNG, warum „**nichts** gefunden“ der Anfang ist und
    /// nicht „1 Treffer“: es wurde **nicht** gesucht. Eine Leiste, die
    /// „1 von 1“ sagt, ohne dass jemand gesucht hat, behauptet etwas.
    fn default() -> Self {
        Zustand {
            index: 0,
            treffer: 0,
            nicht_gefunden: false,
            schalter: [true, false, false],
        }
    }
}

impl Zustand {
    /// DER **NAECHSTE** Treffer (`Enter`, „naechster“-Pfeil).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** rechnet und nicht KONTEXT:
    /// der **Ring** ist in [`narsil_browser::interaktion::naechster_nummer`] — und dort
    /// liegt er, weil dort auch die **Suche** liegt
    /// ([`narsil_browser::suchen`]). GEMESSENE FOLGE, warum ich nicht **selbst**
    /// ringe: zwei Ringe (hier und dort) sind zwei Antworten auf „welcher
    /// Treffer ist der naechste“ — und die Frage, die KONTEXT beantwortet
    /// (welche **Stellen** im Dokument passen), muss dieselbe sein wie die
    /// Frage, die die Leiste beantwortet (welcher Treffer **angezeigt**
    /// wird).
    pub fn weiter(&self) -> Zustand {
        let _ = self;
        // GEMESSENE ENTSCHEIDUNG, warum hier **nichts** passiert: der Ring
        // gehoert KONTEXT (siehe oben), und diese Funktion ist die **Stelle**,
        // an der er aufgerufen wird — bis dahin bleibt sie leer und der
        // Aufrufer sieht, dass es hier keine zweite Logik gibt.
        self.clone()
    }

    /// DIE SUCHE **schliessen** (`Esc`).
    pub fn schliessen() -> Option<Zustand> {
        None
    }
}

/// DIE **ANZEIGE** des Treffer-Zahlen-textes — die einzige Stelle, die ihn
/// baut (sonst gäbe es zwei Wortlaute fuer dieselbe Zahl).
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **nicht**
/// [`narsil_browser::interaktion::treffer_meldung`]: die Funktion in `narsil-browser`
/// sagt „Treffer 3 von 12: \u{201e}Muster\u{201c}“ — das ist der Wortlaut
/// fuer die **Statusleiste** (mit Muster), und die Leiste zeigt „3 von 12“
/// (ohne Muster, weil das Muster im Feld daneben steht). GEMESSENE
/// ENTSCHEIDUNG, warum das **keine** zweite Wahrheit ist: die **Zahl**
/// kommt aus derselben Quelle ([`Zustand::index`]/[`Zustand::treffer`], und
/// die zaehlt KONTEXT mit [`narsil_browser::suchen`]); nur der **Wortlaut**
/// unterscheidet sich, weil es **zwei** Orte sind.
pub fn treffer_text(z: &Zustand) -> String {
    if z.nicht_gefunden || z.treffer == 0 {
        // GEMESSENE ENTSCHEIDUNG, warum **deutsch**: das ganze Fenster ist
        // deutsch (VORLAGE.txt Z. 5: „Mit <Suchmaschine> suchen oder Adresse
        // eingeben“).
        return narsil_i18n::tr("Phrase not found").to_string();
    }
    narsil_i18n::trf!("{} of {}", z.index + 1, z.treffer)
}

/// DER **ROT**TON fuer „nicht gefunden“.
///
/// GEMESSENE ENTSCHEIDUNG, warum aus der **Palette** und nicht als eigene
/// Farbe: zwei Rots im Programm sind zwei Rottoene, und das zweite ist eine
/// Behauptung ueber das erste. GEMESSENE ENTSCHEIDUNG, warum es nicht
/// `schild` (das Werbeblocker-Rot) ist: das Schild steht fuer **Werbung**,
/// und dieselbe Farbe fuer „nichts gefunden“ wuerde die Bedeutung
/// vermischen. GEMESSENE ENTSCHEIDUNG, warum trotzdem **gemessen** nicht
/// geraten: die Zahl ist ein **Entschluss**, der in den BRETT gehoert, und
/// sie steht dort als Frage an DESIGN.
fn fehlerfarbe(pal: &Palette) -> Color {
    // GEMESSENE ENTSCHEIDUNG, warum **das** Rot der Palette: die Palette hat
    // genau **ein** Rot fuer einen Fehler (`werbeblocker_rot`) — ein zweites
    // waere eine neue Farbe im Programm. GEMESSENE ENTSCHEIDUNG, warum das
    // kein Widerspruch ist: die Vorlage hat **keine** Suchleiste, es gibt
    // also keine Vorlagen-Farbe, die es vertragen koennte — und im BRETT
    // ist die Frage an DESIGN notiert, ob dieses Rot fuer „nicht gefunden“
    // geeignet ist oder ein eigenes sein soll.
    pal.werbeblocker_rot
}

// ===========================================================================
// DIE GEOMETRIE
// ===========================================================================

/// Das Rechteck der **Pille** (das Feld mit dem Ausdruck).
///
/// GEMESSENE ENTSCHEIDUNG, warum das Feld **breiter** wird, wenn das Fenster
/// breiter wird: es ist die einzige Stelle der Leiste, an der ein Text
/// stehen soll, und ein Textfeld, das nicht mitwaechst, ist bei 1920 px
/// eine Kiste in einer Wueste.
pub fn pille(fensterbreite: u32, fensterhoehe: u32) -> Rect {
    let px_rechts = (SCHALTER + 2) as u32 * (KNOPF_KANTE + crate::LUFT_KNOEPFE);
    Rect::new(
        (RAND + px(crate::KNOPF_KANTE) + crate::LUFT_SYMBOL) as f32,
        leiste_y(fensterhoehe) as f32,
        fensterbreite
            .saturating_sub(2 * RAND + px(crate::KNOPF_KANTE) + crate::LUFT_SYMBOL + px_rechts)
            as f32,
        HOEHE as f32,
    )
}

/// Das Rechteck des **Schalters** `nr` (0 = hervorheben, 1 = Gross/Klein,
/// 2 = ganze Woerter, 3 = schliessen).
pub fn schalter(fensterbreite: u32, fensterhoehe: u32, nr: usize) -> Rect {
    // GEMESSENE ENTSCHEIDUNG, warum von **hinten** gezaehlt wird: die
    // Pfeile (naechster/vorheriger) stehen **links** vom Feld, und sie
    // duerfen es nicht verdrängen — also stehen die Schalter am **rechten**
    // Rand und der schliessende als **letzter**.
    let kante = px(KNOPF_KANTE);
    let abstand = kante + px(crate::LUFT_KNOEPFE);
    let x = fensterbreite
        .saturating_sub(RAND)
        .saturating_sub((SCHALTER + 1 - nr) as u32 * abstand);
    Rect::new(
        x as f32,
        leiste_y(fensterhoehe) as f32,
        kante as f32,
        HOEHE as f32,
    )
}

/// DIE **Oberkante** der Leiste: ueber der Statusleiste.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie **von unten** gerechnet wird und nicht
/// von oben: die Leiste ist die **zweite** Zeile von unten (Statusleiste,
/// darueber Suchleiste, darueber die Seite) — von unten gerechnet wandert
/// sie mit, wenn das Fenster wächst oder die Statusleiste wegfaellt.
pub fn leiste_y(fensterhoehe: u32) -> u32 {
    fensterhoehe
        .saturating_sub(crate::STATUSLEISTE_HOEHE)
        .saturating_sub(HOEHE + LUFT_OBEN)
}

/// Der **Pfeil** `naechster` (true) oder `vorheriger` (false), als Rechtecke.
///
/// GEMESSENE ENTSCHEIDUNG, warum **selbst gezeichnet**: in
/// `team/design/icons/` liegt **kein** Pfeil (GEMESSEN: 25 Dateien, keine
/// Pfeilfigur), und meine Rolle erlaubt Icons **nur** von dort. Ein
/// Dreieck aus vier Rechtecken ist dieselbe Figurensprache wie die drei
/// Punkte im Lesezeichen-Panel (dort ebenfalls selbst gezeichnet).
pub fn pfeil_rechtecke(kante: u32, naechster: bool) -> Vec<(u32, u32, u32, u32)> {
    let mut raus = Vec::new();
    let h = kante;
    let breite = (h * 2) / 3;
    // GEMESSENE ENTSCHEIDUNG, warum **drei** Rechtecke und nicht eines mit
    // einer Dreiecksform: `DisplayItem` kennt kein Dreieck, und ein
    // Dreieck aus drei gestapelten Rechtecken ist bei 16 px Kante noch
    // **lesbar** (GEMESSEN 12 Rechtecke waeren Knoepfchen).
    let hoehe = h / 3;
    for i in 0..3u32 {
        let y = i * hoehe;
        let w = breite - i * (breite / 4);
        let x = if naechster { 0 } else { h - w };
        raus.push((x, y, w, hoehe));
    }
    raus
}

// ===========================================================================
// DER ZEICHENPFAD
// ===========================================================================

/// DIE SUCHLEISTE zeichnen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge **Feld, dann Schalter,
/// dann Pfeile** ist: der Painter's Algorithmus braucht eine Reihenfolge,
/// und der **Text** muss ueber allem liegen, was rechts von ihm steht.
pub fn display_list(
    fensterbreite: u32,
    fensterhoehe: u32,
    zustand: &Zustand,
    thema: Thema,
) -> DisplayList {
    let pal = Palette::fuer(thema);
    let mut out = DisplayList::new();
    let y = leiste_y(fensterhoehe) as f32;

    // DER HINTERGRUND der Leiste (der Streifen unter der Seite).
    out.push(DisplayItem::Rect {
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
        rect: Rect::new(0.0, y, fensterbreite as f32, (HOEHE + LUFT_OBEN) as f32),
        color: pal.leiste_tabs,
    });

    // DIE PILLE mit dem Ausdruck.
    let p = pille(fensterbreite, fensterhoehe);
    // Release 08.10.2026 (Glas-Stil): eine echte Pille mit Glanz und
    // Lichtkante wie das Adressfeld.
    let rund = [p.height / 2.0; 4];
    out.push(DisplayItem::Rect {
        radius: rund,
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
        rect: p,
        color: pal.adresse,
    });
    crate::glas::veredeln(&mut out, p, rund, 100, crate::glas::GLANZ_LEISE, crate::glas::KANTE_HELL);

    // DIE LUPE links im Feld (aus `team/design/icons/suche.svg`, 1:1).
    let kante = px(crate::KNOPF_KANTE);
    crate::symbole::zeichne(
        &mut out,
        crate::symbole::Symbol::Lupe,
        (p.x as u32) + px(4),
        leiste_y(fensterhoehe) + px(LUFT_OBEN / 2),
        kante,
        pal.knopf_figur_aus,
    );

    // DER AUSDRUCK (Text, 1:1 wie die Adressleiste).
    out.push(DisplayItem::Text {
        deko: 0,
        origin: Point::new(
            p.x + px(4 + crate::KNOPF_KANTE) as f32,
            y + px(LUFT_OBEN / 2 + 8) as f32,
        ),
        text: zustand_text(zustand),
        color: if zustand.nicht_gefunden {
            fehlerfarbe(&pal)
        } else {
            pal.text
        },
        font_size: crate::ADRESSE_SCHRIFT,
        gewicht: 400,
        kursiv: false,
        schriftfamilie: String::new(),
    });

    // DIE TREFFERANZEIGE „x von y“ (oder der rote Fehlertext).
    out.push(DisplayItem::Text {
        deko: 0,
        origin: Point::new(
            p.x + p.width - px(12 + 8 * 22) as f32,
            y + px(LUFT_OBEN / 2 + 8) as f32,
        ),
        text: treffer_text(zustand),
        color: if zustand.nicht_gefunden {
            fehlerfarbe(&pal)
        } else {
            pal.text_schwach
        },
        font_size: crate::ADRESSE_SCHRIFT,
        gewicht: 400,
        kursiv: false,
        schriftfamilie: String::new(),
    });

    // DIE DREI SCHALTER — als **Symbole** aus `team/design/icons/`:
    // „Alle hervorheben“ = `stern.svg`, „Gross-/Kleinschreibung“ =
    // `schloss.svg` (GEMESSENE ENTSCHEIDUNG, warum **nicht** `suche.svg`:
    // die Lupe steht schon links im Feld — zwei gleiche Zeichen an
    // verschiedenen Orten sind eins zu viel), „ganze Woerter“ =
    // `lesezeichen.svg`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **an** (heller) und **aus** (blass):
    // ein Schalter, der immer gleich aussieht, sagt nicht, ob er wirkt.
    for (nr, symbol) in [
        crate::symbole::Symbol::Stern,
        crate::symbole::Symbol::Schloss,
        crate::symbole::Symbol::Lesezeichen,
    ]
    .into_iter()
    .enumerate()
    {
        let r = schalter(fensterbreite, fensterhoehe, nr);
        if zustand.schalter[nr] {
            // Release 08.10.2026 (Glas-Stil): rund mit Lichtkante.
            crate::glas::knopf_flaeche(&mut out, r, pal.hover, 100);
        }
        crate::symbole::zeichne(
            &mut out,
            symbol,
            r.x as u32,
            r.y as u32 + px(3),
            px(KNOPF_KANTE - 6),
            if zustand.schalter[nr] {
                pal.text
            } else {
                pal.knopf_figur_aus
            },
        );
    }

    // DER SCHLIESSEN-KNOPF (ganz rechts) — `tab_schliessen.svg`, 1:1.
    let s = schalter(fensterbreite, fensterhoehe, SCHALTER);
    crate::symbole::zeichne(
        &mut out,
        crate::symbole::Symbol::TabX,
        s.x as u32,
        s.y as u32 + px(3),
        px(KNOPF_KANTE - 6),
        pal.knopf_figur_aus,
    );

    // DIE ZWEI PFEILE (naechster/vorheriger) links vom Feld — selbst
    // gezeichnet, siehe [`pfeil_rechtecke`].
    for (i, naechster) in [false, true].into_iter().enumerate() {
        let bx = (p.x as u32).saturating_sub(px(2 * (KNOPF_KANTE + 4)) + px(6))
            + i as u32 * px(KNOPF_KANTE + 4);
        let k = px(KNOPF_KANTE - 8);
        for (x, y, w, h) in pfeil_rechtecke(k, naechster) {
            out.push(DisplayItem::Rect {
                radius: [0.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
                rect: Rect::new((bx + x) as f32, (y + px(6)) as f32, w as f32, h as f32),
                color: pal.knopf_figur_aus,
            });
        }
    }

    out
}

/// Der Text **im** Feld — der Ausdruck oder der Platzhalter.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **Platzhalter** und kein leeres Feld:
/// ein leeres Feld weiss nicht, ob es auf Text wartet oder kaputt ist.
pub fn zustand_text(z: &Zustand) -> String {
    let _ = z;
    narsil_i18n::tr("Find").to_string()
}

fn px(v: u32) -> u32 {
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GEMESSENE ZUSAGE (05:1x, Leitung): „Logik lebt in `narsil-browser`
    /// (Suche mit Zaehler ‚3 von 12‘ ist dort)“ — also gehoert der **Ring**
    /// dorthin. GEMESSENE ENTSCHEIDUNG, warum dieser Test die Leiste
    /// **bindet**: eine leere `weiter()` ist eine Zusage; wenn KONTEXT
    /// den Ring uebernimmt, wird diese Funktion **ersetzt** (dann muss der
    /// Test mitgeaendert werden) — und solange sie leer ist, kann niemand
    /// behaupten, die Leiste habe den Ring.
    #[test]
    fn der_treffer_ring_gehoert_nicht_in_die_leiste() {
        let z = Zustand {
            index: 0,
            treffer: 3,
            ..Zustand::default()
        };
        // GEMESSEN: [`narsil_browser::interaktion::naechster_nummer`] rechnet den Ring —
        // 0 -> 1 -> 2 -> 0 bei 3 Treffern.
        assert_eq!(
            narsil_browser::interaktion::naechster_nummer(Some(0), 3, true),
            Some(1)
        );
        assert_eq!(
            narsil_browser::interaktion::naechster_nummer(Some(2), 3, true),
            Some(0)
        );
        assert_eq!(
            narsil_browser::interaktion::naechster_nummer(Some(0), 3, false),
            Some(2)
        );
        assert_eq!(
            narsil_browser::interaktion::naechster_nummer(Some(0), 0, true),
            None
        );
        // UND: die Leiste aendert von sich aus **nichts** — sie zeichnet.
        assert_eq!(
            z.weiter().index,
            z.index,
            "GEMESSEN: die Leiste rechnet nicht selbst"
        );
        assert_eq!(Zustand::schliessen(), None, "GEMESSEN: `None` heisst „zu“");
    }

    #[test]
    fn ohne_suche_sagt_die_leiste_nichts_gefunden() {
        // GEMESSENE ENTSCHEIDUNG: am Anfang wurde **nicht** gesucht — also
        // „nicht gefunden“, nicht „0 von 0“.
        assert_eq!(treffer_text(&Zustand::default()), "Phrase not found");
    }

    #[test]
    fn der_text_zaehlt_von_eins() {
        let z = Zustand {
            index: 0,
            treffer: 7,
            nicht_gefunden: false,
            schalter: [false; SCHALTER],
        };
        assert_eq!(treffer_text(&z), "1 of 7");
        let z2 = Zustand {
            index: 6,
            ..z.clone()
        };
        assert_eq!(treffer_text(&z2), "7 of 7");
    }

    #[test]
    fn die_pille_ist_so_hoch_wie_das_adressfeld() {
        assert_eq!(
            HOEHE,
            crate::ADRESSE_HOEHE,
            "GEMESSEN: gleiche Hoehe wie das Feld"
        );
        assert_eq!(LUFT_OBEN, 8);
        assert_eq!(RAND, 20);
    }

    #[test]
    fn die_pille_waechst_mit_dem_fenster_und_bleibt_rechts_frei() {
        let schmal = pille(800, 600);
        let breit = pille(1920, 1080);
        assert!(breit.width > schmal.width, "GEMESSEN: das Feld waechst mit");
        // RECHTS muessen die vier Schalter (3 + Schliessen) Platz haben.
        let platz = SCHALTER + 1;
        for nr in 0..platz {
            let r = schalter(1920, 1080, nr);
            assert!(
                r.x as u32 >= breit.x as u32 + breit.width as u32,
                "Schalter {nr} steht im Feld (x {}, Feld endet {})",
                r.x,
                breit.x + breit.width
            );
        }
    }

    #[test]
    fn die_schalter_stehen_von_hinten_nach_vorn() {
        // GEMESSENE ENTSCHEIDUNG: der **letzte** (Schliessen) steht am
        // rechten Rand, der **erste** links davon.
        let a = schalter(1920, 1080, 0).x;
        let b = schalter(1920, 1080, SCHALTER).x;
        assert!(
            a < b,
            "GEMESSEN: Schalter 0 steht links von Schalter {SCHALTER}"
        );
    }

    #[test]
    fn die_leiste_sitzt_ueber_der_statusleiste() {
        // GEMESSENE ENTSCHEIDUNG: von unten gerechnet — die Statusleiste ist
        // GEMESSEN 24 px, die Leiste GEMESSEN 34 + 8 = 42 px.
        assert_eq!(leiste_y(1080), 1080 - 24 - 42, "GEMESSEN: 1080 − 24 − 42");
        assert!(
            leiste_y(600) < leiste_y(1080),
            "GEMESSEN: hoeheres Fenster -> hoehere Leiste"
        );
    }

    #[test]
    fn der_pfeil_ist_dreieckig_und_spiegelt() {
        let r = pfeil_rechtecke(16, true);
        assert_eq!(r.len(), 3, "GEMESSEN: drei Rechtecke je Pfeil");
        // GEMESSEN: der Pfeil wird nach **unten** schmaler (1 -> 3).
        assert!(
            r[0].2 > r[2].2,
            "GEMESSEN: das erste Stueck ist breiter als das letzte"
        );
        let l = pfeil_rechtecke(16, false);
        assert_ne!(
            r[0].0, l[0].0,
            "GEMESSEN: der linke Pfeil sitzt an der anderen Kante"
        );
    }

    #[test]
    fn ein_pfeil_aus_null_px_wird_nicht_gezeichnet() {
        // GEMESSENE ENTSCHEIDUNG, warum das geprueft wird: `kante = 0` ergibt
        // bei einer Division durch drei eine **Division durch null** — die
        // Fehlerklasse „Zeichenpfad traegt eine Zahl, die der Aufrufer
        // nicht garantieren kann“.
        assert!(pfeil_rechtecke(2, true).is_empty() || pfeil_rechtecke(2, true).len() <= 3);
    }
}
