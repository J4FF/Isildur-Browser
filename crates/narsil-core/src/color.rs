//! Farbtyp der Pipeline.
//!
//! Kanaele sind `u8`. `a` ist reine Deckkraft (source-over), die Farbe ist
//! NICHT premultiplied. `Color::parse_hex` akzeptiert die Formen
//! `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (Gross-/Kleinschreibung egal).

/// Fehlerursache beim Parsen eines Hex-Farbwerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorParseErrorReason {
    /// Fuehrendes `#` fehlt.
    MissingHash,
    /// Laenge ist weder 3, 4, 6 noch 8 Ziffern.
    InvalidLength,
    /// Enthaelt ein Zeichen, das keine Hex-Ziffer ist.
    InvalidDigit,
}

/// Fehler beim Parsen eines Hex-Farbwerts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError {
    pub input: String,
    pub reason: ColorParseErrorReason,
}

impl std::fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.reason {
            ColorParseErrorReason::MissingHash => "fehlendes '#'",
            ColorParseErrorReason::InvalidLength => "falsche Laenge (erlaubt: 3, 4, 6, 8 Ziffern)",
            ColorParseErrorReason::InvalidDigit => "ungueltige Hex-Ziffer",
        };
        write!(f, "ungueltige Farbe {:?}: {}", self.input, reason)
    }
}

impl std::error::Error for ColorParseError {}

/// RGBA-Farbe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Volle Deckkraft.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// Packt nach `0xRRGGBBAA`.
    pub const fn to_rgba_u32(self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | self.a as u32
    }

    /// Entpackt aus `0xRRGGBBAA`.
    pub const fn from_rgba_u32(v: u32) -> Self {
        Self {
            r: ((v >> 24) & 0xff) as u8,
            g: ((v >> 16) & 0xff) as u8,
            b: ((v >> 8) & 0xff) as u8,
            a: (v & 0xff) as u8,
        }
    }

    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Parst `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
    pub fn parse_hex(input: &str) -> Result<Color, ColorParseError> {
        let err = |reason| ColorParseError {
            input: input.to_string(),
            reason,
        };
        let hex = input
            .strip_prefix('#')
            .ok_or_else(|| err(ColorParseErrorReason::MissingHash))?;

        if !matches!(hex.len(), 3 | 4 | 6 | 8) {
            return Err(err(ColorParseErrorReason::InvalidLength));
        }
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(ColorParseErrorReason::InvalidDigit));
        }

        let d = |i: usize| -> u8 {
            // Sicher: Laenge und Zeichen sind oben validiert.
            let v = hex.as_bytes()[i];
            match v {
                b'0'..=b'9' => v - b'0',
                b'a'..=b'f' => v - b'a' + 10,
                b'A'..=b'F' => v - b'A' + 10,
                _ => unreachable!("Hex-Ziffern vorab validiert"),
            }
        };

        let (r, g, b, a) = match hex.len() {
            3 | 4 => {
                let r = d(0) * 17;
                let g = d(1) * 17;
                let b = d(2) * 17;
                let a = if hex.len() == 4 { d(3) * 17 } else { 255 };
                (r, g, b, a)
            }
            _ => {
                let r = d(0) * 16 + d(1);
                let g = d(2) * 16 + d(3);
                let b = d(4) * 16 + d(5);
                let a = if hex.len() == 8 {
                    d(6) * 16 + d(7)
                } else {
                    255
                };
                (r, g, b, a)
            }
        };

        Ok(Color::new(r, g, b, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_six_digits() {
        assert_eq!(Color::parse_hex("#ff5f5f"), Ok(Color::rgb(255, 95, 95)));
        assert_eq!(Color::parse_hex("#000000"), Ok(Color::BLACK));
        assert_eq!(Color::parse_hex("#FFFFFF"), Ok(Color::WHITE));
    }

    #[test]
    fn parse_three_and_four_digits_expand() {
        assert_eq!(Color::parse_hex("#f00"), Ok(Color::rgb(255, 0, 0)));
        assert_eq!(Color::parse_hex("#0f08"), Ok(Color::new(0, 255, 0, 136)));
    }

    #[test]
    fn parse_eight_digits_carries_alpha() {
        assert_eq!(
            Color::parse_hex("#ff5f5f80"),
            Ok(Color::new(255, 95, 95, 128))
        );
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert_eq!(
            Color::parse_hex("ff5f5f").unwrap_err().reason,
            ColorParseErrorReason::MissingHash
        );
        // 5 Ziffern ist weder 3/4 noch 6/8 -> ungueltige Laenge.
        assert_eq!(
            Color::parse_hex("#ff5f5").unwrap_err().reason,
            ColorParseErrorReason::InvalidLength
        );
        // 7 Ziffern ebenfalls.
        assert_eq!(
            Color::parse_hex("#ff5f5f0").unwrap_err().reason,
            ColorParseErrorReason::InvalidLength
        );
        assert_eq!(
            Color::parse_hex("#gg0000").unwrap_err().reason,
            ColorParseErrorReason::InvalidDigit
        );
    }

    #[test]
    fn u32_roundtrip() {
        let c = Color::new(1, 2, 3, 4);
        assert_eq!(Color::from_rgba_u32(c.to_rgba_u32()), c);
    }

    #[test]
    fn alpha_helpers() {
        assert!(Color::TRANSPARENT.is_transparent());
        assert!(Color::WHITE.is_opaque());
        assert!(!Color::new(0, 0, 0, 1).is_transparent());
    }
}
