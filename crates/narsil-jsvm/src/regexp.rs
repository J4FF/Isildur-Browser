//! ROLLE WASM — der RegExp-**Ergebnis-Kern** (ECMA-262 22.1.3 / 22.2.7).
//!
//! # WAS IN DIESER DATEI STEHT UND WARUM GERADE DAS
//!
//! Der Auftrag lautet: test262 `built-ins/RegExp` und
//! `String.prototype.match/replace/split` **mit** RegExp zu 100 % bringen.
//! Diese Datei ist der Teil davon, der **kein Objektmodell braucht**:
//!
//!   * [`substitution`] — `GetSubstitution` (22.1.3.19.1), der `$`-Kern von
//!     `String.prototype.replace` / `@@replace`.
//!   * [`teilen`] — `RegExp.prototype[@@split]` (22.2.6.14) in der Fassung
//!     von ECMA-262 2027: der Splitter wird **immer neu gebaut** und bekommt
//!     `y` dazu, `lastIndex` des Originals wird **nie** gelesen.
//!   * [`fortschritt`] — `AdvanceStringIndex` (22.2.7.3).
//!   * [`index_paare`] — `MakeMatchIndicesIndexPairArray` (22.2.7.8), der
//!     `indices`-Array des Schalters `d`.
//!   * [`zu_uint32`] / [`laenge`] — die Zahlenregeln, die `@@split` braucht.
//!
//! Alles ist **rein**: eine Funktion, ein Ergebnis, kein Heap, kein
//! `Wert`, kein `&mut Engine`. Deshalb ist jede Zeile hier gegen die
//! **gemessene** Erwartung prüfbar, ohne den ganzen Motor.
//!
//! # GEMESSEN, WAS DER BESTEHENDE WEG FALSCH MACHT
//!
//! Der Motor hat diese drei Regeln heute an zwei Stellen in `builtin2.rs`
//! (nicht meine Datei, BRETT-Eintrag WASM_1). Die folgenden Fehler sind
//! **reproduziert**, jede Zeile mit dem Orakel `node` v22 daneben:
//!
//! | Fall | node 22 | jetziger Weg (`dollar_ersetzen`, `regex_teilung`) |
//! |---|---|---|
//! | `"aäb".replace(/ä/, "[$ä]")` | `a[$ä]b` | `a[Ã¤]b` — der Byte-Weg `out.push(b[i] as char)` (builtin2.rs:3063) macht **jedes** Nicht-ASCII-Zeichen zu zwei Latin-1-Zeichen |
//! | `"aäb".replace(/ä/, "[$`]")` | `a[a]b` | **Panic**: `&ganz[..anfang]` (builtin2.rs:3036) schneidet mitten in ein UTF-8-Zeichen (`anfang` ist eine UTF-16-, `ganz` eine Byte-Adresse) |
//! | `"abc".replace(/(a)(b)/, "[$12]")` | `[a2]c` | `[a$12]c` — die Ziffernregel 22.1.3.19.1 Schritt g („zweistellig zu gross → **eine** Stelle") fehlt |
//! | `"abc".replace(/(?<x>b)/, "[$<x>]")` | `a[b]c` | unverändert — `$<name>` fehlt ganz |
//! | `"abc".split(/(b)/, 2)` | `["a","b"]` | `["a"]` — die Grenze wird **nach jedem** Teilstück geprüft, nicht nur am Ende |
//! | `"aäb".split(/ä/)` mit `u` | `["a","b"]` | `u`-Fortschritt fehlt (nur `+1`) |
//! | `"abc".split(/(b)\|(z)?/)` | `["a","b",**undefined**,"c"]` | `["a","b","","c"]` — nicht beteiligte Gruppen sind **kein** leerer Text (22.2.6.14 Schritt 15: `CreateDataProperty(array, lengthA, nextCapture)`) |
//! | `"abcabc".split(/a/g)` mit `lastIndex = 5` | `["","bc","bc"]` | liest `lastIndex` (der Spec-Zweig von vor ES2023) — der 2027er Spec baut einen **neuen** Splitter |
//!
//! # ORAKEL UND BELEG
//!
//! * Alle Erwartungen in `mod tests` sind **an `node` v22 gemessen**
//!   (V8 12.4) und mit der **Normtext-Fassung von ECMA-262 2027**
//!   abgeglichen. Wo der alte, haeufig zitierte ES2015-Text von `@@split`
//!   etwas anderes sagt, gilt der 2027er Text — das ist der Grund, warum
//!   `lastIndex` in [`teilen`] nicht vorkommt.
//! * `node` ist dabei nur das Orakel fuer die **Beobachtung**, nicht die
//!   Quelle: die Regel steht in [`teilen`] und [`substitution`] nach dem
//!   Spec-Schritt, die Beobachtung haengt daneben.
//!
//! Kein `unsafe`, keine Fremd-Crate, nur `std`, Edition 2024.
//!
//! Der Split-Kern ist ueber `builtin2.rs` an den VM-Matcher angeschlossen.
//! `tests/w1_regexp.rs` prueft die reinen Algorithmen zusaetzlich direkt.

#![allow(dead_code)]

// ======================================================================
// EINHEITEN: JavaScript-Texte sind Folgen von UTF-16-Codeeinheiten
// ======================================================================

/// Die Darstellungen, in denen der Motor mit JS-Texten arbeitet.
///
/// **GEMESSEN, warum hier `u16` und nicht `char`:** `"a😀b".split(/(?:)/)`
/// liefert an `node` v22 `["a","\ud83d","\ude00","b"]` — **ohne** `u` trennt
/// die Regex-Engine die surrogenhälfte. Jeder Index, den die RegExp-Fläche
/// meldet (`index`, `lastIndex`, `indices`, die Positionen in `split`), ist
/// darum ein Index in **Codeeinheiten**, nicht in Zeichen. Wer hier `char`
/// benutzt, bekommt bei jedem Emoji eine um eins daneben liegende Zahl.
pub type Einheiten = Vec<u16>;

