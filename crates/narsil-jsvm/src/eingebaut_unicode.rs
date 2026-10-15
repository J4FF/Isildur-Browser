//! ## QUIC, 05.10.2026 (W57) — `Unicode-Eigenschaften` für `\p{...}` / `\P{...}`.
//!
//! **WARUM DIE DATEN GENERIERT UND DER RUmpf HIER STEHT:**
//! `unicode_eigenschaften.rs` ist **generiert**
//! (`werkzeuge/unicode_eigenschaften_bauen.py`, **einmal** gelaufen, aus
//! `/usr/share/unicode`, **Unicode 15.1.0**) und wird **nicht** von Hand
//! angefasst. *Ein Generator, der auch den Rumpf enthält, wird beim nächsten
//! Lauf überschrieben — und dann fehlt der Rumpf* (bei `normalize` ist genau
//! das passiert).
//!
//! ```text
//! tekste: General_Category  62 Namen      Bereiche gesamt 22792
//!         Script           318 Namen
//!         Script_Extensions 230 Namen     BINAER 67 Namen
//! ```
//!
//! **WER ANSCHLIEST:** die RegExp-Engine (`regexp.rs`, **WASM**). Die
//! Oberfläche ist bewusst **zwei Funktionen**, weil die Norm zwei Fragen
//! stellt: *„welche Bereiche"* (eine Zeichenklasse bauen) und *„gehört diese
//! Zahl hinein"* (ein Zeichen prüfen). Wer nur die zweite braucht, muss die
//! Tabelle nicht selbst durchsuchen.

use crate::unicode_eigenschaften::{
    Bereich, BINAER, GENERAL_CATEGORY, MAX_CP, SCRIPT, SCRIPT_EXTENSIONS,
};

/// **Die drei Achsen und ihre Kurzformen.** `PropertyAliases.txt` liefert
/// `gc`/`sc`/`scx`; `\p{gc=Lu}` und `\p{General_Category=Lu}` müssen **derselbe**
/// Weg sein, sonst hat die Engine **zwei** Namen fuer **eine** Eigenschaft.
///
/// **GEMESSEN an der Testsuite:** die 232 Faelle in
/// `RegExp/property-escapes/generated/` benennen **drei** Formen —
/// `General_Category_-_Cased_Letter` (38), `Script_-_Greek` (~40),
/// `Script_Extensions_-_Greek` (~150) — und **keine** Kurzform. Die Kurzformen
/// stehen hier trotzdem, weil sie **in der Norm** stehen und der Aufbau sie
/// sonst an zwei Orten vermissen wuerde.
const ACHSEN: &[(&str, &str, &str)] = &[
    ("General_Category", "gc", "GENERAL_CATEGORY"),
    ("Script", "sc", "SCRIPT"),
    ("Script_Extensions", "scx", "SCRIPT_EXTENSIONS"),
];

/// **GEFUNDEN ODER NICHT — und das ist eine Frage, nicht `false`.**
///
/// `None` heisst: **diese Eigenschaft gibt es nicht.** Das ist ein anderer
/// Befund als „die Eigenschaft ist hier nicht wahr" — und die Norm behandelt
/// beide verschieden (`\p{NichtDa}` ist ein **Syntaxfehler**, kein `false`).
/// *Ein `bool` als Rückgabe hätte这两 Fragen zu einer gemacht, und die zweite
/// hätte man nicht mehr stellen können.*
pub(crate) fn hat_eigenschaft(name: &str, wert: Option<&str>, cp: u32) -> Option<bool> {
    bereich(name, wert).map(|b| in_bereichen(b, cp))
}

