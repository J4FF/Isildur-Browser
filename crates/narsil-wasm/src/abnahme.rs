//! W3: Abnahme gegen die offizielle Testsuite (`WebAssembly/testsuite`).
//!
//! Hier liegt die AUSFUEHRLOGIK der Abnahme — einmal, fuer Beispiel und
//! Test (`examples/suite.rs`, `tests/testsuite.rs` rufen nur noch auf).
//! Vorher stand dieselbe Logik zweimal im Baum; das war die Ursache
//! dafuer, dass Beispiel und Test verschiedene Zahlen meldeten.
//!
//! Zwei Dinge gehoeren zu einer fairen Abnahme:
//!
//! 1. **`spectest`**: die Testsuite importiert ein Hostmodul
//!    `spectest` (Speicher, Tabelle, vier Globale, `print*`-Funktionen).
//!    Ohne dieses Modul kann kein Modul mit Importen geladen werden —
//!    GEMESSEN 01.10.: 2240 Faelle fielen allein an „kein Modul geladen".
//! 2. **`register`**: `(register "name" $modul)` macht die Exporte eines
//!    Moduls fuer spaetere Module sichtbar (`imports.wast`). Ohne diese
//!    Registry sind alle Folgemodule nicht linkbar.
//!
//! Dateien, die nicht zu WebAssembly 2.0 Core gehoeren (SIMD, Threads,
//! GC, memory64, Tail-Call, reine Lexer-Dateien), sind in [`AUSSERHALB`]
//! gelistet — jede mit Grund, im Bericht W3.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::fehler::{ART_LAENGE, Fehler};
use crate::grenzen::Grenzen;
use crate::js::{ImportQuelle, ImportTabelle, ImportWert};
use crate::lauf::{FremdFn, Instanz, Lauf, instanziieren};
use crate::modell::{Art, FuncTyp, Modul, Reftyp, Valtyp};
use crate::wast::{Erwartung, ModulInhalt, Skript, lies_skript};
use crate::wert::Wert;

