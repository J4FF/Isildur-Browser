// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Compiler: AST -> Bytecode (WELLE6, V1).
//!
//! Der Compiler kennt die Klammerstruktur, also loest er `break`/`continue`
//! **statisch** auf: die VM kennt zur Laufzeit keine Marken. Jeder
//! Kontrollfluss ist ein Befehl mit konstantem Sprungziel.
//!
//! # STAPELDISZIPLIN
//!
//! `Pick`/`Rotiere` haben konstante Offsets, weil der Compiler die
//! Zielreihenfolge kennt. Umgebungen (Bloecke, Schleifenkopfs-Bindungen,
//! `catch`) liegen in **Temps** des Rahmens, nicht auf dem Operandenstapel —
//! so kann ein `break` sie nicht verwaessen.
//!
//! # WARUM DAS EIGENTLICH NOETIG IST
//!
//! GEMESSEN in `narsil-js`: `await` kann dort nicht anhalten, weil die
//! Fortsetzung der Rust-Stapel ist. Hier ist die Fortsetzung ein `pc` in
//! `Vec<Op>` — `Erwartung` merkt sich den `pc` im Rahmen, und die
//! Mikrotask-Schlange setzt genau dort fort.

/// BEGRUENDUNG fuer `dead_code` (gemessen am 30.09.2026): Compiler-Oberflaeche. Einige Felder/Helfer sind fuer die naechsten
/// Schritte vorbereitet (statische Felder, `new.target`) und werden heute
/// nicht erzeugt.
#[allow(dead_code)]
use std::collections::HashMap;

use crate::ast::*;
use crate::fehler::JsFehler;
use crate::op::{
    Bytecode, Funktion, Handler, KEIN_NAME, KlassenDef, KlassenElementCode, MAX_BEFEHLE,
    MAX_KLASSEN, MAX_KONSTANTEN, MAX_TEMPS, Op,
};

/// Sprungziel einer `break`/`continue` (Schleife, `switch`, Marke, Block).
struct Ziel {
    /// Marken, die DIREKT auf diesem Ziel stehen (Nameindizes).
    marken: Vec<u32>,
    /// Offene Patchstellen fuer `break`.
    ende: Vec<usize>,
    /// Offene Patchstellen fuer `continue` (`None` bei `switch`/Block).
    weiter: Option<Vec<usize>>,
    /// Anzahl offener Block-Umgebungen beim Anlegen (fuer sauberes `break`).
    umg_tiefe: usize,
    /// `true` bei Schleifen (dort ist `continue` erlaubt).
    schleife: bool,
    /// Release 14.10.2026: the iterator temp of a `for…of` loop (closed by
    /// `break`/`continue`/`return` that leave it, see `Op::IterSchliessen`).
    iter_temp: Option<u16>,
    /// Release 15.10.2026: the iterator temp of a `for await` loop (closed
    /// with `await it.return()`, see `asyncgen::HILFE_ASYNC_SCHLIESSEN`).
    async_iter_temp: Option<u16>,
}

/// Zustand eines `try`-Blocks waehrend des Kompilierens.
struct TryCtx {
    /// Offene Stellen, die auf den Anfang des `finally`-Blocks zeigen.
    f_stelle: Vec<usize>,
    /// `true`, wenn es `finally` gibt.
    hat_finally: bool,
}

/// Compiler-Zustand fuer EINE Funktion.
struct Fn {
    /// J28: die Zeile der Funktionsdeklaration (0 = unbekannt).
    zeile: u32,
    code: Vec<Op>,
    namen: Vec<String>,
    namen_index: HashMap<String, u32>,
    handler: Vec<Handler>,
    temps: u16,
    var_namen: Vec<String>,
    /// Release 09.10.2026: dieselben Namen als Menge (`contains` auf der
    /// Liste war quadratisch: 70 000 `var` kosteten 14 s Uebersetzung).
    var_namen_menge: std::collections::HashSet<String>,
    kopier_namen: Vec<u32>,
    /// Schluessellisten fuer `Op::MusterRest` (siehe `op::Funktion`).
    muster_aus: Vec<Vec<u32>>,
    ziele: Vec<Ziel>,
    try_stapel: Vec<TryCtx>,
    /// Offene Block-Umgebungen (Temps) — fuer `break`/`continue`.
    block_temps: Vec<u16>,
    parameter: u16,
    rest: bool,
    /// J37: die Parameternamen als Text und die Frage „einfache Liste".
    /// Der Bauer verwirft die **Muster**, also muessen die Namen hier
    /// gesichert werden, solange `FunktionsBau.params` noch lebt.
    param_namen_text: Vec<String>,
    param_einfach: bool,
    /// J48: wird gerade der **Parameterprolog** gebaut? Dann legt
    /// `muster_binden` seine Bindungen in der Parameterumgebung an
    /// (`Op::DefParam` statt `Op::DefVar`).
    param_prolog: bool,
    /// J38: die **lexikalischen** Namen der obersten Listenebene
    /// (`let`/`const`/`class`) — fuer die zweite `eval`-Regel aus 9.2.12.
    ///
    /// GEMESSEN (node 22), warum **nur die oberste Ebene**:
    ///   `function*(){ let x; eval('var x;') }`     -> SyntaxError
    ///   `function*(){ { let x; } eval('var x;') }` -> **erlaubt**
    /// `LexicallyDeclaredNames` gilt fuer **eine** StatementList; ein
    /// innerer Block hat seine eigene.
    lex_namen: Vec<String>,
    pfeil: bool,
    asynchron: bool,
    generator: bool,
    name: String,
    braucht_this: bool,
}

impl Fn {
    /// Release 09.10.2026: einen `var`-Namen einmal aufnehmen (O(1)).
    fn var_dazu(&mut self, n: &str) {
        if self.var_namen_menge.insert(n.to_string()) {
            self.var_namen.push(n.to_string());
        }
    }

    fn neu(name: &str) -> Self {
        Fn {
            zeile: 0,
            code: Vec::new(),
            namen: Vec::new(),
            namen_index: HashMap::new(),
            handler: Vec::new(),
            temps: 0,
            var_namen: Vec::new(),
            var_namen_menge: std::collections::HashSet::new(),
            kopier_namen: Vec::new(),
            muster_aus: Vec::new(),
            ziele: Vec::new(),
            try_stapel: Vec::new(),
            block_temps: Vec::new(),
            parameter: 0,
            param_namen_text: Vec::new(),
            param_einfach: true,
            // J48: siehe das Feld `param_prolog`.
            param_prolog: false,
            lex_namen: Vec::new(),
            rest: false,
            pfeil: false,
            asynchron: false,
            generator: false,
            name: name.to_string(),
            braucht_this: true,
        }
    }

    /// Name -> Index im Namenspool (M4: Pool ist durch `u16` begrenzt).
    fn name_index(&mut self, n: &str) -> u32 {
        if let Some(i) = self.namen_index.get(n) {
            return *i;
        }
        // Release 09.10.2026: `u32` — vorher endete die Liste bei 65 534, und
        // jeder weitere Name bekam DENSELBEN Index (der Hauptrumpf von
        // youtube.com hat mehr Namen: `console.log` las dann ein fremdes
        // `console`).
        let i = self.namen.len() as u32;
        self.namen.push(n.to_string());
        self.namen_index.insert(n.to_string(), i);
        i
    }

    /// Eine Liste von Nameindizes fuer `KopiereNamen` anlegen.
    fn kopier_liste(&mut self, namen: &[u32]) -> (u32, u16) {
        let start = self.kopier_namen.len() as u32;
        for n in namen {
            self.kopier_namen.push(*n);
        }
        (start, namen.len() as u16)
    }

    fn temp(&mut self) -> Result<u16, JsFehler> {
        if self.temps >= MAX_TEMPS {
            return Err(JsFehler::Speicher(
                "zu viele Temp-Plaetze in einer Funktion (M4)".to_string(),
            ));
        }
        let t = self.temps;
        self.temps += 1;
        Ok(t)
    }

    fn emittiere(&mut self, o: Op) -> usize {
        self.code.push(o);
        self.code.len() - 1
    }

    fn sprung(&mut self, o: Op) -> usize {
        self.code.push(o);
        self.code.len() - 1
    }

    fn hier(&self) -> u32 {
        self.code.len() as u32
    }

    /// Ziel eines Sprungbefehls setzen (alle Sprungarten).
    fn patch(&mut self, pos: usize, ziel: u32) {
        let z = ziel;
        match &mut self.code[pos] {
            Op::Spring(x)
            | Op::SpringWennFalsch(x)
            | Op::SpringWennFalschBehalten(x)
            | Op::SpringWennWahr(x)
            | Op::SpringWennUndef(x)
            | Op::SpringWennNichtNullish(x)
            | Op::FertigVormerken(x) => *x = z,
            Op::IterNaechster(_, x) => *x = z,
            Op::LeseZielTemp(_, x) => *x = z as u8,
            // J3b: der Superbefehl traegt sein Sprungziel an der dritten
            // Stelle. GEMESSEN/GEFUNDEN: ohne diesen Arm blieb das Ziel 0
            // und die Schleife sprang am Ende **an den Anfang des
            // Programms** — `for (let i = 0; i < 2; i++)` endete nie
            // (4 Milliarden Schritte, Schrittgrenze).
            Op::TestWertKleinerZahlSpring(_, _, x) => *x = z,
            _ => {}
        }
    }
}

/// AST-Programm aus `parse` (Alias, damit `parse` nicht oeffentlich sein muss).
pub(crate) type AstProgramm = crate::ast::Programm;

/// Der Compiler.
pub(crate) struct Compiler {
    /// J28: die Zeile der gerade uebersetzten Funktion.
    fn_zeile: u32,
    texte: Vec<String>,
    texte_index: HashMap<String, u32>,
    funktionen: Vec<crate::op::Funktion>,
    klassen: Vec<KlassenDef>,
    /// Marke der gerade uebersetzten SCHLEIFE (`aussen: for (…)`).
    ///
    /// GEMESSEN: die Marke muss im Ziel DER SCHLEIFE stehen. Vorher landete
    /// sie im Ziel des umgebenden Konstrukts — `continue aussen` fand dann
    /// kein Ziel, die VM beendete den ganzen Aufruf und das Programm endete
    /// still nach vier Zeilen (p2.js).
    offene_marke: Option<String>,
    /// Release 10.10.2026: die offenen OPTIONALEN KETTEN (`a?.b.c()`): je
    /// Kette die Kurzschluss-Spruenge, die an ihr Ende zeigen.
    ketten: Vec<Vec<usize>>,
    /// Release 10.10.2026: der Feld-Initialisierer des ABGELEITETEN
    /// Konstruktors, der gerade uebersetzt wird — er laeuft direkt nach jedem
    /// `super(...)` (ES2023 13.3.7.1 Schritt 11: InitializeInstanceElements).
    felder_nach_super: Option<u32>,
    /// Release 10.10.2026: in diesem Konstruktor stand ein direktes
    /// `super(...)` (dann entfaellt der Rueckfall am Rumpfende).
    super_felder_gesetzt: bool,
    /// J12: Zaehler fuer die Variablen der **berechneten** Klassennamen
    /// (`['n']`). Der Name muss je Element verschieden sein, weil alle in
    /// derselben Funktion liegen (dem Scope, der die Klasse baut).
    klassenzaehler: usize,
    /// J48: die Indizes der **gerade uebersetzten** Funktionen (Stack).
    ///
    /// Wozu: eine Pfeilfunktion hat **kein** eigenes `arguments` — der Name
    /// gehoert der umgebenden Funktion. Die Optimierung aus J8 („`arguments`
    /// nur bauen, wenn der Name im Namenspool steht") muss das wissen, sonst
    /// fehlt die Bindung.
    ///
    /// GEMESSEN (node 22): `var a5; var f5 = (x = (a5 = arguments)) => {};
    /// f5(); typeof a5` -> `object`; im Motor `ReferenceError`.
    fn_stapel: Vec<usize>,
    /// Release 09.10.2026: ist die gerade uebersetzte Funktion strikt?
    /// (Funktionsdeklarationen in Bloecken: locker `var`, strikt blockweit.)
    strikt_stapel: Vec<bool>,
    /// Release 09.10.2026: wie tief stecken wir in `switch`-Faellen?
    auswahl_tiefe: u32,
    /// Release 12.10.2026: lockere Block-Funktionen (Annex B.3.3), deren
    /// Anweisung den Wert ins `var` der Funktion kopiert (Adresse des
    /// AST-Knotens; beim Uebersetzen der Anweisung wieder entfernt).
    annex_b: std::collections::HashSet<usize>,
    /// J48: Marken „diese Funktion braucht ein `arguments`-Objekt", **nach**
    /// dem Ueberschreiben des Platzhalters.
    ///
    /// GEMESSEN/GEFUNDEN: die erste Fassung setzte das Merkmal direkt an
    /// `self.funktionen[i]` — und das ging verloren, weil `funktion_kompilieren_mit`
    /// am Ende `self.funktionen[idx] = f` macht und damit die Marke
    /// ueberschreibt. Der Pfeil stand in einer Funktion, die danach noch
    /// fertig uebersetzt wurde.
    fn_arguments: std::collections::HashSet<usize>,
    /// Release 09.10.2026: Basis des laufenden Programms. Der Compiler haelt
    /// nur die **neuen** Texte, Funktionen und Klassen; ihre Indizes beginnen
    /// hinter denen des laufenden Programms (siehe `basis_setzen`).
    basis_texte: u32,
    basis_funktionen: usize,
    basis_klassen: usize,
    /// J20: Stack der **Wert-Slots** fuer den Vervollstaendigungswert.
    ///
    /// GEMESSEN, warum ueberhaupt ein Slot noetig ist: die
    /// Ausdrucksanweisung hat ihren Wert mit `Op::Pop(1)` weggeworfen —
    /// darum lieferte `try { 5 } catch (e) { 9 }` `undefined` statt `5`
    /// (18 von 30 gemessenen Faellen).
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum ein **Temp** und nicht der
    /// Operandenstapel: `TempSet` nimmt den Wert vom Stapel und legt ihn
    /// im Platz ab — der Stapel bleibt also genauso ausgeglichen wie
    /// vorher mit `Pop(1)`. Ein Fehler hier kann den Stapel therefore
    /// nicht verheben, und `pruefe()` (die Stapelbilanz) bleibt
    /// aussagekraeftig.
    wert_slots: Vec<u16>,
    /// J20: der Slot mit dem Vervollstaendigungswert der **obersten**
    /// Anweisungsliste — der Wert, den `ausfuehren` zurueckgibt.
    wert_letzter: Option<u16>,
}

impl Compiler {
    pub(crate) fn neu() -> Self {
        Compiler {
            fn_zeile: 0,
            texte: Vec::new(),
            texte_index: HashMap::new(),
            funktionen: Vec::new(),
            klassen: Vec::new(),
            offene_marke: None,
            ketten: Vec::new(),
            felder_nach_super: None,
            super_felder_gesetzt: false,
            klassenzaehler: 0,
            // J48: siehe die beiden Felder.
            fn_stapel: Vec::new(),
            strikt_stapel: Vec::new(),
            auswahl_tiefe: 0,
            annex_b: std::collections::HashSet::new(),
            fn_arguments: std::collections::HashSet::new(),
            basis_texte: 0,
            basis_funktionen: 0,
            basis_klassen: 0,
            // J20: der Vervollstaendigungswert (siehe `wert_slots`).
            wert_slots: Vec::new(),
            wert_letzter: None,
        }
    }

    /// Nameindizes fuer das Ziel der gerade uebersetzten Schleife.
    fn marken_liste(&mut self, f: &mut Fn) -> Vec<u32> {
        match self.offene_marke.take() {
            Some(n) => vec![f.name_index(&n)],
            None => Vec::new(),
        }
    }

    fn text(&mut self, s: &str) -> Result<u32, JsFehler> {
        if let Some(i) = self.texte_index.get(s) {
            return Ok(*i);
        }
        if self.basis_texte as usize + self.texte.len() >= MAX_KONSTANTEN {
            return Err(JsFehler::Speicher("zu viele Textkonstanten".to_string()));
        }
        let i = self.basis_texte + self.texte.len() as u32;
        self.texte.push(s.to_string());
        self.texte_index.insert(s.to_string(), i);
        Ok(i)
    }

    /// OPUS_1: `eval` **im laufenden Programm**. Die Funktionen, Texte und
    /// Klassen des laufenden Programms werden vorne uebernommen, damit alle
    /// Indizes (Text, Funktion, Klasse) im Ergebnis dieselben bleiben; der
    /// neue Hauptlauf liegt am Ende (`Bytecode::haupt`).
    ///
    /// GEMESSEN vorher: `eval` rief `starten` und ersetzte das laufende
    /// Programm samt Rahmen und Stapel — jeder Lauf mit `eval` endete in
    /// „Hauptlauf hat angehalten (intern)" (25 test262-Faelle).
    /// Release 09.10.2026: an ein laufendes Programm **anhaengen**, ohne es
    /// zu kopieren. Der Compiler bekommt nur die Laengen der Pools und den
    /// Textindex des Motors (geliehen, siehe `Engine::uebersetzen`); er
    /// liefert ausschliesslich die neuen Eintraege, deren Indizes hinter den
    /// alten beginnen.
    ///
    /// GEMESSEN, warum: die vorige Fassung (`uebersetze_anhang`) klonte bei
    /// **jedem** weiteren Skript, `eval` und `new Function` alle Texte, alle
    /// Funktionen (samt Namenspools) und alle Klassen des laufenden
    /// Programms und baute den Textindex neu. Nach kevlar_base (youtube.com,
    /// zehntausende Funktionen) kostete so jedes kleine Skript danach
    /// dasselbe wie das grosse.
    pub(crate) fn basis_setzen(
        &mut self,
        texte: usize,
        funktionen: usize,
        klassen: usize,
        texte_index: HashMap<String, u32>,
    ) {
        self.basis_texte = texte as u32;
        self.basis_funktionen = funktionen;
        self.basis_klassen = klassen;
        self.texte_index = texte_index;
        // Erzeugte Hilfsnamen (`__jsvm_key_…`) duerfen nicht mit denen des
        // laufenden Programms zusammenfallen.
        self.klassenzaehler = 1_000_000 + funktionen * 64;
    }

    /// Den (geliehenen) Textindex zurueckgeben. Bei `gueltig == false` (die
    /// Uebersetzung schlug fehl) werden die neuen Texte wieder entfernt —
    /// sie kommen nie ins Programm.
    pub(crate) fn texte_index_zurueck(&mut self, gueltig: bool) -> HashMap<String, u32> {
        let mut idx = std::mem::take(&mut self.texte_index);
        if !gueltig {
            for t in &self.texte {
                if idx.get(t).is_some_and(|i| *i >= self.basis_texte) {
                    idx.remove(t);
                }
            }
        }
        idx
    }

    /// Ein Programm uebersetzen.
    pub(crate) fn uebersetze(&mut self, p: &AstProgramm) -> Result<Bytecode, JsFehler> {
        // Index 0 ist reserviert ("leer") — ein Null-Funktionsindex darf nie
        // vorkommen, muss aber darstellbar sein (z. B. `Klasse`).
        // OPUS_1: im Anhang-Modus liegt der Hauptlauf hinter den
        // uebernommenen Funktionen.
        let haupt = self.basis_funktionen + self.funktionen.len();
        self.funktionen.push(leere_funktion("<leer>"));
        // J48: der Hauptlauf (Index 0) ist die umgebende Funktion eines
        // Pfeils auf oberster Ebene — sonst faende die Meldung niemanden.
        self.fn_stapel.push(haupt);
        self.strikt_stapel.push(p.strikt);
        let mut f = Fn::neu("<main>");

        // J48: auch der Hauptlauf hat ein `arguments` (ES2023 10.2.11
        // Schritt 29 gilt fuer jede Funktion; node gibt `typeof arguments`
        // auf oberster Ebene als `object`).
        //
        // GEMESSEN (node 22), der Anlass:
        //   `var a6; var f6 = (x = (a6 = arguments)) => { }; f6(); typeof a6`
        //   -> `object`; ohne diesen Befehl `ReferenceError`
        //
        // Der Befehl baut das Objekt nur, wenn es gebraucht wird (J8).
        f.emittiere(Op::DefArgumente);
        self.hissen_in(&mut f, &p.anweisungen)?;
        // GEMESSEN: `ausfuehren` gibt den VERVOLLSTAENDIGUNGSWERT des
        // Programms zurueck. Deshalb bleibt der Wert der LETZTEN
        // Ausdrucksanweisung liegen und ist das Ergebnis (wie in node bei
        // `node -e` und in narsil-js mit `Ablauf::Wert`).
        let n = p.anweisungen.len();
        let mut schwanz = false;
        for (i, a) in p.anweisungen.iter().enumerate() {
            // J28: `MitZeile` ist durchsichtig — der Knoten traegt nur
            // die Quelltextzeile. GEMESSEN, warum das hier zwingend ist:
            // ohne `.inhalt()` traf der Rueckgabe-Schwanz nicht mehr zu,
            // und **jedes** Skript lieferte `undefined` zurueck (auch
            // `var a=1; var b=a+2; b` — gemessen mit examples/diag.rs).
            if i + 1 == n
                && let Anweisung::Ausdruck(e) = a.inhalt()
            {
                self.ausdruck(&mut f, e)?;
                f.emittiere(Op::Rueckgabe);
                schwanz = true;
                break;
            }
            self.anweisung(&mut f, a)?;
        }
        if !schwanz {
            // J20: ist die LETZTE Anweisung eine wertliefernde Form (Block,
            // `if`, Schleife, `try`, `switch`), steht ihr Wert im Slot
            // `wert_letzter`. Sonst ist das Ergebnis `undefined`
            // (`{ let a = 1 }` -> `undefined`, node gleich).
            match self.wert_letzter {
                Some(t) => {
                    f.emittiere(Op::TempGet(t));
                }
                None => {
                    f.emittiere(Op::Undef);
                }
            }
            f.emittiere(Op::Rueckgabe);
        }
        self.pruefe(&f)?;
        let mut mf = fn_von(f, "<main>", false, false, false, false);
        // J48: siehe `fn_arguments`.
        if self.fn_arguments.contains(&haupt) {
            mf.braucht_arguments = true;
        }
        self.fn_stapel.pop();
        self.strikt_stapel.pop();
        self.funktionen[haupt - self.basis_funktionen] = mf;
        Ok(Bytecode {
            funktionen: std::mem::take(&mut self.funktionen),
            texte: std::sync::Arc::new(std::mem::take(&mut self.texte)),
            klassen: std::mem::take(&mut self.klassen),
            haupt,
            strikt: p.strikt,
        })
    }

    fn pruefe(&self, f: &Fn) -> Result<(), JsFehler> {
        if f.code.len() > MAX_BEFEHLE {
            return Err(JsFehler::Speicher("Funktion zu gross (M4)".to_string()));
        }
        Ok(())
    }

    // =================================================================
    // Hissen
    // =================================================================

