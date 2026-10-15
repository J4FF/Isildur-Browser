// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Mini-Regex-Engine (J1/J4: "Regex-Literale (einfach)"): Literale, `.`,
//! `\d\D\w\W\s\S`, Klassen `[...]`, Quantoren (`* + ? {n,m}`, gierig/lazy),
//! Gruppen + Alternation, Anker `^ $`, Wortgrenzen, Lookahead/behind,
//! Rueckverweise. Schalter `g i m s y` (`u` wird toleriert).
//!
//! Matching auf UTF-16-Einheiten (JS-Index-Semantik). Backtracking mit
//! Schritt-Budget: pathologische Muster enden mit "kein Treffer" statt
//! Endlosschleife (M3). Keine Panics bei Eingaben.

/// Obergrenze der Backtracking-Schritte je Suche (M3).
pub(crate) const MAX_REGEX_SCHRITTE: u64 = 500_000;

/// Wie viele Schritte **zusaetzlich** je Zeichen der Eingabe bereitstehen.
///
/// GEMESSEN 04.10. (`ASCII.js`-Naeherung: 1.045.000 Zeichen, Muster
/// `\w+`, `[a-z]+`, `[^a-z]`, `\s+`, `\W+`): mit der reinen Basis von
/// 500.000 Schritten war **kein** Fall richtig — der Suchlauf lief in das
/// Budget und lieferte **abgeschnittene** Enden (5 von 5 falsch, 54 ms).
/// Das trifft genau die amtlichen Grossfaelle: jeder Lauf ueber mehr als
/// ~500.000 Zeichen war zwangslaeufig falsch, egal wie schnell.
pub(crate) const SCHRITTE_PRO_ZEICHEN: u64 = 8;

/// Harte Obergrenze des Budgets (M3/M5) — auch bei einem sehr grossen
/// Text bleibt der Lauf endlich.
pub(crate) const MAX_REGEX_SCHRITTE_GESAMT: u64 = 64_000_000;

/// Harte Obergrenze fuer die **Gesamtzahl** erzeugter Staende eines
/// Laufs (M3/M5, M8: kein Haenger).
///
/// GEMESSEN 04.10.: das Schrittbudget allein genuegt nicht, seit es mit der
/// Eingabegroesse waechst. Bei `(?:a|aa)+b` auf 100.000 Zeichen lief der
/// verzweigende Teil ueber 25 s: jede Runde liefert **zwei** Staende, die
/// Kette laeuft 100.000 Runden, und jeder Stand wird weiter verzweigt —
/// Aufwand quadratisch in der Rundenzahl, begrenzt nur durch die Breite
/// (`MAX_STAENDE` = 4096) und die Rekursionstiefe. Diese Konstante
/// begrenzt die **Summe** ueber alle Knoten und beendet damit beide.
pub(crate) const MAX_STAENDEN_GESAMT: u32 = 200_000;
/// Maximale Muster-Laenge (M4).
pub(crate) const MAX_MUSTER: usize = 100_000;
/// Maximale Rueckschau-Weite fuer Lookbehind (M4).
const MAX_LOOKBEHIND: usize = 1024;
/// Maximale Knoten-Tiefe beim Matchen (M5).
const MAX_MATCH_TIEFE: usize = 512;
/// Maximale Laenge eines Gruppen**namens** in Zeichen (M4).
///
/// GEMESSEN 03.10.: die alte Fassung zaehlte **Bytes** (`n.len() > 64`) —
/// ein Name aus 20 kastenlos liegenden Zeichen (60 Byte) war damit noch
/// gueltig, einer aus 22 nicht. Die Grenze gilt jetzt fuer Zeichen, weil
/// die Spec in Zeichen spricht.
const MAX_GRUPPENNAME: usize = 64;

#[derive(Debug, Clone)]
pub(crate) enum Knoten {
    Leer,
    Einheit(u16),
    Klasse {
        negiert: bool,
        bereiche: Vec<(u16, u16)>,
        /// Die Klassenbereiche **gefaltet** (Randpunkte gefaltet,
        /// zusammengefuehrt) — fuer den `i`-Schalter.
        ///
        /// GEMESSEN 04.10.: die Klassen-Seite muss mitgefaltet werden.
        /// `[ΐ]` gegen `U+1FD3` mit `ui` verlangt das ebenso wie `[K]/iu`
        /// gegen `U+212A`. **Bereiche, keine Einzelzeichen:** der
        /// Vertreter ist der kleinste Code-Point der Klasse, `falte('a')`
        /// ist also `'A'` — mit einer Liste einzelner Vertreter fand
        /// `[a-z]` sein eigenes `'a'` nicht mehr (3 gemessene
        /// Regressionen, alle mit `i`).
        gefalten: Vec<(u16, u16)>,
        /// Die Klassenbereiche **kanonisiert** (ECMA-262 22.2.2.8
        /// `Canonicalize`) — nur fuer `i` **ohne** `u`. Wird ausschliesslich
        /// in dieser Datei gebaut (`Knoten::Klasse` kommt in keiner fremden
        /// Datei vor), deshalb ist das Feld hier gefahrlos — anders als ein
        /// Feld in `Schalter`.
        kanon: Vec<(u16, u16)>,
    },
    Punkt,
    Anfang,
    Ende,
    Wortgrenze(bool),
    Folge(Vec<Knoten>),
    Auswahl(Vec<Knoten>),
    Wiederholung {
        inner: Box<Knoten>,
        min: u32,
        max: Option<u32>,
        gierig: bool,
        /// Die Gruppen-Ids **innerhalb** des Rumpfes — bei jeder neuen
        /// Runde der Wiederholung werden sie zurueckgesetzt (ECMA-262
        /// 22.2.2.7.4: die `MatchState` der Rumpf-Ebene ist pro Runde
        /// neu; die Gruppen **ausserhalb** bleiben erhalten).
        ///
        /// GEMESSEN 04.10. (Orakel `node` v22,
        /// `(z)((a+)?(b+)?(c))*` auf `"zaacbbbcac"`): ohne Ruecksetzung
        /// lieferte die Engine 5 belegte Gruppen, `node` 4 belegte und
        /// 1 leere — die Gruppen der **letzten** Runde sind die einzigen
        /// gueltigen.
        gruppen_innen: Vec<usize>,
    },
    Gruppe {
        id: usize,
        inner: Box<Knoten>,
    },
    Rueckverweis(usize),
    /// `\p{…}` / `\P{…}`: mit `u`/`v` eine **Eigenschaftsklasse**, ohne
    /// `u` nach Annex B.1.2 ein **Literal** — `\p` ist dort ein
    /// IdentityEscape, also trifft es `p`.
    ///
    /// GEMESSEN 04.10. (Orakel `node` v22): `/\p{L}+/` **ohne** `u`
    /// trifft `"p{L}abc"`, mit `u` trifft es `"abc"`. Vorher wertete der
    /// Matcher die Eigenschaft **immer** — 2 von 26 Mustern der
    /// Faehigkeitsprobe waren dadurch falsch.
    Eigenschaft {
        /// `true` bei `\P{…}` (die Bereiche sind dann schon komplementiert)
        _negativ_bereits: bool,
        bereiche: Vec<(u16, u16)>,
        /// `p`/`P` + der Text zwischen `{` und `}` — genau so, wie er
        /// geschrieben wurde.
        literal: Vec<u16>,
    },
    /// `\u{…}`: mit `u` der **Codepoint**, ohne `u` nach Annex B.1.2 das
    /// Literal — `\u` ist ein IdentityEscape, also trifft es `u`
    /// (GEMESSEN: `/\u{1F600}/.test("u{1F600}")` ist **wahr**).
    CodepunktOderLiteral {
        /// UTF-16 des Codepoints (eine oder zwei Einheiten)
        cp: Vec<u16>,
        /// `u` + `{` + Ziffern + `}`
        literal: Vec<u16>,
    },
    Vorausschau {
        inner: Box<Knoten>,
        positiv: bool,
    },
    Rueckschau {
        inner: Box<Knoten>,
        positiv: bool,
    },
    /// Release 15.10.2026: a character class over CODE POINTS (unicode
    /// mode `u`/`v`, see `CpKlasse`).
    KlasseCp(Box<CpKlasse>),
    /// Release 15.10.2026: `(?ims-ims:…)` (ES2025 pattern modifiers): the
    /// flags `an` are switched on and `aus` off while `inner` matches.
    /// Bits: 1 = `i`, 2 = `m`, 4 = `s`.
    Modifikator {
        inner: Box<Knoten>,
        an: u8,
        aus: u8,
    },
    /// Release 15.10.2026: `\k<name>` when the name belongs to several groups
    /// (ES2025 duplicate named groups): the one that participated.
    RueckverweisNamen(Vec<usize>),
}

