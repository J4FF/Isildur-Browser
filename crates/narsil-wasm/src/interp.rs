//! W2 Punkt 2: Interpreter. Alle MVP-Befehle plus sign-ext, sat-trunc,
//! bulk-memory, multi-value und reference-types.
//!
//! ENTWURF (die erste Fassung wurde verworfen: sie hielt
//! `block/loop/if/else` fuer „nichts tun" und suchte Sprungziele bei jedem
//! Sprung neu). Jetzt:
//!   * Aufrufkette als `Vec<Rahmen>` — KEINE Rust-Rekursion (M5),
//!   * EIN gemeinsamer Stapel, auf dem auch die Locals liegen: jeder
//!     Rahmen merkt `[lokal_basis .. basis)` = Locals, `basis ..` =
//!     Operanden (GEMESSEN 04.10., Hebel 1: vorher ein eigener
//!     `Vec<Wert>` je Rahmen),
//!   * eine `Sprungtabelle` EINMAL je Funktion beim Instanziieren
//!     (`Funktion::Eigen::sprung`): Index des `block`/`loop`/`if` ->
//!     (Position des `else` oder `None`, Position des `end`),
//!   * Brennstoff je Befehl (`Lauf::verbrauche`), danach der BENANNTE
//!     Brennstofffehler statt eines Haengers.

use std::sync::Arc;

use crate::fehler::*;
use crate::lauf::{Funktion, Host, Instanz, Lauf, NoHost};
use crate::modell::{FuncTyp, Reftyp, Valtyp};
use crate::op::{Befehl, Blockart};
use crate::wert::{
    Ref, Wert, ist_nan32, ist_nan64, nan32, nan64, nearest, nearest32, trunk, trunk_sat,
};

/// Ein Label: `block`, `loop`, `if` oder der Funktionskoerper.
#[derive(Debug)]
struct Label {
    /// Index des eroeffnenden Befehls im Koerper.
    start: usize,
    /// Operandstapel-Hoehe, auf der die Label-Werte liegen.
    hoehe: usize,
    /// Wie viele Werte ein `br` auf dieses Label mitnehmen muss.
    arity: usize,
    /// `loop`: der Sprung zeigt zurueck statt vorwaerts.
    schleife: bool,
    /// `try_table`: die Fangliste. LEER bei allen anderen Labels.
    ///
    /// Exception Handling, Wasm 3.0. GEMESSEN 02.10. (`instance.wast`).
    faenge: Vec<crate::op::Fang>,
}

/// Vorab berechnete Sprungziele: Index = Index von `block`/`loop`/`if`.
/// Inhalt = (Index des `else` oder `None`, Index des `end`).
///
/// GEMESSEN 04.10. (Hebel 1): die Tabelle wird EINMAL je Funktion beim
/// Instanziieren gebaut (`Funktion::Eigen::sprung`) statt bei jedem
/// Aufruf neu — vorher kostete jeder Rahmen einen `Vec` mit
/// `koerper.len()` Eintraegen.
#[derive(Debug)]
pub struct Sprungtabelle {
    ziele: Vec<Option<(Option<usize>, usize)>>,
}

impl Sprungtabelle {
    /// Ein Durchlauf ueber den Koerper. M3/M4: Die Zahl der Befehle ist
    /// durch `Grenzen::befehle_je_funktion` hart begrenzt.
    pub(crate) fn baue(koerper: &[Befehl]) -> Sprungtabelle {
        let mut ziele: Vec<Option<(Option<usize>, usize)>> = vec![None; koerper.len()];
        let mut offen: Vec<(usize, Option<usize>)> = Vec::new();
        for (i, b) in koerper.iter().enumerate() {
            match b {
                Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_) | Befehl::TryTable(_, _) => {
                    offen.push((i, None))
                }
                Befehl::Else => {
                    if let Some(oben) = offen.last_mut() {
                        if oben.1.is_none() {
                            oben.1 = Some(i);
                        }
                    }
                }
                Befehl::End => {
                    if let Some((start, else_pos)) = offen.pop() {
                        ziele[start] = Some((else_pos, i));
                    }
                }
                _ => {}
            }
        }
        Sprungtabelle { ziele }
    }
}

/// Eine noch nicht gefangene Ausnahme (Tagindex im gemeinsamen Raum).
///
/// Exception Handling, Wasm 3.0. GEMESSEN 02.10. (`instance.wast`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ausnahme(pub u32);

/// Ein Aufrufrahmen.
#[derive(Debug)]
struct Rahmen {
    funktion: Arc<Funktion>,
    /// Anfang der Lokalvariablen auf dem GEMEINSAMEN Stapel (Parameter
    /// zuerst, dann eigene): `[lokal_basis .. basis)`.
    ///
    /// GEMESSEN 04.10. (Hebel 1): vorher lag jeder Rahmen in einem
    /// eigenen `Vec<Wert>` — jeder Aufruf allokierte (Argument-Vec,
    /// Locals-Vec).
    lokal_basis: usize,
    /// Anfang des Operandenstapels (Ende der Locals).
    basis: usize,
    /// Befehlszaehler.
    pc: usize,
    /// Offene Labels (Index 0 = Funktionskoerper).
    labels: Vec<Label>,
    /// Speicherindex (0 oder None).
    /// Speicherindizes des Moduls (Multi-Memory: mehrere, 2.0: einer).
    speicher: Vec<u32>,
    /// Lokaler Tabellenindex -> Index im Store.
    tabellen_index: Vec<u32>,
    /// Tabellenindex fuer `call_indirect` ohne Argument (0).
    tabelle: u32,
    /// Typraum des Moduls (fuer `block (type N)`).
    typen: Arc<Vec<FuncTyp>>,
    /// Die `(sub …)`-Kette des Moduls (fuer `call_indirect`).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff): `call_indirect`
    /// muss den Typindex der aufgerufenen Funktion ueber die Kette pruefen.
    sub_ober: Arc<Vec<Option<u32>>>,
    /// Typindex je Funktion, nach Store-Index.
    fn_typ: Arc<Vec<Option<u32>>>,
    /// Je Typindex: ist der Typ final? (`call_indirect`).
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 344ff): ein finaler Typ
    /// ist mit einem strukturgleichen nicht-finalen **kein** Subtyp.
    typ_final: Arc<Vec<bool>>,
    /// Gruppennormalform je Typindex des Moduls (Zielseite von `ref.test`).
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 476ff/492ff, 6 Faelle):
    /// `$f11 <: $f21` gilt, obwohl die Typen in verschiedenen Gruppen
    /// stehen — ihre Gruppen sind gleich, weil der Obertyp des zweiten
    /// Mitglieds **gruppenintern** ist.
    ziel_gruppe: Arc<Vec<crate::modell::Gruppenform>>,
    /// Array- und Strukturtypen des Moduls (fuer `array.new_default`
    /// und die Feld-/Elementzugriffe).
    ///
    /// GEMESSEN 03.10. (`array.wast`, `struct.wast`).
    arrays: Arc<Vec<Option<crate::modell::ArrayTyp>>>,
    strukt: Arc<Vec<Option<crate::modell::StructTyp>>>,
    /// Indexraum der globalen Variablen (lokaler Index -> Store-Index).
    globale_index: Arc<Vec<u32>>,
    /// Indexraum der Funktionen des Moduls (lokaler Index -> Store-Index).
    /// `funktions_start + i` waere falsch, sobald das Modul Funktionen
    /// importiert (GEMESSEN 01.10., `table_init.wast`).
    funktionen: Arc<Vec<u32>>,
    /// Basis der Daten-/Elementabschnitte im Store (Modulindex -> Store).
    /// GEMESSEN 02.10. (`bulk.wast`): `memory.init 0` schrieb sonst in den
    /// Abschnitt eines frueheren Moduls.
    daten_start: u32,
    elemente_start: u32,
    /// Indexraum der Tags des Moduls (lokaler Index -> Index im Store).
    ///
    /// Zwei Instanzen desselben Moduls haben UNTERSCHIEDLICHE Tags — nur so
    /// ist `throw` in der einen Instanz von einem Tag der anderen
    /// unterscheidbar; derselbe Tag zweimal importiert ist DERSELBE
    /// Eintrag (GEMESSEN 02.10., `instance.wast`: 12 Faelle).
    tags: Arc<Vec<u32>>,
}

/// Der AUSFUEHRUNGSKONTEXT einer Funktion: Speicher, Daten-/Element-
/// abschnitte, Tags, Tabellen, Typraum und die Indexraeume.
///
/// Aus dem Rahmen herausgelöst, weil `return_call` denselben Kontext
/// braucht — der neue Rahmen ersetzt den alten, erbt ihn aber nicht.
///
/// GEMESSEN 03.10. (`return_call.wast`).
#[derive(Clone)]
struct Kontext {
    arrays: Arc<Vec<Option<crate::modell::ArrayTyp>>>,
    strukt: Arc<Vec<Option<crate::modell::StructTyp>>>,
    speicher: Vec<u32>,
    daten_start: u32,
    elemente_start: u32,
    tags: Arc<Vec<u32>>,
    tabellen_index: Vec<u32>,
    tabelle: u32,
    typen: Arc<Vec<FuncTyp>>,
    sub_ober: Arc<Vec<Option<u32>>>,
    fn_typ: Arc<Vec<Option<u32>>>,
    ziel_gruppe: Arc<Vec<crate::modell::Gruppenform>>,
    typ_final: Arc<Vec<bool>>,
    globale_index: Arc<Vec<u32>>,
    funktionen: Arc<Vec<u32>>,
}

impl Kontext {
    /// Ist `k` (der Store-Kontext einer Funktion) GLEICH dem Kontext
    /// eines Rahmens? Dann darf der Rahmen ihn behalten — ein
    /// End-Aufruf in dasselbe Modul kopiert sonst zwei `Vec` je Aufruf.
    ///
    /// GEMESSEN 03.10. (`return_call*.wast`: der End-Aufruf war mit
    /// 296 ns je Aufruf der teuerste Weg des Interpreters; ein
    /// Normalschritt kostet 21 ns).
    fn gleich_wie(k: &crate::lauf::ModulKontext, r: &Rahmen) -> bool {
        Arc::ptr_eq(&k.typen, &r.typen)
            && Arc::ptr_eq(&k.arrays, &r.arrays)
            && Arc::ptr_eq(&k.strukt, &r.strukt)
            && Arc::ptr_eq(&k.globale_index, &r.globale_index)
            && Arc::ptr_eq(&k.funktionen, &r.funktionen)
            && k.tags == r.tags
            && k.speicher == r.speicher
            && k.tabellen_index == r.tabellen_index
            && k.tabelle == r.tabelle
            && k.daten_start == r.daten_start
            && k.elemente_start == r.elemente_start
    }

    /// Der Kontext, in dem eine FRMD-Funktion laeuft: der Store weiss
    /// ihn. Fehlt der Eintrag (ein Test ohne Store), gilt der Kontext
    /// des aufrufenden Rahmens.
    fn aus_store(lauf: &Lauf, global: u32, aufrufer: &Rahmen) -> Kontext {
        // EINMAL klonen (GEMESSEN 04.10., Hebel 2, „keine
        // Heap-Allokation im heissen Pfad"): vorher klonte
        // `.and_then(|k| k.clone())` den ganzen Store-Kontext, und der
        // Arm baute denselben Kontext aus diesem Klon NOCH EINMAL auf —
        // pro Aufruf 20 Rc-Inkremen und VIER Vec-Allokationen statt
        // zehn und zwei (der erste Klon fiel sofort wieder an).
        match lauf.store.funktion_kontext.get(global as usize) {
            Some(Some(k)) => Kontext {
                arrays: k.arrays.clone(),
                strukt: k.strukt.clone(),
                speicher: k.speicher.clone(),
                daten_start: k.daten_start,
                elemente_start: k.elemente_start,
                tags: k.tags.clone(),
                tabellen_index: k.tabellen_index.clone(),
                tabelle: k.tabelle,
                typen: k.typen.clone(),
                sub_ober: k.sub_ober.clone(),
                fn_typ: k.fn_typ.clone(),
                ziel_gruppe: k.ziel_gruppe.clone(),
                typ_final: k.typ_final.clone(),
                globale_index: k.globale_index.clone(),
                funktionen: k.funktionen.clone(),
            },
            // Store ohne Eintrag (Test ohne Kontext): der des Aufrufers.
            _ => aufrufer.kontext(),
        }
    }
}

/// Zustand der Ausfuehrung: Rahmenstapel + Operandstapel.
#[derive(Debug)]
struct Maschine {
    stapel: Vec<Wert>,
    rahmen: Vec<Rahmen>,
    /// Eine Ausnahme, die den aktuellen Rahmen verlassen hat und im
    /// AUFRUFER gesucht wird (Exception Handling, Wasm 3.0).
    ///
    /// GEMESSEN 02.10. (`instance.wast`).
    ausnahme: Option<Ausnahme>,
}

/// Zahl der Ergebnisse einer Funktion (fuer `return` und Rahmenende).
fn ergebnis_anzahl(f: &Funktion) -> usize {
    match f {
        Funktion::Eigen { typ, .. } | Funktion::Fremd { typ, .. } => typ.ergebnis.len(),
    }
}

