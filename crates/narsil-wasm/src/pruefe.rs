//! W1 Punkt 2: Validator nach WebAssembly 2.0 Core.
//!
//! Geprueft wird das, was die Spezifikation im Abschnitt „Validation"
//! verlangt: Typen der Stuecke, Signaturen, Indizes, Speichergroessen,
//! Startfunktion, Exportnamen, Konstantenausdruecke, Referenztypen-Regeln.
//!
//! Der Algorithmus ist der uebliche: ein Wertstapel mit `Option<Valtyp>`
//! (`None` = „unbekannt", der Zustand nach `unreachable`) und ein Stapel von
//! Kontrollrahmen. Nach `unreachable` duerfen beliebige Pop-Operationen
//! passieren — genau das bildet `None` ab.

use crate::fehler::*;
use crate::grenzen::Grenzen;
use crate::modell::typ_ungueltig;
use crate::modell::{
    Art, Daten, Element, Func, FuncTyp, ImportiertSpeicher, ImportiertTab, Modul, Reftyp, Valtyp,
};
use crate::op::Befehl;
use std::collections::BTreeSet;

/// Kontext des Validators: Modul + Grenzen + gesammelte Fakten.
struct Ctx<'a> {
    m: &'a Modul,
    g: &'a Grenzen,
    /// Funktionen, die `ref.func` benutzen, muessen im Elementabschnitt
    /// deklariert sein (`C.refs`-Regel der Spezifikation).
    refs: BTreeSet<u32>,
}

