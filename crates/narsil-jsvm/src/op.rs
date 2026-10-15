// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Bytecode der VM (WELLE6, V1).
//!
//! # WARUM EIGENER BYTECODE UND KEIN BAUM
//!
//! Der Baum-Interpreter in `narsil-js` benutzt den **Rust-Stapel** als
//! Fortsetzung: die Fortsetzung eines `Ausdruck`-Knotens ist kein Wert.
//! Damit kann `await` nicht anhalten und `yield` gar nicht existieren
//! (gemessen in `crates/narsil-js/tests/await_reihenfolge.rs`).
//!
//! Hier ist die Fortsetzung ein `pc` (Programmzaehler) in einem `Vec<Op>`,
//! der Rahmen ein Wert in `Vec<Rahmen>`. Beides sind **Daten**, also kann ein
//! Lauf an jeder Stelle anhalten und spaeter genau dort fortsetzen (V3). Kein
//! JS-Aufruf braucht Rust-Rekursion (V1); die Grenze ist die Zahl der
//! Rahmen, der `RangeError` ist fangbar (V4).
//!
//! Literale tragen ihre Zahl direkt im Befehl (kein Zahlenpool), Texte und
//! Funktionsindizes liegen im Pool.

/// BEGRUENDUNG fuer `dead_code` (gemessen am 30.09.2026): Bytecode-/Befehls-Oberflaeche. Einige Befehle und Felder sind fuer die
/// naechsten Schritte (Generator, statische Felder, `exec`) vorbereitet und
/// werden von der VM noch nicht erzeugt — sie bleiben benannt, damit die
/// Befehlsnummerierung nicht wandert.
#[allow(dead_code)]
use crate::ast::MethodenArt;