/// Release 15.10.2026: a character class over code points. MEASURED before:
/// classes held UTF-16 units only, so with `u` an astral character never
/// matched (`/^[😀😁]$/u.test('😁')`, `/^\p{Emoji_Presentation}$/u.test('😀')`
/// and `/^[^a]$/u.test('😀')` were false, node 22: true).
#[derive(Debug, Clone)]
pub(crate) struct CpKlasse {
    pub(crate) negiert: bool,
    /// Sorted, merged code point ranges.
    pub(crate) bereiche: Vec<(u32, u32)>,
    /// With `i`: the ranges under case folding (`falte`); a character
    /// matches when its folded form is in here. Valid when `faltung`.
    pub(crate) gefalten: Vec<(u32, u32)>,
    pub(crate) faltung: bool,
    /// `\q{…}` strings of two or more code points, longest first (v mode).
    pub(crate) texte: Vec<Vec<u16>>,
    /// `\q{}` holds the empty string (tried last).
    pub(crate) leer: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Schalter {
    pub global: bool,
    pub egal_gross: bool,
    pub mehrzeilig: bool,
    pub punkt_alles: bool,
    pub klebrig: bool,
}

pub(crate) fn schalter_aus(s: &str) -> Result<Schalter, String> {
    let mut f = Schalter {
        global: false,
        egal_gross: false,
        mehrzeilig: false,
        punkt_alles: false,
        klebrig: false,
    };
    for c in s.chars() {
        match c {
            'g' => f.global = true,
            'i' => f.egal_gross = true,
            'm' => f.mehrzeilig = true,
            's' => f.punkt_alles = true,
            'y' => f.klebrig = true,
            'u' => {} // toleriert (Einheiten-Semantik bleibt).
            // ES2022 `d` (`hasIndices`): **gueltiger** Schalter. Die
            // Indizes stehen nur in `exec(...).indices`; der Kern liefert
            // Start/Ende ohnehin, also wird das Flag hier nur akzeptiert.
            //
            // GEMESSEN 04.10.: `test/built-ins/RegExp/match-indices/**`
            // (45: „ungueltiger Regex-Schalter 'd'"), und `node` v22
            // akzeptiert `new RegExp("a", "d")`.
            'd' => {}
            // ES2024 `v` (`unicodeSets`): **gueltiger** Schalter. Aktiviert
            // Unicode-Modus (wie `u`) und erlaubt Mengenoperationen in
            // Zeichenklassen (`[a-z--[aeiou]]`, `[\p{L}&&\p{Uppercase}]`).
            // Der Parser aktiviert `unicode` bei `u` **oder** `v`.
            'v' => {}
            _ => return Err(format!("ungueltiger Schalter '{c}'")),
        }
    }
    // Release 15.10.2026: `u` and `v` together are a SyntaxError (ECMA-262
    // 22.2.3.4 step 12). MEASURED before: `new RegExp('a', 'uv')` was accepted.
    if s.contains('u') && s.contains('v') {
        return Err("die Schalter u und v schliessen sich aus".to_string());
    }
    Ok(f)
}

/// Syntax pruefen (fuer Literale + Konstruktor).
pub(crate) fn pruefen(muster: &str) -> Result<(), String> {
    pruefen_mit_flags(muster, "")
}

/// Syntax pruefen **mit Schaltern**.
///
/// 05.10.2026: GEMESSEN an youtubes ES5-Bundle — `parse.rs` prueft
/// Regex-LITERALE mit `pruefen(&m)` und liess die Schalter weg. Damit wurde
/// `/[\u{1D400}-\u{1D7FF}\u2100-\u214F]/u` als **Nicht-Unicode** geprueft
/// (Annex-B: `\u` = Buchstabe u), und der Parser brach mit
/// „Bereich ausserhalb der Reihenfolge: 125-117" ab. `RegExp(…)` mit
/// Schaltern (`kompilieren_mit_flags`) war immer richtig — nur der
/// Literal-Weg nicht.
pub(crate) fn pruefen_mit_flags(muster: &str, flags: &str) -> Result<(), String> {
    if muster.len() > MAX_MUSTER {
        return Err("Muster zu lang".to_string());
    }
    let zeichen: Vec<char> = muster.chars().collect();
    let mut p = Parser::neu(zeichen, flags);
    let _ = p.alternation()?;
    if p.pos != p.zeichen.len() {
        return Err("Rest nach Muster".to_string());
    }
    Ok(())
}

/// Muster uebersetzen. Liefert den Knoten, die Zahl der Gruppen und die
/// NAMEN der Gruppen (Index = Gruppennummer, leer = unbenannt) —
///
/// GEMESSEN (A5): `(?<name>...)` war vorher abgelehnt
/// („nur (?<=)/(?<!) unterstuetzt"), `match(...).groups` fehlte ganz.
pub(crate) fn kompilieren(muster: &str) -> Result<(Knoten, usize, Vec<String>), String> {
    kompilieren_mit_flags(muster, "")
}

pub(crate) fn kompilieren_mit_flags(muster: &str, flags: &str) -> Result<(Knoten, usize, Vec<String>), String> {
    // Release 15.10.2026: compiled patterns are cached per thread. MEASURED
    // before: every `exec`/`test`/`match`/`replace` parsed the pattern again
    // (a `while (re.exec(s))` loop parsed it once per match).
    type Uebersetzt = (Knoten, usize, Vec<String>);
    thread_local! {
        static ABLAGE: std::cell::RefCell<std::collections::HashMap<String, Uebersetzt>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if muster.len() > MAX_MUSTER {
        return Err("Muster zu lang".to_string());
    }
    let schluessel = format!("{flags}\u{0}{muster}");
    if let Some(t) = ABLAGE.with(|a| a.borrow().get(&schluessel).cloned()) {
        return Ok(t);
    }
    let t = kompilieren_neu(muster, flags)?;
    ABLAGE.with(|a| {
        let mut a = a.borrow_mut();
        // M4: bounded; a page with very many patterns starts over.
        if a.len() >= 512 {
            a.clear();
        }
        a.insert(schluessel, t.clone());
    });
    Ok(t)
}

fn kompilieren_neu(muster: &str, flags: &str) -> Result<(Knoten, usize, Vec<String>), String> {
    let zeichen: Vec<char> = muster.chars().collect();
    let mut p = Parser::neu(zeichen, flags);
    let k = p.alternation()?;
    if p.pos != p.zeichen.len() {
        return Err("Rest nach Muster".to_string());
    }
    Ok((k, p.gruppen, p.namen))
}

struct Parser {
    unicode: bool,
    zeichen: Vec<char>,
    pos: usize,
    gruppen: usize,
    /// Name je Gruppe (leer = unbenannt).
    namen: Vec<String>,

    /// GEMESSEN 03.10.: fuer `\k<name>` (benannter Rueckverweis,
    /// ECMA-262 22.2.2.8) muss der Leser den **Namen** in eine
    /// Gruppennummer uebersetzen. `namen` alone reicht nicht: dort stehen
    /// nur die *benannten* Gruppen (das ist der Vertrag, den
    /// `builtin2.rs:2264` benutzt), also kaeme `/(a)(?<b>x)\k<b>/` auf
    /// die Nummer 1 statt 2. Deshalb eine eigene, nach Parse-Ende
    /// wegfallende Liste **mit** Nummer.
    ///
    /// GEMESSEN, warum der Eintrag **vor** dem Rumpf kommt: `(?<a>\k<a>)`
    /// ist erlaubt (Selbstbezug), das heisst der Name muss stehen, bevor
    /// `alternation()` den Rumpf liest.
    namen_mit_id: Vec<(String, usize)>,

    /// J18: war die zuletzt gelesene Klassen-Einheit eine
    /// **Abkuerzung** (`\d \D \w \W \s \S`)? Eine Abkuerzung ist
    /// auch bei einem einzigen Bereich (`\d` = 0..9) keine einzelne
    /// Zeichen-Einheit — GEMESSEN vorher: `RegExp('[\\d]').test('5')`
    /// lieferte `false`, weil `[0..9]` auf `[0..0]` zusammengefiel.
    abkuerzung: bool,

    /// Release 15.10.2026: `v` (unicodeSets) mode.
    v: bool,
    /// `i` is in effect where the parser stands (flag or modifier).
    i_aktiv: bool,
    /// The (disjunction, alternative) path to the current position and the
    /// disjunction counter: two groups may share a name only in different
    /// alternatives (ES2025 duplicate named groups).
    alt_pfad: Vec<(u32, u32)>,
    alt_zaehler: u32,
    namen_pfade: Vec<(String, Vec<(u32, u32)>)>,
}

impl Parser {
    /// Release 15.10.2026: one constructor for both entry points.
    fn neu(zeichen: Vec<char>, flags: &str) -> Parser {
        Parser {
            namen_mit_id: Parser::gruppen_vorab(&zeichen),
            zeichen,
            pos: 0,
            gruppen: 0,
            namen: Vec::new(),
            abkuerzung: false,
            unicode: flags.contains('u') || flags.contains('v'),
            v: flags.contains('v'),
            i_aktiv: flags.contains('i'),
            alt_pfad: Vec::new(),
            alt_zaehler: 0,
            namen_pfade: Vec::new(),
        }
    }

    /// Durchgang 0: **alle** benannten Gruppen mit Nummer einsammeln.
    ///
    /// GEMESSEN 03.10. (`named-groups/non-unicode-references.js`, zwei
    /// gezaehlte Faelle): `\k<a>` darf **vor** `(?<a>…)` stehen — die Norm
    /// (22.2.1) sammelt die Gruppennamen erst, **nachdem** das ganze Muster
    /// gelesen ist. Der Leser loest den Rueckverweis aber sofort auf und
    /// kannte den Namen noch nicht: „unbekannter Gruppenname im
    /// Rueckverweis". Dieser Vorbau behebt das, und er muss die Nummern
    /// **genau so** zaehlen wie der Hauptpass:
    ///
    ///  * `(` ohne `?` ist eine fangende Gruppe -> Nummer,
    ///  * `(?:` `(?=` `(?!` `(?<=` `(?<!` sind **keine**,
    ///  * `(?<name>` ist eine fangende Gruppe **mit** Namen,
    ///  * alles in `[ … ]` und hinter `\` gehoert nicht dazu.
    ///
    /// M3: die Schleife laeuft ueber die Zeichen des Musters (das ist die
    /// Eingabe) und ist durch `MAX_MUSTER` (100 000) gedeckelt; der
    /// Namenslauf endet zudem an `MAX_GRUPPENNAME`.
    fn gruppen_vorab(zeichen: &[char]) -> Vec<(String, usize)> {
        let mut raus: Vec<(String, usize)> = Vec::new();
        let mut id = 0usize;
        let mut i = 0usize;
        let mut in_klasse = false;
        while i < zeichen.len() {
            let c = zeichen[i];
            if c == '\\' {
                i += 2; // Esc und das Zeichen danach ueberspringen
                continue;
            }
            if in_klasse {
                if c == ']' {
                    in_klasse = false;
                }
                i += 1;
                continue;
            }
            if c == '[' {
                in_klasse = true;
                i += 1;
                continue;
            }
            if c != '(' {
                i += 1;
                continue;
            }
            if i + 1 < zeichen.len() && zeichen[i + 1] == '?' {
                let ist_name = i + 3 < zeichen.len()
                    && zeichen[i + 2] == '<'
                    && zeichen[i + 3] != '='
                    && zeichen[i + 3] != '!';
                if !ist_name {
                    i += 2;
                    continue;
                }
                let mut name = String::new();
                let mut j = i + 3;
                while j < zeichen.len() && zeichen[j] != '>' {
                    if name.chars().count() > MAX_GRUPPENNAME {
                        break;
                    }
                    name.push(zeichen[j]);
                    j += 1;
                }
                id += 1;
                raus.push((name, id));
                i = j;
                continue;
            }
            id += 1;
            i += 1;
        }
        raus
    }

    fn rest(&self) -> usize {
        self.zeichen.len().saturating_sub(self.pos)
    }

    fn alternation(&mut self) -> Result<Knoten, String> {
        self.alt_zaehler += 1;
        self.alt_pfad.push((self.alt_zaehler, 0));
        let r = self.alternation_innen();
        self.alt_pfad.pop();
        r
    }

    fn alternation_innen(&mut self) -> Result<Knoten, String> {
        let mut zweige = vec![self.folge()?];
        while self.pos < self.zeichen.len() && self.zeichen[self.pos] == '|' {
            self.pos += 1;
            if let Some(l) = self.alt_pfad.last_mut() {
                l.1 += 1;
            }
            zweige.push(self.folge()?);
        }
        if zweige.len() == 1 {
            Ok(zweige.pop().unwrap())
        } else {
            Ok(Knoten::Auswahl(zweige))
        }
    }

    fn folge(&mut self) -> Result<Knoten, String> {
        let mut teile = Vec::new();
        while self.pos < self.zeichen.len()
            && self.zeichen[self.pos] != '|'
            && self.zeichen[self.pos] != ')'
        {
            teile.push(self.wiederholung()?);
        }
        if teile.len() == 1 {
            Ok(teile.pop().unwrap())
        } else {
            Ok(Knoten::Folge(teile))
        }
    }

    fn wiederholung(&mut self) -> Result<Knoten, String> {
        let mut k = self.atom()?;
        loop {
            if self.pos >= self.zeichen.len() {
                break;
            }
            // **Welche** Quantifier es war, entscheidet die Bedeutung des
            // folgenden `?`: nach `*`/`+` und nach `{n,m}` ist es der
            // **Lazy**-Marker, nach `?` ist es eine zweite OptIONALitaet
            // (`a??` = lazy optional, Annex B).
            //
            // GEMESSEN 04.10. (Orakel `node` v22):
            // * `a[a-z]{2,4}?` auf `"abcdefghi"` lieferte 0..5 statt 0..3:
            //   die alte Fassung fraß das `?` als „verdoppelte Quantifier"
            //   und machte **alle** `*?`/`+?`/`??`/`{n,m}?` gierig.
            // * `java(script)?`, `b?b?b?b`, `^(A)?(A.*)$` liefen in einer
            //   Zwischenfassung **schlechter** (Quote 905 -> 896): dort
            //   wurde das `?` des Quantifiers selbst als Lazy-Marker
            //   gelesen und die Quantifier-Ziffer nicht konsumiert.
            // * `self.zeichen[self.pos]` panikte am Ende der Zeichenkette
            //   (`index out of bounds`, len 11 / index 11) — der alte
            //   Kommentar „direkter Zugriff lief daneben" galt fort.
            let (min, max, war_optional) = match self.zeichen[self.pos] {
                '*' => (0, None, false),
                '+' => (1, None, false),
                '?' => (0, Some(1), true),
                '{' => match self.klammer_menge()? {
                    Some((n, m)) => (n, m, false),
                    None => break,
                },
                _ => break,
            };
            let mut gierig = true;
            if war_optional {
                // Das `?` selbst konsumieren …
                self.pos += 1;
                // … und ein weiteres `?` bedeutet lazy (`a??`).
                if self.zeichen.get(self.pos).copied() == Some('?') {
                    gierig = false;
                    self.pos += 1;
                }
            } else if matches!(self.zeichen.get(self.pos).copied(), Some('*') | Some('+')) {
                // `*`/`+` selbst konsumieren; bei `{n,m}` steht `pos` schon
                // hinter der Klammer (siehe Kommentar oben).
                self.pos += 1;
                match self.zeichen.get(self.pos).copied() {
                    // Lazy: `a*?`, `a+?`, `a{n,m}?`
                    Some('?') => {
                        gierig = false;
                        self.pos += 1;
                    }
                    // Annex B: `a**`, `a{n,m}*`
                    Some('*') | Some('+') => self.pos += 1,
                    _ => {}
                }
            } else if self.zeichen.get(self.pos).copied() == Some('?') {
                // `{n,m}?` — Lazy.
                gierig = false;
                self.pos += 1;
            }
            let mut gruppen_innen = Vec::new();
            sammle_gruppen(&k, &mut gruppen_innen);
            k = Knoten::Wiederholung {
                inner: Box::new(k),
                min,
                max,
                gierig,
                gruppen_innen,
            };
        }
        Ok(k)
    }

    /// Gruppenname nach `(?<` lesen (Position auf `>` gesetzt).
    ///
    /// ECMA-262 22.2.1.1: `GroupName :: RegExpIdentifierName` — und das ist
    /// **keine** „Buchstaben und `_`"-Regel:
    /// `RegExpIdentifierStart ::= UnicodeIDStart | UnicodeIDContinue | $`,
    /// `RegExpIdentifierPart ::= UnicodeIDContinue | $ | U+200C | U+200D`.
    ///
    /// GEMESSEN 03.10. (Lauf `T262_NUR=RegExp`, 302 gezaehlte Faelle): die
    /// alte Regel `c.is_alphanumeric() || c == '_' || c == '$'` verwarf
    /// **π**, **`$`**, **`_‌`**, **`ಠ_ಠ`** und jedes **Escape**
    /// (`A`, `{1d4d1}`) als „ungueltiger Gruppenname" — vier
    /// gezaehlte Faelle (`named-groups/non-unicode-property-names*.js`).
    /// Sie nahm ausserdem **keine** Escapes an, obwohl die Suite sie
    /// verlangt.
    ///
    /// Die Astral-Ebene (ausserhalb BMP) kann die Tabelle von `unicode`
    /// nicht liefern: sie speichert Bereiche als **u16**-Paare, und ein
    /// Buchstabe jenseits U+FFFF ist dort nicht von zwei beliebigen
    /// Ersatzzeichen zu unterscheiden — genau diesen Unterschied verlangt
    /// `named-groups/non-unicode-property-names-invalid.js` (`𝟚` ist als
    /// Fortsetzung gueltig, als Anfang nicht). Fuer diese Ebene wird
    /// deshalb `char` befragt: der Ersatz ist benannt und begrenzt, nicht
    /// stillschweigend.
    fn gruppen_name(&mut self) -> Result<String, String> {
        let mut n = String::new();
        let mut erster = true;
        while self.pos < self.zeichen.len() {
            let c = self.zeichen[self.pos];
            if c == '>' {
                break;
            }
            if c == '\\' {
                // GEMESSEN: `(?<A>a)` ist gueltig und `(?<{1d4d1}>a)`
                // auch — der Name wird **dekodiert**, nicht roh
                // uebernommen (`groups.π` muss `π` liefern).
                let w = self.name_escape()?;
                if !Self::name_erlaubt(w, erster) {
                    return Err("ungueltiger Gruppenname".to_string());
                }
                n.push(w);
                erster = false;
                continue;
            }
            if !Self::name_erlaubt(c, erster) {
                return Err("ungueltiger Gruppenname".to_string());
            }
            n.push(c);
            erster = false;
            self.pos += 1;
            // M3: harte Grenze, an **Zeichen** (nicht Bytes) gemessen —
            // ein Name aus 4-Byte-Zeichen darf nicht ungegrenzt wachsen.
            if n.chars().count() > MAX_GRUPPENNAME {
                return Err("Gruppenname zu lang".to_string());
            }
        }
        self.erwarte('>')?;
        if n.is_empty() {
            return Err("leerer Gruppenname".to_string());
        }
        // Release 15.10.2026: ES2025 allows one name twice when the groups
        // cannot both participate (different alternatives of a disjunction).
        // MEASURED before: `/(?<y>a)|(?<y>b)/` was a SyntaxError.
        let pfad = self.alt_pfad.clone();
        for (anderer, p) in &self.namen_pfade {
            if *anderer == n && !Self::schliessen_sich_aus(p, &pfad) {
                return Err("Gruppenname doppelt".to_string());
            }
        }
        self.namen_pfade.push((n.clone(), pfad));
        Ok(n)
    }

    /// Ein Zeichen aus einem Namens-Escape (`A`, `{1d4d1}`).
    /// Liefert das **Zeichen**; ein halbes Ersatzpaar wird abgewiesen
    /// (der Name kann kein Ersatzzeichen enthalten).
    fn name_escape(&mut self) -> Result<char, String> {
        // self.pos zeigt auf `\`.
        self.pos += 1;
        if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != 'u' {
            return Err("ungueltiger Gruppenname".to_string());
        }
        self.pos += 1;
        if self.pos >= self.zeichen.len() {
            return Err("ungueltiger Gruppenname".to_string());
        }
        if self.zeichen[self.pos] == '{' {
            self.pos += 1;
            let mut w: u32 = 0;
            let mut stellen = 0;
            while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                let d = self.zeichen[self.pos]
                    .to_digit(16)
                    .ok_or("ungueltiger Gruppenname")?;
                w = w * 16 + d;
                stellen += 1;
                // M3: eine `{…}`-Angabe ist mit 6 Stellen gedeckelt.
                if stellen > 6 || w > 0x10FFFF {
                    return Err("ungueltiger Gruppenname".to_string());
                }
                self.pos += 1;
            }
            if stellen == 0 || self.pos >= self.zeichen.len() {
                return Err("ungueltiger Gruppenname".to_string());
            }
            self.pos += 1; // }
            return char::from_u32(w).ok_or_else(|| "ungueltiger Gruppenname".to_string());
        }
        // `XXXX`: vier Hex-Ziffern, notfalls als Paar.
        let lo = self.hex(4)? as u32;
        if (0xD800..0xDC00).contains(&lo) {
            // Ersatzzeichen: nur gueltig, wenn unmittelbar der zweite
            // Halbwert folgt (GEMESSEN: `(?<𝓑>a)` ist erlaubt).
            if self.pos + 2 <= self.zeichen.len()
                && self.zeichen[self.pos] == '\\'
                && self.zeichen[self.pos + 1] == 'u'
            {
                let merker = self.pos;
                self.pos += 2;
                let hi = self.hex(4)? as u32;
                if (0xDC00..0xE000).contains(&hi) {
                    let w = 0x10000 + ((lo - 0xD800) << 10) + (hi - 0xDC00);
                    return char::from_u32(w).ok_or_else(|| "ungueltiger Gruppenname".to_string());
                }
                self.pos = merker;
            }
            return Err("ungueltiger Gruppenname".to_string());
        }
        char::from_u32(lo).ok_or_else(|| "ungueltiger Gruppenname".to_string())
    }

    /// Darf `c` an dieser Stelle im Namen stehen?
    fn name_erlaubt(c: char, erster: bool) -> bool {
        // `$` ist in BEIDEN Regeln erlaubt; ZWNJ/ZWJ nur als Fortsetzung.
        if c == '$' {
            return true;
        }
        if !erster && (c == '\u{200C}' || c == '\u{200D}') {
            return true;
        }
        let cp = c as u32;
        if cp > 0xFFFF {
            // Ausserhalb BMP: `is_alphabetic` = L* (der ID_Start-Anteil),
            // `is_numeric` = N* (der ID_Continue-Anteil).
            return if erster {
                c.is_alphabetic()
            } else {
                c.is_alphabetic() || c.is_numeric()
            };
        }
        let w = cp as u16;
        if w == u16::from(b'_') {
            return true;
        }
        let start =
            crate::unicode::eigenschaft_bereiche(if erster { "ID_Start" } else { "ID_Continue" });
        if Self::in_bereichen(start.as_deref(), w) {
            return true;
        }
        // `erster` darf zusaetzlich jeder ID_Continue-Zeichen sein
        // (Spec: `UnicodeIDStart | UnicodeIDContinue`), z. B. `(?<0a>…)`.
        if erster {
            let cont = crate::unicode::eigenschaft_bereiche("ID_Continue");
            return Self::in_bereichen(cont.as_deref(), w);
        }
        false
    }

    /// Bereichsliste (u16) — lineare Suche ueber die Zeilen der Tabelle.
    /// M3: die Schleife laeuft ueber die Tabelle, nicht ueber die Eingabe.
    fn in_bereichen(tabelle: Option<&[(u16, u16)]>, w: u16) -> bool {
        match tabelle {
            None => false,
            Some(b) => b.iter().any(|&(lo, hi)| w >= lo && w <= hi),
        }
    }

    fn klammer_menge(&mut self) -> Result<Option<(u32, Option<u32>)>, String> {
        // Versucht `{n}`, `{n,}`, `{n,m}` ab pos. Gibt None zurueck, wenn
        // es keine gueltige Menge ist (dann ist `{` literal).
        let start = self.pos;
        self.pos += 1; // {
        let mut n_str = String::new();
        while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
            n_str.push(self.zeichen[self.pos]);
            self.pos += 1;
        }
        if n_str.is_empty() {
            self.pos = start;
            return Ok(None);
        }
        let n: u32 = n_str.parse().map_err(|_| "Menge zu gross".to_string())?;
        if n > 100_000 {
            return Err("Menge zu gross".to_string());
        }
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '}' {
            self.pos += 1;
            return Ok(Some((n, Some(n))));
        }
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == ',' {
            self.pos += 1;
            let mut m_str = String::new();
            while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
                m_str.push(self.zeichen[self.pos]);
                self.pos += 1;
            }
            if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '}' {
                self.pos += 1;
                if m_str.is_empty() {
                    return Ok(Some((n, None)));
                }
                let m: u32 = m_str.parse().map_err(|_| "Menge zu gross".to_string())?;
                if m > 100_000 || m < n {
                    return Err("ungueltige Menge".to_string());
                }
                return Ok(Some((n, Some(m))));
            }
        }
        self.pos = start;
        Ok(None)
    }

    fn atom(&mut self) -> Result<Knoten, String> {
        if self.pos >= self.zeichen.len() {
            return Err("Muster endet unerwartet".to_string());
        }
        let c = self.zeichen[self.pos];
        match c {
            '^' => {
                self.pos += 1;
                Ok(Knoten::Anfang)
            }
            '$' => {
                self.pos += 1;
                Ok(Knoten::Ende)
            }
            '.' => {
                self.pos += 1;
                Ok(Knoten::Punkt)
            }
            '(' => {
                self.pos += 1;
                if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '?' {
                    self.pos += 1;
                    if self.pos >= self.zeichen.len() {
                        return Err("(? am Ende".to_string());
                    }
                    match self.zeichen[self.pos] {
                        ':' => {
                            self.pos += 1;
                            let k = self.alternation()?;
                            self.erwarte(')')?;
                            Ok(k)
                        }
                        '=' => {
                            self.pos += 1;
                            let k = self.alternation()?;
                            self.erwarte(')')?;
                            Ok(Knoten::Vorausschau {
                                inner: Box::new(k),
                                positiv: true,
                            })
                        }
                        '!' => {
                            self.pos += 1;
                            let k = self.alternation()?;
                            self.erwarte(')')?;
                            Ok(Knoten::Vorausschau {
                                inner: Box::new(k),
                                positiv: false,
                            })
                        }
                        '<' => {
                            self.pos += 1;
                            if self.pos >= self.zeichen.len() {
                                return Err("(?< am Ende".to_string());
                            }
                            let positiv = match self.zeichen[self.pos] {
                                '=' => true,
                                '!' => false,
                                // GEMESSEN (A5): benannte Gruppe `(?<name>...)`.
                                _ => {
                                    let name = self.gruppen_name()?;
                                    self.gruppen += 1;
                                    if self.gruppen > 10_000 {
                                        return Err("zu viele Gruppen".to_string());
                                    }
                                    let id = self.gruppen;
                                    // GEMESSEN 03.10.: der Name wird **vor**
                                    // dem Rumpf eingetragen, weil
                                    // `(?<a>\k<a>)` erlaubt ist (Selbstbezug).
                                    self.namen_mit_id.push((name.clone(), id));
                                    // Release 15.10.2026: `namen[i]` belongs
                                    // to group i + 1, entered when the group
                                    // OPENS. MEASURED before: names were
                                    // pushed after the body and unnamed groups
                                    // not at all, so `/(a)(?<x>b)/` gave
                                    // `groups.x === 'a'`.
                                    self.namen.push(name);
                                    let k = self.alternation()?;
                                    self.erwarte(')')?;
                                    return Ok(Knoten::Gruppe {
                                        id,
                                        inner: Box::new(k),
                                    });
                                }
                            };
                            self.pos += 1;
                            let k = self.alternation()?;
                            self.erwarte(')')?;
                            Ok(Knoten::Rueckschau {
                                inner: Box::new(k),
                                positiv,
                            })
                        }
                        'i' | 'm' | 's' | '-' => self.modifikator(),
                        _ => Err("unbekannte (?-Form".to_string()),
                    }
                } else {
                    self.gruppen += 1;
                    if self.gruppen > 10_000 {
                        return Err("zu viele Gruppen".to_string());
                    }
                    let id = self.gruppen;
                    self.namen.push(String::new());
                    let k = self.alternation()?;
                    self.erwarte(')')?;
                    Ok(Knoten::Gruppe {
                        id,
                        inner: Box::new(k),
                    })
                }
            }
            // Release 15.10.2026: unicode mode classes are code point sets.
            '[' if self.unicode => self.klasse_cp(),
            '[' => self.klasse(),
            '\\' => self.escape(),
            // J18: `]` **ausserhalb** einer Klasse ist in JS ein
            // SyntaxError — ausser im Nicht-Unicode-Modus, wo Annex B es
            // als **Literal** erlaubt. GEMESSEN mit node 22: `node 22
            // /[]]/.source` gibt `[]]` zurueck (es wird also akzeptiert),
            // und `[]]` ist die leere Klasse gefolgt von einem Literal
            // `]`. Vorher brach der Parser hier mit „einsames ']'" ab
            // — das ist die Meldung, an der ein echtes Tages-Skript
            // (dayjs) scheiterte.
            ']' if !self.unicode => {
                // GEMESSEN, warum das `pos += 1` hier Pflicht ist: ohne
                // das blieb der Zeiger stehen und die Schleife in
                // `konkatenation` lief endlos — `RegExp('[]]').source`
                // haengte sich auf (der Motor tat nach 60 s nichts mehr).
                // Jeder andere Arm hier erhoeht `pos` ebenfalls.
                self.pos += 1;
                Ok(Knoten::Einheit(']' as u16))
            }
            '{' if !self.unicode => {
                let start = self.pos;
                if self.klammer_menge()?.is_some() { return Err("Menge ohne Atom".to_string()); }
                self.pos = start + 1;
                Ok(Knoten::Einheit('{' as u16))
            }
            '}' if !self.unicode => { self.pos += 1; Ok(Knoten::Einheit('}' as u16)) }
            ')' | '|' | '{' | '}' | ']' | '*' | '+' | '?' => Err(format!("einsames '{c}'")),
            _ => {
                self.pos += 1;
                let mut buf = [0u16; 2];
                let s = c.encode_utf16(&mut buf);
                if s.len() == 1 {
                    Ok(Knoten::Einheit(s[0]))
                } else {
                    Ok(Knoten::Folge(vec![
                        Knoten::Einheit(s[0]),
                        Knoten::Einheit(s[1]),
                    ]))
                }
            }
        }
    }

    fn erwarte(&mut self, c: char) -> Result<(), String> {
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == c {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("erwartet '{c}'"))
        }
    }

    fn escape(&mut self) -> Result<Knoten, String> {
        self.pos += 1; // \
        if self.pos >= self.zeichen.len() {
            return Err("Escape am Ende".to_string());
        }
        let c = self.zeichen[self.pos];
        self.pos += 1;
        // Release 15.10.2026: with `u`/`v`, `\p{…}`, `\P{…}`, `\D`, `\W`, `\S`
        // are code point classes (an astral character is one character).
        if self.unicode && matches!(c, 'p' | 'P' | 'D' | 'W' | 'S') {
            let m = self.cp_abkuerzung(c)?;
            return Ok(self.cp_knoten(CpMenge { bereiche: m, texte: Vec::new() }, false));
        }
        match c {
            'd' => Ok(Self::klasse_kanonisch(
                false,
                vec![('0' as u16, '9' as u16)],
            )),
            'D' => Ok(Self::klasse_kanonisch(true, vec![('0' as u16, '9' as u16)])),
            'w' => Ok(Self::klasse_kanonisch(
                false,
                Self::zeichen_bereiche(b'0' as u16, b'9' as u16)
                    .into_iter()
                    .chain(Self::zeichen_bereiche(b'A' as u16, b'Z' as u16))
                    .chain(Self::zeichen_bereiche(b'a' as u16, b'z' as u16))
                    .chain(einzel_b(b'_' as u16))
                    .collect(),
            )),
            'W' => Ok(Self::klasse_kanonisch(
                true,
                Self::zeichen_bereiche(b'0' as u16, b'9' as u16)
                    .into_iter()
                    .chain(Self::zeichen_bereiche(b'A' as u16, b'Z' as u16))
                    .chain(Self::zeichen_bereiche(b'a' as u16, b'z' as u16))
                    .chain(einzel_b(b'_' as u16))
                    .collect(),
            )),
            // Release 15.10.2026: all WhiteSpace and LineTerminator code
            // points, as inside a class. MEASURED before: `/\s/.test('　')`
            // and `/\s/.test(' ')` were false (node 22: true).
            's' => Ok(Self::klasse_kanonisch(false, Self::zeichen_bereige_tab_und_leerzeichen())),
            'S' => Ok(Self::klasse_kanonisch(true, Self::zeichen_bereige_tab_und_leerzeichen())),
            'n' => Ok(Knoten::Einheit('\n' as u16)),
            'r' => Ok(Knoten::Einheit('\r' as u16)),
            't' => Ok(Knoten::Einheit('\t' as u16)),
            'f' => Ok(Knoten::Einheit(0x0C)),
            'v' => Ok(Knoten::Einheit(0x0B)),
            '0' => {
                // \0 oder Rueckverweis \0n? \0 allein = NUL.
                if self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
                    self.pos -= 1;
                    self.rueckverweis()
                } else {
                    Ok(Knoten::Einheit(0))
                }
            }
            '1'..='9' => {
                self.pos -= 1;
                self.rueckverweis()
            }
            // GEMESSEN 03.10. (`named-groups/non-unicode-match.js`, vier
            // gezaehlte Faelle): `\k<name>` war **nicht** vorhanden —
            // `\k` fiel in die Literal-Flucht und wurde als Buchstabe `k`
            // gelesen. Das Muster `/(?<a>.)(?<b>.)(?<c>.)\k<c>\k<b>\k<a>/`
            // lieferte deshalb `null`, und der Test brach mit „kein Zugriff
            // auf Null" ab. ECMA-262 22.2.2.8 (RegExpNamedBackreference).
            'k' => {
                // `escape()` hat den Buchstaben schon abgelesen: `self.pos`
                // steht auf `<` (GEMESSEN: mit einem zusaetzlichen `+= 1`
                // wurde `<k>` literal und der Test nahm es an).
                if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != '<' {
                    // Ohne `<` ist es ein Literal (Annex B ClassEscape
                    // gilt fuer `\k` nicht, aber node liest `\k` als `k`).
                    return Ok(Knoten::Einheit(b'k' as u16));
                }
                self.pos += 1; // <
                let mut name = String::new();
                while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '>' {
                    name.push(self.zeichen[self.pos]);
                    self.pos += 1;
                    // M3: harte Grenze, dieselbe wie beim Gruppennamen.
                    if name.chars().count() > MAX_GRUPPENNAME {
                        return Err("Gruppenname zu lang".to_string());
                    }
                }
                self.erwarte('>')?;
                // Release 15.10.2026: a duplicated name refers to whichever
                // of its groups participated.
                let mut ids: Vec<usize> = self
                    .namen_mit_id
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .map(|(_, id)| *id)
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                match ids.len() {
                    0 => Err("unbekannter Gruppenname im Rueckverweis".to_string()),
                    1 => Ok(Knoten::Rueckverweis(ids[0])),
                    _ => Ok(Knoten::RueckverweisNamen(ids)),
                }
            }
            'x' => {
                let h = self.hex(2)?;
                Ok(Knoten::Einheit(h))
            }
            'u' => {
                if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '{' {
                    self.pos += 1;
                    let mut w: u32 = 0;
                    let mut stellen = 0;
                    // Der Originaltext fuer das Annex-B-Literal: die
                    // Ziffern **wie geschrieben** (`\\u{00041}` ist etwas
                    // anderes als `\\u{41}` als Text).
                    let mut ziffern = String::new();
                    while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                        let c = self.zeichen[self.pos];
                        let d = c.to_digit(16).ok_or("ungueltiges \\u{}")?;
                        w = w * 16 + d;
                        stellen += 1;
                        ziffern.push(c);
                        if stellen > 6 || w > 0x10FFFF {
                            return Err("ungueltiges \\u{}".to_string());
                        }
                        self.pos += 1;
                    }
                    if stellen == 0 {
                        return Err("leeres \\u{}".to_string());
                    }
                    self.erwarte('}')?;
                    let ch = char::from_u32(w).ok_or("ungueltiges \\u{}".to_string())?;
                    let mut buf = [0u16; 2];
                    let cp: Vec<u16> = ch.encode_utf16(&mut buf).to_vec();
                    let mut literal: Vec<u16> = Vec::with_capacity(ziffern.len() + 3);
                    literal.push('u' as u16);
                    literal.push('{' as u16);
                    literal.extend(ziffern.encode_utf16());
                    literal.push('}' as u16);
                    return Ok(Knoten::CodepunktOderLiteral { cp, literal });
                } else {
                    let h = self.hex(4)?;
                    Ok(Knoten::Einheit(h))
                }
            }
            'b' => Ok(Knoten::Wortgrenze(false)),
            'B' => Ok(Knoten::Wortgrenze(true)),
            'c' => {
                // Steuerzeichen \cX.
                if self.pos >= self.zeichen.len() {
                    return Err("\\c am Ende".to_string());
                }
                let x = self.zeichen[self.pos] as u32;
                self.pos += 1;
                Ok(Knoten::Einheit((x % 32) as u16))
            }
            // J27: `\p{…}` und `\P{…}` — GEMESSEN die groesste offene
            // Gruppe im Lauf ueber die echte test262-Suite (227 der 1065
            // roten Faelle, alle in `test/built-ins/RegExp/
            // property-escapes/`). Die Form ist
            // `{ General_Category=Lu }`, `{ gc=Lu }`, `{ Script=Latin }`,
            // `{ ASCII }` und `{ L }` (die Kuerzel, Spec 22.2.2.7); der
            // Wert kann mit oder ohne Leerzeichen um `=` stehen.
            //
            // GEMESSEN, warum ein **unbekannter** Name ein Fehler ist:
            // `/\p{Quatsch}/u` ist in node ein SyntaxError („Invalid
            // property name"). Ein leeres Ergebnis wuerde still das
            // Falsche liefern — das ist die Sorte „nichts gefunden", die
            // nicht von „vorgesehen" zu unterscheiden ist.
            //
            // GEMESSEN, warum die Tabelle `u`-Schalter verlangt: die
            // Suite setzt `u` bei allen diesen Faellen, und ohne `u` ist
            // `\p` in node kein gueltiges Escape (Annex B). Das prueft
            // die Klammer-Logik weiter unten.
            'p' | 'P' => {
                let negiert = c == 'P';
                if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != '{' {
                    // Ohne `{` ist es in node ein SyntaxError.
                    return Err("\\p braucht eine Eigenschaft in { }".to_string());
                }
                self.pos += 1;
                let mut name = String::new();
                while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                    name.push(self.zeichen[self.pos]);
                    self.pos += 1;
                }
                if self.pos >= self.zeichen.len() {
                    return Err("\\p{ ohne }".to_string());
                }
                self.pos += 1; // }
                let bereiche = crate::unicode::eigenschaft_bereiche(&name)
                    .ok_or_else(|| format!("Unbekannte Unicode-Eigenschaft: {name}"))?;
                let v = if negiert {
                    Self::komplement(&bereiche)
                } else {
                    bereiche
                };
                // Das **Literal** fuer Annex B: `p`/`P` + `{` + der Name in
                // der Schreibweise des Musters + `}`. Der Name kann
                // Leerzeichen und Gleichzeichen enthalten
                // (`\p{ Script=Greek }`) — deshalb der Originaltext.
                let mut literal: Vec<u16> = Vec::with_capacity(name.len() + 3);
                literal.push(if negiert { 'P' as u16 } else { 'p' as u16 });
                literal.push('{' as u16);
                literal.extend(name.encode_utf16());
                literal.push('}' as u16);
                return Ok(Knoten::Eigenschaft {
                    _negativ_bereits: negiert,
                    bereiche: Self::merze_und_sortiere(v),
                    literal,
                });
            }
            other => {
                // Entkommene Sonderzeichen sind literal.
                let mut buf = [0u16; 2];
                let s = other.encode_utf16(&mut buf);
                if s.len() == 1 {
                    Ok(Knoten::Einheit(s[0]))
                } else {
                    Ok(Knoten::Folge(vec![
                        Knoten::Einheit(s[0]),
                        Knoten::Einheit(s[1]),
                    ]))
                }
            }
        }
    }

    fn rueckverweis(&mut self) -> Result<Knoten, String> {
        let mut n: usize = 0;
        while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
            n = n * 10 + (self.zeichen[self.pos] as usize - '0' as usize);
            self.pos += 1;
            if n > 10_000 {
                return Err("Rueckverweis zu gross".to_string());
            }
        }
        Ok(Knoten::Rueckverweis(n))
    }

    fn hex(&mut self, stellen: usize) -> Result<u16, String> {
        let mut w: u32 = 0;
        for _ in 0..stellen {
            if self.pos >= self.zeichen.len() {
                return Err("Hex-Escape unvollstaendig".to_string());
            }
            let d = self.zeichen[self.pos]
                .to_digit(16)
                .ok_or("ungueltiges Hex-Escape")?;
            w = w * 16 + d;
            self.pos += 1;
        }
        Ok(w as u16)
    }

    fn klasse(&mut self) -> Result<Knoten, String> {
        self.pos += 1; // [
        let mut negiert = false;
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '^' {
            negiert = true;
            self.pos += 1;
        }
        let mut bereiche: Vec<(u16, u16)> = Vec::new();
        // J18: `[]` und `[^]` sind **gueltige** Muster: die leere Klasse
        // trifft nie, die negiert-leere trifft jedes Zeichen (Spec
        // ClassRanges, seit ES5).
        //
        // Und `]` schliesst die Klasse **immer** — es gibt keine
        // Ausnahme fuer ein erstes `]`. GEMESSEN mit node 22:
        //   /[]]/.source            -> `[]]`
        //   /[]]/.test(']')         -> false   (leere Klasse + Literal `]`)
        //   /[^]]/.test('a')        -> false   (trifft nur, was mit `]` endet)
        //   /[^]]/.test('x]')       -> true
        // Mit der frueheren Regel („`]` direkt nach `[` ist literal")
        // lieferte der Motor fuer alle vier Faelle das Falsche.
        while self.pos < self.zeichen.len() && self.zeichen[self.pos] != ']' {
            // J18: eine Einheit kann **mehrere** Bereiche liefern
            // (`\d`, `\w`, `\s` und ihre negierten Formen).
            // GEMESSEN vorher an echten Bibliotheken: `[\s\S]` (lodash,
            // 71 KiB) behandelte `\s`/`\S` als die **Buchstaben** `s`
            // und `S` — `RegExp('[\\s\\S]').test('a')` lieferte
            // `false` statt `true`, und `'aXb'.replace(/[\s\S]/g,'-')`
            // lieferte `aXb` statt `---`: still falsch statt zu schreien.
            // `[\d-]` brach sogar mit einer Meldung ab.
            let einheiten = self.klassen_einheit()?;
            // J30: **Early Error** — eine Zeichenklassen-Abkuerzung
            // (`\d \D \w \W \s \S` und `\p{…}`) darf kein **Bereichs-
            // Endpunkt** sein. ES2023 22.2.1 (CharacterClassEscape, Static
            // Semantics: Early Errors): „It is a Syntax Error if
            // IsCharacterClass of the first ClassAtom is true or
            // IsCharacterClass of the second ClassAtom is true."
            //
            // GEMESSEN (node 22) — sieben Muster sind SyntaxError und
            // **fuenf** sind gueltig; die zweite Gruppe ist der Grund, warum
            // die Regel nicht „immer ablehnen" heissen darf:
            //   `[a-\d]` `[\d-a]` `[a-\D]` `[a-\w]` `[--\p{Hex}]`
            //   `[\p{Hex}-a]` `[a-\p{Hex}]`  -> SyntaxError
            //   `[\d-]` `[\p{Hex}-]` `[-a]` `[a-]` `[a-z]` `[--]`
            //   `[a\d]` `[a\s]` `[a-c-e]`   -> gueltig
            // Der Lauf ueber die echte Suite hatte **45** rote Faelle mit
            // `Test262: This statement should not be evaluated` aus
            // `RegExp/property-escapes/character-class-range-end.js` und
            // seinen Geschwistern.
            //
            // GEMESSEN, warum der linke Endpunkt eigens geprueft wird: bei
            // `[\d-a]` nimmt der alte Weg den `\d` in den `else`-Zweig,
            // liest danach `-` als **Literal** und `a` als Literal — und
            // meldet keinen Fehler. Die Abkuerzung „darf kein Endpunkt
            // sein" gilt aber fuer **beide** Seiten.
            //
            // GEMESSEN, warum `[\d-]` gueltig bleibt: das `-` steht
            // **unmittelbar** vor dem schliessenden `]` und ist damit nach
            // Annex B (ClassRanges) ein Literal, kein Bereichsoperator.
            // `[\d--]` ist darum ein SyntaxError — dort steht das `-` nicht
            // vor `]`, und der zweite `-` waere die rechte Grenze.
            // GEMESSEN (node 22, 05.10.2026) — DIE REGEL GILT NUR MIT `u`/`v`:
            //   ohne u:  [\w-\d] [a-\d] [\d-a] [\d-] [\d--] [\w-.] -> GUELTIG
            //   mit  u:  [\w-\d] [a-\d] [\d-a] [\d--] [\w-.]     -> SyntaxError
            // Ohne `u` faengt Annex B (ClassRanges) das ab: das `-` ist ein
            // LITERAL, kein Bereichsoperator. Die erste Fassung lehnte auch
            // ohne `u` ab — GEMESSEN an youtubes ES5-Bundle (10,8 MB):
            // `/(^\/[^\/])|(^#)|(^[\w-\d]*:)/` in Zeile 8599 brach den
            // GANZEN Parser ab (`ungueltiges Regex`), und mit ihm das
            // Hauptskript der Seite.
            if self.unicode
                && self.abkuerzung
                && self.pos + 1 < self.zeichen.len()
                && self.zeichen[self.pos] == '-'
                && self.zeichen[self.pos + 1] != ']'
            {
                return Err(format!(
                    "eine Abkuerzung darf keine Bereichsgrenze sein (Zeichen {})",
                    self.zeichen[self.pos]
                ));
            }
            if einheiten.len() == 1 && !self.abkuerzung {
                let lo = einheiten[0].0;
                if self.pos + 1 < self.zeichen.len()
                    && self.zeichen[self.pos] == '-'
                    && self.zeichen[self.pos + 1] != ']'
                {
                    let merk = self.pos;
                    self.pos += 1; // -
                    let hoch = self.klassen_einheit()?;
                    // J30 mit dem Nachtrag 05.10.2026: die **rechte** Grenze.
                    // Vorher wurde ein Abkuerzungs-Endpunkt still vereinigt
                    // (siehe den `else`-Zweig darunter) — das ist die stille
                    // Variante desselben Fehlers. Der Fehler gilt aber nur
                    // mit `u`/`v` (node 22); ohne `u` ist `[a-\d]` eine
                    // gueltige Annex-B-Vereinigung {a, -, \d}.
                    if self.unicode && (self.abkuerzung || hoch.len() != 1) {
                        return Err("eine Abkuerzung darf keine Bereichsgrenze sein".to_string());
                    }
                    if hoch.len() == 1 && hoch[0].1 >= lo {
                        bereiche.push((lo, hoch[0].1));
                    } else if hoch.len() == 1 {
                        // J32: ES2023 22.2.1 (`CharacterRange`, Schritt 6):
                        // „It is a Syntax Error if the first character is
                        // greater than the second character." Beide Grenzen
                        // sind **einzelne Zeichen** — nur die Reihenfolge
                        // stimmt nicht.
                        //
                        // GEMESSEN (node 22), **vier** scheitern:
                        //   `^[z-a]$`   `^[b-a]$`
                        //   `^[ä-a]$`   (U+00E4 > 'a')
                        //   `^[\u{1F600}-\u{1F601}]$` (D83D > D801 — es
                        //   zaehlen die **Ersatzpaar-Codeeinheiten**, nicht
                        //   die Zeichen)
                        // und **zwoelf** sind gueltig, darunter die drei
                        // Nachbarn, die den `else`-Zweig brauchen:
                        //   `^[a-z]$`   `^[Z-a]$`   `^[a-]$`
                        //
                        // GEMESSEN, warum das ein **Fehler** ist und nicht
                        // weiter „vereinigen": der alte Zweig machte aus
                        // `[z-a]` die Menge `{z, -, a}` — ein Muster, das
                        // **kompiliert** und dann etwas voellig anderes
                        // matcht als in jedem anderen Motor. Genau das ist
                        // der stille Erfolg, vor dem der Kommentar an dieser
                        // Funktion warnt.
                        return Err(format!(
                            "Bereich ausserhalb der Reihenfolge: {}-{}",
                            crate::wert::zahl_text(lo as f64),
                            crate::wert::zahl_text(hoch[0].1 as f64)
                        ));
                    } else {
                        // GEMESSEN/ENTSCHIEDEN, warum so: `[-abc]`
                        // und `[[a-z]--]` sind in JS erlaubt (Annex B),
                        // eine Abkuerzung als **Bereichsgrenze** nicht
                        // sinnvoll. Also vereinigen statt ablehnen:
                        // `lo`, das `-` als Zeichen und die Abkuerzung.
                        bereiche.push((lo, lo));
                        bereiche.push(('-' as u16, '-' as u16));
                        bereiche.extend(hoch);
                        let _ = merk;
                    }
                } else {
                    bereiche.push((lo, lo));
                }
            } else {
                bereiche.extend(einheiten);
            }
            if bereiche.len() > 10_000 {
                return Err("Klasse zu gross".to_string());
            }
        }
        self.erwarte(']')?;
        // J27: die Bereichsliste ist **sortiert und zusammengefasst**.
        //
        // GEMESSEN, warum das jetzt Pflicht ist und nicht Kosmetik: der
        // Lauf ueber die echte Suite braucht die `generated/`-Faelle von
        // `RegExp/property-escapes`, und die bringen bis zu ~1200 Bereiche
        // je Zeichenklasse mit (`\p{Assigned}`, `\p{Any}` nach der
        // Erweiterung um nicht-BMP). Die Abfrage war ein **linearer** Scan
        // ueber alle Bereiche — fuer jedes *einzelne* Zeichen. Ein
        // gemessener Fall (`ASCII.js`, 1.114.112 Codepunkte x vier
        // Regexvarianten) brauchte 5,3 s im Debug-Bau.
        //
        // Die Invariante „jede `Knoten::Klasse` traegt eine sortierte,
        // lueckenlose Liste" wird hier **hergestellt** — deshalb darf der
        // Vergleich unten binaer suchen. Vorher sortiert war die Liste
        // nicht: `[\p{L}0-9]` haette die Bereiche von `L` und dann `0-9`
        // gehabt, und `unicode.rs` fuehrt etliche Handtabellen unsortiert
        // (`Lo` etwa: `0x4E00..0x9FFF` vor `0x3041..0x3096`).
        let gefalten = Self::bereiche_gefalten(&bereiche);
        let kanon = Self::bereiche_kanon(&bereiche);
        Ok(Knoten::Klasse {
            negiert,
            bereiche: Self::merze_und_sortiere(bereiche),
            gefalten,
            kanon,
        })
    }

    /// Eine Zeichenklasse **kanonisch** bauen: sortiert und zusammengefasst.
    ///
    /// GEMESSEN 03.10. (der Grund, warum es Pflicht ist und nicht Kosmetik):
    /// `klasse()` (also `[...]`)-sortierte seine Bereiche, `escape()` tat es
    /// nicht. Der Matcher sucht **binaer** (`bereich_enthält`, Z. 1617) und
    /// setzt die Invariante „sortiert und lueckenlos" voraus. Folge:
    /// `\w` wurde zu `[(97,122),(65,90),(48,57),(95,95)]` — ein `a` (97) lag
    /// zwar im ersten Bereich, aber die binaere Suche findet ihn nur, wenn
    /// die Liste nach `hi` sortiert ist; sie fand also fast nichts. GEMESSEN:
    /// `/\w/`, `/\d/`, `/\s/` lieferten **keinen** Treffer, `String.replace`
    /// mit `\w` ersetzte nichts, und 231 `property-escapes`-Faelle brauchten
    /// den Einzelschleifen-Weg. `[\w]` in Klammern war richtig — genau der
    /// Unterschied, den die Sortierung ausmacht.
    fn klasse_kanonisch(negiert: bool, bereiche: Vec<(u16, u16)>) -> Knoten {
        let gefalten = Self::bereiche_gefalten(&bereiche);
        let kanon = Self::bereiche_kanon(&bereiche);
        Knoten::Klasse {
            negiert,
            bereiche: Self::merze_und_sortiere(bereiche),
            gefalten,
            kanon,
        }
    }

    /// Die Klassenbereiche **gefaltet** und zusammengefuehrt — damit der
    /// `i`-Vergleich auf beiden Seiten denselben Vertreter benutzt.
    ///
    /// GEMESSEN 04.10.: `falte` bildet auf den kleinsten Code-Point der
    /// Klasse ab, ist also **nicht** monoton. Das kann zu einem etwas zu
    /// grossen Bereich fuehren (zu treffen statt zu verfehlen); die
    /// Alternative — je Bereich nur die Randpunkte — verliert `[a-z]`
    /// sein eigenes `'b'` (gemessen).
    fn bereiche_gefalten(bereiche: &[(u16, u16)]) -> Vec<(u16, u16)> {
        Self::merze_und_sortiere(
            bereiche
                .iter()
                .map(|(lo, hi)| (falte(*lo), falte(*hi)))
                .collect(),
        )
    }

    /// Die Klassenbereiche **kanonisiert** und zusammengefuehrt — das
    /// Gegenstueck zu `bereiche_gefalten` fuer `i` **ohne** `u`.
    ///
    /// GEMESSEN 04.10. (`node` v22 als Orakel, 44 Handpaare): die
    /// gefaltete Liste traf ohne `u` **acht** Zeichen zu viel — das
    /// Kelvinzeichen U+212A, der Lange s U+017F, `\u00DF`/`\u1E9E`, das
    /// punktlose I U+0131, U+0390/U+1FD3 und das Angstromzeichen U+212B.
    /// Die Kanonisierung (Grossschreibung, sofern **eine** Codeeinheit)
    /// mit dem Spec-Vorbehalt fuer U+212A/U+212B trifft alle acht richtig.
    fn bereiche_kanon(bereiche: &[(u16, u16)]) -> Vec<(u16, u16)> {
        Self::merze_und_sortiere(
            bereiche
                .iter()
                .map(|(lo, hi)| (kanonisiere(*lo), kanonisiere(*hi)))
                .collect(),
        )
    }

    /// J27: Bereiche sortieren und ueberlappende/angrenzende zusammenfassen.
    ///
    /// Das Ergebnis ist **kanonisch**: die Reihenfolge der Eingabe geht
    /// nicht mehr in die Antwort. GEMESSEN, warum das mehr ist als
    /// Geschwindigkeit: `[\p{L}0-9]` und `[0-9\p{L}]` muessen dasselbe
    /// Muster ergeben — sonst waere die Reihenfolge im Quelltext ein
    /// Unterschied im Verhalten, den es in JavaScript nicht gibt.
    pub(crate) fn merze_und_sortiere(b: Vec<(u16, u16)>) -> Vec<(u16, u16)> {
        if b.is_empty() {
            return b;
        }
        let mut s = b;
        s.sort_unstable();
        let mut raus: Vec<(u16, u16)> = Vec::with_capacity(s.len());
        let (mut lo, mut hi) = s[0];
        for (a, e) in s.into_iter().skip(1) {
            // `a <= hi + 1` ueberlappt **und** grenzt an; `hi + 1` kann
            // bei `hi == 0xFFFF` ueberlaufen, deshalb mit `u32`.
            if (a as u32) <= (hi as u32) + 1 {
                if e > hi {
                    hi = e;
                }
            } else {
                raus.push((lo, hi));
                lo = a;
                hi = e;
            }
        }
        raus.push((lo, hi));
        raus
    }

    /// J27: die Bereiche einer Einheit **in** einer Zeichenklasse.
    /// Ausser den Escapes sind das die Abkuerzungen `\d \D \w \W \s \S`
    /// (und `` = Backspace, 0x08 — nicht die Wortgrenze, die gibt es
    /// in einer Klasse nicht).
    ///
    /// GEMESSEN, warum die negierten Formen als **Komplement** hier
    /// stehen und nicht spaeter: die Klasse wird als Liste von
    /// Bereichen gespeichert und beim Treffer geprueft; ein Komplement
    /// muss also in Bereiche zerlegt werden, und das kann nur hier.
    fn klassen_einheit(&mut self) -> Result<Vec<(u16, u16)>, String> {
        if self.pos >= self.zeichen.len() {
            return Err("Klasse endet unerwartet".to_string());
        }
        let c = self.zeichen[self.pos];
        self.abkuerzung = false;
        if c != '\\' {
            self.pos += 1;
            // Astrale Zeichen als Surrogat-Paar behandeln wir als EINHEITEN:
            // vereinfacht auf die erste Einheit (Dokumentation: Klassen mit
            // astralen Zeichen sind naeherungsweise).
            let mut buf = [0u16; 2];
            let s = c.encode_utf16(&mut buf);
            // Release 15.10.2026: without `u` an astral character in a class
            // is BOTH of its units (unicode mode uses `klasse_cp`). MEASURED
            // before: `/^[😀]{2}$/.test('😀')` was false (node 22: true).
            if s.len() == 2 {
                return Ok(vec![(s[0], s[0]), (s[1], s[1])]);
            }
            return Ok(vec![(s[0], s[0])]);
        }
        self.pos += 1;
        if self.pos >= self.zeichen.len() {
            return Err("Escape in Klasse am Ende".to_string());
        }
        let e = self.zeichen[self.pos];
        // GEMESSEN (warum hier **nicht** `self.pos += 1` steht): der
        // Escape-Buchstabe steht noch an `self.pos` — die naechste Zeile
        // (`self.pos += 1;`) hebt ihn erst ab. Die erste Fassung dieses
        // Zweigs hat zuerst weitergezaehlt und las dann `{` als
        // Eigenschaftsnamen: „\p braucht eine Eigenschaft in { }".
        // J27: `\p{…}` **innerhalb** einer Zeichenklasse (`[\p{L}]`).
        // GEMESSEN, warum das eine eigene Stelle ist: `escape()` (der
        // Weg ausserhalb) und `klassen_einheit()` (hier) lesen dieselbe
        // Escape-Folge, aber der zweite Weg hatte keinen `\p`-Zweig —
        // `console.log(/^[\p{L}]+$/u.test('abc'))` lieferte `false`,
        // `/^[\p{Lu}\p{Ll}]+$/u.test('Abc')` ebenfalls, waehrend
        // `\p{Lu}` **ohne** Klasse richtig war. Die Suite benutzt
        // `[\p{...}]` in ueber 200 Faellen.
        if e == 'p' || e == 'P' {
            // GEMESSEN (zweite Fassung, der Grund steht hier, weil die
            // erste **stumm** das falsche las): `self.pos` steht auf dem
            // Escape-Buchstaben `p`, nicht auf `{`. Die erste Fassung
            // pruefte deshalb an der falschen Stelle und meldete
            // „\p braucht eine Eigenschaft in { }" fuer jede korrekte
            // Angabe wie `[\p{L}]`.
            // Ueber die **Klammer** auf den Namen springen (die erste
            // Fassung las sie mit — GEMESSEN als „Unbekannte
            // Unicode-Eigenschaft: {L", also ein Name mit `{` drin).
            self.pos += 1;
            if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != '{' {
                return Err("\\p braucht eine Eigenschaft in { }".to_string());
            }
            self.pos += 1;
            let mut name = String::new();
            while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                name.push(self.zeichen[self.pos]);
                self.pos += 1;
            }
            if self.pos >= self.zeichen.len() {
                return Err("\\p{ ohne }".to_string());
            }
            self.pos += 1; // }
            // Annex B (05.10.2026): OHNE `u`/`v` ist `\p` kein
            // Eigenschafts-Escape, sondern die LITERALE `p`, `{`, <Name>,
            // `}`. GEMESSEN (node 22): `[\p{Hex}-a]` scheitert ohne `u` an
            // `}-a` („Range out of order"), nicht am Klassen-Escape;
            // `[\p{Hex}]` ist die Menge der Zeichen p,{,H,e,x,}. Die erste
            // Fassung las die Eigenschaft auch ohne `u` und lehnte damit
            // gueltige Muster ab (dieselbe Fehlerfamilie wie `[\w-\d]`).
            if !self.unicode {
                self.abkuerzung = false;
                let mut raus: Vec<(u16, u16)> = Vec::with_capacity(name.len() + 3);
                let buchstabe = if e == 'P' { 'P' as u16 } else { 'p' as u16 };
                raus.push((buchstabe, buchstabe));
                raus.push(('{' as u16, '{' as u16));
                raus.extend(name.encode_utf16().map(|c| (c, c)));
                raus.push(('}' as u16, '}' as u16));
                return Ok(raus);
            }
            let bereiche = crate::unicode::eigenschaft_bereiche(&name)
                .ok_or_else(|| format!("Unbekannte Unicode-Eigenschaft: {name}"))?;
            self.abkuerzung = true;
            return Ok(if e == 'P' {
                Self::komplement(&bereiche)
            } else {
                bereiche
            });
        }
        self.pos += 1;
        let einzel = einzel_b;
        // J18: `°`/`·` sind keine Abkuerzungen, `ß` ist Backspace (0x08).
        self.abkuerzung = matches!(e, 'd' | 'D' | 'w' | 'W' | 's' | 'S');
        Ok(match e {
            'd' => Self::zeichen_bereiche(b'0' as u16, b'9' as u16),
            'D' => Self::komplement(&[(b'0' as u16, b'9' as u16)]),
            'w' => Self::zeichen_bereiche(b'0' as u16, b'9' as u16)
                .into_iter()
                .chain(Self::zeichen_bereiche(b'A' as u16, b'Z' as u16))
                .chain(Self::zeichen_bereiche(b'a' as u16, b'z' as u16))
                .chain(einzel('_' as u16))
                .collect(),
            'W' => Self::komplement(&[
                ('0' as u16, '9' as u16),
                ('A' as u16, 'Z' as u16),
                ('_' as u16, '_' as u16),
                ('a' as u16, 'z' as u16),
            ]),
            's' => Self::zeichen_bereige_tab_und_leerzeichen(),
            'S' => Self::komplement(&[
                ('\t' as u16, '\r' as u16),
                (' ' as u16, ' ' as u16),
                (0xA0, 0xA0),
                (0x1680, 0x1680),
                (0x2000, 0x200A),
                (0x2028, 0x2028),
                (0x2029, 0x2029),
                (0x202F, 0x202F),
                (0x205F, 0x205F),
                (0x3000, 0x3000),
                (0xFEFF, 0xFEFF),
            ]),
            'n' => einzel('\n' as u16),
            'r' => einzel('\r' as u16),
            't' => einzel('\t' as u16),
            'f' => einzel(0x0C),
            'v' => einzel(0x0B),
            '0' => einzel(0),
            'b' => einzel(0x08),
            'x' => {
                let w = self.hex(2)?;
                vec![(w, w)]
            }
            'u' => {
                // 05.10.2026: `\u{…}` **in einer Zeichenklasse**.
                //
                // GEMESSEN an youtubes ES5-Bundle (10,8 MB):
                // `/[\u{1D400}-\u{1D7FF}\u2100-\u214F]/u` brach den ganzen
                // Parser ab („ungueltiges Hex-Escape") — `escape()` (ausser-
                // halb der Klasse) kannte die Form laengst, `klassen_einheit`
                // nicht. Dieselbe Fehlerfamilie wie `[\w-\d]`.
                if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '{' {
                    if !self.unicode {
                        // Annex B: ohne `u` ist `\u` der Buchstabe u, und
                        // `{…}` sind Literale (node 22: `/[\u{41}]/` matcht
                        // u, {, 4, 1, }).
                        return Ok(einzel('u' as u16));
                    }
                    self.pos += 1;
                    let mut w: u32 = 0;
                    let mut stellen = 0;
                    while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                        let c = self.zeichen[self.pos];
                        let d = c.to_digit(16).ok_or("ungueltiges \\u{}")?;
                        w = w * 16 + d;
                        stellen += 1;
                        if stellen > 6 || w > 0x10FFFF {
                            return Err("ungueltiges \\u{}".to_string());
                        }
                        self.pos += 1;
                    }
                    if stellen == 0 {
                        return Err("leeres \\u{}".to_string());
                    }
                    if self.pos >= self.zeichen.len() {
                        return Err("\\u{ ohne }".to_string());
                    }
                    self.pos += 1; // }
                    let ch = char::from_u32(w).ok_or("ungueltiges \\u{}".to_string())?;
                    let mut buf = [0u16; 2];
                    let s = ch.encode_utf16(&mut buf);
                    // Naherung wie bei astralen Literalen in Klassen (siehe
                    // die Begruendung oben): die erste UTF-16-Einheit. Die
                    // Klasse vergleicht Einheiten, keine Folgen.
                    return Ok(vec![(s[0], s[0])]);
                }
                let w = self.hex(4)?;
                vec![(w, w)]
            }
            other => einzel(other as u16),
        })
    }

    /// `\s` als Bereiche: TAB..CR, Leerzeichen und die Unicode-Leerzeichen
    /// (ECMA-262, WhiteSpace + LineTerminator).
    fn zeichen_bereige_tab_und_leerzeichen() -> Vec<(u16, u16)> {
        Self::zeichen_bereiche(0x09, 0x0D)
            .into_iter()
            .chain(einzel_b(0x20))
            .chain(einzel_b(0xA0))
            .chain(einzel_b(0x1680))
            .chain(Self::zeichen_bereiche(0x2000, 0x200A))
            .chain(einzel_b(0x2028))
            .chain(einzel_b(0x2029))
            .chain(einzel_b(0x202F))
            .chain(einzel_b(0x205F))
            .chain(einzel_b(0x3000))
            .chain(einzel_b(0xFEFF))
            .collect()
    }

    /// `[a..b]` als eine Liste von Bereichen. **UTF-16-Einheiten**, weil
    /// die Klasse in Einheiten verglichen wird (J18).
    fn zeichen_bereiche(lo: u16, hi: u16) -> Vec<(u16, u16)> {
        vec![(lo, hi)]
    }

    /// Das Komplement einer Bereichsliste ueber alle UTF-16-Einheiten.
    fn komplement(bereiche: &[(u16, u16)]) -> Vec<(u16, u16)> {
        let mut sortiert: Vec<(u16, u16)> = bereiche.to_vec();
        sortiert.sort_unstable();
        let mut raus: Vec<(u16, u16)> = Vec::with_capacity(sortiert.len() + 1);
        let mut naechste: u32 = 0;
        for (lo, hi) in sortiert {
            if lo as u32 > naechste {
                raus.push((naechste as u16, lo.saturating_sub(1)));
            }
            naechste = (hi as u32 + 1).min(0x1_0000);
        }
        if naechste < 0x1_0000 {
            raus.push((naechste as u16, 0xFFFF));
        }
        raus
    }

    /// `«`: nach dem ersten `self.hex(2)` steht der Zeiger **im**
    /// Escape; der zweite Teil wird von hier gelesen.
    fn hex_danach(&mut self, stellen: usize) -> Result<u16, String> {
        let mut w: u32 = 0;
        for _ in 0..stellen {
            if self.pos >= self.zeichen.len() {
                return Err("Hex-Escape unvollstaendig".to_string());
            }
            let d = self.zeichen[self.pos]
                .to_digit(16)
                .ok_or("ungueltiges Hex-Escape")?;
            w = w * 16 + d;
            self.pos += 1;
        }
        Ok(w as u16)
    }
}