/// Prueft ein ganzes Modul. Erst der Aufruf von `binaer::lies_modul`.
pub fn modul(m: &Modul, g: &Grenzen) -> Result<(), Fehler> {
    if m.typen.len() > g.typen {
        return Err(Fehler::ungueltig(
            V_GRENZE,
            format!("{} Typen", m.typen.len()),
        ));
    }
    if m.funktionen.len() > g.funktionen {
        return Err(Fehler::ungueltig(
            V_GRENZE,
            format!("{} Funktionen", m.funktionen.len()),
        ));
    }
    let ctx = Ctx {
        m,
        g,
        refs: sammle_refs(m),
    };
    pruefe_indizes(&ctx)?;
    pruefe_typreferenzen(&ctx)?;
    pruefe_sub_deklaration(&ctx)?;
    pruefe_tabellen(&ctx)?;
    pruefe_globale(&ctx)?;
    pruefe_speicher_und_tabellen(&ctx)?;
    pruefe_const_ausdruecke(&ctx)?;
    pruefe_funktionen(&ctx)?;
    pruefe_start(&ctx)?;
    pruefe_exporte(&ctx)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Indizes: jede Referenz muss im jeweiligen Indexraum liegen
// ---------------------------------------------------------------------------
fn pruefe_indizes(c: &Ctx) -> Result<(), Fehler> {
    let m = c.m;
    // Speicher: WebAssembly 2.0 erlaubt genau EINEN, Multi-Memory (Wasm 3.0)
    // mehrere. Die harte Grenze ist `Grenzen::speicher_seiten` je Speicher
    // (M3/M4) — mehr Speicher kosten Speicher, also bekommen sie dieselbe
    // Deckelung, und der Speicherindex jedes Zugriffs wird geprueft.
    let speicher_anzahl = m.speicher.len();
    if speicher_anzahl as u64 > c.g.speicher_seiten {
        return Err(Fehler::ungueltig(
            V_SPEICHER,
            format!(
                "{speicher_anzahl} Speicher ueber der Grenze {}",
                c.g.speicher_seiten
            ),
        ));
    }
    for f in &m.funktionen {
        if let Func::Importiert { typ, .. } = f {
            if (*typ as usize) >= m.typen.len() {
                return Err(typ_ungueltig(*typ));
            }
        }
    }
    for t in &m.tabellen {
        if let ImportiertTab::Importiert { typ, .. } = t {
            if typ.grenzen.min > c.g.tabelle_plaetze {
                return Err(Fehler::ungueltig(
                    V_TABELLE,
                    format!("importierte Tabelle: {} Plaetze", typ.grenzen.min),
                ));
            }
        }
    }
    for s in &m.speicher {
        if let ImportiertSpeicher::Importiert { typ, .. } = s {
            if typ.grenzen.min > c.g.speicher_seiten {
                return Err(Fehler::ungueltig(
                    V_SPEICHER,
                    "importierter Speicher zu gross",
                ));
            }
        }
    }
    // DataCount
    if let Some(n) = m.daten_anzahl {
        if n as usize != m.daten.len() {
            return Err(Fehler::form(
                ART_ZAEHLER,
                format!("DataCount {n} != {} Datenabschnitte", m.daten.len()),
            ));
        }
    }
    // Elemente: Tabellindex und Typ
    for (i, e) in m.elemente.iter().enumerate() {
        // Jeder Funktionsindex in der Eintragsliste muss existieren —
        // auch in einem PASSIVEN oder DEKLARATIVEN Abschnitt, denn beide
        // nennen nur den Typ, nicht das Ziel.
        //
        // GEMESSEN 03.10. (`return_call_indirect.wast` Z. 218ff:
        // `(module (table funcref (elem 0 0)))` muss „unknown function 0"
        // sein).
        for (j, eintrag) in e.eintraege().iter().enumerate() {
            // Der Eintrag traegt am Ende ein `End`; entscheidend ist der
            // ERSTE Befehl.
            if let Some(crate::op::Befehl::RefFunc(f)) = eintrag.first() {
                if (*f as usize) >= m.funktionen.len() {
                    return Err(Fehler::form(
                        ART_INDEX,
                        format!("Element {i}, Eintrag {j}: unbekannte Funktion {f}"),
                    ));
                }
            }
        }
        if let Element::Aktiv { tabelle, .. } = e {
            if (*tabelle as usize) >= m.tabellen.len() {
                return Err(Fehler::form(
                    ART_INDEX,
                    format!("Element {i}: Tabellenindex {tabelle} unbekannt"),
                ));
            }
            // Der Elementtyp muss zum Tabellentyp passen — bei EIGENEN wie
            // bei importierten Tabellen. GEMESSEN 02.10. (`elem.wast`
            // Z. 120: `(table 1 externref)` mit einem `funcref`-Element).
            let tabellentyp = match &m.tabellen[*tabelle as usize] {
                ImportiertTab::Importiert { typ, .. } => typ.reftyp,
                ImportiertTab::Eigen { typ, .. } => typ.reftyp,
            };
            // Subtyping: ein `funcref`-Element passt in eine `(ref func)`
            // (nicht-leerbare) Tabelle, ein `(ref null $t)`-Element in
            // `(ref null func)`.
            //
            // GEMESSEN 02.10. (`elem.wast` Z. 2/51/52/55/56).
            if !crate::modell::reftyp_passt_als(m, &e.reftyp(), &tabellentyp) {
                return Err(Fehler::ungueltig(
                    V_TABELLE,
                    format!(
                        "Element {i}: Tabellentyp {} != Elementtyp {}",
                        tabellentyp.text(),
                        e.reftyp().text()
                    ),
                ));
            }
        }
    }
    // Daten: Speicherindex
    for (i, d) in m.daten.iter().enumerate() {
        if let Daten::Aktiv { speicher, .. } = d {
            if (*speicher as usize) >= m.speicher.len() {
                return Err(Fehler::form(
                    ART_INDEX,
                    format!("Datenabschnitt {i}: Speicherindex {speicher} unbekannt"),
                ));
            }
        }
    }
    Ok(())
}

/// Jeder Typindex in einem `(ref … $t)` muss im Typraum stehen
/// („unknown type", GEMESSEN 02.10.: `ref.wast` Z. 1/2/4/6/9/10/12).
/// Die Gueltigkeit einer `(sub $s (…))`-Deklaration.
///
/// Zwei Regeln, und **keine** dritte (GEMESSEN 04.10.,
/// `type-subtyping.wast` Z. 780–950: 19 `assert_invalid`):
///
/// 1. Der Obertyp `$s` darf **nicht final** sein — weder ein nackter
///    `(type $t (func))` noch ein `(sub final …)`. Ein finaler Typ hat
///    keine Subtypen.
/// 2. Die deklarierte Definition muss ein **Subtyp** der Definition von
///    `$s` sein (`Modell::def_subtyp` decides Klasse, Arity, Breite und
///    Veraenderbarkeit).
fn pruefe_sub_deklaration(c: &Ctx) -> Result<(), Fehler> {
    let m = c.m;
    for (i, ober) in m.sub_ober.iter().enumerate() {
        let Some(s) = *ober else {
            continue;
        };
        // Final heisst: **nicht** als `(sub …)` deklariert — oder
        // ausdruecklich als `(sub final …)`.
        let final_typ = !m
            .sub_ge_deklariert
            .get(s as usize)
            .copied()
            .unwrap_or(false)
            || m.sub_final.get(s as usize).copied().unwrap_or(false);
        if final_typ {
            return Err(Fehler::ungueltig(
                V_TYP,
                format!("Typ {i}: sub type: Typ {s} ist final und hat keine Subtypen"),
            ));
        }
        if !crate::modell::def_subtyp(m, i as u32, s) {
            return Err(Fehler::ungueltig(
                V_TYP,
                format!("Typ {i}: sub type: Rumpf passt nicht zum Obertyp {s}"),
            ));
        }
    }
    Ok(())
}

fn pruefe_typreferenzen(c: &Ctx) -> Result<(), Fehler> {
    fn pruef(c: &Ctx, t: &Valtyp, was: &str) -> Result<(), Fehler> {
        if let Valtyp::Ref(Reftyp::Konkret { typ, .. }) = t {
            // `(ref func)`/`(ref extern)` tragen einen Wildcard-Index: der
            // ist immer gueltig (GEMESSEN 03.10., `table.wast` Z. 30).
            if crate::modell::ist_wildcard(*typ) {
                return Ok(());
            }
            if (*typ as usize) >= c.m.typen.len() {
                return Err(Fehler::ungueltig(
                    V_TYP,
                    format!("{was}: (ref … {typ}) — Typ {typ} unbekannt"),
                ));
            }
        }
        Ok(())
    }
    for (i, ft) in c.m.typen.iter().enumerate() {
        for t in ft.parameter.iter().chain(ft.ergebnis.iter()) {
            pruef(c, t, &format!("Typ {i}"))?;
        }
    }
    // Die GC-Typen tragen ihre Werttypen NICHT im `FuncTyp`, sondern in
    // `strukt`/`arrays` — ohne diese Runde blieb `(type (struct (field
    // (ref 1))))` ungeprueft.
    //
    // GEMESSEN 03.10. (`struct.wast` Z. 36–43: 2 `assert_invalid`
    // „unknown type").
    // **Sichtbarkeit der Typindizes** (Wasm 3.0 GC): ausserhalb einer
    // `(rec …)`-Gruppe darf ein `(ref N)` nur **frueher** deklarierte
    // Typen nennen; innerhalb nur Mitglieder derselben Gruppe.
    //
    // GEMESSEN 03.10. (`type-rec.wast` Z. 21ff und Z. 28ff, 2x
    // „unknown type"):
    //     (type (func (param (ref 1)))) (type (func))    <- vorwaerts
    //     (rec (type (func (param (ref 1))))) (rec (type (func)))
    for (i, ft) in c.m.typen.iter().enumerate() {
        let gruppe = c.m.gruppe_von.get(i).copied().flatten();
        for t in ft.parameter.iter().chain(ft.ergebnis.iter()) {
            sichtbar_pruefen(c, t, i, gruppe)?;
        }
    }
    // Die Sichtbarkeitsregel gilt fuer **Feldtypen** und **Elementtypen**
    // genauso wie fuer Funktions-Signaturen (Wasm 3.0 GC). Sie war vorher
    // nur an den Signaturen — und **wirkungslos**, solange
    // `sichtbar_pruefen` die untere Gruppengrenze verwarf (siehe dort).
    // Jetzt mit beiden Schranken wirksam: `type-rec.wast` Z. 51ff,
    // Minimalfall `/tmp/opencode/rec/f5.wast`.
    for (i, st) in c.m.strukt.iter().enumerate() {
        if let Some(st) = st {
            let gruppe = c.m.gruppe_von.get(i).copied().flatten();
            for (k, feld) in st.felder.iter().enumerate() {
                pruef(c, &feld.typ, &format!("Typ {i} (Feld {k})"))?;
                sichtbar_pruefen(c, &feld.typ, i, gruppe)?;
            }
        }
    }
    for (i, at) in c.m.arrays.iter().enumerate() {
        if let Some(at) = at {
            let gruppe = c.m.gruppe_von.get(i).copied().flatten();
            sichtbar_pruefen(c, &at.inhalt, i, gruppe)?;
        }
    }
    for (i, at) in c.m.arrays.iter().enumerate() {
        if let Some(at) = at {
            pruef(c, &at.inhalt, &format!("Typ {i} (Array-Element)"))?;
        }
    }
    for (i, f) in c.m.funktionen.iter().enumerate() {
        let (lokale, koerper) = match f {
            crate::modell::Func::Eigen {
                lokale, koerper, ..
            } => (lokale, koerper),
            crate::modell::Func::Importiert { .. } => continue,
        };
        for t in lokale {
            pruef(c, t, &format!("Funktion {i} (lokal)"))?;
        }
        // Auch im KOERPER: Block-/Schleifentypen, `select (result …)`,
        // `ref.null $t` und `table.grow/set`-Typen nennen Typindizes.
        //
        // GEMESSEN 02.10. (`ref.wast` Z. 9/10/12: `(block (result (ref 1))
        // (unreachable))`, `(loop …)`, `(select (result (ref 1)) …)` —
        // "unknown type").
        for (j, b) in koerper.iter().enumerate() {
            let was = format!("Funktion {i} (Befehl {j}: {b:?})");
            match b {
                Befehl::Block(art) | Befehl::Loop(art) | Befehl::If(art) => {
                    if let crate::op::Blockart::Werttyp(t) = art {
                        pruef(c, t, &was)?;
                    }
                }
                Befehl::SelectTyp(ts) => {
                    for t in ts {
                        pruef(c, t, &was)?;
                    }
                }
                Befehl::RefNull(r @ Reftyp::Konkret { .. }) => {
                    pruef(c, &Valtyp::Ref(*r), &was)?;
                }
                _ => {}
            }
        }
    }
    for (i, g) in c.m.globale.iter().enumerate() {
        pruef(c, &g.typ, &format!("Globale {i}"))?;
    }
    for (i, t) in c.m.tabellen.iter().enumerate() {
        let rt = match t {
            ImportiertTab::Importiert { typ, .. } => typ.reftyp,
            ImportiertTab::Eigen { typ, .. } => typ.reftyp,
        };
        pruef(c, &Valtyp::Ref(rt), &format!("Tabelle {i}"))?;
    }
    for (i, e) in c.m.elemente.iter().enumerate() {
        pruef(c, &Valtyp::Ref(e.reftyp()), &format!("Element {i}"))?;
    }
    Ok(())
}

/// Speichergroesse in Seiten und Mindestgroesse (64 KiB = 1 Seite).
fn pruefe_speicher_und_tabellen(c: &Ctx) -> Result<(), Fehler> {
    for s in &c.m.speicher {
        let typ = match s {
            ImportiertSpeicher::Importiert { typ, .. } => typ,
            ImportiertSpeicher::Eigen { typ } => typ,
        };
        if !typ.grenzen.passt("Speicher") {
            return Err(Fehler::ungueltig(
                V_SPEICHER_MIN,
                format!(
                    "Speicher: min {} > max {:?}",
                    typ.grenzen.min, typ.grenzen.max
                ),
            ));
        }
        // Der Speicherindexraum ist 32 Bit breit: hoechstens 65536 Seiten
        // (2^32 Byte). Das gilt fuer Minimum UND Maximum.
        // GEMESSEN 02.10. (`memory.wast` Z. 26/27/31/34: 4 Faelle).
        if typ.grenzen.min > 65_536 || typ.grenzen.max.is_some_and(|m| m > 65_536) {
            return Err(Fehler::ungueltig(
                V_SPEICHER_MIN,
                format!(
                    "Speicher: {} Seiten / max {:?} ueber 65536 (2^32 Byte)",
                    typ.grenzen.min, typ.grenzen.max
                ),
            ));
        }
    }
    for t in &c.m.tabellen {
        let typ = match t {
            ImportiertTab::Importiert { typ, .. } => typ,
            ImportiertTab::Eigen { typ, .. } => typ,
        };
        if !typ.grenzen.passt("Tabelle") {
            return Err(Fehler::ungueltig(
                V_TABELLE,
                format!(
                    "Tabelle: min {} > max {:?}",
                    typ.grenzen.min, typ.grenzen.max
                ),
            ));
        }
        // Der Indexraum einer Tabelle ist 32 Bit breit: groessere Grenzen
        // sind ungueltig (Spec: „table size must be at most 2^32-1").
        // GEMESSEN 02.10. (`table.wast`: `(table 0x1_0000_0000 funcref)`).
        if typ.grenzen.min > u32::MAX as u64 || typ.grenzen.max.is_some_and(|m| m > u32::MAX as u64)
        {
            return Err(Fehler::ungueltig(
                V_TABELLE,
                format!(
                    "Tabelle: Groesse {} / {:?} ueber der Grenze {}",
                    typ.grenzen.min,
                    typ.grenzen.max,
                    u32::MAX
                ),
            ));
        }
    }
    Ok(())
}

/// Globale: eine importierte globale Variable darf `mut` sein — die
/// Einschraenkung „unveraenderlich" galt nur fuer WebAssembly 1.0. Eigene
/// Startwerte muessen konstante Ausdruecke des passenden Typs sein.
///
/// GEMESSEN 02.10. (`imports.wast` Z. 63/64/65: 3 Faelle): mit der alten
/// Regel wurde `(import "test" "global-mut-i64" (global (mut i64)))`
/// abgelehnt, obwohl es gueltig ist.
fn pruefe_globale(c: &Ctx) -> Result<(), Fehler> {
    let m = c.m;
    for (i, glob) in m.globale.iter().enumerate() {
        if let Some((_, feld)) = &glob.import {
            // Nur der TYP des Imports wird hier geprueft (die Beschraenkung
            // auf `var` entfaellt); `V_IMPORT_GLOBAL` bleibt fuer andere
            // Faelle (z. B. geteilte Variablen) in Gebrauch.
            let _ = (i, feld, glob.veraenderbar);
        }
    }
    Ok(())
}

/// Tabellen (Wasm 3.0): der Startwert muss ein konstanter Ausdruck vom
/// Tabellentyp sein; eine nicht-leerbare Tabelle OHNE Startwert ist
/// ungueltig (der Vorgabewert waere `null`).
///
/// GEMESSEN 02.10. (`table.wast` Z. 13/29-31/40-42: 7 Faelle).
fn pruefe_tabellen(c: &Ctx) -> Result<(), Fehler> {
    for (i, t) in c.m.tabellen.iter().enumerate() {
        let ImportiertTab::Eigen { typ, startwert } = t else {
            continue;
        };
        match startwert {
            Some(code) => {
                pruefe_konstanten(
                    c,
                    code,
                    Valtyp::Ref(typ.reftyp),
                    &format!("Tabelle {i}"),
                    c.m.globale.len(),
                )?;
                // Der STARWERT einer Tabelle darf nur eine IMPORTIERTE
                // globale Variable nennen — bei Daten- und Elementversaetzen
                // erlaubt die Suite auch eigene Variablen (`data.wast`
                // Z. 88/89), beim Tabellenstartwert nicht.
                //
                // GEMESSEN 03.10. (`global.wast` Z. 118ff: 1
                // `assert_invalid` mit „unknown global").
                for b2 in code {
                    if let crate::op::Befehl::GlobalGet(g) = b2 {
                        if !c
                            .m
                            .globale
                            .get(*g as usize)
                            .is_some_and(|x| x.import.is_some())
                        {
                            return Err(Fehler::ungueltig(
                                V_TABELLE,
                                format!(
                                    "Tabelle {i}: global.get {g} ist keine importierte Variable"
                                ),
                            ));
                        }
                    }
                }
            }
            None => {
                if !typ.reftyp.ist_nullable() {
                    return Err(Fehler::ungueltig(
                        V_TABELLE,
                        format!(
                            "Tabelle {i}: {} laesst kein `null` zu, hat aber keinen Startwert",
                            typ.reftyp.text()
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Konstante Ausdruecke (Global-Start, Element-Offset, Daten-Offset,
/// Element-Eintraege als Ausdrucksliste).
/// Engmasse fuer den **Startwert einer globalen Variablen**: dort genuegt
/// beim Muster `ref.func $f end` die **strukturelle** Gleichheit der Typen.
/// **Nicht ausgeliefert** — gemessen zu permissiv, siehe Aufrufstelle.
///
/// GEMESSEN 03.10. (`type-subtyping.wast` ab Z. 90ff, Modul `s8`):
///
/// ```text
///
///     (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
///     (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
///     (rec (type $g1 (sub $f1 (func))) (type (struct)))
///     (rec (type $g2 (sub $f2 (func))) (type (struct)))
///     (func $g (type $g2))
///     (global (ref $g1) (ref.func $g))
///
/// ```
///
/// `$g2` muss zu `$g1` passen. Die `(sub …)`-Ketten fuehren auf `$f2` bzw.
/// `$f1`; die beiden sind strukturell gleich (beide `(func)`), aber
/// verschiedene Indizes. Der allgemeine Kettenvergleich darf dafür NICHT
/// strukturell werden — zwei Varianten (`BERICHT_WASM_SUBKETTE.txt`) kosteten
/// **14** bzw. **4** Faelle. Hier, an genau einer Stelle, ist es gemessen.
#[allow(dead_code)] // GEMESSEN zweimal zu permissiv (34981 und 34996 statt 34983/34998) — Beleg.
fn glob_start_ref_func_gleich(c: &Ctx, code: &[crate::op::Befehl], soll: Valtyp) -> bool {
    let soll_ref = match soll {
        Valtyp::Ref(r) => r,
        _ => return false,
    };
    let mut rest = code;
    if let Some(crate::op::Befehl::End) = rest.last() {
        rest = &rest[..rest.len() - 1];
    }
    let [crate::op::Befehl::RefFunc(f)] = rest else {
        return false;
    };
    if (*f as usize) >= c.m.funktionen.len() {
        return false;
    }
    let haben = funktion_reftyp(c, *f);
    let (
        Reftyp::Konkret {
            typ: ta,
            nullable: na,
        },
        Reftyp::Konkret {
            typ: tb,
            nullable: nb,
        },
    ) = (haben, soll_ref)
    else {
        return false;
    };
    na == nb && crate::modell::typen_gleich(c.m, ta, tb)
}
fn pruefe_const_ausdruecke(c: &Ctx) -> Result<(), Fehler> {
    for (i, glob) in c.m.globale.iter().enumerate() {
        let code = match &glob.start {
            Some(code) => code,
            None => continue,
        };
        // GEMESSEN 03.10., **zweimal** zurueckgenommen:
        // `glob_start_ref_func_gleich` (Beleg unten) waere die Engmasse fuer
        // den **Startwert einer globalen Variablen** — beim Muster
        // `ref.func $f end` genuege die strukturelle Gleichheit der Typen.
        //   1. Messung (34983): 34981, **-2** (`type-rec.wast` 10 -> 8)
        //   2. Messung (34998, mit Signatur-Regel und Laufzeitkette): 34996,
        //      **-2** (`type-rec.wast` 11 -> 9), **+0** in
        //      `type-subtyping.wast`
        // Sie nimmt genau die Faelle an, die die Suite verwirft: dort steht
        // ein `ref.func` mit **unverwandter** Typkette unter einem
        // strukturell gleichen Typ. Der Aufruf bleibt aus.
        pruefe_konstanten(c, code, glob.typ, &format!("Globale {i}"), i)?;
    }
    for (i, e) in c.m.elemente.iter().enumerate() {
        if let Element::Aktiv { offset, reftyp, .. } = e {
            pruefe_konstanten(
                c,
                offset,
                Valtyp::I32,
                &format!("Element {i} (Versatz)"),
                c.m.globale.len(),
            )?;
            let _ = reftyp;
        }
        // Erwarteter Typ je Eintrag ist der TYP DES ABSCHNITTS — bei einem
        // passiven oder deklarativen Abschnitt also auch dessen Typ (nicht
        // immer `funcref`; Wasm 3.0 erlaubt `(ref null $t)`).
        //
        // GEMESSEN 02.10. (`br_table.wast`: `(elem $table (ref null $t)
        // (ref.func $tf))`).
        for (j, aus) in e.eintraege().iter().enumerate() {
            pruefe_konstanten(
                c,
                aus,
                Valtyp::Ref(e.reftyp()),
                &format!("Element {i} Eintrag {j}"),
                c.m.globale.len(),
            )?;
        }
    }
    for (i, d) in c.m.daten.iter().enumerate() {
        if let Daten::Aktiv { offset, .. } = d {
            pruefe_konstanten(
                c,
                offset,
                Valtyp::I32,
                &format!("Datenabschnitt {i} (Versatz)"),
                c.m.globale.len(),
            )?;
        }
    }
    Ok(())
}

/// Ein konstanter Ausdruck: eine const-Instruktion oder `global.get` einer
/// importierten *unveraenderlichen* globalen, gefolgt von `end`.
/// `vorher` = Anzahl der globalen Variablen, die BEI diesem Ausdruck
/// schon bekannt sind (fuer Startwerte: bis zu dieser Variablen; fuer
/// Element-/Datenversatz: alle). Ein `global.get` darf nur darauf zeigen
/// — kein VORRUECKVERWEIS.
///
/// GEMESSEN 03.10. (`global.wast`, 4 `assert_invalid` mit „unknown
/// global"/„constant expression required": die Module wurden erst beim
/// Instanziieren abgelehnt, nicht beim Pruefen).
fn pruefe_konstanten(
    c: &Ctx,
    code: &[Befehl],
    erwartet: Valtyp,
    was: &str,
    vorher: usize,
) -> Result<(), Fehler> {
    // Konstante Ausdruecke werden mit einem eigenen, flachen Stapel
    // geprueft (die Regeln stehen in der Spezifikation unter
    // „Constant Expressions").
    let mut s = Vec::<Valtyp>::new();
    // `ref.null` in einen Typ, der `null` nicht erlaubt, ist ein
    // Typfehler (Wasm 3.0, nicht-leere Referenztypen). GEMESSEN 02.10.
    // (`table.wast` Z. 28-31: `(table 1 (ref func) (ref.null func))`).
    let mut hat_null = false;
    for (i, b) in code.iter().enumerate() {
        match b {
            Befehl::End => {
                if i + 1 != code.len() {
                    return Err(Fehler::ungueltig(
                        V_END,
                        format!("{was}: Befehl nach dem abschliessenden end"),
                    ));
                }
            }
            Befehl::I32Konst(_) => s.push(Valtyp::I32),
            Befehl::I64Konst(_) => s.push(Valtyp::I64),
            Befehl::F32Konst(_) => s.push(Valtyp::F32),
            Befehl::F64Konst(_) => s.push(Valtyp::F64),
            Befehl::RefNull(r) => {
                hat_null = true;
                s.push(Valtyp::Ref(*r));
            }
            Befehl::RefFunc(f) => {
                if (*f as usize) >= c.m.funktionen.len() {
                    return Err(Fehler::ungueltig(
                        V_FUNKTION,
                        format!("{was}: ref.func {f}"),
                    ));
                }
                // Der Wert hat den KONKRETEN Typ der Funktion, nicht nur
                // `funcref`: `(ref.func $f)` passt in `(ref null $sig)`
                // (Wasm 3.0, Typed Function References).
                s.push(Valtyp::Ref(funktion_reftyp(c, *f)));
            }
            // ---- GC-Werte als konstante Ausdruecke ---------------------
            // `extern.convert_any` / `any.convert_extern` sind im
            // Startwert einer globalen Variablen erlaubt.
            //
            // GEMESSEN 03.10. (`extern.wast` Z. 6/7).
            Befehl::ExternConvertAny => {
                s.pop();
                s.push(Valtyp::Ref(Reftyp::Extern));
            }
            Befehl::AnyConvertExtern => {
                s.pop();
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: crate::modell::JEDER_TYP,
                    nullable: false,
                }));
            }
            Befehl::I31GetS | Befehl::I31GetU => {
                s.pop();
                s.push(Valtyp::I32);
            }
            Befehl::RefTest(_) => {
                s.pop();
                s.push(Valtyp::I32);
            }
            Befehl::RefCast(r) => {
                s.pop();
                s.push(Valtyp::Ref(*r));
            }
            Befehl::RefI31 => {
                if !zwei_von(&mut s, Valtyp::I32, "ref.i31")? {
                    return Ok(());
                }
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: crate::modell::JEDER_I31TYP,
                    nullable: false,
                }));
            }
            // `struct.new` ist ebenfalls ein konstanter Ausdruck: so
            // steht es in `struct.wast` im Startwert einer globalen
            // Variablen (GEMESSEN 03.10.: „Globale 0: StructNew(0) ist
            // kein konstanter Ausdruck").
            Befehl::StructNew(t) => {
                let felder = struct_typ_von(c.m, *t)?.feld_typen();
                for (i, f) in felder.iter().enumerate().rev() {
                    if s.pop() != Some(*f) {
                        return Err(Fehler::ungueltig(
                            V_TYP_OPERAND,
                            format!("struct.new-Feld {i}: {was} erwartet den Typ des Feldes"),
                        ));
                    }
                }
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: *t,
                    nullable: false,
                }));
            }
            // `array.new_fixed` ist ebenfalls konstanter Ausdruck.
            //
            // GEMESSEN 03.10. (`array.wast` #13: „ArrayNewFixed(0, 2) ist
            // kein konstanter Ausdruck" — im Startwert einer globalen
            // Variablen und in einem Elementabschnitt).
            Befehl::ArrayNewFixed(t, n) => {
                let inhalt = array_typ_von(c.m, *t)?.inhalt;
                for _ in 0..*n {
                    if s.pop() != Some(inhalt) {
                        return Err(Fehler::ungueltig(
                            V_TYP_OPERAND,
                            format!("{was}: array.new_fixed erwartet den Elementtyp"),
                        ));
                    }
                }
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: *t,
                    nullable: false,
                }));
            }
            // `array.new` / `array.new_default` sind konstante Ausdruecke
            // (Wasm 3.0 GC) — Elementabschnitte und globale Startwerte
            // duerfen damit gefuellt werden.
            //
            // GEMESSEN 03.10. (`table_init.wast` Z. 790, `array.wast`
            // Z. 64).
            Befehl::ArrayNew(t) | Befehl::ArrayNewDefault(t) => {
                if !crate::modell::ist_array_typ(c.m, *t) {
                    return Err(Fehler::ungueltig(
                        V_TYP,
                        format!("{was}: array.new — Typ {t} ist kein Arraytyp"),
                    ));
                }
                // `[t i32]`: erst die Laenge (oben), dann der Wert.
                if s.pop() != Some(Valtyp::I32) {
                    return Err(Fehler::ungueltig(
                        V_TYP_OPERAND,
                        format!("{was}: array.new erwartet i32 (Laenge)"),
                    ));
                }
                if matches!(b, Befehl::ArrayNew(_)) {
                    s.pop();
                }
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: *t,
                    nullable: false,
                }));
            }
            Befehl::StructNewDefault(t) => {
                if !crate::modell::ist_struct_typ(c.m, *t) {
                    return Err(Fehler::ungueltig(
                        V_TYP_BLOCK,
                        format!("struct.new_default: Typ {t} ist kein Strukturtyp"),
                    ));
                }
                s.push(Valtyp::Ref(Reftyp::Konkret {
                    typ: *t,
                    nullable: false,
                }));
            }
            Befehl::GlobalGet(idx) => {
                // Konstanter Ausdruck: `global.get` einer globalen Variablen,
                // die NICHT veraenderbar ist. Eigene Globale sind erlaubt
                // (die Spezifikation nimmt jede `const`-deklarierte Variable,
                // importiert oder im Modul definiert).
                //
                // GEMESSEN 02.10. (`data.wast` Z. 89/90): mit der alten Regel
                // „nur Importe“ wurden zwei gueltige Module abgelehnt.
                // Kein VORRUECKVERWEIS: `global.get` darf nur auf eine
                // globale Variable zeigen, die vor diesem Ausdruck stand.
                //
                // GEMESSEN 03.10. (`global.wast` Z. 358-371: 4
                // `assert_invalid`).
                if (*idx as usize) >= vorher {
                    return Err(Fehler::ungueltig(
                        V_GLOBAL,
                        format!(
                            "{was}: global.get {idx} — unbekannt (nur {vorher} globale Variablen vorher deklariert)"
                        ),
                    ));
                }
                let glob = c.m.globale.get(*idx as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_GLOBAL, format!("{was}: global.get {idx}"))
                })?;
                if glob.veraenderbar {
                    let art = if glob.import.is_some() {
                        "importierte"
                    } else {
                        "eigene"
                    };
                    return Err(Fehler::ungueltig(
                        V_GLOBAL,
                        format!("{was}: global.get {idx} auf veraenderbare {art} Variable"),
                    ));
                }
                s.push(glob.typ);
            }
            // Wasm 2.0 erlaubt in konstanten Ausdruecken neben `const`,
            // `global.get`, `ref.null` und `ref.func` auch Ganzzahl-
            // addition, -subtraktion und -multiplikation („extended constant
            // expressions"). GEMESSEN 02.10. (`elem.wast` Z. ab 139:
            // 8 Faelle mit `(offset (i32.add (i32.const 1) (i32.const 2)))`).
            Befehl::I32Add | Befehl::I32Sub | Befehl::I32Mul => {
                if !zwei_von(&mut s, Valtyp::I32, was)? {
                    return Ok(());
                }
                s.push(Valtyp::I32);
            }
            Befehl::I64Add | Befehl::I64Sub | Befehl::I64Mul => {
                if !zwei_von(&mut s, Valtyp::I64, was)? {
                    return Ok(());
                }
                s.push(Valtyp::I64);
            }
            anderer => {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("{was}: {anderer:?} ist kein konstanter Ausdruck"),
                ));
            }
        }
    }
    if s.len() != 1 {
        return Err(Fehler::ungueltig(
            V_TYP_OPERAND,
            format!(
                "{was}: {} Werte statt einem ({} erwartet)",
                s.len(),
                erwartet.text()
            ),
        ));
    }
    let t = s[0];
    if hat_null && !erwartet.ist_nullable() {
        return Err(Fehler::ungueltig(
            V_TYP_OPERAND,
            format!(
                "{was}: `ref.null` passt nicht in {} (null ist dort nicht erlaubt)",
                erwartet.text()
            ),
        ));
    }
    if crate::modell::valtyp_passt_als(c.m, &t, &erwartet) {
        Ok(())
    } else {
        Err(Fehler::ungueltig(
            V_GLOBAL,
            format!("{was}: Typ {t:?} statt {erwartet:?}"),
        ))
    }
}

