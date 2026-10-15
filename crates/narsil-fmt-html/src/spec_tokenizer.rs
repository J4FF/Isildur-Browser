//! Der HTML-Tokenizer nach WHATWG HTML, Kapitel 13.2.4/13.2.5.
//!
//! Eigene Datei neben dem alten `tokenizer.rs`, damit der Umbau die
//! bestehende Anzeige nicht mitten im Bau bricht. Alle Zustaende der
//! Spezifikation sind vorhanden, ebenso die Zeichenreferenzen mit der
//! vollen benannten Tabelle aus `entities_tabelle.rs` (2231 Eintraege,
//! erzeugt aus https://html.spec.whatwg.org/entities.json).
//!
//! # Eingabe-Vorverarbeitung (13.2.3)
//!
//! CRLF und CR werden zu LF; ein fuehrendes BOM wird entfernt. Enthaelt
//! die Eingabe kein CR, wird sie NICHT kopiert (M4).
//!
//! # Ausgabe
//!
//! [`SpecToken`] mit zusammengefassten Zeichentokens (die offizielle
//! Testdatei fasst benachbarte Zeichen ebenfalls zusammen). `Eof` wird
//! beliebig oft geliefert.
//!
//! # Grenzen
//!
//! * Lineare Laufzeit, keine Rekursion, kein unsafe, keine Panics.
//! * Die benannte Entitaetentabelle wird per Binaersuche durchsucht;
//!   ein Name ist hoechstens `MAX_ENTITAETSNAME` = 32 Zeichen lang.
//! * Kein Speicherverstaerker: die Ausgabe ist hoechstens so gross wie
//!   die Eingabe plus konstanter Aufschlag je Token.

use crate::entities_tabelle::{ENTITAETEN, MAX_ENTITAETSNAME};

/// Ein benannter Fehler der Tokenizer-Grenzen (M3/M8).
///
/// Beide Grenzen existieren, damit ein Fehler im Zustandsautomaten
/// nicht als Speicherleck endet: der Aufrufer kann `fehler()` lesen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerFehler {
    /// Zu viele Zustandsschritte fuer die Eingabelaenge.
    Schrittzahl,
    /// Zu viele Token fuer die Eingabelaenge.
    Tokenzahl,
}

/// Hoechstzahl der Zustandsschritte je Eingabebyte (M3).
pub const MAX_SCHRITTE_JE_BYTE: usize = 32;

/// Hoechstzahl der Token je Eingabebyte (M3).
pub const MAX_TOKEN_JE_BYTE: usize = 4;

/// Der Zustand des Tokenizers (nur die Zustaende, die von aussen
/// gesetzt werden koennen: die sechs Startzustaende der Testdatei).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zustand {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    CdataSection,
}

/// Ein Token nach der Spezifikation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecToken {
    /// `<!DOCTYPE name PUBLIC "..." "...">`; `quirlig` = force-quirks.
    Doctype {
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
        quirlig: bool,
    },
    /// Ein oeffnendes Tag.
    StartTag {
        name: String,
        attribute: Vec<(String, String)>,
        selbst_schliessend: bool,
    },
    /// Ein schliessendes Tag.
    EndTag { name: String },
    /// Ein Kommentar.
    Comment(String),
    /// Zeichendaten (benachbarte Zeichen zusammengefasst).
    Character(String),
    /// `<?ziel daten?>` (13.2.5.72 - 13.2.5.76).
    ///
    /// NEU, 01.10.2026: der Spec hat echte Processing Instructions, und
    /// der Testkorpus testet sie seit dem Pull vom 29.09.2026. Vorher
    /// wurde `<?…>` als Kommentar verworfen.
    ProcessingInstruction { target: String, daten: String },
    /// Ende der Eingabe.
    Eof,
}

/// Der interne Zustandsautomat: alle Zustaende aus 13.2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum S {
    Data,
    Rcdata,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    Rawtext,
    RawtextLessThanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptData,
    ScriptDataLessThanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThanSign,
    ScriptDataDoubleEscapeEnd,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    // 13.2.5.72 - 13.2.5.76: Processing instruction.
    //
    // GEMESSENE ENTSCHEIDUNG: die fuenf Zustaende fehlten ganz, und
    // `<?…>` ging stattdessen in den BogusComment — es wurde also ein
    // Kommentar daraus. Der heutige Spec (und der Testkorpus seit dem
    // Pull vom 29.09.2026, tokenizer/test2.test) erwartet ein eigenes
    // Token mit Ziel und Daten:
    //   "<?namespace>"     -> ProcessingInstruction("namespace", "")
    //   "<?foo bar?>"      -> ProcessingInstruction("foo", "bar")
    //   "<?foo bar??>"     -> ProcessingInstruction("foo", "bar?")
    //   "<?foo-->"         -> ProcessingInstruction("foo", "-")
    ProcessingInstructionOpen,
    ProcessingInstructionTarget,
    AfterProcessingInstructionTarget,
    ProcessingInstructionData,
    ProcessingInstructionQuestionable,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    DecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

/// In welchen Zustand die Zeichenreferenz zurueckkehrt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rueckkehr {
    Data,
    Rcdata,
    DoppelAnfuehrung,
    EinzelAnfuehrung,
    OhneAnfuehrung,
}

/// Der Tokenizer.
pub struct SpecTokenizer<'a> {
    /// Die Eingabe ohne CR (borrowed, wenn kein CR vorkommt).
    eingabe: &'a str,
    /// Normalisierte Kopie, falls die Eingabe CR enthielt.
    normalisiert: Option<String>,
    pos: usize,
    zustand: S,
    /// Der letzte Start-Tag-Name (fuer "appropriate end tag").
    letzter_start_tag: Option<String>,
    /// Darf `[CDATA[` eine CDATA-Sektion beginnen? (Der Baum-Aufbau
    /// setzt das in Fremdinhalt; in HTML ist es ein Fehler.)
    cdata_erlaubt: bool,
    /// Wurde das Ende der Eingabe schon gemeldet?
    eof_aus: bool,
    /// Zustandsschritte (M3-Grenze).
    schritte: usize,
    /// Die Schrittgrenze, EINMAL berechnet (M1, und siehe `naechstes`).
    step_grenze: usize,
    /// Ausgegebene Token (M3-Grenze).
    token_zahl: usize,
    /// Der benannte Grenzfehler, falls eine Grenze griff.
    fehler: Option<TokenizerFehler>,
    /// Puffer fuer benachbarte Zeichen.
    puffer: String,
    /// Ein Token, das noch ausgeliefert werden muss (weil der
    /// Zeichenpuffer zuerst dran war).
    wartend: Option<SpecToken>,
    /// Laufender Tag-Name / Kommentar / DOCTYPE-Feld.
    name: String,
    attribute: Vec<(String, String)>,
    attribut_name: String,
    attribut_wert: String,
    attribut_duplikat: bool,
    /// Die Hashes der Attributnamen DIESES Tags (siehe
    /// `attribut_speichern`): macht den Duplikatscheck linear statt
    /// quadratisch.
    attribut_hashes: std::collections::HashSet<u32>,
    in_attribut: bool,
    selbst_schliessend: bool,
    ist_end_tag: bool,
    kommentar: String,
    doctype_public: Option<String>,
    doctype_system: Option<String>,
    doctype_quirlig: bool,
    /// Uebergangspuffer fuer End-Tag-Namen in RCDATA/RAWTEXT/Script.
    uebergang: String,
    /// Zeichenreferenz: Rueckkehrzustand.
    rueckkehr: Rueckkehr,
    numerisch: u32,
    numerisch_hex: bool,
    /// Das 'x' oder 'X' aus "&#x"/"&#X" (13.2.5.80). Als Zeichen
    /// gespeichert, weil der Spec ausdruecklich das EINGEGEBENE
    /// Zeichen in den Puffer legt.
    hex_marke: char,
}

impl<'a> SpecTokenizer<'a> {
    /// Neuer Tokenizer im Data-Zustand.
    pub fn neu(eingabe: &'a str) -> Self {
        Self::mit_zustand(eingabe, Zustand::Data, None)
    }