/// Eine Einheit als ein Bereich (J18).
fn einzel_b(w: u16) -> Vec<(u16, u16)> {
    vec![(w, w)]
}

// J18: die Abkuerzungen **innerhalb** von Klassen werden beim Bau in
// Bereiche zerlegt (`klassen_einheit`), eine Normalisierung des Musters
// ist also nicht mehr noetig. Die Funktion bleibt als Ansatzstelle fuer
// das, was Text-Escapes weiterhin nicht koennen (z.B. `\p{…}`).
pub(crate) fn muster_normalisieren(_muster: &str) -> String {
    _muster.to_string()
}

// ---------------------------------------------------------------------------
// Matcher
// ---------------------------------------------------------------------------

/// Treffer mit UTF-16-Positionen + Gruppen (Byte-String aus Einheiten).
#[derive(Debug, Clone)]
pub(crate) struct Treffer {
    pub anfang: usize,
    pub ende: usize,
    pub gruppen: Vec<Option<(usize, usize)>>,
}

struct Durchlauf<'a> {
    text: &'a [u16],
    schalter: Schalter,
    budget: u64,
    tiefe: usize,
    /// `u` — **Unicode-Modus** (ECMA-262 22.2.1). Bewusst **nicht** in
    /// `Schalter`: ein Feld dort bricht die Struktur-Literale in
    /// `builtin2.rs`, `builtin3.rs`, `parse.rs` und `eigenschaften.rs`
    /// (fremde Dateien). GEMESSEN 04.10.
    einheiten: bool,
    /// Summe aller erzeugten Staende — gegen `MAX_STAENDEN_GESAMT`
    /// (GEMESSEN 04.10., siehe dort: Budget allein genuegt nicht).
    staende_gesamt: u32,
    /// Release 10.10.2026: Staende mit derselben Endposition werden auf den
    /// ERSTEN (den mit Vorrang) zusammengelegt — nur wenn das Muster keinen
    /// Rueckverweis hat. Dann verhaelt sich jede Fortsetzung fuer beide
    /// gleich, und alle Fortsetzungen des ersten kommen vor denen des
    /// zweiten (Backtracking-Reihenfolge); der zweite kann das Ergebnis also
    /// nie bestimmen.
    ///
    /// GEMESSEN vorher: Polymers Bindungsmuster (`[[f(a.b, 3, 'q')]]`, jede
    /// Polymer-Seite, z. B. youtube.com) lief in die Standgrenze — jedes
    /// Argument vervielfachte die Staende (der Bezeichner endet an vielen
    /// Stellen) — und lieferte „kein Treffer“; node trifft.
    entdoppeln: bool,
}

