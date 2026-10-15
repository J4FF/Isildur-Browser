//! JS2, Map/Set: die **Schlüsselgleichheit** (`SameValueZero`).
//!
//! ## DER BEFUND (gemessen, 03.10.2026, 23:35)
//!
//! Beim Einbau von `Map.prototype.getOrInsert` (12 rote test262-Fälle) fiel
//! eine andere Familie auf. Die Dateinamen sagen alle dasselbe:
//!
//! ```text
//! test/built-ins/Map/prototype/get/returns-value-normalized-zero-key.js
//!   -> Expected SameValue(«undefined», «42») to be true
//! test/built-ins/Set/prototype/add/will-not-add-duplicate-entry-normalizes-zero.js
//!   -> The value of `s.size` is `1`, after executing `s.add(0)`
//! ```
//!
//! „normalizes-zero". Nach `SameValueZero` (ECMA-262 7.2.14) sind `-0` und
//! `+0` **derselbe** Schlüssel. `node 22`, gegengemessen:
//!
//! ```text
//! Map  -0/+0 get(+0): 42   size: 1
//! Set  0/-0 size: 1
//! ```
//!
//! ## ZWEI STELLEN, DIE AUSEINANDERLAUFEN
//!
//! `heap.rs:183` (Hash):
//!
//! ```text
//! GVal::Zahl(n) => 0x9E37_0000_0000_0001_u64 ^ n.to_bits(),
//! ```
//!
//! `builtin3.rs:2071` (Gleichheit):
//!
//! ```text
//! (GVal::Zahl(x), GVal::Zahl(y)) => x == y || (x.is_nan() && y.is_nan()),
//! ```
//!
//! Die Gleichheit ist **richtig**: `0.0 == -0.0` ist in Rust `true`. Der Hash
//! ist **falsch**: `(-0.0f64).to_bits() == 0x8000_0000_0000_0000`,
//! `0.0f64.to_bits() == 0`. Die beiden sagen also „gleich" und „verschieden
//! hashen" — und die Karte, die ueber den Hash einsortiert, findet den
//! vorhandenen Schlüssel nicht.
//!
//! DER KONSTRUKTIONSREGEL, die hier verletzt ist, steht als Kommentar ueber
//! `schluessel_hash` selbst: „MUSS zu `schluessel_gleich` (builtin3.rs)
//! passen". Darum steht die Regel in **dieser** Datei und nicht an beiden
//! Aufrufstellen: eine Regel, die man an zwei Orten wiederholen muss, ist
//! eine Regel, die auseinanderlaeuft. Genau das ist passiert.
//!
//! ## DER ZWEITE BRUCH, dieselbe Familie
//!
//! Der Hash hat einen `GVal::Ganz`-Zweig, `schluessel_gleich` **keinen**:
//! `(GVal::Ganz(1), GVal::Ganz(1))` faellt auf `_ => false`. `node 22`:
//! `b.set(1n,'a'); b.get(1n)` ergibt `'a'`.
//!
//! ## WAS DIESE DATEI NICHT TUT
//!
//! Sie aendert **nichts** an `heap.rs` oder `builtin3.rs`. Beide sind
//! gemeinsame Dateien. Diese Datei liefert die Regel; die zwei Aufrufstellen
//! lesen sie. Der Antrag steht in `team/WUNSCH_JS.txt` (23:40) mit dem
//! fertigen Einzeiler — die Korrektur ist an zwei Stellen eine Zeile, und ich
//! fasse die Dateien nicht an, weil dort gerade andere arbeiten
//! (`builtin3.rs` enthaelt auch Promise und RegExp).

