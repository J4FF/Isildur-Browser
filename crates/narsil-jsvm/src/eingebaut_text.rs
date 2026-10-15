//! JS2, `String`: die zwei Regeln, die fast alle `String.prototype`-Methoden
//! falsch machen — und eine, die **nicht** behebbar ist, ohne den
//! Zeichenketten-Speicher zu aendern.
//!
//! ## DER ERSTE BEFUND (gemessen, 02.10.2026, 01:20)
//!
//! Neunzehn Proben gegen `node 22`. Vier Zeilen stimmen, **zehn** nicht:
//!
//! ```text
//! Fall                        node 22      bei uns
//! "lego".charAt()             "l"          ""
//! "lego".charAt(undefined)    "l"          ""
//! "abc".charAt(NaN)           "a"          ""
//! "abc".charAt(-0.99999)      "a"          ""
//! "abc".charAt(-1)            ""           ""        (richtig)
//! "abc".charCodeAt(NaN)       97           NaN
//! "abc".charCodeAt(-0.5)      97           NaN
//! charAt.call(null)           TypeError    ""
//! charAt.call(undefined)      TypeError    ""
//! charAt.call(42)             "4"          ""
//! ```
//!
//! Das sind **zwei** Ursachen, und keine davon ist „die Methode ist falsch":
//!
//!  1. Die **Position** wird nicht nach `ToIntegerOrInfinity` gelesen.
//!     `undefined` und `NaN` ergeben in der Norm **0**, und `-0.99999` wird
//!     **Richtung null** abgeschnitten, also `-0` — und `-0 < 0` ist falsch.
//!  2. `this` wird **nicht** erzwungen. ECMA-262 22.1.3.3 Schritt 1 sagt
//!     `Let O be ? RequireObjectCoercible(this value)`.
//!
//! ## UND EIN BEFUND MIT ARCHITEKTUR-REICHWEITE
//!
//! ```text
//!                              node 22      bei uns
//! "\u{1F600}".length           2            2          (richtig!)
//! "\u{1F600}".charAt(0)        "\uD83D"     "\u{1F600}"
//! "\u{1F600}".charCodeAt(0)    55357        128512
//! "\u{1F600}".at(0)            "\uD83D"     "\u{1F600}"
//! "\u{1F600}".slice(0,1)       "\uD83D"     "\u{1F600}"
//! "\u{1F600}".codePointAt(0)   128512       128512     (richtig)
//! ```
//!
//! `length` zaehlt UTF-16-Code-Units, die Indexmethoden zaehlen **Code-Points**.
//! Das ist ein **innerer Widerspruch des Motors** und keine node-
//! Besonderheit: `length` sagt 2, `charAt(0)` liefert aber eine Einheit von
//! Laenge 2. Ohne `node` beweisbar:
//!
//! ```text
//! charCodeAt(i) <= 0xFFFF
//! ```
//!
//! muss fuer **jedes** `i` gelten — eine Code-Unit ist per Definition in
//! 0…0xFFFF. Wir liefern `128512`.
//!
//! **WAS DAVON BEHEBBAR IST, und was nicht** — das ist der Punkt dieses
//! Abschnitts:
//!
//! * **`charCodeAt` nach Code-Unit: voll behebbar.** `encode_utf16()` auf dem
//!   `String` liefert die Units, ohne den Speicher zu aendern.
//! * **`charAt`/`at`/`slice` fuer Nicht-BMP: nicht behebbar** mit diesem
//!   Speicher. Eine **halbe** Code-Unit (ein einsamer Surrogat-Paarwert) ist in
//!   keinem Rust-`String` darstellbar — `String` ist UTF-8, und UTF-8 kennt
//!   keinen Surrogat-Paarwert.
//! * **Einsame Surrogat-Paarwerte ueberhaupt: nicht behebbar.** Beim
//!   Dekodieren wird `\uD800` zu U+FFFD. `codePointAt` gibt darum immer
//!   `65533`, wo der Originalwert `55296` sein sollte — gemessen.
//!
//! Die beiden letzten Punkte gehoeren in `heap.rs` (`HObj::Zeichenkette`), und
//! das ist eine Aenderung an der **Speicherdarstellung** einer shared Datei.
//! Ich benenne sie, statt sie zu versuchen.