    /// `var`-Namen und Funktionsdeklarationen einer Anweisungsliste.
    ///
    /// J38: sammelt **zusaetzlich** die lexikalischen Namen der **obersten**
    /// Ebene (`let`/`const`/`class`). Sie werden fuer die zweite `eval`-
    /// Regel aus ES2023 9.2.12 gebraucht („It is a Syntax Error if any
    /// element of the VarDeclaredNames of evalScript also occurs in the
    /// LexicallyDeclaredNames of FunctionBody").
    ///
    /// GEMESSEN (node 22), **fuenf** Proben:
    ///   `let x; eval('var x;')`      -> SyntaxError
    ///   `const x = 1; eval('var x')` -> SyntaxError
    ///   `class x {}; eval('var x')`  -> SyntaxError
    ///   `let x; eval('var y;')`      -> erlaubt
    ///   `{ let x; } eval('var x;')`  -> **erlaubt** (innerer Block!)
    fn hissen_in(&mut self, f: &mut Fn, liste: &[Anweisung]) -> Result<(), JsFehler> {
        // J38: nur die **oberste** Liste — ein `Block` ist ein eigener
        // Geltungsbereich, und die Regel gilt fuer *diese* Liste.
        for roh in liste {
            let a = roh.inhalt();
            let namen: Vec<String> = match a {
                Anweisung::Deklaration { art, eintraege } if *art != DeklArt::Var => {
                    let mut v = Vec::new();
                    for (m, _) in eintraege {
                        for n in muster_namen(m) {
                            if !v.contains(&n) {
                                v.push(n);
                            }
                        }
                    }
                    v
                }
                Anweisung::Klasse { name: Some(n), .. } => vec![n.clone()],
                _ => Vec::new(),
            };
            for n in namen {
                if !f.lex_namen.contains(&n) {
                    f.lex_namen.push(n);
                }
            }
        }
        for roh in liste {
            // J28: durch `MitZeile` sehen (sonst wird keine Funktion
            // gehisst — gemessen als ReferenceError).
            let a = roh.inhalt();
            if let Anweisung::Funktion {
                name: Some(n),
                funktion,
            } = a
            {
                let idx = self.funktion_kompilieren(funktion, n, None, false)?;
                let ni = f.name_index(n);
                f.emittiere(Op::NeuFunktion(idx));
                f.emittiere(Op::DefVar(ni, 0));
            }
        }
        // Release 09.10.2026: die Funktionsdeklarationen DIESER Liste sind
        // `var`-Bindungen DIESER Funktion (ES2023 10.2.11 Schritt 36) — auch
        // strikt. GEMESSEN vorher: ohne Vordeklaration suchte `DefVar` die
        // Kette hinauf und schrieb in eine AEUSSERE Bindung gleichen Namens:
        // `function o(){'use strict'; var Va=1; function i(){ function Va(){} }
        // i(); return Va }` gab die innere Funktion zurueck. youtube.com
        // (XRegExp, strikt): jedes Modul ueberschrieb das `Va` des anderen.
        for roh in liste {
            if let Anweisung::Funktion { name: Some(n), .. } = roh.inhalt() {
                f.var_dazu(n);
            }
        }
        // `var`-Namen sammeln (Stapel, keine Rekursion — M3/M5).
        let strikt = self.strikt_jetzt();
        let mut stapel: Vec<&Anweisung> = liste.iter().collect();
        let mut besuche = 0usize;
        // 05.10.2026: die alte Grenze `liste.len()*4 + 1024` war fuer
        // minifizierte Buendel zu klein. GEMESSEN an youtubes ES5-Bundle
        // (10,8 MB): `Hissen zu tief`, obwohl der Baum in Ordnung war —
        // die Zahl der KNOTEN ausserhalb von Funktionskoerpern waechst
        // schneller als 4 je Top-Level-Anweisung. Der Durchlauf ist ein
        // STAPEL (keine Rekursion), jeder Besuch ist ein Zeiger-Pop; die
        // Obergrenze ist nur ein Schutz gegen einen korrupten Baum. 20 Mio.
        // Besuche sind in Millisekunden erledigt und fuer jedes echte
        // Programm ausreichend.
        let max = liste.len().saturating_mul(4).saturating_add(20_000_000);
        while let Some(a) = stapel.pop() {
            besuche += 1;
            if besuche > max {
                return Err(JsFehler::Laufzeit("Hissen zu tief".to_string()));
            }
            // J28: dasselbe fuer die `var`-Namen (der Stapel enthaelt
            // die Original-Anweisungen, auch aus Bloecken).
            let a: &Anweisung = a.inhalt();
            match a {
                Anweisung::Deklaration {
                    art: DeklArt::Var,
                    eintraege,
                } => {
                    for (m, _) in eintraege {
                        let namen = muster_namen(m);
                        for n in namen {
                            f.var_dazu(&n);
                        }
                    }
                }
                Anweisung::Block(v) => stapel.extend(v.iter()),
                // Release 09.10.2026 (Annex B.3.3): eine Funktion in einem
                // Block ist im lockeren Modus auch ein `var` der Funktion.
                // (Die oberste Liste deklariert der Prolog ohnehin.)
                Anweisung::Funktion { name: Some(n), .. } if !strikt => {
                    f.var_dazu(n);
                }
                Anweisung::Wenn { dann, sonst, .. } => {
                    stapel.push(dann);
                    if let Some(s) = sonst {
                        stapel.push(s);
                    }
                }
                // Release 09.10.2026: auch das `var` im KOPF einer Schleife
                // (ES2023 14.7.4.2/14.7.5.6 `VarDeclaredNames` von
                // `for (var …;…;…)` und `for (var … in/of …)`).
                //
                // GEMESSEN (node 22 als Soll), der Anlass youtube.com:
                //   `var x = "g"; function u(){ for (var x = 0;;) break;
                //    return x } u() + typeof x` -> node `0string`,
                //   Motor `0number`
                // Ohne Vordeklaration fand `DefVar` (J34: „deklarierende
                // Umgebung suchen") die AEUSSERE Bindung und schrieb hinein.
                // In kevlar ueberschrieb so XRegExps `for (var Cb, he = 0; …)`
                // die Funktion `he` des Moduls: `TypeError: he is not a
                // function`.
                Anweisung::Fuer { kopf, rumpf, .. } => {
                    if let FuerKopf::Deklaration {
                        art: DeklArt::Var,
                        eintraege,
                    } = kopf
                    {
                        for (m, _) in eintraege {
                            for n in muster_namen(m) {
                                f.var_dazu(&n);
                            }
                        }
                    }
                    stapel.push(rumpf);
                }
                Anweisung::FuerIn { art, ziel, rumpf, .. } => {
                    if *art == DeklArtOpt::Var {
                        for n in muster_namen(ziel) {
                            f.var_dazu(&n);
                        }
                    }
                    stapel.push(rumpf);
                }
                Anweisung::Solange { rumpf, .. } => stapel.push(rumpf),
                Anweisung::Versuch {
                    rumpf,
                    fang,
                    endlich,
                    ..
                } => stapel.extend(rumpf.iter().chain(fang.iter()).chain(endlich.iter())),
                Anweisung::Marke(_, i) => stapel.push(i),
                Anweisung::Mit(_, i) => stapel.push(i),
                // Release 09.10.2026: auch `var` und Funktionen im CaseBlock.
                Anweisung::Auswahl { faelle, .. } => {
                    for (_, v) in faelle {
                        stapel.extend(v.iter());
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Eine `FunktionsBau` kompilieren; gibt den Funktionsindex.
    ///
    /// `felder`: `(Funktionsindex, vor_dem_rumpf)` — der Initialisierer der
    /// Instanzfelder der Klasse, den der Konstruktor aufruft (Basis vor dem
    /// Rumpf, Ableitung danach — Naeherung wie in `narsil-js`).
    fn funktion_kompilieren(
        &mut self,
        bau: &FunktionsBau,
        name: &str,
        felder: Option<(u32, bool)>,
        konstruktor: bool,
    ) -> Result<u32, JsFehler> {
        // J28: die Zeile der Deklaration in den Compiler-Zustand
        // (der Parser hat sie in `FunktionsBau::zeile` gesetzt).
        self.fn_zeile = bau.zeile;
        self.funktion_kompilieren_mit(bau, name, felder, konstruktor, None)
    }

    /// Wie `funktion_kompilieren`, mit Name fuer benannte Ausdruecke.
    fn funktion_kompilieren_mit(
        &mut self,
        bau: &FunktionsBau,
        name: &str,
        felder: Option<(u32, bool)>,
        konstruktor: bool,
        selbst_name: Option<String>,
    ) -> Result<u32, JsFehler> {
        if self.basis_funktionen + self.funktionen.len() >= MAX_KONSTANTEN {
            return Err(JsFehler::Speicher("zu viele Funktionen (M4)".to_string()));
        }
        let idx = (self.basis_funktionen + self.funktionen.len()) as u32;
        // Platzhalter: Verschachtelung darf weiter kompilieren.
        self.funktionen.push(leere_funktion(name));
        // J48: Stapel befuellen — beim Zurueckgehen erfährt die nächste
        // umgebende **Nicht-Pfeil**-Funktion, dass sie ein `arguments`-
        // Objekt braucht.
        self.fn_stapel.push(idx as usize);
        let strikt = self.strikt_jetzt() || Self::rumpf_ist_strikt(&bau.rumpf);
        self.strikt_stapel.push(strikt);
        let alte_auswahl_tiefe = std::mem::take(&mut self.auswahl_tiefe);
        // Release 09.10.2026: die Wert-Slots (J20) gehoeren zur **aeusseren**
        // Funktion — ihre Nummern sind Temp-Plaetze DORT. Die innere Funktion
        // zaehlt ihre Temps ab 0 neu; ein geerbter Slot 0 traf dann ihren
        // ersten eigenen Temp.
        //
        // GEMESSEN (node 22 als Soll), der Anlass youtube.com (kevlar_base):
        //   `{ var f = function(J){ switch (J.n) { case 1: J.n = 2 } } }
        //    var J = {n: 1}; f(J); J.n`  -> node `2`, Motor `1`
        // Der `switch`-Wert lag in Temp 0, und `wert_undef` schrieb
        // `undefined` in den geerbten Slot 0 — kein `case` passte mehr. Die
        // Generator-Schleife von kevlar (`switch (J.nextAddress)`) kam so nie
        // voran und lief bis zur Schrittgrenze (300 Mio Befehle).
        let alte_wert_slots = std::mem::take(&mut self.wert_slots);
        let alter_wert_letzter = self.wert_letzter.take();
        // Release 10.10.2026: der Feld-Kontext gehoert NUR zu dieser
        // Funktion (eine innere Funktion hat ihr eigenes `super`).
        // Ein PFEIL im Konstruktor behaelt ihn: `() => super()` ruft den
        // Konstruktor-`super`, die Felder laufen dort.
        let ist_pfeil = bau.pfeil;
        let alte_felder_nach_super = if ist_pfeil { self.felder_nach_super } else { self.felder_nach_super.take() };
        let alt_super_felder = if ist_pfeil { self.super_felder_gesetzt } else { std::mem::take(&mut self.super_felder_gesetzt) };
        if konstruktor && let Some((ff, false)) = felder {
            self.felder_nach_super = Some(ff);
        }
        let mut g = Fn::neu(name);
        // J28: die Zeile der Deklaration im Bytecode-Zustand.
        g.zeile = bau.zeile;
        g.pfeil = bau.pfeil;
        g.asynchron = bau.asynchron;
        g.generator = bau.generator;
        g.braucht_this = !bau.pfeil;
        g.parameter = bau.params.len() as u16;
        // J37: Namen und „einfach" sichern, solange die Muster leben.
        g.param_namen_text = parameter_namen_text(&bau.params, bau.rest.as_ref());
        g.param_einfach = parameterliste_einfach(&bau.params, bau.rest.as_ref());
        g.rest = bau.rest.is_some();
        if !bau.pfeil {
            g.emittiere(Op::DefArgumente);
        }
        self.hissen_in(&mut g, &bau.rumpf)?;
        // J48: ab hier ist der **Parameterprolog** — die Bindungen dieser
        // beiden Schleifen kommen in die Parameterumgebung, die Deklarationen
        // aus `hissen_in` in die des Rumpfs.
        //
        // **UND** die Ausdruecke in den Standardwerten werden ausgewertet,
        // waehrend `Rahmen::umg` schon die Rumpfumgebung ist — der
        // Umschalter `ParamProlog` nimmt sie fuer diese Phase aus dem Spiel.
        //
        // **Nur bei nicht einfacher Liste.** GEMESSEN: `fib(25)` brauchte
        // 4 491 525 Schritte statt der 25 je Aufruf, die der Leistungstest
        // (`tests/j8_leistung.rs`) als Massstab setzt — zwei Schritte je
        // Aufruf allein fuer den Umschalter, ohne dass eine zweite Umgebung
        // entsteht.
        let zweite_umgebung = !g.param_einfach;
        if zweite_umgebung {
            g.emittiere(Op::ParamProlog(true));
        }
        g.param_prolog = zweite_umgebung;
        for (i, m) in bau.params.iter().enumerate() {
            g.emittiere(Op::Param(i as u16));
            self.muster_binden(&mut g, m, 1)?;
        }
        if let Some(r) = &bau.rest {
            g.emittiere(Op::RestParams(bau.params.len() as u16));
            self.muster_binden(&mut g, r, 1)?;
        }
        g.param_prolog = false;
        if zweite_umgebung {
            g.emittiere(Op::ParamProlog(false));
        }
        if let Some((ff, vorher)) = felder {
            if vorher {
                g.emittiere(Op::FelderStart(ff));
            }
        }
        for a in &bau.rumpf {
            self.anweisung(&mut g, a)?;
        }
        if let Some((ff, vorher)) = felder {
            // Release 10.10.2026: nur noch als Rueckfall, wenn der Rumpf kein
            // direktes `super(...)` hat (sonst laufen die Felder dort).
            // GEMESSEN vorher (node 22 als Soll): `class B extends A { x = 1;
            // constructor() { super(); log(this.x) } }` -> `undefined`
            // statt 1, private Felder ebenso.
            if !vorher && !self.super_felder_gesetzt {
                g.emittiere(Op::FelderStart(ff));
            }
        }
        self.pruefe(&g)?;
        if konstruktor {
            g.emittiere(Op::RueckgabeThis);
        } else {
            g.emittiere(Op::Undef);
            g.emittiere(Op::Rueckgabe);
        }
        let mut f = fn_von(g, name, bau.pfeil, bau.asynchron, bau.generator, !bau.pfeil);
        f.selbst_name = selbst_name;
        // Release 09.10.2026: nur ein **direktes** `eval` in diesem Rumpf (oder
        // in einem Pfeil darin, siehe J48 unten) kann `arguments` sehen
        // (ES2023 19.2.1.1: der Eval-Code laeuft in der Umgebung des
        // Aufrufers; eine innere `function` hat ihr eigenes `arguments`).
        //
        // GEMESSEN, warum das hier und nicht mehr am ganzen Skript haengt:
        // vorher reichte das **Wort** `eval` irgendwo im Skript
        // (`Engine::quelle_hat_eval`), und danach baute **jeder** Aufruf
        // sein `arguments`-Objekt. youtube.com (kevlar_base): 4,2 Mio
        // `DefArgumente` zu je ~1,2 us — rund 5 s nur dafuer.
        if f.namen.iter().any(|n| n == "eval") {
            f.braucht_arguments = true;
        }
        // J14: `"use strict"` als **erste** Anweisung des Rumpfs macht diese
        // Funktion strikt (Spec sec-13.1). GEMESSEN mit node 22:
        // `function f(){ 'use strict'; return this } f()` -> `undefined`,
        // eine ohne Direktive daneben -> der globale Bereich.
        // Release 2026-10-15: strict code makes every function inside it
        // strict too (ES2023 11.2.2 "strict mode code"). MEASURED before
        // (WPT dom/collections HTMLCollection-own-props):
        // `(function () { 'use strict'; (function () { return this; })(); })()`
        // gave the global object, and a refused write inside a nested
        // function threw no TypeError.
        f.strikt = strikt;
        // J19: der Quelltext aus dem AST (Pfeilfunktionen, Methoden und
        // `function`-Ausdruecke tragen ihn; der Hauptlauf und die
        // Synthesefunktionen nicht).
        f.quelle = bau.quelle.clone();
        // J19: `async` gehoert zum Quelltext. GEMESSEN: an vier Stellen
        // im Parser wird `async function` erkannt (Anweisung, Ausdruck,
        // Objektliteral, Klasse) — deshalb wird es **hier** einmal
        // ergaenzt statt viermal. node 22:
        // `async function g(){}`.toString() -> `async function g(){}`.
        if bau.asynchron
            && let Some(q) = f.quelle.clone()
            && !q.starts_with("async")
        {
            let mut t = String::with_capacity(q.len() + 6);
            t.push_str("async ");
            t.push_str(&q);
            f.quelle = Some(std::sync::Arc::from(t));
        }
        // J48: Pfeil -> `arguments` an die naechste umgebende Funktion
        // melden. Der Stapel wird von innen nach aussen gebaut, deshalb
        // laeuft die Schleife von hinten; **Pfeile** werden uebersprungen
        // (sie haben kein eigenes `arguments`).
        //
        // GEMESSEN (node 22), der Anlass:
        //   `var a5; var f5 = (x = (a5 = arguments)) => {};
        //    f5(); typeof a5` -> `object`
        // Im Motor: `ReferenceError: 'arguments' ist nicht definiert`, weil
        // die Optimierung aus J8 nur den **eigenen** Namenspool ansah.
        if bau.pfeil && f.braucht_arguments {
            for i in self.fn_stapel.iter().rev().skip(1) {
                if !self.funktionen[*i - self.basis_funktionen].pfeil {
                    self.fn_arguments.insert(*i);
                    break;
                }
            }
        }
        self.fn_stapel.pop();
        self.strikt_stapel.pop();
        self.auswahl_tiefe = alte_auswahl_tiefe;
        self.wert_slots = alte_wert_slots;
        self.felder_nach_super = alte_felder_nach_super;
        if !ist_pfeil {
            self.super_felder_gesetzt = alt_super_felder;
        }
        self.wert_letzter = alter_wert_letzter;
        // J48: eine aeussere Funktion kann wegen eines Pfeils ein
        // `arguments`-Objekt brauchen (siehe `fn_arguments`).
        if self.fn_arguments.contains(&(idx as usize)) {
            f.braucht_arguments = true;
        }
        self.funktionen[idx as usize - self.basis_funktionen] = f;
        Ok(idx)
    }

    /// Synthesefunktion: wertet einen Ausdruck aus und gibt ihn zurueck.
    fn wert_funktion(&mut self, e: &Ausdruck) -> Result<u32, JsFehler> {
        let mut g = Fn::neu("<wert>");
        self.ausdruck(&mut g, e)?;
        g.emittiere(Op::Rueckgabe);
        self.pruefe(&g)?;
        if self.basis_funktionen + self.funktionen.len() >= MAX_KONSTANTEN {
            return Err(JsFehler::Speicher("zu viele Funktionen (M4)".to_string()));
        }
        let idx = (self.basis_funktionen + self.funktionen.len()) as u32;
        self.funktionen
            // (Argumentfolge wie oben: `pfeil` zuerst.)
            .push(fn_von(g, "<wert>", false, false, false, true));
        Ok(idx)
    }

    // =================================================================
    // Anweisungen
    // =================================================================

    fn anweisung(&mut self, f: &mut Fn, a: &Anweisung) -> Result<(), JsFehler> {
        if f.code.len() > MAX_BEFEHLE {
            return Err(JsFehler::Speicher("Funktion zu gross (M4)".to_string()));
        }
        match a {
            // J28: erst die Zeile melden, dann die Anweisung. Das ist die
            // EINZIGE Stelle, an der `Op::Zeile` entsteht — der Haken
            // braucht keine Tabelle und keine Aenderung an `Op` selbst.
            //
            // GEMESSEN, warum VOR der Anweisung: steht das `Op::Zeile`
            // danach, zeigte ein Haltepunkt auf der Zeile einer
            // `if`-Bedingung beim Debugger die Zeile des RUECKSPRUNGS.
            Anweisung::MitZeile { zeile, innen } => {
                f.emittiere(Op::Zeile(*zeile));
                self.anweisung(f, innen)?;
            }
            // J28: `debugger;` bekommt einen Befehl (vorher war es
            // `Anweisung::Leer` — geparst und verworfen).
            Anweisung::Haltepunkt => {
                f.emittiere(Op::Debugger);
                // J20: keine Anweisung ohne Wert -> Slot auf `undefined`.
                self.wert_undef(f);
            }
            Anweisung::Leer => {
                // J20: keine Anweisung ohne Wert -> Slot auf `undefined`.
                self.wert_undef(f);
            }
            Anweisung::Block(v) => self.block(f, v)?,
            Anweisung::Deklaration { art, eintraege } => {
                self.deklaration(f, *art, eintraege)?;
                // J20: eine Deklaration liefert keinen Wert
                // (`{ let a = 1 }` -> `undefined`, node gleich).
                self.wert_undef(f);
            }
            Anweisung::Ausdruck(e) => {
                self.ausdruck(f, e)?;
                // J20: der Wert ist der Vervollstaendigungswert der
                // Anweisungsliste — ohne Slot (oberste Ebene mit
                // direktem Rueckgabe-Schwanz) weiterhin einfach weg.
                match self.wert_slots.last().copied() {
                    Some(t) => {
                        f.emittiere(Op::TempSet(t));
                    }
                    None => {
                        f.emittiere(Op::Pop(1));
                    }
                }
            }
            Anweisung::Wenn {
                bedingung,
                dann,
                sonst,
            } => self.wenn(f, bedingung, dann, sonst)?,
            Anweisung::Solange {
                bedingung,
                rumpf,
                nachlauf,
            } => self.solange(f, bedingung, rumpf, *nachlauf)?,
            Anweisung::Fuer {
                kopf,
                bedingung,
                schritt,
                rumpf,
            } => self.fuer(f, kopf, bedingung, schritt, rumpf)?,
            Anweisung::FuerIn {
                art,
                ziel,
                quelle,
                of,
                rumpf,
                warte,
            } => {
                if *warte {
                    if !*of {
                        return Err(JsFehler::Syntax {
                            zeile: 0,
                            spalte: 0,
                            nachricht: "for await braucht 'of'".to_string(),
                        });
                    }
                    self.fuer_await(f, *art, ziel, quelle, rumpf)?
                } else {
                    self.fuer_in(f, *art, ziel, quelle, *of, rumpf)?
                }
            }
            Anweisung::Auswahl { wert, faelle } => self.auswahl(f, wert, faelle)?,
            Anweisung::Rueckgabe(e) => {
                match e {
                    None => {
                        f.emittiere(Op::Undef);
                    }
                    Some(x) => self.ausdruck(f, x)?,
                }
                // Release 14.10.2026: IteratorClose for the enclosing `for…of`
                // loops (the return value stays on the stack).
                self.iteratoren_schliessen(f, 0);
                self.verlassen_rueckgabe(f, |g| {
                    g.sprung(Op::Rueckgabe);
                });
            }
            Anweisung::Werfen(e) => {
                self.ausdruck(f, e)?;
                f.emittiere(Op::Wurf);
            }
            Anweisung::Versuch {
                rumpf,
                fang_name,
                fang,
                endlich,
                hat_catch,
            } => self.versuch(f, rumpf, fang_name, fang, endlich, *hat_catch)?,
            Anweisung::Abbruch { weiter, marke } => {
                self.abbruch(f, *weiter, marke.as_deref())?;
                // J20: `break`/`continue` liefern keinen Wert.
                self.wert_undef(f);
            }
            // Vom Prolog bzw. beim Betreten des Blocks erledigt — nur in
            // einem Block (locker) setzt die Anweisung selbst das `var`
            // (Annex B.3.3.1: „When the FunctionDeclaration f is evaluated").
            Anweisung::Funktion { name: Some(n), .. }
                if !self.strikt_jetzt()
                    && (self.annex_b.remove(&(a as *const Anweisung as usize)) || self.auswahl_tiefe > 0) =>
            {
                let ni = f.name_index(n);
                f.emittiere(Op::LeseVar(ni));
                f.emittiere(Op::VarSetzen(ni));
            }
            Anweisung::Funktion { .. } => {}
            Anweisung::Klasse {
                name,
                eltern,
                elemente,
                quelle,
            } => {
                self.klasse(
                    f,
                    name.as_deref(),
                    eltern.as_ref(),
                    elemente,
                    true,
                    // J19: Quelltext der Klasse.
                    quelle.as_deref(),
                )?;
            }
            Anweisung::Mit(objekt, rumpf) => {
                // J52: `with` (ES2023 14.11). Der Rumpf bekommt das Objekt als
                // **zusaetzliche** Umgebung; die Befunde stehen bei
                // `Engine::mit_hat`.
                //
                // `var`/`function` im Rumpf gehen **nicht** ins Objekt, sondern
                // in die Funktion (GEMESSEN, node 22):
                //   `var o = {}; with (o) { var v = 3 }` -> `o.v` = `undefined`,
                //   `typeof v` = `"number"`.
                let t = f.temp()?;
                f.emittiere(Op::TempUmgebungSetzen(t));
                self.ausdruck(f, objekt)?;
                f.emittiere(Op::MitStart(t));
                self.anweisung(f, rumpf)?;
                f.emittiere(Op::MitEnde(t));
                f.emittiere(Op::TempLoesche(t));
            }
            Anweisung::Marke(name, innere) => self.marke(f, name, innere)?,
        }
        Ok(())
    }

    /// J20: den Wert-Slot der Anweisungsliste aktivieren (oder den
    /// vorhandenen nehmen). `true`, wenn ein **neuer** Slot angelegt
    /// wurde — dann gehoert er nach dem Block zu `wert_letzter`.
    fn wert_slot(&mut self, f: &mut Fn) -> Result<(u16, bool), JsFehler> {
        if let Some(t) = self.wert_slots.last() {
            return Ok((*t, false));
        }
        let t = f.temp()?;
        self.wert_slots.push(t);
        Ok((t, true))
    }

    /// J20: `undefined` in den aktiven Wert-Slot schreiben (fuer
    /// Anweisungen ohne Wert: Deklarationen, `break`, leere Anweisung …).
    fn wert_undef(&mut self, f: &mut Fn) {
        if let Some(t) = self.wert_slots.last().copied() {
            f.emittiere(Op::Undef);
            f.emittiere(Op::TempSet(t));
        }
    }

    fn block(&mut self, f: &mut Fn, v: &[Anweisung]) -> Result<(), JsFehler> {
        // J20: Der Wert-Slot wird **vor** dem Block angelegt (nur wenn noch
        // keiner laeuft) und nach dem Block wieder abgemeldet. Verschachtelte
        // Bloecke teilen sich denselben Slot — das ist genau die
        // Verkettung, die die Spec verlangt: der Wert einer Anweisungsliste
        // ist der Wert ihrer letzten wertliefernden Anweisung.
        let (slot, neu) = self.wert_slot(f)?;
        if !block_braucht_umgebung(v) {
            for a in v {
                self.anweisung(f, a)?;
            }
        } else {
            let t = f.temp()?;
            f.block_temps.push(t);
            f.emittiere(Op::BlockAuf(t));
            self.block_funktionen(f, v)?;
            for a in v {
                self.anweisung(f, a)?;
            }
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(t));
        }
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    /// Release 09.10.2026: Funktionsdeklarationen einer Block-Liste beim
    /// Betreten anlegen (ES2023 14.2.3 BlockDeclarationInstantiation). Im
    /// lockeren Modus landet der Wert ausserdem im `var` der Funktion
    /// (Annex B.3.3: `{ function f(){} } typeof f` -> `"function"`), strikt
    /// bleibt er im Block. GEMESSEN vorher: `try { function A(){} A.x = 1 }`
    /// warf `ReferenceError: 'A' ist nicht definiert` — die Deklaration
    /// wurde in Bloecken nie angelegt.
    ///
    /// Release 12.10.2026: auch locker bekommt der Block seine EIGENE Bindung
    /// (BlockDeclarationInstantiation gilt immer), das `var` setzt die
    /// Anweisung (Annex B.3.3.1). GEMESSEN vorher (node 22 als Soll):
    /// `with ({f: 1}) { function f(){} typeof f }` -> node `function`, Motor
    /// `number` — ohne Blockbindung gewann das `with`-Objekt (WPT dom/ranges:
    /// die Rahmenskripte laufen in `with (fenster)`, „setupRangeTests is not a
    /// function", 2 x 1.840 Teiltests).
    fn block_funktionen(&mut self, f: &mut Fn, liste: &[Anweisung]) -> Result<(), JsFehler> {
        self.block_funktionen_als(f, liste, false)
    }

    /// `nur_block`: die Funktion bleibt zunaechst im Block (CaseBlock eines
    /// `switch`; ihr `var` setzt erst die Anweisung selbst, siehe `VarSetzen`).
    fn block_funktionen_als(&mut self, f: &mut Fn, liste: &[Anweisung], nur_block: bool) -> Result<(), JsFehler> {
        let strikt = self.strikt_jetzt();
        for roh in liste {
            let knoten = roh.inhalt();
            if let Anweisung::Funktion { name: Some(n), funktion } = knoten {
                let idx = self.funktion_kompilieren(funktion, n, None, false)?;
                let ni = f.name_index(n);
                f.emittiere(Op::NeuFunktion(idx));
                f.emittiere(Op::DefVar(ni, 1));
                // Annex B.3.3.1: nicht fuer Namen der Parameter (node 22:
                // `function h(x){ { function x(){} } return typeof x } h(3)`
                // -> `number`).
                if !strikt && !nur_block && !f.param_namen_text.iter().any(|p| p == n) {
                    self.annex_b.insert(knoten as *const Anweisung as usize);
                }
            }
        }
        Ok(())
    }

    fn strikt_jetzt(&self) -> bool {
        self.strikt_stapel.last().copied().unwrap_or(false)
    }

    /// J34: `IsAnonymousFunctionDefinition` (ES2023 8.2.5) — die
    /// Voraussetzung fuer `SetFunctionName`.
    ///
    /// GEMESSEN (node 22) zu beiden Seiten:
    ///   `var f = function(){}`  -> `f.name` = `"f"`
    ///   `var f = function x(){}` -> `f.name` = `"x"` (der **eigene** Name
    ///                              gewinnt; der VM-Zweig prueft deshalb
    ///                              auf einen noch leeren Namen)
    ///   `var f = class{}`       -> `f.name` = `"f"`
    ///   `o.f = function(){}`    -> `o.f.name` = `""` (Member-Zuweisung!)
    /// J49: definiert das Klassenelement ein **statisches** `name`?
    fn hat_statischen_namen(el: &KlassenElement) -> bool {
        let (statisch, name) = match el {
            KlassenElement::Feld { name, statisch, .. } => (statisch, name),
            KlassenElement::Methode { name, statisch, .. } => (statisch, name),
            KlassenElement::Block { .. } => return false,
        };
        if !*statisch {
            return false;
        }
        match name {
            crate::ast::EigenschaftName::Text(t) => t == "name",
            // J49: auch ein **berechneter** Schluessel, der statisch
            // `"name"` ergibt — der Motor wertet die Schluesselausdruecke
            // ohnehin spaeter aus, hier genuegt der Literalvergleich.
            //
            // GEMESSEN (node 22): `var g = class { static ['name']() {} };
            // typeof g.name` -> `"function"` (die Methode gewinnt). Mit der
            // blosen `Text`-Pruefung bekam der Motor `"string"`.
            crate::ast::EigenschaftName::Berechnet(e) => match &**e {
                Ausdruck::Text(t) => t == "name",
                _ => false,
            },
            crate::ast::EigenschaftName::Zahl(_) => false,
        }
    }

    fn ist_anonyme_funktion(e: &Ausdruck) -> bool {
        match e {
            Ausdruck::Funktion { name, .. } => name.is_none(),
            // J49: eine Klasse, die sich **selbst** ein statisches `name`
            // gibt, traegt ihren Namen (ES2023 15.7.10
            // `ClassDefinitionEvaluation`: `SetFunctionName` nur, wenn die
            // Klasse kein eigenes `name` hat).
            //
            // GEMESSEN (node 22), **sechs** Zeilen:
            //   `var {x2 = class { static name() {} }} = {}; x2.name` -> die
            //      **Methode** (typeof `function`), nicht `"x2"`
            //   `var {y2 = class { static name = 5 }} = {}; y2.name`   -> `5`
            //   `var {z2 = class { static get name() { return 7 } }} = {};`
            //      `typeof z2.name` -> `"number"` (der Accessor gewinnt)
            //   `var {w2 = class { static set name(v) { … } }} = {};`
            //      `typeof w2.name` -> `"undefined"`
            //   `var {cls = class {}} = {}; cls.name`              -> `"cls"`
            //   `var {xCls = class X {}} = {}; xCls.name`          -> `"X"`
            Ausdruck::Klasse { name, elemente, .. } => {
                name.is_none() && !elemente.iter().any(Self::hat_statischen_namen)
            }
            // J49: `Ausdruck::Zuweisung` mit einfachem Ziel ist selbst eine
            // `AssignmentExpression` — der Standard `g = (h = function(){})`
            // benennt also nach `h` (dem Ziel), nicht nach `g`.
            //
            // GEMESSEN (node 22): `var {g = (h = function(){})} = {}; g.name`
            // -> `"h"`; im Motor war es `"g"`.
            Ausdruck::Zuweisung {
                ziel: crate::ast::ZuweisungsZiel::Name(_),
                op: ZuweisungsOp::Gleich,
                wert,
            } => Self::ist_anonyme_funktion(wert),
            // J34: Pfeilfunktionen sind **auch** `Ausdruck::Funktion` mit
            // `pfeil: true` und `name: None` — der Zweig above deckt sie
            // ab. GEMESSEN (node 22): `const {a = ()=>{}} = {}` ->
            // `a.name` = `"a"`.
            _ => false,
        }
    }

    /// J34: `SetFunctionName` ausgeben. Nur bei **Bindungen** — der Aufrufer
    /// von `Ausdruck::Zuweisung` auf ein Member (`o.f = fn`) ruft das
    /// ausdruecklich nicht.
    fn namen_setzen(&mut self, f: &mut Fn, e: &Ausdruck, name: &str) -> Result<(), JsFehler> {
        if !Self::ist_anonyme_funktion(e) {
            return Ok(());
        }
        // J49: ist der Ausdruck selbst eine **Zuweisung an einen einfachen
        // Namen**, dann zaehlt deren Zielname (`SetFunctionName` bekommt ihn
        // von `IsAnonymousFunctionDefinition`/`AssignmentExpression`).
        //
        // GEMESSEN (node 22): `var {g = (h = function(){})} = {}; g.name`
        // -> `"h"`, nicht `"g"`.
        let ziel = match e {
            Ausdruck::Zuweisung {
                ziel: crate::ast::ZuweisungsZiel::Name(z),
                op: ZuweisungsOp::Gleich,
                ..
            } => z.as_str(),
            _ => name,
        };
        let ni = f.name_index(ziel);
        f.emittiere(Op::NamenSetzen(ni));
        Ok(())
    }

    fn deklaration(
        &mut self,
        f: &mut Fn,
        art: DeklArt,
        eintraege: &[(Muster, Option<Ausdruck>)],
    ) -> Result<(), JsFehler> {
        let nr = match art {
            DeklArt::Var => 0,
            DeklArt::Let => 1,
            DeklArt::Const => 2,
        };
        for (m, init) in eintraege {
            match init {
                Some(e) => {
                    self.ausdruck(f, e)?;
                    // J34: `const f = function(){}` -> `f.name` = `"f"`.
                    if let Muster::Name(n) = m {
                        self.namen_setzen(f, e, n)?;
                    }
                    self.muster_binden(f, m, nr)?;
                }
                None => {
                    // J34: `var a;` **ohne** Initialisierer darf eine
                    // vorhandene Bindung **nicht** ueberschreiben.
                    // GEMESSEN (node 22): `function f(a){ var a; return a }
                    // f(7)` -> `7`. In diesem Motor: `undefined` — der
                    // Compiler legte `Undef` auf den Stapel und `DefVar`
                    // schrieb es in die **Parameter**-Bindung.
                    // ES2023 13.2.11 (FunctionDeclarationInstantiation,
                    // Schritt 27.b): `if n is not an element of
                    // parameterBindings, ... create the binding with
                    // undefined`. Der `art`-Code 3 sagt der VM genau das:
                    // deklarieren, **wenn** es fehlt — sonst nichts tun.
                    let art_nr = if nr == 0 { 3 } else { nr };
                    for n in muster_namen(m) {
                        let ni = f.name_index(&n);
                        f.emittiere(Op::Undef);
                        f.emittiere(Op::DefVar(ni, art_nr));
                    }
                }
            }
        }
        Ok(())
    }

    fn wenn(
        &mut self,
        f: &mut Fn,
        bedingung: &Ausdruck,
        dann: &Anweisung,
        sonst: &Option<Box<Anweisung>>,
    ) -> Result<(), JsFehler> {
        self.ausdruck(f, bedingung)?;
        // J20: `if` ist eine wertliefernde Anweisung. Ohne `else` muss der
        // Slot beim **nicht** ausgefuehrten Zweig `undefined` bekommen
        // (node: `if (false) { 5 }` -> `undefined`).
        let (slot, neu) = self.wert_slot(f)?;
        self.wert_undef(f);
        let p1 = f.sprung(Op::SpringWennFalsch(0));
        self.anweisung(f, dann)?;
        match sonst {
            None => {
                let z = f.hier();
                f.patch(p1, z);
            }
            Some(s) => {
                let p2 = f.sprung(Op::Spring(0));
                let z = f.hier();
                f.patch(p1, z);
                self.anweisung(f, s)?;
                let z2 = f.hier();
                f.patch(p2, z2);
            }
        }
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    fn solange(
        &mut self,
        f: &mut Fn,
        bedingung: &Ausdruck,
        rumpf: &Box<Anweisung>,
        nachlauf: bool,
    ) -> Result<(), JsFehler> {
        // J20: eine Schleife liefert den Wert ihres **letzten**
        // Rumpfdurchlaufs; lief sie nie, ist der Wert `undefined`.
        let (slot, neu) = self.wert_slot(f)?;
        self.wert_undef(f);
        if nachlauf {
            let start = f.hier();
            // GEMESSEN: das Ziel muss VOR dem Rumpf stehen, sonst findet ein
            // `break`/`continue` im Rumpf kein Ziel (die VM beendete dann den
            // ganzen Aufruf). `continue` springt auf die BEDINGUNG.
            let marken_liste = self.marken_liste(f);
            f.ziele.push(Ziel {
                iter_temp: None,
                async_iter_temp: None,
                marken: marken_liste,
                ende: Vec::new(),
                weiter: Some(Vec::new()),
                umg_tiefe: f.block_temps.len(),
                schleife: true,
            });
            self.anweisung(f, rumpf)?;
            let bedingung_pc = f.hier();
            self.weiter_patchen(f, bedingung_pc);
            self.ausdruck(f, bedingung)?;
            // NUR ein Ruecksprung: `SpringWennWahr` springt zurueck, der
            // Fall-through ist das Ende. (GEMESSEN: ein zusaetzlicher
            // unbedingter Ruecksprung liess `do {...} while(false)` endlos
            // laufen.)
            f.sprung(Op::SpringWennWahr(start));
            self.ende_und_weiter(f, false);
            if neu {
                self.wert_slots.pop();
                self.wert_letzter = Some(slot);
            }
            return Ok(());
        }
        let start = f.hier();
        self.ausdruck(f, bedingung)?;
        let p = f.sprung(Op::SpringWennFalsch(0));
        let marken_liste = self.marken_liste(f);
        f.ziele.push(Ziel {
            iter_temp: None,
            async_iter_temp: None,
            marken: marken_liste,
            ende: Vec::new(),
            weiter: Some(Vec::new()),
            umg_tiefe: f.block_temps.len(),
            schleife: true,
        });
        self.anweisung(f, rumpf)?;
        self.weiter_patchen(f, start);
        // WICHTIG: der Ruecksprung muss VOR der Sprungmarke der Abbruch-
        // ziel liegen. Sonst sprang der Austritt (z) in den Ruecksprung
        // hinein und `while (i<3) { i=i+1 }` endete nie.
        // GEMESSEN: genau das war der Fehler.
        f.sprung(Op::Spring(start));
        let z = f.hier();
        f.patch(p, z);
        // J3b: `false`, nicht `true`. GEMESSEN/GEFUNDEN: `weiter_patchen`
        // haengt die gepatchten Spruenge wieder an die Liste (damit eine
        // aeussere Schleife sie notfalls neu setzen kann) — `ende_und_
        // weiter(f, true)` patcht sie dann ein **zweites** Mal, diesmal auf
        // das Schleifenende. Ein `continue` verliess also die Schleife:
        // `while (i<5){ if (i%2===0) { i++; continue } s+=i; i++ }` ergab
        // `0` statt `4`. Der Pro-Durchlauf-Zweig und `do…while` nutzten
        // schon `false` — deshalb fiel es nur hier auf.
        self.ende_und_weiter(f, false);
        // J20: Slotabmeldung (siehe `solange`).
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    /// `continue`-Spruenge des innersten Ziels auf `ziel` setzen.
    fn weiter_patchen(&mut self, f: &mut Fn, ziel: u32) {
        let mut liste: Vec<usize> = f
            .ziele
            .last_mut()
            .and_then(|z| z.weiter.as_mut())
            .map(|w| std::mem::take(w))
            .unwrap_or_default();
        // GEMESSEN: ZUERST patchen, DANN zurueckhaengen. `append` leert die
        // Quelle — mit der alten Reihenfolge wurde eine leere Liste gepatcht
        // und `continue` sprang auf 0 (Endlosschleife im markierten `for`).
        for p in liste.iter() {
            f.patch(*p, ziel);
        }
        if let Some(z) = f.ziele.last_mut()
            && let Some(w) = z.weiter.as_mut()
        {
            w.append(&mut liste);
        }
    }

    /// `break`-/`continue`-Ziele: `ende` immer, `weiter` nur wenn gewuenscht.
    fn ende_und_weiter(&mut self, f: &mut Fn, auch_weiter: bool) {
        let ziel = f.hier();
        if let Some(z) = f.ziele.pop() {
            for p in z.ende {
                f.patch(p, ziel);
            }
            if auch_weiter {
                if let Some(w) = z.weiter {
                    for p in w {
                        f.patch(p, ziel);
                    }
                }
            }
        }
    }

    /// Nur die `break`-Spruenge des innersten Ziels hierher auflösen.
    fn ziel_ende_patchen(&mut self, f: &mut Fn) {
        let ziel = f.hier();
        let liste: Vec<usize> = f.ziele.last().map(|z| z.ende.clone()).unwrap_or_default();
        for p in liste {
            f.patch(p, ziel);
        }
    }

    /// Klassische `for`-Schleife.
    fn fuer(
        &mut self,
        f: &mut Fn,
        kopf: &FuerKopf,
        bedingung: &Option<Ausdruck>,
        schritt: &Option<Ausdruck>,
        rumpf: &Box<Anweisung>,
    ) -> Result<(), JsFehler> {
        // J20: Schleife -> Wert des letzten Rumpfdurchlaufs (siehe `solange`).
        let (slot, neu) = self.wert_slot(f)?;
        self.wert_undef(f);
        let mut kopf_namen: Vec<u32> = Vec::new();
        // J3b: **`for (let …)` braucht die Pro-Durchlauf-Umgebung nur,
        // wenn der Rumpf einen Verschluss enthaelt.** ES2015
        // (CreatePerIterationEnvironment) bindet die Kopfvariable je
        // Durchlauf neu, damit `for (let i…) { fs.push(() => i) }` je
        // Durchlauf einen anderen Wert liefert. Ohne Funktion, Pfeil oder
        // Klasse im Rumpf ist diese Bindung **nicht beobachtbar** — und
        // sie kostete: GEMESSEN `for (let i…)` 652 ms je 1 Mio. Durchlaeufe
        // gegen 157 ms fuer `for (var i…)`, also 4x — zwei Umgebungen,
        // zwei Namenskopien und zwei Allokationen je Durchlauf.
        //
        // Die Probe uebersetzt den Rumpf in eine **Wegwerf-Funktion** und
        // zaehlt, ob dabei Funktionen entstanden sind. Das faengt auch
        // verschachtelte Funktionen und Klassen (deren Felder und Methoden
        // werden beim Uebersetzen gebaut). Scheitert die Probe, wird
        // conservativ die alte Form benutzt.
        let per_durchlauf_erlaubt = match kopf {
            FuerKopf::Keiner | FuerKopf::Ausdruck(_) => false,
            FuerKopf::Deklaration { art, eintraege } => {
                if *art == DeklArt::Var {
                    false
                } else {
                    for (m, _) in eintraege {
                        for n in muster_namen(m) {
                            kopf_namen.push(f.name_index(&n));
                        }
                    }
                    self.rumpf_hat_verschluss(rumpf)
                }
            }
        };
        // Der Block fuer die Kopfvariablen: einmal fuer die GANZE
        // Schleife (nicht je Durchlauf), damit `i` danach nicht sichtbar
        // ist (JS-Scoping von `for (let i …)`).
        //
        // ZWEI Temps, GEMESSEN/GEFUNDEN: mit einem Temp war `i` nach der
        // Schleife sichtbar (`for (let i=0;i<3;i++){} i` lieferte `3`
        // statt eines ReferenceError). `BlockAb(t)` stellt die Umgebung
        // aus dem Temp wieder her — ich hatte denselben Temp danach mit
        // `TempUmgebungSetzen` ueberschrieben, also stellte `BlockAb` die
        // **Kopfumgebung** wieder her (genau die, die sie verlassen
        // sollte). Also: `t_kopf` traegt die aeussere Umgebung ( fuer
        // `BlockAb`), `t_umgebung` die Kopfumgebung ( fuer `continue`).
        let t_kopf = if per_durchlauf_erlaubt || !kopf_namen.is_empty() {
            Some(f.temp()?)
        } else {
            None
        };
        let t_umgebung = if t_kopf.is_some() {
            Some(f.temp()?)
        } else {
            None
        };
        if !kopf_namen.is_empty()
            && let Some(t) = t_kopf
        {
            // GEMESSEN/GEFUNDEN: der Kopfblock wird **nicht** auf
            // `block_temps` gelegt — dieser Stapel zaehlt die Blockebenen
            // fuer die Sprungmarken der Schleifen. Mit dem Eintrag
            // bekam `Ziel::umg_tiefe` einen zu hohen Wert, und ein `break`
            // blieb unauflösbar (`Syntaxfehler 0:0: unauflösbares Sprungziel
            // 'aussen'`, p2.js).
            f.emittiere(Op::BlockAuf(t));
            if let Some(tu) = t_umgebung {
                // Die Kopfumgebung merken (fuer `continue`).
                f.emittiere(Op::TempUmgebungSetzen(tu));
            }
        }
        // Die Kopfdeklaration wird in JEDER Form gebraucht (die
        // Pro-Durchlauf-Form kopiert sie danach in die Kopie).
        match kopf {
            FuerKopf::Keiner => {}
            // Release 09.10.2026: der Initialisierer als AUSDRUCK wurde nie
            // ausgewertet. GEMESSEN: `var s=0,i; for(i=0;i<3;i++) s+=i; s`
            // -> 0 statt 3 (`i` blieb undefined, die Schleife lief nie) —
            // an youtube.com (d3: `for(iT=0;iT<DT;++iT)`) und in jedem
            // minifizierten Buendel, das seine Zaehler vorab deklariert.
            FuerKopf::Ausdruck(e) => {
                self.ausdruck(f, e)?;
                f.emittiere(Op::Pop(1));
            }
            FuerKopf::Deklaration { art, eintraege } => {
                self.deklaration(f, *art, eintraege)?;
            }
        }
        // J3b: die einfache Form kommt daher, wenn KEIN Verschluss im
        // Rumpf steckt — nicht bloss bei `var`.
        if !per_durchlauf_erlaubt {
            let start = f.hier();
            let p = match bedingung {
                Some(b) => {
                    // Superinstruktion, wenn das Muster `name < zahl`
                    // passt; die Marke traegt den Superbefehl selbst, damit
                    // `patch` sein Ziel setzen kann.
                    if self.bedingung_als_eine_anweisung(f, b) {
                        Some(f.hier() as usize - 1)
                    } else {
                        self.ausdruck(f, b)?;
                        Some(f.sprung(Op::SpringWennFalsch(0)))
                    }
                }
                None => None,
            };
            let marken_liste = self.marken_liste(f);
            f.ziele.push(Ziel {
                iter_temp: None,
                async_iter_temp: None,
                marken: marken_liste,
                ende: Vec::new(),
                weiter: Some(Vec::new()),
                umg_tiefe: f.block_temps.len(),
                schleife: true,
            });
            self.anweisung(f, rumpf)?;
            // J3b: `continue` springt auf die **Schrittangabe**, wenn es eine
            // gibt (JS-Regel fuer `for`), sonst auf den **Test** (JS-Regel
            // fuer `while`/`do`).
            //
            // GEMESSEN/GEFUNDEN (zwei Fehler hintereinander, beide am
            // Orakel-Fall t137: `for (let i=0;i<5;i++){ if (i%2===0)
            // continue; s+=i }` lieferte `0` statt `4`):
            //  1. hier stand `weiter_patchen(f, start)` — der Sprung lief
            //     am Schritt vorbei, `i` wurde nicht weitergezaehlt;
            //  2. die erste Fassung der Korrektur sprang immer auf die
            //     Stelle **nach** dem Rumpf — bei einer `while`-Schleife
            //     ohne Schritt ist das der Ruecksprung der Schleife, der
            //     Sprung verliess also die Schleife ganz.
            let weiter_ziel = if schritt.is_some() { f.hier() } else { start };
            if let Some(tu) = t_umgebung {
                // J3b: vor dem Schritt (dem `continue`-Ziel) die
                // Kopfumgebung zuruecksetzen — ein `continue` aus einem
                // Block hat dessen `BlockAb` uebersprungen.
                f.emittiere(Op::UmgebungTempSetzen(tu));
            }
            self.weiter_patchen(f, weiter_ziel);
            if let Some(s) = schritt {
                // J3b: `i++` als **eine** Anweisung (der Superbefehl legt
                // nichts auf den Stapel, deshalb entfaellt der `Pop`).
                if !self.schritt_als_eine_anweisung(f, s) {
                    self.ausdruck(f, s)?;
                    f.emittiere(Op::Pop(1));
                }
            }
            f.sprung(Op::Spring(start));
            let z = f.hier();
            if let Some(pp) = p {
                f.patch(pp, z);
            }
            // J3b: `false` — siehe die Begruendung am `while`-Zweig
            // (doppeltes Patchen schickte `continue` ans Schleifenende).
            self.ende_und_weiter(f, false);
            // J3b: der Kopfblock endet nach den `break`/`continue`-Zielen.
            if let Some(t) = t_kopf {
                f.emittiere(Op::BlockAb(t));
            }
            // J20: Slotabmeldung — dieser Weg (Kopfblock **ohne**
            // Pro-Durchlauf-Umgebung) ist der **haeufige**: `for (let i = 0;
            // i < 3; i++) { i }` ohne Verschluss im Rumpf. Ohne die
            // Abmeldung blieb der Slot aktiv und `wert_letzter` ungesetzt —
            // GEMESSEN: alle fuenf `for`-Formen lieferten `undefined` statt
            // `5`/`2`/`0` (node 22).
            if neu {
                self.wert_slots.pop();
                self.wert_letzter = Some(slot);
            }
            return Ok(());
        }
        // Pro-Durchlauf-Umgebungen (ES2015 CreatePerIterationEnvironment).
        let (liste_start, anzahl) = f.kopier_liste(&kopf_namen);
        // Startindex im Namenspool: die Namen liegen NICHT am Anfang (der
        // Pool ist gemeinsam fuer die ganze Funktion). GEMESSEN: mit
        // Start 0 kopierte der Befehl die falschen Namen und `() => i`
        // sah immer die 3.
        let t_ges = f.temp()?;
        let t_body = f.temp()?;
        let start = f.hier();
        let p = match bedingung {
            Some(b) => {
                // J3b: auch im Pro-Durchlauf-Zweig die Superinstruktion —
                // `for (let i = 0; …)` ist DER heisse Fall (der Test war
                // 4 von 26 Schritten je Durchlauf). GEMESSEN: nur im
                // `!per`-Zweig zu fusionieren brachte fuer
                // `Schleife 10^7` **null** Schritte weniger.
                if self.bedingung_als_eine_anweisung(f, b) {
                    Some(f.hier() as usize - 1)
                } else {
                    self.ausdruck(f, b)?;
                    Some(f.sprung(Op::SpringWennFalsch(0)))
                }
            }
            None => None,
        };
        f.emittiere(Op::BlockAuf(t_ges));
        // GEMESSEN: die Quelle MUSS der Temp der gesicherten Eltern-
        // Umgebung sein. Aus der Rahmenumgebung zu raten (frueher so)
        // lieferte `() => i` dreimal die 3 statt 0,1,2.
        f.emittiere(Op::KopiereNamen(t_ges, anzahl, liste_start));
        f.emittiere(Op::TempUmgebungSetzen(t_body));
        f.block_temps.push(t_ges);
        let marken_liste = self.marken_liste(f);
        f.ziele.push(Ziel {
            iter_temp: None,
            async_iter_temp: None,
            marken: marken_liste,
            ende: Vec::new(),
            weiter: Some(Vec::new()),
            umg_tiefe: f.block_temps.len(),
            schleife: true,
        });
        self.anweisung(f, rumpf)?;
        f.block_temps.pop();
        self.ziel_ende_patchen(f);
        // Continue-Ziel: Update in einer KOPIE der Body-Umgebung (JS-Regel),
        // danach die Werte zurueck in die Kopf-Umgebung.
        //
        // Release 10.10.2026: `continue` springt VOR die Kopie. GEMESSEN:
        // mit dem Ziel dahinter lief der Schritt in der Umgebung des
        // Durchlaufs selbst — `for(let i=0;i<3;i++){ fs.push(()=>i);
        // continue }` lieferte 1,2,3 statt 0,1,2 (node), ebenso
        // `continue aussen` aus einer inneren Schleife.
        let weiter_ziel = f.hier();
        f.emittiere(Op::UmgebungAusTemp(t_body));
        f.emittiere(Op::KopiereNamen(t_body, anzahl, liste_start));
        self.weiter_patchen(f, weiter_ziel);
        if let Some(s) = schritt {
            // J3b: `i++` als eine Anweisung (kein `Pop` noetig).
            if !self.schritt_als_eine_anweisung(f, s) {
                self.ausdruck(f, s)?;
                f.emittiere(Op::Pop(1));
            }
        }
        f.emittiere(Op::KopiereZurueck(t_ges, anzahl, liste_start));
        f.emittiere(Op::BlockAb(t_ges));
        f.emittiere(Op::TempLoesche(t_body));
        f.sprung(Op::Spring(start));
        let z = f.hier();
        if let Some(pp) = p {
            f.patch(pp, z);
        }
        self.ende_und_weiter(f, false);
        // Release 10.10.2026: auch dieser Zweig verlaesst den Kopfblock.
        // GEMESSEN: ohne das `BlockAb` blieb die Kopfumgebung nach der
        // Schleife aktiv — `function g(e,t){ for(let t=e;t<3;t++){ () => t }
        // return t }` lieferte 3 statt des Parameters. Turbopacks
        // `interopEsm` (bbc.com) versiegelte so `Object.prototype` samt
        // nur-lesbarem `default`, und jedes spaetere `x.default = …` warf.
        if let Some(t) = t_kopf {
            f.emittiere(Op::BlockAb(t));
        }
        // J20: Slotabmeldung (siehe `solange`).
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    /// J3b: enthaelt der Rumpf einen **Verschluss** (Funktion, Pfeil,
    /// Klasse)? Begruendung und Messung an `fuer`.
    ///
    /// Die Probe uebersetzt den Rumpf in eine Wegwerf-Funktion und
    /// vergleicht die Zahl der dabei entstandenen Funktionen — das
    /// findet auch verschachtelte Funktionen und Klassen. Scheitert die
    /// Probe, gilt sie als „ja" und das alte Verhalten bleibt.
    fn rumpf_hat_verschluss(&mut self, rumpf: &Box<Anweisung>) -> bool {
        let vor = self.funktionen.len();
        // GEMESSEN/GEFUNDEN: die offene **Marke** muss gerettet werden. Die
        // Probe uebersetzt auch den Rumpf; steht darin eine Schleife (oder
        // ein `break aussen`), nimmt deren `marken_liste` die Marke der
        // aeusseren Schleife mit — und der echte Uebersetzungsversuch
        // fand sie nicht mehr („unauflösbares Sprungziel `aussen`" in
        // p2.js, ein **markiertes** `for`).
        let marke = self.offene_marke.take();
        let ketten = std::mem::take(&mut self.ketten);
        // J20: auch die **Wert-Slots** der Probe sind Wegwerf-Zustand. Die
        // Probe uebersetzt den Rumpf in eine Funktion, deren Temp-Zaehlung
        // bei 0 beginnt — ein in der Probe angelegter Slot (Nummer 0, 1, …)
        // gehoerte danach aber zum **echten** Rumpf und schrieb in eine
        // fremde Temp-Zelle.
        //
        // GEMESSEN/GEFUNDEN: `let n = 0; while (n < 3) { n++ }` lieferte
        // `undefined` statt `2` (node), `for (let i = 0; i < 3; i++) { i }`
        // ebenfalls. `if` war nicht betroffen (dort wird nicht geprobt).
        let slots = std::mem::take(&mut self.wert_slots);
        let letzter = self.wert_letzter.take();
        let mut wegwerf = Fn::neu("<probe>");
        let erg = self.anweisung(&mut wegwerf, rumpf);
        self.offene_marke = marke;
        self.ketten = ketten;
        self.wert_slots = slots;
        self.wert_letzter = letzter;
        // GEMESSEN/GEFUNDEN: die Zahl **vor** dem Zuruecksetzen merken —
        // danach ist der Vergleich immer `false`, die Optimierung lief also
        // auch bei `fs.push(() => i)` und lieferte `3,3,3` statt `0,1,2`
        // (p2.js Zeile 3).
        let danach = self.funktionen.len();
        self.funktionen.truncate(vor);
        match erg {
            Ok(()) => danach > vor,
            Err(_) => true,
        }
    }

    /// J3b: die **Bedingung** einer Schleife als eine Anweisung
    /// emittieren, wenn sie das Muster `name < zahl` hat.
    ///
    /// GEMESSEN: `for (let i = 0; i < N; i++) { … }` brauchte 26 Schritte
    /// je Durchlauf; der Test allein waren `LeseVar` + `Zahl` + `Kleiner` +
    /// `SpringWennFalsch` — also vier Dispatches und drei Stackbewegungen
    /// fuer einen Vergleich zweier Zahlen. Mit der Superinstruktion ist es
    /// **einer**. Das Muster wird nur erkannt, wenn links ein **Name** und
    /// rechts eine **Zahl** steht; alles andere geht den gewohnten Weg
    /// (die Semantik bleibt damit unangetastet).
    fn bedingung_als_eine_anweisung(&mut self, f: &mut Fn, b: &Ausdruck) -> bool {
        let Ausdruck::Binaer {
            op: BinaerOp::Kleiner,
            links,
            rechts,
        } = b
        else {
            return false;
        };
        let Ausdruck::Name(n) = links.as_ref() else {
            return false;
        };
        let Ausdruck::Zahl(z) = rechts.as_ref() else {
            return false;
        };
        let idx = f.name_index(n);
        f.emittiere(Op::TestWertKleinerZahlSpring(idx, *z, 0));
        true
    }

    /// J3b: die **Schrittangabe** einer Schleife als eine Anweisung, wenn
    /// sie `name++` oder `++name` ist. Fuenf Dispatches (`LeseVar`, `Zahl`,
    /// `Plus`, `SchreibeVar`, `Pop`) werden zu **einem**.
    fn schritt_als_eine_anweisung(&mut self, f: &mut Fn, s: &Ausdruck) -> bool {
        // Release 09.10.2026: NUR `++`. GEMESSEN vorher: `for(;y>3;y--)` lief
        // endlos — der zusammengefasste Befehl zaehlte auch bei `--` hoch
        // (youtube.com, Polymer `flattenBehaviors`: `for(Y=Z.length-1;Y>=0;Y--)`).
        let Ausdruck::Update {
            ziel: ZuweisungsZiel::Name(n),
            hoch: true,
            ..
        } = s
        else {
            return false;
        };
        let idx = f.name_index(n);
        f.emittiere(Op::ErhoeheUndSchreibe(idx));
        true
    }

    /// `for (x in|of ...)`.
    fn fuer_in(
        &mut self,
        f: &mut Fn,
        art: DeklArtOpt,
        ziel: &Muster,
        quelle: &Ausdruck,
        of: bool,
        rumpf: &Box<Anweisung>,
    ) -> Result<(), JsFehler> {
        self.ausdruck(f, quelle)?;
        // VIER Temps: der Iterator belegt `t`, `t+1` (Feld) und `t+2`
        // (Index); danach kommt der Wert.
        let t_state = f.temp()?;
        let _t_iterfeld = f.temp()?;
        let _t_iterindex = f.temp()?;
        let t_val = f.temp()?;
        f.emittiere(Op::IterStart(t_state, if of { 0u8 } else { 1u8 }));
        let start = f.hier();
        let p = f.sprung(Op::IterNaechster(t_state, 0));
        f.emittiere(Op::TempSet(t_val));
        let nr = match art {
            DeklArtOpt::Keins | DeklArtOpt::Var => 0,
            DeklArtOpt::Let => 1,
            DeklArtOpt::Const => 2,
        };
        let eigen = nr != 0;
        let t_ges = if eigen {
            let t = f.temp()?;
            f.block_temps.push(t);
            f.emittiere(Op::BlockAuf(t));
            Some(t)
        } else {
            None
        };
        f.emittiere(Op::TempGet(t_val));
        self.muster_binden(f, ziel, nr)?;
        let marken_liste = self.marken_liste(f);
        f.ziele.push(Ziel {
            iter_temp: if of { Some(t_state) } else { None },
            async_iter_temp: None,
            marken: marken_liste,
            ende: Vec::new(),
            weiter: Some(Vec::new()),
            umg_tiefe: f.block_temps.len(),
            schleife: true,
        });
        self.anweisung(f, rumpf)?;
        self.ziel_ende_patchen(f);
        if let Some(t) = t_ges {
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(t));
        }
        self.weiter_patchen(f, f.hier());
        f.sprung(Op::Spring(start));
        let z = f.hier();
        f.patch(p, z);
        f.emittiere(Op::TempLoesche(t_state));
        f.emittiere(Op::TempLoesche(t_val));
        self.ende_und_weiter(f, false);
        Ok(())
    }

    /// Release 10.10.2026: den Wert oben auf dem Stapel ERWARTEN — in einer
    /// async-Funktion `Op::Erwartung`, in einem ASYNCHRONEN GENERATOR
    /// `yield marke(x)` (der Treiber in `asyncgen.rs` wartet und setzt fort).
    fn erwarten_emittieren(&mut self, f: &mut Fn) {
        if f.asynchron && f.generator {
            f.emittiere(Op::Undef);
            f.emittiere(Op::Hilfe(crate::asyncgen::HILFE_MARKE));
            f.emittiere(Op::Aufruf(1));
            f.emittiere(Op::Ertrag);
        } else {
            f.emittiere(Op::Erwartung);
        }
    }

    /// Release 10.10.2026: `for await (ziel of quelle) rumpf` (ES2018
    /// 14.7.5): `it = asyncIter(quelle)`, dann je Runde `r = await
    /// it.next()`, Ende bei `r.done`, sonst `ziel = r.value`.
    fn fuer_await(
        &mut self,
        f: &mut Fn,
        art: DeklArtOpt,
        ziel: &Muster,
        quelle: &Ausdruck,
        rumpf: &Box<Anweisung>,
    ) -> Result<(), JsFehler> {
        self.ausdruck(f, quelle)?;
        f.emittiere(Op::Undef);
        f.emittiere(Op::Hilfe(crate::asyncgen::HILFE_ASYNC_ITER));
        f.emittiere(Op::Aufruf(1));
        let t_it = f.temp()?;
        let t_r = f.temp()?;
        f.emittiere(Op::TempSet(t_it));
        let i_next = self.text("next")?;
        let i_done = self.text("done")?;
        let i_value = self.text("value")?;
        let start = f.hier();
        f.emittiere(Op::TempGet(t_it));
        f.emittiere(Op::AufrufMethodeText(0, i_next));
        self.erwarten_emittieren(f);
        f.emittiere(Op::TempSet(t_r));
        f.emittiere(Op::TempGet(t_r));
        f.emittiere(Op::LeseEigenschaft(i_done));
        let p = f.sprung(Op::SpringWennWahr(0));
        let nr = match art {
            DeklArtOpt::Keins | DeklArtOpt::Var => 0,
            DeklArtOpt::Let => 1,
            DeklArtOpt::Const => 2,
        };
        let t_ges = if nr != 0 {
            let t = f.temp()?;
            f.block_temps.push(t);
            f.emittiere(Op::BlockAuf(t));
            Some(t)
        } else {
            None
        };
        f.emittiere(Op::TempGet(t_r));
        f.emittiere(Op::LeseEigenschaft(i_value));
        self.muster_binden(f, ziel, nr)?;
        let marken_liste = self.marken_liste(f);
        f.ziele.push(Ziel {
            iter_temp: None,
            async_iter_temp: Some(t_it),
            marken: marken_liste,
            ende: Vec::new(),
            weiter: Some(Vec::new()),
            umg_tiefe: f.block_temps.len(),
            schleife: true,
        });
        self.anweisung(f, rumpf)?;
        self.ziel_ende_patchen(f);
        if let Some(t) = t_ges {
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(t));
        }
        self.weiter_patchen(f, f.hier());
        f.sprung(Op::Spring(start));
        let z = f.hier();
        f.patch(p, z);
        f.emittiere(Op::TempLoesche(t_it));
        f.emittiere(Op::TempLoesche(t_r));
        self.ende_und_weiter(f, false);
        Ok(())
    }

    /// Release 10.10.2026: `yield* quelle` in einem asynchronen Generator —
    /// jeder Wert des inneren asynchronen Iterators wird weitergereicht, der
    /// gesendete Wert geht an dessen `next`; das Ergebnis ist der letzte
    /// `value`. (Naeherung: `throw`/`return` werden nicht an den inneren
    /// Iterator weitergegeben.)
    fn ertrag_stern_async(&mut self, f: &mut Fn, quelle: &Ausdruck) -> Result<(), JsFehler> {
        self.ausdruck(f, quelle)?;
        f.emittiere(Op::Undef);
        f.emittiere(Op::Hilfe(crate::asyncgen::HILFE_ASYNC_ITER));
        f.emittiere(Op::Aufruf(1));
        let t_it = f.temp()?;
        let t_r = f.temp()?;
        let t_send = f.temp()?;
        f.emittiere(Op::TempSet(t_it));
        f.emittiere(Op::Undef);
        f.emittiere(Op::TempSet(t_send));
        let i_next = self.text("next")?;
        let i_done = self.text("done")?;
        let i_value = self.text("value")?;
        let start = f.hier();
        f.emittiere(Op::TempGet(t_it));
        f.emittiere(Op::TempGet(t_send));
        f.emittiere(Op::AufrufMethodeText(1, i_next));
        self.erwarten_emittieren(f);
        f.emittiere(Op::TempSet(t_r));
        f.emittiere(Op::TempGet(t_r));
        f.emittiere(Op::LeseEigenschaft(i_done));
        let p = f.sprung(Op::SpringWennWahr(0));
        f.emittiere(Op::TempGet(t_r));
        f.emittiere(Op::LeseEigenschaft(i_value));
        f.emittiere(Op::Ertrag);
        f.emittiere(Op::TempSet(t_send));
        f.sprung(Op::Spring(start));
        f.patch(p, f.hier());
        f.emittiere(Op::TempGet(t_r));
        f.emittiere(Op::LeseEigenschaft(i_value));
        f.emittiere(Op::TempLoesche(t_it));
        f.emittiere(Op::TempLoesche(t_r));
        f.emittiere(Op::TempLoesche(t_send));
        Ok(())
    }

    /// `switch` als Kette von Vergleichen (korrekt, nicht schnell).
    fn auswahl(
        &mut self,
        f: &mut Fn,
        wert: &Ausdruck,
        faelle: &[(Option<Ausdruck>, Vec<Anweisung>)],
    ) -> Result<(), JsFehler> {
        self.ausdruck(f, wert)?;
        let t = f.temp()?;
        f.emittiere(Op::TempSet(t));
        // J20: `switch` ist eine wertliefernde Anweisung — der Wert ist der
        // der **ausgefuehrten** Verzweigung (node: `switch (5) { case 1: 4;
        // default: 9 }` -> 9). Deshalb vor dem Vergleichen `undefined` in den
        // Slot, und `break` schreibt `undefined` hinein (siehe `abbruch`).
        let (slot, neu) = self.wert_slot(f)?;
        self.wert_undef(f);
        // Release 09.10.2026: Funktionsdeklarationen im CaseBlock leben in
        // einer eigenen Block-Umgebung; ihr `var` setzt erst ihr Fall
        // (GEMESSEN node 22: `switch(0){case 1: function f(){}} typeof f`
        // -> `undefined`, `switch(1){case 1: function f(){}} typeof f` ->
        // `function`).
        let hat_funktion = faelle
            .iter()
            .any(|(_, v)| v.iter().any(|a| matches!(a.inhalt(), Anweisung::Funktion { name: Some(_), .. })));
        let t_block = if hat_funktion {
            let tb = f.temp()?;
            f.block_temps.push(tb);
            f.emittiere(Op::BlockAuf(tb));
            for (_, v) in faelle {
                self.block_funktionen_als(f, v, true)?;
            }
            Some(tb)
        } else {
            None
        };
        self.auswahl_tiefe += 1;
        let mut tests: Vec<usize> = Vec::new();
        for (test, _) in faelle {
            if let Some(e) = test {
                f.emittiere(Op::TempGet(t));
                self.ausdruck(f, e)?;
                f.emittiere(Op::StriktGleich);
                tests.push(f.sprung(Op::SpringWennWahr(0)));
            }
        }
        let p_leer = f.sprung(Op::Spring(0));
        f.ziele.push(Ziel {
            iter_temp: None,
            async_iter_temp: None,
            marken: Vec::new(),
            ende: Vec::new(),
            weiter: None,
            umg_tiefe: f.block_temps.len(),
            schleife: false,
        });
        // Bloecke in Reihenfolge (Fall-through entsteht von selbst).
        let mut starts: Vec<u32> = Vec::new();
        let mut standard: Option<u32> = None;
        for (test, v) in faelle {
            if test.is_none() {
                standard = Some(f.hier());
            }
            starts.push(f.hier());
            for a in v {
                self.anweisung(f, a)?;
            }
        }
        self.ende_und_weiter(f, false);
        self.auswahl_tiefe -= 1;
        let ende = f.hier();
        if let Some(tb) = t_block {
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(tb));
        }
        // J22: **zwei Zaehler statt einem.** GEMESSEN/GEFUNDEN an
        // `consent-banner.20ae7deac179.js` (mozilla.org): ein `switch` mit
        // `default` **vor** einem `case` liess `tests` kuerzer als `faelle`
        // (`tests` bekommt nur einen Platz je Fall **mit** Testausdruck) —
        // `tests[ti]` mit laufendem `ti` lief dann ueber das Ende hinaus:
        // `index out of bounds: the len is 1 but the index is 1` (Panik,
        // im Fenster „JS-Absturz (intern)"). Minimal:
        // `switch (1) { default: 5; case 1: 4; }`.
        //
        // `ti` zaehlt die Tests, `si` zaehlt die Faelle — das Sprungziel
        // ist der **Fall**, nicht der Test: `starts[si]`.
        let mut ti = 0usize;
        for (si, (test, _)) in faelle.iter().enumerate() {
            if test.is_some() {
                f.patch(tests[ti], starts[si]);
                ti += 1;
            }
        }
        f.patch(p_leer, standard.unwrap_or(ende));
        f.emittiere(Op::TempLoesche(t));
        // J20: Slotabmeldung (siehe `solange`).
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    /// `try { } catch (e) { } finally { }`.
    fn versuch(
        &mut self,
        f: &mut Fn,
        rumpf: &[Anweisung],
        fang_name: &Option<Muster>,
        fang: &[Anweisung],
        endlich: &[Anweisung],
        hat_catch: bool,
    ) -> Result<(), JsFehler> {
        let hat_finally = !endlich.is_empty();
        let handler = f.handler.len();
        f.handler.push(Handler {
            fang: 0,
            finally_: 0,
            hat_finally,
        });
        // J20: `try` ist eine wertliefernde Anweisung — Rumpf **und**
        // `catch` schreiben in denselben Slot (`try { 1 } catch { 2 }`
        // liefert 1, `try { x } catch { 2 }` liefert 2).
        let (slot, neu) = self.wert_slot(f)?;
        // J7: der Sprung in den `finally`-Block muss schon **vor** dem
        // Rumpf bekannt sein — ein `return` im Rumpf braucht ihn als Ziel
        // (`verlassen_rueckgabe`). Der Platzhalter `p_vor` liegt deshalb vor
        // `VersuchStart`; der normale Lauf springt mit `p_ueber` darueber
        // hinweg, `versuch` patcht `p_vor` (und die Spruenge von
        // `return`/`break`/`continue`) am Ende auf den `finally`-Anfang.
        //
        // GEMESSEN, warum der Platzhalter nicht einfach vor dem Rumpf
        // stehen darf: ein `Spring` ist ein Sprung — der normale Lauf
        // verliess den Rumpf und `try { return 'a' } finally { 1 }` lieferte
        // `undefined` (der Rueckgabewert wurde nie ausgewertet).
        // Release 10.10.2026: die Umgebung VOR dem `try` merken — `catch`
        // und `finally` beginnen dort, egal in welchem Block geworfen wurde
        // (die VM setzt beim Wurf die Umgebung nicht zurueck).
        let t_vor = f.temp()?;
        f.emittiere(Op::TempUmgebungSetzen(t_vor));
        let p_ueber = f.sprung(Op::Spring(0));
        let p_vor = f.sprung(Op::Spring(0));
        f.patch(p_ueber, f.hier());
        f.emittiere(Op::VersuchStart(handler as u16));
        f.try_stapel.push(TryCtx {
            f_stelle: vec![p_vor],
            hat_finally,
        });
        // J7 (behoben): `return` **im** try-Rumpf merkt seinen Wert als
        // Aktion vor und laesst den `finally`-Block zuerst laufen
        // (`verlassen_rueckgabe`); ein `return` **im** finally-Block legt
        // eine eigene Aktion darueber und gewinnt. GEMESSEN vorher:
        // `try { return 'a' } finally { return 'b' }` lieferte `a` statt
        // `b` (`tests/t262/faelle/ctl-finally-override.js`).
        //
        // BEKANNTE NAEHERUNG: wirft der `finally`-Block, laeuft er bei einem
        // `return` aus dem try-Rumpf zweimal (der Handler der Anweisung ist
        // da noch auf der Rahmenliste). Verhalten: der Wurf gewinnt, der
        // vorgemerkte Rueckgabewert faellt weg — wie in JS.
        // Release 10.10.2026: der `try`-Rumpf ist ein BLOCK (ES2023 14.15:
        // `try Block`). GEMESSEN vorher: `let q = 'aussen'; try { let q = 1 }
        // catch {} q` lieferte 1 (node: 'aussen') — Next.js schrieb so den
        // Router in das `let n` eines `try`-Blocks statt in die Modul-
        // Variable, die Hydration von bbc.com brach ab (leere Seite).
        let t_try = if block_braucht_umgebung(rumpf) {
            let t = f.temp()?;
            f.block_temps.push(t);
            f.emittiere(Op::BlockAuf(t));
            Some(t)
        } else {
            None
        };
        self.block_funktionen(f, rumpf)?;
        for a in rumpf {
            self.anweisung(f, a)?;
        }
        if let Some(t) = t_try {
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(t));
        }
        f.emittiere(Op::VersuchEnde);
        let ueber = f.sprung(Op::Spring(0));
        if hat_catch {
            let fang_pc = f.hier();
            f.handler[handler].fang = fang_pc;
            f.emittiere(Op::UmgebungTempSetzen(t_vor));
            // ZWEI Temps: einer haelt den Wurf, der andere die Eltern-
            // Umgebung des catch-Blocks (sonst ueberschreiben sie sich).
            let t = f.temp()?;
            let t_umg = f.temp()?;
            f.emittiere(Op::VersuchEnde);
            f.emittiere(Op::TempSet(t));
            f.emittiere(Op::BlockAuf(t_umg));
            f.block_temps.push(t_umg);
            f.emittiere(Op::TempGet(t));
            match fang_name {
                Some(m) => self.muster_binden(f, m, 1)?,
                None => {
                    f.emittiere(Op::Pop(1));
                }
            }
            self.block_funktionen(f, fang)?;
            for a in fang {
                self.anweisung(f, a)?;
            }
            f.block_temps.pop();
            f.emittiere(Op::BlockAb(t_umg));
        }
        let mut fertig_vormerken: Option<usize> = None;
        // (der `catch`-Sprung wird ueber `ueber` gepatcht, nicht ueber diese
        //  Variable — daher wird sie hier nicht befuellt)
        #[allow(unused_assignments)]
        let mut nach_catch: Option<usize> = None;
        if hat_finally {
            let p2 = f.sprung(Op::Spring(0));
            f.try_stapel.last_mut().unwrap().f_stelle.push(p2);
            let p1 = f.sprung(Op::FertigVormerken(0));
            fertig_vormerken = Some(p1);
            nach_catch = Some(p1);
        } else {
            nach_catch = Some(f.sprung(Op::Spring(0)));
        }
        f.patch(ueber, f.hier());
        if hat_finally {
            let fpc = f.hier();
            // WICHTIG: der Handler muss den `finally`-Anfang kennen — die VM
            // springt bei einem Wurf direkt dorthin (mit vorgemerkter Aktion).
            // GEMESSEN: ohne dieses `finally_ = 0` sprang der Wurf auf
            // Befehl 0 (zurueck in den `try`-Rumpf) und die Schleife lief
            // endlos.
            f.handler[handler].finally_ = fpc;
            if let Some(t) = f.try_stapel.pop() {
                for p in t.f_stelle {
                    f.patch(p, fpc);
                }
            }
            // J20: der `finally`-Block schreibt in einen **eigenen** Slot.
            // GEMESSEN: `try { 5 } finally { 7 }` lieferte `7` statt `5`
            // (node 22) — der `finally`-Rumpf pfuschte in den Wert-Slot des
            // `try`. Nach der Spec (13.15, `UpdateEmpty` des finally) zaehlt
            // der Wert des Rumpfes nur, wenn der `finally`-Block **nicht
            // abrupt** endet; ein `return` darin nimmt seinen Weg ueber
            // `verlassen_rueckgabe` und ist davon unberuehrt.
            // Release 10.10.2026: `finally` beginnt in der Umgebung vor dem
            // `try` und ist selbst ein Block.
            f.emittiere(Op::UmgebungTempSetzen(t_vor));
            if !endlich.is_empty() {
                let t = f.temp()?;
                self.wert_slots.push(t);
                let t_fin = if block_braucht_umgebung(endlich) {
                    let tb = f.temp()?;
                    f.block_temps.push(tb);
                    f.emittiere(Op::BlockAuf(tb));
                    Some(tb)
                } else {
                    None
                };
                self.block_funktionen(f, endlich)?;
                for a in endlich {
                    self.anweisung(f, a)?;
                }
                if let Some(tb) = t_fin {
                    f.block_temps.pop();
                    f.emittiere(Op::BlockAb(tb));
                }
                self.wert_slots.pop();
            }
            f.emittiere(Op::FertigEnde);
        } else {
            f.try_stapel.pop();
        }
        if let Some(p1) = fertig_vormerken {
            f.patch(p1, f.hier());
        }
        if let Some(p2) = nach_catch {
            f.patch(p2, f.hier());
        }
        // J20: Slotabmeldung nach dem `finally`-Aufraeumen (der Wert
        // bleibt im Slot; ein `return` im try/finaly nimmt seinen Weg
        // ueber `verlassen_rueckgabe` und ist damit nicht betroffen).
        if neu {
            self.wert_slots.pop();
            self.wert_letzter = Some(slot);
        }
        Ok(())
    }

    /// `break`/`continue` (statisch aufgeloest, mit `finally`-Kette).
    fn abbruch(&mut self, f: &mut Fn, weiter: bool, marke: Option<&str>) -> Result<(), JsFehler> {
        let namen = match marke {
            None => KEIN_NAME,
            Some(n) => f.name_index(n),
        };
        let mut treffer: Option<(usize, bool)> = None;
        for (i, z) in f.ziele.iter().enumerate().rev() {
            if namen == KEIN_NAME {
                if !weiter || z.schleife {
                    treffer = Some((i, weiter && z.schleife));
                    break;
                }
            } else if z.marken.contains(&namen) {
                treffer = Some((i, weiter && z.schleife));
                break;
            }
        }
        let (idx, ist_weiter) = match treffer {
            Some(t) => t,
            None => {
                // GEMESSEN/GEFORDERT (W6B): ein stiller Abbruch ist nicht
                // erlaubt — `break`/`continue` mit unauflösbarer Marke ist in
                // JS ein SyntaxError. Ohne Marke bleibt es tolerant (wie
                // narsil-js), damit ein `break` ausserhalb einer Schleife nicht
                // das ganze Programm umstellt.
                if marke.is_some() {
                    return Err(JsFehler::Syntax {
                        zeile: 0,
                        spalte: 0,
                        nachricht: format!(
                            "unauflösbares Sprungziel `{}`",
                            marke.unwrap_or_default()
                        ),
                    });
                }
                self.verlassen(f, |g| {
                    g.sprung(Op::RueckgabeUndefined);
                });
                return Ok(());
            }
        };
        if weiter && !ist_weiter {
            self.verlassen(f, |g| {
                g.sprung(Op::RueckgabeUndefined);
            });
            return Ok(());
        }
        // Release 14.10.2026: IteratorClose for every `for…of` the jump
        // leaves, innermost first (a `continue` keeps its own loop running).
        let bis = if ist_weiter { idx + 1 } else { idx };
        self.iteratoren_schliessen(f, bis);
        // Block-Umgebungen schliessen, die das Ziel verlassen.
        let ziel_umg = f.ziele[idx].umg_tiefe;
        if !weiter {
            self.blocks_schliessen(f, ziel_umg);
        }
        self.verlassen(f, |g| {
            g.sprung(Op::Spring(0));
        });
        let pos = f.code.len() - 1;
        if ist_weiter {
            if let Some(z) = f.ziele.get_mut(idx)
                && let Some(w) = z.weiter.as_mut()
            {
                w.push(pos);
            }
        } else if let Some(z) = f.ziele.get_mut(idx) {
            z.ende.push(pos);
        }
        Ok(())
    }

    /// Release 14.10.2026: close the iterators of the `for…of` / `for await`
    /// loops `f.ziele[von..]`, innermost first (IteratorClose /
    /// AsyncIteratorClose). The stack is left as it is.
    fn iteratoren_schliessen(&mut self, f: &mut Fn, von: usize) {
        let zu: Vec<(Option<u16>, Option<u16>)> = f.ziele[von.min(f.ziele.len())..]
            .iter()
            .rev()
            .map(|z| (z.iter_temp, z.async_iter_temp))
            .collect();
        for (sync, asynchron) in zu {
            if let Some(t) = sync {
                f.emittiere(Op::IterSchliessen(t));
            }
            if let Some(t) = asynchron {
                f.emittiere(Op::TempGet(t));
                f.emittiere(Op::Undef);
                f.emittiere(Op::Hilfe(crate::asyncgen::HILFE_ASYNC_SCHLIESSEN));
                f.emittiere(Op::Aufruf(1));
                self.erwarten_emittieren(f);
                f.emittiere(Op::Pop(1));
            }
        }
    }

    /// `BlockAb` fuer alle offenen Bloecke ueber `tiefe` ausgeben.
    fn blocks_schliessen(&mut self, f: &mut Fn, tiefe: usize) {
        // Release 10.10.2026: NUR die Befehle — `block_temps` beschreibt die
        // Bloecke an DIESER Stelle des Quelltexts und bleibt fuer die
        // Anweisungen nach dem `break` gueltig. Vorher wurde hier gepoppt:
        // nach `if (x) break;` in einem Block nahm das Blockende den Eintrag
        // des AEUSSEREN Blocks mit, und spaetere Schleifen bekamen eine zu
        // kleine `umg_tiefe`.
        let von = tiefe.min(f.block_temps.len());
        let liste: Vec<u16> = f.block_temps[von..].iter().rev().copied().collect();
        for t in liste {
            f.emittiere(Op::BlockAb(t));
        }
    }

    /// Kontrollwechsel mit `finally`-Kette.
    ///
    /// Reihenfolge (LIFO): das eigentliche Ziel wird ZUERST vorgemerkt, dann
    /// die aeusseren `finally`-Bloecke (von aussen nach innen), dann springt
    /// man in den INNERSTEN. `FertigEnde` des innersten holt damit den
    /// naechsten, usw., bis das eigentliche Ziel ausgefuehrt wird.
    fn verlassen<F: FnOnce(&mut Fn)>(&mut self, f: &mut Fn, emit: F) {
        self.verlassen_mit(f, emit, false);
    }

    /// `return` mit Wert: der Wert liegt schon auf dem Stapel. Mit
    /// `finally`-Kette wird er als Aktion vorgemerkt (J7).
    fn verlassen_rueckgabe<F: FnOnce(&mut Fn)>(&mut self, f: &mut Fn, emit: F) {
        self.verlassen_mit(f, emit, true);
    }

    fn verlassen_mit<F: FnOnce(&mut Fn)>(&mut self, f: &mut Fn, emit: F, als_rueckgabe: bool) {
        let mut pcs: Vec<usize> = Vec::new();
        let mut try_idx: Vec<usize> = Vec::new();
        for (i, t) in f.try_stapel.iter().enumerate() {
            if t.hat_finally {
                if let Some(p) = t.f_stelle.first().copied() {
                    pcs.push(p);
                    try_idx.push(i);
                }
            }
        }
        if pcs.is_empty() {
            emit(f);
            return;
        }
        let p0 = if als_rueckgabe {
            None
        } else {
            Some(f.sprung(Op::FertigVormerken(0)))
        };
        if als_rueckgabe {
            // sec-14.15.2: der Wert wird vorgemerkt, der `finally`-Block
            // laeuft danach, und **sein** `return` (falls vorhanden) legt
            // eine eigene Aktion darueber — das gewinnt (J7).
            f.emittiere(Op::RueckgabeVormerken);
        }
        for i in (1..pcs.len()).rev() {
            f.emittiere(Op::FertigVormerken(pcs[i] as u32));
        }
        let p = f.sprung(Op::Spring(pcs[0] as u32));
        f.try_stapel[try_idx[0]].f_stelle.push(p);
        if let Some(p0) = p0 {
            f.patch(p0, f.hier());
        }
        if !als_rueckgabe {
            emit(f);
        }
    }

    /// `label: anweisung`.
    fn marke(&mut self, f: &mut Fn, name: &str, innere: &Box<Anweisung>) -> Result<(), JsFehler> {
        let ni = f.name_index(name);
        // J28: `MitZeile` ist durchsichtig (dieselbe Regel wie in
        // `hissen_in`, `rumpf_ist_strikt`, `uebersetze`).
        // GEMESSEN, warum es hier zwingend war: die Marke haengt an einer
        // **Schleife**, und `matches!` sah nur den `MitZeile`-Knoten. Die
        // Schleife wurde dann nicht als Schleife erkannt, `offene_marke`
        // blieb leer — und `continue aussen` lief in die falsche Richtung.
        // Gemessen als Abbruch nach `ij 0 0` in `tests/koord.rs::p2`.
        let schleife = matches!(
            (**innere).inhalt(),
            Anweisung::Solange { .. } | Anweisung::Fuer { .. } | Anweisung::FuerIn { .. }
        );
        if schleife {
            // Die Marke wird von der Schleifen-Uebersetzung in ihr eigenes
            // Ziel uebernommen (siehe `offene_marke`).
            self.offene_marke = Some(name.to_string());
            self.anweisung(f, innere)?;
            self.offene_marke = None;
        } else {
            f.ziele.push(Ziel {
                iter_temp: None,
                async_iter_temp: None,
                marken: vec![ni],
                ende: Vec::new(),
                weiter: None,
                umg_tiefe: f.block_temps.len(),
                schleife: false,
            });
            self.anweisung(f, innere)?;
            let ziel = f.hier();
            if let Some(z) = f.ziele.pop() {
                for p in z.ende {
                    f.patch(p, ziel);
                }
            }
        }
        Ok(())
    }

    // =================================================================
    // Muster (Destrukturierung)
    // =================================================================

    /// Muster an den Quellwert auf dem Stapel binden (Deklaration/Parameter).
    fn muster_binden(&mut self, f: &mut Fn, m: &Muster, art: u8) -> Result<(), JsFehler> {
        match m {
            // Release 10.10.2026: der Parser baut `Muster::Ziel` nur fuer
            // Zuweisungen; eine Deklaration mit Member-Ziel ist ein
            // Syntaxfehler (ES2023 14.3.3 `BindingPattern`).
            Muster::Ziel(_) => {
                return Err(JsFehler::Syntax {
                    zeile: 0,
                    spalte: 0,
                    nachricht: "ein Member ist kein Bindungsname".to_string(),
                });
            }
            Muster::Name(n) => {
                let ni = f.name_index(n);
                // J48: im **Parameterprolog** gehoert die Bindung in die
                // Parameterumgebung, damit ein Ausdruck in einer spaeteren
                // Parameterliste die Deklarationen des **Rumpfs** nicht sieht
                // (ES2023 10.2.11 Schritt 27).
                if f.param_prolog {
                    f.emittiere(Op::DefParam(ni, art));
                } else {
                    f.emittiere(Op::DefVar(ni, art));
                }
            }
            Muster::Standard(inner, expr) => {
                // J34: `const {a = function(){}} = {}` -> `a.name` = `"a"`
                // (ES2023 8.2.5 `BindingElement : SingleNameBinding BindingInitializer`,
                // `NamedEvaluation` mit dem **einfachen** Namen als Ziel).
                if let Muster::Name(n) = inner.as_ref() {
                    self.namen_setzen(f, expr, n)?;
                }
                // GEMESSEN: `SpringWennUndef` springt, wenn der Wert
                // UNDEFINED ist — der Standard wird dann berechnet. Bei
                // definiertem Wert muss ueber den Standard gesprungen werden
                // (sonst ueberschrieb `{a=5}` mit `{a:1}` immer die 5).
                let p_undef = f.sprung(Op::SpringWennUndef(0));
                let p_ueber = f.sprung(Op::Spring(0));
                let pop_pos = f.hier();
                f.emittiere(Op::Pop(1));
                self.ausdruck(f, expr)?;
                let p_ende = f.sprung(Op::Spring(0));
                f.patch(p_undef, pop_pos);
                f.patch(p_ende, f.hier());
                f.patch(p_ueber, f.hier());
                self.muster_binden(f, inner, art)?;
            }
            Muster::Rest(inner) => {
                // `[...r]` als Deklarationsziel.
                f.emittiere(Op::Pick(0));
                f.emittiere(Op::MusterRestFeld(0));
                self.muster_binden(f, inner, art)?;
            }
            Muster::Feld(teile) => {
                // J34: die Quelle einmal auf einen Feldwert bringen (siehe
                // `Op::MusterQuelle`). Ein **echtes** Feld bleibt liegen —
                // der indizierte Weg ist dann schneller als jeder Iterator,
                // und ein echtes Feld ist der haeufigste Fall.
                //
                // **Leeres** Muster `[] = q`: nur die Iterierbarkeit pruefen.
                // `MusterQuelle` wuerde einen Generator materialisieren und
                // damit **verbrauchen** (`iter_werte` laeuft den Rumpf), obwohl
                // `[]` keinen einzigen Wert braucht — siehe die Notiz bei
                // `Op::MusterQuelleLeer`. (test262
                // `class/dstr/gen-meth-ary-ptrn-elem-ary-empty-init`.)
                if teile.is_empty() {
                    f.emittiere(Op::MusterQuelleLeer);
                    // Release 10.10.2026: auch hier verbraucht (siehe unten).
                    f.emittiere(Op::Pop(1));
                    return Ok(());
                }
                f.emittiere(Op::MusterQuelleFeld);
                for (i, t) in teile.iter().enumerate() {
                    // Release 10.10.2026: eine Luecke bindet nichts (siehe
                    // `LOCH`) — die Quelle ist schon ein Feld, der Index
                    // laeuft trotzdem weiter.
                    if ist_loch(&t.muster) {
                        continue;
                    }
                    match &t.muster {
                        Muster::Rest(inner) => {
                            f.emittiere(Op::Pick(0));
                            f.emittiere(Op::MusterRestFeld(i as u32));
                            self.muster_binden(f, inner, art)?;
                        }
                        _ => {
                            f.emittiere(Op::Pick(0));
                            f.emittiere(Op::Zahl(i as f64));
                            f.emittiere(Op::LeseIndex);
                            let zielname = match &t.muster {
                                Muster::Name(n) => Some(n.as_str()),
                                _ => None,
                            };
                            if let Some(std) = t.standard.as_deref() {
                                self.standard_ueber(f, Some(std), zielname)?;
                            }
                            self.muster_binden(f, &t.muster, art)?;
                        }
                    }
                }
                // Release 10.10.2026: die Quelle VERBRAUCHEN wie der
                // Namenszweig (`DefVar` nimmt den Wert). Vorher blieb sie
                // liegen — ein verschachteltes Muster liess die INNERE Quelle
                // stehen, und das naechste Geschwister las aus ihr. GEMESSEN
                // (node 22 in Klammern): `var [[x], [y]] = [[1], [2]]` ->
                // TypeError  (`1/2`); `var [[a, b], c] = [['k', 1], ['l', 2]]`
                // -> c = 1  (`l,2`). Dazu blieb je Destrukturierung ein Wert
                // auf dem Stapel.
                f.emittiere(Op::Pop(1));
            }
            Muster::Gegenstand(teile) => {
                // GEMESSEN (A5, Orakel w039): `const {a, ...rest} = o`
                // lieferte eine Kopie von ALLEN Schluesseln. Der Rest
                // bekaommt jetzt die Liste der schon entnommenen.
                let mut raus: Vec<u32> = Vec::new();
                for g in teile {
                    // GEMESSEN: die Kurzform `{a}` ist ein ganz normaler
                    // Schluessel `a` — sie gehört in die Ausschlussliste
                    // (vorher war `!g.kurz` Bedingung, dann blieb `a` im
                    // Rest: `const {a, ...s}` -> `{a:1,b:2}` statt `{b:2}`).
                    if !matches!(g.muster, Muster::Rest(_))
                        && let crate::ast::EigenschaftName::Text(t) = &g.schluessel
                    {
                        raus.push(self.text(t)?);
                    }
                }
                for g in teile {
                    match &g.muster {
                        Muster::Rest(inner) => {
                            f.emittiere(Op::Pick(0));
                            let idx = f.muster_aus.len() as u32;
                            f.muster_aus.push(raus.clone());
                            f.emittiere(Op::MusterRest(idx));
                            self.muster_binden(f, inner, art)?;
                        }
                        _ => {
                            // Quelle kopieren, Schluessel drauf, lesen.
                            f.emittiere(Op::Pick(0));
                            self.eigenschaft_name(f, &g.schluessel, g.kurz)?;
                            f.emittiere(Op::LeseIndex);
                            // J49: **zwei** Benennungen, nicht eine.
                            //
                            // ES2023 8.2.5 (`BindingElement : SingleNameBinding
                            // BindingInitializer` → `NamedEvaluation`) benennt
                            // **beide** Werte: den aus der Quelle und — nur,
                            // wenn er eine `AssignmentExpression` ist — den aus
                            // dem Standard. Ein Kommaausdruck ist keine.
                            //
                            // GEMESSEN (node 22), **zehn** Zeilen:
                            //   `var {a = (function(){})} = {}; a.name`       -> `"a"`
                            //   `var {c = (0, function(){})} = {}; c.name`    -> `""`
                            //   `var {d = (0, 1, function(){})} = {}; d.name` -> `""`
                            //   `var {e = ((0, function(){}))} = {}; e.name`  -> `""`
                            //   `var {i = (1 ? f : g) = {}; i.name`           -> `""`
                            //   `var {j = (0 || f) = {}; j.name`              -> `""`
                            //   `var {u = (0, class {})} = {}; u.name`        -> `""`
                            //   `var {g = (h = function(){})} = {}; g.name`   -> `"h"`
                            //   `const {a} = {a: function(){}}; a.name`       -> `"a"`
                            //   `const {a} = {a: function inn(){}}`           -> `"inn"`
                            //
                            // **Die erste Fassung benannte nur den Wert aus der
                            // Quelle** und gab dem Standardweg keinen Namen
                            // (`standard_ueber(f, …, None)`). Damit bekam `c`
                            // den Namen `c` statt `""` — die Gruppen
                            // `fn-name-cover` und `fn-name-class` blieben rot.
                            if let Muster::Name(n) = &g.muster {
                                let ni = f.name_index(n);
                                f.emittiere(Op::NamenSetzen(ni));
                            }
                            self.standard_ueber(
                                f,
                                g.standard.as_deref(),
                                match &g.muster {
                                    Muster::Name(n) => Some(n.as_str()),
                                    _ => None,
                                },
                            )?;
                            self.muster_binden(f, &g.muster, art)?;
                        }
                    }
                }
                // Release 10.10.2026: die Quelle verbrauchen (siehe Feldzweig).
                f.emittiere(Op::Pop(1));
            }
        }
        Ok(())
    }

    /// `standard: Some(Ausdruck)` anwenden (Wert liegt auf dem Stapel).
    ///
    /// `name` ist der Zielname fuer `SetFunctionName` — **nur** im Zweig, in
    /// dem der Standard wirklich berechnet wird (J34).
    fn standard_ueber(
        &mut self,
        f: &mut Fn,
        e: Option<&Ausdruck>,
        name: Option<&str>,
    ) -> Result<(), JsFehler> {
        let e = match e {
            None => return Ok(()),
            Some(x) => x,
        };
        // Siehe `Muster::Standard`: der Standard wird nur bei UNDEFINED
        // berechnet, sonst wird darueber gesprungen.
        let p_undef = f.sprung(Op::SpringWennUndef(0));
        let p_ueber = f.sprung(Op::Spring(0));
        let pop_pos = f.hier();
        f.emittiere(Op::Pop(1));
        self.ausdruck(f, e)?;
        // J34: **hier**, nicht nachher — im anderen Zweig liegt der Wert aus
        // der Quelle auf dem Stapel, und der gehoert nach ES2023 14.3.3
        // (`BindingElement : SingleNameBinding` → `NamedEvaluation` von
        // `BindingElementInitializer`) zwar auch zur Benennung, aber der
        // Array-Weg nutzt `IteratorElementInitialization`, das **kein**
        // `NamedEvaluation` kennt. GEMESSEN (node 22):
        //   `const [a = function(){}] = []`          -> `a.name` = `"a"`
        //   `const [a = function(){}] = [function(){}]` -> `a.name` = `""`
        if let Some(n) = name {
            self.namen_setzen(f, e, n)?;
        }
        let p_ende = f.sprung(Op::Spring(0));
        f.patch(p_undef, pop_pos);
        f.patch(p_ende, f.hier());
        f.patch(p_ueber, f.hier());
        Ok(())
    }

    /// Schluessel eines Musters auf den Stapel legen.
    fn eigenschaft_name(
        &mut self,
        f: &mut Fn,
        n: &EigenschaftName,
        kurz: bool,
    ) -> Result<(), JsFehler> {
        match n {
            EigenschaftName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::Text(i));
            }
            EigenschaftName::Zahl(z) => {
                f.emittiere(Op::Zahl(*z));
            }
            EigenschaftName::Berechnet(e) => self.ausdruck(f, e)?,
        }
        let _ = kurz;
        Ok(())
    }

    // =================================================================
    // Ausdruecke
    // =================================================================

    /// Ein Ausdruck uebersetzen. Er SCHIEBT genau einen Wert.
    fn ausdruck(&mut self, f: &mut Fn, e: &Ausdruck) -> Result<(), JsFehler> {
        if f.code.len() > MAX_BEFEHLE {
            return Err(JsFehler::Speicher("Funktion zu gross (M4)".to_string()));
        }
        match e {
            Ausdruck::Zahl(z) => {
                f.emittiere(Op::Zahl(*z));
            }
            // J24: BigInt-Literal (`123n`).
            Ausdruck::Ganz(g) => {
                f.emittiere(Op::Ganz(*g));
            }
            Ausdruck::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::Text(i));
            }
            Ausdruck::Wahr(b) => {
                f.emittiere(if *b { Op::Wahr } else { Op::Falsch });
            }
            Ausdruck::Null => {
                f.emittiere(Op::Null);
            }
            Ausdruck::Undefiniert => {
                f.emittiere(Op::Undef);
            }
            Ausdruck::Dies => {
                f.emittiere(Op::This);
            }
            Ausdruck::Super => {
                f.emittiere(Op::This);
            }
            // W14 (JS, 04.10.2026): `new.target` — ES2023 13.3.7.2. Der
            // Parser hat es vorher pauschal abgelehnt („new.target wird nicht
            // unterstuetzt"), obwohl die **Laufzeitseite** vollstaendig war:
            // `Rahmen::neu` wird bei `new F()` gesetzt und beim `super()`
            // weitergereicht. Es fehlte nur der Lese-Befehl.
            Ausdruck::NeuZiel => {
                f.emittiere(Op::NeuZiel);
            }
            Ausdruck::Regex { muster, schalter } => {
                let m = self.text(muster)?;
                let s = self.text(schalter)?;
                f.emittiere(Op::Regex(m, s));
            }
            Ausdruck::Name(n) => {
                let ni = f.name_index(n);
                f.emittiere(Op::LeseVar(ni));
            }
            // Release 15.10.2026: tagged template object (see `Op::VorlagenObjekt`).
            Ausdruck::VorlagenObjekt(teile) => {
                let (gekocht, roh) = &**teile;
                if gekocht.len() > u16::MAX as usize {
                    return Err(JsFehler::Speicher("Template zu gross (M4)".to_string()));
                }
                for g in gekocht {
                    match g {
                        Some(t) => {
                            let i = self.text(t)?;
                            f.emittiere(Op::Text(i));
                        }
                        None => {
                            f.emittiere(Op::Undef);
                        }
                    }
                }
                for r in roh {
                    let i = self.text(r)?;
                    f.emittiere(Op::Text(i));
                }
                f.emittiere(Op::VorlagenObjekt(gekocht.len() as u16));
            }
            Ausdruck::Template(teile) => {
                for t in teile {
                    match t {
                        TemplateTeil::Text(s) => {
                            let i = self.text(s)?;
                            f.emittiere(Op::Text(i));
                        }
                        TemplateTeil::Ausdruck(x) => {
                            self.ausdruck(f, x)?;
                            f.emittiere(Op::ZuText);
                        }
                    }
                }
                f.emittiere(Op::Vorlage(teile.len() as u16));
            }
            Ausdruck::Feld(teile) => {
                f.emittiere(Op::NeuesFeld);
                for t in teile {
                    match t {
                        FeldTeil::Loch => {
                            // J7: `[1,,3]` ist ein Feld mit einer Luecke
                            // (sec-12.2.2), nicht mit `undefined`.
                            f.emittiere(Op::Undef);
                            f.emittiere(Op::FeldAnhaengen);
                            f.emittiere(Op::FeldLoch);
                        }
                        FeldTeil::Wert(x) => {
                            self.ausdruck(f, x)?;
                            f.emittiere(Op::FeldAnhaengen);
                        }
                        FeldTeil::Streu(x) => {
                            self.ausdruck(f, x)?;
                            f.emittiere(Op::FeldStreu);
                        }
                    }
                }
            }
            Ausdruck::Gegenstand(teile) => {
                f.emittiere(Op::NeuesObjekt);
                for t in teile {
                    match t {
                        GegenstandTeil::Paar(n, x) => {
                            // J49: `SetFunctionName` fuer den Wert einer
                            // Objekteigenschaft (ES2023 13.2.5.3
                            // `PropertyDefinitionEvaluation`, Schritt 8:
                            // „If propValue is an anonymous function
                            // definition, then perform
                            // SetFunctionName(propValue, propKey)").
                            //
                            // **DIESE REGEL FEHLTE BISHER VOLLSTAENDIG.**
                            //
                            // GEMESSEN (node 22), **vierundzwanzig** Zeilen —
                            // **achtundzwanzig** waren im Motor falsch:
                            //   `var a = {n: function(){}}; a.n.name`      -> `"n"`
                            //   `var c = {n: class {}}; c.n.name`          -> `"n"`
                            //   `var e = {n: () => 1}; e.n.name`            -> `"n"`
                            //   `var b = {n: function inn(){}}`             -> `"inn"`
                            //   `var g = {1: function(){}}; g[1].name`      -> `"1"`
                            //   `var h = {1.5: function(){}}`               -> `"1.5"`
                            //   `var i = {0: function(){}}`                 -> `"0"`
                            //   `var l = {[k]: function(){}}` (k = 'q')     -> `"q"`
                            //   `var m = {[s]: function(){}}` (s = Symbol('r')) -> `"[r]"`
                            //   `var n2 = {[7]: function(){}}`              -> `"7"`
                            //   `var u = {k: {n: function(){}}}`           -> `"n"`
                            //   `var v = {m = ({n: function(){}})}`        -> `"n"`
                            //   `var w = {n: function(){}, n: function(){}}`-> `"n"`
                            //   `var x = {'': function(){}}`                -> `""`
                            //   `var f = {n: (0, function(){})}`            -> `""` (Komma!)
                            //   `var k2 = {[function(){}]: function(){}}`    -> **kein** Name
                            //                              (Schluessel ist ein Objekt)
                            //   Zwei berechnete Schluessel: **einmal**
                            //   ausgewertet (`zaehler` = 2, nicht 4).
                            let namen = Self::ist_anonyme_funktion(x);
                            self.ausdruck(f, x)?;
                            match n {
                                EigenschaftName::Text(t) if namen && !t.is_empty() => {
                                    let ni = f.name_index(t);
                                    f.emittiere(Op::NamenSetzen(ni));
                                    self.def_eigenschaft(f, n)?;
                                }
                                EigenschaftName::Zahl(z) if namen => {
                                    let text = crate::ops::zahl_schluessel(*z);
                                    let ni = f.name_index(&text);
                                    f.emittiere(Op::NamenSetzen(ni));
                                    self.def_eigenschaft(f, n)?;
                                }
                                // Der berechnete Schluessel liegt **oben**,
                                // der Wert darunter — genau das liest
                                // `NamenSetzenBerechnet`, und es wird
                                // **vor** dem Speichern getan, damit der
                                // Schluessel-Ausdruck **einmal** laeuft.
                                EigenschaftName::Berechnet(_) if namen => {
                                    self.eigenschaft_name(f, n, false)?;
                                    f.emittiere(Op::NamenSetzenBerechnet);
                                    f.emittiere(Op::DefEigenschaftNamen);
                                }
                                _ => {
                                    self.def_eigenschaft(f, n)?;
                                }
                            }
                        }
                        GegenstandTeil::Kurz(n) => {
                            let ni = f.name_index(n);
                            f.emittiere(Op::LeseVar(ni));
                            let i = self.text(n)?;
                            f.emittiere(Op::Text(i));
                            f.emittiere(Op::DefEigenschaftNamen);
                        }
                        // J49: eine **Methodendefinition** traegt den
                        // Schluessel als Namen (ES2023 13.2.5.4
                        // `MethodDefinitionEvaluation`, Schritt 3), ein
                        // Accessor mit **Praefix** (`get g`, `set g`).
                        //
                        // GEMESSEN (node 22), **drei** Zeilen — alle drei
                        // waren im Motor leer:
                        //   `var y = {m(){}}; y.m.name` -> `"m"`
                        //   `var p = {get g(){ return 1 }};
                        //    Object.getOwnPropertyDescriptor(p,'g').get.name`
                        //        -> `"get g"`
                        GegenstandTeil::Methode(n, bau) => {
                            let idx = self.funktion_kompilieren(
                                bau,
                                &Self::eigenschafts_text(n),
                                None,
                                false,
                            )?;
                            f.emittiere(Op::NeuFunktion(idx));
                            // Release 15.10.2026: [[HomeObject]] for `super`.
                            f.emittiere(Op::HeimSetzen);
                            self.def_eigenschaft(f, n)?;
                        }
                        GegenstandTeil::Getter(n, bau) => {
                            let idx = self.funktion_kompilieren(
                                bau,
                                &format!("get {}", Self::eigenschafts_text(n)),
                                None,
                                false,
                            )?;
                            f.emittiere(Op::NeuFunktion(idx));
                            f.emittiere(Op::HeimSetzen);
                            self.def_zugriff(f, n, 0)?;
                        }
                        GegenstandTeil::Setter(n, bau) => {
                            let idx = self.funktion_kompilieren(
                                bau,
                                &format!("set {}", Self::eigenschafts_text(n)),
                                None,
                                false,
                            )?;
                            f.emittiere(Op::NeuFunktion(idx));
                            f.emittiere(Op::HeimSetzen);
                            self.def_zugriff(f, n, 1)?;
                        }
                        GegenstandTeil::Streu(x) => {
                            self.ausdruck(f, x)?;
                            f.emittiere(Op::StreuKopieren);
                        }
                    }
                }
            }
            Ausdruck::Funktion { name, funktion } => {
                // GEMESSEN: `(function f(){ f() })` braucht den Namen im
                // eigenen Scope — ohne das kam `ReferenceError: f`.
                match name.clone() {
                    Some(n) => {
                        let idx = self.funktion_kompilieren_mit(
                            funktion,
                            &n,
                            None,
                            false,
                            Some(n.clone()),
                        )?;
                        f.emittiere(Op::NeuFunktion(idx));
                    }
                    None => {
                        let idx = self.funktion_kompilieren(funktion, "", None, false)?;
                        f.emittiere(Op::NeuFunktion(idx));
                    }
                }
            }
            Ausdruck::Klasse {
                name,
                eltern,
                elemente,
                quelle,
            } => {
                self.klasse(
                    f,
                    name.as_deref(),
                    eltern.as_deref(),
                    elemente,
                    false,
                    // J19: Quelltext der Klasse (siehe `Ausdruck::Klasse`).
                    quelle.as_deref(),
                )?;
            }
            Ausdruck::Zugriff {
                objekt,
                name,
                optional,
            } => self.zugriff(f, objekt, name, *optional)?,
            Ausdruck::Aufruf {
                ziel,
                args,
                streu,
                optional,
            } => self.aufruf(f, ziel, args, streu, *optional)?,
            Ausdruck::Neu { ziel, args, streu } => {
                let streu_hat = streu.iter().any(|s| *s);
                if streu_hat {
                    self.args_bauen(f, args, streu)?;
                    self.ausdruck(f, ziel)?;
                    f.emittiere(Op::AufrufNeuStreu);
                } else {
                    for a in args {
                        self.ausdruck(f, a)?;
                    }
                    self.ausdruck(f, ziel)?;
                    f.emittiere(Op::AufrufNeu(args.len() as u16));
                }
            }
            Ausdruck::Zuweisung { ziel, op, wert } => self.zuweisung(f, ziel, *op, wert)?,
            Ausdruck::Update { ziel, hoch, nach } => self.update(f, ziel, *hoch, *nach)?,
            Ausdruck::Binaer { op, links, rechts } => {
                self.ausdruck(f, links)?;
                self.ausdruck(f, rechts)?;
                f.emittiere(binaer_op(*op));
            }
            Ausdruck::Unaer { op, ausdruck, nach } => self.unaer(f, *op, ausdruck, *nach)?,
            Ausdruck::Bedingt {
                bedingung,
                dann,
                sonst,
            } => {
                self.ausdruck(f, bedingung)?;
                let p1 = f.sprung(Op::SpringWennFalsch(0));
                self.ausdruck(f, dann)?;
                let p2 = f.sprung(Op::Spring(0));
                f.patch(p1, f.hier());
                self.ausdruck(f, sonst)?;
                f.patch(p2, f.hier());
            }
            Ausdruck::Logisch { op, links, rechts } => {
                self.ausdruck(f, links)?;
                match op {
                    LogischOp::Und => {
                        // GEMESSEN (A5): `SpringWennFalsch` verwirft den
                        // Wert — im Ausdruck muss er aber liegen bleiben
                        // (`false && x` ergibt `false`).
                        let p = f.sprung(Op::SpringWennFalschBehalten(0));
                        f.emittiere(Op::Pop(1));
                        self.ausdruck(f, rechts)?;
                        f.patch(p, f.hier());
                    }
                    LogischOp::Oder => {
                        let p = f.sprung(Op::SpringWennWahr(0));
                        f.emittiere(Op::Pop(1));
                        self.ausdruck(f, rechts)?;
                        f.patch(p, f.hier());
                    }
                    LogischOp::FrageFrage => {
                        let p = f.sprung(Op::SpringWennNichtNullish(0));
                        f.emittiere(Op::Pop(1));
                        self.ausdruck(f, rechts)?;
                        f.patch(p, f.hier());
                    }
                }
            }
            Ausdruck::Folge(v) => {
                for (i, x) in v.iter().enumerate() {
                    self.ausdruck(f, x)?;
                    if i + 1 < v.len() {
                        f.emittiere(Op::Pop(1));
                    }
                }
            }
            Ausdruck::Ertrag(None) => {
                f.emittiere(Op::Ertrag);
            }
            Ausdruck::Ertrag(Some(x)) => {
                self.ausdruck(f, x)?;
                f.emittiere(Op::Ertrag);
            }
            Ausdruck::ErtragStern(x) if f.asynchron && f.generator => {
                self.ertrag_stern_async(f, x)?;
            }
            Ausdruck::ErtragStern(x) => {
                self.ausdruck(f, x)?;
                f.emittiere(Op::ErtragStern);
            }
            Ausdruck::Erwartung(x) => {
                self.ausdruck(f, x)?;
                self.erwarten_emittieren(f);
            }
        }
        Ok(())
    }

    /// `name: wert` anhaengen (Schluessel als Konstante oder vom Stapel).
    /// J49: der Text eines Schluessels fuer einen **Funktionsnamen**
    /// (Objektliteral-Methode, Accessor). Berechnete Schluessel haben hier
    /// keinen Text — dafuer gibt es `Op::NamenSetzenBerechnet`.
    fn eigenschafts_text(n: &EigenschaftName) -> String {
        match n {
            crate::ast::EigenschaftName::Text(t) => t.clone(),
            crate::ast::EigenschaftName::Zahl(z) => crate::ops::zahl_schluessel(*z),
            crate::ast::EigenschaftName::Berechnet(_) => String::new(),
        }
    }

    fn def_eigenschaft(&mut self, f: &mut Fn, n: &EigenschaftName) -> Result<(), JsFehler> {
        match n {
            EigenschaftName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::DefEigenschaft(i));
            }
            _ => {
                self.eigenschaft_name(f, n, false)?;
                f.emittiere(Op::DefEigenschaftNamen);
            }
        }
        Ok(())
    }

    fn def_zugriff(&mut self, f: &mut Fn, n: &EigenschaftName, art: u8) -> Result<(), JsFehler> {
        match n {
            EigenschaftName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::DefZugriff(i, art));
            }
            _ => {
                self.eigenschaft_name(f, n, false)?;
                f.emittiere(Op::DefZugriffNamen(art));
            }
        }
        Ok(())
    }

    fn args_bauen(
        &mut self,
        f: &mut Fn,
        args: &[Ausdruck],
        streu: &[bool],
    ) -> Result<(), JsFehler> {
        f.emittiere(Op::NeuesFeld);
        for (i, a) in args.iter().enumerate() {
            self.ausdruck(f, a)?;
            if streu.get(i).copied().unwrap_or(false) {
                f.emittiere(Op::FeldStreu);
            } else {
                f.emittiere(Op::FeldAnhaengen);
            }
        }
        Ok(())
    }

    /// Release 10.10.2026: eine OPTIONALE KETTE (ES2020 13.3.9) beginnt am
    /// aeussersten Zugriff/Aufruf; jedes `?.` darin springt bei nullish mit
    /// `undefined` an ihr Ende. GEMESSEN vorher: `var n = null; n?.a.b`
    /// warf „kein Zugriff auf Undefiniert (Eigenschaft b)" (node:
    /// `undefined`) — nur das eine Glied schloss kurz.
    fn kette_beginnen(&mut self) {
        self.ketten.push(Vec::new());
    }

    fn kette_beenden(&mut self, f: &mut Fn) {
        if let Some(liste) = self.ketten.pop() {
            let z = f.hier();
            for p in liste {
                f.patch(p, z);
            }
        }
    }

    /// Der Kurzschluss: `undefined` liegt oben, Sprung ans Kettenende.
    fn kurzschluss(&mut self, f: &mut Fn) {
        let p = f.sprung(Op::Spring(0));
        match self.ketten.last_mut() {
            Some(l) => l.push(p),
            // Ohne offene Kette (darf nicht vorkommen): direkt dahinter.
            None => {
                let z = f.hier();
                f.patch(p, z);
            }
        }
    }

    /// Das Objekt eines Kettenglieds: ein weiterer Zugriff/Aufruf gehoert
    /// zur SELBEN Kette, alles andere ist ein gewoehnlicher Ausdruck.
    fn kettenglied(&mut self, f: &mut Fn, e: &Ausdruck) -> Result<(), JsFehler> {
        match e {
            Ausdruck::Zugriff {
                objekt,
                name,
                optional,
            } => self.zugriff_glied(f, objekt, name, *optional),
            Ausdruck::Aufruf {
                ziel,
                args,
                streu,
                optional,
            } => self.aufruf_glied(f, ziel, args, streu, *optional),
            _ => self.ausdruck(f, e),
        }
    }

    /// `a.b`, `a[i]`, `a?.b` — der Anfang einer Kette.
    fn zugriff(
        &mut self,
        f: &mut Fn,
        objekt: &Ausdruck,
        name: &ZugriffName,
        optional: bool,
    ) -> Result<(), JsFehler> {
        self.kette_beginnen();
        let r = self.zugriff_glied(f, objekt, name, optional);
        self.kette_beenden(f);
        r
    }

    /// `a.b`, `a[i]`, `a?.b` als Glied einer Kette.
    fn zugriff_glied(
        &mut self,
        f: &mut Fn,
        objekt: &Ausdruck,
        name: &ZugriffName,
        optional: bool,
    ) -> Result<(), JsFehler> {
        // `super.name` laeuft nicht ueber `this`, sondern ab Heimat-Prototyp.
        if let Ausdruck::Super = objekt {
            return match name {
                ZugriffName::Text(t) => {
                    let i = self.text(t)?;
                    f.emittiere(Op::SuperLese(i));
                    Ok(())
                }
                ZugriffName::Ausdruck(e) => {
                    self.ausdruck(f, e)?;
                    f.emittiere(Op::SuperLeseDynamisch);
                    Ok(())
                }
                // J11: private Felder laufen ueber denselben Weg wie ein
                // Textname — der Name traegt sein `#` mit (`this.#x` liest
                // die Eigenschaft `"#x"`).
                ZugriffName::Privat(n) => {
                    let i = self.text(n)?;
                    f.emittiere(Op::SuperLese(i));
                    Ok(())
                }
            };
        }
        self.kettenglied(f, objekt)?;
        if optional {
            let p = f.sprung(Op::SpringWennNichtNullish(0));
            f.emittiere(Op::Pop(1));
            f.emittiere(Op::Undef);
            self.kurzschluss(f);
            f.patch(p, f.hier());
        }
        self.lesen_durch(f, name)?;
        Ok(())
    }

    /// Lesezugriff mit bereits auf dem Stapel liegendem Objekt.
    fn lesen_durch(&mut self, f: &mut Fn, name: &ZugriffName) -> Result<(), JsFehler> {
        match name {
            ZugriffName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::LeseEigenschaft(i));
            }
            ZugriffName::Ausdruck(e) => {
                self.ausdruck(f, e)?;
                f.emittiere(Op::LeseIndex);
            }
            // J11: privater Feldzugriff wie ein Textname (Schluessel mit `#`).
            ZugriffName::Privat(n) => {
                let i = self.text(n)?;
                f.emittiere(Op::LeseEigenschaft(i));
            }
        }
        Ok(())
    }

    /// Aufruf: `f(...)`, `a.b(...)`, `a?.b(...)`, `super(...)` — der
    /// Anfang einer Kette.
    fn aufruf(
        &mut self,
        f: &mut Fn,
        ziel: &Ausdruck,
        args: &[Ausdruck],
        streu: &[bool],
        optional: bool,
    ) -> Result<(), JsFehler> {
        self.kette_beginnen();
        let r = self.aufruf_glied(f, ziel, args, streu, optional);
        self.kette_beenden(f);
        r
    }

    /// Ein Aufruf als Glied einer Kette.
    fn aufruf_glied(
        &mut self,
        f: &mut Fn,
        ziel: &Ausdruck,
        args: &[Ausdruck],
        streu: &[bool],
        optional: bool,
    ) -> Result<(), JsFehler> {
        let streu_hat = streu.iter().any(|s| *s);
        // `super(...)` als Ziel.
        if matches!(ziel, Ausdruck::Super) {
            for a in args {
                self.ausdruck(f, a)?;
            }
            if streu_hat {
                f.emittiere(Op::AufrufSuperStreu);
            } else {
                f.emittiere(Op::AufrufSuper(args.len() as u16));
            }
            // Release 10.10.2026: die Instanzfelder direkt nach `super()`
            // (der Wert von `super()` — `this` — bleibt liegen, die
            // Rueckgabe des Initialisierers faellt weg).
            if let Some(ff) = self.felder_nach_super {
                f.emittiere(Op::FelderStart(ff));
                f.emittiere(Op::Pop(1));
                self.super_felder_gesetzt = true;
            }
            return Ok(());
        }
        // `super.m(...)`: Suche ab Heimat-Prototyp, `this` aus dem Rahmen.
        if let Ausdruck::Zugriff {
            objekt: super_obj,
            name,
            optional: false,
        } = ziel
            && matches!(**super_obj, Ausdruck::Super)
        {
            for a in args {
                self.ausdruck(f, a)?;
            }
            self.lesen_durch_nur_schluessel(f, name)?;
            if streu_hat {
                f.emittiere(Op::AufrufSuperStreu);
            } else {
                f.emittiere(Op::AufrufSuperMethode(args.len() as u16));
            }
            return Ok(());
        }
        // Methodenaufruf: erst das Objekt, dann die Argumente.
        if let Ausdruck::Zugriff {
            objekt,
            name,
            optional: op_opt,
        } = ziel
        {
            self.kettenglied(f, objekt)?;
            // Release 10.10.2026: `a?.b(…)` prueft das OBJEKT `a` (und nicht
            // erst den Funktionswert `a.b`). GEMESSEN: `var w = {};
            // w.t?.cp("n")` warf „kein Zugriff auf Undefiniert (Eigenschaft
            // cp)" statt `undefined` zu liefern — Next.js
            // (`window.trustedTypes?.createPolicy(…)`) brach auf bbc.com die
            // Hydration ab. `a.b?.(…)` prueft weiter den Funktionswert,
            // `a?.b?.(…)` beides.
            if *op_opt {
                let p = f.sprung(Op::SpringWennNichtNullish(0));
                f.emittiere(Op::Pop(1));
                f.emittiere(Op::Undef);
                self.kurzschluss(f);
                f.patch(p, f.hier());
            }
            return self.methoden_aufruf(f, name, args, streu, optional);
        }
        self.normaler_aufruf(f, ziel, args, streu, optional)
    }

    /// Release 10.10.2026: der Methodenaufruf `o.name(…)` mit dem Objekt
    /// bereits auf dem Stapel (`optional`: `o.name?.(…)`).
    fn methoden_aufruf(
        &mut self,
        f: &mut Fn,
        name: &ZugriffName,
        args: &[Ausdruck],
        streu: &[bool],
        optional: bool,
    ) -> Result<(), JsFehler> {
        let streu_hat = streu.iter().any(|s| *s);
        let opt = optional;
        {
            // Release 10.10.2026: auch mit Spread pruefen wir den
            // FUNKTIONSWERT (vorher das Objekt, und die Argumente liefen
            // im Kurzschluss trotzdem); der Kurzschluss geht ans KETTENENDE.
            if opt {
                // `o.f?.(…)` prueft den FUNKTIONSWERT, nicht das Objekt
                // (GEMESSEN: `o = {f: null}` warf „Aufrufziel ist keine
                // Funktion" statt `undefined` zu liefern).
                let t = f.temp()?;
                let tn = f.temp()?;
                f.emittiere(Op::TempSet(t));
                self.lesen_durch_nur_schluessel(f, name)?;
                f.emittiere(Op::TempSet(tn));
                f.emittiere(Op::TempGet(t));
                // `LeseZielTemp` holt den Schluessel aus dem Temp — der
                // Schluessel darf ALSO nicht noch einmal auf dem Stapel
                // liegen (GEMESSEN: sonst war das „Objekt" der Schluessel
                // und der Wert `undefined`).
                f.emittiere(Op::LeseZielTemp(tn, 0));
                let p = f.sprung(Op::SpringWennNichtNullish(0));
                // Nullish-Pfad (Fall-through): nur `[o, wert]` aufraeumen —
                // die Argumente sind hier noch gar nicht ausgewertet worden,
                // ein `Pop` dafuer entfernte einen Wert des AUFRUFERS
                // (GEMESSEN: „Operandenstapel zu kurz").
                f.emittiere(Op::Pop(2));
                f.emittiere(Op::Undef);
                self.kurzschluss(f);
                let aufruf_pos = f.hier();
                f.patch(p, aufruf_pos);
                // Der gepruefte Funktionswert liegt noch auf dem Stapel —
                // er muss weg, sonst stoert er den Aufruf (GEMESSEN).
                f.emittiere(Op::Pop(1));
                // Das Objekt liegt noch auf dem Stapel (der Pop hat nur den
                // geprueften WERT entfernt) — ein zweites `TempGet(t)` legte
                // eine Kopie darueber und der Aufruf schluckte sie nicht
                // (GEMESSEN: Fehler schon bei `console.log(o.f?.())`).
                if streu_hat {
                    self.args_bauen(f, args, streu)?;
                    f.emittiere(Op::TempGet(tn));
                    f.emittiere(Op::AufrufMethodeStreu);
                } else {
                    for a in args {
                        self.ausdruck(f, a)?;
                    }
                    f.emittiere(Op::TempGet(tn));
                    f.emittiere(Op::AufrufMethode(args.len() as u16));
                }
                return Ok(());
            }
            if streu_hat {
                self.args_bauen(f, args, streu)?;
                self.lesen_durch_nur_schluessel(f, name)?;
                f.emittiere(Op::AufrufMethodeStreu);
            } else {
                for a in args {
                    self.ausdruck(f, a)?;
                }
                // Release 09.10.2026: fester Name -> `AufrufMethodeText`.
                if let ZugriffName::Text(t) = name {
                    let i = self.text(t)?;
                    f.emittiere(Op::AufrufMethodeText(args.len() as u16, i));
                } else {
                    self.lesen_durch_nur_schluessel(f, name)?;
                    f.emittiere(Op::AufrufMethode(args.len() as u16));
                }
            }
            return Ok(());
        }
    }

    /// Release 10.10.2026: `f(…)` / `f?.(…)` (abgetrennt von `aufruf`).
    fn normaler_aufruf(
        &mut self,
        f: &mut Fn,
        ziel: &Ausdruck,
        args: &[Ausdruck],
        streu: &[bool],
        optional: bool,
    ) -> Result<(), JsFehler> {
        let streu_hat = streu.iter().any(|s| *s);
        // Normaler Aufruf: Argumente, dann `this` (undefined), dann das Ziel.
        //
        // J5: bei einem Spread wird das **gesamte** Argumentfeld gebaut
        // (`args_bauen`), nicht jedes Argument einzeln auf den Stapel.
        // GEMESSEN: `f(...[1,2])` lief nur, WEIL der Befehl `AufrufStreu`
        // das erste Argument als Feld las — es war das Array. Mit zwei
        // Spreads (`f(...[1], ...[2,3])`) las er danach `this` und das
        // Ziel an den falschen Stellen: `sum(...n)` bekam 2 statt 3
        // Argumente (`tests/t262/faelle/fun-spread.js`).
        if streu_hat {
            self.args_bauen(f, args, streu)?;
            f.emittiere(Op::Undef);
            self.ausdruck(f, ziel)?;
            f.emittiere(Op::AufrufStreu);
            return Ok(());
        }
        for a in args {
            self.ausdruck(f, a)?;
        }
        f.emittiere(Op::Undef);
        if optional {
            // GEMESSEN: `f?.()` prueft das ZIEL, nicht den `this`-Platz.
            // Vorher stand `Undef` (this) oben und der Test war nie
            // wahrheitsfaellig — `setTimeout?.(f, 0)` rief NICHTS auf
            // (p2.js Z18: „mikro"/„timeout" fehlten).
            // Reihenfolge: Argumente, Ziel pruefen, Ziel in den Temp,
            // `this`, Ziel, Aufruf.
            self.ausdruck(f, ziel)?;
            // `SpringWennNichtNullish` springt, wenn der Wert NICHT nullish
            // ist — das Sprungziel muss der AUFRUF sein.
            let p = f.sprung(Op::SpringWennNichtNullish(0));
            // Nullish-Pfad (Fall-through): `[args, this, ziel]` aufraeumen
            // (auch der `this`-Platz — sonst blieb ein Wert zu viel liegen
            // und der umgebende Aufruf sah eine falsche Argumentzahl,
            // GEMESSEN: „Aufrufziel ist keine Funktion").
            f.emittiere(Op::Pop(args.len() as u8 + 2));
            f.emittiere(Op::Undef);
            // Release 10.10.2026: ans Kettenende (`f?.().x` -> undefined).
            self.kurzschluss(f);
            // GEMESSEN: `f.hier()` muss HIER stehen (nach dem Nullish-Pfad)
            // — vorher zeigte es auf dessen erstes `Pop` und der Aufruf
            // wurde nie erreicht (`f?.(x)` rief nichts auf).
            let aufruf_pos = f.hier();
            f.patch(p, aufruf_pos);
            // Aufruf-Pfad: ZIEL kurz in den Temp (die Argumente und der
            // `this`-Platz liegen schon darunter — ein ZWEITES `Undef`
            // verdrehte den Aufruf um eine Stelle, GEMESSEN: das erste
            // Argument kam als `undefined` an und das Ergebnis war NaN).
            let t = f.temp()?;
            f.emittiere(Op::TempSet(t));
            f.emittiere(Op::TempGet(t));
            if streu_hat {
                f.emittiere(Op::AufrufStreu);
            } else {
                f.emittiere(Op::Aufruf(args.len() as u16));
            }
            return Ok(());
        }
        self.ausdruck(f, ziel)?;
        if streu_hat {
            f.emittiere(Op::AufrufStreu);
        } else {
            // Release 09.10.2026: nur `eval(...)` mit dem Bezeichner ist direkt.
            if matches!(ziel, Ausdruck::Name(n) if n == "eval") {
                f.emittiere(Op::EvalDirekt);
            }
            f.emittiere(Op::Aufruf(args.len() as u16));
        }
        Ok(())
    }

    /// Nur den Schluessel eines Zugriffs auf den Stapel legen (fuer
    /// `AufrufMethode`).
    fn lesen_durch_nur_schluessel(
        &mut self,
        f: &mut Fn,
        name: &ZugriffName,
    ) -> Result<(), JsFehler> {
        match name {
            ZugriffName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::Text(i));
            }
            ZugriffName::Ausdruck(e) => self.ausdruck(f, e)?,
            // J11: privater Feldzugriff wie ein Textname (Schluessel mit `#`).
            ZugriffName::Privat(n) => {
                let i = self.text(n)?;
                f.emittiere(Op::Text(i));
            }
        }
        Ok(())
    }

    fn zuweisung(
        &mut self,
        f: &mut Fn,
        ziel: &ZuweisungsZiel,
        op: ZuweisungsOp,
        wert: &Ausdruck,
    ) -> Result<(), JsFehler> {
        match ziel {
            ZuweisungsZiel::Name(n) => {
                let ni = f.name_index(n);
                match op {
                    ZuweisungsOp::Gleich => {
                        self.ausdruck(f, wert)?;
                        // J35: `SetFunctionName` bei einer Zuweisung an einen
                        // **Bezeichner** (ES2023 13.3.1 `AssignmentExpression`
                        // Schritt 1.e.i — der Infotext des test262-Falls
                        // `fn-name-arrow.js` zitiert ihn woertlich).
                        //
                        // GEMESSEN (node 22), **zehn** Proben, vorher alle
                        // ohne Namen:
                        //   `var f; f = function(){}`  -> `f.name` = `"f"`
                        //   `var f; f = () => {}`       -> `"f"`
                        //   `var f; f = class{}`        -> `"f"`
                        //   `var f; f = function*(){}`  -> `"f"`
                        //   `var f; f = async function(){}` -> `"f"`
                        //   `f = function inn(){}`      -> `"inn"` (eigener Name)
                        //   `f = function(){}; f = function(){}` -> `"f"`
                        //   `f = class Inner{}`         -> `"Inner"`
                        //   `let f; f = function(){}`    -> `"f"`
                        //   `var x; x = function(){}; return x.name` (in einer
                        //     Funktion) -> `"x"`
                        //
                        // **NICHT** bei einer **Member**-Zuweisung — die
                        // naechste Zeile `ZuweisungsZiel::Zugriff` ruft das
                        // nicht auf. GEMESSEN: `var o = {}; o.p =
                        // function(){}` -> `o.p.name` = `""` (node).
                        self.namen_setzen(f, wert, n)?;
                        f.emittiere(Op::ZuweisVar(ni));
                    }
                    // GEMESSEN (A5): `a &&= 2` war als `a = !(a && 2)`
                    // gebaut (Ergebnis: `false`) und `a &= 1`/`a |= 1`
                    // nahmen den Kurzschlusszweig. Die logischen Formen
                    // heissen im AST `UndUnd`/`OderOder`/`FrageFrage` —
                    // `Und`/`Oder` sind die bitweisen.
                    ZuweisungsOp::UndUnd | ZuweisungsOp::OderOder | ZuweisungsOp::FrageFrage => {
                        f.emittiere(Op::LeseVar(ni));
                        let p = match op {
                            ZuweisungsOp::UndUnd => f.sprung(Op::SpringWennFalschBehalten(0)),
                            ZuweisungsOp::OderOder => f.sprung(Op::SpringWennWahr(0)),
                            _ => f.sprung(Op::SpringWennNichtNullish(0)),
                        };
                        f.emittiere(Op::Pop(1));
                        self.ausdruck(f, wert)?;
                        f.emittiere(Op::ZuweisVar(ni));
                        // Auch im Kurzschlussfall ist der gelesene Wert
                        // das Ergebnis der Zuweisung; erst die Anweisung
                        // darf ihn mit ihrem normalen Pop verwerfen.
                        f.patch(p, f.hier());
                    }
                    _ => {
                        // Zusammengesetzt: `x op= w`.
                        f.emittiere(Op::LeseVar(ni));
                        self.ausdruck(f, wert)?;
                        f.emittiere(zuweisung_op(op));
                        f.emittiere(Op::ZuweisVar(ni));
                    }
                }
                Ok(())
            }
            ZuweisungsZiel::Zugriff {
                objekt,
                name,
                optional,
            } => self.zuweisung_zugriff(f, objekt, name, *optional, op, wert),
            ZuweisungsZiel::Muster(m) => {
                if op != ZuweisungsOp::Gleich {
                    return Err(JsFehler::Syntax {
                        zeile: 0,
                        spalte: 0,
                        nachricht: "nur `=` ist bei Muster-Zuweisung erlaubt".to_string(),
                    });
                }
                self.ausdruck(f, wert)?;
                self.muster_zuweisen(f, m)
            }
        }
    }

    fn zuweisung_zugriff(
        &mut self,
        f: &mut Fn,
        objekt: &Ausdruck,
        name: &ZugriffName,
        optional: bool,
        op: ZuweisungsOp,
        wert: &Ausdruck,
    ) -> Result<(), JsFehler> {
        self.ausdruck(f, objekt)?;
        if optional {
            let p = f.sprung(Op::SpringWennNichtNullish(0));
            self.ausdruck(f, wert)?;
            f.emittiere(Op::Pop(1));
            f.emittiere(Op::Pop(1));
            f.emittiere(Op::Undef);
            let p2 = f.sprung(Op::Spring(0));
            f.patch(p, f.hier());
            if op == ZuweisungsOp::Gleich {
                self.schreiben_durch(f, name, wert)?;
            } else {
                return Err(JsFehler::Syntax {
                    zeile: 0,
                    spalte: 0,
                    nachricht: "`?.` mit zusammengesetzter Zuweisung".to_string(),
                });
            }
            f.patch(p2, f.hier());
            return Ok(());
        }
        if op == ZuweisungsOp::Gleich {
            self.schreiben_durch(f, name, wert)?;
            return Ok(());
        }
        // Zusammengesetzt: altes Ziel sichern (Temp), rechnen, zurueckschreiben.
        //
        // GEMESSEN: `LeseZielTemp`/`SchreibeZielTemp` lesen und schreiben in
        // der Form [.., Objekt, Wert] (Schluessel aus dem Temp). Der alte
        // Aufbau (Objekt, Wert, Schluessel) schob den Schluessel auf den
        // Wert — `o.n += 4` schrieb dann den Schluessel „n" als Zahl.
        let tb = f.temp()?;
        let tn = f.temp()?;
        let t_neu = f.temp()?;
        f.emittiere(Op::TempSet(tb));
        self.tastatur(f, name, Some(tn))?;
        f.emittiere(Op::TempGet(tb));
        f.emittiere(Op::LeseZielTemp(tn, 0));
        if matches!(op, ZuweisungsOp::UndUnd | ZuweisungsOp::OderOder | ZuweisungsOp::FrageFrage) {
            let skip = match op {
                ZuweisungsOp::UndUnd => f.sprung(Op::SpringWennFalschBehalten(0)),
                ZuweisungsOp::OderOder => f.sprung(Op::SpringWennWahr(0)),
                _ => f.sprung(Op::SpringWennNichtNullish(0)),
            };
            f.emittiere(Op::Pop(1));
            self.ausdruck(f, wert)?;
            f.emittiere(Op::SchreibeZielTemp(tn, 0));
            f.patch(skip, f.hier());
            // Beide Wege enthalten [Basis, Ergebnis]; nur Ergebnis bleibt.
            f.emittiere(Op::TempSet(t_neu));
            f.emittiere(Op::Pop(1));
            f.emittiere(Op::TempGet(t_neu));
            f.emittiere(Op::TempLoesche(tb));
            f.emittiere(Op::TempLoesche(tn));
            f.emittiere(Op::TempLoesche(t_neu));
            return Ok(());
        }
        self.ausdruck(f, wert)?;
        f.emittiere(zuweisung_op(op));
        f.emittiere(Op::TempSet(t_neu));
        f.emittiere(Op::Pop(1));
        f.emittiere(Op::TempGet(tb));
        f.emittiere(Op::TempGet(t_neu));
        f.emittiere(Op::SchreibeZielTemp(tn, 0));
        // GEMESSEN: `SchreibeZielTemp` laesst das OBJEKT liegen (es poppt nur
        // den Wert und schiebt ihn wieder). Ohne `Pop(2)` blieb das Objekt als
        // zusaetzliches Argument auf dem Stapel — der Aufruf Basis/Argumente
        // war dann um eins verschoben („Aufrufziel ist keine Funktion").
        f.emittiere(Op::Pop(2));
        f.emittiere(Op::TempGet(t_neu));
        Ok(())
    }

    /// Schluessel/Name als GVal in den Temp `t` legen ( fuer `name` als Text,
    /// sonst als Wert).
    /// Schluessel/Namen als GVal auf den Stapel — optional in einen Temp.
    /// GEMESSEN: das Ziel-Temp wird übergeben; vorher stand hier fest `0`
    /// und kollidierte mit dem Objekt-Temp (`o.n += 4` las danach `undefined`).
    fn tastatur(
        &mut self,
        f: &mut Fn,
        name: &ZugriffName,
        temp: Option<u16>,
    ) -> Result<(), JsFehler> {
        match name {
            ZugriffName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::Text(i));
            }
            ZugriffName::Ausdruck(e) => self.ausdruck(f, e)?,
            ZugriffName::Privat(_) => {
                return Err(JsFehler::Syntax {
                    zeile: 0,
                    spalte: 0,
                    nachricht: "private Felder werden nicht unterstuetzt".to_string(),
                });
            }
        }
        if let Some(t) = temp {
            f.emittiere(Op::TempSet(t));
        }
        Ok(())
    }

    fn lesen_durch_tastatur(&mut self, f: &mut Fn, name: &ZugriffName) -> Result<(), JsFehler> {
        self.lesen_durch(f, name)
    }

    fn lesen_durch_tastatur_nur_schluessel(
        &mut self,
        f: &mut Fn,
        name: &ZugriffName,
    ) -> Result<(), JsFehler> {
        self.lesen_durch_nur_schluessel(f, name)
    }

    /// Schreiben mit bereits ausgewerteter Basis. Ein berechneter Schluessel
    /// wird einmal vor der rechten Seite ausgewertet und im Temp gesichert.
    fn schreiben_durch(
        &mut self, f: &mut Fn, name: &ZugriffName, wert: &Ausdruck,
    ) -> Result<(), JsFehler> {
        match name {
            ZugriffName::Text(t) => {
                let i = self.text(t)?;
                self.ausdruck(f, wert)?;
                f.emittiere(Op::SchreibeEigenschaft(i));
            }
            _ => {
                let key = f.temp()?;
                self.tastatur(f, name, Some(key))?;
                self.ausdruck(f, wert)?;
                f.emittiere(Op::TempGet(key));
                f.emittiere(Op::SchreibeIndex);
                f.emittiere(Op::TempLoesche(key));
            }
        }
        Ok(())
    }

    /// `x++` / `++x`.
    fn update(
        &mut self,
        f: &mut Fn,
        ziel: &ZuweisungsZiel,
        hoch: bool,
        nach: bool,
    ) -> Result<(), JsFehler> {
        let op = if hoch { Op::Inc } else { Op::Dec };
        // (wird zweimal gebraucht: Inc und ggf. der Praefix-Wert —
        //  deshalb als Kopie, `Op` ist nicht `Copy`)
        match ziel {
            ZuweisungsZiel::Name(n) => {
                let ni = f.name_index(n);
                if nach {
                    // GEMESSEN: der alte Wert muss am ENDE allein auf dem
                    // Stapel liegen. Mit einem zweiten `TempGet` VOR dem
                    // Speichern lagen zwei Werte da — als Argument verdrehte
                    // das den Aufrufbasis-Zeiger („Aufrufziel ist keine
                    // Funktion").
                    let t = f.temp()?;
                    f.emittiere(Op::LeseVar(ni));
                    f.emittiere(Op::TempSet(t));
                    f.emittiere(Op::TempGet(t));
                    f.emittiere(op);
                    f.emittiere(Op::ZuweisVar(ni));
                    f.emittiere(Op::Pop(1));
                    f.emittiere(Op::TempGet(t));
                } else {
                    f.emittiere(Op::LeseVar(ni));
                    f.emittiere(op);
                    f.emittiere(Op::ZuweisVar(ni));
                }
                Ok(())
            }
            ZuweisungsZiel::Zugriff { objekt, name, .. } => {
                // GEMESSEN: `SchreibeZielTemp(t, 0)` will den WERT ganz oben
                // und das OBJEKT einen Platz darunter (Schluessel aus `t`).
                // Vorher lag der Schluessel oben — dann wurde der Schluessel
                // als Wert geschrieben (`a[0]++` speicherte "0").
                let tb = f.temp()?;
                let tn = f.temp()?;
                self.ausdruck(f, objekt)?;
                f.emittiere(Op::TempSet(tb));
                self.lesen_durch_nur_schluessel(f, name)?;
                f.emittiere(Op::TempSet(tn));
                // GEMESSEN: `Inc`/`Dec` wirken auf den STAPEL-OBERSTEN Wert,
                // der Schreibweg `SchreibeZielTemp(t, 0)` will die Form
                // [Objekt, Wert] — und LAESST das Objekt liegen. Deshalb:
                //   [o] lesen (Objekt bleibt) -> Inc -> schreiben -> Pop(2)
                // Danach liegt nur der Ausdruckswert da (alt bei `x++`,
                // alt+1 bei `++x`).
                let t_alt = f.temp()?;
                f.emittiere(Op::TempGet(tb));
                f.emittiere(Op::LeseZielTemp(tn, 0));
                f.emittiere(Op::TempSet(t_alt));
                f.emittiere(Op::TempGet(t_alt));
                f.emittiere(op.clone());
                f.emittiere(Op::SchreibeZielTemp(tn, 0));
                f.emittiere(Op::Pop(2));
                f.emittiere(Op::TempGet(t_alt));
                if !nach {
                    f.emittiere(op);
                }
                Ok(())
            }
            ZuweisungsZiel::Muster(_) => Err(JsFehler::Syntax {
                zeile: 0,
                spalte: 0,
                nachricht: "Update auf Muster".to_string(),
            }),
        }
    }

    fn unaer(&mut self, f: &mut Fn, op: UnaerOp, e: &Ausdruck, nach: bool) -> Result<(), JsFehler> {
        let _ = nach;
        match op {
            UnaerOp::Typeof => {
                if let Ausdruck::Name(n) = e {
                    let ni = f.name_index(n);
                    f.emittiere(Op::TypVonName(ni));
                    return Ok(());
                }
                self.ausdruck(f, e)?;
                f.emittiere(Op::TypVon);
            }
            UnaerOp::Delete => match e {
                Ausdruck::Zugriff {
                    objekt,
                    name,
                    optional: false,
                } => {
                    self.ausdruck(f, objekt)?;
                    match name {
                        ZugriffName::Text(t) => {
                            let i = self.text(t)?;
                            f.emittiere(Op::LoescheEigenschaft(i));
                        }
                        ZugriffName::Ausdruck(x) => {
                            self.ausdruck(f, x)?;
                            f.emittiere(Op::LoescheIndex);
                        }
                        ZugriffName::Privat(_) => {
                            return Err(JsFehler::Syntax {
                                zeile: 0,
                                spalte: 0,
                                nachricht: "private Felder werden nicht unterstuetzt".to_string(),
                            });
                        }
                    }
                }
                _ => {
                    // W13 (JS, 05.10.2026): `delete <Name>` ist **kein**
                    // Wertezugriff. Der alte Weg (`ausdruck; Pop; Wahr`)
                    // war doppelt falsch: er **wertete** den Operanden und
                    // warf damit fuer einen nicht deklarierten Namen den
                    // ReferenceError, den ES2023 13.5.1.1 Schritt 3/6 gerade
                    // verbietet („If IsUnresolvableReference(ref) is true,
                    // return true"), und er konnte die Bindung gar nicht
                    // loeschen. GEMESSEN vorher:
                    //   `String(delete nie_dagewesen)` -> ReferenceError
                    //   `var x=5; delete x; typeof x` -> `number`
                    //   `let y=5; delete y`            -> `true` (Soll `false`)
                    if let Ausdruck::Name(n) = e {
                        let ni = f.name_index(n);
                        f.emittiere(Op::LoescheName(ni));
                    } else {
                        self.ausdruck(f, e)?;
                        f.emittiere(Op::Pop(1));
                        f.emittiere(Op::Wahr);
                    }
                }
            },
            UnaerOp::Ruf => {
                // GEMESSEN (A5): `!` wurde als `e.call(undefined)` gebaut —
                // der AST-Knoten `UnaerOp::Ruf` entsteht im Parser NUR fuer
                // das logische Nicht (parse.rs), der Aufruf-Zweig war Altlast.
                // Folge war: `console.log(!true)` las `.call` von `true` und
                // endete mit „Aufrufziel ist keine Funktion". Es gab in der
                // ganzen VM kein einziges funktionierendes `!`.
                self.ausdruck(f, e)?;
                f.emittiere(Op::LogischNicht);
            }
            UnaerOp::PrePlusPlus | UnaerOp::PreMinusMinus => {
                // Der Parser baut Praefix-`++` als `Ausdruck::Update` (mit
                // Ziel). Hier bleibt nur der Fehlerfall (z. B. `++1`).
                return Err(JsFehler::Syntax {
                    zeile: 0,
                    spalte: 0,
                    nachricht: "ungueltiges ++/-- -Ziel".to_string(),
                });
            }
            _ => {
                self.ausdruck(f, e)?;
                f.emittiere(match op {
                    UnaerOp::Minus => Op::Negativ,
                    UnaerOp::Plus => Op::Positiv,
                    UnaerOp::Tilde => Op::BitNicht,
                    UnaerOp::Void => Op::LeerWert,
                    // `Ruf` ist oben behandelt; fuer alle uebrigen
                    // vorbereiteten Formen ist `+` die harmlose Wahl.
                    _ => Op::Positiv,
                });
            }
        }
        Ok(())
    }

    /// Muster-ZUWEISUNG (`[a,b] = arr`, `({a} = o)`).
    ///
    /// Der Quellwert liegt auf dem Stapel. Reihenfolge wie in JS: erst alle
    /// Zielreferenzen sammeln, dann je Blatt lesen und schreiben. Weil jede
    /// Blattzuweisung ihren Wert wieder wegwirft, liegt der Quellwert immer
    /// oben — die `Pick`-Tiefe ist damit konstant 0.
    ///
    /// GEMESSEN/Begruendung: `Muster` kann (anders als `ZuweisungsZiel`)
    /// keine Member-Ziele enthalten, also ist jedes Blatt ein Name.
    fn muster_zuweisen(&mut self, f: &mut Fn, m: &Muster) -> Result<(), JsFehler> {
        let mut blaetter: Vec<Blatt> = Vec::new();
        self.blatt_sammeln(f, m, &mut blaetter)?;
        // J34: dasselbe wie in `muster_binden` — **vor** dem Lesen, weil
        // dort der Quellwert in Temp `key` wandert und die Blaetter dann
        // nur noch ueber `key` lesen. Ohne diese Zeile blieb der
        // Zuweisungsweg (`[...a] = g()` als **Anweisung**) beim Indexweg
        // und lieferte `undefined` bzw. `TypeError` — GEMESSEN als zwei
        // rote Tests in `j27_ecke.rs`, waehrend die Deklaration
        // (`const [...a] = g()`) schon stimmte.
        //
        // Die Regel steht bewusst an **beiden** Musterwegen und nicht in
        // einem gemeinsamen Helfer: die beiden Wege teilen sich nur den
        // `Muster`, nicht den Bytecode (das eine liest per `LeseIndex`, das
        // andere per `Blatt`).
        // Release 10.10.2026: ein FELD-Muster nimmt die Feld-Quelle (siehe
        // `Op::MusterQuelleFeld`), ein Objektmuster die allgemeine.
        if matches!(m, Muster::Feld(_)) {
            f.emittiere(Op::MusterQuelleFeld);
        } else {
            f.emittiere(Op::MusterQuelle);
        }
        self.blaetter_zuweisen(f, &blaetter)
    }

    /// Blaetter eines Musters sammeln (flach, Reihenfolge = Quellreihenfolge).
    fn blatt_sammeln(
        &mut self,
        f: &mut Fn,
        m: &Muster,
        out: &mut Vec<Blatt>,
    ) -> Result<(), JsFehler> {
        match m {
            // Release 10.10.2026: ein Member-Ziel als ganzes Muster (ueber
            // einen Rest oder einen Standard erreicht) — der Wert ist die
            // Quelle selbst.
            Muster::Ziel(_) => out.push(Blatt {
                name: u32::MAX,
                key: 0,
                lesen: LesArt::Var,
                standard: None,
                muster: Some(m.clone()),
                muster_aus: 0,
            }),
            Muster::Name(n) => {
                let ni = f.name_index(n);
                out.push(Blatt {
                    name: ni,
                    key: 0,
                    lesen: LesArt::Var,
                    standard: None,
                    muster: None,
                    muster_aus: 0,
                });
            }
            Muster::Standard(inner, _) => self.blatt_sammeln(f, inner, out)?,
            Muster::Rest(inner) => {
                if let Muster::Name(n) = &**inner {
                    let ni = f.name_index(n);
                    out.push(Blatt {
                        name: ni,
                        key: 0,
                        lesen: LesArt::Rest,
                        standard: None,
                        muster: None,
                        muster_aus: 0,
                    });
                } else {
                    return Err(JsFehler::Syntax {
                        zeile: 0,
                        spalte: 0,
                        nachricht: "ungueltiges Rest-Ziel".to_string(),
                    });
                }
            }
            Muster::Feld(teile) => {
                for (i, t) in teile.iter().enumerate() {
                    // Release 10.10.2026: eine Luecke ist kein Blatt (siehe
                    // `LOCH`) — vorher schrieb `[, a] = q` eine Variable.
                    if ist_loch(&t.muster) {
                        continue;
                    }
                    let key = f.temp()?;
                    match &t.muster {
                        // J34: das Ziel **mitgeben**. Die erste Fassung
                        // schob ein Blatt mit `name: u32::MAX` und
                        // `muster: None` — gelesen wurde der Rest also, aber
                        // **gebunden wurde niemand**.
                        // GEMESSEN (node 22) gegen diesen Motor:
                        //   `var a; [...a] = [1,2]` -> `a` ist `[1,2]`
                        //   (hier: `TypeError`)
                        //   `var a; [...a] = new Set([1,2])` -> `a` ist `[1,2]`
                        //   (hier: `TypeError`)
                        // Der Objekt-Rest `({x, ...r} = o)` funktionierte,
                        // weil dort `LesArt::Rest` den **Namen** mitbringt —
                        // die beiden Restzweige waren nicht gleich.
                        Muster::Rest(inner) => out.push(Blatt {
                            name: u32::MAX,
                            key,
                            lesen: LesArt::FeldRest(i as u32),
                            standard: None,
                            muster: Some((**inner).clone()),
                            muster_aus: 0,
                        }),
                        _ => match &t.muster {
                            Muster::Name(n) => {
                                let ni = f.name_index(n);
                                out.push(Blatt {
                                    name: ni,
                                    key,
                                    lesen: LesArt::Index(i as u32),
                                    standard: t.standard.as_deref().cloned(),
                                    muster: None,
                                    muster_aus: 0,
                                });
                            }
                            _ => {
                                // Verschachteltes Muster.
                                out.push(Blatt {
                                    name: u32::MAX,
                                    key,
                                    lesen: LesArt::Index(i as u32),
                                    standard: t.standard.as_deref().cloned(),
                                    muster: Some(t.muster.clone()),
                                    muster_aus: 0,
                                });
                            }
                        },
                    }
                }
            }
            Muster::Gegenstand(teile) => {
                // GEMESSEN (A5, Orakel w039): `const {a, ...rest} = o`
                // lieferte ALLE Schluessel. Der Rest bekommt jetzt die
                // Liste der schon entnommenen (Schluessel-Textindizes).
                let mut raus: Vec<u32> = Vec::new();
                for g in teile {
                    // GEMESSEN: die Kurzform `{a}` ist ein ganz normaler
                    // Schluessel `a` — sie gehört in die Ausschlussliste
                    // (vorher war `!g.kurz` Bedingung, dann blieb `a` im
                    // Rest: `const {a, ...s}` -> `{a:1,b:2}` statt `{b:2}`).
                    if !matches!(g.muster, Muster::Rest(_))
                        && let crate::ast::EigenschaftName::Text(t) = &g.schluessel
                    {
                        raus.push(self.text(t)?);
                    }
                }
                for g in teile {
                    let key = f.temp()?;
                    match &g.muster {
                        Muster::Rest(inner) => {
                            // GEMESSEN: `({a, ...s} = o)` liess `s`
                            // undefiniert — das Blatt hatte `name:
                            // u16::MAX` („kein Name"), es wurde also
                            // nirgends hingesetzt. Wie beim Feld-Rest wird
                            // der innere Name gebunden.
                            let name = match &**inner {
                                Muster::Name(n) => f.name_index(n),
                                _ => {
                                    return Err(JsFehler::Syntax {
                                        zeile: 0,
                                        spalte: 0,
                                        nachricht: "ungueltiges Rest-Ziel".to_string(),
                                    });
                                }
                            };
                            let idx = f.muster_aus.len() as u32;
                            f.muster_aus.push(raus.clone());
                            out.push(Blatt {
                                name,
                                key,
                                lesen: LesArt::Rest,
                                standard: None,
                                muster: None,
                                muster_aus: idx,
                            })
                        }
                        _ => match &g.muster {
                            Muster::Name(n) => {
                                let ni = f.name_index(n);
                                out.push(Blatt {
                                    name: ni,
                                    key,
                                    lesen: LesArt::Prop(g.schluessel.clone()),
                                    standard: g.standard.as_deref().cloned(),
                                    muster: None,
                                    muster_aus: 0,
                                });
                            }
                            _ => out.push(Blatt {
                                name: u32::MAX,
                                key,
                                lesen: LesArt::Prop(g.schluessel.clone()),
                                standard: g.standard.as_deref().cloned(),
                                muster: Some(g.muster.clone()),
                                muster_aus: 0,
                            }),
                        },
                    }
                }
            }
        }
        Ok(())
    }

    /// Release 10.10.2026: den Wert oben auf dem Stapel einem Muster
    /// zuweisen; der Wert BLEIBT liegen (wie nach `ZuweisVar`).
    fn muster_wert_zuweisen(&mut self, f: &mut Fn, m: &Muster) -> Result<(), JsFehler> {
        match m {
            Muster::Name(n) => {
                let ni = f.name_index(n);
                f.emittiere(Op::ZuweisVar(ni));
            }
            Muster::Ziel(z) => self.muster_ziel_schreiben(f, z)?,
            Muster::Standard(inner, std) => {
                // `({f = function(){}} = {})` -> `f.name` = `"f"` (wie im
                // Deklarationsweg, ES2023 13.15.5.6 `NamedEvaluation`).
                if let Muster::Name(n) = inner.as_ref() {
                    self.namen_setzen(f, std, n)?;
                }
                self.standard_stack(f, std)?;
                self.muster_wert_zuweisen(f, inner)?;
            }
            Muster::Rest(inner) => self.muster_wert_zuweisen(f, inner)?,
            Muster::Feld(_) | Muster::Gegenstand(_) => {
                // Verschachteltes Muster: der Wert ist seine Quelle.
                if matches!(m, Muster::Feld(_)) {
                    f.emittiere(Op::MusterQuelleFeld);
                }
                let mut sub: Vec<Blatt> = Vec::new();
                self.blatt_sammeln(f, m, &mut sub)?;
                self.blaetter_zuweisen(f, &sub)?;
            }
        }
        Ok(())
    }

    /// Release 10.10.2026: den Wert oben auf dem Stapel in ein Member-Ziel
    /// schreiben; er bleibt liegen (dieselbe Befehlsfolge wie
    /// `schreiben_durch`, nur liegt der Wert schon da).
    fn muster_ziel_schreiben(&mut self, f: &mut Fn, ziel: &Ausdruck) -> Result<(), JsFehler> {
        let Ausdruck::Zugriff { objekt, name, .. } = ziel else {
            return Err(JsFehler::Syntax {
                zeile: 0,
                spalte: 0,
                nachricht: "ungueltiges Zuweisungsziel".to_string(),
            });
        };
        let tv = f.temp()?;
        f.emittiere(Op::TempSet(tv));
        self.ausdruck(f, objekt)?;
        match name {
            ZugriffName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::TempGet(tv));
                f.emittiere(Op::SchreibeEigenschaft(i));
            }
            _ => {
                let key = f.temp()?;
                self.tastatur(f, name, Some(key))?;
                f.emittiere(Op::TempGet(tv));
                f.emittiere(Op::TempGet(key));
                f.emittiere(Op::SchreibeIndex);
                f.emittiere(Op::TempLoesche(key));
            }
        }
        f.emittiere(Op::TempLoesche(tv));
        Ok(())
    }

    /// Phase 2: alle Blaetter zuweisen.
    fn blaetter_zuweisen(&mut self, f: &mut Fn, blaetter: &[Blatt]) -> Result<(), JsFehler> {
        for b in blaetter {
            // 1. Wert aus dem Quellobjekt lesen.
            f.emittiere(Op::Pick(0));
            match &b.lesen {
                LesArt::Var => {}
                LesArt::Rest => {
                    // Ausschlusslisten-Nummer aus Phase 1 (siehe
                    // `muster_bauen`); 0 = keine (dann bleibt der Rest eine
                    // Vollkopie — der alte, gemessene Sonderfall).
                    f.emittiere(Op::MusterRest(b.muster_aus));
                }
                LesArt::Index(i) => {
                    f.emittiere(Op::Zahl(*i as f64));
                    f.emittiere(Op::LeseIndex);
                }
                LesArt::FeldRest(i) => {
                    f.emittiere(Op::MusterRestFeld(*i));
                }
                LesArt::Prop(n) => {
                    self.eigenschaft_name_stack(f, n)?;
                    f.emittiere(Op::LeseIndex);
                }
            }
            if let Some(std) = &b.standard {
                self.standard_stack(f, std)?;
            }
            // 2. Ziel binden — der Wert bleibt dabei LIEGEN (wie bei
            // `ZuweisVar`), Schritt 3 raeumt ihn ab.
            //
            // Release 10.10.2026: vorher raeumte der verschachtelte Zweig
            // selbst ab, und Schritt 3 nahm dann die AEUSSERE Quelle mit.
            // GEMESSEN (node 22 in Klammern): `[[a], b] = [[1], 2]` -> b aus
            // `undefined`  (2); `[a = 5] = []` -> a = undefined  (5) — der
            // Standardwert eines Elements ging im verschachtelten Weg verloren.
            match &b.muster {
                Some(inner) => self.muster_wert_zuweisen(f, inner)?,
                None => {
                    if b.name != u32::MAX {
                        f.emittiere(Op::ZuweisVar(b.name));
                    }
                }
            }
            // 3. Der Wert bleibt nicht liegen (die Quelle schon).
            f.emittiere(Op::Pop(1));
        }
        Ok(())
    }

    /// Standard-Ausdruck anwenden, wenn der Wert `undefined` ist.
    fn standard_stack(&mut self, f: &mut Fn, e: &Ausdruck) -> Result<(), JsFehler> {
        let p_undef = f.sprung(Op::SpringWennUndef(0));
        let p_ueber = f.sprung(Op::Spring(0));
        let pop_pos = f.hier();
        f.emittiere(Op::Pop(1));
        self.ausdruck(f, e)?;
        let p_ende = f.sprung(Op::Spring(0));
        f.patch(p_undef, pop_pos);
        f.patch(p_ende, f.hier());
        f.patch(p_ueber, f.hier());
        Ok(())
    }

    /// Schluessel eines Musters auf den Stapel (ohne Objekt).
    fn eigenschaft_name_stack(&mut self, f: &mut Fn, n: &EigenschaftName) -> Result<(), JsFehler> {
        match n {
            EigenschaftName::Text(t) => {
                let i = self.text(t)?;
                f.emittiere(Op::Text(i));
            }
            EigenschaftName::Zahl(z) => {
                f.emittiere(Op::Zahl(*z));
            }
            EigenschaftName::Berechnet(e) => self.ausdruck(f, e)?,
        }
        Ok(())
    }
}

