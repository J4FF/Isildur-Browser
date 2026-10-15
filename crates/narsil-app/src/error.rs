//! Fehlertypen der Anwendungsschicht.
//!
//! Alle Meldungen sind deutsch und nennen konkrete Werte, damit der
//! Aufrufer sofort sieht, was falsch war. Fehler mit Ort im Quelltext
//! tragen Stufe plus `Span`; ein `String` allein koennte danach nicht
//! mehr ausgewertet, nur noch angezeigt werden.

use std::path::PathBuf;

use narsil_dom::Span;

/// In welcher Pipeline-Stufe ein Fehler passiert ist.
///
/// Der Nutzer sieht an der Meldung, WO es passiert ist, nicht nur dass
/// es passiert ist: ein Layout-Fehler ohne Span ("Viewport ist 0 breit")
/// hoert sich anders an als ein Parser-Fehler mit Zeile und Spalte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Quelltext einlesen und pruefen.
    Parse,
    /// Dokument in Geometrie rechnen.
    Layout,
    /// Display-Liste in Pixel rechnen.
    Raster,
    /// Die Anwendung selbst (z. B. Platzhalter bis TASK_0009).
    App,
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Stage::Parse => "Parser",
            Stage::Layout => "Layout",
            Stage::Raster => "Rasterizer",
            Stage::App => "Anwendung",
        };
        f.write_str(name)
    }
}

/// Fehler beim Auswerten der Kommandozeile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// Pflichtargument fehlt.
    MissingArgument(&'static str),
    /// Unbekanntes Flag.
    UnknownFlag(String),
    /// Flag ohne Wert.
    MissingValue(&'static str),
    /// Wert ist keine gueltige Zahl.
    InvalidNumber {
        /// Das Flag, z. B. `"--width"`.
        flag: &'static str,
        /// Der uebergebene Text.
        value: String,
    },
    /// Wert ist <= 0.
    NotPositive {
        /// Das Flag, z. B. `"--width"`.
        flag: &'static str,
        /// Der uebergebene Wert.
        value: u32,
    },
    /// Mehr Input-Dateien angegeben.
    TooManyInputs(usize),
    /// Ausgabeformat unbekannt.
    UnknownFormat(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::MissingArgument(what) => {
                write!(f, "fehlendes Argument: {what} erwartet")
            }
            CliError::UnknownFlag(flag) => write!(f, "unbekannte Option: {flag:?}"),
            CliError::MissingValue(flag) => write!(f, "Option {flag} braucht einen Wert"),
            CliError::InvalidNumber { flag, value } => {
                write!(f, "ungueltiger Zahlenwert fuer {flag}: {value:?}")
            }
            CliError::NotPositive { flag, value } => {
                write!(
                    f,
                    "Wert fuer {flag} muss groesser als 0 sein, erhalten: {value}"
                )
            }
            CliError::TooManyInputs(n) => write!(
                f,
                "zu viele Eingabedateien: {n} angegeben, hoechstens 2 (Eingabe + Ausgabe)"
            ),
            CliError::UnknownFormat(fmt) => {
                write!(f, "unbekanntes Ausgabeformat: {fmt:?} (nur \"ppm\")")
            }
        }
    }
}

impl std::error::Error for CliError {}

/// Fehler eines Anwendungslaufs.
#[derive(Debug)]
pub enum AppError {
    /// Kommandozeile war falsch.
    Cli(CliError),
    /// Eingabedatei nicht lesbar.
    Read {
        /// Die fragliche Datei.
        path: PathBuf,
        /// Der IO-Fehler.
        source: std::io::Error,
    },
    /// Ausgabe konnte nicht geschrieben werden.
    Write {
        /// Die fragliche Datei.
        path: PathBuf,
        /// Der IO-Fehler.
        source: std::io::Error,
    },
    /// Fehler einer Pipeline-Stufe, mit Ort im Quelltext.
    Source {
        /// Welche Stufe.
        stage: Stage,
        /// Das Problem in einem Satz.
        message: String,
        /// Wo im Quelltext, wenn die Stufe das weiss.
        span: Option<Span>,
    },
    /// Fehler ohne Ort: Aufruf mit ungueltigen Argumenten jenseits der
    /// Kommandozeile, Stufen-Fehler ohne Span. Dateizugriff bleibt bei
    /// `Read`/`Write`.
    Message {
        /// Welche Stufe.
        stage: Stage,
        /// Das Problem in einem Satz.
        message: String,
    },
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Cli(e) => write!(f, "Kommandozeile: {e}"),
            AppError::Read { path, source } => {
                write!(f, "Eingabedatei {} nicht lesbar: {source}", path.display())
            }
            AppError::Write { path, source } => {
                write!(f, "Ausgabe {} nicht schreibbar: {source}", path.display())
            }
            AppError::Source {
                stage,
                message,
                span,
            } => match span {
                Some(s) => write!(f, "{stage} [Byte {}+{}]: {message}", s.start, s.len),
                None => write!(f, "{stage}: {message}"),
            },
            AppError::Message { stage, message } => write!(f, "{stage}: {message}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Cli(e) => Some(e),
            AppError::Read { source, .. } | AppError::Write { source, .. } => Some(source),
            AppError::Source { .. } | AppError::Message { .. } => None,
        }
    }
}

impl From<CliError> for AppError {
    /// Wandelt einen CLI-Fehler in einen Anwendungsfehler um.
    fn from(e: CliError) -> Self {
        AppError::Cli(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_display_names_values() {
        assert!(
            std::string::ToString::to_string(&CliError::MissingArgument("<input>"))
                .contains("<input>")
        );
        assert!(
            std::string::ToString::to_string(&CliError::UnknownFlag("--x".into())).contains("--x")
        );
        assert!(std::string::ToString::to_string(&CliError::TooManyInputs(3)).contains('3'));
    }

    #[test]
    fn app_error_converts_from_cli() {
        let e: AppError = CliError::MissingArgument("<input>").into();
        assert!(matches!(e, AppError::Cli(_)));
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn source_display_names_stage_message_and_span() {
        let e = AppError::Source {
            stage: Stage::Parse,
            message: "erwartet ':', gefunden '}'".into(),
            span: Some(Span::new(312, 1)),
        };
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains("Parser"));
        assert!(text.contains("erwartet ':'"));
        assert!(text.contains("312"));
    }

    #[test]
    fn source_without_span_names_stage_and_message() {
        let e = AppError::Source {
            stage: Stage::Layout,
            message: "Viewport ist 0 breit".into(),
            span: None,
        };
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains("Layout"));
        assert!(text.contains("Viewport ist 0 breit"));
    }

    #[test]
    fn message_display_names_stage_and_message() {
        let e = AppError::Message {
            stage: Stage::Raster,
            message: "Schrift fehlt".into(),
        };
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains("Rasterizer"));
        assert!(text.contains("Schrift fehlt"));
        assert!(std::error::Error::source(&e).is_none());
    }

    #[test]
    fn stage_names_are_german() {
        assert_eq!(std::string::ToString::to_string(&Stage::Parse), "Parser");
        assert_eq!(std::string::ToString::to_string(&Stage::Layout), "Layout");
        assert_eq!(
            std::string::ToString::to_string(&Stage::Raster),
            "Rasterizer"
        );
        assert_eq!(std::string::ToString::to_string(&Stage::App), "Anwendung");
    }
}