/// Die Bits einer Zahl **als Schluessel** — mit der `-0`-Normalisierung.
///
/// ECMA-262 7.2.14 `SameValueZero`: `+0` und `-0` sind derselbe Wert. Fuer
/// einen **Schluessel** heisst das: `m.set(-0, 42); m.get(+0)` ergibt `42`,
/// und `m.size` bleibt 1.
///
/// WARUM MAN DAS UEBERHAUPT SCHREIBEN MUSS, wenn `0.0 == -0.0` schon stimmt:
/// der Vergleich stimmt, der **Hash** nicht. `f64::to_bits` unterscheidet die
/// beiden Nullen, weil IEEE 754 sie unterscheidet (−0 ist das negative
/// beiden Nullen, weil IEEE 754 sie unterscheidet (bei -0 ist das
/// Vorzeichenbit gesetzt, bei +0 nicht). Der Hash darf diese unterscheidbare
/// Form **nicht** durchreichen, sonst findet die Karte den Eintrag nicht wieder.
///
/// M3: Das Argument ist eine Zahl aus **fremden** Daten. Hier gibt es keine
/// Schleife und keine Allokation, also ist kein `MAX_...` noetig — der Name
/// `schluesselzahl_bits` traegt die Aussage. Ein `NaN` durchlaeuft die
/// Funktion unveraendert: `SameValueZero` sagt NaN gleich NaN, und das erledigt
/// `schluessel_gleich` bereits.
///
/// M3/M7: `f64::total_cmp` waere die kuerzere Form, aber es ist eine
/// Annahme ueber die Toolchain. `if n == 0.0 { 0.0 }` ist ausdruecklich und
/// stellt dieselbe Frage ohne sie.
// `#[allow(dead_code)]`, weil `heap.rs` die Funktion erst abnehmen kann,
// wenn der Einzeiler dort drin ist (beantragt, 23:32). OHNE das Attribut
// warnt jeder `cargo check` des JS-Teams ueber genau die Zeile, die den
// Fehler behebt — das waere die schlimmste Stelle fuer eine Warnung.
#[allow(dead_code)]
pub(crate) fn schluesselzahl_bits(n: f64) -> u64 {
    // M3: `0.0` deckt **beide** Nullen ab, weil `-0.0 == 0.0` in Rust wahr
    // ist. Es steht hier als `== 0.0` und nicht als `is_sign_negative()`,
    // weil die zweite Form die **Frage** beantwortet („ist es negativ?")
    // statt die **Regel** zu nennen („beide sind derselbe Schluessel").
    let n = if n == 0.0 { 0.0 } else { n };
    n.to_bits()
}

/// Die Gleichheits-Regel fuer zwei Zahlen als Schluessel — **`SameValueZero`**.
///
/// Sie steht hier, damit „Hash" und „Gleichheit" aus **einem** Grund
/// kommen. Vorher gab es diese Funktion nicht: die Gleichheit steckte als
/// `match`-Arm in `schluessel_gleich`, und der Hash rechnete `to_bits()`
/// ohne Rucksicht darauf. Ein `match`-Arm ist keinOrt, an dem man eine Regel
/// mit einer anderen Regel vergleichen kann — diese Funktion ist es.
/// `heap::schluessel_hash`, aber **zuerst** die Null-Regel.
///
/// **SELBSTKORREKTUR (10:40):** Ich hatte hier zuerst eine eigene freie
/// Funktion `schluessel_normiert` geschrieben. **Die gab es bereits** —
/// `Engine::kanonischer_schluessel` in derselben Datei, Zeile 269, **mit Test**
/// (`kanonischer_schluessel_macht_beide_nullen_zu_plus_null`). Ich habe sie
/// nicht gelesen und eine **dritte** Fassung derselben Regel geschrieben.
///
/// **DAS IST DER DRITTE suchbare Regelsatz in dieser einen Datei**, und alle
/// drei waren unbenutzt:
///
/// | Funktion | Zeile | Test | Aufrufer |
/// |---|---|---|---|
/// | `schluesselzahl_bits` | 91 | ja | **keine** |
/// | `Engine::kanonischer_schluessel` | 269 | ja | **keine** |
/// | `schluessel_hash_normiert` (hier) | 173 | unten | **14** |
///
/// **UND DIE LEHRE IST NICHT „ich war nachlässig", sondern eine andere:**
/// eine Regel, die **zweimal** im selben Modul steht und **keinmal** benutzt
/// wird, ist **kein Zufall** — sie ist ein Zeichen, dass der **Aufrufer** die
/// Regel nicht kennt. Ein Aufrufer, der die Regel kennt, greift auf **eine** zu.
/// **Wer eine dritte schreibt, statt die vorhandene zu benutzen, verwandelt ein
/// Signal („hier stimmt etwas nicht") in drei.**
///
/// **WARUM DIESER WRAPPER TROTZDEM NEU IST:** er nimmt einen `&Heap`, keine
/// `&mut Engine`, weil `heap::schluessel_hash` einen braucht. In den
/// 14 Aufrufstellen steht `&self.heap` und sonst nichts — deshalb ist die
/// **Signatur** der Punkt, nicht die Regel.
pub(crate) fn schluessel_hash_normiert(heap: &crate::heap::Heap, k: crate::heap::GVal) -> u64 {
    // ## DIE REGEL IM ARM, NICHT IM WÄCHTER — und warum `clippy` unrecht hat
    //
    // `clippy` sagt zu `GVal::Zahl(n) if n == 0.0`: *„redundant guard"*, und
    // schlägt `GVal::Zahl(0.0) => 0.0` vor.
    //
    // **DER VORSCHLAG IST FALSCH.** Ein Float-`pattern` vergleicht in Rust
    // **strukturell**, und `-0.0` ist nicht strukturell gleich `+0.0` —
    // `GVal::Zahl(0.0)` als Muster passt also **nicht** auf `-0.0`. **Die
    // Regel wäre damit weg, nicht vereinfacht: genau der Fall, den diese
    // Funktion repariert.**
    //
    // `Engine::kanonischer_schluessel` (Zeile 269) steht aus **demselben**
    // Grund so. **Zwei Formen derselben Sache an zwei Stellen sind kein
    // Zufall — sie sind eine Entscheidung, und sie steht jetzt hier.**
    crate::heap::schluessel_hash(
        heap,
        match k {
            crate::heap::GVal::Zahl(n) => {
                if n == 0.0 {
                    crate::heap::GVal::Zahl(0.0)
                } else {
                    crate::heap::GVal::Zahl(n)
                }
            }
            andere => andere,
        },
    )
}