/// Oeffentlicher Einstieg.
pub fn aufrufen(
    lauf: &mut Lauf,
    instanz: &Instanz,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Result<Vec<Wert>, Fehler> {
    aufrufen_host(lauf, &mut NoHost, instanz, funktion, args, eintr)
}

/// Like [`aufrufen`], with the embedder that host imports reach.
pub fn aufrufen_host(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    instanz: &Instanz,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Result<Vec<Wert>, Fehler> {
    match funktion.as_ref() {
        Funktion::Fremd { typ, funktion: f } => fremd_rufen(lauf, host, typ, f, args, eintr),
        Funktion::Eigen { .. } => {
            // Hebel 3 (flacher Schnellpfad): ohne Haltepunkt und ohne
            // Spur laeuft die Uebersetzung in `schnell` — mit Rueckfall
            // auf diesen Pfad, sobald die Uebersetzung scheitert.
            if lauf.spur.is_none() {
                if let Some(r) = crate::schnell::versuche(lauf, host, funktion, args, eintr) {
                    return r;
                }
            }
            let mut m = erster_rahmen(lauf, instanz, funktion, args, eintr)?;
            // Hebel 2 (GEMESSEN 04.10.): Pruefen wie vorher
            // (`verbrauche(1)`), danach zaehlt nur noch `zaehle`.
            lauf.pruefe_brennstoff()?;
            lauf.zaehle(1);
            schleife(lauf, host, &mut m)
        }
    }
}

/// Call a function by its STORE index without an instance at hand (the JS
/// API calls exported functions, table entries and re-exports this way).
/// A wasm function runs in the context stored for it at instantiation, so
/// no per-call context is built.
pub fn aufrufen_store(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Result<Vec<Wert>, Fehler> {
    match funktion.as_ref() {
        Funktion::Fremd { typ, funktion: f } => fremd_rufen(lauf, host, typ, f, args, eintr),
        Funktion::Eigen { .. } => {
            if lauf.spur.is_none() {
                if let Some(r) = crate::schnell::versuche(lauf, host, funktion, args, eintr) {
                    return r;
                }
            }
            let k = lauf
                .store
                .funktion_kontext
                .get(eintr as usize)
                .and_then(|k| k.clone())
                .ok_or_else(|| {
                    Fehler::trap(T_INDIREKT, format!("function {eintr} has no module context"))
                })?;
            let mut m = erster_rahmen_mit(lauf, &k, funktion, args, eintr)?;
            lauf.pruefe_brennstoff()?;
            lauf.zaehle(1);
            schleife(lauf, host, &mut m)
        }
    }
}

/// Den ERSTEN Rahmen eines Aufrufs anlegen (W5/D11: auch die `Sitzung`
/// braucht ihn, ohne dass etwas ausgefuehrt wird).
///
/// GEMESSEN 03.10. (`Sitzung::starte`).
fn erster_rahmen(
    lauf: &mut Lauf,
    instanz: &Instanz,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Result<Maschine, Fehler> {
    // Gehoert die Funktion einem ANDEREN Modul (sie wurde ueber einen
    // Re-Export aufgerufen), gilt deren Kontext — nicht der des
    // aufrufenden Moduls. Sonst las eine importierte Ladefunktion den
    // Speicher des Aufrufers.
    //
    // GEMESSEN 03.10. (`linking.wast` Z. 494ff: 2 Faelle, 0xf2 statt 0x02;
    // `linking1.wast`).
    if let Some(k) = lauf
        .store
        .funktion_kontext
        .get(eintr as usize)
        .and_then(|k| k.clone())
    {
        // Vergleich ueber die INSTANZNUMMER: zwei Module ohne Speicher und
        // ohne globale Variablen haben sonst identische Indexraeume.
        //
        // GEMESSEN 03.10. (`linking.wast` Z. 494ff: 4 Faelle).
        let gleiche = k.nr == instanz.nr;
        if !gleiche {
            return erster_rahmen_mit(lauf, &k, funktion, args, eintr);
        }
    }
    let typ = match funktion.as_ref() {
        Funktion::Eigen { typ, .. } => typ,
        Funktion::Fremd { .. } => {
            return Err(Fehler::link(
                L_SIGNATUR,
                format!("Funktion {eintr}: keine Wasm-Funktion"),
            ));
        }
    };
    if args.len() != typ.parameter.len() {
        return Err(Fehler::link(
            L_SIGNATUR,
            format!(
                "Funktion {eintr}: {} Parameter erwartet, {}",
                typ.parameter.len(),
                args.len()
            ),
        ));
    }
    // Der Kontext traegt die Standardtabelle fuer `call_indirect` ohne
    // Tabellenargument (die erste Tabelle des Moduls — GEMESSEN 01.10.,
    // `table_fill.wast`).
    let k = crate::lauf::ModulKontext::von_instanz(instanz);
    erster_rahmen_mit(lauf, &k, funktion, args, eintr)
}

/// Aufruftiefe pruefen, wenn ein RAHMEN ENTSTEHT (Hebel 2, GEMESSEN
/// 04.10.): vorher pruefte `schritt` nach JEDEM Befehl, ob
/// `m.rahmen.len()` ueber der Grenze lag — die Laenge kann sich nur
/// beim Anlegen eines Rahmens aendern. Die Meldung nennt wie vorher
/// die Zahl, um die die Grenze ueberschritten wird.
#[inline(always)]
fn pruefe_tiefe(lauf: &Lauf, neu: usize) -> Result<(), Fehler> {
    if neu > lauf.grenzen.aufruftiefe {
        return Err(Fehler::trap(
            T_AUFRUFTIEFE,
            format!(
                "Aufruftiefe {neu} ueber der Grenze {}",
                lauf.grenzen.aufruftiefe
            ),
        ));
    }
    Ok(())
}

/// Wie `erster_rahmen`, aber mit einem vorgegebenen Kontext.
fn erster_rahmen_mit(
    lauf: &mut Lauf,
    k: &crate::lauf::ModulKontext,
    funktion: &Arc<Funktion>,
    args: &[Wert],
    eintr: u32,
) -> Result<Maschine, Fehler> {
    let (typ, lokale) = match funktion.as_ref() {
        Funktion::Eigen { typ, lokale, .. } => (typ, lokale),
        Funktion::Fremd { .. } => {
            return Err(Fehler::link(
                L_SIGNATUR,
                format!("Funktion {eintr}: keine Wasm-Funktion"),
            ));
        }
    };
    if args.len() != typ.parameter.len() {
        return Err(Fehler::link(
            L_SIGNATUR,
            format!(
                "Funktion {eintr}: {} Parameter erwartet, {}",
                typ.parameter.len(),
                args.len()
            ),
        ));
    }
    let tabelle = k.tabelle;
    let arity = typ.ergebnis.len();
    let mut m = Maschine {
        stapel: Vec::with_capacity(64 + args.len() + lokale.len()),
        rahmen: Vec::with_capacity(8),
        ausnahme: None,
    };
    // Die Argumente SIND die ersten Locals und bleiben liegen: Locals
    // liegen IM gemeinsamen Stapel (GEMESSEN 04.10., Hebel 1) — vorher
    // hatte jeder Rahmen zusaetzlich einen eigenen `Vec<Wert>`.
    m.stapel.extend_from_slice(args);
    let lokal_basis = 0;
    for t in lokale {
        m.stapel.push(Wert::standard(*t));
    }
    let basis = m.stapel.len();
    pruefe_tiefe(lauf, 1)?;
    m.rahmen.push(Rahmen {
        funktion: funktion.clone(),
        lokal_basis,
        basis,
        pc: 0,
        labels: vec![Label {
            start: 0,
            hoehe: basis,
            arity,
            schleife: false,
            faenge: Vec::new(),
        }],
        speicher: k.speicher.clone(),
        daten_start: k.daten_start,
        elemente_start: k.elemente_start,
        tags: k.tags.clone(),
        tabellen_index: k.tabellen_index.clone(),
        tabelle,
        typen: k.typen.clone(),
        sub_ober: k.sub_ober.clone(),
        fn_typ: k.fn_typ.clone(),
        ziel_gruppe: k.ziel_gruppe.clone(),
        typ_final: k.typ_final.clone(),
        arrays: k.arrays.clone(),
        strukt: k.strukt.clone(),
        globale_index: k.globale_index.clone(),
        funktionen: k.funktionen.clone(),
    });
    let _ = lauf;
    Ok(m)
}

/// Hostfunktion aufrufen (mit Einbettungsgrenze M5).
fn fremd_rufen(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    typ: &FuncTyp,
    f: &crate::lauf::FremdFn,
    args: &[Wert],
    eintr: u32,
) -> Result<Vec<Wert>, Fehler> {
    lauf.einbettung += 1;
    if lauf.einbettung as usize > lauf.grenzen.einbettung {
        lauf.einbettung -= 1;
        return Err(Fehler::trap(
            T_EINBETTUNG,
            format!("Hostaufruf tiefer als {} (Grenze)", lauf.grenzen.einbettung),
        ));
    }
    if args.len() != typ.parameter.len() {
        lauf.einbettung -= 1;
        return Err(Fehler::link(
            L_SIGNATUR,
            format!(
                "Fremdfunktion {eintr}: {} Parameter erwartet, {}",
                typ.parameter.len(),
                args.len()
            ),
        ));
    }
    let r = f(lauf, host, args);
    lauf.einbettung -= 1;
    r
}

/// Hauptschleife: ruft `schritt`, bis der Rahmenstapel leer ist.
fn schleife(lauf: &mut Lauf, host: &mut dyn Host, m: &mut Maschine) -> Result<Vec<Wert>, Fehler> {
    loop {
        match schritt(lauf, host, m)? {
            SchrittErgebnis::Laufend => {}
            SchrittErgebnis::Fertig(w) => return Ok(w),
        }
    }
}

/// Der Zustand einer angehaltenen Ausfuehrung: eine Zeile je Rahmen
/// (von aussen nach innen) plus die Werte des obersten Rahmens.
///
/// GEMESSEN 03.10. (D11: „Debugger echt anhalten" — der DevTools-Bereich
/// braucht genau diese Sicht auf einen LAUFENDEN Aufruf).
#[derive(Debug, Clone, PartialEq)]
pub struct Zustand {
    /// Je Rahmen: (Funktionsindex im Store, naechster Befehlsversatz).
    pub aufrufstapel: Vec<(u32, usize)>,
    /// Lokalvariablen des obersten Rahmens.
    pub lokale: Vec<Wert>,
    /// Operandstapel des obersten Rahmens.
    pub stapel: Vec<Wert>,
    /// Verbrauchtes Brennstoff (Beurteilung der Laufzeit).
    pub verbraucht: u64,
    /// ALLE Rahmen, aeusserster zuerst — je mit eigenen Locals und
    /// eigenem Operandenstapel.
    ///
    /// Vorher stand hier nur der oberste Rahmen; der Reiter „WebAssembly"
    /// will „lokale Variablen und Operandenstapel JE RAHMEN"
    /// (team/rollen/WASM.txt). Reihenfolge wie `aufrufstapel`.
    pub rahmen: Vec<RahmenBild>,
}

/// EIN Rahmen des angehaltenen Laufs: Funktionsindex, naechster
/// Befehlsversatz, eigene Locals, EIGENER Operandenstapel.
///
/// GEMESSEN 03.10.: der gemeinsame Operandstapel liegt flach in
/// `Maschine.stapel`; Rahmen `ri` reicht von seiner `basis` bis zur Basis
/// des naechsttieferen Rahmens, der letzte bis zum Ende des Stapels.
#[derive(Debug, Clone, PartialEq)]
pub struct RahmenBild {
    /// Funktionsindex im Modul (`u32::MAX` = Hostfunktion, kein Rahmen).
    pub funktion: u32,
    /// Naechster auszufuehrender Befehlsversatz.
    pub versatz: usize,
    /// Lokalvariablen (Parameter zuerst, dann eigene).
    pub lokale: Vec<Wert>,
    /// Operandstapel dieses Rahmens, von unten nach oben.
    pub stapel: Vec<Wert>,
}

/// EIN unterbrechbarer Aufruf.
///
/// `starte` bereitet den ersten Rahmen vor, `weiter` fuehrt EINEN Befehl
/// aus, `ist_fertig` sagt, ob der Aufruf beendet ist. Der Zustand laesst
/// sich jederzeit als `Zustand` abgreifen (auch mitten im Aufruf).
///
/// GEMESSEN 03.10. (`debug::Debugger::weiter_live` + D11).
#[derive(Debug)]
pub struct Sitzung {
    m: Maschine,
    fertig: Option<Vec<Wert>>,
    fehler: Option<Fehler>,
}

impl Sitzung {
    /// Einen Aufruf vorbereiten (erst `weiter` fuehrt etwas aus).
    pub fn starte(
        lauf: &mut Lauf,
        instanz: &Instanz,
        funktion: &Arc<Funktion>,
        args: &[Wert],
        eintr: u32,
    ) -> Result<Sitzung, Fehler> {
        let m = erster_rahmen(lauf, instanz, funktion, args, eintr)?;
        Ok(Sitzung {
            m,
            fertig: None,
            fehler: None,
        })
    }

    /// EINEN Befehl ausfuehren. `false`, wenn der Aufruf beendet ist.
    ///
    /// M3: jeder Schritt kostet Brennstoff — eine Endlosschleife endet
    /// also auch im Einzelschrittbetrieb mit einem benannten Fehler.
    pub fn weiter(&mut self, lauf: &mut Lauf) -> bool {
        if self.ist_fertig() {
            return false;
        }
        match schritt(lauf, &mut NoHost, &mut self.m) {
            Ok(SchrittErgebnis::Laufend) => true,
            Ok(SchrittErgebnis::Fertig(w)) => {
                self.fertig = Some(w);
                false
            }
            Err(e) => {
                self.fehler = Some(e);
                self.fertig = Some(Vec::new());
                false
            }
        }
    }

    /// Bis zum Ende weiterlaufen (wie ein normaler Aufruf) — der
    /// bequeme Weg fuer Tests und fuer Aufrufe ohne Haltepunkt.
    pub fn bis_ende(&mut self, lauf: &mut Lauf) -> Result<Vec<Wert>, Fehler> {
        while self.weiter(lauf) {}
        if let Some(e) = &self.fehler {
            return Err(e.clone());
        }
        Ok(self.fertig.clone().unwrap_or_default())
    }

    pub fn ist_fertig(&self) -> bool {
        self.fertig.is_some()
    }

    /// Das Ergebnis, sobald der Aufruf beendet ist.
    pub fn ergebnis(&self) -> Option<&[Wert]> {
        self.fertig.as_deref()
    }

    /// Der Fehler, falls der Lauf mit einem Fehler geendet hat.
    pub fn fehler(&self) -> Option<&Fehler> {
        self.fehler.as_ref()
    }

    /// Der Zustand an der NAECHSTEN auszufuehrenden Stelle.
    pub fn zustand(&self, lauf: &Lauf) -> Zustand {
        self.zustand_mit(lauf.verbraucht)
    }

    /// (Funktionsindex, naechster Befehlsversatz) des obersten Rahmens —
    /// OHNE etwas zu kopieren.
    ///
    /// Die Haltepunktsuche braucht diese beiden Zahlen bei JEDEM Schritt;
    /// vorher klonte sie pro Schritt alle Locals und den ganzen Stapel.
    ///
    /// GEMESSEN 03.10. (`debug::Debugger::bis_haltepunkt_live`).
    pub fn position(&self) -> Option<(u32, usize)> {
        let r = self.m.rahmen.last()?;
        Some((r_funktionsindex(&r.funktion), r.pc))
    }

    /// Anzahl der offenen Rahmen (0 = der Aufruf ist beendet).
    /// `aufrufstapel.len()` ohne Kopie.
    pub fn tiefe(&self) -> usize {
        self.m.rahmen.len()
    }

    fn zustand_mit(&self, verbraucht: u64) -> Zustand {
        // JEDES Rahmens Locals und Operandenstapel. Beide liegen IM
        // gemeinsamen Stapel: `[lokal_basis .. basis)` sind die Locals,
        // `basis ..` die Operanden, und die enden am `lokal_basis` des
        // naechsttieferen Rahmens (dessen Locals liegen dort), der
        // letzte Rahmen bis zum Ende.
        //
        // GEMESSEN 04.10. (Hebel 1): vorher war `lokale` ein eigener
        // `Vec` je Rahmen.
        let n = self.m.rahmen.len();
        let mut rahmen = Vec::with_capacity(n);
        for (ri, r) in self.m.rahmen.iter().enumerate() {
            let basis = r.basis;
            let ende = if ri + 1 < n {
                self.m.rahmen[ri + 1].lokal_basis.max(basis)
            } else {
                self.m.stapel.len().max(basis)
            };
            rahmen.push(RahmenBild {
                funktion: r_funktionsindex(&r.funktion),
                versatz: r.pc,
                lokale: self
                    .m
                    .stapel
                    .get(r.lokal_basis..basis)
                    .unwrap_or_default()
                    .to_vec(),
                stapel: self.m.stapel.get(basis..ende).unwrap_or_default().to_vec(),
            });
        }
        let aufrufstapel = rahmen.iter().map(|r| (r.funktion, r.versatz)).collect();
        let (lokale, stapel) = rahmen
            .last()
            .map(|r| (r.lokale.clone(), r.stapel.clone()))
            .unwrap_or((Vec::new(), Vec::new()));
        Zustand {
            aufrufstapel,
            lokale,
            stapel,
            verbraucht,
            rahmen,
        }
    }
}

/// Der Funktionsindex im Store (fuer den Debugger).
fn r_funktionsindex(f: &Arc<Funktion>) -> u32 {
    match f.as_ref() {
        Funktion::Eigen { raum, .. } => *raum,
        // Hostfunktionen haben keinen Index im Modul.
        Funktion::Fremd { .. } => u32::MAX,
    }
}

/// Ergebnis EINES Schritts.
#[derive(Debug, Clone, PartialEq)]
pub enum SchrittErgebnis {
    /// Der Schritt ist ausgefuehrt, es geht weiter.
    Laufend,
    /// Der Rahmenstapel ist leer: das Ergebnis des Aufrufs.
    Fertig(Vec<Wert>),
}

/// EIN Schritt der Hauptschleife: fuehrt hoechstens EINEN Befehl aus.
///
/// GEMESSEN 03.10. (D11: der Debugger muss den Lauf wirklich anhalten
/// koennen — dazu muss der Schritt einzeln aufrufbar sein).
///
/// GEMESSEN 04.10. (Hebel 2, verschr. A/B): die Variante mit lokalem
/// Koerper-Handle (`schleife` haelt `Rc<Funktion>` in zwei Slots und
/// ruft `schritt_mit` ohne Klon) kostete die Schleifenlast +11 % und
/// die Aufruflast +10 % gegenueber dieser Form — der Zugewinn faellt
/// erst an, wenn die Hauptschleife den Koerper tatsaechlich INLINEN
/// kann. Hier also der Rahmen-Zugriff wie vorher; geaendert bleiben:
/// Brennstoff wird gezaehlt (`zaehle`) und nur an Schleifenrueck-
/// springen/Aufrufen geprueft, die Aufruftiefe nur beim Rahmenaufbau
/// (`pruefe_tiefe`) — das sind zwei Pruefungen je Befehl weniger.
fn schritt(lauf: &mut Lauf, host: &mut dyn Host, m: &mut Maschine) -> Result<SchrittErgebnis, Fehler> {
    let ri = match m.rahmen.len().checked_sub(1) {
        Some(r) => r,
        None => return Ok(SchrittErgebnis::Fertig(Vec::new())),
    };
    // Koerperlaenge OHNE den `Rc` zu klonen: nur das Match (Hebel 1).
    let koerper_len = rahmen_koerper_len(&m.rahmen[ri]);
    if m.rahmen[ri].pc >= koerper_len {
        // Der Rahmen ist schon zu (pc hinter dem Koerper): hier
        // zurueckgeben und NICHT weiterlaufen — die Rueckkehr ist ein
        // eigener Schritt (siehe `rahmen_ende`).
        if let Some(erg) = rahmen_ende(m, ri)? {
            return Ok(SchrittErgebnis::Fertig(erg));
        }
        return Ok(SchrittErgebnis::Laufend);
    }
    lauf.zaehle(1);
    let pc = m.rahmen[ri].pc;
    // W5: Spur aufzeichnen (nur wenn ein Debugger haengt).
    if lauf.spur.is_some() {
        let r = &m.rahmen[ri];
        let funktion = match r.funktion.as_ref() {
            Funktion::Eigen { raum, .. } => *raum,
            // Fremdfunktionen haben keinen Index im Modul: fuer die Spur
            // genuegt ein Platz, der garantiert ausserhalb liegt.
            Funktion::Fremd { .. } => u32::MAX,
        };
        if let Some(spur) = lauf.spur.as_mut() {
            spur.push(crate::debug::Schritt {
                funktion,
                versatz: pc,
                tiefe: m.rahmen.len() - 1,
                lokale: m.stapel[r.lokal_basis..r.basis].to_vec(),
                stapel: m.stapel[r.basis..].to_vec(),
                ergebnis: Vec::new(),
                speicher_seiten: lauf
                    .speicher_idx
                    .and_then(|i| lauf.store.speicher.get(i as usize))
                    .map(|s| s.seiten())
                    .unwrap_or(0),
                verbraucht: lauf.verbraucht,
            });
        }
    }
    // EIN Befehl nach WERT holen: Index-Zugriff in den Koerper, KEIN
    // `Rc<Vec<Befehl>>`-Klon je Schritt (Hebel 1). Erst danach darf `m`
    // veraendert werden — die Ausfuehrung braucht `&mut m`.
    let b = match m.rahmen[ri].funktion.as_ref() {
        Funktion::Eigen { koerper, .. } => koerper.get(pc).cloned(),
        // Fremdfunktionen haben keinen Koerper: der Rahmen endet oben.
        Funktion::Fremd { .. } => None,
    };
    let Some(b) = b else {
        if let Some(erg) = rahmen_ende(m, ri)? {
            return Ok(SchrittErgebnis::Fertig(erg));
        }
        return Ok(SchrittErgebnis::Laufend);
    };
    m.rahmen[ri].pc = pc + 1;
    if let Some(ziel) = befehl(lauf, host, m, ri, b, koerper_len)? {
        if ziel >= koerper_len {
            // Sprung bis zum Ende des Koerpers = Rueckkehr: EIN Schritt,
            // der naechste Befehl gehoert dem AUFRUFER (siehe `rahmen_ende`).
            if let Some(erg) = rahmen_ende(m, ri)? {
                return Ok(SchrittErgebnis::Fertig(erg));
            }
            return Ok(SchrittErgebnis::Laufend);
        }
        m.rahmen[ri].pc = ziel;
    } else if m.rahmen[ri].pc >= koerper_len {
        // Das schliessende `end` hat den Koerper verlassen (pc == len):
        // der Rahmen ist hier zu — sonst waere der Zustand „pc hinter
        // dem Koerper" zwischen zwei Schritten sichtbar, und der haette
        // weder Befehl noch Haltepunkt noch Disassembly-Marke.
        if let Some(erg) = rahmen_ende(m, ri)? {
            return Ok(SchrittErgebnis::Fertig(erg));
        }
        return Ok(SchrittErgebnis::Laufend);
    }
    Ok(SchrittErgebnis::Laufend)
}

/// Rahmen `ri` zurueckgeben: die obersten `n` Werte sind sein Ergebnis.
///
/// GEMESSEN 04.10. (Hebel 2, „keine Heap-Allokation im heissen Pfad"):
/// vorher `split_off` + `extend` — JEDE Rueckkehr legte fuer die
/// Ergebnisse einen `Vec` an; fib(32) hat 7 049 155 Rueckkehrn. Jetzt
/// schiebt `copy_within` die Werte an Ort und Stelle (ohne Allokation,
/// identisches Ergebnis).
fn rahmen_zurueck(m: &mut Maschine, ri: usize, n: usize) {
    // Die Locals liegen IM Stapel: der ganze Bereich des Rahmens
    // (`lokal_basis ..`) wird durch die Ergebnisse ersetzt.
    let lokal_basis = m.rahmen[ri].lokal_basis;
    let len = m.stapel.len();
    // Begrenzen: die Ergebnisse liegen validiert ueber `lokal_basis` —
    // so kann `copy_within` in keinem Fall ueber den Stapel ragen.
    let n = n.min(len.saturating_sub(lokal_basis));
    if n > 0 {
        m.stapel.copy_within(len - n.., lokal_basis);
    }
    m.stapel.truncate(lokal_basis + n);
    m.rahmen.truncate(ri);
}

/// Ein Rahmen ist am Ende: Ergebnisse liegen jetzt auf dem Aufruferstapel,
/// der Rahmen ist entfernt. `Some(v)` = das war der LETZTE Rahmen, der
/// Aufruf ist fertig.
///
/// **GEMESSEN 03.10. (tests/diagnose.rs):** vorher sprang die Schleife
/// nach dem Zurueckgeben IMSELBEN Schritt weiter und fuehrte sofort den
/// naechsten Befehl des AUFRUFERS aus. Dadurch war der Rueckkehrplatz (die
/// Anweisung nach einem `call`) nie zwischen zwei Schritten beobachtbar:
/// * ein Haltepunkt dort konnte ueberhaupt nicht anspringen,
/// * „Ueber" und „Aus" landeten um EINE Anweisung zu weit hinten (der
///   Aufrufer hatte seinen `end` schon ausgefuehrt), und
/// * die Anzeige stand auf einem Platz, an dem es keinen Befehl gibt.
///
/// Jetzt ist die Rueckkehr ein eigener Schritt; der naechste Schritt
/// fuehrt den Aufrufer aus. An allen drei Stellen, an denen ein Rahmen
/// endet, wird diese Funktion benutzt (siehe `schritt`).
fn rahmen_ende(m: &mut Maschine, ri: usize) -> Result<Option<Vec<Wert>>, Fehler> {
    let n = ergebnis_anzahl(&m.rahmen[ri].funktion);
    rahmen_zurueck(m, ri, n);
    if m.rahmen.is_empty() {
        if let Some(a) = m.ausnahme.take() {
            return Err(Fehler::trap(
                T_DIVERSE,
                format!("uncaught exception (Tag {})", a.0),
            ));
        }
        let start = m.stapel.len().saturating_sub(n);
        return Ok(Some(m.stapel.split_off(start)));
    }
    if m.ausnahme.is_some() {
        ausnahme_weitergeben(m);
    }
    Ok(None)
}

/// Befehl ausfuehren. `Ok(Some(ziel))` = Sprung, davon `ziel == koerper_len`
/// bedeutet „Rahmen beenden".
fn befehl(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    m: &mut Maschine,
    ri: usize,
    b: Befehl,
    koerper_len: usize,
) -> Result<Option<usize>, Fehler> {
    // Der Aufrufer uebergibt einen Klon: `b` wird unten an die
    // Musterzerlegung bewegt. Der teure Fall (Vektor-Befehle) ist damit
    // genau einmal pro Ausfuehrung geklont — vorher stand die Kopie
    // schon im Schritt.
    //
    // GEMESSEN 03.10. (Schlussphase: Kopien vermeiden).
    use Befehl::*;
    // Multi-Memory: `MitSpeicher` nennt den Speicher, sonst gilt der
    // ERSTE Speicher des Moduls (WebAssembly 2.0 hat genau einen).
    let (b, speicher) = match b {
        MitSpeicher {
            befehl,
            speicher: si,
        } => {
            let idx = m.rahmen[ri]
                .speicher
                .get(si as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(
                        T_DIVERSE,
                        format!(
                            "Speicher {si} fehlt (Modul hat {})",
                            m.rahmen[ri].speicher.len()
                        ),
                    )
                })?;
            (*befehl, Some(idx))
        }
        andere => (andere, m.rahmen[ri].speicher.first().copied()),
    };
    match b {
        Unreachable => Err(Fehler::trap(T_UNERREICHBAR, "unreachable")),
        Nop => Ok(None),
        Block(bt) | Loop(bt) => {
            let (p, e) = blocktypen(&m.rahmen[ri], &bt);
            let hoehe =
                m.stapel.len().checked_sub(p).ok_or_else(|| {
                    Fehler::trap(T_DIVERSE, format!("Block: {p} Parameter fehlen"))
                })?;
            let arity = if matches!(b, Loop(_)) { p } else { e };
            let start = m.rahmen[ri].pc - 1;
            let schleife = matches!(b, Loop(_));
            m.rahmen[ri].labels.push(Label {
                start,
                hoehe,
                arity,
                schleife,
                faenge: Vec::new(),
            });
            Ok(None)
        }
        // `try_table`: wie ein `block`, aber das Label traegt die
        // Fangliste; `throw` sucht darin (Exception Handling, Wasm 3.0).
        //
        // GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
        TryTable(bt, faenge) => {
            let (p, e) = blocktypen(&m.rahmen[ri], &bt);
            let hoehe = m.stapel.len().checked_sub(p).ok_or_else(|| {
                Fehler::trap(T_DIVERSE, format!("try_table: {p} Parameter fehlen"))
            })?;
            let start = m.rahmen[ri].pc - 1;
            m.rahmen[ri].labels.push(Label {
                start,
                hoehe,
                arity: e,
                schleife: false,
                faenge: faenge.clone(),
            });
            Ok(None)
        }
        Throw(t) => {
            // Hebel 2 (GEMESSEN 04.10.): Brennstoff an SPRUNGEN — ueber
            // das fangende `try_table` kann ein `throw` zyklisch sein.
            lauf.pruefe_brennstoff()?;
            ausnahme_werfen(m, ri, t, koerper_len)
        }
        // `array.new_default $t`: legt ein Array an (Standardwerte).
        //
        // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
        ArrayNewDefault(t) => {
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32;
            // Die Standardwerte des ELEMENTTYPS fuellen den Array.
            //
            // GEMESSEN 03.10. (`array.wast`: „len" und „get" nach
            // `array.new_default` — vorher standen hier lauter Nullen,
            // weil der Elementtyp im Rahmen fehlte).
            gc_laenge_pruefen(lauf, laenge)?;
            lauf.store.gc_slots_buchen(laenge as usize)?;
            let standard =
                element_standard(m.rahmen[ri].arrays.as_ref(), t).unwrap_or(Wert::I32(0));
            let inhalt = vec![standard; laenge as usize];
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge,
                inhalt: crate::modell::GcInhalt::Array(inhalt),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // ---- GC-Arrays ------------------------------------------------
        // `array.new $t` (Laenge, Wert), `array.len`, `array.get`,
        // `array.get_s/u`, `array.set`.
        //
        // GEMESSEN 03.10. (`array.wast`: 47 Faelle).
        ArrayNew(t) => {
            // Die Signatur ist `[t i32]`: die LAENGE liegt oben.
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32;
            let wert = pop(m)?;
            gc_laenge_pruefen(lauf, laenge)?;
            lauf.store.gc_slots_buchen(laenge as usize)?;
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge,
                inhalt: crate::modell::GcInhalt::Array(vec![wert; laenge as usize]),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        ArrayLen => {
            let id = id_von(&pop(m)?)
                .ok_or_else(|| Fehler::trap(T_INDIREKT, "array.len: kein Arrayverweis"))?
                as u32;
            let laenge = lauf
                .store
                .gc
                .get(id as usize)
                .map(|o| o.laenge)
                .ok_or_else(|| Fehler::trap(T_INDIREKT, "array.len: Objekt fehlt"))?;
            m.stapel.push(Wert::I32(laenge as i32));
            Ok(None)
        }
        // `array.get` liefert den deklarierten Typ; `get_s`/`get_u` sind
        // die gepackten Formen — vorzeichenbehaftet bzw. ohne Vorzeichen.
        //
        // GEMESSEN 03.10.: `array.get_u` gab ungefiltert den Speicherwert
        // zurueck (`array.wast` Z. 168ff liest gepackte Bytes).
        // `array.get` liefert den deklarierten Typ; `get_s`/`get_u` sind
        // die gepackten Formen — vorzeichenbehaftet bzw. ohne Vorzeichen.
        //
        // GEMESSEN 03.10.: `array.get_u` gab ungefiltert den Speicherwert
        // zurueck (`array.wast` Z. 168ff liest gepackte Bytes).
        ArrayGet(t) => {
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.get")?;
            let (_laenge, wert) = array_element(lauf, id, index)?;
            let _ = t;
            m.stapel.push(wert);
            Ok(None)
        }
        ArrayGetS(t) => {
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.get_s")?;
            let (_laenge, wert) = array_element(lauf, id, index)?;
            let _ = t;
            // Der Wert liegt bereits vorzeichenbehaftet im Haufen.
            m.stapel.push(wert);
            Ok(None)
        }
        ArrayGetU(t) => {
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.get_u")?;
            let (_laenge, wert) = array_element(lauf, id, index)?;
            // Gepackt: auf die Bit-Zahl kuerzen, OHNE Vorzeichen.
            let bits = array_typen(m.rahmen[ri].arrays.as_ref(), t)
                .map(|a| a.bits)
                .unwrap_or(0);
            m.stapel.push(if bits > 0 {
                Wert::I32(wert.als_i32().unwrap_or(0).rem_euclid(1 << bits.min(31)))
            } else {
                wert
            });
            Ok(None)
        }
        // `array.new_fixed $t n`: die `n` Werte liegen in Reihenfolge auf
        // dem Stapel (erstes Feld zuerst), die Laenge steht im Befehl.
        //
        // GEMESSEN 03.10. (`array.wast`: `new_fixed`).
        ArrayNewFixed(t, n) => {
            let at = array_typen(m.rahmen[ri].arrays.as_ref(), t);
            let anzahl = n as usize;
            gc_laenge_pruefen(lauf, n)?;
            let mut werte: Vec<Wert> = Vec::with_capacity(anzahl);
            for _ in 0..anzahl {
                werte.push(packe(&at, pop(m)?));
            }
            werte.reverse();
            lauf.store.gc_slots_buchen(anzahl)?;
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge: n,
                inhalt: crate::modell::GcInhalt::Array(werte),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // `array.new_data $t $d (offset, size)` — Bytes des
        // Datenabschnitts, `element_breite` Byte je Element.
        //
        // GEMESSEN 03.10. (`array_new_data.wast`: 0/23 -> ?).
        ArrayNewData(t, seg) => {
            let at = array_typen(m.rahmen[ri].arrays.as_ref(), t);
            let groesse = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let offset = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let werte =
                daten_als_werte(lauf, m.rahmen[ri].daten_start + seg, offset, groesse, &at)?;
            lauf.store.gc_slots_buchen(werte.len())?;
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge: werte.len() as u32,
                inhalt: crate::modell::GcInhalt::Array(werte),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // `array.new_elem $t $e (offset, size)` — Verweise aus dem
        // Elementabschnitt.
        //
        // GEMESSEN 03.10. (`array_new_elem.wast`: 0/19 -> ?).
        ArrayNewElem(t, seg) => {
            let groesse = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let offset = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let werte = elem_als_werte(lauf, m.rahmen[ri].elemente_start + seg, offset, groesse)?;
            lauf.store.gc_slots_buchen(werte.len())?;
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge: werte.len() as u32,
                inhalt: crate::modell::GcInhalt::Array(werte),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // `array.init_data $t $d (a, i, doff, size)`.
        ArrayInitData(t, seg) => {
            let at = array_typen(m.rahmen[ri].arrays.as_ref(), t);
            let groesse = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let daten_offset = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.init_data")?;
            let werte = daten_als_werte(
                lauf,
                m.rahmen[ri].daten_start + seg,
                daten_offset,
                groesse,
                &at,
            )?;
            array_abschnitt_schreiben(lauf, id, index, &werte, "array.init_data")?;
            Ok(None)
        }
        // `array.init_elem $t $e (a, i, eoff, size)`.
        ArrayInitElem(_t, seg) => {
            let groesse = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let elem_offset = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.init_elem")?;
            let werte = elem_als_werte(
                lauf,
                m.rahmen[ri].elemente_start + seg,
                elem_offset,
                groesse,
            )?;
            array_abschnitt_schreiben(lauf, id, index, &werte, "array.init_elem")?;
            Ok(None)
        }
        // `array.fill $t (a, i, v, n)` — GEMESSEN 03.10.
        //
        // Der Stapel von unten: `[Array, Index, Wert, Anzahl]`, gepoppt
        // wird von oben. Die Grenzen werden VOR dem ersten Schreibvorgang
        // geprueft (M3/M8).
        ArrayFill(_) => {
            let anzahl = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let wert = pop(m)?;
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = array_id(&pop(m)?, "array.fill")?;
            let laenge = array_len(lauf, id);
            if index as u64 + anzahl as u64 > laenge as u64 {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!(
                        "array.fill: out of bounds array access (Index {index}, {anzahl} Elemente, Laenge {laenge})"
                    ),
                ));
            }
            if anzahl == 0 {
                return Ok(None);
            }
            if let Some(crate::modell::GcObjekt {
                inhalt: crate::modell::GcInhalt::Array(elemente),
                ..
            }) = lauf.store.gc.get_mut(id as usize)
            {
                for stelle in elemente.iter_mut().skip(index).take(anzahl) {
                    *stelle = wert;
                }
            }
            Ok(None)
        }
        // `array.copy $d $s (d, di, s, si, n)` — GEMESSEN 03.10.
        //
        // Bei Ueberlappung wird ueber eine Zwischenkopie geschrieben, so
        // wie `memory.copy` es verlangt.
        ArrayCopy(_, _) => {
            let anzahl = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let quell_index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let quell_id = array_id(&pop(m)?, "array.copy")?;
            let ziel_index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let ziel_id = array_id(&pop(m)?, "array.copy")?;
            let ziel_laenge = array_len(lauf, ziel_id);
            let quell_laenge = array_len(lauf, quell_id);
            if ziel_index as u64 + anzahl as u64 > ziel_laenge as u64
                || quell_index as u64 + anzahl as u64 > quell_laenge as u64
            {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!(
                        "array.copy: out of bounds array access (Ziel {ziel_index}+{anzahl} von {ziel_laenge}, Quelle {quell_index}+{anzahl} von {quell_laenge})"
                    ),
                ));
            }
            if anzahl == 0 {
                return Ok(None);
            }
            // Zwischenkopie: die Reihenfolge der Schreibvorgaenge ist
            // sonst bei Ueberlappung sichtbar.
            let stuecke: Vec<Wert> = match lauf.store.gc.get(quell_id as usize) {
                Some(crate::modell::GcObjekt {
                    inhalt: crate::modell::GcInhalt::Array(e),
                    ..
                }) => e.iter().skip(quell_index).take(anzahl).copied().collect(),
                _ => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        "array.copy: Quelle ist kein Array",
                    ));
                }
            };
            if let Some(crate::modell::GcObjekt {
                inhalt: crate::modell::GcInhalt::Array(e),
                ..
            }) = lauf.store.gc.get_mut(ziel_id as usize)
            {
                for (i, w) in stuecke.into_iter().enumerate() {
                    if let Some(stelle) = e.get_mut(ziel_index + i) {
                        *stelle = w;
                    }
                }
            }
            Ok(None)
        }
        ArraySet(_) => {
            let wert = pop(m)?;
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let id = id_von(&pop(m)?)
                .ok_or_else(|| Fehler::trap(T_INDIREKT, "array.set: kein Arrayverweis"))?
                as u32;
            if index as u32
                >= lauf
                    .store
                    .gc
                    .get(id as usize)
                    .map(|o| o.laenge)
                    .unwrap_or(0)
            {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!(
                        "array.set: Index {index} ausserhalb (Laenge {})",
                        array_len(lauf, id)
                    ),
                ));
            }
            if let Some(crate::modell::GcObjekt {
                inhalt: crate::modell::GcInhalt::Array(elemente),
                ..
            }) = lauf.store.gc.get_mut(id as usize)
            {
                if let Some(stelle) = elemente.get_mut(index) {
                    *stelle = wert;
                }
            }
            Ok(None)
        }
        // ---- GC-Werte (Wasm 3.0) ---------------------------------------
        // `ref.i31 x`: der Wert ist ein GC-Objekt mit dem i31-Inhalt.
        //
        // GEMESSEN 03.10. (`extern.wast`: 16 Faelle).
        RefI31 => {
            // Ein i31 hat 31 Bit: `ref.i31 0xc000_0000` ist `0x4000_0000`,
            // und `i31.get_s` liefert dafuer -1073741824.
            //
            // GEMESSEN 03.10. (`i31.wast` Z. 14: „i32 -1073741824
            // erwartet, i32 1073741824").
            let w = pop(m)?.als_i32().unwrap_or(0) & 0x7fff_ffff;
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: crate::modell::JEDER_I31TYP,
                laenge: 1,
                inhalt: crate::modell::GcInhalt::I31(w),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // `struct.new_default $t`: die Felder bekommen ihre Standardwerte.
        StructNewDefault(t) => {
            // Die Standardwerte der Felder fuellen das Objekt. Vorher
            // stand hier `Feld(Vec::new())` — ein Objekt ganz ohne Felder,
            // obwohl der Typ welche hat (GEMESSEN 03.10., `struct.wast`).
            let felder = strukt_standard_werte(m.rahmen[ri].strukt.as_ref(), t).unwrap_or_default();
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge: felder.len() as u32,
                inhalt: crate::modell::GcInhalt::Feld(felder),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        // ---- GC-Strukturen (Wasm 3.0) --------------------------------
        // GEMESSEN 03.10. (`struct.wast`: 5/24 -> 24/24).
        StructNew(t) => {
            let felder = strukt_felder(m.rahmen[ri].strukt.as_ref(), t).unwrap_or_default();
            // Die Feldwerte liegen in Typ-Reihenfolge auf dem Stapel, das
            // ERSTE Feld also am tiefsten.
            let mut werte: Vec<Wert> = Vec::with_capacity(felder.len());
            for i in (0..felder.len()).rev() {
                let w = pop(m)?;
                // Gepackte Felder werden auf ihre Bit-Zahl gekuerzt und
                // vorzeichenbehaftet abgelegt; alles andere bleibt, wie es
                // ist (ein `(ref …)`-Feld traegt seinen Referenzwert).
                werte.push(match felder.get(i) {
                    Some(f) if f.bits > 0 => Wert::I32(crate::modell::kuerzen_und_vorzeichen(
                        w.als_i32().unwrap_or(0),
                        f.bits,
                    )),
                    _ => w,
                });
            }
            werte.reverse();
            lauf.store.gc.push(crate::modell::GcObjekt {
                typ: t,
                laenge: werte.len() as u32,
                inhalt: crate::modell::GcInhalt::Feld(werte),
            });
            let id = (lauf.store.gc.len() - 1) as u32;
            m.stapel.push(Wert::Gcref(Some(id)));
            Ok(None)
        }
        StructGet(t, f) | StructGetS(t, f) | StructGetU(t, f) => {
            // `struct.get_s` liest vorzeichenbehaftet, `_u` ohne; ein
            // ungepacktes Feld wird von beiden nie erreicht (der
            // Validator laesst es nicht zu).
            let vorzeichen = matches!(b, StructGetS(_, _));
            let id = strukt_id(&pop(m)?, "struct.get")?;
            let felder = strukt_felder(m.rahmen[ri].strukt.as_ref(), t).unwrap_or_default();
            let bits = felder
                .get(f as usize)
                .map(|feld| feld.bits)
                .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("struct.get: Feld {f} fehlt")))?;
            let wert = feld_wert(lauf, id, f as usize, "struct.get")?;
            m.stapel.push(if bits > 0 && !vorzeichen {
                Wert::I32(wert.als_i32().unwrap_or(0).rem_euclid(1 << bits.min(31)))
            } else {
                wert
            });
            Ok(None)
        }
        StructSet(t, f) => {
            let wert = pop(m)?;
            let id = strukt_id(&pop(m)?, "struct.set")?;
            let felder = strukt_felder(m.rahmen[ri].strukt.as_ref(), t).unwrap_or_default();
            let bits = felder
                .get(f as usize)
                .map(|feld| feld.bits)
                .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("struct.set: Feld {f} fehlt")))?;
            let abgelegt = if bits > 0 {
                Wert::I32(crate::modell::kuerzen_und_vorzeichen(
                    wert.als_i32().unwrap_or(0),
                    bits,
                ))
            } else {
                wert
            };
            match lauf.store.gc.get_mut(id as usize) {
                Some(crate::modell::GcObjekt {
                    inhalt: crate::modell::GcInhalt::Feld(werte),
                    ..
                }) => match werte.get_mut(f as usize) {
                    Some(stelle) => {
                        *stelle = abgelegt;
                        Ok(None)
                    }
                    None => Err(Fehler::trap(
                        T_INDIREKT,
                        format!("struct.set: Feld {f} fehlt im Objekt"),
                    )),
                },
                Some(_) => Err(Fehler::trap(
                    T_INDIREKT,
                    "struct.set: Objekt ist keine Struktur",
                )),
                None => Err(Fehler::trap(
                    T_INDIREKT,
                    "struct.set: Objekt fehlt im Haufen",
                )),
            }
        }
        // `any.convert_extern`: der WERT bleibt derselbe; nur die
        // Typretraegheit aendert sich (wie bei `ref.as_non_null`).
        //
        // GEMESSEN 03.10.: mit einem Weglegen des Wertes gab
        // `externalize-ii` einen fremden Stapel zurueck.
        // Die IDENTITAET — mit EINER Ausnahme: `null` bekommt die
        // Darstellung der Zielart, sonst waere ein `anyref`-null ein
        // `externref`-null und die Erwartung `(ref.null any)` passte
        // nicht.
        //
        // GEMESSEN 03.10. (`extern.wast` Z. 41/42:
        // `internalize(null extern) -> (ref.null any)` und
        // `externalize(null any) -> (ref.null extern)`).
        AnyConvertExtern => {
            let w = pop(m)?;
            m.stapel.push(match w {
                Wert::Extern(None) => Wert::Func(None),
                other => other,
            });
            Ok(None)
        }
        ExternConvertAny => {
            let w = pop(m)?;
            m.stapel.push(match w {
                Wert::Func(None) | Wert::Gcref(None) => Wert::Extern(None),
                other => other,
            });
            Ok(None)
        }
        // `ref.test` / `ref.cast`: die Typfrage an einen GC-Wert.
        //
        // GEMESSEN 03.10. (`i31.wast` 17, `ref_test.wast` 68,
        // `ref_cast.wast` 40 — die drei Dateen brauchten nur dies).
        RefTest(ziel) => {
            let w = pop(m)?;
            // Die **Gruppennormalform** entscheidet mit, wenn beide Seiten
            // einen Typindex aus einer `(rec …)`-Gruppe nennen.
            //
            // GEMESSEN 04.10. (`type-subtyping.wast` Z. 476ff/492ff,
            // 6 Faelle „i32 1 erwartet, i32 0"): `$f11 <: $f21` gilt,
            // obwohl die Typen in **verschiedenen** Gruppen stehen —
            // ihre Gruppen sind gleich, weil der Obertyp des zweiten
            // Mitglieds **gruppenintern** (`Ziel::Intern`) ist. Der reine
            // Typindex-Vergleich (und auch die `(sub …)`-Kette) sagt
            // hier „nein".
            //
            // Der umgekehrte Fall bleibt „nein" und damit richtig
            // (Z. 515ff, `assert_return (i32.const 0)`): dort zeigt der
            // Obertyp des zweiten Mitglieds in eine **andere** Gruppe
            // (`Ziel::Extern`), die Normalformen unterscheiden sich.
            let ueber_gruppe = match (&w, &ziel) {
                (Wert::Func(Some(f)), Reftyp::Konkret { typ: z, .. }) => {
                    let k = &m.rahmen[ri];
                    match (
                        lauf.store
                            .funktion_gruppe
                            .get(*f as usize)
                            .cloned()
                            .flatten(),
                        k.ziel_gruppe.get(*z as usize).cloned().flatten(),
                    ) {
                        (Some((ga, pa)), Some((gb, pb))) => pa == pb && ga == gb,
                        _ => false,
                    }
                }
                _ => false,
            };
            let passt = ueber_gruppe || wert_passt_zu(lauf, &w, &ziel);
            m.stapel.push(Wert::I32(i32::from(passt)));
            Ok(None)
        }
        RefCast(ziel) => {
            let w = pop(m)?;
            // Ein nicht-leerbares Ziel schliesst `null` aus.
            let ist_null = matches!(w, Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None));
            if ist_null && !ziel.ist_nullable() {
                return Err(Fehler::trap(T_INDIREKT, "null reference"));
            }
            // Die `(sub …)`-Kette zaehlt auch hier: `ref.cast (ref $t0)` auf
            // einen Wert vom Typ `$t1` gelingt, wenn
            // `(type $t1 (sub $t0 …))` gilt.
            //
            // GEMESSEN 03.10. (`type-subtyping.wast` Z. 300ff, Abschnitt
            // „Runtime types", `run`): `wert_passt_zu` vergleicht bei
            // Funktionen nur den Typindex exakt, also`|wert| <: |ziel|`
            // wurde faelschlich zum Trap.
            let ueber_kette = match (&w, &ziel) {
                (Wert::Func(Some(f)), Reftyp::Konkret { typ: z, .. }) => {
                    let k = &m.rahmen[ri];
                    k.fn_typ
                        .get(*f as usize)
                        .copied()
                        .flatten()
                        .is_some_and(|ti| crate::modell::subtyp_ueber_kette_in(&k.sub_ober, ti, *z))
                }
                _ => false,
            };
            if !ueber_kette && !wert_passt_zu(lauf, &w, &ziel) {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!("ref.cast: Wert passt nicht zu {}", ziel.text()),
                ));
            }
            m.stapel.push(w);
            Ok(None)
        }
        // `i31.get_s` / `i31.get_u`: der i31-Wert als `i32`. `_u`
        // maskiert auf 31 Bit (`-1` wird `0x7fff_ffff`).
        //
        // GEMESSEN 03.10. (`i31.wast` Z. 37:
        // `get_u(-1)` -> 0x7fff_ffff).
        I31GetS | I31GetU => {
            let w = pop(m)?;
            let id = match w {
                Wert::Gcref(Some(i)) => i,
                _ => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        "i31.get_s/i31.get_u: kein i31-Verweis",
                    ));
                }
            };
            let wert = match lauf.store.gc.get(id as usize).map(|o| &o.inhalt) {
                Some(crate::modell::GcInhalt::I31(v)) => *v,
                _ => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        "i31.get_s/i31.get_u: Objekt ist kein i31",
                    ));
                }
            };
            m.stapel.push(Wert::I32(if matches!(b, Befehl::I31GetS) {
                // Bit 30 ist das Vorzeichen (31 Bit gross).
                if wert & 0x4000_0000 != 0 {
                    wert | !0x7fff_ffff_u32 as i32
                } else {
                    wert
                }
            } else {
                wert & 0x7fff_ffff
            }));
            Ok(None)
        }
        // `any.convert_extern` und `extern.convert_any` sind die
        // IDENTITAET: der WERT bleibt, nur die Typretraegheit wechselt
        // (`externref` <-> `anyref`). `null` bleibt `null`.
        //
        // GEMESSEN 03.10. (`extern.wast` Z. 45-57: mit „GC-Objekt ->
        // null" waren 7 von 16 Faellen rot — der Rueckweg muss den
        // i31-Wert unveraendert zurueckgeben, `(ref.i31)` erwartet ihn).
        //
        // `ref.eq`: Identitaet zweier GC-Referenzen (`null == null` ist 1).
        RefEq => {
            let b = pop(m)?;
            let a = pop(m)?;
            let gleich = match (a, b) {
                (Wert::Gcref(x), Wert::Gcref(y)) => x == y,
                _ => false,
            };
            m.stapel.push(Wert::I32(i32::from(gleich)));
            Ok(None)
        }
        If(bt) => {
            let bedingung = pop(m)?.als_i32().unwrap_or(0);
            let (p, e) = blocktypen(&m.rahmen[ri], &bt);
            let hoehe = m
                .stapel
                .len()
                .checked_sub(p)
                .ok_or_else(|| Fehler::trap(T_DIVERSE, format!("if: {p} Parameter fehlen")))?;
            let pos = m.rahmen[ri].pc - 1;
            m.rahmen[ri].labels.push(Label {
                start: pos,
                hoehe,
                arity: e,
                schleife: false,
                faenge: Vec::new(),
            });
            if bedingung != 0 {
                return Ok(None);
            }
            // Sprungziel des if holen (Sprungtabelle ist bereits gebaut).
            let (else_pos, end_pos) =
                sprungziel(m, ri, pos).ok_or_else(|| Fehler::trap(T_DIVERSE, "if ohne end"))?;
            match else_pos {
                Some(e) => {
                    // else-Zweig: hinter dem `else` weitermachen, Label bleibt.
                    m.rahmen[ri].pc = e + 1;
                }
                None => {
                    // Kein else: zum `end` springen, das Label wird dort geschlossen.
                    m.rahmen[ri].pc = end_pos;
                }
            }
            Ok(None)
        }
        Else => {
            // Ende des then-Zweigs: die Ergebnisse liegen ueber der
            // Label-Hoehe; wir springen zum `end` der Struktur.
            let pos = m.rahmen[ri]
                .labels
                .last()
                .map(|l| l.start)
                .ok_or_else(|| Fehler::trap(T_DIVERSE, "else ohne if"))?;
            let (_, end_pos) = sprungziel(m, ri, pos)
                .ok_or_else(|| Fehler::trap(T_DIVERSE, "else: if ohne end"))?;
            m.rahmen[ri].pc = end_pos;
            Ok(None)
        }
        End => {
            // Label schliessen; die Ergebnisse bleiben auf dem Stapel.
            m.rahmen[ri].labels.pop();
            Ok(None)
        }
        Br(t) => {
            // Hebel 2 (GEMESSEN 04.10.): Brennstoff NUR an
            // Rueckspruengen pruefen — die einzige Quelle von
            // Endlosschleifen — statt nach jedem Befehl.
            lauf.pruefe_brennstoff()?;
            br_ausfuehren(m, ri, t)
        }
        BrIf(t) => {
            let bedingung = pop(m)?.als_i32().unwrap_or(0);
            if bedingung == 0 {
                return Ok(None);
            }
            lauf.pruefe_brennstoff()?;
            br_ausfuehren(m, ri, t)
        }
        BrTable(ziele, vorgabe) => {
            let idx = pop(m)?.als_i32().unwrap_or(0) as u32 as usize;
            let t = ziele.get(idx).copied().unwrap_or(vorgabe);
            lauf.pruefe_brennstoff()?;
            br_ausfuehren(m, ri, t)
        }
        Return => {
            m.rahmen[ri].labels.clear();
            m.rahmen[ri].pc = koerper_len;
            Ok(Some(koerper_len))
        }
        Call(idx) => {
            let global = m.rahmen[ri].store_funktion(idx);
            let n = lauf
                .store
                .funktionen
                .get(global as usize)
                .map(|f| match f.as_ref() {
                    Funktion::Eigen { typ, .. } | Funktion::Fremd { typ, .. } => {
                        typ.parameter.len()
                    }
                })
                .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("call {idx}: Funktion fehlt")))?;
            unteraufruf(lauf, host, m, ri, global, n)?;
            Ok(None)
        }
        CallIndirect {
            typ: typidx,
            tabelle: tidx,
        } => {
            let store_idx = m.rahmen[ri]
                .tabellen_index
                .get(tidx as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_INDIREKT, format!("call_indirect: Tabelle {tidx} fehlt"))
                })?;
            // Die Tabelle muss `funcref` halten — erst HIER geprueft, nicht
            // beim Betreten der Funktion (GEMESSEN 01.10., `table_fill.wast`).
            // Die Tabelle muss Funktionsreferenzen halten: `funcref`
            // **oder** ein konkreter Typindex aus `(rec …)`.
            //
            // GEMESSEN 04.10. (`type-subtyping.wast` Z. 373ff, 1 Fall
            // „call_indirect: Tabelle 0 haelt keine funcref-Eintraege"):
            // `(table (ref null $t2) (elem $f2 $f3))` ist eine ganz normale
            // GC-Tabelle und **darf** indirect gerufen werden. `externref`
            // bleibt ausgeschlossen — der Trap ist genau fuer diese
            // Tabelle da (GEMESSEN 01.10., `table_fill.wast`).
            let passt = lauf
                .store
                .tabellen
                .get(store_idx as usize)
                .map(|t| matches!(t.reftyp(), Reftyp::Func | Reftyp::Konkret { .. }))
                .unwrap_or(true);
            if !passt {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!("call_indirect: Tabelle {tidx} haelt keine funcref-Eintraege"),
                ));
            }
            let laenge = lauf
                .store
                .tabellen
                .get(store_idx as usize)
                .map(|t| t.len())
                .unwrap_or(0);
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let eintrag = lauf
                .store
                .tabellen
                .get(store_idx as usize)
                .and_then(|t| t.lies(index).ok())
                .ok_or_else(|| {
                    Fehler::trap(
                        T_INDIREKT,
                        format!("undefined element: Index {index} (Tabelle {store_idx} hat {laenge} Plaetze)"),
                    )
                })?;
            let func = match eintrag {
                Some(Ref::Func(f)) => f,
                _ => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!("uninitialized element {index}"),
                    ));
                }
            };
            // Signaturpruefung: Zieltyp gegen den angegebenen Typindex.
            let n_param = lauf
                .store
                .funktionen
                .get(func as usize)
                .map(|f| crate::lauf::signatur(f))
                .ok_or_else(|| {
                    Fehler::trap(T_INDIREKT, format!("Funktion {func} fehlt im Store"))
                })?;
            if let Some(w) = m.rahmen[ri].typen.get(typidx as usize) {
                // Die `(sub …)`-Kette zaehlt ebenso: `call_indirect (type $t0)`
                // darf eine Funktion vom Typ `$t1` rufen, wenn
                // `(type $t1 (sub $t0 …))` gilt.
                //
                // GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime
                // types", 7 Faelle): der reine Strukturvergleich verlangt dort
                // faelschlich „indirect call type mismatch".
                //
                // GEMESSEN 03.10. (**dreimal zurueckgenommen**):
                //  * nur nominal statt strukturell: 34964  (-26, 21 im Kern)
                //  * nominal **nur** bei `(sub …)`-Deklaration im Modul:
                //    in allen drei Kombinationen **neutral** (34997, 34990,
                //    34990) — sie repariert die vier Trap-Faelle aus `sf2`
                //    (Z. 373ff) und verliert genau vier Faelle mit falschen
                //    Rueckgabewerten. Die offene Frage bleibt: Wasm 2.0 will
                //    Struktur, die GC-Erweiterung Nominalitaet; die vier
                //    Wert-Faelle gehoeren zur selben Regel.
                if !subtyp_indirekt(lauf, &m.rahmen[ri], func, typidx, &n_param, w) {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!(
                            "indirect call type mismatch: {} != {}",
                            zeile(&n_param),
                            zeile(w)
                        ),
                    ));
                }
            }
            unteraufruf(lauf, host, m, ri, func, n_param.parameter.len())?;
            Ok(None)
        }
        // ---- Tail Calls (Wasm 3.0) -------------------------------------
        ReturnCall(f) => {
            let global = m.rahmen[ri].store_funktion(f);
            return schwanzaufruf(lauf, host, m, ri, global, koerper_len);
        }
        ReturnCallIndirect { typ, tabelle } => {
            let store_idx = m.rahmen[ri]
                .tabellen_index
                .get(tabelle as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(
                        T_INDIREKT,
                        format!("return_call_indirect: Tabelle {tabelle} fehlt"),
                    )
                })?;
            let laenge = lauf
                .store
                .tabellen
                .get(store_idx as usize)
                .map(|t| t.len())
                .unwrap_or(0);
            // Der Index liegt oben, darunter die Labeltypen.
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let eintrag = lauf
                .store
                .tabellen
                .get(store_idx as usize)
                .and_then(|t| t.lies(index).ok())
                .ok_or_else(|| {
                    Fehler::trap(
                        T_INDIREKT,
                        format!("undefined element: Index {index} (Tabelle {store_idx} hat {laenge} Plaetze)"),
                    )
                })?;
            let func = match eintrag {
                Some(Ref::Func(f)) => f,
                _ => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!("uninitialized element {index}"),
                    ));
                }
            };
            if let Some(w) = m.rahmen[ri].typen.get(typ as usize) {
                let n_param = lauf
                    .store
                    .funktionen
                    .get(func as usize)
                    .map(|f| crate::lauf::signatur(f))
                    .ok_or_else(|| {
                        Fehler::trap(T_INDIREKT, format!("Funktion {func} fehlt im Store"))
                    })?;
                // Die `(sub …)`-Kette zaehlt ebenso: `call_indirect (type $t0)`
                // darf eine Funktion vom Typ `$t1` rufen, wenn
                // `(type $t1 (sub $t0 …))` gilt.
                //
                // GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime
                // types", 7 Faelle): der reine Strukturvergleich verlangt dort
                // faelschlich „indirect call type mismatch".
                //
                // GEMESSEN 03.10. (**dreimal zurueckgenommen**):
                //  * nur nominal statt strukturell: 34964  (-26, 21 im Kern)
                //  * nominal **nur** bei `(sub …)`-Deklaration im Modul:
                //    in allen drei Kombinationen **neutral** (34997, 34990,
                //    34990) — sie repariert die vier Trap-Faelle aus `sf2`
                //    (Z. 373ff) und verliert genau vier Faelle mit falschen
                //    Rueckgabewerten. Die offene Frage bleibt: Wasm 2.0 will
                //    Struktur, die GC-Erweiterung Nominalitaet; die vier
                //    Wert-Faelle gehoeren zur selben Regel.
                if !subtyp_indirekt(lauf, &m.rahmen[ri], func, typ, &n_param, w) {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!(
                            "indirect call type mismatch: {} != {}",
                            zeile(&n_param),
                            zeile(w)
                        ),
                    ));
                }
            }
            return schwanzaufruf(lauf, host, m, ri, func, koerper_len);
        }
        ReturnCallRef(t) => {
            // Die Referenz liegt OBEN ueber den Parametern.
            let w = pop(m)?;
            let func = match w {
                Wert::Func(Some(f)) => f,
                Wert::Func(None) => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        "return_call_ref: null-Funktionsreferenz",
                    ));
                }
                andere => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!("return_call_ref: Funktionsreferenz erwartet, {andere:?}"),
                    ));
                }
            };
            if let Some(w) = m.rahmen[ri].typen.get(t as usize) {
                let n_param = lauf
                    .store
                    .funktionen
                    .get(func as usize)
                    .map(|f| crate::lauf::signatur(f))
                    .ok_or_else(|| {
                        Fehler::trap(T_INDIREKT, format!("Funktion {func} fehlt im Store"))
                    })?;
                // Die `(sub …)`-Kette zaehlt ebenso: `call_indirect (type $t0)`
                // darf eine Funktion vom Typ `$t1` rufen, wenn
                // `(type $t1 (sub $t0 …))` gilt.
                //
                // GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime
                // types", 7 Faelle): der reine Strukturvergleich verlangt dort
                // faelschlich „indirect call type mismatch".
                //
                // GEMESSEN 03.10. (**dreimal zurueckgenommen**):
                //  * nur nominal statt strukturell: 34964  (-26, 21 im Kern)
                //  * nominal **nur** bei `(sub …)`-Deklaration im Modul:
                //    in allen drei Kombinationen **neutral** (34997, 34990,
                //    34990) — sie repariert die vier Trap-Faelle aus `sf2`
                //    (Z. 373ff) und verliert genau vier Faelle mit falschen
                //    Rueckgabewerten. Die offene Frage bleibt: Wasm 2.0 will
                //    Struktur, die GC-Erweiterung Nominalitaet; die vier
                //    Wert-Faelle gehoeren zur selben Regel.
                if !subtyp_indirekt(lauf, &m.rahmen[ri], func, t, &n_param, w) {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!(
                            "call_ref: Signatur passt nicht: {} != {}",
                            zeile(&n_param),
                            zeile(w)
                        ),
                    ));
                }
            }
            return schwanzaufruf(lauf, host, m, ri, func, koerper_len);
        }
        RefNull(r) => {
            // `(ref null $t)` und `(ref $t)` ergeben beide `null`; der
            // Typindex gehoert nur zur Pruefung. Fuer `extern`-Typen ist
            // der Wert eine externe Referenz.
            //
            // GEMESSEN 03.10. (`table.wast` Z. 30).
            m.stapel.push(if r.ist_extern() {
                Wert::Extern(None)
            } else {
                Wert::Func(None)
            });
            Ok(None)
        }
        RefIsNull => {
            let w = pop(m)?;
            let ist_null = matches!(w, Wert::Func(None) | Wert::Extern(None));
            m.stapel.push(Wert::I32(i32::from(ist_null)));
            Ok(None)
        }
        RefFunc(f) => {
            let global = m.rahmen[ri].store_funktion(f);
            m.stapel.push(Wert::Func(Some(global)));
            Ok(None)
        }
        // `ref.as_non_null`: der Wert BLEIBT liegen, nur die
        // Typretraegheit aendert sich — zur Laufzeit ist es derselbe Wert.
        //
        // GEMESSEN 03.10. (ein erstes `pop` ohne Zuruecklegen liess den
        // Aufrufer mit einem fremden Wert auf dem Stapel zurueck).
        // `ref.as_non_null` ist die nicht-leerbare Form: ein `null`
        // ist ein Trap („null reference").
        //
        // GEMESSEN 03.10. (`ref_cast.wast` Z. 51/56-58:
        // `ref_cast_non_null` auf `ref.null any` muss werfen).
        RefAsNonNull => {
            let w = pop(m)?;
            if matches!(w, Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None)) {
                return Err(Fehler::trap(T_INDIREKT, "null reference"));
            }
            m.stapel.push(w);
            Ok(None)
        }
        // `call_ref $t`: die Referenz wird wie beim `call_indirect`
        // aufgeloest, aber OHNE Tabelle — der Typindex traegt die Signatur.
        //
        // GEMESSEN 03.10. (`unreached-valid.wast`).
        CallRef(typidx) => {
            // Die REFERENZ liegt OBEN, darunter die Parameter (Spec:
            // `[t1* (ref t)]`). Die Argumente werden nach dem Pop der
            // Referenz geholt, dann wird der Stapel um ihre Hoehe
            // gekuerzt.
            //
            // GEMESSEN 03.10.: mit umgekehrter Reihenfolge kam der
            // i32-Parameter als Referenz heraus („call_ref:
            // Funktionsreferenz erwartet, I32(42)"); `br_on_null.wast`
            // Z. 45 verlangt die umgekehrte Reihenfolge.
            let w = pop(m)?;
            let func = match w {
                Wert::Func(Some(f)) => f,
                Wert::Func(None) => {
                    return Err(Fehler::trap(T_INDIREKT, "call_ref: null-Funktionsreferenz"));
                }
                andere => {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!("call_ref: Funktionsreferenz erwartet, {andere:?}"),
                    ));
                }
            };
            let n_param = lauf
                .store
                .funktionen
                .get(func as usize)
                .map(|f| crate::lauf::signatur(f))
                .ok_or_else(|| {
                    Fehler::trap(T_INDIREKT, format!("Funktion {func} fehlt im Store"))
                })?;
            if let Some(w) = m.rahmen[ri].typen.get(typidx as usize) {
                // Die `(sub …)`-Kette zaehlt ebenso: `call_indirect (type $t0)`
                // darf eine Funktion vom Typ `$t1` rufen, wenn
                // `(type $t1 (sub $t0 …))` gilt.
                //
                // GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime
                // types", 7 Faelle): der reine Strukturvergleich verlangt dort
                // faelschlich „indirect call type mismatch".
                //
                // GEMESSEN 03.10. (**dreimal zurueckgenommen**):
                //  * nur nominal statt strukturell: 34964  (-26, 21 im Kern)
                //  * nominal **nur** bei `(sub …)`-Deklaration im Modul:
                //    in allen drei Kombinationen **neutral** (34997, 34990,
                //    34990) — sie repariert die vier Trap-Faelle aus `sf2`
                //    (Z. 373ff) und verliert genau vier Faelle mit falschen
                //    Rueckgabewerten. Die offene Frage bleibt: Wasm 2.0 will
                //    Struktur, die GC-Erweiterung Nominalitaet; die vier
                //    Wert-Faelle gehoeren zur selben Regel.
                if !subtyp_indirekt(lauf, &m.rahmen[ri], func, typidx, &n_param, w) {
                    return Err(Fehler::trap(
                        T_INDIREKT,
                        format!(
                            "call_ref: Signatur passt nicht: {} != {}",
                            zeile(&n_param),
                            zeile(w)
                        ),
                    ));
                }
            }
            // Die Parameter liegen jetzt oben.
            let n_arg = n_param.parameter.len();
            if m.stapel.len() < n_arg {
                return Err(Fehler::trap(
                    T_DIVERSE,
                    format!("call_ref: {n_arg} Argumente fehlen auf dem Stapel"),
                ));
            }
            // Die Argumente liegen jetzt oben. `unteraufruf` nimmt nur
            // ihre ZAHL — kein `to_vec` mehr (Hebel 1) — und raeumt sie
            // selbst vom Stapel.
            //
            // GEMESSEN 03.10. (`call_ref.wast`): ein zusaetzliches
            // `truncate` liess `unteraufruf` zweimal abraeumen und
            // panicte („attempt to subtract with overflow").
            unteraufruf(lauf, host, m, ri, func, n_arg)?;
            Ok(None)
        }
        // `br_on_null $l`: im `null`-Fall springt der Befehl und
        // VERBRAUCHT die Referenz (der Sprung traegt nur die Labeltypen
        // `t*`); im anderen Fall bleibt sie als nicht-leerbare Referenz
        // liegen.
        //
        // Beleg: `br_on_null.wast` Z. 38
        //     (func (param $r (ref null $t)) (drop (br_on_null 0 (local.get $r))))
        // waere mit mitgetragener Referenz ungueltig (die Funktion hat
        // kein Ergebnis). Z. 45 dagegen erwartet, dass der i32-Parameter
        // im Label ankommt.
        //
        // GEMESSEN 03.10.: mit Zuruecklegen in BEIDEN Faellen kam die
        // Referenz als Labelwert an („i32 3 erwartet, null").
        // `br_on_cast` / `br_on_cast_fail` (Wasm 3.0 GC) — die Frage ist
        // dieselbe wie bei `ref.cast`, nur mit Sprungziel.
        //
        // GEMESSEN 03.10. (`br_on_cast.wast` 6/31).
        BrOnCast(l, _, rt2) => {
            let w = pop(m)?;
            if wert_passt_zu(lauf, &w, &rt2) {
                m.stapel.push(w);
                lauf.pruefe_brennstoff()?;
                return br_ausfuehren(m, ri, l);
            }
            m.stapel.push(w);
            Ok(None)
        }
        BrOnCastFail(l, _, rt2) => {
            let w = pop(m)?;
            if wert_passt_zu(lauf, &w, &rt2) {
                m.stapel.push(w);
                Ok(None)
            } else {
                m.stapel.push(w);
                lauf.pruefe_brennstoff()?;
                br_ausfuehren(m, ri, l)
            }
        }
        // `br_on_non_null l rt` (Wasm 3.0, Typed Function References):
        // der getippte `br_on_null` — gesprungen wird, wenn der Wert **nicht
        // null** ist *und* zum Typ passt; dann kommt er mit dem
        // nicht-nullbaren Typ im Label an.
        //
        // GEMESSEN 03.10. (`br_on_non_null.wast` 1/9, `br_on_cast_fail.wast`
        // 3/31): ohne diesen Arm war der Befehl nicht ausfuehrbar.
        BrOnNonNull(l) => {
            let w = pop(m)?;
            let null = matches!(w, Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None));
            if !null {
                m.stapel.push(w);
                lauf.pruefe_brennstoff()?;
                return br_ausfuehren(m, ri, l);
            }
            m.stapel.push(w);
            Ok(None)
        }
        BrOnNull(l) => {
            let w = pop(m)?;
            let ist_null = matches!(w, Wert::Func(None) | Wert::Extern(None));
            if ist_null {
                // Der Rueckgabewert ist die neue Programmziffer (der Sprung
                // ueberspringt den Rest des Blocks) — ohne ihn laeuft der
                // Block weiter, obwohl das Label schon entfernt ist.
                //
                // GEMESSEN 03.10. („br 1: nur 1 Labels (0..0)").
                lauf.pruefe_brennstoff()?;
                return br_ausfuehren(m, ri, l);
            }
            m.stapel.push(w);
            Ok(None)
        }
        Drop => {
            pop(m)?;
            Ok(None)
        }
        Select | SelectTyp(_) => {
            let bedingung = pop(m)?.als_i32().unwrap_or(0);
            let b = pop(m)?;
            let a = pop(m)?;
            m.stapel.push(if bedingung != 0 { a } else { b });
            Ok(None)
        }
        LocalGet(i) => {
            let pos = lokal_position(m, ri, i).map_err(|anzahl| {
                Fehler::trap(
                    T_DIVERSE,
                    format!("local.get {i}: ausserhalb (0..{anzahl})"),
                )
            })?;
            let w = m.stapel[pos];
            m.stapel.push(w);
            Ok(None)
        }
        LocalSet(i) => {
            let w = pop(m)?;
            let pos = lokal_position(m, ri, i)
                .map_err(|_| Fehler::trap(T_DIVERSE, format!("local.set {i}: ausserhalb")))?;
            m.stapel[pos] = w;
            Ok(None)
        }
        LocalTee(i) => {
            let w = pop(m)?;
            let pos = lokal_position(m, ri, i)
                .map_err(|_| Fehler::trap(T_DIVERSE, format!("local.tee {i}: ausserhalb")))?;
            m.stapel[pos] = w;
            m.stapel.push(w);
            Ok(None)
        }
        GlobalGet(i) => {
            // Ueber die Zelle: zwei Importe derselben Variablen teilen sich
            // den Wert (GEMESSEN 02.10., `instance.wast`).
            // Ueber den Indexraum der Instanz: Importe liegen im Store vor
            // `globale_start` (GEMESSEN 03.10., `global.wast` Z. 633ff).
            let gi = m.rahmen[ri]
                .globale_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(
                        T_DIVERSE,
                        format!(
                            "global.get {i}: ausserhalb ({} globale Variablen)",
                            m.rahmen[ri].globale_index.len()
                        ),
                    )
                })?;
            let w = lauf
                .zelle(gi)
                .copied()
                .ok_or_else(|| Fehler::trap(T_DIVERSE, format!("global.get {i}: ausserhalb")))?;
            m.stapel.push(w);
            Ok(None)
        }
        GlobalSet(i) => {
            let gi = m.rahmen[ri]
                .globale_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(
                        T_DIVERSE,
                        format!(
                            "global.set {i}: ausserhalb ({} globale Variablen)",
                            m.rahmen[ri].globale_index.len()
                        ),
                    )
                })?;
            let w = pop(m)?;
            *lauf
                .zelle_mut(gi)
                .ok_or_else(|| Fehler::trap(T_DIVERSE, format!("global.set {i}: ausserhalb")))? = w;
            Ok(None)
        }
        // ---- Speicher laden ------------------------------------------------
        I32Load { versatz } => lade(m, lauf, speicher, versatz, Lade::I32, 2, false),
        I32Load8S { versatz } => lade(m, lauf, speicher, versatz, Lade::I32, 0, true),
        I32Load8U { versatz } => lade(m, lauf, speicher, versatz, Lade::I32, 0, false),
        I32Load16S { versatz } => lade(m, lauf, speicher, versatz, Lade::I32, 1, true),
        I32Load16U { versatz } => lade(m, lauf, speicher, versatz, Lade::I32, 1, false),
        I64Load { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 3, false),
        I64Load8S { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 0, true),
        I64Load8U { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 0, false),
        I64Load16S { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 1, true),
        I64Load16U { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 1, false),
        I64Load32S { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 2, true),
        I64Load32U { versatz } => lade(m, lauf, speicher, versatz, Lade::I64, 2, false),
        F32Load { versatz } => lade(m, lauf, speicher, versatz, Lade::F32, 2, false),
        F64Load { versatz } => lade(m, lauf, speicher, versatz, Lade::F64, 3, false),
        // ---- Speicher schreiben --------------------------------------------
        I32Store { versatz } => speichere(m, lauf, speicher, versatz, Lade::I32, 2),
        I32Store8 { versatz } => speichere(m, lauf, speicher, versatz, Lade::I32, 0),
        I32Store16 { versatz } => speichere(m, lauf, speicher, versatz, Lade::I32, 1),
        I64Store { versatz } => speichere(m, lauf, speicher, versatz, Lade::I64, 3),
        I64Store8 { versatz } => speichere(m, lauf, speicher, versatz, Lade::I64, 0),
        I64Store16 { versatz } => speichere(m, lauf, speicher, versatz, Lade::I64, 1),
        I64Store32 { versatz } => speichere(m, lauf, speicher, versatz, Lade::I64, 2),
        F32Store { versatz } => speichere(m, lauf, speicher, versatz, Lade::F32, 2),
        F64Store { versatz } => speichere(m, lauf, speicher, versatz, Lade::F64, 3),
        MemorySize => {
            let n = speicher_von(lauf, speicher)?.seiten() as i32;
            m.stapel.push(Wert::I32(n));
            Ok(None)
        }
        MemoryGrow => {
            // Der Operand ist eine SEITENZAHL (u32). Ein negativer Wert ist
            // laut Spezifikation ein sehr grosses Wachstum und schlaegt
            // fehl — nicht „0 Seiten wachsen" (GEMESSEN 01.10.:
            // `delta.max(0)` machte aus `memory.grow(-1)` einen Erfolg).
            let delta = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let alt = speicher_mut(lauf, speicher)?.waechst(delta);
            m.stapel.push(Wert::I32(alt));
            Ok(None)
        }
        MemoryInit(idx) => {
            // Modulindex -> Storeindex (siehe `Rahmen::daten_start`).
            let idx = m.rahmen[ri].daten_start + idx;
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let quelle = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ziel = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            // M4: Erst die Grenzen pruefen, dann kopieren. Die Kopie ist auf
            // `laenge` Byte begrenzt, und `laenge <= Speichergroesse`.
            let dq = match lauf.store.daten.get(idx as usize) {
                Some(Some(d)) => d.len() as u64,
                Some(None) => 0, // verworfen (`data.drop`)
                None => {
                    return Err(Fehler::trap(
                        T_SPEICHER_LESEN,
                        format!("memory.init: Datenabschnitt {idx} unbekannt"),
                    ));
                }
            };
            let sp_max = speicher_von(lauf, speicher)?.byte_len() as u64;
            if quelle + laenge > dq || ziel + laenge > sp_max {
                return Err(Fehler::trap(
                    T_SPEICHER_SCHREIBEN,
                    format!(
                        "memory.init: {quelle}+{laenge} (Abschnitt hat {dq} Byte) -> \
                         {ziel}+{laenge} (Speicher hat {sp_max} Byte)"
                    ),
                ));
            }
            let stuecke: Vec<u8> = lauf.store.daten[idx as usize]
                .as_ref()
                .map(|d| d[quelle as usize..(quelle + laenge) as usize].to_vec())
                .unwrap_or_default();
            speicher_mut(lauf, speicher)?.schreib(ziel, &stuecke)?;
            Ok(None)
        }
        DataDrop(idx) => {
            let idx = m.rahmen[ri].daten_start + idx;
            if let Some(d) = lauf.store.daten.get_mut(idx as usize) {
                *d = None;
            }
            Ok(None)
        }
        MemoryCopy {
            ziel: zi,
            quelle: qu,
        } => {
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let quelle = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ziel = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            // Multi-Memory: Quelle und Ziel koennen verschiedene Speicher
            // sein (beide null = derselbe).
            //
            // GEMESSEN 03.10. (`memory_copy1.wast` 4 Faelle, `memory_copy0.wast`
            // 15 Faelle nach dem ersten Versuch: `$mem3 $mem3` benutzte den
            // Standardspeicher statt des genannten).
            let (ziel_store, qu_store) = speicher_paar(m, ri, zi, qu)?;
            // Quelle lesen (Grenze pruefen, BEVOR geschrieben wird — M4).
            let max_q = lauf
                .store
                .speicher
                .get(qu_store as usize)
                .map(|s| s.byte_len() as u64)
                .ok_or_else(|| {
                    Fehler::trap(T_SPEICHER_LESEN, "memory.copy: Quelle fehlt im Store")
                })?;
            if laenge > max_q || quelle + laenge > max_q {
                return Err(Fehler::trap(
                    T_SPEICHER_LESEN,
                    format!("memory.copy: Quelle {quelle}+{laenge} > {max_q} Byte"),
                ));
            }
            if ziel_store == qu_store {
                let s = lauf
                    .store
                    .speicher
                    .get_mut(ziel_store as usize)
                    .ok_or_else(|| {
                        Fehler::trap(T_SPEICHER_SCHREIBEN, "memory.copy: Ziel fehlt im Store")
                    })?;
                let max = s.byte_len() as u64;
                if laenge > max || ziel + laenge > max {
                    return Err(Fehler::trap(
                        T_SPEICHER_SCHREIBEN,
                        format!("memory.copy {quelle}+{laenge} -> {ziel}+{laenge} > {max} Byte"),
                    ));
                }
                s.kopiere(ziel, quelle, laenge as usize)?;
                return Ok(None);
            }
            let stueck: Vec<u8> = {
                let qs = lauf.store.speicher.get(qu_store as usize).ok_or_else(|| {
                    Fehler::trap(T_SPEICHER_LESEN, "memory.copy: Quelle fehlt im Store")
                })?;
                qs.lies(quelle, laenge as usize)?.to_vec()
            };
            let zs = lauf
                .store
                .speicher
                .get_mut(ziel_store as usize)
                .ok_or_else(|| {
                    Fehler::trap(T_SPEICHER_SCHREIBEN, "memory.copy: Ziel fehlt im Store")
                })?;
            let max_z = zs.byte_len() as u64;
            if laenge > max_z || ziel + laenge > max_z {
                return Err(Fehler::trap(
                    T_SPEICHER_SCHREIBEN,
                    format!("memory.copy: Ziel {ziel}+{laenge} > {max_z} Byte"),
                ));
            }
            zs.kopiere_von(ziel, &stueck)?;
            Ok(None)
        }
        MemoryFill => {
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let wert = pop(m)?.als_i32().unwrap_or(0) as u8;
            let ziel = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let s = speicher_mut(lauf, speicher)?;
            let max = s.byte_len() as u64;
            if ziel + laenge > max {
                return Err(Fehler::trap(
                    T_SPEICHER_SCHREIBEN,
                    format!("memory.fill {ziel}+{laenge} > {max} Byte"),
                ));
            }
            s.fuelle(ziel, wert, laenge as usize)?;
            Ok(None)
        }
        TableInit { element, tabelle } => {
            let element = m.rahmen[ri].elemente_start + element;
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let quelle = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ziel = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            // Ein VERWORFENER Abschnitt (`elem.drop`) verhaelt sich wie ein
            // leerer: Laenge 0 ist erlaubt, jede andere Laenge trapt in der
            // Grenzpruefung unten. Nur ein unbekannter Index ist ein
            // Fehler. GEMESSEN 02.10. (`bulk.wast`: `init_passive` nach
            // `drop_passive` — „Element 0 verworfen" war zu streng).
            let el = lauf
                .store
                .elemente
                .get(element as usize)
                .map(|e| e.clone().unwrap_or_default())
                .ok_or_else(|| {
                    Fehler::trap(
                        T_TABELLE_LESEN,
                        format!("table.init: Element {element} unbekannt"),
                    )
                })?;
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(tabelle as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(
                        T_TABELLE_LESEN,
                        format!("table.init: Tabelle {tabelle} fehlt"),
                    )
                })?;
            lauf.store
                .tabellen
                .get_mut(ti as usize)
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("Tabelle {ti} fehlt im Store"))
                })?
                .init(&el, ziel, quelle, laenge)?;
            Ok(None)
        }
        ElemDrop(idx) => {
            let idx = m.rahmen[ri].elemente_start + idx;
            if let Some(e) = lauf.store.elemente.get_mut(idx as usize) {
                *e = None;
            }
            Ok(None)
        }
        TableCopy { ziel, quelle } => {
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let q = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let z = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let zi = m.rahmen[ri]
                .tabellen_index
                .get(ziel as usize)
                .copied()
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "table.copy: Zieltabelle fehlt"))?;
            let qi = m.rahmen[ri]
                .tabellen_index
                .get(quelle as usize)
                .copied()
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "table.copy: Quelltabelle fehlt"))?;
            // Grenzen VOR dem Kopieren pruefen (M4 und Spec): die Quelle
            // wie auch das Ziel muessen den ganzen Bereich aufnehmen.
            // Ein Bereich von Laenge 0 am Ende ist erlaubt, dahinter nicht.
            //
            // GEMESSEN 03.10. (`table_copy.wast` Z. 2019/2044/2169:
            // 3 `assert_trap` „out of bounds table access" fielen aus —
            // die Quelle lieferte schlicht `None` statt zu trappen).
            {
                let laenge_q = lauf
                    .store
                    .tabellen
                    .get(qi as usize)
                    .map(|t| t.len() as u64)
                    .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Quelltabelle fehlt"))?;
                let laenge_z = lauf
                    .store
                    .tabellen
                    .get(zi as usize)
                    .map(|t| t.len() as u64)
                    .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Zieltabelle fehlt"))?;
                if q.saturating_add(laenge) > laenge_q {
                    return Err(Fehler::trap(
                        T_TABELLE_LESEN,
                        format!("table.copy: Quelle {q}+{laenge} > {laenge_q} Eintraege"),
                    ));
                }
                if z.saturating_add(laenge) > laenge_z {
                    return Err(Fehler::trap(
                        T_TABELLE_SCHREIBEN,
                        format!("table.copy: Ziel {z}+{laenge} > {laenge_z} Eintraege"),
                    ));
                }
            }
            if zi == qi {
                lauf.store
                    .tabellen
                    .get_mut(zi as usize)
                    .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt"))?
                    .kopiere(z, q, laenge)?;
            } else {
                let stuecke: Vec<Option<Ref>> = (0..laenge)
                    .map(|i| {
                        lauf.store
                            .tabellen
                            .get(qi as usize)
                            .and_then(|t| t.lies(q + i).ok())
                            .flatten()
                    })
                    .collect();
                debug_assert_eq!(stuecke.len() as u64, laenge);
                lauf.store
                    .tabellen
                    .get_mut(zi as usize)
                    .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Zieltabelle fehlt"))?
                    .init(&stuecke, z, 0, laenge)?;
            }
            Ok(None)
        }
        TableGrow(i) => {
            let anzahl = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let wert = pop(m)?.als_ref();
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("table.grow: Tabelle {i} fehlt"))
                })?;
            let alt = lauf
                .store
                .tabellen
                .get_mut(ti as usize)
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt"))?
                .waechst(anzahl, wert);
            m.stapel.push(Wert::I32(alt));
            Ok(None)
        }
        TableSize(i) => {
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("table.size: Tabelle {i} fehlt"))
                })?;
            let n = lauf
                .store
                .tabellen
                .get(ti as usize)
                .map(|t| t.len())
                .unwrap_or(0) as i32;
            m.stapel.push(Wert::I32(n));
            Ok(None)
        }
        TableGet(i) => {
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("table.get: Tabelle {i} fehlt"))
                })?;
            let tabelle = lauf
                .store
                .tabellen
                .get(ti as usize)
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt im Store"))?;
            // Der `null`-Wert eines leeren Platzes richtet sich nach dem
            // ELEMENTTYP der Tabelle (siehe `ref_wert_typ`).
            let reftyp = tabelle.reftyp();
            let wert = tabelle.lies(index)?;
            m.stapel.push(ref_wert_typ(wert, &reftyp));
            Ok(None)
        }
        TableSet(i) => {
            let wert = pop(m)?.als_ref();
            let index = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("table.set: Tabelle {i} fehlt"))
                })?;
            lauf.store
                .tabellen
                .get_mut(ti as usize)
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt im Store"))?
                .schreib(index, wert)?;
            Ok(None)
        }
        TableFill(i) => {
            let laenge = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let wert = pop(m)?.als_ref();
            let start = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
            let ti = m.rahmen[ri]
                .tabellen_index
                .get(i as usize)
                .copied()
                .ok_or_else(|| {
                    Fehler::trap(T_TABELLE_LESEN, format!("table.fill: Tabelle {i} fehlt"))
                })?;
            lauf.store
                .tabellen
                .get_mut(ti as usize)
                .ok_or_else(|| Fehler::trap(T_TABELLE_LESEN, "Tabelle fehlt"))?
                .fuelle(start, wert, laenge)?;
            Ok(None)
        }
        I32Konst(v) => {
            m.stapel.push(Wert::I32(v));
            Ok(None)
        }
        I64Konst(v) => {
            m.stapel.push(Wert::I64(v));
            Ok(None)
        }
        F32Konst(bits) => {
            m.stapel.push(Wert::F32(bits));
            Ok(None)
        }
        F64Konst(bits) => {
            m.stapel.push(Wert::F64(bits));
            Ok(None)
        }
        andere => rechnen(m, andere),
    }
}

