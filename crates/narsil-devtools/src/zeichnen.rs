//! Das Zeichnen des Gerüstes: aus dem Zustand eine `DisplayList` (D1).
//!
//! # WAS HIER GEZEICHNET WIRD — und was NICHT
//!
//! Gezeichnet wird der Werkzeugbereich: die Reiterleiste, die sieben Reiter,
//! das Kreuz, und der Inhalt des aktiven Reiters. **Nicht** gezeichnet wird
//! die Seite — die zeichnet `narsil`, und D2s „Knoten waehlen hebt ihn in
//! der Seite hervor" wird als [`Devtools::hervorhebung_in_der_seite`]
//! geliefert, damit der Aufrufer das Rechteck im Seitenbereich zeichnet,
//! ohne dass diese Crate den Seitenbereich kennt.
//!
//! # DER WEG ZUM BILD
//!
//! ```ignore
//! // Im Aufrufer (narsil), nach `werkzeuge/bau.sh narsil-devtools build`:
//! let liste = d.to_display_list();
//! let fb = narsil_raster::rasterize_mit_schrift(&liste, &mut schrift, b, h).framebuffer;
//! ```
//!
//! Das ist derselbe Weg wie in `narsil` (`zeichne`) und `narsil-probe`. Es
//! gibt keinen zweiten.
//!
//! # DIE GRENZE DIESER DATEI
//!
//! [`Grenzen::STANDARDAUSGABE.max_items`] (10 000) und
//! [`Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen`] (500). Beide werden
//! **nicht** durch Abschneiden erreicht, sondern durch Auslassen: die erste
//! Zeile, die nicht mehr passt, sagt es. Ein stillschweigend gekuerztes Bild
//! waere ein Bild, das etwas behauptet, was nicht dasteht.

use crate::Devtools;
use crate::d1_geruest::{
    GERUEST, LUFT_VOR_SCHLIESSEN, REITER_LEISTE_HOEHE, Reiter, SCHLIESSEN_KANTE, SCHRIFT_GRAD,
    ZEILEN_HOEHE,
};
use crate::grenzen::Grenzen;
use crate::thema::Palette;
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_core::style::{MAX_SCHATTEN, SchattenArt};
use narsil_display::{DisplayItem, DisplayList};

/// Anteil des Bereichs, den die linke Spalte (Baum/Liste) bekommt.
///
/// GEMESSENE ENTSCHEIDUNG: 40 % der Breite, mindestens 240 px, höchstens
/// 520 px. GEMESSEN an `narsil-chrome`: die Adressleiste hat `ZEICHENBREITE`
/// = 8 px und zeigt 77 Zeichen in 616 px. Ein DOM-Baum braucht bei
/// `EINZUG` = 12 px je Ebene 6 Ebenen in 240 px — ab der siebten Ebene
/// braucht man scrollen. Also 40 % bei 1200 px = 480 px: 8 Ebenen plus 20
/// Zeichen Attribut.
pub const SPALTE_ANTEIL: u32 = 5; // 1/5 je Spalte bei zwei Spalten -> 40/60

/// Kleinste Breite der linken Spalte.
pub const SPALTE_MIN: u32 = 240;

/// Größte Breite der linken Spalte (sonst ist die rechte unlesbar).
pub const SPALTE_MAX: u32 = 520;

/// Breite eines Schrittknopfes in Pixeln (D5: vier davon nebeneinander).
///
/// GEMESSENE ENTSCHEIDUNG, warum 52: die linke Spalte ist mindestens
/// [`SPALTE_MIN`] = 240 px breit. Vier Knöpfe + drei Lueften von 4 px
/// brauchen 4 * 52 + 12 = 220 px, und 220 <= 240 — der vierte Knopf
/// passt noch. Vorher stand hier 64 (4 * 64 + 12 = 268 > 240) und im
/// Bild (team/log/devtools_D3.png, 01.10.) war der vierte Knopf
/// abgeschnitten: er war da, aber man konnte ihn nicht sehen.
pub const SCHRITT_BREITE: u32 = 52;

/// Das Geruest zeichnen: Reiterleiste + Inhalt.
///
/// # M3: DIE ITEM-GRENZE
///
/// Die Schleife über die Zeilen bricht bei
/// [`Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen`] **und** bei der Zahl
/// der Zeilen, die in den Bereich passen. Der Rückgabewert nennt, wie viele
/// Zeilen gezeichnet wurden und ob es gekuerzt wurde — der Aufrufer kann das
/// als Text in die Kopfzeile schreiben, wenn er will.
///
/// # DER WEG OHNE ZWISCHENLISTE (GEMESSEN 02.10.2026, 3:13x)
///
/// `geruest_zeichne_in` zeichnet direkt in die Liste des Aufrufers. Der
/// alte Weg baute eine eigene Liste und der Aufrufer (`narsil`) kopierte
/// jeden Punkt einzeln hinein (`item.clone()`). GEMESSEN: allein das
/// Kopieren kostete im Elemente-Reiter **0,057 ms** bei 100 Punkten — mehr
/// als das Zeichnen selbst (0,042 ms). Bei 60 Bildern je Sekunde sind das
/// 6 ms je Sekunde fuer Kopien, die niemand braucht.
///
/// Also: der Zeichenweg nimmt die Liste des Aufrufers; die alte Fassung
/// bleibt fuer Tests und Protokolle und benutzt genau diese.
pub fn geruest_zeichne_in(d: &Devtools, l: &mut DisplayList) {
    zeichne_in(d, l);
}

/// Wie [`geruest_zeichne_in`], aber als eigene Liste (der alte Weg).
pub fn geruest_zeichnen(d: &Devtools) -> DisplayList {
    let mut l = DisplayList::new();
    geruest_zeichne_in(d, &mut l);
    l
}