/// Ein Befehl der Stapelmaschine.
///
/// Release 09.10.2026: `Copy` — kein Befehl traegt Heap-Daten; die Schleife
/// holt jeden Befehl als Kopie (vorher `clone()` mit Variantenabgleich).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Op {
    /// J28: **die Quelltextzeile** — reine Information fuer den
    /// Debug-Haken, keine Wirkung auf den Lauf.
    ///
    /// GEMESSEN, warum ein Befehl und nicht eine Tabelle `pc -> zeile`:
    /// die Tabelle waere ein zweiter Vektor je Funktion, der beim
    /// Uebersetzen **mit**laufen muss (jeder `pc` von Hand nachgetragen),
    /// und sie waere bei jedem Sprung (`? :`, `&&`, Schleife) an **mehreren**
    /// `pc` richtig — ein `Op::Zeile` an der Anweisung ist immer genau
    /// eine Stelle. Kostet: **ein Schritt je Anweisung** (bei 20.000
    /// Schritten je Frame also 0,005 %; gemessen 0,0 % Laufzeit, siehe
    /// `tests/j28_debugger.rs::schritt_zahl_aendert_sich_nur_gering`).
    ///
    /// GEMESSEN, warum `u32` und nicht `u16`: eine Skriptzeile kann in
    /// eingebetteten Skripten (HTML-Attribut `onclick`) weit hinten
    /// liegen, und `u16` waere bei 65.536 Zeilen stumm falsch — eine
    /// falsche Zeilennummer ist schlimmer als kein Debugger.
    Zeile(u32),

    /// J28: `debugger;` — **die Anweisung des Programms**.
    ///
    /// GEMESSEN, warum das kein Sonderfall im Parser ist: `debugger` ist
    /// ein Schluesselwort, das als Anweisung **mit** allem anderen
    /// uebersetzt wird (J20-Slot auf `undefined`). Ohne diesen Befehl
    /// waere es genau das, was der Wunsch beschreibt: geparst und
    /// verworfen.
    Debugger,

    // --- Literale ---
    Null,
    Undef,
    Wahr,
    Falsch,
    /// Zahl direkt im Befehl.
    Zahl(f64),
    /// J24: BigInt-Literal (`GVal::Ganz`).
    Ganz(i128),
    /// Text aus `Programm::texte`.
    Text(u32),

    // --- Stapel ---
    /// `n` Werte verwerfen.
    Pop(u8),
    /// Kopie des obersten Werts.
    Dup,
    /// Kopie des Werts, der `n` Plaetze unter dem obersten liegt (0 = oben).
    Pick(u8),
    /// Linke Rotation der obersten `n+1` Werte: [a,b,c] -> [b,c,a].
    DreheLinks(u8),
    /// Platzhalter (undefined) — reserviert einen Slot ohne Nettoeffekt.
    Reserviere,

    // --- Variablen (Umgebungen liegen auf dem Heap) ---
    /// Name lesen (Kette, dann global). Nameindex.
    LeseVar(u32),
    /// Name schreiben (legt sloppy an). Nameindex.
    SchreibeVar(u32),
    /// Deklarieren und mit dem obersten Wert initialisieren
    /// (Nameindex, Art: 0 = var, 1 = let, 2 = const).
    DefVar(u32, u8),
    /// J48: legt eine **Parameter**-Bindung an — im Gegensatz zu `DefVar`
    /// in der **Parameterumgebung**, nicht in der des Rumpfs.
    ///
    /// GEMESSEN (node 22), der Anlass:
    ///   `var x = 'outside';
    ///    var pp, pb;
    ///    function f(a = (pp = function(){ return x; })) {
    ///      var x = 'inside'; pb = function(){ return x; };
    ///    }
    ///    f(); pp()` -> `outside`, `pb()` -> `inside`
    ///
    /// In **einer** Umgebung sieht beides `inside` — die Deklaration des
    /// Rumpfs waere fuer den Ausdruck in der Parameterliste sichtbar.
    /// ES2023 10.2.11 Schritt 27 verlangt dafuer einen **zweiten**
    /// Environment Record (`varEnv`).
    DefParam(u32, u8),
    /// J48: schaltet die Namensauflösung zwischen Rumpfumgebung (`false`)
    /// und **Parameter**umgebung (`true`). Der Compiler stellt ihn um den
    /// Parameterprolog.
    ///
    /// GEMESSEN (node 22), warum das noetig ist und nicht nur eine zweite
    /// Umgebung: ein **Ausdruck** in der Parameterliste wird ausgewertet,
    /// waehrend `Rahmen::umg` schon die des Rumpfs ist. Ohne den
    /// Umschalter sah er `var x` des Rumpfs:
    ///   `var x = 'outside';
    ///    var pp; function f(a = (pp = function(){ return x; })) { var x = 'i' }
    ///    f(); pp()` -> node `outside`, ohne Umschalter `inside`
    ParamProlog(bool),
    /// Zuweisung an eine bestehende Variable; haelt den Wert als Ausdruck.
    ZuweisVar(u32),
    /// `this` des Rahmens (Pfeilfunktionen erben von aussen).
    This,
    /// W14 (JS, 04.10.2026): `new.target` — legt `Rahmen::neu` auf den
    /// Stapel (ES2023 13.3.7.2 Schritt 4: „Return function.[[NewTarget]]").
    /// Ausserhalb einer Funktion wirft der Befehl den `SyntaxError` der
    /// Norm (Schritt 3).
    NeuZiel,
    /// Release 09.10.2026: der naechste `Aufruf` ist ein **direktes**
    /// `eval(...)` (ES2023 13.3.6.1: nur der Bezeichner `eval` als Ziel).
    EvalDirekt,
    /// `arguments` anlegen und im Rahmen-Scope binden.
    DefArgumente,
    /// `arguments[i]` bzw. undefined.
    Param(u16),
    /// Feld aus `arguments[k..]`.
    RestParams(u16),
    /// `typeof` ohne Referenz (freier Bezeichner -> "undefined").
    TypVonName(u32),

    // --- Operatoren ---
    Plus,
    Minus,
    Mal,
    Geteilt,
    Modulo,
    Potenz,
    Negativ,
    Positiv,
    LogischNicht,
    BitNicht,
    TypVon,
    LeerWert,
    Gleich,
    Ungleich,
    StriktGleich,
    StriktUngleich,
    Kleiner,
    KleinerGleich,
    Groesser,
    GroesserGleich,
    InOperator,
    InstanceVon,
    BitUnd,
    BitOder,
    BitXor,
    LinksSchieben,
    RechtsSchieben,
    UnsRechtsSchieben,
    /// J3b: die Rahmenumgebung auf den Temp-Wert setzen.
    ///
    /// GEMESSEN/GEFUNDEN: ein `continue` aus einem Block heraus
    /// ueberspringt dessen `BlockAb` — die Rahmenumgebung blieb die des
    /// Rumpfes, und die naechste Runde bekam eine **Kette von
    /// Rumpfumgebungen** (der Schritt schrieb in die falsche Bindung,
    /// `for (let i=0;i<5;i++){ if (i%2===0) continue; s+=i }` lieferte `0`
    /// statt `4`). Der Pro-Durchlauf-Zweig reparierte das von selbst
    /// (sein Update-Teil beginnt mit `UmgebungAusTemp`); die einfache
    /// Form tut es jetzt ausdruecklich.
    UmgebungTempSetzen(u16),
    /// J3b: **`name < zahl` als eine Anweisung** — liest die Variable,
    /// vergleicht und springt bei falsch. Ersetzt `LeseVar` + `Zahl` +
    /// `Kleiner` + `SpringWennFalsch` (4 Befehle -> 1).
    ///
    /// GEMESSEN: `for (let i = 0; i < N; i++)` hatte **26 Schritte je
    /// Durchlauf**; davon waren allein 8 der Test und der Schritt. Der
    /// Wert wird wie bei `SpringWennFalsch` verworfen.
    ///
    /// Die Semantik bleibt die normale: ist der Wert keine Zahl, rechnet
    /// der Befehl ueber den gewohnten Vergleichsweg (`Op::Kleiner`), damit
    /// `"10" < 9` weiterhin `false` ergibt.
    TestWertKleinerZahlSpring(u32, f64, u32),
    /// J3b: **`name++` als Anweisung** — erhoeht die Variable und legt
    /// **nichts** auf den Stapel (der Wert des Updates wird in einer
    /// `for`-Schrittangabe nicht benutzt). Ersetzt `LeseVar` + `Zahl(1)`
    /// + `Plus` + `SchreibeVar` + `Pop` (5 Befehle -> 1).
    ErhoeheUndSchreibe(u32),
    /// `++` auf dem obersten Wert.
    Inc,
    /// `--` auf dem obersten Wert.
    Dec,

    // --- Sprünge (Ziel = Index im Code) ---
    Spring(u32),
    /// Springe, wenn der oberste Wert falsch ist (Wert wird verworfen).
    SpringWennFalsch(u32),
    /// Springe, wenn der oberste Wert falsch ist (Wert BLEIBT liegen).
    ///
    /// GEMESSEN (A5): `SpringWennFalsch` verwirft den Wert immer — fuer
    /// Anweisungen (`if`, `while`) richtig, im AUSDRUCK `a && b` aber falsch:
    /// das nachfolgende `Pop(1)` fraess dann einen fremden Stapelwert
    /// (`console.log("ende", a && b ? 1 : 2)` lieferte „Operandenstapel zu
    /// kurz", Argumente wurden verschoben). Dieses Op behält den Wert.
    SpringWennFalschBehalten(u32),
    /// Springe, wenn der oberste Wert wahr ist (Wert bleibt liegen).
    SpringWennWahr(u32),
    /// Springe, wenn der oberste Wert `undefined` ist (Wert bleibt liegen).
    SpringWennUndef(u32),
    /// Springe, wenn der oberste Wert NICHT nullish ist (Wert bleibt liegen).
    SpringWennNichtNullish(u32),

    /// Release 10.10.2026: einen Helfer des Vorspanns auf den Stapel legen
    /// (`asyncgen::HILFE_*`).
    Hilfe(u8),

    // --- Aufrufe ---
    /// `f(this, arg1..argn)`; Reihenfolge auf dem Stapel: this, argn..arg1,
    /// dann f (oben). Der Befehl ersetzt alles durch den Aufruferwert.
    Aufruf(u16),
    /// `f(this, ...feld)` — Argumentliste als Feld unter `this`.
    AufrufStreu,
    /// `new F(arg1..argn)`.
    AufrufNeu(u16),
    /// `new F(...feld)`.
    AufrufNeuStreu,
    /// `super.f(arg1..argn)` — Reihenfolge wie `AufrufMethode`.
    AufrufSuper(u16),
    /// `super.f(...feld)`.
    AufrufSuperStreu,
    /// `objekt.schluessel(arg1..argn)`; Stapel: this, key, argn..arg1, objekt.
    AufrufMethode(u16),
    /// Release 09.10.2026: `basis.name(args)` mit FESTEM Namen (Textindex).
    /// Wie `AufrufMethode`, nur liegt der Schluessel nicht auf dem Stapel:
    /// der Name kommt aus dem Textpool und die Suche nutzt den Inline-Cache
    /// (`hole_ic`). GEMESSEN vorher: jeder Methodenaufruf machte aus dem
    /// Schluessel einen neuen `String` (`rechnen_schluessel`).
    AufrufMethodeText(u16, u32),
    /// `objekt.schluessel(...feld)`.
    AufrufMethodeStreu,
    /// `super.schluessel(arg1..argn)` — Suche ab Heimat-Prototyp.
    /// Stapel (unten): arg1..argn, schluessel.
    AufrufSuperMethode(u16),

    // --- super ---
    /// `super.name` lesen (Nameindex).
    SuperLese(u32),
    /// `super[ausdruck]` lesen (Schluessel liegt auf dem Stapel).
    SuperLeseDynamisch,
    /// `super.name = wert` (der Wert liegt auf dem Stapel).
    SuperSchreibe(u32),

    // --- Rueckgabe ---
    /// Rumpf beenden; der oberste Wert ist das Ergebnis.
    Rueckgabe,
    /// Wie `Rueckgabe`, aber der Wert wird als **Aktion** vorgemerkt und der
    /// `finally`-Block laeuft zuerst (sec-14.15.2: `try { return 'a' } finally
    /// { return 'b' }` liefert `b`).
    RueckgabeVormerken,
    /// Rumpf beenden, Ergebnis ist `this` (abgeleiteter Konstruktor).
    RueckgabeThis,
    /// Rumpf verlassen, Ergebnis `undefined` (`break` ausserhalb, tolerant).
    RueckgabeUndefined,

    // --- Objekte und Felder ---
    /// Neues Objekt auf den Stapel.
    NeuesObjekt,
    /// `wert` an `name` haengen (Nameindex); Stapel: objekt, wert.
    DefEigenschaft(u32),
    /// Wie oben, Schluessel als Wert auf dem Stapel; Stapel: objekt, wert, key.
    DefEigenschaftNamen,
    /// Getter/Setter definieren (Nameindex, 0 = Getter, 1 = Setter).
    DefZugriff(u32, u8),
    /// Wie oben mit Schluessel auf dem Stapel.
    DefZugriffNamen(u8),
    /// `ziel = {...quelle}` (Quelle liegt oben, Ziel darunter).
    StreuKopieren,
    /// Neues Feld auf den Stapel.
    NeuesFeld,
    /// Wert an das Feld unter dem obersten Wert haengen.
    FeldAnhaengen,
    /// Den **letzten** Index des Feldes unter dem obersten Wert zur Luecke
    /// machen (`[1,,3]`, sec-12.2.2). Der Wert selbst bleibt `undefined`.
    FeldLoch,
    /// Alle Elemente der Quelle an das Feld haengen.
    FeldStreu,
    /// `objekt.name` lesen (Nameindex).
    LeseEigenschaft(u32),
    /// `objekt.name = wert`; Stapel: objekt, wert — laesst `wert` liegen.
    SchreibeEigenschaft(u32),
    /// `objekt[index]` lesen; Stapel: objekt, index.
    LeseIndex,
    /// `objekt[index] = wert`; Stapel: objekt, wert, index — laesst `wert` liegen.
    SchreibeIndex,
    /// Ziel-Zuweisung; Stapel: basis, key, wert — schreibt, laesst `wert` liegen.
    SchreibeZiel,
    /// Ziel-Zuweisung mit Schluessel im Temp; nach dem Poppen des Werts liegt
    /// die Basis `tiefe` Plaetze tiefer.
    SchreibeZielTemp(u16, u8),
    /// Wert aus der Quelle (Tiefe `q`, 0 = oben) mit dem Schluessel im Temp
    /// lesen und auf den Stapel legen.
    LeseZielTemp(u16, u8),
    /// `delete objekt.name` (Nameindex).
    LoescheEigenschaft(u32),
    /// W13 (JS, 05.10.2026): `delete` auf einen **blossen Namen**. Der Operand
    /// wird **nicht** geladen — eine unaufloesbare Referenz liefert `true`
    /// (ES2023 13.5.1.1 Schritt 3/6), und eine loeschbare `var`-Bindung wird
    /// wirklich entfernt (9.1.1.4.2). Siehe `Engine::delete_binding`.
    LoescheName(u32),
    /// Release 09.10.2026 (Annex B.3.3.3): den obersten Wert in die naechste
    /// `var`-Bindung dieses Namens schreiben (Block-Bindungen `let`/`const`
    /// werden uebersprungen) — die Funktionsdeklaration eines CaseBlocks
    /// setzt ihr `var` erst, wenn ihr Fall laeuft.
    VarSetzen(u32),
    /// `delete objekt[index]`.
    LoescheIndex,
    /// Restobjekt aus dem Quellobjekt (oberster Wert) — fuer `{...r}`.
    /// RUHE im Objekt-Muster: Operand = Index in `Funktion::muster_aus`.
    MusterRest(u32),
    /// Restfeld ab Index — fuer `[...r]`.
    MusterRestFeld(u32),

    // --- Umgebungen (Temps des Rahmens) ---
    /// Kind-Umgebung der Rahmenumgebung; Eltern-Umgebung in Temp `t`.
    BlockAuf(u16),
    /// J52: `with (objekt) { … }` — legt eine Umgebung an, die **zuerst** das
    /// Objekt sieht (ES2023 9.1.2.6 `Object Environment Record`).
    /// Das Objekt liegt auf dem Stapel, der Temp `t` merkt die alte Umgebung.
    ///
    /// GEMESSEN (node 22), **dreissig** Zeilen (`js52_node.js`), davon vier,
    /// die diese Befehle bestimmen:
    ///   `var o = {a: 1}; with (o) { a }`       -> `1`
    ///   `var o = {a: 1}; with (o) { a = 7 }`   -> `o.a` = `7`
    ///   `var o = {}; with (o) { b = 5 }`       -> `o.b` = `undefined`
    ///   `with (null) { … }` / `with (undefined)` -> **TypeError**
    MitStart(u16),
    /// Gegenstueck zu `MitStart`: die alte Umgebung aus dem Temp.
    MitEnde(u16),
    /// Rahmenumgebung aus Temp `t` zurueckholen und Temp leeren.
    BlockAb(u16),
    /// Aktuelle Rahmenumgebung in Temp `t` merken.
    TempUmgebungSetzen(u16),
    /// Rahmenumgebung = Kind der in Temp `t` gemerkten Umgebung.
    UmgebungAusTemp(u16),
    /// `anzahl` Namen ab `start` aus `quelle` (KEIN_TEMP = Rahmenumgebung)
    /// in die aktuelle Rahmenumgebung kopieren.
    KopiereNamen(u16, u16, u32),
    /// `anzahl` Namen ab `start` aus der aktuellen Rahmenumgebung in die
    /// in `ziel` gemerkte Umgebung kopieren (Liste aus `kopier_namen`).
    KopiereZurueck(u16, u16, u32),
    /// Temp setzen (oberster Wert), Temp lesen, Temp leeren.
    TempSet(u16),
    TempGet(u16),
    TempLoesche(u16),

    // --- Kontrolle (try/catch/finally ueber die Handler-Tabelle) ---
    /// Handler dieser Funktion aktivieren (Index in `Funktion::handler`).
    VersuchStart(u16),
    /// Handler deaktivieren.
    VersuchEnde,
    /// Fertig-Aktion vormerken (`Springen`, `Rueckgabe`, `Wurf`).
    FertigVormerken(u32),
    /// Vormerkte Fertig-Aktion ausfuehren.
    FertigEnde,
    /// Ausnahme werfen (oberster Wert).
    Wurf,

    // --- Vorlagen, Regex, Klassen, Funktionen ---
    /// `n` Teile (je ein Wert auf dem Stapel) zu einem Text verbinden.
    Vorlage(u16),
    /// Release 15.10.2026: the template object of a tagged template from
    /// `n` cooked values (text or undefined) and `n` raw texts on the stack;
    /// built once per site and cached (ECMA-262 13.2.8.4).
    VorlagenObjekt(u16),
    /// Release 15.10.2026: the method on top gets the object below it as
    /// [[HomeObject]] (object-literal methods and accessors, for `super`).
    HeimSetzen,
    /// In einen Text umwandeln (ToString).
    ZuText,
    /// Regex aus zwei Textkonstanten.
    Regex(u32, u32),
    /// Klasse anlegen (Index in `Programm::klassen`); Elternwert liegt darunter.
    Klasse(u32),
    /// Closure aus einem Funktionsindex erzeugen.
    NeuFunktion(u32),
    /// J34: `SetFunctionName` — der Wert **oben** bekommt den Namen dieses
    /// Namensindex, **nur wenn** er noch keinen hat (ES2023 8.2.5
    /// `NamedEvaluation`: „If IsAnonymousFunctionDefinition(value) is true,
    /// perform SetFunctionName(value, name)").
    NamenSetzen(u32),
    /// J41: wie `NamenSetzen`, aber der Name steht **oben** auf dem Stapel
    /// und benannt wird der Wert **darunter**.
    ///
    /// GEMESSEN (node 22), warum es eine eigene Operation braucht:
    ///   `var x = 'a'; class C { [x] = function(){} }` -> `new C().a.name`
    ///     = `"a"` — der Name ist der **Wert** des Schluesselausdrucks und
    ///     steht erst zur Laufzeit fest. Ein Index aus dem Namenpool kann
    ///     ihn nicht kennen.
    ///
    /// Der Stapel sieht dann so aus (siehe `feld_init`):
    /// `[.., this, wert, schluessel]` — der Schluessel ist die **oberste**
    /// Marke, der zu benennende Wert liegt darunter.
    NamenSetzenBerechnet,
    FelderStart(u32),

    // --- Anhalten und Fortsetzen (V3) ---
    /// `await wert` — haelt den Rahmen an; Fortsetzung als Mikrotask.
    Erwartung,
    /// `yield wert` — haelt den Generator an.
    Ertrag,
    /// J44: der Parameterprolog eines Generators ist **zu Ende** — ab hier
    /// sind `[x]`-Muster erlaubt. GEMESSEN (node 22):
    ///   `delete Array.prototype[Symbol.iterator];
    ///    class C { *m([x]){} }  C.prototype.m([1])` -> **TypeError beim
    ///   Aufruf** (nicht erst beim `next()`), weil das Muster einen Iterator
    ///   braucht, den es nicht mehr gibt. Die Marke wird beim **Start**
    ///   gesetzt (nicht beim ersten `next()`); in jedem anderen Lauf ist der
    ///   Befehl ein Nichts.
    GeneratorPrologEnde,
    /// `yield* wert`.
    ErtragStern,

    /// Iteration vorbereiten (Temp, 0 = of, 1 = in).
    IterStart(u16, u8),
    /// J34: die **Quelle** eines Feld-Musters auf einen Feldwert bringen:
    /// ist sie schon ein Feld (`HObj::Feld`), bleibt sie liegen; sonst wird
    /// der **Iterator** gefahren (ES2023 7.4.7) und das Ergebnis als Feld
    /// benutzt. Nahezu in der Mitte zwischen beiden Regeln zu stehen ist
    /// genau der Grund fuer diese Operation.
    MusterQuelle,
    /// Leeres Feldmuster `[] = q`: **nur** pruefen, dass `q` iterierbar ist.
    ///
    /// GEMESSEN (test262 `class/dstr/gen-meth-ary-ptrn-elem-ary-empty-init`,
    /// 12 Faelle in `class/dstr` + `generators/dstr`): `Op::MusterQuelle`
    /// materialisiert einen Generator per `iter_werte` — das **verbraucht**
    /// ihn und laesst den Rumpf laufen. Bei `[]` braucht das Muster aber
    /// **null** Werte: ES2023 7.4.9 `IteratorBindingInitialization` holt nur
    /// den Iterator, und `IteratorClose` bei normaler Fertigstellung ruft
    /// `return` **nicht**. Ein Generator muss unberuehrt bleiben.
    /// `MusterQuelle` kann das nicht wissen (er steht auch vor nichtleeren
    /// Mustern) — deshalb dieser eigene Befehl fuer `teile.is_empty()`.
    MusterQuelleLeer,
    /// Release 10.10.2026: die Quelle eines FELD-Musters (`[a, b] = q`) —
    /// wie `MusterQuelle`, materialisiert aber auch Map, Set und Objekte mit
    /// `@@iterator` (Iteratoren). `MusterQuelle` steht auch vor
    /// Objektmustern (`({size} = set)`), dort waere das falsch.
    /// GEMESSEN vorher (node 22 in Klammern): `var [a, b] = new Set([1, 2])`
    /// -> `undefined/undefined`  (`1/2`), ebenso `[k] = map.keys()`.
    MusterQuelleFeld,
    /// Naechsten Wert holen (auf den Stapel) oder zum Ziel springen.
    IterNaechster(u16, u32),
    /// Release 14.10.2026: IteratorClose (ES2024 7.4.11) for the lazy
    /// `for…of` run in temp `t` — calls `iterator.return()` if present; the
    /// stack is left untouched (a `return` value stays on it).
    IterSchliessen(u16),
    /// Nichts tun.
    Nichts,
}

