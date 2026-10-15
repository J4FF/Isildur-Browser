//! JS2, Teil 1: die **Parameterzahl** der eingebauten Funktionen.
//!
//! ## DER BEFUND (gemessen, 03.10.2026)
//!
//! test262, Stand vor dieser Datei:
//!
//! ```text
//! t262_echt: Quote 1052 von 2117 = 49.7 %
//!   built-ins/Array   103 / 503 = 20.48 %
//! ```
//!
//! Die drei groessten Fehlerposten in `Array` (127 von 503 Faellen, also 25 %)
//! waren **alle** dieselbe Sache:
//!
//! ```text
//! 47  Getter nur auf Objekten                    [Array/prototype/concat/…]
//! 45  Unbehandelt: name should be an own property [Array/from/Array.from-name.js]
//! 35  Unbehandelt: length should be an own property [Array/from/Array.from_arity.js]
//! ```
//!
//! `name` wird aus `HObj::Nativ.name` geliefert und war in Ordnung; **`length`
//! nicht**. In `builtin.rs:2426` steht fuer ein `Nativ`:
//!
//! ```text
//! GVal::Zahl(0.0)     // "Ein Native kennt seine Parameterzahl nicht"
//! ```
//!
//! Und `builtin.rs:279`:
//!
//! ```text
//! HObj::Nativ { .. } => if name == "length" { return Ok(GVal::Zahl(0.0)); }
//! ```
//!
//! `Array.from.length` ist aber **1** (ECMA-262, und die Suite prueft es mit
//! `verifyProperty(Array.from, "length", {value: 1, …})`).
//!
//! ## WARUM EINE EIGENE DATEI
//!
//! Der Auftrag lautet: nur neue Dateien `eingebaut_<name>.rs` bzw. die
//! bestehenden builtin-Dateien dieser Objekte. `eigenschaften.rs` und
//! `vm.rs` sind **gemeinsam** — dort steht die Sache in zwei Zeilen, und beide
//! brauchen dieselbe Zahl. Also steht die **Zahl** hier, und die beiden
//! Aufrufstellen lesen sie (das ist der in `team/WUNSCH_JS.txt` beschriebene
//! Weg: gemeinsame Datei nur ueber die Wunschliste).
//!
//! DIE TABELLE, und warum sie nicht „aus dem Namen abgeleitet" werden kann:
//! `Nativ` hat **kein** `arity`-Feld, und `HObj::Nativ` wird in
//! `heap.rs:992` ohne Parameterzahl angelegt. Eine Ableitung aus dem Namen
//! („`includes` hat zwei Parameter") ist geraten und test262 merkt es.
//!
//! ## WAS DIESE DATEI NICHT TUT
//!
//! Sie aendert **kein** Verhalten von `vm.rs` oder `eigenschaften.rs` ohne
//! Wunsch. Sie liefert die Zahl, die dort gebraucht wird.

use crate::builtin::Nativ;

/// Die Parameterzahl einer eingebauten Funktion.
///
/// ## DIE EINZIGE QUELLE DER ZAHLEN: test262, nicht die Norm
///
/// GEMESSEN, und das ist der wichtigste Punkt dieser Datei: **meine erste
/// Fassung dieser Tabelle hatte zwei falsche Zahlen.** Ich hatte sie aus einer
/// Normregel abgeleitet („`length` ist die Zahl der Parameter **vor** dem
/// ersten optionalen") und die beiden JSON-Faelle selbst als **ungeprueft**
/// markiert. Die Suite sagt etwas anderes:
///
/// ```text
/// test/built-ins/JSON/parse/length.js      value: 2
/// test/built-ins/JSON/stringify/length.js  value: 3
/// ```
///
/// Also ist `JSON.parse.length` **2** und `JSON.stringify.length` **3** — die
/// Zahl **aller** Parameter. Und `Array.from` ist **1**, obwohl es drei
/// Parameter hat. Es gibt also **keine** gemeinsame Regel, aus der man die
/// Zahlen ableiten kann; die Norm schreibt sie je Funktion einzeln. Die
/// Tabelle ist deshalb eine **Abschrift der Suite**, und der naechste Eintrag
/// ist so zu lesen wie ein Zitat, nicht wie eine Herleitung.
///
/// DIE QUELLE, MEHRFACH (nicht aus dem Gedaechtnis): ausgewertet wurden alle
/// `verifyProperty(<fn>, 'length', {value: N` in `/home/jaff/test262/test/
/// built-ins/`. Ergebnis: **600** solcher Faelle in der Suite, davon
/// **79** in meinen vier Objekten (Array 38, String 19, Map 11, Set 7, JSON 4).
/// Die erste Auszaehlung ergab 83 — vier davon waren Fehltreffer meiner eigenen
/// Auswertung (`desc.get`, `descriptor.get` sind Accessor-Deskriptoren und
/// keine Natives) und sind abgezogen.
///
/// ## WAS IN DER TABELLE STEHT UND WAS NICHT
///
/// Steht drin: **nur** Zahlen, die test262 selbst behauptet.
/// Steht **nicht** drin: alles, wofuer die Suite keine `length.js` hat. Fuer
/// die bleibt 0, und die Namen stehen in [`UNGEMESSENE_PARAMETERZAHL`] — damit
/// die Luecke **benannt** ist und nicht so aussieht, als waere 0 richtig.
///
/// M3: `Nativ` ist ein Enum aus **meinem** Crate. `unreachable!()` waere hier
/// der ehrliche Fehler, aber ein Absturz ist im Fuzz (`T4`) kein Fehler, sondern
/// ein Abbruch. Also 0 zurueckgeben und es hier sagen.
///
/// M3/M7: Die Tabelle ist **konstant**, wächst nicht aus Eingaben und hat keine
/// Schleife. Ein Zaehler ist nicht noetig; `match` ist die vollstaendige
/// Behandlung.
/// ECMA-262 10.1.14 `IsConstructor`.
///
/// | was | `node 22` | warum |
/// |---|---|---|
/// | `Array.of.call(T, 3)` | `T` wird gerufen | `T` hat `[[Construct]]` |
/// | `Array.of.call({}, 1)` | `TypeError` | `{}` ist **kein** `Function` |
/// | `Array.of.call(()=>{}, 1)` | `TypeError` | Pfeilfunktion ohne `[[Construct]]` |
///
/// **WARUM NICHT `ist_aufrufbar`:** das sagt `true` fuer eine Pfeilfunktion,
/// weil sie aufrufbar ist. **`IsConstructor` fragt etwas anderes.** Genau wie
/// `RequireObjectCoercible` und `IsCallable` zwei verschiedene Regeln sind,
/// obwohl beide „darf man?".
/// **`Reflect.construct` Schritt 3** (ES2023 7.3.16): *"If IsConstructor
/// (newTarget) is false, throw a TypeError exception."*
///
/// **DIE AUSNAHME FUER `Symbol` GEHOERT HIERHER NICHT — und das ist gemessen,
/// nicht behauptet.** `ist_konstruktor` oben nimmt `Symbol` aus, weil
/// `Array.of`/`Array.from` es ablehnen. `node 22` (04.10.2026) haelt **beide**
/// Regeln gleichzeitig:
///
/// ```js
/// Array.of.call(Symbol, 1)                 // -> TypeError
/// Array.from.call(Symbol, [])              // -> TypeError
/// Reflect.construct(function(){},[],Symbol) // -> KEIN Wurf, liefert Object
/// ```
///
/// **UND test262 nagelt beide Seiten fest:** `Symbol/is-constructor.js`
/// behauptet `assert(isConstructor(Symbol))`, waehrend die `Array.of`-Faelle
/// den `TypeError` verlangen. *Es ist also nicht eine Regel mit zwei
/// Ausnahmen, sondern **eine** Regel mit **zwei** Fragen — und wer sie
/// zusammenfasst, baut eines von beidem falsch.* Tatsaechlich hat das
/// `Symbol`-Funktionsobjekt einen `[[Construct]]`, der erst beim Aufruf
/// ueber `new` wirft; 20.4.2.1 beschreibt etwas anderes als die
/// `IsConstructor`-Antwort, und die Suite folgt der Messung, nicht dem Wort.
///
/// **DARUM EIGENE FUNKTION statt ein Flag an `ist_konstruktor`:** die
/// Regel von 20.4.2.1 bleibt unberuehrt dort, wo sie gehoert — und die
/// abweichende Stelle bekommt ihren **eigenen, benannten** Weg. *Ein Flag,
/// das zwei verschiedenen Fragen bedienen soll, beantwortet frueher oder
/// spaeter eine davon falsch — und man findet die Stelle dann nicht mehr.*
pub(crate) fn ist_konstruktor_als_neutziel(
    e: &crate::interp::Engine,
    v: crate::heap::GVal,
) -> bool {
    if ist_konstruktor(e, v) {
        return true;
    }
    // Der Rest ist genau `Symbol`: `ist_konstruktor` liefert dort `false`,
    // obwohl `Symbol` ueber `mk_ctor` in `nativ_proto` steht.
    let crate::heap::GVal::Ref(r) = v else {
        return false;
    };
    matches!(e.heap.holen(r), crate::heap::HObj::Nativ { .. }) && crate::symbole::ist_symbol(e, r)
}

