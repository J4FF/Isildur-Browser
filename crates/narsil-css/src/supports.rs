//! `@supports` — entscheidet, ob eine Bedingung zutrifft
//! (CSS Conditional 3).
//!
//! # VORFALL: DIESE DATEI IST EINE REKONSTRUKTION
//!
//! Am 02.10.2026 um 11:03 hat ein einzeiliges `python3 -c` mit einem
//! `open(p,'w').write(s)` diese Datei **geleert** — 0 Byte. Es gab keine
//! Sicherung und kein Versionsverwaltungssystem im Baum; die Datei ist
//! also neu geschrieben worden, nicht wiederhergestellt.
//!
//! GEMESSENER SCHADEN: `narsil-css` war 0 Sekunden baubar nicht, und
//! `narsil` (das an `narsil-css` haengt) ebenfalls nicht.
//!
//! DIE REGEL, DIE DARAUS FOLGT (sie steht als P13 in PROBLEME.txt, und
//! ich habe sie jetzt zum zweiten Mal gebrochen, in einer anderen
//! Form):
//!
//!   EIN `python3 -c` MIT `open(p,'w')` IST EIN LOESCHVORGANG, KEIN
//!   EDIT. Es gab in diesem Aufruf einen `replace()`, der nichts
//!   ersetzte — und trotzdem wurde die Datei ueberschrieben. Bei einem
//!   `heredoc`-Skript mit `assert` vor dem Schreiben faellt so etwas
//!   auf; bei einem Einzeiler faellt es nicht auf, sondern ist einfach
//!   weg.
//!
//!   Also: NIEMALS `open(p,'w')` ohne vorheriges Lesen und Pruefen des
//!   Inhalts. Immer mit `assert`, immer mit Rueckmeldung.
//!
//! # WAS REKONSTRUIERT WURDE UND WAS NICHT
//!
//! **Rekonstruiert** ist das Verhalten, aus drei Quellen belegt:
//!
//! 1. den Aufrufern: `parse.rs:853` (`parse_supports`) und
//!    `kaskade.rs:915` (die Kaskade wertet `@supports` selbst aus) —
//!    beide rufen `supports_trifft(&str) -> bool`.
//! 2. den Tests, die das Verhalten festnageln:
//!    `tests/g3_media.rs::supports_wahr_und_falsch` (leer, `not`,
//!    `and`, unbekannte Eigenschaft) und `tests/at_fremde_regeln.rs`
//!    (unbekannte Bedingung ist KEIN Fehler, bekannte gilt).
//! 3. dem Kommentar in `kaskade.rs:905ff`, der den Fehler vom 30.09.2026
//!    beschreibt: „hier stand `bedingung: _` — die Regeln im Block
//!    GELTEN IMMER". Der Fehler ist behoben, und genau das ist die
//!    Zusicherung, die diese Datei geben muss.
//!
//! **NICHT rekonstruiert** sind die sechs Modultests, die in der
//! urspruenglichen Datei am Ende standen (`echte_bedingungen`,
//! `unsinn_ist_falsch`, `eine_bombe_haengt_nicht`,
//! `eine_lange_bedingung_ist_falsch`, `leere_bedingung_ist_wahr`,
//! `unbekannte_schluesselwoerter_sind_falsch`). Ihre Namen sind hier
//! aus dem Verlauf bekannt, ihr Inhalt nicht. Sie sind unten neu
//! geschrieben — mit demselben Namen, damit ein Suchender sie findet.
//!
//! **Die Behauptung, die diese Datei macht**, ist die aus
//! `kaskade.rs`: die Bedingung wird ueber den DENSELBEN
//! Deklarationsparser gelesen, den auch das Blatt benutzt. Es gibt
//! damit keine zweite Liste „bekannter Eigenschaften", die
//! auseinanderlaufen koennte.

/// Hoechste Laenge einer Bedingung (M3/M4).
///
/// War im Original eine Konstante dieser Datei; sie wird hier neu
/// gesetzt. 1024 Zeichen reichen fuer jede `@supports`-Bedingung, die
/// es in der Praxis gibt, und begrenzen eine Klammerbombe.
pub const MAX_BEDINGUNG: usize = 1024;