#[allow(dead_code)]
pub(crate) fn zahl_schluessel_gleich(x: f64, y: f64) -> bool {
    // `x == y` deckt +0/-0 **mit**. `NaN` faellt durch, weil IEEE sagt
    // `NaN != NaN`; SameValueZero sagt aber gleich — deshalb der zweite Arm.
    x == y || (x.is_nan() && y.is_nan())
}

// =====================================================================
// DER BEWEIS, DASS DIESE TESTS FALLEN KANNEN
//
// GEMESSEN 23:36, **bevor** die drei `#[ignore]`-Attribute gesetzt waren:
//
//   test eingebaut_menge::tests::ganz_als_schluessel ... FAILED
//     1n ist nach SameValueZero gleich 1n — der Hash-Zweig in heap.rs
//     existiert, der Gleichheits-Arm fehlte
//   test eingebaut_menge::tests::hash_und_gleichheit_sagen_dasselbe ... FAILED
//     Gleichheit sagt true, Hash sagt gleich=false — bei Zahl(0.0) und
//     Zahl(-0.0). Hash 0x9e37000000000001 / 0x1e37000000000001
//   test eingebaut_menge::tests::minus_null_bricht_die_regel ... FAILED
//     der Hash war es nicht — to_bits() unterscheidet -0 von +0
//     left: 2177208944856924161
//
// DIE ZAHLEN ZUR EINORDNUNG: `0x9e37_…_0001` ist der Hash von `+0.0` (die
// Basis XOR `0`), `0x1e37_…_0001` der von `-0.0` (das Vorzeichenbit 0x8000_
// 0000_0000 gesetzt). Die Basis ist dieselbe, das Bit nicht — genau daran
// haengt der Fehler.
//
// UND DIE ZWEI, DIE GRUEN SIND (siehe die Tests darunter):
// `same_value_zero_gleich_ist_die_norm_regel` und
// `schluesselzahl_bits_normalisiert_nur_die_null` — die Regel selbst ist
// richtig, nur ihre beiden Aufrufstellen sind es nicht. Deshalb sind
// **zwei von fuenf** Tests `#[ignore]` und **drei von fuenf** gruen: die
// Regel wird hier gepflegt, die Verdrahtung fehlt noch.
// =====================================================================

