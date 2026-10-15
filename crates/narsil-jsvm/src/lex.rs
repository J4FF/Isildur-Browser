// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Lexer (J1, ES2020-Naeherung): ASI, Template-Strings, einfache
//! Regex-Literale, Unicode-Bezeichner, alle Zahlenformen. Fehler tragen
//! Zeile/Spalte. Alle Schleifen haben harte Obergrenzen (M3/M4).

use crate::fehler::JsFehler;
use crate::grenzen::{MAX_QUELLTEXT_HART, MAX_TOKEN_HART};

/// Obergrenze der Zeichen, die EIN Token lang sein darf (M4): schuetzt vor
/// `'"' + 'x'*1e9`-Eingaben, bevor alloziert wird.
pub(crate) const MAX_TOKEN_LAENGE: usize = 1_000_000;
/// Maximale Tiefe geschachtelter Template-`${}`-Ausdruecke im Lexer (M5).
const MAX_TEMPLATE_TIEFE: usize = 64;

/// Ein Token mit Position (1-basiert, Spalte in Zeichen).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub art: TokenArt,
    pub zeile: u32,
    pub spalte: u32,
    /// `true`, wenn unmittelbar davor ein Zeilenumbruch (oder Kommentar mit
    /// Umbruch) stand — Grundlage fuer ASI.
    pub umbruch_davor: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenArt {
    Bezeichner(String),
    BezeichnerEscape(String),
    Zahl(f64),
    /// J24: BigInt-Literal `123n` (dezimal oder mit Basis-Praefix). Der
    /// Wert liegt in `i128`; groessere Werte sind ein RangeError (siehe
    /// `ganz.rs`).
    Ganz(i128),
    Zeichenkette(String),
    /// Roher Template-Abschnitt zwischen Backticks/`${}` (gekochte Form:
    /// Escapes sind aufgeloest).
    /// Release 15.10.2026: with the raw text beside it (see `TemplateText`).
    TemplateStueck(Box<TemplateText>),
    Regex {
        muster: String,
        schalter: String,
    },
    /// Schluesselwort (`if`, `function`, ...).
    Wort(Schluessel),
    Zeichen(Zeichen),
    /// Kuenstliches Zeilenende fuer ASI.
    StrichpunktErsatz,
    Ende,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Schluessel {
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Enum,
    Export,
    Extends,
    False,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    Instanceof,
    Let,
    New,
    Null,
    Return,
    Super,
    Switch,
    This,
    Throw,
    True,
    Try,
    Typeof,
    Var,
    Void,
    While,
    With,
    Yield,
    Static,
    Get,
    Set,
    Of,
    Target,
    Async,
    Await,
    From,
    As,
    Constructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zeichen {
    LRunde,
    RRunde,
    LGeschweift,
    RGeschweift,
    LEckig,
    REckig,
    Semikolon,
    Komma,
    Punkt,
    Frage,
    DoppelFrage,
    Pfeil,
    Plus,
    PlusPlus,
    PlusGleich,
    Minus,
    MinusMinus,
    MinusGleich,
    Stern,
    SternStern,
    SternGleich,
    SternSternGleich,
    Schrag,
    SchragGleich,
    Prozent,
    ProzentGleich,
    Gleich,
    GleichGleich,
    GleichGleichGleich,
    Ruf,
    RufGleich,
    RufGleichGleich,
    Kleiner,
    KleinerGleich,
    KleinerKleiner,
    KleinerKleinerGleich,
    Groesser,
    GroesserGleich,
    GroesserGroesser,
    GroesserGroesserGleich,
    GroesserGroesserGroesser,
    GroesserGroesserGroesserGleich,
    Und,
    UndUnd,
    UndGleich,
    UndUndGleich,
    Oder,
    OderOder,
    OderGleich,
    OderOderGleich,
    FrageDoppelGleich,
    SterneFrageGleich,
    SchragFrageGleich,
    ProzentFrageGleich,
    FragePunkt,
    DoppelPunkt,
    Tilde,
    /// J11: `#` — nur fuer private Namen (`#x`, `this.#x`, `#x in o`).
    Raute,
    TildeGleich,
    Hoch,
    HochGleich,
    DollarLGeschweift,
    Backtick,
    Punkte3,
    DoppelPunktFrage,
}

pub(crate) fn schluesselwort(s: &str) -> Option<Schluessel> {
    Some(match s {
        "break" => Schluessel::Break,
        "case" => Schluessel::Case,
        "catch" => Schluessel::Catch,
        "class" => Schluessel::Class,
        "const" => Schluessel::Const,
        "continue" => Schluessel::Continue,
        "debugger" => Schluessel::Debugger,
        "default" => Schluessel::Default,
        "delete" => Schluessel::Delete,
        "do" => Schluessel::Do,
        "else" => Schluessel::Else,
        "enum" => Schluessel::Enum,
        "export" => Schluessel::Export,
        "extends" => Schluessel::Extends,
        "false" => Schluessel::False,
        "finally" => Schluessel::Finally,
        "for" => Schluessel::For,
        "function" => Schluessel::Function,
        "if" => Schluessel::If,
        "import" => Schluessel::Import,
        "in" => Schluessel::In,
        "instanceof" => Schluessel::Instanceof,
        "let" => Schluessel::Let,
        "new" => Schluessel::New,
        "null" => Schluessel::Null,
        "return" => Schluessel::Return,
        "super" => Schluessel::Super,
        "switch" => Schluessel::Switch,
        "this" => Schluessel::This,
        "throw" => Schluessel::Throw,
        "true" => Schluessel::True,
        "try" => Schluessel::Try,
        "typeof" => Schluessel::Typeof,
        "var" => Schluessel::Var,
        "void" => Schluessel::Void,
        "while" => Schluessel::While,
        "with" => Schluessel::With,
        "yield" => Schluessel::Yield,
        "static" => Schluessel::Static,
        "get" => Schluessel::Get,
        "set" => Schluessel::Set,
        "of" => Schluessel::Of,
        "target" => Schluessel::Target,
        "async" => Schluessel::Async,
        "await" => Schluessel::Await,
        "from" => Schluessel::From,
        "as" => Schluessel::As,
        "constructor" => Schluessel::Constructor,
        _ => return None,
    })
}

fn ist_bezeichner_start(c: char) -> bool {
    c == '_' || c == '$' || c == '\u{2118}' || c.is_alphabetic()
}

fn ist_bezeichner_teil(c: char) -> bool {
    c == '_'
        || c == '$'
        || c == '\u{2118}'
        || c.is_alphanumeric()
        || c == '\u{200C}'
        || c == '\u{200D}'
}

fn ist_dezimal_ziffer(c: char) -> bool {
    c.is_ascii_digit()
}

fn hex_wert(c: char) -> Option<u32> {
    c.to_digit(16)
}

/// Lext den Quelltext in Token. Gibt bei Ueberlange oder Fehlform sofort
/// einen benannten Fehler zurueck (M4: pruefen BEVOR alloziert wird).
pub(crate) fn lexen(
    quelle: &str,
    max_bytes: usize,
    max_token: usize,
) -> Result<Vec<Token>, JsFehler> {
    lexen_mit(quelle, max_bytes, max_token, true)
}

/// Release 12.10.2026: wie [`lexen`] fuer ein MODUL — ohne die HTML-artigen
/// Kommentare (ECMA-262 Annex B.1.1 gilt nur fuer Skripte).
pub(crate) fn lexen_modul(
    quelle: &str,
    max_bytes: usize,
    max_token: usize,
) -> Result<Vec<Token>, JsFehler> {
    lexen_mit(quelle, max_bytes, max_token, false)
}

fn lexen_mit(
    quelle: &str,
    max_bytes: usize,
    max_token: usize,
    html_kommentare: bool,
) -> Result<Vec<Token>, JsFehler> {
    if quelle.len() > MAX_QUELLTEXT_HART || quelle.len() > max_bytes {
        return Err(JsFehler::Syntax {
            zeile: 1,
            spalte: 1,
            nachricht: format!(
                "Quelltext zu lang ({} Bytes, Grenze {})",
                quelle.len(),
                max_bytes.min(MAX_QUELLTEXT_HART)
            ),
        });
    }
    let zeichen: Vec<char> = quelle.chars().collect();
    // M4: Kapazitaet gedeckelt, nicht aus Eingabe abgeleitet.
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = 0usize;
    let n = zeichen.len();
    let mut zeile: u32 = 1;
    let mut spalte: u32 = 1;
    let mut umbruch_davor = false;
    // Template-Modus-Stapel: (geschweifte Tiefe beim `${`, im_template).
    let mut template_stapel: Vec<usize> = Vec::new();
    let mut klammer_tiefe: usize = 0;
    // Release 10.10.2026: je offene `(`, ob sie den Kopf von `if`/`while`/
    // `for`/`with` oeffnet — und fuer die zuletzt geschlossene `)`, ob sie
    // so einen Kopf schloss. Danach beginnt eine ANWEISUNG, ein `/` ist
    // also ein Regex (`if (o < 128) /[!#]/.test(s)`), sonst Division
    // (`(a) / 2`). Gedeckelt wie die anderen Stapel (M3).
    let mut runde_koepfe: Vec<bool> = Vec::new();
    let mut letzte_runde_kopf = false;

    // M3: jede Eingabe-abhaengige Schleife mit harter Obergrenze.
    // `n` ist endlich (Quelltext liegt vor), zusaetzlich wachen die
    // Token-Grenzen ueber pathologische Faelle.
    let grenze_token = max_token.min(MAX_TOKEN_HART);
    let mut schritte: usize = 0;
    // Harte Schrittobergrenze als Vielfaches der Eingabelaenge (M3):
    // jeder Schleifendurchlauf verbraucht mind. ein Zeichen oder erzeugt
    // ein Token; Faktor 8 deckt Lookahead ab.
    let max_schritte = n.saturating_mul(8).saturating_add(1024);

    macro_rules! fehler {
        ($msg:expr) => {
            JsFehler::Syntax {
                zeile,
                spalte,
                nachricht: ($msg).to_string(),
            }
        };
    }

    while i < n {
        schritte += 1;
        if schritte > max_schritte {
            return Err(fehler!(
                "Lexer-Schrittgrenze ueberschritten (Eingabe pathologisch?)"
            ));
        }
        if tokens.len() > grenze_token {
            return Err(fehler!(format!("zu viele Token (Grenze {grenze_token})")));
        }
        let c = zeichen[i];
        // --- Leerraum und Umbrueche ---
        if c == ' ' || c == '\t' || c == '\u{000B}' || c == '\u{000C}' || c == '\u{00A0}' {
            i += 1;
            spalte += 1;
            continue;
        }
        if c == '\n' || c == '\r' || c == '\u{2028}' || c == '\u{2029}' {
            if c == '\r' && i + 1 < n && zeichen[i + 1] == '\n' {
                i += 1;
            }
            i += 1;
            zeile = zeile.saturating_add(1);
            spalte = 1;
            umbruch_davor = true;
            continue;
        }
        // Release 12.10.2026: HTML-artige Kommentare (ECMA-262 Annex B.1.1):
        // `<!--` beginnt ueberall einen Zeilenkommentar, `-->` am Anfang einer
        // Zeile (davor nur Leerraum/Kommentare). GEMESSEN vorher (alte
        // Skripte `<!-- ... // -->`, WPT-Hilfsdateien): Syntaxfehler, das
        // ganze Skript lief nicht.
        let html_auf = html_kommentare
            && c == '<'
            && i + 3 < n
            && zeichen[i + 1] == '!'
            && zeichen[i + 2] == '-'
            && zeichen[i + 3] == '-';
        let html_zu = html_kommentare
            && c == '-'
            && (umbruch_davor || tokens.is_empty())
            && i + 2 < n
            && zeichen[i + 1] == '-'
            && zeichen[i + 2] == '>';
        if html_auf || html_zu {
            while i < n && !matches!(zeichen[i], '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                i += 1;
                spalte += 1;
                schritte += 1;
                if schritte > max_schritte {
                    return Err(fehler!("Kommentar zu lang"));
                }
            }
            continue;
        }
        // --- Kommentare ---
        if c == '/' && i + 1 < n && (zeichen[i + 1] == '/' || zeichen[i + 1] == '*') {
            if zeichen[i + 1] == '/' {
                i += 2;
                spalte += 2;
                while i < n && zeichen[i] != '\n' && zeichen[i] != '\r' {
                    i += 1;
                    spalte += 1;
                    schritte += 1;
                    if schritte > max_schritte {
                        return Err(fehler!("Kommentar zu lang"));
                    }
                }
                continue;
            } else {
                let z0 = zeile;
                let s0 = spalte;
                i += 2;
                spalte += 2;
                let mut geschlossen = false;
                while i < n {
                    schritte += 1;
                    if schritte > max_schritte {
                        return Err(fehler!("Kommentar zu lang"));
                    }
                    if zeichen[i] == '*' && i + 1 < n && zeichen[i + 1] == '/' {
                        i += 2;
                        spalte += 2;
                        geschlossen = true;
                        break;
                    }
                    if zeichen[i] == '\n' || zeichen[i] == '\r' {
                        if zeichen[i] == '\r' && i + 1 < n && zeichen[i + 1] == '\n' {
                            i += 1;
                        }
                        i += 1;
                        zeile = zeile.saturating_add(1);
                        spalte = 1;
                        umbruch_davor = true;
                    } else {
                        i += 1;
                        spalte += 1;
                    }
                }
                if !geschlossen {
                    return Err(JsFehler::Syntax {
                        zeile: z0,
                        spalte: s0,
                        nachricht: "nicht geschlossener Blockkommentar".to_string(),
                    });
                }
                continue;
            }
        }
        let tz = zeile;
        let ts = spalte;
        let ub = umbruch_davor;
        umbruch_davor = false;

        // --- Template-Ende/abschnitte: Backtick startet Template ---
        if c == '`' {
            i += 1;
            spalte += 1;
            let (stueck, ni, nz, ns, ende) =
                lies_template(&zeichen, i, zeile, spalte, n).map_err(|msg| JsFehler::Syntax {
                    zeile: tz,
                    spalte: ts,
                    nachricht: msg,
                })?;
            i = ni;
            zeile = nz;
            spalte = ns;
            if ende == TemplateEnde::Ausdruck {
                // J16: `${` ist eine **implizite Klammerebene** — der
                // Inhalt der Ersetzung wird eine Ebene tiefer gelesen und
                // das schliessende `}` bringt sie wieder hoch. GEMESSEN
                // vorher (echtes github-Buendel `app-runtime`, 93 KiB):
                // `` `${ (function(){ return `${a[0]}` })() }` `` brach mit
                // `erwartet: ')'` ab, weil beim schliessenden `}` der
                // Tiefenwert des **Funktionsrumpfs** (1) mit dem
                // Templatewert (1) zusammenfiel: das `}` wurde als
                // Template-Ende genommen und senkte die Tiefe — der
                // Rumpf galt danach als geschlossen.
                template_stapel.push(klammer_tiefe);
                klammer_tiefe = klammer_tiefe.saturating_add(1);
                if template_stapel.len() > MAX_TEMPLATE_TIEFE {
                    return Err(fehler!("Template-Verschachtelung zu tief"));
                }
            }
            tokens.push(Token {
                art: TokenArt::TemplateStueck(Box::new(stueck)),
                zeile: tz,
                spalte: ts,
                umbruch_davor: ub,
            });
            if ende == TemplateEnde::Ausdruck {
                tokens.push(Token {
                    art: TokenArt::Zeichen(Zeichen::DollarLGeschweift),
                    zeile,
                    spalte,
                    umbruch_davor: false,
                });
            }
            continue;
        }
        // --- Zeichenketten ---
        if c == '"' || c == '\'' {
            let (s, ni, _ns) =
                lies_zeichenkette(&zeichen, i, n, tz, ts, max_zeichenkette(max_bytes))?;
            let dl = ni - i;
            i = ni;
            spalte += dl as u32;
            tokens.push(Token {
                art: TokenArt::Zeichenkette(s),
                zeile: tz,
                spalte: ts,
                umbruch_davor: ub,
            });
            continue;
        }
        // --- Zahlen (inkl. .5) ---
        if ist_dezimal_ziffer(c) || (c == '.' && i + 1 < n && zeichen[i + 1].is_ascii_digit()) {
            let (z, ni) = lies_zahl(&zeichen, i, n).map_err(|msg| JsFehler::Syntax {
                zeile: tz,
                spalte: ts,
                nachricht: msg,
            })?;
            let dl = ni - i;
            i = ni;
            spalte += dl as u32;
            tokens.push(Token {
                art: match z {
                    ZahlWert::F64(w) => TokenArt::Zahl(w),
                    ZahlWert::Ganz(g) => TokenArt::Ganz(g),
                },
                zeile: tz,
                spalte: ts,
                umbruch_davor: ub,
            });
            continue;
        }
        // --- Bezeichner / Schluesselwort ---
        if ist_bezeichner_start(c) || (c == '\\' && i + 1 < n && zeichen[i + 1] == 'u') {
            let (name, ni, hat_escape) = lies_bezeichner(&zeichen, i, n);
            let dl = ni - i;
            if dl == 0 {
                return Err(fehler!("ungueltige Escape-Sequenz im Bezeichner"));
            }
            i = ni;
            spalte += dl as u32;
            if name.len() > MAX_TOKEN_LAENGE {
                return Err(fehler!("Bezeichner zu lang"));
            }
            let art = if hat_escape {
                TokenArt::BezeichnerEscape(name)
            } else {
                match schluesselwort(&name) {
                    Some(w) => TokenArt::Wort(w),
                    None => TokenArt::Bezeichner(name),
                }
            };
            tokens.push(Token {
                art,
                zeile: tz,
                spalte: ts,
                umbruch_davor: ub,
            });
            continue;
        }
        // --- Regex oder Division? Heuristik: nach Wert-Tokens ist `/`
        // Division, sonst Regex-Anfang. (Vereinfachung, dokumentiert.)
        if c == '/' {
            let regex_positon = match tokens.last() {
                None => true,
                Some(t) => match &t.art {
                    TokenArt::Bezeichner(_) | TokenArt::BezeichnerEscape(_)
                    | TokenArt::Zahl(_)
                    // J24: `10n / 3` ist Division — ohne `Ganz` hier las
                    // der Lexer den `/` als Regex-Anfang und die Zeile
                    // `String(2n ** 64n), String(10n / 3n)` brach mit
                    // „Syntaxfehler 1:43: erwartet: ')'" ab (gemessen).
                    | TokenArt::Ganz(_)
                    | TokenArt::Zeichenkette(_)
                    | TokenArt::Wort(Schluessel::True)
                    | TokenArt::Wort(Schluessel::False)
                    | TokenArt::Wort(Schluessel::Null)
                    | TokenArt::Wort(Schluessel::This)
                    | TokenArt::Wort(Schluessel::Super) => false,
                    // Release 10.10.2026: `)` eines Anweisungskopfs, siehe
                    // `runde_koepfe`. GEMESSEN (github.com, nsj-Modul, node
                    // 22 gueltig): `else if(o<128)/[!#$&-;=?-Z_a-z~]/.test(…)`
                    // -> „erwartet: Ausdruck" (der `/` war Division).
                    TokenArt::Zeichen(Zeichen::RRunde) => letzte_runde_kopf,
                    TokenArt::Zeichen(z) => !matches!(
                        z,
                        Zeichen::REckig | Zeichen::PlusPlus | Zeichen::MinusMinus
                    ),
                    _ => true,
                },
            };
            // J22: **die Entscheidung kommt aus dem Kontext, nicht aus dem
            // nächsten Zeichen.** GEMESSEN/GEFUNDEN an echten Skripten:
            // `highlight.pack.js` 6:19049 (`{begin:/=\s*/}`) und
            // `airgap.js` 5:14317 (`Sv=/=(&|$)/g`) scheiterten an der
            // Zusatzbedingung `zeichen[i+1] != '='` — sie las das `/=` als
            // **Operator** und meldete dann spaeter „unerwartetes Zeichen
            // '\'" (bei `\s` im Regex) bzw. „erwartet: Ausdruck".
            //
            // Warum die Bedingung falsch war: `/=` kann **kein** Divisions-
            // Zuweisung sein, wenn davor ein Token steht, das einen Ausdruck
            // erwartet — nach `=`, `(`, `,`, `:`, `return` … folgt laut Spec
            // ein **Regex**, und ein Regex darf mit `=` beginnen
            // (`/=\\s*/`, `/=(&|$)/g` — beides echte Fundstellen). Die
            // echte Division-Zuweisung `a /= b` hat davor ein **Wert**-
            // Token (`Bezeichner`, `)`, `]`, Zahl …), und dort ist
            // `regex_positon` schon `false` — der Operator wird ganz ohne
            // Zusatzbedingung gelesen (siehe `mini`-Proben n1…n9 im Bericht).
            if regex_positon {
                match lies_regex(&zeichen, i, n) {
                    Some((muster, schalter, ni)) => {
                        let dl = ni - i;
                        i = ni;
                        spalte += dl as u32;
                        tokens.push(Token {
                            art: TokenArt::Regex { muster, schalter },
                            zeile: tz,
                            spalte: ts,
                            umbruch_davor: ub,
                        });
                        continue;
                    }
                    None => {
                        // Kein gueltiges Regex -> Division o.ae. (Parser meldet).
                    }
                }
            }
        }
        // --- Operatoren / Interpunktion (laengste zuerst) ---
        let rest = &zeichen[i..];
        macro_rules! nimm {
            ($len:expr, $z:expr) => {{
                i += $len;
                spalte += $len as u32;
                tokens.push(Token {
                    art: TokenArt::Zeichen($z),
                    zeile: tz,
                    spalte: ts,
                    umbruch_davor: ub,
                });
                continue;
            }};
        }
        let s0 = rest.get(0).copied().unwrap_or('\0');
        let s1 = rest.get(1).copied().unwrap_or('\0');
        let s2 = rest.get(2).copied().unwrap_or('\0');
        let s3 = rest.get(3).copied().unwrap_or('\0');
        match (s0, s1, s2, s3) {
            ('.', '.', '.', _) => nimm!(3, Zeichen::Punkte3),
            ('>', '>', '>', '=') => nimm!(4, Zeichen::GroesserGroesserGroesserGleich),
            ('>', '>', '>', _) => nimm!(3, Zeichen::GroesserGroesserGroesser),
            ('*', '*', '=', _) => nimm!(3, Zeichen::SternSternGleich),
            ('<', '<', '=', _) => nimm!(3, Zeichen::KleinerKleinerGleich),
            ('>', '>', '=', _) => nimm!(3, Zeichen::GroesserGroesserGleich),
            ('=', '=', '=', _) => nimm!(3, Zeichen::GleichGleichGleich),
            ('!', '=', '=', _) => nimm!(3, Zeichen::RufGleichGleich),
            ('?', '?', '=', _) => nimm!(3, Zeichen::FrageDoppelGleich),
            ('&', '&', '=', _) => nimm!(3, Zeichen::UndUndGleich),
            ('|', '|', '=', _) => nimm!(3, Zeichen::OderOderGleich),
            ('*', '*', _, _) => nimm!(2, Zeichen::SternStern),
            ('?', '?', _, _) => nimm!(2, Zeichen::DoppelFrage),
            ('?', '.', _, _) if !s2.is_ascii_digit() => nimm!(2, Zeichen::FragePunkt),
            ('=', '>', _, _) => nimm!(2, Zeichen::Pfeil),
            ('=', '=', _, _) => nimm!(2, Zeichen::GleichGleich),
            ('!', '=', _, _) => nimm!(2, Zeichen::RufGleich),
            ('<', '=', _, _) => nimm!(2, Zeichen::KleinerGleich),
            ('>', '=', _, _) => nimm!(2, Zeichen::GroesserGleich),
            ('<', '<', _, _) => nimm!(2, Zeichen::KleinerKleiner),
            ('>', '>', _, _) => nimm!(2, Zeichen::GroesserGroesser),
            ('+', '+', _, _) => nimm!(2, Zeichen::PlusPlus),
            ('-', '-', _, _) => nimm!(2, Zeichen::MinusMinus),
            ('+', '=', _, _) => nimm!(2, Zeichen::PlusGleich),
            ('-', '=', _, _) => nimm!(2, Zeichen::MinusGleich),
            ('*', '=', _, _) => nimm!(2, Zeichen::SternGleich),
            ('/', '=', _, _) => nimm!(2, Zeichen::SchragGleich),
            ('%', '=', _, _) => nimm!(2, Zeichen::ProzentGleich),
            ('&', '=', _, _) => nimm!(2, Zeichen::UndGleich),
            ('|', '=', _, _) => nimm!(2, Zeichen::OderGleich),
            ('^', '=', _, _) => nimm!(2, Zeichen::HochGleich),
            ('&', '&', _, _) => nimm!(2, Zeichen::UndUnd),
            ('|', '|', _, _) => nimm!(2, Zeichen::OderOder),
            ('~', '=', _, _) => nimm!(2, Zeichen::TildeGleich),
            ('*', '/', _, _) => {
                return Err(fehler!("einsames '*/' ohne Kommentar-Anfang"));
            }
            _ => {}
        }
        let z = match c {
            '(' => {
                let kopf = match tokens.last().map(|t| &t.art) {
                    Some(TokenArt::Wort(
                        Schluessel::If | Schluessel::While | Schluessel::For | Schluessel::With,
                    )) => true,
                    // `for await (…)`
                    Some(TokenArt::Wort(Schluessel::Await)) => tokens
                        .len()
                        .checked_sub(2)
                        .is_some_and(|k| matches!(tokens[k].art, TokenArt::Wort(Schluessel::For))),
                    _ => false,
                };
                if runde_koepfe.len() < MAX_TOKEN_LAENGE {
                    runde_koepfe.push(kopf);
                }
                Zeichen::LRunde
            }
            ')' => {
                letzte_runde_kopf = runde_koepfe.pop().unwrap_or(false);
                Zeichen::RRunde
            }
            '{' => {
                klammer_tiefe += 1;
                Zeichen::LGeschweift
            }
            '}' => {
                // Schliessende Klammer eines `${}` -> Template fortsetzen.
                // Das `}` schliesst die `${`-Oeffnung: es kommt genau dann,
                // wenn die Klammertiefe wieder **auf** dem bei `${`
                // gespeicherten Wert steht (die Oeffnung selbst zaehlt
                // nicht mit).
                //
                // GEMESSEN (J5): vorher galt `last == klammer_tiefe - 1`,
                // wodurch bereits die schliessende Klammer eines
                // **Objektliterals** im Ausdruck als Template-Ende galt:
                // `` `${ {a:1} }` `` brach mit „erwartet: '}' in Template"
                // ab (`tests/t262/faelle/obj-tostring.js`).
                // J16: seit `${` die Tiefe erhoeht, liegt der Inhalt eine
                // Ebene tiefer als der gespeicherte Wert.
                if template_stapel.last().map(|t| t.saturating_add(1)) == Some(klammer_tiefe) {
                    template_stapel.pop();
                    klammer_tiefe = klammer_tiefe.saturating_sub(1);
                    // Template-Mitte/Ende lesen.
                    i += 1;
                    spalte += 1;
                    let (stueck, ni, nz, ns, ende) = lies_template(&zeichen, i, zeile, spalte, n)
                        .map_err(|msg| JsFehler::Syntax {
                        zeile: tz,
                        spalte: ts,
                        nachricht: msg,
                    })?;
                    i = ni;
                    zeile = nz;
                    spalte = ns;
                    if ende == TemplateEnde::Ausdruck {
                        template_stapel.push(klammer_tiefe);
                        klammer_tiefe = klammer_tiefe.saturating_add(1);
                    }
                    tokens.push(Token {
                        art: TokenArt::Zeichen(Zeichen::RGeschweift),
                        zeile: tz,
                        spalte: ts,
                        umbruch_davor: ub,
                    });
                    tokens.push(Token {
                        art: TokenArt::TemplateStueck(Box::new(stueck)),
                        zeile,
                        spalte,
                        umbruch_davor: false,
                    });
                    if ende == TemplateEnde::Ausdruck {
                        tokens.push(Token {
                            art: TokenArt::Zeichen(Zeichen::DollarLGeschweift),
                            zeile,
                            spalte,
                            umbruch_davor: false,
                        });
                    }
                    continue;
                }
                klammer_tiefe = klammer_tiefe.saturating_sub(1);
                Zeichen::RGeschweift
            }
            '[' => Zeichen::LEckig,
            ']' => Zeichen::REckig,
            ';' => Zeichen::Semikolon,
            ',' => Zeichen::Komma,
            '.' => Zeichen::Punkt,
            '?' => Zeichen::Frage,
            ':' => {
                if s1 == '?' {
                    nimm!(2, Zeichen::DoppelPunktFrage);
                }
                Zeichen::DoppelPunkt
            }
            '+' => Zeichen::Plus,
            '-' => Zeichen::Minus,
            '*' => Zeichen::Stern,
            '/' => Zeichen::Schrag,
            '%' => Zeichen::Prozent,
            '=' => Zeichen::Gleich,
            '!' => Zeichen::Ruf,
            '<' => Zeichen::Kleiner,
            '>' => Zeichen::Groesser,
            '&' => Zeichen::Und,
            '|' => Zeichen::Oder,
            '~' => Zeichen::Tilde,
            '^' => Zeichen::Hoch,
            // J11: `#` leitet einen privaten Namen ein. GEMESSEN, warum
            // das kein Fehler mehr ist: `class C { #x = 1; m(){ return
            // this.#x } }` stand in echten Seitenskripten (J6) und brach
            // schon im Lexer ab — der Motor konnte die Form gar nicht
            // erst sehen.
            '#' => Zeichen::Raute,
            _ => {
                return Err(fehler!(format!("unerwartetes Zeichen {c:?}")));
            }
        };
        // `*=` etc. mit Frage davor (`*?=` gibt es nicht in JS; nur
        // logische Zuweisung `&&=`, `||=`, `??=` und arithmetische).
        // `**=`, `&&=` usw. oben schon behandelt; hier Einfaches.
        i += 1;
        spalte += 1;
        // Logische/arithmetische `?=`-Formen pruefen (`*?=` ist ungueltig,
        // `&&=` wurde oben behandelt).
        if (z == Zeichen::Stern || z == Zeichen::Schrag || z == Zeichen::Prozent)
            && i < n
            && zeichen[i] == '?'
        {
            let zz = match z {
                Zeichen::Stern => Zeichen::SterneFrageGleich,
                Zeichen::Schrag => Zeichen::SchragFrageGleich,
                _ => Zeichen::ProzentFrageGleich,
            };
            if i + 1 < n && zeichen[i + 1] == '=' {
                i += 2;
                spalte += 2;
                tokens.push(Token {
                    art: TokenArt::Zeichen(zz),
                    zeile: tz,
                    spalte: ts,
                    umbruch_davor: ub,
                });
                continue;
            }
        }
        tokens.push(Token {
            art: TokenArt::Zeichen(z),
            zeile: tz,
            spalte: ts,
            umbruch_davor: ub,
        });
    }
    tokens.push(Token {
        art: TokenArt::Ende,
        zeile,
        spalte,
        umbruch_davor: false,
    });
    Ok(tokens)
}

/// Release 15.10.2026: one template piece as ECMA-262 13.2.8.6 defines it:
/// the cooked value (TV), `None` for an invalid escape sequence (allowed only
/// in tagged templates), and the raw value (TRV: the source text with CR LF
/// and CR as LF). MEASURED before: only the cooked text existed, so
/// `String.raw` and every tag function saw no `raw` (lit-html checks it), and
/// String.raw`\unicode` was a syntax error.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TemplateText {
    pub(crate) gekocht: Option<String>,
    pub(crate) roh: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemplateEnde {
    Ausdruck,
    Fertig,
}

/// Liest ab `i` (nach dem oeffnenden Backtick bzw. `}`) ein Template-Stueck
/// bis `${` oder schliessendem Backtick. Gibt (Text, neues i, Zeile, Spalte,
/// Ende) zurueck. Escapes werden gekocht.
fn lies_template(
    zeichen: &[char],
    mut i: usize,
    mut zeile: u32,
    mut spalte: u32,
    n: usize,
) -> Result<(TemplateText, usize, u32, u32, TemplateEnde), String> {
    let mut out = String::new();
    // Release 15.10.2026: the raw text and whether every escape was valid.
    let mut roh = String::new();
    let mut gueltig = true;
    let mut schritte = 0usize;
    let max = n.saturating_mul(2).saturating_add(64);
    let fertig = |out: String, roh: String, gueltig: bool| TemplateText {
        gekocht: if gueltig { Some(out) } else { None },
        roh,
    };
    while i < n {
        schritte += 1;
        if schritte > max {
            return Err("Template zu lang".to_string());
        }
        if out.len() > MAX_TOKEN_LAENGE || roh.len() > MAX_TOKEN_LAENGE {
            return Err("Template-Abschnitt zu lang".to_string());
        }
        let c = zeichen[i];
        if c == '`' {
            i += 1;
            spalte += 1;
            return Ok((fertig(out, roh, gueltig), i, zeile, spalte, TemplateEnde::Fertig));
        }
        if c == '$' && i + 1 < n && zeichen[i + 1] == '{' {
            i += 2;
            spalte += 2;
            return Ok((fertig(out, roh, gueltig), i, zeile, spalte, TemplateEnde::Ausdruck));
        }
        if c == '\\' {
            match koche_escape(zeichen, i, zeile, spalte, n) {
                Ok((e, ni, nz, ns)) => {
                    out.push_str(&e);
                    // TRV: the escape as written, line terminators as LF.
                    let mut k = i;
                    while k < ni {
                        match zeichen[k] {
                            '\r' if k + 1 < ni && zeichen[k + 1] == '\n' => {}
                            '\r' => roh.push('\n'),
                            ch => roh.push(ch),
                        }
                        k += 1;
                    }
                    i = ni;
                    zeile = nz;
                    spalte = ns;
                }
                Err(_) => {
                    // NotEscapeSequence: the cooked value is undefined; the
                    // characters after the backslash are read as text.
                    gueltig = false;
                    roh.push('\\');
                    i += 1;
                    spalte += 1;
                }
            }
            continue;
        }
        if c == '\n' || c == '\r' {
            if c == '\r' && i + 1 < n && zeichen[i + 1] == '\n' {
                i += 1;
            }
            i += 1;
            out.push('\n');
            roh.push('\n');
            zeile = zeile.saturating_add(1);
            spalte = 1;
            continue;
        }
        out.push(c);
        roh.push(c);
        i += 1;
        spalte += 1;
    }
    Err("nicht geschlossenes Template-Literal".to_string())
}

/// J-AR: **die Grenze fuer ein Zeichenketten-Literal** — an die
/// Skriptgrenze gekoppelt, nicht an `MAX_TOKEN_LAENGE`.
///
/// GEMESSEN/ANLASS (ar.wikipedia.org, `load.php` vom 01.10., 72 KiB):
/// die Bruecke baut `document.documentElement.innerHTML = "<rumpf>"`. Der
/// Rumpf dieser Seite ist **1.145.919** Zeichen; mit dem Escaping von
/// `narsil/src/jsvm.rs:1049` (Tabs -> `\t`, Steuerzeichen -> `\u%04x`)
/// wird daraus ein Literal von **1.223.998** Zeichen. `MAX_TOKEN_LAENGE` ist
/// **1.000.000** — also brach der Lexer ab mit
/// „Syntaxfehler 1:38: Zeichenkette zu lang", und weil das Literal an
/// Spalte 38 beginnt, nennt die Meldung genau die Spalte des
/// `innerHTML`-Aufrufs.
///
/// WAS FALSCH WAR: `MAX_TOKEN_LAENGE` ist eine **Token**-Grenze (ein
/// Bezeichner, ein Regex). Ein Text-Literal ist kein „Token" im Sinne
/// dieser Grenze — es ist **Nutzdaten**, und seine Grenze ist bereits
/// an anderer Stelle benannt: `Grenzen::max_string_laenge` (8 MiB) fuer
/// Zeichenketten im Heap und `max_bytes` fuer den Quelltext. Der Auftrag
/// sagt es woertlich: „Grenze fuer Zeichenketten-Literale an die
/// Skriptgrenze koppeln".
fn max_zeichenkette(max_bytes: usize) -> usize {
    // Das Literal darf nicht groesser sein als der **Quelltext**, in dem
    // es steht — sonst koennte es nicht existieren. Plus 1/8 Reserve fuer
    // das Escaping (Tab -> `\t` verdoppelt, Steuerzeichen -> `\u00xx`
    // versechsfacht).
    max_bytes
        .saturating_mul(9)
        .saturating_div(8)
        .max(MAX_TOKEN_LAENGE)
}

fn lies_zeichenkette(
    zeichen: &[char],
    i0: usize,
    n: usize,
    zeile: u32,
    spalte: u32,
    max_zeichenkette: usize,
) -> Result<(String, usize, u32), JsFehler> {
    let quote = zeichen[i0];
    let mut out = String::new();
    let mut i = i0 + 1;
    let mut sp = spalte + 1;
    let mut schritte = 0usize;
    // J-AR: **DIE** Grenze, die ein 2-MB-Literal zurueckwies — nicht die
    // Token-Grenze, sondern diese **Schrittgrenze**: `n × 2 + 64` zaehlte
    // die Zeichen des Literal-Lesens, erlaubte also nur `2 × n`. Ein
    // Literal ist aber nie laenger als der **gesamte** Quelltext `n`, also
    // muss die Grenze mindestens `n + 64` sein (ein Zeichen je Schritt, plus
    // Reserve fuer die abschliessenden Zeichen).
    //
    // GEMESSEN (vorher/nachher): `console.log("xxx…")` mit 2.000.000 `x`
    // brach ab mit „Syntaxfehler 1:13: Zeichenkette zu lang" — obwohl
    // `max_zeichenkette` (9/8 × `max_quelltext_bytes` = 5.898.240) das
    // Literal zulaesst. Der Auftrag nennt diese Zeile 1:38 bei
    // ar.wikipedia.org; hier 1:13, weil das Literal frueher im Skript
    // beginnt.
    let max = n.saturating_add(max_zeichenkette.min(n).saturating_add(64));
    while i < n {
        schritte += 1;
        if schritte > max {
            return Err(JsFehler::Syntax {
                zeile,
                spalte,
                nachricht: format!("Zeichenkette zu lang (mehr als {max} Zeichen gelesen)"),
            });
        }
        if out.len() > max_zeichenkette {
            // GEMESSEN: der Text nennt die **beiden** Zahlen — vorher stand
            // nur „Zeichenkette zu lang", und man wusste nicht, welche
            // Grenze (Token? Skript? Heap?) gemeint war.
            return Err(JsFehler::Syntax {
                zeile,
                spalte,
                nachricht: format!(
                    "Zeichenkette zu lang ({} Zeichen, Grenze {max_zeichenkette})",
                    out.len()
                ),
            });
        }
        let c = zeichen[i];
        if c == quote {
            return Ok((out, i + 1, sp + 1));
        }
        if c == '\n' || c == '\r' {
            return Err(JsFehler::Syntax {
                zeile,
                spalte: sp,
                nachricht: "Zeilenumbruch in Zeichenkette".to_string(),
            });
        }
        if c == '\\' {
            let (e, ni, _, ns) =
                koche_escape(zeichen, i, zeile, sp, n).map_err(|msg| JsFehler::Syntax {
                    zeile,
                    spalte: sp,
                    nachricht: msg,
                })?;
            out.push_str(&e);
            // Spalte naeherungsweise weiterzaehlen.
            sp = ns
                .saturating_add((ni - i) as u32)
                .saturating_sub(ns.saturating_sub(sp));
            sp = sp.saturating_add((ni - i) as u32);
            i = ni;
            continue;
        }
        out.push(c);
        i += 1;
        sp += 1;
    }
    Err(JsFehler::Syntax {
        zeile,
        spalte,
        nachricht: "nicht geschlossene Zeichenkette".to_string(),
    })
}

/// Kocht EIN Escape ab Position `i` (zeigt auf `\`). Gibt (Text, neues i,
/// Zeile, Spalte) zurueck. `\u{...}` wird unterstuetzt.
fn koche_escape(
    zeichen: &[char],
    i: usize,
    zeile: u32,
    spalte: u32,
    n: usize,
) -> Result<(String, usize, u32, u32), String> {
    if i + 1 >= n {
        return Err("Escape am Eingabeende".to_string());
    }
    let e = zeichen[i + 1];
    let mut ni = i + 2;
    let mut ns = spalte + 2;
    let nz = zeile;
    let s = match e {
        'n' => "\n".to_string(),
        'r' => "\r".to_string(),
        't' => "\t".to_string(),
        'b' => "\u{0008}".to_string(),
        'f' => "\u{000C}".to_string(),
        'v' => "\u{000B}".to_string(),
        '0' if i + 2 >= n || !zeichen[i + 2].is_ascii_digit() => "\0".to_string(),
        '\'' => "'".to_string(),
        '"' => "\"".to_string(),
        '\\' => "\\".to_string(),
        '\n' => {
            // Zeilenfortsetzung: faellt weg, Zeile weiter.
            return Ok((String::new(), ni, nz.saturating_add(1), 1));
        }
        '\r' => {
            if i + 2 < n && zeichen[i + 2] == '\n' {
                ni += 1;
            }
            return Ok((String::new(), ni, nz.saturating_add(1), 1));
        }
        'x' => {
            if i + 3 >= n {
                return Err("unvollstaendiges \\x-Escape".to_string());
            }
            let h = hex2(zeichen[i + 2], zeichen[i + 3])
                .ok_or_else(|| "ungueltiges \\x-Escape".to_string())?;
            ns += 2;
            ni += 2;
            char::from_u32(h)
                .map(|c| c.to_string())
                .unwrap_or("\u{FFFD}".to_string())
        }
        'u' => {
            if i + 2 < n && zeichen[i + 2] == '{' {
                let mut j = i + 3;
                let mut wert: u32 = 0;
                let mut stellen = 0;
                loop {
                    if j >= n {
                        return Err("unvollstaendiges \\u-Escape".to_string());
                    }
                    if zeichen[j] == '}' {
                        break;
                    }
                    let d =
                        hex_wert(zeichen[j]).ok_or_else(|| "ungueltiges \\u-Escape".to_string())?;
                    wert = wert * 16 + d;
                    stellen += 1;
                    if stellen > 6 || wert > 0x10FFFF {
                        return Err("ungueltiges \\u-Escape".to_string());
                    }
                    j += 1;
                }
                if stellen == 0 {
                    return Err("unvollstaendiges \\u-Escape".to_string());
                }
                // Release 10.10.2026: das Escape reicht von `i` (Backslash)
                // bis einschliesslich `}` (bei `j`), also `j - i + 1`
                // Zeichen. Vorher `ns += j - i` auf `spalte + 2` — eine
                // Spalte zu weit. Jedes Token dahinter in derselben Zeile
                // lag falsch, und `ist_legacy_oktal` las das Zeichen
                // DANEBEN: GEMESSEN (github.com, react-core-Modul)
                // „Legacy-Oktal-Literal ist im strikten Code nicht erlaubt"
                // fuer `statusCode:200` (gelesen ab der zweiten `0`), weil
                // weiter vorn `\u{2014}` in einem Template stand. node:
                // gueltig.
                ns = spalte + (j + 1 - i) as u32;
                ni = j + 1;
                char::from_u32(wert)
                    .map(|c| c.to_string())
                    .unwrap_or("\u{FFFD}".to_string())
            } else {
                if i + 5 >= n {
                    return Err("unvollstaendiges \\u-Escape".to_string());
                }
                let mut wert: u32 = 0;
                for k in 0..4 {
                    let d = hex_wert(zeichen[i + 2 + k])
                        .ok_or_else(|| "ungueltiges \\u-Escape".to_string())?;
                    wert = wert * 16 + d;
                }
                // W23 Teil 1 (JS2/QUIC, Runde 72, ANGEKUENDIGT in WUNSCH_JS.txt):
                // ein **Lead-Surrogat** gefolgt von `\u` + **Trail-Surrogat**
                // ist **ein** Zeichen (U+10000..U+10FFFF), und `char::from_u32`
                // kann es nicht liefern — es gab fuer jedes einzelne Surrogat
                // `U+FFFD` zurueck und machte aus `'\uD800\uDC00'` **zwei**
                // Ersatzzeichen.
                //
                // **GEMESSEN, `node 22` (siehe WUNSCH_JS.txt, W23 Teil 1):**
                //   `'\uD800\uDC00'.charCodeAt(0)`  node 55296  bei uns 65533
                //   `'\uD800\uDC00'.charCodeAt(1)`  node 56320  bei uns 65533
                //   `String.fromCharCode(0xD800,0xDC00).charCodeAt(0)`
                //                              node 55296  bei uns 55296  <- beweis
                //
                // **DER VORHANDENE HELFER, und er ist derselbe, den
                // `n_text_aus_zeichen` benutzt** (`builtin2.rs:1204`):
                // `regexmini::als_text(&einheiten)` =
                // `char::decode_utf16(…).map(|r| r.unwrap_or('\u{FFFD}'))`.
                // Ein **Paar** dekodiert darin zu **einem** `char`; ein
                // **einzelnes** Surrogat nicht. *Deshalb ist die Grenze
                // genau hier und nicht im Heap — und deshalb ist es kein
                // Heap-Problem.*
                //
                // **W23 RESTE BLEIBEN:** zwei Leads (`'\uD800\uDBFF'`) und
                // ein einzelnes Surrogat (`'\uD800'`) sind mit einer
                // `String` nicht darstellbar. Diese Zeile behauptet das
                // nicht weg — sie repariert nur das, was darstellbar ist.
                // `i` zeigt auf den BACKSLASH (die Hexziffern liegen auf `i+2..i+5`),
                // das zweite Escape beginnt also auf `i+6`, seine Hexziffern
                // auf `i+8..i+11`.
                if (0xD800..=0xDBFF).contains(&wert)
                    && i + 11 < n
                    && zeichen[i + 6] == '\\'
                    && zeichen[i + 7] == 'u'
                {
                    let mut trail: u32 = 0;
                    let mut ok = true;
                    for k in 0..4 {
                        match hex_wert(zeichen[i + 8 + k]) {
                            Some(d) => trail = trail * 16 + d,
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && (0xDC00..=0xDFFF).contains(&trail) {
                        // Zehn Zeichen zusaetzlich: `\u` + vier Hexziffern.
                        return Ok((
                            crate::regexmini::als_text(&[wert as u16, trail as u16]),
                            ni + 10,
                            nz,
                            ns + 10,
                        ));
                    }
                }
                ns += 4;
                ni += 4;
                char::from_u32(wert)
                    .map(|c| c.to_string())
                    .unwrap_or("\u{FFFD}".to_string())
            }
        }
        d if d.is_ascii_digit() => {
            // Oktal-Escapes (Altlast): max. 3 Stellen, als Byte.
            let mut j = i + 1;
            let mut wert: u32 = 0;
            for _ in 0..3 {
                if j < n && ('0'..='7').contains(&zeichen[j]) {
                    wert = wert * 8 + (zeichen[j] as u32 - '0' as u32);
                    j += 1;
                } else {
                    break;
                }
            }
            ns += (j - (i + 1)) as u32;
            ni = j;
            char::from_u32(wert)
                .map(|c| c.to_string())
                .unwrap_or("\u{FFFD}".to_string())
        }
        other => other.to_string(),
    };
    Ok((s, ni, nz, ns))
}

fn hex2(a: char, b: char) -> Option<u32> {
    Some(hex_wert(a)? * 16 + hex_wert(b)?)
}

fn lies_bezeichner(zeichen: &[char], i0: usize, n: usize) -> (String, usize, bool) {
    let mut out = String::new();
    let mut i = i0;
    let mut hat_escape = false;
    // M3: `n` endlich; zusaetzlich Laengendeckel.
    while i < n && out.len() <= MAX_TOKEN_LAENGE {
        let c = zeichen[i];
        if c == '\\' && i + 1 < n && zeichen[i + 1] == 'u' {
            let mut gelesene_zeichen = 0;
            let mut wert: u32 = 0;
            let mut ok = false;

            if i + 2 < n && zeichen[i + 2] == '{' {
                let mut j = i + 3;
                let mut valid_hex = true;
                let mut empty = true;
                while j < n && zeichen[j] != '}' {
                    empty = false;
                    match hex_wert(zeichen[j]) {
                        Some(d) => {
                            wert = wert * 16 + d;
                            if wert > 0x10FFFF {
                                valid_hex = false;
                            }
                        }
                        None => valid_hex = false,
                    }
                    j += 1;
                }
                if j < n && zeichen[j] == '}' && valid_hex && !empty {
                    ok = true;
                    gelesene_zeichen = j + 1 - i;
                }
            } else if i + 5 < n {
                ok = true;
                for k in 0..4 {
                    match hex_wert(zeichen[i + 2 + k]) {
                        Some(d) => wert = wert * 16 + d,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    gelesene_zeichen = 6;
                }
            }

            if ok {
                if let Some(ch) = char::from_u32(wert) {
                    if (out.is_empty() && (ist_bezeichner_start(ch) || ch == '\\'))
                        || (!out.is_empty() && ist_bezeichner_teil(ch))
                    {
                        hat_escape = true;
                        out.push(ch);
                        i += gelesene_zeichen;
                        continue;
                    }
                }
            }
            break;
        }
        if out.is_empty() {
            if ist_bezeichner_start(c) {
                out.push(c);
                i += 1;
            } else {
                break;
            }
        } else if ist_bezeichner_teil(c) {
            out.push(c);
            i += 1;
        } else {
            break;
        }
    }
    (out, i, hat_escape)
}

/// Das Ergebnis einer Zahl im Quelltext: `Number` oder `BigInt`.
///
/// J24: `1n` war bis dahin ein Syntaxfehler („BigInt (1n) wird nicht
/// unterstuetzt") — damit schloss `typ-bigint` nicht laufen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ZahlWert {
    F64(f64),
    Ganz(i128),
}

/// Liest eine Zahl ab `i`. Unterstuetzt dezimal (mit Exponent und
/// Zifferntrennstrich `_`), hex (`0x`), oktal (`0o`), binaer (`0b`) und
/// Legacy-Oktal (`077`) — jeweils auch als BigInt-Literal mit `n`
/// (ohne Punkt und ohne Exponent, wie es die Spec verlangt).
fn lies_zahl(zeichen: &[char], i0: usize, n: usize) -> Result<(ZahlWert, usize), String> {
    let mut i = i0;
    // Basis-Praefixe.
    if zeichen[i] == '0' && i + 1 < n {
        let p = zeichen[i + 1];
        if p == 'x' || p == 'X' || p == 'o' || p == 'O' || p == 'b' || p == 'B' {
            let basis = if p == 'x' || p == 'X' {
                16
            } else if p == 'o' || p == 'O' {
                8
            } else {
                2
            };
            i += 2;
            let mut wert: f64 = 0.0;
            let mut ganz: i128 = 0;
            let mut stellen = 0;
            while i < n {
                let c = zeichen[i];
                if c == '_' {
                    i += 1;
                    continue;
                }
                let d = match hex_wert(c) {
                    Some(d) if (d as i64) < basis => d,
                    _ => break,
                };
                wert = wert * basis as f64 + d as f64;
                // GEMESSEN/ENTSCHIEDEN: der BigInt-Wert laeuft in `i128`;
                // ein Ueberlauf ist ein RangeError (kein stilles Runden).
                ganz = ganz
                    .checked_mul(basis as i128)
                    .and_then(|w| w.checked_add(d as i128))
                    .ok_or_else(|| {
                        format!("BigInt-Literal passt nicht in i128 ({basis}aerisch)")
                    })?;
                stellen += 1;
                i += 1;
            }
            if stellen == 0 {
                return Err("Zahl ohne Ziffern nach Basis-Praefix".to_string());
            }
            if i < n && (zeichen[i] == 'n' || zeichen[i] == 'N') {
                return Ok((ZahlWert::Ganz(ganz), i + 1));
            }
            return Ok((ZahlWert::F64(wert), i));
        }
    }
    let start = i;
    let mut hat_punkt = false;
    let mut hat_exp = false;
    // Vorkomma + Punkt + Nachkomma.
    while i < n {
        let c = zeichen[i];
        if c.is_ascii_digit() || c == '_' {
            i += 1;
        } else if c == '.' && !hat_punkt && !hat_exp {
            hat_punkt = true;
            i += 1;
        } else {
            break;
        }
    }
    // Exponent.
    if i < n && (zeichen[i] == 'e' || zeichen[i] == 'E') {
        let mut j = i + 1;
        if j < n && (zeichen[j] == '+' || zeichen[j] == '-') {
            j += 1;
        }
        if j < n && zeichen[j].is_ascii_digit() {
            hat_exp = true;
            i = j;
            while i < n && (zeichen[i].is_ascii_digit() || zeichen[i] == '_') {
                i += 1;
            }
        }
    }
    let roh: String = zeichen[start..i].iter().collect();
    if i < n && (zeichen[i] == 'n' || zeichen[i] == 'N') {
        // Spec 12.8.3: ein BigInt-Literal hat **weder** Punkt **noch**
        // Exponent — `1.5n` und `1e3n` sind Syntaxfehler.
        if hat_punkt {
            return Err("BigInt-Literal mit Punkt ist ungueltig".to_string());
        }
        if hat_exp {
            return Err("BigInt-Literal mit Exponent ist ungueltig".to_string());
        }
        let sauber: String = roh.chars().filter(|&c| c != '_').collect();
        let mut ganz: i128 = 0;
        for c in sauber.chars() {
            let Some(d) = c.to_digit(10) else {
                return Err("BigInt-Literal mit unbrauchbarer Ziffer".to_string());
            };
            ganz = ganz
                .checked_mul(10)
                .and_then(|w| w.checked_add(d as i128))
                .ok_or_else(|| "BigInt-Literal passt nicht in i128".to_string())?;
        }
        return Ok((ZahlWert::Ganz(ganz), i + 1));
    }
    let sauber: String = roh.chars().filter(|&c| c != '_').collect();
    // Legacy-Oktal `077`: fuehrende Null + nur 0-7.
    if sauber.len() > 1 && sauber.starts_with('0') && !hat_punkt && !hat_exp {
        let rest = &sauber[1..];
        if !rest.is_empty() && rest.chars().all(|c| ('0'..='7').contains(&c)) {
            let mut w: f64 = 0.0;
            for c in rest.chars() {
                w = w * 8.0 + (c as u32 - '0' as u32) as f64;
            }
            return Ok((ZahlWert::F64(w), i));
        }
    }
    sauber
        .parse::<f64>()
        .map(|v| (ZahlWert::F64(v), i))
        .map_err(|_| "ungueltige Zahl".to_string())
}

/// Versucht ab `i` (zeigt auf `/`), ein Regex-Literal zu lesen. Gibt
/// `None` zurueck, wenn es keins ist (dann entscheidet der Parser ueber
/// Division). Zeichenklassen `[...]` werden beachtet, damit `/[/]/`
/// funktioniert.
fn lies_regex(zeichen: &[char], i0: usize, n: usize) -> Option<(String, String, usize)> {
    let mut i = i0 + 1;
    let mut muster = String::new();
    let mut in_klasse = false;
    let mut schritte = 0usize;
    let max = (n - i0).saturating_mul(2).saturating_add(16);
    while i < n {
        schritte += 1;
        if schritte > max || muster.len() > MAX_TOKEN_LAENGE {
            return None;
        }
        let c = zeichen[i];
        if c == '\\' {
            if i + 1 >= n {
                return None;
            }
            muster.push(c);
            muster.push(zeichen[i + 1]);
            i += 2;
            continue;
        }
        if c == '\n' || c == '\r' {
            return None;
        }
        if c == '[' {
            in_klasse = true;
        } else if c == ']' {
            in_klasse = false;
        } else if c == '/' && !in_klasse {
            i += 1;
            let mut schalter = String::new();
            while i < n && zeichen[i].is_ascii_alphabetic() {
                schalter.push(zeichen[i]);
                i += 1;
            }
            if schalter.len() > 16 {
                return None;
            }
            return Some((muster, schalter, i));
        }
        muster.push(c);
        i += 1;
    }
    None
}