/// Was geladen/gespeichert wird.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lade {
    I32,
    I64,
    F32,
    F64,
}

/// Sprungziel eines `block`/`loop`/`if` holen.
fn sprungziel(m: &Maschine, ri: usize, block_pos: usize) -> Option<(Option<usize>, usize)> {
    // Die Tabelle gehoert zur FUNKTION und ist beim Instanziieren
    // gebaut (Hebel 1) — vorher lag sie je Rahmen und wurde beim ersten
    // Schritt nachgezogen.
    match m.rahmen.get(ri)?.funktion.as_ref() {
        Funktion::Eigen { sprung, .. } => sprung.ziele.get(block_pos).copied().flatten(),
        Funktion::Fremd { .. } => None,
    }
}

/// `br`/`br_if`/`br_table`: Stapeltransaktion und Sprungziel.
fn br_ausfuehren(m: &mut Maschine, ri: usize, tiefe: u32) -> Result<Option<usize>, Fehler> {
    let n_labels = m.rahmen[ri].labels.len();
    if tiefe as usize >= n_labels {
        return Err(Fehler::trap(
            T_DIVERSE,
            format!(
                "br {tiefe}: nur {n_labels} Labels (0..{})",
                n_labels.saturating_sub(1)
            ),
        ));
    }
    let ziel_idx = n_labels - 1 - tiefe as usize;
    if ziel_idx == 0 {
        // Sprung auf den Funktionskoerper -> Rahmen beenden.
        m.rahmen[ri].labels.clear();
        return Ok(Some(usize::MAX));
    }
    let (schleife, hoehe, arity, start) = {
        let l = &m.rahmen[ri].labels[ziel_idx];
        (l.schleife, l.hoehe, l.arity, l.start)
    };
    // Arity-Werte auf die Label-Hoehe legen — `copy_within` statt
    // `split_off`+`extend`: Spruenge mit Ergebniswerten legen KEINEN
    // `Vec` mehr an (GEMESSEN 04.10., Hebel 2).
    let len = m.stapel.len();
    let a = arity.min(len.saturating_sub(hoehe));
    if a > 0 {
        m.stapel.copy_within(len - a.., hoehe);
    }
    m.stapel.truncate(hoehe + a);
    if schleife {
        m.rahmen[ri].labels.truncate(ziel_idx + 1);
        Ok(Some(start + 1))
    } else {
        m.rahmen[ri].labels.truncate(ziel_idx);
        let (_, end_pos) = sprungziel(m, ri, start)
            .ok_or_else(|| Fehler::trap(T_DIVERSE, format!("br {tiefe}: Block ohne end")))?;
        Ok(Some(end_pos + 1))
    }
}

