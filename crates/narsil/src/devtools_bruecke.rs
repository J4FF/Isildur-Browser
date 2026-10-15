//! D9 — DIE EINHÄNGUNG DER ENTWICKLERWERKZEUGE IN DEN ECHTEN BROWSER.
//!
//! # DIE FREIGABE, WORAUF SICH DIESE DATEI STÜTZT
//!
//! `team/AUTONOM.txt`, Zeile 589, wörtlich:
//!
//! > D9 Einhaengen in den echten Browser (narsil): F12 / Strg+Shift+I /
//! > Kontextmenue 'Untersuchen' oeffnen den Bereich unten, Seite oben
//! > sichtbar (Freigabe: neue Datei narsil/src/devtools_bruecke.rs +
//! > minimale Einhaengung; HTML und UI informiert ueber
//! > WUNSCH_HTML/WUNSCH_UI).
//!
//! Also: EINE neue Datei (diese) plus die kleinstmöglichen Aufrufe in
//! `sitzung.rs`, `laden.rs`, `bilder.rs` und `blaetter.rs`. Kein Umbau,
//! keine neue Wahrheit: diese Datei enthält keine Entscheidung über
//! Farben, Geometrie oder Reiternamen — die stehen in `narsil-devtools`
//! (d1_geruest.rs, thema.rs, zeichnen.rs).
//!
//! # WELCHE ENTSCHEIDUNGEN HIER STEHEN UND WELCHE NICHT
//!
//! | Frage | Wo die Antwort steht |
//! |---|---|
//! | Wie hoch ist der Bereich? | `narsil_devtools::Geruest::bereichs_hoehe` |
//! | Welche Reiter gibt es? | `narsil_devtools::GERUEST` (7) |
//! | Wie sieht ein Reiter aus? | `narsil_devtools::zeichnen::geruest_zeichnen` |
//! | Was macht F12? | `narsil_devtools::Geruest::taste_wirkt` |
//! | Welcher Knoten ist unter dem Zeiger? | `narsil_devtools::Elemente::knoten_unter` |
//! | Wie heisst die Reiterzeile? | `narsil_devtools::d1_geruest::Reiter::name` |
//! | **Wohin kommt der Zustand?** | **hier** (`Sitzung::devtools`) |
//! | **Wann wird gefuellt?** | **hier** (`an_seite`, `netz_*`) |
//! | **Wo liegt der Klick?** | **hier** (`mausklick_zuerst`) |
//!
//! GEMESSENE BEGRÜNDUNG für den letzten Punkt: die Reihenfolge im Klick
//! ist die Fehlerklasse „zwei Stellen entscheiden". `Sitzung::mausklick`
//! entscheidet heute über genau zwei Flächen (Leiste oben, Seite unten),
//! und es entscheidet sie an EINER Stelle (`sitzung.rs:1540`). Der
//! Werkzeugbereich liegt UNTEN, also wird er VOR dieser Zweiteilung
//! geprüft — sonst hätte die Seite ihn geschluckt, weil ein Punkt am
//! unteren Rand „in der Seite" ist.
//!
//! # DIE DREI WEGE, UND WAS JEDER KOSTET
//!
//! 1. **F12** — eine Zeile in `Sitzung::taste`, VOR dem `match k.key`.
//!    GEMESSEN: heute landet F12 in `_ => {}` (sitzung.rs:1139), wird
//!    also ignoriert. Ein `Key::F(n)`-Zweig existiert in `narsil/src`
//!    überhaupt nicht (`grep "Key::F("` = 0 Treffer außer im Test).
//! 2. **Strg+Shift+I** — dieselbe Stelle. GEMESSEN und SCHLIMMER: heute
//!    kommt `Key::Char('i')` bei gesetztem Strg+Shift in `self.tippe(c)`
//!    (sitzung.rs:1091) — die Taste leert also die **Adressleiste** und
//!    schreibt „i" hinein. Das ist kein Kosmetikfehler, das ist ein
//!    Fehler in der Bedienung; die Abfangstelle steht darum VOR dem
//!    `match`, nicht darin.
//! 3. **Kontextmenü „Untersuchen"** — ein Eintrag in `menue_eintraege`
//!    und ein Arm in `menue_klick`. GEMESSEN: das Rechtsklick-Menü gibt
//!    es (UI4, 03.10.2026), es hat aber nur „Link oeffnen" und „Adresse
//!    kopieren" (sitzung.rs:755) — „Untersuchen" fehlt.
//!
//! # SEITE OBEN SICHTBAR (die dritte Bedingung des Auftrags)
//!
//! GEMESSENE ENTSCHEIDUNG, wo das passiert: `Sitzung::sicht_hoehe()`
//! (sitzung.rs:589) ist die EINZIGE Zahl, die sagt, wie viele Pixel die
//! Seite benutzen darf. Sie geht an den Umbruch (sitzung.rs:2222,
//! `let viewport = Size::new(breite, sicht_hoehe)`), ans Scrollen
//! (`max_versatz`), ans Bild (`ist_seitenflaeche`) und ans Cursorkasten.
//! Also wird dort die Werkzeughöhe abgezogen — EINMAL, an EINER Stelle.
//! Wer sie an drei Stellen abziehen würde, hätte drei Zahlen, die
//! auseinanderlaufen; das ist genau der Grund, an dem der Abzug der
//! Statuszeile (UI1, 03.10. 10:45) previously dreimal stand.

use crate::Sitzung;
use narsil_core::geometry::{Point, Rect};
use narsil_devtools::d1_geruest::Taste;
use narsil_devtools::{Devtools, Reiter};

// ---------------------------------------------------------------------------
// A — Der Zustand und sein Fuellen
// ---------------------------------------------------------------------------

/// Der Zustand fuer eine neue Sitzung.
///
/// GEMESSENE ENTSCHEIDUNG: `Devtools::neu(breite, hoehe)` bekommt
/// dieselben Fenstermasse wie die Sitzung. Sie sind nicht dieselbe Zahl,
/// aber sie muessen am Anfang gleich sein — sonst haette der Werkzeug-
/// bereich eine Breite, die es im Fenster nicht gibt.
pub fn neu(breite: u32, hoehe: u32) -> Devtools {
    Devtools::neu(breite, hoehe)
}

/// Die aktuelle Seite in die Werkzeuge uebernehmen (D2, D3, D5, D8).
///
/// GEMESSENE ENTSCHEIDUNG, warum das EINE Funktion ist und nicht vier an
/// vier Stellen: die drei Reiter brauchen dieselben zwei Quellen
/// (`Angezeigt::seite` und `Angezeigt::baum`), und ein Aufruf, der sie
/// vergisst, zeigt einen leeren Reiter neben einem gefuellten — der
/// Fehler, den niemand sieht, weil die andere Haelfte stimmt.
pub fn an_seite(s: &mut Sitzung) {
    let (breite, sicht) = (s.breite, s.sicht_hoehe());
    let Some(a) = s.angezeigt.as_ref() else {
        // Keine Seite: die Werkzeuge zeigen „nichts", aber sie vergessen
        // die alte Seite nicht erst beim Oeffnen (dann wuerde der Nutzer
        // die Elemente der vorigen Seite sehen).
        s.devtools.seite_verlassen();
        return;
    };
    // D2: Baum, Kaskade, gelegte Kaesten.
    s.devtools.elemente.an_seite(&a.seite);
    s.devtools.elemente.mit_ansicht(breite, sicht);
    s.devtools.elemente.mit_layout(&a.baum);
    // D3: die Konsolenausgaben der Seite. GEMESSEN: `Angezeigt::konsole`
    // ist die Liste aus `Engine::konsole()` (sitzung.rs:177), also genau
    // das, was `Konsole::aus_engine` erwartet — nur ohne Engine.
    //
    // GEMESSENER FEHLER (02.10.2026, 3:3x), gefunden beim Messen von D3:
    // hier stand eine Schleife ueber ALLE Zeilen ohne Pruefung, ob sie
    // schon drin sind. Folge nach dreimal F12 (auf, zu, auf): jede
    // Meldung stand DREIMAL im Reiter. Jetzt uebernimmt
    // `seite_uebernehmen` nur die NEUEN Zeilen — und es haelt die Zeilen
    // einer Konsoleneingabe, die nicht von der Seite kommen.
    s.devtools.konsole.seite_uebernehmen(&a.konsole);
    // D5: die Skripte der Seite (echte Quelldateien mit Zeilennummern).
    s.devtools.quellen.an_seite(&a.seite);
    // D8: der Vorgang heisst nach der Adresse.
    // GEMESSENE ENTSCHEIDUNG (D9 fand es): die Adresse der SEITE, nicht die
    // des Verlaufs. `Browser::adresse()` ist leer, wenn die Seite nicht
    // ueber `gehe_zu` geladen wurde (im Test: `s.angezeigt = Some(..)`),
    // und dann hiesse der Vorgang leer — obwohl eine Seite da ist.
    let adresse = match a.seite.basis.clone() {
        b if b.is_empty() => s.browser.adresse().to_string(),
        b => b,
    };
    //
    // GEMESSENE AENDERUNG (02.10.2026, 2:1x): `vorgang_benennen` statt
    // `vorgang_beginnen`. `an_seite` laeuft nach dem Fuellen der
    // Stufen, und `vorgang_beginnen` loescht sie — im Abnahmebild standen
    // danach sechsmal 0,00 ms, obwohl die Sammlung voll war (parse 5,0,
    // kaskade 36,2, layout 9,4, js 171,9 ms). Das Zuruecksetzen gehoert an
    // den ANFANG eines Ladevorgangs (`Sitzung::lade_jetzt`), nicht ans
    // Ende. Siehe `Leistung::vorgang_benennen`.
    s.devtools.leistung.vorgang_benennen(&adresse);
    //
    // D8 nach AUTONOM Z. 665 („Leistung-Reiter mit echten Stufenzeiten
    // (TLS Phasen + NARSIL_ZEITEN)"): die Phasen des LETZTEN Abrufs mitnehmen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der LETZTE und nicht der langsamste:
    // die Stufe „Netz" ist der Abruf, der oben in der Liste steht — die
    // Hauptanfrage. Die Phasen eines Bildes oder eines Skripts sind eine
    // andere Abfrage; sie stehen im Netzwerkreiter, wo sie hingehören.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Phasen NICHT in die Summe kommen:
    // sie stecken IN der Stufe Netz (GEMESSEN: DNS 5 + TCP 30 + TLS 502 +
    // erstes Byte 29 von 1570 ms). Als siebte Stufe wuerde die Summe
    // doppelt zaehlen und der Balken doppelt lang stehen.
    if let Some(z) = s.devtools.netz.zeilen.last() {
        s.devtools.leistung.merke_netz_phasen(z.phasen.clone());
    }
    // D7: Cookies und localStorage sind von der HERKUNFT abhaengig, und
    // eine neue Seite kann eine andere Herkunft haben. Also gehoeren sie
    // nicht zum Zustand der vorigen Seite.
    //
    // GEMESSENER FEHLER, den D9 fand (01.10.2026, 21:3x): hier stand
    // AUCH `netz.leeren()`. Das leerte das Protokoll NACH dem Laden — und
    // `an_seite` wird am ENDE von `lade_jetzt` gerufen, also nach dem
    // Aufzeichnen der Anfrage. GEMESSEN in der Abnahme: „Anfragen 0"
    // bei einer Seite, die 2602 ms geladen hat. Das Netz-Protokoll wird
    // deshalb am ANFANG eines Ladevorgangs geleert (in `lade_jetzt`, eine
    // Zeile), wo es hingehört: vor der ersten Anfrage dieser Seite.
    s.devtools.speicher.zuruecksetzen();
    //
    // D7: DIE ECHTEN COOKIES, danach (02.10.2026, 3:2x).
    //
    // GEMESSENER ZUSTAND BIS HIER: `Speicher` wurde hier `zuruecksetzen()`
    // und danach NIE gefuellt — im Browser stand im Speicher-Reiter immer
    // „0 | 0 | 0". GEMESSEN am Code: `cookies_lesen` (mit
    // `narsil_browser::CookieSpeicher`) hatte keinen Aufrufer, und der Browser
    // BESITZT keinen Speicher (`Browser` in `narsil-browser` hat kein Feld
    // dafuer, `Sitzung` erzeugt keinen).
    //
    // Also kommt das, was es wirklich gibt: die `Set-Cookie`-Kopfzeilen der
    // Antworten aus dem Netz-Protokoll (D10). Das sind echte Cookies einer
    // echten Seite (wikipedia: 5 Stueck um 3:2x) — und der Reiter sagt
    // oben, dass er sie NICHT gespeichert haelt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum NACH dem `zuruecksetzen()`: davor
    // wuerde das Zuruecksetzen die Zeilen wieder loeschen. Dieselbe
    // Fehlerklasse wie beim Netz-Protokoll (D10) und bei der Leistung (D8):
    // der Aufrufer, der zuruecksetzt, steht nach dem Fuellen.
    let set_cookies = s.devtools.netz.set_cookie_zeilen();
    s.devtools
        .speicher
        .cookies_aus_set_cookie(&set_cookies, &adresse);
}