/// Marke „kein Temp".
pub(crate) const KEIN_TEMP: u16 = u16::MAX;
/// Marke „kein Name" (unmarkiertes `break`/`continue`).
pub(crate) const KEIN_NAME: u32 = u32::MAX;

/// Obergrenze fuer Temp-Plaetze je Funktion (M4).
///
/// Ein Temp ist ein `GVal` (16 Byte) plus Slots. 05.10.2026: von 256 auf
/// **16.384** gehoben — GEMESSEN an youtubes ES5-Bundle: die globale
/// Funktion (das Skript selbst) brauchte mehr als 256 Temp-Plaetze
/// (`Speichergrenze: zu viele Temp-Plaetze in einer Funktion (M4)`), weil
/// jeder `for`/`try`/`catch`-Block im minifizierten Code einen belegt.
/// 16.384 Temps sind ~256 KiB — die Indexgrenze (`u16`) bleibt gewahrt.
pub(crate) const MAX_TEMPS: u16 = 16_384;
/// Obergrenze fuer Befehle je Funktion (M4: 1 Befehl = 24 Byte).
///
/// 05.10.2026: von 400.000 auf **2.000.000** gehoben. GEMESSEN an
/// youtubes ES5-Bundle (10,8 MB): die GLOBALE Funktion (das Skript selbst)
/// ueberschritt 400k Befehle mit `Speichergrenze: Funktion zu gross (M4)`.
/// Minifizierte Buendel legen ihren gesamten Startcode in eine Funktion;
/// 2 Mio Befehle sind im schlimmsten Fall ~48 MB Bytecode fuer EINEN
/// Aufruf — die Ausfuehrung bleibt durch `max_schritte`/`max_dauer_ms`
/// begrenzt, und der Speicher wird nach dem Lauf freigegeben.
pub(crate) const MAX_BEFEHLE: usize = 2_000_000;
/// Obergrenze fuer Konstanten (Texte/Funktionen) je Programm (M4).
/// Release 09.10.2026: die Indizes fuer Texte, Funktionen und Klassen sind
/// `u32` (vorher `u16` bei einer Grenze von 200 000 — ab 65 536 liefen sie
/// stumm ueber: youtube.com (10,8 MB) hat mehr Funktionen, ein `NeuFunktion`
/// zeigte dann auf den HAUPTLAUF, und das Skript begann von vorn).
pub(crate) const MAX_KONSTANTEN: usize = 4_000_000;
/// Obergrenze fuer Klassen je Programm (M4).
pub(crate) const MAX_KLASSEN: usize = 20_000;
/// Obergrenze fuer aktive Generator-/Versprechen-Rahmen je Generator (M5).
pub(crate) const MAX_GENERATOR_RAHMEN: usize = 10_000;

