//! UI-VORLAGE SCHRITT 5 — DIE EINSTELLUNGEN IM FENSTER.
//!
//! # WER ZEICHNET WAS
//!
//! GEMESSEN an der Rollenverteilung (`team/WUNSCH_UI.txt`, Grundregel oben):
//! `narsil-chrome` ist ZUSTAND und GEOMETRIE, `narsil` zeichnet. Die Rolle UI
//! hat `crates/narsil-chrome/src/einstellungen.rs` fertig (25 Tests, Bilder
//! `ui1_*_a_einst_*.png`). Was fehlte, war der Aufruf — GEMESSEN, weil
//! `Sitzung::menue_aktion` bei `MenueAktion::Einstellungen` bis heute
//! `self.meldung = Some("Einstellungen kommen mit U4")` schrieb (also seit
//! dem Umbau der alten flachen Liste, die 4 Zeilen ohne Kategorien hatte).
//!
//! Diese Datei ist die Bruecke: sie haelt den Zustand (die Einstellungen und
//! die Seite), liest und speichert ueber `narsil_chrome::einstellungen` und
//! reicht Zeichnen und Klick durch. Sie aendert keine fremde Datei.
//!
//! # GEMESSENE ENTSCHEIDUNGEN (mit Grund, nicht aus Uebersicht)
//!
//! 1. **Zustand in der Sitzung, nicht global.** GEMESSEN: `Sitzung::neu`
//!    ist der einzige Konstruktor (`grep "Sitzung {"` findet nur die
//!    Definition), also sind zwei Felder plus eine Zeile in `neu` die
//!    kleinste Aenderung. Eine `thread_local`-Variante wuerde zwei Fenster
//!    teilen — und die Einstellungen gehoeren zum Fenster.
//! 2. **Laden mit `lesen_von`, Fehler nicht als Abbruch.** GEMESSEN: eine
//!    fehlende Datei ist der Normalfall beim ersten Start; die Einstellungen
//!    stehen dann auf den Vorgaben (`Default`). GEMESSENE ENTSCHEIDUNG, warum
//!    kein Absturz: `lesen_von` liefert `Result`, und ein Browser, der sich
//!    wegen einer fehlenden Konfigurationsdatei nicht starten laesst, ist
//!    kaputt. Der Fehlertext geht in die Meldung — versteckt wird er nicht.
//! 3. **Speichern nur nach einer Aktion.** GEMESSEN: `schreiben_nach` nach
//!    JEDEM Zugriff (auch nur einem Lesen der Anzeige) wuerde die Datei bei
//!    jedem Oeffnen neu schreiben. Gespeichert wird, wenn `ausfuehren` eine
//!    Aenderung gemeldet hat (`true`).

use narsil_chrome::Chrome;
use narsil_chrome::einstellungen::{self, Aktion, Einstellungen, Seite};
use narsil_i18n::{Language, tr, trf};

/// Applies the UI language of freshly loaded settings (at start-up).
///
/// Test processes skip this: all tests of one process share one profile, so a
/// language saved by one test would switch the UI of every other test.
/// Tests choose their language with `narsil_i18n::scoped` or
/// `NARSIL_LANGUAGE` instead.
pub fn apply_startup_language(e: &Einstellungen) {
    if einstellungen::laeuft_als_test() {
        return;
    }
    narsil_i18n::set_language(e.language);
}

/// Applies a language the user just chose in the settings. Takes effect
/// without a restart: every UI string is looked up when it is drawn.
///
/// In a test process without a scoped language the change stays on the
/// calling thread, so parallel tests keep their language.
pub fn apply_chosen_language(language: Language) {
    if einstellungen::laeuft_als_test() && !narsil_i18n::has_thread_language() {
        narsil_i18n::set_thread_language(language);
    } else {
        narsil_i18n::set_language(language);
    }
}

/// Ist das die Einstellungs-Seite? (beide Schreibweisen gehen)
pub fn ist_einstellungen(adresse: &str) -> bool {
    // Release 08.10.2026: auch mit Anker (`about:einstellungen#features`).
    matches!(
        adresse.trim().split('#').next().unwrap_or(""),
        "about:einstellungen" | "about:settings"
    )
}

