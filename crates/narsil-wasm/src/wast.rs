//! W3: `.wast`-Skriptleser (die Spec-Testsuite besteht aus Skripten).
//!
//! Unterstuetzt werden alle Befehle, die die Testsuite fuer WebAssembly
//! 2.0 Core benutzt:
//! `(module …)` / `(module binary "…")` / `(module quote "…")`,
//! `(register "name")`, `(invoke …)`, `(assert_return …)`,
//! `(assert_trap …)`, `(assert_exhaustion …)`, `(assert_malformed …)`,
//! `(assert_invalid …)`, `(assert_unlinkable …)`,
//! `(assert_return_canonical_nan …)`, `(assert_return_arithmetic_nan …)`,
//! `(get …)`, `(assert_exhaustion …)`.
//!
//! Der Leser liefert eine flache Liste von `Befehl_der_Skripts`; die
//! Ausfuehrung macht `testsuite.rs` (Test) bzw. `examples/wast_lauf.rs`.
//!
//! Wichtig fuer den Speicher (M4/M8): Laengen von Zeichenketten und die
//! Zahl der Befehle sind begrenzt, jede Modulgrenze wird VOR der
//! Allokation geprueft.

use crate::fehler::*;
use crate::grenzen::Grenzen;
use crate::modell::Valtyp;
use crate::wert::Wert;

/// Ein erwarteter Wert (`assert_return`).
#[derive(Debug, Clone, PartialEq)]
pub enum Erwartung {
    /// Exakter Wert (f32/f64 als Bitmuster, damit NaN vergleichbar ist).
    Genau(Wert),
    /// `nan:canonical` — irgendeine kanonische NaN.
    NanKanonisch(bool),
    /// `nan:arithmetic` — irgendeine NaN mit gesetztem Ruhebit.
    NanArithmetisch(bool),
    /// `either` (Float-Befehle der Spec).
    Entweder(Box<Erwartung>, Box<Erwartung>),
    /// `(ref.i31 7)`, `(ref.struct)`, `(ref.array …)`, `(ref.eq)` als
    /// Erwartung: die Art des GC-Objekts und (bei i31/array) sein Inhalt.
    ///
    /// GEMESSEN 03.10. (`extern.wast` Z. 53-55, `i31.wast`,
    /// `struct.wast`, `array.wast`): ohne diese Variante war das Skript
    /// nicht lesbar und die Datei nicht einmal im Nenner.
    Gc(GcArt, GcInhalt),
    /// `(ref.extern)` / `(ref.host)` OHNE Zahl: irgendein nicht-nuller
    /// `externref`.
    ///
    /// GEMESSEN 03.10. (`extern.wast` Z. 47: `(ref.extern)` ist KEIN
    /// Null-Wert — mit `Wert::Extern(None)` verglichen war der Fall rot).
    IrgendeineExtern,
    /// `(ref.func)` OHNE Index als Erwartung: irgendeine nicht-leere
    /// Funktionsreferenz (GEMESSEN 02.10., `select.wast`
    /// `join-funcnull` — erwartet wurde „irgendeine Funktion", geliefert
    /// `null`).
    IrgendeineFunktion,
    /// `(ref.null)` OHNE Heaptype: irgendeine null-Referenz
    /// (`funcref`, `externref`, GC-Referenz …).
    ///
    /// GEMESSEN 03.10. (`global.wast` Z. 659, `ref_null.wast`:
    /// `(assert_return (invoke "ref") (ref.null))`). Davor hat der Leser
    /// daraus `Func(None)` gemacht — jeder Vergleich schlug fehl.
    IrgendeineNull,
}

/// Die Art eines erwarteten GC-Objekts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcArt {
    /// `(ref.i31 …)`
    I31,
    /// `(ref.struct …)` — optional mit Feldwerten `(ref.struct 1 2)`.
    Struct,
    /// `(ref.array …)` — optional mit Elementen `(ref.array 1 2)`.
    Array,
    /// `(ref.eq …)` — `eqref`.
    Eq,
}

/// Der erwartete Inhalt: `null`, oder die Liste der Feld-/Elementwerte.
#[derive(Debug, Clone, PartialEq)]
pub enum GcInhalt {
    /// `(ref.i31)` / `(ref.struct)` / `(ref.array)` ohne weitere Angabe:
    /// nur die Art wird geprueft.
    Irrelevant,
    /// `null`.
    Null,
    /// `(ref.i31 7)`: genau dieser i31-Wert.
    I31(i32),
    /// `(ref.struct 1 2)` / `(ref.array 1 2)`: genau diese Werte.
    Werte(Vec<Wert>),
}

/// Was ein Skriptbefehl tut.
#[derive(Debug, Clone)]
pub enum Skript {
    /// Ein Modul (Text oder Binaer). `nummer` ist die laufende Modulnummer.
    Modul {
        nummer: u32,
        /// Byte-Inhalt (Binaerform) oder Text (WAT).
        inhalt: ModulInhalt,
        /// `(module definition …)`: nur DEFINIERT, nicht instanziieren.
        ///
        /// GEMESSEN 02.10. (`table.wast` Z. 6: `(module definition (table
        /// 0xffff_ffff funcref))` wurde instanziert und meldete eine
        /// Groessenbeschraenkung, die erst beim Linken greift).
        definition: bool,
    },
    /// `(module instance $I $M)`: erzeugt eine Instanz des previously
    /// definierten Moduls `$M` und legt sie unter dem Modulnummer-Raum ab
    /// (Shared-Everything).
    ///
    /// GEMESSEN 02.10. (`instance.wast`: 12 Faelle — zwei Instanzen
    /// desselben Moduls verhalten sich unabhaengig, dieselbe Instanz
    /// zweimal importiert ist EIN Zustand).
    Instanz {
        nummer: u32,
        /// Nummer des Moduls, dessen Definition instanziert wird
        /// (`$M`).
        quelle: u32,
    },
    /// `(register "name" $modul?)`
    Register { name: String, nummer: Option<u32> },
    /// `(invoke "name" arg…)` auf das aktuelle Modul. Mit
    /// `(invoke $modul "name" …)` laeuft der Aufruf auf einem benannten
    /// Modul (GEMESSEN 02.10., `elem.wast`: 21 Kommandos).
    Aufrufen {
        modul: Option<u32>,
        name: String,
        argumente: Vec<Wert>,
    },
    /// `(get "name")` — liest einen globalen/exportierten Wert;
    /// `(get $modul "name")` auf einem benannten Modul.
    Lesen { modul: Option<u32>, name: String },
    /// `(assert_return (invoke …) erwartung…)`
    Rueckgabe {
        aufruf: Box<Skript>,
        erwartet: Vec<Erwartung>,
    },
    /// `(assert_trap (invoke …) "text")` — Erwartung ist ein Trap.
    Trap { aufruf: Box<Skript>, text: String },
    /// `(assert_exhaustion (invoke …) "text")` — Brennstoff/Stack.
    Erschopfung { aufruf: Box<Skript>, text: String },
    /// `(assert_malformed (module …) "text")`
    Fehlerhaft { modul: ModulInhalt, text: String },
    /// `(assert_invalid (module …) "text")`
    Ungueltig { modul: ModulInhalt, text: String },
    /// `(assert_unlinkable (module …) "text")`
    NichtLinkbar { modul: ModulInhalt, text: String },
    /// `assert_return_canonical_nan` / `arithmetic_nan`
    NanPruefung {
        aufruf: Box<Skript>,
        arithmetisch: bool,
    },
}

