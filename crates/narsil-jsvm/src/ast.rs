// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! AST-Knoten (J2). Der Parser baut daraus einen Baum; der Interpreter
//! laeuft darueber. Alle `Box`-Schachtelungen sind durch die Parser-
//! Tiefengrenze gedeckelt (M5).

/// Ganzes Programm: Anweisungsfolge + Strict-Kennzeichen.
#[derive(Debug, Clone)]
pub struct Programm {
    pub anweisungen: Vec<Anweisung>,
    pub strikt: bool,
    /// J10 (ES-Module): die Modul-Angaben. `None` = normales Skript
    /// (`<script>` ohne `type="module"`), dann sind `import`/`export`
    /// ein Syntaxfehler (so ist es in node auch).
    pub modul: Option<Modul>,
}

/// J10: was der Parser an `import`/`export` gefunden hat.
///
/// GEMESSEN, warum das **im AST** steht und nicht in einer Tabelle des
/// Motors: die Anweisungen selbst bleiben normale Anweisungen
/// (`export const x = 1` ist eine `Deklaration`, `export default e` wird
/// zu `const __jsvm_default__ = e`) — damit braucht der Uebersetzer keine
/// Aenderung, und der Loader kann nach dem Lauf einfach die Namen aus der
/// Modul-Umgebung lesen.
#[derive(Debug, Clone, Default)]
pub struct Modul {
    /// Die Importe in Quelltext-Reihenfolge (die Reihenfolge der
    /// Verarbeitung im Loader; fuer die Bindings selbst zaehlt die
    /// Spezifikation nichts).
    pub import: Vec<Import>,
    /// Die Exporte: externer Name, interner Name, optionale Quelle
    /// (`export { a as b } from 'm'` / `export * from 'm'`).
    pub export: Vec<Export>,
}

/// J10: eine Import-Anweisung.
#[derive(Debug, Clone)]
pub struct Import {
    pub art: ImportArt,
    /// Die Quell-Angabe, wie sie im Quelltext stand. Der Loader loest sie
    /// gegen die Herkunft auf (relative URLs) — die Bruecke des Hosts
    /// (narsil) macht das mit ihrem HTTP-Client.
    pub quelle: String,
}

/// J10: welche Bindungen ein Import liefert.
#[derive(Debug, Clone, Default)]
pub enum ImportArt {
    /// `import 'm';` — nur zum Ausfuehren (Seiteneffekte).
    #[default]
    Leer,
    /// `import d from 'm'` — der Vorgabewert als `d`.
    Vorgabe { name: String },
    /// `import { a, b as c } from 'm'` — Paare (extern, intern).
    Liste { paare: Vec<(String, String)> },
    /// `import * as ns from 'm'` — ein Objekt mit allen Exporten.
    Alles { name: String },
    /// `import d, { a as b }, * as ns from 'm'`.
    Gemischt {
        vorgabe: Option<String>,
        paare: Vec<(String, String)>,
        alles: Option<String>,
    },
}

/// J10: ein Export-Eintrag.
#[derive(Debug, Clone)]
pub struct Export {
    /// Der Name, unter dem der Importeur sieht. `*` = `export * from 'm'`.
    pub extern_: String,
    /// Der Name in der Modul-Umgebung (bei `from` der Name im
    /// Quellmodul).
    pub intern: String,
    /// `Some(quelle)` bei `export … from '…'`.
    pub quelle: Option<String>,
}

