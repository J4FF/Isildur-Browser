//! W5: Debug-Schnittstelle fuer die Entwicklerwerkzeuge
//! (`tools.browser`, Reiter „WebAssembly", Punkt D6 der Rolle DEVTOOLS).
//!
//! # Warum eine Spur statt eines Schrittzustands im Interpreter
//!
//! Der erste Entwurf wollte den Interpreter anhalten und fortsetzen. Das ist
//! bei einem flachen Rahmenstapel moeglich, kostet aber einen wiederaufsetzbaren
//! Maschinenzustand im `Lauf`. GEMESSEN am 01.10. ist die ehrlichere und
//! einfachere Loesung: laufen lassen und jeden Befehl als SCHRITT
//! aufzeichnen (`Spur`), mit harter Obergrenze aus `Grenzen`
//! (`debug_schritte`, Voreinstellung 200 000). Haltepunkte, Einzelschritt,
//! Aufrufstapel, Locals und Speicheransicht lesen dann nur noch aus dieser
//! Spur — ohne den Interpreter anzufassen.
//!
//! Nachteil, offen benannt: die Spur ist nicht „live". Fuer einen
//! Einzelschritt ueber dieselbe Stelle wird neu gestartet. Fuer echten
//! Live-Betrieb (Stopp im Dauerlauf) muesste der Interpreter einen
//! fortsetzbaren Zustand bekommen — siehe `team/WUNSCH_WASM.txt`.

use crate::fehler::Fehler;
use crate::grenzen::Grenzen;
use crate::lauf::{FremdFn, Instanz, Lauf};
use crate::modell::{Func, Modul};
use crate::wert::Wert;

/// Hoechstgezeigte Byte in der Hex-Ansicht (64 KiB).
///
/// Die Grenze gehoert zur Anzeige, nicht zum Speicher: was darueber
/// liegt, kann in einer Hex-Liste niemand lesen.
pub const MAX_HEX_BYTE: usize = 64 * 1024;

/// Ein aufgezeichneter Schritt: ein Befehl wurde ausgefuehrt (oder soll
/// ausgefuehrt werden — siehe `pc`).
#[derive(Debug, Clone, PartialEq)]
pub struct Schritt {
    /// Funktionsindex im Modul (lokal, ohne Importe).
    pub funktion: u32,
    /// Befehlsindex im Koerper (die Position, an der der Schritt beginnt).
    pub versatz: usize,
    /// Tiefe im Aufrufstapel (0 = aufrufende Funktion).
    pub tiefe: usize,
    /// Lokalvariablen des Rahmens (Parameter zuerst).
    pub lokale: Vec<Wert>,
    /// Operandstapel des Rahmens, von unten nach oben.
    pub stapel: Vec<Wert>,
    /// Ergebnis der bisherigen Laeufe (nur beim tieferen Schritt sinnvoll).
    pub ergebnis: Vec<Wert>,
    /// Speichergroesse in Seiten zu diesem Zeitpunkt.
    pub speicher_seiten: u64,
    /// Verbrauchtes Budget (Brennstoff).
    pub verbraucht: u64,
}

/// Die Spur: eine Liste von Schritten mit harter Obergrenze.
#[derive(Debug, Default)]
pub struct Spur {
    pub schritte: Vec<Schritt>,
    grenze: usize,
    /// True, wenn die Grenze erreicht wurde (der Rest fehlt).
    pub abgeschnitten: bool,
}

impl Spur {
    pub fn neu(grenze: usize) -> Spur {
        Spur {
            schritte: Vec::new(),
            grenze: grenze.min(crate::grenzen::MAX_DEBUG_SCHRITTE_HART),
            abgeschnitten: false,
        }
    }
    /// Schritt aufzeichnen; wird die Grenze erreicht, wird abgeschnitten.
    pub fn push(&mut self, s: Schritt) {
        if self.schritte.len() >= self.grenze {
            self.abgeschnitten = true;
            return;
        }
        self.schritte.push(s);
    }
    pub fn len(&self) -> usize {
        self.schritte.len()
    }
    /// Passt zu `len` (Clippy) und fuer die Anzeige.
    pub fn is_empty(&self) -> bool {
        self.schritte.is_empty()
    }
    pub fn ist_leer(&self) -> bool {
        self.schritte.is_empty()
    }
}

/// Haltepunkt: je Funktion ein Satz von Befehlsversetzen.
#[derive(Debug, Default, Clone)]
pub struct Haltepunkte {
    pub set: std::collections::BTreeMap<u32, std::collections::BTreeSet<usize>>,
}

