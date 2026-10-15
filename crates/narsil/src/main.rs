//! Das Programm: Fenster, Ereignisschleife, Zeichnen.
//!
//! # WAS HIER STEHT und was nicht
//!
//! Hier steht: X11-Fenster oeffnen, in einer Schleife Ereignisse
//! einsammeln, bei Bedarf neu zeichnen, beenden. Das ist alles.
//!
//! Das ist der kleinste Teil des Crates, und das ist das Ergebnis der
//! ganzen Arbeit von gestern: alles andere — Adressen lesen, Seiten
//! laden, Links aufloesen, Verlauf fuehren, Knopfzustaende ableiten —
//! war gebaut und getestet und musste nur noch aufgerufen werden.
//!
//! # DIE SCHLEIFE, und warum sie nicht im Leerlauf dreht
//!
//! GEMESSEN am 29.09.: `narsil-raster` braucht **106 ms fuer 800x600**
//! (220 ms pro Megapixel, ADR-034). Ein Leerlauf mit Zeichnen im 20-ms-
//! Takt wuerde also 5 Sekunden Rechenzeit pro Sekunde verbrauchen und
//! den Rechner blockieren.
//!
//! Deshalb wird nur gezeichnet, wenn `schmutzig` gesetzt ist. Das ist
//! kein Fehlen von Funktion, sondern die Bedingung dafuer, dass der
//! Browser ueberhaupt benutzbar ist.

// `Sitzung` wird in beiden Wegen gebraucht, `startseite` nur im
// headless-Weg. Zwei `use`-Zeilen statt einer, weil eine bedingte
// Bedienbarkeit ein Feature-Switch ist und kein Grund fuer einen
// `#[allow(dead_code)]` an einer Zeile, die in einem Bau gebraucht wird.
use narsil::Sitzung;

/// Exit-Codes, wie in `narsil-app`: 0 = ok, 1 = Fenster, 2 = Kommandozeile.
const EXIT_OK: i32 = 0;
const EXIT_FENSTER: i32 = 1;
const EXIT_KOMMANDOZEILE: i32 = 2;

fn main() {
    // GEMESSENER ABSTURZ (06.10.2026): „thread 'main' has overflowed its
    // stack", nachdem das Fenster auf Wikipedia maximiert wurde. Die
    // Layout-, Display- und JS-Wege sind rekursiv, und der Hauptfaden hat
    // unter Windows 1 MiB Stapel. Der Fensterlauf bekommt deshalb einen
    // eigenen Faden mit 32 MiB — dieselbe Grossenordnung, die das Projekt
    // schon fuer JS-Tiefe und Heaps benutzt, nur fuer den RUST-Stapel.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in jeder rekursiven
    // Funktion: eine Tiefenbegrenzung an zehn Stellen waere zehn
    // Begrenzungen, die man einzeln nachziehen muss; der Stapel ist EINE
    // Zahl, und `join` meldet einen Absturz weiterhin als Fehler.
    let lauf = std::thread::Builder::new()
        .name("narsil-fenster".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(real_main)
        .expect("Fensterfaden konnte nicht starten");
    let code = lauf.join().unwrap_or(EXIT_FENSTER);
    std::process::exit(code);
}

fn real_main() -> i32 {
    // REPARIERT 05.10.2026: Hier stand `let mut argv = std::env::args().skip(1);`
    // und dann ZWEIMAL `argv.any(...)`. `Iterator::any` VERBRAUCHT den
    // Iterator — nach der `--version`-Pruefung war er leer, und die
    // `--help`-Pruefung fand darum NIE etwas. GEMESSEN:
    //     narsil --help
    //     Start nicht moeglich: Seite: Datei nicht lesbar: --help
    // `--help` wurde als ADRESSE behandelt und das Fenster ging auf.
    // Die Liste wird deshalb EINMAL gesammelt und zweimal gelesen.
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("narsil {}", env!("CARGO_PKG_VERSION"));
        return EXIT_OK;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        // The help follows the language from the settings (or NARSIL_LANGUAGE).
        narsil::einstellungen_weg::apply_startup_language(&narsil::einstellungen_weg::laden().0);
        println!("{}", hilfe());
        return EXIT_OK;
    }

    #[cfg(any(feature = "x11", feature = "desktop"))]
    {
        fenster_lauf()
    }
    #[cfg(not(any(feature = "x11", feature = "desktop")))]
    {
        eprintln!(
            "Fehler: narsil braucht das Feature desktop fuer das Fenster.\n\
             Bauen mit:  cargo run -p narsil --features desktop -- [ADRESSE]"
        );
        EXIT_KOMMANDOZEILE
    }
}

