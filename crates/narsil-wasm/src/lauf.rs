//! W2: Laufzeit — Store, Instanz, Fremdfunktionen.
//!
//! ENTWURF (nach einer ersten Fassung mit `Rc<Speicher>` verworfen):
//! Der Store BESITZT Speicher, Tabellen, Globale, Daten- und
//! Elementabschnitte. Warum: `memory.grow`/`table.grow` brauchen `&mut`.
//! Mit `Rc` waeren die Instanzen weitere Zaehler und `Rc::get_mut`
//! schluege dann fehl (gemessen: der erste Entwurf lief genau dort in
//! einen immer wiederkehrenden -1). Der Store ist der einzige Besitzer;
//! eine Instanz merkt sich nur die Indizes.
//!
//! Aufrufe laufen ueber einen EXPLIZITEN Rahmenstapel (`interp::Rahmen`),
//! nicht ueber Rust-Rekursion (M5/M6). Nur Hostfunktionen duerfen wieder
//! WASM aufrufen; das ist ueber `Grenzen::einbettung` begrenzt.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::fehler::*;
use crate::grenzen::Grenzen;
use crate::js::{ImportQuelle, ImportWert};
use crate::modell::typ_ungueltig;
use crate::modell::{
    Art, Daten, Element, Func, FuncTyp, ImportiertSpeicher, ImportiertTab, Modul, Reftyp, Valtyp,
};
use crate::speicher::Speicher;
use crate::tabelle::Tabelle;
use crate::wert::{Ref, Wert};

/// Die Indexraeume EINER Instanz — der Ort, an dem eine Funktion
/// definiert ist.
///
/// WICHTIG (GEMESSEN 03.10., `linking1.wast` / `linking.wast`): eine
/// IMPORTIERTE Funktion laeuft im Kontext des DEFINIERENDEN Moduls, nicht
/// im Kontext des Aufrufers. Sonst las eine aus `$Mm` importierte
/// Ladefunktion den Speicher des aufrufenden Moduls (17 Faelle fielen aus:
/// erwartet 0xa7, geliefert 0xf2).
/// Die Gruppennormalform fuer **jeden** Typindex des Moduls.
///
/// M3/M5: `gruppen_normalform` hat selbst eine harte Tiefengrenze, ein
/// zyklischer Typ haengt hier nicht.
fn ziel_gruppen(m: &Modul) -> Vec<crate::modell::Gruppenform> {
    (0..m.typen.len())
        .map(|t| crate::modell::gruppen_normalform(m, t as u32).map(|(g, p, _)| (g, p)))
        .collect()
}

#[derive(Debug, Clone)]
pub struct ModulKontext {
    /// Nummer der Instanz (Identitaet).
    pub nr: u32,
    /// Speicherindizes des Moduls (lokaler Index -> Store-Index).
    pub speicher: Vec<u32>,
    /// Tabellenindizes des Moduls.
    pub tabellen_index: Vec<u32>,
    /// Standardtabelle fuer `call_indirect` ohne Argument.
    pub tabelle: u32,
    /// Typraum des Moduls.
    pub typen: Arc<Vec<FuncTyp>>,
    /// Die `(sub …)`-Kette des Moduls (Index -> Obertyp).
    ///
    /// GEMESSEN 03.10. (`type-subtyping.wast` Z. 283ff, „Runtime types",
    /// 7 Faelle): `call_indirect` muss pruefen, ob der **Typindex** der
    /// aufgerufenen Funktion ueber der Kette unter dem angegebenen
    /// Typindex liegt (`$t1 ⊑ $t0`). Der Laufzeitkontext hatte nur die
    /// Signaturen, nicht die Kette.
    pub sub_ober: Arc<Vec<Option<u32>>>,
    /// Je Typindex: ist der Typ **final**? Ein Typ ist final, wenn er
    /// nicht als `(sub …)` deklariert wurde oder ausdruecklich als
    /// `(sub final …)`.
    ///
    /// Das braucht der **Laufzeit**-Vergleich von `call_indirect`: ein
    /// finaler Typ ist mit einem strukturgleichen **nicht-finalen** kein
    /// Subtyp — in **beiden** Richtungen.
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 344ff, 2 Faelle
    /// „Trap erwartet, keiner"): `$t1 (sub (func))` und
    /// `$t2 (sub final (func))` sind beide `(func)`; der reine
    /// Signaturvergleich nahm sie fuer gleich und rief, statt zu
    /// trappen. Ausserhalb des Moduls (Import, Wasm 2.0) gilt
    /// `true` — dort ist alles final.
    pub typ_final: Arc<Vec<bool>>,
    /// Je Typindex des Moduls: die **Normalform seiner `(rec …)`-Gruppe**
    /// und die eigene Position darin (`None` ausserhalb jeder Gruppe).
    ///
    /// Das ist die Zielseite von `ref.test`/`ref.cast`: der Wert bringt
    /// seine Gruppenform aus `Store::funktion_gruppe` mit, das Ziel steht
    /// hier. Ohne diese Tabelle entschied `ref.test` nur ueber den
    /// Typindex und lieferte dort 0 statt 1, wo die Suite 1 verlangt.
    ///
    /// GEMESSEN 04.10. (`type-subtyping.wast` Z. 476ff und 492ff,
    /// 6 Faelle): `$f11 <: $f21` gilt, obwohl beide Typen in
    /// **verschiedenen** Gruppen stehen — entscheidet die
    /// **Gruppennormalform**, in der ein gruppeninterner Obertyp
    /// (`Ziel::Intern`) von einem gruppenfremden (`Ziel::Extern`)
    /// unterscheidbar bleibt.
    pub ziel_gruppe: Arc<Vec<crate::modell::Gruppenform>>,
    /// Typindex je Funktion, **nach Store-Index** (der Interpreter kennt
    /// nur den Store-Index).
    pub fn_typ: Arc<Vec<Option<u32>>>,
    /// Array- und Strukturtypen des Moduls (Index wie `typen`).
    ///
    /// GEMESSEN 03.10. (`array.wast`: ohne sie kann `array.new_default`
    /// die Standardwerte nicht fuellen).
    pub arrays: Arc<Vec<Option<crate::modell::ArrayTyp>>>,
    pub strukt: Arc<Vec<Option<crate::modell::StructTyp>>>,
    /// Indexraum der globalen Variablen (lokaler Index -> Store-Index).
    pub globale_index: Arc<Vec<u32>>,
    /// Indexraum der Funktionen (lokaler Index -> Store-Index).
    pub funktionen: Arc<Vec<u32>>,
    /// Startindex der Datenabschnitte.
    pub daten_start: u32,
    /// Startindex der Elementabschnitte.
    pub elemente_start: u32,
    /// Indexraum der Tags.
    pub tags: Arc<Vec<u32>>,
}

/// Typindex je **Store-Index** einer Funktion (lokaler Index -> Store-Index
/// ist `Instanz::funktionen`).
///
/// GEMESSEN 03.10.: `call_indirect` kennt nur den Store-Index; ohne diese
/// Tabelle kann es die `(sub …)`-Kette der aufgerufenen Funktion nicht
/// pruefen (`type-subtyping.wast` Z. 283ff).
fn typen_indizes(i: &Instanz) -> Vec<Option<u32>> {
    let mut raus: Vec<Option<u32>> = Vec::new();
    for (lokal, store) in i.funktionen.iter().enumerate() {
        let s = *store as usize;
        if raus.len() <= s {
            raus.resize(s + 1, None);
        }
        if let Some(f) = i.modul.funktionen.get(lokal) {
            // Nur EIGENE Funktionen: der Typindex einer importierten
            // Funktion gehoert zum definierenden Modul, dessen `(sub …)`
            //-Kette hier nicht vorliegt. Fuer sie bleibt der
            // Strukturvergleich (GEMESSEN 03.10.).
            if !matches!(f, crate::modell::Func::Importiert { .. }) {
                raus[s] = Some(crate::modell::funktion_typindex(f));
            }
        }
    }
    raus
}

impl ModulKontext {
    /// Kontext einer Instanz (Werte sind alle schon angelegt).
    pub fn von_instanz(i: &Instanz) -> ModulKontext {
        ModulKontext {
            nr: i.nr,
            speicher: i.speicher.clone(),
            tabellen_index: i.tabellen.clone(),
            tabelle: i.tabellen.first().copied().unwrap_or(0),
            typen: Arc::new(i.modul.typen.clone()),
            sub_ober: Arc::new(i.modul.sub_ober.clone()),
            typ_final: Arc::new(
                (0..i.modul.typen.len())
                    .map(|t| {
                        !i.modul.sub_ge_deklariert.get(t).copied().unwrap_or(false)
                            || i.modul.sub_final.get(t).copied().unwrap_or(false)
                    })
                    .collect(),
            ),
            ziel_gruppe: Arc::new(ziel_gruppen(&i.modul)),
            fn_typ: Arc::new(typen_indizes(i)),
            arrays: Arc::new(i.modul.arrays.clone()),
            strukt: Arc::new(i.modul.strukt.clone()),
            globale_index: Arc::new(i.globale_index.clone()),
            funktionen: Arc::new(i.funktionen.clone()),
            daten_start: i.daten_start,
            elemente_start: i.elemente_start,
            tags: Arc::new(i.tags.clone()),
        }
    }
}

/// Eine Funktion im Store.
pub enum Funktion {
    /// WASM-Funktion: Typ, Lokaltypen, Code.
    Eigen {
        typ: FuncTyp,
        lokale: Vec<Valtyp>,
        koerper: Arc<Vec<crate::op::Befehl>>,
        /// Sprungziele des Koerpers: EINMAL je Funktion beim
        /// Instanziieren gebaut (GEMESSEN 04.10., Hebel 1) — vorher
        /// wurde die Tabelle bei jedem Aufruf neu gebaut.
        sprung: Arc<crate::interp::Sprungtabelle>,
        /// Index des Funktionsraums im Modul (fuer den Debugger).
        raum: u32,
    },
    /// Hostfunktion: Signatur + Rust-Funktion.
    Fremd { typ: FuncTyp, funktion: FremdFn },
}

impl std::fmt::Debug for Funktion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Funktion::Eigen {
                typ,
                lokale,
                koerper,
                raum,
                ..
            } => write!(
                f,
                "Eigen(raum {raum}, {} Parameter, {} Lokale, {} Befehle)",
                typ.parameter.len(),
                lokale.len(),
                koerper.len()
            ),
            Funktion::Fremd { typ, .. } => {
                write!(
                    f,
                    "Fremd({} Parameter, {} Ergebnisse)",
                    typ.parameter.len(),
                    typ.ergebnis.len()
                )
            }
        }
    }
}