/// Wie ein Modul im Skript steht.
#[derive(Debug, Clone)]
pub enum ModulInhalt {
    /// WAT-Text (bereits zusammengesetzt).
    Text(String),
    /// Binaerbytes.
    Bytes(Vec<u8>),
}

/// Warum ein Skript nicht lesbar war.
fn skriptfehler(t: &str) -> Fehler {
    Fehler::form(ART_UNVOLLSTAENDIG, format!("wast: {t}"))
}

/// Ein Token des Skripts.
#[derive(Debug, Clone)]
enum T {
    Auf(String),
    Zu,
    Wort(String),
    Text(Vec<u8>),
    Name(String),
}

impl T {
    /// Token als Text — fuer den WAT-Leser. Anfuehrungszeichen und
    /// Escapes MUESSEN wiederhergestellt werden, sonst verliert der
    /// zusammengesetzte Modultext seine Strings (`(export "f")`).
    fn text(&self) -> String {
        match self {
            T::Auf(s) => format!("({s}"),
            T::Zu => ")".to_string(),
            T::Wort(w) => w.clone(),
            T::Name(n) => {
                // Ein Name mit Trennzeichen muss als `$"…"` zurueck in den
                // Text, sonst zerlegt der WAT-Leser ihn in mehrere Token.
                if n.chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\\' || c == '(' || c == ')')
                {
                    let mut s = String::from("$\"");
                    for c in n.chars() {
                        match c {
                            '"' => s.push_str("\\\""),
                            '\\' => s.push_str("\\\\"),
                            '\n' => s.push_str("\\n"),
                            '\t' => s.push_str("\\t"),
                            '\r' => s.push_str("\\r"),
                            _ => s.push(c),
                        }
                    }
                    s.push('"');
                    s
                } else {
                    format!("${n}")
                }
            }
            T::Text(b) => {
                let mut s = String::from("\"");
                for &x in b {
                    match x {
                        b'"' => s.push_str("\\\""),
                        b'\\' => s.push_str("\\\\"),
                        0x20..=0x7e => s.push(x as char),
                        _ => s.push_str(&format!("\\{x:02x}")),
                    }
                }
                s.push('"');
                s
            }
        }
    }
}

/// Skript in Tokens zerlegen (Klammern, Strings mit Escapes, Kommentare).
fn tokenisier(text: &str, g: &Grenzen) -> Result<Vec<T>, Fehler> {
    let mut out: Vec<T> = Vec::new();
    let b = text.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if out.len() >= g.wast_befehle.saturating_mul(20) {
            return Err(skriptfehler("zu viele Tokens"));
        }
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b';' if i + 1 < b.len() && b[i + 1] == b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if i + 1 < b.len() && b[i + 1] == b';' => {
                let mut tiefe = 1usize;
                i += 2;
                while i < b.len() && tiefe > 0 {
                    if i + 1 < b.len() && b[i] == b'(' && b[i + 1] == b';' {
                        tiefe += 1;
                        i += 2;
                    } else if i + 1 < b.len() && b[i] == b';' && b[i + 1] == b')' {
                        tiefe -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if tiefe > 0 {
                    return Err(skriptfehler("Blockkommentar nicht geschlossen"));
                }
            }
            b'(' => {
                i += 1;
                let mut name = String::new();
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'.' | b'_' | b'-' | b'!'))
                {
                    name.push(b[i] as char);
                    i += 1;
                }
                out.push(T::Auf(name));
            }
            b')' => {
                i += 1;
                out.push(T::Zu);
            }
            b'"' => {
                let (bytes, neu) = lese_string(b, i)?;
                out.push(T::Text(bytes));
                i = neu;
            }
            b'$' => {
                i += 1;
                let mut name = String::new();
                // Bezeichner duerfen alle druckbaren Zeichen ausser den
                // Trennzeichen enthalten (`id.wast`: `$!?@#a$%^&*b-+_.:9`)
                // … GEMESSEN 02.10.: mit der engen Zeichenliste wurde
                // `$!?@#a…` als Befehl gelesen.
                // `$"…"`: Bezeichner als maskierte Zeichenkette (`id.wast`).
                if i < b.len() && b[i] == b'"' {
                    name.push_str(&maskierter_name(b, &mut i)?);
                }
                while i < b.len()
                    && !matches!(
                        b[i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"' | b';'
                    )
                {
                    name.push(b[i] as char);
                    i += 1;
                }
                // `$` ohne Namen ist ein Syntaxfehler („empty identifier",
                // GEMESSEN 02.10., `id.wast`).
                if name.is_empty() {
                    return Err(skriptfehler("leerer Bezeichner: `$` ohne Namen"));
                }
                out.push(T::Name(name));
            }
            _ => {
                let start = i;
                while i < b.len()
                    && !matches!(
                        b[i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"' | b';'
                    )
                {
                    i += 1;
                }
                if i == start {
                    return Err(skriptfehler(&format!("Zeichen {:?}", b[i] as char)));
                }
                out.push(T::Wort(text[start..i].to_string()));
            }
        }
    }
    Ok(out)
}

