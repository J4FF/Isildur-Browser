//! W1 Punkt 1: Binarformat-Parser. Alle Sektionen des WebAssembly-2.0-
//! Kernformats, LEB128 ueber `bytes::Leser` (mit Byte-Obergrenzen),
//! alle Vektoren und Laengen VOR der Allokation geprueft (M4).

use crate::bytes::Leser;
use crate::fehler::{
    ART_ABHAENGIG, ART_BLOCKTYP, ART_DATEN, ART_DOPPELT, ART_GRENZE, ART_INDEX, ART_LAENGE,
    ART_MAGIE, ART_OPCODE, ART_REFTYP, ART_REIHENFOLGE, ART_REST, ART_SEKTION, ART_SPEICHER,
    ART_TABELLE, ART_UNVOLLSTAENDIG, ART_VERSION, ART_ZAEHLER, Fehler,
};
use crate::grenzen::Grenzen;
use crate::modell::{
    Art, Custom, Daten, Element, Export, Func, FuncTyp, Global, Importart, ImportiertSpeicher,
    ImportiertTab, Limits, Modul, Reftyp, Speichertyp, Tabellentyp, Valtyp, valtyp_lesen,
};
use crate::op::{Befehl, lies_befehl};

/// Name einer Sektion (fuer Fehlermeldungen und die Modul-Uebersicht im
/// Debugger). Custom = 0, darf ueberall stehen.
pub fn sektionsname(art: u8) -> &'static str {
    match art {
        0 => "custom",
        1 => "typ",
        2 => "import",
        3 => "funktion",
        4 => "tabelle",
        5 => "speicher",
        6 => "global",
        7 => "export",
        8 => "start",
        9 => "element",
        10 => "code",
        11 => "daten",
        12 => "datenanzahl",
        _ => "unbekannt",
    }
}

/// Reihenfolge-Rang einer Sektion (fuer die Reihenfolgenpruefung).
/// `DataCount` (12) steht laut Spezifikation VOR `Code` (10) und `Daten` (11).
fn rang(art: u8) -> u8 {
    match art {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        6 => 6,
        7 => 7,
        8 => 8,
        9 => 9,
        12 => 10,
        10 => 11,
        11 => 12,
        _ => 0,
    }
}