/// Rust-Funktion fuer einen Import. Bekommt die Argumente und den `Lauf`
/// (fuer Speicher, Globale und Gegenrufe aus der JS-Welt).
///
/// The second parameter is the embedder's [`Host`]: a host function that
/// needs the embedder (the JS engine calling a JS import) reaches it there.
/// Functions that do not need it ignore it.
///
/// `Arc` and `Send + Sync` (2026-10-15): the store lives inside the JS
/// engine, and that engine moves to its big-stack thread for every run.
pub type FremdFn =
    Arc<dyn Fn(&mut Lauf, &mut dyn Host, &[Wert]) -> Result<Vec<Wert>, Fehler> + Send + Sync>;

/// The embedder seen from a running wasm call.
///
/// The JS engine runs wasm from inside one of its natives, so it cannot hand
/// itself to an import closure that was created earlier. Instead every call
/// entry point (`rufe_index_host`, `instanziieren_host`) takes the embedder
/// as `&mut dyn Host`, the interpreter passes it down to every host call, and
/// an import closure calls `host.call_import(..)` with its own slot number.
/// Entry points without a host use [`NoHost`].
pub trait Host {
    /// Call the embedder's import number `slot` with wasm values.
    fn call_import(&mut self, lauf: &mut Lauf, slot: u32, args: &[Wert])
    -> Result<Vec<Wert>, Fehler>;
}

/// The host of calls that have no embedder (tests, the spec suite, the
/// debugger): every embedder import traps.
pub struct NoHost;

impl Host for NoHost {
    fn call_import(
        &mut self,
        _lauf: &mut Lauf,
        slot: u32,
        _args: &[Wert],
    ) -> Result<Vec<Wert>, Fehler> {
        Err(Fehler::trap(
            T_EINBETTUNG,
            format!("host import {slot} called without an embedder"),
        ))
    }
}

/// Besitzender Zustand aller Instanzen.
#[derive(Debug, Default)]
pub struct Store {
    pub speicher: Vec<Speicher>,
    pub tabellen: Vec<Tabelle>,
    pub globale: Vec<Wert>,
    /// Deklarierter Typ je globaler Variable (ein Referenzwert traegt zur
    /// Laufzeit keinen Typindex, der Importvergleich braucht ihn aber).
    pub globale_typen: Vec<Valtyp>,
    /// Veraenderbarkeit je globaler Variable (Link-Regel: ein veraenderbarer
    /// Import braucht einen veraenderbaren Export).
    pub globale_veraenderbar: Vec<bool>,
    /// Echte Zelle je globaler Variable: `globale[i]` ist ein NAME, hinter
    /// dem die Zelle `globale_zelle[i]` liegt. Zwei Importe derselben
    /// exportierten Variablen zeigen auf DIESELBE Zelle — nur so ist ein
    /// veraenderbarer Import wirklich geteilt.
    ///
    /// GEMESSEN 02.10. (`instance.wast`: `global.set` ueber Import A war
    /// ueber Import B nicht sichtbar — 3 Faelle).
    pub globale_zelle: Vec<u32>,
    /// Kontext (Indexraeume) je Funktion im Store — parallel zu
    /// `funktionen`. Wird beim Instanziieren gefuellt; ein importierter
    /// Aufruf nutzt den Kontext des DEFINIERENDEN Moduls.
    ///
    /// GEMESSEN 03.10. (`linking1.wast`).
    pub funktion_kontext: Vec<Option<Arc<ModulKontext>>>,
    /// Uebersetzte flache Programme je Store-Index (Hebel 3,
    /// `schnell::Status`). Store-Indizes wachsen beim Instanziieren
    /// monoton, die Kontexte werden einmal gesetzt — ein Eintrag wird
    /// deshalb nie ungueltig und muss nicht invalidiert werden.
    pub schnell: Vec<crate::schnell::Status>,
    /// GC-Objekte (Wasm 3.0 GC). Unsere Arrays speichern nur ihren Typ und
    /// ihre Laenge — die ELEMENTSPEICHER der Suite-Dateien `array*.wast`
    /// liegen ausserhalb des 2.0-Umfangs (`gehoert_2_0`).
    ///
    /// GEMESSEN 03.10. (`table_init.wast` Z. 790: 1 Fall — dort wird nur
    /// `array.new_default` mit Laenge 0 und `ref.eq` gebraucht).
    pub gc: Vec<crate::modell::GcObjekt>,
    /// Gebuchte Zahl der Array-Elemente im ganzen Haufen (M3: Eingaben
    /// duerfen den Speicher nicht unbegrenzt allozieren).
    ///
    /// GEMESSEN 03.10. (KOORDINATION): ohne diese Buchung brauchte der
    /// Bombentest 4,5 GB.
    pub gc_slots: usize,
    /// Signaturen der Tags im gemeinsamen Indexraum (Exception Handling
    /// ohne `try`/`catch`: ein Tag ist nur eine Signatur).
    ///
    /// GEMESSEN 02.10. (`imports.wast` Z. 0/10).
    pub tags: Vec<crate::modell::FuncTyp>,
    pub funktionen: Vec<Arc<Funktion>>,
    /// Je **Store-Funktion**: ihre `(rec …)`-Gruppe (alle Mitglieds-
    /// Signaturen in Reihenfolge) und die Position des Mitglieds in der
    /// Gruppe — `None` fuer Typen ausserhalb jeder Gruppe.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast` Z. 148/156, 2 `assert_unlinkable`):
    /// der Importvergleich muss **iso-rekursiv** sein. Der Exporttyp ist
    /// `(rec (type (func)) (type (struct)))`; die beiden unlinkbaren
    /// Importe bringen `(rec (type (struct)) (type (func)))` (andere
    /// Reihenfolge) und `(rec (type (func)))` (unvollstaendig). Beide sind
    /// als Signatur `(func)` nicht von ihm zu unterscheiden.
    pub funktion_gruppe: Vec<crate::modell::Gruppenform>,
    /// Passive/aktive Datenabschnitte; `None` = verworfen (`data.drop`).
    pub daten: Vec<Option<Vec<u8>>>,
    /// Elementabschnitte als Referenzlisten; `None` = verworfen.
    pub elemente: Vec<Option<Vec<Option<Ref>>>>,
    /// Hostobjekte fuer `externref` (W4): vom Host verwaltete Tabelle.
    pub hostobjekte: Vec<Hostobjekt>,
}

/// Ein Hostobjekt (die Welt von aussen).
#[derive(Debug, Clone, PartialEq)]
pub enum Hostobjekt {
    Zahl(f64),
    Text(String),
    /// Sonstiges; die JS-Seite haelt die eigentlichen Daten.
    Sonstiges(u32),
}

impl Store {
    /// M3/M8: `slots` Array-Elemente auf dem Haufen buchen. Ueber dem
    /// Budget: **benannter** Trap statt eines Absturzes durch den
    /// Wachhund.
    ///
    /// GEMESSEN 03.10. (`array.wast`, KOORDINATION-Meldung).
    pub fn gc_slots_buchen(&mut self, slots: usize) -> Result<(), Fehler> {
        if self.gc.len() >= crate::grenzen::MAX_GC_OBJEKTE_HART {
            return Err(Fehler::trap(
                T_INDIREKT,
                format!(
                    "GC-Haufen: mehr als {} Objekte (MAX_GC_OBJEKTE_HART)",
                    crate::grenzen::MAX_GC_OBJEKTE_HART
                ),
            ));
        }
        let neu = self.gc_slots.saturating_add(slots);
        if neu > crate::grenzen::MAX_GC_SLOTS_HART {
            return Err(Fehler::trap(
                T_INDIREKT,
                format!(
                    "GC-Haufen: {neu} Slots > {} (MAX_GC_SLOTS_HART)",
                    crate::grenzen::MAX_GC_SLOTS_HART
                ),
            ));
        }
        self.gc_slots = neu;
        Ok(())
    }
}

/// Der Zustand einer Ausfuehrung: Store + Brennstoff + Tiefe.
pub struct Lauf {
    pub store: Store,
    pub grenzen: Grenzen,
    /// Gesamtverbrauch (nur Messzwecke/Status).
    pub verbraucht: u64,
    /// Aktuelle Host-Einbettungstiefe (M5: begrenzt).
    pub einbettung: u32,
    /// Aktiver Speicherindex der aufrufenden Instanz (`memory.*`).
    pub speicher_idx: Option<u32>,
    /// Laufende Nummer der Instanzen (Identitaet, nicht Index).
    ///
    /// WICHTIG (GEMESSEN 03.10., `linking.wast` Z. 494ff): zwei Module
    /// ohne Speicher und ohne globale Variablen sind in ihren Indexraeumen
    /// nicht unterscheidbar — der Rahmen einer importierten Funktion
    /// brauchte aber genau diese Identitaet, um den Kontext des
    /// DEFINIERENDEN Moduls zu nehmen (sonst las sie den Tisch des
    /// Aufrufers: 0xf2 statt 0x02, dann „indirect call type mismatch").
    pub instanz_zaehler: u32,
    /// Restbrennstoff (M3: Endlosschleifen enden mit benanntem Fehler).
    pub brennstoff: u64,
    /// W5: Spur der Ausfuehrung (ein Schritt je Befehl). `Some` heisst
    /// „Debugger haengt dran" — der Interpreter schreibt dann nach jedem
    /// Befehl einen Schnappschuss. Ohne Debugger ist `None`, und der
    /// Interpreter prueft das mit EINEM Vergleich je Befehl.
    pub spur: Option<crate::debug::Spur>,
}

