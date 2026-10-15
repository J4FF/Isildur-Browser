//! Release 10.10.2026: XHTML (`application/xhtml+xml`) fuer den HTML-Parser.
//!
//! Ein XHTML-Dokument liest der Browser mit dem XML-Parser. Narsil hat
//! keinen; die zwei Unterschiede, die echte XHTML-Seiten sichtbar trennen,
//! werden hier VOR dem HTML-Parser in HTML-Form gebracht:
//!
//! * `<![CDATA[ … ]]>` ist in XML Text. In `<style>`/`<script>` bleiben nur
//!   die Huellen weg (HTML liest den Inhalt dort ohnehin roh), anderswo wird
//!   der Inhalt als maskierter Text eingesetzt (HTML machte daraus einen
//!   Kommentar und verlor ihn).
//! * `<div/>` ist in XML ein LEERES Element, in HTML ein offenes Tag; es
//!   wird zu `<div></div>` (leere Elemente wie `<br/>` bleiben).
//!
//! GEMESSEN vorher (Web-Platform-Tests, `css/CSS2`): die Referenzen tragen ihr
//! CSS als `<style><![CDATA[ … ]]></style>` — die erste Regel war ungueltig,
//! 1 301 von 3 318 gescheiterten Reftests hatten CDATA in Test oder Referenz.

use crate::OHNE_SCHLIESSEN;

fn beginnt(b: &[u8], i: usize, muster: &[u8]) -> bool {
    b.len() >= i + muster.len() && &b[i..i + muster.len()] == muster
}

fn beginnt_klein(b: &[u8], i: usize, muster: &[u8]) -> bool {
    b.len() >= i + muster.len() && b[i..i + muster.len()].eq_ignore_ascii_case(muster)
}

fn finde(b: &[u8], ab: usize, muster: &[u8]) -> Option<usize> {
    if ab >= b.len() {
        return None;
    }
    b[ab..].windows(muster.len()).position(|w| w == muster).map(|p| ab + p)
}

fn maskiert(text: &str, aus: &mut String) {
    for c in text.chars() {
        match c {
            '&' => aus.push_str("&amp;"),
            '<' => aus.push_str("&lt;"),
            '>' => aus.push_str("&gt;"),
            c => aus.push(c),
        }
    }
}

/// Release 11.10.2026 (Agent CSS2): eine XML-Zeichenreferenz am Anfang von
/// `t` (`&gt;`, `&lt;`, `&amp;`, `&quot;`, `&apos;`, `&#NN;`, `&#xHH;`) —
/// das Zeichen und die Laenge der Referenz.
fn zeichenreferenz(t: &str) -> Option<(char, usize)> {
    let ende = t.get(..t.len().min(12))?.find(';')?;
    let name = &t[1..ende];
    let c = match name {
        "gt" => '>',
        "lt" => '<',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let zahl = name.strip_prefix('#')?;
            let wert = match zahl.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => zahl.parse::<u32>().ok()?,
            };
            char::from_u32(wert).filter(|c| *c != '\0')?
        }
    };
    Some((c, ende + 1))
}