/// Die Werkzeuge an die Fenstermasse anpassen (nach `resize`).
///
/// GEMESSENE ENTSCHEIDUNG, warum hier und nicht im Zeichenen: die
/// Fenstermasse stehen in `Sitzung::breite`/`hoehe`, und die
/// Werkzeuggeometrie rechnet aus IHREN. Wenn die nicht mitwandern,
/// liegt der Bereich nach einem Fenstergroessenwechsel an der alten
/// Stelle — und der Nutzer klickt ins Leere.
pub fn bei_fensterwechsel(s: &mut Sitzung) {
    let (b, h) = (s.breite, s.hoehe);
    s.devtools.geruest.fenster_breite = b;
    s.devtools.geruest.fenster_hoehe = h;
    s.schmutzig = true;
}

// ---------------------------------------------------------------------------
// B — Die drei Wege
// ---------------------------------------------------------------------------

/// F12 / Strg+Shift+I / Menü: der EINE Aufruf.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge im Aufrufer die Rolle
/// bestimmt und nicht die Reihenfolge hier: `Taste::oeffnet()` sagt, ob
/// ein Weg schaltet oder setzt. F12 und Strg+Shift+I schalten (der
/// Nutzer tippt sie im Wechsel), das Kontextmenü setzt (der Nutzer hat
/// es eben geöffnet und will es offen haben). `taste_wirkt` entscheidet
/// das an EINER Stelle — mein `d1_geruest.rs` hat dafuer einen Test.
pub fn taste(s: &mut Sitzung, t: Taste) -> bool {
    let alt = s.devtools.geruest.taste_wirkt(t);
    neu_legen(s);
    // GEMESSENE ENTSCHEIDUNG, warum beim Oeffnen die Seite neu gefuellt
    // wird: der Nutzer drueckt F12 nicht, um eine leere Liste zu sehen,
    // sondern um die Seite zu untersuchen. Und es ist billig: es sind
    // drei Aufrufe auf Daten, die schon da sind.
    if s.devtools.offen() {
        an_seite(s);
    }
    s.schmutzig = true;
    alt
}

/// Das Menü hat „Untersuchen" gewählt: auf, nicht um.
///
/// GEMESSENE ENTSCHEIDUNG: das ist ein eigener Aufruf und nicht
/// `taste(Untersuchen)`. `Taste::Untersuchen` SCHALTET (der Nutzer hat
/// es aus einem Menü geklickt, er will es offen sehen). Der Unterschied
/// ist eine Zeile, aber eine sichtbare: ein umschaltendes „Untersuchen"
/// schliesst die Werkzeuge, wenn sie offen sind, und der Nutzer wundert
/// sich, warum sein Klick nichts getan hat.
pub fn aus_menue(s: &mut Sitzung) {
    s.devtools.oeffnen();
    neu_legen(s);
    an_seite(s);
    s.schmutzig = true;
}

/// D3: EINE TASTE AN DIE KONSOLENEINGABE — ODER AN KEINEN.
///
/// Liefert `true`, wenn die Konsole die Taste genommen hat. Der Aufrufer
/// (`Sitzung::ereignis`) gibt sie dann NICHT weiter — sonst kaeme jedes
/// Zeichen doppelt an: einmal in der Eingabezeile und einmal auf der Seite.
///
/// # GEMESSENE ENTSCHEIDUNG, WARUM DIE BRUECKE UND NICHT `Sitzung`
///
/// `Sitzung::ereignis` ist die einzige Stelle, die Tasten verteilt (Seite,
/// Adressleiste, Suche, F12). Jede Taste, die die Konsole annehmen soll,
/// muss dort entschieden werden — und diese Entscheidung gehoert zur
/// Werkzeug-Bruecke, nicht in den Ladeweg der Sitzung.
///
/// GEMESSENE ENTSCHEIDUNG, warum nach Enter KEIN `an_seite` laeuft: das
/// fuellt die Konsole aus `Angezeigt::konsole` neu und LEERT vorher die
/// Zeilen (`Konsole::leeren`) — das Ergebnis der gerade ausgefuehrten
/// Eingabe waere im Reiter wieder weg. Dieselbe Fehlerklasse wie beim
/// Netz-Protokoll in D10: der Aufrufer, der zuruecksetzt, steht nach dem
/// Fuellen.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein Neuzeichnen des Seitenbaus: die
/// Konsoleneingabe laeuft in einem EIGENEN Motor (mit `Motor::bombe()`, siehe
/// `Konsole::fuehre_aus`), also aendert sich an der Seite nichts — und ein
/// `neu_legen` wuerde Kaskade und Layout wiederholen (GEMESSEN 30 ms und
/// 8 ms an wikipedia.org, also 38 ms fuer eine Konsoleneingabe, die die
/// Seite nicht einmal anfasst).
pub fn eingabe_nimmt(s: &mut Sitzung, d: narsil_devtools::d3_konsole::Druck) -> bool {
    use narsil_devtools::d3_konsole::Druck;
    // D12: DAS FILTERFELD (Konsole und Netzwerk) ist eine EINGABE wie die
    // Konsolezeile — es nimmt dieselben Tasten an, nur an einem anderen Ort.
    // GEMESSENE ENTSCHEIDUNG, warum es eine eigene Abfrage ist und kein
    // weiteres Feld in `Konsole`: der Netzwerkreiter hat keine Konsole.
    // Wer beide in eine Structur legt, hat sie an zwei Orten.
    if d == Druck::Sonst {
        return false;
    }
    if filter_nimmt(s, d) {
        return true;
    }
    // D3: DREI BEDINGUNGEN, und jede davon wird einzeln gebraucht.
    if !s.devtools.offen() || s.devtools.reiter() != Reiter::Konsole {
        return false;
    }
    if !s.devtools.konsole.fokus {
        return false;
    }
    // `Druck::Sonst` (Tab, F5, Bild-auf, …) wird nicht geschluckt: es geht
    // an die Seite, wie es ohne die Konsole tat.
    let genommen = s.devtools.konsole.druck(d);
    if genommen {
        // GEMESSENE ENTSCHEIDUNG, warum NUR `schmutzig`: die Werkzeuge
        // zeichnen aus `s.devtools`, und `Angezeigt` (die Seite) ist
        // unberuehrt. `schmutzig` sagt dem Fenster, dass es neu zeichnen
        // soll — mehr ist nicht noetig und nichts ist schaedlich.
        s.schmutzig = true;
    }
    genommen
}

/// D12: Der Tastendruck landet im Filterfeld (wenn es den Fokus hat).
///
/// GEMESSENE ENTSCHEIDUNG, warum NUR `Zeichen`, `Zurueck` und `Flucht`:
/// die anderen Tasten (Pfeile, Entf, Enter) waeren im Filterfeld eine
/// Bedienung, die es nicht gibt — ein Filter hat keine History wie eine
/// Adressleiste. Sie gehen an die Seite, wie es ohne Filterfeld tat.
///
/// M4: die Laenge begrenzt `Konsole::filter_einfuegen` /
/// `NetzLog::filter_einfuegen` (`max_filterzeichen`), nicht diese Funktion.
fn filter_nimmt(s: &mut Sitzung, d: narsil_devtools::d3_konsole::Druck) -> bool {
    use narsil_devtools::d3_konsole::Druck;
    if !s.devtools.offen() {
        return false;
    }
    match s.devtools.reiter() {
        Reiter::Konsole if s.devtools.konsole.filter_fokus => match d {
            Druck::Zeichen(c) => s.devtools.konsole.filter_einfuegen(c),
            Druck::Zurueck => s.devtools.konsole.filter_backspace(),
            Druck::Flucht => {
                s.devtools.konsole.filtertext.clear();
                s.devtools.konsole.filter_fokus = false;
            }
            _ => return false,
        },
        Reiter::Netzwerk if s.devtools.netz.filter_fokus => match d {
            Druck::Zeichen(c) => s.devtools.netz.filter_einfuegen(c),
            Druck::Zurueck => s.devtools.netz.filter_backspace(),
            Druck::Flucht => {
                s.devtools.netz.filter.clear();
                s.devtools.netz.filter_fokus = false;
            }
            _ => return false,
        },
        _ => return false,
    }
    s.schmutzig = true;
    true
}

/// D3: EINE FENSTER-TASTE AN DIE KONSOLENEINGABE (mit Uebersetzung).
///
/// # DIE EINZIGE STELLE, DIE UEBERSETZT
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x): `narsil_input::Key` hat kein
/// `name()`, und `narsil-input` ist eine fremde Crate (Rolle Eingabe) — also
/// wird hier uebersetzt, in meiner Datei, an EINER Stelle. Wer die
/// Uebersetzung in `sitzung.rs` (oder in `narsil-input`) machte, haette sie an
/// zwei Orten, sobald die zweite Taste dazukommt.
///
/// GEMESSENE ENTSCHEIDUNG, warum Strg/Alt die Eingabezeile NICHT
/// erreichen: `Ctrl+C` soll kopieren und `Alt+p` den Menuepunkt suchen,
/// nicht „c" bzw. „p" in die Konsole tippen. Wer diese Abfrage weglässt,
/// hat in jedem Fenster mit Strg+C ein „c" in der Konsole.
pub fn taste_eingabe(s: &mut Sitzung, k: &narsil_input::KeyEvent) -> bool {
    use narsil_input::Key;
    // Losgelassene Tasten und wiederholte Ereignisse sind keine Eingabe.
    if !k.pressed || k.modifiers.ctrl || k.modifiers.alt {
        return false;
    }
    // Release 09.10.2026: das Feld des Box-Modells zuerst. GEMESSEN vorher:
    // `box_eingabe_taste` war gebaut und wurde nirgends gerufen — der Klick
    // waehlte das Feld, aber jede Taste ging an die Konsole, und „12" blieb
    // „12".
    if box_eingabe_taste(s, k) {
        s.schmutzig = true;
        return true;
    }
    let d = match k.key {
        Key::Char(c) => narsil_devtools::d3_konsole::Druck::Zeichen(c),
        Key::Space => narsil_devtools::d3_konsole::Druck::Zeichen(' '),
        Key::Backspace => narsil_devtools::d3_konsole::Druck::Zurueck,
        Key::Delete => narsil_devtools::d3_konsole::Druck::Entf,
        Key::Left => narsil_devtools::d3_konsole::Druck::Links,
        Key::Right => narsil_devtools::d3_konsole::Druck::Rechts,
        Key::Enter => narsil_devtools::d3_konsole::Druck::Enter,
        Key::Escape => narsil_devtools::d3_konsole::Druck::Flucht,
        _ => narsil_devtools::d3_konsole::Druck::Sonst,
    };
    eingabe_nimmt(s, d)
}