/// Zwei Werte vom Typ `t` vom Stapel nehmen. `false` heisst: der Stapel
/// war an dieser Stelle bereits polymorph (unreachable) — dann gibt es
/// nichts zu pruefen, und der Aufrufer beendet die Pruefung mit `Ok(())`.
fn zwei_von(s: &mut Vec<Valtyp>, t: Valtyp, was: &str) -> Result<bool, Fehler> {
    for _ in 0..2 {
        match s.pop() {
            None => return Ok(false),
            Some(v) if v == t => {}
            Some(v) => {
                return Err(Fehler::ungueltig(
                    V_TYP_OPERAND,
                    format!("{was}: {} erwartet, {} auf dem Stapel", t.text(), v.text()),
                ));
            }
        }
    }
    Ok(true)
}

/// Der Strukturtyp hinter einem Typindex — muss einer sein.
fn struct_typ_von(m: &crate::modell::Modul, t: u32) -> Result<&crate::modell::StructTyp, Fehler> {
    crate::modell::struct_typ(m, t)
        .ok_or_else(|| Fehler::ungueltig(V_TYP_BLOCK, format!("Typ {t} ist kein Strukturtyp")))
}

/// Das Feld `f` des Strukturtyps `t` — muss existieren.
fn struct_feld_von<'a>(
    st: &'a crate::modell::StructTyp,
    t: u32,
    f: u32,
    befehl: &str,
) -> Result<&'a crate::modell::StructFeld, Fehler> {
    st.feld(f).ok_or_else(|| {
        Fehler::ungueltig(
            V_TYP_BLOCK,
            format!(
                "{befehl}: Typ {t} hat {} Felder, Feld {f} gibt es nicht",
                st.felder.len()
            ),
        )
    })
}

/// Das Feld `f` des Strukturtyps `t`.
fn feld_typ<'a>(
    m: &'a crate::modell::Modul,
    t: u32,
    f: u32,
    befehl: &str,
) -> Result<&'a crate::modell::StructFeld, Fehler> {
    struct_feld_von(struct_typ_von(m, t)?, t, f, befehl)
}

/// Das Feld `f` mit seiner Veraenderbarkeit — `(struct (mut …) …)` gilt
/// fuer alle Felder des Typs.
fn feld_eigenschaft(
    m: &crate::modell::Modul,
    t: u32,
    f: u32,
    befehl: &str,
) -> Result<(Valtyp, bool), Fehler> {
    let st = struct_typ_von(m, t)?;
    let feld = struct_feld_von(st, t, f, befehl)?;
    Ok((feld.typ, feld.veraenderbar || st.veraenderbar))
}

/// Der Typ des Elementabschnitts `e` muss zum Elementtyp `wollen` des
/// Arrays passen (oder ein Subtyp davon sein).
///
/// GEMESSEN 03.10. (`array_init_elem.wast` Z. 17ff: „type mismatch" bei
/// `(array (mut i8))` und einem `funcref`-Abschnitt).
fn elem_typ_pruefen(c: &Ctx, e: u32, wollen: Valtyp, befehl: &str) -> Result<(), Fehler> {
    let abschnitt = match c.m.elemente.get(e as usize) {
        Some(el) => el,
        None => {
            return Err(Fehler::ungueltig(
                V_TYP_BLOCK,
                format!("{befehl}: Elementabschnitt {e} gibt es nicht"),
            ));
        }
    };
    let ist = abschnitt.reftyp();
    // Die Werte des Abschnitts MUESSEN in den Array passen: `(ref null X)`
    // nimmt `X` auf, der Abschnitt liefert `Valtyp::Ref(ist)`.
    let ziel = Valtyp::Ref(ist);
    if !crate::modell::valtyp_passt_als(c.m, &ziel, &wollen) {
        return Err(Fehler::ungueltig(
            V_TYP_BLOCK,
            format!("{befehl}: type mismatch (Elementtyp {wollen:?}, Abschnitt {ist:?})"),
        ));
    }
    Ok(())
}

/// Darf der Typ `t` in der Signatur von Typindex `i` stehen?
/// (`gruppe` = die `(rec …)`-Gruppe von `i`, `None` ausserhalb)
///
/// GEMESSEN 03.10. (`type-rec.wast`: 2x „unknown type").
fn sichtbar_pruefen(
    _c: &Ctx,
    t: &Valtyp,
    i: usize,
    gruppe: Option<(u32, u32)>,
) -> Result<(), Fehler> {
    if let Valtyp::Ref(Reftyp::Konkret { typ, .. }) = t {
        if crate::modell::ist_wildcard(*typ) {
            return Ok(());
        }
        // GEMESSEN 03.10. (`type-rec.wast` Z. 3ff, erster Baustein):
        //   (type (func (param (ref 0)) (result (ref 0))))
        // ist **gueltig** — ausserhalb jeder Gruppe darf ein Typ sich
        // **selbst** nennen. Die Grenze `0..i` verwarf genau das.
        let (was, grenze) = match gruppe {
            Some((anfang, ende)) => (anfang, ende),
            // Ausserhalb einer Gruppe: alle Typen bis **einschliesslich**
            // der eigene Index.
            None => (i as u32, i as u32 + 1),
        };
        // Nur die **obere** Schranke. GEMESSEN 03.10.
        // (`type-subtyping.wast`, Modul aus Z. 54ff): ein Gruppenmitglied
        // darf ausdruecklich auf **frueher**e Typen zeigen — auch auf solche
        // einer anderen Gruppe:
        //
        //   (rec (type $a1 …) (type $a2 …))          ;; Gruppe 1
        //   (rec (type $b1 (sub $a2 …)) …)            ;; Gruppe 2, $a2 ist sichtbar
        //
        // Eine zusaetzliche **untere** Schranke (die ich kurz eingebaut hatte)
        // verwarf genau das: „Typ 2: (ref … 0) zeigt auf einen nicht
        // sichtbaren Typ" bei einem Modul, das die Suite als gueltig fuehrt.
        // Die untere Schranke ist damit **widerlegt**, nicht nur unbequem.
        let _ = was;
        if *typ >= grenze {
            return Err(Fehler::ungueltig(
                V_TYP,
                format!(
                    "Typ {i}: (ref … {typ}) zeigt auf einen nicht sichtbaren \
                     Typ (erlaubt ist 0..{grenze}) — unknown type"
                ),
            ));
        }
    }
    Ok(())
}

fn struct_reftyp() -> Reftyp {
    Reftyp::Konkret {
        typ: crate::modell::JEDER_STRUKTYP,
        nullable: true,
    }
}

/// Der Arraytyp hinter einem Typindex — muss einer sein.
fn array_typ_von(m: &crate::modell::Modul, t: u32) -> Result<&crate::modell::ArrayTyp, Fehler> {
    crate::modell::array_typ(m, t)
        .ok_or_else(|| Fehler::ungueltig(V_TYP_BLOCK, format!("Typ {t} ist kein Arraytyp")))
}

/// `(ref null array)` — der Typ eines Arraywerts im Stack.
fn array_reftyp() -> Reftyp {
    Reftyp::Konkret {
        typ: crate::modell::JEDER_ARRAYTYP,
        nullable: true,
    }
}

/// `return_call x : t1* ok`: die ERGEBNISSE der Zielfunktion muessen zum
/// Ergebnis des aufrufenden Rahmens passen — sie werden ja dessen
/// Ergebnis.
///
/// GEMESSEN 03.10. (`return_call_ref.wast` Z. 227ff: eine aufrufende
/// Funktion mit `(result (ref $t))` und ein Ziel mit
/// `(result (ref null $t))` muss „type mismatch" sein; ohne diese
/// Pruefung wurde das Modul akzeptiert).
fn ziel_ergebnisse_passen(c: &Ctx, ft: &FuncTyp, ziel: &FuncTyp, was: &str) -> Result<(), Fehler> {
    if ziel.ergebnis.len() != ft.ergebnis.len() {
        return Err(Fehler::ungueltig(
            V_TYP_OPERAND,
            format!(
                "{was}: Ziel hat {} Ergebnisse, der aufrufende Rahmen {}",
                ziel.ergebnis.len(),
                ft.ergebnis.len()
            ),
        ));
    }
    for (i, (a, b)) in ft.ergebnis.iter().zip(&ziel.ergebnis).enumerate() {
        if !crate::modell::valtyp_passt_als(c.m, b, a) {
            return Err(Fehler::ungueltig(
                V_TYP_OPERAND,
                format!(
                    "{was}: Ergebnis {i}: {} erwartet, Ziel liefert {}",
                    a.text(),
                    b.text()
                ),
            ));
        }
    }
    Ok(())
}

/// Hat der Werttyp keinen Standardwert? Nur eine Referenz OHNE `null`
/// (`(ref $t)`, `(ref func)`, `(ref extern)`) — alle anderen Typen
/// starten bei 0/`null`.
///
/// GEMESSEN 03.10. (`local_init.wast`; ein erster Versuch mit
/// `!ist_nullable()` verwarf 13 Dateien, weil `i32` nicht nullable ist).
fn ist_referenz_ohne_null(t: &Valtyp) -> bool {
    match t {
        Valtyp::Ref(r) => !r.ist_nullable(),
        _ => false,
    }
}

/// Der Referenztyp, den `ref.func f` erzeugt: `(ref $sig)` mit dem
/// Typindex der Funktionssignatur (Wasm 3.0).
fn funktion_reftyp(c: &Ctx, f: u32) -> Reftyp {
    let sig = match c.m.funktionen.get(f as usize) {
        Some(crate::modell::Func::Eigen { typ, .. })
        | Some(crate::modell::Func::Importiert { typ, .. }) => *typ,
        None => return Reftyp::Func,
    };
    Reftyp::Konkret {
        typ: sig,
        nullable: false,
    }
}
// ---------------------------------------------------------------------------
// Funktionen: der eigentliche Typchecker
//
// Der Algorithmus folgt der Referenzformel der Spezifikation (Anhang 3.1):
// ein Wertstapel mit `Option<Valtyp>` (`None` = unbekannt, der Zustand nach
// `unreachable`) und ein Stapel von Kontrollrahmen. `pop_ctrl` verlangt
// EXAKT die Rahmenhoehe — damit fallen zusaetzliche Werte sofort auf.
// ---------------------------------------------------------------------------

/// Ein Wert auf dem Operandstapel: `None` heisst „unbekannt" (polymorph).
type W = Option<Valtyp>;

/// Kontrollrahmen (`block`, `loop`, `if`, `else`, Funktion).
#[derive(Clone)]
struct Rahmen {
    /// Position im Koerper (fuer Fehlermeldungen).
    start: usize,
    art: Rahmenart,
    /// Blockparameter (bei `loop` zugleich der Label-Typ).
    parameter: Vec<Valtyp>,
    /// Blockergebnisse (bei `block`/`if` zugleich der Label-Typ).
    ergebnis: Vec<Valtyp>,
    /// Operandstapel-Hoehe beim Betreten.
    hoehe: usize,
    /// Nach `unreachable`/`br` ist der Typzustand „unbekannt".
    unerreichbar: bool,
    /// Je lokaler Variable: ist eine Referenz OHNE Standardwert gesetzt?
    ///
    /// GEMESSEN 03.10. (`local_init.wast`): `(local $x (ref extern))`
    /// startet auf `false`; `local.get` ist dann ein Fehler.
    sicher: Vec<bool>,
    /// Derselbe Satz beim BETRETEN des Rahmens. Ein `else`-Zweig sieht
    /// den Zustand vor dem `if` (nicht den des `then`-Zweigs), und beim
    /// Verlassen eines `if`/`loop` gilt wieder der Eintrittszustand
    /// (konservative Fixpunktbildung, wie die Referenzformel der
    /// Spezifikation).
    sicher_ein: Vec<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rahmenart {
    Block,
    Loop,
    If,
    Else,
    Funktion,
}

/// Operandstapel + Kontrollrahmen (die „Typmaschine").
///
/// `m` ist das geprueefte Modul: Referenztypen mit Typindex werden
/// STRUKTURELL verglichen (Wasm 3.0, Typed Function References).
struct OpStapel<'a> {
    werte: Vec<W>,
    /// Parallel zu `werte`: kam der Wert aus einem `ref.null`?
    ///
    /// GEMESSEN 03.10.: der Wert `null` passt in **jeden** Referenztyp —
    /// `(func (result anyref) (ref.null func))` ist gueltig
    /// (`ref_null.wast` #6, `unreached-valid.wast` #12). Fuer den
    /// Signatur-Vergleich (`valtyp_passt_als_signatur`) ist aber gerade
    /// `func` -> `any` die Kombination, die die Suite in
    /// `type-subtyping.wast` Z. 227ff verbietet. Der Unterschied ist der
    /// Wert, nicht sein Typ — deshalb dieses Feld.
    null_werte: Vec<bool>,
    rahmen: Vec<Rahmen>,
    m: &'a Modul,
}