/// Ein Handler (try/catch/finally) einer Funktion.
#[derive(Debug, Clone)]
pub(crate) struct Handler {
    /// Sprungziel des `catch`-Blocks (0, wenn keiner).
    pub fang: u32,
    /// Sprungziel des `finally`-Blocks (nur bei `hat_finally`).
    pub finally_: u32,
    /// `true`, wenn es `finally` gibt.
    pub hat_finally: bool,
}

/// Eine kompilierte Funktion.
#[derive(Debug, Clone)]
pub struct Funktion {
    /// Der Bytecode als `Arc` — J3.
    ///
    /// GEMESSEN, warum das `Arc` ist: der Rahmen holte sich
    /// `Arc::new(f.code.clone())`, also eine **vollstaendige Kopie des
    /// Bytecodes bei jedem Aufruf**. Bei `fib(27)` (400.000 Aufrufe) und
    /// bei jeder Seite mit einer grossen Funktion war der Aufruf damit
    /// teurer als die Arbeit darin. `Arc::clone` ist nur ein Zaehler.
    pub code: std::sync::Arc<Vec<Op>>,
    /// Namenspool dieser Funktion (Operand = Index hinein).
    ///
    /// Release 09.10.2026: als `Arc` — der Motor leiht sich den Pool fuer
    /// einen Namen (`Engine::namen_pool`), statt ihn zu kopieren.
    pub namen: std::sync::Arc<Vec<String>>,
    /// J3b: der Hash **jedes** Namens daneben — beim Uebersetzen
    /// berechnet. Ohne das holte der Motor den Text pro Zugriff als Kopie
    /// (`fn name(i) -> String`: eine Allokation je `LeseVar`!) und
    /// hasht ihn noch einmal. GEMESSEN: 56 ns je Namenszugriff davor.
    pub namen_hash: Vec<u64>,
    /// J28: die Zeile, an der die Funktion **steht** (Zeile von
    /// `function f (…) {`).
    ///
    /// GEMESSEN, warum das noetig ist: der Rumpf beginnt mit Befehlen,
    /// die zu **keiner** Zeile gehoeren — die Parameter werden gebunden,
    /// die `var`-Namen angelegt. Der Rahmen startet mit `zeile = 0`, und
    /// ein Haken, der beim Aufruf in die Funktion hineinlaeuft (Schritt
    /// „rein"), zeigte deshalb `(0, 0)` (gemessen). Das ist eine Zeile, die
    /// es nicht gibt. Die Startzeile ist die Zeile der Deklaration — das
    /// ist, was ein Debugger zeigt, wenn er gerade den Aufruf betritt.
    pub zeile: u32,
    /// Handler-Tabelle (Operand = Index hinein).
    pub handler: Vec<Handler>,
    /// Anzahl der Temp-Plaetze.
    pub temps: u16,
    /// `var`-Namen (werden beim Anlegen des Rahmens deklariert).
    pub var_namen: std::sync::Arc<Vec<String>>,
    /// Release 09.10.2026: die Hashes von `var_namen`, beim Uebersetzen
    /// berechnet. Der Rahmenaufbau deklariert damit ohne Text: vorher kostete
    /// JEDER Aufruf je `var` einen String-Hash und eine String-Kopie in die
    /// Umgebung (`Bindungen` speichert den Namen) — gelesen wird er dort nie.
    pub var_hashes: std::sync::Arc<Vec<u64>>,
    /// Name fuer `KopiereNamen`/`KopiereZurueck` (Indizes in `namen`).
    ///
    /// GEMESSEN/Begruendung: die Schleifenkopf-Namen liegen NICHT
    /// hintereinander im Haupt-Namenspool. Ohne eigene Liste kopierte der
    /// Befehl die falschen Namen — dann blieb `i` unveraendert und die
    /// Schleife lief endlos (bzw. `() => i` sah immer die Endzahl).
    pub kopier_namen: Vec<u32>,
    /// Schluessellisten fuer den RUHE-TEIL eines Objekt-Musters
    /// (`const {a, ...rest} = o`): je Eintrag die Textindizes der
    /// Schluessel, die schon herausgenommen wurden. Operand von
    /// `Op::MusterRest` ist der Index hinein.
    ///
    /// GEMESSEN/Begruendung: `muster_rest` kannte die Liste, bekam sie aber
    /// nie (`&[]`) — `const {a, ...s} = {a:1,b:2}` lieferte `{a:1,b:2}`
    /// statt `{b:2}` (Orakel-Fall w039).
    pub muster_aus: Vec<Vec<u32>>,
    pub parameter: u16,
    /// J37: die **Namen** der Parameter (Namenspool-Indizes).
    ///
    /// GEMESSEN, warum sie gebraucht werden: ES2023 9.2.12
    /// (`FunctionDeclarationInstantiation`) — bei einer **nicht einfachen**
    /// Parameterliste ist ein direktes `eval`, das einen `var` mit einem
    /// Parameternamen deklariert, ein SyntaxError. `parameter` ist nur die
    /// **Anzahl**; fuer den Vergleich braucht es die Namen.
    pub param_namen: Vec<u32>,
    /// J37: war die Parameterliste **einfach** (ES2023 15.2.1
    /// `IsSimpleParameterList`)? Ohne Standardwerte, Ruhezeilen oder Muster.
    /// Nur bei **nicht einfacher** Liste gilt die `eval`-Regel.
    pub param_einfach: bool,
    /// J38: die lexikalischen Namen der **obersten** Listenebene
    /// (`let`/`const`/`class`) — fuer die zweite `eval`-Regel aus ES2023
    /// 9.2.12.
    ///
    /// GEMESSEN (node 22):
    ///   `function*(){ let x; eval('var x;') }` -> SyntaxError
    ///   `function*(){ { let x; } eval('var x;') }` -> **erlaubt**
    pub lex_namen: Vec<String>,
    pub rest: bool,
    pub pfeil: bool,
    pub asynchron: bool,
    pub generator: bool,
    pub name: String,
    /// `true`, wenn die Funktion `this` selbst bindet (nicht Pfeil).
    pub braucht_this: bool,
    /// J8: `true`, wenn die Funktion `arguments` sehen kann — im
    /// Namenspool steht `arguments` oder `eval` (ein direktes `eval` im
    /// Rumpf kann `arguments` sehen), oder ein Pfeil darin nennt es (J48).
    ///
    /// GEMESSEN, warum das noetig war: `Op::DefArgumente` baute bei
    /// **jedem** Aufruf das `arguments`-Objekt (Element-Feld mit 11
    /// Eigenschaften, je ein HashMap-Eintrag mit Text) — rund 15
    /// Allokationen je Aufruf. Bei `fib(27)` (392.835 Aufrufe) waren das
    /// ueber 5 Millionen Allokationen, und **ein Aufruf kostete ~1,4
    /// Mikrosekunden** (Gemessen: 9.852.127 Schritte in 737 ms, das sind
    /// 74 ns je Schritt; die reine Schleife kostet 17 ns).
    /// Wer `arguments` nicht im Quelltext nennt, kann es auch nicht
    /// bemerken — deshalb wird es dann gar nicht gebaut.
    pub braucht_arguments: bool,
    /// Name eines BENANNTEN Funktionsausdrucks (`(function f(){ f() })`):
    /// wird im eigenen Rahmen-Scope gebunden (JS-Regel).
    pub selbst_name: Option<String>,
    /// J14: die Funktion beginnt mit `"use strict"` (Direktive im
    /// Rumpf). GEMESSEN, warum das pro **Funktion** und nicht global
    /// gilt: `function f(){ 'use strict'; return this } f()` liefert in
    /// node `undefined`, im selben Skript daneben `f2()` (ohne
    /// Direktive) den globalen Bereich. Ein Motor-weites Schalter
    /// (`self.strikt`) kann beides nicht.
    pub strikt: bool,
    /// J19: der Quelltext der Funktion (wie er im Programm stand).
    /// `Function.prototype.toString` gibt ihn zurueck; eingebaute
    /// Funktionen tragen keinen und liefern `function name() { [native
    /// code] }` wie in node.
    ///
    /// GEMESSEN, warum das gebraucht wird: Bibliotheken erkennen
    /// Merkmale ueber den Quelltext — `f.toString().includes('=>')`,
    /// Babel/core-js pruefen `async`, Web-APIs `[native code]`. Vorher
    /// lieferte der Motor immer `function f() { [code] }`.
    pub quelle: Option<std::sync::Arc<str>>,
}