/// D9: NUR DAS LAYOUT neu rechnen, wenn sich die Hoehe aendert.
///
/// # WARUM DAS EIGENE FUNKTION UND NICHT `neu_laden()`
///
/// GEMESSENE ENTSCHEIDUNG, gestuetzt auf Messungen der Rolle PROBE
/// (01.10.2026): der TEURE Teil des Seitenbaus ist die **Kaskade**
/// (github.com: 6153 ms im Debug-Bau, 8,6-fach besser seit dem
/// `opt-level`-Eintrag). `neu_laden()` macht Parse + Kaskade + Layout —
/// also wuerde jeder F12-Druck die ganze Kaskade wiederholen. Bei zwei
/// Druck sind das zwei Kaskaden.
///
/// GEMESSENE ENTSCHEIDUNG, warum trotzdem neu rechnen: `sicht_hoehe()`
/// ist in den Umbruch gegangen (D9), und ohne neuen Umbruch bleibt die
/// Seite so gelegt, wie sie bei voller Fenstereohe war — der untere Teil
/// der Seite waere unsichtbar, ohne dass man es merkt. Also: nur
/// `layout_mit_plan` neu, mit dem Knotenstil, den die Kaskade schon
/// berechnet hat. Das ist der billige Teil.
///
/// GEMESSENE ENTSCHEIDUNG, warum das bei JEDEM Auf und Zu passiert und
/// nicht nur beim Auf: der Nutzer schliesst die Werkzeuge, um wieder die
/// volle Seite zu sehen — und wenn die Seite dann 100 px zu kurz bleibt,
/// schliesst er sie wieder. Es muss beidseitig stimmen.
pub fn neu_legen(s: &mut Sitzung) {
    let breite = s.breite.max(1) as f32;
    let hoehe = s.sicht_hoehe().max(1) as f32;
    let zoom = s.zoom;
    // GEMESSENE ENTSCHEIDUNG, warum beide Werte VOR dem `&mut` geholt
    // werden: `MessungMesser::new(self.messung.clone())` braucht `self`,
    // `a` ist `&mut self.angezeigt` — und beides gleichzeitig waere ein
    // doppelter Leihvertrag auf `self`. Dieselbe Reihenfolge steht in
    // `aus_ist` (sitzung.rs, `MessungMesser::new(self.messung.clone())`
    // VOR dem `layout_mit_plan`).
    let messer = narsil_layout_engine::MessungMesser::new(s.messung.clone());
    let viewport = narsil_core::geometry::Size::new(breite * zoom, hoehe * zoom);
    let Some(a) = s.angezeigt.as_mut() else {
        return;
    };
    // GEMESSENE ENTSCHEIDUNG, warum ein Fehler hier NICHT panickt: der
    // alte Baum bleibt stehen. Eine kaputte Seite bleibt lesbar (mit dem
    // alten Umbruch), und der Nutzer sieht die Werkzeuge.
    //
    // GEMESSENE ENTSCHEIDUNG (04.10., clippy `single_match`): ein
    // `match` mit einem einzigen brauchbaren Zweig ist ein `if let` mit
    // einem Extrawort — und clippy sagt es bei jedem Lauf. Die Form ist
    // dieselbe, die Entscheidung auch.
    if let Ok(baum) = narsil_layout_engine::layout_mit_plan(
        &a.seite.document,
        viewport,
        &messer,
        &a.seite.layout_plan,
    ) {
        a.baum = baum;
    }
}

// ---------------------------------------------------------------------------
// C — Der Klick: VOR der Leisten-/Seiten-Zweiteilung
// ---------------------------------------------------------------------------

/// Trifft der Punkt den Werkzeugbereich? (D1, D2, D5)
///
/// GEMESSENE ENTSCHEIDUNG, warum `bool` und nicht `Option<bool>`: der
/// Aufrufer will zwei Dinge — „war es der Werkzeugbereich?" und „hat der
/// Klick dort etwas getan?". Das erste steht in `behandelt`, das zweite
/// im Rueckgabewert. `menue_klick` (sitzung.rs:775) macht es genauso.
///
/// # WAS DIE BRUECKE IN DEN WERKZEUGEN AUSLOEST
///
/// * Ein Klick auf ein Reiterfeld waehlt den Reiter.
/// * Der Doppelklick auf ein Reiterfeld macht es auf/zu — NEIN: hier
///   gibt es keinen Doppelklick (`ButtonPressed` traegt keine Klickzahl,
///   `narsil_window::MouseButton` kennt keine), also wird der Zweite
///   Klick auf dasselbe Feld als „alles auf" gelesen? Nein. GEMESSENE
///   ENTSCHEIDUNG: **kein** Zweitklick-Verhalten. Es gibt keine
///   Klickzahl im Ereignis, und eine Heuristik („gleiches Feld
///   zweimal") waere geraten.
/// * Ein Klick auf ein Dreieck klappt auf/zu.
/// * Ein Klick auf eine Zeile im Baum waehlt den Knoten (D2).
/// * Ein Klick auf das Kreuz schliesst.
pub fn mausklick_zuerst(s: &mut Sitzung, punkt: Point) -> bool {
    if !s.devtools.offen() {
        return false;
    }
    // Die Reiterleiste zuerst: sie ist oben im Bereich, und die Zeilen
    // liegen darunter. `Geruest::klick` entscheidet das selbst (erst das
    // Kreuz, dann die Felder) — siehe dort die Begruendung zur Reihenfolge.
    let im_rahmen = s
        .devtools
        .bereich()
        .is_some_and(|b| b.contains(narsil_core::geometry::Point::new(punkt.x, punkt.y)));
    if !im_rahmen {
        return false;
    }
    // 1. Reiterleiste und Kreuz.
    if s.devtools.geruest.klick(punkt.x, punkt.y) {
        s.schmutzig = true;
        return true;
    }
    // 2. D12: DIE WERKZEUGLEISTE — Filterfeld, Umschalter, Aktionsknoepfe.
    // GEMESSENE ENTSCHEIDUNG, warum sie VOR `inhalt_klick` kommt: sie liegt
    // im Inhaltsbereich, und `inhalt_unter` rechnet sie zu Zeile 0. Ein
    // Klick auf „Leeren" waere sonst ein Klick auf die erste Zeile der
    // Konsole — das ist die Fehlerklasse „gezeichnet an der einen Stelle,
    // getroffen an der anderen", nur einmal nach unten versetzt.
    if werkzeug_klick(s, punkt) {
        s.schmutzig = true;
        return true;
    }
    // 3. D18/D19 (04.10., Freigabe der Leitung): die Werkzeugkoepfe und das
    // Bearbeitungsfeld liegen IM Reiter, nicht in der Leiste — sie werden
    // hier geprueft, BEVOR die Zeile gefragt wird. GEMESSENE ENTSCHEIDUNG:
    // ein Knopf, der unter einer Zeile liegt (alle drei liegen in der
    // oberen Reihe), waere sonst nicht erreichbar.
    if reiter_kopf_klick(s, punkt) {
        s.schmutzig = true;
        return true;
    }
    // 4. Der Inhalt: eine Zeile im aktiven Reiter.
    let getroffen = inhalt_klick(s, punkt);
    if getroffen {
        s.schmutzig = true;
    }
    // GEMESSENE ZUSAGE, und sie ist wichtig: ein Klick IM Werkzeugbereich
    // geht IMMER an den Werkzeugbereich. Ein Klick, der dort nichts
    // trifft, wird nicht an die Seite weitergereicht — sonst wuerde ein
    // Klick neben einem Reiter einen Link in der Seite ausloesen, und
    // der Nutzer haelt das Werkzeug fuer undurchsichtig.
    true
}

/// D12: EIN KLICK IN DIE WERKZEUGLEISTE — und was er bewirkt.
///
/// # WER WAS KANN
///
/// Die Geometrie kommt aus [`narsil_devtools::werkzeug`] (EIN Ort, wie bei
/// den Schrittknoepfen und den Speicher-Reitern). Die Wirkungen sind zweierlei:
///
/// * was die Crate kann (Reiter wechseln, Filter leeren, Schalter umlegen),
///   macht [`narsil_devtools::werkzeug::wirke`] — damit ist der Zustand nach
///   dem Klick derselbe wie nach einem Aufruf aus dem Fenster.
/// * was die Crate nicht kann (Seite neu laden, neu messen, den echten
///   Speicher loeschen), macht diese Funktion.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Filter-Fokus HIER steht und nicht im
/// Zustand der Crate: Fokus ist eine Eigenschaft des Fensters (welches Feld
/// die Tastatur bedient), und `fokus` der Konsoleneingabe liegt aus demselben
/// Grund schon hier im Fensterpfad.
fn werkzeug_klick(s: &mut Sitzung, punkt: Point) -> bool {
    use narsil_devtools::werkzeug::{Wirkung, element_unter, leiste};
    let Some(f) = s.devtools.geruest.inhalt() else {
        return false;
    };
    let reiter = s.devtools.reiter();
    let Some((_i, e)) = element_unter(f, reiter, punkt.x, punkt.y) else {
        return false;
    };
    match e.wirkung {
        // Ein Filterfeld nimmt den Fokus — getippt wird in `taste_eingabe`.
        Wirkung::KonsoleFilter => {
            s.devtools.konsole.filtertext.clear();
            s.devtools.konsole.filter_fokus = true;
            s.devtools.konsole.fokus = false;
            true
        }
        Wirkung::NetzFilter => {
            s.devtools.netz.filter.clear();
            s.devtools.netz.filter_fokus = true;
            true
        }
        // Ein Schalter im Speicher-Reiter geht an `inhalt_klick` (D7 hat die
        // Reiter dort schon, und die Geometrie ist dieselbe Funktion).
        Wirkung::SpeicherBereich(_) | Wirkung::SpeicherLoeschen => false,
        // Der Schritt des JS-Debuggers: derselbe Weg wie die vier Knoepfe
        // (`schritt_unter` liefert jetzt die Leistenpositionen).
        Wirkung::Schritt(_) | Wirkung::HaltepunktSetzen => debugger_schritt(s, punkt),
        // Der Schritt des WASM-Debuggers braucht `&mut Sitzung` UND den
        // Motor aus `Angezeigt` — beides hat diese Funktion.
        Wirkung::WasmSchritt(a) => wasm_schritt_klick(s, a),
        // Aktionen, die die Bruecke selbst machen muss.
        Wirkung::QuellenNeu | Wirkung::LeistungNeu => {
            an_seite(s);
            true
        }
        Wirkung::NetzNeu => {
            // GEMESSENE ENTSCHEIDUNG: die Seite neu laden ist `gehe_zu` mit
            // derselben Adresse (`Angezeigt::seite.basis`, sonst die
            // Adressleiste) — der Browser hat keinen zweiten Weg dafuer.
            let url = s
                .angezeigt
                .as_ref()
                .map(|a| a.seite.basis.clone())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| s.browser.adresse().to_string());
            if url.is_empty() {
                // GEMESSENE ENTSCHEIDUNG: hier wird nichts behauptet. Ohne
                // Adresse gibt es keinen zweiten Weg, und eine Zeile, die
                // „neu geladen" sagen wuerde, waere eine Behauptung.
                eprintln!("D12: kein 'Seite neu laden' — es gibt keine Adresse");
                return true;
            }
            let _ = s.gehe_zu(&url);
            an_seite(s);
            true
        }
        // Alles andere kann die Crate selbst (der Zustand liegt in ihren
        // Reitern — dieselben Felder, die ein Aufrufer setzen wuerde).
        andere => narsil_devtools::werkzeug::wirke(&mut s.devtools, e),
    }
}

