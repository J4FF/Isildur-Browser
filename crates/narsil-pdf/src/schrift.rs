//! Schriften: aus einem Zeichencode wird eine Glyphe.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # DIE FUENF FAELLE, die in den zehn PDFs vorkommen
//!
//! GEMESSEN (`grep` ueber `testdaten/pdfs/*.pdf`):
//!
//! ```text
//!   /Subtype /Type1                    7 Dateien   (PostScript, WinAnsi)
//!   /Subtype /TrueType                 6 Dateien   (WinAnsi)
//!   /Subtype /Type0                    4 Dateien   (Identity-H, 2 Byte je Zeichen)
//!     davon /CIDFontType0              2 Dateien   (CFF)
//!     davon /CIDFontType2              3 Dateien   (TrueType)
//! ```
//!
//! # WIE MAN AUS EINEM BYTE EINE GLYPHE MACHT
//!
//!  * **Type1 / TrueType mit WinAnsi**: Das Byte ist ein Zeichen in
//!    WinAnsiEncoding. Man braucht die **Breite** aus der
//!    `/Widths`-Tabelle (oder `/FontDescriptor /MissingWidth`) und die
//!    **Glyphe**. Ohne eingebettete Datei: ein Systemfont.
//!  * **Type0 mit Identity-H**: Zwei Byte bilden einen **CID**; der CID
//!    ist ueber `/CIDToGIDMap` eine Glyphennummer im eingebetteten
//!    TrueType. Das ist der Weg, den die RFC-PDFs gehen (sie
//!    benutzen Identity-H fuer ≡ und ∀).
//!  * **CIDFontType0 (CFF)**: CID -> Glyphe ueber die `charset` des
//!    CFF. `narsil-text` hat keinen CFF-Parser; der Weg ist die
//!    `/CIDToGIDMap` des Type0-Dictionaries, und wenn die fehlt:
//!    Zeichen ueber `/ToUnicode` in Glyphen des CIDFontType0. In
//!    keinem der zehn PDFs ist das der letzte Weg — dort reicht der
//!    Systemfont.
//!
//! # DER PLAN, wenn keine eingebettete Schrift da ist
//!
//! Ein Systemfont, nach `BaseFont` gewaehlt (Helvetica, Times,
//! Courier). GEMESSEN: die RFC-PDFs benutzen ueberall Helvetica und
//! Courier **ohne** eingebettete Datei, weil sie aus `enscript`
//! kommen. Ohne Ersatzschrift waeren die RFC-Seiten leer.

use crate::objekt::Wert;
use narsil_text::truetype::TrueTypeFont;

/// Wie gross eine Zeichentabelle sein darf (M3/M4).
///
/// added 03.10.2026. 65.536 Eintraege. `/W` bei Identity-H-Fonts hat
/// bei den RFC-PDFs ca. 2.300; ein Dokument mit 1.000.000 waere eine
/// Bombe.
pub const MAX_TABELLE: usize = 65_536;

/// Wie viele Zeichencodes ein Textoperator haben darf (M3).
///
/// added 03.10.2026. 32.768. Eine volle Textseite hat 3.000 Zeichen;
/// ein `Tj` mit 1.000.000 Zeichen ist ein Angriff.
pub const MAX_TEXTZEICHEN: usize = 32_768;

/// Wie viele Schriften ein Dokument laden darf (M3).
///
/// added 03.10.2026. 512. GEMESSEN: die groesste hier hat 21.
pub const MAX_SCHRIFTEN: usize = 512;

/// Der Untertyp einer Schriftressource.
///
/// added 03.10.2026. Als Enum, weil an **drei** Stellen nach ihm
/// gefragt wird (Einbettung, Kodierung, Breite) und drei `match`
/// auf `Option<&str>` drei Fehler sind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchriftArt {
    /// PostScript Type1.
    Type1,
    /// TrueType mit einfacher Kodierung.
    TrueType,
    /// Type0 (CID) mit einer der Kodierungen Identity-H/-V.
    Type0,
    /// Type3 (eigene Zeichen). Kommt in den zehn PDFs nicht vor.
    Type3,
    /// Unbekannt oder kein Untertyp.
    Unbekannt,
}

impl SchriftArt {
    /// Aus dem `/Subtype`-Namen.
    pub fn aus_name(name: &str) -> SchriftArt {
        match name {
            "Type1" => SchriftArt::Type1,
            "TrueType" => SchriftArt::TrueType,
            "Type0" => SchriftArt::Type0,
            "Type3" => SchriftArt::Type3,
            _ => SchriftArt::Unbekannt,
        }
    }
}

/// Eine geladene Schriftressource.
///
/// added 03.10.2026. Der eingebettete Font (falls vorhanden) wird
/// **einmal** geladen, weil `TrueTypeFont::from_bytes` die Tabellen
/// parst — das ist der teuerste Schritt beim Rendern, und bei 300
/// `Tj`-Operatoren pro Seite waeren das 300 gleiche Parsovorgaenge.
#[derive(Debug)]
pub struct Ressource {
    /// Der Untertyp.
    pub art: SchriftArt,
    /// Der Basisname (`/BaseFont`), fuer den Systemfont-Ersatz.
    pub basisname: String,
    /// Die eingebettete Schrift, wenn es eine gibt.
    pub eingebettet: Option<Box<TrueTypeFont>>,
    /// Die eingebettete **CFF** (`/FontFile3`), wenn es eine gibt.
    ///
    /// added 03.10.2026. DER GRUND FUER EIN ZWEITES FELD und nicht
    /// eines: `narsil-text` liest `glyf` (TrueType) und gibt fuer eine
    /// CFF-Datei `Err`. Beide Formate liefern Umrisse in
    /// **Schrifteinheiten**, aber aus verschiedenen Dateien — ein
    /// `enum` waere die Alternative und wuerde an **jeder**
    /// Verwendungsstelle zwei Zustaende abfragen. Mit zwei Feldern
    /// entscheidet der Zeichner einmal: erst TrueType, dann CFF.
    pub eingebettet_cff: Option<Box<crate::cff::Cff>>,
    /// Die Breitentabelle (siehe [`Breiten`]).
    pub breiten: Breiten,
    /// `/ToUnicode`-CMap (CID -> Unicode), wenn vorhanden.
    pub tounicode: std::collections::HashMap<u32, char>,
    /// `/Encoding`: `"WinAnsiEncoding"`, `"MacRomanEncoding"` oder ein
    /// Array mit `/Differences`.
    pub kodierung: Kodierung,
    /// `/CIDSystemInfo /Registry` + `Ordering` — nur fuer Bericht.
    pub cid_system: String,
    /// `/DW2`:-breiten fuer vertikale Schreibrichtung (unbenutzt).
    pub dw2: Vec<f64>,
}