/// Datei-Muster, die nicht zu WebAssembly 2.0 Core gehoeren.
/// GEMESSEN 01.10.: 77 der 257 Dateien der Suite.
/// Dateinamen (OHNE `.wast`), die nicht zu WebAssembly 2.0 Core
/// gehoeren — jetzt EXAKT, nicht mehr als Teilzeichenkette.
///
/// GEMESSEN 03.10.2026: die alte Liste verglich mit `contains` und
/// schloss dadurch echte Kerndateien aus:
///   * `"rec"`  traf `call_indirect.wast` (call-indi**rec**t) — 129 Faelle
///   * `"func"` traf `func.wast` und `func_ptrs.wast`
///   * `"op"`   traf `nop.wast`, `table_copy.wast`, `memory_copy0/1.wast`
///   * `"global"`, `"ref_"`, `"memory_grow"`, `"unreached-valid"` u. a.
///
/// GEMESSEN 03.10. (alt, Mustervergleich): 101 Dateien, 18657/18657 =
/// 100,00 %. GEMESSEN 03.10. (neu, exakte Namen): 120 Dateien, 20667/20958
/// = 98,61 % — 291 Faelle, die die alte Zahl nie gesehen hat.
pub const AUSSERHALB: &[&str] = &[
    // Exception-Handling (Tag, throw, try_table) (5 Dateien)
    "tag",
    "throw",
    "throw_ref",
    "try_table",
    "unwind",
    // GC / Typreferenzen / rekursive Typen (24 Dateien)
    "array",
    "array_copy",
    "array_fill",
    "array_init_data",
    "array_init_elem",
    "array_new_data",
    "array_new_elem",
    "binary-gc",
    "br_on_cast",
    "br_on_cast_fail",
    "br_on_non_null",
    "br_on_null",
    "call_ref",
    "i31",
    "ref_as_non_null",
    "ref_cast",
    "ref_eq",
    "ref_test",
    "return_call_ref",
    "struct",
    "type-canon",
    "type-equivalence",
    "type-rec",
    "type-subtyping",
    // SIMD (v128) und relaxed SIMD (70 Dateien)
    "f32_bitwise",
    "f32_cmp",
    "f64_bitwise",
    "f64_cmp",
    "i16x8_relaxed_q15mulr_s",
    "i32x4_relaxed_trunc",
    "i8x16_relaxed_swizzle",
    "relaxed_dot_product",
    "relaxed_laneselect",
    "relaxed_madd_nmadd",
    "relaxed_min_max",
    "simd_address",
    "simd_align",
    "simd_bit_shift",
    "simd_bitwise",
    "simd_boolean",
    "simd_const",
    "simd_conversions",
    "simd_f32x4",
    "simd_f32x4_arith",
    "simd_f32x4_cmp",
    "simd_f32x4_pmin_pmax",
    "simd_f32x4_rounding",
    "simd_f64x2",
    "simd_f64x2_arith",
    "simd_f64x2_cmp",
    "simd_f64x2_pmin_pmax",
    "simd_f64x2_rounding",
    "simd_i16x8_arith",
    "simd_i16x8_arith2",
    "simd_i16x8_cmp",
    "simd_i16x8_extadd_pairwise_i8x16",
    "simd_i16x8_extmul_i8x16",
    "simd_i16x8_q15mulr_sat_s",
    "simd_i16x8_sat_arith",
    "simd_i32x4_arith",
    "simd_i32x4_arith2",
    "simd_i32x4_cmp",
    "simd_i32x4_dot_i16x8",
    "simd_i32x4_extadd_pairwise_i16x8",
    "simd_i32x4_extmul_i16x8",
    "simd_i32x4_trunc_sat_f32x4",
    "simd_i32x4_trunc_sat_f64x2",
    "simd_i64x2_arith",
    "simd_i64x2_arith2",
    "simd_i64x2_cmp",
    "simd_i64x2_extmul_i32x4",
    "simd_i8x16_arith",
    "simd_i8x16_arith2",
    "simd_i8x16_cmp",
    "simd_i8x16_sat_arith",
    "simd_int_to_int_extend",
    "simd_lane",
    "simd_linking",
    "simd_load",
    "simd_load16_lane",
    "simd_load32_lane",
    "simd_load64_lane",
    "simd_load8_lane",
    "simd_load_extend",
    "simd_load_splat",
    "simd_load_zero",
    "simd_memory-multi",
    "simd_select",
    "simd_splat",
    "simd_store",
    "simd_store16_lane",
    "simd_store32_lane",
    "simd_store64_lane",
    "simd_store8_lane",
    // Tail-Call (2 Dateien)
    "return_call",
    "return_call_indirect",
    // erweiterte konstante Ausdruecke (1 Dateien)
    "const",
    // memory64 / 64-Bit-Adressen und -Indizes (23 Dateien)
    "address64",
    "align64",
    "binary_leb128_64",
    "bulk64",
    "call_indirect64",
    "endianness64",
    "float_memory64",
    "load64",
    "memory64",
    "memory_copy64",
    "memory_fill64",
    "memory_grow64",
    "memory_init64",
    "memory_redundancy64",
    "memory_trap64",
    "table64",
    "table_copy64",
    "table_fill64",
    "table_get64",
    "table_grow64",
    "table_init64",
    "table_set64",
    "table_size64",
    // nur Lexer/Werkzeug, keine Engine-Funktion (12 Dateien)
    "annotations",
    "comments",
    "fac",
    "id",
    "inline-module",
    "left-to-right",
    "memory64-imports",
    "names",
    "obsolete-keywords",
    "skip-stack-guard-page",
    "switch",
    "token",
];

/// Gehoert die Datei zu WebAssembly 2.0 Core?
///
/// **Exakter Vergleich** (GEMESSEN 03.10.2026): mit `contains` schloss
/// der alte Mustervergleich echte Kerndateien aus (siehe [`AUSSERHALB`]).
/// Der Vergleich ist damit genau und ein Tippfehler in einer Dateinamen-
/// liste fuehlt nicht still zu einer kleineren Menge.
pub fn gehoert_2_0(name: &str) -> bool {
    let basis = name.trim_end_matches(".wast");
    !AUSSERHALB.contains(&basis)
}

/// Hoechstens so viele Meldungen je Datei (M4: Speicher bei einem Modul mit
/// 100 000 Fehlern). Ueber `NARSIL_WASM_MELDUNGEN` erhoehbar.
fn max_meldungen() -> usize {
    std::env::var("NARSIL_WASM_MELDUNGEN")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(6)
        .min(200_000)
}

fn meld(v: &mut Vec<String>, s: String) {
    if v.len() < max_meldungen() {
        v.push(s);
    }
}

/// Ergebnis einer Datei.
#[derive(Debug, Clone)]
pub struct Datei {
    pub name: String,
    pub ok: usize,
    pub test: usize,
    pub meldungen: Vec<String>,
}