pub(crate) fn ist_konstruktor(e: &crate::interp::Engine, v: crate::heap::GVal) -> bool {
    let crate::heap::GVal::Ref(r) = v else {
        return false;
    };
    match e.heap.holen(r) {
        // ## W20, GEBAUT (JS2/QUIC, 03.10.2026, 04:05)
        //
        // Vorher stand hier `!ist_symbol(e, r)` — also war **jeder** `Nativ`
        // ausser `Symbol` ein Konstruktor. GEMESSEN (`node 22`):
        // ```js
        // Array.of.call(Math.cos)          node: []  (ein Array!)
        // ```
        // und bei uns kam `TypeError: Construct lieferte keinen Gegenstand`,
        // weil `Math.cos` kein Konstruktor **ist** und trotzdem fuer einen
        // gehalten wurde.
        //
        // **DIE INFORMATION LIEGT BEREITS IM MOTOR — ich musste sie nur lesen.**
        // `nativ_prototyp` wird an **genau zwei** Stellen aufgerufen:
        // `ctor!` (`builtin.rs:784`) und `mk_ctor` (`builtin.rs:859`). Das sind
        // die **beiden** Konstruktorwege; `nativ_anlegen` ruft sonst niemand
        // mit Prototyp auf (24 Aufrufer, 2 davon Konstruktoren). Also:
        //
        // ```text
        // nativ_proto enthaelt r   <=>   r wurde als Konstruktor gebaut
        // ```
        //
        // **DAS IST DIE DRITTE FASSUNG DERSELBEN REGEL IN DIESER SITZUNG**
        // (nach `schluesselzahl_bits` und `kanonischer_schluessel`): die
        // richtige Karte lag im Haus und war fuer diesen Zweck unbenutzt.
        // `symbole.rs:55` las `nativ_proto` bereits — nur an anderer Stelle.
        //
        // **`Symbol` bleibt draussen, und das ist keine Erinnerung, sondern die
        // Norm:** 20.4.2.1 sagt woertlich, `Symbol` sei *„the only builtin
        // constructor function that does not have a `[[Construct]]` internal
        // method"*. `Symbol` laeuft ueber `mk_ctor`, ist also in `nativ_proto`
        // — die Sonderregel ist damit **noetig**, nicht historisch.
        crate::heap::HObj::Nativ { .. } => {
            e.nativ_proto.contains_key(&r) && !crate::symbole::ist_symbol(e, r)
        }
        crate::heap::HObj::Funktion {
            fn_index,
            gebundenes_this,
            ..
        } => {
            // ## W35 (JS2/QUIC, Runde 95) — eine **gebundene** Funktion ist
            // **kein** Konstruktor.
            //
            // **GEMESSEN (`node 22`), der Fall ist
            // `Array/of/return-a-new-array-object.js`, letzte Zeile:**
            // ```js
            // Array.of.call(Math.cos)             instanceof Array  node true  bei uns true
            // Array.of.call(Math.cos.bind(Math))  instanceof Array  node true
            //   bei uns  WURF "Laufzeitfehler: Funktionsindex falsch (intern)"
            // ```
            //
            // **DER WEG, in genau drei Schritten:**
            // 1. `n_funktion_binden` (`builtin.rs:4342`) baut die gebundene
            //    Funktion als `HObj::Funktion` mit **`fn_index: usize::MAX`**
            //    und **`ist_konstruktor: false`** — der Aufruf ist ein
            //    Sprung ueber `scope: Some(orig)`.
            // 2. Dieser Arm hat **das Feld `ist_konstruktor` ignoriert** und
            //    stattdessen `aufruf_kopf(usize::MAX)` gefragt.
            // 3. Das scheitert, und **`Err(_) => true` las den Fehlschlag als
            //    „ist ein Konstruktor"** — also nahm `Array.of` den
            //    `Construct`-Weg, und der VM fand `usize::MAX` nicht.
            //
            // **WARUM `gebundenes_this` UND NICHT NUR DAS FELD:** das Feld
            // `ist_konstruktor` wird laut `vm.rs:3035` nur an **einer** Stelle
            // auf `true` gesetzt — es ist damit **kein** zuverlaessiger
            // Positivbefund fuer gewöhnliche Funktionen, und mich auf es zu
            // stuetzen hiesse, **eine** Funktion zu entwerten, um **eine**
            // andere zu retten. *Ein Feld, das nur an einer Stelle gesetzt
            // wird, ist eine Absicht, keine Tatsache.*
            //
            // **WARUM ALLE gebundenen Funktionen `false`, obwohl die Norm
            // `IsConstructor(boundF) == IsConstructor(target)` sagt:** unser
            // `bind` setzt `ist_konstruktor` **immer** auf `false` — es
            // **traegt die Konstruktorfaehigkeit gar nicht erst**. Der Befund
            // `false` ist also **nicht neu erfunden**, sondern das, was `bind`
            // bereits behauptet; `ArrayCreate` (der spec-konforme
            // `ArrayCreate(len)`-Zweig fuer einen Nicht-Konstruktor) macht daraus
            // ein **echtes** Array. *Der Weg, den die Norm fuer einen
            // Nicht-Konstruktor vorsieht, ist hier auch der richtige.*
            //
            // **`Err(_) => true` bleibt unangetastet** — es ist eine
            //moeglicherweise relevante Annahme ueber **andere** Funktionen, und
            // die kann ich mit **diesem** Fall nicht belegen. *Ein Fehlschlag,
            // den man nicht erklaeren kann, gehoert nicht in dieselbe Aenderung
            // wie ein Fehlschlag, den man erklaeren kann.*
            // **UND `ist_konstruktor` wird hier ausdruecklich NICHT als
            // Positivbefund benutzt** (siehe oben) — deshalb steht es im
            // Muster nicht drin.
            if gebundenes_this.is_some() {
                return false;
            }
            match e.aufruf_kopf(*fn_index) {
                // Der VM lehnt Pfeilfunktionen beim Konstruktor-Aufruf selbst ab
                // (`vm.rs`). Hier steht nur, was man **vor** dem Aufruf wissen muss.
                Ok(k) => !k.pfeil,
                Err(_) => true,
            }
        }
        _ => false,
    }
}

