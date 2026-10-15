//! Zeichensatz-Erkennung und -Dekodierung (Aufgabe H3).
//!
//! Alles hier folgt dem WHATWG Encoding Standard
//! (https://encoding.spec.whatwg.org/) und dem Prescan des WHATWG HTML
//! Standard (https://html.spec.whatwg.org/multipage/parsing.html).
//!
//! # WAS HIER STEHEN MUSS UND WARUM
//!
//! Eine Seite kommt als STROM VON BYTES an. Ohne diesen Schritt ist jeder
//! Umlaut ein Fragezeichen und jedes japanische Zeichen ein Kasten. Die
//! Erkennung ist kein Ratespiel: die Reihenfolge lautet BOM, dann
//! Transport-Hinweis (`Content-Type`), dann ein Blick in die ersten 1024
//! Byte (`<meta charset=...>`), dann der Fallback windows-1252.
//!
//! # BENANNTE GRENZEN
//!
//! * `MAX_KODIERUNG_EINGABE` (64 MiB): laengere Eingaben werden am Anfang
//!   abgeschnitten, das heisst: die ERSTEN 64 MiB werden dekodiert, der
//!   Rest wird verworfen. Diese Grenze ist BENANNT und nicht still.
//! * `MAX_PRESCAN` (1024 Byte): weiter als bis hierhin schaut die
//!   Erkennung nicht. Ein `<meta charset>` hinter Byte 1024 greift nicht.
//!   Die Spezifikation ERLAUBT genau diese Grenze ("User agents are
//!   encouraged to only prescan the first 1024 bytes").
//! * UTF-16: es gibt keinen UTF-16-Dekoder in dieser Liste. Ein
//!   UTF-16LE/BE-BOM (und eine UTF-16-XML-Deklaration) wird deshalb als
//!   UTF-8 behandelt. Das ist eine BENANNTE GRENZE: die Bytes werden
//!   dadurch zu U+FFFD statt zu Text, aber es gibt keinen Panic und keine
//!   falsche Behauptung. Der Fall steht im Bericht als offener Punkt.
//!
//! # KEINE PANICS
//!
//! Jede Bytefolge ist gueltige Eingabe. Alle Tabellenzugriffe laufen ueber
//! `get` oder ueber die Binaersuche; ein fehlender Eintrag ist laut
//! Spezifikation ein Fehler und wird zu U+FFFD.
//!
//! # LAUFZEIT
//!
//! Jeder Dekoder geht EINMAL ueber die Eingabe (linear). Die Zweibyte- und
//! Vierbyte-Tabellen werden per Binaersuche durchsucht (logarithmisch),
//! nicht linear.

// GEMESSEN, warum `#[path]`: `lib.rs` bindet diese Datei als
// `pub mod kodierung;` ein, also ist `src/kodierung.rs` die Modulwurzel.
// Ein Untermodul wuerde Rust normalerweise in `src/kodierung/` suchen.
// Die Tabelle liegt aber laut Vorgabe in `src/kodierung_tabellen.rs` —
// `#[path]` sagt dem Compiler genau das, ohne die Datei zu verschieben.
#[path = "kodierung_tabellen.rs"]
mod kodierung_tabellen;
// Release 11.10.2026: die uebrigen Tabellen des WHATWG Encoding Standard
// (alle Etiketten, Einbyte-Indizes, big5, jis0212), erzeugt von
// `werkzeuge/kodierung_wpt_bauen.py` aus dem WPT-Klon.
#[path = "kodierung_tabellen_wpt.rs"]
mod kodierung_tabellen_wpt;
use kodierung_tabellen_wpt::{BIG5, EINBYTE, ETIKETTEN_WHATWG, JIS0212};

use kodierung_tabellen::{
    ETIKETTEN, EUC_KR, GB18030, GB18030_BEREICHE, ISO_8859_15, JIS0208, WINDOWS_1252,
};

/// Obergrenze der Eingabe fuer `dekodiere` (M3, benannte Grenze).
///
/// 64 MiB ist die groesste HTML-Datei, die noch sinnvoll ein Dokument ist;
/// alles darueber ist ein Download. GEMESSEN: eine 64-MiB-Eingabe als
/// `String` mit bis zu vier Byte je Zeichen belegt den Speicher zweimal,
/// und das ist mit dieser Grenze kalkulierbar.
pub const MAX_KODIERUNG_EINGABE: usize = 64 * 1024 * 1024;

/// Bis hierhin (und nicht weiter) sucht der Prescan nach `<meta charset>`
/// (WHATWG HTML, "determining the character encoding").
pub const MAX_PRESCAN: usize = 1024;

/// Das Ersatzzeichen U+FFFD (REPLACEMENT CHARACTER).
///
/// Die Spezifikation schreibt genau dieses Zeichen fuer jede ungueltige
/// Folge vor. Die Spezifikation verlangt ausserdem, dass eine ungueltige
/// Folge GENAU EIN U+FFFD ergibt (nicht zwei, nicht null) — deshalb sind
/// die Dekoder unten Zustandsmaschinen und keine `from_utf8_lossy`-Aufrufe.
const ERSATZ: char = '\u{FFFD}';

/// Ein unterstuetzter Zeichensatz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kodierung {
    Utf8,
    Utf16Le,
    Utf16Be,
    Windows1252,
    Iso885915,
    ShiftJis,
    EucKr,
    Gb18030,
    /// Release 11.10.2026: die uebrigen Kodierungen des WHATWG Encoding
    /// Standard. `gbk` dekodiert wie gb18030, traegt aber seinen Namen.
    Gbk,
    Big5,
    EucJp,
    Iso2022Jp,
    /// Eine Einbyte-Kodierung aus `EINBYTE` (Nummer in der Tabelle).
    Einbyte(u8),
    /// iso-8859-8-i: dieselbe Tabelle wie iso-8859-8, eigener Name.
    Iso88598I,
    XUserDefined,
    /// Die Ersatz-Kodierung (iso-2022-kr, hz-gb-2312 …): ein U+FFFD.
    Ersatz,
}

impl Kodierung {
    /// Der WHATWG-Name.
    pub fn name(&self) -> &'static str {
        match self {
            Kodierung::Utf8 => "utf-8",
            Kodierung::Utf16Le => "utf-16le",
            Kodierung::Utf16Be => "utf-16be",
            Kodierung::Windows1252 => "windows-1252",
            Kodierung::Iso885915 => "iso-8859-15",
            Kodierung::ShiftJis => "shift_jis",
            Kodierung::EucKr => "euc-kr",
            Kodierung::Gb18030 => "gb18030",
            Kodierung::Gbk => "gbk",
            Kodierung::Big5 => "big5",
            Kodierung::EucJp => "euc-jp",
            Kodierung::Iso2022Jp => "iso-2022-jp",
            Kodierung::Einbyte(i) => EINBYTE.get(*i as usize).map_or("windows-1252", |e| e.0),
            Kodierung::Iso88598I => "iso-8859-8-i",
            Kodierung::XUserDefined => "x-user-defined",
            Kodierung::Ersatz => "replacement",
        }
    }

    /// Release 11.10.2026: die Kodierung zu ihrem WHATWG-Namen (klein).
    pub fn aus_name(name: &str) -> Option<Kodierung> {
        Some(match name {
            "utf-8" => Kodierung::Utf8,
            "utf-16le" => Kodierung::Utf16Le,
            "utf-16be" => Kodierung::Utf16Be,
            "windows-1252" => Kodierung::Windows1252,
            "iso-8859-15" => Kodierung::Iso885915,
            "shift_jis" => Kodierung::ShiftJis,
            "euc-kr" => Kodierung::EucKr,
            "gb18030" => Kodierung::Gb18030,
            "gbk" => Kodierung::Gbk,
            "big5" => Kodierung::Big5,
            "euc-jp" => Kodierung::EucJp,
            "iso-2022-jp" => Kodierung::Iso2022Jp,
            "iso-8859-8-i" => Kodierung::Iso88598I,
            "x-user-defined" => Kodierung::XUserDefined,
            "replacement" => Kodierung::Ersatz,
            _ => Kodierung::Einbyte(EINBYTE.iter().position(|e| e.0 == name)? as u8),
        })
    }

    /// Ist es UTF-8 oder UTF-16 (dann wird nicht umkodiert)?
    pub fn ist_utf(&self) -> bool {
        matches!(self, Kodierung::Utf8 | Kodierung::Utf16Le | Kodierung::Utf16Be)
    }

    /// Ist es UTF-8 (dann wird nicht umkodiert)?
    pub fn ist_utf8(&self) -> bool {
        *self == Kodierung::Utf8
    }

    /// Nummer fuer die erzeugte Etikettentabelle.
    ///
    /// Die Zahlen stehen auch im Kopf von `kodierung_tabellen.rs`. Im
    /// Produktionsweg geht es nur in die andere Richtung (`aus_nummer`);
    /// diese Richtung braucht der Test, der beide Seiten zusammenhaelt.
    /// WICHTIG: Die Nummern 0-5 muessen den Original-Kodierungen entsprechen,
    /// da die ETIKETTEN-Tabelle diese Nummern verwendet. UTF-16 wird
    /// gesondert in `kodierung_aus_label` behandelt und geht nicht ueber
    /// die Tabelle.
    #[cfg(test)]
    fn nummer(&self) -> u8 {
        match self {
            Kodierung::Utf8 => 0,
            Kodierung::Windows1252 => 1,
            Kodierung::Iso885915 => 2,
            Kodierung::ShiftJis => 3,
            Kodierung::EucKr => 4,
            Kodierung::Gb18030 => 5,
            Kodierung::Utf16Le => 6,
            Kodierung::Utf16Be => 7,
            _ => 255,
        }
    }

    /// Umkehrung von `nummer`; `None` fuer unbekannte Zahlen.
    fn aus_nummer(n: u8) -> Option<Kodierung> {
        Some(match n {
            0 => Kodierung::Utf8,
            1 => Kodierung::Windows1252,
            2 => Kodierung::Iso885915,
            3 => Kodierung::ShiftJis,
            4 => Kodierung::EucKr,
            5 => Kodierung::Gb18030,
            6 => Kodierung::Utf16Le,
            7 => Kodierung::Utf16Be,
            _ => return None,
        })
    }
}