/// XHTML-Quelltext in die HTML-Form, die [`crate::parse_html`] wie ein
/// XML-Parser versteht (siehe Moduldoku).
pub fn xhtml_als_html(text: &str) -> String {
    let b = text.as_bytes();
    let mut aus = String::with_capacity(text.len() + text.len() / 32);
    let mut roh: Option<String> = None;
    let mut i = 0;
    let mut stueck = 0;
    // Release 11.10.2026 (Agent CSS2): in `<style>`/`<script>` AUSSERHALB
    // von CDATA ersetzt der XML-Parser Zeichenreferenzen (XML 1.0 §4.4.2;
    // HTML liest dort roh). GEMESSEN vorher (WPT `inline-table-zorder-003`,
    // `div#test &gt; span { ... }`): `&gt;` blieb stehen, der Selektor war
    // ungueltig.
    let mut in_cdata = false;
    while i < b.len() {
        if let Some(name) = &roh {
            if beginnt(b, i, b"<![CDATA[") {
                aus.push_str(&text[stueck..i]);
                i += 9;
                stueck = i;
                in_cdata = true;
                continue;
            }
            if beginnt(b, i, b"]]>") {
                aus.push_str(&text[stueck..i]);
                i += 3;
                stueck = i;
                in_cdata = false;
                continue;
            }
            if !in_cdata
                && b[i] == b'&'
                && let Some((zeichen, laenge)) = zeichenreferenz(&text[i..])
                && !(zeichen == '<' && b.get(i + laenge) == Some(&b'/'))
            {
                aus.push_str(&text[stueck..i]);
                aus.push(zeichen);
                i += laenge;
                stueck = i;
                continue;
            }
            if b[i] == b'<' && b.get(i + 1) == Some(&b'/') && beginnt_klein(b, i + 2, name.as_bytes()) {
                roh = None;
            }
            i += 1;
            continue;
        }
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if beginnt(b, i, b"<!--") {
            i = finde(b, i + 4, b"-->").map_or(b.len(), |e| e + 3);
            continue;
        }
        if beginnt(b, i, b"<![CDATA[") {
            aus.push_str(&text[stueck..i]);
            let ende = finde(b, i + 9, b"]]>").unwrap_or(b.len());
            maskiert(&text[i + 9..ende], &mut aus);
            i = (ende + 3).min(b.len());
            stueck = i;
            continue;
        }
        if !b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        // Ein Start-Tag: Name, dann Attribute (Anfuehrungszeichen beachten).
        let mut j = i + 1;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b':' | b'_' | b'.')) {
            j += 1;
        }
        let name = text[i + 1..j].to_ascii_lowercase();
        let mut q: Option<u8> = None;
        let mut k = j;
        while k < b.len() {
            match (q, b[k]) {
                (Some(z), c) if c == z => q = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => q = Some(b[k]),
                (None, b'>') => break,
                _ => {}
            }
            k += 1;
        }
        if k >= b.len() {
            break;
        }
        let selbst = k > j && b[k - 1] == b'/';
        if selbst && !OHNE_SCHLIESSEN.contains(&name.as_str()) {
            aus.push_str(&text[stueck..k - 1]);
            aus.push('>');
            aus.push_str("</");
            aus.push_str(&name);
            aus.push('>');
            stueck = k + 1;
        } else if !selbst && matches!(name.as_str(), "script" | "style") {
            roh = Some(name);
        }
        i = k + 1;
    }
    aus.push_str(&text[stueck.min(text.len())..]);
    aus
}

#[cfg(test)]
mod tests {
    use super::xhtml_als_html;

    #[test]
    fn cdata_im_stil_verliert_die_huelle() {
        let t = xhtml_als_html("<style type=\"text/css\"><![CDATA[\n div { color: green }\n ]]></style><p>x</p>");
        assert_eq!(t, "<style type=\"text/css\">\n div { color: green }\n </style><p>x</p>");
    }

    /// Release 11.10.2026 (Agent CSS2): Zeichenreferenzen im Stil ausserhalb
    /// von CDATA ersetzt der XML-Parser (WPT `inline-table-zorder-003`).
    #[test]
    fn zeichenreferenzen_im_stil_werden_ersetzt() {
        assert_eq!(xhtml_als_html("<style>a &gt; b, i&#160;{}</style>"), "<style>a > b, i\u{a0}{}</style>");
        assert_eq!(xhtml_als_html("<style><![CDATA[a &gt; b]]></style>"), "<style>a &gt; b</style>");
        assert_eq!(xhtml_als_html("<style>x &lt;/style></style>"), "<style>x &lt;/style></style>", "kein falsches Ende");
    }

    #[test]
    fn cdata_im_text_wird_maskierter_text() {
        assert_eq!(xhtml_als_html("<p><![CDATA[a<b & c]]></p>"), "<p>a&lt;b &amp; c</p>");
    }

    #[test]
    fn selbst_schliessende_elemente_werden_leer() {
        assert_eq!(
            xhtml_als_html("<div id=\"a/\"/><br/><span title='x/>y' />z"),
            "<div id=\"a/\"></div><br/><span title='x/>y' ></span>z"
        );
    }

    #[test]
    fn kommentare_und_skripte_bleiben() {
        let q = "<!-- <div/> --><script>if (a < b) x = '<p/>';</script><p/>";
        assert_eq!(xhtml_als_html(q), "<!-- <div/> --><script>if (a < b) x = '<p/>';</script><p></p>");
    }
}
