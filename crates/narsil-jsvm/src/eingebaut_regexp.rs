//! RegExp-Built-ins: die Rechenkern-Teile der RegExp-Flaeche von
//! test262 `built-ins/RegExp` (und der String-Seite
//! `String.prototype.match/replace/split/search`).
//!
//! GEMESSEN 03.10.2026 (Basislinie `T262_NUR=RegExp`):
//! **33 von 433 Fallen = 7,62 %** (98 uebersprungen). Die Hauptursachen
//! der Fehlschlaege, jede mit Beleg aus dem Lauf:
//!
//!   1. `@@match specified as a primitive boolean` /
//!      `RegExp descriptor should not be enumerable`
//!      -> `RegExp.prototype` kennt nur `test`, `exec`, `toString`
//!      (Registrierung `builtin2.rs:381`). Die **Getter** (`source`,
//!      `flags`, `global`, …) und die **Symbol-Methoden**
//!      (`Symbol.match/replace/split/search/matchAll`) fehlen.
//!   2. `ungueltiger Schalter 'd'` — der Schalter `d` (`hasIndices`)
//!      wird von `regexmini::schalter_aus` abgewiesen (Aufrufstelle
//!      `builtin3.rs:1999`, Constructor).
//!   3. `$<name>`-Ersetzungen und `groups`/`indices` am Treffer-Array.
//!
//! **WARUM DIESE DATEI ALLEIN NICHT REICHT** (Standort genau benannt,
//! damit die Rolle JS nur noch verdrahten muss):
//!
//!   * Diese Datei enthaelt die **Rechenkerne** als `pub(crate)`-Funktionen
//!     mit eigenen Tests — sie sind hier gemessen und gruen.
//!   * Erreichbar gemacht werden sie ueber drei Eingriffe in Dateien, die
//!     Rolle JS gehoeren (siehe `team/BERICHT_WASM_HILFE_JS.txt`):
//!     (a) neue `Nativ`-Varianten in `builtin.rs` (Enum ab Z. 427),
//!     (b) ein `match`-Arm in `rufe_nativ_teil2` (`builtin2.rs:862`),
//!     (c) eine `reg`-Zeile fuer `self.proto_regex` (`builtin2.rs:381`).
//!   * Fuer `Symbol.match` & Co. fehlt **im Objektmodell** die
//!     Moeglichkeit, eine Eigenschaft unter einem Symbol-Schluessel zu
//!     fuehren: `HObj::Gegenstand.eigenschaften` ist ein
//!     `HashMap<String, Eigenschaft>` (`heap.rs:361`). Das ist eine
//!     Aenderung am Modell, keine Verdrahtung — sie gehoert der Rolle JS.
//!
//! Kein `unsafe`, keine Fremd-Crate, Edition 2024, nur `std`.

// Die Funktionen dieser Datei sind der Rechenkern der RegExp-Flaeche;
// die Verdrahtung auf `RegExp.prototype` (Getter, `Symbol.match` & Co.)
// gehoert in `builtin2.rs` und steht noch aus — der Grund ist in
// `team/BERICHT_WASM_HILFE_JS.txt` benannt. Bis dahin waere jedes
// `dead_code`-Rauschen an dieser Datei nur eine Meldung ueber fremden
// Fortschritt, deshalb hier ausdruecklich erlaubt. `quelle_text` ist
// bereits verdrahtet (`builtin3.rs`, `n_regex_text`).
#![allow(dead_code)]

use crate::regexmini::Schalter;

/// Die **Reihenfolge** der Schalter in `RegExp.prototype.flags`
/// (ECMA-262 22.2.6.1, `flags`): `d g i m s u v y`. Die Reihenfolge ist
/// normativ — `String(new RegExp("a","yi")).flags` ist `"iy"`.
pub(crate) const SCHALTER_REIHENFOLGE: [char; 8] = ['d', 'g', 'i', 'm', 's', 'u', 'v', 'y'];

/// `RegExp.prototype.source` (ECMA-262 22.2.6.2) = `EscapeRegExp(P)`.
///
/// **GEMESSEN 03.10.2026 an node 22 (V8):**
///
/// * `a/b.c*d+e?f(g)h[i]j{k}|l^m$n\o` -> `a\/b.c*d+e?f(g)h[i]j{k}|l^m$n\o`
/// * `\d` -> `\d`, `a\.b` -> `a\.b`, `x\\y` -> `x\\y`
/// * `""` -> `(?:)`
///
/// Maskiert werden also **nur** `/` und die Zeilenumbraeche; `. * + ?
/// ( ) [ ] { } | ^ $` **und ein vorhandener Backslash** bleiben
/// unveraendert.
///
/// Das widerspricht der ersten Fassung dieses Kommentars, die „alle
/// Syntaxzeichen" behauptete. Der Test
/// `gewoehnliches_muster_bleibt_unveraendert` hat die Behauptung
/// widerlegt und die Messung bestaetigt ihn: massgeblich ist die
/// Rundlauf-Regel (`source` muss wieder einlesbar sein), nicht die
/// Nennung der Zeichenmenge im Spec-Text.
pub(crate) fn quelle(muster: &str) -> String {
    if muster.is_empty() {
        return "(?:)".to_string();
    }
    let mut r = String::with_capacity(muster.len() + 4);
    for c in muster.chars() {
        match c {
            '/' => {
                r.push('\\');
                r.push(c);
            }
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\u{2028}' => r.push_str("\\u2028"),
            '\u{2029}' => r.push_str("\\u2029"),
            _ => r.push(c),
        }
    }
    r
}