impl Haltepunkte {
    /// Haltepunkt setzen/entfernen. `aktiv` = true setzt, false entfernt.
    pub fn setze(&mut self, funktion: u32, versatz: usize, aktiv: bool) {
        if aktiv {
            self.set.entry(funktion).or_default().insert(versatz);
        } else if let Some(m) = self.set.get_mut(&funktion) {
            m.remove(&versatz);
            if m.is_empty() {
                self.set.remove(&funktion);
            }
        }
    }
    /// Liegt an dieser Stelle ein Haltepunkt?
    pub fn hat(&self, funktion: u32, versatz: usize) -> bool {
        self.set
            .get(&funktion)
            .map(|m| m.contains(&versatz))
            .unwrap_or(false)
    }
    /// Alle Haltepunkte als Liste (fuer die Anzeige).
    pub fn alle(&self) -> Vec<(u32, usize)> {
        let mut v = Vec::new();
        for (f, m) in &self.set {
            for o in m {
                v.push((*f, *o));
            }
        }
        v
    }
}

/// Wie die Ausfuehrung fortgesetzt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fortsetzung {
    /// Frei laufen (kein Debugger-Stopp).
    Freies,
    /// Bis zum naechsten Haltepunkt.
    BisHaltepunkt,
    /// Genau ein Schritt.
    Einzelschritt,
    /// Bis zur Rueckkehr aus der aktuellen Funktion.
    BisRueckkehr,
}

/// Die drei Schrittknoepfe des Reiters „WebAssembly".
///
/// * [`SchrittArt::Hinein`] — EIN Befehl, bei `call` also in der
///   aufgerufenen Funktion (Versatz 0).
/// * [`SchrittArt::Ueber`] — die aufgerufene Funktion wird ganz
///   ausgefuehrt, danach steht man wieder im Aufrufer.
/// * [`SchrittArt::Hinaus`] — bis zur Rueckkehr aus dem aktuellen Rahmen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchrittArt {
    Hinein,
    Ueber,
    Hinaus,
}

/// EINE lokale Variable in der Anzeige.
///
/// Die `name`-Sektion hat einen Unterabschnitt „lokale Namen", den
/// `binaer.rs::lies_namen` nicht liest (er liest Unterabschnitt 1, die
/// Funktionsnamen) — bis der kommt, ist der Name aus der Stellung
/// abgeleitet: `param0`, `param1`, `local0` …
#[derive(Debug, Clone, PartialEq)]
pub struct LokalAnzeige {
    /// `param0` / `local0` …
    pub name: String,
    /// Der Wasm-Typ als Text (`i32`, `f64`, `funcref` …).
    pub typ: String,
    /// Der Wert als Text.
    pub wert: String,
}

/// EIN Rahmen in der Anzeige — „lokale Variablen und Operandenstapel JE
/// RAHMEN" (team/rollen/WASM.txt).
#[derive(Debug, Clone, PartialEq)]
pub struct RahmenAnzeige {
    /// Funktionsindex im Modul.
    pub funktion: u32,
    /// Anzeigename: Export, sonst `name`-Sektion, sonst `func#<index>`
    /// (eine Regel, `Modul::funktions_name`).
    pub name: String,
    /// Naechster auszufuehrender Befehlsversatz (= Haltepunktversatz).
    pub versatz: usize,
    /// 0 = aeusserster Rahmen.
    pub tiefe: usize,
    /// Parameter zuerst, dann eigene.
    pub lokal: Vec<LokalAnzeige>,
    /// Von UNTEN nach OBEN; die Anzeige dreht sie um (oberster zuerst).
    pub stapel: Vec<String>,
}

/// Ein gesetzter Haltepunkt in der Anzeige (beides: zum Umschalten und
/// zum Zeigen).
#[derive(Debug, Clone, PartialEq)]
pub struct HaltepunktAnzeige {
    pub funktion: u32,
    pub versatz: usize,
    pub name: String,
}

/// Eine Zeile der Hex-Ansicht: Adresse, Byte, ASCII-Spalte.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeicherZeile {
    pub adresse: u64,
    pub werte: Vec<u8>,
    /// Nicht druckbare Byte als `.` (Spaltenbreite = `werte.len()`).
    pub ascii: String,
}