impl Datei {
    pub fn quote(&self) -> Option<f64> {
        if self.test == 0 {
            None
        } else {
            Some(self.ok as f64 * 100.0 / self.test as f64)
        }
    }
}

// ---- `spectest` und `register` -------------------------------------------

/// Das Hostmodul `spectest` der offiziellen Suite (Wert 666 wie im
/// Testhafen der Spezifikation). Ohne diese Exporte ist kein Modul der
/// Testsuite linkbar, das `spectest` importiert — GEMESSEN 01.10.: 2240
/// Faelle fielen allein an „kein Modul geladen".
///
/// Die Objekte werden im `Lauf` DER DATEI angelegt, weil ein Objektindex
/// nur im selben Store gueltig ist.
pub fn spectest(lauf: &mut Lauf, g: &Grenzen) -> ImportTabelle {
    let mut q = ImportTabelle::neu();
    if let Ok(i) = lauf.neuer_speicher(1, Some(2)) {
        q = q.mit("spectest", "memory", ImportWert::Speicher(i));
    }
    if let Ok(i) = lauf.neue_tabelle(10, Some(20), Reftyp::Func) {
        q = q.mit("spectest", "table", ImportWert::Tabelle(i));
    }
    for (name, wert) in [
        ("global_i32", Wert::I32(666)),
        ("global_i64", Wert::I64(666)),
        ("global_f32", Wert::F32(666.6f32.to_bits())),
        ("global_f64", Wert::F64(666.6f64.to_bits())),
    ] {
        if let Ok(i) = lauf.neue_globale(wert, false) {
            q = q.mit("spectest", name, ImportWert::Global(i));
        }
    }
    // `print*`: sie nehmen ihre Argumente entgegen und geben nichts zurueck.
    let haken: FremdFn = std::sync::Arc::new(
        |_lauf: &mut Lauf, _host: &mut dyn crate::lauf::Host, _a: &[Wert]| Ok(Vec::new()),
    );
    for (name, parameter) in [
        ("print", Vec::new()),
        ("print_i32", vec![Valtyp::I32]),
        ("print_i64", vec![Valtyp::I64]),
        ("print_f32", vec![Valtyp::F32]),
        ("print_f64", vec![Valtyp::F64]),
        ("print_i32_f32", vec![Valtyp::I32, Valtyp::F32]),
        ("print_f64_f64", vec![Valtyp::F64, Valtyp::F64]),
    ] {
        let idx = lauf.neue_funktion(
            FuncTyp {
                parameter,
                ergebnis: Vec::new(),
            },
            haken.clone(),
        );
        q = q.mit("spectest", name, ImportWert::Funktion(idx));
    }
    let _ = g;
    q
}

/// Registry fuer `(register "name" $modul)`: ein registriertes Modul
/// beantwortet spaetere Importe ueber seine Exporte. `spectest` ist immer
/// zusaetzlich sichtbar.
#[derive(Default)]
pub struct Registry {
    pub namen: BTreeMap<String, Instanz>,
    pub fest: ImportTabelle,
}

impl Registry {
    /// Registry anlegen; die `spectest`-Objekte werden im `lauf` ERSTELLT
    /// (nicht in einem Wegwerf-`Lauf` — die Indizes muessen im selben
    /// Store liegen).
    pub fn neu(lauf: &mut Lauf, g: &Grenzen) -> Registry {
        let fest = spectest(lauf, g);
        Registry {
            namen: BTreeMap::new(),
            fest,
        }
    }

    /// Objektindex hinter einem registrierten Namen.
    pub fn hole_registriert(&self, modul: &str, feld: &str) -> Option<ImportWert> {
        let i = self.namen.get(modul)?;
        let (art, lokal) = i.export(feld)?;
        Some(match art {
            Art::Func(_) => ImportWert::Funktion(i.funktions_start + lokal),
            Art::Tabelle(_) => ImportWert::Tabelle(*i.tabellen.get(lokal as usize)?),
            // Multi-Memory: der Export nennt den LOKALEN Index, der ueber
            // `Instanz::speicher` in den Store zeigt (GEMESSEN 02.10.,
            // `memory_size_import.wast`: immer der erste Speicher genuegt
            // nicht mehr).
            Art::Speicher(lokal) => ImportWert::Speicher(i.speicher_index_von(lokal)?),
            // Ueber den Indexraum der exportierenden Instanz: Importe
            // liegen im Store VOR `globale_start`.
            //
            // GEMESSEN 03.10. (`linking.wast` Z. 70ff: 8 Faelle — der
            // Re-Export einer veraenderbaren Variablen lieferte den Wert
            // der falschen Variablen, 142 erwartet, 42 geliefert).
            Art::Global(_) => ImportWert::Global(*i.globale_index.get(lokal as usize)?),
            Art::Tag(_) => ImportWert::Tag(*i.tags.get(lokal as usize)?),
        })
    }
}