/// WHATWG-Label -> Kodierung (z. B. "latin1" -> windows-1252).
///
/// Die Etikettentabelle wird aus `encodings.json` erzeugt und deckt ALLE
/// WHATWG-Etiketten der sechs Kodierungen ab, nicht nur die haeufigsten:
/// "ansi_x3.4-1968", "iso_8859-1:1987", "ks_c_5601-1989" und so weiter
/// stehen alle in `kodierung_tabellen.rs`.
///
/// Release 11.10.2026: ALLE WHATWG-Etiketten (228, `ETIKETTEN_WHATWG`), auch
/// iso-8859-2 … , euc-jp, big5, iso-2022-jp, `replacement` und
/// `x-user-defined` (vorher `None` bzw. windows-1252). Unbekannte Etiketten
/// ergeben `None`; wer `None` bekommt, macht weiter wie ohne Etikett. Die
/// HTML-Regeln des Prescans (UTF-16 -> UTF-8, x-user-defined ->
/// windows-1252) stehen in `prescan_regel`.
pub fn kodierung_aus_label(label: &str) -> Option<Kodierung> {
    // WHATWG: nur ASCII-Leerzeichen an den Raendern entfernen und dann
    // ASCII-klein schreiben. `str::trim` wuerde auch U+00A0 entfernen und
    // damit ein Etikett erkennen, das die Spezifikation nicht erkennt.
    let normalisiert = label
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    if normalisiert.is_empty() {
        return None;
    }
    // UTF-16-Varianten direkt behandeln (nicht in der ETIKETTEN-Tabelle).
    if matches!(
        normalisiert.as_str(),
        "utf-16" | "utf-16le" | "utf-16be"
    ) {
        return Some(match normalisiert.as_str() {
            "utf-16be" => Kodierung::Utf16Be,
            _ => Kodierung::Utf16Le, // "utf-16" und "utf-16le" -> LE (WHATWG Default)
        });
    }
    // Release 11.10.2026: alle WHATWG-Etiketten (228). `x-user-defined`
    // ist hier die eigene Kodierung; die HTML-Regel „im Prescan wird daraus
    // windows-1252" steht im Prescan selbst (`prescan_regel`).
    if let Ok(i) = ETIKETTEN_WHATWG.binary_search_by(|&(e, _)| e.cmp(normalisiert.as_str())) {
        return Kodierung::aus_name(ETIKETTEN_WHATWG[i].1);
    }
    // Die alte Tabelle traegt zusaetzlich die IANA-Etiketten `latin0`/`latin9`.
    match ETIKETTEN.binary_search_by(|&(e, _)| e.cmp(normalisiert.as_str())) {
        Ok(i) => Kodierung::aus_nummer(ETIKETTEN[i].1),
        Err(_) => None,
    }
}

// ===========================================================================
// ERKENNUNG
// ===========================================================================

/// Erkennt den Zeichensatz: BOM, Transport-Hinweis, `<meta charset>` in
/// den ersten 1024 Byte, sonst windows-1252 (WHATWG-Algorithmus).
///
/// Die Reihenfolge ist keine Geschmacksfrage: der BOM schlaegt alles
/// (Spezifikation: "the byte order mark is more authoritative than
/// anything else"), der Transport schlaegt das Dokument, das Dokument
/// schlaegt den Fallback.
/// Release 11.10.2026: wie [`erkenne`], aber OHNE den Rueckfall auf
/// windows-1252 — `None`, wenn weder BOM noch Transport-Hinweis noch
/// `<meta>` eine Kodierung nennen. Der Browser liest solche Seiten wie
/// bisher (als UTF-8, die heute uebliche Annahme).
pub fn erkenne_angabe(rohdaten: &[u8], transport_hinweis: Option<&str>) -> Option<Kodierung> {
    if rohdaten.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(Kodierung::Utf8);
    }
    if rohdaten.starts_with(&[0xFF, 0xFE]) {
        return Some(Kodierung::Utf16Le);
    }
    if rohdaten.starts_with(&[0xFE, 0xFF]) {
        return Some(Kodierung::Utf16Be);
    }
    if let Some(k) = ist_utf16_xml_deklaration(rohdaten) {
        return Some(k);
    }
    if let Some(k) = transport_hinweis.and_then(kodierung_aus_transport) {
        return Some(k);
    }
    prescan(&rohdaten[..rohdaten.len().min(MAX_PRESCAN)])
}

pub fn erkenne(rohdaten: &[u8], transport_hinweis: Option<&str>) -> Kodierung {
    // 1. BOM zuerst.
    if rohdaten.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Kodierung::Utf8;
    }
    if rohdaten.starts_with(&[0xFF, 0xFE]) {
        // UTF-16LE BOM
        return Kodierung::Utf16Le;
    }
    if rohdaten.starts_with(&[0xFE, 0xFF]) {
        // UTF-16BE BOM
        return Kodierung::Utf16Be;
    }
    // BENANNTE GRENZE: dieselbe Entscheidung fuer die UTF-16-XML-Deklaration
    // ("<?x" little-endian bzw. big-endian), die der Prescan sonst als
    // erstes prueft.
    if let Some(k) = ist_utf16_xml_deklaration(rohdaten) {
        return k;
    }

    // 2. Transport-Hinweis (Content-Type oder ein reines Etikett).
    if let Some(hinweis) = transport_hinweis {
        if let Some(k) = kodierung_aus_transport(hinweis) {
            return k;
        }
    }

    // 3. Prescan der ersten 1024 Byte.
    let fenster = &rohdaten[..rohdaten.len().min(MAX_PRESCAN)];
    if let Some(k) = prescan(fenster) {
        return k;
    }

    // 4. Fallback. Die Spezifikation nennt windows-1252 ausdruecklich,
    // weil damit JEDE Bytefolge Text ergibt.
    Kodierung::Windows1252
}

/// UTF-16-XML-Deklaration am Anfang (`<?x` als UTF-16LE/BE)?
/// Release 11.10.2026: HTML §13.2.3.2 (Prescan): aus UTF-16 wird UTF-8, aus
/// `x-user-defined` windows-1252.
fn prescan_regel(k: Kodierung) -> Kodierung {
    match k {
        Kodierung::Utf16Le | Kodierung::Utf16Be => Kodierung::Utf8,
        Kodierung::XUserDefined => Kodierung::Windows1252,
        k => k,
    }
}

fn ist_utf16_xml_deklaration(daten: &[u8]) -> Option<Kodierung> {
    if daten.starts_with(&[0x3C, 0x00, 0x3F, 0x00, 0x78, 0x00]) {
        Some(Kodierung::Utf16Le)
    } else if daten.starts_with(&[0x00, 0x3C, 0x00, 0x3F, 0x00, 0x78]) {
        Some(Kodierung::Utf16Be)
    } else {
        None
    }
}

/// Der Transport-Hinweis: entweder `text/html; charset=shift_jis` oder ein
/// reines Etikett wie `shift_jis`.
///
/// Ein reines `text/html` (ohne charset) ergibt `None` — das ist wichtig,
/// sonst wuerde der Medientyp als Etikett missverstanden.
fn kodierung_aus_transport(hinweis: &str) -> Option<Kodierung> {
    if let Some(wert) = charset_wert(hinweis) {
        return kodierung_aus_label(wert);
    }
    // Kein charset-Parameter: der GANZE Hinweis darf ein Etikett sein.
    kodierung_aus_label(hinweis)
}

/// Liest den Wert des `charset`-Parameters aus einem Kopfzeilenwert.
///
/// Dieselbe Form wie im Prescan: `charset`, optional Leerraum, `=`,
/// optional Leerraum, dann entweder ein Anfuehrungszeichen bis zum
/// passenden Gegenstueck oder ein Wert bis `;`/Leerraum/Ende.
fn charset_wert(hinweis: &str) -> Option<&str> {
    let bytes = hinweis.as_bytes();
    let mut ab = 0usize;
    loop {
        let i = finde_ascii_ci(bytes, b"charset", ab)?;
        let mut p = i + b"charset".len();
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'=' {
            // Kein Parameter: hinter dem Fund weitersuchen.
            ab = p.max(i + 1);
            continue;
        }
        p += 1;
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= bytes.len() {
            return None;
        }
        let anfang = p;
        if bytes[p] == b'"' || bytes[p] == b'\'' {
            let anfuehrung = bytes[p];
            p += 1;
            let start = p;
            while p < bytes.len() && bytes[p] != anfuehrung {
                p += 1;
            }
            if p >= bytes.len() {
                return None; // kein schliessendes Anfuehrungszeichen
            }
            return Some(&hinweis[start..p]);
        }
        while p < bytes.len() && !bytes[p].is_ascii_whitespace() && bytes[p] != b';' {
            p += 1;
        }
        if p == anfang {
            return None;
        }
        return Some(&hinweis[anfang..p]);
    }
}

// ===========================================================================
// DER PRESCAN (WHATWG HTML, "prescan a byte stream")
// ===========================================================================

/// Sucht `<meta charset=...>` bzw. `<meta http-equiv=content-type ...>`
/// in den uebergebenen Bytes. `None` heisst: nichts gefunden (oder die
/// Bytefolge ist zu Ende, bevor das Dokument eine Aussage macht).
///
/// Der Algorithmus ist der des HTML-Standards, Schritt fuer Schritt:
/// Kommentare werden uebersprungen, ein `<meta` wird nur erkannt, wenn ein
/// Leerzeichen oder `/` folgt, und die Attribute werden mit demselben
/// "get an attribute" gelesen wie der echte Parser. Deshalb endet er auch
/// bei kaputtem Markup: `hole_attribut` kann an jedem Byte abbrechen.
fn prescan(daten: &[u8]) -> Option<Kodierung> {
    let mut pos = 0usize;
    while pos < daten.len() {
        if daten[pos] != 0x3C {
            pos += 1;
            continue;
        }
        // --- Kommentar <!-- ... --> ---
        if daten[pos..].starts_with(b"<!--") {
            // Kein Ende im Fenster: die Spezifikation bricht ab.
            let ende = finde_kommentarende(daten, pos)?;
            pos = ende + 1;
            continue;
        }
        // --- <meta ...> ---
        if ist_meta_start(daten, pos) {
            // Position zeigt auf das Trennzeichen (Leerzeichen oder /).
            let mut p = pos + 5;
            // Doppelte Attribute: der ERSTE gewinnt (Spezifikation). Fuer
            // die Auswertung zaehlt nur, ob eines der drei wichtigen
            // Attribute schon da war — die anderen haben keinen Effekt.
            let mut http_equiv_gesehen = false;
            let mut content_gesehen = false;
            let mut charset_gesehen = false;
            let mut got_pragma = false;
            let mut need_pragma: Option<bool> = None;
            let mut charset: Option<Kodierung> = None;
            let mut charset_fehler = false;

            loop {
                let (name, wert, neuer_p) = match hole_attribut(daten, p) {
                    // Kein Attribut mehr (Byte-Ende oder '>'): zur
                    // Auswertung. Das Byte-Ende ist kein Sonderfall fuer
                    // die Auswertung selbst — ein vorher vollstaendig
                    // gelesenes `charset` zaehlt trotzdem.
                    None => {
                        p = daten.len();
                        break;
                    }
                    Some(dreier) => dreier,
                };
                p = neuer_p;
                if name.is_empty() {
                    break; // Abbruch an '>'
                }
                if name.eq_ignore_ascii_case(b"http-equiv") {
                    if !http_equiv_gesehen {
                        http_equiv_gesehen = true;
                        if wert.eq_ignore_ascii_case(b"content-type") {
                            got_pragma = true;
                        }
                    }
                } else if name.eq_ignore_ascii_case(b"content") {
                    // Nur wenn noch kein charset vom `charset`-Attribut
                    // gesetzt wurde. Ein unbekanntes Etikett setzt NICHTS
                    // (es ist kein Fehler, es ist nur nicht verwertbar).
                    if !content_gesehen {
                        content_gesehen = true;
                        if charset.is_none() && !charset_fehler {
                            if let Some(k) = encoding_aus_content(wert) {
                                charset = Some(k);
                                need_pragma = Some(true);
                            }
                        }
                    }
                } else if name.eq_ignore_ascii_case(b"charset") && !charset_gesehen {
                    charset_gesehen = true;
                    match kodierung_aus_label(&verlustfreier_text(wert)) {
                        Some(k) => {
                            charset = Some(k);
                            charset_fehler = false;
                        }
                        None => {
                            // Laut Spezifikation ist ein unerkanntes
                            // Etikett ein FEHLER, kein "nichts gefunden":
                            // ein spaeteres `<meta charset>` derselben
                            // Seite kann ihn nicht mehr heilen.
                            charset = None;
                            charset_fehler = true;
                        }
                    }
                    need_pragma = Some(false);
                }
            }

            // Auswertung (Spezifikation, Schritt "processing").
            if need_pragma.is_some()
                && !(need_pragma == Some(true) && !got_pragma)
                && !charset_fehler
            {
                if let Some(k) = charset {
                    return Some(prescan_regel(k));
                }
            }
            // "Next byte": ein Byte weiter. Ist das Byte-Ende erreicht,
            // bricht der ganze Prescan ab.
            if p >= daten.len() {
                return None;
            }
            pos = p + 1;
            continue;
        }
        // --- irgendein anderer Tag: Attribute ueberspringen ---
        if let Some((tag_anfang, tag_ende_gefunden)) = anderer_tag(daten, pos) {
            if !tag_ende_gefunden {
                return None;
            }
            let mut p = tag_anfang;
            loop {
                match hole_attribut(daten, p) {
                    None => {
                        p = daten.len();
                        break;
                    }
                    Some((name, _wert, neuer_p)) => {
                        p = neuer_p;
                        if name.is_empty() {
                            break;
                        }
                    }
                }
            }
            if p >= daten.len() {
                return None;
            }
            pos = p + 1;
            continue;
        }
        // --- <! ...>, </ ...>, <? ...> bis zum '>' ueberspringen ---
        if daten[pos..].starts_with(b"<!")
            || daten[pos..].starts_with(b"</")
            || daten[pos..].starts_with(b"<?")
        {
            let i = daten[pos + 1..].iter().position(|&b| b == 0x3E)?;
            pos = pos + 1 + i + 1;
            continue;
        }
        // --- jedes andere Byte ---
        pos += 1;
    }
    None
}

