// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Interpreter-Kern (J3): Baumdurchlauf mit Geltungsbereichen, Closures,
//! Prototypen, `this`/`new`, Ausnahmen, Getter/Setter, Schritt- und
//! Wanduhr-Begrenzung, Heap-Limit mit GC, expliziter Wurzelstapel.

/// BEGRUENDUNG fuer `dead_code` (gemessen am 30.09.2026): Engine-Oberflaeche: Felder und Hilfer, die der Walker-Betrieb
/// (narsil-js) hatte und die die VM heute nicht nutzt.
#[allow(dead_code)]
use std::collections::HashMap;
use std::time::Instant;

use crate::builtin::Nativ;
use crate::fehler::JsFehler;
use crate::grenzen::{
    Grenzen, MAX_AUFRUFTIEFE_HART, MAX_DAUER_HART_MS, MAX_HANDLER_ARGUMENTE, MAX_HEAP_HART,
    MAX_HOERER_HART, MAX_KNOTEN_HART, MAX_MIKROTASK_HART, MAX_NETZ_ANTWORT_HART,
    MAX_NETZ_AUFGABEN_HART, MAX_NETZ_FRIST_HART_MS, STAPEL_LAUF,
};
use crate::heap::{Bindung, BindungsArt, GVal, HObj, Heap, HeapRef, KEIN_REF, Umgebung};
use crate::wert::Wert;

/// Wieviele Allokationen bis zum naechsten vorsorglichen GC-Lauf.
const GC_ALLE_N_ALLOKS: usize = 4096;
/// J3c: und wie viele **Bytes** bis zum naechsten Lauf — als Bruchteil der
/// Heap-Grenze, mit diesem Mindestwert.
///
/// GEMESSEN, warum beides noetig ist: der zaehlbasierte Ausloeser allein
/// haengt an der Zahl der Objekte, nicht an ihrer Groesse. Bei
/// `s = s + 'x'` (ein Obekt je Schritt, aber immer groesser) sammelte er
/// bis zu 4096 Zeichenketten ein — **181 MiB** am Ende eines Laufs, dessen
/// Ergebnis 100 KiB gross ist, und mit `Grenzen::standard()` (64 MiB)
/// scheiterte der Lauf an der Heap-Grenze. Mit dem Byte-Anteil wird
/// gesammelt, **bevor** so viel zusammenliegt; die Zaehlgrenze bleibt fuer
/// kleine Objekte.
const GC_BYTES_ANTEIL: usize = 16;
/// Der Mindestbetrag, damit eine winzige Heap-Grenze nicht jeden
/// Schritt sammeln laesst.
const GC_BYTES_MINDEST: usize = 256 * 1024;
/// Wanduhr-Grenze fuer EINEN `ausfuehren`-Aufruf (J5: `< 2 s`).
/// Der Wert kommt aus `Grenzen::max_dauer_ms` (J3); die Konstante hier ist
/// nur der Deckel, mit dem die Grenze gehaertet wird.
const WANDUHR_HART_MS: u128 = crate::grenzen::MAX_DAUER_HART_MS as u128;

/// Ablauf-Steuerung fuer Anweisungen (Rueckgabe/Abbruch/Wurf).
#[derive(Debug, Clone)]
pub(crate) enum Ablauf {
    Weiter,
    /// Vervollstaendigungswert (fuer `eval`/Programm-Ergebnis).
    Wert(GVal),
    Rueck(GVal),
    Abbruch {
        weiter: bool,
        marke: Option<String>,
    },
    Wurf(GVal),
}

/// Baut ein EREIGNIS-Objekt fuer einen Handler (W6D: J9/J10).
///
/// # EINE STELLE, weil es zwei Aufrufer gibt
///
/// `narsil` baut das Objekt an zwei Orten: im W6D-Weg (der Handler ist
/// ein Funktionswert) und im W6B-Weg `__event` (der Handler ist
/// Quelltext). GEMESSEN, warum das aufgefallen ist: die W6B-Fassung
/// hatte `preventDefault: undefined` und keine `type` — ein Skript,
/// das `ev.type` las, bekam `undefined` und der Handler tat nichts.
/// Zwei Fassungen sind zwei Wahrheiten; ab jetzt gibt es eine.
///
/// `x`/`y` sind Bildschirmkoordinaten (`clientX`/`clientY`); im W6D-Weg
/// sind sie 0, weil die Engine die Koordinaten nicht kennt.
pub fn ereignis_als_wert(typ: &str, target: Option<u32>, x: f32, y: f32) -> Wert {
    Wert::Gegenstand(vec![
        ("type".to_string(), Wert::Text(typ.to_string())),
        (
            "target".to_string(),
            match target {
                Some(t) => Wert::Zahl(t as f64),
                None => Wert::Undefiniert,
            },
        ),
        (
            "currentTarget".to_string(),
            match target {
                Some(t) => Wert::Zahl(t as f64),
                None => Wert::Undefiniert,
            },
        ),
        ("clientX".to_string(), Wert::Zahl(x as f64)),
        ("clientY".to_string(), Wert::Zahl(y as f64)),
        ("defaultPrevented".to_string(), Wert::Wahr(false)),
        // GEMESSEN, warum `Nativ` und nicht `undefined` (14:50): die
        // Koordination verlangt `preventDefault` als RUFEN. Als
        // `undefined` gibt es `ev.preventDefault is not a function` —
        // und ein Handler, der so endet, macht den Klick wirkungslos.
        // Der Name ist der einer Host-Funktion; `import_wert` prueft
        // ihn beim Einhaengen.
        (
            "preventDefault".to_string(),
            Wert::Nativ(PREVENT_DEFAULT.to_string()),
        ),
    ])
}

/// Stoerung aus Ausdrucks-Sicht: fangbarer Wurf oder Engine-Limit
/// (Limits duerfen KEIN `catch` fangen — sonst koennte ein Skript die
/// Speichergrenze wegfangen).
#[derive(Debug, Clone)]
pub(crate) enum Stoerung {
    Wurf(GVal),
    Fehler(JsFehler),
}

impl From<JsFehler> for Stoerung {
    fn from(e: JsFehler) -> Self {
        Stoerung::Fehler(e)
    }
}

/// Eine Mikrotask (Promise-Reaktion, Timer, async-Fortsetzung light).
#[derive(Debug, Clone)]
pub(crate) struct Mikrotask {
    pub reaktion_fn: Option<HeapRef>,
    pub folge: HeapRef,
    pub wert: GVal,
    pub erfuellt: bool,
    /// V3: Fortsetzung eines Generators / angehaltenen async-Rahmens.
    pub fortsetzung: Option<HeapRef>,
    /// Was der Fortsetzung uebergeben wird.
    pub aufnahme: crate::vm::Aufnahme,
    /// 0 = normale Reaktion, 1 = `finally`, 2 = `then`,
    /// 3 = **Thenable-Job** (J9: `then` eines fremden Objekts aufrufen).
    pub art: u8,
}

impl Mikrotask {
    /// J9: `PromiseResolveThenableJob` (sec-27.2.3.3.1) — `then` eines
    /// Thenables aufrufen und das Versprechen `folge` damit erfuellen.
    pub(crate) fn thenable_job(then_fn: HeapRef, thenable: GVal, folge: HeapRef) -> Self {
        Mikrotask {
            reaktion_fn: Some(then_fn),
            folge,
            wert: thenable,
            erfuellt: true,
            fortsetzung: None,
            aufnahme: crate::vm::Aufnahme::Wert(GVal::Undefiniert),
            art: 3,
        }
    }

    /// Reaktion (Promise-Aufloesung).
    pub(crate) fn reaktion(
        reaktion_fn: Option<HeapRef>,
        folge: HeapRef,
        wert: GVal,
        erfuellt: bool,
    ) -> Self {
        Mikrotask {
            reaktion_fn,
            folge,
            wert,
            erfuellt,
            fortsetzung: None,
            aufnahme: crate::vm::Aufnahme::Wert(GVal::Undefiniert),
            art: 0,
        }
    }

    /// Reaktion mit Art (`finally` = 1, `then` = 2).
    pub(crate) fn reaktion_art(
        reaktion_fn: Option<HeapRef>,
        folge: HeapRef,
        wert: GVal,
        erfuellt: bool,
        art: u8,
    ) -> Self {
        Mikrotask {
            reaktion_fn,
            folge,
            wert,
            erfuellt,
            fortsetzung: None,
            aufnahme: crate::vm::Aufnahme::Wert(GVal::Undefiniert),
            art,
        }
    }

    /// Fortsetzung eines Generators/async-Rahmens (V3).
    pub(crate) fn fortsetzung(gz: HeapRef, aufnahme: crate::vm::Aufnahme) -> Self {
        Mikrotask {
            reaktion_fn: None,
            folge: KEIN_REF,
            wert: GVal::Undefiniert,
            erfuellt: true,
            fortsetzung: Some(gz),
            aufnahme,
            art: 0,
        }
    }
}

/// Timer-Eintrag (`setTimeout`/`setInterval`/`requestAnimationFrame`).
///
/// A2: der Zeitpunkt ist eine **absolute** Angabe auf der virtuellen Uhr
/// der Engine (`Engine::uhr_ms`). Das Skript selbst hat keine Wartezeit,
/// also wird beim Abarbeiten die Uhr auf den naechsten Termin gestellt —
/// dadurch ist die Reihenfolge deterministisch und ohne Warten messbar.
#[derive(Debug, Clone)]
pub(crate) struct Timer {
    pub id: u32,
    pub funktion: GVal,
    pub args: Vec<GVal>,
    /// Wann der Timer faellig ist (ms auf der virtuellen Uhr).
    pub bis_ms: u64,
    /// `Some(ms)` bei `setInterval`: dann nach jedem Lauf neu stellen.
    pub intervall_ms: Option<u64>,
    pub leerlauf_timeout_ms: Option<u64>,
    /// Nur zur Fehlersuche: welcher Aufruf hat ihn angelegt.
    pub art: TimerArt,
}

/// Welcher Aufruf einen Timer angelegt hat (nur Diagnose/Ausgabe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerArt {
    Timeout,
    Intervall,
    Bild,
    /// `requestIdleCallback` — Näherung über die Host-Timer-Brücke
    /// (echte Idle-/Frame-basierte Terminplanung ist eine spätere Aufgabe).
    Leerlauf,
}