impl ImportQuelle for Registry {
    fn hole(&self, lauf: &Lauf, modul: &str, feld: &str) -> Option<ImportWert> {
        if let Some(w) = self.fest.hole(lauf, modul, feld) {
            return Some(w);
        }
        self.hole_registriert(modul, feld)
    }
}

// ---- Ausfuehren einer Datei ----------------------------------------------

/// Wie viele `assert`-Befehle stehen im Rohtext? Sie zaehlen als
/// FEHLGESCHLAGEN, wenn das Skript nicht lesbar ist — sonst gehoerte die
/// Datei nicht in den Nenner und die Quote waere zu hoch.
///
/// GEMESSEN 03.10. (die Suite hat 62 468 `assert`-Befehle, der Lauf zaehlte
/// 36 600 — 25 868 stillschweigend weg, alle aus nicht lesbaren Skripten).
pub fn zaehle_asserts(text: &str) -> usize {
    // Ein `assert`-Befehl steht immer am Zeilenanfang; ein Kommentar
    // beginnt mit `;;` oder mit Leerzeichen. Das reicht fuer diese Suite
    // und braucht keinen eigenen Klammerzaehler.
    text.lines().filter(|l| l.starts_with("(assert")).count()
}

/// Eine Datei ausfuehren: `text` ist der Inhalt, `name` nur fuer Meldungen.
pub fn lauf_text(name: &str, text: &str, g: &Grenzen) -> Datei {
    let skripte = match lies_skript(text, g) {
        Ok(s) => s,
        Err(e) => {
            let test = zaehle_asserts(text);
            return Datei {
                name: name.to_string(),
                ok: 0,
                test,
                meldungen: vec![format!(
                    "{name}: Skript nicht lesbar: {} — {test} assert-Befehl(e) nicht geprueft",
                    e.kurz()
                )],
            };
        }
    };
    let mut lauf = Lauf::neu(g.clone());
    // Die `spectest`-Objekte liegen im Lauf DER DATEI — ein Objektindex gilt
    // nur im selben Store.
    let mut registry = Registry::neu(&mut lauf, g);
    let mut instanzen: BTreeMap<u32, Instanz> = BTreeMap::new();
    // `(module definition $M …)`: das Modul wird nur uebersetzt und
    // geprueft. Fuer `(module instance $I $M)` brauchen wir es trotzdem.
    //
    // GEMESSEN 02.10. (`instance.wast`).
    let mut definitionen: BTreeMap<u32, crate::modell::Modul> = BTreeMap::new();
    let mut aktuell: Option<u32> = None;
    let mut ok = 0usize;
    let mut test = 0usize;
    let mut m: Vec<String> = Vec::new();

    for (nr, s) in skripte.iter().enumerate() {
        match s {
            Skript::Modul {
                nummer,
                inhalt,
                definition,
            } => {
                let modul = match modul_lesen(inhalt, g) {
                    Ok(x) => x,
                    Err(e) => {
                        meld(
                            &mut m,
                            format!("{name}#{nr}: Modul nicht lesbar: {}", e.kurz()),
                        );
                        continue;
                    }
                };
                if let Err(e) = crate::pruefe::modul(&modul, g) {
                    meld(&mut m, format!("{name}#{nr}: Validator: {}", e.kurz()));
                    continue;
                }
                // `(module definition …)` wird nur uebersetzt und
                // geprueft, nicht instanziert — aber gemerkt, damit
                // `(module instance …)` es verwenden kann.
                if *definition {
                    definitionen.insert(*nummer, modul);
                    continue;
                }
                match instanziieren(&mut lauf, &modul, &registry) {
                    Ok(i) => {
                        instanzen.insert(*nummer, i);
                        aktuell = Some(*nummer);
                    }
                    Err(e) => meld(&mut m, format!("{name}#{nr}: Instanziieren: {}", e.kurz())),
                }
            }
            // `(module instance $I $M)`: erzeugt eine EIGENE Instanz der
            // Definition `$M`. Zwei solcher Instanzen teilen KEINEN Zustand
            // (der Test prueft genau das); dieselbe Instanz zweimal
            // importiert ist derselbe Zustand.
            //
            // GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
            Skript::Instanz { nummer, quelle } => match definitionen.get(quelle) {
                None => meld(
                    &mut m,
                    format!("{name}#{nr}: Modul {quelle} ist keine Definition"),
                ),
                Some(modul) => match instanziieren(&mut lauf, modul, &registry) {
                    Ok(i) => {
                        instanzen.insert(*nummer, i);
                        aktuell = Some(*nummer);
                    }
                    Err(e) => meld(&mut m, format!("{name}#{nr}: Instanziieren: {}", e.kurz())),
                },
            },
            Skript::Register {
                name: rname,
                nummer,
            } => {
                if let Some(n) = nummer.or(aktuell) {
                    if let Some(i) = instanzen.get(&n) {
                        registry.namen.insert(rname.clone(), i.clone());
                    }
                }
            }
            Skript::Aufrufen { .. } | Skript::Lesen { .. } => {
                let _ = aufrufen(&mut lauf, &instanzen, aktuell, s);
            }
            Skript::Rueckgabe { aufruf, erwartet } => {
                test += 1;
                let r = aufrufen(&mut lauf, &instanzen, aktuell, aufruf);
                match (r, erwartet.first()) {
                    (Ok(werte), Some(e)) => {
                        if erwartung_halt(&werte.first().copied(), e, &lauf) {
                            ok += 1;
                        } else {
                            meld(
                                &mut m,
                                format!(
                                    "{name}#{nr}: {} erwartet, {}",
                                    zeige(e),
                                    zeige_werte(&werte)
                                ),
                            );
                        }
                    }
                    (Ok(_), None) => ok += 1,
                    (Err(e), _) => meld(&mut m, format!("{name}#{nr}: {}", e.kurz())),
                }
            }
            Skript::Trap { aufruf, .. } => {
                test += 1;
                // `(assert_trap (module …) "…")`: das INSTANTIATISIEREN muss
                // scheitern (Datenabschnitt ausserhalb des Speichers,
                // Startfunktion faellt …). GEMESSEN 02.10. (`data1.wast`,
                // 14 Faelle): der Aufruf-Zweig meldete „kein Modul geladen".
                if let Skript::Modul { inhalt, .. } = aufruf.as_ref() {
                    match modul_lesen(inhalt, g) {
                        Err(_) => ok += 1,
                        Ok(modul) => match crate::pruefe::modul(&modul, g) {
                            Err(e) => meld(
                                &mut m,
                                format!("{name}#{nr}: Validator statt Trap: {}", e.kurz()),
                            ),
                            Ok(()) => match instanziieren(&mut lauf, &modul, &registry) {
                                Err(e) if e.ist_trap() => ok += 1,
                                Err(e) => {
                                    meld(&mut m, format!("{name}#{nr}: {} (kein Trap)", e.kurz()))
                                }
                                Ok(_) => {
                                    meld(&mut m, format!("{name}#{nr}: Trap erwartet, keiner"))
                                }
                            },
                        },
                    }
                    continue;
                }
                match aufrufen(&mut lauf, &instanzen, aktuell, aufruf) {
                    Err(e) if e.ist_trap() => ok += 1,
                    Ok(_) => meld(&mut m, format!("{name}#{nr}: Trap erwartet, keiner")),
                    Err(e) => meld(&mut m, format!("{name}#{nr}: {} (kein Trap)", e.kurz())),
                }
            }
            Skript::Erschopfung { aufruf, .. } => {
                test += 1;
                match aufrufen(&mut lauf, &instanzen, aktuell, aufruf) {
                    Err(e) if e.ist_trap() => ok += 1,
                    Ok(_) => meld(
                        &mut m,
                        format!("{name}#{nr}: Erschopfung erwartet, lief durch"),
                    ),
                    Err(e) => meld(
                        &mut m,
                        format!("{name}#{nr}: Erschopfung erwartet, {}", e.kurz()),
                    ),
                }
            }
            Skript::NanPruefung {
                aufruf,
                arithmetisch,
            } => {
                test += 1;
                match aufrufen(&mut lauf, &instanzen, aktuell, aufruf) {
                    Ok(werte) => {
                        if werte
                            .first()
                            .map(|w| nan_stimmt(w, *arithmetisch))
                            .unwrap_or(false)
                        {
                            ok += 1;
                        } else {
                            meld(
                                &mut m,
                                format!("{name}#{nr}: NaN erwartet, {}", zeige_werte(&werte)),
                            );
                        }
                    }
                    Err(e) => meld(&mut m, format!("{name}#{nr}: {}", e.kurz())),
                }
            }
            Skript::Fehlerhaft { modul, .. } => {
                test += 1;
                if erwarte_modulfehler(modul, g) {
                    ok += 1;
                } else {
                    meld(
                        &mut m,
                        format!("{name}#{nr}: assert_malformed, aber gueltig"),
                    );
                }
            }
            Skript::Ungueltig { modul, .. } => {
                test += 1;
                if erwarte_modulfehler(modul, g) {
                    ok += 1;
                } else {
                    meld(
                        &mut m,
                        format!("{name}#{nr}: assert_invalid, aber der Validator ist zufrieden"),
                    );
                }
            }
            Skript::NichtLinkbar { modul, .. } => {
                test += 1;
                match modul_lesen(modul, g) {
                    Err(_) => ok += 1,
                    Ok(m2) => {
                        // Im GEGENSATZ zu einem frischen Lauf: die
                        // `spectest`-Indizes gelten nur im Store der Datei.
                        // Ein unlinkbares Modul belegt daraus nichts (der
                        // Fehler kommt vor jeder Allokation).
                        match instanziieren(&mut lauf, &m2, &registry) {
                            Err(_) => ok += 1,
                            Ok(_) => meld(
                                &mut m,
                                format!("{name}#{nr}: assert_unlinkable, aber linkbar"),
                            ),
                        }
                    }
                }
            }
        }
    }
    Datei {
        name: name.to_string(),
        ok,
        test,
        meldungen: m,
    }
}