/// # GEMESSENE EINSCHRAENKUNG, die ich nicht wegzaubern kann
///
/// Der Motor (`WasmMotor`) gehoert dem AUFRUFER, nicht der Sitzung: die
/// Bruecke gibt ihn zurueck (`wasm_motor_starten`), und `Sitzung` hat kein
/// Feld dafuer (das waere eine Aenderung an `sitzung.rs`, einer fremden
/// Datei der Rolle UI). Also kann ein Klick im Fenster den Motor nicht
/// fahren — was er tun KANN, ist den Schritt beauftragen
/// ([`narsil_devtools::d6_wasm::WasmDbg::schritt_beauftragen`]) und, wenn der
/// Debugger nicht in der Pause steht, genau das sagen.
///
/// GEMESSENE ENTSCHEIDUNG, warum NICHT „tut nichts": ein Knopf, der im
/// Bild da ist und stillschweigend nichts bewirkt, ist der Fehler, den D11
/// am Quellenreiter gefunden hat. Der Hinweis im Reiter ist die kleinste
/// ehrliche Antwort.
fn wasm_schritt_klick(s: &mut Sitzung, a: narsil_devtools::d6_wasm::WasmSchritt) -> bool {
    if s.devtools.wasm.schritt_beauftragen(a).is_none() {
        s.devtools.wasm.hinweis = format!(
            "{} nicht moeglich: der Debugger steht nicht in der Pause.",
            a.name()
        );
    }
    s.schmutzig = true;
    true
}

/// Der Inhaltsbereich: eine Zeile im aktiven Reiter.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in `narsil-devtools`:
/// `Geruest::inhalt_unter` gibt die Zeilennummer, aber WAS die Zeile
/// bewirkt, haengt vom Reiter ab — und die Reiter gehoeren `narsil`
/// (das Fenster), waehrend die Zeilen `narsil-devtools` gehoeren. Die
/// Bruecke ist also genau hier, einmal, fuer alle sieben Reiter.
fn inhalt_klick(s: &mut Sitzung, punkt: Point) -> bool {
    let Some((zeile, _spalte)) = s.devtools.geruest.inhalt_unter(punkt.x, punkt.y) else {
        return false;
    };
    match s.devtools.reiter() {
        // D2: Baumzeile = Knoten waehlen; Pfeil auf- oder zuklappen.
        Reiter::Elemente => {
            let Some(z) = s.devtools.elemente.zeilen.get(zeile as usize).cloned() else {
                return false;
            };
            // GEMESSENE ENTSCHEIDUNG: der Einrueckungsbereich (die ersten
            // 14 px einer Zeile) ist das Dreieck. Sonst waere das
            // Aufklappen ueber einem Text zu treffen, und der Nutzer
            // muesste auf den Pfeil zielen statt auf die Zeile.
            let dreieck = z.tiefe as f32 * 14.0 + 2.0;
            if punkt.x - self_x(s) <= dreieck + 12.0 {
                return s.devtools.elemente.kippe(z.css);
            }
            s.devtools.elemente.waehle(z.css)
        }
        // D3: DER KLICK AUF DIE EINGABEZEILE SETZT DEN FOKUS.
        //
        // GEMESSENER ZUSTAND BIS 02.10.2026 3:1x: dieser Zweig gab `false`
        // zurueck — also „der Klick gehoert der Seite". Die Eingabezeile
        // stand gezeichnet da, lud zur Eingabe ein und nahm nichts an: es
        // gab keinen Weg, sie zu tippen. Das war die letzte Luecke von D3
        // („JS-Eingabe ausfuehren"): die Ausfuehrung war da
        // (`Konsole::ausfuehren_und_leeren`), nur der Weg nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das Feld GEKLICKT werden muss und
        // nicht der Reiter: sonst kaeme jede Seite, auf der die Werkzeuge
        // offen sind, die Tastatur vom Nutzer (er tippt ja in die Seite).
        // Der Klick ist die ausdrueckliche Wahl — wie in Chrome.
        Reiter::Konsole => {
            let feld = s
                .devtools
                .geruest
                .inhalt()
                .map(narsil_devtools::zeichnen::konsole_eingabefeld);
            if feld.is_some_and(|f| f.contains(punkt)) {
                s.devtools.konsole.fokus = true;
                return true;
            }
            false
        }
        // D5: Quelltextzeile = Haltepunkt setzen/entfernen (Chrome macht
        // das im Rand neben der Zeilennummer — hier im Text, weil es
        // keine separate Randspalte gibt).
        //
        // D11 (04.10.2026, 1:1x): ZUERST die vier Schrittknöpfe. Sie
        // liegen in der linken Spalte ganz UNTEN — also weit weg von der
        // Zeile, die der Klick getroffen haette. Wer sie nicht zuerst
        // prueft, wuerde beim Klick auf „Weiter" einen Haltepunkt setzen.
        Reiter::Quellen => {
            if debugger_schritt(s, punkt) {
                return true;
            }
            let Some((nr, _, _)) = s
                .devtools
                .quellen
                .sichtbare_zeilen(zeile as usize + 1)
                .get(zeile as usize)
                .cloned()
            else {
                return false;
            };
            s.devtools
                .quellen
                .haltepunkt(s.devtools.quellen.gewaehlt, nr)
        }
        // D4: Zeile waehlen (die Detailansicht folgt).
        Reiter::Netzwerk => {
            let zeilen = s.devtools.netz.sichtbare_zeilen();
            if let Some(z) = zeilen.get(zeile as usize) {
                s.devtools.netz.gewaehlt = Some(z.nr);
                return true;
            }
            false
        }
        // D8: die Leistungsliste ist eine Anzeige, keine Bedienung.
        Reiter::Leistung => false,
        // D7: Zeile waehlen (Cookie-Details).
        //
        // D7: DREI KNOPFE UND DIE ZEILE (02.10.2026, 3:2x).
        //
        // GEMESSENER ZUSTAND BIS HIER: dieser Zweig kannte NUR die Zeile.
        // Die drei Bereichsknoepfe wurden gezeichnet und waren nicht
        // klickbar, und der Loeschknopf gab es nicht — obwohl
        // `cookie_loeschen`, `alle_cookies_loeschen`, `lokal_loeschen`,
        // `lokal_leeren`, `cache_loeschen` und `cache_leeren` alle fertig
        // implementiert waren (GEMESSEN: alle sechs ohne einen einzigen
        // Aufrufer). D7 verlangt „ansehen/loeschen" — geloscht wurde
        // nichts, und „Cache" und „localStorage" waren unerreichbar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge so ist: erst die
        // Knoepfe, dann die Zeile. Eine Zeile kann unter einem Knopf
        // liegen, und wer zuerst die Zeile fragt, trifft den Knopf nie.
        Reiter::Speicher => {
            // Der Inhaltsbereich — dieselbe Rechnung, mit der der Reiter
            // gezeichnet hat (`self_x` sagt es im Kommentar).
            let panel = match s.devtools.geruest.inhalt() {
                Some(r) => r,
                None => return false,
            };
            for b in narsil_devtools::d7_speicher::BEREICHE {
                if narsil_devtools::zeichnen::bereich_knopf(panel, b).contains(punkt) {
                    s.devtools.speicher.reiter = b;
                    // GEMESSENE ENTSCHEIDUNG, warum die Auswahl weggeht:
                    // gewaehlt ist ein COOKIE-Name; im Cache-Bereich gibt es
                    // keine Cookies, und eine Marke auf einer fremden Zeile
                    // waer genau die „Zahl stimmt, Bild stimmt nicht".
                    s.devtools.speicher.gewaehlt = None;
                    return true;
                }
            }
            if narsil_devtools::zeichnen::loesch_knopf(panel).contains(punkt) {
                let text = s.devtools.speicher.loeschen();
                s.schmutzig = true;
                eprintln!("D7: {text}");
                return true;
            }
            let zeilen = s.devtools.speicher.zeilen();
            if let (Reiter::Speicher, Some(z)) = (s.devtools.reiter(), zeilen.get(zeile as usize)) {
                s.devtools.speicher.gewaehlt = z.split_whitespace().next().map(|s| s.to_string());
                return true;
            }
            false
        }
        // D6: Zeile waehlen (Funktion waehlen).
        Reiter::WebAssembly => {
            let funktionen = s.devtools.wasm.funktions_zeilen();
            if funktionen.get(zeile as usize).is_some() {
                return s.devtools.wasm.waehle_funktion(zeile as usize);
            }
            false
        }
    }
}

/// Die Signatur einer Funktion als Kurztext: `(i32, i32) -> i32`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion und kein
/// `to_string()` ist: `narsil_wasm::modell::FuncTyp` hat KEINE Anzeige-
/// methode (gemessen 2:2x: nur `parameter` und `ergebnis`, kein `impl
/// FuncTyp`). Der Reiter braucht genau diese eine Form, also wird sie hier
/// gebaut — aus `Valtyp::text()`, das die Crate DOES anbietet (modell.rs:81).
fn func_typ_kurz(t: &narsil_wasm::modell::FuncTyp) -> String {
    let p: Vec<&str> = t.parameter.iter().map(|v| v.text()).collect();
    let e: Vec<&str> = t.ergebnis.iter().map(|v| v.text()).collect();
    format!("({}) -> {}", p.join(", "), e.join(", "))
}