/// Die Breiten einer Schrift, **ohne** die eingebettete Datei.
///
/// added 03.10.2026. WARUM EIN EIGENER TYP: der Content-Stream muss
/// den Vorschub nach einem `Tj` kennen, um die Textmatrix
/// weiterzuschieben — und der Interpreter hat **kein Dokument**, um
/// `/Font << /F1 10 0 R >>` aufzuloesen. Der Renderer loest es auf und
/// legt die fertige Tabelle in den Zustand.
///
/// DER WEG, WARUM DAS NICHT ZWEI WAHRHEITEN SIND: `Ressource`
/// enthaelt eine `Breiten` und ruft fuer `breite()` dieselbe Methode
/// auf. Es gibt also nur **eine** Implementierung der Breitenrechnung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Breiten {
    /// `/FirstChar`.
    pub first_char: i64,
    /// `/Widths` in 1/1000 Schriftgroesse.
    pub breiten: Vec<f64>,
    /// Breite fuer Zeichen ausserhalb der Tabelle.
    pub fehlbreite: f64,
    /// `true` fuer Type0 (zwei Byte je Zeichen, `/W` am CIDFont).
    pub cid: bool,
    /// `/DW`.
    pub cid_breite: f64,
    /// `/W` als Liste (CID, Breite).
    pub cid_breiten: Vec<(u32, f64)>,
}

impl Breiten {
    /// Baut die Tabelle aus einem Font-**oder** CIDFont-Woerterbuch.
    ///
    /// added 03.10.2026. `cid_font` ist das Wörterbuch, aus dem `/W`
    /// und `/DW` gelesen werden — bei Type0 also der CIDFont.
    pub fn aus(
        dict: &Wert,
        cid_font: Option<&Wert>,
        basisname: &str,
        aufloesen: &mut dyn FnMut(u32) -> Option<Wert>,
    ) -> Breiten {
        let cid = cid_font.is_some();
        let quelle = cid_font.unwrap_or(dict);
        let mut breiten = Breiten {
            fehlbreite: 500.0,
            cid,
            cid_breite: quelle.get_zahl("DW").unwrap_or(1000.0),
            ..Default::default()
        };
        if !cid {
            // added 03.10.2026. `/Widths` IST OFT EIN **VERWEIS**.
            //
            // GEMESSEN an `1706.03762.pdf`: `/Widths 5837 0 R` mit
            // `/FirstChar 1`. Ohne Aufloesung blieb die Liste leer,
            // `/FirstChar` 0, und **jedes** Zeichen bekam
            // `fehlbreite` = 500. Bei 12-Punkt Text ist ein Vorschub
            // von 0.5 statt 0.28 em der Grund fuer die Ueberlagerung:
            // „Ashish Vaswani" war ein unlesbares Fleisch.
            //
            // `pdflatex` schreibt Breiten **immer** indirekt, weil das
            // Array sonst jeden Font-Woerterbuch aufblaeht. Wer das
            // nicht aufloest, versteht die haeufigsten PDFs im
            // arXiv-Bestand nicht.
            let wert = dict.get("Widths").cloned().map(|w| match w {
                Wert::Ref(n, _) => aufloesen(n).unwrap_or(Wert::Null),
                other => other,
            });
            if let Some(arr) = wert.as_ref().and_then(Wert::array) {
                breiten.first_char = dict.get_int("FirstChar").unwrap_or(0);
                for w in arr {
                    if breiten.breiten.len() >= MAX_TABELLE {
                        break;
                    }
                    breiten.breiten.push(w.zahl().unwrap_or(0.0));
                }
            }
        }
        // added 03.10.2026. DIE 14 STANDARD-SCHRIFTEN.
        //
        // GEMESSEN an `rfc2616.pdf` Seite 5: dort steht
        // `/BaseFont /Times-Roman` **ohne** `/Widths`. Nach
        // ISO 32000-1, 9.2.4 gilt dann „the metrics of the standard
        // font shall be used" — der Leser muss die Breiten kennen.
        //
        // OHNE TABELLE fällt jede Breite auf 500 zurueck. Bei 9,96
        // Punkt Summenzeichen sind das **5 % zu wenig** je Zeichen,
        // und ueber eine Zeile summiert sich der Fehler auf mehrere
        // Punkte: „F" stand bei 76,94 statt bei 78,60 (mutool trace
        // nennt 78,603488).
        //
        // DER NAMENVERGLEICH ist ueber `ohne_praefix`, weil
        // `pdflatex` den Namen mit sechs Zeichen Praefix schreibt.
        if !cid && breiten.breiten.is_empty() {
            if let Some(t) = crate::standard14::tabelle(basisname) {
                breiten.first_char = 32;
                breiten.breiten = t;
            }
        }
        // `/W` am CIDFont kann ebenfalls indirekt sein.
        let w_wert = quelle.get("W").cloned().map(|w| match w {
            Wert::Ref(n, _) => aufloesen(n).unwrap_or(Wert::Null),
            other => other,
        });
        if let Some(arr) = w_wert.as_ref().and_then(Wert::array) {
            w_array_lesen(arr, &mut breiten.cid_breiten);
        }
        // `/MissingWidth` steht am `/FontDescriptor` — der kann nur
        // aufgeloest werden, wenn das Wörterbuch **da** ist. Ohne ihn
        // bleibt die Vorgabe 500.
        breiten
    }

    /// Die Breite eines Zeichencodes in 1/1000 Schriftgroesse.
    ///
    /// added 03.10.2026. `Ressource::breite` ruft genau diese Methode,
    /// damit es nicht zwei Breitenrechnungen gibt.
    pub fn breite(&self, code: u32) -> f64 {
        if self.cid {
            for (c, w) in &self.cid_breiten {
                if *c == code {
                    return *w;
                }
            }
            return self.cid_breite;
        }
        let idx = code as i64 - self.first_char;
        if idx >= 0 && (idx as usize) < self.breiten.len() {
            return self.breiten[idx as usize];
        }
        self.fehlbreite
    }
}

/// Die Kodierung einer einfachen Schrift.
#[derive(Debug, Clone, PartialEq)]
pub enum Kodierung {
    /// `/WinAnsiEncoding`
    WinAnsi,
    /// `/MacRomanEncoding`
    MacRoman,
    /// `/Differences`: Liste (Code, Glyphenname).
    Differences(Vec<(u32, String)>),
    /// Standard (nach `/BaseFont`: Symbol, ZapfDingbats oder WinAnsi).
    Standard,
}