/// Liest ein Modul aus Binaerbytes.
pub fn lies_modul(daten: &[u8], g: &Grenzen) -> Result<Modul, Fehler> {
    // M4: Eingabegroesse gegen die Grenze, BEVOR irgendetwas alloziert wird.
    if daten.len() > g.modul_bytes {
        return Err(Fehler::form(
            ART_GRENZE,
            format!(
                "Modul {} Byte ueber der Grenze {}",
                daten.len(),
                g.modul_bytes
            ),
        ));
    }
    let mut l = Leser::neu(daten).mit_namengrenze(g.name_bytes);
    let mut m = Modul::leer();
    m.byte_groesse = daten.len();

    // --- Kopf: Magie + Version ------------------------------------------
    let magic = l.nimm(4, "magic")?;
    if magic != b"\0asm" {
        return Err(Fehler::form(
            ART_MAGIE,
            format!("magic {magic:02x?} statt 00 61 73 6d"),
        ));
    }
    let version = l.u32le("Version")?;
    if version != 1 {
        return Err(Fehler::form(
            ART_VERSION,
            format!("Version {version} statt 1"),
        ));
    }

    let mut letzte_rang: u8 = 0;
    // Bitmaske der schon gesehenen Abschnitte (0 = custom, beliebig oft).
    let mut gesehen: u16 = 0;
    // (Typindex aus der Funktionssektion) fuer die eigenen Funktionen, in
    // Reihenfolge. Die Code-Sektion schreibt die Koerper in gleicher Folge.
    let mut eigene_typen: Vec<u32> = Vec::new();
    let mut abschnitt_nr: u32 = 0;
    // M3: Sektionsschleife ist ueber die Eingabe begrenzt.
    while !l.ist_ende() {
        abschnitt_nr += 1;
        if abschnitt_nr as usize > g.sektionen {
            return Err(Fehler::form(
                ART_GRENZE,
                format!("mehr als {} Sektionen", g.sektionen),
            ));
        }
        let art = l.u8("Sektionsart")?;
        let groesse = l.leb_u32(true)? as usize;
        // M4: Sektionsinhalt vor dem Fenster pruefen.
        if groesse > l.uebrig() {
            return Err(Fehler::form(
                ART_LAENGE,
                format!(
                    "Sektion {art:#04x}: {groesse} Byte angekuendigt, nur {} da",
                    l.uebrig()
                ),
            ));
        }
        let inhalt = l.nimm(groesse, "Sektionsinhalt")?;

        if art != 0 {
            let r = rang(art);
            if r == 0 {
                return Err(Fehler::form(
                    ART_REIHENFOLGE,
                    format!("Sektionsart {art} unbekannt (0..=12 erlaubt)"),
                ));
            }
            let _ = sektionsname(art);
            if r < letzte_rang {
                return Err(Fehler::form(
                    ART_REIHENFOLGE,
                    format!(
                        "Sektion {art} kommt nach Rang {letzte_rang} (Spezifikation verlangt aufsteigende Reihenfolge)"
                    ),
                ));
            }
            letzte_rang = r;
        }
        // Jeder Abschnitt darf hoechstens einmal kommen („unexpected
        // content after last section"). GEMESSEN 02.10. (`binary.wast`:
        // zwei Start-, zwei Funktions- und zwei Codeabschnitte).
        if art != 0 {
            if gesehen & (1u16 << art) != 0 {
                return Err(Fehler::form(
                    ART_REIHENFOLGE,
                    format!("Abschnitt {art} kommt mehrfach vor"),
                ));
            }
            gesehen |= 1u16 << art;
        }

        let mut s = Leser::neu(inhalt).mit_namengrenze(g.name_bytes);
        match art {
            0 => {
                let name = s.name()?;
                let inhalt = s.nimm_kopie(s.uebrig(), "custom")?;
                if s.uebrig() != 0 {
                    return Err(Fehler::form(ART_REST, "custom: Restbytes nach dem Inhalt"));
                }
                // `name`-Sektion: Woerterbuchfunktion -> Index (nur lesbar).
                if name == "name" {
                    m.namen = Some(lies_namen(&inhalt, g)?);
                }
                if m.custom.len() >= g.sektionen {
                    return Err(Fehler::form(ART_GRENZE, "zu viele custom Sektionen"));
                }
                m.custom.push(Custom {
                    name,
                    inhalt,
                    abschnitt: art,
                });
            }
            1 => lies_typen(&mut s, &mut m, g)?,
            2 => lies_importe(&mut s, &mut m, g)?,
            3 => {
                let n = s.vektor_len(g.funktionen, "Funktionsabschnitt")?;
                for _ in 0..n {
                    let t = s.leb_u32(true)?;
                    eigene_typen.push(t);
                }
                rest_oder_fehler(&s, "Funktionsabschnitt")?;
            }
            4 => lies_tabellen(&mut s, &mut m, g)?,
            5 => lies_speicher(&mut s, &mut m, g)?,
            6 => lies_globale(&mut s, &mut m, g)?,
            7 => lies_exporte(&mut s, &mut m, g)?,
            8 => {
                m.start = Some(s.leb_u32(true)?);
                rest_oder_fehler(&s, "Startabschnitt")?;
            }
            9 => lies_elemente(&mut s, &mut m, g)?,
            10 => lies_code(&mut s, &mut m, &eigene_typen, g)?,
            11 => lies_daten(&mut s, &mut m, g)?,
            12 => m.daten_anzahl = Some(s.leb_u32(true)?),
            // `rang()` ist die letzte Instanz: der Bereich ist
            // abgedeckt, aber eine NEUE Sektionsnummer (oder ein Bug im
            // Aufrufer) darf keinen Panic erzeugen (M8) — benannter
            // Fehler statt `unreachable!()`.
            //
            // GEMESSEN 03.10. (Schlussphase: systematische Pruefung).
            id => {
                return Err(Fehler::form(
                    ART_SEKTION,
                    format!("Sektionsnummer {id} unbekannt"),
                ));
            }
        }
    }

    // --- Nachtrag: eigene Funktionen anlegen --------------------------------
    // Der Code-Abschnitt hat die Koerper bereits in `m.funktionen` gelegt;
    // die Funktionssektion liefert die Typindizes in derselben Reihenfolge.
    // Die Importe stehen schon im Vektor und BEHALTEN ihre Position (Index 0..n).
    for (i, &typ) in eigene_typen.iter().enumerate() {
        if typ as usize >= m.typen.len() {
            return Err(Fehler::form(
                ART_INDEX,
                format!(
                    "Funktionsabschnitt: Typindex {typ} unbekannt (nur {} Typen)",
                    m.typen.len()
                ),
            ));
        }
        // Position = Importe + i.
        let pos = m.importierte_funktionen() + i;
        if pos >= m.funktionen.len() {
            return Err(Fehler::form(
                ART_ZAEHLER,
                format!("Codeabschnitt fehlt fuer eigene Funktion {i}"),
            ));
        }
        match &mut m.funktionen[pos] {
            Func::Eigen { typ: t, .. } => *t = typ,
            _ => {
                return Err(Fehler::form(
                    ART_ZAEHLER,
                    "Import-/Code-Reihenfolge stimmt nicht",
                ));
            }
        }
    }
    Ok(m)
}

