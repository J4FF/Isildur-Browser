//! Byte-Offset wird zu Zeile, Spalte und Quelltext-Auszug.
//!
//! Der Parser meldet Fehler als [`Span`](narsil_dom::Span) mit Byte-Offset.
//! Ein Mensch kann mit Offset 312 nichts anfangen, mit "Zeile 14, Spalte 22"
//! dagegen sofort — sein Editor zeigt dieselben Zahlen. Diese Map schliesst
//! die Luecke, ohne je zu paniken (auch nicht bei kaputten Spans).

use std::path::Path;

use narsil_dom::Span;

use crate::error::Stage;

/// Rechnet Byte-Offsets in Zeilen/Spalten um und formatiert Auszuege.
///
/// Speicher: Quelltext plus ein `u32` pro Zeilenanfang — linear in der
/// Dateigroesse (bei 1 MB also ~1 MB plus 4 Byte mal Zeilenzahl).
#[derive(Debug, Clone)]
pub struct SourceMap {
    source: String,
    /// Byte-Offsets aller Zeilenanfaenge, aufsteigend, beginnt mit 0.
    /// Jeder Eintrag liegt auf einer Zeichengrenze (`\n`/`\r` sind ASCII).
    line_starts: Vec<u32>,
}

impl SourceMap {
    /// Baut die Map aus dem vollstaendigen Quelltext.
    ///
    /// Zeilenenden `\n`, `\r\n` und `\r` werden alle korrekt gezaehlt;
    /// `\r\n` gilt als EIN Ende, nicht als zwei.
    pub fn new(source: &str) -> Self {
        let mut line_starts: Vec<u32> = vec![0];
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                line_starts.push((i + 1) as u32);
                i += 1;
            } else if bytes[i] == b'\r' {
                if bytes.get(i + 1) == Some(&b'\n') {
                    line_starts.push((i + 2) as u32);
                    i += 2;
                } else {
                    line_starts.push((i + 1) as u32);
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        Self {
            source: source.to_string(),
            line_starts,
        }
    }

    /// Byte-Offset -> (Zeile, Spalte), beide 1-basiert.
    ///
    /// Die Spalte zaehlt ZEICHEN, nicht Bytes (`ü` ist eine Spalte).
    /// Regeln fuer kaputte Eingaben (nie Panic):
    /// * Offset in der Mitte eines Mehrbyte-Zeichens faellt auf die
    ///   Zeichengrenze links davon zurueck.
    /// * Offset hinter dem Dateiende wird ans Dateiende geklemmt.
    /// * Ein Offset genau auf `\r` oder `\n` gehoert noch zur alten Zeile
    ///   (Spalte zeigt auf das Umbruchzeichen als letztes Zeichen).
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let clamped = self.clamp_to_boundary(offset);
        let line_idx = self.line_index(clamped);
        let line_start = self.line_starts[line_idx] as usize;
        let col = self.source[line_start..clamped].chars().count() as u32 + 1;
        (line_idx as u32 + 1, col)
    }

    /// Quelltext-Auszug um die Stelle herum, mit einem Caret `^` darunter.
    ///
    /// Zeigt die betroffene Zeile (ohne Umbruchzeichen) plus eine
    /// Caret-Zeile. `len` markiert zusaetzlich `~` fuer weitere betroffene
    /// Zeichen derselben Zeile. Kaputte Offsets folgen denselben Regeln
    /// wie [`line_col`](Self::line_col); es gibt keine Kuerzung langer
    /// Zeilen (bewusst: Kuerzen wuerde den Caret verschieben — siehe
    /// FERTIG_0012.txt, Frage 3).
    pub fn excerpt(&self, offset: u32, len: u32) -> String {
        let clamped = self.clamp_to_boundary(offset);
        let line_idx = self.line_index(clamped);
        let line_start = self.line_starts[line_idx] as usize;
        let line_end = self.line_end(line_idx);
        let line_text: String = self.source[line_start..line_end]
            .trim_end_matches(['\r', '\n'])
            .to_string();
        let caret_col = self.source[line_start..clamped].chars().count();
        let marked = self.marked_chars(clamped, len, line_end);
        let mut caret = String::new();
        for _ in 0..caret_col {
            caret.push(' ');
        }
        caret.push('^');
        for _ in 1..marked {
            caret.push('~');
        }
        std::format!("{line_text}\n{caret}")
    }