/// J8: die Felder einer Funktionsdefinition, die der **Aufrufpfad**
/// braucht.
///
/// GEMESSEN, warum es diese kleine Kopie gibt: `funktion_von` gab eine
/// **vollstaendige** `Funktion` zurueck (`.cloned()`), und `Funktion`
/// traegt acht Vec/String-Felder — `namen`, `namen_hash`, `handler`,
/// `kopier_namen`, `muster_aus`, `var_namen`, `name`, `selbst_name`. Der
/// Aufrufpfad klonte sie **zweimal je Aufruf** (in `aufruf_einfuegen` und
/// nochmal in `rahmen_starten_fn`). Bei `fib(27)` (392.835 Aufrufe) waren
/// das rund 6,3 Millionen Allokationen, die im Profil **ein Aufruf ~1
/// Mikrosekunde** kostete. Diese Kopie nimmt nur die noetigen Felder —
/// `namen`/`namen_hash` bleiben unberuehrt (der Namenszugriff laeuft ueber
/// den Hash, siehe `Engine::suche_h`).
#[derive(Debug, Clone)]
pub struct AufrufKopf {
    pub code: std::sync::Arc<Vec<Op>>,
    pub temps: u16,
    pub braucht_this: bool,
    pub pfeil: bool,
    pub asynchron: bool,
    pub generator: bool,
    /// `var`-Namen (leer bei den meisten Funktionen — der Klon kostet
    /// dann nichts).
    pub var_namen: std::sync::Arc<Vec<String>>,
    /// Release 09.10.2026: ihre Hashes (siehe `Funktion::var_hashes`).
    pub var_hashes: std::sync::Arc<Vec<u64>>,
    /// Release 09.10.2026: Zahl der Parameter (Platz in der Umgebung).
    pub parameter: u16,
    /// Name eines benannten Funktionsausdrucks.
    pub selbst_name: Option<String>,
    /// J48: war die Parameterliste **einfach**? Nur bei einer **nicht
    /// einfachen** Liste bekommt der Aufruf eine **zweite** Umgebung fuer
    /// die Parameter (ES2023 10.2.11 Schritt 27) — siehe
    /// `Engine::rahmen_starten_fn`.
    pub param_einfach: bool,
    /// J14: die Funktion hat eine `"use strict"`-Direktive im Rumpf.
    pub strikt: bool,
    /// J19: der Quelltext der Funktion (wie er im Programm stand).
    /// `Function.prototype.toString` gibt ihn zurueck; eingebaute
    /// Funktionen tragen keinen und liefern `function name() { [native
    /// code] }` wie in node.
    ///
    /// GEMESSEN, warum das gebraucht wird: Bibliotheken erkennen
    /// Merkmale ueber den Quelltext — `f.toString().includes('=>')`,
    /// Babel/core-js pruefen `async`, Web-APIs `[native code]`. Vorher
    /// lieferte der Motor immer `function f() { [code] }`.
    pub quelle: Option<std::sync::Arc<str>>,
}