/// Eine Anweisung.
#[derive(Debug, Clone)]
pub(crate) enum Anweisung {
    /// J28: eine Anweisung **mit ihrer Quelltextzeile**.
    ///
    /// GEMESSEN, warum ein eigener Knoten und kein Feld an allen
    /// Varianten: `Anweisung` hat 20 Varianten, und ein `zeile`-Feld an
    /// jeder waere 20 Aenderungen an jedem Musterabgleich im Parser und
    /// im Compiler. Dieser Knoten wird nur an **einer** Stelle gebaut
    /// (`parse_anweisung`) und nur an **einer** ausgewertet
    /// (`compile::anweisung`) — dort entsteht `Op::Zeile`.
    ///
    /// Die Zeile ist die Zeile des **ersten Tokens** der Anweisung (der
    /// Lexer zaehlt ab 1, wie `editor` und die Devtools-Oberflaeche).
    MitZeile { zeile: u32, innen: Box<Anweisung> },
    /// `{ ... }`.
    Block(Vec<Anweisung>),
    /// `var x = 1, y;` / `let` / `const`.
    Deklaration {
        art: DeklArt,
        eintraege: Vec<(Muster, Option<Ausdruck>)>,
    },
    /// Ausdrucksanweisung (`f();`, `x++;` ...).
    Ausdruck(Ausdruck),
    /// `if (a) b else c`.
    Wenn {
        bedingung: Ausdruck,
        dann: Box<Anweisung>,
        sonst: Option<Box<Anweisung>>,
    },
    /// `while (a) b`, `do b while (a)`.
    Solange {
        bedingung: Ausdruck,
        rumpf: Box<Anweisung>,
        nachlauf: bool,
    },
    /// Klassische `for`-Schleife (mit/ohne Deklaration im Kopf).
    Fuer {
        kopf: FuerKopf,
        bedingung: Option<Ausdruck>,
        schritt: Option<Ausdruck>,
        rumpf: Box<Anweisung>,
    },
    /// `for (x in obj)` / `for (x of iter)`.
    FuerIn {
        art: DeklArtOpt,
        ziel: Muster,
        quelle: Ausdruck,
        of: bool,
        rumpf: Box<Anweisung>,
        /// Release 10.10.2026: `for await (… of …)`.
        warte: bool,
    },
    /// `switch (a) { case ...: ...; default: ... }`.
    Auswahl {
        wert: Ausdruck,
        faelle: Vec<(Option<Ausdruck>, Vec<Anweisung>)>,
    },
    /// `return expr?`.
    Rueckgabe(Option<Ausdruck>),
    /// `throw expr`.
    Werfen(Ausdruck),
    /// `try { } catch (e) { } finally { }`.
    Versuch {
        rumpf: Vec<Anweisung>,
        fang_name: Option<Muster>,
        fang: Vec<Anweisung>,
        endlich: Vec<Anweisung>,
        /// `true`, wenn `catch` (ggf. ohne Bindung/leer) vorhanden ist.
        hat_catch: bool,
    },
    /// `break label?`, `continue label?` (Label nur geparkt, Sprung nur
    /// unmarkiert — markierte Schleifen werden wie unmarkierte behandelt,
    /// ausser der Name steht an einer Schleife und passt).
    Abbruch { weiter: bool, marke: Option<String> },
    /// `function name(...) { ... }`.
    Funktion {
        name: Option<String>,
        funktion: FunktionsBau,
    },
    /// `class Name [extends X] { ... }`.
    Klasse {
        name: Option<String>,
        eltern: Option<Ausdruck>,
        elemente: Vec<KlassenElement>,
        /// J19: der Quelltext der Klasse (`class A { … }`) — er wird dem
        /// Konstruktor als `quelle` mitgegeben, damit `A.toString()` wie
        /// in node den Klassentext liefert.
        quelle: Option<std::sync::Arc<str>>,
    },
    /// `;` (leere Anweisung).
    Leer,
    /// `debugger` (kein Effekt).
    Haltepunkt,
    /// `with (obj) ...` (nur geparst, zur Laufzeit Fehler — Altlast).
    Mit(Ausdruck, Box<Anweisung>),
    /// `label: anweisung`.
    Marke(String, Box<Anweisung>),
}