    /// Zeilenanfang-Index fuer einen geklemmten Offset (binaere Suche).
    fn line_index(&self, clamped: usize) -> usize {
        let mut lo = 0;
        let mut hi = self.line_starts.len();
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.line_starts[mid] as usize <= clamped {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Ende der Zeile (Beginn der naechsten oder Dateiende).
    fn line_end(&self, line_idx: usize) -> usize {
        if line_idx + 1 < self.line_starts.len() {
            self.line_starts[line_idx + 1] as usize
        } else {
            self.source.len()
        }
    }

    /// Klemmt auf Dateiende und faellt auf die Zeichengrenze links zurueck.
    fn clamp_to_boundary(&self, offset: u32) -> usize {
        let mut at = (offset as usize).min(self.source.len());
        while at > 0 && !self.source.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// Wie viele Zeichen ab `clamped` als betroffen markiert werden
    /// (mindestens 1, hoechstens bis Zeilenende, ohne Panic).
    fn marked_chars(&self, clamped: usize, len: u32, line_end: usize) -> usize {
        let mut end = clamped.saturating_add(len as usize).min(line_end);
        while end > clamped && !self.source.is_char_boundary(end) {
            end -= 1;
        }
        let n = self.source[clamped..end].chars().count();
        n.max(1)
    }
}

/// Formatiert einen Quelltext-Fehler als `datei:zeile:spalte: meldung`
/// plus Auszug mit Caret (stderr-Format, stdout bleibt fuers Bild).
///
/// Ohne `span` entfaellt die Position: `datei: [Stufe] meldung`.
/// Siehe [`format_errors`] fuer mehrere Fehler untereinander.
pub fn format_error(
    path: &Path,
    source: &str,
    stage: Stage,
    span: Option<Span>,
    message: &str,
) -> String {
    format_errors(path, source, &[(stage, span, message)])
}

/// Formatiert mehrere Fehler untereinander, jeder mit eigener Position.
///
/// `ParseError` ist derzeit ein einzelner Fehler; sobald eine Stufe mehrere
/// liefert, landen sie hier unveraendert in Reihenfolge im Text.
pub fn format_errors(path: &Path, source: &str, errors: &[(Stage, Option<Span>, &str)]) -> String {
    let map = SourceMap::new(source);
    let mut out = String::new();
    for (i, (stage, span, message)) in errors.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match span {
            Some(s) => {
                let (line, col) = map.line_col(s.start);
                out.push_str(&std::format!(
                    "{}:{line}:{col}: [{stage}] {message}\n\n{}",
                    path.display(),
                    map.excerpt(s.start, s.len)
                ));
            }
            None => {
                out.push_str(&std::format!("{}: [{stage}] {message}", path.display()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_byte_is_line_1_col_1() {
        let m = SourceMap::new("page {\n}");
        assert_eq!(m.line_col(0), (1, 1));
    }

    #[test]
    fn second_line_offsets() {
        let m = SourceMap::new("ab\ncd");
        assert_eq!(m.line_col(3), (2, 1));
        assert_eq!(m.line_col(4), (2, 2));
    }

    #[test]
    fn multibyte_column_counts_chars_not_bytes() {
        // Fall (a): Offset auf dem `ü` (2 Bytes) ergibt Spalte 3.
        let src = "Grüße";
        let off = src.find('ü').unwrap_or(0) as u32;
        let m = SourceMap::new(src);
        assert_eq!(m.line_col(off), (1, 3));
    }

    #[test]
    fn mid_char_offset_falls_back_left_without_panic() {
        // Fall (b): Mitte von `ü` -> Grenze links davon, also Spalte 3.
        let src = "Grüße";
        let ue = src.find('ü').unwrap_or(0);
        let m = SourceMap::new(src);
        assert_eq!(m.line_col((ue + 1) as u32), (1, 3));
        let text = m.excerpt((ue + 1) as u32, 1);
        assert!(text.contains('^'));
    }

    #[test]
    fn offset_past_end_clamps_without_panic() {
        // Fall (c): hinter dem Ende -> ans Dateiende geklemmt.
        let m = SourceMap::new("ab");
        assert_eq!(m.line_col(99), (1, 3));
        assert_eq!(m.line_col(u32::MAX), (1, 3));
        let text = m.excerpt(99, 5);
        assert!(text.contains("ab"));
    }

    #[test]
    fn crlf_counts_as_single_ending() {
        // Fall (d): \r\n ist EIN Ende.
        let m = SourceMap::new("a\r\nb");
        assert_eq!(m.line_col(3), (2, 1));
        assert_eq!(m.line_col(0), (1, 1));
    }

    #[test]
    fn lone_cr_is_an_ending() {
        let m = SourceMap::new("a\rb");
        assert_eq!(m.line_col(2), (2, 1));
    }

    #[test]
    fn german_text_with_umlauts() {
        let src = "page {\n  text \"Grüße aus München\" {\n  }\n}";
        let m = SourceMap::new(src);
        let off = src.find("München").unwrap_or(0) as u32;
        let (line, col) = m.line_col(off);
        assert_eq!(line, 2);
        // `  text "Grüße aus ` davor: 2 + 4 + 1 + 1 + 5 + 1 + 3 + 1 = 18
        // Zeichen -> Spalte 19 (Umlaute zaehlen einfach).
        assert_eq!(col, 19);
        let text = m.excerpt(off, 7);
        assert!(text.contains("München"));
        assert!(text.contains('^'));
    }

    #[test]
    fn offset_on_crlf_stays_on_old_line() {
        // Frage 2: Offset auf \r bzw. \n gehoert zur alten Zeile.
        let m = SourceMap::new("a\r\nb");
        assert_eq!(m.line_col(1), (1, 2));
        assert_eq!(m.line_col(2), (1, 3));
    }

    #[test]
    fn empty_source_has_line_1_col_1() {
        let m = SourceMap::new("");
        assert_eq!(m.line_col(0), (1, 1));
        assert_eq!(m.line_col(7), (1, 1));
    }

    #[test]
    fn excerpt_marks_length_with_tildes() {
        let m = SourceMap::new("color: #336699");
        let text = m.excerpt(7, 7);
        assert!(text.contains("#336699"));
        assert!(text.contains("^~~~~~~"));
    }

    #[test]
    fn excerpt_caret_aligns_after_umlaut() {
        let src = "Grüße!";
        let m = SourceMap::new(src);
        let text = m.excerpt(7, 1);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("Grüße!"));
        // `!` ist das 6. Zeichen -> 5 Leerzeichen, dann ^.
        assert_eq!(lines.next(), Some("     ^"));
    }
}
