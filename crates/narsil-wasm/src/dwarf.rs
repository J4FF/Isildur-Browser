//! W5: DWARF-Zeilen aus den benutzerdefinierten Sektionen.
//!
//! Die WebAssembly-Toolchain legt Debug-Information als `custom`-Sektion ab:
//! je Funktion eine Sektion, deren Name der Funktionsindex als Zeichenkette
//! ist (`"12"`), und darin die DWARF-Abschnitte `.debug_info`,
//! `.debug_abbrev`, `.debug_line`, `.debug_str` (LLVM-Standard: „DWARF in
//! custom sections"). Aelterere Werkzeuge legen sie modulweit ab
//! (`.debug_line` als eigener Abschnitt).
//!
//! Gelesen wird hier nur, was die Entwicklerwerkzeuge fuer
//! „Zeile an dieser Adresse" brauchen: die Zeilentabelle des
//! `.debug_line`-Programms (Version 2–5, Kopfdaten und Standard-, Sonder-
//! und Erweiterungsopcodes nach Spec 6.2.5). Adressabbildung:
//! Byte-Offset im Koerper -> (Zeile, Spalte, Dateiname).
//!
//! GEMESSEN 01.10.: Der Programmbeginn steht im `header_length` (Version 2–4
//! als u32, Version 5 als ULEB128 nach `address_size` und
//! `segment_selector_size`) — der Leser folgt ihm, statt die Dateiliste
//! zu raten. Die Unteropcodes der Sonderopcodes sind ULEB128 (DWARF 2:
//! ein Byte, was ULEB128 genauso liest); `DW_FORM_*`-Offsets in
//! `.debug_str` werden uebersprungen, nicht aufgeloest (ohne die
//! Sektion gibt es keinen Text — der Sprung muss trotzdem exakt sein).

use crate::bytes::Leser;
use crate::fehler::{ART_UNVOLLSTAENDIG, Fehler};
use crate::grenzen::Grenzen;
use crate::modell::Modul;

/// Eine Zeile der Zuordnung Adresse -> Quelle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zeile {
    /// Byte-Offset im Funktionskoerper.
    pub adresse: u32,
    /// Quellzeile (1-basiert, 0 = unbekannt).
    pub zeile: u32,
    pub spalte: u32,
    /// Dateiname, falls im Programmheader benannt.
    pub datei: String,
}

fn dw(d: &str) -> Fehler {
    Fehler::form(ART_UNVOLLSTAENDIG, format!("DWARF: {d}"))
}

// ---- Standardopcodes `DW_LNS_*` (DWARF 5, Abschnitt 6.2.5.1) ----
const DW_LNS_COPY: u8 = 1;
const DW_LNS_ADVANCE_PC: u8 = 2;
const DW_LNS_ADVANCE_LINE: u8 = 3;
const DW_LNS_SET_FILE: u8 = 4;
const DW_LNS_SET_COLUMN: u8 = 5;
const DW_LNS_CONST_ADD_PC: u8 = 8;
const DW_LNS_FIXED_ADVANCE_PC: u8 = 9;
const DW_LNS_SET_ISA: u8 = 12;

// ---- Sonderopcodes `DW_LNE_*`: erstes Byte 0, dann Laenge (ULEB128),
// dann der Unteropcode (ULEB128; in DWARF 2 ein Byte — ULEB128 liest ein
// einzelnes Byte genauso). ----
const DW_LNE_END_SEQUENCE: u64 = 1;
const DW_LNE_SET_ADDRESS: u64 = 2;
const DW_LNE_DEFINE_FILE: u64 = 3;
const DW_LNE_SET_DISCRIMINATOR: u64 = 4;

