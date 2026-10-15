// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Parser (J2): rekursiver Abstieg, ES2020-Naeherung. Tiefengrenze (M5),
//! Fehler mit Zeile/Spalte. `function*`/Generatoren und BigInt lehnt der
//! Lexer/Parser mit benanntem Fehler ab (dokumentierte Luecken).

use crate::ast::*;
use crate::fehler::JsFehler;
use crate::grenzen::MAX_PARSER_TIEFE_HART;
use crate::lex::{Schluessel, Token, TokenArt, Zeichen};

pub(crate) struct Parser {
    /// Der zuletzt gelesene Primarausdruck stand in Klammern. Wird fuer
    /// die JS-Regel `(-8) ** 2` erlaubt, `-8 ** 2` nicht (GEMESSEN:
    /// `(-8) ** (1/3)` wurde faelschlich abgelehnt).
    geklammert: bool,
    tokens: Vec<Token>,
    pos: usize,
    tiefe: u32,
    max_tiefe: u32,
    /// J10 (ES-Module): `Some`, wenn die Quelle als **Modul** gelesen wird.
    /// In einem normalen Skript bleiben `import`/`export` ein
    /// Syntaxfehler — so ist es in node auch, und so unterscheidet der
    /// Host `<script type="module">` von einem klassischen `<script>`.
    modul: Option<crate::ast::Modul>,
    /// J32: dieselbe Tiefe, aber fuer `super()` — und sie wird von einem
    /// **Platzhalter** ersetzt statt dekrementiert, wenn eine **Klasse**
    /// beginnt.
    ///
    /// GEMESSEN, warum ein eigener Zaehler und nicht derselbe:
    /// `ContainsSuperCall` (ES2023 15.2.1) rekursiert durch **verschachtelte
    /// Funktionen** — `class C extends O { x = function(){ return super() } }`
    /// ist ein SyntaxError. Bei `arguments` ist es umgekehrt: dort hebt eine
    /// verschachtelte Funktion das Verbot **auf**, weil sie eine eigene
    /// Bindung hat. Ein Zaehler fuer beides kann nicht zwei Regeln
    /// beschreiben, die entgegengesetzt sind.
    ///
    /// Und was eine verschachtelte **Klasse** angeht, gemessen (node 22):
    ///   `class C extends O { x = class extends P { constructor(){ super() } } }`
    ///   -> **gueltig** — der Konstruktor einer inneren Klasse darf
    ///   `super()` natuerlich. Deshalb wird hier beim Betreten einer
    ///   Klasse **auf 0 gesetzt** (und danach wiederhergestellt), und
    ///   nicht dekrementiert.
    feld_super: u32,
    super_erlaubt: bool,
    /// J32: wie tief ist der Parser gerade in einem
    /// **Klassenfeld-Initialisierer**? Dort ist `arguments` als Bezeichner
    /// ein Syntaxfehler (ES2023 15.2.1, `FieldDefinition`: „It is a Syntax
    /// Error if ContainsArguments of Initializer is true").
    ///
    /// Ein **Zaehler** und kein Flag, weil sich Felder verschachteln
    /// koennen: GEMESSEN (node 22)
    /// `class C { x = class { y = arguments } }` -> SyntaxError.
    /// Mit einem Flag wuerde das innere `class` es zuruecksetzen und die
    /// aeussere Pruefung verlieren.
    feld_initialisierer: u32,
    /// J32: die im **laufenden Klassenkörper** benutzten privaten Namen
    /// (ES2023 15.7.1 `AllPrivateNamesValid`).
    ///
    /// GEMESSEN (node 22), `var C = class { f = (() => this)().#x };` ->
    /// SyntaxError: `#x` ist **benutzt**, aber nirgends deklariert. Ohne
    /// diese Pruefung nimmt der Motor den Ausdruck an und scheitert spaeter
    /// zur Laufzeit — die Suite hat **18** solche Faelle.
    ///
    /// ZWEI Buecher, weil die Reihenfolge nicht reicht: ein Element darf
    /// `#x` benutzen, das ein **spaeter** deklariertes Element erst einfuehrt
    /// (`m(){ return #x } #x;`). Deshalb wird am **Ende** des Rumpfes
    /// geprueft, nicht beim Benutzen.
    ///
    /// Ein **verschachtelter Klassenkoerper** bekommt eine eigene/private
    /// Umgebung (spec Schritt 6/8), deshalb werden beide Buecher beim
    /// Betreten einer Klasse **gesichert und leer neu angelegt** — und nach
    /// dem Rumpf wiederhergestellt. Eine verschachtelte **Funktion** setzt
    /// nichts zurueck: ihre private Umgebung ist die des umgebenden
    /// Klassenkörpers.
    privat_ausgelieht: Vec<String>,
    /// J32: die im **laufenden Klassenkörper** **deklarierten** privaten
    /// Namen (`#x`, `#m() {}`, `get #x(){}`, `static #x`).
    privat_vergeben: Vec<String>,
    /// J32: wird gerade der **Elternausdruck** (`class C extends …`)
    /// uebersetzt? Dann ist ein **privater Name** verboten.
    ///
    /// ES2023 15.7.1 (`ClassHeritage : extends ClassExpression`, Static
    /// Semantics: Early Errors): „It is a Syntax Error if ContainsPrivate
    /// of ClassExpression is true." `ContainsPrivate` ist rein
    /// **syntaktisch** und rekursiert durch alle Produktionen.
    ///
    /// GEMESSEN (node 22) — **fuenf** scheitern, **drei** sind gueltig:
    ///   `class C extends (function(){ return this.#x }) {}` -> SyntaxError
    ///   `class C extends (class { m(){ return this.#y } }) {}` -> SyntaxError
    ///   `class C extends (class { #x; m(){ return #x } }) {}` -> SyntaxError
    ///   `class C extends (() => { class { #y; … } return 1 }) {}` -> SyntaxError
    ///   `class C extends (#x in {}) {}`                     -> SyntaxError
    ///   `class C extends (class { #x }) {}`                 -> **gueltig**
    ///   `class C extends (function(){ return 1 }) {}`       -> **gueltig**
    ///   `class C extends (() => 1) {}`                      -> **gueltig**
    ///
    /// DER UNTERSCHIED, DER DIE REGEL TRENNT: `class { #x }` ist
    /// **gueltig** — das ist eine **Deklaration** (`PrivateBoundIdentifier`)
    /// und kein `PrivateIdentifier`. Der Motor muss deshalb an den
    /// **Referenz**-Stellen pruefen (`.#name` und `#name in`), nicht an der
    /// Deklarationsstelle im Klassenrumpf; dort waere sonst jede private
    /// Felddeklaration in einer verschachtelten Klasse verboten.
    ///
    /// Und weil die Regel **syntaktisch** ist, wird sie in einer
    /// verschachtelten Klasse **nicht** zurueckgenommen — der dritte
    /// Messfall ist genau das und **scheitert** in node.
    privat_verboten: bool,
    /// J32: steht der Parser gerade im **Operand von `delete`**?
    ///
    /// ES2023 13.5.1 (`UnaryExpression : delete UnaryExpression`, Static
    /// Semantics: Early Errors): ist der `UnaryExpression` in einer
    /// `MemberExpression . PrivateName` enthalten, ist es ein Syntaxfehler.
    ///
    /// GEMESSEN (node 22) — **sechs** scheitern, und **nicht** nur im
    /// Klassenfeld-Initialisierer (der Testname sagt es, die Regel nicht):
    ///   `class C { #x; g = delete this.#x }`               -> SyntaxError
    ///   `class C { #x; #g = delete this.#x }`             -> SyntaxError
    ///   `class C { #x; static g = delete this.#x }`       -> SyntaxError
    ///   `class C { #x; m(){ return delete this.#x } }`    -> SyntaxError
    ///   `class C { #x; m = function(){ return delete this.#x } }`
    ///                                                        -> SyntaxError
    ///   `class C { #x; m(){ return delete this.#y } }`    -> SyntaxError
    /// und **gueltig** sind vier, darunter der direkte Nachbar:
    ///   `class C { #x; g = delete this.y }`               -> ok
    ///   `class C { #x; m(){ return this.#x } }`           -> ok
    ///   `class C { #x; g = typeof this.#x }`              -> ok
    ///   `class C { #x; g = delete C.prototype }`          -> ok
    ///
    /// Es ist ein **Zaehler**, weil `delete` sich verschachteln kann
    /// (`delete delete a.#x`) und weil die verschachtelte
    /// `function`-Form gueltig bleibt, **wenn** kein `delete` im Spiel ist.
    /// J32: wird gerade der **Operand eines unaryaeren Operators**
    /// uebersetzt? Dort ist `yield` im Generator verboten (ES2023 15.5:
    /// `YieldExpression` ist keine `UnaryExpression`).
    unaer_operand: bool,
    delete_pfad: u32,
    /// J42: stehen wir **im Rumpf einer Funktion**? `return` ist nur dort
    /// erlaubt (ES2023 14.10 `ReturnStatement`, Early Errors: „It is a
    /// Syntax Error if this ReturnStatement is not contained in the
    /// FunctionBody of a function definition").
    ///
    /// GEMESSEN (node 22), vier Fehlerfaelle und fuenf Gegenproben:
    ///   `return;`                        -> SyntaxError
    ///   `var x=1; return x;`             -> SyntaxError
    ///   `try { return 1 } catch(e){…}`   -> SyntaxError  (immer noch oben!)
    ///   `eval('return 1')`               -> SyntaxError
    ///   `function f(){ return 1 }`       -> ok
    ///   `function f(){ if (1) { return 2 } }` -> ok
    ///   `class C { m(){ return 1 } }`    -> ok
    ///   `var f = () => { return 5 }`     -> ok
    ///   `var o = { m(){ return 7 } }`    -> ok
    in_funktionsrumpf: bool,
    /// J45: steht der `[Await]`-Kontext (async-Funktion, async-Methode,
    /// async-Generator)? Dann ist `await` **kein** Bindungsname (ES2023
    /// 13.1.1) — siehe `schluessel_als_bindung_verboten`.
    in_async: bool,
    /// J32: steht der Parser gerade im Rumpf eines **Generators**? Dort
    /// ist `yield` der Operator (also erlaubt) und zugleich als
    /// **Bindung** verboten (ES2023 14.4.1, `BindingIdentifier : Identifier`:
    /// „It is a Syntax Error if this production has a [Yield] parameter and
    /// StringValue of Identifier is \"yield\"").
    ///
    /// GEMESSEN (node 22): `class C { *#gen(){ var yield } }` und
    /// `function* g(){ var yield }` sind SyntaxError,
    /// `class C { *gen(){ yield 1 } }` ist gueltig. Es sind **zwei**
    /// Regeln mit entgegengesetzter Richtung, und beide brauchen je ein
    /// Flag — ein Flag fuer „Klasse" allein entscheidet eine davon falsch.
    /// J32: steht der Parser gerade in der **Parameterliste eines
    /// Generators**? Dort ist `yield` als **Ausdruck** ein Syntaxfehler.
    ///
    /// ES2023 15.4.1: `GeneratorDeclaration : function * BindingIdentifier (
    /// FormalParameters ) { GeneratorBody }` — die Parameterliste ist ein
    /// `[Yield]`-Kontext, aber ein `YieldExpression` steht dort nicht zur
    /// Verfuegung; nur der Rumpf nimmt yield-Werte an.
    ///
    /// GEMESSEN (node 22), **sieben** von zwoelf scheitern:
    ///   `function* g(a = yield) {}`    `function* g(a = yield 1) {}`
    ///   `function* g({a = yield}) {}`  `function* g(a = yield, b) {}`
    ///   `function* g(a = (yield)) {}`  `var o = { *g(a = yield) {} }`
    ///   `class C { *g(a = yield) {} }`
    /// und **fuenf** sind gueltig — zwei davon grenzen das Flag ab:
    ///   `function f(a = yield) {}` -> ok (kein Generator)
    ///   `function* g(a = function(){ return yield }) {}` -> ok, denn der
    ///   Rumpf einer **verschachtelten Funktion** ist kein Generatorrumpf.
    ///
    /// GEMESSEN, warum ein **eigenes** Flag und nicht `in_generator`: im
    /// Rumpf ist `yield` erlaubt und benutzt **denselben** Zweig des
    /// Parsers. Am `yield`-Zweig laesst sich also nicht unterscheiden — nur an
    /// der Stelle, von der man hinkommt.
    in_generator_param: bool,
    in_generator: bool,
    /// J32: steht der Parser gerade **in einer Klasse**? Klassen sind
    /// strikter Code (ES2023 10.2.1), und dort ist `yield` als
    /// `IdentifierReference` ein Syntaxfehler (ES2023 12.1.1).
    ///
    /// GEMESSEN (node 22): `var C = class yield {}` und
    /// `class C extends yield {}` scheitern, `class { yield(){} }` nicht —
    /// ein Methoden**name** ist ein `PropertyName`, kein Bezeichner. Der
    /// Wert wird in `parse_klasse_rumpf` gesetzt und danach
    /// **zurueckgesetzt**, weil Klassen verschachtelt sein koennen.
    in_klasse: bool,
    /// J27: steht der umgebende Code im **Strict-Modus**? `"use strict"`
    /// wird in `lesen` gesetzt, sobald die Direktive gesehen ist — also
    /// **vor** jeder danach kommenden Anweisung.
    ///
    /// GEMESSEN, warum das Feld noetig ist (und nicht ein Blick in den
    /// Quelltext): die Early Error fuer doppelte Parameternamen haengt
    /// davon ab (ES2023 15.2.1, `FormalParameters`):
    ///   `function h(a, a) {}`                     -> erlaubt (sloppy)
    ///   `function h(a, a = 1) {}`                -> SyntaxError
    ///   `"use strict"; function h(a, a) {}`      -> SyntaxError
    ///   `async function f(a, a) {}`              -> erlaubt (sloppy)
    /// Die Direktive steht im **Rumpf einer anderen Funktion**, nicht im
    /// Parameterbereich — dafueuer braucht es den Zustand.
    strikt: bool,
    /// J19: der Quelltext in **Zeichen** (nicht Bytes — die Spalten des
    /// Lexers zaehlen Zeichen) und der Zeilenbeginn je Zeilennummer.
    ///
    /// GEMESSEN, warum eine Zeilentabelle und keine Offsets im Token:
    /// die Token tragen `zeile`/`spalte`, aber keine Byte-Position; eine
    /// Position im Token zu ergaenzen hiesse den Lexer an 13 Stellen zu
    /// aendern. Die Tabelle entsteht einmal pro Parse und kostet
    /// `Zeilen + 1` Eintraege.
    quelle: Vec<char>,
    zeilen_start: Vec<usize>,
}

impl Parser {
    pub(crate) fn neu(tokens: Vec<Token>, max_tiefe: u32) -> Self {
        Parser::mit_quelle(tokens, max_tiefe, "")
    }

    /// J19: wie `neu`, aber mit dem Quelltext (fuer `toString`).
    pub(crate) fn mit_quelle(tokens: Vec<Token>, max_tiefe: u32, quelle: &str) -> Self {
        let zeichen: Vec<char> = quelle.chars().collect();
        let mut zeilen_start = vec![0usize];
        for (i, c) in zeichen.iter().enumerate() {
            if *c == '\n' {
                zeilen_start.push(i + 1);
            }
        }
        Parser {
            tokens,
            pos: 0,
            tiefe: 0,
            max_tiefe: max_tiefe.min(MAX_PARSER_TIEFE_HART).max(16),
            geklammert: false,
            modul: None,
            unaer_operand: false,
            in_generator_param: false,
            privat_verboten: false,
            privat_ausgelieht: Vec::new(),
            privat_vergeben: Vec::new(),
            delete_pfad: 0,
            in_funktionsrumpf: false,
            in_async: false,
            feld_initialisierer: 0,
            feld_super: 0,
            super_erlaubt: false,
            in_klasse: false,
            in_generator: false,
            strikt: false,
            quelle: zeichen,
            zeilen_start,
        }
    }

    /// J19: Zeichen-Offset von (Zeile, Spalte), 1-basiert wie der Lexer.
    fn offset_von(&self, zeile: u32, spalte: u32) -> usize {
        let z = (zeile.max(1) - 1) as usize;
        let basis = match self.zeilen_start.get(z) {
            Some(b) => *b,
            None => return self.quelle.len(),
        };
        (basis + spalte.max(1) as usize - 1).min(self.quelle.len())
    }

    /// J19: der Quelltext der Token `[von, bis)`.
    ///
    /// `bis` ist der Index **nach** dem letzten Token; steht dort noch
    /// ein Token, endet die Funktion vor dessen Anfang, sonst am Ende der
    /// Quelle.
    fn quelle_von_bis(&self, von: usize, bis: usize) -> Option<std::sync::Arc<str>> {
        if self.quelle.is_empty() || von >= bis {
            return None;
        }
        let von = von.min(self.tokens.len().saturating_sub(1));
        let ende = match self.tokens.get(bis) {
            Some(t) => self.offset_von(t.zeile, t.spalte),
            None => self.quelle.len(),
        };
        let start = self.offset_von(self.tokens[von].zeile, self.tokens[von].spalte);
        if ende <= start || ende > self.quelle.len() {
            return None;
        }
        let text: String = self.quelle[start..ende].iter().collect();
        // Trailing Whitespace abschneiden (ein `;` nach der Funktion
        // gehoert nicht dazu).
        let text = text.trim_end().to_string();
        if text.is_empty() {
            None
        } else {
            Some(std::sync::Arc::from(text))
        }
    }

    /// J10: als **Modul** lesen (`import`/`export` erlaubt).
    pub(crate) fn als_modul(mut self) -> Self {
        self.modul = Some(crate::ast::Modul::default());
        self
    }

    fn fehler(&self, msg: String) -> JsFehler {
        let t = self.aktuell();
        JsFehler::Syntax {
            zeile: t.zeile,
            spalte: t.spalte,
            nachricht: msg,
        }
    }