pub(crate) fn parameterzahl(n: &Nativ) -> u32 {
    match n {
        // Release 15.10.2026: the ES2022-2026 natives know their length.
        Nativ::Es(op) => op.laenge(),
        // ================= Array (38 Faelle in der Suite) =================
        Nativ::FeldAus => 1,       // unbenutzt: die alte Array.from-Fassung
        Nativ::FeldVon => 1,       // Array.from  (ECMA-262 23.1.2.2)
        Nativ::FeldOf => 0,        // Array.of    (ECMA-262 23.1.2.3)
        Nativ::FeldAn => 1,        // Array.prototype.at
        Nativ::FeldVerketten => 1, // concat
        Nativ::FeldKopierenInnerhalb => 2, // copyWithin
        // W40 (QUIC, 04.10.2026): ES2023 23.1.3.34-38. `node 22` sagt
        // `findLast.length` = 1, `toReversed.length` = 0, `toSorted.length` =
        // **1** (es nimmt `comparefn`), `with.length` = 2, `toSpliced.length`
        // = **2** (start, skipCount — der Rest sind variabel).
        Nativ::FeldFindeIndexLetzten => 1,
        Nativ::FeldKopiertUmdreht => 0,
        Nativ::FeldKopiertSortiert => 1,
        Nativ::FeldMit => 2,
        Nativ::FeldKopiertGeteilt => 2,
        Nativ::FeldEintraegeIter => 0,  // entries
        Nativ::FeldAlle => 1,           // every
        Nativ::FeldFuellen => 1,        // fill
        Nativ::FeldFiltern => 1,        // filter
        Nativ::FeldFinden => 1,         // find
        Nativ::FeldFindeIndex => 1,     // findIndex
        Nativ::FeldFuerJedes => 1,      // forEach
        Nativ::FeldEnthaelt => 1,       // includes
        Nativ::FeldIndexVon => 1,       // indexOf
        Nativ::FeldVerbinden => 1,      // join
        Nativ::FeldSchluesselIter => 0, // keys
        Nativ::FeldLetzterIndex => 1,   // lastIndexOf
        Nativ::FeldAbbilden => 1,       // map
        Nativ::FeldNehmen => 0,         // pop
        Nativ::FeldSchieben => 1,       // push
        Nativ::FeldFalten => 1,         // reduce
        Nativ::FeldFaltenRechts => 1,   // reduceRight
        Nativ::FeldUmdrehen => 0,       // reverse
        Nativ::FeldVorne => 0,          // shift
        Nativ::FeldScheibe => 2,        // slice
        Nativ::FeldEinige => 1,         // some
        Nativ::FeldSortieren => 1,      // sort
        Nativ::FeldSpleissen => 2,      // splice
        Nativ::FeldProtoText => 0,      // toString
        Nativ::FeldHinten => 1,         // unshift
        Nativ::FeldWerteIter => 0,      // values
        // ================= JSON (4 Faelle in der Suite) =================
        //
        // DIE ZWEI KORRIGIERTEN ZAHLEN. Sie standen vorher als 1 und waren
        // falsch; die Suite sagt 2 und 3. Siehe die Datei-Doku.
        Nativ::JsonLesen => 2,
        Nativ::JsonSchreiben => 3,
        // ================= Map (11 Faelle in der Suite) =================
        Nativ::KarteLeeren => 0,     // clear
        Nativ::KarteLoeschen => 1,   // delete
        Nativ::KarteEintraege => 0,  // entries
        Nativ::KarteFuerJedes => 1,  // forEach
        Nativ::KarteHolen => 1,      // get
        Nativ::KarteHaben => 1,      // has
        Nativ::KarteSchluessel => 0, // keys
        Nativ::KarteSetzen => 2,     // set
        // JS2, 23:45: `getOrInsert/length.js` sagt `value: 2` — belegt,
        // also in der Tabelle und nicht auf der Warnliste.
        Nativ::KarteGetOrInsert => 2,
        Nativ::KarteWerte => 0, // values
        // ================= Set (7 Faelle in der Suite) =================
        Nativ::MengeHinzu => 1,     // add
        Nativ::MengeLeeren => 0,    // clear
        Nativ::MengeLoeschen => 1,  // delete
        Nativ::MengeEintraege => 0, // entries
        Nativ::MengeFuerJedes => 1, // forEach
        Nativ::MengeHaben => 1,     // has
        Nativ::MengeWerte => 0,     // values
        // ================= String (19 Faelle in der Suite) ================
        // 19 `length.js` + 14 aus der `X.length !== N`-Form (02.10.2026,
        // 00:55). Quelle je Zeile steht in der Klammer.
        Nativ::TextAusPunkten => 1, // fromCodePoint (length.js)
        Nativ::TextAusZeichen => 1, // fromCharCode (X.length !== 1)
        //
        // --- DIE EINE GRUPPE, die auf einen Schlag kam (00:55) ---
        //
        // Vorher standen die alle auf `UNGEMESSENE_PARAMETERZAHL` mit der
        // Begruendung „keine `length.js`". Die Begruendung war **falsch**,
        // und die Suite sagte es lauter: `charAt` und `charCodeAt` waren je
        // ein **roter** Fall mit der Meldung
        //   `String.prototype.charAt.length === 1. Actual: 0`.
        Nativ::TextZeichenAn => 1,    // charAt (X.length !== 1)
        Nativ::TextZeichenKode => 1,  // charCodeAt (X.length !== 1)
        Nativ::TextIndexVon => 1,     // indexOf (X.length !== 1)
        Nativ::TextLetzterIndex => 1, // lastIndexOf (X.length !== 1)
        Nativ::TextVerketten => 1,    // concat (X.length !== 1)
        Nativ::TextTreffer => 1,      // match (X.length !== 1)
        Nativ::TextSuchen => 1,       // search (X.length !== 1)
        Nativ::TextVergleich => 1,    // localeCompare (X.length !== 1)
        Nativ::TextScheibe => 2,      // slice (X.length !== 2)
        Nativ::TextTeil => 2,         // substring (X.length !== 2)
        Nativ::TextKlein => 0,        // toLowerCase (X.length !== 0)
        Nativ::TextGross => 0,        // toUpperCase (X.length !== 0)
        Nativ::TextTeilen => 2,       // split (length.js: 2)
        //
        // JS2 (03.10.2026, 07:55): `Array.isArray` stand in
        // `UNGEMESSENE_PARAMETERZAHL` mit der Begruendung „kein Beleg in
        // **irgendeiner** Form". **Die Begruendung war falsch**, und der Beleg
        // steht in `Array/isArray/15.4.3.2-0-2.js`:
        //
        // ```text
        // assert.sameValue(Array.isArray.length, 1, ...)
        // ```
        //
        // **DAS IST DER ZWEITE MAL in dieser Sitzung, dass diese Liste zu
        // frueh gefuellt war** (das erste war `JSON.parse.length` 1 statt 2 und
        // `JSON.stringify.length` 1 statt 3). Die Liste sagt „niemand weiss
        // es" — und `test262` weiss es. **Wer eine Zahl nicht misst, sollte sie
        // nicht eintragen, sondern suchen.**
        //
        // ## DIE KONSTRUKTOREN — und sie sind eine **andere** Tabelle in
        // derselben Liste
        //
        // GEMESSEN (11:25), `node 22`. Vorher stand hier **0** fuer **alle**:
        //
        // ```js
        // Array.length   node 22: 1      bei uns: 0
        // String.length  node 22: 1      bei uns: 0
        // ```
        //
        // **WARUM SIE IN DIESER TABELLE STEHEN UND NICHT IN EINER EIGENEN:**
        // `parameterzahl` hat genau **zwei** Verwendungen — `ops.rs:270`
        // (die `length` des Funktionsobjekts) und ein Test hier. **Eine zweite
        // Tabelle waere an zwei Stellen richtig und an einer einzigen
        // schaedlich**, denn die `length` wird beim Anlegen gesetzt und
        // nirgends nachgeschlagen. **Eine Liste, eine Zahl je `Nativ`.**
        //
        // **UND DIE NORMAUSNAHME, DIE HIER STEHT:** `Object.length` ist 1,
        // `Function.length` ist 1, `Date.length` ist **7** (der Constructor
        // nimmt `year, month, …, tz` als sieben Parameter), `RegExp.length`
        // ist 2, `Map`/`Set`/`Symbol` sind **0**. Das ist keine Regel, das
        // sind **gemessene** Zahlen — jede einzelne mit `node` gegengemessen.
        // W43 (QUIC, 04.10.2026): `Reflect` — ES2015 28.1 gibt fuer **jedes**
        // der 13 Verfahren eine `length` an, und die Suite prueft jede einzeln
        // (`built-ins/Reflect/<name>/length.js`). **GEMESSEN `node 22`:**
        // apply 3, construct 2, defineProperty 3, deleteProperty 2, get 2,
        // getOwnPropertyDescriptor 2, getPrototypeOf 1, has 2, isExtensible 1,
        // ownKeys 1, preventExtensions 1, set 3, setPrototypeOf 2.
        //
        // **DIE DREI NULLEN SIND NULLEN** — `construct`, `getPrototypeOf`,
        // `isExtensible`, `ownKeys`, `preventExtensions` sind 2/1/1/1/1, aber
        // `Reflect.construct.length` ist **2** (nicht 1, `newTarget` ist
        // **optional** und wird nicht gezaehlt) und `apply`/`set`/
        // `defineProperty` haben **3**. Jede Zahl hier ist eine **gemessene**,
        // und die Liste `UNGEMESSENE_PARAMETERZAHL` waere fuer `Reflect`
        // **falsch** — dort steht kein Name aus diesen 13.
        Nativ::ReflAnwenden => 3,
        Nativ::ReflKonstruieren => 2,
        Nativ::ReflDefiniere => 3,
        Nativ::ReflLoesche => 2,
        Nativ::ReflHole => 2,
        Nativ::ReflDeskriptor => 2,
        Nativ::ReflProto => 1,
        Nativ::ReflHat => 2,
        Nativ::ReflErweiterbar => 1,
        Nativ::ReflEigeneSchluessel => 1,
        Nativ::ReflVerhindere => 1,
        Nativ::ReflSetze => 3,
        Nativ::ReflSetzeProto => 2,
        // W44 (QUIC, 04.10.2026): ES2024 — `Object.groupBy.length` und
        // `Map.groupBy.length` sind **beide 2** (`node 22` gemessen). Kein
        // drittes Argument: `groupBy` nimmt `items, callbackfn`.
        // W46: `structuredClone.length` ist **1** (HTML 8.1.7.3 — es nimmt
        // `value` und, **optional**, `options`; ein optionales Argument wird
        // nicht gezaehlt). Ohne diesen Eintrag stand hier eine **0**, und die
        // Suite sagt es in `length.js`.
        Nativ::StructuredClone => 1,
        // W47: `String.prototype.matchAll.length` ist **1** (`node 22`) — es
        // nimmt `(regexp)` und sonst nichts.
        Nativ::TextMatchAlle => 1,
        // **`next` hat `length` 0** — es nimmt keine Argumente. Ohne diesen
        // Eintrag stuende eine **0**, und diesmal **aus Zufall richtig**; ich
        // trage sie trotzdem ein, weil eine Zahl, die richtig ist, weil niemand
        // nachgesehen hat, die schlechteste Sorte von Zahl ist.
        Nativ::TextMatchAlleNext => 0,
        Nativ::ObjektGroupBy => 2,
        Nativ::KarteGroupBy => 2,
        Nativ::ObjektCtor => 1, // Object.length
        // J-CACHE (05.10.2026): statische Object-Funktionen, ALLE GEMESSEN
        // `node 22` (eine Runde, keine Einzel-Runden). Vorher `_ => 0` —
        // das kostete u.a. assign-length.js und defineProperties.length
        // („Expected SameValue(«0», «2»)").
        Nativ::ObjektZuweisung => 2,      // Object.assign
        Nativ::UrlKann => 1,              // URL.canParse
        Nativ::ObjektErzeugen => 2,       // Object.create
        Nativ::ObjektDefiniere => 3,      // Object.defineProperty
        Nativ::ObjektDefiniereAlle => 2,  // Object.defineProperties
        Nativ::ObjektSchluessel => 1,     // Object.keys
        Nativ::ObjektWerte => 1,          // Object.values
        Nativ::ObjektEintraege => 1,      // Object.entries
        Nativ::ObjektHatEigen => 2,       // Object.hasOwn
        Nativ::ObjektNamen => 1,          // Object.getOwnPropertyNames
        Nativ::ObjektDeskriptor => 2,     // Object.getOwnPropertyDescriptor
        Nativ::ObjektDeskriptoren => 1,   // Object.getOwnPropertyDescriptors
        Nativ::ObjektIst => 2,            // Object.is
        Nativ::ObjektAusEintraegen => 1,  // Object.fromEntries
        Nativ::ObjektProtoVon => 1,       // Object.getPrototypeOf
        Nativ::ObjektProtoSetzen => 2,    // Object.setPrototypeOf
        Nativ::ObjektEinfrieren => 1,     // Object.freeze
        Nativ::ObjektVersiegeln => 1,     // Object.seal
        Nativ::ObjektNichtErweiterbar => 1, // Object.preventExtensions
        Nativ::ObjektIstGefroren => 1,    // Object.isFrozen
        Nativ::ObjektIstVersiegelt => 1,  // Object.isSealed
        Nativ::ObjektIstErweiterbar => 1, // Object.isExtensible
        Nativ::FeldCtor => 1,   // Array.length   (Array/length.js)
        Nativ::TextCtor => 1,   // String.length
        Nativ::ZahlCtor => 1,   // Number.length
        // J-CACHE (04.10.2026): statische Number-Pruefungen, GEMESSEN
        // `node 22`: alle 1. Vorher `_ => 0` — das kostete
        // isNaN/isFinite/isInteger/isSafeInteger/length.js.
        Nativ::ZahlIstNaN => 1,     // Number.isNaN
        Nativ::ZahlIstEndlich => 1, // Number.isFinite
        Nativ::ZahlIstGanz => 1,    // Number.isInteger
        Nativ::ZahlIstSicher => 1,  // Number.isSafeInteger
        Nativ::BoolCtor => 1,       // Boolean.length
        Nativ::FunktionCtor => 1,   // Function.length
        Nativ::FunktionArtCtor(_) => 1, // AsyncFunction.length usw.
        Nativ::RegexCtor => 2,      // RegExp.length
        Nativ::DatumCtor => 7,      // Date.length — sieben Parameter
        // ## W53 (QUIC, 05.10.2026) — DIE GANZE MATH-GRUPPE, **33 Eintraege,
        // 25 Suite-Faelle**.
        //
        // **GEMESSEN, wie ich darauf kam (`T262_NUR=length.js`, 1,13 s statt
        // 138 s Volllauf): 29 von 133 `length.js`-Faellen rot, davon 28
        // `Math/*/length.js`** mit **einer** Ursache: die Native hatte keine
        // Parameterzahl und fiel auf `_ => 0`. Die Meldung der Suite nennt die
        // Sollzahl selbst („length descriptor value should be 1; length value
        // should be 1") — **die Suite ist das Zeugnis, nicht meine Normregel**
        // (vgl. `TextAusZeichen` weiter oben: die Regel hat mich bei
        // `JSON.parse` in die Irre gefuehrt).
        //
        // **JEDE Zahl hier ist aus `node 22` GEGENMESSEN, nicht aus der
        // Suite und nicht aus dem Gedächtnis.** Ein Lauf, alle 35 Namen:
        //
        // ```text
        // FUNKTIONEN: 35
        // NICHT 1: atan2=2 hypot=2 imul=2 max=2 min=2 pow=2 random=0
        // ```
        //
        // **UND JEDER Variantenname ist aus der REGISTRIERUNG gelesen**, nicht
        // uebersetzt (`builtin2.rs:159-190`): `("sign", Nativ::MatheVor)`,
        // `("clz32", Nativ::MatheClz)`. *Ein Variantenname, den ich
        // uebersetze statt nachzuschlagen, ist an genau den Stellen falsch,
        // die man nicht bemerkt — `MatheVor` ist `sign`, `MatheRund` ist
        // `round`, `MatheKapp` ist `trunc`.*
        //
        // **DIE DREI FEHLENDEN (`acosh`, `asinh`, `atanh`) stehen hier
        // NICHT**, weil sie keine `length`-Luecke sind, sondern gar nicht
        // existieren: `node 22` hat sie (`typeof Math.acosh === "function"`),
        // wir haben kein `Nativ` dafuer — sie melden „kein Objekt aus
        // null/undefined", nicht „length sollte 1 sein". *Eine Tabelle, die
        // eine Zahl fuer etwas eintraegt, das es nicht gibt, heilt nichts.*
        // Das sind **3 Faelle zum Bauen**, nicht zum Messen — Math ist nicht
        // mein Bereich, also gemeldet statt gebaut.
        Nativ::MatheAbs => 1,        // Math.abs
        Nativ::MatheBoden => 1,      // Math.floor
        Nativ::MatheDecke => 1,      // Math.ceil
        Nativ::MatheRund => 1,       // Math.round
        Nativ::MatheKapp => 1,       // Math.trunc
        Nativ::MatheVor => 1,        // Math.sign
        Nativ::MatheWurzel => 1,     // Math.sqrt
        Nativ::MatheKubik => 1,      // Math.cbrt
        Nativ::MathePotenz => 2,     // Math.pow
        Nativ::MatheExp => 1,        // Math.exp
        Nativ::MatheExpm1 => 1,      // Math.expm1
        Nativ::MatheLog => 1,        // Math.log
        Nativ::MatheLog2 => 1,       // Math.log2
        Nativ::MatheLog10 => 1,      // Math.log10
        Nativ::MatheLog1p => 1,      // Math.log1p
        Nativ::MatheSin => 1,        // Math.sin
        Nativ::MatheCos => 1,        // Math.cos
        Nativ::MatheTan => 1,        // Math.tan
        Nativ::MatheAsin => 1,       // Math.asin
        Nativ::MatheAcos => 1,       // Math.acos
        Nativ::MatheAtan => 1,       // Math.atan
        Nativ::MatheAtan2 => 2,      // Math.atan2
        Nativ::MatheSinh => 1,       // Math.sinh
        Nativ::MatheCosh => 1,       // Math.cosh
        Nativ::MatheTanh => 1,       // Math.tanh
        Nativ::MatheMax => 2,        // Math.max
        Nativ::MatheMin => 2,        // Math.min
        Nativ::MatheZufall => 0,     // Math.random
        Nativ::MatheHypot => 2,      // Math.hypot
        Nativ::MatheClz => 1,        // Math.clz32
        Nativ::MatheImul => 2,       // Math.imul
        Nativ::MatheFround => 1,     // Math.fround
        // W55 (QUIC): die drei hyperbolischen Funktionen, **ALLE 1** —
        // **GEMESSEN `node 22`**, ein Lauf ueber `Object.getOwnPropertyNames`
        // nach der Registrierung. Das sind nicht geratene Zahlen, sondern
        // dieselbe Messung wie bei den 33 Eintraegen oben; `atanh` ist die
        // **letzte** Math-Funktion in der Liste und damit auch die letzte
        // Zeile dieser Tabelle.
        //
        // **OHNE DIESE DREI EINTRAEGE** waeren ihre `length.js` **rot**,
        // obwohl die Funktionen existieren — und das ist die Sorte Fehler,
        // die man erst sieht, wenn man die Zahl daneben schreibt.
        Nativ::MatheAcosh => 1,      // Math.acosh
        Nativ::MatheAsinh => 1,      // Math.asinh
        Nativ::MatheAtanh => 1,      // Math.atanh
        //
        // **DIE NULLEN, und sie sind NICHT vergessen:** `KarteCtor`,
        // `MengeCtor`, `SymCtor` und die TextEncoder/Decoder sind **0** nach
        // `node 22`, und 0 ist der Vorgabe-Wert — sie stehen hier **nicht**,
        // weil eine Null, die man nicht messen muss, auch nicht behauptet
        // werden muss.
        Nativ::FeldIstFeld => 1,       // Array.isArray (15.4.3.2-0-2.js)
        Nativ::TextStutzen => 0,       // trim (length.js: 0)
        Nativ::TextNormalisieren => 0, // normalize (length.js: 0)
        //
        // --- NUR `node 22`, kein Beleg in der Suite (00:52) ---
        //
        // Diese zwei tragen trotzdem einen Wert, weil 0 **falsch** waere und
        // `node 22` an beiden verlaesslich ist. Sie sind markiert, damit
        // „kein Beleg" und „Beleg aus node" nicht gleich aussehen.
        Nativ::TextTeilAlt => 2, // substr — NUR node (2)
        //
        // DER LETZTE WERT, der nicht aus einer `length.js` kommt, sondern aus
        // einem anderen Test — und deshalb der letzte, den ich **gemessen**
        // habe, statt ihn zu uebernehmen:
        //   test/built-ins/String/fromCharCode/S15.5.3.2_A1.js
        //     -> "String.fromCharCode.length === 1. Actual: 0"
        // GEMESSEN 23:00, nachdem der Fall **rot** geworden war. Ein Test, der
        // mir die Zahl sagt, ist ein besseres Zeugnis als meine Normregel —
        // die hat mich bei `JSON.parse` in die Irre gefuehrt.
        //
        // (`TextAusZeichen` steht weiter unten in der Gruppe aus der zweiten
        // Extraktion — dort mit dem Beleg `S15.5.3.2_A1.js`. Hier noch mal
        // einzutragen war ein Versehen: `rustc` sagt „unreachable pattern",
        // und das ist die richtige Antwort.)
        Nativ::TextRoh => 1,          // raw
        Nativ::TextAn => 1,           // at
        Nativ::TextPunktKode => 1,    // codePointAt
        Nativ::TextEndet => 1,        // endsWith
        Nativ::TextBeginnt => 1,      // startsWith
        Nativ::TextEnthaelt => 1,     // includes
        Nativ::TextFuellenEnde => 1,  // padEnd
        Nativ::TextFuellenStart => 1, // padStart
        Nativ::TextWiederholen => 1,  // repeat
        Nativ::TextErsetzen => 2,     // replace
        Nativ::TextErsetzenAlle => 2, // replaceAll
        Nativ::TextProtoText => 0,    // toString
        Nativ::TextStutzenEnde => 0,  // trimEnd
        Nativ::TextStutzenStart => 0, // trimStart
        Nativ::TextWertVon => 0,      // valueOf
        // J-CACHE (04.10.2026): Number-Prototyp-Methoden. GEMESSEN `node 22`:
        // toString=1 (radix), toFixed=1, toExponential=1, toPrecision=1.
        // Vorher fielen sie auf `_ => 0` — das kostete toFixed/toExponential/
        // toPrecision/length.js + S15.7.4.5_A2_T01.js. valueOf (= 0 nach
        // node) steht hier nicht (Vorgabe, keine Behauptung noetig).
        Nativ::ZahlProtoText => 1, // Number.prototype.toString
        Nativ::ZahlFest => 1,      // Number.prototype.toFixed
        Nativ::ZahlExpo => 1,      // Number.prototype.toExponential
        Nativ::ZahlGenau => 1,     // Number.prototype.toPrecision
        // J-CACHE (04.10.2026): Date.prototype.toJSON.length = 1
        // (GEMESSEN node 22). Vorher `_ => 0`.
        Nativ::DatumJson => 1, // Date.prototype.toJSON
        // J-CACHE (04.10.2026): Web-Konstruktoren/Methoden, GEMESSEN node 22.
        Nativ::WebKopfAnhaengen => 2,     // Headers.append
        Nativ::WebKopfLoeschen => 1,      // Headers.delete
        Nativ::WebKopfHolen => 1,         // Headers.get
        Nativ::WebKopfHat => 1,           // Headers.has
        Nativ::WebKopfSetzen => 2,        // Headers.set
        Nativ::WebAnfrageCtor => 1,       // Request
        Nativ::WebAntwortCtor => 0,       // Response
        Nativ::WebKlecksCtor => 0,        // Blob
        Nativ::WebKlecksPuffer => 0,      // Blob.arrayBuffer
        Nativ::WebDateiCtor => 2,         // File
        Nativ::WebFormularAnhaengen => 2, // FormData.append
        Nativ::WebFormularLoeschen => 1,  // FormData.delete
        Nativ::WebFormularHolen => 1,     // FormData.get
        Nativ::WebFormularHat => 1,       // FormData.has
        Nativ::WebFormularSetzen => 2,    // FormData.set
        Nativ::WebFormularAlle => 1,      // FormData.getAll
        Nativ::WebFormularSchluessel => 0, // FormData.keys
        Nativ::WebFormularWerte => 0,     // FormData.values
        Nativ::WebFormularEintraege => 0, // FormData.entries
        Nativ::WebFormularFuerAlle => 1,  // FormData.forEach
        // J-CACHE (05.10., PROBE_24, node 22): Beobachter-Ctoren nehmen
        // den Rueckruf (length 1); observe/disconnect/takeRecords je
        // Spec; Knoten-Ctoren sind verboten (length 0 wie node).
        Nativ::WebMutationCtor => 1,  // MutationObserver
        Nativ::WebKreuzungCtor => 1, // IntersectionObserver
        Nativ::WebGroesseCtor => 1,  // ResizeObserver
        Nativ::WebBeobachten => 1,   // observe(target)
        Nativ::WebNehmen => 0,       // takeRecords()
        Nativ::WebKnoten => 0,
        Nativ::WebSchnittstelle(_) => 0,
        Nativ::HostLesen(_) => 0,
        Nativ::HostSchreiben(_) => 1,
        // Release 10.10.2026: `Proxy.length` 2, `Proxy.revocable.length` 2,
        // `revoke.length` 0 (node 22).
        Nativ::ProxyKonstruktor => 2,
        Nativ::ProxyWiderrufbar => 2,
        Nativ::ProxyWiderrufen => 0,
        Nativ::CeHaken => 1,
        Nativ::CeDefinieren => 2,
        Nativ::CeBauen => 2,
        Nativ::CeFertig => 0,
        Nativ::ObjektUnterProtoLesen => 0,
        Nativ::ObjektUnterProtoSetzen => 1,
        Nativ::WebElement => 0,
        Nativ::WebHtmlElement => 0,
        Nativ::DomCreateElementNS => 2,
        Nativ::HostMethode { name, .. } => match name.as_str() {
            "createElementNS" | "setAttribute" | "setProperty" | "addEventListener" | "removeEventListener" => 2,
            "click" | "focus" | "blur" | "add" | "remove" => 0,
            _ => 1,
        },
        Nativ::DomCreateEvent | Nativ::EreignisInit | Nativ::EreignisCustomInit => 1,
        Nativ::DomRefSchreiben(_) => 1,
        // J-CACHE (04.10.2026): Date-Setter + Date.UTC, GEMESSEN node 22.
        // Vorher `_ => 0` — 16 length.js-Dateien.
        Nativ::DatumSetzenJahr => 3,      // setFullYear
        Nativ::DatumSetzenJahrLokal => 3, // setFullYear
        Nativ::DatumSetzenMonat => 2,     // setMonth
        Nativ::DatumSetzenMonatLokal => 2,
        Nativ::DatumSetzenTag => 1,       // setDate
        Nativ::DatumSetzenTagLokal => 1,
        Nativ::DatumSetzenStunden => 4,   // setHours
        Nativ::DatumSetzenStundenLokal => 4,
        Nativ::DatumSetzenMinuten => 3,   // setMinutes
        Nativ::DatumSetzenMinutenLokal => 3,
        Nativ::DatumSetzenSekunden => 2,  // setSeconds
        Nativ::DatumSetzenSekundenLokal => 2,
        Nativ::DatumSetzenMillis => 1,    // setMilliseconds
        Nativ::DatumSetzenMillisLokal => 1,
        Nativ::DatumZeitSetzen => 1,      // setTime
        Nativ::DatumUTC => 7,             // Date.UTC
        // J-CACHE (04.10.2026): Date.parse.length = 1 (GEMESSEN node 22).
        Nativ::DatumLesen => 1, // Date.parse
        // J-CACHE (04.10.2026): Math-Methoden, GEMESSEN node 22.
        // Vorher `_ => 0` — das kostete hypot/max/min/pow/round/trunc
        // length.js. acosh/asinh/atanh/sumPrecise fehlen als FUNKTION
        // (QUIC, gemeldet) und stehen hier NICHT (kein toter Eintrag).
        Nativ::MatheHypot => 2, // Math.hypot
        Nativ::MatheMax => 2,    // Math.max
        Nativ::MatheMin => 2,    // Math.min
        Nativ::MathePotenz => 2, // Math.pow
        Nativ::MatheRund => 1,   // Math.round
        Nativ::MatheKapp => 1,   // Math.trunc
        // --- der Rest ---
        //
        // 0 ist hier die **Untergrenze**, nicht eine Aussage. Sie steht in
        // `UNGEMESSENE_PARAMETERZAHL`, damit man sie nicht fuer eine Messung
        // haelt.
        _ => 0,
    }
}