/// D18/D19 (04.10.): DIE KNOEPFE IM RECHTER SEIT DES REITERS.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie VOR `inhalt_klick` kommen: sie liegen
/// auf der Zeile, in der auch Text steht (die Typ-Knoepfe auf der
/// Zaehlerzeile des Netz-Reiters, das Bearbeitungsfeld ueber dem Kasten).
/// Wer zuerst die Zeile fragt, trifft die Zeile — und der Knopf ist fuer den
/// Nutzer tot.
///
/// # WAS JEDER KNOPF TUT
/// * Konsole: eine Zeile des Ergebnis-Baums auf- oder zuklappen
///   (`konsole_baum_unter` -> `baum_umschalten`).
/// * Netzwerk: den Typ-Filter setzen (`netz_gruppen_rechtecke` ->
///   `gruppe_unter` -> `gruppe_setzen`) und den Reiter der Detailansicht
///   (`netz_detail_rechtecke` -> `detail_setzen`).
/// * Elemente: das Bearbeitungsfeld fuer einen Box-Wert waehlen
///   (`box_eingabefeld_unter` -> `box_eingabe.waehlen`) und einen
///   Regelschalter umlegen (`regeln.schalten`).
fn reiter_kopf_klick(s: &mut Sitzung, punkt: Point) -> bool {
    // GEMESSENE ENTSCHEIDUNG (04.10., 07:00): die Panel-Flaeche kommt aus
    // `zeichnen::panel_bereich(&s.devtools)` — das ist dieselbe Flaeche, mit
    // der der Reiter gezeichnet wird. `geruest.inhalt()` waere die Flaeche
    // MIT der Werkzeugleiste, also eine Ebene zu viel.
    match s.devtools.reiter() {
        // D18: der Ergebnisbaum der Konsole.
        Reiter::Konsole => {
            let panel = narsil_devtools::zeichnen::panel_bereich(&s.devtools);
            match narsil_devtools::zeichnen::konsole_baum_unter(&s.devtools, panel, punkt) {
                Some(pfad) => s.devtools.konsole.baum_umschalten(&pfad),
                None => false,
            }
        }
        // D18/D19: Typ-Knoepfe und Detail-Reiter.
        Reiter::Netzwerk => {
            let panel = narsil_devtools::zeichnen::panel_bereich(&s.devtools);
            let gruppen = narsil_devtools::zeichnen::netz_gruppen_rechtecke(panel);
            if let Some(g) = narsil_devtools::d4_netzwerk::gruppe_unter(&gruppen, punkt) {
                return s.devtools.netz.gruppe_setzen(g);
            }
            let reiter = narsil_devtools::zeichnen::netz_detail_rechtecke(panel);
            if let Some(d) = narsil_devtools::d4_netzwerk::detail_unter(&reiter, punkt) {
                return s.devtools.netz.detail_setzen(d);
            }
            false
        }
        // D19: das Bearbeitungsfeld des Box-Wertes und der Regelschalter.
        Reiter::Elemente => {
            let panel = narsil_devtools::zeichnen::panel_bereich(&s.devtools);
            //
            // GEMESSENER FEHLER (04.10., 07:00): die erste Fassung stand hier
            // als `s.devtools.elemente.stil.as_ref()?` — ein `?` in einer
            // Funktion, die `bool` zurueckgibt. Das haette nicht gebaut; der
            // Wert 0 waere hier nicht richtig (0 px ist eine Behauptung), also
            // wird ohne Stil gar nichts gewaehlt.
            if let Some(ziel) =
                narsil_devtools::zeichnen::box_eingabefeld_unter(&s.devtools, panel, punkt)
                && let Some(stil) = s.devtools.elemente.stil.as_ref()
            {
                let wert =
                    narsil_devtools::d9_layout::box_wert(stil, ziel.0, ziel.1).unwrap_or(0.0);
                return s.devtools.elemente.box_eingabe.waehlen(ziel, wert);
            }
            if let Some((zeile, eig)) =
                narsil_devtools::zeichnen::regel_schalter_unter(&s.devtools, panel, punkt)
            {
                return s.devtools.elemente.regeln.schalten(zeile, eig);
            }
            false
        }
        _ => false,
    }
}

/// D19: DIE TASTEN DES BEARBEITUNGSFELDES.
///
/// GEMESSENE ENTSCHEIDUNG, warum das VOR der Konsoleneingabe kommt: beide
/// Felder nehmen dieselben Tasten (`Zeichen`, `Backspace`, `Enter`), und ein
/// Tastendruck kann nur an EINEM Feld landen. Das Feld mit Fokus bekommt ihn
/// — und der Fokus entsteht durch den Klick (`box_eingabe.fokus`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `Enter` die Aenderung NICHT anwendet: das
/// Anwenden braucht den Weg aus `team/BRETT.txt` (03:35, Abschnitt 3):
/// `Seite` neu rechnen (`berechnet_stil_fuer`) ist ein Aufruf in
/// `narsil-page`. Bis der da ist, gibt `uebernehmen` nur den Wert zurueck —
/// und der Reiter schreibt ihn in den Hinweis, damit der Nutzer sieht, was
/// er getippt hat. GEMESSEN: ein stilles Wegwerfen waere die schlimmste
/// Form davon (der Wert ist getippt und nirgends).
fn box_eingabe_taste(s: &mut Sitzung, k: &narsil_input::KeyEvent) -> bool {
    use narsil_input::Key;
    if !k.pressed || k.modifiers.ctrl || k.modifiers.alt {
        return false;
    }
    let e = &mut s.devtools.elemente.box_eingabe;
    if !e.fokus {
        return false;
    }
    match k.key {
        Key::Char(c) => e.tippen(c),
        Key::Backspace => e.zurueck(),
        Key::Enter => {
            let knoten = s.devtools.elemente.gewaehlt.unwrap_or(0);
            match e.uebernehmen(knoten) {
                Some(a) => {
                    s.devtools.elemente.hinweis = a.to_string();
                    s.schmutzig = true;
                    true
                }
                None => {
                    s.devtools.elemente.hinweis =
                        String::from("Nichts geaendert: der Wert ist keine Zahl");
                    s.schmutzig = true;
                    true
                }
            }
        }
        // GEMESSENE ENTSCHEIDUNG (D3-Konsole): Escape leert das Feld und gibt
        // den Fokus erst beim zweiten Mal her. Dasselbe gilt hier — ein
        // vertipptes Feld soll sich zuruecknehmen lassen.
        Key::Escape => {
            if e.text.is_empty() {
                e.verlassen();
            } else {
                e.text.clear();
                e.cursor = 0;
            }
            s.schmutzig = true;
            true
        }
        _ => false,
    }
}

/// D8: DIE PHASEN DES LADENS IN DEN LEISTUNGSREITER ÜBERNEHMEN.
///
/// # WAS HIER ECHT WIRD (gemessen 02.10.2026, 1:8x)
///
/// `narsil::zeiten::Messung` misst die Phasen seit langem — aber nur, wenn
/// `NARSIL_ZEITEN=1` in der Umgebung steht, und dann gehen die Zahlen nach
/// **stderr**. Der Reiter „Leistung" sah darum im Browser immer Null.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein Puffer durch den Ladeweg: der Weg
/// ist `&self` (`aus_ist`, `laden_mit_breite`), und ein `&mut Leistung`
/// als Parameter zieht durch jede Signatur und ~40 Testaufrufe. Stattdessen
/// sammelt `zeiten::Messung::phase` nach Name in eine Thread-Liste, und
/// diese eine Stelle liest der Reiter. Das ist die Ausgabepuffer-Variante
/// aus `team/WUNSCH_DEVTOOLS.txt` Abschnitt 7 — mit dem Vorteil, dass
/// keine fremde Signatur sich aendert.
///
/// GEMESSENE ENTSCHEIDUNG, warum „netz" aus `lade_jetzt` und die anderen
/// aus `aus_ist` beide ankommen: ein Ladevorgang hat ZWEI `Messung`
/// (netz in `gehe_zu`/`lade_jetzt`, der Rest in `aus_ist`). Die Sammlung
/// ist deshalb „bis zum naechsten `sammel_leeren`", nicht „der letzte
/// Vorgang" — sonst stuende am Ende nur noch `netz` im Reiter.
/// (GEMESSEN genau so in der ersten Fassung.)
///
/// WAS NICHT ANKOMMT: „blaetter" (externe Stylesheets) hat keine Stufe in
/// D8 — die sechs Stufen sind Netz, Parse, Kaskade, Layout, Raster, Js.
/// Der Name wird bewusst NICHT auf eine andere Stufe gemappt: 12 ms
/// Blatt-Abruf als „Kaskade" waere eine Zahl, die stimmt und falsch ist.
/// Und „Raster" bleibt leer, weil das Rastern im Fensterloop laeuft
/// (`narsil::zeichne`), nicht im Ladeweg — dafuer steht ein Wunsch an die
/// Rolle UI (ein Aufruf).
pub fn leistung_uebernehmen(adresse: &str, l: &mut narsil_devtools::d8_leistung::Leistung) {
    use narsil_devtools::d8_leistung::Stufe;
    // Der Vorgang faengt VOR dem Laden an — der Name ist die Adresse.
    l.vorgang_beginnen(adresse);
    let ges = crate::zeiten::gesammelt();
    for (name, ns) in ges.iter().copied() {
        let stufe = match name {
            "netz" => Stufe::Netz,
            "parse" => Stufe::Parse,
            "kaskade" => Stufe::Kaskade,
            "layout" => Stufe::Layout,
            "js" => Stufe::Js,
            // GEMESSENE ENTSCHEIDUNG: `Raster` wird hier NICHT gesetzt.
            // Es gibt im Ladeweg keinen Raster-Schritt, und eine Zahl zu
            // erfinden waere die schlimmste Variante.
            _ => continue,
        };
        l.merke(stufe, ns);
    }
}

/// D8: DIE MESSUNG AN- ODER AUSSCHALTEN (der Reiter schaltet sie selbst).
///
/// GEMESSENE ENTSCHEIDUNG, warum der Reiter das tut und nicht der Aufrufer:
/// `Messung::neu()` wird siebenmal im Ladeweg aufgerufen. Wer sie einschaltet,
/// muss es also an jeder Stelle tun — oder an der EINEN Stelle, die alle
/// benutzt. Das ist `zeiten::laeuft()` (02.10.2026, 1:8x).
pub fn messung_schalten(an: bool) {
    if an {
        crate::zeiten::erwecke();
    } else {
        crate::zeiten::schlaefe();
    }
}

/// D8: LÄSST SICH DIE MESSUNG ÜBERHAUPT EINSCHALTEN?
#[must_use]
pub fn messung_moeglich() -> bool {
    true
}

/// D6 + W5: DER MOTOR — Debugger, Lauf und Instanz eines Moduls.
///
/// # WARUM EINE EIGENE STRUKTUR UND KEIN FELD IN `Sitzung` (02.10.2026)
///
/// W5 ist gelandet (`narsil-wasm/src/debug.rs`): `Debugger` mit
/// `ausfuehren/weiter/zurueck/bis_haltepunkt/bis_rueckkehr`,
/// `aufrufstapel/lokale/operandstapel/speicheransicht/disassembly/zeilen`,
/// und `Haltepunkte::setze(funktion, versatz, aktiv)`.
///
/// Der Debugger braucht beim Anlegen und Ausfuehren einen `Lauf` und eine
/// `Instanz` (`Debugger::ausfuehren(&mut Lauf, &Instanz, …)`), und die
/// Speicheransicht braucht den `Lauf` weiterhin. Drei Werte, die
/// zusammenleben muessen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der AUFRUFER sie haelt und nicht die
/// Bruecke: eine Bruecke aus freien Funktionen kann keinen Zustand
/// halten — ausser ueber `static` (das verbietet REGELN M6: globaler
/// Zustand, zweite Wahrheit) oder ueber ein Feld in `Sitzung` (das ist
/// `sitzung.rs`, eine Datei, in der parallel die Rolle UI arbeitet; ich
/// habe dort Freigabe fuer „minimale Einhaengung", und ein Debugger-Feld
/// ist das nicht).
///
/// Also: der Aufrufer haelt den Motor. Im Browser ist das die Stelle, die
/// das Modul geladen hat (W4, `WebAssembly.instantiate` — steht noch aus);
/// im Abnahmebeispiel ist es das Beispiel; in den Tests ist es der Test.
/// Die Bruecke stellt die sechs Schritte bereit, alle an EINER Stelle:
///
/// ```ignore
/// let mut m = devtools_bruecke::wasm_motor_starten(&mut s, &modul, "quadriere", &[3])?;
/// devtools_bruecke::wasm_motor_haltepunkt(&mut m, &mut s, 1, 4, true);
/// devtools_bruecke::wasm_motor_bis_haltepunkt(&mut m, &mut s);
/// ```
pub struct WasmMotor {
    /// Der Debugger der Engine (W5).
    pub debugger: narsil_wasm::debug::Debugger,
    /// Der Lauf — er muss mit dem Debugger leben, weil die Speicheransicht
    /// ihn braucht.
    pub lauf: narsil_wasm::Lauf,
    /// Die Instanz des Moduls.
    pub instanz: narsil_wasm::Instanz,
    /// Der Modulname fuer die Anzeige.
    pub name: String,
    /// Der Export, der ausgefuehrt wurde.
    pub export: String,
}