    fn aktuell(&self) -> &Token {
        // Das letzte Token ist immer `Ende` (Lexer garantiert es).
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn weiter(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn ist_ende(&self) -> bool {
        matches!(self.aktuell().art, TokenArt::Ende)
    }

    fn ist_zeichen(&self, z: Zeichen) -> bool {
        matches!(&self.aktuell().art, TokenArt::Zeichen(x) if *x == z)
    }

    fn ist_wort(&self, w: Schluessel) -> bool {
        matches!(&self.aktuell().art, TokenArt::Wort(x) if *x == w)
    }

    fn frisst_zeichen(&mut self, z: Zeichen) -> bool {
        if self.ist_zeichen(z) {
            self.weiter();
            true
        } else {
            false
        }
    }

    fn frisst_wort(&mut self, w: Schluessel) -> bool {
        if self.ist_wort(w) {
            self.weiter();
            true
        } else {
            false
        }
    }

    fn erwartet_zeichen(&mut self, z: Zeichen, was: &str) -> Result<(), JsFehler> {
        if self.frisst_zeichen(z) {
            Ok(())
        } else {
            Err(self.fehler(format!("erwartet: {was}")))
        }
    }

    /// ASI: Strichpunkt, Umbruch, `}` oder Ende beendet die Anweisung.
    fn erwartet_semi(&mut self) -> Result<(), JsFehler> {
        if self.frisst_zeichen(Zeichen::Semikolon) {
            return Ok(());
        }
        let t = self.aktuell();
        if t.umbruch_davor
            || matches!(t.art, TokenArt::Ende)
            || matches!(t.art, TokenArt::Zeichen(Zeichen::RGeschweift))
        {
            return Ok(());
        }
        // J22: **ASI-Regel 1b der Spec (12.9.1)**: ein Semikolon wird auch
        // dann eingefuegt, wenn das **vorherige** Token `}` ist — ganz ohne
        // Zeilenumbruch. GEMESSEN/GEFUNDEN an `j16_bundel.rs`
        // (`yield_mit_feld_und_objektliteral`): mit dem nackten IIFE
        // (`function* g(){ … } g().next()`) ist der Generator nun ein
        // **Ausdruck**, und der Ausdruck endet auf `}`; ohne diese Regel
        // meldete der Motor `Syntaxfehler 1:32: erwartet: ';'`. node 22
        // laesst die Zeile durch.
        if self.pos > 0
            && self
                .tokens
                .get(self.pos - 1)
                .is_some_and(|v| matches!(v.art, TokenArt::Zeichen(Zeichen::RGeschweift)))
        {
            return Ok(());
        }
        Err(self.fehler("erwartet: ';'".to_string()))
    }

    /// Tiefenzaehler fuer M5: verschachtelte Strukturen sind begrenzt.
    fn tief(&mut self) -> Result<TiefenWacht, JsFehler> {
        if self.tiefe >= self.max_tiefe {
            return Err(self.fehler(format!(
                "Verschachtelung zu tief (Grenze {})",
                self.max_tiefe
            )));
        }
        self.tiefe += 1;
        Ok(TiefenWacht)
    }

    fn aktuell_bezeichner(&self) -> Option<String> {
        match &self.aktuell().art {
            TokenArt::Bezeichner(n) | TokenArt::BezeichnerEscape(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// J10: ein Bezeichner als **Pflicht** (fuer `import`/`export`), mit
    /// genannter Erwartung im Fehlertext.
    fn bezeichner(&mut self, erwartung: &str) -> Result<String, JsFehler> {
        match self.aktuell().art.clone() {
            TokenArt::Bezeichner(n) | TokenArt::BezeichnerEscape(n) => {
                self.weiter();
                Ok(n)
            }
            TokenArt::Wort(w) => {
                let t = wort_text(w).to_string();
                self.weiter();
                Ok(t)
            }
            _ => Err(self.fehler(format!("{erwartung} erwartet"))),
        }
    }

    fn frisst_bezeichner(&mut self) -> Option<String> {
        let n = self.aktuell_bezeichner()?;
        self.weiter();
        Some(n)
    }

    /// Bezeichner ODER Kontextwort (get/set/static/of/...) — noetig, weil
    /// diese auch als Eigenschaftsnamen/Variablen vorkommen.
    fn frisst_name(&mut self) -> Option<String> {
        match &self.aktuell().art {
            TokenArt::Bezeichner(n) | TokenArt::BezeichnerEscape(n) => {
                let n = n.clone();
                self.weiter();
                Some(n)
            }
            TokenArt::Wort(w) => {
                let s = wort_text(*w);
                self.weiter();
                Some(s.to_string())
            }
            _ => None,
        }
    }
}

/// J10: der interne Name, unter dem `export default` abgelegt wird.
///
/// GEMESSEN/ENTSCHIEDEN, warum ein fester Name: der Loader liest nach dem
/// Lauf die Exporte aus der Modul-Umgebung, und da muss etwas stehen.
/// Der Name ist mit zwei Unterstrichen umgeben, damit er in
/// Skripten praktisch nicht vorkommt.
pub(crate) const DEFAULT_NAME: &str = "__jsvm_default__";

struct TiefenWacht;

pub(crate) fn parsen(tokens: Vec<Token>, max_tiefe: u32) -> Result<Programm, JsFehler> {
    Parser::neu(tokens, max_tiefe).lesen()
}

/// J19: wie `parsen`, aber mit Quelltext — dann kennt jede Funktion ihren
/// Quelltext (`Function.prototype.toString`).
pub(crate) fn parsen_mit_quelle(
    tokens: Vec<Token>,
    max_tiefe: u32,
    quelle: &str,
) -> Result<Programm, JsFehler> {
    Parser::mit_quelle(tokens, max_tiefe, quelle).lesen()
}

/// J10: Quelltext als **Modul** lesen (Importgraph, `export`).
pub(crate) fn parsen_als_modul(tokens: Vec<Token>, max_tiefe: u32) -> Result<Programm, JsFehler> {
    Parser::neu(tokens, max_tiefe).als_modul().lesen()
}

/// J19: wie `parsen_als_modul`, aber mit Quelltext (siehe
/// `parsen_mit_quelle`).
pub(crate) fn parsen_als_modul_mit_quelle(
    tokens: Vec<Token>,
    max_tiefe: u32,
    quelle: &str,
) -> Result<Programm, JsFehler> {
    Parser::mit_quelle(tokens, max_tiefe, quelle)
        .als_modul()
        .lesen()
}

impl Parser {
    /// Die ganze Quelle lesen (Top-Level-Schleife).
    fn lesen(mut self) -> Result<Programm, JsFehler> {
        let mut anweisungen = Vec::new();
        let mut strikt = false;
        let mut erste = true;
        while !self.ist_ende() {
            // `"use strict";` als Direktive am Anfang.
            if erste
                && let TokenArt::Zeichenkette(s) = &self.aktuell().art
                && s == "use strict"
            {
                strikt = true;
                // J27: auch im Parser hinterlegen — die Early Errors fuer
                // doppelte Parameternamen fragen danach (Begruendung am
                // Feld `strikt`).
                self.strikt = true;
            }
            erste = false;
            anweisungen.push(parse_anweisung(&mut self)?);
        }
        // J47: **auch die Skript-Ebene** hat ihre Neudeklarations-Regeln —
        // sie ist eine `StatementList` wie jede andere, nur mit `Rumpf`-Art
        // (eine Skript-Ebene ist kein `Block`, doppelte `function f` sind
        // dort erlaubt).
        //
        // GEMESSEN (node 22), **sechs** Proben, alle **SyntaxError** und
        // alle vor dieser Zeile **angenommen**:
        //   `function f(){}; let f;`     -> SyntaxError
        //   `class f{}; function f(){}`  -> SyntaxError
        //   `let f; let f;`              -> SyntaxError
        //   `class f{}; class f{}`        -> SyntaxError
        //   `let f; var f;`              -> SyntaxError
        //   `function* f(){}; function* f(){}` -> **erlaubt**
        //   `function f(){}; var f;`     -> **erlaubt**
        fruehfehler_neudeklaration(&self, &anweisungen, ListenArt::Rumpf)?;
        Ok(Programm {
            anweisungen,
            strikt,
            modul: self.modul,
        })
    }
}

/// J32: Darf dieses Schluesselwort als **Bindungsname** stehen?
///
/// ES2023 13.1.1 (`BindingIdentifier : Identifier`, Static Semantics: Early
/// Errors): „It is a Syntax Error if StringValue of Identifier is the same
/// String value as the StringValue of any ReservedWord except yield or
/// await."
///
/// GEMESSEN (node 22), **sieben** von sieben scheitern — und **ohne** Escape
/// genauso wie mit, denn der `Lexer` loest `\u0074his` schon zu `this` auf
/// und macht daraus dasselbe Token:
///   `var this = 1`                    -> SyntaxError
///   `var finally = 1`                 -> SyntaxError
///   `function f(){ var this }`        -> SyntaxError
///   `function f(){ var \u0074his }`   -> SyntaxError
///   `var \u0066inally = 1`            -> SyntaxError
///   `for (var this of []) {}`         -> SyntaxError
///   `var C = class { *gen(){ void yield } }` -> SyntaxError
///
/// GEMESSEN, warum `yield` und `await` **nicht** in dieser Liste stehen: die
/// Normzeile nimmt sie ausdruecklich aus, weil sie **Kontextwoerter** sind —
/// sie haben eigene Regeln (`in_generator`, `in_klasse`). Wer sie hier
/// aufnehmen wuerde, wuerde `function f(){ var yield = 1 }` toeten; wer sie
/// weglässt, laesst `class C { *gen(){ var yield } }` durch.
///
/// `get`, `set`, `of`, `async` und `target` sind ebenfalls **nicht** dabei:
/// sie sind keine ReservedWords, `var get = 1` ist erlaubt.
///
/// `let`, `static`, `implements`, `interface`, `package`, `private`,
/// `protected` und `public` sind im **strikten** Code reserviert (ES2023
/// 14.1). GEMESSEN (node 22): `'use strict'; var \u006cet = 1` ist ein
/// Syntaxfehler, `var \u006cet = 1` nicht — der Motor rechnet den Modus also
/// ueber `p.strikt`.
fn schluessel_als_bindung_verboten(w: Schluessel, strikt: bool, in_async: bool) -> bool {
    // J45: ES2023 13.1.1 (`BindingIdentifier : Identifier`, Early Errors):
    // „It is a Syntax Error if this production has a [Await] parameter and
    // StringValue of Identifier is "await"."
    //
    // GEMESSEN (node 22), **zwoelf** Proben:
    //   `class C { async *m() { var await } }`     -> SyntaxError
    //   `class C { async m() { var await } }`      -> SyntaxError
    //   `class C { async m(await) {} }`            -> SyntaxError
    //   `async function f() { var await }`         -> SyntaxError
    //   `function* g() { var await }`              -> **erlaubt** (kein [Await]!)
    //   `async function f() { function g(){ var await } }` -> **erlaubt**
    //   `class C { async m() { var x = { await: 1 } } }` -> **erlaubt**
    //   `class C { async m() { return this.await } }`     -> **erlaubt**
    //   `class await {}`                           -> **erlaubt**
    //   `function* g() { var yield }`              -> SyntaxError (gilt schon)
    //   `var await = 1; await` (Skript)            -> **erlaubt**
    //   `function f() { var await }`               -> **erlaubt**
    if in_async && w == Schluessel::Await {
        return true;
    }
    match w {
        // Immer reserviert (ES2023 12.6.2 Keywords + ReservedWord).
        Schluessel::Break
        | Schluessel::Case
        | Schluessel::Catch
        | Schluessel::Class
        | Schluessel::Const
        | Schluessel::Continue
        | Schluessel::Debugger
        | Schluessel::Default
        | Schluessel::Delete
        | Schluessel::Do
        | Schluessel::Else
        | Schluessel::Enum
        | Schluessel::Export
        | Schluessel::Extends
        | Schluessel::False
        | Schluessel::Finally
        | Schluessel::For
        | Schluessel::Function
        | Schluessel::If
        | Schluessel::Import
        | Schluessel::In
        | Schluessel::Instanceof
        | Schluessel::New
        | Schluessel::Null
        | Schluessel::Return
        | Schluessel::Super
        | Schluessel::Switch
        | Schluessel::This
        | Schluessel::Throw
        | Schluessel::True
        | Schluessel::Try
        | Schluessel::Typeof
        | Schluessel::Var
        | Schluessel::Void
        | Schluessel::While
        | Schluessel::With => true,
        // Nur im strikten Code (ES2023 14.1 StrictModeReservedWord).
        Schluessel::Let | Schluessel::Static => strikt,
        // Kontextwoerter: `yield`, `await`, `get`, `set`, `of`, `async`,
        // `target` — **keine** ReservedWords.
        _ => false,
    }
}

fn wort_text(w: Schluessel) -> &'static str {
    match w {
        Schluessel::Break => "break",
        Schluessel::Case => "case",
        Schluessel::Catch => "catch",
        Schluessel::Class => "class",
        Schluessel::Const => "const",
        Schluessel::Continue => "continue",
        Schluessel::Debugger => "debugger",
        Schluessel::Default => "default",
        Schluessel::Delete => "delete",
        Schluessel::Do => "do",
        Schluessel::Else => "else",
        Schluessel::Enum => "enum",
        Schluessel::Export => "export",
        Schluessel::Extends => "extends",
        Schluessel::False => "false",
        Schluessel::Finally => "finally",
        Schluessel::For => "for",
        Schluessel::Function => "function",
        Schluessel::If => "if",
        Schluessel::Import => "import",
        Schluessel::In => "in",
        Schluessel::Instanceof => "instanceof",
        Schluessel::Let => "let",
        Schluessel::New => "new",
        Schluessel::Null => "null",
        Schluessel::Return => "return",
        Schluessel::Super => "super",
        Schluessel::Switch => "switch",
        Schluessel::This => "this",
        Schluessel::Throw => "throw",
        Schluessel::True => "true",
        Schluessel::Try => "try",
        Schluessel::Typeof => "typeof",
        Schluessel::Var => "var",
        Schluessel::Void => "void",
        Schluessel::While => "while",
        Schluessel::With => "with",
        Schluessel::Yield => "yield",
        Schluessel::Static => "static",
        Schluessel::Get => "get",
        Schluessel::Set => "set",
        Schluessel::Of => "of",
        Schluessel::Target => "target",
        Schluessel::Async => "async",
        Schluessel::Await => "await",
        Schluessel::From => "from",
        Schluessel::As => "as",
        Schluessel::Constructor => "constructor",
    }
}

// ---------------------------------------------------------------------------
// Anweisungen
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// J10: ES-Module — `import` und `export`
// ---------------------------------------------------------------------------

/// `import … from 'q'` / `import 'q'` / `import * as ns from 'q'`.
///
/// GEMESSEN/ENTSCHIEDEN, warum der Parser **keine** eigene Anweisung
/// erzeugt: die Bindings werden spater vom Loader in die Modul-Umgebung
/// gelegt (eine Datei `module.rs`, komplett in meinem Crate, ohne Aenderung
/// am Uebersetzer). Der Import liefert also nur Angaben zurueck.
fn parse_import(p: &mut Parser) -> Result<(), JsFehler> {
    p.weiter(); // `import`
    // J12: `import(…)` ist der **dynamische** Import — ein Ausdruck, kein
    // Modul-Schluessel. Er wird in `parse_primaer` gelesen; hier
    // (Anweisungsebene) ist er nie gueltig.
    // `import 'q';` — kein Binding.
    if let TokenArt::Zeichenkette(q) = p.aktuell().art.clone() {
        p.weiter();
        let q = import_attribute(p, &q)?;
        p.erwartet_semi()?;
        notiz_import(p, ImportArt::Leer, &q);
        return Ok(());
    }
    // Sonst eine Namensliste, optional mit Vorgabe und `* as ns`.
    let mut vorgabe: Option<String> = None;
    let mut paare: Vec<(String, String)> = Vec::new();
    let mut alles: Option<String> = None;
    if p.ist_zeichen(Zeichen::Stern) {
        p.weiter();
        if !p.frisst_wort(Schluessel::As) {
            return Err(p.fehler("'* as' erwartet".to_string()));
        }
        alles = Some(p.bezeichner("Name nach '* as'")?);
    } else if p.ist_zeichen(Zeichen::LRunde) {
        return Err(p.fehler("import(...) ist ein Ausdruck".to_string()));
    } else if p.aktuell().art != TokenArt::Zeichen(Zeichen::LGeschweift) {
        // Der Vorgabename: `import d from 'q'` — `from` ist ein Schluessel,
        // `d` ein Bezeichner.
        let name = p.bezeichner("Name im Import")?;
        if p.ist_wort(Schluessel::From) {
            vorgabe = Some(name);
        } else {
            paare.push((name.clone(), name));
        }
        if p.frisst_zeichen(Zeichen::Komma) {
            if p.ist_zeichen(Zeichen::Stern) {
                p.weiter();
                if !p.frisst_wort(Schluessel::As) {
                    return Err(p.fehler("'* as' erwartet".to_string()));
                }
                alles = Some(p.bezeichner("Name nach '* as'")?);
            } else if p.ist_zeichen(Zeichen::LGeschweift) {
                paare.extend(parse_import_liste(p)?);
            } else {
                return Err(p.fehler("',' '{' oder '* as' erwartet".to_string()));
            }
        }
    }
    if p.ist_zeichen(Zeichen::LGeschweift) {
        paare.extend(parse_import_liste(p)?);
    }
    if !p.frisst_wort(Schluessel::From) {
        return Err(p.fehler("'from' nach dem Import erwartet".to_string()));
    }
    let TokenArt::Zeichenkette(q) = p.aktuell().art.clone() else {
        return Err(p.fehler("Module-Adresse in Anfuehrungszeichen".to_string()));
    };
    p.weiter();
    let q = import_attribute(p, &q)?;
    p.erwartet_semi()?;
    // In eine Form pressen: alles in `Gemischt`, ausser es ist genau eine
    // der einfachen Formen (das haelt die Meldungen beim Lader kurz).
    let art = match (&vorgabe, paare.is_empty(), &alles) {
        (None, true, None) => ImportArt::Leer,
        (Some(v), true, None) => ImportArt::Vorgabe { name: v.clone() },
        (None, false, None) => ImportArt::Liste { paare },
        (None, true, Some(n)) => ImportArt::Alles { name: n.clone() },
        _ => ImportArt::Gemischt {
            vorgabe: vorgabe.clone(),
            paare,
            alles: alles.clone(),
        },
    };
    notiz_import(p, art, &q);
    Ok(())
}

/// `{ a, b as c, d as e }` innerhalb eines Imports.
fn parse_import_liste(p: &mut Parser) -> Result<Vec<(String, String)>, JsFehler> {
    p.erwartet_zeichen(Zeichen::LGeschweift, "'{'")?;
    let mut paare = Vec::new();
    loop {
        // `type X` (TypeScript) wird nicht gebraucht; `default` als
        // Schluessel ist erlaubt.
        let extern_ = match &p.aktuell().art {
            TokenArt::Bezeichner(n) | TokenArt::BezeichnerEscape(n) => {
                let n = n.clone();
                p.weiter();
                n
            }
            TokenArt::Wort(w) => {
                let t = wort_text(*w).to_string();
                p.weiter();
                t
            }
            _ => return Err(p.fehler("Name in der Importliste erwartet".to_string())),
        };
        let intern = if p.frisst_wort(Schluessel::As) {
            p.bezeichner("Name nach 'as'")?
        } else {
            extern_.clone()
        };
        paare.push((extern_, intern));
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
        if p.ist_zeichen(Zeichen::RGeschweift) {
            break; // Nachgestelltes Komma ist erlaubt.
        }
    }
    p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
    Ok(paare)
}

/// `export …` — liefert die **Anweisung**, die der Uebersetzer sieht.
fn parse_export(p: &mut Parser) -> Result<Anweisung, JsFehler> {
    p.weiter(); // `export`
    // `export default …`
    if p.frisst_wort(Schluessel::Default) {
        let wert = parse_export_default_wert(p)?;
        notiz_export(p, "default", DEFAULT_NAME, None);
        return Ok(Anweisung::Deklaration {
            art: DeklArt::Const,
            eintraege: vec![(Muster::Name(DEFAULT_NAME.to_string()), Some(wert))],
        });
    }
    // `export * from 'q'` / `export * as ns from 'q'`
    if p.ist_zeichen(Zeichen::Stern) {
        p.weiter();
        if p.frisst_wort(Schluessel::As) {
            let name = p.bezeichner("Name nach '* as'")?;
            if !p.frisst_wort(Schluessel::From) {
                return Err(p.fehler("'from' erwartet".to_string()));
            }
            let q = parse_quelle(p)?;
            notiz_export(p, &name, "*", Some(&q));
            p.erwartet_semi()?;
            return Ok(Anweisung::Leer);
        }
        if !p.frisst_wort(Schluessel::From) {
            return Err(p.fehler("'from' nach '*' erwartet".to_string()));
        }
        let q = parse_quelle(p)?;
        notiz_export(p, "*", "*", Some(&q));
        p.erwartet_semi()?;
        return Ok(Anweisung::Leer);
    }
    // `export { a, b as c }` / `export { a as b } from 'q'`
    if p.ist_zeichen(Zeichen::LGeschweift) {
        let paare = parse_import_liste(p)?;
        let quelle = if p.frisst_wort(Schluessel::From) {
            Some(parse_quelle(p)?)
        } else {
            None
        };
        p.erwartet_semi()?;
        // GEMESSEN, warum hier die Rollen **vertauscht** sind: bei
        // `import { a as b }` ist `a` der Name im Quellmodul und `b` der
        // lokale; bei `export { a as b }` ist es umgekehrt — `a` ist die
        // lokale Bindung und `b` der Name, unter dem der Importeur sie
        // sieht. Ohne den Tausch lieferte `export { B as B2 } from './a.js'`
        // den Export **B** (GEMESSEN) statt B2.
        for (lokal, sichtbar) in paare {
            notiz_export(p, &sichtbar, &lokal, quelle.as_deref());
        }
        return Ok(Anweisung::Leer);
    }
    // `export const/let/var …`, `export function …`, `export class …`:
    // normale Anweisung + je deklariertem Namen ein Export.
    let vorname = match &p.aktuell().art {
        TokenArt::Wort(Schluessel::Var) => "var",
        TokenArt::Wort(Schluessel::Let) => "let",
        TokenArt::Wort(Schluessel::Const) => "const",
        _ => "",
    };
    if !vorname.is_empty() {
        let art = match vorname {
            "var" => DeklArt::Var,
            "let" => DeklArt::Let,
            _ => DeklArt::Const,
        };
        p.weiter();
        let eintraege = parse_dekl_liste(p)?;
        for (m, _) in &eintraege {
            if let Muster::Name(n) = m {
                notiz_export(p, n, n, None);
            }
        }
        p.erwartet_semi()?;
        return Ok(Anweisung::Deklaration { art, eintraege });
    }
    if p.ist_wort(Schluessel::Async)
        && p.tokens.get(p.pos + 1).map(|t| &t.art) == Some(&TokenArt::Wort(Schluessel::Function))
    {
        p.weiter();
        // J45: hier ist KEIN [Await] — `await` ist wieder ein
        // Bindungsname (GEMESSEN: `async function f(){ function g(){
        // var await } }` ist in node erlaubt).
        let war_async = p.in_async;
        p.in_async = false;
        let r = parse_funktion(p, false);
        p.in_async = war_async;
        let (name, bau) = r?;
        if let Some(n) = &name {
            notiz_export(p, n, n, None);
        }
        return Ok(Anweisung::Funktion {
            name,
            funktion: bau,
        });
    }
    if p.ist_wort(Schluessel::Function) {
        // J45: hier ist KEIN [Await] — `await` ist wieder ein
        // Bindungsname (GEMESSEN: `async function f(){ function g(){
        // var await } }` ist in node erlaubt).
        let war_async = p.in_async;
        p.in_async = false;
        let r = parse_funktion(p, false);
        p.in_async = war_async;
        let (name, bau) = r?;
        if let Some(n) = &name {
            notiz_export(p, n, n, None);
        }
        return Ok(Anweisung::Funktion {
            name,
            funktion: bau,
        });
    }
    if p.ist_wort(Schluessel::Class) {
        let q_start = p.pos; // J19: `class` beginnt den Quelltext
        p.weiter();
        let (name, eltern, elemente) = parse_klasse_rumpf(p)?;
        if let Some(n) = &name {
            notiz_export(p, n, n, None);
        }
        return Ok(Anweisung::Klasse {
            name,
            eltern,
            elemente,
            // J19: Quelltext der Klasse.
            quelle: p.quelle_von_bis(q_start, p.pos),
        });
    }
    Err(p.fehler("nach 'export' erwartet: Deklaration, default, *, {…} oder from".to_string()))
}

/// Der Wert von `export default …`.
fn parse_export_default_wert(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    if p.ist_wort(Schluessel::Function) {
        // J45: hier ist KEIN [Await] — `await` ist wieder ein
        // Bindungsname (GEMESSEN: `async function f(){ function g(){
        // var await } }` ist in node erlaubt).
        let war_async = p.in_async;
        p.in_async = false;
        let r = parse_funktion(p, false);
        p.in_async = war_async;
        let (name, bau) = r?;
        p.erwartet_semi()?;
        return Ok(Ausdruck::Funktion {
            name,
            funktion: Box::new(bau),
        });
    }
    if p.ist_wort(Schluessel::Async)
        && p.tokens.get(p.pos + 1).map(|t| &t.art) == Some(&TokenArt::Wort(Schluessel::Function))
    {
        p.weiter();
        // J45: `await` ist ab hier **kein** Bindungsname — fuer Parameter
        // und Rumpf (ES2023 13.1.1).
        let war_async = p.in_async;
        p.in_async = true;
        let r = parse_funktion(p, false);
        p.in_async = war_async;
        let (name, mut bau) = r?;
        bau.asynchron = true;
        p.erwartet_semi()?;
        return Ok(Ausdruck::Funktion {
            name,
            funktion: Box::new(bau),
        });
    }
    if p.ist_wort(Schluessel::Class) {
        let q_start = p.pos; // J19: `class` beginnt den Quelltext
        p.weiter();
        let (name, eltern, elemente) = parse_klasse_rumpf(p)?;
        return Ok(Ausdruck::Klasse {
            name,
            eltern: eltern.map(Box::new),
            elemente,
            // J19: Quelltext der Klasse.
            quelle: p.quelle_von_bis(q_start, p.pos),
        });
    }
    let e = parse_ausdruck(p)?;
    p.erwartet_semi()?;
    Ok(e)
}

/// Die Modul-Adresse in Anfuehrungszeichen holen.
fn parse_quelle(p: &mut Parser) -> Result<String, JsFehler> {
    let TokenArt::Zeichenkette(q) = p.aktuell().art.clone() else {
        return Err(p.fehler("Module-Adresse in Anfuehrungszeichen".to_string()));
    };
    p.weiter();
    import_attribute(p, &q)
}

/// Release 11.10.2026: Import-Attribute (ES2025 `with { type: "json" }`,
/// auch das alte `assert { … }`). Der Typ wandert als Marke in die
/// Modul-Adresse (`module::MODUL_TYP_MARKE`) — ein JSON-Modul ist ein
/// anderes Modul als ein Skript derselben Adresse (HTML §8.1.5.6
/// „module map" mit dem Typ im Schluessel). Ein anderer Schluessel als
/// `type` ist ein SyntaxError. GEMESSEN vorher: `import v from "./m.json"
/// with { type: "json" }` war ein Syntaxfehler (WPT json-module,
/// css-module, text-module: 49 Dateien).
fn import_attribute(p: &mut Parser, q: &str) -> Result<String, JsFehler> {
    let ist_with = p.ist_wort(Schluessel::With);
    let ist_assert = matches!(&p.aktuell().art, TokenArt::Bezeichner(n) if n == "assert") && !p.aktuell().umbruch_davor;
    if !ist_with && !ist_assert {
        return Ok(q.to_string());
    }
    p.weiter();
    p.erwartet_zeichen(Zeichen::LGeschweift, "'{' nach 'with'")?;
    let mut typ: Option<String> = None;
    let mut fremd: Option<String> = None;
    while !p.ist_zeichen(Zeichen::RGeschweift) {
        let k = match &p.aktuell().art {
            TokenArt::Bezeichner(n) | TokenArt::BezeichnerEscape(n) => n.clone(),
            TokenArt::Wort(w) => wort_text(*w).to_string(),
            TokenArt::Zeichenkette(t) => t.clone(),
            _ => return Err(p.fehler("Schluessel im Import-Attribut erwartet".to_string())),
        };
        p.weiter();
        p.erwartet_zeichen(Zeichen::DoppelPunkt, "':' im Import-Attribut")?;
        let TokenArt::Zeichenkette(v) = p.aktuell().art.clone() else {
            return Err(p.fehler("der Wert eines Import-Attributs ist Text".to_string()));
        };
        p.weiter();
        if k == "type" {
            typ = Some(v);
        } else if fremd.is_none() {
            fremd = Some(k);
        }
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
    }
    p.erwartet_zeichen(Zeichen::RGeschweift, "'}' nach den Import-Attributen")?;
    if let Some(k) = fremd {
        return Err(p.fehler(format!("Import-Attribut '{k}' wird nicht unterstuetzt")));
    }
    Ok(match typ {
        Some(t) => format!("{q}{}{t}", crate::module::MODUL_TYP_MARKE),
        None => q.to_string(),
    })
}

/// Einen Import/Vorgang im Parser vermerken (nur im Modul-Modus).
fn notiz_import(p: &mut Parser, art: ImportArt, quelle: &str) {
    if let Some(m) = p.modul.as_mut() {
        m.import.push(crate::ast::Import {
            art,
            quelle: quelle.to_string(),
        });
    }
}

fn notiz_export(p: &mut Parser, extern_: &str, intern: &str, quelle: Option<&str>) {
    if let Some(m) = p.modul.as_mut() {
        m.export.push(crate::ast::Export {
            extern_: extern_.to_string(),
            intern: intern.to_string(),
            quelle: quelle.map(|q| q.to_string()),
        });
    }
}

fn parse_anweisung(p: &mut Parser) -> Result<Anweisung, JsFehler> {
    let _w = p.tief().map_err(|e| e)?;
    // J28: die Zeile des ersten Tokens VOR dem Parsen nehmen — danach
    // steht der Parser schon in der Anweisung drin.
    //
    // GEMESSEN, warum hier und nicht im Lexer: der Lexer kennt die
    // Zeile, aber nicht, welche Tokens EINE Anweisung sind. Das weiss
    // erst der Parser, und diese eine Stelle ist der einzige Weg, durch
    // den jede Anweisung laeuft (auch die in `Marke:`-Rumpfen, in
    // `if`-Zweigen und in Bloecken — die rufen alle `parse_anweisung`).
    let zeile = p.aktuell().zeile;
    let mut a = parse_anweisung_tief(p)?;
    p.tiefe = p.tiefe.saturating_sub(1);
    // J28: die Zeile an die Funktionsbauten im Rumpf weitergeben (ein
    // Haken beim Aufruf zeigt sonst Zeile 0 — gemessen).
    Anweisung::zeilen_setzen(&mut a, zeile);
    Ok(Anweisung::MitZeile {
        zeile,
        innen: Box::new(a),
    })
}

fn parse_anweisung_tief(p: &mut Parser) -> Result<Anweisung, JsFehler> {
    // Marke `name:` — Bezeichner gefolgt von Doppelpunkt.
    if let Some(n) = p.aktuell_bezeichner() {
        let art = p.aktuell().art.clone();
        if p.tokens.get(p.pos + 1).map(|t| t.art.clone())
            == Some(TokenArt::Zeichen(Zeichen::DoppelPunkt))
        {
            p.weiter();
            p.weiter();
            if let Some(s) = ist_bindungsname_verboten(&art, p.strikt, p.in_async) {
                return Err(p.fehler(format!("'{}' ist kein gueltiges Label", s)));
            }
            if p.in_generator && n == "yield" {
                return Err(p.fehler("'yield' ist im Generator kein gueltiges Label".to_string()));
            }
            if p.in_klasse && n == "yield" {
                return Err(
                    p.fehler("'yield' ist im Klassenrumpf kein gueltiges Label".to_string())
                );
            }
            let innere = parse_rumpf_anweisung(p, "Marke")?;
            return Ok(Anweisung::Marke(n, Box::new(innere)));
        }
    }
    // J10 (ES-Module): `import` / `export`. Beides ist nur auf der obersten
    // Ebene **eines Moduls** erlaubt — in einem klassischen `<script>`
    // (und in node ohne `type="module"`) ist es ein Syntaxfehler, und
    // verschachtelt (`if (x) import …`) ebenfalls.
    //
    // GEMESSEN, warum die Fehlermeldung so konkret ist: vorher stand hier
    // `Syntaxfehler 2:8: erwartet: ';'` (das aus J6 gemeldete Zeichen) —
    // daraus sieht niemand, dass es am `type="module"` liegt.
    if p.tiefe == 1
        && matches!(
            &p.aktuell().art,
            TokenArt::Wort(Schluessel::Import) | TokenArt::Wort(Schluessel::Export)
        )
    {
        // `import(…)` ist der **dynamische** Import — er ist ein Ausdruck
        // und darf auch im Skript stehen (in node so). Deshalb sagt der
        // Motor hier die *eigentliche* Luecke, nicht „kein Modul":
        // GEMESSEN, warum das wichtig ist: `import('./a.js')` bekam sonst
        // „nur in einem Modul" und damit eine falsche Diagnose.
        if p.ist_wort(Schluessel::Import)
            && matches!(
                p.tokens.get(p.pos + 1).map(|t| &t.art),
                Some(TokenArt::Zeichen(Zeichen::LRunde))
            )
        {
            // `import('./a.js');` als **Anweisung** ist in node gueltig
            // (der Ausdruck als Ausdrucksanweisung).
            let e = parse_ausdruck(p)?;
            p.erwartet_semi()?;
            return Ok(Anweisung::Ausdruck(e));
        }
        // J15: `import.meta` ist ein **Ausdruck** — wie der dynamische
        // Import darf er auch im Skript stehen; im Skript ist er ein
        // Syntaxfehler (node ebenso), und genau das sagt der Motor hier.
        if p.ist_wort(Schluessel::Import)
            && matches!(
                p.tokens.get(p.pos + 1).map(|t| &t.art),
                Some(TokenArt::Zeichen(Zeichen::Punkt))
            )
        {
            if p.modul.is_none() {
                return Err(p.fehler(
                    "import.meta gibt es nur in einem Modul (script type=\"module\")".to_string(),
                ));
            }
            let e = parse_ausdruck(p)?;
            p.erwartet_semi()?;
            return Ok(Anweisung::Ausdruck(e));
        }
        if p.modul.is_none() {
            return Err(
                p.fehler("import/export nur in einem Modul (script type=\"module\")".to_string())
            );
        }
        if p.ist_wort(Schluessel::Import) {
            return parse_import(p).map(|()| Anweisung::Leer);
        }
        return parse_export(p);
    }
    match &p.aktuell().art.clone() {
        TokenArt::Zeichen(Zeichen::LGeschweift) => {
            p.weiter();
            let mut v = Vec::new();
            while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
                v.push(parse_anweisung(p)?);
            }
            p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
            // J34: die Neudeklarations-Regeln gelten fuer **jede**
            // StatementList — auch fuer eine, die als `Block`-Anweisung dasteht.
            // GEMESSEN: die erste Fassung stand nur in
            // `parse_block_anweisungen` (Funktionsrumpf) und liess deshalb
            // `{ const f = 1; var f; }` durch.
            fruehfehler_neudeklaration(p, &v, ListenArt::Block)?;
            Ok(Anweisung::Block(v))
        }
        TokenArt::Zeichen(Zeichen::Semikolon) => {
            p.weiter();
            Ok(Anweisung::Leer)
        }
        TokenArt::Wort(Schluessel::Var)
        | TokenArt::Wort(Schluessel::Let)
        | TokenArt::Wort(Schluessel::Const) => {
            let art = match &p.aktuell().art {
                TokenArt::Wort(Schluessel::Var) => DeklArt::Var,
                TokenArt::Wort(Schluessel::Let) => DeklArt::Let,
                _ => DeklArt::Const,
            };
            p.weiter();
            let eintraege = parse_dekl_liste(p)?;
            // J35: `const` ohne Anfangswert.
            fruehfehler_const_initialisierer(p, art, &eintraege)?;
            p.erwartet_semi()?;
            Ok(Anweisung::Deklaration { art, eintraege })
        }
        TokenArt::Wort(Schluessel::If) => {
            p.weiter();
            p.erwartet_zeichen(Zeichen::LRunde, "'(' nach if")?;
            let b = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            let dann = Box::new(parse_rumpf_anweisung(p, "if")?);
            let sonst = if p.frisst_wort(Schluessel::Else) {
                Some(Box::new(parse_rumpf_anweisung(p, "else")?))
            } else {
                None
            };
            Ok(Anweisung::Wenn {
                bedingung: b,
                dann,
                sonst,
            })
        }
        TokenArt::Wort(Schluessel::While) => {
            p.weiter();
            p.erwartet_zeichen(Zeichen::LRunde, "'(' nach while")?;
            let b = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
            Ok(Anweisung::Solange {
                bedingung: b,
                rumpf,
                nachlauf: false,
            })
        }
        TokenArt::Wort(Schluessel::Do) => {
            p.weiter();
            let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
            if !p.frisst_wort(Schluessel::While) {
                return Err(p.fehler("erwartet: 'while' nach do".to_string()));
            }
            p.erwartet_zeichen(Zeichen::LRunde, "'('")?;
            let b = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            // J16: nach `do … while (…)` ist das Semikolon **optional**
            // (Spec: ASI). GEMESSEN an einem echten github-Bündel
            // (`selector-observer`, 20 KiB): der Rumpf endet mit
            // `…break}}while(s)return f}` — ohne `;` brach das mit
            // `Syntaxfehler: erwartet: ';'` ab. node 22 laeuft durch.
            let _ = p.frisst_zeichen(Zeichen::Semikolon);
            Ok(Anweisung::Solange {
                bedingung: b,
                rumpf,
                nachlauf: true,
            })
        }
        TokenArt::Wort(Schluessel::For) => parse_for(p),
        TokenArt::Wort(Schluessel::Switch) => {
            p.weiter();
            p.erwartet_zeichen(Zeichen::LRunde, "'(' nach switch")?;
            let w = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            p.erwartet_zeichen(Zeichen::LGeschweift, "'{'")?;
            let mut faelle = Vec::new();
            while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
                if p.frisst_wort(Schluessel::Case) {
                    let c = parse_ausdruck(p)?;
                    p.erwartet_zeichen(Zeichen::DoppelPunkt, "':'")?;
                    let mut koerper = Vec::new();
                    while !p.ist_wort(Schluessel::Case)
                        && !p.ist_wort(Schluessel::Default)
                        && !p.ist_zeichen(Zeichen::RGeschweift)
                        && !p.ist_ende()
                    {
                        koerper.push(parse_anweisung(p)?);
                    }
                    // J34: **nicht** je `case` pruefen — alle Faelle teilen
                    // **einen** Geltungsbereich (ES2023 14.15, `CaseBlock`:
                    // `LexicallyDeclaredNames` und `VarDeclaredNames` laufen
                    // ueber die ganze Liste). GEMESSEN (node 22):
                    // `switch(1){ case 1: let x; case 2: var x; }`
                    //   -> SyntaxError; mit einer Pruefung je `case` rutscht
                    // der Fehler durch. Die Sammelpruefung steht unten.
                    faelle.push((Some(c), koerper));
                } else if p.frisst_wort(Schluessel::Default) {
                    p.erwartet_zeichen(Zeichen::DoppelPunkt, "':'")?;
                    let mut koerper = Vec::new();
                    while !p.ist_wort(Schluessel::Case)
                        && !p.ist_wort(Schluessel::Default)
                        && !p.ist_zeichen(Zeichen::RGeschweift)
                        && !p.ist_ende()
                    {
                        koerper.push(parse_anweisung(p)?);
                    }
                    faelle.push((None, koerper));
                } else {
                    return Err(p.fehler("erwartet: 'case' oder 'default'".to_string()));
                }
            }
            p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
            // J34: eine einzige Pruefung ueber **alle** Faelle — siehe oben.
            let alle: Vec<Anweisung> = faelle.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
            fruehfehler_neudeklaration(p, &alle, ListenArt::Block)?;
            Ok(Anweisung::Auswahl { wert: w, faelle })
        }
        TokenArt::Wort(Schluessel::Return) => {
            // J42: `return` nur im Funktionsrumpf. GEMESSEN (node 22) — siehe
            // das Flag `in_funktionsrumpf`. **WICHTIG:** die Pruefung kommt
            // **vor** `p.weiter()`, damit die Meldung die Zeile von `return`
            // nennt und nicht die der Auswertung.
            if !p.in_funktionsrumpf {
                return Err(
                    p.fehler("'return' ist nur im Rumpf einer Funktion erlaubt".to_string())
                );
            }
            p.weiter();
            let t = p.aktuell();
            if t.umbruch_davor
                || matches!(
                    t.art,
                    TokenArt::Zeichen(Zeichen::Semikolon)
                        | TokenArt::Zeichen(Zeichen::RGeschweift)
                        | TokenArt::Ende
                )
            {
                p.erwartet_semi()?;
                return Ok(Anweisung::Rueckgabe(None));
            }
            let e = parse_ausdruck(p)?;
            p.erwartet_semi()?;
            Ok(Anweisung::Rueckgabe(Some(e)))
        }
        TokenArt::Wort(Schluessel::Throw) => {
            p.weiter();
            if p.aktuell().umbruch_davor {
                return Err(p.fehler("kein Zeilenumbruch nach 'throw'".to_string()));
            }
            let e = parse_ausdruck(p)?;
            p.erwartet_semi()?;
            Ok(Anweisung::Werfen(e))
        }
        TokenArt::Wort(Schluessel::Try) => {
            p.weiter();
            p.erwartet_zeichen(Zeichen::LGeschweift, "'{' nach try")?;
            let mut rumpf = Vec::new();
            while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
                rumpf.push(parse_anweisung(p)?);
            }
            p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
            fruehfehler_neudeklaration(p, &rumpf, ListenArt::Block)?;
            // J34: siehe `Block` — jede StatementList hat ihre eigenen
            // Neudeklarationsregeln.
            let mut fang_name = None;
            let mut fang = Vec::new();
            let mut hat_catch = false;
            if p.frisst_wort(Schluessel::Catch) {
                hat_catch = true;
                if p.frisst_zeichen(Zeichen::LRunde) {
                    fang_name = Some(parse_muster(p)?);
                    p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
                }
                p.erwartet_zeichen(Zeichen::LGeschweift, "'{' nach catch")?;
                while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
                    fang.push(parse_anweisung(p)?);
                }
                p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
                fruehfehler_neudeklaration(p, &fang, ListenArt::Block)?;
                // J34: siehe `Block` — jede StatementList hat ihre eigenen
                // Neudeklarationsregeln.
            }
            let mut endlich = Vec::new();
            let mut hat_finally = false;
            if p.frisst_wort(Schluessel::Finally) {
                hat_finally = true;
                p.erwartet_zeichen(Zeichen::LGeschweift, "'{' nach finally")?;
                while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
                    endlich.push(parse_anweisung(p)?);
                }
                p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
                fruehfehler_neudeklaration(p, &endlich, ListenArt::Block)?;
                // J34: siehe `Block` — jede StatementList hat ihre eigenen
                // Neudeklarationsregeln.
            }
            if !hat_catch && !hat_finally {
                return Err(p.fehler("try ohne catch/finally".to_string()));
            }
            Ok(Anweisung::Versuch {
                rumpf,
                fang_name,
                fang,
                endlich,
                hat_catch,
            })
        }
        TokenArt::Wort(Schluessel::Break) => {
            p.weiter();
            let marke = if !p.aktuell().umbruch_davor {
                p.frisst_name()
            } else {
                None
            };
            // `break ident` ohne Doppelpunkt ist Marke; sonst Variable — hier
            // immer Marke (break ohne Ziel aequivalent).
            p.erwartet_semi()?;
            Ok(Anweisung::Abbruch {
                weiter: false,
                marke,
            })
        }
        TokenArt::Wort(Schluessel::Continue) => {
            p.weiter();
            let marke = if !p.aktuell().umbruch_davor {
                p.frisst_name()
            } else {
                None
            };
            p.erwartet_semi()?;
            Ok(Anweisung::Abbruch {
                weiter: true,
                marke,
            })
        }
        TokenArt::Wort(Schluessel::Function) => {
            // GEMESSEN (J22, node 22 als Orakel): eine namenlose
            // `function`/`async function` an Statement-Start ist ein
            // **SyntaxError** — `ExpressionStatement` schliesst
            // `function`/`async function`/`class`/`let [` per Lookahead
            // aus (Spec 13.3). Der Motor lehnt sie deshalb auch ab;
            // `function(){ return 1 }()` scheitert bei node genauso. Der
            // Bündel-Fall liegt **nicht** an Statement-Start, sondern
            // in einer Argumentliste (siehe `parse_kette_ab`).
            // J45: hier ist KEIN [Await] — `await` ist wieder ein
            // Bindungsname (GEMESSEN: `async function f(){ function g(){
            // var await } }` ist in node erlaubt).
            let war_async = p.in_async;
            p.in_async = false;
            let r = parse_funktion(p, false);
            p.in_async = war_async;
            let (name, bau) = r?;
            Ok(Anweisung::Funktion {
                name,
                funktion: bau,
            })
        }
        TokenArt::Wort(Schluessel::Async) => {
            // `async function` oder Ausdruck.
            if naechster_ist_function(p) {
                p.weiter();
                // J45: `await` ist ab hier kein Bindungsname (Parameter
                // und Rumpf) — siehe `schluessel_als_bindung_verboten`.
                let war_async = p.in_async;
                p.in_async = true;
                let r = parse_funktion(p, false);
                p.in_async = war_async;
                let (name, mut bau) = r?;
                bau.asynchron = true;
                Ok(Anweisung::Funktion {
                    name,
                    funktion: bau,
                })
            } else {
                let e = parse_ausdruck(p)?;
                p.erwartet_semi()?;
                Ok(Anweisung::Ausdruck(e))
            }
        }
        TokenArt::Wort(Schluessel::Class) => {
            let q_start = p.pos; // J19: `class` beginnt den Quelltext
            p.weiter();
            let (name, eltern, elemente) = parse_klasse_rumpf(p)?;
            Ok(Anweisung::Klasse {
                name,
                eltern,
                elemente,
                // J19: Quelltext der Klasse.
                quelle: p.quelle_von_bis(q_start, p.pos),
            })
        }
        TokenArt::Wort(Schluessel::Debugger) => {
            p.weiter();
            p.erwartet_semi()?;
            Ok(Anweisung::Haltepunkt)
        }
        TokenArt::Wort(Schluessel::With) => {
            p.weiter();
            p.erwartet_zeichen(Zeichen::LRunde, "'(' nach with")?;
            let e = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            let b = Box::new(parse_rumpf_anweisung(p, "with")?);
            Ok(Anweisung::Mit(e, b))
        }
        _ => {
            let e = parse_ausdruck(p)?;
            p.erwartet_semi()?;
            Ok(Anweisung::Ausdruck(e))
        }
    }
}

fn naechster_ist_function(p: &Parser) -> bool {
    matches!(
        p.tokens.get(p.pos + 1).map(|t| &t.art),
        Some(TokenArt::Wort(Schluessel::Function))
    )
}

