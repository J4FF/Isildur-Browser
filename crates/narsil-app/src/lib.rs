//! Anwendungsschicht: CLI, Datei-IO, Pipeline-Steckdose.
//!
//! Der vertikale MVP entsteht in einem einzigen Kommando:
//! `narsil-app examples/hello.narsil out.ppm`
//!
//! Hinweis zur Ausgabe: narsil-raster wird spaeter selbst ein `write_ppm`
//! anbieten. Bis dahin steht hier eine eigene P6-Implementation (~15 Zeilen).
//! Das ist eine bewusste, dokumentierte Ueberschneidung, die Agent1 in
//! TASK_0009 zusammenfuehrt.

pub mod cli;
pub mod error;
pub mod pipeline;
pub mod source_map;

pub use cli::{Cli, OutputFormat, parse_args};
pub use error::{AppError, CliError, Stage};
pub use pipeline::{Pipeline, PipelineError, RealPipeline, RenderedImage};
pub use source_map::{SourceMap, format_error, format_errors};

/// Text fuer `--help`.
pub const HELP: &str = "\
narsil-app — Narsil headless Renderer (MVP 0)

Benutzung:
  narsil-app <input.narsil> <out.ppm> [Optionen]

Argumente:
  <input.narsil>        Eingabedatei in der Narsil-Sprache
  <out.ppm>           Ausgabedatei (binares PPM, P6)

Optionen:
  --width <n>         Renderbreite erzwingen (ganze Zahl > 0)
  --height <n>        Renderhoehe erzwingen (ganze Zahl > 0)
  --format <fmt>      Ausgabeformat, nur \"ppm\" (Standard)
  --fenster, --window Bild im X11-Fenster zeigen statt Datei schreiben
                       (braucht Feature x11; sonst sauberer Fehler)
  -h, --help          diese Hilfe anzeigen
  -V, --version       Version anzeigen
  --                  Optionen beenden, Rest sind Dateinamen

Beispiele:
  narsil-app examples/hello.narsil out.ppm
  narsil-app --width 800 a.narsil b.ppm
  narsil-app --fenster examples/hello.narsil
";

/// Programmversion (Workspace-Version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fuehrt einen Lauf aus. Testbar, kein Prozess noetig.
///
/// Liest `cli.input`, rendert ueber `pipeline` und schreibt das Bild als
/// PPM nach `cli.output`. Stufenfehler kommen als `AppError::Source` mit
/// Stufe, Meldung und Span zurueck — die Anzeige (Datei:Zeile:Spalte plus
/// Auszug) entsteht erst beim Formatieren, nicht hier. Ohne Ausgabepfad
/// (`None`, nur im Fenstermodus moeglich) gibt es
/// `MissingArgument("<output>")` statt stillen Erfolgs.
pub fn run<P: Pipeline>(pipeline: &P, cli: &Cli) -> Result<(), AppError> {
    let output = cli
        .output
        .as_ref()
        .ok_or(CliError::MissingArgument("<output>"))?;
    let source = std::fs::read_to_string(&cli.input).map_err(|source| AppError::Read {
        path: cli.input.clone(),
        source,
    })?;
    let image = pipeline
        .render(&source, cli.width, cli.height)
        .map_err(|e| AppError::Source {
            stage: e.stage,
            message: e.message,
            span: e.span,
        })?;
    write_ppm(&image, output)?;
    Ok(())
}

/// Schreibt RGBA8 als binaere PPM-Datei (P6).
///
/// Wird von `run` genutzt und ist public, damit testbar. Header:
/// `P6\\n{breite} {hoehe}\\n255\\n`, danach RGB-Bytes (Alpha entfällt).
/// Gibt die Anzahl geschriebener Bytes zurueck.
pub fn write_ppm(image: &RenderedImage, path: &std::path::Path) -> Result<usize, AppError> {
    let header = std::format!("P6\n{} {}\n255\n", image.width, image.height);
    let mut buf: Vec<u8> = Vec::with_capacity(header.len() + image.pixel_count() * 3);
    buf.extend_from_slice(header.as_bytes());
    for px in image.rgba.chunks_exact(4) {
        buf.push(px[0]);
        buf.push(px[1]);
        buf.push(px[2]);
    }
    let len = buf.len();
    std::fs::write(path, &buf).map_err(|source| AppError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_names_program_options() {
        for needle in [
            "narsil-app",
            "Benutzung",
            "--width",
            "--height",
            "--format",
            "--fenster",
            "--help",
        ] {
            assert!(HELP.contains(needle), "HELP fehlt: {needle}");
        }
    }

    #[test]
    fn version_is_not_empty() {
        assert!(!VERSION.is_empty());
    }
}