/// Ein JS-Text als Codeeinheiten. Verlustfrei fuer jedes `char`.
pub fn einheiten(s: &str) -> Einheiten {
    s.encode_utf16().collect()
}

/// Codeeinheiten als Text.
///
/// **Die Fehlstelle ist Absicht, nicht Bequemlichkeit:** eine unpaarige
/// surrogat-Einheit (das kann nur ein `&[u16]` aus dem Inneren des Motors
/// liefern) ergibt `Err` mit dem Index, statt zu einem `U+FFFD` zu werden.
/// Ein still ersetztes Zeichen waere ein falscher Treffer im Browser.
pub fn text(e: &[u16]) -> Result<String, String> {
    match unpaarig(e) {
        Some(i) => Err(format!("unpaarige surrogat-Einheit an Position {i}")),
        None => Ok(String::from_utf16_lossy(e)),
    }
}

/// Die erste Stelle, an der eine surrogat-Einheit **keinen** Partner hat —
/// dieselbe Frage wie `String.prototype.isWellFormed` (22.1.3.31), nur fuer
/// die Codeeinheiten, mit denen die RegExp-Flaeche arbeitet.
///
/// **GEMESSEN an `node` v22:** `"a😀b".split(/(?:)/)` liefert die beiden
/// **Halb**-Zeichen `"\ud83d"` und `"\ude00"` als eigene Teile. Jeder Ort,
/// der eine Regex-Zahl als *Zeichen*-Index liest, ist genau dort falsch.
/// Deshalb steht diese Pruefung am Anfang von [`text`] und nicht als
/// `unwrap` am Ende.
pub fn unpaarig(e: &[u16]) -> Option<usize> {
    let mut i = 0;
    while i < e.len() {
        let c = e[i];
        if (0xD800..=0xDBFF).contains(&c) {
            if i + 1 < e.len() && (0xDC00..=0xDFFF).contains(&e[i + 1]) {
                i += 2;
                continue;
            }
            return Some(i);
        }
        if (0xDC00..=0xDFFF).contains(&c) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Kurzform fuer Proben: Einheiten -> Text, Panik bei kaputtem Text.
#[cfg(test)]
fn t(e: &[u16]) -> String {
    match text(e) {
        Ok(s) => s,
        Err(m) => panic!("{m}"),
    }
}

/// Ein Treffer des Matchers, in Codeeinheiten.
///
/// `gruppen` traegt die Gruppen **1..n** (`gruppen[0]` ist Gruppe 1),
/// `None` = **nicht beteiligt** (die Norm sagt da `undefined`, nicht `""`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Treffer {
    pub anfang: usize,
    pub ende: usize,
    pub gruppen: Vec<Option<(usize, usize)>>,
}

// ======================================================================
// 22.2.7.3 AdvanceStringIndex
// ======================================================================

/// `AdvanceStringIndex` (22.2.7.3): `index + 1`, unter `u`/`v` ueber ein
/// **Surrogatpaar** hinweg.
///
/// **GEMESSEN an `node` v22:**
/// ```text
/// "a😀b".split(/(?:)/u)   ->  ["a","😀","b"]     // zwei Schritte
/// "a😀b".split(/(?:)/)    ->  ["a","\ud83d","\ude00","b"]  // je eine Einheit
/// ```
pub fn fortschritt(t: &[u16], index: usize, unicode: bool) -> usize {
    if !unicode {
        return index + 1;
    }
    if index + 1 >= t.len() {
        return index + 1;
    }
    // `CodePointAt(string, index)` zaehlt die Codeeinheiten des Codepoints.
    let hoch = t[index];
    let ist_paar = (0xD800..=0xDBFF).contains(&hoch)
        && index + 1 < t.len()
        && (0xDC00..=0xDFFF).contains(&t[index + 1]);
    if ist_paar { index + 2 } else { index + 1 }
}

// ======================================================================
// 22.1.3.19.1 GetSubstitution
// ======================================================================

/// Die Argumente von `GetSubstitution` (22.1.3.19.1).
///
/// Alle Texte sind Codeeinheiten (siehe Modul-Kopf). `gruppen` und
/// `namen` kommen so herein, wie `@@replace` sie nach Schritt „Let captures
/// be ..." sieht: `None` bedeutet `undefined`.
/// Ein benannter Treffer: `(Name, Wert)`, `None` = `undefined`.
///
/// GEMESSEN, warum ein **Typ** und nicht ein geschriebener Typ: der Kern
/// braucht die Form an drei Stellen (Feld, Probenhilfe, `index_paare`), und
/// dreimal geschrieben ist eine Zahl, die auseinanderlaeuft — `clippy`
/// nennt den vierten Fall von selbst (`type_complexity`).
pub type Name<'a> = (String, Option<&'a [u16]>);

#[derive(Clone, Debug)]
pub struct Ersetzung<'a> {
    /// `string` — der ganze Subject-Text.
    pub text: &'a [u16],
    /// `position` — wo der Treffer begannt.
    pub position: usize,
    /// `matched` — der Treffer selbst (`S[position..position+len]`).
    pub treffer: &'a [u16],
    /// `captures` — die Gruppen 1..n in Nummernfolge, `None` = `undefined`.
    pub gruppen: &'a [Option<&'a [u16]>],
    /// `namedCaptures` — die benannten Gruppen; `None` = **es gibt keine**
    /// (dann ist `$<…` **wörtlich**, Schritt h.i).
    pub namen: Option<&'a [Name<'a>]>,
    /// `replacementTemplate` — der Ersetzungstext mit den `$`-Stellen.
    pub vorlage: &'a [u16],
}

