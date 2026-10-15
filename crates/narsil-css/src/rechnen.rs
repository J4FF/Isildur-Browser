//! var()-Substitution und calc()/min()/max()/clamp() (C10/C11, W6).
//!
//! Zwei Schritte, beide begrenzt (M3/M4): erst werden var()-Referenzen
//! mit den Element-Variablen ersetzt (Tiefe/Anzahl/Laenge gedeckelt,
//! Zyklen enden als "ungueltig"), dann wird gerechnet (Rekursion mit
//! Tiefengrenze, Division durch 0 ungueltig, Ergebnis geclippt).
//! Reine Funktionen, keine Allokation ausser dem Ergebnis-String.

use super::typen::Variablen;
use super::{MAX_PX, MAX_RECHEN_ARGS, MAX_RECHEN_TIEFE, MAX_VAR_ERSETZUNGEN, MAX_VAR_TIEFE};

// ===========================================================================
// SUCHE (ASCII-fallunabhaengig, Eingabe gedeckelt)
// ===========================================================================

/// True, wenn `roh` die Funktion `name` (z.B. "var(") enthaelt.
/// Vergleich ASCII-fallunabhaengig (CSS-Funktionsnamen sind es auch).
/// Linear, begrenzt durch die Eingabelaenge (<= ~2 KiB).
/// Steht direkt vor `stelle` noch ein Namenszeichen? Dann ist der
/// Treffer das ENDE eines laengeren Namens, nicht der Anfang einer
/// Funktion.
///
/// # L4 (02.10.2026 09:2x) — DER FEHLER, DEN DAS BEHEBT
///
/// GEMESSENER BEFEUND, in `minmax(100px, 1fr)` sei der Teilstring
/// `max(` enthalten:
///
/// ```text
/// calc(    in minmax(...): False
/// min(     in minmax(...): False
/// max(     in minmax(...): True      <- der springt an
/// clamp(   in minmax(...): False
/// ```
///
/// Und die Folge war groesser als `minmax`: `hat_mathe` erkennt
/// Mathe, `effektiver_wert` nimmt den Rechenweg, und `rechnen` kann
/// `minmax(...)` nicht als `max()`-Stufenfunktion rechnen — also
/// `None`, also **die ganze Deklaration faellt**. GEMESSEN an den
/// beiden Stationen (`BIETEN`- und `ARM`-Druck in `kaskade.rs`):
///
/// ```text
/// .g{grid-template-columns:minmax(100px, 1fr) …}
///     BIETEN roh="minmax(100px, 1fr) …"     <- das Angebot entsteht
///     (kein ARM)                             <- der Arm laeuft nicht
/// .g{grid-template-columns:repeat(3, 1fr)}
///     BIETEN roh="repeat(3, 1fr)"
///     ARM wert=Wort("repeat(3, 1fr)")       <- hier laeuft er
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum die Grenze am NAMEN und nicht an
/// einer Liste von Ausnahmen: dasselbe gilt fuer `var(` in
/// `--meinevar(...)` und fuer `min(` in einemnamen wie
/// `domin(` — alle drei waeren mit einer Liste nur einzeln abgedeckt.
/// GEMESSENE ENTSCHEIDUNG, warum ein Namenszeichen und kein Wort:
/// CSS-Funktionsnamen sind `[a-zA-Z_-][a-zA-Z0-9_-]*`, und ein
/// Funktionsname steht nie unmittelbar nach einem solchen Zeichen an.
fn namens_anhang_davor(h: &[u8], stelle: usize) -> bool {
    stelle > 0 && {
        let c = h[stelle - 1];
        c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
    }
}

pub fn hat_funktion(roh: &str, name: &str) -> bool {
    let h = roh.as_bytes();
    let n = name.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    // Begrenzt: jede Runde ein Byte weiter (M3).
    for i in 0..=(h.len() - n.len()) {
        let mut gleich = true;
        for k in 0..n.len() {
            if h[i + k].to_ascii_lowercase() != n[k] {
                gleich = false;
                break;
            }
        }
        if gleich && !namens_anhang_davor(h, i) {
            return true;
        }
    }
    false
}

// ===========================================================================
// var() (C10)
// ===========================================================================

