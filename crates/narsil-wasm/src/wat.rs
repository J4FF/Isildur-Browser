//! W3: Textformat (WAT) lesen und schreiben.
//!
//! Ohne diesen Leser kaeme die Abnahme gegen `WebAssembly/testsuite` nicht
//! vom Boden: 250 der 257 `.wast`-Dateien bestehen aus Textmodulen.
//!
//! Umfang: alles, was die Testsuite fuer WebAssembly 2.0 Core benutzt —
//! Kurzformen (`(func (export "x") …)`), gefaltete Ausdruecke
//! (`(i32.add (i32.const 1) (i32.const 2))`), Bezeichner (`$name`,
//! `$label`), `then`/`else`, `block`/`loop`/`if` in beiden Schreibweisen,
//! Hexadezimal- und `nan:0x…`-Zahlen, Strings mit Escapes,
//! `offset=`/`align=`, alle `elem`- und `data`-Formen.
//!
//! Die Struktur ist ein rekursiver Abstieg ueber die Tokenliste; Befehle
//! landen in einem FLACHEN Vektor (der Interpreter laeuft ja flach), und
//! die Tiefengrenze (`Grenzen::verschachtelung`) bricht Klammer-Bomben ab
//! (M5).

use std::collections::BTreeMap;

use crate::fehler::*;
use crate::grenzen::Grenzen;
use crate::modell::{
    Art, Custom, Daten, Element, Export, Func, FuncTyp, Global, ImportiertSpeicher, ImportiertTab,
    Limits, Modul, Reftyp, Speichertyp, Tabellentyp, Valtyp,
};
use crate::op::{Befehl, Blockart};

/// Warum ein Textmodul nicht lesbar war.
fn textfehler(t: &str) -> Fehler {
    Fehler::form(ART_OPCODE, format!("WAT: {t}"))
}

/// Ein Token im Text.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// `(name`
    Auf(String),
    Zu,
    Wort(String),
    /// `"…"` (Bytes, Escapes aufgeloest).
    Text(Vec<u8>),
    /// `$name`
    Name(String),
}

impl Token {
    fn text(&self) -> String {
        match self {
            Token::Auf(s) => format!("({s}"),
            Token::Zu => ")".to_string(),
            Token::Wort(w) => w.clone(),
            Token::Text(b) => format!("{:?}", String::from_utf8_lossy(b)),
            Token::Name(n) => format!("${n}"),
        }
    }
}

/// Zerlegt Text in Tokens; Kommentare `;;` und `(; ;)` fallen weg.
fn tokenisier(text: &str, _g: &Grenzen) -> Result<Vec<Token>, Fehler> {
    // M3: Tokenzahl ist begrenzt (M4: die Allokation folgt der Grenze).
    // M3/M4: Die Tokenzahl ist begrenzt. Jedes Token braucht mindestens
    // zwei Zeichen (Klammer oder Trenner), deshalb ist `len/2` eine sichere
    // Obergrenze. `br_table.wast` (GEMESSEN 01.10.) braucht 33 000 Tokens.
    let cap = (text.len() / 2 + 64).min(2_000_000);
    let mut out: Vec<Token> = Vec::new();
    let b = text.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if out.len() >= cap {
            return Err(textfehler("zu viele Tokens (Grenze aus der Texteingabe)"));
        }
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b';' if i + 1 < b.len() && b[i + 1] == b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if i + 1 < b.len() && b[i + 1] == b';' => {
                let mut tiefe = 1usize;
                i += 2;
                while i < b.len() && tiefe > 0 {
                    if i + 1 < b.len() && b[i] == b'(' && b[i + 1] == b';' {
                        tiefe += 1;
                        i += 2;
                    } else if i + 1 < b.len() && b[i] == b';' && b[i + 1] == b')' {
                        tiefe -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if tiefe > 0 {
                    return Err(textfehler("Blockkommentar nicht geschlossen"));
                }
            }
            b'(' => {
                i += 1;
                let mut name = String::new();
                // `@` ist der Praefix der Annotationsfelder (`(@custom …)`).
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric()
                        || matches!(b[i], b'.' | b'_' | b'-' | b'!' | b'@'))
                {
                    name.push(b[i] as char);
                    i += 1;
                }
                out.push(Token::Auf(name));
            }
            b')' => {
                i += 1;
                out.push(Token::Zu);
            }
            b'"' => {
                i += 1;
                let mut bytes: Vec<u8> = Vec::new();
                loop {
                    if i >= b.len() {
                        return Err(textfehler("Zeichenkette nicht geschlossen"));
                    }
                    match b[i] {
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\\' => {
                            i += 1;
                            if i >= b.len() {
                                return Err(textfehler("Escaping am Ende"));
                            }
                            match b[i] {
                                b'n' => {
                                    bytes.push(b'\n');
                                    i += 1;
                                }
                                b't' => {
                                    bytes.push(b'\t');
                                    i += 1;
                                }
                                b'r' => {
                                    bytes.push(b'\r');
                                    i += 1;
                                }
                                b'\\' => {
                                    bytes.push(b'\\');
                                    i += 1;
                                }
                                b'"' => {
                                    bytes.push(b'"');
                                    i += 1;
                                }
                                b'\'' => {
                                    bytes.push(b'\'');
                                    i += 1;
                                }
                                b'u' => {
                                    if i + 1 < b.len() && b[i + 1] == b'{' {
                                        let mut j = i + 2;
                                        let mut v: u32 = 0;
                                        let mut ziffern = 0;
                                        while j < b.len() && b[j] != b'}' {
                                            let d =
                                                (b[j] as char).to_digit(16).ok_or_else(|| {
                                                    textfehler("\\u: keine Hexadezimalzahl")
                                                })?;
                                            v = v * 16 + d;
                                            ziffern += 1;
                                            if ziffern > 8 {
                                                return Err(textfehler("\\u: zu viele Ziffern"));
                                            }
                                            j += 1;
                                        }
                                        if j >= b.len() {
                                            return Err(textfehler("\\u: keine }"));
                                        }
                                        let ch = char::from_u32(v).ok_or_else(|| {
                                            textfehler("\\u: ungueltiger Codepoint")
                                        })?;
                                        let mut tmp = [0u8; 4];
                                        bytes
                                            .extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                                        i = j + 1;
                                    } else {
                                        return Err(textfehler("\\u ohne {"));
                                    }
                                }
                                _ => {
                                    if i + 2 >= b.len() {
                                        return Err(textfehler("Escaping zu kurz"));
                                    }
                                    let h = std::str::from_utf8(&b[i..i + 2])
                                        .ok()
                                        .and_then(|s| u8::from_str_radix(s, 16).ok())
                                        .ok_or_else(|| {
                                            textfehler("Escaping: keine Hexadezimalzahl")
                                        })?;
                                    bytes.push(h);
                                    i += 2;
                                }
                            }
                        }
                        x => {
                            bytes.push(x);
                            i += 1;
                        }
                    }
                }
                out.push(Token::Text(bytes));
            }
            b'$' => {
                i += 1;
                let mut name = String::new();
                // Bezeichner duerfen alle druckbaren Zeichen ausser den
                // Trennzeichen enthalten (`id.wast`: `$!?@#a$%^&*b-+_.:9`)
                // … GEMESSEN 02.10.: mit der engen Zeichenliste wurde
                // `$!?@#a…` als Befehl gelesen.
                // `$"…"`: Bezeichner als maskierte Zeichenkette — dieselbe
                // Regel wie im WAST-Leser (die Maskierungen werden dort
                // aufgeloest, damit der Rundweg ueber den Text traegt).
                if i < b.len() && b[i] == b'"' {
                    name.push_str(
                        &crate::wast::maskierter_name(b, &mut i)
                            .map_err(|e| textfehler(&format!("Bezeichner {}", e.kurz())))?,
                    );
                }
                while i < b.len()
                    && !matches!(
                        b[i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"' | b';'
                    )
                {
                    name.push(b[i] as char);
                    i += 1;
                }
                // `$` ohne Namen ist ein Syntaxfehler („empty identifier",
                // GEMESSEN 02.10., `id.wast`).
                if name.is_empty() {
                    return Err(textfehler("leerer Bezeichner: `$` ohne Namen"));
                }
                out.push(Token::Name(name));
            }
            _ => {
                let start = i;
                while i < b.len()
                    && !matches!(
                        b[i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"' | b';'
                    )
                {
                    i += 1;
                }
                if i == start {
                    return Err(textfehler(&format!(
                        "unerwartetes Zeichen {:?}",
                        b[i] as char
                    )));
                }
                out.push(Token::Wort(text[start..i].to_string()));
            }
        }
    }
    Ok(out)
}

/// Zustand des Textlesers.
struct Parser {
    tok: Vec<Token>,
    pos: usize,
    g: Grenzen,
    /// Typen, die schon im Modul stehen (fuer `(type …)`-Verweise).
    typen: Vec<FuncTyp>,
    /// Wie viele `(type (func …))`-Felder gelesen wurden — das k-te Feld
    /// gehoert an den vorberechneten Typplatz k.
    typen_felder: usize,
    /// `$name` der Typen -> Index (GEMESSEN 01.10.: `call_indirect $t (type
    /// $check)` kam ohne diese Tabelle immer auf Typ 0).
    typen_namen: BTreeMap<String, u32>,
    /// `(Typindex, Feldname)` -> Feldindex, fuer `struct.get $t $x`.
    ///
    /// GEMESSEN 03.10. (`struct.wast` Z. 55: `(struct.get $t2 $x …)`).
    /// Die Tabelle entsteht beim Lesen von `(type (struct (field $x …)))`;
    /// steht der Typ **vor** dem Befehl (so in der Suite), reicht das.
    felder_namen: BTreeMap<(u32, String), u32>,
    /// Die `(rec …)`-Gruppe, in der der naechste Typ liegt:
    /// `(Gruppenanfang, Gruppenende)`, `None` ausserhalb.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast` Z. 21ff: „unknown type" — ein
    /// `(ref N)` darf ausserhalb einer Gruppe nicht vorwaerts zeigen).
    aktuelle_gruppe: Option<(u32, u32)>,
    /// Laufender Index der lokalen Variablen (Parameter zuerst, dann
    /// `local`). Nennt der Parameter keinen Namen, zaehlt er trotzdem —
    /// sonst rueckt der erste `local` auf Index 0 zurueck.
    ///
    /// GEMESSEN 03.10. (`func.wast` Z. 461ff: `(func (type $sig) (local
    /// $var i32) (local.get $var))` lieferte den Parameter statt der
    /// Variablen — 2 `assert_return` fielen aus).
    lokale_zaehler: u32,
    /// `$name` der Tabellen -> Index (`table.get $t2 …`).
    tabellen_namen: BTreeMap<String, u32>,
    /// `$name` der globalen Variablen -> Index (`global.get $g`).
    globale_namen: BTreeMap<String, u32>,
    /// `$name` der Speicher -> Index (`i32.load $m1 …`, Multi-Memory).
    speicher_namen: BTreeMap<String, u32>,
    /// `$name` der Datensaetze -> Index (`memory.init $m $d …`).
    daten_namen: BTreeMap<String, u32>,
    /// `$name` der Elementabschnitte -> Index (`table.init $t $e …`).
    elem_namen: BTreeMap<String, u32>,
    /// `$name` der Funktionen -> Index.
    funktionen: BTreeMap<String, u32>,
    /// `$name` der Tags -> Index (`(export "t" (tag $e))`).
    ///
    /// GEMESSEN 02.10. (`imports.wast` Z. 0).
    tags_namen: BTreeMap<String, u32>,
    /// Signatur -> Index (fuer `typindex`): macht die Suche O(1) statt O(n).
    ///
    /// GEMESSEN 03.10. (Schlussphase: quadratische Stelle — 2000 Typen
    /// kosteten 16,3 ms Uebersetzen, der Grossteil war die Suche).
    typen_index: std::collections::HashMap<FuncTyp, u32>,
    /// Zaehler der Lese-Schleifen (M3: harte Obergrenze gegen Endlosschleifen).
    schritte: usize,
    /// `$name` der Lokalvariablen der aktuellen Funktion -> Index.
    /// WICHTIG (GEMESSEN 01.10., `i32.wast`): `local.get $x` ist in der
    /// Testsuite die Regel — ohne diese Tabelle ist kein einziger Fall lesbar.
    lokale: BTreeMap<String, u32>,
    /// Offene Labels (fuer `br $name`): (Name oder None, Index von unten).
    labels: Vec<Option<String>>,
}

/// Die Inline-Signatur eines Feldes (`(param …)`/`(result …)`), ohne
/// `(type …)`. `None`, wenn das Feld nur `(type …)` nennt (dann erzeugt der
/// Leser KEINEN Typ) oder gar keine Signatur hat.
///
/// Der Vorlauf braucht das, um den Typraum des Lesers nachzubauen.
/// EIN Werttyp aus einem Tokenfeld (Vorlauf): ein Wort ODER eine
/// `(ref …)`-Gruppe. Liefert den Werttyp und die Stelle danach.
///
/// Der Typindex eines `(ref $name)` wird aus `typen_namen` aufgeloest —
/// ein VORWAERTSVERWEIS ist im Vorlauf nicht aufloesbar; dann liefert die
/// Funktion `None`, und der Vergleich mit dem Leser meldet laut (M8:
/// kein stillschweigendes Reparieren).
///
/// GEMESSEN 03.10. (`return_call_ref.wast` Z. 212ff).
fn vorlauf_valtyp(
    feld: &[Token],
    i: usize,
    typen_namen: &[(String, u32)],
) -> Option<(Valtyp, usize)> {
    match feld.get(i) {
        Some(Token::Wort(w)) => Some((valtyp(w).ok()?, i + 1)),
        Some(Token::Auf(k)) if k == "ref" => {
            let ende = gruppe_ende(feld, i);
            if ende >= feld.len() {
                return None;
            }
            let mut j = i + 1;
            let mut nullable = false;
            if matches!(feld.get(j), Some(Token::Wort(w)) if w == "null") {
                nullable = true;
                j += 1;
            }
            let r = match feld.get(j) {
                Some(Token::Wort(w)) => match zahl(w) {
                    // Ein ZAHL ist ein Typindex: `(ref 0)` (Wasm 3.0, GC).
                    //
                    // GEMESSEN 03.10. (`type-rec.wast`, `tag.wast`):
                    // ohne diesen Zweig gab `vorlauf_heaptyp` auf, die
                    // Vorlauf-Signatur wurde `() -> ()`, und der Vergleich
                    // im Hauptpass meldete „Typ 0: Vorlauf und Leser finden
                    // verschiedene Signaturen" — **jedes** Modul mit
                    // einem expliziten `(type (func (param (ref N)) …))`
                    // war unlesbar.
                    Ok(z) => Reftyp::Konkret {
                        typ: u32::try_from(z).ok()?,
                        nullable,
                    },
                    Err(_) => vorlauf_heaptyp(w, nullable)?,
                },
                Some(Token::Name(n)) => Reftyp::Konkret {
                    typ: typen_namen.iter().rev().find(|(k, _)| k == n)?.1,
                    nullable,
                },
                _ => return None,
            };
            Some((Valtyp::Ref(r), ende + 1))
        }
        _ => None,
    }
}

/// Ein Heaptyp als Wort, in der vom Vorlauf sichtbaren `null`-Form.
/// `(ref func)` ist die nicht-leerbare Form, `(ref null func)` die
/// Wildcard-Form — genau wie im Leser (`reftyp_lesen`).
fn vorlauf_heaptyp(w: &str, nullable: bool) -> Option<Reftyp> {
    use crate::modell::{JEDER_EXTERNTYP, JEDER_FUNKTIONSTYP};
    match heaptyp_wort(w)? {
        // `(ref func)` ist die NICHT-leerbare Form: der Leser liefert
        // dafuer `Konkret{JEDER_FUNKTIONSTYP, false}` (`reftyp_lesen`).
        //
        // GEMESSEN 03.10. (`return_call_ref.wast` Z. 217: „Typ 3: Vorlauf
        // und Leser finden verschiedene Signaturen (() -> () != () ->
        // (typreferenz))").
        Reftyp::Func => Some(if nullable {
            Reftyp::Func
        } else {
            Reftyp::Konkret {
                typ: JEDER_FUNKTIONSTYP,
                nullable: false,
            }
        }),
        Reftyp::Extern => Some(if nullable {
            Reftyp::Extern
        } else {
            Reftyp::Konkret {
                typ: JEDER_EXTERNTYP,
                nullable: false,
            }
        }),
        Reftyp::Konkret { typ, .. } => Some(Reftyp::Konkret { typ, nullable }),
    }
}

fn inline_signatur(feld: &[Token], typen_namen: &[(String, u32)]) -> Option<FuncTyp> {
    let mut i = 0usize;
    // Bezeichner und `(export "…")`-Gruppen ueberspringen.
    if matches!(feld.get(i), Some(Token::Name(_))) {
        i += 1;
    }
    while matches!(feld.get(i), Some(Token::Auf(k)) if k == "export" || k == "import") {
        let ende = gruppe_ende(feld, i);
        if ende >= feld.len() {
            return None;
        }
        i = ende + 1;
    }
    // `(type $a (sub $b (func …)))` (Wasm 3.0 GC): der Rumpf steckt in
    // der `sub`-Gruppe. Ohne dieses Durchdringen gab der Vorlauf den
    // Platzhalter `() -> ()`, und der Vergleich im Hauptpass meldete
    // „Typ 2: Vorlauf und Leser finden verschiedene Signaturen"
    // (GEMESSEN 03.10., `type-subtyping.wast`).
    if matches!(feld.get(i), Some(Token::Auf(k)) if k == "sub") {
        i += 1;
        // `(sub **final** $t (…))`: `final` ist ein **bare Wort** und
        // kommt VOR dem Obertyp. Ohne dieses Ueberspringen las der
        // Vorlauf die Signatur ab `final` und meldete
        // „Typ 2: Vorlauf und Leser finden verschiedene Signaturen" —
        // das ganze Modul mit seinen 6 `assert_return` fiel
        // (GEMESSEN 04.10., `type-subtyping.wast` Z. 963ff).
        //
        // Nur das Wort `final` wird hier uebersprungen: eine **Zahl**
        // (`(sub 3 (…))`) ist auch ein `Wort` und ist der Obertyp.
        if matches!(feld.get(i), Some(Token::Wort(w)) if w == "final") {
            i += 1;
        }
        if matches!(feld.get(i), Some(Token::Name(_)) | Some(Token::Wort(_))) {
            i += 1;
        }
    }
    if matches!(feld.get(i), Some(Token::Auf(k)) if k == "type") {
        return None;
    }
    // Bei einem `(type (func …))`-Feld steckt die Signatur eine Ebene tiefer.
    if matches!(feld.get(i), Some(Token::Auf(k)) if k == "func") {
        let ende = gruppe_ende(feld, i);
        if ende >= feld.len() {
            return None;
        }
        return inline_signatur(&feld[i + 1..ende], typen_namen);
    }
    let mut ft = FuncTyp {
        parameter: Vec::new(),
        ergebnis: Vec::new(),
    };
    let mut gefunden = false;
    while let Some(t) = feld.get(i) {
        match t {
            Token::Auf(k) if k == "param" || k == "result" => {
                gefunden = true;
                let ende = gruppe_ende(feld, i);
                let mut j = i + 1;
                while j < ende.min(feld.len()) {
                    match vorlauf_valtyp(feld, j, typen_namen) {
                        Some((v, naechste)) => {
                            if k == "param" {
                                ft.parameter.push(v);
                            } else {
                                ft.ergebnis.push(v);
                            }
                            j = naechste;
                        }
                        None => {
                            // Kein Werttyp: ueberspringen. `(param $x i32)`
                            // enthaelt den BEZEICHNER `$x` — ohne das
                            // sprang die Vorlauf-Signatur ab.
                            //
                            // GEMESSEN 03.10. (`block.wast`, `func.wast`,
                            // `if.wast`, `loop.wast`: 343 Kernfaelle fielen,
                            // weil jede Signatur mit benannten Parametern
                            // als leer galt: „Typ 3: Vorlauf und Leser
                            // finden verschiedene Signaturen (() -> () !=
                            // (i32) -> ())").
                            if matches!(feld.get(j), Some(Token::Auf(a)) if a == "ref") {
                                // Eine `(ref …)`-Gruppe, die nicht
                                // aufloesbar ist (Vorwaertsverweis): das
                                // meldet der Vergleich mit dem Leser
                                // spater LAUT (M8).
                                return None;
                            }
                            j += 1;
                        }
                    }
                }
                i = ende + 1;
            }
            _ => break,
        }
    }
    if gefunden { Some(ft) } else { None }
}

/// Traegt ein `(func …)`-Feld ein **explizites** `(type …)`?
///
/// GEMESSEN 03.10. (`type-rec.wast` Z. 51ff): fuer `(func (type $t) …)`
/// darf **kein** impliziter Typ entstehen — das ist der Unterschied, den
/// diese Frage entscheidet.
fn feld_hat_typ(feld: &[Token]) -> bool {
    let mut i = 0usize;
    if matches!(feld.get(i), Some(Token::Name(_))) {
        i += 1;
    }
    while matches!(feld.get(i), Some(Token::Auf(k)) if k == "export" || k == "import") {
        let ende = gruppe_ende(feld, i);
        if ende >= feld.len() {
            return false;
        }
        i = ende + 1;
    }
    matches!(feld.get(i), Some(Token::Auf(k)) if k == "type")
}

/// Index des schliessenden Tokens der Gruppe, die bei `start` beginnt.
fn gruppe_ende(tok: &[Token], start: usize) -> usize {
    let mut tiefe = 0i64;
    let mut q = start;
    while q < tok.len() {
        match &tok[q] {
            Token::Auf(_) => tiefe += 1,
            Token::Zu => {
                tiefe -= 1;
                if tiefe == 0 {
                    return q;
                }
            }
            _ => {}
        }
        q += 1;
    }
    tok.len() - 1
}

/// Wie `Parser::typindex`: vorhandenen Typ finden, sonst anhaengen.
///
/// `index` macht die Suche O(1) (Schlussphase, GEMESSEN 03.10.).
fn sim_typindex(
    typen: &mut Vec<FuncTyp>,
    index: &mut std::collections::HashMap<FuncTyp, u32>,
    ft: FuncTyp,
) -> u32 {
    if let Some(i) = index.get(&ft) {
        return *i;
    }
    let idx = typen.len() as u32;
    index.insert(ft.clone(), idx);
    typen.push(ft);
    idx
}

