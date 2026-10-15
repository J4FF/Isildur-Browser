//! Kommandozeilenargumente fuer narsil-app.
//!
//! Aufruf: `narsil-app <input.narsil> <out.ppm> [Optionen]`
//! Handgeschrieben statt `clap`: ~80 Zeilen und eine Abhaengigkeit weniger
//! in der Trusted Computing Base (ADR-009).

use std::path::PathBuf;

use crate::error::CliError;

/// Wie das Programm aufgerufen wurde.
#[derive(Debug, Clone, PartialEq)]
pub struct Cli {
    /// Eingabedatei, z. B. `examples/hello.narsil`.
    pub input: PathBuf,
    /// Ausgabedatei, z. B. `out.ppm`.
    ///
    /// `None` im Fenstermodus ohne Pfad (`--fenster input.narsil`): Es wird
    /// nichts geschrieben. Mit Pfad wird Datei UND Fenster bedient.
    pub output: Option<PathBuf>,
    /// Erzwungene Renderbreite, falls gesetzt.
    pub width: Option<u32>,
    /// Erzwungene Renderhoehe, falls gesetzt.
    pub height: Option<u32>,
    /// Ausgabeformat. MVP 0: nur Ppm.
    pub format: OutputFormat,
    /// Fenster oeffnen statt Datei schreiben (--fenster/--window).
    pub window: bool,
    /// Hilfe anzeigen statt zu rendern.
    pub help: bool,
    /// Version anzeigen statt zu rendern.
    pub version: bool,
}

/// Ausgabeformat. MVP 0: nur Ppm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Binares PPM (P6).
    Ppm,
}

/// Parst Kommandozeilenargumente (ohne Programmname).
///
/// `Ok(None)` heisst: Hilfe oder Version anzeigen, nichts rendern.
/// `--help`/`--version` funktionieren auch ohne Pflichtargumente.
/// Im Fenstermodus (`--fenster`/`--window`) ist der Ausgabepfad optional:
/// ohne Pfad nur Fenster, mit Pfad Fenster UND Datei.
/// `--` beendet die Optionen; alles danach sind Eingabedateien.
/// `--flag=wert` wird wie `--flag wert` behandelt.
pub fn parse_args<I, S>(args: I) -> Result<Option<Cli>, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut positionals: Vec<String> = Vec::new();
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut format = OutputFormat::Ppm;
    let mut help = false;
    let mut version = false;
    let mut window = false;
    let mut options_done = false;

    let mut iter = args.into_iter().map(Into::into);
    while let Some(arg) = iter.next() {
        if options_done {
            positionals.push(arg);
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        if arg == "--help" || arg == "-h" {
            help = true;
            continue;
        }
        if arg == "--version" || arg == "-V" {
            version = true;
            continue;
        }
        // Fenster statt Datei: deutsch und englisch, beides gilt.
        // Begruendung: --help ist Benutzeroberflaeche; wer --window
        // tippt, meint dasselbe wie --fenster und bekommt keinen Fehler.
        if arg == "--fenster" || arg == "--window" {
            window = true;
            continue;
        }
        // Langform mit Gleichheitszeichen: --width=800.
        if let Some((flag, value)) = split_equals(&arg) {
            match flag {
                "--width" => width = Some(parse_dim("--width", value)?),
                "--height" => height = Some(parse_dim("--height", value)?),
                "--format" => format = parse_format(value)?,
                _ => return Err(CliError::UnknownFlag(arg)),
            }
            continue;
        }
        match arg.as_str() {
            "--width" => match iter.next() {
                None => return Err(CliError::MissingValue("--width")),
                Some(v) => width = Some(parse_dim("--width", &v)?),
            },
            "--height" => match iter.next() {
                None => return Err(CliError::MissingValue("--height")),
                Some(v) => height = Some(parse_dim("--height", &v)?),
            },
            "--format" => match iter.next() {
                None => return Err(CliError::MissingValue("--format")),
                Some(v) => format = parse_format(&v)?,
            },
            _ if arg.starts_with('-') => return Err(CliError::UnknownFlag(arg)),
            _ => positionals.push(arg),
        }
    }

    if help || version {
        return Ok(None);
    }
    match (positionals.len(), window) {
        (0, _) => Err(CliError::MissingArgument("<input>")),
        (1, false) => Err(CliError::MissingArgument("<output>")),
        (1, true) => Ok(Some(Cli {
            input: PathBuf::from(&positionals[0]),
            output: None,
            width,
            height,
            format,
            window,
            help,
            version,
        })),
        (2, _) => Ok(Some(Cli {
            input: PathBuf::from(&positionals[0]),
            output: Some(PathBuf::from(&positionals[1])),
            width,
            height,
            format,
            window,
            help,
            version,
        })),
        (n, _) => Err(CliError::TooManyInputs(n)),
    }
}

/// Teilt `--flag=wert` in Flag und Wert. Gibt `None` zurueck, wenn der
/// Text keine Option mit `=` ist (kein fuehrendes `-` oder kein `=`).
fn split_equals(arg: &str) -> Option<(&str, &str)> {
    if !arg.starts_with('-') {
        return None;
    }
    let eq = arg.find('=')?;
    Some((&arg[..eq], &arg[eq + 1..]))
}

/// Parst eine Massangabe (> 0). `abc` -> InvalidNumber, `0` -> NotPositive.
fn parse_dim(flag: &'static str, value: &str) -> Result<u32, CliError> {
    match value.parse::<u32>() {
        Ok(0) => Err(CliError::NotPositive { flag, value: 0 }),
        Ok(n) => Ok(n),
        Err(_) => Err(CliError::InvalidNumber {
            flag,
            value: value.to_string(),
        }),
    }
}

/// Parst das Ausgabeformat. MVP 0 kennt nur `ppm` (Grossschreibung egal).
fn parse_format(value: &str) -> Result<OutputFormat, CliError> {
    if value.eq_ignore_ascii_case("ppm") {
        Ok(OutputFormat::Ppm)
    } else {
        Err(CliError::UnknownFormat(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_positionals_yield_defaults() {
        let parsed = parse_args(["a.narsil", "b.ppm"]);
        assert!(matches!(&parsed, Ok(Some(_))));
        if let Ok(Some(cli)) = parsed {
            assert_eq!(cli.input, PathBuf::from("a.narsil"));
            assert_eq!(cli.output, Some(PathBuf::from("b.ppm")));
            assert_eq!(cli.width, None);
            assert_eq!(cli.height, None);
            assert_eq!(cli.format, OutputFormat::Ppm);
            assert!(!cli.window);
            assert!(!cli.help);
            assert!(!cli.version);
        }
    }

    #[test]
    fn equals_form_is_split() {
        assert_eq!(split_equals("--width=800"), Some(("--width", "800")));
        assert_eq!(split_equals("a=b.narsil"), None);
        assert_eq!(split_equals("--help"), None);
    }
}