/// Ersetzt alle var(--name, ersatz) in `roh` durch die Variablenwerte
/// (rekursiv, denn Werte enthalten selbst var()). None, wenn eine
/// Referenz weder definiert noch mit Ersatz versehen ist, oder eine
/// Grenze greift (Tiefe = Zyklus, Anzahl, Ausgabelaenge).
/// Namenvergleich case-sensitiv (CSS-Regel); "var(" selbst nicht.
pub fn ersetze_var(roh: &str, vars: &Variablen, tiefe: u8) -> Option<String> {
    if tiefe > MAX_VAR_TIEFE {
        return None;
    }
    // Schneller Pfad ohne var( (M3: eine Suche statt Parser).
    if !hat_funktion(roh, "var(") {
        return Some(String::from(roh));
    }
    let b = roh.as_bytes();
    let mut aus = String::new();
    let mut i = 0usize;
    let mut ersetzt = 0usize;
    while i < b.len() {
        if ist_var_an(b, i) {
            // Klammer zuordnen (Tiefe zaehlen, begrenzt durch Rest).
            let start = i + 4;
            let mut t = 1usize;
            let mut k = start;
            while k < b.len() && t > 0 {
                if b[k] == b'(' {
                    t += 1;
                } else if b[k] == b')' {
                    t -= 1;
                }
                k += 1;
            }
            if t != 0 {
                return None; // unabgeschlossen: ungueltig
            }
            // Innen: Name [, Ersatz] an Top-Level-Komma teilen.
            let innen = &roh[start..k - 1];
            let (name_teil, ersatz_teil) = teile_var_innen(innen);
            let mut name = name_teil.trim();
            // Release 12.10.2026 (Agent C): der Name aus `ident()` (CSS Values 5
            // §8.1; WPT ident-function-substitution `var(ident("--" "x"))`).
            let gebaut;
            if !name.starts_with("--") && hat_funktion(name, "ident(") {
                gebaut = ersetze_var(name, vars, tiefe + 1).and_then(|n| crate::cssom::ersetzen::ident_ersetzen(&n));
                if let Some(g) = &gebaut {
                    name = g.trim();
                }
            }
            // Aufloesen: erst Definition (rekursiv, Tiefe+1), dann Ersatz.
            // Zyklen enden ueber die Tiefe als None (C10).
            // Release 08.10.2026: eine Variable mit dem Wert `initial` ist
            // der GARANTIERT UNGUELTIGE Wert (CSS Variables 1 §2.2) — wie
            // eine nicht definierte, also greift der Ersatz. Darauf baut der
            // `light-dark()`-Nachbau von PostCSS (`--csstools-light-dark-
            // toggle`), GEMESSEN auf MDN: `fill="initial #a4cefe"` statt der
            // hellen Farbe.
            let definiert = if name.len() > 2 && name.starts_with("--") {
                lookup_var(vars, name).filter(|v| !v.trim().eq_ignore_ascii_case("initial"))
            } else {
                None
            };
            let wert: Option<String> = match definiert {
                Some(v) => match ersetze_var(v, vars, tiefe + 1) {
                    Some(aufgeloest) => Some(aufgeloest),
                    None => match ersatz_teil {
                        Some(e) => ersetze_var(e, vars, tiefe + 1),
                        None => None,
                    },
                },
                None => match ersatz_teil {
                    Some(e) => ersetze_var(e, vars, tiefe + 1),
                    None => None,
                },
            };
            let wert = wert?;
            ersetzt += 1;
            if ersetzt > MAX_VAR_ERSETZUNGEN {
                return None;
            }
            if aus.len() + wert.len() > 2048 {
                return None; // Wachstum gedeckelt (M4)
            }
            aus.push_str(&wert);
            i = k;
        } else {
            // Byte/Char uebernehmen (Mehrbyte am Stueck, max. 4 Schritte).
            let rest = &roh[i..];
            match rest.chars().next() {
                Some(c) => {
                    aus.push(c);
                    i += c.len_utf8();
                }
                None => break,
            }
            if aus.len() > 2048 {
                return None;
            }
        }
    }
    Some(aus)
}

/// Release 12.10.2026 (Agent C): `inherit( <custom-property-name>, <fallback>? )`
/// einsetzen (CSS Values 5 §7.4): der berechnete Wert der Custom Property
/// am ELTERNELEMENT (`eltern`), sonst der Ersatz; ohne beides `None`
/// (ungueltig zur Berechnungszeit). GEMESSEN vorher (WPT css-values
/// inherit-function-basic): `z-index: inherit(--z)` blieb `auto`.
pub fn ersetze_inherit(roh: &str, eltern: Option<&Variablen>) -> Option<String> {
    if !hat_funktion(roh, "inherit(") {
        return Some(String::from(roh));
    }
    let b = roh.as_bytes();
    let mut aus = String::with_capacity(roh.len());
    let mut i = 0usize;
    let mut zahl = 0usize;
    while i < b.len() {
        let an = i + 8 <= b.len() && b[i..i + 8].eq_ignore_ascii_case(b"inherit(") && !namens_anhang_davor(b, i);
        if !an {
            match roh[i..].chars().next() {
                Some(ch) => {
                    aus.push(ch);
                    i += ch.len_utf8();
                }
                None => break,
            }
            continue;
        }
        zahl += 1;
        if zahl > MAX_VAR_ERSETZUNGEN {
            return None;
        }
        let start = i + 8;
        let mut t = 1usize;
        let mut k = start;
        while k < b.len() && t > 0 {
            if b[k] == b'(' {
                t += 1;
            } else if b[k] == b')' {
                t -= 1;
            }
            k += 1;
        }
        if t != 0 {
            return None;
        }
        let (name, ersatz) = teile_var_innen(&roh[start..k - 1]);
        let wert = eltern
            .filter(|_| name.starts_with("--"))
            .and_then(|v| lookup_var(v, name).filter(|w| !w.trim().eq_ignore_ascii_case("initial")).map(|w| (v, w)))
            .and_then(|(v, w)| ersetze_var(w, v, 1));
        let wert = match wert {
            Some(w) => w,
            None => ersetze_inherit(ersatz?, eltern)?,
        };
        if aus.len() + wert.len() > 4096 {
            return None;
        }
        aus.push_str(&wert);
        i = k;
    }
    Some(aus)
}