// ---- Formate der DWARF-5-Eintragsbeschreibung (`DW_FORM_*`) ----
const DW_FORM_DATA2: u64 = 0x05;
const DW_FORM_DATA4: u64 = 0x06;
const DW_FORM_DATA8: u64 = 0x07;
const DW_FORM_STRING: u64 = 0x08;
const DW_FORM_BLOCK: u64 = 0x09;
const DW_FORM_BLOCK1: u64 = 0x0a;
const DW_FORM_DATA1: u64 = 0x0b;
const DW_FORM_FLAG: u64 = 0x0c;
const DW_FORM_SDATA: u64 = 0x0d;
const DW_FORM_STRP: u64 = 0x0e;
const DW_FORM_UDATA: u64 = 0x0f;
const DW_FORM_SEC_OFFSET: u64 = 0x17;
const DW_FORM_STRX: u64 = 0x1a;
const DW_FORM_DATA16: u64 = 0x1e;
const DW_FORM_LINE_STRP: u64 = 0x1f;
const DW_FORM_STRX1: u64 = 0x25;
const DW_FORM_STRX2: u64 = 0x26;
const DW_FORM_STRX3: u64 = 0x27;
const DW_FORM_STRX4: u64 = 0x28;
/// `DW_LNCT_path` — das gesuchte Feld einer Eintragsbeschreibung.
const DW_LNCT_PATH: u64 = 1;

/// DWARF-Abschnitt eines Moduls finden: zuerst die Sektion mit dem
/// Funktionsindex im Namen (LLVM), dann die modulweite (aeltere Werkzeuge).
fn abschnitt(m: &Modul, funktion: u32, kurz: &str) -> Option<Vec<u8>> {
    let je_funktion = format!("{funktion}.debug_{kurz}");
    for c in &m.custom {
        if c.name == je_funktion {
            return Some(c.inhalt.clone());
        }
    }
    let modulweit = format!(".debug_{kurz}");
    for c in &m.custom {
        if c.name == modulweit {
            return Some(c.inhalt.clone());
        }
    }
    None
}

/// Zeilentabelle einer Funktion lesen. Ohne DWARF-Sektion: leere Liste
/// (kein Fehler — die Entwicklerwerkzeuge zeigen dann nur den Versatz).
pub fn zeilen_der_funktion(m: &Modul, funktion: u32, g: &Grenzen) -> Vec<Zeile> {
    let daten = match abschnitt(m, funktion, "line") {
        Some(d) => d,
        None => return Vec::new(),
    };
    // Ein kaputter DWARF-Abschnitt darf die Engine nicht anhalten.
    lies_zeilen(&daten, g).unwrap_or_default()
}