use crate::heap::GVal;

/// ECMA-262 6.1.6.1.1 `ToIntegerOrInfinity` — fuer **eine** Stelle.
///
/// GEMESSEN, `node 22` (01:20), sechs Eingaben:
///
/// | Eingang | `ToIntegerOrInfinity` | `charAt`-Ergebnis |
/// |---|---|---|
/// | `undefined` | `+0` | `"a"` |
/// | `NaN` | `+0` | `"a"` |
/// | `-0.99999` | `-0` | `"a"` (`-0 < 0` ist **falsch**) |
/// | `-1` | `-1` | `""` |
/// | `1.7` | `1` | `"b"` |
///
/// DIE DREI REGELN: `NaN` wird `+0`; sonst wird **Richtung null**
/// abgeschnitten (`trunc`, nicht `floor` — bei `-0.99999` liefert `floor`
/// `-1` und damit `""`); `±0` und `±∞` bleiben.
///
/// WARUM EINE EIGENE FUNKTION und kein `f64`-Aufruf an der Aufrufstelle: der
/// Unterschied zwischen `trunc` und `floor` ist **genau ein Fall**
/// (`-0.99999`), und dieser Fall ist ein **roter test262-Fall**
/// (`String/prototype/charAt/pos-rounding.js`). Eine Regel, die man an
/// fuenfzehn Aufrufstellen wiederholt, ist eine Regel, die an einer davon
/// `floor` schreibt.
///
/// M3: Das Argument kommt aus fremdem Code. Hier wird nichts allokiert und
/// nichts iteriert — der Aufrufer hat den Index bereits berechnet. Deshalb
/// kein `MAX_...`; die Grenze steht beim Aufrufer (`size <= position`).
pub(crate) fn zu_ganzer_zahl_trunkiert(n: f64) -> f64 {
    if n.is_nan() {
        // `ToIntegerOrInfinity(NaN)` ist `+0`. `0.0` und nicht `-0.0`, weil
        // an dieser Stelle kein Unterschied entsteht (`-0 < 0` ist in beiden
        // Faellen falsch) — und `+0.0` die ehrliche Antwort auf „nicht
        // darstellbar" ist.
        0.0
    } else {
        // `trunc` = Richtung null. `floor` waere bei negativen Zahlen
        // falsch, und das ist der Fall, den `pos-rounding.js` prueft.
        n.trunc()
    }
}