/// Eine Ausnahme mit dem lokalen Tagindex `tag` behandeln: die Labels von
/// innen nach aussen durchsuchen. Passt ein `try_table`, wird dorthin
/// gesprungen; sonst verlaesst die Ausnahme den Rahmen und wird im
/// Aufrufer weiter gesucht (`m.ausnahme`).
///
/// Exception Handling, Wasm 3.0. GEMESSEN 02.10. (`instance.wast`: 12
/// Faelle — eine Instanz wirft einen Tag, den nur ein Tag der ANDEREN
/// Instanz faengt; `catch_all` faengt ihn stattdessen).
fn ausnahme_werfen(
    m: &mut Maschine,
    ri: usize,
    tag: u32,
    koerper_len: usize,
) -> Result<Option<usize>, Fehler> {
    let eigener = match m.rahmen[ri].tags.get(tag as usize) {
        Some(g) => *g,
        None => {
            return Err(Fehler::trap(
                T_DIVERSE,
                format!("throw: Tag {tag} fehlt im Instanzraum"),
            ));
        }
    };
    let n_labels = m.rahmen[ri].labels.len();
    for tiefe in 0..n_labels {
        let ziel_idx = n_labels - 1 - tiefe;
        let faenge = m.rahmen[ri].labels[ziel_idx].faenge.clone();
        if faenge.is_empty() {
            continue;
        }
        // Der ERSTE passende Zweig gewinnt: `catch $t` vor `catch_all`.
        let treffer = faenge.iter().find(|f| match f.tag {
            Some(t) => m.rahmen[ri].tags.get(t as usize) == Some(&eigener),
            None => true,
        });
        let ziel = match treffer {
            Some(f) => f.label,
            // Dieses `try_table` passt nicht: die Ausnahme fliegt weiter
            // nach aussen (sein Label verschwindet beim Sprung).
            None => continue,
        };
        match ziel {
            // Weiterwerfen: das `try_table`-Label faellt weg, die Suche
            // geht beim naechsten Label weiter.
            None => {
                m.rahmen[ri].labels.truncate(ziel_idx);
                continue;
            }
            Some(rel) => {
                // Die catch-Label sind relativ zu den umschliessenden
                // Labels angegeben (das `try_table`-Label steht beim Lesen
                // noch NICHT im Stapel) — zur Laufzeit kommt seine Ebene
                // dazu.
                //
                // GEMESSEN 02.10. (`instance.wast`: ohne die `+ 1` sprang
                // `catch_all` auf das try_table-Ende und danach lief
                // `(unreachable)`).
                let zum_ziel = (tiefe as u32) + rel + 1;
                return br_ausfuehren(m, ri, zum_ziel);
            }
        }
    }
    // Kein Fang in diesem Rahmen: der Rahmen endet mit der Ausnahme.
    m.rahmen[ri].labels.clear();
    m.ausnahme = Some(Ausnahme(eigener));
    Ok(Some(koerper_len))
}