/// `<` + `meta` (beliebige Gross-/Kleinschreibung) + Trennzeichen?
fn ist_meta_start(daten: &[u8], pos: usize) -> bool {
    let rest = match daten.get(pos..pos + 6) {
        Some(r) => r,
        None => return false,
    };
    if rest[0] != 0x3C {
        return false;
    }
    if !rest[1..5].eq_ignore_ascii_case(b"meta") {
        return false;
    }
    matches!(rest[5], 0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F)
}

/// Das erste `>` nach einem `<!--`, dem zwei `-` vorangehen.
///
/// Der Rueckgabewert ist der Index des `>`. `None` heisst: es gibt keines.
fn finde_kommentarende(daten: &[u8], pos: usize) -> Option<usize> {
    // Ab pos+1 suchen: das '>' muss hinter dem '<' liegen. Die zwei '-'
    // duerfen aus dem "<!--" selbst stammen — deshalb reicht i >= 2.
    let mut i = pos + 1;
    while i < daten.len() {
        if daten[i] == 0x3E && i >= 2 && daten[i - 1] == 0x2D && daten[i - 2] == 0x2D {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `<`, optional `/`, dann ein ASCII-Buchstabe?
///
/// Rueckgabe: (Position des ersten Trennzeichens nach dem Tag-Namen,
/// ob ueberhaupt eines gefunden wurde). Ohne Trennzeichen bricht der
/// Prescan ab (Byte-Ende).
fn anderer_tag(daten: &[u8], pos: usize) -> Option<(usize, bool)> {
    let mut i = pos + 1;
    if daten.get(i) == Some(&0x2F) {
        i += 1;
    }
    match daten.get(i) {
        Some(b) if b.is_ascii_alphabetic() => {}
        _ => return None,
    }
    let mut j = i;
    while j < daten.len() {
        if daten[j].is_ascii_whitespace() || daten[j] == 0x3E {
            return Some((j, true));
        }
        j += 1;
    }
    Some((daten.len(), false))
}

/// Ein Attribut lesen (WHATWG HTML, "get an attribute").
///
/// Rueckgabe: `(name, wert, neue_position)`; `None`, wenn kein Attribut
/// gelesen werden konnte, weil die Bytes zu Ende sind. Ein leeres Ergebnis
/// `("", "", pos)` bedeutet: an dieser Stelle steht `>` und es gibt kein
/// Attribut mehr.
///
/// Die Spezifikation schreibt die Fortsetzung "Restore byte to ioQueue"
/// an zwei Stellen vor (ein ASCII-Byte nach einem kaputten Wert wird neu
/// gelesen). Hier wird einfach die Position zurueckgegeben: der Aufrufer
/// liest das Byte dann erneut.
fn hole_attribut(daten: &[u8], pos: usize) -> Option<(&[u8], &[u8], usize)> {
    let mut p = pos;
    // Leerraum und '/' ueberspringen.
    while p < daten.len() && (daten[p].is_ascii_whitespace() || daten[p] == 0x2F) {
        p += 1;
    }
    if p >= daten.len() {
        return None;
    }
    if daten[p] == 0x3E {
        return Some((b"", b"", p));
    }

    // --- Name ---
    //
    // Die Schleife merkt sich `name_ende` selbst: die Spezifikation baut
    // den Namen Byte fuer Byte auf, und ein '=' zaehlt nur dann als
    // Trenner, wenn der Name schon laenger als leer ist.
    let name_anfang = p;
    let mut name_ende = p;
    loop {
        if p >= daten.len() {
            return None; // Byte-Ende mitten im Namen
        }
        let b = daten[p];
        if b == 0x3D && p > name_anfang {
            p += 1; // '=' gefunden: weiter zum Wert
            break;
        }
        if b.is_ascii_whitespace() {
            // "Spaces": Leerraum ueberspringen; danach muss '=' kommen,
            // sonst ist es ein Namensattribut ohne Wert.
            name_ende = p;
            while p < daten.len() && daten[p].is_ascii_whitespace() {
                p += 1;
            }
            if p >= daten.len() {
                return None;
            }
            if daten[p] != 0x3D {
                return Some((&daten[name_anfang..name_ende], b"", p));
            }
            p += 1;
            break;
        }
        if b == 0x2F || b == 0x3E {
            name_ende = p;
            return Some((&daten[name_anfang..name_ende], b"", p));
        }
        p += 1;
        name_ende = p;
    }
    if p >= daten.len() {
        return None; // Byte-Ende direkt nach dem '='
    }
    let name = &daten[name_anfang..name_ende];

    // --- Wert ---
    while p < daten.len() && daten[p].is_ascii_whitespace() {
        p += 1;
    }
    if p >= daten.len() {
        return None;
    }
    let wert_anfang = p;
    let quote = daten[p];
    if quote == 0x22 || quote == 0x27 {
        p += 1;
        let anfang = p;
        while p < daten.len() && daten[p] != quote {
            p += 1;
        }
        if p >= daten.len() {
            return None; // kein schliessendes Anfuehrungszeichen
        }
        let wert = &daten[anfang..p];
        p += 1;
        return Some((name, wert, p));
    }
    while p < daten.len() {
        let b = daten[p];
        if b.is_ascii_whitespace() || b == 0x3E {
            return Some((name, &daten[wert_anfang..p], p));
        }
        p += 1;
    }
    None // Byte-Ende mitten im Wert
}

/// Der Wert eines `content`-Attributs, in dem `charset=...` stehen kann.
///
/// Das ist der HTML-Algorithmus "extracting a character encoding from a
/// meta element": `charset` suchen, `=` verlangen, Leerraum ueberspringen,
/// Anfuehrungszeichen beachten. Ein `charset`, das nicht von `=` gefolgt
/// ist, wird uebersprungen — weiter suchen ist Pflicht, sonst scheitert
/// `<meta content="text/html; xcharset; charset=utf-8">`.
fn encoding_aus_content(wert: &[u8]) -> Option<Kodierung> {
    let mut ab = 0usize;
    loop {
        let i = finde_ascii_ci(wert, b"charset", ab)?;
        let mut p = i + 7;
        while p < wert.len() && wert[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= wert.len() {
            return None;
        }
        if wert[p] != b'=' {
            ab = p;
            continue;
        }
        p += 1;
        while p < wert.len() && wert[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= wert.len() {
            return None;
        }
        let c = wert[p];
        if c == b'"' || c == b'\'' {
            // Nur wenn ein SPAETERES gleiches Zeichen existiert.
            let rel = wert[p + 1..].iter().position(|&b| b == c)?;
            let anfang = p + 1;
            let ende = anfang + rel;
            return kodierung_aus_label(&verlustfreier_text(&wert[anfang..ende]));
        }
        // Bis Leerraum, ';' oder Ende.
        let mut q = p;
        while q < wert.len() && !wert[q].is_ascii_whitespace() && wert[q] != b';' {
            q += 1;
        }
        return kodierung_aus_label(&verlustfreier_text(&wert[p..q]));
    }
}

/// Wandelt einen Attributwert in Text, ohne je zu panicken.
///
/// Die Etiketten sind ASCII. `String::from_utf8_lossy` waere hier auch
/// richtig, aber es soll sichtbar sein, dass ein Nicht-ASCII-Byte ein
/// unbekanntes Etikett ergibt und kein Panic.
fn verlustfreier_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Findet `nadel` in `heuhaufen` ab `ab`, ASCII-case-insensitiv.
fn finde_ascii_ci(heuhaufen: &[u8], nadel: &[u8], ab: usize) -> Option<usize> {
    if nadel.is_empty() || heuhaufen.len() < nadel.len() {
        return None;
    }
    let mut i = ab;
    while i + nadel.len() <= heuhaufen.len() {
        if heuhaufen[i..i + nadel.len()].eq_ignore_ascii_case(nadel) {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ===========================================================================
// DEKODIERUNG
// ===========================================================================

/// UTF-16LE dekodieren (mit BOM-Erkennung).
///
/// Wenn ein BOM (FF FE) vorhanden ist, wird es uebersprungen.
/// Ungueltige Surrogate-Paare ergeben U+FFFD.
fn utf16le_dekodiere(daten: &[u8]) -> String {
    // BOM ueberspringen falls vorhanden
    let daten = if daten.len() >= 2 && daten[0] == 0xFF && daten[1] == 0xFE {
        &daten[2..]
    } else {
        daten
    };

    // Bytes in u16-Werte umwandeln (Little Endian)
    let mut units = Vec::with_capacity(daten.len() / 2);
    let mut i = 0;
    while i + 1 < daten.len() {
        let lo = daten[i];
        let hi = daten[i + 1];
        units.push(u16::from_le_bytes([lo, hi]));
        i += 2;
    }
    // Ungerade Anzahl Bytes: letztes Byte ignorieren (wie bei from_utf16_lossy)
    // Aber wir geben ein U+FFFD dafuer aus.
    if i < daten.len() {
        units.push(0xFFFD);
    }

    // Surrogate-Paare validieren und zu String konvertieren
    let mut raus = String::with_capacity(units.len());
    let mut idx = 0;
    while idx < units.len() {
        let u = units[idx];
        // High Surrogate (Lead)
        if (0xD800..=0xDBFF).contains(&u) {
            if idx + 1 < units.len() {
                let next = units[idx + 1];
                // Low Surrogate (Trail)
                if (0xDC00..=0xDFFF).contains(&next) {
                    // Gueltiges Paar
                    let codepoint = ((u as u32 - 0xD800) << 10) + (next as u32 - 0xDC00) + 0x10000;
                    if let Some(c) = char::from_u32(codepoint) {
                        raus.push(c);
                    } else {
                        raus.push(ERSATZ);
                    }
                    idx += 2;
                    continue;
                }
            }
            // Ungueltiges High Surrogate (kein folgendes Low Surrogate)
            raus.push(ERSATZ);
            idx += 1;
        }
        // Low Surrogate ohne vorangehendes High Surrogate
        else if (0xDC00..=0xDFFF).contains(&u) {
            raus.push(ERSATZ);
            idx += 1;
        }
        // Normaler BMP-Codepoint
        else {
            if let Some(c) = char::from_u32(u as u32) {
                raus.push(c);
            } else {
                raus.push(ERSATZ);
            }
            idx += 1;
        }
    }
    raus
}

/// UTF-16BE dekodieren (mit BOM-Erkennung).
///
/// Wenn ein BOM (FE FF) vorhanden ist, wird es uebersprungen.
/// Ungueltige Surrogate-Paare ergeben U+FFFD.
fn utf16be_dekodiere(daten: &[u8]) -> String {
    // BOM ueberspringen falls vorhanden
    let daten = if daten.len() >= 2 && daten[0] == 0xFE && daten[1] == 0xFF {
        &daten[2..]
    } else {
        daten
    };

    // Bytes in u16-Werte umwandeln (Big Endian)
    let mut units = Vec::with_capacity(daten.len() / 2);
    let mut i = 0;
    while i + 1 < daten.len() {
        let hi = daten[i];
        let lo = daten[i + 1];
        units.push(u16::from_be_bytes([hi, lo]));
        i += 2;
    }
    // Ungerade Anzahl Bytes: letztes Byte ignorieren
    if i < daten.len() {
        units.push(0xFFFD);
    }

    // Surrogate-Paare validieren und zu String konvertieren
    let mut raus = String::with_capacity(units.len());
    let mut idx = 0;
    while idx < units.len() {
        let u = units[idx];
        // High Surrogate (Lead)
        if (0xD800..=0xDBFF).contains(&u) {
            if idx + 1 < units.len() {
                let next = units[idx + 1];
                // Low Surrogate (Trail)
                if (0xDC00..=0xDFFF).contains(&next) {
                    // Gueltiges Paar
                    let codepoint = ((u as u32 - 0xD800) << 10) + (next as u32 - 0xDC00) + 0x10000;
                    if let Some(c) = char::from_u32(codepoint) {
                        raus.push(c);
                    } else {
                        raus.push(ERSATZ);
                    }
                    idx += 2;
                    continue;
                }
            }
            // Ungueltiges High Surrogate
            raus.push(ERSATZ);
            idx += 1;
        }
        // Low Surrogate ohne vorangehendes High Surrogate
        else if (0xDC00..=0xDFFF).contains(&u) {
            raus.push(ERSATZ);
            idx += 1;
        }
        // Normaler BMP-Codepoint
        else {
            if let Some(c) = char::from_u32(u as u32) {
                raus.push(c);
            } else {
                raus.push(ERSATZ);
            }
            idx += 1;
        }
    }
    raus
}

/// Dekodiert Rohbytes in Text; ungueltige Folgen werden U+FFFD.
/// Panickt nie, lineare Laufzeit.
///
/// Eingaben ueber `MAX_KODIERUNG_EINGABE` werden am Anfang abgeschnitten:
/// dekodiert werden die ersten 64 MiB, der Rest nicht. Ein Mehrbyte-
/// Zeichen, das genau an der Grenze zerschnitten wird, ergibt U+FFFD —
/// das ist dieselbe Regel wie am Eingabe-Ende.
pub fn dekodiere(rohdaten: &[u8], kodierung: Kodierung) -> String {
    let daten = if rohdaten.len() > MAX_KODIERUNG_EINGABE {
        &rohdaten[..MAX_KODIERUNG_EINGABE]
    } else {
        rohdaten
    };
    match kodierung {
        Kodierung::Utf8 => dekodiere_utf8(daten),
        Kodierung::Utf16Le => utf16le_dekodiere(daten),
        Kodierung::Utf16Be => utf16be_dekodiere(daten),
        Kodierung::Windows1252 => dekodiere_einbyte(daten, &WINDOWS_1252),
        Kodierung::Iso885915 => dekodiere_einbyte(daten, &ISO_8859_15),
        Kodierung::ShiftJis => dekodiere_shift_jis(daten),
        Kodierung::EucKr => dekodiere_euc_kr(daten),
        Kodierung::Gb18030 | Kodierung::Gbk => dekodiere_gb18030(daten),
        Kodierung::Big5 => dekodiere_big5(daten),
        Kodierung::EucJp => dekodiere_euc_jp(daten),
        Kodierung::Iso2022Jp => dekodiere_iso2022jp(daten),
        Kodierung::Einbyte(i) => match EINBYTE.get(i as usize) {
            Some((_, t)) => dekodiere_einbyte16(daten, t),
            None => dekodiere_einbyte(daten, &WINDOWS_1252),
        },
        Kodierung::Iso88598I => match EINBYTE.iter().find(|e| e.0 == "iso-8859-8") {
            Some((_, t)) => dekodiere_einbyte16(daten, t),
            None => dekodiere_einbyte(daten, &WINDOWS_1252),
        },
        Kodierung::XUserDefined => daten
            .iter()
            .map(|&b| if b < 0x80 { b as char } else { char::from_u32(0xF780 + b as u32 - 0x80).unwrap_or(ERSATZ) })
            .collect(),
        Kodierung::Ersatz => {
            if daten.is_empty() {
                String::new()
            } else {
                ERSATZ.to_string()
            }
        }
    }
}

/// Release 11.10.2026: Einbyte-Tabelle mit 16-Bit-Codepunkten (0 = keiner).
fn dekodiere_einbyte16(daten: &[u8], tabelle: &[u16; 128]) -> String {
    let mut raus = String::with_capacity(daten.len());
    for &b in daten {
        if b < 0x80 {
            raus.push(b as char);
        } else {
            let cp = tabelle[(b - 0x80) as usize];
            raus.push(if cp == 0 { ERSATZ } else { char::from_u32(cp as u32).unwrap_or(ERSATZ) });
        }
    }
    raus
}

/// Release 11.10.2026: der Big5-Dekoder (WHATWG Encoding §12.1.1).
fn dekodiere_big5(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut fuehrer = 0u8;
    let mut i = 0usize;
    while i < daten.len() {
        let b = daten[i];
        if fuehrer != 0 {
            let l = fuehrer as u32;
            fuehrer = 0;
            let versatz = if b < 0x7F { 0x40u32 } else { 0x62u32 };
            if (0x40..=0x7E).contains(&b) || (0xA1..=0xFE).contains(&b) {
                let pointer = (l - 0x81) * 157 + (b as u32 - versatz);
                let zwei = match pointer {
                    1133 => Some("\u{00CA}\u{0304}"),
                    1135 => Some("\u{00CA}\u{030C}"),
                    1164 => Some("\u{00EA}\u{0304}"),
                    1166 => Some("\u{00EA}\u{030C}"),
                    _ => None,
                };
                if let Some(z) = zwei {
                    raus.push_str(z);
                    i += 1;
                    continue;
                }
                if let Some(c) = index_codepunkt(&BIG5, pointer) {
                    raus.push(c);
                    i += 1;
                    continue;
                }
            }
            raus.push(ERSATZ);
            if b <= 0x7F {
                continue; // ASCII wird neu gelesen
            }
            i += 1;
            continue;
        }
        if b <= 0x7F {
            raus.push(b as char);
        } else if (0x81..=0xFE).contains(&b) {
            fuehrer = b;
        } else {
            raus.push(ERSATZ);
        }
        i += 1;
    }
    if fuehrer != 0 {
        raus.push(ERSATZ);
    }
    raus
}

/// Release 11.10.2026: der EUC-JP-Dekoder (WHATWG Encoding §13.1.1).
fn dekodiere_euc_jp(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut fuehrer = 0u8;
    let mut jis0212 = false;
    let mut i = 0usize;
    while i < daten.len() {
        let b = daten[i];
        if fuehrer == 0x8E && (0xA1..=0xDF).contains(&b) {
            fuehrer = 0;
            raus.push(char::from_u32(0xFF61 - 0xA1 + b as u32).unwrap_or(ERSATZ));
            i += 1;
            continue;
        }
        if fuehrer == 0x8F && (0xA1..=0xFE).contains(&b) {
            jis0212 = true;
            fuehrer = b;
            i += 1;
            continue;
        }
        if fuehrer != 0 {
            let l = fuehrer as u32;
            fuehrer = 0;
            let mut c = None;
            if (0xA1..=0xFE).contains(&l) && (0xA1..=0xFE).contains(&b) {
                let pointer = (l - 0xA1) * 94 + b as u32 - 0xA1;
                c = index_codepunkt(if jis0212 { &JIS0212[..] } else { &JIS0208[..] }, pointer);
            }
            jis0212 = false;
            if let Some(c) = c {
                raus.push(c);
                i += 1;
                continue;
            }
            raus.push(ERSATZ);
            if b <= 0x7F {
                continue;
            }
            i += 1;
            continue;
        }
        if b <= 0x7F {
            raus.push(b as char);
        } else if b == 0x8E || b == 0x8F || (0xA1..=0xFE).contains(&b) {
            fuehrer = b;
        } else {
            raus.push(ERSATZ);
        }
        i += 1;
    }
    if fuehrer != 0 {
        raus.push(ERSATZ);
    }
    raus
}

/// Release 11.10.2026: der ISO-2022-JP-Dekoder (WHATWG Encoding §13.2.1) —
/// eine Zustandsmaschine; „voranstellen" heisst: das Byte wird neu gelesen.
fn dekodiere_iso2022jp(daten: &[u8]) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Z {
        Ascii,
        Roman,
        Katakana,
        Fuehrer,
        Folge,
        EscStart,
        Esc,
    }
    let mut raus = String::with_capacity(daten.len());
    let (mut z, mut ausgabe_z) = (Z::Ascii, Z::Ascii);
    let mut fuehrer = 0u8;
    let mut ausgabe = false;
    // Voranzustellende Bytes (hoechstens zwei) vor dem naechsten Eingabebyte.
    let mut vorn: Vec<u8> = Vec::new();
    let mut i = 0usize;
    loop {
        let b = if let Some(v) = vorn.pop() {
            Some(v)
        } else if i < daten.len() {
            i += 1;
            Some(daten[i - 1])
        } else {
            None
        };
        match z {
            Z::Ascii | Z::Roman => match b {
                Some(0x1B) => z = Z::EscStart,
                None => break,
                Some(0x5C) if z == Z::Roman => {
                    ausgabe = false;
                    raus.push('\u{00A5}');
                }
                Some(0x7E) if z == Z::Roman => {
                    ausgabe = false;
                    raus.push('\u{203E}');
                }
                Some(b) if b <= 0x7F && b != 0x0E && b != 0x0F => {
                    ausgabe = false;
                    raus.push(b as char);
                }
                Some(_) => {
                    ausgabe = false;
                    raus.push(ERSATZ);
                }
            },
            Z::Katakana => match b {
                Some(0x1B) => z = Z::EscStart,
                None => break,
                Some(b) if (0x21..=0x5F).contains(&b) => {
                    ausgabe = false;
                    raus.push(char::from_u32(0xFF61 - 0x21 + b as u32).unwrap_or(ERSATZ));
                }
                Some(_) => {
                    ausgabe = false;
                    raus.push(ERSATZ);
                }
            },
            Z::Fuehrer => match b {
                Some(0x1B) => z = Z::EscStart,
                None => break,
                Some(b) if (0x21..=0x7E).contains(&b) => {
                    ausgabe = false;
                    fuehrer = b;
                    z = Z::Folge;
                }
                Some(_) => {
                    ausgabe = false;
                    raus.push(ERSATZ);
                }
            },
            Z::Folge => match b {
                Some(0x1B) => {
                    z = Z::EscStart;
                    raus.push(ERSATZ);
                }
                Some(b) if (0x21..=0x7E).contains(&b) => {
                    z = Z::Fuehrer;
                    let pointer = (fuehrer as u32 - 0x21) * 94 + b as u32 - 0x21;
                    raus.push(index_codepunkt(&JIS0208, pointer).unwrap_or(ERSATZ));
                }
                None => {
                    z = Z::Fuehrer;
                    raus.push(ERSATZ);
                }
                Some(_) => {
                    z = Z::Fuehrer;
                    raus.push(ERSATZ);
                }
            },
            Z::EscStart => match b {
                Some(b @ (0x24 | 0x28)) => {
                    fuehrer = b;
                    z = Z::Esc;
                }
                b => {
                    if let Some(b) = b {
                        vorn.push(b);
                    }
                    ausgabe = false;
                    z = ausgabe_z;
                    raus.push(ERSATZ);
                    if b.is_none() {
                        // Ende: im Ausgabezustand weiter (dort endet es).
                        continue;
                    }
                }
            },
            Z::Esc => {
                let l = fuehrer;
                fuehrer = 0;
                let neu = match (l, b) {
                    (0x28, Some(0x42)) => Some(Z::Ascii),
                    (0x28, Some(0x4A)) => Some(Z::Roman),
                    (0x28, Some(0x49)) => Some(Z::Katakana),
                    (0x24, Some(0x40 | 0x42)) => Some(Z::Fuehrer),
                    _ => None,
                };
                if let Some(n) = neu {
                    z = n;
                    ausgabe_z = n;
                    let war = ausgabe;
                    ausgabe = true;
                    if war {
                        raus.push(ERSATZ);
                    }
                    continue;
                }
                // Fuehrer und Byte voranstellen (in Lesereihenfolge).
                if let Some(b) = b {
                    vorn.push(b);
                }
                vorn.push(l);
                ausgabe = false;
                z = ausgabe_z;
                raus.push(ERSATZ);
            }
        }
    }
    raus
}

/// Der UTF-8-Dekoder aus dem WHATWG Encoding Standard.
///
/// WARUM NICHT `String::from_utf8_lossy`: Rusts Verlustdekoder folgt
/// derselben Empfehlung ("maximal subparts"), aber die Spezifikation ist
/// hier die Wahrheit, gegen die gemessen wird, und die Zustandsmaschine
/// macht die Regel sichtbar: ein ungueltiges Fortsetzungsbyte wird NICHT
/// verbraucht, sondern von vorn gelesen — genau ein U+FFFD je maximalem
/// Teil, das noch UTF-8 haette sein koennen.
fn dekodiere_utf8(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut i = 0usize;
    let mut bytes_noetig = 0u32;
    let mut gesehen = 0u32;
    let mut codepunkt = 0u32;
    let mut unten = 0x80u8;
    let mut oben = 0xBFu8;

    while i < daten.len() {
        let b = daten[i];
        if bytes_noetig == 0 {
            if b <= 0x7F {
                raus.push(b as char);
                i += 1;
            } else if (0xC2..=0xDF).contains(&b) {
                codepunkt = (b & 0x1F) as u32;
                bytes_noetig = 1;
                i += 1;
            } else if (0xE0..=0xEF).contains(&b) {
                if b == 0xE0 {
                    unten = 0xA0;
                }
                if b == 0xED {
                    oben = 0x9F;
                }
                codepunkt = (b & 0x0F) as u32;
                bytes_noetig = 2;
                i += 1;
            } else if (0xF0..=0xF4).contains(&b) {
                if b == 0xF0 {
                    unten = 0x90;
                }
                if b == 0xF4 {
                    oben = 0x8F;
                }
                codepunkt = (b & 0x07) as u32;
                bytes_noetig = 3;
                i += 1;
            } else {
                // 0x80..0xC1 und 0xF5..0xFF koennen keinen Anfang bilden.
                raus.push(ERSATZ);
                i += 1;
            }
            continue;
        }
        // Wir sind mitten in einer Folge.
        if b < unten || b > oben {
            // Ungueltig: ZURUECKSETZEN und das Byte NICHT verbrauchen.
            bytes_noetig = 0;
            gesehen = 0;
            codepunkt = 0;
            unten = 0x80;
            oben = 0xBF;
            raus.push(ERSATZ);
            continue;
        }
        unten = 0x80;
        oben = 0xBF;
        codepunkt = (codepunkt << 6) | (b & 0x3F) as u32;
        gesehen += 1;
        i += 1;
        if gesehen == bytes_noetig {
            // Die Grenzen oben schliessen Surrogate, Overlongs und
            // Codepunkte ueber U+10FFFF aus; `from_u32` kann hier nur
            // dann `None` liefern, wenn die Tabelle falsch waere.
            raus.push(char::from_u32(codepunkt).unwrap_or(ERSATZ));
            bytes_noetig = 0;
            gesehen = 0;
            codepunkt = 0;
        }
    }
    if bytes_noetig != 0 {
        // Abgebrochene Folge am Ende: genau ein U+FFFD.
        raus.push(ERSATZ);
    }
    raus
}

/// Ein-Byte-Dekoder (windows-1252, iso-8859-15).
///
/// Bytes 0x00..0x7F sind ASCII; 0x80..0xFF kommen aus der Tabelle. Die
/// Parameter sind die 128 Eintraege fuer 0x80..0xFF.
fn dekodiere_einbyte(daten: &[u8], tabelle: &[u32; 128]) -> String {
    let mut raus = String::with_capacity(daten.len());
    for &b in daten {
        if b < 0x80 {
            raus.push(b as char);
        } else {
            raus.push(char::from_u32(tabelle[(b - 0x80) as usize]).unwrap_or(ERSATZ));
        }
    }
    raus
}

/// Shift_JIS (WHATWG): ASCII, Halbbreiten-Katakana 0xA1..0xDF,
/// Zweibyte 0x81..0x9F/0xE0..0xFC mit dem JIS0208-Index.
///
/// Die Sonderregel: Pointer 8836..=10715 sind EUDC (Windows-Zeichen
/// U+E000..), und ein Fuehrungsbyte 0x80/0xA0/0xFD..0xFF kann keinen
/// Zweibyte-Anfang bilden. Ein ungueltiges Paar ergibt U+FFFD; ist das
/// zweite Byte ASCII, wird es erneut gelesen (Spezifikation).
fn dekodiere_shift_jis(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut i = 0usize;
    let mut fuehrer = 0u8;
    while i < daten.len() {
        let b = daten[i];
        if fuehrer != 0 {
            if (0x40..=0x7E).contains(&b) || (0x80..=0xFC).contains(&b) {
                let offset = if b < 0x7F { 0x40u32 } else { 0x41u32 };
                let fuehrer_offset = if fuehrer < 0xA0 { 0x81u32 } else { 0xC1u32 };
                let pointer = (fuehrer as u32 - fuehrer_offset) * 188 + (b as u32 - offset);
                if (8836..=10715).contains(&pointer) {
                    // EUDC: 0xE000 - 8836 + pointer.
                    raus.push(char::from_u32(0xE000 - 8836 + pointer).unwrap_or(ERSATZ));
                    fuehrer = 0;
                    i += 1;
                    continue;
                }
                if let Some(c) = index_codepunkt(&JIS0208, pointer) {
                    raus.push(c);
                    fuehrer = 0;
                    i += 1;
                    continue;
                }
            }
            // Ungueltiges zweites Byte.
            raus.push(ERSATZ);
            fuehrer = 0;
            if b <= 0x7F {
                // ASCII wird erneut gelesen (Spezifikation).
                continue;
            }
            i += 1;
            continue;
        }
        if b <= 0x7F || b == 0x80 {
            raus.push(b as char);
        } else if (0xA1..=0xDF).contains(&b) {
            raus.push(char::from_u32(0xFF61 - 0xA1 + b as u32).unwrap_or(ERSATZ));
        } else if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            fuehrer = b;
        } else {
            // 0xA0 und 0xFD..0xFF.
            raus.push(ERSATZ);
        }
        i += 1;
    }
    if fuehrer != 0 {
        raus.push(ERSATZ);
    }
    raus
}

/// EUC-KR (WHATWG): ASCII, Zweibyte 0x81..0xFE mit dem EUC-KR-Index.
fn dekodiere_euc_kr(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut i = 0usize;
    let mut fuehrer = 0u8;
    while i < daten.len() {
        let b = daten[i];
        if fuehrer != 0 {
            if (0x41..=0xFE).contains(&b) {
                let pointer = (fuehrer as u32 - 0x81) * 190 + (b as u32 - 0x41);
                if let Some(c) = index_codepunkt(&EUC_KR, pointer) {
                    raus.push(c);
                    fuehrer = 0;
                    i += 1;
                    continue;
                }
            }
            raus.push(ERSATZ);
            fuehrer = 0;
            if b <= 0x7F {
                continue; // ASCII erneut lesen
            }
            i += 1;
            continue;
        }
        if b <= 0x7F {
            raus.push(b as char);
        } else if (0x81..=0xFE).contains(&b) {
            fuehrer = b;
        } else {
            // 0x80 und 0xFF.
            raus.push(ERSATZ);
        }
        i += 1;
    }
    if fuehrer != 0 {
        raus.push(ERSATZ);
    }
    raus
}

/// gb18030 (WHATWG): ASCII, 0x80 = Euro, Zweibyte und Vierbyte.
///
/// Die Vierbyte-Pointer werden ueber `GB18030_BEREICHE` abgebildet; die
/// Spezifikation schliesst Pointer 39420..188999 (Surrogate) und alles
/// ueber 1237575 aus, und Pointer 7457 ist der Sonderfall U+E7C7.
fn dekodiere_gb18030(daten: &[u8]) -> String {
    let mut raus = String::with_capacity(daten.len());
    let mut i = 0usize;
    let mut erster = 0u8;
    let mut zweiter = 0u8;
    let mut dritter = 0u8;
    while i < daten.len() {
        let b = daten[i];
        if dritter != 0 {
            if !(0x30..=0x39).contains(&b) {
                // Zweiter, dritter und dieses Byte erneut lesen: die
                // Position steht auf dem vierten Byte, `zweiter` und
                // `dritter` liegen zwei Byte davor.
                i -= 2;
                erster = 0;
                zweiter = 0;
                dritter = 0;
                raus.push(ERSATZ);
                continue;
            }
            let pointer = (erster as u32 - 0x81) * (10 * 126 * 10)
                + (zweiter as u32 - 0x30) * (10 * 126)
                + (dritter as u32 - 0x81) * 10
                + (b as u32 - 0x30);
            erster = 0;
            zweiter = 0;
            dritter = 0;
            match gb18030_bereich(pointer) {
                Some(c) => raus.push(c),
                None => raus.push(ERSATZ),
            }
            i += 1;
            continue;
        }
        if zweiter != 0 {
            if (0x81..=0xFE).contains(&b) {
                dritter = b;
                i += 1;
                continue;
            }
            // Zweiter und dieses Byte erneut lesen: die Position steht
            // auf dem dritten Byte, `zweiter` liegt eines davor.
            i -= 1;
            erster = 0;
            zweiter = 0;
            raus.push(ERSATZ);
            continue;
        }
        if erster != 0 {
            if (0x30..=0x39).contains(&b) {
                zweiter = b;
                i += 1;
                continue;
            }
            let fuehrer = erster;
            erster = 0;
            let offset = if b < 0x7F { 0x40u32 } else { 0x41u32 };
            if (0x40..=0x7E).contains(&b) || (0x80..=0xFE).contains(&b) {
                let pointer = (fuehrer as u32 - 0x81) * 190 + (b as u32 - offset);
                if let Some(c) = index_codepunkt(&GB18030, pointer) {
                    raus.push(c);
                    i += 1;
                    continue;
                }
            }
            raus.push(ERSATZ);
            if b <= 0x7F {
                continue; // ASCII erneut lesen
            }
            i += 1;
            continue;
        }
        if b <= 0x7F {
            raus.push(b as char);
        } else if b == 0x80 {
            raus.push('\u{20AC}'); // Euro
        } else if (0x81..=0xFE).contains(&b) {
            erster = b;
        } else {
            raus.push(ERSATZ); // 0xFF
        }
        i += 1;
    }
    if erster != 0 || zweiter != 0 || dritter != 0 {
        raus.push(ERSATZ);
    }
    raus
}

/// Binaersuche im Zweibyte-Index: Pointer -> Codepunkt.
///
/// Ein fehlender Pointer ist laut Spezifikation "invalid" und ergibt
/// `None` (der Aufrufer schreibt U+FFFD).
fn index_codepunkt(tabelle: &[(u32, u32)], pointer: u32) -> Option<char> {
    match tabelle.binary_search_by_key(&pointer, |&(p, _)| p) {
        Ok(i) => char::from_u32(tabelle[i].1),
        Err(_) => None,
    }
}

/// Der gb18030-Vierbyte-Pointer -> Codepunkt (index gb18030 ranges).
///
/// Die drei Sonderfaelle stehen ausdruecklich in der Spezifikation:
/// Pointer 39420..188999 und ueber 1237575 sind ungueltig (Surrogate
/// bzw. ausserhalb Unicode), Pointer 7457 ist U+E7C7.
fn gb18030_bereich(pointer: u32) -> Option<char> {
    if (pointer > 39419 && pointer < 189000) || pointer > 1237575 {
        return None;
    }
    if pointer == 7457 {
        return Some('\u{E7C7}');
    }
    // Letzter Eintrag mit Start <= pointer.
    let index = match GB18030_BEREICHE.binary_search_by_key(&pointer, |&(start, _)| start) {
        Ok(i) => i,
        Err(0) => return None,
        Err(i) => i - 1,
    };
    let (start, codepunkt) = GB18030_BEREICHE[index];
    char::from_u32(codepunkt + (pointer - start))
}

// ===========================================================================
// KODIERER (Release 11.10.2026, WHATWG Encoding §4.3 und die Encoder der
// einzelnen Kodierungen) — fuer URL-Abfragen und Formulare im Zeichensatz
// des Dokuments.
// ===========================================================================

/// Ein Stueck der kodierten Ausgabe: Bytes, oder ein Zeichen, das die
/// Kodierung nicht kennt („encode or fail": der Aufrufer entscheidet, was
/// daraus wird — in URLs `%26%23N%3B`, in Formularen `&#N;`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kodiert {
    Bytes(Vec<u8>),
    Fehler(char),
}

impl Kodierung {
    /// Die Ausgabe-Kodierung (§4.3 „get an output encoding"): replacement
    /// und UTF-16 kodieren als UTF-8.
    pub fn ausgabe(self) -> Kodierung {
        match self {
            Kodierung::Ersatz | Kodierung::Utf16Le | Kodierung::Utf16Be => Kodierung::Utf8,
            k => k,
        }
    }

    /// Der Name in der Schreibweise des Standards (`document.characterSet`):
    /// `UTF-8`, `Shift_JIS`, `EUC-KR`, `Big5`, `GBK`, `ISO-8859-2`, `KOI8-R`,
    /// `IBM866`, aber `windows-1252`, `macintosh`, `gb18030`, `x-…`.
    pub fn kanonischer_name(&self) -> String {
        let n = self.name();
        match n {
            "shift_jis" => String::from("Shift_JIS"),
            "big5" => String::from("Big5"),
            "gbk" => String::from("GBK"),
            _ if n.starts_with("utf-") || n.starts_with("iso-") || n.starts_with("koi8-") || n.starts_with("euc-") || n == "ibm866" => {
                n.to_ascii_uppercase()
            }
            _ => n.to_string(),
        }
    }
}

/// Die Rueckwaertstabelle Codepunkt -> Zeiger (erster Zeiger, ausser die
/// Ausnahmen), einmal gebaut.
fn rueckwaerts(tabelle: &[(u32, u32)], auslassen: impl Fn(u32) -> bool) -> std::collections::HashMap<u32, u32> {
    let mut m = std::collections::HashMap::with_capacity(tabelle.len());
    for &(zeiger, cp) in tabelle {
        if !auslassen(zeiger) {
            m.entry(cp).or_insert(zeiger);
        }
    }
    m
}

fn jis0208_rueck() -> &'static std::collections::HashMap<u32, u32> {
    static M: std::sync::OnceLock<std::collections::HashMap<u32, u32>> = std::sync::OnceLock::new();
    M.get_or_init(|| rueckwaerts(&JIS0208, |_| false))
}

/// §13.3.2 „index Shift_JIS pointer": jis0208 ohne die Zeiger 8272..=8835.
fn shift_jis_rueck() -> &'static std::collections::HashMap<u32, u32> {
    static M: std::sync::OnceLock<std::collections::HashMap<u32, u32>> = std::sync::OnceLock::new();
    M.get_or_init(|| rueckwaerts(&JIS0208, |z| (8272..=8835).contains(&z)))
}

fn euc_kr_rueck() -> &'static std::collections::HashMap<u32, u32> {
    static M: std::sync::OnceLock<std::collections::HashMap<u32, u32>> = std::sync::OnceLock::new();
    M.get_or_init(|| rueckwaerts(&EUC_KR, |_| false))
}

fn gb18030_rueck() -> &'static std::collections::HashMap<u32, u32> {
    static M: std::sync::OnceLock<std::collections::HashMap<u32, u32>> = std::sync::OnceLock::new();
    M.get_or_init(|| rueckwaerts(&GB18030, |_| false))
}

/// §12.1.2 „index Big5 pointer": ohne die Zeiger unter (0xA1-0x81)*157; fuer
/// sechs Codepunkte gilt der LETZTE Zeiger.
fn big5_rueck() -> &'static std::collections::HashMap<u32, u32> {
    static M: std::sync::OnceLock<std::collections::HashMap<u32, u32>> = std::sync::OnceLock::new();
    M.get_or_init(|| {
        let mut m = rueckwaerts(&BIG5, |z| z < (0xA1 - 0x81) * 157);
        for &(zeiger, cp) in BIG5.iter() {
            if zeiger >= (0xA1 - 0x81) * 157 && matches!(cp, 0x2550 | 0x255E | 0x2561 | 0x256A | 0x5341 | 0x5345) {
                m.insert(cp, zeiger);
            }
        }
        m
    })
}

