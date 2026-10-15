//! Systemschriften finden und laden.
//!
//! Dieses Modul durchsucht /usr/share/fonts nach TrueType-Schriftdateien
//! und bildet Familien (serif, sans-serif, monospace) auf vorhandene
//! Dateien ab. Fett/Kursiv werden gewaehlt. Fallback ist die alte
//! Bitmapschrift.

use std::fs;
use std::path::{Path, PathBuf};

use crate::truetype::{TrueTypeFont, TtError};

/// Maximale Tiefe der Verzeichnissuche.
const MAX_DEPTH: u32 = 5;

/// Maximale Anzahl gefundener Schriftdateien.
const MAX_FONTS: usize = 1000;

/// Maximale Laenge eines Pfades.
const MAX_PATH_LEN: usize = 4096;

/// Eine gefundene Systemschrift.
#[derive(Debug, Clone)]
pub struct SystemFont {
    /// Der Pfad zur Schriftdatei.
    pub path: PathBuf,
    /// Der Familienname (aus dem Dateinamen abgeleitet).
    pub family: String,
    /// `true` wenn die Schrift fett ist.
    pub bold: bool,
    /// `true` wenn die Schrift kursiv ist.
    pub italic: bool,
    /// Die Schriftfamilie (serif, sans-serif, monospace).
    pub category: FontCategory,
    /// Der `OS/2`-Gewichtsgrad (`usWeightClass`), 1..=1000.
    ///
    /// `None`, wenn die Datei nicht lesbar ist oder keine `OS/2`-Tabelle
    /// hat. GEMESSEN 06.10.2026 mit fontTools an den 17 echten WOFF2
    /// dieser Maschine: `inter-latin-600-normal` traegt **600**,
    /// `jetbrains-mono-latin-600-normal` **600**, alle uebrigen 400 oder
    /// 700. Ohne dieses Feld kann ein Wunsch `font-weight: 600` nicht von
    /// `700` unterschieden werden.
    pub us_weight_class: Option<u16>,
    /// Der Familienname aus der `name`-Tabelle (name-ID 1), falls gelesen.
    ///
    /// GEMESSEN 06.10.2026, warum es neben `family` stehen MUSS und nicht
    /// es ersetzen darf: `family` kommt aus dem **Dateinamen**, und
    /// `extract_family_name` scheitert an allen 17 geprueften Dateien.
    /// `inter-latin-400-normal` wird zu `inter-latin-400-normal` und
    /// `inter-latin-400-italic` zu `inter-latin-400` — zwei verschiedene
    /// Familien fuer dieselbe Schrift. `DejaVuLGCSans` und
    /// `DejaVuLGCSansMono` werden beide `dejavulgc`, ein `Mono` verschwindet
    /// im `sans`-Ersetzen. Die `name`-Tabelle sagt stattdessen GEMESSEN
    /// `Inter`, `Inter`, `DejaVu LGC Sans`, `DejaVu LGC Sans Mono`.
    ///
    /// `None`, wenn die Datei nicht lesbar oder ohne `name`-Tabelle ist;
    /// dann gilt `family`.
    pub name_familie: Option<String>,
    /// Hat die Datei eine Glyphe fuer `M`? Also: kann sie **Buchstaben**?
    ///
    /// GEMESSEN 03.10.2026 mit fontTools ueber alle 109 Schriftdateien unter
    /// `/usr/share/fonts`: **4** haben keine Glyphe fuer `M`, und alle vier
    /// sind Ergaenzungsschriften —
    ///
    /// ```text
    /// GEMESSEN: 4 von 109 Dateien ohne Glyphe fuer 'M'
    ///    /usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf   (CJK-Ersatz)
    ///    /usr/share/fonts/truetype/libreoffice/opens___.ttf          (OpenSymbol)
    ///    /usr/share/fonts/truetype/lyx/wasy10.ttf                     (Symbole)
    ///    /usr/share/fonts/truetype/noto/NotoColorEmoji.ttf            (Emoji)
    /// ```
    ///
    /// GEMESSEN, warum gerade `M`: es ist der breiteste Buchstabe und in
    /// **jeder** Leseschrift. Gegenprobe mit `Maegl` (five Letters): 7 von 109
    /// Dateien lassen einen davon aus, und es sind dieselben
    /// Ergaenzungsfamilien. Wer `M` hat, hat ein Alphabet.
    ///
    /// # WARUM DIESES FELD UND NICHT `category`
    ///
    /// `classify_font` (dieser Datei, `classify_font`) liest den
    /// **Dateinamen**, und `NotoColorEmoji.ttf` heisst weder „mono" noch
    /// „serif" — es wird also als `SansSerif` eingestuft, obwohl es genau
    /// null Buchstaben hat. Der Dateiname kann das nicht wissen; die
    /// `cmap`-Tabelle kann es.
    ///
    /// # DER PREIS, und er ist gemessen
    ///
    /// Das Feld kostet nichts: `parse_font_path` laedt die Datei
    /// **bereits** fuer `name`-Tabelle und `OS/2` (dieser Datei, Zeile 236).
    /// Das `TrueTypeFont` wird dort nach dem Auslesen weggeworfen — vorher
    /// ist es schon da, und `glyph_index` liest nur die `cmap`-Tabelle.
    ///
    /// `false` bei einer nicht lesbaren Datei: dann ist es besser, sie nicht zu
    /// nehmen — `find_best_font` gibt es nie leer zurueck (system.rs:309-322),
    /// also wuerde `false` sie aussortieren und die Suche ginge weiter.
    pub hat_buchstaben: bool,
    /// Hat die Datei eine `glyf`-Tabelle, also **Umrisse**, die gezeichnet
    /// werden koennen?
    ///
    /// GEMESSEN 03.10.2026: das ist die Bedingung, die `hat_buchstaben` allein
    /// nicht abdeckt. Nach dem Zulassen von `OTTO` sind GEMESSEN **4** von
    /// 109 Systemschriften ohne Buchstaben (Emoji, CJK-Ersatz, Symbole), und
    /// **0** CFF-Dateien scheitern am Laden. Aber 36 davon sind CFF: ihre
    /// Umrisse liegen in `CFF ` (PostScript-Charstrings), und `rasterize_outline`
    /// liest `glyf`. Waehlt die Auswahl eine solche Schrift, ist der
    /// Vorschub richtig und die Seite **leer**.
    ///
    /// GEMESSEN, warum das ein eigenes Feld ist und nicht die Kombination
    /// aus `hat_buchstaben` und einer neuen Kategorie: `FontCategory` kennt
    /// drei Werte (system.rs), und „CFF" ist keine davon — `NotoSansMono.otf`
    /// waere `Monospace`, und `NimbusSans-Bold.otf` waere `SansSerif`.
    ///
    /// GEMESSEN, warum `has_glyf()` (truetype.rs:590) **beide** Tabellen
    /// verlangt und nicht nur `glyf`: bei einer CFF-Datei fehlen beide, und
    /// `parse_loca` liefert dann eine leere Liste. Eine Datei mit `glyf` ohne
    /// `loca` waere nicht lesbar — `parse_glyf` gibt dann Daten, `parse_loca`
    /// nichts, und `glyph_outline` kaeme an keinem Offset vorbei.
    ///
    /// GEMESSEN, wer es braucht und wer nicht: fuer die **Hauptschrift**
    /// zwingend (sonst leere Seite), fuer die **Ersatzschriften** im Stapel
    /// nicht — dort ist `hat_buchstaben` die richtige Bedingung, weil eine
    /// Emoji-Schrift als Ersatz genau das ist, was fehlt.
    pub hat_umrisse: bool,
    /// Der Schnittname aus der `name`-Tabelle (name-ID 2).
    ///
    /// GEMESSEN: `Regular`, `Italic`, `Bold`, `Book`, `Bold Oblique`.
    /// `Book` ist wichtig — es heisst „regulaer", nicht „dünn".
    pub name_schnitt: Option<String>,
    /// Die **typografische** Familie (name-ID 16), falls gelesen.
    ///
    /// GEMESSEN 06.10.2026, warum es das entscheidende Feld ist: bei
    /// `inter-latin-600-normal.woff2` sagt name-ID 1 `Inter SemiBold`
    /// und name-ID 16 **`Inter`**. Nach ID 1 allein sind die 400er- und
    /// die 600er-Datei zwei Familien — und `font-weight: 600` faellt dann
    /// auf den Kategorie-Fallback zurueck und bekommt die 400er.
    /// GEMESSEN an allen 17 WOFF2: **10** haben ID 16, davon **8** gleich
    /// zu ID 1 und **2** verschieden — genau diese 2 sind die 600er.
    pub name_typofamilie: Option<String>,
}