/// Die Engine (J0): `Engine::neu(Grenzen)`, `ausfuehren`, `host_funktion`.
pub struct Engine {
    pub(crate) grenzen: Grenzen,
    pub(crate) heap: Heap,
    pub(crate) global_umgebung: HeapRef,
    pub(crate) global_objekt: HeapRef,
    /// W6/V2: das uebersetzte Programm (Bytecode, Pools, Klassen).
    pub(crate) programm: Option<crate::op::Bytecode>,
    /// J6: **Timer dem Host ueberlassen** (Browser/Oberflaeche).
    ///
    /// GEMESSEN, warum das noetig war: `ausfuehren` leert die
    /// Timerwarteschlange am Ende ueber eine **virtuelle** Uhr — alles
    /// laeuft sofort, in der Reihenfolge der Termine. Fuer ein Skript wie
    /// `setTimeout(() => ladeanzeige_aus(), 3000)` heisst das: die Anzeige
    /// verschwindet im selben Moment. Ist das an, laeuft nur noch die
    /// Mikrotask-Schlange; den Timer holt der Host mit
    /// `Engine::timer_einen_ausfuehren`, wenn er dran ist
    /// (`timer_frueheste_ms` sagt wann).
    ///
    /// Aus ist der Schalter von Anfang an — sonst aendert sich am
    /// Skriptverhalten nichts (`tests/j9_reihenfolge.rs` laeuft ueber
    /// genau diesen Weg und bleibt gruen).
    /// J10: der Modul-Lader (alle bekannten Module, ihre Umgebungen und
    /// Exporte). Der Host fuettert ihn mit Quellen, der Motor verknuepft
    /// sie (siehe `module.rs`).
    pub(crate) module: crate::module::ModulLader,
    pub(crate) timer_beim_host: bool,
    /// J6: `performance.mark`-Marken (Name, ms seit Laufbeginn), gedeckelt.
    pub(crate) performance_marken: Vec<(String, f64)>,
    /// J8: der Textpool als `Arc` — `LeseEigenschaft` leiht den Namen
    /// daraus, statt ihn je Zugriff als `String` zu kopieren (eine
    /// Allokation je Feldzugriff, GEMESSEN in der Property-Schleife).
    pub(crate) texte_arc: std::sync::Arc<Vec<String>>,
    /// J8: Inline-Cache je Textindex (siehe `op::IcEintrag`).
    pub(crate) inline_cache: Vec<crate::op::IcEintrag>,
    /// Release 09.10.2026: Textindex des laufenden Programms (Text ->
    /// Index in `programm.texte`). Der Compiler leiht ihn sich je
    /// Uebersetzung (`Engine::uebersetzen`), statt ihn neu zu bauen.
    pub(crate) texte_index: std::collections::HashMap<String, u32>,
    /// Release 09.10.2026: Groesse des Programms (fuer M4, aufsummiert).
    pub(crate) programm_bytes: usize,
    /// J44: der Generator-Prolog endet **jetzt** (das Feld stuetzt
    /// `Op::GeneratorPrologEnde` aus der wiederhergestellten Fassung).
    pub(crate) generator_startlauf: bool,
    /// W6/V2: die Rahmen als DATEN (kein Rust-Stapel je JS-Aufruf). Der Lauf
    /// eines `for…in`/`for…of` steht seit dem 09.10.2026 im Rahmen
    /// (`Rahmen::iter_lauf`).
    pub(crate) rahmen: Vec<crate::vm::Rahmen>,
    /// Gemeinsamer Operandenstapel aller Rahmen.
    pub(crate) stapel: Vec<GVal>,
    /// Generator, dessen Rahmen gerade laufen (fuer await/yield).
    pub(crate) aktiver_generator: Option<HeapRef>,
    /// Rahmenzahl, unter der die laufende Schleife endet.
    pub(crate) lauf_ende: usize,
    /// Native-Rueckrufe (Rust-Rekursion, begrenzt).
    pub(crate) nativ_tiefe: u32,
    pub(crate) native: HashMap<HeapRef, Nativ>,
    /// Nativ-Ctor -> `prototype`-Objekt (synthetisch in hole()).
    pub(crate) nativ_proto: HashMap<HeapRef, HeapRef>,
    pub(crate) host_funktionen:
        HashMap<String, Box<dyn Fn(&[Wert]) -> Result<Wert, String> + Send>>,
    ///
    /// J28: der Debug-Haken. `None` = **kein Haken**.
    ///
    /// GEMESSEN, warum ein `Option` und kein `bool` mit leerem Zustand:
    /// ohne Haken ist der einzige Zugriff `self.haken.is_some()` — ein
    /// Vergleich auf `None`, ein_branch, und der Zweig ist **immer**
    /// derselbe (fuer den Sprungvorhersager unsichtbar). Messung:
    /// `tests/j28_debugger.rs::ohne_haken_ist_der_preis_nicht_messbar`.
    pub(crate) haken: Option<crate::debugger::Haken>,
    /// J28: die Dateigrenzen der Skripte (welche Zeile zu welcher
    /// Datei gehoert). Ohne sie ist alles Datei 0.
    pub(crate) skripte: crate::debugger::SkriptTabelle,
    /// J28: der Datei-Index beim Start — damit ein Haken, der waehrend
    /// des Laufs gesetzt wird, weiss wo er steht.
    pub(crate) lauf_datei: u32,
    ///
    /// Die Haken fuer HOST-OBJEKTE (W6B, J9).
    ///
    /// Ein Host-Objekt ist ein Objekt mit genau einem Feld `__host`
    /// (eine Zahl). Wird eine Eigenschaft davon gelesen oder geschrieben,
    /// fragt die Engine hier nach. Das ist der Ersatz fuer Getter/Setter
    /// auf Objektebene — `host_funktion` kann nur globale Funktionen
    /// anmelden, und `el.textContent` ist keine globale Funktion.
    pub(crate) host_objekte: Option<std::sync::Arc<dyn crate::hostobjekt::HostObjektHaken>>,
    pub(crate) konsole: Vec<String>,
    pub(crate) schritte: u64,
    pub(crate) start: Instant,
    /// Monotonic lifetime origin, independent of task execution deadlines.
    uhr_start: Instant,
    /// Hostgesteuerte Task-Grenze: Listener teilen Deadline und Schrittbudget.
    browser_task_aktiv: bool,
    pub(crate) aufruftiefe: u32,
    /// Release 10.10.2026: Tiefe verschachtelter Proxy-Weiterleitungen
    /// (`proxy.rs`, M3).
    pub(crate) proxy_tiefe: u32,
    pub(crate) async_tiefe: u32,
    pub(crate) strikt: bool,
    /// Expliziter Wurzelstapel fuer GC (M6): temporaere Werte, die in
    /// Rust-Variablen leben, waehrend weiter alloziert wird.
    pub(crate) wurzeln: Vec<GVal>,
    /// J21: **alle seit dem letzten Befehl neu belegten Heap-Slots.**
    ///
    /// GEMESSEN/GEFUNDEN, warum das sein muss (der tote Verweis aus der
    /// Abnahme 11:07, wikipedia.org): `dom_element` legt den Knoten an
    /// (`neues_objekt`), hält ihn in einer Rust-Variablen und ruft dann
    /// `text_wert`/`versteckt_setzen` — beides alloziert und kann einen GC
    /// ausloesen. Der Knoten war in diesem Moment **weder auf dem Stapel
    /// noch in einer Umgebung**: die Markierung fand ihn nicht, der Sweep
    /// raeumte seinen Slot, und das naechste `holen_mut` sagte
    /// „Heap: toter Verweis (interner Fehler)" — im Fenster als
    /// „JS-Absturz (intern)".
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum die Liste erst **am Befehlswechsel**
    /// geleert wird und nicht schon im GC: zwischen zwei Allokationen
    /// kann ein zweiter GC laufen (`neues_objekt` → `vor_allok`,
    /// `text_wert` → `vor_allok`, `versteckt_setzen` → `vor_allok` — drei
    /// moegliche Sammelpunkte fuer **einen** Knoten). Wuerde `sammeln` die
    /// Liste leeren, waere der Knoten beim zweiten GC wieder unmarkiert und
    /// stuerbe genau so. Am **Befehlswechsel** ist die Lage dagegen
    /// entschieden: was der Befehl erzeugt hat, liegt dann auf dem Stapel,
    /// in einer Umgebung, in einem Timer oder in einer Host-Tabelle — also
    /// in einer echten Wurzel; was nirgends liegt, ist Muell und darf
    /// sterben.
    ///
    /// Die Liste ist damit **kurz** (höchstens die Allokationen eines
    /// Befehls) und der Preis ist ein `push` je Allokation — gegenueber
    /// `belegen` (Kostenberechnung, Grenzpruefung, Slotbuch) vernachlaessigbar.
    pub(crate) frisch: Vec<GVal>,
    /// Release 09.10.2026: ein GC ist faellig und laeuft am naechsten
    /// Befehlswechsel (siehe `vor_allok`).
    pub(crate) gc_faellig: bool,
    /// Release 09.10.2026: `quelltext_ausfuehren` im GLOBALEN Bereich (fuer
    /// `new Function`, ES2023 20.2.1.1.1 CreateDynamicFunction).
    pub(crate) eval_global: bool,
    /// `__narsilRunScript`: the next `quelltext_ausfuehren` is a classic
    /// script (strict code keeps no environment of its own).
    pub(crate) skript_global: bool,
    /// Release 10.10.2026: die Helfer des Vorspanns fuer asynchrone
    /// Generatoren und `for await` (siehe `asyncgen.rs`), GC-Wurzeln.
    pub(crate) hilfen: Vec<HeapRef>,
    /// Release 15.10.2026: GC runs so far, and the position cache of the
    /// last string read by index (`text_einheiten::TextZeiger`).
    pub(crate) gc_laeufe: u64,
    pub(crate) text_zeiger: Option<crate::text_einheiten::TextZeiger>,
    /// Release 10.10.2026: abgelehnte Versprechen OHNE Handler (HTML §8.1.7.3
    /// „notify about rejected promises"); nach dem Abarbeiten der Mikrotasks
    /// meldet die Konsole „Uncaught (in promise) …" fuer die, die bis dahin
    /// keinen Handler bekommen haben. GC-Wurzeln bis zur Meldung.
    pub(crate) unbehandelt: Vec<HeapRef>,
    /// Release 10.10.2026: meldet der Host unbehandelte Ablehnungen? Die
    /// Meldung ist HTML-Verhalten (ECMAScript kennt nur den
    /// HostPromiseRejectionTracker) — die Seiten-Engine schaltet sie an
    /// (`unbehandelte_ablehnungen_melden`), der blanke Motor schweigt.
    pub(crate) melde_unbehandelt: bool,
    /// Interrupt flag set by the host from another thread (a worker's
    /// `terminate()`): checked with the wall clock every 4096 steps, so a
    /// running script ends with a time-limit error.
    pub(crate) interrupt: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The value of the last exception nobody caught (script, timer or
    /// handler), for the host's "report an exception" (HTML §8.1.4.6):
    /// `__narsilTakeUncaught()` hands it out once. A GC root while held.
    pub(crate) letzter_wurf: Option<GVal>,
    /// The arrays `Array.prototype.join` is joining right now: an array that
    /// contains itself joins as "" instead of recursing without end (what
    /// every browser engine does).
    pub(crate) join_stapel: Vec<HeapRef>,
    /// `__narsilMicrotaskCheckpoint()` is running (not reentrant).
    pub(crate) checkpoint_aktiv: bool,
    /// Release 09.10.2026: `Op::EvalDirekt` stand vor dem laufenden Aufruf
    /// (siehe `n_eval`).
    pub(crate) eval_direkt: bool,
    /// Release 09.10.2026: je Textkonstante (`Op::Text`) EIN Heap-Text, beim
    /// ersten Lesen angelegt. GEMESSEN vorher: jedes Auswerten eines
    /// Literals kopierte den Text und legte ein neues Objekt an (youtube.com:
    /// 1,27 Mio `Op::Text` zu ~440 ns). Texte sind unveraenderlich, also ist
    /// das Teilen nicht beobachtbar. Die Eintraege sind GC-Wurzeln.
    pub(crate) text_werte: Vec<GVal>,
    /// Release 09.10.2026: wiederverwendbare Listen fuer Argumente und Temps
    /// der Rahmen (`Engine::pool_holen`). GEMESSEN vorher: jeder Aufruf
    /// allozierte und befreite beide Listen.
    pub(crate) gval_pool: Vec<Vec<GVal>>,
    /// Release 09.10.2026: die Ergebnis-Texte von `typeof` ("undefined",
    /// "function", …) je einmal im Heap (GC-Wurzeln). GEMESSEN vorher: jedes
    /// `typeof` legte einen neuen Text an (youtube.com: `TypVonName` ~430 ns).
    pub(crate) typ_werte: Vec<(&'static str, GVal)>,
    /// Release 09.10.2026: `%IteratorPrototype%` (ES2023 27.1.2) — einmal
    /// angelegt (`Engine::iterator_basis`), GC-Wurzel.
    pub(crate) proto_iterator: Option<HeapRef>,
    /// Release 09.10.2026: Eigenschaftsschluessel -> Symbol (fuer
    /// `Object.getOwnPropertySymbols`), GC-Wurzeln.
    pub(crate) symbol_nach_schluessel: std::collections::HashMap<String, HeapRef>,
    /// Release 09.10.2026 (nur Diagnose bei `NARSIL_JS_GC_STRESS`): die
    /// Mindestlaenge der Wurzelliste je aktivem Rueckruf.
    pub(crate) wurzel_boden: Vec<usize>,
    /// Release 09.10.2026: waehrend ein Generator/`async`-Rumpf laeuft, liegen
    /// Stapel, Rahmen, Wurzeln und `frisch` des Aufrufers HIER (statt in
    /// Rust-Variablen) — der GC sieht sie. GEMESSEN vorher: ein GC im
    /// Generator raeumte alles weg, was nur der Aufrufer hielt (youtube.com:
    /// „Heap: toter Verweis", der Rueckruf eines `forEach` war tot).
    pub(crate) ausgelagert: Vec<(Vec<GVal>, Vec<crate::vm::Rahmen>, Vec<GVal>, Vec<GVal>)>,
    /// Var-Scope-Stapel (von `var` benutzt; die VM loest Namen ueber die
    /// Umgebungskette auf, der Stapel bleibt fuer Kompatibilitaet).
    pub(crate) var_scope_stapel: Vec<HeapRef>,
    /// Wurf des letzten Laufs (fuer `wurf_text` nach dem Lauf).
    pub(crate) geworfen: Option<GVal>,
    pub(crate) mikrotasks: Vec<Mikrotask>,
    pub(crate) timer: Vec<Timer>,
    pub(crate) naechste_timer_id: u32,
    /// Virtuelle Uhr in Millisekunden (A2). Startet bei 0 und wird beim
    /// Abarbeiten der Timer auf den naechsten Termin gestellt.
    pub(crate) uhr_ms: u64,
    /// Ids abgesagter Timer (A2). WICHTIG: `clearInterval` im laufenden
    /// Intervall-Callback trifft den Timer nicht mehr in der Liste (er ist
    /// gerade herausgenommen) — ohne diese Liste liefe er endlos weiter.
    pub(crate) geloeschte_timer: Vec<u32>,
    // --- A4: fetch ---
    /// Die Herkunft der Seite (Basis fuer relative URLs, Massstab der
    /// Herkunftsgrenze). Ohne Wert lehnt `fetch` jede Anfrage ab.
    pub(crate) herkunft: Option<crate::netz::Herkunft>,
    /// Release 10.10.2026: fremde Herkuenfte nach CORS statt verboten
    /// (`Engine::cors_erlauben`, nur die Seiten-Engine).
    pub(crate) cors_an: bool,
    /// Release 10.10.2026: Pfeile, die ein abgeleiteter Konstruktor VOR
    /// `super()` anlegte — (Rahmen-ID des Konstruktors, Pfeil). `super()`
    /// gibt ihnen das dann feste `this` (`Vm::pfeile_nach_super_binden`).
    pub(crate) pfeile_vor_super: Vec<(u64, crate::heap::HeapRef)>,
    /// Die Uebertragung. `None` heisst: `fetch` lehnt mit TypeError ab.
    pub(crate) netzbau: Option<Box<dyn crate::netz::Netzbau + Send>>,
    /// Laufende Netzaufgaben, durch `max_netz_aufgaben` gedeckelt.
    pub(crate) netz: Vec<crate::netz::NetzAuftrag>,
    /// Nummer der naechsten Anfrage (fuer `netz_ergebnis`).
    pub(crate) naechste_netz_id: u64,
    /// A6: Anmeldungen aus `addEventListener` (Reihenfolge = Anmeldung).
    pub(crate) hoerer: Vec<crate::ereignis::Hoerer>,
    // --- J1: DOM ---
    /// Das Dokument (ein Objekt des Heaps, `KEIN_REF` = noch keins).
    pub(crate) dom_dokument: Option<HeapRef>,
    /// Erzeugte Knoten je Lauf (Grenze `max_dom_knoten`, M4).
    pub(crate) dom_knoten: usize,
    /// Naechste Knotennummer (J2: der Host spricht Knoten ueber diese
    /// Nummer an — eine `HeapRef` waere nach einer Sammlung ungueltig).
    pub(crate) dom_naechste_knoten: u32,
    /// J50: je Rahmen ein Merkmal „steht gerade in einem `super`-Zugriff".
    /// Der alte Stapel lag auf der Engine (Stand 30.09.); die Tiefe ist
    /// ueber `MAX_GENERATOR_RAHMEN` begrenzt.
    pub super_marke: Vec<bool>,
    /// J3b: Zaehler fuer die Rahmen-Nummern (jeder Aufruf und jede
    /// Rueckkehr erhaelt eine neue; der Bytecode-Cache der Schleife
    /// richtet sich danach).
    pub(crate) naechste_rahmen_id: u64,
    /// J3: die Umgebungen der **offenen Bloecke**, ein Eintrag je
    /// `Op::BlockAuf`.
    ///
    /// GEMESSEN, warum es diese Liste geben muss: `BlockAuf` legt eine
    /// Umgebung an (und gibt sie als GC-Wurzel), `BlockAb` stellt die
    /// aeltere Umgebung wieder her — aber die Block-Umgebung selbst
    /// haengt danach an NICHTSEM mehr (bei einer Schleife wird sie in der
    /// naechsten Runde ersetzt). Ihre Wurzel blieb also fuer immer
    /// stehen: 20.000 Leer-Durchlaeufe ergaben 20.000 Wurzeln und einen
    /// Heap, aus dem der GC NICHTS freigeben konnte (4,4 MiB nach 20.000
    /// Durchlaeufen, 104 MiB nach `fib(26)`). Mit dieser Liste loest
    /// `BlockAb` genau die Umgebung, die `BlockAuf` angelegt hat.
    pub(crate) block_umgebungen: Vec<HeapRef>,
    /// J25: die Schritte **dieses Frames** (der Host setzt sie je Frame
    /// ueber `frame_beginnen` zurueck).
    pub(crate) frame_schritte: u64,
    /// J25: **wirksam?** Das Frame-Budget zaehlt erst, wenn der Host
    /// einmal `frame_beginnen` gerufen hat.
    ///
    /// GEMESSEN (der Grund fuer dieses Flag): mit sofortiger Wirkung brach
    /// `tests/j21_gc_wurzel.rs` ab — `breites_bild_bleibt_vollstaendig`
    /// (3000 Objekte + 20000 Muellobjekte) braucht rund 1,2 Mio Schritte,
    /// also mehr als das 400.000er Budget. Das ist aber **kein** Frame,
    /// sondern ein Stapellauf: ein Host, der den Bildaufbau nicht meldet,
    /// soll nicht gebremst werden. `narsil` (der Browser) ruft
    /// `frame_beginnen` je Bild — dort greift das Budget.
    pub(crate) frame_aktiv: bool,
    pub(crate) zufalls_zustand: u64,
    // Prototypen (als Heap-Refs, immer erreichbar ueber Globalobjekt).
    pub(crate) proto_objekt: HeapRef,
    pub(crate) proto_feld: HeapRef,
    pub(crate) proto_text: HeapRef,
    pub(crate) proto_zahl: HeapRef,
    pub(crate) proto_bool: HeapRef,
    pub(crate) proto_funktion: HeapRef,
    /// Release 10.10.2026: `%AsyncFunction.prototype%`,
    /// `%GeneratorFunction.prototype%`, `%AsyncGeneratorFunction.prototype%`
    /// (ES2023 27.7.3, 27.3.3, 27.4.3) — die [[Prototype]] der jeweiligen
    /// Funktionen; ihre `constructor` sind die (nicht globalen)
    /// Konstruktoren `AsyncFunction` & Co.
    pub(crate) proto_async_funktion: HeapRef,
    pub(crate) proto_generator_funktion: HeapRef,
    pub(crate) proto_async_generator_funktion: HeapRef,
    pub(crate) proto_fehler: HeapRef,
    pub(crate) proto_typfehler: HeapRef,
    pub(crate) proto_bereichsfehler: HeapRef,
    pub(crate) proto_syntaxfehler: HeapRef,
    pub(crate) proto_referenzfehler: HeapRef,
    pub(crate) proto_karte: HeapRef,
    pub(crate) proto_menge: HeapRef,
    /// W16 (JS, 04.10.2026): `ArrayBuffer.prototype`,
    /// `SharedArrayBuffer.prototype` und `DataView.prototype`.
    ///
    /// **ZWEI** Prototypen fuer die Puffer, nicht einer: GEMESSEN (node 22)
    ///     `new SharedArrayBuffer(4) instanceof ArrayBuffer`  ->  **false**
    /// Die beiden Klassen sind nach ES2023 25.1/25.2 getrennt, und ein
    /// gemeinsamer Prototyp wuerde genau diesen Unterschied aufheben.
    pub(crate) proto_puffer: HeapRef,
    pub(crate) proto_shared_puffer: HeapRef,
    pub(crate) proto_dataview: HeapRef,
    pub(crate) proto_datum: HeapRef,
    pub(crate) proto_versprechen: HeapRef,
    pub(crate) proto_regex: HeapRef,
    /// A4: Prototyp der `Response`-Objekte (`text`/`json`/`arrayBuffer`).
    pub(crate) proto_antwort: HeapRef,
    /// J23: Prototyp von `URL` bzw. `URLSearchParams`.
    pub(crate) proto_url: HeapRef,
    pub(crate) proto_params: HeapRef,
    /// Release 13.10.2026: `HTMLCollection.prototype` (aus `als_sammlung`) —
    /// fuer den Namenszugriff `sammlung.name` (WebIDL §3.9, `namedItem`).
    pub(crate) proto_html_sammlung: Option<HeapRef>,
    /// J24: `Symbol.prototype`, `WeakMap.prototype`, `WeakSet.prototype`.
    pub(crate) proto_symbol: HeapRef,
    /// J24: `BigInt.prototype` und der zugehoerige Konstruktor (die
    /// Konstruktoren der anderen Primitive liegen auch im GC-Wurzelsatz).
    pub(crate) proto_bigint: HeapRef,
    pub(crate) ctor_bigint: HeapRef,
    pub(crate) proto_weakmap: HeapRef,
    pub(crate) proto_weakset: HeapRef,
    /// J24: `Symbol.for`-Registry (Schluessel -> Symbol) und die
    /// wohlbekannten Symbole (Nummer -> Symbol).
    pub(crate) symbol_registry: std::collections::HashMap<String, HeapRef>,
    pub(crate) symbol_bekannt: Vec<(u64, HeapRef)>,
    /// J24: naechste eindeutige Symbol-Nummer.
    pub(crate) naechste_symbol: u64,
    /// A4: Prototyp der `headers`-Objekte (`get`/`has`/`entries`).
    pub(crate) proto_kopf: HeapRef,
    /// A6: Prototyp der Ereignisobjekte (`preventDefault` & Co.).
    pub(crate) proto_ereignis: HeapRef,
    /// A6: Prototyp von `EventTarget` (`addEventListener` & Co.).
    pub(crate) proto_ereignisziel: HeapRef,
    // --- J1: Prototypen des DOM ---
    pub(crate) host_wrapper: std::collections::HashMap<(u32, u8, bool), HeapRef>,
    /// Release 2026-10-15: the `length` and indices read from live
    /// collections, valid while the host's `sammlung_stand` is the stamp
    /// stored with them. The values are host wrappers (rooted through
    /// `host_wrapper`) or primitives.
    pub(crate) sammlung_cache: (u64, std::collections::HashMap<HeapRef, (Option<GVal>, Vec<Option<GVal>>)>),
    /// Release 09.10.2026: Custom Elements — Prototyp der Klasse -> Name
    /// (`customElements.define`, gemeldet von `vorspann.js`).
    pub(crate) ce_protos: Vec<(HeapRef, String, String, HeapRef, HeapRef, bool)>,
    /// Die Baustapel der Definitionen (HTML §4.13.4 "construction stack"):
    /// `None` ist die Marke "schon gebaut".
    pub(crate) ce_bau: Vec<(HeapRef, Option<HeapRef>)>,
    /// Der Reaktions-Haken des Skripts (bekommt `[art, knoten, …]`-Listen).
    pub(crate) ce_haken: Option<HeapRef>,
    /// Verschachtelungstiefe der Reaktionen (Callbacks rufen DOM-Methoden).
    pub(crate) ce_tiefe: u32,
    /// Release 09.10.2026: Prototyp je Elementname (`div` ->
    /// `HTMLDivElement.prototype`, SVG als `svg:<name>`).
    pub(crate) dom_tag_protos: std::collections::HashMap<String, HeapRef>,
    /// Alle eingebauten DOM-Prototypen (dort endet die Suche nach Feldern
    /// einer Custom-Element-Klasse, siehe `ce_eigene_kette`).
    pub(crate) dom_eingebaut: std::collections::HashSet<HeapRef>,
    pub(crate) host_methoden: std::collections::HashMap<(String, u8), HeapRef>,
    host_callback_wurzeln: std::collections::HashSet<HeapRef>,
    pub(crate) letzte_fehlerstelle: Option<String>,
    pub(crate) proto_dom_html: HeapRef,
    pub(crate) proto_dom_image: HeapRef,
    pub(crate) proto_dom_element: HeapRef,
    pub(crate) proto_dom_text: HeapRef,
    pub(crate) proto_dom_dokument: HeapRef,
    pub(crate) proto_dom_stil: HeapRef,
    pub(crate) proto_dom_klasse: HeapRef,
    /// W6/V3: Prototyp der Generator-Objekte (`next`/`return`/`throw`).
    pub(crate) proto_generator: HeapRef,
    /// J36: Schluessel **und** Funktionswert des eingebauten
    /// `Array.prototype[Symbol.iterator]` — einmal beim Aufbau abgelegt.
    ///
    /// Zweck: der Schnellweg von `iter_werte` und `Op::MusterQuelle` darf
    /// den indizierten Weg nur nehmen, wenn `@@iterator` **dieser** Wert
    /// ist. GEMESSEN (node 22): `[][Symbol.iterator] === [].values` -> true.
    /// Wer `Array.prototype[Symbol.iterator]` ersetzt, muss auch im Muster
    /// und in `for…of` gelten — sonst nimmt der Motor den Indexweg und
    /// bemerkt die Ersetzung nie (`test/language/expressions/class/dstr/
    /// private-gen-meth-static-ary-ptrn-elem-id-iter-val-array-prototype.js`:
    /// `z` war `3` statt `42`).
    pub(crate) iter_schluessel_feld: String,
    pub(crate) iter_wert_feld: Option<HeapRef>,
    // Konstruktoren (fuer `instanceof`, Fehlertypen, `new` ohne Import).
    pub(crate) ctor_objekt: HeapRef,
    pub(crate) ctor_feld: HeapRef,
    pub(crate) ctor_text: HeapRef,
    pub(crate) ctor_zahl: HeapRef,
    pub(crate) ctor_bool: HeapRef,
    pub(crate) ctor_funktion: HeapRef,
    pub(crate) ctor_fehler: HeapRef,
    pub(crate) ctor_typfehler: HeapRef,
    pub(crate) ctor_bereichsfehler: HeapRef,
    pub(crate) ctor_syntaxfehler: HeapRef,
    pub(crate) ctor_referenzfehler: HeapRef,
    /// Lazily loaded web platform APIs and their state (`web_platform.rs`).
    pub(crate) platform: crate::web_platform::Platform,
    /// Release 15.10.2026: state of the ES2022-2026 built-ins (`es_modern.rs`).
    pub(crate) es: crate::es_modern::EsZustand,
}

impl Engine {
    /// Billiger Rohling ohne Aufbau — nur als Tauschpartner fuer den
    /// Thread-Umzug in ausfuehren() (wird nie benutzt, sofort ersetzt).
    pub(crate) fn roh() -> Self {
        Engine {
            grenzen: Grenzen::standard(),
            heap: Heap::neu(),
            global_umgebung: KEIN_REF,
            global_objekt: KEIN_REF,
            programm: None,
            module: crate::module::ModulLader::neu(),
            timer_beim_host: false,
            performance_marken: Vec::new(),
            texte_arc: std::sync::Arc::new(Vec::new()),
            inline_cache: Vec::new(),
            texte_index: std::collections::HashMap::new(),
            programm_bytes: 0,
            generator_startlauf: false,
            rahmen: Vec::new(),
            super_marke: Vec::new(),
            stapel: Vec::new(),
            aktiver_generator: None,
            lauf_ende: 0,
            nativ_tiefe: 0,
            native: HashMap::new(),
            nativ_proto: HashMap::new(),
            host_funktionen: HashMap::new(),
            haken: None,
            skripte: crate::debugger::SkriptTabelle::eine_datei(),
            lauf_datei: 0,
            host_objekte: None,
            konsole: Vec::new(),
            schritte: 0,
            start: Instant::now(),
            uhr_start: Instant::now(),
            browser_task_aktiv: false,
            aufruftiefe: 0,
            proxy_tiefe: 0,
            async_tiefe: 0,
            strikt: false,
            wurzeln: Vec::new(),
            // J21: frisch belegte Slots (siehe das Feld).
            frisch: Vec::new(),
            gc_faellig: false,
            eval_global: false,
            skript_global: false,
            hilfen: Vec::new(),
            gc_laeufe: 0,
            text_zeiger: None,
            unbehandelt: Vec::new(),
            melde_unbehandelt: false,
            interrupt: None,
            letzter_wurf: None,
            join_stapel: Vec::new(),
            checkpoint_aktiv: false,
            eval_direkt: false,
            text_werte: Vec::new(),
            gval_pool: Vec::new(),
            typ_werte: Vec::new(),
            proto_iterator: None,
            symbol_nach_schluessel: std::collections::HashMap::new(),
            wurzel_boden: Vec::new(),
            ausgelagert: Vec::new(),
            var_scope_stapel: Vec::new(),
            geworfen: None,
            mikrotasks: Vec::new(),
            timer: Vec::new(),
            naechste_timer_id: 1,
            uhr_ms: 0,
            geloeschte_timer: Vec::new(),
            herkunft: None,
            cors_an: false,
            pfeile_vor_super: Vec::new(),
            netzbau: None,
            netz: Vec::new(),
            naechste_netz_id: 1,
            hoerer: Vec::new(),
            dom_dokument: None,
            dom_knoten: 0,
            dom_naechste_knoten: 0,
            naechste_rahmen_id: 1,
            block_umgebungen: Vec::new(),
            frame_schritte: 0,
            frame_aktiv: false,
            zufalls_zustand: 0x1234_5678_9ABC_DEF1,
            proto_objekt: KEIN_REF,
            proto_feld: KEIN_REF,
            proto_text: KEIN_REF,
            proto_zahl: KEIN_REF,
            proto_bool: KEIN_REF,
            proto_funktion: KEIN_REF,
            proto_async_funktion: KEIN_REF,
            proto_generator_funktion: KEIN_REF,
            proto_async_generator_funktion: KEIN_REF,
            proto_fehler: KEIN_REF,
            proto_typfehler: KEIN_REF,
            proto_bereichsfehler: KEIN_REF,
            proto_syntaxfehler: KEIN_REF,
            proto_referenzfehler: KEIN_REF,
            proto_karte: KEIN_REF,
            // W16 (JS, 04.10.2026): ArrayBuffer / SharedArrayBuffer /
            // DataView. Bis `builtin.rs` die Prototypen anlegt, zeigen sie auf
            // `KEIN_REF` — ein nicht gebauter Prototyp, **kein** Phantom-
            // Objekt. Wichtig: `SharedArrayBuffer` hat einen **eigenen**,
            // weil node 22 `sab instanceof ArrayBuffer` -> `false` liefert.
            proto_puffer: KEIN_REF,
            proto_shared_puffer: KEIN_REF,
            proto_dataview: KEIN_REF,
            proto_menge: KEIN_REF,
            proto_datum: KEIN_REF,
            proto_versprechen: KEIN_REF,
            proto_regex: KEIN_REF,
            proto_antwort: KEIN_REF,
            // J23: URL / URLSearchParams
            proto_url: KEIN_REF,
            proto_params: KEIN_REF,
            proto_html_sammlung: None,
            // J24: Symbol / WeakMap / WeakSet / BigInt
            proto_symbol: KEIN_REF,
            proto_bigint: KEIN_REF,
            ctor_bigint: KEIN_REF,
            proto_weakmap: KEIN_REF,
            proto_weakset: KEIN_REF,
            symbol_registry: std::collections::HashMap::new(),
            symbol_bekannt: Vec::new(),
            naechste_symbol: 0,
            proto_kopf: KEIN_REF,
            proto_ereignis: KEIN_REF,
            proto_ereignisziel: KEIN_REF,
            host_wrapper: std::collections::HashMap::new(),
            sammlung_cache: (0, std::collections::HashMap::new()),
            ce_protos: Vec::new(),
            ce_bau: Vec::new(),
            ce_haken: None,
            ce_tiefe: 0,
            dom_tag_protos: std::collections::HashMap::new(),
            dom_eingebaut: std::collections::HashSet::new(),
            host_methoden: std::collections::HashMap::new(),
            host_callback_wurzeln: Default::default(),
            letzte_fehlerstelle: None,
            proto_dom_html: KEIN_REF,
            proto_dom_image: KEIN_REF,
            proto_dom_element: KEIN_REF,
            proto_dom_text: KEIN_REF,
            proto_dom_dokument: KEIN_REF,
            proto_dom_stil: KEIN_REF,
            proto_dom_klasse: KEIN_REF,
            proto_generator: KEIN_REF,
            iter_schluessel_feld: String::new(),
            iter_wert_feld: None,
            ctor_objekt: KEIN_REF,
            ctor_feld: KEIN_REF,
            ctor_text: KEIN_REF,
            ctor_zahl: KEIN_REF,
            ctor_bool: KEIN_REF,
            ctor_funktion: KEIN_REF,
            ctor_fehler: KEIN_REF,
            ctor_typfehler: KEIN_REF,
            ctor_bereichsfehler: KEIN_REF,
            ctor_syntaxfehler: KEIN_REF,
            ctor_referenzfehler: KEIN_REF,
            platform: Default::default(),
            es: Default::default(),
        }
    }

    /// Neue Engine mit den gegebenen Grenzen (J0: `Engine::neu(Grenzen)`).
    pub fn neu(grenzen: Grenzen) -> Self {
        let mut e = Engine::roh();
        e.grenzen = grenzen;
        e.grenzen.max_heap_bytes = e.grenzen.max_heap_bytes.min(MAX_HEAP_HART);
        e.grenzen.max_aufruftiefe = e.grenzen.max_aufruftiefe.min(MAX_AUFRUFTIEFE_HART).max(8);
        e.grenzen.max_mikrotasks = e.grenzen.max_mikrotasks.min(MAX_MIKROTASK_HART).max(8);
        // A4: die harten Netzgrenzen greifen auch bei `Grenzen` aus dem
        // Skript (M4) — sie duerfen nie groesser sein als die Vorgabe.
        e.grenzen.max_netz_aufgaben = e.grenzen.max_netz_aufgaben.min(MAX_NETZ_AUFGABEN_HART);
        e.grenzen.netz_antwort_bytes = e
            .grenzen
            .netz_antwort_bytes
            .min(MAX_NETZ_ANTWORT_HART)
            .max(1024);
        e.grenzen.netz_frist_ms = e.grenzen.netz_frist_ms.min(MAX_NETZ_FRIST_HART_MS).max(1);
        // A6: Anmeldungen je Motor (hart gedeckelt, M4).
        e.grenzen.max_hoerer = e.grenzen.max_hoerer.min(MAX_HOERER_HART).max(8);
        // J1: DOM-Grenzen (hart gedeckelt, M4/M5).
        e.grenzen.max_dom_knoten = e.grenzen.max_dom_knoten.min(MAX_KNOTEN_HART).max(16);
        // J3: die Wanduhr ist hart gedeckelt (M4) — ein Host darf einer
        // Seite langer rechnen lassen, aber nicht beliebig.
        e.grenzen.max_dauer_ms = e.grenzen.max_dauer_ms.min(MAX_DAUER_HART_MS).max(50);
        e.grenzen.max_dom_tiefe = e.grenzen.max_dom_tiefe.min(crate::dom::TIEFE_HART).max(4);
        e.grenzen.max_dom_html = e
            .grenzen
            .max_dom_html
            .min(crate::dom::MAX_HTML_HART)
            .max(1024);
        e.aufbauen();
        e
    }

    /// Registriert eine Host-Funktion als globale JS-Funktion (J0, fuer
    /// spaetere DOM-Anbindung: `document`-Attrappen in Tests).
    /// `Send` ist Pflicht: die Engine zieht zur Ausfuehrung in einen
    /// Kind-Thread mit grossem Stapel um (Koordination W5B).
    pub fn host_funktion<F>(&mut self, name: &str, f: F)
    where
        F: Fn(&[Wert]) -> Result<Wert, String> + Send + 'static,
    {
        self.host_funktionen.insert(name.to_string(), Box::new(f));
        let r = self.nativ_anlegen(name, Nativ::Host(name.to_string()), self.proto_funktion);
        self.global_binden(name, GVal::Ref(r));
    }

    /// Registriert die Haken fuer HOST-OBJEKTE (W6B, J9).
    ///
    /// Danach ist jedes Objekt mit dem Feld `__host` (einer Zahl) ein
    /// Host-Objekt: das Lesen und Schreiben seiner Eigenschaften geht
    /// an `haken`, nicht an die Feldliste.
    ///
    /// `Send + Sync` ist Pflicht (die Engine zieht in einen Kind-Thread
    /// um, `VERTRAG_JS`).
    pub fn host_objekt(&mut self, haken: std::sync::Arc<dyn crate::hostobjekt::HostObjektHaken>) {
        self.host_objekte = Some(haken);
    }

    /// Erzeugt ein Host-Objekt fuer die gegebene Id (W6B, J9).
    ///
    /// GEMESSEN, warum das die ENGINE macht und nicht der Aufrufer: das
    /// Objekt ist `{ __host: id }` — ein Objekt mit einem einzigen
    /// Feld. Wer es selbst baut, baut es irgendwann falsch (mit zwei
    /// Feldern, mit einem String statt einer Zahl), und der Fehler faellt
    /// dann als „Eigenschaft ist undefined" auf — weit weg von der
    /// Ursache.
    pub fn host_objekt_als_wert(&mut self, id: u32) -> Result<Wert, JsFehler> {
        if let Some(h) = self.host_objekte.clone() {
            // Die Engine fragt den Haken einmal selbst: ein Host-Objekt,
            // das der Haken nicht kennt, ist ein Programmierfehler und
            // wird hier benannt statt spaeter als `undefined`.
            let _ = h.lesen(id, crate::hostobjekt::HOST_MERKE);
        }
        Ok(Wert::Gegenstand(vec![(
            crate::hostobjekt::HOST_MERKE.to_string(),
            Wert::Zahl(id as f64),
        )]))
    }

    /// J6: der Wert fuer eine **benannte Methode** eines Host-Objekts
    /// (`knopf.meldung`), ohne dass der Host eine Tabelle mit Nummern je
    /// Methode bauen muss (siehe `Wert::host_methode`).
    pub fn host_methode_als_wert(&mut self, id: u32, name: &str) -> Wert {
        Wert::host_methode(id, name)
    }

    /// BINDET EINEN WERT als globale Variable (W6B, J9).
    ///
    /// # WARUM DAS KEIN `host_funktion` IST
    ///
    /// `host_funktion("document", …)` macht eine FUNKTION daraus. Eine
    /// Funktion kann Eigenschaften haben (`document.body` waere dann eine
    /// Eigenschaft der Funktion — `undefined`), aber nicht die des
    /// DOM-Hakens. GEMESSEN 14:45: mit `host_funktion` war `document` eine
    /// Funktion, `document.body` lieferte `undefined`, und die Konsole
    /// blieb leer (ReferenceError, kein Output).
    ///
    /// Also: der Wert wird gebaut (`import_wert`) und als Variable gebunden.
    /// Das ist der Weg, den eine globale Konstante geht.
    pub fn host_global(&mut self, name: &str, wert: Wert) -> Result<(), JsFehler> {
        let mark = self.wurzel_merke();
        let g = match self.import_wert(&wert, 0) {
            Ok(value) => value,
            Err(error) => { self.wurzel_zurueck(mark); return Err(error); }
        };
        self.wurzeln.push(g);
        self.global_binden(name, g);
        // The global object/environment now owns this value. Replacement must
        // not retain every historical event object on the temporary root stack.
        self.wurzel_zurueck(mark);
        Ok(())
    }

    // ------------------------------------------------------------------
    // W6D — HANDLER ALS FUNKTIONSWERT (`addEventListener('click', …)`)
    // ------------------------------------------------------------------

    /// Die Heap-Nummer einer GLOBALEN Funktion (W6D).
    ///
    /// # WOFUER
    ///
    /// Wer einen Handler speichern will, braucht die Nummer seines
    /// Funktionswerts. Ein Host bekommt sie ueber `Wert::Funktion`, aber
    /// ein TEST (oder eine Seite, die eine Funktion als Ereignis benutzt)
    /// hat oft nur den Namen. Ohne diese Methode muesste man `ausfuehren`
    /// mit `typeof` und einem Trick arbeiten — und die Nummer waere
    /// trotzdem nicht zu bekommen.
    ///
    /// # GEMESSEN, warum `None` statt einer Zahl 0
    ///
    /// `0` ist eine GEBRAUCHTE Heap-Nummer. Eine Funktion, die es nicht
    /// gibt, als `0` zu melden, hiesse: der Aufruf laeuft und scheitert
    /// irgendwo im_heap, weit weg von der Ursache. `None` sagt: der Name
    /// ist keine Funktion.
    ///
    /// # GEMESSEN (J2), warum die Umgebung dazugehoert
    ///
    /// Vorher stand hier NUR das globale Objekt — und damit war die
    /// Methode immer `None`: `function a(){}`, `var c = …` und
    /// `const b = …` legen ihre Bindung in der **globalen Umgebung** an,
    /// nicht als Eigenschaft des globalen Objekts (das ist die einzige
    /// Stelle, an der mein Motor von der Spec abweicht). Das Beispiel
    /// `examples/bruecke.rs` hat es gemeldet: Schritt 8 fand keine
    /// Funktion. Jetzt wird erst das globale Objekt, dann die
    /// Bindungskette der globalen Umgebung gelesen.
    pub fn funktionsnummer(&mut self, name: &str) -> Option<u32> {
        if let Ok(g) = self.hole_ohne_host(GVal::Ref(self.global_objekt), name)
            && let Some(r) = self.funktions_ref(g)
        {
            return Some(r);
        }
        // M3: Die Kette der Umgebungen ist durch `MAX_AUFRUFTIEFE_HART`
        // begrenzt — die globale Kette ist kurz, die Abfrage laeuft
        // aber sonst in einen Endlosfall, wenn die Kette kaputt ist.
        let mut umg = Some(self.global_umgebung);
        let mut reste = MAX_AUFRUFTIEFE_HART;
        while let Some(u) = umg {
            if reste == 0 {
                break;
            }
            reste -= 1;
            let mut naechste = None;
            if let HObj::Umgebung(daten) = self.heap.holen(u) {
                if let Some(b) = daten.bindungen.find(crate::heap::namens_hash(name), name)
                    && let Some(r) = self.funktions_ref(b.wert)
                {
                    return Some(r);
                }
                naechste = daten.eltern;
            }
            umg = naechste;
        }
        None
    }

    /// Die Heap-Nummer eines Funktionswerts (sonst `None`).
    fn funktions_ref(&self, g: GVal) -> Option<u32> {
        match g {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Funktion { .. } | HObj::Nativ { .. } => Some(r.0),
                _ => None,
            },
            _ => None,
        }
    }

    /// Haelt einen Funktionswert als GC-WURZEL fest (W6D).
    ///
    /// GEMESSEN, warum das noetig ist: ein `HeapRef` zeigt in den Heap,
    /// und der Heap wird bei einer Sammlung freigegeben. Der Handler ist
    /// eine CLOSUE ueber Variablen des Seitenskripts (`const z`); ohne
    /// Wurzel waere sie nach der ersten Sammlung weg und der Klick liefe
    /// ins Leere (oder auf ein fremdes Objekt mit derselben Nummer).
    ///
    /// M6: es wird nichts kopiert, nur eine Nummer behalten. M3: die Zahl
    /// der Wurzeln ist durch `max_heap_bytes` begrenzt; jeder Handler
    /// kostet eine Wurzel, und `MAX_HANDLER` (in `narsil`) begrenzt ihre
    /// Zahl je Seite.
    /// Synchronizes live external callback handles after a browser task.
    /// Removed listeners release their closures; VM scopes/temporary roots stay intact.
    pub fn host_callback_wurzeln_setzen(&mut self, handles: &[u32]) {
        self.host_callback_wurzeln = handles.iter().copied().map(HeapRef).collect();
    }

    /// Release 09.10.2026: ist `r` ein Rueckruf, den der Host haelt (und
    /// der darum als Wurzel lebt)? Nur solche Funktionen darf ein Host als
    /// `Wert::Funktion` an das Skript zurueckgeben (`dispatchEvent`).
    pub(crate) fn ist_host_rueckruf(&self, r: HeapRef) -> bool {
        self.host_callback_wurzeln.contains(&r) && self.heap.lebt(r)
    }

    pub(crate) fn host_argument(&mut self, value: GVal) -> Wert {
        let exported = self.gval_als_wert(value).unwrap_or_else(||self.export_wert(value, 0));
        fn roots(value: &Wert, into: &mut std::collections::HashSet<HeapRef>) {
            match value {
                Wert::Funktion(index) => { into.insert(HeapRef(*index)); },
                Wert::Feld(values) => { for value in values { roots(value, into); } },
                Wert::Gegenstand(fields) => { for (_, value) in fields { roots(value, into); } },
                _ => {}
            }
        }
        roots(&exported, &mut self.host_callback_wurzeln);
        exported
    }

    pub fn handler_halten(&mut self, index: u32) {
        self.wurzeln.push(GVal::Ref(crate::heap::HeapRef(index)));
    }

    /// Ruft einen gehaltenen Handler mit Werten auf (W6D).
    ///
    /// # DER WEG, und warum die Engine selbst
    ///
    /// Der Handler ist eine Closure ueber die Variablen des
    /// Seitenskripts. Diese Variablen leben in der GLOBALEN UMGEBUNG
    /// dieser Engine — also muss derselbe Aufruf hier laufen, in dem das
    /// Skript lief. GEMESSEN (W6D): mit einer frischen Engine je Klick
    /// war `z` (aus `const z`) nicht da; der Handler scheiterte an
    /// `z.textContent`, und die Seite blieb unveraendert.
    ///
    /// # DIE ARGUMENTE
    ///
    /// `args` sind oeffentliche `Wert`e; ein Objekt wird importiert
    /// (also: eine Kopie). Das reicht fuer das Ereignis-Objekt, weil
    /// `preventDefault` eine EIGENE Funktion im Objekt ist und nicht im
    /// Original nachgeschlagen werden muss (GEMESSEN: der Aufruf-Haken
    /// gibt `preventDefault` als Nummer zurueck, und `narsil` haengt die
    /// Funktion an).
    ///
    /// M5: die Aufruftiefe der Engine gilt auch hier (`RangeError`).
    pub fn handler_aufrufen(&mut self, index: u32, args: &[Wert]) -> Result<Wert, JsFehler> {
        self.handler_aufrufen_mit_this(index, args, &Wert::Undefiniert)
    }

    /// Ruft einen gehaltenen Handler mit dem vom Host bestimmten Empfaenger
    /// auf. Bei einem DOM-Listener ist `this` das aktuelle Ereignisziel.
    /// Host-Objekte behalten beim Import ihre Identitaet; gewoehnliche
    /// `Wert::Gegenstand`-Werte bleiben Momentaufnahmen.
    ///
    /// Gebundene Funktionen und Pfeilfunktionen behalten ihre JS-Regeln:
    /// `bind` ersetzt den Empfaenger, Pfeile nutzen ihr lexikalisches `this`.
    /// Nach dem Callback werden Mikrotasks begrenzt abgearbeitet. Timer
    /// bleiben beim Host, damit sie nicht innerhalb der Ereignisaufgabe laufen.
    pub fn handler_aufrufen_mit_this(
        &mut self,
        index: u32,
        args: &[Wert],
        this: &Wert,
    ) -> Result<Wert, JsFehler> {
        //
        // DER GROSSE STAPEL, und zwar vor allem (GEMESSEN 14:35, W6D):
        // der Handler ruft Skript, und Skript ruft Skript. Ohne den
        // 256-MiB-Thread bricht ein rekursiver Handler den Stapel des
        // Aufrufers (8 MiB im Hauptthread, 2 MiB im Testthread) ab, statt
        // mit `RangeError` zu enden.
        //
        // Die Argumente werden VOR dem Umzug kopiert: `Wert` ist
        // heapfrei, und ein Zeiger in den Heap waere nach dem Umzug
        // genau der Fehler, den W6D vermeiden soll.
        let argumente: Vec<Wert> = args.iter().take(MAX_HANDLER_ARGUMENTE).cloned().collect();
        let empfaenger = this.clone();
        self.auf_dickem_stapel(move |e| {
            e.handler_aufrufen_innen(index, &argumente, &empfaenger)
        })
            // Zwei Ebenen Fehler (Thread, dann Skript) -> eine (Meldung).
            .and_then(|r| r)
    }

    /// Der Aufruf selbst (laeuft im Kind-Thread, siehe `handler_aufrufen`).
    fn handler_aufrufen_innen(
        &mut self,
        index: u32,
        args: &[Wert],
        this: &Wert,
    ) -> Result<Wert, JsFehler> {
        // Im Legacy-Einstieg ist jeder Aufruf ein eigener Laufzeitabschnitt.
        // Eine Host-Task teilt dagegen eine Deadline ueber alle ihre Listener.
        if !self.browser_task_aktiv {
            self.start = Instant::now();
        }
        let r = crate::heap::HeapRef(index);
        // Die Wurzel, solange der Aufruf laeuft (M6): ein GC waehrend des
        // Aufrufs darf den Handler nicht einsammeln.
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(r));
        let empfaenger = match self.import_wert(this, 0) {
            Ok(g) => {
                self.wurzeln.push(g);
                g
            }
            Err(f) => {
                self.wurzel_zurueck(marke);
                return Err(f);
            }
        };
        // M4: die Argumente werden einzeln importiert; `import_wert`
        // begrenzt die Tiefe auf 32 und meldet zu tiefe Werte.
        // M3/M4: hoechstens `MAX_HANDLER_ARGUMENTE` (8) Argumente, und
        // der Vektor wird auf genau ihre Zahl bemessen (`min`).
        let mut gvals: Vec<GVal> = Vec::with_capacity(args.len().min(MAX_HANDLER_ARGUMENTE));
        for a in args.iter().take(MAX_HANDLER_ARGUMENTE) {
            match self.import_wert(a, 0) {
                Ok(g) => {
                    self.wurzeln.push(g);
                    gvals.push(g);
                }
                Err(f) => {
                    self.wurzel_zurueck(marke);
                    return Err(f);
                }
            }
        }
        if self.aufruftiefe >= self.grenzen.max_aufruftiefe {
            self.wurzel_zurueck(marke);
            return Err(JsFehler::AufrufZuTief(
                "Maximum call stack size exceeded".to_string(),
            ));
        }
        self.aufruftiefe += 1;
        let rahmen_vor = self.rahmen.len();
        let stapel_vor = self.stapel.len();
        let r2 = match self.entbinde(GVal::Ref(r), empfaenger, &gvals) {
            Ok((funktion, gebundenes_this, argumente)) => {
                self.wurzeln.push(funktion);
                self.wurzeln.push(gebundenes_this);
                self.wurzeln.extend_from_slice(&argumente);
                self.aufruf_synchron(funktion, gebundenes_this, &argumente, false)
            }
            Err(e) => Err(e),
        };
        self.aufruftiefe -= 1;
        // Ein abgebrochener Callback darf keine aktiven Rahmen oder Operanden
        // fuer den folgenden Listener hinterlassen.
        self.rahmen.truncate(rahmen_vor);
        self.stapel.truncate(stapel_vor);
        //
        // GEMESSEN, warum der Wurf zu `Laufzeit` wird (14:05): ein Handler,
        // der wirft, ist ein Fehler des Skripts, nicht der Seite. Die
        // Seite bleibt stehen, und die Meldung kommt in die Statuszeile
        // (`Angezeigt::konsole`).
        let out = match r2 {
            Ok(g) => {
                // Mikrotasks koennen allokieren und GC ausloesen, bevor der
                // Rueckgabewert als hostseitige Momentaufnahme exportiert wird.
                self.wurzeln.push(g);
                match self.schlange_abarbeiten() {
                    Some(f) => Err(f),
                    None => Ok(self.gval_als_wert(g).unwrap_or(Wert::Undefiniert)),
                }
            }
            Err(Stoerung::Fehler(f)) => Err(f),
            Err(Stoerung::Wurf(g)) => {
                self.letzter_wurf = Some(g);
                let text = self.wurf_text(g);
                // Auch ein JS-Wurf beendet die Aufgabe; bereits geplante
                // Promise-Reaktionen duerfen nicht bis zum naechsten Klick warten.
                let _ = self.schlange_abarbeiten();
                Err(JsFehler::Laufzeit(format!("Ausnahme im Handler: {text}")))
            }
        };
        self.rahmen.truncate(rahmen_vor);
        self.stapel.truncate(stapel_vor);
        self.wurzel_zurueck(marke);
        out
    }