    /// Neuer Tokenizer mit Startzustand und letztem Start-Tag.
    pub fn mit_zustand(
        eingabe: &'a str,
        zustand: Zustand,
        letzter_start_tag: Option<&str>,
    ) -> Self {
        // Vorverarbeitung: CRLF/CR -> LF. Nur kopieren, wenn noetig.
        // Ein fuehrendes BOM wird NICHT entfernt: das BOM entfernt die
        // Zeichensatz-Erkennung (H3), nicht der Tokenizer (13.2.3.5).
        let normalisiert = if eingabe.contains('\r') {
            let mut s = String::with_capacity(eingabe.len());
            let mut zeichen = eingabe.chars().peekable();
            while let Some(c) = zeichen.next() {
                if c == '\r' {
                    if zeichen.peek() == Some(&'\n') {
                        zeichen.next();
                    }
                    s.push('\n');
                } else {
                    s.push(c);
                }
            }
            Some(s)
        } else {
            None
        };
        // M1 + GEMESSEN (03.10.2026): die Schrittgrenze ist eine
        // KONSTANTE der Eingabe. Sie stand in `naechstes()` und wurde
        // dort PRO ZEICHEN neu berechnet — bei 8 MB Rohtext also 8
        // Millionen Mal eine Multiplikation und ein Schnitt. Der
        // Tokenizer brauchte dafuer 581 ms (gemessen, Debug). Jetzt
        // steht sie einmal hier.
        let step_grenze = MAX_SCHRITTE_JE_BYTE
            * eingabe
                .len()
                .saturating_add(normalisiert.as_ref().map_or(0, |s| s.len()))
            + 4096;
        let t = SpecTokenizer {
            eingabe,
            normalisiert,
            pos: 0,
            zustand: match zustand {
                Zustand::Data => S::Data,
                Zustand::Rcdata => S::Rcdata,
                Zustand::Rawtext => S::Rawtext,
                Zustand::ScriptData => S::ScriptData,
                Zustand::Plaintext => S::Plaintext,
                Zustand::CdataSection => S::CdataSection,
            },
            letzter_start_tag: letzter_start_tag.map(|s| s.to_string()),
            cdata_erlaubt: false,
            eof_aus: false,
            schritte: 0,
            step_grenze,
            token_zahl: 0,
            fehler: None,
            puffer: String::new(),
            wartend: None,
            name: String::new(),
            attribute: Vec::new(),
            attribut_name: String::new(),
            attribut_wert: String::new(),
            attribut_duplikat: false,
            attribut_hashes: std::collections::HashSet::new(),
            in_attribut: false,
            selbst_schliessend: false,
            ist_end_tag: false,
            kommentar: String::new(),
            doctype_public: None,
            doctype_system: None,
            doctype_quirlig: false,
            uebergang: String::new(),
            rueckkehr: Rueckkehr::Data,
            numerisch: 0,
            numerisch_hex: false,
            hex_marke: 'x',
        };
        // Ein fuehrendes BOM wird NICHT entfernt: das erledigt die
        // Zeichensatz-Erkennung (H3), nicht der Tokenizer (13.2.3.5);
        // domjs.test: "leading U+FEFF must pass through".
        t
    }

    /// Der Text (normalisiert oder geliehen).
    fn text(&self) -> &str {
        match &self.normalisiert {
            Some(s) => s.as_str(),
            None => self.eingabe,
        }
    }

    /// Die aktuelle Position in Byte.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Setzt den Zustand (der Baum-Aufbau braucht das).
    pub fn setze_zustand(&mut self, zustand: Zustand) {
        self.zustand = match zustand {
            Zustand::Data => S::Data,
            Zustand::Rcdata => S::Rcdata,
            Zustand::Rawtext => S::Rawtext,
            Zustand::ScriptData => S::ScriptData,
            Zustand::Plaintext => S::Plaintext,
            Zustand::CdataSection => S::CdataSection,
        };
    }

    /// Setzt den letzten Start-Tag-Namen.
    pub fn setze_letzter_start_tag(&mut self, name: &str) {
        self.letzter_start_tag = Some(name.to_string());
    }

    /// Erlaubt oder verbietet CDATA (Fremdinhalt).
    pub fn setze_cdata_erlaubt(&mut self, ja: bool) {
        self.cdata_erlaubt = ja;
    }

    // ------------------------------------------------------------------
    // Zeichenzugriff
    // ------------------------------------------------------------------

    /// Das Zeichen an der aktuellen Position.
    fn aktuell(&self) -> Option<char> {
        self.text()[self.pos..].chars().next()
    }

    /// Beginnt der Rest (ASCII-gross/klein-unabhaengig) mit `muster`?
    fn beginnt_mit_ci(&self, muster: &str) -> bool {
        let rest = &self.text()[self.pos..];
        rest.len() >= muster.len()
            && rest.is_char_boundary(muster.len())
            && rest[..muster.len()].eq_ignore_ascii_case(muster)
    }

    /// Beginnt der Rest mit `muster`?
    fn beginnt_mit(&self, muster: &str) -> bool {
        self.text()[self.pos..].starts_with(muster)
    }