/// Binaerer Operator -> Befehl.
fn binaer_op(op: BinaerOp) -> Op {
    match op {
        BinaerOp::Plus => Op::Plus,
        BinaerOp::Minus => Op::Minus,
        BinaerOp::Stern => Op::Mal,
        BinaerOp::Schrag => Op::Geteilt,
        BinaerOp::Prozent => Op::Modulo,
        BinaerOp::SternStern => Op::Potenz,
        BinaerOp::Kleiner => Op::Kleiner,
        BinaerOp::KleinerGleich => Op::KleinerGleich,
        BinaerOp::Groesser => Op::Groesser,
        BinaerOp::GroesserGleich => Op::GroesserGleich,
        BinaerOp::GleichGleich => Op::Gleich,
        BinaerOp::RufGleich => Op::Ungleich,
        BinaerOp::GleichGleichGleich => Op::StriktGleich,
        BinaerOp::RufGleichGleich => Op::StriktUngleich,
        BinaerOp::Instanceof => Op::InstanceVon,
        BinaerOp::In => Op::InOperator,
        BinaerOp::KleinerKleiner => Op::LinksSchieben,
        BinaerOp::GroesserGroesser => Op::RechtsSchieben,
        BinaerOp::GroesserGroesserGroesser => Op::UnsRechtsSchieben,
        BinaerOp::Und => Op::BitUnd,
        BinaerOp::Oder => Op::BitOder,
        BinaerOp::Hoch => Op::BitXor,
    }
}