    /// Gesammelte `console.log`-Ausgaben (statt stdout, J4).
    pub fn konsole(&self) -> &[String] {
        &self.konsole
    }

    /// Release 09.10.2026: das Schrittbudget fuer den naechsten Lauf neu
    /// beginnen (der Host gibt jedem Seitenskript sein eigenes).
    pub fn schritte_zuruecksetzen(&mut self) {
        self.schritte = 0;
    }

    /// Aktuell gezaehlte Heap-Bytes (fuer GC-Tests).
    pub fn heap_bytes(&self) -> usize {
        self.heap.bytes
    }

    /// J4: der **groesste** Byte-Stand, den der Heap je hatte.
    ///
    /// GEMESSEN, warum das daneben steht: `heap_bytes()` ist der Stand am
    /// Ende eines Laufs. Ein Leck zwischen zwei Sammellaeufen (der
    /// Kollektor laeuft erst alle 4096 Allokationen) ist daran nicht zu
    /// sehen. Mit der Spitze behauptet ein Test den tatsaechlichen
    /// Bedarf: bei 10^6 Zyklen 4,1 MiB, am Ende 275 KiB.
    pub fn heap_spitze_bytes(&self) -> usize {
        self.heap.spitze_bytes
    }

    /// J4: die Zahl der **belegten** Heap-Slots — die „lebenden" Objekte.
    ///
    /// Der Stand ist vom letzten Sammellauf; dazwischen koennen mehr
    /// Objekte da sein. `heap_spitze_bytes` ist die scharfe Groesse, das
    /// hier die lesbare: ein Skript, das 1.000.000 Objekte anlegt und
    /// wieder wegwirft, laesst diese Zahl klein.
    pub fn heap_objekte(&self) -> usize {
        self.heap.lebend_slots
    }