    /// Verbraucht `n` BYTES (immer auf Zeichengrenzen aufgerufen).
    fn weiter(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.text().len());
        while self.pos < self.text().len() && !self.text().is_char_boundary(self.pos) {
            self.pos += 1;
        }
    }

    /// Haengt ein Zeichen an den Zeichenpuffer.
    fn zeichen(&mut self, c: char) {
        self.puffer.push(c);
    }

    /// Haengt einen String an den Zeichenpuffer.
    fn zeichen_str(&mut self, s: &str) {
        self.puffer.push_str(s);
    }

    /// Liefert ein Token; der Zeichenpuffer geht vor.
    fn liefere(&mut self, t: SpecToken) -> SpecToken {
        self.token_zahl += 1;
        let grenze = MAX_TOKEN_JE_BYTE * self.text().len() + 64;
        if self.token_zahl > grenze {
            self.fehler = Some(TokenizerFehler::Tokenzahl);
            self.eof_aus = true;
            return SpecToken::Eof;
        }
        if !self.puffer.is_empty() {
            self.wartend = Some(t);
            return SpecToken::Character(std::mem::take(&mut self.puffer));
        }
        t
    }

    /// Setzt den Attributwert: in einem Attribut wird angehaengt, sonst
    /// gehen die Zeichen in den Zeichenpuffer (13.2.5.72).
    fn ref_ausgabe(&mut self, s: &str) {
        if self.in_attribut {
            self.attribut_wert.push_str(s);
        } else {
            self.zeichen_str(s);
        }
    }

    /// Das naechste Token.
    pub fn naechstes(&mut self) -> SpecToken {
        if let Some(t) = self.wartend.take() {
            return t;
        }
        if self.eof_aus {
            return SpecToken::Eof;
        }
        loop {
            // DER SCHNELLPFAD (03.10.2026, gemessen).
            //
            // GEMESSEN, woher die Zeit kam: 8 MB Rohtext brauchten
            // 512 ms im Tokenizer, also ~60 ns je ZEICHEN. Der
            // Dispatcher `schritt()` vergleicht ~80 Zustaende — im
            // Debug-Bau kostet jedes davon Vergleiche — und wurde
            // einmal je Zeichen durchlaufen, obwohl in "data" fast
            // immer dasselbe passiert: ein gewoehnliches Zeichen an den
            // Zeichenpuffer haengen.
            //
            // Der Schnellpfad nimmt einen GANZEN Lauf solcher Zeichen
            // auf einmal. Das aendert den Tokenstrom NICHT: der Puffer
            // wird ohnehin als EIN Character-Token ausgegeben (das ist
            // der Grund, warum 8 MB Rohtext zwei Token ergeben und
            // nicht acht Millionen), und die drei Zeichen, an denen der
            // Lauf endet, sind genau die, an denen der Data-Zustand
            // etwas anderes tut (13.2.5.1: `&` -> character reference,
            // `<` -> tag open, U+0000 -> unexpected-null-character).
            // (Fortsetzung 03.10.2026, gemessen) Der Schnellpfad gilt
            // jetzt auch fuer die ROHTEXT-Zustaende. GEMESSEN am
            // selben Inhalt (9,2 MB `<script>`):
            //   Data      46,7 ms  (Schnellpfad)
            //   ScriptData 682,3 ms (ohne Schnellpfad)
            //   parse_dokument 2106 ms
            // `<script>`, `<style>`, `<textarea>`, `<title>` und
            // `<pre>` sind genau die Zustaende, in denen echte Seiten
            // ihre grossen Textmassen haben — sie zu uebersehen heisst,
            // den haeufigsten Fall zu uebersehen.
            //
            // DIE STOPPZEICHEN je Zustand (13.2.5.1/2/3/4/5):
            //   Data            `<` `&` U+0000
            //   Rcdata          `<` `&` U+0000   (RCDATA kennt `&`)
            //   Rawtext         `<` U+0000       (kein `&`!)
            //   ScriptData      `<` U+0000
            //   Plaintext       U+0000           (kein `<`!)
            // Alles andere haengt in diesen Zustaenden nur an den
            // Zeichenpuffer — das ist an `st_roh`/`st_script`/
            // `st_data` abgelesen, nicht angenommen.
            let stopps: &[u8] = match self.zustand {
                S::Data | S::Rcdata => b"<\x00&",
                S::Rawtext | S::ScriptData => b"<\x00",
                S::Plaintext => b"\x00",
                _ => b"",
            };
            if !stopps.is_empty()
                && matches!(
                    self.zustand,
                    S::Data | S::Rcdata | S::Rawtext | S::ScriptData | S::Plaintext
                )
            {
                let mut laenge = 0usize;
                {
                    let Self {
                        eingabe,
                        normalisiert,
                        pos,
                        ..
                    } = self;
                    let text: &str = match normalisiert {
                        Some(s) => s.as_str(),
                        None => eingabe,
                    };
                    let b = text.as_bytes();
                    // `stopps.contains(&b[...])` statt drei Vergleiche:
                    // drei Zustaende haben verschiedene Stopps, und
                    // die Liste ist winzig (2 bis 3 Byte).
                    while *pos + laenge < b.len() && !stopps.contains(&b[*pos + laenge]) {
                        laenge += 1;
                    }
                }
                if laenge > 0 {
                    // `b[ab+laenge]` ist ASCII, also liegt die Stelle an
                    // einer Zeichengrenze — schneiden ist erlaubt.
                    //
                    // ZERLEGUNG statt `self.text()`: GEMESSENER
                    // Compiler-Fehler (03.10.2026) zweimal — "cannot
                    // borrow `self.puffer` as mutable because it is also
                    // borrowed as immutable". `text()` leiht GANZ
                    // `self`, und der Puffer ist ein Feld davon. Mit
                    // der Zerlegung sind die Borrows disjunkt.
                    let Self {
                        eingabe,
                        normalisiert,
                        puffer,
                        pos,
                        ..
                    } = self;
                    let text: &str = match normalisiert {
                        Some(s) => s.as_str(),
                        None => eingabe,
                    };
                    puffer.push_str(&text[*pos..*pos + laenge]);
                    self.schritte += laenge;
                    self.pos += laenge;
                    continue;
                }
            }
            self.schritte += 1;
            if self.schritte > self.step_grenze {
                self.fehler = Some(TokenizerFehler::Schrittzahl);
                self.eof_aus = true;
                return SpecToken::Eof;
            }
            let c = self.aktuell();
            if let Some(t) = self.schritt(c) {
                return self.liefere(t);
            }
        }
    }

    /// Der benannte Grenzfehler, falls eine Grenze gegriffen hat.
    pub fn fehler(&self) -> Option<TokenizerFehler> {
        self.fehler
    }

    /// Die Zahl der Zustandsschritte (fuer Tests).
    pub fn schritte(&self) -> usize {
        self.schritte
    }

    /// Meldet das Ende der Eingabe (genau einmal wirksam).
    fn eof(&mut self) -> Option<SpecToken> {
        self.eof_aus = true;
        Some(SpecToken::Eof)
    }

    /// Die fuenf Processing-instruction-Zustaende (13.2.5.72 - .76).
    ///
    /// Eigene Funktion, damit der grosse `st_data`-Zweig nicht an der
    /// letzten Stelle wachsen muss. Der Aufrufer verteilt die Zustaende
    /// dorthin (siehe `schritt`).
    fn st_pi(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::ProcessingInstructionOpen => match c {
                // ASCII alpha, U+005F: reconsume im Target-Zustand.
                Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                    self.kommentar.push(ch);
                    self.weiter(1);
                    self.zustand = S::ProcessingInstructionTarget;
                    None
                }
                None => {
                    // 13.2.5.72, woertlich: "This is an
                    // eof-in-processing-instruction parse error. Emit an
                    // end-of-file token." — KEIN Kommentar.
                    //
                    // GEMESSENER FEHLER (03.10.2026), tokenizer-
                    // Abnahme (test3.test): ich gab hier einen
                    // Kommentar aus. Erwartet: [] (nur EOF).
                    //   <?        erwartet []  bekommen [Kommentar("?")]
                    return self.eof();
                }
                Some(_) => {
                    // "invalid-first-character-of-processing-instruction-
                    // target ... Convert the temporary buffer to a
                    // comment. Reconsume in the bogus comment state."
                    self.zustand = S::BogusComment;
                    None
                }
            },
            S::ProcessingInstructionTarget => match c {
                Some(ch) if matches!(ch, '\t' | '\n' | '\u{c}' | ' ' | '?' | '>') => {
                    // Der Puffer beginnt mit dem `?`: es ist der erste
                    // Buchstabe des Kommentars, aber NICHT Teil des
                    // Ziels. Beides wird getrennt behandelt.
                    let puffer = std::mem::take(&mut self.kommentar);
                    let ziel: String = puffer.chars().skip(1).collect();
                    // 13.2.5.73, woertlich: "If target is an ASCII
                    // case-insensitive match for 'xml' or
                    // 'xml-stylesheet': This is a
                    // disallowed-processing-instruction-target parse
                    // error. Convert the temporary buffer to a comment.
                    // Reconsume in the bogus comment state."
                    //
                    // GEMESSENER FEHLER (01.10.2026, an beiden Stellen
                    // der Datei): der Vergleich lief gegen den PUFFER,
                    // der mit `?` begann — er war also nie wahr, und
                    // `<?xml …>` lieferte ein Processing Instruction
                    // statt des Kommentars.
                    if ziel.eq_ignore_ascii_case("xml")
                        || ziel.eq_ignore_ascii_case("xml-stylesheet")
                    {
                        self.kommentar = puffer;
                        self.zustand = S::BogusComment;
                        return None;
                    }
                    self.name = ziel;
                    self.attribut_wert.clear();
                    self.zustand = S::AfterProcessingInstructionTarget;
                    None
                }
                Some(ch) if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' => {
                    self.kommentar.push(ch);
                    self.weiter(1);
                    None
                }
                None => {
                    // 13.2.5.73, woertlich: "This is an
                    // eof-in-processing-instruction parse error. Emit an
                    // end-of-file token." — KEIN Kommentar.
                    // GEMESSENER FEHLER (03.10.2026): <?a lieferte
                    // [Kommentar("?a")], erwartet [].
                    return self.eof();
                }
                Some(_) => {
                    self.zustand = S::BogusComment;
                    None
                }
            },
            S::AfterProcessingInstructionTarget => match c {
                // Whitespace wird ignoriert.
                Some(ch) if matches!(ch, '\t' | '\n' | '\u{c}' | ' ') => {
                    self.weiter(1);
                    None
                }
                _ => {
                    self.zustand = S::ProcessingInstructionData;
                    None
                }
            },
            S::ProcessingInstructionData => match c {
                Some('?') => {
                    self.weiter(1);
                    self.zustand = S::ProcessingInstructionQuestionable;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    return Some(SpecToken::ProcessingInstruction {
                        target: std::mem::take(&mut self.name),
                        daten: std::mem::take(&mut self.attribut_wert),
                    });
                }
                None => {
                    // 13.2.5.75, woertlich: "This is an
                    // eof-in-processing-instruction parse error. Emit an
                    // end-of-file token." Das PI-Token wird an dieser
                    // Stelle NICHT ausgegeben — nur bei '>' bzw. '?>'.
                    //
                    // GEMESSENER FEHLER (03.10.2026), 12 Faelle in
                    // test3.test: <?z z lieferte
                    // [Anweisung { ziel: "z", daten: "z" }], erwartet [].
                    return self.eof();
                }
                Some(ch) => {
                    self.attribut_wert.push(ch);
                    self.weiter(1);
                    None
                }
            },
            S::ProcessingInstructionQuestionable => match c {
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    return Some(SpecToken::ProcessingInstruction {
                        target: std::mem::take(&mut self.name),
                        daten: std::mem::take(&mut self.attribut_wert),
                    });
                }
                None => {
                    // 13.2.5.76, woertlich: "This is an
                    // eof-in-processing-instruction parse error. Emit an
                    // end-of-file token." (auch hier kein PI-Token).
                    // GEMESSENER FEHLER (03.10.2026): <?Z Z? (Eingabe
                    // "<?Z Z") lieferte ein Anweisung-Token, erwartet [].
                    return self.eof();
                }
                // "Append U+003F (?) to the current processing
                // instruction token's data. Reconsume in the processing
                // instruction data state."
                Some(_) => {
                    self.attribut_wert.push('?');
                    self.zustand = S::ProcessingInstructionData;
                    None
                }
            },
            _ => None,
        }
    }

    /// Ein Schritt des Zustandsautomaten. `None` = weiter, `Some` = Token.
    fn schritt(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::Data => self.st_data(c),
            S::ProcessingInstructionOpen
            | S::ProcessingInstructionTarget
            | S::AfterProcessingInstructionTarget
            | S::ProcessingInstructionData
            | S::ProcessingInstructionQuestionable => self.st_pi(c),
            S::Rcdata
            | S::RcdataLessThanSign
            | S::RcdataEndTagOpen
            | S::RcdataEndTagName
            | S::Rawtext
            | S::RawtextLessThanSign
            | S::RawtextEndTagOpen
            | S::RawtextEndTagName
            | S::Plaintext
            | S::CdataSection
            | S::CdataSectionBracket
            | S::CdataSectionEnd => self.st_roh(c),
            S::ScriptData
            | S::ScriptDataLessThanSign
            | S::ScriptDataEndTagOpen
            | S::ScriptDataEndTagName
            | S::ScriptDataEscapeStart
            | S::ScriptDataEscapeStartDash
            | S::ScriptDataEscaped
            | S::ScriptDataEscapedDash
            | S::ScriptDataEscapedDashDash
            | S::ScriptDataEscapedLessThanSign
            | S::ScriptDataEscapedEndTagOpen
            | S::ScriptDataEscapedEndTagName
            | S::ScriptDataDoubleEscapeStart
            | S::ScriptDataDoubleEscaped
            | S::ScriptDataDoubleEscapedDash
            | S::ScriptDataDoubleEscapedDashDash
            | S::ScriptDataDoubleEscapedLessThanSign
            | S::ScriptDataDoubleEscapeEnd => self.st_script(c),
            S::TagOpen
            | S::EndTagOpen
            | S::TagName
            | S::BeforeAttributeName
            | S::AttributeName
            | S::AfterAttributeName
            | S::BeforeAttributeValue
            | S::AttributeValueDoubleQuoted
            | S::AttributeValueSingleQuoted
            | S::AttributeValueUnquoted
            | S::AfterAttributeValueQuoted
            | S::SelfClosingStartTag => self.st_tag(c),
            S::BogusComment
            | S::MarkupDeclarationOpen
            | S::CommentStart
            | S::CommentStartDash
            | S::Comment
            | S::CommentLessThanSign
            | S::CommentLessThanSignBang
            | S::CommentLessThanSignBangDash
            | S::CommentLessThanSignBangDashDash
            | S::CommentEndDash
            | S::CommentEnd
            | S::CommentEndBang => self.st_kommentar(c),
            S::Doctype
            | S::BeforeDoctypeName
            | S::DoctypeName
            | S::AfterDoctypeName
            | S::AfterDoctypePublicKeyword
            | S::BeforeDoctypePublicIdentifier
            | S::DoctypePublicIdentifierDoubleQuoted
            | S::DoctypePublicIdentifierSingleQuoted
            | S::AfterDoctypePublicIdentifier
            | S::BetweenDoctypePublicAndSystemIdentifiers
            | S::AfterDoctypeSystemKeyword
            | S::BeforeDoctypeSystemIdentifier
            | S::DoctypeSystemIdentifierDoubleQuoted
            | S::DoctypeSystemIdentifierSingleQuoted
            | S::AfterDoctypeSystemIdentifier
            | S::BogusDoctype => self.st_doctype(c),
            S::CharacterReference
            | S::NamedCharacterReference
            | S::AmbiguousAmpersand
            | S::NumericCharacterReference
            | S::HexadecimalCharacterReferenceStart
            | S::DecimalCharacterReferenceStart
            | S::HexadecimalCharacterReference
            | S::DecimalCharacterReference
            | S::NumericCharacterReferenceEnd => self.st_charref(c),
        }
    }

    // @@WEITERE@@

    // ------------------------------------------------------------------
    // 13.2.5.77-84 Zeichenreferenzen
    // ------------------------------------------------------------------

    fn st_charref(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::CharacterReference => {
                if matches!(c, Some(ch) if ch.is_ascii_alphanumeric()) {
                    self.zustand = S::NamedCharacterReference;
                } else if c == Some('#') {
                    self.weiter(1);
                    self.zustand = S::NumericCharacterReference;
                } else {
                    self.flush_ref("&");
                }
                None
            }
            S::NamedCharacterReference => {
                let rest = &self.text()[self.pos..];
                match finde_entitaet(rest) {
                    Some((name, chars)) => {
                        let danach = rest[name.len()..].chars().next();
                        let mit_semikolon = name.ends_with(';');
                        if self.in_attribut
                            && !mit_semikolon
                            && matches!(danach, Some(ch) if ch == '=' || ch.is_ascii_alphanumeric())
                        {
                            self.flush_ref_ambiguous("&");
                        } else {
                            let chars = chars.to_string();
                            self.weiter(name.len());
                            self.ref_ausgabe(&chars);
                            self.zustand = self.rueckkehr_zustand();
                        }
                    }
                    None => self.flush_ref_ambiguous("&"),
                }
                None
            }
            S::AmbiguousAmpersand => {
                if matches!(c, Some(ch) if ch.is_ascii_alphanumeric()) {
                    let ch = c.expect("Zeichen");
                    self.weiter(ch.len_utf8());
                    if self.in_attribut {
                        self.attribut_wert.push(ch);
                    } else {
                        self.zeichen(ch);
                    }
                    None
                } else {
                    self.zustand = self.rueckkehr_zustand();
                    None
                }
            }
            S::NumericCharacterReference => {
                if c == Some('x') || c == Some('X') {
                    // 13.2.5.80, woertlich: "U+0078 LATIN SMALL LETTER
                    // X / U+0058 LATIN CAPITAL LETTER X — Append the
                    // CURRENT INPUT CHARACTER to the temporary buffer."
                    // Also das Originalzeichen, nicht ein festes 'x'.
                    if let Some(ch) = c {
                        self.hex_marke = ch;
                    }
                    self.weiter(1);
                    self.numerisch = 0;
                    self.numerisch_hex = true;
                    self.zustand = S::HexadecimalCharacterReferenceStart;
                } else {
                    self.numerisch = 0;
                    self.numerisch_hex = false;
                    self.zustand = S::DecimalCharacterReferenceStart;
                }
                None
            }
            S::HexadecimalCharacterReferenceStart => {
                if matches!(c, Some(ch) if ch.is_ascii_hexdigit()) {
                    self.zustand = S::HexadecimalCharacterReference;
                } else {
                    // GEMESSENER FEHLER (01.10.2026): hier stand fest
                    // `flush_ref("&#x")`. Bei der Eingabe `&#X` kam
                    // deshalb `&#x` heraus statt `&#X` — zwei Faelle
                    // (entities01.dat "FOO&#XZOO", tests2.dat "&#X").
                    if self.hex_marke == 'X' {
                        self.flush_ref("&#X");
                    } else {
                        self.flush_ref("&#x");
                    }
                }
                None
            }
            S::DecimalCharacterReferenceStart => {
                if matches!(c, Some(ch) if ch.is_ascii_digit()) {
                    self.zustand = S::DecimalCharacterReference;
                } else {
                    self.flush_ref("&#");
                }
                None
            }
            S::HexadecimalCharacterReference => {
                if let Some(ch) = c.filter(|ch| ch.is_ascii_hexdigit()) {
                    self.numerisch = self
                        .numerisch
                        .saturating_mul(16)
                        .saturating_add(ch.to_digit(16).expect("Hexziffer"));
                    self.weiter(1);
                } else {
                    self.zustand = S::NumericCharacterReferenceEnd;
                }
                None
            }
            S::DecimalCharacterReference => {
                if let Some(ch) = c.filter(|ch| ch.is_ascii_digit()) {
                    self.numerisch = self
                        .numerisch
                        .saturating_mul(10)
                        .saturating_add(ch.to_digit(10).expect("Ziffer"));
                    self.weiter(1);
                } else {
                    self.zustand = S::NumericCharacterReferenceEnd;
                }
                None
            }
            S::NumericCharacterReferenceEnd => {
                if c == Some(';') {
                    self.weiter(1);
                }
                let zeichen = numerisch_zu_zeichen(self.numerisch);
                self.ref_ausgabe(&zeichen.to_string());
                self.numerisch = 0;
                self.numerisch_hex = false;
                self.zustand = self.rueckkehr_zustand();
                None
            }
            _ => self.eof(),
        }
    }

    /// Gibt den Puffer einer gescheiterten Zeichenreferenz aus und kehrt
    /// in den Rueckkehrzustand zurueck (13.2.5.77).
    fn flush_ref(&mut self, puffer: &str) {
        self.ref_ausgabe(puffer);
        self.zustand = self.rueckkehr_zustand();
    }

    /// Wie [`Self::flush_ref`], aber ueber den Zustand "ambiguous
    /// ampersand" (13.2.5.79): die folgenden alphanumerischen Zeichen
    /// werden als normale Zeichen ausgegeben.
    fn flush_ref_ambiguous(&mut self, puffer: &str) {
        self.ref_ausgabe(puffer);
        self.zustand = S::AmbiguousAmpersand;
    }

    /// Der interne Zustand zum Rueckkehrzustand der Zeichenreferenz.
    fn rueckkehr_zustand(&self) -> S {
        match self.rueckkehr {
            Rueckkehr::Data => S::Data,
            Rueckkehr::Rcdata => S::Rcdata,
            Rueckkehr::DoppelAnfuehrung => S::AttributeValueDoubleQuoted,
            Rueckkehr::EinzelAnfuehrung => S::AttributeValueSingleQuoted,
            Rueckkehr::OhneAnfuehrung => S::AttributeValueUnquoted,
        }
    }

    // ------------------------------------------------------------------
    // 13.2.5.53-68 DOCTYPE-Zustaende
    // ------------------------------------------------------------------

    fn st_doctype(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::Doctype => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeDoctypeName;
                    None
                }
                Some('>') => {
                    self.zustand = S::BeforeDoctypeName;
                    None
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.zustand = S::BeforeDoctypeName;
                    None
                }
            },
            S::BeforeDoctypeName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.doctype_quirlig = true;
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                Some('\0') => {
                    self.weiter(1);
                    self.name.clear();
                    self.name.push('\u{FFFD}');
                    self.zustand = S::DoctypeName;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.name.clear();
                    self.name.push(ch.to_ascii_lowercase());
                    self.zustand = S::DoctypeName;
                    None
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
            },
            S::DoctypeName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::AfterDoctypeName;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                Some('\0') => {
                    self.weiter(1);
                    self.name.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.name.push(ch.to_ascii_lowercase());
                    None
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
            },
            S::AfterDoctypeName => {
                if c.is_none() {
                    self.doctype_quirlig = true;
                    return Some(self.doctype_token());
                }
                // 13.2.5.56, woertlich: "U+003E GREATER-THAN SIGN (>) —
                // Switch to the data state. Emit the current DOCTYPE
                // token." — OHNE das Quirks-Flag zu setzen.
                //
                // GEMESSENER FEHLER (03.10.2026), Tokenizer-Abnahme
                // (test3.test): der Arm FEHLTE, `>` fiel damit in
                // "Anything else" ("invalid-character-sequence-after-
                // doctype-name … Set the current DOCTYPE token's
                // force-quirks flag to on. Reconsume in the bogus
                // DOCTYPE state.") — das Ergebnis war der Quirks-Flag:
                //   <!DOCTYPE a >  erwartet: korrekt
                //                 bekommen: Quirks (bogus DOCTYPE)
                //   <!DOCTYPEa >   dasselbe
                // Zwei Faelle. (Die aehnliche Fassung "abrupt-
                // doctype-public-identifier: Force the DOCTYPE
                // token's flags to on" hat der Spec entfernt; der
                // Korpus ist hier auf dem neuen Stand und deckt
                // sich mit 13.2.5.56.)
                if c == Some('>') {
                    self.weiter(1);
                    self.zustand = S::Data;
                    return Some(self.doctype_token());
                }
                if matches!(c, Some(ch) if ist_html_whitespace(ch)) {
                    self.weiter(1);
                    return None;
                }
                if self.beginnt_mit_ci("public") {
                    self.weiter(6);
                    self.zustand = S::AfterDoctypePublicKeyword;
                    return None;
                }
                if self.beginnt_mit_ci("system") {
                    self.weiter(6);
                    self.zustand = S::AfterDoctypeSystemKeyword;
                    return None;
                }
                self.doctype_quirlig = true;
                self.zustand = S::BogusDoctype;
                None
            }
            S::AfterDoctypePublicKeyword => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeDoctypePublicIdentifier;
                    None
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_public = Some(String::new());
                    self.zustand = S::DoctypePublicIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_public = Some(String::new());
                    self.zustand = S::DoctypePublicIdentifierSingleQuoted;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.doctype_quirlig = true;
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::BeforeDoctypePublicIdentifier => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_public = Some(String::new());
                    self.zustand = S::DoctypePublicIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_public = Some(String::new());
                    self.zustand = S::DoctypePublicIdentifierSingleQuoted;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.doctype_quirlig = true;
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::DoctypePublicIdentifierDoubleQuoted | S::DoctypePublicIdentifierSingleQuoted => {
                let anfuehrung = if self.zustand == S::DoctypePublicIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match c {
                    Some(ch) if ch == anfuehrung => {
                        self.weiter(1);
                        self.zustand = S::AfterDoctypePublicIdentifier;
                        None
                    }
                    Some('\0') => {
                        self.weiter(1);
                        if let Some(p) = self.doctype_public.as_mut() {
                            p.push('\u{FFFD}');
                        }
                        None
                    }
                    Some('>') => {
                        self.weiter(1);
                        self.doctype_quirlig = true;
                        self.zustand = S::Data;
                        Some(self.doctype_token())
                    }
                    Some(ch) => {
                        self.weiter(ch.len_utf8());
                        if let Some(p) = self.doctype_public.as_mut() {
                            p.push(ch);
                        }
                        None
                    }
                    None => {
                        self.doctype_quirlig = true;
                        Some(self.doctype_token())
                    }
                }
            }
            S::AfterDoctypePublicIdentifier => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BetweenDoctypePublicAndSystemIdentifiers;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierSingleQuoted;
                    None
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::BetweenDoctypePublicAndSystemIdentifiers => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierSingleQuoted;
                    None
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::AfterDoctypeSystemKeyword => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeDoctypeSystemIdentifier;
                    None
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierSingleQuoted;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.doctype_quirlig = true;
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::BeforeDoctypeSystemIdentifier => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('"') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.doctype_system = Some(String::new());
                    self.zustand = S::DoctypeSystemIdentifierSingleQuoted;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.doctype_quirlig = true;
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.doctype_quirlig = true;
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::DoctypeSystemIdentifierDoubleQuoted | S::DoctypeSystemIdentifierSingleQuoted => {
                let anfuehrung = if self.zustand == S::DoctypeSystemIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match c {
                    Some(ch) if ch == anfuehrung => {
                        self.weiter(1);
                        self.zustand = S::AfterDoctypeSystemIdentifier;
                        None
                    }
                    Some('\0') => {
                        self.weiter(1);
                        if let Some(s) = self.doctype_system.as_mut() {
                            s.push('\u{FFFD}');
                        }
                        None
                    }
                    Some('>') => {
                        self.weiter(1);
                        self.doctype_quirlig = true;
                        self.zustand = S::Data;
                        Some(self.doctype_token())
                    }
                    Some(ch) => {
                        self.weiter(ch.len_utf8());
                        if let Some(s) = self.doctype_system.as_mut() {
                            s.push(ch);
                        }
                        None
                    }
                    None => {
                        self.doctype_quirlig = true;
                        Some(self.doctype_token())
                    }
                }
            }
            S::AfterDoctypeSystemIdentifier => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => {
                    self.doctype_quirlig = true;
                    Some(self.doctype_token())
                }
                Some(_) => {
                    self.zustand = S::BogusDoctype;
                    None
                }
            },
            S::BogusDoctype => match c {
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.doctype_token())
                }
                None => Some(self.doctype_token()),
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    None
                }
            },
            _ => self.eof(),
        }
    }

    /// Baut das DOCTYPE-Token aus den Feldern.
    fn doctype_token(&mut self) -> SpecToken {
        // Am Eingabeende ist der Tokenizer fertig (sonst liefert der
        // naechste Aufruf dasselbe Token noch einmal).
        if self.aktuell().is_none() {
            self.eof_aus = true;
        }
        let name = if self.name.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.name))
        };
        SpecToken::Doctype {
            name,
            public_id: self.doctype_public.take(),
            system_id: self.doctype_system.take(),
            quirlig: self.doctype_quirlig,
        }
    }

    // ------------------------------------------------------------------
    // 13.2.5.41 und 13.2.5.43-52 Kommentar-Zustaende
    // ------------------------------------------------------------------

    fn st_kommentar(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            // 13.2.5.72 - 13.2.5.76, woertlich umgesetzt.
            S::BogusComment => match c {
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(SpecToken::Comment(std::mem::take(&mut self.kommentar)))
                }
                Some('\0') => {
                    self.weiter(1);
                    self.kommentar.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.kommentar.push(ch);
                    None
                }
            },
            S::MarkupDeclarationOpen => {
                if self.beginnt_mit("--") {
                    self.weiter(2);
                    self.kommentar.clear();
                    self.zustand = S::CommentStart;
                } else if self.beginnt_mit_ci("doctype") {
                    self.weiter(7);
                    self.doctype_zuruecksetzen();
                    self.zustand = S::Doctype;
                } else if self.beginnt_mit("[CDATA[") {
                    self.weiter(7);
                    if self.cdata_erlaubt {
                        self.zustand = S::CdataSection;
                    } else {
                        self.kommentar.clear();
                        self.kommentar.push_str("[CDATA[");
                        self.zustand = S::BogusComment;
                    }
                } else {
                    self.kommentar.clear();
                    self.zustand = S::BogusComment;
                }
                None
            }
            S::CommentStart => {
                if c == Some('-') {
                    self.weiter(1);
                    self.zustand = S::CommentStartDash;
                } else if c == Some('>') {
                    self.weiter(1);
                    self.zustand = S::Data;
                    return Some(SpecToken::Comment(std::mem::take(&mut self.kommentar)));
                } else {
                    self.zustand = S::Comment;
                }
                None
            }
            S::CommentStartDash => match c {
                Some('-') => {
                    self.weiter(1);
                    self.zustand = S::CommentEnd;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(SpecToken::Comment(std::mem::take(&mut self.kommentar)))
                }
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some(_) => {
                    self.kommentar.push('-');
                    self.zustand = S::Comment;
                    None
                }
            },
            S::Comment => match c {
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some('<') => {
                    self.weiter(1);
                    self.kommentar.push('<');
                    self.zustand = S::CommentLessThanSign;
                    None
                }
                Some('-') => {
                    self.weiter(1);
                    self.zustand = S::CommentEndDash;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.kommentar.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.kommentar.push(ch);
                    None
                }
            },
            S::CommentLessThanSign => {
                if c == Some('!') {
                    self.weiter(1);
                    self.kommentar.push('!');
                    self.zustand = S::CommentLessThanSignBang;
                } else if c == Some('<') {
                    self.weiter(1);
                    self.kommentar.push('<');
                } else {
                    self.zustand = S::Comment;
                }
                None
            }
            S::CommentLessThanSignBang => {
                if c == Some('-') {
                    self.weiter(1);
                    self.zustand = S::CommentLessThanSignBangDash;
                } else {
                    self.zustand = S::Comment;
                }
                None
            }
            S::CommentLessThanSignBangDash => {
                if c == Some('-') {
                    self.weiter(1);
                    self.zustand = S::CommentLessThanSignBangDashDash;
                } else {
                    self.zustand = S::CommentEndDash;
                }
                None
            }
            S::CommentLessThanSignBangDashDash => {
                self.zustand = S::CommentEnd;
                None
            }
            S::CommentEndDash => match c {
                Some('-') => {
                    self.weiter(1);
                    self.zustand = S::CommentEnd;
                    None
                }
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some(_) => {
                    self.kommentar.push('-');
                    self.zustand = S::Comment;
                    None
                }
            },
            S::CommentEnd => match c {
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(SpecToken::Comment(std::mem::take(&mut self.kommentar)))
                }
                Some('!') => {
                    self.weiter(1);
                    self.zustand = S::CommentEndBang;
                    None
                }
                Some('-') => {
                    self.weiter(1);
                    self.kommentar.push('-');
                    None
                }
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some(_) => {
                    self.kommentar.push('-');
                    self.kommentar.push('-');
                    self.zustand = S::Comment;
                    None
                }
            },
            S::CommentEndBang => match c {
                Some('-') => {
                    self.weiter(1);
                    self.kommentar.push('-');
                    self.kommentar.push('-');
                    self.kommentar.push('!');
                    self.zustand = S::CommentEndDash;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(SpecToken::Comment(std::mem::take(&mut self.kommentar)))
                }
                None => {
                    let k = std::mem::take(&mut self.kommentar);
                    self.wartend = self.eof();
                    Some(SpecToken::Comment(k))
                }
                Some(_) => {
                    self.kommentar.push('-');
                    self.kommentar.push('-');
                    self.kommentar.push('!');
                    self.zustand = S::Comment;
                    None
                }
            },
            _ => self.eof(),
        }
    }

    /// Setzt die DOCTYPE-Felder zurueck.
    fn doctype_zuruecksetzen(&mut self) {
        self.doctype_public = None;
        self.doctype_system = None;
        self.doctype_quirlig = false;
        self.name.clear();
    }

    // ------------------------------------------------------------------
    // 13.2.5.4 und 13.2.5.15-31 Script-Daten
    // ------------------------------------------------------------------

    fn st_script(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::ScriptData => match c {
                None => self.eof(),
                Some('<') => {
                    self.weiter(1);
                    self.zustand = S::ScriptDataLessThanSign;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    None
                }
            },
            S::ScriptDataLessThanSign => {
                if c == Some('/') {
                    self.uebergang.clear();
                    self.weiter(1);
                    self.zustand = S::ScriptDataEndTagOpen;
                } else if c == Some('!') {
                    self.weiter(1);
                    self.zeichen('<');
                    self.zeichen('!');
                    self.zustand = S::ScriptDataEscapeStart;
                } else {
                    self.zeichen('<');
                    self.zustand = S::ScriptData;
                }
                None
            }
            S::ScriptDataEndTagOpen => match c {
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.ist_end_tag = true;
                    self.name.clear();
                    self.name.push(ch.to_ascii_lowercase());
                    self.uebergang.clear();
                    self.uebergang.push(ch);
                    self.weiter(1);
                    self.zustand = S::ScriptDataEndTagName;
                    None
                }
                _ => {
                    self.zeichen('<');
                    self.zeichen('/');
                    self.zustand = S::ScriptData;
                    None
                }
            },
            S::ScriptDataEndTagName => self.end_tag_name_schritt(c, S::ScriptData),
            S::ScriptDataEscapeStart => {
                if c == Some('-') {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataEscapeStartDash;
                } else {
                    self.zustand = S::ScriptData;
                }
                None
            }
            S::ScriptDataEscapeStartDash => {
                if c == Some('-') {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataEscapedDashDash;
                } else {
                    self.zustand = S::ScriptData;
                }
                None
            }
            S::ScriptDataEscaped => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataEscapedDash;
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zustand = S::ScriptDataEscapedLessThanSign;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    None
                }
            },
            S::ScriptDataEscapedDash => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataEscapedDashDash;
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zustand = S::ScriptDataEscapedLessThanSign;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    self.zustand = S::ScriptDataEscaped;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    self.zustand = S::ScriptDataEscaped;
                    None
                }
            },
            S::ScriptDataEscapedDashDash => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zustand = S::ScriptDataEscapedLessThanSign;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zeichen('>');
                    self.zustand = S::ScriptData;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    self.zustand = S::ScriptDataEscaped;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    self.zustand = S::ScriptDataEscaped;
                    None
                }
            },
            S::ScriptDataEscapedLessThanSign => {
                if c == Some('/') {
                    self.uebergang.clear();
                    self.weiter(1);
                    self.zustand = S::ScriptDataEscapedEndTagOpen;
                } else if matches!(c, Some(ch) if ch.is_ascii_alphabetic()) {
                    // Reconsume: das Zeichen wird NICHT verbraucht; die
                    // double-escape-start-Runde haengt es an den Puffer.
                    self.uebergang.clear();
                    self.zeichen('<');
                    self.zustand = S::ScriptDataDoubleEscapeStart;
                } else {
                    self.zeichen('<');
                    self.zustand = S::ScriptDataEscaped;
                }
                None
            }
            S::ScriptDataEscapedEndTagOpen => match c {
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.ist_end_tag = true;
                    self.name.clear();
                    self.name.push(ch.to_ascii_lowercase());
                    self.uebergang.clear();
                    self.uebergang.push(ch);
                    self.weiter(1);
                    self.zustand = S::ScriptDataEscapedEndTagName;
                    None
                }
                _ => {
                    self.zeichen('<');
                    self.zeichen('/');
                    self.zustand = S::ScriptDataEscaped;
                    None
                }
            },
            S::ScriptDataEscapedEndTagName => self.end_tag_name_schritt(c, S::ScriptDataEscaped),
            S::ScriptDataDoubleEscapeStart | S::ScriptDataDoubleEscapeEnd => {
                let doppelt = self.zustand == S::ScriptDataDoubleEscapeStart;
                let basis = if doppelt {
                    S::ScriptDataDoubleEscaped
                } else {
                    S::ScriptDataEscaped
                };
                let gegen = if doppelt {
                    S::ScriptDataEscaped
                } else {
                    S::ScriptDataDoubleEscaped
                };
                match c {
                    Some(ch) if ist_html_whitespace(ch) || ch == '/' || ch == '>' => {
                        self.weiter(1);
                        self.zeichen(ch);
                        self.zustand = if self.uebergang == "script" {
                            basis
                        } else {
                            gegen
                        };
                        None
                    }
                    Some(ch) if ch.is_ascii_alphabetic() => {
                        self.uebergang.push(ch.to_ascii_lowercase());
                        self.weiter(1);
                        self.zeichen(ch);
                        None
                    }
                    _ => {
                        self.zustand = gegen;
                        None
                    }
                }
            }
            S::ScriptDataDoubleEscaped => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataDoubleEscapedDash;
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zeichen('<');
                    self.zustand = S::ScriptDataDoubleEscapedLessThanSign;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    None
                }
            },
            S::ScriptDataDoubleEscapedDash => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    self.zustand = S::ScriptDataDoubleEscapedDashDash;
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zeichen('<');
                    self.zustand = S::ScriptDataDoubleEscapedLessThanSign;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    self.zustand = S::ScriptDataDoubleEscaped;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    self.zustand = S::ScriptDataDoubleEscaped;
                    None
                }
            },
            S::ScriptDataDoubleEscapedDashDash => match c {
                None => self.eof(),
                Some('-') => {
                    self.weiter(1);
                    self.zeichen('-');
                    None
                }
                Some('<') => {
                    self.weiter(1);
                    self.zeichen('<');
                    self.zustand = S::ScriptDataDoubleEscapedLessThanSign;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zeichen('>');
                    self.zustand = S::ScriptData;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    self.zustand = S::ScriptDataDoubleEscaped;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    self.zustand = S::ScriptDataDoubleEscaped;
                    None
                }
            },
            S::ScriptDataDoubleEscapedLessThanSign => {
                if c == Some('/') {
                    self.uebergang.clear();
                    self.weiter(1);
                    self.zeichen('/');
                    self.zustand = S::ScriptDataDoubleEscapeEnd;
                } else {
                    self.zustand = S::ScriptDataDoubleEscaped;
                }
                None
            }
            _ => self.eof(),
        }
    }

    // ------------------------------------------------------------------
    // 13.2.5.1 Data, 13.2.5.2 RCDATA, 13.2.5.3 RAWTEXT, 13.2.5.5
    // PLAINTEXT, 13.2.5.69-71 CDATA section.
    // ------------------------------------------------------------------

    fn st_data(&mut self, c: Option<char>) -> Option<SpecToken> {
        match c {
            None => self.eof(),
            Some('&') => {
                self.rueckkehr = Rueckkehr::Data;
                self.in_attribut = false;
                self.weiter(1);
                self.zustand = S::CharacterReference;
                None
            }
            Some('<') => {
                self.weiter(1);
                self.zustand = S::TagOpen;
                None
            }
            // Data state: U+0000 ist "unexpected-null-character" und wird
            // als ZEICHEN-Token ausgegeben (der Baumaufbau verwirft es).
            // In RAWTEXT/RCDATA/Script/PLAINTEXT ist U+FFFD richtig.
            Some('\0') => {
                self.weiter(1);
                self.zeichen('\0');
                None
            }
            Some(ch) => {
                self.weiter(ch.len_utf8());
                self.zeichen(ch);
                None
            }
        }
    }

    fn st_roh(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::Rcdata | S::Rawtext => {
                let basis = self.zustand;
                match c {
                    None => self.eof(),
                    Some('&') if basis == S::Rcdata => {
                        self.rueckkehr = Rueckkehr::Rcdata;
                        self.in_attribut = false;
                        self.weiter(1);
                        self.zustand = S::CharacterReference;
                        None
                    }
                    Some('<') => {
                        self.weiter(1);
                        self.zustand = if basis == S::Rcdata {
                            S::RcdataLessThanSign
                        } else {
                            S::RawtextLessThanSign
                        };
                        None
                    }
                    Some('\0') => {
                        self.weiter(1);
                        self.zeichen('\u{FFFD}');
                        None
                    }
                    Some(ch) => {
                        self.weiter(ch.len_utf8());
                        self.zeichen(ch);
                        None
                    }
                }
            }
            S::RcdataLessThanSign | S::RawtextLessThanSign => {
                let basis = if self.zustand == S::RcdataLessThanSign {
                    S::Rcdata
                } else {
                    S::Rawtext
                };
                if c == Some('/') {
                    self.uebergang.clear();
                    self.weiter(1);
                    self.zustand = if basis == S::Rcdata {
                        S::RcdataEndTagOpen
                    } else {
                        S::RawtextEndTagOpen
                    };
                } else {
                    self.zeichen('<');
                    self.zustand = basis;
                }
                None
            }
            S::RcdataEndTagOpen | S::RawtextEndTagOpen => {
                let basis = if self.zustand == S::RcdataEndTagOpen {
                    S::Rcdata
                } else {
                    S::Rawtext
                };
                match c {
                    Some(ch) if ch.is_ascii_alphabetic() => {
                        self.ist_end_tag = true;
                        self.name.clear();
                        self.name.push(ch.to_ascii_lowercase());
                        self.uebergang.clear();
                        self.uebergang.push(ch);
                        self.weiter(1);
                        self.zustand = if basis == S::Rcdata {
                            S::RcdataEndTagName
                        } else {
                            S::RawtextEndTagName
                        };
                    }
                    _ => {
                        self.zeichen('<');
                        self.zeichen('/');
                        self.zustand = basis;
                    }
                }
                None
            }
            S::RcdataEndTagName | S::RawtextEndTagName => {
                let basis = if self.zustand == S::RcdataEndTagName {
                    S::Rcdata
                } else {
                    S::Rawtext
                };
                self.end_tag_name_schritt(c, basis)
            }
            S::Plaintext => match c {
                None => self.eof(),
                Some('\0') => {
                    self.weiter(1);
                    self.zeichen('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    None
                }
            },
            S::CdataSection => match c {
                None => self.eof(),
                Some(']') => {
                    self.weiter(1);
                    self.zustand = S::CdataSectionBracket;
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.zeichen(ch);
                    None
                }
            },
            S::CdataSectionBracket => {
                if c == Some(']') {
                    self.weiter(1);
                    self.zustand = S::CdataSectionEnd;
                } else {
                    self.zeichen(']');
                    self.zustand = S::CdataSection;
                }
                None
            }
            S::CdataSectionEnd => {
                if c == Some('>') {
                    self.weiter(1);
                    self.zustand = S::Data;
                } else if c == Some(']') {
                    self.weiter(1);
                    self.zeichen(']');
                } else {
                    self.zeichen(']');
                    self.zeichen(']');
                    self.zustand = S::CdataSection;
                }
                None
            }
            _ => self.eof(),
        }
    }

    /// Ein Schritt in RCDATA/RAWTEXT end tag name (13.2.5.11/13.2.5.14).
    fn end_tag_name_schritt(&mut self, c: Option<char>, basis: S) -> Option<SpecToken> {
        match c {
            Some(ch) if ch.is_ascii_alphabetic() => {
                self.name.push(ch.to_ascii_lowercase());
                self.uebergang.push(ch);
                self.weiter(1);
                None
            }
            Some(ch) if ist_html_whitespace(ch) => {
                if self.ist_angemessen() {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeName;
                } else {
                    self.flush_endtag(basis);
                }
                None
            }
            Some('/') => {
                if self.ist_angemessen() {
                    self.weiter(1);
                    self.zustand = S::SelfClosingStartTag;
                } else {
                    self.flush_endtag(basis);
                }
                None
            }
            Some('>') if self.ist_angemessen() => {
                self.weiter(1);
                self.zustand = S::Data;
                Some(SpecToken::EndTag {
                    name: std::mem::take(&mut self.name),
                })
            }
            Some('>') => {
                self.flush_endtag(basis);
                None
            }
            _ => {
                self.flush_endtag(basis);
                None
            }
        }
    }

    /// Ist das aktuelle End-Tag "appropriate" (13.2.4.5)?
    fn ist_angemessen(&self) -> bool {
        self.letzter_start_tag.as_deref() == Some(self.name.as_str())
    }

    /// Gibt `<`, `/` und den Uebergangspuffer als Zeichen aus und kehrt
    /// in den Basis-Zustand zurueck (nicht konsumieren!).
    fn flush_endtag(&mut self, basis: S) {
        self.zeichen('<');
        self.zeichen('/');
        let u = std::mem::take(&mut self.uebergang);
        self.zeichen_str(&u);
        self.name.clear();
        self.zustand = basis;
    }

    // ------------------------------------------------------------------
    // 13.2.5.6-40 Tag- und Attribut-Zustaende
    // ------------------------------------------------------------------

    fn st_tag(&mut self, c: Option<char>) -> Option<SpecToken> {
        match self.zustand {
            S::TagOpen => match c {
                Some('!') => {
                    self.weiter(1);
                    self.zustand = S::MarkupDeclarationOpen;
                    None
                }
                Some('/') => {
                    self.weiter(1);
                    self.zustand = S::EndTagOpen;
                    None
                }
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.ist_end_tag = false;
                    self.name.clear();
                    self.name.push(ch.to_ascii_lowercase());
                    self.weiter(1);
                    self.zustand = S::TagName;
                    None
                }
                Some('?') => {
                    // 13.2.5.4 Markup declaration open state: "U+003F
                    // QUESTION MARK (?): Set the temporary buffer to
                    // the empty string. Switch to the processing
                    // instruction open state."
                    //
                    // WICHTIG und gemessen: das Fragezeichen wird
                    // VERBRAUCHT, der Puffer ist danach LEER. Der alte
                    // Weg in den BogusComment hat es nicht verbraucht,
                    // deshalb stand in drei Faellen von comments01.dat
                    // ein `?` ZU VIEL im Kommentar:
                    //   <?xml version="1.0">
                    //     erwartet: <!-- ?xml version="1.0" -->
                    //     bekommen: <!-- xml version="1.0" -->
                    // Der Testkorpus ist hier aelter als der Spec und
                    // erwartet das `?` — der Spec gewinnt, und die drei
                    // Faelle werden als Restfall ausgewiesen (siehe
                    // team/BERICHT_CACHE_A1_SPECVERSION.txt).
                    // Das Fragezeichen bleibt im Puffer. Es ist der
                    // ERSTE Buchstabe des Kommentars, den die
                    // PI-Fehlerregeln erzeugen — wortlich in 13.2.3
                    // (invalid-first-character-of-processing-
                    // instruction-target): "The preceding U+003F (?)
                    // and all content that follows up to a U+003E (>)
                    // (if present) or to the end of the input stream is
                    // treated as a comment."
                    //
                    // GEMESSENER FEHLER (01.10.2026): es stand hier
                    // `self.kommentar.clear()`, das `?` ging verloren,
                    // und drei Faelle in tests1.dat waren deswegen mit
                    // einem "Restfall" entschuldigt statt behoben:
                    //   <?#   erwartet <!-- ?# -->   bekommen <!-- # -->
                    self.kommentar.clear();
                    self.kommentar.push('?');
                    self.weiter(1);
                    self.zustand = S::ProcessingInstructionOpen;
                    None
                }
                None => {
                    self.zeichen('<');
                    self.eof()
                }
                Some(_) => {
                    self.zeichen('<');
                    self.zustand = S::Data;
                    None
                }
            },
            S::EndTagOpen => match c {
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.ist_end_tag = true;
                    self.name.clear();
                    self.name.push(ch.to_ascii_lowercase());
                    self.weiter(1);
                    self.zustand = S::TagName;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    None
                }
                None => {
                    self.zeichen('<');
                    self.zeichen('/');
                    self.eof()
                }
                Some(_) => {
                    self.kommentar.clear();
                    self.zustand = S::BogusComment;
                    None
                }
            },
            S::TagName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeName;
                    None
                }
                Some('/') => {
                    self.weiter(1);
                    self.zustand = S::SelfClosingStartTag;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                Some('\0') => {
                    self.weiter(1);
                    self.name.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.name.push(ch.to_ascii_lowercase());
                    None
                }
                None => self.eof(),
            },
            S::BeforeAttributeName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('=') => {
                    self.attribut_neu();
                    self.attribut_name.push('=');
                    self.weiter(1);
                    self.zustand = S::AttributeName;
                    None
                }
                Some(_) => {
                    self.attribut_neu();
                    self.zustand = S::AttributeName;
                    None
                }
                None => {
                    self.zustand = S::AfterAttributeName;
                    None
                }
            },
            S::AttributeName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::AfterAttributeName;
                    None
                }
                Some('/') | Some('>') => {
                    self.zustand = S::AfterAttributeName;
                    None
                }
                Some('=') => {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeValue;
                    None
                }
                Some('\0') => {
                    self.weiter(1);
                    self.attribut_name.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.attribut_name.push(ch.to_ascii_lowercase());
                    None
                }
                None => {
                    self.zustand = S::AfterAttributeName;
                    None
                }
            },
            S::AfterAttributeName => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('/') => {
                    self.weiter(1);
                    self.zustand = S::SelfClosingStartTag;
                    None
                }
                Some('=') => {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeValue;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                None => self.eof(),
                Some(_) => {
                    self.attribut_neu();
                    self.zustand = S::AttributeName;
                    None
                }
            },
            S::BeforeAttributeValue => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    None
                }
                Some('"') => {
                    self.weiter(1);
                    self.zustand = S::AttributeValueDoubleQuoted;
                    None
                }
                Some('\'') => {
                    self.weiter(1);
                    self.zustand = S::AttributeValueSingleQuoted;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                Some(_) => {
                    self.zustand = S::AttributeValueUnquoted;
                    None
                }
                None => {
                    self.zustand = S::AttributeValueUnquoted;
                    None
                }
            },
            S::AttributeValueDoubleQuoted | S::AttributeValueSingleQuoted => {
                let anfuehrung = if self.zustand == S::AttributeValueDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match c {
                    Some(ch) if ch == anfuehrung => {
                        self.weiter(1);
                        self.zustand = S::AfterAttributeValueQuoted;
                        None
                    }
                    Some('&') => {
                        self.rueckkehr = if anfuehrung == '"' {
                            Rueckkehr::DoppelAnfuehrung
                        } else {
                            Rueckkehr::EinzelAnfuehrung
                        };
                        self.in_attribut = true;
                        self.weiter(1);
                        self.zustand = S::CharacterReference;
                        None
                    }
                    Some('\0') => {
                        self.weiter(1);
                        self.attribut_wert.push('\u{FFFD}');
                        None
                    }
                    Some(ch) => {
                        self.weiter(ch.len_utf8());
                        self.attribut_wert.push(ch);
                        None
                    }
                    None => self.eof(),
                }
            }
            S::AttributeValueUnquoted => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeName;
                    None
                }
                Some('&') => {
                    self.rueckkehr = Rueckkehr::OhneAnfuehrung;
                    self.in_attribut = true;
                    self.weiter(1);
                    self.zustand = S::CharacterReference;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                Some('\0') => {
                    self.weiter(1);
                    self.attribut_wert.push('\u{FFFD}');
                    None
                }
                Some(ch) => {
                    self.weiter(ch.len_utf8());
                    self.attribut_wert.push(ch);
                    None
                }
                None => self.eof(),
            },
            S::AfterAttributeValueQuoted => match c {
                Some(ch) if ist_html_whitespace(ch) => {
                    self.weiter(1);
                    self.zustand = S::BeforeAttributeName;
                    None
                }
                Some('/') => {
                    self.weiter(1);
                    self.zustand = S::SelfClosingStartTag;
                    None
                }
                Some('>') => {
                    self.weiter(1);
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                None => self.eof(),
                Some(_) => {
                    self.zustand = S::BeforeAttributeName;
                    None
                }
            },
            S::SelfClosingStartTag => match c {
                Some('>') => {
                    self.weiter(1);
                    self.selbst_schliessend = true;
                    self.zustand = S::Data;
                    Some(self.tag_beenden())
                }
                None => self.eof(),
                Some(_) => {
                    self.zustand = S::BeforeAttributeName;
                    None
                }
            },
            _ => self.eof(),
        }
    }

    /// Beginnt ein neues Attribut.
    fn attribut_neu(&mut self) {
        self.attribut_speichern();
        self.attribut_name.clear();
        self.attribut_wert.clear();
        self.attribut_duplikat = false;
    }

    /// Ein fertiges Attribut wird vor dem naechsten gespeichert. Bei
    /// doppeltem Namen bleibt gemaess HTML-Tokenizer der erste Wert.
    fn attribut_speichern(&mut self) {
        if self.attribut_name.is_empty() || self.attribut_duplikat {
            return;
        }
        let name = std::mem::take(&mut self.attribut_name);
        let wert = std::mem::take(&mut self.attribut_wert);
        // GEMESSENER FEHLER (03.10.2026): der Duplikatscheck war
        // `self.attribute.iter().any(...)` — ein Scan ueber ALLE
        // bisherigen Attribute, also quadratisch. Gemessen an einem
        // Tag mit N Attributen:
        //     2 000 Attribute   0,016 s
        //     4 000 Attribute   0,071 s   (x 4,4)
        //     8 000 Attribute   0,290 s   (x 4,1)
        //    16 000 Attribute   0,931 s   (x 3,2)
        //    50 000 Attribute  ca. 11 s
        // Das verletzt M8 ("jede muss in unter einer Sekunde enden").
        //
        // LOESUNG: die Hashes der Namen in einer HashSet. Ist der Hash
        // nicht dabei, ist der Name definitiv neu (O(1)). Ist er dabei,
        // geht der lineare Scan ueber den EXAKTEN Namen — sonst wuerde
        // eine Hash-Kollision ein Attribut verschlucken, und
        // Datenverlust ist schlimmer als Zeitverlust.
        //
        // ZWEITER SCHRITT, ebenfalls gemessen: ein Vec<u32> statt einer
        // HashSet war immer noch quadratisch (der Vec wird linear
        // durchsucht) — 16 000 Attribute: 0,855 s gegen 0,931 s, also
        // nur zehnmal schneller pro Vergleich, aber dieselbe Ordnung.
        // Mit HashSet ist die Suche O(1).
        let h = fnv1a(&name);
        let vorhanden = if self.attribut_hashes.contains(&h) {
            self.attribute.iter().any(|(n, _)| *n == name)
        } else {
            false
        };
        if !vorhanden {
            self.attribute.push((name, wert));
            self.attribut_hashes.insert(h);
        }
    }

    /// Schliesst das laufende Attribut ab und baut das Tag-Token.
    fn tag_beenden(&mut self) -> SpecToken {
        self.attribut_speichern();
        // Die Hashes gehoeren ZU DIESEM Tag (siehe `attribut_speichern`).
        // Ohne das Leeren wuerde der erste Attributname des naechsten
        // Tags mit dem des vorigen verwechselt.
        self.attribut_hashes.clear();
        self.attribut_name.clear();
        self.attribut_wert.clear();
        let selbst = self.selbst_schliessend;
        self.selbst_schliessend = false;
        if self.ist_end_tag {
            self.ist_end_tag = false;
            self.attribute.clear();
            SpecToken::EndTag {
                name: std::mem::take(&mut self.name),
            }
        } else {
            SpecToken::StartTag {
                name: std::mem::take(&mut self.name),
                attribute: std::mem::take(&mut self.attribute),
                selbst_schliessend: selbst,
            }
        }
    }
}