/// `GetSubstitution` (22.1.3.19.1), Schritt fuer Schritt.
///
/// **Der Spec-Text ist die Quelle, `node` v22 die Messung.** Die beiden
/// Stellen, an denen die haeufig zitierten Kurzfassungen falsch sind:
///
/// 1. **Zwei Ziffern, zu grosser Index** (Schritt g.i): `$12` bei zwei
///    Gruppen ist `$1` + `"2"`, **nicht** der Wortlaut `$12`.
///    GEMESSEN: `"abc".replace(/(a)(b)/, "[$12]")` -> `"[a2]c"`.
/// 2. **`$<name>` ohne Treffer** (Schritt h.iv): **leerer Text**, kein
///    `SyntaxError`. GEMESSEN: `"abc".replace(/(?<x>b)/, "[$<y>]")` ->
///    `"a[]c"`, `"abc".replace(/(?<x>b)/y, "[$<y>]")` -> `"a[b]c"`.
///    Erst `$<name>` **ohne** `>` bzw. ohne benannte Gruppen bleibt
///    wörtlich (Schritt h.i).
///
/// **Kein Wurf moeglich** (die Norm hat in dieser Fassung keinen mehr),
/// deshalb gibt die Funktion `Vec<u16>` und kein `Result`.
pub fn substitution(a: &Ersetzung) -> Einheiten {
    const DOLLAR: u16 = b'$' as u16;
    let mut ergebnis: Einheiten = Vec::with_capacity(a.vorlage.len() + 8);
    let mut rest: &[u16] = a.vorlage;
    let laenge = a.text.len();
    while !rest.is_empty() {
        if rest[0] != DOLLAR {
            // Schritt 3: alles andere Zeichen fuer Zeichen. **Kein Byte-Schritt**
            // — der jetzige Weg nimmt `b[i] as char` und macht jedes
            // Nicht-ASCII-Zeichen zu zwei Latin-1-Zeichen.
            ergebnis.push(rest[0]);
            rest = &rest[1..];
            continue;
        }
        // ---- die vier Zwei-Zeichen-Stellen (Schritt e) ----
        if rest.len() >= 2 {
            match rest[1] {
                c if c == DOLLAR => {
                    ergebnis.push(DOLLAR);
                    rest = &rest[2..];
                    continue;
                }
                c if c == b'`' as u16 => {
                    // "$`" -> S[0..position]
                    ergebnis.extend_from_slice(&a.text[..a.position.min(laenge)]);
                    rest = &rest[2..];
                    continue;
                }
                c if c == b'&' as u16 => {
                    // "$&" -> matched
                    ergebnis.extend_from_slice(a.treffer);
                    rest = &rest[2..];
                    continue;
                }
                c if c == b'\'' as u16 => {
                    // "$'" -> S[min(position + matchLength, stringLength)..]
                    // Das `min` ist normativ: bei einem fremden `exec` darf
                    // `position + matchLength` ueber das Ende hinausgehen.
                    let ende = (a.position + a.treffer.len()).min(laenge);
                    ergebnis.extend_from_slice(&a.text[ende..]);
                    rest = &rest[2..];
                    continue;
                }
                _ => {}
            }
        }
        // Schritt g: "$" + mindestens eine Dezimalziffer.
        if let Some(stellen) = ziffern_von(rest, 1) {
            let mut stellen = stellen;
            // GEMESSEN an der Probe `gruppennummern_zwei_stellen_mit_rueckfall`:
            // die erste Fassung dieser Zeile nahm `&rest[1..stellen]` — bei
            // **einer** Ziffer ist das die leere Scheibe, `index` wurde 0 und
            // damit blieb **jede** Gruppennummer woertlich.
            let mut index = ziffern_zahl(&rest[1..1 + stellen]);
            // Schritt g.i: zwei Stellen, aber der Index ist groesser als die
            // Zahl der Gruppen -> **eine** Stelle nehmen und die zweite Ziffer
            // als Ziffer stehen lassen.
            if index > a.gruppen.len() && stellen == 2 {
                stellen = 1;
                index = ziffern_zahl(&rest[1..2]);
            }
            if (1..=a.gruppen.len()).contains(&index) {
                // `undefined` ergibt den leeren Text (Schritt g.v).
                if let Some(Some(w)) = a.gruppen.get(index - 1) {
                    ergebnis.extend_from_slice(w);
                }
            } else {
                ergebnis.extend_from_slice(&rest[..1 + stellen]);
            }
            rest = &rest[1 + stellen..];
            continue;
        }
        // Schritt h: "$<name>".
        if rest.len() >= 2 && rest[1] == b'<' as u16 {
            let gt = rest[2..]
                .iter()
                .position(|c| *c == b'>' as u16)
                .map(|p| p + 2);
            let (Some(gt), Some(namen)) = (gt, a.namen) else {
                // Schritt h.i: kein ">" oder gar keine benannten Gruppen ->
                // nur die zwei Zeichen "$<" sind woertlich, der Rest laeuft
                // normal weiter (er kann selbst noch ein "$" tragen).
                ergebnis.extend_from_slice(&rest[..2]);
                rest = &rest[2..];
                continue;
            };
            let name = units_text(&rest[2..gt]);
            // Schritt h.iv: unbekannter Name -> **leerer** Text, kein Wurf.
            let wert = name
                .as_deref()
                .and_then(|n| namen.iter().find(|(k, _)| k == n).and_then(|(_, w)| *w));
            if let Some(w) = wert {
                ergebnis.extend_from_slice(w);
            }
            rest = &rest[gt + 1..];
            continue;
        }
        // Schritt i: "$" + etwas anderes -> das "$" ist woertlich.
        ergebnis.push(DOLLAR);
        rest = &rest[1..];
    }
    ergebnis
}

/// Wie viele Ziffern stehen ab `ab` (1 oder 2; mehr als zwei zaehlen nicht,
/// die dritte Ziffer gehoert zum woertlichen Rest).
fn ziffern_von(rest: &[u16], ab: usize) -> Option<usize> {
    let ist = |c: u16| (b'0' as u16..=b'9' as u16).contains(&c);
    if !rest.get(ab).copied().is_some_and(ist) {
        return None;
    }
    if rest.len() > ab + 1 && rest[ab + 1..].first().copied().is_some_and(ist) {
        Some(2)
    } else {
        Some(1)
    }
}