/// Typ eines Wertes fuer den Store (ohne Typindex — der ergibt sich erst
/// aus der Deklaration).
/// Bezug fuer `global.get` in einem konstanten Ausdruck.
///
/// GEMESSEN 03.10. (`global.wast` Z. 633ff, 6 Faelle): `global.get` ohne
/// Indexraum las die globale Variable eines FRUEHEREN Moduls (666 statt 4).
enum Bezug<'a> {
    /// Die fertige Instanz (`globale_index` = Indexraum des Moduls).
    Instanz(&'a Instanz),
    /// Der Indexraum der bisher angelegten globalen Variablen - fuer den
    /// Startwert der naechsten Variablen.
    Index(&'a [u32]),
}

fn wert_stufe_typ(w: &Wert) -> Valtyp {
    w.typ()
}

impl Lauf {
    pub fn neu(grenzen: Grenzen) -> Lauf {
        let brennstoff = grenzen.brennstoff;
        Lauf {
            store: Store::default(),
            grenzen,
            verbraucht: 0,
            einbettung: 0,
            speicher_idx: None,
            instanz_zaehler: 0,
            brennstoff,
            spur: None,
        }
    }

    /// Restbrennstoff setzen (Standard = `Grenzen::brennstoff`).
    pub fn setze_brennstoff(&mut self, b: u64) {
        self.brennstoff = b.min(self.grenzen.brennstoff);
    }

    /// Brennstoff verbrauchen; bei 0 Rest endet die Ausfuehrung mit dem
    /// BENANNTEN Brennstofffehler (M3: Endlosschleife statt Haenger).
    pub fn verbrauche(&mut self, n: u64) -> Result<(), Fehler> {
        if self.brennstoff < n {
            self.verbraucht += self.brennstoff;
            self.brennstoff = 0;
            return Err(Fehler::Brennstoff {
                verbraucht: self.verbraucht,
                grenze: self.grenzen.brennstoff,
            });
        }
        self.brennstoff -= n;
        self.verbraucht += n;
        Ok(())
    }

    /// Einen Befehl ZAEHLEN, ohne zu pruefen (Hebel 2, GEMESSEN
    /// 04.10.): die Pruefung passiert nur noch an
    /// Schleifenrueckspruengen und Aufrufen (`pruefe_brennstoff`) —
    /// `verbraucht` zaehlt wie vorher jeden Befehl.
    #[inline(always)]
    pub fn zaehle(&mut self, n: u64) {
        self.verbraucht += n;
        self.brennstoff = self.brennstoff.saturating_sub(n);
    }

    /// Brennstoff-Pruefung an einer Stelle, an der unendliche Schleifen
    /// nachweislich vorbeikommen: Ruecksprung (`br` & Verwandte) oder
    /// Aufruf. Zwei Pruefungen je Schleifeniteration statt einer
    /// Pruefung je Befehl (GEMESSEN 04.10., Hebel 2).
    ///
    /// Sicherheit: ohne Ruecksprung gibt es keine Endlosschleife — ein
    /// gerader Befehlsstrom ist hoechstens so lang wie ein Koerper.
    #[inline(always)]
    pub fn pruefe_brennstoff(&self) -> Result<(), Fehler> {
        if self.brennstoff == 0 {
            Err(Fehler::Brennstoff {
                verbraucht: self.verbraucht,
                grenze: self.grenzen.brennstoff,
            })
        } else {
            Ok(())
        }
    }

    pub fn speicher(&self) -> Option<&Speicher> {
        self.speicher_idx
            .and_then(|i| self.store.speicher.get(i as usize))
    }
    pub fn speicher_mut(&mut self) -> Option<&mut Speicher> {
        match self.speicher_idx {
            Some(i) => self.store.speicher.get_mut(i as usize),
            None => None,
        }
    }

    /// Neue Speicherinstanz anlegen (W4: `new WebAssembly.Memory`).
    pub fn neuer_speicher(&mut self, min: u64, max: Option<u64>) -> Result<u32, Fehler> {
        let s = Speicher::neu(min, max, &self.grenzen)?;
        self.store.speicher.push(s);
        Ok((self.store.speicher.len() - 1) as u32)
    }
    /// Neue Tabelle anlegen (`new WebAssembly.Table`).
    pub fn neue_tabelle(&mut self, min: u64, max: Option<u64>, r: Reftyp) -> Result<u32, Fehler> {
        let t = Tabelle::neu(min, max, r, &self.grenzen)?;
        self.store.tabellen.push(t);
        Ok((self.store.tabellen.len() - 1) as u32)
    }
    /// Die ZELLE hinter einem globalen Index (Importe teilen sich Zellen).
    pub fn zelle(&self, i: u32) -> Option<&Wert> {
        let z = *self.store.globale_zelle.get(i as usize)?;
        self.store.globale.get(z as usize)
    }

    /// Wie `zelle`, aber zum Schreiben.
    pub fn zelle_mut(&mut self, i: u32) -> Option<&mut Wert> {
        let z = *self.store.globale_zelle.get(i as usize)?;
        self.store.globale.get_mut(z as usize)
    }

    /// Neue globale Variable (`new WebAssembly.Global`).
    pub fn neue_globale(&mut self, wert: Wert, veraenderbar: bool) -> Result<u32, Fehler> {
        self.store.globale.push(wert);
        self.store.globale_typen.push(wert_stufe_typ(&wert));
        self.store.globale_veraenderbar.push(veraenderbar);
        // Eigene Zelle: der Index zeigt auf sich selbst.
        self.store
            .globale_zelle
            .push((self.store.globale.len() - 1) as u32);
        Ok((self.store.globale.len() - 1) as u32)
    }
    /// Neue Hostfunktion (`new WebAssembly.Function`).
    pub fn neue_funktion(&mut self, typ: FuncTyp, f: FremdFn) -> u32 {
        self.store
            .funktionen
            .push(Arc::new(Funktion::Fremd { typ, funktion: f }));
        // Eine Hostfunktion gehoert zu keinem Modul, also hat sie auch
        // keine `(rec …)`-Gruppe. Der Platzhalter `None` ist Pflicht:
        // ohne ihn laufen `funktionen` und `funktion_gruppe` auseinander
        // und der Importvergleich liest die Gruppe eines **anderen**
        // Funktions.
        //
        // GEMESSEN 03.10.: genau das war der Fehler — `type-rec.wast` #15
        // (ein gueltiger Import) wurde abgewiesen, weil die vier
        // spectest-Hostfunktionen die Vektoren verschoben hatten.
        self.store.funktion_gruppe.push(None);
        (self.store.funktionen.len() - 1) as u32
    }
    /// Hostobjekt anlegen (fuer `externref`).
    pub fn neues_hostobjekt(&mut self, o: Hostobjekt) -> u32 {
        self.store.hostobjekte.push(o);
        (self.store.hostobjekte.len() - 1) as u32
    }

    /// W5: Debugger anhaengen (Spur mit fester Obergrenze).
    pub fn spur_starten(&mut self, grenze: usize) {
        self.spur = Some(crate::debug::Spur::neu(grenze));
    }

    /// W5: Spur beenden (Speicher freigeben).
    pub fn spur_beenden(&mut self) {
        self.spur = None;
    }

    /// Ruft einen EXPORT auf (W2-Einstieg, W3-Testsuite).
    pub fn rufe_funktion(
        &mut self,
        i: &Instanz,
        name: &str,
        args: &[Wert],
    ) -> Result<Vec<Wert>, Fehler> {
        let (art, idx) = i
            .export(name)
            .ok_or_else(|| Fehler::link(L_EXPORT, format!("kein Export {name:?}")))?;
        match art {
            Art::Func(_) => self.rufe_index(i, idx, args),
            _ => Err(Fehler::link(
                L_EXPORT,
                format!("Export {name:?} ist keine Funktion"),
            )),
        }
    }

    /// Eine unterbrechbare Ausfuehrung starten (W5/D11: der Debugger
    /// haelt den Lauf wirklich an, statt nur eine Spur zu lesen).
    ///
    /// GEMESSEN 03.10. (`debug::Debugger::weiter_live`).
    pub fn sitzung(
        &mut self,
        i: &Instanz,
        name: &str,
        args: &[Wert],
    ) -> Result<crate::interp::Sitzung, Fehler> {
        let (art, idx) = i
            .export(name)
            .ok_or_else(|| Fehler::link(L_EXPORT, format!("kein Export {name:?}")))?;
        let Art::Func(_) = art else {
            return Err(Fehler::link(
                L_EXPORT,
                format!("Export {name:?} ist keine Funktion"),
            ));
        };
        self.sitzung_index(i, idx, args)
    }

    /// Wie `sitzung`, aber ueber den Funktionsindex im RAUM DER INSTANZ.
    ///
    /// GEMESSEN 03.10. (`debug::Debugger::weiter_live`).
    pub fn sitzung_index(
        &mut self,
        i: &Instanz,
        idx: u32,
        args: &[Wert],
    ) -> Result<crate::interp::Sitzung, Fehler> {
        self.setze_brennstoff(self.grenzen.brennstoff);
        // Lokaler Index -> Store (Importfunktionen liegen fremd).
        let eintr = i.funktions_index(idx).unwrap_or(i.funktions_start + idx);
        let f = self
            .store
            .funktionen
            .get(eintr as usize)
            .cloned()
            .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("Funktion {eintr} fehlt im Store")))?;
        crate::interp::Sitzung::starte(self, i, &f, args, eintr)
    }

    /// Ruft eine Funktion ueber ihren Funktionsindex im Store auf.
    pub fn rufe_index(
        &mut self,
        i: &Instanz,
        idx: u32,
        args: &[Wert],
    ) -> Result<Vec<Wert>, Fehler> {
        self.setze_brennstoff(self.grenzen.brennstoff);
        let vorher = self.verbraucht;
        // Lokaler Index -> Store (Importfunktionen liegen fremd)
        let eintr = i.funktions_index(idx).unwrap_or(i.funktions_start + idx);
        let f = self
            .store
            .funktionen
            .get(eintr as usize)
            .cloned()
            .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("Funktion {eintr} fehlt im Store")))?;
        let alt_speicher = self.speicher_idx;
        let r = crate::interp::aufrufen(self, i, &f, args, eintr);
        self.verbraucht = vorher + (self.grenzen.brennstoff - self.brennstoff);
        self.speicher_idx = alt_speicher;
        r
    }

    /// Call a function by its STORE index with an embedder (the JS API:
    /// exported functions, table entries, re-exported imports). Fuel is
    /// refilled per call, the active memory is restored afterwards (a host
    /// call in between may run other instances).
    pub fn rufe_store_host(
        &mut self,
        host: &mut dyn Host,
        eintr: u32,
        args: &[Wert],
    ) -> Result<Vec<Wert>, Fehler> {
        let f = self
            .store
            .funktionen
            .get(eintr as usize)
            .cloned()
            .ok_or_else(|| Fehler::trap(T_INDIREKT, format!("function {eintr} missing in store")))?;
        let brennstoff_alt = self.brennstoff;
        self.setze_brennstoff(self.grenzen.brennstoff);
        let vorher = self.verbraucht;
        let alt_speicher = self.speicher_idx;
        let r = crate::interp::aufrufen_store(self, host, &f, args, eintr);
        self.verbraucht = vorher + (self.grenzen.brennstoff - self.brennstoff);
        self.speicher_idx = alt_speicher;
        // A nested call (JS import -> export) must not refill the fuel of
        // the call around it.
        self.brennstoff = brennstoff_alt;
        r
    }

    /// Ist ein Debugger anhaengig?
    pub fn debug_anhaengig(&self) -> bool {
        self.spur.is_some()
    }

    /// Konstanter Ausdruck einer INSTANZ auswerten: `global.get` und
    /// `ref.func` nennen Indizes im Raum des Moduls, der Store kennt die
    /// umgerechneten. Ohne diese Umrechnung las ein Datenoffset die
    /// globale Variable eines FRUEHEREN Moduls (GEMESSEN 02.10.,
    /// `elem.wast`: „table.init 0+1 -> 666+1").
    pub fn konstanter_wert_in(
        &mut self,
        code: &[crate::op::Befehl],
        instanz: &Instanz,
    ) -> Result<Wert, Fehler> {
        let wert = self.konstanter_auswerten(code, Some(instanz))?;
        wert.ok_or_else(|| {
            Fehler::link(
                L_GLOBAL,
                "konstanter Ausdruck liefert keinen Wert".to_string(),
            )
        })
    }

    /// Ein konstanter Ausdruck mit Bezug auf einen Indexraum.
    ///
    /// GEMESSEN 03.10. (`global.wast` Z. 633ff: 6 Faelle): `global.get` in
    /// einem Ausdruck braucht den Indexraum des Moduls. `globale_start + i`
    /// ist falsch, sobald das Modul globale Variablen importiert — die
    /// Importe liegen im Store VOR `globale_start`.
    fn konstanter_wert_index(
        &mut self,
        code: &[crate::op::Befehl],
        globale_index: &[u32],
        strukt: &[Option<crate::modell::StructTyp>],
    ) -> Result<Wert, Fehler> {
        self.konstanter_auswerten_mit(code, Some(&Bezug::Index(globale_index)), strukt)?
            .ok_or_else(|| {
                Fehler::link(
                    L_GLOBAL,
                    "konstanter Ausdruck liefert keinen Wert".to_string(),
                )
            })
    }

    /// Ein konstanter Ausdruck, optional mit Instanzbezug (`global.get`
    /// und `ref.func` nennen dann Indizes im Raum des Moduls).
    fn konstanter_auswerten(
        &mut self,
        code: &[crate::op::Befehl],
        instanz: Option<&Instanz>,
    ) -> Result<Option<Wert>, Fehler> {
        let b = instanz.map(Bezug::Instanz);
        // Ohne Instanz gibt es keine Strukturtypen — die GC-Befehle
        // melden dann, statt leere Objekte anzulegen.
        let strukt: &[Option<crate::modell::StructTyp>] =
            instanz.map(|i| i.modul.strukt.as_slice()).unwrap_or(&[]);
        self.konstanter_auswerten_mit(code, b.as_ref(), strukt)
    }

    /// Wie `konstanter_auswerten`, aber mit waehlbarem Bezug fuer
    /// `global.get`: die fertige Instanz, der beim Anlegen der globalen
    /// Variablen bisher bekannte Teil-Indexraum - oder gar keiner.
    fn konstanter_auswerten_mit(
        &mut self,
        code: &[crate::op::Befehl],
        bezug: Option<&Bezug<'_>>,
        strukt: &[Option<crate::modell::StructTyp>],
    ) -> Result<Option<Wert>, Fehler> {
        let mut stapel: Vec<Wert> = Vec::new();
        for b in code {
            use crate::op::Befehl;
            match b {
                Befehl::I32Konst(v) => stapel.push(Wert::I32(*v)),
                Befehl::I64Konst(v) => stapel.push(Wert::I64(*v)),
                Befehl::F32Konst(v) => stapel.push(Wert::F32(*v)),
                Befehl::F64Konst(v) => stapel.push(Wert::F64(*v)),
                // `ref.null` ergibt `null`; der Typindex gehoert nur zur
                // Pruefung. Fuer `extern`-Typen ist der Wert eine externe
                // Referenz (GEMESSEN 03.10., `table.wast` Z. 30).
                Befehl::RefNull(r) => stapel.push(if r.ist_extern() {
                    Wert::Extern(None)
                } else {
                    Wert::Func(None)
                }),
                Befehl::RefFunc(f) => stapel.push(Wert::Func(Some(*f))),
                // Wasm 2.0: erweiterte konstante Ausdruecke
                // (`i32/i64 add|sub|mul`). GEMESSEN 02.10. (`elem.wast`).
                // ---- GC-Werte als konstante Ausdruecke -------------------
                // `extern.convert_any` / `any.convert_extern` /
                // `ref.i31` / `struct.new_default` sind im Startwert einer
                // globalen Variablen erlaubt.
                //
                // GEMESSEN 03.10. (`extern.wast` Z. 6/7/20:
                // „konstanter Ausdruck: ExternConvertAny ist nicht konstant").
                // Die IDENTITAET, mit EINER Ausnahme: `null` bekommt die
                // Darstellung der Zielart (GEMESSEN 03.10.,
                // `extern.wast` Z. 41/42 — sonst passt `(ref.null any)`
                // nicht auf einen externen Nullwert).
                Befehl::AnyConvertExtern => {
                    let w = pop_wert(&mut stapel)?;
                    stapel.push(match w {
                        Wert::Extern(None) => Wert::Func(None),
                        other => other,
                    });
                }
                Befehl::ExternConvertAny => {
                    let w = pop_wert(&mut stapel)?;
                    stapel.push(match w {
                        Wert::Func(None) | Wert::Gcref(None) => Wert::Extern(None),
                        other => other,
                    });
                }
                Befehl::RefI31 => {
                    // 31 Bit (siehe interp.rs).
                    let w = as_i32(pop_wert(&mut stapel)?)? & 0x7fff_ffff;
                    self.store.gc.push(crate::modell::GcObjekt {
                        typ: crate::modell::JEDER_I31TYP,
                        laenge: 1,
                        inhalt: crate::modell::GcInhalt::I31(w),
                    });
                    let id = (self.store.gc.len() - 1) as u32;
                    stapel.push(Wert::Gcref(Some(id)));
                }
                // `array.new_fixed` im Startwert (GEMESSEN 03.10.,
                // `array.wast` #13/#35).
                Befehl::ArrayNewFixed(t, n) => {
                    let anzahl = *n as usize;
                    if anzahl as u64 > crate::grenzen::MAX_GC_ELEMENTS_HART as u64 {
                        return Err(Fehler::trap(
                            T_DIVERSE,
                            format!("Arraylaenge {n} > MAX_GC_ELEMENTS_HART"),
                        ));
                    }
                    let mut werte: Vec<Wert> = Vec::with_capacity(anzahl);
                    for _ in 0..anzahl {
                        werte.push(pop_wert(&mut stapel)?);
                    }
                    werte.reverse();
                    self.store.gc_slots_buchen(anzahl)?;
                    self.store.gc.push(crate::modell::GcObjekt {
                        typ: *t,
                        laenge: *n,
                        inhalt: crate::modell::GcInhalt::Array(werte),
                    });
                    let id = (self.store.gc.len() - 1) as u32;
                    stapel.push(Wert::Gcref(Some(id)));
                }
                // `struct.new` im Startwert einer globalen Variablen
                // (GEMESSEN 03.10., `struct.wast`: „StructNew(0) ist kein
                // konstanter Ausdruck").
                Befehl::StructNew(t) => {
                    // Die Feldtypen kommen als Parameter herein — beim
                    // Startwert einer globalen Variablen gibt es noch
                    // KEINE fertige Instanz, die Instanz waere der
                    // falsche Ort (GEMESSEN 03.10., `struct.wast`:
                    // Objekte ganz ohne Felder).
                    let felder = strukt
                        .get(*t as usize)
                        .and_then(|x| x.as_ref())
                        .map(|st| st.felder.clone())
                        .unwrap_or_default();
                    let mut werte: Vec<Wert> = Vec::with_capacity(felder.len());
                    for i in (0..felder.len()).rev() {
                        let w = pop_wert(&mut stapel)?;
                        werte.push(match felder.get(i) {
                            Some(f) if f.bits > 0 => {
                                Wert::I32(crate::modell::kuerzen_und_vorzeichen(
                                    w.als_i32().unwrap_or(0),
                                    f.bits,
                                ))
                            }
                            _ => w,
                        });
                    }
                    werte.reverse();
                    self.store.gc.push(crate::modell::GcObjekt {
                        typ: *t,
                        laenge: werte.len() as u32,
                        inhalt: crate::modell::GcInhalt::Feld(werte),
                    });
                    let id = (self.store.gc.len() - 1) as u32;
                    stapel.push(Wert::Gcref(Some(id)));
                }
                // `array.new` / `array.new_default` im Startwert einer
                // globalen Variablen (GEMESSEN 03.10., `array.wast` Z. 64).
                Befehl::ArrayNew(t) | Befehl::ArrayNewDefault(t) => {
                    // `[t i32]`: erst die Laenge (oben), dann der Wert.
                    let laenge = as_i32(pop_wert(&mut stapel)?)? as u32;
                    if laenge as usize > crate::grenzen::MAX_GC_ELEMENTS_HART {
                        return Err(Fehler::trap(
                            T_DIVERSE,
                            format!(
                                "Arraylaenge {laenge} > {} (MAX_GC_ELEMENTS_HART)",
                                crate::grenzen::MAX_GC_ELEMENTS_HART
                            ),
                        ));
                    }
                    // Auch der Startwert einer globalen Variablen
                    // bucht im Haufen (GEMESSEN 03.10.: dieser Weg lief
                    // an der Grenze vorbei — der Wachhund sah 4,5 GB).
                    self.store.gc_slots_buchen(laenge as usize)?;
                    let wert = if matches!(b, Befehl::ArrayNew(_)) {
                        Some(pop_wert(&mut stapel)?)
                    } else {
                        None
                    };
                    let inhalt = match wert {
                        Some(w) => vec![w; laenge as usize],
                        None => vec![Wert::I32(0); laenge as usize],
                    };
                    self.store.gc.push(crate::modell::GcObjekt {
                        typ: *t,
                        laenge,
                        inhalt: crate::modell::GcInhalt::Array(inhalt),
                    });
                    let id = (self.store.gc.len() - 1) as u32;
                    stapel.push(Wert::Gcref(Some(id)));
                }
                Befehl::StructNewDefault(t) => {
                    self.store.gc.push(crate::modell::GcObjekt {
                        typ: *t,
                        laenge: 0,
                        inhalt: crate::modell::GcInhalt::Feld(Vec::new()),
                    });
                    let id = (self.store.gc.len() - 1) as u32;
                    stapel.push(Wert::Gcref(Some(id)));
                }
                Befehl::I32Add | Befehl::I32Sub | Befehl::I32Mul => {
                    let art = b;
                    let rechts = as_i32(pop_wert(&mut stapel)?)?;
                    let links = as_i32(pop_wert(&mut stapel)?)?;
                    stapel.push(Wert::I32(match art {
                        Befehl::I32Add => links.wrapping_add(rechts),
                        Befehl::I32Sub => links.wrapping_sub(rechts),
                        _ => links.wrapping_mul(rechts),
                    }));
                }
                Befehl::I64Add | Befehl::I64Sub | Befehl::I64Mul => {
                    let art = b;
                    let rechts = as_i64(pop_wert(&mut stapel)?)?;
                    let links = as_i64(pop_wert(&mut stapel)?)?;
                    stapel.push(Wert::I64(match art {
                        Befehl::I64Add => links.wrapping_add(rechts),
                        Befehl::I64Sub => links.wrapping_sub(rechts),
                        _ => links.wrapping_mul(rechts),
                    }));
                }
                Befehl::GlobalGet(i) => {
                    // Ueber den Indexraum der Instanz: Importe liegen im
                    // Store vor `globale_start` (GEMESSEN 03.10.,
                    // `global.wast` Z. 633ff — 6 Faelle).
                    let idx = match bezug {
                        Some(Bezug::Instanz(ins)) => {
                            ins.globale_index.get(*i as usize).copied().ok_or_else(|| {
                                Fehler::link(
                                    L_GLOBAL,
                                    format!("global.get {i}: unbekannte globale Variable"),
                                )
                            })? as usize
                        }
                        Some(Bezug::Index(raum)) => *raum.get(*i as usize).ok_or_else(|| {
                            Fehler::link(
                                L_GLOBAL,
                                format!(
                                    "global.get {i}: nur {} globale Variablen bisher",
                                    raum.len()
                                ),
                            )
                        })? as usize,
                        None => *i as usize,
                    };
                    let w = self.zelle(idx as u32).copied().ok_or_else(|| {
                        Fehler::link(
                            L_GLOBAL,
                            format!("global.get {i} im konstanten Ausdruck fehlt"),
                        )
                    })?;
                    stapel.push(w);
                }
                Befehl::End => {}
                andere => {
                    return Err(Fehler::link(
                        L_GLOBAL,
                        format!("konstanter Ausdruck: {andere:?} ist nicht konstant"),
                    ));
                }
            }
        }
        Ok(stapel.pop())
    }

    /// Konstanter Ausdruck auswerten (Globale, Element-/Datenoffset) OHNE
    /// Instanzbezug: `global.get` nennt dann den Index im Store.
    pub fn konstanter_wert(&mut self, code: &[crate::op::Befehl]) -> Result<Wert, Fehler> {
        self.konstanter_auswerten(code, None)?.ok_or_else(|| {
            Fehler::link(
                L_GLOBAL,
                "konstanter Ausdruck liefert keinen Wert".to_string(),
            )
        })
    }
}