/// D6/W5: EIN MODUL MIT DEBUGGER STARTEN und den Reiter fuellen.
///
/// Reihenfolge (gemessen, und sie ist nicht beliebig):
/// 1. `instanziieren` — Speicher, Tabellen, Globale anlegen.
/// 2. `Debugger::neu` mit dem Modul.
/// 3. **Die Haltepunkte des Reiters in die Engine geben**, BEVOR der
///    Lauf beginnt. Der Reiter ist die Quelle (das ist die Entscheidung aus
///    D11, und sie gilt fuer W5 genauso): ein Lauf ohne die Haltepunkte des
///    Reiters waere ein Lauf an einer Stelle, die der Nutzer nicht gewaehlt
///    hat.
/// 4. `ausfuehren` — zeichnet die Spur.
/// 5. Den Zustand lesen (Schritt 6 in [`wasm_motor_stand_lesen`]).
///
/// GEMESSENE ENTSCHEIDUNG, warum `ImportTabelle::default()`: das Modul der
/// Abnahme hat keine Importe. Fuer ein Modul MIT Importen braucht es eine
/// echte Quelle — das ist der Weg, den W4 (`WebAssembly.instantiate`) geht.
pub fn wasm_motor_starten(
    s: &mut Sitzung,
    modul: &narsil_wasm::Modul,
    export: &str,
    argumente: &[narsil_wasm::Wert],
) -> Result<WasmMotor, String> {
    // GEMESSENE ENTSCHEIDUNG (02.10.2026, 0:7x): `Grenzen` ist KEIN
    // `Copy` — der Uebersetzer hat es gemeldet („use of moved value").
    // Also zwei: einer fuer den Lauf, einer fuer den Debugger. Beide sind
    // derselbe Wert (`standard()`), und es ist besser, das zu sagen als
    // `clone()` zu schreiben und zu tun, als waeren es verschiedene.
    let grenzen_lauf = narsil_wasm::Grenzen::standard();
    let grenzen_debugger = narsil_wasm::Grenzen::standard();
    let mut lauf = narsil_wasm::Lauf::neu(grenzen_lauf);
    let instanz =
        narsil_wasm::lauf::instanziieren(&mut lauf, modul, &narsil_wasm::js::ImportTabelle::default())
            .map_err(|e| format!("Instanziieren: {e:?}"))?;
    let rc = std::rc::Rc::new(modul.clone());
    let mut debugger = narsil_wasm::debug::Debugger::neu(rc, grenzen_debugger);
    // Die Haltepunkte des Reiters zuerst (Schritt 3 der Begruendung).
    let h = s.devtools.wasm.haltepunkte.clone();
    for p in h.iter() {
        debugger
            .haltepunkte
            .setze(p.funktion as u32, p.versatz as usize, true);
    }
    debugger
        .ausfuehren(&mut lauf, &instanz, export, argumente)
        .map_err(|e| format!("{export}(): {e:?}"))?;
    // GEMESSENE ENTSCHEIDUNG (02.10.2026, 1:4x, clippy `unused_mut`):
    // der Motor wird hier nicht mehr veraendert — `wasm_motor_stand_lesen`
    // nimmt ihn nur als Referenz. Das `mut` kam daher, weil der Aufrufer
    // spaeter `&mut motor` braucht; das ist seine Sache.
    let motor = WasmMotor {
        debugger,
        lauf,
        instanz,
        name: s
            .devtools
            .wasm
            .modul()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "<wasm>".to_string()),
        export: export.to_string(),
    };
    wasm_motor_stand_lesen(&motor, s);
    Ok(motor)
}

/// D6/W5: DER ZUSTAND AUS DER ENGINE IN DEN REITER LESEN.
///
/// Was gelesen wird (alles aus `Schnittstelle`, plus Speicher):
/// Zustand, Aufrufstapel, Locals, Operandenstapel, Aufenthaltsort
/// (Modul, Funktion, Befehlsindex) und — falls vorhanden — 256 Byte des
/// Speichers ab Adresse 0.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Zustand so bestimmt wird: W5 hat
/// `aktuell: Option<Schritt>` und `Fortsetzung`. Ein Schritt vorhanden
/// heisst: die Ausfuehrung steht an dieser Stelle. Also `Some` -> Pause,
/// `None` -> Fertig. `Laeuft` gibt es in diesem Zustandsbild nicht, weil
/// die Engine im Spurmodus immer anhaelt (ein Schritt wird gezeichnet und
/// nicht sofort weitergezaehlt). Das ist die ehrliche Abbildung — und der
/// Reiter wuerde sonst „Laeuft" zeigen, waehrend nichts passiert.
pub fn wasm_motor_stand_lesen(m: &WasmMotor, s: &mut Sitzung) {
    use narsil_devtools::d6_wasm::{NamWert, Schnittstelle, WasmRahmen, WasmZustand};
    let d = &m.debugger;
    // GEMESSENE ENTSCHEIDUNG (02.10.2026, 1:4x, clippy
    // `field_reassign_with_default`): alle Felder werden gleich im
    // Konstruktor gesetzt statt einzeln danach. Also wird der Zustand
    // hier berechnet und nicht zugewiesen — das ist auch die Form, die
    // `Schnittstelle::default()` nie falsch machen kann.
    let mut si = Schnittstelle {
        zustand: if d.aktuell.is_some() {
            WasmZustand::Pause
        } else {
            WasmZustand::Fertig
        },
        ..Schnittstelle::default()
    };
    // Der Aufrufstapel: W5 liefert „Name @ Index", je Tiefe eine Zeile.
    for z in d.aufrufstapel() {
        if si.stapel.len() >= narsil_devtools::grenzen::Grenzen::STANDARDAUSGABE.max_stapel {
            break;
        }
        si.stapel.push(WasmRahmen::aus_wasm_text(&m.name, &z));
    }
    for z in d.lokale() {
        if si.locals.len() >= narsil_devtools::grenzen::Grenzen::STANDARDAUSGABE.max_variablen {
            break;
        }
        si.locals.push(NamWert::aus_wasm_text(&z));
    }
    for z in d.operandstapel() {
        if si.werte.len() >= narsil_devtools::grenzen::Grenzen::STANDARDAUSGABE.max_variablen {
            break;
        }
        si.werte.push(z);
    }
    if let Some(a) = d.aktuell.as_ref() {
        si.steht_in = Some((0, a.funktion as usize, a.versatz as u32));
    }
    s.devtools.wasm.aus_schnittstelle(&si);
    // Der Speicher: 256 Byte ab 0 — die erste Seite, Anfang.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `speicheransicht()` NICHT benutzt wird:
    // es liefert fertig formatierte HEX-ZEICHEN (`Debugger::speicheransicht`),
    // und der Reiter will BYTE (`speicher: Vec<u8>`), weil er daraus seinen
    // Hexdump selbst macht. Aus dem Hex zurueck zu Bytes waere eine
    // Umrechnung fuer nichts.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Speicher ueber die INSTANZ laeuft
    // und nicht ueber `Lauf::speicher()` (02.10.2026, 1:2x): der Lauf
    // merkt sich `speicher_idx` nur WAEHREND der Ausfuehrung (`interp.rs`
    // setzt und loescht ihn). Nach `ausfuehren` ist er `None`, und im
    // Abnahmebild stand „Speicher Seite 0/0" bei einem Modul MIT Speicher.
    // Die Instanz merkt sich den Speicher dauerhaft: `Instanz::speicher`
    // (lauf.rs:352) ist die Abbildung Modulindex -> Storeindex, und
    // `Store::speicher` ist `pub` (lauf.rs:77).
    if let Some(i) = m.instanz.speicher.first().copied()
        && let Some(sp) = m.lauf.store.speicher.get(i as usize)
        && let Ok(bytes) = sp.lies(0, 256)
    {
        s.devtools.wasm.speicher_seite_setzen(0, bytes);
    }
}

/// D6/W5: EINEN SCHRITT WEITER (Einzelschritt = ein Spur-Eintrag).
pub fn wasm_motor_weiter(m: &mut WasmMotor, s: &mut Sitzung) -> bool {
    let r = m.debugger.weiter();
    wasm_motor_stand_lesen(m, s);
    r
}

/// D6/W5: BIS ZUM NÄCHSTEN HALTEPUNKT.
pub fn wasm_motor_bis_haltepunkt(m: &mut WasmMotor, s: &mut Sitzung) -> bool {
    let r = m.debugger.bis_haltepunkt().is_some();
    wasm_motor_stand_lesen(m, s);
    r
}

/// D6/W5: EINEN SCHRITT ZURÜCK (die Spur ist ein Array).
pub fn wasm_motor_zurueck(m: &mut WasmMotor, s: &mut Sitzung) -> bool {
    let r = m.debugger.zurueck();
    wasm_motor_stand_lesen(m, s);
    r
}

/// D6/W5: EINEN HALTEPUNKT IN DER ENGINE SETZEN ODER LÖSCHEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Reiter die Quelle bleibt und nicht die
/// Engine: der Klick im Reiter setzt `WasmDbg::haltepunkt`, und DIESE
/// Funktion spiegelt ihn in die Engine. Der andere Weg (die Engine liest)
/// wuerde zwei Listen ergeben, die auseinanderlaufen koennen — und genau
/// das ist der Fehler, den ich in D11 einmal hatte („weiter() lief bis zum
/// Ende, weil die Engine noch die alte Liste hatte").
pub fn wasm_motor_haltepunkt(
    m: &mut WasmMotor,
    s: &mut Sitzung,
    funktion: usize,
    versatz: u32,
    aktiv: bool,
) -> bool {
    m.debugger
        .haltepunkte
        .setze(funktion as u32, versatz as usize, aktiv);
    // Und der Reiter: damit die Marke im Bild und der Zustand in der
    // Engine aus EINER Quelle kommen.
    let _ = s;
    aktiv
}

/// D6/W5: DIE OFFSETS DER DISASSEMBLY SIND ECHT (wenn die Spur sie kennt).
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 0:6x): `Schritt.versatz` ist der
/// Index im Rumpf (`koerper[pc]`), und `wat::schreibe_funktion` gibt den
/// Rumpf in derselben Reihenfolge aus. Also ist der i-ten Befehl der
/// i-te Versatz — OHNE etwas zu raten. Diese Funktion schreibt genau das in
/// die Disassembly-Zeilen und schaltet `versatz_echt` ein.
///
/// Der Test dafuer: `der_index_der_zeile_ist_der_versatz_des_schritts` in
/// `narsil/tests/d9_devtools.rs` vergleicht den aktuellen Schritt der Engine
/// mit der Zeile, die der Reiter markiert.
pub fn wasm_motor_offsets_setzen(m: &WasmMotor, s: &mut Sitzung, funktion: usize) -> bool {
    let Some(md) = s.devtools.wasm.module.get_mut(0) else {
        return false;
    };
    let Some(f) = md.funktionen.get_mut(funktion) else {
        return false;
    };
    // Die Indizes, die die Spur fuer diese Funktion gesehen hat.
    let mut gesehen = std::collections::BTreeSet::new();
    for sc in m.debugger.spur.schritte.iter() {
        if sc.funktion as usize == funktion {
            gesehen.insert(sc.versatz as u32);
        }
    }
    if gesehen.is_empty() {
        return false;
    }
    for (i, z) in f.befehle.iter_mut().enumerate() {
        if gesehen.contains(&(i as u32)) {
            z.versatz = i as u32;
        }
    }
    f.versatz_echt = true;
    true
}