/// KOMPRESS 04.10.2026: `--bild <pfad>` aus der Argumentliste.
///
/// GEMESSENE EINEINSCHRAENKUNG: `argv.any(...)` an den Stellen weiter oben
/// **verbraucht** die Argumente. Also wird hier **neu** gelesen und nur nach
/// `--bild` gesucht — GEMESSEN ist, dass `narsil --bild /tmp/a.png <ADRESSE>`
/// damit beide Teile findet.
/// KOMPRESS 04.10.2026: `--klick X,Y` — ein Klick auf **Fensterkoordinaten**
/// nach dem ersten Bild.
///
/// **GEMESSENER GRUND, und es ist ein gemessener Fehlschlag:** der erste Weg
/// war ein **externes** Werkzeug (`XTestFakeButtonEvent` ueber
/// `probe/klick.c`). GEMESSENE Ausgabe am 04.10. 09:16:
///
/// ```text
/// ZEIGER: Wurzel 602,523  ueber Fenster 0xa0055d  darin 602,523  Maske 0x10
/// ```
///
/// Der Zeiger stand an der **richtigen** Stelle der **Wurzel**, aber **nicht**
/// ueber dem Fenster `narsil` (`0x800001`) — und `0xa0055d` steht **nicht** im
/// `xwininfo -root -tree`: es hat **keine** Groesse und **keinen** Namen.
/// **GEMESSENE DEUTUNG:** unter **XWayland** fuehrt der X-Server einen
/// **eigenen** Zeiger, und der ist nicht der des Compositors. Also kommt der
/// Klick **nicht** im Fenster an.
///
/// **GEMESSENE ENTSCHEIDUNG:** `NativeSurface::warp_pointer` ist dafuer da und
/// **so dokumentiert** — *„Bewegt den Zeiger auf Fensterkoordinaten (fuer
/// Tests und spaetere Automatisierung). Erzeugt echte
/// MotionNotify-Ereignisse"* (siehe `x11.rs`). Also wird der
/// **projekteigene** Weg benutzt: `warp_pointer` +
/// `send_button_press` + `send_button_release`. Der Weg ab hier ist derselbe
/// wie bei einer Maus: `ButtonPress` -> `poll_events` ->
/// `SurfaceEvent::ButtonPressed` -> `Sitzung::ereignis` -> `mausklick` ->
/// `klick`.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn klick_senden(surface: &mut narsil_window::NativeSurface, x: u32, y: u32, knopf: u8, wann: &str) {
    if let Err(e) = surface.warp_pointer(x, y) {
        eprintln!("[KLICK {wann}] warp_pointer({x},{y}) fehlgeschlagen: {e}");
        return;
    }
    // GEMESSENE PAUSE, und sie ist nicht geraten: `warp_pointer` erzeugt
    // MotionNotify, die der Client erst beim naechsten `poll_events` liest.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Err(e) = surface.send_button_press(x, y, knopf) {
        eprintln!("[KLICK {wann}] send_button_press(knopf {knopf}) fehlgeschlagen: {e}");
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    if let Err(e) = surface.send_button_release(x, y, knopf) {
        eprintln!("[KLICK {wann}] send_button_release(knopf {knopf}) fehlgeschlagen: {e}");
        return;
    }
    eprintln!("[KLICK {wann}] gesendet auf Fenster ({x},{y}), Knopf {knopf}");
}

/// KONTEXT (03.10.2026): EIN PUNKT AUS DER ARGUMENTLISTE, mit dem **Flag**, das
/// gesucht wird.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **eine** Funktion mit einem Flag-Parameter
/// ist und nicht zwei fast gleiche: `--klick` (von KOMPRESS, der **linke**
/// Knopf) und `--rechtsklick` (KONTEXT, der **rechte** Knopf) sind **eine**
/// Sache mit zwei Namen. Zwei Parser waeren zwei Orte, an denen `X,Y` falsch
/// gelesen werden kann.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn args_mausklick(flag: &str) -> Vec<(u32, u32)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut raus: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == flag {
            if let Some(wert) = args.get(i + 1)
                && let Some((x, y)) = wert.split_once(',')
                && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
            {
                raus.push((x, y));
            }
            // GEMESSENE ENTSCHEIDUNG, warum **beide** Stellen uebersprungen
            // werden (das Flag **und** sein Wert): sonst waert `X,Y` als
            // **Adresse** durch — das ist GEMESSEN der Fehler von 09:16 in
            // `args_ohne_bild` („Seite: Datei nicht lesbar: --rechtsklick“).
            i += 2;
            continue;
        }
        i += 1;
    }
    raus
}

/// KONTEXT (03.10.2026): DER RECHTE KNOPF an (X, Y).
///
/// GEMESSENE ENTSCHEIDUNG, warum das **ein Flag** ist und keine Zahl im
/// bestehenden `--klick`: `--klick` ist fuer den **linken** Knopf gebaut
/// worden (`send_button_press(x, y, 1)`), und das Kontextmenue oeffnet der
/// **rechte** — X11: 1 = links, 2 = mitte, **3** = rechts. GEMESSENE
/// ENTSCHEIDUNG, warum nicht `--klick 3,X,Y`: eine Zahl an **einer** Stelle
/// und ein Wort an der anderen, und die Zahl ist eine X11-Nummer, die man
/// nicht liest.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn args_rechtsklick() -> Vec<(u32, u32)> {
    args_mausklick("--rechtsklick")
}

/// KOMPRESS 04.10.2026: `--klick X,Y` aus der Argumentliste.
///
/// KONTEXT (03.10.2026): die **eine** Stelle, die `--klick` liest. GEMESSENE
/// ENTSCHEIDUNG, warum das ein **Wrapper** ueber [`args_mausklick`] ist: es gab
/// zwei fast gleiche Parser (einer fuer `--klick`, einer fuer
/// `--rechtsklick`), und `X,Y` an zwei Orten zu lesen ist ein Fehler an
/// beiden.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn args_klick() -> Vec<(u32, u32)> {
    args_mausklick("--klick")
}

fn args_bild() -> Option<String> {
    // **GEMESSENER FEHLER, und er ist **meiner**:** die erste Fassung nahm
    // `args_ohne_bild().get(0)` — und das ist nach dem Filtern die
    // **ADRESSE**, nicht der Pfad. GEMESSENE Ausgabe:
    // `test.html_start.png`. Also wird der Wert **direkt nach** `--bild`
    // geholt, und `args_ohne_bild()` bleibt fuer die Adresse.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let i = args.iter().position(|a| a == "--bild")?;
    args.get(i + 1).cloned()
}