/// `.debug_line` lesen: Kopf bis Version 5, dann das Programm.
///
/// `header_length` ist die verlaessliche Angabe, wo das Programm beginnt
/// (M5: nicht raten, sondern dem Kopf folgen). Nach dem Programmende
/// (`DW_LNE_end_sequence`) ist Schluss; ein fehlendes Ende liefert die bis
/// dahin gelesenen Zeilen statt eines Fehlers.
pub fn lies_zeilen(daten: &[u8], g: &Grenzen) -> Result<Vec<Zeile>, Fehler> {
    let mut l: Leser<'_> = Leser::neu(daten);
    // `unit_length` (4 Byte) zaehlt die Einheit OHNE dieses Feld; 0xffff_ffff
    // waere das 64-Bit-Format (nicht in .debug_line, DWARF 5 Abschnitt 6.2).
    let einheit = l.u32le("debug_line Einheit")?;
    if einheit < 2 || einheit as usize + 4 > l.alle().len() {
        return Err(dw(&format!(
            "unit_length {einheit} passt nicht zur Sektion"
        )));
    }
    let version = l.u16le("debug_line Version")?;
    // Kopf: Version 2–4 mit u32-Laenge; Version 5 mit address_size,
    // segment_selector_size und ULEB-Laenge. `adr_groesse` wird fuer
    // `DW_LNE_set_address` gebraucht.
    let (header_laenge, adr_groesse): (usize, usize) = match version {
        2..=4 => (l.u32le("debug_line header_length")? as usize, 4),
        5 => {
            let a = l.u8("debug_line address_size")? as usize;
            let _segment = l.u8("debug_line segment_selector_size")?;
            (l.leb_u64(true)? as usize, a.clamp(1, 8))
        }
        v => return Err(dw(&format!("Version {v} nicht unterstuetzt"))),
    };
    // Das Programm beginnt `header_laenge` Byte hinter dem Laengenfeld.
    let programm_start = l.pos().checked_add(header_laenge);
    if programm_start.is_none_or(|p| p > l.alle().len()) {
        return Err(dw("header_length laeuft ueber das Ende"));
    }
    let programm_start = programm_start.unwrap_or(l.alle().len());
    let min_laenge = l.u8("minimum_instruction_length")? as u64;
    if version >= 4 {
        let _max_ops = l.u8("maximum_operations_per_instruction")?;
    }
    let _default_is_stmt = l.u8("default_is_stmt")?;
    let zeilen_basis = l.u8("line_base")? as i8 as i64;
    let zeilen_weite = l.u8("line_range")? as u64;
    if zeilen_weite == 0 {
        return Err(dw("line_range 0"));
    }
    let opcode_basis = l.u8("opcode_base")?;
    if opcode_basis == 0 {
        return Err(dw("opcode_base 0"));
    }
    // Die Argumentlaengen der Standardopcodes werden gelesen (der Kopf muss
    // vollstaendig uebersprungen werden), aber nicht ausgewertet: die
    // Argumentzahl steht im `match` unten (DWARF 5, 6.2.5.1).
    let _ = l.nimm(
        (opcode_basis as usize).saturating_sub(1),
        "standard_opcode_lengths",
    )?;
    let mut dateien = lies_programm_header(&mut l, version, g)?;
    // Zum Programm springen. Nur vorwaerts: ein zu kleines `header_length`
    // darf uns nicht zurueck in den Kopf nehmen.
    let ziel = programm_start.max(l.pos());
    if ziel > l.alle().len() {
        return Err(dw("header_length laeuft ueber das Ende"));
    }
    l.gehe_zu(ziel);

    // ---- Programm: Standard-, Sonder- und Erweiterungsopcodes ----
    let mut zeilen: Vec<Zeile> = Vec::new();
    let mut adresse: u64 = 0;
    let mut zeile_nr: i64 = 0;
    let mut spalte: u32 = 0;
    // DWARF 2–4 zaehlen Dateien ab 1, DWARF 5 ab 0.
    let mut datei_index: u64 = u64::from(version < 5);
    let grenze = g.dwarf_zeilen;
    while zeilen.len() < grenze && !l.ist_ende() {
        let op = l.u8("Opcode")?;
        if op == 0 {
            // ---- Sonderopcode: Laenge, Unteropcode, Nutzlast
            let laenge = l.leb_u64(true)? as usize;
            if laenge == 0 {
                continue; // "extended op": zulaessig, tut nichts
            }
            if l.pos() + laenge > l.alle().len() {
                return Err(dw("Sonderopcode laeuft ueber das Ende"));
            }
            let mut unter: Leser<'_> = Leser::neu(l.nimm(laenge, "Sonderopcode")?);
            match unter.leb_u64(true)? {
                DW_LNE_END_SEQUENCE => {
                    schreibe(
                        &mut zeilen,
                        adresse,
                        zeile_nr,
                        spalte,
                        &dateien,
                        datei_index,
                    );
                    return Ok(zeilen);
                }
                DW_LNE_SET_ADDRESS => {
                    let mut v: u64 = 0;
                    for i in 0..adr_groesse {
                        v |= (unter.u8("DW_LNE_set_address")? as u64) << (8 * i);
                    }
                    adresse = v;
                }
                DW_LNE_DEFINE_FILE => {
                    // Datei zur Laufzeit hinzufuegen (Form DWARF 5)
                    let n = lies_eintrag_5(&mut unter);
                    if let Some(n) = n {
                        if dateien.len() < g.dwarf_records.max(1) {
                            dateien.push(n);
                        }
                    }
                }
                DW_LNE_SET_DISCRIMINATOR => {
                    let _ = unter.leb_u64(true);
                }
                // Unbekanntes: die Laenge hat die Nutzlast schon uebersprungen.
                _ => {}
            }
            continue;
        }
        if op < opcode_basis {
            match op {
                DW_LNS_COPY => {
                    schreibe(
                        &mut zeilen,
                        adresse,
                        zeile_nr,
                        spalte,
                        &dateien,
                        datei_index,
                    );
                }
                DW_LNS_ADVANCE_PC => {
                    let a = l.leb_u64(true)?;
                    adresse = adresse.saturating_add(a.saturating_mul(min_laenge));
                }
                DW_LNS_ADVANCE_LINE => {
                    zeile_nr = zeile_nr.saturating_add(l.leb_s32(true)? as i64);
                }
                DW_LNS_SET_FILE => {
                    datei_index = l.leb_u64(true)?;
                }
                DW_LNS_SET_COLUMN => {
                    spalte = l.leb_u64(true)? as u32;
                }
                DW_LNS_CONST_ADD_PC => {
                    // Wie ein Sonderopcode 255 (Spec 6.2.5.2), aber ohne Zeile.
                    let angepasst = 255u64.saturating_sub(opcode_basis as u64);
                    adresse = adresse
                        .saturating_add((angepasst / zeilen_weite).saturating_mul(min_laenge));
                    zeile_nr =
                        zeile_nr.saturating_add(zeilen_basis + (angepasst % zeilen_weite) as i64);
                }
                DW_LNS_FIXED_ADVANCE_PC => {
                    adresse = adresse.saturating_add(l.u16le("DW_LNS_fixed_advance_pc")? as u64);
                }
                DW_LNS_SET_ISA => {
                    let _ = l.leb_u64(true);
                }
                // `DW_LNS_negate_stmt` (6), `set_basic_block` (7),
                // `set_prologue_end` (10), `set_epilogue_begin` (11):
                // keine Argumente, keine Wirkung auf die Zuordnung.
                _ => {}
            }
            continue;
        }
        // ---- Erweiterter Opcode: op >= opcode_basis
        let angepasst = (op - opcode_basis) as u64;
        adresse = adresse.saturating_add((angepasst / zeilen_weite).saturating_mul(min_laenge));
        zeile_nr = zeile_nr.saturating_add(zeilen_basis + (angepasst % zeilen_weite) as i64);
        schreibe(
            &mut zeilen,
            adresse,
            zeile_nr,
            spalte,
            &dateien,
            datei_index,
        );
    }
    Ok(zeilen)
}

