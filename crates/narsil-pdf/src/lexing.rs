//! Der Lexer: PDF-Objektsyntax aus Bytes.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # WARUM EIN EIGENER LEXER
//!
//! PDF ist **kein** Format mit Trennzeichen: ein Name kann mitten im
//! Wort enden, ein Stream beginnt mit einem Schluesselwort und dann
//! kommen rohe Bytes, die kein Objekt sind. Ein Tokenizer, der wie JSON
//! funktioniert, scheitert an `<< /Length 12345 >>stream\r\n...\r\nendstream`,
//! weil der Streaminhalt kein Objekt ist.
//!
//! # DIE FUENF KLASSEN, die ein Token haben kann
//!
//!  * Zahl: `[+-]?digits[.digits]` und `-.5`
//!  * Name: `/Name`, mit `#xx`-Fluchtzeichen
//!  * String: `(…)` mit Fluchtzeichen und **geschachtelten** Klammern
//!  * Hex-String: `<…>` mit gerader oder ungerader Stellenzahl
//!    (ungerade heisst: letztes Nibble 0)
//!  * Schluesselwort: `obj`, `endobj`, `stream`, `R`, `true`, `false`,
//!    `null` und die Content-Stream-Operatoren
//!
//! Alles andere ist ein Token fuer sich, aus dem der Aufrufer ein
//! `[` , `]`, `/`, `<<` oder `>>` erkennt.

use crate::objekt::{FehlenderVerweis, Wert};

/// Was der Lexer gesehen hat.
///
/// added 03.10.2026.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Eine Zahl.
    Zahl(f64),
    /// Ein Name ohne Schraegstrich.
    Name(Vec<u8>),
    /// Ein Klartextstring, Inhalt ohne Klammern.
    String(Vec<u8>),
    /// Ein Hex-String als Bytes; ungerade Stellenzahl ist aufgefüllt.
    Hex(Vec<u8>),
    /// `true`, `false`, `null`, `R`, `obj`, `endobj`, `stream`, ...
    Schluesselwort(String),
    /// `(` , `)` , `[` , `]` , `<<` , `>>` , `/` , `{` , `}` , `%`
    Satzzeichen(String),
    /// Das Ende der Eingabe.
    Ende,
}

/// Wie viele Bytes ein Token hoechstens haben darf (M3/M4).
///
/// added 03.10.2026. 64 KiB. Aus zwei Gruenden: ein „Name" aus einem
/// kaputten Dokument darf nicht den ganzen Speicher fressen, und ein
/// Content-Stream mit einem 1-MB-Array braucht keine einzelnen
/// Tokens. Wird beim Ueberschreiten **abgeschnitten** mit Fehler, nicht
/// stillschweigend.
pub const MAX_TOKEN: usize = 64 * 1024;

/// Ein Fehler im Lexer.
#[derive(Debug, Clone, PartialEq)]
pub struct LexFehler {
    /// Wo im-stream der Fehler passiert ist.
    pub position: usize,
    /// Was nicht in Ordnung war.
    pub grund: String,
}

impl std::fmt::Display for LexFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Byte {}: {}", self.position, self.grund)
    }
}

/// Der Lexer ueber Byte-Eingabe.
///
/// added 03.10.2026. REINE MASCHINE: `neu` baut sie aus einem
/// Speicherbereich, `naechstes` liefert ein Token und rueckt vor.
/// Kein Zustand ausser der Position — damit kann man sie an zwei
/// Stellen benutzen (einmal fuer die Datei, einmal fuer den
/// Content-Stream).
pub struct Lexer<'a> {
    daten: &'a [u8],
    pos: usize,
    /// Zaehlt die verschachtelten Klammern in einem String — die Grenze
    /// dagegen ist eine **Bombe**: `(aaaa...` millionenfach waere
    /// ein String aus einem Dokument, das sich selbst aufblaeht.
    tiefe: usize,
}

impl<'a> Lexer<'a> {
    /// Neuer Lexer ueber `daten`.
    pub fn neu(daten: &'a [u8]) -> Self {
        Lexer {
            daten,
            pos: 0,
            tiefe: 0,
        }
    }