/// Laenge des Koerpers eines Rahmens (0 fuer Hostfunktionen).
fn rahmen_koerper_len(r: &Rahmen) -> usize {
    match r.funktion.as_ref() {
        Funktion::Eigen { koerper, .. } => koerper.len(),
        Funktion::Fremd { .. } => 0,
    }
}

/// Nach einem Rahmenende mit offener Ausnahme: im neuen obersten Rahmen
/// weiter suchen. Findet sich dort kein Fang, endet auch dieser Rahmen
/// (die Ausnahme bleibt fuer den naechsten Aufrufer offen).
///
/// Exception Handling, Wasm 3.0. GEMESSEN 02.10. (`instance.wast`: der
/// `throw` steht in einer exportierten Funktion, der `try_table` in der
/// aufrufenden Instanz).
fn ausnahme_weitergeben(m: &mut Maschine) {
    let Some(ri) = m.rahmen.len().checked_sub(1) else {
        return;
    };
    let Some(Ausnahme(tag)) = m.ausnahme else {
        return;
    };
    let n_labels = m.rahmen[ri].labels.len();
    for tiefe in 0..n_labels {
        let ziel_idx = n_labels - 1 - tiefe;
        let faenge = m.rahmen[ri].labels[ziel_idx].faenge.clone();
        if faenge.is_empty() {
            continue;
        }
        let treffer = faenge.iter().find(|f| match f.tag {
            Some(t) => m.rahmen[ri].tags.get(t as usize) == Some(&tag),
            None => true,
        });
        let ziel = match treffer {
            Some(f) => f.label,
            None => continue,
        };
        let rel = match ziel {
            // Weiterwerfen: das `try_table`-Label faellt weg.
            None => {
                m.rahmen[ri].labels.truncate(ziel_idx);
                continue;
            }
            Some(r) => r,
        };
        // Wie in `ausnahme_werfen`: das `try_table`-Label kommt dazu.
        let zum_ziel = (tiefe as u32) + rel + 1;
        match br_ausfuehren(m, ri, zum_ziel) {
            Ok(Some(z)) => {
                m.rahmen[ri].pc = z;
                m.ausnahme = None;
            }
            // Sprung ans Rahmenende: auch dieser Rahmen ist vorbei.
            Ok(None) => m.rahmen[ri].pc = rahmen_koerper_len(&m.rahmen[ri]),
            Err(_) => m.rahmen[ri].pc = rahmen_koerper_len(&m.rahmen[ri]),
        }
        return;
    }
    // Kein Fang: dieser Rahmen endet ebenfalls.
    m.rahmen[ri].labels.clear();
    m.rahmen[ri].pc = rahmen_koerper_len(&m.rahmen[ri]);
}