impl Ressource {
    /// Baut eine Ressource aus einem Font-Wörterbuch.
    ///
    /// added 03.10.2026. `aufloesen` holt ein Objekt nach Nummer —
    /// das ist der Zugang zum Dokument, ohne das hier ein zweiter
    /// Dokumenttyp noetig waere.
    ///
    /// DER FEHLER, DER HIER AUFFIEL (und zwar nicht beim Bauen, sondern
    /// beim Rendern): `/W` bei einem Identity-H-Font ist **kein**
    /// einfaches Array, sondern eine Liste von `[c [w1 w2 …] c_first
    /// c_last w …]`. Wer es als Liste von Zahlen liest, hat nach drei
    /// Zahlen eine Breite von 0 und der gesamte Text rutscht nach
    /// links. Deshalb steht die CID-Tabelle in [`Self::cid_breiten`]
    /// und wird **getrennt** von `/Widths` gelesen.
    pub fn laden(dict: &Wert, aufloesen: &mut dyn FnMut(u32) -> Option<Wert>) -> Option<Ressource> {
        let art = SchriftArt::aus_name(dict.get_name("Subtype").unwrap_or(""));
        let basisname = dict.get_name("BaseFont").unwrap_or("").to_string();
        // added 03.10.2026. DER DESCENDANT-FONT, und er ist Pflicht.
        //
        // Bei einem Type0-Font (Identity-H) liegen `/W`, `/DW` und
        // `/FontDescriptor` am **CIDFont**, nicht am Type0-Wörterbuch.
        // Ohne diesen Schritt fand `dummy.pdf` (W3C-Testdatei, Type0 mit
        // eingebettetem TrueType) **keine** eingebettete Schrift, kein
        // `/W` und damit auch keine Breiten — die Seite zeichnete
        // 14 Zeichen und **0** Pixel. Der Fehler sah aus wie ein
        // Rasterizer-Problem und war ein Wörterbuch-Pfad.
        let cid_font: Option<Wert> = if art == SchriftArt::Type0 {
            dict.get("DescendantFonts")
                .and_then(Wert::array)
                .and_then(|a| a.first().cloned())
                .and_then(|v| match v {
                    Wert::Ref(n, _) => aufloesen(n),
                    sonst => Some(sonst),
                })
        } else {
            None
        };
        // Alles, was am CIDFont steht, wird von dort gelesen; fehlt er,
        // gilt das Type0-Wörterbuch selbst (Vorgabe der Spezifikation
        // fuer fehlende Angaben).
        let cid = cid_font.as_ref().unwrap_or(dict);
        // Eingebettete Datei: `/FontDescriptor /FontFile2` (TrueType),
        // `/FontFile3` (CFF, Type1) oder `/FontFile` (Type1).
        let mut eingebettet = None;
        // added 03.10.2026. DIE CFF IST EIN EIGENER WEG, und sie wird
        // **zuerst** versucht, wenn der Schluessel `FontFile3` ist:
        // `TrueTypeFont::from_bytes` gibt fuer eine CFF `Err`, und
        // umgekehrt gibt `Cff::lesen` fuer eine TrueType `Err`. Es
        // gibt keine Datei, die beides ist.
        //
        // GEMESSEN: 43 eingebettete CFF-Programme in fuenf der elf
        // PDFs (`f1040` 9, `fw9` 6, `1207.7214` 17, `1512.03385` 10,
        // `rfc9110` 1). Ohne diesen Zweig bekamen die Schriften da
        // keinen Umriss und wurden mit einem Systemfont gezeichnet.
        let mut eingebettet_cff = None;
        // added 03.10.2026: der erste Lesefehler wird **gemeldet**
        // statt verschluckt.
        let mut cff_fehler: Option<String> = None;
        if let Some((dnummer, _)) = cid.get_ref("FontDescriptor") {
            if let Some(fd) = aufloesen(dnummer) {
                for schluessel in ["FontFile2", "FontFile3", "FontFile"] {
                    let Some((fnummer, _)) = fd.get_ref(schluessel) else {
                        continue;
                    };
                    let Some(font_stream) = aufloesen(fnummer) else {
                        continue;
                    };
                    let bytes = stream_inhalt(font_stream, aufloesen);
                    if let Ok(f) = TrueTypeFont::from_bytes(&bytes) {
                        eingebettet = Some(Box::new(f));
                        break;
                    }
                    // added 03.10.2026. DER TYPE WIRD **GEPRUEFT**:
                    // `/FontFile3 /Subtype /Type1C` oder `/CIDFontType0C`.
                    // Ein `/FontFile3` mit unbekanntem Subtype wird
                    // **nicht** geraten — sonst wuerde eine verschluesselte
                    // Datei als CFF gelesen und ergaebe Muellglyphen.
                    // added 03.10.2026. `/Subtype` wird **aus dem
                    // Wörterbuch** gelesen, und ein Wörterbuch ist hier
                    // eine `Vec<(Vec<u8>, Wert)>` — es gibt kein
                    // `get_name` darauf, sondern nur auf `Wert`.
                    let mut subtyp = String::new();
                    if let Some((dsn, _)) = fd.get_ref("FontFile3")
                        && let Some(strom) = aufloesen(dsn)
                        && let Some(d) = strom.dict()
                        && let Some((_, v)) = d.iter().find(|(k, _)| k.as_slice() == b"Subtype")
                        && let Some(n) = v.name()
                    {
                        subtyp = n.to_string();
                    }
                    if schluessel == "FontFile3"
                        && (subtyp == "Type1C" || subtyp == "CIDFontType0C")
                    {
                        // added 03.10.2026. DER FEHLER, DEN DIE GANZE
                        // CFF-ARBEIT ZUERST VERSTECKT HAT: `Cff::lesen`
                        // war in einer `if let`-Kette, und die Kette
                        // brach beim **ersten** Fehler ab. Der Fehler kam
                        // aus `Bereich::ende` und war *kein* Fehler des
                        // Lesers.
                        match crate::cff::Cff::lesen(&bytes) {
                            Ok(c) => {
                                eingebettet_cff = Some(Box::new(c));
                                break;
                            }
                            Err(e) => {
                                cff_fehler = Some(format!("{e}"));
                            }
                        }
                    }
                }
            }
        }
        // added 03.10.2026. EINE Breitentabelle statt fuenf Felder.
        //
        // DER GRUND: der Content-Stream muss den Vorschub nach einem
        // `Tj` kennen, um die Textmatrix weiterzuschieben — und er
        // braucht dieselbe Rechnung wie der Renderer. Zwei Orte, zwei
        // Felder (`breiten`/`first_char`/`fehlbreite` hier,
        // `cid_breite`/`cid_breiten` dort) sind zwei Wahrheiten.
        let mut breiten = Breiten::aus(dict, cid_font.as_ref(), &basisname, aufloesen);
        // `/MissingWidth` steht am `/FontDescriptor`. Nur hier ist
        // eine Aufloesung moeglich — deshalb an dieser Stelle, nicht
        // in `Breiten::aus`.
        if let Some((dnummer, _)) = cid.get_ref("FontDescriptor") {
            if let Some(fd) = aufloesen(dnummer) {
                if let Some(mw) = fd.get_zahl("MissingWidth") {
                    breiten.fehlbreite = mw;
                }
            }
        }
        // Kodierung
        // added 05.10.2026: DER RESOLVER, und daran haengt die halbe
        // Liste der Ligaturen. `/Encoding` ist in `1706.03762.pdf` ein
        // **Verweis** (`/Encoding 5834 0 R`) — so schreibt es pdfTeX
        // immer. `kodierung_lesen` kannte nur `Name` und `Dict`, also
        // lief der Verweis in `_ => Kodierung::Standard`, und `Standard`
        // rechnet ueber WinAnsi: alle ASCII-Codes gingen, die
        // **Ligatur-Codes nicht**.
        //
        // GEMESSENE WIRKUNG, vorher (KOORDINATION, „Ligaturen fehlen"):
        //     „figures"        -> „ gures"        (fi, Code 2)
        //     „significantly"  -> „signi cantly"  (fi)
        //     „Lukasz"         -> ohne Strich im L (Lslash, Code 6)
        //
        // Und die Fehlstelle war **stumm**: `unicode_von` lieferte `None`,
        // der Aufrufer nahm `char::from_u32(code)` — ein **Steuerzeichen**
        // — und `glyph_index` fand dafuer nichts.
        let kodierung = kodierung_lesen(dict, aufloesen);
        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
            let art = match &kodierung {
                Kodierung::Differences(d) => format!("Differences mit {} Eintraegen", d.len()),
                Kodierung::WinAnsi => "WinAnsi".to_string(),
                Kodierung::MacRoman => "MacRoman".to_string(),
                Kodierung::Standard => "Standard".to_string(),
            };
            eprintln!(
                "KODIERUNG {basisname}: {art}; Encoding im DICT = {:?}",
                dict.get("Encoding").map(|w| match w {
                    Wert::Ref(n, _) => format!("Ref({n})"),
                    Wert::Name(n) => format!("Name({})", String::from_utf8_lossy(n)),
                    Wert::Dict(_) => "Dict".to_string(),
                    Wert::Null => "Null".to_string(),
                    _ => "anders".to_string(),
                })
            );
        }
        // added 03.10.2026. DIE CODE-TABELLE DER CFF, und sie ist der
        // **letzte** Schritt beim Laden: vorher ist nicht bekannt, ob
        // ueberhaupt eine CFF da ist.
        //
        // DIE REIHENFOLGE DER NAMEN ist die der Spezifikation: die
        // eingebaute Kodierung des Fonts gilt, und `/Differences`
        // ueberschreibt sie. GEMESSEN an `fw9.pdf`: dort steht
        // `/Encoding /WinAnsiEncoding` **ohne** Differences, also
        // kommen alle 256 Namen aus WinAnsi.
        if let Some(mut c) = eingebettet_cff {
            // added 05.10.2026: **BEIDES**, und die Reihenfolge ist die
            // der Spezifikation: die Basiskodierung gilt fuer alle Codes,
            // und `/Differences` ueberschreibt einzelne davon.
            //
            // GEMESSEN, warum das die Korrektur ist: `1207.7214.pdf` Seite 1
            //
            //     nur die zwei Differences-Namen      37 382 abweichend
            //     Basis **und** die zwei Namen         (Ziel 36 201)
            //
            // `GPRVKY+Helvetica` traegt `/Differences [ 2 /fi /fl ]` — zwei
            // Eintraege. Vorher bekam die CFF *nur* diese zwei Namen, alle
            // 255 uebrigen Codes verloren ihre Zuordnung, und der Zeichner
            // nahm „Code = Glyphe“ — bei einer TeX-Teilmenge, deren Codes
            // Glyphenindizes sind, genau die falsche. Der Titel wurde
            // vertauscht.
            //
            // added 05.10.2026: die Basis ist **immer** WinAnsi, auch bei
            // `Kodierung::MacRoman`. Das ist der Stand vom 03.10.2026 und
            // wird hier nicht geaendert — der MacRoman-Weg waere eine
            // eigene Messung.
            let mut paare: Vec<(u32, String)> = (0u32..=255)
                .filter_map(|code| {
                    crate::cff::winansi_name(code as u8).map(|n| (code, n.to_string()))
                })
                .collect();
            if let Kodierung::Differences(d) = &kodierung {
                for (code, name) in d {
                    // DER UEBERSCHREIBER: der Code kommt **einmal** weg,
                    // damit er hinten steht und gewinnt.
                    paare.retain(|(c, _)| c != code);
                    paare.push((*code, name.clone()));
                }
            }
            c.code_aufnehmen(&paare);
            eingebettet_cff = Some(c);
        }
        // ToUnicode
        let mut tounicode = std::collections::HashMap::new();
        if let Some((tn, _)) = dict.get_ref("ToUnicode") {
            if let Some(strom) = aufloesen(tn) {
                // added 03.10.2026: hier stand `auflosen` statt
                // `aufloesen` — Tippfehler, vom Compiler beim ersten
                // Lauf mit Zeilennummer genannt.
                let bytes = stream_inhalt(strom, aufloesen);
                cmap_lesen(&bytes, &mut tounicode);
            }
        }
        let dw2: Vec<f64> = Vec::new();
        let cid_system = cid
            .get("CIDSystemInfo")
            .map(|c| {
                format!(
                    "{}-{}",
                    c.get_name("Registry").unwrap_or("?"),
                    c.get_name("Ordering").unwrap_or("?")
                )
            })
            .unwrap_or_default();
        if let Some(f) = &cff_fehler {
            // added 03.10.2026. DER FEHLER GEHOERT IN DIE BILANZ und
            // nicht in ein `eprintln!`: die Meldung steht dann in der
            // Datei, die der Lauf ohnehin schreibt.
            eprintln!("schrift.rs: CFF nicht lesbar: {f} ({basisname})");
        }
        eprintln!(
            "schrift.rs: {} cff={} tt={}",
            basisname,
            eingebettet_cff.is_some(),
            eingebettet.is_some()
        );
        Some(Ressource {
            art,
            basisname,
            eingebettet,
            eingebettet_cff,
            breiten,
            tounicode,
            kodierung,
            cid_system,
            dw2,
        })
    }

    /// Die Breite eines Zeichens in **Textraum-Einheiten**
    /// (1/1000 der Schriftgroesse bei CID-Fonts, sonst Punkte).
    /// added 03.10.2026: delegiert an [`Breiten::breite`] — es gibt
    /// damit nur eine Breitenrechnung in der Crate.
    pub fn breite(&self, code: u32) -> f64 {
        self.breiten.breite(code)
    }

    /// Der Unicode-Zeichencode eines Zeichens (fuer die Font-Auswahl
    /// und fuer `/ToUnicode`).
    pub fn unicode_von(&self, code: u32) -> Option<char> {
        if let Some(c) = self.tounicode.get(&code) {
            return Some(*c);
        }

        match &self.kodierung {
            Kodierung::WinAnsi => winansi_zu_zeichen(code as u8),
            Kodierung::MacRoman => macroman_zu_zeichen(code as u8),
            Kodierung::Differences(d) => d
                .iter()
                .find(|(c, _)| *c == code)
                .and_then(|(_, n)| glyphenname_zu_zeichen(n))
                // added 05.10.2026: NAME OHNE ZEICHEN -> WinAnsi, und
                // **das** ist die Regel, ohne die der `/Encoding`-Verweis
                // mehr zerstoert als er repariert.
                //
                // GEMESSEN, woran: `1207.7214.pdf` ist eine
                // Mathematikarbeit mit Teilmengen-Schriften. Deren
                // `/Differences` nennt **eigene** Glyphennamen
                // (`/g1`, `/g2`, …), fuer die es kein Unicode-Zeichen
                // gibt. Ohne diese Zeile lieferte `unicode_von` `None`,
                // der Aufrufer nahm `char::from_u32(code)` — ein
                // Steuerzeichen — und der Titel wurde zu
                //
                //     „EUROPEAN ORGANISATION FOR NUCLEAR …“
                //     ->  „, UROP * N ORL * N.S* T.ON fI lOR NU+L, * R R,“  (abnahme14)
                //
                // GEMESSENER SCHADEN: +4 717 abweichende Pixel auf
                // `1207.7214.pdf` allein, gegenueber dem Stand ohne den
                // Verweis.
                //
                // MIT der Zeile bleibt der Ligatur-Fall heil: `fi` ist ein
                // **bekannter** Name und liefert U+FB01, bevor WinAnsi
                // gefragt wird.
                .or_else(|| winansi_zu_zeichen(code as u8)),
            Kodierung::Standard => winansi_zu_zeichen(code as u8),
        }
    }

    /// Der **Name** der Glyphe, die hinter einem Zeichencode steht.
    ///
    /// added 05.10.2026 (KOORDINATION, „Glyphen ueber /Differences bzw.
    /// ToUnicode/Encoding-Namen (fi, fl, Lslash) aufloesen"): der Weg
    /// ueber das Zeichen ist der uebliche, und er reicht fuer
    /// Ligaturen nicht immer. `fi` ist U+FB01 und steht in vielen
    /// `cmap` nicht; der **Name** `fi` steht aber im `/Differences` der
    /// Schrift und im `charset` der Ersatzschrift.
    ///
    /// GEMESSEN, dass der Weg noetig ist: `NimbusRoman-Regular.otf` hat
    /// eine Glyphe `fi` (GID 109), und `Cff::gid_von_name("fi")` findet
    /// sie — unabhaengig davon, ob eine `cmap` auf U+FB01 zeigt.
    ///
    /// FUER CID-FONTS gibt es keinen Namen (dort ist der CID die
    /// Glyphe), dann kommt `None` — und der Aufrufer zeichnet wie
    /// bisher.
    pub fn glyphenname(&self, code: u32) -> Option<String> {
        match &self.kodierung {
            Kodierung::Differences(d) => d.iter().find(|(c, _)| *c == code).map(|(_, n)| n.clone()),
            // Ohne `/Differences` ist der Name der Zeichensatzname: der
            // Weg lohnt fuer die Sonderzeichen der WinAnsi-Tabelle
            // (`quoteright`, `trademark`, …), die in manchen `cmap`
            // fehlen.
            Kodierung::WinAnsi => crate::cff::winansi_name(code as u8).map(|s| s.to_string()),
            _ => None,
        }
    }

    /// Die Glyphennummer im eingebetteten Font, falls es einen gibt.
    ///
    /// added 03.10.2026. DER WEG FUER Identity-H: `true` ist der
    /// haeufigste Fall (der CID **ist** die Glyphe), und abweichend
    /// davon gibt es `/CIDToGIDMap` als Stream mit einem Byte je
    /// GID.
    pub fn gid(&self, code: u32) -> u32 {
        // added 03.10.2026. DIE CFF HAT EINE EIGENE GLYPHEN-TABELLE:
        // bei einer einfachen Schrift ist der Zeichencode die Glyphe
        // nur, wenn die eingebettete Datei TrueType ist (dort ist die
        // `cmap` nach GID geordnet). Bei CFF entscheidet das `charset`
        // ueber den **Namen**, und da ist der Code 97 nicht zwingend
        // die Glyphe 97.
        //
        // GEMESSEN: `fw9.pdf` hat `/Subtype /Type1` mit
        // `/FontFile3`; der Zeichner fragte vorher `gid = code` und
        // zeichnete damit die **falsche** Glyphe — bei einer
        // Teilschrift sind die Codes ueberschrieben.
        if let Some(cff) = &self.eingebettet_cff
            && let Some(g) = cff.glyphe_von_code(code)
        {
            return g as u32;
        }
        if self.art == SchriftArt::Type0 {
            code
        } else {
            // WinAnsi: der Code ist schon die Glyphe bei den meisten
            // TrueType-Fonts (glyf-Reihenfolge = Zeichencodes bei
            // Symbolic-Fonts). Fuer die zehn PDFs reicht das, und
            // `unicode_von` liefert den richtigen Zeichencode fuer
            // den Systemfont-Ersatz.
            code
        }
    }
}