/// `RegExp.prototype.toString` / `toSource` (ECMA-262 22.2.6.3):
/// `"/" + source + "/" + flags` — in genau dieser Form.
///
/// **Verdrahtet** in `builtin3.rs::n_regex_text` (GEMESSEN 03.10.):
/// vorher stand dort `format!("/{muster}/{schalter}")` mit dem ROHEN
/// Muster, also lieferte `String(/a/b/g)` `"/a/b/g"` statt `"/a\/b/g"`.
pub(crate) fn quelle_text(muster: &str, schalter: &str) -> String {
    format!("/{}/{}", quelle(muster), schalter_normale(schalter))
}

/// `RegExp.prototype.flags` (ECMA-262 22.2.6.1): die Schalter in
/// `SCHALTER_REIHENFOLGE`, jeder genau einmal.
///
/// Doppelte Schalter fallen weg (`"gg"` -> `"g"`); die Reihenfolge der
/// Eingabe ist ohne Bedeutung (`"ig"` -> `"gi"`). Unbekannte Zeichen
/// werden **nicht** uebernommen — der Constructor laesst sie gar nicht
/// erst zu, und `flags` ist nur lesbar.
pub(crate) fn schalter_normale(schalter: &str) -> String {
    let mut r = String::with_capacity(schalter.len());
    for c in SCHALTER_REIHENFOLGE {
        if schalter.contains(c) {
            r.push(c);
        }
    }
    r
}

/// Traegt der Schalter `c`? (`global`, `ignoreCase`, `multiline`,
/// `dotAll`, `unicode`, `sticky`, `hasIndices`, `unicodeSets`.)
pub(crate) fn schalter_hat(schalter: &str, c: char) -> bool {
    schalter.contains(c)
}

/// `RegExp.prototype.lastIndex` beim **Schreiben** (ECMA-262
/// 22.2.6.4 `RegExpSetLastIndex`, Schritt 3): `ToLength(ToInteger(value))`.
///
/// `NaN`/`+0`/`-0`/`-0.5` ergeben `0` (mit **positiver** Null),
/// `3.9` ergibt `3`, und `ToLength` kappt bei `2^53-1` — also auch
/// `Infinity`.
///
/// Rust braucht dafuer zwei Bedingungen: `-0.0 < 0.0` ist FALSCH, die
/// negative Null bliebe sonst stehen (GEMESSEN 03.10.: genau das hat
/// der Test `letzte_index_schreibregeln` gefunden).
pub(crate) fn letzte_index(v: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    let t = v.trunc();
    let t = if t < 0.0 || t == 0.0 { 0.0 } else { t };
    t.min(9_007_199_254_740_991.0)
}

/// `lastIndex` **als Ganzzahl in Einheiten** (Zeichen, kein Byte) — so
/// benutzt es `regexmini::suchen` (Startindex in `[u16]`-Einheiten).
pub(crate) fn letzte_index_einheiten(v: f64, laenge: usize) -> usize {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    let als_usize = if v >= laenge as f64 {
        laenge
    } else {
        v as usize
    };
    als_usize.min(laenge)
}

/// Die Schalter als **Bitmaske** — eine Stelle je Schalter, in der
/// Reihenfolge `g i m s y`.
///
/// GEMESSEN 03.10.: `regexmini::Schalter` hat genau diese fuenf Felder
/// (`global`, `egal_gross`, `mehrzeilig`, `punkt_alles`, `klebrig`);
/// `u` wird in `schalter_aus` **geduldet, aber nicht gespeichert**
/// (`regexmini.rs:86`) — eine Maske kann es darum nicht abbilden.
pub(crate) const BIT_GLOBAL: u16 = 1 << 0;
pub(crate) const BIT_EGAL_GROSS: u16 = 1 << 1;
pub(crate) const BIT_MEHRZEILIG: u16 = 1 << 2;
pub(crate) const BIT_PUNKT_ALLES: u16 = 1 << 3;
pub(crate) const BIT_KLEBRIG: u16 = 1 << 4;