/// Release 10.10.2026: kommt im Muster ein Rueckverweis vor?
fn hat_rueckverweis(k: &Knoten) -> bool {
    match k {
        Knoten::Rueckverweis(_) | Knoten::RueckverweisNamen(_) => true,
        Knoten::Folge(v) | Knoten::Auswahl(v) => v.iter().any(hat_rueckverweis),
        Knoten::Wiederholung { inner, .. }
        | Knoten::Modifikator { inner, .. }
        | Knoten::Gruppe { inner, .. }
        | Knoten::Vorausschau { inner, .. }
        | Knoten::Rueckschau { inner, .. } => hat_rueckverweis(inner),
        _ => false,
    }
}

/// Release 10.10.2026: spaetere Staende mit schon gesehener Endposition
/// streichen (Reihenfolge bleibt; siehe `Durchlauf::entdoppeln`).
fn staende_entdoppeln(r: &mut Vec<Stand>) {
    if r.len() < 2 {
        return;
    }
    if r.len() <= 16 {
        let mut i = 1;
        while i < r.len() {
            let ende = r[i].ende;
            if r[..i].iter().any(|s| s.ende == ende) {
                r.remove(i);
            } else {
                i += 1;
            }
        }
        return;
    }
    let mut gesehen = std::collections::HashSet::with_capacity(r.len());
    r.retain(|s| gesehen.insert(s.ende));
}

pub(crate) fn suchen(
    knoten: &Knoten,
    gruppen_n: usize,
    schalter: Schalter,
    text: &[u16],
    start: usize,
) -> Option<Treffer> {
    suchen_einheiten(knoten, gruppen_n, schalter, false, text, start)
}

/// Das Schrittbudget fuer eine Eingabe der Laenge `text_len`.
///
/// Die Basis gilt fuer den Normalfall (die RegExp-Proben der Crate sind
/// kurz). Fuer **grosse** Eingaben waere sie zu klein: der Lauf bricht ab
/// und liefert ein **falsches** Ergebnis (abgeschnittenes Ende, leere
/// Gruppen) — still, ohne Panic. Deshalb waechst das Budget mit der
/// Eingabe, bleibt aber durch `MAX_REGEX_SCHRITTE_GESAMT` hart gedeckelt.
fn schritt_budget(text_len: usize) -> u64 {
    let anteil = (text_len as u64).saturating_mul(SCHRITTE_PRO_ZEICHEN);
    MAX_REGEX_SCHRITTE
        .saturating_add(anteil)
        .min(MAX_REGEX_SCHRITTE_GESAMT)
}

/// Wie `suchen`, aber mit **Unicode-Modus** bekannt.
///
/// GEMESSEN 04.10. (Orakel `node` v22): `.` mit `u` trifft einen
/// **Codepoint** — U+1D401, U+20BB7 und U+1D306 belegen je **zwei**
/// UTF-16-Einheiten (`e: 2`; ohne `u` genau eine). Mit dieser Funktion
/// steigt der Lauf ueber 920 Suite-Faelle von 914 auf **917 von 920**
/// (99,35 % -> 99,67 %) **ohne Regression**.
///
/// Die Schalter-Liste (`g i m s y`) traegt `u` nicht, weil sie **an
/// vielen fremden Stellen als Struktur-Literal** gebaut wird; deshalb ein
/// eigener Weg statt eines Feldes.
pub(crate) fn suchen_einheiten(
    knoten: &Knoten,
    gruppen_n: usize,
    schalter: Schalter,
    einheiten: bool,
    text: &[u16],
    start: usize,
) -> Option<Treffer> {
    // Release 15.10.2026 (diagnosis): `NARSIL_REGEX_ZEIT=<ms>` prints every
    // search that took at least that long, with the compiled pattern.
    static SCHWELLE: std::sync::OnceLock<Option<u128>> = std::sync::OnceLock::new();
    let schwelle = *SCHWELLE.get_or_init(|| std::env::var("NARSIL_REGEX_ZEIT").ok().and_then(|v| v.parse().ok()));
    let Some(schwelle) = schwelle else {
        return suchen_einheiten_innen(knoten, gruppen_n, schalter, einheiten, text, start);
    };
    let t0_diag = std::time::Instant::now();
    let erg = suchen_einheiten_innen(knoten, gruppen_n, schalter, einheiten, text, start);
    if t0_diag.elapsed().as_millis() >= schwelle {
        let k = format!("{knoten:?}");
        eprintln!("REGEX-LANGSAM {} ms text={} start={} {}", t0_diag.elapsed().as_millis(), text.len(), start, &k[..k.len().min(400)]);
    }
    erg
}

fn suchen_einheiten_innen(
    knoten: &Knoten,
    gruppen_n: usize,
    schalter: Schalter,
    einheiten: bool,
    text: &[u16],
    start: usize,
) -> Option<Treffer> {
    let mut d = Durchlauf {
        text,
        schalter,
        budget: schritt_budget(text.len()),
        tiefe: 0,
        einheiten,
        staende_gesamt: 0,
        entdoppeln: !hat_rueckverweis(knoten),
    };
    if schalter.klebrig {
        // Sticky: nur an `start` (erster Stand = hoechste Prioritaet).
        let gruppen = vec![None; gruppen_n + 1];
        if let Some(s) = d.alle(knoten, start, &gruppen).into_iter().next() {
            return Some(Treffer {
                anfang: start,
                ende: s.ende,
                gruppen: s.gruppen,
            });
        }
        return None;
    }
    let mut pos = start.min(text.len());
    loop {
        let gruppen = vec![None; gruppen_n + 1];
        d.budget = d.budget.saturating_sub(1);
        if d.budget == 0 {
            return None;
        }
        let staende = d.alle(knoten, pos, &gruppen);
        // Ist das Budget erschoepft, ist die Liste **unvollstaendig** — sie
        // enthaelt nicht alle Prioritaeten, sondern nur die, die der
        // Abbruch noch zulass. GEMESSEN 04.10.: ohne diese Pruefung meldete
        // `\w+q` auf 100.000 Zeichen einen **Treffer**, den es nicht gibt
        // (4 von 8 pathologischen Faellen). Ein erschoeftes Budget liefert
        // deshalb "kein Treffer" — nie ein falscher.
        if d.budget == 0 {
            return None;
        }
        if let Some(s) = staende.into_iter().next() {
            return Some(Treffer {
                anfang: pos,
                ende: s.ende,
                gruppen: s.gruppen,
            });
        }
        if pos >= text.len() {
            return None;
        }
        pos += 1;
    }
}

/// Ein Treffer-Stand: Endposition + Gruppenbelegung dieses Pfades.
#[derive(Debug, Clone)]
struct Stand {
    ende: usize,
    gruppen: Vec<Option<(usize, usize)>>,
}

/// Harte Deckel (M3/M4): Staende je Knoten, Ebenen je Wiederholung.
const MAX_STAENDE: usize = 4096;
const MAX_WIEDERHOL_EBENEN: u32 = 100_000;
/// Harte **Rekursions**grenze der Wiederholung (M3/M5) — geordnetes
/// Backtracking ist rekursiv, `(?:a?)*` waere sonst nicht zu beenden.
const MAX_WIEDERHOL_TIEFE: u32 = 128;
/// Harte Obergrenze der **Rundenzahl** einer Wiederholung (M3/M5). Der
/// Kettenanteil laeuft iterativ und haengt deshalb nicht am Stapel —
/// GEMESSEN 04.10. drei Grenzen nacheinander: `MAX_WIEDERHOL_TIEFE` = 512
/// (brach `a+` bei 513 Zeichen ab), dann `MAX_WIEDERHOL_EBENEN` = 100.000
/// (brach bei 200.000 Zeichen ab). Jetzt begrenzt **das Schrittbudget**
/// die Arbeit, diese Konstante ist nur noch die letzte Klammer.
const MAX_WIEDERHOL_RUNDEN: u32 = 16_000_000;