// ---- Hilfen fuer die konstanten Ausdruecke ---------------------------------

/// Einen Wert vom Stapel nehmen (mit Pruefung der Reihenfolge).
fn pop_wert(stapel: &mut Vec<Wert>) -> Result<Wert, Fehler> {
    stapel
        .pop()
        .ok_or_else(|| Fehler::link(L_GLOBAL, "konstanter Ausdruck: Stapelunterlauf".to_string()))
}

/// Der Wert als `i32` (die Rechenbefehle duerfen nur Ganzzahlen sehen).
fn as_i32(w: Wert) -> Result<i32, Fehler> {
    match w {
        Wert::I32(v) => Ok(v),
        andere => Err(Fehler::link(
            L_GLOBAL,
            format!("konstanter Ausdruck: {andere:?} ist keine i32"),
        )),
    }
}

/// Der Wert als `i64`.
fn as_i64(w: Wert) -> Result<i64, Fehler> {
    match w {
        Wert::I64(v) => Ok(v),
        andere => Err(Fehler::link(
            L_GLOBAL,
            format!("konstanter Ausdruck: {andere:?} ist keine i64"),
        )),
    }
}

/// Eine Instanz: die Sicht auf den Store.
#[derive(Debug, Clone)]
pub struct Instanz {
    /// Eigene Nummer der Instanz (Identitaet — `Lauf::instanz_zaehler`).
    pub nr: u32,
    /// Speicherindizes des Moduls (lokaler Index -> Store-Index).
    /// WebAssembly 2.0: genau ein Eintrag (Index 0).
    pub speicher: Vec<u32>,
    /// Tabellenindizes.
    pub tabellen: Vec<u32>,
    /// Startindex der globalen Variablen.
    pub globale_start: u32,
    /// Indexraum der globalen Variablen des Moduls: lokaler Index ->
    /// Index im Store.
    ///
    /// WICHTIG (GEMESSEN 03.10., `global.wast` Z. 633ff): `globale_start + i`
    /// ist FALSCH, sobald das Modul globale Variablen IMPORTIERT — die
    /// Importe liegen im Store VOR `globale_start`, die eigenen dahinter.
    /// Ein Elementabschnitt mit Versatz `(global.get $g1)` schrieb deshalb
    /// an den Platz der importierten Variablen (666 statt 8:
    /// „table.init 0+1 -> 666+1", 6 Faelle fielen aus).
    pub globale_index: Vec<u32>,
    /// Indexraum der Tags (lokaler Index -> Index in `Store::tags`).
    ///
    /// WICHTIG (GEMESSEN 02.10., `instance.wast`): derselbe Tag darf nicht
    /// zweimal im Store liegen — sonst waeren zwei Importe desselben Tags
    /// verschiedene Objekte, und `catch $tag1` wuerde ein `throw $tag2`
    /// derselben Instanz nicht fangen.
    pub tags: Vec<u32>,
    /// Startindex der Funktionen (nur die Eigenfunktionen liegen dort).
    pub funktions_start: u32,
    /// Indexraum der Funktionen: lokaler Index -> Index im Store.
    ///
    /// WICHTIG (GEMESSEN 01.10., `table_init.wast` 81 Faelle): der lokale
    /// Index zaehlt ZUERST die Importe und dann die Eigenfunktionen, der
    /// Store kennt aber nur die Eigenfunktionen (eine importierte Funktion
    /// zeigt auf ein fremdes Objekt). `funktions_start + i` ist deshalb nur
    /// fuer importlose Module richtig — diese Tabelle ist es immer.
    pub funktionen: Vec<u32>,
    /// Startindex der Datenabschnitte im Store.
    ///
    /// WICHTIG (GEMESSEN 02.10., `bulk.wast`): `memory.init 0` nennt den
    /// Index im Modul. Im Store liegen die Abschnitte aller Module einer
    /// Datei hintereinander — ohne diese Basis schrieb `memory.init` in den
    /// Abschnitt eines FRUEHEREN Moduls (dort meist schon verworfen, also
    /// „nichts geschrieben").
    pub daten_start: u32,
    /// Startindex der Elementabschnitte im Store (siehe `daten_start`).
    pub elemente_start: u32,
    /// Exporte: Name -> (Art, Index IM INSTANZRAUM).
    pub exporte: BTreeMap<String, (Art, u32)>,
    /// Der Modulcode (Debugger).
    pub modul: Arc<Modul>,
}