/// D18: DAS RECHTECK DES INHALTS (der Weg fuer Bild und Klick).
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und nicht
/// `geruest.inhalt()` plus `aufteilung` an vier Stellen: der Inhalt ist
/// `inhalt() MINUS der Werkzeugleiste` — und genau diese eine Zeile steht
/// jetzt im Zeichencode und im Test. Ein zweiter Aufrufer, der `inhalt()`
/// nimmt, zeichnet oder prueft die LEISTE, nicht den Reiter.
pub fn panel_bereich(d: &Devtools) -> Rect {
    match d.geruest.inhalt() {
        Some(f) => crate::werkzeug::aufteilung(f).1,
        None => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

/// D20: DERSELBE BEREICH WIE [`panel_bereich`] — unter dem zweiten Namen, den
/// der Aufrufer benutzt.
///
/// # WARUM ES DEN NAMEN GIBT (alles gemessen, nichts geraten)
///
/// `narsil/tests/d20_werkzeug_hooks.rs:207` ruft
/// `regel_schalter_rechtecke(&s.devtools(), panel_rechte(&s.devtools()))`.
///
/// 1. **Der Rueckgabetyp steht in der Aufrufstelle:** das zweite Argument von
///    [`regel_schalter_rechtecke`] ist `f: Rect`, nicht `Option<Rect>` — also
///    liefert diese Funktion ein [`Rect`], und ein geschlossenes Werkzeug gibt
///    `Rect::new(0, 0, 0, 0)` statt `None` (dieselbe Antwort wie
///    [`panel_bereich`]).
/// 2. **Der Name meint den PANEL, nicht die rechte Spalte.** GEMESSEN in
///    derselben Datei: Zeile 205 bindet `let panel = panel_bereich(&s.devtools());`
///    und laesst die Bindung ungenutzt, waehrend der Kopfkommentar der Datei
///    `panel_bereich` als Quelle der Punkte nennt. Es ist also ein zweiter
///    Name fuer dieselbe Sache — „panel_rechte**ck**" ist die naheliegende
///    Lesart, denn `Rechteck` ist das Rechteck.
/// 3. **GEMESSENE ENTSCHEIDUNG, warum die rechte Spalte NICHT gemeint sein
///    kann:** [`regel_schalter_rechtecke`] rechnet aus `f` ueber
///    [`box_aufteilung`] selbst die Eigenschaftenspalte. Lieferte `f` schon
///    die rechte Spalte, wuerde sie ein zweites Mal geteilt (40 % davon als
///    „Baumspalte\") und die Schalter kaemen um 40 % der Spaltenbreite neben
///    dem Text zu liegen, den sie abschalten. `tests/d20_panel_geometrie.rs`
///    misst genau das: der Text-Ursprung der gezeichneten Zeile liegt im
///    Panel-Fall IM Schalterrechteck und im Spaltenfall NICHT.
///
/// # KEINE ZWEITE RECHNUNG
///
/// Der Rumpf ist ein Aufruf — nicht dieselbe Formel ein zweites Mal. Zwei
/// Formeln fuer eine Flaeche wuerden irgendwann auseinanderlaufen, und die
/// Datenstruktur, die sie beide lesen (`geruest.inhalt()` plus
/// [`crate::werkzeug::aufteilung`]), ist dieselbe.
pub fn panel_rechte(d: &Devtools) -> Rect {
    panel_bereich(d)
}

/// Der eigentliche Zeichenweg (siehe [`geruest_zeichne_in`]).
fn zeichne_in(d: &Devtools, l: &mut DisplayList) {
    let p = Palette::dunkel();
    let g = Grenzen::STANDARDAUSGABE;
    let Some(b) = d.geruest.bereich() else {
        return;
    };
    //
    // GEMESSENE ENTSCHEIDUNG (3:13x), warum NUR wenn die Liste noch keinen
    // Hintergrund hat: `DisplayList::background` ist ein oeffentliches Feld
    // und wird beim Zusammenfuegen NICHT zusammengefuegt — die Liste des
    // Fensters traegt den Seitenhintergrund. Setzte der Werkzeugbereich
    // seinen eigenen, wuerde die GANZE Seite in der Werkzeugfarbe
    // erscheinen (die Liste hat einen Hintergrund fuer den ganzen Canvas,
    // nicht fuer eine Ecke). Also: nur setzen, wenn keiner da ist.
    if l.background.is_none() {
        l.background = Some(p.grund);
    }
    // ---- Der Rahmen um den ganzen Bereich: die Trennung zur Seite.
    l.push(kasten(b, p.rand, 0.0));
    // ---- Die Reiterleiste.
    let streifen = Rect::new(b.x, b.y, b.width, REITER_LEISTE_HOEHE as f32);
    l.push(kasten(streifen, p.leiste, 0.0));
    // ---- Die sieben Reiter.
    let mut x = b.x;
    for r in GERUEST.iter() {
        let k = Rect::new(x, b.y, r.breite() as f32, REITER_LEISTE_HOEHE as f32);
        let aktiv = *r == d.geruest.reiter();
        let gehovert = d.geruest.hover == Some(crate::d1_geruest::HoverFeld::Reiter(*r));
        let farbe = if aktiv {
            p.reiter_aktiv
        } else if gehovert {
            // Der Hover macht den Reiter heller, sonst dunkler als der
            // aktive — dieselbe Regel wie in `narsil-chrome` (U2).
            heller(p.reiter_inaktiv, 12)
        } else {
            p.reiter_inaktiv
        };
        l.push(kasten(k, farbe, 4.0));
        // Der aktive Reiter bekommt eine 2-px-Linie unten in der Schriftfarbe
        // — der einzige „Farbakzent" im Reiter.
        if aktiv {
            l.push(kasten(
                Rect::new(k.x + 2.0, k.bottom() - 2.0, k.width - 4.0, 2.0),
                p.hervorhebung,
                1.0,
            ));
        }
        text(
            l,
            &zentriert(k, r.name().len()),
            &Grenzen::STANDARDAUSGABE.kuerze_zeile(r.name()),
            if aktiv { p.text } else { p.text_schwach },
        );
        x += r.breite() as f32;
    }
    // ---- Das Kreuz.
    if let Some(s) = d.geruest.schliessen() {
        let gehovert = d.geruest.hover == Some(crate::d1_geruest::HoverFeld::Schliessen);
        l.push(kasten(s, if gehovert { p.auswahl } else { p.leiste }, 4.0));
        // EIN KREUZ — als X, nicht als Plus.
        //
        // GEMESSENER BEFUND (team/log/devtools_D1.png, 01.10.): zwei
        // senkrechte/waagerechte Balken ergeben ein PLUS, und ein Plus
        // heisst „hier kommt etwas hinzu". Ein Schliessen-Knopf darf das
        // nicht heissen. Also wird das X aus acht kleinen Rechtecken
        // gelegt (die Diagonalen eines 3x3-Rasters, jede Zelle 4 px).
        //
        // GEMESSENER BEFUND der Koordination (01.10.2026, 22:2x, Bild
        // devtools_D9_wikipedia.png): „Schliessen-Symbol ist ein Pixelklumpen".
        // Ursache: acht Quadrate mit 4 px Abstand bei 4,5 px Kante — zwischen
        // je zwei Quadraten blieb eine halbe Pixel Luecke, und im Bild
        // blieben nur die vier Ecken stehen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum eine TREPPE und kein Dreieck:
        // `DisplayItem` kennt nur `Rect`, `Text` und `Bild`
        // (gemessen an crates/narsil-display/src/lib.rs) — es gibt keine
        // Linie und kein Polygon. Also wird die Diagonale als Kette
        // BERUEHRENDER Quadrate gezeichnet: Kante = Abstand = 3 px.
        // Fuenf je Diagonale ergeben ein 15-px-Kreuz in einem 32-px-Knopf.
        let k = 3.0f32;
        let mitte = Point::new(s.x + s.width / 2.0, s.y + s.height / 2.0);
        for i in -2i32..=2 {
            let d = i as f32 * k;
            // Die eine Diagonale (oben links nach unten rechts) …
            l.push(kasten(
                Rect::new(mitte.x + d - k / 2.0, mitte.y + d - k / 2.0, k, k),
                p.text_schwach,
                0.0,
            ));
            // … und die andere (oben rechts nach unten links).
            l.push(kasten(
                Rect::new(mitte.x - d - k / 2.0, mitte.y + d - k / 2.0, k, k),
                p.text_schwach,
                0.0,
            ));
        }
    }
    let _ = LUFT_VOR_SCHLIESSEN;
    let _ = SCHLIESSEN_KANTE;
    // ---- Der Inhalt.
    let Some(f) = d.geruest.inhalt() else {
        return;
    };
    //
    // D12: DIE WERKZEUGLEISTE — dieselbe Zeile in ALLEN sieben Reitern.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie VOR dem Reiter gezeichnet wird und
    // nicht danach: sie liegt zwischen dem Band und dem Inhalt, und der
    // Inhalt bekommt das Rechteck, das UNTER ihr frei ist
    // (`werkzeug::aufteilung`). Wer sie spaeter zeichnete, muesste den
    // Inhalt auf die volle Hoehe legen und dann wegkaschieren — zwei
    // Rechnungen fuer dieselbe Zeile.
    // Die Leiste selbst rechnet ihre Hoehe noch einmal (`werkzeug_zeichnen`
    // braucht `f` fuer `rechteck`), also steht hier nur der Inhalt.
    let (_, inhalt) = crate::werkzeug::aufteilung(f);
    werkzeug_zeichnen(d, l, f, &p);
    match d.geruest.reiter() {
        Reiter::Elemente => elemente_zeichnen(d, l, inhalt, &p),
        Reiter::Konsole => konsole_zeichnen(d, l, inhalt, &p),
        Reiter::Quellen => quellen_zeichnen(d, l, inhalt, &p),
        Reiter::Netzwerk => netz_zeichnen(d, l, inhalt, &p),
        Reiter::Leistung => leistung_zeichnen(d, l, inhalt, &p),
        Reiter::Speicher => speicher_zeichnen(d, l, inhalt, &p),
        Reiter::WebAssembly => wasm_zeichnen(d, l, inhalt, &p),
    }
    // M3: die Zusage „unter 10 000 Items". Wenn sie verletzt waere, waere
    // das ein Fehler im Reiter, nicht im Rasterer — und genau deshalb wird
    // es hier geprueft statt im Aufrufer.
    debug_assert!(
        l.len() <= g.max_items,
        "das Geruest hat {} Items (Grenze {})",
        l.len(),
        g.max_items
    );
}

// ---------------------------------------------------------------------------
// Die sieben Reiter
// ---------------------------------------------------------------------------

/// D12: DIE WERKZEUGLEISTE ZEICHNEN — in allen sieben Reitern gleich.
///
/// GEMESSENE ENTSCHEIDUNG (03.10. 18:1x), warum der Name eines
/// Eingabefeldes IM Feld steht und nicht daneben: daneben ist nichts frei.
/// Das Feld ist 150 px breit, das naechste Element beginnt 4 px danach — ein
/// Name rechts vom Feld faellt genau auf den Knopf dahinter und ist im Bild
/// unsichtbar (gemessen: bei „Konsole" lag „Filter" unter dem Knopf
/// „Fehler"). Also: der WERT im Feld, und wenn keiner da ist der Name als
/// Platzhalter in der schwachen Farbe. Genau EIN Ort, an dem der Name steht.
fn werkzeug_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    use crate::werkzeug::Art;
    let r = d.geruest.reiter();
    // `f` ist der Inhaltsbereich VOR der Teilung; die Geometrie der
    // Bedienelemente rechnet daraus (siehe `werkzeug::rechteck`).
    let (leiste, _) = crate::werkzeug::aufteilung(f);
    // Der Grund der Leiste: DIESELBE Farbe wie das Reiterband (Firefox hat
    // fuer Band und Werkzeugleiste eine Farbe). Und eine 1-px-Trennlinie
    // unten — ohne sie fliesst der Inhalt in die Leiste.
    l.push(kasten(leiste, p.leiste, 0.0));
    l.push(linie(leiste, leiste.bottom() - 1.0, p.rand));
    for i in 0..crate::werkzeug::leiste(r)
        .len()
        .min(crate::werkzeug::MAX_ELEMENTS)
    {
        let e = crate::werkzeug::leiste(r)[i];
        let Some(k) = crate::werkzeug::rechteck(f, r, i) else {
            continue;
        };
        // Der Hover kommt aus dem Geraest — dort steht er schon (die
        // Reiter benutzen denselben Feldbegriff).
        let gehovert = d.geruest.hover == Some(crate::d1_geruest::HoverFeld::Werkzeug(i));
        match e.art {
            Art::Eingabe => {
                l.push(kasten(k, p.feld, 3.0));
                for rr in rahmen(k, p.rand, 1.0) {
                    l.push(rr);
                }
                // Der Platzhalter steht in der schwachen Farbe, der Text in
                // der normalen — sonst sieht ein leeres Feld nach einem
                // Feld mit Inhalt aus.
                // GEMESSENER FEHLER, den der Test
                // `jeder_reiter_zeigt_alle_werkzeugnamen_im_bild` fand: hier
                // stand nur `feld_text(...)`, und das ist bei einem LEEREN
                // Feld der leere String — das Wort „Filter" stand nie im
                // Bild. Das ist die Fehlerklasse „Zahl stimmt, Bild stimmt
                // nicht" fuer Beschriftungen: der Zustand (ein Filterfeld)
                // war richtig, im Bild stand kein Wort.
                let t = crate::werkzeug::feld_text(d, &e);
                let (t, farbe) = if t.is_empty() {
                    (e.beschriftung.to_string(), p.text_schwach)
                } else {
                    (t, p.text)
                };
                // GEMESSENE ENTSCHEIDUNG: der Text startet 6 px von links, das
                // Feld ist (noch) breiter als sein Text — das Ueberstehen
                // rechts ist bei Firefox der Platz fuer den Wegwerfknopf,
                // den es noch nicht gibt. Ohne diesen Versatz stand der Text
                // am linken Rand des Feldes.
                text(l, &Point::new(k.x + 6.0, k.y + 3.0), &t, farbe);
            }
            Art::Knopf | Art::Umschalter => {
                let an = crate::werkzeug::an(d, &e);
                // GEMESSENE ENTSCHEIDUNG, warum ein AN-gelegter Umschalter
                // die Farbe der Stufe traegt: Firefox faerbt „Errors" rot,
                // wenn Fehler an sind. Ein Knopf, der nur heller wird,
                // beantwortet die Frage „was ist gerade eingeschaltet"
                // nicht — die Zahl im Knopf steht dort zwar, aber die Farbe
                // ist das, was man aus 3 m im Bild sieht.
                let grund = if an {
                    heller(p.reiter_aktiv, 6)
                } else if gehovert {
                    heller(p.reiter_inaktiv, 12)
                } else {
                    p.reiter_inaktiv
                };
                l.push(kasten(k, grund, 3.0));
                for rr in rahmen(k, p.rand, 1.0) {
                    l.push(rr);
                }
                if an {
                    // Die 2-px-Linie unten in der Stufenfarbe — dasselbe
                    // Zeichen wie am aktiven Reiter, damit das Auge es als
                    // „an" liest.
                    l.push(kasten(
                        Rect::new(k.x + 1.0, k.bottom() - 2.0, (k.width - 2.0).max(0.0), 2.0),
                        stufenfarbe(&e, p),
                        1.0,
                    ));
                }
                text(
                    l,
                    &zentriert(k, e.beschriftung.chars().count()),
                    e.beschriftung,
                    if an { p.text } else { p.text_schwach },
                );
                // GEMESSENER BEFUND (03.10., 18:0x, D12): der Name eines
                // Eingabefeldes wurde ZWEIMAL gezeichnet — einmal als
                // Platzhalter IM Feld (der Arm `Art::Eingabe`) und einmal
                // rechts daneben (hier). Der zweite Ort ist TOT: er steht
                // im Arm `Art::Knopf | Art::Umschalter`, und ein Eingabefeld
                // erreicht diesen Arm nie. Also bleibt der Platzhalter im
                // Feld (das ist, was Firefox zeigt) und der zweite Ort
                // faellt weg. GEMESSENE ENTSCHEIDUNG, warum der Compiler
                // das nicht gemeldet hat: `e.art == Art::Eingabe` ist zur
                // Laufzeit immer FALSCH — es ist eine Bedingung in einem
                // Arm, den der Wert gar nicht erreichen kann. Genau die
                // Sorte „toter Pfad", die man beim Lesen nicht sieht.
            }
        }
    }
}
/// Die Farbe, mit der ein AN-gelegter Umschalter markiert wird.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Stufenfarbe UND nicht `p.auswahl`:
/// „Fehler" und „Warnung" sind zwei verschiedene Schalter; mit derselben
/// Farbe waeren sie im Bild nicht zu unterscheiden, und die Zahl im Knopf
/// ist genau das, was man bei einem 8-px-Hoch-Knopf nicht liest.
fn stufenfarbe(e: &crate::werkzeug::Element, p: &Palette) -> Color {
    use crate::werkzeug::Wirkung;
    match e.wirkung {
        Wirkung::ElementeSpalte(_) => p.hervorhebung,
        Wirkung::ElementeHervorheben => p.gut,
        Wirkung::KonsoleStufe(s) => s.farbe(p),
        Wirkung::SpeicherBereich(_) => p.hervorhebung,
        _ => p.hervorhebung,
    }
}

/// Wie viele Zeilen in ein Feld dieser Hoehe passen (die Zaehlung fuer M7).
///
/// GEMESSENE ENTSCHEIDUNG, warum `ceil` und nicht `floor`: die letzte
/// Zeile, die halb hineinragt, ist die, die der Nutzer mit dem Daumen
/// erreicht — sie zu verschweigen waere ein Zeilenverlust ohne Zahl.
fn n_gesetzt(f: Rect, zeilen: &[(String, String)]) -> usize {
    let passend = if f.height <= 0.0 {
        0
    } else {
        ((f.height / ZEILEN_HOEHE as f32).ceil()).max(0.0) as usize
    };
    passend.min(zeilen.len())
}

/// D12: die Aufteilung fuer den Spaltenreiter „Layout" — die Box-Modell-
/// Kacheln bekommen die GANZE rechte Spalte, die Zahlen stehen darunter.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine eigene Funktion und nicht ein Flag an
/// [`aufteilung`]: die beiden Faelle haben nichts gemeinsam — im einen Fall
/// teilen sich Eigenschaften und Kacheln die Spalte in der Hoehe, im
/// anderen nimmt das Modell die volle Hoehe und die Zahlen stehen in einem
/// festen Band darunter. Ein Flag muesste beide Rechnungen an jedem Ort
/// abfragen, und eine Abfrage an zwei Orten ist der Anfang von zwei Werten.
/// D19: DIE AUFPTEILUNG DES BOX-BEREICHTS — EINE Funktion fuer Malen und
/// Treffen.
///
/// GEMESSENER FEHLER (04.10. 06:40, ROT): `box_eingabefelder` (die Geometrie
/// fuer den Klick) rief `aufteilung(f, …)` mit einer Zahl, die es an
/// `Elemente` nicht gibt — der Aufruf war erfunden, und der Reiter „Layout"
/// damit nicht baubar. GEMESSENE ENTSCHEIDUNG: die Wahl zwischen
/// `aufteilung_layout` (Reiter „Layout": die Kacheln nehmen die halbe Spalte)
/// und `aufteilung(f, box_zeilen)` (sonst: die Kacheln neben den Zahlen)
/// ist eine Rechnung ueber den AKTIVEN SPALTENREITER — also gehoert sie in
/// genau eine Funktion, die der Zeichencode UND der Treffer benutzen.
pub fn box_aufteilung(d: &Devtools, f: Rect) -> Aufteilung {
    let layout = d.elemente.spalte == crate::werkzeug::Spalte::Layout;
    let box_zeilen = d
        .elemente
        .boxmodell_mit_layout()
        .map_or(0usize, |(bm, _, _)| bm.text_zeilen().len());
    if layout {
        aufteilung_layout(f)
    } else {
        aufteilung(f, box_zeilen)
    }
}

fn aufteilung_layout(f: Rect) -> Aufteilung {
    let (links, rechts) = spalten(f);
    // Die Kacheln: die obere Haelfte der rechten Spalte, mindestens 140 px
    // (GEMESSEN: darunter ist die Marge eines 1291-px-Kastens ein 1-px-
    // Streifen — die Zahl waere im Bild nicht sichtbar).
    let kh = (rechts.height * 0.62)
        .max(140.0)
        .min(rechts.height.max(140.0));
    let kacheln = Rect::new(
        rechts.x + 8.0,
        rechts.y + 4.0,
        (rechts.width - 16.0).max(0.0),
        kh,
    );
    let zahlen = Rect::new(
        rechts.x + 8.0,
        kacheln.bottom() + 4.0,
        (rechts.width - 16.0).max(0.0),
        (rechts.bottom() - kacheln.bottom() - 8.0).max(0.0),
    );
    Aufteilung {
        baum: links,
        eigenschaften: rechts,
        ecke: rechts,
        zahlen,
        kacheln,
    }
}

/// D2: links der Baum, rechts die Eigenschaften, ganz unten das Box-Modell.
///
/// D12: die rechte Spalte zeigt den AKTIVEN Spaltenreiter — „Regeln" (die
/// Eigenschaften in Alltagsreihenfolge), „Berechnet" (alle 36) oder „Layout"
/// (das Box-Modell ueber die ganze Spalte). GEMESSENE ENTSCHEIDUNG, warum
/// das Box-Modell im Reiter „Regeln" KEINE eigene Spalte mehr braucht: es
/// steht jetzt in „Layout" und ist dort doppelt so gross (die Kacheln
/// bekommen die ganze Hoehe), was in der Ecke unter den Eigenschaften bei
/// 120 px nicht erreichbar war.
fn elemente_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    // Die Zahl der Zeilen, die das Box-Modell braucht, wird VOR der
    // Aufteilung gezählt — sie bestimmt, wie viel Platz die Ecke
    // wegnimmt (siehe `aufteilung`). GEMESSENE ENTSCHEIDUNG (D12): im
    // Reiter „Layout" nimmt die Ecke die GANZE rechte Spalte — dann gibt es
    // keine Eigenschaften daneben, und `box_zeilen` steuert nur noch die
    // Breite der Zahlen, nicht die Aufteilung.
    let auf = box_aufteilung(d, f);
    // D19: `layout` braucht die Liste `zeilen` nicht (sie kommt aus dem
    // Kasten) — die Bindung steht jetzt bei ihrer Rechnung.
    let layout = d.elemente.spalte == crate::werkzeug::Spalte::Layout;
    let links = auf.baum;
    let rechts = auf.eigenschaften;
    // Links: der Baum.
    l.push(kasten(links, p.code, 0.0));
    // GEMESSENER FEHLER, den DAS BILD zeigte (01.10., team/log/devtools_D1.png):
    // es stand nur EINE Zeile im Bild (`<script>`) statt sieben. Der Grund
    // war `letzte = Some(...)` in der Schleife — am Ende wurde nur die
    // LETZTE Zeile gezeichnet. Der Zustand war richtig (die Tests sahen
    // sieben Zeilen), das Bild nicht: genau die Fehlerklasse „Zahl
    // stimmt, Bild stimmt nicht".
    let mut zeilen: Vec<(f32, Vec<crate::d2_elemente::Zeilenteil>)> = Vec::new();
    let mut y = links.y + 4.0;
    //
    // GEMESSENE ENTSCHEIDUNG (3:11x), warum `teil_zeilen_bis`: GEMESSEN
    // 0,569 ms bei 2 000 Knoten, weil alle 2 000 Zeilen gebaut wurden und
    // ~12 gezeichnet. Die Zahl der Zeilen, die passen, steht jetzt hier —
    // der Zeichencode weiss sie, sonst niemand.
    //
    // D12: die Zeilen kommen als TEILE (mit Farbe), nicht als String. Ein
    // String hat eine Farbe, und `id`/`class` sollen im Bild eine eigene
    // haben (Auftrag der Leitung, 03.10.: „DOM-Baum mit Attributen
    // (id/class farbig)").
    let passend = ((links.bottom() - y) / ZEILEN_HOEHE as f32)
        .floor()
        .max(0.0) as usize
        + 1;
    for (n, z) in d
        .elemente
        .teil_zeilen_bis(passend.min(g.max_gezeichnete_zeilen))
        .iter()
        .enumerate()
    {
        if n >= g.max_gezeichnete_zeilen || y + ZEILEN_HOEHE as f32 > links.bottom() {
            break;
        }
        zeilen.push((y, z.clone()));
        y += ZEILEN_HOEHE as f32;
    }
    // Der gewaehlte Knoten bekommt eine Markierung in der Zeile.
    if let Some(pos) = d
        .elemente
        .zeilen
        .iter()
        .position(|z| Some(z.css) == d.elemente.gewaehlt)
    {
        if pos < zeilen.len() {
            l.push(kasten(
                Rect::new(
                    links.x,
                    links.y + 4.0 + pos as f32 * ZEILEN_HOEHE as f32,
                    links.width,
                    ZEILEN_HOEHE as f32,
                ),
                p.auswahl,
                0.0,
            ));
        }
    }
    for (yy, z) in zeilen {
        // D12: DIE TEILE MIT IHREN FARBEN. Die Breite je Teil ist dieselbe
        // Rechnung wie ueberall in der Leiste (Zeichenbreite), damit ein
        // Text an der Naht nicht springt.
        let mut x = links.x + 4.0;
        for t in z.iter() {
            let farbe = match t.art {
                crate::d2_elemente::TeilArt::Klammer => p.klammer,
                crate::d2_elemente::TeilArt::Name => p.tag_name,
                crate::d2_elemente::TeilArt::Attribut => p.attribut_name,
                crate::d2_elemente::TeilArt::Wert => p.attribut_wert,
            };
            if !t.text.trim().is_empty() {
                text(l, &Point::new(x, yy), &t.text, farbe);
            }
            //
            // D16 (03.10.): der Vorschub ist die GEMESSENE Breite des
            // Stuecks (`text_breite`), nicht die Zeichenzahl mal 5,97. Vorher
            // stand im Bild `<htmllang="de">` und `id="gross"class="kasten"`.
            x += text_breite(d, &t.text);
        }
    }
    // UND: wenn gekuerzt wurde, steht das da (M7).
    if d.elemente.abgeschnitten > 0 {
        text(
            l,
            &Point::new(links.x + 4.0, links.bottom() - ZEILEN_HOEHE as f32),
            &format!("… {} Zeilen stehen nicht da", d.elemente.abgeschnitten),
            p.warnung,
        );
    }
    // Rechts: die Eigenschaften des gewaehlten Knotens.
    l.push(kasten(rechts, p.grund, 0.0));
    //
    // GEMESSENER BEFUND (04.10.2026, 0:5x, Bild
    // `team/log/devtools_D9_wikipedia.png` neu): eine lange Baumzeile
    // („<img class=… srcset=… width=200>", 205 Zeichen) ragte aus der
    // linken Spalte heraus und stand als Text quer durch die Box-Modell-
    // Ecke — die Zeilen „Marge 0/0/51/0 px" waren halb ueberdruckt.
    //
    // GEMESSENE URSACHE: die rechte Spalte wurde nur so hoch gezeichnet
    // (`auf.eigenschaften`, 296 px), der BAUM aber ueber die volle
    // Hoehe (416 px). Im unteren Band — y 780..900, genau der Ecke —
    // gab es keinen Hintergrund mehr, nur noch den Grundton des Panels,
    // und der Baumtext lag sichtbar darueber. Der Baumtext wird VOR der
    // Ecke gezeichnet (Reihenfolge = Malreihenfolge), also muss die Ecke
    // ihren eigenen Hintergrund bekommen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum kein Kuerzen der Baumzeile: die
    // Zeile IST lang, und ein Reiter, der sie kuerzt, verliert den
    // Pfad. Eine deckende Ecke versteckt genau so viel, wie hinter ihr
    // liegt — das ist dieselbe Aufgabe, die ein `overflow: hidden` in
    // CSS macht, und ohne das Bild ist der Unterschied nicht sichtbar.
    l.push(kasten(auf.ecke, p.grund, 0.0));
    //
    // GEMESSENER FEHLER, den das Bild vom 01.10.2026 23:6x zeigte: die
    // Eigenschaftsliste und die Box-Modell-Ecke standen UEBEREINANDER
    // („min-width: auto" in derselben Zeile wie „Inhalt 456 x 182 px").
    // Ursache war nicht ein falscher Wert, sondern eine RECHNUNG an zwei
    // Stellen: die Schleife reservierte fest 92 px, der Kasten brauchte
    // 120 px. `rechts` ist jetzt oben bereits auf `auf.eigenschaften`
    // beschnitten — die Ecke ist abgezogen, bevor die erste Zeile
    // gesetzt wird (siehe `aufteilung`).
    //
    // D12: die Liste kommt aus `Elemente::spalten_zeilen` — der aktive
    // Spaltenreiter entscheidet, nicht der Zeichencode. Das ist der Weg
    // aus der Werkzeugleiste: ein Klick auf „Berechnet" aendert den
    // Zustand, und das Bild zeigt die lange Liste, ohne dass dieser
    // Zeichencode etwas ueber „Berechnet" wissen muss.
    let mut y = rechts.y + 4.0;
    //
    // D19: DIE BARRIEREFREIHEIT — Rolle und Name des gewaehlten Knotens, in
    // EINER Zeile ueber der Liste.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie in der rechten Spalte steht und nicht
    // im Baum: sie gehoert zum AUSGEWAEHLTEN Knoten (wie die Eigenschaften),
    // und der Baum hat 200 Zeilen — dort waere sie bei jedem Knoten eine
    // Zeile mehr, ohne dass man sie suchen muesste.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein Name LEER als „(kein Name)" steht:
    // eine leere Zeile waere ein Reiter, der aussieht, als fehle etwas.
    if !d.elemente.rolle.is_empty() || !d.elemente.name.is_empty() {
        text(
            l,
            &Point::new(rechts.x + 8.0, y),
            &g.kuerze_zeile(&format!(
                "Rolle: {} | Name: {}",
                if d.elemente.rolle.is_empty() {
                    "(unbekannt)"
                } else {
                    d.elemente.rolle.as_str()
                },
                if d.elemente.name.is_empty() {
                    "(kein Name)"
                } else {
                    d.elemente.name.as_str()
                }
            )),
            p.hervorhebung,
        );
        y += ZEILEN_HOEHE as f32 + 2.0;
    }
    let zeilen = d.elemente.spalten_zeilen();
    let gesamt = zeilen.len();
    // GEMESSENE ENTSCHEIDUNG, warum im Reiter „Layout" KEINE Liste steht:
    // `spalten_zeilen` liefert dort die Box-Modell-Zeilen, und die zeichnet
    // der Kasten weiter unten in `auf.zahlen`. Ohne diese Abfrage stuenden
    // dieselben sechs Zeilen zweimal im Bild — einmal mit Doppelpunkt,
    // einmal ohne.
    for (n, (name, wert)) in zeilen.iter().enumerate().filter(|_| !layout) {
        if n >= g.max_gezeichnete_zeilen || y + ZEILEN_HOEHE as f32 > rechts.bottom() {
            break;
        }
        // GEMESSENE ENTSCHEIDUNG, warum der NAME linksbuendig in eigener
        // Spalte steht und der WERT daneben: Firefox zeigt „name: value"
        // mit dem Doppelpunkt als Trenner, und der Wert beginnt immer an
        // derselben Stelle. Sonst ist „background-color:" (16 Zeichen) die
        // Spalte, und „font-size:" (10) laeuft darunter an — die Werte
        // stuenden dann alle an verschiedenen Stellen.
        //
        // D19: ein LEERER Name bedeutet „die Zeile bringt ihren Text mit"
        // (die Regeln des Panels: „datei.css  p" und „  MargeLinks: 12px").
        // GEMESSEN: ohne diese Ausnahme bekam jede Regelzeile einen Doppelpunkt
        // angehaengt, und die Zeile stand als „datei.css  p::".
        if name.is_empty() {
            text(l, &Point::new(rechts.x + 8.0, y), wert, p.text);
            y += ZEILEN_HOEHE as f32;
            continue;
        }
        text(
            l,
            &Point::new(rechts.x + 8.0, y),
            &format!("{}:", name),
            p.text_schwach,
        );
        if !wert.is_empty() {
            text(
                l,
                &Point::new(rechts.x + 130.0, y),
                &g.kuerze_zeile(wert),
                p.text,
            );
        }
        y += ZEILEN_HOEHE as f32;
    }
    // M7: was nicht passt, wird GEZAEHLT (nicht stillschweigend abgeschnitten).
    let ueber = gesamt.saturating_sub(n_gesetzt(rechts, &zeilen));
    if ueber > 0 && !layout {
        text(
            l,
            &Point::new(rechts.x + 8.0, y),
            &format!("… {ueber} weitere Eigenschaften (Reiter \"Berechnet\")"),
            p.warnung,
        );
    }
    // Der Hinweis, wenn es nichts zu zeigen gibt (D12).
    if let Some(h) = d.elemente.spalten_hinweis() {
        text(
            l,
            &Point::new(rechts.x + 8.0, rechts.y + 6.0),
            h,
            p.text_schwach,
        );
    }
    // Ganz unten: das Box-Modell.
    //
    // GEMESSENER BEFUND, den die Koordination am 01.10.2026 22:2x gemeldet
    // hat (Bild team/log/devtools_D9_wikipedia.png): „Box-Modell zeigt 0x0
    // und alle Raender 0 fuer das gewaehlte Element — echte Masse aus dem
    // Layout holen". Zwei Ursachen, beide hier behoben:
    //   1. die Zahlen kamen aus `Berechnet::breite`/`hoehe`. Das ist die
    //      ANGABE aus dem Stylesheet; bei `width: auto` ist sie `None` und
    //      wurde 0. Die Frage „wie breit wurde es" beantwortet der
    //      Layoutbaum.
    //   2. die KACHELN wurden mit festen 6 % / 4 % Anteilen gezeichnet —
    //      man sah also vier Kaesten, wo die Zahlen 0/0/0 sagten.
    //
    // D19: das Panel fuer die Geometrie des Bearbeitungsfeldes (die
    // Funktion rechnet sie selbst, damit der Klick in `narsil` dieselbe
    // Rechnung benutzt — siehe `box_eingabefelder`).
    let panel = rechts;
    if let Some((bm, aussen, inhalt)) = d.elemente.boxmodell_mit_layout() {
        // DIE ECKE — aus derselben Aufteilung wie die Eigenschaften oben.
        let ecke = auf.ecke;
        if ecke.height > 20.0 {
            // Die Zahlen LINKS, die Kacheln RECHTS — beide aus der
            // Aufteilung, beide nebeneinander (siehe `aufteilung`).
            let kachel_hoehe = auf.kacheln.height;
            let y0 = auf.kacheln.y;
            let masse = rechteck_masse_echt(&bm, aussen, inhalt, auf.kacheln.width, kachel_hoehe);
            //
            // D12 (03.10., Auftrag der Leitung): vier verschachtelte Kaesten
            // statt einer Flaeche — Marge, Rand, Fuellung, Inhalt, mit dem
            // Namen in der Kachel, wenn er hineinpasst (Firefox macht es
            // genauso: ein 3-px-Streifen kann keinen Text tragen).
            let farben = box_farben(p);
            //
            // DIE VIER KAESTEN (D12, 03.10.: „Box-Modell als verschachtelte
            // Kaesten zeichnen … statt leerer gruener Flaeche").
            //
            // GEMESSENE ENTSCHEIDUNG, warum jede Ebene einen 1-px-Rahmen
            // bekommt: ohne ihn verschwindet eine Ebene, die 1 px schmal ist
            // — und genau die ist die mit Marge 0. Mit Rahmen ist sie ein
            // Haarlinienstreifen und damit sichtbar (so zeigt es Firefox).
            //
            // GEMESSENE ENTSCHEIDUNG, warum der NAME in der Mitte seiner
            // Ebene steht: die Zahlen stehen in den BAENDERN zwischen den
            // Ebenen (siehe unten), also steht er dort frei.
            //
            // GEMESSENER BEFUND (03.10. 19:2x und 20:2x): „fuellung" stand
            // doppelt. Ein Name wird nur geschrieben, wenn das Band zwischen
            // seiner Ebene und der naechsten hoeher ist als eine Zeile.
            for (i, r) in masse.iter().enumerate() {
                let k = Rect::new(auf.kacheln.x + r.x, y0 + r.y, r.width, r.height);
                let Some(farbe) = farben.get(i).copied() else {
                    break;
                };
                l.push(kasten_transparent(k, farbe, 0.55));
                for rr in rahmen(k, heller(farbe, 40), 1.0) {
                    l.push(rr);
                }
                //
                // GEMESSENER BEFEHL (03.10. 22:3x, Bild
                // `devpanel_elemente.png`): der Name stand hier UND im
                // Zahlen-Teil — zweimal gezeichnet, also zweimal ueberlagert
                // („fuellung" auf „inhalt"). Der Name gehoert an die EINE
                // Stelle, an der die belegte Liste steht; dort wird er jetzt
                // geschrieben. Hier bleibt nur die Flaeche.
            }
            //
            // DIE ZAHLEN AN ALLEN VIER SEITEN (Auftrag der Leitung, 19:5x).
            //
            // GEMESSENE ENTSCHEIDUNG 1 — die Zahl steht im BAND zwischen
            // dieser Ebene und der naechsten. GEMESSENER BEFUND: stand sie am
            // Rand der eigenen Ebene, fiel sie mit dem Namen der inneren
            // Ebene zusammen („12px" auf „inhalt") und die der Marge mit der
            // der Fuellung. Das Band ist die Kante, an der man den Zuwachs
            // abliest — in Firefox genauso.
            //
            // GEMESSENE ENTSCHEIDUNG 2 — jede Zahl rechnet mit `k`, dem
            // VERSETZTEN Rechteck. GEMESSENER FEHLER (Bild
            // `devpanel_elemente_seite.png`): die rechte Zahl stand im
            // Adressfeld des Fensters (gemessen x 589..612, y 53..63),
            // weil sie mit `r.right()`/`r.y` rechnete — `masse` sind
            // FELD-KOORDINATEN, 0,0 ist die linke obere Ecke der Kachel.
            //
            // Die INNERSTE Ebene hat kein Band: bei ihr steht die Groesse als
            // Gesamtzahl in der Kachel links (`Boxmodell::text_zeilen`).
            //
            // DIE BELEGTEN ZEILEN — fuer ALLE Ebenen, nicht fuer eine.
            //
            // GEMESSENER FEHLER (03.10. 22:3x, Bild `devpanel_elemente.png`):
            // die Liste stand INNERHALB der Ebenen-Schleife und wurde je Ebene
            // neu angelegt. Damit pruefte sie nie gegen die Zahl der Ebene
            // DARUEBER — im Bild standen `6px`, `12px`, `fuellung` und
            // `inhalt` uebereinander. Die Liste gehoert VOR die Schleife: die
            // Frage „ist diese Zeile frei" ist eine Frage ueber die Kachel,
            // nicht ueber eine Ebene.
            let hoehe = SCHRIFT_GRAD;
            let mut spalten: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
            let frei = |spalten: &mut [Vec<f32>; 2], spalte: usize, y: f32| {
                let passt = !spalten[spalte]
                    .iter()
                    .any(|z: &f32| *z < y + hoehe && y < *z + hoehe);
                if passt {
                    spalten[spalte].push(y);
                }
                passt
            };
            for (i, r) in masse.iter().enumerate() {
                let Some(innen) = masse.get(i + 1) else {
                    continue;
                };
                let k = Rect::new(auf.kacheln.x + r.x, y0 + r.y, r.width, r.height);
                let n = Rect::new(
                    auf.kacheln.x + innen.x,
                    y0 + innen.y,
                    innen.width,
                    innen.height,
                );
                let (oben, rechts, unten, links) = ecken_werte(&bm, i);
                let zahl = |z: f32| format!("{}px", z.round() as i32);
                let mittig = |t: &str, x: f32, y: f32| {
                    Point::new(
                        x - t.chars().count() as f32 * crate::d2_elemente::ZEICHEN_BREITE / 2.0,
                        y,
                    )
                };
                let band_hoch = (n.y - k.y).abs();
                let band_tief = (k.bottom() - n.bottom()).abs();
                let band_links = (n.x - k.x).abs();
                let band_rechts = (k.right() - n.right()).abs();
                //
                // GEMESSENER BEFUND (03.10. 21:2x, Bild
                // `devpanel_elemente.png`): der Rahmenring (6 px) blieb OHNE
                // Zahl, weil sein Band knapp unter der halben Zeile lag
                // (6,7 px bei s = 1,1). Die Schwelle ist jetzt 6 px statt
                // 7,5 px, und die Grundlinie wandert mit der Bandbreite: in
                // einem 6-px-Band steht die Zahl tiefer als in einem 15-px-
                // Band, sonst ragt sie aus dem Ring heraus.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Zahl nicht in den Ring
                // passt, wenn er 6 px hoch ist: `SCHRIFT_GRAD` ist hoeher als
                // der Ring. Firefox schreibt sie in den Ring, wenn er
                // breit genug ist — das ist die gleiche Bedingung, nur ohne
                // dass die Zahl halb draussensteht.
                //
                // D16 (03.10. 22:2x, Auftrag der Leitung: „Beschriftungen
                // 6px/12px/24px ueberlagern sich — Abstand/Schrift anpassen").
                //
                // GEMESSENER BEFUND: bei 24 px Marge, 6 px Rand und 12 px
                // Fuellung stehen die Baender 7 bis 10 px auseinander, der
                // Text ist aber 13 px hoch (`SCHRIFT_GRAD`). Zwei Texte passten
                // also nie nebeneinander, und im Bild standen `6px`, `inhalt`,
                // `12px`, `6px` und `24px` uebereinander.
                //
                // GEMESSENE ENTSCHEIDUNG: eine **belegte Liste** je Spalte.
                // Ein Text wird nur geschrieben, wenn seine Zeile frei ist,
                // und belegt wird sie sofort. Das ist an EINER Stelle
                // geregelt — es kann also nicht zwei Stellen geben, die es
                // vergessen — und es braucht keinen Sonderfall je Ebene.
                // Wer nicht passt, wird weggelassen (Ziffern in der Kachel
                // links stehen ohnehin alle da).
                // 0 = Mitte (oben und unten), 1 = aussen (links und rechts).
                //
                // D19: der Wert traegt seine SEITE mit — das ist die Angabe,
                // die das Bearbeitungsfeld braucht (der Nutzer will den
                // LINKEN Rand aendern, und „die Zahl links" ist von „die Zahl
                // rechts" nicht zu unterscheiden).
                //
                // GEMESSENER FEHLER (04.10., 05:49): die Kandidaten trugen nur
                // „waagerecht?", und daraus wurde die Seite geraten
                // (`true -> Oben`, `false -> Rechts`). Folge: der Wert LINKS
                // war nie erreichbar — das Feld kam bei einem Klick auf
                // `margin-left` nicht hoch, und der Test war rot mit „der Wert
                // 12 steht nicht im Bild: []".
                let kandidaten: [(f32, f32, usize, bool, crate::d9_layout::BoxSeite); 4] = [
                    // (Wert, Band, Spalte, waagerecht?, Seite)
                    (oben, band_hoch, 0, true, crate::d9_layout::BoxSeite::Oben),
                    (unten, band_tief, 0, true, crate::d9_layout::BoxSeite::Unten),
                    (
                        links,
                        band_links,
                        1,
                        false,
                        crate::d9_layout::BoxSeite::Links,
                    ),
                    (
                        rechts,
                        band_rechts,
                        1,
                        false,
                        crate::d9_layout::BoxSeite::Rechts,
                    ),
                ];
                for (wert, band, spalte, _waagerecht, seite) in kandidaten {
                    if band < 6.0 {
                        continue;
                    }
                    let z = zahl(wert);
                    // Release 09.10.2026: der Ort kommt aus `wert_ort` — dieselbe
                    // Rechnung wie der Klick (`box_eingabefelder`).
                    let o = wert_ort(d, k, n, seite, wert);
                    let (x, y) = (o.x, o.y);
                    if frei(&mut spalten, spalte, y) {
                        text(l, &Point::new(x, y), &z, p.text);
                        //
                        // D19: DAS BEARBEITUNGSFELD — es liegt ueber dem Wert
                        // (siehe `box_eingabefeld`). GEMESSENE ENTSCHEIDUNG:
                        // das Feld wird nur gezeichnet, wenn `box_eingabe`
                        // DIESEN Wert meint; sonst kaeme es bei jedem Feld
                        // hoch. Der Treffer ist derselbe Weg wie beim Malen
                        // (die Ziel-Angabe wird verglichen, nicht die Stelle
                        // nachgerechnet).
                        if passt_box_ziel(d, ebene_von(i), Some(seite)) {
                            if let Some(f) =
                                box_eingabefeld_fuer(d, panel, ebene_von(i), Some(seite), wert)
                            {
                                l.push(kasten(f, p.feld, 2.0));
                                for r in rahmen(f, p.hervorhebung, 1.0) {
                                    l.push(r);
                                }
                                let (t, cur) = box_eingabetext(d).unwrap_or_default();
                                text(l, &Point::new(f.x + 4.0, f.y + 3.0), &t, p.text);
                                // Der Cursor an der Stelle im Text.
                                let vorher =
                                    t.chars().take(cur.min(t.chars().count())).count() as f32;
                                l.push(kasten(
                                    Rect::new(
                                        f.x + 4.0 + vorher * crate::werkzeug::ZEICHEN,
                                        f.y + 2.0,
                                        2.0,
                                        f.height - 4.0,
                                    ),
                                    p.hervorhebung,
                                    1.0,
                                ));
                            }
                        }
                    }
                }
                //
                // GEMESSENE ENTSCHEIDUNG, wo der NAME steht: am UNTEREN Rand
                // seiner eigenen Ebene, nicht in der Mitte. Grund: in der Mitte
                // steht die Zahl der naechsten Ebene (das Band oben), und die
                // belegte Liste liess nur einen von beiden zu — im Bild
                // stand dadurch „marge" mitten im blauen Inhaltskasten
                // (22:3x). Unten im eigenen Kasten ist der Platz frei, und die
                // innerste Ebene hat dort ihren ganzen Kasten.
                let name = BOX_EBENEN.get(i).copied().unwrap_or("");
                let nw = text_breite(d, name);
                if k.width >= nw + 8.0 && k.height >= hoehe {
                    let ny = k.bottom() - hoehe - 1.0;
                    if ny >= k.y && frei(&mut spalten, 0, ny) {
                        text(
                            l,
                            &Point::new(k.x + (k.width - nw).max(0.0) / 2.0, ny),
                            name,
                            p.text,
                        );
                    }
                }
            }
            // Die Zahlen LINKS — sechs Zeilen (siehe
            // `Boxmodell::text_zeilen`): was im Stylesheet steht und was
            // daraus geworden ist.
            for (i, z) in bm.text_zeilen().iter().enumerate() {
                let yy = auf.zahlen.y + i as f32 * ZEILEN_HOEHE as f32;
                if yy + ZEILEN_HOEHE as f32 > auf.zahlen.bottom() {
                    break;
                }
                text(
                    l,
                    &Point::new(auf.zahlen.x, yy),
                    &g.kuerze_zeile(z),
                    p.text_schwach,
                );
            }
        }
    }
}