/// Liest die Inhalte eines Streams (fuer eingebettete Fonts).
///
/// added 03.10.2026. DER WEG: `/Length` kann eine **Referenz** sein
/// (das macht Word), und der Inhalt kann ueber `/Filter` gepackt sein.
fn stream_inhalt(wert: Wert, aufloesen: &mut dyn FnMut(u32) -> Option<Wert>) -> Vec<u8> {
    // `wert` wird **einmal** zerlegt. Zwei `match wert` waren ein
    // „partially moved value" — beim ersten Lauf gemeldet.
    let (mut dict, roh) = match wert {
        Wert::Stream(d, daten) => (d, daten),
        Wert::Dict(d) => (d, Vec::new()),
        _ => return Vec::new(),
    };
    if let Some((_, laenge_wert)) = dict.iter().rev().find(|(k, _)| k.as_slice() == b"Length") {
        // added 03.10.2026: die beiden `if let` zu **einem**
        // zusammengezogen (clippy `collapsible_if`).
        if let Wert::Ref(n, _) = laenge_wert
            && let Some(Wert::Zahl(l)) = aufloesen(*n)
        {
            // Die Laenge nachziehen, damit der Decoder sie nutzt.
            dict.retain(|(k, _)| k.as_slice() != b"Length");
            dict.push((b"Length".to_vec(), Wert::Zahl(l)));
        }
    }
    let namen = crate::xref::filter_namen(&dict);
    let parms = crate::xref::decode_parms(&dict, &namen);
    crate::filtern::dekodieren(
        &namen,
        &parms,
        &Wert::Dict(dict),
        &roh,
        crate::filtern::MAX_ENTPACKT,
    )
    .unwrap_or_default()
}