/// Nach dem Lesen einer Sektion: keine Restbytes erlaubt.
fn rest_oder_fehler(l: &Leser, was: &str) -> Result<(), Fehler> {
    if l.uebrig() != 0 {
        return Err(Fehler::form(
            ART_REST,
            format!("{was}: {} unerwartete Restbytes", l.uebrig()),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 1 Typ
// ---------------------------------------------------------------------------
fn lies_typen(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.typen, "Typabschnitt")?;
    m.typen.reserve(n.min(4096));
    for _ in 0..n {
        let form = s.u8("Typform")?;
        if form != 0x60 {
            return Err(Fehler::form(
                ART_BLOCKTYP,
                format!("Typform {form:#04x}, nur 0x60 (func) in Wasm 2.0 Core"),
            ));
        }
        let pn = s.vektor_len(g.name_bytes, "Parameterliste")?;
        let mut parameter = Vec::with_capacity(pn.min(1024));
        for _ in 0..pn {
            parameter.push(lies_valtyp(s)?);
        }
        let en = s.vektor_len(g.name_bytes, "Ergebnisliste")?;
        // M4: Ergebnisse sind bei MVP/2.0 durch die Stapelhoehe begrenzt, das
        // Feld kann aber absurd gross sein -> vektor_len hat es geprueft.
        let mut ergebnis = Vec::with_capacity(en.min(1024));
        for _ in 0..en {
            ergebnis.push(lies_valtyp(s)?);
        }
        m.typen.push(FuncTyp {
            parameter,
            ergebnis,
        });
    }
    rest_oder_fehler(s, "Typabschnitt")
}

// ---------------------------------------------------------------------------
// 2 Import
// ---------------------------------------------------------------------------
fn lies_importe(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.importe, "Importabschnitt")?;
    for _ in 0..n {
        let modul = s.name()?;
        let feld = s.name()?;
        let art = s.u8("Importart")?;
        // The import order for the JS API (`Modul::import_reihe`).
        let reihe = match art {
            0x00 => Some(Art::Func(m.funktionen.len() as u32)),
            0x01 => Some(Art::Tabelle(m.tabellen.len() as u32)),
            0x02 => Some(Art::Speicher(m.speicher.len() as u32)),
            0x03 => Some(Art::Global(m.globale.len() as u32)),
            0x04 => Some(Art::Tag(m.tags.len() as u32)),
            _ => None,
        };
        if let Some(r) = reihe {
            m.import_reihe.push(r);
        }
        let import_art = match art {
            0x00 => {
                let t = s.leb_u32(true)?;
                m.funktionen.push(Func::Importiert {
                    modul,
                    feld,
                    typ: t,
                });
                continue; // Funktionen stehen schon im Indexraum
            }
            0x01 => {
                let tt = lies_tabellentyp(s, g)?;
                Importart::Tabelle(tt)
            }
            0x02 => {
                let st = lies_speichertyp(s, g)?;
                Importart::Speicher(st)
            }
            0x03 => {
                let t = lies_valtyp(s)?;
                let veraenderbar = lies_mutierbarkeit(s)?;
                Importart::Global(t, veraenderbar)
            }
            _ => {
                return Err(Fehler::form(
                    ART_OPCODE,
                    format!("Importart {art:#04x} unbekannt"),
                ));
            }
        };
        match &import_art {
            Importart::Tabelle(_) => {
                m.tabellen.push(ImportiertTab::Importiert {
                    modul: modul.clone(),
                    feld: feld.clone(),
                    typ: match import_art {
                        Importart::Tabelle(t) => t,
                        // Die Beschriftung oben sagt `Tabelle`; ein
                        // anderer Fall waere ein innerer Fehler — trotzdem
                        // kein Panic, sondern ein benannter Fehler (M8).
                        andere => {
                            return Err(Fehler::form(
                                ART_TABELLE,
                                format!("Importart passt nicht zur Tabelle: {andere:?}"),
                            ));
                        }
                    },
                });
            }
            Importart::Speicher(_) => {
                m.speicher.push(ImportiertSpeicher::Importiert {
                    modul: modul.clone(),
                    feld: feld.clone(),
                    typ: match import_art {
                        Importart::Speicher(t) => t,
                        andere => {
                            return Err(Fehler::form(
                                ART_SPEICHER,
                                format!("Importart passt nicht zum Speicher: {andere:?}"),
                            ));
                        }
                    },
                });
            }
            Importart::Global(t, v) => {
                m.globale.push(Global {
                    typ: *t,
                    veraenderbar: *v,
                    start: None,
                    import: Some((modul.clone(), feld.clone())),
                });
            }
            _ => {}
        }
    }
    rest_oder_fehler(s, "Importabschnitt")
}

fn lies_mutierbarkeit(s: &mut Leser) -> Result<bool, Fehler> {
    match s.u8("Mutierbarkeit")? {
        0x00 => Ok(false),
        0x01 => Ok(true),
        b => Err(Fehler::form(
            ART_OPCODE,
            format!("Mutierbarkeitsbyte {b:#04x}, nur 0 oder 1"),
        )),
    }
}

fn lies_limits(s: &mut Leser, g: &Grenzen, waren: &str) -> Result<Limits, Fehler> {
    let flags = s.u8("Limitbyte")?;
    match flags {
        0x00 => Ok(Limits {
            min: s.leb_u32(true)? as u64,
            max: None,
        }),
        0x01 => {
            let min = s.leb_u32(true)? as u64;
            let max = s.leb_u32(true)? as u64;
            if min > max {
                return Err(Fehler::form(
                    ART_SPEICHER,
                    format!("{waren}: min {min} groesser als max {max}"),
                ));
            }
            Ok(Limits {
                min,
                max: Some(max),
            })
        }
        0x02 | 0x03 => Err(Fehler::form(
            ART_SPEICHER,
            format!("{waren}: geteilter Speicher (Threads) wird nicht unterstuetzt"),
        )),
        0x04..=0x07 => Err(Fehler::form(
            ART_SPEICHER,
            format!("{waren}: 64-Bit-Speicher (memory64) wird nicht unterstuetzt"),
        )),
        _ => Err(Fehler::form(
            ART_SPEICHER,
            format!("{waren}: Limitbyte {flags:#04x} unbekannt"),
        )),
    }
    .map(|l| {
        // M4: Speicherseiten hart begrenzen.
        let grenze = if waren.starts_with("Speicher") {
            g.speicher_seiten
        } else {
            g.tabelle_plaetze
        };
        if l.min > grenze {
            return Err(Fehler::form(
                ART_SPEICHER,
                format!("{waren}: min {} Byte ueber der Grenze {grenze}", l.min),
            ));
        }
        Ok(l)
    })?
}

/// `(ref func)`: jede Funktionsindex-Liste traegt diesen Typ.
fn nicht_leerbare_funktionsreferenz() -> Reftyp {
    Reftyp::Konkret {
        typ: crate::modell::JEDER_FUNKTIONSTYP,
        nullable: false,
    }
}

/// `elemkind 0x00` in einer Funktionsindex-Liste bedeutet `(ref func)`.
fn elemkind_als_funktionsliste(r: Reftyp) -> Reftyp {
    match r {
        Reftyp::Func => nicht_leerbare_funktionsreferenz(),
        andere => andere,
    }
}

/// Heaptype, der auf 0x63/0x64 folgt (Wasm 3.0, Typed Function
/// References). Kurzformen als Byte: 0x70 `func`, 0x69 `none`/`nofunc`,
/// 0x6f `extern`, 0x68 `noextern`; alles andere ist ein Typindex als s33.
///
/// GEMESSEN 02.10. (`elem.wast` Z. 52ff: 8 Faelle).
fn lies_heaptype(s: &mut Leser, nullable: bool) -> Result<Reftyp, Fehler> {
    let b = s
        .sieht()
        .ok_or_else(|| Fehler::form(ART_UNVOLLSTAENDIG, "Heaptype fehlt"))?;
    match b {
        0x70 => {
            s.u8("Heaptype")?;
            // `(ref func)` / `(ref null func)`: jeder Funktionstyp.
            Ok(Reftyp::Konkret {
                typ: crate::modell::JEDER_FUNKTIONSTYP,
                nullable,
            })
        }
        0x69 => {
            s.u8("Heaptype")?;
            if !nullable {
                return Err(Fehler::form(
                    ART_REFTYP,
                    "Referenztyp `(ref none)` ohne `null` wird nicht unterstuetzt",
                ));
            }
            // `(ref null none)`: passt in jeden Referenztyp.
            Ok(Reftyp::Konkret {
                typ: crate::modell::JEDER_FUNKTIONSTYP,
                nullable: true,
            })
        }
        // `extern` (0x6f) ist ein ganz normaler Heaptyp; `noextern`
        // (0x68) passt nur in einen nullable Typ.
        //
        // GEMESSEN 03.10. (`table.wast` Z. 30: `(ref extern)`).
        0x6f => {
            s.u8("Heaptype")?;
            Ok(Reftyp::Konkret {
                typ: crate::modell::JEDER_EXTERNTYP,
                nullable,
            })
        }
        0x68 => {
            s.u8("Heaptype")?;
            if !nullable {
                return Err(Fehler::form(
                    ART_REFTYP,
                    "Referenztyp `(ref noextern)` ohne `null` wird nicht unterstuetzt",
                ));
            }
            Ok(Reftyp::Konkret {
                typ: crate::modell::JEDER_EXTERNTYP,
                nullable: true,
            })
        }
        0x6e | 0x6d | 0x6c | 0x6b | 0x6a | 0x67 => Err(Fehler::form(
            ART_REFTYP,
            format!(
                "Referenztyp {b:#04x} (externe Referenz oder GC-Typen) wird nicht unterstuetzt"
            ),
        )),
        _ => {
            let idx = s.leb_s33(true)?;
            if idx < 0 || idx > u32::MAX as i64 {
                return Err(Fehler::form(
                    ART_REFTYP,
                    format!("Heaptyp {idx} ist kein Typindex"),
                ));
            }
            Ok(Reftyp::Konkret {
                typ: idx as u32,
                nullable,
            })
        }
    }
}

/// Referenztyp mit moeglichem Typindex: 0x63 `(ref null ht)`, 0x64
/// `(ref ht)`, sonst 0x70/0x6f.
fn lies_reftyp_breit(s: &mut Leser) -> Result<Reftyp, Fehler> {
    match s.sieht() {
        Some(0x63) => {
            s.u8("Referenztyp")?;
            lies_heaptype(s, true)
        }
        Some(0x64) => {
            s.u8("Referenztyp")?;
            lies_heaptype(s, false)
        }
        _ => lies_reftyp(s),
    }
}

/// Werttyp mit moeglichem Typindex (Parameter, Ergebnisse, Lokale, globale
/// Typen, Blocktypen).
pub fn lies_valtyp(s: &mut Leser) -> Result<Valtyp, Fehler> {
    match s.sieht() {
        Some(0x63) | Some(0x64) => Ok(Valtyp::Ref(lies_reftyp_breit(s)?)),
        _ => valtyp_lesen(s.u8("Werttyp")?),
    }
}

fn lies_tabellentyp(s: &mut Leser, g: &Grenzen) -> Result<Tabellentyp, Fehler> {
    let reftyp = lies_reftyp_breit(s)?;
    let grenzen = lies_limits(s, g, "Tabelle")?;
    Ok(Tabellentyp { reftyp, grenzen })
}

fn lies_speichertyp(s: &mut Leser, g: &Grenzen) -> Result<Speichertyp, Fehler> {
    let grenzen = lies_limits(s, g, "Speicher")?;
    Ok(Speichertyp { grenzen })
}

// ---------------------------------------------------------------------------
// 4 Tabelle (eigene)
// ---------------------------------------------------------------------------
fn lies_tabellen(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.tabellen, "Tabellenabschnitt")?;
    for _ in 0..n {
        // Wasm 3.0: `0x40 0x00 <tabellentyp> <ausdruck>` ist eine Tabelle
        // mit Startwert. Wir lesen sie als Tabelle plus aktive
        // Elementliste an Versatz 0 (genau die Bedeutung).
        //
        // GEMESSEN 02.10. (`elem.wast` Z. 52ff: 8 Faelle mit
        // `\40\00\64\70\00\01\d2\00\0b`).
        if s.sieht() == Some(0x40) {
            s.u8("Tabellenpraefix")?;
            let form = s.u8("Tabellenform")?;
            if form != 0x00 {
                return Err(Fehler::form(
                    ART_TABELLE,
                    format!("Tabellenpraefix 0x40 {form:#04x} unbekannt"),
                ));
            }
            let tt = lies_tabellentyp(s, g)?;
            let wert = lies_konstanten_ausdruck(s, g)?;
            m.tabellen.push(ImportiertTab::Eigen {
                typ: tt,
                startwert: Some(wert),
            });
            continue;
        }
        let tt = lies_tabellentyp(s, g)?;
        m.tabellen.push(ImportiertTab::Eigen {
            typ: tt,
            startwert: None,
        });
    }
    rest_oder_fehler(s, "Tabellenabschnitt")
}

// ---------------------------------------------------------------------------
// 5 Speicher (eigene)
// ---------------------------------------------------------------------------
fn lies_speicher(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(1, "Speicherabschnitt")?;
    if n > 1 {
        return Err(Fehler::form(
            ART_SPEICHER,
            "mehr als ein Speicher (Multi-Memory ist Wasm 3.0)",
        ));
    }
    for _ in 0..n {
        let st = lies_speichertyp(s, g)?;
        m.speicher.push(ImportiertSpeicher::Eigen { typ: st });
    }
    rest_oder_fehler(s, "Speicherabschnitt")
}

// ---------------------------------------------------------------------------
// 6 Globale
// ---------------------------------------------------------------------------
fn lies_globale(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.globale, "Globalabschnitt")?;
    for _ in 0..n {
        let t = lies_valtyp(s)?;
        let veraenderbar = lies_mutierbarkeit(s)?;
        let start = lies_konstanten_ausdruck(s, g)?;
        m.globale.push(Global {
            typ: t,
            veraenderbar,
            start: Some(start),
            import: None,
        });
    }
    rest_oder_fehler(s, "Globalabschnitt")
}