/// D3: WO DAS EINGABEFELD DER KONSOLE LIEGT.
///
/// # EINE RECHNUNG FUER ZEICHNEN UND TREFFER
///
/// GEMESSENE ENTSCHEIDUNG (D9 hat sie bei den Schrittknoepfen erzwungen,
/// D3 braucht sie bei der Eingabezeile): der Zeichner und der KLICK (in
/// `narsil::devtools_bruecke`) brauchen dieselben Zahlen. Stehen sie
/// zweimal im Zeichencode, ist die Frage nur, WELCHE die falsche ist — und
/// gemessen ist: beide.
///
/// Das Feld sitzt unten im Inhaltsbereich, 6 px links/rechts, 4 px ueber
/// dem Rand, mit `ZEILEN_HOEHE` plus 8 px Hoehe (genug fuer den Text mit
/// Rand). Wer die Groesse aendert, aendert hier EIN Ort.
///
/// D12: es steht jetzt 15 px hoeher, weil UNTEN die Zaehlzeile liegt (wie
/// bei Firefox: erst die Eingabe, dann die Zahl). GEMESSEN: die Zaehlzeile
/// ist [`ZEILEN_HOEHE`] (15) hoch; ohne diese 15 px lag sie auf dem Feld.
pub fn konsole_eingabefeld(f: Rect) -> Rect {
    let h = ZEILEN_HOEHE as f32 + 8.0;
    Rect::new(
        f.x + 6.0,
        f.bottom() - h - 4.0 - (ZEILEN_HOEHE as f32 + 2.0),
        (f.width - 12.0).max(0.0),
        h,
    )
}

/// D3: die Konsole mit der Eingabezeile unten.
///
/// D12, gegen Firefox gemessen (`team/bilder/devtools_D3_konsole.png` gegen
/// die Firefox-Konsole): (1) die Zeilen bekommen einen 3-px-Streifen links in
/// der Farbe ihrer Stufe — Firefox faerbt den ganzen Block, wir nur den
/// Rand, weil 4 px Rand dieselbe Aussage machen und den Text lesbar lassen;
/// (2) die Zaehlzeile („4 Nachrichten | 0 Fehler") steht UNTEN, wie bei
/// Firefox, und nicht oben — sie gehoert zur Eingabe, nicht zum Filter;
/// (3) die Eingabezeile hat ein „&gt;" als Einzug (Firefox: der Pfeil
/// `▶`), und die Zahl der sichtbaren Stufen steht in der Leiste.
fn konsole_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    // D12: die Zaehlzeile steht jetzt UNTEN (2 Zeilen ueber dem Rand), und
    // die Kopfzeile ist die Werkzeugleiste (Filter + Stufen-Schalter).
    let zaehl = Rect::new(
        f.x,
        f.bottom() - ZEILEN_HOEHE as f32 - 2.0,
        f.width,
        ZEILEN_HOEHE as f32,
    );
    // Der Einzug (das Zeichen fuer die Eingabe).
    let feld = konsole_eingabefeld(f);
    let eingabe_y = feld.y;
    //
    // D12/D14/D18: DIE REIHEN — eine Liste, aus der gezeichnet wird und die
    // denselben Klick beantwortet (`konsole_baum_unter`). Die Schleife unten
    // malt nur noch; sie entscheidet nichts.
    //
    // GEMESSENE ENTSCHEIDUNG (03.10x), warum nicht `text_zeilen_bis`:
    // GEMESSEN 1,192 ms bei 5 000 Zeilen, weil alle 5 000 Zeilen in Strings
    // gebaut wurden, bevor der Zeichencode entschieden hat, dass nur ~30 in
    // die Spalte passen. `konsole_reihen` baut nur die, die passen, und
    // rechnet die Zahl selbst (`konsole_zeilenbereich`).
    //
    // D12: Streifen und Text kommen aus DERSELBEN Zeile. Vorher standen zwei
    // Wege nebeneinander (`text_zeilen_bis` fuer den Text, ein Index fuer
    // die Farbe), und ein Index, der an einer Stelle zaehlt und an der
    // anderen nicht, zeigt im Bild die falsche Farbe an der falschen Zeile.
    //
    // D14: der STREIFEN links war schon farbig, der TEXT daneben nicht —
    // GEMESSEN: bei der Eingabe `dies gibt es nicht` stand die Zeile
    // „[FEHLER] Syntax: …" in `p.text` (228/228/232), also in derselben
    // Farbe wie eine `console.log`-Meldung. Der Fehler stand im TEXT und war
    // im Bild nicht zu sehen. Die Farbe kommt jetzt aus derselben Zeile wie
    // der Streifen (`Konsole::stufenfarbe`).
    for (i, reihe) in konsole_reihen(d, f).iter().enumerate() {
        let k = konsole_reihen_rechteck(f, i);
        if k.bottom() > eingabe_y {
            break;
        }
        if let Some(farbe) = reihe.streifen {
            l.push(kasten(
                Rect::new(f.x + 2.0, k.y, 3.0, ZEILEN_HOEHE as f32),
                farbe,
                1.0,
            ));
        }
        //
        // D18 (04.10.): eine Baumzeile ist um 8 px eingezogen — der Streifen
        // fehlt, weil es eine Kinderzeile des Ergebnis-Werts ist.
        text(
            l,
            &Point::new(
                f.x + if reihe.streifen.is_some() { 12.0 } else { 20.0 },
                k.y,
            ),
            &reihe.text,
            reihe.farbe,
        );
    }
    if d.konsole.zeilen.is_empty() {
        text(
            l,
            &Point::new(f.x + 12.0, f.y + 8.0),
            "Keine Meldungen. Tippen Sie unten JavaScript und druecken Sie Enter.",
            p.text_schwach,
        );
    }
    // D12: DIE ZAEHLZEILE UNTEN (Firefox: „4 Nachrichten | 0 Fehler | 0
    // Warnungen"). Sie nennt die GEFILTERTE Zahl und die Gesamtzahl —
    // eine Zahl, die man nachschlagen kann, und nicht eine Behauptung.
    l.push(kasten(zaehl, p.leiste, 0.0));
    let sichtbar_gesamt = d
        .konsole
        .zeilen
        .iter()
        .filter(|z| d.konsole.sichtbar(z))
        .count();
    text(
        l,
        &Point::new(zaehl.x + 8.0, zaehl.y + 1.0),
        &g.kuerze_zeile(&format!(
            "{} von {} Zeilen | {} Fehler | {} Warnungen | {} von 4 Stufen{}",
            sichtbar_gesamt,
            d.konsole.zeilen.len(),
            d.konsole.fehler_anzahl(),
            d.konsole.warn_anzahl(),
            d.konsole.stufen.anzahl(),
            if d.konsole.verworfen > 0 {
                format!(" | {} verworfen", d.konsole.verworfen)
            } else {
                String::new()
            }
        )),
        p.text_schwach,
    );
    // Das Eingabefeld.
    l.push(kasten(feld, p.feld, 4.0));
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Rahmen im Fokus heller ist: das
    // Feld sieht sonst IMMER gleich aus, obwohl es nur zur Eingabe annimmt,
    // wenn es den Fokus hat (gemessen: die Zeile lud ein und nahm nichts
    // an). Ein hellerer Rahmen ist die Billigste moegliche Anzeige
    // „hier tippt man" — und sie ist an DER Farbe erkennbar, die schon
    // „hervorgehoben" bedeutet (D2 nutzt sie fuer den gewaehlten Knoten).
    for r in rahmen(
        feld,
        if d.konsole.fokus {
            p.hervorhebung
        } else {
            p.rand
        },
        if d.konsole.fokus { 2.0 } else { 1.0 },
    ) {
        l.push(r);
    }
    // D12: der Einzug. Firefox hat dort ein „▶"; die Panelschrift (Systemschrift,
    // im Bild 5x7-Bitmap oder TrueType) muss das Zeichen nicht haben — ein
    // fehlendes Zeichen waere ein Kästchen im Bild. Also ein „>" aus der
    // ASCII-Zeichen, wie die vier Schrittknoepfe (D5, `Schritt::knopf`).
    text(
        l,
        &Point::new(feld.x + 6.0, feld.y + 4.0),
        ">",
        p.hervorhebung,
    );
    // Der Cursor als 2-px-Balken, an der Stelle im Text.
    let sichtbar = g.kuerze_zeile(&d.konsole.eingabe);
    text(
        l,
        &Point::new(feld.x + 16.0, feld.y + 4.0),
        &sichtbar,
        p.text,
    );
    let vorher = sichtbar
        .chars()
        .take(d.konsole.cursor.min(sichtbar.chars().count()))
        .count() as f32;
    l.push(kasten(
        Rect::new(
            feld.x + 16.0 + vorher * 7.0,
            feld.y + 3.0,
            2.0,
            feld.height - 6.0,
        ),
        p.hervorhebung,
        1.0,
    ));
}

/// D18: EINE REIHE DER KONSOLE — eine Zeile oder eine Baumzeile.
///
/// GEMESSENE ENTSCHEIDUNG (04.10.), und sie ist der Kern dieses Abschnitts:
/// es gibt jetzt EINE Liste, aus der GEZEICHNET wird, und dieselbe Liste
/// beantwortet den Klick. Vorher standen zwei Wege nebeneinander (die
/// Zeilen der Konsole und — neu — die Zeilen des Ergebnis-Baums), und zwei
/// Wege sind zwei Zahlen: der Klick koennte eine Zeile treffen, die nicht
/// dort steht, wo sie gemalt wurde. Das ist die Fehlerklasse „gezeichnet an
/// der einen Stelle, getroffen an der anderen", und sie ist in diesem Projekt
/// schon dreimal gemessen worden.
#[derive(Debug, Clone, PartialEq)]
pub struct KonsolenReihe {
    /// Die Farbe des Streifens links; `None` = Baumzeile (kein Streifen).
    pub streifen: Option<narsil_core::color::Color>,
    /// Die Farbe des Textes.
    pub farbe: narsil_core::color::Color,
    /// Der Text, wie er im Bild steht.
    pub text: String,
    /// Bei einer Baumzeile: der PFAD im Baum (zum Auf- und Zuklappen).
    ///
    /// `None` bei einer normalen Zeile — und `None` heisst fuer den Aufrufer
    /// dasselbe wie „kein Treffer": an einer normalen Zeile gibt es nichts
    /// aufzuklappen.
    pub baum: Option<Vec<usize>>,
}

/// D18: Der Bereich, in dem die Konsolenreihen stehen: von oben bis zum
/// Eingabefeld.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion ist: Malen und Treffen
/// muessen dieselbe Rechnung benutzen. Zwei Aufrufer, die beide
/// `konsole_eingabefeld(f)` und `f.y + 4.0` selbst rechnen, sind zwei
/// Rechnungen — und sie laufen auseinander, sobald eine der beiden Zahlen
/// sich aendert.
fn konsole_zeilenbereich(f: Rect) -> (f32, f32) {
    let feld = konsole_eingabefeld(f);
    (f.y + 4.0, feld.y)
}