    /// Bisher verbrauchte Schritte (fuer Tests).
    pub fn schritte(&self) -> u64 {
        self.schritte
    }

    /// Fuehrt Quelltext aus (J0). Gibt den Vervollstaendigungswert des
    /// letzten Statements zurueck oder einen benannten Fehler. Danach
    /// werden Mikrotasks/Timer begrenzt abgearbeitet.
    ///
    /// Die Ausfuehrung laeuft in einem eigenen Thread mit 256 MiB Stapel
    /// (Koordination W5B): Tiefe 2000+ ist dadurch stapelsicher; der Stapel
    /// ist virtueller Speicher und wird nur bei Nutzung belegt. Die Engine
    /// zieht um und kommt mit dem Ergebnis zurueck (REPL-Zustand bleibt).
    /// Startfehler verlieren KEINEN Zustand (Kanal-Rueckholung).
    /// J7: ein **Quelltextfehler** (in `new Function(...)` oder `eval(...)`)
    /// wird als `SyntaxError` **geworfen** (sec-16.1.7, sec-18.6.1.1), nicht
    /// als Engine-Fehler zurueckgegeben — nur so kann ein `catch` ihn fangen.
    ///
    /// GEMESSEN: vorher brach `new Function('let =')` den ganzen Lauf ab
    /// (`Syntaxfehler 1:17: erwartet: Variablenname oder Muster`) und ein
    /// `try { new Function('let =') } catch (e) { … }` fing nichts
    /// (`tests/t262/faelle/err-function-syntax.js`). Andere Fehler
    /// (Speicher/Tiefe) bleiben Engine-Fehler.
    pub(crate) fn syntax_als_wurf(&mut self, e: JsFehler) -> Stoerung {
        match e {
            JsFehler::Syntax { nachricht, .. } => {
                Stoerung::Wurf(self.fehler_wert("SyntaxError", &nachricht))
            }
            andere => Stoerung::Fehler(andere),
        }
    }

    pub fn ausfuehren(&mut self, quelle: &str) -> Result<Wert, JsFehler> {
        let quelle = quelle.to_string();
        self.auf_dickem_stapel(move |e| e.ausfuehren_innen(&quelle))
            .and_then(|r| r)
    }

    /// ## W54 (JS, 05.10.2026) — DIE VIER TIMER-METHODEN DES HOSTS
    ///
    /// **`test --no-run` hat diese vier gefunden**, nicht `check`: die
    /// Testdatei `tests/j6_bruecke_ui.rs` ruft sie, und **keine davon war
    /// implementiert** — obwohl der Zustand (`timer`, `timer_beim_host`,
    /// `naechste_timer_id`, `geloeschte_timer`) **und die Kommentare, die sie
    /// beschreiben**, seit langem da waren. `interp.rs:248` sagt woertlich
    /// „`Engine::timer_einen_ausfuehren`, wenn er dran ist".
    ///
    /// **Das ist die Lektion von diesem ROT-Alarm, und sie ist zweimal teuer:**
    /// Ein `check` prueft die **Lib**. Ein `test --no-run` baut auch die
    /// **Tests** — und dort fand sich ein Aufruf auf eine Methode, die es nicht
    /// gibt. **Ab jetzt vor jedem Commit `test --no-run`.**
    ///
    /// GEMESSEN, was der Host sehen muss (aus `j6_bruecke_ui.rs`):
    /// ```text
    //  setTimeout(fn, 3000); setTimeout(fn, 0);   -> timer_anzahl() == 2
    //  timer_frueheste_ms()                       -> Some(1)  (J9: 0 ms -> 1 ms)
    //  timer_einen_ausfuehren()                   -> laesst **einen** laufen
    //  danach ist die Konsole  ["skript", " gleich"]
    // ```
    /// Der letzte Punkt ist der Grund fuer den hostgesteuerten Ablauf: **die
    /// Mikrotask-Schlange hinter dem Timer muss mitlaufen**, sonst sieht
    /// `console.log` im Callback nicht, was `Promise.resolve().then()` erzeugt
    /// hat.
    ///
    /// **TIMER BEIM HOST:** `timer_vom_host(true)` sagt dem Motor, dass er die
    /// Warteschlange **nicht** selbst leeren soll — sonst laeuft ein 3-Sekunden-
    /// Timer im selben Tick mit, und das Skript wartet nicht.
    pub fn timer_vom_host(&mut self, an: bool) {
        self.timer_beim_host = an;
    }

    /// Installs an interrupt flag: once another thread sets it, running
    /// script stops at the next check (every 4096 steps) with a time-limit
    /// error, and every later run stops the same way.
    pub fn set_interrupt_flag(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.interrupt = Some(flag);
    }