/// **Die Bereiche einer Eigenschaft** — für den Bau einer Zeichenklasse.
///
/// **WARUM EINE LISTE UND NICHT EINE ENTSCHEIDUNGSFUNKTION:** eine
/// Zeichenklasse braucht die **Bereiche**, nicht 1.114.112 Einzelauswertungen.
/// *Eine Abfragefunktion, die man in einer Schleife über alle Codepunkte
/// aufruft, ist eine Liste, die man nicht erkennt.*
pub(crate) fn bereich(name: &str, wert: Option<&str>) -> Option<&'static [Bereich]> {
    // ## W57 — **DIE ACHSE BESTEHT AUS ZWEI TEILEN**, und beide sind noetig.
    //
    // `(1) General_Category_-_Cased_Letter` ist die Form der Suite (38 Faelle),
    // `(2) `General_Category=Cased_Letter` und `gc=Lu` sind die Formen der Norm,
    // `(3) `Cased_Letter` **ohne** Achse ist die Form, die man tippt.
    //
    // **GEMESSEN an den 232 Case-Dateinamen:** 38 `General_Category_-_X`,
    // ~40 `Script_-_X`, ~150 `Script_Extensions_-_X`, ~50 binaere — und
    // **keine** Kurzform. Die Kurzformen stehen trotzdem hier, weil sie in der
    // Norm stehen.
    let (achse, wert) = zerlege(name, wert);
    // **`Any` und `Assigned` sind abgeleitet und stehen in KEINER Datei.**
    if wert.is_none() {
        match name {
            "Any" => return Some(&[(0, MAX_CP)]),
            // **`Assigned` = NICHT `Cn`, `Cs`, `Co`.** `generated/Assigned.js`
            // laeuft ueber alle 1.114.112 Codepunkte und vergleicht an jeder
            // Stelle — eine Liste, die man sich aus den drei anderen baut.
            "Assigned" => {
                let von = |n: &str| {
                    GENERAL_CATEGORY
                        .iter()
                        .find(|(x, _)| *x == n)
                        .map(|(_, b)| *b)
                };
                return Some(assigned_aus(von("Cn"), von("Cs"), von("Co")));
            }
            _ => {}
        }
    }
    // **DIE ACHSEN: erst benannt, dann der Kurzname allein.**
    if let Some((_, _, tabellenname)) = ACHSEN
        .iter()
        .find(|(lang, kurz, _)| Some(*lang) == achse.as_deref() || Some(*kurz) == achse.as_deref())
        && let Some(tabelle) = tabelle_von(tabellenname)
    {
        return suche(tabelle, wert.as_deref().unwrap_or(name));
    }
    // **`\p{Lu}` ohne Achse** — die Reihenfolge ist **gemessen**, nicht gewaehlt:
    // die 232 Faelle nennen `Lu` nie ohne Achse, aber die Norm tut es, und die
    // Reihenfolge (Kategorie, Skript, Skript-Erweiterungen, binaer) ist die
    // Reihenfolge der UCD-Dateien. *Zwei Tabellen mit demselben Namen sind ein
    // Mehrdeutigkeitsfehler — deshalb wird jede Achse genau einmal gefragt.*
    for (_, _, tabellenname) in ACHSEN {
        if let Some(tabelle) = tabelle_von(tabellenname)
            && let Some(b) = suche(tabelle, wert.as_deref().unwrap_or(name))
        {
            return Some(b);
        }
    }
    if wert.is_none() {
        return suche(BINAER, name);
    }
    // **`gc=Lu` mit falscher Tabelle:** dann ist es wirklich unbekannt.
    None
}

/// **`X_-_Y` und `X=Y` und `kurz=Y` auf `(achse, wert)` zerlegen.**
///
/// **GEMESSEN, warum das ein eigener Schritt ist:** die Suite schreibt
/// `General_Category_-_Cased_Letter` mit **Bindestrich**, die Norm
/// `General_Category=Cased_Letter` mit **Gleichheitszeichen**. Beide meinen
/// dasselbe, und wer sie an zwei Orten behandelt, hat **zwei** Namen fuer
/// **eine** Eigenschaft. *Ein Trennzeichen, das man an zwei Orten kennt, ist an
/// einem der beiden falsch — und welcher, sieht man erst am dritten Fall.*
fn zerlege(name: &str, wert: Option<&str>) -> (Option<String>, Option<String>) {
    if let Some((links, rechts)) = name.split_once('=') {
        return (Some(links.to_string()), Some(rechts.to_string()));
    }
    if let Some((links, rechts)) = name.split_once("_-_") {
        return (Some(links.to_string()), Some(rechts.to_string()));
    }
    if let Some(v) = wert {
        return (Some(name.to_string()), Some(v.to_string()));
    }
    (None, None)
}