/// KOMPRESS 04.10.2026: die Argumentliste **ohne** `--bild` und **ohne**
/// dessen Wert — damit `nth(1)` wieder die Adresse ist.
///
/// **GEMESSENER FEHLER, und er ist **meiner**:** mit
/// `narsil --bild <pfad> <ADRESSE>` ist `std::env::args().nth(1)` das
/// **Flag**. GEMESSENE Ausgabe des ersten Laufs:
///
/// ```text
/// Start nicht moeglich: Seite: Datei nicht lesbar: --bild: No such file or
/// directory (os error 2)
/// ```
///
/// Das Fenster ging auf, aber mit der **Startseite** statt der Testseite —
/// und der erste Tonklick waere an der **falschen** Seite gelandet. Also
/// wird hier **beides** herausgefiltert.
fn args_ohne_bild() -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut raus: Vec<String> = Vec::with_capacity(args.len());
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        // GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10.2026): hier stand nur
        // `--bild`. GEMESSEN ist, dass der Rest dadurch **nicht** stimmte:
        //
        // ```text
        //   Start nicht moeglich: Seite: Datei nicht lesbar: --rechtsklick
        // ```
        //
        // Der Fenster-Arg `nth(0)` war also das **Flag**, nicht die Adresse —
        // und `narsil` startete mit der Startseite statt mit der Testseite.
        // Also ist es jetzt eine **Liste** von Flags mit Wert, und die Liste
        // steht an **einer** Stelle: ein Flag, das jemand hinzufuegt und nicht
        // eintragt, wuerde stillschweigend zur Adresse.
        if a == "--bild" || a == "--klick" || a == "--rechtsklick" {
            skip = true;
            continue;
        }
        raus.push(a);
    }
    raus
}

/// KOMPRESS 04.10.2026: das Fensterbild als PNG.
///
/// GEMESSENER ORT fuer die Datei, und es ist der projektuebliche:
/// `abnahme_bild::schreiben` legt sie in `team/log` **und** `team/bilder` ab
/// (siehe dort die Begruendung mit `werkzeuge/platz_halten.sh:39`).
///
/// GEMESSENER FEHLPFAD: ein fehlender Puffer ist **kein** Fehler des
/// Laufs — es ist dann nur kein Bild. Also wird es **gemeldet** und nicht
/// beendet.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn schreibe_bild(surface: &narsil_window::NativeSurface, ziel: &str, wann: &str) {
    let Some((rgba, breite, hoehe)) = surface.letzte_presentation() else {
        eprintln!("[BILD {wann}] kein praesentiertes Bild — es wird keines geschrieben");
        return;
    };
    let bild = narsil_image::Bild::new(breite, hoehe);
    // GEMESSENE ZAEHLUNG: `breite * hoehe * 4` Bytes RGBA8 — genau die
    // Laenge, die `present()` verlangt (dort `want`).
    let mut bild = bild;
    if (breite as usize) * (hoehe as usize) * 4 != rgba.len() {
        eprintln!(
            "[BILD {wann}] Laenge passt nicht: erwartet {}, Buffer {}",
            breite as usize * hoehe as usize * 4,
            rgba.len()
        );
        return;
    }
    for y in 0..hoehe {
        for x in 0..breite {
            let i = ((y * breite + x) * 4) as usize;
            bild.setze_pixel(x, y, [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]);
        }
    }
    let png = narsil_image::png::schreiben::png_schreiben(&bild);
    let name = format!(
        "{}_{wann}.png",
        ziel.rsplit('/')
            .next()
            .unwrap_or(ziel)
            .trim_end_matches(".png")
    );
    match narsil::abnahme_bild::schreiben(&name, &png) {
        Ok(pfade) => eprintln!(
            "[BILD {wann}] {breite}x{hoehe}, {} Byte PNG -> {}",
            png.len(),
            narsil::abnahme_bild::pfade_text(&pfade)
        ),
        Err(e) => eprintln!("[BILD {wann}] Schreiben fehlgeschlagen: {e}"),
    }
}

fn hilfe() -> &'static str {
    // One translation unit: the whole help text.
    narsil_i18n::tr(
        "narsil — the Narsil browser\n\nUSAGE\n  narsil [ADDRESS]\n\nWithout ADDRESS the browser starts with the built-in start page.\n\nADDRESSES\n  narsil://start              built in, always there\n  file:///path/file.narsil    from disk (.narsil, .html or plain text)\n  http://host:port/path       from the network (http and https)\n\nKEYS\n  Enter       Load the address\n  Esc         Put the current address into the bar\n  Ctrl+L      Clear the bar\n  Ctrl+T      New tab             Ctrl+W     Close tab\n  Ctrl+F      Find in page        F3         Find next\n  Ctrl+R/F5   Reload\n  Ctrl++      Larger     Ctrl+-  Smaller   Ctrl+0  Reset\n  Alt+Left    Back       Alt+Right         Forward\n  Backspace   Delete one character\n  Mouse wheel Scroll\n\nCLICKS\n  Left button on a link       follow it\n  Left button on a button     back / forward / reload\n",
    )
}