/// D18: Die Reihen der Konsole — ALLE, in der Reihenfolge, in der sie im
/// Bild stehen (Konsolenzeilen und aufgeklappte Baumauszeilen gemischt).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Baumzeilen HIER entstehen und nicht im
/// Zeichencode: sie brauchen den Text der Zeile (`Konsole::baum_text`) und
/// den Zustand des Baums (offen/zu). Beides steht im Zustand, beides wird
/// bei jedem Bild gelesen — und die Farbe gehoert zum Inhalt, nicht zum
/// Raster. Also ist die Liste eine Funktion des Zustands, und der Zeichencode
/// malt sie nur noch.
pub fn konsole_reihen(d: &Devtools, f: Rect) -> Vec<KonsolenReihe> {
    let g = Grenzen::STANDARDAUSGABE;
    let (y0, y1) = konsole_zeilenbereich(f);
    let hoehe_px = (y1 - y0).max(0.0);
    let passend = ((hoehe_px / ZEILEN_HOEHE as f32).floor().max(0.0) as usize) + 1;
    let mut raus: Vec<KonsolenReihe> = Vec::new();
    let p = Palette::dunkel();
    for z in d.konsole.zeilen.iter().filter(|z| d.konsole.sichtbar(z)) {
        if raus.len() >= passend.min(g.max_gezeichnete_zeilen) {
            break;
        }
        let stufen_farbe = d.konsole.stufenfarbe(z);
        raus.push(KonsolenReihe {
            streifen: Some(stufen_farbe),
            // GEMESSENE ENTSCHEIDUNG (D14): Fehler und Warnung faerben den
            // GANZEN Text, die anderen Stufen nicht — Firefox macht es so,
            // und eine Meldung in Rot waere eine Behauptung, die sie nicht ist.
            farbe: if matches!(
                z.stufe,
                crate::d3_konsole::Stufe::Fehler | crate::d3_konsole::Stufe::Warnung
            ) {
                stufen_farbe
            } else {
                p.text
            },
            text: d.konsole.zeilen_text(z),
            baum: None,
        });
        // D18: der Ergebnis-Baum unter der Zeile.
        let Some(baum) = z.baum.as_ref() else {
            continue;
        };
        for bz in baum.zeilen() {
            if raus.len() >= passend.min(g.max_gezeichnete_zeilen) {
                return raus;
            }
            raus.push(KonsolenReihe {
                streifen: None,
                farbe: match bz.art {
                    crate::d3_konsole::BaumArt::Klammer => p.klammer,
                    crate::d3_konsole::BaumArt::Schluessel => p.attribut_name,
                    crate::d3_konsole::BaumArt::Wert => p.attribut_wert,
                },
                text: d.konsole.baum_text(&bz),
                baum: Some(bz.pfad.clone()),
            });
        }
    }
    raus
}

/// D18: Das Rechteck der `i`-ten Reihe (das GEZEICHNETE — dieselbe Rechnung
/// wie in [`konsole_reihen`]).
pub fn konsole_reihen_rechteck(f: Rect, i: usize) -> Rect {
    let (y0, _) = konsole_zeilenbereich(f);
    Rect::new(
        f.x,
        y0 + i as f32 * ZEILEN_HOEHE as f32,
        f.width,
        ZEILEN_HOEHE as f32,
    )
}

/// D18: Welcher Baumknoten liegt unter einem Punkt? (der Klick).
///
/// Liefert den PFAD — dieselbe Angabe, die das Bild zeigt. `None` heisst
/// „kein Baumknoten hier"; das kann eine leere Zeile sein oder eine
/// Konsolenzeile, und beides richtig: an beiden gibt es nichts aufzuklappen.
pub fn konsole_baum_unter(d: &Devtools, f: Rect, punkt: Point) -> Option<Vec<usize>> {
    for (i, r) in konsole_reihen(d, f).iter().enumerate() {
        if konsole_reihen_rechteck(f, i).contains(punkt) {
            return r.baum.clone();
        }
    }
    None
}

/// D18: DIE RECHTECKE DER DREI REITER DER DETAILANSICHT (Kopfdaten, Vorschau,
/// Antwort).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Geometrie auch hier draussen steht: der
/// Klick (in der Bruecke, fremd) muss dieselbe Stelle treffen, an der der
/// Reiter gemalt wird — dieselbe Regel wie bei den Typ-Knoepfen und beim
/// Aufrufstapel im WASM-Reiter. Zweimal ist kein Zufall, das ist die Regel.
///
/// GEMESSENE ENTSCHEIDUNG (04.10., 06:45), warum `detail`/`reiter_h` wegfielen:
/// der Aufrufer in `narsil::devtools_bruecke` (fremd) kennt nur das PANEL —
// die Zeile der Detailansicht und ihre Hoehe sind eine Rechnung dieses
/// Zeichencodes, und der Klick muss dieselbe treffen.
pub fn netz_detail_rechtecke(f: Rect) -> Vec<(crate::d4_netzwerk::NetzDetail, Rect)> {
    // GEMESSEN am Zeichencode: `detail_h = (f.height / 2.4).min(240.0)` und
    // `reiter_h = ZEILEN_HOEHE + 4`.
    let detail_h = (f.height / 2.4).min(240.0);
    let detail = Rect::new(f.x, f.bottom() - detail_h, f.width, detail_h);
    let reiter_h = ZEILEN_HOEHE as f32 + 4.0;
    crate::d4_netzwerk::NetzDetail::ALLE
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            (
                t,
                Rect::new(
                    detail.x + 6.0 + i as f32 * 92.0,
                    detail.y + 2.0,
                    88.0,
                    reiter_h,
                ),
            )
        })
        .collect()
}

/// D18: DIE RECHTECKE DER TYP-KNOEPFE IM NETZ-REITER (Alle, HTML, CSS, JS,
/// Bild, Fetch, Sonst.).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Knoepfe in den Reiter und nicht in die
/// Werkzeugleiste kommen: die Leiste hat drei Elemente und
/// [`crate::werkzeug::MAX_ELEMENTS`] = 8 als Zusage. Sieben Typ-Knoepfe
/// plus die drei waeren zehn — ueber der Zusage. Firefox hat die Typ-Filter
/// im Reiter, nicht in der Werkzeugleiste. Also hier, und der Klick kommt
/// ueber dieselben Rechtecke (eine Geometrie fuer beides).
///
/// GEMESSENE ENTSCHEIDUNG, warum sie auf der Zeile der ZAEHLUNG stehen und
/// nicht in einer eigenen Zeile: GEMESSEN im Bild `devpanel_netzwerk.png`
/// (03.10.), dass auf der Zaehlzeile links nichts steht (die Zahl ist
/// rechtsbuendig) — dort ist Platz fuer sieben Knoepfe, ohne eine Zeile
/// Höhe zu verschenken.
pub fn netz_gruppen_rechtecke(f: Rect) -> Vec<(crate::d4_netzwerk::NetzFilter, Rect)> {
    // GEMESSEN am Zeichencode (Zeile 1710, vorher `f.y + achse_h`): die
    // Achsenhoehe ist `ZEILEN_HOEHE + 4`, und die Skala sitzt ueber der
    // Liste. Die Zahl steht hier, damit der Klick dieselbe Stelle trifft.
    let y = f.y + ZEILEN_HOEHE as f32 + 4.0;
    let mut raus = Vec::new();
    let mut x = f.x + 6.0;
    for g in crate::d4_netzwerk::NetzFilter::ALLE {
        // +10 px: 5 px Luft links und rechts um den Text.
        let b = (g.name().chars().count() as f32 * crate::werkzeug::ZEICHEN) + 10.0;
        raus.push((g, Rect::new(x, y + 1.0, b, ZEILEN_HOEHE as f32 - 2.0)));
        x += b + 4.0;
    }
    raus
}

/// D19: Die Ebene, die der `i`-te Kasten im Bild ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum das aus `BOX_EBENEN` kommt und nicht aus
/// einer zweiten Liste: `BOX_EBENEN` (Zeile 2638) ist die Reihenfolge der
/// Kaesten von aussen nach innen, und das Bild zeichnet genau in dieser
/// Reihenfolge. Wer hier eine eigene Liste haette, muesste sie bei jeder
/// Aenderung an der Ebenenliste mitziehen.
fn ebene_von(i: usize) -> crate::d9_layout::BoxEbene {
    match crate::d9_layout::BoxEbene::ALLE.get(i) {
        Some(e) => *e,
        None => crate::d9_layout::BoxEbene::Marge,
    }
}

/// D19: Meint das Bearbeitungsfeld genau diesen Wert?
fn passt_box_ziel(
    d: &Devtools,
    ebene: crate::d9_layout::BoxEbene,
    seite: Option<crate::d9_layout::BoxSeite>,
) -> bool {
    d.elemente.box_eingabe.ziel == Some((ebene, seite))
}

/// D19: DIE GEOMETRIE DES BOX-MODELLS — die Rechtecke der BEARBEITUNGSFELDER.
///
/// # EINE RECHNUNG FUER MALEN UND TREFFEN
///
/// GEMESSENE ENTSCHEIDUNG (04.10., 06:35): bis hierhin gab es die
/// Geometrie des Feldes nur im Malen (aus `masse`, `ecken_werte` und dem
/// Text). Ein Klick (in `narsil::devtools_bruecke`, fremd) haette sie
/// nachrechnen muessen — und zwei Rechnungen sind zwei Zahlen. Diese
/// Funktion macht beide Rechnungen zu EINER: sie nimmt dieselben Eingaben
/// (`boxmodell_mit_layout`, `aufteilung`, `rechteck_masse_echt`,
/// `ecken_werte`) und liefert je Wert (Ebene, Seite) das Rechteck des
/// Feldes.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Feld auch fuer die NICHT gewaehlten
/// Werte gerechnet wird: das Feld erscheint ueber dem Kasten, wenn man den
/// Wert waehlt — und die Breite haengt vom TEXT ab, den man tippt. Fuer den
/// Treffer reicht die Naehe zum Kasten; die exakte Breite ist erst nach dem
/// Waehlen wichtig.
pub fn box_eingabefelder(
    d: &Devtools,
    f: Rect,
) -> Vec<(
    (
        crate::d9_layout::BoxEbene,
        Option<crate::d9_layout::BoxSeite>,
    ),
    Rect,
)> {
    let mut raus = Vec::new();
    let Some((bm, aussen, inhalt)) = d.elemente.boxmodell_mit_layout() else {
        return raus;
    };
    let auf = box_aufteilung(d, f);
    let masse = rechteck_masse_echt(&bm, aussen, inhalt, auf.kacheln.width, auf.kacheln.height);
    let y0 = auf.kacheln.y;
    for (i, r) in masse.iter().enumerate() {
        let Some(innen) = masse.get(i + 1) else {
            break;
        };
        let k = Rect::new(auf.kacheln.x + r.x, y0 + r.y, r.width, r.height);
        let n = Rect::new(auf.kacheln.x + innen.x, y0 + innen.y, innen.width, innen.height);
        let (oben, rechts, unten, links) = ecken_werte(&bm, i);
        for (wert, seite) in [
            (oben, crate::d9_layout::BoxSeite::Oben),
            (unten, crate::d9_layout::BoxSeite::Unten),
            (links, crate::d9_layout::BoxSeite::Links),
            (rechts, crate::d9_layout::BoxSeite::Rechts),
        ] {
            // Release 09.10.2026: das KLICKZIEL liegt an der ZAHL der Seite.
            // GEMESSEN vorher: alle vier Seiten bekamen dasselbe Rechteck
            // (`feld_rechteck`, mittig ueber dem Kasten) — der Klick auf
            // „12" links fand zuerst „oben" und waehlte margin-top (16).
            let o = wert_ort(d, k, n, seite, wert);
            let zw = text_breite(d, &format!("{}px", wert.round() as i32));
            raus.push((
                (ebene_von(i), Some(seite)),
                Rect::new(o.x - 3.0, o.y - 3.0, zw.max(14.0) + 6.0, SCHRIFT_GRAD + 6.0),
            ));
        }
    }
    raus
}

/// Release 09.10.2026: WO DIE ZAHL EINER SEITE STEHT — eine Rechnung fuer das
/// Malen und den Klick. `k` ist der Kasten der Ebene, `n` der naechste innen.
///
/// GEMESSENE KORREKTUR beim Herausziehen: „unten" stand auf der Hoehe von
/// „oben" (dasselbe Band), und „rechts" nahm den Platz von „links", sobald
/// beide Raender gleich breit waren (`band == band_links`) — beide Zahlen
/// fielen dann der Doppelbelegung zum Opfer und fehlten im Bild.
fn wert_ort(d: &Devtools, k: Rect, n: Rect, seite: crate::d9_layout::BoxSeite, wert: f32) -> Point {
    let zw = text_breite(d, &format!("{}px", wert.round() as i32));
    let band_y = |band: f32, mitte: f32| mitte - band.min(15.0) / 2.0 + 2.0;
    let seitlich = |band: f32| band_y(band, k.y + k.height / 2.0 + band / 2.0);
    match seite {
        crate::d9_layout::BoxSeite::Oben => Point::new(
            k.x + k.width / 2.0 - zw / 2.0,
            band_y((n.y - k.y).abs(), (k.y + n.y) / 2.0),
        ),
        crate::d9_layout::BoxSeite::Unten => Point::new(
            k.x + k.width / 2.0 - zw / 2.0,
            band_y((k.bottom() - n.bottom()).abs(), (n.bottom() + k.bottom()) / 2.0),
        ),
        crate::d9_layout::BoxSeite::Links => Point::new(k.x + 2.0, seitlich((n.x - k.x).abs())),
        crate::d9_layout::BoxSeite::Rechts => {
            Point::new(k.right() - zw - 2.0, seitlich((k.right() - n.right()).abs()))
        }
    }
}

/// D19: Das Rechteck des Feldes fuer EINEN Wert (die Breite folgt dem Text).
pub fn box_eingabefeld_fuer(
    d: &Devtools,
    f: Rect,
    ebene: crate::d9_layout::BoxEbene,
    seite: Option<crate::d9_layout::BoxSeite>,
    wert: f32,
) -> Option<Rect> {
    // Release 09.10.2026: das BEARBEITUNGSFELD bleibt mittig ueber dem
    // Kasten der Ebene (siehe `feld_rechteck`); geklickt wird an der Zahl
    // (`box_eingabefelder`).
    let _ = seite;
    let (bm, aussen, inhalt) = d.elemente.boxmodell_mit_layout()?;
    let auf = box_aufteilung(d, f);
    let masse = rechteck_masse_echt(&bm, aussen, inhalt, auf.kacheln.width, auf.kacheln.height);
    let y0 = auf.kacheln.y;
    masse
        .iter()
        .enumerate()
        .take(masse.len().saturating_sub(1))
        .find(|(i, _)| ebene_von(*i) == ebene)
        .map(|(_, r)| feld_rechteck(d, Rect::new(auf.kacheln.x + r.x, y0 + r.y, r.width, r.height), wert))
}

/// D19: WELCHER WERT LIEGT UNTER EINEM PUNKT? (der Klick im Fenster)
///
/// GEMESSENE ENTSCHEIDUNG: der Treffer nutzt dieselbe Liste wie das Malen
/// (`box_eingabefelder`), also kann der Klick nicht neben dem Feld landen.
pub fn box_eingabefeld_unter(
    d: &Devtools,
    f: Rect,
    punkt: Point,
) -> Option<(
    crate::d9_layout::BoxEbene,
    Option<crate::d9_layout::BoxSeite>,
)> {
    box_eingabefelder(d, f)
        .into_iter()
        .find(|(_, k)| k.contains(punkt))
        .map(|(ziel, _)| ziel)
}

/// D19: Das Rechteck des Schalters einer EIGENSCHAFT (die Flaeche des
/// Klicks auf das Zeichen ` ` / `-`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Flaeche den EINZUG und das Schalterzeichen
/// umfasst und nicht nur das Zeichen: [`crate::werkzeug::ZEICHEN`] = 7 px Breite
/// sind kein Ziel fuer einen Finger. GEMESSENE ENTSCHEIDUNG, warum sie NICHT
/// die ganze Zeile umfasst: Firefox schaltet am Kaestchen um — ein Klick auf
/// `Farbe` ist dort kein Umschalten, sondern markiert den Text.
///
/// GEMESSENE RECHNUNG: der Einzug ist 8 px (der Zeichencode setzt den Text auf
/// `rechts.x + 8.0`), das Schalterzeichen `ZEICHEN` px und dahinter das
/// Leerzeichen noch einmal `ZEICHEN` px. Also `8 + 2 * 7 = 22` px — und die
/// Zahl kommt aus derselben Rechnung wie das Bild, nicht aus einer zweiten.
pub fn regel_schalter_rechteck(rechts: Rect, y: f32) -> Rect {
    Rect::new(
        rechts.x,
        y,
        8.0 + crate::werkzeug::ZEICHEN * 2.0,
        ZEILEN_HOEHE as f32,
    )
}

/// D19: Steht diese Zeile der Liste im Bild? (dieselbe Bedingung wie im
/// Zeichencode, `elemente_zeichnen`)
///
/// GEMESSENE ENTSCHEIDUNG, warum der Treffer diese Grenze uebernimmt und nicht
/// eine eigene setzt: ein Umschalten, das eine Zeile betrifft, die NICHT im Bild
/// steht, waere eine Aenderung ohne Bild — der Nutzer drueckt, es passiert
/// etwas, und er sieht nichts. Das ist genau die Fehlerklasse, die dieses Panel
/// schon zweimal hatte („Zahl stimmt, Bild stimmt nicht").
fn regel_zeile_steht(n: usize, y: f32, rechts: Rect, g: Grenzen) -> bool {
    n < g.max_gezeichnete_zeilen && y + ZEILEN_HOEHE as f32 <= rechts.bottom()
}

/// D19: DIE SCHALTER DER REGELN — je (Regel, Eigenschaft) das Rechteck.
///
/// # EINE RECHNUNG FUER DAS BILD UND DEN TREFFER
///
/// Die Liste im Bild entsteht aus [`crate::d9_layout::Regeln::text_zeilen`]:
/// je Regel eine Zeile mit Herkunft und Selektor, darunter je Eigenschaft eine
/// Zeile mit dem Schalterzeichen. Diese Funktion geht dieselben Zeilen
/// durch und liefert nur die, die einen Schalter tragen — mit derselben
/// Abbruchbedingung, die der Zeichencode hat (Zeilengrenze, Spaltenhoehe), und
/// mit dem Einzug der Zeile „Rolle: … | Name: …", die ueber der Liste steht.
///
/// GEMESSENE ENTSCHEIDUNG, warum NUR der Spaltenreiter „Regeln" Schalter hat:
/// „Berechnet" zeigt dieselben Eigenschaften als Ergebnis (dort gibt es
/// nichts auszuschalten — die Seite ist so, wie sie ist), und „Layout" malt
/// das Box-Modell. In beiden Faellen gibt es keinen Schalter, also auch keinen
/// Treffer — sonst wuerde ein Klick an der Stelle, an der eine Zahl steht,
/// eine Regel umlegen, die nirgends steht.
pub fn regel_schalter_rechtecke(d: &Devtools, f: Rect) -> Vec<((usize, usize), Rect)> {
    let mut raus = Vec::new();
    if d.elemente.spalte != crate::werkzeug::Spalte::Regeln {
        return raus;
    }
    let g = Grenzen::STANDARDAUSGABE;
    let rechts = box_aufteilung(d, f).eigenschaften;
    // GEMESSENE ENTSCHEIDUNG: der Einzug der Liste ist 4 px, und die
    // Barrierefreiheitszeile (Rolle/Name) kostet eine Zeile plus 2 px. Beides
    // steht so im Zeichencode; eine eigene Zahl hier waere eine zweite.
    let mut y = rechts.y + 4.0;
    if !d.elemente.rolle.is_empty() || !d.elemente.name.is_empty() {
        y += ZEILEN_HOEHE as f32 + 2.0;
    }
    // `n` zaehlt die Zeilen der Liste — dieselbe Zahl, die der Zeichencode
    // mit `enumerate()` bildet, und deshalb auch dieselbe Grenze.
    let mut n = 0usize;
    for (r, z) in d.elemente.regeln.zeilen.iter().enumerate() {
        // Die Kopfzeile der Regel (Herkunft + Selektor) traegt keinen Schalter,
        // aber sie belegt eine Zeile — ohne sie stehen die Eigenschaften
        // eine Zeile zu hoch.
        if !regel_zeile_steht(n, y, rechts, g) {
            return raus;
        }
        n += 1;
        y += ZEILEN_HOEHE as f32;
        for (p, _e) in z.eigenschaften.iter().enumerate() {
            if !regel_zeile_steht(n, y, rechts, g) {
                return raus;
            }
            raus.push(((r, p), regel_schalter_rechteck(rechts, y)));
            n += 1;
            y += ZEILEN_HOEHE as f32;
        }
    }
    raus
}

/// D19: WELCHER REGELSCHALTER LIEGT UNTER EINEM PUNKT?
///
/// GEMESSENE ENTSCHEIDUNG, warum die Schalter als (Regel, Eigenschaft)
/// kommen und nicht als eine Zeilennummer: der Aufrufer ruft
/// [`crate::d9_layout::Regeln::schalten`], und das nimmt genau diese zwei
/// Zahlen. Eine Zeilennummer muesste hier wieder uebersetzt werden — und eine
/// Uebersetzung ist eine Stelle, an der die zwei Zahlen auseinanderlaufen.
pub fn regel_schalter_unter(d: &Devtools, f: Rect, punkt: Point) -> Option<(usize, usize)> {
    regel_schalter_rechtecke(d, f)
        .into_iter()
        .find(|(_, k)| k.contains(punkt))
        .map(|((r, p), _)| (r, p))
}