/// Zusammengesetzter Zuweisungsoperator -> Befehl.
fn zuweisung_op(op: ZuweisungsOp) -> Op {
    match op {
        ZuweisungsOp::Plus => Op::Plus,
        ZuweisungsOp::Minus => Op::Minus,
        ZuweisungsOp::Stern => Op::Mal,
        ZuweisungsOp::Schrag => Op::Geteilt,
        ZuweisungsOp::Prozent => Op::Modulo,
        ZuweisungsOp::SternStern => Op::Potenz,
        ZuweisungsOp::KleinerKleiner => Op::LinksSchieben,
        ZuweisungsOp::GroesserGroesser => Op::RechtsSchieben,
        ZuweisungsOp::GroesserGroesserGroesser => Op::UnsRechtsSchieben,
        ZuweisungsOp::Und => Op::BitUnd,
        ZuweisungsOp::Oder => Op::BitOder,
        ZuweisungsOp::Hoch => Op::BitXor,
        // GEMESSEN (A5): nicht erreichbar — `&&=`, `||=` und `??=` werden
        // mit Sprüngen behandelt (siehe `Ausdruck::Zuweisung`). Vorher
        // standen hier Sprünge ins Leere (`Op::LogischNicht`).
        ZuweisungsOp::UndUnd | ZuweisungsOp::OderOder | ZuweisungsOp::FrageFrage => Op::Positiv,
        ZuweisungsOp::Gleich => Op::Positiv,
    }
}