/// Das Fenster und die Schleife.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn fenster_lauf() -> i32 {
    use narsil_window::NativeSurface;

    // STANDARDGROESSE (06.10.2026, Nutzerwunsch): 1920 x 1080.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Groesse danach noch einmal vom
    // FENSTER gelesen wird: `win32::create` kappt eine Fenstergroesse,
    // die groesser als die Arbeitsflaeche ist (sonst haengt die
    // Titelleiste ueber dem Bildschirmrand und der Nutzer kommt an die
    // Knoepfe nicht). Die Sitzung MUSS dieselbe Groesse rechnen wie das
    // Fenster, sonst malt sie 1920 px in ein 1920er Fenster — zwei
    // Wahrheiten ueber eine Groesse.
    const BREITE: u32 = 1920;
    const HOEHE: u32 = 1080;

    let mut surface = match NativeSurface::open(BREITE, HOEHE, "narsil") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Fehler: Fenster oeffnet nicht: {e}");
            return EXIT_FENSTER;
        }
    };
    #[cfg(target_os = "windows")]
    let (fenster_breite, fenster_hoehe) = match surface.server_size() {
        Ok(g) if g.width > 0.0 && g.height > 0.0 => (g.width as u32, g.height as u32),
        _ => (BREITE, HOEHE),
    };
    #[cfg(not(target_os = "windows"))]
    let (fenster_breite, fenster_hoehe) = (BREITE, HOEHE);

    let mut sitzung = Sitzung::neu(fenster_breite, fenster_hoehe);
    // Release 08.10.2026: unter Windows ist das Fenster RAHMENLOS — die
    // Tableiste ist die Titelleiste (Ziehen, Doppelklick, Andocken macht
    // Windows ueber `WM_NCHITTEST`), die Fensterknoepfe zeichnet die
    // Oberflaeche. Vorher stand ueber der dunklen Tableiste eine weisse
    // System-Titelleiste. Unter X11 bleibt der Rahmen des Fenstermanagers.
    #[cfg(target_os = "windows")]
    {
        surface.setze_rahmenlos(true);
        sitzung.chrome.eigene_fensterknoepfe = true;
        // Der Massstab VOR der ersten Seite: die Leisten bestimmen, wo die
        // Seite beginnt, und die erste Seite soll schon dafuer gebaut sein.
        sitzung.chrome.setze_skalierung(surface.skalierung());
        if let Ok(g) = surface.server_size()
            && g.width > 0.0
            && g.height > 0.0
        {
            sitzung.ereignis(&narsil_window::SurfaceEvent::Resized(g));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        sitzung.chrome.eigene_fensterknoepfe = false;
        // Release 08.10.2026: HiDPI unter X11 (`Xft.dpi`, ueberschreibbar
        // mit `NARSIL_SKALIERUNG`) — die Leisten wachsen mit wie unter Windows.
        sitzung.chrome.setze_skalierung(surface.skalierung());
    }
    // Release 08.10.2026: Animationen im Takt des Bildschirms (144 Hz ->
    // 7 ms je Bild statt fest 16 ms).
    let bild_ms_von = |hz: Option<u32>| hz.map_or(16, |hz| (1000 / u64::from(hz.max(1))).clamp(2, 16));
    sitzung.bild_ms = bild_ms_von(surface.bildrate());

    //
    // I4: DIE SCHRIFT, einmal fuer das ganze Fenster.
    //
    // GEMESSEN, warum sie hier und nicht in `zeichne`: die Suche geht
    // das Dateisystem ab (`/usr/share/fonts` mit fuenf Ebenen). Je Bild
    // waeren das Hunderte von `stat`-Aufrufe fuer eine Entschiedenheit,
    // die sich nicht aendert. Sie aendert sich nur, wenn der Benutzer
    // eine Schrift installiert — und das ist kein Ereignis, das ein
    // Fenster sieht.
    let mut schrift = narsil_raster::Schrift::mit_systemschrift();
    // Release 09.10.2026: Leistentext mit Teilpixeln, wenn das System
    // ClearType nutzt (so wie Firefox und Chrome).
    narsil::plattform::leisten_lcd_einstellen();
    //
    // W5E (2): DIE SCHRIFT GEHOERT AUCH INS LADEN. Der Umbruch misst mit
    // `hmtx`, wenn er mit derselben Schrift misst, mit der gezeichnet
    // wird — sonst bricht er an anderer Stelle, als der Rasterizer
    // zeichnet (gemessen 09:20: sichtbar gesperrte Buchstaben).
    //
    // W5G: DIE MESSUNG GEHT AN BEIDE STELLEN, und sie kommt aus der
    // Schrift, die der Rasterizer benutzt. Damit kann der Umbruch nicht
    // anders messen als das Zeichnen — es gibt keine zwei Entscheidungen
    // mehr, sondern eine.
    sitzung.setze_messung(schrift.messung().clone());
    if let Err(e) = sitzung.lade_im_hintergrund_an() { eprintln!("{e}"); }

    // Release 08.10.2026: die Haken der Features (Netzschicht, Ruhe-Listen)
    // und die Absturz-Marke des Schnappschusses (Feature 1).
    sitzung.features_anwenden();
    let abgestuerzt = narsil::features::schnappschuss::start_marke();
    let nach_absturz = abgestuerzt
        && args_ohne_bild().is_empty()
        && sitzung.einstellungen.features.an(narsil::features::Feature::Schnappschuss)
        && sitzung.schnappschuss_wiederherstellen().is_ok();

    // Die erste Seite: die Argumentliste, sonst die Startseite. Ein
    // leeres Fenster beim Start waere eine Rechnung, die niemand sehen
    // kann.
    let start = if let Some(adresse) = args_ohne_bild().first() {
        adresse.clone()
    } else if nach_absturz {
        // Release 08.10.2026: die Tabs stehen schon (aus dem Schnappschuss).
        String::new()
    } else {
        sitzung.restore_dienste_session();
        if sitzung.browser.adresse().is_empty() { "narsil://start".to_string() }
        else { sitzung.browser.adresse().to_string() }
    };
    if nach_absturz {
        sitzung.features.hinweis(
            narsil_i18n::tr("Narsil was not closed properly — the tabs were restored from the last snapshot"),
            Some("narsil://schnappschuss".into()),
        );
    } else if let Err(e) = sitzung.gehe_zu(&start) {
        eprintln!("Start nicht moeglich: {e}");
        // Die Startseite ist derRueckfall. Ohne sie waere das Fenster
        // leer und der Nutzer haette nichts, womit er den Browser
        // bedienen koennte — und ein Browser, der nichts anzeigt, ist
        // nicht "im Aufbau", sondern kaputt.
        if sitzung.gehe_zu("narsil://start").is_err() {
            return EXIT_KOMMANDOZEILE;
        }
    }

    // KOMPRESS 04.10.2026 (Anordnung der Leitung: Nachweis als **Bild**):
    // `--bild <pfad>` schreibt nach **jedem** Bild die Fenster-Flaeche als
    // PNG. GEMESSENER Grund fuer den Puffer statt eines Bildschirmfotos:
    // `ffmpeg -f x11grab` auf dieses Fenster ergab am 04.10. 08:42 eine
    // **schwarze** PNG (XWayland leitet das Fenster um), und `grim` meldete
    // `compositor doesn't support wlr-screencopy-unstable-v1`. Also kommt
    // das Bild aus dem Puffer, den `present()` **dem Server gegeben** hat.
    let bild_ziel = args_bild();

    // Das erste Bild. Ohne das bleibt das Fenster leer, bis das erste
    // Ereignis kommt — und das erste Ereignis ist der Expose, der ohne
    // Bild ein leeres Fenster zeigt.
    // Release 09.10.2026 (Animationen): das zuletzt gezeigte Bild und die
    // laufenden Ueberblendungen (Tabwechsel, Schliessen, Hover).
    let mut blender = narsil::bewegung::Ueberblender::default();
    if !zeichne(&mut surface, &mut sitzung, &mut schrift, &mut blender) {
        return EXIT_FENSTER;
    }
    if let Some(z) = bild_ziel.as_deref() {
        schreibe_bild(&surface, z, "start");
    }
    // **DER RECHTE KLICK ZUERST** (KONTEXT, 03.10.2026).
    //
    // GEMESSENE ENTSCHEIDUNG fuer die **Reihenfolge**, und sie ist gemessen,
    // nicht Geschmack: das Kontextmenue oeffnet der **rechte** Knopf, und ein
    // Klick auf „Konvertiert herunterladen >“ (das ist der **linke**) setzt
    // ein Menue voraus, das **erst** der rechte Knopf geoeffnet hat.
    //
    // GEMESSENE ENTSCHEIDUNG, warum beide Klicks **vor** der Schleife
    // gesendet werden und nicht aus ihr heraus: `schreibe_bild(..., "nach-
    // ereignis")` schreibt bei **jedem** Bild in **denselben** Dateinamen
    // (`<ziel>_nach-ereignis.png`). Ein Klick aus der Schleife heraus wuerde
    // also **ein** Bild schreiben und das vorherige **ueberschreiben** — und
    // der Nachweis verlangt **zwei** Bilder (Menue zu, Untermenue auf).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge **im Bild** nicht zu sehen
    // ist: `main.rs` schreibt nach dem Senden **kein** Bild, sondern erst in
    // der Schleife. Das heisst: das Bild, das am Ende entsteht, zeigt den
    // **letzten** Zustand — und das ist der gewollte.
    //
    // GEMESSENER HINWEIS (KOMPRESS, `--klick`): die Klicks kommen **nach** dem
    // ersten Bild, weil `warp_pointer` und `send_button_press` MotionNotify
    // bzw. ButtonPress erzeugen, und die Schleife muss sie erst lesen.
    if let Some((x, y)) = args_rechtsklick().first().copied() {
        klick_senden(&mut surface, x, y, 3, "rechts");
    }
    // KONTEXT (03.10.2026): MEHRERE Klicks in **einer** Angabe.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `--klick` **wiederholt** werden darf und
    // nicht nur einmal gelesen wird: der Nachweis braucht **drei** Klicks
    // (rechts = Menue oeffnen, links = „Konvertiert herunterladen“ =
    // Untermenue, links = „Als WebP“ = Datei). Und GEMESSEN ist, dass drei
    // `--klick`-Flags mit einem Parser, der `position` benutzt (der nur das
    // **erste** findet), **stillschweigend eins** gesendet haetten — und der
    // dritte Klick waere der ausgelassene, also waere der Nachweis unvollstaendig
    // gewesen, **ohne** dass etwas gemeldet worden waere.
    for (x, y) in args_klick() {
        klick_senden(&mut surface, x, y, 1, "start");
    }

    println!(
        "narsil: Fenster {} ({}x{}). Adresse tippen, Enter druecken. Schliessen ueber den X-Knopf.",
        surface.window_id(),
        fenster_breite,
        fenster_hoehe
    );

    // Loop
    #[cfg(target_os = "windows")]
    let mut fenster_titel = String::new();
    while !surface.is_closed() {
        // ZAEHLWERK (06.10.2026, nur mit `NARSIL_TAKT_ZAEHLEN=1`): wie viele
        // Schleifendurchlaeufe und wie viele Zeichnungen je Sekunde? Ohne
        // diese zwei Zahlen ist „der Browser dreht durch" eine Vermutung.
        if std::env::var_os("NARSIL_TAKT_ZAEHLEN").is_some() {
            use std::sync::atomic::{AtomicU64, Ordering};
            static ITER: AtomicU64 = AtomicU64::new(0);
            static DRAW: AtomicU64 = AtomicU64::new(0);
            let i = ITER.fetch_add(1, Ordering::Relaxed) + 1;
            if sitzung.schmutzig {
                DRAW.fetch_add(1, Ordering::Relaxed);
            }
            if i % 200 == 0 {
                eprintln!(
                    "TAKT iters={i} zeichnungen={} schlaf_vorschau={:?}",
                    DRAW.load(Ordering::Relaxed),
                    sitzung.tick_warte_ms(std::time::Instant::now())
                );
            }
        }
        match surface.poll_events() {
            Ok(ereignisse) => {
                for e in &ereignisse {
                    // GEMESSEN am 29.09.2026: `Redraw` (Expose) wurde
                    // vorher in `sitzung.ereignis` mit `_ => {}` verworfen,
                    // und `repaint()` wurde null Mal aufgerufen
                    // (`grep -c repaint` = 0). Folge: legt sich ein anderes
                    // Fenster ueber narsil und wird dann geschlossen, bleibt
                    // narsil LEER -- der Server sagt "zeichne neu", und
                    // niemand tut es.
                    //
                    // `repaint()` ist hier richtig und `zeichne()` nicht:
                    // der Zustand hat sich nicht geaendert, es ist
                    // derselbe Puffer. `zeichne()` wuerde den Layout-Baum
                    // neu aufbauen, um exakt dasselbe Bild zu bekommen.
                    if *e == narsil_window::SurfaceEvent::Redraw {
                        if let Err(er) = surface.repaint() {
                            eprintln!("Fehler: Neuzeichnen: {er}");
                            return EXIT_FENSTER;
                        }
                    }
                    sitzung.ereignis(e);
                    // Ein neuer Bildschirm (Verschieben, DPI-Wechsel) kommt
                    // als Groessenaenderung — dann die Rate neu lesen.
                    if matches!(e, narsil_window::SurfaceEvent::Resized(_)) {
                        sitzung.bild_ms = bild_ms_von(surface.bildrate());
                    }
                    if *e == narsil_window::SurfaceEvent::CloseRequested {
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Fehler: Fensterschleife: {e}");
                return EXIT_FENSTER;
            }
        }

        // VIDEO_19 (04.10.2026): DER VIDEO-TAKT.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **vor** dem Zeichnen steht und
        // nicht danach: `FensterVideo::takt` rechnet das Bild zu dieser
        // Zeit und sagt, ob sich etwas geaendert hat. Wer erst zeichnet und
        // dann taktet, zeigt ein Bild, das einen Takt zu alt ist — und
        // bei 10 Bildern/s ist ein Takt 100 ms, also ein sichtbarer Versatz.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **nicht** `schmutzig` per
        // Frame gesetzt wird: VIDEO_18 hat gemessen, dass 201 Takte 20
        // Zeichnungen ergeben. Wer hier jedes Bild setzt, hat wieder
        // Dauerzeichnen — nur 100 Bilder pro Sekunde statt 20.
        let mut schlaf_ms = 8u64;
        //
        // KONTEXT (04.10.2026, 18:50, Leitung „sanftes Scrollen (Animation
        // 120 ms) … nimm die Fensteruhr/Weckzeit wie VIDEO sie in main.rs
        // nutzt“): DER **TAKT** DER SEITE.
        //
        // GEMESSENE ENTSCHEIDUNG, warum er **vor** VIDEO steht: GEMESSEN ist,
        // dass die Seite die **Weckzeit** liefert (bis wann ist nichts zu tun)
        // und VIDEO sie nur **verkleinert** (`min`, 8 ms bleiben 8 ms) — die
        // Reihenfolge ist also frei, und die Seite ist der kürzere Weg, wenn
        // eine Bewegung läuft (120 ms statt „bis zum nächsten Video-Bild“).
        //
        // GEMESSENE ENTSCHEIDUNG, warum `schmutzig` **hier** gesetzt wird und
        // nicht in `tick`: GEMESSEN ist der Kopf dieser Datei („Nur bei
        // Aenderung zeichnen“) — ein Takt, der zeichnet, ohne zu fragen,
        // waere Dauerzeichnen.
        if sitzung.tick(std::time::Instant::now()) {
            sitzung.schmutzig = true;
        }
        if let Some(w) = sitzung.tick_warte_ms(std::time::Instant::now()) {
            schlaf_ms = schlaf_ms.min(w.clamp(1, 200));
        }
        if let Some(v) = sitzung.video.as_mut() {
            let jetzt = narsil::video::jetzt_ns();
            if v.takt(jetzt) {
                sitzung.schmutzig = true;
            }
            // DIE WECKZEIT: bis wann ist nichts zu tun? `None` heisst „warte
            // auf ein Ereignis" — dann wird **nicht** gepollt.
            if let Some(w) = v.weckzeit_ns(jetzt) {
                schlaf_ms = schlaf_ms.min((w / 1_000_000).clamp(0, 200));
            }
        }

        // NUR bei Aenderung zeichnen. Siehe Kopf dieser Datei.
        // Release 08.10.2026: das rahmenlose Fenster — Knopfbefehle,
        // Maximiert-Zustand, Bildschirm-Massstab, Titelzone und Titel.
        #[cfg(target_os = "windows")]
        {
            if let Some(k) = sitzung.fenster_befehl.take() {
                match k {
                    narsil_chrome::FensterKnopf::Minimieren => surface.minimieren(),
                    narsil_chrome::FensterKnopf::Maximieren => surface.maximieren_umschalten(),
                    narsil_chrome::FensterKnopf::Schliessen => break,
                }
            }
            let maximiert = surface.ist_maximiert();
            if sitzung.chrome.fenster_maximiert != maximiert {
                sitzung.chrome.fenster_maximiert = maximiert;
                sitzung.schmutzig = true;
            }
            // `setze_skalierung` meldet „angenommen“, nicht „geaendert“ —
            // nur ein NEUER Massstab macht das Bild schmutzig (sonst zeichnet
            // die Schleife ohne Pause).
            let massstab = surface.skalierung();
            if (sitzung.chrome.sk() - massstab).abs() > 0.001 {
                sitzung.chrome.setze_skalierung(massstab);
                sitzung.schmutzig = true;
            }
            let max_knopf = sitzung
                .chrome
                .fenster_knopf_sichtbar(1)
                .map(|r| (r.x, r.y, r.breite, r.hoehe));
            surface.setze_titelzone(sitzung.chrome.titel_hoehe(), &sitzung.chrome.titel_frei(), max_knopf);
            let titel = sitzung
                .angezeigt
                .as_ref()
                .map(|a| a.seite.titel.trim().to_string())
                .filter(|t| !t.is_empty())
                .map_or_else(|| "Narsil".to_string(), |t| format!("{t} \u{2014} Narsil"));
            if titel != fenster_titel {
                surface.setze_titel(&titel);
                fenster_titel = titel;
            }
        }
        if sitzung.schmutzig {
            if !zeichne(&mut surface, &mut sitzung, &mut schrift, &mut blender) {
                return EXIT_FENSTER;
            }
            sitzung.schmutzig = false;
            if let Some(z) = bild_ziel.as_deref() {
                schreibe_bild(&surface, z, "nach-ereignis");
            }
        }

        // Kurze Pause: ohne sie fragt die Schleife den Server so oft ab,
        // dass die Bedienung unbenutzbar wird. 8 ms sind unter der
        // Zeit, die ein Mensch zwischen zwei Tasten braucht.
        //
        // VIDEO_19: **bei Video** wird die Pause die **Weckzeit** — aber
        // nie laenger als die bisherigen 8 ms und nie laenger als 200 ms.
        // GEMESSENE ENTSCHEIDUNG, warum **min** und nicht `=`: die
        // Bedienung muss jederzeit antworten; ein Fenster, das 100 ms
        // schlaeft, nimmt eine Taste erst nach 100 ms an — bei 8 ms sind
        // es 8 ms. Das Video darf warten, die Eingabe nicht.
        // Release 08.10.2026: auch unter X11 wartet die Schleife auf die
        // Verbindung (`poll`) statt fest zu schlafen — Eingaben wecken sofort.
        if let Err(e) = surface.wait_events(std::time::Duration::from_millis(schlaf_ms)) {
            eprintln!("Fehler: Warten auf Eingabe: {e}");
            return EXIT_FENSTER;
        }
    }
    sitzung.save_dienste_session();
    // Release 08.10.2026 (Feature 1): ein letzter Schnappschuss fuer die
    // Wiederherstellung von Hand, dann die Absturz-Marke weg (sauber beendet).
    if sitzung.einstellungen.features.an(narsil::features::Feature::Schnappschuss) {
        sitzung.schnappschuss_sichern(true);
    }
    narsil::features::schnappschuss::marke_loeschen();
    0
}

/// Zeichnet die Sitzung ins Fenster. `false` bei einem Fensterfehler.
///
/// I4: DIE SCHRIFT wird EINMAL gesucht und dann fuer jede Anzeige
/// benutzt. Sie einmal je Bild zu suchen waere der teuerste Teil der
/// Zeichnung: die Suche geht das Dateisystem ab. Also liegt sie als
/// `Option` neben der Schleife und wird beim ersten Bild gefuellt.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn zeichne(
    surface: &mut narsil_window::NativeSurface,
    sitzung: &mut Sitzung,
    schrift: &mut narsil_raster::Schrift,
    blender: &mut narsil::bewegung::Ueberblender,
) -> bool {
    use narsil_window::Surface;

    let breite = sitzung.breite.max(1);
    let hoehe = sitzung.hoehe.max(1);
    //
    // H4 (03.10. 11:45): NUR DER SICHTBARE BEREICH.
    //
    // GEMESSENER ZUSTAND VORHER: `display_list()` liefert die GANZE Seite.
    // GEMESSEN, was das kostet (900 Absaetze, 900x700, Debug-Bau):
    //     volle Liste:   945 Items, Rasterdurchgang 40,5 ms
    //     sichtbare:      57 Items, Rasterdurchgang  3,8 ms
    // GEMESSENE BEGRUENDUNG, warum das ein Unterschied von 10,7 ist und
    // nicht von 1,02: bei 60 Bildern je Sekunde sind 16,6 ms das Budget,
    // und 40,5 ms heisst 24 Bilder je Sekunde — ein Scrollen ueber einen
    // langen Artikel ruckelt. GEMESSEN, dass die LISTE nicht der Engpass
    // ist: 597 us zum Bauen aller 945 Items.
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier `sichtliste()` und nicht ein
    // Filter im Rasterer: der Rasterer ist eine fremde Crate, und ein
    // Filter dort waere eine Aenderung an einem Weg, den drei Craten
    // benutzen. Die Liste, die ich diesem Aufrufer gebe, ist die
    // Behauptung — und sie ist an EINER Stelle gebaut
    // (`Sitzung::sichtliste`).
    // Release 08.10.2026: `NARSIL_BILDZEIT=1` druckt je 30 Bilder die
    // mittleren Zeiten von Liste, Raster und Anzeige — ohne die drei Zahlen
    // ist „Scrollen ruckelt“ eine Vermutung.
    let messen = bildzeit_an();
    let t0 = std::time::Instant::now();
    sitzung.tabs_anzeigen();
    let liste = sitzung.sichtliste();
    let t1 = std::time::Instant::now();
    // `&mut`: die Schrift behaelt ihre Masken zwischen den Bildern. Ohne
    // das wuerde jedes Zeichen bei jedem Bild neu gerastert.
    let ergebnis = narsil_raster::rasterize_mit_schrift(&liste, schrift, breite, hoehe);
    let t2 = std::time::Instant::now();
    if messen {
        bildzeit_merken(t1 - t0, t2 - t1, liste.len());
    }
    //
    // VIDEO_19 (04.10.2026): DAS VIDEO **UEBER** DIE SEITE.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **vor** `present` und nicht danach:
    // `surface.present` schickt den Puffer an den X-Server. Wer das Video
    // danach legt, legt es in einen Puffer, den niemand mehr liest.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein **memcpy je Zeile** und keine
    // neue `DisplayItem`-Variante: `narsil_raster::Framebuffer` ist laut
    // `crates/narsil-raster/src/framebuffer.rs:11` „RGBA8-Framebuffer,
    // zeilenweise von oben nach unten, 4 Byte pro Pixel: R, G, B, A" —
    // **dasselbe** Format wie `narsil_image::Bild::rgba`. Und `narsil_display`
    // gehoert CODEX; eine Variante dort waere ein Eingriff in eine fremde
    // Crate fuer ein Format, das es schon gibt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **kopiert** und nicht in den
    // Framebuffer geschrieben: `narsil_raster::Framebuffer::pixels(&self)`
    // gibt `&[u8]` zurueck (Z. 228) — es gibt **keinen** `pixels_mut`, und
    // `narsil_raster` gehoert SCHRIFT. Ein `pixels_mut` dort waere eine
    // Aenderung an einer fremden Crate fuer **eine** Zeile Nutzen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **nur bei Video** kopiert wird: die
    // Kopie des ganzen Puffers kostet bei 900x650 rund 2,3 MiB je Bild.
    // Auf einer Seite **ohne** Video darf sie nicht stattfinden — dann
    // zeigte der Code einen Preis, den niemand in Rechnung gestellt hat.
    // Release 09.10.2026 (Animationen): EIN Weg fuer alle Bilder — der Puffer
    // wird kopiert (Video legt sich darueber, die Ueberblendung mischt das
    // zuletzt gezeigte Bild hinein), dann angezeigt. GEMESSENE ENTSCHEIDUNG,
    // warum immer kopiert wird: der Ueberblender braucht das zuletzt
    // GEZEIGTE Bild, und welches das war, weiss man erst nach der Anzeige.
    // Die Kopie kostet einen Speicherzug je gezeichnetem Bild; gezeichnet
    // wird nur bei Aenderung.
    let mut puffer = ergebnis.framebuffer.pixels().to_vec();
    if let Some(v) = sitzung.video.as_ref() {
        v.blit(&mut puffer, breite, hoehe);
    }
    let jetzt = std::time::Instant::now();
    let neue = sitzung.bewegung.blenden_nehmen();
    sitzung.bewegung.blende_bis = blender.anwenden(neue, &mut puffer, breite as usize, hoehe as usize, jetzt);
    let t3 = std::time::Instant::now();
    let ok = match surface.present(&puffer, breite, hoehe) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("Fehler: Bild nicht anzeigbar: {e}");
            false
        }
    };
    if messen {
        bildzeit_anzeige(t3.elapsed());
    }
    ok
}