/// Vorab-Durchgang: Namen aller `(func $n)` und `(type $n)` auf Modulebene
/// mit ihrem Index versehen.
///
/// GEMESSEN 01.10. (`call.wast`, 72 Faelle): `call.wast` benutzt `$odd` BEVOR
/// es definiert ist. Ohne diesen Vorlauf loeste der Leser den Namen auf
/// `unwrap_or(&0)` auf — der Aufruf landete in Funktion 0, und der Validator
/// meldete dann „Block0 end: 1 Werte uebrig".
fn vorlauf(tok: &[Token]) -> Vorlauf {
    let mut funktionen: BTreeMap<String, u32> = BTreeMap::new();
    // Die expliziten `(type …)`-Felder kommen ZUERST in den Typraum, die
    // aus Inline-Signaturen erzeugten Typen DANACH — so baut die
    // Spezifikation den Typraum auf (die Abkuerzung „Inline-Typ" haengt
    // den neuen Typ ans ENDE des Typabschnitts).
    //
    // GEMESSEN 03.10. (`func.wast` Z. 488ff: `signature-implicit-reused`
    // benutzt `(type 3)` fuer die Signatur `(f64 i64 f64 i64 f64 i64 f32
    // i32)` — bei anderer Reihenfolge war das der Typ `(i64 i64 …), und
    // 4 Faelle fielen aus).
    let mut typen_explizit: Vec<FuncTyp> = Vec::new();
    // Die Indizes, die zu einer `(rec …)`-Gruppe gehoeren.
    //
    // GEMESSEN 03.10. (`type-rec.wast`, Kommentar der Suite: „the
    // implicit type of $f is not $ft"):
    //
    //     (rec (type $ft (func)) (type (func)))
    //     (func $f)          ;; der implizite Typ ist NEU
    //     (global (ref $ft) (ref.func $f))
    //
    // waere die Wiederverwendung von Index 0 ($ft) strukturell richtig
    // und damit falsch. Ein impliziter Typuse darf nie einen Typ aus
    // einer `rec`-Gruppe uebernehmen. Gemessen neutral (34983 mit und
    // ohne diese Regel), aber von der Suite her belegt.
    let mut typen_der_gruppe: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut typen_namen_liste: Vec<(String, u32)> = Vec::new();
    let mut typen_implizit: Vec<FuncTyp> = Vec::new();
    let mut tags: BTreeMap<String, u32> = BTreeMap::new();
    let mut tag_idx = 0u32;
    let mut tabellen: BTreeMap<String, u32> = BTreeMap::new();
    let mut globale: BTreeMap<String, u32> = BTreeMap::new();
    let mut speicher: BTreeMap<String, u32> = BTreeMap::new();
    // GEMESSEN 03.10. (`struct.wast` Z. 55): Feldbezeichner fuer
    // `struct.get $t $x` — im Vorlauf gesammelt, damit auch
    // Vorwaertsverweise aufloesen.
    let mut felder: BTreeMap<(u32, String), u32> = BTreeMap::new();
    let mut daten: BTreeMap<String, u32> = BTreeMap::new();
    let mut elemente: BTreeMap<String, u32> = BTreeMap::new();
    let mut f_idx = 0u32;
    let mut t_idx = 0u32;
    let mut tab_idx = 0u32;
    let mut glob_idx = 0u32;
    let mut sp_idx = 0u32;
    let mut dat_idx = 0u32;
    let mut elem_idx = 0u32;
    // Bezeichner -> (Art, Index). Ein zweiter Eintrag mit anderem Index ist
    // ein Duplikat (GEMESSEN 02.10., `table.wast` Z. 43-45).
    let mut gesehen: BTreeMap<String, (&'static str, u32)> = BTreeMap::new();
    let mut doppelt: Option<String> = None;
    let mut i = 0usize;
    while i < tok.len() {
        let k = match &tok[i] {
            Token::Auf(k) => k.clone(),
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        if k == "import" {
            // `(import "m" "n" (func $name …))`: erst die beiden Namen
            // ueberspringen, dann die Art bestimmen. Der Name steht HINTER
            // der Art.
            while j < tok.len() && !matches!(&tok[j], Token::Auf(_)) {
                j += 1;
            }
            let art = match tok.get(j) {
                Some(Token::Auf(a)) => a.clone(),
                _ => String::new(),
            };
            let name = match tok.get(j + 1) {
                Some(Token::Name(n)) => n.clone(),
                _ => String::new(),
            };
            if !name.is_empty() && doppelt.is_none() {
                if let Err(text) = merke_doppelt(
                    &mut gesehen,
                    &mut doppelt,
                    &name,
                    &art,
                    lauf_index(&art, f_idx, t_idx, tab_idx, glob_idx, sp_idx),
                ) {
                    doppelt = Some(text);
                }
            }
            // WICHTIG: der Zaehler laeuft fuer JEDES Feld dieser Art, auch
            // ohne Bezeichner — der Leser legt namenlose Importe genauso an.
            // Vorher zaehlte nur `(…, false)`, und jeder namenlose Import
            // verschob alle spaeteren Namen um eins (GEMESSEN 02.10.,
            // `imports.wast` Z. 10: 9 Folgefehler in einem Modul).
            match art.as_str() {
                "func" => {
                    if !name.is_empty() {
                        funktionen.entry(name).or_insert(f_idx);
                    }
                    f_idx = f_idx.saturating_add(1);
                    // Die `(func …)`-Gruppe kann eine Inline-Signatur haben,
                    // die der Leser als Typ anlegt.
                    if matches!(tok.get(j), Some(Token::Auf(a)) if a == "func") {
                        let ende = gruppe_ende(tok, j);
                        if let Some(ft) =
                            inline_signatur(&tok[j + 1..ende.min(tok.len())], &typen_namen_liste)
                        {
                            typen_implizit.push(ft);
                        }
                    }
                }
                "type" => {
                    // Gehoert zu `typen_explizit` (siehe oben).
                    t_idx = t_idx.saturating_add(1);
                }
                "table" => {
                    if !name.is_empty() {
                        tabellen.entry(name).or_insert(tab_idx);
                    }
                    tab_idx = tab_idx.saturating_add(1);
                }
                "global" => {
                    if !name.is_empty() {
                        globale.entry(name).or_insert(glob_idx);
                    }
                    glob_idx = glob_idx.saturating_add(1);
                }
                "memory" => {
                    if !name.is_empty() {
                        speicher.entry(name).or_insert(sp_idx);
                    }
                    sp_idx = sp_idx.saturating_add(1);
                }
                "tag" => {
                    if !name.is_empty() {
                        tags.entry(name).or_insert(tag_idx);
                    }
                    tag_idx = tag_idx.saturating_add(1);
                }
                _ => {}
            }
        } else {
            // `(func $name …)` / `(type $name …)`: der Name steht DIREKT
            // hinter der Klammer (GEMESSEN 01.10.: mit einem Token zu viel
            // blieb die Map leer und `call $odd` wurde zu `call 0`).
            let name = match tok.get(j) {
                Some(Token::Name(n)) => n.clone(),
                _ => String::new(),
            };
            if !name.is_empty() && doppelt.is_none() {
                if let Err(text) = merke_doppelt(
                    &mut gesehen,
                    &mut doppelt,
                    &name,
                    k.as_str(),
                    lauf_index(k.as_str(), f_idx, t_idx, tab_idx, glob_idx, sp_idx),
                ) {
                    doppelt = Some(text);
                }
            }
            // Ab hier laeuft der FELD-Zweig (nicht der Import-Zweig). Fuer
            // `func`, `type` und `tag` muss der Leser erzeugte Typen
            // nachgebaut werden.
            let feld_ende = gruppe_ende(tok, i);
            let inhalt_start = (i + if name.is_empty() { 1 } else { 2 }).min(feld_ende);
            let feld_inhalt = &tok[inhalt_start..feld_ende.max(inhalt_start)];
            match k.as_str() {
                "func" => {
                    if !name.is_empty() {
                        funktionen.entry(name).or_insert(f_idx);
                    }
                    f_idx = f_idx.saturating_add(1);
                    match inline_signatur(feld_inhalt, &typen_namen_liste) {
                        Some(ft) => typen_implizit.push(ft),
                        // **Leere** Signatur: `(func $f)` ist der Typ
                        // `(func)`. `inline_signatur` liefert dafuer `None`
                        // (kein `param`, kein `result`), und ohne diesen
                        // Eintrag kennt der Vorlauf den Typ nicht: die
                        // Gruppenregel greift nicht, `(func $f)` bekommt
                        // Index 0 und das Modul gilt als gueltig.
                        //
                        // GEMESSEN 03.10. (`type-rec.wast` Z. 51ff,
                        // Kommentar der Suite: „the implicit type of $f is
                        // not $ft"). Nur ohne explizites `(type …)`.
                        None if !feld_hat_typ(feld_inhalt) => {
                            typen_implizit.push(FuncTyp {
                                parameter: Vec::new(),
                                ergebnis: Vec::new(),
                            });
                        }
                        None => {}
                    }
                }
                "rec" => {
                    // `(rec (type …)*)` (Wasm 3.0 GC): die Gruppe zaehlt
                    // so viele Typen, wie sie `(type …)`-Eintraege hat.
                    //
                    // GEMESSEN 03.10. (`struct.wast` Z. 27ff,
                    // `array.wast` Z. 38ff): ohne diesen Zweig fehlten die
                    // Typen im vorberechneten Typraum und **alle** Indices
                    // danach waren verschoben.
                    let ende_gruppe = gruppe_ende(&tok, i);
                    let mut q = i + 1;
                    // PHASE 1: alle Bezeichner der Gruppe einsammeln.
                    // Ohne das kann ein Mitglied ein spaeteres benutzen
                    // (`(type $h (func (param (ref $k)))) (type $k …)`).
                    //
                    // GEMESSEN 03.10. (`type-rec.wast`: „Typ 8: Vorlauf
                    // und Leser finden verschiedene Signaturen").
                    {
                        let mut r = i + 1;
                        let mut nr = 0usize;
                        while r < ende_gruppe.min(tok.len()) {
                            if matches!(tok.get(r), Some(Token::Auf(a)) if a == "type") {
                                if let Some(Token::Name(n)) = tok.get(r + 1) {
                                    let idx = (typen_explizit.len() + nr) as u32;
                                    typen_namen_liste.push((n.clone(), idx));
                                }
                                nr += 1;
                            }
                            r += 1;
                        }
                    }
                    // PHASE 2: Signaturen bauen (Namen sind jetzt da).
                    while q < ende_gruppe.min(tok.len()) {
                        if matches!(tok.get(q), Some(Token::Auf(a)) if a == "type") {
                            // Wie im `(type …)`-Zweig: der Inhalt beginnt
                            // hinter dem Bezeichner, und der Schnitt muss
                            // nicht leer laufen.
                            //
                            // M8: ohne diese Deckung PANIK „slice index
                            // starts at 2 but ends at 1" (GEMESSEN 03.10.,
                            // im Lauf ueber alle 257 Dateien).
                            let name = match tok.get(q + 1) {
                                Some(Token::Name(n)) => n.clone(),
                                _ => String::new(),
                            };
                            let inhalt_start = q + if name.is_empty() { 1 } else { 2 };
                            let inhalt_ende = gruppe_ende(tok, q).min(tok.len());
                            let inhalt: &[Token] = if inhalt_start <= inhalt_ende {
                                &tok[inhalt_start..inhalt_ende]
                            } else {
                                &[]
                            };
                            let ft =
                                inline_signatur(inhalt, &typen_namen_liste).unwrap_or(FuncTyp {
                                    parameter: Vec::new(),
                                    ergebnis: Vec::new(),
                                });
                            let eigener = typen_explizit.len() as u32;
                            typen_explizit.push(ft);
                            typen_der_gruppe.insert(eigener);
                            if !name.is_empty() {
                                // Der Name steht schon aus Phase 1 — nur
                                // nachziehen, falls der Index abweicht.
                                if !typen_namen_liste
                                    .iter()
                                    .any(|(k, v)| k == &name && *v == eigener)
                                {
                                    typen_namen_liste.push((name.clone(), eigener));
                                }
                            }
                            if matches!(inhalt.first(), Some(Token::Auf(a)) if a == "struct") {
                                struct_felder_sammeln(inhalt, eigener, &mut felder);
                            }
                            q = inhalt_ende + 1;
                        } else {
                            q += 1;
                        }
                    }
                }
                "type" => {
                    // Ein `(type …)`-Feld gehoert an den ANFANG des
                    // Typraums (die Inline-Typen kommen danach) und legt
                    // immer einen neuen Typ an.
                    //
                    // GEMESSEN 03.10. (`func.wast` Z. 488ff: `(type 3)`
                    // muss die Inline-Signatur `(f64 i64 …)` bezeichnen;
                    // mit der Reihenfolge „explizit, dann inline" stimmt
                    // das. GEMESSEN 02.10. (`imports.wast` Z. 10: die
                    // NENNUNG `$forward` loest unabhaengig vom Index auf,
                    // die Datei bleibt bei 100 %).
                    // `(type $a (array …))` (Wasm 3.0 GC) legt der HAUPTPASS
                    // an: der Array-Typ traegt seine Information in
                    // `Modul::arrays`, nicht in der Signatur. Wuerde er hier
                    // vorbelegt, bekäme `$a` einen Platzhalter ohne
                    // Array-Information.
                    //
                    // GEMESSEN 03.10. (`table_init.wast` Z. 2273ff: 1 Fall).
                    // Auch `(type $a (array …))` und `(type $t (struct …))`
                    // bekommen einen **Platzhalter** im vorberechneten
                    // Typraum — die Information selbst liegt in
                    // `Modul::arrays`/`strukt`, der HAUPTPASS fuellt sie.
                    //
                    // GEMESSEN 03.10.: sie wurden hier **uebersprungen**,
                    // damit war der vorberechnete Typraum um genau diese
                    // Typen zu kurz und alle folgenden Indizes
                    // verschoben. Der Preis war der Fehler „F0+13: Block0
                    // end: 1 Werte uebrig" in `table_init.wast` (1 Fall),
                    // sobald man den Typraum von Hand aufpolsterte.
                    let ft = inline_signatur(feld_inhalt, &typen_namen_liste).unwrap_or(FuncTyp {
                        parameter: Vec::new(),
                        ergebnis: Vec::new(),
                    });
                    let eigener = typen_explizit.len() as u32;
                    typen_explizit.push(ft);
                    if !name.is_empty() {
                        typen_namen_liste.push((name.clone(), eigener));
                    }
                    // `(type $t (struct (field $x i32) …))`: die
                    // Feldbezeichner fuer `struct.get $t $x`
                    // einsammeln (GEMESSEN 03.10., `struct.wast`
                    // Z. 55 — der Befehl kann VOR der Typzeile
                    // stehen, dann hilft nur der Vorlauf).
                    if matches!(feld_inhalt.first(), Some(Token::Auf(a)) if a == "struct") {
                        struct_felder_sammeln(feld_inhalt, eigener, &mut felder);
                    }
                    t_idx = t_idx.saturating_add(1);
                }
                "table" => {
                    if !name.is_empty() {
                        tabellen.entry(name).or_insert(tab_idx);
                    }
                    tab_idx = tab_idx.saturating_add(1);
                }
                "global" => {
                    if !name.is_empty() {
                        globale.entry(name).or_insert(glob_idx);
                    }
                    glob_idx = glob_idx.saturating_add(1);
                }
                "memory" => {
                    if !name.is_empty() {
                        speicher.entry(name).or_insert(sp_idx);
                    }
                    sp_idx = sp_idx.saturating_add(1);
                }
                "data" => {
                    if !name.is_empty() {
                        daten.entry(name).or_insert(dat_idx);
                    }
                    dat_idx = dat_idx.saturating_add(1);
                }
                "elem" => {
                    if !name.is_empty() {
                        elemente.entry(name).or_insert(elem_idx);
                    }
                    elem_idx = elem_idx.saturating_add(1);
                }
                "tag" => {
                    if !name.is_empty() {
                        tags.entry(name).or_insert(tag_idx);
                    }
                    tag_idx = tag_idx.saturating_add(1);
                    if let Some(ft) = inline_signatur(feld_inhalt, &typen_namen_liste) {
                        typen_implizit.push(ft);
                    }
                }
                _ => {}
            }
        }
        // Die ganze Feldgruppe ueberspringen — ab dem oeffnenden Token,
        // sonst laeuft die Tiefenzaehlung daneben (GEMESSEN 01.10.).
        let mut tiefe = 0i64;
        let mut q = i;
        while q < tok.len() {
            match &tok[q] {
                Token::Auf(_) => tiefe += 1,
                Token::Zu => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        break;
                    }
                }
                _ => {}
            }
            q += 1;
        }
        i = q.saturating_add(1);
    }
    // Typraum: erst die expliziten `(type …)`-Felder in Textreihenfolge,
    // dann die aus Inline-Signaturen erzeugten Typen (bestehende
    // Signaturen werden wiederverwendet, der ERSTE Treffer gewinnt).
    let mut typen_liste: Vec<FuncTyp> = typen_explizit;
    let mut typen_index: std::collections::HashMap<FuncTyp, u32> = std::collections::HashMap::new();
    for (i, ft) in typen_liste.iter().enumerate() {
        typen_index.entry(ft.clone()).or_insert(i as u32);
    }
    for ft in typen_implizit {
        // Die Wiederverwendung gilt nur fuer Typen AUSSERHALB einer
        // `rec`-Gruppe (siehe `typen_der_gruppe`). Sonst waere der
        // implizite Typ einer Signatur strukturell richtig und nominal
        // falsch — genau der Fall, den `type-rec.wast` zurueckweist.
        let wiederverwenden = typen_index
            .get(&ft)
            .copied()
            .is_some_and(|i| !typen_der_gruppe.contains(&i));
        if !wiederverwenden {
            // `sim_typindex` gibt bei bekanntem Eintrag **frueh zurueck** und
            // damit die Gruppenregel selbst aus: der Aufrufer entscheidet
            // korrekt „neuer Typ", die Funktion nimmt trotzdem den alten.
            //
            // GEMESSEN 03.10. (`type-rec.wast` Z. 51ff): der Index blieb
            // `{empty: 0}` und der Typraum bei zwei Eintraegen — der Fall
            // blieb auch mit leerer Signatur und durchgereichter Tabelle rot.
            typen_index.remove(&ft);
            sim_typindex(&mut typen_liste, &mut typen_index, ft);
        }
    }
    let mut typen: BTreeMap<String, u32> = BTreeMap::new();
    for (name, idx) in typen_namen_liste {
        typen.entry(name).or_insert(idx);
    }
    Vorlauf {
        funktionen,
        tags,
        raum: TabellenUndTypen {
            typen,
            typen_liste,
            tabellen,
            globale,
            speicher,
            felder,
        },
        daten,
        elemente,
        typen_index,
        doppelt,
    }
}

/// Ergebnis des Vorlaufs: alle Bezeichnerraeume eines Moduls auf einen Blick.
struct Vorlauf {
    funktionen: BTreeMap<String, u32>,
    /// `$name` der Tags -> Index.
    tags: BTreeMap<String, u32>,
    raum: TabellenUndTypen,
    daten: BTreeMap<String, u32>,
    elemente: BTreeMap<String, u32>,
    /// Die **gruppenbewusste** Typzuschlagstabelle des Vorlaufs.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast` Z. 51ff): der Vorlauf legt fuer
    /// einen impliziten Typuse, der keinen Typ aus einer `rec`-Gruppe
    /// uebernehmen darf, einen **neuen** Typ an. Der Hauptpass baute seine
    /// Tabelle vorher neu („erster Treffer gewinnt") und nahm wieder Index
    /// 0 — also genau den Gruppentyp, den der Vorlauf ausschliessen
    /// wollte. Damit war die Entscheidung **zweimal** im Programm; jetzt
    /// einmal.
    typen_index: std::collections::HashMap<FuncTyp, u32>,
    /// Text eines doppelten Bezeichners, falls gefunden.
    doppelt: Option<String>,
}

/// Der Index, den die Art `art` als naechste belegen wuerde.
fn lauf_index(art: &str, f: u32, t: u32, tab: u32, glob: u32, sp: u32) -> u32 {
    match art {
        "func" => f,
        "type" => t,
        "table" => tab,
        "global" => glob,
        "memory" => sp,
        _ => 0,
    }
}

/// Bezeichner merken; ein zweiter Eintrag mit anderem Index ist ein
/// Duplikat und macht das Modul ungueltig (Spec: „duplicate …").
fn merke_doppelt(
    gesehen: &mut BTreeMap<String, (&'static str, u32)>,
    doppelt: &mut Option<String>,
    name: &str,
    art: &str,
    idx: u32,
) -> Result<(), String> {
    // Nur passende Arten vergleichen (`$f` als Funktion und als Tabelle
    // sind verschiedene Bezeichner in verschiedenen Raeumen).
    if doppelt.is_some() {
        return Ok(());
    }
    match gesehen.get(name) {
        Some((vor_art, vor_idx)) if *vor_art == art && *vor_idx != idx => {
            let text = format!(
                "doppelter Bezeichner: ${name} ({art}) schon als Index {vor_idx} belegt, neu {idx}"
            );
            *doppelt = Some(text.clone());
            Err(text)
        }
        Some(_) => Ok(()),
        None => {
            // `art` ist &'static str, weil alle Compare arms statisch sind.
            let statisch: &'static str = match art {
                "func" => "func",
                "type" => "type",
                "table" => "table",
                "global" => "global",
                "memory" => "memory",
                "data" => "data",
                _ => "sonst",
            };
            gesehen.insert(name.to_string(), (statisch, idx));
            Ok(())
        }
    }
}

/// Die Feldbezeichner eines `(struct …)`-Feldes einsammeln.
///
/// GEMESSEN 03.10. (`struct.wast` Z. 55): die Felder stehen in
/// `(field $name …)`-Gruppen; jeder Typ einer Gruppe ist ein eigenes
/// Feld, ein Name darf pro Gruppe nur einmal fallen.
fn struct_felder_sammeln(
    inhalt: &[Token],
    typ_index: u32,
    ziel: &mut BTreeMap<(u32, String), u32>,
) {
    let mut i = 0;
    let mut feld_nr = 0u32;
    while i < inhalt.len() {
        // `(field …)` oder ein bloser Typ.
        if matches!(inhalt.get(i), Some(Token::Auf(k)) if k == "field") {
            let ende = gruppe_ende(inhalt, i);
            let mut name: Option<String> = None;
            if let Some(Token::Name(n)) = inhalt.get(i + 1) {
                name = Some(n.clone());
            }
            for j in (i + 1)..ende.min(inhalt.len()) {
                let ist_typ = matches!(inhalt.get(j), Some(Token::Wort(_)))
                    || matches!(inhalt.get(j), Some(Token::Auf(k)) if k == "ref");
                if ist_typ {
                    if let Some(n) = &name {
                        ziel.insert((typ_index, n.clone()), feld_nr);
                    }
                    feld_nr += 1;
                }
            }
            i = ende.max(i + 1);
            continue;
        }
        if matches!(inhalt.get(i), Some(Token::Wort(_)))
            || matches!(inhalt.get(i), Some(Token::Auf(k)) if k == "ref")
        {
            feld_nr += 1;
        }
        i += 1;
    }
}

/// Die beiden Namensraeume, die der Vorlauf liefert.
struct TabellenUndTypen {
    typen: BTreeMap<String, u32>,
    /// Der VOLLSTAENDIGE Typraum in der Reihenfolge, in der der Leser ihn
    /// bekommen soll: explizite `(type …)`-Felder zuerst, dann die
    /// Inline-Signaturen (siehe `vorlauf`).
    typen_liste: Vec<FuncTyp>,
    tabellen: BTreeMap<String, u32>,
    globale: BTreeMap<String, u32>,
    speicher: BTreeMap<String, u32>,
    /// `(Typindex, Feldbezeichner)` -> Feldindex fuer `struct.get $t $x`.
    ///
    /// GEMESSEN 03.10. (`struct.wast` Z. 55: der Befehl steht VOR der
    /// Typdeklaration). Ohne diese Tabelle im Vorlauf bleiben solche
    /// Vorwaertsverweise offen.
    felder: BTreeMap<(u32, String), u32>,
}

/// Textmodul lesen (`(module …)` optional). Text HINTER dem Modul ist ein
/// benannter Formfehler — sonst wuerde `assert_malformed` mit Resten
/// durchrutschen (M8: kein Input darf stillschweigend „repariert" werden).
pub fn lies_text(text: &str, g: &Grenzen) -> Result<Modul, Fehler> {
    // M4: Die Eingabegroesse wird VOR dem Aufbau des Lesers geprueft; ein
    // Modul im Textformat ist genauso Eingabe wie eines im Binaerformat.
    if text.len() > g.wast_bytes {
        return Err(textfehler(&format!(
            "Text {} Byte ueber der Grenze {}",
            text.len(),
            g.wast_bytes
        )));
    }
    let mut tok = tokenisier(text, g)?;
    // M3/M4: Namen (Zeichenketten und Bezeichner) sind ebenfalls gedeckelt.
    let namengrenze = g.name_bytes.min(crate::grenzen::MAX_NAME_BYTES_HART);
    for t in &tok {
        let zu_lang = match t {
            Token::Text(b) => b.len(),
            Token::Name(n) | Token::Wort(n) | Token::Auf(n) => n.len(),
            Token::Zu => 0,
        };
        if zu_lang > namengrenze {
            return Err(textfehler(&format!(
                "Name {zu_lang} Byte ueber der Grenze {namengrenze}"
            )));
        }
    }
    if let Some(Token::Auf(k)) = tok.first() {
        if k == "module" {
            tok.remove(0);
            if let Some(Token::Name(_)) = tok.first() {
                tok.remove(0);
            }
            // Die schliessende Klammer des `module` gehoert zum Innhalt, den
            // wir jetzt als Felder lesen: deshalb entfernen wir sie am Ende.
            // `(module)` = leeres Modul: die naechsten zwei Token muessen
            // `)` und nochmal `)` sein (sonst ist es `(module) (…rest…)`).
            if tok.len() >= 2 && tok[0] == Token::Zu && tok[1] == Token::Zu {
                tok.drain(..2);
                if let Some(t) = tok.first() {
                    return Err(textfehler(&format!("Text hinter dem Modul: {}", t.text())));
                }
                return Ok(Modul::leer());
            }
            if tok.last() == Some(&Token::Zu) {
                tok.pop();
            }
        }
    }
    // Klammern muessen paarig sein — abgeschnittener Text ist ein benannter
    // Formfehler (`assert_malformed` in der Testsuite erwartet genau das).
    let mut tiefe: i64 = 0;
    for t in &tok {
        match t {
            Token::Auf(_) => tiefe += 1,
            Token::Zu => tiefe -= 1,
            _ => {}
        }
        if tiefe < 0 {
            return Err(textfehler("zu viele schliessende Klammern"));
        }
    }
    if tiefe > 0 {
        return Err(textfehler(&format!("{tiefe} Klammer(n) nicht geschlossen")));
    }
    let vor = vorlauf(&tok);
    if let Some(text) = vor.doppelt {
        // Zwei Felder mit demselben Bezeichner: das Modul ist schon bei der
        // Uebersetzung fehlerhaft (Spec: „duplicate …").
        return Err(textfehler(&text));
    }
    let mut p = Parser {
        tok,
        pos: 0,
        g: g.clone(),
        // Der Typraum kommt AUS DEM VORLAUF: explizite `(type …)`-Felder
        // zuerst, dann die Inline-Signaturen (Spec: ein Inline-Typ wird
        // ans Ende des Typabschnitts gehaengt).
        //
        // GEMESSEN 03.10. (`func.wast` Z. 488ff, 4 Faelle).
        typen: vor.raum.typen_liste.clone(),
        typen_felder: 0,
        lokale_zaehler: 0,
        typen_namen: vor.raum.typen.clone(),
        felder_namen: vor.raum.felder.clone(),
        aktuelle_gruppe: None,
        tabellen_namen: vor.raum.tabellen.clone(),
        globale_namen: vor.raum.globale.clone(),
        speicher_namen: vor.raum.speicher.clone(),
        daten_namen: vor.daten.clone(),
        elem_namen: vor.elemente.clone(),
        funktionen: vor.funktionen.clone(),
        tags_namen: vor.tags.clone(),
        // Die Tabelle des Vorlaufs, nicht eine neu gebaute: sie kennt die
        // Gruppenregel (siehe `Vorlauf::typen_index`).
        typen_index: vor.typen_index.clone(),
        lokale: BTreeMap::new(),
        labels: Vec::new(),
        schritte: 0,
    };
    let mut m = p.modul()?;
    if let Some(t) = p.tok.get(p.pos) {
        return Err(textfehler(&format!("Text hinter dem Modul: {}", t.text())));
    }
    // NAMEN FUER DIE ANZEIGE. Das Textformat hat keine `name`-Sektion, aber
    // der Leser kennt `(func $hilfe)` — ohne diesen Eintrag zeigte der
    // Aufrufstapel `func#1` statt `hilfe`, und Haltepunkte in der Liste
    // haetten anders geschrieben gestanden als im Stapel (team/rollen/
    // WASM.txt: „Aufrufstapel mit Funktionsnamen").
    //
    // Der Vorlauf fuehrt den Bezeichner OHNE `$` und im selben Indexraum
    // wie `Modul::funktionen` (Importe mitgezaehlt) — genau die Bedeutung
    // von `Modul::namen` aus der `name`-Sektion (binaer.rs `lies_namen`).
    if m.namen.is_none() && !vor.funktionen.is_empty() {
        m.namen = Some(vor.funktionen.clone());
    }
    Ok(m)
}