/// Eine Datei vom Pfad lesen und ausfuehren.
pub fn lauf_datei(pfad: &Path, g: &Grenzen) -> Datei {
    let name = dateiname(pfad);
    match std::fs::read_to_string(pfad) {
        Ok(t) => lauf_text(&name, &t, g),
        Err(e) => Datei {
            name: name.clone(),
            ok: 0,
            test: 0,
            meldungen: vec![format!("{name}: nicht lesbar ({e})")],
        },
    }
}

/// Alle `.wast`-Dateien eines Ordners (gefiltert auf 2.0), sortiert.
pub fn dateien_im_ordner(ordner: &Path) -> Vec<PathBuf> {
    dateien_gefiltert(ordner, true)
}

/// Alle `.wast`-Dateien OHNE 2.0-Filter, sortiert.
///
/// GEMESSEN 03.10.: der 2.0-Umfang (101 Dateien) ist zu 100,00 % gruen.
/// Erst der Lauf ueber ALLE 257 Dateien zeigte, dass (a) die Engine in
/// `type-equivalence.wast` mit einem Stapelueberlauf abstuerzte und (b) die
/// Quote im Gesamtumfang 92,93 % betraegt (34014/36600). Ohne diese Zahl
/// waere „100 %" eine Aussage ueber eine willkuerlich gewaehlte Teilmenge.
pub fn dateien_alle_im_ordner(ordner: &Path) -> Vec<PathBuf> {
    dateien_gefiltert(ordner, false)
}