    pub(crate) fn interrupted(&self) -> bool {
        self.interrupt.as_ref().is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Serializes `value` like `postMessage` does (HTML §2.7.3) without a
    /// script: for host-made messages and tests.
    pub fn strukturiert_serialisieren(&mut self, quelle: &str) -> Result<crate::structured::SerializedMessage, JsFehler> {
        let w = self.ausfuehren(&format!("__narsilSerialize(({quelle}), [], [], [])"))?;
        match w {
            Wert::Zahl(h) => crate::structured::take(h as u32)
                .ok_or_else(|| JsFehler::Laufzeit("serialization lost".to_string())),
            _ => Err(JsFehler::Laufzeit("serialization failed".to_string())),
        }
    }

    /// Die WANDUHR in Motorzeit (ms seit `Engine::neu`).
    ///
    /// 06.10.2026: Die Timerschlange traegt absolute Weckzeiten
    /// (`Timer::bis_ms`); der Host vergleicht sie mit dieser Zahl, um zu
    /// entscheiden, ob ein Timer faellig ist. Ohne sie muesste der Host
    /// seine eigene Startzeit fuehren — eine zweite Uhr.
    /// Release 15.10.2026 (Agent C): the page's time origin in ms
    /// (`performance.now()`, `requestAnimationFrame` timestamps; HR Time 3
    /// §4). Monotonic over the engine's lifetime — `start` is reset at
    /// every task (it measures the script deadline). MEASURED before (WPT
    /// web-animations): `performance.now()` restarted near 0 in each task,
    /// so animation timelines never advanced.
    pub(crate) fn time_origin_ms(&self) -> f64 {
        self.uhr_start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn wanduhr_ms(&self) -> u64 {
        self.uhr_start.elapsed().as_millis().min(u64::MAX as u128) as u64
    }

    /// Die virtuelle Uhr auf die Wanduhr ziehen (nie rueckwaerts).
    ///
    /// GEMESSEN, warum der Host das braucht: `setTimeout(fn, 100)` rechnet
    /// `bis_ms = uhr_ms + 100`. Wer nur `timer_einen_ausfuehren` ruft,
    /// laesst Timer sofort laufen (die Uhr steht); erst dieser Aufruf macht
    /// aus „spaeter" ein „faellig".
    pub fn uhr_auf_wanduhr(&mut self) {
        self.uhr_ms = self.uhr_ms.max(self.wanduhr_ms());
    }

    /// Die virtuelle Motorzeit (ms).
    pub fn uhr_ms(&self) -> u64 {
        self.uhr_ms
    }

    /// Wie viele Timer **warten**. Der Host braucht die Zahl fuer seine
    /// eigene Schleife: solange sie nicht 0 ist, gibt es Arbeit.
    pub fn timer_anzahl(&self) -> usize {
        self.timer.len()
    }

    /// Wann der naechste Timer faellig ist — `None`, wenn keiner wartet.
    /// Der Host braucht **dieselbe** Zahl, die der Motor intern benutzt (J9:
    /// `setTimeout(cb, 0)` steht bei 1 ms, nicht bei 0 — sonst wuerde der
    /// Host in einer Dauerschleife 0-ms-Timer abfragen).
    pub fn timer_frueheste_ms(&self) -> Option<u64> {
        self.timer.iter().map(|t| t.bis_ms).min()
    }

    /// **Einen** Timer laufen lassen: den fruehesten nehmen, seinen Aufruf
    /// ausfuehren, die Mikrotask-Schlange dahinter abarbeiten.
    ///
    /// `false` heisst: **nichts wartet mehr** — das ist der Ausstiegspunkt
    /// fuer die Host-Schleife, und deshalb ein **Wert**, kein Fehler.
    pub fn timer_einen_ausfuehren(&mut self) -> Result<bool, JsFehler> {
        // Der frueheste; bei Gleichstand der mit der kleineren ID — das macht
        // die Reihenfolge **stabil**, und die ist bei `setTimeout(fn,0)` und
        // `setTimeout(fn,0)` der ganze Unterschied.
        let Some(idx) = self
            .timer
            .iter()
            .enumerate()
            .min_by_key(|(_, t)| (t.bis_ms, t.id))
            .map(|(i, _)| i)
        else {
            return Ok(false);
        };
        let t = self.timer.remove(idx);
        self.uhr_ms = self.uhr_ms.max(t.bis_ms);
        let marke = self.wurzel_merke();
        self.wurzeln.push(t.funktion);
        let args_fuer_aufruf: Vec<GVal> = if t.art == TimerArt::Leerlauf {
            let timed_out = t.leerlauf_timeout_ms.is_some_and(|end| self.uhr_ms >= end);
            let deadline = match self.neues_objekt(Some(self.proto_objekt)) {
                Ok(r) => r,
                Err(e) => { self.wurzel_zurueck(marke); return Err(e); }
            };
            self.wurzeln.push(GVal::Ref(deadline));
            // An idle turn has an 8 ms budget; elapsed wall time consumes it.
            let ende_ms = self.wanduhr_ms().saturating_add(if timed_out { 0 } else { 8 });
            let remaining = self.nativ_anlegen("timeRemaining", crate::builtin::Nativ::LeerlaufRestzeit { ende_ms }, self.proto_funktion);
            self.setze_eigen(deadline, "timeRemaining", GVal::Ref(remaining));
            self.setze_eigen(deadline, "didTimeout", GVal::Bool(timed_out));
            vec![GVal::Ref(deadline)]
        } else if t.art == TimerArt::Bild {
            // Release 09.10.2026: `requestAnimationFrame(cb)` ruft
            // `cb(DOMHighResTimeStamp)` (HTML §8.10) — derselbe Zeitstrahl wie
            // `performance.now()`. GEMESSEN vorher: das Argument fehlte, und
            // Planer, die damit ihre Frist rechnen (youtube.com), bekamen NaN.
            vec![GVal::Zahl(self.time_origin_ms())]
        } else {
            t.args.clone()
        };
        self.wurzeln.extend_from_slice(&args_fuer_aufruf);
        if let Some(iv) = t.intervall_ms {
            self.timer.push(crate::interp::Timer {
                bis_ms: t.bis_ms.saturating_add(iv),
                args: t.args.clone(),
                ..t
            });
        }
        let ergebnis = match self.aufruf_synchron(t.funktion, GVal::Undefiniert, &args_fuer_aufruf, false) {
            Ok(_) => match self.schlange_abarbeiten() {
                Some(e) => Err(e),
                None => Ok(true),
            },
            Err(Stoerung::Fehler(e)) => Err(e),
            Err(Stoerung::Wurf(g)) => {
                self.letzter_wurf = Some(g);
                // Release 09.10.2026: mit Stapel (`e.stack`), wenn es ihn gibt.
                let text = self.wurf_text_mit_stapel(g);
                Err(JsFehler::Laufzeit(format!("Ausnahme im Timer: {text}")))
            }
        };
        self.wurzel_zurueck(marke);
        ergebnis
    }

    /// Ohne Fensterhost laufen Timer nach der virtuellen Uhr, begrenzt.
    pub(crate) fn timer_autonom_abarbeiten(&mut self) -> Result<(), JsFehler> {
        if self.timer_beim_host { return Ok(()); }
        let mut anzahl = 0usize;
        while !self.timer.is_empty() {
            if anzahl >= self.grenzen.max_timer_ausfuehrungen {
                return Ok(());
            }
            self.timer_einen_ausfuehren()?;
            anzahl += 1;
        }
        Ok(())
    }

    /// J25: **ein neuer Frame beginnt** — der Schrittzähler des
    /// Frame-Budgets beginnt neu.
    ///
    /// GEMESSEN (der Grund fuer die Existenz dieser Methode): `max_schritte`
    /// gilt fuer den ganzen `ausfuehren`-Aufruf. Eine Seite mit einem
    /// Endlosskript blockierte damit **1905 ms** am Stueck — bei 60 Hz
    /// sind das 114 Bilder, die nicht gezeichnet wurden. Der Host (der
    /// weiss, wann ein Bild gebraucht wird) ruft vor jedem Frame diese
    /// Methode; ein Skript darf dann ueber viele Frames arbeiten, aber
    /// nicht in einem einzigen.
    ///
    /// Ein Aufruf von `ausfuehren` **ohne** vorheriges `frame_beginnen`
    /// verhaelt sich wie bisher — das Budget laeuft dann seit dem letzten
    /// Frame mit, und ein Host, der es nicht nutzt, merkt nichts (der
    /// Zaehler wird beim naechsten `frame_beginnen` ohnehin zurueckgesetzt).
    pub fn frame_beginnen(&mut self) {
        self.frame_schritte = 0;
        self.frame_aktiv = true;
    }

    /// J25: wie viele Schritte der **aktuelle Frame** bisher verbraucht hat
    /// (der Host kann das fuer eine Statuszeile nehmen).
    pub fn frame_schritte(&self) -> u64 {
        self.frame_schritte
    }

    /// Beginnt eine vom Browserhost gestartete Aufgabe (Ereignis, Timer oder
    /// Netzantwort). Der Host ruft dies einmal vor der gesamten Aufgabe auf,
    /// nicht vor jedem Listener. Deadline, Task-Schritte und das aktive
    /// Frame-Budget beginnen neu; alle Callbacks und Mikrotasks teilen sie.
    ///
    /// Heap, Realm, wartende Aufgaben, aktive VM-Rahmen und Operanden bleiben
    /// erhalten. Der Host verwendet diese Grenze ausserhalb laufenden JS-Codes
    /// und beendet sie nach dem letzten Callback/Checkpoint.
    pub fn browser_task_beginnen(&mut self) {
        self.start = Instant::now();
        self.schritte = 0;
        self.frame_schritte = 0;
        self.browser_task_aktiv = true;
    }

    /// Beendet die Host-Task ohne Zaehler oder VM-Zustand zu veraendern.
    /// Folgende Legacy-Einstiege messen ihre Laufzeit wieder selbst.
    pub fn browser_task_beenden(&mut self) {
        self.browser_task_aktiv = false;
    }

    /// FUEHRT EINEN AUFTRAG AUF DEM 256-MiB-STAPEL AUS (W6D).
    ///
    /// # WARUM DAS EIN EIGENER WEG IST
    ///
    /// GEMESSEN am 14:35: ein Aufruf dieser Engine braucht **26.800 Bytes**
    /// Rust-Stapel (Adresse einer lokalen Variabel in 401 verschachtelten
    /// Aufrufen; Test `crates/narsil/tests/_stapel_messen.rs`, danach
    /// geloschen). `max_aufruftiefe: 2000` verlangt damit rund 54 MiB —
    /// das passt in 256 MiB, aber in einen 2-MiB-Testthread (75 Aufrufe)
    /// und nicht in einen 8-MiB-Hauptthread (299).
    ///
    /// GEMESSEN, warum das W6D auffiel: `ausfuehren` legte sich schon
    /// vorher auf den grossen Thread, `handler_aufrufen` (der W6D-Weg)
    /// aber nicht — der lief auf dem Stapel des Aufrufers, also im
    /// Testthread. Der Test `ein_endlos_handler_endet_mit_rangeerror`
    /// brach mit `has overflowed its stack` ab, statt mit `RangeError` zu
    /// enden (M5: Rekursion nur mit Tiefengrenze — die Grenze muss da
    /// sein, BEVOR der Stapel weg ist).
    ///
    /// JEDER Weg in die Engine laeuft darueber: Skript, Handler und
    /// spaeter (W7) Bytecode. Sonst haengt die Sicherheit an der
    /// Aufrufstelle — und die ist nicht kontrollierbar.
    fn auf_dickem_stapel<T: Send + 'static>(
        &mut self,
        auftrag: impl FnOnce(&mut Engine) -> T + Send + 'static,
    ) -> Result<T, JsFehler> {
        type Auftrag<T> = Box<dyn FnOnce(&mut Engine) -> T + Send>;
        let (tx_hin, rx_hin) = std::sync::mpsc::channel::<(Engine, Auftrag<T>)>();
        let kind = std::thread::Builder::new()
            .name("narsil-js".to_string())
            .stack_size(STAPEL_LAUF)
            .spawn(move || match rx_hin.recv() {
                Ok((mut e, f)) => {
                    let r = f(&mut e);
                    (e, Some(r))
                }
                Err(_) => (Engine::roh(), None),
            });
        let k = match kind {
            Ok(k) => k,
            Err(e) => {
                return Err(JsFehler::Laufzeit(format!("JS-Thread nicht startbar: {e}")));
            }
        };
        // Erst JETZT umziehen (Start ist gelungen).
        let wiederherstell_grenzen = self.grenzen.clone();
        let mut tausch = Engine::roh();
        std::mem::swap(self, &mut tausch);
        // Senden scheitert nur, wenn das Kind schon tot ist — dann steht
        // die Engine in der Fehlermeldung und wird zurueckgeholt.
        if let Err(zurueck) = tx_hin.send((tausch, Box::new(auftrag))) {
            let (e, _) = zurueck.0;
            *self = e;
            return Err(JsFehler::Laufzeit(
                "JS-Thread vor Start gestorben (intern)".to_string(),
            ));
        }
        drop(tx_hin);
        match k.join() {
            Ok((e_zurueck, Some(r))) => {
                *self = e_zurueck;
                Ok(r)
            }
            Ok((_, None)) => Err(JsFehler::Laufzeit("Startkanal zu (intern)".to_string())),
            // Kind-Panik (Bug, kein Skript-Fehler): Panic-Text als Fehler
            // zurueckmelden statt den Host zu reissen (Koordination W5C).
            // (Echter Stapelueberlauf bräche ab — die Tiefengrenze und der
            // 256-MiB-Stapel verhindern ihn.)
            Err(panik) => {
                // Der Kind-Thread hat die alte VM verloren. Eine rohe Engine
                // enthaelt ungueltige Prototypreferenzen und darf nie zum Host
                // zurueckkehren. Initialisierten Zustand und Grenzen herstellen.
                *self = Engine::neu(wiederherstell_grenzen);
                Err(JsFehler::Laufzeit(format!(
                    "JS-Absturz (intern): {}", panik_text(panik)
                )))
            },
        }
    }

    /// Eigentliche Ausfuehrung (laeuft im Kind-Thread, siehe ausfuehren).
    ///
    /// W6: Lexen -> Parsen -> **Bytecode** -> VM-Schleife (kein Baumlauf).
    pub(crate) fn ausfuehren_innen(&mut self, quelle: &str) -> Result<Wert, JsFehler> {
        if !self.browser_task_aktiv {
            self.start = Instant::now();
        }
        self.lauf_ende = 0;
        let letzter = self.programm_ausfuehren(quelle);
        let out = match letzter {
            Ok(v) => v,
            Err(e) => {
                self.rahmen.clear();
                self.stapel.clear();
                if crate::vm::profil_an() {
                    crate::vm::profil_drucken();
                    crate::vm::profil_funktionen_drucken(self.programm.as_ref());
                }
                return Err(e);
            }
        };
        // Das Ergebnis bleibt waehrend allokierender Callbacks eine GC-Wurzel.
        let marke = self.wurzel_merke();
        self.wurzeln.push(out);
        let ergebnis = (|| {
            if let Some(e) = self.schlange_abarbeiten() { return Err(e); }
            self.timer_autonom_abarbeiten()?;
            Ok(self.export_wert(out, 0))
        })();
        self.wurzel_zurueck(marke);
        if crate::vm::profil_an() {
            crate::vm::profil_drucken();
            crate::vm::profil_funktionen_drucken(self.programm.as_ref());
        }
        ergebnis
    }

    /// Ein Schritt zaehlen + Grenzen pruefen (J5). Wird an jedem
    /// Schleifendurchlauf, Aufruf und jeder Anweisung aufgerufen.
    /// J28: `schritt()` ohne Grenzpruefung — fuer den Hakenlauf, der
    /// seine **eigene** Grenze hat (`vm.rs::haken_lauf`). Ohne das
    /// waere jeder Hakenlauf zweimal gezaehlt.
    pub(crate) fn schritt_bare(&mut self) -> Result<(), JsFehler> {
        self.schritte = self.schritte.wrapping_add(1);
        if self.schritte > self.grenzen.max_schritte {
            return Err(JsFehler::Zeitlimit(format!(
                "Schrittgrenze {} ueberschritten",
                self.grenzen.max_schritte
            )));
        }
        // J25: das Frame-Budget (siehe `schritt`).
        if self.frame_aktiv {
            self.frame_schritte = self.frame_schritte.wrapping_add(1);
            if self.frame_schritte > self.grenzen.frame_schritte {
                return Err(JsFehler::Zeitlimit(format!(
                    "Frame-Budget {} Schritte ueberschritten",
                    self.grenzen.frame_schritte
                )));
            }
        }
        // Wanduhr alle 4096 Schritte (siehe `schritt`).
        if self.schritte & 4095 == 0 && self.interrupted() {
            return Err(JsFehler::Zeitlimit("terminated".to_string()));
        }
        if self.schritte & 4095 == 0 && self.start.elapsed().as_millis() as u128 > WANDUHR_HART_MS {
            return Err(JsFehler::Zeitlimit(format!(
                "Wanduhr-Grenze ({WANDUHR_HART_MS} ms) ueberschritten"
            )));
        }
        Ok(())
    }

    pub(crate) fn schritt(&mut self) -> Result<(), JsFehler> {
        self.schritte = self.schritte.wrapping_add(1);
        if self.schritte > self.grenzen.max_schritte {
            return Err(JsFehler::Zeitlimit(format!(
                "Schrittgrenze {} ueberschritten",
                self.grenzen.max_schritte
            )));
        }
        // J25: das **Frame-Budget**. `max_schritte` gilt fuer den ganzen
        // `ausfuehren`-Aufruf; ein Skript darf aber nicht 2,5 s am Stueck
        // (50 Mio Schritte, ~150 Bilder bei 60 Hz) die Oberflaeche
        // blockieren. Der Host setzt es je Frame neu
        // (`frame_beginnen`).
        //
        // GEMESSEN: ohne diese Grenze kam `while (true) {}` erst nach
        // 1905 ms (der Wanduhr-Grenze) zurueck — das sind ~114 Bilder, die
        // nicht gezeichnet wurden. Jetzt: `Frame-Budget` nach
        // `frame_schritte` Schritten.
        if self.frame_aktiv {
            self.frame_schritte = self.frame_schritte.saturating_add(1);
        }
        if self.frame_aktiv && self.frame_schritte > self.grenzen.frame_schritte {
            return Err(JsFehler::Zeitlimit(format!(
                "Frame-Budget {} Schritte ueberschritten",
                self.grenzen.frame_schritte
            )));
        }
        // Wanduhr nur alle 4096 Schritte pruefen (billig, aber wirksam).
        if self.schritte & 4095 == 0 {
            if self.interrupted() {
                return Err(JsFehler::Zeitlimit("terminated".to_string()));
            }
            let grenze = (self.grenzen.max_dauer_ms as u128).min(WANDUHR_HART_MS);
            if self.start.elapsed().as_millis() > grenze {
                return Err(JsFehler::Zeitlimit(format!(
                    "Wanduhr-Grenze ({grenze} ms) ueberschritten"
                )));
            }
        }
        Ok(())
    }

    /// Release 09.10.2026 (Diagnose): ist der GC-Stress an, prueft der Motor
    /// zusaetzlich, ob Werte beim Eintritt in ein Natives noch leben.
    pub(crate) fn gc_stress_an(&self) -> bool {
        gc_stress() != usize::MAX
    }

    /// Heap-Limit pruefen, ggf. GC, sonst `Speicher`-Fehler (J5/M4).
    pub(crate) fn vor_allok(&mut self, kosten: usize) -> Result<(), JsFehler> {
        // J3c: **beide** Ausloeser — Zahl der Objekte ODER Zahl der Bytes.
        let byte_grenze = (self.grenzen.max_heap_bytes / GC_BYTES_ANTEIL).max(GC_BYTES_MINDEST);
        // Release 09.10.2026 (Diagnose): `NARSIL_JS_GC_STRESS=<n>` sammelt
        // schon nach n Allokationen — Wurzel-Fehler werden so reproduzierbar.
        let stress = gc_stress();
        // Release 09.10.2026: der regelmaessige GC laeuft NICHT mitten im
        // Befehl, sondern am naechsten Befehlswechsel (`vm.rs`, dort liegt
        // alles Lebende auf dem Stapel oder in einer Umgebung). GEMESSEN
        // vorher: `Op::FeldAnhaengen` nahm den Wert vom Stapel und rief
        // dann `vor_allok` — ein GC genau dort raeumte das neue Element weg
        // (`["a","b"]` hatte danach ein fremdes Objekt an Stelle 1;
        // youtube.com: „Heap: toter Verweis", „replace is not a function").
        // Nur die harte Grenze unten sammelt sofort.
        // Release 09.10.2026: die Zaehlgrenze waechst mit dem lebenden Heap
        // (gesammelt wird, wenn so viel neu belegt ist, wie lebt) — sonst
        // markierte JEDER GC nach 4096 Belegungen den ganzen Heap: bei
        // youtube.com (10,8 MB Skript, Hunderttausende lebende Objekte)
        // war das quadratisch.
        let zaehl_grenze = GC_ALLE_N_ALLOKS.max(self.heap.lebend_slots).min(stress);
        if self.heap.alloks_seit_gc > zaehl_grenze || self.heap.bytes_seit_gc > byte_grenze {
            self.gc_faellig = true;
        }
        // Release 15.10.2026: scripts stop a reserve short of the limit
        // (1/256 of it, at most 64 KiB), so a script that exhausted the heap
        // leaves room for the host's own work (error, reactions, the next
        // task) and the heap stays below the limit. MEASURED before (j25
        // `keine_bombe_frisst_den_heap_auf`): after a few more built-in
        // objects (MathMLElement, ...) the array bomb ended at 65 536 KiB,
        // exactly on the 64 MiB limit, instead of below it.
        let grenze = self
            .grenzen
            .max_heap_bytes
            .saturating_sub((self.grenzen.max_heap_bytes / 256).min(64 * 1024));
        if self.heap.bytes.saturating_add(kosten) > grenze {
            self.sammeln();
            if self.heap.bytes.saturating_add(kosten) > grenze {
                return Err(JsFehler::Speicher(format!(
                    "Heap-Grenze {} Bytes ueberschritten (Heap {})",
                    self.grenzen.max_heap_bytes, self.heap.bytes
                )));
            }
        }
        Ok(())
    }

    /// J21: EIN Belegpunkt fuer den ganzen Motor — der Slot landet in
    /// `frisch` und ist damit bis zum naechsten Befehl eine GC-Wurzel.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum das ein Trichter ist und nicht 29
    /// Aufrufe von Hand: `neues_objekt`, `text_wert`, `neues_feld`,
    /// `neues_funktion` und die uebrigen ~20 Stellen hatten **je eine eigene
    /// Zeile** `self.belege(...)`. Eine Stelle, die man beim Lesen
    /// uebersieht, ist genau der Fehler, den die Abnahme 11:07 gezeigt hat.
    /// Alle Belegungen des Motors laufen jetzt hier durch.
    pub(crate) fn belege(&mut self, obj: HObj, kosten: usize) -> HeapRef {
        let r = self.heap.belegen(obj, kosten);
        self.frisch.push(GVal::Ref(r));
        r
    }

    /// String-Laengen-Grenze pruefen (J5).
    pub(crate) fn string_grenze(&self, len: usize) -> Result<(), JsFehler> {
        if len > self.grenzen.max_string_laenge {
            return Err(JsFehler::Speicher(format!(
                "Zeichenkette zu lang ({len}, Grenze {})",
                self.grenzen.max_string_laenge
            )));
        }
        Ok(())
    }

    /// Mark-and-Sweep mit allen Wurzeln der Engine.
    pub(crate) fn sammeln(&mut self) -> usize {
        let mut wurzeln = self.wurzeln.clone();
        // J21: die seit dem letzten Befehl frisch belegten Slots sind Wurzeln
        // (Beweis und Grenze im Feld `frisch`). Die Liste wird hier **nicht**
        // geleert — das geschieht am Befehlswechsel (`schleife_innen`).
        wurzeln.extend(self.frisch.iter().copied());
        if let Some(g) = self.geworfen {
            wurzeln.push(g);
        }
        if let Some(g) = self.letzter_wurf {
            wurzeln.push(g);
        }
        for m in &self.mikrotasks {
            wurzeln.push(m.wert);
            if let Some(f) = m.reaktion_fn {
                wurzeln.push(GVal::Ref(f));
            }
            wurzeln.push(GVal::Ref(m.folge));
            // Release 09.10.2026: der wartende `async`-Rahmen (sein Container)
            // und der Wert, mit dem er fortgesetzt wird. GEMESSEN vorher: ein
            // GC zwischen `await` und Fortsetzung raeumte den Container weg.
            if let Some(g) = m.fortsetzung {
                wurzeln.push(GVal::Ref(g));
            }
            let (crate::vm::Aufnahme::Wert(w) | crate::vm::Aufnahme::Wurf(w)) = &m.aufnahme;
            wurzeln.push(*w);
        }
        for t in &self.timer {
            wurzeln.push(t.funktion);
            wurzeln.extend(t.args.iter().copied());
        }
        self.platform.roots(&mut wurzeln);
        self.es.roots(&mut wurzeln);
        // Release 10.10.2026: die EREIGNIS-HANDLER (`addEventListener`) und
        // ihre Ziele. Sie fehlten: eine direkt uebergebene Funktion
        // (`el.addEventListener('click', function () {…})`) hing NUR an
        // dieser Liste und war nach der naechsten Sammlung weg — der Klick
        // tat dann nichts. GEMESSEN mit `NARSIL_JS_GC_STRESS=32`:
        // `j6_bruecke_ui::handler_fehler_landet_in_der_konsole` („der zweite
        // Handler lief nicht"), auch schon vor dem 10.10.
        for h in &self.hilfen {
            wurzeln.push(GVal::Ref(*h));
        }
        for h in &self.unbehandelt {
            wurzeln.push(GVal::Ref(*h));
        }
        for h in &self.hoerer {
            wurzeln.push(GVal::Ref(h.funktion));
            if h.ziel != KEIN_REF {
                wurzeln.push(GVal::Ref(h.ziel));
            }
            if let Some(sig) = h.signal {
                wurzeln.push(GVal::Ref(sig));
            }
        }
        // Release 09.10.2026: die Versprechen laufender `fetch`-Anfragen. Die
        // Antwort kommt spaeter von aussen (`netz_ergebnis`); haelt das
        // Skript selbst keinen Verweis mehr, waere das Versprechen sonst
        // eingesammelt (siehe `n_fetch`).
        for a in &self.netz {
            wurzeln.push(GVal::Ref(a.versprechen));
        }
        // Release 09.10.2026: die geteilten Literal-Texte (`text_werte`).
        wurzeln.extend(self.text_werte.iter().copied().filter(|v| v.ist_ref()));
        wurzeln.extend(self.typ_werte.iter().map(|(_, v)| *v));
        wurzeln.extend(self.proto_iterator.map(GVal::Ref));
        wurzeln.extend(self.symbol_nach_schluessel.values().copied().map(GVal::Ref));
        // W6: die aktiven Rahmen sind Wurzeln (Umgebungen, `this`, Temps,
        // Argumente) — sonst wuerde ein GC mitten im Lauf eine Umgebung
        // einsammeln, die eine Closure noch braucht.
        for r in &self.rahmen {
            wurzeln.push(GVal::Ref(r.umg));
            // Release 15.10.2026: the generator / async function object a
            // running frame belongs to. MEASURED before (chip.de with
            // NARSIL_JS_GC_STRESS=64): an async function called from another
            // one ran its first part with its object referenced only by
            // `Rahmen::generator`; a GC swept it and its first `await`
            // panicked ("Heap: toter Verweis" in `erwartung`).
            if let Some(g) = r.generator {
                wurzeln.push(GVal::Ref(g));
            }
            // Release 09.10.2026: auch die Parameterumgebung (J48).
            if let Some(p) = r.param_umg {
                wurzeln.push(GVal::Ref(p));
            }
            // Release 10.10.2026: die Rumpfumgebung waehrend des Prologs.
            if let Some(p) = r.prolog_rumpf {
                wurzeln.push(GVal::Ref(p));
            }
            if let Some(h) = r.heim {
                wurzeln.push(GVal::Ref(h));
            }
            if let Some(c) = r.eltern_ctor {
                wurzeln.push(GVal::Ref(c));
            }
            wurzeln.push(r.this);
            wurzeln.push(r.neu);
            for t in &r.temps {
                wurzeln.push(*t);
            }
            for a in &r.args {
                wurzeln.push(*a);
            }
            // Release 09.10.2026: die Werte laufender `for-in`/`for-of`.
            for (werte, _) in &r.iter_lauf {
                wurzeln.extend(werte.iter().copied());
            }
            if let Some(crate::vm::Aufnahme::Wert(w)) = &r.aufnahme {
                wurzeln.push(*w);
            }
            if let Some(crate::vm::Aufnahme::Wurf(w)) = &r.aufnahme {
                wurzeln.push(*w);
            }
            // J7: vorgemerkte `finally`-Aktionen (Wert oder Wurf) sind
            // Wurzeln — sonst koennte ein GC den Rueckgabewert einsammeln,
            // waehrend der `finally`-Block laeuft.
            for a in &r.fertig {
                match a {
                    crate::vm::Aktion::Rueckgabe(v) | crate::vm::Aktion::Wurf(v) => {
                        wurzeln.push(*v);
                    }
                    crate::vm::Aktion::Springen(_) => {}
                }
            }
        }
        wurzeln.extend(self.stapel.iter().copied());
        // Release 09.10.2026: ausgelagerte Aufrufer (Generator/`async`).
        for (stapel, rahmen, w, frisch) in &self.ausgelagert {
            wurzeln.extend(stapel.iter().copied());
            wurzeln.extend(w.iter().copied());
            wurzeln.extend(frisch.iter().copied());
            for r in rahmen {
                crate::vm::rahmen_wurzeln(r, &mut wurzeln);
            }
        }
        // J10: Module, die zwischen zwei Laeufen im Lader liegen (Umgebung
        // + Exporte). Ohne das sammelt ein GC genau die weg, die das
        // naechste Modul beim Import noch braucht.
        crate::module::module_wurzeln_sammeln(&self.module, &mut wurzeln);
        if let Some(g) = self.aktiver_generator {
            wurzeln.push(GVal::Ref(g));
        }
        // Stabile Engine-Wurzeln (Koordination W5C): Prototypen und
        // Konstruktoren sind NUR aus Engine-Feldern/Tabellen erreichbar
        // (nicht aus dem Globalobjekt) — ohne diese Wurzeln fegt ein GC
        // nach ~4096 Alloks (z.B. fib(20)) die Prototypen weg und jede
        // spaetere Methodenauflösung läuft in tote Slots (Panic!).
        wurzeln.extend(self.host_wrapper.values().copied().map(GVal::Ref));
        // Release 09.10.2026: Custom Elements (Klassen-Prototypen, Baustapel, Haken).
        for (proto, _, _, basis, registry, _) in &self.ce_protos {
            wurzeln.extend([GVal::Ref(*proto), GVal::Ref(*basis), GVal::Ref(*registry)]);
        }
        wurzeln.extend(self.ce_bau.iter().map(|(p, _)| GVal::Ref(*p)));
        wurzeln.extend(self.ce_bau.iter().filter_map(|(_, e)| e.map(GVal::Ref)));
        wurzeln.extend(self.ce_haken.map(GVal::Ref));
        // Release 10.10.2026: Pfeile, die noch auf ihr `this` warten.
        wurzeln.extend(self.pfeile_vor_super.iter().map(|(_, p)| GVal::Ref(*p)));
        wurzeln.extend(self.dom_tag_protos.values().copied().map(GVal::Ref));
        wurzeln.extend(self.dom_eingebaut.iter().copied().map(GVal::Ref));
        wurzeln.extend(self.host_methoden.values().copied().map(GVal::Ref));
        wurzeln.extend(self.host_callback_wurzeln.iter().copied().map(GVal::Ref));
        // Release 10.10.2026: auch die Prototypen von Response, Headers, Event
        // und EventTarget — sie hingen bisher nur zufaellig (ueber die
        // Tabelle `native`) am Leben. GEMESSEN mit `NARSIL_JS_GC_STRESS=32`:
        // „Heap: toter Verweis" in `antwort_objekt` (tests/netz.rs).
        if let Some(d) = self.dom_dokument {
            wurzeln.push(GVal::Ref(d));
        }
        let stabil = [
            self.proto_antwort,
            self.proto_kopf,
            self.proto_ereignis,
            self.proto_ereignisziel,
            self.proto_objekt,
            self.proto_feld,
            self.proto_dom_html,
            self.proto_dom_image,
            self.proto_dom_element,
            self.proto_dom_text,
            self.proto_dom_dokument,
            self.proto_dom_stil,
            self.proto_dom_klasse,
            // J23: ohne diese Wurzel fegt ein GC die Prototypen von
            // `URL`/`URLSearchParams` weg und `new URL(...)` liefert danach
            // ein Objekt ohne Methoden (dieselbe Fehlerklasse wie W5C).
            self.proto_url,
            self.proto_params,
            // J24: ohne diese Wurzeln fegt ein GC die Prototypen weg und
            // `new Symbol()` liefert danach ein Objekt ohne Methoden.
            self.proto_symbol,
            self.proto_weakmap,
            self.proto_weakset,
            self.proto_bigint,
            self.proto_text,
            self.proto_zahl,
            self.proto_bool,
            self.proto_funktion,
            self.proto_fehler,
            self.proto_typfehler,
            self.proto_bereichsfehler,
            self.proto_syntaxfehler,
            self.proto_referenzfehler,
            self.proto_karte,
            self.proto_menge,
            // W53: die drei Byte-Daten-Prototypen waren **nie** in dieser
            // Liste. Ohne sie faellt ein GC sie weg, und danach hat ein
            // `new ArrayBuffer(8)` keinen Prototyp mehr — dieselbe Fehlerklasse
            // wie J23/J24 direkt darueber.
            self.proto_puffer,
            self.proto_shared_puffer,
            self.proto_dataview,
            self.proto_datum,
            self.proto_versprechen,
            self.proto_regex,
            self.proto_generator,
            self.ctor_objekt,
            self.ctor_feld,
            self.ctor_text,
            self.ctor_zahl,
            self.ctor_bigint,
            self.ctor_bool,
            self.ctor_funktion,
            self.ctor_fehler,
            self.ctor_typfehler,
            self.ctor_bereichsfehler,
            self.ctor_syntaxfehler,
            self.ctor_referenzfehler,
        ];
        for r in stabil {
            if r != KEIN_REF {
                wurzeln.push(GVal::Ref(r));
            }
        }
        // Release 10.10.2026: die Tabelle `native` ist KEINE Wurzel mehr — sie
        // ordnet nur zu, welcher Rust-Teil eine native Funktion ist. Vorher
        // hielt sie JEDE je erzeugte native Funktion fest und damit alles,
        // was daran haengt (resolve/reject eines jeden Promise samt Ergebnis,
        // `next` jedes Map/Set-Iterators …). GEMESSEN (youtube.com Suche,
        // `NARSIL_JS_HEAP_ZAEHLEN`): 104 407 native Funktionen lebend, der
        // Heap lief in die Grenze von 256 MB, danach brachen die Timer ab.
        // Die eingebauten haengen an Prototypen und Globalem; tote Eintraege
        // fallen direkt nach der Sammlung weg (unten), bevor ein Platz neu
        // belegt wird.
        // J24: `Symbol.for` und die wohlbekannten Symbole muessen leben,
        // sonst waeren sie nach einem GC weg (und `Symbol.for('x')` gaelbe
        // ein neues Symbol zurueck).
        for r in self.symbol_registry.values() {
            wurzeln.push(GVal::Ref(*r));
        }
        for (_, r) in &self.symbol_bekannt {
            wurzeln.push(GVal::Ref(*r));
        }
        for (k, v) in &self.nativ_proto {
            wurzeln.push(GVal::Ref(*k));
            wurzeln.push(GVal::Ref(*v));
        }
        // Umgebungskette: global + Eltern aller laufenden? Die aktuelle
        // Umgebung steckt in den Aufruf-Frames (Rust-Stapel) — NICHT
        // sichtbar. Deshalb haelt die Engine ZUSAETZLICH jede betretene
        // Umgebung in `wurzeln` (siehe `umgebung_betreten`), bis sie
        // verlassen wird. Global-Umgebung immer.
        wurzeln.push(GVal::Ref(self.global_objekt));
        let umgs = [self.global_umgebung];
        let n_wurzeln = wurzeln.len();
        let frei = self.heap.sammeln(&wurzeln, &umgs);
        // Release 15.10.2026: freed slots may be reused from now on.
        self.gc_laeufe += 1;
        self.text_zeiger = None;
        // Release 10.10.2026: eingesammelte native Funktionen aus der Tabelle
        // (siehe oben) — SOFORT, ein `HeapRef` ist nur der Platz.
        {
            let heap = &self.heap;
            self.native.retain(|r, _| heap.lebt(*r));
        }
        // Release 10.10.2026 (Diagnose): `NARSIL_JS_HEAP_ZAEHLEN=<MB>` druckt
        // nach jeder Sammlung ueber dieser Groesse die lebenden Objekte nach
        // Art und die groessten Wurzel-Quellen.
        static ZAEHLEN_AB: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        if let Some(ab) = *ZAEHLEN_AB.get_or_init(|| {
            std::env::var("NARSIL_JS_HEAP_ZAEHLEN").ok().and_then(|v| v.trim().parse::<usize>().ok())
        }) && self.heap.bytes >= ab * 1024 * 1024
        {
            eprintln!(
                "HEAP {} MB nach Sammlung (frei {frei}), Wurzeln {n_wurzeln}: wurzeln {} frisch {} native {} host_wrapper {} text_werte {} timer {} mikrotasks {} host_callbacks {}",
                self.heap.bytes / (1024 * 1024),
                self.wurzeln.len(),
                self.frisch.len(),
                self.native.len(),
                self.host_wrapper.len(),
                self.text_werte.len(),
                self.timer.len(),
                self.mikrotasks.len(),
                self.host_callback_wurzeln.len(),
            );
            for (art, n, b) in self.heap.zaehlung().into_iter().take(10) {
                eprintln!("HEAP   {art:<14} {n:>9} Objekte {:>9} KB", b / 1024);
            }
        }
        frei
    }

    /// Neue Kind-Umgebung anlegen (mit Heap-Limit) + als Wurzel parken.
    /// Release 09.10.2026: wie `umgebung_neu`, mit Platz fuer `n` Bindungen.
    pub(crate) fn umgebung_neu_kap(&mut self, eltern: Option<HeapRef>, n: usize) -> Result<HeapRef, JsFehler> {
        let obj = HObj::Umgebung(Umgebung {
            eltern,
            bindungen: crate::heap::Bindungen::mit_kapazitaet(n),
            objekt: None,
        });
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(r)
    }

    pub(crate) fn umgebung_neu(&mut self, eltern: Option<HeapRef>) -> Result<HeapRef, JsFehler> {
        let obj = HObj::Umgebung(Umgebung {
            eltern,
            bindungen: Default::default(),
            // J52: nur `with` setzt das (siehe `umgebung_mit_objekt`).
            objekt: None,
        });
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(r)
    }

    /// J52: Umgebung fuer `with (objekt) { … }` — dieselbe Kette, aber der
    /// Rumpf sieht **zuerst** das Objekt (ES2023 9.1.2.6
    /// `Object Environment Record`).
    pub(crate) fn umgebung_mit_objekt(
        &mut self,
        eltern: Option<HeapRef>,
        objekt: HeapRef,
    ) -> Result<HeapRef, JsFehler> {
        let obj = HObj::Umgebung(Umgebung {
            eltern,
            bindungen: Default::default(),
            objekt: Some(objekt),
        });
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(r)
    }

    /// Umgebung wieder vom Wurzelstapel nehmen (muss zur `umgebung_neu`
    /// passen — diszipliniert in Block/Aufruf-Auswertung).
    pub(crate) fn umgebung_entfernen(&mut self, r: HeapRef) {
        if let Some(p) = self.wurzeln.iter().rposition(|g| *g == GVal::Ref(r)) {
            if let Some(b) = self.wurzel_boden.last()
                && p < *b
            {
                panic!("GC-Diagnose: Umgebung unter dem Boden {b} entfernt (Stelle {p})");
            }
            self.wurzeln.remove(p);
        }
    }

    pub(crate) fn wurzel_merke(&self) -> usize {
        self.wurzeln.len()
    }

    pub(crate) fn wurzel_zurueck(&mut self, marke: usize) {
        if let Some(b) = self.wurzel_boden.last()
            && marke < *b
            && self.wurzeln.len() >= *b
        {
            panic!("GC-Diagnose: Wurzeln unter den Boden {b} gekuerzt (auf {marke})");
        }
        self.wurzeln.truncate(marke);
    }

    /// Bindung in dieser Umgebung vorhanden? (fuer `DefVar`)
    pub(crate) fn umgebung_hat(&self, umg: HeapRef, name: &str) -> bool {
        match self.heap.holen(umg) {
            crate::heap::HObj::Umgebung(env) => env
                .bindungen
                .find(crate::heap::namens_hash(name), name)
                .is_some(),
            _ => false,
        }
    }

    /// J3b: den Text zu einem Hash **nur fuer eine Meldung** — der heisse
    /// Weg kennt den Namen nicht mehr (er soll ihn nicht kopieren).
    pub(crate) fn name_fuer_text(&self, name: &str, hash: u64) -> String {
        if !name.is_empty() {
            return name.to_string();
        }
        if let Some(p) = &self.programm {
            // Release 09.10.2026: zuerst die LAUFENDE Funktion — dort steht der
            // Name fast immer (der Hash kam aus ihrem Namenspool). GEMESSEN
            // vorher: jedes Lesen einer globalen Variable aus einer Funktion
            // (`window`, `document`, `Math` …) durchlief die Namenslisten ALLER
            // Funktionen bis zum Treffer — bei youtube.com Zehntausende.
            if let Some(r) = self.rahmen.last()
                && let Some(f) = p.funktionen.get(r.fun)
                && let Some(i) = f.namen_hash.iter().position(|h| *h == hash)
                && let Some(n) = f.namen.get(i)
            {
                return n.clone();
            }
            for f in &p.funktionen {
                if let Some(i) = f.namen_hash.iter().position(|h| *h == hash)
                    && let Some(n) = f.namen.get(i)
                {
                    return n.clone();
                }
            }
        }
        String::new()
    }

    /// Oberster Var-Scope (Funktion/Global) fuer `var`-Bindungen.
    pub(crate) fn var_stapel(&self) -> &[HeapRef] {
        &self.var_scope_stapel
    }
    pub(crate) fn var_stapel_mut(&mut self) -> &mut Vec<HeapRef> {
        &mut self.var_scope_stapel
    }

    /// `DeleteBinding(N)` — ES2023 9.1.1.4.2, fuer `delete` auf einen
    /// **blossen Namen** (die Objektform laeuft ueber `Engine::loesche`).
    ///
    /// ## W13 (JS, 05.10.2026) — WARUM DAS EIGENES VERFAHREN IST
    ///
    /// GEMESSEN, vorher (`delete x` auf einen Namen):
    ///     `String(delete nie_dagewesen)` -> `ReferenceError: 'nie_dagewesen' ist
    ///                                        nicht definiert`   (Soll: `true`)
    ///     `var x=5; delete x; typeof x` -> `number`            (Soll: `undefined`)
    ///     `let y=5; delete y`            -> `true`             (Soll: `false`)
    ///     `const z=5; delete z`          -> `true`             (Soll: `false`)
    /// Ursache: der Compiler baute `delete x` als **Wertladung** plus `true`.
    /// Die Ladung wirft fuer einen nicht deklarierten Namen (der ReferenzError
    /// oben), und die Bindung zu loeschen gab es gar nicht, weil
    /// `Bindungen` kein `entfernen` hatte.
    ///
    /// Die Norm trennt drei Faelle, und alle drei stehen hier:
    ///   * **unaufloesbare Referenz** -> `true` (13.5.1.1 Schritt 3 und 6:
    ///     „If IsUnresolvableReference(ref) is true, return true"). Das ist
    ///     der test262-Fall `S11.1.6_A2_T2`.
    ///   * **Objekt-Umgebung** (`with`) -> die Bindung ist eine Eigenschaft,
    ///     also `DeletePropertyOrThrow` auf dem Objekt.
    ///   * **deklarativ** -> Schritt 3: nur eine `var`-Bindung ist loeschbar;
    ///     `let`/`const`/Funktions-/Klassendeklarationen liefern `false`.
    ///     (Im strikten Code waere es ein `TypeError` — dort greift aber schon
    ///     die Early-Error-Regel im Parser, parse.rs J35.)
    pub(crate) fn delete_binding(
        &mut self,
        umg: HeapRef,
        h: u64,
        name: &str,
    ) -> Result<bool, Stoerung> {
        let Some((wo, _)) = self.suche_h(umg, h, name) else {
            // Kein Treffer in der Umgebungskette. Im **globalen** Bereich kann
            // der Name trotzdem eine **Eigenschaft des globalen Objekts**
            // sein: so entsteht ein impliziter Globaler (`q = 1` ohne `var`,
            // ES2023 8.1.7 d.ii.2) — er hat bei uns **keine**
            // Umgebungsbindung, nur die Eigenschaft. Nach 9.1.1.4.2 ist die
            // Referenz trotzdem eine Environment-Referenz (sie laeuft ueber
            // den ObjectRecord des GlobalEnvironmentRecord), also greift
            // Schritt 4.a `DeleteProperty`.
            //
            // W15, GEMESSEN (node 22): `qg=1; delete qg` -> `true` und danach
            // `typeof qg` -> `undefined`. Ohne diesen Zweig gab es `true` und
            // danach `number`, denn es war nichts entfernt worden.
            if self.eigene_lese(self.global_objekt, name).is_some() {
                return self.loesche(GVal::Ref(self.global_objekt), name);
            }
            // Sonst wirklich unaufloesbar — der Aufrufer macht daraus `true`
            // (13.5.1.1 Schritt 3/6).
            return Ok(false);
        };
        // Objekt-Umgebung (`with`): loescht die **Eigenschaft**.
        let obj = match self.heap.holen(wo) {
            crate::heap::HObj::Umgebung(env) => env.objekt,
            _ => None,
        };
        if let Some(obj) = obj {
            return self.loesche(GVal::Ref(obj), name);
        }
        // Globaler Bereich: `GlobalEnv.DeleteBinding` Schritt 4.a — **nur**
        // wenn der Name eine **Eigenschaft des globalen Objekts** ist,
        // entscheidet `DeleteProperty`, und das beachtet `configurable`.
        //
        // W15 (JS, 04.10.2026), node 22 (v22.23.3) als Soll:
        //     `qg = 1; delete qg`          -> `true`  (configurable: true)
        //     `eval('var e=1'); delete e`   -> `true`  (configurable: true)
        //     `var v = 5; delete v`         -> `false` (configurable: false)
        //     `let l = 5; delete l`          -> `false` — `l` ist **keine**
        //                                        Eigenschaft des globalen
        //                                        Objekts, also greift
        //                                        Schritt 6 (deklarativ).
        if wo == self.global_umgebung && self.eigene_lese(self.global_objekt, name).is_some() {
            // GLOBALES `var` IST NICHT LOESCHBAR (ES2023, 9.1.1.4.16 +
            // 10.2.1.2.2 `DeleteBinding`): die Eigenschaft entsteht mit
            // `configurable: false`. GEMESSEN node 22:
            //     `var v=5; delete v` -> `false`, danach `typeof v` -> number.
            // Ohne diese Zeile lieferte unser `setze_eigen` eine
            // **veraenderliche** Eigenschaft, `delete v` gab `true`, und die
            // Bindung verschwand (Test `js10_delete_sollwerte`, Faelle 2a/2b,
            // 5a/5b). Eine Zuweisung OHNE `var` (`qg = 1`) hat keine Bindung
            // und bleibt loeschbar (`true`, 9.1.1.4.5).
            let ist_var_bindung = match self.heap.holen(wo) {
                crate::heap::HObj::Umgebung(env) => env
                    .bindungen
                    .find(h, name)
                    .is_some_and(|b| b.art == BindungsArt::Var),
                _ => false,
            };
            if ist_var_bindung {
                return Ok(false);
            }
            let r = self.loesche(GVal::Ref(self.global_objekt), name)?;
            if r {
                // Bindung und Eigenschaft sind hier dasselbe (8.1.7
                // `CreateGlobalVarBinding`) — sonst waere der Name nach
                // `delete qg` ueber die Bindung weiterhin aufloesbar.
                // GEMESSEN: node 22 liefert danach `undefined`.
                if let crate::heap::HObj::Umgebung(env) = self.heap.holen_mut(wo) {
                    env.bindungen.entfernen(h, name);
                }
            }
            return Ok(r);
        }
        // Deklarativ (9.1.1.4.2 Schritt 3): **immer `false`, und die Bindung
        // bleibt.** Es gibt hier KEINE Var/Let-Const-Unterscheidung — die
        // hatte ich in JS_10 erfunden (siehe W15 im Kopfkommentar).
        //
        // W15 (JS, 04.10.2026): mein gemeldeter Ist-Stand `var x=5; delete x
        // -> true` war **falsch**; node 22 (v22.23.3) sagt `false`, und
        // `typeof x` bleibt `number`.
        Ok(false)
    }

    // ------------------------------------------------------------------
    // Umgebungs-Zugriff
    // ------------------------------------------------------------------

    /// Bindung lexikalisch suchen. Gibt (Umgebung, initialisiert?) zurueck.
    ///
    /// J3b: die Variante mit `hash` ist der heisse Weg — sie braucht
    /// weder eine Kopie des Namens (`fn name(i) -> String`, eine
    /// Allokation je Zugriff) noch einen String-Hash. GEMESSEN: 56 ns je
    /// Zugriff davor, 3 bis 5 ns danach. Die Textvariante ist nur noch
    /// der bequeme Weg fuer Aufrufer ohne Bytecode-Index (Natives,
    /// Host, Fehlertexte).
    pub(crate) fn suche(&self, umg: HeapRef, name: &str) -> Option<(HeapRef, bool)> {
        self.suche_h(umg, crate::heap::namens_hash(name), name)
    }

    /// Wie [`Engine::suche`], aber mit dem vorberechneten Hash. `name` dient
    /// nur der Absicherung gegen einen Hash-Zusammenstoss und dem
    /// Fehlertext; ist er leer, wird **nur** nach dem Hash gesucht.
    /// J48: wie [`Engine::suche_h`], aber die Kette endet **bei** `grenze`
    /// (die Umgebung selbst wird noch geprueft).
    ///
    /// GEMESSEN/GEFUNDEN, der Anlass: die Suche „`var` ist funktionsweit"
    /// (beim Befehl `DefVar`) lief weiter in die **Parameterumgebung** und
    /// von dort in den **globalen** Bereich. Zwei Folgen:
    ///   `var args;
    ///    var f = function(x = args = arguments) { function arguments() {} };
    ///    f(); typeof args`
    ///   -> node `object`; ohne diese Grenze `function`, weil die hochgezogene
    ///   Deklaration des Rumpfs das `arguments` des **Skripts** ueberschrieb.
    ///   `var f = function(x = (a = arguments)) { var arguments = 5 }; f(); a`
    ///   -> node: das `arguments`-Objekt; ohne Grenze `undefined`.
    ///
    /// Ein `var` gehoert in die **varEnv des Rahmens** — nie darueber hinaus.
    pub(crate) fn suche_bis(
        &self,
        umg: HeapRef,
        grenze: HeapRef,
        hash: u64,
        name: &str,
    ) -> Option<(HeapRef, bool)> {
        let mut cur = Some(umg);
        // M3: Kettenlaenge ist durch Aufruftiefe + Blocktiefe gedeckelt;
        // zusaetzlicher Zaehldeckel gegen Pfadologie.
        let mut rest = 100_000;
        while let Some(u) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            let obj = self.heap.holen(u);
            if let HObj::Umgebung(env) = obj {
                // J52: `with` — erst das Objekt, dann die Bindungen.
                //
                // GEMESSEN (node 22), **dreissig** Zeilen; die zwei, die
                // diese Stelle bestimmen:
                //   `var o = {a: 1}; with (o) { a }`      -> `1`
                //   `var o = {}; with (o) { b = 5 }`      -> `o.b` bleibt
                //      `undefined`: ein Name, den das Objekt **nicht** hat,
                //      faellt an die aeussere Umgebung.
                if let Some(o) = env.objekt
                    && self.mit_hat(o, name)
                {
                    return Some((o, true));
                }
                let b = if name.is_empty() {
                    env.bindungen.find_h(hash)
                } else {
                    env.bindungen.find(hash, name)
                };
                if let Some(b) = b {
                    return Some((u, b.initialisiert));
                }
                if u == grenze {
                    break;
                }
                cur = env.eltern;
            } else {
                break;
            }
        }
        None
    }