/// §11.2.2: der Zeiger in den gb18030-Bereichen.
fn gb18030_bereich_zeiger(cp: u32) -> u32 {
    if cp == 0xE7C7 {
        return 7457;
    }
    if cp >= 0x10000 {
        return 189000 + cp - 0x10000;
    }
    let i = match GB18030_BEREICHE.binary_search_by_key(&cp, |&(_, c)| c) {
        Ok(i) => i,
        Err(0) => 0,
        Err(i) => i - 1,
    };
    let (zeiger, start) = GB18030_BEREICHE[i];
    zeiger + cp - start
}

fn einbyte_kodieren(c: char, finde: impl Fn(u32) -> Option<usize>) -> Kodiert {
    let cp = c as u32;
    if cp < 0x80 {
        return Kodiert::Bytes(vec![cp as u8]);
    }
    match finde(cp) {
        Some(i) => Kodiert::Bytes(vec![0x80 + i as u8]),
        None => Kodiert::Fehler(c),
    }
}

/// Release 11.10.2026: `text` in der Ausgabe-Kodierung von `k` („encode or
/// fail" je Zeichen). ISO-2022-JP schaltet vor einem Fehler und am Ende
/// zurueck auf ASCII (`ESC ( B`).
pub fn kodiere(text: &str, k: Kodierung) -> Vec<Kodiert> {
    let k = k.ausgabe();
    if k == Kodierung::Iso2022Jp {
        return kodiere_iso2022jp(text);
    }
    text.chars().map(|c| kodiere_zeichen(c, k)).collect()
}