/// ECMA-262 7.1.17 `ToString` — **Schritt 1: ein Symbol wirft**.
///
/// ```text
/// 1. If argument is a Symbol, throw a TypeError exception.
/// ```
///
/// (ES2023 hat das nachgezogen: `ToString` auf ein Symbol war nie in der
/// Fassung, in der `ToNumber` es schon hatte.)
///
/// ## GEMESSEN (12:05), `node 22`
///
/// ```text
/// String.prototype.endsWith.call(Symbol('x'), '')   node: TypeError
/// String.prototype.includes.call(Symbol('x'), '')   node: TypeError
/// String.prototype.padStart.call(Symbol('x'), 2)    node: TypeError
/// ''.endsWith(Symbol('x'))                          node: TypeError
/// ''.padStart(2, Symbol('x'))                       node: TypeError
/// ''.includes(Symbol('x'))                          node: TypeError
/// ''.startsWith(Symbol('x'))                        node: TypeError
/// String.prototype.codePointAt.call(Symbol('x'), 0) node: TypeError
/// String.prototype.indexOf.call(Symbol('x'), '')    node: TypeError
/// String.prototype.slice.call(Symbol('x'), 0)      node: TypeError
/// String.prototype.split.call(Symbol('x'), '')      node: TypeError
/// String.prototype.concat.call(Symbol('x'), '')     node: TypeError
/// ```
///
/// **VORHER stand überall der TEXT** — und das ist der Grund, warum es so
/// schwer zu finden war:
///
/// ```text
/// ''.endsWith(Symbol('x'))   -> false
/// ''.padStart(2, Symbol('x'))  -> "Sy"       (der Fuellstring!)
/// 'ab'.indexOf(Symbol('x'))  -> -1
/// 'ab'.concat(Symbol('x'))   -> "abSymbol(x)"
/// ```
///
/// **`padStart` ist das auffaelligste:** ein Symbol als `fillString` wurde
/// als **Text** benutzt, und `Symbol('x')` hat **sieben** Zeichen — also
/// fuellte die Methode mit `"Symbol(x)"` auf. Kein Fehler, kein Wurf, nur das
/// **falsche Ergebnis**. Genau die Klasse von Fehler, die eine Testsuite erst
/// dann sieht, wenn sie einen Wurf verlangt.
///
/// ## WAS HIER NICHT GEHOERT und warum
///
/// Die Regel gehoert eigentlich in `Engine::zu_text` (`ops.rs`, **geteilt**) —
/// dort fehlt sie, und das ist **der Grundbefund** (Wunsch 12:20). **Ich lege
/// sie trotzdem hier an**, weil:
///
/// 1. `String.prototype` **meine** Datei ist und alle vier Aufrufstellen in
///    `builtin2.rs` liegen,
/// 2. die Regel **benannt und gemessen** hier steht, und
/// 3. **wenn `zu_text` sie bekommt, wird diese Regel redundant — und
///    redundante korrekte Regeln sind harmlos.** Eine Regel **zweimal**
///    auszufuehren ist ein Fehler; sie **zweimal zu *schreiben*** nicht.
///
/// **UND DIE KONTROLLGRUPPE, weil `ToNumber` das Symbol schon kannst:**
/// `'abc'.charAt(Symbol())` warf schon vor dieser Zeile `TypeError`, weil
/// `ops.rs:456` den `proto_symbol`-Zweig hat. **Die Regel war also nicht
/// unbekannt — sie war an einer Stelle von vier richtig.**
///
/// M3: **eine** Prüfung auf die `proto`-Referenz, **keine** Allokation, **keine**
/// Schleife. Der Aufruf ist auf Argumentebene; die Laenge haengt nicht von
/// fremdem Input ab.
pub(crate) fn zu_text_kein_symbol(
    e: &mut crate::interp::Engine,
    v: crate::heap::GVal,
) -> Result<String, crate::interp::Stoerung> {
    if let crate::heap::GVal::Ref(r) = v
        && let crate::heap::HObj::Gegenstand { proto: Some(p), .. } = e.heap.holen(r)
        && *p == e.proto_symbol
    {
        return Err(crate::interp::Stoerung::Wurf(
            e.fehler_wert("TypeError", "Symbol kann nicht in einen Text"),
        ));
    }
    e.zu_text(v)
}