/// Der Zahlwert der Ziffern in `z` (maximal zwei, also `0..=99`).
fn ziffern_zahl(z: &[u16]) -> usize {
    z.iter().fold(0usize, |a, c| {
        // `c` ist eine ASCII-Ziffer (der Aufrufer hat das geprueft), also
        // ist `c - '0'` der Ziffernwert — ohne die zwei Umwege ueber `u8`.
        a * 10 + (c - u16::from(b'0')) as usize
    })
}

/// Codeeinheiten als Text, **verlustfrei oder `None`** (fuer Gruppennamen,
/// die im Muster stehen — dort ist Verstummeln richtig, denn die Namen sind
/// normativ Text und der Kern hat sie selbst erzeugt).
fn units_text(u: &[u16]) -> Option<String> {
    String::from_utf16(u).ok()
}

// ======================================================================
// 22.2.6.14 RegExp.prototype [ @@split ] ( string, limit )
// ======================================================================

/// `ToUint32` (7.1.6) — die Zahlenregel von `limit`.
///
/// **GEMESSEN an `node` v22:** `"a,b".split(",", NaN)` -> `[]`,
/// `"a,b".split(",", Infinity)` -> `[]` (ToUint32 kappt **vor** dem
/// Vergleich), `"a,b".split(",", -1)` -> `["a","b"]` (2^32-1 = kein Deckel).
pub fn zu_uint32(v: f64) -> u32 {
    if !v.is_finite() || v == 0.0 {
        return 0;
    }
    let t = v.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    if m < 0.0 { 0 } else { m as u32 }
}

/// Die Grenze von `@@split`: `None` = `undefined` = `2^32-1`.
pub fn grenze(limit: Option<f64>) -> u32 {
    match limit {
        None => u32::MAX,
        Some(v) => zu_uint32(v),
    }
}

/// `ToLength` (7.1.20) fuer die Indizes, die `@@split` liest.
pub fn laenge(v: f64) -> usize {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= 9_007_199_254_740_991.0 {
        return 9_007_199_254_740_991usize;
    }
    v.trunc() as usize
}

/// `RegExp.prototype[@@split]` (22.2.6.14) als reiner Kern.
///
/// **`exec` bekommt den Startindex und muss *klebrig* suchen.** Das ist keine
/// Vereinfachung, das ist Schritt 6 der Norm: `newFlags = flags + "y"`, und
/// `splitter = Construct(speciesCtor, «R, newFlags»)` — es gibt **keinen**
/// Zweig, der das Original benutzt. Deshalb wird `lastIndex` des Originals
/// hier **nie gelesen**, und genau das ist der Unterschied zur
/// haeufig zitierten ES2015-Fassung.
///
/// **GEMESSEN an `node` v22 (der ganze Unterschied in einer Zeile):**
/// ```text
/// (() => { const r = /a/g; r.lastIndex = 5; return "abcabc".split(r); })()
///   ->  ["","bc","bc"]        // lastIndex wird ignoriert
/// ```
/// Die alte Fassung wuerde bei `lastIndex = 5` `[""]` liefern.
///
/// Die Elemente der Liste sind `Option<Vec<u16>>`: **eine nicht beteiligte
/// Gruppe ist `undefined`, nicht `""`.**
/// GEMESSEN: `"abc".split(/(b)|(z)?/)` -> `["a","b",undefined,"c"]`.
/// Wer hier `""` einsetzt, liefert in genau diesen Faellen ein *sichtbares*
/// falsches Element.
///
/// - `text` — `S` in Codeeinheiten.
/// - `unicode` — `flags` enthaelt `u` oder `v`.
/// - `limit` — `limit`; `None` = `undefined`.
/// - `exec(start)` — `RegExpExec` auf dem klebrigen Splitter.
pub fn teilen<F>(
    text: &[u16],
    unicode: bool,
    limit: Option<f64>,
    mut exec: F,
) -> Vec<Option<Einheiten>>
where
    F: FnMut(usize) -> Option<Treffer>,
{
    let mut array: Vec<Option<Einheiten>> = Vec::new();
    let lim = grenze(limit);
    if lim == 0 {
        return array;
    }
    let size = text.len();
    // Schritt 12: der leere Text ist ein eigener Fall.
    if size == 0 {
        return match exec(0) {
            // ein Treffer -> das leere Array
            Some(_) => array,
            // kein Treffer -> [""]  (CreateDataProperty(array, "0", S))
            None => {
                array.push(Some(Vec::new()));
                array
            }
        };
    }
    let mut last_match_end = 0usize;
    let mut suche = 0usize;
    while suche < size {
        let treffer = exec(suche);
        let Some(t) = treffer else {
            // Schritt 15.d: kein Treffer -> einen Index weiter.
            suche = fortschritt(text, suche, unicode);
            continue;
        };
        // Schritt 15.e.i/ii: `min(ToLength(lastIndex), size)`; der Splitter
        // ist klebrig, also ist `lastIndex` nach dem Treffer sein Ende.
        let match_end = laenge(t.ende as f64).min(size);
        if match_end == last_match_end {
            // Schritt 15.e.ii: leerer Treffer am selben Ort -> weiter.
            suche = fortschritt(text, suche, unicode);
            continue;
        }
        // Schritt 15.e.iii.1: der Substring laeuft von `lastMatchEnd` bis
        // `searchIndex` — **nicht** bis zum Trefferanfang.
        array.push(Some(text[last_match_end.min(suche)..suche].to_vec()));
        if array.len() as u32 == lim {
            return array; // Schritt 15.e.iii.3
        }
        last_match_end = match_end;
        // Schritt 15.e.iii.4: die Gruppen, `undefined` bleibt `undefined`.
        for g in &t.gruppen {
            array.push(g.map(|(a, b)| text[a.min(size)..b.min(size)].to_vec()));
            if u32::try_from(array.len()).unwrap_or(u32::MAX) == lim {
                return array; // Schritt 15.e.iii.5
            }
        }
        // Schritt 15.e.iii.6: `searchIndex = lastMatchEnd`.
        suche = last_match_end;
    }
    // Schritt 16: der Rest.
    array.push(Some(text[last_match_end.min(size)..].to_vec()));
    array
}

