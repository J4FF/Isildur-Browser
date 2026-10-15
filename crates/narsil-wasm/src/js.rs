//! W4: Anbindung an JavaScript. Hier steht die Schnittstelle, die
//! `narsil-jsvm` (Rolle JS) aufsetzt — Auftrag: „Wunsch an JS ueber
//! team/WUNSCH_JS.txt, Schnittstelle zuerst festlegen".
//!
//! Die Schnittstelle ist bewusst schmal und frei von Fremd-Crates:
//!
//! ```text
//! WebAssembly.validate(bytes)                  -> bool
//! WebAssembly.compile(bytes)                   -> Module
//! WebAssembly.instantiate(bytes|module, imp)   -> Instance
//! WebAssembly.Module      .exports/.imports
//! WebAssembly.Instance    .exports[name]
//! WebAssembly.Memory      new M({initial,maximum}) / .buffer / .grow(n)
//! WebAssembly.Table       new T({element,initial,maximum}) / .get/.set/.length/.grow
//! WebAssembly.Global      new G({value, mutable}) / .value
//! WebAssembly.CompileError / LinkError / RuntimeError
//! ```
//!
//! Der Trick: JS-Objekte (`Memory`, `Table`, `Global`, `Function`) sind KEINE
//! eigenen Typen hier, sondern **Indizes in einem `Lauf`**. Damit ist
//! Objektidentitaet ueber die Sprachgrenze hinweg garantiert: WASM schreibt
//! in `Memory`, die JS-Seite liest denselben Index. (Die erste Fassung
//! dieser Datei wollte `Rc<Speicher>` in beide Welten geben — das
//! scheitert an `&mut` fuer `memory.grow`, siehe `lauf.rs`.)

use crate::fehler::*;
use crate::grenzen::Grenzen;
use crate::lauf::{FremdFn, Lauf, instanziieren};
use crate::modell::{FuncTyp, Modul, Reftyp, Valtyp};
use crate::speicher::Speicher;
use crate::tabelle::Tabelle;
use crate::wert::{Ref, Wert};

/// Ein Importwert: Index im Store (siehe Modulkopf).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportWert {
    Funktion(u32),
    Tabelle(u32),
    Speicher(u32),
    Global(u32),
    /// Tag (Exception Handling): Index in `Store::tags`.
    Tag(u32),
}

impl ImportWert {
    pub fn text(&self) -> &'static str {
        match self {
            ImportWert::Funktion(_) => "Funktion",
            ImportWert::Tabelle(_) => "Tabelle",
            ImportWert::Speicher(_) => "Speicher",
            ImportWert::Global(_) => "globale Variable",
            ImportWert::Tag(_) => "Tag",
        }
    }
}

/// Importquelle: die JS-Seite beantwortet `modulfeld -> Index`.
pub trait ImportQuelle {
    fn hole(&self, lauf: &Lauf, modul: &str, feld: &str) -> Option<ImportWert>;
}

/// Importquelle aus einer Tabelle (fuer Tests und fuer die
/// `WebAssembly.instantiate(importObject)`-Uebersetzung).
#[derive(Debug, Default, Clone)]
pub struct ImportTabelle {
    pub eintraege: Vec<((String, String), ImportWert)>,
}

impl ImportQuelle for ImportTabelle {
    fn hole(&self, _lauf: &Lauf, modul: &str, feld: &str) -> Option<ImportWert> {
        self.eintraege
            .iter()
            .find(|((m, f), _)| m == modul && f == feld)
            .map(|(_, w)| *w)
    }
}

impl ImportTabelle {
    pub fn neu() -> ImportTabelle {
        ImportTabelle::default()
    }
    pub fn mit(mut self, modul: &str, feld: &str, w: ImportWert) -> ImportTabelle {
        self.eintraege
            .push(((modul.to_string(), feld.to_string()), w));
        self
    }
}

/// Was die JS-Seite als „Import-Objekt" anbietet (rein datentragend).
#[derive(Clone)]
pub enum Angebot {
    /// Eine Rust-Funktion (die JS-Seite baut sie aus JS-Code).
    Funktion(FuncTyp, FremdFn),
    /// Ein Speicher/Objekt, das schon im Store liegt.
    Wert(ImportWert),
}