/// Die Einstellungen laden (Vorgabe, wenn die Datei fehlt oder kaputt ist).
///
/// Liefert `(einstellungen, gemeldeter_fehler)` — der Fehler ist ein Text fuer
/// die Statuszeile, kein Abbruch (siehe Punkt 2 im Modul-Kopf).
pub fn laden() -> (Einstellungen, Option<String>) {
    let pfad = einstellungen::pfad();
    match einstellungen::lesen_von(&pfad) {
        Ok(e) => (e, None),
        Err(f) => (
            Einstellungen::default(),
            Some(trf!("Settings unreadable ({}), defaults used", f)),
        ),
    }
}

/// Die Zeichenschicht der Einstellungen an die Fensterliste haengen.
pub fn anhaengen(
    seite: &Seite,
    e: &Einstellungen,
    chrome: &Chrome,
    breite: u32,
    hoehe: u32,
    ziel: &mut narsil_display::DisplayList,
) {
    // Release 09.10.2026: `anhaengen` — das Glas der Kategorienliste kommt mit.
    ziel.anhaengen(&einstellungen::display_list(seite, e, chrome, breite, hoehe));
}

/// WAS DER KLICK BEWIRKT HAT — alles zum Kopieren, ohne Geliehene.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine eigene kleine Struktur und nicht
/// vier Callbacks (`thema_setzen`, `zoom_setzen`, …): GEMESSEN am Compiler,
/// warum die Callback-Variante nicht geht. Der Klick braucht `&mut Seite`
/// UND `&mut Einstellungen` (beide Felder der Sitzung) und soll danach
/// `&mut Sitzung` an `zoom_setzen` geben — drei gleichzeitige geliehene
/// `&mut` auf derselben Struktur kann Rust nicht auseinanderhalten, weil
/// der Pfad durch die Struktur laeuft (`&mut self.einstellungen` und
/// `&mut self` ueberschneiden sich). Mit dieser Struktur liest der Klick
/// die vier Werte als Kopien heraus, und der Aufrufer setzt sie danach an
/// seiner eigenen Sitzung — jederzeit, ohne Leihen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Werte hier UND die Speicherung
/// getrennt sind: das Speichern gehoert dem Aufrufer (er hat den Pfad und
/// die Meldung), die Aenderung dem Modul (`narsil-chrome`). Wer beides an
/// einer Stelle macht, kann nicht mehr pruefen, ob das Speichern der
/// GEZEUPTEN Datei schadet — und genau das ist der Test unten.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wirkung {
    /// Hat sich ueberhaupt etwas bewegt (dann: Seite neu zeichnen)?
    pub geaendert: bool,
    pub erweiterungen_laden: bool,
    /// Muss in die Datei geschrieben werden?
    pub speichern: bool,
    /// Die neue Schriftgroesse in px (nur wenn sie sich geaendert hat).
    pub schriftgroesse: Option<u16>,
    /// Das neue Thema (nur wenn es sich geaendert hat).
    pub thema: Option<narsil_chrome::thema::Thema>,
    /// Der neue Zoomfaktor (nur wenn sie sich geaendert hat).
    pub zoom: Option<f32>,
    /// Die neue Suchmaschine (nur wenn sie sich geaendert hat).
    pub suchmaschine: Option<narsil_chrome::suche::Suchmaschine>,
    /// Ein Text fuer die Meldungszeile (Fehler, oder was sich geaendert hat).
    pub meldung: Option<String>,
    /// Release 08.10.2026: ein Schalter im Tab „Features" hat sich geaendert
    /// (die Sitzung setzt dann ihre Haken neu).
    pub features: bool,
    /// Release 08.10.2026: eine eingebaute Seite einer Funktion oeffnen.
    pub oeffnen: Option<String>,
    /// The new UI language (only when it changed).
    pub language: Option<Language>,
}