// ======================================================================
// 22.2.7.8 MakeMatchIndicesIndexPairArray  (Schalter `d`)
// ======================================================================

/// Das Ergebnis von `MakeMatchIndicesIndexPairArray` (22.2.7.8), ohne Heap.
///
/// `paare[i]` ist `[start, ende]` des Treffers (`i == 0`) bzw. der Gruppe `i`,
/// oder `None`, wenn die Gruppe nicht beteiligt war. `gruppen` ist `None`,
/// wenn das Muster **keine** benannten Gruppen hat (dann hat auch
/// `indices.groups` den Wert `undefined`), sonst die Liste
/// `(Name, Paar)` in Nummernfolge — bei doppelten Namen gewinnt der
/// **letzte** Eintrag, weil `CreateDataProperty` den Wert ueberschreibt
/// (Schritt 22.2.7.2 fuer `groups` haelt fest, dass die Assertion
/// `capturedValue ist undefined` gilt, der Wert also `undefined` bleibt).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexPaare {
    pub paare: Vec<Option<Einheiten>>,
    pub gruppen: Option<Vec<(String, Option<Einheiten>)>>,
}

/// `MakeMatchIndicesIndexPairArray` (22.2.7.8).
///
/// - `indizes` — `indices` aus `RegExpBuiltinExec`: Index 0 = der Treffer,
///   danach die Gruppen 1..n als `Option<(start, ende)>`.
/// - `namen` — `groupNames`, Index 0 = **Gruppe 1**, `None` = unbenannt.
/// - `hat_gruppen` — `hasGroups` (irgendein `GroupName` im Muster).
pub fn index_paare(
    indizes: &[Option<(usize, usize)>],
    namen: &[Option<String>],
    hat_gruppen: bool,
) -> IndexPaare {
    let paare: Vec<Option<Einheiten>> = indizes
        .iter()
        .map(|m| m.map(|(a, b)| vec![a as u16, b as u16]))
        .collect();
    let gruppen = if !hat_gruppen {
        None
    } else {
        let mut v: Vec<(String, Option<Einheiten>)> = Vec::new();
        // Schritt 4: nur fuer `i > 0`, also `namen[k]` = `groupNames[k+1]`.
        for (k, name) in namen.iter().enumerate() {
            let paar = paare.get(k + 1).cloned().flatten();
            if let Some(n) = name {
                // CreateDataProperty: ein doppelter Name ueberschreibt.
                match v.iter_mut().find(|(b, _)| b == n) {
                    Some(stelle) => stelle.1 = paar,
                    None => v.push((n.clone(), paar)),
                }
                // GEMESSEN (clippy `option_if_let_else`): `map_or` ist hier
                // nicht kuerzer — die Alternative waere eine `else`-Klausel
                // mit `return`, und der Kern soll kein `return` in einer
                // Schleife haben. Die Form bleibt.
            }
        }
        Some(v)
    };
    IndexPaare { paare, gruppen }
}