/// D6: EIN ECHTES MODUL IN DEN REITER (Module, Funktionen, Disassembly).
///
/// # WAS HIER ECHT WIRD (gemessen 04.10.2026, 2:2x)
///
/// `narsil-wasm` hat W1 (Parser/Validator), W2 (Interpreter) und einen
/// Disassembly-Schreiber (`wat::schreibe_funktion`, Z. 3024). Damit sind
/// VIER von den sechs D6-Punkten heute baubar: **Modul, Funktionen,
/// Disassembly, Speicher**. Der Reiter stand vorher auf dem ehrlichen
/// Hinweis, weil es diese Daten nicht gab.
///
/// NICHT baubar ohne W5: **Haltepunkte, Schritt, Locals, Operandenstapel**.
/// Das ist kein Restfehler, den ich beheben kann — W5 ist der Debug-Haken
/// im Interpreter, und `debug.rs` ist dort eine Zeile Platzhalter.
///
/// # WARUM DIE UMRECHNUNG HIER STEHT UND NICHT IN `narsil-devtools`
///
/// Meine Crate kennt `narsil-wasm` nicht (sie kennt auch `narsil-jsvm` nicht
/// als Typ, sondern ueber `Schnittstelle`-Strukturen mit Text). Dieselbe
/// Regel wie bei `NamWert` aus J28: die Oberflaeche bekommt Text und
/// Zahlen, nicht die Typen einer fremden Crate. `narsil` ist die einzige
/// Stelle, die beide Crate kennt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Disassembly als TEXT uebergeben wird
/// und nicht als Befehlsliste: `narsil_wasm::wat::schreibe_funktion` liefert
/// einen S-Ausdruck („(func (type 0) (local i32)\n  local.get 0 …)"), und
/// genau so nimmt es `WasmFunktion::aus_text` an. Eine eigene
/// Befehls-Darstellung waere eine ZWEITE Disassembly — und eine andere als
/// die der Engine.
///
/// GEMESSENE EINSICHT, die dabei herauskam (und im Reiter steht): die
/// Versaetze JE BEFEHL kennt das Modell nicht (`Func::Eigen` fuehrt nur
/// `befehl_offset`, den Anfang der Funktion). `aus_text` rechnet sie darum
/// hoch und `versatz_echt` wird false — der Reiter zeigt „····" statt
/// Hexzahlen. Das ist kein Makel im Reiter, sondern der Befund: Haltepunkte
/// brauchen echte Versaetze, und die kommen mit W5.
pub fn wasm_aus_modul(s: &mut Sitzung, m: &narsil_wasm::Modul) -> Result<usize, String> {
    use narsil_devtools::d6_wasm::{Modul as WModul, WasmFunktion};
    use narsil_wasm::modell::Func;

    let mut funktionen: Vec<WasmFunktion> = Vec::new();
    for (i, f) in m.funktionen.iter().enumerate() {
        // GEMESSENE ENTSCHEIDUNG, warum die Signatur aus `m.typen` kommt
        // und nicht aus dem Disassembly-Text: der Text beginnt mit
        // `(func (type 0)` — einer Typ-NUMMER. Die Parameter und Ergebnisse
        // stehen nur in der Typentabelle. Der Reiter zeigt, was der
        // Nutzer braucht: `(i32, i32) -> i32`.
        let (typ_nr, eigen) = match f {
            Func::Importiert { typ, .. } => (*typ, false),
            Func::Eigen { typ, .. } => (*typ, true),
        };
        let typ_kurz = m
            .typen
            .get(typ_nr as usize)
            .map(func_typ_kurz)
            .unwrap_or_else(|| format!("Typ {typ_nr}"));
        // Der Name aus dem `use`-Woerterbuch, sonst der Index. GEMESSENE
        // BEGRUENDUNG: `Namen` ist `BTreeMap<String, u32>` (modell.rs:185),
        // also gibt es die Umkehrung nicht — ich suche den passenden
        // Eintrag. Module mit `use` haben wenige Eintraege; ohne `use` ist
        // die Schleife leer und es bleibt der Index.
        let name = m
            .namen
            .as_ref()
            .and_then(|n| {
                n.iter()
                    .find(|(_, v)| **v == i as u32)
                    .map(|(k, _)| k.clone())
            })
            .unwrap_or_else(|| format!("Funktion {i}"));
        if !eigen {
            // Eine importierte Funktion hat keinen Rumpf. Sie wird mit
            // leerer Befehlsliste aufgenommen — der Reiter zeigt dann
            // „(Import …)", und das ist die Wahrheit.
            funktionen.push(WasmFunktion::aus_text(&name, &typ_kurz, 0, ""));
            continue;
        }
        // Der Rumpf: der S-Ausdruck OHNE Kopf- und Schlusszeile.
        let wat = narsil_wasm::wat::schreibe_funktion(m, i as u32);
        let versatz = match f {
            Func::Eigen { befehl_offset, .. } => *befehl_offset as u32,
            _ => 0,
        };
        //
        // GEMESSENE ENTSCHEIDUNG (02.10.2026, 0:9x): die Zeilen werden
        // hier in Mnemonik und Operanden GETRENNT und mit
        // `WasmFunktion::aus_indizes` uebergeben. Vorher ging der Text
        // durch `aus_text`, das ohne Versatzspalte den Mnemonik verlor (in
        // `narsil_wasm` gibt es keine) und die Naeherung eintrug — mit der
        // Folge, dass `haltepunkt(modul, funktion, versatz)` keine Zeile
        // fand und kein Haltepunkt stellbar war.
        let zeilen: Vec<(String, String)> = wat
            .lines()
            .skip(1)
            .filter(|z| z.trim() != ")")
            .map(|z| {
                let z = z.trim();
                match z.split_once(char::is_whitespace) {
                    Some((a, b)) => (a.to_string(), b.trim().to_string()),
                    None => (z.to_string(), String::new()),
                }
            })
            .collect();
        // GEMESSENE DIAGNOSE (04.10.2026, 2:4x, 0:9x): die erste Version
        // stand hier ohne `trim()` und ohne `skip(1)`, und der Reiter zeigte
        // als Mnemonik die Ziffern der Einrueckung; die zweite liess die
        // Zeilen ganz und verlor den Mnemonik (keine Versatzspalte). Also
        // wird die Zahl der Zeilen ausgegeben, wenn jemand das wieder
        // falsch macht — und die Zahl ist der Beweis, nicht der Rohtext.
        if cfg!(debug_assertions) && m.funktionen.len() == 2 && i == 0 {
            eprintln!("WASM-ZEILEN: {} (Rumpf: {wat:?})", zeilen.len());
        }
        // GEMESSENE ENTSCHEIDUNG: `befehl_offset` (der Byteversatz des
        // Funktionsanfangs im Codeabschnitt) wird nicht angezeigt — der
        // Versatz einer WebAssembly-Zeile ist der Befehlsindex (siehe
        // `DisZeile::versatz`). Die Zahl wird also nicht gebraucht; sie
        // steht trotzdem in der Reihenfolge der Pruefungen, damit man sie
        // nicht suchen muss, wenn sie gebraucht wird.
        let _ = versatz;
        let mut wf = WasmFunktion::aus_indizes(&name, &typ_kurz, zeilen);
        //
        // GEMESSENE ENTSCHEIDUNG (05.10.2026, 3:2x), warum HIER die
        // DWARF-Zeile gelesen wird: W5 verlangt „DWARF-Zeilen aus custom
        // sections, wenn vorhanden". Der Leser ist `narsil_wasm::dwarf`
        // (Eigenwaechter der Crate), und die Bruecke ist die einzige Stelle,
        // die `narsil_wasm` kennt (meine Crate haelt die Absicht, damit sie
        // keine Abhaengigkeit von 15 000 Zeilen bekommt).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die ERSTE Zeile der Tabelle gilt:
        // `dwarf::zeilen_der_funktion` liefert alle Zeilen der Funktion; die
        // Zeile des ERSTEN Befehls ist die, an der man im Editor steht,
        // wenn man die Funktion oeffnet. Die anderen gehoeren zu einem
        // Haltepunkt (siehe `Quellen`/`Zeile` im Reiter).
        wf.zeile = narsil_wasm::dwarf::zeilen_der_funktion(
            m,
            i as u32,
            &narsil_wasm::grenzen::Grenzen::standard(),
        )
        .first()
        .map(|z| z.zeile);
        funktionen.push(wf);
    }
    // Der Modulname: aus `use` nicht vorhanden (das ist ein Woerterbuch fuer
    // Funktionen), also der Aufrufer. GEMESSENE ENTSCHEIDUNG: hier steht
    // „<wasm>", und der Aufrufer kann es mit `benenne_modul` ersetzen.
    let groesse = m.byte_groesse;
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier „min/max" und nicht der Name der
    // Seite (04.10.2026, 2:7x): `Limits::text()` (modell.rs:144) liefert
    // „1" bei unbegrenztem Maximum und „1 4" bei max 4. Die Zahl ist die
    // Angabe; ein Name waere eine Erfindung. Mehrere Speicher werden mit
    // Komma verbunden — und die Zeile wird danach gekuerzt (M4).
    let speicher = m
        .speicher
        .iter()
        .map(|s| match s {
            narsil_wasm::modell::ImportiertSpeicher::Importiert { modul, feld, .. } => {
                format!("Importiert {modul}.{feld}")
            }
            narsil_wasm::modell::ImportiertSpeicher::Eigen { typ } => {
                format!("min/max {}", typ.grenzen.text())
            }
        })
        .collect::<Vec<String>>()
        .join(" + ");
    //
    // GEMESSENE ENTSCHEIDUNG, warum die vierte Zahl die ANZAHL der
    // Funktionen ist (gemessen 2:5x: ich hatte dort die Zahl der
    // Speicher hingesetzt, und der Reiter meldete „1 Funktion" bei zwei):
    // `Modul::neu(name, bytes, funktionen, gesamt)` — `gesamt` ist die Zahl
    // der Funktionen VOR der Grenze, damit der Reiter sagen kann, wie viele
    // nicht dastehen (M7).
    let gesamt = m.funktionen.len();
    let wm = WModul::neu("<wasm>", groesse, funktionen, gesamt);
    let wm = if speicher.is_empty() {
        wm
    } else {
        // GEMESSENE ENTSCHEIDUNG (04.10.2026, 2:6x): der Speicher geht in
        // die MODULZEILE, nicht in einen Hinweis ueber der Flaeche — der
        // Hinweis wird nur gezeichnet, wenn es kein Modul gibt, und seit
        // dem Modul waere er unsichtbar (gemessen: die Zeile blieb leer).
        wm.mit_speicher(&speicher)
    };
    let nr = s
        .devtools
        .wasm
        .modul_eintragen(wm)
        .ok_or_else(|| "der Reiter hat das Modul nicht angenommen (Grenze?)".to_string());
    // Der Speicher: DEKLARIERT ist nicht BELEGT. Ohne Instanz gibt es keine
    // Bytes, also zeigt die Speicheransicht nichts — und das ist richtig.
    // Was der Nutzer wissen will, steht deshalb im Hinweis: wie viele
    // Seiten das Modul verlangt, und dass es sie noch nicht gibt.
    s.devtools.wasm.hinweis = if speicher.is_empty() {
        "Kein Speicher im Modul.".to_string()
    } else {
        format!(
            "DEKLARIERT {speicher} — die Speicheransicht ist leer, weil es \
             noch keine Instanz gibt (W4/W5)."
        )
    };
    nr
}