/// Eine Zuordnungszeile anhaengen. Der Dateiname kommt aus der
/// Dateiliste des Kopfes (Index je DWARF-Version, siehe `datei_index`).
fn schreibe(
    zeilen: &mut Vec<Zeile>,
    adresse: u64,
    zeile_nr: i64,
    spalte: u32,
    dateien: &[String],
    datei_index: u64,
) {
    let datei = dateien
        .get(usize::try_from(datei_index).unwrap_or(usize::MAX))
        .or_else(|| dateien.first())
        .cloned()
        .unwrap_or_default();
    zeilen.push(Zeile {
        adresse: adresse.min(u32::MAX as u64) as u32,
        zeile: zeile_nr.clamp(0, i64::from(u32::MAX)) as u32,
        spalte,
        datei,
    });
}

/// Name im DWARF-Format (bis zum 0-Byte). `None`, wenn kein 0-Byte folgt.
fn lies_name(l: &mut Leser<'_>) -> Option<String> {
    let mut v: Vec<u8> = Vec::new();
    while let Ok(b) = l.u8("Name") {
        if b == 0 {
            return Some(String::from_utf8_lossy(&v).to_string());
        }
        v.push(b);
        // Harte Grenze: ein "Name" ohne 0-Byte ist ein Angriff (M3/M4).
        if v.len() >= crate::grenzen::MAX_NAME_BYTES_HART {
            return None;
        }
    }
    None
}