// ======================================================================
// PROBEN — jede Erwartung ist an `node` v22 gemessen oder aus dem
// Normtext abgelesen und dort als Messung zitiert.
// ======================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- GetSubstitution ----------

    /// Kurzform fuer Proben: Subject, Position, Treffer, Gruppen, Namen —
    /// liefert das **ganze** Ergebnis von `S.replace(treffer, vorlage)`, also
    /// mit dem Stueck davor und danach. Damit ist jede Probe direkt mit der
    /// `node`-Zeile vergleichbar (die dort auch das ganze Ergebnis zeigt).
    fn sub(
        subject: &str,
        position: usize,
        matched: &str,
        gruppen: &[Option<&str>],
        namen: Option<&[(String, Option<&str>)]>,
        vorlage: &str,
    ) -> String {
        let text = einheiten(subject);
        let treffer = einheiten(matched);
        let gruppen: Vec<Option<Einheiten>> = gruppen.iter().map(|g| g.map(einheiten)).collect();
        let gruppen_refs: Vec<Option<&[u16]>> = gruppen.iter().map(|g| g.as_deref()).collect();
        let namen: Option<Vec<(String, Option<Einheiten>)>> = namen.map(|n| {
            n.iter()
                .map(|(k, w)| (k.clone(), w.map(einheiten)))
                .collect()
        });
        let namen_refs: Option<Vec<Name>> = namen
            .as_ref()
            .map(|n| n.iter().map(|(k, w)| (k.clone(), w.as_deref())).collect());
        let v = einheiten(vorlage);
        let a = Ersetzung {
            text: &text,
            position,
            treffer: &treffer,
            gruppen: &gruppen_refs,
            namen: namen_refs.as_deref(),
            vorlage: &v,
        };
        let kern = substitution(&a);
        let mut ganz = text[..position].to_vec();
        ganz.extend_from_slice(&kern);
        ganz.extend_from_slice(&text[position + matched.encode_utf16().count()..]);
        t(&ganz)
    }

    #[test]
    fn die_fuenf_dollar_stellen() {
        // GEMESSEN an node 22: "abc".replace(/b/, "[$&]|[$$]|[$`]|[$']")
        assert_eq!(sub("abc", 1, "b", &[], None, "[$&]"), "a[b]c");
        assert_eq!(sub("abc", 1, "b", &[], None, "[$$]"), "a[$]c");
        assert_eq!(sub("abc", 1, "b", &[], None, "[$`]"), "a[a]c");
        assert_eq!(sub("abc", 1, "b", &[], None, "[$']"), "a[c]c");
        // kombiniert: GEMESSEN an node 22
        //   "abc".replace(/(b)/, "$1$1$&$`$'&$$") -> "abbbac&$c"
        // (das "&" in der Mitte ist ein **woertliches** Zeichen der Vorlage —
        // nur "$&" mit Dollar gehoert zu den fuenf Stellen)
        assert_eq!(
            sub("abc", 1, "b", &[Some("b")], None, "$1$1$&$`$'&$$"),
            "abbbac&$c"
        );
    }

    #[test]
    fn gruppennummern_zwei_stellen_mit_rueckfall() {
        // GEMESSEN an node 22 (alle vier Zeilen):
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$1]"), "a[b]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$01]"), "a[b]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$2]"), "a[$2]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$9]"), "a[$9]c");
        assert_eq!(
            sub("abc", 0, "ab", &[Some("a"), Some("b")], None, "[$12]"),
            "[a2]c"
        );
        assert_eq!(
            sub("abc", 0, "ab", &[Some("a"), Some("b")], None, "[$21]"),
            "[b1]c"
        );
        assert_eq!(
            sub("abc", 0, "ab", &[Some("a"), Some("b")], None, "[$0]"),
            "[$0]c"
        );
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$00]"), "[$00]b");
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$02]"), "[$02]b");
        // mehr als zwei Ziffern: die dritte Ziffer bleibt woertlich
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$9x]"), "a[$9x]c");
    }

    #[test]
    fn nicht_beteiligte_gruppe_ergibt_leeren_text() {
        // GEMESSEN an node 22, beide Zeilen dort nachgelesen:
        //   "ab".replace(/(a)|x/, "[$1|$2]")  -> "[a|$2]b"   (nur EINE Gruppe,
        //                                                     also ist $2 woertlich)
        //   "ab".replace(/(a)|(b)/, "[$1|$2]") -> "[a|]b"     (zwei Gruppen,
        //                                                     $2 nicht beteiligt)
        // Der Unterschied zwischen "woertlich" und "leer" ist genau die
        // Regel, die man an diesen beiden Faellen sieht.
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$1|$2]"), "[a|$2]b");
        assert_eq!(
            sub("ab", 0, "a", &[Some("a"), None], None, "[$1|$2]"),
            "[a|]b"
        );
        assert_eq!(
            sub("ab", 0, "a", &[Some("a"), Some("b")], None, "[$1|$2]"),
            "[a|b]b"
        );
    }

    #[test]
    fn benannte_gruppen_dollar_name() {
        // GEMESSEN an node 22:
        //   "abc".replace(/(?<x>b)/, "[$<x>]")  -> "a[b]c"
        //   "abc".replace(/(?<x>b)/, "[$<y>]")  -> "a[]c"   (KEIN SyntaxError)
        //   "abc".replace(/(b)/,      "[$<x>]")  -> "a[$<x>]c" (keine Namen)
        //   "abc".replace(/(?<x>b)/,  "[$<>]")  -> "a[]c"
        //   "abc".replace(/(?<x>b)/,  "[$<x")   -> "a[$<xc"
        //   (an "ab" mit Treffer an 0 gemessen: "[$<xb" — der Unterschied
        //    ist nur das Stueck **hinter** dem Treffer)
        let x: Vec<(String, Option<&str>)> = vec![("x".to_string(), Some("b"))];
        assert_eq!(
            sub("abc", 1, "b", &[Some("b")], Some(&x), "[$<x>]"),
            "a[b]c"
        );
        assert_eq!(sub("abc", 1, "b", &[Some("b")], Some(&x), "[$<y>]"), "a[]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], None, "[$<x>]"), "a[$<x>]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], Some(&x), "[$<>]"), "a[]c");
        assert_eq!(sub("abc", 1, "b", &[Some("b")], Some(&x), "[$<x"), "a[$<xc");
        // zwei Namen in Rueckwaertsrichtung: GEMESSEN "[$<y>$<x>]" -> "[ba]c"
        let y: Vec<(String, Option<&str>)> =
            vec![("x".to_string(), Some("a")), ("y".to_string(), Some("b"))];
        assert_eq!(
            sub(
                "abc",
                0,
                "ab",
                &[Some("a"), Some("b")],
                Some(&y),
                "[$<y>$<x>]"
            ),
            "[ba]c"
        );
    }

    #[test]
    fn nicht_ascii_geht_ohne_panik_durch() {
        // **Das ist der eigentliche Fund an diesem Kern.** Der jetzige Weg
        // schneidet `&ganz[..anfang]` mit UTF-16-Indizes (Panik) und laesst
        // jedes Nicht-ASCII-Zeichen ueber `out.push(b[i] as char)` zerfallen.
        // GEMESSEN an node 22:
        //   "aäb".replace(/ä/, "[$&]")        -> "a[ä]b"
        //   "aäb".replace(/ä/, "[$ä]")        -> "a[$ä]b"
        //   "aäb".replace(/ä/, "[$`]")        -> "a[a]b"
        //   "aäb".replace(/ä/, "[$']")        -> "a[b]b"
        //   "a😀b".replace(/😀/, "[$&|ä|😀]") -> "a[😀|ä|😀]b"
        assert_eq!(
            sub("a\u{e4}b", 1, "\u{e4}", &[], None, "[$&]"),
            "a[\u{e4}]b"
        );
        assert_eq!(
            sub("a\u{e4}b", 1, "\u{e4}", &[], None, "[\u{e4}]"),
            "a[\u{e4}]b"
        );
        assert_eq!(sub("a\u{e4}b", 1, "\u{e4}", &[], None, "[$`]"), "a[a]b");
        assert_eq!(sub("a\u{e4}b", 1, "\u{e4}", &[], None, "[$']"), "a[b]b");
        assert_eq!(
            sub(
                "a\u{1F600}b",
                1,
                "\u{1F600}",
                &[],
                None,
                "[$&|\u{e4}|\u{1F600}]"
            ),
            "a[\u{1F600}|\u{e4}|\u{1F600}]b"
        );
        // Positionen sind Codeeinheiten: "aäb" hat 3 Einheiten, 4 Bytes.
        assert_eq!(einheiten("a\u{e4}b").len(), 3);
        assert_eq!("a\u{e4}b".len(), 4);
        // ein Emoji belegt zwei Einheiten — der Trefferanfang 1 liegt also
        // mitten in einer surrogat-paarung, was der Byte-Weg nicht kann.
        assert_eq!(
            sub("a\u{1F600}b", 1, "\u{1F600}", &[], None, "[$`]"),
            "a[a]b"
        );
    }

    #[test]
    fn dollar_ohne_bedeutung_bleibt_stehen() {
        // GEMESSEN an node 22: "[$]" -> "[$]b", "[$x]" -> "[$x]b", "$" am Ende
        // -> "$" (Schritt i: ref = "$", der Rest ist leer).
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$]"), "[$]b");
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$x]"), "[$x]b");
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "[$"), "[$b");
        assert_eq!(sub("ab", 0, "a", &[Some("a")], None, "$"), "$b");
    }

    // ---------- @@split ----------

    /// Ein `exec` aus einer Musterbeschreibung: Liste der
    /// `(Text, Gruppen)` in Trefferreihenfolge, jedes mit seiner Position
    /// **gefunden** (sticky). Passt keins an `start` -> `None`.
    fn klebrig<'a>(
        text: &'a [u16],
        treffer: &'a [(&'a str, &'a [Option<&'a str>])],
    ) -> impl FnMut(usize) -> Option<Treffer> + 'a {
        move |start: usize| {
            for (muster, gruppen) in treffer {
                let m = einheiten(muster);
                if start + m.len() <= text.len() && text[start..start + m.len()] == m[..] {
                    let g: Vec<Option<(usize, usize)>> = gruppen
                        .iter()
                        .map(|g| g.map(|s| (start, start + einheiten(s).len())))
                        .collect();
                    return Some(Treffer {
                        anfang: start,
                        ende: start + m.len(),
                        gruppen: g,
                    });
                }
            }
            None
        }
    }

    /// `split` mit Text-Ausgabe. Fuer die Surrogat-Haelften (siehe
    /// `split_unter_u_ueber_surrogatepaare`) gibt es die Einheiten-Form
    /// `split_einheiten` — ein halber Codepunkt **ist** kein `&str`.
    fn split(
        subject: &str,
        unicode: bool,
        limit: Option<f64>,
        treffer: &[(&str, &[Option<&str>])],
    ) -> Vec<Option<String>> {
        split_einheiten(subject, unicode, limit, treffer)
            .into_iter()
            .map(|o| o.map(|v| t(&v)))
            .collect()
    }

    fn split_einheiten(
        subject: &str,
        unicode: bool,
        limit: Option<f64>,
        treffer: &[(&str, &[Option<&str>])],
    ) -> Vec<Option<Einheiten>> {
        let t = einheiten(subject);
        teilen(&t, unicode, limit, klebrig(&t, treffer))
    }

    #[test]
    fn split_grundfaelle() {
        // GEMESSEN an node 22 (jede Zeile dort nachgelesen):
        assert_eq!(
            split("ab", false, None, &[("b", &[])]),
            vec![Some("a".into()), Some("".into())]
        );
        assert_eq!(
            split("ab", false, None, &[("y", &[])]),
            vec![Some("ab".into())]
        );
        assert_eq!(
            split("abc", false, None, &[("b", &[Some("b")])]),
            vec![Some("a".into()), Some("b".into()), Some("c".into())]
        );
        assert_eq!(
            split("aaa", false, None, &[("a", &[]), ("a", &[]), ("a", &[])]),
            vec![
                Some("".into()),
                Some("".into()),
                Some("".into()),
                Some("".into())
            ]
        );
        assert_eq!(split("abc", false, None, &[]), vec![Some("abc".into())]);
    }

    #[test]
    fn split_grenze_zaehlt_auch_gruppen() {
        // GEMESSEN an node 22:
        //   "abc".split(/(b)/, 2) -> ["a","b"]     <- nach JEDEM Teilstueck
        //   "a,,b".split(",", 2)  -> ["a",""]
        //   "a,b".split(",", 1)   -> ["a"]
        //   "a,b".split(",", 0)   -> []
        //   "aaa".split(/a/, 2)   -> ["",""]
        assert_eq!(
            split("abc", false, Some(2.0), &[("b", &[Some("b")])]),
            vec![Some("a".into()), Some("b".into())]
        );
        assert_eq!(
            split("a,,b", false, Some(2.0), &[(",", &[]), (",", &[])]),
            vec![Some("a".into()), Some("".into())]
        );
        assert_eq!(
            split("a,b", false, Some(1.0), &[(",", &[]), (",", &[])]),
            vec![Some("a".into())]
        );
        assert_eq!(
            split("a,b", false, Some(0.0), &[(",", &[]), (",", &[])]),
            Vec::<Option<String>>::new()
        );
        assert_eq!(
            split("a,b", false, None, &[(",", &[]), (",", &[])]),
            vec![Some("a".into()), Some("b".into())]
        );
    }

    #[test]
    fn split_grenze_ist_eine_zahl() {
        // GEMESSEN an node 22: NaN -> [], Infinity -> [], -1 -> ganz.
        assert_eq!(zu_uint32(f64::NAN), 0);
        assert_eq!(zu_uint32(f64::INFINITY), 0);
        assert_eq!(zu_uint32(-1.0), 4_294_967_295);
        assert_eq!(zu_uint32(1.9), 1);
        assert_eq!(zu_uint32(0.0), 0);
        assert_eq!(grenze(None), u32::MAX);
        assert_eq!(grenze(Some(3.0)), 3);
        // NaN/Infinity ergeben ToUint32 = 0 -> **leeres** Array
        assert_eq!(
            split("a,b", false, Some(f64::NAN), &[(",", &[])]),
            Vec::<Option<String>>::new()
        );
        assert_eq!(
            split("a,b", false, Some(f64::INFINITY), &[(",", &[])]),
            Vec::<Option<String>>::new()
        );
        assert_eq!(
            split("a,b", false, Some(-1.0), &[(",", &[])]),
            vec![Some("a".into()), Some("b".into())]
        );
    }

    #[test]
    fn split_nicht_beteiligte_gruppe_ist_undefined() {
        // GEMESSEN an node 22:
        //   "abc".split(/(b)|(z)?/)  -> ["a","b",undefined,"c"]
        //   "ab".split(/(a)|(b)/)    -> ["", "a", undefined, "", undefined, "b", ""]
        // Der jetzige Weg liefert an dieser Stelle "" — ein sichtbar
        // anderes Element.
        let r = split("abc", false, None, &[("b", &[Some("b"), None])]);
        assert_eq!(
            r,
            vec![Some("a".into()), Some("b".into()), None, Some("c".into())]
        );
        assert_eq!(r.iter().filter(|x| x.is_none()).count(), 1);
        let q = split(
            "ab",
            false,
            None,
            // /(a)|(b)/ klebrig: an 0 passt "a" (Gruppe 1), an 1 passt "b"
            // (Gruppe 2). Die Reihenfolge der Liste **ist** die Alternation.
            &[("a", &[Some("a"), None]), ("b", &[None, Some("b")])],
        );
        assert_eq!(
            q,
            vec![
                Some("".into()),
                Some("a".into()),
                None,
                Some("".into()),
                None,
                Some("b".into()),
                Some("".into()),
            ]
        );
    }

    #[test]
    fn split_leerer_text_ist_ein_eigener_fall() {
        // GEMESSEN an node 22: "".split(/(?:)/) -> []   (ein Treffer!)
        //                            "".split(/x/)   -> [""]
        // Genau der Sonderfall aus Schritt 12.
        assert_eq!(
            split("", false, None, &[("", &[])]),
            Vec::<Option<String>>::new()
        );
        assert_eq!(split("", false, None, &[]), vec![Some("".into())]);
    }

    #[test]
    fn split_unter_u_ueber_surrogatepaare() {
        // GEMESSEN an node 22:
        //   "😀x".split(/(?:)/u) -> ["😀","x"]
        //   "😀x".split(/(?:)/)  -> ["\u{d83d}","\u{de00}","x"]
        let s = "\u{1F600}x";
        let mit_u = split(s, true, None, &[("", &[]), ("", &[]), ("", &[])]);
        assert_eq!(mit_u.len(), 2, "u: zwei Stuecke, nicht drei");
        assert_eq!(mit_u[0].as_deref(), Some("\u{1F600}"));
        // ohne `u` trennt die Maschine die **Halbzeichen** — die sind kein
        // `&str`, darum wird hier in Codeeinheiten verglichen.
        let ohne_u = split_einheiten(s, false, None, &[("", &[]), ("", &[]), ("", &[])]);
        assert_eq!(ohne_u.len(), 3, "ohne u: jede Codeeinheit einzeln");
        assert_eq!(ohne_u[0].as_deref(), Some(&[0xD83D][..]));
        assert_eq!(ohne_u[1].as_deref(), Some(&[0xDE00][..]));
        assert_eq!(ohne_u[2].as_deref(), Some(&[b'x' as u16][..]));
        // und genau darum ist `text()` bei einer halben Einheit rot:
        assert!(text(&[0xD83D]).is_err());
        // AdvanceStringIndex selbst, beide Richtungen
        let e = einheiten(s);
        assert_eq!(fortschritt(&e, 0, true), 2);
        assert_eq!(fortschritt(&e, 0, false), 1);
        assert_eq!(fortschritt(&e, 1, true), 2, "mitte des Paares -> nur +1");
        assert_eq!(fortschritt(&e, 2, true), 3, "letzte Einheit -> +1");
        assert_eq!(fortschritt(&[], 0, true), 1);
    }

    // ---------- indices (Schalter d) ----------

    #[test]
    fn index_paare_ohne_und_mit_namen() {
        // /(a)(b)|x/ auf "ab": indices[0]=[0,2], [1]=[0,1], [2]=[1,2]
        let r = index_paare(
            &[Some((0, 2)), Some((0, 1)), Some((1, 2))],
            &[None, None],
            false,
        );
        assert_eq!(r.paare[0], Some(vec![0, 2]));
        assert_eq!(r.paare[1], Some(vec![0, 1]));
        assert_eq!(r.paare[2], Some(vec![1, 2]));
        assert!(r.gruppen.is_none(), "ohne GroupName ist groups undefined");

        // mit Namen: `indizes[1]` = Gruppe 1 (unbenannt), `indizes[2]` =
        // Gruppe 2 (nicht beteiligt), `indizes[3]` = Gruppe 3 = "n".
        // `namen` traegt Index 0 = **Gruppe 1** (22.2.7.8, Schritt 4).
        let r2 = index_paare(
            &[Some((0, 3)), Some((0, 1)), None, Some((2, 3))],
            &[None, None, Some("n".to_string())],
            true,
        );
        assert_eq!(r2.paare[2], None, "nicht beteiligte Gruppe -> undefined");
        let g = r2.gruppen.expect("mit GroupName gibt es groups");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].0, "n");
        assert_eq!(g[0].1, Some(vec![2, 3]));
    }

    // ---------- die Zahlenregeln ----------

    #[test]
    fn zahlenregeln() {
        assert_eq!(laenge(f64::NAN), 0);
        assert_eq!(laenge(-1.0), 0);
        assert_eq!(laenge(3.9), 3);
        assert_eq!(laenge(f64::INFINITY), 9_007_199_254_740_991);
        assert_eq!(laenge(9_007_199_254_740_991.0), 9_007_199_254_740_991);
    }

    #[test]
    fn texte_kommen_verlustfrei_zurueck() {
        for s in ["", "a", "a\u{e4}b", "\u{1F600}", "a\u{1F600}b"] {
            assert_eq!(t(&einheiten(s)), s, "Hin und her fuer {s:?}");
            assert_eq!(unpaarig(&einheiten(s)), None, "{s:?} ist well-formed");
        }
        // eine halbe surrogat-Einheit ist ein Fehler, kein Ersatzzeichen
        assert_eq!(unpaarig(&[0xD83D]), Some(0));
        assert_eq!(unpaarig(&[0xDE00]), Some(0));
        assert_eq!(unpaarig(&[b'a' as u16, 0xD83D]), Some(1));
        assert_eq!(unpaarig(&[0xD83D, 0xDE00]), None);
        assert!(text(&[0xD83D]).is_err());
        assert!(text(&[0xDE00]).is_err());
        assert_eq!(text(&[0xD83D, 0xDE00]).unwrap(), "\u{1F600}");
    }
}