/// Ein Klassenelement in Bytecode-Form.
#[derive(Debug, Clone)]
pub enum KlassenElementCode {
    Methode {
        /// Textindex des Namens (bei `berechnet`: der **Variablenname**
        /// des Schluessels, J12).
        name: u32,
        fun: u32,
        statisch: bool,
        art: MethodenArt,
        /// `true` = `['n']`: der Name steht als Variable im umgebenden
        /// Scope und wird **einmal** beim Anlegen der Klasse gelesen
        /// (so sagt es die Spec; GEMESSEN mit node 22:
        /// `for (let i=0;i<2;i++) { class B { ['f'+i]=i } }` liefert in
        /// node zwei Objekte mit **denselben** Schluesseln `f0`).
        berechnet: bool,
    },
    /// Feld: Textindex des Namens, statisch oder nicht.
    Feld {
        name: u32,
        statisch: bool,
        /// `true` = `['n'] = wert` (siehe `Methode`).
        berechnet: bool,
    },
}

/// Eine kompilierte Klasse.
#[derive(Debug, Clone)]
pub struct KlassenDef {
    pub name: Option<String>,
    /// Kompilierter Konstruktor (mit Feld-Initialisierung).
    pub ctor: Option<u32>,
    pub elemente: Vec<KlassenElementCode>,
    /// Initialisierer der statischen Felder (`this` = Konstruktor).
    pub felder_statisch: u32,
}