fn kodiere_zeichen(c: char, k: Kodierung) -> Kodiert {
    let cp = c as u32;
    if cp < 0x80 && !matches!(k, Kodierung::Utf8) {
        return Kodiert::Bytes(vec![cp as u8]);
    }
    let zwei = |lead: u32, trail: u32| Kodiert::Bytes(vec![lead as u8, trail as u8]);
    match k {
        Kodierung::Utf8 | Kodierung::Utf16Le | Kodierung::Utf16Be | Kodierung::Ersatz | Kodierung::Iso2022Jp => {
            let mut b = [0u8; 4];
            Kodiert::Bytes(c.encode_utf8(&mut b).as_bytes().to_vec())
        }
        Kodierung::Windows1252 => einbyte_kodieren(c, |cp| WINDOWS_1252.iter().position(|&x| x == cp)),
        Kodierung::Iso885915 => einbyte_kodieren(c, |cp| ISO_8859_15.iter().position(|&x| x == cp)),
        Kodierung::Einbyte(i) => match EINBYTE.get(i as usize) {
            Some((_, t)) => einbyte_kodieren(c, |cp| t.iter().position(|&x| x != 0 && x as u32 == cp)),
            None => Kodiert::Fehler(c),
        },
        Kodierung::Iso88598I => match EINBYTE.iter().find(|e| e.0 == "iso-8859-8") {
            Some((_, t)) => einbyte_kodieren(c, |cp| t.iter().position(|&x| x != 0 && x as u32 == cp)),
            None => Kodiert::Fehler(c),
        },
        Kodierung::XUserDefined => {
            if (0xF780..=0xF7FF).contains(&cp) {
                Kodiert::Bytes(vec![(cp - 0xF780 + 0x80) as u8])
            } else {
                Kodiert::Fehler(c)
            }
        }
        // §13.3.2
        Kodierung::ShiftJis => {
            if cp == 0x80 {
                return Kodiert::Bytes(vec![0x80]);
            }
            if cp == 0xA5 {
                return Kodiert::Bytes(vec![0x5C]);
            }
            if cp == 0x203E {
                return Kodiert::Bytes(vec![0x7E]);
            }
            if (0xFF61..=0xFF9F).contains(&cp) {
                return Kodiert::Bytes(vec![(cp - 0xFF61 + 0xA1) as u8]);
            }
            let cp = if cp == 0x2212 { 0xFF0D } else { cp };
            match shift_jis_rueck().get(&cp) {
                Some(&z) => {
                    let (lead, trail) = (z / 188, z % 188);
                    let lead_v = if lead < 0x1F { 0x81 } else { 0xC1 };
                    let trail_v = if trail < 0x3F { 0x40 } else { 0x41 };
                    zwei(lead + lead_v, trail + trail_v)
                }
                None => Kodiert::Fehler(c),
            }
        }
        // §13.1.2
        Kodierung::EucJp => {
            if cp == 0xA5 {
                return Kodiert::Bytes(vec![0x5C]);
            }
            if cp == 0x203E {
                return Kodiert::Bytes(vec![0x7E]);
            }
            if (0xFF61..=0xFF9F).contains(&cp) {
                return Kodiert::Bytes(vec![0x8E, (cp - 0xFF61 + 0xA1) as u8]);
            }
            let cp = if cp == 0x2212 { 0xFF0D } else { cp };
            match jis0208_rueck().get(&cp) {
                Some(&z) => zwei(z / 94 + 0xA1, z % 94 + 0xA1),
                None => Kodiert::Fehler(c),
            }
        }
        // §14.1.2
        Kodierung::EucKr => match euc_kr_rueck().get(&cp) {
            Some(&z) => zwei(z / 190 + 0x81, z % 190 + 0x41),
            None => Kodiert::Fehler(c),
        },
        // §12.1.2
        Kodierung::Big5 => match big5_rueck().get(&cp) {
            Some(&z) => {
                let trail = z % 157;
                zwei(z / 157 + 0x81, trail + if trail < 0x3F { 0x40 } else { 0x62 })
            }
            None => Kodiert::Fehler(c),
        },
        // §11.2.2
        Kodierung::Gbk | Kodierung::Gb18030 => {
            let gbk = k == Kodierung::Gbk;
            if cp == 0xE5E5 {
                return Kodiert::Fehler(c);
            }
            if gbk && cp == 0x20AC {
                return Kodiert::Bytes(vec![0x80]);
            }
            if let Some(&z) = gb18030_rueck().get(&cp) {
                let trail = z % 190;
                return zwei(z / 190 + 0x81, trail + if trail < 0x3F { 0x40 } else { 0x41 });
            }
            if gbk {
                return Kodiert::Fehler(c);
            }
            let mut z = gb18030_bereich_zeiger(cp);
            let b1 = z / (10 * 126 * 10);
            z %= 10 * 126 * 10;
            let b2 = z / (10 * 126);
            z %= 10 * 126;
            let b3 = z / 10;
            let b4 = z % 10;
            Kodiert::Bytes(vec![(b1 + 0x81) as u8, (b2 + 0x30) as u8, (b3 + 0x81) as u8, (b4 + 0x30) as u8])
        }
    }
}