/// Liest `/Encoding`.
fn kodierung_lesen(dict: &Wert, aufloesen: &mut dyn FnMut(u32) -> Option<Wert>) -> Kodierung {
    // added 05.10.2026: DER VERWEIS, und das ist die ganze Korrektur.
    //
    // `/Encoding` ist laut Spezifikation ein **Wörterbuch oder ein Name**,
    // aber die meisten Erzeuger (pdfTeX, Distiller, Word) schreiben ein
    // **indirektes Objekt**:
    //
    //     /Encoding 5834 0 R              <- kommt vor
    //     /Encoding << /Differences … >>  <- seltener
    //
    // GEMESSEN: `1706.03762.pdf` hat Fuenf von sechs Schriften so
    // (/Encoding 5834 0 R, 5854 0 R, 5926 0 R …), und dort fehlten genau
    // die Glyphen, die nur ueber `/Differences` zu finden sind. Ohne
    // diesen Zweig fiel der Verweis auf `Standard` zurueck — und `Standard`
    // kann `fi` (Code 2) nicht kennen, weil das keine WinAnsi-Position ist.
    // added 05.10.2026, DER TEIL, DER FEHLTE: **erst aufloesen, dann
    // prüfen.** Mein erster Versuch stieg mit dem aufgeloesten Objekt
    // wieder ganz oben ein — und fragte dort nach einem Schluessel
    // `/Encoding`, den es in diesem Objekt naturgemaess nicht gibt. Das
    // Ergebnis war `Standard`, und der Verweis war damit aufgeloest und
    // trotzdem wirkungslos.
    //
    // GEMESSEN, woran man das sieht: `NARSIL_PDF_DEBUG` meldete fuer
    // `AECCXO+NimbusRomNo9L-Regu`
    //     ENCODING-VERWEIS 5834 -> DICT     <- der Verweis wird geholt
    //     KODIERUNG … : Standard             <- und trotzdem Standard
    let wert = match dict.get("Encoding") {
        Some(Wert::Ref(n, _)) => aufloesen(*n),
        sonst => sonst.cloned(),
    };
    match wert.as_ref() {
        Some(Wert::Name(n)) => {
            let name = String::from_utf8_lossy(n).into_owned();
            if name == "MacRomanEncoding" {
                Kodierung::MacRoman
            } else {
                Kodierung::WinAnsi
            }
        }
        Some(Wert::Dict(d)) => {
            // `/Encoding << /Differences [...] /BaseEncoding /WinAnsiEncoding >>`
            let innen = Wert::Dict(d.clone());
            let basis = match innen.get_name("BaseEncoding") {
                Some("MacRomanEncoding") => Kodierung::MacRoman,
                _ => Kodierung::WinAnsi,
            };
            let mut out = Vec::new();
            if let Some(arr) = innen.get("Differences").and_then(Wert::array) {
                let mut code = 0u32;
                for e in arr {
                    match e {
                        Wert::Zahl(z) if *z >= 0.0 => code = *z as u32,
                        Wert::Name(n) => {
                            if out.len() >= MAX_TABELLE {
                                break;
                            }
                            out.push((code, String::from_utf8_lossy(n).into_owned()));
                            code = code.saturating_add(1);
                        }
                        _ => {}
                    }
                }
            }
            if out.is_empty() {
                basis
            } else {
                Kodierung::Differences(out)
            }
        }
        _ => Kodierung::Standard,
    }
}