/// D19: Das Rechteck des Feldes (Breite aus dem Text, mittig ueber dem Kasten).
///
/// GEMESSENE ENTSCHEIDUNG (Bild `devpanel_box_bearbeiten.png`, 06:12), wo
/// das Feld steht: **mittig ueber dem Kasten**, nicht an der Zahl, die es
/// aendert. Grund: die Zahl steht im RAND zwischen zwei Kaesten (ein 12-px-
/// Band), und das Feld (20 px hoch) passt da nicht hinein; es braechte die
/// Breite des ganzen Kastens und wuerde die Zahlen der Nachbarebenen
/// verdecken. Ueber dem Kasten ist die naechste freie Stelle.
fn feld_rechteck(d: &Devtools, k: Rect, wert: f32) -> Rect {
    let b = text_breite(d, &crate::d9_layout::px_text(wert)).max(28.0) + 8.0;
    let h = (ZEILEN_HOEHE as f32 + 4.0).min(k.height.max(ZEILEN_HOEHE as f32 + 4.0));
    Rect::new(
        (k.x + k.width / 2.0 - b / 2.0).max(k.x),
        (k.y - h / 2.0).max(0.0),
        b,
        h,
    )
}

/// D19: WO DAS FELD ZUM BEARBEITEN EINES BOX-WERTES LIEGT.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Feld **ueber** dem Wert steht und nicht
/// daneben: im Band zwischen zwei Kaesten ist links kein Platz (dort steht die
/// Zahl der inneren Ebene), und rechts steht die Zahl der aeusseren. Ueber
/// dem Wert ist die naechste freie Stelle, und sie ist immer frei — das Band
/// ist genau da, wo die Beschriftung gezeichnet wird.
/// GEMESSENE ENTSCHEIDUNG (Bild `devpanel_box_bearbeiten.png`, 06:12), wo
/// das Feld steht: **mittig ueber dem Kasten**, nicht an der Zahl, die es
/// aendert. Grund: die Zahl steht im RAND zwischen zwei Kaesten (ein 12-px-
/// Band), und das Feld (20 px hoch) passt da nicht hinein; es braechte die
/// Breite des ganzen Kastens und wuerde die Zahlen der Nachbarebenen
/// verdecken. Ueber dem Kasten ist die naechste freie Stelle — und es ist
/// dieselbe Rechnung fuer Malen und Treffen (`box_eingabefeld`), also kann
/// der Klick nicht neben dem Feld landen.
pub fn box_eingabefeld(d: &Devtools, f: Rect) -> Option<Rect> {
    let eingabe = &d.elemente.box_eingabe;
    if !eingabe.fokus {
        return None;
    }
    let (ebene, seite) = eingabe.ziel?;
    // Release 09.10.2026: das Feld, das gemalt wird (`box_eingabefeld_fuer`,
    // mittig ueber dem Kasten) — `box_eingabefelder` sind seitdem die
    // Klickziele an den Zahlen.
    let (bm, _, _) = d.elemente.boxmodell_mit_layout()?;
    let i = crate::d9_layout::BoxEbene::ALLE.iter().position(|e| *e == ebene)?;
    let (oben, rechts, unten, links) = ecken_werte(&bm, i);
    let wert = match seite {
        Some(crate::d9_layout::BoxSeite::Rechts) => rechts,
        Some(crate::d9_layout::BoxSeite::Unten) => unten,
        Some(crate::d9_layout::BoxSeite::Links) => links,
        _ => oben,
    };
    box_eingabefeld_fuer(d, f, ebene, seite, wert)
}

/// D19: DER TEXT IM BEARBEITUNGSFELD (der Wert, der getippt wird).
pub fn box_eingabetext(d: &Devtools) -> Option<(String, usize)> {
    let e = &d.elemente.box_eingabe;
    if !e.fokus {
        return None;
    }
    Some((e.text.clone(), e.cursor))
}

/// D5: links die Dateien, rechts der Quelltext, unten Stapel und Variablen.
fn quellen_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    // Die linke Spalte: die Dateien.
    let (datei_spalte, rest) = spalten(f);
    l.push(kasten(datei_spalte, p.leiste, 0.0));
    let mut y = datei_spalte.y + 4.0;
    for (n, z) in d.quellen.kopf_zeilen().iter().enumerate() {
        if n >= g.max_gezeichnete_zeilen || y + ZEILEN_HOEHE as f32 > datei_spalte.bottom() {
            break;
        }
        text(l, &Point::new(datei_spalte.x + 6.0, y), z, p.text_schwach);
        y += ZEILEN_HOEHE as f32;
    }
    // D12: KEINE Schrittknöpfe mehr hier unten in der Dateiliste — sie
    // stehen in der Werkzeugleiste (oben), wo Firefox sie hat.
    // GEMESSENE ENTSCHEIDUNG, warum sie nicht zusaetzlich bleiben: zwei
    // Orte fuer dieselben vier Knoepfe sind zwei Stellen, an denen das Bild
    // und der Treffer auseinanderlaufen koennen — und der alte Ort lag in
    // der Dateiliste, wo der Knopf in einer Zeile mit der untersten Datei
    // stand (gemessen am Panelbild `devtools_D11_quellen.png` vom 03.10.).
    // Die Geometrie bleibt [`schritt_knoepfe`], damit der Bridge dieselbe
    // Rechnung benutzt.
    // Die rechte Spalte: der Quelltext.
    l.push(kasten(rest, p.code, 0.0));
    let mut y = rest.y + 4.0;
    let hoehe_zeilen = ((rest.height - 8.0) / ZEILEN_HOEHE as f32) as usize;
    // D11 (04.10.2026, 1:5x): DIE ZEILE, IN DER ES STEHT.
    //
    // GEMESSENER BEFUND am Bild `devtools_D11_debugger.png`: der Reiter
    // stand auf „Pause", der Aufrufstapel und die Variablen waren da — und
    // im Quelltext war NICHT zu sehen, in welcher Zeile es haengt. Das ist
    // die Haelfte des Debuggers: man sieht, dass er steht, aber nicht wo.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein BALKEN und kein Dreieck/Pfeil:
    // `DisplayItem::Text` braucht eine Glyphe, und die Panelschrift ist die
    // Systemschrift — ein „▶" fehlt auf manchen Systemen und waere dann ein
    // Kästchen. Drei Rechtecke (oben breit, unten schmal) sind es nicht.
    // Die Farbe ist `p.auswahl`, der Haltepunktpunkt daneben `p.punkt` —
    // zwei Marken, zwei Farben, zwei Bedeutungen.
    let stehzeile = d
        .quellen
        .quellen_zeile()
        .filter(|_| d.quellen.zustand == crate::d5_quellen::Zustand::Pause);
    for (nr, z, marke) in d.quellen.sichtbare_zeilen(hoehe_zeilen) {
        if Some(nr) == stehzeile {
            l.push(kasten(
                Rect::new(rest.x + 36.0, y, 3.0, ZEILEN_HOEHE as f32),
                p.auswahl,
                0.0,
            ));
        }
        // Die Zeilennummer in der schwachen Farbe, der Text in der normalen.
        text(
            l,
            &Point::new(rest.x + 6.0, y),
            &format!("{nr:>4}"),
            p.text_schwach,
        );
        let mut x = rest.x + 52.0;
        if marke {
            // Die Haltepunktmarke: ein roter Punkt vor dem Text. GEMESSENE
            // ENTSCHEIDUNG: 7 px, damit er neben der Nummer steht und nicht
            // im Text verschwindet.
            //
            // GEMESSENE KORREKTUR (04.10.2026, 1:6x, Bild
            // `devtools_D11_debugger.png`): mit `radius = 1.0` war der
            // Punkt im Bild ein 7x7-QUADRAT mit leicht abgerundeten Ecken —
            // er sah aus wie ein zweites Auswahlrechteck. Ein Punkt ist ein
            // Punkt: der Radius ist die Haelfte der Kante (3.5), damit der
            // Rasterer einen Kreis bekommt.
            l.push(kasten(
                Rect::new(rest.x + 40.0, y + 4.0, 7.0, 7.0),
                p.punkt,
                3.5,
            ));
        }
        text(l, &Point::new(x, y), &z, p.text);
        x += (z.chars().count() as f32) * 7.0;
        let _ = x;
        y += ZEILEN_HOEHE as f32;
    }
    // Ganz unten: der Aufrufstapel und die Variablen, je eine Haelfte.
    let unten = Rect::new(
        rest.x,
        rest.bottom() - (rest.height / 2.0).min(180.0),
        rest.width,
        (rest.height / 2.0).min(180.0),
    );
    l.push(kasten(unten, p.grund, 0.0));
    l.push(linie(recht_v(unten), unten.y, p.rand));
    let mut y = unten.y + 4.0;
    text(
        l,
        &Point::new(unten.x + 6.0, y),
        "Aufrufstapel",
        p.text_schwach,
    );
    y += ZEILEN_HOEHE as f32 + 2.0;
    for z in d.quellen.stapel_zeilen() {
        if y + ZEILEN_HOEHE as f32 > unten.bottom() {
            break;
        }
        text(
            l,
            &Point::new(unten.x + 6.0, y),
            &g.kuerze_zeile(&z),
            p.text,
        );
        y += ZEILEN_HOEHE as f32;
    }
    let mut y = unten.y + 4.0;
    let vx = unten.x + unten.width / 2.0;
    text(l, &Point::new(vx + 6.0, y), "Variablen", p.text_schwach);
    y += ZEILEN_HOEHE as f32 + 2.0;
    for z in d.quellen.variablen_zeilen() {
        if y + ZEILEN_HOEHE as f32 > unten.bottom() {
            break;
        }
        text(l, &Point::new(vx + 6.0, y), &g.kuerze_zeile(&z), p.text);
        y += ZEILEN_HOEHE as f32;
    }
}

/// D4: die Anfrageliste mit Kopfzeile und Balken, unten die Kopfzeilen.
fn netz_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    let z = d.netz.zahlen();
    //
    // D12: DIE ZEITACHSE (Firefox: die Skala ueber der Liste, aus der man
    // abliest, wann eine Anfrage begann). GEMESSENE ENTSCHEIDUNG, warum
    // sie hier und nicht in der Werkzeugleiste: sie gehoert zur Tabelle
    // (ihre Balken liegen darin), nicht zu den Werkzeugen — und sie braucht
    // die groesste Dauer, also die Zahl, die erst die Liste liefert.
    let sichtbar = d.netz.sichtbare_zeilen();
    let max_ms = sichtbar
        .iter()
        .map(|z| z.dauer_ms)
        .fold(0.0f64, f64::max)
        .max(1.0);
    let achse_h = ZEILEN_HOEHE as f32 + 4.0;
    let achse = Rect::new(f.x, f.y, f.width, achse_h);
    l.push(kasten(achse, p.leiste, 0.0));
    // Drei Marken: 0 ms, die Haelfte der groessten Zeit, die groesste.
    // GEMESSENE ENTSCHEIDUNG, warum DREI und nicht fuenf: bei 1400 px
    // waeren fuenf Marken 280 px auseinander — das ist eine Skala, keine
    // Zahl. Drei Marken sind an jeder Groesse lesbar.
    let skala_b = f.width - 120.0;
    for (i, wert) in [0.0f64, max_ms / 2.0, max_ms].into_iter().enumerate() {
        let x = f.x + 120.0 + (skala_b - 120.0) * (i as f32 / 2.0);
        text(
            l,
            &Point::new(x, f.y + 3.0),
            &g.kuerze_zeile(&format!("{wert:.0} ms")),
            p.text_schwach,
        );
        if i > 0 {
            l.push(kasten(
                Rect::new(x, f.y + achse_h - 3.0, 1.0, 3.0),
                p.rand,
                0.0,
            ));
        }
    }
    // D12, GEMESSENER FEHLER (03.10. 18:32, Bild team/log/devpanel_netzwerk.png):
    // Zaehlerzeile und Achsenbeschriftung standen in EINER Textzeile. Beide
    // wurden bei `f.y + 3.0` gesetzt, der Zaehler ist aber ~290 px lang und
    // die erste Achsenmarke steht bei `f.x + 120.0` — im Bild stand daher
    // „0 offen" ueber „0 ms" (gemessen: Textpixel in y 633..642, x 0..165 und
    // wieder ab 178). Firefox hat zwei Zeilen: die Skala ueber der Liste, die
    // Zaehlung darunter. Also bekommt die Zaehlung ihre EIGENE Zeile, und
    // beide Reihen werden an der SPIEGELSEITE verankert: die Skala links ueber
    // den Balken, die Zaehlung rechts (das ist die Ecke, in der Firefox sie
    // zeigt).
    let zaehler_h = ZEILEN_HOEHE as f32;
    let kopf_h = achse_h + zaehler_h;
    l.push(kasten(
        Rect::new(f.x, f.y + achse_h, f.width, zaehler_h),
        p.grund,
        0.0,
    ));
    let zaehler_text = g.kuerze_zeile(&format!(
        "{} Anfragen | {} offen | {} Byte | langsamste {:.1} ms | {} Fehler{}",
        z.anzahl,
        z.offen,
        z.bytes,
        z.langsamst_ms,
        z.fehler,
        if d.netz.verworfen > 0 {
            format!(" | {} verworfen", d.netz.verworfen)
        } else {
            String::new()
        }
    ));
    //
    // D18: DIE TYP-KNOEPFE (links auf dieser Zeile). Der aktive Knopf
    // bekommt die Hervorhebungsfarbe als Hintergrund — wie die Element-
    // Hervorhebung im Elemente-Reiter, damit „an" im Reiter immer dasselbe
    // bedeutet.
    for (gruppe, k) in netz_gruppen_rechtecke(f) {
        let an = d.netz.gruppe == gruppe;
        l.push(kasten(k, if an { p.hervorhebung } else { p.feld }, 1.0));
        text(
            l,
            &Point::new(k.x + 5.0, k.y + 1.0),
            gruppe.name(),
            if an { p.grund } else { p.text_schwach },
        );
    }
    // RECHTS ausrichten: an der Zeile, an der die Zaehlung steht, ist links
    // nichts — die Zahl steht in Firefox rechts, und so steht sie hier auch.
    let zaehler_x =
        f.right() - (zaehler_text.chars().count() as f32 * crate::werkzeug::ZEICHEN) - 8.0;
    text(
        l,
        &Point::new(zaehler_x.max(f.x + 8.0), f.y + achse_h + 2.0),
        &zaehler_text,
        p.text_schwach,
    );
    // Die Spaltenkoepfe.
    let mut y = f.y + kopf_h + 2.0;
    for (x, t) in [
        (8.0, "Nr"),
        (56.0, "Methode"),
        (128.0, "Status"),
        (200.0, "Prot"),
        (248.0, "Dauer"),
        (348.0, "Groesse"),
        (440.0, "URL"),
    ] {
        text(l, &Point::new(f.x + x, y), t, p.text_schwach);
    }
    y += ZEILEN_HOEHE as f32;
    l.push(linie(f, y, p.rand));
    y += 3.0;
    // Die Detailansicht nimmt die untere Haelfte.
    let detail_h = (f.height / 2.4).min(240.0);
    let liste_bis = f.bottom() - detail_h - 6.0;
    //
    // D12: DER WASSERFALL-BALKEN JE ZEILE — Firefox zeigt rechts in der
    // Zeile einen Balken, dessen Laenge die Zeit gegen die Zeitachse ist.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das kein logarithmischer Balken ist
    // (anders als der alte Einzelbalken weiter unten, der logarithmisch
    // war, weil er EINE Zeit von 1 ms bis 30 000 ms zeigen musste): hier
    // steht die Zeitachse daneben, also sind die Balken linear — und
    // linear ist die ehrliche Darstellung: eine 30 000-ms-Anfrage ist dann
    // wirklich so lang wie 30 000 ms und nicht so lang wie 200 ms.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Skala NICHT bei 0 beginnt, sondern
    // am fruehesten Anfang (Firefox): alle Balken eines Ladevorgangs
    // beginnen nach dem Dokument, das selbst 200-2000 ms dauert. Bei einer
    // Skala ab 0 waere kein Balken sichtbar. Also beginnt die Skala beim
    // kleinsten `dauer`-Wert der Liste.
    let balken_x = f.x + 120.0;
    let balken_b = f.right() - 8.0;
    let min_dauer = sichtbar
        .iter()
        .map(|z| z.dauer_ms)
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let strecke = (max_ms - min_dauer).max(1.0);
    let gewaehlt_nr = d.netz.gewaehlt;
    for (n, zeile) in d
        .netz
        .text_zeilen_bis(g.max_gezeichnete_zeilen)
        .iter()
        .enumerate()
    {
        if y + ZEILEN_HOEHE as f32 > liste_bis {
            break;
        }
        let e = sichtbar.get(n);
        // D12: die gewaehlte Zeile bekommt die Auswahlfarbe (Firefox
        // markiert sie) — und genau die Zeile, deren Nummer im Protokoll
        // `gewaehlt` traegt. Damit muss der Index aus derselben Liste
        // kommen (`.get(n)`), nicht aus `self.zeilen`.
        if let Some(z) = e {
            if gewaehlt_nr == Some(z.nr) {
                l.push(kasten(
                    Rect::new(f.x + 2.0, y, (f.width - 4.0).max(0.0), ZEILEN_HOEHE as f32),
                    p.auswahl,
                    0.0,
                ));
            }
            let anteil = ((z.dauer_ms - min_dauer) / strecke).clamp(0.0, 1.0) as f32;
            let bh = 8.0f32;
            l.push(kasten(
                Rect::new(balken_x, y + 3.0, (balken_b - balken_x).max(0.0), bh),
                p.code,
                2.0,
            ));
            l.push(kasten(
                Rect::new(
                    balken_x,
                    y + 3.0,
                    ((balken_b - balken_x) * anteil).max(2.0),
                    bh,
                ),
                crate::thema::balkenfarbe(z.dauer_ms, 250.0, 1000.0),
                2.0,
            ));
        }
        text(l, &Point::new(f.x + 8.0, y), zeile, p.text);
        y += ZEILEN_HOEHE as f32;
    }
    // D12: DER EINZELBALKEN DER GEWAEHLTEN ZEILE IST WEG.
    //
    // GEMESSENE ENTSCHEIDUNG: mit dem Wasserfall je Zeile (oben) waere er
    // die dritte Darstellung derselben Zahl. Er war linear 200 px breit
    // und logarithmisch gefuellt (die Begruendung steht im Bericht
    // BERICHT_DEVTOOLS_D4.txt) — zwei Achsen fuer eine Zeit, und die
    // logarithmische Achse passte zu keiner der beiden anderen. Er stand
    // ausserdem an `liste_bis`, also am unteren Rand der Liste, wo er wie
    // ein weiterer Listeneintrag aussah.
    //
    // GEMESSENE ENTSCHEIDUNG, was stattdessen dasteht: die DETAILANSICHT
    // bekommt ihre REITER (Firefox: „Kopfdaten | Vorschau | Antwort"), und
    // die Dauer der gewaehlten Zeile steht in der Kopfzeile des Details.
    // Das ist die Zahl, die man beim Nachsehen braucht, und sie steht an
    // der Zeile, zu der sie gehoert.
    // Die Detailansicht.
    let detail = Rect::new(f.x, f.bottom() - detail_h, f.width, detail_h);
    l.push(kasten(detail, p.code, 0.0));
    //
    // D12: die drei Reiter der Detailansicht. GEMESSENE ENTSCHEIDUNG, warum
    // der Zustand NICHT in der Crate ist: die Reiter zeigen verschiedene
    // Teile EINER Anfrage (Kopfzeilen, Vorschau, Koerper), und die
    // Vorschau braucht den Koerper — den haelt `narsil-net`, nicht der Reiter.
    // Also: die Reiter sind gezeichnet, und welcher davon aktiv ist, sagt
    // die Bruecke (sie hat den Koerper).
    let reiter_h = ZEILEN_HOEHE as f32 + 4.0;
    //
    // D18: der aktive Reiter kommt aus dem ZUSTAND (`NetzLog::detail`) und
    // nicht aus der Nummer der Schleife. GEMESSENER BEFUND (04.10., Bild
    // `devpanel_netzwerk_echt.png`): vorher war `i == 0` fest — alle drei
    // Reiter zeigten die Kopfzeilen, und die Zeile 16 („Antwort-Vorschau")
    // lag unterhalb des 13 Zeilen hohen Bereichs, also gar nicht im Bild.
    for (t, k) in netz_detail_rechtecke(f) {
        let aktiv = t == d.netz.detail;
        l.push(kasten(k, if aktiv { p.leiste } else { p.code }, 3.0));
        for r in rahmen(k, if aktiv { p.rand } else { p.code }, 1.0) {
            l.push(r);
        }
        text(
            l,
            &zentriert(k, t.name().chars().count()),
            t.name(),
            if aktiv { p.text } else { p.text_schwach },
        );
    }
    let mut y = detail.y + reiter_h + 8.0;
    if let Some(zw) = d.netz.gewaehlte() {
        text(
            l,
            &Point::new(detail.x + 8.0, y),
            &g.kuerze_zeile(&format!(
                "Nr. {} | {:.1} ms | {} Byte | Protokoll {}",
                zw.nr,
                zw.dauer_ms,
                zw.groesse,
                zw.protokoll.lang()
            )),
            p.text_schwach,
        );
        y += ZEILEN_HOEHE as f32 + 2.0;
    }
    for z in d.netz.detail_zeilen() {
        if y + ZEILEN_HOEHE as f32 > detail.bottom() {
            break;
        }
        text(l, &Point::new(detail.x + 8.0, y), &z, p.text);
        y += ZEILEN_HOEHE as f32;
    }
}