/// Die Natives meiner vier Objekte, fuer die die Suite **keine** `length.js`
/// hat — und fuer die deshalb 0 dasteht.
///
/// DER NUTZEN DIESER LISTE: sie macht die Luecke **sichtbar**. Ohne sie sieht
/// die Tabelle vollstaendig aus, und ein vollstaendig aussehender `match` mit
/// `_ => 0` ist die haeufigste Form einer erfundenen Behauptung.
///
/// GEMESSEN, wie gross die Luecke ist: die Suite enthaelt 600 `length`-Faelle
/// ueberhaupt, aber nur 79 in meinen vier Objekten — fuer die uebrigen
/// Funktionen **prueft test262 die Zahl nicht**. Das heisst nicht, dass 0
/// richtig waere; es heisst, dass es niemand weiss und ich es nicht behaupte.
// `#[allow(dead_code)]`, und der Grund steht hier: die Liste wird nur von den
// Tests benutzt und von `team/WUNSCH_JS.txt` gelesen. Ohne das Attribut
// warnt jeder `cargo check` des JS-Teams ueber eine Zeile, die niemand
// braucht — und die Zeile ist genau die, die eine erfundene Zahl verhindert.
#[allow(dead_code)]
pub(crate) const UNGEMESSENE_PARAMETERZAHL: &[&str] = &[
    // Array: kein Beleg in **irgendeiner** Form.
    //
    // `Array.isArray` ist **nicht** mehr hier: es hat einen Beleg in
    // `Array/isArray/15.4.3.2-0-2.js` (`length === 1`), und 0 waere falsch.
    // Siehe den Kommentar an seinem Tabelleneintrag (07:55).
    "Array.prototype.flat",
    "Array.prototype.flatMap",
    "Array.prototype.toLocaleString",
    "Array.prototype.toReversed",
    "Array.prototype.toSorted",
    "Array.prototype.toSpliced",
    "Array.prototype.with",
    // String: kein Beleg in **irgendeiner** Form
    //
    // `substr` ist **nicht** mehr hier: es hat keinen Beleg in der Suite,
    // aber `node 22` sagt 2 (00:52), und 0 waere falsch. Der Eintrag steht
    // in der Tabelle und ist dort als „nur node" markiert.
    //
    // `trimStart`/`trimEnd`/`valueOf`/`toString`/`normalize`/`matchAll` sind
    // hier bzw. in der Tabelle mit Suite-Beleg (`trimEnd`/`trimStart`/
    // `valueOf`/`toString` stehen in der Tabelle wegen `length.js`).
    //
    // `String.prototype[Symbol.iterator]` ist kein Nativ, sondern ein
    // Symbol-Schluessel — die Liste ist eine Dokumentation, kein Nachschlage-
    // werk, und der Eintrag steht hier, damit die Luecke benannt ist.
    "String.prototype[Symbol.iterator]",
    // Map/Set: nicht in der Suite vorhanden (Aussicht 2026)
    "Map.groupBy",
    "Map.prototype.getOrInsert",
    // JS2, 23:45: `getOrInsertComputed` ist implementiert, aber es gibt
    // KEINE `getOrInsertComputed/length.js` in der Suite (19 Dateien im
    // Verzeichnis, keine davon). Nach der Signatur
    // `getOrInsertComputed(key, callbackfn)` waere es 2 — **ungeprueft**,
    // und deshalb hier und nicht in der Tabelle. Dieselbe Regel wie bei
    // `JSON.parse` in der ersten Fassung: eine Zahl, die niemand belegt hat,
    // ist geraten, auch wenn die Vermutung plausibel ist.
    //
    // DER HINWEIS AUF DIE ZWEITE EXTRAKTION (00:55) steht hier, weil er
    // genau diesen Fall betrifft: es gibt keine `length.js` — aber es gibt
    // auch keine `X.length !== N`-Assertion. **Beide** Formen wurden
    // gesucht. Als Gegensatz: `charAt` hatte ebenfalls keine `length.js`
    // und stand trotzdem hier — zu Unrecht.
    "Map.prototype.getOrInsertComputed",
];