/// `WebAssembly.validate`: Bytes -> ja/nein (Fehlertext im Rueckgabe-Tupel).
pub fn validate(bytes: &[u8], g: &Grenzen) -> Result<(), Fehler> {
    crate::modul_pruefen(bytes, g).map(|_| ())
}

/// `WebAssembly.compile`: Bytes -> Modul (fehlerfrei geprueft).
pub fn compile(bytes: &[u8], g: &Grenzen) -> Result<Modul, Fehler> {
    crate::modul_pruefen(bytes, g)
}

/// `WebAssembly.Module.exports`: Liste `(Name, Art)` fuer die Anzeige.
pub fn module_exports(m: &Modul) -> Vec<(String, &'static str)> {
    m.exporte
        .iter()
        .map(|e| (e.name.clone(), e.art.text()))
        .collect()
}

/// `WebAssembly.Module.imports`: Liste `(Modul, Feld, Art)`.
pub fn module_imports(m: &Modul) -> Vec<(String, String, &'static str)> {
    let mut v = Vec::new();
    for f in &m.funktionen {
        if let crate::modell::Func::Importiert { modul, feld, .. } = f {
            v.push((modul.clone(), feld.clone(), "func"));
        }
    }
    for t in &m.tabellen {
        if let crate::modell::ImportiertTab::Importiert { modul, feld, .. } = t {
            v.push((modul.clone(), feld.clone(), "table"));
        }
    }
    for s in &m.speicher {
        if let crate::modell::ImportiertSpeicher::Importiert { modul, feld, .. } = s {
            v.push((modul.clone(), feld.clone(), "memory"));
        }
    }
    for g in &m.globale {
        if let Some((modul, feld)) = &g.import {
            v.push((modul.clone(), feld.clone(), "global"));
        }
    }
    v
}

/// `WebAssembly.instantiate`: Modul + Importe -> Instanz.
pub fn instantiate(
    lauf: &mut Lauf,
    m: &Modul,
    quelle: &dyn ImportQuelle,
) -> Result<crate::lauf::Instanz, Fehler> {
    instanziieren(lauf, m, quelle)
}

/// `WebAssembly.Memory`: Speicherobjekt anlegen und den Index liefern.
pub fn neue_memory(lauf: &mut Lauf, initial: u64, maximum: Option<u64>) -> Result<u32, Fehler> {
    lauf.neuer_speicher(initial, maximum)
}

/// `Memory.buffer`: Bytes des Speichers (unveraendliche Sicht fuer die
/// JS-Seite; Schreibzugriffe aus JS laufen ueber `memory_set`).
pub fn memory_bytes(lauf: &Lauf, idx: u32) -> Result<u64, Fehler> {
    lauf.store
        .speicher
        .get(idx as usize)
        .map(|s: &Speicher| s.byte_len() as u64)
        .ok_or_else(|| Fehler::link(L_SPEICHER, format!("Speicher {idx} existiert nicht")))
}

/// `Memory.buffer`: die Bytes als Slice (nur lesend — Schreibzugriffe aus JS
/// gehen ueber `memory_set`, damit die Grenzen an einer Stelle bleiben).
pub fn memory_view(lauf: &Lauf, idx: u32) -> Result<&[u8], Fehler> {
    lauf.store
        .speicher
        .get(idx as usize)
        .map(|s: &Speicher| s.daten_view())
        .ok_or_else(|| Fehler::link(L_SPEICHER, format!("Speicher {idx} existiert nicht")))
}

/// `memory_set`: Bytes aus JS in den Speicher schreiben (fuer
/// `WebAssembly.Memory` in JS, wenn die JS-Seite den Puffer fuellt).
pub fn memory_set(lauf: &mut Lauf, idx: u32, adresse: u64, daten: &[u8]) -> Result<(), Fehler> {
    lauf.store
        .speicher
        .get_mut(idx as usize)
        .ok_or_else(|| Fehler::link(L_SPEICHER, format!("Speicher {idx} existiert nicht")))?
        .schreib(adresse, daten)
}