/// Die fuenf gespeicherten Schalter als Bitmaske.
pub(crate) fn schalter_maske(f: &Schalter) -> u16 {
    let mut m = 0u16;
    if f.global {
        m |= BIT_GLOBAL;
    }
    if f.egal_gross {
        m |= BIT_EGAL_GROSS;
    }
    if f.mehrzeilig {
        m |= BIT_MEHRZEILIG;
    }
    if f.punkt_alles {
        m |= BIT_PUNKT_ALLES;
    }
    if f.klebrig {
        m |= BIT_KLEBRIG;
    }
    m
}

/// Welche Schalter `regexmini::schalter_aus` **nicht** kennt — der
/// Constructor lehnt sie ab (`builtin3.rs:1999`), also braucht jede
/// weitere RegExp-Flaeche diese Liste als Vorwarnung.
///
/// GEMESSEN 03.10.: `d` (`hasIndices`) fehlt — Fehlschlaege in
/// `built-ins/RegExp/match-indices/` mit genau dieser Meldung.
pub(crate) fn schalter_fehlen(schalter: &str) -> Vec<char> {
    schalter
        .chars()
        // `d` ist seit ES2022 ein **gueltiger** Schalter (`hasIndices`) —
        // GEMESSEN 04.10. an `node` v22 (`new RegExp("a", "d")` laeuft)
        // und an `test/built-ins/RegExp/match-indices/**`, das vorher an
        // diesem Zeichen scheiterte.
        // `v` (unicodeSets) ist seit ES2024 gueltig.
        .filter(|c| !matches!(c, 'g' | 'i' | 'm' | 's' | 'y' | 'u' | 'd' | 'v'))
        .collect()
}