impl Anweisung {
    /// J28: durch den `MitZeile`-Knoten **sehen**.
    ///
    /// GEMESSEN, warum das noetig ist (und wann es gemerkt wurde): der
    /// Knoten umschliesst **jede** Anweisung, und es gibt Stellen, die
    /// eine Anweisung **erkennen**, ohne sie zu uebersetzen — das Hissen
    /// (`compile.rs:322`, Funktionsdeklarationen) und
    /// `block_braucht_umgebung` (`compile.rs:3001`). Ohne diesen Helfer
    /// sahen sie nur noch `MitZeile` und **hissen keine Funktion mehr**:
    /// gemessen wurde das als `ReferenceError: 'p' ist nicht definiert` bei
    /// `function p(x){…} console.log(p(1))`.
    ///
    /// Die Gegenprobe steht in `tests/j28_debugger.rs`
    /// (`eine_funktionsdeklaration_bleibt_erreichbar`).
    /// J28: die Zeile in die Funktionsbauten **unterhalb** dieser
    /// Anweisung eintragen.
    ///
    /// GEMESSEN, warum `max`: eine verschachtelte Funktion kann ihre
    /// Zeile schon haben (von innen gesetzt), und 0 darf sie nicht
    /// ueberschreiben.
    ///
    /// GEMESSEN, warum die Zeile **per Parameter** und nicht aus dem
    /// Knoten gelesen wird: `match a { Anweisung::Funktion { funktion, .. }
    /// => … a.zeile … }` leiht `a` im Muster bereits mutabel aus (E0502).
    pub(crate) fn zeilen_setzen(a: &mut Anweisung, zeile: u32) {
        match a {
            Anweisung::MitZeile { innen, .. } => Self::zeilen_setzen(innen, zeile),
            Anweisung::Funktion { funktion, .. } => {
                funktion.zeile = zeile.max(funktion.zeile);
                for r in funktion.rumpf.iter_mut() {
                    Self::zeilen_setzen(r, 0);
                }
            }
            Anweisung::Block(v) => {
                for r in v.iter_mut() {
                    Self::zeilen_setzen(r, 0);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn inhalt(&self) -> &Anweisung {
        match self {
            Anweisung::MitZeile { innen, .. } => innen.inhalt(),
            a => a,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeklArt {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeklArtOpt {
    Keins,
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone)]
pub(crate) enum FuerKopf {
    Keiner,
    Ausdruck(Ausdruck),
    Deklaration {
        art: DeklArt,
        eintraege: Vec<(Muster, Option<Ausdruck>)>,
    },
}

/// Funktionskoerper-Bauplan (fuer Deklaration, Ausdruck, Pfeil, Methode).
#[derive(Debug, Clone, Default)]
pub(crate) struct FunktionsBau {
    pub params: Vec<Muster>,
    pub rest: Option<Muster>,
    pub rumpf: Vec<Anweisung>,
    /// Pfeilfunktion: kein eigenes `this`/`arguments`, kein `new`.
    pub pfeil: bool,
    /// `async function` (V3: haelt an, Fortsetzung als Mikrotask).
    pub asynchron: bool,
    /// `function*` (V3: haelt bei `yield` an).
    pub generator: bool,
    /// Konstruktor einer Klasse (dann `RueckgabeThis` am Ende).
    pub konstruktor: bool,
    /// J19: der **Quelltext** dieser Funktion, so wie er im Programm
    /// stand — fuer `Function.prototype.toString`.
    ///
    /// GEMESSEN, warum das gebraucht wird: Bibliotheken erkennen
    /// Merkmale ueber den Quelltext (`(() => {}).toString().includes('=>')`,
    /// Babel/core-js pruefen `async`, Web-APIs pruefen `[native code]`).
    /// Vorher lieferte der Motor immer `function f() { [code] }`.
    /// `Arc<str>`, weil `FunktionsBau` oft geklont wird (Klassenelemente,
    /// Verschachtelung) und der Text sonst kopiert wuerde.
    pub(crate) quelle: Option<std::sync::Arc<str>>,
    /// J28: die Zeile, an der die Funktion steht (`function f (…) {`).
    ///
    /// GEMESSEN, warum das hier und nicht erst im `Funktion` (Bytecode):
    /// der Compiler sieht an den meisten Aufrufstellen keine Zeile mehr
    /// (`MitZeile` hat sie schon an `anweisung` weitergegeben). Der
    /// **Parser** ist die einzige Stelle, die Anweisung und Zeile
    /// gleichzeitig hat — also schreibt er sie hier hinein.
    pub zeile: u32,
}

/// Klassenelement.
#[derive(Debug, Clone)]
pub(crate) enum KlassenElement {
    Methode {
        name: EigenschaftName,
        funktion: FunktionsBau,
        statisch: bool,
        art: MethodenArt,
    },
    Feld {
        name: EigenschaftName,
        wert: Option<Ausdruck>,
        statisch: bool,
    },
    /// Release 10.10.2026: `static { … }` (ES2022 ClassStaticBlock) — als
    /// sofort aufgerufener Pfeil (`this` = die Klasse, eigener `var`-
    /// Bereich), ausgewertet in der Reihenfolge der statischen Felder.
    Block { ausdruck: Ausdruck },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodenArt {
    Normal,
    Getter,
    Setter,
    Konstruktor,
}

/// Eigenschaftsname in Literalen/Klassen.
#[derive(Debug, Clone)]
pub(crate) enum EigenschaftName {
    Text(String),
    Zahl(f64),
    Berechnet(Box<Ausdruck>),
}

/// Zuweisungsziel-Muster (Destrukturierung).
#[derive(Debug, Clone)]
pub(crate) enum Muster {
    Name(String),
    Feld(Vec<FeldMuster>),
    Gegenstand(Vec<GegenstandMuster>),
    Standard(Box<Muster>, Box<Ausdruck>),
    Rest(Box<Muster>),
    /// Release 10.10.2026: ein MEMBER-Ziel in einem Zuweisungsmuster
    /// (`[o.a, a[i]] = …`, ES2023 13.15.5 `DestructuringAssignmentTarget`)
    /// — nur in Zuweisungen, nie in Deklarationen. Traegt einen
    /// `Ausdruck::Zugriff`.
    Ziel(Box<Ausdruck>),
}

#[derive(Debug, Clone)]
pub(crate) struct FeldMuster {
    pub muster: Muster,
    pub standard: Option<Box<Ausdruck>>,
}

#[derive(Debug, Clone)]
pub(crate) struct GegenstandMuster {
    pub schluessel: EigenschaftName,
    pub muster: Muster,
    pub standard: Option<Box<Ausdruck>>,
    /// Kurzform `{a}` = Schluessel `a`, Ziel Name `a`.
    pub kurz: bool,
}

impl FunktionsBau {
    /// `true`, wenn die Bauart ein Klassenkonstruktor ist.
    pub(crate) fn ist_konstruktor_marke(&self) -> bool {
        self.konstruktor
    }
}

impl GegenstandMuster {
    /// `true` bei Rest-Eintrag `{...rest}` (leerer Schluessel als Markierung).
    pub(crate) fn schluessel_ist_leer(&self) -> bool {
        matches!(&self.schluessel, EigenschaftName::Text(s) if s.is_empty())
    }
}

/// Ein Ausdruck.
#[derive(Debug, Clone)]
pub(crate) enum Ausdruck {
    Zahl(f64),
    /// J24: BigInt-Literal (`123n`).
    Ganz(i128),
    Text(String),
    Template(Vec<TemplateTeil>),
    /// Release 15.10.2026: the template object of a tagged template
    /// (ECMA-262 13.2.8.4 GetTemplateObject): cooked strings (`None` for
    /// an invalid escape) and raw strings, one per piece.
    VorlagenObjekt(Box<(Vec<Option<String>>, Vec<String>)>),
    Regex {
        muster: String,
        schalter: String,
    },
    Wahr(bool),
    Null,
    Undefiniert,
    Dies,
    Super,
    /// W14 (JS, 04.10.2026): `new.target` — ES2023 13.3.7.2
    /// (`MemberExpression : new . new.target`). Frueher hat der Parser das
    /// mit „new.target wird nicht unterstuetzt" abgelehnt, obwohl die
    /// Laufzeitseite (`Rahmen::neu`) **vollstaendig** war: `new F()` setzt
    /// es (vm.rs) und `super()` reicht es weiter.
    NeuZiel,
    Name(String),
    Feld(Vec<FeldTeil>),
    Gegenstand(Vec<GegenstandTeil>),
    Funktion {
        name: Option<String>,
        funktion: Box<FunktionsBau>,
    },
    Klasse {
        name: Option<String>,
        eltern: Option<Box<Ausdruck>>,
        elemente: Vec<KlassenElement>,
        /// J19: der Quelltext der Klasse (siehe `Anweisung::Klasse`).
        quelle: Option<std::sync::Arc<str>>,
    },
    Aufruf {
        ziel: Box<Ausdruck>,
        args: Vec<Ausdruck>,
        streu: Vec<bool>,
        /// `?.()`-Aufruf: kurzschliessen bei nullish (J2: `?.`).
        optional: bool,
    },
    Neu {
        ziel: Box<Ausdruck>,
        args: Vec<Ausdruck>,
        streu: Vec<bool>,
    },
    Zugriff {
        objekt: Box<Ausdruck>,
        name: ZugriffName,
        optional: bool,
    },
    Zuweisung {
        ziel: ZuweisungsZiel,
        op: ZuweisungsOp,
        wert: Box<Ausdruck>,
    },
    Update {
        ziel: ZuweisungsZiel,
        hoch: bool,
        nach: bool,
    },
    Binaer {
        op: BinaerOp,
        links: Box<Ausdruck>,
        rechts: Box<Ausdruck>,
    },
    Unaer {
        op: UnaerOp,
        ausdruck: Box<Ausdruck>,
        nach: bool,
    },
    Bedingt {
        bedingung: Box<Ausdruck>,
        dann: Box<Ausdruck>,
        sonst: Box<Ausdruck>,
    },
    Folge(Vec<Ausdruck>),
    /// `a && b`, `a || b`, `a ?? b`.
    Logisch {
        op: LogischOp,
        links: Box<Ausdruck>,
        rechts: Box<Ausdruck>,
    },
    /// `yield` / `yield expr` (V3).
    Ertrag(Option<Box<Ausdruck>>),
    /// `yield* expr` (V3).
    ErtragStern(Box<Ausdruck>),
    /// `await ausdruck` (nur in async-Funktionen sinnvoll).
    Erwartung(Box<Ausdruck>),
}

#[derive(Debug, Clone)]
pub(crate) enum TemplateTeil {
    Text(String),
    Ausdruck(Box<Ausdruck>),
}

#[derive(Debug, Clone)]
pub(crate) enum FeldTeil {
    Wert(Box<Ausdruck>),
    Streu(Box<Ausdruck>),
    Loch,
}

#[derive(Debug, Clone)]
pub(crate) enum GegenstandTeil {
    Paar(EigenschaftName, Box<Ausdruck>),
    Kurz(String),
    Methode(EigenschaftName, FunktionsBau),
    Getter(EigenschaftName, FunktionsBau),
    Setter(EigenschaftName, FunktionsBau),
    Streu(Box<Ausdruck>),
}

#[derive(Debug, Clone)]
pub(crate) enum ZugriffName {
    Text(String),
    Ausdruck(Box<Ausdruck>),
    Privat(String),
}

#[derive(Debug, Clone)]
pub(crate) enum ZuweisungsZiel {
    Name(String),
    Zugriff {
        objekt: Box<Ausdruck>,
        name: ZugriffName,
        optional: bool,
    },
    Muster(Muster),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZuweisungsOp {
    Gleich,
    Plus,
    Minus,
    Stern,
    Schrag,
    Prozent,
    SternStern,
    KleinerKleiner,
    GroesserGroesser,
    GroesserGroesserGroesser,
    Und,
    Oder,
    Hoch,
    UndUnd,
    OderOder,
    FrageFrage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaerOp {
    Plus,
    Minus,
    Stern,
    Schrag,
    Prozent,
    SternStern,
    Kleiner,
    KleinerGleich,
    Groesser,
    GroesserGleich,
    GleichGleich,
    RufGleich,
    GleichGleichGleich,
    RufGleichGleich,
    Instanceof,
    In,
    KleinerKleiner,
    GroesserGroesser,
    GroesserGroesserGroesser,
    Und,
    Oder,
    Hoch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaerOp {
    Minus,
    Plus,
    Ruf,
    Tilde,
    Typeof,
    Void,
    Delete,
    PrePlusPlus,
    PreMinusMinus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogischOp {
    Und,
    Oder,
    FrageFrage,
}