/// DIE FRAGE, die der Knopf beim ersten Klick stellt.
///
/// GEMESSENE ENTSCHEIDUNG (04.10.2026, geaendert): die Worte stehen jetzt
/// in `narsil_chrome::einstellungen::FRAGE_STANDARDBROWSER`. GEMESSENER
/// Grund: die Frage muss im **Bild** stehen, und das Bild zeichnet
/// `einstellungen.rs`. An dieser Stelle standen sie seit heute frueh — die
/// Begruendung damals („nennt den Prozess beim Namen") war richtig, der Ort
/// war es nicht: eine Zeichenfolge an zwei Orten ist eine, die
/// auseinanderlaufen kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein Dialog: `narsil-chrome` hat keinen
/// (Modulkopf: keine Fremd-Crate), und die Zeile, in der Firefox seinen
/// Hinweis schreibt, gibt es dort als `hinweis_zeile`.
pub use narsil_chrome::einstellungen::FRAGE_STANDARDBROWSER;

/// DER TEXT, den der Knopf „Als Standardbrowser festlegen" zurueckgibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein FALSCHER (`Ok`) und kein leerer
/// Erfolg ist: `ausfuehren` behauptete fuer `Aktion::Standardbrowser`
/// `true` — „hat sich etwas geaendert" — und `a_wert_ist_setzbar` likewise,
/// also wurde die Einstellungsdatei geschrieben, **obwohl sich kein Wert
/// geaendert hatte** und **kein Befehl lief**. GEMESSEN an
/// `narsil_chrome/src/einstellungen.rs`: `Aktion::Standardbrowser =>
/// true`, und die Datei `Einstellungen` hat gar kein Feld dafuer. Ein Knopf,
/// der speichert und sonst nichts, ist die Form, in der „nur Optik" aussieht.
fn standardbrowser_meldung() -> String {
    match standardbrowser_setzen() {
        Ok(text) => text,
        Err(grund) => trf!("Default browser not set: {}", grund),
    }
}

/// DIESEN BROWSER ZUM STANDARDMACHT — der Befehl laeuft hier.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Prozess in `narsil` startet und nicht in
/// `narsil-chrome`: der Modulkopf von `einstellungen.rs` sagt es selbst —
/// „das Setzen als Standardbrowser ruft `xdg-settings` auf — das ist ein
/// Programm, und Programme gehoeren in die Crate, die den Prozess startet",
/// und `narsil-chrome` kennt ausser `narsil-browser` keine Fremd-Crate. Der
/// BEFEHL und der NAME stehen als Konstanten in `narsil-chrome` (dort faellt die
/// Entscheidung), AUSGEFUEHRT wird hier.
///
/// GEMESSENE ENTSCHEIDUNG, warum `true` (Erfolg) nicht `stdout` ist:
/// `xdg-settings` schweigt auch dann, wenn es nichts getan hat. Die
/// ENTSCHEIDUNG ist der Exitcode, und der Text ist die Ausgabe fuer den,
/// der nachfragt — beides wird getrennt gemeldet, damit „getan" und
/// „gesagt" nicht verwechselt werden.
pub fn standardbrowser_setzen() -> Result<String, String> {
    // GEMESSENE ENTSCHEIDUNG, warum `STANDARD_BROWSER_BEFEHL` GESPALTEN und
    // nicht als eine Zeile an `sh -c` gehaengt wird: `xdg-settings` und der
    // Name sind zwei Argumente, und ein Befehl als Shell-Kette ist eine
    // Zeichenkette, in der ein Name mit Leerzeichen drei Argumente wird.
    // `split_whitespace` traegt die Reihenfolge aus der Konstanten mit
    // (siehe deren Kommentar: „ein Vertauschen ergibt einen Standardbrowser
    // mit einem falschen Namen").
    let mut teile = einstellungen::STANDARD_BROWSER_BEFEHL.split_whitespace();
    let program = teile.next().unwrap_or("xdg-settings");
    let args: Vec<&str> = teile.collect();
    // GEMESSENE ENTSCHEIDUNG, warum die Zahl der Argumente geprueft wird und
    // nicht die Laenge des Programms: `split_whitespace` gibt nie einen
    // leeren Programmnamen, aber sehr wohl **null** Argumente, wenn die
    // Konstante leer waere — und `xdg-settings` mit keinem
    // `set default-web-browser` behaelt den Standardbrowser und meldet
    // trotzdem Erfolg. Das ist der Fall, in dem der Knopf luegt.
    if args.len() < 2 {
        return Err(trf!(
            "the command {} has fewer than two arguments",
            format!("{:?}", einstellungen::STANDARD_BROWSER_BEFEHL)
        ));
    }
    if einstellungen::STANDARD_BROWSER_NAME.chars().count() > einstellungen::MAX_DESKTOP_NAME {
        return Err(trf!(
            "the name {} is longer than {} characters",
            format!("{:?}", einstellungen::STANDARD_BROWSER_NAME),
            einstellungen::MAX_DESKTOP_NAME
        ));
    }
    let ausgabe = std::process::Command::new(program)
        .args(&args)
        .arg(einstellungen::STANDARD_BROWSER_NAME)
        .output();
    match ausgabe {
        // GEMESSENE ENTSCHEIDUNG, warum die Ausgabe (nicht der Status) der
        // Text ist: `xdg-settings` schreibt bei Erfolg nichts und bei
        // Misserfolg den Grund. Wer den Grund nicht weitergibt, hat dem
        // Nutzer einen Knopf gegeben, der sich nicht bewegt.
        Ok(ergebnis) if ergebnis.status.success() => {
            let gesagt = String::from_utf8_lossy(&ergebnis.stderr).trim().to_string();
            if gesagt.is_empty() {
                Ok(trf!(
                    "{} set with {}",
                    einstellungen::STANDARD_BROWSER_NAME,
                    program
                ))
            } else {
                Ok(trf!(
                    "{} set with {}: {}",
                    einstellungen::STANDARD_BROWSER_NAME,
                    program,
                    gesagt
                ))
            }
        }
        Ok(ergebnis) => {
            let grund = String::from_utf8_lossy(&ergebnis.stderr).trim().to_string();
            Err(if grund.is_empty() {
                trf!(
                    "{} reported exit code {}",
                    program,
                    ergebnis.status.code().unwrap_or(-1)
                )
            } else {
                grund
            })
        }
        // GEMESSENE ENTSCHEIDUNG, warum ein FEHLENDER BEFEHL ein Fehler mit
        // Text ist und kein Absturz: `xdg-settings` gehoert nicht zu jedem
        // System, und ein Browser, der wegen eines fehlenden Programms
        // stirbt, ist kaputt (dieselbe Begruendung wie beim Laden der
        // Datei, Punkt 2 im Modulkopf).
        Err(f) => Err(trf!("Cannot start {}: {}", program, f)),
    }
}