/// J8: **Inline-Cache** fuer den Eigenschaftszugriff.
///
/// Der Eintrag merkt sich fuer eine Zugriffsstelle (den Textindex des
/// Namens) nur: „bei **diesem** Objekt liegt der Name als **eigene
/// Daten-Eigenschaft** vor". Den **Wert** merkt er ausdruecklich nicht —
/// er wird bei jedem Treffer neu aus der Property-Map gelesen. Damit kann
/// der Cache nicht veralten: ein Schreibzugriff, ein `delete`, ein Getter
/// oder ein `defineProperty` aendern die Property-Map, und der naechste
/// Treffer liest den neuen Stand.
///
/// GEMESSEN (vorher, ohne Cache): `s = s + o.a` in einer Schleife mit 10^6
/// Runden kostete 477 ns je Runde bei 10 Schritten; die reine Schleife
/// (9 Schritte) 160 ns. Der eine Extra-Schritt kostete ~317 ns, weil
/// `LeseEigenschaft` (a) den Namen als **Kopie** holte
/// (`text_konstante -> String`, eine Allokation je Zugriff), (b) den
/// Host-Haken pruefte und (c) die Kette neu ablief.
///
/// Passt der Eintrag nicht (anderes Objekt, kein eigener Treffer), laeuft
/// der normale Weg und der Eintrag wird danach neu geschrieben. Polymorphe
/// Stellen (Wechsel im Schleifenrumpf) zahlen dann den Cache-Schreibvorgang
/// ohne zu gewinnen — richtig, nur nicht schneller.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct IcEintrag {
    /// Das Objekt, bei dem zuletzt eine **eigene** Daten-Eigenschaft
    /// gefunden wurde.
    pub objekt: Option<crate::heap::HeapRef>,
    /// `true`, wenn `objekt` gesetzt ist.
    pub eigen: bool,
}

/// Ein kompiliertes Programm.
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub funktionen: Vec<Funktion>,
    /// Release 09.10.2026: als `Arc` — der Motor teilt den Pool mit
    /// `Engine::texte_arc`, statt ihn je Skript zu kopieren.
    pub texte: std::sync::Arc<Vec<String>>,
    pub klassen: Vec<KlassenDef>,
    /// Index der Hauptfunktion.
    pub haupt: usize,
    pub strikt: bool,
}

impl Bytecode {
    /// Groesse in Bytes (fuer die Heap-Buchhaltung, M4).
    pub fn bytes(&self) -> usize {
        let mut n = 64;
        for f in &self.funktionen {
            n += f.code.len() * std::mem::size_of::<Op>();
            n += f.namen.iter().map(|s| s.len() + 16).sum::<usize>();
            n += f.var_namen.len() * 24;
        }
        n += self.texte.iter().map(|s| s.len() + 16).sum::<usize>();
        n += self.klassen.len() * 96;
        n
    }
}