/// §13.2.2 der ISO-2022-JP-Kodierer (ohne die Halbbreiten-Katakana-
/// Umsetzung: sie sind hier ein Fehler).
fn kodiere_iso2022jp(text: &str) -> Vec<Kodiert> {
    #[derive(Clone, Copy, PartialEq)]
    enum Z {
        Ascii,
        Roman,
        Jis0208,
    }
    const ZU_ASCII: [u8; 3] = [0x1B, 0x28, 0x42];
    let mut raus = Vec::new();
    let mut z = Z::Ascii;
    let mut zeichen: Vec<char> = text.chars().rev().collect();
    while let Some(c) = zeichen.pop() {
        let cp = c as u32;
        let ascii = cp < 0x80;
        if matches!(z, Z::Ascii | Z::Roman) && matches!(cp, 0x0E | 0x0F | 0x1B) {
            if z != Z::Ascii {
                raus.push(Kodiert::Bytes(ZU_ASCII.to_vec()));
                z = Z::Ascii;
            }
            raus.push(Kodiert::Fehler('\u{FFFD}'));
            continue;
        }
        if z == Z::Ascii && ascii {
            raus.push(Kodiert::Bytes(vec![cp as u8]));
            continue;
        }
        if z == Z::Roman && ((ascii && cp != 0x5C && cp != 0x7E) || cp == 0xA5 || cp == 0x203E) {
            let b = match cp {
                0xA5 => 0x5C,
                0x203E => 0x7E,
                _ => cp as u8,
            };
            raus.push(Kodiert::Bytes(vec![b]));
            continue;
        }
        if ascii && z != Z::Ascii {
            zeichen.push(c);
            z = Z::Ascii;
            raus.push(Kodiert::Bytes(ZU_ASCII.to_vec()));
            continue;
        }
        if (cp == 0xA5 || cp == 0x203E) && z != Z::Roman {
            zeichen.push(c);
            z = Z::Roman;
            raus.push(Kodiert::Bytes(vec![0x1B, 0x28, 0x4A]));
            continue;
        }
        let cp = if cp == 0x2212 { 0xFF0D } else { cp };
        match jis0208_rueck().get(&cp) {
            Some(&zeiger) => {
                if z != Z::Jis0208 {
                    zeichen.push(c);
                    z = Z::Jis0208;
                    raus.push(Kodiert::Bytes(vec![0x1B, 0x24, 0x42]));
                    continue;
                }
                raus.push(Kodiert::Bytes(vec![(zeiger / 94 + 0x21) as u8, (zeiger % 94 + 0x21) as u8]));
            }
            None => {
                // „encode or fail": erst zurueck nach ASCII, dann der Fehler.
                if z != Z::Ascii {
                    raus.push(Kodiert::Bytes(ZU_ASCII.to_vec()));
                    z = Z::Ascii;
                }
                raus.push(Kodiert::Fehler(c));
            }
        }
    }
    if z != Z::Ascii {
        raus.push(Kodiert::Bytes(ZU_ASCII.to_vec()));
    }
    raus
}