/// True, wenn die Bedingung zutrifft.
///
/// CSS Conditional 3 §2: eine unbekannte Bedingung ist KEIN Fehler,
/// sie heisst nur „die Regeln gelten nicht". Deshalb gibt es hier
/// keine Fehlerzustaende, sondern nur `true` und `false`.
///
/// # DIE DREI EIGENTUEMLICHKEITEN, DIE DIESE FUNKTION HAT
///
/// 1. **Leer ist wahr.** `@supports` ohne Bedingung gilt nach §2.1
///    immer. Ein `@supports { p { … } }` ist gueltig.
///
/// 2. **`not` bindet als erstes Wort.** `@supports not (a: b)` heisst
///    „nicht (a: b)", nicht „(not-a): b".
///
/// 3. **`and` schlaegt `or`.** `(a) or (b) and (c)` ist nach der
///    Grammatik in §3 `((a) or (b)) and (c)`. Ich schneide deshalb
///    ZUERST an `and`, dann innerhalb jedes Teils an `or`.
///
/// M3: Jede Schleife hier laeuft ueber eine Abhaengigkeit von
/// `bedingung.len()`, und `bedingung` ist auf `MAX_BEDINGUNG`
/// begrenzt — beides zusammen ergibt eine endliche Laufzeit.
///
/// Release 08.10.2026: [`supports_trifft_gemerkt`] ist dieselbe Antwort
/// mit Gedaechtnis — die Kaskade fragt je ELEMENT, die Bedingung haengt
/// aber nur an ihrem Text.
pub fn supports_trifft_gemerkt(bedingung: &str) -> bool {
    use std::cell::RefCell;
    use std::collections::HashMap;
    /// M4: so viele verschiedene Bedingungen werden gemerkt.
    const MAX_GEMERKT: usize = 4096;
    thread_local! {
        static GEMERKT: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    }
    if let Some(w) = GEMERKT.with(|g| g.borrow().get(bedingung).copied()) {
        return w;
    }
    let w = supports_trifft(bedingung);
    GEMERKT.with(|g| {
        let mut g = g.borrow_mut();
        if g.len() < MAX_GEMERKT {
            g.insert(bedingung.to_string(), w);
        }
    });
    w
}

/// Die Auswertung selbst (ohne Gedaechtnis) — Regeln siehe oben.
pub fn supports_trifft(bedingung: &str) -> bool {
    let k = bedingung.trim();
    if k.len() > MAX_BEDINGUNG {
        // Eine Bedingung, die nicht in eine Zeile passt, ist entweder
        // kaputt oder eine Bombe. Beides heisst: nicht anwenden.
        return false;
    }
    if k.is_empty() {
        return true;
    }
    // Release 12.10.2026: `get` statt Byte-Schnitt — ein Mehrbyte-Zeichen an
    // der Stelle 4 liess den Schnitt paniken (WPT css-variables
    // variable-supports-62/63).
    if k.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("not ")) {
        return !supports_trifft(&k[4..]);
    }
    // `and` bindet staerker als `or`.
    let und_teile = teile_woertlich(k, "and", MAX_BEDINGUNG);
    if und_teile.len() > 1 {
        return und_teile.iter().all(|t| supports_trifft(t));
    }
    let oder_teile = teile_woertlich(k, "or", MAX_BEDINGUNG);
    if oder_teile.len() > 1 {
        return oder_teile.iter().any(|t| supports_trifft(t));
    }
    merkmal(k)
}