/// Der Klick: Treffer -> Klick -> Aktion -> Wirkung.
///
/// `punkt` ist der Fensterpunkt. `None` heisst: der Punkt hat nichts
/// getroffen (dann bleibt alles, wie es war — auch die Datei).
///
/// GEMESSENE ENTSCHEIDUNG, warum WIR die Wirkung bestimmen und nicht
/// `narsil_chrome`: `WUNSCH_UI.txt` sagt es ausdruecklich
/// (`e.zoom -> Sitzung::zoom_setzen`, `e.thema -> die Oberflaeche`), und
/// das sind Felder der Sitzung, nicht der Chrome-Form.
pub fn klick(
    seite: &mut Seite,
    e: &mut Einstellungen,
    chrome: &Chrome,
    punkt: narsil_core::geometry::Point,
    breite: u32,
    hoehe: u32,
) -> Option<Wirkung> {
    let vorher = e.clone();
    let t = einstellungen::treffer(chrome, seite, e, &punkt, breite, hoehe)?;
    let mut a: Aktion = einstellungen::klick(seite, e, t);
    //
    // DER SCHIEBER: EIN KLICK AUF DEN BALKEN SETZT DIE ZAHL.
    //
    // GEMESSENER FEHLER (03.10.2026, 18:2x, Leitung: „alle funktionsfaehig,
    // nicht nur Optik"): GEMESSEN am ganzen Baum —
    // `grep -rn schieber_aus_pixel crates/` findet **nur**
    // `narsil-chrome/src/einstellungen.rs` (die Funktion und ihre eigenen
    // Tests). `narsil` rief sie **nie** auf, und `einstellungen::klick`
    // gibt fuer `Bedienung::Schieber` ausdruecklich `Aktion::Nichts`
    // zurueck (dort steht: „das ZIEHEN des Griffs ist eine eigene Frage
    // (`schieber_aus_pixel`), und `narsil` stellt sie"). GEMESSENE FOLGE:
    // **die Frage wurde nie gestellt.** Beide Schieber der Einstellungen
    // (Zoom, Schriftgroesse) waren im Bild da und reagierten auf NICHTS —
    // und `ausfuehren` gab trotzdem `true` zurueck („etwas hat sich
    // geaendert"), also `geaendert: true`, `schmutzig`, und eine Meldung,
    // die nichts sagte. Zwei der sechs Dinge aus ANFORDERUNGEN Zeile 9 waren
    // reine Optik.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der KLICK den Wert setzt und nicht das
    // ZIEHEN: (1) `narsil` bekommt `ButtonPressed` und mehr nicht — ein
    // Ziehen braucht `ButtonMoved`, und das wird in `narsil/src/` nirgends
    // behandelt (`grep -rn "ButtonMoved|Motion" narsil/src/` trifft nur
    // `MotionNotify` in `main.rs`, das ist das FENSTER, nicht die Seite);
    // (2) Firefox laesst den Griff einem Klick auf den Balken folgen, und
    // das ist die Kuerze, die ohne Ziehen auskommt; (3) die Entscheidung
    // gehoert nicht in `narsil-chrome`, denn `einstellungen::klick` kennt den
    // Punkt nicht — es bekommt nur ein `Treffer::Element(Feld)`. Der Ort,
    // an dem der Punkt ANKOMMT, ist diese Bruecke.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Index aus der FELDERLISTE kommt und
    // nicht als Zahl dasteht: `element()` nimmt die Position des Feldes in
    // seiner Kategorie. Eine festgetragene Zahl waere eine zweite Wahrheit
    // ueber die Reihenfolge (dieselbe Begruendung wie im Test
    // `ein_klick_auf_einen_wert_aendert_und_speichert`).
    if let einstellungen::Treffer::Element(f) = t {
        if f.bedienung() == einstellungen::Bedienung::Schieber {
            if let Some((_, min, max)) = f.schieber(e) {
                // GEMESSENE ENTSCHEIDUNG, warum die Liste aus der SEITE
                // kommt: `Einstellungen` weiss nichts davon, welche Kategorie
                // gerade offen ist — das steht in `Seite::kategorie`, und
                // `felder()` gibt eine `&'static`-Liste zurueck (also keine
                // geliehene `&mut` mitten im Klick).
                if let Some(index) = seite.kategorie.felder().iter().position(|x| *x == f) {
                    // GEMESSENE KORREKTUR (04.10.): `element()` nimmt seit
                    // dem Umbau auf Abschnitten (Panel 4) die **Kategorie**
                    // mit — die Oberkante eines Feldes haengt an den
                    // Abschnittsueberschriften ueber ihm, und die weiss nur
                    // die Kategorie.
                    let r = einstellungen::element(seite.kategorie, chrome, f, index, breite);
                    let px = if punkt.x < 0.0 { 0 } else { punkt.x as u32 };
                    let wert = einstellungen::schieber_aus_pixel(px, &r, min, max);
                    // Der Griff folgt dem Klick: die Mitte des Balkens ist
                    // der Punkt, an dem man ihn erwartet, und genau dort
                    // steht der Griff danach.
                    a = Aktion::Schieben(f, wert);
                }
            }
        }
    }
    // GEMESSENE ENTSCHEIDUNG, warum `vorher` mitkommt: `ausfuehren` gibt nur
    // `true` zurueck, wenn sich etwas GEANDERT hat. Ohne Vorher koennte der
    // Aufrufer nicht sagen, ob er die Seite neu zeichnen und speichern muss.
    if !einstellungen::ausfuehren(e, &a) {
        return None;
    }
    let mut w = Wirkung {
        geaendert: true,
        erweiterungen_laden: matches!(a, Aktion::ErweiterungLaden),
        speichern: a_wert_ist_setzbar(&a),
        ..Wirkung::default()
    };
    // Die Wirkung, Feld fuer Feld (WUNSCH_UI.txt Punkt „Wirkung sofort
    // sichtbar").
    if e.schriftgroesse != vorher.schriftgroesse {
        // GEMESSENE ENTSCHEIDUNG, warum hier NUR gemeldet wird: die
        // Schriftgroesse der Seite gehoert `narsil-text`/`narsil-layout` und
        // damit einer anderen Rolle (Wunsch in `WUNSCH_UI.txt`). Der Wert
        // wird trotzdem zurueckgegeben — er steht dann in der Meldung, und
        // die Zahl ist nachschlagbar statt verloren.
        w.schriftgroesse = Some(e.schriftgroesse);
        w.meldung = Some(trf!(
            "Font size {} px (page font under `Font` in the settings)",
            e.schriftgroesse
        ));
    }
    if e.thema != vorher.thema {
        w.thema = Some(e.thema);
    }
    if e.zoom != vorher.zoom {
        w.zoom = Some(e.zoom_faktor());
    }
    if e.suchmaschine != vorher.suchmaschine {
        w.suchmaschine = Some(e.suchmaschine);
    }
    // DER KNOPF „ALS STANDARDBROWSER FESTLEGEN" MACHT JETZT ETWAS.
    //
    // GEMESSENER FEHLER (03.10.2026, 18:1x): `Aktion::Standardbrowser`
    // lief in `einstellungen::ausfuehren` in die Zeile `true` neben
    // `Nichts`, `Kategorie` und `Klappe` — also „hat sich etwas geaendert".
    // GEMESSENE FOLGE, beides mit einer Zahl: (1) `a_wert_ist_setzbar`
    // gab ebenfalls `true`, also **schrieb die Datei**, ohne dass sich ein
    // Wert geaendert hatte; (2) `narsil_chrome::einstellungen::Einstellungen`
    // hat **kein Feld** dafuer — es gab nichts zu speichern. GEMESSEN an
    // `sitzung.rs`: `w.thema`, `w.zoom`, `w.suchmaschine` werden gesetzt,
    // und fuer den Knopf steht dort **nichts**. Der Knopf war gezeichnet,
    // getroffen, meldete „geaendert" — und tat nichts. Das ist genau die
    // Form, in der „nur Optik" aussieht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum es hier passiert und nicht in
    // `sitzung.rs`: der Prozess gehoert in die Crate, die Programme startet
    // (Modulkopf `einstellungen.rs`), und `Wirkung::meldung` landet
    // bereits in `sitzung.rs` (`self.meldung = Some(m)`). Damit wirkt der
    // Knopf, **ohne** dass eine fremde Datei angefasst wird.
    //
    // DIE FRAGE (04.10.2026, Leitung: „nur nach Rueckfrage"). Der erste
    // Klick auf den Knopf sagt, WAS passieren wuerde, und tut es nicht.
    // GEMESSENE ENTSCHEIDUNG, warum der Text hier steht und nicht in
    // `narsil-chrome`: er ist die **Antwort eines Programms** (`xdg-settings`
    // nennt sich selbst, wenn es fehlschlaegt), und die Antwort gehoert an
    // die Stelle, die den Prozess startet. Die Frage selbst ist ein Text
    // ueber die Handlung — die gehoert in die Datei, die die Handlung kennt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Frage NICHT in `einstellungen.rs`
    // steht, obwohl der Knopf dort ist: `einstellungen.rs` hat keinen
    // Prozess und soll auch keinen bekommen (Modulkopf). Wer die Frage
    // dort baute, haette entweder den Prozess dort (dann gehoert die
    // Datei nicht mehr in `narsil-chrome`) oder eine zweite Zeichenfolge
    // fuer dieselben Worte.
    if matches!(a, Aktion::StandardbrowserFrage) {
        w.meldung = Some(tr(FRAGE_STANDARDBROWSER).to_string());
    }
    //
    // DIE AUSFUEHRUNG — erst jetzt, nach der Rueckfrage.
    if matches!(a, Aktion::Standardbrowser) {
        w.meldung = Some(standardbrowser_meldung());
    }
    // Release 08.10.2026: der Tab „Features".
    if e.features != vorher.features {
        w.features = true;
        if let Aktion::Schalten(einstellungen::Feld::Feature(f), an) = &a {
            w.meldung = Some(format!("{}: {}", f.name(), if *an { tr("on") } else { tr("off") }));
        }
    }
    if let Aktion::Verwalten(f) = &a {
        w.oeffnen = f.verwalten().map(|(_, adresse)| adresse.to_string());
    }
    if e.language != vorher.language {
        w.language = Some(e.language);
    }
    Some(w)
}

