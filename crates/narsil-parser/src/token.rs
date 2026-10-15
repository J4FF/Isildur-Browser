/// Lexikalische Tokens der Narsil-Sprache.
///
/// Jedes Token trägt einen `Span`, der die Position im Quelltext angibt.
/// Der Parser nutzt diese Spans für Fehlermeldungen.
use narsil_dom::Span;

/// Art eines Tokens.
///
/// `Eq` ist hier bewusst NICHT abgeleitet: `Float(f32)` erfuellt die
/// Anforderungen von `Eq` nicht.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// Schlüsselwort `page`
    Page,
    /// Schlüsselwort `container`
    Container,
    /// Schlüsselwort `text`
    Text,
    /// Bezeichner (Property-Namen)
    Ident(String),
    /// Ganzzahl: `800`, `24`
    Number(u32),
    /// Dezimalzahl: `24.5`, `0.5`
    Float(f32),
    /// Prozentwert: `50%`
    Percent(f32),
    /// Schlüsselwort `auto`
    Auto,
    /// Farbe: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`
    Color(String),
    /// String-Literal: `"Hallo Welt"`
    String(String),
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `row`
    Row,
    /// `column`
    Column,
    /// Dateiende
    Eof,
}

/// Ein Token mit seiner Position im Quelltext.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    /// Erzeugt ein neues Token.
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Prüft, ob das Token dieselbe Variante ist.
    ///
    /// ACHTUNG: vergleicht NUR die Variante, NICHT den Inhalt.
    /// `Ident("width".into())` und `Ident("height".into())` sind hier
    /// gleich. Für Payload-Vergleiche `match` benutzen, nicht `is()`.
    pub fn is(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.kind) == std::mem::discriminant(kind)
    }

    /// Prüft Variante UND Inhalt.
    pub fn is_exactly(&self, kind: &TokenKind) -> bool {
        &self.kind == kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_dom::Span;

    #[test]
    fn token_creation() {
        let span = Span::new(0, 4);
        let token = Token::new(TokenKind::Page, span);
        assert_eq!(token.span, span);
        assert!(token.is(&TokenKind::Page));
    }
}