impl Instanz {
    pub fn export(&self, name: &str) -> Option<(Art, u32)> {
        self.exporte.get(name).copied()
    }
    /// Alle Exportnamen (fuer `Object.keys(instance.exports)`).
    pub fn export_namen(&self) -> Vec<String> {
        self.exporte.keys().cloned().collect()
    }
    pub fn tabellen_index(&self, lokal: u32) -> Option<u32> {
        self.tabellen.get(lokal as usize).copied()
    }
    /// Speicherindex des ersten Speichers (WebAssembly 2.0).
    pub fn speicher_index(&self) -> Option<u32> {
        self.speicher.first().copied()
    }
    /// Speicherindex zu einem lokalen Index (Multi-Memory).
    pub fn speicher_index_von(&self, lokal: u32) -> Option<u32> {
        self.speicher.get(lokal as usize).copied()
    }
    /// Storeindex eines lokalen Funktionsindex (siehe `funktionen`).
    pub fn funktions_index(&self, lokal: u32) -> Option<u32> {
        self.funktionen.get(lokal as usize).copied()
    }
}

/// Instanziieren: Importe aufloesen, Speicher/Tabellen/Globale anlegen,
/// Eigenfunktionen einhaengen, Element-/Datenabschnitte schreiben,
/// Startfunktion rufen.
pub fn instanziieren(
    lauf: &mut Lauf,
    m: &Modul,
    quelle: &dyn ImportQuelle,
) -> Result<Instanz, Fehler> {
    instanziieren_host(lauf, &mut NoHost, m, quelle)
}

