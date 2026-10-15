//! Laengen und Abstaende.
//!
//! Eine `Length` ist entweder absolut (`Px`), relativ zur Referenzgroesse
//! (`Percent`) oder automatisch (`Auto`). Ungueltige Werte (NaN, negativ,
//! Prozent ausserhalb 0..=100) werden von `resolve` als `None` behandelt,
//! damit NaN nicht in die Layout-Phase durchschlägt.

/// Eine Laengenangabe.
///
/// GEMESSEN 03.10.2026 (clippy `derivable_impls`): `Default` war von Hand
/// geschrieben und gab `Auto` zurueck. Mit `#[derive(Default)]` auf der
/// ERSTEN Variante ist es dasselbe und die Warnung weg. Das `Auto` als
/// erster Eintrag ist hier nicht Kosmetik, sondern die Bedeutung: CSS 2.1
/// 10.3.2 nennt `auto` den Anfangswert fuer die meisten Eigenschaften.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// Wert wird vom Layout berechnet (z. B. Inhaltsgroesse).
    #[default]
    Auto,
    /// Absolute Pixel.
    Px(f32),
    /// Prozent der Referenzgroesse.
    Percent(f32),
    /// Release 11.10.2026: `calc()` mit Prozentanteil — `px + prozent % der
    /// Referenz` (CSS Values 4 §10.9: eine `<length-percentage>` bleibt eine
    /// lineare Mischung, bis die Referenz bekannt ist). GEMESSEN vorher: die
    /// Kaskade rechnete den Prozentanteil gegen die Ansicht (WPT css-flexbox
    /// `gap-010-ltr`, `dynamic-isize-change-003`).
    Gemischt {
        /// Der feste Anteil in px (darf negativ sein: `calc(100% - 20px)`).
        px: f32,
        /// Der Prozentanteil (20 = 20 %).
        prozent: f32,
    },
}

impl Length {
    pub const AUTO: Length = Length::Auto;

    pub const fn px(v: f32) -> Self {
        Length::Px(v)
    }

    pub const fn percent(v: f32) -> Self {
        Length::Percent(v)
    }

    pub const fn is_auto(self) -> bool {
        matches!(self, Length::Auto)
    }

    /// Release 11.10.2026: eine Mischung aus px und Prozent; ohne
    /// Prozentanteil eine reine px-Laenge.
    pub fn gemischt(px: f32, prozent: f32) -> Self {
        if prozent == 0.0 { Length::Px(px) } else { Length::Gemischt { px, prozent } }
    }

    /// Haengt der Wert von einer Referenzgroesse ab (Prozent, auch gemischt)?
    pub const fn hat_prozent(self) -> bool {
        matches!(self, Length::Percent(_) | Length::Gemischt { .. })
    }

    /// `true`, wenn der Wert aufloesbar und gueltig ist.
    ///
    /// GILT FUER `width` UND `height`: ein Prozentwert ueber 100 ist
    /// gueltig. `width: 150%` in einem 800 px breiten Eltern ergibt 1200 px
    /// und ist in v0 ausdruecklich erlaubt — Version 0 clippt nicht.
    /// Der alte Wert (`p <= 100.0`) hat das im Parser verhindert, obwohl
    /// die Layout-Engine es seit TASK_0004 unterstuetzt hat: der Vertrag
    /// war strenger als die Implementierung, und `width: 150%` war ueber
    /// den echten Einstiegspunkt unerreichbar.
    ///
    /// Fuer `margin` und `padding` gilt NICHT dasselbe — dort ist 100 % die
    /// Obergrenze. Dafuer ist [`Length::is_valid_spacing`] zustaendig.
    pub const fn is_valid(self) -> bool {
        match self {
            Length::Auto => true,
            Length::Px(v) => v.is_finite() && v >= 0.0,
            Length::Percent(p) => p.is_finite() && p >= 0.0,
            // Das Ergebnis kann negativ werden; die Aufrufer klemmen es wie
            // jede andere Groesse.
            Length::Gemischt { px, prozent } => px.is_finite() && prozent.is_finite(),
        }
    }

    /// `true`, wenn der Wert fuer `margin`/`padding` gueltig ist.
    ///
    /// Wie [`Length::is_valid`], aber mit der Obergrenze 100 % fuer
    /// Prozentwerte. `margin: 150%` in einem 800 px breiten Eltern ergaebe
    /// 1200 px Abstand auf allen vier Seiten — das ist in v0 verboten,
    /// weil es den Inhalt vollstaendig aus dem Dokument schiebt und es
    /// keinen Weg gibt, es zu bemerken.
    ///
    /// Die Grenze ist eine Entscheidung der SPRACHE, keine Eigenschaft der
    /// Zahl: dieselbe `Length` ist an `width` gueltig und an `margin` nicht.
    pub const fn is_valid_spacing(self) -> bool {
        match self {
            Length::Auto => true,
            Length::Percent(p) => p.is_finite() && p >= 0.0 && p <= 100.0,
            other => other.is_valid(),
        }
    }

    /// Loest gegen eine Referenzgroesse auf.
    /// `None` bei `Auto` oder ungueltigem Wert.
    ///
    /// `None` auch, wenn das ERGEBNIS nicht endlich ist. `is_valid`
    /// prueft die Laenge, nicht die Referenz — und
    /// `Length::percent(50).resolve(f32::NAN)` waere sonst `Some(NaN)`.
    /// Damit das `NaN` nicht durch die Hintertuer zurueckkommt, wird
    /// hier das Ergebnis geprueft, nicht nur die Eingabe. Gefunden in
    /// TASK_0013B, Agent6.
    pub fn resolve(self, reference: f32) -> Option<f32> {
        if !self.is_valid() {
            return None;
        }
        let out = match self {
            Length::Auto => None,
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(reference * p / 100.0),
            Length::Gemischt { px, prozent } => Some(px + reference * prozent / 100.0),
        };
        match out {
            Some(v) if v.is_finite() => Some(v),
            _ => None,
        }
    }