/// `Memory.grow`.
pub fn memory_grow(lauf: &mut Lauf, idx: u32, seiten: u64) -> i32 {
    match lauf.store.speicher.get_mut(idx as usize) {
        Some(s) => s.waechst(seiten),
        None => -1,
    }
}

/// `WebAssembly.Table`: neues Tabellenobjekt.
pub fn neue_table(
    lauf: &mut Lauf,
    element: Reftyp,
    initial: u64,
    maximum: Option<u64>,
) -> Result<u32, Fehler> {
    lauf.neue_tabelle(initial, maximum, element)
}

/// `Table.get`: `Option<Wert>` (`None` = null).
pub fn table_get(lauf: &Lauf, idx: u32, i: u64) -> Result<Option<Wert>, Fehler> {
    let t: &Tabelle = lauf
        .store
        .tabellen
        .get(idx as usize)
        .ok_or_else(|| Fehler::link(L_TABELLE, format!("Tabelle {idx} existiert nicht")))?;
    match t.lies(i) {
        Ok(Some(Ref::Func(f))) => Ok(Some(Wert::Func(Some(f)))),
        Ok(Some(Ref::Extern(e))) => Ok(Some(Wert::Extern(Some(e)))),
        // GC-Referenzen (Arrays) kommen aus der Tabelle heraus (W4 gibt
        // sie nicht nach aussen) — als Wert weitergereicht.
        //
        // GEMESSEN 03.10. (`table_init.wast` Z. 790).
        Ok(Some(Ref::Gcref(g))) => Ok(Some(Wert::Gcref(Some(g)))),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `Table.set`.
pub fn table_set(lauf: &mut Lauf, idx: u32, i: u64, wert: Option<Wert>) -> Result<(), Fehler> {
    let t = lauf
        .store
        .tabellen
        .get_mut(idx as usize)
        .ok_or_else(|| Fehler::link(L_TABELLE, format!("Tabelle {idx} existiert nicht")))?;
    t.schreib(i, wert.and_then(|w| w.als_ref()))
}

/// `Table.length` (`WebAssembly.Table.prototype.length`).
pub fn table_len(lauf: &Lauf, idx: u32) -> Result<u64, Fehler> {
    lauf.store
        .tabellen
        .get(idx as usize)
        .map(|t| t.len())
        .ok_or_else(|| Fehler::link(L_TABELLE, format!("Tabelle {idx} existiert nicht")))
}

/// `Table.grow`.
pub fn table_grow(lauf: &mut Lauf, idx: u32, delta: u64, wert: Option<Wert>) -> i32 {
    match lauf.store.tabellen.get_mut(idx as usize) {
        Some(t) => t.waechst(delta, wert.and_then(|w| w.als_ref())),
        None => -1,
    }
}

/// `WebAssembly.Global`: Objekt anlegen.
pub fn new_global(lauf: &mut Lauf, wert: Wert, veraenderbar: bool) -> Result<u32, Fehler> {
    lauf.neue_globale(wert, veraenderbar)
}

/// `Global.value` lesen/schreiben.
pub fn global_value(lauf: &Lauf, idx: u32) -> Result<Wert, Fehler> {
    lauf.store
        .globale
        .get(idx as usize)
        .copied()
        .ok_or_else(|| Fehler::link(L_GLOBAL, format!("globale Variable {idx} existiert nicht")))
}
pub fn set_global_value(lauf: &mut Lauf, idx: u32, w: Wert) -> Result<(), Fehler> {
    lauf.store
        .globale
        .get_mut(idx as usize)
        .map(|g| *g = w)
        .ok_or_else(|| Fehler::link(L_GLOBAL, format!("globale Variable {idx} existiert nicht")))
}

/// `WebAssembly.Function`: neues Funktionsobjekt aus einer Rust-Funktion.
/// Die JS-Seite uebergibt `Rc<dyn Fn(&mut Lauf, &[Wert]) -> ...>`, also
/// laeuft die JS-Funktion im Rust-Kontext — dasselbe Prinzip wie in
/// `narsil-jsvm` (`FremdFn`).
pub fn new_function(lauf: &mut Lauf, typ: FuncTyp, f: FremdFn) -> u32 {
    lauf.neue_funktion(typ, f)
}

/// Einen WASM-Export aufrufen (`instance.exports.f(...)`).
pub fn call_export(
    lauf: &mut Lauf,
    instanz: &crate::lauf::Instanz,
    name: &str,
    args: &[Wert],
) -> Result<Vec<Wert>, Fehler> {
    lauf.rufe_funktion(instanz, name, args)
}

/// Prueft, ob ein Wert zum Typ passt (fuer `WebAssembly.Global`).
pub fn passt_typ(w: Wert, t: Valtyp) -> bool {
    w.typ() == t
}

/// Hilfsfunktion: `WebAssembly.RuntimeError` bei Trap, `LinkError` bei Linkfehler,
/// `CompileError` bei Parse-/Validatorfehler. Gibt den JS-Fehlertext.
pub fn js_fehlerart(e: &Fehler) -> &'static str {
    match e {
        Fehler::Form(_, _) | Fehler::Ungueltig(_, _) => "CompileError",
        Fehler::Link(_, _) => "LinkError",
        Fehler::Trap(_, _) | Fehler::Brennstoff { .. } => "RuntimeError",
    }
}

#[cfg(test)]
mod einheit {
    use super::*;
    use crate::lauf::Lauf;

    /// M7: Ergebnisse und Fehlerarten sichern.
    #[test]
    fn speicher_und_tabelle_objekte() {
        let mut l = Lauf::neu(Grenzen::standard());
        let s = neue_memory(&mut l, 1, Some(2)).unwrap();
        memory_set(&mut l, s, 0, &[1, 2, 3]).unwrap();
        assert_eq!(memory_grow(&mut l, s, 1), 1);
        assert_eq!(memory_grow(&mut l, s, 5), -1);
        assert_eq!(memory_bytes(&l, s).unwrap(), 2 * 65_536);
        assert_eq!(memory_view(&l, s).unwrap().len(), 2 * 65_536);
        let t = neue_table(&mut l, Reftyp::Func, 3, Some(4)).unwrap();
        assert_eq!(table_len(&l, t).unwrap(), 3);
        table_set(&mut l, t, 1, Some(Wert::Func(Some(7)))).unwrap();
        assert_eq!(table_get(&l, t, 1).unwrap(), Some(Wert::Func(Some(7))));
        // Rueckgabe von grow = alte Laenge
        assert_eq!(table_grow(&mut l, t, 1, None), 3);
        assert_eq!(table_len(&l, t).unwrap(), 4);
        assert_eq!(table_grow(&mut l, t, 9, None), -1);
        let g = new_global(&mut l, Wert::I32(5), true).unwrap();
        assert_eq!(global_value(&l, g).unwrap(), Wert::I32(5));
        set_global_value(&mut l, g, Wert::I32(6)).unwrap();
        assert_eq!(global_value(&l, g).unwrap(), Wert::I32(6));
        assert!(passt_typ(Wert::I32(1), Valtyp::I32));
        assert!(!passt_typ(Wert::I32(1), Valtyp::I64));
    }

    #[test]
    fn import_tabelle_und_fehlerart() {
        let q = ImportTabelle::neu()
            .mit("env", "f", ImportWert::Funktion(3))
            .mit("env", "m", ImportWert::Speicher(1));
        let l = Lauf::neu(Grenzen::standard());
        assert_eq!(q.hole(&l, "env", "f"), Some(ImportWert::Funktion(3)));
        assert_eq!(q.hole(&l, "env", "x"), None);
        assert_eq!(ImportWert::Speicher(1).text(), "Speicher");
        assert_eq!(js_fehlerart(&Fehler::form(ART_MAGIE, "x")), "CompileError");
        assert_eq!(js_fehlerart(&Fehler::link(L_IMPORT, "x")), "LinkError");
        assert_eq!(
            js_fehlerart(&Fehler::trap(T_UNERREICHBAR, "x")),
            "RuntimeError"
        );
    }
}