/// ALLES, was der Reiter „WebAssembly" auf einen Blick zeigt — die
/// Schnittstelle zu DEVPANEL, gebaut von [`Debugger::ansicht`].
#[derive(Debug, Clone, PartialEq)]
pub struct Ansicht {
    /// `laeuft: <name>@<versatz>, Tiefe <n>` | `fertig: <werte>` |
    /// `Fehler: <text>`.
    pub status: String,
    /// Verbrauchtes Brennstoff dieser Sitzung.
    pub verbraucht: u64,
    pub haltepunkte: Vec<HaltepunktAnzeige>,
    /// Aeusserster Rahmen zuerst (wie `Zustand.rahmen`).
    pub rahmen: Vec<RahmenAnzeige>,
    /// Hex-Ansicht; **leer**, wenn der Speicher nicht reicht — der Grund
    /// steht in `speicher_hinweis`.
    pub speicher: Vec<SpeicherZeile>,
    pub speicher_hinweis: Option<String>,
    /// WAT der aktuellen Funktion, naechster Befehl markiert.
    pub wat: String,
}

/// Der Debugger: Modul, Spur, Haltepunkte, Position.
pub struct Debugger {
    pub modul: std::rc::Rc<Modul>,
    pub spur: Spur,
    pub haltepunkte: Haltepunkte,
    /// Index des naechsten Schritts in der Spur.
    pub pos: usize,
    /// Aktueller Schritt (fortgeschrieben).
    pub aktuell: Option<Schritt>,
    pub grenzen: Grenzen,
}

impl Debugger {
    /// Debugger anlegen — fuehrt die Funktion aus und zeichnet die Spur auf.
    pub fn neu(modul: std::rc::Rc<Modul>, grenzen: Grenzen) -> Debugger {
        Debugger {
            modul,
            spur: Spur::neu(grenzen.debug_schritte),
            haltepunkte: Haltepunkte::default(),
            pos: 0,
            aktuell: None,
            grenzen,
        }
    }

    /// Einen Export ausfuehren und dabei die Spur zeichnen.
    pub fn ausfuehren(
        &mut self,
        lauf: &mut Lauf,
        instanz: &Instanz,
        name: &str,
        args: &[Wert],
    ) -> Result<Vec<Wert>, Fehler> {
        lauf.spur_starten(self.grenzen.debug_schritte);
        let r = lauf.rufe_funktion(instanz, name, args);
        self.spur = lauf
            .spur
            .take()
            .unwrap_or_else(|| Spur::neu(self.grenzen.debug_schritte));
        self.pos = 0;
        self.aktuell = self.spur.schritte.first().cloned();
        r
    }

    /// Zum naechsten Schritt gehen; `false` am Ende der Spur.
    /// EINE Anweisung im LAUFENDEN Zustand ausfuehren und den neuen
    /// Zustand als `Schritt` liefern (W5/D11: „echt anhalten").
    ///
    /// `halt` entscheidet VOR dem Schritt, ob angehalten wird: `true`
    /// heisst, der Aufrufer will am aktuellen Punkt stehen bleiben
    /// (Haltepunkt), dann wird NICHTS ausgefuehrt.
    ///
    /// GEMESSEN 03.10. (`instance_wast_lauf_unterbrechen`).
    pub fn weiter_live(
        &mut self,
        sitzung: &mut crate::interp::Sitzung,
        lauf: &mut crate::lauf::Lauf,
        halt: impl Fn(u32, usize) -> bool,
    ) -> Option<crate::interp::Zustand> {
        // `position()` statt `zustand()`: beim Stehenbleiben wird kopiert,
        // beim Weiterlaufen nicht.
        let (f, v) = sitzung.position()?;
        if halt(f, v) {
            // stehen bleiben: der Aufrufer entscheidet, was passiert.
            return Some(sitzung.zustand(lauf));
        }
        sitzung.weiter(lauf);
        Some(sitzung.zustand(lauf))
    }

    /// Bis zum naechsten Haltepunkt weiterlaufen (Schritt fuer Schritt,
    /// damit Endlosschleifen ueber das Brennstofflimit enden).
    ///
    /// `grenze` ist die harte Obergrenze der Schritte (M3).
    ///
    /// GEMESSEN 03.10.: die Suche braucht pro Schritt ZWEI Zahlen. Vorher
    /// holte sie sich die ueber `zustand()` — also alle Locals und den
    /// ganzen Operandenstapel aller Rahmen, geklont, um sie wegzuwerfen.
    /// Jetzt liest `position()` die beiden Zahlen ohne Kopie; kopiert wird
    /// nur noch am Haltepunkt selbst.
    pub fn bis_haltepunkt_live(
        &mut self,
        sitzung: &mut crate::interp::Sitzung,
        lauf: &mut crate::lauf::Lauf,
        grenze: usize,
    ) -> Option<crate::interp::Zustand> {
        for _ in 0..grenze {
            if sitzung.ist_fertig() {
                return None;
            }
            if let Some((f, v)) = sitzung.position() {
                if self.haltepunkte.hat(f, v) {
                    return Some(sitzung.zustand(lauf));
                }
            }
            sitzung.weiter(lauf);
        }
        None
    }