/// J35: `if`/`else`, `while`, `do`, `for`, `with` und eine Marke verlangen
/// eine **`Statement`**, und ES2023 14.12 Grammatik gehoert
/// `LexicalDeclaration` (und `ClassDeclaration`) **nicht** dazu.
///
/// GEMESSEN (node 22):
///   `if (1) const x = 1;`          -> SyntaxError
///   `while (1) const x = 1;`       -> SyntaxError
///   `do const x = 1; while (0);`   -> SyntaxError
///   `for (;;) const x = 1;`        -> SyntaxError
///   `for (var i=0;i<0;i++) const x = 1;` -> SyntaxError
///   `l: const x = 1;`              -> SyntaxError
///   `with ({}) const x = 1;`       -> SyntaxError
///   `if (1) class C {}`            -> SyntaxError
///   `if (1) { const x = 1 }`       -> ok  (ein **Block** ist eine StatementList)
///   `switch (1) { case 1: const x = 1 }` -> ok (CaseBlock ist eine Liste)
///   `if (1) function f(){}`        -> ok  (Annex B.3.3, sloppy!)
///   `l: function f(){}`            -> ok  (ebenso)
///
/// **WARUM `function` AUSGENOMMEN IST, OBWOHL ES EINE `Declaration` IST:**
/// Anhang B.3.3 („Labelled Function Declarations" und
/// „FunctionDeclarations in IfStatement Statement Clauses") erlaubt es in
/// schlampigem Code ausdruecklich. Eine Regel ohne diese Ausnahme wuerde
/// gaengigen Code ablehnen — genau die Falsch-Positiv-Falle, gegen die
/// Punkt 13 abgesichert wurde.
///
/// **WARUM DIE `MitZeile`-Huelle geprueft wird:** `parse_anweisung`
/// verpackt jede Anweisung darin; `inhalt()` schaltet sie ab (dieselbe
/// Hilfsfunktion, die `compile.rs` benutzt).
fn fruehfehler_rumpf_keine_deklaration(
    p: &Parser,
    a: &Anweisung,
    ort: &str,
) -> Result<(), JsFehler> {
    // J40: `async function` (und `async function*`) ist in **keiner**
    // Rumpfposition erlaubt — auch nicht in schlampigem Code.
    //
    // GEMESSEN (node 22), **zehn** Proben:
    //   `if (0) async function f(){}`        -> SyntaxError
    //   `if (0) async function* f(){}`       -> SyntaxError
    //   `if (0) ; else async function* f(){}`-> SyntaxError
    //   `while (0) async function f(){}`     -> SyntaxError
    //   `for (;;) async function f(){}`      -> SyntaxError
    //   `l: async function f(){}`            -> SyntaxError
    //   `with ({}) async function f(){}`     -> SyntaxError
    //   `{ async function f(){} }`           -> **erlaubt** (Block!)
    //   `if (0) { async function f(){} }`    -> **erlaubt**
    //   `if (0) function f(){}`              -> **erlaubt** (Anhang B.3.3!)
    //   `class C { async m(){} }`            -> **erlaubt** (Methode)
    //
    // **DER UNTERSCHIED ZU `function` IST DER PUNKT:** Anhang B.3.3 erlaubt
    // eine *Labelled* FunctionDeclaration in schlampigem Code ausdruecklich —
    // fuer `async` gibt es diese Ausnahme **nicht**. Wer beide Fuesse gleich
    // behandelt, lehnt `if (x) function f(){}` ab (Anhang B) **oder**
    // laesst `if (x) async function f(){}` durch (node lehnt es ab).
    if let Anweisung::Funktion { funktion, .. } = a
        && funktion.asynchron
    {
        return Err(
            p.fehler("'async function' ist hier nicht erlaubt — in { } schreiben".to_string())
        );
    }
    let was = match a {
        Anweisung::Deklaration {
            art: DeklArt::Var, ..
        } => return Ok(()),
        Anweisung::Deklaration { .. } | Anweisung::Klasse { .. } => true,
        _ => false,
    };
    if was {
        return Err(p.fehler(format!(
            "hinter '{ort}' steht eine Deklaration ohne Block — in {{ }} schreiben"
        )));
    }
    Ok(())
}

/// J36: Rumpf-Anweisung holen **und** die Grammatik pruefen. Genau diese
/// Funktion nehmen alle Rumpf-Stellen, damit die Regel an **einer** Stelle
/// steht: eine Regel an sieben Stellen wird sechsmal vergessen.
///
/// `ort` steht in der Meldung, damit die Fehlermeldung sagt, **wo** der
/// Fehler ist — `parse_anweisung` meldet sonst `0:0`.
fn parse_rumpf_anweisung(p: &mut Parser, ort: &str) -> Result<Anweisung, JsFehler> {
    let a = parse_anweisung(p)?;
    fruehfehler_rumpf_keine_deklaration(p, a.inhalt(), ort)?;
    Ok(a)
}

/// J35: ES2023 14.3.1 (`LexicalDeclaration : LetOrConst BindingList`),
/// Early Errors: „It is a Syntax Error if Initializer is not present."
///
/// **Nur `const`** — `let x;` ist erlaubt. Und die Regel haengt an der
/// **BindingList**, nicht an einem einzelnen Eintrag: `const x = 1, y;`
/// scheitert, weil `y` keinen Initialisierer hat.
///
/// GEMESSEN (node 22):
///   `const x;`                  -> SyntaxError
///   `let x;`                    -> ok
///   `const x = 1, y;`           -> SyntaxError
///   `for (const x;;) { break }` -> SyntaxError
///   `for (const x of []) {}`    -> ok  (anderes Productionsglied!)
///   `for (let x;;) { break }`   -> ok
///   `const {a = 1} = {};`       -> ok  (der **Muster**-Default ersetzt
///                                  keinen Initialisierer der Liste)
///   `const [a] = [];`           -> ok
fn fruehfehler_const_initialisierer(
    p: &Parser,
    art: DeklArt,
    eintraege: &[(Muster, Option<Ausdruck>)],
) -> Result<(), JsFehler> {
    // J35: Dubletten **innerhalb** einer BindingList. ES2023 14.3.1:
    // „It is a Syntax Error if the BoundNames of BindingList contains any
    // duplicate entries." GEMESSEN (node 22):
    //   `const {a} = {}, a = 2;`       -> SyntaxError
    //   `let a = 1, a = 2;`            -> SyntaxError
    //   `const {a, b} = {}, {a} = {};` -> SyntaxError
    // Die Regel gilt fuer `let` **und** `const`; `var a = 1, a = 2` ist
    // erlaubt (`var` ist nicht lexikalisch gebunden).
    if art != DeklArt::Var {
        let mut gesehen: Vec<String> = Vec::new();
        for (m, _) in eintraege {
            for n in crate::compile::muster_namen(m) {
                // J34: der Platzhalter einer Luecke (`[, , a]`) ist ein
                // **Compile**-Name, kein vomProgramm gebundener Name. Er
                // wird hier absichtlich nicht gezaehlt — sonst waere jede
                // Luecke eine „Dublette".
                if n == crate::compile::LOCH {
                    continue;
                }
                if gesehen.contains(&n) {
                    return Err(
                        p.fehler(format!("'{n}' ist in dieser Deklaration zweimal gebunden"))
                    );
                }
                gesehen.push(n);
            }
        }
    }
    if art != DeklArt::Const {
        return Ok(());
    }
    for (m, init) in eintraege {
        if init.is_none() {
            let n = crate::compile::muster_namen(m)
                .first()
                .cloned()
                .unwrap_or_default();
            return Err(p.fehler(format!("'const' braucht einen Anfangswert ('const {n};')")));
        }
    }
    Ok(())
}

fn parse_dekl_liste(p: &mut Parser) -> Result<Vec<(Muster, Option<Ausdruck>)>, JsFehler> {
    let mut v = Vec::new();
    loop {
        let m = parse_muster(p)?;
        let init = if p.frisst_zeichen(Zeichen::Gleich) {
            Some(parse_zuweisung(p)?)
        } else {
            None
        };
        v.push((m, init));
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
    }
    Ok(v)
}

fn parse_for(p: &mut Parser) -> Result<Anweisung, JsFehler> {
    p.weiter(); // for
    // Release 10.10.2026: `for await (… of …)` (ES2018 14.7.5).
    let warte = p.frisst_wort(Schluessel::Await);
    p.erwartet_zeichen(Zeichen::LRunde, "'(' nach for")?;
    if p.frisst_wort(Schluessel::Await) {
        return Err(p.fehler("for await wird nicht unterstuetzt".to_string()));
    }
    // Leerer Kopf `for (;;)`.
    if p.frisst_zeichen(Zeichen::Semikolon) {
        let bed = if p.ist_zeichen(Zeichen::Semikolon) {
            None
        } else {
            Some(parse_ausdruck(p)?)
        };
        p.erwartet_zeichen(Zeichen::Semikolon, "';'")?;
        let schritt = if p.ist_zeichen(Zeichen::RRunde) {
            None
        } else {
            Some(parse_ausdruck(p)?)
        };
        p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
        let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
        return Ok(Anweisung::Fuer {
            kopf: FuerKopf::Keiner,
            bedingung: bed,
            schritt,
            rumpf,
        });
    }
    // Deklaration im Kopf?
    let dekl_art = match &p.aktuell().art {
        TokenArt::Wort(Schluessel::Var) => Some(DeklArt::Var),
        // J53: `let` ist **nicht immer** ein Deklarationsschluessel. Die
        // Norm (ES2023 14.7.1) schliesst die Lookaheads `let [` und
        // `async of` aus; danach entscheidet, ob ein Bindungsname folgt.
        TokenArt::Wort(Schluessel::Let) => {
            if let_ist_deklaration(p) {
                Some(DeklArt::Let)
            } else {
                None
            }
        }
        TokenArt::Wort(Schluessel::Const) => Some(DeklArt::Const),
        _ => None,
    };
    if let Some(art) = dekl_art {
        // Voraus-Schau: `for (var X in/of ...)` — ein Muster, dann in/of.
        let merk = p.pos;
        p.weiter();
        let muster = parse_muster(p)?;
        let of_in = if p.ist_wort(Schluessel::Of) || p.aktuell_bezeichner().as_deref() == Some("of")
        {
            Some(true)
        } else if p.ist_wort(Schluessel::In) {
            Some(false)
        } else {
            None
        };
        if let Some(of) = of_in {
            p.weiter();
            let quelle = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
            let ao = match art {
                DeklArt::Var => DeklArtOpt::Var,
                DeklArt::Let => DeklArtOpt::Let,
                DeklArt::Const => DeklArtOpt::Const,
            };
            return Ok(Anweisung::FuerIn {
                warte,
                art: ao,
                ziel: muster,
                quelle,
                of,
                rumpf,
            });
        }
        // Sonst klassisch: Rest der Deklaration ab Muster erneut parsen.
        p.pos = merk;
        p.weiter();
        let eintraege = parse_dekl_liste(p)?;
        // J35: `for (const x;;)` ist ein Fehler, `for (const x of [])` nicht
        // (das ist ein anderes Productionsglied und wird oben zurueckgegeben,
        // **bevor** diese Stelle erreicht wird).
        fruehfehler_const_initialisierer(p, art, &eintraege)?;
        // `for (a in b)` ohne Dekl-Art: hier nicht (wir haben Art) — aber
        // `in` nach Deklaration ohne Muster? Abgedeckt: nur EIN Eintrag ohne
        // Init darf for-in sein.
        if eintraege.len() == 1 && eintraege[0].1.is_none() {
            if p.ist_wort(Schluessel::In) {
                p.weiter();
                let quelle = parse_ausdruck(p)?;
                p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
                let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
                let ao = match art {
                    DeklArt::Var => DeklArtOpt::Var,
                    DeklArt::Let => DeklArtOpt::Let,
                    DeklArt::Const => DeklArtOpt::Const,
                };
                let ziel = eintraege.into_iter().next().map(|(m, _)| m).unwrap();
                return Ok(Anweisung::FuerIn {
                warte,
                    art: ao,
                    ziel,
                    quelle,
                    of: false,
                    rumpf,
                });
            }
            if p.ist_wort(Schluessel::Of) || p.aktuell_bezeichner().as_deref() == Some("of") {
                p.weiter();
                let quelle = parse_ausdruck(p)?;
                p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
                let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
                let ao = match art {
                    DeklArt::Var => DeklArtOpt::Var,
                    DeklArt::Let => DeklArtOpt::Let,
                    DeklArt::Const => DeklArtOpt::Const,
                };
                let ziel = eintraege.into_iter().next().map(|(m, _)| m).unwrap();
                return Ok(Anweisung::FuerIn {
                warte,
                    art: ao,
                    ziel,
                    quelle,
                    of: true,
                    rumpf,
                });
            }
        }
        p.erwartet_zeichen(Zeichen::Semikolon, "';'")?;
        let bed = if p.ist_zeichen(Zeichen::Semikolon) {
            None
        } else {
            Some(parse_ausdruck(p)?)
        };
        p.erwartet_zeichen(Zeichen::Semikolon, "';'")?;
        let schritt = if p.ist_zeichen(Zeichen::RRunde) {
            None
        } else {
            Some(parse_ausdruck(p)?)
        };
        p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
        let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
        return Ok(Anweisung::Fuer {
            kopf: FuerKopf::Deklaration { art, eintraege },
            bedingung: bed,
            schritt,
            rumpf,
        });
    }
    // Ausdruck-Kopf: `for (a in b)`, `for (a of b)` oder klassisch.
    // `in` ist auf Kopf-Ebene verboten (wird danach als for-in erkannt).
    let start = p.pos;
    let expr = parse_ausdruck_in(p, false)?;
    if p.ist_wort(Schluessel::In) {
        p.weiter();
        let quelle = parse_ausdruck(p)?;
        p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
        let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
        let ziel = muster_aus_ausdruck(expr)
            .ok_or_else(|| p.fehler("ungueltiges for-in-Ziel".to_string()))?;
        let _ = start;
        return Ok(Anweisung::FuerIn {
                warte,
            art: DeklArtOpt::Keins,
            ziel,
            quelle,
            of: false,
            rumpf,
        });
    }
    if p.ist_wort(Schluessel::Of) || p.aktuell_bezeichner().as_deref() == Some("of") {
        p.weiter();
        let quelle = parse_ausdruck(p)?;
        p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
        let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
        // J32: `for (arguments of …)` ist dasselbe einfache Ziel wie
        // `arguments = 1` (ES2023 13.15.1). GEMESSEN (node 22):
        // `'use strict'; for (arguments of []);` -> SyntaxError,
        // `for (arguments.x of [])` -> gueltig, ohne `use strict` ->
        // gueltig.
        strict_ziel_verboten(p, &expr)?;
        let ziel = muster_aus_ausdruck(expr)
            .ok_or_else(|| p.fehler("ungueltiges for-of-Ziel".to_string()))?;
        return Ok(Anweisung::FuerIn {
                warte,
            art: DeklArtOpt::Keins,
            ziel,
            quelle,
            of: true,
            rumpf,
        });
    }
    p.erwartet_zeichen(Zeichen::Semikolon, "';'")?;
    let bed = if p.ist_zeichen(Zeichen::Semikolon) {
        None
    } else {
        Some(parse_ausdruck(p)?)
    };
    p.erwartet_zeichen(Zeichen::Semikolon, "';'")?;
    let schritt = if p.ist_zeichen(Zeichen::RRunde) {
        None
    } else {
        Some(parse_ausdruck(p)?)
    };
    p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
    let rumpf = Box::new(parse_rumpf_anweisung(p, "Schleife")?);
    Ok(Anweisung::Fuer {
        kopf: FuerKopf::Ausdruck(expr),
        bedingung: bed,
        schritt,
        rumpf,
    })
}