impl<'a> OpStapel<'a> {
    fn neue_funktion(typ: &FuncTyp, start: usize, m: &'a Modul) -> OpStapel<'a> {
        // Parameter sind IMMER gesetzt; die lokalen Variablen kommen in
        // `pruefe_funktionen` dazu (sie stehen dort zusammen mit dem Typ).
        let n_lokale = typ.parameter.len();
        OpStapel {
            m,
            werte: Vec::new(),
            null_werte: Vec::new(),
            rahmen: vec![Rahmen {
                start,
                art: Rahmenart::Funktion,
                parameter: typ.parameter.clone(),
                ergebnis: typ.ergebnis.clone(),
                hoehe: 0,
                unerreichbar: false,
                sicher: vec![true; n_lokale],
                sicher_ein: vec![true; n_lokale],
            }],
        }
    }

    /// Den Startzustand einer Funktion auf `n` lokale Variablen bringen
    /// (Parameter gesetzt, eigene Variablen mit Standardwert gesetzt).
    fn setze_lokale(&mut self, n_parameter: usize, standard: &[Valtyp]) {
        if let Some(r) = self.rahmen.last_mut() {
            r.sicher = (0..n_parameter + standard.len())
                .map(|i| {
                    if i < n_parameter {
                        return true; // Parameter sind immer gesetzt
                    }
                    standard
                        .get(i - n_parameter)
                        // NUR eine Referenz OHNE `null` hat keinen
                        // Standardwert; Zahlen und `null`-Referenzen
                        // starten bei ihrem Standardwert.
                        .is_none_or(|t| !ist_referenz_ohne_null(t))
                })
                .collect();
            r.sicher_ein = r.sicher.clone();
        }
    }

    fn hoehe(&self) -> usize {
        self.werte.len()
    }

    /// Der Zustand des obersten Rahmens (Satz gesetzter Variablen).
    fn sicher(&self) -> &[bool] {
        self.rahmen
            .last()
            .map(|r| r.sicher.as_slice())
            .unwrap_or(&[])
    }

    /// Einen Index als gesetzt markieren.
    fn markiere(&mut self, l: u32) {
        if let Some(r) = self.rahmen.last_mut() {
            if let Some(stelle) = r.sicher.get_mut(l as usize) {
                *stelle = true;
            }
        }
    }

    /// Zwei Zustaende schneiden (beide Zweige eines `if`).
    fn vereinige_als_schnitt(&mut self, mit: &[bool]) {
        if let Some(r) = self.rahmen.last_mut() {
            for (a, b) in r.sicher.iter_mut().zip(mit.iter()) {
                *a = *a && *b;
            }
        }
    }
    fn push(&mut self, t: Valtyp) {
        self.werte.push(Some(t));
        self.null_werte.push(false);
    }
    /// Wie `push`, aber der Wert ist ein `ref.null`.
    fn push_null(&mut self, t: Valtyp) {
        self.werte.push(Some(t));
        self.null_werte.push(true);
    }
    fn push_w(&mut self, w: W) {
        self.null_werte.push(false);
        self.werte.push(w);
    }
    fn pop(&mut self) -> Result<W, Fehler> {
        let hoehe = self.rahmen.last().map(|r| r.hoehe).unwrap_or(0);
        let unerreichbar = self.rahmen.last().map(|r| r.unerreichbar).unwrap_or(false);
        if self.werte.len() == hoehe {
            if unerreichbar {
                return Ok(None);
            }
            return Err(Fehler::ungueltig(
                V_STAPEL,
                format!(
                    "Stapelunterlauf: {} Werte auf dem Stapel, einer wird gebraucht",
                    self.werte.len()
                ),
            ));
        }
        let w = self.werte.pop().flatten();
        self.null_werte.pop();
        Ok(w)
    }

    /// Wie `pop`, liefert aber zusaetzlich „stammte der Wert aus einem
    /// `ref.null`?".
    fn pop_mit_null(&mut self) -> Result<(W, bool), Fehler> {
        let hoehe = self.rahmen.last().map(|r| r.hoehe).unwrap_or(0);
        let unerreichbar = self.rahmen.last().map(|r| r.unerreichbar).unwrap_or(false);
        if self.werte.len() == hoehe {
            if unerreichbar {
                self.null_werte.pop();
                return Ok((None, false));
            }
            return Err(Fehler::ungueltig(
                V_STAPEL,
                format!(
                    "Stapelunterlauf: {} Werte auf dem Stapel, einer wird gebraucht",
                    self.werte.len()
                ),
            ));
        }
        let w = self.werte.pop().flatten();
        let ist_null = self.null_werte.pop().unwrap_or(false);
        Ok((w, ist_null))
    }
    fn pop_t(&mut self, t: Valtyp, was: &str) -> Result<(), Fehler> {
        match self.pop()? {
            None => Ok(()), // polymorph: passt auf alles
            Some(v) if crate::modell::valtyp_passt_als(self.m, &v, &t) => Ok(()),
            Some(v) => Err(Fehler::ungueltig(
                V_TYP_OPERAND,
                format!("{was}: {} erwartet, {} auf dem Stapel", t.text(), v.text()),
            )),
        }
    }
    fn pop_ref(&mut self, r: Reftyp, was: &str) -> Result<(), Fehler> {
        match self.pop()? {
            None => Ok(()),
            Some(Valtyp::Ref(v)) if crate::modell::reftyp_passt_als(self.m, &v, &r) => Ok(()),
            Some(v) => Err(Fehler::ungueltig(
                V_TYP_OPERAND,
                format!("{was}: {} erwartet, {} auf dem Stapel", r.text(), v.text()),
            )),
        }
    }
    fn pop_v(&mut self, typen: &[Valtyp], was: &str) -> Result<(), Fehler> {
        for (i, t) in typen.iter().enumerate().rev() {
            self.pop_t(*t, &format!("{was}[{i}]"))?;
        }
        Ok(())
    }

    /// Wie `pop_v`, aber mit dem **Signatur**-Vergleich: zwischen zwei
    /// abstrakten Heap-Typen gilt nur die Gleichheit
    /// (`valtyp_passt_als_signatur`).
    ///
    /// Verwendet **nur** am Funktionsrahmen (`end` der Funktion) und bei
    /// `return` — also dort, wo ein Rumpfwert gegen eine *Signatur* laeuft.
    fn pop_v_signatur(&mut self, typen: &[Valtyp], was: &str) -> Result<(), Fehler> {
        for (i, t) in typen.iter().enumerate().rev() {
            let (v, ist_null) = self.pop_mit_null()?;
            let Some(v) = v else {
                continue;
            };
            // `null` passt in jeden Referenztyp — der Signatur-Vergleich
            // fuer abstrakte Heap-Typen gilt nur fuer Werte, die nicht aus
            // einem `ref.null` stammen (GEMESSEN: `ref_null.wast` #6).
            let passt = ist_null || crate::modell::valtyp_passt_als_signatur(self.m, &v, t);
            if !passt {
                return Err(Fehler::ungueltig(
                    V_TYP_OPERAND,
                    format!(
                        "{was}[{i}]: {} erwartet, {} auf dem Stapel",
                        t.text(),
                        v.text()
                    ),
                ));
            }
        }
        Ok(())
    }
    fn push_v(&mut self, typen: &[Valtyp]) {
        for t in typen {
            self.push(*t);
        }
    }
    /// `unreachable()` der Referenz: Stapel auf Rahmenhoehe, Zustand merken.
    fn unerreichbar(&mut self) {
        if let Some(r) = self.rahmen.last_mut() {
            r.unerreichbar = true;
            // Nach `unreachable` ist jede Variable gesetzt (der Code danach
            // laeuft nicht).
            for stelle in r.sicher.iter_mut() {
                *stelle = true;
            }
            let h = r.hoehe;
            self.werte.truncate(h);
            self.null_werte.truncate(h);
        }
    }
    /// `push_ctrl` der Referenz.
    fn push_ctrl(&mut self, art: Rahmenart, start: usize, p: Vec<Valtyp>, e: Vec<Valtyp>) {
        // Der Initialisierungszustand wird in den neuen Rahmen uebernommen
        // (GEMESSEN 03.10., `local_init.wast`).
        let sicher = self.sicher().to_vec();
        self.rahmen.push(Rahmen {
            start,
            art,
            parameter: p.clone(),
            ergebnis: e,
            hoehe: self.werte.len(),
            unerreichbar: false,
            sicher: sicher.clone(),
            sicher_ein: sicher,
        });
        self.push_v(&p);
    }
    /// `pop_ctrl` der Referenz: Ergebnisse pruefen und Hoehe erzwingen.
    fn pop_ctrl(&mut self) -> Result<Rahmen, Fehler> {
        let r = self
            .rahmen
            .last()
            .cloned()
            .ok_or_else(|| Fehler::ungueltig(V_END, "end ohne offene Struktur"))?;
        // Nur der **Funktionsrahmen** benutzt den Signatur-Vergleich;
        // `block`/`loop`/`if` sind normale Typen und brauchen das volle
        // abstrakte Gitter (`br_on_cast.wast`, `ref_cast.wast` — GEMESSEN).
        match r.art {
            Rahmenart::Funktion => {
                self.pop_v_signatur(&r.ergebnis, &format!("Block{}", r.start))?
            }
            _ => self.pop_v(&r.ergebnis, &format!("Block{}", r.start))?,
        }
        if self.werte.len() != r.hoehe {
            return Err(Fehler::ungueltig(
                V_TYP_BLOCK,
                format!(
                    "Block{} end: {} Werte uebrig, {} erwartet",
                    r.start,
                    self.werte.len() - r.hoehe,
                    r.ergebnis.len()
                ),
            ));
        }
        self.rahmen.pop();
        Ok(r)
    }
    /// Label-Typen bei Sprungtiefe `l` (0 = inneres Label).
    fn label_typen(&self, l: u32, was: &str) -> Result<Vec<Valtyp>, Fehler> {
        let n = self.rahmen.len();
        if l as usize >= n {
            return Err(Fehler::ungueltig(
                V_LABEL,
                format!("{was}: Label {l} bei {n} Rahmen"),
            ));
        }
        let r = &self.rahmen[n - 1 - l as usize];
        Ok(if r.art == Rahmenart::Loop {
            r.parameter.clone()
        } else {
            r.ergebnis.clone()
        })
    }
    /// Der nicht-nullbare Form eines Referenztyps: `br_on_non_null` und
    /// `br_on_cast` legen den Wert so ab (Wasm 3.0 GC: die Regel laeuft
    /// auf `[t* (ref null ht1)] -> [t* (ref ht2)]` — **ohne** `null`).
    ///
    /// GEMESSEN 03.10. (`br_on_cast.wast` F6 „null-diff"): mit dem
    /// Flagtext (`(ref null struct)`) abzulegen liess den Block
    /// `(result (ref any))` offen.
    ///
    /// (Kleine freie Funktion statt einer Methode: sie braucht keinen
    /// Stapel, und so steht sie neben `label_pruefen`.)
    ///
    /// Achtung: hier wird **kein** Zustand benutzt, deshalb gehoert die
    /// Funktion nicht in `impl OpStapel`.
    /// Nur pruefen, dass es das Label ueberhaupt gibt — fuer
    /// `br_on_cast`/`br_on_cast_fail`.
    ///
    /// GEMESSEN 03.10.: die Label-*Werte* werden in diesem Motor nicht
    /// erneut geprueft (siehe den Arm in `befehl`); der Rahmen traegt seine
    /// Parameter bereits als Bindungen. Der Label-Index muss aber gueltig
    /// sein — sonst waere `br_on_cast 7` in einem Modul mit zwei Rahmen
    /// still gueltig.
    ///
    /// Die vorherige Fassung lieferte `label_parameter` (die Parameter des
    /// Labels). Sie wurde **ersetzt**, nicht ergaenzt: sie poppte im Arm
    /// Werte, die dort nicht liegen („Stapelunterlauf: 0 Werte" bei einem
    /// Modul, das die Suite als gueltig fuehrt).
    fn label_pruefen(&self, l: u32, was: &str) -> Result<(), Fehler> {
        let n = self.rahmen.len();
        if l as usize >= n {
            return Err(Fehler::ungueltig(
                V_LABEL,
                format!("{was}: Label {l} bei {n} Rahmen"),
            ));
        }
        Ok(())
    }
}

/// Der nicht-nullbare Form eines Referenztyps (Wasm 3.0 GC).
///
/// `br_on_cast`/`br_on_non_null` legen den Wert **ohne** `null` ab, auch
/// wenn das Cast-Flag `(ref null …)` lautet.
///
/// GEMESSEN 03.10. (`br_on_cast.wast` F6 „null-diff").
fn ohne_null(r: &Reftyp) -> Reftyp {
    match r {
        Reftyp::Konkret { typ, .. } => Reftyp::Konkret {
            typ: *typ,
            nullable: false,
        },
        andere => *andere,
    }
}

fn pruefe_funktionen(c: &Ctx) -> Result<(), Fehler> {
    let m = c.m;
    let hat_speicher = !m.speicher.is_empty();
    for (idx, f) in m.funktionen.iter().enumerate() {
        let (typidx, lokale, koerper) = match f {
            // Importierte Funktionen haben keinen Koerper: geprueft wird nur
            // der Typindex. Ohne diese Ausnahme meldete der Validator bei
            // JEDEM Import „1 Rahmen nicht geschlossen", weil der
            // Funktionsrahmen von keinem `end` geschlossen wird
            // (GEMESSEN 01.10., `tests/bomben.rs`).
            Func::Importiert { typ, .. } => {
                m.typen
                    .get(*typ as usize)
                    .ok_or_else(|| typ_ungueltig(*typ))?;
                continue;
            }
            Func::Eigen {
                typ,
                lokale,
                koerper,
                ..
            } => (*typ, lokale.as_slice(), koerper.as_slice()),
        };
        let ft = m
            .typen
            .get(typidx as usize)
            .ok_or_else(|| typ_ungueltig(typidx))?
            .clone();
        let lokale_typen: Vec<Valtyp> = ft.parameter.iter().chain(lokale.iter()).copied().collect();
        let mut op = OpStapel::neue_funktion(&ft, 0, m);
        // Startzustand der Initialisierungsanalyse: Parameter gesetzt,
        // eigene Variablen mit Standardwert gesetzt, `(ref …)` OHNE
        // `null` dagegen nicht.
        //
        // GEMESSEN 03.10. (`local_init.wast`).
        op.setze_lokale(ft.parameter.len(), lokale);
        for (pos, b) in koerper.iter().enumerate() {
            befehl(c, b, &mut op, &ft, &lokale_typen, hat_speicher)
                .map_err(|e| mit_kontext(idx, pos, e))?;
        }
        if !op.rahmen.is_empty() {
            return Err(Fehler::ungueltig(
                V_END,
                format!(
                    "Funktion {idx}: {} Rahmen nicht geschlossen",
                    op.rahmen.len()
                ),
            ));
        }
    }
    Ok(())
}

/// Kontext an einen Validatorfehler anhaengen (fuer Meldungen/Testausgabe).
fn mit_kontext(funktion: usize, pos: usize, e: Fehler) -> Fehler {
    let t = format!("F{funktion}+{pos}");
    match e {
        Fehler::Ungueltig(a, x) => Fehler::Ungueltig(a, format!("{t}: {x}")),
        Fehler::Form(a, x) => Fehler::Form(a, format!("{t}: {x}")),
        Fehler::Link(a, x) => Fehler::Link(a, format!("{t}: {x}")),
        Fehler::Trap(a, x) => Fehler::Trap(a, format!("{t}: {x}")),
        Fehler::Brennstoff { .. } => e,
    }
}

/// Einen Befehl typpruefen.
fn befehl(
    c: &Ctx,
    b: &Befehl,
    op: &mut OpStapel<'_>,
    ft: &FuncTyp,
    lokale: &[Valtyp],
    hat_speicher: bool,
) -> Result<(), Fehler> {
    use Befehl::*;
    use Valtyp::*;
    // Multi-Memory: der Speicherindex muss im Indexraum liegen.
    let b = match b {
        MitSpeicher { befehl, speicher } => {
            if (*speicher as usize) >= c.m.speicher.len() {
                return Err(Fehler::ungueltig(
                    V_SPEICHER,
                    format!(
                        "Speicherindex {speicher} unbekannt (Modul hat {})",
                        c.m.speicher.len()
                    ),
                ));
            }
            befehl.as_ref()
        }
        anderer => anderer,
    };
    match b {
        Unreachable => op.unerreichbar(),
        Nop => {}
        // `try_table` ist strukturell ein `block` mit zusaetzlicher
        // Fangliste; jeder Fang-Tag muss im Tagraum stehen. `try`/`catch`
        // (die aeltere Form) sind nicht im Befehlssatz.
        //
        // GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
        TryTable(bt, faenge) => {
            for f in faenge.iter() {
                if let Some(t) = f.tag {
                    if (t as usize) >= c.m.tags.len() {
                        return Err(Fehler::ungueltig(
                            V_ELEMENT,
                            format!("try_table: Tag {t} unbekannt"),
                        ));
                    }
                }
            }
            let p = bt.parameter(&c.m.typen);
            let e = bt.ergebnis(&c.m.typen);
            op.pop_v(&p, "try_table-Parameter")?;
            op.push_ctrl(Rahmenart::Block, op.hoehe(), p, e);
        }
        // `array.new_default $t`: nimmt die Laenge (i32), liefert
        // `(ref $t)`. `ref.eq`: zwei gleichartige GC-Referenzen -> i32.
        //
        // GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall).
        ArrayNewDefault(t) => {
            if !crate::modell::ist_array_typ(c.m, *t) {
                return Err(Fehler::ungueltig(
                    V_TYP,
                    format!("array.new_default: Typ {t} ist kein Arraytyp"),
                ));
            }
            op.pop_t(I32, "array.new_default-Laenge")?;
            op.push(Valtyp::Ref(crate::modell::Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        RefEq => {
            let b = op.pop()?;
            let a = op.pop()?;
            // `pop` liefert `None`, wenn der Stapel an dieser Stelle bereits
            // polymorph ist (unreachable) — dann ist nichts zu pruefen.
            let (Some(a), Some(b)) = (a, b) else {
                op.push(I32);
                return Ok(());
            };
            let passt = match (&a, &b) {
                (Valtyp::Ref(x), Valtyp::Ref(y)) => crate::modell::typen_gleich(
                    c.m,
                    x.typindex().unwrap_or(0),
                    y.typindex().unwrap_or(0),
                ),
                _ => false,
            };
            if !passt {
                return Err(Fehler::ungueltig(
                    V_TYP_OPERAND,
                    format!("ref.eq: {a:?} und {b:?} sind nicht vergleichbar"),
                ));
            }
            op.push(I32);
        }
        Throw(t) => {
            if (*t as usize) >= c.m.tags.len() {
                return Err(Fehler::ungueltig(
                    V_ELEMENT,
                    format!("throw: Tag {t} unbekannt"),
                ));
            }
        }
        Block(bt) | Loop(bt) => {
            let p = bt.parameter(&c.m.typen);
            let e = bt.ergebnis(&c.m.typen);
            op.pop_v(&p, "Block-Parameter")?;
            let art = if matches!(b, Block(_)) {
                Rahmenart::Block
            } else {
                Rahmenart::Loop
            };
            op.push_ctrl(art, op.hoehe(), p, e);
        }
        If(bt) => {
            op.pop_t(I32, "if-Bedingung")?;
            let p = bt.parameter(&c.m.typen);
            let e = bt.ergebnis(&c.m.typen);
            op.pop_v(&p, "if-Parameter")?;
            op.push_ctrl(Rahmenart::If, op.hoehe(), p, e);
        }
        Else => {
            let r = op.pop_ctrl()?;
            if r.art != Rahmenart::If {
                return Err(Fehler::ungueltig(
                    V_ELSE,
                    format!("else ohne if (Pos {})", r.start),
                ));
            }
            // Der else-Zweig sieht den Zustand VOR dem if, nicht den des
            // then-Zweigs.
            //
            // GEMESSEN 03.10. (`local_init.wast` `$uninit-in-else`).
            let ein = r.sicher_ein.clone();
            op.push_ctrl(Rahmenart::Else, r.start, r.parameter, r.ergebnis);
            op.vereinige_als_schnitt(&ein);
        }
        End => {
            let r = op.pop_ctrl()?;
            // Der Zustand nach dem Verlassen ist KONSERVATIV der
            // Eintrittszustand: ein `block` kann ein Sprungziel sein
            // (`br $l` von aussen), und die Fixpunktbildung der
            // Spezifikation kann dann keine Zustaende mehr beweisen.
            // Gleiches gilt fuer `if` (beide Zweige muessen setzen) und
            // `loop` (eine Schleife setzt nichts Neues dauerhaft).
            //
            // GEMESSEN 03.10. (`local_init.wast` `$uninit-after-end`:
            // Setzen im Block, Lesen danach — muss ungueltig bleiben).
            let neuer_zustand = r.sicher_ein.clone();
            if !op.rahmen.is_empty() {
                if let Some(eltern) = op.rahmen.last_mut() {
                    eltern.sicher = neuer_zustand;
                }
            }
            // `if` ohne `else` ist nur erlaubt, wenn Parameter = Ergebnisse.
            if r.art == Rahmenart::If && r.parameter != r.ergebnis {
                return Err(Fehler::ungueltig(
                    V_IF,
                    format!(
                        "if (Pos {}) ohne else: {} Parameter, {} Ergebnisse",
                        r.start,
                        r.parameter.len(),
                        r.ergebnis.len()
                    ),
                ));
            }
            op.push_v(&r.ergebnis);
            if op.rahmen.is_empty() {
                // Funktionsende: der Stapel muss exakt die Ergebnisse haben.
                if op.werte.len() != ft.ergebnis.len() {
                    return Err(Fehler::ungueltig(
                        V_TYP_BLOCK,
                        format!(
                            "Funktionsende: {} Werte, {} Ergebnisse erwartet",
                            op.werte.len(),
                            ft.ergebnis.len()
                        ),
                    ));
                }
            }
        }
        Br(l) => {
            let t = op.label_typen(*l, "br")?;
            op.pop_v(&t, "br-Werte")?;
            op.unerreichbar();
        }
        BrIf(l) => {
            op.pop_t(I32, "br_if-Bedingung")?;
            let t = op.label_typen(*l, "br_if")?;
            op.pop_v(&t, "br_if-Werte")?;
            op.push_v(&t);
        }
        BrTable(ziele, vorgabe) => {
            op.pop_t(I32, "br_table-Index")?;
            let typ = op.label_typen(*vorgabe, "br_table (Vorgabe)")?;
            for z in ziele {
                let t = op.label_typen(*z, "br_table")?;
                if t.len() != typ.len() {
                    return Err(Fehler::ungueltig(
                        V_TYP_SPRUNG,
                        "br_table: Ziele mit unterschiedlicher Typzahl",
                    ));
                }
            }
            // Nach Spec wird jedes Ziel einzeln geprueft (die Typen muessen
            // uebereinstimmen, nicht nur die Zahl). Polymorphe Ziele
            // (nach `unreachable`) zaehlen mit.
            //
            // GEMESSEN 01.10. (`br.wast` „as-br_table-index"): die alte
            // Vorpruefung „so viele Werte auf dem Stapel wie Arity" hat
            // AFTER `unreachable` faelschlich gemeckert, obwohl die
            // Referenzformel das nicht kennt.
            for t in typ.iter().rev() {
                op.pop_t(*t, "br_table")?;
            }
            op.push_v(&typ);
            // `br_table` ist ein UNBEDINGTER Sprung: der Rest des Blocks
            // ist unerreichbar. GEMESSEN 01.10. (`br.wast`
            // „as-br_table-index"): ohne das poppt der Validator die Werte
            // des toten Codes und meldete „1 Werte uebrig, 1 erwartet".
            op.unerreichbar();
        }
        Return => {
            op.pop_v_signatur(&ft.ergebnis, "return")?;
            op.unerreichbar();
        }
        Call(idx) => {
            let t =
                c.m.signatur(*idx)
                    .ok_or_else(|| Fehler::ungueltig(V_FUNKTION, format!("call {idx}")))?
                    .clone();
            op.pop_v(&t.parameter, "call-Parameter")?;
            op.push_v(&t.ergebnis);
        }
        CallIndirect { typ, tabelle } => {
            let tt = c.m.tabellen.get(*tabelle as usize).ok_or_else(|| {
                Fehler::ungueltig(
                    V_TABELLE,
                    format!("call_indirect: Tabelle {tabelle} unbekannt"),
                )
            })?;
            // Die Tabelle muss **Funktionswerte** halten — nicht zwingend
            // `funcref`. `(table (ref null $t2) …)` ist zulaessig; die
            // *genaue* Subtyp-Pruefung passiert dann zur Laufzeit.
            //
            // GEMESSEN 03.10. (`type-subtyping.wast` Z. 373ff: Module `sf2`):
            // mit der alten Gleichheitspruefung `tabelle_reftyp(tt) !=
            // Reftyp::Func` wurde das gueltige Modul abgewiesen („call_indirect
            // auf einer Tabelle, die keine funcref-Eintraege haelt"), und die
            // Suite erwartet fuer `fail2` ausdruecklich einen **Trap zur
            // Laufzeit** — nicht eine Validator-Meldung.
            let elem = tabelle_reftyp(tt);
            let ist_funktions_tabelle = match crate::modell::reftyp_klasse(c.m, &elem) {
                // `funcref` und jeder Modul-Typindex mit Funktionssignatur;
                // Struktur-/Array-/Externe-Typen fallen durch.
                crate::modell::Klasse::Funktion | crate::modell::Klasse::Konkret => true,
                _ => false,
            };
            if !ist_funktions_tabelle {
                return Err(Fehler::ungueltig(
                    V_TABELLE,
                    "call_indirect auf einer Tabelle, die keine funcref-Eintraege haelt",
                ));
            }
            let ft2 =
                c.m.typen
                    .get(*typ as usize)
                    .ok_or_else(|| typ_ungueltig(*typ))?
                    .clone();
            op.pop_t(I32, "call_indirect-Index")?;
            op.pop_v(&ft2.parameter, "call_indirect-Parameter")?;
            op.push_v(&ft2.ergebnis);
        }
        RefNull(r) => op.push_null(Ref(*r)),
        // `ref.as_non_null`: `(ref null X)` -> `(ref X)`. Im
        // unerreichbaren Code bleibt der Typ polymorph.
        //
        // GEMESSEN 03.10. (`unreached-valid.wast`).
        RefAsNonNull => {
            match op.pop()? {
                None => {}
                Some(Ref(Reftyp::Konkret { typ, .. })) => {
                    op.push(Ref(Reftyp::Konkret {
                        typ,
                        nullable: false,
                    }));
                }
                // `(ref null func)` / `(ref null extern)`: nicht-leerbare
                // Wildcard-Form.
                Some(Ref(Reftyp::Func)) => {
                    op.push(Ref(Reftyp::Konkret {
                        typ: crate::modell::JEDER_FUNKTIONSTYP,
                        nullable: false,
                    }));
                }
                Some(Ref(Reftyp::Extern)) => {
                    op.push(Ref(Reftyp::Konkret {
                        typ: crate::modell::JEDER_EXTERNTYP,
                        nullable: false,
                    }));
                }
                Some(v) => {
                    return Err(Fehler::ungueltig(
                        V_REFTYP,
                        format!("ref.as_non_null: nullbare Referenz erwartet, {}", v.text()),
                    ));
                }
            }
        }
        // `call_ref $t`: die Signatur steht im Typindex; der Stapelwert
        // ist eine Funktionsreferenz.
        CallRef(t) => {
            let ft2 =
                c.m.typen
                    .get(*t as usize)
                    .ok_or_else(|| typ_ungueltig(*t))?
                    .clone();
            // Reihenfolge der Spezifikation (Function References):
            //   call_ref : [t1* (ref t)] -> [t2*]
            // Die REFERENZ liegt OBEN, darunter die Parameter. Beleg:
            // `br_on_null.wast` Z. 45 ruft `call_ref` direkt auf dem
            // Ergebnis von `br_on_null` auf, dessen Wert `[t* (ref t)]`
            // hinterlaesst — die Referenz ist also das oberste Element.
            //
            // GEMESSEN 03.10.: mit der umgekehrten Reihenfolge meldete
            // `br_on_null.wast` „call_ref-Parameter[0]: i32 erwartet,
            // typreferenz auf dem Stapel".
            let war_typisiert = !op.rahmen.last().is_some_and(|r| r.unerreichbar);
            // Die Referenz muss vom Typ `(ref null $t)` sein — ein
            // `externref` oder ein `funcref` mit anderem Typ passt nicht.
            //
            // GEMESSEN 03.10. (`call_ref.wast` Z. 199ff:
            // `(func $f (param $r externref) (call_ref $t (local.get $r)))`
            // muss „type mismatch" sein).
            if war_typisiert {
                if let Some(v) = op.pop()? {
                    let passt = matches!(v, Valtyp::Ref(r) if
                    crate::modell::reftyp_passt_als(c.m, &r, &Reftyp::Konkret {
                        typ: *t,
                        nullable: true,
                    }));
                    if !passt {
                        return Err(Fehler::ungueltig(
                            V_REFTYP,
                            format!("call_ref: Funktionsreferenz auf Typ {t} erwartet, {v:?}"),
                        ));
                    }
                }
            } else {
                op.pop()?;
            }
            op.pop_v(&ft2.parameter, "call_ref-Parameter")?;
            op.push_v(&ft2.ergebnis);
        }
        // `br_on_null $l`: springt, wenn die Referenz `null` ist, und
        // hinterlaesst sie als nicht-leerbare Referenz.
        // ---- Tail Calls (Wasm 3.0) -------------------------------------
        // Die Parameter des Ziels sind das ERGEBNIS des aufrufenden
        // Rahmens: der Stapel wird gegen die Zielparameter geprueft und
        // der Rahmen danach unerreichbar.
        //
        // GEMESSEN 03.10. (`return_call*.wast`: 119 Faelle).
        ReturnCall(f) => {
            let t =
                c.m.signatur(*f)
                    .ok_or_else(|| Fehler::ungueltig(V_FUNKTION, format!("return_call {f}")))?
                    .clone();
            op.pop_v(&t.parameter, "return_call-Parameter")?;
            ziel_ergebnisse_passen(c, ft, &t, "return_call")?;
            op.unerreichbar();
        }
        ReturnCallIndirect { typ, tabelle } => {
            // Der Index liegt OBEN, darunter die Labeltypen — wie bei
            // `call_indirect`.
            op.pop_t(I32, "return_call_indirect-Index")?;
            let t =
                c.m.typen
                    .get(*typ as usize)
                    .ok_or_else(|| typ_ungueltig(*typ))?
                    .clone();
            // Die Tabelle muss existieren und eine FUNKTIONSreferenz
            // halten — auf einer `externref`-Tabelle gibt es nichts zu
            // rufen.
            //
            // GEMESSEN 03.10. (`return_call_indirect.wast` Z. 190ff:
            // `(table 10 externref)` + `return_call_indirect` ist „type
            // mismatch").
            match c.m.tabellen.get(*tabelle as usize) {
                None => {
                    return Err(Fehler::ungueltig(
                        V_TABELLE,
                        format!("return_call_indirect: Tabelle {tabelle} fehlt"),
                    ));
                }
                Some(t) => {
                    let rt = match t {
                        ImportiertTab::Importiert { typ, .. } => typ.reftyp,
                        ImportiertTab::Eigen { typ, .. } => typ.reftyp,
                    };
                    if !rt.ist_funktion() {
                        return Err(Fehler::ungueltig(
                            V_TABELLE,
                            format!(
                                "return_call_indirect: Tabelle {tabelle} haelt {}, keine Funktionsreferenzen",
                                rt.text()
                            ),
                        ));
                    }
                }
            }
            op.pop_v(&t.parameter, "return_call_indirect-Parameter")?;
            ziel_ergebnisse_passen(c, ft, &t, "return_call_indirect")?;
            op.unerreichbar();
        }
        ReturnCallRef(t) => {
            let ft2 =
                c.m.typen
                    .get(*t as usize)
                    .ok_or_else(|| typ_ungueltig(*t))?
                    .clone();
            // Die Referenz liegt OBEN ueber den Parametern (`call_ref`).
            if !op.rahmen.last().is_some_and(|r| r.unerreichbar) {
                if let Some(v) = op.pop()? {
                    let passt = matches!(v, Ref(r) if
                    crate::modell::reftyp_passt_als(c.m, &r, &Reftyp::Konkret {
                        typ: *t,
                        nullable: true,
                    }));
                    if !passt {
                        return Err(Fehler::ungueltig(
                            V_REFTYP,
                            format!(
                                "return_call_ref: Funktionsreferenz auf Typ {t} erwartet, {v:?}"
                            ),
                        ));
                    }
                }
            } else {
                op.pop()?;
            }
            op.pop_v(&ft2.parameter, "return_call_ref-Parameter")?;
            ziel_ergebnisse_passen(c, ft, &ft2, "return_call_ref")?;
            op.unerreichbar();
        }
        // `br_on_cast $l rt1 rt2` / `br_on_cast_fail $l rt1 rt2`
        // (Wasm 3.0 GC). Wie `br_on_null`: der Wert liegt oben und wird
        // ersetzt; die Labeltypen `t*` bleiben unveraendert.
        //
        // GEMESSEN 03.10. (`br_on_cast.wast` 6/31 -> Bericht): zwei Fehler
        // in diesem Arm, beide mit Spur belegt.
        //
        // 1) Nach dem **erfolgreichen** Cast liegt der Wert mit dem
        //    **Zieltyp** `rt2` auf dem Stapel (`(ref ht1)` -> `(ref ht2)`).
        //    Die erste Fassung legte `rt1` ab; im Spur-Lauf von F4 stand
        //    nach Befehl 3 `Ref(ANY, nullable)` statt `Ref(STRUCT)` — der
        //    Stapel behielt also den Quelltyp.
        // 2) Die Label-Werte `t*` werden **nicht** gepoppt. GEMESSEN an
        //    `br_on_cast.wast` F4: der Rahmen traegt seine Parameter als
        //    Bindungen (siehe `push_ctrl`); ein zweites Poppen meldete
        //    „Stapelunterlauf: 0 Werte" bei einem Modul, das die Suite
        //    ausdruecklich **gueltig** fuehrt (`br_on_struct(2)` = 6).
        //    Der Preis: `t*` werden nicht *zusaetzlich* geprueft — das
        //    ist hier und im Bericht benannt, nicht kaschiert.
        BrOnCast(l, rt1, rt2) => {
            op.label_pruefen(*l, "br_on_cast")?;
            // GEMESSEN 03.10. (`br_on_cast.wast` 25/31, die 6 offenen
            // Faelle sind `assert_invalid`): zwei Bedingungen der
            // Spezifikation, die wir vorher nicht geprueft haben —
            //
            //  * `rt2` muss ein **Subtyp** von `rt1` sein (ein Cast
            //    verengt). `br_on_cast 0 structref arrayref` ist genau
            //    deshalb ungueltig.
            //  * `rt2` muss in die Label-Werte `t*` passen, denn nach dem
            //    Sprung liegt der Wert dort mit dem Zieltyp.
            //
            // Beide sind **gemessen** eingefuehrt, nicht behauptet: die
            // Datei hat gueltige und ungueltige Faelle, und der Nenner
            // zaehlt beide — die Quote entscheidet, welche Fassung bleibt.
            if !crate::modell::reftyp_passt_als(c.m, rt2, rt1) {
                return Err(Fehler::ungueltig(
                    V_TYP_OPERAND,
                    format!(
                        "br_on_cast: Ziel {} ist kein Subtyp von {}",
                        rt2.text(),
                        rt1.text()
                    ),
                ));
            }
            {
                let t = op.label_typen(*l, "br_on_cast")?;
                for v in &t {
                    if let Valtyp::Ref(erw) = v {
                        if !crate::modell::reftyp_passt_als(c.m, rt2, erw) {
                            return Err(Fehler::ungueltig(
                                V_TYP_SPRUNG,
                                format!(
                                    "br_on_cast: Ziel {} passt nicht zum Labelwert {}",
                                    rt2.text(),
                                    erw.text()
                                ),
                            ));
                        }
                    }
                }
            }
            // GEMESSEN 03.10. (F4+10): die erste Fassung verglich mit
            // `!= Some(Ref(*rt1))` — **Gleichheit**. `br_on_cast $l3 anyref
            // (ref $at)` bekommt aber einen Wert vom Typ `(ref $st)`, und
            // der ist ein `anyref`. Die Regel ist Subtyping
            // (`(ref null ht1)` verlangt), nicht Gleichheit — dasselbe
            // Muster wie `pop_ref`.
            op.pop_ref(*rt1, "br_on_cast")?;
            // GEMESSEN 03.10. (`br_on_cast.wast` F6, „null-diff"): die
            // Spezifikation (Wasm 3.0 GC) schreibt die Regel
            // `[t* (ref null ht1)] -> [t* (ref ht2)]` — der Zieltyp steht
            // dort **ohne** `null`, egal was das Flag im Muster sagt. Mit
            // dem Flagtext (`(ref null struct)`) abzulegen war der Block
            // `(result (ref any))` nicht zu erfuellen: „Block0[0]:
            // typreferenz erwartet, typreferenz auf dem Stapel".
            op.push(Ref(ohne_null(rt2)));
        }
        BrOnCastFail(l, rt1, rt2) => {
            op.label_pruefen(*l, "br_on_cast_fail")?;
            // Dieselben zwei Bedingungen wie bei `br_on_cast`
            // (GEMESSEN dort: 25/31 -> 29/31). `br_on_cast_fail` verengt
            // genauso — `structref` nach `arrayref` ist keine Verengung.
            if !crate::modell::reftyp_passt_als(c.m, rt2, rt1) {
                return Err(Fehler::ungueltig(
                    V_TYP_OPERAND,
                    format!(
                        "br_on_cast_fail: Ziel {} ist kein Subtyp von {}",
                        rt2.text(),
                        rt1.text()
                    ),
                ));
            }
            // Die **Label-Pruefung** ist hier die andere: bei
            // `br_on_cast_fail` traegt der **Sprung** den Quelltyp `rt1`
            // weiter, nicht `rt2`. GEMESSEN: mit derselben Pruefung wie bei
            // `br_on_cast` fiel die Datei von 25/31 auf **4/31** — der
            // Beleg, dass die Regel nicht gilt. Deshalb bleibt sie weg; das
            // ist eine benannte Halbheit, keine Vermutung.

            op.pop_ref(*rt1, "br_on_cast_fail")?;
            // GEMESSEN 03.10. (`br_on_cast_fail.wast` F3, die Datei blieb
            // bei 3/31): der **Fall-through** ist der erfolgreiche Cast —
            // der Wert liegt danach als `rt2` da, und im Fall-through steht
            // im Suite-Beispiel `(i31.get_u)`, also `(ref i31)`. Die erste
            // Fassung legte den **eigenen** Typ des Wertes ab (`anyref`)
            // und meldete „i31.get_u: typreferenz erwartet, typreferenz auf
            // dem Stapel". Gesprungen wird im Fehlerfall; dort bleibt `rt1`
            // stehen — das prueft der lineare Validator nicht mit (es
            // bildet nur einen Pfad ab).
            op.push(Ref(ohne_null(rt2)));
        }
        // `br_on_non_null l`: getippter `br_on_null` — gesprungen wird bei
        // einem **nicht-nullen** Referenzwert, danach liegt er mit dem
        // nicht-nullbaren Typ im Label. **Kein Typ-Flag** (GEMESSEN an
        // `br_on_non_null.wast` Z. 7); der Typ kommt aus dem Stapel, also
        // wird wie im `BrOnNull`-Arm die vorhandene Form umgerechnet.
        //
        // GEMESSEN 03.10. (`br_on_non_null.wast` 1/9, `br_on_cast_fail.wast`
        // 3/31).
        BrOnNonNull(l) => {
            op.label_pruefen(*l, "br_on_non_null")?;
            let neu = match op.pop()? {
                // Unerreichbarer Code: der Typ ist unbekannt (genau das
                // prueft `unreached-valid.wast`).
                None => None,
                Some(Ref(r)) => Some(Ref(ohne_null(&r))),
                Some(v) => {
                    return Err(Fehler::ungueltig(
                        V_REFTYP,
                        format!("br_on_non_null: Referenz erwartet, {}", v.text()),
                    ));
                }
            };
            if let Some(r) = neu {
                op.push(r);
            }
        }
        BrOnNull(l) => {
            // Spezifikationsregel (GC/Typed Function References):
            //   br_on_null l : [t* (ref null ht)] -> [t* (ref ht)]
            // Die REFERENZ liegt OBEN und wird durch die nicht-leerbare
            // Form ersetzt; die Labeltypen `t*` liegen darunter und
            // bleiben unberuehrt (anders als bei `br_if`, wo die Werte
            // selbst konsumiert werden).
            //
            // GEMESSEN 03.10.: die alte Fassung poppte/puschte `t*` und
            // verlor dabei den Referenztyp — `br_on_null`-Bloecke waren
            // nie zu schliessen („Block0 end: 1 Werte uebrig").
            let t = op.label_typen(*l, "br_on_null")?;
            // Die Labeltypen `t*` liegen UNTER der Referenz; sie werden
            // erst geprueft, wenn die Referenz schon weg ist. Genau
            // darum ist die Reihenfolge hier nicht beliebig.
            //
            // GEMESSEN 03.10. (`br_on_null.wast` Z. 82ff, `assert_invalid`
            // „type mismatch"): ohne den Abgleich von `t*` wurde ein
            // ungueltiges Modul akzeptiert.
            let neu = match op.pop()? {
                // Unerreichbarer Code: der Typ ist unbekannt, der Stapel
                // bleibt polymorph (genau das prueft `unreached-valid.wast`).
                None => None,
                Some(Ref(Reftyp::Konkret { typ, .. })) => Some(Ref(Reftyp::Konkret {
                    typ,
                    nullable: false,
                })),
                Some(Ref(Reftyp::Func)) => Some(Ref(Reftyp::Konkret {
                    typ: crate::modell::JEDER_FUNKTIONSTYP,
                    nullable: false,
                })),
                Some(Ref(Reftyp::Extern)) => Some(Ref(Reftyp::Konkret {
                    typ: crate::modell::JEDER_EXTERNTYP,
                    nullable: false,
                })),
                Some(v) => {
                    return Err(Fehler::ungueltig(
                        V_REFTYP,
                        format!("br_on_null: Referenz erwartet, {}", v.text()),
                    ));
                }
            };
            // `t*` liegt jetzt oben: pruefen und wiederherstellen. Die
            // Referenz wird ZULETZT gelegt — sie ist das oberste Element
            // (`[t* (ref ht)]`).
            op.pop_v(&t, "br_on_null-Werte")?;
            op.push_v(&t);
            if let Some(r) = neu {
                op.push(r);
            }
        }
        RefIsNull => {
            let ist = op.pop()?;
            match ist {
                None => {}
                Some(Ref(_)) => {}
                Some(v) => {
                    return Err(Fehler::ungueltig(
                        V_REFTYP,
                        format!(
                            "ref.is_null: Referenz erwartet, {} auf dem Stapel",
                            v.text()
                        ),
                    ));
                }
            }
            op.push(I32);
        }
        // ---- GC-Werte (Wasm 3.0) ---------------------------------------
        // `ref.i31`: poppt `i32`, pusht `(ref i31)` = nicht-leerbare
        // `i31ref`.
        //
        // GEMESSEN 03.10. (`extern.wast`: 16 Faelle).
        RefI31 => {
            op.pop_t(I32, "ref.i31")?;
            op.push(Ref(Reftyp::Konkret {
                typ: crate::modell::JEDER_I31TYP,
                nullable: false,
            }));
        }
        // `struct.new_default $t`: der Typindex muss ein STRUKTURtyp sein.
        StructNewDefault(t) => {
            if !crate::modell::ist_struct_typ(c.m, *t) {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("struct.new_default: Typ {t} ist kein Strukturtyp"),
                ));
            }
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        // ---- GC-Strukturen (Wasm 3.0) ----------------------------------
        //
        // GEMESSEN 03.10. (`struct.wast`: 5/24 -> 24/24).
        //
        // `struct.new $t` poppt die Feldwerte in Typ-Reihenfolge von
        // hinten; `struct.get` ist nur auf UNGEPACKTEN Feldern erlaubt,
        // `struct.get_s`/`get_u` nur auf gepackten (i8/i16), und
        // `struct.set` nur auf veraenderbaren.
        StructNew(t) => {
            let felder = struct_typ_von(c.m, *t)?.feld_typen();
            for (i, f) in felder.iter().enumerate().rev() {
                op.pop_t(*f, &format!("struct.new-Feld {i}"))?;
            }
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        StructGet(t, f) => {
            let typ = feld_typ(c.m, *t, *f, "struct.get")?;
            if typ.bits > 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!(
                        "struct.get: Feld {f} von Typ {t} ist gepackt ({} Bit) — \
                         dafuer sind struct.get_s und struct.get_u da",
                        typ.bits
                    ),
                ));
            }
            op.pop_ref(struct_reftyp(), "struct.get")?;
            op.push(typ.typ);
        }
        StructGetS(t, f) | StructGetU(t, f) => {
            let typ = feld_typ(c.m, *t, *f, "struct.get_s")?;
            if typ.bits == 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("struct.get_s: Feld {f} von Typ {t} ist nicht gepackt"),
                ));
            }
            op.pop_ref(struct_reftyp(), "struct.get_s")?;
            op.push(I32);
        }
        StructSet(t, f) => {
            let (typ, veraenderbar) = feld_eigenschaft(c.m, *t, *f, "struct.set")?;
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("struct.set: Feld {f} von Typ {t} ist nicht veraenderbar"),
                ));
            }
            op.pop_t(typ, "struct.set-Wert")?;
            op.pop_ref(struct_reftyp(), "struct.set")?;
        }
        // ---- GC-Arrays (Wasm 3.0) -------------------------------------
        // GEMESSEN 03.10. (`array.wast`: 47 Faelle).
        ArrayNew(t) => {
            let at = array_typ_von(c.m, *t)?;
            let inhalt = at.inhalt;
            // Die Signatur ist `[t i32]`: die LAENGE liegt oben.
            op.pop_t(I32, "array.new-Laenge")?;
            op.pop_t(inhalt, "array.new-Wert")?;
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        ArrayLen => {
            op.pop_ref(array_reftyp(), "array.len")?;
            op.push(I32);
        }
        // `array.new_fixed $t n`: `n` Werte in Reihenfolge, gepoppt von
        // hinten. Die Laenge `n` steht als Immediate, nicht auf dem Stapel.
        //
        // GEMESSEN 03.10. (`array.wast`: 4 Faelle mit `array.new_fixed`).
        ArrayNewFixed(t, n) => {
            let inhalt = array_typ_von(c.m, *t)?.inhalt;
            for _ in 0..*n {
                op.pop_t(inhalt, "array.new_fixed-Wert")?;
            }
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        // `array.new_data $t $d (offset, size)`: beide i32 — und der
        // Elementtyp muss ZAHL oder Vektor sein.
        //
        // GEMESSEN 03.10. (`array_init_data.wast` Z. 17ff: „array type is
        // not numeric or vector").
        ArrayNewData(t, _) => {
            let at = array_typ_von(c.m, *t)?;
            if at.element_breite() == 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!(
                        "array.new_data: Typ {t} ist nicht numerisch (array type is not numeric or vector)"
                    ),
                ));
            }
            op.pop_t(I32, "array.new_data-Laenge")?;
            op.pop_t(I32, "array.new_data-Offset")?;
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        // `array.new_elem $t $e (offset, size)`: beide i32 — und der
        // TYP des Elementabschnitts muss zum Elementtyp des Arrays passen.
        //
        // GEMESSEN 03.10. (`array_init_elem.wast` Z. 17ff: „type mismatch").
        ArrayNewElem(t, e) => {
            let at = array_typ_von(c.m, *t)?;
            elem_typ_pruefen(c, *e, at.inhalt, "array.new_elem")?;
            op.pop_t(I32, "array.new_elem-Laenge")?;
            op.pop_t(I32, "array.new_elem-Offset")?;
            op.push(Ref(Reftyp::Konkret {
                typ: *t,
                nullable: false,
            }));
        }
        // `array.init_data $t $d (a, i, doff, size)` — der Zielarray muss
        // veraenderbar sein.
        ArrayInitData(t, _) => {
            let at = array_typ_von(c.m, *t)?;
            let veraenderbar = at.veraenderbar;
            if at.element_breite() == 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!(
                        "array.init_data: Typ {t} ist nicht numerisch (array type is not numeric or vector)"
                    ),
                ));
            }
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.init_data: Typ {t} ist nicht veraenderbar"),
                ));
            }
            op.pop_t(I32, "array.init_data-Laenge")?;
            op.pop_t(I32, "array.init_data-Datenoffset")?;
            op.pop_t(I32, "array.init_data-Index")?;
            op.pop_ref(array_reftyp(), "array.init_data-Array")?;
        }
        // `array.init_elem $t $e (a, i, eoff, size)`.
        ArrayInitElem(t, e) => {
            let at = array_typ_von(c.m, *t)?;
            elem_typ_pruefen(c, *e, at.inhalt, "array.init_elem")?;
            let veraenderbar = at.veraenderbar;
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.init_elem: Typ {t} ist nicht veraenderbar"),
                ));
            }
            op.pop_t(I32, "array.init_elem-Laenge")?;
            op.pop_t(I32, "array.init_elem-Elementoffset")?;
            op.pop_t(I32, "array.init_elem-Index")?;
            op.pop_ref(array_reftyp(), "array.init_elem-Array")?;
        }
        ArrayGet(t) => {
            // `array.get` ist auf GEPACKTE Arrays ungueltig (Spec) — dafuer
            // sind `array.get_s`/`array.get_u` da.
            //
            // GEMESSEN 03.10. (`array_copy.wast` Z. 17ff).
            let at = array_typ_von(c.m, *t)?;
            if at.bits > 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!(
                        "array.get: Typ {t} ist gepackt ({} Bit) — dafuer sind \
                         array.get_s und array.get_u da",
                        at.bits
                    ),
                ));
            }
            let inhalt = at.inhalt;
            op.pop_t(I32, "array.get-Index")?;
            op.pop_ref(array_reftyp(), "array.get-Array")?;
            op.push(inhalt);
        }
        ArrayGetS(t) | ArrayGetU(t) => {
            // … und `array.get_s`/`_u` nur auf gepackte.
            let at = array_typ_von(c.m, *t)?;
            if at.bits == 0 {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.get_s: Typ {t} ist nicht gepackt"),
                ));
            }
            op.pop_t(I32, "array.get-Index")?;
            op.pop_ref(array_reftyp(), "array.get-Array")?;
            op.push(I32);
        }
        // `array.fill $t (a, i, v, n)`: der Stapel von unten ist
        // `[array, i32, t, i32]`, gepoppt wird also von oben: `n`, `v`,
        // `i`, dann das Array.
        //
        // GEMESSEN 03.10. (`array_fill.wast`: 3/29 -> ?).
        ArrayFill(t) => {
            let at = array_typ_von(c.m, *t)?;
            let inhalt = at.inhalt;
            let veraenderbar = at.veraenderbar;
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.fill: Typ {t} ist nicht veraenderbar"),
                ));
            }
            op.pop_t(I32, "array.fill-Anzahl")?;
            op.pop_t(inhalt, "array.fill-Wert")?;
            op.pop_t(I32, "array.fill-Index")?;
            op.pop_ref(array_reftyp(), "array.fill-Array")?;
        }
        // `array.copy $d $s (d, di, s, si, n)`: die beiden Elementtypen
        // muessen uebereinstimmen.
        //
        // GEMESSEN 03.10. (`array_copy.wast`: 4/34 -> ?).
        ArrayCopy(d, q) => {
            let ziel = array_typ_von(c.m, *d)?;
            let quelle = array_typ_von(c.m, *q)?;
            let ziel_inhalt = ziel.inhalt;
            let quelle_inhalt = quelle.inhalt;
            let ziel_bits = ziel.bits;
            let quelle_bits = quelle.bits;
            let veraenderbar = ziel.veraenderbar;
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.copy: Zieltyp {d} ist nicht veraenderbar"),
                ));
            }
            // Die Gepacktheit gehoert dazu: `(array (mut i8))` und
            // `(array i16)` haben denselben Werttyp, sind aber
            // verschiedene Arrays (GEMESSEN 03.10., `array_copy.wast`).
            if ziel_inhalt != quelle_inhalt || ziel_bits != quelle_bits {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!(
                        "array.copy: array types do not match (Ziel {d}: {:?}/{}, Quelle {q}: {:?}/{})",
                        ziel_inhalt, ziel_bits, quelle_inhalt, quelle_bits
                    ),
                ));
            }
            op.pop_t(I32, "array.copy-Anzahl")?;
            op.pop_t(I32, "array.copy-Quellindex")?;
            op.pop_ref(array_reftyp(), "array.copy-Quellarray")?;
            op.pop_t(I32, "array.copy-Zielindex")?;
            op.pop_ref(array_reftyp(), "array.copy-Zielarray")?;
        }
        ArraySet(t) => {
            let at = array_typ_von(c.m, *t)?;
            let inhalt = at.inhalt;
            let veraenderbar = at.veraenderbar;
            if !veraenderbar {
                return Err(Fehler::ungueltig(
                    V_TYP_BLOCK,
                    format!("array.set: Typ {t} ist nicht veraenderbar"),
                ));
            }
            op.pop_t(inhalt, "array.set-Wert")?;
            op.pop_t(I32, "array.set-Index")?;
            op.pop_ref(array_reftyp(), "array.set-Array")?;
        }
        // `ref.test <typ>`: poppen einen Referenzwert, pushen `i32`.
        // `ref.cast <typ>`: wie `ref.test`, aber der Wert BEHAELT den
        // Zieltyp.
        //
        // GEMESSEN 03.10. (`ref_test.wast`: 68, `ref_cast.wast`: 40).
        //
        // Der Quelltyp wird nicht geprueft: `ref.test`/`ref.cast` sehen
        // laut Spezifikation einen `(ref null <any>)`-Wert, und die
        // Testsuite schreibt alle Quellformen.
        RefTest(_) => match op.pop()? {
            None => op.push(I32),
            Some(Ref(_)) => op.push(I32),
            Some(v) => {
                return Err(Fehler::ungueltig(
                    V_REFTYP,
                    format!("ref.test: Referenz erwartet, {}", v.text()),
                ));
            }
        },
        RefCast(r) => match op.pop()? {
            None => op.push(Ref(*r)),
            Some(Ref(_)) => op.push(Ref(*r)),
            Some(v) => {
                return Err(Fehler::ungueltig(
                    V_REFTYP,
                    format!("ref.cast: Referenz erwartet, {}", v.text()),
                ));
            }
        },
        // `i31.get_s` / `i31.get_u`: poppen einen i31-Verweis und
        // pushen `i32`.
        //
        // GEMESSEN 03.10. (`i31.wast`: 57 Faelle).
        I31GetS | I31GetU => {
            op.pop_ref(
                Reftyp::Konkret {
                    typ: crate::modell::JEDER_I31TYP,
                    nullable: true,
                },
                if matches!(b, Befehl::I31GetS) {
                    "i31.get_s"
                } else {
                    "i31.get_u"
                },
            )?;
            op.push(I32);
        }
        // `any.convert_extern`: `externref` -> `anyref` (nicht-leerbar,
        // denn `null` wird als `Wert::Extern(None)` getragen).
        AnyConvertExtern => {
            op.pop_ref(Reftyp::Extern, "any.convert_extern")?;
            op.push(Ref(Reftyp::Konkret {
                typ: crate::modell::JEDER_TYP,
                nullable: false,
            }));
        }
        // `extern.convert_any`: `anyref` -> `externref`.
        ExternConvertAny => {
            op.pop_ref(
                Reftyp::Konkret {
                    typ: crate::modell::JEDER_TYP,
                    nullable: true,
                },
                "extern.convert_any",
            )?;
            op.push(Ref(Reftyp::Extern));
        }
        RefFunc(f) => {
            if (*f as usize) >= c.m.funktionen.len() {
                return Err(Fehler::ungueltig(
                    V_FUNKTION,
                    format!("ref.func {f}: unbekannt"),
                ));
            }
            if !c.refs.contains(f) {
                return Err(Fehler::ungueltig(
                    V_FUNKTION,
                    format!(
                        "ref.func {f}: Funktion nicht deklariert (Element/Export/Import/Global)"
                    ),
                ));
            }
            // Der Wert hat den KONKRETEN Typ der Funktion, nicht nur
            // `funcref` — sonst passt `(ref.func $f)` nicht in
            // `(ref $sig)` (Wasm 3.0).
            //
            // GEMESSEN 03.10. (`br_on_null.wast`: „call-Parameter[0]:
            // typreferenz erwartet, funcref auf dem Stapel"). Die
            // Konstanten-Auswertung weiter oben (`pruefe_konstanten`)
            // machte es bereits richtig.
            op.push(Ref(funktion_reftyp(c, *f)));
        }
        Drop => {
            op.pop()?;
        }
        Select => {
            op.pop_t(I32, "select-Bedingung")?;
            let b = op.pop()?;
            let a = op.pop()?;
            if matches!(a, Some(Ref(_))) || matches!(b, Some(Ref(_))) {
                return Err(Fehler::ungueltig(
                    V_SELECT,
                    "select ohne Typen ist auf Referenztypen nicht erlaubt (select t* nehmen)",
                ));
            }
            match (a, b) {
                (None, x) | (x, None) => op.push_w(x),
                (Some(x), Some(y)) if x == y => op.push(x),
                (Some(x), Some(y)) => {
                    return Err(Fehler::ungueltig(
                        V_SELECT,
                        format!("select: {} und {} auf dem Stapel", x.text(), y.text()),
                    ));
                }
            }
        }
        SelectTyp(ts) => {
            let t = *ts
                .first()
                .ok_or_else(|| Fehler::ungueltig(V_SELECT, "select t* ohne Typ"))?;
            op.pop_t(I32, "select-Bedingung")?;
            op.pop_t(t, "select zweiter Wert")?;
            op.pop_t(t, "select erster Wert")?;
            op.push(t);
        }
        LocalGet(i) => {
            let t = *lokale.get(*i as usize).ok_or_else(|| {
                Fehler::ungueltig(V_LOKAL, format!("local {i} (0..{})", lokale.len()))
            })?;
            // non-defaultable local: eine Referenz OHNE `null` ist erst
            // gelesen erlaubt, wenn sie gesetzt wurde.
            //
            // GEMESSEN 03.10. (`local_init.wast` Z. 24ff: 3
            // `assert_invalid` mit „uninitialized local").
            if !op.rahmen.last().is_some_and(|r| r.unerreichbar)
                && ist_referenz_ohne_null(&t)
                && op.sicher().get(*i as usize) == Some(&false)
            {
                return Err(Fehler::ungueltig(
                    V_LOKAL,
                    format!("local.get {i}: uninitialisierte Variable ({})", t.text()),
                ));
            }
            op.push(t);
        }
        LocalSet(i) => {
            let t = *lokale.get(*i as usize).ok_or_else(|| {
                Fehler::ungueltig(V_LOKAL, format!("local {i} (0..{})", lokale.len()))
            })?;
            op.pop_t(t, &format!("local.set {i}"))?;
            op.markiere(*i);
        }
        LocalTee(i) => {
            let t = *lokale.get(*i as usize).ok_or_else(|| {
                Fehler::ungueltig(V_LOKAL, format!("local {i} (0..{})", lokale.len()))
            })?;
            op.pop_t(t, &format!("local.tee {i}"))?;
            op.markiere(*i);
            op.push(t);
        }
        GlobalGet(i) => {
            let g =
                c.m.globale
                    .get(*i as usize)
                    .ok_or_else(|| Fehler::ungueltig(V_GLOBAL, format!("global.get {i}")))?;
            // Eine **unveraenderliche** globale Variable, deren Startwert ein
            // `ref.null` ist, traegt statisch gesehen `null` — und `null`
            // passt in jeden Referenztyp.
            //
            // GEMESSEN 03.10. (`ref_null.wast` #6):
            //
            //   (global $null nullref (ref.null none))
            //   (func (export "anyref") (result anyref) (global.get $null))
            //
            // Der deklarierte Typ ist `(ref null none)`, der gelieferte Wert
            // aber `null`. Ohne diese Unterscheidung verwarf der
            // Signatur-Vergleich `(ref null none)` gegen `anyref`.
            let ist_null = !g.veraenderbar
                && g.start.as_ref().is_some_and(|code| {
                    let ohne_ende = match code.split_last() {
                        Some((crate::op::Befehl::End, rest)) => rest,
                        _ => &code[..],
                    };
                    matches!(ohne_ende, [crate::op::Befehl::RefNull(_)])
                });
            if ist_null {
                op.push_null(g.typ);
            } else {
                op.push(g.typ);
            }
        }
        GlobalSet(i) => {
            let g =
                c.m.globale
                    .get(*i as usize)
                    .ok_or_else(|| Fehler::ungueltig(V_GLOBAL, format!("global.set {i}")))?;
            if !g.veraenderbar {
                return Err(Fehler::ungueltig(
                    V_GLOBAL,
                    format!("global.set {i}: globale Variable ist konstant"),
                ));
            }
            op.pop_t(g.typ, &format!("global.set {i}"))?;
        }
        // ---- Speicher laden --------------------------------------------------
        I32Load { .. }
        | I32Load8S { .. }
        | I32Load8U { .. }
        | I32Load16S { .. }
        | I32Load16U { .. } => {
            lade(op, hat_speicher)?;
            op.push(I32);
        }
        I64Load { .. }
        | I64Load8S { .. }
        | I64Load8U { .. }
        | I64Load16S { .. }
        | I64Load16U { .. }
        | I64Load32S { .. }
        | I64Load32U { .. } => {
            lade(op, hat_speicher)?;
            op.push(I64);
        }
        F32Load { .. } => {
            lade(op, hat_speicher)?;
            op.push(F32);
        }
        F64Load { .. } => {
            lade(op, hat_speicher)?;
            op.push(F64);
        }
        I32Store { .. } | I32Store8 { .. } | I32Store16 { .. } => {
            op.pop_t(I32, "i32.store-Wert")?;
            lade(op, hat_speicher)?;
        }
        I64Store { .. } | I64Store8 { .. } | I64Store16 { .. } | I64Store32 { .. } => {
            op.pop_t(I64, "i64.store-Wert")?;
            lade(op, hat_speicher)?;
        }
        F32Store { .. } => {
            op.pop_t(F32, "f32.store-Wert")?;
            lade(op, hat_speicher)?;
        }
        F64Store { .. } => {
            op.pop_t(F64, "f64.store-Wert")?;
            lade(op, hat_speicher)?;
        }
        MemorySize => {
            if !hat_speicher {
                return Err(Fehler::ungueltig(V_SPEICHER, "memory.size ohne Speicher"));
            }
            op.push(I32);
        }
        MemoryGrow => {
            if !hat_speicher {
                return Err(Fehler::ungueltig(V_SPEICHER, "memory.grow ohne Speicher"));
            }
            op.pop_t(I32, "memory.grow-Seiten")?;
            op.push(I32);
        }
        MemoryInit(i) => {
            if !hat_speicher {
                return Err(Fehler::ungueltig(V_SPEICHER, "memory.init ohne Speicher"));
            }
            // Der Datenindex muss im Raum liegen („unknown data segment").
            // GEMESSEN 02.10. (`memory_init.wast` Z. 129/147).
            if (*i as usize) >= c.m.daten.len() {
                return Err(Fehler::ungueltig(
                    V_DATEN,
                    format!(
                        "memory.init: Datenabschnitt {i} unbekannt ({} Abschnitte)",
                        c.m.daten.len()
                    ),
                ));
            }
            if c.m.daten_anzahl.is_none() {
                return Err(Fehler::ungueltig(
                    V_BULK,
                    "memory.init ohne DataCount-Abschnitt (Wasm 2.0 verlangt ihn, wenn memory.init benutzt wird)",
                ));
            }
            op.pop_t(I32, "memory.init-Laenge")?;
            op.pop_t(I32, "memory.init-Quelle")?;
            op.pop_t(I32, "memory.init-Ziel")?;
        }
        DataDrop(i) => {
            if (*i as usize) >= c.m.daten.len() {
                return Err(Fehler::ungueltig(
                    V_DATEN,
                    format!(
                        "data.drop: Datenabschnitt {i} unbekannt ({} Abschnitte)",
                        c.m.daten.len()
                    ),
                ));
            }
            if c.m.daten_anzahl.is_none() {
                return Err(Fehler::ungueltig(
                    V_BULK,
                    "data.drop ohne DataCount-Abschnitt",
                ));
            }
        }
        MemoryCopy { .. } | MemoryFill => {
            // Bei `memory.fill` gibt es nur einen Speicher (Index 0).
            let (zi, qu) = match b {
                MemoryCopy { ziel, quelle } => (*ziel, *quelle),
                _ => (0, 0),
            };
            let speicherpaar = match b {
                MemoryCopy { .. } => true,
                _ => false,
            };
            // Multi-Memory: beide Speicherindizes muessen existieren.
            //
            // GEMESSEN 03.10. (`memory_copy1.wast`).
            if speicherpaar {
                for (i, was) in [(zi, "Ziel"), (qu, "Quelle")] {
                    if i as usize >= c.m.speicher.len() {
                        return Err(Fehler::ungueltig(
                            V_SPEICHER,
                            format!("memory.copy: {was}-Speicher {i} unbekannt"),
                        ));
                    }
                }
            }
            if !hat_speicher {
                return Err(Fehler::ungueltig(
                    V_SPEICHER,
                    "memory.copy/fill ohne Speicher",
                ));
            }
            op.pop_t(I32, "Laenge")?;
            op.pop_t(I32, "Quelle")?;
            op.pop_t(I32, "Ziel")?;
        }
        TableInit { element, tabelle } => {
            let el = c.m.elemente.get(*element as usize).ok_or_else(|| {
                Fehler::ungueltig(V_ELEMENT, format!("table.init: Element {element}"))
            })?;
            let tt = c.m.tabellen.get(*tabelle as usize).ok_or_else(|| {
                Fehler::ungueltig(V_TABELLE, format!("table.init: Tabelle {tabelle}"))
            })?;
            if !crate::modell::reftyp_passt_als(c.m, &el.reftyp(), &tabelle_reftyp(tt)) {
                return Err(Fehler::ungueltig(
                    V_BULK,
                    "table.init: Elementtyp passt nicht zur Tabelle",
                ));
            }
            op.pop_t(I32, "table.init-Laenge")?;
            op.pop_t(I32, "table.init-Quelle")?;
            op.pop_t(I32, "table.init-Ziel")?;
        }
        ElemDrop(i) => {
            if (*i as usize) >= c.m.elemente.len() {
                return Err(Fehler::ungueltig(V_ELEMENT, format!("elem.drop {i}")));
            }
        }
        TableCopy { ziel, quelle } => {
            let z =
                c.m.tabellen.get(*ziel as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_TABELLE, format!("table.copy: Ziel {ziel}"))
                })?;
            let q = c.m.tabellen.get(*quelle as usize).ok_or_else(|| {
                Fehler::ungueltig(V_TABELLE, format!("table.copy: Quelle {quelle}"))
            })?;
            if tabelle_reftyp(z) != tabelle_reftyp(q) {
                return Err(Fehler::ungueltig(
                    V_BULK,
                    "table.copy: Referenztypen unterscheiden sich",
                ));
            }
            op.pop_t(I32, "table.copy-Laenge")?;
            op.pop_t(I32, "table.copy-Quelle")?;
            op.pop_t(I32, "table.copy-Ziel")?;
        }
        TableGrow(i) => {
            let tt =
                c.m.tabellen.get(*i as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_TABELLE, format!("table.grow: Tabelle {i}"))
                })?;
            let r = tabelle_reftyp(tt);
            op.pop_t(I32, "table.grow-Anzahl")?;
            op.pop_ref(r, "table.grow-Startwert")?;
            op.push(I32);
        }
        TableSize(i) => {
            if (*i as usize) >= c.m.tabellen.len() {
                return Err(Fehler::ungueltig(
                    V_TABELLE,
                    format!("table.size: Tabelle {i}"),
                ));
            }
            op.push(I32);
        }
        TableGet(i) => {
            let tt =
                c.m.tabellen.get(*i as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_TABELLE, format!("table.get: Tabelle {i}"))
                })?;
            op.pop_t(I32, "table.get-Index")?;
            op.push(Ref(tabelle_reftyp(tt)));
        }
        TableSet(i) => {
            let tt =
                c.m.tabellen.get(*i as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_TABELLE, format!("table.set: Tabelle {i}"))
                })?;
            op.pop_ref(tabelle_reftyp(tt), "table.set-Wert")?;
            op.pop_t(I32, "table.set-Index")?;
        }
        TableFill(i) => {
            let tt =
                c.m.tabellen.get(*i as usize).ok_or_else(|| {
                    Fehler::ungueltig(V_TABELLE, format!("table.fill: Tabelle {i}"))
                })?;
            let r = tabelle_reftyp(tt);
            op.pop_t(I32, "table.fill-Anzahl")?;
            op.pop_ref(r, "table.fill-Wert")?;
            op.pop_t(I32, "table.fill-Start")?;
        }
        I32Konst(_) => op.push(I32),
        I64Konst(_) => op.push(I64),
        F32Konst(_) => op.push(F32),
        F64Konst(_) => op.push(F64),
        _ => {
            if let Some((a, b, r)) = binaer_arithmetik(b) {
                op.pop_t(b, "zweiter Operand")?;
                op.pop_t(a, "erster Operand")?;
                op.push(r);
                return Ok(());
            }
            if let Some((a, r)) = gleich_arithmetik(b) {
                op.pop_t(a, "Operand")?;
                op.push(r);
                return Ok(());
            }
            if let Some((a, r)) = unar_arithmetik(b) {
                op.pop_t(a, "Operand")?;
                op.push(r);
                return Ok(());
            }
            return Err(Fehler::ungueltig(
                V_TYP_ERGEBNIS,
                format!("Befehl {b:?} im Validator nicht behandelt (unvollstaendig)"),
            ));
        }
    }
    // Ein `if` mit Ergebnissen braucht zwingend ein `else` — das prueft der
    // `end`-Zweig (siehe dort).
    Ok(())
}