    /// Release 09.10.2026: schneller Lesepfad — EIN Gang durch die Kette,
    /// der Wert kommt direkt aus der gefundenen Bindung. `None` heisst: der
    /// allgemeine Weg muss ran (`with`-Objekt, globales `var` mit
    /// Objekt-Spiegel, TDZ, nicht gefunden).
    ///
    /// GEMESSEN, warum: `lese_h` suchte erst die Umgebung (`suche_h`) und
    /// danach dieselbe Bindung noch einmal; jedes `LeseVar` ging damit zweimal
    /// durch Kette und Bindungsliste.
    #[inline]
    pub(crate) fn lese_schnell(&self, umg: HeapRef, h: u64) -> Option<GVal> {
        let mut cur = umg;
        for _ in 0..100_000 {
            let HObj::Umgebung(env) = self.heap.holen(cur) else { return None };
            if env.objekt.is_some() {
                return None;
            }
            if let Some(b) = env.bindungen.find_h(h) {
                if !b.initialisiert || (cur == self.global_umgebung && b.art == BindungsArt::Var) {
                    return None;
                }
                return Some(b.wert);
            }
            cur = env.eltern?;
        }
        None
    }

    /// Release 09.10.2026: schneller Schreibpfad (Gegenstueck zu
    /// `lese_schnell`). `false` heisst: der allgemeine Weg muss ran
    /// (`const`, TDZ, globales `var`, `with`, nicht gefunden).
    #[inline]
    pub(crate) fn schreibe_schnell(&mut self, umg: HeapRef, h: u64, wert: GVal) -> bool {
        let mut cur = umg;
        let mut ziel = None;
        for _ in 0..100_000 {
            let HObj::Umgebung(env) = self.heap.holen(cur) else { return false };
            if env.objekt.is_some() {
                return false;
            }
            if let Some(b) = env.bindungen.find_h(h) {
                if !b.initialisiert
                    || !b.veraenderbar
                    || (cur == self.global_umgebung && b.art == BindungsArt::Var)
                {
                    return false;
                }
                ziel = Some(cur);
                break;
            }
            match env.eltern {
                Some(e) => cur = e,
                None => return false,
            }
        }
        let Some(z) = ziel else { return false };
        if let HObj::Umgebung(env) = self.heap.holen_mut(z)
            && let Some(b) = env.bindungen.find_mut_h(h)
        {
            b.wert = wert;
            return true;
        }
        false
    }