// DER WEG, DEN I **NICHT** GEBAUT HABE — und warum nicht.
//
// Ich wollte `ops.rs` (das die Zahl braucht) einen zweiten Zugang anbieten:
// `parameterzahl_von_name(name: &str)`, weil `nativ_anlegen` dort den
// JS-Namen bereits als `&str` in der Hand hat. Die Tabelle stand in 20
// Minuten.
//
// **GEMESSEN, warum sie trotzdem wieder raus ist** (03.10.2026, 22:55): der
// Test `die_ungemessene_liste_trage_keine_zahl` fiel mit
//
// ```text
// `String.prototype.concat` steht in UNGEMESSENE_PARAMETERZAHL,
// traegt aber die Zahl 1
// ```
//
// Das ist kein Fehler in der Liste, sondern in der **Idee**. `concat` gibt es
// zweimal: `Array.prototype.concat` ist **1** (test262 sagt es), und
// `String.prototype.concat` hat **keine** `length.js`. Ein Schluessel aus dem
// kurzen Namen kann die beiden nicht unterscheiden und gibt beiden dieselbe
// Zahl — fuer die eine richtig, fuer die andere erfunden. Dasselbe gilt fuer
// `at`, `includes`, `toString`, `entries`, `keys`, `values`, `clear`,
// `delete`, `forEach`, `get`, `has`, `set`.
//
// Ein Schluessel aus `Nativ` hat das Problem **nicht**: die Enum-Variante ist
// eindeutig. Darum ist die Tabelle oben nach `Nativ` gebaut und nicht nach
// Namen — und darum gibt es diesen zweiten Zugang nicht.
//
// WAS ICH DAFUER ANBIETE: `UNGEMESSENE_PARAMETERZAHL` als **Dokumentation**,
// nicht als Lookup. Sie sagt, welche Zahlen niemand weiss.

/// Liest die **Elemente** eines array-ähnlichen Werts.
///
/// DER BEFUND, und er ist der Grund fuer diese Datei (03.10.2026, gemessen):
///
/// ```text
/// t262_echt:    27  Unbehandelt: TypeError: every braucht Feld
///               [test/built-ins/Array/prototype/every/15.4.4.16-1-10.js]
/// ```
///
/// `feld_als_ref` verlangt in `builtin.rs:2933` **zwingend** `HObj::Feld`:
///
/// ```text
/// HObj::Feld { .. } => Ok(r),
/// _ => Err(TypeError "{name} braucht Feld")
/// ```
///
/// Nach ECMA-262 23.1.3 sind die `Array.prototype`-Methoden aber auf **jedem**
/// Objekt anwendbar, das `length` und die Indizes hat — sie lesen generisch
/// ( „generic"), nicht ueber eine interne Feldstruktur. Genau das ist der
/// Testfall in Zeile oben: ein Objekt mit `length` und Indexeigenschaften, aber
/// kein echtes Array.
///
/// DER WEG, und warum es eine eigene Funktion ist: `feld_elemente` und
/// `feld_indizes` (builtin.rs:3086 und :3129) lesen **nur** `HObj::Feld`. Alle
/// 21 Aufrufstellen von `feld_als_ref` brauchen aber dieselbe Auslegung. Eine
/// eigene Funktion, ein Aufruf an zwei Stellen statt 21.
///
/// M3: `quelle` ist ein fremder Wert (aus dem Programm). Null/undefined gibt
/// einen `TypeError` — das ist ECMA-262, nicht meine Erfindung. Und die
/// Laenge ist **gedeckelt** (`MAX_LAENGE`), weil sie aus fremden Daten kommt;
/// ein Objekt mit `length: 1e9` darf keine 10^9-Schleife ausloesen.
pub(crate) const MAX_LAENGE: usize = 4_000_000;

/// Die Elemente in Lesereihenfolge, oder `None`, wenn der Wert nichts
/// array-ähnliches ist.
///
/// DER RUECKGABETYP, und warum er **hier** steht: `hole` bekommt einen
/// Abschluss ueber `&mut self` (in `builtin.rs:feld_elemente`), nicht ueber
/// `self` — ein Getter darf zurueck in den Motor. Darum kommt der Leser als
/// Parameter herein und die Logik hierher: so ist sie ohne den Motor
/// **pruefbar** (die drei Tests am Dateiende) und steht an **einer** Stelle
/// statt an 21.
///
/// GEMESSEN (03.10.2026): `GVal` hat **keine** Text-Variante — ein String ist
/// `GVal::Ref` auf ein `HObj::Text` (`heap.rs:26`). Meine erste Fassung
/// behandelte `GVal::Text` und haette nicht einmal kompiliert; die Varianten
/// habe ich in `heap.rs` nachgelesen, statt sie zu raten. Das ist der Grund,
/// warum diese Datei keinen Fall „String" fuehrt: Strings laufen ueber
/// `HObj::Text`, und das liest der Motor.
pub(crate) fn elemente_lesen(
    hole: &mut dyn FnMut(&str) -> crate::heap::GVal,
) -> Option<Vec<crate::heap::GVal>> {
    // DIE LAENGE kommt aus `laenge_lesen` und **nicht** aus einer zweiten
    // Kopie der `ToLength`-Regel: zwei Kopien laufen auseinander, und das ist
    // keine theoretische Sorge — in N49 fehlte genau ein `match`-Arm einer
    // solchen Kopie, und der Test blieb rot, ohne dass jemand es merkte.
    let len = laenge_lesen(hole);
    if len == 0 {
        return Some(Vec::new());
    }
    // Die Kapazitaet ist **klein** (4096) und nicht `len`: `with_capacity(len)`
    // wuerde genau die Allokation tun, die der Deckel verhindern soll.
    let mut out = Vec::with_capacity(len.min(4096));
    for i in 0..len {
        out.push(hole(&i.to_string()));
    }
    Some(out)
}

/// Die Laenge, die zu `elemente_lesen` gehoert — **dieselbe** Rechnung.
///
/// ZWEITER AUFRUFSTELLER, und darum eine eigene Funktion: `feld_indizes` in
/// `builtin.rs` braucht die Zahl der Indizes, nicht die Werte.
///
/// ECMA-262 6.1.6.1 `ToLength`: `undefined`/NaN/negativ -> 0. Und M3/M7: der
/// Wert kommt aus **fremden** Daten, also gedeckelt — `MAX_LAENGE` ist der
/// Name fuer diese Grenze.
pub(crate) fn laenge_lesen(hole: &mut dyn FnMut(&str) -> crate::heap::GVal) -> usize {
    use crate::heap::GVal;
    let n = match hole("length") {
        GVal::Zahl(n) if n.is_finite() && n > 0.0 => n,
        _ => return 0,
    };
    (n.min(MAX_LAENGE as f64)) as usize
}

// =====================================================================
// `Array.from(items, mapfn, thisArg)` — die zwei Regeln fuer `mapfn`
//
// GEMESSEN, 00:33, fuenf rote test262-Faelle in `built-ins/Array/from`, und
// sie zerfallen in **zwei** Ursachen mit **einer** Zeile Ursache je:
// =====================================================================

/// Ist `mapfn` nach ECMA-262 23.1.2.12 gueltig, und wenn nicht: warum nicht?
///
/// DIE REGEL (ECMA-262 23.1.2.12, `Array.from`, Schritt 5):
///
/// ```text
/// If mapfn is not undefined, then
///   a. If IsCallable(mapfn) is false, throw a TypeError exception.
/// ```
///
/// Das Wort, das alles traegt, ist **„nicht undefined"**. `mapfn` wird
/// **nicht** aufgerufen, wenn es `undefined` ist — es wird **geprueft**, wenn
/// es etwas anderes ist. `null` ist nicht `undefined`, also ein `TypeError`.
///
/// `node 22`, 00:33, alle vier gemessen:
///
/// ```text
/// Array.from([], null)            -> TypeError
/// Array.from([], Symbol('1'))     -> TypeError
/// Array.from([1,2], undefined)    -> [1,2]      (kein Fehler)
/// ```
///
/// WAS ICH VORHER HATTE, und warum es die Fehler nicht erklaert: ein
/// `if ist_aufrufbar(mapfn)` — ein **nicht aufrufbares** `mapfn` wurde
/// **stillschweigend ignoriert**. Das ist ein *anderes* Verhalten als
/// „kein `mapfn`", und es ist still: bei `Array.from([1,2], null)` gibt node
/// einen `TypeError` und wir `[1,2]`.
///
/// M3: Die Frage kommt aus fremdem Code, aber sie hat **keine** Schleife und
/// keine Allokation — die Antwort ist ein `bool`. Also kein `MAX_...`.
pub(crate) fn mapfn_ist_gueltig(ist_undefiniert: bool, ist_aufrufbar: bool) -> bool {
    // GEGEN LESEN, sonst ist es falsch: gueltig ist es, wenn `mapfn`
    // **undefined** ist **oder** aufrufbar. Beide Faelle heissen „kein
    // Fehler". Der Fehler ist der **dritte** Fall: nicht undefined und nicht
    // aufrufbar — also `null`, `42`, `Symbol()`, ein Objekt.
    //
    // (Ich hatte zuerst `!ist_undefiniert || ist_aufrufbar` stehen. Das
    // ist die Umkehrung, und der Test `mapfn_nur_bei_undefined_ist_erlaubt`
    // ist bei `(true, false)` sofort rot geworden — genau da, wo man
    // `undefined` als „nichts zu tun" liest.)
    ist_undefiniert || ist_aufrufbar
}

/// Wie viele Argumente bekommt der `mapfn` von `Array.from`? **Zwei.**
///
/// ECMA-262 23.1.2.12 Schritt 6: „let mappedValue be `? Call(mapfn, thisArg,
/// « kValue, 𝔽(k) »)" — zwei Elemente, kein drittes.
///
/// GEMESSEN, `node 22` (00:33), und der Grund, warum das **beobachtbar**
/// ist: der Ruecklauf kann `arguments.length` zaehlen.
///
/// ```text
/// Array.from([1], function(){ ... arguments.length ... })  -> 2
///   Argumente: [ 1, 0 ]
/// thisArg: [7]
/// ```
///
/// DER UNTERSCHIED ZU `Array.prototype.map`, und er ist der Grund fuer die
/// getrennte Konstante: `map` ruft mit **drei** Argumenten (Wert, Index, Feld).
/// `Array.from` ruft mit **zwei**. Wer eine gemeinsame Zahl benutzt, baut einen
/// Aufruf, der fuer einen der beiden falsch ist — und `arguments.length` macht
/// das sichtbar.
///
/// M3: Konstante; kein Schrittzaehler noetig, die Zahl steht als Name da.
pub(crate) const MAPFN_ARGUMENTZAHL: usize = 2;