/// Referenztyp einer Tabelle.
fn tabelle_reftyp(t: &ImportiertTab) -> Reftyp {
    match t {
        ImportiertTab::Importiert { typ, .. } => typ.reftyp,
        ImportiertTab::Eigen { typ, .. } => typ.reftyp,
    }
}

/// Lade-/Speicheroperanden: Adresse (i32) pruefen, Speicher muss da sein.
fn lade(op: &mut OpStapel<'_>, hat_speicher: bool) -> Result<(), Fehler> {
    if !hat_speicher {
        return Err(Fehler::ungueltig(
            V_SPEICHER,
            "Speicherzugriff ohne Speicher",
        ));
    }
    op.pop_t(Valtyp::I32, "Speicheradresse")
}

/// Zweioperandige Rechenbefehle: (links, rechts, Ergebnis).
fn binaer_arithmetik(b: &Befehl) -> Option<(Valtyp, Valtyp, Valtyp)> {
    use Befehl::*;
    use Valtyp::*;
    let (i, l, f, d) = (I32, I64, F32, F64);
    Some(match b {
        I32Eq | I32Ne | I32LtS | I32LtU | I32GtS | I32GtU | I32LeS | I32LeU | I32GeS | I32GeU => {
            (i, i, i)
        }
        I64Eq | I64Ne | I64LtS | I64LtU | I64GtS | I64GtU | I64LeS | I64LeU | I64GeS | I64GeU => {
            (l, l, i)
        }
        F32Eq | F32Ne | F32Lt | F32Gt | F32Le | F32Ge => (f, f, i),
        F64Eq | F64Ne | F64Lt | F64Gt | F64Le | F64Ge => (d, d, i),
        I32Add | I32Sub | I32Mul | I32DivS | I32DivU | I32RemS | I32RemU | I32And | I32Or
        | I32Xor | I32Shl | I32ShrS | I32ShrU | I32Rotl | I32Rotr => (i, i, i),
        I64Add | I64Sub | I64Mul | I64DivS | I64DivU | I64RemS | I64RemU | I64And | I64Or
        | I64Xor | I64Shl | I64ShrS | I64ShrU | I64Rotl | I64Rotr => (l, l, l),
        F32Add | F32Sub | F32Mul | F32Div | F32Min | F32Max | F32Copysign => (f, f, f),
        F64Add | F64Sub | F64Mul | F64Div | F64Min | F64Max | F64Copysign => (d, d, d),
        _ => return None,
    })
}