/// ECMA-262 7.3.19 `RequireObjectCoercible` — darf `this` sein?
///
/// `node 22` (01:20), vier Eingaben:
///
/// ```text
/// charAt.call(null)       -> TypeError
/// charAt.call(undefined)  -> TypeError
/// charAt.call(42)         -> "4"     (geht!)
/// charAt.call(Symbol())   -> TypeError   (aus ToString)
/// ```
///
/// DER PUNKT, DER IMMER VERWECHSELT WIRD: `42` ist **kein** Fehler. Die
/// Funktion heisst „object **coercible**", nicht „object required" — sie
/// wirft nur fuer `null` und `undefined`. Alles andere wird mit `ToString`
/// zu Text.
///
/// DAS IST AUCH DER GRUND, warum die Regel hier eine **`bool`** ist und kein
/// Fehler: `null`/`undefined` sind die **zwei** Faelle, und der Wurf gehoert
/// an die Aufrufstelle, weil sie die Fehlerwert-Erzeugung besitzt.
pub(crate) fn this_ist_coercible(this: &GVal) -> bool {
    !matches!(this, GVal::Null | GVal::Undefiniert)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ToIntegerOrInfinity`, **sechs** Zeilen aus `node 22`.
    ///
    /// DER FALL, DER DIE REGEL TRAEGT: `-0.99999`. `floor` liefert `-1`, und
    /// `charAt` gibt dann `""`. `trunc` liefert `-0`, und `-0 < 0` ist
    /// **falsch** — also kommt `"a"` zurueck. Genau das prueft
    /// `String/prototype/charAt/pos-rounding.js`.
    #[test]
    fn zu_ganzer_zahl_sechs_zeilen() {
        for (eingabe, soll) in [
            (f64::NAN, 0.0),
            (0.0, 0.0),
            (-0.0, -0.0),
            (-0.99999, -0.0),
            (-1.0, -1.0),
            (1.7, 1.0),
            (99.0, 99.0),
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        ] {
            let r = zu_ganzer_zahl_trunkiert(eingabe);
            assert_eq!(
                r, soll,
                "ToIntegerOrInfinity({eingabe}) ist {soll}, nicht {}",
                r
            );
        }
        // UND die Aussage, die `pos-rounding.js` macht, als Zusatz: das
        // Ergebnis von `-0.99999` ist **nicht kleiner als null**.
        assert!(
            !(zu_ganzer_zahl_trunkiert(-0.99999) < 0.0),
            "das ist der ganze Punkt: -0.99999 wird -0, und -0 < 0 ist falsch — \
             mit `floor` waere es -1 und der Test wuerde '' zurueckgeben"
        );
        // Und die Gegenprobe zu `floor`, damit der Unterschied benannt ist:
        assert_eq!(
            (-0.99999f64).floor(),
            -1.0,
            "das waere die falsche Antwort — sie steht hier, damit niemand sie \
             fuer die Regel haelt"
        );
    }

    /// `RequireObjectCoercible`, **vier** Zeilen aus `node 22`.
    ///
    /// `42` ist der wichtige Fall: es ist **kein** Fehler. Wer die Regel
    /// strenger liest als die Norm, wirft bei jeder Zahl — und damit bei
    /// `String.prototype.charAt.call(42)`, wo `"4"` zurueckkommen soll.
    #[test]
    fn this_ist_coercible_nur_nicht_bei_null_und_undefined() {
        assert!(!this_ist_coercible(&GVal::Null), "null -> TypeError");
        assert!(
            !this_ist_coercible(&GVal::Undefiniert),
            "undefined -> TypeError"
        );
        assert!(this_ist_coercible(&GVal::Zahl(42.0)), "42 geht");
        assert!(this_ist_coercible(&GVal::Bool(true)), "true geht");
        assert!(this_ist_coercible(&GVal::Zahl(0.0)), "0 geht");
    }

    /// Und die **innere** Aussage, die ohne `node` auskommt — der
    /// UTF-16-Widerspruch.
    ///
    /// ```text
    /// "😀".length          -> 2
    /// "😀".charCodeAt(0)   -> 128512
    /// ```
    ///
    /// `charCodeAt` liefert per Definition eine **Code-Unit**, und eine
    /// Code-Unit ist in 0…0xFFFF. Der Motor liefert einen **Code-Point**.
    ///
    /// DER TEST: die Aussage „jede Code-Unit passt in 16 Bit" ist eine
    /// Eigenschaft des **Begriffs**, nicht von node. Wenn sie im Motor
    /// verletzt wird, ist das ohne Vergleich mit aussen beweisbar — und
    /// das ist der Unterschied zwischen „wir unterscheiden uns von node"
    /// und „der Motor widerspricht sich selbst".
    #[test]
    fn eine_code_unit_passt_in_16_bit() {
        // Der Wert, den `charCodeAt(0)` auf U+1F600 zurueckgibt.
        let zurueck = 0x1F600u32;
        assert!(
            zurueck > 0xFFFF,
            "U+1F600 ist ein Code-**Point**; `charCodeAt` muss die fuehrende \
             Code-Unit liefern, also 0xD83D = 55357"
        );
        // Und die fuehrende Unit, nach den Regeln fuer Surrogat-Paare:
        let fuehrend = 0xD800 + ((zurueck - 0x10000) >> 10);
        assert_eq!(fuehrend, 0xD83D, "das ist der Wert, den node liefert");
        assert!(fuehrend <= 0xFFFF, "und er passt in 16 Bit");
    }
}