/// Block mit let/const/function? Dann braucht er eine eigene Umgebung.
fn block_braucht_umgebung(v: &[Anweisung]) -> bool {
    let mut stapel: Vec<&Anweisung> = v.iter().collect();
    let mut n = 0usize;
    let max = v.len().saturating_mul(4).saturating_add(64);
    while let Some(a) = stapel.pop() {
        n += 1;
        if n > max {
            return true;
        }
        // J28: durch `MitZeile` sehen (ein Block mit `function` braucht
        // eine eigene Umgebung — sonst waeren die Namen `undefined`).
        match a.inhalt() {
            Anweisung::Deklaration { art, .. } if *art != DeklArt::Var => return true,
            Anweisung::Funktion { .. } | Anweisung::Klasse { .. } => return true,
            Anweisung::Block(x) => stapel.extend(x.iter()),
            Anweisung::Wenn { dann, sonst, .. } => {
                stapel.push(dann);
                if let Some(s) = sonst {
                    stapel.push(s);
                }
            }
            _ => {}
        }
    }
    false
}

fn leere_funktion(name: &str) -> crate::op::Funktion {
    // J37: `leere_funktion` hat **keine** Parameter — leere Namen, und die
    // Liste gilt als einfach (die `eval`-Regel aus 9.2.12 greift dann nicht).
    let param_namen: Vec<u32> = Vec::new();
    let param_einfach = true;
    crate::op::Funktion {
        code: std::sync::Arc::new(Vec::new()),
        namen: Default::default(),
        namen_hash: Vec::new(),
        // J28: unbekannt (0) — diese Funktion ist leer.
        zeile: 0,
        handler: Vec::new(),
        temps: 0,
        var_namen: Default::default(),
        var_hashes: Default::default(),
        kopier_namen: Vec::new(),
        parameter: 0,
        // J37: keine Parameter — die `eval`-Regel aus 9.2.12 greift nicht.
        param_namen,
        param_einfach,
        // J38: lexikalische Namen der obersten Ebene.
        lex_namen: Vec::new(),
        rest: false,
        pfeil: false,
        asynchron: false,
        generator: false,
        name: name.to_string(),
        braucht_this: false,
        braucht_arguments: false,
        selbst_name: None,
        strikt: false,
        // J19: Platzhalter (Hauptlauf und Zwischenfunktionen haben
        // keinen eigenen Quelltext — siehe `funktions*_quelle`).
        quelle: None,
        muster_aus: Vec::new(),
    }
}