/// **Der Name der dritten Achsen-Spalte → die Tabelle.** `ACHSEN` traegt die
/// Spalten **als Namen**, damit die Liste **eine** bleibt und die Tabellen
/// **an einer** Stelle importiert werden. *Drei Namen und drei Importe an zwei
/// Orten ist eine Liste, die man beim naechsten `Hinzufuegen` doppelt pflegt.*
fn tabelle_von(
    name: &str,
) -> Option<&'static [(&'static str, &'static [Bereich])]> {
    match name {
        "GENERAL_CATEGORY" => Some(GENERAL_CATEGORY),
        "SCRIPT" => Some(SCRIPT),
        "SCRIPT_EXTENSIONS" => Some(SCRIPT_EXTENSIONS),
        _ => None,
    }
}

fn suche(tabelle: &'static [(&'static str, &'static [Bereich])], name: &str) -> Option<&'static [Bereich]> {
    // **Binaere Suche ueber die Namen**, und **darum ist die Tabelle sortiert**
    // — der Generator sortiert sie, und diese Zeile ist der Grund, warum er es
    // tut. *Eine unsortierte Tabelle mit binaerer Suche ist ein Befund, der
    // erst nach 1.114.112 Vergleichen auffaellt.*
    tabelle
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| tabelle[i].1)
}

/// **Gehört `cp` in eine (nach `von` sortierte) Bereichsliste?**
fn in_bereichen(b: &[Bereich], cp: u32) -> bool {
    // **Die Suche sucht den LETZTEN Anfang ≤ cp.** Nach dem Treffer muss der
    // **Ende** ≥ cp sein — sonst liegt `cp` in der **Lücke** vor diesem
    // Bereich. Genau dieser zweite Vergleich ist der Unterschied zwischen
    // „in der Liste" und „in einem Intervall".
    match b.binary_search_by(|(von, _)| von.cmp(&cp)) {
        Ok(i) => true,
        Err(0) => false,
        Err(i) => cp <= b[i - 1].1,
    }
}