/// Programmkopf bis zu den Dateinamen (fuer die Anzeige „Zeile x aus
/// datei.c"). Version 5 hat zwei Eintagslisten (Verzeichnisse, Dateinamen)
/// mit je eigener Formatbeschreibung; Version 2–4 hat flache Listen.
///
/// Beide Zweige lesen dieselbe Bytekette, bis der Aufrufer danach per
/// `header_length` zum Programm springt — ein Fehler in einem Namen
/// (kein 0-Byte) wird als benannter Fehler gemeldet, nicht geraten.
fn lies_programm_header(
    l: &mut Leser<'_>,
    version: u16,
    g: &Grenzen,
) -> Result<Vec<String>, Fehler> {
    // Harte Obergrenze fuer beide Zweige (M3): ein Kopf kann hoechstens so
    // viele Eintraege haben, wie die Sektion Bytes hat; `dwarf_records`
    // ist zusaetzlich der Deckel aus den Grenzen.
    let grenze = g
        .dwarf_records
        .clamp(1, crate::grenzen::MAX_DWARF_RECORDS_HART);
    let mut dateien: Vec<String> = Vec::new();
    if version >= 5 {
        // Verzeichnisse ueberspringen (sie stehen vor den Dateinamen).
        let _dirs = lies_eintrags_liste(l, grenze)?;
        dateien = lies_eintrags_liste(l, grenze)?;
    } else {
        // include_directories: Namen, ein leerer Name beendet die Liste.
        loop {
            match lies_name(l) {
                Some(s) if s.is_empty() => break,
                Some(s) => {
                    if dateien.len() >= grenze {
                        return Err(dw("include_directories ueber der Grenze"));
                    }
                    dateien.push(s);
                }
                None => return Err(dw("include_directories ohne Abschluss")),
            }
        }
        // file_names: Name, Verzeichnisindex, mtime, Laenge; ein leerer
        // Name beendet die Liste. Index 0 bleibt frei (DWARF 2–4 zaehlen
        // Dateien ab 1).
        dateien.push(String::new());
        loop {
            let name = match lies_name(l) {
                Some(s) => s,
                None => return Err(dw("file_names ohne Abschluss")),
            };
            if name.is_empty() {
                break;
            }
            let _dir = l.leb_u64(true)?;
            let _mtime = l.leb_u64(true)?;
            let _laenge = l.leb_u64(true)?;
            if dateien.len() >= grenze {
                return Err(dw("file_names ueber der Grenze"));
            }
            dateien.push(name);
        }
    }
    Ok(dateien)
}

/// Eine DWARF-5-Eintragsliste: Formatbeschreibung, dann die Eintraege.
/// Rueckgabe: die `DW_LNCT_path`-Werte in Reihenfolge (leer, wenn das
/// Feld nicht enthalten ist).
fn lies_eintrags_liste(l: &mut Leser<'_>, grenze: usize) -> Result<Vec<String>, Fehler> {
    let n = l.u8("entry_format_count")? as usize;
    if n > grenze {
        return Err(dw("entry_format_count ueber der Grenze"));
    }
    let mut format: Vec<(u64, u64)> = Vec::new();
    for _ in 0..n {
        let t = l.leb_u64(true)?;
        let f = l.leb_u64(true)?;
        format.push((t, f));
    }
    let anzahl = l.leb_u64(true)? as usize;
    // M4: vor der Allokation deckeln.
    if anzahl > grenze {
        return Err(dw("Eintragszahl ueber der Grenze"));
    }
    let mut r: Vec<String> = Vec::with_capacity(anzahl.min(1024));
    for _ in 0..anzahl {
        let mut pfad = String::new();
        for (t, f) in &format {
            let wert = lies_eintrags_wert(l, *f)?;
            if *t == DW_LNCT_PATH {
                pfad = wert;
            }
        }
        r.push(pfad);
    }
    Ok(r)
}

/// Ein Eintrag fuer `DW_LNE_define_file` (Formbeschreibung im Befehl).
fn lies_eintrag_5(l: &mut Leser<'_>) -> Option<String> {
    let n = l.u8("entry_format_count").ok()? as usize;
    if n > 64 {
        return None;
    }
    let mut format: Vec<(u64, u64)> = Vec::new();
    for _ in 0..n {
        format.push((l.leb_u64(true).ok()?, l.leb_u64(true).ok()?));
    }
    let mut pfad = String::new();
    for (t, f) in &format {
        let wert = lies_eintrags_wert(l, *f).ok()?;
        if *t == DW_LNCT_PATH {
            pfad = wert;
        }
    }
    Some(pfad)
}