fn fn_von(
    // J37: `&mut Fn`, weil `param_namen` die Namen noch indizieren muss
    // (`name_index` braucht `&mut self`) — siehe die Rechnung vor dem Literal.
    mut f: Fn,
    name: &str,
    pfeil: bool,
    asynchron: bool,
    generator: bool,
    braucht_this: bool,
) -> crate::op::Funktion {
    // J3b: die Namens-Hashes **einmal** hier berechnen — im Betrieb
    // kostet ein Namenszugriff dann nur noch einen `u64`-Vergleich
    // (sonst: Text kopieren + String-Hash, 56 ns je Zugriff).
    let namen_hash: Vec<u64> = f
        .namen
        .iter()
        .map(|n| crate::heap::namens_hash(n))
        .collect();
    // J8: `arguments` nur bauen, wenn der Name im Pool steht (Begruendung
    // beim Feld `Funktion::braucht_arguments`).
    let braucht_arguments = f.namen.iter().any(|n| n == "arguments");
    // J37: Namen und „einfach" **vor** dem Literal berechnen: der Bauer wird
    // im Literal schrittweise verschoben (`f.muster_aus`, `f.var_namen`, …),
    // und `name_index` braucht `&mut Fn`.
    //
    // GEMESSEN, warum zwei Schritte: `name_index` leiht `f` veraenderlich,
    // das Uebertragen von `f.param_namen_text` leiht es unveraenderlich —
    // beides zugleich gibt `E0382` (gemessen, nicht vermutet).
    let param_namen: Vec<u32> = {
        let namen = f.param_namen_text.clone();
        let mut idx = Vec::with_capacity(namen.len());
        for n in &namen {
            idx.push(f.name_index(n));
        }
        idx
    };
    let param_einfach = f.param_einfach;
    // J38: die lexikalischen Namen der **obersten** Ebene mitgeben — siehe
    // `hissen_in`. Vor dem Literal berechnet, weil `f` dort verschoben wird.
    let lex_namen: Vec<String> = f.lex_namen.clone();
    crate::op::Funktion {
        // J3: einmalig in eine `Arc` — der Rahmen zaehlt sie nur noch.
        code: std::sync::Arc::new(f.code),
        namen: std::sync::Arc::new(f.namen),
        namen_hash,
        // J28: die Zeile der Deklaration (der Parser hat sie gesetzt).
        zeile: f.zeile,
        braucht_arguments,
        handler: f.handler,
        temps: f.temps,
        var_hashes: std::sync::Arc::new(f.var_namen.iter().map(|n| crate::heap::namens_hash(n)).collect()),
        var_namen: std::sync::Arc::new(f.var_namen),
        kopier_namen: f.kopier_namen,
        // Schluessellisten fuer `Op::MusterRest` (A5/Orakel w039).
        muster_aus: f.muster_aus,
        parameter: f.parameter,
        // J37: Namen und „einfach" mitgeben — siehe `op.rs`.
        //
        // **ZWEI Schritte, nicht einer** (Leihproblem): `name_index` braucht
        // `&mut Fn`, und ueber `f.params` zu iterieren leiht `f` schon.
        // Deshalb erst die Namen als **Text** sammeln, dann indizieren.
        param_namen,
        param_einfach,
        // J38: lexikalische Namen der obersten Ebene.
        lex_namen,
        rest: f.rest,
        pfeil,
        asynchron,
        generator,
        name: name.to_string(),
        braucht_this,
        selbst_name: None,
        // J14: wird von `funktion_kompilieren` aus der Direktive gesetzt.
        strikt: false,
        // J19: wird von `funktion_kompilieren` aus `FunktionsBau` gesetzt.
        quelle: None,
    }
}