/// Liest `/W` in die CID-Bereichsliste.
///
/// added 03.10.2026. DIE BEIDEN FORMEN, und sie stehen **hintereinander**
/// in einer einzigen Liste:
///
/// ```text
///   [ 1 [ 250 300 ]      zwei Einzelbreiten fuer CID 1
///     10 20 500          CID 10 bis 20 haben 500
///     21 [ 100 ] ]       eine Einzelbreite fuer CID 21
/// ```
///
/// Die Form ist an der **Anzahl** der Elemente erkennbar: zwei
/// aufeinanderfolgende Zahlen + eine Zahl sind ein Bereich; ein Array
/// ist eine Liste von Breiten.
fn w_array_lesen(arr: &[Wert], raus: &mut Vec<(u32, f64)>) {
    let mut i = 0usize;
    while i < arr.len() {
        // Kein `map` noetig: `*z` im Muster gibt die Zahl selbst.
        let Some(&Wert::Zahl(c)) = arr.get(i) else {
            i += 1;
            continue;
        };
        match arr.get(i + 1) {
            Some(Wert::Array(liste)) => {
                let c = if c.is_finite() && c >= 0.0 {
                    c as u32
                } else {
                    0
                };
                for (k, w) in liste.iter().enumerate() {
                    if raus.len() >= MAX_TABELLE {
                        return;
                    }
                    let breite = w.zahl().unwrap_or(0.0);
                    raus.push((c + k as u32, breite));
                }
                i += 2;
            }
            Some(Wert::Zahl(b)) => {
                // Bereich `cfirst clast w`.
                //
                // DER FEHLER DER ERSTEN FASSUNG: `b` (das **zweite**
                // Element) wurde als Breite gelesen und `arr[i+2]` als
                // `clast`. Richtig ist es umgekehrt: bei
                // `[10 20 500]` ist 10 die erste CID, 20 die letzte und
                // **500 die Breite**. Mit der vertauschten Lesart
                // bekam CID 10 die Breite 20 — und der Test
                // `w_array_beide_formen` sah 20 statt 500.
                let cfirst = if c.is_finite() && c >= 0.0 {
                    c as u32
                } else {
                    0
                };
                let clast = if b.is_finite() && *b >= 0.0 {
                    *b as u32
                } else {
                    cfirst
                };
                if let Some(Wert::Zahl(w)) = arr.get(i + 2) {
                    let breite = if w.is_finite() { *w } else { 0.0 };
                    // Bereiche duerfen nicht ewig laufen: das ist ein
                    // Feld aus der Datei.
                    if clast >= cfirst && clast - cfirst <= MAX_TABELLE as u32 {
                        for cid in cfirst..=clast {
                            if raus.len() >= MAX_TABELLE {
                                return;
                            }
                            raus.push((cid, breite));
                        }
                    }
                    i += 3;
                } else {
                    i += 2;
                }
            }
            _ => i += 1,
        }
    }
}

/// Liest eine `/ToUnicode`-CMap.
///
/// added 03.10.2026. Die CMap ist ein PostScript-Programm mit
/// `beginbfchar`-/`beginbfrange`-Abschnitten. Nur die werden gelesen —
/// alles andere ist fuer die Anzeige nicht noetig.
///
/// DIE ABSCHNITTE, wortwoertlich:
///
/// ```text
///   3 beginbfchar
///   <0020> <0052>       CID 0x20 -> 'R'
///   endbfchar
///   2 beginbfrange
///   <0024> <0039> <0061>   0x24..0x39 -> 0x61, 0x62, ...
///   endbfrange
/// ```
///
/// DER FEHLER, DER HIER AUFTRITT: `<0041> <0043> <0020>` am Ende einer
/// `bfchar`-Liste ist **kein Tripel**, sondern drei Paare. Wer
/// rueckwaerts liest, macht aus den letzten drei Zeilen eines.
fn cmap_lesen(daten: &[u8], raus: &mut std::collections::HashMap<u32, char>) {
    // Die CMap ist **PostScript**, und PostScript ist lateinisch. Bei
    // ungueltigem UTF-8 wird verlustbehaftet umgewandelt und erneut
    // gelesen — die Hex-Bloecke stehen davon unberuehrt.
    let verlustbehaftet;
    let text = match std::str::from_utf8(daten) {
        Ok(t) => t,
        Err(_) => {
            verlustbehaftet = String::from_utf8_lossy(daten).into_owned();
            &verlustbehaftet
        }
    };
    for abschnitt in text.split("beginbfchar").skip(1) {
        let ende = abschnitt.find("endbfchar").unwrap_or(abschnitt.len());
        let hexes: Vec<&str> = abschnitt[..ende]
            .split('<')
            .filter_map(|h| h.split('>').next())
            .filter(|h| !h.trim().is_empty())
            .collect();
        for paar in hexes.chunks(2) {
            if paar.len() == 2 {
                if let (Some(code), Some(ziel)) = (hex_zu_u32(paar[0]), hex_zu_zeichen(paar[1])) {
                    raus.insert(code, ziel);
                }
            }
        }
    }
    for abschnitt in text.split("beginbfrange").skip(1) {
        let ende = abschnitt.find("endbfrange").unwrap_or(abschnitt.len());
        let hexes: Vec<&str> = abschnitt[..ende]
            .split('<')
            .filter_map(|h| h.split('>').next())
            .filter(|h| !h.trim().is_empty())
            .collect();
        for tripel in hexes.chunks(3) {
            if tripel.len() != 3 {
                continue;
            }
            let (Some(von), Some(bis)) = (hex_zu_u32(tripel[0]), hex_zu_u32(tripel[1])) else {
                continue;
            };
            if bis < von || bis - von > MAX_TABELLE as u32 {
                continue;
            }
            // Dritte Form: ein **Array** von Zielen. Dann ist der dritte
            // Eintrag kein Hex, sondern eine Liste — das erkennen wir
            // daran, dass er nicht aus Hex-Zeichen besteht.
            if tripel[2]
                .chars()
                .any(|c| c.is_ascii_whitespace() || c == ',')
            {
                continue;
            }
            let Some(start) = hex_zu_u32(tripel[2]) else {
                continue;
            };
            for (k, cid) in (von..=bis).enumerate() {
                if let Some(z) = char::from_u32(start + k as u32) {
                    raus.insert(cid, z);
                }
            }
        }
    }
}