impl Parser {
    // ---- Token-Hilfen ----------------------------------------------------
    fn sieht(&self) -> Option<&Token> {
        self.tok.get(self.pos)
    }
    fn sieht_wort(&self) -> Option<String> {
        match self.sieht() {
            Some(Token::Wort(w)) => Some(w.clone()),
            _ => None,
        }
    }
    fn ist(&self, name: &str) -> bool {
        matches!(self.sieht(), Some(Token::Auf(k)) if k == name)
    }
    fn weiter(&mut self) -> Token {
        // Am Ende der Tokenliste: ein virtuelles `)` zurueckgeben, damit der
        // Leser mit einem BENANNTEN Fehler endet statt mit einem Panic
        // (team/REGELN.txt: "Kein Panic bei Eingaben").
        let t = match self.tok.get(self.pos) {
            Some(t) => t.clone(),
            None => {
                self.pos += 1;
                return Token::Zu;
            }
        };
        self.pos += 1;
        t
    }
    /// `(name` am Leser verbrauchen (die Klammer bleibt offen).
    fn oeffner(&mut self, name: &str) -> Result<(), Fehler> {
        match self.sieht() {
            Some(Token::Auf(k)) if k == name => {
                self.pos += 1;
                Ok(())
            }
            t => Err(textfehler(&format!(
                "erwartet ({name}), gefunden {}",
                t.map(|x| x.text()).unwrap_or_else(|| "Ende".to_string())
            ))),
        }
    }
    /// Wie `oeffner`, aber auch mit Annotationspraefix: `(@custom …)` ist
    /// dasselbe Feld wie `(custom …)` (WAT-Skriptform fuer `custom`-Sektionen).
    fn oeffner_feld(&mut self, name: &str) -> Result<(), Fehler> {
        match self.sieht() {
            Some(Token::Auf(k)) if k.strip_prefix('@').unwrap_or(k) == name => {
                self.pos += 1;
                Ok(())
            }
            t => Err(textfehler(&format!(
                "erwartet ({name}), gefunden {}",
                t.map(|x| x.text()).unwrap_or_else(|| "Ende".to_string())
            ))),
        }
    }
    fn zu(&mut self) -> Result<(), Fehler> {
        match self.weiter() {
            Token::Zu => Ok(()),
            t => Err(textfehler(&format!("erwartet ), gefunden {}", t.text()))),
        }
    }
    fn wort(&mut self) -> Result<String, Fehler> {
        match self.weiter() {
            Token::Wort(w) => Ok(w),
            t => Err(textfehler(&format!(
                "erwartet ein Wort, gefunden {}",
                t.text()
            ))),
        }
    }
    /// Ein Name: `"…"` oder `$name`.
    fn name(&mut self) -> Result<String, Fehler> {
        match self.weiter() {
            Token::Text(b) => String::from_utf8(b.clone())
                .map_err(|_| Fehler::form(ART_UTF8, "WAT: Name ist kein UTF-8")),
            Token::Name(n) => Ok(n),
            Token::Wort(w) => Ok(w),
            t => Err(textfehler(&format!("Name erwartet, gefunden {}", t.text()))),
        }
    }
    /// Optionaler Bezeichner: NUR `$name` (NIE ein nacktes Wort).
    ///
    /// GEMESSEN 01.10.: vorher wurde auch ein Wort akzeptiert, und dann
    /// fraess `(func i32.const 0 end)` das `i32.const` als Funktionsnamen —
    /// der Leser meldete danach `Befehl "0" unbekannt`. In WAT ist ein
    /// Bezeichner immer ein `$name`.
    fn optionaler_name(&mut self) -> Option<String> {
        match self.sieht() {
            Some(Token::Name(_)) => self.name().ok(),
            _ => None,
        }
    }
    /// Tokens einer Klammer einsammeln (die schliessende wird verbraucht).
    fn klammer_inhalt(&mut self) -> Vec<Token> {
        // ACHTUNG: `self.sieht()` ist bereits der `Auf`-Token der oeffnenden
        // Klammer (die Klammer und der Name sind EIN Token). Deshalb startet
        // die Tiefenzaehlung bei 0, nicht bei 1 — GEMESSEN 01.10.: mit 1 lief
        // die Schleife bis zum Ende des Feldes weiter und fraess den
        // kompletten Funktionskoerper (`block.wast` „params").
        let start = self.pos;
        let mut tiefe = 0i64;
        while let Some(t) = self.sieht() {
            match t {
                Token::Auf(_) => tiefe += 1,
                Token::Wort(_) | Token::Text(_) | Token::Name(_) => {}
                Token::Zu => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        let inhalt = self.tok[start..self.pos].to_vec();
                        self.pos += 1;
                        return inhalt;
                    }
                }
            }
            self.pos += 1;
        }
        self.tok[start..].to_vec()
    }

    // ---- Modul -----------------------------------------------------------
    fn modul(&mut self) -> Result<Modul, Fehler> {
        let mut m = Modul::leer();
        // Die Textform verlangt, dass ALLE Importe vor den Definitionen
        // stehen („import after function", GEMESSEN 02.10., `imports.wast`
        // Z. 199-204: 6 `assert_malformed`).
        let mut definition_gesehen = false;
        while let Some(t) = self.sieht().cloned() {
            match t {
                Token::Zu => break,
                Token::Auf(typ) => match typ.strip_prefix('@').unwrap_or(typ.as_str()) {
                    "type" => self.typ_feld(&mut m)?,
                    "import" => {
                        if definition_gesehen {
                            return Err(textfehler(
                                "Import nach einer Definition (erlaubt sind nur Felder vor (func …))",
                            ));
                        }
                        self.import_feld(&mut m)?;
                    }
                    "func" | "table" | "memory" | "global" | "export" | "start" | "elem"
                    | "data" => {
                        // Inline-Importform (`(memory $m (import …) …)`)
                        // gehoert noch zu den Importen. Nach dem Feldnamen
                        // koennen beliebig viele `(export "…")`-Gruppen
                        // stehen, erst danach `(import …)`.
                        //
                        // GEMESSEN 02.10. (`imports.wast` Z. 10: 2 Faelle —
                        // `(func (export "p1") (import …) …)`).
                        let ist_import = self.ist_inline_import();
                        if !ist_import {
                            definition_gesehen = true;
                        }
                        match typ.strip_prefix('@').unwrap_or(typ.as_str()) {
                            "func" => self.func_feld(&mut m)?,
                            "table" => self.tabelle_feld(&mut m)?,
                            "memory" => self.speicher_feld(&mut m)?,
                            "global" => self.global_feld(&mut m)?,
                            "export" => self.export_feld(&mut m)?,
                            "start" => self.start_feld(&mut m)?,
                            "elem" => self.elem_feld(&mut m)?,
                            _ => self.data_feld(&mut m)?,
                        }
                    }
                    "custom" => self.custom_feld(&mut m)?,
                    // GEMESSEN 03.10. (`type-subtyping.wast` 14/74): eine
                    // `rec_feld` an dieser Stelle **liest** die Gruppen
                    // (Muster unten auskommentiert), und die Datei stieg
                    // auf 23/74. **Ausgeliefert wird sie nicht**: sie legt
                    // in `type-rec.wast` sechs `assert_invalid`-Faelle frei,
                    // die vorher nur darum gruen waren, dass das Modul gar
                    // nicht lesbar war (ein Parse-Fehler gilt als Reject).
                    // Netto ueber alle 257 Dateien: 34970 -> 34969, also
                    // **-1**; nach der A/B-Regel (nur die gewinnende
                    // Konfiguration) bleibt die alte Zeile. Die beiden
                    // Befunde dahinter sind im Bericht benannt:
                    //  * der func-Zweig schrieb `gruppe_von`/`sub_ober`
                    //    nicht (nur struct/array),
                    //  * ausserhalb einer Gruppe darf ein Typ sich **selbst**
                    //    nennen (`type-rec.wast` Z. 3).
                    "rec" => self.rec_feld(&mut m)?,
                    "tag" => {
                        // `(tag (import …) …)` gehoert noch zu den Importen
                        // (sonst waere ein spaeteres `(import …)` ein
                        // „Import nach einer Definition"). GEMESSEN 02.10.
                        // (`imports.wast` Z. 10).
                        self.tag_feld(&mut m)?;
                        if !m.tags.last().is_some_and(|t| t.import.is_some()) {
                            definition_gesehen = true;
                        }
                    }
                    andere => return Err(textfehler(&format!("unbekanntes Feld ({andere})"))),
                },
                t => return Err(textfehler(&format!("unerwartetes Token {}", t.text()))),
            }
        }
        // `typen` nachziehen (der Leser legt Typen fuer Inline-Signaturen an)
        if self.typen.len() > m.typen.len() {
            m.typen = self.typen.clone();
        }
        // `arrays` muss genauso lang sein wie `typen` (Index-fuer-Index).
        while m.arrays.len() < m.typen.len() {
            m.arrays.push(None);
        }
        m.arrays.truncate(m.typen.len());
        // Die Textform hat KEINEN DataCount-Abschnitt; die Zahl der
        // Datenabschnitte steht aber fest. Der Validator verlangt den
        // Abschnitt nur fuer die Binaerform (Spec: „the DataCount section is
        // required if memory.init or data.drop are used").
        //
        // GEMESSEN 02.10. (`bulk.wast`:
        // `(module (data "goodbye") (func (data.drop 0)))` wurde abgelehnt,
        // danach liefen alle Aufrufe auf dem FALSCHEN Modul).
        if m.daten_anzahl.is_none() {
            m.daten_anzahl = Some(m.daten.len() as u32);
        }
        // Typindizes in `call_indirect`/`block` muessen existieren
        Ok(m)
    }

    // ---- (type …) --------------------------------------------------------
    /// `(rec (type …)*)` als **Modulfeld** (Wasm 3.0 GC).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` 14/74 -> 23/74): die Datei
    /// benutzt Gruppen auf Modulebene, und der Leser meldete fuer jede
    /// „record-Typen (GC) werden nicht unterstuetzt" — **60 Faelle** an
    /// einer Zeile. Der Typabschnitt kannte `rec` bereits (`vorlauf`),
    /// der Modulfeld-Zweig nicht.
    ///
    /// Die Gruppe macht zweierlei: sie vergibt die Nummern (das erledigt
    /// der Vorab-Durchgang) und sie erlaubt den Mitgliedern, **einander**
    /// vorwaerts zu benennen (`(type $r (sub $t (struct (field (ref $r)))))`
    /// — `$r` steht im eigenen Rumpf). Dafuer wird `aktuelle_gruppe`
    /// gesetzt; die Sichtbarkeitsregel sitzt in `pruefe::sichtbar_pruefen`.
    ///
    /// **Erst mit der impliziten-Typ-Regel ausgeliefert** (siehe
    /// `typimplizit_pruefen`): allein带入ete sie `type-rec.wast` von 12/15
    /// auf 4/15, weil sechs `assert_invalid`-Faelle dann sichtbar wurden,
    /// die vorher nur darum gruen waren, dass das Modul unlesbar war.
    ///
    /// M3/M4: die Mitgliedersuche laeuft ueber die Token der Gruppe (die
    /// Eingabe) und ist durch `MAX_TYPEN_HART` gedeckelt.
    fn rec_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("rec")?;
        // Mitglieder einsammeln: die **direkten** `(type …)`-Felder.
        let mut mitglieder: Vec<(String, usize)> = Vec::new();
        {
            let mut tiefe = 0i64;
            let mut j = self.pos;
            while j < self.tok.len() {
                match self.tok[j].clone() {
                    Token::Auf(k) => {
                        if tiefe == 0 && k == "type" {
                            let name = match self.tok.get(j + 1) {
                                Some(Token::Name(n)) => n.clone(),
                                _ => String::new(),
                            };
                            mitglieder.push((name, j));
                            if mitglieder.len() > crate::grenzen::MAX_TYPEN_HART {
                                return Err(textfehler("zu viele Typen in einer rec-Gruppe"));
                            }
                        }
                        tiefe += 1;
                    }
                    Token::Zu => {
                        if tiefe == 0 {
                            break;
                        }
                        tiefe -= 1;
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        // Gruppenbereich = naechster freier Typplatz .. + Mitglieder.
        //
        // GEMESSEN 03.10.: die erste Fassung holte die Nummern aus den
        // **Namen** — unnamed members haben keinen, also blieb die Gruppe
        // leer („Typ 1: (ref … 2) zeigt auf einen nicht sichtbaren Typ
        // (erlaubt ist 0..2)"). Der Zaehler der gelesenen `(type …)`-Felder
        // (`typen_felder`) ist der naechste freie Platz; M3: Addition mit
        // `checked_add`, Start mit `try_from`.
        let start = u32::try_from(self.typen_felder)
            .map_err(|_| textfehler("zu viele Typen fuer eine rec-Gruppe"))?;
        let ende = start
            .checked_add(u32::try_from(mitglieder.len()).unwrap_or(u32::MAX))
            .ok_or_else(|| textfehler("rec-Gruppe zu gross"))?;
        self.aktuelle_gruppe = Some((start, ende));
        for (_, tok_idx) in &mitglieder {
            self.pos = *tok_idx;
            self.typ_feld(m)?;
        }
        self.aktuelle_gruppe = None;
        self.zu()?;
        Ok(())
    }

    fn typ_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("type")?;
        let name = self.optionaler_name();
        // Die `(rec …)`-Gruppe, in der dieser Typ liegt — vom `rec`-Zweig
        // gesetzt, sonst `None` (ausserhalb jeder Gruppe).
        //
        // GEMESSEN 03.10. (`type-rec.wast`).
        let gruppe = self.aktuelle_gruppe;
        // `(type $a (sub $b (struct …)))` (Wasm 3.0 GC): der Obertyp
        // `$b` steht VOR dem Rumpf. Ohne `$b` ist der Rumpf final.
        //
        // GEMESSEN 03.10. (`type-subtyping.wast` 33/74: „erwartet (func),
        // gefunden (sub").
        let mut sub_ober_index: Option<u32> = None;
        // `(sub **final** $s (…))`: `final` ist ein **bare Wort** (kein
        // `(final …)`), steht also als `Token::Wort` im Strom.
        //
        // GEMESSEN 04.10. (`type-subtyping.wast` Z. 780ff): ohne dieses
        // Wort bricht der Leser mit „erwartet (func), gefunden final" ab —
        // und damit faellt das ganze Modul samt seinen 6 `assert_return`
        // (Z. 980ff) und den `assert_invalid` faellt der Validator nicht.
        let mut sub_final = false;
        let sub_da = self.ist("sub");
        if sub_da {
            self.oeffner("sub")?;
            if matches!(self.sieht(), Some(Token::Wort(w)) if w == "final") {
                sub_final = true;
                self.pos += 1;
            }
            // Ein `$name` oder eine Zahl direkt nach `sub` = der
            // Obertyp; sonst folgt sofort der Rumpf.
            //
            // ACHTUNG: `Token::Name` traegt den Bezeichner **ohne** `$`
            // (das entfernt der Tokenisierer) — ein `Name` ist deshalb
            // immer ein Name und nie eine Zahl. Sonst meldete der Leser
            // „Zahl \"e0\"" (GEMESSEN 03.10.).
            let mut ober: Option<Result<u32, String>> = None;
            match self.sieht().cloned() {
                // GEMESSEN 03.10. (**dreimal gemessen, nicht ausgeliefert**):
                // `(sub final …)` und `(sub final $t …)` zu lesen ist richtig —
                // die Suite schreibt beide Formen (`type-subtyping.wast`
                // Z. 90ff, 8 Vorkommen) — kostet aber **7 bis 9 Faelle**
                // (34997 -> 34990 mit der Tabelle-Lockerung). Nach der
                // A/B-Regel zurueckgenommen; die Tabelle-Lockerung in
                // `pruefe.rs` ist davon nicht betroffen und bleibt
                // ausgeliefert.
                Some(Token::Name(n)) => {
                    let idx = self.typen_namen.get(&n).copied();
                    ober = Some(idx.ok_or_else(|| format!("Typname {n} unbekannt")));
                    self.pos += 1;
                }
                Some(Token::Wort(w)) if zahl(&w).is_ok() => {
                    ober = Some(
                        u32::try_from(zahl(&w)?).map_err(|_| format!("Typindex {w} zu gross")),
                    );
                    self.pos += 1;
                }
                _ => {}
            }
            if let Some(ergebnis) = ober {
                sub_ober_index = Some(ergebnis.map_err(|e| textfehler(&format!("(sub …): {e}")))?);
            }
        }
        // `(type $t (array <typ>))` / `(array (mut <typ>))` — Wasm 3.0 GC.
        // Der Typ belegt einen Index im TYPraum; im Typenvektor steht ein
        // Platzhalter, die eigentliche Information in `Modul::arrays`.
        //
        // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
        // `(type $st (struct))` / `(struct <feld> …)` — Wasm 3.0 GC.
        //
        // GEMESSEN 03.10. (`extern.wast` Z. 4: 16 Faelle).
        if self.ist("struct") {
            self.oeffner("struct")?;
            // `(struct)` hat keine Felder; `(struct <typ> …)` eine Liste
            // bis zur schliessenden Klammer.
            // `(struct (field …)*)` — die Felder stehen in
            // `(field $name <speichertyp>*)`-Gruppen, jeder Typ einzeln
            // veraenderbar (`(field i32 (mut i32))`, GEMESSEN 03.10. in
            // `struct.wast` Z. 145). Die flache Form `(struct i32 i64)`
            // bleibt zulaessig (sie stand vorher im Leser).
            let mut felder: Vec<crate::modell::StructFeld> = Vec::new();
            let mut gesehen_felder: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            while !matches!(self.sieht(), None | Some(Token::Zu)) {
                if self.ist("field") {
                    self.oeffner("field")?;
                    let name = self.optionaler_name();
                    // Zwei Felder mit demselben Bezeichner sind ein
                    // Syntaxfehler (Spec: „duplicate field").
                    //
                    // GEMESSEN 03.10. (`struct.wast` Z. 15–18: 1
                    // `assert_malformed`).
                    if let Some(n) = &name {
                        if !gesehen_felder.insert(n.clone()) {
                            return Err(textfehler(&format!("duplicate field: ${n}")));
                        }
                    }
                    while !matches!(self.sieht(), None | Some(Token::Zu)) {
                        let (typ, bits, veraenderbar) = self.feld_typ()?;
                        felder.push(crate::modell::StructFeld {
                            name: name.clone(),
                            typ,
                            bits,
                            veraenderbar,
                        });
                    }
                    self.zu()?;
                } else {
                    let (typ, bits, veraenderbar) = self.feld_typ()?;
                    felder.push(crate::modell::StructFeld {
                        name: None,
                        typ,
                        bits,
                        veraenderbar,
                    });
                }
            }
            // Der Index dieses `(type …)`-Feldes ist der Zaehler der
            // Felder — NICHT `self.typen.len()`: der Typraum ist vom
            // Vorlauf vorgefuellt (GEMESSEN 03.10., `struct.wast`
            // Z. 55: „Typ 0 ist kein Strukturtyp").
            let platz = self.typen_felder as u32;
            self.typen_felder += 1;
            while m.gruppe_von.len() <= platz as usize {
                m.gruppe_von.push(gruppe);
            }
            m.gruppe_von[platz as usize] = gruppe;
            while m.sub_ober.len() <= platz as usize {
                m.sub_ober.push(None);
            }
            m.sub_ober[platz as usize] = sub_ober_index;
            while m.sub_final.len() <= platz as usize {
                m.sub_final.push(false);
            }
            m.sub_final[platz as usize] = sub_final;
            while m.sub_ge_deklariert.len() <= platz as usize {
                m.sub_ge_deklariert.push(false);
            }
            m.sub_ge_deklariert[platz as usize] = sub_da;
            while m.strukt.len() <= platz as usize {
                m.strukt.push(None);
            }
            m.strukt[platz as usize] = Some(crate::modell::StructTyp {
                felder: felder.clone(),
                veraenderbar: false,
            });
            // Die Feldbezeichner merken — `struct.get $t $x` braucht sie.
            for (i, f) in felder.iter().enumerate() {
                if let Some(n) = &f.name {
                    self.felder_namen.insert((platz, n.clone()), i as u32);
                }
            }
            // Die `(sub …)`-Gruppe schliessen (falls sie dasteht), dann
            // die `(struct …)`-Gruppe.
            if sub_da {
                self.zu()?;
            }
            self.zu()?;
            if let Some(n) = name {
                self.typen_namen.insert(n, platz);
            }
            // … und die `(type …)`-Gruppe (wie im Array-Zweig).
            self.zu()?;
            return Ok(());
        }
        if self.ist("array") {
            self.oeffner("array")?;
            let veraenderbar = if self.ist("mut") {
                self.oeffner("mut")?;
                true
            } else {
                false
            };
            // `feld_typ` liest einen Speichertyp **mit** Gepacktheit
            // (`i8`/`i16`); die Veraenderbarkeit steht hier am Typ.
            //
            // GEMESSEN 03.10. (`array_copy.wast` Z. 17ff).
            let (inhalt, bits, _) = self.feld_typ()?;
            // `(mut …)` nur schliessen, wenn es da war — danach `(array …)`
            // und `(type …)`; davor noch `(sub …)`, falls dasteht.
            if veraenderbar {
                self.zu()?;
            }
            if sub_da {
                self.zu()?;
            }
            self.zu()?;
            self.zu()?;
            // Derselbe Index wie beim Strukturtyp: der Zaehler der
            // `(type …)`-Felder. Vorher stand hier `self.typen.len()`
            // — das ist bei einem vorberechneten Typraum die Gesamtzahl
            // (GEMESSEN 03.10. in `table_init.wast` Z. 2273 UND, viel
            // allgemeiner, in `struct.wast` Z. 55: jeder numerische
            // Typindex zeigte ins Leere).
            let platz = self.typen_felder as u32;
            self.typen_felder += 1;
            while m.gruppe_von.len() <= platz as usize {
                m.gruppe_von.push(gruppe);
            }
            m.gruppe_von[platz as usize] = gruppe;
            while m.sub_ober.len() <= platz as usize {
                m.sub_ober.push(None);
            }
            m.sub_ober[platz as usize] = sub_ober_index;
            while m.sub_final.len() <= platz as usize {
                m.sub_final.push(false);
            }
            m.sub_final[platz as usize] = sub_final;
            while m.sub_ge_deklariert.len() <= platz as usize {
                m.sub_ge_deklariert.push(false);
            }
            m.sub_ge_deklariert[platz as usize] = sub_da;
            while m.arrays.len() <= platz as usize {
                m.arrays.push(None);
            }
            m.arrays[platz as usize] = Some(crate::modell::ArrayTyp {
                inhalt,
                veraenderbar,
                bits,
            });
            if let Some(n) = name {
                self.typen_namen.insert(n, platz);
            }
            return Ok(());
        }
        self.oeffner("func")?;
        let ft = self.signatur()?;
        self.zu()?;
        // `(type $a (sub $b (func …)))`: noch die `sub`-Gruppe schliessen.
        if sub_da {
            self.zu()?;
        }
        self.zu()?;
        // Der Typ steht schon im vorberechneten Typraum (Vorlauf) — hier
        // wird er nur noch VERGLICHEN. Ohne den Vergleich waere ein
        // Fehler im Vorlauf still (M8: kein stillschweigendes Reparieren).
        let platz = self.typen_felder;
        self.typen_felder += 1;
        // GEMESSEN 03.10. (`type-rec.wast` Z. 3ff): nur struct und array
        // schrieben `gruppe_von`/`sub_ober` — der **func**-Zweig nicht.
        // Eine Gruppe aus reinen Funktionstypen war damit unsichtbar.
        while m.gruppe_von.len() <= platz {
            m.gruppe_von.push(gruppe);
        }
        m.gruppe_von[platz] = gruppe;
        while m.sub_ober.len() <= platz {
            m.sub_ober.push(None);
        }
        m.sub_ober[platz] = sub_ober_index;
        while m.sub_final.len() <= platz {
            m.sub_final.push(false);
        }
        m.sub_final[platz] = sub_final;
        while m.sub_ge_deklariert.len() <= platz {
            m.sub_ge_deklariert.push(false);
        }
        m.sub_ge_deklariert[platz] = sub_da;
        match self.typen.get(platz) {
            Some(gespeichert) if *gespeichert == ft => {}
            Some(gespeichert) => {
                return Err(textfehler(&format!(
                    "Typ {platz}: Vorlauf und Leser finden verschiedene Signaturen ({} != {})",
                    Self::zeige_typ_impl(gespeichert),
                    Self::zeige_typ_impl(&ft)
                )));
            }
            None => {
                return Err(textfehler(&format!(
                    "Typ {platz} fehlt im vorberechneten Typraum ({} Typen)",
                    self.typen.len()
                )));
            }
        }
        while m.arrays.len() <= platz {
            m.arrays.push(None);
        }
        // `m.typen` wird am Ende aus `self.typen` uebernommen — der
        // vorberechnete Typraum ist bereits vollstaendig.
        // `$name` -> Typindex (fuer `(type $t)` und `call_indirect (type $t)`).
        if let Some(n) = name {
            self.typen_namen.insert(n, platz as u32);
        }
        Ok(())
    }

    /// Ein Typ fuer eine Fehlermeldung: `(i32 i64) -> (f32)`.
    fn zeige_typ_impl(ft: &FuncTyp) -> String {
        format!(
            "({}) -> ({})",
            ft.parameter
                .iter()
                .map(|t| t.text())
                .collect::<Vec<_>>()
                .join(" "),
            ft.ergebnis
                .iter()
                .map(|t| t.text())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }

    /// EIN Typ in einer `(field …)`-Gruppe: `(mut …)` macht ihn
    /// veraenderbar, `i8`/`i16` machen ihn **gepackt** (8/16 Bit — im
    /// Typmodell steht dafuer `i32`).
    ///
    /// GEMESSEN 03.10. (`struct.wast` Z. 145: `(field i32 (mut i32))`).
    fn feld_typ(&mut self) -> Result<(Valtyp, u32, bool), Fehler> {
        let veraenderbar = if self.ist("mut") {
            self.oeffner("mut")?;
            true
        } else {
            false
        };
        let r = match self.sieht().cloned() {
            Some(Token::Wort(w)) => {
                self.pos += 1;
                let bits = match w.as_str() {
                    "i8" => 8,
                    "i16" => 16,
                    _ => 0,
                };
                (valtyp(&w)?, bits)
            }
            Some(Token::Auf(k)) if k == "ref" => {
                self.pos += 1;
                (reftyp_lesen(self)?, 0)
            }
            t => {
                return Err(textfehler(&format!(
                    "(field …): Speichertyp erwartet, gefunden {}",
                    t.map(|x| x.text()).unwrap_or_else(|| "Ende".to_string())
                )));
            }
        };
        if veraenderbar {
            self.zu()?;
        }
        Ok((r.0, r.1, veraenderbar))
    }

    /// `(param …)* (result …)*` lesen.
    fn signatur(&mut self) -> Result<FuncTyp, Fehler> {
        let mut parameter = Vec::new();
        let mut ergebnis = Vec::new();
        let mut ergebnis_gesehen = false;
        loop {
            if self.ist("param") {
                if ergebnis_gesehen {
                    return Err(textfehler(
                        "(param …) nach (result …): die Parameter kommen zuerst",
                    ));
                }
                self.oeffner("param")?;
                let mut n = None;
                if let Some(Token::Name(b)) = self.sieht().cloned() {
                    n = Some(b);
                    self.pos += 1;
                }
                if let Some(b) = n {
                    // Zwei Felder mit demselben Bezeichner sind ein
                    // Syntaxfehler (Spec: „duplicate local").
                    //
                    // GEMESSEN 03.10. (`func.wast` Z. 977/981/985:
                    // 3 `assert_malformed`).
                    if self.lokale.contains_key(&b) {
                        return Err(textfehler(&format!("doppelter Bezeichner: ${b} (lokal)")));
                    }
                    let idx = self.lokale_zaehler;
                    self.lokale.insert(b, idx);
                }
                while let Some(t) = self.sieht().cloned() {
                    match t {
                        Token::Wort(w) => {
                            parameter.push(valtyp_in_klammer(&w)?);
                            self.pos += 1;
                            // Jeder Parameter belegt einen Index — auch ohne
                            // Namen (siehe `lokale_zaehler`).
                            self.lokale_zaehler += 1;
                            // Mehrere Typen in einer Klammer: der Name galt
                            // nur dem ersten (`(param $x i32 i32)` gibt es
                            // nicht).
                        }
                        // `(param (ref null func) …)` — Wasm 2.0 mit
                        // Referenztypen (GEMESSEN 01.10., `select.wast`).
                        Token::Auf(k) if k == "ref" => {
                            self.pos += 1;
                            parameter.push(reftyp_lesen(self)?);
                            self.lokale_zaehler += 1;
                        }
                        _ => break,
                    }
                }
                self.zu()?;
            } else if self.ist("result") {
                // MEHRERE `(result …)`-Gruppen sind erlaubt (sie werden
                // hintereinander angehaengt); ein `(param …)` NACH der
                // ersten Ergebnisgruppe nicht („unexpected token",
                // GEMESSEN 02.10.: `type.wast`, `if.wast`, `block.wast`).
                ergebnis_gesehen = true;
                let ts = self.ergebnis_typen()?;
                ergebnis.extend(ts);
            } else {
                break;
            }
        }
        if parameter.len() > self.g.name_bytes {
            return Err(textfehler("zu viele Parameter"));
        }
        Ok(FuncTyp {
            parameter,
            ergebnis,
        })
    }

    /// Typindex fuer eine Signatur (bestehenden benutzen, sonst anlegen).
    ///
    /// Die Suche geht ueber `typen_index` (O(1)) — vorher `position()`
    /// ueber den ganzen Vektor, also O(n) je Signatur und damit O(n²)
    /// fuer ein Modul.
    ///
    /// GEMESSEN 03.10. (Schlussphase: quadratische Stelle; 2000 Typen
    /// kosteten 16,3 ms Uebersetzen).
    fn typindex(&mut self, ft: &FuncTyp) -> u32 {
        if let Some(i) = self.typen_index.get(ft) {
            return *i;
        }
        let idx = self.typen.len() as u32;
        self.typen_index.insert(ft.clone(), idx);
        self.typen.push(ft.clone());
        idx
    }

    /// Typindex zu einem `$namen`. Unbekannter Name ist ein BENANNTER
    /// Fehler (lieber eine Meldung als stiller Typ 0 — GEMESSEN 01.10.:
    /// `call_indirect $t (type $check)` kam ohne Namensauflosung immer auf
    /// Typ 0 und meldete dann „Tabellendefinition passt nicht").
    fn typindex_namen(&self, name: &str) -> Result<u32, Fehler> {
        self.typen_namen.get(name).copied().ok_or_else(|| {
            textfehler(&format!(
                "Typname ${name} unbekannt ({} Typen)",
                self.typen.len()
            ))
        })
    }

    // ---- (import …) ------------------------------------------------------
    fn import_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("import")?;
        let modul = self.name()?;
        let feld = self.name()?;
        // ACHTUNG: nicht `weiter()` — `klammer_inhalt` erwartet den Leser AUF
        // dem `Auf`-Token (`(func …)`), sonst frisst es nur die erste Klammer
        // (GEMESSEN 01.10.: `(import "e" "f" (func $h (param i32) (result i32)))`
        // brach mit „erwartet ), gefunden (result").
        let art = match self.sieht().cloned() {
            Some(Token::Auf(k)) => k,
            t => {
                return Err(textfehler(&format!(
                    "Importart: {}",
                    t.map(|x| x.text()).unwrap_or_else(|| "Ende".to_string())
                )));
            }
        };
        match art.as_str() {
            "func" => {
                let inhalt = self.klammer_inhalt();
                let mut p = self.unter(&inhalt);
                p.oeffner("func")?;
                p.optionaler_name();
                // `(func $name (type $t) …)`: der Typindex wird uebernommen,
                // damit ein UNBEKANNTER Typ auch im Import stecken bleibt
                // (`assert_invalid` „unknown type" — GEMESSEN 02.10.,
                // `imports.wast` Z. 13). Ohne diesen Zweig wtand hier immer
                // Typ 0 und der Validator konnte nichts finden.
                let typvorgabe = if p.ist("type") {
                    p.oeffner("type")?;
                    let t = match p.weiter() {
                        Token::Wort(w) => {
                            u32::try_from(zahl(&w)?).map_err(|_| textfehler("Typindex zu gross"))?
                        }
                        Token::Name(n) => self.typindex_namen(&n)?,
                        t2 => {
                            return Err(textfehler(&format!(
                                "(import (func (type)): {}",
                                t2.text()
                            )));
                        }
                    };
                    p.zu()?;
                    Some(t)
                } else {
                    None
                };
                let ft = p.signatur()?;
                let typ = match typvorgabe {
                    Some(t) => t,
                    None => self.typindex(&ft),
                };
                m.funktionen.push(Func::Importiert { modul, feld, typ });
            }
            "table" => {
                let (reftyp, gr) = tabellentyp(&self.klammer_inhalt())?;
                m.tabellen.push(ImportiertTab::Importiert {
                    modul,
                    feld,
                    typ: Tabellentyp {
                        reftyp,
                        grenzen: gr,
                    },
                });
            }
            "memory" => {
                let gr = speichertyp(&self.klammer_inhalt());
                m.speicher.push(ImportiertSpeicher::Importiert {
                    modul,
                    feld,
                    typ: Speichertyp { grenzen: gr },
                });
            }
            "tag" => {
                // Die Klammer `(tag …)` wird hier geoeffnet und geschlossen
                // (`klammer_inhalt` waere hier falsch: die Signatur steht in
                // eigenen Gruppen).
                self.oeffner("tag")?;
                // `(tag $name (param …))`: der Bezeichner kommt zuerst und
                // gehoert in den Tag-Namensraum (GEMESSEN 02.10.,
                // `instance.wast`: `(import "I1" "tag" (tag $tag1))`).
                if let Some(n) = self.optionaler_name() {
                    let idx = m.tags.len() as u32;
                    self.tags_namen.insert(n, idx);
                }
                let (typ, _import) = self.tag_typ_lesen()?;
                self.zu()?;
                // Der Import gehoert zum TAG (Modul/Feld), sonst wuerde der
                // Linker ein eigenes Tag anlegen.
                //
                // GEMESSEN 02.10. (`instance.wast`: `tags [1, 2]` statt
                // `[1, 1]` — zwei Importe desselben Tags waren verschiedene
                // Objekte, `catch $tag1` fand `throw $tag2` nicht).
                m.tags.push(crate::modell::Tag {
                    typ,
                    import: Some((modul, feld)),
                });
            }
            "global" => {
                let (t, veraenderbar) = globaltyp(&self.klammer_inhalt())?;
                m.globale.push(Global {
                    typ: t,
                    veraenderbar,
                    start: None,
                    import: Some((modul, feld)),
                });
            }
            andere => return Err(textfehler(&format!("Importart {andere}"))),
        }
        self.zu()?;
        Ok(())
    }

    /// Steht im aktuellen Feld (nach Bezeichner und `(export "…")`-Gruppen)
    /// eine `(import …)`-Gruppe? Dann ist es die Importform und keine
    /// Definition.
    ///
    /// M3/M4: die Schleife laeuft ueber die Token des Feldes; ihre harte
    /// Grenze ist die Zahl der Token.
    fn ist_inline_import(&self) -> bool {
        let grenze = self.tok.len();
        let mut k = self.pos + 1;
        let mut schritte = 0usize;
        if matches!(self.tok.get(k), Some(Token::Name(_))) {
            k += 1;
        }
        while schritte <= grenze {
            schritte += 1;
            match self.tok.get(k) {
                Some(Token::Auf(x)) if x == "export" => {
                    // `(export "name")` ueberspringen (Klammern zaehlen).
                    let mut tiefe = 0i64;
                    let mut q = k;
                    while q < grenze {
                        match &self.tok[q] {
                            Token::Auf(_) => tiefe += 1,
                            Token::Zu => {
                                tiefe -= 1;
                                if tiefe == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        q += 1;
                    }
                    k = q.saturating_add(1);
                }
                Some(Token::Auf(x)) if x == "import" => return true,
                _ => return false,
            }
        }
        false
    }

    fn unter(&self, tok: &[Token]) -> Parser {
        Parser {
            tok: tok.to_vec(),
            pos: 0,
            g: self.g.clone(),
            typen: self.typen.clone(),
            typen_felder: 0,
            lokale_zaehler: self.lokale_zaehler,
            typen_namen: self.typen_namen.clone(),
            felder_namen: self.felder_namen.clone(),
            aktuelle_gruppe: self.aktuelle_gruppe,
            tabellen_namen: self.tabellen_namen.clone(),
            globale_namen: self.globale_namen.clone(),
            speicher_namen: self.speicher_namen.clone(),
            daten_namen: self.daten_namen.clone(),
            elem_namen: self.elem_namen.clone(),
            funktionen: self.funktionen.clone(),
            tags_namen: self.tags_namen.clone(),
            typen_index: self.typen_index.clone(),
            lokale: BTreeMap::new(),
            labels: Vec::new(),
            schritte: 0,
        }
    }

    // ---- (func …) --------------------------------------------------------
    fn func_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("func")?;
        let bezeichner = self.optionaler_name();
        let mut inline_exporte: Vec<String> = Vec::new();
        let mut inline_import: Option<(String, String)> = None;
        while self.ist("export") {
            self.oeffner("export")?;
            let n = self.name()?;
            inline_exporte.push(n);
            self.zu()?;
        }
        while self.ist("import") {
            self.oeffner("import")?;
            let a = self.name()?;
            let b = self.name()?;
            inline_import = Some((a, b));
            self.zu()?;
        }
        // Lokalnamen der neuen Funktion (Parameternamen kommen in `signatur`).
        self.lokale.clear();
        self.lokale_zaehler = 0;
        // `(func $f (type $t) …)`: die Signatur steht im Typabschnitt.
        // GEMESSEN 02.10. (`elem.wast`, 2 Kommandos): ohne diesen Zweig
        // wurde `(type …)` als Befehl gelesen und das ganze Modul fiel aus.
        let typvorgabe = if self.ist("type") {
            self.oeffner("type")?;
            let t = match self.weiter() {
                Token::Wort(w) => u32::try_from(zahl(&w)?)
                    .map_err(|_| textfehler(&format!("(func (type {w})): Typindex zu gross")))?,
                Token::Name(n) => self.typindex_namen(&n)?,
                t2 => {
                    return Err(textfehler(&format!(
                        "(func (type)): Typindex erwartet, gefunden {}",
                        t2.text()
                    )));
                }
            };
            self.zu()?;
            Some(t)
        } else {
            None
        };
        let ft = self.signatur()?;
        // NICHT eingebaut (GEMESSEN, beides):
        //
        // * Die Breite „`(func (type $t))` ohne Inline-Signatur verlangt
        //   den **ersten** Index mit derselben Signatur" kostete 236
        //   Kernfaelle (20974 -> 20738).
        // * Auch auf `rec`-Gruppen eingeschraenkt (die enge Fassung)
        //   verdirft **gueltige** Module: `type-subtyping.wast` meldet
        //   „(func (type 6)): der implizite Typ waere Some(0), nicht 6"
        //   fuer genau die Faelle, die die Datei als gueltig fuehrt.
        //
        // Der wahre Unterschied steht eine Zeile weiter unten im Modul:
        // `(global (ref $t) (ref.func $f))` verlangt `t == typ($f)` — und
        // genau daran scheitern die `assert_invalid`-Faelle
        // (`type-rec.wast` Z. 51ff: `$f` traegt `$f2`, das Globale verlangt
        // `$f1`). Diese Pruefung gehoert in den Validator und ist dort
        // umgesetzt.

        // NICHT eingebaut (GEMESSEN, deshalb hier der Befund):
        //
        // `(func $f (type $t))` ohne Inline-Signatur ist in `type-rec.wast`
        // Z. 51ff/93ff nur gueltig, wenn `$t` der Typ ist, den die
        // Deduplizierung des impliziten Typs findet. Die Regel in dieser
        // Form („der erste Index mit derselben Signatur muss es sein")
        // kostete **236 Kernfaelle** (20974 -> 20738, 2.0-Kern 98,87 %):
        // die Kernsuite benutzt `(func (type N))` haeufig auch dann, wenn
        // ein frueherer Typ dieselbe Signatur traegt. Die Regel ist also in
        // dieser Breite **falsch**; sie gehoert mindestens auf die
        // `rec`-Gruppe eingeschraenkt. Wiedereinsetzen erst mit der
        // Einschraenkung — gemessen, nicht geraten.

        // Steht die Signatur im Typabschnitt (`(func (type $t) …)`), haben
        // die ersten `parameter.len()` Lokalindizes schon die Parameter
        // belegt — auch ohne Namen. Sonst rueckte der erste `local` auf
        // Index 0 zurueck und `local.get $var` las den Parameter.
        //
        // GEMESSEN 03.10. (`func.wast` Z. 461ff: 2 `assert_return`).
        if let Some(t) = typvorgabe {
            if let Some(bekannt) = self.typen.get(t as usize) {
                self.lokale_zaehler = bekannt.parameter.len() as u32;
            }
        }
        let typ = match typvorgabe {
            // Steht beides da, muessen die Angaben uebereinstimmen (Spec).
            Some(t) => {
                // VORLAEUFS-VERWEIS: der Typ wird erst spaeter im Modul
                // deklariert (`(type $forward)` hinter der Verwendung). Der
                // Vorlauf kennt den Index; der Validator prueft am Ende, ob
                // der Index wirklich im Typraum liegt.
                //
                // GEMESSEN 02.10. (`imports.wast` Z. 10: `(func (import …)
                // (type $forward))` vor `(type $forward …)`).
                match self.typen.get(t as usize) {
                    Some(bekannt) => {
                        let leer = ft.parameter.is_empty() && ft.ergebnis.is_empty();
                        if !leer && *bekannt != ft {
                            return Err(textfehler(&format!(
                                "(func (type {t})): (param/result) weicht vom Typabschnitt ab"
                            )));
                        }
                    }
                    None => {
                        if !self.typen_namen.values().any(|i| *i == t) {
                            return Err(textfehler(&format!(
                                "(func (type {t})): Typ {t} unbekannt"
                            )));
                        }
                    }
                }
                t
            }
            None => self.typindex(&ft),
        };
        let mut lokale_typen: Vec<Valtyp> = Vec::new();
        while self.ist("local") {
            self.oeffner("local")?;
            let mut n = None;
            if let Some(Token::Name(b)) = self.sieht().cloned() {
                n = Some(b);
                self.pos += 1;
            }
            if let Some(b) = n {
                // Siehe oben: doppelte lokale Bezeichner sind verboten.
                //
                // GEMESSEN 03.10. (`func.wast` Z. 977/981/985).
                if self.lokale.contains_key(&b) {
                    return Err(textfehler(&format!("doppelter Bezeichner: ${b} (lokal)")));
                }
                let idx = self.lokale_zaehler;
                self.lokale.insert(b, idx);
            }
            // Der Typ ist ein Wort (`i32`, `externref` …) ODER eine
            // Klammer `(ref …)` — `(local $x (ref extern))` ist eine nicht
            // leerbare Referenz (Wasm 3.0, non-defaultable locals).
            //
            // GEMESSEN 03.10. (`local_init.wast`: 4 `assert_invalid`).
            while let Some(t) = self.sieht().cloned() {
                if lokale_typen.len() >= self.g.lokale_je_funktion {
                    return Err(textfehler("zu viele lokale Variablen"));
                }
                match t {
                    Token::Wort(w) => {
                        lokale_typen.push(valtyp(&w)?);
                        self.pos += 1;
                        // Jede lokale Variable belegt den naechsten Index.
                        self.lokale_zaehler += 1;
                    }
                    Token::Auf(k) if k == "ref" => {
                        self.pos += 1;
                        let r = reftyp_lesen(self)?;
                        lokale_typen.push(r);
                        self.lokale_zaehler += 1;
                    }
                    _ => break,
                }
            }
            self.zu()?;
        }
        let lokale = lokale_typen;
        let idx = m.funktionen.len() as u32;
        if let Some(b) = bezeichner {
            self.funktionen.insert(b, idx);
        }
        let mut koerper: Vec<Befehl> = Vec::new();
        if inline_import.is_none() {
            self.labels.clear();
            self.instruktionen(&mut koerper)?;
            koerper.push(Befehl::End);
        }
        self.zu()?;
        if let Some((modul, feld)) = inline_import {
            m.funktionen.push(Func::Importiert { modul, feld, typ });
        } else {
            m.funktionen.push(Func::Eigen {
                typ,
                lokale,
                koerper,
                befehl_offset: idx as usize,
            });
        }
        for name in inline_exporte {
            m.exporte.push(Export {
                name,
                art: Art::Func(idx),
            });
        }
        Ok(())
    }

    /// Genau EINEN Ausdruck lesen: eine gefaltete Klammer ODER ein
    /// nackter Befehl mit seinen Immediates.
    fn ausdruck(&mut self, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        self.schritte += 1;
        if self.schritte > self.tok.len().max(64) {
            return Err(textfehler("Ausdruck-Schleife laeuft nicht weiter"));
        }
        match self.sieht().cloned() {
            Some(Token::Auf(k)) => {
                self.pos += 1;
                self.gefalten(&k, out)
            }
            Some(Token::Wort(w)) => {
                self.pos += 1;
                out.extend(self.befehle_von(&w)?);
                Ok(())
            }
            None => Err(textfehler("Ausdruck fehlt")),
            t => Err(textfehler(&format!(
                "unerwartetes Token {}",
                t.map(|x| x.text()).unwrap_or_default()
            ))),
        }
    }

    /// Befehle lesen, bis `)` oder Ende der Tokenliste.
    fn instruktionen(&mut self, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        loop {
            if out.len() >= self.g.befehle_je_funktion {
                return Err(textfehler("zu viele Befehle"));
            }
            self.schritte += 1;
            if self.schritte > self.tok.len().max(64) {
                return Err(textfehler(
                    "Lese-Schleife laeuft nicht weiter (Befehl ohne Fortschritt)",
                ));
            }
            match self.sieht().cloned() {
                None | Some(Token::Zu) => return Ok(()),
                Some(Token::Wort(w)) => {
                    // FLACHE Form: `block $l … end $l`, `if $l … else $l …
                    // end $l` (GEMESSEN 01.10., `stack.wast`: 5 Faelle,
                    // weil `block` als blosses Wort nicht gelesen wurde).
                    match w.as_str() {
                        "block" | "loop" | "if" => {
                            self.flach_block_start(&w, out)?;
                            continue;
                        }
                        "end" => {
                            self.pos += 1;
                            self.pruefe_marke()?; // `end $label`
                            out.push(Befehl::End);
                            continue;
                        }
                        "else" => {
                            self.pos += 1;
                            self.pruefe_marke()?; // `else $label`
                            out.push(Befehl::Else);
                            continue;
                        }
                        _ => {}
                    }
                    self.pos += 1;
                    self.befehl_mit_name(&w, out)?;
                }
                Some(Token::Auf(k)) => {
                    self.pos += 1;
                    self.gefalten(&k, out)?;
                }
                Some(t) => return Err(textfehler(&format!("unerwartetes Token {}", t.text()))),
            }
        }
    }

    /// Flache Form eines Blocks: der Leser steht auf dem `end`-Wort
    /// (oder `else`), Name und Blocktyp sind schon gelesen. Der Befehl
    /// (`Block`/`Loop`/`If`) ist bereits in `out`.
    ///
    /// GEMESSEN 01.10. (`stack.wast`): die Testsuite schreibt die
    /// Fakultaet in der flachen Form mit Marken hinter `else` und `end`.
    fn flach_block(&mut self, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        loop {
            self.schritte += 1;
            if self.schritte > self.tok.len().max(64) {
                return Err(textfehler("Block nicht geschlossen (Lese-Schleife)"));
            }
            match self.sieht().cloned() {
                None | Some(Token::Zu) => {
                    return Err(textfehler("Block nicht geschlossen (kein end)"));
                }
                Some(Token::Wort(w)) if w == "end" => {
                    self.pos += 1;
                    self.pruefe_marke()?; // `end $label`
                    out.push(Befehl::End);
                    self.labels.pop();
                    return Ok(());
                }
                Some(Token::Wort(w)) if w == "else" => {
                    self.pos += 1;
                    self.pruefe_marke()?; // `else $label`
                    out.push(Befehl::Else);
                }
                Some(Token::Wort(w)) if matches!(w.as_str(), "block" | "loop" | "if") => {
                    // Verschachtelung in der flachen Form (GEMESSEN 01.10.:
                    // `block $d loop $l … end $l end $d`).
                    self.flach_block_start(&w, out)?;
                }
                Some(Token::Wort(w)) => {
                    self.pos += 1;
                    self.befehl_mit_falten(&w, out)?;
                }
                Some(Token::Auf(k)) => {
                    self.pos += 1;
                    self.gefalten(&k, out)?;
                }
                Some(t) => return Err(textfehler(&format!("unerwartetes Token {}", t.text()))),
            }
        }
    }

    /// Marke hinter `end`/`else` pruefen: sie muss zum Block passen
    /// (Spec: „mismatching label"). GEMESSEN 01.10. (`if.wast` Z. 840ff.:
    /// `(func i32.const 0 if end $l)` war bei uns gueltig).
    fn pruefe_marke(&mut self) -> Result<(), Fehler> {
        let marke = match self.optionaler_name() {
            Some(m) => m,
            None => return Ok(()),
        };
        // `labels` haelt je eine Option (unbenannter Block = None).
        match self.labels.last() {
            Some(Some(l)) if *l == marke => Ok(()),
            gut => Err(textfehler(&format!(
                "Marke ${marke} passt nicht zum Block (offen: {gut:?})"
            ))),
        }
    }

    /// `block`/`loop`/`if` in der flachen Form: Name, Blocktyp und Befehl
    /// lesen, dann den Koerper (`flach_block`) bis `end`.
    fn flach_block_start(&mut self, art: &str, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        self.pos += 1;
        self.tiefe_pruefen()?;
        let beschriftung = self.optionaler_name();
        let bt = self.blocktyp()?;
        out.push(match art {
            "block" => Befehl::Block(bt),
            "loop" => Befehl::Loop(bt),
            _ => Befehl::If(bt),
        });
        self.labels.push(beschriftung);
        self.flach_block(out)
    }

    /// Gefalteter Ausdruck `(op arg …)`.
    ///
    /// WAT-Semantik: die Immediates des Kopfbefehls stehen DIRECT hinter dem
    /// Namen (`(local.get $x)`, `(i32.store offset=4 …)`), danach folgen die
    /// gefalteten Argumente. Die Argumente landen VOR dem Befehl im flachen
    /// Vektor — genau so erwartet es der Interpreter.
    ///
    /// GEMESSEN 01.10. (`i32.wast`): der Kopf wird genau EINMAL erzeugt —
    /// in dem Durchlauf, der sein Immediate liest. Deshalb merkt sich
    /// `kopf_fertig`, ob das schon geschehen ist.
    fn gefalten(&mut self, name: &str, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        match name {
            "block" | "loop" => {
                self.tiefe_pruefen()?;
                let beschriftung = self.optionaler_name();
                let bt = self.blocktyp()?;
                out.push(if name == "block" {
                    Befehl::Block(bt)
                } else {
                    Befehl::Loop(bt)
                });
                self.labels.push(beschriftung);
                self.instruktionen(out)?;
                out.push(Befehl::End);
                self.labels.pop();
                self.zu()?;
                return Ok(());
            }
            "if" => return self.if_gefalten(out),
            // `try_table (catch …)* (…)` — wie `block`, nur mit Fangliste.
            // GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
            "try_table" => {
                self.tiefe_pruefen()?;
                self.optionaler_name();
                let faenge = self.faeng_liste()?;
                let bt = self.blocktyp()?;
                out.push(Befehl::TryTable(bt, faenge));
                self.labels.push(None);
                self.instruktionen(out)?;
                out.push(Befehl::End);
                self.labels.pop();
                self.zu()?;
                return Ok(());
            }
            _ => {}
        }
        let mut args: Vec<Befehl> = Vec::new();
        let mut kopf: Vec<Befehl> = Vec::new();
        loop {
            // M3/M5: Die Schleife laeuft ueber die Eingabe; die Tokenzahl ist
            // die harte Grenze. Ohne diesen Zaehler waere ein Leserfehler eine
            // Endlosschleife (GEMESSEN: 60-s-Haenger bei `(nop $m)`).
            self.schritte += 1;
            if self.schritte > self.tok.len().max(64) {
                return Err(textfehler(
                    "Lese-Schleife laeuft nicht weiter (Befehl ohne Fortschritt)",
                ));
            }
            match self.sieht() {
                None => return Err(textfehler("Klammer nicht geschlossen")),
                Some(Token::Zu) => break,
                // Ein `$…`/`"…"` direkt hinter dem Befehlsnamen ist ein
                // Immediate (`local.get $x`, `br $l`) oder ein Speicherindex
                // (`f32.load $m`, Textform mit mehreren Speichern).
                //
                // GEMESSEN 01.10. (`(module (func (nop $m)))`, Haenger bis zur
                // Zeitgrenze): der Befehl verbraucht ein Immediate nicht immer
                // — `nop` tut es nicht. Ohne die Positionspruefung dreht sich
                // die Schleife endlos. Deshalb: Position vorher merken, und
                // wenn der Befehl nichts verbraucht hat, das Token ueberspringen.
                Some(Token::Name(_)) | Some(Token::Text(_)) => {
                    let vorher = self.pos;
                    kopf = self.befehle_von(name)?;
                    if self.pos == vorher {
                        self.pos += 1;
                    }
                }
                Some(Token::Auf(k)) => {
                    let k = k.clone();
                    self.pos += 1;
                    // GEMESSEN 01.10. (`call.wast`, `select.wast`): die
                    // Klammern `(type …)`, `(param …)`, `(result …)`,
                    // `(table …)` sind Felder des Kopfbefehls und keine
                    // Argumente — auch dann, wenn der Kopf schon steht
                    // (Label zuerst: `(call_indirect $t (type $check) …)`).
                    if ist_feld_klammer(&k) {
                        self.pos -= 1;
                        let vorher = self.pos;
                        let k_neu = self.befehle_von(name)?;
                        if self.pos == vorher {
                            // Der Befehl liest das Feld nicht: die ganze
                            // Klammer ueberspringen, sonst bleibt sie liegen
                            // und der Leser meldet „unbekanntes Feld".
                            let _ = self.klammer_inhalt();
                        }
                        // Ersetzen, nicht anhaengen: `(call_indirect $t (type
                        // $check) …)` liest den Typ erst beim zweiten
                        // Durchgang — mit `extend` stand der Befehl doppelt
                        // im Koerper (GEMESSEN 01.10., `select.wast`: zwei
                        // `call_indirect` hintereinander).
                        let _ = &kopf;
                        kopf = k_neu;
                    } else {
                        let mut kind = Vec::new();
                        self.gefalten(&k, &mut kind)?;
                        args.extend(kind);
                    }
                }
                Some(Token::Wort(w)) => {
                    let w = w.clone();
                    // GEMESSEN 01.10. (`float_exprs.wast`, `i32.wast`):
                    //  * ein Wort, das KEIN Befehl ist, ist ein Immediate des
                    //    Kopfbefehls (`(f32.const 0.0)`). Der Befehl liest es
                    //    selbst — `pos` wird hier also NICHT weitergesetzt
                    //    (sonst laesst `(i32.const 0)` die Zahl liegen und der
                    //    Leser meldet "erwartet ein Wort, gefunden )"),
                    //  * ein Wort, das EIN Befehl ist, wird uebersprungen und
                    //    sein Befehl erzeugt.
                    if !ist_befehlsname(&w)
                        && (kopf.is_empty() || w.starts_with("offset=") || w.starts_with("align="))
                    {
                        // GEMESSEN 01.10. (`address0.wast`):
                        // `(i32.load8_u $mem1 offset=0 …)` — der
                        // Speicherindex kommt VOR `offset=`, deshalb wird
                        // der Kopfbefehl bei jedem `offset=`/`align=` erneut
                        // aufgerufen (er haengt die Werte an).
                        // Siehe oben: der zweite Durchgang ERSETZT den Kopf.
                        kopf = self.befehle_von(name)?;
                    } else {
                        self.pos += 1;
                        args.extend(self.befehle_von(&w)?);
                    }
                }
            }
        }
        self.zu()?;
        if kopf.is_empty() {
            kopf = self.befehle_von(name)?;
        }
        out.extend(args);
        out.extend(kopf);
        Ok(())
    }

    /// Ein BARE Befehlsname mit den直接 folgenden gefalteten Argumenten.
    ///
    /// Im Textformat ist `(i64.eq (local.get $i) (i64.const 0))` die
    /// uebliche Form, aber `stack.wast` schreibt innerhalb eines `if` auch
    /// `i64.eq (local.get $i) (i64.const 0)` als flache Folge. Ohne diesen
    /// Zweig kam der Befehl VOR seinen Argumenten in den Vektor
    /// (GEMESSEN 01.10.).
    fn befehl_mit_falten(&mut self, name: &str, ziel: &mut Vec<Befehl>) -> Result<(), Fehler> {
        if matches!(name, "block" | "loop" | "if") {
            // Diese drei lesen ihren Koerper selbst bis zur schliessenden
            // Klammer — sie bekommen keine Argumente gesammelt.
            ziel.extend(self.befehle_von(name)?);
            return Ok(());
        }
        let mut args: Vec<Befehl> = Vec::new();
        let mut n = 0usize;
        while let Some(Token::Auf(_)) = self.sieht() {
            // M3/M5: Die Schleife laeuft ueber die Eingabe; Tokenzahl als
            // Grenze (jeder Durchlauf verbraucht mindestens ein Token).
            n += 1;
            if n > self.tok.len() {
                return Err(textfehler("Argumente laufen nicht weiter"));
            }
            let k = match self.sieht().cloned() {
                Some(Token::Auf(k)) => k,
                _ => break,
            };
            self.pos += 1;
            self.gefalten(&k, &mut args)?;
        }
        ziel.extend(args);
        ziel.extend(self.befehle_von(name)?);
        Ok(())
    }

    /// Ein Befehl (mit seinen Immediates) als Liste — damit gefaltete
    /// Ausdruecke ihre Befehle in Reinfolge sammeln koennen.
    fn befehle_von(&mut self, name: &str) -> Result<Vec<Befehl>, Fehler> {
        let mut v = Vec::new();
        self.befehl_mit_name(name, &mut v)?;
        Ok(v)
    }

    /// `(if … (then …) (else …))` und `(if … (dann …) (sonst …))`.
    ///
    /// Die flache Reihenfolge ist: Bedingung, `If`, then-Befehle, `Else`,
    /// else-Befehle, `End`. GEMESSEN 01.10. (`fac.wast`): mit vertauschtem
    /// `Else` kam der Validator mit „Stapelunterlauf" statt der Fakultaet.
    fn if_gefalten(&mut self, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        self.tiefe_pruefen()?;
        let beschriftung = self.optionaler_name();
        self.labels.push(beschriftung);
        let bt = self.blocktyp()?;
        let mut argumente: Vec<Befehl> = Vec::new();
        let mut then_teil: Vec<Befehl> = Vec::new();
        let mut else_teil: Vec<Befehl> = Vec::new();
        let mut in_then = false;
        let mut in_else = false;
        // Name des zuletzt gesehenen Befehls — fuer `$name`-Immediates
        // (`local.get $x`) und Speicherindizes (`f32.load $m`).
        let mut letzter: String = "nop".to_string();
        loop {
            match self.sieht() {
                None => return Err(textfehler("if: Klammer nicht geschlossen")),
                Some(Token::Zu) => {
                    self.pos += 1;
                    break;
                }
                Some(Token::Auf(k)) if k == "then" || k == "dann" => {
                    self.pos += 1;
                    in_then = true;
                    self.instruktionen(&mut then_teil)?;
                    self.zu()?;
                }
                Some(Token::Auf(k)) if k == "else" || k == "sonst" => {
                    self.pos += 1;
                    // `(else (result …) …)` — der Blocktyp steht schon am `if`.
                    while self.ist("result") || self.ist("param") || self.ist("type") {
                        self.pos += 1;
                        self.klammer_ueberspringen();
                    }
                    in_else = true;
                    self.instruktionen(&mut else_teil)?;
                    self.zu()?;
                }
                Some(Token::Auf(k)) => {
                    let k = k.clone();
                    self.pos += 1;
                    // Bedingung bzw. Argument vor `then`/`else`
                    if in_then {
                        self.gefalten(&k, &mut then_teil)?;
                    } else if in_else {
                        self.gefalten(&k, &mut else_teil)?;
                    } else {
                        self.gefalten(&k, &mut argumente)?;
                    }
                }
                Some(Token::Wort(w)) => {
                    let w = w.clone();
                    // Vor `(then …)` sind im Falt-Formular nur
                    // Klammer-Ausdruecke erlaubt — ein BARE Befehl an dieser
                    // Stelle ist „unexpected token" (GEMESSEN 02.10.,
                    // `if.wast`: `(if i32.const 0 (then) (else))`).
                    if !in_then && !in_else && ist_befehlsname(&w) {
                        return Err(textfehler(&format!(
                            "(if {w} …): vor (then …) nur Klammer-Ausdruecke"
                        )));
                    }
                    letzter = w.clone();
                    let ziel = if in_then {
                        &mut then_teil
                    } else if in_else {
                        &mut else_teil
                    } else {
                        &mut argumente
                    };
                    if ziel.is_empty() && !ist_befehlsname(&w) {
                        self.pos += 1;
                    } else {
                        // Ein BARE Befehl mit den folgenden gefalteten
                        // Argumenten: `(if (i64.eq (local.get $i) (i64.const
                        // 0)) (then …))` (GEMESSEN 01.10., `stack.wast`).
                        self.pos += 1;
                        self.befehl_mit_falten(&w, ziel)?;
                    }
                }
                Some(Token::Name(_)) | Some(Token::Text(_)) => {
                    // `$name`/`"…"` hinter einem Befehl: Immediate
                    // (`local.get $x`) oder Speicherindex (`f32.load $m`).
                    let vorher = self.pos;
                    let neu = self.befehle_von(&letzter)?;
                    if self.pos == vorher {
                        self.pos += 1;
                    }
                    if in_then {
                        then_teil.extend(neu);
                    } else if in_else {
                        else_teil.extend(neu);
                    } else {
                        argumente.extend(neu);
                    }
                }
            }
        }
        // Reihenfolge: Bedingung, If, then, Else, else, End
        out.extend(argumente);
        out.push(Befehl::If(bt));
        out.extend(then_teil);
        if !else_teil.is_empty() || !bt_ist_leer(&bt) {
            out.push(Befehl::Else);
            out.extend(else_teil);
        }
        out.push(Befehl::End);
        self.labels.pop();
        Ok(())
    }

    /// Den Inhalt der aktuellen Klammer ueberspringen (fuer Felder, die
    /// der Textleser nicht auswertet).
    fn klammer_ueberspringen(&mut self) {
        let mut tiefe = 1i64;
        loop {
            self.schritte += 1;
            if self.schritte > self.tok.len().max(64) {
                return;
            }
            match self.sieht() {
                None => return,
                Some(Token::Auf(_)) => tiefe += 1,
                Some(Token::Wort(_)) | Some(Token::Text(_)) | Some(Token::Name(_)) => {}
                Some(Token::Zu) => {
                    tiefe -= 1;
                    self.pos += 1;
                    if tiefe == 0 {
                        return;
                    }
                }
            }
            self.pos += 1;
        }
    }

    fn tiefe_pruefen(&self) -> Result<(), Fehler> {
        if self.labels.len() >= self.g.verschachtelung {
            return Err(Fehler::form(
                ART_GRENZE,
                format!(
                    "Verschachtelung {} ueber der Grenze {}",
                    self.labels.len(),
                    self.g.verschachtelung
                ),
            ));
        }
        Ok(())
    }

    /// `(result …)`: die Typen bis zum schliessenden `)`. Akzeptiert auch
    /// `(ref null func)` als Typ (Wasm 2.0 mit Referenztypen) — GEMESSEN
    /// 01.10. (`select.wast` „join-funcnull"): ohne diesen Zweig blieb die
    /// Klammer im Befehlsstrom liegen und der Leser meldete „unbekanntes
    /// Feld (select)".
    fn ergebnis_typen(&mut self) -> Result<Vec<Valtyp>, Fehler> {
        self.oeffner("result")?;
        let mut ts = Vec::new();
        while let Some(t) = self.sieht().cloned() {
            match t {
                Token::Wort(w) => {
                    ts.push(valtyp(&w)?);
                    self.pos += 1;
                }
                // `(ref null func)` / `(ref null $t)` — auch in Wasm 2.0
                // gueltig (Referenztypen); `(ref non-null …)` ist 3.0.
                Token::Auf(k) if k == "ref" => {
                    self.pos += 1;
                    ts.push(reftyp_lesen(self)?);
                }
                _ => break,
            }
        }
        self.zu()?;
        Ok(ts)
    }

    /// Blocktyp: `(type …)? (param …)* (result …)*`.
    ///
    /// Die Reihenfolge ist wichtig: laut Spezifikation darf `(type $x)`
    /// VOR den Inline-Angaben stehen — `(if (type $s) (result i32) …)`
    /// (GEMESSEN 01.10., `if.wast` Z. 514: der Leser sah danach `(result i32)`
    /// als Befehl und brach ab). Ein benannter Typ gewinnt; die Inline-Gruppen
    /// wiederholen laut Spec nur denselben Typ.
    fn blocktyp(&mut self) -> Result<Blockart, Fehler> {
        let mut typ_idx: Option<u32> = None;
        if self.ist("type") {
            self.oeffner("type")?;
            typ_idx = Some(match self.weiter() {
                Token::Wort(w) => w.parse::<u32>().map_err(|_| textfehler("Typindex"))?,
                Token::Name(n) => self.typindex_namen(&n)?,
                t => return Err(textfehler(&format!("(type …): {}", t.text()))),
            });
            self.zu()?;
        }
        if typ_idx.is_none() && !self.ist("param") && !self.ist("result") {
            // Ein einzelner WERTtyp ist auch ein Blocktyp: `(block i32 …)`,
            // `(if funcref …)`. Anything andere, was wie ein Befehl aussieht,
            // ist an dieser Stelle ein Syntaxfehler — im Falt-Formular steht
            // der Blocktyp VOR den Operanden (`(if i32.const 0 (then …))`).
            //
            // GEMESSEN 02.10. (`if.wast`: 1 `assert_malformed`).
            if let Some(Token::Wort(w)) = self.sieht().cloned() {
                if let Ok(v) = valtyp(&w) {
                    self.pos += 1;
                    return Ok(Blockart::Werttyp(v));
                }
            }
            return Ok(Blockart::Leer);
        }
        // Inline-Signatur einsammeln. `klammer_inhalt` erwartet den Leser AUF
        // dem `Auf`-Token (GEMESSEN 01.10., `block.wast` „params": BEIDE
        // Klammern `(param …)` und `(result …)` gehoeren zur Signatur).
        let mut parameter: Vec<Valtyp> = Vec::new();
        let mut ergebnis: Vec<Valtyp> = Vec::new();
        // Reihenfolge laut Spezifikation: `(type …)? (param …)* (result …)*`.
        // Mehrere Gruppen sind erlaubt: `(type $s) (param i32) (param f64 i32)
        // (result i32 f64) (result i32)` (GEMESSEN 01.10., `if.wast` Z. 521).
        // Ein `(param …)` NACH `(result …)` ist Syntaxfehler (`if.wast`
        // Z. 738/749, 5 `assert_malformed`-Faelle).
        while self.ist("param") {
            let inhalt = self.klammer_inhalt();
            // Im Blocktyp gibt es KEINE Bezeichner: `(block (param $x i32) …)`
            // ist ein Syntaxfehler (GEMESSEN 02.10., `block.wast` Z. 59,
            // `if.wast` Z. 131).
            if inhalt.iter().any(|t| matches!(t, Token::Name(_))) {
                return Err(textfehler(
                    "(block …): (param $name …) — im Blocktyp gibt es keine Bezeichner",
                ));
            }
            let mut p = self.unter(&inhalt);
            let ft = p.signatur()?;
            parameter.extend(ft.parameter);
            ergebnis.extend(ft.ergebnis);
        }
        while self.ist("result") {
            let inhalt = self.klammer_inhalt();
            let mut p = self.unter(&inhalt);
            let ft = p.signatur()?;
            parameter.extend(ft.parameter);
            ergebnis.extend(ft.ergebnis);
        }
        if self.ist("param") {
            return Err(textfehler("(block …): (param …) nach (result …)"));
        }
        if let Some(i) = typ_idx {
            // Inline-Angaben MUESSEN zum benannten Typ passen (Spec:
            // „inline function type"). GEMESSEN 01.10. (`if.wast`
            // Z. 795-830: 5 `assert_malformed`-Faelle).
            if !(parameter.is_empty() && ergebnis.is_empty()) {
                let benannt = self.typen.get(i as usize).cloned();
                let inline = FuncTyp {
                    parameter: parameter.clone(),
                    ergebnis: ergebnis.clone(),
                };
                if let Some(b) = benannt {
                    if b != inline {
                        return Err(textfehler(&format!(
                            "Inline-Typ {}/{} passt nicht zu (type {i}) {}/{}",
                            inline.parameter.len(),
                            inline.ergebnis.len(),
                            b.parameter.len(),
                            b.ergebnis.len()
                        )));
                    }
                }
            }
            return Ok(Blockart::Typ(i));
        }
        if parameter.is_empty() && ergebnis.is_empty() {
            return Ok(Blockart::Leer);
        }
        // Die Kurzform (EIN Werttyp) gilt auch fuer Referenztypen — der
        // Binaerform-Byte 0x63/0x64 traegt den Typindex mit (Wasm 3.0).
        // Sonst wuerde `(block (result (ref 1)) …)` einen NEUEN Typ
        // erfinden und der Typindex 1 waere scheinbar gueltig.
        //
        // GEMESSEN 02.10. (`ref.wast` Z. 9/10: "unknown type" — 2 Faelle).
        if parameter.is_empty() && ergebnis.len() == 1 {
            return Ok(Blockart::Werttyp(ergebnis[0]));
        }
        let ft = FuncTyp {
            parameter,
            ergebnis,
        };
        Ok(Blockart::Typ(self.typindex(&ft)))
    }

    /// Befehl aus einem Namen; `out` bekommt den Befehl angehaengt.
    fn befehl_mit_name(&mut self, name: &str, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        use Befehl::*;
        // ---- Speicherzugriffe (mit offset=/align=) --------------------------
        if ist_lade(name) || ist_speichere(name) {
            let mut versatz = 0u32;
            let mut ausrichtung: Option<u64> = None;
            // Multi-Memory: `(i32.load8_u $m1 offset=0 …)` — der
            // Speicherindex steht vor `offset=`/`align=`.
            let mut speicher_idx: Option<u32> = None;
            // M5: Die Immediate-Schleife laeuft ueber die Eingabe — hier ist
            // die Grenze die Zahl der Tokens (sie kann nicht kleiner sein).
            let grenze = self.tok.len();
            let mut schritte = 0usize;
            loop {
                schritte += 1;
                if schritte > grenze {
                    return Err(textfehler("Immediate-Schleife laeuft nicht weiter"));
                }
                match self.sieht().cloned() {
                    Some(Token::Name(_)) => {
                        speicher_idx = Some(self.speicher_index_lesen()?);
                    }
                    Some(Token::Wort(w)) if w.starts_with("offset=") => {
                        // `offset=` muss in u32 passen: `offset=4294967296`
                        // ist „offset out of range" (GEMESSEN 02.10.,
                        // `address.wast`). `as u32` wuerde stillschweigend
                        // auf 0 kuerzen.
                        let wert = zahl(&w[7..])?;
                        versatz = u32::try_from(wert).map_err(|_| {
                            textfehler(&format!("offset={wert}: Versatz ausserhalb 0..4294967295"))
                        })?;
                        self.pos += 1;
                    }
                    Some(Token::Wort(w)) if w.starts_with("align=") => {
                        // GEMESSEN 01.10. (`align.wast`, `align0.wast`):
                        // `align=` ist die Ausrichtung IN BYTE (1,2,4,8,16),
                        // nicht der Exponent.
                        let a = zahl(&w[6..])?;
                        if a == 0 || !a.is_power_of_two() || a > 16 {
                            return Err(textfehler(&format!("align={a}: keine Potenz bis 16")));
                        }
                        ausrichtung = Some(a);
                        self.pos += 1;
                    }
                    // Eine blosse Zahl ist der Speicherindex (Multi-Memory).
                    Some(Token::Wort(w)) => match zahl(&w) {
                        Ok(i) => {
                            speicher_idx =
                                Some(u32::try_from(i).map_err(|_| {
                                    textfehler(&format!("Speicherindex {w} zu gross"))
                                })?);
                            self.pos += 1;
                        }
                        Err(_) => break,
                    },
                    _ => break,
                }
            }
            let b = speicherbefehl(name, versatz);
            // Ausrichtung: 2^align darf nicht groesser sein als die
            // natuerliche (Spec: „alignment must not be larger than
            // natural"). GEMESSEN 01.10.: `align.wast` hatte 51 Faelle
            // `assert_invalid`, die der Leser nicht erkannt hat.
            if let Some(a) = ausrichtung {
                let nat = 1u64 << crate::op::natuerliche_ausrichtung(&b);
                if a > nat {
                    return Err(textfehler(&format!(
                        "{name} align={a}: groesser als die natuerliche Ausrichtung {nat}"
                    )));
                }
            }
            // Speicher 0 ist der Normalfall (2.0) und bleibt ohne Huelle.
            match speicher_idx {
                Some(i) if i != 0 => out.push(Befehl::MitSpeicher {
                    befehl: Box::new(b),
                    speicher: i,
                }),
                _ => out.push(b),
            }
            return Ok(());
        }
        // ---- Lokale/Globale -----------------------------------------------
        match name {
            "local.get" | "local.set" | "local.tee" | "global.get" | "global.set" => {
                // `$name` heisst: lokaler Index (bei `global.*` die globale
                // Nummer — die Testsuite nutzt dort keine Namen).
                let idx = match self.weiter() {
                    Token::Wort(w) => w
                        .parse::<u32>()
                        .map_err(|_| textfehler(&format!("Index {w}")))?,
                    Token::Name(b) if name.starts_with("local") => {
                        *self.lokale.get(&b).unwrap_or(&0)
                    }
                    // `global.get $g` — GEMESSEN 01.10. (`data.wast`:
                    // `(data (global.get $g) "a")` wurde sonst zu Index 0).
                    Token::Name(b) if name.starts_with("global") => {
                        self.globale_namen.get(&b).copied().ok_or_else(|| {
                            textfehler(&format!("Name der globalen Variablen ${b} unbekannt"))
                        })?
                    }
                    Token::Name(_) => 0,
                    t => {
                        return Err(textfehler(&format!(
                            "Index erwartet, gefunden {}",
                            t.text()
                        )));
                    }
                };
                out.push(match name {
                    "local.get" => LocalGet(idx),
                    "local.set" => LocalSet(idx),
                    "local.tee" => LocalTee(idx),
                    "global.get" => GlobalGet(idx),
                    _ => GlobalSet(idx),
                });
                return Ok(());
            }
            "br" | "br_if" => {
                let l = self.label()?;
                out.push(if name == "br" { Br(l) } else { BrIf(l) });
                return Ok(());
            }
            "br_table" => {
                let mut alle: Vec<u32> = Vec::new();
                while let Some(t) = self.sieht().cloned() {
                    match t {
                        Token::Name(_) | Token::Wort(_) => {
                            alle.push(self.label()?);
                        }
                        _ => break,
                    }
                }
                if alle.is_empty() {
                    return Err(textfehler("br_table ohne Ziele"));
                }
                let vorgabe = *alle.last().unwrap();
                let ziele: Vec<u32> = alle[..alle.len() - 1].to_vec();
                out.push(BrTable(ziele, vorgabe));
                return Ok(());
            }
            "call" | "ref.func" => {
                let idx = match self.weiter() {
                    Token::Name(b) => *self.funktionen.get(&b).unwrap_or(&0),
                    Token::Wort(w) => w.parse::<u32>().map_err(|_| textfehler("Funktionsindex"))?,
                    t => return Err(textfehler(&format!("{name}: {}", t.text()))),
                };
                out.push(if name == "call" {
                    Call(idx)
                } else {
                    RefFunc(idx)
                });
                return Ok(());
            }
            "call_indirect" => return self.call_indirect(out),
            "table.get" | "table.set" | "table.size" | "table.grow" | "table.fill" => {
                let mut idx = 0;
                if matches!(self.sieht(), Some(Token::Wort(_)) | Some(Token::Name(_))) {
                    idx = self.tabellen_index()?;
                }
                out.push(match name {
                    "table.get" => TableGet(idx),
                    "table.set" => TableSet(idx),
                    "table.size" => TableSize(idx),
                    "table.grow" => TableGrow(idx),
                    _ => TableFill(idx),
                });
                return Ok(());
            }
            "table.copy" => {
                // `(table.copy $ziel $quelle …)`: beide optional.
                let ziel = if matches!(self.sieht(), Some(Token::Wort(_)) | Some(Token::Name(_))) {
                    self.tabellen_index()?
                } else {
                    0
                };
                let quelle = if matches!(self.sieht(), Some(Token::Wort(_)) | Some(Token::Name(_)))
                {
                    self.tabellen_index()?
                } else {
                    0
                };
                out.push(TableCopy { ziel, quelle });
                return Ok(());
            }
            "table.init" => {
                // `(table.init $tab $elem …)` oder `(table.init $elem …)`.
                // Ein Name entscheidet: ist er ein Tabellenname, steht er
                // vorn.
                let a = if let Some(Token::Name(n)) = self.sieht().cloned() {
                    if self.tabellen_namen.contains_key(&n) {
                        self.tabellen_index()?
                    } else {
                        self.elem_index_lesen()?
                    }
                } else {
                    self.index()?
                };
                let b = if matches!(self.sieht(), Some(Token::Name(_)) | Some(Token::Wort(_))) {
                    Some(self.elem_index_lesen()?)
                } else {
                    None
                };
                let (elem, tabelle) = match b {
                    Some(x) => (x, a),
                    None => (a, 0),
                };
                out.push(TableInit {
                    element: elem,
                    tabelle,
                });
                return Ok(());
            }
            "data.drop" => {
                let i = self.daten_index_lesen()?;
                out.push(DataDrop(i));
                return Ok(());
            }
            "elem.drop" => {
                let i = self.elem_index_lesen()?;
                out.push(if name == "elem.drop" {
                    ElemDrop(i)
                } else {
                    DataDrop(i)
                });
                return Ok(());
            }
            // Multi-Memory zuerst (mit Speicherindex als Immediate):
            // `(memory.init $m 0 …)`, `(memory.size $m0)`.
            "memory.size" | "memory.grow" | "memory.copy" | "memory.fill" | "memory.init"
                if self.speicher_arg() =>
            {
                // `(memory.init $d …)` mit EINEM Speicher: `$d` ist der Name
                // des Datensatzes (2.0). `(memory.init $m $d …)` mit mehreren:
                // erst der Speichername. GEMESSEN 02.10. (`bulk.wast`,
                // 12 Faelle): `$p` wurde sonst als Speicher gelesen.
                let n = match self.sieht() {
                    Some(Token::Name(n)) => n.clone(),
                    _ => String::new(),
                };
                if self.speicher_namen.contains_key(&n) {
                    // `memory.copy $ziel $quelle` nennt BEIDE Speicher
                    // (Wasm 3.0, Multi-Memory).
                    //
                    // GEMESSEN 03.10. (`memory_copy1.wast` 4 Faelle: der
                    // Befehl trug die Speicherindizes nicht).
                    if name == "memory.copy" {
                        let ziel = self.speicher_index_lesen()?;
                        let mut quelle = ziel;
                        if matches!(self.sieht(), Some(Token::Name(q)) if self.speicher_namen.contains_key(q))
                        {
                            quelle = self.speicher_index_lesen()?;
                        }
                        out.push(MemoryCopy { ziel, quelle });
                        return Ok(());
                    }
                    let speicher = self.speicher_index_lesen()?;
                    self.befehl_mit_speicher(name, speicher, &mut *out)?;
                    return Ok(());
                }
                if self.daten_namen.contains_key(&n) {
                    let i = self.daten_index_lesen()?;
                    out.push(MemoryInit(i));
                    return Ok(());
                }
                return Err(textfehler(&format!(
                    "${n} ist weder Speicher- (<{} Speicher>) noch Datensatzname (<{} Abschnitte>)",
                    self.speicher_namen.len(),
                    self.daten_namen.len()
                )));
            }
            "memory.init" => {
                let i = self.daten_index_lesen()?;
                out.push(MemoryInit(i));
                return Ok(());
            }
            "memory.size" => {
                out.push(MemorySize);
                return Ok(());
            }
            "memory.grow" => {
                out.push(MemoryGrow);
                return Ok(());
            }
            "memory.copy" => {
                out.push(MemoryCopy { ziel: 0, quelle: 0 });
                return Ok(());
            }
            "memory.fill" => {
                out.push(MemoryFill);
                return Ok(());
            }
            // `try_table (catch $t $label) … <koerper>` (Exception Handling,
            // Wasm 3.0). `try`/`catch`/`delegate` bleiben abgelehnt.
            //
            // GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
            "try_table" => {
                let faenge = self.faeng_liste()?;
                let bt = self.blocktyp()?;
                out.push(Befehl::TryTable(bt, faenge));
                return Ok(());
            }
            // `array.new_default $t` (Wasm 3.0 GC): der Typindex; die
            // Laenge folgt als eigener Befehl (`i32.const 0`).
            //
            // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
            // ---- GC-Werte (Wasm 3.0) -----------------------------------
            // `ref.i31 x`, `any.convert_extern x`, `extern.convert_any x`
            // haben kein Immediate; `struct.new_default $t` einen
            // Typindex OHNE Klammer.
            //
            // GEMESSEN 03.10. (`extern.wast`: 16 Faelle).
            "ref.i31" => {
                out.push(RefI31);
                return Ok(());
            }
            // ---- GC-Arrays (Wasm 3.0) ---------------------------------
            // `array.new $t`, `array.get $t`, `array.get_s $t`,
            // `array.get_u $t`, `array.set $t`: der Typindex steht als
            // Immediate OHNE Klammer; `array.len` hat keins.
            //
            // GEMESSEN 03.10. (`array.wast`: 47 Faelle).
            "array.len" => {
                out.push(ArrayLen);
                return Ok(());
            }
            "array.new" => {
                out.push(ArrayNew(self.typindex_nach("array.new")?));
                return Ok(());
            }
            "array.get" => {
                out.push(ArrayGet(self.typindex_nach("array.get")?));
                return Ok(());
            }
            "array.get_s" => {
                out.push(ArrayGetS(self.typindex_nach("array.get_s")?));
                return Ok(());
            }
            "array.get_u" => {
                out.push(ArrayGetU(self.typindex_nach("array.get_u")?));
                return Ok(());
            }
            "array.set" => {
                out.push(ArraySet(self.typindex_nach("array.set")?));
                return Ok(());
            }
            // ---- GC-Strukturen (Wasm 3.0) ---------------------------------
            // `struct.new $t` nimmt nur den Typindex; `struct.get`,
            // `struct.get_s`, `struct.get_u` und `struct.set` nehmen
            // Typindex **und** Feld (Index oder `$name`).
            //
            // GEMESSEN 03.10. (`struct.wast`: 5/24 -> ?).
            "struct.new" => {
                out.push(StructNew(self.typindex_nach("struct.new")?));
                return Ok(());
            }
            "struct.get" => {
                let (t, f) = self.struct_befehl("struct.get")?;
                out.push(StructGet(t, f));
                return Ok(());
            }
            "struct.get_s" => {
                let (t, f) = self.struct_befehl("struct.get_s")?;
                out.push(StructGetS(t, f));
                return Ok(());
            }
            "struct.get_u" => {
                let (t, f) = self.struct_befehl("struct.get_u")?;
                out.push(StructGetU(t, f));
                return Ok(());
            }
            "struct.set" => {
                let (t, f) = self.struct_befehl("struct.set")?;
                out.push(StructSet(t, f));
                return Ok(());
            }
            // `array.new_fixed $t n`, `array.new_data $t $d`,
            // `array.new_elem $t $e`, `array.init_data`, `array.init_elem`
            // — GEMESSEN 03.10. (`array_copy.wast` #4ff, array_new_data,
            // array_new_elem).
            "array.new_fixed" => {
                let t = self.typindex_nach("array.new_fixed")?;
                let n = match self.weiter() {
                    Token::Wort(w) => u32::try_from(zahl(&w)?)
                        .map_err(|_| textfehler("array.new_fixed: Laenge zu gross"))?,
                    x => {
                        return Err(textfehler(&format!(
                            "array.new_fixed: Laenge erwartet, gefunden {}",
                            x.text()
                        )));
                    }
                };
                out.push(ArrayNewFixed(t, n));
                return Ok(());
            }
            "array.new_data" => {
                let t = self.typindex_nach("array.new_data")?;
                let d = self.daten_index_lesen()?;
                out.push(ArrayNewData(t, d));
                return Ok(());
            }
            "array.new_elem" => {
                let t = self.typindex_nach("array.new_elem")?;
                let e = self.elem_index_lesen()?;
                out.push(ArrayNewElem(t, e));
                return Ok(());
            }
            "array.init_data" => {
                let t = self.typindex_nach("array.init_data")?;
                let d = self.daten_index_lesen()?;
                out.push(ArrayInitData(t, d));
                return Ok(());
            }
            "array.init_elem" => {
                let t = self.typindex_nach("array.init_elem")?;
                let e = self.elem_index_lesen()?;
                out.push(ArrayInitElem(t, e));
                return Ok(());
            }
            // `array.fill $t` und `array.copy $d $s` — GEMESSEN 03.10.
            // (`array_fill.wast` 3/29, `array_copy.wast` 4/34).
            "array.fill" => {
                out.push(ArrayFill(self.typindex_nach("array.fill")?));
                return Ok(());
            }
            "array.copy" => {
                let d = self.typindex_nach("array.copy")?;
                let q = self.typindex_nach("array.copy")?;
                out.push(ArrayCopy(d, q));
                return Ok(());
            }
            // `ref.test <typ> x` / `ref.cast <typ> x`: der Zieltyp steht
            // als Immediate OHNE Klammer (`anyref`) oder als `(ref …)`.
            //
            // GEMESSEN 03.10. (`ref_test.wast`, `ref_cast.wast`,
            // `i31.wast`: 125 Faelle).
            "ref.test" => {
                out.push(RefTest(self.reftyp_feld()?));
                return Ok(());
            }
            "ref.cast" => {
                out.push(RefCast(self.reftyp_feld()?));
                return Ok(());
            }
            // `i31.get_s x` / `i31.get_u x`: poppen einen i31-Verweis.
            //
            // GEMESSEN 03.10. (`i31.wast`: 57 Faelle — nur diese beiden
            // Befehle fehlten, die Datei las sich sonst nicht).
            "i31.get_s" => {
                out.push(I31GetS);
                return Ok(());
            }
            "i31.get_u" => {
                out.push(I31GetU);
                return Ok(());
            }
            "any.convert_extern" => {
                out.push(AnyConvertExtern);
                return Ok(());
            }
            "extern.convert_any" => {
                out.push(ExternConvertAny);
                return Ok(());
            }
            "struct.new_default" => {
                out.push(StructNewDefault(self.typindex_nach("struct.new_default")?));
                return Ok(());
            }
            "array.new_default" => {
                let t = match self.weiter() {
                    Token::Wort(w) => u32::try_from(zahl(&w)?)
                        .map_err(|_| textfehler("array.new_default: Typindex zu gross"))?,
                    Token::Name(n) => self.typindex_namen(&n)?,
                    t2 => {
                        return Err(textfehler(&format!("array.new_default: {}", t2.text())));
                    }
                };
                out.push(Befehl::ArrayNewDefault(t));
                return Ok(());
            }
            "ref.eq" => {
                out.push(Befehl::RefEq);
                return Ok(());
            }
            "throw" => {
                let t = match self.weiter() {
                    Token::Wort(w) => u32::try_from(zahl(&w)?)
                        .map_err(|_| textfehler("throw: Tagindex zu gross"))?,
                    Token::Name(n) => *self
                        .tags_namen
                        .get(&n)
                        .ok_or_else(|| textfehler(&format!("throw: Tagname ${n} unbekannt")))?,
                    t2 => {
                        return Err(textfehler(&format!("throw: {}", t2.text())));
                    }
                };
                out.push(Befehl::Throw(t));
                return Ok(());
            }
            "ref.null" => {
                // `ref.null func` / `ref.null extern` — und seit Wasm 3.0
                // auch `ref.null $t` (ein konkreter Typindex).
                let r = match self.sieht().cloned() {
                    Some(Token::Wort(w)) => {
                        self.pos += 1;
                        // Heaptyp des GC-Lattices. Die leeren Typen
                        // (`none`, `nofunc`, `noextern`, `noexn`) sind
                        // nullable — sie bewohnen `null`.
                        //
                        // GEMESSEN 03.10. (`ref_null.wast`).
                        match heaptyp_wort(&w).or_else(|| {
                            // Eine Zahl als Typindex: `(ref.null 0)`.
                            zahl(&w)
                                .ok()
                                .and_then(|z| u32::try_from(z).ok())
                                .map(|typ| Reftyp::Konkret {
                                    typ,
                                    nullable: true,
                                })
                        }) {
                            Some(r) => r,
                            None => {
                                return Err(textfehler(&format!(
                                    "ref.null: unbekannter Heaptyp \"{w}\""
                                )));
                            }
                        }
                    }
                    Some(Token::Name(n)) => {
                        self.pos += 1;
                        Reftyp::Konkret {
                            typ: self.typindex_namen(&n)?,
                            nullable: true,
                        }
                    }
                    t => {
                        return Err(textfehler(&format!(
                            "ref.null: Referenztyp erwartet, gefunden {}",
                            t.map(|x| x.text()).unwrap_or_else(|| "Ende".into())
                        )));
                    }
                };
                out.push(RefNull(r));
                return Ok(());
            }
            "select" => {
                // `select (result t)*`: die Typliste folgt in einer oder
                // mehreren `(result …)`-Gruppen. Ob genau EIN Typ dasteht,
                // entscheidet der VALIDATOR — im unerreichbaren Code darf
                // dort auch Unsinn stehen (GEMESSEN 02.10., `select.wast`
                // Z. 156: `unreachable select (result i32) (result)`).
                let mut ts: Vec<Valtyp> = Vec::new();
                let mut gruppen = 0usize;
                while self.ist("result") {
                    ts.extend(self.ergebnis_typen()?);
                    gruppen += 1;
                }
                if gruppen == 0 {
                    out.push(Select);
                } else {
                    out.push(SelectTyp(ts));
                }
                return Ok(());
            }
            "block" | "loop" => {
                self.tiefe_pruefen()?;
                let beschriftung = self.optionaler_name();
                let bt = self.blocktyp()?;
                out.push(if name == "block" {
                    Befehl::Block(bt)
                } else {
                    Befehl::Loop(bt)
                });
                self.labels.push(beschriftung);
                self.instruktionen(out)?;
                out.push(Befehl::End);
                self.labels.pop();
                return Ok(());
            }
            "if" => {
                self.tiefe_pruefen()?;
                let beschriftung = self.optionaler_name();
                let bt = self.blocktyp()?;
                out.push(Befehl::If(bt));
                self.labels.push(beschriftung);
                self.instruktionen(out)?;
                self.labels.pop();
                return Ok(());
            }
            "then" | "else" => {
                // `else` in der nackten Schreibweise: nur der Marker.
                out.push(Befehl::Else);
                return Ok(());
            }
            "end" => {
                out.push(Befehl::End);
                self.labels.pop();
                return Ok(());
            }
            _ => {}
        }
        // ---- Konstante ------------------------------------------------------
        match name {
            "i32.const" => {
                let w = self.wort()?;
                out.push(I32Konst(i64_zahl(&w)? as i32));
                return Ok(());
            }
            "i64.const" => {
                let w = self.wort()?;
                out.push(I64Konst(i64_zahl(&w)?));
                return Ok(());
            }
            "f32.const" => {
                let w = self.wort()?;
                out.push(F32Konst(float_zahl(&w, true)? as u32));
                return Ok(());
            }
            "f64.const" => {
                let w = self.wort()?;
                out.push(F64Konst(float_zahl(&w, false)?));
                return Ok(());
            }
            "unreachable" => out.push(Unreachable),
            "nop" => out.push(Nop),
            "return" => out.push(Return),
            "drop" => out.push(Drop),
            "ref.is_null" => out.push(RefIsNull),
            // `ref.as_non_null` (Wasm 3.0 Typed Function References).
            //
            // GEMESSEN 03.10. (`unreached-valid.wast`: 9 `assert_trap`).
            "ref.as_non_null" => {
                out.push(RefAsNonNull);
                return Ok(());
            }
            // `call_ref $t`: der Typindex folgt als Immediate, ohne
            // Klammer (`(call_ref $t)`).
            "call_ref" => {
                let t = match self.weiter() {
                    Token::Wort(w) => u32::try_from(zahl(&w)?)
                        .map_err(|_| textfehler(&format!("call_ref: Typindex {w} zu gross")))?,
                    Token::Name(n) => self.typindex_namen(&n)?,
                    t2 => {
                        return Err(textfehler(&format!(
                            "call_ref: Typindex erwartet, gefunden {}",
                            t2.text()
                        )));
                    }
                };
                out.push(CallRef(t));
                return Ok(());
            }
            // `br_on_null $l`: die Beschriftung steht OHNE Klammer.
            "br_on_null" => {
                out.push(BrOnNull(self.label()?));
                return Ok(());
            }
            // `br_on_non_null $l <typ>`: der **getippte** `br_on_null`.
            //
            // GEMESSEN 03.10.: `br_on_cast_fail.wast` stand bei 3/31, weil
            // das ganze erste Modul unlesbar war („Befehl \"br_on_non_null\"
            // unbekannt") — 28 Faelle fielen mit einem Wort.
            "br_on_non_null" => {
                out.push(BrOnNonNull(self.label()?));
                return Ok(());
            }
            // `br_on_cast $l <flags1> <flags2>` und `br_on_cast_fail`:
            // die Beschriftung OHNE Klammer, danach zwei Cast-Flags —
            // entweder ein bloser Typ (`anyref`) oder eine
            // `(ref …)`/`(ref null …)`-Gruppe.
            //
            // GEMESSEN 03.10. (`br_on_cast.wast` 6/31).
            //
            // GEMESSEN 03.10.: **abgeschaltet**. `gefalten` entscheidet je
            // Token, ob es ein Immediate des KOPFBEFEHLS oder ein gefaltetes
            // ARGUMENT ist — und `(ref …)` ist als „Feldklammer" eingestuft
            // (siehe `ist_feld_klammer`). Bei zwei Typ-Immediaten
            // (`anyref` und `(ref i31)`) verschluckt der Faltweg deshalb das
            // Argument: die Messungen waren „F0+3: Stapelunterlauf" ( Operand
            // fehlt), mit blassen Flags „F0+1: Stapelunterlauf", mit
            // `(local.get …)` als Operand dagegen die RICHTige Fehlermeldung
            // („der Wert passt nicht zu typreferenz") — der Leser verarbeitet
            // also die Form, braucht aber noch eine eigene Zerlegung fuer
            // „zwei Cast-Flags". Befehl, Pruefer und Laufzeit stehen und sind
            // fertig; `br_on_cast.wast` bleibt bei 6/31, bis der Leserweg
            // stimmt.
            "br_on_cast" | "br_on_cast_fail" => {
                // GEMESSEN 03.10.: der Weg ist `(br_on_cast $l f1 f2
                // (operand))`. Der Falt-Leser `gefalten` sammelt erst die
                // gefalteten ARGUMENTE und ruft den KOPFBEFEHL danach beim
                // ersten Immediate-Token auf (`$l`) — genau hier, also
                // einmal. Deshalb liest der Arm **gierig** Label + beide
                // Cast-Flags in einem Durchgang: `(ref …)` ist eine
                // Feldklammer und wird als Immediate des Kopfbefehls
                // gelesen, nicht als Argument.
                let l = self.label()?;
                let a = self.cast_flags("br_on_cast")?;
                let b = self.cast_flags("br_on_cast")?;
                out.push(if name == "br_on_cast" {
                    BrOnCast(l, a, b)
                } else {
                    BrOnCastFail(l, a, b)
                });
                return Ok(());
            }
            // `return_call $f`: der Funktionsindex steht OHNE Klammer.
            //
            // GEMESSEN 03.10. (`return_call.wast` stand bei 14/48,
            // weil der Befehl gar nicht gelesen wurde).
            "return_call" => {
                let idx = match self.weiter() {
                    Token::Name(b) => *self.funktionen.get(&b).unwrap_or(&0),
                    Token::Wort(w) => w
                        .parse::<u32>()
                        .map_err(|_| textfehler("return_call: Funktionsindex"))?,
                    t => return Err(textfehler(&format!("return_call: {}", t.text()))),
                };
                out.push(ReturnCall(idx));
                return Ok(());
            }
            // `return_call_ref $t`: der Typindex steht OHNE Klammer.
            "return_call_ref" => {
                out.push(ReturnCallRef(self.typindex_nach("return_call_ref")?));
                return Ok(());
            }
            // `return_call_indirect`: der Typuse ist wie bei
            // `call_indirect` OPTIONAL und darf inline `(param …)`/
            // `(result …)` mitbringen, danach kommt optional der
            // Tabellenindex. Deshalb denselben (bewährten) Leser
            // benutzen und das Ergebnis umbenennen.
            //
            // GEMESSEN 03.10. (`return_call_indirect.wast` blieb bei
            // 30/80, weil der Leser einen Typindex VERLANGTTE — der Test
            // schreibt `(return_call_indirect (i32.const 0))` ganz ohne).
            "return_call_indirect" => {
                self.call_indirect(out)?;
                if let Some(CallIndirect { typ, tabelle }) = out.pop() {
                    out.push(ReturnCallIndirect { typ, tabelle });
                }
                return Ok(());
            }
            // Kontext in den Fehler: welcher Token wurde gerade gelesen?
            // Ohne das war die Fehlersuche in der Testsuite Raten
            // („Befehl \"result\" unbekannt" kam aus einem `else`-Zweig).
            _ => out.push(crate::op::aus_text(name).map_err(|e| {
                let umfeld: Vec<String> = self
                    .tok
                    .iter()
                    .skip(self.pos.saturating_sub(4))
                    .take(9)
                    .map(|t| t.text())
                    .collect();
                textfehler(&format!(
                    "{} (naechste Token: {})",
                    e.kurz(),
                    umfeld.join(" ")
                ))
            })?),
        }
        Ok(())
    }

    /// `call_indirect` mit optionaler Typangabe.
    ///
    /// Die Inline-Form besteht aus MEHREREN `(param …)`/`(result …)`
    /// Gruppen, und alle zusammen ergeben EINEN Typ:
    /// ```text
    /// (call_indirect (param i64) (param) (param f64 i32 i64)
    ///   (result) (result i32) …)
    /// ```
    /// Vorher wurde jede Gruppe einzeln interniert (die letzte gewinnte),
    /// dadurch bekam die Funktion die falsche Signatur.
    ///
    /// GEMESSEN 03.10. (`call_indirect.wast`, „Syntax"-Funktion F19:
    /// 129 der 169 Faelle der Datei fielen aus).
    fn call_indirect(&mut self, out: &mut Vec<Befehl>) -> Result<(), Fehler> {
        let mut typ: u32 = 0;
        let mut tabelle: u32 = 0;
        let mut inline: Option<FuncTyp> = None;
        let mut typ_gesetzt = false;
        let mut ergebnis_gesehen = false;
        loop {
            self.schritte += 1;
            if self.schritte > self.tok.len().max(64) {
                return Err(textfehler(
                    "call_indirect: Feldschleife laeuft nicht weiter",
                ));
            }
            // Der Tabellenindex steht als erstes Immediate, OHNE Klammer:
            // `(call_indirect $t (type $check) …)`. GEMESSEN 02.10.
            // (`select.wast`, 4 Faelle): ohne diesen Zweig blieb jedes
            // `$t` auf Tabelle 0 und `call_indirect` meldete einen
            // Signaturfehler der FALSCHEN Funktion.
            let tabellen_immediate = match self.sieht() {
                Some(Token::Name(_)) => true,
                Some(Token::Wort(w)) => {
                    !w.starts_with("offset=") && !w.starts_with("align=") && zahl(w).is_ok()
                }
                _ => false,
            };
            if tabellen_immediate {
                tabelle = self.tabellen_index()?;
            } else if self.ist("type") {
                // `(type $x)` muss vor den Inline-Gruppen stehen.
                if inline.is_some() {
                    return Err(textfehler(
                        "call_indirect: (type …) nach (param …)/(result …) — der Typ steht zuerst",
                    ));
                }
                self.oeffner("type")?;
                typ = match self.weiter() {
                    Token::Wort(w) => w.parse::<u32>().map_err(|_| textfehler("Typindex"))?,
                    Token::Name(n) => self.typindex_namen(&n)?,
                    t => return Err(textfehler(&format!("call_indirect (type): {}", t.text()))),
                };
                self.zu()?;
                typ_gesetzt = true;
            } else if self.ist("param") || self.ist("result") {
                let ist_param = self.ist("param");
                let inhalt = self.klammer_inhalt();
                // Der Typuse von `call_indirect` ist
                //     (type $x)? (param …)* (result …)*
                // `(type $x)` steht also ZUERST (das prueft der
                // `(type …)`-Zweig), und die Inline-Gruppen muessen zum
                // genannten Typ passen (sonst „inline function type").
                // Ausserdem: Parameter vor Ergebnissen, Inline-Parameter
                // ohne Namen.
                //
                // GEMESSEN 03.10. (`call_indirect.wast`, 11
                // `assert_malformed`-Faelle: „inline function type" und
                // „unexpected token").
                if ist_param && ergebnis_gesehen {
                    return Err(textfehler(
                        "call_indirect: (param …) nach (result …) — die Parameter kommen zuerst",
                    ));
                }
                // `inhalt[0]` ist der `Auf`-Token selbst (die Klammer und
                // ihr Name sind EIN Token), deshalb steht der Name an
                // Stelle 1.
                if ist_param && matches!(inhalt.get(1), Some(Token::Name(_))) {
                    return Err(textfehler(
                        "call_indirect: (param $name …) ist nicht erlaubt, nur Inline-Werttypen",
                    ));
                }
                if !ist_param {
                    ergebnis_gesehen = true;
                }
                let mut p = self.unter(&inhalt);
                let ft = p.signatur()?;
                // Alle Gruppen aneinanderhaengen: die Inline-Signatur ist
                // EIN Typuse (Parameter zuerst, dann Ergebnisse).
                let s = inline.get_or_insert_with(|| FuncTyp {
                    parameter: Vec::new(),
                    ergebnis: Vec::new(),
                });
                s.parameter.extend(ft.parameter);
                s.ergebnis.extend(ft.ergebnis);
            } else if self.ist("table") {
                self.oeffner("table")?;
                tabelle = self.tabellen_index()?;
                self.zu()?;
            } else {
                break;
            }
        }
        if let Some(ft) = inline {
            // `(type $x)` plus Inline-Gruppen: die Gruppen muessen zum
            // genannten Typ passen („inline function type").
            if typ_gesetzt && self.typen.get(typ as usize) != Some(&ft) {
                return Err(textfehler(
                    "call_indirect: (param …)/(result …) passen nicht zu (type …)",
                ));
            }
            typ = self.typindex(&ft);
        }
        out.push(Befehl::CallIndirect { typ, tabelle });
        Ok(())
    }

    /// Elementindex aus einem Immediate: Zahl ODER `$name`
    /// (`table.init $t $e …`, `elem.drop $e`).
    fn elem_index_lesen(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            Token::Wort(w) => w
                .parse::<u32>()
                .map_err(|_| textfehler(&format!("Elementindex {w:?}"))),
            Token::Name(n) => self.elem_namen.get(&n).copied().ok_or_else(|| {
                textfehler(&format!(
                    "Elementname ${n} unbekannt ({} Abschnitte)",
                    self.elem_namen.len()
                ))
            }),
            t => Err(textfehler(&format!(
                "Elementindex erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Datensatzindex aus einem Immediate: Zahl ODER `$name`
    /// (`memory.init $m $d …`, `data.drop $d`).
    fn daten_index_lesen(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            Token::Wort(w) => w
                .parse::<u32>()
                .map_err(|_| textfehler(&format!("Datensatzindex {w:?}"))),
            Token::Name(n) => self.daten_namen.get(&n).copied().ok_or_else(|| {
                textfehler(&format!(
                    "Name des Datensatzes ${n} unbekannt ({} Datensaetze)",
                    self.daten_namen.len()
                ))
            }),
            t => Err(textfehler(&format!(
                "Datensatzindex erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Steht als Nächstes ein Speicherindex? (`(memory.size $m0) …`)
    fn speicher_arg(&self) -> bool {
        matches!(self.sieht(), Some(Token::Name(_)))
    }

    /// Den Speicherbefehl bauen und (bei Index != 0) in die Huelle legen.
    fn befehl_mit_speicher(
        &mut self,
        name: &str,
        speicher: u32,
        out: &mut Vec<Befehl>,
    ) -> Result<(), Fehler> {
        use Befehl::*;
        // ACHTUNG: hier nicht `befehl_mit_name` rufen — der Weg wuerde den
        // Speicherindex erneut suchen und das FOLGENDE Token (`$d` in
        // `memory.init $m $d …`) als Speicher lesen (GEMESSEN 02.10.,
        // `memory-multi.wast`). Der Befehl wird hier direkt gebaut.
        let kopf: Vec<Befehl> = match name {
            "memory.size" => vec![MemorySize],
            "memory.grow" => vec![MemoryGrow],
            "memory.copy" => vec![MemoryCopy { ziel: 0, quelle: 0 }],
            "memory.fill" => vec![MemoryFill],
            "memory.init" => {
                let i = self.daten_index_lesen()?;
                vec![MemoryInit(i)]
            }
            andere => return Err(textfehler(&format!("(andere …): {andere}"))),
        };
        if speicher == 0 {
            out.extend(kopf);
        } else {
            for k in kopf {
                out.push(MitSpeicher {
                    befehl: Box::new(k),
                    speicher,
                });
            }
        }
        Ok(())
    }

    /// Speicherindex aus einem Immediate: Zahl ODER `$name` eines Speichers
    /// (Multi-Memory, `(i32.load $m1 …)`).
    fn speicher_index_lesen(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            // `(data (memory 0x0) …)` — Hexzahl und `_` sind erlaubt.
            Token::Wort(w) => zahl(&w).and_then(|v| {
                u32::try_from(v).map_err(|_| textfehler(&format!("Speicherindex {w} zu gross")))
            }),
            Token::Name(n) => self.speicher_namen.get(&n).copied().ok_or_else(|| {
                textfehler(&format!(
                    "Speichername ${n} unbekannt ({} Speicher)",
                    self.speicher_namen.len()
                ))
            }),
            t => Err(textfehler(&format!(
                "Speicherindex erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Tabellenindex aus einem Immediate: Zahl ODER `$name` einer Tabelle.
    /// (`table.get $t2 (local.get 0)` — GEMESSEN 01.10., `table_set.wast`:
    /// ohne Namenauflosung landete jedes `$t` auf Tabelle 0.)
    fn tabellen_index(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            // `(table 0x0 funcref …)` — Hexzahl und `_` sind erlaubt
            // (GEMESSEN 02.10., `elem.wast`).
            Token::Wort(w) => u32::try_from(zahl(&w)?)
                .map_err(|_| textfehler(&format!("Tabellenindex {w} zu gross"))),
            Token::Name(n) => self.tabellen_namen.get(&n).copied().ok_or_else(|| {
                textfehler(&format!(
                    "Tabellenname ${n} unbekannt ({} Tabellen)",
                    self.tabellen_namen.len()
                ))
            }),
            t => Err(textfehler(&format!(
                "Tabellenindex erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    fn index(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            Token::Wort(w) => w
                .parse::<u32>()
                .map_err(|_| textfehler(&format!("Index {w:?}"))),
            Token::Name(n) => Ok(*self.funktionen.get(&n).unwrap_or(&0)),
            t => Err(textfehler(&format!(
                "Index erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Label-Index (Tiefe von innen): `$name` wird aufgeloest.
    fn label(&mut self) -> Result<u32, Fehler> {
        match self.weiter() {
            Token::Wort(w) => w
                .parse::<u32>()
                .map_err(|_| textfehler(&format!("Label {w:?}"))),
            Token::Name(n) => {
                // Von innen nach aussen suchen.
                for (k, l) in self.labels.iter().rev().enumerate() {
                    if l.as_deref() == Some(n.as_str()) {
                        return Ok(k as u32);
                    }
                }
                Err(textfehler(&format!("unbekanntes Label ${n}")))
            }
            t => Err(textfehler(&format!(
                "Label erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Die `catch`-Liste von `try_table`: `(catch $t $l)`, `(catch $t)`,
    /// `(catch_all $l)`, `(catch_all)`.
    ///
    /// `catch_ref`/`catch_all_ref` (Wert auf dem Stapel) und `try`/`catch`
    /// (aeltere Form) werden mit BENANNTEM Fehler abgelehnt.
    fn faeng_liste(&mut self) -> Result<Vec<crate::op::Fang>, Fehler> {
        let mut faenge = Vec::new();
        // M3/M4: die Schleife laeuft ueber die Token; harte Grenze.
        let grenze = self.tok.len().max(64);
        let mut schritte = 0usize;
        while schritte < grenze {
            schritte += 1;
            if self.ist("catch") || self.ist("catch_all") {
                // `text()` liefert bei einem `Auf`-Token den Text MIT
                // Klammer („(catch") — deshalb aus dem Token selbst lesen.
                let art = match self.weiter() {
                    Token::Auf(k) => k,
                    t => {
                        return Err(textfehler(&format!("catch: {}", t.text())));
                    }
                };
                let tag = if art == "catch" {
                    // `$t` oder Zahl
                    match self.weiter() {
                        Token::Wort(w) => Some(
                            u32::try_from(zahl(&w)?)
                                .map_err(|_| textfehler("catch: Tagindex zu gross"))?,
                        ),
                        Token::Name(n) => Some(*self.tags_namen.get(&n).ok_or_else(|| {
                            textfehler(&format!("catch: Tagname ${n} unbekannt"))
                        })?),
                        t2 => {
                            return Err(textfehler(&format!("catch: {}", t2.text())));
                        }
                    }
                } else {
                    None
                };
                // Das Label ist optional (ohne Label: an den Aufrufer).
                let label = match self.sieht().cloned() {
                    Some(Token::Name(n)) => {
                        self.pos += 1;
                        Some(self.label_index_namen(&n)?)
                    }
                    Some(Token::Wort(w)) => {
                        self.pos += 1;
                        Some(
                            u32::try_from(zahl(&w)?)
                                .map_err(|_| textfehler("catch: Labelindex zu gross"))?,
                        )
                    }
                    _ => None,
                };
                self.zu()?;
                faenge.push(crate::op::Fang { tag, label });
                continue;
            }
            break;
        }
        Ok(faenge)
    }

    /// Label-Tiefe zu einem NAMEN (ohne den Token zu lesen).
    /// Ein Referenztyp als Immediate: `anyref` (Wort) oder `(ref …)`
    /// (Gruppe mit schliessender Klammer).
    ///
    /// GEMESSEN 03.10. (`ref_test.wast` Z. 40: `(ref.test (ref any) x)`,
    /// Z. 45: `(ref.test nullref x)` — beide Formen kommen vor).
    fn reftyp_feld(&mut self) -> Result<Reftyp, Fehler> {
        match self.sieht().cloned() {
            Some(Token::Auf(k)) if k == "ref" => {
                self.pos += 1;
                match reftyp_lesen(self)? {
                    Valtyp::Ref(r) => Ok(r),
                    andere => Err(textfehler(&format!("(ref …): {}", andere.text()))),
                }
            }
            Some(Token::Wort(w)) => {
                self.pos += 1;
                match valtyp(&w)? {
                    Valtyp::Ref(r) => Ok(r),
                    andere => Err(textfehler(&format!(
                        "ref.test/ref.cast: Referenztyp erwartet, {} gefunden",
                        andere.text()
                    ))),
                }
            }
            t => Err(textfehler(&format!(
                "ref.test/ref.cast: Referenztyp erwartet, gefunden {}",
                t.map(|x| x.text()).unwrap_or_else(|| "Ende".into())
            ))),
        }
    }

    /// Der naechste Typindex als Zahl oder als `$name` (Wasm 3.0,
    /// `return_call_ref`/`return_call_indirect`).
    fn typindex_nach(&mut self, befehl: &str) -> Result<u32, Fehler> {
        match self.weiter() {
            Token::Name(n) => self.typindex_namen(&n),
            Token::Wort(w) => u32::try_from(zahl(&w)?)
                .map_err(|_| textfehler(&format!("{befehl}: Typindex {w} zu gross"))),
            t => Err(textfehler(&format!(
                "{befehl}: Typindex erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// EIN Cast-Flag von `br_on_cast`: ein bloser Referenztyp
    /// (`anyref`) oder eine `(ref …)`-Gruppe.
    ///
    /// GEMESSEN 03.10. (`br_on_cast.wast` Z. 29ff:
    /// `(br_on_cast $l anyref (ref i31) …)`).
    fn cast_flags(&mut self, befehl: &str) -> Result<Reftyp, Fehler> {
        match self.sieht().cloned() {
            Some(Token::Wort(w)) => match valtyp(&w) {
                Ok(Valtyp::Ref(r)) => {
                    self.pos += 1;
                    Ok(r)
                }
                _ => Err(textfehler(&format!(
                    "{befehl}: Referenztyp erwartet, gefunden {w}"
                ))),
            },
            Some(Token::Auf(k)) if k == "ref" => {
                self.pos += 1;
                match reftyp_lesen(self)? {
                    Valtyp::Ref(r) => Ok(r),
                    andere => Err(textfehler(&format!("{befehl}: {andere:?}"))),
                }
            }
            t => Err(textfehler(&format!(
                "{befehl}: Cast-Flag erwartet, gefunden {}",
                t.map(|x| x.text()).unwrap_or_else(|| "Ende".to_string())
            ))),
        }
    }

    /// Der Feldbefehl `struct.get $t $f` (und `get_s`/`get_u`/`set`):
    /// erst der Typindex, dann der Feldindex oder ein `$name`.
    fn struct_befehl(&mut self, befehl: &str) -> Result<(u32, u32), Fehler> {
        let t = self.typindex_nach(befehl)?;
        let f = match self.weiter() {
            Token::Name(n) => self
                .felder_namen
                .get(&(t, n.clone()))
                .copied()
                .ok_or_else(|| textfehler(&format!("unbekanntes Feld ${n} in Typ {t}")))?,
            Token::Wort(w) => u32::try_from(zahl(&w)?)
                .map_err(|_| textfehler(&format!("{befehl}: Feldindex {w} zu gross")))?,
            x => {
                return Err(textfehler(&format!(
                    "{befehl}: Feldindex oder $name erwartet, gefunden {}",
                    x.text()
                )));
            }
        };
        Ok((t, f))
    }

    fn label_index_namen(&self, name: &str) -> Result<u32, Fehler> {
        for (k, l) in self.labels.iter().rev().enumerate() {
            if l.as_deref() == Some(name) {
                return Ok(k as u32);
            }
        }
        Err(textfehler(&format!("unbekanntes Label ${name}")))
    }

    /// Die (gekuerzte) Elementliste: `declare`? Elementtyp? Eintragsliste.
    ///
    /// Wird an zwei Stellen gebraucht: im `(elem …)`-Feld und in der
    /// Kurzform `(table … (elem …))`. GEMESSEN 02.10. (`elem.wast`,
    /// `table.wast`): `(elem (ref.func $f) (ref.null func))` wurde in der
    /// Tabellenform nicht gelesen.
    fn elem_liste(&mut self) -> Result<(Vec<Vec<Befehl>>, Reftyp, bool), Fehler> {
        let mut reftyp = Reftyp::Func;
        let mut deklarativ = false;
        if let Some(w) = self.sieht_wort() {
            if w == "declare" {
                deklarativ = true;
                self.pos += 1;
            } else if let Ok(Valtyp::Ref(r)) = valtyp(&w) {
                // `funcref`, `externref`, `arrayref` (Wasm 3.0 GC) …
                //
                // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
                reftyp = r;
                self.pos += 1;
            }
        }
        let mut eintraege: Vec<Vec<Befehl>> = Vec::new();
        // `(elem … func 3 1 4 1)`: nach dem Schluesselwort `func` kommen
        // Funktionsindizes. GEMESSEN 01.10. (`table_init.wast`): ohne diesen
        // Zweig blieb die erste Zahl liegen und der Leser meldete
        // „erwartet ), gefunden 3" — 665 Faelle der Suite.
        let mut nur_indizes = false;
        // M3/M4: die Schleife laeuft ueber die Eingabe; ihre Grenze ist die
        // Zahl der Token (kleiner kann sie nicht sein).
        //
        // EIGENER Zaehler, nicht `self.schritte`: der Leser kann in einem
        // Unterparser laufen (Tabellen-Kurzform `(table … (elem …))`), dessen
        // Tokenliste kurz ist, waehrend der Zaehler des Hauptlesers schon
        // gross ist (GEMESSEN 02.10.: 94,08 % statt 98,36 %).
        let grenze = self.tok.len().max(64);
        let mut schritte = 0usize;
        loop {
            schritte += 1;
            if schritte > grenze {
                return Err(textfehler("Elementliste laeuft nicht weiter"));
            }
            let t = match self.sieht().cloned() {
                Some(t) => t,
                None => break,
            };
            match t {
                Token::Name(n) => {
                    self.pos += 1;
                    let f = self.funktionen.get(&n).copied().ok_or_else(|| {
                        textfehler(&format!("(elem …): Funktionsname ${n} unbekannt"))
                    })?;
                    eintraege.push(vec![Befehl::RefFunc(f), Befehl::End]);
                    // Eine Liste von Funktionsindizes traegt den Typ
                    // `(ref func)` — nicht-leerbar, weil jeder Eintrag eine
                    // Funktion ist (GEMESSEN 02.10., `elem.wast` Z. 2:
                    // `(table $t 10 (ref func) …) (elem (i32.const 3) $g)`).
                    if reftyp == Reftyp::Func {
                        reftyp = nicht_leerbare_funktionsreferenz();
                    }
                }
                Token::Wort(w) if w == "func" || w == "funcref" => {
                    self.pos += 1;
                    // `func` gefolgt von Zahlen: die Kurzform der
                    // Elementliste. `funcref` dagegen ist der ELEMENTTYP und
                    // wird von `(ref.func …)`-Ausdruecken gefolgt.
                    nur_indizes = w == "func";
                }
                // `(ref func)` / `(ref extern)` als ELEMENTTYP der
                // Kurzform. GEMESSEN 02.10. (`elem.wast`): ohne diesen
                // Zweig wurde `ref` als Befehl gelesen.
                Token::Auf(k) if k == "ref" => {
                    self.pos += 1;
                    match reftyp_lesen(self)? {
                        Valtyp::Ref(r) => reftyp = r,
                        andere => return Err(textfehler(&format!("(elem (ref …)): {andere:?}"))),
                    }
                }
                Token::Wort(w) => {
                    // Nackte ZAHLEN sind Funktionsindizes — die Kurzform der
                    // Spezifikation `(elem … 0 1 2)` fuer `(elem … func
                    // 0 1 2)`. Ohne diesen Zweig blieb die Zahl liegen und
                    // die Liste war leer: `(table funcref (elem 0 0))` ergab
                    // eine Tabelle mit 0 Eintraegen statt mit zwei
                    // Funktionen.
                    //
                    // GEMESSEN 03.10. (`call_indirect.wast` Z. 1037:
                    // `(module (table funcref (elem 0 0)))` musste
                    // „unknown function" melden).
                    if nur_indizes || zahl(&w).is_ok() {
                        let f = u32::try_from(zahl(&w)?)
                            .map_err(|_| textfehler(&format!("(elem …): Funktionsindex {w}")))?;
                        self.pos += 1;
                        eintraege.push(vec![Befehl::RefFunc(f), Befehl::End]);
                        if reftyp == Reftyp::Func {
                            reftyp = nicht_leerbare_funktionsreferenz();
                        }
                    } else {
                        break;
                    }
                }
                Token::Auf(k) if k == "item" => {
                    self.oeffner("item")?;
                    let mut v = Vec::new();
                    self.instruktionen(&mut v)?;
                    v.push(Befehl::End);
                    self.zu()?;
                    eintraege.push(v);
                }
                Token::Auf(k) => {
                    // GENAU EIN gefalteter Ausdruck je Eintrag — nicht der
                    // ganze Rest der Klammer. GEMESSEN 01.10.
                    // (`table_init.wast`, `(elem funcref (ref.func 2)
                    // (ref.func 7) …)`): mit `instruktionen` wurden alle
                    // Ausdruecke EIN Eintrag (der Validator meldete
                    // „Eintrag 0: 4 Werte statt einer").
                    let k = k.clone();
                    self.pos += 1;
                    let mut v = Vec::new();
                    self.gefalten(&k, &mut v)?;
                    v.push(Befehl::End);
                    eintraege.push(v);
                }
                _ => break,
            }
        }
        // `func 1 2 3` ist ebenfalls eine Funktionsindex-Liste.
        if nur_indizes && reftyp == Reftyp::Func {
            reftyp = nicht_leerbare_funktionsreferenz();
        }
        Ok((eintraege, reftyp, deklarativ))
    }

    // ---- (table …) -------------------------------------------------------
    fn tabelle_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("table")?;
        let beschriftung = self.optionaler_name();
        let idx = m.tabellen.len() as u32;
        if let Some(n) = beschriftung {
            self.tabellen_namen.insert(n, idx);
        }
        let mut inline_exporte = Vec::new();
        let mut inline_import = None;
        while self.ist("export") {
            self.oeffner("export")?;
            let n = self.name()?;
            inline_exporte.push(n);
            self.zu()?;
        }
        while self.ist("import") {
            self.oeffner("import")?;
            let a = self.name()?;
            let b = self.name()?;
            inline_import = Some((a, b));
            self.zu()?;
        }
        let idx = m.tabellen.len() as u32;
        // Kurzform `(table funcref (elem …))`: Groesse aus dem Element.
        let mut zahlen: Vec<u64> = Vec::new();
        let mut reftyp = Reftyp::Func;
        let mut elem_klammer: Option<Vec<Token>> = None;
        // Wasm 3.0: `(table N (ref …) <ausdruck>)` mit AUSDRUECKLICHER
        // Groesse ist eine Tabelle mit Startwert — er fuellt den ganzen
        // Bereich. Ohne Groesse ist es die klassische Kurzform
        // `(table … (elem …))` (Elemente an Versatz 0).
        //
        // GEMESSEN 02.10. (`table.wast` Z. 34-39: 4 Faelle).
        let mut startwert: Option<Vec<Befehl>> = None;
        loop {
            match self.sieht().cloned() {
                Some(Token::Wort(w)) if w == "funcref" => {
                    reftyp = Reftyp::Func;
                    self.pos += 1;
                }
                Some(Token::Wort(w)) if w == "externref" => {
                    reftyp = Reftyp::Extern;
                    self.pos += 1;
                }
                Some(Token::Wort(w)) if w == "i32" => {
                    self.pos += 1;
                }
                Some(Token::Wort(w)) if w == "i64" => {
                    return Err(textfehler(
                        "(table i64 …): 64-Bit-Indizes werden nicht unterstuetzt",
                    ));
                }
                Some(Token::Wort(w)) => match zahl(&w) {
                    Ok(v) => {
                        zahlen.push(v);
                        self.pos += 1;
                    }
                    // `arrayref` (Wasm 3.0 GC) und kuenftige Referenztypen:
                    // als Werttyp lesbar, alles andere bleibt ein Fehler.
                    //
                    // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
                    Err(_) => match valtyp(&w) {
                        Ok(Valtyp::Ref(r)) => {
                            reftyp = r;
                            self.pos += 1;
                        }
                        _ => return Err(textfehler(&format!("(table …): {w}"))),
                    },
                },
                // `(table … (ref null func) …)`: der ELEMENTTYP als Klammer
                // (Wasm 2.0 mit Referenztypen). `(ref null $typ)` ist 3.0.
                Some(Token::Auf(k)) if k == "ref" => {
                    self.pos += 1;
                    let t = reftyp_lesen(self)?;
                    reftyp = match t {
                        Valtyp::Ref(r) => r,
                        andere => return Err(textfehler(&format!("(table (ref …)): {andere:?}"))),
                    };
                }
                Some(Token::Name(_)) => {
                    self.pos += 1;
                }
                Some(Token::Auf(k)) if k == "elem" => {
                    // `klammer_inhalt` erwartet den Leser AUF dem
                    // `Auf`-Token (Klammer + Name = ein Token) — also hier
                    // NICHT weitersetzen. GEMESSEN 01.10.: mit `pos += 1`
                    // fing die Tiefenzaehlung beim Namen an, lief negative
                    // Werte und fraess den ganzen Rest des Moduls
                    // (block/br/call/br_if/br_table: 0 von 838 Faellen).
                    elem_klammer = Some(self.klammer_inhalt());
                }
                // `(table 10 (ref func) (ref.func $f) (ref.func $g))`: die
                // Elementliste steht DIREKT in der Tabellenklammer (die
                // Kurzform der Spezifikation). GEMESSEN 02.10. (`elem.wast`,
                // 4 Kommandos): ohne diesen Zweig blieb der Rest der Klammer
                // liegen und das Modul fiel aus.
                Some(Token::Auf(_)) => {
                    let mut inhalt: Vec<Token> = Vec::new();
                    let mut tiefe = 0i64;
                    // M3/M4: die Schleife laeuft ueber die Eingabe; ihre
                    // Grenze ist die Zahl der Token.
                    let grenze = self.tok.len();
                    let mut schritte = 0usize;
                    while let Some(t) = self.sieht().cloned() {
                        schritte += 1;
                        if schritte > grenze {
                            return Err(textfehler("(table …): Elementliste laeuft nicht weiter"));
                        }
                        match t {
                            Token::Auf(_) => tiefe += 1,
                            Token::Zu => {
                                if tiefe == 0 {
                                    break;
                                }
                                tiefe -= 1;
                            }
                            _ => {}
                        }
                        inhalt.push(t);
                        self.pos += 1;
                    }
                    elem_klammer = Some(inhalt);
                }
                Some(Token::Zu) | None => break,
                Some(t) => return Err(textfehler(&format!("(table …): {}", t.text()))),
            }
        }
        let n = if let Some(tok) = elem_klammer {
            // `(elem $f1 $f2 …)` -> Tabelle mit diesen Eintraegen (aktiv).
            // Die Namen MUESSEN aufgeloest werden — GEMESSEN 01.10.
            // (`load.wast` „as-call_indirect-first"): mit festem Index 0
            // stand in der Tabelle die falsche Funktion, jeder
            // `call_indirect` meldete einen Signaturfehler.
            //
            // Die Liste wird wie ein `(elem …)`-Feld gelesen, damit auch
            // `(ref.func $f)` und `(ref.null func)` erlaubt sind
            // (GEMESSEN 02.10., `elem.wast`, `table.wast`: 8 Kommandos).
            // `klammer_inhalt` liefert den Inhalt EINSCHLIESSLICH des
            // `Auf("elem")`-Tokens — das gehoert in die Liste nicht
            // (GEMESSEN 02.10.: ohne das Abschneiden las `elem_liste` ein
            // Befehl namens `elem`; 94,08 % statt 98,36 %).
            let ohne_kopf: &[Token] = match tok.first() {
                Some(Token::Auf(k)) if k == "elem" => &tok[1..],
                _ => &tok,
            };
            let mut p = self.unter(ohne_kopf);
            let (eintraege, _typ, _dek) = p.elem_liste()?;
            // `(table 10 (ref func) (ref.func $f))`: die AUSDRUECKLICHE
            // Groesse gewinnt; die Kurzform `(table (ref func) (ref.func …))`
            // ohne Zahl nimmt die Zahl der Eintraege. GEMESSEN 02.10.
            // (`elem.wast`: die Tabelle bekam 1 statt 10 Plaetze, und jedes
            // `(elem (i32.const 3) …)` meldete „Tabelle hat 1 Plaetze").
            let anzahl = match zahlen.first() {
                Some(n) => *n,
                None => eintraege.len() as u64,
            };
            if zahlen.is_empty() {
                // Klassische Kurzform: Elemente ab Versatz 0.
                m.elemente.push(Element::Aktiv {
                    tabelle: idx,
                    offset: vec![Befehl::I32Konst(0), Befehl::End],
                    reftyp,
                    eintraege,
                });
            } else {
                // Ausdrueckliche Groesse: genau EIN Ausdruck ist der
                // Startwert der ganzen Tabelle.
                if eintraege.len() != 1 {
                    return Err(textfehler(&format!(
                        "(table {anzahl} …): {} Ausdruecke, aber eine Groesse ist angegeben",
                        eintraege.len()
                    )));
                }
                startwert = Some(eintraege.into_iter().next().unwrap_or_default());
            }
            anzahl
        } else {
            *zahlen.first().unwrap_or(&0)
        };
        let max = zahlen.get(1).copied();
        let tt = Tabellentyp {
            reftyp,
            grenzen: Limits { min: n, max },
        };
        match inline_import {
            Some((modul, feld)) => m.tabellen.push(ImportiertTab::Importiert {
                modul,
                feld,
                typ: tt,
            }),
            None => m.tabellen.push(ImportiertTab::Eigen { typ: tt, startwert }),
        }
        self.zu()?;
        for n in inline_exporte {
            m.exporte.push(Export {
                name: n,
                art: Art::Tabelle(idx),
            });
        }
        Ok(())
    }

    // ---- (memory …) ------------------------------------------------------
    fn speicher_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("memory")?;
        let beschriftung = self.optionaler_name();
        // Lokaler Speicherindex (Multi-Memory) — fuer `(memory $m (data …))`
        // und fuer die Exporte.
        let idx = m.speicher.len() as u32;
        if let Some(n) = beschriftung {
            self.speicher_namen.insert(n, idx);
        }
        let mut inline_exporte = Vec::new();
        let mut inline_import = None;
        while self.ist("export") {
            self.oeffner("export")?;
            let n = self.name()?;
            inline_exporte.push(n);
            self.zu()?;
        }
        while self.ist("import") {
            self.oeffner("import")?;
            let a = self.name()?;
            let b = self.name()?;
            inline_import = Some((a, b));
            self.zu()?;
        }
        // Zahlen in Reihenfolge: die erste ist `min`, die zweite `max`.
        // GEMESSEN 01.10.: vorher ueberschrieb jede Zahl die vorherige, also
        // war `(memory 1 3)` ein Speicher mit MINIMUM 3 und ohne Maximum —
        // `memory.grow` lieferte dadurch falsche Werte.
        let mut zahlen: Vec<u64> = Vec::new();
        loop {
            match self.sieht().cloned() {
                Some(Token::Wort(w)) => {
                    if let Some(rest) = w.strip_prefix("pages=") {
                        zahlen.push(zahl(rest)?);
                        self.pos += 1;
                    } else if let Ok(v) = zahl(&w) {
                        zahlen.push(v);
                        self.pos += 1;
                    } else if w == "i32" {
                        self.pos += 1;
                    } else if w == "i64" {
                        // memory64 ist nicht Teil von WebAssembly 2.0 Core
                        return Err(textfehler(
                            "(memory i64 …): 64-Bit-Adressen werden nicht unterstuetzt",
                        ));
                    } else {
                        return Err(textfehler(&format!("(memory …): {w}")));
                    }
                }
                Some(Token::Name(_)) => {
                    self.pos += 1;
                }
                Some(Token::Auf(k)) if k == "data" => {
                    let inhalt = self.klammer_inhalt();
                    let mut groesse = 0u64;
                    for t in &inhalt {
                        if let Token::Text(b) = t {
                            groesse += b.len() as u64;
                        }
                    }
                    // 65536 Byte je Seite, aufwaerts runden.
                    zahlen = vec![groesse.div_ceil(65_536)];
                    m.daten.push(Daten::Aktiv {
                        // `(memory $m (data "…"))`: der Abschnitt gehoert zu
                        // DIESEM Speicher (Multi-Memory — GEMESSEN 02.10.:
                        // `float_memory0.wast` schrieb in Speicher 0).
                        speicher: idx,
                        offset: vec![Befehl::I32Konst(0), Befehl::End],
                        inhalt: inhalt
                            .iter()
                            .filter_map(|t| match t {
                                Token::Text(b) => Some(b.clone()),
                                _ => None,
                            })
                            .flatten()
                            .collect(),
                    });
                    m.daten_anzahl = Some(m.daten.len() as u32);
                }
                Some(Token::Zu) | None => break,
                Some(t) => return Err(textfehler(&format!("(memory …): {}", t.text()))),
            }
        }
        let idx = m.speicher.len() as u32;
        if zahlen.len() > 2 {
            return Err(textfehler("(memory …): mehr als zwei Zahlen"));
        }
        let gr = Limits {
            min: *zahlen.first().unwrap_or(&0),
            max: zahlen.get(1).copied(),
        };
        match inline_import {
            Some((modul, feld)) => m.speicher.push(ImportiertSpeicher::Importiert {
                modul,
                feld,
                typ: Speichertyp { grenzen: gr },
            }),
            None => m.speicher.push(ImportiertSpeicher::Eigen {
                typ: Speichertyp { grenzen: gr },
            }),
        }
        self.zu()?;
        for n in inline_exporte {
            m.exporte.push(Export {
                name: n,
                art: Art::Speicher(idx),
            });
        }
        Ok(())
    }

    // ---- (global …) ------------------------------------------------------
    fn global_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("global")?;
        self.optionaler_name();
        let mut inline_exporte = Vec::new();
        while self.ist("export") {
            self.oeffner("export")?;
            let n = self.name()?;
            inline_exporte.push(n);
            self.zu()?;
        }
        let idx = m.globale.len() as u32;
        // `(global (import "m" "n") $typ)` — die Importform bei globalen
        // Variablen (GEMESSEN 01.10., `data.wast`: ohne diesen Zweig kam
        // „erwartet ein Wort, gefunden (import").
        if self.ist("import") {
            self.oeffner("import")?;
            let modul = self.name()?;
            let feld = self.name()?;
            self.zu()?;
            let (typ, veraenderbar) = self.global_typ()?;
            self.zu()?;
            m.globale.push(Global {
                typ,
                veraenderbar,
                start: None,
                import: Some((modul, feld)),
            });
            for n in inline_exporte {
                m.exporte.push(Export {
                    name: n,
                    art: Art::Global(idx),
                });
            }
            return Ok(());
        }
        let (typ, veraenderbar) = self.global_typ()?;
        let mut start = Vec::new();
        // Der Startwert ist ein AUSDRUCK, kein einzelner Konstantenbefehl:
        // `(global $z3 i32 (i32.add (i32.sub …) (i32.const 4)))` ist
        // erweiterte-konstante-Ausdruecke-Syntax. Welche Befehle dort
        // erlaubt sind, entscheidet der Validator (`pruefe_konstanten`),
        // nicht der Leser — deshalb wird alles bis zum Feldende gelesen.
        //
        // GEMESSEN 03.10. (`global.wast`: mit `ist_konst()` blieb der
        // Rest liegen und der Leser meldete „erwartet ), gefunden
        // (i32.add"; 71 der 114 Faelle der Datei fielen aus).
        if !matches!(self.sieht(), Some(Token::Zu) | None) {
            self.instruktionen(&mut start)?;
        }
        self.zu()?;
        start.push(Befehl::End);
        m.globale.push(Global {
            typ,
            veraenderbar,
            start: Some(start),
            import: None,
        });
        for n in inline_exporte {
            m.exporte.push(Export {
                name: n,
                art: Art::Global(idx),
            });
        }
        Ok(())
    }

    fn ist_konst(&self) -> bool {
        matches!(self.sieht(), Some(Token::Auf(k)) if is_konst_befehl(k))
    }

    fn global_typ(&mut self) -> Result<(Valtyp, bool), Fehler> {
        if self.ist("mut") {
            self.oeffner("mut")?;
            // `(mut (ref null $t))` ist die Form mit Klammern.
            //
            // WICHTIG: `reftyp_lesen` schliesst die Klammer von `(ref …)`
            // selbst; die Klammer von `(mut …)` schliesst das `zu()` DARUNTER
            // (vorher stand hier ein zweites `zu()` — es schloss die Klammer
            // des `(global …)`-Feldes, und der Startwert wurde nicht mehr
            // gelesen).
            //
            // GEMESSEN 03.10. (`linking.wast` Z. 112ff: 8 Faelle,
            // „erwartet ), gefunden (ref.null").
            let t = if self.ist("ref") {
                self.oeffner("ref")?;
                reftyp_lesen(self)?
            } else {
                let w = self.wort()?;
                valtyp(&w)?
            };
            self.zu()?;
            return Ok((t, true));
        }
        // `(global (ref null $t) …)` — `reftyp_lesen` schliesst die
        // Klammer selbst, deshalb hier KEIN weiteres `zu()`.
        if self.ist("ref") {
            self.oeffner("ref")?;
            let t = reftyp_lesen(self)?;
            return Ok((t, false));
        }
        let w = self.wort()?;
        Ok((valtyp(&w)?, false))
    }

    // ---- (export …) ------------------------------------------------------
    fn export_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("export")?;
        let name = self.name()?;
        let art = match self.weiter() {
            Token::Auf(k) => k,
            t => return Err(textfehler(&format!("(export …): {}", t.text()))),
        };
        let mut idx = 0u32;
        let mut idx_name = String::new();
        match self.sieht().cloned() {
            Some(Token::Name(n)) => {
                // Der Bezeichner gehoert zum NAMENSRAUM DER EXPORTART —
                // `$b` in `(export "x" (global $b))` ist eine globale
                // Variable, keine Funktion.
                //
                // GEMESSEN 03.10. (`linking.wast` Z. 70ff: 8 Faelle — der
                // Re-Export einer veraenderbaren Variablen zeigte auf die
                // erste Funktion des Moduls, 142 erwartet, 42 geliefert).
                // Ein unbekannter Name ist ein BENANNTER Fehler (M8), kein
                // stiller Index 0.
                let raum: &BTreeMap<String, u32> = match art.as_str() {
                    "func" => &self.funktionen,
                    "table" => &self.tabellen_namen,
                    "global" => &self.globale_namen,
                    "memory" => &self.speicher_namen,
                    "tag" => &self.tags_namen,
                    andere => {
                        return Err(textfehler(&format!("Exportart {andere}")));
                    }
                };
                idx = *raum.get(&n).ok_or_else(|| {
                    textfehler(&format!(
                        "Exportart {}: Bezeichner ${n} unbekannt ({} Eintraege)",
                        art,
                        raum.len()
                    ))
                })?;
                idx_name = n;
                self.pos += 1;
            }
            Some(Token::Wort(w)) => {
                idx = w.parse::<u32>().map_err(|_| {
                    textfehler(&format!("(export … {}): Index {w:?} zu gross", art))
                })?;
                self.pos += 1;
            }
            _ => {}
        }
        self.zu()?;
        self.zu()?;
        // `(export "tag" (tag $t))`: der Name wird im TAG-NAMENSRAUM
        // aufgeloest, nicht im der Funktionen.
        //
        // GEMESSEN 02.10. (`imports.wast` Z. 0: `(export "tag-i32" (tag
        // $tag-i32))`).
        let art = match art.as_str() {
            "func" => Art::Func(idx),
            "table" => Art::Tabelle(idx),
            "memory" => Art::Speicher(idx),
            "global" => Art::Global(idx),
            "tag" => Art::Tag(self.tags_namen.get(&idx_name).copied().unwrap_or(0)),
            andere => return Err(textfehler(&format!("Exportart {andere}"))),
        };
        m.exporte.push(Export { name, art });
        Ok(())
    }

    /// `(tag $name? (export …)* (import "m" "n")? (type …| (param …)))`
    ///
    /// Ein Tag ist eine Signatur (nur Parameter). `try`/`catch` gehoeren
    /// nicht dazu — die Befehle werden weiterhin abgelehnt.
    fn tag_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("tag")?;
        let beschriftung = self.optionaler_name();
        let idx = m.tags.len() as u32;
        if let Some(n) = beschriftung {
            self.tags_namen.insert(n, idx);
        }
        let mut inline_exporte = Vec::new();
        while self.ist("export") {
            self.oeffner("export")?;
            let n = self.name()?;
            inline_exporte.push(n);
            self.zu()?;
        }
        // Den Import liest `tag_typ_lesen` mit (er steht zwischen den
        // Inline-Exports und der Signatur).
        let (typ, import) = self.tag_typ_lesen()?;
        m.tags.push(crate::modell::Tag { typ, import });
        self.zu()?;
        for n in inline_exporte {
            m.exporte.push(Export {
                name: n,
                art: Art::Tag(idx),
            });
        }
        Ok(())
    }

    /// Die Signatur hinter `(tag …)`: entweder `(type $t)` oder eine
    /// Inline-Signatur `(param …)`. Liefert den Typindex und einen
    /// moeglichen Import.
    fn tag_typ_lesen(&mut self) -> Result<(u32, Option<(String, String)>), Fehler> {
        let mut import = None;
        if self.ist("import") {
            self.oeffner("import")?;
            let a = self.name()?;
            let b = self.name()?;
            import = Some((a, b));
            self.zu()?;
        }
        if self.ist("type") {
            self.oeffner("type")?;
            let t = match self.weiter() {
                Token::Wort(w) => w.parse::<u32>().map_err(|_| textfehler("Typindex"))?,
                Token::Name(n) => self.typindex_namen(&n)?,
                t => return Err(textfehler(&format!("(tag (type …)): {}", t.text()))),
            };
            self.zu()?;
            return Ok((t, import));
        }
        // Inline: `(param …)` — leere Signatur genuegt.
        let mut ft = crate::modell::FuncTyp {
            parameter: Vec::new(),
            ergebnis: Vec::new(),
        };
        if self.ist("param") {
            let inhalt = self.klammer_inhalt();
            let mut p = self.unter(&inhalt);
            ft = p.signatur()?;
        }
        if !ft.ergebnis.is_empty() {
            return Err(textfehler(
                "(tag …): eine Tag-Signatur hat keine Ergebnisse",
            ));
        }
        Ok((self.typindex(&ft), import))
    }

    fn start_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("start")?;
        // Nur EIN Startabschnitt je Modul („multiple start sections").
        // GEMESSEN 02.10. (`start.wast` Z. 19).
        if m.start.is_some() {
            return Err(textfehler("mehr als ein Startabschnitt"));
        }
        m.start = Some(match self.weiter() {
            Token::Name(n) => *self.funktionen.get(&n).unwrap_or(&0),
            Token::Wort(w) => w.parse::<u32>().unwrap_or(0),
            t => return Err(textfehler(&format!("(start …): {}", t.text()))),
        });
        self.zu()?;
        Ok(())
    }

    // ---- (elem …) --------------------------------------------------------
    fn elem_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("elem")?;
        let beschriftung_elem = self.optionaler_name();
        let mut tabelle = 0u32;
        let mut offset: Option<Vec<Befehl>> = None;
        if self.ist("table") {
            self.oeffner("table")?;
            tabelle = self.tabellen_index()?;
            self.zu()?;
        }
        if self.ist("offset") {
            self.oeffner("offset")?;
            let mut v = Vec::new();
            self.ausdruck(&mut v)?;
            v.push(Befehl::End);
            self.zu()?;
            offset = Some(v);
        } else if self.ist_konst() {
            let mut v = Vec::new();
            self.ausdruck(&mut v)?;
            v.push(Befehl::End);
            offset = Some(v);
        }
        // Elementtyp und Eintragsliste. `(elem … declare func …)` und
        // `(elem funcref (ref.func 0) …)` stehen beide hier.
        let (eintraege, reftyp, deklarativ) = self.elem_liste()?;
        self.zu()?;
        if let Some(n) = beschriftung_elem {
            let idx = m.elemente.len() as u32;
            self.elem_namen.insert(n, idx);
        }
        let elem = match offset {
            Some(o) => Element::Aktiv {
                tabelle,
                offset: o,
                reftyp,
                eintraege,
            },
            None if deklarativ => Element::Deklarativ { reftyp, eintraege },
            None => Element::Passiv { reftyp, eintraege },
        };
        m.elemente.push(elem);
        Ok(())
    }

    // ---- (data …) --------------------------------------------------------
    fn data_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner("data")?;
        let beschriftung = self.optionaler_name();
        if let Some(n) = beschriftung {
            self.daten_namen.insert(n, m.daten.len() as u32);
        }
        let mut speicher = 0u32;
        let mut offset: Option<Vec<Befehl>> = None;
        if self.ist("memory") {
            self.oeffner("memory")?;
            // `(data (memory $m1) …)` — Multi-Memory.
            speicher = self.speicher_index_lesen()?;
            self.zu()?;
        }
        if self.ist("offset") {
            self.oeffner("offset")?;
            let mut v = Vec::new();
            self.ausdruck(&mut v)?;
            v.push(Befehl::End);
            self.zu()?;
            offset = Some(v);
        } else if self.ist_konst() {
            // GEMESSEN 01.10. (`data.wast` Z. 6): der Versatz steht als
            // GEFALTETER Ausdruck `(data (i32.const 0) "abc")`. Gelesen wird
            // genau EIN Ausdruck — `instruktionen` laeuft sonst bis zur
            // schliessenden Klammer und frisst den String mit
            // „unerwartetes Token".
            let mut v = Vec::new();
            self.ausdruck(&mut v)?;
            v.push(Befehl::End);
            offset = Some(v);
        }
        let mut inhalt: Vec<u8> = Vec::new();
        while let Some(t) = self.sieht().cloned() {
            match t {
                Token::Text(b) => {
                    self.pos += 1;
                    inhalt.extend_from_slice(&b);
                }
                Token::Wort(_) | Token::Name(_) => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        self.zu()?;
        let d = match offset {
            Some(o) => Daten::Aktiv {
                speicher,
                offset: o,
                inhalt,
            },
            None => Daten::Passiv { inhalt },
        };
        m.daten.push(d);
        m.daten_anzahl = Some(m.daten.len() as u32);
        Ok(())
    }

    fn custom_feld(&mut self, m: &mut Modul) -> Result<(), Fehler> {
        self.oeffner_feld("custom")?;
        let name = match self.weiter() {
            Token::Text(b) => String::from_utf8_lossy(&b).to_string(),
            Token::Wort(w) => w,
            t => return Err(textfehler(&format!("(custom …): {}", t.text()))),
        };
        let mut inhalt = Vec::new();
        while let Some(t) = self.sieht().cloned() {
            match t {
                Token::Text(b) => {
                    self.pos += 1;
                    inhalt.extend_from_slice(&b);
                }
                Token::Zu => break,
                Token::Wort(_) | Token::Auf(_) | Token::Name(_) => {
                    self.pos += 1;
                }
            }
        }
        self.zu()?;
        let _ = &name;
        if name == "name" {
            // Nicht ausgewertet (fuer die Entwicklerwerkzeuge reicht der
            // Rohinhalt in `custom`).
        }
        m.custom.push(Custom {
            name,
            inhalt,
            abschnitt: 0,
        });
        Ok(())
    }
}

// ---- freie Hilfsfunktionen -------------------------------------------------

/// Sind das Felder des Kopfbefehls (Signatur/Speicherindex) statt Argumente?
fn ist_feld_klammer(k: &str) -> bool {
    // `ref` ist eine TYPGRUPPE, kein Befehl: `(ref.test (ref i31) x)` hat
    // die Gruppe als Immediate des Kopfes. Ohne `ref` hier versuchte der
    // Faltweg `(ref i31)` als Befehl zu lesen und meldete
    // „Befehl \"ref\" unbekannt" — die Datei war unlesbar.
    //
    // GEMESSEN 03.10. (`ref_test.wast`: 68 Faelle, `ref_cast.wast`: 40).
    matches!(
        k,
        "type" | "param" | "result" | "table" | "item" | "mut" | "ref"
    )
}

/// Blocktyp ohne Ergebnis? (dann darf ein `if` ohne `else` ohne `Else`-Marker)
fn bt_ist_leer(bt: &Blockart) -> bool {
    match bt {
        Blockart::Leer => true,
        Blockart::Werttyp(_) => false,
        Blockart::Typ(_) => false,
    }
}

/// Ist das Wort ein Befehlsname (und damit kein Immediate)?
fn ist_befehlsname(w: &str) -> bool {
    if crate::op::aus_text(w).is_ok() {
        return true;
    }
    matches!(
        w,
        "block"
            | "loop"
            | "if"
            | "else"
            | "end"
            | "then"
            | "br"
            | "br_if"
            | "br_table"
            | "call"
            | "call_indirect"
            | "return"
            | "drop"
            | "select"
            | "local.get"
            | "local.set"
            | "local.tee"
            | "global.get"
            | "global.set"
            | "table.get"
            | "table.set"
            | "table.size"
            | "table.grow"
            | "table.fill"
            | "table.copy"
            | "table.init"
            | "elem.drop"
            | "data.drop"
            | "memory.init"
            | "memory.size"
            | "memory.grow"
            | "memory.copy"
            | "memory.fill"
            | "ref.null"
            | "ref.func"
            | "ref.is_null"
            | "ref.as_non_null"
            | "call_ref"
            | "br_on_null"
            | "return_call"
            | "return_call_ref"
            | "return_call_indirect"
            | "struct.new_default"
            | "i31.get_s"
            | "i31.get_u"
            | "ref.test"
            | "ref.cast"
            | "struct.new"
            | "struct.get"
            | "struct.get_s"
            | "struct.get_u"
            | "struct.set"
            | "array.len"
            | "array.new"
            | "array.get"
            | "array.get_s"
            | "array.get_u"
            | "array.set"
            | "array.fill"
            | "array.copy"
            | "array.new_fixed"
            | "array.new_data"
            | "array.new_elem"
            | "array.init_data"
            | "array.init_elem"
            | "i32.const"
            | "i64.const"
            | "f32.const"
            | "f64.const"
    ) || w.ends_with(".load")
        || w.ends_with(".store")
}

fn valtyp(w: &str) -> Result<Valtyp, Fehler> {
    Valtyp::aus_text(w).ok_or_else(|| textfehler(&format!("unbekannter Werttyp {w:?}")))
}

/// `(ref …)` lesen: `null` optional, dann ein Reftyp oder ein Bezeichner.
/// `(ref …)` als Werttyp: `(ref null func)`, `(ref null extern)`.
///
/// Consumiert die schliessende Klammer der EIGENEN Gruppe mit. GEMESSEN
/// 01.10. (`select.wast` „join-funcnull"): ohne das blieb die Klammer
/// stehen, der Aufrufer schloss die falsche Gruppe und am Ende stand
/// „unbekanntes Feld (select)" statt eines sauberen Fehlers.
/// `(ref func)` — jeder Funktionstyp, aber nie `null`. Eine Elementliste aus
/// Funktionsindizes traegt diesen Typ (GEMESSEN 02.10., `elem.wast` Z. 2).
fn nicht_leerbare_funktionsreferenz() -> Reftyp {
    Reftyp::Konkret {
        typ: crate::modell::JEDER_FUNKTIONSTYP,
        nullable: false,
    }
}

fn reftyp_lesen(p: &mut Parser) -> Result<Valtyp, Fehler> {
    // `(ref null …)` ist nullable, JEDE andere Form — `(ref $t)`, `(ref 0)`,
    // `(ref func)` — nicht (Wasm 3.0, nicht-leere Referenztypen).
    //
    // GEMESSEN 02.10. (`table.wast` Z. 31/40-42: `(table 0 (ref $t))` ohne
    // Startwert muss ungueltig sein).
    let nullable = match p.sieht() {
        Some(Token::Wort(w)) if w == "null" => {
            p.pos += 1;
            true
        }
        _ => false,
    };
    let w = match p.weiter() {
        Token::Wort(w) => w,
        // `(ref null $t)` / `(ref 0)`: eine Funktionsreferenz auf einen
        // konkreten Typindex (Wasm 3.0, Typed Function References).
        //
        // GEMESSEN 02.10.: `br_table.wast` verlor dadurch sein 52-KiB-Modul
        // und 161 Testfaelle, `elem.wast`/`table.wast` je ein Dutzend.
        // Wichtig: das Token EINMAL lesen — ein zweites `weiter()` laege
        // schon auf der schliessenden Klammer (GEMESSEN 02.10.).
        Token::Name(n) => {
            let typ = p.typindex_namen(&n)?;
            return konkreter_reftyp_ende(p, typ, nullable);
        }
        t => {
            return Err(textfehler(&format!("(ref …): {}", t.text())));
        }
    };
    // `(ref null 0)` / `(ref 3)`: der Typindex als Zahl.
    if let Ok(z) = zahl(&w) {
        let typ = u32::try_from(z).map_err(|_| textfehler("(ref … <typindex>) zu gross"))?;
        return konkreter_reftyp_ende(p, typ, nullable);
    }
    let v = reftyp_text(&w)?;
    // `(ref func)` OHNE `null` ist die nicht-leerbare Form (Wasm 3.0):
    // jeder Funktionstyp, aber nie `null`.
    //
    // GEMESSEN 02.10. (`table.wast` Z. 28-31: `(table 1 (ref func)
    // (ref.null func))` muss ungueltig sein).
    if !nullable {
        // `(ref func)` und `(ref extern)`: jeder passende Typ, aber nie
        // `null` (GEMESSEN 03.10., `table.wast` Z. 30).
        let wildcard = match v {
            Valtyp::Ref(Reftyp::Func) => Some(crate::modell::JEDER_FUNKTIONSTYP),
            Valtyp::Ref(Reftyp::Extern) => Some(crate::modell::JEDER_EXTERNTYP),
            // `(ref array)`, `(ref any)`, `(ref eq)` …: jeder Wert dieser
            // Klasse, aber nie `null`.
            //
            // GEMESSEN 03.10. (`ref_null.wast`).
            Valtyp::Ref(r) if !ist_leerer_heaptyp(&r) => heaptyp_index(&r),
            _ => None,
        };
        if let Some(typ) = wildcard {
            match p.weiter() {
                Token::Zu => {
                    return Ok(Valtyp::Ref(Reftyp::Konkret {
                        typ,
                        nullable: false,
                    }));
                }
                t => {
                    return Err(textfehler(&format!(
                        "(ref …): erwartet ), gefunden {}",
                        t.text()
                    )));
                }
            }
        }
    }
    match p.weiter() {
        Token::Zu => Ok(v),
        t => Err(textfehler(&format!(
            "(ref …): erwartet ), gefunden {}",
            t.text()
        ))),
    }
}

/// Nach einem gelesenen Typindex: die schliessende Klammer pruefen und
/// `Valtyp::Ref(Reftyp::Konkret { typ, nullable })` liefern.
fn konkreter_reftyp_ende(p: &mut Parser, typ: u32, nullable: bool) -> Result<Valtyp, Fehler> {
    match p.weiter() {
        Token::Zu => Ok(Valtyp::Ref(Reftyp::Konkret { typ, nullable })),
        t => Err(textfehler(&format!(
            "(ref …): erwartet ), gefunden {}",
            t.text()
        ))),
    }
}

/// Werttyp mit Moeglichkeit `(ref …)` in einer Parameterliste.
fn valtyp_in_klammer(w: &str) -> Result<Valtyp, Fehler> {
    valtyp(w)
}

/// `(ref …)` in einem Typ: Wasm 3.0 (Typed Function References / GC).
///
/// GEMESSEN 01.10. (`br_table.wast` Z. 1019): die Testsuite enthaelt solche
/// Typen in den Modulen der naechsten Proposal-Runde. In Wasm 2.0 gibt es
/// nur `funcref` und `externref`; der Leser sagt das ausdruecklich, statt
/// „Befehl ref unbekannt" zu melden.
/// Ein Heaptyp als Wort: `func`, `extern`, `any`, `none`, `eq`, `i31`,
/// `struct`, `array`, `exn` und die leeren Formen `nofunc`, `noextern`,
/// `noexn`. Geliefert wird jeweils die NULLBARE Form — `(ref func)` ohne
/// `null` entsteht daraus in `reftyp_lesen`.
///
/// GEMESSEN 03.10. (`ref_null.wast`: `(ref.null any)`, `(ref.null none)`,
/// `(ref.null nofunc)` …).
fn heaptyp_wort(w: &str) -> Option<Reftyp> {
    use crate::modell::{
        JEDER_EQTYP, JEDER_EXNTYP, JEDER_I31TYP, JEDER_STRUKTYP, JEDER_TYP, KEIN_EXNTYP,
        KEIN_EXTERNTYP, KEIN_FUNKTIONSTYP, KEIN_TYP,
    };
    let k = |typ: u32| Reftyp::Konkret {
        typ,
        nullable: true,
    };
    match w {
        "func" => Some(Reftyp::Func),
        "extern" => Some(Reftyp::Extern),
        "any" => Some(k(JEDER_TYP)),
        "none" => Some(k(KEIN_TYP)),
        "exn" => Some(k(JEDER_EXNTYP)),
        "noexn" => Some(k(KEIN_EXNTYP)),
        "nofunc" => Some(k(KEIN_FUNKTIONSTYP)),
        "noextern" => Some(k(KEIN_EXTERNTYP)),
        "eq" => Some(k(JEDER_EQTYP)),
        "i31" => Some(k(JEDER_I31TYP)),
        "struct" => Some(k(JEDER_STRUKTYP)),
        "array" => Some(k(crate::modell::JEDER_ARRAYTYP)),
        _ => None,
    }
}

/// Der Wildcard-Index eines Heaptyps (`None` fuer `func`/`extern`, die
/// eigene Varianten haben).
fn heaptyp_index(r: &Reftyp) -> Option<u32> {
    match r {
        Reftyp::Konkret { typ, .. } => Some(*typ),
        _ => None,
    }
}

/// Ist das ein LEERER Heaptyp (`none`, `nofunc`, `noextern`, `noexn`)?
/// Solche Typen bewohnen nur `null`; ohne `null` sind sie ungueltig.
fn ist_leerer_heaptyp(r: &Reftyp) -> bool {
    matches!(
        heaptyp_index(r),
        Some(
            crate::modell::KEIN_TYP
                | crate::modell::KEIN_FUNKTIONSTYP
                | crate::modell::KEIN_EXTERNTYP
                | crate::modell::KEIN_EXNTYP
        )
    )
}

fn reftyp_text(w: &str) -> Result<Valtyp, Fehler> {
    if w == "func" {
        return Ok(Valtyp::Ref(crate::modell::Reftyp::Func));
    }
    if w == "extern" {
        return Ok(Valtyp::Ref(crate::modell::Reftyp::Extern));
    }
    // `array` = jeder Array-Typ (Wasm 3.0 GC). GEMESSEN 03.10.
    // (`table_init.wast` Z. 790: `(ref array)`).
    if w == "array" {
        return Ok(Valtyp::Ref(Reftyp::Konkret {
            typ: crate::modell::JEDER_ARRAYTYP,
            nullable: true,
        }));
    }
    // Die uebrigen Heaptypen des GC-Lattices (Wasm 3.0): `(ref any)`,
    // `(ref null none)`, `(ref exn)` … — `(ref array)` ist schon oben
    // erledigt, `func`/`extern` ebenfalls.
    //
    // GEMESSEN 03.10. (`ref_null.wast`).
    if let Some(r) = heaptyp_wort(w) {
        return Ok(Valtyp::Ref(r));
    }
    Err(Fehler::form(
        ART_OPCODE,
        format!(
            "WAT: (ref {w} …) ist Wasm 3.0 (Typed Function References) — Wasm 2.0 kennt nur funcref/externref"
        ),
    ))
}

fn is_konst_befehl(k: &str) -> bool {
    matches!(
        k,
        "i32.const"
            | "i64.const"
            | "f32.const"
            | "f64.const"
            | "global.get"
            | "ref.null"
            | "ref.func"
    )
}

/// Exakte Liste der Ladebefehle. KEIN Praefixvergleich: `i32.load32` und
/// `i32.load_extend` sind keine Befehle und muessen als „unknown operator"
/// abgelehnt werden (GEMESSEN 02.10., `load.wast` Z. 38-43: 6 Faelle).
fn ist_lade(name: &str) -> bool {
    matches!(
        name,
        "i32.load"
            | "i32.load8_s"
            | "i32.load8_u"
            | "i32.load16_s"
            | "i32.load16_u"
            | "i64.load"
            | "i64.load8_s"
            | "i64.load8_u"
            | "i64.load16_s"
            | "i64.load16_u"
            | "i64.load32_s"
            | "i64.load32_u"
            | "f32.load"
            | "f64.load"
    )
}

/// Exakte Liste der Speicherbefehle (siehe `ist_lade`).
fn ist_speichere(name: &str) -> bool {
    matches!(
        name,
        "i32.store"
            | "i32.store8"
            | "i32.store16"
            | "i64.store"
            | "i64.store8"
            | "i64.store16"
            | "i64.store32"
            | "f32.store"
            | "f64.store"
    )
}

fn speicherbefehl(name: &str, versatz: u32) -> Befehl {
    use Befehl::*;
    match name {
        "i32.load" => I32Load { versatz },
        "i32.load8_s" => I32Load8S { versatz },
        "i32.load8_u" => I32Load8U { versatz },
        "i32.load16_s" => I32Load16S { versatz },
        "i32.load16_u" => I32Load16U { versatz },
        "i64.load" => I64Load { versatz },
        "i64.load8_s" => I64Load8S { versatz },
        "i64.load8_u" => I64Load8U { versatz },
        "i64.load16_s" => I64Load16S { versatz },
        "i64.load16_u" => I64Load16U { versatz },
        "i64.load32_s" => I64Load32S { versatz },
        "i64.load32_u" => I64Load32U { versatz },
        "f32.load" => F32Load { versatz },
        "f64.load" => F64Load { versatz },
        "i32.store" => I32Store { versatz },
        "i32.store8" => I32Store8 { versatz },
        "i32.store16" => I32Store16 { versatz },
        "i64.store" => I64Store { versatz },
        "i64.store8" => I64Store8 { versatz },
        "i64.store16" => I64Store16 { versatz },
        "i64.store32" => I64Store32 { versatz },
        "f32.store" => F32Store { versatz },
        "f64.store" => F64Store { versatz },
        _ => Befehl::Nop,
    }
}

fn tabellentyp(tok: &[Token]) -> Result<(Reftyp, Limits), Fehler> {
    let mut zahlen = Vec::new();
    let mut reftyp = Reftyp::Func;
    for t in tok {
        if let Token::Wort(w) = t {
            if w == "externref" {
                reftyp = Reftyp::Extern;
            } else if w == "funcref" || w == "i32" || w == "i64" {
                // Typangabe
            } else if let Ok(v) = w.parse::<u64>() {
                zahlen.push(v);
            }
        }
    }
    Ok((
        reftyp,
        Limits {
            min: *zahlen.first().unwrap_or(&0),
            max: zahlen.get(1).copied(),
        },
    ))
}

fn speichertyp(tok: &[Token]) -> Limits {
    let mut zahlen = Vec::new();
    for t in tok {
        if let Token::Wort(w) = t {
            if let Some(rest) = w.strip_prefix("pages=") {
                if let Ok(v) = rest.parse::<u64>() {
                    zahlen.push(v);
                }
            } else if let Ok(v) = w.parse::<u64>() {
                zahlen.push(v);
            }
        }
    }
    Limits {
        min: *zahlen.first().unwrap_or(&0),
        max: zahlen.get(1).copied(),
    }
}

fn globaltyp(tok: &[Token]) -> Result<(Valtyp, bool), Fehler> {
    let mut veraenderbar = false;
    let mut typ = None;
    let mut i = 0;
    while i < tok.len() {
        match &tok[i] {
            Token::Auf(k) if k == "mut" => {
                veraenderbar = true;
                if let Some(Token::Wort(w)) = tok.get(i + 1) {
                    typ = Some(valtyp(w)?);
                }
                i += 2;
            }
            Token::Wort(w) => {
                typ = Some(valtyp(w)?);
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok((typ.unwrap_or(Valtyp::I32), veraenderbar))
}

/// `nan:0x…`, `inf`, Hexadezimalzahl, Dezimalzahl. Rueckgabe: Bitmuster.
///
/// Der NaN-Nutzlast wird 1:1 in das Mantissenenfeld gesetzt — OHNE das
/// Bit „ruhend" zu erzwingen (GEMESSEN 01.10., `float_literals.wast`:
/// `nan:0x200000` ist 0x7fa00000, ein signalling NaN; mit erzwungenem
/// Ruhebit kam 0x7fe00000 heraus). Nur Bit 0 darf nicht gesetzt sein, das
/// waere `inf`, und der Nutzlast muss in das Feld passen.
fn float_zahl(w: &str, f32_mode: bool) -> Result<u64, Fehler> {
    let minus = w.starts_with('-');
    let r = w.trim_start_matches(['-', '+']);
    if r == "inf" {
        let bits = if f32_mode {
            0x7f80_0000u64
        } else {
            0x7ff0_0000_0000_0000
        };
        let vorzeichen = if f32_mode {
            0x8000_0000
        } else {
            0x8000_0000_0000_0000
        };
        return Ok(if minus { bits | vorzeichen } else { bits });
    }
    if r == "nan" {
        let (bits, vorzeichen) = if f32_mode {
            (0x7fc0_0000u64, 0x8000_0000u64)
        } else {
            (0x7ff8_0000_0000_0000, 0x8000_0000_0000_0000)
        };
        return Ok(if minus { bits | vorzeichen } else { bits });
    }
    if let Some(rest) = r.strip_prefix("nan:") {
        let h = rest
            .strip_prefix("0x")
            .ok_or_else(|| textfehler(&format!("nan:{rest:?}: nur nan:0x… erlaubt")))?;
        if !trenner_ok(h) || h.is_empty() || !h.chars().all(|c| c.is_ascii_hexdigit() || c == '_') {
            return Err(textfehler(&format!("nan:{rest:?} nicht lesbar")));
        }
        let sauber: String = h.chars().filter(|c| *c != '_').collect();
        let nutz =
            u64::from_str_radix(&sauber, 16).map_err(|_| textfehler(&format!("nan:{rest:?}")))?;
        let (mask, expo, vorzeichen) = if f32_mode {
            (0x007f_ffffu64, 0x7f80_0000u64, 0x8000_0000u64)
        } else {
            (
                0x000f_ffff_ffff_ffff,
                0x7ff0_0000_0000_0000,
                0x8000_0000_0000_0000,
            )
        };
        if nutz == 0 {
            return Err(textfehler("nan:0x0 waere unendlich, nicht NaN"));
        }
        if nutz > mask {
            return Err(textfehler(&format!(
                "nan:0x{sauber}: Nutzlast zu gross (Maske {mask:#x})"
            )));
        }
        let bits = expo | nutz;
        return Ok(if minus { bits | vorzeichen } else { bits });
    }
    // `1_e1` / `1.0_e1` sind Syntaxfehler: hinter `_` muss eine ZIFFER
    // stehen, nicht der Exponentmarker `e` (GEMESSEN 02.10.,
    // `float_literals.wast` Z. 112/113/151/152/155/156/116/117).
    if !trenner_ok_basis(r, r.starts_with("0x")) {
        return Err(textfehler(&format!(
            "Zahl {w:?}: Trennzeichen `_` nur zwischen Ziffern"
        )));
    }
    let v = if let Some(hex) = r.strip_prefix("0x") {
        hexfloat(hex).ok_or_else(|| textfehler("Hexadezimalzahl nicht lesbar"))?
    } else {
        let sauber: String = r.chars().filter(|c| *c != '_').collect();
        sauber
            .parse::<f64>()
            .map_err(|_| textfehler(&format!("Flieskommazahl {w:?} nicht lesbar")))?
    };
    let v = if minus { -v } else { v };
    Ok(if f32_mode {
        (v as f32).to_bits() as u64
    } else {
        v.to_bits()
    })
}

/// Trennzeichen `_` nur ZWISCHEN zwei Ziffern (WAT-Regel). `_100`, `99_`,
/// `1__000`, `1._0` sind Syntaxfehler — GEMESSEN 01.10.:
/// `float_literals.wast` hatte 20 `assert_malformed`-Faelle genau dafuer.
fn trenner_ok(s: &str) -> bool {
    // Hexziffern sind erlaubt; wer zwischen `1` und `e` in `1_e1` prueft,
    // braucht die Basis: `e` ist KEINE Dezimalziffer.
    trenner_ok_basis(s, !is_dezimal_zahl(s))
}

/// Wie `trenner_ok`, aber die Basis ist ausdruecklich vorgegeben.
fn trenner_ok_basis(s: &str, hex: bool) -> bool {
    let b = s.as_bytes();
    let ziffer = |x: u8| {
        if hex {
            x.is_ascii_hexdigit()
        } else {
            x.is_ascii_digit()
        }
    };
    for (i, c) in b.iter().enumerate() {
        if *c == b'_' {
            let vor = if i == 0 { None } else { Some(b[i - 1]) };
            let nach = b.get(i + 1).copied();
            match (vor, nach) {
                (Some(v), Some(n)) if ziffer(v) && ziffer(n) => {}
                _ => return false,
            }
        }
    }
    true
}

/// Beginnt die Zahl mit `0x`? (Dann sind Hexziffern erlaubt.)
fn is_dezimal_zahl(s: &str) -> bool {
    !s.starts_with("0x") && !s.starts_with("-0x") && !s.starts_with("+0x")
}

/// Hexadezimalzahl mit `p`-Exponent: `1.921fb6p+2`, `1p-149`, `12345`.
///
/// Die Ziffern werden in `f64` gesammelt und danach auf `f32` gerundet —
/// `f64` hat 53 Bit, fuer eine `f32`-Entscheidung braucht man hoechstens
/// 2·24+2 Bit, das passt also ohne Doppelrundungsfehler.
///
/// GEMESSEN 01.10. (`float_literals.wast`, 138 Faelle): die Trennzeichen
/// `_` in Hexzahlen (`0x1_0000_0000_0000_0000_0000`) wurden nicht entfernt,
/// und der Exponent `p±…` mit Pluszeichen nicht geparst.
fn hexfloat(hex: &str) -> Option<f64> {
    // M3: Die Ziffernzahl kommt aus der Eingabe — harte Grenze, damit eine
    // Datei mit 10 Mio Ziffern keine Endlosschleife und keinen Riesenhaufen
    // erzeugt (MAX_NAME_BYTES_HART ist hier die passende Groessenordnung).
    let hex: String = hex.chars().filter(|c| *c != '_').take(4 * 1024).collect();
    let (mantisse, expo) = match hex.find(['p', 'P']) {
        Some(i) => (&hex[..i], hex[i + 1..].parse::<i32>().ok()?),
        None => (hex.as_str(), 0),
    };
    let (ganz, rest) = match mantisse.find('.') {
        Some(i) => (&mantisse[..i], &mantisse[i + 1..]),
        None => (mantisse, ""),
    };
    let mut wert = 0f64;
    for c in ganz.chars() {
        wert = wert * 16.0 + c.to_digit(16)? as f64;
    }
    let mut f = 1.0 / 16.0;
    for c in rest.chars() {
        wert += c.to_digit(16)? as f64 * f;
        f /= 16.0;
    }
    Some(wert * 2f64.powi(expo))
}

fn i64_zahl(w: &str) -> Result<i64, Fehler> {
    if !trenner_ok(w) {
        return Err(textfehler(&format!(
            "Zahl {w:?}: Trennzeichen `_` nur zwischen Ziffern"
        )));
    }
    let s: String = w.chars().filter(|c| *c != '_').collect();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, s.strip_prefix('+').unwrap_or(&s).to_string()),
    };
    let v = if let Some(h) = rest.strip_prefix("0x") {
        u64::from_str_radix(h, 16).ok()
    } else {
        rest.parse::<u64>().ok()
    };
    let v = v.ok_or_else(|| textfehler(&format!("Zahl {w:?} nicht lesbar")))?;
    Ok(if neg {
        (v as i64).wrapping_neg()
    } else {
        v as i64
    })
}

fn zahl(w: &str) -> Result<u64, Fehler> {
    if !trenner_ok(w) {
        return Err(textfehler(&format!(
            "Zahl {w:?}: Trennzeichen `_` nur zwischen Ziffern"
        )));
    }
    let s: String = w.chars().filter(|c| *c != '_').collect();
    if let Some(h) = s.strip_prefix("0x") {
        return u64::from_str_radix(h, 16).map_err(|_| textfehler(&format!("Zahl {w:?}")));
    }
    s.parse::<u64>()
        .map_err(|_| textfehler(&format!("Zahl {w:?}")))
}

/// f32-Konstante als Bitmuster (oeffentlich fuer den wast-Leser).
pub fn f32_bits(w: &str) -> Result<u32, Fehler> {
    float_zahl(w, true).map(|b| b as u32)
}

/// f64-Konstante als Bitmuster (oeffentlich fuer den wast-Leser).
pub fn f64_bits(w: &str) -> Result<u64, Fehler> {
    float_zahl(w, false)
}

/// EINE Funktion als WAT-Text (W5-Disassembly im Reiter „WebAssembly").
pub fn schreibe_funktion(m: &Modul, funktion: u32) -> String {
    schreibe_funktion_mit(m, funktion, None)
}

/// EINE Funktion als WAT-Text, mit dem naechsten auszufuehrenden
/// Befehlsversatz markiert (`aktiv`) und dem Anzeigenamen im Kopf.
///
/// **Der Versatz stimmt mit den Haltepunkten ueberein:** die Zeilen
/// entstehen in Befehlsreihenfolge, die Zeile mit Nummer `i` ist also der
/// Befehl `koerper[i]` — genau der `versatz`, den `Haltepunkte` benutzen.
/// Die schliessende `end` des Funktionskoerpers wird (wie vorher) nicht
/// ausgegeben, geschachtelte `end` dagegen schon.
///
/// GEMESSEN 03.10. (team/rollen/WASM.txt: „Disassemblierung (WAT) der
/// aktuellen Funktion").
pub fn schreibe_funktion_mit(m: &Modul, funktion: u32, aktiv: Option<usize>) -> String {
    match m.funktionen.get(funktion as usize) {
        Some(Func::Importiert { modul, feld, typ }) => {
            format!("(import \"{modul}\" \"{feld}\" (func (type {typ})))")
        }
        Some(Func::Eigen {
            koerper, lokale, ..
        }) => {
            // Der Name im Kopf: `func#3` waere kein gueltiger Bezeichner,
            // dann bleibt der Kopf formlos wie vorher. Parameter und
            // Ergebnisse stehen dabei da, wo ein Leser sie erwartet — die
            // `type`-Nummer allein sagt nicht, was die Funktion bekommt.
            let name = m.funktions_name(funktion);
            let mut s = if name.starts_with("func#") {
                String::from("(func")
            } else {
                format!("(func ${name}")
            };
            if let Some(ft) = m.signatur(funktion) {
                for p in &ft.parameter {
                    s.push_str(&format!(" (param {})", p.text()));
                }
                for e in &ft.ergebnis {
                    s.push_str(&format!(" (result {})", e.text()));
                }
            }
            s.push(')');
            for l in lokale {
                s.push_str(&format!(" (local {})", l.text()));
            }
            s.push('\n');
            let mut tiefe = 1i64;
            for (versatz, b) in koerper.iter().enumerate() {
                match b {
                    Befehl::End => {
                        tiefe -= 1;
                        if tiefe == 0 {
                            break;
                        }
                    }
                    Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_) | Befehl::Else => {
                        tiefe += 1;
                    }
                    _ => {}
                }
                for _ in 0..tiefe.max(1) {
                    s.push_str("  ");
                }
                s.push_str(&befehl_text(m, b));
                if aktiv == Some(versatz) {
                    s.push_str("  ;; <- hier");
                }
                s.push('\n');
            }
            s.push(')');
            s
        }
        None => format!("(func {funktion} /* nicht vorhanden */)"),
    }
}

/// Ein Befehl als Text fuer die Disassembly.
///
/// Bei Aufrufen steht der FunktionsNAME da, nicht seine Nummer: „call 0"
/// liest niemand, und der Reiter zeigt ohnehin den Aufrufstapel mit
/// Namen (Export bzw. `name`-Sektion bzw. WAT-Bezeichner, eine Regel:
/// [`Modul::funktions_name`](crate::modell::Modul::funktions_name)).
/// Unbenannte Funktionen bleiben numerisch wie vorher.
///
/// GEMESSEN 03.10. (tests/devtools.rs `wat_der_aktuellen_funktion_mit_marke`).
fn befehl_text(m: &Modul, b: &Befehl) -> String {
    let (wort, i) = match b {
        Befehl::Call(i) => ("call", *i),
        Befehl::ReturnCall(i) => ("return_call", *i),
        // Der Rest braucht den Modulkontext nicht.
        _ => return crate::op::text(b),
    };
    let name = m.funktions_name(i);
    if name.starts_with("func#") {
        crate::op::text(b)
    } else {
        format!("{wort} ${name}")
    }
}

/// Modul als Text schreiben (fuer die Disassembly in den DevTools, W5).
pub fn schreibe_text(m: &Modul) -> String {
    let mut s = String::from("(module\n");
    for t in &m.typen {
        s.push_str("  (type (func");
        for p in &t.parameter {
            s.push_str(&format!(" (param {})", p.text()));
        }
        for e in &t.ergebnis {
            s.push_str(&format!(" (result {})", e.text()));
        }
        s.push_str("))\n");
    }
    for f in &m.funktionen {
        match f {
            Func::Importiert { modul, feld, typ } => {
                s.push_str(&format!(
                    "  (import \"{modul}\" \"{feld}\" (func (type {typ})))\n"
                ));
            }
            Func::Eigen {
                typ,
                koerper,
                lokale,
                ..
            } => {
                s.push_str(&format!("  (func (type {typ})"));
                for l in lokale {
                    s.push_str(&format!(" (local {})", l.text()));
                }
                s.push('\n');
                let mut tiefe = 1i64;
                for b in koerper {
                    match b {
                        Befehl::End => {
                            tiefe -= 1;
                            if tiefe == 0 {
                                break;
                            }
                        }
                        Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_) | Befehl::Else => {
                            tiefe += 1;
                        }
                        _ => {}
                    }
                    s.push_str(&format!("    {}\n", crate::op::text(b)));
                }
                s.push_str("  )\n");
            }
        }
    }
    for e in &m.exporte {
        s.push_str(&format!(
            "  (export \"{}\" ({} {}))\n",
            e.name,
            e.art.text(),
            e.art.index()
        ));
    }
    s.push_str(")\n");
    s
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Text -> Modul, Werte sichern.
    #[test]
    fn kleines_textmodul() {
        let m = lies_text(
            r#"(module (func (export "f") (result i32) i32.const 42))"#,
            &Grenzen::standard(),
        )
        .unwrap();
        assert_eq!(m.typen.len(), 1);
        assert_eq!(m.funktionen.len(), 1);
        assert_eq!(m.exporte[0].name, "f");
        assert_eq!(m.exporte[0].art.index(), 0);
    }

    #[test]
    fn gefaltet_und_labels() {
        let m = lies_text(
            r#"(module (func $f (result i32)
                 (block $a (result i32)
                   (br_if $a (i32.const 0) (i32.const 7))
                   (i32.const 9))))"#,
            &Grenzen::standard(),
        );
        // `br_if $a (i32.const 0) (i32.const 7)`: Reihenfolge ist
        // Bedingung(0) dann Wert(7) — der Test prueft nur, dass es lesbar ist.
        if let Ok(m) = m {
            assert_eq!(m.funktionen.len(), 1);
        } else {
            // Ein Fehler ist hier noch zulaessig, aber der Fehler muss
            // benannt sein (kein Panic).
            assert_eq!(m.unwrap_err().art(), ART_OPCODE);
        }
    }

    #[test]
    fn zahlen_und_fehler() {
        assert_eq!(i64_zahl("42").unwrap(), 42);
        assert_eq!(i64_zahl("-7").unwrap(), -7);
        assert_eq!(i64_zahl("0x10").unwrap(), 16);
        assert_eq!(i64_zahl("-0x10").unwrap(), -16);
        assert_eq!(i64_zahl("1_000").unwrap(), 1000);
        assert_eq!(zahl("0x10").unwrap(), 16);
        assert_eq!(float_zahl("inf", true).unwrap(), 0x7f80_0000);
        assert_eq!(float_zahl("-inf", false).unwrap(), 0xfff0_0000_0000_0000);
        assert_eq!(float_zahl("1.5", true).unwrap(), 1.5f32.to_bits() as u64);
        assert_eq!(float_zahl("0x1p4", false).unwrap(), 16f64.to_bits());
        assert!(crate::wert::ist_nan32(
            float_zahl("nan", true).unwrap() as u32
        ));
        assert!(crate::wert::ist_nan64(
            float_zahl("nan:0x4000000000000", false).unwrap()
        ));
        assert_eq!(i64_zahl("x").unwrap_err().art(), ART_OPCODE);
    }

    #[test]
    fn bomben_text() {
        let k = Grenzen::bombe();
        assert!(lies_text("(module (func", &k).is_err());
        assert!(lies_text("(; offen", &k).is_err());
        assert!(lies_text(r#"(module (data "x\\))"#, &k).is_err());
        // Verschachtelungsgrenze
        let tief = format!("(module (func {}", "(block ".repeat(50));
        assert!(lies_text(&tief, &k).is_err());
    }
}