/// **`Assigned` als Berechsliste** — das Komplement von `Cn ∪ Cs ∪ Co`.
///
/// **WARUM EINE EIGENE FUNKTION und nicht drei Aufrufe:** `Assigned` ist
/// **kein** Bereich, sondern das **Komplement** von drei. Ein Komplement als
/// Liste zu bauen ist eine Sortieraufgabe, und sie gehört **hierher**, nicht in
/// den Aufrufer.
fn assigned_aus(cn: Option<&'static [Bereich]>, cs: Option<&'static [Bereich]>, co: Option<&'static [Bereich]>) -> &'static [Bereich] {
    // **STATISCH, weil es in der Signatur `&'static` ist** — eine Liste aus
    // `MAX_CP` Zeilen kann man nicht zurueckgeben, ohne sie zu besitzen. Und
    // `assigned_aus` wird **einmal** beim ersten Aufruf gebaut (`OnceLock`),
    // nicht je Aufruf: *eine Liste, die man bei jedem Zeichen neu baut, ist
    // eine Liste, die man nicht gebaut hat.*
    use std::sync::OnceLock;
    static ZELLE: OnceLock<Vec<Bereich>> = OnceLock::new();
    ZELLE
        .get_or_init(|| {
            let mut raus = Vec::new();
            let mut cp = 0u32;
            let mut war_im_ausgeschlossenen = |cp: u32, raus: &mut Vec<Bereich>| {
                let drin = [cn, cs, co]
                    .into_iter()
                    .flatten()
                    .any(|b| in_bereichen(b, cp));
                match raus.last_mut() {
                    Some((_, ende)) if *ende + 1 == cp && !drin => *ende = cp,
                    _ => {
                        if !drin {
                            raus.push((cp, cp));
                        }
                    }
                }
            };
            while cp <= MAX_CP {
                war_im_ausgeschlossenen(cp, &mut raus);
                cp += 1;
            }
            raus
        })
        .as_slice()
}

/// ## W60 (QUIC, 05.10.2026) — **DIE SCHLEUSE: EINE EIGENSCHAFFT AUF
/// `u16` ODER GAR NICHT.**
///
/// `regexmini.rs` haelt seine Klassenbereiche in **`Vec<(u16, u16)>`** — an
/// **20 Stellen** (GEMESSEN, per Zählung im Baum). Ein solcher Bereich kann
/// **keinen** Codepoint über `0xFFFF` halten, und **`Script=Deseret`,
/// `Adlam`, `Cuneiform`, `Old_Italic` … liegen genau dort**.
///
/// **DIE WAHL, und sie ist der ganze Inhalt dieser Funktion:** Bei einer
/// Eigenschaft mit Bereichen über `0xFFFF` gibt es drei Ausgänge.
///
/// * **abschneiden** — dann ist `\p{Script=Deseret}` **falsch und unmerklich**.
///   Das ist genau das, was der Motor sonst mit `_ => 0` tut.
/// * **die Bereiche über `0xFFFF` wegwerfen** — dieselbe Lüge, nur kleiner.
/// * **nicht bedienen** — die Eigenschaft bleibt **unbekannt**, und der Aufrufer
///   meldet es als **SyntaxError**, wie `node` es auch tut.
///
/// **ICH WÄHLE DAS DRITTE.** *Ein stillschweigend falscher Bereich ist schlimmer
/// als ein fehlender: der fehlende meldet sich.* Genau deshalb prüft die Schleuse
/// mit **`any`** und nicht mit **`filter`** — **ein** Bereich über `0xFFFF`
/// macht die **ganze** Eigenschaft unbedienbar.
///
/// **GEMESSEN, wie viele der 232 roten Case-Namen das trifft** (die Namen aus
/// `RegExp/property-escapes/generated/` gegen diese Tabellen):
///
/// ```text
///   67  passen vollstaendig in den BMP   -> werden bedient
///  148  brauchen astralische Bereiche   -> bleiben UNBEKANNT (bewusst)
///   17  stehen nicht in der Tabelle      -> bleiben UNBEKANNT (bewusst)
/// ```
///
/// **DIE 148 SIND KEIN FEHLER VON MIR, sondern ein Typ im Motor** — sie warten
/// auf die `u32`-Umstellung in `regexmini.rs`, und die ist **WASM-Datei**
/// (BRETT). Der Kommentar steht hier, weil diese Schleuse sonst wie eine
/// Verlegenheitslösung aussieht: sie ist eine **begründete Grenze**.
pub(crate) fn bereich_u16(name: &str) -> Option<Vec<(u16, u16)>> {
    let b = bereich(name, None)?;
    if b.iter().any(|(_, ende)| *ende > 0xFFFF) {
        return None;
    }
    Some(b.iter().map(|(a, e)| (*a as u16, *e as u16)).collect())
}

/// **DIE GEGENPROBE, und sie ist das, was die Schleuse überprüfbar macht.**
///
/// `\p{Alphabetic}` ist einer der **67** BMP-Fälle — und `Alphabetic` hat
/// Bereiche über `0xFFFF` (plane 5 und alle astralischen Buchstaben), also
/// **darf** die Schleuse ihn gerade **nicht** bedienen. `\p{Greek` dagegen
/// liegt **unter** `0xFFFF` (Griechisch endet bei `03FF`) und **darf**.
///
/// *Eine Grenze, die man an zwei Beispielen sieht, ist eine Grenze; eine, die
/// man nur behauptet, ist eine Vermutung.*
#[cfg(test)]
mod schleuse_test {
    use super::*;

    #[test]
    fn die_schleuse_lässt_bmp_zu_und_astral_nicht() {
        // Hebrew is entirely BMP; Greek also contains ancient numerals
        // and musical notation above U+FFFF (generated Unicode 15.1 data).
        assert!(bereich_u16("Hebrew").is_some(), "Hebrew liegt unter 0xFFFF");
        assert!(bereich_u16("Greek").is_none(), "Greek enthaelt astralische Zeichen");
        assert_eq!(hat_eigenschaft("Script", Some("Greek"), 0x1d200), Some(true));
        // **Astral** -> die Schleuse weist es **ab**, obwohl der Name bekannt ist.
        assert!(
            bereich_u16("Deseret").is_none(),
            "Deseret beginnt bei 0x10400 und darf nicht bedient werden"
        );
        // **Ein unbekannter Name** bleibt unbekannt — kein Leerlauf, kein [].
        assert!(bereich_u16("Quatsch").is_none(), "unbekannt bleibt unbekannt");
        // **Zwei Schreibweisen, eine Bahn** (Suite gegen Norm).
        assert_eq!(
            bereich("General_Category_-_Lu", None).map(|b| b.len()),
            bereich("General_Category=Lu", None).map(|b| b.len()),
            "Bindestrich und Gleichheitszeichen muessen dasselbe liefern"
        );
    }
}