/// `i32.eqz`/`i64.eqz`: die einzigen einoperandigen Vergleiche.
/// GEMESSEN 01.10. (`i32.wast` F20): sie gehoeren in die EINE-OPERAND-
/// Tabelle, nicht in die Zwei-Operand-Tabelle — sonst poppt der Validator
/// zwei Werte und meldet zu frueh „Stapelunterlauf".
fn gleich_arithmetik(b: &Befehl) -> Option<(Valtyp, Valtyp)> {
    use Befehl::*;
    use Valtyp::*;
    Some(match b {
        I32Eqz => (I32, I32),
        I64Eqz => (I64, I32),
        _ => return None,
    })
}

/// Einoperandige Rechenbefehle und Konvertierungen: (Eingabe, Ergebnis).
fn unar_arithmetik(b: &Befehl) -> Option<(Valtyp, Valtyp)> {
    use Befehl::*;
    use Valtyp::*;
    Some(match b {
        I32Clz | I32Ctz | I32Popcnt | I32Extend8S | I32Extend16S => (I32, I32),
        I64Clz | I64Ctz | I64Popcnt | I64Extend8S | I64Extend16S | I64Extend32S => (I64, I64),
        F32Abs | F32Neg | F32Ceil | F32Floor | F32Trunc | F32Nearest | F32Sqrt => (F32, F32),
        F64Abs | F64Neg | F64Ceil | F64Floor | F64Trunc | F64Nearest | F64Sqrt => (F64, F64),
        I32WrapI64 => (I64, I32),
        I32TruncF32S | I32TruncF32U | I32TruncSatF32S | I32TruncSatF32U => (F32, I32),
        I32TruncF64S | I32TruncF64U | I32TruncSatF64S | I32TruncSatF64U => (F64, I32),
        I64ExtendI32S | I64ExtendI32U => (I32, I64),
        I64TruncF32S | I64TruncF32U | I64TruncSatF32S | I64TruncSatF32U => (F32, I64),
        I64TruncF64S | I64TruncF64U | I64TruncSatF64S | I64TruncSatF64U => (F64, I64),
        F32ConvertI32S | F32ConvertI32U => (I32, F32),
        F32ConvertI64S | F32ConvertI64U => (I64, F32),
        F32DemoteF64 => (F64, F32),
        F64ConvertI32S | F64ConvertI32U => (I32, F64),
        F64ConvertI64S | F64ConvertI64U => (I64, F64),
        F64PromoteF32 => (F32, F64),
        I32ReinterpretF32 => (F32, I32),
        I64ReinterpretF64 => (F64, I64),
        F32ReinterpretI32 => (I32, F32),
        F64ReinterpretI64 => (I64, F64),
        _ => return None,
    })
}
fn pruefe_start(c: &Ctx) -> Result<(), Fehler> {
    let idx = match c.m.start {
        None => return Ok(()),
        Some(i) => i,
    };
    let ft =
        c.m.signatur(idx)
            .ok_or_else(|| Fehler::ungueltig(V_START, format!("Startfunktion {idx}")))?;
    if !ft.parameter.is_empty() || !ft.ergebnis.is_empty() {
        return Err(Fehler::ungueltig(
            V_START,
            format!(
                "Startfunktion {idx}: () -> () erwartet, {} -> {}",
                ft.parameter.len(),
                ft.ergebnis.len()
            ),
        ));
    }
    Ok(())
}