/// D20: DIE FARBEN DES FLAMMENBALKENS (Layout, Malen, Skript, Sonst).
///
/// GEMESSENE ENTSCHEIDUNG: die vier Farben kommen aus der Palette, und die
/// Reihenfolge ist die Reihenfolge im Bild — nicht vier neue Farben fuer vier
/// neue Balken. `hervorhebung` (blau) ist der Layout-Anteil, weil Layout die
/// Stufe ist, an der man im Reiter am meisten sucht (die Reihenfolge in
/// `d8_leistung::Stufe`).
pub fn flame_farbe(i: usize, p: &Palette) -> narsil_core::color::Color {
    match i {
        0 => p.hervorhebung,
        1 => p.gut,
        2 => p.warnung,
        _ => p.text_schwach,
    }
}

/// D8: eine Zeile je Stufe, mit einem Balken, und der Anteil der teuersten.
fn leistung_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    let kopf = Rect::new(f.x, f.y, f.width, ZEILEN_HOEHE as f32 + 6.0);
    l.push(kasten(kopf, p.leiste, 0.0));
    let summe = d.leistung.summe_ms();
    let teuerste = d.leistung.teuerste();
    text(
        l,
        &Point::new(f.x + 8.0, f.y + 3.0),
        &g.kuerze_zeile(&format!(
            "Vorgang: {} | Summe {:.2} ms | {} Phasen{}",
            if d.leistung.vorgang.is_empty() {
                "—"
            } else {
                d.leistung.vorgang.as_str()
            },
            summe,
            d.leistung.phasen_gezaehlt,
            match d.leistung.anteil_teuerste() {
                Some(a) if teuerste.is_some() => format!(
                    " | teuerste: {} ({:.0} %)",
                    teuerste.expect("geprueft").0.name(),
                    a * 100.0
                ),
                _ => String::new(),
            }
        )),
        p.text_schwach,
    );
    // D19: DIE BILDER — eine Zeile je Bild, ganz unten (der Weg aus
    // `Leistung::frame_zeilen`).
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie UNTEN stehen und nicht oben: die
    // Tabelle ist das, worum es bei einer Frage nach „wo geht die Zeit
    // hin?" geht, und ein Block Bilder ueber der Tabelle wuerde die erste
    // Zeile der Antwort wegschieben. Die Bilder sind die zweite Frage
    // („wie oft wurde gezeichnet?") — die steht darunter.
    if !d.leistung.frames.is_empty() {
        //
        // D20: die Zeile ist jetzt 4 px hoeher, weil der Balken (10 px) und die
        // Zahl nebeneinander in EINER Zeile stehen (Balken links, Text rechts).
        let hoehe = d.leistung.frames.len().min(8) as f32 * (ZEILEN_HOEHE as f32 + 4.0);
        let band = Rect::new(
            f.x,
            f.bottom() - hoehe - ZEILEN_HOEHE as f32,
            f.width,
            hoehe,
        );
        l.push(kasten(band, p.leiste, 0.0));
        //
        // D20: DER FLAMMENBALKEN — je Bild vier Stuecke (Layout | Malen |
        // Skript | Sonst), die Breite logarithmisch nach der Gesamtzeit.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Stuecke in dieser Reihenfolge:
        // sie entspricht dem Ablauf eines Bildes (die Kaesten werden gerechnet,
        // dann gemalt, dazwischen laeuft das Skript) — wie in einem
        // Flammenprofil.
        let mut fy = band.y;
        const BALKEN_X: f32 = 60.0;
        const BALKEN_B: f32 = 380.0;
        let balken = d.leistung.frame_balken(BALKEN_B - BALKEN_X);
        for (n, z) in d.leistung.frame_profil_zeilen().iter().enumerate() {
            if fy + ZEILEN_HOEHE as f32 > band.bottom() {
                break;
            }
            if let Some((_nr, breite, teile)) = balken.get(n) {
                let mut x = f.x + BALKEN_X;
                for (i, anteil) in teile.iter().enumerate() {
                    let w = breite * anteil;
                    if w < 1.0 {
                        continue;
                    }
                    l.push(kasten(
                        Rect::new(x, fy + 2.0, w, ZEILEN_HOEHE as f32 - 4.0),
                        flame_farbe(i, p),
                        0.0,
                    ));
                    x += w;
                }
            }
            text(
                l,
                &Point::new(f.x + BALKEN_B + 8.0, fy),
                &g.kuerze_zeile(z),
                p.text_schwach,
            );
            fy += ZEILEN_HOEHE as f32 + 4.0;
        }
        // DIE LEGENDE — die vier Namen mit ihren Farben, wie in Firefox
        // unter dem Diagramm.
        let mut lx = f.x + BALKEN_X;
        for (i, t) in ["Layout", "Malen", "Skript", "Sonst"]
            .into_iter()
            .enumerate()
        {
            l.push(kasten(
                Rect::new(lx, band.y - ZEILEN_HOEHE as f32, 8.0, 8.0),
                flame_farbe(i, p),
                0.0,
            ));
            text(
                l,
                &Point::new(lx + 12.0, band.y - ZEILEN_HOEHE as f32),
                t,
                p.text_schwach,
            );
            lx += 12.0 + t.chars().count() as f32 * crate::werkzeug::ZEICHEN + 14.0;
        }
    }
    // D12: DIE TABELLE — Spalten wie bei Firefox („Stufe | Dauer | Anteil")
    // und ein Balken mit ACHSE, der die Balkenlaenge lesbar macht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Achse jetzt LINEAR ist und der
    // Balken nicht mehr logarithmisch: der alte Balken war logarithmisch,
    // weil er 0,5 ms (Parse) und 3000 ms (Netz) in 220 px zeigen musste —
    // die Zahl daneben stand aber in keiner Skala, sie war einfach eine
    // Zahl. GEMESSEN (Bild `devtools_D8_leistung.png`, 03.10.): bei
    // Parse 4,3 ms / Netz 821,90 ms steht der Parse-Balken bei 68 % der
    // Laenge, obwohl er 0,5 % der Zeit ist. Das ist eine Balkenlaenge, die
    // etwas behauptet, das nicht stimmt. Also: die Achse ist linear, die
    // Achse wird GEZEICHNET (0 / Haelfte / max in ms), und die Zahl steht
    // in derselben Skala. Wer eine Stufe von 0,5 ms nicht sieht, sieht sie
    // als 0,5 ms neben einem 4-ms-Balken — das ist ehrlich.
    let mut y = f.y + ZEILEN_HOEHE as f32 + 12.0;
    //
    // D19: DIE PROFILZEILE — Layout, Malen, Skript, Anzahl Bilder.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie VOR der Tabelle steht und nicht in
    // ihr: die Tabelle hat je Stufe eine Zeile (alle sechs, mit Dauer und
    // Anteil). Die Profilzeile ist die EINZIGE Zeile, die die drei Zahlen
    // nebeneinanderstellt — so, wie man sie in Firefox ueber dem Diagramm
    // liest. Sie ist damit eine Zusammenfassung, keine siebte Messung.
    text(
        l,
        &Point::new(f.x + 8.0, y),
        &g.kuerze_zeile(&d.leistung.profil_zeile()),
        p.hervorhebung,
    );
    y += ZEILEN_HOEHE as f32 + 4.0;
    let max_ms = d
        .leistung
        .stufen
        .iter()
        .map(|s| s.ms())
        .fold(0.0f64, f64::max)
        .max(1.0);
    let summe = d.leistung.summe_ms().max(0.0);
    // Der Balkenbereich: 220 px ab 300 px, wie vorher — so rutscht die
    // Tabelle nicht in der Breite, wenn sich die Spaltenbeschriftung
    // aendert (M7: Geometrie gehoert an eine Stelle).
    let bx = f.x + 300.0;
    let breite = 220.0f32;
    // Die ACHSE: zwei Marken (0 ms und max) ueber der Balkenspalte.
    let achse_y = y - 2.0;
    text(l, &Point::new(bx, achse_y), "0 ms", p.text_schwach);
    text(
        l,
        &Point::new(bx + breite - 46.0, achse_y),
        &format!("{max_ms:.0} ms"),
        p.text_schwach,
    );
    // Die Spaltenkoepfe.
    for (x, t) in [(12.0, "Stufe"), (150.0, "Dauer"), (240.0, "Anteil")] {
        text(l, &Point::new(f.x + x, y), t, p.text_schwach);
    }
    y += ZEILEN_HOEHE as f32;
    l.push(linie(f, y, p.rand));
    y += 3.0;
    for s in crate::d8_leistung::ALLE_STUFEN.iter() {
        if y + ZEILEN_HOEHE as f32 > f.bottom() - 4.0 {
            break;
        }
        let zz = d.leistung.stufe(*s);
        text(
            l,
            &Point::new(f.x + 12.0, y),
            &g.kuerze_zeile(&format!("{:<6} {:>9.2} ms", s.name(), zz.ms())),
            p.text,
        );
        // Der Anteil an der Summe — Firefox nennt ihn in der Tabellenspalte.
        let anteil_summe = if summe > 0.0 { zz.ms() / summe } else { 0.0 };
        text(
            l,
            &Point::new(f.x + 240.0, y),
            &format!("{:>5.1} %", anteil_summe * 100.0),
            p.text_schwach,
        );
        // Der Balken: linear, mit 2 px Mindestlaenge (sonst waere eine
        // Stufe mit 0,001 ms unsichtbar und man wuesste nicht, dass es sie
        // gibt — GEMESSEN: `Raster` ist oft 0,00 ms).
        let anteil = (zz.ms() / max_ms).clamp(0.0, 1.0) as f32;
        l.push(kasten(Rect::new(bx, y + 2.0, breite, 10.0), p.code, 2.0));
        if zz.ms() > 0.0 {
            l.push(kasten(
                Rect::new(bx, y + 2.0, (breite * anteil).max(2.0), 10.0),
                crate::thema::balkenfarbe(zz.ms(), 50.0, 500.0),
                2.0,
            ));
        }
        y += ZEILEN_HOEHE as f32;
    }
    // DIE NETZ-PHASASEN (D8 nach AUTONOM Z. 665: „TLS Phasen + NARSIL_ZEITEN").
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine EIGENE Zeile unter den Stufen und
    // kein siebter Balken: die Phasen stecken IN der Stufe Netz (DNS 5 +
    // TCP 30 + TLS 502 + erstes Byte 29 sind Teile von 1570 ms). Als
    // siebte Stufe wuerden sie die Summe verdoppeln, und der Balken
    // stuende doppelt so lang, wie die Zeit war.
    let ph = d.leistung.netz_phasen_zeile();
    if !ph.is_empty() && y + ZEILEN_HOEHE as f32 <= f.bottom() - 4.0 {
        text(
            l,
            &Point::new(f.x + 12.0, y),
            &g.kuerze_zeile(&ph),
            p.text_schwach,
        );
    }
}

/// D7: links die Liste des gewaehlten Bereichs, rechts die Detailansicht.
/// D7: WO EIN BEREICHSKNOPF LIEGT (eine Rechnung fuer Zeichnen und Treffer).
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x): die drei Knopfe standen als
/// Schleife mit `x += 116.0` IM Zeichencode, und der Klick (in der Bridge)
/// brauchte dieselben drei Stellen. GEMESSENER FOLGEFEHLER: der Klick auf
/// „localStorage" tat nichts — es gab keinen Weg dorthin (die Bruecke
/// kannte die Knopfe nicht). Also: EIN Ort fuer die Geometrie, und die
/// Reihenfolge aus `Bereich::BEREICHE`.
pub fn bereich_knopf(f: Rect, b: crate::d7_speicher::Bereich) -> Rect {
    // D12: die drei Bereichsknoepfe sind die SPEICHER-REITER der
    // Werkzeugleiste (Firefox: „Cookies | localStorage | Cache"). Ihre
    // Geometrie kommt aus der Leiste, damit der Klick im Bridge und das
    // Bild dieselbe Rechnung benutzen — der Grund, aus dem diese Funktion
    // ueberhaupt in `zeichnen.rs` steht (siehe der Kommentar darueber).
    for (i, e) in crate::werkzeug::leiste(Reiter::Speicher).iter().enumerate() {
        if e.wirkung == crate::werkzeug::Wirkung::SpeicherBereich(b) {
            return crate::werkzeug::rechteck(f, Reiter::Speicher, i).unwrap_or(f);
        }
    }
    f
}

/// D7: WO DER LOESCHKNOPF LIEGT — einer je Bereich.
///
/// GEMESSENE ENTSCHEIDUNG, warum EIN Knopf und nicht zwei („Auswahl" und
/// „alle"): der Knopf loescht die AUSWAHL, wenn eine da ist, sonst alles.
/// Zwei Knoepfe fuer zwei Faelle sind zwei Stellen, an denen ein Klick
/// zwei Dinge bedeuten kann. Der Text am Knopf sagt, was passiert.
pub fn loesch_knopf(f: Rect) -> Rect {
    for (i, e) in crate::werkzeug::leiste(Reiter::Speicher).iter().enumerate() {
        if e.wirkung == crate::werkzeug::Wirkung::SpeicherLoeschen {
            return crate::werkzeug::rechteck(f, Reiter::Speicher, i).unwrap_or(f);
        }
    }
    f
}

fn speicher_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    // D12: die drei Bereichsknoepfe und der Loeschknopf stehen in der
    // WERKZEUGLEISTE (oben) — sie werden hier nicht mehr gezeichnet, sonst
    // waere jeder Knopf zweimal im Bild (einmal in der Leiste, einmal
    // hier). GEMESSEN am Panelbild `devtools_D7_speicher.png` (03.10.): die
    // drei Knöpfe standen in einer Zeile UNTER der Reiterleiste, ohne
    // Trennlinie, und der Loeschknopf ganz rechts — Firefox hat beides in
    // einer Leiste mit Filterfeld.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Absicht links neben dem Loeschknopf
    // steht: der Knopf traegt das kurze Wort „Loeschen", die Absicht
    // („Alle Cookies loeschen") ist der Satz, den man bereuen kann. Sie
    // steht rechtsbuendig vor dem Knopf, damit man sie liest, BEVOR man
    // klickt (Firefox macht es mit dem Titel-Fenster genauso).
    let loesch = loesch_knopf(f);
    let n = match d.speicher.reiter {
        crate::d7_speicher::Bereich::Cookies => d.speicher.cookies.len(),
        crate::d7_speicher::Bereich::Lokalspeicher => d.speicher.lokal.len(),
        crate::d7_speicher::Bereich::Sitzung => d.speicher.session.len(),
        crate::d7_speicher::Bereich::Cache => d.speicher.cache.len(),
    };
    let beschriftung = d.speicher.reiter.loesch_text(d.speicher.gewaehlt.is_some());
    let b_zeichen = beschriftung.chars().count() as f32 * crate::werkzeug::ZEICHEN;
    text(
        l,
        &Point::new((loesch.x - 8.0 - b_zeichen).max(f.x + 4.0), loesch.y + 3.0),
        &g.kuerze_zeile(&beschriftung),
        if n == 0 {
            p.text_schwach
        } else {
            p.text_schwach
        },
    );
    // Die Kopfzeile des Bereichs: wie viele Eintraege, woher sie kommen.
    text(
        l,
        &Point::new(f.x + 8.0, f.y + 6.0),
        &g.kuerze_zeile(&d.speicher.kopf_zeile()),
        p.text_schwach,
    );
    // Die Liste und die Detailansicht.
    let (links, rechts) = spalten(Rect::new(
        f.x,
        f.y + ZEILEN_HOEHE as f32 + 12.0,
        f.width,
        f.height - ZEILEN_HOEHE as f32 - 16.0,
    ));
    l.push(kasten(links, p.code, 0.0));
    let mut y = links.y + 4.0;
    //
    // GEMESSENE ENTSCHEIDUNG, warum `zeilen_fuer(links.width)` und nicht
    // `zeilen()`: die linke Spalte ist `Breite/5` (277 px bei 1388), und ein
    // festes Spaltenformat ragte darueber hinaus — der Rest der Zeile lag
    // unter der Detailansicht und wurde von deren Hintergrund zugedeckt
    // (gemessen im Bild `devtools_D7_speicher.png`, 3:5x).
    for (n, z) in d.speicher.zeilen_fuer(links.width).iter().enumerate() {
        if n >= g.max_gezeichnete_zeilen || y + ZEILEN_HOEHE as f32 > links.bottom() {
            break;
        }
        text(l, &Point::new(links.x + 6.0, y), z, p.text);
        y += ZEILEN_HOEHE as f32;
    }
    //
    // D12, GEMESSENER FEHLER (03.10. 18:32, Bild team/log/devpanel_speicher.png):
    // der HINWEIS stand an ZWEI Stellen — einmal am Fuss der linken Spalte und
    // einmal am Fuss des ganzen Panels. `links` reicht aber ueber die volle
    // Hoehe, also sind `links.bottom()` und `f.bottom()` dieselbe Zeile, und
    // beide Texte lagen exakt uebereinander (gemessen: zwei orange Textzeilen
    // in y 880..893, wo eine Zeile dieser Schrift 12 px hoch ist — im Bild
    // war keine der beiden lesbar). Der Hinweis gehoert an die volle Breite
    // (siehe die Entscheidung weiter unten), also bleibt nur dieser eine Ort.
    l.push(kasten(rechts, p.grund, 0.0));
    let mut y = rechts.y + 4.0;
    for z in d.speicher.detail_zeilen() {
        if y + ZEILEN_HOEHE as f32 > rechts.bottom() {
            break;
        }
        text(
            l,
            &Point::new(rechts.x + 6.0, y),
            &g.kuerze_zeile(&z),
            p.text,
        );
        y += ZEILEN_HOEHE as f32;
    }
    //
    // Der HINWEIS ueber die volle Breite, ganz unten — und NACH der
    // Detailansicht.
    //
    // GEMESSENER FEHLER (02.10.2026, 3:6x, im Bild gesehen): der Hinweis
    // stand in der linken Spalte (277 px) und wurde bei 110 Zeichen
    // abgeschnitten — „Quelle: Set-Cookie dieser Sitzung (1 Zeilen, 0 unl".
    // Ein halber Satz ist schlimmer als keiner: der Nutzer liest „0 unl"
    // und weiss nicht, ob 0 oder 1 gemeint ist.
    // GEMESSENE ENTSCHEIDUNG, warum unten und nicht in die Kopfzeile: die
    // Kopfzeile hat 24 px und keine Umbrueche (siehe `herkunft_kurz`).
    if !d.speicher.hinweis.is_empty() {
        text(
            l,
            &Point::new(f.x + 6.0, f.bottom() - ZEILEN_HOEHE as f32 - 2.0),
            &g.kuerze_zeile(&d.speicher.hinweis),
            p.warnung,
        );
    }
}