// =====================================================================
// `ArraySetLength` — die Gueltigkeitspruefung fuer `length`
//
// GEMESSEN, 01:10, `node 22`, **zehn** Eingaben, nicht geraten:
//
//     [].length = 4294967296   -> RangeError
//     [].length = -1           -> RangeError
//     [].length = 1.5           -> RangeError      <-- nicht 1!
//     [].length = NaN           -> RangeError
//     [].length = 1e21          -> RangeError
//     defineProperty([], "length", {value: -1})  -> RangeError
//     [].length = 1.0           -> 1
//     [].length = "3"           -> 3
//     [].length = true          -> 1
//     [].length = -0            -> 0
//
// DIE REGEL, und sie ist **ein Vergleich** (ECMA-262 10.4.2.4,
// `ArraySetLength`, Schritt 1 d):
//
//     Let newLen be ? ToUint32(Desc.[[Value]]).
//     Let numberLen be ? ToNumber(Desc.[[Value]]).
//     If newLen is not numberLen, throw a RangeError exception.
//
// Zwei Umwandlungen, ein Vergleich. Das erklaert **alle** zehn Zeilen oben
// ohne eine Ausnahme:
//
// | Eingang | `ToUint32` | `ToNumber` | gleich? |
// |---|---|---|---|
// | `2^32`  | 0 | 4294967296 | nein -> RangeError |
// | `-1`    | 4294967295 | -1 | nein |
// | `1.5`   | 1 | 1.5 | nein -> **deshalb** kein 1 |
// | `NaN`   | 0 | NaN | nein (NaN ist nie gleich) |
// | `1e21`  | 0 | 1e21 | nein |
// | `1.0`   | 1 | 1 | ja |
// | `"3"`   | 3 | 3 | ja |
// | `true`  | 1 | 1 | ja |
// | `-0`    | 0 | -0 | **ja**, weil `0 === -0` in JS |
//
// DER PUNKT MIT `-0`: in Rust ist `-0.0 == 0.0` ebenfalls `true`, der
// Vergleich verhaelt sich also richtig. Das ist einer der seltenen Faelle,
// an denen Rust und JS in dieselbe Richtung luegen, und es waere ein Grund,
// es zu pruefen statt es anzunehmen.
//
// WAS ICH VORHER HATTE, und warum es die Fehler nicht erklaert:
// `let n = self.zu_uint32(wert)? as usize;` — das **schneidet**. Aus `-1`
// wurde `4294967295`, aus `1.5` wurde `1`, aus `NaN` wurde `0`. Danach kam
// unsere Speichergrenze, also ein **Speicherfehler** statt `RangeError` —
// und `S15.4.5.2_A3_T4.js` meldete genau das: `Speichergrenze: Feld-Laenge zu
// gross` statt des erwarteten `RangeError`.
// =====================================================================