/// String ab `b[start] == '"'` lesen; Rueckgabe (Bytes, neue Position).
fn lese_string(b: &[u8], start: usize) -> Result<(Vec<u8>, usize), Fehler> {
    let mut i = start + 1;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        if i >= b.len() {
            return Err(skriptfehler("Zeichenkette nicht geschlossen"));
        }
        match b[i] {
            b'"' => return Ok((bytes, i + 1)),
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    return Err(skriptfehler("Escaping am Ende"));
                }
                match b[i] {
                    b'n' => {
                        bytes.push(b'\n');
                        i += 1;
                    }
                    b't' => {
                        bytes.push(b'\t');
                        i += 1;
                    }
                    b'r' => {
                        bytes.push(b'\r');
                        i += 1;
                    }
                    b'\\' => {
                        bytes.push(b'\\');
                        i += 1;
                    }
                    b'"' => {
                        bytes.push(b'"');
                        i += 1;
                    }
                    b'\'' => {
                        bytes.push(b'\'');
                        i += 1;
                    }
                    b'u' => {
                        if i + 1 < b.len() && b[i + 1] == b'{' {
                            let mut j = i + 2;
                            let mut v: u32 = 0;
                            while j < b.len() && b[j] != b'}' {
                                v = v * 16
                                    + (b[j] as char)
                                        .to_digit(16)
                                        .ok_or_else(|| skriptfehler("\\u"))?;
                                j += 1;
                            }
                            if let Some(ch) = char::from_u32(v) {
                                let mut tmp = [0u8; 4];
                                bytes.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                            }
                            i = j + 1;
                        } else {
                            return Err(skriptfehler("\\u ohne {"));
                        }
                    }
                    _ => {
                        if i + 2 >= b.len() {
                            return Err(skriptfehler("Escaping zu kurz"));
                        }
                        let h = std::str::from_utf8(&b[i..i + 2])
                            .ok()
                            .and_then(|s| u8::from_str_radix(s, 16).ok())
                            .ok_or_else(|| skriptfehler("Escaping: keine Hexadezimalzahl"))?;
                        bytes.push(h);
                        i += 2;
                    }
                }
            }
            x => {
                bytes.push(x);
                i += 1;
            }
        }
    }
}

/// Skript aus Tokens lesen.
pub fn lies_skript(text: &str, g: &Grenzen) -> Result<Vec<Skript>, Fehler> {
    let tok = tokenisier(text, g)?;
    // Klammern muessen paarig sein: `assert_malformed` in der Testsuite
    // erwartet bei abgeschnittenem Text genau diese Art Fehler.
    let mut tiefe: i64 = 0;
    for t in &tok {
        match t {
            T::Auf(_) => tiefe += 1,
            T::Zu => tiefe -= 1,
            _ => {}
        }
        if tiefe < 0 {
            return Err(skriptfehler("zu viele schliessende Klammern"));
        }
    }
    if tiefe > 0 {
        return Err(skriptfehler(&format!(
            "{tiefe} Klammer(n) nicht geschlossen"
        )));
    }
    let mut p = SkriptLeser {
        tok,
        pos: 0,
        g: g.clone(),
        module: 0,
        zeilen: Vec::new(),
        bezeichner_nummer: std::collections::BTreeMap::new(),
    };
    p.alle()
}

struct SkriptLeser {
    tok: Vec<T>,
    pos: usize,
    g: Grenzen,
    /// Laufende Modulnummer (fuer `register`).
    module: u32,
    zeilen: Vec<Skript>,
    /// `$name` der Modulbefehle -> Nummer (`(module $m …) (register "x" $m)`).
    bezeichner_nummer: std::collections::BTreeMap<String, u32>,
}

impl SkriptLeser {
    fn sieht(&self) -> Option<&T> {
        self.tok.get(self.pos)
    }
    fn weiter(&mut self) -> T {
        match self.tok.get(self.pos) {
            Some(t) => {
                let t = t.clone();
                self.pos += 1;
                t
            }
            None => T::Zu,
        }
    }
    /// Steht als nacktes Wort da? (`(module binary "…")`)
    fn sieht_wort_ist(&self, w: &str) -> bool {
        matches!(self.sieht(), Some(T::Wort(x)) if x == w)
    }
    fn zu(&mut self) -> Result<(), Fehler> {
        match self.weiter() {
            T::Zu => Ok(()),
            t => Err(skriptfehler(&format!("erwartet ), gefunden {}", t.text()))),
        }
    }
    fn name(&mut self) -> Result<String, Fehler> {
        match self.weiter() {
            T::Text(b) => String::from_utf8(b)
                .map_err(|_| Fehler::form(ART_UTF8, "wast: Name ist kein UTF-8")),
            T::Name(n) => Ok(n),
            T::Wort(w) => Ok(w),
            t => Err(skriptfehler(&format!(
                "Name erwartet, gefunden {}",
                t.text()
            ))),
        }
    }