/// Konstanter Ausdruck (Wasm 2.0 Core: const + `global.get` nur importierter
/// *unveraenderlicher* Globale). Der Validator macht den Rest.
fn lies_konstanten_ausdruck(s: &mut Leser, g: &Grenzen) -> Result<Vec<Befehl>, Fehler> {
    let mut v = Vec::new();
    let tiefe = 0usize;
    loop {
        if v.len() > 4096 {
            return Err(Fehler::form(ART_GRENZE, "konstanter Ausdruck zu lang"));
        }
        let b = lies_befehl(s, tiefe)?;
        match b {
            Befehl::End => {
                v.push(b);
                break;
            }
            Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_) => {
                return Err(Fehler::form(
                    ART_ABHAENGIG,
                    "Block im konstanten Ausdruck (nur const/global.get erlaubt)",
                ));
            }
            _ => v.push(b),
        }
    }
    let _ = g;
    Ok(v)
}

// ---------------------------------------------------------------------------
// 7 Export
// ---------------------------------------------------------------------------
fn lies_exporte(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.exporte, "Exportabschnitt")?;
    let mut gesehen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for _ in 0..n {
        let name = s.name()?;
        let kb = s.u8("Exportart")?;
        let idx = s.leb_u32(true)?;
        let art = match kb {
            0 => Art::Func(idx),
            1 => Art::Tabelle(idx),
            2 => Art::Speicher(idx),
            3 => Art::Global(idx),
            _ => {
                return Err(Fehler::form(
                    ART_OPCODE,
                    format!("Exportart {kb:#04x} unbekannt"),
                ));
            }
        };
        if !gesehen.insert(name.clone()) {
            return Err(Fehler::form(
                ART_DOPPELT,
                format!("Exportname {name:?} doppelt"),
            ));
        }
        m.exporte.push(Export { name, art });
    }
    rest_oder_fehler(s, "Exportabschnitt")
}