/// Einen Wert in der angegebenen `DW_FORM_*`-Form lesen. Textformen
/// liefern den Text, alle anderen nur ihren Platz in der Kette (der
/// Versatz in `.debug_str` ist ohne diese Sektion nicht aufloesbar, der
/// Sprung muss aber exakt sein, sonst verliert der Leser die Fassung).
fn lies_eintrags_wert(l: &mut Leser<'_>, form: u64) -> Result<String, Fehler> {
    match form {
        DW_FORM_STRING => Ok(lies_name(l).ok_or_else(|| dw("DW_FORM_string ohne Abschluss"))?),
        DW_FORM_LINE_STRP | DW_FORM_STRP | DW_FORM_SEC_OFFSET => {
            let _ = l.u32le("DW_FORM_*_strp")?;
            Ok(String::new())
        }
        DW_FORM_DATA1 | DW_FORM_FLAG => {
            let _ = l.u8("DW_FORM_data1")?;
            Ok(String::new())
        }
        DW_FORM_DATA2 => {
            let _ = l.u16le("DW_FORM_data2")?;
            Ok(String::new())
        }
        DW_FORM_DATA4 | DW_FORM_STRX4 => {
            let _ = l.u32le("DW_FORM_data4")?;
            Ok(String::new())
        }
        DW_FORM_DATA8 => {
            let _ = l.nimm(8, "DW_FORM_data8")?;
            Ok(String::new())
        }
        DW_FORM_DATA16 => {
            let _ = l.nimm(16, "DW_FORM_data16")?;
            Ok(String::new())
        }
        DW_FORM_STRX2 => {
            let _ = l.u16le("DW_FORM_strx2")?;
            Ok(String::new())
        }
        DW_FORM_STRX1 => {
            let _ = l.u8("DW_FORM_strx1")?;
            Ok(String::new())
        }
        DW_FORM_STRX3 => {
            let _ = l.nimm(3, "DW_FORM_strx3")?;
            Ok(String::new())
        }
        DW_FORM_UDATA | DW_FORM_STRX => {
            let _ = l.leb_u64(true)?;
            Ok(String::new())
        }
        DW_FORM_SDATA => {
            let _ = l.leb_s32(true)?;
            Ok(String::new())
        }
        DW_FORM_BLOCK => {
            let n = l.leb_u64(true)? as usize;
            let _ = l.nimm(n.min(l.uebrig()), "DW_FORM_block")?;
            Ok(String::new())
        }
        DW_FORM_BLOCK1 => {
            let n = l.u8("DW_FORM_block1")? as usize;
            let _ = l.nimm(n.min(l.uebrig()), "DW_FORM_block1")?;
            Ok(String::new())
        }
        f => Err(dw(&format!("DWARF: Form {f:#x} nicht unterstuetzt"))),
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// Kopfteil eines `.debug_line`-Abschnitts (ab `minimum_instruction_length`).
    fn kopf(version: u16) -> Vec<u8> {
        let mut h: Vec<u8> = Vec::new();
        h.push(1); // minimum_instruction_length
        if version >= 4 {
            h.push(1); // maximum_operations_per_instruction
        }
        h.push(1); // default_is_stmt
        h.push((-5i8) as u8); // line_base
        h.push(14); // line_range
        h.push(13); // opcode_base
        h.extend_from_slice(&[0u8; 12]); // 12 Standardopcodes ohne Argumente
        h
    }

    /// M7: Zeilen aus einem handgebauten `.debug_line` lesen (Version 4).
    #[test]
    fn version4_zeilen() {
        let mut h: Vec<u8> = kopf(4);
        h.push(0); // include_directories: kein Verzeichnis
        h.extend_from_slice(b"a.c\0"); // Dateiname
        h.push(0); // Verzeichnisindex
        h.push(0); // mtime
        h.push(0); // Länge
        h.push(0); // leerer Name beendet die Liste
        // Programm: eine Zeile 5 bei Adresse 0, dann end_sequence.
        let mut p: Vec<u8> = Vec::new();
        p.push(3); // DW_LNS_advance_line
        p.push(5); // SLEB128 +5
        p.push(1); // DW_LNS_copy -> Zeile
        p.push(0); // Sonderopcode
        p.push(1); // Laenge
        p.push(1); // DW_LNE_end_sequence
        let d: Vec<u8> = stuecke(&h, &p);
        let z = lies_zeilen(&d, &Grenzen::standard()).unwrap();
        assert_eq!(z.len(), 2, "Zeilen: {z:?}");
        assert_eq!(z[0].adresse, 0);
        assert_eq!(z[0].zeile, 5);
        assert_eq!(z[0].datei, "a.c");
        assert_eq!(z[1].adresse, 0); // end_sequence: letzte Adresse
        assert_eq!(z[1].zeile, 5);
    }

    /// M7: Version 5 liest address_size, Eintragslisten und Sonderopcodes.
    #[test]
    fn version5_zeilen() {
        let mut h: Vec<u8> = kopf(5);
        // directory_entry_format_count = 1, (DW_LNCT_path, DW_FORM_line_strp)
        h.push(1);
        h.push(DW_LNCT_PATH as u8);
        h.push(DW_FORM_LINE_STRP as u8);
        h.push(0); // directories_count = 0
        // file_name_entry_format_count = 1, (DW_LNCT_path, DW_FORM_string)
        h.push(1);
        h.push(DW_LNCT_PATH as u8);
        h.push(DW_FORM_STRING as u8);
        h.push(1); // file_names_count = 1
        h.extend_from_slice(b"b.c\0");
        let mut p: Vec<u8> = Vec::new();
        p.push(0); // DW_LNE_set_address
        p.push(9); // Laenge: 1 Unteropcode + 8 Adressbyte
        p.push(DW_LNE_SET_ADDRESS as u8);
        p.extend_from_slice(&0x100u64.to_le_bytes());
        p.push(4); // DW_LNS_set_file
        p.push(0); // Datei 0 (DWARF 5 zaehlt ab 0)
        p.push(3); // DW_LNS_advance_line
        p.push(7); // SLEB128 +7
        p.push(1); // DW_LNS_copy -> Zeile
        p.push(0);
        p.push(1);
        p.push(1); // DW_LNE_end_sequence
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(&[0u8; 4]);
        d.extend_from_slice(&5u16.to_le_bytes());
        d.push(8); // address_size
        d.push(0); // segment_selector_size
        // header_length als ULEB128
        assert!(h.len() < 128);
        d.push(h.len() as u8);
        d.extend_from_slice(&h);
        d.extend_from_slice(&p);
        let unit = u32::try_from(d.len() - 4).expect("kurzer Abschnitt");
        d[0..4].copy_from_slice(&unit.to_le_bytes());
        let z = lies_zeilen(&d, &Grenzen::standard()).unwrap();
        assert_eq!(z.len(), 2, "Zeilen: {z:?}");
        assert_eq!(z[0].adresse, 0x100);
        assert_eq!(z[0].zeile, 7);
        assert_eq!(z[0].datei, "b.c");
    }

    /// Ein ganzer Abschnitt aus Kopf und Programm (Version 2–4, bei denen
    /// `header_length` ein u32 direkt nach der Version steht). Die
    /// Laengenfelder werden richtig gefuellt — so, wie ein Compiler sie
    /// schreibt.
    fn stuecke(kopf: &[u8], programm: &[u8]) -> Vec<u8> {
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(&[0u8; 4]); // unit_length, unten gefuellt
        d.extend_from_slice(&4u16.to_le_bytes());
        d.extend_from_slice(
            &u32::try_from(kopf.len())
                .expect("kleiner Kopf")
                .to_le_bytes(),
        );
        d.extend_from_slice(kopf);
        d.extend_from_slice(programm);
        let einheit = u32::try_from(d.len() - 4).expect("kleiner Abschnitt");
        d[0..4].copy_from_slice(&einheit.to_le_bytes());
        d
    }

    /// M8: Bösartige Eingaben liefern BENANNTE Fehler, keine Panik.
    #[test]
    fn boesartige_abschnitte() {
        // header_length laeuft ueber das Ende
        let h: Vec<u8> = kopf(4);
        let mut d: Vec<u8> = stuecke(&h, &[]);
        d[6..10].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        let e = lies_zeilen(&d, &Grenzen::standard()).unwrap_err();
        assert!(e.kurz().contains("header_length"), "{e:?}");

        // Version 99 ist nicht unterstuetzt
        let mut d2: Vec<u8> = stuecke(&h, &[]);
        d2[4..6].copy_from_slice(&99u16.to_le_bytes());
        let e2 = lies_zeilen(&d2, &Grenzen::standard()).unwrap_err();
        assert!(e2.kurz().contains("Version 99"), "{e2:?}");

        // Ein Sonderopcode, dessen Laenge ueber das Ende laeuft
        let mut h3: Vec<u8> = kopf(4);
        h3.push(0); // include_directories
        h3.push(0); // file_names: leerer Name
        let d3: Vec<u8> = stuecke(&h3, &[0, 200, 1]);
        let e3 = lies_zeilen(&d3, &Grenzen::standard()).unwrap_err();
        assert!(e3.kurz().contains("Sonderopcode"), "{e3:?}");

        // Dateiliste ohne Abschluss (kein leerer Name)
        let mut h4: Vec<u8> = kopf(4);
        h4.extend_from_slice(b"verzeichnis\0x"); // kein leerer Name
        let d4: Vec<u8> = stuecke(&h4, &[]);
        let e4 = lies_zeilen(&d4, &Grenzen::standard()).unwrap_err();
        assert!(e4.kurz().contains("include_directories"), "{e4:?}");

        // unit_length passt nicht zur Sektion
        let mut d5: Vec<u8> = stuecke(&h, &[]);
        d5[0..4].copy_from_slice(&1u32.to_le_bytes());
        let e5 = lies_zeilen(&d5, &Grenzen::standard()).unwrap_err();
        assert!(e5.kurz().contains("unit_length"), "{e5:?}");

        // line_range 0: Sonderopcodes waeren undefiniert
        let mut h6: Vec<u8> = kopf(4);
        h6[4] = 0; // line_range
        let d6: Vec<u8> = stuecke(&h6, &[]);
        let e6 = lies_zeilen(&d6, &Grenzen::standard()).unwrap_err();
        assert!(e6.kurz().contains("line_range"), "{e6:?}");

        // opcode_base 0: keine Standardopcodes
        let mut h7: Vec<u8> = kopf(4);
        h7[5] = 0; // opcode_base
        let d7: Vec<u8> = stuecke(&h7, &[]);
        let e7 = lies_zeilen(&d7, &Grenzen::standard()).unwrap_err();
        assert!(e7.kurz().contains("opcode_base"), "{e7:?}");
    }

    /// M7: Die Zeilengrenze greift (kein Endlosschleifen-Programm).
    #[test]
    fn zeilengrenze_greift() {
        let mut h: Vec<u8> = kopf(4);
        h.push(0); // include_directories
        h.push(0); // file_names
        // 100 Zeilen aus Sonderopcodes (opcode_base + 1 = 14)
        let mut p: Vec<u8> = Vec::new();
        for _ in 0..100 {
            p.push(14);
        }
        let d: Vec<u8> = stuecke(&h, &p);
        let mut g = Grenzen::eng();
        g.dwarf_zeilen = 10;
        let z = lies_zeilen(&d, &g).unwrap();
        assert_eq!(z.len(), 10, "Grenze nicht wirksam");
    }

    #[test]
    fn ohne_abschnitt_leer() {
        let m = crate::wat::lies_text("(module (func))", &Grenzen::standard()).unwrap();
        assert!(zeilen_der_funktion(&m, 0, &Grenzen::standard()).is_empty());
    }

    /// M7: Die Zuordnung kommt aus der benutzerdefinierten Sektion der
    /// Funktion (`"0.debug_line"`).
    #[test]
    fn zeilen_aus_custom_sektion() {
        let mut h: Vec<u8> = kopf(4);
        h.push(0);
        h.extend_from_slice(b"q.c\0");
        h.push(0);
        h.push(0);
        h.push(0);
        h.push(0);
        let d: Vec<u8> = stuecke(&h, &[3, 9, 1, 0, 1, 1]);
        let m = crate::wat::lies_text(
            "(module (func) (@custom \"0.debug_line\" \"\"))",
            &Grenzen::standard(),
        )
        .unwrap();
        // Ohne Inhalt ist die Liste leer; mit Inhalt kommt sie aus der Sektion.
        assert!(zeilen_der_funktion(&m, 0, &Grenzen::standard()).is_empty());
        let mut m2 = m.clone();
        m2.custom = vec![crate::modell::Custom {
            name: "0.debug_line".to_string(),
            inhalt: d,
            abschnitt: 0,
        }];
        let z = zeilen_der_funktion(&m2, 0, &Grenzen::standard());
        assert_eq!(z.len(), 2, "Zeilen: {z:?}");
        assert_eq!(z[0].zeile, 9);
        assert_eq!(z[0].datei, "q.c");
    }
}