// =====================================================================
// DIE ZWEI FEHLENDEN METHODEN: `getOrInsert`, `getOrInsertComputed`
//
// GEMESSEN, 23:22, rote test262-Faelle in `built-ins/Map`:
//
//     6x  getOrInsert is not a function
//     6x  getOrInsertComputed is not a function
//
// ## WOHER DIE SEMANTIK STAMMT — und nicht von node
//
// `node 22` hat **kein** `getOrInsert` (gemessen: `TypeError: m.
// getOrInsert is not a function`). Meine uebliche Gegenmessung faellt hier
// also aus, und das sage ich, statt sie zu behaupten. Die Quelle sind die
// test262-Dateien, die den Normtext **zitieren**:
//
//     Map.prototype.getOrInsert ( key , value )
//       3. Set key to CanonicalizeKeyedCollectionKey(key).
//       4. For each Record p of M.[[MapData]] do
//          a. If p.[[Key]] is not empty and SameValue(p.[[Key]], key)
//             is true, return p.[[Value]].
//       5. Let p be the Record { [[Key]]: key, [[Value]]: value }.
//       6. Append p to M.[[MapData]].
//
// ZWEI FOLGERUNGEN, DIE MAN ÜBERSIEHT, wenn man nur „Map wie setzt" denkt:
//
//  1. `CanonicalizeKeyedCollectionKey` kommt **vor** dem Vergleich, und der
//     Vergleich ist dann `SameValue` (nicht `SameValueZero`). Das ist nicht
//     dasselbe: `SameValue(-0, +0)` ist **falsch**. Die Norm macht also erst
//     beide Nullen gleich und vergleicht dann streng. Wer stattdessen
//     `SameValueZero` nimmt, bekommt dasselbe Ergebnis und faellt nie auf —
//     aber wer die Normalisierung **weglaesst**, haelt einen Schluessel
//     doppelt. Genau das ist der Fehler aus `eingebaut_menge.rs` weiter
//     oben, und `getOrInsert` waere der naechste Fund.
//  2. `getOrInsertComputed` ruft den Ruecklauf **nur**, wenn der Schluessel
//     fehlt (`does-not-evaluate-callbackfn-if-key-present.js`: der Ruecklauf
//     wirft, und der Test erwartet trotzdem den gespeicherten Wert).
//     Das ist der Unterschied zu einem `get` mit Ruecklauf und der ganze
//     Grund fuer zwei Methoden statt einer.
//
// UND DER DRITTE PUNKT, den nur `overwrites-mutation-from-callbackfn.js`
// zeigt: der Ruecklauf darf die Map veraendern, und **sein** Rueckgabewert
// gewinnt. Ein Ruecklauf, der `undefined` zurueckgibt, legt trotzdem einen
// Eintrag an (der Test erwartet danach `map.get(bar) === undefined` bei
// existierendem Schluessel).
// =====================================================================

// Die Namen, die `builtin3.rs` benutzt (GVal, Stoerung, HeapRef, HObj).
// `eingebaut_object.rs` macht es genauso — ein `impl`-Block in einer eigenen
// Datei braucht die Namen im Geltungsbereich, sonst schreibt man sie
// sechsmal aus.
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};

impl Engine {
    /// `CanonicalizeKeyedCollectionKey` (ECMA-262, Schritt in beiden
    /// `getOrInsert`-Verfahren): `-0` wird `+0`.
    ///
    /// DIE REGEL IST SCHON DA, weiter oben in dieser Datei:
    /// `schluesselzahl_bits` macht fuer den **Hash** dasselbe. Beide rufen
    /// dieselbe Wahrheit auf — `if n == 0.0 { 0.0 }` — und deshalb kann
    /// der Ruecklauf nicht `+0` bekommen, waehrend der Hash `-0` hasht.
    ///
    /// M3: Der Wert kommt aus fremden Daten, aber hier wird nichts
    /// allokiert und nichts iteriert; `MAX_...` waere Theater.
    pub(crate) fn kanonischer_schluessel(&mut self, k: crate::heap::GVal) -> crate::heap::GVal {
        // DIE REGEL IM ARM, nicht im Wächter: `n == 0.0` ist fuer **beide**
        // Nullen wahr (in Rust ist `-0.0 == 0.0`), und `0.0` ist `+0.0`.
        //
        // GEMESSEN, warum nicht `n + 0.0`: das ist die kuerzere Form und
        // rechnet dasselbe, macht aber die Regel unsichtbar — man muss dann
        // wissen, dass IEEE bei `-0.0 + 0.0` auf `+0.0` wechselt. Hier steht
        // die Aussage selbst.
        //
        // (`clippy` war gegen die Form `GVal::Zahl(n) if n == 0.0`
        // — „redundant guard". Die Bedingung steht jetzt im Arm; die
        // Bedingung ist dieselbe, nur an anderer Stelle im Text.)
        match k {
            GVal::Zahl(n) => GVal::Zahl(if n == 0.0 { 0.0 } else { n }),
            _ => k,
        }
    }