/// Ein Zielblatt einer Muster-Zuweisung.
struct Blatt {
    /// Zielname (u16::MAX = kein Name, sondern verschachteltes Muster).
    name: u32,
    /// Temp (Platzhalter, damit die Zuweisung eine Adresse hat).
    key: u16,
    /// Wie der Wert gelesen wird.
    lesen: LesArt,
    standard: Option<Ausdruck>,
    /// Verschachteltes Muster (dessen Wert wird weiter verteilt).
    muster: Option<Muster>,
    /// Nummer der Ausschlussliste fuer `LesArt::Rest` (0 = keine).
    muster_aus: u32,
}

/// Leseart eines Blattes.
enum LesArt {
    /// Der Wert selbst ist schon da (Name ohne Restructure).
    Var,
    /// `objekt.schluessel`.
    Prop(EigenschaftName),
    /// `feld[index]`.
    Index(u32),
    /// `feld[index..]` (Rest).
    FeldRest(u32),
    /// `{...rest}`.
    Rest,
}

// ---------------------------------------------------------------------------
// Klassen (V2)
// ---------------------------------------------------------------------------

impl Compiler {
    /// Eine Klasse uebersetzen: Elternwert auf dem Stapel, dann `Klasse(idx)`.
    ///
    /// `binden`: Klassendeklaration (Name im umgebenden Scope binden).
    /// `quelle` (J19): der Quelltext der Klasse — er wird dem
    /// Konstruktor als `quelle` mitgegeben, damit `A.toString()` den
    /// Klassentext liefert (wie in node).
    fn klasse(
        &mut self,
        f: &mut Fn,
        name: Option<&str>,
        eltern: Option<&Ausdruck>,
        elemente: &[KlassenElement],
        binden: bool,
        quelle: Option<&str>,
    ) -> Result<(), JsFehler> {
        let abgeleitet = eltern.is_some();
        match eltern {
            None => {
                f.emittiere(Op::Null);
            }
            Some(e) => self.ausdruck(f, e)?,
        }
        // Feld-Initialisierer (Instanz + statisch) als Funktionen bauen.
        // Feld-Initialisierer (Instanz + statisch) als Funktionen bauen.
        //
        // J12: berechnete Elementnamen (`['n']`) werden **einmal** hier
        // ausgewertet — in Quelltext-Reihenfolge und in dem Scope, in dem
        // die Klasse entsteht (Spec sec-15.7). GEMESSEN mit node 22 als
        // Vergleich: `for (let i=0;i<2;i++) class B { ['f'+i]=i }` — dort
        // bekommen beide Objekte den Schluessel `f0`.
        //
        // Zwei Durchlaenge: erst nur die **Namen** der Schluessel-
        // Variablen (die braucht `feld_init` fuer `this[schluessel] = …`),
        // dann die Auswertung in `f` — sie steht vor `Op::Klasse`.
        let mut key_variablen: Vec<Option<String>> = Vec::with_capacity(elemente.len());
        for (idx, el) in elemente.iter().enumerate() {
            let n = match el {
                KlassenElement::Methode { name, .. } | KlassenElement::Feld { name, .. } => name,
                KlassenElement::Block { .. } => {
                    key_variablen.push(None);
                    continue;
                }
            };
            let key = if let EigenschaftName::Berechnet(e) = n {
                let var = format!("__jsvm_key_{}_{}__", idx, self.klassenzaehler);
                self.klassenzaehler = self.klassenzaehler.wrapping_add(1);
                self.ausdruck(f, e)?;
                let ni = f.name_index(&var);
                f.emittiere(Op::DefVar(ni, 1));
                Some(var)
            } else {
                None
            };
            key_variablen.push(key);
        }
        let felder_inst = self.feld_init(elemente, false, &key_variablen)?;
        let felder_stat = self.feld_init(elemente, true, &key_variablen)?;
        // Konstruktor: explizit oder Default.
        let cname = name.unwrap_or("").to_string();
        let mut ctor: Option<u32> = None;
        for el in elemente {
            if let KlassenElement::Methode {
                art: MethodenArt::Konstruktor,
                statisch: false,
                funktion,
                ..
            } = el
            {
                // Basis: Felder VOR dem Rumpf, Ableitung: danach (Naeherung
                // wie in narsil-js, VERTRAG_JS).
                ctor = Some(self.funktion_kompilieren(
                    funktion,
                    &cname,
                    Some((felder_inst, !abgeleitet)),
                    true,
                )?);
            }
        }
        let ctor = match ctor {
            Some(c) => c,
            None => self.default_ctor(&cname, abgeleitet, felder_inst)?,
        };
        // J19: der Konstruktor traegt den **Klassen**quelltext — damit
        // `A.toString()` die Klasse zeigt und nicht nur ihren Rumpf.
        // GEMESSEN mit node 22: `class C { constructor(){ this.x=1 } }`
        // `.toString()` -> `class C { constructor(){ this.x=1 } }`.
        if let Some(q) = quelle
            && let Some(fl) = (ctor as usize)
                .checked_sub(self.basis_funktionen)
                .and_then(|i| self.funktionen.get_mut(i))
        {
            fl.quelle = Some(std::sync::Arc::from(q));
        }
        // Elemente (die Schluessel-Variablen sind oben schon berechnet).
        let mut code_elemente: Vec<KlassenElementCode> = Vec::new();
        for (idx, el) in elemente.iter().enumerate() {
            let key_var = key_variablen[idx].clone();
            match el {
                KlassenElement::Methode {
                    name: n,
                    funktion,
                    statisch,
                    art,
                } => {
                    if matches!(art, MethodenArt::Konstruktor) && !*statisch {
                        continue;
                    }
                    let idx = self.funktion_kompilieren(funktion, "", None, false)?;
                    let (ni, berechnet) = match &key_var {
                        Some(v) => (self.text(v)?, true),
                        None => (self.name_text_index(n)?, false),
                    };
                    code_elemente.push(KlassenElementCode::Methode {
                        name: ni,
                        fun: idx,
                        statisch: *statisch,
                        art: *art,
                        berechnet,
                    });
                }
                KlassenElement::Feld {
                    name: n, statisch, ..
                } => {
                    let (ni, berechnet) = match &key_var {
                        Some(v) => (self.text(v)?, true),
                        None => (self.name_text_index(n)?, false),
                    };
                    code_elemente.push(KlassenElementCode::Feld {
                        name: ni,
                        statisch: *statisch,
                        berechnet,
                    });
                }
                // Release 10.10.2026: `static { … }` laeuft in `feld_init`.
                KlassenElement::Block { .. } => {}
            }
        }
        if self.klassen.len() >= MAX_KLASSEN {
            return Err(JsFehler::Speicher("zu viele Klassen (M4)".to_string()));
        }
        self.klassen.push(KlassenDef {
            name: name.map(|s| s.to_string()),
            ctor: Some(ctor),
            elemente: code_elemente,
            felder_statisch: felder_stat,
        });
        f.emittiere(Op::Klasse((self.basis_klassen + self.klassen.len() - 1) as u32));
        // ## W12 (JS, 04.10.2026) — STAPEL-GLEICHGEWICHT nach einer
        // Klassendeklaration
        //
        // `Op::Klasse` schiebt den Klassenwert (**+1**). `Op::DefVar`
        // **nimmt** ihn mit `stapel.pop()` mit (vm.rs, `Op::DefVar`) — der
        // Wert ist damit schon weg. Der darauf folgende `Op::Pop(1)` poppte
        // also einen Wert, den **niemand geliefert** hatte.
        //
        // GEMESSEN, mit `o={m:function(a,b,c){return a+'/'+b+'/'+c}}` und
        // `f=function(){class C{} return 7}`:
        //     `o.m(1,f(),3)`                ->  `7/7/3`   (norm `1/7/3`)
        //     `o.m(1,2)` (ohne Klasse)      ->  `1/2`     (norm, richtig)
        //     `var r=f(); o.m(1,r,3)`       ->  `1/7/3`   (zwei Anweisungen)
        // `Op::AufrufMethode` liest die Basis an einem **festen Abstand**,
        // `stapel[l - n - 2]` (Layout `basis, arg1..argn, schluessel`). Der
        // Extra-Pop nahm einen Operanden weg, und **alle** Argumente rutschten
        // um eins nach vorn — daher das `7` an erster Stelle und das
        // richtige `3` an letzter.
        //
        // `if (false) { class C{} }` war **gruen**: nur eine
        // **ausgefuehrte** Deklaration loest es aus. Das ist der Beleg, dass
        // es an diesem Befehl liegt und nicht an Compile-Zeit, GC,
        // Inline-Cache oder am Klassennamen (alle vier sind gemessen und
        // ausgeschlossen, Bericht JS_8 Abschnitt 5).
        //
        // FUER DEN ANONYMEN FALL bleibt der `Pop`: gibt es kein `DefVar`,
        // das den Wert nimmt, muss er hier abgeraeumt werden.
        if binden {
            match name {
                Some(n) => {
                    let ni = f.name_index(n);
                    f.emittiere(Op::DefVar(ni, 1));
                }
                None => {
                    f.emittiere(Op::Pop(1));
                }
            }
        }
        Ok(())
    }