/// Wandelt einen bereits geparsten Ausdruck in ein Zuweisungsmuster um
/// (fuer `for (a.b in c)`, `for ([x] of y)`).
fn muster_aus_ausdruck(e: Ausdruck) -> Option<Muster> {
    match e {
        Ausdruck::Name(n) => Some(Muster::Name(n)),
        Ausdruck::Feld(_) | Ausdruck::Gegenstand(_) => {
            // Vereinfachung: Destrukturierung im for-Kopf ohne Deklaration
            // wird nicht unterstuetzt (selten, dokumentiert).
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Muster (Destrukturierung)
// ---------------------------------------------------------------------------

fn parse_muster(p: &mut Parser) -> Result<Muster, JsFehler> {
    // J35: `eval`/`arguments` sind im strikten Code keine Bindungsnamen.
    // GEMESSEN (node 22) — siehe `muster_name_pruefen` (sieben Proben).
    if let Some(n) = naechster_muster_name(p) {
        muster_name_pruefen(p, &n)?;
    }
    let _w = p.tief().map_err(|e| e)?;
    let m = parse_muster_tief(p)?;
    p.tiefe = p.tiefe.saturating_sub(1);
    Ok(m)
}

fn parse_muster_tief(p: &mut Parser) -> Result<Muster, JsFehler> {
    // KEIN `=`-Verbrauch hier: `= expr` gehoert in Deklarationen und
    // Parametern zum AEUSSEREN Handler (Initialisierung), in
    // Destrukturierungs-Armen zu deren expliziten Standard-Zweigen.
    // (Frueher fraß diese Stelle das `=` von `let x = 5` als Default.)
    parse_muster_atom(p)
}

fn parse_muster_atom(p: &mut Parser) -> Result<Muster, JsFehler> {
    if p.frisst_zeichen(Zeichen::Punkte3) {
        let inner = parse_muster_atom(p)?;
        return Ok(Muster::Rest(Box::new(inner)));
    }
    if p.ist_zeichen(Zeichen::LEckig) {
        p.weiter();
        let mut teile = Vec::new();
        while !p.ist_zeichen(Zeichen::REckig) && !p.ist_ende() {
            if p.frisst_zeichen(Zeichen::Komma) {
                teile.push(FeldMuster {
                    // J34: Platzhalter je Luecke. Release 10.10.2026: ein
                    // **fester** Name (`compile::LOCH`), den weder
                    // `muster_namen` noch die Bindung je sieht. Vorher
                    // hiess er `_loch_<i>` und wurde echt gebunden —
                    // GEMESSEN (github.com, app-runtime-Modul): `let [, t] =
                    // …, [, n, ...i] = …` in EINEM Block scheiterte mit
                    // „'_loch_0' ist in diesem Block zweimal als let/const
                    // deklariert" (node: gueltig), und `[, a] = q` schrieb
                    // eine globale Variable `_loch_0`.
                    muster: Muster::Name(crate::compile::LOCH.to_string()),
                    standard: None,
                });
                continue;
            }
            if p.ist_zeichen(Zeichen::Punkte3) {
                p.weiter();
                let inner = parse_muster(p)?;
                teile.push(FeldMuster {
                    muster: Muster::Rest(Box::new(inner)),
                    standard: None,
                });
                // Rest muss letztes sein, aber tolerant weiterlesen lassen.
            } else {
                let m = parse_muster(p)?;
                let standard = if p.frisst_zeichen(Zeichen::Gleich) {
                    Some(Box::new(parse_zuweisung(p)?))
                } else {
                    None
                };
                teile.push(FeldMuster {
                    muster: m,
                    standard,
                });
            }
            if !p.frisst_zeichen(Zeichen::Komma) {
                break;
            }
        }
        p.erwartet_zeichen(Zeichen::REckig, "']'")?;
        // J32: ein `BindingRestElement` muss das **letzte** Element sein
        // (ES2023 13.3.3, `ArrayBindingPattern`:
        //   `[ Elisionopt BindingRestElement? Elisionopt ]`
        //   `[ BindingElementList , Elisionopt BindingRestElement? ]`
        // — ein Rest steht nie in der Mitte).
        fruehfehler_rest_letztes(
            p,
            &teile
                .iter()
                .map(|f| matches!(f.muster, Muster::Rest(_)))
                .collect::<Vec<bool>>(),
        )?;
        return Ok(Muster::Feld(teile));
    }
    if p.ist_zeichen(Zeichen::LGeschweift) {
        p.weiter();
        let mut teile = Vec::new();
        while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
            if p.ist_zeichen(Zeichen::Punkte3) {
                p.weiter();
                let inner = parse_muster(p)?;
                teile.push(GegenstandMuster {
                    schluessel: EigenschaftName::Text(String::new()),
                    muster: Muster::Rest(Box::new(inner)),
                    standard: None,
                    kurz: false,
                });
                if !p.frisst_zeichen(Zeichen::Komma) {
                    break;
                }
                continue;
            }
            let name = parse_eigenschaft_name(p)?;
            // Kurzform `{a}`, `{a = 1}`, `{a: muster}`, `{a: muster = 1}`.
            if p.frisst_zeichen(Zeichen::DoppelPunkt) {
                let m = parse_muster(p)?;
                let standard = if p.frisst_zeichen(Zeichen::Gleich) {
                    Some(Box::new(parse_zuweisung(p)?))
                } else {
                    None
                };
                teile.push(GegenstandMuster {
                    schluessel: name,
                    muster: m,
                    standard,
                    kurz: false,
                });
            } else {
                let schluessel_text = match &name {
                    EigenschaftName::Text(s) => Some(s.clone()),
                    _ => None,
                };
                let standard = if p.frisst_zeichen(Zeichen::Gleich) {
                    Some(Box::new(parse_zuweisung(p)?))
                } else {
                    None
                };
                let muster = match schluessel_text {
                    Some(s) => Muster::Name(s),
                    None => {
                        return Err(
                            p.fehler("berechneter Schluessel braucht ': Muster'".to_string())
                        );
                    }
                };
                teile.push(GegenstandMuster {
                    schluessel: name,
                    muster,
                    standard,
                    kurz: true,
                });
            }
            if !p.frisst_zeichen(Zeichen::Komma) {
                break;
            }
        }
        p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
        // J32: dasselbe fuer das **Objektmuster** (ES2023 13.3.3,
        // `ObjectBindingPattern` — dort ist es `BindingRestProperty`, und
        // auch sie steht nur am Ende).
        //
        // GEMESSEN (node 22), alle vier:
        //   `function({...a, b}) {}`  -> SyntaxError
        //   `function({...a}) {}`     -> gueltig
        fruehfehler_rest_letztes(
            p,
            &teile
                .iter()
                .map(|g| matches!(g.muster, Muster::Rest(_)))
                .collect::<Vec<bool>>(),
        )?;
        return Ok(Muster::Gegenstand(teile));
    }
    // J32: ein reserviertes Wort ist **kein** `BindingIdentifier`
    // (ES2023 13.1.1). GEMESSEN (node 22), sieben von sieben: `var this = 1`,
    // `var finally = 1`, `for (var this of []) {}` — und unescaped genauso wie
    // `var \u0074his`, weil der `Lexer` den Escape schon aufgeloest hat.
    //
    // GEMESSEN, warum die Pruefung **hier** steht: `parse_muster` ist der
    // einzige Ort fuer `var`/`let`/`const`, **Parameter**, `catch` und
    // `for`-Koepfe — eine Liste statt fuenf Stellen. Und GEMESSEN, warum
    // **nicht** in `frisst_name()`: das liest auch **Eigenschaftsnamen**, und
    // `o.finally = 1` ist in node **gueltig**.
    if let Some(s) = ist_bindungsname_verboten(&p.aktuell().art, p.strikt, p.in_async) {
        return Err(p.fehler(format!("'{}' ist kein gueltiger Bindungsname", s)));
    }
    match p.frisst_name() {
        // J32: die **Gegenregel** zu `in_klasse`. Im Generatorrumpf ist
        // `yield` der Operator; als **Bindung** ist es verboten (ES2023
        // 14.4.1). GEMESSEN (node 22):
        //   `class C { *#gen(){ var yield } }` -> SyntaxError
        //   `function* g(){ var yield }`       -> SyntaxError
        //   `class C { *gen(){ yield 1 } }`    -> gueltig
        //
        // GEMESSEN, warum **hier** und nicht in `var`/Parameter getrennt:
        // `parse_dekl_liste` (alle `var`/`let`/`const`) und
        // `parse_param_liste` (alle Parameter) gehen beide ueber
        // `parse_muster` — das ist **eine** Stelle fuer Deklarationen,
        // Parameter, `for`-Koepfe und `catch`. Drei Stellen wären drei
        // Regeln und drei Wege, sie zu vergessen.
        //
        // `await` ist auch hier **nicht** dabei: es ist nur in `async`-
        // Kontexten ein Bindungsname-Verbot, und ein Generator ist keiner.
        Some(n) if n == "yield" && (p.in_generator || p.in_klasse) => {
            Err(p.fehler(if p.in_generator {
                "'yield' ist im Generatorrumpf keine Bindung".to_string()
            } else {
                // J32: dieselbe Regel im Klassenrumpf, weil Klassen strikter
                // Code sind. GEMESSEN (node 22):
                // `class C { m(){ var yield; } }` -> SyntaxError.
                //
                // GEMESSEN, warum das der **erste** Entwurf nicht sah: bei
                // `class C { m(){ var yield = 1; return yield } }` fiel die
                // Referenz auf, und der Test war gruen — der Fehler lag an
                // der *Bindung*, nicht an der Referenz. Ein Test, der nur
                // die Referenz prueft, haelt einen halben Fehler fuer
                // erledigt.
                "'yield' ist in einem Klassenrumpf strikter Code".to_string()
            }))
        }
        Some(n) => Ok(Muster::Name(n)),
        None => Err(p.fehler("erwartet: Variablenname oder Muster".to_string())),
    }
}

/// J32: ein `RestElement` muss das **letzte** Element eines Feld- oder
/// Objektmusters sein.
///
/// ES2023 13.3.3 (`ArrayBindingPattern`, `ObjectBindingPattern`): ein Rest
/// steht nur am Ende — im Feld- und im Objektmuster gleichermassen.
///
/// GEMESSEN (node 22) — **drei** scheitern, **sechs** sind gueltig:
///   `function*([...a, b]) {}`            -> SyntaxError
///   `function*([...a, b] = [1]) {}`      -> SyntaxError
///   `function({...a, b}) {}`             -> SyntaxError
///   `function*([...[a], b]) {}`          -> SyntaxError
///   `var [x, ...y, z] = [1,2,3]`         -> SyntaxError
///   `function*([a, ...b]) {}`            -> gueltig
///   `function*([...a]) {}`               -> gueltig
///   `function*([a, b]) {}`               -> gueltig
///   `function*([, ...a]) {}`             -> gueltig
///   `function({...a}) {}`                -> gueltig
///   `var [x, ...y] = [1,2,3]`            -> gueltig
///
/// Der Lauf ueber die echte Suite hatte **38** rote Faelle dieser Form
/// (`test/language/expressions/async-generator/dstr/dflt-ary-ptrn-rest-
/// not-final-*` und die Geschwister in `statements/`, `expressions/`).
///
/// Die Stellen kommen als Liste der „ist ein Rest"-Bit an — eine Liste,
/// weil **beide** Musterarten geprueft werden muessen und die beiden
/// Elementtypen (`FeldMuster`, `GegenstandMuster`) verschieden sind.
/// J32: Steht der aktuelle Token **unmittelbar** hinter einem `#`?
///
/// `PrivateName :: # IdentifierName` erlaubt keinen Leerraum dazwischen
/// (ES2023 3.11.28). GEMESSEN (node 22): `class C { set # x(v) {} }`,
/// `class C { # x; }` und `class C { #\tx; }` sind Syntaxfehler,
/// `class C { #x; }` und `class C { set\n#x(v) {} }` sind gueltig.
///
/// GEMESSEN, warum **Zeile und Spalte** und kein Byte-Offset: die Token
/// tragen keine absolute Quellposition, und ein Offset waere ein Eingriff in
/// den `Lexer` (Fremddatei). Die Spalte zaehlt **Zeichen** (der Lexer laeuft
/// ueber `Vec<char>`), ein Tab zaeht als **eins** — deshalb fallen genau die
/// Faelle mit Leerzeichen, Tab und Kommentar durch, und der Zeilenumbruch
/// (`set\n#x`) faellt nicht, weil die Zeile mitgeprueft wird.
/// J35: Steht am **Anfang** des Funktionsrumpfs die Direktive
/// `"use strict"`? ES2023 14.1 (`Directive Prologue`).
///
/// Der Aufrufer steht auf dem `{`, das den Rumpf oeffnet.
///
/// **WARUM VORAUSSCHAUEN und nicht NACHSEHEN:** die Early Errors des
/// strikten Codes (`eval`/`arguments` als Ziel, `delete` eines Namens,
/// Oktal-Literale, doppelte Parameternamen) muessen **waehrend** des
/// Parsens des Rumpfes feuern. Wer sie erst nach dem Parsen prueft, hat
/// einen syntaktisch gueltigen Baum und damit nichts.
///
/// **KONSERVATIV bei der Form der Direktive:**
///   `function f(){ "use strict"; }`   -> streng
///   `function f(){ "use strict" }`     -> streng (ASI vor `}`)
///   `function f(){ "use strict" + 1 }` -> **keine** Direktive: der Ausdruck
///      laeuft weiter (String mit `+`), es gibt keinen Prolog
/// Deshalb muss auf die Direktive ein `;`, ein `}` oder ein Zeilenumbruch
/// folgen.
fn vorschau_use_strict(p: &Parser) -> bool {
    if !matches!(p.aktuell().art, TokenArt::Zeichen(Zeichen::LGeschweift)) {
        return false;
    }
    match p.tokens.get(p.pos + 1) {
        Some(t) => {
            if !matches!(&t.art, TokenArt::Zeichenkette(s) if s == "use strict") {
                return false;
            }
            match p.tokens.get(p.pos + 2) {
                None => false,
                Some(x) => match &x.art {
                    TokenArt::Zeichen(Zeichen::Semikolon) => true,
                    TokenArt::Zeichen(Zeichen::RGeschweift) => true,
                    // Zeilenumbruch -> ASI setzt den Trenner.
                    _ => x.zeile > t.zeile,
                },
            }
        }
        None => false,
    }
}

/// J35: Ist das aktuelle Zahl-Token ein **Legacy-Oktal**? ES2023 12.9.3 /
/// `NumericLiteral : DecimalLiteral`, Early Errors: „It is a Syntax Error if
/// the source code matching this production is strict mode code and the
/// numeric value of LiteralInteger is not the same value as the mathematical
/// integer value of DecimalInteger" — das trifft jedes Oktal-Literal.
///
/// GEMESSEN (node 22), **zehn** Proben:
///   `010`, `08`, `00`, `01`, `0777` -> SyntaxError (strikt)
///   `0.5`, `0e1`, `0x1F`, `0b101`, `0o17` -> gueltig (strikt)
///   `010` ohne Direktive -> `8`
///
/// **GEMESSEN/GEFUNDEN, warum der Parser und nicht der Lexer:** der Lexer
/// rechnet `0777` zu `511` und **darf** das auch (sloppy ist erlaubt); die
/// Entscheidung braucht den strikten Kontext, den nur der Parser kennt.
/// Deshalb wird hier der **Quelltext** an der Tokenstelle gelesen — das
/// Token selbst traegt nur den Zahlenwert.
/// J35: `eval`/`arguments` als Bindungsname — zentrale Stelle.
///
/// GEMESSEN (node 22), **sechs** Faelle (alle `SyntaxError`):
///   `"use strict"; var arguments = 1`       `"use strict"; var eval = 1`
///   `"use strict"; let arguments = 1`       `"use strict"; function f(arguments){}`
///   `"use strict"; var f = (arguments) => 1`  `class C { m(){ var arguments = 1 } }`
///
/// **UND DIE GEGENPROBE:** `"use strict"; var undefined = 1` ist erlaubt
/// (node `1`) — `undefined` ist ein Identifier, kein ReservedWord.
///
/// **WARUM AN ALLEN MUSTER-STELLEN und nicht bei `var`/`let`/`const`:**
/// die Regel gilt fuer **jede** Bindung, auch fuer Parameter und
/// `catch (arguments)`. Eine Liste von Aufrufern wird erfahrungsgemaess
/// unvollstaendig gefuellt; eine Pruefung am Muster ist **eine** Stelle.
fn muster_name_pruefen(p: &Parser, n: &str) -> Result<(), JsFehler> {
    if name_als_bindung_verboten(n, p.strikt) {
        return Err(p.fehler(format!(
            "'{n}' darf im strikten Code kein Bindungsname sein"
        )));
    }
    Ok(())
}

/// J35: Name des **einfachen** Musters an der aktuellen Stelle, ohne zu
/// parsen. Nur der Text wird gebraucht (`eval`, `arguments`).
/// J53: Ist ein `let` an dieser Stelle ein **Deklarationsschluessel**?
///
/// GEMESSEN (`node 22`, eine Zeile je Fall, jede in einem eigenen
/// Prozess — ein `let` aus einem frueheren Fall macht spaetere zu
/// Syntaxfehlern, wenn man alles in eine Datei schreibt):
///
/// ```text
///   for ( let in obj ) ;      -> gueltig   (`let` ist ein Bezeichner)
///   for ( let = 3; ; )         -> gueltig   (Zuweisung an `let`)
///   for ( let; ; )             -> gueltig   (`let` als IdentifierReference)
///   for ( let [a] of [[1]] );  -> gueltig   (Deklaration, Feldmuster)
///   for ( let {a} of [{a:1}] ) -> gueltig   (Deklaration, Objektmuster)
///   for ( let async of [1] );  -> gueltig   (Deklaration, Name `async`)
///   for ( let of [1] );        -> SyntaxError
///   for ( let (a) of [1] );    -> SyntaxError
/// ```
///
/// Die letzte beiden Zeilen sind der Grund fuer die Ausnahme: `of` fuehrt
/// auf den **Deklarations**-Zweig (dort ist `of` kein Bindungsname, also
/// SyntaxError — genau wie in node), `in` nicht. Alles andere, was keinen
/// Bindungsnamen beginnen kann (`;`, `)`, `=`, `(`, Operatoren), laesst
/// `let` als Bezeichner durch.
fn let_ist_deklaration(p: &mut Parser) -> bool {
    let merk = p.pos;
    p.weiter();
    let art = p.aktuell().art.clone();
    p.pos = merk;
    match art {
        // Ein Name darf folgen — auch `let`, `async`, `yield`.
        TokenArt::Bezeichner(_) => true,
        // Die beiden Muster-oeffner.
        TokenArt::Zeichen(Zeichen::LEckig) | TokenArt::Zeichen(Zeichen::LGeschweift) => true,
        // `of` gehoert zum Deklarationszweig (siehe oben).
        TokenArt::Wort(Schluessel::Of) => true,
        // `in` und alles, was keinen Bindungsnamen eroeffnet: `let` ist ein
        // Bezeichner.
        _ => false,
    }
}

fn naechster_muster_name(p: &Parser) -> Option<String> {
    match &p.aktuell().art {
        TokenArt::Bezeichner(n) => Some(n.clone()),
        _ => None,
    }
}

fn ist_legacy_oktal(p: &Parser) -> bool {
    let t = p.aktuell();
    if !matches!(t.art, TokenArt::Zahl(_)) {
        return false;
    }
    let start = p.offset_von(t.zeile, t.spalte);
    let q = &p.quelle;
    if start >= q.len() {
        return false;
    }
    if q.get(start).copied() != Some('0') {
        return false;
    }
    match q.get(start + 1).copied() {
        // `0x1F`, `0b101`, `0o17` sind ausdruecklich erlaubt.
        Some('x') | Some('X') | Some('b') | Some('B') | Some('o') | Some('O') => false,
        Some(c) if c.is_ascii_digit() => true,
        // `0.5`, `0e1`, `0`, `0n`? — kein Legacy-Oktal.
        _ => false,
    }
}

/// J35: `eval` und `arguments` duerfen im **strikten** Code keine
/// Bindungsnamen sein (ES2023 13.1.1).
///
/// GEMESSEN (node 22), **sechs**:
///   `"use strict"; var arguments = 1`      -> SyntaxError
///   `"use strict"; var eval = 1`           -> SyntaxError
///   `"use strict"; let arguments = 1`      -> SyntaxError
///   `"use strict"; function f(arguments){}`-> SyntaxError
///   `"use strict"; var f = (arguments) => 1` -> SyntaxError
///   `class C { m(){ var arguments = 1 } }` -> SyntaxError
///
/// **UND DIE GEGENPROBE**, die man leicht verliert:
///   `"use strict"; var undefined = 1` -> **erlaubt** (node `1`).
///   `undefined` ist ein Identifier, kein ReservedWord — die Regel
///   12.1.1/13.1.1 nennt nur `eval` und `arguments`.
fn name_als_bindung_verboten(n: &str, strikt: bool) -> bool {
    strikt && (n == "eval" || n == "arguments")
}

fn privat_name_direkt(p: &Parser) -> bool {
    if p.pos == 0 {
        return true;
    }
    let vor = &p.tokens[p.pos - 1];
    if !matches!(vor.art, TokenArt::Zeichen(Zeichen::Raute)) {
        return true;
    }
    let jetzt = &p.tokens[p.pos];
    vor.zeile == jetzt.zeile && jetzt.spalte == vor.spalte.saturating_add(1)
}

fn fruehfehler_rest_letztes(p: &Parser, rest_stellen: &[bool]) -> Result<(), JsFehler> {
    let n = rest_stellen.len();
    for (i, ist_rest) in rest_stellen.iter().enumerate() {
        if *ist_rest && i + 1 != n {
            return Err(
                p.fehler("ein Rest-Element muss das letzte Element des Musters sein".to_string())
            );
        }
    }
    Ok(())
}

fn parse_eigenschaft_name(p: &mut Parser) -> Result<EigenschaftName, JsFehler> {
    if p.ist_zeichen(Zeichen::LEckig) {
        p.weiter();
        let e = parse_zuweisung(p)?;
        p.erwartet_zeichen(Zeichen::REckig, "']'")?;
        return Ok(EigenschaftName::Berechnet(Box::new(e)));
    }
    // J11: `get #x() {}` / `set #x(v) {}` / ein nacktes `#x` im
    // Klassenrumpf. GEMESSEN vorher: `class C { #p = 1; get #q(){ … } }`
    // brach mit „erwartet: Eigenschaftsname" ab, weil nach `get` kein
    // Bezeichner, sondern die Raute stand.
    if p.ist_zeichen(Zeichen::Raute) {
        p.weiter();
        // J32: `PrivateName :: # IdentifierName` — zwischen `#` und dem
        // Namen darf **kein** Leerraum stehen (ES2023 3.11.28 / 12.7.1).
        // GEMESSEN (node 22), **acht** scheitern:
        //   `class C { set # x(v) {} }`   `class C { # x; }`
        //   `class C { get # x() {} }`   `class C { m(){ return this.# x } }`
        //   `class C { #\tx; }`          `class C { static # y = 1 }`
        //   `class C { * # z() {} }`     `class C { async # w() {} }`
        // und **gueltig** sind die ohne Leerraum — auch ueber einen
        // Zeilenumbruch: `class C { set\n#x(v) {} }` laeuft in node durch.
        // Die Regel ist also rein **waagerecht**.
        if !privat_name_direkt(p) {
            return Err(p.fehler(
                "zwischen '#' und dem privaten Namen darf kein Leerraum stehen".to_string(),
            ));
        }
        let n = p
            .frisst_name()
            .ok_or_else(|| p.fehler("Name nach '#' erwartet".to_string()))?;
        // J32: **Deklaration** — `AllPrivateNamesValid` (ES2023 15.7.1)
        // sammelt die privaten Namen dieses Klassenkoerpers.
        p.privat_vergeben.push(format!("#{n}"));
        return Ok(EigenschaftName::Text(format!("#{n}")));
    }
    match &p.aktuell().art.clone() {
        TokenArt::Zeichenkette(s) => {
            let s = s.clone();
            p.weiter();
            Ok(EigenschaftName::Text(s))
        }
        TokenArt::Zahl(z) => {
            let z = *z;
            p.weiter();
            Ok(EigenschaftName::Zahl(z))
        }
        _ => {
            let _art = p.aktuell().art.clone();
            match p.frisst_name() {
                Some(n) => Ok(EigenschaftName::Text(n)),
                None => Err(p.fehler("erwartet: Eigenschaftsname".to_string())),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Funktionen / Klassen
// ---------------------------------------------------------------------------

/// Parst ab `function` (oder nach `async`). Gibt (Name, Bau) zurueck.
fn parse_funktion(
    p: &mut Parser,
    _pfeil_kontext: bool,
) -> Result<(Option<String>, FunktionsBau), JsFehler> {
    // J19: Position vor `function` merken, damit der Quelltext der
    // Funktion abgeschnitten werden kann (`Function.prototype.toString`).
    let q_start = p.pos;
    p.weiter(); // function
    // V3: `function*` ist erlaubt (Generator).
    let generator = p.frisst_zeichen(Zeichen::Stern);
    // J32: ein Funktionsname ist ein `BindingIdentifier` — also dieselbe
    // ReservedWord-Regel wie bei `parse_muster` (ES2023 13.1.1). GEMESSEN
    // (node 22): `function this() {}` -> SyntaxError, `function get() {}`
    // -> gueltig (`get` ist kein ReservedWord).
    let name = match &p.aktuell().art {
        _ if let Some(s) = ist_bindungsname_verboten(&p.aktuell().art, p.strikt, p.in_async) => {
            return Err(p.fehler(format!("'{}' ist kein gueltiger Funktionsname", s)));
        }
        _ => p.frisst_name(),
    };
    // J32: das Generator-Flag steht **vor** der Parameterliste, denn die
    // Parameter eines Generators sind ein `[Yield]`-Kontext.
    // GEMESSEN (node 22): `function* g(yield) { }` -> SyntaxError. Die
    // erste Fassung setzte das Flag erst vor dem Rumpf und liess das
    // deshalb durch.
    //
    // `in_klasse` wird hier **zurueckgenommen**: `yield` als
    // `IdentifierReference` ist im Rumpf eines Generators erlaubt, auch
    // wenn die Funktion in einer Klasse steht (die erste Fassung dieser
    // Regel hat `tests/j16_bundel.rs::generator_methode_im_klassenrumpf`
    // rot gemacht — gegen einen echten github-Bündel).
    let war_in_klasse = p.in_klasse;
    let war_in_generator = p.in_generator;
    let war_super_erlaubt = p.super_erlaubt;
    p.super_erlaubt = false;
    // J32: die Sicherung fuer die **Parameterliste** liegt ausserhalb
    // des `if` — die Ruecksetzung steht nach `parse_param_liste` und
    // muss deshalb denselben Scope haben. Erste Fassung: drinnen ->
    // `E0425` an beiden Stellen, die Crate baute nicht.
    // GEMESSEN, warum **nicht** nur `if generator`: der Rumpf einer
    // **verschachtelten einfachen** Funktion ist kein Generatorrumpf, dort ist
    // `yield` ein Bezeichner. `node 22`:
    // `function* g(a = function(){ return yield }) {}` ist **gueltig**. Setzte
    // man das Flag nur im Generatorzweig, bliebe es dort stehen und der Fall
    // waere faelschlich rot.
    let param_vorher = p.in_generator_param;
    p.in_generator_param = generator;
    if generator {
        p.in_klasse = false;
        p.in_generator = true;
    }
    // J32: eine verschachtelte Funktion hat ihre **eigene**
    // `arguments`-Bindung — dort ist `arguments` erlaubt. GEMESSEN
    // (node 22): `class C { x = function(){ return arguments } }` ist
    // gueltig, `class C { x = () => arguments }` nicht (ein Pfeil hat keine
    // eigene Bindung). Deshalb wird der Feld-Zaehler hier zurueckgenommen.
    let feld_tiefe = p.feld_initialisierer;
    p.feld_initialisierer = 0;
    let (params, rest) = parse_param_liste(p)?;
    // J32: der **Rumpf ist nie eine Parameterliste** — dort wird das Flag
    // **genullt**, und erst **nach** dem Rumpf wiederhergestellt.
    //
    // GEMESSEN, warum das Reihenfolge-Detail alles ist: mit einem blossen
    // „vor dem Rumpf zuruecksetzen" bekam der Rumpf einer **verschachtelten
    // Funktion** den Wert der **aeusseren** Parameterliste zurueck —
    // `function* g(a = function(){ return yield }) {}` wurde dadurch rot,
    // obwohl `node 22` es laesst. Und mit einem blossen „vor dem Rumpf
    // loeschen, danach nicht zuruecksetzen" waere nach der verschachtelten
    // Funktion der Rest der aeusseren Parameterliste ungeprueft geblieben
    // (`function* g(a = function(){}, b = yield) {}`).
    p.in_generator_param = false;
    // J35: die Direktive im Rumpf macht den **ganzen** Rumpf zu striktem
    // Code — fuer den **Parser**, nicht nur fuer die Laufzeit.
    //
    // GEMESSEN (node 22), fuenfzehn Proben. Vorher stuften **sieben** davon
    // falsch ein:
    //   `function f(){ "use strict"; arguments = 1 }` -> SyntaxError
    //   `function f(a, a){ "use strict" }`            -> SyntaxError
    //   `function f(){ "use strict"; return 010 }`    -> SyntaxError
    //   `function f(){ "use strict"; var x = 1; delete x }` -> SyntaxError
    //   `function f(){ "use strict"; var arguments = 1 }`    -> SyntaxError
    //   `class C { m(){ arguments = 1 } }`            -> SyntaxError
    //   `class C { m(){ return 010 } }`               -> SyntaxError
    //
    // **DIE LAUFZEIT HATTE ES SCHON RICHTIG** (GEMESSEN):
    //   `function f(){ "use strict"; eval = 42 }` -> ReferenceError (strict)
    // Der Compiler las die Direktive, der **Parser** nicht — zwei Stellen,
    // eine Regel. Das ist die uebliche Ursache fuer „der Motor lehnt ab,
    // aber zu spaet und mit dem falschen Fehler".
    // J47: die Direktive wird in `parse_funktionsrumpf` gelesen — dort
    // entsteht **jeder** Rumpf. Hier bleibt nur das Ruecksetzen fuer den
    // Fall, dass eine gewaehnliche Funktion zusaetzlich noch etwas braucht.
    let rumpf_strikt = vorschau_use_strict(p);
    let strikt_vorher = p.strikt;
    let _ = rumpf_strikt;
    let rumpf = parse_funktionsrumpf(p)?;
    p.strikt = strikt_vorher;
    p.in_generator_param = param_vorher;
    p.feld_initialisierer = feld_tiefe;
    p.in_klasse = war_in_klasse;
    p.in_generator = war_in_generator;
    p.super_erlaubt = war_super_erlaubt;
    // J27: Early Error (ES2023 15.7.1 / 15.2.1 / 15.5.1) — ein
    // Parametername darf nicht zugleich im Rumpf lexikalisch
    // deklariert sein.
    //
    // GEMESSEN (node 22):
    //   `(async function foo (bar) { let bar; });`
    //     -> `SyntaxError: Identifier 'bar' has already been declared`
    // Der Lauf ueber die echte test262-Suite hatte **43** rote Faelle
    // mit `Test262: This statement should not be evaluated` aus
    // `test/language/expressions/async-function/early-errors-*`.
    //
    // GEMESSEN, warum **nicht** auch die Dubletten untereinander hier
    // geprueft werden: node laesst `async function f(a, a) {}` in
    // sloppy **zu** (es lehnt erst `function h(a, a = 1) {}` und die
    // Strict-Mode-Variante ab). Die beiden Regeln sind getrennt; die
    // Dubletten fuer Pfeilfunktionen stehen in
    // `pfeil_doppelte_namen_pruefen`.
    //
    // DIESE STELLE IST DER GEMEINSAME BAUER fuer `function`, `async
    // function` und `function*` — eine Aenderung, drei Faelle.
    fruehfehler_parameter_gegen_rumpf(p, &params, rest.as_ref(), &rumpf)?;
    // J31: und die zweite Early Error derselben Gruppe — `use strict` im
    // Rumpf bei nicht-einfacher Parameterliste (ES2023 14.2.1). GEMESSEN
    // (node 22): `function f({a}) { 'use strict'; }` ist ein SyntaxError,
    // `function f(a, b) { 'use strict'; }` nicht. Der Lauf über die echte
    // Suite hatte **41** rote Fälle dieser Form.
    fruehfehler_strict_mit_parametern(p, &params, rest.as_ref(), &rumpf)?;
    // J27: doppelte Parameternamen — die Regel aus ES2023 15.2.1.
    //
    //   StrictFormalParameters (Pfeile, async, Methoden)
    //     „It is a Syntax Error if BoundNames of FormalParameters
    //      contains any duplicate elements."        -> **immer**
    //   FormalParameters (gewoehnliche Funktionen, Generator)
    //     „It is a Syntax Error if IsSimpleParameterList is false and
    //      BoundNames contains any duplicate elements."
    //                                                  -> nur bei Default
    //                                                     oder Rest
    //
    // GEMESSEN (node 22), alle sechs:
    //   `function h(a, a) {}`                 -> erlaubt (sloppy)
    //   `function h(a, a = 1) {}`             -> SyntaxError
    //   `"use strict"; function h(a, a) {}`   -> SyntaxError
    //   `async function f(a, a) {}`           -> erlaubt (sloppy)
    //   `function* g(a, a) {}`                -> erlaubt (sloppy)
    //   `function h(a, ...a) {}`              -> SyntaxError
    //
    // Der Fall `async-function/named-dflt-params-duplicates.js` ist die
    // nicht-einfache Liste in einer async-Funktion; die Suite hatte
    // **42** rote Faelle dieser Form.
    //
    // `async function` wird in node **nicht** immer abgelehnt — die
    // `StrictFormalParameters`-Regel gilt fuer async nur, wenn Strict gilt
    // oder die Liste nicht einfach ist. Deshalb steht `asynchron` hier
    // **nicht** als Bedingung.
    // J35: `rumpf_strikt` zaehlt mit — `p.strikt` ist wieder der
    // Ausgangswert. GEMESSEN (node 22): `function f(a, a){ "use strict" }`
    // ist ein SyntaxError, ohne die Direktive nicht.
    // J35: fuer die Dubletten die **Nachsehen**-Variante
    // (`rumpf_hat_use_strict(&rumpf)`) — sie liest den geparsten Rumpf und
    // ist damit unabhaengig vom Token-Vorausblick; `p.strikt` ist wieder
    // der Ausgangswert. GEMESSEN (node 22):
    // `function f(a, a){ "use strict" }` -> SyntaxError, ohne Direktive nicht.
    if p.strikt
        || rumpf_strikt
        || rumpf_hat_use_strict(&rumpf)
        || !ist_einfache_parameterliste(&params, rest.as_ref())
    {
        doppelte_parameternamen_pruefen(p, &params, rest.as_ref())?;
    }
    Ok((
        name,
        FunktionsBau {
            params,
            rest,
            rumpf,
            pfeil: false,
            asynchron: false,
            // J32: die **gelesene** Variable, nicht `false`.
            //
            // GEMESSEN, was ein hartes `false` kostet: `function* g(){ yield 5 }
            // lieferte `TypeError: kein Zugriff auf Undefiniert` statt `5`, und
            // `tests/j16_bundel.rs::yield_mit_feld_und_objektliteral` (ein Test gegen
            // einen **echten** github-Bündel, `catalyst-…js` 2:1244) wurde rot.
            // Der Motor kannte an der Stelle zwar `p.in_generator` (der Rumpf war der
            // richtige), aber `FunktionsBau.generator` war `false` — der Uebersetzer
            // sah also eine ganz normale Funktion.
            generator,
            konstruktor: false,
            // J19: der Quelltext dieser Funktion.
            quelle: p.quelle_von_bis(q_start, p.pos),
            zeile: zeile_von(p, q_start),
        },
    ))
}

fn parse_param_liste(p: &mut Parser) -> Result<(Vec<Muster>, Option<Muster>), JsFehler> {
    p.erwartet_zeichen(Zeichen::LRunde, "'(' bei Parametern")?;
    let mut params = Vec::new();
    let mut rest = None;
    while !p.ist_zeichen(Zeichen::RRunde) && !p.ist_ende() {
        if p.ist_zeichen(Zeichen::Punkte3) {
            p.weiter();
            rest = Some(parse_muster(p)?);
            break;
        }
        // `this`-Typannotation o.ae. gibt es nicht; `,`-Loecher tolerieren.
        if p.frisst_zeichen(Zeichen::Komma) {
            continue;
        }
        let m = parse_muster(p)?;
        let m = if p.frisst_zeichen(Zeichen::Gleich) {
            let std = parse_zuweisung(p)?;
            Muster::Standard(Box::new(m), Box::new(std))
        } else {
            m
        };
        params.push(m);
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
    }
    p.erwartet_zeichen(Zeichen::RRunde, "')' bei Parametern")?;
    Ok((params, rest))
}

/// J42: der Rumpf einer **Funktion** — `return` ist hier erlaubt.
///
/// **WARUM EINE EIGENE FUNKTION und nicht ein Flag an jeder Stelle:**
/// `parse_block_anweisungen` wird auch fuer Klassenkoerper benutzt, und
/// dort ist `return` **verboten** (`class C { f = (return 1) }` ist ein
/// SyntaxError). Wer das Flag an den Aufrufern drehte, haette sieben Stellen
/// — und eine Regel an sieben Stellen wird sechsmal vergessen (so erging es
/// mir in diesem Durchgang beim Rumpf von J36).
fn parse_funktionsrumpf(p: &mut Parser) -> Result<Vec<Anweisung>, JsFehler> {
    let vorher = p.in_funktionsrumpf;
    p.in_funktionsrumpf = true;
    // J47: **jeder** Funktionsrumpf hat einen `Directive Prologue` (ES2023
    // 14.1) — nicht nur der einer gewöhnlichen Funktion. Die Regel stand
    // vorher nur in `parse_funktion`, damit fehlte sie in **sieben**formen.
    //
    // GEMESSEN (node 22), alle sieben **SyntaxError**:
    //   `((m) => {'use strict'; { function f(){} function f(){}; } })`
    //   `({m(){'use strict'; { function f(){} function f(){}; } }})`
    //   `({get x(){'use strict'; { function f(){} function f(){}; } }})`
    //   `({set x(v){'use strict'; { function f(){} function f(){}; } }})`
    //   `({*m(){'use strict'; { function f(){} function f(){}; } }})`
    //   `({async m(){'use strict'; { function f(){} function f(){}; } }})`
    //   `(async (m) => {'use strict'; { function f(){} function f(){}; } })`
    //
    // **UND DIE GEGENPROBEN** (dieselben Quellen **ohne** Direktive sind
    // erlaubt — eine Ausnahme je Rumpf, nicht ueberall):
    //   `({m(){ { function f(){} function f(){}; } }})` -> **erlaubt**
    //   `((m) => { { function f(){} function f(){}; } })` -> **erlaubt**
    //   `class C { m(){ { function f(){} function f(){}; } } }`
    //        -> SyntaxError (ein Klassenrumpf ist **immer** strikt)
    let rumpf_strikt = vorschau_use_strict(p);
    let strikt_vorher = p.strikt;
    p.strikt = p.strikt || rumpf_strikt;
    let r = parse_block_anweisungen(p);
    p.strikt = strikt_vorher;
    p.in_funktionsrumpf = vorher;
    r
}

fn parse_block_anweisungen(p: &mut Parser) -> Result<Vec<Anweisung>, JsFehler> {
    p.erwartet_zeichen(Zeichen::LGeschweift, "'{'")?;
    let mut v = Vec::new();
    while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
        v.push(parse_anweisung(p)?);
    }
    p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
    // J34: die Neudeklarations-Regeln **dieses** Blocks. Sie stehen hier,
    // weil das die einzige Stelle ist, an der eine vollstaendige
    // `StatementList` vorliegt — und weil die Norm sie an die Liste bindet,
    // nicht an einzelne Anweisungen.
    fruehfehler_neudeklaration(p, &v, ListenArt::Rumpf)?;
    Ok(v)
}

/// J34: ES2023 14.2.1 (`Block`, Early Errors) und 14.3.1
/// (`LexicalDeclaration`).
///
/// * „It is a Syntax Error if the LexicallyDeclaredNames of StatementList
///   contains any duplicate entries."
/// * „It is a Syntax Error if any element of the LexicallyDeclaredNames of
///   StatementList also occurs in the VarDeclaredNames of StatementList."
/// * 14.3.1 `LexicalDeclaration`: „It is a Syntax Error if the BoundNames of
///   BindingList contains any element that is also the BoundName of a
///   VarDeclaredNames of StatementList."
///
/// GEMESSEN (node 22), **acht** scheitern und **vier** sind gueltig:
///   `{ const f = 1; var f; }`          -> SyntaxError
///   `{ var f = 1; const f; }`          -> SyntaxError  (die **andere**
///                                          Richtung!)
///   `{ let f = 1; var f; }`            -> SyntaxError
///   `{ var f = 1; let f; }`            -> SyntaxError
///   `{ const f = 1; const f = 2; }`     -> SyntaxError  (lexikalisch/lexikalisch)
///   `{ let a = 1; let a = 2 }`         -> SyntaxError
///   `function g(){ const f = 1; var f; }` -> SyntaxError
///   `{ const f = 1; var f = 2; }`      -> SyntaxError  (der Initialisierer
///                                          ist **ohne Bedeutung**)
///   `{ let a = 1; } { let a = 2; }`    -> gueltig  (verschiedene Bloecke!)
///   `var a = 1; var a = 2`             -> gueltig  (var darf doppelt)
///   `function f(a){ var a }`           -> gueltig  (Parameter + var)
///   `function f(a){ let a }`           -> SyntaxError (J27, schon da)
///
/// **DER STREIKTE MODUS IST IRRELEVANT** — das war die Ueberraschung, und
/// sie steht deshalb hier: `{ const f = 1; var f; }` scheitert in node
/// **genauso** ohne `'use strict'`. Der test262-Fall heisst
/// `redeclaration-error-from-within-strict-mode-function-const.js` und
/// traegt `flags: [noStrict]`, was den Namen erklaert: der Test **nutzt**
/// strikten Code, die Regel **haengt nicht** daran.
///
/// GEMESSEN, warum die Pruefung **am Ende** der Liste steht: `var` und
/// `let` koennen in beliebiger Reihenfolge kommen, und `{ var f = 1; let f; }`
/// ist genauso ein Fehler wie die umgekehrte Reihenfolge. Ein Vorwaerts-
/// Vergleich haette nur die eine Richtung gefunden.
///
/// GEMESSEN, warum `var` **rekursiv** gesammelt wird: `VarDeclaredNames`
/// eines Blocks umfasst auch die `var` in **untergeordneten** Anweisungen
/// (`if (1) { var f } let f;` ist ein Fehler). Verschachtelte **Funktionen**
/// und Klassen werden **nicht** betreten — dort gilt die Regel der jeweiligen
/// eigenen Liste.
/// J47: welche Art von Anweisungsliste die Neudeklarations-Regeln bekommen.
///
/// **Block** = jede Liste **in geschweiften Klammern**: `Block`,
/// `try`/`catch`/`finally` und der `CaseBlock` eines `switch`.
/// **Rumpf** = Rumpf einer Funktion, Skript, Pfeilrumpf.
///
/// Der Unterschied ist bei Funktionsdeklarationen **echt** — und er ist die
/// ganze Arbeit wert, denn ohne ihn akzeptiert der Motor
/// `switch (0) { case 1: async function* f() {} default: function f() {} }`.
/// **Rumpf** = die Liste am Rumpf einer Funktion, am Skript und an einem
/// Pfeilfunktionrumpf.
///
/// GEMESSEN (node 22), warum es zwei Arten geben muss — eine **Matrix** aus
/// **21 Paaren je sieben Containern**, also **147** Einzelmessungen:
///
/// ```text
///               ff   gg  afaf agag kk   ll   vv
/// block         ok   ERR ERR  ERR  ERR  ERR  ok
/// caseblock     ok   ERR ERR  ERR  ERR  ERR  ok
/// rumpf         ok   ok  ok   ok   ERR  ERR  ok
/// skript        ok   ok  ok   ok   ERR  ERR  ok
/// ```
///
/// Die beiden Listen unterscheiden sich **nur bei Funktionsdeklarationen** —
/// und nur im **lockeren** Modus:
///   `{ function* f(){} function* f(){} }`      -> SyntaxError
///   `(function(){ function* f(){} function* f(){} })` -> **erlaubt**
///   `{ 'use strict'; function* f(){} function* f(){} }` -> **erlaubt**
/// `„`streng„` heisst also **lockerer** fuer doppelte Funktionen — die
/// strenge Fassung schaltet die Web-Legacy-Regeln ab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenArt {
    /// Jede Liste **in geschweiften Klammern**: `Block`, `try`/`catch`/
    /// `finally` und der `CaseBlock` eines `switch`.
    Block,
    /// Der Rumpf einer Funktion, das Skript, ein Pfeilrumpf.
    Rumpf,
}

fn fruehfehler_neudeklaration(
    p: &Parser,
    anweisungen: &[Anweisung],
    art: ListenArt,
) -> Result<(), JsFehler> {
    // GEMESSEN, warum die `MitZeile`-Huelle hier abgeschaltet werden
    // muss: `parse_anweisung` verpackt **jedes** Statement in
    // `Anweisung::MitZeile { zeile, innen }` (die Zeilennummer des
    // Test262-Harnesses). Ohne das Abschaelen sieht diese Pruefung nur
    // Varianten mit der Nummer 0 und erkennt **keine** Deklaration —
    // die erste Fassung liess deshalb alle acht Faelle durch.
    let innen: Vec<&Anweisung> = anweisungen
        .iter()
        .map(|a| match a {
            Anweisung::MitZeile { innen, .. } => innen.as_ref(),
            sonst => sonst,
        })
        .collect();
    let mut lexikalisch: Vec<String> = Vec::new();
    for a in innen.iter().copied() {
        match a {
            Anweisung::Deklaration { art, eintraege } if *art != DeklArt::Var => {
                for (m, _) in eintraege {
                    for n in crate::compile::muster_namen(m) {
                        if lexikalisch.contains(&n) {
                            return Err(p.fehler(format!(
                                "'{n}' ist in diesem Block zweimal als let/const deklariert"
                            )));
                        }
                        lexikalisch.push(n);
                    }
                }
            }
            Anweisung::Klasse { name: Some(n), .. } => {
                if lexikalisch.contains(n) {
                    return Err(p.fehler(format!(
                        "'{n}' ist in diesem Block zweimal als let/const deklariert"
                    )));
                }
                lexikalisch.push(n.clone());
            }
            _ => {}
        }
    }
    // J47: die **HoistableDeclaration**-Namen dieser Liste. Sie sind weder
    // `let`/`const`/`class` noch `var` — sie sind der Grund, weshalb es
    // `ListenArt` gibt (siehe dort die Matrix).
    //
    // GEMESSEN (node 22), **funfzehn** Einzelproben zu den Grenzen:
    //   `{ let f; function f(){} }`            -> SyntaxError  (let vs. f)
    //   `(function(){ let f; function f(){} })` -> SyntaxError  (gilt auch da)
    //   `{ function f(){} class f{} }`         -> SyntaxError
    //   `{ function f(){} function f(){} }`    -> **erlaubt**
    //   `{ function f(){} var f; }`            -> SyntaxError  (locker!)
    //   `{ var f; function f(){} }`            -> SyntaxError  (beide Reihen!)
    //   `({m(){'use strict'; function f(){} var f;}})` -> **erlaubt**
    let mut gewoehnlich: Vec<String> = Vec::new();
    let mut gehoben: Vec<String> = Vec::new();
    for a in innen.iter().copied() {
        if let Anweisung::Funktion {
            name: Some(n),
            funktion,
        } = a
        {
            // `Anweisung::MitZeile` ist oben schon abgeschuert — deshalb
            // sieht der Abgleich hier die **innere** Anweisung.
            if funktion.generator || funktion.asynchron {
                gehoben.push(n.clone());
            } else {
                gewoehnlich.push(n.clone());
            }
        }
    }
    // Release 09.10.2026: eine Menge statt einer Liste — `contains` auf der
    // Liste war quadratisch (youtube.com: Zehntausende `var` in EINEM Rumpf).
    let mut variablen: std::collections::HashSet<String> = std::collections::HashSet::new();
    var_namen_sammeln(anweisungen, &mut variablen);

    if art == ListenArt::Block {
        if p.strikt {
            // **Strikter Block.** Alle Funktionsdeklarationen sind lexikalisch.
            //
            // GEMESSEN (node 22), **vierzehn** Proben:
            //   `'use strict'; { function f(){} function f(){} }`   -> SyntaxError
            //   `(function(){'use strict'; { function f(){} function f(){} }})`
            //        -> SyntaxError
            //   `({m(){'use strict'; { function f(){} function f(){} }}})`
            //        -> SyntaxError
            //   `'use strict'; { { function f(){} function f(){} } }`  -> SyntaxError
            //   `'use strict'; function f(){} function f(){}` (kein Block!)
            //        -> **erlaubt**
            //   `{ switch(0){case 1: { function f(){} function f(){} }} }` (locker)
            //        -> **erlaubt**
            let mut zusammen = lexikalisch.clone();
            for n in gehoben.iter().chain(gewoehnlich.iter()) {
                if zusammen.contains(n) {
                    return Err(p.fehler(format!(
                        "'{n}' ist in diesem Block zweimal als let/const deklariert"
                    )));
                }
                zusammen.push(n.clone());
            }
            for n in &zusammen {
                if variablen.contains(n) {
                    return Err(p.fehler(format!(
                        "'{n}' ist zugleich let/const und var im selben Block"
                    )));
                }
            }
            return Ok(());
        }

        // **Lockerer Block.** Gemessen (node 22), die genau vier Faelle:
        //   `{ function* f(){} function* f(){} }`        -> SyntaxError
        //   `{ async function f(){} function f(){} }`    -> SyntaxError
        //   `{ class f{} function f(){} }`               -> SyntaxError
        //   `{ function f(){} var f; }`                  -> SyntaxError
        //   `{ function f(){} function f(){} }`          -> **erlaubt**
        //   `{ var f; var f; }`                          -> **erlaubt**
        //
        // Also: `let`/`const`/`class`/`function*`/`async function`/
        // `async function*` bilden **eine** Menge, die keine Dubletten
        // haben darf und die gegen `function` **und** `var` abgesichert
        // ist. Zwei gewöhnliche `function f` und zwei `var f` sind je
        // **erlaubt** — eine Dublettenregel je Gruppe waere zu streng.
        //
        // **UND DER LOCKERE `CaseBlock`** (der Anlass dieses Punktes) folgt
        // denselben Regeln — mit **neunundzwanzig** eigenen Proben belegt:
        //   `switch(0){case 1: function f(){} default: function f(){}}` -> ok
        //   `switch(0){case 1: function* f(){} default: function* f{}{}}`
        //        -> SyntaxError
        //   `switch(0){case 1: function f(){}; var f;}` -> SyntaxError
        //   `switch(0){case 1: var f; function f(){}}` -> SyntaxError
        let mut zusammen = lexikalisch.clone();
        for n in &gehoben {
            if zusammen.contains(n) {
                return Err(p.fehler(format!(
                    "'{n}' ist in diesem Block zweimal als let/const deklariert"
                )));
            }
            zusammen.push(n.clone());
        }
        for n in &zusammen {
            if gewoehnlich.contains(n) {
                return Err(p.fehler(format!(
                    "'{n}' ist zugleich let/const und function im selben Block"
                )));
            }
        }
        for n in &zusammen {
            if variablen.contains(n) {
                return Err(p.fehler(format!(
                    "'{n}' ist zugleich let/const und var im selben Block"
                )));
            }
        }
        for n in &gewoehnlich {
            if variablen.contains(n) {
                return Err(p.fehler(format!(
                    "'{n}' ist zugleich function und var im selben Block"
                )));
            }
        }
        return Ok(());
    }

    // **Rumpf-Liste**: hier sind die Funktionsdeklarationen schlicht
    // `var`-Namen — Doppelten sind erlaubt (GEMESSEN: `function f(){}
    // function* f(){}` und `function f(){} var f;` sind beide erlaubt).
    for n in &lexikalisch {
        if variablen.contains(n) || gehoben.contains(n) || gewoehnlich.contains(n) {
            return Err(p.fehler(format!(
                "'{n}' ist zugleich let/const und var im selben Block"
            )));
        }
    }
    Ok(())
}

/// J34: `VarDeclaredNames` einer Anweisungsliste — rekursiv, aber **nicht**
/// in verschachtelte Funktionen oder Klassen.
fn var_namen_sammeln(anweisungen: &[Anweisung], raus: &mut std::collections::HashSet<String>) {
    for a in anweisungen {
        match a {
            Anweisung::Deklaration { art, eintraege } if *art == DeklArt::Var => {
                for (m, _) in eintraege {
                    for n in crate::compile::muster_namen(m) {
                        raus.insert(n);
                    }
                }
            }
            Anweisung::Block(v) => var_namen_sammeln(v, raus),
            Anweisung::Wenn { dann, sonst, .. } => {
                var_namen_sammeln(std::slice::from_ref(dann), raus);
                if let Some(o) = sonst {
                    var_namen_sammeln(std::slice::from_ref(o), raus);
                }
            }
            Anweisung::Solange { rumpf, .. } => {
                var_namen_sammeln(std::slice::from_ref(rumpf), raus)
            }
            Anweisung::Fuer {
                kopf:
                    FuerKopf::Deklaration {
                        art: DeklArt::Var,
                        eintraege,
                    },
                rumpf,
                ..
            } => {
                // GEMESSEN (node 22): `(function(){ { for (var i = 0; i < 0; i++ ) {} let i; } })`
                //   -> SyntaxError. `var i` steht im **Kopf**, nicht in einem
                // Statement — die erste Fassung sammelte nur `Deklaration`-
                // Anweisungen und liess den Fall durch.
                for (m, _) in eintraege {
                    for n in crate::compile::muster_namen(m) {
                        raus.insert(n);
                    }
                }
                var_namen_sammeln(std::slice::from_ref(rumpf), raus);
            }
            Anweisung::Fuer { rumpf, .. } => var_namen_sammeln(std::slice::from_ref(rumpf), raus),
            Anweisung::FuerIn {
                art: DeklArtOpt::Var,
                ziel,
                rumpf,
                ..
            } => {
                for n in crate::compile::muster_namen(ziel) {
                    raus.insert(n);
                }
                var_namen_sammeln(std::slice::from_ref(rumpf), raus);
            }
            Anweisung::FuerIn { rumpf, .. } => var_namen_sammeln(std::slice::from_ref(rumpf), raus),
            Anweisung::Auswahl { faelle, .. } => {
                for (_, v) in faelle {
                    var_namen_sammeln(v, raus);
                }
            }
            Anweisung::Versuch {
                rumpf,
                fang,
                endlich,
                ..
            } => {
                var_namen_sammeln(rumpf, raus);
                var_namen_sammeln(fang, raus);
                var_namen_sammeln(endlich, raus);
            }
            Anweisung::Marke(_, innen) => var_namen_sammeln(std::slice::from_ref(innen), raus),
            Anweisung::Mit(_, innen) => var_namen_sammeln(std::slice::from_ref(innen), raus),
            Anweisung::MitZeile { innen, .. } => {
                var_namen_sammeln(std::slice::from_ref(innen), raus)
            }
            // Funktionen und Klassen: ihre eigenen Listen gelten, nicht
            // diese — GEMESSEN: `function f(){ var a } let a = 1;` ist
            // gueltig.
            _ => {}
        }
    }
}

/// Parst ab `class`-Name (nachdem `class` verbraucht ist).
fn parse_klasse_rumpf(
    p: &mut Parser,
) -> Result<(Option<String>, Option<Ausdruck>, Vec<KlassenElement>), JsFehler> {
    // J32: **Klassen sind strikter Code** (ES2023 10.2.1: „All parts of a
    // ClassDeclaration or a ClassExpression are strict mode code"), und in
    // striktem Code ist `yield` als `IdentifierReference` (ES2023 12.1.1) ein
    // Syntaxfehler. Das Flag gilt fuer **Kopf und Rumpf** — deshalb steht
    // es hier **vor** dem Namen, und der alte Wert wird danach
    // wiederhergestellt (Klassen koennen einander verschachtelt sein).
    //
    // GEMESSEN (node 22), **vier** scheitern und **sechs** sind gueltig —
    // und die sechs sind der Grund, warum die Regel nicht „jedes Wort
    // `yield`" heissen darf:
    //   `var C = class yield {}`                 -> SyntaxError  (Klassenname)
    //   `class C extends yield {}`                -> SyntaxError  (Elternausdruck)
    //   `class C { m(){ var yield = 1; … } }`    -> SyntaxError  (Bindung)
    //   `class C { static x = yield }`           -> SyntaxError  (Referenz)
    //   `var C = class { yield(){} }`            -> gueltig  (Methoden**name**)
    //   `var C = class { get yield(){…} }`       -> gueltig  (Eigenschaftsname)
    //   `function f(){ var yield = 1 }`          -> gueltig  (kein Klassenrumpf)
    //   `var o = { yield: 1 }`                   -> gueltig  (Eigenschaftsname)
    //
    // `await` ist **nicht** dabei: es ist nur in `async`-Kontexten
    // reserviert, und eine Klasse ist keiner. GEMESSEN: `class await {}`,
    // `class { await(){} }` und `class C { m(){ var await = 1 } }` sind alle
    // gueltig.
    let war_in_klasse = p.in_klasse;
    p.in_klasse = true;
    // J32: eine (verschachtelte) Klasse darf `super()` in ihrem
    // Konstruktor — GEMESSEN (node 22):
    //   `class C extends O { x = class extends P { constructor(){ super() } } }`
    //   -> gueltig. Deshalb wird der Zaehler **hier** auf 0 gesetzt und
    //   danach wiederhergestellt (die aeussere Feldinitialisierung laeuft
    //   weiter). Anders waere der Konstruktor der inneren Klasse rot.
    let feld_super_der_äusseren = p.feld_super;
    p.feld_super = 0;
    // J16: `class extends X { … }` (Klassen**ausdruck** ohne Namen, in
    // echten Bundeln haeufig: `let C = class extends HTMLElement {}`).
    // GEMESSEN vorher: `frisst_name()` las `extends` als **Namen**, dann
    // wurde `HTMLElement` statt `{` erwartet ->
    // `Syntaxfehler: erwartet: '{' nach class`. node 22 laeuft durch.
    let name = if p.ist_wort(Schluessel::Extends) {
        None
    } else {
        // J32: der Klassenname ist ein `BindingIdentifier` — ReservedWords
        // sind verboten (ES2023 13.1.1). GEMESSEN (node 22):
        // `var C = class this {}` -> SyntaxError, `var C = class get {}`
        // -> gueltig.
        match &p.aktuell().art {
            _ if let Some(s) =
                ist_bindungsname_verboten(&p.aktuell().art, p.strikt, p.in_async) =>
            {
                return Err(p.fehler(format!("'{}' ist kein gueltiger Klassenname", s)));
            }
            _ => p.frisst_name(),
        }
    };
    // J32: der **Klassenname** selbst. `class yield {}` ist ein SyntaxError,
    // weil `yield` in striktem Code kein `BindingIdentifier` ist (ES2023
    // 15.7.1 zusammen mit 10.2.1). GEMESSEN (node 22): `var C = class
    // yield {}` -> SyntaxError.
    //
    // GEMESSEN, warum der Ausdruck-Zweig hier nicht hilft: der Name wird
    // von `frisst_name()` gelesen und **nie** zu einem `Ausdruck::Name` —
    // die Pruefung im Ausdruck-Zweig (siehe dort) sieht ihn also nie.
    if name.as_deref() == Some("yield") {
        return Err(p.fehler("'yield' ist in einem Klassenrumpf strikter Code".to_string()));
    }
    // J32: die private Umgebung einer (verschachtelten) Klasse ist eine
    // **eigene** (ES2023 15.7.1 Schritt 6/8) — die beiden Buecher werden
    // deshalb gesichert und leer neu angelegt.
    //
    // GEMESSEN/ENTSCHIEDEN: das Sichern steht **vor** der `extends`-Abfrage,
    // nicht darin. In der ersten Fassung lag es im Zweig, und dann waren die
    // Variablen fuer eine Klasse **ohne** `extends` nicht vorhanden — der
    // Motor brauchte also `class C { m(){ return this.#x } }` (der haeufigste
    // Fall!) und liess den Crate nicht uebersetzen.
    let privat_ausgelieht_vorher = std::mem::take(&mut p.privat_ausgelieht);
    // J39: die Deklarationen der **aeusseren** Klassen muessen sichtbar
    // bleiben. `std::mem::take` leerte die Liste und nahm damit genau das
    // weg, was die Regel braucht.
    //
    // GEMESSEN (node 22), **vier** Proben:
    //   `class C { get #m(){} B = class { method(c){ return c.#m } } }`
    //     -> `test262` (die innere Klasse **sieht** das aeussere `#m`)
    //   `class C { #m = 1; B = class { method(c){ return c.#m } } }` -> `1`
    //   `class A { #g(){} B = class { method(a){ return a.#g() } } }` -> `1`
    //   `class C { B = class { method(c){ return c.#m } } }`
    //     -> SyntaxError (es gibt **kein** aeusseres `#m`)
    //
    // Deshalb: Laenge merken, am Ende wieder auf die Laenge kuerzen. Die
    // Liste enthaelt dann waehrend des Rumpfes **alle** sichtbaren
    // Deklarationen — die inneren zuerst, die aeusseren danach.
    let privat_vergeben_laenge = p.privat_vergeben.len();
    let eltern = if p.frisst_wort(Schluessel::Extends) {
        // J16: der Elternausdruck ist ein **LeftHandSideExpression** —
        // auch `extends a.b.c` und `extends a[0]`. GEMESSEN an einem
        // echten github-Bündel (`j05`, 132 KiB):
        // `class extends tu.T{request(){…}}` brach mit
        // `erwartet: '{' nach class` ab, weil nur der Primaerausdruck
        // (`tu`) gelesen wurde und `.T` stehen blieb. node 22 laeuft durch.
        //
        // J22: **`extends` ist auch ein Aufruf erlaubt** — die Spec sagt
        // `extends LeftHandSideExpression`, und das ist auch ein Aufruf.
        // GEMESSEN an `index.ae4f4679716a7f81.js` (mdn, 192:2234):
        //   `class MDNSearchModal extends (0,n.J)(r.WF){ … }`
        // brach mit `erwartet: '{' nach class` ab (und `extends
        // ({m(){…}}).m()` ebenso). Vorher stand hier
        // `parse_kette_ohne_rruf` — **ohne** Aufruf, damit `extends A {`
        // den Rumpf nicht frisst. Der Aufruf braucht aber `(`; ein `{`
        // kann ihn nicht ausloesen, deshalb ist die Sorge unbegruendet.
        // J32: der Elternausdruck ist eine eigene Zone (ES2023 15.7.1).
        // Das Flag wird **nicht** in verschachtelten Klassen zurueckgesetzt
        // — `ContainsPrivate` ist syntaktisch und `node` lehnt auch
        // `class C extends (class { #x; m(){ return #x } }) {}` ab.
        let eltern = {
            let vorher = p.privat_verboten;
            p.privat_verboten = true;
            let e = parse_postfix(p);
            p.privat_verboten = vorher;
            Some(e?)
        };
        eltern
    } else {
        None
    };
    p.erwartet_zeichen(Zeichen::LGeschweift, "'{' nach class")?;
    // J35: **alle Teile** einer Klasse sind strikter Code (ES2023 10.2.1:
    // „All parts of a ClassDeclaration or a ClassExpression are strict mode
    // code"). Das ist der Punkt, an dem `p.strikt` fuer Methodenruempfe und
    // Feld-Initialisierer gesetzt wird — bisher nirgends, deshalb waren
    // Klassenmethoden **nicht** streng.
    //
    // GEMESSEN (node 22), **fuenf** Proben, alle `SyntaxError`:
    //   `class C { m(){ arguments = 1 } }`        -> SyntaxError
    //   `class C { m(){ return 010 } }`           -> SyntaxError
    //   `class C { m(){ var x = 1; delete x } }`  -> SyntaxError
    //   `class C { m(){ eval = 1 } }`             -> SyntaxError
    //   `class C { m(a, a){} }`                   -> SyntaxError
    //   `"use strict"; class C { m(a, a){} }`     -> SyntaxError
    //
    // **WICHTIG, GEMESSEN:** `class C { m(){} }` in **schlampigem** Code
    // bleibt gueltig, und `class C { m(a, a){} }` ebenfalls — die
    // Striktheit kommt **aus der Klasse**, nicht aus der Umgebung. Deshalb
    // wird sie hier **gesetzt** (und nachher wiederhergestellt), nicht
    // geerbet.
    let strikt_vorher = p.strikt;
    p.strikt = true;
    let abgeleitet = eltern.is_some();
    let mut elemente = Vec::new();
    // J46: „It is a Syntax Error if ContainsDuplicateConstructorMethods of
    // ClassBody is true" (ES2023 15.7.1). GEMESSEN (node 22):
    //   `class C { constructor(){} constructor(){} }` -> SyntaxError
    let mut konstruktoren = 0u32;
    while !p.ist_zeichen(Zeichen::RGeschweift) && !p.ist_ende() {
        if p.frisst_zeichen(Zeichen::Semikolon) {
            continue;
        }
        // J11: privater Name im Klassenrumpf — `#x = 1`, `#m() {}`,
        // `get #x() {}`, `static #s = 1`. Der Name geht mit `#` in die
        // Feldliste (`"#x"`), wie beim Zugriff.
        if p.ist_zeichen(Zeichen::Raute) {
            // OPUS_1: der Quelltext der Methode beginnt beim `#`
            // (GEMESSEN vorher: `String(o.#m)` -> `() { … }`, 26 Faelle).
            let q_start = p.pos;
            p.weiter();
            // J32: `PrivateName :: # IdentifierName` — zwischen `#` und dem
            // Namen darf **kein** Leerraum stehen (ES2023 3.11.28 / 12.7.1).
            // GEMESSEN (node 22), **acht** scheitern:
            //   `class C { set # x(v) {} }`   `class C { # x; }`
            //   `class C { get # x() {} }`   `class C { m(){ return this.# x } }`
            //   `class C { #\tx; }`          `class C { static # y = 1 }`
            //   `class C { * # z() {} }`     `class C { async # w() {} }`
            // und **gueltig** sind die ohne Leerraum — auch ueber einen
            // Zeilenumbruch: `class C { set\n#x(v) {} }` laeuft in node durch.
            // Die Regel ist also rein **waagerecht**.
            if !privat_name_direkt(p) {
                return Err(p.fehler(
                    "zwischen '#' und dem privaten Namen darf kein Leerraum stehen".to_string(),
                ));
            }
            let n = p
                .frisst_name()
                .ok_or_else(|| p.fehler("Name nach '#' erwartet".to_string()))?;
            // J32: **Deklaration** — `AllPrivateNamesValid` (ES2023 15.7.1)
            // sammelt die privaten Namen dieses Klassenkoerpers.
            p.privat_vergeben.push(format!("#{n}"));
            elemente.push(klassen_element(
                p,
                q_start,
                format!("#{n}"),
                false,
                false,
                abgeleitet,
            )?);
            continue;
        }
        let mut statisch = false;
        // `static` (+ optional Block `static { ... }` — nicht unterstuetzt).
        if (p.ist_wort(Schluessel::Static) || p.aktuell_bezeichner().as_deref() == Some("static"))
            && !folgender_ist_klammer(p)
        {
            // Vorsicht: Feld namens `static` (`static;` / `static = 1;`).
            let merk = p.pos;
            p.weiter();
            if p.ist_zeichen(Zeichen::LGeschweift) {
                // Release 10.10.2026: `static { … }` (ES2022). GEMESSEN
                // vorher (bbc.com, Next.js-Router `class J{static{…}}`): das
                // ganze Buendel scheiterte mit „statische Bloecke werden nicht
                // unterstuetzt".
                let q_start = p.pos;
                let koerper = parse_pfeil_koerper(p)?;
                let pfeil = pfeil_bau(p, q_start, Vec::new(), None, koerper, false);
                elemente.push(KlassenElement::Block {
                    ausdruck: Ausdruck::Aufruf {
                        ziel: Box::new(pfeil),
                        args: Vec::new(),
                        streu: Vec::new(),
                        optional: false,
                    },
                });
                continue;
            }
            if p.ist_zeichen(Zeichen::Semikolon) || p.ist_zeichen(Zeichen::Gleich) {
                // Feld namens "static".
                p.pos = merk;
            } else {
                statisch = true;
                // J11: `static #x = 1` / `static #m() {}`.
                if p.ist_zeichen(Zeichen::Raute) {
                    let q_start = p.pos; // OPUS_1: Quelltext ab `#`
                    p.weiter();
                    // J32: `PrivateName :: # IdentifierName` — zwischen `#` und dem
                    // Namen darf **kein** Leerraum stehen (ES2023 3.11.28 / 12.7.1).
                    // GEMESSEN (node 22), **acht** scheitern:
                    //   `class C { set # x(v) {} }`   `class C { # x; }`
                    //   `class C { get # x() {} }`   `class C { m(){ return this.# x } }`
                    //   `class C { #\tx; }`          `class C { static # y = 1 }`
                    //   `class C { * # z() {} }`     `class C { async # w() {} }`
                    // und **gueltig** sind die ohne Leerraum — auch ueber einen
                    // Zeilenumbruch: `class C { set\n#x(v) {} }` laeuft in node durch.
                    // Die Regel ist also rein **waagerecht**.
                    if !privat_name_direkt(p) {
                        return Err(p.fehler(
                            "zwischen '#' und dem privaten Namen darf kein Leerraum stehen"
                                .to_string(),
                        ));
                    }
                    let n = p
                        .frisst_name()
                        .ok_or_else(|| p.fehler("Name nach '#' erwartet".to_string()))?;
                    // J32: **Deklaration** — `AllPrivateNamesValid` (ES2023 15.7.1)
                    // sammelt die privaten Namen dieses Klassenkoerpers.
                    p.privat_vergeben.push(format!("#{n}"));
                    elemente.push(klassen_element(
                        p,
                        q_start,
                        format!("#{n}"),
                        true,
                        false,
                        abgeleitet,
                    )?);
                    continue;
                }
            }
        }
        // J16: Generator-Methode `*name() {}`. GEMESSEN an einem echten
        // github-Bundel (`j05`, 132 KiB): `*eachDirectDescendantNode(){…}`
        // brach mit `Syntaxfehler: erwartet: Eigenschaftsname` ab, weil der
        // Name gelesen wurde, bevor das `*` gesehen war. node 22 laeuft
        // durch. Das `*` steht **vor** dem Namen.
        let mut generator = false;
        if p.ist_zeichen(Zeichen::Stern) && !folgender_ist_klammer(p) {
            p.weiter();
            generator = true;
        }
        // get/set-Kontext.
        let mut merk = p.pos;
        // J19: der Name der Methode beginnt ihren Quelltext.
        let q_start = p.pos;
        let erster = p.frisst_name();
        // J6: `get`/`set` koennen auch **Feldnamen** sein
        // (`class C { get = 1 }`, `class C { set }`). Dann folgt `=`, `;`
        // oder `}` — und der Accessor-Zweig waere falsch. GEMESSEN vorher:
        // `class C{ get=1; … }` brach mit „erwartet: Eigenschaftsname" ab.
        let ist_get_set = matches!(erster.as_deref(), Some("get") | Some("set"))
            && !folgender_ist_klammer(p)
            && !p.ist_zeichen(Zeichen::Gleich)
            && !p.ist_zeichen(Zeichen::Semikolon)
            && !p.ist_zeichen(Zeichen::RGeschweift);
        if ist_get_set {
            let art_get = erster.as_deref() == Some("get");
            let q_start = p.pos; // J19: der Name beginnt den Quelltext
            let name = parse_eigenschaft_name(p)?;
            // J46: `get constructor` / `set constructor` sind SyntaxError —
            // **nicht** statisch (GEMESSEN, node 22):
            //   `class C { get constructor(){} }`  -> SyntaxError
            //   `class C { static get constructor(){} }` -> **erlaubt**
            if !statisch
                && matches!(&name, crate::ast::EigenschaftName::Text(t) if t == "constructor")
            {
                return Err(p.fehler(
                    "'constructor' ist nur als gewoehnliche Methode erlaubt (ES2023 15.7.1)"
                        .to_string(),
                ));
            }
            let (params, rest) = parse_param_liste(p)?;
            let rumpf = parse_funktionsrumpf(p)?;
            elemente.push(KlassenElement::Methode {
                name,
                funktion: FunktionsBau {
                    params,
                    rest,
                    rumpf,
                    pfeil: false,
                    asynchron: false,
                    generator: false,
                    konstruktor: false,
                    // J19: Quelltext der Methode.
                    quelle: p.quelle_von_bis(q_start, p.pos),
                    // J28: die Zeile der Getter-/Setter-Deklaration.
                    zeile: zeile_von(p, q_start),
                },
                statisch,
                art: if art_get {
                    MethodenArt::Getter
                } else {
                    MethodenArt::Setter
                },
            });
            continue;
        }
        // `async` vor Methode.
        let mut asynchron = false;
        let mut name_str = erster;
        if name_str.as_deref() == Some("async")
            && !folgender_ist_klammer(p)
            && !p.ist_zeichen(Zeichen::Gleich)
            && !p.ist_zeichen(Zeichen::Semikolon)
        {
            asynchron = true;
            if p.frisst_zeichen(Zeichen::Stern) {
                // J43: `async function*` wird **angenommen** und wie ein
                // Generator behandelt — die `async`-Semantik (Versprechen
                // statt Werten) fehlt noch.
                //
                // GEMESSEN (node 22), warum das ein Gewinn und kein
                // Risiko ist: `test/language/expressions/class/dstr/
                // async-gen-meth-ary-init-iter-get-err-array-prototype.js`
                // erwartet einen `TypeError` **beim Aufruf** — der entsteht
                // aus `delete Array.prototype[Symbol.iterator]` und dem
                // Feld-Muster der **Parameter**, also **vor** dem ersten
                // `yield`. Die Async-Semantik wird dort nie beruehrt.
                //
                // **UND: der Fall lag schon vorher falsch**, weil mein Motor
                // `async *method([x,y,z])` gar nicht parsen konnte und
                // statt des erwarteten `TypeError` einen SyntaxError
                // meldete. Ein SyntaxError ist kein `TypeError` — der Test
                // blieb rot, gleich aus welchem Grund.
                generator = true;
            }
            // Release 10.10.2026: der Name beginnt NACH `async`/`*`. Vorher
            // fiel ein Nicht-Bezeichner-Name auf `merk` zurueck — und das
            // stand VOR `async`, also wurde `async` selbst der Name.
            // GEMESSEN (node 22 gueltig, hier „erwartet: ';'"):
            //   `class A { async #p(){} }`  (github.com, react-core-Modul)
            //   `class A { async *[Symbol.asyncIterator](){} }`
            //   `class A { async 'q r'(){} }`  `class A { async 7(){} }`
            //   `class A { static async *#z(){} }`
            merk = p.pos;
            name_str = p.frisst_name();
        }
        let name = match name_str {
            Some(s) => EigenschaftName::Text(s),
            None => parse_eigenschaft_name_nach_klammer(p)?,
        };
        // Name kann auch Zahl/String/[berechnet] sein, wenn frisst_name
        // nichts fand — oben schon erledigt; hier Rest:
        let name = match name {
            EigenschaftName::Text(s) if s.is_empty() => {
                p.pos = merk;
                parse_eigenschaft_name(p)?
            }
            n => n,
        };
        // J45: `async` heisst `[Await]` — fuer die **Methode** gilt, dass
        // `await` kein Bindungsname ist. GEMESSEN (node 22):
        //   `class C { async m() { var await } }`  -> SyntaxError
        //   `class C { async *m() { var await } }` -> SyntaxError
        // (Anlass: `class/async-gen-method-static/await-as-binding-identifier.js`)
        let war_async = p.in_async;
        p.in_async = asynchron;
        let r =
            klassen_element_ab_name(p, q_start, name, statisch, asynchron, generator, abgeleitet);
        p.in_async = war_async;
        let el = r?;
        // J46: nur ein **nicht-statischer** `constructor` zaehlt —
        // `static constructor(){}` ist eine gewoehnliche statische Methode
        // (GEMESSEN: erlaubt).
        if !statisch
            && matches!(&el, KlassenElement::Methode {
                name: crate::ast::EigenschaftName::Text(t),
                ..
            } if t == "constructor")
        {
            konstruktoren += 1;
            if konstruktoren > 1 {
                return Err(p.fehler(
                    "eine Klasse hat nur einen 'constructor' (ES2023 15.7.1)".to_string(),
                ));
            }
        }
        elemente.push(el);
    }
    // J33: ES2023 15.7.1 — `ClassElement : static MethodDefinition` und
    // `static FieldDefinition`: „It is a Syntax Error if PropName of
    // MethodDefinition is \"prototype\"."
    //
    // GEMESSEN (node 22), **acht** scheitern und **vier** sind gueltig:
    //   `static prototype() {}`      `static get prototype() {}`
    //   `static set prototype(v) {}` `static *prototype() {}`
    //   `static async prototype() {}` `static prototype = 1`
    //   `static prototype;`
    //   `prototype() {}`              -> gueltig (nicht statisch!)
    //   `get prototype() {}`          -> gueltig
    //   `static #prototype() {}`      -> gueltig (privater Name = anderer Name)
    //   `static get ['prototype']() {}` -> gueltig  <- **berechneter** Name
    //
    // GEMESSEN, warum **ein** Durchgang nach der Schleife und nicht vier
    // Stellen in den Element-Bauern: es gibt vier `elemente.push(...)`, und
    // eine Regel an vier Stellen ist eine Regel, die man dreimal vergisst.
    // GEMESSEN, warum der Name **nicht** normalisiert wird: ein berechneter
    // Name `[... 'prototype']` **darf** statisch sein — die Norm spricht von
    // `PropName`, und bei einem berechneten Namen weiss der Uebersetzer den
    // Text erst zur Laufzeit. Wer hier `to_text` aufruft, wuerde
    // `static ['prototype']() {}` toeten.
    for el in &elemente {
        let (statisch, name) = match el {
            KlassenElement::Methode { name, statisch, .. }
            | KlassenElement::Feld { name, statisch, .. } => (*statisch, name),
            KlassenElement::Block { .. } => continue,
        };
        if statisch
            && let EigenschaftName::Text(s) = name
            && s == "prototype"
        {
            return Err(p.fehler(
                "ein statisches Klassenelement darf nicht 'prototype' heissen".to_string(),
            ));
        }
    }
    p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
    // J32: der alte Wert zurueck — Klassen koennen verschachtelt sein, und
    // `class C { m() { var yield = 1 } }` ist ein Fehler, waehrend
    // `function f() { var yield = 1 }` einer **ausserhalb** der Klasse ist.
    // Ohne dieses Zurueckblieben waere nach der ersten Klasse jede weitere
    // im selben Block strikt.
    p.in_klasse = war_in_klasse;
    p.feld_super = feld_super_der_äusseren;
    // J32: `AllPrivateNamesValid` — jeder **benutzte** private Name muss in
    // **diesem** Klassenkoerper deklariert sein.
    //
    // GEMESSEN (node 22):
    //   `class { f = (() => this)().#x }`            -> SyntaxError
    //   `class { #x; f = (() => this)().#x }`       -> **gueltig**
    //   `class { #x; m(){ return this.#x } }`        -> **gueltig**
    //   `class { m(){ return this.#x } }`            -> SyntaxError
    //   `class { #x; m = function(){ return this.#x } }` -> **gueltig**
    //   `class { #x; m(){ return #x in this } }`     -> **gueltig**
    //
    // GEMESSEN, warum die Pruefung **am Ende** des Rumpfes steht und nicht
    // beim Benutzen: `m(){ return #x }` gefolgt von `#x;` ist in node
    // **gueltig** — ein frueher Fehler haette echten Code getoetet, und die
    // Reihenfolge der Elemente ist genau der Punkt.
    if let Some(fehlend) = p
        .privat_ausgelieht
        .iter()
        .find(|n| !p.privat_vergeben.contains(n))
        .cloned()
    {
        return Err(p.fehler(format!(
            "der private Name {fehlend} ist in diesem Klassenkoerper nicht deklariert"
        )));
    }
    p.privat_ausgelieht = privat_ausgelieht_vorher;
    // J39: auf die gemerkte Laenge zurueckkuerzen — die Deklarationen der
    // aeusseren Klassen **bleiben** stehen (siehe oben).
    p.privat_vergeben.truncate(privat_vergeben_laenge);
    // J35: die Striktheit der Klasse endet mit ihrem Rumpf — der Aufrufer
    // (Feld-Initialisierer, Elternausdruck, Programmtext) ist wieder im
    // alten Zustand. GEMESSEN (node 22): `class C { m(){ return 010 } }
    // var x = 010;` -> `SyntaxError` **allein** wegen `m`.
    p.strikt = strikt_vorher;
    Ok((name, eltern, elemente))
}

/// J11: ein Klassenelement aus einem **bekannten** Namen — Methode,
/// Getter, Setter oder Feld. Fuer die gewoehnlichen Elemente und fuer die
/// privaten (`#x`); so liegt der private Name im selben Weg, statt ein
/// zweiter Pfad fuer ihn zu wachsen.
fn klassen_element(
    p: &mut Parser,
    q_start: usize,
    name: String,
    statisch: bool,
    generator: bool,
    abgeleitet: bool,
) -> Result<KlassenElement, JsFehler> {
    // `get #x() {}` / `set #x(v) {}`: der Accessor steht **vor** dem
    // privaten Namen, deshalb wird er hier geprueft.
    if (p.ist_wort(Schluessel::Get) || p.aktuell_bezeichner().as_deref() == Some("get"))
        || (p.ist_wort(Schluessel::Set) || p.aktuell_bezeichner().as_deref() == Some("set"))
    {
        let art_get =
            p.ist_wort(Schluessel::Get) || p.aktuell_bezeichner().as_deref() == Some("get");
        p.weiter();
        let (params, rest) = parse_param_liste(p)?;
        let rumpf = parse_funktionsrumpf(p)?;
        return Ok(KlassenElement::Methode {
            name: EigenschaftName::Text(name),
            funktion: FunktionsBau {
                params,
                rest,
                rumpf,
                pfeil: false,
                asynchron: false,
                generator: false,
                konstruktor: false,
                // J19: Quelltext der Methode.
                quelle: p.quelle_von_bis(q_start, p.pos),
                zeile: 0,
            },
            statisch,
            art: if art_get {
                MethodenArt::Getter
            } else {
                MethodenArt::Setter
            },
        });
    }
    klassen_element_ab_name(
        p,
        q_start,
        EigenschaftName::Text(name),
        statisch,
        false,
        generator,
        abgeleitet,
    )
}

/// Der Kern: der Name ist bekannt, jetzt folgt `()`, `= wert` oder `;`.
fn klassen_element_ab_name(
    p: &mut Parser,
    q_start: usize,
    name: EigenschaftName,
    statisch: bool,
    asynchron: bool,
    generator: bool,
    abgeleitet: bool,
) -> Result<KlassenElement, JsFehler> {
    // J46: `constructor` ist **kein gewöhnlicher Methodenname** (ES2023
    // 15.7.1 `MethodDefinition : ClassElementName ( UniqueFormalParameters )
    // { FunctionBody }`, Static Semantics: Early Errors) — es gibt
    // `ClassElementName : constructor` **nur** in dieser einen Form.
    //
    // GEMESSEN (node 22), **dreizehn** Proben:
    //   `class C { async * constructor(){} }` -> SyntaxError
    //   `class C { async constructor(){} }`   -> SyntaxError
    //   `class C { * constructor(){} }`      -> SyntaxError
    //   `class C { get constructor(){} }`    -> SyntaxError
    //   `class C { set constructor(){} }`    -> SyntaxError
    //   `class C { static async * constructor(){} }` -> **erlaubt**
    //   `class C { static async constructor(){} }`   -> **erlaubt**
    //   `class C { static * constructor(){} }`        -> **erlaubt**
    //   `class C { static get constructor(){} }`      -> **erlaubt**
    //   `class C { static set constructor(){} }`      -> **erlaubt**
    //   `class C { static constructor(){} }`          -> **erlaubt**
    //   `class C { constructor(){} }`                 -> **erlaubt**
    //   `class C { constructor(){} constructor(){} }` -> SyntaxError (anderes
    //     Gesetz — zwei Konstruktoren)
    //   `class C { #constructor(){} }`                -> SyntaxError
    //
    // **DIE STATISCHEN FORMEN SIND ERLAUBT** — sie sind ganz normale
    // statische Methoden namens `constructor`. Wer die Regel pauschal auf
    // den Namen legt, lehnt `static constructor(){}` ab, und das ist
    // gaengiger Code.
    let privat_name = matches!(&name, crate::ast::EigenschaftName::Text(t) if t.starts_with('#'));
    let heisst_constructor =
        matches!(&name, crate::ast::EigenschaftName::Text(t) if t == "constructor");
    if privat_name && matches!(&name, crate::ast::EigenschaftName::Text(t) if t == "#constructor") {
        return Err(p.fehler(
            "'#constructor' ist kein zulaessiger privater Name (ES2023 15.7.1)".to_string(),
        ));
    }
    if heisst_constructor && !statisch && (asynchron || generator) {
        return Err(p.fehler(
            "'constructor' ist nur als gewoehnliche Methode erlaubt (ES2023 15.7.1)".to_string(),
        ));
    }
    if p.ist_zeichen(Zeichen::LRunde) {
        if p.frisst_zeichen(Zeichen::Stern) {
            // Wurde oben schon geprueft; hier nur falls `*` direkt.
        }
        // J32: vor der **Parameterliste** (die Parameter eines Generators
        // sind ein `[Yield]`-Kontext, GEMESSEN mit node an
        // `class C { *gen(yield) { } }`), und `in_klasse` wird
        // zurueckgenommen — siehe die gleiche Stelle in `parse_funktion`.
        let war_in_klasse = p.in_klasse;
        let war_in_generator = p.in_generator;
        // J32: die Sicherung fuer die **Parameterliste** liegt ausserhalb
        // des `if` — die Ruecksetzung steht nach `parse_param_liste` und
        // muss deshalb denselben Scope haben. Erste Fassung: drinnen ->
        // `E0425` an beiden Stellen, die Crate baute nicht.
        let param_vorher = p.in_generator_param;
        if generator {
            p.in_generator_param = true;
            // J32: `in_klasse` wird hier zurueckgenommen — der Rumpf eines
            // Generators ist **kein** strikter Klassenrumpf, dort ist `yield`
            // der Operator.
            //
            // GEMESSEN, was ohne diese Zeile passiert: `class A { *walk(){
            // yield 1; yield 2 } }` wurde zu `«'yield' ist in einem
            // Klassenrumpf strikter Code»`. Diese Zeile ist bei einer
            // Rekonstruktion des Blocks verloren gegangen — und der Test, der
            // es faengt, ist `tests/j27_ecke.rs` (gegen echte github-
            // Bündel geschrieben), nicht die Suite.
            p.in_klasse = false;
            p.in_generator = true;
        }
        // J32: siehe `parse_funktion` — der Methodenrumpf bindet `arguments`
        // selbst (GEMESSEN: `class C { m(){ return arguments } }` ist ok).
        let feld_tiefe = p.feld_initialisierer;
        p.feld_initialisierer = 0;
        let war_super_erlaubt = p.super_erlaubt;
        p.super_erlaubt = abgeleitet
            && !statisch
            && matches!(&name, EigenschaftName::Text(s) if s == "constructor");
        let (params, rest) = parse_param_liste_nach_pruef(p)?;
        // J32: siehe `parse_funktion` — der Rumpf ist nie eine Parameterliste,
        // also **null** statt zurueck, und die Wiederherstellung kommt
        // **nach** dem Rumpf (sonst erbt der Rumpf einer verschachtelten
        // Funktion den Wert der aeusseren Parameterliste).
        p.in_generator_param = false;
        let rumpf = parse_funktionsrumpf(p)?;
        p.in_generator_param = param_vorher;
        p.feld_initialisierer = feld_tiefe;
        p.super_erlaubt = war_super_erlaubt;
        p.in_klasse = war_in_klasse;
        p.in_generator = war_in_generator;
        let art = match &name {
            EigenschaftName::Text(s) if s == "constructor" && !statisch => MethodenArt::Konstruktor,
            _ => MethodenArt::Normal,
        };
        Ok(KlassenElement::Methode {
            name,
            funktion: FunktionsBau {
                params,
                rest,
                rumpf,
                pfeil: false,
                asynchron,
                generator,
                konstruktor: art == MethodenArt::Konstruktor,
                // J19: Quelltext der Methode.
                quelle: p.quelle_von_bis(q_start, p.pos),
                zeile: 0,
            },
            statisch,
            art,
        })
    } else if p.frisst_zeichen(Zeichen::Gleich) {
        // J32: der Zaehler steigt fuer den Initialisierer (ES2023 15.2.1).
        //
        // GEMESSEN, warum der Zaehler **vor** dem Parsen wieder heruntergeht
        // und nicht erst danach: `parse_zuweisung` kann **fehlschlagen**, und
        // dann waere der Zaehler beim `?` hochgeblieben — jede spaetere
        // `arguments`-Pruefung in derselben Datei waere dann falsch. Der
        // Rueckbau steht deshalb **vor** dem `?`.
        // GEMESSEN, warum **Sichern und Zuruecksetzen** und nicht
        // `+= 1` / `-= 1`: die Zaehler waren unausgeglichen. Mit
        // `class C extends O { x = function(){ return super() } }` lief der
        // Motor in `attempt to subtract with overflow` (panic), und mit
        // `saturating_sub` waere der Absturz weg gewesen **und** die
        // Ungleichheit geblieben — jede spaetere `arguments`- oder
        // `super()`-Pruefung in derselben Datei waere dann falsch
        // geworden, ohne dass man es merkt.
        //
        // Das Sichern hat zwei Vorteile und keinen Nachteil: es kann
        // **nicht** unterlaufen, und es stellt auch nach einem
        // `parse_zuweisung`, das mit `Err` zurueckkommt, exakt den
        // vorherigen Wert her — derselbe Grundsatz wie bei `in_klasse`
        // und `in_generator`.
        let init_vorher = p.feld_initialisierer;
        let super_vorher = p.feld_super;
        p.feld_initialisierer = init_vorher + 1;
        p.feld_super = super_vorher + 1;
        let wert = parse_zuweisung(p);
        p.feld_initialisierer = init_vorher;
        p.feld_super = super_vorher;
        let wert = Some(wert?);
        p.erwartet_semi()?;
        Ok(KlassenElement::Feld {
            name,
            wert,
            statisch,
        })
    } else {
        p.erwartet_semi()?;
        Ok(KlassenElement::Feld {
            name,
            wert: None,
            statisch,
        })
    }
}

/// `true`, wenn nach der aktuellen Position `(` folgt (Methoden-Erkennung).
/// Release 10.10.2026: steht nach einem Namen im Objektliteral `,`, `}` oder
/// `=`, ist er eine Kurzform (`{get}`, `{async = 1}`) und kein Schluesselwort.
fn ende_einer_kurzform(p: &Parser) -> bool {
    p.ist_zeichen(Zeichen::Komma) || p.ist_zeichen(Zeichen::RGeschweift) || p.ist_zeichen(Zeichen::Gleich)
}

fn folgender_ist_klammer(p: &Parser) -> bool {
    matches!(
        p.tokens.get(p.pos).map(|t| &t.art),
        Some(TokenArt::Zeichen(Zeichen::LRunde))
    )
}

fn parse_eigenschaft_name_nach_klammer(_p: &mut Parser) -> Result<EigenschaftName, JsFehler> {
    Ok(EigenschaftName::Text(String::new()))
}

fn parse_param_liste_nach_pruef(p: &mut Parser) -> Result<(Vec<Muster>, Option<Muster>), JsFehler> {
    parse_param_liste(p)
}

// ---------------------------------------------------------------------------
// Ausdruecke (Pratt-Parser: EINE iterative Schleife statt 14 verschachtelter
// Funktionen — flache Rust-Frames, harte Tiefengrenze, M5)
// ---------------------------------------------------------------------------

fn parse_ausdruck(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    parse_ausdruck_in(p, true)
}

fn parse_ausdruck_in(p: &mut Parser, in_erlaubt: bool) -> Result<Ausdruck, JsFehler> {
    let _w = p.tief().map_err(|e| e)?;
    let e = parse_zuweisung_in(p, in_erlaubt)?;
    p.tiefe = p.tiefe.saturating_sub(1);
    // Kommaoperator.
    if p.ist_zeichen(Zeichen::Komma) {
        let mut v = vec![e];
        while p.frisst_zeichen(Zeichen::Komma) {
            v.push(parse_zuweisung_in(p, in_erlaubt)?);
        }
        return Ok(Ausdruck::Folge(v));
    }
    Ok(e)
}

/// J32: `arguments` als **einfaches** Ziel ist in striktem Code verboten
/// (ES2023 13.15.1/13.15.2/13.4.1).
///
/// GEMESSEN (node 22), **sechs** scheitern und **drei** sind gueltig:
///   `'use strict'; arguments = 1`  `arguments += 1`  `arguments >>= 20`
///   `'use strict'; arguments++`    `arguments--`
///   `'use strict'; arguments[0] = 1` -> **ok** (Member, kein einfaches Ziel)
///   `'use strict'; arguments.x = 1`  -> **ok**
///   `'use strict'; var x = arguments`-> **ok** (nur gelesen)
///   `arguments++` ohne `use strict`  -> **ok**
fn strict_ziel_verboten(p: &Parser, ziel: &Ausdruck) -> Result<(), JsFehler> {
    if p.strikt {
        if let Ausdruck::Name(n) = ziel {
            if n == "arguments" || n == "eval" {
                return Err(p.fehler(format!(
                    "'{}' darf in striktem Code kein Zuweisungsziel sein",
                    n
                )));
            }
        }
    }
    Ok(())
}

fn parse_zuweisung(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    parse_zuweisung_in(p, true)
}

/// Zuweisung mit `in`-Kontext (for-Koepfe verbieten unparenthesiertes `in`).
/// Tiefenbilanz: +1 beim Eintritt, -1 bei JEDEM Austritt (auch Fehler) —
/// dadurch bleibt die Grenze exakt (kein Kriechen wie zuvor).
fn parse_zuweisung_in(p: &mut Parser, in_erlaubt: bool) -> Result<Ausdruck, JsFehler> {
    p.tief().map_err(|e| e)?;
    // Release 10.10.2026: eine neue AssignmentExpression ist kein unaryaerer
    // Operand mehr (siehe `unaer_operand`): `void (yield e)` ist gueltig.
    // GEMESSEN vorher (bbc.com, axios im Next.js-Buendel): `return void
    // (yield e)` -> „'yield' ist im Generator keine unaryaere Expression".
    let unaer = std::mem::replace(&mut p.unaer_operand, false);
    let r = parse_zuweisung_rumpf(p, in_erlaubt);
    p.unaer_operand = unaer;
    p.tiefe = p.tiefe.saturating_sub(1);
    r
}

fn parse_zuweisung_rumpf(p: &mut Parser, in_erlaubt: bool) -> Result<Ausdruck, JsFehler> {
    // Pfeilfunktion mit EINEM Parameter: `x => ...`, `async x => ...`.
    if let Some(n) = p.aktuell_bezeichner() {
        if p.tokens.get(p.pos + 1).map(|t| &t.art) == Some(&TokenArt::Zeichen(Zeichen::Pfeil)) {
            let q_start = p.pos; // J19
            p.weiter();
            p.weiter();
            let koerper = parse_pfeil_koerper(p)?;
            return Ok(pfeil_bau(
                p,
                q_start,
                vec![Muster::Name(n)],
                None,
                koerper,
                false,
            ));
        }
    }
    if p.ist_wort(Schluessel::Async) {
        // `async (a) => ...`, `async x => ...`, `async function`.
        let merk = p.pos;
        p.weiter();
        if p.ist_wort(Schluessel::Function) {
            // J45: `await` ist ab hier kein Bindungsname (Parameter
            // und Rumpf) — siehe `schluessel_als_bindung_verboten`.
            let war_async = p.in_async;
            p.in_async = true;
            let r = parse_funktion(p, false);
            p.in_async = war_async;
            let (name, mut bau) = r?;
            bau.asynchron = true;
            // J19: `async` gehoert zum Quelltext (`async function f(){}`).
            if bau.quelle.is_some() {
                let mut q = String::from("async ");
                q.push_str(&bau.quelle.as_deref().unwrap_or_default());
                bau.quelle = Some(std::sync::Arc::from(q));
            }
            // J22: `async function(){…}(args)` und `async function(){…}(a)(b)`
            // — Fundstelle `behaviors-50c0665c5a324b22.js` 5:18668 (github):
            //   `…, async function(){ … }(0,v.l)("form.js-…", {…})`
            // GEMESSEN vorher: `Syntaxfehler 1:37: erwartet: ';'` (bzw.
            // `erwartet: ')'` in einer Argumentliste) — der Aufruf blieb
            // ungeparst, weil dieser Zweig unterhalb des Postfix-Niveaus
            // liegt. node 22 parst die Datei durch.
            return parse_kette_ab(
                p,
                Ausdruck::Funktion {
                    name,
                    funktion: Box::new(bau),
                },
            );
        }
        if let Some(n) = p.aktuell_bezeichner() {
            if p.tokens.get(p.pos + 1).map(|t| &t.art) == Some(&TokenArt::Zeichen(Zeichen::Pfeil)) {
                let q_start = p.pos; // J19
                p.weiter();
                p.weiter();
                let koerper = parse_pfeil_koerper(p)?;
                return Ok(pfeil_bau(
                    p,
                    q_start,
                    vec![Muster::Name(n)],
                    None,
                    koerper,
                    true,
                ));
            }
        }
        if p.ist_zeichen(Zeichen::LRunde) {
            let merk2 = p.pos;
            if let Ok((params, rest)) = versuche_pfeil_params(p) {
                if p.frisst_zeichen(Zeichen::Pfeil) {
                    pfeil_doppelte_namen_pruefen(p, &params, rest.as_ref())?;
                    let koerper = parse_pfeil_koerper(p)?;
                    let rumpf = match &koerper {
                        PfeilKoerper::Ausdruck(e) => vec![Anweisung::Rueckgabe(Some(e.clone()))],
                        PfeilKoerper::Block(b) => b.clone(),
                    };
                    fruehfehler_parameter_gegen_rumpf(p, &params, rest.as_ref(), &rumpf)?;
                    return Ok(pfeil_bau(p, merk, params, rest, koerper, true));
                }
            }
            p.pos = merk2;
        }
        p.pos = merk;
    }
    // `(`-Pfeil: `(a, b) => ...`, `() => ...`, `([x]) => ...`.
    if p.ist_zeichen(Zeichen::LRunde) {
        let merk = p.pos;
        if let Ok((params, rest)) = versuche_pfeil_params(p) {
            if p.frisst_zeichen(Zeichen::Pfeil) {
                pfeil_doppelte_namen_pruefen(p, &params, rest.as_ref())?;
                let koerper = parse_pfeil_koerper(p)?;
                let rumpf = match &koerper {
                    PfeilKoerper::Ausdruck(e) => vec![Anweisung::Rueckgabe(Some(e.clone()))],
                    PfeilKoerper::Block(b) => b.clone(),
                };
                fruehfehler_parameter_gegen_rumpf(p, &params, rest.as_ref(), &rumpf)?;
                return Ok(pfeil_bau(p, merk, params, rest, koerper, false));
            }
        }
        p.pos = merk;
    }
    // V3: `yield` ist erlaubt — der Uebersetzer entscheidet, was damit
    // passiert (ausserhalb eines Generators: Laufzeitfehler).

    let links = parse_pratt(p, 0, in_erlaubt)?;
    // Zuweisung?
    let op = match &p.aktuell().art {
        TokenArt::Zeichen(Zeichen::Gleich) => ZuweisungsOp::Gleich,
        TokenArt::Zeichen(Zeichen::PlusGleich) => ZuweisungsOp::Plus,
        TokenArt::Zeichen(Zeichen::MinusGleich) => ZuweisungsOp::Minus,
        TokenArt::Zeichen(Zeichen::SternGleich) => ZuweisungsOp::Stern,
        TokenArt::Zeichen(Zeichen::SchragGleich) => ZuweisungsOp::Schrag,
        TokenArt::Zeichen(Zeichen::ProzentGleich) => ZuweisungsOp::Prozent,
        TokenArt::Zeichen(Zeichen::SternSternGleich) => ZuweisungsOp::SternStern,
        TokenArt::Zeichen(Zeichen::KleinerKleinerGleich) => ZuweisungsOp::KleinerKleiner,
        TokenArt::Zeichen(Zeichen::GroesserGroesserGleich) => ZuweisungsOp::GroesserGroesser,
        TokenArt::Zeichen(Zeichen::GroesserGroesserGroesserGleich) => {
            ZuweisungsOp::GroesserGroesserGroesser
        }
        TokenArt::Zeichen(Zeichen::UndGleich) => ZuweisungsOp::Und,
        TokenArt::Zeichen(Zeichen::OderGleich) => ZuweisungsOp::Oder,
        TokenArt::Zeichen(Zeichen::HochGleich) => ZuweisungsOp::Hoch,
        TokenArt::Zeichen(Zeichen::UndUndGleich) => ZuweisungsOp::UndUnd,
        TokenArt::Zeichen(Zeichen::OderOderGleich) => ZuweisungsOp::OderOder,
        TokenArt::Zeichen(Zeichen::FrageDoppelGleich) => ZuweisungsOp::FrageFrage,
        _ => return Ok(links),
    };
    // J32: in **striktem** Code darf `arguments` kein **einfaches**
    // Zuweisungsziel sein (ES2023 13.15.1 `SimpleAssignmentExpression` /
    // 13.15.2 `AssignmentExpression`, Static Semantics: Early Errors: „It is a
    // Syntax Error if AssignmentTargetType of LeftHandSideExpression is not
    // simple and this production is contained in strict mode code", und
    // 13.15.2 nennt `arguments` ausdruecklich).
    //
    // GEMESSEN (node 22), **sechs** von seben scheitern:
    //   `'use strict'; arguments = 1`   `arguments += 1`  `arguments >>= 20`
    //   `arguments++` -> (siehe `parse_update`)  `for (arguments of []);`
    //   `'use strict'; function f(){ arguments = 1 }`
    // und **drei** sind gueltig — und die drei grenzen die Regel ab:
    //   `'use strict'; arguments[0] = 1`  -> ok  (ein **Member**, kein Name)
    //   `'use strict'; arguments.x = 1`   -> ok
    //   `'use strict'; var x = arguments` -> ok  (nur **gelesen**)
    //   `function f(){ arguments = 1 }` (ohne `use strict`) -> **ok**
    //
    // GEMESSEN, warum die Pruefung **hier** steht und nicht in
    // `ziel_aus_ausdruck`: dort weiss man nicht, ob ueberhaupt zugewiesen
    // wird — `var x = arguments` laeuft durch denselben Weg und ist
    // erlaubt.
    strict_ziel_verboten(p, &links)?;
    p.weiter();
    let wert = Box::new(parse_zuweisung_in(p, in_erlaubt)?);
    let ziel = ziel_aus_ausdruck(links)
        .ok_or_else(|| p.fehler("ungueltiges Zuweisungsziel".to_string()))?;
    Ok(Ausdruck::Zuweisung { ziel, op, wert })
}

/// J19: `q_start` ist die Tokenposition **vor** der Pfeilfunktion, damit
/// der Quelltext fuer `toString` abgeschnitten werden kann.
/// J28: die Zeile des Tokens an Position `pos`.
///
/// GEMESSEN, warum ueber den Token und nicht ueber die Anweisung: der
/// Getter/Setter-Pfad baut den `FunktionsBau`, **bevor** es eine
/// `Anweisung` gibt — die Zeile steckt hier nur im Token.
fn zeile_von(p: &Parser, pos: usize) -> u32 {
    p.tokens.get(pos).map(|t| t.zeile).unwrap_or(0)
}

/// J27: **Early Error** fuer doppelte Parameternamen einer Pfeilfunktion.
///
/// ES2023 15.4.1 (ArrowFunction), Static Semantics: Early Errors:
/// „It is a Syntax Error if BoundNames of ArrowParameters contains any
/// duplicate elements."
///
/// GEMESSEN (node 22) — alle drei sind SyntaxError:
///   `var f = (x, x) => 1;`    -> „Duplicate parameter name not allowed
///                                in this context"
///   `var f = (x, ...x) => 1;`  -> derselbe Text
///   `var f = (a, a) => 1;`     -> derselbe Text
/// Vorher nahm der Motor alle drei an. Der Lauf ueber die echte
/// test262-Suite hatte **44** rote Faelle mit
/// `Test262: This statement should not be evaluated` aus
/// `test/language/expressions/arrow-function/syntax/early-errors/`.
///
/// Die Namen kommen aus `compile::muster_namen` — **derselben** Funktion,
/// die der Uebersetzer zum Binden benutzt. Eine eigene Namenssammlung
/// waere eine zweite Wahrheit ueber dasselbe Muster.
fn pfeil_doppelte_namen_pruefen(
    p: &Parser,
    params: &[Muster],
    rest: Option<&Muster>,
) -> Result<(), JsFehler> {
    let mut gesehen: Vec<String> = Vec::new();
    let mut alle: Vec<&Muster> = params.iter().collect();
    if let Some(r) = rest {
        alle.push(r);
    }
    for m in alle {
        for n in crate::compile::muster_namen(m) {
            if gesehen.contains(&n) {
                return Err(p.fehler(format!(
                    "doppelter Parametername '{n}' in einer Pfeilfunktion"
                )));
            }
            gesehen.push(n);
        }
    }
    Ok(())
}

/// J27: **Early Error** — ein Parametername darf **nicht** zugleich im
/// Rumpf lexikalisch (`let`/`const`/`class`) deklariert sein.
///
/// ES2023 15.7.1 (AsyncFunctionDeclaration) und die entsprechenden
/// Abschnitte fuer `FunctionDeclaration`, `FunctionExpression`,
/// `MethodDefinition` und `ArrowFunction`, Static Semantics: Early
/// Errors: „It is a Syntax Error if any element of the BoundNames of
/// FormalParameters also occurs in the LexicallyDeclaredNames of
/// FunctionBody."
///
/// GEMESSEN (node 22) und der Fall, der auffaellt:
///   `(async function foo (bar) { let bar; });`
///     -> `SyntaxError: Identifier 'bar' has already been declared`
/// Der Lauf ueber die echte test262-Suite hatte **43** rote Faelle mit
/// `Test262: This statement should not be evaluated` aus
/// `test/language/expressions/async-function/early-errors-*`.
///
/// GEMESSEN, warum das **nicht** dieselbe Regel wie
/// `pfeil_doppelte_namen_pruefen` ist: node laesst
/// `async function f(a, a) {}` in sloppy **zu** und lehnt nur
/// `function h(a, a = 1) {}` (nicht-einfache Parameterliste) und die
/// Strict-Mode-Variante ab. Die beiden Regeln sind getrennt und werden
/// getrennt geprueft.
fn fruehfehler_parameter_gegen_rumpf(
    p: &Parser,
    params: &[Muster],
    rest: Option<&Muster>,
    rumpf: &[Anweisung],
) -> Result<(), JsFehler> {
    let mut param_namen: Vec<String> = Vec::new();
    for m in params.iter() {
        param_namen.extend(crate::compile::muster_namen(m));
    }
    if let Some(r) = rest {
        param_namen.extend(crate::compile::muster_namen(r));
    }
    if param_namen.is_empty() {
        return Ok(());
    }
    for a in rumpf {
        // M3: der Rumpf ist ein **Baum**; die Deklaration kann in einem
        // `Block` stehen (`{ { let x } }` zaehlt nicht — der innere Block
        // ist ein eigener Geltungsbereich). Deshalb wird **nur** die
        // oberste Ebene betrachtet, mit einer Tiefenschranke.
        match a {
            Anweisung::Deklaration { art, eintraege } if !matches!(art, DeklArt::Var) => {
                for (m, _) in eintraege {
                    for n in crate::compile::muster_namen(m) {
                        if param_namen.contains(&n) {
                            return Err(p.fehler(format!(
                                "Parametername '{n}' ist im Rumpf noch einmal deklariert"
                            )));
                        }
                    }
                }
            }
            Anweisung::MitZeile { innen, .. } => {
                fruehfehler_parameter_gegen_rumpf(p, params, rest, std::slice::from_ref(innen))?;
            }
            // J27: `class bar {}` ist eine **Deklaration** und damit
            // lexikalisch gebunden. Sie steht im AST als eigene Variante
            // `Anweisung::Klasse` — **nicht** als
            // `Anweisung::Ausdruck(Ausdruck::Klasse{..})`; die erste
            // Fassung dieses Zweigs pruefte die falsche und liess den Fall
            // durch. `parse.rs:616` und `:1038` sind die beiden Stellen,
            // die `Anweisung::Klasse` erzeugen.
            //
            // GEMESSEN (node 22):
            //   `function foo (bar) { class bar {} }`
            //     -> `SyntaxError: Identifier 'bar' has already been
            //        declared`
            // Der `if` steckt im **Match-Guard**: eine leere
            // `Anweisung::Klasse` (ohne Namen) darf nicht abgelehnt
            // werden, und `clippy::collapsible_match` will genau diese
            // Form.
            Anweisung::Klasse { name: Some(n), .. } if param_namen.contains(n) => {
                return Err(p.fehler(format!(
                    "Parametername '{n}' ist im Rumpf als Klasse deklariert"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// J27: ES2023 15.2.1, Static Semantics: `IsSimpleParameterList`.
///
/// `true`, wenn **kein** Parameter einen Vorgabewert und **kein** Parameter
/// ein `Rest` hat — also genau dann, wenn die Liste nur aus Bezeichnern
/// besteht.
///
/// GEMESSEN (node 22), woran das haengt:
///   `function h(a, a) {}`      -> erlaubt      (einfach)
///   `function h(a, a = 1) {}`  -> SyntaxError  (nicht einfach)
///   `function h(a, ...a) {}`   -> SyntaxError  (nicht einfach)
fn ist_einfache_parameterliste(params: &[Muster], rest: Option<&Muster>) -> bool {
    if rest.is_some() {
        return false;
    }
    fn einfach(m: &Muster) -> bool {
        match m {
            Muster::Name(_) => true,
            // Ein Vorgabewert macht die Liste **nicht** einfach — auch
            // dann nicht, wenn er selbst einfach ist (`(a, b = 1)`).
            Muster::Standard(..) | Muster::Rest(_) => false,
            // Zwei Musterarten, die in einer Parameterliste gar nicht
            // vorkommen; sie werden hier **nicht** als „einfach"
            // behauptet, damit eine spaetere Erweiterung nicht still
            // durchrutscht.
            Muster::Feld(_) | Muster::Gegenstand(_) | Muster::Ziel(_) => false,
        }
    }
    params.iter().all(einfach)
}

/// J27: doppelte Parameternamen in einer Parameterliste.
///
/// GEMESSEN (node 22) fuer alle Faelle, die diese Funktion abdeckt:
///   `function h(a, a) {}` in Strict     -> „Duplicate parameter name not
///                                          allowed in this context"
///   `function h(a, a = 1) {}`           -> derselbe Text
///   `function h(a, ...a) {}`            -> derselbe Text
fn doppelte_parameternamen_pruefen(
    p: &Parser,
    params: &[Muster],
    rest: Option<&Muster>,
) -> Result<(), JsFehler> {
    let mut gesehen: Vec<String> = Vec::new();
    let mut alle: Vec<&Muster> = params.iter().collect();
    if let Some(r) = rest {
        alle.push(r);
    }
    for m in alle {
        for n in crate::compile::muster_namen(m) {
            if gesehen.contains(&n) {
                return Err(p.fehler(format!("doppelter Parametername '{n}'")));
            }
            gesehen.push(n);
        }
    }
    Ok(())
}

/// J31: **`"use strict"` im Rumpf bei nicht-einfacher Parameterliste**.
///
/// ES2023 14.2.1 / 15.2.1 (Static Semantics: Early Errors):
/// „It is a Syntax Error if ContainsUseStrict of FunctionBody is true and
/// IsSimpleParameterList of FormalParameters is false."
///
/// GEMESSEN (node 22) — vier scheitern, **acht** sind gültig, und die
/// zweiten acht sind der Grund, warum die Regel nicht „bei
/// Destrukturierung" heißen darf:
///   `function f({a}) { 'use strict'; }`        -> SyntaxError
///   `function f([a]) { 'use strict'; }`        -> SyntaxError
///   `function f(a=1) { 'use strict'; }`        -> SyntaxError
///   `function f(...a) { 'use strict'; }`       -> SyntaxError
///   `async function f([a]) { 'use strict'; }`  -> SyntaxError
///   `var o = { m({a}) { 'use strict'; } }`     -> SyntaxError
///   `function f(a, b) { 'use strict'; }`       -> gueltig (einfach!)
///   `function f({a}) { }`                      -> gueltig (kein strict)
///   `function f(a) { 'use strict'; }`          -> gueltig (einfach)
///   `function f({a}) { var x; 'use strict'; }`  -> gueltig (nicht **erste**)
///   `function f({a}) { 'nicht use strict'; }`  -> gueltig
///   `function f({a}) { 'use strict'; return 1}`-> SyntaxError
///
/// Der Lauf über die echte Suite hatte **41** rote Fälle dieser Form
/// (`test/language/expressions/async-function/nameless-*` und
/// `test/language/statements/class/elements/*`).
///
/// `rumf_hat_use_strict` sieht **nur die erste Anweisung** an — das ist
/// `ContainsUseStrict` mit seinen zwei Teilen (die Direktive steht vor der
/// ersten Anweisung, und der Rumpf darf sie nicht „verstecken").
fn rumpf_hat_use_strict(rumpf: &[Anweisung]) -> bool {
    let erste = match rumpf.first() {
        Some(a) => a,
        None => return false,
    };
    // `MitZeile` ist nur die Zeilenmarke (J28) — sie zaehlt nicht als
    // Anweisung, sonst wuerde eine markierte Direktive unsichtbar.
    let a = match erste {
        Anweisung::MitZeile { innen, .. } => innen.as_ref(),
        sonst => sonst,
    };
    match a {
        Anweisung::Ausdruck(Ausdruck::Text(t)) => t.trim() == "use strict",
        _ => false,
    }
}

/// J31: die Early Error „`use strict` + nicht-einfache Parameterliste".
fn fruehfehler_strict_mit_parametern(
    p: &Parser,
    params: &[Muster],
    rest: Option<&Muster>,
    rumpf: &[Anweisung],
) -> Result<(), JsFehler> {
    if rumpf_hat_use_strict(rumpf) && !ist_einfache_parameterliste(params, rest) {
        return Err(p.fehler(
            "'use strict' ist mit einer nicht-einfachen Parameterliste nicht erlaubt".to_string(),
        ));
    }
    Ok(())
}

fn pfeil_bau(
    p: &Parser,
    q_start: usize,
    params: Vec<Muster>,
    rest: Option<Muster>,
    koerper: PfeilKoerper,
    asynchron: bool,
) -> Ausdruck {
    let (rumpf, _ausdruck) = match koerper {
        PfeilKoerper::Ausdruck(e) => (vec![Anweisung::Rueckgabe(Some(e))], true),
        PfeilKoerper::Block(b) => (b, false),
    };
    Ausdruck::Funktion {
        name: None,
        funktion: Box::new(FunktionsBau {
            params,
            rest,
            rumpf,
            pfeil: true,
            asynchron,
            generator: false,
            konstruktor: false,
            // J19: der Quelltext der Pfeilfunktion.
            quelle: p.quelle_von_bis(q_start, p.pos),
            zeile: zeile_von(p, q_start),
        }),
    }
}
enum PfeilKoerper {
    Ausdruck(Ausdruck),
    Block(Vec<Anweisung>),
}

fn parse_pfeil_koerper(p: &mut Parser) -> Result<PfeilKoerper, JsFehler> {
    if p.ist_zeichen(Zeichen::LGeschweift) {
        Ok(PfeilKoerper::Block(parse_funktionsrumpf(p)?))
    } else {
        Ok(PfeilKoerper::Ausdruck(parse_zuweisung(p)?))
    }
}

/// Parst versuchsweise `(params)` und setzt bei Misserfolg zurueck (der
/// Aufrufer prueft danach auf `=>`).
/// **Vorpruefung** (J24, gegen den Fuzz-Fund `js-haenger.bin`): Nach der
/// schliessenden `)` einer Parameterliste muss ein `=>` stehen, sonst ist es
/// keine Pfeilfunktion.
///
/// GEMESSEN/GEFUNDEN: ohne diese Pruefung parste
/// `ler = (ler = (ler = (…` an **jeder** Klammer erneut den ganzen Rest als
/// Parameterliste (`versuche_pfeil_params` macht bei Fehlschlag `p.pos`
/// zurueck). 200 Ebenen ergaben `T(n) = 2·T(n−1)` — der Fuzzer meldete
/// „HAENGER (> 5 s)" fuer 148 Klammern. Der Scan ist linear und laeuft nur
/// ueber die Klammern; der eigentliche Parse folgt nur bei `=>`.
fn klammer_vor_pfeil(p: &Parser) -> bool {
    let mut tiefe = 0usize;
    let mut i = p.pos;
    while i < p.tokens.len() {
        match &p.tokens[i].art {
            // Die Klammerarten zaehlen gemeinsam (die Vorpruefung braucht
            // nur die Balance, nicht die Art).
            // Release 10.10.2026: auch `${` eines Templates — sein `}` kommt
            // als `RGeschweift` (Lexer). GEMESSEN vorher (bbc.com, Next.js):
            // `({t: n = `${a}x`}) => n` galt nicht als Pfeil -> Syntaxfehler.
            TokenArt::Zeichen(Zeichen::LRunde)
            | TokenArt::Zeichen(Zeichen::LGeschweift)
            | TokenArt::Zeichen(Zeichen::DollarLGeschweift) => {
                tiefe += 1;
            }
            TokenArt::Zeichen(Zeichen::RRunde) | TokenArt::Zeichen(Zeichen::RGeschweift) => {
                tiefe = tiefe.saturating_sub(1);
                if tiefe == 0 {
                    // Nach der schliessenden Klammer: `=>` **ohne** Leerzeichen
                    // dazwischen (sonst waere es ein Aufruf von `a => b`-Text
                    // als Argument — da ist die Form `(a, b) => c` gemeint,
                    // und JS laesst kein Komma-Zeichen dazwischen zu).
                    return p.tokens.get(i + 1).map(|t| &t.art)
                        == Some(&TokenArt::Zeichen(Zeichen::Pfeil));
                }
            }
            // Ein `;` oder eine Zeile vor dem Ende: die Klammer wird nie
            // geschlossen — dann kann es auch keine Pfeilfunktion sein.
            TokenArt::StrichpunktErsatz | TokenArt::Ende => return false,
            _ => {}
        }
        i += 1;
    }
    false
}

fn versuche_pfeil_params(p: &mut Parser) -> Result<(Vec<Muster>, Option<Muster>), JsFehler> {
    // J24: die billige Vorpruefung (siehe `klammer_vor_pfeil`).
    if !klammer_vor_pfeil(p) {
        return Err(p.fehler("keine Pfeil-Parameter (Vorpruefung)".to_string()));
    }
    let merk = p.pos;
    let merk_tiefe = p.tiefe;
    let r = parse_param_liste(p);
    match r {
        Ok(v) => Ok(v),
        Err(_) => {
            p.pos = merk;
            p.tiefe = merk_tiefe;
            Err(p.fehler("keine Pfeil-Parameter".to_string()))
        }
    }
}

/// Wandelt Ausdruck in Zuweisungsziel um (Name, Zugriff, Destrukturierung).
fn ziel_aus_ausdruck(e: Ausdruck) -> Option<ZuweisungsZiel> {
    match e {
        Ausdruck::Name(n) => Some(ZuweisungsZiel::Name(n)),
        Ausdruck::Zugriff {
            objekt,
            name,
            optional,
        } => {
            if optional {
                None
            } else {
                Some(ZuweisungsZiel::Zugriff {
                    objekt,
                    name,
                    optional,
                })
            }
        }
        Ausdruck::Feld(teile) => {
            let mut m = Vec::new();
            for t in teile {
                match t {
                    FeldTeil::Wert(a) => m.push(FeldMuster {
                        muster: muster_fuer_ziel(*a)?,
                        standard: None,
                    }),
                    FeldTeil::Streu(a) => m.push(FeldMuster {
                        muster: Muster::Rest(Box::new(muster_fuer_ziel(*a)?)),
                        standard: None,
                    }),
                    FeldTeil::Loch => m.push(FeldMuster {
                        // J34: siehe oben — der feste Luecken-Platzhalter.
                        muster: Muster::Name(crate::compile::LOCH.to_string()),
                        standard: None,
                    }),
                }
            }
            Some(ZuweisungsZiel::Muster(Muster::Feld(m)))
        }
        Ausdruck::Gegenstand(teile) => {
            let mut m = Vec::new();
            for t in teile {
                match t {
                    GegenstandTeil::Paar(schluessel, wert) => m.push(GegenstandMuster {
                        schluessel,
                        muster: muster_fuer_ziel(*wert)?,
                        standard: None,
                        kurz: false,
                    }),
                    GegenstandTeil::Kurz(n) => m.push(GegenstandMuster {
                        schluessel: EigenschaftName::Text(n.clone()),
                        muster: Muster::Name(n),
                        standard: None,
                        kurz: true,
                    }),
                    GegenstandTeil::Streu(a) => m.push(GegenstandMuster {
                        schluessel: EigenschaftName::Text(String::new()),
                        muster: Muster::Rest(Box::new(muster_fuer_ziel(*a)?)),
                        standard: None,
                        kurz: false,
                    }),
                    _ => return None,
                }
            }
            Some(ZuweisungsZiel::Muster(Muster::Gegenstand(m)))
        }
        _ => None,
    }
}

fn muster_fuer_ziel(e: Ausdruck) -> Option<Muster> {
    match e {
        Ausdruck::Name(n) => Some(Muster::Name(n)),
        // Release 10.10.2026: ein Member ist ein gueltiges Ziel (ES2023
        // 13.15.5). GEMESSEN vorher (node 22 in Klammern):
        // `[a[i], a[j]] = [a[j], a[i]]` -> „Syntaxfehler: ungueltiges
        // Zuweisungsziel" (getauscht) — das ganze Skript lief nicht.
        Ausdruck::Zugriff { optional: false, .. } => Some(Muster::Ziel(Box::new(e))),
        Ausdruck::Zugriff { .. } => None,
        Ausdruck::Zuweisung {
            ziel,
            op: ZuweisungsOp::Gleich,
            wert,
        } => {
            // `= standard` in Destrukturierung.
            let m = match ziel {
                ZuweisungsZiel::Name(n) => Muster::Name(n),
                ZuweisungsZiel::Muster(m) => m,
                ZuweisungsZiel::Zugriff {
                    objekt,
                    name,
                    optional: false,
                } => Muster::Ziel(Box::new(Ausdruck::Zugriff {
                    objekt,
                    name,
                    optional: false,
                })),
                _ => return None,
            };
            Some(Muster::Standard(Box::new(m), wert))
        }
        Ausdruck::Feld(_) | Ausdruck::Gegenstand(_) => {
            let z = ziel_aus_ausdruck(e)?;
            match z {
                ZuweisungsZiel::Muster(m) => Some(m),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Praezedenz-Stufen (hoeher bindet staerker). `in` hat eine eigene
/// Stufe, damit for-Koepfe es verbieten koennen.
const PR_TERN: u8 = 2;
const PR_FRAGE: u8 = 3;
const PR_ODER: u8 = 4;
const PR_UND: u8 = 5;
const PR_BIT_ODER: u8 = 6;
const PR_BIT_XOR: u8 = 7;
const PR_BIT_UND: u8 = 8;
const PR_GLEICH: u8 = 9;
const PR_REL: u8 = 10;
const PR_SCHIEBE: u8 = 11;
const PR_ADD: u8 = 12;
const PR_MULT: u8 = 13;
const PR_EXP: u8 = 14;

/// Infix-Operator an aktueller Position? Gibt (Stufe, rechts-assoziativ,
/// binaer/logisch, binaerer Op) zurueck. `in` nur wenn erlaubt (for-Kopf).
fn infix_art(p: &Parser, in_erlaubt: bool) -> Option<(u8, bool, bool, BinaerOp, LogischOp)> {
    // Tupel: (Stufe, rechts, ist_logisch, binaer_op, logisch_op).
    // Nicht benutzte Op-Felder tragen Dummies (werden nicht gelesen).
    let t =
        |stufe: u8, rechts: bool, bop: BinaerOp| Some((stufe, rechts, false, bop, LogischOp::Und));
    let l = |stufe: u8, lop: LogischOp| Some((stufe, false, true, BinaerOp::Plus, lop));
    match &p.aktuell().art {
        TokenArt::Zeichen(Zeichen::DoppelFrage) => l(PR_FRAGE, LogischOp::FrageFrage),
        TokenArt::Zeichen(Zeichen::OderOder) => l(PR_ODER, LogischOp::Oder),
        TokenArt::Zeichen(Zeichen::UndUnd) => l(PR_UND, LogischOp::Und),
        TokenArt::Zeichen(Zeichen::Oder) => t(PR_BIT_ODER, false, BinaerOp::Oder),
        TokenArt::Zeichen(Zeichen::Hoch) => t(PR_BIT_XOR, false, BinaerOp::Hoch),
        TokenArt::Zeichen(Zeichen::Und) => t(PR_BIT_UND, false, BinaerOp::Und),
        TokenArt::Zeichen(Zeichen::GleichGleich) => t(PR_GLEICH, false, BinaerOp::GleichGleich),
        TokenArt::Zeichen(Zeichen::RufGleich) => t(PR_GLEICH, false, BinaerOp::RufGleich),
        TokenArt::Zeichen(Zeichen::GleichGleichGleich) => {
            t(PR_GLEICH, false, BinaerOp::GleichGleichGleich)
        }
        TokenArt::Zeichen(Zeichen::RufGleichGleich) => {
            t(PR_GLEICH, false, BinaerOp::RufGleichGleich)
        }
        TokenArt::Zeichen(Zeichen::Kleiner) => t(PR_REL, false, BinaerOp::Kleiner),
        TokenArt::Zeichen(Zeichen::KleinerGleich) => t(PR_REL, false, BinaerOp::KleinerGleich),
        TokenArt::Zeichen(Zeichen::Groesser) => t(PR_REL, false, BinaerOp::Groesser),
        TokenArt::Zeichen(Zeichen::GroesserGleich) => t(PR_REL, false, BinaerOp::GroesserGleich),
        TokenArt::Wort(Schluessel::Instanceof) => t(PR_REL, false, BinaerOp::Instanceof),
        TokenArt::Wort(Schluessel::In) if in_erlaubt => t(PR_REL, false, BinaerOp::In),
        TokenArt::Zeichen(Zeichen::KleinerKleiner) => {
            t(PR_SCHIEBE, false, BinaerOp::KleinerKleiner)
        }
        TokenArt::Zeichen(Zeichen::GroesserGroesser) => {
            t(PR_SCHIEBE, false, BinaerOp::GroesserGroesser)
        }
        TokenArt::Zeichen(Zeichen::GroesserGroesserGroesser) => {
            t(PR_SCHIEBE, false, BinaerOp::GroesserGroesserGroesser)
        }
        TokenArt::Zeichen(Zeichen::Plus) => t(PR_ADD, false, BinaerOp::Plus),
        TokenArt::Zeichen(Zeichen::Minus) => t(PR_ADD, false, BinaerOp::Minus),
        TokenArt::Zeichen(Zeichen::Stern) => t(PR_MULT, false, BinaerOp::Stern),
        TokenArt::Zeichen(Zeichen::Schrag) => t(PR_MULT, false, BinaerOp::Schrag),
        TokenArt::Zeichen(Zeichen::Prozent) => t(PR_MULT, false, BinaerOp::Prozent),
        TokenArt::Zeichen(Zeichen::SternStern) => t(PR_EXP, true, BinaerOp::SternStern),
        _ => None,
    }
}

/// Pratt-Schleife: alle binaeren/logischen Ops + Ternaer in EINER
/// Schleife (flache Frames). Tiefenbilanz wie parse_zuweisung_in.
fn parse_pratt(p: &mut Parser, min_prec: u8, in_erlaubt: bool) -> Result<Ausdruck, JsFehler> {
    p.tief().map_err(|e| e)?;
    let r = parse_pratt_rumpf(p, min_prec, in_erlaubt);
    p.tiefe = p.tiefe.saturating_sub(1);
    r
}

fn parse_pratt_rumpf(p: &mut Parser, min_prec: u8, in_erlaubt: bool) -> Result<Ausdruck, JsFehler> {
    let mut links = parse_unaer(p)?;
    // Stand `links` in Klammern? (siehe `Parser::geklammert`)
    let links_ist_geklammert = p.geklammert;
    p.geklammert = false;
    loop {
        // Bedingtausdruck `a ? b : c` (rechts ueber Zuweisungs-Aeste).
        if min_prec <= PR_TERN && p.ist_zeichen(Zeichen::Frage) {
            p.weiter();
            let dann = parse_zuweisung_in(p, in_erlaubt)?;
            p.erwartet_zeichen(Zeichen::DoppelPunkt, "':' im Bedingtausdruck")?;
            let sonst = parse_zuweisung_in(p, in_erlaubt)?;
            links = Ausdruck::Bedingt {
                bedingung: Box::new(links),
                dann: Box::new(dann),
                sonst: Box::new(sonst),
            };
            continue;
        }
        let (prec, rechts_assoc, ist_logisch, bop, lop) = match infix_art(p, in_erlaubt) {
            Some(v) => v,
            None => break,
        };
        if prec < min_prec {
            break;
        }
        p.weiter();
        // `-a ** b` braucht Klammern (JS-Regel: `(-8) ** 2` ist erlaubt,
        // `-8 ** 2` nicht). GEMESSEN: `(-8) ** (1/3)` wurde faelschlich
        // abgelehnt, weil die Klammer nicht vom Ausdruck unterscheidbar war.
        // Loesung: `parse_unaer` merkt sich, dass der Operand in Klammern
        // stand — dann zaehlt `Unaer` als geklammert.
        if matches!(bop, BinaerOp::SternStern)
            && !ist_logisch
            && matches!(links, Ausdruck::Unaer { .. })
            && !links_ist_geklammert
        {
            return Err(p.fehler("Unaerausdruck vor '**' braucht Klammern".to_string()));
        }
        let naechst = if rechts_assoc { prec } else { prec + 1 };
        let rechts_a = parse_pratt(p, naechst, in_erlaubt)?;
        links = if ist_logisch {
            Ausdruck::Logisch {
                op: lop,
                links: Box::new(links),
                rechts: Box::new(rechts_a),
            }
        } else {
            binaer(bop, links, rechts_a)
        };
    }
    Ok(links)
}

fn binaer(op: BinaerOp, l: Ausdruck, r: Ausdruck) -> Ausdruck {
    Ausdruck::Binaer {
        op,
        links: Box::new(l),
        rechts: Box::new(r),
    }
}

fn parse_unaer(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    // J32: `YieldExpression` ist **keine** `UnaryExpression` (ES2023 15.5).
    // Deshalb ist `yield` im Generator an der Stelle eines unaryaeren
    // Operanden ein Syntaxfehler — und **nur** dort.
    //
    // GEMESSEN (node 22), **vier** scheitern:
    //   `function* g(){ void yield }`     `function* g(){ typeof yield }`
    //   `function* g(){ -yield }`         `function* g(){ !yield }`
    // und **acht** sind gueltig — und die acht sind der Grund, warum die
    // Regel nicht „irgendwo im Generator" heissen darf:
    //   `function* g(){ yield }`          (Anweisung = AssignmentExpression)
    //   `function* g(){ var x = yield 1 }` (rechte Seite)
    //   `function* g(){ (yield) }`        (Klammern -> `Expression`)
    //   `function* g(){ [yield] }`        (Feld -> `AssignmentExpression`)
    //   `function* g(){ yield + 1 }`      (binaer)
    //   `function* g(){ yield yield }`
    //
    // GEMESSEN, warum die Regel an **dieser** Stelle und nicht in
    // `parse_muster`/`frisst_name` steht: `(yield)` und `[yield]` sind
    // gueltig und laufen **hier** durch (`(` und `[` rufen `parse_zuweisung`).
    // Eine Regel an der Wortform wuerde genau diese beiden toeten.
    // `await expr` (async-Kontext prueft der Interpreter zur Laufzeit).
    // Rueckverfolgung: `await` allein kann Bezeichner sein.
    if p.ist_wort(Schluessel::Await) {
        let merk = p.pos;
        let merk_tiefe = p.tiefe;
        p.weiter();
        match parse_unaer(p) {
            Ok(inner) => {
                return Ok(Ausdruck::Erwartung(Box::new(inner)));
            }
            Err(_) => {
                p.pos = merk;
                p.tiefe = merk_tiefe;
            }
        }
    }
    // J32: `YieldExpression` ist **keine** `UnaryExpression` (ES2023 15.5) —
    // `yield` ist also nur an der Stelle eines unaryaeren **Operanden**
    // verboten. GEMESSEN (die erste Fassung dieser Regel stand ganz oben in
    // `parse_unaer` und hat **alles** kaputt gemacht — auch `function* g(){
    // yield }` und `var x = yield 1`, weil `yield` ueber diesen Weg gebaut
    // wird): `function* g(){ void yield }`, `typeof yield`, `-yield`,
    // `!yield` sind Syntaxfehler; `yield`, `var x = yield 1`, `(yield)`,
    // `[yield]`, `yield + 1`, `yield yield` sind **gueltig**.
    //
    // GEMESSEN, warum ein **Flag** und kein Rueckgabewert: die sieben
    // unaryaeren Zweige rekursieren alle ueber `parse_unaer`, und der
    // Operand ist in **jedem** derselbe Aufruf. Das Flag wird dort gesetzt
    // und nach dem Operanden **wiederhergestellt** — sonst waere auch
    // `void typeof yield` verboten (bei node erlaubt).
    if p.unaer_operand && p.in_generator && p.ist_wort(Schluessel::Yield) {
        return Err(p.fehler("'yield' ist im Generator keine unaryaere Expression".to_string()));
    }
    match &p.aktuell().art.clone() {
        TokenArt::Wort(Schluessel::Typeof) => {
            p.weiter();
            let vorher = p.unaer_operand;
            p.unaer_operand = true;
            let inner = parse_unaer(p);
            p.unaer_operand = vorher;
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Typeof,
                ausdruck: Box::new(inner?),
                nach: false,
            })
        }
        TokenArt::Wort(Schluessel::Void) => {
            p.weiter();
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Void,
                ausdruck: Box::new({
                    let v = p.unaer_operand;
                    p.unaer_operand = true;
                    let r = parse_unaer(p);
                    p.unaer_operand = v;
                    r?
                }),
                nach: false,
            })
        }
        TokenArt::Wort(Schluessel::Delete) => {
            p.weiter();
            // J32: der Zaehler steht nur fuer die **Dauer des Operanden** —
            // ein Operand ist ein Ausdruck, also gibt es keine FUNCTION, die
            // er betreten koennte. Deshalb wird hier **nicht** in
            // Funktionsruepfen zurueckgenommen (anders als `feld_super`).
            //
            // GEMESSEN, warum das nicht zu frueh zurueckgenommen werden darf:
            // `class C { #x; g = delete (this.#x) }` ist derselbe Fehler —
            // die Klammer ist fuer die Regel ohne Bedeutung, weil die Regel
            // auf dem **Teilbaum** `MemberExpression . PrivateName`
            // steht, nicht auf der Klammer.
            let vorher = p.delete_pfad;
            p.delete_pfad = vorher + 1;
            // J32: `delete` ist auch ein unaryaerer Operator — siehe den
            // Kommentar am `unaer_operand`-Flag.
            let operand_vorher = p.unaer_operand;
            p.unaer_operand = true;
            let ausdruck = parse_unaer(p);
            p.unaer_operand = operand_vorher;
            p.delete_pfad = vorher;
            let ausdruck = ausdruck?;
            // J35: `delete` eines **einfachen** Namens ist im strikten Code
            // ein Syntaxfehler (ES2023 13.5.1). GEMESSEN (node 22):
            //   `"use strict"; var x = 1; delete x` -> SyntaxError
            //   `"use strict"; var o = {}; delete o.x` -> `true`
            //   `"use strict"; delete this.x`          -> `true`
            // Der Unterschied ist der **Operand**: `Ausdruck::Name` ist eine
            // `IdentifierReference`, alles andere eine `MemberExpression`.
            if p.strikt
                && let Ausdruck::Name(_) = &ausdruck
            {
                return Err(p.fehler(
                    "'delete' auf einer Variablen ist im strikten Code nicht erlaubt".to_string(),
                ));
            }
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Delete,
                ausdruck: Box::new(ausdruck),
                nach: false,
            })
        }
        TokenArt::Zeichen(Zeichen::Ruf) => {
            p.weiter();
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Ruf,
                ausdruck: Box::new({
                    let v = p.unaer_operand;
                    p.unaer_operand = true;
                    let r = parse_unaer(p);
                    p.unaer_operand = v;
                    r?
                }),
                nach: false,
            })
        }
        TokenArt::Zeichen(Zeichen::Tilde) => {
            p.weiter();
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Tilde,
                ausdruck: Box::new({
                    let v = p.unaer_operand;
                    p.unaer_operand = true;
                    let r = parse_unaer(p);
                    p.unaer_operand = v;
                    r?
                }),
                nach: false,
            })
        }
        TokenArt::Zeichen(Zeichen::Minus) => {
            p.weiter();
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Minus,
                ausdruck: Box::new({
                    let v = p.unaer_operand;
                    p.unaer_operand = true;
                    let r = parse_unaer(p);
                    p.unaer_operand = v;
                    r?
                }),
                nach: false,
            })
        }
        TokenArt::Zeichen(Zeichen::Plus) => {
            p.weiter();
            Ok(Ausdruck::Unaer {
                op: UnaerOp::Plus,
                ausdruck: Box::new({
                    let v = p.unaer_operand;
                    p.unaer_operand = true;
                    let r = parse_unaer(p);
                    p.unaer_operand = v;
                    r?
                }),
                nach: false,
            })
        }
        // GEMESSEN: `++x` muss ein `Update` mit Ziel sein, KEIN blosser
        // Ein-Operand-Ausdruck. Als `Unaer{PrePlusPlus}` wurde der Wert nur
        // gerechnet, aber nie gespeichert — der Zaehler-Closure aus p1.js
        // lieferte dadurch immer 1 statt 2.
        TokenArt::Zeichen(Zeichen::PlusPlus) => {
            p.weiter();
            let z = parse_unaer_ziel(p)?;
            // J32: `++`/`--` sind genauso verboten wie `=` (ES2023 13.4.1
            // `UpdateExpression`, Early Errors, mit 13.15.2). GEMESSEN
            // (node 22): `'use strict'; arguments++` -> SyntaxError,
            // `arguments[0]++` -> gueltig, `arguments++` **ohne**
            // `use strict` -> gueltig.
            strict_ziel_verboten(p, &z)?;
            let ziel =
                ziel_aus_ausdruck(z).ok_or_else(|| p.fehler("ungueltiges ++-Ziel".to_string()))?;
            Ok(Ausdruck::Update {
                ziel,
                hoch: true,
                nach: false,
            })
        }
        TokenArt::Zeichen(Zeichen::MinusMinus) => {
            p.weiter();
            let z = parse_unaer_ziel(p)?;
            strict_ziel_verboten(p, &z)?;
            let ziel =
                ziel_aus_ausdruck(z).ok_or_else(|| p.fehler("ungueltiges -- -Ziel".to_string()))?;
            Ok(Ausdruck::Update {
                ziel,
                hoch: false,
                nach: false,
            })
        }
        _ => {
            let mut e = parse_postfix(p)?;
            // Nachgestelltes `++` / `--` (kein Umbruch davor erlaubt).
            if !p.aktuell().umbruch_davor {
                if p.frisst_zeichen(Zeichen::PlusPlus) {
                    strict_ziel_verboten(p, &e)?;
                    let z = ziel_aus_ausdruck(e)
                        .ok_or_else(|| p.fehler("ungueltiges ++-Ziel".to_string()))?;
                    e = Ausdruck::Update {
                        ziel: z,
                        hoch: true,
                        nach: true,
                    };
                } else if p.frisst_zeichen(Zeichen::MinusMinus) {
                    strict_ziel_verboten(p, &e)?;
                    let z = ziel_aus_ausdruck(e)
                        .ok_or_else(|| p.fehler("ungueltiges ---Ziel".to_string()))?;
                    e = Ausdruck::Update {
                        ziel: z,
                        hoch: false,
                        nach: true,
                    };
                }
            }
            Ok(e)
        }
    }
}

fn parse_unaer_ziel(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    let e = parse_postfix(p)?;
    // Praefix braucht kein gueltiges Ziel zur Parsezeit (Fehler spaeter).
    Ok(e)
}

/// Aufruf-/Zugriffs-Kette (links-assoziativ, iterativ — kein Tiefenrisiko).
fn parse_postfix(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    let e = parse_primaer_mit_kette_start(p)?;
    parse_kette_ab(p, e)
}

/// J22: die **Postfix-Kette** an einem fertigen Ausdruck fortsetzen —
/// `.`/`[]`/`()`/getaggtes Template. Gebraucht fuer `async function(){}(…)`:
/// der `async function`-Zweig liegt in `parse_zuweisung_rumpf` (unterhalb des
/// Postfix-Niveaus), sodass der Aufruf sonst nicht erkannt wurde.
fn parse_kette_ab(p: &mut Parser, mut e: Ausdruck) -> Result<Ausdruck, JsFehler> {
    loop {
        if p.ist_zeichen(Zeichen::FragePunkt) {
            p.weiter();
            if p.frisst_zeichen(Zeichen::LRunde) {
                let (args, streu) = parse_arg_liste(p)?;
                e = Ausdruck::Aufruf {
                    ziel: Box::new(e),
                    args,
                    streu,
                    optional: false,
                };
                // Optional-Markierung: Aufwand sparen — Aufruf merkt sich
                // Optionalitaet nicht; stattdessen Zugriff davor. Wir
                // markieren durch Umhuellung nicht noetig: Der Interpreter
                // prueft Kurzschluss ueber spezielle Knoten. Vereinfachung:
                // `a?.b` wird als Zugriff{optional:true} gebaut, `a?.()`
                // als Aufruf eines optionalen Zugriffs auf `a` selbst?
                // Korrekt: `a?.()` kurzschliesst, wenn `a` nullish ist.
                // Wir bauen dafuer Zugriff{Name "$__opt"} nicht — stattdessen:
                e = optional_aufruf(e);
            } else if p.frisst_zeichen(Zeichen::LEckig) {
                let idx = parse_ausdruck(p)?;
                p.erwartet_zeichen(Zeichen::REckig, "']'")?;
                e = Ausdruck::Zugriff {
                    objekt: Box::new(e),
                    name: ZugriffName::Ausdruck(Box::new(idx)),
                    optional: true,
                };
            } else {
                let name = p
                    .frisst_name()
                    .ok_or_else(|| p.fehler("erwartet: Eigenschaft nach '?.'".to_string()))?;
                e = Ausdruck::Zugriff {
                    objekt: Box::new(e),
                    name: ZugriffName::Text(name),
                    optional: true,
                };
            }
            continue;
        }
        if p.ist_zeichen(Zeichen::Punkt) {
            p.weiter();
            // J11: `a.#x` — der private Name kommt mit `#` in die
            // Feldliste (Schluessel `"#x"`). GEMESSEN/ENTSCHIEDEN, warum
            // so und nicht ueber eine Marke: der Motor hat nur einen
            // Feldnamen-Raum; ein Name mit `#` ist in JS nicht ueber
            // `a['#x']` erreichbar (dort ist er es bei uns — siehe
            // Bericht J11, Abschnitt „bekannte Grenzen").
            let privat = p.ist_zeichen(Zeichen::Raute);
            if privat {
                p.weiter();
                // J32: kein Leerraum zwischen `#` und dem Namen — dieselbe
                // Regel wie an den Deklarationsstellen (siehe dort).
                if !privat_name_direkt(p) {
                    return Err(p.fehler(
                        "zwischen '#' und dem privaten Namen darf kein Leerraum stehen".to_string(),
                    ));
                }
                // J32: `delete x.#name` ist ein Syntaxfehler (ES2023
                // 13.5.1) — siehe das Feld `delete_pfad`.
                if p.delete_pfad > 0 {
                    return Err(p.fehler(
                        "'delete' auf einem privaten Namen ist nicht erlaubt".to_string(),
                    ));
                }
                // J32: und im **Elternausdruck** ist jeder private Name
                // verboten (ES2023 15.7.1) — siehe `privat_verboten`.
                if p.privat_verboten {
                    return Err(p.fehler(
                        "ein privater Name ist im Elternausdruck einer Klasse nicht erlaubt"
                            .to_string(),
                    ));
                }
            }
            let name = p
                .frisst_name()
                .ok_or_else(|| p.fehler("erwartet: Eigenschaft nach '.'".to_string()))?;
            let name = if privat { format!("#{name}") } else { name };
            // J32: **Referenz** — wird am Ende des Klassenkoerpers gegen die
            // Deklarationen geprueft (`AllPrivateNamesValid`).
            if privat {
                p.privat_ausgelieht.push(name.clone());
            }
            // Template-Tag: `a.b`...`` — unten bei Primaer behandelt; hier
            // nur Zugriff bauen.
            e = Ausdruck::Zugriff {
                objekt: Box::new(e),
                name: ZugriffName::Text(name),
                optional: false,
            };
            // Getaggtes Template direkt danach?
            if let TokenArt::TemplateStueck(..) = &p.aktuell().art {
                e = parse_tagged_nach(p, e)?;
            }
            continue;
        }
        if p.ist_zeichen(Zeichen::LEckig) {
            p.weiter();
            let idx = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::REckig, "']'")?;
            e = Ausdruck::Zugriff {
                objekt: Box::new(e),
                name: ZugriffName::Ausdruck(Box::new(idx)),
                optional: false,
            };
            if let TokenArt::TemplateStueck(..) = &p.aktuell().art {
                e = parse_tagged_nach(p, e)?;
            }
            continue;
        }
        if p.ist_zeichen(Zeichen::LRunde) {
            p.weiter();
            let (args, streu) = parse_arg_liste_ab(p)?;
            e = Ausdruck::Aufruf {
                ziel: Box::new(e),
                args,
                streu,
                optional: false,
            };
            continue;
        }
        // Getaggtes Template nach **jedem** Ziel: tag`...`.
        //
        // J22: GEMESSEN/GEFUNDEN an `index.ae4f4679716a7f81.js` (mdn,
        // 159:3083) und `behaviors-50c0665c5a324b22.js` (github, 5:18668):
        // die Beschraenkung auf `Name`/`Zugriff`/`Aufruf` liess das Template
        // stehen, wenn das Ziel ein **Klammerausdruck** war — die haeufigste
        // Form in gebuendelten Skripten:
        //
        // * `let r = (0, a.JW)\`<svg …>\``  (mdn: ein Tag aus einer
        //   Komma-Expression, in Klammern — der Import-Baustein von Rollup)
        // * `new g()\`z\``, `(1)\`x\``, `(0, String.raw)\`x${1}y\``
        //
        // Fehlerbild: „Syntaxfehler 159:3083: erwartet: ';'" — die Meldung
        // zeigte auf das **Backtick** und damit auf die Ursache.
        //
        // Warum ohne Einschraenkung richtig ist: ein Template-Literal, das
        // an dieser Stelle (hinter einem fertigen Ausdruck) steht, ist
        // per Spec **immer** ein getaggtes Template — ein loses Template
        // kann dort nicht stehen (es waere ein neuer Ausdruck, und genau
        // das wollte der Aufrufer hier nicht). `1\`x\`` ist in node gueltig
        // (Fehler zur Laufzeit, weil `1` keine Funktion ist), also gehoert
        // es hierher.
        if let TokenArt::TemplateStueck(..) = &p.aktuell().art {
            e = parse_tagged_nach(p, e)?;
            continue;
        }
        break;
    }
    Ok(e)
}

/// Markiert einen Aufruf als optional kurzschliessend (`?.()`).
fn optional_aufruf(e: Ausdruck) -> Ausdruck {
    match e {
        Ausdruck::Aufruf {
            ziel, args, streu, ..
        } => Ausdruck::Aufruf {
            ziel,
            args,
            streu,
            optional: true,
        },
        other => other,
    }
}

fn parse_arg_liste(p: &mut Parser) -> Result<(Vec<Ausdruck>, Vec<bool>), JsFehler> {
    // Nach `?.(` — `(` bereits verbraucht.
    parse_arg_liste_ab(p)
}

fn parse_arg_liste_ab(p: &mut Parser) -> Result<(Vec<Ausdruck>, Vec<bool>), JsFehler> {
    let mut args = Vec::new();
    let mut streu = Vec::new();
    if p.frisst_zeichen(Zeichen::RRunde) {
        return Ok((args, streu));
    }
    loop {
        if p.ist_zeichen(Zeichen::Punkte3) {
            p.weiter();
            args.push(parse_zuweisung(p)?);
            streu.push(true);
        } else {
            // Leerstelle `f(,)` ist ungueltig — Fehler durch parse_zuweisung.
            args.push(parse_zuweisung(p)?);
            streu.push(false);
        }
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
        if p.ist_zeichen(Zeichen::RRunde) {
            break;
        }
    }
    p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
    Ok((args, streu))
}

/// Primaer-Ausdruck inkl. `new`-Ketten-Anfang.
fn parse_primaer_mit_kette_start(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    if p.ist_wort(Schluessel::New) {
        p.weiter();
        if p.frisst_zeichen(Zeichen::Punkt) {
            // W14 (JS, 04.10.2026): `new.target` — ES2023 13.3.7.2
            // (`MemberExpression : new . new.target`). Frueher stand hier
            // pauschal `Err("new.target wird nicht unterstuetzt")`, obwohl die
            // Laufzeitseite (`Rahmen::neu`) fertig war: `new F()` setzt es,
            // und `super()` reicht es an den Elternkonstruktor weiter.
            //
            // Nur `target` ist hier zulaessig — `new.foo` ist auch in node 22
            // ein SyntaxError, das bleibt also bei der alten Meldung.
            if p.ist_wort(Schluessel::Target) {
                p.weiter();
                return Ok(Ausdruck::NeuZiel);
            }
            return Err(p.fehler("new.target wird nicht unterstuetzt".to_string()));
        }
        // `new` bindet: Ziel ist Postfix-Ausdruck OHNE Aufruf-Argumente?
        // JS-Regel: `new a.b()` gehoert alles zu new; `new a` ohne Klammern
        // ok. Wir parsen gierig einen Postfix und nehmen einen evtl.
        // endstaendigen Aufruf als new-Argumente.
        let ziel = parse_primaer_mit_kette_start(p)?;
        // Zugriffskette nach dem Ziel (ohne Aufrufe) einsammeln.
        let mut e = ziel;
        loop {
            if p.ist_zeichen(Zeichen::Punkt) {
                p.weiter();
                let name = p
                    .frisst_name()
                    .ok_or_else(|| p.fehler("erwartet: Eigenschaft".to_string()))?;
                e = Ausdruck::Zugriff {
                    objekt: Box::new(e),
                    name: ZugriffName::Text(name),
                    optional: false,
                };
            } else if p.ist_zeichen(Zeichen::LEckig) {
                p.weiter();
                let idx = parse_ausdruck(p)?;
                p.erwartet_zeichen(Zeichen::REckig, "']'")?;
                e = Ausdruck::Zugriff {
                    objekt: Box::new(e),
                    name: ZugriffName::Ausdruck(Box::new(idx)),
                    optional: false,
                };
            } else {
                break;
            }
        }
        if p.ist_zeichen(Zeichen::LRunde) {
            p.weiter();
            let (args, streu) = parse_arg_liste_ab(p)?;
            return Ok(Ausdruck::Neu {
                ziel: Box::new(e),
                args,
                streu,
            });
        }
        return Ok(Ausdruck::Neu {
            ziel: Box::new(e),
            args: Vec::new(),
            streu: Vec::new(),
        });
    }
    parse_primar_mit_kette(p)
}

fn parse_primar_mit_kette(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    parse_primaer(p)
}

/// J16: Primaerausdruck mit `.`-/`[]`-Kette, **ohne** Aufruf (der
/// Elternausdruck von `class … extends …`). Ohne Aufruf, weil
/// `extends new Foo()` ohne Klammern ungueltig ist und ein Aufruf nach
/// dem Namen gehoerte sonst zum Rumpf (`extends A {` muss der Rumpf
/// sein — der Aufruf gehoerte zum Kind).
fn parse_kette_ohne_rruf(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    let mut e = parse_primaer(p)?;
    loop {
        if p.ist_zeichen(Zeichen::Punkt) {
            p.weiter();
            let privat = p.ist_zeichen(Zeichen::Raute);
            if privat {
                p.weiter();
                // J32: kein Leerraum zwischen `#` und dem Namen — dieselbe
                // Regel wie an den Deklarationsstellen (siehe dort).
                if !privat_name_direkt(p) {
                    return Err(p.fehler(
                        "zwischen '#' und dem privaten Namen darf kein Leerraum stehen".to_string(),
                    ));
                }
                // J32: `delete x.#name` ist ein Syntaxfehler (ES2023
                // 13.5.1) — siehe das Feld `delete_pfad`.
                if p.delete_pfad > 0 {
                    return Err(p.fehler(
                        "'delete' auf einem privaten Namen ist nicht erlaubt".to_string(),
                    ));
                }
                // J32: und im **Elternausdruck** ist jeder private Name
                // verboten (ES2023 15.7.1) — siehe `privat_verboten`.
                if p.privat_verboten {
                    return Err(p.fehler(
                        "ein privater Name ist im Elternausdruck einer Klasse nicht erlaubt"
                            .to_string(),
                    ));
                }
            }
            let name = p
                .frisst_name()
                .ok_or_else(|| p.fehler("erwartet: Eigenschaft nach '.'".to_string()))?;
            let name = if privat { format!("#{name}") } else { name };
            // J32: **Referenz** — wird am Ende des Klassenkoerpers gegen die
            // Deklarationen geprueft (`AllPrivateNamesValid`).
            if privat {
                p.privat_ausgelieht.push(name.clone());
            }
            e = Ausdruck::Zugriff {
                objekt: Box::new(e),
                name: ZugriffName::Text(name),
                optional: false,
            };
            continue;
        }
        if p.ist_zeichen(Zeichen::LEckig) {
            p.weiter();
            let idx = parse_zuweisung(p)?;
            p.erwartet_zeichen(Zeichen::REckig, "']'")?;
            e = Ausdruck::Zugriff {
                objekt: Box::new(e),
                name: ZugriffName::Ausdruck(Box::new(idx)),
                optional: false,
            };
            continue;
        }
        return Ok(e);
    }
}

fn parse_primaer(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    let _w = p.tief().map_err(|e| e)?;
    let e = parse_primaer_tief(p)?;
    p.tiefe = p.tiefe.saturating_sub(1);
    Ok(e)
}

fn parse_primaer_tief(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    // J15: `import.meta` — der Meta-Zugriff eines Moduls. Er wird auf die
    // Objekt-Bindung `__jsvm_import_meta__` umgeschrieben, die der
    // Entzucker im Modulrumpf anlegt (dort steht die URL des Moduls).
    //
    // GEMESSEN/ENTSCHIEDEN, warum ein Name und keine eigene AST-Form: der
    // Entzucker legt schon eine Bindung mit der Modul-URL an
    // (`__jsvm_modul_url__`, J12b) — das Muster ist erprobt, und die
    // Form braucht keine Aenderung am Compiler.
    if p.ist_wort(Schluessel::Import)
        && matches!(
            p.tokens.get(p.pos + 1).map(|t| &t.art),
            Some(TokenArt::Zeichen(Zeichen::Punkt))
        )
    {
        // GEMESSEN, warum die Pruefung HIER (Ausdrucksebene) und nicht nur
        // auf der Anweisungsebene steht: im Skript kam der Ausdruck
        // zuerst durch den allgemeinen Ausdrucksweg und lieferte dann
        // `ReferenceError: '__jsvm_import_meta__' ist nicht definiert` —
        // eine interne Meldung statt der Regel. Im Modul ist beides
        // gueltig, darum erst hier.
        if p.modul.is_none() {
            return Err(p.fehler(
                "import.meta gibt es nur in einem Modul (script type=\"module\")".to_string(),
            ));
        }
        p.weiter(); // `import`
        p.erwartet_zeichen(Zeichen::Punkt, "'.meta'")?;
        if p.aktuell_bezeichner().as_deref() != Some("meta") {
            return Err(p.fehler("nur 'import.meta' ist erlaubt".to_string()));
        }
        p.weiter(); // `meta`
        return Ok(Ausdruck::Name(crate::module::N_IMPORT_META.to_string()));
    }
    // J12: `#x in obj` — die **Markenpruefung** eines privaten Namens
    // (sec-12.10.3 / ErgonomicBrandChecks). Sie ist genau dann wahr,
    // wenn das Objekt das private Feld traegt.
    //
    // GEMESSEN/ENTSCHIEDEN, warum der Name als gewoehnlicher Text mit `#`
    // in den Ausdruck geht: im Motor ist ein privates Feld ein Feld mit
    // `#` im Schluessel (siehe J11, `heap::ist_privat`) — die Marke ist
    // damit genau das `in` auf diesem Schluessel. node 22 als Referenz:
    // `class C { #x = 1; m(o){ return #x in o } } new C().m(new C())` ->
    // `true`, mit einem leeren Objekt -> `false`.
    //
    // Ohne `in` ist ein privater Name ungueltig (node: SyntaxError) — das
    // wird hier ausdruecklich benannt statt still ein Text zu liefern.
    if p.ist_zeichen(Zeichen::Raute) {
        p.weiter();
        // J32: `PrivateName :: # IdentifierName` — zwischen `#` und dem
        // Namen darf **kein** Leerraum stehen (ES2023 3.11.28 / 12.7.1).
        // GEMESSEN (node 22), **acht** scheitern:
        //   `class C { set # x(v) {} }`   `class C { # x; }`
        //   `class C { get # x() {} }`   `class C { m(){ return this.# x } }`
        //   `class C { #\tx; }`          `class C { static # y = 1 }`
        //   `class C { * # z() {} }`     `class C { async # w() {} }`
        // und **gueltig** sind die ohne Leerraum — auch ueber einen
        // Zeilenumbruch: `class C { set\n#x(v) {} }` laeuft in node durch.
        // Die Regel ist also rein **waagerecht**.
        if !privat_name_direkt(p) {
            return Err(p.fehler(
                "zwischen '#' und dem privaten Namen darf kein Leerraum stehen".to_string(),
            ));
        }
        let n = p
            .frisst_name()
            .ok_or_else(|| p.fehler("Name nach '#' erwartet".to_string()))?;
        // J32: `#name in` ist eine **Referenz** und im Elternausdruck
        // verboten (ES2023 15.7.1) — GEMESSEN (node 22):
        // `class C extends (#x in {}) {}` -> SyntaxError.
        //
        // GEMESSEN, warum hier **keine** Deklaration steht: die erste Fassung
        // meiner Einfuegung setzte hier `privat_vergeben.push(...)` mit — damit
        // galt jeder per `#x in` **benutzte** Name fuer **deklariert**, und
        // `var C = class { m(){ return #x in this } }` blieb `ok`, waehrend
        // node `SyntaxError` sagt. GEMESSEN (node 22):
        //   `class { m(){ return #x in this } }`   -> SyntaxError
        //   `class { #x; m(){ return #x in this } }` -> **gueltig**
        if p.privat_verboten {
            return Err(p.fehler(
                "ein privater Name ist im Elternausdruck einer Klasse nicht erlaubt".to_string(),
            ));
        }
        if !p.ist_wort(Schluessel::In) {
            return Err(p.fehler(
                "ein privater Name ist nur mit 'in' erlaubt (Markenpruefung)".to_string(),
            ));
        }
        // J32: **Referenz** (`#name in`) — siehe `AllPrivateNamesValid`.
        p.privat_ausgelieht.push(format!("#{n}"));
        return Ok(Ausdruck::Text(format!("#{n}")));
    }
    // J12: `import(…)` — der **dynamische** Import. Er ist ein Ausdruck
    // und darf ueberall stehen (auch im Skript). GEMESSEN/ENTSCHIEDEN, warum
    // ein Motors-Native und nicht eine eigene AST-Form: der Import
    // braucht `&mut self` (Lader + Versprechen) — genau das kann ein
    // Nativ, eine AST-Form muesste durch den ganzen Compiler.
    if p.ist_wort(Schluessel::Import)
        && matches!(
            p.tokens.get(p.pos + 1).map(|t| &t.art),
            Some(TokenArt::Zeichen(Zeichen::LRunde))
        )
    {
        p.weiter(); // `import`
        p.erwartet_zeichen(Zeichen::LRunde, "'(' nach import")?;
        // `erwartet_zeichen` hat die Klammer schon genommen;
        // `parse_arg_liste_ab` liest ab hier und schliesst mit `)`.
        let (argumente, _) = parse_arg_liste_ab(p)?;
        let n = argumente.len();
        return Ok(Ausdruck::Aufruf {
            ziel: Box::new(Ausdruck::Name(crate::module::N_IMPORT_DYN.to_string())),
            args: argumente,
            streu: vec![false; n],
            optional: false,
        });
    }
    match &p.aktuell().art.clone() {
        TokenArt::Zahl(z) => {
            let z = *z;
            // J35: Legacy-Oktal ist im strikten Code verboten — siehe
            // `ist_legacy_oktal` (zehn Messungen in dessen Kommentar).
            if p.strikt && ist_legacy_oktal(p) {
                return Err(p.fehler(
                    "Legacy-Oktal-Literal ist im strikten Code nicht erlaubt".to_string(),
                ));
            }
            p.weiter();
            Ok(Ausdruck::Zahl(z))
        }
        // J24: `123n` (BigInt-Literal).
        TokenArt::Ganz(g) => {
            let g = *g;
            p.weiter();
            Ok(Ausdruck::Ganz(g))
        }
        TokenArt::Zeichenkette(s) => {
            let s = s.clone();
            p.weiter();
            // `"use strict"` ist Anweisungs-, kein Ausdruckskontext — egal.
            Ok(Ausdruck::Text(s))
        }
        TokenArt::TemplateStueck(..) => parse_template(p),
        TokenArt::Regex { muster, schalter } => {
            let (m, s) = (muster.clone(), schalter.clone());
            // J27: **Early Error** — das Muster wird schon beim
            // Uebersetzen geprueft, nicht erst beim Auswerten.
            //
            // GEMESSEN, warum das eine echte Luecke war und keine
            // Feinheit: der Lauf ueber die echte test262-Suite hatte
            // **62** rote Faelle mit `Test262: This statement should not
            // be evaluated` — alle in
            // `test/built-ins/RegExp/property-escapes/`. Diese Dateien
            // tragen `negative: {phase: parse, type: SyntaxError}` und
            // rufen `$DONOTEVALUATE()` **vor** dem Literal auf. Vorher
            // meldete der Motor den Fehler als *Laufzeit*fehler
            // (`Laufzeitfehler: ungueltiges Regex: …`), also kam die
            // Anweisung `/\P{ASCII=F}/u;` gar nicht mehr dran.
            //
            // GEMESSEN (node 22), was die Suite verlangt:
            //   `var r = /\p{ASCII=F}/u;`
            //     -> `SyntaxError: Invalid regular expression:
            //         /\p{ASCII=F}/u: Invalid property name`
            //   `var r = /[/u;` -> `SyntaxError: … missing /`
            // Beides ist ein **Uebersetzungs**fehler, kein Laufzeitfehler.
            //
            // Es wird **nicht** das Muster gebaut, sondern geprueft:
            // `pruefen` ist die vorhandene Pruefung und baut keinen
            // Knotenbaum, der dann wieder verworfen werden muesste.
            if let Err(w) = crate::regexmini::pruefen_mit_flags(&m, &s) {
                return Err(p.fehler(format!("ungueltiges Regex: {w}")));
            }
            if let Err(w) = crate::regexmini::schalter_aus(&s) {
                return Err(p.fehler(format!("ungueltiger Regex-Schalter: {w}")));
            }
            p.weiter();
            Ok(Ausdruck::Regex {
                muster: m,
                schalter: s,
            })
        }
        TokenArt::Wort(Schluessel::True) => {
            p.weiter();
            Ok(Ausdruck::Wahr(true))
        }
        TokenArt::Wort(Schluessel::False) => {
            p.weiter();
            Ok(Ausdruck::Wahr(false))
        }
        TokenArt::Wort(Schluessel::Null) => {
            p.weiter();
            Ok(Ausdruck::Null)
        }
        TokenArt::Wort(Schluessel::This) => {
            p.weiter();
            Ok(Ausdruck::Dies)
        }
        TokenArt::Wort(Schluessel::Super) => {
            // J32: **`super()` als Aufruf** ist in einem Klassenfeld-
            // Initialisierer verboten (ES2023 15.2.1,
            // `FieldDefinition`: „It is a Syntax Error if Initializer is
            // present and Initializer Contains SuperCall is true").
            //
            // GEMESSEN (node 22) — **sechs** scheitern und, das ist der
            // Punkt, **super.prop** ist **gueltig**:
            //   `class C { #x = typeof super() }`            -> SyntaxError
            //   `class C extends O { x = super() }`          -> SyntaxError
            //   `class C extends O { x = () => super() }`    -> SyntaxError
            //   `class C extends O { static x = () => super() }` -> SyntaxError
            //   `class C extends O { x = { a: super() } }`   -> SyntaxError
            //   `class C extends O { x = function(){ return super() } }`
            //   `class C extends O { x = class { y = super() } }`
            //                                                  -> SyntaxError
            //   `class C extends B { constructor(){ super() } }` -> **ok**
            //   `class C extends B { x = super.toString }`      -> **ok**
            //   `class C extends B { x = () => super.toString }`-> **ok**
            //
            // GEMESSEN, warum die Unterscheidung an der **Klammer** haengt
            // und nicht am Wort: `super.prop` hat einen [[HomeObject]] (die
            // Klasse), `super()` nicht (es ist ein **Aufruf**). Eine Regel
            // „kein `super` im Initialisierer" wuerde die drei gueltigen
            // Faelle toeten — und die kommen in echtem Code vor.
            // GEMESSEN, warum die Klammer **nach** `p.weiter()` geprueft
            // wird: `ist_zeichen` sieht das **aktuelle** Token, und das ist
            // vor `weiter()` noch `super`. Die erste Fassung pruefte vorher
            // und liess **jeden** der sieben Faelle durch (Motor `ok` gegen
            // node `SyntaxError`) — dieselbe Falle wie bei `\p{…}`.
            p.weiter();
            if p.ist_zeichen(Zeichen::LRunde) {
                if p.feld_super > 0 {
                    return Err(p.fehler(
                        "'super()' ist in einem Klassenfeld-Initialisierer nicht erlaubt"
                            .to_string(),
                    ));
                }
                if !p.super_erlaubt {
                    return Err(p.fehler("'super()' ist hier nicht erlaubt".to_string()));
                }
            }
            Ok(Ausdruck::Super)
        }
        TokenArt::Wort(Schluessel::Function) => {
            // J45: hier ist KEIN [Await] — `await` ist wieder ein
            // Bindungsname (GEMESSEN: `async function f(){ function g(){
            // var await } }` ist in node erlaubt).
            let war_async = p.in_async;
            p.in_async = false;
            let r = parse_funktion(p, false);
            p.in_async = war_async;
            let (name, bau) = r?;
            Ok(Ausdruck::Funktion {
                name,
                funktion: Box::new(bau),
            })
        }
        TokenArt::Wort(Schluessel::Class) => {
            let q_start_k = p.pos; // J19: `class` beginnt den Quelltext
            p.weiter();
            let (name, eltern, elemente) = parse_klasse_rumpf(p)?;
            Ok(Ausdruck::Klasse {
                name,
                eltern: eltern.map(Box::new),
                elemente,
                // J19: Quelltext der Klasse.
                quelle: p.quelle_von_bis(q_start_k, p.pos),
            })
        }
        TokenArt::Wort(Schluessel::Async) => {
            let merk = p.pos;
            p.weiter();
            if p.ist_wort(Schluessel::Function) {
                // J45: `await` ist ab hier kein Bindungsname (Parameter
                // und Rumpf) — siehe `schluessel_als_bindung_verboten`.
                let war_async = p.in_async;
                p.in_async = true;
                let r = parse_funktion(p, false);
                p.in_async = war_async;
                let (name, mut bau) = r?;
                bau.asynchron = true;
                return Ok(Ausdruck::Funktion {
                    name,
                    funktion: Box::new(bau),
                });
            }
            p.pos = merk;
            Err(p.fehler("unerwartetes 'async'".to_string()))
        }
        TokenArt::Zeichen(Zeichen::LRunde) => {
            p.weiter();
            if p.frisst_zeichen(Zeichen::RRunde) {
                return Err(p.fehler("leere Klammer ist kein Ausdruck".to_string()));
            }
            let e = parse_ausdruck(p)?;
            p.erwartet_zeichen(Zeichen::RRunde, "')'")?;
            // GEMESSEN: `(-8) ** 2` ist erlaubt, `-8 ** 2` nicht. Ohne diese
            // Merkung wurde beides abgelehnt (`(-8) ** (1/3)`).
            p.geklammert = true;
            Ok(e)
        }
        TokenArt::Zeichen(Zeichen::LEckig) => parse_feld_literal(p),
        TokenArt::Zeichen(Zeichen::LGeschweift) => parse_gegenstand_literal(p),
        _ => {
            let _art = p.aktuell().art.clone();
            match p.frisst_name() {
                // V3: `yield` / `yield expr` / `yield* expr`.
                Some(n)
                    if n == "yield"
                        && matches!(_art, TokenArt::Wort(Schluessel::Yield))
                        && !p.ist_zeichen(Zeichen::Punkte3) =>
                {
                    // J32: im Klassenrumpf ist das ein Syntaxfehler (ES2023
                    // 10.2.1 + 12.1.1). GEMESSEN (node 22):
                    // `class C { static x = yield }` -> SyntaxError.
                    if p.in_klasse {
                        return Err(p.fehler(
                            "'yield' ist in einem Klassenrumpf strikter Code".to_string(),
                        ));
                    }
                    // J32: in der **Parameterliste** eines Generators ist
                    // `yield` als Ausdruck ein Syntaxfehler (ES2023 15.4.1) —
                    // siehe das Feld `in_generator_param`. Im **Rumpf** ist
                    // derselbe Zweig erlaubt, und eine **verschachtelte
                    // Funktion** setzt das Flag zurueck (siehe
                    // `parse_funktion`), weil ihr Rumpf kein Generatorrumpf
                    // ist. GEMESSEN (node 22):
                    //   `function* g(a = yield) {}`   -> SyntaxError
                    //   `function* g(a = yield 1) {}` -> SyntaxError
                    //   `function* g({a = yield}) {}` -> SyntaxError
                    //   `function f(a = yield) {}`    -> gueltig
                    //   `function* g(a = function(){ return yield }) {}` ->
                    //       gueltig
                    if p.in_generator_param {
                        return Err(p.fehler(
                            "'yield' ist in der Parameterliste eines Generators nicht erlaubt"
                                .to_string(),
                        ));
                    }
                    if p.frisst_zeichen(Zeichen::Stern) {
                        let e = parse_zuweisung(p)?;
                        return Ok(Ausdruck::ErtragStern(Box::new(e)));
                    }
                    // `yield;`, `yield)` usw. -> ohne Operanden.
                    // J16: `LEckig` (`]`) und `LGeschweift` (`}`) sind **kein**
                    // solcher Fall — sie eroeffnen einen Ausdruck (Feld- bzw.
                    // Objektliteral). GEMESSEN vorher an echten Bundeln
                    // (github `catalyst`): `yield{type:…}` brach mit
                    // `Syntaxfehler: erwartet: ';'` ab, `yield[1,2]` lieferte
                    // `undefined` statt des Feldes. node 22 yieldet beides.
                    // `}` **am Schluss** (Ende einer Template-Ersetzung)
                    // bleibt „ohne Operanden".
                    if p.ist_zeichen(Zeichen::Semikolon)
                        || p.ist_zeichen(Zeichen::RRunde)
                        || p.ist_zeichen(Zeichen::RGeschweift)
                        || p.ist_zeichen(Zeichen::Komma)
                    {
                        return Ok(Ausdruck::Ertrag(None));
                    }
                    let e = parse_zuweisung(p)?;
                    Ok(Ausdruck::Ertrag(Some(Box::new(e))))
                }
                // J32: `arguments` im Klassenfeld-Initialisierer.
                //
                // GEMESSEN (node 22), **sechs** scheitern und — das ist der
                // Grund fuer den Zaehler und nicht fuer ein Flag — **eine**
                // verschachtelte Klasse auch:
                //   `class C { #x = () => arguments }`        -> SyntaxError
                //   `class C { x = arguments }`               -> SyntaxError
                //   `class C { x = () => arguments }`         -> SyntaxError
                //   `class C { static x = arguments }`        -> SyntaxError
                //   `class C { x = { a: arguments } }`        -> SyntaxError
                //   `class C { x = [arguments] }`             -> SyntaxError
                //   `class C { x = class { y = arguments } }` -> SyntaxError
                //
                // Und **gueltig** sind vier, davon zwei gegenlaeufig:
                //   `class C { x = function(){ return arguments } }` -> ok
                //       (eine verschachtelte Funktion hat ihre **eigene**
                //        `arguments`-Bindung)
                //   `class C { m(){ return arguments } }`             -> ok
                //   `class C { get g(){ return arguments } }`          -> ok
                //   `class C { #p; x = this.#p }`                     -> ok
                // Deshalb wird der Zaehler in `parse_funktion` und im
                // Methodenrumpf **zurueckgenommen** (siehe dort) — sonst
                // waere der erste gueltige Fall rot.
                Some(n) if n == "arguments" && p.feld_initialisierer > 0 => Err(p.fehler(
                    "'arguments' ist in einem Klassenfeld-Initialisierer nicht erlaubt".to_string(),
                )),
                Some(n) => {
                    if p.in_generator && n == "yield" {
                        return Err(p.fehler(
                            "'yield' ist im Generator keine Bezeichner-Referenz".to_string(),
                        ));
                    }
                    if p.in_klasse && n == "yield" {
                        return Err(
                            p.fehler("'yield' ist im Klassenrumpf strikter Code".to_string())
                        );
                    }
                    Ok(Ausdruck::Name(n))
                }
                None => Err(p.fehler("erwartet: Ausdruck".to_string())),
            }
        }
    }
}

fn parse_template(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    let mut teile = Vec::new();
    loop {
        match &p.aktuell().art.clone() {
            TokenArt::TemplateStueck(t) => {
                // Release 15.10.2026: an invalid escape is a SyntaxError in
                // an untagged template only (ECMA-262 13.2.8.1).
                let Some(s) = t.gekocht.clone() else {
                    return Err(p.fehler("Invalid escape sequence in template".to_string()));
                };
                p.weiter();
                if !s.is_empty() {
                    teile.push(TemplateTeil::Text(s));
                }
                // Folgt `${`, geht es nach dem Ausdruck weiter.
                if p.ist_zeichen(Zeichen::DollarLGeschweift) {
                    p.weiter();
                    let e = parse_ausdruck(p)?;
                    teile.push(TemplateTeil::Ausdruck(Box::new(e)));
                    // Lexer hat `}` + naechstes Stueck als RGeschweift +
                    // TemplateStueck erzeugt.
                    p.erwartet_zeichen(Zeichen::RGeschweift, "'}' in Template")?;
                    continue;
                }
                break;
            }
            _ => return Err(p.fehler("erwartet: Template-Abschnitt".to_string())),
        }
    }
    Ok(Ausdruck::Template(teile))
}

/// Getaggtes Template nach einem Ziel: tag`a${b}c` -> Aufruf(tag,
/// [Vorlagen-Array, b]). Das Vorlagen-Array baut der Interpreter.
fn parse_tagged_nach(p: &mut Parser, ziel: Ausdruck) -> Result<Ausdruck, JsFehler> {
    // Release 15.10.2026: the first argument is the template object (cooked
    // and raw strings, one per piece, empty pieces included), built and
    // cached per site by `Op::VorlagenObjekt`. MEASURED before: the strings
    // were a plain array of cooked texts without `raw`, empty pieces were
    // dropped (`tag`${a}${b}`` got 0 strings for 2 values), and a new array
    // came on every call (lit-html caches templates by that identity).
    let mut gekocht = Vec::new();
    let mut roh = Vec::new();
    let mut werte = Vec::new();
    loop {
        let TokenArt::TemplateStueck(t) = &p.aktuell().art else {
            return Err(p.fehler("erwartet: Template-Abschnitt".to_string()));
        };
        gekocht.push(t.gekocht.clone());
        roh.push(t.roh.clone());
        p.weiter();
        if p.ist_zeichen(Zeichen::DollarLGeschweift) {
            p.weiter();
            werte.push(parse_ausdruck(p)?);
            p.erwartet_zeichen(Zeichen::RGeschweift, "'}' in Template")?;
            continue;
        }
        break;
    }
    let mut args = vec![Ausdruck::VorlagenObjekt(Box::new((gekocht, roh)))];
    args.extend(werte);
    let streu = vec![false; args.len()];
    Ok(Ausdruck::Aufruf {
        ziel: Box::new(ziel),
        args,
        streu,
        optional: false,
    })
}

fn parse_feld_literal(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    p.weiter(); // [
    let mut teile = Vec::new();
    if p.frisst_zeichen(Zeichen::REckig) {
        return Ok(Ausdruck::Feld(teile));
    }
    loop {
        if p.ist_zeichen(Zeichen::REckig) {
            break;
        }
        if p.frisst_zeichen(Zeichen::Komma) {
            teile.push(FeldTeil::Loch);
            continue;
        }
        if p.frisst_zeichen(Zeichen::Punkte3) {
            let e = parse_zuweisung(p)?;
            teile.push(FeldTeil::Streu(Box::new(e)));
        } else {
            let e = parse_zuweisung(p)?;
            teile.push(FeldTeil::Wert(Box::new(e)));
        }
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
    }
    p.erwartet_zeichen(Zeichen::REckig, "']'")?;
    Ok(Ausdruck::Feld(teile))
}

fn parse_gegenstand_literal(p: &mut Parser) -> Result<Ausdruck, JsFehler> {
    p.weiter(); // {
    let mut teile = Vec::new();
    if p.frisst_zeichen(Zeichen::RGeschweift) {
        return Ok(Ausdruck::Gegenstand(teile));
    }
    loop {
        if p.ist_zeichen(Zeichen::RGeschweift) {
            break;
        }
        if p.frisst_zeichen(Zeichen::Punkte3) {
            let e = parse_zuweisung(p)?;
            teile.push(GegenstandTeil::Streu(Box::new(e)));
            if !p.frisst_zeichen(Zeichen::Komma) {
                break;
            }
            continue;
        }
        // get/set-Methoden.
        let merk = p.pos;
        // J19: der Name der Methode beginnt ihren Quelltext.
        let q_start_m = p.pos;
        let erster = p.frisst_name();
        if matches!(erster.as_deref(), Some("get") | Some("set")) && !folgender_ist_klammer(p) {
            let art_get = erster.as_deref() == Some("get");
            // Aber `get: 1` ist ein Paar! Pruefen: folgt `:` direkt?
            // Release 10.10.2026: ebenso die Kurzform `{get}` / `{get = 1}`
            // (`,`, `}`, `=` danach). GEMESSEN vorher: de.wikipedia.org,
            // `module.exports={…,get,getAsync,…}` im ResourceLoader-Buendel
            // war ein Syntaxfehler („erwartet: Eigenschaftsname", 203:343) —
            // das ganze Buendel fiel aus.
            if p.ist_zeichen(Zeichen::DoppelPunkt) || ende_einer_kurzform(p) {
                p.pos = merk;
                // J6: der Name noch **einmal** verbrauchen. GEMESSEN: ohne
                // das stand der Parser wieder auf `get`, erkannte den
                // Kurzform-Zweig und meldete `erwartet: '}'` — jedes
                // `{get: …}`/`{set: …}` in einem Objektliteral war ein
                // Syntaxfehler. Gefunden an echten Seitenskripten
                // (`de.wikipedia.org/w/load.php`: `Map.prototype={get:…}`,
                // Fehler 1:696) — in J6 gemeldet.
                p.weiter();
            } else {
                let q_start = p.pos; // J19: der Name beginnt den Quelltext
                let name = parse_eigenschaft_name(p)?;
                let (params, rest) = parse_param_liste(p)?;
                let rumpf = parse_funktionsrumpf(p)?;
                // J31: `use strict` bei nicht-einfacher Parameterliste.
                // GEMESSEN (node 22): `var o = { m({a}) { 'use strict'; } }`
                // ist ein SyntaxError.
                fruehfehler_strict_mit_parametern(p, &params, rest.as_ref(), &rumpf)?;
                let bau = FunktionsBau {
                    params,
                    rest,
                    rumpf,
                    pfeil: false,
                    asynchron: false,
                    generator: false,
                    konstruktor: false,
                    // J19: Quelltext der Methode.
                    quelle: p.quelle_von_bis(q_start, p.pos),
                    // J28: die Zeile der Getter-/Setter-Deklaration.
                    zeile: zeile_von(p, q_start),
                };
                teile.push(if art_get {
                    GegenstandTeil::Getter(name, bau)
                } else {
                    GegenstandTeil::Setter(name, bau)
                });
                if !p.frisst_zeichen(Zeichen::Komma) {
                    break;
                }
                continue;
            }
        }
        // async-Methode?
        let mut asynchron = false;
        let mut name_opt = erster;
        // Release 10.10.2026: `async *m(){}` (asynchrone Generator-Methode) —
        // der Stern steht VOR dem Namen. Vorher: „Generatoren werden nicht
        // unterstuetzt".
        let mut async_generator = false;
        if name_opt.as_deref() == Some("async")
            && !folgender_ist_klammer(p)
            && !p.ist_zeichen(Zeichen::DoppelPunkt)
            && !ende_einer_kurzform(p)
        {
            // `async: 1` bleibt Paar.
            asynchron = true;
            async_generator = p.frisst_zeichen(Zeichen::Stern);
            name_opt = p.frisst_name();
        }
        // Generator-Methode `*m(){}`.
        //
        // J32: `generator` ist eine **eigene** Variable dieses Pfades. Im
        // Objektliteral wird das `*` **hier** gelesen; im Klassenrumpf
        // weiter oben. Ohne die eigene Variable landet das `generator = true`
        // in einer Variable aus einem **aeusseren** Scope — der `Lexer`
        // kompiliert dann, `FunktionsBau.generator` ist `false`, und der
        // Rumpf wird nicht als Generator behandelt. GEMESSEN, wie das
        // aussieht: `tests/j16_bundel.rs::yield_mit_feld_und_objektliteral`
        // wurde rot (`TypeError: kein Zugriff auf Undefiniert` statt `1`) —
        // ein Test gegen einen **echten** github-Bündel.
        let mut generator = async_generator;
        if !generator && name_opt.is_none() && p.frisst_zeichen(Zeichen::Stern) {
            generator = true;
        }
        let name = match name_opt {
            Some(s) => EigenschaftName::Text(s),
            None => parse_eigenschaft_name(p)?,
        };
        if p.ist_zeichen(Zeichen::LRunde) {
            let war_in_generator = p.in_generator;
            // J32: dieselbe Parameterlisten-Regel wie bei `function*` und
            // `class { *g(){} }` (ES2023 15.4.1). GEMESSEN (node 22):
            // `var o = { *g(a = yield) {} }` -> SyntaxError. Die erste
            // Fassung dieses Flags hat nur die beiden anderen Pfade
            // bekommen, also blieb genau diese Form durchgewunken.
            let param_vorher = p.in_generator_param;
            p.in_generator_param = generator;
            if generator {
                p.in_generator = true;
            }
            let (params, rest) = parse_param_liste(p)?;
            // J32: siehe `parse_funktion` — der Rumpf ist nie eine
            // Parameterliste, also **null** statt zurueck, und die
            // Wiederherstellung kommt **nach** dem Rumpf.
            p.in_generator_param = false;
            let rumpf = parse_funktionsrumpf(p)?;
            p.in_generator_param = param_vorher;
            p.in_generator = war_in_generator;
            // J31: siehe oben — die Methode im Objektliteral.
            fruehfehler_strict_mit_parametern(p, &params, rest.as_ref(), &rumpf)?;
            teile.push(GegenstandTeil::Methode(
                name,
                FunktionsBau {
                    params,
                    rest,
                    rumpf,
                    pfeil: false,
                    asynchron,
                    generator,
                    konstruktor: false,
                    // J19: Quelltext der Methode.
                    quelle: p.quelle_von_bis(q_start_m, p.pos),
                    zeile: 0,
                },
            ));
        } else if p.frisst_zeichen(Zeichen::DoppelPunkt) {
            let w = parse_zuweisung(p)?;
            teile.push(GegenstandTeil::Paar(name, Box::new(w)));
        } else {
            // Kurzform `{a}` — nur bei Textnamen.
            match name {
                // Release 10.10.2026: `{a = 1}` (ES2023 13.2.5
                // `CoverInitializedName`) — gueltig nur als Zuweisungsziel
                // (`({a = 1} = o)`), hier als `{a: a = 1}` gelesen; der
                // Musterweg (`muster_fuer_ziel`) macht daraus `a` mit
                // Standardwert. GEMESSEN vorher (node 22 in Klammern):
                // `var a; ({a = 1} = {}); a` -> Syntaxfehler  (1).
                EigenschaftName::Text(s) if p.ist_zeichen(Zeichen::Gleich) => {
                    p.weiter();
                    let standard = parse_zuweisung(p)?;
                    teile.push(GegenstandTeil::Paar(
                        EigenschaftName::Text(s.clone()),
                        Box::new(Ausdruck::Zuweisung {
                            ziel: ZuweisungsZiel::Name(s),
                            op: ZuweisungsOp::Gleich,
                            wert: Box::new(standard),
                        }),
                    ));
                }
                EigenschaftName::Text(s) => teile.push(GegenstandTeil::Kurz(s)),
                _ => return Err(p.fehler("erwartet: ':' nach Eigenschaftsname".to_string())),
            }
        }
        let _ = asynchron;
        if !p.frisst_zeichen(Zeichen::Komma) {
            break;
        }
    }
    p.erwartet_zeichen(Zeichen::RGeschweift, "'}'")?;
    Ok(Ausdruck::Gegenstand(teile))
}

fn ist_bindungsname_verboten(art: &TokenArt, strikt: bool, in_async: bool) -> Option<String> {
    match art {
        TokenArt::Wort(w) if schluessel_als_bindung_verboten(*w, strikt, in_async) => {
            Some(wort_text(*w).to_string())
        }
        TokenArt::BezeichnerEscape(n) => crate::lex::schluesselwort(n)
            .filter(|w| schluessel_als_bindung_verboten(*w, strikt, in_async))
            .map(|_| n.clone()),
        _ => None,
    }
}