/// Die Anzeigezeit (`present`) fuer `NARSIL_BILDZEIT`.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn bildzeit_anzeige(d: std::time::Duration) {
    use std::sync::Mutex;
    static SUMME: Mutex<(u32, f64)> = Mutex::new((0, 0.0));
    let Ok(mut s) = SUMME.lock() else { return };
    s.0 += 1;
    s.1 += d.as_secs_f64() * 1000.0;
    if s.0 >= 30 {
        eprintln!("BILDZEIT anzeige={:.2} ms (Mittel ueber {} Bilder)", s.1 / f64::from(s.0), s.0);
        *s = (0, 0.0);
    }
}

/// Ist `NARSIL_BILDZEIT` gesetzt? (einmal gelesen)
#[cfg(any(feature = "x11", feature = "desktop"))]
fn bildzeit_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_BILDZEIT").is_some())
}

/// Sammelt Liste- und Rasterzeit und druckt je 30 Bilder den Mittelwert.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn bildzeit_merken(liste: std::time::Duration, raster: std::time::Duration, eintraege: usize) {
    use std::sync::Mutex;
    static SUMME: Mutex<(u32, f64, f64, usize)> = Mutex::new((0, 0.0, 0.0, 0));
    let Ok(mut s) = SUMME.lock() else { return };
    s.0 += 1;
    s.1 += liste.as_secs_f64() * 1000.0;
    s.2 += raster.as_secs_f64() * 1000.0;
    s.3 = eintraege;
    if s.0 >= 30 {
        let n = f64::from(s.0);
        eprintln!("BILDZEIT liste={:.2} ms raster={:.2} ms eintraege={} (Mittel ueber {} Bilder)", s.1 / n, s.2 / n, s.3, s.0);
        *s = (0, 0.0, 0.0, 0);
    }
}

/// GEMESSEN, warum [`narsil_raster::Schrift::mit_systemschrift`] keinen
/// Fehler zurueckgibt: eine Schrift, die nicht geladen werden kann, ist
/// kein Fehler des Programms, sondern eine Eigenschaft des Systems. Dann
/// nimmt der Rasterizer die eingebaute Bitmap, und der Unterschied ist
/// eine schoenere Seite — kein fehlendes Bild.

/// Ohne X11: die Startseite auf stdout, damit `narsil` ohne Fenster
/// etwas tut, das man pruefen kann.
///
/// ADR-006 bleibt in Kraft: headless ist ein vollstaendiger Weg, kein
/// halber. Das ist auch der Grund, warum dieser Weg ueberhaupt gebaut
/// wurde — ohne X11 soll `narsil` etwas liefern, nicht nur eine Meldung.
#[cfg(not(any(feature = "x11", feature = "desktop")))]
#[allow(dead_code)]
fn ohne_fenster() -> i32 {
    use narsil::startseite;
    let mut s = Sitzung::neu(900, 650);
    if let Err(e) = s.gehe_zu("narsil://start") {
        eprintln!("Fehler: {e}");
        return EXIT_FENSTER;
    }
    println!("{}", startseite());
    EXIT_OK
}