/// Prueft einen Wert fuer `Array.prototype.length`.
///
/// `to_uint32` und `als_zahl` sind die **beiden** Umwandlungen der Norm —
/// sie werden getrennt uebergeben, weil sie im Motor verschiedene Wege gehen
/// (`zu_zahl` darf werfen, `als_uint32` rechnet nur). `Ok(n)` heisst
/// „gueltig, und das ist die Laenge", `Err(())` heisst „`RangeError`".
///
/// WARUM `Err(())` und nicht `Result<u32, String>`: die Meldung entsteht an
/// der Aufrufstelle, weil sie dort den `RangeError`-Wert baut. Hier steht nur
/// die Entscheidung — und eine Funktion, die eine Zeichenkette liefert, die
/// niemand liest, ist eine zweite Wahrheit ueber denselben Text.
///
/// M3: `als_zahl` kommt aus fremdem Code. Der Vergleich ist billig und
/// allokiert nichts; es gibt keine Schleife und damit keinen `MAX_...`.
pub(crate) fn laenge_pruefen(to_uint32: u32, als_zahl: f64) -> Result<u32, ()> {
    // `newLen is not numberLen`. In Rust: `!=`. Fuer `NaN` ist `!=` immer
    // wahr (`NaN != NaN`), und genau das will die Norm — `NaN` ist ungueltig.
    // Fuer `-0` ist `0.0 != -0.0` **falsch**, `-0` ist also gueltig.
    if (to_uint32 as f64) != als_zahl {
        return Err(());
    }
    Ok(to_uint32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M3: Die Tabelle ist die Quelle, und `Array.from` ist der gemessene Fall.
    #[test]
    fn array_from_hat_eine_parameterzahl() {
        assert_eq!(
            parameterzahl(&Nativ::FeldVon),
            1,
            "M3: `Array.from.length` ist 1 nach ECMA-262 17.2.3.1 (der zweite \
Parameter `mapfn` ist optional)."
        );
    }

    /// Und `Array.of.length` ist **0** — dieselbe Normregel, anderes Ergebnis.
    /// Ohne diese Zeile koennte die Tabelle `from` und `of` verwechseln.
    #[test]
    fn array_of_hat_null_parameter() {
        assert_eq!(
            parameterzahl(&Nativ::FeldOf),
            0,
            "M3: `Array.of.length` ist 0 — `Array.of(...items)` nimmt nur \
Rest-Parameter."
        );
    }

    /// Der Fall, den die Suite am lautesten prueft: `JSON.parse.length === 1`.
    #[test]
    fn json_parse_hat_zwei_parameter() {
        assert_eq!(
            parameterzahl(&Nativ::JsonLesen),
            2,
            "GEMESSEN an test/built-ins/JSON/parse/length.js: value 2. Meine \
erste Fassung stand auf 1, mit der Begruendung 'reviver ist optional' — das \
war geraten, und die Suite sagt es anders."
        );
    }

    /// Und der zweite korrigierte Wert. `stringify` war in meiner ersten
    /// Fassung ebenfalls 1.
    #[test]
    fn json_stringify_hat_drei_parameter() {
        assert_eq!(
            parameterzahl(&Nativ::JsonSchreiben),
            3,
            "GEMESSEN an test/built-ins/JSON/stringify/length.js: value 3."
        );
    }

    // -----------------------------------------------------------------
    // Die Kappung — und hier ist der Test **fall-faehig**
    // -----------------------------------------------------------------

    /// Ein Leser, der immer dieselbe Zahl meldet.
    fn hole_mit(l: f64) -> impl FnMut(&str) -> crate::heap::GVal {
        move |_k: &str| crate::heap::GVal::Zahl(l)
    }

    /// M3: `length: 1e9` ergibt **nicht** 10^9 Indizes, sondern `MAX_LAENGE`.
    ///
    /// DIESER TEST KANN FALLEN, und das ist der Grund, warum er hier steht
    /// statt im Integrationstest: `tests/js2_eigen.rs` behauptet, das Programm
    /// breche ab — das stimmt auch **ohne** Kappung, weil der alte Weg das
    /// Objekt mit einem `TypeError` ablehnt. Gemessen am 03.10.2026: dieser
    /// Unit-Test ist gruen, der Integrationstest waere es auch.
    ///
    /// DIE GEGENPROBE, mit der ich das geprueft habe: `.min(MAX_LAENGE as f64)`
    /// aus `laenge_lesen` entfernt -> `1_000_000_000` statt `4_000_000`, der
    /// Test schlaegt fehl.
    #[test]
    fn max_laenge_begrenzt_die_laenge() {
        assert_eq!(
            laenge_lesen(&mut hole_mit(1e9)),
            MAX_LAENGE,
            "M3: `length: 1e9` darf keine 10^9-Schleife ausloesen. Ohne den \
Deckel waere das eine Allokation von rund 24 GB."
        );
    }

    /// Und die **kleinen** Laengen kommen **unveraendert** durch — die Kappung
    /// darf keinen normalen Wert veraendern.
    #[test]
    fn kleine_laengen_bleiben_unveraendert() {
        for l in [1.0, 2.0, 3.0, 1000.0] {
            assert_eq!(
                laenge_lesen(&mut hole_mit(l)),
                l as usize,
                "M3: der Deckel ist fuer fremde Grossen, nicht fuer normale."
            );
        }
    }

    /// ECMA-262 6.1.6.1 `ToLength`: was **keine** endliche positive Zahl ist,
    /// ergibt 0 — und das ist der Weg, ueber den `undefined` (also ein Objekt
    /// ohne `length`) zu „leer" wird.
    #[test]
    fn ToLength_macht_aus_ungueltigen_werden_null() {
        for l in [f64::NAN, f64::INFINITY, -1.0, -0.0, 0.0] {
            assert_eq!(
                laenge_lesen(&mut hole_mit(l)),
                0,
                "ECMA-262 6.1.6.1: `ToLength` von einem Wert <= 0 oder NaN ist 0. \
                 Gemessen fuer {l}."
            );
        }
    }

    /// `elemente_lesen` benutzt **dieselbe** Laenge wie `laenge_lesen`.
    ///
    /// Ohne diese Zeile koennte jemand `elemente_lesen` wieder die alte,
    /// eigene `ToLength`-Kopie geben (das war mein erster Stand, und der
    /// Grund fuer diesen Test): die Indizes und die Elemente waeren dann
    /// unterschiedlich lang, und `every` wuerde am Ende `true` sagen, ohne
    /// alle Elemente gesehen zu haben.
    #[test]
    fn elemente_und_indizes_haben_dieselbe_laenge() {
        let mut h1 = hole_mit(7.0);
        let mut h2 = hole_mit(7.0);
        let e = elemente_lesen(&mut h1).unwrap_or_default();
        let l = laenge_lesen(&mut h2);
        assert_eq!(
            e.len(),
            l,
            "zwei Kopien derselben Regel wuerden auseinanderlaufen"
        );
        assert_eq!(e.len(), 7, "und beide muessen `length: 7` ergeben");
    }

    /// DIE LISTE DER UNGEMESSENEN ist **Dokumentation**, kein Nachschlagewerk.
    ///
    /// Sie sagt, fuer welche Namen die Suite keine `length.js` hat und die
    /// deshalb 0 tragen. Zwei Faenge, die man pruefen kann, ohne den fremden
    /// test262-Baum zu lesen:
    ///
    /// * jeder Eintrag ist **voll qualifiziert** (mit `Array.`/`String.`/
    ///   `Map.`/`Set.` davor) — ein kurzer Name waere mehrdeutig, und das ist
    ///   der Grund, warum es keine Funktion `parameterzahl_von_name` gibt
    ///   (siehe die Datei-Doku oben);
    /// * keine Dubletten — ein Name zweimal waere eine stille Auskunft, die nur
    ///   an einer Stelle zaehlt.
    ///
    /// WAS ICH **NICHT** BEHAUPTE: dass die Suite fuer keinen dieser Namen ein
    /// `length.js` hat. Das habe ich um 22:55 gemessen, und es kann sich mit der
    /// Suite aendern. Diese Behauptung ist an den Baum gebunden, nicht an den
    /// Code, und der Code soll laenger leben als die Messung.
    #[test]
    fn die_ungemessene_liste_ist_voll_qualifiziert_und_ohne_dubletten() {
        for (i, a) in UNGEMESSENE_PARAMETERZAHL.iter().enumerate() {
            assert!(
                a.starts_with("Array.")
                    || a.starts_with("String.")
                    || a.starts_with("Map.")
                    || a.starts_with("Set."),
                "`{a}` ist nicht voll qualifiziert — ein kurzer Name ist \
                 mehrdeutig (`concat` gibt es bei Array und String)"
            );
            for b in &UNGEMESSENE_PARAMETERZAHL[i + 1..] {
                assert_ne!(a, b, "`{a}` steht zweimal in UNGEMESSENE_PARAMETERZAHL");
            }
        }
    }

    // -----------------------------------------------------------------
    // `mapfn` — die zwei Regeln
    // -----------------------------------------------------------------

    /// Die Gueltigkeitspruefung, **alle vier** Faelle aus `node 22`.
    ///
    /// DIE FORM DER ALTEN FASSUNG ist der Test: sie war
    /// `if ist_aufrufbar(mapfn) { ... aufrufen ... }` — ein nicht aufrufbares
    /// `mapfn` wurde also **stillschweigend** uebersprungen, und das Ergebnis
    /// war `[1,2]` statt eines `TypeError`. Diese Funktion sagt `false`, und
    /// der Aufrufer wirft.
    #[test]
    fn mapfn_nur_bei_undefined_ist_erlaubt() {
        // (ist_undefiniert, ist_aufrufbar) -> gueltig?
        assert!(
            mapfn_ist_gueltig(true, false),
            "undefined: kein Ruecklauf, aber auch kein Fehler"
        );
        assert!(
            mapfn_ist_gueltig(true, true),
            "undefined: ob aufrufbar oder nicht, ist gleichgueltig"
        );
        assert!(mapfn_ist_gueltig(false, true), "eine Funktion ist gueltig");
        assert!(
            !mapfn_ist_gueltig(false, false),
            "alles andere, was nicht aufrufbar ist, ist ein TypeError — \
             node 22 (00:33): Array.from([], null) und Array.from([], \
             Symbol('1')) werfen beide"
        );
    }

    // -----------------------------------------------------------------
    // Die Regel, die um 00:55 einen Fehlschluss ersetzt hat
    // -----------------------------------------------------------------

    /// `charAt`, `charCodeAt` und die anderen elf — **Suite-Beleg**.
    ///
    /// DER FEHLSCHLUSS, den dieser Test verhindert: ich hatte diese Namen auf
    /// `UNGEMESSENE_PARAMETERZAHL` gepackt, weil es **keine
    /// `charAt/length.js`** gibt. Die Begruendung war „die Zahl weiss
    /// niemand" — und die Suite wusste sie sehr wohl, nur in
    /// `String/prototype/charAt/S15.5.4.4_A11.js`:
    ///
    /// ```text
    /// if (String.prototype.charAt.length !== 1) { throw new Test262Error(…); }
    /// ```
    ///
    /// Die beiden Quellen, 00:52/00:55: `node 22` und diese Assertion —
    /// **beide** sagen 1. Und der Fall war rot: gemeldet wurde
    /// `String.prototype.charAt.length === 1. Actual: 0`.
    ///
    /// DER TEST: die Zahl steht in der Tabelle. Faellt er, ist eine Zahl
    /// verschwunden — nicht „die Regel".
    #[test]
    fn die_zwoelf_aus_der_zweiten_extraktion() {
        for (nativ, soll, quelle) in [
            (Nativ::TextZeichenAn, 1u32, "charAt"),
            // (die `1u32` oben typisiert das Array; ohne sie nimmt Rust
            // `i32` und der Vergleich mit `u32` passt nicht)
            (Nativ::TextZeichenKode, 1, "charCodeAt"),
            (Nativ::TextIndexVon, 1, "indexOf"),
            (Nativ::TextLetzterIndex, 1, "lastIndexOf"),
            (Nativ::TextVerketten, 1, "concat"),
            (Nativ::TextTreffer, 1, "match"),
            (Nativ::TextSuchen, 1, "search"),
            (Nativ::TextVergleich, 1, "localeCompare"),
            (Nativ::TextScheibe, 2, "slice"),
            (Nativ::TextTeil, 2, "substring"),
            (Nativ::TextKlein, 0, "toLowerCase"),
            (Nativ::TextGross, 0, "toUpperCase"),
        ] {
            assert_eq!(
                parameterzahl(&nativ),
                soll,
                "{quelle}.length === {soll} — Beleg: X.length !== {soll} in der \
                 Suite, gegengemessen an node 22 (00:52). Beide Quellen sagen \
                 dasselbe."
            );
        }
    }

    /// Und die Warnliste traegt **keinen** davon mehr.
    ///
    /// DAS IST DER EIGENTLICHE TEST gegen den Fehlschluss: nicht „die Zahl
    /// stimmt", sondern „der Name steht nicht mehr auf der Liste". Eine Zahl
    /// in der Tabelle **und** der Name auf der Liste waere genau der Zustand,
    /// der mich um 00:55 getaeuscht hat — beides gleichzeitig behauptet.
    #[test]
    fn keiner_der_zwoelf_steht_auf_der_warnliste() {
        for name in [
            "String.prototype.charAt",
            "String.prototype.charCodeAt",
            "String.prototype.indexOf",
            "String.prototype.lastIndexOf",
            "String.prototype.concat",
            "String.prototype.match",
            "String.prototype.search",
            "String.prototype.localeCompare",
            "String.prototype.slice",
            "String.prototype.substring",
            "String.prototype.toLowerCase",
            "String.prototype.toUpperCase",
            "String.prototype.substr",
        ] {
            assert!(
                !UNGEMESSENE_PARAMETERZAHL.contains(&name),
                "`{name}` steht auf UNGEMESSENE_PARAMETERZAHL **und** hat einen \
                 Wert in der Tabelle. Beides gleichzeitig zu behaupten ist der \
                 Zustand, der mich um 00:55 getaeuscht hat: ich habe die Zahl \
                 fuer nicht existent gehalten, weil es keine `length.js` gibt."
            );
        }
    }

    // -----------------------------------------------------------------
    // `length` — die Regel aus zwei Umwandlungen und einem Vergleich
    // -----------------------------------------------------------------

    /// Die zehn Zeilen aus `node 22` (01:10), eine nach der anderen.
    ///
    /// DIE ZEILE MIT `1.5` IST DER INTERESSANTE FALL: sie ist der Grund fuer
    /// die ganze Regel. `ToUint32(1.5)` ist `1`, und wer nur `zu_uint32`
    /// benutzt, behaeltet `1` — also **kein** Fehler. Node wirft. Die Norm
    /// vergleicht `ToUint32` gegen `ToNumber`, und `1 != 1.5`.
    #[test]
    fn length_regel_alle_zehn_zeilen() {
        // (eingabe, to_uint32, als_zahl, gueltig?)
        let faelle: &[(f64, u32, f64, bool)] = &[
            (4294967296.0, 0, 4294967296.0, false),
            (-1.0, 4294967295, -1.0, false),
            (1.5, 1, 1.5, false),
            (f64::NAN, 0, f64::NAN, false),
            (1e21, 0, 1e21, false),
            (1.0, 1, 1.0, true),
            (3.0, 3, 3.0, true), // der Fall von "3"
            (1.0, 1, 1.0, true), // der Fall von true
            (-0.0, 0, -0.0, true),
        ];
        for (eingabe, u32, zahl, gueltig) in faelle {
            assert_eq!(
                laenge_pruefen(*u32, *zahl).is_ok(),
                *gueltig,
                "length = {eingabe}: ToUint32={u32}, ToNumber={zahl} —                  node 22 (01:10) sagt {}",
                if *gueltig { "gueltig" } else { "RangeError" }
            );
        }
        // Und der gueltige Fall liefert genau die Zahl zurueck, die
        // `ToUint32` ergab — nicht die Zahl und nicht die andere.
        assert_eq!(laenge_pruefen(7, 7.0), Ok(7));
    }

    /// `-0` ist gueltig, weil `0 === -0` in JS gilt — und in Rust auch.
    ///
    /// DAS IST EIN EIGENER TEST, weil es der einzige Fall ist, an dem ein
    /// „ausschliesslich"-Vergleich (`>` statt `!=`) das **richtige** Ergebnis
    /// liefert. Wer die Regel als Bereichspruefung schreibt
    /// (`0 <= n <= 2^32-1`), bekommt bei `-0` zwar auch 0 — aber bei `1.5`
    /// wieder die falsche Antwort. Der Vergleich ist nicht kuerzer, er ist
    /// **richtig**.
    #[test]
    fn minus_null_ist_eine_gueltige_laenge() {
        assert_eq!(laenge_pruefen(0, -0.0), Ok(0));
        // Und die Rust-Grundsaetze, damit der Vergleich nicht zufaellig ist:
        assert!(0.0 == -0.0, "`0.0 == -0.0` muss in Rust gelten");
        assert!(
            !(0.0 != -0.0),
            "und damit `!=` false — die Klammern, weil`
             Rust keine verketteten Vergleiche erlaubt"
        );
        assert!(
            0.0f64 != f64::NAN,
            "`NaN` ist nie gleich — das ist der \
             Grund, warum `NaN` ungueltig ist"
        );
    }

    /// Und die Argumentzahl — **zwei**, nicht drei.
    #[test]
    fn mapfn_bekommt_zwei_argumente() {
        assert_eq!(
            MAPFN_ARGUMENTZAHL, 2,
            "ECMA-262 23.1.2.12 Schritt 6: `Call(mapfn, thisArg, « kValue, \
             𝔽(k) »)` — zwei Elemente. `Array.prototype.map` dagegen drei."
        );
    }
}

// =====================================================================
// `copyWithin`: ROHLÄNGE fuer das Klemmen, DECKEL fuer die Schleife
// =====================================================================

/// M3: die Obergrenze der **kopierten** Elemente. `MAX_LAENGE` (4 000 000)
/// ist die Obergrenze einer *gelesenen* Länge; `copyWithin` braucht eine
/// eigene, weil es nicht liest, sondern **schreibt**.
///
/// DER UNTERSCHIED, und er ist der ganze Punkt (03.10.2026, 03:40,
/// `copyWithin/length-near-integer-limit.js`): `count` haengt an
/// `min(final - from, len - to)`, und `len` ist bei diesem Fall
/// `Number.MAX_SAFE_INTEGER` — **9 007 199 254 740 991**. Ohne Deckel
/// waere das eine Schleife ueber 9·10^15 Elemente aus fremdem Input.
pub(crate) const MAX_KOPIEREN: usize = 4_000_000;

/// Die `length` **ohne** Deckel — nur zum Klemmen der drei Argumente.
///
/// ## WARUM EINE EIGENE REGEL und nicht `laenge_lesen`
///
/// `laenge_lesen` deckelt auf `MAX_LAENGE` (4 000 000). Fuer das **Lesen**
/// ist das richtig: es speichert die Elemente in einem `Vec`. Fuer
/// `copyWithin` waere es **falsch**, und der Beweis ist der Test selbst:
///
/// ```text
/// var startIndex = Number.MAX_SAFE_INTEGER - 3;      // 9007199254740991 - 3
/// var arrayLike = { 0:0, 1:1, 2:2, length: Number.MAX_SAFE_INTEGER };
/// arrayLike[startIndex]     = -3;
/// arrayLike[startIndex + 2] = -1;
/// Array.prototype.copyWithin.call(arrayLike, 0, startIndex, startIndex + 3);
/// // arrayLike[0] muss -3 sein
/// ```
///
/// `RelativeIndex(9007199254740991, len)` ist `min(9007199254740991, len)`.
/// Mit `len = 4 000 000` waere `from = 4 000 000` und `final = 4 000 000`,
/// also `count = min(0, 4 000 000) = 0` — **nichts** wird kopiert, und
/// `arrayLike[0]` bleibt `0` statt `-3`. Der Deckel hat nicht den Speicher
/// geschuetzt, sondern das **Ergebnis** zerstoert.
///
/// ## DIE TRENNUNG, UM DIE ES GEHT
///
/// * **Klemmen: roh.** `RelativeIndex` rechnet mit der echten Laenge; das
///   sind drei Rechenoperationen, keine Schleife (M3: keine Allokation).
/// * **Schleife: gedeckelt.** `count = min(final - from, len - to)` ist
///   durch `MAX_KOPIEREN` begrenzt. `count` kann 9·10^15 sein, wenn
///   jemand `copyWithin.call({length: 1e15}, 0, 0)` aufruft.
///
/// ## DIE FALLEN, DIE DIESE REGEL ABFANGEN SOLL
///
/// | Eingabe | `len` | `count` | geklemmt auf |
/// |---|---|---|---|
/// | `{length: 1e15}`, `copyWithin(0,0)` | 10^15 | 10^15 | `MAX_KOPIEREN` |
/// | `MAX_SAFE_INTEGER`, `copyWithin(0, s, s+3)` | 2^53−1 | **3** | 3 |
/// | `[1,2,3]`, `copyWithin(0,1)` | 3 | 2 | 2 |
/// | `{length: "x"}` | 0 | 0 | 0 |
///
/// ## WAS ICH **NICHT** BEHAUPTE
///
/// Dass `MAX_KOPIEREN == MAX_LAENGE` **richtig** ist. Es ist eine
/// Festlegung, und sie ist an **einer** Stelle getroffen, damit die beiden
/// Zahlen nicht auseinanderlaufen. Wer eine davon aendert, sollte die andere
/// mitdenken — dieselbe Regel, die in `elemente_lesen` steht (dort mit der
/// begruendeten Erinnerung an N49: dort fehlte genau ein `match`-Arm einer
/// zweiten Kopie, und der Test blieb rot, ohne dass jemand es merkte).
///
/// ## DER RUECKGABETYP, und warum er `Result` ist (03.10.2026, 04:20)
///
/// `LengthOfArrayLike(O)` ist `ToLength(Get(O, "length"))`, und **beide**
/// duerfen werfen: `Get` ueber einen Getter, `ToLength` ueber ein Symbol.
/// Mein erster Entwurf nahm `unwrap_or(GVal::Undefiniert)` im Aufrufer, also
/// `len = 0` — und `copyWithin` tat **nichts**, statt zu werfen. Zwei rote
/// test262-Faelle genau darum (`return-abrupt-from-this-length.js` und
/// `...-this-length-as-symbol.js`).
///
/// **DIE LEHRE, und sie ist die vierte in dieser Datei:** ein
/// `unwrap_or(Default::default())` um einen `Result` herum ist die
/// schnellste Art, eine Normvorschrift zu einer stillen Nichts-Operation zu
/// machen. Wer `?` schreiben kann, sollte es schreiben.
/// Die **Zahlenregel** von `ToLength` (ECMA-262 6.1.6.1 Schritt 2 und 3) —
/// **herausgeloest**, damit der Aufrufer, der den Heap hat, sie benutzen kann,
/// ohne sie zu wiederholen.
///
/// **WARUM DAS EIN EIGENER HELFER IST und nicht in `laenge_roh_lesen` bleibt:**
/// `laenge_roh_lesen` ist eine **freie Funktion** mit einem `FnMut`-Leser und
/// laesst sich ohne Motor pruefen — ihre drei Unit-Tests brauchen keinen. Der
/// Aufrufer in `builtin.rs` **hat** den Motor, muss aber `ToNumber` auf dem
/// Laengenwert **anwenden** (damit `valueOf` laeuft und Wuerfe
/// propagateieren). *Wer die Regel einmal schreibt, schreibt sie richtig;
/// wer sie zweimal schreibt, hat sie einmal falsch geschrieben.*
pub(crate) fn laenge_von_zahl(n: f64) -> f64 {
    // Schritt 2: NaN, +0, -0, negative und unendliche Zahlen -> +0.
    if !n.is_finite() || n <= 0.0 {
        return 0.0;
    }
    // Schritt 3: `min(max(ToInteger(n)), 2^53-1)`. `2^53 - 1` ist
    // `Number.MAX_SAFE_INTEGER` — die Obergrenze, weil `length` eine
    // vorzeichenlose 32-Bit-Groesse **oder** eine
    // `Number.MAX_SAFE_INTEGER`-Beschraenkung traegt.
    n.trunc().min(9_007_199_254_740_991.0_f64)
}

pub(crate) fn laenge_roh_lesen(
    hole: &mut dyn FnMut(&str) -> Result<crate::heap::GVal, crate::interp::Stoerung>,
) -> Result<f64, crate::interp::Stoerung> {
    use crate::heap::GVal;
    let n = match hole("length")? {
        GVal::Zahl(n) => n,
        // NICHG number (und nicht durchgereicht): bis Runde 76 gab es hier
        // `0.0` zurueck, ohne dass der `valueOf`/`toString` des Laengenwerts
        // **jemals lief** — siehe `builtin.rs`, Aufrufstelle von Runde 76.
        _ => return Ok(0.0),
    };
    Ok(laenge_von_zahl(n))
}

/// Der `count` aus ECMA-262 23.1.3.28 Schritt 7 — **gedeckelt** (M3).
///
/// `count = min(final - from, len - to)`. `count` kommt aus fremden Daten,
/// also ist der Deckel `MAX_KOPIEREN` Pflicht; und `count <= len` ist per
/// Konstruktion nicht garantiert, wenn `len` roh (also 2^53−1) war.
pub(crate) fn copywithin_zaehlung(len: f64, ziel: f64, von: f64, bis: f64) -> usize {
    // DIE DREI PRUEFUNGEN, und sie sind nicht uebertrieben.
    //
    // GEMESSEN (03:45, Rust `f64::min`): `1.0_f64.min(f64::NAN)` ist
    // **`1.0`**, nicht `NaN`. `min` gibt den nicht-NaN-Operanden zurueck —
    // es **propagiert NaN nicht**. Ein `count` aus einer NaN-Laenge waere
    // also stillschweigend eine **echte** Zahl geworden, und die Schleife
    // haette sie ausgefuehrt.
    //
    // `laenge_roh_lesen` liefert nie `NaN` (es gibt `0.0` zurueck), also ist
    // der Vertrag tatsaechlich eingehalten. Die Pruefung steht trotzdem da:
    // `copywithin_zaehlung` ist eine **freie Funktion**, und ein freier
    // Aufrufer ist nicht an die Vorbedingung gebunden. Eine Funktion, die
    // eine Vorbedingung stillschweigend annimmt, ist eine, bei der der
    // naechste Aufrufer sie nicht kennt.
    if !(len.is_finite() && ziel.is_finite() && von.is_finite() && bis.is_finite()) {
        return 0;
    }
    let c = (bis - von).min(len - ziel);
    if c <= 0.0 {
        return 0;
    }
    (c.trunc() as u64).min(MAX_KOPIEREN as u64) as usize
}

#[cfg(test)]
mod tests_copywithin {
    use super::*;
    use crate::heap::GVal;

    /// Die **fuenf** Zeilen aus `length-near-integer-limit.js`, mit den
    /// Werten des Tests. Das ist der Fall, an dem der Deckel in
    /// `laenge_lesen` das Ergebnis zerstoert haette.
    #[test]
    fn rohl_haengt_bei_max_safe_integer() {
        // Der Aufbau des Tests, als `length`-Antwort.
        let mut hole = |k: &str| -> Result<GVal, crate::interp::Stoerung> {
            Ok(if k == "length" {
                GVal::Zahl(9_007_199_254_740_991.0)
            } else {
                GVal::Undefiniert
            })
        };
        let len = laenge_roh_lesen(&mut hole).expect("kein Fehler erwartet");
        assert_eq!(
            len, 9_007_199_254_740_991.0,
            "die ROHLÄNGE ist Number.MAX_SAFE_INTEGER"
        );
        // Und `laenge_lesen` wuerde hier 4 000 000 liefern — das ist der
        // ganze Unterschied, und deshalb sind es zwei Funktionen.
        let mut hole2 = |k: &str| -> GVal {
            if k == "length" {
                GVal::Zahl(9_007_199_254_740_991.0)
            } else {
                GVal::Undefiniert
            }
        };
        assert_eq!(
            laenge_lesen(&mut hole2),
            4_000_000,
            "der GEDEKELTE Wert — und genau deshalb darf er nicht zum Klemmen \
             benutzt werden"
        );
        // `RelativeIndex(startIndex, len)` = min(2^53-4, 2^53-1) = 2^53-4
        let s: f64 = 9_007_199_254_740_991.0 - 3.0;
        let von = s.min(len);
        let bis = (s + 3.0).min(len);
        let ziel = 0.0;
        assert_eq!(
            copywithin_zaehlung(len, ziel, von, bis),
            3,
            "`count = min(final - from, len - to)` ist **3** — drei Elemente, \
             nicht 9·10^15. Der Deckel muss am `count` haengen, nicht an der \
             Länge."
        );
    }

    /// Der Deckel **greift**, wenn er muss — der M3-Fall.
    #[test]
    fn der_deckel_greift_bei_riesiger_zaehlung() {
        assert_eq!(copywithin_zaehlung(1e15, 0.0, 0.0, 1e15), MAX_KOPIEREN);
        // Und die vier Faelle, in denen nichts passiert.
        assert_eq!(copywithin_zaehlung(3.0, 0.0, 2.0, 3.0), 1);
        assert_eq!(copywithin_zaehlung(3.0, 2.0, 0.0, 3.0), 1);
        assert_eq!(copywithin_zaehlung(3.0, 3.0, 0.0, 3.0), 0, "kein Platz");
        assert_eq!(copywithin_zaehlung(3.0, 0.0, 3.0, 3.0), 0, "leeres Fenster");
        // UND die drei nicht-endlichen Eingaben. `laenge_roh_lesen` liefert
        // nie `NaN`, aber `copywithin_zaehlung` ist frei aufrufbar — und
        // `1.0.min(NaN)` ist `1.0`, also waere eine NaN-Laenge still zu einer
        // **echten** Zaehlung geworden.
        assert_eq!(copywithin_zaehlung(f64::NAN, 0.0, 0.0, 1.0), 0, "NaN-Länge");
        assert_eq!(copywithin_zaehlung(f64::INFINITY, 0.0, 0.0, 1.0), 0);
        assert_eq!(copywithin_zaehlung(1.0, f64::NAN, 0.0, 1.0), 0, "NaN-Ziel");
        assert_eq!(copywithin_zaehlung(1.0, 0.0, f64::NAN, 1.0), 0, "NaN-Start");
        assert_eq!(copywithin_zaehlung(1.0, 0.0, 0.0, f64::NAN), 0, "NaN-Ende");
    }

    /// Und die **sechs** Formen von `length`, die `laenge_roh_lesen`
    /// beantworten muss — inklusive der, die `hole` nie liefert.
    #[test]
    fn sechs_laengen_formen() {
        // SECHS, und die letzten drei sind die interessanten: `NaN`, ein
        // Boolean und `undefined` — alle drei sind **keine** Zahlen, und
        // alle drei ergeben 0 (also "leeres Feld"), nicht einen Fehler.
        for (antwort, soll) in [
            (GVal::Zahl(0.0), 0.0),
            (GVal::Zahl(-1.0), 0.0),
            (GVal::Zahl(3.7), 3.0),
            (GVal::Zahl(f64::NAN), 0.0),
            (GVal::Bool(true), 0.0),
            (GVal::Undefiniert, 0.0),
        ] {
            let mut hole = |k: &str| -> Result<GVal, crate::interp::Stoerung> {
                Ok(if k == "length" {
                    antwort
                } else {
                    GVal::Undefiniert
                })
            };
            assert_eq!(
                laenge_roh_lesen(&mut hole).expect("kein Fehler erwartet"),
                soll,
                "length = {antwort:?} -> {soll}"
            );
        }
    }
}