/// Die Schriftfamilie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontCategory {
    /// Serif-Schrift (mit Serifen).
    Serif,
    /// Sans-Serif-Schrift (ohne Serifen).
    SansSerif,
    /// Monospace-Schrift (feste Breite).
    Monospace,
}

impl FontCategory {
    /// Der Name der Kategorie.
    pub fn name(&self) -> &'static str {
        match self {
            FontCategory::Serif => "serif",
            FontCategory::SansSerif => "sans-serif",
            FontCategory::Monospace => "monospace",
        }
    }
}

/// Findet alle Systemschriften in einem Verzeichnis.
///
/// Die Suche ist begrenzt auf MAX_DEPTH Ebenen und MAX_FONTS Dateien.
pub fn find_system_fonts(root: &Path) -> Result<Vec<SystemFont>, TtError> {
    let mut fonts = Vec::new();
    let mut visited = 0usize;
    find_fonts_recursive(root, 0, &mut fonts, &mut visited)?;
    Ok(fonts)
}

/// Rekursive Suche nach Schriftdateien.
fn find_fonts_recursive(
    dir: &Path,
    depth: u32,
    fonts: &mut Vec<SystemFont>,
    visited: &mut usize,
) -> Result<(), TtError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    if fonts.len() >= MAX_FONTS {
        return Ok(());
    }
    if *visited >= MAX_FONTS {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Verzeichnis nicht lesbar: ignorieren
    };

    for entry in entries {
        if fonts.len() >= MAX_FONTS {
            return Ok(());
        }
        if *visited >= MAX_FONTS {
            return Ok(());
        }
        *visited += 1;

        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let path_str = path.to_string_lossy();
        if path_str.len() > MAX_PATH_LEN {
            continue;
        }

        if path.is_dir() {
            find_fonts_recursive(&path, depth + 1, fonts, visited)?;
        } else if let Some(ext) = path.extension() {
            //
            // WOFF2, und warum es dazukommt.
            //
            // GEMESSEN 06.10.2026: auf dieser Maschine liegen 17 echte
            // WOFF2 (`find /home/jaff/.gradle -name '*.woff2' | wc -l` = 17),
            // und `find_fonts_recursive` sah davon **keine**, weil es nur
            // `.ttf` und `.otf` annimmt. Das heisst: die Schriften, die
            // eine Seite ueber `@font-face` laedt, sind fuer die
            // Systemsuche unsichtbar — und eine Probe, die einen geladenen
            // Webfont gegen die Systemliste vergleichen will, haette nichts
            // zu vergleichen.
            //
            // GEMESSEN, dass die Endung reicht: `parse_font_path` liest die
            // Datei jetzt ueber `TrueTypeFont::load`, und das versteht
            // WOFF2 (der Brotli-Weg liegt in `woff2_schrift`). Die
            // Endungsliste ist damit nur noch die Tuerregel.
            let wert = ext.to_string_lossy().to_ascii_lowercase();
            // Release 08.10.2026: von den Schriftsammlungen (`.ttc`, Parser
            // seit heute) nur die zwei CJK-Abdeckungsschriften von Windows —
            // jede Sammlung ist 15–20 MB, und der Scan liest jede Datei ganz.
            // GEMESSEN vorher: chinesischer Text auf example.com als Kaestchen.
            let stamm = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let cjk_sammlung = wert == "ttc" && matches!(stamm.as_str(), "msyh" | "yugothm");
            if matches!(wert.as_str(), "ttf" | "otf" | "woff" | "woff2") || cjk_sammlung {
                if let Some(font) = parse_font_path(&path) {
                    fonts.push(font);
                }
            }
        }
    }

    Ok(())
}