/// Like [`instanziieren`], with the embedder that host imports reach (the
/// start function may call them).
pub fn instanziieren_host(
    lauf: &mut Lauf,
    host: &mut dyn Host,
    m: &Modul,
    quelle: &dyn ImportQuelle,
) -> Result<Instanz, Fehler> {
    // Speicher/Tabellen/Globale/Funktionen werden in EINEM Durchgang
    // angelegt, aber in der REIHENFOLGE des Moduls (Import-Indexraeume).
    // Speicherindizes des Moduls: lokaler Index -> Index im Store.
    // WebAssembly 2.0 hat genau einen Speicher (Index 0), Multi-Memory
    // (Wasm 3.0) mehrere — die Liste traegt beide Faelle.
    let mut speicher_idx: Vec<u32> = Vec::new();
    let mut tabellen: Vec<u32> = Vec::new();

    // 1) Tabellen
    for t in &m.tabellen {
        let idx = match t {
            ImportiertTab::Importiert { modul, feld, typ } => {
                match quelle.hole(lauf, modul, feld) {
                    Some(ImportWert::Tabelle(i)) => {
                        // Referenztyp UND Grenzen muessen passen (Spec:
                        // „incompatible import type"). GEMESSEN 02.10.
                        // (`imports.wast` Z. 124-129: 6 Faelle).
                        let (gleicher_typ, passt_groesse) = lauf
                            .store
                            .tabellen
                            .get(i as usize)
                            .map(|x| {
                                let max_passt = match typ.grenzen.max {
                                    None => true,
                                    Some(gewuenscht) => {
                                        x.max().is_some_and(|ist| ist <= gewuenscht)
                                    }
                                };
                                (
                                    x.reftyp() == typ.reftyp,
                                    x.len() >= typ.grenzen.min && max_passt,
                                )
                            })
                            .unwrap_or((false, false));
                        if !gleicher_typ || !passt_groesse {
                            let ist = lauf
                                .store
                                .tabellen
                                .get(i as usize)
                                .map(|t| format!("{}..{:?}", t.len(), t.max()))
                                .unwrap_or_else(|| "fehlt".to_string());
                            return Err(Fehler::link(
                                L_TABELLE,
                                format!(
                                    "Import {modul}.{feld}: Tabellentyp/Grenzen passen nicht \
                                     (gefordert {} {}..{:?}, vorhanden {ist})",
                                    typ.reftyp.text(),
                                    typ.grenzen.min,
                                    typ.grenzen.max
                                ),
                            ));
                        }
                        i
                    }
                    Some(anderes) => {
                        return Err(Fehler::link(
                            L_SIGNATUR,
                            format!(
                                "Import {modul}.{feld}: {} erwartet, {anderes:?} gefunden",
                                "Tabelle"
                            ),
                        ));
                    }
                    None => {
                        return Err(Fehler::link(
                            L_IMPORT,
                            format!("fehlender Import {modul}.{feld} (Tabelle)"),
                        ));
                    }
                }
            }
            ImportiertTab::Eigen { typ, .. } => {
                lauf.neue_tabelle(typ.grenzen.min, typ.grenzen.max, typ.reftyp)?
            }
        };
        tabellen.push(idx);
    }

    // 2) Speicher
    for s in &m.speicher {
        match s {
            ImportiertSpeicher::Importiert { modul, feld, typ } => {
                match quelle.hole(lauf, modul, feld) {
                    Some(ImportWert::Speicher(i)) => {
                        // Mindestgroesse pruefen (Link-Regel).
                        // Grenzen muessen passen (Spec: „incompatible
                        // import type"): angefordertes Minimum <= reales
                        // Minimum; ein angefordertes Maximum verlangt ein
                        // reales Maximum, das nicht groesser ist.
                        // GEMESSEN 02.10. (`imports2.wast` Z. 17/19,
                        // `imports3.wast` Z. 9).
                        let ok = lauf.store.speicher.get(i as usize).map(|x| {
                            let ist_groesse = x.seiten();
                            let hat_max = x.max_seiten().is_some();
                            let max_passt = match typ.grenzen.max {
                                None => true,
                                Some(gewuenscht) => {
                                    hat_max && x.max_seiten().is_some_and(|ist| ist <= gewuenscht)
                                }
                            };
                            ist_groesse >= typ.grenzen.min && max_passt
                        });
                        if ok != Some(true) {
                            let ist = lauf
                                .store
                                .speicher
                                .get(i as usize)
                                .map(|x| format!("{}..{:?}", x.seiten(), x.max_seiten()))
                                .unwrap_or_else(|| "fehlt".to_string());
                            return Err(Fehler::link(
                                L_SPEICHER,
                                format!(
                                    "Import {modul}.{feld}: Grenzen passen nicht                                      (gefordert {}..{:?}, vorhanden {ist})",
                                    typ.grenzen.min, typ.grenzen.max
                                ),
                            ));
                        }
                        speicher_idx.push(i);
                    }
                    Some(_) => {
                        return Err(Fehler::link(
                            L_SIGNATUR,
                            format!("Import {modul}.{feld}: Speicher erwartet"),
                        ));
                    }
                    None => {
                        return Err(Fehler::link(
                            L_IMPORT,
                            format!("fehlender Import {modul}.{feld} (Speicher)"),
                        ));
                    }
                }
            }
            ImportiertSpeicher::Eigen { typ } => {
                speicher_idx.push(lauf.neuer_speicher(typ.grenzen.min, typ.grenzen.max)?);
            }
        }
    }

    // 3) Funktionen: Importe zuerst (Reihenfolge = Modulreihenfolge der
    //    Func-Liste, in der die importierten zuerst stehen).
    let funktions_start = lauf.store.funktionen.len() as u32;
    let mut eigene_zaehler = 0u32;
    // Indexraum des Moduls -> Store (Importe zuerst, siehe `Instanz`).
    let mut funktions_raum: Vec<u32> = Vec::new();
    for f in &m.funktionen {
        match f {
            Func::Importiert { modul, feld, typ } => {
                let ft = m
                    .typen
                    .get(*typ as usize)
                    .ok_or_else(|| typ_ungueltig(*typ))?
                    .clone();
                match quelle.hole(lauf, modul, feld) {
                    Some(ImportWert::Funktion(i)) => {
                        // **Iso-rekursiver Vergleich**, wenn der
                        // Importtyp in einer `(rec …)`-Gruppe liegt: dann
                        // zaehlt die ganze Gruppe in ihrer Reihenfolge, nicht
                        // nur die eine Signatur.
                        //
                        // GEMESSEN 03.10. (`type-rec.wast` Z. 148/156):
                        // Export `(rec (type (func)) (type (struct)))`,
                        // Import `(rec (type (struct)) (type (func)))` oder
                        // `(rec (type (func)))` — beide als Signatur
                        // ununterscheidbar, beide `assert_unlinkable`.
                        //
                        // Ausserhalb einer Gruppe gilt weiter die
                        // Signatur-Gleichheit (Wasm 2.0, `imports.wast`).
                        // **Iso-rekursive Aequivalenz in Normalform**: die
                        // Mitgliedsliste wird gruppenbezogen verglichen —
                        // ein Verweis **innerhalb** der Gruppe zaehlt als
                        // Position, einer **nach aussen** als Rohindex. Nur
                        // so sind `m25_Z652` und `m34_Z746` zu unterscheiden:
                        // als blosse Signaturen sind ihre Gruppen gleich.
                        //
                        // GEMESSEN 03.10. (`type-subtyping.wast` Z. 652/746,
                        // 2 `assert_unlinkable`).
                        let passt = match crate::modell::gruppen_normalform(m, *typ) {
                            Some((glieder, pos, _)) => lauf
                                .store
                                .funktion_gruppe
                                .get(i as usize)
                                .cloned()
                                .flatten()
                                .is_some_and(|(g, p)| g == glieder && p == pos),
                            None => lauf
                                .store
                                .funktionen
                                .get(i as usize)
                                .map(|x| signatur(x) == ft)
                                .unwrap_or(false),
                        };
                        if !passt {
                            return Err(Fehler::link(
                                L_SIGNATUR,
                                format!("Import {modul}.{feld}: Signatur passt nicht"),
                            ));
                        }
                        funktions_raum.push(i);
                    }
                    Some(_) => {
                        return Err(Fehler::link(
                            L_SIGNATUR,
                            format!("Import {modul}.{feld}: Funktion erwartet"),
                        ));
                    }
                    None => {
                        return Err(Fehler::link(
                            L_IMPORT,
                            format!("fehlender Import {modul}.{feld} (Funktion)"),
                        ));
                    }
                }
            }
            Func::Eigen {
                typ,
                lokale,
                koerper,
                ..
            } => {
                let ft = m
                    .typen
                    .get(*typ as usize)
                    .ok_or_else(|| typ_ungueltig(*typ))?
                    .clone();
                let sprung = Arc::new(crate::interp::Sprungtabelle::baue(koerper));
                lauf.store.funktionen.push(Arc::new(Funktion::Eigen {
                    typ: ft,
                    lokale: lokale.clone(),
                    koerper: Arc::new(koerper.clone()),
                    sprung,
                    raum: funktions_start + eigene_zaehler,
                }));
                // Die `(rec …)`-Gruppe mitfuehren: der Importvergleich
                // braucht sie iso-rekursiv (GEMESSEN 03.10., `type-rec.wast`
                // Z. 148/156 — siehe `Store::funktion_gruppe`).
                lauf.store.funktion_gruppe.push(
                    crate::modell::gruppen_normalform(m, *typ)
                        .map(|(glieder, pos, _)| (glieder, pos)),
                );
                funktions_raum.push(funktions_start + eigene_zaehler);
                eigene_zaehler += 1;
            }
        }
    }

    // 4) Globale
    // Startindex der globalen Variablen — nach dem Anlegen (Schritt 4).
    let globale_start = lauf.store.globale.len() as u32;
    // Indexraum der globalen Variablen: Importe zeigen auf ihren
    // Store-Eintrag, eigene Variablen auf ihren neuen Platz.
    let mut globale_raum: Vec<u32> = Vec::new();
    for g in &m.globale {
        if let Some((modul, feld)) = &g.import {
            match quelle.hole(lauf, modul, feld) {
                Some(ImportWert::Global(i)) => {
                    let wert = lauf.store.globale.get(i as usize).copied().ok_or_else(|| {
                        Fehler::link(L_GLOBAL, format!("Import {modul}.{feld}: Global {i} fehlt"))
                    })?;
                    // Subtyppruefung: ein Wert vom Typ `(ref null func)`
                    // erfuellt `(ref null $t)`, aber nicht umgekehrt
                    // (Wasm 3.0). GEMESSEN 02.10. (`table.wast` Z. 34:
                    // „Import M.g: Typ funcref != typreferenz").
                    // Der DEKLARIERTE Typ der exportierten Variablen steht in
                    // `globale_typen` (ein Referenzwert selbst traegt keinen
                    // Typindex).
                    let ist = lauf
                        .store
                        .globale_typen
                        .get(i as usize)
                        .copied()
                        .unwrap_or_else(|| wert.typ());
                    // Der Import darf einen ALLGEMEINEREN Typ nennen als
                    // der Export anbietet (`export <: import`); bei einer
                    // veraenderbaren Variablen muss der Typ GLEICH sein.
                    //
                    // GEMESSEN 03.10. (`linking.wast` Z. 112-200: 9 gueltige
                    // und 17 `assert_unlinkable`-Faelle).
                    let passt = match (&ist, &g.typ) {
                        (Valtyp::Ref(ra), Valtyp::Ref(rb)) => {
                            crate::modell::reftyp_import_passt_als(ra, rb, g.veraenderbar)
                        }
                        _ => ist == g.typ,
                    };
                    if !passt {
                        return Err(Fehler::link(
                            L_GLOBAL,
                            format!("Import {modul}.{feld}: Typ {ist:?} != {:?}", g.typ),
                        ));
                    }
                    // Veraenderbarkeit: ein veraenderbarer Import braucht
                    // einen veraenderbaren Export (Spec: „incompatible
                    // import type"). GEMESSEN 02.10. (`imports.wast` Z. 71/
                    // 75/79: 3 `assert_unlinkable`).
                    let veraenderbar_export = lauf
                        .store
                        .globale_veraenderbar
                        .get(i as usize)
                        .copied()
                        .unwrap_or(g.veraenderbar);
                    if g.veraenderbar != veraenderbar_export {
                        return Err(Fehler::link(
                            L_GLOBAL,
                            format!(
                                "Import {modul}.{feld}: veraenderbar={} passt nicht zum Export (veraenderbar={veraenderbar_export})",
                                g.veraenderbar
                            ),
                        ));
                    }
                    lauf.store.globale.push(wert);
                    lauf.store.globale_typen.push(g.typ);
                    lauf.store.globale_veraenderbar.push(g.veraenderbar);
                    // DERSELBE Store-Eintrag (dieselbe Zelle) wird erneut
                    // benutzt — das ist die Identitaet der Variablen
                    // (GEMESSEN 02.10., `instance.wast`: 3 Faelle).
                    lauf.store.globale_zelle.push(
                        lauf.store
                            .globale_zelle
                            .get(i as usize)
                            .copied()
                            .unwrap_or(i),
                    );
                    globale_raum.push((lauf.store.globale.len() - 1) as u32);
                }
                Some(_) => {
                    return Err(Fehler::link(
                        L_SIGNATUR,
                        format!("Import {modul}.{feld}: globale Variable erwartet"),
                    ));
                }
                None => {
                    return Err(Fehler::link(
                        L_IMPORT,
                        format!("fehlender Import {modul}.{feld} (globale Variable)"),
                    ));
                }
            }
        } else {
            let code = g.start.clone().unwrap_or_default();
            // Der Startwert sieht nur die BISHER angelegten globalen
            // Variablen (Importe und die eigenen davor) - ueber deren
            // Indexraum, nicht ueber `globale_start + i`.
            //
            // GEMESSEN 03.10. (`global.wast` Z. 633ff: 6 Faelle).
            let mut wert = lauf.konstanter_wert_index(&code, &globale_raum, &m.strukt)?;
            // `ref.func n` im Startausdruck nennt einen Index im Raum des
            // Moduls; die globale Variable traegt einen Speicherindex.
            // GEMESSEN 02.10. (`elem.wast` `call_imported_elem`: der
            // importierten Funktionsreferenz zeigte auf eine andere
            // Funktion).
            if let Wert::Func(Some(f)) = wert {
                wert = Wert::Func(Some(*funktions_raum.get(f as usize).unwrap_or(&f)));
            }
            lauf.store.globale.push(wert);
            lauf.store.globale_typen.push(g.typ);
            lauf.store.globale_veraenderbar.push(g.veraenderbar);
            lauf.store
                .globale_zelle
                .push((lauf.store.globale.len() - 1) as u32);
            globale_raum.push((lauf.store.globale.len() - 1) as u32);
        }
    }

    // 4b) Tags (Exception Handling): Signatur in den Store. Ein Import wird
    //     gegen die Signatur des Exports geprueft (Spec: „incompatible
    //     import type").
    //
    //     GEMESSEN 02.10. (`imports.wast` Z. 0/10: 2 Faelle direkt, 9 als
    //     Folgefehler).
    let mut tags_raum: Vec<u32> = Vec::new();
    for t in &m.tags {
        let ft = m
            .typen
            .get(t.typ as usize)
            .cloned()
            .ok_or_else(|| typ_ungueltig(t.typ))?;
        if let Some((modul, feld)) = &t.import {
            match quelle.hole(lauf, modul, feld) {
                Some(ImportWert::Tag(i)) => {
                    let ist = lauf.store.tags.get(i as usize).ok_or_else(|| {
                        Fehler::link(L_IMPORT, format!("Import {modul}.{feld}: Tag {i} fehlt"))
                    })?;
                    // Tags haben nur Parameter; der Vergleich ist der
                    // ueber die Parameterliste (die Typindizes zweier
                    // Module muessen nicht gleich sein).
                    if ist.parameter != ft.parameter {
                        return Err(Fehler::link(
                            L_SIGNATUR,
                            format!(
                                "Import {modul}.{feld}: Tag-Signatur {} != {}",
                                ist.parameter.len(),
                                ft.parameter.len()
                            ),
                        ));
                    }
                    // DERSELBE Store-Eintrag wird erneut benutzt — das ist
                    // die Identitaet des Tags (Spec: „the same tag").
                    tags_raum.push(i);
                }
                Some(anderes) => {
                    return Err(Fehler::link(
                        L_IMPORT,
                        format!(
                            "Import {modul}.{feld}: Tag erwartet, {} gefunden",
                            anderes.text()
                        ),
                    ));
                }
                None => {
                    return Err(Fehler::link(
                        L_IMPORT,
                        format!("fehlender Import {modul}.{feld} (Tag)"),
                    ));
                }
            }
        } else {
            lauf.store.tags.push(ft.clone());
            tags_raum.push((lauf.store.tags.len() - 1) as u32);
        }
    }

    // 5) Exporte
    let mut exporte = BTreeMap::new();
    for e in &m.exporte {
        if exporte
            .insert(e.name.clone(), (e.art, e.art.index()))
            .is_some()
        {
            return Err(Fehler::link(
                L_EXPORT,
                format!("Export {:?} doppelt", e.name),
            ));
        }
    }

    // Basen der Abschnittsraeume: die Abschnitte DER MODULE liegen ab hier
    // im Store (siehe `Instanz::daten_start`).
    let daten_start = lauf.store.daten.len() as u32;
    let elemente_start = lauf.store.elemente.len() as u32;

    // Den Speicher MERKEN: `Lauf::speicher_idx` sagt, welcher Speicher
    // gemeint ist, wenn niemand im Laufen ist — die Speicheransicht der
    // DevTools liest darueber. VORHER schrieb diese Stelle keine, und
    // `speicher_idx` blieb fuer immer `None`: jede Hex-Ansicht meldete
    // „kein Speicher". GEMESSEN 03.10. (tests/diagnose.rs).
    let zuerst = speicher_idx.first().copied();
    let instanz = Instanz {
        nr: lauf.instanz_zaehler,
        speicher: speicher_idx,
        tabellen: tabellen.clone(),
        globale_start,
        globale_index: globale_raum,
        tags: tags_raum,
        funktions_start,
        funktionen: funktions_raum,
        daten_start,
        elemente_start,
        exporte,
        modul: Arc::new(m.clone()),
    };

    lauf.instanz_zaehler += 1;
    if zuerst.is_some() {
        lauf.speicher_idx = zuerst;
    }

    // 5a) Kontext der Eigenfunktionen im Store hinterlegen. Ein importierter
    //     Aufruf laeuft im Kontext des DEFINIERENDEN Moduls (Speicher,
    //     Tabellen, globale Variablen, Typen, Tags).
    //
    //     GEMESSEN 03.10. (`linking1.wast`: 2 Faelle mit 0xf2 statt 0xa7,
    //     `linking.wast`/`memory_copy1.wast`: weitere).
    {
        let kontext = Arc::new(ModulKontext::von_instanz(&instanz));
        for (lokal, &store_index) in instanz.funktionen.iter().enumerate() {
            // Nur die Eigenfunktionen bekommen den Kontext; importierte
            // zeigen auf den Store-Eintrag des fremden Moduls (dort steht
            // schon dessen Kontext).
            if lokal >= m.importierte_funktionen() {
                while lauf.store.funktion_kontext.len() <= store_index as usize {
                    lauf.store.funktion_kontext.push(None);
                }
                lauf.store.funktion_kontext[store_index as usize] = Some(kontext.clone());
            }
        }
    }

    // 5b) Startwerte der eigenen Tabellen (Wasm 3.0, „table initializer“):
    //     der Startwert fuellt den GANZEN Bereich der Tabelle. Das kommt
    //     VOR den aktiven Elementabschnitten (Reihenfolge der
    //     Spezifikation) und NACH den globalen Variablen, weil
    //     `global.get` im Startwert eine globale Variable nennen darf.
    //
    //     GEMESSEN 02.10. (`table.wast` Z. 34-39: 4 Faelle — `get2` bis
    //     `get5` lesen Index 4/7/8/9 einer Tabelle mit 10 Plaetzen).
    for (i, t) in m.tabellen.iter().enumerate() {
        let startwert = match t {
            ImportiertTab::Eigen {
                startwert: Some(code),
                ..
            } => code,
            _ => continue,
        };
        let wert = lauf
            .konstanter_auswerten(startwert, Some(&instanz))?
            .ok_or_else(|| {
                Fehler::link(
                    L_TABELLE,
                    format!("Tabelle {i}: Startwert liefert keinen Wert"),
                )
            })?;
        let r = wert.als_ref();
        let store_idx = instanz
            .tabellen
            .get(i)
            .copied()
            .ok_or_else(|| Fehler::link(L_TABELLE, format!("Tabelle {i} fehlt im Store")))?;
        let tabelle = lauf
            .store
            .tabellen
            .get_mut(store_idx as usize)
            .ok_or_else(|| Fehler::link(L_TABELLE, format!("Tabelle {i} fehlt im Store")))?;
        let laenge = tabelle.len();
        tabelle.fuelle(0, r, laenge)?;
    }

    // 6) Elementabschnitte in den Store (fuer table.init/elem.drop) und
    //    aktive Abschnitte anwenden.
    for e in &m.elemente {
        // Deklarative Abschnitte dienen nur `ref.func`; fuer `table.init`
        // sind sie LEER (GEMESSEN 02.10., `elem.wast` Z. 716:
        // `table.init` auf einem deklarativen Abschnitt muss trapen).
        if matches!(e, Element::Deklarativ { .. }) {
            lauf.store.elemente.push(Some(Vec::new()));
            continue;
        }
        let refs = element_zu_refs(lauf, e, &instanz.funktionen, &instanz)?;
        lauf.store.elemente.push(Some(refs));
    }
    // Aktive ELEMENTabschnitte anwenden — VOR den Datenabschnitten.
    //
    // GEMESSEN 03.10. (`linking0.wast` Z. 27ff, 1 Fall): der
    // Elementabschnitt eines Moduls muss stehen, auch wenn ein spaeterer
    // Datenabschnitt ausserhalb des Speichers liegt und das Instanziieren
    // abbrechb. (`linking3.wast` Z. 25ff bestaetigt die andere Richtung:
    // ein frueher geschriebener Datenabschnitt bleibt, wenn ein spaeterer
    // trappt.) Die Elemente kommen also ZUERST, die Daten DANACH.
    for (i, e) in m.elemente.iter().enumerate() {
        if let Element::Aktiv {
            tabelle, offset, ..
        } = e
        {
            let off = lauf
                .konstanter_wert_in(offset, &instanz)?
                .als_i32()
                .unwrap_or(0) as u32 as u64;
            let refs = element_zu_refs(lauf, e, &instanz.funktionen, &instanz)?;
            let tidx = instanz.tabellen_index(*tabelle).ok_or_else(|| {
                Fehler::link(L_TABELLE, format!("Element {i}: Tabelle {tabelle} fehlt"))
            })?;
            let laenge = refs.len() as u64;
            lauf.store
                .tabellen
                .get_mut(tidx as usize)
                .ok_or_else(|| Fehler::link(L_TABELLE, format!("Element {i}: Tabelle fehlt")))?
                .init(&refs, off, 0, laenge)?;
            // Aktiv -> nach dem Instanziieren verworfen. Der INDEX ist
            // `elemente_start + i` (Modulindex -> Store); sonst landet das
            // Leeren in einem Abschnitt eines frueheren Moduls
            // (GEMESSEN 02.10., `bulk.wast`: "table.init: Element 2
            // verworfen").
            lauf.store.elemente[elemente_start as usize + i] = Some(Vec::new());
        }
    }

    for (i, d) in m.daten.iter().enumerate() {
        match d {
            Daten::Aktiv {
                speicher: sidx,
                offset,
                inhalt,
            } => {
                let off = lauf
                    .konstanter_wert_in(offset, &instanz)?
                    .als_i32()
                    .unwrap_or(0) as u32 as u64;
                // `sidx` ist der LOKALE Speicherindex des Moduls (Wasm 2.0
                // hat genau einen Speicher, also 0). Der Store kennt ihn
                // unter `instanz.speicher` — beide Zahlen sind NICHT
                // dasselbe, seit ein Lauf mehrere Speicher halten kann
                // (GEMESSEN 01.10.: mit `spectest` im selben Lauf war
                // `address.wast` 0/256, weil 0 != 1).
                let s = match instanz.speicher_index_von(*sidx) {
                    Some(s) => s,
                    None => {
                        return Err(Fehler::link(
                            L_SPEICHER,
                            format!(
                                "Datenabschnitt: Speicherindex {sidx} unbekannt (Modul hat {})",
                                instanz.speicher.len()
                            ),
                        ));
                    }
                };
                lauf.store
                    .speicher
                    .get_mut(s as usize)
                    .ok_or_else(|| Fehler::link(L_SPEICHER, "Datenabschnitt: Speicher fehlt"))?
                    .schreib(off, inhalt)?;
                // Aktive Abschnitte sind nach dem Instanziieren verworfen
                // (`data.drop`): `memory.init` muss danach mit einem Trap
                // antworten, nicht mit einer leeren Sektion.
                lauf.store.daten.push(None);
            }
            Daten::Passiv { inhalt } => {
                lauf.store.daten.push(Some(inhalt.clone()));
            }
        }
        let _ = i;
    }
    // 7) Startfunktion
    if let Some(s) = m.start {
        let store_idx = instanz
            .funktions_index(s)
            .ok_or_else(|| Fehler::link(L_START, format!("Startfunktion {s} fehlt")))?;
        let f = lauf
            .store
            .funktionen
            .get(store_idx as usize)
            .cloned()
            .ok_or_else(|| Fehler::link(L_START, format!("Startfunktion {s} fehlt")))?;
        lauf.setze_brennstoff(lauf.grenzen.brennstoff);
        crate::interp::aufrufen_host(lauf, host, &instanz, &f, &[], store_idx)?;
    }
    Ok(instanz)
}