// ======================================================================
// Tests — jede Pruefung sichert ein Ergebnis (M7).
// ======================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leeres_muster_liefert_leere_gruppe() {
        // ECMA-262 22.2.6.2: leeres Muster -> `(?:)` (GEMESSEN an node).
        assert_eq!(quelle(""), "(?:)");
    }

    #[test]
    fn schraegstrich_und_zeilenumbruch_werden_maskiert() {
        // GEMESSEN 03.10. an node 22 (V8): `source` maskiert NUR den
        // Schraegstrich und die Zeilenumbraeche. Ein vorhandener
        // Backslash bleibt unveraendert.
        assert_eq!(quelle("a/b"), "a\\/b");
        assert_eq!(quelle("a/b/c"), "a\\/b\\/c");
        assert_eq!(quelle("a\\b"), "a\\b");
        assert_eq!(quelle("a\\/b"), "a\\\\/b");
        assert_eq!(quelle("a\\\\b"), "a\\\\b");
    }

    #[test]
    fn zeilenumbrueche_werden_zu_einer_zwei_stelligen_folge() {
        assert_eq!(quelle("a\nb"), "a\\nb");
        assert_eq!(quelle("a\rb"), "a\\rb");
        assert_eq!(quelle("a\u{2028}b"), "a\\u2028b");
        assert_eq!(quelle("a\u{2029}b"), "a\\u2029b");
    }

    #[test]
    fn gewoehnliches_muster_bleibt_unveraendert() {
        // GEMESSEN 03.10. an node 22: `source` aendert NICHTS an, was
        // nicht `/` oder Zeilenumbruch ist — auch nicht `.`, `^`, `$`,
        // die Klammern, `*`, `+`, `?`, `{`, `}` und `|`.
        assert_eq!(quelle("[a-z]+"), "[a-z]+");
        assert_eq!(quelle("^a.b$"), "^a.b$");
        assert_eq!(quelle("(a|b)*+?{1,2}"), "(a|b)*+?{1,2}");
        assert_eq!(quelle("^\\d{2,3}$"), "^\\d{2,3}$");
    }

    #[test]
    fn quelle_text_setzt_beide_striche_daneben() {
        // `new RegExp("a/b", "g").toString()` -> `"/a\/b/g"`.
        assert_eq!(quelle_text("a/b", "g"), "/a\\/b/g");
        assert_eq!(quelle_text("", ""), "/(?:)/");
        // Die Schalter stehen normiert in `flags`-Reihenfolge.
        assert_eq!(quelle_text("a", "yi"), "/a/iy");
    }

    #[test]
    fn flags_stehen_in_normativer_reihenfolge() {
        // ECMA-262 22.2.6.1: die Ausgabe ist immer `dgimsuvy`-geordnet.
        assert_eq!(schalter_normale("yi"), "iy");
        assert_eq!(schalter_normale("gim"), "gim");
        assert_eq!(schalter_normale("mig"), "gim");
        assert_eq!(schalter_normale(""), "");
    }

    #[test]
    fn doppelte_und_unbekannte_schalter_fallen_weg() {
        assert_eq!(schalter_normale("gg"), "g");
        // "xyz" enthaelt das echte Schalterzeichen `y` — das steht also
        // drin. Unbekannt sind z. B. `x`, `q`, `z` (GEMESSEN 03.10.:
        // `y` wird von `schalter_aus` durchaus akzeptiert).
        assert_eq!(schalter_normale("xyz"), "y");
        assert_eq!(schalter_normale("xqz"), "");
    }

    #[test]
    fn schalter_hat_prueft_einzeln() {
        assert!(schalter_hat("gi", 'g'));
        assert!(schalter_hat("gi", 'i'));
        assert!(!schalter_hat("gi", 'm'));
        assert!(!schalter_hat("", 'g'));
    }

    #[test]
    fn last_index_schreibregeln() {
        // NaN -> 0
        assert_eq!(letzte_index(f64::NAN), 0.0);
        // -0 -> +0 (`-0.0 < 0.0` ist falsch, das braucht die zweite
        // Bedingung — ohne sie blieb die negative Null stehen)
        assert_eq!(letzte_index(-0.0).to_bits(), 0.0f64.to_bits());
        // negativ -> 0 (auch -0.5, das erst trunkiert wird)
        assert_eq!(letzte_index(-0.5), 0.0);
        assert_eq!(letzte_index(-7.0), 0.0);
        // unveraendert bzw. abgeschnitten
        assert_eq!(letzte_index(3.0), 3.0);
        assert_eq!(letzte_index(3.9), 3.0);
        // `ToLength(Infinity)` = 2^53-1 (der ToLength-Constructor
        // kappt, NICHT erst danach — die erste Fassung dieses Kommentars
        // behauptete das Gegenteil).
        assert_eq!(letzte_index(f64::INFINITY), 9_007_199_254_740_991.0);
        assert_eq!(letzte_index(1e19), 9_007_199_254_740_991.0);
    }

    #[test]
    fn last_index_als_einheiten_bleibt_in_der_laenge() {
        assert_eq!(letzte_index_einheiten(0.0, 10), 0);
        assert_eq!(letzte_index_einheiten(4.0, 10), 4);
        // laenger als der Text -> auf die Laenge begrenzt (kein Ueberlauf)
        assert_eq!(letzte_index_einheiten(99.0, 10), 10);
        assert_eq!(letzte_index_einheiten(f64::INFINITY, 10), 10);
        // ein Text der Laenge 0 kann nie starten
        assert_eq!(letzte_index_einheiten(1.0, 0), 0);
    }

    #[test]
    fn schalter_maske_entspricht_regexmini() {
        let f = crate::regexmini::schalter_aus("gimsu").expect("gimsu ist erlaubt");
        let m = schalter_maske(&f);
        assert_eq!(m & BIT_GLOBAL, BIT_GLOBAL, "global");
        assert_eq!(m & BIT_EGAL_GROSS, BIT_EGAL_GROSS, "ignoreCase");
        assert_eq!(m & BIT_MEHRZEILIG, BIT_MEHRZEILIG, "multiline");
        assert_eq!(m & BIT_PUNKT_ALLES, BIT_PUNKT_ALLES, "dotAll");
        // `y` fehlt in "gimsu" -> die Bitte bleibt leer
        assert_eq!(m & BIT_KLEBRIG, 0, "sticky ist nicht gesetzt");
        // Mit `y` ist sie gesetzt (GEMESSEN: das Feld heisst `klebrig`).
        let g = crate::regexmini::schalter_aus("y").expect("y ist erlaubt");
        assert_eq!(schalter_maske(&g) & BIT_KLEBRIG, BIT_KLEBRIG);
    }

    /// Die Gruppennamen-Regel (ECMA-262 22.2.1.1) — M7: jeder Fall
    /// **sichert ein Ergebnis**, gueltige wie ungueltige.
    ///
    /// Die Mustertexte enthalten echte Regex-Escapes (`\u03c0`). Damit der
    /// Rust-Compiler sie nicht selbst deutet (Rust kennt nur `\u{…}`),
    /// steht in den Mustern ein `!` **anstelle des Backslashes**; es wird
    /// unten zu `\` ersetzt. Die **erwarteten Namen** stehen als Zeichen —
    /// der Kern dekodiert, das ist der Punkt der Probe.
    #[test]
    fn gruppennamen_der_norm_werden_angenommen() {
        for (muster, soll) in [
            ("(?<a>a)", "a"),
            (r"(?<!u03c0>a)", "\u{3c0}"),
            ("(?<$>a)", "$"),
            ("(?<_>a)", "_"),
            (r"(?<_!u200c>a)", "_\u{200c}"),
            (r"(?<_!u200d>a)", "_\u{200d}"),
            (r"(?<!u0ca0_!u0ca0>a)", "\u{ca0}_\u{ca0}"),
            // Escape im Namen wird **dekodiert**, nicht roh uebernommen.
            (r"(?<!u0041>a)", "A"),
            (r"(?<!u{1d4d1}!u{1d4fb}>a)", "\u{1d4d1}\u{1d4fb}"),
            (r"(?<!ud835!udcd1>a)", "\u{1d4d1}"),
            // Ziffern sind erlaubt: `RegExpIdentifierStart` enthaelt
            // `UnicodeIDContinue` (Spec 22.2.1.1).
            ("(?<0a>a)", "0a"),
            (r"(?<the!u{1d7da}>the)", "the\u{1d7da}"),
        ] {
            let muster = muster.replace('!', "\\");
            match crate::regexmini::kompilieren(&muster) {
                Ok((_, _, namen)) => {
                    assert_eq!(namen.last().map(String::as_str), Some(soll), "{muster}");
                }
                Err(e) => panic!("{muster} wurde abgewiesen: {e}"),
            }
        }
    }

    #[test]
    fn ungueltige_gruppennamen_werden_abgewiesen() {
        // GEMESSEN an `named-groups/non-unicode-property-names-invalid.js`:
        // die Emoji sind keine Bezeichner, und U+1D7DA ist als
        // **Anfang** nicht erlaubt (als Fortsetzung schon).
        for muster in [
            r"(?<!u{1f98a}>fox)",
            r"(?<!ud83e!udd8a>fox)",
            r"(?<!u{1f415}>dog)",
            r"(?<!u{1d7da}the>the)",
            r"(?<!ud835!udfdathe>the)",
            "(?<a-b>x)",
            "(?< x>x)",
            "(?<>x)",
            "(?<a>x)(?<a>y)",
            r"(?<!uq>x)",
        ] {
            let muster = muster.replace('!', "\\");
            assert!(
                crate::regexmini::kompilieren(&muster).is_err(),
                "{muster} wurde angenommen"
            );
        }
    }

    /// Gruppen **innerhalb** eines Lookbehinds muessen erhalten bleiben
    /// (ECMA-262 22.2.2.9). GEMESSEN 03.10.:
    /// `named-groups/lookbehind.js` (5 gezaehlte Faelle).
    ///
    /// **BEKANNTE ABWEICHUNG (gemessen, nicht kaschiert):** bei einer
    /// *wiederholten* Gruppe im Lookbehind liefert der Vorwaechs-Lauf das
    /// **rechteste** Zeugnis, die Norm verlangt das **linkeste**
    /// (`(?<=(?<a>\w){4})f` auf `"abcdef"`: Norm `"b"`, wir `"e"`). Das
    /// ist eine Rueckwaechs-Uebersetzung, die es braucht — der Vorwaechs-Lauf
    /// kann sie nicht liefern. Die Probe sichert deshalb beides: das, was
    /// die Norm verlangt **und was der Motor kann** (unwiederholte Gruppe),
    /// und den gemessenen Wert der Abweichung.
    /// Die Abkuerzungen `\w`, `\d`, `\s` matchen wieder (Sortier-Reparatur).
    ///
    /// GEMESSEN 03.10.: `klasse()` sortierte seine Bereiche, `escape()` nicht,
    /// und der Matcher sucht **binaer** — deshalb lieferten `/\w/`, `/\d/`,
    /// `/\s/` **keinen** Treffer, `[\w]` in Klammern schon. Diese Probe
    /// sichert genau das: die Abkuerzung **ausserhalb** der Klammer, und
    /// ihre negierte Form (die `komplement()` erzeugt — dort ist die
    /// Sortier-Invariante besonders leicht zu verletzen).
    #[test]
    fn abkuerzungen_matchen_ausserhalb_der_klammer() {
        for (muster, soll, darf_nicht) in [
            // \w = [A-Za-z0-9_] (ECMA-262 22.2.2.6)
            (r"!w", "aA0_", "\u{1F98A}-\u{4e00}"),
            (r"!W", "- \u{e4}", "aA0_"),
            (r"!d", "07", "a- "),
            (r"!D", "a- ", "07"),
            (r"!s", " \t\n", "a0"),
            (r"!S", "a0", " \t\n"),
            // und in Klammern (dieser Weg war schon richtig)
            (r"[!w]", "aA0_", "\u{1F98A}-\u{4e00}"),
            (r"[!d]", "07", "a- "),
        ] {
            let muster = muster.replace('!', "\\");
            let (knoten, gn, _) = crate::regexmini::kompilieren(&muster).expect("muster");
            let f = crate::regexmini::schalter_aus("").expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(soll);
            assert!(
                crate::regexmini::suchen(&knoten, gn, f, &einheiten, 0).is_some(),
                "{muster} passt nicht auf {soll:?}"
            );
            let nicht = crate::regexmini::als_einheiten(darf_nicht);
            assert!(
                crate::regexmini::suchen(&knoten, gn, f, &nicht, 0).is_none(),
                "{muster} passt auf {darf_nicht:?}, darf es nicht"
            );
        }
    }

    /// `\k<name>` — benannter Rueckverweis (ECMA-262 22.2.2.8).
    ///
    /// GEMESSEN 03.10. (`named-groups/non-unicode-match.js`, 4 gezaehlte
    /// Faelle): `\k` fehlte ganz und wurde als Buchstabe `k` gelesen, also
    /// lieferte `/(?<a>.)(?<b>.)(?<c>.)\k<c>\k<b>\k<a>/` kein Treffer und
    /// der Test brach mit „kein Zugriff auf Null" ab. Die Nummernsuche
    /// benutzt eine eigene Liste **mit** Nummer: `namen` traegt nur die
    /// benannten Gruppen (Vertrag von `builtin2.rs`), also waere
    /// `/(a)(?<b>x)\k<b>/` sonst auf Gruppe 1 statt 2 gegangen.
    #[test]
    fn benannter_rueckverweis_zeigt_auf_die_gruppe() {
        // (Muster, Text, erwarteter Trefferanfang, erwartetes Ende)
        for (muster, text, a, e) in [
            // Selbstbezug ist erlaubt — und trifft eine noch nicht
            // gesetzte Gruppe, also die **leere** (Spec 22.2.2.8
            // BackreferenceMatcher: „if r's matched is empty, return
            // empty"). GEMESSEN an node 22: `"aa".match(/(?<a>\k<a>)/)[0]`
            // ist `""`.
            (r"(?<a>!k<a>)", "aa", 0usize, 0usize),
            // drei benannte Gruppen, umgekehrte Reihenfolge
            (r"(?<a>.)(?<b>.)(?<c>.)!k<c>!k<b>!k<a>", "abccba", 0, 6),
            // die Nummernsuche muss die **Gruppen-Nummer** treffen, nicht
            // die Position in der Namensliste: hier ist Gruppe 1 unbenannt.
            (r"(x)(?<b>!w)!k<b>", "xaa", 0, 3),
            (r"(?<b>!w)(x)!k<b>", "axa", 0, 3),
            // **Vorwaertsverweis** (ECMA-262 22.2.1: die Namen werden erst
            // nach dem ganzen Muster geprueft). GEMESSEN 03.10.:
            // `named-groups/non-unicode-references.js` (2 Faelle).
            (r"!k<a>(?<a>b)!w!k<a>", "bab", 0, 3),
            (r"(?<b>b)!k<a>(?<a>a)!k<b>", "bab", 0, 3),
            // GEMESSEN an node 22: `"baa".match(/\k<b>(?<b>b)(?<a>\k<a>)/)`
            // liefert `"b"` und `groups.a === ""` — beide Vorgaenge im
            // Leerlauf (ungesetzte Gruppe), also **ein** Zeichen.
            (r"!k<b>(?<b>b)(?<a>!k<a>)", "baa", 0, 1),
        ] {
            let muster = muster.replace('!', "\\");
            let (knoten, gn, namen) = crate::regexmini::kompilieren(&muster).expect("muster");
            assert!(!namen.is_empty(), "{muster} hat keine benannte Gruppe");
            let f = crate::regexmini::schalter_aus("").expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(text);
            let t = crate::regexmini::suchen(&knoten, gn, f, &einheiten, 0)
                .unwrap_or_else(|| panic!("{muster} passt nicht auf {text:?}"));
            assert_eq!((t.anfang, t.ende), (a, e), "{muster}");
        }
    }

    #[test]
    fn unbekannter_name_im_rueckverweis_ist_ein_fehler() {
        for muster in [r"!k<quatsch>", r"(?<a>a)!k<b>", r"!k<>"] {
            let muster = muster.replace('!', "\\");
            assert!(
                crate::regexmini::kompilieren(&muster).is_err(),
                "{muster} wurde angenommen"
            );
        }
        // `\k` ohne `<` ist ein Literal `k` (Annex B) — kein Fehler.
        assert!(crate::regexmini::kompilieren(r"!k").is_ok());
    }

    #[test]
    fn lazy_quantifier_versus_quantifier_fragezeichen() {
        // GEMESSEN 04.10. (Orakel `node` v22, Lauf ueber 920 Faelle aus
        // `test/built-ins/RegExp`): `regexmini::wiederholung` fraß das `?`
        // eines Lazy-Markers als „verdoppelte Quantifier" — damit waren
        // `a*?`, `a+?`, `a??` und `a{n,m}?` **alle gierig**.
        //
        // Das `?` **als Quantifier** (`java(script)?`, `b?b?b?b`) ist
        // dagegen gierig, und `a??` ist lazy optional (Annex B). Beide
        // Richtungen stehen in dieser Probe, damit eine Vereinfachung an
        // dieser Stelle sofort auffaellt.
        fn treffer(muster: &str, text: &str) -> Option<(usize, usize)> {
            let (knoten, n, _) = crate::regexmini::kompilieren(muster).expect("kompilieren");
            let sch = crate::regexmini::schalter_aus("").expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(text);
            crate::regexmini::suchen(&knoten, n, sch, &einheiten, 0).map(|t| (t.anfang, t.ende))
        }
        // Lazy: kuerzeste moegliche Uebereinstimmung.
        assert_eq!(treffer(r"a[a-z]{2,4}?", "abcdefghi"), Some((0, 3)));
        assert_eq!(treffer(r"^.*?(:|$)", "Hello: World"), Some((0, 6)));
        assert_eq!(treffer(r"a*?", "aaa"), Some((0, 0)));
        // `a??` = lazy optional -> leerer Treffer zuerst.
        assert_eq!(treffer(r"a??", "a"), Some((0, 0)));
        // Gierig: `?` als Quantifier, `*` nimmt so viel wie moeglich.
        assert_eq!(treffer(r"b?b?b?b", "abbbbc"), Some((1, 5)));
        assert_eq!(treffer(r"^(A)?(A.*)$", "AAA"), Some((0, 3)));
        assert_eq!(
            treffer(r"java(script)?", "state: javascript"),
            Some((7, 17))
        );
    }

    #[test]
    fn lange_wiederholung_und_erschoepftes_budget() {
        // GEMESSEN 04.10. drei Grenzen hintereinander, jede mit eigener
        // Ursache — die Zahlen stehen an `MAX_WIEDERHOL_TIEFE`,
        // `MAX_WIEDERHOL_RUNDEN` und `schritt_budget`.
        fn treffer(muster: &str, text: &str) -> Option<(usize, usize)> {
            let (knoten, n, _) = crate::regexmini::kompilieren(muster).expect("kompilieren");
            let f = crate::regexmini::schalter_aus("").expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(text);
            crate::regexmini::suchen(&knoten, n, f, &einheiten, 0).map(|t| (t.anfang, t.ende))
        }
        // (1) Eine gierige Wiederholung darf **nicht** an einer
        //     Rekursionsgrenze enden: `a+` auf 2000 Zeichen liefert 0..2000.
        //     Vorher: 0..512 (Tiefenprobe 500/511 gut, 513/600/2000 falsch).
        let lang = "a".repeat(2000);
        assert_eq!(treffer("a+", &lang), Some((0, 2000)));
        // (2) Ein **erschoepftes** Budget liefert "kein Treffer", nie einen
        //     falschen: der Lauf bricht ab, seine Standliste ist
        //     unvollstaendig. Ohne diese Regel meldete `\w+q` auf 20.000
        //     Zeichen einen Treffer, den es nicht gibt.
        let viel = "a".repeat(20000);
        assert_eq!(treffer("\\w+q", &viel), None);
        // (3) Das verzweigende Muster `(?:a|aa)+b` beendet sich und meldet
        //     nichts. Es war der Stapelueberlauf: die Rekursionstiefe muss
        //     `MAX_WIEDERHOL_TIEFE` auch benutzen (mit 100.000 Runden
        //     brachte der Lauf 20.000 Zeichen nicht hinter sich).
        assert_eq!(treffer("(?:a|aa)+b", &viel), None);
    }

    #[test]
    fn i_ohne_u_kanonisiert_statt_gefalten() {
        // ECMA-262 22.2.2.8: `i` **ohne** `u` vergleicht **kanonisiert**
        // (`Canonicalize`), mit `u` **gefaltet**. GEMESSEN 04.10. als
        // Orakel `node` v22 ueber 44 Handpaare: die gefaltete Liste traf
        // ohne `u` **acht** Zeichen zu viel.
        fn trifft(schalter: &str, muster: &str, text: &str) -> bool {
            let (knoten, n, _) = crate::regexmini::kompilieren(muster).expect("kompilieren");
            let f = crate::regexmini::schalter_aus(schalter).expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(text);
            crate::regexmini::suchen_einheiten(&knoten, n, f, schalter.contains('u'), &einheiten, 0)
                .is_some()
        }

        // (1) Die Faltung gehoert **ausschliesslich** unter `u`.
        // Kelvinzeichen, Langer s, `\u00DF`/`\u1E9E`, punktloses I,
        // U+0390/U+1FD3 und Angstromzeichen: alle **nur** mit `u`.
        assert!(!trifft("i", "[K]", "\u{212A}"), "Kelvin ohne u");
        assert!(trifft("iu", "[K]", "\u{212A}"), "Kelvin mit u");
        assert!(!trifft("i", "[\u{00DF}]", "\u{1E9E}"), "ss/SS ohne u");
        assert!(trifft("iu", "[\u{00DF}]", "\u{1E9E}"), "ss/SS mit u");
        assert!(!trifft("i", "[\u{212B}]", "\u{00C5}"), "Angstrom ohne u");
        assert!(trifft("iu", "[\u{212B}]", "\u{00C5}"), "Angstrom mit u");

        // (2) Kanonisierung **ja** — die Grossschreibung entscheidet, und
        // sie ist gerade fuer Bereiche noetig (`[À-Þ]` haette kein
        // einzelnes Zeichen, das auf `à` zeigt).
        assert!(trifft("i", "[a-z]", "A"));
        assert!(trifft("i", "[σ]", "\u{3C2}"), "Sigma/ finales Sigma");
        assert!(trifft("i", "[\u{03C2}]", "\u{3A3}"), "finales Sigma/Sigma");
        assert!(trifft("i", "[\u{00C0}-\u{00DE}]", "\u{00E0}"), "Bereich");
        assert!(trifft("i", "[\u{FF21}-\u{FF3A}]", "\u{FF41}"), "Vollbreite");
        assert!(trifft("i", "[\u{01C4}]", "\u{01C5}"), "Titelfall");

        // (3) Zwei Stellen, an denen `node` **vom Spec abweicht**: es
        // benutzt eine unvollstaendige Grossschreibungstabelle. Nach
        // ECMA-262 22.1.1.17 kanonisieren `\u{017F}` und `\u{0131}` beide
        // nach `S` bzw. `I`, also **mit** Treffer. `node` sagt hier
        // "nein" — wir halten uns an die Spec, sonst haengt die
        // amtliche Quote an einer Engine-Eigenheit.
        assert!(trifft("i", "s", "\u{017F}"), "Langer s, Spec");
        assert!(trifft("i", "\u{0131}", "I"), "punktloses I, Spec");
    }

    #[test]
    fn punkt_trifft_unter_u_den_codepoint() {
        // GEMESSEN 04.10. (Orakel `node` v22): `.` mit `u` trifft einen
        // **Codepoint**, ohne `u` genau eine UTF-16-Einheit:
        //
        //     "u"  .  -> 0..2      ""  .  -> 0..1
        //     "su" .  -> 0..2      "u" . auf "a" -> 0..1
        //
        // Die Schalterliste (`g i m s y`) traegt `u` **nicht**, weil sie an
        // vielen fremden Stellen als Struktur-Literal gebaut wird; der
        // Unicode-Modus kommt deshalb ueber `suchen_einheiten`.
        fn treffer(schalter: &str, text: &str) -> Option<(usize, usize)> {
            let (knoten, n, _) = crate::regexmini::kompilieren(".").expect("kompilieren");
            let f = crate::regexmini::schalter_aus(schalter).expect("schalter");
            let einheiten = crate::regexmini::als_einheiten(text);
            crate::regexmini::suchen_einheiten(&knoten, n, f, schalter.contains('u'), &einheiten, 0)
                .map(|t| (t.anfang, t.ende))
        }
        assert_eq!(treffer("u", "\u{1D401}"), Some((0, 2)));
        assert_eq!(treffer("su", "\u{1D306}"), Some((0, 2)));
        assert_eq!(treffer("", "\u{1D401}"), Some((0, 1)));
        assert_eq!(treffer("u", "a"), Some((0, 1)));
    }

    #[test]
    fn unbekannte_schalter_werden_gemeldet() {
        // GEMESSEN 03.10.: `d` lehnt der Constructor ab — die Liste hier
        // sagt das vorher, statt es im Testlauf zu zeigen.
        // `d` ist gueltig (ES2022) — GEMESSEN an `node` v22 und an
        // `test/built-ins/RegExp/match-indices/**` (vorher Fehler 45).
        // `v` (unicodeSets) ist seit ES2024 gueltig.
        assert!(schalter_fehlen("gd").is_empty());
        assert!(schalter_fehlen("dgimsu").is_empty());
        assert!(schalter_fehlen("dgimsu").is_empty());
        assert!(schalter_fehlen("dgimsu").is_empty());
        assert_eq!(schalter_fehlen("gimsu"), Vec::<char>::new());
        assert_eq!(schalter_fehlen("dgimsu"), Vec::<char>::new());
        assert_eq!(schalter_fehlen("dgimsu"), Vec::<char>::new());
        assert_eq!(schalter_fehlen("dv"), Vec::<char>::new());
    }
}