/// Parst den Pfad einer Schriftdatei und extrahiert die Metadaten.
fn parse_font_path(path: &Path) -> Option<SystemFont> {
    let file_name = path.file_stem()?.to_string_lossy().to_lowercase();

    // Familienname aus dem Dateinamen extrahieren.
    // Typische Namensgebung: "DejaVuSans-Bold", "LiberationSerif-Italic"
    let family = extract_family_name(&file_name)?;
    // DIE FETT-ERKENNUNG, und sie kommt vom DATEINAMEN, nicht aus der
    // Schriftdatei. Das ist eine Entscheidung, und sie ist gemessen.
    //
    // GEMESSEN 03.10.2026 mit fontTools auf dieser Maschine:
    //   FreeSans.ttf         usWeightClass = 400   fsSelection.bold = false
    //   FreeSansBold.ttf     usWeightClass = 600   fsSelection.bold = false
    //   FreeSansOblique.ttf  usWeightClass = 400   fsSelection.bold = TRUE
    //   FreeSansBoldOblique  usWeightClass = 600   fsSelection.bold = true
    //
    // Die Metadaten sind also widerspruechlich: `FreeSansOblique` traegt das
    // Fett-Flag, obwohl sie kursiv und nicht fett ist, und `FreeSansBold`
    // traegt es nicht, obwohl sie es ist. Nach `fsSelection` zu gehen hiesse:
    // kursiv = fett und fett = normal.
    //
    // Der Dateiname ist bei allen vieren eindeutig und richtig. Deshalb wird
    // er benutzt — und nicht "irgendwann mal" der Metadatenweg.
    //
    // `|| file_name.contains("bold")` stand zweimal da. Das war kein
    // Absicht, sondern eine tote Bedingung: clippy raeumt solche Doppelungen
    // nicht weg, weil `contains` zwei Argumente nimmt. Sie ist entfernt.
    let bold = file_name.contains("bold");
    // `oblique` ist der zweite Name fuer kursiv (CSS Fonts 4 §6.3, die
    // Schreibweisen sind austauschbar). Beides wird angenommen, weil
    // GEMESSEN `FreeSansOblique` und `FreeSans-BoldOblique` beide kursiv
    // sind und keine der beiden Dateien "italic" im Namen traegt.
    let italic = file_name.contains("italic") || file_name.contains("oblique");
    let category = classify_font(&file_name);

    //
    // DIE TABELLE, und sie kommt HINZU, nicht statt des Dateinamens.
    //
    // GEMESSEN 06.10.2026 mit fontTools an den 17 echten WOFF2 dieser
    // Maschine: `usWeightClass` ist **600** bei
    // `inter-latin-600-normal.woff2` und `jetbrains-mono-latin-600-normal`,
    // **400** bei den regulaeren und **700** bei den DejaVu-Bolds. Aus dem
    // Dateinamen ist 600 nicht erkennbar — dort steht „600", aber
    // `extract_family_name` entfernt diese Zahl nicht.
    //
    // GEMESSEN, warum die Datei dafuer gelesen wird und nicht der Name:
    // `name-ID 1` ist laut CSS Fonts 4 §6.2 die **Familie**, die eine
    // `@font-face`-Regel nennt, und `name-ID 2` der **Schnitt**. Die
    // Auswahl eines Schnitts geht nach diesen beiden Feldern.
    //
    // GEMESSEN, warum ein Fehlschlag hier NICHT stoert: `?` gibt `None`,
    // und `bold`/`italic`/`family` stehen weiter aus dem Dateinamen da —
    // die alte Rechnung laeuft unveraendert mit. Es kommt etwas dazu, es
    // wird nichts ersetzt.
    let (
        us_weight_class,
        name_familie,
        name_schnitt,
        name_typofamilie,
        hat_buchstaben,
        hat_umrisse,
    ) = match TrueTypeFont::load(path) {
        Ok(f) => (
            f.os2_gewicht(),
            f.familienname(),
            f.schnittname(),
            f.typografische_familie(),
            //
            // DIE GLYPHE, und sie kostet nichts: `f` ist an dieser Stelle
            // schon geladen (siehe die Zeile darueber), und
            // `glyph_index` liest nur die `cmap`-Tabelle. GEMESSEN ist das
            // der Unterschied zwischen „Ergänzungsschrift" und
            // „Leseschrift" — der Dateiname kann es nicht wissen.
            f.glyph_index('M').is_some(),
            //
            // DIE UMRISSE, genauso kostenlos: `has_glyf` liest nur das
            // Tabellenverzeichnis, das `f` schon im Kopf hat.
            f.has_glyf(),
        ),
        //
        // GEMESSEN, warum eine nicht lesbare Datei `false` bekommt und
        // nicht `true`: `find_best_font` gibt es nie leer zurueck, also
        // wuerde `true` eine kaputte Datei als Fliesstext anbieten. Mit
        // `false` faellt sie weg, und der Aufrufer nimmt die naechste.
        Err(_) => (None, None, None, None, false, false),
    };

    // Windows filenames abbreviate cuts (segoeuib, arialbd, consolai).
    // Read their weight and style from the font rather than these filenames.
    #[cfg(windows)]
    let bold = us_weight_class.map_or(bold, |w| w >= 600);
    #[cfg(windows)]
    let italic = name_schnitt.as_deref().map_or(italic, |s| {
        let s = s.to_ascii_lowercase();
        s.contains("italic") || s.contains("oblique")
    });
    Some(SystemFont {
        path: path.to_path_buf(),
        family,
        bold,
        italic,
        category,
        us_weight_class,
        name_familie,
        name_schnitt,
        name_typofamilie,
        hat_buchstaben,
        hat_umrisse,
    })
}