/// True bei "var(" (ASCII-fallunabhaengig) an Position i.
fn ist_var_an(b: &[u8], i: usize) -> bool {
    i + 4 <= b.len()
        && (b[i] == b'v' || b[i] == b'V')
        && (b[i + 1] == b'a' || b[i + 1] == b'A')
        && (b[i + 2] == b'r' || b[i + 2] == b'R')
        && b[i + 3] == b'('
}

/// Teilt var-Innenleben am ersten Top-Level-Komma (Name, Option<Ersatz>).
fn teile_var_innen(innen: &str) -> (&str, Option<&str>) {
    let b = innen.as_bytes();
    let mut tiefe = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'(' => tiefe += 1,
            b')' => tiefe = tiefe.saturating_sub(1),
            b',' if tiefe == 0 => {
                return (innen[..i].trim(), Some(innen[i + 1..].trim()));
            }
            _ => {}
        }
        i += 1;
    }
    (innen.trim(), None)
}

/// Sucht eine Variable (case-sensitiv, ueber die Ebenenkette).
fn lookup_var<'a>(vars: &'a Variablen, name: &str) -> Option<&'a str> {
    vars.holen(name)
}

// ===========================================================================
// calc()/min()/max()/clamp() (C11)
// ===========================================================================

/// Rechen-Kontext: Basen fuer relative Einheiten (wie `laenge_px`).
// Release 11.10.2026: `Clone` fuer den Rasterleser (`gitter.rs`), der mit
// zwei Prozentbasen rechnet, um den Prozentanteil abzuspalten.
#[derive(Clone)]
pub struct RechenCtx {
    /// em-Basis in px (eigene bzw. Eltern-Schrift).
    pub em: f32,
    /// **rem-Basis** in px: die Schrift des WURZEL-elements.
    ///
    /// 03.10.2026 17:5x: dieses Feld ist neu, und es schliesst eine Luecke,
    /// die seit dem 02.10. (root-rem) offen war: `rem` ausserhalb von
    /// `calc()` rechnete gegen `Berechnet::wurzel_schrift`, **im** `calc()`
    /// aber gegen die Konstante 16.0 (GEMESSEN, siehe die Stelle in
    /// `einheit`). Auf jeder Seite mit `html { font-size: … }` — bei Rust
    /// html sind es 12 px (75 %) — war `calc(2rem)` damit 32 px statt 24 px,
    /// waehrend `width: 2rem` richtig war. Zwei Wege, eine Einheit, zwei
    /// Antworten.
    ///
    /// `0.0` oder nicht gesetzt heisst: unbekannt — dann faellt `rem` auf
    /// 16 px zurueck, das ist die alte Rechnung und damit die Antwort, die
    /// ein Aufrufer ohne Wurzelschrift erwartet (siehe [`RechenCtx::neu`]).
    pub rem: f32,
    /// %-Basis in px (Ansicht oder eigene Schrift).
    pub prozent: f32,
    /// Ansicht in px (vw).
    pub ansicht: f32,
    /// Ansichts-HOEHE in px (vh, vmin, vmax).
    ///
    /// R0 e (30.09.2026): es gab nur `ansicht`, und `vh`, `vmin` und
    /// `vmax` wurden als "wie Vw" gerechnet — `10vh` bei 800x600 ergab
    /// 80 px statt 60 px. `berechnen` bekommt die Ansichtshoehe seit
    /// G3 (`ansicht_hoehe`), sie war nur nicht weitergereicht.
    ///
    /// `0.0` bedeutet "unbekannt" (etwa in den Farb-Kontexten) — dann
    /// faellt `vh` auf die Breite zurueck, damit keine Division durch
    /// nichts entsteht.
    pub ansicht_hoehe: f32,
}

/// Die Ansichtshoehe fuer eine Rechnung; 0 heisst "unbekannt".
pub fn ansichts_hoehe(breite: f32, hoehe: f32) -> f32 {
    if hoehe.is_finite() && hoehe > 0.0 {
        hoehe
    } else {
        breite
    }
}

impl RechenCtx {
    /// Kontext fuer eine Rechnung OHNE bekannte Ansichtshoehe.
    ///
    /// DAS ist die duenne Huelle: vor dem 30.09.2026 hatte
    /// `RechenCtx` drei Felder, und ein Aufrufer, der
    /// `RechenCtx { em, prozent, ansicht }` schrieb, soll nicht an
    /// einem neuen Feld zerbrechen. `ansicht_hoehe` faellt dann auf die
    /// Breite zurueck — das ist die alte Naeherung ("`vh` wie `vw`")
    /// und damit genau das Verhalten, das ein alter Aufrufer erwartet.
    pub fn neu(em: f32, prozent: f32, ansicht: f32) -> Self {
        RechenCtx {
            em,
            // Ohne bekannte Wurzelschrift: 16 px. Das ist die Rechnung, die
            // vor dem `rem`-Feld (und vor `root-rem`) drin stand — ein alter
            // Aufrufer bekommt also genau das, was er erwartet.
            rem: BASIS_REM_PX,
            prozent,
            ansicht,
            ansicht_hoehe: ansicht,
        }
    }