    /// EIN Einzelschritt in der laufenden Sitzung (Hinein/Ueber/Hinaus).
    ///
    /// `None`, wenn der Aufruf **beendet** ist (Ergebnis oder Fehler danach
    /// aus der Sitzung lesen) oder wenn `grenze` Schritte ohne Ergebnis
    /// vergangen sind — eine Endlosschleife endet also auch hier benannt
    /// statt endlos (M3).
    ///
    /// Innen gilt dasselbe wie am Haltepunkt: wird dabei ein Haltepunkt
    /// erreicht, bleibt man stehen, auch wenn die Schrittart weiterwollte.
    pub fn schritt(
        &mut self,
        sitzung: &mut crate::interp::Sitzung,
        lauf: &mut crate::lauf::Lauf,
        art: SchrittArt,
        grenze: usize,
    ) -> Option<crate::interp::Zustand> {
        let tiefe0 = sitzung.tiefe();
        for _ in 0..grenze.max(1) {
            if !sitzung.weiter(lauf) || sitzung.ist_fertig() {
                return None;
            }
            let (f, v) = sitzung.position()?;
            if self.haltepunkte.hat(f, v) {
                return Some(sitzung.zustand(lauf));
            }
            let t = sitzung.tiefe();
            match art {
                // Ein Befehl ist getan — egal wie tief er gefuehrt hat.
                SchrittArt::Hinein => return Some(sitzung.zustand(lauf)),
                // Die Tiefe wieder am Start: der aufgerufene Teil ist
                // ganz durchgelaufen.
                SchrittArt::Ueber if t <= tiefe0 => return Some(sitzung.zustand(lauf)),
                // Die Tiefe kleiner als beim Start: der Rahmen ist
                // zurueckgekehrt.
                SchrittArt::Hinaus if t < tiefe0 => return Some(sitzung.zustand(lauf)),
                SchrittArt::Ueber | SchrittArt::Hinaus => {}
            }
        }
        None
    }

    pub fn weiter(&mut self) -> bool {
        if self.pos < self.spur.len() {
            self.aktuell = Some(self.spur.schritte[self.pos].clone());
            self.pos += 1;
            true
        } else {
            self.aktuell = None;
            false
        }
    }

    /// Einen Schritt zurueck (die Spur ist ein Array, das geht).
    pub fn zurueck(&mut self) -> bool {
        if self.pos > 0 {
            self.pos -= 1;
            self.aktuell = Some(self.spur.schritte[self.pos].clone());
            true
        } else {
            false
        }
    }

    /// Bis zum naechsten Haltepunkt laufen. `None`, wenn keiner mehr kommt.
    pub fn bis_haltepunkt(&mut self) -> Option<Schritt> {
        while self.pos < self.spur.len() {
            let s = self.spur.schritte[self.pos].clone();
            self.pos += 1;
            self.aktuell = Some(s.clone());
            if self.haltepunkte.hat(s.funktion, s.versatz) {
                return Some(s);
            }
        }
        None
    }

    /// Bis zur Rueckkehr aus der aktuellen Funktion laufen.
    pub fn bis_rueckkehr(&mut self) -> Option<Schritt> {
        let start = self.aktuell.as_ref().map(|s| s.tiefe).unwrap_or(0);
        while self.pos < self.spur.len() {
            let s = self.spur.schritte[self.pos].clone();
            self.pos += 1;
            self.aktuell = Some(s.clone());
            if s.tiefe < start {
                return Some(s);
            }
        }
        None
    }

    /// Aufrufstapel: Funktion + Versatz + Tiefe, von aussen nach innen.
    pub fn aufrufstapel(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for s in &self.spur.schritte[..self.pos.min(self.spur.len())] {
            let name = self.name(s.funktion);
            let eintrag = format!("{name}+{}", s.versatz);
            // Nur den obersten Rahmen je Tiefe behalten.
            if v.len() > s.tiefe {
                v.truncate(s.tiefe);
            }
            v.push(eintrag);
        }
        // Reihenfolge: aufrufende Funktion zuerst.
        let mut r: Vec<String> = Vec::new();
        let mut gesehen: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        for s in &self.spur.schritte[..self.pos.min(self.spur.len())] {
            gesehen.insert(
                s.tiefe,
                format!("{} @ {}", self.name(s.funktion), s.versatz),
            );
        }
        for (_, w) in gesehen {
            r.push(w);
        }
        r
    }