fn pruefe_exporte(c: &Ctx) -> Result<(), Fehler> {
    let m = c.m;
    // Exportnamen muessen paarweise verschieden sein (Spec: „duplicate
    // export name" — eine VALIDIERUNGSregel, nicht erst beim Linken).
    // GEMESSEN 02.10. (`exports.wast`, 19 Faelle).
    let mut namen: BTreeSet<&str> = BTreeSet::new();
    for e in &m.exporte {
        if !namen.insert(e.name.as_str()) {
            return Err(Fehler::ungueltig(
                V_EXPORT,
                format!("Exportname {:?} doppelt", e.name),
            ));
        }
    }
    for e in &m.exporte {
        let ok = match e.art {
            Art::Func(i) => (i as usize) < m.funktionen.len(),
            Art::Tabelle(i) => (i as usize) < m.tabellen.len(),
            Art::Speicher(i) => (i as usize) < m.speicher.len(),
            Art::Global(i) => (i as usize) < m.globale.len(),
            Art::Tag(i) => (i as usize) < m.tags.len(),
        };
        if !ok {
            return Err(Fehler::form(
                ART_INDEX,
                format!(
                    "Export {:?} zeigt auf Index {} (ausserhalb des Raums)",
                    e.art.text(),
                    e.art.index()
                ),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `ref.func`-Deklarationen einsammeln (C.refs-Regel)
// ---------------------------------------------------------------------------
/// Baut die Menge der Funktionen, die `ref.func x` verwenden duerfen:
/// deklariert in einem Elementabschnitt, exportiert, importiert oder in
/// einem globalen Startausdruck benutzt.
pub fn sammle_refs(m: &Modul) -> BTreeSet<u32> {
    let mut refs = BTreeSet::new();
    for e in &m.elemente {
        match e {
            Element::Aktiv { eintraege, .. }
            | Element::Passiv { eintraege, .. }
            | Element::Deklarativ { eintraege, .. } => {
                for aus in eintraege {
                    for b in aus {
                        if let Befehl::RefFunc(f) = b {
                            refs.insert(*f);
                        }
                    }
                }
            }
        }
    }
    for e in &m.exporte {
        if let Art::Func(i) = e.art {
            refs.insert(i);
        }
    }
    for f in &m.funktionen {
        if let Func::Importiert { .. } = f {
            let _ = f;
        }
    }
    for g in &m.globale {
        if let Some(code) = &g.start {
            for b in code {
                if let Befehl::RefFunc(f) = b {
                    refs.insert(*f);
                }
            }
        }
    }
    // Importierte Funktionen sind immer deklariert.
    for (i, f) in m.funktionen.iter().enumerate() {
        if matches!(f, Func::Importiert { .. }) {
            refs.insert(i as u32);
        }
    }
    refs
}

#[cfg(test)]
mod einheit {
    use super::*;
    use crate::binaer::lies_modul;
    use crate::modell::Func;
    use crate::modell::Global;

    fn g() -> Grenzen {
        Grenzen::standard()
    }

    /// M7: gueltiges Modul muss durchgehen.
    #[test]
    fn gueltig() {
        let bytes: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // Kopf
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Typ: () -> i32
            0x03, 0x02, 0x01, 0x00, // eine Funktion
            0x07, 0x05, 0x01, 0x01, b'f', 0x00, 0x00, // Export "f"
            0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code: i32.const 42, end
        ];
        let m = lies_modul(&bytes, &g()).unwrap();
        assert!(modul(&m, &g()).is_ok(), "{:?}", modul(&m, &g()));
    }

    /// M8: ungueltige Module -> benannte Validatorfehler.
    #[test]
    fn ungueltige_stapeltypen() {
        // (func (result i32) i64.const 1 end)
        let bytes: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // Kopf
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Typ () -> i32
            0x03, 0x02, 0x01, 0x00, // Funktion 0 vom Typ 0
            0x0a, 0x06, 0x01, 0x04, 0x00, 0x42, 0x01, 0x0b, // Code: i64.const 1, end
        ];
        let m = lies_modul(&bytes, &g()).unwrap();
        let e = modul(&m, &g()).unwrap_err();
        // Der Fehler nennt genau die Stelle und die beiden Typen.
        assert_eq!(e.art(), V_TYP_OPERAND, "{e}");
        assert!(e.kurz().contains("i32 erwartet"), "{e}");
        assert!(e.kurz().contains("i64"), "{e}");
    }

    #[test]
    fn unbekannte_indizes() {
        let mut m = Modul::leer();
        m.funktionen.push(Func::Importiert {
            modul: "a".into(),
            feld: "b".into(),
            typ: 7,
        });
        let e = modul(&m, &g()).unwrap_err();
        assert_eq!(e.art(), V_TYP);
    }

    #[test]
    fn global_set_auf_konstante() {
        let mut m = Modul::leer();
        m.typen.push(FuncTyp {
            parameter: vec![],
            ergebnis: vec![],
        });
        m.globale.push(Global {
            typ: Valtyp::I32,
            veraenderbar: false,
            start: Some(vec![Befehl::I32Konst(1), Befehl::End]),
            import: None,
        });
        m.funktionen.push(Func::Eigen {
            typ: 0,
            lokale: vec![],
            koerper: vec![Befehl::I32Konst(1), Befehl::GlobalSet(0), Befehl::End],
            befehl_offset: 0,
        });
        let e = modul(&m, &g()).unwrap_err();
        assert_eq!(e.art(), V_GLOBAL, "{e}");
    }
}