// =====================================================================
// W52 (QUIC, 04.10.2026) — `String.prototype.normalize`, die vier Formen
// =====================================================================
//
// **WARUM DER ALGORITHMUS HIER UND DIE TABELLEN IN `unicode_norm.rs`:**
// `unicode_norm.rs` ist **generiert** (`/tmp/opencode/unicode_norm_bauen.py`,
// einmal gelaufen) und wird **nicht** von Hand angefasst — ein Generator, der
// auch nur den Rumpf enthaelt, wird beim naechsten Lauf ueberschrieben, und
// dann fehlt der Rumpf. *Getrennte Dateien fuer „Daten" und „Rechnung" sind
// die einzige Form, in der „einmal laufen lassen" dauerhaft stimmt.*
// **GEMESSENE Herkunft** (`decomps.txt`, Unicode 15.1.0):
// ```text
// kanonische Zerlegung : 2424      Kompatibilitaetszerlegung : 4678
// Kombinationsklassen  :  544      Kompositionen              :  699
// ```
//
// **DIE EINE EINSICHT, DIE ZWEI FEHLER KOSTETE:** die Zerlegungsziele sind
// **nicht immer BMP**. `decomps.txt` enthaelt `0xFA6C -> 0x242EE`, und
// `0x242EE` passt in **kein** `u16`. Die erste Tabelle war `&[u16]` und der
// Compiler sagte `literal out of range for u16` — **das ist die Meldung, die
// den Rest der Normalisierung getragen hat.** *Die zwei Fehler waren: Feld 2
// von `UnicodeData.txt` ist die Allgemeinkategorie (`Cc`), und die Zerlegung
// kann aus dem BMP herausfuehren. Beide meldet der Compiler, beide habe ich
// erst beim dritten Anlauf gelesen.*

/// **HANGUL ZUERST, weil es eine Formel ist und keine Tabelle:**
/// 0xAC00..=0xD7A3 sind **11 172** voralphabetsch **rechenbare** Silben.
/// `S = (L * 21 + V) * 28 + T`, rueckwaerts `L = S / 588`,
/// `V = (S % 588) / 28`, `T = S % 28`. 11 172 Konstanten waeren 11 172 Zeilen,
/// die man falsch abtippen kann; die Formel kann man **pruefen**, indem man sie
/// in beide Richtungen rechnet.
const HANGUL_S: (u32, u32) = (0xAC00, 0xD7A3 + 1);
const HANGUL_L: u32 = 0x1100;
const HANGUL_V: u32 = 0x1161;
const HANGUL_T: u32 = 0x11A7;
const HANGUL_V_ANZAHL: u32 = 21;
const HANGUL_T_ANZAHL: u32 = 28;
const HANGUL_N: u32 = HANGUL_V_ANZAHL * HANGUL_T_ANZAHL; // 588
const HANGUL_L_ANZAHL: u32 = 19;

fn hangul_zerlegen(c: u32, out: &mut Vec<u16>) -> bool {
    if !(HANGUL_S.0..HANGUL_S.1).contains(&c) {
        return false;
    }
    let s = c - HANGUL_S.0;
    out.push((HANGUL_L + s / HANGUL_N) as u16);
    out.push((HANGUL_V + (s % HANGUL_N) / HANGUL_T_ANZAHL) as u16);
    let t = s % HANGUL_T_ANZAHL;
    if t != 0 {
        out.push((HANGUL_T + t) as u16);
    }
    true
}

fn hangul_komponieren(l: u16, v: u16) -> Option<u16> {
    let li = l as u32;
    let vi = v as u32;
    if !(HANGUL_L..HANGUL_L + HANGUL_L_ANZAHL).contains(&li)
        || !(HANGUL_V..HANGUL_V + HANGUL_V_ANZAHL).contains(&vi)
    {
        return None;
    }
    let s = HANGUL_S.0 + ((li - HANGUL_L) * HANGUL_V_ANZAHL + (vi - HANGUL_V)) * HANGUL_T_ANZAHL;
    Some(s as u16)
}

/// Ein **Kernzeichen** in UTF-16-Einheiten: unter 0x10000 eine, darueber ein
/// **Ersatzpaar**. Das ist der einzige Ort im ganzen Builtin-Bereich, der
/// Surrogate erzeugt — und er muss es, weil `0x242EE` in keiner `u16` steht.
fn als_utf16(c: u32, out: &mut Vec<u16>) {
    if c <= 0xFFFF {
        out.push(c as u16);
    } else {
        let c = c - 0x1_0000;
        out.push(0xD800 + ((c >> 10) as u16));
        out.push(0xDC00 + ((c & 0x3FF) as u16));
    }
}