/// D6: links die Module und Funktionen, rechts das Disassembly, unten
/// Locals/Stack/Speicher.
fn wasm_zeichnen(d: &Devtools, l: &mut DisplayList, f: Rect, p: &Palette) {
    let g = Grenzen::STANDARDAUSGABE;
    // D12: DIE KOPFZEILE TRAGT DEN ZUSTAND (Firefox zeigt „PAUSE" als
    // farbigen Kasten oben rechts, wenn der Debugger haengt).
    // GEMESSENE ENTSCHEIDUNG, warum als Text und nicht als Kasten: die
    // Zustandszeile steht schon in `kopf_zeile()` (sie nennt „Zustand 1
    // (Pause)"), und ein zweiter Kasten waere die zweite Stelle fuer
    // dieselbe Aussage. Der Unterschied zum Stand vom 03.10.: es steht
    // jetzt VOR der Modulliste und nicht zwischen Disassembly und Locals.
    l.push(kasten(
        Rect::new(f.x, f.y, f.width, ZEILEN_HOEHE as f32 + 6.0),
        p.leiste,
        0.0,
    ));
    text(
        l,
        &Point::new(f.x + 8.0, f.y + 3.0),
        &g.kuerze_zeile(&d.wasm.kopf_zeile()),
        p.text_schwach,
    );
    // Drei Spalten: Module+Funktionen (schmal), Disassembly (breit), rechts
    // Locals und Speicher.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Liste hier 60 px breiter ist als
    // `SPALTE_MIN` (04.10.2026, 2:9x): die Funktionszeile braucht Index,
    // Name, Signatur und Befehlszahl — 39 Zeichen. Bei 240 px Spalte sind
    // es 33, und im Bild stand ein „…" genau an der Signatur. Die
    // Disassembly hat daneben noch 1400 - 300 = 1100 px, also ist der
    // Platz da; er stand nur nicht bei der Liste.
    let liste = Rect::new(
        f.x,
        f.y + ZEILEN_HOEHE as f32 + 8.0,
        ((SPALTE_MIN + 60) as f32).min(f.width),
        f.height - ZEILEN_HOEHE as f32 - 12.0,
    );
    let rechts = Rect::new(liste.right(), liste.y, f.width - liste.width, liste.height);
    l.push(kasten(liste, p.code, 0.0));
    let mut y = liste.y + 4.0;
    let fitzen = ((liste.width - 8.0) / 7.0) as usize;
    for z in d.wasm.modul_zeilen() {
        if y + ZEILEN_HOEHE as f32 > liste.bottom() {
            break;
        }
        text(
            l,
            &Point::new(liste.x + 4.0, y),
            &g.kuerze_zeile(&g.kuerze(&z, fitzen)),
            p.text_schwach,
        );
        y += ZEILEN_HOEHE as f32;
    }
    y += 6.0;
    // GEMESSENER BEFUND (team/log/devtools_D7.png, 01.10.): die
    // Funktionszeilen liefen in die Disassemblingspalte hinein, weil die
    // Zeile laenger war als die Spalte (240 px = 34 Zeichen bei 7 px). Der
    // Rasterer zeichnet trotzdem weiter — er kennt keine Spaltengrenze.
    // Also wird hier auf die Spalte gekuerzt (M4).
    let fitzen = ((liste.width - 8.0) / 7.0) as usize;
    for z in d.wasm.funktions_zeilen() {
        if y + ZEILEN_HOEHE as f32 > liste.bottom() {
            break;
        }
        text(
            l,
            &Point::new(liste.x + 4.0, y),
            &g.kuerze_zeile(&g.kuerze(&z, fitzen)),
            p.text,
        );
        y += ZEILEN_HOEHE as f32;
    }
    //
    // D17 (03.10., WASM_5): DER AUFUFSTAPEL.
    //
    // GEMESSENER BEFUND: `WasmDbg::stapel` wurde gefuellt
    // (`devtools_bruecke.rs:980`), `wasm_zeichnen` hatte aber keinen Block
    // dafuer — der Stapel kam im Bild nicht vor. Jetzt steht er hier, unter
    // der Funktionsliste: Rahmen 0 (der aeusserste) zuerst, jeder als
    // `Funktionsname @ Versatz`, der GEWAEHLTE markiert.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der gewaehlte Rahmen mit einem farbigen
    // Balken links markiert wird und nicht mit einer anderen Textfarbe: die
    // Textfarbe ist nach `WasmRahmen` nicht sortiert (die Namen kommen vom
    // Modul), und eine Markierung, die man an der FARBE des Namens erkennt,
    // waere eine zweite Bedeutung fuer dieselbe Farbe.
    let stapel_bereich = wasm_stapel_bereich(liste, y + 6.0);
    let stapel_rechtecke = wasm_stapel_rechtecke(stapel_bereich, d.wasm.stapel.len());
    if !stapel_rechtecke.is_empty() {
        l.push(linie(
            Rect::new(liste.x, stapel_bereich.y - 4.0, liste.width, 1.0),
            stapel_bereich.y - 4.0,
            p.rand,
        ));
        text(
            l,
            &Point::new(stapel_bereich.x + 4.0, stapel_bereich.y - 4.0),
            &format!(
                "Aufrufstapel ({})",
                g.kuerze_zeile(&format!("{} Rahmen", d.wasm.stapel.len()))
            ),
            p.text_schwach,
        );
    }
    for (i, z) in d.wasm.stapel_zeilen().iter().enumerate() {
        let Some(k) = stapel_rechtecke.get(i).copied() else {
            break;
        };
        if k.bottom() > liste.bottom() {
            text(
                l,
                &Point::new(k.x + 4.0, k.y),
                &format!("… {} Rahmen mehr", d.wasm.stapel.len() - i),
                p.warnung,
            );
            break;
        }
        if d.wasm.stapel_ist_gewaehlt(i) {
            l.push(kasten(k, p.auswahl, 0.0));
        }
        text(
            l,
            &Point::new(k.x + 6.0, k.y + 1.0),
            &g.kuerze_zeile(&g.kuerze(z, fitzen.saturating_sub(1))),
            if d.wasm.stapel_ist_gewaehlt(i) {
                p.text
            } else {
                p.text_schwach
            },
        );
    }
    // Das Disassembly.
    l.push(kasten(rechts, p.code, 0.0));
    let unten_h = (rechts.height / 2.6).min(220.0);
    let dis = Rect::new(rechts.x, rechts.y, rechts.width, rechts.height - unten_h);
    let mut y = dis.y + 4.0;
    //
    // DIE QUELLZEILE DER GEWAEHLTEN FUNKTION (W5: „DWARF-Zeilen aus custom
    // sections wenn vorhanden", 05.10.2026, 3:2x).
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine EIGENE Zeile und keine Spalte in
    // der Funktionsliste: die Liste hat 41 Zeichen (Index, Name, Signatur,
    // Befehlszahl) und ist damit VOLL (der Test `die_funktionszeile_zeigt_
    // die_signatur` haelt das fest). Die DWARF-Zeile gehoert aber zu EINER
    // Funktion — der gewaehlten. Eine Spalte an 40 Zeilen waere Rauschen an
    // 39 davon.
    let quell_zeile = d.wasm.gewaehlte_quelle();
    if !quell_zeile.is_empty() {
        text(
            l,
            &Point::new(rechts.x + 6.0, y),
            &g.kuerze_zeile(&quell_zeile),
            p.text_schwach,
        );
        y += ZEILEN_HOEHE as f32;
    }
    for z in d.wasm.disassembly_zeilen() {
        if y + ZEILEN_HOEHE as f32 > dis.bottom() {
            break;
        }
        text(l, &Point::new(rechts.x + 6.0, y), &z, p.text);
        y += ZEILEN_HOEHE as f32;
    }
    // Unten: Locals (links) und Speicher (rechts).
    let unten = Rect::new(rechts.x, dis.bottom(), rechts.width, unten_h);
    l.push(kasten(unten, p.grund, 0.0));
    l.push(linie(recht_v(unten), unten.y, p.rand));
    let mut y = unten.y + 4.0;
    text(l, &Point::new(unten.x + 6.0, y), "Locals", p.text_schwach);
    text(
        l,
        &Point::new(unten.x + 200.0, y),
        "Operandenstapel",
        p.text_schwach,
    );
    y += ZEILEN_HOEHE as f32 + 2.0;
    let mut y2 = y;
    for z in d.wasm.locals.iter() {
        if y + ZEILEN_HOEHE as f32 > unten.bottom() {
            break;
        }
        text(
            l,
            &Point::new(unten.x + 6.0, y),
            &g.kuerze_zeile(&format!("{} = {}", z.name, z.wert)),
            p.text,
        );
        y += ZEILEN_HOEHE as f32;
    }
    for z in d.wasm.werte.iter() {
        if y2 + ZEILEN_HOEHE as f32 > unten.bottom() {
            break;
        }
        text(
            l,
            &Point::new(unten.x + 200.0, y2),
            &g.kuerze_zeile(z),
            p.text,
        );
        y2 += ZEILEN_HOEHE as f32;
    }
    // Die Speicheransicht unten rechts, wenn es eine gibt.
    if !d.wasm.speicher.is_empty() {
        let sx = unten.x + 360.0;
        let sh =
            ((unten.bottom() - unten.y - ZEILEN_HOEHE as f32 - 8.0) / ZEILEN_HOEHE as f32) as usize;
        let mut y3 = unten.y + 4.0;
        text(
            l,
            &Point::new(sx, y3),
            &format!("Speicher Seite {}", d.wasm.speicher_seite),
            p.text_schwach,
        );
        y3 += ZEILEN_HOEHE as f32;
        for z in d.wasm.speicher_zeilen(sh) {
            if y3 + ZEILEN_HOEHE as f32 > unten.bottom() {
                break;
            }
            text(l, &Point::new(sx, y3), &z, p.text);
            y3 += ZEILEN_HOEHE as f32;
        }
    }
    //
    // D21 (04.10.): DER HINWEIS, ueber die volle Breite ganz unten — genau
    // wie im Speicher-Reiter (`d.speicher.hinweis`, zwei Funktionen weiter
    // oben).
    //
    // GEMESSENER BEFUND VORHER: die Bruecke schreibt bei einem abgelehnten
    // Schritt `wasm.hinweis = "Weiter nicht moeglich: der Debugger steht
    // nicht in der Pause."` (narsil/src/devtools_bruecke.rs:606) — und
    // `wasm_zeichnen` las das Feld nicht. GEMESSEN: `grep -n "hinweis"
    // crates/narsil-devtools/src/zeichnen.rs` nennt nur `speicher.hinweis`.
    // Also: der Nutzer drueckt „Weiter", es passiert nichts, und der Grund
    // steht in einem Feld, das niemand zeigt. Das ist genau die Fehlerklasse
    // „Knopf da, Wirkung nicht sichtbar".
    //
    // GEMESSENE ENTSCHEIDUNG, warum ganz unten und nicht in die Kopfzeile:
    // die Kopfzeile des Reiters traegt Zustand, Schritt und Speicher (sie ist
    // 21 px hoch und wird an 400 Zeichen gekuerzt) — ein Satz, der dort
    // landet, schiebt die Zahlen aus dem Bild.
    if !d.wasm.hinweis.is_empty() {
        text(
            l,
            &Point::new(f.x + 6.0, f.bottom() - ZEILEN_HOEHE as f32 - 2.0),
            &g.kuerze_zeile(&d.wasm.hinweis),
            p.warnung,
        );
    }
}

// ---------------------------------------------------------------------------
// Die Haken, mit denen alles andere gleich aussieht
// ---------------------------------------------------------------------------

