//! Einstiegspunkt des headless Renderers.
//!
//! Exit-Codes: 0 = ok (auch Hilfe/Version), 1 = Lese-/Schreibfehler,
//! 2 = falsche Kommandozeile, 3 = Stufenfehler (Source/Message).
//! Fehlermeldungen gehen nach stderr; stdout ist fuers Bild.

use narsil_app::{AppError, HELP, RealPipeline, VERSION, format_error, parse_args, run};

/// Platzhalter? Nein — seit TASK_0009 steht hier die echte Pipeline.
/// (Der Stub ist Geschichte; `RealPipeline` lebt in `pipeline.rs`.)

fn main() {
    std::process::exit(real_main());
}

/// Argumentauswertung und Ablauf, getrennt von `main` für Testbarkeit.
fn real_main() -> i32 {
    let mut argv = std::env::args();
    argv.next();
    let args: Vec<String> = argv.collect();
    let wants_version = args.iter().any(|a| a == "--version" || a == "-V")
        && !args.iter().any(|a| a == "--help" || a == "-h");
    match parse_args(args) {
        Ok(None) => {
            if wants_version {
                println!("narsil-app {VERSION}");
            } else {
                println!("{HELP}");
            }
            0
        }
        Err(e) => {
            eprintln!("Fehler: {e}");
            2
        }
        Ok(Some(cli)) => {
            if cli.window {
                return show_window(&cli);
            }
            match run(&RealPipeline, &cli) {
                Ok(()) => {
                    match &cli.output {
                        Some(out) => {
                            println!("Fertig: {} -> {}", cli.input.display(), out.display())
                        }
                        None => println!("Fertig: {}", cli.input.display()),
                    }
                    0
                }
                Err(e) => {
                    let code = match &e {
                        AppError::Read { .. } | AppError::Write { .. } => 1,
                        AppError::Cli(_) => 2,
                        AppError::Source { .. } | AppError::Message { .. } => 3,
                    };
                    eprintln!("{}", render_error(&cli.input, &e));
                    code
                }
            }
        }
    }
}

/// Zeigt das gerenderte Bild im X11-Fenster statt es zu schreiben.
///
/// Ablauf: Datei lesen, rendern (Fehler wie in `run`: Exit 1/3 mit
/// Datei:Zeile:Spalte), Fenster oeffnen, zeigen, Ereignisschleife bis zum
/// Schliessen (X-Button). Expose malt neu, Resize merkt die Groesse.
/// Ohne Feature x11: sauberer Fehler statt halb gebautem Fenster.
fn show_window(cli: &narsil_app::Cli) -> i32 {
    #[cfg(any(feature = "x11", feature = "desktop"))]
    {
        show_window_x11(cli)
    }
    #[cfg(not(any(feature = "x11", feature = "desktop")))]
    {
        let _ = cli;
        eprintln!(
            "Fehler: --fenster braucht das Feature desktop (cargo run -p narsil-app --features desktop -- ...)"
        );
        2
    }
}