    /// Textindex eines Klassenelement-Namens.
    ///
    /// J12: berechnete Namen (`['n']`) laufen ueber denselben Aufruf und
    /// liefern `(Index des VARIABLENNAMENS, true)`. GEMESSEN, warum eine
    /// Variable und kein Text: der Schluessel wird **einmal** beim Anlegen
    /// der Klasse ausgewertet (Spec sec-15.7); der Wert laeuft in der
    /// Bindung der umgebenden Funktion, die die Klasse baut — dort kann
    /// der Initialisierer (`<felder>`) und der Klassenaufbau sie lesen.
    fn name_text_index(&mut self, n: &EigenschaftName) -> Result<u32, JsFehler> {
        match n {
            EigenschaftName::Text(t) => self.text(t),
            EigenschaftName::Zahl(z) => self.text(&crate::ops::zahl_schluessel(*z)),
            EigenschaftName::Berechnet(_) => Err(JsFehler::Syntax {
                zeile: 0,
                spalte: 0,
                nachricht: "berechnete Klassennamen werden nicht unterstuetzt".to_string(),
            }),
        }
    }

    /// J14: beginnt die Anweisungsliste mit der Direktive `"use strict"`?
    fn rumpf_ist_strikt(rumpf: &[Anweisung]) -> bool {
        for a in rumpf.iter().take(2) {
            // J28: `MitZeile` ist durchsichtig (siehe `uebersetze`, wo
            // dasselbe Prinzip gemessen wurde: `function f(){ 'use strict';
            // return this } f()` lieferte ohne `.inhalt()` `object` statt
            // `undefined` — der Motor war also **nicht** strikt).
            match a.inhalt() {
                // `"use strict";`
                Anweisung::Ausdruck(Ausdruck::Text(t)) => return t == "use strict",
                Anweisung::Leer => continue,
                _ => return false,
            }
        }
        false
    }

    /// Initialisierer der Felder: `this.name = <wert>` fuer jedes Feld.
    fn feld_init(
        &mut self,
        elemente: &[KlassenElement],
        statisch: bool,
        key_variablen: &[Option<String>],
    ) -> Result<u32, JsFehler> {
        let mut g = Fn::neu("<felder>");
        for (idx, el) in elemente.iter().enumerate() {
            // Release 10.10.2026: `static { … }` in der Reihenfolge der
            // statischen Felder (ES2022 ClassDefinitionEvaluation, Schritt 31).
            if let KlassenElement::Block { ausdruck } = el {
                if statisch {
                    self.ausdruck(&mut g, ausdruck)?;
                    g.emittiere(Op::Pop(1));
                }
                continue;
            }
            if let KlassenElement::Feld {
                name: n,
                wert,
                statisch: st,
            } = el
            {
                if *st != statisch {
                    continue;
                }
                // GEMESSEN: `SchreibeIndex` liest [.., basis, wert, schluessel]
                // — der Schluessel liegt GANZ OBEN. Mit [this, key, wert]
                // las der Befehl den Schluessel als Objekt und schrieb auf
                // einen Text („kein Setzen auf Primitiv"): statische Felder
                // wurden nie angelegt (p2.js Z8).
                // GEMESSEN: `SchreibeIndex` liest den Stapel als
                // [.., basis, wert, schluessel] — Objekt ZUERST, dann der
                // Wert, dann der Schluessel ganz oben.
                g.emittiere(Op::This);
                match wert {
                    Some(e) => self.ausdruck(&mut g, e)?,
                    None => {
                        g.emittiere(Op::Undef);
                    }
                }
                // J12: berechneter Schluessel -> die Variable aus dem
                // umgebenden Scope lesen. Sie liegt in der Kette, weil die
                // Initialisierer-Funktion den Klassen-Scope erbt
                // (`closure_anlegen_als(…, umg_def, …)`).
                // J41: `SetFunctionName` fuer den Feldinitialisierer
                // (ES2023 15.7.1 `ClassFieldDefinitionEvaluation`, Schritt 3:
                // „If IsAnonymousFunctionDefinition(value) is true, perform
                // SetFunctionName(value, propKey)").
                //
                // GEMESSEN (node 22), **acht** von elf Proben waren falsch:
                //   `static #f = function(){}` -> `"#f"` (mit **Raute**!)
                //   `#f = function(){}`        -> `"#f"`
                //   `static f = function(){}`  -> `"f"`
                //   `f = function(){}`         -> `"f"`
                //   `[x] = function(){}` (x = 'a') -> `"a"`  (berechnet!)
                //   `static #f = class{}`      -> `"#f"`
                //   `static #f = () => {}`     -> `"#f"`
                //   `static #f = function inner(){}` -> `"inner"` (eigener Name)
                //
                // **ZWEI OPS, WEIL DER NAME ZWEI FORMEN HAT:** bei festem
                // Namen kennt ihn der Uebersetzer (`NamenSetzen`), bei
                // berechnetem steht er erst zur Laufzeit fest — dann steht
                // der Schluessel **oben** und der Wert darunter
                // (`NamenSetzenBerechnet`, siehe `op.rs`).
                let namen_gesetzt = match wert {
                    Some(e) => Self::ist_anonyme_funktion(e),
                    None => false,
                };
                // **VOR** dem Schluessel-Push: der Wert muss **oben** liegen
                // (GEMESSEN/GEFUNDEN: nach dem Push lag der Schluessel oben,
                // also wurde der Schluessel benannt — und der Wert nicht).
                let berechnet = key_variablen.get(idx).and_then(|k| k.clone()).is_some();
                if namen_gesetzt && !berechnet {
                    let text = match n {
                        crate::ast::EigenschaftName::Text(t) => t.clone(),
                        crate::ast::EigenschaftName::Zahl(z) => crate::ops::zahl_schluessel(*z),
                        crate::ast::EigenschaftName::Berechnet(_) => String::new(),
                    };
                    if !text.is_empty() {
                        let idx_name = g.name_index(&text);
                        g.emittiere(Op::NamenSetzen(idx_name));
                    }
                }
                match key_variablen.get(idx).and_then(|k| k.clone()) {
                    Some(var) => {
                        let ni = g.name_index(&var);
                        g.emittiere(Op::LeseVar(ni));
                        if namen_gesetzt {
                            g.emittiere(Op::NamenSetzenBerechnet);
                        }
                    }
                    None => {
                        let ni = self.name_text_index(n)?;
                        g.emittiere(Op::Text(ni));
                    }
                }
                g.emittiere(Op::SchreibeIndex);
            }
        }
        g.emittiere(Op::Undef);
        g.emittiere(Op::Rueckgabe);
        self.pruefe(&g)?;
        if self.basis_funktionen + self.funktionen.len() >= MAX_KONSTANTEN {
            return Err(JsFehler::Speicher("zu viele Funktionen (M4)".to_string()));
        }
        let idx = (self.basis_funktionen + self.funktionen.len()) as u32;
        self.funktionen
            // GEMESSEN: die Argumentfolge von `fn_von` ist
            // (pfeil, asynchron, generator, braucht_this). Mit `true` an
            // erster Stelle wurde der Feld-Initialisierer eine Pfeilfunktion
            // OHNE `this` — `Op::This` lieferte `undefined` und das Schreiben
            // endete mit „kein Setzen auf Primitiv" (statische Felder).
            .push(fn_von(g, "<felder>", false, false, false, true));
        Ok(idx)
    }

    /// Standardkonstruktor: `(...args){ super(...args) }` bzw. leer.
    fn default_ctor(&mut self, name: &str, abgeleitet: bool, felder: u32) -> Result<u32, JsFehler> {
        let mut g = Fn::neu(name);
        g.parameter = 0;
        g.rest = true;
        if abgeleitet {
            g.emittiere(Op::RestParams(0));
            g.emittiere(Op::AufrufSuperStreu);
        }
        g.emittiere(Op::FelderStart(felder));
        g.emittiere(Op::RueckgabeThis);
        self.pruefe(&g)?;
        if self.basis_funktionen + self.funktionen.len() >= MAX_KONSTANTEN {
            return Err(JsFehler::Speicher("zu viele Funktionen (M4)".to_string()));
        }
        let idx = (self.basis_funktionen + self.funktionen.len()) as u32;
        self.funktionen
            .push(fn_von(g, name, false, false, false, true));
        Ok(idx)
    }
}

/// Alle gebundenen Namen eines Musters (Hissen, `for`-Kopf, Parameter).
/// J37: die Namen einer Parameterliste als Text — **alle** gebundenen
/// Namen, auch die aus Mustern.
///
/// GEMESSEN (node 22), warum `einfach` getrennt geprueft wird:
///   `(a) => eval('var a = 1')`        -> **erlaubt** (einfache Liste)
///   `(a = eval('var a = 42')) => { }` -> SyntaxError (nicht einfach)
///   `(a = eval('var b = 42')) => { }` -> **erlaubt** (anderer Name)
pub(crate) fn parameter_namen_text(params: &[Muster], rest: Option<&Muster>) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for m in params {
        for n in muster_namen(m) {
            if !raus.contains(&n) {
                raus.push(n);
            }
        }
    }
    if let Some(r) = rest {
        for n in muster_namen(r) {
            if !raus.contains(&n) {
                raus.push(n);
            }
        }
    }
    raus
}

/// J37: `IsSimpleParameterList` (ES2023 15.2.1) — nur Namen, keine Ruhe-
/// zeile, keine Standardwerte, keine Muster.
pub(crate) fn parameterliste_einfach(params: &[Muster], rest: Option<&Muster>) -> bool {
    rest.is_none() && params.iter().all(|m| matches!(m, Muster::Name(_)))
}

/// Release 10.10.2026: der Platzhalter einer Feld-Luecke (`[, a]`). Das
/// fuehrende NUL-Zeichen macht ihn zu einem Namen, den kein Programm
/// schreiben kann — er kollidiert also weder mit `var _loch_0` noch mit
/// einer zweiten Luecke im selben Block. Gebunden wird er nie
/// (`muster_binden`, `blatt_sammeln`), und `muster_namen` meldet ihn nicht.
pub(crate) const LOCH: &str = "\u{0}loch";

pub(crate) fn ist_loch(m: &Muster) -> bool {
    matches!(m, Muster::Name(n) if n == LOCH)
}

pub(crate) fn muster_namen(m: &Muster) -> Vec<String> {
    let mut v = Vec::new();
    let mut stapel = vec![m];
    // M3: Der Musterbaum endlich (Parser-Tiefe) + Deckel.
    let mut rest = 10_000u32;
    while let Some(x) = stapel.pop() {
        if rest == 0 {
            break;
        }
        rest -= 1;
        match x {
            Muster::Name(n) if n == LOCH => {}
            Muster::Name(n) => v.push(n.clone()),
            Muster::Feld(t) => {
                for f in t {
                    stapel.push(&f.muster);
                }
            }
            Muster::Gegenstand(t) => {
                for g in t {
                    stapel.push(&g.muster);
                }
            }
            Muster::Standard(inner, _) => stapel.push(inner),
            Muster::Rest(inner) => stapel.push(inner),
            // Ein Member-Ziel bindet keinen Namen.
            Muster::Ziel(_) => {}
        }
    }
    v
}

impl Compiler {
    /// Genau EINE Funktion uebersetzen (fuer `new Function(...)` zur
    /// Laufzeit, V6). Der Bytecode ist dann Teil des laufenden Programms.
    pub(crate) fn einzel_funktion(
        &mut self,
        bau: &FunktionsBau,
        name: &str,
    ) -> Result<u32, JsFehler> {
        // Index 0 bleibt reserviert.
        self.funktionen.push(leere_funktion("<leer>"));
        self.funktion_kompilieren(bau, name, None, false)
    }

    /// Kopie einer uebersetzten Funktion (Eintragen in ein laufendes Programm).
    pub(crate) fn funktion_kopie(&self, idx: u32) -> Result<Funktion, JsFehler> {
        self.funktionen
            .get(idx as usize)
            .cloned()
            .ok_or_else(|| JsFehler::Laufzeit("Funktionsindex falsch (intern)".to_string()))
    }
}