/// Nur die Aktionen, die einen Wert wirklich aendern, loesen ein Speichern
/// aus. GEMESSEN: `Aktion::Kategorie` und `Aktion::Klappe` sind Navigation.
///
/// GEMESSENE KORREKTUR (03.10.2026, 18:1x): `Aktion::Standardbrowser` stand
/// hier drin und hat die Einstellungsdatei geschrieben, obwohl **kein Wert**
/// in ihr steht, der sich geaendert haette. Der Knopf setzt jetzt den
/// Standardbrowser (ein Programm) und speichert nichts — eine Einstellung,
/// die es nicht gibt, gehoert nicht in die Datei.
fn a_wert_ist_setzbar(a: &Aktion) -> bool {
    // GEMESSENE KORREKTUR (04.10.2026): `Aktion::StandardbrowserFrage` ist
    // hinzugekommen — es ist wie `Kategorie` und `Klappe` **Navigation**
    // (eine Rueckfrage aendert keinen gespeicherten Wert), also ebenfalls
    // kein Speichern.
    !matches!(
        a,
        Aktion::Nichts
            | Aktion::Kategorie(_)
            | Aktion::Klappe(_)
            | Aktion::StandardbrowserFrage
            | Aktion::Standardbrowser
            | Aktion::ErweiterungLaden
            // Release 08.10.2026: (i) und der Knopf zur Seite aendern keinen Wert.
            | Aktion::Info(_)
            | Aktion::Verwalten(_)
    )
}