/// Release 11.10.2026: URL-Standard „percent-encode after encoding" fuer die
/// Abfrage (`query` bzw. `special-query percent-encode set`): Bytes aus dem
/// Satz werden `%XX`, nicht darstellbare Zeichen `%26%23N%3B`.
pub fn abfrage_prozentkodieren(text: &str, k: Kodierung, besonders: bool) -> String {
    let mut raus = String::with_capacity(text.len());
    for teil in kodiere(text, k) {
        match teil {
            Kodiert::Bytes(bs) => {
                for b in bs {
                    let im_satz = b < 0x21 || b > 0x7E || matches!(b, b'"' | b'#' | b'<' | b'>') || (besonders && b == b'\'');
                    if im_satz {
                        raus.push_str(&format!("%{b:02X}"));
                    } else {
                        raus.push(b as char);
                    }
                }
            }
            Kodiert::Fehler(c) => raus.push_str(&format!("%26%23{}%3B", c as u32)),
        }
    }
    raus
}

// ===========================================================================
// Tests (nur die Einheit, die zu dieser Datei gehoert)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_und_utf8() {
        assert_eq!(Kodierung::Utf8.name(), "utf-8");
        assert_eq!(Kodierung::Utf16Le.name(), "utf-16le");
        assert_eq!(Kodierung::Utf16Be.name(), "utf-16be");
        assert_eq!(Kodierung::Windows1252.name(), "windows-1252");
        assert_eq!(Kodierung::Iso885915.name(), "iso-8859-15");
        assert_eq!(Kodierung::ShiftJis.name(), "shift_jis");
        assert_eq!(Kodierung::EucKr.name(), "euc-kr");
        assert_eq!(Kodierung::Gb18030.name(), "gb18030");
        assert!(Kodierung::Utf8.ist_utf8());
        assert!(!Kodierung::Windows1252.ist_utf8());
        assert!(Kodierung::Utf8.ist_utf());
        assert!(Kodierung::Utf16Le.ist_utf());
        assert!(Kodierung::Utf16Be.ist_utf());
        assert!(!Kodierung::Windows1252.ist_utf());
    }

    #[test]
    fn labels_mit_rand_leerraum() {
        assert_eq!(
            kodierung_aus_label("  LATIN1\n"),
            Some(Kodierung::Windows1252)
        );
        assert_eq!(kodierung_aus_label("latin9"), Some(Kodierung::Iso885915));
        // Release 11.10.2026: `gb2312` ist ein GBK-Etikett (WHATWG); GBK dekodiert
        // wie gb18030, traegt aber seinen eigenen Namen.
        assert_eq!(kodierung_aus_label("gb2312"), Some(Kodierung::Gbk));
        assert_eq!(kodierung_aus_label("sjis"), Some(Kodierung::ShiftJis));
        assert_eq!(kodierung_aus_label("euc-kr"), Some(Kodierung::EucKr));
        assert_eq!(kodierung_aus_label("bogus"), None);
        assert_eq!(kodierung_aus_label(""), None);
    }

    #[test]
    fn nummern_der_tabelle_stimmen() {
        // Die Zahlen stehen im Kopf von kodierung_tabellen.rs und in
        // `nummer`. Diese Pruefung haelt beide Seiten zusammen: wer eine
        // Zahl aendert, ohne die andere Seite zu aendern, faellt hier auf.
        // Nummern 0-5 sind die Original-Kodierungen (ETIKETTEN-Tabelle).
        // UTF-16 (6, 7) wird nicht ueber die Tabelle adressiert.
        for (kod, nummer) in [
            (Kodierung::Utf8, 0u8),
            (Kodierung::Windows1252, 1),
            (Kodierung::Iso885915, 2),
            (Kodierung::ShiftJis, 3),
            (Kodierung::EucKr, 4),
            (Kodierung::Gb18030, 5),
            (Kodierung::Utf16Le, 6),
            (Kodierung::Utf16Be, 7),
        ] {
            assert_eq!(kod.nummer(), nummer);
            assert_eq!(Kodierung::aus_nummer(nummer), Some(kod));
        }
        assert_eq!(Kodierung::aus_nummer(8), None);
    }

    #[test]
    fn utf8_genau_ein_ersatzzeichen_je_teil() {
        // Ein abgebrochenes Dreierzeichen ohne Fortsetzung: EIN U+FFFD.
        assert_eq!(dekodiere(b"\xE2\x82", Kodierung::Utf8), "\u{FFFD}");
        // E0 80: E0 kann hier nicht beginnen (unten A0), 80 ist fuer sich
        // ungueltig -> ZWEI U+FFFD.
        assert_eq!(dekodiere(b"\xE0\x80", Kodierung::Utf8), "\u{FFFD}\u{FFFD}");
        // Gueltiges bleibt gueltig.
        assert_eq!(
            dekodiere("Gruesse \u{20AC}".as_bytes(), Kodierung::Utf8),
            "Gruesse \u{20AC}"
        );
    }

    #[test]
    fn einbyte_sonderzeichen() {
        // windows-1252: 0x80 = Euro, 0x9C = oe-Ligatur.
        assert_eq!(
            dekodiere(&[0x80, 0x9C], Kodierung::Windows1252),
            "\u{20AC}\u{153}"
        );
        // iso-8859-15: 0xA4 = Euro, 0x80 = C1-Steuerzeichen.
        assert_eq!(
            dekodiere(&[0xA4, 0x80], Kodierung::Iso885915),
            "\u{20AC}\u{80}"
        );
    }

    #[test]
    fn zweibyte_bekannte_werte() {
        // Shift_JIS "あ" = 0x82 0xA0.
        assert_eq!(dekodiere(&[0x82, 0xA0], Kodierung::ShiftJis), "\u{3042}");
        // EUC-KR "가" = 0xB0 0xA1.
        assert_eq!(dekodiere(&[0xB0, 0xA1], Kodierung::EucKr), "\u{AC00}");
        // gb18030 zweibyte "啊" = 0xB0 0xA1.
        assert_eq!(dekodiere(&[0xB0, 0xA1], Kodierung::Gb18030), "\u{554A}");
        // gb18030 vierbyte U+10000 = 0x90 0x30 0x81 0x30.
        assert_eq!(
            dekodiere(&[0x90, 0x30, 0x81, 0x30], Kodierung::Gb18030),
            "\u{10000}"
        );
    }

    #[test]
    fn grenzen_und_binaersuche() {
        // Ein fehlender Pointer im JIS0208-Index (108 ist eine Luecke).
        assert_eq!(index_codepunkt(&JIS0208, 108), None);
        // Der erste und der letzte Eintrag. 11103 ist der POINTER des
        // letzten Eintrags (0x2B5F), keine Zeilennummer der Indexdatei
        // — das stand hier vorher als "Zeile 11103" und liest sich
        // wie eine Dateigroesse (03.10.2026).
        assert_eq!(index_codepunkt(&JIS0208, 0), Some('\u{3000}'));
        assert_eq!(index_codepunkt(&JIS0208, 11103), Some('黑'));
        // Und der groesste Codepunkt ueberhaupt (U+FFE5, Pointer 78).
        assert_eq!(index_codepunkt(&JIS0208, 78), Some('\u{FFE5}'));
        // gb18030-Bereiche: unterster und oberster gueltiger Pointer.
        assert_eq!(gb18030_bereich(0), Some('\u{80}'));
        assert_eq!(gb18030_bereich(189000), Some('\u{10000}'));
        assert_eq!(gb18030_bereich(1237575), Some('\u{10FFFF}'));
        assert_eq!(gb18030_bereich(1237576), None);
        assert_eq!(gb18030_bereich(7457), Some('\u{E7C7}'));
    }

    /// DIE VORAUSSETZUNG DER BINAERSUCHE, als Zusicherung.
    ///
    /// GEMESSEN (03.10.2026) an allen Index-Tabellen: JIS0208 7724
    /// Eintraege, streng aufsteigend, Pointer eindeutig, erster
    /// Pointer 0x0 (U+3000), letzter 0x2B5F = 11103 (U+9ED1 '黑');
    /// EUC_KR 17048, GB18030 23940, GB18030_BEREICHE 207 — dieselben
    /// Zahlen wie im Dateikopf von `kodierung_tabellen.rs`.
    ///
    /// WARUM DIES EIN TEST IST und kein Kommentar: `index_codepunkt`
    /// sucht mit `binary_search_by_key`. Ein spaeter erzeugter Index,
    /// der unsortiert ist (etwa weil die Datei eine Zeile weniger
    /// traegt oder weil ein Generator sortiert nach Text statt nach
    /// Zahl), liefert dann stillschweigend FALSCHE Codepoints — kein
    /// Panik, kein Fehler, nur falsche Zeichen in einer Seite. Die
    /// Randzeilen-Zusicherungen oben allein fangen das nicht: sie
    /// pruefen zwei Zeilen, nicht die Reihenfolge dazwischen.
    #[test]
    fn alle_indizes_sind_steigend_und_eindeutig() {
        fn pruefe(name: &str, tabelle: &[(u32, u32)]) {
            assert!(
                !tabelle.is_empty(),
                "{name}: leere Tabelle — die Binaersuche kann dort nichts finden"
            );
            for w in tabelle.windows(2) {
                assert!(
                    w[0].0 < w[1].0,
                    "{name}: Pointer {:#x} steht nicht vor {:#x} — die \
                     Binaersuche in `index_codepunkt` verlangt strenge \
                     aufsteigende Reihenfolge",
                    w[0].0,
                    w[1].0
                );
            }
        }
        pruefe("JIS0208", &JIS0208);
        pruefe("EUC_KR", &EUC_KR);
        pruefe("GB18030", &GB18030);
        pruefe("GB18030_BEREICHE", &GB18030_BEREICHE);
    }

    /// DER INHALT DER TABELLEN, wie ihn die WHATWG-Quelle hat.
    ///
    /// WICHTIG, und das ist der Grund fuer diesen Test: die Laengen
    /// sind bereits durch den Typ erzwungen (`[(u32, u32); 7724]`),
    /// eine Laengen-Zusicherung im Test waere also-tautologisch und
    /// koennte nicht beissen. GEMESSEN am 03.10.2026: ich habe eine
    /// Tabelle testweise um einen Eintrag vergroessert, und der
    /// Compiler hat ROT gemeldet, bevor der Test laufen konnte.
    ///
    /// Was der Compiler NICHT abdeckt, ist der Inhalt: ein Generator,
    /// der nach Text statt nach Zahl sortiert, oder der einen
    /// Codepoint verschiebt, liefert eine Tabelle der richtigen
    /// Laenge mit falschen Zeichen. Genau dagegen prueft die
    /// FNV-1a-Summe hier — und sie ist heute gegen die offiziellen
    /// Indexdateien gemessen, Position fuer Position:
    /// 128/128, 128/128, 7724/7724, 17048/17048, 23940/23940,
    /// 207/207, zusammen 49175/49175 = 100,0000 % (Bericht _76,
    /// Skript /home/jaff/mutation_md/gemini/indizes/pruefe_indizes.py).
    ///
    /// Wer die Tabellen neu erzeugt, MUSS diese Summen neu messen
    /// und den Kommentar hier mit dem Ergebnis fuellen — sonst ist
    /// der Test der richtige Bescheid.
    #[test]
    fn tabellen_inhalte_entsprechen_der_whatwg_quelle() {
        fn fnv64(werte: &[u32]) -> u64 {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for w in werte {
                for b in w.to_le_bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x100_0000_01b3);
                }
            }
            h
        }
        fn alle(tabelle: &[(u32, u32)]) -> Vec<u32> {
            let mut raus = Vec::with_capacity(tabelle.len() * 2);
            for (a, b) in tabelle {
                raus.push(*a);
                raus.push(*b);
            }
            raus
        }
        assert_eq!(fnv64(&WINDOWS_1252), 0x3c48_1ca4_9eeF_fb78, "windows-1252");
        assert_eq!(fnv64(&ISO_8859_15), 0xec2d_f6e1_72f1_bed8, "iso-8859-15");
        assert_eq!(fnv64(&alle(&JIS0208)), 0xf7de_ec2d_0647_7726, "jis0208");
        assert_eq!(fnv64(&alle(&EUC_KR)), 0x5dab_56cc_6213_a925, "euc-kr");
        assert_eq!(fnv64(&alle(&GB18030)), 0x5525_7293_2854_88de, "gb18030");
        assert_eq!(
            fnv64(&alle(&GB18030_BEREICHE)),
            0x23f6_7acb_eb94_c118,
            "gb18030-ranges"
        );
    }

    /// KEINE ZWEIFACHEN ETIKETTEN: die Etikettentabelle wird beim
    /// Nachschlagen linear durchlaufen, ein doppelter Schluessel
    /// waere stiller Schatten. Zusaetzlich wird jeder Schluessel
    /// genau einmal gesehen — beides in einem Lauf, damit ein
    /// fehlender Test hier nicht still bleibt.
    #[test]
    fn etiketten_sind_eindeutig() {
        let mut gesehen = std::collections::BTreeSet::new();
        for (name, _) in ETIKETTEN.iter() {
            assert!(
                gesehen.insert(*name),
                "Etikett {name:?} steht zweimal in ETIKETTEN — der \
                 erste Treffer gewinnt, der zweite ist toter Code"
            );
        }
        assert_eq!(gesehen.len(), ETIKETTEN.len());
        // Schluessel, die der Dateikopf als IANA-Zusaetze benennt;
        // diese muessen weiterhin in der Tabelle vorhanden sein.
        // "utf-16" wird in `kodierung_aus_label` gesondert behandelt
        // (nicht ueber die Tabelle).
        for erwartet in ["latin0", "latin9", "gbk", "euc-kr"] {
            assert!(
                gesehen.contains(erwartet),
                "Etikett {erwartet:?} fehlt — es ist im Dateikopf als \
                 vorhanden beschrieben"
            );
        }
    }
}