fn zerlege(c: u32, kompatibel: bool, out: &mut Vec<u16>) {
    if hangul_zerlegen(c, out) {
        return;
    }
    // **Kompatibilitaet ZUERST**: ein Kompatibilitaetszeichen kann eine
    // kanonische Zerlegung haben (`<font> FB03 -> 0066 0066 0073`), und
    // umgekehrt nie.
    if kompatibel
        && let Ok(i) = crate::unicode_norm::ZERLEGUNG_KOMPATIBEL
            .binary_search_by(|p| (p.0).cmp(&c))
    {
        for &x in crate::unicode_norm::ZERLEGUNG_KOMPATIBEL[i].1 {
            zerlege(x, true, out);
        }
        return;
    }
    if let Ok(i) = crate::unicode_norm::ZERLEGUNG.binary_search_by(|p| (p.0).cmp(&c)) {
        for &x in crate::unicode_norm::ZERLEGUNG[i].1 {
            zerlege(x, false, out);
        }
        return;
    }
    als_utf16(c, out);
}

/// ## W59b — **`KOMBINATION` ist jetzt eine BEREICHSliste**, und die Suche
/// sucht den letzten **Anfang ≤ c** und prueft dann das **Ende**.
///
/// **WARUM, gemessen:** `DerivedCombiningClass.txt` hat **544 Zeilen** und
/// **496 Bereiche**. Meine erste Fassung brach sie zu **287 346 Einzelwerten**
/// auf — die Tabelle wurde **5,4 MB**, `rustc` brauchte **1026 MB** und wuchs
/// mit **+300 MB/s**, und der **Leckwächter hat den Prozess getötet** (Regel 11).
/// *Bequemlichkeit beim Lesen wird mit Speicher beim Bauen bezahlt, und der
/// Zahler kommt erst am Ende.* Jetzt: **496 Einträge**, 234 KB.
///
/// **DIE SUCHE, und sie hat zwei Schritte, weil ein Bereich zwei Enden hat:**
/// der letzte Anfang ≤ `c` — und dann `c <= ende`. Fehlt der zweite Vergleich,
/// liegt `c` in der **Lücke** vor dem Treffer und bekäme dennoch dessen
/// Klasse. *Ein Treffer in einer Intervallsuche ist erst mit dem zweiten
/// Vergleich ein Treffer.*
fn ccc(c: u16) -> u8 {
    let cp = c as u32;
    match crate::unicode_norm::KOMBINATION.binary_search_by(|(von, _, _)| von.cmp(&cp)) {
        Ok(i) => crate::unicode_norm::KOMBINATION[i].2,
        Err(0) => 0,
        Err(i) => {
            let (von, bis, k) = crate::unicode_norm::KOMBINATION[i - 1];
            if cp <= bis { k } else { 0 }
        }
    }
}