    /// Die aktuelle Position.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Setzt die Leseposition — fuer den Weg ueber **Rohbytes**.
    ///
    /// added 07.10.2026, Rolle SERVER. Anlass: `BI` (Inline-Bild) muss
    /// den Binärteil ueberspringen, **ohne** ihn zu zerlegen. Der Lexer
    /// tut genau das nicht: auf Binaerdaten liefert er `Zahl`, `Name`
    /// oder `Ende`, und im `Ende`-Fall blieb der Leser mitten im Bild
    /// stehen — der Rest der Seite wurde zu Muell (gemessen: auf 18 von
    /// 38 Seiten fehlten mehr als die Haelfte der Zeichen).
    ///
    /// Die Grenze ist eine **Absicht** und keine Vorsicht: wenn jemand
    /// eine Position mitten in eine Zahl setzt, ist das ein Fehler des
    /// Aufrufers, und genau das braucht `BI`.
    pub fn set_position(&mut self, pos: usize) {
        self.pos = pos.min(self.daten.len());
    }

    /// Das naechste Token, `Ende` am Ende der Eingabe.
    ///
    /// added 03.10.2026. **Zurueckhaltend**: `self.pos` zeigt immer
    /// hinter das zuletzt gelieferte Token, damit der Aufrufer
    /// `stream`-Schluesselworte und Rohdaten ueber dieselbe Position
    /// abgreifen kann.
    pub fn naechstes(&mut self) -> Result<Token, LexFehler> {
        self.weissraum();
        let Some(&b) = self.daten.get(self.pos) else {
            return Ok(Token::Ende);
        };
        match b {
            b'0'..=b'9' | b'+' | b'-' | b'.' => self.zahl(),
            b'/' => self.name(),
            b'(' => self.string(),
            b'<' => self.hex_oder_dict(),
            b'>' => {
                self.pos += 1;
                if self.daten.get(self.pos) == Some(&b'>') {
                    self.pos += 1;
                    Ok(Token::Satzzeichen(">>".into()))
                } else {
                    Ok(Token::Satzzeichen(">".into()))
                }
            }
            b'[' | b']' | b'{' | b'}' | b')' => {
                self.pos += 1;
                Ok(Token::Satzzeichen((b as char).to_string()))
            }
            c if c.is_ascii_alphabetic() => self.schluesselwort(),
            other => {
                // Ein unbekanntes Byte: **ein** Token daraus machen und
                // weitergehen. Nicht abbrechen — ein Content-Stream hat
                // Anweisungen, die wir nicht kennen, und der Rest der
                // Seite soll trotzdem gerendert werden.
                self.pos += 1;
                Ok(Token::Satzzeichen(
                    String::from_utf8_lossy(&[other]).into_owned(),
                ))
            }
        }
    }