/// Die Einstellungen schreiben. Fehler gehen in die Meldung, nicht in einen
/// Absturz (GEMESSEN: die Datei liegt in `~/.config/narsil/`, und ein
/// fehlender Ordner ist kein Grund, den Browser zu beenden).
pub fn speichern(e: &Einstellungen, meldung: &mut dyn FnMut(String)) {
    let pfad = einstellungen::pfad();
    if let Err(f) = einstellungen::schreiben_nach(&pfad, e) {
        meldung(trf!("Cannot save settings: {}", f));
    }
}

#[cfg(test)]
mod tests {
    //! DIE ZWEI BEHAUPTUNGEN UEBER DEN KNOPF — an der Funktion, nicht am Bild.
    //!
    //! GEMESSENE ENTSCHEIDUNG, warum `a_wert_ist_setzbar` hier und nicht im
    //! Bild geprueft wird: ein Klick auf den Knopf, der die Datei schreibt,
    //! ist von aussen **nicht** zu sehen — die Datei bekommt danach denselben
    //! Inhalt. GEMESSEN ist die Behauptung an der Funktion: sie sagt, ob
    //! ueberhaupt geschrieben wird.
    use super::*;
    // GEMESSENE ENTSCHEIDUNG, warum `Kategorie` **hier** importiert wird und
    // nicht oben bei `use narsil_chrome::einstellungen::{..}` (E0433, nur im
    // Testbau): die obere Zeile wird auch **ohne** `cfg(test)` gebaut, und
    // `Kategorie` wird ausserhalb der Tests nie genannt — dort wuerde der
    // Import eine Warnung `unused_import` erzeugen. Der Typ gehoert zu
    // `Aktion::Kategorie`, und die Behauptung „das schreibt nichts" heisst
    // ausdruecklich auch fuer die Kategorie-Zeile.
    use narsil_chrome::einstellungen::Kategorie;