/// Signatur einer Funktion (fuer den Link-Vergleich).
pub fn signatur(f: &Funktion) -> FuncTyp {
    match f {
        Funktion::Eigen { typ, .. } => typ.clone(),
        Funktion::Fremd { typ, .. } => typ.clone(),
    }
}

/// Anzahl der Eintraege eines Elements.
pub fn element_anzahl(e: &Element) -> usize {
    eintraege(e).len()
}

/// Elementabschnitt -> Referenzliste.
/// Elementabschnitt in Referenzliste umrechnen.
///
/// `ref.func 5` meint den Index 5 im INDEXRAUM DES MODULS (Importe zuerst).
/// Im Store liegt die Funktion an `raum[5]` — nicht bei `5`
/// (GEMESSEN 01.10., `table_init.wast`: ohne diese Tabelle waren 81 Faelle
/// „Funktion 23 fehlt im Store").
fn element_zu_refs(
    lauf: &mut Lauf,
    e: &Element,
    raum: &[u32],
    instanz: &Instanz,
) -> Result<Vec<Option<Ref>>, Fehler> {
    let eintraege = eintraege(e);
    let mut v = Vec::with_capacity(eintraege.len());
    for aus in eintraege {
        // `ref.func` rechnet der Aufrufer ueber `raum` um; `global.get` wird
        // hier schon auf den Store umgerechnet (GEMESSEN 02.10.,
        // `elem.wast`: `(elem (offset …) funcref (global.get 0))`).
        let w = lauf.konstanter_wert_in(aus, instanz)?;
        // Nur `ref.func n` nennt einen Index im Raum des MODULS und muss
        // ueber `raum` umgerechnet werden. Ein `global.get` liefert schon
        // einen Speicherindex (der importierten globalen Variable) — eine
        // zweite Umrechnung zeigte auf die falsche Funktion
        // (GEMESSEN 02.10., `elem.wast` `call_imported_elem`: der Aufruf
        // sprang in eine Fremdfunktion und lief in die Aufruftiefe).
        let aus_reffunc = matches!(aus.first(), Some(crate::op::Befehl::RefFunc(_)));
        v.push(match w.als_ref() {
            Some(Ref::Func(f)) if aus_reffunc => {
                Some(Ref::Func(raum.get(f as usize).copied().unwrap_or(f)))
            }
            andere => andere,
        });
    }
    Ok(v)
}