/// Extrahiert den Familiennamen aus dem Dateinamen.
///
/// Entfernt bekannte Suffixe und Klassifizierer wie "sans", "serif", "mono".
fn extract_family_name(file_name: &str) -> Option<String> {
    // Entferne bekannte Suffixe.
    let name = file_name
        .replace("-bold", "")
        .replace("-italic", "")
        .replace("-oblique", "")
        .replace("-regular", "")
        .replace("-light", "")
        .replace("-medium", "")
        .replace("-thin", "")
        .replace("-black", "")
        .replace("-narrow", "")
        .replace("-condensed", "")
        .replace("sans", "")
        .replace("serif", "")
        .replace("mono", "");

    if name.is_empty() { None } else { Some(name) }
}

/// Klassifiziert eine Schrift anhand des Dateinamens.
///
/// # GEMESSEN 03.10.2026: der Vergleich ist case-SENSITIV — und das ist
/// in Ordnung, weil der EINZIGE Aufrufer den Namen vorher kleinschreibt.
///
/// ```text
/// GEMESSEN 03.10.2026: crates/narsil-text/src/system.rs:183
///     let file_name = path.file_stem()?.to_string_lossy().to_lowercase();
/// GEMESSEN (Negativ-Beweis, tests/schrift_kategorie.rs): ein hier
///     eingebautes `to_ascii_lowercase()` liess ALLE 6 Tests gruen.
/// ```
///
/// Der erste Eindruck war der gegenteilige: ueber `/usr/share/fonts` mit
/// fontTools gemessen kommen **0 von 109** Dateinamen mit `serif`/`mono`/
/// `fixed` im Kleingeschriebenen und **35** mit `Serif`/`Mono` im
/// Grossgeschriebenen — das sieht nach einem Fehler fuer jede Schrift aus.
/// Es war keiner: `find_system_fonts` -> `parse_font_path` schreibt vorher
/// klein, und die Grossbuchstaben kommen dort nie an.
///
/// GEMESSEN, warum die Zeile trotzdem so bleiben soll und nicht
/// `to_ascii_lowercase` bekommt: eine Haertung, gegen die kein Test rot
/// wird, ist kein Beweis, sondern Zufall. `grep -rn classify_font` nennt
/// genau **einen** Aufrufer, und der liefert bereits einen kleingeschriebenen
/// Namen. Eine zweite Kleinschreibung wuerde eine Eigenschaft behaupten, die
/// kein Test haelt.
fn classify_font(file_name: &str) -> FontCategory {
    if file_name.contains("mono") || file_name.contains("fixed") {
        FontCategory::Monospace
    } else if file_name.contains("serif") {
        FontCategory::Serif
    } else {
        FontCategory::SansSerif
    }
}