    /// Release 12.10.2026: liegt in der Kette ein `with`-Objekt (eine
    /// Objekt-Umgebung)? Billig: keine Allokation, kein JS.
    pub(crate) fn kette_hat_mit(&self, umg: HeapRef) -> bool {
        let mut cur = Some(umg);
        let mut rest = 100_000;
        while let Some(u) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            match self.heap.holen(u) {
                HObj::Umgebung(env) => {
                    if env.objekt.is_some() {
                        return true;
                    }
                    cur = env.eltern;
                }
                _ => break,
            }
        }
        false
    }

    pub(crate) fn suche_h(&self, umg: HeapRef, hash: u64, name: &str) -> Option<(HeapRef, bool)> {
        let mut cur = Some(umg);
        // M3: Kettenlaenge ist durch Aufruftiefe + Blocktiefe gedeckelt;
        // zusaetzlicher Zaehldeckel gegen Pfadologie.
        let mut rest = 100_000;
        while let Some(u) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            let obj = self.heap.holen(u);
            if let HObj::Umgebung(env) = obj {
                let b = if name.is_empty() {
                    env.bindungen.find_h(hash)
                } else {
                    env.bindungen.find(hash, name)
                };
                if let Some(b) = b {
                    return Some((u, b.initialisiert));
                }
                cur = env.eltern;
            } else {
                break;
            }
        }
        None
    }

    pub(crate) fn lese(&mut self, umg: HeapRef, name: &str) -> Result<GVal, Stoerung> {
        self.lese_h(umg, crate::heap::namens_hash(name), name)
    }

    /// Release 10.10.2026: die Suche MIT `with`-Objekten (ES2023 9.1.1.2.1
    /// HasBinding eines Object Environment Record) — fuer einen Proxy ruft
    /// sie die `has`-Falle, darum `&mut`. `suche_h` (ohne JS) sah die
    /// Objekte gar nicht: GEMESSEN vorher `with ({a: 1}) { a }` -> „nicht
    /// definiert", und ein Proxy-Bereich (Alpine.js) fand `open` nicht und
    /// nahm `window.open`.
    pub(crate) fn suche_mit(
        &mut self,
        umg: HeapRef,
        hash: u64,
        name: &str,
    ) -> Result<Option<(HeapRef, bool)>, Stoerung> {
        let mut cur = Some(umg);
        let mut rest = 100_000;
        while let Some(u) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            let (objekt, eltern, treffer) = match self.heap.holen(u) {
                HObj::Umgebung(env) => {
                    let b = if name.is_empty() {
                        env.bindungen.find_h(hash)
                    } else {
                        env.bindungen.find(hash, name)
                    };
                    (env.objekt, env.eltern, b.map(|b| b.initialisiert))
                }
                _ => break,
            };
            if let Some(o) = objekt {
                let n = self.name_fuer_text(name, hash);
                let hat = if matches!(self.heap.holen(o), HObj::Proxy { .. }) {
                    self.proxy_has(o, &n)?
                } else {
                    self.mit_hat(o, &n)
                };
                if hat {
                    return Ok(Some((o, true)));
                }
            }
            if let Some(i) = treffer {
                return Ok(Some((u, i)));
            }
            cur = eltern;
        }
        Ok(None)
    }

    /// Wie [`Engine::lese`], aber mit vorberechnetem Hash (J3b).
    pub(crate) fn lese_h(&mut self, umg: HeapRef, hash: u64, name: &str) -> Result<GVal, Stoerung> {
        match self.suche_mit(umg, hash, name)? {
            Some((u, true)) => {
                let obj = self.heap.holen(u);
                match obj {
                    HObj::Umgebung(env) => {
                        let wert = if name.is_empty() {
                            env.bindungen.find_h(hash)
                        } else {
                            env.bindungen.find(hash, name)
                        };
                        if u == self.global_umgebung
                            && wert.is_some_and(|b| b.art == BindungsArt::Var)
                        {
                            let n = self.name_fuer_text(name, hash);
                            return self.hole_ohne_host(GVal::Ref(self.global_objekt), &n);
                        }
                        Ok(wert.map(|b| b.wert).unwrap_or(GVal::Undefiniert))
                    }
                    // J52: `with` — der Treffer ist das **Objekt** selbst.
                    // GEMESSEN: `var o = {get a(){ return 42 }};
                    // with (o) { a }` -> `42` (auch ein Getter zaehlt).
                    _ => {
                        let n = self.name_fuer_text(name, hash);
                        self.hole(GVal::Ref(u), &n)
                    }
                }
            }
            Some(_) => {
                // J3b: der Text kommt aus dem Hash, wenn der Aufrufer ihn
                // nicht mitgegeben hat (der heisse Weg kennt ihn nicht).
                let n = self.name_fuer_text(name, hash);
                Err(Stoerung::Wurf(self.fehler_wert(
                    "ReferenceError",
                    &format!("TDZ: '{n}' vor Initialisierung"),
                )))
            }
            None => {
                // J3b: **hier** wird der Name gebraucht — das Globalobjekt
                // wird nach Text schluesseldurchsucht (Host-Globals,
                // Builtins). GEMESSEN/GEFUNDEN: mit dem leeren Namen aus
                // dem Hash-Weg fand der Orakel **0 von 263** Programmen
                // (jedes las ein Global und bekam „nicht definiert").
                let schluessel = self.name_fuer_text(name, hash);
                let g = self.global_objekt;
                if self.eigene_lese(g, &schluessel).is_some() {
                    return self.hole_ohne_host(GVal::Ref(g), &schluessel);
                }
                // Lazily loaded platform globals (`WebAssembly`, …).
                if self.platform_pending() && self.platform_lazy_global(&schluessel)? {
                    return self.hole_ohne_host(GVal::Ref(g), &schluessel);
                }
                // J-CACHE (05.10.2026): `window.X = …` als GLOBALE Variable.
                //
                // GEMESSEN an www.youtube.com: das erste Skript schreibt
                // `window.ytcfg = {…}`, das naechste liest `ytcfg` als
                // **globale** Variable. Der Host legt die Eigenschaft am
                // `window`-Host-Objekt ab; der globale Namensraum kannte sie
                // nicht -> `ReferenceError: 'ytcfg' ist nicht definiert`,
                // und die Seite blieb ein Skelett.
                //
                // DIE REGEL ist ES2023, 9.1.1.4: der globale Namensraum ist
                // Object-Record **und** Declarative-Record. Eine Eigenschaft
                // des globalen Objekts ist damit eine globale Variable.
                // Gefragt wird NUR, ob der Host sie kennt (`Option`) — sonst
                // bleibt es beim ReferenceError, sonst waere jede
                // undeklarierte Variable still `undefined`.
                if let Some(v) = self.global_aus_window_host(&schluessel) {
                    return Ok(v);
                }
                // Release 15.10.2026: the global object inherits from
                // `Object.prototype` (ES2023 9.1.1.4.1: HasProperty on the
                // object record walks the chain), so a bare `toString`,
                // `hasOwnProperty` or `valueOf` is a global variable.
                // MEASURED before: chip.de's validator chunk
                // `{ normalizeEmail, toString, isSlug }` threw
                // `ReferenceError: 'toString' ist nicht definiert`.
                if self.proto_kette_hat(g, &schluessel) {
                    return self.hole_ohne_host(GVal::Ref(g), &schluessel);
                }
                Err(Stoerung::Wurf(self.fehler_wert(
                    "ReferenceError",
                    &format!("'{schluessel}' ist nicht definiert"),
                )))
            }
        }
    }

    /// Der Wert einer **`window`-Host-Eigenschaft** als globale Variable.
    ///
    /// J-CACHE (05.10.2026). GEMESSEN an www.youtube.com: `window.ytcfg = {…}`
    /// in einem Skript, `ytcfg` als globale Variable im naechsten.
    ///
    /// `None` heisst „der Host kennt den Namen nicht" — dann bleibt es beim
    /// `ReferenceError` der Aufrufstelle. Ein `Some` ist genau der Fall
    /// „das globale Objekt hat diese Eigenschaft" (ES2023, 9.1.1.4).
    fn global_aus_window_host(&mut self, name: &str) -> Option<GVal> {
        let w = self.eigene_lese(self.global_objekt, "window")?;
        if let GVal::Ref(r) = w {
            if let Some(value) = self.eigene_lese(r, name) { return Some(value); }
        }
        let id = self.host_id_von_gval(w)?;
        // Der Haken wird geklont, damit `self` nicht gleichzeitig geliehen
        // ist (dieselbe Begruendung wie in `hole`).
        let h = self.host_objekte.clone()?;
        match h.lesen(id, name) {
            Ok(Some(wert)) => Some(self.gval_aus_wert(wert)),
            _ => None,
        }
    }

    /// Zuweisung (lexikalisch oder global). Gibt den Wert zurueck.
    pub(crate) fn schreibe(
        &mut self,
        umg: HeapRef,
        name: &str,
        wert: GVal,
        strikt_modus: bool,
    ) -> Result<(), Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(wert);
        let r = self.schreibe_inner(umg, name, wert, strikt_modus);
        self.wurzel_zurueck(marke);
        r
    }

    pub(crate) fn schreibe_inner(
        &mut self,
        umg: HeapRef,
        name: &str,
        wert: GVal,
        strikt_modus: bool,
    ) -> Result<(), Stoerung> {
        self.schreibe_h(
            umg,
            crate::heap::namens_hash(name),
            name,
            wert,
            strikt_modus,
        )
    }

    /// Der heisse Schreibweg (J3b): der Hash kommt aus dem Bytecode, der
    /// Text nur fuer die Meldung. `name == ""` heisst: „der Aufrufer
    /// braucht den Namen nicht" — dann wird **nicht** verglichen, denn der
    /// Hash ist aus genau diesem Text gebildet.
    pub(crate) fn schreibe_h(
        &mut self,
        umg: HeapRef,
        hash: u64,
        name: &str,
        wert: GVal,
        strikt_modus: bool,
    ) -> Result<(), Stoerung> {
        match self.suche_mit(umg, hash, name)? {
            // Release 10.10.2026: `with`-Objekt — die Zuweisung ist ein
            // `[[Set]]` auf dem Objekt (ES2023 9.1.1.2.5 SetMutableBinding).
            Some((u, true)) if !matches!(self.heap.holen(u), HObj::Umgebung(_)) => {
                let n = self.name_fuer_text(name, hash);
                self.setze(GVal::Ref(u), &n, wert)
            }
            Some((u, true)) => {
                let obj = self.heap.holen_mut(u);
                if let HObj::Umgebung(env) = obj {
                    let b = env
                        .bindungen
                        .find_mut(hash, name)
                        .expect("Bindung wurde gerade gefunden");
                    if !b.veraenderbar {
                        let n = self.name_fuer_text(name, hash);
                        return Err(Stoerung::Wurf(
                            self.fehler_wert("TypeError", &format!("Konstante '{n}'")),
                        ));
                    }
                    let global_var = u == self.global_umgebung && b.art == BindungsArt::Var;
                    b.wert = wert;
                    if global_var {
                        let n = self.name_fuer_text(name, hash);
                        self.setze(GVal::Ref(self.global_objekt), &n, wert)?;
                    }
                    Ok(())
                } else {
                    unreachable!()
                }
            }
            Some(_) => {
                let n = self.name_fuer_text(name, hash);
                Err(Stoerung::Wurf(self.fehler_wert(
                    "ReferenceError",
                    &format!("TDZ: '{n}' vor Initialisierung"),
                )))
            }
            None => {
                // GEMESSEN, warum der leere Name hier nichts anlegt: der
                // heisse Weg (`schreibe_h` mit `name == ""`, siehe
                // `Op::SchreibeVar`) kennt den Namen nicht — er legte
                // darum `globalThis[""]` an und liess die Zuweisung danach
                // scheinbar verschwinden: `x = 5; globalThis.x` war
                // `undefined`, `x` danach ein ReferenceError. GEMESSEN
                // mit node 22 (`x = 5` in einem sloppy Skript → `5`).
                // Also: ohne Namen **kein** globales Feld anlegen, sondern
                // den Fehler melden — der Aufrufer wiederholt mit dem
                // Namen (der kalte Weg), und dort passiert die Anlage.
                if name.is_empty() {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "ReferenceError",
                        "Zuweisung an undeklarierte Variable (Name folgt im kalten Weg)",
                    )));
                }
                // Sloppy: globale erzeugen; strikt: ReferenceError.
                let n = self.name_fuer_text(name, hash);
                // Release 10.10.2026: `onload = f` & Co. — die Ereignis-
                // Handler sind Eigenschaften des globalen Objekts (das
                // Fenster, HTML §8.1.8.2), also auch im strikten Modus kein
                // ReferenceError, und die Zuweisung setzt den Handler.
                // GEMESSEN vorher (Web-Platform-Tests, `execution-timing`):
                // `onload = t.step_func(test)` legte eine gewoehnliche
                // globale Variable an, der Handler lief nie — Zeitgrenze.
                if n.len() > 2
                    && n.starts_with("on")
                    && n.bytes().all(|b| b.is_ascii_lowercase())
                    && let Some(fenster) = self.eigene_lese(self.global_objekt, "window")
                    && self.host_id_von_gval(fenster).is_some()
                {
                    return self.setze(fenster, &n, wert);
                }
                // A property of the global object (own or inherited) is the
                // binding of an object environment record: the assignment is
                // a [[Set]] on the global (ES2023 9.1.1.2.5 / 9.1.1.4.15), so
                // an accessor's setter runs — in sloppy and in strict code.
                // MEASURED before: `onmessage = f` in a worker replaced the
                // accessor of the event handler by a data property, and the
                // handler never ran.
                if self.hat_eigenschaft_ohne_getter(GVal::Ref(self.global_objekt), &n) {
                    return self.setze(GVal::Ref(self.global_objekt), &n, wert);
                }
                // J14: `strikt_modus` kommt vom Aufrufer und ist die
                // **Rahmenlage** (Skriptzuweisungen melden
                // `Engine::rahmen_strikt()`, interne Schreibzugriffe wie
                // `constructor` melden `false`). GEMESSEN mit node 22:
                //   function f(){ 'use strict'; y = 1 }  -> ReferenceError
                //   function g(){ y = 2 }                  -> `2` (global)
                if strikt_modus || self.strikt {
                    Err(Stoerung::Wurf(self.fehler_wert(
                        "ReferenceError",
                        &format!("Zuweisung an undeklarierte Variable '{n}' (strict)"),
                    )))
                } else {
                    // ## W12-NEBENWIRKUNG (JS2/QUIC, 04.45), ANGEKUENDIGT in
                    // WUNSCH_JS.txt. **Eine** von elf Aufrufern von
                    // `global_binden` war **nicht** „ein eingebautes Global“,
                    // sondern die **implizite Erzeugung** aus einer
                    // sloppy-Zuweisung.
                    //
                    // ECMA-262 8.1.7 d.ii.2: `{ writable: true, enumerable:
                    // **true**, configurable: false }`. GEMESSEN, `node 22`:
                    // `q2 = 5; Object.keys(globalThis).includes('q2')` -> `true`
                    // (und eine **Funktionsdeklaration** ist `false` — beide
                    // stimmen mit der Norm ueberein).
                    //
                    // **Warum genau hier und nicht im `new`-Zweig:** die
                    // Verzweigung oben trennt **strikt** (ReferenceError) von
                    // **sloppy** (globale Erzeugung). Der strikte Zweil darf
                    // `global_binden` nicht erreichen, und tut es nicht.
                    //
                    // **Nicht vollstaendig:** `setze_eigen` setzt
                    // `konfigurierbar: true`, die Norm sagt `false`. Das ist
                    // **offen** (kein Parameter vorhanden, und `setze_eigen`
                    // hat viele Aufrufer). Ich baue nicht mehr, als gemessen.
                    self.setze_eigen(self.global_objekt, &n, wert);
                    Ok(())
                }
            }
        }
    }

    /// J14: **interner** Schreibzugriff ohne Strict-Pruefung — fuer die
    /// Eigenarten des Motors (`constructor`, `prototype` beim Bau einer
    /// Funktion).
    ///
    /// GEMESSEN, warum das noetig war: `schreibe(.., strikt_modus = false)`
    /// prueft trotzdem `self.strikt`. In einem Skript mit `"use strict"`
    /// scheiterte der Bau einer Funktion darum an
    /// `ReferenceError: Zuweisung an undeklarierte Variable 'constructor'
    /// (strict)` — eine Motor-Eigenart, keine Skriptzuweisung.
    pub(crate) fn schreibe_intern(&mut self, ziel: GVal, name: &str, wert: GVal) {
        if let GVal::Ref(r) = ziel {
            self.setze_eigen(r, name, wert);
        }
    }

    /// Binde `name` in `umg` mit `wert` — **ein** Kettenlauf.
    ///
    /// J3b: ersetzt `umgebung_hat` + `deklariere` + `initialisiere`, also
    /// drei Laeufe durch die Bindungskette mit je einem Hash. Aufgerufen
    /// von `KopiereNamen`/`KopiereZurueck`, also **zweimal je Durchlauf**
    /// einer `for (let …)`-Schleife — dort war das der teuerste Teil der
    /// Schleife nach den Allokationen.
    pub(crate) fn binde_oder_ersetze(
        &mut self,
        umg: HeapRef,
        name: &str,
        wert: GVal,
        art: BindungsArt,
    ) -> Result<(), JsFehler> {
        let obj = self.heap.holen_mut(umg);
        if let HObj::Umgebung(env) = obj {
            let hash = crate::heap::namens_hash(name);
            match env.bindungen.find_mut(hash, name) {
                Some(b) => {
                    b.wert = wert;
                    b.initialisiert = true;
                }
                None => {
                    env.bindungen.einfuegen(
                        hash,
                        name,
                        Bindung {
                            wert,
                            veraenderbar: !matches!(art, BindungsArt::Const),
                            initialisiert: true,
                            art,
                        },
                    );
                    self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                }
            }
            Ok(())
        } else {
            Err(JsFehler::Laufzeit("Umgebung korrupt (intern)".to_string()))
        }
    }

    /// Deklariert in `umg` (ohne Duplikat-Pruefung — Aufrufer hisst).
    pub(crate) fn deklariere(
        &mut self,
        umg: HeapRef,
        name: &str,
        art: BindungsArt,
    ) -> Result<(), JsFehler> {
        {
            let obj = self.heap.holen_mut(umg);
            if let HObj::Umgebung(env) = obj {
                let hash = crate::heap::namens_hash(name);
                let vorhanden = env.bindungen.find(hash, name).is_some();
                env.bindungen.einfuegen(
                    hash,
                    name,
                    Bindung {
                        wert: GVal::Undefiniert,
                        veraenderbar: !matches!(art, BindungsArt::Const),
                        initialisiert: matches!(art, BindungsArt::Var),
                        art,
                    },
                );
                if !vorhanden {
                    self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                }
            } else {
                return Err(JsFehler::Laufzeit("Umgebung korrupt (intern)".to_string()));
            }
        }
        // CREATE-GLOBAL-VAR-BINDING (ES2023, 9.1.1.4.16): ein globales `var`
        // legt die Bindung **und** eine Eigenschaft am globalen Objekt an.
        //
        // GEMESSEN an www.youtube.com: `var ytcfg={…}` gefolgt von
        // `window.ytcfg.set(…)`. Die Bindung war da, `window.ytcfg` aber
        // **undefined** — `TypeError: kein Zugriff auf Undefiniert`, und
        // damit war `ytcfg` fuer jedes spaetere Skript weg
        // (`ReferenceError`). Die Gegenrichtung (Eigenschaft -> Bindung)
        // heilt `global_aus_window_host`; **diese** Zeile ist die
        // Richtung Bindung -> Eigenschaft.
        //
        // NUR wenn die Eigenschaft fehlt: `window.foo = 1; var foo;` darf
        // die 1 nicht auf `undefined` zuruecksetzen (9.1.1.4.16 d.ii).
        if art == BindungsArt::Var
            && umg == self.global_umgebung
            && self.eigene_lese(self.global_objekt, name).is_none()
        {
            self.setze_eigen(self.global_objekt, name, GVal::Undefiniert);
        }
        Ok(())
    }

    /// J8: `umgebung_hat` nur nach dem Hash (kein Text, keine Kopie).
    pub(crate) fn umgebung_hat_h(&self, umg: HeapRef, hash: u64) -> bool {
        match self.heap.holen(umg) {
            crate::heap::HObj::Umgebung(env) => env.bindungen.find_h(hash).is_some(),
            _ => false,
        }
    }

    /// J8: wie [`Engine::deklariere`], aber **ohne** den Namen zu kopieren —
    /// der Hash genuegt zum Finden.
    ///
    /// GEMESSEN, warum das noetig war: `Op::DefVar` holte den Namen als
    /// Kopie aus dem Namenspool (`fn name(i) -> String`) und legte ihn
    /// ein zweites Mal in der Bindung ab — zwei Allokationen **je
    /// Parameter je Aufruf**. Bei `fib(27)` waren das 785.670 Kopien fuer
    /// den einen Parameter `n`.
    ///
    /// Die Namen fehlen danach nur in den Umgebungen **oberhalb** des
    /// Globalrahmens: die werden nie aufzaehlt (`for-in` zaehlt keine
    /// Umgebungen), und Fehlermeldungen holen den Text aus dem Namenspool
    /// (`name_fuer_text`). Der Globalrahmen nimmt weiterhin den Weg mit
    /// Namen — `Object.keys(globalThis)` und `for-in` brauchen sie.
    pub(crate) fn deklariere_h(
        &mut self,
        umg: HeapRef,
        hash: u64,
        art: BindungsArt,
    ) -> Result<(), JsFehler> {
        let obj = self.heap.holen_mut(umg);
        if let HObj::Umgebung(env) = obj {
            let vorhanden = env.bindungen.find_h(hash).is_some();
            env.bindungen.einfuegen(
                hash,
                "",
                Bindung {
                    wert: GVal::Undefiniert,
                    veraenderbar: !matches!(art, BindungsArt::Const),
                    initialisiert: matches!(art, BindungsArt::Var),
                    art,
                },
            );
            if !vorhanden {
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            }
        }
        Ok(())
    }

    /// J8: `initialisiere` nur nach dem Hash.
    pub(crate) fn initialisiere_h(&mut self, umg: HeapRef, hash: u64, wert: GVal) {
        // J52 (zurueckgenommen 03.10., 14:40): der Objekt-Schreibweg fuer
        // `with` ist **weg** — er rief `setze` mit einem Ziel, das im
        // globalen Fall kein Heap-Slot ist, und alle 1971 Suite-Faelle
        // endeten mit „kein Setzen auf Primitiv“. Die Funktion bleibt
        // geparkt (siehe `team/NOTIZ_VM_VERLOREN.txt`), der Schreibweg
        // kommt erst mit dem ganzen `with` zurueck.
        let obj = self.heap.holen_mut(umg);
        if let HObj::Umgebung(env) = obj
            && let Some(b) = env.bindungen.find_mut_h(hash)
        {
            b.wert = wert;
            b.initialisiert = true;
        }
    }

    /// J52: hat das `with`-Objekt den Namen? (eigene **und** Prototype,
    /// ausser `Symbol.unscopables`.)
    ///
    /// **OHNE** den Text geht es nicht: der Schluessel `@@unscopables` ist ein
    /// **Symbol**, und nur mit dem Text kann der Motor es nachschlagen. Der
    /// heisse Weg (`name == ""`) bekommt darum **keinen** Treffer — im
    /// `with` gibt es keine Bindungen, die ohne Text sichtbar waeren.
    /// J52: hat das `with`-Objekt den Namen? (eigene **und** Prototype,
    /// ausser `Symbol.unscopables`.)
    ///
    /// **LESEND, ohne Getter** — `HasBinding` (ES2023 9.1.2.6) ruft keinen
    /// Getter auf, und `suche_h` hat nur `&self`.
    ///
    /// GEMESSEN (node 22): `var o = {get a(){ return 42 }}; with (o) { a }`
    /// -> `42` — der Getter **zaehlt** als vorhanden und wird erst beim Lesen
    /// gerufen.
    pub(crate) fn mit_hat(&self, obj: HeapRef, name: &str) -> bool {
        // Der Schluessel `@@unscopables` ist ein **Symbol**; nur mit dem Text
        // kann der Motor es nachschlagen. Ohne Text (heisser Weg) gibt es
        // darum keinen Treffer — im `with` gibt es keine Bindungen, die ohne
        // Text sichtbar waeren.
        if name.is_empty() {
            return false;
        }
        if !self.hat_eigenschaft_ohne_getter(GVal::Ref(obj), name) {
            return false;
        }
        // Release 15.10.2026: `Get(bindingObject, @@unscopables)` follows the
        // prototype chain (Array.prototype[@@unscopables]), the key is the
        // engine's well-known symbol key, and a name is excluded when its
        // value is truthy (ECMA-262 9.1.1.2.1 HasBinding steps 6-8).
        // MEASURED before: the lookup used the key "@sym7", which no object
        // has, so `with (el) { remove }` found el.remove although Element
        // marks it unscopable (event handler attributes run in with-scopes).
        let Some(GVal::Ref(liste)) = self.datenwert_in_kette(obj, "Symbol(Symbol.unscopables)") else {
            return true;
        };
        match self.datenwert_in_kette(liste, name) {
            Some(v) => !self.zu_bool(v),
            None => true,
        }
    }

    /// Release 15.10.2026: the value of a data property `name` on `obj` or
    /// its prototypes (no getters run; an accessor counts as absent).
    fn datenwert_in_kette(&self, obj: HeapRef, name: &str) -> Option<GVal> {
        let mut cur = Some(obj);
        let mut uebrig = 64;
        while let Some(o) = cur {
            if let Some(e) = self.eigene_eigenschaft(o, name) {
                return (e.getter.is_none() && e.setter.is_none()).then_some(e.wert);
            }
            if let Some(v) = self.eigene_lese(o, name) {
                return Some(v);
            }
            uebrig -= 1;
            if uebrig == 0 {
                return None;
            }
            cur = self.heap_proto_von(o);
        }
        None
    }

    /// J52: `HasProperty` **ohne** Getter — dieselbe Form wie
    /// `Engine::hat_eigenschaft` in `builtin.rs`, aber hier zugaenglich (die
    /// `with`-Pruefung sitzt in `interp.rs`).
    fn hat_eigenschaft_ohne_getter(&self, basis: GVal, name: &str) -> bool {
        const MAX_PROTO_KETTE: u32 = 64;
        let GVal::Ref(r) = basis else { return false };
        if self.eigene_lese(r, name).is_some() {
            return true;
        }
        let mut cur = self.heap_proto_von(r);
        let mut uebrig = MAX_PROTO_KETTE;
        while let Some(p) = cur {
            if self.eigene_lese(p, name).is_some() {
                return true;
            }
            uebrig -= 1;
            if uebrig == 0 {
                return false;
            }
            cur = self.heap_proto_von(p);
        }
        false
    }

    pub(crate) fn initialisiere(&mut self, umg: HeapRef, name: &str, wert: GVal) {
        let mut ist_var = false;
        {
            let obj = self.heap.holen_mut(umg);
            if let HObj::Umgebung(env) = obj
                && let Some(b) = env.bindungen.find_mut(crate::heap::namens_hash(name), name)
            {
                ist_var = b.art == BindungsArt::Var;
                b.wert = wert;
                b.initialisiert = true;
            }
        }
        // CREATE-GLOBAL-VAR-BINDING, zweiter Teil (ES2023, 9.1.1.4.16 mit
        // 9.1.1.4.5 `SetMutableBinding`): die Initialisierung eines
        // GLOBALEN `var` schreibt den Wert **auch** in die Eigenschaft des
        // globalen Objekts.
        //
        // GEMESSEN an www.youtube.com: `var ytcfg={…}` — die Bindung hatte
        // den Wert, `window.ytcfg` war **undefined** (`TypeError: kein
        // Zugriff auf Undefiniert`), und jedes spaetere Skript lief in
        // „'ytcfg' ist nicht definiert". NUR fuer `var`: ein globales
        // `let`/`const` gehoert **nicht** ans globale Objekt (ES2023,
        // 9.1.1.4.14/15 — Declarative-Record).
        if ist_var && umg == self.global_umgebung {
            self.setze_eigen(self.global_objekt, name, wert);
        }
    }
}