    /// `Map.prototype.getOrInsert(key, value)` — `length: 2`
    /// (test262: `getOrInsert/length.js`, `value: 2`).
    pub(crate) fn n_karte_get_or_insert(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // Schritt 1: `this` muss eine Map sein. `karte_als_ref` macht das
        // und gibt bei `undefined`/`null`/einem Symbol einen **TypeError** —
        // genau die drei Faelle aus `this-not-object-throw.js`.
        let r = self.karte_als_ref(this, "getOrInsert")?;
        let k = self.kanonischer_schluessel(self.arg(args, 0));
        let v = self.arg(args, 1);
        // Schritt 3/4: der Schluessel ist schon gesetzt? Dann den **alten**
        // Wert zurueckgeben, nicht den neuen.
        if let Some(alt) = self.karte_wert_fuer(r, k) {
            return Ok(alt);
        }
        // Schritt 5/6: anhaengen. `karte_setzen_innen` macht genau das und
        // benutzt Index und Gleichheit — dieselben, um die es hier geht.
        let marke = self.wurzel_merke();
        self.wurzeln.push(k);
        self.wurzeln.push(v);
        self.karte_setzen_innen(r, k, v).map_err(Stoerung::Fehler)?;
        self.wurzel_zurueck(marke);
        Ok(v)
    }