/// Unteraufruf: Fremdfunktion direkt, WASM-Funktion als neuer Rahmen.
/// `ri` ist der aufrufende Rahmen (fuer Speicher/Tabellen/Typraum),
/// `n` die Zahl der Argumente — sie liegen schon oben auf dem Stapel.
///
/// GEMESSEN 04.10. (Hebel 1): kein `to_vec` mehr. Die Argumente
/// bleiben liegen und werden zu den Locals des neuen Rahmens (die
/// Locals liegen im gemeinsamen Stapel).
fn unteraufruf(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    m: &mut Maschine,
    ri: usize,
    global: u32,
    n: usize,
) -> Result<(), Fehler> {
    // Hebel 2 (GEMESSEN 04.10.): Brennstoff am AUFRUF pruefen —
    // Rekursion ist die zweite Unendlichkeitsquelle — vor dem Aufbau
    // des Rahmens.
    lauf.pruefe_brennstoff()?;
    if m.stapel.len() < n {
        return Err(Fehler::trap(
            T_DIVERSE,
            format!("call: {n} Argumente fehlen auf dem Stapel"),
        ));
    }
    let f = lauf
        .store
        .funktionen
        .get(global as usize)
        .cloned()
        .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("Funktion {global} fehlt im Store")))?;
    match f.as_ref() {
        Funktion::Fremd { typ, funktion } => {
            let anfang = m.stapel.len() - n;
            let r = fremd_rufen(lauf, host, typ, funktion, &m.stapel[anfang..], global)?;
            m.stapel.truncate(anfang);
            m.stapel.extend(r);
        }
        Funktion::Eigen { typ, lokale, .. } => {
            if typ.parameter.len() != n {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!(
                        "Funktion {global}: {} Parameter erwartet, {}",
                        typ.parameter.len(),
                        n
                    ),
                ));
            }
            // Die Argumente bleiben liegen (Hebel 1): sie sind die
            // ersten Locals, die uebrigen Locals kommen dahinter.
            let lokal_basis = m.stapel.len() - n;
            for t in lokale {
                m.stapel.push(Wert::standard(*t));
            }
            let basis = m.stapel.len();
            let arity = typ.ergebnis.len();
            let kontext = Kontext::aus_store(lauf, global, &m.rahmen[ri]);
            // Der neue Rahmen laeuft im Kontext der INSTANZ, die die
            // Funktion definiert — nicht im Kontext des Aufrufers. Nur so
            // liest eine importierte Funktion den Speicher ihres eigenen
            // Moduls.
            //
            // GEMESSEN 03.10. (`linking1.wast` Z. 4/8: 0xf2 statt 0xa7,
            // `linking.wast`/`memory_copy1.wast`/`global.wast`).
            pruefe_tiefe(lauf, m.rahmen.len() + 1)?;
            m.rahmen.push(Rahmen::in_kontext(
                kontext,
                f.clone(),
                lokal_basis,
                basis,
                arity,
            ));
            lauf.zaehle(1);
        }
    }
    Ok(())
}

/// END-AUFRUF (`return_call`, `return_call_indirect`, `return_call_ref`):
/// der Rahmen `ri` wird durch den der aufgerufenen Funktion ERSETZT.
///
/// Die Parameter der Zielfunktion liegen bereits als Ergebnis des
/// aufrufenden Rahmens auf dem Stapel — der Validator prueft genau das,
/// also wird hier nichts abgezogen.
///
/// Warum das der Sinn von Tail Calls ist: der Aufrufstapel waechst nicht.
/// Ein End-Aufruf in einer Kette von Millionen Aufrufen laeuft daher im
/// gleichen Speicher wie ein einziger.
///
/// GEMESSEN 03.10. (`return_call*.wast`: 119 Faelle; vorher wuchs der
/// Rahmenstapel, bis der Grenzfehler kam).
fn schwanzaufruf(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    m: &mut Maschine,
    ri: usize,
    global: u32,
    koerper_len: usize,
) -> Result<Option<usize>, Fehler> {
    // Hebel 2 (GEMESSEN 04.10.): Brennstoff am AUFRUF pruefen — auch
    // wenn kein Rahmen entsteht (Tail-Call), ist der Koerperwechsel
    // die Unendlichkeitsquelle.
    lauf.pruefe_brennstoff()?;
    let f = lauf
        .store
        .funktionen
        .get(global as usize)
        .cloned()
        .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("Funktion {global} fehlt im Store")))?;
    match f.as_ref() {
        Funktion::Fremd { typ, funktion } => {
            // Fremdfunktion: Ergebnis holen, dann endet der Rahmen wie
            // bei `return`.
            let n = typ.parameter.len();
            let basis = m.rahmen[ri].basis;
            if m.stapel.len().saturating_sub(basis) < n {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!("return_call: {n} Argumente fehlen auf dem Stapel"),
                ));
            }
            let anfang = m.stapel.len() - n;
            // KEIN `to_vec` (Hebel 1): die Argumente werden gelesen und
            // danach abgeraeumt.
            let r = fremd_rufen(lauf, host, typ, funktion, &m.stapel[anfang..], global)?;
            m.stapel.truncate(anfang);
            m.stapel.extend(r);
            m.rahmen[ri].labels.clear();
            m.rahmen[ri].pc = koerper_len;
            Ok(Some(koerper_len))
        }
        Funktion::Eigen { typ, lokale, .. } => {
            let n = typ.parameter.len();
            let basis = m.rahmen[ri].basis;
            if m.stapel.len().saturating_sub(basis) < n {
                return Err(Fehler::trap(
                    T_INDIREKT,
                    format!("return_call: {n} Argumente fehlen auf dem Stapel"),
                ));
            }
            // Locals liegen IM gemeinsamen Stapel (Hebel 1): die
            // Argumente werden an die Stelle der ALTEN Locals
            // verschoben (`copy_within` schiebt ohne Allokation), die
            // uebrigen Locals kommen dahinter.
            let lokal_alt = m.rahmen[ri].lokal_basis;
            let neu = m.stapel.len() - n;
            m.stapel.copy_within(neu.., lokal_alt);
            m.stapel.truncate(lokal_alt + n);
            for t in lokale {
                m.stapel.push(Wert::standard(*t));
            }
            let basis = m.stapel.len();
            // Gleicher Kontext? Dann den des Rahmens behalten.
            let gleich = lauf
                .store
                .funktion_kontext
                .get(global as usize)
                .and_then(|k| k.as_deref())
                .is_some_and(|k| Kontext::gleich_wie(k, &m.rahmen[ri]));
            let kontext = if gleich {
                None
            } else {
                Some(Kontext::aus_store(lauf, global, &m.rahmen[ri]))
            };
            let arity = typ.ergebnis.len();
            // Der Rahmen wird an Ort und Stelle ueberschrieben statt neu
            // gebaut: das spart je Aufruf eine `Vec<Label>` und den
            // Abbau des alten Rahmens.
            //
            // GEMESSEN 03.10. (5 Mio. End-Aufrufe: 1478 ms mit neuem
            // Rahmen, siehe Bericht; ein normaler Schritt kostet 21 ms
            // je 1 Mio.).
            let r = &mut m.rahmen[ri];
            r.funktion = f.clone();
            r.lokal_basis = lokal_alt;
            r.basis = basis;
            r.pc = 0;
            r.labels.clear();
            r.labels.push(Label {
                start: 0,
                hoehe: basis,
                arity,
                schleife: false,
                faenge: Vec::new(),
            });
            if let Some(k) = kontext {
                r.arrays = k.arrays;
                r.strukt = k.strukt;
                r.speicher = k.speicher;
                r.daten_start = k.daten_start;
                r.elemente_start = k.elemente_start;
                r.tags = k.tags;
                r.tabellen_index = k.tabellen_index;
                r.tabelle = k.tabelle;
                r.typen = k.typen;
                r.globale_index = k.globale_index;
                r.funktionen = k.funktionen;
            }
            // `r.sprung = None` entfaellt: die Sprungtabelle gehoert zur
            // FUNKTION (EINMAL gebaut, Hebel 1), nicht zum Rahmen.
            lauf.zaehle(1);
            Ok(None)
        }
    }
}

impl Rahmen {
    /// Lokaler Funktionsindex -> Index im Store.
    fn store_funktion(&self, lokal: u32) -> u32 {
        self.funktionen
            .get(lokal as usize)
            .copied()
            .unwrap_or(lokal)
    }

    /// Der eigene Ausfuehrungskontext (als `Kontext`).
    fn kontext(&self) -> Kontext {
        Kontext {
            sub_ober: self.sub_ober.clone(),
            fn_typ: self.fn_typ.clone(),
            ziel_gruppe: self.ziel_gruppe.clone(),
            typ_final: self.typ_final.clone(),
            arrays: self.arrays.clone(),
            strukt: self.strukt.clone(),
            speicher: self.speicher.clone(),
            daten_start: self.daten_start,
            elemente_start: self.elemente_start,
            tags: self.tags.clone(),
            tabellen_index: self.tabellen_index.clone(),
            tabelle: self.tabelle,
            typen: self.typen.clone(),
            globale_index: self.globale_index.clone(),
            funktionen: self.funktionen.clone(),
        }
    }

    /// Ein neuer Rahmen in diesem Kontext.
    ///
    /// GEMESSEN 03.10. (Refactoring fuer `return_call`).
    fn in_kontext(
        k: Kontext,
        funktion: Arc<Funktion>,
        lokal_basis: usize,
        basis: usize,
        arity: usize,
    ) -> Rahmen {
        Rahmen {
            funktion,
            lokal_basis,
            basis,
            pc: 0,
            ziel_gruppe: k.ziel_gruppe.clone(),
            typ_final: k.typ_final.clone(),
            arrays: k.arrays.clone(),
            strukt: k.strukt.clone(),
            labels: vec![Label {
                start: 0,
                hoehe: basis,
                arity,
                schleife: false,
                faenge: Vec::new(),
            }],
            speicher: k.speicher,
            daten_start: k.daten_start,
            elemente_start: k.elemente_start,
            tags: k.tags,
            tabellen_index: k.tabellen_index,
            tabelle: k.tabelle,
            typen: k.typen,
            sub_ober: k.sub_ober,
            fn_typ: k.fn_typ,
            globale_index: k.globale_index,
            funktionen: k.funktionen,
        }
    }
}

fn blocktypen(r: &Rahmen, bt: &Blockart) -> (usize, usize) {
    match bt {
        Blockart::Leer => (0, 0),
        Blockart::Werttyp(_) => (0, 1),
        Blockart::Typ(i) => r
            .typen
            .get(*i as usize)
            .map(|t| (t.parameter.len(), t.ergebnis.len()))
            .unwrap_or((0, 0)),
    }
}

/// Passt der WERT zum Referenztyp? (`ref.test` / `ref.cast`)
///
/// GEMESSEN 03.10. (`i31.wast`, `ref_test.wast`, `ref_cast.wast`).
/// Darf eine Funktion vom Typindex `aufgerufen` ueber den Zieltypindex
/// `ziel` gerufen werden? (`call_indirect`, `return_call_indirect`,
/// `call_ref`, `return_call_ref` — **eine** Relation fuer alle vier.)
///
/// Die Stationen, jede mit ihrer eigenen Messung:
///
/// 1. **Gleicher Typindex** — Wasm 2.0.
/// 2. **Die `(sub …)`-Kette** — `$t1 ⊑ $t0` heisst „darf als `$t0`
///    gerufen werden" (GEMESSEN 03.10., `type-subtyping.wast` Z. 283ff,
///    7 Faelle).
/// 3. **Gleiche Gruppenform** — strukturell gleich ueber Gruppengrenzen
///    (GEMESSEN 04.10., Z. 476ff/492ff; dieselbe Form wie `ref.test`).
/// 4. **Signatur**, aber **nur wenn beide Typen final sind**.
///
/// GEMESSEN 04.10. (`type-subtyping.wast`):
/// * Z. 344ff (2 Faelle „Trap erwartet, keiner"): `$t1 (sub (func))` und
///   `$t2 (sub final (func))` sind beide `(func)` — aber final gegen
///   nicht-final ist **kein** Subtyp, in keiner Richtung.
/// * Z. 373ff (`fail1`): `call_indirect (type $t3)` auf eine Funktion vom
///   Typ `$t2` trappt. Beide sind `(func)` und beide **nicht-final**, und
///   `$t2 ⊑ $t3` gilt **nicht** — Station 4 liefert also nichts, und die
///   Kette liefert auch nichts ⇒ Trap. Vorher kam der Trap **zufaellig**
///   aus der Tabellensperre („haelt keine funcref-Eintraege"); mit der
///   Tabelle `(ref null $t2)` muss die **Typrelation** tragen.
///
/// Eine importierte Funktion kennt keinen Typindex im laufenden Modul —
/// da entscheidet wie in Wasm 2.0 die Signatur.
fn subtyp_indirekt(
    lauf: &Lauf,
    k: &Rahmen,
    aufgerufen: u32,
    ziel: u32,
    n_param: &FuncTyp,
    ziel_sig: &FuncTyp,
) -> bool {
    subtyp_in(
        lauf,
        &k.fn_typ,
        &k.sub_ober,
        &k.ziel_gruppe,
        &k.typ_final,
        aufgerufen,
        ziel,
        n_param,
        ziel_sig,
    )
}

/// Dieselbe Pruefung fuer den SCHNELLEN Pfad (T2c `call_indirect`):
/// dieselben vier Raeume, aber aus dem `ModulKontext` statt aus dem
/// alten Rahmen — der Cache gilt fuer alle Instanzen des Moduls.
pub(crate) fn subtyp_indirekt_ktx(
    lauf: &Lauf,
    k: &crate::lauf::ModulKontext,
    aufgerufen: u32,
    ziel: u32,
    n_param: &FuncTyp,
    ziel_sig: &FuncTyp,
) -> bool {
    subtyp_in(
        lauf,
        &k.fn_typ,
        &k.sub_ober,
        &k.ziel_gruppe,
        &k.typ_final,
        aufgerufen,
        ziel,
        n_param,
        ziel_sig,
    )
}

/// Der Kern beider Aufrufer: Typraum + `(sub …)`-Kette + Gruppen +
/// finale Typen — Reihenfolge und Ergebnis wie zuvor im Rahmen.
fn subtyp_in(
    lauf: &Lauf,
    fn_typ: &[Option<u32>],
    sub_ober: &[Option<u32>],
    ziel_gruppe: &[crate::modell::Gruppenform],
    typ_final: &[bool],
    aufgerufen: u32,
    ziel: u32,
    n_param: &FuncTyp,
    ziel_sig: &FuncTyp,
) -> bool {
    let Some(ti) = fn_typ.get(aufgerufen as usize).copied().flatten() else {
        return typ_gleich(n_param, ziel_sig);
    };
    if ti == ziel {
        return true;
    }
    if crate::modell::subtyp_ueber_kette_in(sub_ober, ti, ziel) {
        return true;
    }
    if let (Some((a, pa)), Some((b, pb))) = (
        lauf.store
            .funktion_gruppe
            .get(aufgerufen as usize)
            .cloned()
            .flatten(),
        ziel_gruppe.get(ziel as usize).cloned().flatten(),
    ) {
        if pa == pb && a == b {
            return true;
        }
    }
    // Wasm 2.0: strukturelle Signatur — nur wenn **beide** final sind.
    let vom_final = typ_final.get(ti as usize).copied().unwrap_or(true);
    let zum_final = typ_final.get(ziel as usize).copied().unwrap_or(true);
    vom_final && zum_final && typ_gleich(n_param, ziel_sig)
}

fn wert_passt_zu(lauf: &Lauf, w: &Wert, ziel: &Reftyp) -> bool {
    use crate::modell::{GcInhalt, JEDER_ARRAYTYP, JEDER_EQTYP, JEDER_EXNTYP, JEDER_I31TYP};
    use crate::modell::{JEDER_STRUKTYP, JEDER_TYP};
    // `null` passt zu jedem Typ, der `null` erlaubt.
    let ist_null = matches!(w, Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None));
    if ist_null {
        return ziel.ist_nullable();
    }
    // Die Wildcards des Lattices.
    if let Reftyp::Konkret { typ: z, .. } = ziel {
        match *z {
            JEDER_TYP => return true,
            JEDER_EQTYP => return matches!(w, Wert::Gcref(Some(_))),
            JEDER_I31TYP => return matches!(gc_inhalt(lauf, w), Some(GcInhalt::I31(_))),
            JEDER_ARRAYTYP => return matches!(gc_inhalt(lauf, w), Some(GcInhalt::Array(_))),
            JEDER_STRUKTYP => return matches!(gc_inhalt(lauf, w), Some(GcInhalt::Feld(_))),
            JEDER_EXNTYP => return matches!(w, Wert::Extern(Some(_))),
            crate::modell::JEDER_FUNKTIONSTYP => return matches!(w, Wert::Func(Some(_))),
            crate::modell::JEDER_EXTERNTYP => return matches!(w, Wert::Extern(Some(_))),
            crate::modell::KEIN_TYP
            | crate::modell::KEIN_FUNKTIONSTYP
            | crate::modell::KEIN_EXTERNTYP
            | crate::modell::KEIN_EXNTYP => return false,
            _ => {
                // Ein konkreter Typindex: das GC-Objekt muss genau diesen
                // Typ haben (der Typindex stammt aus demselben Modul).
                return id_von(w)
                    .and_then(|i| lauf.store.gc.get(i))
                    .is_some_and(|o| o.typ == *z);
            }
        }
    }
    match ziel {
        Reftyp::Func => matches!(w, Wert::Func(Some(_))),
        Reftyp::Extern => matches!(w, Wert::Extern(Some(_))),
        _ => false,
    }
}