    /// Wie `resolve`, aber `Auto` bzw. ungueltig liefert `auto_value`.
    pub fn resolve_or(self, reference: f32, auto_value: f32) -> f32 {
        self.resolve(reference).unwrap_or(auto_value)
    }
}

/// Breite und Hoehe.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dimension {
    pub width: Length,
    pub height: Length,
}

impl Dimension {
    pub const AUTO: Dimension = Dimension {
        width: Length::Auto,
        height: Length::Auto,
    };

    pub const fn fixed(width: f32, height: f32) -> Self {
        Dimension {
            width: Length::Px(width),
            height: Length::Px(height),
        }
    }

    pub const fn is_valid(&self) -> bool {
        self.width.is_valid() && self.height.is_valid()
    }
}

/// Vier Seitenwerte fuer Margin und Padding.
///
/// Reihenfolge der Felder: links, oben, rechts, unten.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spacing {
    pub left: Length,
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
}

impl Spacing {
    pub const ZERO: Spacing = Spacing {
        left: Length::Px(0.0),
        top: Length::Px(0.0),
        right: Length::Px(0.0),
        bottom: Length::Px(0.0),
    };

    pub const fn all(value: Length) -> Self {
        Spacing {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// `true`, wenn alle vier Seiten fuer `margin`/`padding` gueltig sind.
    ///
    /// Nutzt [`Length::is_valid_spacing`], nicht `is_valid` — deshalb ist
    /// `padding: 150%` ungueltig, waehrend `width: 150%` gueltig ist.
    pub const fn is_valid(&self) -> bool {
        self.left.is_valid_spacing()
            && self.top.is_valid_spacing()
            && self.right.is_valid_spacing()
            && self.bottom.is_valid_spacing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_auto() {
        assert_eq!(Length::default(), Length::Auto);
        assert_eq!(Dimension::default(), Dimension::AUTO);
    }

    #[test]
    fn resolve_absolute_and_percent() {
        assert_eq!(Length::px(10.0).resolve(999.0), Some(10.0));
        assert_eq!(Length::percent(50.0).resolve(200.0), Some(100.0));
        assert_eq!(Length::percent(100.0).resolve(200.0), Some(200.0));
    }

    #[test]
    fn auto_resolves_to_none() {
        assert_eq!(Length::Auto.resolve(100.0), None);
        assert_eq!(Length::Auto.resolve_or(100.0, 42.0), 42.0);
    }

    #[test]
    fn invalid_values_never_produce_nan() {
        // `Length::percent(101.0)` stand hier bis 04:35 als "ungueltig".
        // Das war eine FALSCHE Zusage: 101 % sind fuer `width` gueltig
        // (Version 0 clippt nicht). Ersetzt durch Werte, die fuer JEDE
        // Property ungueltig sind. Die Grenze 100 % gilt nur fuer
        // `margin`/`padding` — siehe `spacing_percent_over_100_is_rejected`.
        for bad in [
            Length::px(f32::NAN),
            Length::px(-1.0),
            Length::px(f32::INFINITY),
            Length::percent(-5.0),
            Length::percent(f32::NAN),
            Length::percent(f32::INFINITY),
        ] {
            assert!(!bad.is_valid(), "{bad:?} sollte ungueltig sein");
            assert_eq!(bad.resolve(100.0), None, "{bad:?} darf nicht aufloesen");
        }
    }

    #[test]
    fn valid_boundaries() {
        assert!(Length::px(0.0).is_valid());
        assert!(Length::percent(0.0).is_valid());
        assert!(Length::percent(100.0).is_valid());
        // Ueber 100: gueltig fuer width/height. Ohne Obergrenze, weil
        // Version 0 nicht clippt — `width: 150%` ergibt 1200 in 800.
        assert!(Length::percent(101.0).is_valid());
        assert!(Length::percent(1000.0).is_valid());
        assert_eq!(Length::percent(150.0).resolve(800.0), Some(1200.0));
    }

    #[test]
    fn spacing_percent_over_100_is_rejected() {
        // Dieselbe Zahl, anderes Feld, anderes Urteil. Genau das ist der
        // Grund, warum es zwei Pruefungen gibt und nicht eine.
        // 100 % ist die Grenze selbst und noch gueltig — `padding: 100%`
        // ist erlaubt, `padding: 101%` nicht.
        assert!(Length::percent(100.0).is_valid_spacing());
        for p in [101.0f32, 150.0, 1000.0] {
            assert!(Length::percent(p).is_valid(), "width/height: {p}% gueltig");
            assert!(
                !Length::percent(p).is_valid_spacing(),
                "margin/padding: {p}% muss ungueltig sein"
            );
        }
        // Pixel sind an beiden Stellen unbegrenzt.
        for p in [10.0f32, 100.0, 1000.0] {
            assert!(Length::px(p).is_valid());
            assert!(Length::px(p).is_valid_spacing());
        }
        // Auto bleibt gueltig: `margin: auto` wird vom Parser abgelehnt
        // (Fehlerfall E15), nicht von diesem Vertrag.
        assert!(Length::Auto.is_valid_spacing());
    }

    #[test]
    fn spacing_uniform_and_validity() {
        let s = Spacing::all(Length::px(4.0));
        assert_eq!(s.left, Length::px(4.0));
        assert_eq!(s.bottom, Length::px(4.0));
        assert!(s.is_valid());
        assert!(!Spacing::all(Length::px(-1.0)).is_valid());
        assert!(!Dimension::fixed(10.0, f32::NAN).is_valid());
    }
}