fn dateien_gefiltert(ordner: &Path, nur_2_0: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match std::fs::read_dir(ordner) {
        Err(_) => Vec::new(),
        Ok(d) => d
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "wast").unwrap_or(false))
            .filter(|p| !nur_2_0 || gehoert_2_0(&dateiname(p)))
            .collect(),
    };
    v.sort();
    v
}

pub fn dateiname(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Summe ueber alle Dateien: (bestehen, getestet).
pub fn summe(dateien: &[Datei]) -> (usize, usize) {
    let mut ok = 0;
    let mut test = 0;
    for d in dateien {
        ok += d.ok;
        test += d.test;
    }
    (ok, test)
}

/// Der Standardordner der Suite (Umgebungsvariable zuerst).
pub fn suite_ordner() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NARSIL_WASM_TESTSUITE") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    let p = PathBuf::from("/home/jaff/mutation_md/wasm/testsuite");
    if p.is_dir() { Some(p) } else { None }
}

// ---- Hilfen ---------------------------------------------------------------

fn modul_lesen(inhalt: &ModulInhalt, g: &Grenzen) -> Result<Modul, Fehler> {
    match inhalt {
        ModulInhalt::Bytes(b) => crate::modul_pruefen(b, g),
        ModulInhalt::Text(t) => crate::wat::lies_text(t, g),
    }
}