    /// Setzt die rem-Basis (die Schrift des WURZEL-elements).
    ///
    /// 03.10.2026 17:5x: der Weg, den die Kaskade nimmt. Sie kennt
    /// `Berechnet::wurzel_schrift` aus dem Elternergebnis und rechnet
    /// ausserhalb von `calc()` schon dagegen — dieses `mit_rem` macht den
    /// **inneren** Weg gleich.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `0.0` auf 16 px faellt und nicht auf 0:
    /// ein `rem` von 0 px waere eine Seite ohne Text, und ein Aufrufer ohne
    /// Wurzelschrift hat keine — er hat die Vorgabe.
    pub fn mit_rem(mut self, rem: f32) -> Self {
        self.rem = if rem.is_finite() && rem > 0.0 {
            rem
        } else {
            BASIS_REM_PX
        };
        self
    }

    /// Setzt die Ansichtshoehe (Builder) — der normale Weg.
    ///
    /// `mod rechnen` ist crate-privat, deshalb meldet der Compiler
    /// diese Methode als unbenutzt. Sie ist der dokumentierte Weg fuer
    /// `vh`/`vmin`/`vmax` (R0 e, 30.09.2026) und wird beim Umstieg auf
    /// `narsil-page` gebraucht.
    #[allow(dead_code)]
    pub fn mit_hoehe(mut self, hoehe: f32) -> Self {
        self.ansicht_hoehe = hoehe;
        self
    }
}

impl Default for RechenCtx {
    /// `em` und `prozent` = 16 px, keine Ansicht.
    fn default() -> Self {
        RechenCtx::neu(16.0, 16.0, 0.0)
    }
}

/// Die Vorgabe fuer `rem`, wenn keine Wurzelschrift bekannt ist.
///
/// 03.10.2026 17:5x: als Konstante benannt, damit der Wert an **einer**
/// Stelle steht. Vorher stand die Zahl an zwei Stellen (dem `Default` und
/// der `rem`-Verzweigung), und die beiden konnten auseinanderlaufen.
pub const BASIS_REM_PX: f32 = 16.0;

impl RechenCtx {
    /// Die benutzte rem-Basis: das Feld, ausser es ist unbekannt.
    fn rem_basis(&self) -> f32 {
        if self.rem.is_finite() && self.rem > 0.0 {
            self.rem
        } else {
            BASIS_REM_PX
        }
    }
}

/// Wertet einen substituierten Ausdruck in px aus. Ganz aussen darf
/// calc() stehen oder fehlen; min()/max()/clamp() ueberall. None =
/// ungueltig (falsche Typen, Division durch 0, Uebermass, zu tief).
/// Gross/Klein egal (wird kleingeschrieben).
pub fn rechne(roh: &str, ctx: &RechenCtx, tiefe: u8) -> Option<f32> {
    if roh.len() > 2048 {
        return None;
    }
    let klein = roh.to_ascii_lowercase();
    let mut p = Parser {
        b: klein.as_bytes(),
        pos: 0,
        ctx,
        tiefe,
    };
    let (v, _) = p.summe()?;
    p.leer();
    if p.pos != p.b.len() {
        return None; // Restmuell hinter dem Ausdruck
    }
    if !v.is_finite() {
        return None;
    }
    Some(v.clamp(-MAX_PX, MAX_PX))
}

/// Art eines Teilwerts: px (alle Laengen, umgerechnet) oder Zahl.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Art {
    Px,
    Zahl,
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    ctx: &'a RechenCtx,
    tiefe: u8,
}