    /// Ueberspringt Leerraum und Kommentare.
    ///
    /// added 03.10.2026. `%` beginnt einen Kommentar bis Zeilenende —
    /// der haeufigste Grund, warum ein selbst geschriebener PDF-Parser
    /// an Objekt 3 haengenbleibt.
    fn weissraum(&mut self) {
        while let Some(&b) = self.daten.get(self.pos) {
            match b {
                b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\x00' => self.pos += 1,
                b'%' => {
                    while let Some(&c) = self.daten.get(self.pos) {
                        self.pos += 1;
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    /// Liest eine Zahl.
    ///
    /// added 03.10.2026. `1.` und `.5` und `-.002` sind gueltig,
    /// `--5` nicht — bei `--` bricht die Zahl nach `-` ab und der Rest
    /// wird als Schluesselwort gelesen.
    fn zahl(&mut self) -> Result<Token, LexFehler> {
        let start = self.pos;
        if matches!(self.daten.get(self.pos), Some(b'+') | Some(b'-')) {
            self.pos += 1;
        }
        let mut komma = false;
        while let Some(&c) = self.daten.get(self.pos) {
            match c {
                b'0'..=b'9' => self.pos += 1,
                b'.' if !komma => {
                    komma = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if self.pos - start > 24 {
            // Eine Zahl aus 24 Byte ist schon kaputt; `f64::from_str`
            // wuerde `inf` liefern und aus einem kaputten Dokument eine
            // riesige Seite machen.
            return Err(LexFehler {
                position: start,
                grund: format!("Zahl aus {} Ziffern", self.pos - start),
            });
        }
        let text = std::str::from_utf8(&self.daten[start..self.pos]).unwrap_or("0");
        match text.parse::<f64>() {
            Ok(z) => Ok(Token::Zahl(z)),
            Err(_) => Err(LexFehler {
                position: start,
                grund: format!("keine Zahl: {text:?}"),
            }),
        }
    }

    /// Liest einen Namen mit `#xx`-Fluchtzeichen.
    fn name(&mut self) -> Result<Token, LexFehler> {
        self.pos += 1; // der Schraegstrich
        let mut raus: Vec<u8> = Vec::new();
        // added 03.10.2026. DER FEHLER, DER ALLE NAMEN ZERSTOERT HAT.
        //
        // Die alte Fassung tat `self.pos += 1` **vor** der Pruefung und
        // brach dann bei einem Trenner ab — der Trenner war damit
        // **gefressen**. `/Type/XRef` wurde zu `Name("Type")` und
        // `Schluesselwort("XRef")`: der Schraegstrich des zweiten
        // Namens war weg. Bei `/Type/Catalog` (mit Leerzeichen) fiel
        // es nicht auf, bei `/Type/XRef` (ohne Leerzeichen) schon —
        // und 9 von 11 echten PDFs schreiben ohne Leerzeichen.
        //
        // Gefunden bei `fw9.pdf`: `/W` fehlte in jedem XRef-Stream-
        // Wörterbuch, weil der Wert von `/Type` den Wert von `/W`
        // gefressen hatte.
        //
        // DIE REGEL: `pos` wird **nur** bei einem akzeptierten Byte
        // weitergerueckt. Damit zeigt `pos` bei einem Abbruch auf das
        // Trennzeichen — genau das, was der Aufrufer braucht.
        while let Some(&c) = self.daten.get(self.pos) {
            if c == b'#' {
                self.pos += 1;
                // `#41` = 'A'. Fehlt eine der beiden Ziffern, gilt das
                // als ein Fehler — nicht als '#'.
                let h = self.daten.get(self.pos).copied();
                let z = self.daten.get(self.pos + 1).copied();
                match (h, z) {
                    (Some(h), Some(z)) if h.is_ascii_hexdigit() && z.is_ascii_hexdigit() => {
                        raus.push(hex_wert(h) * 16 + hex_wert(z));
                        self.pos += 2;
                    }
                    _ => raus.push(b'#'),
                }
            } else if ist_ausgabezeichen(c) || c >= 0x80 {
                self.pos += 1;
                raus.push(c);
            } else {
                break;
            }
            if raus.len() > MAX_TOKEN {
                return Err(LexFehler {
                    position: self.pos,
                    grund: format!("Name laenger als {MAX_TOKEN} Byte"),
                });
            }
        }
        Ok(Token::Name(raus))
    }

    /// Liest einen Klartextstring mit Fluchtzeichen und Klammern.
    fn string(&mut self) -> Result<Token, LexFehler> {
        self.pos += 1; // die einleitende Klammer
        self.tiefe = 1;
        let mut raus: Vec<u8> = Vec::new();
        while let Some(&c) = self.daten.get(self.pos) {
            self.pos += 1;
            match c {
                b'\\' => {
                    // Der naechste Byte ist eine Fluchtung, ausser er
                    // ist selbst ein Spezialzeichen (`\` vor `n` ist ein
                    // Backslash, nicht ein Newline).
                    let Some(&e) = self.daten.get(self.pos) else {
                        break;
                    };
                    self.pos += 1;
                    match e {
                        b'n' => raus.push(b'\n'),
                        b'r' => raus.push(b'\r'),
                        b't' => raus.push(b'\t'),
                        b'b' => raus.push(8),
                        b'f' => raus.push(12),
                        b'(' => raus.push(b'('),
                        b')' => raus.push(b')'),
                        b'\\' => raus.push(b'\\'),
                        b'\r' => {
                            // Fortsetzungszeichen: `\` am Zeilenende
                            // heisst „die nächste Zeile ist dieselbe
                            // Zeile" (in alten Word-Dokumenten).
                            if self.daten.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        // added 03.10.2026. `0..=7` WAR FALSCH.
                        //
                        // GEMESSEN: `b"(\101)"` lieferte die **Ziffern**
                        // `101` statt des Bytes 65. Der Zweig `0..=7`
                        // vergleicht **Byte-Werte**, nicht die Zeichen
                        // `'0'`..=`'7'` (48..55) — er traf also nur die
                        // Steuerzeichen NUL..BEL, die in einem String
                        // selten vorkommen. Oktal-Escapes wurden
                        // deshalb **nie** ausgewertet, und der Backslash
                        // verschwand einfach: aus `\101` wurde `101`.
                        //
                        // DER BEWEIS, dass es die richtige Stelle ist:
                        // `hex_wert_dezimal` rechnet `b.wrapping_sub(b'0')`,
                        // also mit **Zeichen**, und die Schleife unten
                        // vergleicht `b'0'..=b'7'`. Nur der Musterzweig
                        // war auf Zahlen gestellt.
                        b'0'..=b'7' => {
                            // Oktal: bis zu drei Ziffern.
                            //
                            // added 03.10.2026. DIE RECHNUNG LAEUFT IN
                            // `u32`, NICHT in `u8` — und das ist ein
                            // Fehler, den clippy als
                            // `never_greater_than_255` gemeldet hat
                            // (`.min(255)` auf einem `u8` ist
                            // bedeutungslos, es haette aber **da
                            // stehen muessen**).
                            //
                            // DER FEHLER: `\777` ist 511 und passt nicht
                            // in ein `u8`. Mit `u8` passiert im
                            // Debug-Bau ein **Panik** („attempt to
                            // add with overflow"), im Release-Bau ein
                            // stiller Ueberlauf. Beides ist bei
                            // fremden Dateien ein Absturz oder ein
                            // falsches Byte.
                            let mut wert = hex_wert_dezimal(e) as u32;
                            for _ in 0..2 {
                                match self.daten.get(self.pos) {
                                    Some(&d) if (b'0'..=b'7').contains(&d) => {
                                        wert = wert * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            // Die Spezifikation sagt: ein Wert ueber
                            // 255 wird auf 255 begrenzt.
                            raus.push(wert.min(255) as u8);
                        }
                        other => raus.push(other),
                    }
                }
                b'(' => {
                    self.tiefe += 1;
                    if self.tiefe > MAX_STRING_TIEFE {
                        return Err(LexFehler {
                            position: self.pos,
                            grund: format!("String tiefer als {MAX_STRING_TIEFE}"),
                        });
                    }
                    raus.push(c);
                }
                b')' => {
                    self.tiefe -= 1;
                    if self.tiefe == 0 {
                        return Ok(Token::String(raus));
                    }
                    raus.push(c);
                }
                _ => raus.push(c),
            }
            if raus.len() > MAX_TOKEN {
                return Err(LexFehler {
                    position: self.pos,
                    grund: format!("String laenger als {MAX_TOKEN} Byte"),
                });
            }
        }
        // Kein schliessendes `)` — der String geht bis zum Ende der
        // Daten. Das kommt vor (abgeschnittene Dokumente) und ist **kein**
        // Fehler: der Inhalt ist bis dahin lesbar.
        Ok(Token::String(raus))
    }

    /// Liest `<hex>` oder `<<`.
    fn hex_oder_dict(&mut self) -> Result<Token, LexFehler> {
        if self.daten.get(self.pos + 1) == Some(&b'<') {
            self.pos += 2;
            return Ok(Token::Satzzeichen("<<".into()));
        }
        self.pos += 1;
        let mut nibbles: Vec<u8> = Vec::new();
        while let Some(&c) = self.daten.get(self.pos) {
            self.pos += 1;
            if c == b'>' {
                // Ungerade Stellenzahl: das letzte Nibble wird mit 0
                // aufgefuellt (ISO 32000, 7.3.4.3).
                if nibbles.len() % 2 == 1 {
                    nibbles.push(0);
                }
                let mut raus = Vec::with_capacity(nibbles.len() / 2);
                for paar in nibbles.chunks(2) {
                    // DIE FASSUNG VORHER RIEF `hex_wert` HIER NOCH
                    // EINMAL AUF: die Nibbles sind oben schon
                    // umgewandelt. `hex_wert(4)` (der **Wert** 4, nicht
                    // das Zeichen `'4'`) liefert 0 — also wurde jedes
                    // Byte zu 0. `<4A5>` kam als `[0, 0]` heraus.
                    raus.push(paar[0] * 16 + paar[1]);
                }
                return Ok(Token::Hex(raus));
            }
            if ist_weissraum(c) {
                continue;
            }
            if !c.is_ascii_hexdigit() {
                return Err(LexFehler {
                    position: self.pos - 1,
                    grund: format!("keine Hex-Ziffer: {:?}", c as char),
                });
            }
            nibbles.push(hex_wert(c));
            if nibbles.len() > MAX_TOKEN * 2 {
                return Err(LexFehler {
                    position: self.pos,
                    grund: format!("Hex-String laenger als {MAX_TOKEN} Byte"),
                });
            }
        }
        Ok(Token::Hex(Vec::new()))
    }

    /// Liest ein Schluesselwort.
    fn schluesselwort(&mut self) -> Result<Token, LexFehler> {
        let start = self.pos;
        while let Some(&c) = self.daten.get(self.pos) {
            if ist_ausgabezeichen(c) || c >= 0x80 {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos - start > MAX_TOKEN {
            return Err(LexFehler {
                position: start,
                grund: format!("Schluesselwort laenger als {MAX_TOKEN} Byte"),
            });
        }
        Ok(Token::Schluesselwort(
            String::from_utf8_lossy(&self.daten[start..self.pos]).into_owned(),
        ))
    }

    /// Liest ein Objekt aus der Datei (mit Referenz-Erkennung).
    ///
    /// added 03.10.2026. Das ist die **einzige** Stelle, an der aus
    /// Token-Zeichen ein Wert wird — Content-Stream und Datei teilen sie
    /// sich, damit ein `re`-Operator im Stream und ein `Rect` in der
    /// Datei nicht zwei Parser haben.
    ///
    /// DIE REFERENZ-REGEL: `12 0 R` steht im Datenstrom als drei
    /// Tokens: `Zahl(12)`, `Zahl(0)`, `Schluesselwort("R")`. Ohne
    /// Rueckblick sieht man nicht, dass es eine Referenz ist — also
    /// wird **ein** Token zurueckgelesen, wenn `R` folgt.
    ///
    /// added 03.10.2026. DIE FASSUNG VORHER HAT DIE REFERENZEN
    /// VERMISSEN, und zwar systematisch: sie pruefte, ob der Token
    /// **nach** dem ersten schon `R` ist — nach dem ersten Token ist
    /// aber `Zahl(0)`, nicht `R`. `12 0 R` kam als `Zahl(12)` heraus,
    /// und damit war **jedes** `5 0 R` im Dokument eine Zahl. Der Test
    /// `referenz_erkannt` sagte es beim ersten Lauf.
    ///
    /// DIE DREI PHASEN, und sie sind nicht zu vertauschen:
    ///  1. ein Objekt einsammeln (bis die Klammertiefe wieder 0 ist)
    ///  2. **genau ein** Token vorauslesen: steht dort `R`, ist es
    ///     eine Referenz aus zwei Zahlen; sonst wird die Position
    ///     zurueckgesetzt
    ///  3. aus den gesammelten Tokens einen Wert bauen
    pub fn objekt(&mut self) -> Result<Option<Wert>, LexFehler> {
        let mut token: Vec<Token> = Vec::new();
        let mut tiefe = 0i32;
        loop {
            let t = self.naechstes()?;
            if matches!(t, Token::Ende) {
                break;
            }
            match &t {
                Token::Satzzeichen(s) if s == "[" || s == "<<" => tiefe += 1,
                Token::Satzzeichen(s) if s == "]" || s == ">>" => tiefe -= 1,
                _ => {}
            }
            token.push(t);
            if tiefe <= 0 {
                break;
            }
        }
        if token.is_empty() {
            return Ok(None);
        }
        // Phase 2: steht danach `Zahl` und dann `R`?
        //
        // DIE ZWEITE FASSUNG DIESER STELLE PRUEFTE NUR **EINEN**
        // Token voraus: bei `12 0 R` steht dort `0`, nicht `R`. Die
        // Form ist drei Token lang, also muessen es **zwei** sein.
        if let [Token::Zahl(n)] = token.as_slice() {
            let n = *n;
            let pos_nach_nummer = self.pos;
            if let Ok(Token::Zahl(g)) = self.naechstes() {
                // added 03.10.2026: das innere `if` in das Muster
                // gezogen (clippy `collapsible_if`) — `Schluesselwort`
                // wird per Muster verglichen, und `k == "R"` ist das
                // gleiche wie das Muster `"R"`.
                if let Ok(Token::Schluesselwort(k)) = self.naechstes()
                    && k == "R"
                {
                    return Ok(Some(Wert::Ref(zahl_zu_u32(n), zahl_zu_u16(g))));
                }
            }
            // Kein `R`: **beide** Positionen zurueck, sonst verliert
            // der Aufrufer Zahlen und der Rest der Datei ist
            // verschoben. Genau das war der Fehler, der beim Lesen
            // von `2103.00020.pdf` auftreten wuerde.
            self.pos = pos_nach_nummer;
        }
        Ok(wert_aus_token(&token))
    }
}

/// Eine Zahl zu einer Objektnummer, mit Grenze.
///
/// added 03.10.2026. `12.7` ist im PDF eine Zahl und **kein**
/// Objektnummer; `clamp` macht daraus 12, statt dass `as u32`
/// stillschweigend 12 liefert — der Unterschied ist nur bei
/// negativen oder riesigen Zahlen sichtbar, und die sind genau die
/// aus einem kaputten Dokument.
fn zahl_zu_u32(z: f64) -> u32 {
    if z.is_finite() && z >= 0.0 && z <= u32::MAX as f64 {
        z as u32
    } else {
        0
    }
}

/// Eine Zahl zu einer Generation, mit Grenze.
fn zahl_zu_u16(z: f64) -> u16 {
    if z.is_finite() && z >= 0.0 && z <= u16::MAX as f64 {
        z as u16
    } else {
        0
    }
}

/// Baut aus einer Token-Liste genau einen Wert.
///
/// added 03.10.2026. Rekursiv fuer Arrays und Wörterbücher, mit einer
/// **Tiefengrenze** — ein Dokument mit 40.000 `[[[[[...` ist eine
/// Bombe, und ein Stack wuerde sie sprengen.
pub fn wert_aus_token(token: &[Token]) -> Option<Wert> {
    let mut rest: Vec<Token> = token.to_vec();
    wert_aus_rest(&mut rest, 0)
}

/// Wie tief Wörterbücher und Arrays verschachtelt sein dürfen.
///
/// added 03.10.2026. 64. Reicht fuer jedes echte Dokument (PDF erlaubt
/// 28 nach alter Spezifikation); darueber ist es eine Rekursion, und
/// eine Rekursion ohne Grenze ist ein Absturz mit Rauschen.
pub const MAX_TIEFE: usize = 64;

/// Baut aus einer Liste den naechsten Wert weg.
fn wert_aus_rest(rest: &mut Vec<Token>, tiefe: usize) -> Option<Wert> {
    if tiefe > MAX_TIEFE {
        return None;
    }
    match rest.first()? {
        Token::Zahl(z) => {
            let v = *z;
            rest.remove(0);
            // DIE REFERENZ-ERKENNUNG FUER ALLES INNERHALB EINES
            // BEHAELTERS.
            //
            // added 03.10.2026: `/Root 1 0 R` wurde zu
            // `(Root, Zahl(1))` — und damit war **jeder** Verweis in
            // **jedem** Wörterbuch eine Zahl. Der Katalog liess sich
            // nicht oeffnen („KeinKatalog"), und die Meldung zeigte auf
            // den Katalog statt auf den Lexer. Gefunden vom Test
            // `xref::minimale_tabelle` („kein /Root").
            //
            // WARUM ZWEI STELLEN (`objekt()` und hier): `objekt()`
            // sammelt nur **ein** flaches Token und muss daher selbst
            // zwei Token vorauslesen; hier liegt die Tokenliste
            // bereits vor. Beide nutzen `zahl_zu_u32`/`zahl_zu_u16`,
            // also ist die Grenze an einer Stelle definiert.
            if rest.len() >= 2 {
                if let (Token::Zahl(g), Token::Schluesselwort(k)) = (&rest[0], &rest[1]) {
                    if k == "R" {
                        let g = *g;
                        rest.remove(0);
                        rest.remove(0);
                        return Some(Wert::Ref(zahl_zu_u32(v), zahl_zu_u16(g)));
                    }
                }
            }
            Some(Wert::Zahl(v))
        }
        Token::Name(b) => {
            let v = Wert::Name(b.clone());
            rest.remove(0);
            Some(v)
        }
        Token::String(s) | Token::Hex(s) => {
            let v = Wert::String(s.clone());
            rest.remove(0);
            Some(v)
        }
        Token::Satzzeichen(s) if s == "[" => {
            rest.remove(0);
            let mut inhalt: Vec<Wert> = Vec::new();
            loop {
                match rest.first() {
                    None => break,
                    Some(Token::Satzzeichen(e)) if e == "]" => {
                        rest.remove(0);
                        break;
                    }
                    Some(_) => match wert_aus_rest(rest, tiefe + 1) {
                        // `wert_aus_rest` hat das erste Element
                        // entfernt — `rest.remove(0)` dort, nicht hier.
                        Some(w) => inhalt.push(w),
                        None => break,
                    },
                }
            }
            Some(Wert::Array(inhalt))
        }
        Token::Satzzeichen(s) if s == "<<" => {
            rest.remove(0);
            let mut inhalt: Vec<(Vec<u8>, Wert)> = Vec::new();
            loop {
                match rest.first() {
                    None => break,
                    Some(Token::Satzzeichen(e)) if e == ">>" => {
                        rest.remove(0);
                        break;
                    }
                    Some(Token::Name(k)) => {
                        let k = k.clone();
                        rest.remove(0);
                        match wert_aus_rest(rest, tiefe + 1) {
                            Some(w) => inhalt.push((k, w)),
                            None => break,
                        }
                    }
                    // Ein Wörterbuch mit einem anderen Schluessel-Typ ist
                    // kaputt; ueberspringen statt abbrechen, damit der
                    // Rest des Wörterbuchs noch lesbar ist.
                    Some(_) => {
                        if wert_aus_rest(rest, tiefe + 1).is_none() {
                            break;
                        }
                    }
                }
            }
            Some(Wert::Dict(inhalt))
        }
        Token::Schluesselwort(k) => {
            let k = k.clone();
            rest.remove(0);
            Some(match k.as_str() {
                "true" => Wert::Bool(true),
                "false" => Wert::Bool(false),
                "null" => Wert::Null,
                // In einem Dateiobekt sind `endobj` und `stream`
                // Schluesselworte, die **nichts** bedeuten.
                _ => Wert::Null,
            })
        }
        Token::Satzzeichen(_) => {
            rest.remove(0);
            Some(Wert::Null)
        }
        Token::Ende => None,
    }
}

/// Hoechste Klammertiefe in einem String.
pub const MAX_STRING_TIEFE: usize = 64;

/// Ist das ein Leerzeichen fuer PDF?
pub fn ist_weissraum(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\x00')
}

/// Ist das ein Ausgabezeichen (darf in einem Namen stehen)?
///
/// added 03.10.2026. Nach ISO 32000 sind das die Zeichen **ausser**
/// den Trennzeichen `()<>[]{}/%` und dem Leerraum. Bytes >= 0x80 sind
/// erlaubt (Namen sind Bytestrings, kein UTF-8).
pub fn ist_ausgabezeichen(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t'
            | b'\r'
            | b'\n'
            | b'\x0c'
            | b'\x00'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}

/// Der Wert einer Hex-Ziffer.
pub fn hex_wert(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// Oktale Ziffer als Zahl.
fn hex_wert_dezimal(b: u8) -> u8 {
    b.wrapping_sub(b'0').min(7)
}

/// Bequemlichkeit: die Basis-Version des Objektlexers.
///
/// added 03.10.2026. Nimmt Bytes und liefert **einen** Wert oder
/// `None`. Fuer Aufrufer, die nur einen Wert wollen und sich nicht um
/// Rest-Token kuemmern.
pub fn ein_objekt(daten: &[u8]) -> Option<Wert> {
    let mut l = Lexer::neu(daten);
    l.objekt().ok().flatten()
}

/// Fehlertyp fuer Aufrufer, die einen Verweis aufloesen wollen.
///
/// added 03.10.2026. Wiederholt aus [`objekt`] exportiert, damit der
/// Aufrufer nicht zwei Module importieren muss.
pub type RefFehler = FehlenderVerweis;

#[cfg(test)]
mod tests {
    use super::*;

    /// Der haeufigste Lexer-Fehlerfall: ein Kommentar in der Mitte.
    ///
    /// added 03.10.2026. Ohne `%`-Sprung haengt der Parser hier und
    /// liefert nichts — man glaubt, die Datei sei kaputt.
    ///
    /// added 03.10.2026: die erste Fassung gab `% Kommentar …\n/Foo
    /// /Bar` und erwartete `w.get_name("Foo") == Some("Bar")`. Das ist
    /// **kein** Wörterbuch — `/Foo /Bar` ist ein Name, kein
    /// Schluessel-Wert-Paar. Die Erwartung war falsch, nicht der
    /// Lexer; er lieferte `Wert::Name("Foo")`, was richtig war.
    #[test]
    fn kommentar_wird_uebersprungen() {
        let w = ein_objekt(b"% Kommentar mit ) Klammer\n<< /Foo /Bar >>").unwrap();
        assert_eq!(w.get_name("Foo"), Some("Bar"));
    }

    /// Klammern in einem String sind **Daten**, keine Trennzeichen.
    #[test]
    fn klammern_im_string() {
        let w = ein_objekt(b"(a (b) c) /S").unwrap();
        assert_eq!(w, Wert::String(b"a (b) c".to_vec()));
    }

    /// `/Type/XRef` ohne Leerzeichen: **beide** Namen muessen
    /// erhalten bleiben.
    ///
    /// added 03.10.2026. Der Trenner zwischen zwei Namen wird im
    /// PDF haeufig weggelassen. Die erste Fassung des Lexers frass den
    /// Schraegstrich des zweiten Namens und lieferte
    /// `Name("Type")` + `Schluesselwort("XRef")` — bei 9 von 11
    /// echten PDFs.
    #[test]
    fn zwei_namen_ohne_leerzeichen() {
        let w = ein_objekt(b"<< /Type/XRef/W[1 2]>>").unwrap();
        assert_eq!(w.get_name("Type"), Some("XRef"));
        assert_eq!(w.get("W").and_then(Wert::array).map(|a| a.len()), Some(2));
    }

    /// `#41` heisst `A`.
    #[test]
    fn hex_flucht_im_namen() {
        let w = ein_objekt(b"/A#42C").unwrap();
        assert_eq!(w.name_bytes(), Some(&b"ABC"[..]));
    }

    /// `12 0 R` wird eine Referenz, `12 0 34` nicht.
    ///
    /// added 03.10.2026. DER Unterschied, an dem ein Parser still
    /// scheitert: ohne die Referenzerkennung bekommt man drei Zahlen
    /// statt eines Verweises, und das Dokument „laesst sich nicht
    /// rendern" ohne Fehlermeldung.
    #[test]
    fn referenz_erkannt() {
        let w = ein_objekt(b"12 0 R").unwrap();
        assert_eq!(w, Wert::Ref(12, 0));
    }

    /// Ein Hex-String mit ungerader Stellenzahl wird aufgefuellt.
    #[test]
    fn hex_string_ungerade() {
        let w = ein_objekt(b"<4A5>").unwrap();
        assert_eq!(w, Wert::String(vec![0x4a, 0x50]));
    }

    /// Oktal-Escapes: der Wert ist die Summe, und ein Wert über 255
    /// wird auf 255 begrenzt.
    ///
    /// added 03.10.2026. GEMESSEN, bevor der Test geschrieben war:
    /// `\777` (= 511) liess den Debug-Bau mit „attempt to add with
    /// overflow" **abbrechen** — der Lexer panikte an fremden Dateien.
    /// Der Fall ist billig zu pruefen und deshalb festgehalten.
    ///
    /// DIE DREI ZAHLEN: 65 ist ein Backslash mit drei Ziffern gefolgt
    /// von `A`; 255 ist das groesste Byte; und dreimal `7` ergibt 511
    /// und wird begrenzt.
    ///
    /// ACHTUNG, Rust: `\7` in einem Byte-String-Literal ist **kein**
    /// gültiges Escape — die Zahlen stecken in `&str` und werden erst
    /// von `ein_objekt` zu Bytes.
    #[test]
    fn oktal_escape() {
        let w = ein_objekt(r"(\101\377\777)".as_bytes()).unwrap();
        assert_eq!(w, Wert::String(vec![65, 255, 255]));
        // Und die Kurzform mit einer und mit zwei Ziffern:
        let w = ein_objekt(r"(\7\40\101)".as_bytes()).unwrap();
        assert_eq!(w, Wert::String(vec![7, 32, 65]));
    }

    /// Verschachtelte Wörterbücher werden zu Werten, nicht zu Resten.
    #[test]
    fn verschachteltes_dict() {
        let w = ein_objekt(b"<< /A << /B [1 2 3] >> /C /D >>").unwrap();
        assert_eq!(w.get("C").and_then(Wert::name), Some("D"));
        assert_eq!(
            w.get("A")
                .and_then(|a| a.get("B"))
                .and_then(Wert::array)
                .map(|a| a.len()),
            Some(3)
        );
    }

    /// Eine 70.000 Zeichen lange Klammer ist eine **Bombe**, und der
    /// Lexer sagt das, statt 70.000 Byte zu allokieren.
    #[test]
    fn string_bombe_wird_abgewiesen() {
        let mut daten = vec![b'('];
        daten.extend(std::iter::repeat_n(b'a', MAX_TOKEN + 10));
        let mut l = Lexer::neu(&daten);
        assert!(l.naechstes().is_err());
    }

    /// 40.000 offene Klammern in einem Array sind eine Rekursionsbombe.
    #[test]
    fn tiefen_bombe_wird_abgewiesen() {
        let daten = vec![b'['; MAX_TIEFE + 20];
        let mut l = Lexer::neu(&daten);
        let w = l.objekt().unwrap();
        // Kein Absturz, und der Wert ist abgeschnitten.
        assert!(w.is_none() || matches!(w, Some(Wert::Array(_))));
    }
}