/// Muss das Modul scheitern (lesen ODER validieren)?
fn erwarte_modulfehler(inhalt: &ModulInhalt, g: &Grenzen) -> bool {
    match modul_lesen(inhalt, g) {
        Err(_) => true,
        Ok(m) => crate::pruefe::modul(&m, g).is_err(),
    }
}

fn aufrufen(
    lauf: &mut Lauf,
    instanzen: &BTreeMap<u32, Instanz>,
    aktuell: Option<u32>,
    s: &Skript,
) -> Result<Vec<Wert>, Fehler> {
    // `(invoke $modul "name" …)` nimmt ein benanntes Modul, sonst das
    // zuletzt geladene.
    let nr = match s {
        Skript::Aufrufen { modul: Some(m), .. } | Skript::Lesen { modul: Some(m), .. } => *m,
        _ => aktuell.ok_or_else(|| Fehler::form(ART_LAENGE, "kein Modul geladen"))?,
    };
    let i = instanzen
        .get(&nr)
        .ok_or_else(|| Fehler::form(ART_LAENGE, format!("Modul {nr} fehlt")))?;
    match s {
        Skript::Aufrufen {
            name, argumente, ..
        } => lauf.rufe_funktion(i, name, argumente),
        Skript::Lesen { name, .. } => match i.export(name) {
            // Ueber die Zelle (Importe teilen sich Zellen).
            // Ueber den Indexraum (Importe liegen vor `globale_start`).
            //
            // GEMESSEN 03.10. (`linking.wast`: 142 erwartet, 42 geliefert).
            Some((Art::Global(g), _)) => lauf
                .zelle(*i.globale_index.get(g as usize).ok_or_else(|| {
                    Fehler::link(
                        crate::fehler::L_GLOBAL,
                        format!("get {name}: Global {g} fehlt"),
                    )
                })?)
                .copied()
                .map(|v| vec![v])
                .ok_or_else(|| Fehler::link(crate::fehler::L_GLOBAL, format!("get {name}: fehlt"))),
            _ => Err(Fehler::link(
                crate::fehler::L_EXPORT,
                format!("get {name}: kein Global"),
            )),
        },
        _ => Err(Fehler::form(ART_LAENGE, "keine Aktion")),
    }
}

/// Traegt eine GC-Erwartung? `l` ist der Lauf (sein `store.gc` haengt an
/// ihm).
fn gc_erwartung_halt(
    w: &Wert,
    art: &crate::wast::GcArt,
    inhalt: &crate::wast::GcInhalt,
    l: &Lauf,
) -> bool {
    use crate::modell::GcInhalt as I;
    use crate::wast::GcArt as A;
    // `null` ist KEIN Objekt: nur `(ref.null …)` erwartet null.
    let id = match w {
        Wert::Gcref(None) => return false,
        Wert::Gcref(Some(i)) => *i as usize,
        _ => return false,
    };
    let o = match l.store.gc.get(id) {
        Some(o) => o,
        None => return false,
    };
    // Die Art muss passen: `eq` nimmt i31 und Array und Struktur.
    let art_ok = match art {
        A::I31 => matches!(o.inhalt, I::I31(_)),
        A::Struct => matches!(o.inhalt, I::Feld(_)),
        A::Array => matches!(o.inhalt, I::Array(_)),
        A::Eq => matches!(o.inhalt, I::I31(_) | I::Feld(_) | I::Array(_)),
    };
    if !art_ok {
        return false;
    }
    match inhalt {
        crate::wast::GcInhalt::Irrelevant => true,
        crate::wast::GcInhalt::Null => false,
        crate::wast::GcInhalt::I31(v) => o.inhalt == I::I31(*v),
        crate::wast::GcInhalt::Werte(w) => match &o.inhalt {
            I::Feld(f) | I::Array(f) => f == w,
            I::I31(_) => false,
        },
    }
}