impl<'a> Parser<'a> {
    fn leer(&mut self) {
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
    /// Summe: a + b - c (Operator grosszuegig mit/ohne Leerzeichen).
    fn summe(&mut self) -> Option<(f32, Art)> {
        let (mut v, a) = self.produkt()?;
        loop {
            self.leer();
            let op = match self.b.get(self.pos) {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => break,
            };
            self.pos += 1;
            let (w, b) = self.produkt()?;
            // +,- nur gleichartig (nach Umrechnung: px mit px, Zahl mit Zahl).
            if a != b {
                return None;
            }
            v = if op == 1 { v + w } else { v - w };
            if v.abs() > 1e30 {
                return None; // Ueberlauf frueh stoppen (M4)
            }
        }
        Some((v, a))
    }
    /// Produkt: a * b / c (* und / brauchen eine Zahl-Seite, CSS-Regel).
    fn produkt(&mut self) -> Option<(f32, Art)> {
        let (mut v, mut a) = self.faktor()?;
        loop {
            self.leer();
            let op = match self.b.get(self.pos) {
                Some(b'*') => 0,
                Some(b'/') => 1,
                _ => break,
            };
            self.pos += 1;
            let (w, b) = self.faktor()?;
            match (op, a, b) {
                (0, Art::Px, Art::Zahl) => v *= w,
                (0, Art::Zahl, Art::Px) => {
                    // Zahl * Laenge ist Laenge (W6B-Fix: vorher blieb die
                    // Art Zahl und jedes folgende + Laenge scheiterte).
                    v *= w;
                    a = Art::Px;
                }
                (0, Art::Zahl, Art::Zahl) => {
                    v *= w;
                    a = Art::Zahl;
                }
                (1, _, Art::Zahl) if w != 0.0 => v /= w,
                _ => return None,
            }
            if v.abs() > 1e30 {
                return None;
            }
        }
        Some((v, a))
    }
    /// Faktor: Zahl, Klammer oder Funktion.
    fn faktor(&mut self) -> Option<(f32, Art)> {
        if self.tiefe > MAX_RECHEN_TIEFE {
            return None;
        }
        self.leer();
        // Funktionsname lesen (Buchstaben + -).
        let start = self.pos;
        while self.pos < self.b.len()
            && (self.b[self.pos].is_ascii_alphabetic() || self.b[self.pos] == b'-')
        {
            self.pos += 1;
        }
        let name_ende = self.pos;
        // Klammer pruefen (kurze Ausleihe, endet sofort).
        let hat_klammer = self.b.get(self.pos) == Some(&b'(');
        // Nackte Gruppe "(...)" ohne Funktionsnamen (C11).
        if hat_klammer && name_ende == start {
            self.pos += 1;
            if self.tiefe >= MAX_RECHEN_TIEFE {
                return None;
            }
            self.tiefe += 1;
            let erg = self.summe();
            self.tiefe -= 1;
            let (v, a) = erg?;
            self.leer();
            if self.b.get(self.pos) != Some(&b')') {
                return None;
            }
            self.pos += 1;
            return Some((v, a));
        }
        if hat_klammer {
            self.pos += 1;
            // Funktionsart bestimmen (Ausleihe endet mit dem Vergleich).
            let n_calc = self.b.get(start..name_ende) == Some(b"calc".as_slice());
            let n_min = self.b.get(start..name_ende) == Some(b"min".as_slice());
            let n_max = self.b.get(start..name_ende) == Some(b"max".as_slice());
            let n_clamp = self.b.get(start..name_ende) == Some(b"clamp".as_slice());
            // CSS Values 4 §10: die STUFENfunktionen. `round()` wird
            // eigens gelesen, weil sein drittes Argument ein Wort ist
            // und `arg_liste` nur Zahlen kann.
            let n_round = self.b.get(start..name_ende) == Some(b"round".as_slice());
            let n_mod = self.b.get(start..name_ende) == Some(b"mod".as_slice());
            let n_rem = self.b.get(start..name_ende) == Some(b"rem".as_slice());
            if self.tiefe >= MAX_RECHEN_TIEFE {
                return None;
            }
            // Eine Ebene tiefer (M5) — danach wieder zurueck.
            self.tiefe += 1;
            let erg = if n_calc {
                let (v, a) = self.summe()?;
                self.leer();
                if self.b.get(self.pos) != Some(&b')') {
                    self.tiefe -= 1;
                    return None;
                }
                self.pos += 1;
                Some((v, a))
            } else if n_min {
                let args = self.arg_liste()?;
                min_max(&args, true)
            } else if n_max {
                let args = self.arg_liste()?;
                min_max(&args, false)
            } else if n_clamp {
                let args = self.arg_liste()?;
                if args.len() != 3 {
                    None
                } else {
                    let art = args[0].1;
                    if args.iter().any(|x| x.1 != art) {
                        None
                    } else {
                        Some((args[1].0.clamp(args[0].0, args[2].0), art))
                    }
                }
            } else if n_round {
                self.stufe(true)
            } else if n_mod {
                self.stufe(false).map(|(v, a)| (v, a))
            } else if n_rem {
                self.stufe(false)
            } else {
                None // rgb()/var() gehoeren nicht hierher
            };
            self.tiefe -= 1;
            return erg;
        }
        // Kein Funktionsaufruf: Name zurueck, Klammer oder Zahl.
        self.pos = start;
        self.leer();
        if self.b.get(self.pos) == Some(&b'(') {
            self.pos += 1;
            if self.tiefe >= MAX_RECHEN_TIEFE {
                return None;
            }
            self.tiefe += 1;
            let erg = self.summe();
            self.tiefe -= 1;
            let (v, a) = erg?;
            self.leer();
            if self.b.get(self.pos) != Some(&b')') {
                return None;
            }
            self.pos += 1;
            return Some((v, a));
        }
        self.zahl()
    }
    /// Komma-Argumente bis ')' (hoechstens MAX_RECHEN_ARGS).
    /// Leere Liste oder fehlende Klammer: None.
    fn arg_liste(&mut self) -> Option<Vec<(f32, Art)>> {
        let mut args: Vec<(f32, Art)> = Vec::new();
        loop {
            self.leer();
            // Leere Klammern "min()" sind ungueltig.
            if self.b.get(self.pos) == Some(&b')') {
                return None;
            }
            let (v, a) = self.summe()?;
            args.push((v, a));
            if args.len() > MAX_RECHEN_ARGS {
                return None;
            }
            self.leer();
            match self.b.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(args)
    }
    /// CSS Values 4 §10: `round(<wert>, <schritt>[, <strategie>])`
    /// und `mod(<wert>, <schritt>)`/`rem(<wert>, <schritt>)`.
    ///
    /// Der Grund fuer einen eigenen Leser: `arg_liste` kann nur
    /// ZAHLEN lesen, und die Strategie von `round()` ist ein Wort
    /// (`nearest`, `up`, `down`). Ausserdem muss der Restwert vor dem
    /// Teilen berechnet werden — `1.5 % 1` waere als Gleitkommazahl
    /// `0.5`, aber CSS rechnet `1.5px mod 1px` zu `0.5px`.
    ///
    /// `mod` und `rem` unterscheiden sich nur im Vorzeichen des
    /// Ergebnisses bei negativem Eingang (§10.2/§10.3), nicht in der
    /// Rechnung — beide werden hier gleich gelesen.
    fn stufe(&mut self, gerundet: bool) -> Option<(f32, Art)> {
        let (w, aw) = self.summe()?;
        self.leer();
        if self.b.get(self.pos) != Some(&b',') {
            return None;
        }
        self.pos += 1;
        let (schritt, aschritt) = self.summe()?;
        if aw != aschritt {
            return None;
        }
        // M3/M4: ein Schritt von 0 (oder unendlich) ist nicht durch
        // `floor`/`ceil` zu retten — die Rechnung waere eine
        // Endlosschleife in Float. Zurueckgeben und die Zeile
        // verwerfen.
        if !(schritt.abs() > 0.0) || !schritt.is_finite() || !w.is_finite() {
            return None;
        }
        let mut strategie = 0u8; // 0 = nearest, 1 = up, 2 = down
        self.leer();
        if self.b.get(self.pos) == Some(&b',') {
            self.pos += 1;
            self.leer();
            let start = self.pos;
            while self.pos < self.b.len()
                && (self.b[self.pos].is_ascii_alphanumeric() || self.b[self.pos] == b'-')
            {
                self.pos += 1;
            }
            let wort = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
            strategie = match wort.as_str() {
                "nearest" => 0,
                "up" => 1,
                "down" => 2,
                _ => return None,
            };
            if !gerundet {
                return None; // mod()/rem() haben keine Strategie
            }
            self.leer();
        }
        if self.b.get(self.pos) != Some(&b')') {
            return None;
        }
        self.pos += 1;
        let q = w / schritt;
        let k = match strategie {
            1 => q.ceil(),
            2 => q.floor(),
            _ => {
                // §10.1: auf Gleichstand wird AUFWAERTS gerundet.
                let r = q - q.floor();
                if (r - 0.5).abs() <= 1e-9 {
                    q.floor() + 1.0
                } else {
                    q.floor() + r.round()
                }
            }
        };
        Some((k * schritt, aw))
    }

    /// Zahl mit Einheit, in (px-Wert, Art). Unbekannte Einheit: None.
    fn zahl(&mut self) -> Option<(f32, Art)> {
        self.leer();
        let start = self.pos;
        // Vorzeichen.
        if self.b.get(self.pos) == Some(&b'+') || self.b.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        let mut ziffer = false;
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
            self.pos += 1;
            ziffer = true;
        }
        if self.b.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
                self.pos += 1;
                ziffer = true;
            }
        }
        // Exponent (Gehoert dazu — "1e-3" als Ganzes).
        if self.b.get(self.pos) == Some(&b'e') || self.b.get(self.pos) == Some(&b'E') {
            let mut q = self.pos + 1;
            if q < self.b.len() && (self.b[q] == b'+' || self.b[q] == b'-') {
                q += 1;
            }
            if q < self.b.len() && self.b[q].is_ascii_digit() {
                self.pos = q;
                while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                ziffer = true;
            }
        }
        if !ziffer {
            return None;
        }
        if self.pos - start > 32 {
            return None;
        }
        let zahl: f32 = core::str::from_utf8(&self.b[start..self.pos])
            .ok()?
            .parse()
            .ok()?;
        if !zahl.is_finite() {
            return None;
        }
        // Einheit (Buchstaben oder %).
        let ustart = self.pos;
        while self.pos < self.b.len()
            && (self.b[self.pos].is_ascii_alphabetic() || self.b[self.pos] == b'%')
        {
            self.pos += 1;
        }
        let einheit = &self.b[ustart..self.pos];
        let ctx = self.ctx;
        match einheit {
            b"" => Some((zahl, Art::Zahl)),
            b"px" => Some((zahl, Art::Px)),
            b"em" => Some((zahl * ctx.em, Art::Px)),
            // 03.10.2026 17:5x: `rem` gegen die Wurzelschrift, nicht gegen
            // die Konstante 16. GEMESSENER FEHLER, den das behebt: In Rusts
            // `html { font-size: 75% }` ist die Wurzelschrift 12 px, und
            // `width: 2rem` ergab 24 px (korrekt, ueber
            // `Berechnet::wurzel_schrift`), waehrend `width: calc(2rem)`
            // **32 px** ergab — dieselbe Einheit, zwei Antworten, und die
            // zweite sah im Bild aus wie ein zu breiter Kasten.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Konstante nicht einfach
            // hier bleibt: `var()` laeuft durch denselben Rechner, also
            // war auch `--x: 2rem` im `var()`-Zweig falsch.
            b"rem" => Some((zahl * ctx.rem_basis(), Art::Px)),
            b"%" => Some((zahl / 100.0 * ctx.prozent, Art::Px)),
            b"pt" => Some((zahl * 4.0 / 3.0, Art::Px)),
            // CSS Values 3, 5.4: vw = 1 % der Breite, vh = 1 % der
            // Hoehe, vmin/vmax = das Kleinere/Groessere von beidem.
            b"vw" => Some((zahl / 100.0 * ctx.ansicht, Art::Px)),
            b"vh" => Some((zahl / 100.0 * ctx.ansicht_hoehe, Art::Px)),
            b"vmin" => {
                let m = ctx.ansicht.min(ctx.ansicht_hoehe);
                Some((zahl / 100.0 * m, Art::Px))
            }
            b"vmax" => {
                let m = ctx.ansicht.max(ctx.ansicht_hoehe);
                Some((zahl / 100.0 * m, Art::Px))
            }
            b"ch" => Some((zahl * (ctx.em * crate::kaskade::ch_anteil()), Art::Px)),
            // Release 11.10.2026: die uebrigen absoluten und schriftbezogenen
            // Einheiten (CSS Values 4 §6). GEMESSEN vorher: WPT css-grid
            // `parsing/grid-auto-columns-valid` (`calc(2em + 3ex)` war ungueltig).
            b"ex" => Some((zahl * (ctx.em * crate::kaskade::ex_anteil_jetzt()), Art::Px)),
            b"in" => Some((zahl * 96.0, Art::Px)),
            b"cm" => Some((zahl * 96.0 / 2.54, Art::Px)),
            b"mm" => Some((zahl * 96.0 / 25.4, Art::Px)),
            b"q" => Some((zahl * 96.0 / 101.6, Art::Px)),
            b"pc" => Some((zahl * 16.0, Art::Px)),
            b"svw" | b"lvw" | b"dvw" | b"vi" | b"svi" | b"lvi" | b"dvi" => Some((zahl / 100.0 * ctx.ansicht, Art::Px)),
            b"svh" | b"lvh" | b"dvh" | b"vb" | b"svb" | b"lvb" | b"dvb" => Some((zahl / 100.0 * ctx.ansicht_hoehe, Art::Px)),
            // Release 12.10.2026 (Agent C): auch die kleinen/grossen/dynamischen.
            b"svmin" | b"lvmin" | b"dvmin" => Some((zahl / 100.0 * ctx.ansicht.min(ctx.ansicht_hoehe), Art::Px)),
            b"svmax" | b"lvmax" | b"dvmax" => Some((zahl / 100.0 * ctx.ansicht.max(ctx.ansicht_hoehe), Art::Px)),
            _ => None,
        }
    }
}