    /// Alle Top-Level-Befehle lesen.
    fn alle(&mut self) -> Result<Vec<Skript>, Fehler> {
        while let Some(t) = self.sieht().cloned() {
            match t {
                T::Zu => break,
                T::Auf(k) => {
                    let s = self.befehl(&k)?;
                    self.zeilen.push(s);
                    if self.zeilen.len() > self.g.wast_befehle {
                        return Err(skriptfehler("mehr Befehle als die Grenze erlaubt"));
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        Ok(std::mem::take(&mut self.zeilen))
    }

    /// Einen Top-Level-Befehl lesen (`self.sieht()` ist `(name`).
    fn befehl(&mut self, k: &str) -> Result<Skript, Fehler> {
        self.pos += 1;
        match k {
            "module" => {
                // `(module $name …)` und `(module definition $name …)`:
                // die Skriptform der Testsuite fuer benannte Definitionen
                // (GEMESSEN 01.10., `instance.wast`, `table.wast`: ohne das
                // Schlusswort blieb `definition` als Modulinhalt liegen).
                let definition = self.sieht_wort_ist("definition");
                if definition {
                    self.pos += 1;
                }
                // `(module instance $I $M)`: instanziert die Definition
                // `$M` neu. GEMESSEN 02.10. (`instance.wast`: 12 Faelle).
                if self.sieht_wort_ist("instance") {
                    self.pos += 1;
                    let nummer = self.module;
                    self.module += 1;
                    // `$I` (Bezeichnung der neuen Instanz) — optional.
                    if let Some(T::Name(n)) = self.sieht().cloned() {
                        self.pos += 1;
                        self.bezeichner_nummer.insert(n, nummer);
                    }
                    // `$M`: die Definition, die instanziert wird.
                    let quelle_name = self.name()?;
                    let quelle = self
                        .bezeichner_nummer
                        .get(&quelle_name)
                        .copied()
                        .ok_or_else(|| {
                            Fehler::form(
                                crate::fehler::ART_UNVOLLSTAENDIG,
                                format!(
                                    "(module instance): ${quelle_name} ist keine Moduldefinition"
                                ),
                            )
                        })?;
                    // Die schliessende Klammer des `(module instance …)`
                    // noch lesen (sonst laeuft der Leser aus der Gruppe).
                    if matches!(self.sieht(), Some(T::Zu)) {
                        self.zu()?;
                    }
                    return Ok(Skript::Instanz { nummer, quelle });
                }
                if let Some(T::Name(n)) = self.sieht().cloned() {
                    self.pos += 1;
                    self.bezeichner_nummer.insert(n, self.module);
                }
                let m = self.modul_inhalt()?;
                let nummer = self.module;
                self.module += 1;
                Ok(Skript::Modul {
                    nummer,
                    inhalt: m,
                    definition,
                })
            }
            "register" => {
                let name = self.name()?;
                // `(register "n")`, `(register "n" 2)` oder `(register "n" $id)`
                let nummer = match self.sieht().cloned() {
                    Some(T::Wort(_)) => self.name()?.parse::<u32>().ok(),
                    Some(T::Name(_)) => {
                        let bezeichner = self.name()?;
                        // `$name` wird vom Lauflese-Schritt ueber die
                        // Modulbezeichner aufgeloest; ohne Tabelle `None`.
                        self.bezeichner_nummer.get(&bezeichner).copied()
                    }
                    _ => None,
                };
                self.zu()?;
                Ok(Skript::Register { name, nummer })
            }
            "invoke" => {
                // `(invoke $modul "name" …)` — das erste Token kann der
                // Bezeichner eines Moduls sein.
                let mut modul = None;
                if let Some(T::Name(n)) = self.sieht().cloned() {
                    self.pos += 1;
                    modul = self.bezeichner_nummer.get(&n).copied();
                    if modul.is_none() {
                        return Err(Fehler::form(
                            ART_UNVOLLSTAENDIG,
                            format!("wast: Modulbezeichner ${n} unbekannt"),
                        ));
                    }
                }
                let name = self.name()?;
                let mut argumente = Vec::new();
                while !self.istende() {
                    argumente.push(self.wert()?);
                }
                self.zu()?;
                Ok(Skript::Aufrufen {
                    modul,
                    name,
                    argumente,
                })
            }
            "get" => {
                let mut modul = None;
                if let Some(T::Name(n)) = self.sieht().cloned() {
                    self.pos += 1;
                    modul = self.bezeichner_nummer.get(&n).copied();
                    if modul.is_none() {
                        return Err(Fehler::form(
                            ART_UNVOLLSTAENDIG,
                            format!("wast: Modulbezeichner ${n} unbekannt"),
                        ));
                    }
                }
                let name = self.name()?;
                self.zu()?;
                Ok(Skript::Lesen { modul, name })
            }
            "assert_return" => {
                let aufruf = self.aktion()?;
                let mut erwartet = Vec::new();
                while !self.istende() {
                    erwartet.push(self.erwartung()?);
                }
                self.zu()?;
                Ok(Skript::Rueckgabe {
                    aufruf: Box::new(aufruf),
                    erwartet,
                })
            }
            "assert_return_canonical_nan" | "assert_return_arithmetic_nan" => {
                let aufruf = self.aktion()?;
                self.zu()?;
                Ok(Skript::NanPruefung {
                    aufruf: Box::new(aufruf),
                    arithmetisch: k == "assert_return_arithmetic_nan",
                })
            }
            "assert_trap" => {
                let aufruf = self.aktion()?;
                let text = self.name().unwrap_or_default();
                // Erwarteter Text darf aus mehreren Woertern bestehen.
                let mut voll = text;
                while let Some(T::Wort(_)) = self.sieht() {
                    voll.push(' ');
                    voll.push_str(&self.name()?);
                }
                self.zu()?;
                Ok(Skript::Trap {
                    aufruf: Box::new(aufruf),
                    text: voll,
                })
            }
            "assert_exhaustion" => {
                let aufruf = self.aktion()?;
                let text = self.name().unwrap_or_default();
                self.zu()?;
                Ok(Skript::Erschopfung {
                    aufruf: Box::new(aufruf),
                    text,
                })
            }
            "assert_malformed" | "assert_invalid" | "assert_unlinkable" => {
                let modul = self.modul_inhalt()?;
                let mut text = String::new();
                while !self.istende() {
                    text.push(' ');
                    text.push_str(&self.name()?);
                }
                self.zu()?;
                Ok(match k {
                    "assert_malformed" => Skript::Fehlerhaft {
                        modul,
                        text: text.trim().to_string(),
                    },
                    "assert_invalid" => Skript::Ungueltig {
                        modul,
                        text: text.trim().to_string(),
                    },
                    _ => Skript::NichtLinkbar {
                        modul,
                        text: text.trim().to_string(),
                    },
                })
            }
            // Unbekannte Top-Level-Befehle: ueberspringen (z. B. `;;`-Zeilen
            // in Kommentar-Form). Das haelt den Leser robust gegen Erweiterungen.
            _ => {
                self.klammer_ueberspringen();
                Ok(Skript::Register {
                    name: format!("__unbekannt:{k}"),
                    nummer: None,
                })
            }
        }
    }

    fn istende(&self) -> bool {
        matches!(self.sieht(), Some(T::Zu) | None)
    }

    /// Den Rest der aktuellen Klammer ueberspringen.
    ///
    /// WICHTIG: die Position wird in JEDEM Zweig weitergesetzt. Vorher
    /// stand in `T::Auf(_) => tiefe += 1` kein `pos += 1` — der Leser
    /// blickte immer wieder auf dieselbe Klammer und lief endlos.
    ///
    /// GEMESSEN 03.10. (Fuzz-Fall 6 von 2 000:
    /// `(assert__return (i32.const 1))`; M8 verbietet den Haenger).
    /// Die harte Grenze ist die Tokenzahl (`Grenzen::wast_befehle`).
    fn klammer_ueberspringen(&mut self) {
        let mut tiefe = 1i64;
        // M3: harte Grenze — hoechstens so viele Schritte wie Tokens plus
        // die eine oeffnende Klammer.
        let grenze = self.tok.len() + 2;
        let mut schritte = 0usize;
        while let Some(t) = self.sieht() {
            schritte += 1;
            if schritte > grenze {
                return;
            }
            match t {
                T::Auf(_) => tiefe += 1,
                T::Zu => {
                    tiefe -= 1;
                    self.pos += 1;
                    if tiefe == 0 {
                        return;
                    }
                }
                _ => self.pos += 1,
            }
        }
    }

    /// `(invoke …)` oder `(get …)` als Aktion lesen.
    fn aktion(&mut self) -> Result<Skript, Fehler> {
        match self.sieht().cloned() {
            Some(T::Auf(k)) => self.befehl(&k),
            t => Err(skriptfehler(&format!(
                "Aktion erwartet, gefunden {}",
                t.map(|x| x.text()).unwrap_or_default()
            ))),
        }
    }

    /// Den Inhalt eines `(module …)` lesen.
    ///
    /// GEMESSEN 01.10.: `binary`/`quote` stehen als NACKTES Wort, nicht in
    /// Klammern (`(module binary "…")`), und die Klammer `(module` ist beim
    /// Top-Level-Aufruf schon verbraucht — in `assert_malformed` aber NICHT
    /// (dort steht `(assert_malformed (module …) …)`), deshalb wird sie hier
    /// optional mitgenommen.
    fn modul_inhalt(&mut self) -> Result<ModulInhalt, Fehler> {
        if matches!(self.sieht(), Some(T::Auf(k)) if k == "module") {
            self.pos += 1;
            if self.sieht_wort_ist("definition") {
                self.pos += 1;
            }
            if let Some(T::Name(_)) = self.sieht() {
                self.pos += 1;
            }
        }
        if self.sieht_wort_ist("binary") || self.sieht_wort_ist("quote") {
            let art = self.weiter().text();
            let mut bytes: Vec<u8> = Vec::new();
            while let Some(t) = self.sieht().cloned() {
                match t {
                    T::Text(b) => {
                        self.pos += 1;
                        bytes.extend_from_slice(&b);
                    }
                    T::Zu => break,
                    _ => {
                        self.pos += 1;
                    }
                }
            }
            self.zu()?;
            return Ok(if art == "binary" {
                ModulInhalt::Bytes(bytes)
            } else {
                ModulInhalt::Text(String::from_utf8_lossy(&bytes).to_string())
            });
        }
        // Textmodul: Tokens einsammeln und wieder zu Text zusammenbauen.
        let start = self.pos;
        let mut tiefe = 1i64;
        while let Some(t) = self.sieht() {
            match t {
                T::Auf(_) => tiefe += 1,
                T::Zu => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        break;
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
        let teile: Vec<String> = self.tok[start..self.pos].iter().map(|t| t.text()).collect();
        self.zu()?;
        // Woerter mit Trennzeichen: `(func (param i32) i32.const 1 end)` wird
        // zu `(func (param i32) i32.const 1 end)` — mit Leerzeichen, was der
        // WAT-Leser wieder zerlegt.
        let mut text = String::from("(module");
        for t in teile {
            text.push(' ');
            text.push_str(&t);
        }
        text.push(')');
        Ok(ModulInhalt::Text(text))
    }

    /// Ein Argument: `(i32.const 5)` usw. (NaN-Erwartungen sind hier nicht
    /// erlaubt und ergeben einen benannten Fehler).
    fn wert(&mut self) -> Result<Wert, Fehler> {
        self.literal()?
            .0
            .into_wert()
            .ok_or_else(|| skriptfehler("Argument: nan:… ist hier nicht erlaubt"))
    }

    /// `(i32.const …)`, `(ref.func …)`, `nan:canonical`, `either …` lesen.
    /// Rueckgabe: Erwartung und (falls bekannt) der Werttyp.
    fn literal(&mut self) -> Result<(Erwartung, Option<Valtyp>), Fehler> {
        let k = match self.weiter() {
            T::Auf(k) => k,
            t => {
                return Err(skriptfehler(&format!(
                    "Literal erwartet, gefunden {}",
                    t.text()
                )));
            }
        };
        // ---- GC-Erwartungen (Wasm 3.0 GC) -----------------------------
        // `(ref.i31)`, `(ref.i31 (i32.const 7))`, `(ref.struct)`,
        // `(ref.array)`, `(ref.array i32 (i32.const 1))`, `(ref.eq)`.
        //
        // GEMESSEN 03.10. (`extern.wast`, `i31.wast`, `struct.wast`,
        // `array.wast`, `array_new_data.wast`, `array_new_elem.wast`):
        // ohne diese Formen war das Skript nicht lesbar und die Datei
        // kam mit 0 von 0 nicht einmal in den Nenner.
        if matches!(
            k.as_str(),
            "ref.i31" | "ref.struct" | "ref.array" | "ref.eq"
        ) {
            let art = match k.as_str() {
                "ref.i31" => GcArt::I31,
                "ref.struct" => GcArt::Struct,
                "ref.array" => GcArt::Array,
                _ => GcArt::Eq,
            };
            // Der Inhalt wird als Liste von LITERALEN gelesen; ein
            // Elementtyp-Wort (`i32`) wird uebersprungen.
            let mut werte: Vec<Wert> = Vec::new();
            let mut null = false;
            while !matches!(self.sieht(), Some(T::Zu) | None) {
                match self.sieht().cloned() {
                    Some(T::Wort(w)) if w == "null" => {
                        self.pos += 1;
                        null = true;
                    }
                    Some(T::Wort(w)) if w.parse::<i64>().is_ok() => {
                        self.pos += 1;
                    }
                    Some(T::Auf(a))
                        if a.ends_with("const")
                            || a == "ref.null"
                            || a == "ref.extern"
                            || a == "ref.host" =>
                    {
                        match self.literal()? {
                            (Erwartung::Genau(w), _) => werte.push(w),
                            // `(ref.null …)` o. Ae. als Inhalt = null.
                            _ => null = true,
                        }
                    }
                    t => {
                        return Err(skriptfehler(&format!(
                            "ref.{k}: unerwartetes Token {}",
                            t.map(|x| x.text()).unwrap_or_else(|| "Ende".into())
                        )));
                    }
                }
            }
            self.zu()?;
            let inhalt = if null {
                GcInhalt::Null
            } else if art == GcArt::I31 {
                match werte.first() {
                    Some(Wert::I32(v)) => GcInhalt::I31(*v),
                    _ => GcInhalt::Irrelevant,
                }
            } else if werte.is_empty() {
                GcInhalt::Irrelevant
            } else {
                GcInhalt::Werte(werte)
            };
            return Ok((Erwartung::Gc(art, inhalt), None));
        }
        if k.ends_with("const") {
            let typ = if k.starts_with("i32") {
                Valtyp::I32
            } else if k.starts_with("i64") {
                Valtyp::I64
            } else if k.starts_with("f32") {
                Valtyp::F32
            } else {
                Valtyp::F64
            };
            let w = self.wort()?;
            // `(f32.const nan:canonical)` / `(f32.const nan:arithmetic)`: die
            // Testsuite schreibt die NaN-Art IN der Konstante.
            if matches!(typ, Valtyp::F32 | Valtyp::F64)
                && (w == "nan:canonical" || w == "nan:arithmetic")
            {
                self.zu()?;
                return Ok((
                    if w.ends_with("canonical") {
                        Erwartung::NanKanonisch(false)
                    } else {
                        Erwartung::NanArithmetisch(false)
                    },
                    Some(typ),
                ));
            }
            let wert = match typ {
                Valtyp::I32 => Wert::I32(i64_wert(&w)? as i32),
                Valtyp::I64 => Wert::I64(i64_wert(&w)?),
                Valtyp::F32 => Wert::F32(crate::wat::f32_bits(&w)?),
                _ => Wert::F64(crate::wat::f64_bits(&w)?),
            };
            self.zu()?;
            return Ok((Erwartung::Genau(wert), Some(typ)));
        }
        match k.as_str() {
            "nan:canonical" => {
                self.zu()?;
                Ok((Erwartung::NanKanonisch(false), None))
            }
            "nan:arithmetic" => {
                self.zu()?;
                Ok((Erwartung::NanArithmetisch(false), None))
            }
            "either" => {
                let a = self.literal()?;
                let b = self.literal()?;
                self.zu()?;
                Ok((Erwartung::Entweder(Box::new(a.0), Box::new(b.0)), None))
            }
            "ref.func" | "ref.extern" | "ref.null" | "ref.host" => {
                let mut arg: Option<u32> = None;
                // Der Heaptype wird festgehalten: `ref.null func` und
                // `ref.null extern` sind VERSCHIEDENE null-Werte. Ohne das
                // wurde jedes `ref.null` als `funcref`-null gelesen.
                //
                // GEMESSEN 03.10. (`global.wast` Z. 206/207: „null
                // erwartet, null").
                let mut extern_art = k.contains("extern") || k.contains("host");
                let mut heaptype_gesehen = false;
                while let Some(t) = self.sieht().cloned() {
                    match t {
                        T::Wort(w) if w == "func" => {
                            heaptype_gesehen = true;
                            self.pos += 1;
                        }
                        T::Wort(w) if w == "extern" || w == "host" => {
                            heaptype_gesehen = true;
                            extern_art = true;
                            self.pos += 1;
                        }
                        T::Name(_) if k == "ref.null" => {
                            // `(ref.null $t)` — ein konkreter Typindex.
                            heaptype_gesehen = true;
                            self.pos += 1;
                        }
                        T::Wort(w) if k == "ref.null" && w != "null" => {
                            // `(ref.null none)`, `(ref.null any)` … die
                            // 3.0-Typen; als Erwartung heisst das
                            // „irgendeine null-Referenz".
                            //
                            // `noextern` ist die LEERE Form von `extern`:
                            // der Wert `null` wird zur Laufzeit als
                            // externer null abgelegt (siehe
                            // `Reftyp::ist_extern`), also muss die
                            // Erwartung dasselbe erwarten.
                            //
                            // GEMESSEN 03.10. (`ref_null.wast`: „null
                            // erwartet, null").
                            heaptype_gesehen = true;
                            if w.contains("extern") {
                                extern_art = true;
                            }
                            self.pos += 1;
                        }
                        T::Wort(w) => {
                            arg = zahl(&w).ok().map(|v| v as u32);
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                self.zu()?;
                if k == "ref.null" && !heaptype_gesehen {
                    return Ok((Erwartung::IrgendeineNull, None));
                }
                // NUR `(ref.extern)` / `(ref.host)` OHNE Zahl sind
                // Platzhalter. `(ref.null extern)` hat zwar auch keine
                // Zahl, ist aber eine NULL-Erwartung — mein erster
                // Versuch hat sie mit hier abgefangen und 40 Faelle
                // gekostet (Kernquote 99,97 % -> 99,81 %,
                // 20935/20974, GEMESSEN 03.10.).
                if (k == "ref.extern" || k == "ref.host") && arg.is_none() {
                    //
                    // KEIN zweites `self.zu()` — die Gruppe ist oben schon
                    // geschlossen. Mit dem zweiten `zu()` sprang der Leser
                    // eine Klammer zu weit und alle folgenden Dateien
                    // zerfielen (Kernquote 99,97 % -> 98,17 %,
                    // 20590/20974, GEMESSEN 03.10.).
                    return Ok((Erwartung::IrgendeineExtern, None));
                }
                let wert = if extern_art {
                    Wert::Extern(arg)
                } else if arg.is_none() {
                    // `(ref.func)` ohne Zahl ist die Erwartung „irgendeine
                    // Funktionsreferenz"; als AUFRUFARGUMENT bleibt es null.
                    if k == "ref.func" {
                        return Ok((Erwartung::IrgendeineFunktion, None));
                    }
                    Wert::Func(None)
                } else {
                    Wert::Func(arg)
                };
                Ok((Erwartung::Genau(wert), None))
            }
            andere => Err(skriptfehler(&format!("unbekanntes Literal ({andere})"))),
        }
    }

    /// Eine Erwartung in `assert_return` lesen.
    fn erwartung(&mut self) -> Result<Erwartung, Fehler> {
        Ok(self.literal()?.0)
    }

    fn wort(&mut self) -> Result<String, Fehler> {
        match self.weiter() {
            T::Wort(w) => Ok(w),
            t => Err(skriptfehler(&format!(
                "Wort erwartet, gefunden {}",
                t.text()
            ))),
        }
    }
}

/// Erwartung -> Wert (fuer Aufruf-Argumente).
impl Erwartung {
    pub fn into_wert(self) -> Option<Wert> {
        match self {
            Erwartung::Genau(w) => Some(w),
            // Als AUFRUFARGUMENT ist der Platzhalter `(ref.extern)` ein
            // externref-Null — so wurde es vor der Erweiterung gelesen.
            //
            // GEMESSEN 03.10.: ohne diesen Arm brachen alle Dateien, die
            // `(ref.extern)` als Argument schreiben
            // (`ref_is_null.wast`, `table_fill.wast`, `table_grow.wast`,
            // `table_set.wast`: „Argument: nan:… ist hier nicht erlaubt").
            Erwartung::IrgendeineExtern => Some(Wert::Extern(None)),
            _ => None,
        }
    }
}

/// Der Wert einer Hexziffer.
fn hexwert(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Ein als maskierte Zeichenkette geschriebener Bezeichner: `$"…"`.
///
/// Regeln der Spezifikation: die Maskierungen werden aufgeloest (`\t`,
/// `\41`, `\u{41}`), ein UNMASKIERTER Tabulator oder Zeilenumbruch ist
/// verboten, hinter der schliessenden Klammer muss ein Trennzeichen
/// stehen, und der Name darf nicht leer sein.
///
/// Die Bytes werden gesammelt und am Ende als UTF-8 gelesen: ein
/// unmaskierter Mehrbyte-Bereich (`$"\u{f61a}"`-Schreibweise in
/// `id.wast`) soll EIN Zeichenzug sein.
///
/// GEMESSEN 02.10. (`id.wast`): fuenf `assert_malformed` (leerer
/// Bezeichner, unmaskiertes Zeilenzeichen, kaputtes UTF-8) und ein Modul
/// mit Bezeichnern wie `$"\41B"` — der Name lautet `AB`.
pub fn maskierter_name(b: &[u8], i: &mut usize) -> Result<String, Fehler> {
    let mut bytes: Vec<u8> = Vec::new();
    *i += 1; // oeffnendes Anfuehrungszeichen
    while *i < b.len() {
        match b[*i] {
            b'"' => {
                *i += 1;
                if *i < b.len()
                    && !matches!(
                        b[*i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"' | b';'
                    )
                {
                    return Err(skriptfehler(
                        "leerer Bezeichner: hinter der Zeichenkette muss ein Trennzeichen stehen",
                    ));
                }
                if bytes.is_empty() {
                    return Err(skriptfehler("leerer Bezeichner"));
                }
                return String::from_utf8(bytes)
                    .map_err(|_| skriptfehler("Bezeichner ist kein gueltiges UTF-8"));
            }
            // Unmaskierte Zeilenzeichen sind verboten; die Maskierungen
            // `\t` und `\n` sind es nicht.
            b'\n' | b'\r' | b'\t' => {
                return Err(skriptfehler(
                    "Bezeichner mit unmaskiertem Zeichen (Zeilenumbruch/Tabulator)",
                ));
            }
            b'\\' => {
                if *i + 1 >= b.len() {
                    return Err(skriptfehler("Maskierung am Ende der Datei"));
                }
                let zeichen = b[*i + 1];
                match zeichen {
                    b'u' => {
                        let mut k = *i + 2;
                        while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                            k += 1;
                        }
                        if k >= b.len() || b[k] != b'{' {
                            return Err(skriptfehler("\\u ohne \"{\""));
                        }
                        k += 1;
                        let mut wert: u32 = 0;
                        let mut ziffern = 0usize;
                        while k < b.len() && b[k] != b'}' {
                            let z = hexwert(b[k])
                                .ok_or_else(|| skriptfehler("\\u{…}: keine Hexziffer"))?;
                            wert = wert.saturating_mul(16).saturating_add(z as u32);
                            ziffern += 1;
                            k += 1;
                        }
                        if k >= b.len() || ziffern == 0 {
                            return Err(skriptfehler("\\u{…} ohne Ziffern"));
                        }
                        *i = k + 1;
                        let c = char::from_u32(wert)
                            .ok_or_else(|| skriptfehler("\\u{…}: kein Zeichen"))?;
                        let mut puffer = [0u8; 4];
                        bytes.extend_from_slice(c.encode_utf8(&mut puffer).as_bytes());
                    }
                    // Die einfachen Maskierungen ZUERST: `\t` ist eine
                    // Tabulator-Maskierung und nicht die Hexziffern `\t`
                    // mit der folgenden Ziffer.
                    b't' | b'n' | b'r' | b'"' | b'\'' => {
                        *i += 2;
                        bytes.push(maskierung(zeichen) as u8);
                    }
                    b'\\' => {
                        *i += 2;
                        bytes.push(b'\\');
                    }
                    _ if zeichen.is_ascii_hexdigit() => {
                        if *i + 2 >= b.len() {
                            return Err(skriptfehler("\\XY: zu kurz"));
                        }
                        let z = match (hexwert(b[*i + 1]), hexwert(b[*i + 2])) {
                            (Some(hoch), Some(niedrig)) => hoch * 16 + niedrig,
                            _ => {
                                return Err(skriptfehler(&format!(
                                    "\\XY: keine Hexziffern (Pos {})",
                                    *i
                                )));
                            }
                        };
                        *i += 3;
                        bytes.push(z);
                    }
                    andere => {
                        *i += 2;
                        bytes.push(maskierung(andere) as u8);
                    }
                }
            }
            _ => {
                bytes.push(b[*i]);
                *i += 1;
            }
        }
    }
    Err(skriptfehler("unvollstaendiger Bezeichner"))
}

/// Eine Maskierung in einer Zeichenkette aufloesen (`\\n` -> Zeilenumbruch).
pub fn maskierung(b: u8) -> char {
    match b {
        b'n' => '\n',
        b't' => '\t',
        b'r' => '\r',
        b'\\' => '\\',
        b'\'' => '\'',
        b'"' => '"',
        andere => andere as char,
    }
}

/// Vorzeichenbehaftete Zahl (fuer `i32.const -1`).
fn i64_wert(w: &str) -> Result<i64, Fehler> {
    let s: String = w.chars().filter(|c| *c != '_').collect();
    if let Some(r) = s.strip_prefix('-') {
        if let Some(h) = r.strip_prefix("0x") {
            return i64::from_str_radix(h, 16)
                .map(|v| v.wrapping_neg())
                .map_err(|_| skriptfehler(&format!("Zahl {w}")));
        }
        // GEMESSEN 01.10. (`int_literals.wast`): -9223372036854775808 ist
        // i64::MIN und passt nicht in i64 — deshalb ueber u64 lesen.
        if let Ok(v) = r.parse::<u64>() {
            return Ok((v as i64).wrapping_neg());
        }
        return r
            .parse::<i64>()
            .map(|v| v.wrapping_neg())
            .map_err(|_| skriptfehler(&format!("Zahl {w}")));
    }
    if let Some(h) = s.strip_prefix("0x") {
        // GEMESSEN 01.10. (`memory.wast`): Konstanten reichen bis
        // 0xffff_ffff_ffff_ffff, also als u64 lesen und umdeuten.
        return u64::from_str_radix(h, 16)
            .map(|v| v as i64)
            .map_err(|_| skriptfehler(&format!("Zahl {w}")));
    }
    if let Ok(v) = s.parse::<i64>() {
        return Ok(v);
    }
    s.parse::<u64>()
        .map(|v| v as i64)
        .map_err(|_| skriptfehler(&format!("Zahl {w}")))
}

fn zahl(w: &str) -> Result<u64, Fehler> {
    let s: String = w.chars().filter(|c| *c != '_').collect();
    if let Some(h) = s.strip_prefix("0x") {
        return u64::from_str_radix(h, 16).map_err(|_| skriptfehler(&format!("Zahl {w}")));
    }
    if let Some(r) = s.strip_prefix('-') {
        return u64::from_str_radix(r, 16)
            .or_else(|_| r.parse::<u64>())
            .map(|v| v.wrapping_neg())
            .map_err(|_| skriptfehler(&format!("Zahl {w}")));
    }
    s.parse::<u64>()
        .map_err(|_| skriptfehler(&format!("Zahl {w}")))
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Skript lesen, Befehlsarten zaehlen.
    #[test]
    fn grundbefehle() {
        let s = lies_skript(
            r#"
            (module (func (export "f") (result i32) i32.const 42))
            (assert_return (invoke "f") (i32.const 42))
            (assert_trap (invoke "f") "unreachable")
            (module binary "\00asm\01\00\00\00")
            (register "m")
            "#,
            &Grenzen::standard(),
        )
        .unwrap();
        assert_eq!(s.len(), 5);
        match &s[0] {
            Skript::Modul {
                nummer,
                inhalt,
                definition,
            } => {
                assert_eq!(*nummer, 0);
                assert!(!*definition);
                assert!(matches!(inhalt, ModulInhalt::Text(_)));
            }
            _ => panic!("erstes muss ein Modul sein"),
        }
        assert!(matches!(s[1], Skript::Rueckgabe { .. }));
        assert!(matches!(s[2], Skript::Trap { .. }));
        assert!(matches!(
            s[3],
            Skript::Modul {
                inhalt: ModulInhalt::Bytes(_),
                ..
            }
        ));
        assert!(matches!(s[4], Skript::Register { .. }));
    }

    #[test]
    fn nan_erwartungen() {
        let s = lies_skript(
            r#"(module) (assert_return (invoke "f") (f32.const nan:canonical) (f64.const nan:arithmetic))"#,
            &Grenzen::standard(),
        )
        .unwrap();
        match &s[1] {
            Skript::Rueckgabe { erwartet, .. } => {
                assert_eq!(erwartet.len(), 2);
                assert_eq!(erwartet[0], Erwartung::NanKanonisch(false));
                assert_eq!(erwartet[1], Erwartung::NanArithmetisch(false));
            }
            _ => panic!("muss Rueckgabe sein"),
        }
    }

    #[test]
    fn bomben_skript() {
        let k = Grenzen::bombe();
        assert!(lies_skript("(module", &k).is_err());
        assert!(lies_skript("(assert_return", &k).is_err());
        // Endlosschleife im Text: viele Klammern -> Grenze
        let viel = format!("(module){}", "(invoke \"f\")".repeat(5000));
        assert!(lies_skript(&viel, &k).is_err());
    }
}