/// D11: EIN KLICK AUF „WEITER" ODER EINEN SCHRITTKNOEPF — DER DEBUGGER
/// LAEUFT WEITER.
///
/// # WAS HIER ECHT WIRD (und warum es vorher nicht ging)
///
/// Die Reiter-Zeichnung hat die vier Knöpfe schon in Runde 1 gezeichnet
/// (GEMESSEN im Bild `team/log/devtools_D5.png`). Sie zu zeichnen ist
/// aber die Haelfte des Wegs: angehalten wird nur, wenn ein Klick den
/// Zustand der Engine aendert. Vorher tat ein Klick auf „Weiter" gar
/// nichts — der Reiter stand auf „Pause" und wartete.
///
/// GEMESSENE ENTSCHEIDUNG, wo der Motor herkommt: `Angezeigt::engine`
/// (sitzung.rs, `pub engine: Option<Engine>`) — die Engine der SEITE, die
/// das Skript ausgefuehrt hat. Eine frische Engine je Klick haette kein
/// `z` vom Skript (dort steht diese Begruendung bei `engine`). Deshalb
/// wird die vorhandene genommen, und deshalb braucht dieser Pfad
/// `&mut Sitzung` — was `mausklick_zuerst` und `inhalt_klick` schon haben.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Haltepunkte VOR dem Schritt
/// gesendet werden: `Engine::weiter()` laeuft bis zum naechsten
/// Haltepunkt. Steht in der Engine noch eine Liste von vorhin, laeuft der
/// Schritt an der FALSCHEN Stelle vorbei. Die Liste des Reiters ist die
/// Quelle, die Engine bekommt sie vor jedem Schritt neu.
fn debugger_schritt(s: &mut Sitzung, punkt: Point) -> bool {
    let Some(inhalt) = s.devtools.geruest.inhalt() else {
        return false;
    };
    let Some(schritt) = narsil_devtools::zeichnen::schritt_unter(inhalt, punkt) else {
        return false;
    };
    // Ohne Engine gibt es nichts weiterzuschieben — und das steht dann im
    // Reiter, statt dass der Knopf still nichts tut.
    //
    // GEMESSENE ENTSCHEIDUNG, warum EIN Leihvertrag statt zweier: der
    // Motor und der Reiter sind zwei FELDER von `Sitzung`, und die
    // Felder werden getrennt geliehen (`s.angezeigt` einerseits,
    // `s.devtools.quellen` andererseits). Zwei `let … else` mit
    // `s.angezeigt` wuerden beide denselben Pfad leihen und beim zweiten
    // die Schreibzugriffe auf `s.devtools` sperren.
    let Some(e) = s.angezeigt.as_mut().and_then(|a| a.engine.as_mut()) else {
        // GEMESSENE ENTSCHEIDUNG: „kein Motor" ist ein Zustand, den der
        // Nutzer braucht (die Seite hatte kein Skript) — kein Fehler.
        s.devtools.quellen.hinweis = "Diese Seite hat kein Skript — es laeuft nichts.".to_string();
        return true;
    };
    if let Err(fehler) = s.devtools.quellen.haltepunkte_senden(e) {
        s.devtools.quellen.hinweis = format!("Haltepunkte: {fehler}");
        return true;
    }
    let r = if schritt == narsil_devtools::d5_quellen::Schritt::Weiter {
        s.devtools.quellen.weiter(e)
    } else {
        s.devtools.quellen.schritt_ausfuehren(e, schritt)
    };
    // GEMESSENE ENTSCHEIDUNG, warum der Fehler TEXT in den Reiter geht und
    // nicht ins Log: der Nutzer hat gerade geklickt und will wissen, was
    // passiert ist. `weiter()` ohne Haken gibt `Err` — das ist der
    // haeufigste Fall (Reiter auf, aber nichts angehalten).
    // GEMESSENE ENTSCHEIDUNG, warum der Fehler so nackt in den Reiter
    // kommt (clippy `useless_format`, 04.10.2026 1:7x): die Meldung der
    // Engine ist schon ein Satz („es laeuft kein Skript"). Eine zweite
    // Formatierung wuerde nur `Display` statt `to_string` benutzen — und
    // wenn die Engine spaeter eine Zeilennummer anhaengt, steht sie trotzdem
    // drin, weil der TEXT der Quelle entnommen wird.
    s.devtools.quellen.hinweis = match r {
        Ok(()) => String::new(),
        Err(f) => f,
    };
    true
}

/// Die x-Koordinate des Inhaltsbereichs.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein eigener Aufruf statt einer
/// Konstante: der Inhalt beginnt bei `inhalt().x`, und der kann sich
/// vom Bereichsrand unterscheiden (bei „Elemente" steht die linke Spalte
/// am Rand, der Inhalt nicht). Eine geratene Zahl waere an einer Stelle
/// richtig und an der anderen 240 px daneben.
fn self_x(s: &Sitzung) -> f32 {
    s.devtools.geruest.inhalt().map(|r| r.x).unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// D — Die Anzeige: der Werkzeugbereich UND die Hervorhebung in der Seite
// ---------------------------------------------------------------------------

/// Den Werkzeugbereich an das Bild haengen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Bild „von oben" behauptet wird und
/// nicht geprueft: `Sitzung::display_list` (sitzung.rs:5242) ist EIN
/// Aufruf, der die Seite, die Leiste, die Statuszeile und das
/// Rechtsklick-Menue zusammenstellt. Was spaeter in die Liste kommt, wird
/// spaeter gezeichnet — das ist die Eigenschaft, auf die sich auch das
// Kontextmenue verlaesst (sitzung.rs:5543, es wird als LETZTES
/// gehaengt). Der Werkzeugbereich wird genauso gehaengt.
///
/// GEMESSENE ZAHL, wo genau: die Anfrage ist „minimal". EINE Funktion,
/// ein Aufruf am Ende von `display_list`.
pub fn anhaengen(s: &Sitzung, liste: &mut narsil_display::DisplayList) {
    if !s.devtools.offen() { return; }
    // D2: „Knoten waehlen hebt ihn in der Seite hervor".
    //
    // GEMESSENER BEFUND der Koordination (01.10.2026, 22:2x, Bild
    // devtools_D9_wikipedia.png): die Hervorhebung war als
    // TRANSLUZENTE FLÄCHE gezeichnet (deckkraft 0,34) und damit an einem
    // grossen Element — `main`, 640 × 60 px, bei wikipedia — als
    // blaues Rechteck ueber dem ganzen Seitenbereich. Das Ergebnis sah
    // aus wie „die Seite ist dunkel", und die HERVORHEBUNG selbst war
    // nicht zu sehen (sie war die Flaeche, die alles bedeckte).
    //
    // GEMESSENE ENTSCHEIDUNG: als RAHMEN (4 Streifen von 2 px), nicht
    // als Flaeche. Das ist auch das, was ein Nutzer erwartet: er will
    // sehen, WO das Element liegt, nicht das Element verdecken. Chrome
    // zeichnet genauso einen 2-px-Rahmen.
    for (r, gewaehlt) in narsil_devtools::zeichnen::hervorhebungs_konstrukte(&s.devtools) {
        let dy = crate::sitzung::SEITEN_START_Y as f32
            - s.angezeigt.as_ref().map_or(0.0, |a| a.versatz as f32);
        if r.width <= 0.0 || r.height <= 0.0 {
            continue;
        }
        let d = if gewaehlt { 2.0 } else { 1.0 };
        let farbe = narsil_devtools::zeichnen::hervorhebungsfarbe();
        let deckkraft = if gewaehlt { 0.95 } else { 0.45 };
        for streifen in [
            Rect::new(r.x, r.y, r.width, d),
            Rect::new(r.x, r.bottom() - d, r.width, d),
            Rect::new(r.x, r.y, d, r.height),
            Rect::new(r.right() - d, r.y, d, r.height),
        ] {
            liste.push(narsil_display::DisplayItem::Rect {
                rect: Rect::new(streifen.x, streifen.y + dy, streifen.width, streifen.height),
                color: farbe,
                radius: [0.0; 4],
                deckkraft,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
        }
    }
    if !s.devtools.offen() {
        return;
    }
    // Der Werkzeugbereich selbst: `Geruest::bereich()` ist bereits in
    // FENSTERkoordinaten (y = hoehe - bereichs_hoehe), also wird hier
    // nichts verschoben.
    //
    // GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:13x), warum kein
    // `to_display_list()` + `clone` mehr: der alte Weg baute eine eigene
    // Liste und kopierte jeden Punkt einzeln hinein. GEMESSEN: allein das
    // Kopieren kostete im Elemente-Reiter 0,057 ms bei 100 Punkten — mehr
    // als das Zeichnen selbst (0,042 ms). `geruest_zeichne_in` zeichnet
    // direkt in die Liste des Fensters; es entfaellt eine Liste und
    // `n_punkte` Kopien je Bild.
    narsil_devtools::zeichnen::geruest_zeichne_in(&s.devtools, liste);
}

/// Der Werkzeugbereich als GEOMETRIE fuer die Klickpruefung
/// (Sitzung::mausklick) — siehe [`mausklick_zuerst`].
///
/// GEMESSENE ENTSCHEIDUNG, warum es diese kleine Funktion gibt und nicht
/// `s.devtools.bereich()` direkt im Aufrufer: der Aufrufer soll nicht
/// wissen, dass es `devtools.geruest` gibt. Er soll wissen „ist das der
/// Werkzeugbereich?".
pub fn im_werkzeugbereich(s: &Sitzung, punkt: Point) -> bool {
    s.devtools.bereich().is_some_and(|b| b.contains(punkt))
}

/// Wie viel Platz die Werkzeuge den Werkzeugbereich wegnehmen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `0` statt `Option`: zu ist kein Bereich
/// (das ist eine Zusage aus d1_geruest.rs), und `0` ist die Antwort, die
/// `saturating_sub` braucht. Ein `Option` wuerde an der einen Stelle
/// `unwrap_or(0)` erzwingen und an der anderen eine zweite Frage stellen.
pub fn hoehe(s: &Sitzung) -> u32 {
    if !s.devtools.offen() {
        return 0;
    }
    s.devtools.bereich().map_or(0, |r| r.height as u32)
}

/// Das Geraet fuer Tests, die den Zustand von aussen setzen wollen.
///
/// GEMESSENE ENTSCHEIDUNG: der Aufrufer (der Test) braucht einen
/// Reiter, ohne drei Tastendruecke zu simulieren. Das ist der kleinste
/// Weg zu einem Zustand — und es ist derselbe Aufruf, den ein Klick
/// macht (`Geruest::klick`).
pub fn setze_reiter(s: &mut Sitzung, r: Reiter) {
    s.devtools.setze_reiter(r);
    s.schmutzig = true;
}