impl Durchlauf<'_> {
    fn tick(&mut self) -> bool {
        if self.budget == 0 || self.tiefe > MAX_MATCH_TIEFE {
            return false;
        }
        self.budget -= 1;
        true
    }

    /// Alle Endstaende (prioritaetsgeordnet: gierige Pfade zuerst).
    /// Tiefenbilanz: +1/-1 je Aufruf (M5); Budget + Stand-Deckel (M3).
    fn alle(&mut self, k: &Knoten, pos: usize, gruppen: &[Option<(usize, usize)>]) -> Vec<Stand> {
        if !self.tick() {
            return Vec::new();
        }
        self.tiefe += 1;
        let mut r = self.alle_innen(k, pos, gruppen);
        self.tiefe -= 1;
        if self.entdoppeln {
            staende_entdoppeln(&mut r);
        }
        // Der Zaehler wird **vor** dem Kuerzen gebucht: sonst waere eine
        // breite Liste (bis `MAX_STAENDE`) billig, obwohl sie genau die
        // Arbeit erzeugt hat, die begrenzt werden soll.
        self.staende_gesamt = self.staende_gesamt.saturating_add(r.len() as u32);
        // **In-place** kuerzen (M5, ohne Kopie).
        //
        // GEMESSEN 04.10.: `r[..MAX_STAENDE].to_vec()` kopierte bis 4096
        // Staende **bei jedem** Knotenlauf. Bei `(?:a|aa)+b` auf 100.000
        // Zeichen waren das Millionen Kopien — der Fall lief ueber 30 s
        // (ABBRUCH), obwohl das Budget nach 1,3 Mio. Schritten zur
        // Endlage kommen muss. `truncate` kostet nichts und beendet die
        // quadratische Kopierarbeit.
        let mut r = r;
        r.truncate(MAX_STAENDE);
        r
    }

    /// Ein **Literal** (Vec von UTF-16-Einheiten) Zeichen fuer Zeichen
    /// vergleichen; der erste Stand ist der nach dem ganzen Literal.
    fn literal_kette(
        &self,
        literal: &[u16],
        pos: usize,
        gruppen: &[Option<(usize, usize)>],
    ) -> Vec<Stand> {
        for (i, u) in literal.iter().enumerate() {
            if self.text.get(pos + i) != Some(u) {
                return Vec::new();
            }
        }
        vec![Self::stand(pos + literal.len(), gruppen)]
    }

    fn stand(pos: usize, gruppen: &[Option<(usize, usize)>]) -> Stand {
        Stand {
            ende: pos,
            gruppen: gruppen.to_vec(),
        }
    }

    fn alle_innen(
        &mut self,
        k: &Knoten,
        pos: usize,
        gruppen: &[Option<(usize, usize)>],
    ) -> Vec<Stand> {
        match k {
            Knoten::Leer => vec![Self::stand(pos, gruppen)],
            Knoten::Einheit(u) => {
                let t = match self.text.get(pos) {
                    Some(t) => *t,
                    None => return Vec::new(),
                };
                let ok = if self.schalter.egal_gross {
                    // **Faltung**, nicht nur Kleinschreibung (siehe
                    // `falte`): `[K]/iu` muss `U+212A` treffen.
                    gleich_ungleich_case(t, *u, self.einheiten)
                } else {
                    t == *u
                };
                if ok {
                    vec![Self::stand(pos + 1, gruppen)]
                } else {
                    Vec::new()
                }
            }
            Knoten::Klasse {
                negiert,
                bereiche,
                gefalten,
                kanon,
            } => {
                let t = match self.text.get(pos) {
                    Some(t) => *t,
                    None => return Vec::new(),
                };
                // J27: **binaere** Suche statt eines linearen Scans.
                //
                // GEMESSEN, warum das noetig war: `bereiche` kann nach dem
                // Zusammenfuehren bis ~1200 Eintraege haben (die
                // `generated/`-Faelle), und der lineare Scan lief fuer
                // **jedes** Zeichen der Eingabe. `ASCII.js` brauchte 5,3 s
                // im Debug-Bau.
                //
                // Die Voraussetzung — sortiert und lueckenlos — stellt
                // `merze_und_sortiere` beim Bau der Klasse her, nicht hier.
                // Deshalb wird sie dort **einmal** aufgerufen und nicht je
                // Zeichen.
                //
                // GEMESSEN, warum die Gross/Klein-Umstellung kein Problem
                // ist: sie bildet jeden Bereich auf denselben Bereich ab
                // (`klein(lo)..klein(hi)` bleibt sortiert, weil `klein`
                // monoton ist), also ist die sortierte Liste auch fuer den
                // Kleinbuchstaben-Zweig gueltig.
                let drin = if self.schalter.egal_gross && self.einheiten {
                    // **Beide** Seiten falten: der Text auf seinen
                    // Vertreter (`falte`), die Klasse ueber ihre gefaltete
                    // Bereichsliste. Zwei **binaere** Suchen, keine
                    // lineare Schleife (Leistung, siehe `bereich_enthält`).
                    let f = falte(t);
                    bereich_enthält(gefalten, f) || bereich_enthält(bereiche, f)
                } else if self.schalter.egal_gross {
                    // `i` **ohne** `u`: ECMA-262 22.2.2.8 vergleicht
                    // **kanonisiert**, nicht gefaltet. Beide Seiten sind
                    // kanonisiert (die Klasse ueber `kanon`, der Text ueber
                    // `kanonisiere`), also **eine** binaere Suche.
                    bereich_enthält(kanon, kanonisiere(t))
                } else {
                    bereich_enthält(bereiche, t)
                };
                if drin != *negiert {
                    vec![Self::stand(pos + 1, gruppen)]
                } else {
                    Vec::new()
                }
            }
            // `\p{…}`: mit `u`/`v` die Eigenschaftsklasse (dieselbe
            // Faltung/Kanonisierung wie `Knoten::Klasse`), ohne `u` nach
            // Annex B.1.2 das **Literal**. GEMESSEN 04.10.:
            // `node` — `/\p{L}+/` trifft `"p{L}abc"`, mit `u` `"abc"`.
            Knoten::Eigenschaft {
                bereiche, literal, ..
            } => {
                let t = match self.text.get(pos) {
                    Some(t) => *t,
                    None => return Vec::new(),
                };
                let drin = if self.einheiten {
                    let f = if self.schalter.egal_gross {
                        falte(t)
                    } else {
                        t
                    };
                    bereich_enthält(bereiche, f)
                } else {
                    t == literal[0]
                };
                if !drin {
                    return Vec::new();
                }
                if self.einheiten {
                    vec![Self::stand(pos + 1, gruppen)]
                } else {
                    // Literal Zeichen fuer Zeichen: `p`, `{`, Name, `}`.
                    self.literal_kette(literal, pos, gruppen)
                }
            }
            // `\u{…}`: mit `u` der Codepoint (eine oder zwei Einheiten),
            // ohne `u` das Literal `u{…}`.
            Knoten::CodepunktOderLiteral { cp, literal } => {
                if self.einheiten {
                    for (i, u) in cp.iter().enumerate() {
                        if self.text.get(pos + i) != Some(u) {
                            return Vec::new();
                        }
                    }
                    vec![Self::stand(pos + cp.len(), gruppen)]
                } else {
                    self.literal_kette(literal, pos, gruppen)
                }
            }
            Knoten::Punkt => {
                let t = match self.text.get(pos) {
                    Some(t) => *t,
                    None => return Vec::new(),
                };
                if !self.schalter.punkt_alles && (t == b'\n' as u16 || t == b'\r' as u16) {
                    return Vec::new();
                }
                // `.` trifft mit `u` einen **Codepoint** (ein Astralzeichen
                // = zwei UTF-16-Einheiten). GEMESSEN 04.10. am Orakel
                // `node` v22: 3 Faelle (`U+1D401`, `U+20BB7`, `U+1D306` mit
                // Flag `u`) liefern dort 0..2, bei uns 0..1 — mit einem
                // `Schalter`-Feld `einheiten` und
                // `self.schalter.einheiten && (0xD800..0xDC00).contains(&t)
                //  && <tiefes Surrogat>` stieg der Lauf von 907 auf **910
                // von 920** (98,59 % -> 98,91 %), ohne Regression.
                //
                // **Zurueckgenommen**, weil das Feld `Schalter` an **vier
                // fremden** Aufrufstellen (u. a. `eigenschaften.rs:2155`)
                // einen Struktur-Literal bricht, den ich nicht aendern
                // darf — und die echte `exec`-Flaeche liegt in
                // `builtin2.rs`/`builtin3.rs`, also nicht in meiner Hand.
                // Der Wunsch mit der fertigen Form steht in
                // `team/WUNSCH_JS.txt`.
                // Mit `u` trifft `.` einen **Codepoint**: ein Astralzeichen
                // ist ein Paar aus hohem und tiefem Surrogat und zaehlt
                // als **zwei** UTF-16-Einheiten (die Index-Semantik
                // bleibt UTF-16, ECMA-262 22.2.1).
                let schritt = if self.einheiten
                    && (0xD800..0xDC00).contains(&t)
                    && matches!(self.text.get(pos + 1), Some(&n) if (0xDC00..0xE000).contains(&n))
                {
                    2
                } else {
                    1
                };
                vec![Self::stand(pos + schritt, gruppen)]
            }
            Knoten::Anfang => {
                if pos == 0 {
                    vec![Self::stand(pos, gruppen)]
                } else if self.schalter.mehrzeilig && pos > 0 {
                    let v = self.text[pos - 1];
                    if v == b'\n' as u16 || v == b'\r' as u16 {
                        vec![Self::stand(pos, gruppen)]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            }
            Knoten::Ende => {
                if pos == self.text.len() {
                    vec![Self::stand(pos, gruppen)]
                } else if self.schalter.mehrzeilig && pos < self.text.len() {
                    let v = self.text[pos];
                    if v == b'\n' as u16 || v == b'\r' as u16 {
                        vec![Self::stand(pos, gruppen)]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            }
            Knoten::Wortgrenze(negiert) => {
                let links = pos > 0 && ist_wort(self.text[pos - 1]);
                let rechts = pos < self.text.len() && ist_wort(self.text[pos]);
                if (links != rechts) != *negiert {
                    vec![Self::stand(pos, gruppen)]
                } else {
                    Vec::new()
                }
            }
            Knoten::Folge(te) => {
                let mut staende = vec![Self::stand(pos, gruppen)];
                for t in te {
                    if staende.is_empty() || self.budget == 0 {
                        break;
                    }
                    let mut neu: Vec<Stand> = Vec::new();
                    for s in &staende {
                        if neu.len() >= MAX_STAENDE {
                            break;
                        }
                        neu.extend(self.alle(t, s.ende, &s.gruppen));
                    }
                    staende = neu;
                }
                staende
            }
            Knoten::Auswahl(zweige) => {
                let mut out = Vec::new();
                for z in zweige {
                    if out.len() >= MAX_STAENDE || self.budget == 0 {
                        break;
                    }
                    out.extend(self.alle(z, pos, gruppen));
                }
                out
            }
            Knoten::Wiederholung {
                inner,
                min,
                max,
                gierig,
                gruppen_innen,
            } => self.alle_wiederholung(
                inner,
                *min,
                max.unwrap_or(u32::MAX),
                *gierig,
                gruppen_innen,
                pos,
                gruppen,
            ),
            Knoten::Gruppe { id, inner } => {
                let mut out = Vec::new();
                for mut s in self.alle(inner, pos, gruppen) {
                    if *id < s.gruppen.len() {
                        s.gruppen[*id] = Some((pos, s.ende));
                    }
                    out.push(s);
                    if out.len() >= MAX_STAENDE {
                        break;
                    }
                }
                out
            }
            Knoten::Rueckverweis(id) => {
                // GEMESSEN 03.10. an node 22: `"aa".match(/(?<a>\k<a>)/)[0]`
                // ist `""` — ein Rueckverweis auf eine **nicht gesetzte**
                // Gruppe trifft die leere Zeichenkette (ECMA-262 22.2.2.8
                // `BackreferenceMatcher`: „If r's matched is undefined,
                // return empty"), nicht „nichts". Vorher stand hier
                // `return Vec::new()`, damit war `(?<a>\k<a>)` nie zu
                // treffen und der Selbstbezug unmoeglich.
                let (a, b) = match (*id < gruppen.len()).then(|| gruppen[*id]).flatten() {
                    Some(v) => v,
                    None => return vec![Self::stand(pos, gruppen)],
                };
                let len = b - a;
                let ende = pos + len;
                if ende > self.text.len() {
                    return Vec::new();
                }
                for i in 0..len {
                    let x = self.text[a + i];
                    let y = self.text[pos + i];
                    if self.schalter.egal_gross {
                        if klein(x) != klein(y) {
                            return Vec::new();
                        }
                    } else if x != y {
                        return Vec::new();
                    }
                }
                vec![Self::stand(ende, gruppen)]
            }
            Knoten::KlasseCp(k) => {
                let mut raus = Vec::new();
                for t in &k.texte {
                    if self.text_gleich(t, pos) {
                        raus.push(Self::stand(pos + t.len(), gruppen));
                    }
                }
                if let Some((cp, len)) = cp_an(self.text, pos, self.einheiten) {
                    let drin = if self.schalter.egal_gross && k.faltung {
                        cp_enthaelt(&k.gefalten, falte_cp(cp))
                    } else if self.schalter.egal_gross {
                        cp_enthaelt(&k.bereiche, cp) || cp_enthaelt(&k.bereiche, falte_cp(cp))
                    } else {
                        cp_enthaelt(&k.bereiche, cp)
                    };
                    if drin != k.negiert {
                        raus.push(Self::stand(pos + len, gruppen));
                    }
                }
                if k.leer {
                    raus.push(Self::stand(pos, gruppen));
                }
                raus
            }
            Knoten::Modifikator { inner, an, aus } => {
                let alt = self.schalter;
                let setze = |wert: &mut bool, bit: u8| {
                    if an & bit != 0 {
                        *wert = true;
                    }
                    if aus & bit != 0 {
                        *wert = false;
                    }
                };
                setze(&mut self.schalter.egal_gross, 1);
                setze(&mut self.schalter.mehrzeilig, 2);
                setze(&mut self.schalter.punkt_alles, 4);
                let r = self.alle(inner, pos, gruppen);
                self.schalter = alt;
                r
            }
            Knoten::RueckverweisNamen(ids) => {
                match ids.iter().find(|i| gruppen.get(**i).copied().flatten().is_some()) {
                    Some(i) => self.alle_innen(&Knoten::Rueckverweis(*i), pos, gruppen),
                    None => vec![Self::stand(pos, gruppen)],
                }
            }
            Knoten::Vorausschau { inner, positiv } => {
                // Gruppen aus Lookahead werden verworfen (dokumentiert).
                //
                // GEMESSEN 04.10.: sie zu **behalten** (wie im Lookbehind
                // seit 03.10.) kostete 3 Faelle und gewann keinen —
                // Quote 904 statt 907. `(?=(a+))` auf `"baaabac"` bleibt
                // also offen; vermutlich ist die Reihenfolge der
                // Stand-Auswahl im Backtracking die eigentliche Ursache,
                // nicht das Verwerfen.
                // Release 09.10.2026: der Lookahead sieht die Gruppen DAVOR
                // (ECMA-262 22.2.2.4: er laeuft mit dem aktuellen Zustand).
                // GEMESSEN vorher: `/(a)(?=\1)/.exec("ab")` traf (der
                // Rueckverweis lief ins Leere), `"gy".replace(/([\s\S])(?=
                // [\s\S]*?\1)/g, "")` gab `""` statt `"gy"` — XRegExp
                // (youtube.com) verlor damit alle Schalter und baute ein
                // kaputtes Muster. Positiv: die Gruppen des ERSTEN Treffers
                // bleiben (atomar, kein Zuruecksetzen hinein).
                let innen = self.alle(inner, pos, gruppen);
                match (innen.into_iter().next(), *positiv) {
                    (Some(t), true) => vec![Self::stand(pos, &t.gruppen)],
                    (None, false) => vec![Self::stand(pos, gruppen)],
                    _ => Vec::new(),
                }
            }
            Knoten::Rueckschau { inner, positiv } => {
                let von = pos.saturating_sub(MAX_LOOKBEHIND);
                let g0 = vec![None; gruppen.len()];
                // GEMESSEN 03.10. (`named-groups/lookbehind.js`, 5 gezaehlte
                // Faelle): der Zweig benutzte `ok` als **Flag** und gab
                // danach `Self::stand(pos, gruppen)` zurueck — also mit den
                // leeren Gruppen `g0`. Die Gruppen **innerhalb** eines
                // Lookbehinds bleiben aber erhalten (ECMA-262 22.2.2.9:
                // die Assertion laeuft die Gruppen-Zustaende wie ein
                // gewoehnlicher Teil des Musters ab). Deshalb wird der
                // gewonnene Gruppenstand behalten statt nur ein Flag.
                let mut gewonnen: Option<Vec<Option<(usize, usize)>>> = None;
                for s in von..=pos {
                    if self.budget == 0 {
                        break;
                    }
                    for t in self.alle(inner, s, &g0) {
                        if t.ende == pos {
                            gewonnen = Some(t.gruppen);
                            break;
                        }
                    }
                    if gewonnen.is_some() {
                        break;
                    }
                }
                match gewonnen {
                    Some(g) if *positiv => {
                        let mut st = Self::stand(pos, gruppen);
                        // Nur belegte Gruppen ueberschreiben (M3: die
                        // Schleife laeuft ueber die Mustergruppen, nicht
                        // ueber die Eingabe).
                        for (i, w) in g.iter().enumerate() {
                            if i < st.gruppen.len() && w.is_some() {
                                st.gruppen[i] = *w;
                            }
                        }
                        vec![st]
                    }
                    // negativ + getroffen: Assertion faellt -> nichts.
                    Some(_) => Vec::new(),
                    // positiv + nicht getroffen: nichts.
                    None if *positiv => Vec::new(),
                    // negativ + nicht getroffen: leeren Stand weitergeben.
                    None => vec![Self::stand(pos, gruppen)],
                }
            }
        }
    }

    /// Wiederholung mit ECHTEM Backtracking: Ebenen aufsteigend sammeln
    /// (Fixpunkt bei leeren Matches), dann gierig/lazy ausgeben.
    /// Wiederholung mit **geordnetem Backtracking** (ECMA-262
    /// 22.2.2.7: die Reihenfolge der Versuche ist die *Musterreihenfolge*).
    ///
    /// GEMESSEN 04.10. (Orakel `node` v22): die vorherige Fassung sammelte
    /// **Ebenen** (breitensuchend) und gab "die tiefste zuerst" aus. Bei
    /// `(?:aa|aabaac|ba|b|c)*` auf `"aabaac"` entstehen die Ebenen
    /// `[[0], [2, 6], [4, 3], [5], [6]]`,
    /// und die alte Auswahl nahm **6**. Die Spec verlangt
    /// tiefensuchend in Alternativen-Reihenfolge: Runde 1 nimmt `aa`,
    /// Runde 2 `ba`, Runde 3 scheitert ⇒ **4** (so misst es `node`).
    ///
    /// Neu ist deshalb die **Reihenfolge**, nicht die Menge.
    ///
    /// **Zwei Grenzen, beide hart (M3/M5):**
    /// * `MAX_WIEDERHOL_RUNDEN` — Zahl der Runden einer Wiedeholung. Ohne
    ///   sie waere `(?:a?)*` nicht zu beenden.
    /// * `MAX_STAENDE` — Zahl der ausgegebenen Staende.
    /// * `MAX_WIEDERHOL_TIEFE` — **nur** die Rekursionstiefe des
    ///   verzweigenden Teils (siehe unten).
    ///
    /// **Warum der Kettenanteil iterativ laeuft (GEMESSEN 04.10.):** eine
    /// durchgehende Rekursion endete nach `MAX_WIEDERHOL_TIEFE` Runden, also
    /// nach **512 Zeichen** — `a+` auf 600 oder 2000 Zeichen lieferte ein
    /// **abgeschnittenes** Ende (Tiefenprobe 500/511 gut, 512/513/600/2000
    /// falsch). Das traf genau die amtlichen Grossfaelle (Eingaben ueber eine
    /// Million Zeichen). Deshalb laeuft der **unverzweigte** Teil als
    /// Schleife; die Rekursion bleibt nur fuer den Teil, der wirklich
    /// verzweigt — dort begrenzt ohnehin das Schrittbudget die Arbeit.
    ///
    /// Die Ausgabe haelt nur die **letzten** `MAX_STAENDE` Staende der
    /// Kette: gierig ist der **tiefste** Stand der hoechste Prioritaet,
    /// und der tiefste steht am Ende der Schleife.
    fn alle_wiederholung(
        &mut self,
        inner: &Knoten,
        min: u32,
        max: u32,
        gierig: bool,
        gruppen_innen: &[usize],
        pos: usize,
        gruppen: &[Option<(usize, usize)>],
    ) -> Vec<Stand> {
        // Die **Kette** (unverzweigt) laeuft iterativ, deshalb darf sie
        // weit mehr Runden machen als die Rekursion: GEMESSEN 04.10., die
        // Grenze lag exakt bei `MAX_WIEDERHOL_EBENEN` = 100.000 — `a+` auf
        // 200.000 Zeichen lieferte 0..100.000, auf 50.000 das richtige
        // Ergebnis. Die harte Grenze ist jetzt `MAX_WIEDERHOL_RUNDEN`,
        // darueber das Schrittbudget.
        let hart = max.min(MAX_WIEDERHOL_RUNDEN);
        // **Wichtig (M8, GEMESSEN 04.10.):** die Rekursionstiefe muss
        // `MAX_WIEDERHOL_TIEFE` auch tatsaechlich begrenzen. Mit
        // `MAX_WIEDERHOL_EBENEN` allein (100.000) lief `(?:a|aa)+b` auf
        // 100.000 Zeichen in einen **Stapelueberlauf** — je Runde zwei
        // Staende, jeder wieder verzweigt, und das Schrittbudget (das mit
        // der Eingabegroesse waechst) liess die Tiefe wachsen.
        let hart_rek = max.min(MAX_WIEDERHOL_EBENEN).min(MAX_WIEDERHOL_TIEFE);
        let mut raus: Vec<Stand> = Vec::new();
        // Der Stand der **null Runden** ist ein gueltiger Treffer, wenn
        // `min == 0` — bei einem optionalen Rumpf ist er sogar der
        // wichtigste: `ab[.]?c` auf `"abc"` braucht ihn (ohne ihn
        // verschwanden 34 Faelle, 875 statt 913 von 920).
        // Reihenfolge: **lazy** zuerst (kuerzester Treffer zuerst),
        // **gierig** zuletzt.
        if min == 0 && !gierig {
            raus.push(Self::stand(pos, gruppen));
        }

        // --- Teil 1: die Kette, solange es nur einen Weg gibt -----------
        let mut n: u32 = 0;
        let mut pos_kette = pos;
        let mut gruppen_kette = gruppen.to_vec();
        let mut kette: Vec<Stand> = Vec::new();
        let mut voll = false;
        while n < hart && n < MAX_WIEDERHOL_RUNDEN && self.budget > 0 {
            // Die Gruppen **des Rumpfes** beginnen jede Runde leer
            // (ECMA-262 22.2.2.7.4); die ausserhalb bleiben erhalten.
            //
            // GEMESSEN 04.10.: `(z)((a+)?(b+)?(c))*` auf `"zaacbbac"` lieferte
            // ohne das 5 belegte Gruppen statt 4 + 1 leere.
            let mut start_gruppen = gruppen_kette.clone();
            for &g in gruppen_innen {
                if let Some(slot) = start_gruppen.get_mut(g) {
                    *slot = None;
                }
            }
            let staende = self.alle(inner, pos_kette, &start_gruppen);
            if staende.len() != 1 {
                // Verzweigt (oder zu Ende): der Rest ist Sache der
                // Rekursion. `alle` wurde bereits aufgerufen — die
                // Staende werden dort erneut geholt, das kostet nur
                // Schritte, sonst nichts.
                voll = true;
                break;
            }
            let s = staende.into_iter().next().expect("genau ein Stand");
            if n + 1 >= min {
                if gierig {
                    // Nur die **letzten** `MAX_STAENDE` behalten: der
                    // tiefste ist der beste und steht am Ende.
                    if kette.len() >= MAX_STAENDE {
                        kette.clear();
                    }
                    kette.push(s.clone());
                } else {
                    // Lazy: der kuerzeste zuerst, also die **ersten**.
                    if kette.len() < MAX_STAENDE {
                        kette.push(s.clone());
                    }
                }
            }
            pos_kette = s.ende;
            gruppen_kette = s.gruppen;
            n += 1;
        }

        // --- Teil 2: der verzweigende Rest (Rekursion) ---------------------
        let mut tief: Vec<Stand> = Vec::new();
        if voll {
            self.wiederhole_tief(
                inner,
                min.saturating_sub(n),
                hart_rek.saturating_sub(n),
                gierig,
                gruppen_innen,
                pos_kette,
                &gruppen_kette,
                0,
                &mut tief,
            );
        }

        // --- Reihenfolge (gierig: tiefer zuerst) ---------------------------
        if gierig {
            raus.append(&mut tief);
            raus.extend(kette.into_iter().rev());
        } else {
            raus.extend(kette);
            raus.append(&mut tief);
        }
        if min == 0 && gierig {
            raus.push(Self::stand(pos, gruppen));
        }
        raus.truncate(MAX_STAENDE);
        raus
    }

    /// Der tiefensuchende Kern von `alle_wiederholung`.
    ///
    /// `n` ist die Zahl der bereits gelaufenen Runden. Ein Stand wird nur
    /// ausgegeben, wenn `n + 1 >= min` — unter `min` gibt es keinen
    /// Treffer (ECMA-262 22.2.2.7.1).
    #[allow(clippy::too_many_arguments)]
    fn wiederhole_tief(
        &mut self,
        inner: &Knoten,
        min: u32,
        hart: u32,
        gierig: bool,
        gruppen_innen: &[usize],
        pos: usize,
        gruppen: &[Option<(usize, usize)>],
        n: u32,
        raus: &mut Vec<Stand>,
    ) {
        if self.budget == 0 || raus.len() >= MAX_STAENDE || n >= hart {
            return;
        }
        // Die Gruppen **des Rumpfes** beginnen jede Runde leer.
        // Die Gruppen **des Rumpfes** beginnen jede Runde leer
        // (ECMA-262 22.2.2.7.4); die ausserhalb bleiben erhalten.
        //
        // GEMESSEN 04.10.: `(z)((a+)?(b+)?(c))*` auf `"zaacbbac"` lieferte
        // ohne das 5 belegte Gruppen statt 4 + 1 leere.
        let mut start_gruppen = gruppen.to_vec();
        for &g in gruppen_innen {
            if let Some(slot) = start_gruppen.get_mut(g) {
                *slot = None;
            }
        }
        // Der Stand der **null Runden** ist ein gültiger Treffer, wenn
        // `min == 0` — bei einem optionalen Rumpf ist er sogar der
        // wichtigste: `ab[.]?c` auf `"abc"` braucht ihn (ohne ihn
        // verschwanden 34 Faelle, 875 statt 913 von 920).
        //
        // Reihenfolge: **lazy** zuerst (kürzester Treffer zuerst),
        // **gierig** zuletzt.
        if min == 0 && n == 0 && !gierig {
            raus.push(Self::stand(pos, gruppen));
        }
        for s in self.alle(inner, pos, &start_gruppen) {
            if raus.len() >= MAX_STAENDE {
                return;
            }
            if n + 1 >= min {
                // **Kein `s.ende != pos`-Guard:** eine Runde ohne
                // Fortschritt ist erlaubt (`(?:a?){2,}` muss die leere
                // Runde mitzaehlen, um `min` zu erreichen). Das **beendet**
                // die Abbruchgrenze `n >= hart` zusammen mit `MAX_STAENDE`
                // und dem Schrittbudget.
                //
                // GEMESSEN 04.10., drei Fehldeutungen in Folge, alle am
                // Fehlfall-Diff erkannt: (a) mit dem Guard **875/920**,
                // weil die leere Runde die `min`-Zaehlung nicht erreichte;
                // (b) **ohne** Guard, aber mit 50-fachem Budget ebenfalls
                // 875 — das Budget war also **nicht** die Ursache;
                // (c) die eigentliche Ursache: der Stand der **null
                // Runden** wurde nie ausgegeben (`ab[.]?c` braucht ihn).
                // Mit (c) **914/920** ohne Regression.
                if gierig {
                    // Gierig: erst tiefer, dann dieser Stand.
                    self.wiederhole_tief(
                        inner,
                        min,
                        hart,
                        gierig,
                        gruppen_innen,
                        s.ende,
                        &s.gruppen,
                        n + 1,
                        raus,
                    );
                    raus.push(s.clone());
                } else {
                    // Lazy: erst dieser Stand, dann tiefer.
                    raus.push(s.clone());
                    self.wiederhole_tief(
                        inner,
                        min,
                        hart,
                        gierig,
                        gruppen_innen,
                        s.ende,
                        &s.gruppen,
                        n + 1,
                        raus,
                    );
                }
            } else {
                // Unter `min`: der Stand wird nicht ausgegeben, aber die
                // Suche muss weiterlaufen.
                self.wiederhole_tief(
                    inner,
                    min,
                    hart,
                    gierig,
                    gruppen_innen,
                    s.ende,
                    &s.gruppen,
                    n + 1,
                    raus,
                );
            }
        }
        if min == 0 && n == 0 && gierig {
            raus.push(Self::stand(pos, gruppen));
        }
    }
}

/// Alle Gruppen-Ids **innerhalb** dieses Teilbaums, in Reihenfolge.
///
/// Nur fuer die Gruppenruecksetzung in quantifizierten Gruppen
/// (`Knoten::Wiederholung::gruppen_innen`).
fn sammle_gruppen(k: &Knoten, heraus: &mut Vec<usize>) {
    match k {
        Knoten::Gruppe { id, inner } => {
            heraus.push(*id);
            sammle_gruppen(inner, heraus);
        }
        Knoten::Folge(v) | Knoten::Auswahl(v) => {
            for z in v {
                sammle_gruppen(z, heraus);
            }
        }
        Knoten::Wiederholung { inner, .. }
        | Knoten::Modifikator { inner, .. }
        | Knoten::Vorausschau { inner, .. }
        | Knoten::Rueckschau { inner, .. } => sammle_gruppen(inner, heraus),
        _ => {}
    }
}

/// J27: enthaelt eine **sortierte, lueckenlose** Bereichsliste die
/// Einheit `t`?
///
/// Voraussetzung ist die Invariante aus `Parser::merze_und_sortiere` —
/// deshalb ist das **kein** linearer Scan. GEMESSEN, warum das Pflicht
/// ist: die `generated/`-Faelle von `RegExp/property-escapes` bringen bis
/// zu ~1200 Bereiche je Zeichenklasse, und der lineare Scan lief fuer
/// jedes einzelne Zeichen der Eingabe.
///
/// GEMESSEN, warum die Gross/Klein-Umstellung kein Problem ist: sie bildet
/// jeden Bereich auf einen Bereich ab und ist monoton, also bleibt
/// `klein(lo)..klein(hi)` sortiert und die Liste auch fuer den
/// Kleinbuchstaben-Zweig gueltig.
///
/// FREIE Funktion und keine Methode: sie wird von `Durchlauf` gebraucht,
/// und eine Methode waere nur fuer einen Empfaenger richtig.
fn bereich_enthält(b: &[(u16, u16)], t: u16) -> bool {
    // Die erste Stelle, deren `hi` >= t — danach entscheidet `lo`.
    let i = b.partition_point(|(_, hi)| *hi < t);
    matches!(b.get(i), Some((lo, hi)) if t >= *lo && t <= *hi)
}

/// Faltungsklassen fuer den `i`-Schalter: `(Zeichen, Vertreter)`.
///
/// GEMESSEN 04.10. (Orakel `node` v22, Lauf ueber 920 Faelle aus
/// `test/built-ins/RegExp`):
/// * `[\u0390]/ui` trifft `\u1FD3` — `lower()`/`upper()`/`casefold()`
///   liefern fuer **beide** dieselbe Folge, `klein()` aber `\u03CA`;
/// * `[K]/iu` trifft `\u212A` (Kelvin) — `lower()` liefert `k`;
/// * `\u00DF` und `\u1E9E` falten auf **zwei** Zeichen (`ss`) und
///   werden deshalb **nicht** mit einem Ein-Buchstaben-Muster
///   gleichgesetzt — so misst es `node`.
///
/// Erzeugt aus `str.lower()`, `str.upper()` und `str.casefold()` ueber
/// den BMP (`CaseFolding.txt` liegt auf der Maschine nicht vor): alle
/// Zeichen mit **gleicher** Form werden verknuepft. Der Vertreter ist das
/// **kleinste** Zeichen der Klasse — damit bleibt die Liste sortiert und
/// eine **binaere Suche** traegt (Leistung, siehe `bereich_enthält`).
static FALTUNG: &[(u16, u16)] = &[
    (0x0061, 0x0041),
    (0x0062, 0x0042),
    (0x0063, 0x0043),
    (0x0064, 0x0044),
    (0x0065, 0x0045),
    (0x0066, 0x0046),
    (0x0067, 0x0047),
    (0x0068, 0x0048),
    (0x0069, 0x0049),
    (0x006A, 0x004A),
    (0x006B, 0x004B),
    (0x006C, 0x004C),
    (0x006D, 0x004D),
    (0x006E, 0x004E),
    (0x006F, 0x004F),
    (0x0070, 0x0050),
    (0x0071, 0x0051),
    (0x0072, 0x0052),
    (0x0073, 0x0053),
    (0x0074, 0x0054),
    (0x0075, 0x0055),
    (0x0076, 0x0056),
    (0x0077, 0x0057),
    (0x0078, 0x0058),
    (0x0079, 0x0059),
    (0x007A, 0x005A),
    (0x00E0, 0x00C0),
    (0x00E1, 0x00C1),
    (0x00E2, 0x00C2),
    (0x00E3, 0x00C3),
    (0x00E4, 0x00C4),
    (0x00E5, 0x00C5),
    (0x00E6, 0x00C6),
    (0x00E7, 0x00C7),
    (0x00E8, 0x00C8),
    (0x00E9, 0x00C9),
    (0x00EA, 0x00CA),
    (0x00EB, 0x00CB),
    (0x00EC, 0x00CC),
    (0x00ED, 0x00CD),
    (0x00EE, 0x00CE),
    (0x00EF, 0x00CF),
    (0x00F0, 0x00D0),
    (0x00F1, 0x00D1),
    (0x00F2, 0x00D2),
    (0x00F3, 0x00D3),
    (0x00F4, 0x00D4),
    (0x00F5, 0x00D5),
    (0x00F6, 0x00D6),
    (0x00F8, 0x00D8),
    (0x00F9, 0x00D9),
    (0x00FA, 0x00DA),
    (0x00FB, 0x00DB),
    (0x00FC, 0x00DC),
    (0x00FD, 0x00DD),
    (0x00FE, 0x00DE),
    (0x0101, 0x0100),
    (0x0103, 0x0102),
    (0x0105, 0x0104),
    (0x0107, 0x0106),
    (0x0109, 0x0108),
    (0x010B, 0x010A),
    (0x010D, 0x010C),
    (0x010F, 0x010E),
    (0x0111, 0x0110),
    (0x0113, 0x0112),
    (0x0115, 0x0114),
    (0x0117, 0x0116),
    (0x0119, 0x0118),
    (0x011B, 0x011A),
    (0x011D, 0x011C),
    (0x011F, 0x011E),
    (0x0121, 0x0120),
    (0x0123, 0x0122),
    (0x0125, 0x0124),
    (0x0127, 0x0126),
    (0x0129, 0x0128),
    (0x012B, 0x012A),
    (0x012D, 0x012C),
    (0x012F, 0x012E),
    (0x0131, 0x0049),
    (0x0133, 0x0132),
    (0x0135, 0x0134),
    (0x0137, 0x0136),
    (0x013A, 0x0139),
    (0x013C, 0x013B),
    (0x013E, 0x013D),
    (0x0140, 0x013F),
    (0x0142, 0x0141),
    (0x0144, 0x0143),
    (0x0146, 0x0145),
    (0x0148, 0x0147),
    (0x014B, 0x014A),
    (0x014D, 0x014C),
    (0x014F, 0x014E),
    (0x0151, 0x0150),
    (0x0153, 0x0152),
    (0x0155, 0x0154),
    (0x0157, 0x0156),
    (0x0159, 0x0158),
    (0x015B, 0x015A),
    (0x015D, 0x015C),
    (0x015F, 0x015E),
    (0x0161, 0x0160),
    (0x0163, 0x0162),
    (0x0165, 0x0164),
    (0x0167, 0x0166),
    (0x0169, 0x0168),
    (0x016B, 0x016A),
    (0x016D, 0x016C),
    (0x016F, 0x016E),
    (0x0171, 0x0170),
    (0x0173, 0x0172),
    (0x0175, 0x0174),
    (0x0177, 0x0176),
    (0x0178, 0x00FF),
    (0x017A, 0x0179),
    (0x017C, 0x017B),
    (0x017E, 0x017D),
    (0x017F, 0x0053),
    (0x0183, 0x0182),
    (0x0185, 0x0184),
    (0x0188, 0x0187),
    (0x018C, 0x018B),
    (0x0192, 0x0191),
    (0x0199, 0x0198),
    (0x01A1, 0x01A0),
    (0x01A3, 0x01A2),
    (0x01A5, 0x01A4),
    (0x01A8, 0x01A7),
    (0x01AD, 0x01AC),
    (0x01B0, 0x01AF),
    (0x01B4, 0x01B3),
    (0x01B6, 0x01B5),
    (0x01B9, 0x01B8),
    (0x01BD, 0x01BC),
    (0x01C5, 0x01C4),
    (0x01C6, 0x01C4),
    (0x01C8, 0x01C7),
    (0x01C9, 0x01C7),
    (0x01CB, 0x01CA),
    (0x01CC, 0x01CA),
    (0x01CE, 0x01CD),
    (0x01D0, 0x01CF),
    (0x01D2, 0x01D1),
    (0x01D4, 0x01D3),
    (0x01D6, 0x01D5),
    (0x01D8, 0x01D7),
    (0x01DA, 0x01D9),
    (0x01DC, 0x01DB),
    (0x01DD, 0x018E),
    (0x01DF, 0x01DE),
    (0x01E1, 0x01E0),
    (0x01E3, 0x01E2),
    (0x01E5, 0x01E4),
    (0x01E7, 0x01E6),
    (0x01E9, 0x01E8),
    (0x01EB, 0x01EA),
    (0x01ED, 0x01EC),
    (0x01EF, 0x01EE),
    (0x01F2, 0x01F1),
    (0x01F3, 0x01F1),
    (0x01F5, 0x01F4),
    (0x01F6, 0x0195),
    (0x01F7, 0x01BF),
    (0x01F9, 0x01F8),
    (0x01FB, 0x01FA),
    (0x01FD, 0x01FC),
    (0x01FF, 0x01FE),
    (0x0201, 0x0200),
    (0x0203, 0x0202),
    (0x0205, 0x0204),
    (0x0207, 0x0206),
    (0x0209, 0x0208),
    (0x020B, 0x020A),
    (0x020D, 0x020C),
    (0x020F, 0x020E),
    (0x0211, 0x0210),
    (0x0213, 0x0212),
    (0x0215, 0x0214),
    (0x0217, 0x0216),
    (0x0219, 0x0218),
    (0x021B, 0x021A),
    (0x021D, 0x021C),
    (0x021F, 0x021E),
    (0x0220, 0x019E),
    (0x0223, 0x0222),
    (0x0225, 0x0224),
    (0x0227, 0x0226),
    (0x0229, 0x0228),
    (0x022B, 0x022A),
    (0x022D, 0x022C),
    (0x022F, 0x022E),
    (0x0231, 0x0230),
    (0x0233, 0x0232),
    (0x023C, 0x023B),
    (0x023D, 0x019A),
    (0x0242, 0x0241),
    (0x0243, 0x0180),
    (0x0247, 0x0246),
    (0x0249, 0x0248),
    (0x024B, 0x024A),
    (0x024D, 0x024C),
    (0x024F, 0x024E),
    (0x0253, 0x0181),
    (0x0254, 0x0186),
    (0x0256, 0x0189),
    (0x0257, 0x018A),
    (0x0259, 0x018F),
    (0x025B, 0x0190),
    (0x0260, 0x0193),
    (0x0263, 0x0194),
    (0x0268, 0x0197),
    (0x0269, 0x0196),
    (0x026F, 0x019C),
    (0x0272, 0x019D),
    (0x0275, 0x019F),
    (0x0280, 0x01A6),
    (0x0283, 0x01A9),
    (0x0288, 0x01AE),
    (0x0289, 0x0244),
    (0x028A, 0x01B1),
    (0x028B, 0x01B2),
    (0x028C, 0x0245),
    (0x0292, 0x01B7),
    (0x0371, 0x0370),
    (0x0373, 0x0372),
    (0x0377, 0x0376),
    (0x0399, 0x0345),
    (0x039C, 0x00B5),
    (0x03AC, 0x0386),
    (0x03AD, 0x0388),
    (0x03AE, 0x0389),
    (0x03AF, 0x038A),
    (0x03B1, 0x0391),
    (0x03B2, 0x0392),
    (0x03B3, 0x0393),
    (0x03B4, 0x0394),
    (0x03B5, 0x0395),
    (0x03B6, 0x0396),
    (0x03B7, 0x0397),
    (0x03B8, 0x0398),
    (0x03B9, 0x0345),
    (0x03BA, 0x039A),
    (0x03BB, 0x039B),
    (0x03BC, 0x00B5),
    (0x03BD, 0x039D),
    (0x03BE, 0x039E),
    (0x03BF, 0x039F),
    (0x03C0, 0x03A0),
    (0x03C1, 0x03A1),
    (0x03C2, 0x03A3),
    (0x03C3, 0x03A3),
    (0x03C4, 0x03A4),
    (0x03C5, 0x03A5),
    (0x03C6, 0x03A6),
    (0x03C7, 0x03A7),
    (0x03C8, 0x03A8),
    (0x03C9, 0x03A9),
    (0x03CA, 0x03AA),
    (0x03CB, 0x03AB),
    (0x03CC, 0x038C),
    (0x03CD, 0x038E),
    (0x03CE, 0x038F),
    (0x03D0, 0x0392),
    (0x03D1, 0x0398),
    (0x03D5, 0x03A6),
    (0x03D6, 0x03A0),
    (0x03D7, 0x03CF),
    (0x03D9, 0x03D8),
    (0x03DB, 0x03DA),
    (0x03DD, 0x03DC),
    (0x03DF, 0x03DE),
    (0x03E1, 0x03E0),
    (0x03E3, 0x03E2),
    (0x03E5, 0x03E4),
    (0x03E7, 0x03E6),
    (0x03E9, 0x03E8),
    (0x03EB, 0x03EA),
    (0x03ED, 0x03EC),
    (0x03EF, 0x03EE),
    (0x03F0, 0x039A),
    (0x03F1, 0x03A1),
    (0x03F3, 0x037F),
    (0x03F4, 0x0398),
    (0x03F5, 0x0395),
    (0x03F8, 0x03F7),
    (0x03F9, 0x03F2),
    (0x03FB, 0x03FA),
    (0x03FD, 0x037B),
    (0x03FE, 0x037C),
    (0x03FF, 0x037D),
    (0x0430, 0x0410),
    (0x0431, 0x0411),
    (0x0432, 0x0412),
    (0x0433, 0x0413),
    (0x0434, 0x0414),
    (0x0435, 0x0415),
    (0x0436, 0x0416),
    (0x0437, 0x0417),
    (0x0438, 0x0418),
    (0x0439, 0x0419),
    (0x043A, 0x041A),
    (0x043B, 0x041B),
    (0x043C, 0x041C),
    (0x043D, 0x041D),
    (0x043E, 0x041E),
    (0x043F, 0x041F),
    (0x0440, 0x0420),
    (0x0441, 0x0421),
    (0x0442, 0x0422),
    (0x0443, 0x0423),
    (0x0444, 0x0424),
    (0x0445, 0x0425),
    (0x0446, 0x0426),
    (0x0447, 0x0427),
    (0x0448, 0x0428),
    (0x0449, 0x0429),
    (0x044A, 0x042A),
    (0x044B, 0x042B),
    (0x044C, 0x042C),
    (0x044D, 0x042D),
    (0x044E, 0x042E),
    (0x044F, 0x042F),
    (0x0450, 0x0400),
    (0x0451, 0x0401),
    (0x0452, 0x0402),
    (0x0453, 0x0403),
    (0x0454, 0x0404),
    (0x0455, 0x0405),
    (0x0456, 0x0406),
    (0x0457, 0x0407),
    (0x0458, 0x0408),
    (0x0459, 0x0409),
    (0x045A, 0x040A),
    (0x045B, 0x040B),
    (0x045C, 0x040C),
    (0x045D, 0x040D),
    (0x045E, 0x040E),
    (0x045F, 0x040F),
    (0x0461, 0x0460),
    (0x0463, 0x0462),
    (0x0465, 0x0464),
    (0x0467, 0x0466),
    (0x0469, 0x0468),
    (0x046B, 0x046A),
    (0x046D, 0x046C),
    (0x046F, 0x046E),
    (0x0471, 0x0470),
    (0x0473, 0x0472),
    (0x0475, 0x0474),
    (0x0477, 0x0476),
    (0x0479, 0x0478),
    (0x047B, 0x047A),
    (0x047D, 0x047C),
    (0x047F, 0x047E),
    (0x0481, 0x0480),
    (0x048B, 0x048A),
    (0x048D, 0x048C),
    (0x048F, 0x048E),
    (0x0491, 0x0490),
    (0x0493, 0x0492),
    (0x0495, 0x0494),
    (0x0497, 0x0496),
    (0x0499, 0x0498),
    (0x049B, 0x049A),
    (0x049D, 0x049C),
    (0x049F, 0x049E),
    (0x04A1, 0x04A0),
    (0x04A3, 0x04A2),
    (0x04A5, 0x04A4),
    (0x04A7, 0x04A6),
    (0x04A9, 0x04A8),
    (0x04AB, 0x04AA),
    (0x04AD, 0x04AC),
    (0x04AF, 0x04AE),
    (0x04B1, 0x04B0),
    (0x04B3, 0x04B2),
    (0x04B5, 0x04B4),
    (0x04B7, 0x04B6),
    (0x04B9, 0x04B8),
    (0x04BB, 0x04BA),
    (0x04BD, 0x04BC),
    (0x04BF, 0x04BE),
    (0x04C2, 0x04C1),
    (0x04C4, 0x04C3),
    (0x04C6, 0x04C5),
    (0x04C8, 0x04C7),
    (0x04CA, 0x04C9),
    (0x04CC, 0x04CB),
    (0x04CE, 0x04CD),
    (0x04CF, 0x04C0),
    (0x04D1, 0x04D0),
    (0x04D3, 0x04D2),
    (0x04D5, 0x04D4),
    (0x04D7, 0x04D6),
    (0x04D9, 0x04D8),
    (0x04DB, 0x04DA),
    (0x04DD, 0x04DC),
    (0x04DF, 0x04DE),
    (0x04E1, 0x04E0),
    (0x04E3, 0x04E2),
    (0x04E5, 0x04E4),
    (0x04E7, 0x04E6),
    (0x04E9, 0x04E8),
    (0x04EB, 0x04EA),
    (0x04ED, 0x04EC),
    (0x04EF, 0x04EE),
    (0x04F1, 0x04F0),
    (0x04F3, 0x04F2),
    (0x04F5, 0x04F4),
    (0x04F7, 0x04F6),
    (0x04F9, 0x04F8),
    (0x04FB, 0x04FA),
    (0x04FD, 0x04FC),
    (0x04FF, 0x04FE),
    (0x0501, 0x0500),
    (0x0503, 0x0502),
    (0x0505, 0x0504),
    (0x0507, 0x0506),
    (0x0509, 0x0508),
    (0x050B, 0x050A),
    (0x050D, 0x050C),
    (0x050F, 0x050E),
    (0x0511, 0x0510),
    (0x0513, 0x0512),
    (0x0515, 0x0514),
    (0x0517, 0x0516),
    (0x0519, 0x0518),
    (0x051B, 0x051A),
    (0x051D, 0x051C),
    (0x051F, 0x051E),
    (0x0521, 0x0520),
    (0x0523, 0x0522),
    (0x0525, 0x0524),
    (0x0527, 0x0526),
    (0x0529, 0x0528),
    (0x052B, 0x052A),
    (0x052D, 0x052C),
    (0x052F, 0x052E),
    (0x0561, 0x0531),
    (0x0562, 0x0532),
    (0x0563, 0x0533),
    (0x0564, 0x0534),
    (0x0565, 0x0535),
    (0x0566, 0x0536),
    (0x0567, 0x0537),
    (0x0568, 0x0538),
    (0x0569, 0x0539),
    (0x056A, 0x053A),
    (0x056B, 0x053B),
    (0x056C, 0x053C),
    (0x056D, 0x053D),
    (0x056E, 0x053E),
    (0x056F, 0x053F),
    (0x0570, 0x0540),
    (0x0571, 0x0541),
    (0x0572, 0x0542),
    (0x0573, 0x0543),
    (0x0574, 0x0544),
    (0x0575, 0x0545),
    (0x0576, 0x0546),
    (0x0577, 0x0547),
    (0x0578, 0x0548),
    (0x0579, 0x0549),
    (0x057A, 0x054A),
    (0x057B, 0x054B),
    (0x057C, 0x054C),
    (0x057D, 0x054D),
    (0x057E, 0x054E),
    (0x057F, 0x054F),
    (0x0580, 0x0550),
    (0x0581, 0x0551),
    (0x0582, 0x0552),
    (0x0583, 0x0553),
    (0x0584, 0x0554),
    (0x0585, 0x0555),
    (0x0586, 0x0556),
    (0x13F8, 0x13F0),
    (0x13F9, 0x13F1),
    (0x13FA, 0x13F2),
    (0x13FB, 0x13F3),
    (0x13FC, 0x13F4),
    (0x13FD, 0x13F5),
    (0x1C80, 0x0412),
    (0x1C81, 0x0414),
    (0x1C82, 0x041E),
    (0x1C83, 0x0421),
    (0x1C84, 0x0422),
    (0x1C85, 0x0422),
    (0x1C86, 0x042A),
    (0x1C87, 0x0462),
    (0x1C90, 0x10D0),
    (0x1C91, 0x10D1),
    (0x1C92, 0x10D2),
    (0x1C93, 0x10D3),
    (0x1C94, 0x10D4),
    (0x1C95, 0x10D5),
    (0x1C96, 0x10D6),
    (0x1C97, 0x10D7),
    (0x1C98, 0x10D8),
    (0x1C99, 0x10D9),
    (0x1C9A, 0x10DA),
    (0x1C9B, 0x10DB),
    (0x1C9C, 0x10DC),
    (0x1C9D, 0x10DD),
    (0x1C9E, 0x10DE),
    (0x1C9F, 0x10DF),
    (0x1CA0, 0x10E0),
    (0x1CA1, 0x10E1),
    (0x1CA2, 0x10E2),
    (0x1CA3, 0x10E3),
    (0x1CA4, 0x10E4),
    (0x1CA5, 0x10E5),
    (0x1CA6, 0x10E6),
    (0x1CA7, 0x10E7),
    (0x1CA8, 0x10E8),
    (0x1CA9, 0x10E9),
    (0x1CAA, 0x10EA),
    (0x1CAB, 0x10EB),
    (0x1CAC, 0x10EC),
    (0x1CAD, 0x10ED),
    (0x1CAE, 0x10EE),
    (0x1CAF, 0x10EF),
    (0x1CB0, 0x10F0),
    (0x1CB1, 0x10F1),
    (0x1CB2, 0x10F2),
    (0x1CB3, 0x10F3),
    (0x1CB4, 0x10F4),
    (0x1CB5, 0x10F5),
    (0x1CB6, 0x10F6),
    (0x1CB7, 0x10F7),
    (0x1CB8, 0x10F8),
    (0x1CB9, 0x10F9),
    (0x1CBA, 0x10FA),
    (0x1CBD, 0x10FD),
    (0x1CBE, 0x10FE),
    (0x1CBF, 0x10FF),
    (0x1E01, 0x1E00),
    (0x1E03, 0x1E02),
    (0x1E05, 0x1E04),
    (0x1E07, 0x1E06),
    (0x1E09, 0x1E08),
    (0x1E0B, 0x1E0A),
    (0x1E0D, 0x1E0C),
    (0x1E0F, 0x1E0E),
    (0x1E11, 0x1E10),
    (0x1E13, 0x1E12),
    (0x1E15, 0x1E14),
    (0x1E17, 0x1E16),
    (0x1E19, 0x1E18),
    (0x1E1B, 0x1E1A),
    (0x1E1D, 0x1E1C),
    (0x1E1F, 0x1E1E),
    (0x1E21, 0x1E20),
    (0x1E23, 0x1E22),
    (0x1E25, 0x1E24),
    (0x1E27, 0x1E26),
    (0x1E29, 0x1E28),
    (0x1E2B, 0x1E2A),
    (0x1E2D, 0x1E2C),
    (0x1E2F, 0x1E2E),
    (0x1E31, 0x1E30),
    (0x1E33, 0x1E32),
    (0x1E35, 0x1E34),
    (0x1E37, 0x1E36),
    (0x1E39, 0x1E38),
    (0x1E3B, 0x1E3A),
    (0x1E3D, 0x1E3C),
    (0x1E3F, 0x1E3E),
    (0x1E41, 0x1E40),
    (0x1E43, 0x1E42),
    (0x1E45, 0x1E44),
    (0x1E47, 0x1E46),
    (0x1E49, 0x1E48),
    (0x1E4B, 0x1E4A),
    (0x1E4D, 0x1E4C),
    (0x1E4F, 0x1E4E),
    (0x1E51, 0x1E50),
    (0x1E53, 0x1E52),
    (0x1E55, 0x1E54),
    (0x1E57, 0x1E56),
    (0x1E59, 0x1E58),
    (0x1E5B, 0x1E5A),
    (0x1E5D, 0x1E5C),
    (0x1E5F, 0x1E5E),
    (0x1E61, 0x1E60),
    (0x1E63, 0x1E62),
    (0x1E65, 0x1E64),
    (0x1E67, 0x1E66),
    (0x1E69, 0x1E68),
    (0x1E6B, 0x1E6A),
    (0x1E6D, 0x1E6C),
    (0x1E6F, 0x1E6E),
    (0x1E71, 0x1E70),
    (0x1E73, 0x1E72),
    (0x1E75, 0x1E74),
    (0x1E77, 0x1E76),
    (0x1E79, 0x1E78),
    (0x1E7B, 0x1E7A),
    (0x1E7D, 0x1E7C),
    (0x1E7F, 0x1E7E),
    (0x1E81, 0x1E80),
    (0x1E83, 0x1E82),
    (0x1E85, 0x1E84),
    (0x1E87, 0x1E86),
    (0x1E89, 0x1E88),
    (0x1E8B, 0x1E8A),
    (0x1E8D, 0x1E8C),
    (0x1E8F, 0x1E8E),
    (0x1E91, 0x1E90),
    (0x1E93, 0x1E92),
    (0x1E95, 0x1E94),
    (0x1E9B, 0x1E60),
    (0x1E9E, 0x00DF),
    (0x1EA1, 0x1EA0),
    (0x1EA3, 0x1EA2),
    (0x1EA5, 0x1EA4),
    (0x1EA7, 0x1EA6),
    (0x1EA9, 0x1EA8),
    (0x1EAB, 0x1EAA),
    (0x1EAD, 0x1EAC),
    (0x1EAF, 0x1EAE),
    (0x1EB1, 0x1EB0),
    (0x1EB3, 0x1EB2),
    (0x1EB5, 0x1EB4),
    (0x1EB7, 0x1EB6),
    (0x1EB9, 0x1EB8),
    (0x1EBB, 0x1EBA),
    (0x1EBD, 0x1EBC),
    (0x1EBF, 0x1EBE),
    (0x1EC1, 0x1EC0),
    (0x1EC3, 0x1EC2),
    (0x1EC5, 0x1EC4),
    (0x1EC7, 0x1EC6),
    (0x1EC9, 0x1EC8),
    (0x1ECB, 0x1ECA),
    (0x1ECD, 0x1ECC),
    (0x1ECF, 0x1ECE),
    (0x1ED1, 0x1ED0),
    (0x1ED3, 0x1ED2),
    (0x1ED5, 0x1ED4),
    (0x1ED7, 0x1ED6),
    (0x1ED9, 0x1ED8),
    (0x1EDB, 0x1EDA),
    (0x1EDD, 0x1EDC),
    (0x1EDF, 0x1EDE),
    (0x1EE1, 0x1EE0),
    (0x1EE3, 0x1EE2),
    (0x1EE5, 0x1EE4),
    (0x1EE7, 0x1EE6),
    (0x1EE9, 0x1EE8),
    (0x1EEB, 0x1EEA),
    (0x1EED, 0x1EEC),
    (0x1EEF, 0x1EEE),
    (0x1EF1, 0x1EF0),
    (0x1EF3, 0x1EF2),
    (0x1EF5, 0x1EF4),
    (0x1EF7, 0x1EF6),
    (0x1EF9, 0x1EF8),
    (0x1EFB, 0x1EFA),
    (0x1EFD, 0x1EFC),
    (0x1EFF, 0x1EFE),
    (0x1F08, 0x1F00),
    (0x1F09, 0x1F01),
    (0x1F0A, 0x1F02),
    (0x1F0B, 0x1F03),
    (0x1F0C, 0x1F04),
    (0x1F0D, 0x1F05),
    (0x1F0E, 0x1F06),
    (0x1F0F, 0x1F07),
    (0x1F18, 0x1F10),
    (0x1F19, 0x1F11),
    (0x1F1A, 0x1F12),
    (0x1F1B, 0x1F13),
    (0x1F1C, 0x1F14),
    (0x1F1D, 0x1F15),
    (0x1F28, 0x1F20),
    (0x1F29, 0x1F21),
    (0x1F2A, 0x1F22),
    (0x1F2B, 0x1F23),
    (0x1F2C, 0x1F24),
    (0x1F2D, 0x1F25),
    (0x1F2E, 0x1F26),
    (0x1F2F, 0x1F27),
    (0x1F38, 0x1F30),
    (0x1F39, 0x1F31),
    (0x1F3A, 0x1F32),
    (0x1F3B, 0x1F33),
    (0x1F3C, 0x1F34),
    (0x1F3D, 0x1F35),
    (0x1F3E, 0x1F36),
    (0x1F3F, 0x1F37),
    (0x1F48, 0x1F40),
    (0x1F49, 0x1F41),
    (0x1F4A, 0x1F42),
    (0x1F4B, 0x1F43),
    (0x1F4C, 0x1F44),
    (0x1F4D, 0x1F45),
    (0x1F59, 0x1F51),
    (0x1F5B, 0x1F53),
    (0x1F5D, 0x1F55),
    (0x1F5F, 0x1F57),
    (0x1F68, 0x1F60),
    (0x1F69, 0x1F61),
    (0x1F6A, 0x1F62),
    (0x1F6B, 0x1F63),
    (0x1F6C, 0x1F64),
    (0x1F6D, 0x1F65),
    (0x1F6E, 0x1F66),
    (0x1F6F, 0x1F67),
    (0x1F88, 0x1F80),
    (0x1F89, 0x1F81),
    (0x1F8A, 0x1F82),
    (0x1F8B, 0x1F83),
    (0x1F8C, 0x1F84),
    (0x1F8D, 0x1F85),
    (0x1F8E, 0x1F86),
    (0x1F8F, 0x1F87),
    (0x1F98, 0x1F90),
    (0x1F99, 0x1F91),
    (0x1F9A, 0x1F92),
    (0x1F9B, 0x1F93),
    (0x1F9C, 0x1F94),
    (0x1F9D, 0x1F95),
    (0x1F9E, 0x1F96),
    (0x1F9F, 0x1F97),
    (0x1FA8, 0x1FA0),
    (0x1FA9, 0x1FA1),
    (0x1FAA, 0x1FA2),
    (0x1FAB, 0x1FA3),
    (0x1FAC, 0x1FA4),
    (0x1FAD, 0x1FA5),
    (0x1FAE, 0x1FA6),
    (0x1FAF, 0x1FA7),
    (0x1FB8, 0x1FB0),
    (0x1FB9, 0x1FB1),
    (0x1FBA, 0x1F70),
    (0x1FBB, 0x1F71),
    (0x1FBC, 0x1FB3),
    (0x1FBE, 0x0345),
    (0x1FC8, 0x1F72),
    (0x1FC9, 0x1F73),
    (0x1FCA, 0x1F74),
    (0x1FCB, 0x1F75),
    (0x1FCC, 0x1FC3),
    (0x1FD3, 0x0390),
    (0x1FD8, 0x1FD0),
    (0x1FD9, 0x1FD1),
    (0x1FDA, 0x1F76),
    (0x1FDB, 0x1F77),
    (0x1FE3, 0x03B0),
    (0x1FE8, 0x1FE0),
    (0x1FE9, 0x1FE1),
    (0x1FEA, 0x1F7A),
    (0x1FEB, 0x1F7B),
    (0x1FEC, 0x1FE5),
    (0x1FF8, 0x1F78),
    (0x1FF9, 0x1F79),
    (0x1FFA, 0x1F7C),
    (0x1FFB, 0x1F7D),
    (0x1FFC, 0x1FF3),
    (0x2126, 0x03A9),
    (0x212A, 0x004B),
    (0x212B, 0x00C5),
    (0x214E, 0x2132),
    (0x2170, 0x2160),
    (0x2171, 0x2161),
    (0x2172, 0x2162),
    (0x2173, 0x2163),
    (0x2174, 0x2164),
    (0x2175, 0x2165),
    (0x2176, 0x2166),
    (0x2177, 0x2167),
    (0x2178, 0x2168),
    (0x2179, 0x2169),
    (0x217A, 0x216A),
    (0x217B, 0x216B),
    (0x217C, 0x216C),
    (0x217D, 0x216D),
    (0x217E, 0x216E),
    (0x217F, 0x216F),
    (0x2184, 0x2183),
    (0x24D0, 0x24B6),
    (0x24D1, 0x24B7),
    (0x24D2, 0x24B8),
    (0x24D3, 0x24B9),
    (0x24D4, 0x24BA),
    (0x24D5, 0x24BB),
    (0x24D6, 0x24BC),
    (0x24D7, 0x24BD),
    (0x24D8, 0x24BE),
    (0x24D9, 0x24BF),
    (0x24DA, 0x24C0),
    (0x24DB, 0x24C1),
    (0x24DC, 0x24C2),
    (0x24DD, 0x24C3),
    (0x24DE, 0x24C4),
    (0x24DF, 0x24C5),
    (0x24E0, 0x24C6),
    (0x24E1, 0x24C7),
    (0x24E2, 0x24C8),
    (0x24E3, 0x24C9),
    (0x24E4, 0x24CA),
    (0x24E5, 0x24CB),
    (0x24E6, 0x24CC),
    (0x24E7, 0x24CD),
    (0x24E8, 0x24CE),
    (0x24E9, 0x24CF),
    (0x2C30, 0x2C00),
    (0x2C31, 0x2C01),
    (0x2C32, 0x2C02),
    (0x2C33, 0x2C03),
    (0x2C34, 0x2C04),
    (0x2C35, 0x2C05),
    (0x2C36, 0x2C06),
    (0x2C37, 0x2C07),
    (0x2C38, 0x2C08),
    (0x2C39, 0x2C09),
    (0x2C3A, 0x2C0A),
    (0x2C3B, 0x2C0B),
    (0x2C3C, 0x2C0C),
    (0x2C3D, 0x2C0D),
    (0x2C3E, 0x2C0E),
    (0x2C3F, 0x2C0F),
    (0x2C40, 0x2C10),
    (0x2C41, 0x2C11),
    (0x2C42, 0x2C12),
    (0x2C43, 0x2C13),
    (0x2C44, 0x2C14),
    (0x2C45, 0x2C15),
    (0x2C46, 0x2C16),
    (0x2C47, 0x2C17),
    (0x2C48, 0x2C18),
    (0x2C49, 0x2C19),
    (0x2C4A, 0x2C1A),
    (0x2C4B, 0x2C1B),
    (0x2C4C, 0x2C1C),
    (0x2C4D, 0x2C1D),
    (0x2C4E, 0x2C1E),
    (0x2C4F, 0x2C1F),
    (0x2C50, 0x2C20),
    (0x2C51, 0x2C21),
    (0x2C52, 0x2C22),
    (0x2C53, 0x2C23),
    (0x2C54, 0x2C24),
    (0x2C55, 0x2C25),
    (0x2C56, 0x2C26),
    (0x2C57, 0x2C27),
    (0x2C58, 0x2C28),
    (0x2C59, 0x2C29),
    (0x2C5A, 0x2C2A),
    (0x2C5B, 0x2C2B),
    (0x2C5C, 0x2C2C),
    (0x2C5D, 0x2C2D),
    (0x2C5E, 0x2C2E),
    (0x2C5F, 0x2C2F),
    (0x2C61, 0x2C60),
    (0x2C62, 0x026B),
    (0x2C63, 0x1D7D),
    (0x2C64, 0x027D),
    (0x2C65, 0x023A),
    (0x2C66, 0x023E),
    (0x2C68, 0x2C67),
    (0x2C6A, 0x2C69),
    (0x2C6C, 0x2C6B),
    (0x2C6D, 0x0251),
    (0x2C6E, 0x0271),
    (0x2C6F, 0x0250),
    (0x2C70, 0x0252),
    (0x2C73, 0x2C72),
    (0x2C76, 0x2C75),
    (0x2C7E, 0x023F),
    (0x2C7F, 0x0240),
    (0x2C81, 0x2C80),
    (0x2C83, 0x2C82),
    (0x2C85, 0x2C84),
    (0x2C87, 0x2C86),
    (0x2C89, 0x2C88),
    (0x2C8B, 0x2C8A),
    (0x2C8D, 0x2C8C),
    (0x2C8F, 0x2C8E),
    (0x2C91, 0x2C90),
    (0x2C93, 0x2C92),
    (0x2C95, 0x2C94),
    (0x2C97, 0x2C96),
    (0x2C99, 0x2C98),
    (0x2C9B, 0x2C9A),
    (0x2C9D, 0x2C9C),
    (0x2C9F, 0x2C9E),
    (0x2CA1, 0x2CA0),
    (0x2CA3, 0x2CA2),
    (0x2CA5, 0x2CA4),
    (0x2CA7, 0x2CA6),
    (0x2CA9, 0x2CA8),
    (0x2CAB, 0x2CAA),
    (0x2CAD, 0x2CAC),
    (0x2CAF, 0x2CAE),
    (0x2CB1, 0x2CB0),
    (0x2CB3, 0x2CB2),
    (0x2CB5, 0x2CB4),
    (0x2CB7, 0x2CB6),
    (0x2CB9, 0x2CB8),
    (0x2CBB, 0x2CBA),
    (0x2CBD, 0x2CBC),
    (0x2CBF, 0x2CBE),
    (0x2CC1, 0x2CC0),
    (0x2CC3, 0x2CC2),
    (0x2CC5, 0x2CC4),
    (0x2CC7, 0x2CC6),
    (0x2CC9, 0x2CC8),
    (0x2CCB, 0x2CCA),
    (0x2CCD, 0x2CCC),
    (0x2CCF, 0x2CCE),
    (0x2CD1, 0x2CD0),
    (0x2CD3, 0x2CD2),
    (0x2CD5, 0x2CD4),
    (0x2CD7, 0x2CD6),
    (0x2CD9, 0x2CD8),
    (0x2CDB, 0x2CDA),
    (0x2CDD, 0x2CDC),
    (0x2CDF, 0x2CDE),
    (0x2CE1, 0x2CE0),
    (0x2CE3, 0x2CE2),
    (0x2CEC, 0x2CEB),
    (0x2CEE, 0x2CED),
    (0x2CF3, 0x2CF2),
    (0x2D00, 0x10A0),
    (0x2D01, 0x10A1),
    (0x2D02, 0x10A2),
    (0x2D03, 0x10A3),
    (0x2D04, 0x10A4),
    (0x2D05, 0x10A5),
    (0x2D06, 0x10A6),
    (0x2D07, 0x10A7),
    (0x2D08, 0x10A8),
    (0x2D09, 0x10A9),
    (0x2D0A, 0x10AA),
    (0x2D0B, 0x10AB),
    (0x2D0C, 0x10AC),
    (0x2D0D, 0x10AD),
    (0x2D0E, 0x10AE),
    (0x2D0F, 0x10AF),
    (0x2D10, 0x10B0),
    (0x2D11, 0x10B1),
    (0x2D12, 0x10B2),
    (0x2D13, 0x10B3),
    (0x2D14, 0x10B4),
    (0x2D15, 0x10B5),
    (0x2D16, 0x10B6),
    (0x2D17, 0x10B7),
    (0x2D18, 0x10B8),
    (0x2D19, 0x10B9),
    (0x2D1A, 0x10BA),
    (0x2D1B, 0x10BB),
    (0x2D1C, 0x10BC),
    (0x2D1D, 0x10BD),
    (0x2D1E, 0x10BE),
    (0x2D1F, 0x10BF),
    (0x2D20, 0x10C0),
    (0x2D21, 0x10C1),
    (0x2D22, 0x10C2),
    (0x2D23, 0x10C3),
    (0x2D24, 0x10C4),
    (0x2D25, 0x10C5),
    (0x2D27, 0x10C7),
    (0x2D2D, 0x10CD),
    (0xA641, 0xA640),
    (0xA643, 0xA642),
    (0xA645, 0xA644),
    (0xA647, 0xA646),
    (0xA649, 0xA648),
    (0xA64A, 0x1C88),
    (0xA64B, 0x1C88),
    (0xA64D, 0xA64C),
    (0xA64F, 0xA64E),
    (0xA651, 0xA650),
    (0xA653, 0xA652),
    (0xA655, 0xA654),
    (0xA657, 0xA656),
    (0xA659, 0xA658),
    (0xA65B, 0xA65A),
    (0xA65D, 0xA65C),
    (0xA65F, 0xA65E),
    (0xA661, 0xA660),
    (0xA663, 0xA662),
    (0xA665, 0xA664),
    (0xA667, 0xA666),
    (0xA669, 0xA668),
    (0xA66B, 0xA66A),
    (0xA66D, 0xA66C),
    (0xA681, 0xA680),
    (0xA683, 0xA682),
    (0xA685, 0xA684),
    (0xA687, 0xA686),
    (0xA689, 0xA688),
    (0xA68B, 0xA68A),
    (0xA68D, 0xA68C),
    (0xA68F, 0xA68E),
    (0xA691, 0xA690),
    (0xA693, 0xA692),
    (0xA695, 0xA694),
    (0xA697, 0xA696),
    (0xA699, 0xA698),
    (0xA69B, 0xA69A),
    (0xA723, 0xA722),
    (0xA725, 0xA724),
    (0xA727, 0xA726),
    (0xA729, 0xA728),
    (0xA72B, 0xA72A),
    (0xA72D, 0xA72C),
    (0xA72F, 0xA72E),
    (0xA733, 0xA732),
    (0xA735, 0xA734),
    (0xA737, 0xA736),
    (0xA739, 0xA738),
    (0xA73B, 0xA73A),
    (0xA73D, 0xA73C),
    (0xA73F, 0xA73E),
    (0xA741, 0xA740),
    (0xA743, 0xA742),
    (0xA745, 0xA744),
    (0xA747, 0xA746),
    (0xA749, 0xA748),
    (0xA74B, 0xA74A),
    (0xA74D, 0xA74C),
    (0xA74F, 0xA74E),
    (0xA751, 0xA750),
    (0xA753, 0xA752),
    (0xA755, 0xA754),
    (0xA757, 0xA756),
    (0xA759, 0xA758),
    (0xA75B, 0xA75A),
    (0xA75D, 0xA75C),
    (0xA75F, 0xA75E),
    (0xA761, 0xA760),
    (0xA763, 0xA762),
    (0xA765, 0xA764),
    (0xA767, 0xA766),
    (0xA769, 0xA768),
    (0xA76B, 0xA76A),
    (0xA76D, 0xA76C),
    (0xA76F, 0xA76E),
    (0xA77A, 0xA779),
    (0xA77C, 0xA77B),
    (0xA77D, 0x1D79),
    (0xA77F, 0xA77E),
    (0xA781, 0xA780),
    (0xA783, 0xA782),
    (0xA785, 0xA784),
    (0xA787, 0xA786),
    (0xA78C, 0xA78B),
    (0xA78D, 0x0265),
    (0xA791, 0xA790),
    (0xA793, 0xA792),
    (0xA797, 0xA796),
    (0xA799, 0xA798),
    (0xA79B, 0xA79A),
    (0xA79D, 0xA79C),
    (0xA79F, 0xA79E),
    (0xA7A1, 0xA7A0),
    (0xA7A3, 0xA7A2),
    (0xA7A5, 0xA7A4),
    (0xA7A7, 0xA7A6),
    (0xA7A9, 0xA7A8),
    (0xA7AA, 0x0266),
    (0xA7AB, 0x025C),
    (0xA7AC, 0x0261),
    (0xA7AD, 0x026C),
    (0xA7AE, 0x026A),
    (0xA7B0, 0x029E),
    (0xA7B1, 0x0287),
    (0xA7B2, 0x029D),
    (0xA7B5, 0xA7B4),
    (0xA7B7, 0xA7B6),
    (0xA7B9, 0xA7B8),
    (0xA7BB, 0xA7BA),
    (0xA7BD, 0xA7BC),
    (0xA7BF, 0xA7BE),
    (0xA7C1, 0xA7C0),
    (0xA7C3, 0xA7C2),
    (0xA7C4, 0xA794),
    (0xA7C5, 0x0282),
    (0xA7C6, 0x1D8E),
    (0xA7C8, 0xA7C7),
    (0xA7CA, 0xA7C9),
    (0xA7D1, 0xA7D0),
    (0xA7D7, 0xA7D6),
    (0xA7D9, 0xA7D8),
    (0xA7F6, 0xA7F5),
    (0xAB53, 0xA7B3),
    (0xAB70, 0x13A0),
    (0xAB71, 0x13A1),
    (0xAB72, 0x13A2),
    (0xAB73, 0x13A3),
    (0xAB74, 0x13A4),
    (0xAB75, 0x13A5),
    (0xAB76, 0x13A6),
    (0xAB77, 0x13A7),
    (0xAB78, 0x13A8),
    (0xAB79, 0x13A9),
    (0xAB7A, 0x13AA),
    (0xAB7B, 0x13AB),
    (0xAB7C, 0x13AC),
    (0xAB7D, 0x13AD),
    (0xAB7E, 0x13AE),
    (0xAB7F, 0x13AF),
    (0xAB80, 0x13B0),
    (0xAB81, 0x13B1),
    (0xAB82, 0x13B2),
    (0xAB83, 0x13B3),
    (0xAB84, 0x13B4),
    (0xAB85, 0x13B5),
    (0xAB86, 0x13B6),
    (0xAB87, 0x13B7),
    (0xAB88, 0x13B8),
    (0xAB89, 0x13B9),
    (0xAB8A, 0x13BA),
    (0xAB8B, 0x13BB),
    (0xAB8C, 0x13BC),
    (0xAB8D, 0x13BD),
    (0xAB8E, 0x13BE),
    (0xAB8F, 0x13BF),
    (0xAB90, 0x13C0),
    (0xAB91, 0x13C1),
    (0xAB92, 0x13C2),
    (0xAB93, 0x13C3),
    (0xAB94, 0x13C4),
    (0xAB95, 0x13C5),
    (0xAB96, 0x13C6),
    (0xAB97, 0x13C7),
    (0xAB98, 0x13C8),
    (0xAB99, 0x13C9),
    (0xAB9A, 0x13CA),
    (0xAB9B, 0x13CB),
    (0xAB9C, 0x13CC),
    (0xAB9D, 0x13CD),
    (0xAB9E, 0x13CE),
    (0xAB9F, 0x13CF),
    (0xABA0, 0x13D0),
    (0xABA1, 0x13D1),
    (0xABA2, 0x13D2),
    (0xABA3, 0x13D3),
    (0xABA4, 0x13D4),
    (0xABA5, 0x13D5),
    (0xABA6, 0x13D6),
    (0xABA7, 0x13D7),
    (0xABA8, 0x13D8),
    (0xABA9, 0x13D9),
    (0xABAA, 0x13DA),
    (0xABAB, 0x13DB),
    (0xABAC, 0x13DC),
    (0xABAD, 0x13DD),
    (0xABAE, 0x13DE),
    (0xABAF, 0x13DF),
    (0xABB0, 0x13E0),
    (0xABB1, 0x13E1),
    (0xABB2, 0x13E2),
    (0xABB3, 0x13E3),
    (0xABB4, 0x13E4),
    (0xABB5, 0x13E5),
    (0xABB6, 0x13E6),
    (0xABB7, 0x13E7),
    (0xABB8, 0x13E8),
    (0xABB9, 0x13E9),
    (0xABBA, 0x13EA),
    (0xABBB, 0x13EB),
    (0xABBC, 0x13EC),
    (0xABBD, 0x13ED),
    (0xABBE, 0x13EE),
    (0xABBF, 0x13EF),
    (0xFB06, 0xFB05),
    (0xFF41, 0xFF21),
    (0xFF42, 0xFF22),
    (0xFF43, 0xFF23),
    (0xFF44, 0xFF24),
    (0xFF45, 0xFF25),
    (0xFF46, 0xFF26),
    (0xFF47, 0xFF27),
    (0xFF48, 0xFF28),
    (0xFF49, 0xFF29),
    (0xFF4A, 0xFF2A),
    (0xFF4B, 0xFF2B),
    (0xFF4C, 0xFF2C),
    (0xFF4D, 0xFF2D),
    (0xFF4E, 0xFF2E),
    (0xFF4F, 0xFF2F),
    (0xFF50, 0xFF30),
    (0xFF51, 0xFF31),
    (0xFF52, 0xFF32),
    (0xFF53, 0xFF33),
    (0xFF54, 0xFF34),
    (0xFF55, 0xFF35),
    (0xFF56, 0xFF36),
    (0xFF57, 0xFF37),
    (0xFF58, 0xFF38),
    (0xFF59, 0xFF39),
    (0xFF5A, 0xFF3A),
];

/// Der **Faltungsvertreter** eines Zeichens (Feld `FALTUNG`).
///
/// GEMESSEN 04.10. (Orakel `node` v22): `klein()` allein verfehlt die
/// Faelle, in denen die *beiden* Schreibweisen zu einer **mehrzeichenigen**
/// Form fuehren — `[\u0390]/ui` trifft `\u1FD3`, und `lower(U+0390)` ist
/// `\u03CA`. Die Tabelle verknuepft alle Zeichen mit **gleicher** Form
/// (siehe ihre Doku).
///
/// M3/M5: **binaere** Suche (die Liste ist sortiert) — ein linearer
/// Scan waere je Zeichen 1198 Vergleiche.
fn falte(u: u16) -> u16 {
    match FALTUNG.binary_search_by(|paar| paar.0.cmp(&u)) {
        Ok(i) => FALTUNG[i].1,
        Err(_) => u,
    }
}

/// Sind zwei Zeichen **ohne** Beachtung der Gross-/Kleinschreibung gleich?
fn gleich_ungleich_case(a: u16, b: u16, einheiten: bool) -> bool {
    if a == b {
        return true;
    }
    // Ohne `u` vergleicht die Spec **kanonisiert** (ECMA-262 22.2.2.8), mit
    // `u` **gefaltet**. GEMESSEN 04.10. (`node` v22, 44 Handpaare): ohne `u`
    // ist der gefaltete Vergleich an **acht** Zeichen zu weit (Kelvin U+212A,
    // Langer s U+017F, `\u00DF`/`\u1E9E`, punktloses I U+0131, U+0390 gegen
    // U+1FD3, Angstrom U+212B).
    if einheiten {
        falte(a) == falte(b)
    } else {
        kanonisiere(a) == kanonisiere(b)
    }
}

/// `Canonicalize` nach ECMA-262 22.1.1.17: **Grossschreibung**, aber nur,
/// wenn sie **eine** Codeeinheit ergibt; U+212A und U+212B sind
/// ausgenommen (der Spec-Vorbehalt gegen das Kelvinzeichen).
fn kanonisiere(u: u16) -> u16 {
    if u < 128 {
        return (u as u8).to_ascii_uppercase() as u16;
    }
    if u == 0x212A || u == 0x212B {
        return u;
    }
    match char::decode_utf16([u].iter().copied()).next() {
        Some(Ok(c)) => {
            let mut it = c.to_uppercase();
            match (it.next(), it.next()) {
                (Some(g), None) if (g as u32) <= 0xFFFF => g as u16,
                _ => u,
            }
        }
        _ => u,
    }
}

fn klein(u: u16) -> u16 {
    // ASCII + einfache Unicode-Kleinschreibung ueber char.
    if u < 128 {
        (u as u8).to_ascii_lowercase() as u16
    } else {
        match char::decode_utf16([u].iter().copied()).next() {
            Some(Ok(c)) => c.to_lowercase().next().unwrap_or(c) as u16,
            _ => u,
        }
    }
}

fn ist_wort(u: u16) -> bool {
    matches!(u, 0x30..=0x39 | 0x41..=0x5A | 0x5F | 0x61..=0x7A)
        || (u >= 128
            && match char::decode_utf16([u].iter().copied()).next() {
                Some(Ok(c)) => c.is_alphanumeric(),
                _ => false,
            })
}

/// Text in UTF-16-Einheiten (fuer JS-Index-Semantik).
pub(crate) fn als_einheiten(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Einheiten zurueck in String (Ersatzzeichen bei Einzel-Surrogaten).
pub(crate) fn als_text(einheiten: &[u16]) -> String {
    char::decode_utf16(einheiten.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

// ===========================================================================
// Release 15.10.2026: code point classes (`u`/`v` mode), the `v` class
// syntax (set operations, nested classes, `\q{…}`) and pattern modifiers.
// ===========================================================================

/// The largest code point.
const MAX_CP_REGEX: u32 = 0x10FFFF;

/// The value of a class operand in `v` mode: code points plus strings
/// (ECMA-262 22.2.2.9, CharSet with strings). Strings of exactly one code
/// point live in `bereiche`; `texte` holds the empty string and strings of
/// two or more code points.
#[derive(Debug, Clone, Default)]
struct CpMenge {
    bereiche: Vec<(u32, u32)>,
    texte: Vec<Vec<u32>>,
}

impl CpMenge {
    fn aus(bereiche: Vec<(u32, u32)>) -> CpMenge {
        CpMenge { bereiche, texte: Vec::new() }
    }

    fn vereinigen(&mut self, o: CpMenge) {
        self.bereiche.extend(o.bereiche);
        self.bereiche = cp_merzen(std::mem::take(&mut self.bereiche));
        for t in o.texte {
            if !self.texte.contains(&t) {
                self.texte.push(t);
            }
        }
    }

    fn schneiden(self, o: CpMenge) -> CpMenge {
        CpMenge {
            bereiche: cp_schnitt(&self.bereiche, &o.bereiche),
            texte: self.texte.into_iter().filter(|t| o.texte.contains(t)).collect(),
        }
    }

    fn abziehen(self, o: CpMenge) -> CpMenge {
        CpMenge {
            bereiche: cp_schnitt(&self.bereiche, &cp_komplement(&o.bereiche)),
            texte: self.texte.into_iter().filter(|t| !o.texte.contains(t)).collect(),
        }
    }
}

/// Sort and merge code point ranges.
fn cp_merzen(mut b: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    b.sort_unstable();
    let mut raus: Vec<(u32, u32)> = Vec::with_capacity(b.len());
    for (lo, hi) in b {
        if let Some(l) = raus.last_mut()
            && lo <= l.1.saturating_add(1)
        {
            if hi > l.1 {
                l.1 = hi;
            }
            continue;
        }
        raus.push((lo, hi));
    }
    raus
}

/// Is `t` in the sorted, merged list `b`? (binary search)
fn cp_enthaelt(b: &[(u32, u32)], t: u32) -> bool {
    let i = b.partition_point(|(_, hi)| *hi < t);
    matches!(b.get(i), Some((lo, hi)) if t >= *lo && t <= *hi)
}

/// All code points not in the merged list `b`.
fn cp_komplement(b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut raus = Vec::with_capacity(b.len() + 1);
    let mut naechste = 0u32;
    for &(lo, hi) in b {
        if lo > naechste {
            raus.push((naechste, lo - 1));
        }
        if hi >= MAX_CP_REGEX {
            return raus;
        }
        naechste = naechste.max(hi + 1);
    }
    raus.push((naechste, MAX_CP_REGEX));
    raus
}

/// The intersection of two merged lists.
fn cp_schnitt(a: &[(u32, u32)], b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let (mut i, mut j) = (0, 0);
    let mut raus = Vec::new();
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if lo <= hi {
            raus.push((lo, hi));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    raus
}

/// `falte` for a code point (astral characters stay as they are).
fn falte_cp(cp: u32) -> u32 {
    if cp <= 0xFFFF { falte(cp as u16) as u32 } else { cp }
}

/// The image of a set under `falte`: what the set looks like with every
/// member replaced by its case folding representative (`i` flag).
fn falte_bild(b: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut weg = Vec::new();
    let mut dazu = Vec::new();
    for &(k, vertreter) in FALTUNG {
        if k != vertreter && cp_enthaelt(b, k as u32) {
            weg.push((k as u32, k as u32));
            dazu.push((vertreter as u32, vertreter as u32));
        }
    }
    if weg.is_empty() {
        return b.to_vec();
    }
    let mut r = cp_schnitt(b, &cp_komplement(&cp_merzen(weg)));
    r.extend(dazu);
    cp_merzen(r)
}

/// The code point at `pos` and its length in units (a surrogate pair is one
/// code point in unicode mode).
fn cp_an(text: &[u16], pos: usize, unicode: bool) -> Option<(u32, usize)> {
    let t = *text.get(pos)?;
    if unicode
        && (0xD800..0xDC00).contains(&t)
        && let Some(&n) = text.get(pos + 1)
        && (0xDC00..0xE000).contains(&n)
    {
        return Some((0x10000 + ((t as u32 - 0xD800) << 10) + (n as u32 - 0xDC00), 2));
    }
    Some((t as u32, 1))
}

/// Code points as UTF-16 units.
fn cps_als_einheiten(cps: &[u32]) -> Vec<u16> {
    let mut raus = Vec::with_capacity(cps.len());
    for &cp in cps {
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            raus.push(0xD800 + (v >> 10) as u16);
            raus.push(0xDC00 + (v & 0x3FF) as u16);
        } else {
            raus.push(cp as u16);
        }
    }
    raus
}

/// WhiteSpace and LineTerminator (`\s`) as code point ranges.
const LEERRAUM_CP: &[(u32, u32)] = &[
    (0x09, 0x0D),
    (0x20, 0x20),
    (0xA0, 0xA0),
    (0x1680, 0x1680),
    (0x2000, 0x200A),
    (0x2028, 0x2029),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x3000, 0x3000),
    (0xFEFF, 0xFEFF),
];

impl Durchlauf<'_> {
    /// Does the text at `pos` start with `t` (case-insensitively with `i`)?
    fn text_gleich(&self, t: &[u16], pos: usize) -> bool {
        if pos + t.len() > self.text.len() {
            return false;
        }
        t.iter().enumerate().all(|(i, &u)| {
            let x = self.text[pos + i];
            if self.schalter.egal_gross { gleich_ungleich_case(x, u, true) } else { x == u }
        })
    }
}

impl Parser {
    /// Two named groups exclude each other when their paths part in the
    /// same disjunction (different alternatives).
    fn schliessen_sich_aus(a: &[(u32, u32)], b: &[(u32, u32)]) -> bool {
        for (x, y) in a.iter().zip(b.iter()) {
            if x != y {
                return x.0 == y.0;
            }
        }
        false
    }

    /// `(?ims-ims:…)` after `(?` (ES2025 pattern modifiers).
    fn modifikator(&mut self) -> Result<Knoten, String> {
        let mut an = 0u8;
        let mut aus = 0u8;
        let mut minus = false;
        loop {
            let Some(&c) = self.zeichen.get(self.pos) else {
                return Err("ungueltige Modifikatoren".to_string());
            };
            self.pos += 1;
            let bit = match c {
                'i' => 1u8,
                'm' => 2,
                's' => 4,
                '-' if !minus => {
                    minus = true;
                    continue;
                }
                ':' => break,
                _ => return Err("ungueltige Modifikatoren".to_string()),
            };
            // A flag may appear once in the whole modifier.
            if (an | aus) & bit != 0 {
                return Err("ungueltige Modifikatoren".to_string());
            }
            if minus {
                aus |= bit;
            } else {
                an |= bit;
            }
        }
        if minus && aus == 0 {
            return Err("ungueltige Modifikatoren".to_string());
        }
        if an == 0 && aus == 0 {
            return Err("ungueltige Modifikatoren".to_string());
        }
        let i_vorher = self.i_aktiv;
        if an & 1 != 0 {
            self.i_aktiv = true;
        }
        if aus & 1 != 0 {
            self.i_aktiv = false;
        }
        let k = self.alternation();
        self.i_aktiv = i_vorher;
        let k = k?;
        self.erwarte(')')?;
        Ok(Knoten::Modifikator { inner: Box::new(k), an, aus })
    }

    /// The class node of a code point set (folded for `i`).
    fn cp_knoten(&self, m: CpMenge, negiert: bool) -> Knoten {
        let (gefalten, faltung) = if self.i_aktiv {
            // In `v` mode the operands are folded already.
            (if self.v { m.bereiche.clone() } else { falte_bild(&m.bereiche) }, true)
        } else {
            (Vec::new(), false)
        };
        let mut texte: Vec<Vec<u32>> = m.texte.into_iter().filter(|t| t.len() >= 2).collect();
        texte.sort_by(|a, b| b.len().cmp(&a.len()));
        Knoten::KlasseCp(Box::new(CpKlasse {
            negiert,
            bereiche: m.bereiche,
            gefalten,
            faltung,
            texte: texte.iter().map(|t| cps_als_einheiten(t)).collect(),
            leer: false,
        }))
    }

    /// `[` in unicode mode.
    fn klasse_cp(&mut self) -> Result<Knoten, String> {
        let (m, negiert) = if self.v { self.v_klasse()? } else { self.u_klasse()? };
        let leer = m.texte.iter().any(|t| t.is_empty());
        let mut k = self.cp_knoten(m, negiert);
        if let Knoten::KlasseCp(c) = &mut k {
            c.leer = leer;
        }
        Ok(k)
    }

    /// `\p{…}` (the name after `{`) as code points: the BMP from the
    /// generated Unicode 17 table, surrogates and astral planes from
    /// `unicode_eigenschaften` (Unicode 15.1), whose ranges are code points.
    fn cp_eigenschaft(name: &str) -> Result<Vec<(u32, u32)>, String> {
        let alt = crate::unicode::eigenschaft_bereiche(name)
            .ok_or_else(|| format!("Unbekannte Unicode-Eigenschaft: {name}"))?;
        // The general category groups (`L`, `Letter`, `gc=P`, …) are unions
        // of their members there; `Assigned` is everything but `Cn`.
        let n = name.strip_prefix("General_Category=").or_else(|| name.strip_prefix("gc=")).unwrap_or(name);
        let gruppe: Option<&[&str]> = match n {
            "L" | "Letter" => Some(&["Lu", "Ll", "Lt", "Lm", "Lo"]),
            "LC" | "Cased_Letter" => Some(&["Lu", "Ll", "Lt"]),
            "M" | "Mark" | "Combining_Mark" => Some(&["Mn", "Mc", "Me"]),
            "N" | "Number" => Some(&["Nd", "Nl", "No"]),
            "P" | "Punctuation" | "punct" => Some(&["Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po"]),
            "S" | "Symbol" => Some(&["Sm", "Sc", "Sk", "So"]),
            "Z" | "Separator" => Some(&["Zs", "Zl", "Zp"]),
            "C" | "Other" => Some(&["Cc", "Cf", "Cs", "Co", "Cn"]),
            _ => None,
        };
        let neu: Option<Vec<(u32, u32)>> = if let Some(g) = gruppe {
            let mut v = Vec::new();
            for m in g {
                v.extend_from_slice(crate::eingebaut_unicode::bereich(m, None).unwrap_or(&[]));
            }
            Some(v)
        } else if name == "Assigned" {
            crate::eingebaut_unicode::bereich("Cn", None).map(|cn| cp_komplement(&cp_merzen(cn.to_vec())))
        } else {
            crate::eingebaut_unicode::bereich(name, None).map(|b| b.to_vec())
        };
        let Some(neu) = neu else {
            return Ok(cp_merzen(alt.iter().map(|&(a, b)| (a as u32, b as u32)).collect()));
        };
        let mut b: Vec<(u32, u32)> = Vec::with_capacity(alt.len() + neu.len());
        for &(lo, hi) in &alt {
            let (lo, hi) = (lo as u32, hi as u32);
            if hi < 0xD800 || lo > 0xDFFF {
                b.push((lo, hi));
            } else {
                if lo < 0xD800 {
                    b.push((lo, 0xD7FF));
                }
                if hi > 0xDFFF {
                    b.push((0xE000, hi));
                }
            }
        }
        b.extend(cp_schnitt(&cp_merzen(neu), &[(0xD800, 0xDFFF), (0x10000, MAX_CP_REGEX)]));
        Ok(cp_merzen(b))
    }

    /// The positive set of a class escape letter (`d w s p`, also for the
    /// upper case letters, whose complement the caller takes). `self.pos`
    /// stands after the letter.
    fn cp_abkuerzung_positiv(&mut self, e: char) -> Result<Vec<(u32, u32)>, String> {
        Ok(match e.to_ascii_lowercase() {
            'd' => vec![(0x30, 0x39)],
            'w' => {
                let mut w = vec![(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)];
                // ECMA-262 22.2.2.9.3 WordCharacters: with unicode and `i`,
                // the characters that canonicalize into it (ſ and K).
                if self.i_aktiv {
                    w.push((0x17F, 0x17F));
                    w.push((0x212A, 0x212A));
                }
                cp_merzen(w)
            }
            's' => LEERRAUM_CP.to_vec(),
            _ => {
                if self.zeichen.get(self.pos) != Some(&'{') {
                    return Err("\\p braucht eine Eigenschaft in { }".to_string());
                }
                self.pos += 1;
                let mut name = String::new();
                while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                    name.push(self.zeichen[self.pos]);
                    self.pos += 1;
                    if name.len() > 200 {
                        return Err("Eigenschaftsname zu lang".to_string());
                    }
                }
                if self.pos >= self.zeichen.len() {
                    return Err("\\p{ ohne }".to_string());
                }
                self.pos += 1; // }
                Self::cp_eigenschaft(&name)?
            }
        })
    }

    /// A class escape (`\d \D \w \W \s \S \p{} \P{}`) as code points.
    fn cp_abkuerzung(&mut self, e: char) -> Result<Vec<(u32, u32)>, String> {
        let mut b = self.cp_abkuerzung_positiv(e)?;
        // `v` mode with `i` folds the operand before complementing
        // (ECMA-262 22.2.2.9 MaybeSimpleCaseFolding).
        if self.v && self.i_aktiv {
            b = falte_bild(&b);
        }
        Ok(if e.is_ascii_uppercase() { cp_komplement(&b) } else { b })
    }

    /// `\u` escape (after `\u`): `{…}`, four digits, or a surrogate pair
    /// written as two escapes (one code point in unicode mode).
    fn u_escape_cp(&mut self) -> Result<u32, String> {
        if self.zeichen.get(self.pos) == Some(&'{') {
            self.pos += 1;
            let mut w: u32 = 0;
            let mut stellen = 0;
            while self.pos < self.zeichen.len() && self.zeichen[self.pos] != '}' {
                let d = self.zeichen[self.pos].to_digit(16).ok_or("ungueltiges \\u{}")?;
                w = w * 16 + d;
                stellen += 1;
                if stellen > 8 || w > MAX_CP_REGEX {
                    return Err("ungueltiges \\u{}".to_string());
                }
                self.pos += 1;
            }
            if stellen == 0 {
                return Err("leeres \\u{}".to_string());
            }
            self.erwarte('}')?;
            return Ok(w);
        }
        let hoch = self.hex(4)? as u32;
        if (0xD800..0xDC00).contains(&hoch)
            && self.zeichen.get(self.pos) == Some(&'\\')
            && self.zeichen.get(self.pos + 1) == Some(&'u')
        {
            let merk = self.pos;
            self.pos += 2;
            if let Ok(tief) = self.hex(4)
                && (0xDC00..0xE000).contains(&(tief as u32))
            {
                return Ok(0x10000 + ((hoch - 0xD800) << 10) + (tief as u32 - 0xDC00));
            }
            self.pos = merk;
        }
        Ok(hoch)
    }

    /// A class in `u` mode: ranges of code points.
    fn u_klasse(&mut self) -> Result<(CpMenge, bool), String> {
        self.pos += 1; // [
        let mut negiert = false;
        if self.zeichen.get(self.pos) == Some(&'^') {
            negiert = true;
            self.pos += 1;
        }
        let mut b: Vec<(u32, u32)> = Vec::new();
        while self.pos < self.zeichen.len() && self.zeichen[self.pos] != ']' {
            let (menge, einzel) = self.u_einheit()?;
            let bindestrich = self.zeichen.get(self.pos) == Some(&'-')
                && self.zeichen.get(self.pos + 1).is_some_and(|c| *c != ']');
            if bindestrich {
                let Some(lo) = einzel else {
                    return Err("eine Abkuerzung darf keine Bereichsgrenze sein".to_string());
                };
                self.pos += 1; // -
                let (_, hi) = self.u_einheit()?;
                let Some(hi) = hi else {
                    return Err("eine Abkuerzung darf keine Bereichsgrenze sein".to_string());
                };
                if hi < lo {
                    return Err(format!("Bereich ausserhalb der Reihenfolge: {lo}-{hi}"));
                }
                b.push((lo, hi));
            } else {
                b.extend(menge);
            }
            if b.len() > 100_000 {
                return Err("Klasse zu gross".to_string());
            }
        }
        self.erwarte(']')?;
        Ok((CpMenge::aus(cp_merzen(b)), negiert))
    }

    /// One ClassAtom in `u` mode: its set and, for a single character, the
    /// code point.
    fn u_einheit(&mut self) -> Result<(Vec<(u32, u32)>, Option<u32>), String> {
        let c = *self.zeichen.get(self.pos).ok_or("Klasse endet unerwartet")?;
        if c != '\\' {
            self.pos += 1;
            let cp = c as u32;
            return Ok((vec![(cp, cp)], Some(cp)));
        }
        let e = *self.zeichen.get(self.pos + 1).ok_or("Escape in Klasse am Ende")?;
        match e {
            'p' | 'P' | 'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                self.pos += 2;
                Ok((self.cp_abkuerzung(e)?, None))
            }
            'u' => {
                self.pos += 2;
                let cp = self.u_escape_cp()?;
                Ok((vec![(cp, cp)], Some(cp)))
            }
            _ => {
                let v = self.klassen_einheit()?;
                let einzel = (v.len() == 1 && v[0].0 == v[0].1).then(|| v[0].0 as u32);
                Ok((v.iter().map(|&(a, b)| (a as u32, b as u32)).collect(), einzel))
            }
        }
    }

    /// A class in `v` mode (at `[`): ClassSetExpression.
    fn v_klasse(&mut self) -> Result<(CpMenge, bool), String> {
        self.pos += 1; // [
        let mut negiert = false;
        if self.zeichen.get(self.pos) == Some(&'^') {
            negiert = true;
            self.pos += 1;
        }
        let m = self.v_inhalt()?;
        self.erwarte(']')?;
        if negiert && !m.texte.is_empty() {
            return Err("eine negierte Klasse darf keine Zeichenketten enthalten".to_string());
        }
        Ok((m, negiert))
    }

    fn v_ist(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(i, c)| self.zeichen.get(self.pos + i) == Some(&c))
    }

    fn v_ende(&self) -> bool {
        self.pos >= self.zeichen.len() || self.zeichen[self.pos] == ']'
    }

    /// ClassUnion, ClassIntersection or ClassSubtraction (not mixed).
    fn v_inhalt(&mut self) -> Result<CpMenge, String> {
        if self.v_ende() {
            return Ok(CpMenge::default());
        }
        let (erst, bereich) = self.v_operand(true)?;
        for (op, schneiden) in [("&&", true), ("--", false)] {
            if !self.v_ist(op) {
                continue;
            }
            if bereich {
                return Err("eine Mengenoperation braucht Operanden, keinen Bereich".to_string());
            }
            let mut acc = erst;
            while self.v_ist(op) {
                self.pos += 2;
                if schneiden && self.zeichen.get(self.pos) == Some(&'&') {
                    return Err("'&&&' in einer Klasse".to_string());
                }
                let (o, _) = self.v_operand(false)?;
                acc = if schneiden { acc.schneiden(o) } else { acc.abziehen(o) };
            }
            if !self.v_ende() {
                return Err("gemischte Mengenoperationen in einer Klasse".to_string());
            }
            return Ok(acc);
        }
        let mut acc = erst;
        while !self.v_ende() {
            if self.v_ist("&&") || self.v_ist("--") {
                return Err("gemischte Mengenoperationen in einer Klasse".to_string());
            }
            let (o, _) = self.v_operand(true)?;
            acc.vereinigen(o);
            if acc.bereiche.len() > 100_000 {
                return Err("Klasse zu gross".to_string());
            }
        }
        Ok(acc)
    }

    /// The operand set, folded in `v` mode with `i`.
    fn v_gefaltet(&self, b: Vec<(u32, u32)>) -> CpMenge {
        let b = cp_merzen(b);
        CpMenge::aus(if self.i_aktiv { falte_bild(&b) } else { b })
    }

    /// One ClassSetOperand, or with `bereich_ok` also a ClassSetRange; the
    /// flag says whether it was a range.
    fn v_operand(&mut self, bereich_ok: bool) -> Result<(CpMenge, bool), String> {
        let c = *self.zeichen.get(self.pos).ok_or("Klasse endet unerwartet")?;
        if c == '[' {
            let (m, negiert) = self.v_klasse()?;
            if negiert {
                return Ok((CpMenge::aus(cp_komplement(&m.bereiche)), false));
            }
            return Ok((m, false));
        }
        if c == '\\' {
            let e = self.zeichen.get(self.pos + 1).copied();
            match e {
                Some('q') => {
                    self.pos += 2;
                    return Ok((self.v_q()?, false));
                }
                Some(e @ ('d' | 'D' | 'w' | 'W' | 's' | 'S' | 'p' | 'P')) => {
                    self.pos += 2;
                    return Ok((CpMenge::aus(self.cp_abkuerzung(e)?), false));
                }
                _ => {}
            }
        }
        let lo = self.v_zeichen()?;
        if bereich_ok && self.v_ist("-") && !self.v_ist("--") {
            self.pos += 1;
            let hi = self.v_zeichen()?;
            if hi < lo {
                return Err(format!("Bereich ausserhalb der Reihenfolge: {lo}-{hi}"));
            }
            return Ok((self.v_gefaltet(vec![(lo, hi)]), true));
        }
        Ok((self.v_gefaltet(vec![(lo, lo)]), false))
    }

    /// `\q{a|bc|}` (after `\q`): ClassStringDisjunction.
    fn v_q(&mut self) -> Result<CpMenge, String> {
        self.erwarte('{')?;
        let mut m = CpMenge::default();
        let mut aktuell: Vec<u32> = Vec::new();
        loop {
            match self.zeichen.get(self.pos) {
                None => return Err("\\q{ ohne }".to_string()),
                Some('}') | Some('|') => {
                    let ende = self.zeichen[self.pos] == '}';
                    self.pos += 1;
                    let mut s = std::mem::take(&mut aktuell);
                    if self.i_aktiv {
                        s = s.into_iter().map(falte_cp).collect();
                    }
                    if s.len() == 1 {
                        m.bereiche.push((s[0], s[0]));
                    } else if !m.texte.contains(&s) {
                        m.texte.push(s);
                    }
                    if ende {
                        break;
                    }
                }
                Some(_) => {
                    aktuell.push(self.v_zeichen()?);
                    if aktuell.len() > 10_000 {
                        return Err("\\q-Zeichenkette zu lang".to_string());
                    }
                }
            }
        }
        m.bereiche = cp_merzen(m.bereiche);
        Ok(m)
    }

    /// ClassSetCharacter.
    fn v_zeichen(&mut self) -> Result<u32, String> {
        let c = *self.zeichen.get(self.pos).ok_or("Klasse endet unerwartet")?;
        if c == '\\' {
            let e = *self.zeichen.get(self.pos + 1).ok_or("Escape am Ende")?;
            self.pos += 2;
            return Ok(match e {
                'b' => 8,
                'n' => 10,
                'r' => 13,
                't' => 9,
                'f' => 12,
                'v' => 11,
                '0' if !self.zeichen.get(self.pos).is_some_and(|d| d.is_ascii_digit()) => 0,
                'c' => {
                    let Some(&b) = self.zeichen.get(self.pos).filter(|b| b.is_ascii_alphabetic()) else {
                        return Err("ungueltiges \\c".to_string());
                    };
                    self.pos += 1;
                    b as u32 % 32
                }
                'x' => self.hex(2)? as u32,
                'u' => self.u_escape_cp()?,
                '&' | '-' | '!' | '#' | '%' | ',' | ':' | ';' | '<' | '=' | '>' | '@' | '`' | '~' => e as u32,
                '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' => e as u32,
                _ => return Err(format!("ungueltiges Escape \\{e} in einer Klasse")),
            });
        }
        if matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '/' | '-' | '|') {
            return Err(format!("'{c}' muss in einer v-Klasse maskiert werden"));
        }
        if "&!#$%*+,.:;<=>?@^`~".contains(c) && self.zeichen.get(self.pos + 1) == Some(&c) {
            return Err(format!("'{c}{c}' ist in einer v-Klasse reserviert"));
        }
        self.pos += 1;
        Ok(c as u32)
    }
}