    /// Lokale Variablen des aktuellen Schritts als Textzeilen
    /// (`param0 (i32) = 7`).
    pub fn lokale(&self) -> Vec<String> {
        match &self.aktuell {
            Some(s) => self
                .lokal_zeilen(s.funktion, &s.lokale)
                .into_iter()
                .map(|l| format!("{} ({}) = {}", l.name, l.typ, l.wert))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Die Locals EINES Rahmens als Anzeigezeilen: Parameter zuerst, dann
    /// eigene — Name, Typ, Wert.
    ///
    /// Ein Namensrecht fuer alle Anzeigen (Aufrufstapel, `lokale()`, der
    /// Reiter): die Namen kommen aus der Signatur und den Lokaltypen des
    /// Moduls, die Werte aus dem laufenden Rahmen.
    ///
    /// **Bewusste Luecke:** die `name`-Sektion fuehrt LOCALNAMEN
    /// (Unterabschnitt 2: je Funktion die Namen ihrer Locals); die liest
    /// `binaer.rs` nicht mit (dort gibt es nur Abschnitt 1 = die
    /// Funktionsnamen), Locals heissen also `param0`/`local1` auch dann,
    /// wenn die Quelle anders hiess. Steigt DEVPANEL darauf ein, ist das
    /// ein eigener kleiner Abschnitt — abgestimmt im `team/BRETT.txt`,
    /// nicht still eingebaut.
    fn lokal_zeilen(&self, funktion: u32, werte: &[Wert]) -> Vec<LokalAnzeige> {
        let ft = self
            .modul
            .signatur(funktion)
            .cloned()
            .unwrap_or(crate::modell::FuncTyp {
                parameter: vec![],
                ergebnis: vec![],
            });
        let anzahl_parameter = ft.parameter.len();
        let mut typen: Vec<String> = ft.parameter.iter().map(|t| t.text().to_string()).collect();
        if let Some(Func::Eigen { lokale, .. }) = self.modul.funktionen.get(funktion as usize) {
            typen.extend(lokale.iter().map(|t| t.text().to_string()));
        }
        werte
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let typ = typen.get(i).cloned().unwrap_or_else(|| String::from("?"));
                let name = if i < anzahl_parameter {
                    format!("param{i}")
                } else {
                    format!("local{}", i - anzahl_parameter)
                };
                LokalAnzeige {
                    name,
                    typ,
                    wert: crate::wert::als_text(w),
                }
            })
            .collect()
    }

    /// ALLE Rahmen mit Namen, Locals und eigenem Operandenstapel —
    /// aeusserster Rahmen zuerst (wie `Zustand.rahmen`).
    ///
    /// Das ist der Kern der Anforderung „lokale Variablen und
    /// Operandenstapel JE RAHMEN": vorher gab es nur den obersten.
    pub fn rahmen_anzeige(&self, z: &crate::interp::Zustand) -> Vec<RahmenAnzeige> {
        z.rahmen
            .iter()
            .enumerate()
            .map(|(i, r)| RahmenAnzeige {
                funktion: r.funktion,
                name: self.name(r.funktion),
                versatz: r.versatz,
                tiefe: i,
                lokal: self.lokal_zeilen(r.funktion, &r.lokale),
                stapel: r.stapel.iter().map(crate::wert::als_text).collect(),
            })
            .collect()
    }

    /// Operandstapel des aktuellen Schritts, von oben nach unten (wie im
    /// Entwicklerwerkzeug).
    pub fn operandstapel(&self) -> Vec<String> {
        match &self.aktuell {
            Some(s) => s.stapel.iter().rev().map(crate::wert::als_text).collect(),
            None => Vec::new(),
        }
    }