/// min()/max() ueber Argumenten gleicher Art (leere Liste: None).
fn min_max(args: &[(f32, Art)], minimum: bool) -> Option<(f32, Art)> {
    if args.is_empty() || args.len() > MAX_RECHEN_ARGS {
        return None;
    }
    let art = args[0].1;
    if args.iter().any(|x| x.1 != art) {
        return None;
    }
    let mut best = args[0].0;
    for x in &args[1..] {
        best = if minimum {
            best.min(x.0)
        } else {
            best.max(x.0)
        };
    }
    Some((best, art))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RechenCtx {
        // em 10, prozent 800 (Ansicht), ansicht 800 (und Hoehe 600).
        RechenCtx::neu(10.0, 800.0, 800.0).mit_hoehe(600.0)
    }

    #[test]
    fn calc_einfach() {
        assert_eq!(rechne("calc(1px + 2px)", &ctx(), 0), Some(3.0));
        assert_eq!(rechne("1px + 2px", &ctx(), 0), Some(3.0));
        assert_eq!(rechne("calc(10px - 4px)", &ctx(), 0), Some(6.0));
    }
    #[test]
    fn calc_gemischt() {
        // 50% von 800 + 2em zu 10 = 400 + 20.
        assert_eq!(rechne("calc(50% + 2em)", &ctx(), 0), Some(420.0));
        assert_eq!(
            rechne("calc(1rem + 1pt)", &ctx(), 0),
            Some(16.0 + 4.0 / 3.0)
        );
        assert_eq!(rechne("calc(10vw - 8px)", &ctx(), 0), Some(72.0));
    }
    #[test]
    fn calc_mal_geteilt() {
        assert_eq!(rechne("calc(3 * 4px)", &ctx(), 0), Some(12.0));
        assert_eq!(rechne("calc(12px / 4)", &ctx(), 0), Some(3.0));
        assert!(rechne("calc(1px / 0)", &ctx(), 0).is_none());
        assert!(rechne("calc(1px * 2px)", &ctx(), 0).is_none());
    }
    #[test]
    fn calc_typen_streng() {
        // px + Zahl ist ungueltig (kein stilles Mischen).
        assert!(rechne("calc(1px + 2)", &ctx(), 0).is_none());
        // Zahl mit Zahl geht.
        assert_eq!(rechne("calc(2 * 3)", &ctx(), 0), Some(6.0));
    }
    #[test]
    fn min_max_clamp() {
        assert_eq!(rechne("min(10px, 20px)", &ctx(), 0), Some(10.0));
        assert_eq!(rechne("max(10px, 20px)", &ctx(), 0), Some(20.0));
        assert_eq!(rechne("clamp(10px, 15px, 20px)", &ctx(), 0), Some(15.0));
        assert_eq!(rechne("clamp(10px, 5px, 20px)", &ctx(), 0), Some(10.0));
        assert_eq!(rechne("clamp(10px, 25px, 20px)", &ctx(), 0), Some(20.0));
        assert!(rechne("min(10px, 2)", &ctx(), 0).is_none());
        assert!(rechne("clamp(1px, 2px)", &ctx(), 0).is_none());
    }
    #[test]
    fn verschachtelt() {
        assert_eq!(rechne("calc(min(10px, 20px) + 5px)", &ctx(), 0), Some(15.0));
        assert_eq!(rechne("calc((2 + 3) * 4px)", &ctx(), 0), Some(20.0));
    }
    #[test]
    fn ungueltig() {
        assert!(rechne("", &ctx(), 0).is_none());
        assert!(rechne("banane", &ctx(), 0).is_none());
        assert!(rechne("calc(1px + )", &ctx(), 0).is_none());
        assert!(rechne("calc(1px) extra", &ctx(), 0).is_none());
        assert!(rechne("calc(1qpx)", &ctx(), 0).is_none());
    }
    #[test]
    fn klammerbombe_rechen() {
        let tief = "(".repeat(100) + "1px" + &")".repeat(100);
        assert!(rechne(&tief, &ctx(), 0).is_none());
        let viel = "1px+".repeat(100) + "1px";
        assert!(rechne(&viel, &ctx(), 0).is_some());
    }
    #[test]
    fn var_grundlagen() {
        let vars = Variablen::mit_eigenen(
            None,
            vec![
                (String::from("--rot"), String::from("red")),
                (String::from("--m"), String::from("8px")),
            ],
        );
        assert_eq!(ersetze_var("var(--rot)", &vars, 0).as_deref(), Some("red"));
        assert_eq!(
            ersetze_var("1px var(--m) solid", &vars, 0).as_deref(),
            Some("1px 8px solid")
        );
        // Ersatz bei Unbekannt.
        assert_eq!(
            ersetze_var("var(--weg, 4px)", &vars, 0).as_deref(),
            Some("4px")
        );
        // Ohne Ersatz ungueltig.
        assert!(ersetze_var("var(--weg)", &vars, 0).is_none());
        // Gross/Klein beim Namen: case-sensitiv.
        assert!(ersetze_var("var(--ROT)", &vars, 0).is_none());
        // VAR( gross: Funktionsname case-insensitiv.
        assert_eq!(ersetze_var("VAR(--rot)", &vars, 0).as_deref(), Some("red"));
    }
    #[test]
    fn var_zyklus() {
        let vars = Variablen::mit_eigenen(
            None,
            vec![
                (String::from("--a"), String::from("var(--b)")),
                (String::from("--b"), String::from("var(--a)")),
            ],
        );
        assert!(ersetze_var("var(--a)", &vars, 0).is_none());
        assert_eq!(
            ersetze_var("var(--a, fallback)", &vars, 0).as_deref(),
            Some("fallback")
        );
    }
    #[test]
    fn var_kette() {
        let vars = Variablen::mit_eigenen(
            None,
            vec![
                (String::from("--a"), String::from("var(--b)")),
                (String::from("--b"), String::from("calc(1px + 1px)")),
            ],
        );
        assert_eq!(
            ersetze_var("var(--a)", &vars, 0).as_deref(),
            Some("calc(1px + 1px)")
        );
    }
}