// ---------------------------------------------------------------------------
// 9 Elemente (8 Formen)
// ---------------------------------------------------------------------------
fn lies_elemente(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.elementabschnitte, "Elementabschnitt")?;
    for _ in 0..n {
        let flags = s.leb_u32(true)?;
        let elem = match flags {
            // Formen 0..3 sind Listen von Funktionsindizes: ihr Typ ist
            // `(ref func)` (nicht-leerbar), NICHT `funcref` — sonst passt
            // `(table 1 (ref func) …)` nicht (GEMESSEN 02.10.,
            // `elem.wast` Z. 2/51/52).
            0 => Element::Aktiv {
                tabelle: 0,
                offset: lies_konstanten_ausdruck(s, g)?,
                reftyp: nicht_leerbare_funktionsreferenz(),
                eintraege: lies_funktionsliste(s, g)?,
            },
            1 => Element::Passiv {
                reftyp: elemkind_als_funktionsliste(lies_elemkind(s)?),
                eintraege: lies_funktionsliste(s, g)?,
            },
            2 => Element::Aktiv {
                tabelle: s.leb_u32(true)?,
                offset: lies_konstanten_ausdruck(s, g)?,
                reftyp: elemkind_als_funktionsliste(lies_elemkind(s)?),
                eintraege: lies_funktionsliste(s, g)?,
            },
            3 => Element::Deklarativ {
                reftyp: elemkind_als_funktionsliste(lies_elemkind(s)?),
                eintraege: lies_funktionsliste(s, g)?,
            },
            4 => Element::Aktiv {
                tabelle: 0,
                offset: lies_konstanten_ausdruck(s, g)?,
                reftyp: Reftyp::Func,
                eintraege: lies_ausdrucksliste(s, g)?,
            },
            5 => Element::Passiv {
                reftyp: lies_reftyp_breit(s)?,
                eintraege: lies_ausdrucksliste(s, g)?,
            },
            6 => Element::Aktiv {
                tabelle: s.leb_u32(true)?,
                offset: lies_konstanten_ausdruck(s, g)?,
                reftyp: lies_reftyp_breit(s)?,
                eintraege: lies_ausdrucksliste(s, g)?,
            },
            7 => Element::Deklarativ {
                reftyp: lies_reftyp_breit(s)?,
                eintraege: lies_ausdrucksliste(s, g)?,
            },
            _ => {
                return Err(Fehler::form(
                    ART_OPCODE,
                    format!("Elementform {flags} unbekannt (0..=7 in Wasm 2.0)"),
                ));
            }
        };
        m.elemente.push(elem);
    }
    rest_oder_fehler(s, "Elementabschnitt")
}