/// FNV-1a (32 Bit) ueber die Bytes — kein Fremd-Crate (ADR-030), und
/// eine reine Beschleunigung: die Gleichheit entscheidet nie ueber ein
/// Attribut, das gespeichert wird.
fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// HTML-Leerraum (13.2.5: tab, LF, FF, CR, space).
fn ist_html_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0C}' | '\r' | ' ')
}

/// Sucht den laengsten Namen aus der Entitaetentabelle, der ein Praefix
/// des Rests ist (13.2.5.78: "consume the maximum number of characters").
/// findet einen Entitaetennamen im Rest des Eingabepuffers.
///
/// findet einen Entitaetennamen im Rest des Eingabepuffers.
///
/// DIE SCHREIBWEISE IST TEIL DES NAMENS — und das stand hier von
/// 03.10.2026 bis 03.10.2026 abends FALSCH herum. Korrigiert am
/// 03.10.2026 nach der Primaerquelle, Beleg:
/// `tests/entitaeten_schreibweise.rs`.
///
/// 13.2.5.78 (Named character reference state), woertlich: "Consume
/// the maximum number of characters possible, where the consumed
/// characters ARE ONE OF THE IDENTIFIERS in the first column of the
/// named character references table." Das ist Zugehoerigkeit, keine
/// Case-Regel; im Abschnitt kommt "case" nicht vor.
///
/// 13.1.4 (Character references), woertlich: "The ampersand must be
/// followed by one of the names given in the named character
/// references section, USING THE SAME CASE." Das ist die einzige
/// Stelle der Spec, die zur Schreibweise sagt — und sie sagt: gleich.
///
/// `grep -in "case-insensitive"` im geholten parsing.html, gefiltert
/// auf "character reference|entit|named": NULL Treffer. Der Ausdruck
/// kommt im Spec dreissigmal vor (DOCTYPE, PUBLIC/SYSTEM, xml,
/// encoding, hidden), aber NIE bei Zeichenreferenzen.
///
/// WAS ICH FALSCH BEHAUPTET HATTE und was die Korrektur kostete:
/// ich hatte behauptet, die Spec verlange "ASCII case-insensitive
/// match", und daran zwei Reparaturrunden gebaut. Beide sind
/// zurueckgenommen (Kosten: 464 und dann 58 Tokenizer-Faelle). Der
/// Byte-Vergleich hier ist richtig, `&Amp;` bleibt Literaltext, und
/// das ist das verlangte Verhalten. Wer diese Frage noch einmal
/// aufmacht, hat zwei Argumente: die beiden Zitate oben und die
/// Zahlen 464/58.
///
/// GEMESSEN an der Tabelle, damit die Groessenordnung greifbar ist:
/// 106 Namen ohne Semikolon, davon genau ACHT gross (AMP, COPY, ETH,
/// GT, LT, QUOT, REG, THORN). Und `Gt;` ist U+226B (≫), waehrend
/// `GT;` U+003E (>) ist — die Schreibweise aendert das Zeichen, sie
/// ist also nicht Kosmetik.
fn finde_entitaet(rest: &str) -> Option<(&'static str, &'static str)> {
    let mut ende = rest.len().min(MAX_ENTITAETSNAME);
    while ende > 0 {
        if !rest.is_char_boundary(ende) {
            ende -= 1;
            continue;
        }
        let probe = &rest[..ende];
        if let Ok(i) = ENTITAETEN.binary_search_by(|(name, _)| name.cmp(&probe)) {
            return Some(ENTITAETEN[i]);
        }
        ende -= 1;
    }
    None
}

/// Numerische Zeichenreferenz (13.2.5.84): Null, Surrogate und Werte
/// ueber U+10FFFF werden U+FFFD; die C1-Steuerzeichen 0x80..0x9F werden
/// nach windows-1252 abgebildet; alles andere bleibt.
fn numerisch_zu_zeichen(wert: u32) -> char {
    const C1: [u16; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
        0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];
    match wert {
        0 => '\u{FFFD}',
        0xD800..=0xDFFF => '\u{FFFD}',
        w if w > 0x10FFFF => '\u{FFFD}',
        0x80..=0x9F => char::from_u32(C1[(wert - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
        w => char::from_u32(w).unwrap_or('\u{FFFD}'),
    }
}