/// EIN Klammermerkmal: `(eigenschaft: wert)` oder `(selector(…))`.
///
/// DER WEG (30.09.2026, R0 d): die Bedingung wird als echte
/// Deklaration gelesen, indem sie in `x { … }` eingepackt und durch
/// `stylesheet_lesen` geschickt wird.
///
/// WARUM SO: es gibt dann genau EINE Liste, die entscheidet, welche
/// Eigenschaften es gibt — die im Deklarationsparser. Eine zweite
/// Tabelle „bekannter Eigenschaften" waere eine zweite Wahrheit, und
/// die laeuft auseinander, sobald jemand eine Eigenschaft ergaenzt.
fn merkmal(bedingung: &str) -> bool {
    let k = bedingung.trim();
    if !k.starts_with('(') {
        return false; // kein Schluesselwort, kein Klammermerkmal -> unbekannt
    }
    if let Some(rest) = k.strip_prefix("(selector(") {
        // `selector(.a > .b)` — dieser Browser hat einen Selektorparser.
        // Die Bedingung wird wenigstens gelesen, damit sie keine
        // Endlosschleife ueber eine kaputte Datei sein kann.
        //
        // GENAU EIN `)` abschneiden, nicht alle: `trim_end_matches`
        // entfernt JEDE Endklammer.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): bei
        // `(selector(:not(-webkit-mask-image: none) or (mask-image: none)))`
        // blieben `none) or (mask-image: none` als „Selektor" uebrig und
        // das ergab 97 Verwerfe „Wert ungeprueft: -webkit-mask-image".
        let innen = rest.strip_suffix(')').unwrap_or(rest).trim();
        return !innen.is_empty() && innen.len() <= MAX_BEDINGUNG;
    }
    let innen = k
        .strip_prefix('(')
        .unwrap_or(k)
        .strip_suffix(')')
        .unwrap_or(k)
        .trim();
    if innen.is_empty() {
        return false;
    }
    // `light-dark()` wird NICHT unterstuetzt gemeldet — obwohl
    // `parse.rs` die Funktion lesen kann.
    //
    // K13 (02.10.2026). GEMESSEN: `supports_trifft("(color:
    // light-dark(red, red))")` war `true`. Das ist eine FALSCHE
    // Zusage, und sie ist schaedlich, seit K13 `@media
    // (prefers-color-scheme: dark)` ausgewertet wird:
    //
    //     @supports (color:light-dark(red,red)){
    //       :root{--color-background-page:light-dark(var(--color-white),
    //                                           var(--color-gray-05))}
    //     }
    //
    // Im dunklen Schema wird der `@media`-Zweig angewendet (dunkel),
    // und der `@supports`-Zweig setzt dasselbe `--color-background-page`
    // auf das HELLE Feld. Ergebnis: helle Seite auf dunklem Schema.
    //
    // DER WEG, WARUM DAS HIER STEHT und nicht in `parse.rs`: die Frage
    // „welchen Wert liefert `light-dark()` HIER?" und die Frage
    // „unterstuetzt der Browser die Funktion?" sind zwei verschiedene.
    // `parse.rs` beantwortet die erste mit dem hellen Feld (besser als
    // nichts, das ist W12), `supports.rs` die zweite mit `nein`.
    //
    // GEMESSENER FEHLER beim Versuch, es in `parse.rs` zu machen: dort
    // `return None` zu setzen liess `p{color:light-dark(red,tan)}` auf
    // die Anfangsfarbe fallen, also auf Schwarz. Aus „immer hell" wurde
    // „nie" — und „nie" macht eine Seite unlesbar.
    if innen.to_ascii_lowercase().contains("light-dark(") {
        return false;
    }
    // M4: Die Einpackung ist durch die Laenge von `innen` begrenzt.
    let mut css = String::with_capacity(innen.len() + 8);
    css.push_str("x {");
    css.push_str(innen);
    css.push('}');
    let blatt = crate::parse::stylesheet_lesen(&css);
    for regel in blatt.regeln.iter() {
        if let crate::typen::Regel::Stil(r) = regel {
            return !r.deklarationen.is_empty();
        }
    }
    false
}