/// Das (nullte) Element eines Arrays an `index` — Trap ausserhalb.
///
/// GEMESSEN 03.10. (`array.wast`: `get`/`get_s`/`get_u`).
fn array_element(lauf: &Lauf, id: u32, index: usize) -> Result<(u32, Wert), Fehler> {
    let o = lauf
        .store
        .gc
        .get(id as usize)
        .ok_or_else(|| Fehler::trap(T_INDIREKT, "Arrayobjekt fehlt im Haufen"))?;
    match &o.inhalt {
        crate::modell::GcInhalt::Array(e) => match e.get(index) {
            Some(w) => Ok((o.laenge, *w)),
            None => Err(Fehler::trap(
                T_INDIREKT,
                format!("Index {index} ausserhalb (Array mit {} Plaetzen)", o.laenge),
            )),
        },
        _ => Err(Fehler::trap(T_INDIREKT, "Wert ist kein Array")),
    }
}

/// Der GC-Index eines Arrays. `null` ist ein Trap mit dem Namen, den die
/// Suite erwartet („null array reference", GEMESSEN 03.10.).
fn array_id(w: &Wert, was: &str) -> Result<u32, Fehler> {
    id_von(w)
        .map(|i| i as u32)
        .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("{was}: null array reference")))
}

/// Die Laenge eines GC-Objekts (0, wenn es keins gibt).
/// Der Arraytyp hinter einem Typindex (aus dem Rahmen des Moduls).
fn array_typen(
    arrays: &[Option<crate::modell::ArrayTyp>],
    t: u32,
) -> Option<crate::modell::ArrayTyp> {
    arrays.get(t as usize).and_then(|a| a.as_ref()).cloned()
}

/// Einen Wert fuer einen gepackten Array auf seine Bit-Zahl kuerzen.
fn packe(at: &Option<crate::modell::ArrayTyp>, w: Wert) -> Wert {
    match at {
        Some(a) if a.bits > 0 => Wert::I32(crate::modell::kuerzen_und_vorzeichen(
            w.als_i32().unwrap_or(0),
            a.bits,
        )),
        _ => w,
    }
}

/// `groesse` Byte aus dem Datenabschnitt `idx` ab `offset` als Werte —
/// `element_breite` Byte je Element, **kleinsteEndian**.
///
/// GEMESSEN 03.10. (`array_new_data.wast`: „out of bounds memory access").
fn daten_als_werte(
    lauf: &Lauf,
    idx: u32,
    offset: usize,
    groesse: usize,
    at: &Option<crate::modell::ArrayTyp>,
) -> Result<Vec<Wert>, Fehler> {
    let breite = at.as_ref().map(|a| a.element_breite()).unwrap_or(0);
    if breite == 0 || breite > 8 {
        return Err(Fehler::trap(
            T_INDIREKT,
            format!("array.new_data: out of bounds memory access (Element {breite} Byte)"),
        ));
    }
    // Die Laenge ist eine ELEMENTzahl, keine Bytezahl (GEMESSEN 03.10.:
    // `array_init_data_i16 (2, 5, 2)` schreibt „fg" und „hi", also zwei
    // Elemente aus den Datenbytes 5..9).
    let anzahl = groesse;
    let benoetigt = groesse.checked_mul(breite as usize).ok_or_else(|| {
        Fehler::trap(
            T_INDIREKT,
            "array.new_data: out of bounds memory access (Laenge timeschlaegt)",
        )
    })?;
    let bytes: Vec<u8> = match lauf.store.daten.get(idx as usize) {
        Some(Some(d)) => d.clone(),
        Some(None) => Vec::new(),
        None => {
            return Err(Fehler::trap(
                T_INDIREKT,
                format!("array.new_data: Datenabschnitt {idx} unbekannt"),
            ));
        }
    };
    if offset
        .checked_add(benoetigt)
        .map_or(true, |e| e > bytes.len())
    {
        return Err(Fehler::trap(
            T_INDIREKT,
            format!(
                "array.new_data: out of bounds memory access \
                 ({offset}+{benoetigt} von {} Byte fuer {anzahl} Elemente zu {groesse} Byte)",
                bytes.len()
            ),
        ));
    }
    // M3/M4: die Buchung begrenzt die Groesse; die Schleife laeuft ueber
    // die Eingabe, ihre Grenze ist die Zahl der Elemente.
    let mut r: Vec<Wert> = Vec::with_capacity(anzahl.min(4096));
    for i in 0..anzahl {
        let start = offset + i * breite as usize;
        // KleinsteEndian aus den (vorhandenen) Bytes — **kein** Indexzugriff
        // ueber `stueck.len()` hinaus (GEMESSEN 03.10.: PANIK „index out of
        // bounds: the len is 1 but the index is 1").
        let mut n: i64 = 0;
        for (k, b) in bytes[start..start + breite as usize]
            .iter()
            .take(8)
            .enumerate()
        {
            n |= (*b as i64) << (8 * k);
        }
        r.push(match at.as_ref() {
            // Gepackt (i8/i16): auf die Bit-Zahl kuerzen, vorzeichenbehaftet.
            Some(a) if a.bits > 0 => {
                Wert::I32(crate::modell::kuerzen_und_vorzeichen(n as i32, a.bits))
            }
            // 8 Byte: als i64/f64 ablegen.
            Some(a) if breite == 8 => match a.inhalt {
                Valtyp::F64 => Wert::F64(n as u64),
                _ => Wert::I64(n),
            },
            // 4 Byte: als i32/f32 ablegen.
            Some(a) if breite == 4 => match a.inhalt {
                Valtyp::F32 => Wert::F32(n as u32),
                _ => Wert::I32(n as i32),
            },
            // 1 oder 2 Byte: i32.
            _ => Wert::I32(n as i32),
        });
    }
    Ok(r)
}

/// `groesse` Verweise aus dem Elementabschnitt `idx` ab `offset`.
///
/// GEMESSEN 03.10. (`array_new_elem.wast`: „out of bounds table access").
fn elem_als_werte(
    lauf: &Lauf,
    idx: u32,
    offset: usize,
    groesse: usize,
) -> Result<Vec<Wert>, Fehler> {
    let gesamt = match lauf.store.elemente.get(idx as usize) {
        Some(Some(e)) => e.len(),
        Some(None) => 0,
        None => {
            return Err(Fehler::trap(
                T_INDIREKT,
                format!("array.new_elem: Elementabschnitt {idx} unbekannt"),
            ));
        }
    };
    if offset.checked_add(groesse).map_or(true, |e| e > gesamt) {
        return Err(Fehler::trap(
            T_INDIREKT,
            format!("array.new_elem: out of bounds table access ({offset}+{groesse} von {gesamt})"),
        ));
    }
    // Die Verweise eines Elementabschnitts sind `Option<Ref>`; `null` wird
    // ueber `ref_wert_typ` zum Standardwert des Zieltyps.
    match lauf.store.elemente.get(idx as usize) {
        Some(Some(e)) => Ok(e[offset..offset + groesse]
            .iter()
            .map(|r| ref_wert_typ(*r, &Reftyp::Extern))
            .collect()),
        _ => Ok(Vec::new()),
    }
}

/// `werte` ab `index` in den Array `id` schreiben — Trap, wenn es nicht
/// hineinpasst.
///
/// GEMESSEN 03.10. (`array_init_data.wast`: „out of bounds array access").
fn array_abschnitt_schreiben(
    lauf: &mut Lauf,
    id: u32,
    index: usize,
    werte: &[Wert],
    was: &str,
) -> Result<(), Fehler> {
    let laenge = array_len(lauf, id);
    if index as u64 + werte.len() as u64 > laenge as u64 {
        return Err(Fehler::trap(
            T_INDIREKT,
            format!(
                "{was}: out of bounds array access \
                 (Index {index} + {} Elemente, Laenge {laenge})",
                werte.len()
            ),
        ));
    }
    match lauf.store.gc.get_mut(id as usize) {
        Some(crate::modell::GcObjekt {
            inhalt: crate::modell::GcInhalt::Array(e),
            ..
        }) => {
            for (i, w) in werte.iter().enumerate() {
                if let Some(stelle) = e.get_mut(index + i) {
                    *stelle = *w;
                }
            }
            Ok(())
        }
        Some(_) => Err(Fehler::trap(T_INDIREKT, "Wert ist kein Array")),
        None => Err(Fehler::trap(T_INDIREKT, "Objekt fehlt im Haufen")),
    }
}
fn array_len(lauf: &Lauf, id: u32) -> u32 {
    lauf.store
        .gc
        .get(id as usize)
        .map(|o| o.laenge)
        .unwrap_or(0)
}

/// Der GC-Index eines Wertes — `None` bei allem, was KEIN GC-Objekt ist.
///
/// GEMESSEN 03.10. (`ref_cast.wast`: „Trap erwartet, keiner"): mit einer
/// Null-Rueckgabe schlug der Test fuer `externref`/`funcref` nach
/// GC-Objekt 0 nach — und ein `ref.cast i31ref` auf einen Hostwert
/// wurde dadurch erfolgreich.
/// M3/M8: die ELEMENTZAHL eines Arrays ist begrenzt — sonst beendet
/// `(array.new_default $t (i32.const 0xffff_ffff))` den ganzen Prozess.
///
/// GEMESSEN 03.10.: SIGKILL (OOM) ohne diese Pruefung.
fn gc_laenge_pruefen(lauf: &mut Lauf, laenge: u32) -> Result<(), Fehler> {
    if laenge as usize > crate::grenzen::MAX_GC_ELEMENTS_HART {
        // KEIN Brennstoff verbrauchen: die Pruefung ist O(1) und lehnt
        // sofort ab. Der erste Versuch verbrauchte den GANZEN Brennstoff
        // und riss danach jedem Aufruf das Limit weg („Brennstoff {
        // verbraucht: 1000, grenze: 1000}").
        let _ = lauf;
        return Err(Fehler::trap(
            T_DIVERSE,
            format!(
                "Arraylaenge {laenge} > {} (MAX_GC_ELEMENTS_HART)",
                crate::grenzen::MAX_GC_ELEMENTS_HART
            ),
        ));
    }
    Ok(())
}

/// Die Felder eines Strukturtyps (aus dem Rahmen des laufenden Moduls).
fn strukt_felder(
    strukt: &[Option<crate::modell::StructTyp>],
    t: u32,
) -> Option<Vec<crate::modell::StructFeld>> {
    strukt
        .get(t as usize)
        .and_then(|s| s.as_ref())
        .map(|s| s.felder.clone())
}

/// Die Standardwerte aller Felder (`struct.new_default`).
fn strukt_standard_werte(strukt: &[Option<crate::modell::StructTyp>], t: u32) -> Option<Vec<Wert>> {
    strukt
        .get(t as usize)
        .and_then(|s| s.as_ref())
        .map(|s| s.standard_werte())
}

/// Der GC-Index einer Struktur. `null` ist ein Trap mit dem Namen, den
/// die Testsuite erwartet (`struct.wast`: „null structure reference").
fn strukt_id(w: &Wert, was: &str) -> Result<u32, Fehler> {
    id_von(w)
        .map(|i| i as u32)
        .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("{was}: null structure reference")))
}

/// Der Wert des Feldes `f` — Trap, wenn es das Feld nicht gibt.
fn feld_wert(lauf: &Lauf, id: u32, f: usize, was: &str) -> Result<Wert, Fehler> {
    let o = lauf
        .store
        .gc
        .get(id as usize)
        .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("{was}: Objekt fehlt im Haufen")))?;
    match &o.inhalt {
        crate::modell::GcInhalt::Feld(werte) => werte
            .get(f)
            .copied()
            .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("{was}: Feld {f} fehlt"))),
        _ => Err(Fehler::trap(T_INDIREKT, "Wert ist keine Struktur")),
    }
}

/// Der Standardwert des ELEMENTTYPS eines Arraytyps (`null` fuer
/// Referenztypen, `0` fuer Zahlen).
///
/// GEMESSEN 03.10. (`array.wast`: `array.new_default` muss den Array mit
/// den Standardwerten fuellen — ohne den Elementtyp standen dort lauter
/// Nullen).
fn element_standard(arrays: &[Option<crate::modell::ArrayTyp>], t: u32) -> Option<Wert> {
    arrays
        .get(t as usize)
        .and_then(|a| a.as_ref())
        .map(|a| Wert::standard(a.inhalt))
}

fn id_von(w: &Wert) -> Option<usize> {
    match w {
        Wert::Gcref(Some(i)) => Some(*i as usize),
        _ => None,
    }
}

/// Der Inhalt eines GC-Objekts (nur wenn der Wert eines ist).
fn gc_inhalt<'a>(lauf: &'a Lauf, w: &Wert) -> Option<&'a crate::modell::GcInhalt> {
    id_von(w)
        .and_then(|i| lauf.store.gc.get(i))
        .map(|o| &o.inhalt)
}

fn pop(m: &mut Maschine) -> Result<Wert, Fehler> {
    m.stapel
        .pop()
        .ok_or_else(|| Fehler::trap(T_DIVERSE, "Operandstapel leer"))
}

// T2b: `schnell.rs` benutzt dieselben Aufloeser — dieselben Trap-Texte,
// dieselbe Semantik, EIN Weg statt zwei.
pub(crate) fn speicher_von(
    lauf: &Lauf,
    idx: Option<u32>,
) -> Result<&crate::speicher::Speicher, Fehler> {
    let i = idx.ok_or_else(|| Fehler::trap(T_SPEICHER_LESEN, "kein Speicher in dieser Instanz"))?;
    lauf.store
        .speicher
        .get(i as usize)
        .ok_or_else(|| Fehler::trap(T_SPEICHER_LESEN, format!("Speicher {i} fehlt")))
}

pub(crate) fn speicher_mut(
    lauf: &mut Lauf,
    idx: Option<u32>,
) -> Result<&mut crate::speicher::Speicher, Fehler> {
    let i = idx.ok_or_else(|| Fehler::trap(T_SPEICHER_LESEN, "kein Speicher in dieser Instanz"))?;
    lauf.store
        .speicher
        .get_mut(i as usize)
        .ok_or_else(|| Fehler::trap(T_SPEICHER_LESEN, format!("Speicher {i} fehlt")))
}

/// Die Store-Indizes von Ziel- und Quellspeicher eines
/// `memory.copy ziel quelle` (Multi-Memory).
fn speicher_paar(m: &Maschine, ri: usize, ziel: u32, quelle: u32) -> Result<(u32, u32), Fehler> {
    let speicher = &m.rahmen[ri].speicher;
    let hole = |i: u32| -> Result<u32, Fehler> {
        speicher.get(i as usize).copied().ok_or_else(|| {
            Fehler::trap(
                T_SPEICHER_LESEN,
                format!(
                    "memory.copy: Speicher {i} unbekannt ({} Speicher)",
                    speicher.len()
                ),
            )
        })
    };
    Ok((hole(ziel)?, hole(quelle)?))
}

/// Lade-Befehl. `breite` in Bit: 0=1 Byte, 1=2, 2=4, 3=8.
fn lade(
    m: &mut Maschine,
    lauf: &mut Lauf,
    speicher: Option<u32>,
    versatz: u32,
    art: Lade,
    breite: u32,
    signed: bool,
) -> Result<Option<usize>, Fehler> {
    let adresse = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
    let adresse = adresse.wrapping_add(versatz as u64);
    let bytes = 1usize << breite;
    let sp = speicher_von(lauf, speicher)?;
    let roh = sp.lies(adresse, bytes)?;
    let wert = match art {
        Lade::I32 | Lade::F32 => {
            let mut v: u32 = 0;
            for (i, b) in roh.iter().enumerate() {
                v |= (*b as u32) << (8 * i);
            }
            match art {
                Lade::F32 => Wert::F32(v),
                _ => Wert::I32(match (breite, signed) {
                    (0, true) => v as u8 as i8 as i32,
                    (0, false) => v as i32,
                    (1, true) => v as u16 as i16 as i32,
                    (1, false) => v as i32,
                    _ => v as i32,
                }),
            }
        }
        Lade::I64 | Lade::F64 => {
            let mut v: u64 = 0;
            for (i, b) in roh.iter().enumerate() {
                v |= (*b as u64) << (8 * i);
            }
            match art {
                Lade::F64 => Wert::F64(v),
                _ => Wert::I64(match (breite, signed) {
                    (0, true) => v as u8 as i8 as i64,
                    (0, false) => v as i64,
                    (1, true) => v as u16 as i16 as i64,
                    (1, false) => v as i64,
                    (2, true) => v as u32 as i32 as i64,
                    _ => v as i64,
                }),
            }
        }
    };
    m.stapel.push(wert);
    Ok(None)
}

/// Speicher-Befehl.
fn speichere(
    m: &mut Maschine,
    lauf: &mut Lauf,
    speicher: Option<u32>,
    versatz: u32,
    art: Lade,
    breite: u32,
) -> Result<Option<usize>, Fehler> {
    let wert = pop(m)?;
    let adresse = pop(m)?.als_i32().unwrap_or(0) as u32 as u64;
    let adresse = adresse.wrapping_add(versatz as u64);
    let bytes = 1usize << breite;
    let mut roh = [0u8; 8];
    match art {
        // Achtung: fuer f32/f64 das BITMUSTER nehmen, nicht `als_i32()`
        // (liefert fuer Flieskommawerte `None` — GEMESSEN 01.10.:
        // `float_memory.wast`: jeder `f32.store` schrieb Nullen).
        Lade::I32 | Lade::F32 => roh[..4].copy_from_slice(&wert.als_bits32().to_le_bytes()),
        Lade::I64 | Lade::F64 => roh.copy_from_slice(&wert.als_bits64().to_le_bytes()),
    }
    speicher_mut(lauf, speicher)?.schreib(adresse, &roh[..bytes])?;
    Ok(None)
}