/// Fensterlauf mit Feature x11: Voller Browser mit Chrome, Navigation, Eingabe.
#[cfg(any(feature = "x11", feature = "desktop"))]
fn show_window_x11(cli: &narsil_app::Cli) -> i32 {
    use narsil_browser::{Browser, Navigation, Wirkung};
    use narsil_chrome::{
        ADRESSE_X, Chrome, KNOPF_KANTE, KNOPF_NEU_X, KNOPF_VORWAERTS_X, Knopf, Knopfzustand,
        LEISTE_HOEHE,
    };
    use narsil_display::hit_test;
    use narsil_input::Key;
    use narsil_window::{Surface, NativeSurface};

    // Eingebaute Startseite (narsil://mitgeliefert/grosser)
    const BUILTIN_PAGE: &str = r#"page {
    width: 800;
    height: 600;
    background: #101014;

    container {
        width: 600;
        height: 400;
        margin: 40;
        padding: 20;
        background: #1b1b24;
        direction: column;

        text "Willkommen bei Narsil" {
            font-size: 28;
            color: #f2f2f7;
        }

        text "Dies ist die eingebaute Startseite." {
            font-size: 16;
            color: #ccccd5;
        }

        text "Tippe eine Adresse in die Leiste oben und drücke Enter." {
            font-size: 16;
            color: #ccccd5;
        }

        text "Unterstützt: file://, http://, narsil://" {
            font-size: 14;
            color: #888899;
        }

        container {
            width: 200;
            height: 40;
            margin: 20;
            background: #ff5f5f;
            direction: row;

            text "Testseite laden" {
                font-size: 16;
                color: #ffffff;
            }
        }
    }
}"#;

    // Initiale Seite bestimmen
    let initial_source = if cli.input.to_string_lossy() == "narsil://mitgeliefert/grosser" {
        BUILTIN_PAGE.to_string()
    } else {
        match std::fs::read_to_string(&cli.input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Fehler: Eingabedatei {} nicht lesbar: {e}",
                    cli.input.display()
                );
                return 1;
            }
        }
    };

    // Browser- und Chrome-Zustand
    let mut browser = Browser::neu();
    let mut chrome = Chrome::neu(cli.width.unwrap_or(800));
    let mut current_source = initial_source.clone();
    let mut current_url = cli.input.to_string_lossy().to_string();
    let mut window_width = cli.width.unwrap_or(800);
    let mut window_height = cli.height.unwrap_or(600);
    let mut address_focused = false;

    // Initiale Navigation (falls nicht builtin)
    if current_url != "narsil://mitgeliefert/grosser" {
        let _ = browser.anwenden(&Navigation::To(current_url.clone()));
    }

    // Erstes Rendern
    let mut image = match render_page(&current_source, window_width, window_height, &chrome) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("Fehler beim initialen Rendern: {e}");
            return 3;
        }
    };

    // Mit Pfad: Datei UND Fenster. Datei zuerst.
    if let Some(out) = &cli.output {
        if let Err(e) = narsil_app::write_ppm(&image, out) {
            eprintln!("Fehler: {e}");
            return 1;
        }
    }

    let title = std::format!("Narsil — {}", current_url);
    let mut surface = match NativeSurface::open(image.width, image.height, &title) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Fehler: Fenster oeffnet nicht: {e}");
            return 1;
        }
    };

    if let Err(e) = surface.present(&image.rgba, image.width, image.height) {
        eprintln!("Fehler: Bild nicht zeigbar: {e}");
        return 1;
    }

    println!(
        "Fenster {} ({}x{}) — Narsil Browser bereit. Strg+Q zum Beenden.",
        surface.window_id(),
        image.width,
        image.height
    );

    // Haupt-Event-Loop
    while !surface.is_closed() {
        let mut needs_redraw = false;
        let mut new_source: Option<String> = None;
        let mut new_url: Option<String> = None;

        // Events verarbeiten
        match surface.poll_events() {
            Ok(events) => {
                for event in events {
                    use narsil_window::event::SurfaceEvent;
                    match event {
                        SurfaceEvent::Key(key_event) => {
                            // Tastatur-Shortcuts (global)
                            if key_event.pressed {
                                if key_event.is_ctrl_c() || key_event.is_ctrl_q() {
                                    return 0;
                                }
                                if key_event.is_ctrl_l() {
                                    address_focused = true;
                                    chrome.knopfzustand(Knopf::Neu); // Just to trigger redraw
                                    needs_redraw = true;
                                    continue;
                                }
                            }

                            // Escape: Adressleiste leeren oder Fenster schließen
                            if key_event.pressed && key_event.key == Key::Escape {
                                if address_focused {
                                    chrome.leeren();
                                    address_focused = false;
                                    needs_redraw = true;
                                } else if chrome.adresse.is_empty() {
                                    return 0; // Fenster schließen
                                } else {
                                    chrome.leeren();
                                    address_focused = true;
                                    needs_redraw = true;
                                }
                                continue;
                            }

                            // Enter: Navigation ausführen
                            if key_event.pressed && key_event.key == Key::Enter {
                                if address_focused && !chrome.adresse.is_empty() {
                                    new_url = Some(chrome.adresse.clone());
                                    address_focused = false;
                                }
                                continue;
                            }

                            // Nur in Adressleiste tippen, wenn fokussiert
                            if address_focused {
                                if let Some(ch) = key_event.typed_char() {
                                    chrome.einfuegen(ch);
                                    needs_redraw = true;
                                } else if key_event.pressed && key_event.key == Key::Backspace {
                                    chrome.backspace();
                                    needs_redraw = true;
                                }
                            }
                        }
                        SurfaceEvent::ButtonPressed { point, .. } => {
                            // Hit-Test auf Chrome (Buttons + Adressleiste)
                            let chrome_list = chrome.to_display_list();
                            if let Some(hit) = hit_test(&chrome_list, point) {
                                match hit {
                                    narsil_display::HitTarget::None => {
                                        // Prüfen ob auf Knopf geklickt
                                        let r = chrome.adressleiste();
                                        if point.y < LEISTE_HOEHE as f32 {
                                            // In der Leisten-Zeile
                                            if point.x < KNOPF_NEU_X as f32 + KNOPF_KANTE as f32 {
                                                // Einer der drei Buttons
                                                if point.x < KNOPF_VORWAERTS_X as f32 {
                                                    // Zurück
                                                    let wirkung =
                                                        browser.anwenden(&Navigation::Back);
                                                    if wirkung == Wirkung::Geaendert {
                                                        current_url = browser.adresse().to_string();
                                                        new_source = Some(fetch_source(
                                                            &current_url,
                                                            BUILTIN_PAGE,
                                                        ));
                                                        new_url = Some(current_url.clone());
                                                    }
                                                } else if point.x < KNOPF_NEU_X as f32 {
                                                    // Vorwärts
                                                    let wirkung =
                                                        browser.anwenden(&Navigation::Forward);
                                                    if wirkung == Wirkung::Geaendert {
                                                        current_url = browser.adresse().to_string();
                                                        new_source = Some(fetch_source(
                                                            &current_url,
                                                            BUILTIN_PAGE,
                                                        ));
                                                        new_url = Some(current_url.clone());
                                                    }
                                                } else {
                                                    // Neu laden
                                                    let wirkung =
                                                        browser.anwenden(&Navigation::Reload);
                                                    if wirkung == Wirkung::Geaendert {
                                                        new_source = Some(fetch_source(
                                                            &current_url,
                                                            BUILTIN_PAGE,
                                                        ));
                                                    }
                                                }
                                                needs_redraw = true;
                                            } else if point.x >= ADRESSE_X as f32
                                                && point.x < r.rechts() as f32
                                            {
                                                // Adressleiste fokussieren
                                                address_focused = true;
                                                needs_redraw = true;
                                            }
                                        }
                                    }
                                    narsil_display::HitTarget::Text { .. } => {
                                        // Text in Adressleiste geklickt -> fokussieren
                                        address_focused = true;
                                        needs_redraw = true;
                                    }
                                }
                            }
                        }
                        SurfaceEvent::ButtonReleased { .. } => {
                            // Knopf-Zustand zurücksetzen
                            chrome.setze_knopfzustand(Knopf::Zurueck, Knopfzustand::An);
                            chrome.setze_knopfzustand(Knopf::Vorwaerts, Knopfzustand::An);
                            chrome.setze_knopfzustand(Knopf::Neu, Knopfzustand::An);
                            needs_redraw = true;
                        }
                        SurfaceEvent::Resized(size) => {
                            window_width = size.width as u32;
                            window_height = size.height as u32;
                            chrome.setze_fensterbreite(window_width);
                            surface.resize(window_width, window_height).ok();
                            needs_redraw = true;
                        }
                        SurfaceEvent::CloseRequested => {
                            return 0;
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => break,
        }

        // Navigation verarbeiten (nach Events, damit Adresse schon aktualisiert ist)
        if let Some(url) = new_url {
            current_url = url.clone();
            let _ = browser.anwenden(&Navigation::To(url));
            chrome.setze_adresse(&current_url);
            chrome.cursor = current_url.chars().count();
            address_focused = false;
        }

        if let Some(source) = new_source {
            current_source = source;
            needs_redraw = true;
        }

        // Knopf-Zustände aus Browser aktualisieren
        chrome.setze_knopfzustand(
            Knopf::Zurueck,
            if browser.kann_zurueck() {
                Knopfzustand::An
            } else {
                Knopfzustand::Aus
            },
        );
        chrome.setze_knopfzustand(
            Knopf::Vorwaerts,
            if browser.kann_vorwaerts() {
                Knopfzustand::An
            } else {
                Knopfzustand::Aus
            },
        );
        chrome.setze_knopfzustand(Knopf::Neu, Knopfzustand::An);

        // Neu rendern wenn nötig
        if needs_redraw {
            match render_page(&current_source, window_width, window_height, &chrome) {
                Ok(img) => {
                    image = img;
                    if let Err(e) = surface.present(&image.rgba, image.width, image.height) {
                        eprintln!("Fehler beim Präsentieren: {e}");
                        return 1;
                    }
                    if let Some(out) = &cli.output {
                        let _ = narsil_app::write_ppm(&image, out);
                    }
                }
                Err(e) => {
                    eprintln!("Render-Fehler: {e}");
                    // Weiterlaufen, altes Bild zeigen
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(16)); // ~60fps
    }
    0
}

/// Rendert eine Seite mit Chrome-Oberfläche
#[cfg(any(feature = "x11", feature = "desktop"))]
fn render_page(
    source: &str,
    width: u32,
    height: u32,
    chrome: &narsil_chrome::Chrome,
) -> Result<narsil_app::RenderedImage, String> {
    use narsil_chrome::LEISTE_HOEHE;
    use narsil_core::geometry::Size;
    use narsil_display::DisplayList;
    use narsil_layout_engine::{build_display_list, layout};
    use narsil_raster::rasterize_with_font;
    use narsil_text::BitmapFont;

    // Parse
    let document = narsil_parser::parse(source).map_err(|e| e.message)?;

    // Viewport: Chrome-Leiste abziehen
    let content_height = height.saturating_sub(LEISTE_HOEHE);
    let viewport = Size::new(width as f32, content_height as f32);

    // Layout
    let tree = layout(&document, viewport).map_err(|e| e.to_string())?;

    // Display-Liste für Seiteninhalt
    let page_list = build_display_list(&tree, &document);

    // Chrome Display-Liste erzeugen und INHALT nach unten verschieben
    let chrome_list = chrome.to_display_list();

    // Seiteninhalt um Leistenhöhe nach unten verschieben
    let shift_y = LEISTE_HOEHE as f32;
    let shifted_items: Vec<_> = page_list.iter()
        .map(|item| narsil_display::schiebe(item, shift_y)).collect();

    // Kombinieren: erst Chrome (unten), dann Seiteninhalt (oben) - Painter's Algorithm
    let mut full_list = DisplayList::new();
    for item in chrome_list.iter() {
        full_list.push(item.clone());
    }
    for item in shifted_items {
        full_list.push(item);
    }

    // Rastern mit Schrift
    let font = BitmapFont::builtin();
    let result = rasterize_with_font(&full_list, Some(&font), width, height);

    narsil_app::RenderedImage::new(width, height, result.framebuffer.pixels().to_vec()).map_err(|e| e)
}

/// Holt Quelle für eine URL (file://, http://, narsil://)
#[cfg(any(feature = "x11", feature = "desktop"))]
fn fetch_source(url: &str, builtin: &str) -> String {
    use narsil_net::fetch;
    use narsil_url::parse as parse_url;

    if url == "narsil://mitgeliefert/grosser" {
        return builtin.to_string();
    }

    if let Ok(addr) = parse_url(url) {
        match addr.scheme {
            narsil_url::Scheme::Http => {
                use narsil_net::Request;
                let port = addr.port.unwrap_or(80);
                let req = Request::get(&addr.host, port, &addr.path);
                match fetch(&req) {
                    Ok(resp) if resp.is_success() => {
                        return String::from_utf8_lossy(&resp.body).to_string();
                    }
                    Ok(resp) => {
                        return format!(
                            "page {{ background: #ff0000; text \"HTTP {}\" {{ color: #fff; }} }}",
                            resp.status.code
                        );
                    }
                    Err(e) => {
                        return format!(
                            "page {{ background: #ff0000; text \"Netzwerkfehler: {}\" {{ color: #fff; }} }}",
                            e
                        );
                    }
                }
            }
            narsil_url::Scheme::Localhost => {
                // file:// oder lokaler Pfad
                let path = if url.starts_with("file://") {
                    &url[7..]
                } else {
                    url
                };
                match std::fs::read_to_string(path) {
                    Ok(s) => return s,
                    Err(e) => {
                        return format!(
                            "page {{ background: #ff0000; text \"Dateifehler: {}\" {{ color: #fff; }} }}",
                            e
                        );
                    }
                }
            }
            narsil_url::Scheme::Https => {
                return "page { background: #ff0000; text \"HTTPS nicht unterstützt (TLS fehlt)\" { color: #fff; } }".to_string();
            }
        }
    }

    // Fallback: als Dateipfad versuchen
    match std::fs::read_to_string(url) {
        Ok(s) => s,
        Err(_) => format!(
            "page {{ background: #ff0000; text \"Unbekannte Adresse: {}\" {{ color: #fff; }} }}",
            url
        ),
    }
}
///
/// Stellt einen Lauf-Fehler fuer stderr dar.
///
/// `Source`-Fehler mit Span werden als `datei:zeile:spalte:` plus Auszug
/// mit Caret formatiert (dafuer wird die Eingabedatei erneut gelesen —
/// sie existiert, `run` hat sie gerade gelesen). Gelingt das nicht oder
/// hat der Fehler keinen Ort, gilt die `Display`-Form.
///
/// `Source`-Fehler mit Span werden als `datei:zeile:spalte:` plus Auszug
/// mit Caret formatiert (dafuer wird die Eingabedatei erneut gelesen —
/// sie existiert, `run` hat sie gerade gelesen). Gelingt das nicht oder
/// hat der Fehler keinen Ort, gilt die `Display`-Form.
fn render_error(input: &std::path::Path, error: &AppError) -> String {
    match error {
        AppError::Source {
            stage,
            message,
            span,
        } => match span {
            Some(_) => match std::fs::read_to_string(input) {
                Ok(source) => format_error(input, &source, *stage, *span, message),
                Err(_) => std::format!("Fehler: {error}"),
            },
            None => std::format!("Fehler: {error}"),
        },
        _ => std::format!("Fehler: {error}"),
    }
}
