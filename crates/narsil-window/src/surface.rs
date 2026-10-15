//! Senke fuer gerenderte Pixeldaten.
//!
//! Der [`Surface`]-Trait ist die einzige Schnittstelle zwischen Rasterizer
//! und Fensterschicht. Eine spaetere echte GPU- oder Fensteranbindung
//! implementiert genau diesen Trait, ohne dass sich am Rasterizer etwas
//! aendert.
//!
//! Vereinbarungen:
//! * `present` prueft die Laenge von `rgba` gegen `width * height * 4` und
//!   liefert [`SurfaceError::SizeMismatch`], statt zu paniken.
//! * `present` mit `width == 0` oder `height == 0` liefert
//!   [`SurfaceError::ZeroExtent`].
//! * Die Senke muss die Daten kopieren, wenn sie sie behalten will.
//!   Der Aufrufer darf seinen Slice nach dem Aufruf veraendern oder
//!   verwerfen.

use narsil_core::geometry::Size;

/// Senke, die gerenderte Pixeldaten entgegennimmt.
///
/// Der Trait ist die einzige Schnittstelle zwischen Rasterizer und
/// Fensterschicht. Eine spaetere echte GPU-Anbindung implementiert
/// genau diesen Trait — ohne dass sich am Rasterizer etwas aendert.
pub trait Surface {
    /// Aktuelle Groesse der Senke in physischen Pixeln.
    fn size(&self) -> Size;

    /// Bekommt RGBA8-Pixeldaten.
    ///
    /// `rgba` muss genau `width * height * 4` Byte lang sein,
    /// Zeilenweise von oben nach unten, Kanaele R, G, B, A.
    /// Die Senke muss die Daten kopieren, wenn sie sie braucht.
    ///
    /// Fehler statt Panic:
    /// * `width == 0` oder `height == 0` -> [`SurfaceError::ZeroExtent`].
    /// * `rgba.len() != width * height * 4` ->
    ///   [`SurfaceError::SizeMismatch`].
    fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError>;

    /// Name der Senke, nur fuer Diagnose.
    fn name(&self) -> &str;
}