#[cfg(test)]
mod tests_l4_min_max_spur {
    use super::*;

    /// L4 (03.10.2026 08:0x): der Rechner kann `min()`/`max()`/`clamp()`
    /// — der Spur-Weg fragte ihn nur nicht. GEMESSENER FEHLER, gefunden
    /// von `l4_min_max_und_clamp_in_einer_spur` (das war rot mit
    /// `raster_spalten.len() == 0`): die Spur `min(100px, 50%)` kam nicht
    /// durch, und dieser Test sagt, WELCHE der drei Stellen es war.
    #[test]
    fn min_max_clamp_rechnen() {
        let ctx = RechenCtx::neu(16.0, 800.0, 800.0).mit_hoehe(600.0);
        assert_eq!(
            rechne("min(100px,50%)", &ctx, 0),
            Some(100.0),
            "min zweier px"
        );
        assert_eq!(
            rechne("min(100px, 50%)", &ctx, 0),
            Some(100.0),
            "mit Leerzeichen"
        );
        assert_eq!(
            rechne("max(100px,50%)", &ctx, 0),
            Some(400.0),
            "max mit Prozent"
        );
        assert_eq!(
            rechne("clamp(100px,150px,200px)", &ctx, 0),
            Some(150.0),
            "clamp nimmt den mittleren Wert"
        );
        // OFFENE FRAGE, und sie ist eine Messung, keine Behauptung:
        // GEMESSEN liefert `min(100px)` **100** statt `None`. Ob CSS
        // Values 4 §5.2 das mit EINEM Argument als ungueltig bezeichnet,
        // habe ich NICHT belegt — der Schleifenwaechter hat weitere
        // Normabrufe gestoppt, und ich rate nicht. Bis dahin steht hier
        // die gemessene Zahl, und die Frage steht im Bericht.
        assert_eq!(
            rechne("min(100px)", &ctx, 0),
            Some(100.0),
            "GEMESSEN: min() mit einem Argument wird gerechnet, nicht \
             verworfen — die Normlage ist unbelegt"
        );
    }
}

#[cfg(test)]
mod tests_regression_w6b {
    use super::*;

    #[test]
    fn zahl_mal_laenge_bleibt_laenge() {
        // Regression W6B: 2 * 12px ist px — sonst scheitert "+ 1em" am
        // Typvergleich und die Deklaration faellt weg (marge 0 statt 42).
        let ctx = RechenCtx::neu(18.0, 800.0, 800.0).mit_hoehe(600.0);
        assert_eq!(rechne("calc(2 * 12px + 1em)", &ctx, 0), Some(42.0));
        assert_eq!(rechne("calc(12px * 2)", &ctx, 0), Some(24.0));
    }
}