fn lies_elemkind(s: &mut Leser) -> Result<Reftyp, Fehler> {
    match s.u8("elemkind")? {
        0x00 => Ok(Reftyp::Func),
        b => Err(Fehler::form(
            ART_TABELLE,
            format!("elemkind {b:#04x}, nur 0x00 (funcref)"),
        )),
    }
}

fn lies_reftyp(s: &mut Leser) -> Result<Reftyp, Fehler> {
    let b = s.u8("Referenztyp")?;
    Reftyp::von_byte(b).ok_or_else(|| crate::modell::reftyp_ungueltig(format!("{b:#04x}")))
}

fn lies_funktionsliste(s: &mut Leser, g: &Grenzen) -> Result<Vec<Vec<Befehl>>, Fehler> {
    let n = s.vektor_len(g.elementabschnitte, "Funktionsliste")?;
    let mut v = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        v.push(vec![Befehl::RefFunc(s.leb_u32(true)?), Befehl::End]);
    }
    Ok(v)
}

fn lies_ausdrucksliste(s: &mut Leser, g: &Grenzen) -> Result<Vec<Vec<Befehl>>, Fehler> {
    let n = s.vektor_len(g.elementabschnitte, "Ausdrucksliste")?;
    let mut v = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        v.push(lies_konstanten_ausdruck(s, g)?);
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// 10 Code
// ---------------------------------------------------------------------------
fn lies_code(
    s: &mut Leser,
    m: &mut Modul,
    eigene_typen: &[u32],
    g: &Grenzen,
) -> Result<(), Fehler> {
    let n = s.vektor_len(g.funktionen, "Codeabschnitt")?;
    if n != eigene_typen.len() {
        return Err(Fehler::form(
            ART_ZAEHLER,
            format!(
                "Codeabschnitt: {n} Koerper, Funktionsabschnitt: {} Typen",
                eigene_typen.len()
            ),
        ));
    }
    for i in 0..n {
        let groesse = s.leb_u32(true)? as usize;
        if groesse > g.code_bytes {
            return Err(Fehler::form(
                ART_GRENZE,
                format!(
                    "Funktionskoerper {i}: {groesse} Byte ueber der Grenze {}",
                    g.code_bytes
                ),
            ));
        }
        if groesse > s.uebrig() {
            return Err(Fehler::form(
                ART_LAENGE,
                format!(
                    "Funktionskoerper {i}: {groesse} Byte angekuendigt, {} da",
                    s.uebrig()
                ),
            ));
        }
        let inhalt = s.nimm(groesse, "Funktionskoerper")?;
        let mut k = Leser::neu(inhalt).mit_namengrenze(g.name_bytes);
        // Lokaltypen als Komprimierung: Folge gleicher Typen.
        let gn = k.vektor_len(g.lokale_je_funktion, "Lokalgruppen")?;
        let mut lokale: Vec<Valtyp> = Vec::new();
        for _ in 0..gn {
            let anzahl = k.leb_u32(true)? as usize;
            let t = lies_valtyp(&mut k)?;
            // M4: anzahl * 1 Byte muesssen im Koerper noch da sein.
            if anzahl > k.uebrig() + 1 {
                return Err(Fehler::form(
                    ART_GRENZE,
                    format!(
                        "Funktionskoerper {i}: {anzahl} Lokale vom Typ {} passen nicht hinein",
                        t.text()
                    ),
                ));
            }
            if lokale.len() + anzahl > g.lokale_je_funktion {
                return Err(Fehler::form(
                    ART_GRENZE,
                    format!(
                        "Funktionskoerper {i}: mehr als {} Lokale (Grenze {})",
                        lokale.len() + anzahl,
                        g.lokale_je_funktion
                    ),
                ));
            }
            lokale.resize(lokale.len() + anzahl, t);
        }
        // Befehle bis zum abschliessenden `end`.
        let mut koerper: Vec<Befehl> = Vec::new();
        let mut tiefe: usize = 1;
        loop {
            if koerper.len() >= g.befehle_je_funktion {
                return Err(Fehler::form(
                    ART_GRENZE,
                    format!(
                        "Funktionskoerper {i}: mehr als {} Befehle",
                        g.befehle_je_funktion
                    ),
                ));
            }
            if k.ist_ende() {
                return Err(Fehler::form(
                    crate::fehler::ART_UNVOLLSTAENDIG,
                    format!("Funktionskoerper {i}: kein abschliessendes end"),
                ));
            }
            let b = lies_befehl(&mut k, tiefe)?;
            let ende = matches!(b, Befehl::End);
            let oeffnet = matches!(b, Befehl::Block(_) | Befehl::Loop(_) | Befehl::If(_));
            if oeffnet {
                tiefe += 1;
            } else if ende {
                tiefe -= 1;
            }
            koerper.push(b);
            if ende && tiefe == 0 {
                break;
            }
        }
        rest_oder_fehler(&k, "Funktionskoerper")?;
        m.funktionen.push(Func::Eigen {
            typ: 0, // wird im Nachtrag aus `eigene_typen` gesetzt
            lokale,
            koerper,
            befehl_offset: i,
        });
    }
    rest_oder_fehler(s, "Codeabschnitt")
}

// ---------------------------------------------------------------------------
// 11 Daten
// ---------------------------------------------------------------------------
fn lies_daten(s: &mut Leser, m: &mut Modul, g: &Grenzen) -> Result<(), Fehler> {
    let n = s.vektor_len(g.datenabschnitte, "Datenabschnitt")?;
    for _ in 0..n {
        let flags = s.leb_u32(true)?;
        let d = match flags {
            0 => {
                let offset = lies_konstanten_ausdruck(s, g)?;
                let n = s.leb_u32(true)? as usize;
                let inhalt = s.nimm_kopie(n, "Dateninhalt")?;
                Daten::Aktiv {
                    speicher: 0,
                    offset,
                    inhalt,
                }
            }
            1 => {
                let n = s.leb_u32(true)? as usize;
                let inhalt = s.nimm_kopie(n, "Dateninhalt")?;
                Daten::Passiv { inhalt }
            }
            2 => {
                let speicher = s.leb_u32(true)?;
                let offset = lies_konstanten_ausdruck(s, g)?;
                let n = s.leb_u32(true)? as usize;
                let inhalt = s.nimm_kopie(n, "Dateninhalt")?;
                Daten::Aktiv {
                    speicher,
                    offset,
                    inhalt,
                }
            }
            _ => {
                return Err(Fehler::form(
                    ART_DATEN,
                    format!("Datenform {flags} unbekannt (0..=2)"),
                ));
            }
        };
        m.daten.push(d);
    }
    rest_oder_fehler(s, "Datenabschnitt")
}

// ---------------------------------------------------------------------------
// `name`-Sektion (Woerterbuchfunktionen)
// ---------------------------------------------------------------------------
fn lies_namen(inhalt: &[u8], g: &Grenzen) -> Result<crate::modell::Namen, Fehler> {
    let mut l = Leser::neu(inhalt).mit_namengrenze(g.name_bytes);
    let mut namen = std::collections::BTreeMap::new();
    // Der Inhalt ist eine FOLGE von Unterabschnitten `id, groesse, inhalt`
    // — OHNE Zaehler davor (Spec: `namedata = vec(subsection)` ist hier
    // also keine gezaehlte Liste; die Unterabschnitte laufen bis zum Ende).
    //
    // GEMESSEN 03.10. an einer echten Datei (`html.wasm`, die 36 Byte nach
    // `04 6e 61 6d 65`): `01 ca 03 19 00 06 63 61 6c 6c 6f 63 …` — also
    // Unterabschnitt 1, 458 Byte, 25 Eintraege, erstes (Index 0, „calloc").
    //
    // VORHER las der Leser ZUERST einen Zaehler: der war nichts anderes als
    // diese `01`, danach kam `id = 0xca`, die Schleife endete nach einem
    // Durchgang — leer. Folge: KEIN Binaermodul bekam je einen
    // Funktionsnamen, der Aufrufstapel zeigte immer `func#n`
    // (team/rollen/WASM.txt: „Aufrufstapel mit Funktionsnamen").
    while l.uebrig() > 0 {
        let id = l.u8("Namensunterabschnitt")?;
        let groesse = l.leb_u32(true)? as usize;
        if groesse > l.uebrig() {
            return Err(Fehler::form(ART_LAENGE, "name: Unterabschnitt zu lang"));
        }
        let unter = l.nimm(groesse, "Namens-Unterabschnitt")?;
        if id == 1 {
            // Funktionsnamentabelle: Anzahl, dann (Index, Name)*
            let mut k = Leser::neu(unter).mit_namengrenze(g.name_bytes);
            let cnt = k.vektor_len(g.funktionen, "Funktionsnamen")?;
            for _ in 0..cnt {
                let idx = k.leb_u32(true)?;
                let name = k.name()?;
                namen.insert(name, idx);
            }
        }
        // Alle uebrigen Unterabschnitte (0 Modul, 2 lokale Namen, 3 Labels,
        // 4 Typen, 5 …) ueberspringen — groesse Bytes sind bereits gefressen.
    }
    Ok(namen)
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// Kleines gueltiges Modul: `(func (export "f") (result i32) i32.const 42)`.
    /// Von Hand gebaut, damit der Test unabhaengig vom WAT-Leser (W3) ist.
    fn modul() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"\0asm\x01\0\0\0");
        // Typabschnitt (1): Groesse 5, 1 Typ, () -> i32
        v.extend_from_slice(&[0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]);
        // Funktionsabschnitt (3): Groesse 2, 1 Funktion, Typindex 0
        v.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
        // Exportabschnitt (7): Groesse 5, 1 Export, "f", func 0
        v.extend_from_slice(&[0x07, 0x05, 0x01, 0x01, b'f', 0x00, 0x00]);
        // Codeabschnitt (10): Groesse 6 = 1 (Anzahl) + 1 (Body-Laenge) + 4
        let body = [0x00, 0x41, 0x2a, 0x0b];
        v.extend_from_slice(&[0x0a, 0x06, 0x01, body.len() as u8]);
        v.extend_from_slice(&body);
        v
    }

    /// M7: Ergebnis sichern (Signatur, Koerper, Export).
    #[test]
    fn kleines_modul() {
        let g = Grenzen::standard();
        let m = lies_modul(&modul(), &g).unwrap();
        assert_eq!(m.typen.len(), 1);
        assert_eq!(m.typen[0].ergebnis, vec![Valtyp::I32]);
        assert_eq!(m.funktionen.len(), 1);
        assert_eq!(m.koerper(0).unwrap(), &[Befehl::I32Konst(42), Befehl::End]);
        assert_eq!(m.exporte[0].name, "f");
        assert_eq!(m.exporte[0].art.index(), 0);
        assert_eq!(m.byte_groesse, modul().len());
    }

    /// M8/M7: boesartige Eingaben -> benannte Fehler, kein Panic.
    #[test]
    fn benannte_formfehler() {
        let g = Grenzen::standard();
        assert_eq!(
            lies_modul(b"", &g).unwrap_err().art(),
            crate::fehler::ART_UNVOLLSTAENDIG
        );
        assert_eq!(
            lies_modul(b"\0asm\x02\0\0\0", &g).unwrap_err().art(),
            crate::fehler::ART_VERSION
        );
        assert_eq!(
            lies_modul(b"nope", &g).unwrap_err().art(),
            crate::fehler::ART_MAGIE
        );
        // Sektion unbekannt
        assert_eq!(
            lies_modul(b"\0asm\x01\0\0\0\x7f\x00", &g)
                .unwrap_err()
                .art(),
            ART_REIHENFOLGE
        );
        // Sektion zu lang angekuendigt (0x7f Byte, es sind 0 da)
        assert_eq!(
            lies_modul(b"\0asm\x01\0\0\0\x01\x7f", &g)
                .unwrap_err()
                .art(),
            crate::fehler::ART_LAENGE
        );
        // Modul zu gross (M4: die Grenze greift VOR dem Lesen)
        let k = Grenzen {
            modul_bytes: 8,
            ..Grenzen::bombe()
        };
        assert_eq!(lies_modul(&modul(), &k).unwrap_err().art(), ART_GRENZE);
    }
}