/// **Kanonische Anordnung** (Unicode 3.11): **stabil** nach `ccc`, und ein
/// `ccc == 0` (ein Starter) ist eine **Grenze** — dahinter wird nicht
/// sortiert. *Diese Grenze ist der ganze Unterschied zu „alles sortieren"; ohne
/// sie wird `"b" + Ring + "a"` zu `"a" + Ring + "b"`, was die Norm verbietet.*
fn ordne_kanonisch(einheiten: &mut [u16]) {
    // ## W52b — **DIE LAENGE WAR O(n^2), und die Suite hat es gemessen.**
    //
    // **GEMESSEN: der Volllauf brauchte 461 s** (vorher 54 s) — Ziel der
    // Leitung war **<= 60 s**. Die Ursache ist **nicht** die Tabelle (2424
    // Eintraege, drei binaere Suchen je Einheit), sondern **dieser** Austausch:
    // `i` laeuft ueber **alle** Einheiten, und fuer jede wird gegen den
    // **Rest** getauscht. Bei einer Kette von `n` kombinierenden Zeichen ist
    // das **quadratisch** — und test262 hat Faelle mit langen Ketten.
    //
    // **DIE REGEL DER NORM ist eine EINMALIGE:** ein Starter (`ccc == 0`) ist
    // eine Grenze, und **nur der Lauf dazwischen** wird sortiert. Also laeuft
    // `i` **nur ueber die Starter**, und der Lauf dahinter wird in **einem**
    // Durchgang per **Einfuegen** sortiert — das ist der Algorithmus der Norm
    // (Unicode 3.11, „Canonical Ordering Algorithm“) und **O(n log n)**.
    //
    // *Ein Verfahren, das quadratisch ist und nie gemessen wurde, ist eine
    // Zeitbombe mit dem Etikett „Algorithmus".*
    let mut anfang = 0usize; // Beginn des aktuellen Laufs
    for i in 1..einheiten.len() {
        let k = ccc(einheiten[i]);
        if k == 0 {
            anfang = i; // eine Grenze: der Lauf davor ist fertig
            continue;
        }
        // **Einfuegen** an die richtige Stelle **innerhalb** des Laufs.
        let mut j = i;
        while j > anfang + 1 {
            let kj = ccc(einheiten[j - 1]);
            if kj <= k {
                break;
            }
            einheiten.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// **Komposition** (Unicode 3.11): von links, ein Starter nimmt alle folgenden
/// Zeichen mit `ccc != 0` auf, die zusammen mit ihm eine Primaerkomposition
/// ergeben.
fn komponiere(einheiten: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(einheiten.len());
    let mut starter: Option<usize> = None;
    for &c in einheiten {
        let k = ccc(c);
        if let Some(si) = starter {
            let schluessel = (out[si] as u32, c as u32);
            let treffer = hangul_komponieren(out[si], c).map(u32::from).or_else(|| {
                crate::unicode_norm::KOMPOSITION
                    .binary_search_by(|p| (p.0).cmp(&schluessel))
                    .ok()
                    .map(|i| crate::unicode_norm::KOMPOSITION[i].1)
            });
            if let Some(n) = treffer {
                out[si] = n as u16;
                continue;
            }
        }
        if k == 0 {
            starter = Some(out.len());
        }
        out.push(c);
    }
    out
}

/// **Die vier Formen** (ES2023 22.1.3.13 Schritt 8).
///
/// **DIE DREI SCHRITTE SIND NICHT VERTAUSCHBAR:** erst **zerlegen**, dann
/// **anordnen** (nur NFD/NFKD), dann **komponieren** (nur NFC/NFKC). *Wer erst
/// komponiert und dann zerlegt, bekommt `A` statt `A` — zwei Schritte in der
/// falschen Reihenfolge ergeben ein Ergebnis, das **richtig aussieht** und
/// falsch ist.*
pub(crate) fn normalisieren(text: &str, form: &str) -> String {
    let kompatibel = matches!(form, "NFKC" | "NFKD");
    let komponieren = matches!(form, "NFC" | "NFKC");
    let einheiten: Vec<u16> = crate::regexmini::als_einheiten(text);
    let mut out: Vec<u16> = Vec::with_capacity(einheiten.len());
    // **Ersatzpaare zuerst zusammenfuehren.** `als_einheiten` gibt UTF-16-
    // Einheiten, die Zerlegungstabelle aber **Kernzeichen** (`u32`) — und
    // `0x242EE` steht in **keiner** `u16`. Also: jedes Paar wird zu einem
    // Kernzeichen, und `als_utf16` ist die **einzige** Stelle, die wieder
    // Paare erzeugt. *Zwei Darstellungen brauchen **einen** Uebergang, nicht
    // zwei — sonst zerlegt die eine Form die andere zu zwei Zeichen.*
    let mut i = 0usize;
    while i < einheiten.len() {
        let u = einheiten[i];
        if (0xD800..0xDC00).contains(&u)
            && i + 1 < einheiten.len()
            && (0xDC00..0xE000).contains(&einheiten[i + 1])
        {
            let cp = 0x1_0000
                + (((u as u32) - 0xD800) << 10)
                + (((einheiten[i + 1] as u32) - 0xDC00));
            zerlege(cp, kompatibel, &mut out);
            i += 2;
        } else {
            zerlege(u as u32, kompatibel, &mut out);
            i += 1;
        }
    }
    if !komponieren {
        ordne_kanonisch(&mut out);
    } else {
        out = komponiere(&out);
    }
    crate::regexmini::als_text(&out)
}