// ---------------------------------------------------------------------------
// Arithmetik und Konvertierungen
// ---------------------------------------------------------------------------
fn rechnen(m: &mut Maschine, b: Befehl) -> Result<Option<usize>, Fehler> {
    use Befehl::*;
    macro_rules! i2u {
        ($f:expr) => {{
            let b = pop(m)?.als_i32().unwrap_or(0) as u32;
            let a = pop(m)?.als_i32().unwrap_or(0) as u32;
            #[allow(clippy::redundant_closure_call)]
            let w: i32 = $f(a, b);
            m.stapel.push(Wert::I32(w));
            Ok(None)
        }};
    }
    macro_rules! i2s {
        ($f:expr) => {{
            let b = pop(m)?.als_i32().unwrap_or(0);
            let a = pop(m)?.als_i32().unwrap_or(0);
            #[allow(clippy::redundant_closure_call)]
            let w: i32 = $f(a, b);
            m.stapel.push(Wert::I32(w));
            Ok(None)
        }};
    }
    macro_rules! u2i {
        ($f:expr) => {{
            let b = pop(m)?.als_i64().unwrap_or(0) as u64;
            let a = pop(m)?.als_i64().unwrap_or(0) as u64;
            #[allow(clippy::redundant_closure_call)]
            let w: i32 = $f(a, b);
            m.stapel.push(Wert::I32(w));
            Ok(None)
        }};
    }
    macro_rules! i2u64 {
        ($f:expr) => {{
            let b = pop(m)?.als_i64().unwrap_or(0) as u64;
            let a = pop(m)?.als_i64().unwrap_or(0) as u64;
            #[allow(clippy::redundant_closure_call)]
            let w: i64 = $f(a, b);
            m.stapel.push(Wert::I64(w));
            Ok(None)
        }};
    }
    macro_rules! u1 {
        ($f:expr) => {{
            let a = pop(m)?.als_i32().unwrap_or(0);
            #[allow(clippy::redundant_closure_call)]
            let w: i32 = $f(a);
            m.stapel.push(Wert::I32(w));
            Ok(None)
        }};
    }
    macro_rules! l1 {
        ($f:expr) => {{
            let a = pop(m)?.als_i64().unwrap_or(0);
            #[allow(clippy::redundant_closure_call)]
            let w: i64 = $f(a);
            m.stapel.push(Wert::I64(w));
            Ok(None)
        }};
    }
    /// Unary, das ein i32-Ergebnis liefert (`i64.eqz`).
    macro_rules! l1i {
        ($f:expr) => {{
            let a = pop(m)?.als_i64().unwrap_or(0);
            #[allow(clippy::redundant_closure_call)]
            let w: i32 = $f(a);
            m.stapel.push(Wert::I32(w));
            Ok(None)
        }};
    }
    macro_rules! f32c {
        ($f:expr) => {{
            let b = pop(m)?.als_f32().unwrap_or(0.0);
            let a = pop(m)?.als_f32().unwrap_or(0.0);
            #[allow(clippy::redundant_closure_call)]
            let w: bool = $f(a, b);
            m.stapel.push(Wert::I32(i32::from(w)));
            Ok(None)
        }};
    }
    macro_rules! f64c {
        ($f:expr) => {{
            let b = pop(m)?.als_f64().unwrap_or(0.0);
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            #[allow(clippy::redundant_closure_call)]
            let w: bool = $f(a, b);
            m.stapel.push(Wert::I32(i32::from(w)));
            Ok(None)
        }};
    }
    match b {
        I32Eqz => u1!(|a: i32| i32::from(a == 0)),
        I32Eq => i2u!(|a: u32, b: u32| i32::from(a == b)),
        I32Ne => i2u!(|a: u32, b: u32| i32::from(a != b)),
        I32LtS => i2s!(|a: i32, b: i32| i32::from(a < b)),
        I32LtU => i2u!(|a: u32, b: u32| i32::from(a < b)),
        I32GtS => i2s!(|a: i32, b: i32| i32::from(a > b)),
        I32GtU => i2u!(|a: u32, b: u32| i32::from(a > b)),
        I32LeS => i2s!(|a: i32, b: i32| i32::from(a <= b)),
        I32LeU => i2u!(|a: u32, b: u32| i32::from(a <= b)),
        I32GeS => i2s!(|a: i32, b: i32| i32::from(a >= b)),
        I32GeU => i2u!(|a: u32, b: u32| i32::from(a >= b)),
        I32Clz => u1!(|a: i32| a.leading_zeros() as i32),
        I32Ctz => u1!(|a: i32| a.trailing_zeros() as i32),
        I32Popcnt => u1!(|a: i32| a.count_ones() as i32),
        I32Add => i2u!(|a: u32, b: u32| a.wrapping_add(b) as i32),
        I32Sub => i2u!(|a: u32, b: u32| a.wrapping_sub(b) as i32),
        I32Mul => i2u!(|a: u32, b: u32| a.wrapping_mul(b) as i32),
        I32DivS => div_i32(m, true),
        I32DivU => div_i32(m, false),
        I32RemS => rem_i32(m, true),
        I32RemU => rem_i32(m, false),
        I32And => i2u!(|a: u32, b: u32| (a & b) as i32),
        I32Or => i2u!(|a: u32, b: u32| (a | b) as i32),
        I32Xor => i2u!(|a: u32, b: u32| (a ^ b) as i32),
        I32Shl => i2u!(|a: u32, b: u32| a.wrapping_shl(b & 31) as i32),
        I32ShrS => {
            let b = (pop(m)?.als_i32().unwrap_or(0) as u32) & 31;
            let a = pop(m)?.als_i32().unwrap_or(0);
            m.stapel.push(Wert::I32(a.wrapping_shr(b)));
            Ok(None)
        }
        I32ShrU => i2u!(|a: u32, b: u32| (a.wrapping_shr(b & 31)) as i32),
        I32Rotl => i2u!(|a: u32, b: u32| a.rotate_left(b & 31) as i32),
        I32Rotr => i2u!(|a: u32, b: u32| a.rotate_right(b & 31) as i32),
        I64Eqz => l1i!(|a: i64| i32::from(a == 0)),
        I64Eq => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a == b)));
            Ok(None)
        }
        I64Ne => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a != b)));
            Ok(None)
        }
        I64LtS => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a < b)));
            Ok(None)
        }
        I64LtU => u2i!(|a: u64, b: u64| i32::from(a < b)),
        I64GtS => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a > b)));
            Ok(None)
        }
        I64GtU => u2i!(|a: u64, b: u64| i32::from(a > b)),
        I64LeS => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a <= b)));
            Ok(None)
        }
        I64LeU => u2i!(|a: u64, b: u64| i32::from(a <= b)),
        I64GeS => {
            let b = pop(m)?.als_i64().unwrap_or(0);
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I32(i32::from(a >= b)));
            Ok(None)
        }
        I64GeU => u2i!(|a: u64, b: u64| i32::from(a >= b)),
        I64Clz => l1!(|a: i64| a.leading_zeros() as i64),
        I64Ctz => l1!(|a: i64| a.trailing_zeros() as i64),
        I64Popcnt => l1!(|a: i64| a.count_ones() as i64),
        I64Add => i2u64!(|a: u64, b: u64| a.wrapping_add(b) as i64),
        I64Sub => i2u64!(|a: u64, b: u64| a.wrapping_sub(b) as i64),
        I64Mul => i2u64!(|a: u64, b: u64| a.wrapping_mul(b) as i64),
        I64DivS => div_i64(m, true),
        I64DivU => div_i64(m, false),
        I64RemS => rem_i64(m, true),
        I64RemU => rem_i64(m, false),
        I64And => i2u64!(|a: u64, b: u64| (a & b) as i64),
        I64Or => i2u64!(|a: u64, b: u64| (a | b) as i64),
        I64Xor => i2u64!(|a: u64, b: u64| (a ^ b) as i64),
        I64Shl => i2u64!(|a: u64, b: u64| a.wrapping_shl((b & 63) as u32) as i64),
        I64ShrS => {
            let b = ((pop(m)?.als_i64().unwrap_or(0) as u64) & 63) as u32;
            let a = pop(m)?.als_i64().unwrap_or(0);
            m.stapel.push(Wert::I64(a.wrapping_shr(b)));
            Ok(None)
        }
        I64ShrU => i2u64!(|a: u64, b: u64| (a.wrapping_shr((b & 63) as u32)) as i64),
        I64Rotl => i2u64!(|a: u64, b: u64| a.rotate_left((b & 63) as u32) as i64),
        I64Rotr => i2u64!(|a: u64, b: u64| a.rotate_right((b & 63) as u32) as i64),
        F32Eq => f32c!(|a: f32, b: f32| a == b),
        F32Ne => f32c!(|a: f32, b: f32| a != b),
        F32Lt => f32c!(|a: f32, b: f32| a < b),
        F32Gt => f32c!(|a: f32, b: f32| a > b),
        F32Le => f32c!(|a: f32, b: f32| a <= b),
        F32Ge => f32c!(|a: f32, b: f32| a >= b),
        F32Abs => {
            let a = pop(m)?.als_f32().unwrap_or(0.0).to_bits() & 0x7fff_ffff;
            m.stapel.push(Wert::F32(a));
            Ok(None)
        }
        F32Neg => {
            let a = pop(m)?.als_f32().unwrap_or(0.0).to_bits() ^ 0x8000_0000;
            m.stapel.push(Wert::F32(a));
            Ok(None)
        }
        F32Ceil => f32_un(m, |a: f32| a.ceil()),
        F32Floor => f32_un(m, |a: f32| a.floor()),
        F32Trunc => f32_un(m, |a: f32| a.trunc()),
        F32Nearest => f32_un(m, |a: f32| nearest32(a)),
        F32Sqrt => f32_un(m, |a: f32| a.sqrt()),
        F32Add => f32_bi(m, |a: f32, b: f32| a + b),
        F32Sub => f32_bi(m, |a: f32, b: f32| a - b),
        F32Mul => f32_bi(m, |a: f32, b: f32| a * b),
        F32Div => f32_bi(m, |a: f32, b: f32| a / b),
        F32Min => f32_bi(m, wasm_min32),
        F32Max => f32_bi(m, wasm_max32),
        F32Copysign => {
            let b = pop(m)?.als_f32().unwrap_or(0.0).to_bits();
            let a = pop(m)?.als_f32().unwrap_or(0.0).to_bits();
            m.stapel
                .push(Wert::F32((a & 0x7fff_ffff) | (b & 0x8000_0000)));
            Ok(None)
        }
        F64Eq => f64c!(|a: f64, b: f64| a == b),
        F64Ne => f64c!(|a: f64, b: f64| a != b),
        F64Lt => f64c!(|a: f64, b: f64| a < b),
        F64Gt => f64c!(|a: f64, b: f64| a > b),
        F64Le => f64c!(|a: f64, b: f64| a <= b),
        F64Ge => f64c!(|a: f64, b: f64| a >= b),
        F64Abs => {
            let a = pop(m)?.als_f64().unwrap_or(0.0).to_bits() & 0x7fff_ffff_ffff_ffff;
            m.stapel.push(Wert::F64(a));
            Ok(None)
        }
        F64Neg => {
            let a = pop(m)?.als_f64().unwrap_or(0.0).to_bits() ^ 0x8000_0000_0000_0000;
            m.stapel.push(Wert::F64(a));
            Ok(None)
        }
        F64Ceil => f64_un(m, |a: f64| a.ceil()),
        F64Floor => f64_un(m, |a: f64| a.floor()),
        F64Trunc => f64_un(m, |a: f64| a.trunc()),
        F64Nearest => f64_un(m, |a: f64| nearest(a)),
        F64Sqrt => f64_un(m, |a: f64| a.sqrt()),
        F64Add => f64_bi(m, |a: f64, b: f64| a + b),
        F64Sub => f64_bi(m, |a: f64, b: f64| a - b),
        F64Mul => f64_bi(m, |a: f64, b: f64| a * b),
        F64Div => f64_bi(m, |a: f64, b: f64| a / b),
        F64Min => f64_bi(m, wasm_min64),
        F64Max => f64_bi(m, wasm_max64),
        F64Copysign => {
            let b = pop(m)?.als_f64().unwrap_or(0.0).to_bits();
            let a = pop(m)?.als_f64().unwrap_or(0.0).to_bits();
            m.stapel.push(Wert::F64(
                (a & 0x7fff_ffff_ffff_ffff) | (b & 0x8000_0000_0000_0000),
            ));
            Ok(None)
        }
        I32WrapI64 => {
            let a = pop(m)?.als_i64().unwrap_or(0) as i32;
            m.stapel.push(Wert::I32(a));
            Ok(None)
        }
        I32TruncF32S => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I32(trunk(a, 32, true)? as i32));
            Ok(None)
        }
        I32TruncF32U => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I32(trunk(a, 32, false)? as u32 as i32));
            Ok(None)
        }
        I32TruncF64S => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I32(trunk(a, 32, true)? as i32));
            Ok(None)
        }
        I32TruncF64U => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I32(trunk(a, 32, false)? as u32 as i32));
            Ok(None)
        }
        I64ExtendI32S => {
            let a = pop(m)?.als_i32().unwrap_or(0) as i64;
            m.stapel.push(Wert::I64(a));
            Ok(None)
        }
        I64ExtendI32U => {
            let a = pop(m)?.als_i32().unwrap_or(0) as u32 as i64;
            m.stapel.push(Wert::I64(a));
            Ok(None)
        }
        I64TruncF32S => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I64(trunk(a, 64, true)?));
            Ok(None)
        }
        I64TruncF32U => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I64(trunk(a, 64, false)?));
            Ok(None)
        }
        I64TruncF64S => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I64(trunk(a, 64, true)?));
            Ok(None)
        }
        I64TruncF64U => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I64(trunk(a, 64, false)?));
            Ok(None)
        }
        F32ConvertI32S => {
            let a = pop(m)?.als_i32().unwrap_or(0) as f32;
            m.stapel.push(Wert::F32(a.to_bits()));
            Ok(None)
        }
        F32ConvertI32U => {
            let a = pop(m)?.als_i32().unwrap_or(0) as u32 as f32;
            m.stapel.push(Wert::F32(a.to_bits()));
            Ok(None)
        }
        F32ConvertI64S => {
            let a = pop(m)?.als_i64().unwrap_or(0) as f32;
            m.stapel.push(Wert::F32(a.to_bits()));
            Ok(None)
        }
        F32ConvertI64U => {
            let a = pop(m)?.als_i64().unwrap_or(0) as u64 as f32;
            m.stapel.push(Wert::F32(a.to_bits()));
            Ok(None)
        }
        F32DemoteF64 => {
            let a = pop(m)?.als_f64().unwrap_or(0.0) as f32;
            let bits = if ist_nan32(a.to_bits()) {
                nan32()
            } else {
                a.to_bits()
            };
            m.stapel.push(Wert::F32(bits));
            Ok(None)
        }
        F64ConvertI32S => {
            let a = pop(m)?.als_i32().unwrap_or(0) as f64;
            m.stapel.push(Wert::F64(a.to_bits()));
            Ok(None)
        }
        F64ConvertI32U => {
            let a = pop(m)?.als_i32().unwrap_or(0) as u32 as f64;
            m.stapel.push(Wert::F64(a.to_bits()));
            Ok(None)
        }
        F64ConvertI64S => {
            let a = pop(m)?.als_i64().unwrap_or(0) as f64;
            m.stapel.push(Wert::F64(a.to_bits()));
            Ok(None)
        }
        F64ConvertI64U => {
            let a = pop(m)?.als_i64().unwrap_or(0) as u64 as f64;
            m.stapel.push(Wert::F64(a.to_bits()));
            Ok(None)
        }
        F64PromoteF32 => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            let bits = if ist_nan64(a.to_bits()) {
                nan64()
            } else {
                a.to_bits()
            };
            m.stapel.push(Wert::F64(bits));
            Ok(None)
        }
        I32ReinterpretF32 => {
            let a = pop(m)?.als_f32().unwrap_or(0.0).to_bits();
            m.stapel.push(Wert::I32(a as i32));
            Ok(None)
        }
        I64ReinterpretF64 => {
            let a = pop(m)?.als_f64().unwrap_or(0.0).to_bits();
            m.stapel.push(Wert::I64(a as i64));
            Ok(None)
        }
        F32ReinterpretI32 => {
            let a = pop(m)?.als_i32().unwrap_or(0) as u32;
            m.stapel.push(Wert::F32(a));
            Ok(None)
        }
        F64ReinterpretI64 => {
            let a = pop(m)?.als_i64().unwrap_or(0) as u64;
            m.stapel.push(Wert::F64(a));
            Ok(None)
        }
        I32Extend8S => u1!(|a: i32| a as i8 as i32),
        I32Extend16S => u1!(|a: i32| a as i16 as i32),
        I64Extend8S => l1!(|a: i64| a as i8 as i64),
        I64Extend16S => l1!(|a: i64| a as i16 as i64),
        I64Extend32S => l1!(|a: i64| a as i32 as i64),
        I32TruncSatF32S => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I32(trunk_sat(a, 32, true) as i32));
            Ok(None)
        }
        I32TruncSatF32U => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel
                .push(Wert::I32(trunk_sat(a, 32, false) as u32 as i32));
            Ok(None)
        }
        I32TruncSatF64S => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I32(trunk_sat(a, 32, true) as i32));
            Ok(None)
        }
        I32TruncSatF64U => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel
                .push(Wert::I32(trunk_sat(a, 32, false) as u32 as i32));
            Ok(None)
        }
        I64TruncSatF32S => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I64(trunk_sat(a, 64, true)));
            Ok(None)
        }
        I64TruncSatF32U => {
            let a = pop(m)?.als_f32().unwrap_or(0.0) as f64;
            m.stapel.push(Wert::I64(trunk_sat(a, 64, false)));
            Ok(None)
        }
        I64TruncSatF64S => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I64(trunk_sat(a, 64, true)));
            Ok(None)
        }
        I64TruncSatF64U => {
            let a = pop(m)?.als_f64().unwrap_or(0.0);
            m.stapel.push(Wert::I64(trunk_sat(a, 64, false)));
            Ok(None)
        }
        andere => Err(Fehler::ungueltig(
            V_TYP_ERGEBNIS,
            format!("Befehl {andere:?} im Interpreter nicht behandelt"),
        )),
    }
}

fn div_i32(m: &mut Maschine, signed: bool) -> Result<Option<usize>, Fehler> {
    let bv = pop(m)?.als_i32().unwrap_or(0);
    let av = pop(m)?.als_i32().unwrap_or(0);
    if bv == 0 {
        return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
    }
    if signed {
        if av == i32::MIN && bv == -1 {
            return Err(Fehler::trap(T_OVERFLOW, "integer overflow (i32.div_s)"));
        }
        m.stapel.push(Wert::I32(av.wrapping_div(bv)));
    } else {
        m.stapel.push(Wert::I32(((av as u32) / (bv as u32)) as i32));
    }
    Ok(None)
}

fn rem_i32(m: &mut Maschine, signed: bool) -> Result<Option<usize>, Fehler> {
    let bv = pop(m)?.als_i32().unwrap_or(0);
    let av = pop(m)?.als_i32().unwrap_or(0);
    if bv == 0 {
        return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
    }
    if signed {
        m.stapel.push(Wert::I32(av.wrapping_rem(bv)));
    } else {
        m.stapel.push(Wert::I32(((av as u32) % (bv as u32)) as i32));
    }
    Ok(None)
}

fn div_i64(m: &mut Maschine, signed: bool) -> Result<Option<usize>, Fehler> {
    let bv = pop(m)?.als_i64().unwrap_or(0);
    let av = pop(m)?.als_i64().unwrap_or(0);
    if bv == 0 {
        return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
    }
    if signed {
        if av == i64::MIN && bv == -1 {
            return Err(Fehler::trap(T_OVERFLOW, "integer overflow (i64.div_s)"));
        }
        m.stapel.push(Wert::I64(av.wrapping_div(bv)));
    } else {
        m.stapel.push(Wert::I64(((av as u64) / (bv as u64)) as i64));
    }
    Ok(None)
}

fn rem_i64(m: &mut Maschine, signed: bool) -> Result<Option<usize>, Fehler> {
    let bv = pop(m)?.als_i64().unwrap_or(0);
    let av = pop(m)?.als_i64().unwrap_or(0);
    if bv == 0 {
        return Err(Fehler::trap(T_DIVISION, "integer divide by zero"));
    }
    if signed {
        m.stapel.push(Wert::I64(av.wrapping_rem(bv)));
    } else {
        m.stapel.push(Wert::I64(((av as u64) % (bv as u64)) as i64));
    }
    Ok(None)
}

fn f32_un<F: FnOnce(f32) -> f32>(m: &mut Maschine, f: F) -> Result<Option<usize>, Fehler> {
    let a = pop(m)?.als_f32().unwrap_or(0.0);
    let e = f(a);
    let bits = if e.is_nan() { nan32() } else { e.to_bits() };
    m.stapel.push(Wert::F32(bits));
    Ok(None)
}

fn f32_bi<F: FnOnce(f32, f32) -> f32>(m: &mut Maschine, f: F) -> Result<Option<usize>, Fehler> {
    let b = pop(m)?.als_f32().unwrap_or(0.0);
    let a = pop(m)?.als_f32().unwrap_or(0.0);
    let e = f(a, b);
    let bits = if e.is_nan() { nan32() } else { e.to_bits() };
    m.stapel.push(Wert::F32(bits));
    Ok(None)
}

fn f64_un<F: FnOnce(f64) -> f64>(m: &mut Maschine, f: F) -> Result<Option<usize>, Fehler> {
    let a = pop(m)?.als_f64().unwrap_or(0.0);
    let e = f(a);
    let bits = if e.is_nan() { nan64() } else { e.to_bits() };
    m.stapel.push(Wert::F64(bits));
    Ok(None)
}

fn f64_bi<F: FnOnce(f64, f64) -> f64>(m: &mut Maschine, f: F) -> Result<Option<usize>, Fehler> {
    let b = pop(m)?.als_f64().unwrap_or(0.0);
    let a = pop(m)?.als_f64().unwrap_or(0.0);
    let e = f(a, b);
    let bits = if e.is_nan() { nan64() } else { e.to_bits() };
    m.stapel.push(Wert::F64(bits));
    Ok(None)
}

/// Position von Local `i` IM gemeinsamen Stapel `[lokal_basis .. basis)`.
///
/// `Ok(pos)` = die Position, `Err(anzahl)` = Index ausserhalb (die
/// Anzahl fuer die Fehlermeldung).
///
/// GEMESSEN 04.10. (Hebel 1): vorher lasen `local.get/set/tee` einen
/// eigenen `Vec<Wert>` je Rahmen.
fn lokal_position(m: &Maschine, ri: usize, i: u32) -> Result<usize, usize> {
    let r = &m.rahmen[ri];
    let anzahl = r.basis.saturating_sub(r.lokal_basis);
    let pos = r.lokal_basis + i as usize;
    if (i as usize) >= anzahl {
        Err(anzahl)
    } else {
        Ok(pos)
    }
}

/// `Option<Ref>` -> Operandstapelwert (der Typ ergibt sich aus dem Ref).
/// Ein Tabelleneintrag als Wert. `leer` (unbeschriebener Platz) ergibt den
/// `null`-Wert des ELEMENTTYPS der Tabelle — bei einer `externref`-Tabelle
/// also `externref`-null.
///
/// GEMESSEN 03.10. (`table_fill.wast` Z. 16ff: „null erwartet, null" —
/// `table.get` lieferte `funcref`-null in einer `externref`-Tabelle).
pub(crate) fn ref_wert_typ(r: Option<Ref>, reftyp: &Reftyp) -> Wert {
    match r {
        Some(Ref::Func(f)) => Wert::Func(Some(f)),
        Some(Ref::Extern(e)) => Wert::Extern(Some(e)),
        Some(Ref::Gcref(g)) => Wert::Gcref(Some(g)),
        None => Wert::standard(Valtyp::Ref(*reftyp)),
    }
}

/// Wasm `f32.min`/`f64.min`: NaN, wenn ein Operand NaN ist; -0 < +0.
pub fn wasm_min32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.to_bits() | b.to_bits() == 0x8000_0000 {
            -0.0
        } else {
            0.0
        };
    }
    if a < b { a } else { b }
}
pub fn wasm_max32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.to_bits() & b.to_bits() == 0x8000_0000 {
            -0.0
        } else {
            0.0
        };
    }
    if a > b { a } else { b }
}
pub fn wasm_min64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.to_bits() | b.to_bits() == 0x8000_0000_0000_0000 {
            -0.0
        } else {
            0.0
        };
    }
    if a < b { a } else { b }
}
pub fn wasm_max64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.to_bits() & b.to_bits() == 0x8000_0000_0000_0000 {
            -0.0
        } else {
            0.0
        };
    }
    if a > b { a } else { b }
}

/// Sind zwei Funktionssignaturen gleich?
pub fn typ_gleich(a: &FuncTyp, b: &FuncTyp) -> bool {
    a.parameter == b.parameter && a.ergebnis == b.ergebnis
}

/// Kurztext einer Signatur (fuer Trap-Meldungen).
pub(crate) fn zeile(t: &FuncTyp) -> String {
    let p: Vec<&str> = t.parameter.iter().map(|v| v.text()).collect();
    let e: Vec<&str> = t.ergebnis.iter().map(|v| v.text()).collect();
    format!("({}) -> ({})", p.join(","), e.join(","))
}