/// `NARSIL_JS_GC_STRESS=<n>`: GC nach n Allokationen (Diagnose, sonst aus).
pub(crate) fn gc_stress() -> usize {
    static STRESS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *STRESS.get_or_init(|| {
        std::env::var("NARSIL_JS_GC_STRESS").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX)
    })
}

/// Der Name der Host-Funktion hinter `ev.preventDefault` (W6D).
///
/// GEMESSEN, warum er hier steht und nicht in `narsil`: der Wert
/// (`Wert::Nativ`) wird von der Engine gebaut und von `narsil`
/// registriert. Zwei Stellen fuer einen Namen sind zwei Stellen fuer
/// einen Tippfehler — und der faellt erst beim Klick auf.
pub const PREVENT_DEFAULT: &str = "__preventDefault";

/// EIN Debug von Hand, und es sind die drei Zahlen, die man beim
/// Nachmessen braucht (W6D).
///
/// GEMESSEN, warum nicht `#[derive(Debug)]`: `Engine` traegt den Heap mit
/// allen Objekten; ein `Debug` darueber wuerde die ganze Seite in die
/// Fehlermeldung schreiben (ein Test wollte drei Zahlen und bekam 40 KiB).
/// Wer den Heap sehen will, ruft `heap_bytes`.
impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Engine {{ heap: {} Byte, schritte: {}, konsole: {} }}",
            self.heap_bytes(),
            self.schritte(),
            self.konsole().len()
        )
    }
}

/// Panic-Nutzlast lesbar machen (Kind-Thread-Meldung, W5C).
fn panik_text(panik: Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = panik.downcast_ref::<String>() {
        if s.len() > 500 {
            format!("{}...", &s[..500])
        } else {
            s.clone()
        }
    } else if let Some(s) = panik.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "unbekannter Panic-Typ".to_string()
    }
}

#[cfg(test)]
mod live_seiten_stabilitaet {
    use super::*;
    #[test]
    fn task_deadlines_do_not_restart_the_monotonic_page_clock() {
        let mut engine = Engine::neu(Grenzen::standard());
        engine.uhr_start = Instant::now() - std::time::Duration::from_millis(100);
        engine.timer_vom_host(true);
        engine.browser_task_beginnen();
        assert!(engine.wanduhr_ms() >= 100);
        engine.ausfuehren("setTimeout(function(){}, 50);").unwrap();
        assert!(engine.timer_frueheste_ms().unwrap() >= 150,
            "a timer is scheduled relative to its registration time");
        let scheduled = engine.timer_frueheste_ms();
        engine.browser_task_beenden();
        engine.browser_task_beginnen();
        assert!(engine.wanduhr_ms() >= 100);
        assert_eq!(engine.timer_frueheste_ms(), scheduled);
        engine.browser_task_beenden();
    }

    #[test]
    fn browser_task_deadline_is_not_extended_by_callbacks_or_script_entries() {
        let mut engine = Engine::neu(Grenzen::standard());
        engine.ausfuehren(
            "function callback() { let sum = 0; \
             for (let i = 0; i < 1000; i++) { sum += i; } return sum; }",
        ).unwrap();
        let callback = engine.funktionsnummer("callback").unwrap();
        engine.handler_halten(callback);
        engine.browser_task_beginnen();
        let expired = Instant::now() - std::time::Duration::from_secs(5);
        engine.start = expired;
        assert!(matches!(engine.handler_aufrufen(callback, &[]), Err(JsFehler::Zeitlimit(_))));
        assert_eq!(engine.start, expired);
        // A script entry in the same task also retains the shared deadline.
        assert!(matches!(engine.ausfuehren("for (let i = 0; i < 1000; i++) {}"), Err(JsFehler::Zeitlimit(_))));
        assert_eq!(engine.start, expired);
        engine.browser_task_beenden();
        engine.browser_task_beginnen();
        assert_eq!(engine.handler_aufrufen(callback, &[]).unwrap(), Wert::Zahl(499500.0));
        engine.browser_task_beenden();
        // The legacy callback entry still measures its own execution time.
        engine.start = expired;
        assert_eq!(engine.handler_aufrufen(callback, &[]).unwrap(), Wert::Zahl(499500.0));
    }
    #[test]
    fn replacing_host_globals_does_not_accumulate_permanent_roots() {
        let mut engine = Engine::neu(Grenzen::standard());
        let root_count = engine.wurzeln.len();
        for index in 0..1000 {
            engine.host_global("event", Wert::Gegenstand(vec![("value".into(), Wert::Zahl(index as f64))])).unwrap();
        }
        assert_eq!(engine.wurzeln.len(), root_count);
        engine.frisch.clear();
        engine.sammeln();
        assert_eq!(engine.ausfuehren("event.value").unwrap(), Wert::Zahl(999.0));
    }

    #[test]
    fn exported_callback_roots_survive_gc_and_release_after_host_removal() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = captured.clone();
        let mut engine = Engine::neu(Grenzen::standard());
        engine.host_funktion("retain", move |args| {
            if let Some(Wert::Funktion(index)) = args.first() { *capture.lock().unwrap() = Some(*index); }
            Ok(Wert::Undefiniert)
        });
        engine.ausfuehren("retain((function(){let n=40;return function(){return ++n;};})());").unwrap();
        let index = captured.lock().unwrap().unwrap();
        engine.frisch.clear();
        engine.sammeln();
        assert!(engine.heap.lebt(HeapRef(index)));
        assert_eq!(engine.handler_aufrufen(index, &[]).unwrap(), Wert::Zahl(41.0));
        engine.host_callback_wurzeln_setzen(&[]);
        engine.frisch.clear();
        engine.sammeln();
        assert!(!engine.heap.lebt(HeapRef(index)), "removed callback must be reclaimable");
    }

    #[test]
    fn host_wrapper_und_eigene_closure_ueberleben_gc() {
        let mut engine = Engine::neu(Grenzen::standard());
        engine.host_objekt(crate::haken(|_, _| Ok(None), |_, _, _| Ok(()), |_, _, _| Ok(crate::Wert::Undefiniert)));
        let host = crate::Wert::Gegenstand(vec![("__host".into(), crate::Wert::Zahl(123.0))]);
        let original = engine.import_wert(&host, 0).unwrap();
        engine.host_global("node", host.clone()).unwrap();
        engine.ausfuehren("Object.defineProperty(node, 'callback', {value: function() { return 42; }}); node = undefined;").unwrap();
        engine.frisch.clear();
        engine.sammeln();
        let restored = engine.import_wert(&host, 0).unwrap();
        assert_eq!(original, restored);
        engine.host_global("node", host).unwrap();
        assert_eq!(engine.ausfuehren("node.callback()").unwrap().als_text(), "42");
    }
    #[test]
    fn dom_prototypen_bleiben_auch_ohne_globalen_ctor_gc_wurzeln() {
        let mut engine = Engine::neu(Grenzen::standard());
        let protos = [engine.proto_dom_element,engine.proto_dom_text,engine.proto_dom_dokument,engine.proto_dom_stil,engine.proto_dom_klasse];
        engine.ausfuehren("Document=undefined;Element=undefined;Text=undefined;HTMLElement=undefined;Node=undefined;").unwrap();
        engine.frisch.clear();
        engine.sammeln();
        for proto in protos { assert_ne!(proto, KEIN_REF); assert!(engine.heap.lebt(proto)); }
    }
    #[test]
    fn call_arguments_survive_collection_while_creating_a_frame() {
        let mut engine = Engine::neu(Grenzen::standard());
        engine.ausfuehren("function receive(c){return function(){return c();};} var original=function(){return 42;};").unwrap();
        let receive = engine.funktionsnummer("receive").unwrap();
        let original = engine.funktionsnummer("original").unwrap();
        engine.ausfuehren("original=undefined").unwrap();
        engine.frisch.clear();
        engine.heap.alloks_seit_gc = GC_ALLE_N_ALLOKS + 1;
        let closure = engine.aufruf_synchron(GVal::Ref(HeapRef(receive)), GVal::Undefiniert, &[GVal::Ref(HeapRef(original))], false).unwrap();
        let result = engine.aufruf_synchron(closure, GVal::Undefiniert, &[], false).unwrap();
        assert_eq!(result, GVal::Zahl(42.0));
    }

    #[test]
    fn detached_function_keeps_home_and_parent_constructor_alive() {
        let mut engine = Engine::neu(Grenzen::standard());
        engine.ausfuehren("function detached(){}").unwrap();
        let f = HeapRef(engine.funktionsnummer("detached").unwrap());
        let home = engine.neues_objekt(Some(engine.proto_objekt)).unwrap();
        let parent = engine.neues_objekt(Some(engine.proto_objekt)).unwrap();
        if let HObj::Funktion { heim, eltern_ctor, .. } = engine.heap.holen_mut(f) {
            *heim = Some(home); *eltern_ctor = Some(parent);
        } else { panic!("expected function"); }
        engine.frisch.clear();
        engine.sammeln();
        assert!(engine.heap.lebt(home));
        assert!(engine.heap.lebt(parent));
    }

    #[test]
    fn object_environment_keeps_its_binding_object_alive() {
        let mut engine = Engine::neu(Grenzen::standard());
        let object = engine.neues_objekt(Some(engine.proto_objekt)).unwrap();
        let env = engine.umgebung_mit_objekt(Some(engine.global_umgebung), object).unwrap();
        engine.frisch.clear();
        engine.sammeln();
        assert!(engine.heap.lebt(object));
        engine.umgebung_entfernen(env);
        engine.sammeln();
        assert!(!engine.heap.lebt(object));
    }

    #[test]
    fn nach_internem_threadfehler_bleibt_engine_benutzbar() {
        let mut engine = Engine::neu(Grenzen::standard());
        let result: Result<(), _> = engine.auf_dickem_stapel(|_| panic!("Testfehler"));
        assert!(result.is_err());
        engine.host_funktion("antwort", |_| Ok(crate::Wert::Zahl(42.0)));
        assert_eq!(engine.ausfuehren("antwort()").unwrap().als_text(),"42");
    }
}