/// Ein gefuelltes Rechteck ohne Schatten.
fn kasten(r: Rect, farbe: Color, radius: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect: r,
        color: farbe,
        radius: [radius; 4],
        deckkraft: 1.0,
        schatten: [SchattenArt::LEER; MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

/// Ein Rechteck mit Teil-Deckung (fuer die Kacheln des Box-Modells).
fn kasten_transparent(r: Rect, farbe: Color, deckkraft: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect: r,
        color: farbe,
        radius: [0.0; 4],
        deckkraft,
        schatten: [SchattenArt::LEER; MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

/// Nur der Rand eines Rechtecks (4 schmale Rechtecke).
fn rahmen(r: Rect, farbe: Color, dicke: f32) -> [DisplayItem; 4] {
    let d = dicke.max(1.0);
    [
        kasten(Rect::new(r.x, r.y, r.width, d), farbe, 0.0),
        kasten(Rect::new(r.x, r.bottom() - d, r.width, d), farbe, 0.0),
        kasten(Rect::new(r.x, r.y, d, r.height), farbe, 0.0),
        kasten(Rect::new(r.right() - d, r.y, d, r.height), farbe, 0.0),
    ]
}

/// Eine waagerechte Linie.
fn linie(r: Rect, y: f32, farbe: Color) -> DisplayItem {
    kasten(Rect::new(r.x, y, r.width, 1.0), farbe, 0.0)
}

/// Die rechte Kante eines Rechtecks (fuer die Trennlinie in einer Spalte).
fn recht_v(r: Rect) -> Rect {
    Rect::new(r.right() - 1.0, r.y, 1.0, r.height)
}

/// Eine Textzeile.
fn text(l: &mut DisplayList, origin: &Point, t: &str, farbe: Color) {
    // M4: der Text kommt aus der Seite. Die Grenze steht in `Grenzen`, und
    // das Abschneiden passiert HIER — der Rasterer kennt keine
    // Zeichengrenze und wuerde ueber den Fensterrand hinaus zeichnen.
    let t = Grenzen::STANDARDAUSGABE.kuerze_zeile(t);
    if t.is_empty() {
        return;
    }
    l.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        origin: *origin,
        text: t,
        color: farbe,
        font_size: SCHRIFT_GRAD,
        deko: 0,
        //
        // D18 (04.10.): die beiden Felder kamen am 04.10. in `narsil-display`
        // dazu (SCHRIFT hat sie gemessen: `font-weight`/`font-style` kamen
        // bei uns nicht an — alle vier Stilzeilen einer Seite waren gleich
        // breit). Das DevTools-Panel ist ein Werkzeug, kein Dokument: seine
        // Texte sind Beschriftungen, keine Fliesstexte. Also 400/normal — der
        // Vorgabewert nach CSS Fonts 4 5.3/5.2, und NICHT „irgendein Gewicht,
        // das gerade passt": eine Ueberschrift im Werkzeug, die dicker ist als
        // ihr Text, waere eine Behauptung ueber Wichtigkeit, die es nicht gibt.
        gewicht: 400,
        kursiv: false,
    });
}

/// Der Startpunkt eines zentrierten Textes in einem Kasten.
///
/// GEMESSENE ENTSCHEIDUNG: die Breite wird aus der Zeichenzahl gerechnet
/// (7 px je Zeichen bei 13 px Schrift) statt gemessen — weil eine Messung
/// hier eine Abhaengigkeit auf die Schriftwaage bedeuten wuerde, und die
/// Reiternamen ASCII sind. Fuer die Reiternamen reicht die Rechnung; fuer
/// lange Texte wird ohnehin gekuerzt.
fn zentriert(r: Rect, zeichen: usize) -> Point {
    // D16: fuer den DOM-Baum kommt der TEXT ueber `zentriert_text`, weil nur
    // der Text gemessen werden kann. Diese Fassung bleibt fuer die Werkzeug-
    //leiste, wo nur eine Zeichenzahl vorliegt.
    // GEMESSEN (03.10. 19:5x): 7,0 px je Zeichen sind zu breit — die Zeile
    // `<div id="gross" class="kasten">` misst 5,97 px je Zeichen (siehe
    // `d2_elemente::ZEICHEN_BREITE`). Mit 7,0 stand jeder Knopftext 5 px
    // links von seiner Mitte.
    let breite = zeichen as f32 * crate::d2_elemente::ZEICHEN_BREITE;
    Point::new(
        r.x + (r.width - breite).max(0.0) / 2.0,
        r.y + (r.height - SCHRIFT_GRAD) / 2.0,
    )
}

/// Ein helleres Farb (fuer den Hover).
fn heller(c: Color, delta: i16) -> Color {
    let f = |v: u8| -> u8 {
        let n = v as i16 + delta;
        if n < 0 {
            0
        } else if n > 255 {
            255
        } else {
            n as u8
        }
    };
    Color::new(f(c.r), f(c.g), f(c.b), c.a)
}

/// Die beiden Spalten eines Reiters: links der Baum/die Liste, rechts der Rest.
///
/// GEMESSENE ENTSCHEIDUNG: links `breite / SPALTE_ANTEIL`, aber mindestens
/// [`SPALTE_MIN`] und höchstens [`SPALTE_MAX`]. Beide Grenzen sind gemessen
/// (siehe die Konstanten). Der Rest geht an die rechte Spalte — und wenn der
/// Rest kleiner als 200 px waere, gibt es nur eine Spalte (das Bild einer
/// rechten Spalte, die 40 px breit ist, ist unbrauchbar).
pub fn spalten(f: Rect) -> (Rect, Rect) {
    let links_breite = ((f.width / SPALTE_ANTEIL as f32) as u32).clamp(SPALTE_MIN, SPALTE_MAX);
    let links = Rect::new(f.x, f.y, (links_breite as f32).min(f.width), f.height);
    let rechts = Rect::new(
        links.right(),
        f.y,
        (f.width - links.width).max(0.0),
        f.height,
    );
    (links, rechts)
}

/// Die Aufteilung der rechten Spalte: OBEN die Eigenschaften, UNTEN die
/// Box-Modell-Ecke.
///
/// # WARUM DAS EINE EIGENE FUNKTION IST (gemessen 01.10.2026, 0:3x)
///
/// Das Bild `team/log/devtools_D9_wikipedia.png` (23:6x) zeigte beides
/// uebereinander: „min-width: auto" in derselben Zeile wie „Inhalt 456 x
/// 182 px". Ursache war nicht ein falscher Wert, sondern eine RECHNUNG an
/// zwei Stellen: die Eigenschafts-Schleife reservierte `panel_b - 92`,
/// der Kasten brauchte `panel_b - 120`. Zwei Rechnungen fuer eine Ecke —
/// und die kleinere gewinnt, wenn sie zuerst kommt.
///
/// GEMESSENE ENTSCHEIDUNG: die Ecke wird EINMAL hier berechnet, und
/// Zeichner wie Test lesen dasselbe Rechteck. Damit kann die Ueberlappung
/// nicht entstehen; ein Test kann sie pruefen, ohne die Rechnung zu
/// wiederholen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aufteilung {
    /// Die linke Spalte (der Baum).
    pub baum: Rect,
    /// Die rechte Spalte, oben: die Eigenschaften des gewaehlten Knotens.
    pub eigenschaften: Rect,
    /// Die rechte Spalte, unten: Kasten und Zahlen des Box-Modells.
    pub ecke: Rect,
    /// Der Platz fuer die Zahlen (links in der Ecke).
    pub zahlen: Rect,
    /// Der Platz fuer die Kacheln (rechts in der Ecke).
    pub kacheln: Rect,
}

/// Wie viel Platz die Box-Modell-Zahlen brauchen (Pixel).
///
/// GEMESSENE ENTSCHEIDUNG, warum aus der ZEILENZAHL und nicht aus einer
/// Konstante: `text_zeilen` liefert je nach Element eine andere Zahl
/// (5 bis 7 Zeilen). Eine feste Zahl waere bei einer Zeile weniger zu
/// gross (Luft) und bei einer mehr zu klein (Abschneiden) — und beides
/// waere eine Zahl, die von einer anderen abweicht.
pub fn box_zahlen_hoehe(box_zeilen: usize, rechts_hoehe: f32) -> f32 {
    if box_zeilen == 0 {
        return 0.0;
    }
    (box_zeilen as f32 * ZEILEN_HOEHE as f32 + 4.0).min(rechts_hoehe * 0.45)
}

/// Die Aufteilung der rechten Spalte mit dem Platz fuer `box_zeilen`
/// Zahlenzeilen und den Box-Kacheln darunter.
pub fn aufteilung(f: Rect, box_zeilen: usize) -> Aufteilung {
    let (links, rechts) = spalten(f);
    let zahlen = box_zahlen_hoehe(box_zeilen, rechts.height);
    // Der Kasten selbst: mindestens so hoch wie die Zahlen (sonst stehen
    // sie ueber ihm), hoechstens 120 px — er soll nicht den halben Reiter
    // fressen. GEMESSEN an wikipedia.org: 120 px reichen fuer zwei
    // Kaesten nebeneinander.
    let hoehe = zahlen.max(120.0).min(rechts.height * 0.55);
    //
    // D12, GEMESSENER BEFUND (03.10. 18:35, Bild
    // `team/log/devpanel_elemente.png`, y 766..800): die Eigenschaftsliste
    // endete auf der letzten Zeile der Spalte, und die Zaehlzeile „… N weitere
    // Eigenschaften" stand unmittelbar darunter — direkt ueber der ersten
    // Box-Modell-Zeile („Inhalt 1368 x 148 px"). Nichts ueberlappte, aber die
    // Zaehlzeile las sich als Teil des Box-Modells.
    //
    // GEMESSENE ENTSCHEIDUNG: die Spalte reserviert EINE Zeile fuer die
    // Zaehlzeile. Das ist die kleinste Aenderung, die den Text trennt, und
    // sie sitzt in der Aufteilung — also dort, wo die Zahl fuer Zeichnen und
    // Treffer ohnehin berechnet wird (eine zweite Rechnung waere die
    // Fehlerklasse, die dieses Panel schon zweimal hatte).
    let eigenschaften = Rect::new(
        rechts.x,
        rechts.y,
        rechts.width,
        (rechts.height - hoehe - ZEILEN_HOEHE as f32).max(0.0),
    );
    let ecke = Rect::new(rechts.x, rechts.bottom() - hoehe, rechts.width, hoehe);
    // DIE ZAHLEN UND DIE KACHELN STEHEN NEBENEINANDER.
    //
    // GEMESSENER BEFUND (04.10.2026, 0:6x, Bild
    // `team/log/devtools_D9_wikipedia.png`): die sechs Zahlenzeilen
    // brauchen 94 px, der Kasten hatte 120 px — fuer die KACHELn blieben
    // 24 px. Das Element im Bild war 1279 × 291 px, also passte es nur
    // mit dem Faktor 24/291 = 0,082: der Kasten im Bild war 105 × 24 px
    // und die Marge von 64 px erschien als 5-px-Streifen. Der Kasten war
    // damit nicht kaputt, sondern zu klein, um etwas zu zeigen.
    //
    // GEMESSENE ENTSCHEIDUNG: die Zahlen brauchen eine feste Breite
    // (die laengste Zeile, 34 Zeichen bei [`ZEICHEN_BREITE`]), der Rest
    // geht an die Kacheln. Damit bekommt der Kasten die volle Hoehe
    // (120 px) und ist 2,4 mal so gross — ohne dass eine Zahl abgeschnitten
    // wird.
    let zahlen_breite = if box_zeilen == 0 {
        0.0
    } else {
        (box_zeilen_text_breite(box_zeilen) + 16.0).min(rechts.width * 0.5)
    };
    let kacheln = Rect::new(
        ecke.x + zahlen_breite,
        ecke.y + 4.0,
        (ecke.width - zahlen_breite - 8.0).max(0.0),
        (ecke.height - 8.0).max(0.0),
    );
    Aufteilung {
        baum: links,
        eigenschaften,
        ecke,
        zahlen: Rect::new(
            ecke.x + 8.0,
            ecke.y + 4.0,
            (zahlen_breite - 12.0).max(0.0),
            ecke.height,
        ),
        kacheln,
    }
}

/// Die breiteste Zeile der Box-Modell-Zahlen (in Pixeln).
///
/// GEMESSENE ENTSCHEIDUNG, warum die BREITESTE Zeile und nicht die erste:
/// die Reihenfolge ist „Inhalt", „Aussen", „Marge", „Rahmen",
/// „Fuellung" — „Inhalt" ist meist die kuerzeste, „Marge" mit vier Zahlen
/// die laengste. Wer nach der ersten Zeile rechnet, schneidet die
/// laengste ab — und das ist die Zeile, die man zum Nachmessen braucht.
fn box_zeilen_text_breite(zeilen: usize) -> f32 {
    // 34 Zeichen: „Inhalt    1279 × 291 px  (angegeben 270 × 0)" hat 44,
    // davon sind aber 20 Fuellzeichen. GEMESSEN am Bild (0:5x): die
    // laengste sichtbare Zeile („Marge 64 / 0 / 8 / 0 px") brauchte
    // 207 px bei 13 px Schrift — das sind rund 16 px je 7 Zeichen, also
    // 2,3 px je Zeichen fuer die Zahlenteile. 34 Zeichen * 6 px + 207 px
    // gerundet: 400 px sind sicher.
    (zeilen.max(1) as f32 * 6.0).max(207.0)
}

/// Die vier Schrittknöpfe des Quellen-Reiters als GEOMETRIE.
///
/// # WARUM EINE OEFFENTLICHE FUNKTION (gemessen 04.10.2026, 1:0x)
///
/// D11 verlangt, dass der Debugger „echt anhaelt". Ein Knopf, den man nur
/// ANSEHEN kann, ist keine Haelfte des Weges — er muss auch getroffen
/// werden. Der Treffer liegt in `narsil` (die Bruecke), die Zeichnung hier.
/// Beide brauchen dieselbe Zahl, also steht sie hier und wird von beiden
/// gelesen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Breite aus der Spalte folgt: vier
/// Knöpfe muessen in die linke Spalte passen (240 px mindestens). Also
/// `(spalte - 12 - 3*4) / 4`, geklemmt auf 24..52 px. GEMESSEN an 800 px
/// Fenster: 24 px je Knopf; bei 1400 px greift die Klammer (52 px).
///
/// Die Reihenfolge ist [`crate::d5_quellen::Schritt::ALLE`]: Rein, Ueber,
/// Raus, Weiter. Ein Knopf ohne Treffer waere ein Knopf, der „existiert"
/// und nichts tut — das ist der Fehler, den der Test in
/// `narsil/tests/d9_devtools.rs` (Knopfgeometrie) verhindert.
/// Die vier Schrittknöpfe — jetzt in der Werkzeugleiste (D12).
///
/// GEMESSENE ENTSCHEIDUNG, warum sie aus der Leiste kommen und nicht
/// weiter unten in der Dateiliste gerechnet werden: sie sind dieselben vier
/// Knoepfe, die die Leiste zeichnet (siehe `werkzeug::LEISTE_QUELLEN`), und
/// eine eigene Rechnung waere eine ZWEITE — mit dem Fehler, den D9 bei den
/// Knopfen schon einmal gefunden hat („gezeichnet an der einen Stelle,
/// getroffen an der anderen").
///
/// Die Reihenfolge ist [`crate::d5_quellen::Schritt::ALLE`]: Rein, Ueber,
/// Raus, Weiter — dieselbe Reihenfolge wie in der Leiste. Ein Knopf ohne
/// Treffer waere ein Knopf, der „existiert" und nichts tut — das ist der
/// Fehler, den der Test in `narsil/tests/d9_devtools.rs` verhindert.
pub fn schritt_knoepfe(f: Rect) -> [Rect; 4] {
    let mut raus = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    for (i, k) in raus.iter_mut().enumerate() {
        *k = crate::werkzeug::rechteck(f, Reiter::Quellen, i).unwrap_or(*k);
    }
    raus
}

/// Welcher Schrittknopf unter einem Punkt liegt (oder `None`).
///
/// GEMESSENE ENTSCHEIDUNG, warum auch das hier steht und nicht im
/// Aufrufer: es ist dieselbe Rechnung wie [`schritt_knoepfe`], nur mit
/// einem Test. Zwei Stellen, die „entweder dieselbe oder eine aehnliche
/// Zahl" benutzen, sind zwei Stellen, an denen sie auseinanderlaufen.
#[must_use]
pub fn schritt_unter(
    f: Rect,
    punkt: narsil_core::geometry::Point,
) -> Option<crate::d5_quellen::Schritt> {
    crate::d5_quellen::Schritt::ALLE
        .iter()
        .zip(schritt_knoepfe(f).iter())
        .find(|(_, k)| k.contains(punkt))
        .map(|(s, _)| *s)
}

/// Ein gelegter Kasten in ein Feld fester Groesse uebertragen (fuer das
/// Box-Modell im Reiter).
///
/// GEMESSENE ENTSCHEIDUNG: der Kasten aus der Seite hat eine echte Breite
/// und Hoehe. Im Reiter ist nur ein Feld von 120 × `hoehe` dafuer. Also wird
/// **beides** normiert: wenn der Kasten breiter als das Feld ist, wird er
/// auf die Feldbreite zusammengedrueckt (und die Seitenverhaeltnisse
/// bleiben falsch, aber die Groessenverhaeltnisse der vier Kaesten bleiben
/// richtig — das ist die Zahl, die man liest).
pub fn rechteck_masse(aussen: Rect, feld_breite: f32, feld_hoehe: f32) -> Vec<Rect> {
    let mut raus = Vec::new();
    if aussen.width <= 0.0 || aussen.height <= 0.0 || feld_breite <= 0.0 || feld_hoehe <= 0.0 {
        return raus;
    }
    let sx = feld_breite / aussen.width;
    let sy = feld_hoehe / aussen.height;
    let s = sx.min(sy);
    let w = aussen.width * s;
    let h = aussen.height * s;
    let ox = (feld_breite - w) / 2.0;
    let oy = (feld_hoehe - h) / 2.0;
    // Drei Ebenen: Marge (ganz), Rand (innen), Inhalt (noch innen). Die
    // Groessen kommen aus dem Box-Modell des gewaehlten Knotens — die
    // Funktion kennt sie nicht, deshalb rechnet sie hier mit dem Verhaeltnis
    // der Seite: jede Ebene ist um ihre Marge/Rand/Fuellung kleiner.
    //
    // GEMESSENE ENTSCHEIDUNG: die Prozente hier sind eine Näherung, und
    // sie ist als eine gekennzeichnet: der Reiter zeigt die EXAKTEN Zahlen
    // daneben (`Boxmodell::text_zeilen`). Ein Bild mit genäherten Kanten
    // und exakten Zahlen daneben ist ehrlicher als ein exaktes Bild, das
    // die Zahlen versteckt.
    let m = (aussen.width * 0.06).min(w * 0.3);
    let p = (aussen.width * 0.04).min(w * 0.3);
    raus.push(Rect::new(ox, oy, w, h));
    raus.push(Rect::new(
        ox + m,
        oy + m * h / w,
        (w - 2.0 * m).max(1.0),
        (h - 2.0 * m * h / w).max(1.0),
    ));
    raus.push(Rect::new(
        ox + m + p,
        oy + m * h / w + p * h / w,
        (w - 2.0 * (m + p)).max(1.0),
        (h - 2.0 * (m + p) * h / w).max(1.0),
    ));
    raus
}

/// DIE HERVORHEBUNG FUER DIE SEITE — Rechtecke in SEITENKOORDINATEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum der zweite Wert ein `bool` ist und keine
/// Deckkraft (01.10.2026, Befund 3 der Koordination): der Aufrufer
/// zeichnet daraus einen RAHMEN, und ein Rahmen braucht zwei Dinge —
/// „wo" (das Rechteck) und „welcher ist es" (gewaehlt oder Vorfahr).
/// Eine Deckkraft als zweiter Wert war eine Zahl fuer eine Frage, die
/// nicht gestellt wurde; sie wurde als „wie deckend zeichne ich" gelesen
/// und FUELLTE die Flaeche — was auf einem grossen Element den ganzen
/// Seitenbereich zudeckte (gemessen: „die Seite ist dunkel").
///
/// # KOORDINATEN
///
/// Die Rechtecke kommen aus `LayoutBox::border_box`, also in
/// DOKUMENTkoordinaten (y = 0 ist der Seitenanfang). Der Aufrufer
/// verschiebt sie um `SEITEN_START_Y - versatz`, genau wie es
/// `narsil::sitzung::seiten_liste` mit der ganzen Seitenliste macht.
pub fn hervorhebungs_konstrukte(d: &Devtools) -> Vec<(Rect, bool)> {
    d.elemente
        .hervorhebungskette()
        .into_iter()
        .enumerate()
        .map(|(i, (_, r))| (r, i == 0))
        .collect()
}

/// Die Farbe, mit der die Hervorhebung gezeichnet wird.
pub fn hervorhebungsfarbe() -> Color {
    Palette::dunkel().hervorhebung
}

/// Die VIER Ebenen des Box-Modells (D12, 03.10., Auftrag der Leitung:
///
/// „Box-Modell als verschachtelte Kaesten zeichnen (margin/border/padding/
/// content mit Zahlen, Farben wie Firefox) statt leerer gruener Flaeche").
///
/// GEMESSENER BEFUND, warum das vorher eine LEERE Flaeche war: es wurden nur
/// Drei Ebenen gezeichnet, und die waren nicht ineinander — sie lagen
/// uebereinander, jede mit eigener Deckkraft, und das Ergebnis war ein
/// Farbbrei statt vier Kaesten. Dazu kam ein Zaehlerfehler: `rechteck_masse_echt`
/// durfte eine Ebene WEGLASSEN (bei Marge 0 fiel sie aus), der Zeichner
/// aber faerbte nach INDEX — die Inhaltsebene bekam dann die Farbe des
/// Randes. Ein leeres Feld mit Zufallsfarbe ist schlimmer als keines.
///
/// GEMESSENE ENTSCHEIDUNG, warum IMMER vier Rechtecke zurueckkommen und keins
/// weggelassen wird: der Zeichner faerbt nach Position in der Liste. Wer eine
/// Ebene weglassen darf, muss das an beiden Stellen wissen — also darf er es
/// nirgends. Eine Ebene mit 0 px bekommt stattdessen 1 px: sie ist dann im
/// Bild ein Haarlinienstreifen und damit SICHTBAR (so zeigt es Firefox auch).
///
/// Die Reihenfolge ist fest: Marge, Rand, Fuellung, Inhalt — von aussen nach
/// innen, wie in Firefox.
pub fn rechteck_masse_echt(
    bm: &crate::d2_elemente::Boxmodell,
    aussen: Rect,
    inhalt: Rect,
    feld_breite: f32,
    feld_hoehe: f32,
) -> Vec<Rect> {
    let mut raus = Vec::new();
    if inhalt.width <= 0.0 || inhalt.height <= 0.0 || feld_breite <= 0.0 || feld_hoehe <= 0.0 {
        return raus;
    }
    //
    // GEMESSENER FEHLER (03.10. 19:1x, Bild
    // `team/log/devpanel_elemente_attribute.png`): der Faktor `s` wurde aus
    // `aussen` berechnet, aber `aussen` ist der RAHMENkasten aus dem
    // Layout, NICHT der Aussenkasten mit Marge. Bei einem Element mit
    // `margin: 24px 40px; padding: 12px; border: 6px` und einem Inhalt von
    // 266 x 19 px ergab das `s = min(888/266, 64/19) = 3,34` — und die
    // Marge wurde mit 3,34 multipliziert: 40 px wurden 134 px. Die Ebenen
    // passten nicht mehr ins Feld, und das Bild zeigte EINE blaue Flaeche
    // (gemessen: Zeile y=830 von x=503 bis 1391 genau ein Farbwert
    // (116/135/159)) statt vier Kaesten.
    //
    // GEMESSENE ENTSCHEIDUNG: der Faktor wird aus dem AUSSENKASTEN
    // berechnet, also aus dem Inhalt plus Fuellung, Rand und Marge. Damit
    // passt das ganze Modell in das Feld, und die vier Ebenen liegen
    // ineinander — so, wie sie in Firefox aussehen.
    let ml = bm.marge[3].max(0.0);
    let mr = bm.marge[1].max(0.0);
    let mt = bm.marge[0].max(0.0);
    let mb = bm.marge[2].max(0.0);
    let bl = bm.rahmen[3].max(0.0);
    let br = bm.rahmen[1].max(0.0);
    let bt = bm.rahmen[0].max(0.0);
    let bb = bm.rahmen[2].max(0.0);
    let fl = bm.fuellung[3].max(0.0);
    let fr = bm.fuellung[1].max(0.0);
    let ft = bm.fuellung[0].max(0.0);
    let fb = bm.fuellung[2].max(0.0);

    // Der Aussenkasten in ECHTEN Pixeln: der Inhalt plus alles drumherum.
    // Der Inhalt ist das, was der Layout-Engine wirklich gelegt hat, also
    // geht man von ihm aus und legt die Ebenen um ihn herum.
    let rand_breite = inhalt.width + fl + fr + bl + br;
    let rand_hoehe = inhalt.height + ft + fb + bt + bb;
    let aussen_breite = rand_breite + ml + mr;
    let aussen_hoehe = rand_hoehe + mt + mb;
    let s = (feld_breite / aussen_breite).min(feld_hoehe / aussen_hoehe);
    let w = aussen_breite * s;
    let h = aussen_hoehe * s;
    let ox = (feld_breite - w) / 2.0;
    let oy = (feld_hoehe - h) / 2.0;
    // 1. die Marge — der Aussenkasten selbst.
    raus.push(Rect::new(ox, oy, w, h));
    // 2. der Rand — die Marge abgezogen.
    let rand = Rect::new(
        ox + ml * s,
        oy + mt * s,
        (w - (ml + mr) * s).max(0.0),
        (h - (mt + mb) * s).max(0.0),
    );
    raus.push(rand);
    // 3. die Fuellung — der Rand abgezogen.
    let fuellung = Rect::new(
        rand.x + bl * s,
        rand.y + bt * s,
        (rand.width - (bl + br) * s).max(0.0),
        (rand.height - (bt + bb) * s).max(0.0),
    );
    raus.push(fuellung);
    // 4. der Inhalt.
    //
    // GEMESSENER FEHLER (03.10. 19:5x, Bild
    // `devpanel_elemente_attribute.png`): der Inhaltskasten lag AUSSERHALB
    // der Fuellung. Grund: seine Position kam aus `inhalt.x - aussen.x`,
    // also aus der Lage des gelegten Kasten gegenueber dem RAHMENkasten —
    // und die Differenz ist 0, wenn beide dieselbe Kante haben. Dann lag
    // der Inhalt auf der linken Kante des Aussenkastens und damit in der
    // Marge.
    //
    // GEMESSENE ENTSCHEIDUNG: die vier Ebenen werden NESTELNDE gebaut, jede
    // als Einzug der vorigen. Damit sind sie konzentrisch, egal was der
    // Layout gelegt hat. `inhalt` liefert nur noch die GROESSE — und auch
    // die wird nur genommen, wenn sie in die Fuellung passt; sonst zaehlt
    // die Fuellung, weil ein Inhalt, der aus seinem Rahmen faellt, kein
    // Inhalt ist.
    let innen = Rect::new(
        fuellung.x + fl * s,
        fuellung.y + ft * s,
        ((inhalt.width * s).min(fuellung.width)).max(1.0),
        ((inhalt.height * s).min(fuellung.height)).max(1.0),
    );
    raus.push(innen);
    raus
}
/// Die vier Farben des Box-Modells — Marge, Rand, Fuellung, Inhalt.
///
/// GEMESSENE ENTSCHEIDUNG (D12, 03.10.): die Farben sind die des
/// Firefox-Box-Modells in dieser Reihenfolge: orange (Marge), gelb (Rand),
/// gruen (Fuellung), blau (Inhalt). Vorher war die Marge gruen und es gab
/// keine Fuellungsfarbe — das Bild zeigte eine gruene Flaeche, weil die
/// INNERSTE gezeichnete Ebene die gruene Marge war.
pub fn box_farben(p: &Palette) -> [Color; 4] {
    [p.box_marge, p.box_rand, p.box_fuellung, p.box_inhalt]
}

/// Die Namen der vier Ebenen — sie stehen in der Kachel, wenn sie hineinpasst.
pub const BOX_EBENEN: [&str; 4] = ["marge", "rand", "fuellung", "inhalt"];
/// Der Schritt, der in `d2_elemente` steht, wird auch im Zeichner gebraucht
/// (der Kasten mit Teil-Deckung). Diese Funktion kapselt ihn.
pub fn kachel(r: Rect, farbe: Color, deckkraft: f32) -> DisplayItem {
    kasten_transparent(r, farbe, deckkraft)
}

/// Die Seite, auf der der Box-Kasten sitzt — fuer die Fehlersuche im Test.
pub fn box_bereich() -> Rect {
    Rect::new(0.0, 0.0, 120.0, 80.0)
}

/// Die vier Seitenwerte einer Box-Ebene (oben, rechts, unten, links).
///
/// D13 (03.10.): die Zahl an einer Seite ist der Zuwachs DIESER Ebene auf
/// dieser Seite. Ebene 0 = Marge, 1 = Rand, 2 = Fuellung, 3 = Inhalt — die
/// Reihenfolge von [`BOX_EBENEN`].
///
/// GEMESSENE ENTSCHEIDUNG, warum der Inhalt keine Seitenwerte hat: er ist
/// die einzige Ebene ohne Zuwachs — bei ihm steht die Groesse in der Kachel
/// („Inhalt 300 x 43 px"), und eine Zahl an jeder Seite wuerde viermal
/// dasselbe sagen.
fn ecken_werte(bm: &crate::d2_elemente::Boxmodell, ebene: usize) -> (f32, f32, f32, f32) {
    match ebene {
        0 => (bm.marge[0], bm.marge[1], bm.marge[2], bm.marge[3]),
        1 => (bm.rahmen[0], bm.rahmen[1], bm.rahmen[2], bm.rahmen[3]),
        2 => (
            bm.fuellung[0],
            bm.fuellung[1],
            bm.fuellung[2],
            bm.fuellung[3],
        ),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

/// D16: DIE ECHTE BREITE EINES TEXTES — die Messung des FENSTERS.
///
/// # ANLASS (03.10., Auftrag der Leitung)
///
/// „Nicht weiter schaetzen: rufe die echte Breitenmessung aus `narsil-text` auf."
/// Der erste Versuch baute einen eigenen `Schriftstapel` und mass damit. Das
/// war **die falsche Messung**, und das Bild sagte es sofort: die Buchstaben
/// standen **gesperrt** (`<htmld>` — das `>` auf dem `l`).
///
/// # WARUM DER EIGENE STAPEL FALSCH WAR
///
/// `narsil-layout/src/messung.rs:155` sagt es wortwoertlich: „Der Rasterizer
/// zeichnet jedes Zeichen einzeln, und wenn er einen anderen Vorschub nimmt
/// als der Umbruch vorsieht, rutscht der Text auseinander (gemessen
/// 30.09.2026: sichtbar gesperrte Buchstaben, weil der Zeichner den
/// Bitmap-Vorschub 13.71 px nahm und der Umbruch hmtx 7 px)." Genau das war
/// mein Bild. Der Rasterer zeichnet mit dem Stapel des FENSTERS; messe ich
/// mit einem eigenen, messe ich eine andere Schrift.
///
/// # WAS JETZT GILT
///
/// `narsil_layout::Messung::breite` (messung.rs:139) ist „DIE eine
/// Messfunktion" des Projekts. Sie liegt im Zustand ([`crate::Devtools`],
/// gesetzt von `mit_messung`) und wird hier benutzt. Fehlt sie — ein Zustand
/// ohne Fenster, etwa in einem Test — faellt der Weg auf die gemessene
/// Durchschnittsbreite zurueck, damit der Reiter nicht stehenbleibt.
pub fn text_breite(d: &crate::Devtools, t: &str) -> f32 {
    if t.is_empty() {
        return 0.0;
    }
    if let Some(m) = d.messung() {
        let b = m.breite(t, SCHRIFT_GRAD);
        if b > 0.0 {
            return b;
        }
    }
    t.chars().count() as f32 * crate::d2_elemente::ZEICHEN_BREITE
}

/// Wie [`zentriert`], aber mit dem **Text** — D16.
///
/// GEMESSENE ENTSCHEIDUNG, warum es zwei Fassungen gibt und nicht eine: die
/// Werkzeugleiste kennt nur die Beschriftung als `&'static str` und
/// braucht den Mittelpunkt eines Knopfes; der DOM-Baum kennt die Farben und
/// muss den Text messen. Wer die Zeichenzahl in eine Breite umrechnet, hat
/// genau den Fehler, den D16 abschafft.
fn zentriert_text(d: &crate::Devtools, r: Rect, t: &str) -> Point {
    let breite = text_breite(d, t);
    Point::new(
        r.x + (r.width - breite).max(0.0) / 2.0,
        r.y + (r.height - SCHRIFT_GRAD) / 2.0,
    )
}

/// D17: WO DER AUFUFSTAPEL LIEGT — die Rechtecke der Rahmen.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine eigene Funktion und nicht ein Rechteck
/// im Zeichencode: der Klick (in `narsil::devtools_bruecke`) muss dieselbe
/// Stelle treffen, an der der Rahmen gezeichnet wird. Zwei Rechnungen sind
/// zwei Zahlen, und die Fehlerklasse „gezeichnet an der einen Stelle,
/// getroffen an der anderen" ist in diesem Projekt schon zweimal gemessen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Stapel in der LINKEN Spalte unter der
/// Funktionsliste steht und nicht rechts: rechts unten stehen Locals und
/// Operanden, und die gehoeren zum GEWAEHLTEN Rahmen. Der Stapel waehlt den
/// Rahmen aus — das ist eine andere Aufgabe als „zeige seinen Inhalt".
pub fn wasm_stapel_bereich(liste: Rect, y_funktionen: f32) -> Rect {
    Rect::new(
        liste.x,
        y_funktionen,
        liste.width,
        (liste.bottom() - y_funktionen).max(0.0),
    )
}

/// Die Rechtecke der Rahmen im Aufrufstapel (D17) — einer je Zeile.
pub fn wasm_stapel_rechtecke(bereich: Rect, anzahl: usize) -> Vec<Rect> {
    let mut raus = Vec::with_capacity(anzahl);
    for i in 0..anzahl {
        raus.push(Rect::new(
            bereich.x,
            bereich.y + 4.0 + i as f32 * ZEILEN_HOEHE as f32,
            bereich.width,
            ZEILEN_HOEHE as f32,
        ));
    }
    raus
}