fn eintraege(e: &Element) -> &[Vec<crate::op::Befehl>] {
    match e {
        Element::Aktiv { eintraege, .. }
        | Element::Passiv { eintraege, .. }
        | Element::Deklarativ { eintraege, .. } => eintraege,
    }
}

/// Baut einen Speicher und liefert den Index (fuer Tests/Beispiele).
pub fn speicher_allein(lauf: &mut Lauf, min: u64, max: Option<u64>) -> Result<u32, Fehler> {
    lauf.neuer_speicher(min, max)
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Ergebnisse sichern.

    /// M7: `gruppen_normalform` bestimmt die **gruppenbezogene** Form eines
    /// `(rec …)`-Typs: alle Mitglieder als `Mitglied` und die Position des
    /// eigenen Typs. Ausserhalb jeder Gruppe gibt es `None`.
    ///
    /// GEMESSEN 03.10. (`type-rec.wast` Z. 148/156, `type-subtyping.wast`
    /// Z. 652/746).
    #[test]
    fn normalform_der_gruppe() {
        use crate::modell::{Form, Ziel};
        let leer = FuncTyp {
            parameter: Vec::new(),
            ergebnis: Vec::new(),
        };
        let mut m = Modul::leer();
        // (rec (type (func)) (type (func))) (rec (type (func)))
        m.typen = vec![leer.clone(), leer.clone(), leer.clone()];
        m.gruppe_von = vec![Some((0, 2)), Some((0, 2)), Some((2, 3))];
        let (glieder, pos, _) = crate::modell::gruppen_normalform(&m, 0).expect("Gruppe (0,2)");
        assert_eq!(glieder.len(), 2);
        assert_eq!(pos, 0);
        let (glieder2, pos2, _) =
            crate::modell::gruppen_normalform(&m, 1).expect("zweites Mitglied");
        assert_eq!(glieder2, glieder, "alle Mitglieder gehoeren zur Gruppe");
        assert_eq!(pos2, 1, "die Position unterscheidet die Mitglieder");
        let (glieder3, _, _) = crate::modell::gruppen_normalform(&m, 2).expect("zweite Gruppe");
        assert_eq!(
            glieder3.len(),
            1,
            "eine unvollstaendige Gruppe ist eine andere"
        );
        // Ausserhalb jeder Gruppe: `None`.
        let mut ohne = Modul::leer();
        ohne.typen = vec![leer.clone()];
        assert!(crate::modell::gruppen_normalform(&ohne, 0).is_none());
        assert!(
            crate::modell::gruppen_normalform(&m, 99).is_none(),
            "Index ausserhalb der Tabelle"
        );
        // **Intern**: ein Verweis auf ein Mitglied der **eigenen** Gruppe.
        let mut m2 = Modul::leer();
        m2.typen = vec![
            leer.clone(),
            FuncTyp {
                parameter: vec![Valtyp::Ref(crate::modell::Reftyp::Konkret {
                    typ: 0,
                    nullable: false,
                })],
                ergebnis: Vec::new(),
            },
            leer.clone(),
        ];
        m2.gruppe_von = vec![Some((0, 2)), Some((0, 2)), Some((2, 3))];
        m2.sub_ober = vec![None, None, None];
        let (g, _, _) = crate::modell::gruppen_normalform(&m2, 0).expect("Gruppe (0,2)");
        assert_eq!(
            g[1].parameter,
            vec![Form::Ref(false, Some(Ziel::Intern(0)))]
        );
        // **Extern**: derselbe Verweis, aber auf einen Typ der *anderen*
        // Gruppe — genau daran unterscheidet der Link gueltige von
        // ungueltigen Importen (`m25_Z652`, `m34_Z746`).
        let (h, _, _) = crate::modell::gruppen_normalform(&m2, 2).expect("zweite Gruppe");
        assert!(
            h[0].parameter.is_empty(),
            "die zweite Gruppe traegt keine Signatur"
        );
    }
    #[test]
    fn anlegen_und_zugriff() {
        let mut l = Lauf::neu(Grenzen::bombe());
        let s = l.neuer_speicher(1, None).unwrap();
        assert_eq!(s, 0);
        assert_eq!(l.store.speicher.len(), 1);
        l.store.speicher[0].schreib_u32(0, 7).unwrap();
        assert_eq!(l.store.speicher[0].lies_u32(0).unwrap(), 7);
        // Neue globale
        let g = l.neue_globale(Wert::I32(5), false).unwrap();
        assert_eq!(l.store.globale[g as usize], Wert::I32(5));
        // Speichergrenze
        assert_eq!(
            l.neuer_speicher(99, None).unwrap_err().art(),
            crate::fehler::ART_SPEICHER
        );
    }

    #[test]
    fn brennstoff_endet_mit_benanntem_fehler() {
        let mut l = Lauf::neu(Grenzen::bombe());
        l.setze_brennstoff(3);
        l.verbrauche(2).unwrap();
        let e = l.verbrauche(5).unwrap_err();
        assert_eq!(e.art(), T_BRENNSTOFF);
        assert_eq!(l.verbraucht, 3);
    }

    #[test]
    fn fremdfunktion_signatur() {
        let f = Funktion::Fremd {
            typ: FuncTyp {
                parameter: vec![Valtyp::I32],
                ergebnis: vec![Valtyp::I32],
            },
            funktion: Arc::new(|_l, _h, a: &[Wert]| {
                Ok(vec![Wert::I32(a[0].als_i32().unwrap_or(0) + 1)])
            }),
        };
        assert_eq!(signatur(&f).parameter.len(), 1);
        let mut l = Lauf::neu(Grenzen::standard());
        let idx = l.neue_funktion(
            FuncTyp {
                parameter: vec![Valtyp::I32],
                ergebnis: vec![Valtyp::I32],
            },
            Arc::new(|_l, _h, a: &[Wert]| Ok(vec![Wert::I32(a[0].als_i32().unwrap_or(0) + 1)])),
        );
        assert_eq!(idx, 0);
        assert_eq!(signatur(&l.store.funktionen[0]).parameter.len(), 1);
    }
}