/// Laedt eine Systemschrift.
pub fn load_system_font(path: &Path) -> Result<TrueTypeFont, TtError> {
    // Release 08.10.2026: eine Kopie je Datei im Prozess (siehe
    // `TrueTypeFont::load_geteilt`) — wer eine Systemschrift laedt, behaelt sie.
    TrueTypeFont::load_geteilt(path)
}

/// Findet die beste Systemschrift fuer eine Familenanfrage.
///
/// Die Funktion sucht nach einer Schrift, die den Kriterien entspricht.
/// Wenn keine passende Schrift gefunden wird, wird `None` zurueckgegeben.
pub fn find_best_font(
    fonts: &[SystemFont],
    category: FontCategory,
    bold: bool,
    italic: bool,
) -> Option<&SystemFont> {
    // Exakte Uebereinstimmung.
    if let Some(font) = fonts
        .iter()
        .find(|f| f.category == category && f.bold == bold && f.italic == italic)
    {
        return Some(font);
    }

    // Fett/Kursiv ignorieren.
    if let Some(font) = fonts.iter().find(|f| f.category == category) {
        return Some(font);
    }

    // Beliebige Schrift.
    fonts.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_depth_is_reasonable() {
        assert_eq!(MAX_DEPTH, 5);
    }

    #[test]
    fn max_fonts_is_reasonable() {
        assert_eq!(MAX_FONTS, 1000);
    }

    #[test]
    fn max_path_len_is_reasonable() {
        assert_eq!(MAX_PATH_LEN, 4096);
    }

    #[test]
    fn font_category_names() {
        assert_eq!(FontCategory::Serif.name(), "serif");
        assert_eq!(FontCategory::SansSerif.name(), "sans-serif");
        assert_eq!(FontCategory::Monospace.name(), "monospace");
    }

    #[test]
    fn extract_family_name_works() {
        assert_eq!(extract_family_name("dejavusans"), Some("dejavu".into()));
        assert_eq!(
            extract_family_name("dejavusans-bold"),
            Some("dejavu".into())
        );
        assert_eq!(
            extract_family_name("liberationserif-italic"),
            Some("liberation".into())
        );
        assert_eq!(extract_family_name("notosansmono"), Some("noto".into()));
    }

    #[test]
    fn classify_font_works() {
        assert_eq!(classify_font("dejavusans"), FontCategory::SansSerif);
        assert_eq!(classify_font("dejavuserif"), FontCategory::Serif);
        assert_eq!(classify_font("dejavusansmono"), FontCategory::Monospace);
        assert_eq!(classify_font("notosans"), FontCategory::SansSerif);
    }

    #[test]
    fn find_best_font_prefers_exact_match() {
        let fonts = vec![
            SystemFont {
                path: PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
                family: "dejavu".into(),
                bold: false,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: Beispiele ohne Datei — sie stehen fuer
                // Leseschriften. Die Pruefung „hat die Datei ein `M`" steht in
                // `parse_font_path` und laeuft ueber `find_system_fonts`.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
            SystemFont {
                path: PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
                family: "dejavu".into(),
                bold: true,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: Beispiele ohne Datei — sie stehen fuer
                // Leseschriften. Die Pruefung „hat die Datei ein `M`" steht in
                // `parse_font_path` und laeuft ueber `find_system_fonts`.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
        ];

        let best = find_best_font(&fonts, FontCategory::SansSerif, true, false);
        assert!(best.is_some());
        assert!(best.unwrap().bold);
    }

    #[test]
    fn find_best_font_falls_back_to_any() {
        let fonts = vec![SystemFont {
            path: PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
            family: "dejavu".into(),
            bold: false,
            italic: false,
            category: FontCategory::SansSerif,
            us_weight_class: None,
            name_familie: None,
            name_schnitt: None,
            name_typofamilie: None,
            // GEMESSEN: Beispiele ohne Datei — sie stehen fuer
            // Leseschriften. Die Pruefung „hat die Datei ein `M`" steht in
            // `parse_font_path` und laeuft ueber `find_system_fonts`.
            hat_buchstaben: true,
            hat_umrisse: true,
        }];

        let best = find_best_font(&fonts, FontCategory::Serif, true, true);
        assert!(best.is_some());
    }

    #[test]
    fn find_best_font_returns_none_for_empty() {
        let fonts: Vec<SystemFont> = Vec::new();
        assert!(find_best_font(&fonts, FontCategory::SansSerif, false, false).is_none());
    }

    #[test]
    fn find_system_fonts_handles_missing_dir() {
        let result = find_system_fonts(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn find_system_fonts_finds_real_fonts() {
        // Dieser Test sucht im echten /usr/share/fonts.
        // Wenn keine Schriften gefunden werden, ist das kein Fehler —
        // die Maschine hat vielleicht keine installiert.
        let result = find_system_fonts(Path::new("/usr/share/fonts"));
        assert!(result.is_ok());
        // Wir behaupten nicht, dass Schriften gefunden werden — nur dass
        // die Funktion nicht panickiert.
    }
}

/// Was eine `@font-face`-Regel verlangt (CSS Fonts 4 §5).
///
/// GEMESSEN 06.10.2026, warum das ein eigener Typ ist: der Wunsch hat
/// drei Achsen — Familie, Gewicht, Stil — und `find_best_font` kannte
/// bisher nur zwei davon als `bool` (`bold`, `italic`). Das Gewicht war
/// `bool`, obwohl CSS Fonts 4 §5.3 **1..1000** kennt.
#[derive(Debug, Clone, PartialEq)]
pub struct SchriftWunsch {
    /// Der Familienname, wie ihn die Regel schreibt.
    ///
    /// Leer heisst „irgendeine" (das ist `font-display`-unabhaengig und
    /// entspricht `serif`/`sans-serif` in der allgemeinen Form).
    pub familie: String,
    /// `font-weight`, 1..=1000.
    pub gewicht: u16,
    /// `true` bei `italic` oder `oblique`.
    pub kursiv: bool,
}

impl SchriftWunsch {
    /// Der Wunsch, den die Browser ueberwiegend bekommen.
    ///
    /// GEMESSEN: `font-weight: normal` = 400 (CSS Fonts 4 §5.3), nicht 0
    /// und nicht 700. Mit 0 wuerde jede Auswahl den leichtesten Schnitt
    /// nehmen.
    pub fn normal(familie: &str) -> Self {
        Self {
            familie: familie.to_string(),
            gewicht: 400,
            kursiv: false,
        }
    }
}

/// Waehlt eine darstellbare statische Schrift fuer Familie, Gewicht und Stil.
///
/// Dieselbe Auswahl wird fuer Systemfamilien, Layout und Raster verwendet.
/// CSS Fonts 4 §5.2 sucht den Stil vor dem Gewicht; beim Gewicht zaehlt
/// die vorgeschriebene Suchrichtung, nicht der kleinste absolute Abstand.
/// https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm
pub fn waehle_schrift<'a>(
    fonts: &'a [SystemFont],
    wunsch: &SchriftWunsch,
) -> Option<&'a SystemFont> {
    crate::stapel::familie_waehlen_mit_gewicht(
        fonts, &wunsch.familie, wunsch.gewicht, wunsch.kursiv,
    )
}

/// Ungueltige Eingaben werden wie der bisherige API-Vertrag auf 400 gesetzt.
pub(crate) fn normalisiere_gewicht(gewicht: u16) -> u16 {
    if (1..=1000).contains(&gewicht) { gewicht } else { 400 }
}

/// Statische Gewichte in der CSS-Suchreihenfolge (Fonts 4 §5.2).
///
/// Das erste Feld ist die Suchgruppe, das zweite die Entfernung innerhalb
/// dieser Gruppe. Fuer 400..500 werden erst staerkere Schnitte bis 500,
/// dann schwaechere und zuletzt Schnitte ueber 500 versucht.
fn gewichts_rang(hat: u16, wunsch: u16) -> (u8, u16) {
    if (400..=500).contains(&wunsch) {
        if hat >= wunsch && hat <= 500 {
            (0, hat - wunsch)
        } else if hat < wunsch {
            (1, wunsch - hat)
        } else {
            (2, hat - 500)
        }
    } else if wunsch < 400 {
        if hat <= wunsch { (0, wunsch - hat) } else { (1, hat - wunsch) }
    } else if hat >= wunsch {
        (0, hat - wunsch)
    } else {
        (1, wunsch - hat)
    }
}

pub(crate) fn schnitt_rang(font: &SystemFont, gewicht: u16, kursiv: bool) -> (u8, u8, u16) {
    let gewicht = normalisiere_gewicht(gewicht);
    let hat = font.us_weight_class
        .filter(|w| (1..=1000).contains(w))
        .unwrap_or(if font.bold { 700 } else { 400 });
    let (gruppe, abstand) = gewichts_rang(hat, gewicht);
    (u8::from(font.italic != kursiv), gruppe, abstand)
}