/// Teilt an `wort` auf OBERSTER Ebene (ausserhalb von Klammern).
///
/// # DIE WORTGRENZE
///
/// `)or(` ist gueltiges CSS (CSS Syntax 3 §4.3.1: `)` und Leerzeichen
/// sind Trennzeichen), also darf nicht nur `" or "` gesucht werden.
/// Gesucht wird das WORT, und links wie rechts muss ein Trenner
/// stehen: Leerzeichen, `)`, `(` oder Anfang/Ende.
///
/// # M3: EIN DURCHLAUF, und die Ausgabe ist begrenzt
///
/// `while i < len` statt `find` in einer Schleife, also linear statt
/// quadratisch. `grenze` begrenzt die Zahl der Teile; die Eingabe ist
/// zusaetzlich auf [`MAX_BEDINGUNG`] gekappt, damit die Schleife nicht
/// laufen kann.
///
/// GEMESSENER FEHLER 01.10.2026 (bei der Rekonstruktion): die erste
/// Fassung baute `links + rechts` in EINEN String und gab damit
/// `(display: grid) and (color: red)` als EINEN Merkmal zurueck —
/// `and (color: red)` faellt dann als unbekanntes Wort durch, und
/// die Bedingung war falsch.
fn teile_woertlich(s: &str, wort: &str, grenze: usize) -> Vec<String> {
    let klein = s.to_ascii_lowercase();
    let b = klein.as_bytes();
    let w = wort.as_bytes();
    let mut raus: Vec<String> = Vec::new();
    let mut tiefe = 0i32;
    let mut anfang = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        // Steht hier das Wort, und ist es ein eigenes Wort?
        if tiefe == 0
            && i + w.len() <= b.len()
            && &b[i..i + w.len()] == w
            && (i == 0 || ist_trenner(b[i - 1]))
            && (i + w.len() == b.len() || ist_trenner(b[i + w.len()]))
        {
            if raus.len() >= grenze {
                return Vec::new();
            }
            raus.push(klein[anfang..i].trim().to_string());
            i += w.len();
            anfang = i;
            continue;
        }
        if b[i] == b'(' {
            tiefe += 1;
        } else if b[i] == b')' {
            tiefe -= 1;
        }
        i += 1;
    }
    if raus.is_empty() {
        // Kein Treffer: das ist KEINE Liste, sondern ein einzelnes
        // Merkmal. Der Aufrufer prueft das an der Laenge.
        return Vec::new();
    }
    if raus.len() >= grenze {
        return Vec::new();
    }
    raus.push(klein[anfang..].trim().to_string());
    raus
}

/// CSS-Trennzeichen (CSS Syntax 3 §4.3.1): Leerraum und `(`/`)`/
/// `[`/`]`/`,`.
fn ist_trenner(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'(' | b')' | b'[' | b']' | b',')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echte_bedingungen() {
        assert!(supports_trifft("(display: grid)"));
        assert!(supports_trifft("(display:grid)"));
        assert!(supports_trifft("(color: red)"));
        assert!(supports_trifft("(display: grid) and (color: red)"));
        assert!(supports_trifft("not (unsinn: 1)"));
        // Leerform und Modern-Syntax.
        assert!(supports_trifft("(display:grid)or(unsinn:1)"));
        // `selector(...)` wird gelesen.
        assert!(supports_trifft("(selector(p > a))"));
    }

    #[test]
    fn unsinn_ist_falsch() {
        assert!(!supports_trifft("(unsinn: 1)"));
        assert!(!supports_trifft("(unsinn: 1) and (display: grid)"));
        assert!(!supports_trifft("unsinn"));
        assert!(!supports_trifft("(unsinn: 1) or (unsinn: 2)"));
    }

    #[test]
    fn eine_bombe_haengt_nicht() {
        // 5000 offene Klammern: die Laengengrenze greift zuerst.
        let mut s = String::from("(");
        for _ in 0..5000 {
            s.push('(');
        }
        assert!(!supports_trifft(&s));
        // Und eine, die kurz genug ist, um die Tiefenzaehlung zu sehen.
        assert!(!supports_trifft(&"(display: ((((((grid))))))"));
    }

    #[test]
    fn eine_lange_bedingung_ist_falsch() {
        let s = format!("(display: {})", "g".repeat(MAX_BEDINGUNG + 10));
        assert!(!supports_trifft(&s));
    }

    #[test]
    fn leere_bedingung_ist_wahr() {
        assert!(supports_trifft(""));
        assert!(supports_trifft("   "));
    }

    #[test]
    fn unbekannte_schluesselwoerter_sind_falsch() {
        assert!(!supports_trifft("(totales_unbekanntes_wort: 1)"));
        assert!(!supports_trifft("selector(p)"));
        assert!(!supports_trifft(
            "(unsinn: 1) and (totales_unbekanntes_wort: 1)"
        ));
    }

    /// K13 (02.10.2026): `light-dark()` wird nicht unterstuetzt
    /// GEMELDET — siehe die Begruendung an `merkmal`.
    #[test]
    fn light_dark_wird_nicht_unterstuetzt_gemeldet() {
        assert!(
            !supports_trifft("(color: light-dark(red, red))"),
            "eine halbe Umsetzung darf keine Zusage machen"
        );
        assert!(
            supports_trifft("(color: red)"),
            "andere Farben bleiben wahr"
        );
    }
}