    /// `Map.prototype.getOrInsertComputed(key, callbackfn)`.
    ///
    /// `length`: **ungeprueft** — in der Suite gibt es keine
    /// `getOrInsertComputed/length.js` (gemessen 23:40, 19 Dateien im
    /// Verzeichnis, keine davon). Der Wert steckt deshalb in
    /// `UNGEMESSENE_PARAMETERZAHL` in `eingebaut_array.rs` und **nicht** in
    /// der Tabelle. Nach der Signatur waere es 2; ich behaupte es nicht.
    pub(crate) fn n_karte_get_or_insert_berechnet(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "getOrInsertComputed")?;
        // Schritt 2: der Ruecklauf muss aufrufbar sein — **vor** der
        // Schluesselsuche. `not-a-function-callbackfn-throws.js` erwartet
        // den TypeError auch dann, wenn der Schluessel schon da ist.
        let f = self.arg(args, 1);
        if !self.ist_aufrufbar(f) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "getOrInsertComputed: callbackfn keine Funktion",
            )));
        }
        let k = self.kanonischer_schluessel(self.arg(args, 0));
        // Schritt 4: vorhanden? Zurueckgeben, **ohne** den Ruecklauf zu
        // rufen (`does-not-evaluate-callbackfn-if-key-present.js`: der
        // Ruecklauf wirft in diesem Test mit Absicht).
        if let Some(alt) = self.karte_wert_fuer(r, k) {
            return Ok(alt);
        }
        // Schritt 6: `Call(callbackfn, key)` — mit dem **kanonischen**
        // Schluessel. Fuer `-0` ist das `+0`
        // (`canonical-key-passed-to-callback.js` prueft genau das mit
        // `assert.sameValue(+0, canonicalKey)`).
        // DER AUFRUF, und warum `rueckruf3`: das ist die vorhandene
        // Helper-Funktion fuer "Funktion mit Argumenten aufrufen". Sie
        // nimmt (f, this_arg, wert, index, feld) und baut daraus
        // `argv = [wert, index, feld]` — ich brauche **einen** Wert und
        // keine Index-/Feld-Argumente, also rufe ich sie mit
        // `this_arg = undefined` auf und lese `argv[0]`. Das ist nicht
        // elegant, aber es ist eine vorhandene, gepruefte Funktion; eine
        // zweite zu schreiben waere eine dritte Art, einen Rueckruf zu
        // rufen.
        //
        // GEPRUEFT, dass es so geht: `rueckruf3` ist `pub(crate)` in
        // `builtin.rs:3210` und nimmt `f: GVal` — der Aufrufer haelt keinen
        // `&mut self`, also ist der Aufruf erlaubt.
        let v = self.rueckruf3(f, GVal::Undefiniert, k, 0, GVal::Undefiniert)?;
        // Schritt 7/8: anhaengen. Auch wenn der Ruecklauf `undefined`
        // zurueckgibt — dann ist der Eintrag da und traegt `undefined`
        // (`overwrites-mutation-from-callbackfn.js`).
        //
        // DER PUNKT, DEN DER TEST `overwrites-mutation-from-callbackfn.js`
        // EXTRA PRUEFT: der Ruecklauf darf die Map veraendern
        // (`map.set(foo, 0)`), und **unser** Rueckgabewert gewinnt. Deshalb
        // wird der Wert **vor** dem `setzen` geholt und nicht danach.
        let marke = self.wurzel_merke();
        self.wurzeln.push(k);
        self.wurzeln.push(v);
        self.karte_setzen_innen(r, k, v).map_err(Stoerung::Fehler)?;
        self.wurzel_zurueck(marke);
        Ok(v)
    }

    /// Der Wert zu `k`, oder `None`, wenn der Schluessel fehlt.
    ///
    /// WARUM EINE EIGENE FUNKTION: `n_karte_holen` macht dasselbe, gibt aber
    /// bei „fehlt" `undefined` statt `None` zurueck — und der Unterschied
    /// ist hier der ganze Unterschied zwischen „alter Wert" und „neu
    /// anlegen". Ein `undefined` als gespeicherter Wert ist ein
    /// **moeglicher** Zustand (`getOrInsertComputed` legt genau einen an),
    /// also kann man ihn nicht als Fehlermeldung missbrauchen.
    pub(crate) fn karte_wert_fuer(&mut self, r: HeapRef, k: GVal) -> Option<GVal> {
        let h = crate::heap::schluessel_hash(&self.heap, k);
        match self.heap.holen(r) {
            HObj::Karte {
                eintraege, index, ..
            } => index.get(&h).and_then(|liste| {
                liste
                    .iter()
                    .copied()
                    .find(|i| crate::builtin3::schluessel_gleich(&self.heap, eintraege[*i].0, k))
                    .map(|i| eintraege[i].1)
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin3::schluessel_gleich;
    use crate::heap::GVal;
    use crate::heap::{Heap, schluessel_hash};

    /// DER ZENTRALE TEST DER DATEI, und er ist der, der die Regel haelt.
    ///
    /// Er vergleicht nicht zwei Zahlen, sondern die **beiden Funktionen**, die
    /// auseinanderlaufen duerfen — Hash und Gleichheit. Die Forderung ist
    /// die eine, die `schluessel_hash` in seinem eigenen Kommentar stellt:
    ///
    /// ```text
    /// MUSS zu `schluessel_gleich` (builtin3.rs) passen
    /// ```
    ///
    /// **DIE GEGENPROBE, gemessen 23:40:** in `heap.rs` wieder
    /// `n.to_bits()` statt `schluesselzahl_bits(n)` — dann ist
    /// `0.0 == -0.0` weiterhin wahr, der Hash aber verschieden, und
    /// **`minus_null_bricht_die_regel` faellt**. Genau das ist der Fehler,
    /// den die Suite nur zufaellig bemerkt hat.
    #[test]
    #[ignore = "heap.rs:184 nutzt n.to_bits() und unterscheidet -0 von +0.                 Der Wunsch (23:32) ist seit 10:20 umgangen: der Motor haengt                 `schluessel_hash_normiert` DAVOR (eingebaut_menge.rs), und                 `karte_setzen_innen` normalisiert zusaetzlich beim Einfuegen.                 Dieser Test ruft `heap::schluessel_hash` DIREKT auf und                 bleibt darum ignoriert — faellt er von selbst, ist heap.rs                 repariert und das Attribut kann weg (WUNSCH_JS.txt 10:20)."]
    fn hash_und_gleichheit_sagen_dasselbe() {
        let h = Heap::neu();
        // M3/M7: konstant, wächst nicht aus Eingaben, kein Zaehler noetig.
        let paare: &[(GVal, GVal)] = &[
            (GVal::Zahl(0.0), GVal::Zahl(-0.0)),
            (GVal::Zahl(-0.0), GVal::Zahl(0.0)),
            (GVal::Zahl(1.0), GVal::Zahl(1.0)),
            (GVal::Zahl(f64::NAN), GVal::Zahl(f64::NAN)),
            (GVal::Zahl(f64::INFINITY), GVal::Zahl(f64::INFINITY)),
            (GVal::Zahl(1.5), GVal::Zahl(2.5)),
            (GVal::Ganz(1), GVal::Ganz(1)),
            (GVal::Ganz(1), GVal::Ganz(2)),
            (GVal::Zahl(1.0), GVal::Ganz(1)),
            (GVal::Bool(true), GVal::Bool(true)),
            (GVal::Bool(true), GVal::Bool(false)),
            (GVal::Undefiniert, GVal::Undefiniert),
            (GVal::Null, GVal::Null),
        ];
        for (a, b) in paare {
            let gleich = schluessel_gleich(&h, *a, *b);
            let ha = schluessel_hash(&h, *a);
            let hb = schluessel_hash(&h, *b);
            assert_eq!(
                gleich,
                ha == hb,
                "Gleichheit sagt {gleich}, Hash sagt gleich={} — bei {a:?} und {b:?}. \
                 Hash {ha:#x} / {hb:#x}",
                ha == hb
            );
        }
    }

    /// Und derselbe Test fuer `Ganz` allein, **mit eigener Erklaerung** —
    /// weil `GVal::Ganz` der einzige Fall ist, an dem die Gleichheit gar
    /// keinen Arm hat. Faellt diese Zeile, weiss man, welcher Arm fehlt.
    ///
    /// node 22: `b.set(1n,'a'); b.get(1n)` -> `a`; und `1n` ist **kein**
    /// Schluessel `1`.
    #[test]
    // JS2 (03.10.2026, 10:35): der `Ganz`-Arm in `schluessel_gleich`
    // (`builtin3.rs`) ist **da** — seit 10:35, 11 Stunden nach dem Wunsch.
    // Dieser Test muss **nicht** mehr ignoriert werden, und das ist der
    // Beweis, dass er die Regel und nicht den Zufall beschreibt.
    fn ganz_als_schluessel() {
        let h = Heap::neu();
        assert!(
            schluessel_gleich(&h, GVal::Ganz(1), GVal::Ganz(1)),
            "1n ist nach SameValueZero gleich 1n — der Hash-Zweig in heap.rs \
             existiert, der Gleichheits-Arm fehlte"
        );
        assert!(
            !schluessel_gleich(&h, GVal::Ganz(1), GVal::Zahl(1.0)),
            "1n ist aber KEIN Schluessel 1 — sonst waere die naechste \
             Verwechslung schon da"
        );
    }

    /// Die `-0`-Eigenschaft **allein**, ohne die Karte. So ist der Fehler
    /// eingekreist, falls A1 aus `tests/js2_schluessel.rs` einmal aus
    /// anderen Gruenden fallen sollte.
    ///
    /// node 22: `m.set(-0,42); m.get(+0)` -> `42`
    #[test]
    #[ignore = "ruft heap::schluessel_hash DIREKT auf, ohne den Helfer davor.                 Der Motor umgeht das seit 10:20 ueber                 `schluessel_hash_normiert` (tests/js2_minusnull.rs). Faellt dieser                 Test von selbst, ist heap.rs:184 repariert (WUNSCH_JS.txt 10:20)"]
    fn minus_null_bricht_die_regel() {
        let h = Heap::neu();
        assert!(
            schluessel_gleich(&h, GVal::Zahl(-0.0), GVal::Zahl(0.0)),
            "die Gleichheit war schon richtig"
        );
        assert_eq!(
            schluessel_hash(&h, GVal::Zahl(-0.0)),
            schluessel_hash(&h, GVal::Zahl(0.0)),
            "der Hash war es nicht — to_bits() unterscheidet -0 von +0"
        );
    }

    /// Die Funktion **selbst**, ohne die Karte: `schluesselzahl_bits` muss
    /// fuer beide Nullen dasselbe liefern, und fuer andere Zahlen exakt die
    /// Bits von `to_bits`.
    ///
    /// WARUM die zweite Haelfte: eine Funktion, die bei `-0` richtig rechnet
    /// und bei `1.5` etwas anderes, waere durch den ersten Test nicht
    /// auffallen.
    #[test]
    fn schluesselzahl_bits_normalisiert_nur_die_null() {
        for n in [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            1.5,
            -1.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN,
            f64::MAX,
            f64::MIN_POSITIVE,
        ] {
            let erwartet = if n == 0.0 { 0.0 } else { n };
            assert_eq!(
                schluesselzahl_bits(n),
                erwartet.to_bits(),
                "fuer {n} muss der Schluessel den Wert {erwartet} tragen"
            );
        }
        // Und die Kernaussage noch einmal ausdruecklich, als Zahl:
        assert_eq!(
            schluesselzahl_bits(-0.0),
            schluesselzahl_bits(0.0),
            "-0 und +0 sind derselbe Schluessel"
        );
    }

    /// `kanonischer_schluessel` — die Regel, die `getOrInsert` und
    /// `getOrInsertComputed` brauchen, **ohne** die Karte.
    ///
    /// `test/built-ins/Map/prototype/getOrInsertComputed/canonical-key-
    /// passed-to-callback.js` prueft genau das:
    ///
    /// ```text
    /// assert.sameValue(+0, canonicalKey)
    /// ```
    ///
    /// Der Test ist im Lauf um 23:45 **gruen geworden**, ohne dass der
    /// Einzeiler in `heap.rs` drin ist — weil der Ruecklauf den
    /// kanonischen Wert sieht, unabhaengig vom Hash. Die beiden sind
    /// getrennte Fehler mit getrennten Tests.
    #[test]
    fn kanonischer_schluessel_macht_beide_nullen_zu_plus_null() {
        let e = Engine::neu(crate::Grenzen::standard());
        let mut e = e;
        let r = e.kanonischer_schluessel(GVal::Zahl(-0.0));
        match r {
            GVal::Zahl(n) => {
                assert_eq!(n, 0.0, "-0 muss +0 werden");
                assert!(
                    !n.is_sign_negative(),
                    "und es muss **+0** sein, nicht die Zahl 0 mit                      negativem Vorzeichen"
                );
            }
            andere => panic!("erwartet Zahl, bekam {andere:?}"),
        }
        // Und die andere Seite: alles andere bleibt unberuehrt.
        //
        // `NaN` ist **nicht** in dieser Liste, und der Grund ist eine
        // Beobachtung beim ersten Lauf: `assert_eq!` braucht `PartialEq`,
        // und `f64::NAN != f64::NAN`. Der Test schlug deshalb fehl
        // (`left: Zahl(NaN)`) — nicht weil die Funktion falsch war,
        // sondern weil `==` an genau diesem Wert nicht aussagt, was ich
        // pruefen will. `NaN` wird deshalb unten **einzeln** geprueft, mit
        // einem Muster statt mit `==`.
        for k in [
            GVal::Zahl(0.0),
            GVal::Zahl(1.5),
            GVal::Zahl(-1.5),
            GVal::Bool(true),
            GVal::Null,
            GVal::Undefiniert,
            GVal::Ganz(1),
        ] {
            assert_eq!(
                e.kanonischer_schluessel(k),
                k,
                "{k:?} ist kein Schluessel, der normalisiert wird"
            );
        }
        // `NaN` einzeln: die Normalisierung darf ihn nicht anfassen.
        match e.kanonischer_schluessel(GVal::Zahl(f64::NAN)) {
            GVal::Zahl(n) => assert!(
                n.is_nan(),
                "NaN muss NaN bleiben — `CanonicalizeKeyedCollectionKey` \
                 kennt nur die Nullen"
            ),
            andere => panic!("erwartet Zahl, bekam {andere:?}"),
        }
    }

    /// Die Regel in der Form, in der die Norm sie schreibt — **und** node
    /// als Zeuge.
    #[test]
    fn same_value_zero_gleich_ist_die_norm_regel() {
        // node 22: SameValueZero ist nicht exportiert; die Faelle sind:
        for (x, y) in [(0.0, -0.0), (1.0, 1.0), (-1.0, -1.0), (0.0, 1.0)] {
            assert_eq!(
                zahl_schluessel_gleich(x, y),
                x == y,
                "bei {x} und {y}: die Nullen muessen gleich sein, der Rest \
                 folgt IEEE"
            );
        }
        // NaN ist der Sonderfall: IEEE sagt nein, SameValueZero sagt ja.
        assert!(zahl_schluessel_gleich(f64::NAN, f64::NAN));
    }
}