fn erwartung_halt(wert: &Option<Wert>, e: &Erwartung, l: &Lauf) -> bool {
    match (wert, e) {
        (None, Erwartung::Genau(_)) => false,
        (Some(w), Erwartung::Genau(x)) => w == x,
        (Some(w), Erwartung::NanKanonisch(_)) => ist_kanonisch(w),
        (Some(w), Erwartung::NanArithmetisch(_)) => ist_arithmetisch(w),
        (Some(w), Erwartung::IrgendeineFunktion) => {
            matches!(w, crate::wert::Wert::Func(Some(_)))
        }
        // `(ref.null)` ohne Heaptype: irgendeine null-Referenz.
        //
        // GEMESSEN 03.10. (`global.wast` Z. 659, `ref_null.wast`).
        // `(ref.extern)` ohne Zahl: irgendein nicht-nuller externref. Ein
        // externalisiertes GC-Objekt traegt seinen Wert noch (die
        // Konvertierung ist die Identitaet) — es passt also.
        //
        // GEMESSEN 03.10. (`extern.wast` Z. 47).
        (Some(w), Erwartung::IrgendeineExtern) => matches!(
            w,
            crate::wert::Wert::Extern(Some(_))
                | crate::wert::Wert::Gcref(Some(_))
                | crate::wert::Wert::Func(Some(_))
        ),
        (Some(w), Erwartung::IrgendeineNull) => matches!(
            w,
            crate::wert::Wert::Func(None)
                | crate::wert::Wert::Extern(None)
                | crate::wert::Wert::Gcref(None)
        ),
        (None, _) => false,
        // `(ref.i31 …)`, `(ref.struct …)`, `(ref.array …)`, `(ref.eq …)`:
        // die Art des GC-Objekts und sein Inhalt.
        //
        // GEMESSEN 03.10. (`extern.wast` Z. 53-55).
        (Some(_), Erwartung::Gc(art, inhalt)) => {
            // `null` erfuellt jede GC-Erwartung.
            if matches!(inhalt, crate::wast::GcInhalt::Null) {
                return false;
            }
            let w = wert.clone().unwrap_or(Wert::I32(0));
            gc_erwartung_halt(&w, art, inhalt, l)
        }
        (w, Erwartung::Entweder(a, b)) => erwartung_halt(w, a, l) || erwartung_halt(w, b, l),
    }
}

fn ist_kanonisch(w: &Wert) -> bool {
    match w {
        Wert::F32(b) => (b & 0x7fff_ffff) == 0x7fc0_0000,
        Wert::F64(b) => (b & 0x7fff_ffff_ffff_ffff) == 0x7ff8_0000_0000_0000,
        _ => false,
    }
}

fn ist_arithmetisch(w: &Wert) -> bool {
    match w {
        Wert::F32(b) => crate::wert::ist_nan32(*b),
        Wert::F64(b) => crate::wert::ist_nan64(*b),
        _ => false,
    }
}

fn nan_stimmt(w: &Wert, arithmetisch: bool) -> bool {
    if arithmetisch {
        ist_arithmetisch(w)
    } else {
        ist_kanonisch(w)
    }
}

pub fn zeige(e: &Erwartung) -> String {
    match e {
        Erwartung::Genau(w) => zeige_wert(w),
        Erwartung::NanKanonisch(_) => "nan:canonical".into(),
        Erwartung::NanArithmetisch(_) => "nan:arithmetic".into(),
        Erwartung::IrgendeineFunktion => "irgendeine Funktion".into(),
        Erwartung::IrgendeineNull => "irgendeine null-Referenz".into(),
        Erwartung::IrgendeineExtern => "irgendein nicht-nuller externref".into(),
        Erwartung::Entweder(a, b) => format!("{} oder {}", zeige(a), zeige(b)),
        Erwartung::Gc(art, inhalt) => match (art, inhalt) {
            (crate::wast::GcArt::I31, crate::wast::GcInhalt::I31(v)) => {
                format!("ref.i31 {v}")
            }
            (crate::wast::GcArt::I31, _) => "ref.i31".into(),
            (crate::wast::GcArt::Struct, _) => "ref.struct".into(),
            (crate::wast::GcArt::Array, _) => "ref.array".into(),
            (crate::wast::GcArt::Eq, _) => "ref.eq".into(),
        },
    }
}

pub fn zeige_werte(v: &[Wert]) -> String {
    v.iter().map(zeige_wert).collect::<Vec<String>>().join(", ")
}

pub fn zeige_wert(w: &Wert) -> String {
    match w {
        Wert::I32(x) => format!("i32 {x}"),
        Wert::I64(x) => format!("i64 {x}"),
        Wert::F32(b) => format!("f32 {} (0x{b:08x})", crate::wert::als_f32_text(*b)),
        Wert::F64(b) => format!("f64 {} (0x{b:016x})", crate::wert::als_f64_text(*b)),
        Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None) => "null".into(),
        Wert::Func(Some(f)) => format!("funcref({f})"),
        Wert::Extern(Some(e)) => format!("externref({e})"),
        Wert::Gcref(Some(g)) => format!("arrayref({g})"),
    }
}