    /// Speicheransicht: `anzahl` Byte ab `adresse` als Hex (fuer DevTools).
    pub fn speicheransicht(
        &self,
        lauf: &Lauf,
        adresse: u64,
        anzahl: usize,
    ) -> Result<Vec<String>, Fehler> {
        let grenze = crate::grenzen::MAX_BEFEHLE_HART; // weit genug fuer DevTools
        let n = anzahl.min(grenze);
        let s = lauf
            .speicher()
            .ok_or_else(|| Fehler::link(crate::fehler::L_SPEICHER, "kein Speicher"))?;
        let daten = s.daten_view();
        let ende = (adresse as usize + n).min(daten.len());
        if (adresse as usize) >= daten.len() {
            return Ok(Vec::new());
        }
        Ok(daten[adresse as usize..ende]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }

    /// Linearer Speicher als **Hex-Ansicht**: `breite` Byte je Zeile
    /// (`0` = 16, wie ueblich), jeder Zeile ihre Adresse und eine
    /// ASCII-Spalte mit `.` fuer nicht druckbare Byte.
    ///
    /// `anzahl` wird auf [`MAX_HEX_BYTE`] gedeckelt, `breite` auf 64 —
    /// eine Hex-Ansicht, die 64 KiB zuruecklegt, ist fuer die Anzeige
    /// jenseits dessen, was jemand liest.
    ///
    /// GEMESSEN 03.10. (team/rollen/WASM.txt: „linearer Speicher als
    /// Hex-Ansicht").
    pub fn speicher_hex(
        &self,
        lauf: &Lauf,
        adresse: u64,
        anzahl: usize,
        breite: usize,
    ) -> Result<Vec<SpeicherZeile>, Fehler> {
        let breite = if breite == 0 { 16 } else { breite.min(64) };
        let n = anzahl.min(MAX_HEX_BYTE);
        let s = lauf
            .speicher()
            .ok_or_else(|| Fehler::link(crate::fehler::L_SPEICHER, "kein Speicher"))?;
        let daten = s.daten_view();
        let start = adresse as usize;
        if start >= daten.len() {
            return Ok(Vec::new());
        }
        let ende = start.saturating_add(n).min(daten.len());
        let mut zeilen = Vec::new();
        let mut i = start;
        while i < ende {
            let bis = i.saturating_add(breite).min(ende);
            let werte = daten[i..bis].to_vec();
            let ascii = werte
                .iter()
                .map(|b| {
                    if (0x20..0x7f).contains(b) {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            zeilen.push(SpeicherZeile {
                adresse: i as u64,
                werte,
                ascii,
            });
            i = bis;
        }
        Ok(zeilen)
    }

    /// Disassembly einer Funktion als WAT-Text (W5-Anforderung).
    pub fn disassembly(&self, funktion: u32) -> String {
        crate::wat::schreibe_funktion(&self.modul, funktion)
    }

    /// Disassembly der **aktuellen** Funktion — also der Rahmen, in dem
    /// man steht — mit dem naechsten auszufuehrenden Befehl markiert.
    ///
    /// Der markierte Versatz ist derselbe wie in `Zustand.rahmen` und in
    /// den Haltepunkten (team/rollen/WASM.txt: „Disassemblierung (WAT) der
    /// aktuellen Funktion").
    pub fn disassembly_aktuell(&self, z: &crate::interp::Zustand) -> String {
        match z.aufrufstapel.last() {
            Some((funktion, versatz)) => {
                crate::wat::schreibe_funktion_mit(&self.modul, *funktion, Some(*versatz))
            }
            None => String::new(),
        }
    }

    /// Zeilen aus den DWARF-Abschnitten (W5: „DWARF-Zeilen aus custom
    /// sections, wenn vorhanden").
    pub fn zeilen(&self, funktion: u32) -> Vec<crate::dwarf::Zeile> {
        crate::dwarf::zeilen_der_funktion(&self.modul, funktion, &self.grenzen)
    }

    /// Name einer Funktion (Exportname, sonst `name`-Sektion, sonst
    /// `func#<index>`). Eine Regel fuer alle Anzeigen, s.
    /// [`Modul::funktions_name`](crate::modell::Modul::funktions_name).
    pub fn name(&self, funktion: u32) -> String {
        self.modul.funktions_name(funktion)
    }

    /// ALLES, was der Reiter „WebAssembly" zeigt, in EINEM Aufruf — die
    /// Schnittstelle zu DEVPANEL.
    ///
    /// `adresse`/`anzahl` sind der Ausschnitt der Hex-Ansicht (DevTools:
    /// Vorgabe Adresse 0, 256 Byte; die Adresse darf der Nutzer aendern).
    /// Liegt die Adresse ausserhalb des Speichers, bleibt `speicher` leer
    /// und der Grund steht in `speicher_hinweis` — die uebrigen Baeuche
    /// (Stapel, Locals, WAT) sind davon unabhaengig.
    ///
    /// GEMESSEN 03.10. (team/rollen/WASM.txt).
    pub fn ansicht(
        &self,
        sitzung: &crate::interp::Sitzung,
        lauf: &Lauf,
        adresse: u64,
        anzahl: usize,
    ) -> Ansicht {
        let z = sitzung.zustand(lauf);
        let status = if let Some(e) = sitzung.fehler() {
            format!("Fehler: {e}")
        } else if sitzung.ist_fertig() {
            let w: Vec<String> = sitzung
                .ergebnis()
                .unwrap_or_default()
                .iter()
                .map(crate::wert::als_text)
                .collect();
            if w.is_empty() {
                String::from("fertig")
            } else {
                format!("fertig: {}", w.join(", "))
            }
        } else if let Some((funktion, versatz)) = z.aufrufstapel.last() {
            format!(
                "laeuft: {}@{versatz}, Tiefe {}",
                self.name(*funktion),
                z.aufrufstapel.len().saturating_sub(1)
            )
        } else {
            String::from("laeuft")
        };
        let (speicher, speicher_hinweis) = match self.speicher_hex(lauf, adresse, anzahl, 16) {
            Ok(z) if !z.is_empty() => (z, None),
            Ok(_) => (
                Vec::new(),
                Some(format!("Adresse {adresse} liegt ausserhalb des Speichers")),
            ),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let haltepunkte = self
            .haltepunkte
            .alle()
            .into_iter()
            .map(|(funktion, versatz)| HaltepunktAnzeige {
                name: self.name(funktion),
                funktion,
                versatz,
            })
            .collect();
        Ansicht {
            status,
            verbraucht: z.verbraucht,
            haltepunkte,
            rahmen: self.rahmen_anzeige(&z),
            speicher,
            speicher_hinweis,
            wat: self.disassembly_aktuell(&z),
        }
    }
}

impl Ansicht {
    /// Die Anzeige als Text. **Diese Zeilenform ist die Schnittstelle mit
    /// DEVPANEL** (Abschnitt „Schnittstelle" in `team/BRETT.txt`): wer sie
    /// aendert, aendert einen Vertrag.
    pub fn als_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("STATUS {}\n", self.status));
        s.push_str(&format!("VERBRAUCHT {}\n", self.verbraucht));
        if self.haltepunkte.is_empty() {
            s.push_str("HALTEPUNKTE -\n");
        } else {
            let h: Vec<String> = self
                .haltepunkte
                .iter()
                .map(|h| format!("{}+{}", h.name, h.versatz))
                .collect();
            s.push_str(&format!("HALTEPUNKTE {}\n", h.join(", ")));
        }
        s.push_str("RAHMEN (aeusserster zuerst)\n");
        if self.rahmen.is_empty() {
            s.push_str("  -\n");
        }
        for r in &self.rahmen {
            s.push_str(&format!(
                "  [{}] {} @{} (Tiefe {})\n",
                r.tiefe, r.name, r.versatz, r.tiefe
            ));
            if r.lokal.is_empty() {
                s.push_str("      lokal: -\n");
            } else {
                let l: Vec<String> = r
                    .lokal
                    .iter()
                    .map(|l| format!("{} ({}) = {}", l.name, l.typ, l.wert))
                    .collect();
                s.push_str(&format!("      lokal: {}\n", l.join(", ")));
            }
            if r.stapel.is_empty() {
                s.push_str("      stapel: -\n");
            } else {
                // Der Stapel liegt von unten nach oben; angezeigt wird er
                // wie im Werkzeug: DER OBENSTE zuerst.
                let oben: Vec<&str> = r.stapel.iter().rev().map(|w| w.as_str()).collect();
                s.push_str(&format!("      stapel: {}\n", oben.join(", ")));
            }
        }
        match &self.speicher_hinweis {
            Some(h) => s.push_str(&format!("SPEICHER {h}\n")),
            None if self.speicher.is_empty() => s.push_str("SPEICHER -\n"),
            None => {
                s.push_str("SPEICHER (adresse, byte[16], ascii)\n");
                for z in &self.speicher {
                    let mut hex: Vec<String> = z.werte.iter().map(|b| format!("{b:02x}")).collect();
                    while hex.len() < 16 {
                        hex.push(String::from("  "));
                    }
                    s.push_str(&format!(
                        "  {:08x}  {}  |{}|\n",
                        z.adresse,
                        hex.join(" "),
                        z.ascii
                    ));
                }
            }
        }
        s.push_str("WAT (aktuelle Funktion, \";; <- hier\" = naechster Befehl)\n");
        if self.wat.is_empty() {
            s.push_str("  -\n");
        } else {
            for zeile in self.wat.lines() {
                s.push_str(&format!("  {zeile}\n"));
            }
        }
        s
    }
}

/// Wunsch an die JS-Rolle: eine WASM-Funktion aus JS aufrufen. Wird hier
/// nicht gebraucht, aber der Haken-Typ ist derselbe wie im Store.
pub type Fremdfunktion = FremdFn;

#[cfg(test)]
mod einheit {
    use super::*;

    fn modul() -> std::rc::Rc<Modul> {
        let m = crate::wat::lies_text(
            "(module (memory 1) (global $g (mut i32) (i32.const 0)) \
             (func $add (param i32 i32) (result i32) local.get 0 local.get 1 i32.add) \
             (func (export \"main\") (result i32) (call $add (i32.const 20) (i32.const 22))))",
            &Grenzen::standard(),
        )
        .unwrap();
        std::rc::Rc::new(m)
    }

    /// M7: Spur, Haltepunkte, Disassembly sichern.
    #[test]
    fn spur_und_haltepunkte() {
        let m = modul();
        let g = Grenzen::standard();
        crate::pruefe::modul(&m, &g).expect("Modul muss gueltig sein");
        let mut lauf = Lauf::neu(g.clone());
        let inst = crate::lauf::instanziieren(&mut lauf, &m, &crate::js::ImportTabelle::default())
            .unwrap();
        let mut dbg = Debugger::neu(std::rc::Rc::new((*m).clone()), g);
        let r = dbg.ausfuehren(&mut lauf, &inst, "main", &[]).unwrap();
        assert_eq!(r, vec![Wert::I32(42)]);
        assert!(dbg.spur.len() >= 4, "Spur zu kurz: {}", dbg.spur.len());
        // Der erste Schritt ist der Eintritt in `main`.
        let erster = dbg.spur.schritte[0].clone();
        assert_eq!(erster.tiefe, 0);
        assert_eq!(erster.versatz, 0);
        // Haltepunkt auf (0,0) wird beim Vorwaertslaufen gefunden.
        dbg.haltepunkte.setze(erster.funktion, 0, true);
        assert!(dbg.haltepunkte.hat(erster.funktion, 0));
        let s = dbg.bis_haltepunkt().expect("Haltepunkt muss kommen");
        assert_eq!(s.versatz, 0);
        // Disassembly enthaelt den Funktionsnamen/Code.
        let text = dbg.disassembly(0);
        assert!(text.contains("i32.add"), "{text}");
    }

    /// W5/D11: der Debugger haelt einen LAUFENDEN Aufruf an — er fuehrt
    /// Schritt fuer Schritt aus, der Aufrufer sieht den Zustand und
    /// entscheidet, ob es weitergeht. Kein Neustart je Schritt.
    #[test]
    fn lauf_unterbrechen_und_weiter() {
        let m = modul();
        let g = Grenzen::standard();
        crate::pruefe::modul(&m, &g).expect("Modul muss gueltig sein");
        let mut lauf = Lauf::neu(g.clone());
        let inst = crate::lauf::instanziieren(&mut lauf, &m, &crate::js::ImportTabelle::default())
            .unwrap();
        let mut dbg = Debugger::neu(std::rc::Rc::new((*m).clone()), g.clone());
        // Sitzung starten: NICHTS gelaufen.
        let mut s = lauf.sitzung(&inst, "main", &[]).expect("Sitzung");
        assert!(!s.ist_fertig());
        let z0 = s.zustand(&lauf);
        assert_eq!(z0.aufrufstapel.len(), 1, "{z0:?}");
        // Schritt fuer Schritt bis zum Ende — der Aufruf wird NICHT neu
        // gestartet, der Zustand waechst weiter.
        let mut schritte = 0usize;
        while s.weiter(&mut lauf) {
            schritte += 1;
            assert!(schritte < 1000, "kein Fortschritt");
        }
        assert_eq!(s.ergebnis(), Some(&[Wert::I32(42)][..]), "Ergebnis");
        // Haltepunkt: anhalten OHNE den Schritt auszufuehren.
        let mut s2 = lauf.sitzung(&inst, "main", &[]).expect("Sitzung 2");
        let z = dbg
            .weiter_live(&mut s2, &mut lauf, |_f, _v| true)
            .expect("Zustand");
        // Stehengeblieben: der Stapel ist noch leer (vor dem ersten Befehl).
        assert_eq!(z.stapel.len(), 0, "{z:?}");
        assert!(!s2.ist_fertig());
        // Und jetzt laeuft er weiter.
        let v = s2.bis_ende(&mut lauf).expect("Ende");
        assert_eq!(v, vec![Wert::I32(42)]);
        // Haltepunktsuche in der laufenden Ausfuehrung.
        let mut s3 = lauf.sitzung(&inst, "main", &[]).expect("Sitzung 3");
        dbg.haltepunkte.setze(z.aufrufstapel[0].0, 2, true);
        let halt = dbg
            .bis_haltepunkt_live(&mut s3, &mut lauf, 1000)
            .expect("Haltepunkt");
        assert_eq!(
            halt.aufrufstapel.last().copied().map(|(_, v)| v),
            Some(2),
            "{halt:?}"
        );
    }

    #[test]
    fn grenzen_der_spur() {
        let mut s = Spur::neu(2);
        for i in 0..5 {
            s.push(Schritt {
                funktion: 0,
                versatz: i,
                tiefe: 0,
                lokale: vec![],
                stapel: vec![],
                ergebnis: vec![],
                speicher_seiten: 0,
                verbraucht: 0,
            });
        }
        assert_eq!(s.len(), 2);
        assert!(s.abgeschnitten);
    }
}