    /// DER KNOPF SCHREIBT KEINE DATEI.
    ///
    /// GEMESSENER FEHLER (03.10.2026, 18:1x): `a_wert_ist_setzbar` hat
    /// `Aktion::Standardbrowser` mitgeschrieben, also **jeder** Klick auf
    /// „Als Standardbrowser festlegen" die Einstellungsdatei neu geschrieben
    /// — und `Einstellungen` hat kein Feld, in dem sich dafuer etwas
    /// geaendert haette. GEMESSEN am Bild: man sieht es nicht, und genau
    /// darum ist es eine Funktion und kein Bild, was man pruefen muss.
    #[test]
    fn der_knopf_schreibt_keine_datei() {
        assert!(
            !a_wert_ist_setzbar(&Aktion::Standardbrowser),
            "der Knopf schreibt die Einstellungsdatei, ohne einen Wert zu aendern"
        );
        assert!(
            !a_wert_ist_setzbar(&Aktion::ErweiterungLaden),
            "der Knopf 'Erweiterung laden' schreibt die Einstellungsdatei"
        );
    }

    /// UND: die Navigation schreibt auch nicht — der Gegenpol, damit die
    /// vorige Behauptung nicht durch eine leere Liste beweisen waere.
    #[test]
    fn nur_werte_schreiben() {
        assert!(a_wert_ist_setzbar(&Aktion::Wert(
            einstellungen::Feld::Startseite,
            String::new()
        )));
        assert!(a_wert_ist_setzbar(&Aktion::Schieben(
            einstellungen::Feld::Zoom,
            120
        )));
        assert!(a_wert_ist_setzbar(&Aktion::Schalten(
            einstellungen::Feld::Werbeblocker,
            false
        )));
        assert!(!a_wert_ist_setzbar(&Aktion::Nichts));
        assert!(!a_wert_ist_setzbar(&Aktion::Kategorie(Kategorie::Suche)));
        assert!(!a_wert_ist_setzbar(&Aktion::Klappe(
            einstellungen::Feld::Thema
        )));
    }