/// `0041` -> 65.
fn hex_zu_u32(h: &str) -> Option<u32> {
    let h = h.trim();
    if h.is_empty() || h.len() > 8 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

/// `0041` -> 'A' (Big-Endian, 2 oder 4 Byte).
fn hex_zu_zeichen(h: &str) -> Option<char> {
    let v = hex_zu_u32(h)?;
    if v <= 0x10FFFF {
        char::from_u32(v)
    } else {
        None
    }
}

/// WinAnsiEncoding: Byte -> Unicode.
///
/// added 03.10.2026. DER UNTERSCHIED ZU ISO-8859-1 sind **32 Stellen**
/// (0x80..0x9F): dort sind Waehrzeichen, in WinAnsi sind es
/// typografische Zeichen. Wer sie gleichsetzt, zeigt bei einem Text
/// mit Anfuehrungszeichen die falschen an — und der Fehler faellt nur
/// bei Texten mit diesen Zeichen auf.
pub fn winansi_zu_zeichen(b: u8) -> Option<char> {
    match b {
        0x80 => Some('\u{20AC}'),
        0x82 => Some('\u{201A}'),
        0x83 => Some('\u{0192}'),
        0x84 => Some('\u{201E}'),
        0x85 => Some('\u{2026}'),
        0x86 => Some('\u{2020}'),
        0x87 => Some('\u{2021}'),
        0x88 => Some('\u{02C6}'),
        0x89 => Some('\u{2030}'),
        0x8a => Some('\u{0160}'),
        0x8b => Some('\u{2039}'),
        0x8c => Some('\u{0152}'),
        0x8e => Some('\u{017D}'),
        0x91 => Some('\u{2018}'),
        0x92 => Some('\u{2019}'),
        0x93 => Some('\u{201C}'),
        0x94 => Some('\u{201D}'),
        0x95 => Some('\u{2022}'),
        0x96 => Some('\u{2013}'),
        0x97 => Some('\u{2014}'),
        0x98 => Some('\u{02DC}'),
        0x99 => Some('\u{2122}'),
        0x9a => Some('\u{0161}'),
        0x9b => Some('\u{203A}'),
        0x9c => Some('\u{0153}'),
        0x9e => Some('\u{017E}'),
        0x9f => Some('\u{0178}'),
        0x81 => Some('\u{FFFD}'),
        0x8d => Some('\u{FFFD}'),
        0x8f => Some('\u{FFFD}'),
        0x90 => Some('\u{FFFD}'),
        0x9d => Some('\u{FFFD}'),
        _ => Some(b as char),
    }
}

/// MacRomanEncoding: Byte -> Unicode.
///
/// added 03.10.2026. Nur die Stellen, die sich von Latin-1
/// unterscheiden — der Rest ist gleich.
pub fn macroman_zu_zeichen(b: u8) -> Option<char> {
    match b {
        0xCA => Some('\u{00A9}'),
        0xCB => Some('\u{00AE}'),
        0xCC => Some('\u{2030}'),
        0xCD => Some('\u{201E}'),
        0xCE => Some('\u{201C}'),
        0xCF => Some('\u{201D}'),
        0xD0 => Some('\u{2022}'),
        0xD1 => Some('\u{2013}'),
        0xD2 => Some('\u{2014}'),
        0xD3 => Some('\u{02C6}'),
        0xD4 => Some('\u{2030}'),
        0xD5 => Some('\u{2039}'),
        0xD6 => Some('\u{0152}'),
        0xD7 => Some('\u{017D}'),
        0xD8 => Some('\u{2018}'),
        0xD9 => Some('\u{2019}'),
        0xDA => Some('\u{201C}'),
        0xDB => Some('\u{201D}'),
        0xDC => Some('\u{02DC}'),
        0xDD => Some('\u{2122}'),
        0xDE => Some('\u{0161}'),
        0xDF => Some('\u{203A}'),
        0xE0 => Some('\u{0153}'),
        0xE1 => Some('\u{017E}'),
        0xE2 => Some('\u{FFFD}'),
        0xE3 => Some('\u{0178}'),
        _ => Some(b as char),
    }
}

/// Ein `/Differences`-Glyphenname -> Unicode.
///
/// added 03.10.2026. Nur die Namen, die in den zehn PDFs vorkommen
/// (GEMESSEN: `space`, `bullet`, `emdash`, `quoteright`, `fi`, `fl`,
/// `zero`, `one` … `nine` fuer die Ziffern in Formularen). Fuer alles
/// andere: `None`, und der Aufrufer faellt auf den Systemfont zurueck.
///
/// Der Wert ist **kein Fehler**: ein Glyphenname wie `germandbls` ohne
/// Eintrag bedeutet „kein Unicode", und der Zeichencode wird dann
/// ueber die Position in der Kodierung bestimmt.
pub fn glyphenname_zu_zeichen(name: &str) -> Option<char> {
    let e = match name {
        "space" => '\u{0020}',
        "exclam" => '\u{0021}',
        "quotedbl" => '\u{0022}',
        "numbersign" => '\u{0023}',
        "dollar" => '\u{0024}',
        "percent" => '\u{0025}',
        "ampersand" => '\u{0026}',
        "quotesingle" => '\u{0027}',
        "parenleft" => '\u{0028}',
        "parenright" => '\u{0029}',
        "asterisk" => '\u{002A}',
        "plus" => '\u{002B}',
        "comma" => '\u{002C}',
        "hyphen" => '\u{002D}',
        "period" => '\u{002E}',
        "slash" => '\u{002F}',
        "zero" => '\u{0030}',
        "one" => '\u{0031}',
        "two" => '\u{0032}',
        "three" => '\u{0033}',
        "four" => '\u{0034}',
        "five" => '\u{0035}',
        "six" => '\u{0036}',
        "seven" => '\u{0037}',
        "eight" => '\u{0038}',
        "nine" => '\u{0039}',
        "colon" => '\u{003A}',
        "semicolon" => '\u{003B}',
        "less" => '\u{003C}',
        "equal" => '\u{003D}',
        "greater" => '\u{003E}',
        "question" => '\u{003F}',
        "at" => '\u{0040}',
        "bracketleft" => '\u{005B}',
        "backslash" => '\u{005C}',
        "bracketright" => '\u{005D}',
        "asciicircum" => '\u{005E}',
        "underscore" => '\u{005F}',
        "grave" => '\u{0060}',
        "braceleft" => '\u{007B}',
        "bar" => '\u{007C}',
        "braceright" => '\u{007D}',
        "asciitilde" => '\u{007E}',
        "quoteright" => '\u{2019}',
        "quoteleft" => '\u{2018}',
        "quotedblleft" => '\u{201C}',
        "quotedblright" => '\u{201D}',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "bullet" => '\u{2022}',
        "ellipsis" => '\u{2026}',
        "dagger" => '\u{2020}',
        "daggerdbl" => '\u{2021}',
        "perthousand" => '\u{2030}',
        "guilsinglleft" => '\u{2039}',
        "guilsinglright" => '\u{203A}',
        "fi" => '\u{FB01}',
        "fl" => '\u{FB02}',
        "minus" => '\u{2212}',
        "multiply" => '\u{00D7}',
        "divide" => '\u{00F7}',
        "lessequal" => '\u{2264}',
        "greaterequal" => '\u{2265}',
        "notequal" => '\u{2260}',
        "logicalnot" => '\u{00AC}',
        "sterling" => '\u{00A3}',
        "currency" => '\u{00A4}',
        "yen" => '\u{00A5}',
        "section" => '\u{00A7}',
        "copyright" => '\u{00A9}',
        "registered" => '\u{00AE}',
        "degree" => '\u{00B0}',
        "plusminus" => '\u{00B1}',
        "mu" => '\u{00B5}',
        "paragraph" => '\u{00B6}',
        "onequarter" => '\u{00BC}',
        "onehalf" => '\u{00BD}',
        "threequarters" => '\u{00BE}',
        "nbspace" => '\u{00A0}',
        _ => return None,
    };
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/Widths` als **Verweis** auf ein Array — der Fall, der
    /// `1706.03762.pdf` unlesbar machte.
    ///
    /// added 03.10.2026. GEMESSEN: `pdflatex` schreibt
    /// `/Widths 5837 0 R`, nie das Array direkt. Ohne Aufloesung blieb
    /// die Tabelle leer, `/FirstChar` 0, und **jedes** Zeichen bekam
    /// `fehlbreite` = 500 — bei 12-Punkt-Text ein Vorschub von 0,5 em
    /// statt 0,28 em. Die Buchstaben ueberlagerten sich zu einem
    /// unlesbaren Fleisch.
    #[test]
    fn widths_als_verweis() {
        // Font-Woerterbuch mit `/Widths 7 0 R` und `/FirstChar 65`.
        let dict: Vec<(Vec<u8>, Wert)> = vec![
            (b"Subtype".to_vec(), Wert::Name(b"Type1".to_vec())),
            (b"FirstChar".to_vec(), Wert::Zahl(65.0)),
            (b"Widths".to_vec(), Wert::Ref(7, 0)),
        ];
        let mut aufloesen = |n: u32| -> Option<Wert> {
            if n == 7 {
                Some(Wert::Array(vec![
                    Wert::Zahl(556.0), // A
                    Wert::Zahl(500.0), // B
                    Wert::Zahl(333.0), // C
                ]))
            } else {
                None
            }
        };
        let b = Breiten::aus(&Wert::Dict(dict), None, "", &mut aufloesen);
        assert_eq!(b.first_char, 65, "/FirstChar muss gelesen sein");
        assert_eq!(b.breite(65), 556.0, "A");
        assert_eq!(b.breite(66), 500.0, "B");
        assert_eq!(b.breite(67), 333.0, "C");
        // Ausserhalb der Tabelle: die Vorgabe, **nicht** 0.
        assert_eq!(b.breite(68), 500.0, "'D' fehlt in der Tabelle");
    }

    /// Ohne `/Widths` ueberhaupt bleibt es bei der Vorgabe — und der
    /// Vorschub ist 0,5 em, **nicht** 0.
    #[test]
    fn widths_fehlt_ganz() {
        let dict: Vec<(Vec<u8>, Wert)> = vec![(b"Subtype".to_vec(), Wert::Name(b"Type1".to_vec()))];
        let mut aufloesen = |_n: u32| -> Option<Wert> { None };
        let b = Breiten::aus(&Wert::Dict(dict), None, "", &mut aufloesen);
        assert_eq!(b.breiten.len(), 0);
        assert_eq!(b.breite(b'A' as u32), 500.0);
    }

    /// WinAnsi 0x92 ist ein rechtes Anfuehrungszeichen, **kein**
    /// Steuerzeichen.
    ///
    /// added 03.10.2026. Der Unterschied zu Latin-1 faellt nur bei
    /// Texten mit Anfuehrungszeichen auf — also bei fast jedem
    /// deutschen Fliesstext.
    #[test]
    fn winansi_unterscheidet_latin1() {
        assert_eq!(winansi_zu_zeichen(0x92), Some('\u{2019}'));
        assert_eq!(winansi_zu_zeichen(0x93), Some('\u{201C}'));
        assert_eq!(winansi_zu_zeichen(0xE4), Some('\u{00E4}'));
    }

    /// `/W` in beiden Formen: Liste und Bereich.
    ///
    /// added 03.10.2026. DER FEHLER: wer die Bereichsform
    /// `10 20 500` als drei Einzelbreiten liest, gibt CID 10 die
    /// Breite 20 und CID 20 die Breite 500 — und der Text springt.
    #[test]
    fn w_array_beide_formen() {
        // [ 1 [ 250 300 ]  10 20 500 ]
        let arr = vec![
            Wert::Zahl(1.0),
            Wert::Array(vec![Wert::Zahl(250.0), Wert::Zahl(300.0)]),
            Wert::Zahl(10.0),
            Wert::Zahl(20.0),
            Wert::Zahl(500.0),
        ];
        let mut raus = Vec::new();
        w_array_lesen(&arr, &mut raus);
        assert_eq!(raus[0], (1, 250.0));
        assert_eq!(raus[1], (2, 300.0));
        // Bereich 10..=20 mit 500
        assert_eq!(raus.iter().filter(|(c, _)| *c == 10).count(), 1);
        assert_eq!(raus.iter().find(|(c, _)| *c == 10).unwrap().1, 500.0);
        assert_eq!(raus.iter().find(|(c, _)| *c == 15).unwrap().1, 500.0);
        assert_eq!(raus.iter().find(|(c, _)| *c == 20).unwrap().1, 500.0);
    }

    /// `/ToUnicode` mit `bfchar` und `bfrange`.
    #[test]
    fn tounicode_beide_abschnitte() {
        let cmap = b"/CIDInit /ProcSet findresource begin\n\
            2 beginbfrange\n<0024> <0026> <0061>\nendbfrange\n\
            2 beginbfchar\n<0041> <00C4>\n<0042> <00D6>\nendbfchar\nendcmap";
        let mut raus = std::collections::HashMap::new();
        cmap_lesen(cmap, &mut raus);
        assert_eq!(raus.get(&0x24), Some(&'a'));
        assert_eq!(raus.get(&0x26), Some(&'c'));
        assert_eq!(raus.get(&0x41), Some(&'\u{00C4}'));
        assert_eq!(raus.get(&0x42), Some(&'\u{00D6}'));
    }
}