/// Fehler der Praesentationsschicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// Laenge von `rgba` passt nicht zu Breite x Hoehe.
    SizeMismatch {
        /// Erwartete Bytezahl (`width * height * 4`).
        expected: usize,
        /// Tatsaechlich uebergebene Bytezahl.
        actual: usize,
    },
    /// Breite oder Hoehe ist 0.
    ZeroExtent {
        /// Uebergebene Breite.
        width: u32,
        /// Uebergebene Hoehe.
        height: u32,
    },
    /// Die Senke ist im Fehlerzustand (z. B. Geraet verloren).
    ///
    /// Fuer TASK_0009 reserviert: ein spaeteres echtes Fenster-Backend
    /// meldet hier Treiber-Absturz oder entferntes Geraet. Wird aktuell
    /// im Produktivcode nirgends erzeugt.
    DeviceLost(String),
    /// Eingabe war ungueltig. Der Aufrufer kann sie korrigieren.
    /// Kein Geraeteverlust — das Fenster ist funktionsfaehig.
    InvalidArgument(String),
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceError::SizeMismatch { expected, actual } => write!(
                f,
                "Bilddaten passen nicht zur Groesse: erwartet {expected} Bytes, erhalten {actual} Bytes"
            ),
            SurfaceError::ZeroExtent { width, height } => write!(
                f,
                "ungueltige Ausdehnung: Breite {width}, Hoehe {height} (0 ist nicht erlaubt)"
            ),
            SurfaceError::DeviceLost(msg) => write!(f, "Praesentationsfehler: {msg}"),
            SurfaceError::InvalidArgument(msg) => write!(f, "ungueltige Eingabe: {msg}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Erwartete Bytezahl fuer `width * height` RGBA8-Pixel.
///
/// Rechnet in `u128`, damit auch `u32::MAX x u32::MAX` nicht ueberlaeuft
/// und panict. Gibt `None` zurueck, wenn das Ergebnis nicht in `usize`
/// passt.
fn expected_len(width: u32, height: u32) -> Option<usize> {
    let want = width as u128 * height as u128 * 4_u128;
    if want > usize::MAX as u128 {
        None
    } else {
        Some(want as usize)
    }
}

/// Prueft `present`-Eingaben ohne zu paniken.
///
/// Gibt die erwartete Bytezahl zurueck, damit der Aufrufer sie fuer
/// `SizeMismatch` verwenden kann.
pub(crate) fn check_present_args(rgba_len: usize, width: u32, height: u32) -> Result<usize, SurfaceError> {
    if width == 0 || height == 0 {
        return Err(SurfaceError::ZeroExtent { width, height });
    }
    match expected_len(width, height) {
        Some(expected) => {
            if rgba_len == expected {
                Ok(expected)
            } else {
                Err(SurfaceError::SizeMismatch {
                    expected,
                    actual: rgba_len,
                })
            }
        }
        None => Err(SurfaceError::SizeMismatch {
            expected: usize::MAX,
            actual: rgba_len,
        }),
    }
}

/// Testsenke, die das letzte Bild im Speicher behaelt.
///
/// `MemorySurface` ist das wichtigste Testwerkzeug dieser Schicht:
/// damit wird die Trait-Vereinbarung (Laengenpruefung, Kopie,
/// Null-Ausdehnung) ohne Fenster, ohne Grafik und ohne Dateisystem
/// geprueft.
#[derive(Debug, Clone)]
pub struct MemorySurface {
    name: String,
    width: u32,
    height: u32,
    buffer: Vec<u8>,
}

impl MemorySurface {
    /// Neue Testsenke mit Diagnose-Namen und Anfangsgroesse.
    ///
    /// Es wird noch kein Bild gespeichert (`has_image() == false`).
    /// Die Groesse darf 0 enthalten; erst `present` lehnt 0-Masse mit
    /// `ZeroExtent` ab.
    pub fn new(name: &str, width: u32, height: u32) -> Self {
        Self {
            name: name.to_string(),
            width,
            height,
            buffer: Vec::new(),
        }
    }

    /// Breite der Senke in physischen Pixeln.
    ///
    /// Nach einem erfolgreichen `present` ist das die zuletzt
    /// praesentierte Breite, davor die Konstruktionsbreite.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Hoehe der Senke in physischen Pixeln.
    ///
    /// Nach einem erfolgreichen `present` ist das die zuletzt
    /// praesentierte Hoehe, davor die Konstruktionshoehe.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Ob bereits ein Bild per `present` gespeichert wurde.
    pub fn has_image(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Gespeicherte Pixeldaten (RGBA8, Zeilen von oben nach unten).
    ///
    /// Leerer Slice, solange noch kein Bild praesentiert wurde.
    pub fn pixels(&self) -> &[u8] {
        &self.buffer
    }

    /// Gespeichertes Bild mit Massen, `None` solange keins da ist.
    pub fn last_image(&self) -> Option<(&[u8], u32, u32)> {
        if self.buffer.is_empty() {
            None
        } else {
            Some((&self.buffer, self.width, self.height))
        }
    }
}

impl Surface for MemorySurface {
    fn size(&self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }

    fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError> {
        check_present_args(rgba.len(), width, height)?;
        // Kopie: die Senke behaelt eigene Bytes, der Aufrufer-Slice darf
        // danach veraendert oder verworfen werden.
        self.buffer.clear();
        self.buffer.extend_from_slice(rgba);
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_rejects_zero_extent() {
        let err = check_present_args(0, 0, 10);
        assert_eq!(
            err,
            Err(SurfaceError::ZeroExtent {
                width: 0,
                height: 10
            })
        );
        let err = check_present_args(0, 10, 0);
        assert_eq!(
            err,
            Err(SurfaceError::ZeroExtent {
                width: 10,
                height: 0
            })
        );
    }

    #[test]
    fn check_detects_size_mismatch() {
        let err = check_present_args(3, 1, 1);
        assert_eq!(
            err,
            Err(SurfaceError::SizeMismatch {
                expected: 4,
                actual: 3
            })
        );
    }

    #[test]
    fn memory_surface_starts_without_image() {
        let s = MemorySurface::new("test", 4, 4);
        assert!(!s.has_image());
        assert_eq!(s.last_image(), None);
        assert_eq!(s.pixels().len(), 0);
    }

    #[test]
    fn surface_error_display_mentions_sizes() {
        let e = SurfaceError::SizeMismatch {
            expected: 4,
            actual: 3,
        };
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains('4'));
        assert!(text.contains('3'));
        let e = SurfaceError::ZeroExtent {
            width: 0,
            height: 5,
        };
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains('0'));
        let e = SurfaceError::InvalidArgument("breite 0".into());
        let text = std::string::ToString::to_string(&e);
        assert!(text.contains("breite"));
    }
}