    /// DER BEFEHL IST ZWEI ARGUMENTE LANG UND HAT DEN RICHTIGEN NAMEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Befehl **nicht** ausgefuehrt wird:
    /// `xdg-settings set default-web-browser narsil.desktop` kann den
    /// Standardbrowser des Rechners aendern, auf dem der Test laeuft. Ein
    /// Test, der die Maschine des Nutzers umstellt, ist kein Test. GEMESSEN
    /// wird darum die **Befehlszeile** (die ist es, woran es liegt), und die
    /// Ausfuehrung behauptet `narsil/tests/vorlage_schritt_5_6.rs` dort, wo
    /// sie nichts aendern kann (keine `narsil.desktop` installiert).
    /// Release 08.10.2026: im Tab „Features" meldet ein Schalter die
    /// Aenderung (die Sitzung setzt ihre Haken) und speichert; der Knopf
    /// oeffnet die eingebaute Seite und speichert nichts.
    #[test]
    fn der_features_tab_meldet_schalter_und_seiten() {
        use narsil_chrome::features::Feature;
        use narsil_core::geometry::Point;
        let chrome = Chrome::neu(1600);
        let mut e = Einstellungen::default();
        let mut s = Seite::default();
        s.wechsle(Kategorie::Features);
        let k = Kategorie::Features;
        let f = einstellungen::Feld::Feature(Feature::Kopfregeln);
        let i = k.felder().iter().position(|x| *x == f).expect("Feld");
        let r = einstellungen::element(k, &chrome, f, i, 1600);
        let w = klick(&mut s, &mut e, &chrome, Point::new(r.x as f32 + 4.0, r.y as f32 + 4.0), 1600, 1080)
            .expect("Wirkung");
        assert!(w.features && w.speichern, "{w:?}");
        assert!(e.features.an(Feature::Kopfregeln));
        let zeile = einstellungen::feld(k, &chrome, f, i, 1600);
        let kr = narsil_chrome::features::knopf_rect(Feature::Kopfregeln, &zeile).expect("Knopf");
        let w = klick(&mut s, &mut e, &chrome, Point::new(kr.x as f32 + 5.0, kr.y as f32 + 5.0), 1600, 1080)
            .expect("Wirkung");
        assert_eq!(w.oeffnen.as_deref(), Some("narsil://kopfregeln"));
        assert!(!w.speichern);
        assert!(ist_einstellungen("about:einstellungen#features"));
    }

    #[test]
    fn der_befehl_ist_der_richtige() {
        let teile: Vec<&str> = einstellungen::STANDARD_BROWSER_BEFEHL
            .split_whitespace()
            .collect();
        assert_eq!(
            teile,
            vec!["xdg-settings", "set", "default-web-browser"],
            "die Befehlszeile stimmt nicht"
        );
        assert_eq!(einstellungen::STANDARD_BROWSER_NAME, "narsil.desktop");
        assert!(
            einstellungen::STANDARD_BROWSER_NAME.chars().count() <= einstellungen::MAX_DESKTOP_NAME
        );
    }
}
