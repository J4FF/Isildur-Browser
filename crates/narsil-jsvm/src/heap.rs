// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Heap-Arena mit Mark-and-Sweep-GC (J5, M6).
//!
//! Begruendung: Closures erzeugen Zyklen (Funktion -> Umgebung ->
//! Funktion). `Rc`/`RefCell`-Zyklen wuerden dabei lecken (M6 verbietet
//! sie); deshalb Arena mit Indizes + Mark-and-Sweep. Alle Objekte leben
//! hinter [`HeapRef`]-Indizes; der Sweep gibt tote Slots frei.

use std::collections::HashMap;

/// Release 09.10.2026: ein schneller Hasher fuer die Eigenschaftskarten (Fx-
/// Verfahren: je 8 Byte drehen, mischen, multiplizieren). GEMESSEN vorher:
/// jeder Eigenschaftszugriff hashte den Namen mit SipHash (der Standard ist
/// gegen HashDoS gebaut, nicht auf Tempo) — bei Objektketten wie `a.b.c`
/// ein Grossteil der Zeit je Befehl.
#[derive(Default, Clone, Copy)]
pub(crate) struct SchnellHasher(u64);

const SCHNELL_K: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl SchnellHasher {
    #[inline]
    fn mischen(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(SCHNELL_K);
    }
}

impl std::hash::Hasher for SchnellHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut b = bytes;
        while b.len() >= 8 {
            self.mischen(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]));
            b = &b[8..];
        }
        if b.len() >= 4 {
            self.mischen(u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            b = &b[4..];
        }
        for &x in b {
            self.mischen(u64::from(x));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.mischen(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.mischen(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.mischen(i as u64);
    }
}

/// Die Eigenschaftskarte eines Objekts (Name -> Eigenschaft).
pub(crate) type Karte = HashMap<String, Eigenschaft, std::hash::BuildHasherDefault<SchnellHasher>>;

/// Referenz in die Arena (`u32::MAX` = leer). `Copy`, damit der
/// Interpreter sie frei herumreichen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct HeapRef(pub u32);

pub(crate) const KEIN_REF: HeapRef = HeapRef(u32::MAX);

/// Interner JS-Wert: inline Primitive oder Heap-Verweis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GVal {
    Undefiniert,
    Null,
    Bool(bool),
    Zahl(f64),
    /// J24: `BigInt` als `i128` (kein Heap-Slot, also vom GC nicht
    /// betroffen). Die Grenze `i128` ist dokumentiert (`ganz.rs`).
    Ganz(i128),
    Ref(HeapRef),
}

impl GVal {
    pub(crate) fn ist_ref(&self) -> bool {
        matches!(self, GVal::Ref(_))
    }
}

/// Umgebungs-Kette (lexikalisches Scoping + Closures). Jede Umgebung hat
/// einen Heap-Slot, damit Closures sie am Leben halten und der GC Zyklen
/// (Umgebung -> Funktion -> Umgebung) aufraeumen kann.
#[derive(Debug, Clone)]
pub(crate) struct Umgebung {
    pub eltern: Option<HeapRef>,
    /// J3b: Vec mit vorberechneten Namens-Hashes (Begruendung bei
    /// `Bindungen`).
    pub bindungen: Bindungen,
    /// J52: `with (obj) { … }` — der Rumpf sieht dieses **Objekt** als
    /// zusaetzliche Umgebung (ES2023 9.1.2.6 `Object Environment Record`).
    ///
    /// GEMESSEN (node 22), **dreissig** Zeilen (`js52_node.js`):
    ///   `var o = {a: 1}; with (o) { a }`            -> `1`
    ///   `var o = {a: 1}; with (o) { a = 7 } o.a`   -> `7`
    ///   `var o = {}; with (o) { b = 5 } o.b`       -> `undefined`!
    ///     (ein Name **ohne** Eigenschaft im Objekt geht nach aussen)
    ///   `var o = {a: 1}; with (o) { delete a }`     -> geloescht
    ///   `Symbol.unscopables` schliesst Namen aus
    pub objekt: Option<HeapRef>,
}

/// Der Hash eines Bezeichners (FNV-1a, 64 Bit).
///
/// J3b: GEMESSEN, warum es den Hash gibt. Vorher war der Schluessel einer
/// Bindung der **Text** (`HashMap<String, Bindung>`), und der Motor holte
/// den Text pro Zugriff sogar als **Kopie** (`fn name(i) -> String`). Ein
/// Zugriff kostete damit eine Allokation, einen String-Hash und zwoei
/// Kettenlaeufe: gemessen **56 ns je Zugriff**, und eine `for`-Schleife
/// hat acht davon je Durchlauf. Jetzt: der Hash wird **einmal** beim
/// Uebersetzen berechnet (`Funktion::namen_hash`), der Zugriff vergleicht
/// `u64` (und den Text nur noch, um einen Hash-Zusammenstoss
/// auszuschliessen).
/// sec-6.1.7 / OrdinaryOwnPropertyKeys: die **Zahlschluessel** (Array-Index)
/// kommen zuerst und aufsteigend, danach die uebrigen Schluessel in
/// Einfuegereihenfolge. `sort_by` ist stabil, die Einfuegereihenfolge der
/// nichtnumerischen Schluessel bleibt also erhalten.
///
/// GEMESSEN (J5): `Object.keys({b:1, 2:'zwei', a:3, 1:'eins'})` lieferte
/// `b,2,a,1` statt `1,2,b,a`, und `JSON.stringify({2:'a',1:'b',x:'c',0:'d'})`
/// lieferte `{"2":"a","1":"b",...}` statt `{"0":"d","1":"b","2":"a","x":"c"}`
/// (`tests/t262/faelle/obj-literal.js`, `obj-json-order.js`).
pub(crate) fn ordne_schluessel(keys: &mut [String]) {
    keys.sort_by(|a, b| vergleiche_schluessel(a, b));
}

/// Wie `ordne_schluessel`, aber fuer `(Name, aufzaehlbar)`-Paare (`for-in`
/// ueber die Kette, `Object.keys` je Objekt).
pub(crate) fn ordne_schluessel_paare(keys: &mut [(String, bool)]) {
    keys.sort_by(|(a, _), (b, _)| vergleiche_schluessel(a, b));
}

/// Wie [`ordne_schluessel_paare`], aber **ohne** Gleichstand bei
/// Nicht-Index-Schluesseln: die werden alphabetisch sortiert.
///
/// # W9 (04.10.2026, JS) — WOFUER
///
/// `ordne_schluessel_paare` vergleicht zwei Nicht-Index-Schluessel mit
/// `Ordering::Equal`. `sort_by` ist **stabil**, also bleibt ihre
/// Eingangsreihenfolge erhalten — und die kommt bei 32 von 35
/// Einfuegestellen aus einer `HashMap` (`RandomState`, pro Prozess neu
/// gesaet). GEMESSEN (zwei Laeufe, `built-ins/Object/assign/
/// strings-and-symbol-order.js`): einmal `[get sym(y), get sym(x), get b,
/// get a]`, einmal `[get sym(y), get b, get a, get sym(x)]`.
///
/// Diese Funktion wird dort benutzt, wo es **keine** Einfuegereihenfolge
/// gibt: `HObj::Verpackt`, `Funktion`, `Nativ`, `Fehler`, `Datum`, `Regex`,
/// `Karte`, `Menge`, `Versprechen` und die Nicht-Index-Felder von `HObj::
/// Feld`. Fuer `HObj::Gegenstand` ist die **Einfuegereihenfolge** die
/// Vorgabe der Spec (ES2023 20.1.7 OrdinaryOwnPropertyKeys) — dort darf
/// man nicht sortieren, sondern nur die Luecken sortieren.
pub(crate) fn ordne_schluessel_paare_strikt(keys: &mut [(String, bool)]) {
    keys.sort_by(|(a, _), (b, _)| {
        // ES2023 20.1.7 `OrdinaryOwnPropertyKeys`: Symbol-Schluessel stehen
        // **hinter** allen Text-Schluesseln. W9: sie liegen im Property-Map
        // unter `@sym<uid>` (`symbole::symbol_text`).
        match (ist_symbolschluessel(a), ist_symbolschluessel(b)) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => match vergleiche_schluessel(a, b) {
                std::cmp::Ordering::Equal => a.cmp(b),
                andere => andere,
            },
        }
    });
}

/// ES2023 20.1.7: die **Symbole** nach hinten schieben, die uebrige
/// Reihenfolge unangetastet lassen.
///
/// W9 (04.10.2026, JS): `schluessel_werte` und `eigene_schluessel` mischten
/// vorher Symbol- und Textschluessel, weil beide in **einer** `HashMap`
/// liegen. GEMESSEN (`Object.assign`, zwei Laeufe): einmal
/// `[b, a, sym(x), sym(y)]`, einmal `[a, sym(x), b, sym(y)]` — die Symbole
/// wanderten zwischen den Textschluesseln. `sort_by` ist stabil, deshalb
/// bleibt die Reihenfolge **innerhalb** der Gruppen erhalten (das ist die
/// Einfuegereihenfolge, die die Spec verlangt); nur die Gruppen kommen in
/// die richtige Reihenfolge.
pub(crate) fn symbole_nach_hinten(keys: &mut Vec<(String, bool)>) {
    keys.sort_by(
        |(a, _), (b, _)| match (ist_symbolschluessel(a), ist_symbolschluessel(b)) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => std::cmp::Ordering::Equal,
        },
    );
}

/// Ist der Schluessel ein Symbol?
///
/// W9 (04.10.2026, JS): im Property-Map gibt es **zwei** Schluesselformen
/// fuer Symbole, und beide muessen erkannt werden:
///   * `"Symbol(<Beschreibung>)"` — das ist, was `ops.rs::schluessel_text`
///     fuer einen berechneten Zugriff `o[sym] = 1` liefert (GEMESSEN dort:
///     `String(Symbol.iterator)` -> `"Symbol(Symbol.iterator)"`), und
///     davon lebt auch `Object.getOwnPropertyNames(Array.prototype)[0]`.
///   * `"@sym<uid>"` — das ist `symbole::symbol_text`, die Nummer der
///     wohlbekannten Symbole (Registry, `unscopables_text`).
pub(crate) fn ist_symbolschluessel(name: &str) -> bool {
    name.starts_with("Symbol(") || name.starts_with("@sym")
}

fn vergleiche_schluessel(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (
        crate::ops::index_aus_schluessel(a),
        crate::ops::index_aus_schluessel(b),
    ) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Ist `i` im Feld eine **Luecke** (kein Index)?
pub(crate) fn feld_ist_luecke(o: &HObj, i: usize) -> bool {
    match o {
        HObj::Feld { luecken, .. } => luecken.as_ref().is_some_and(|m| m.contains(&i)),
        _ => false,
    }
}

/// Index `i` zur Luecke machen (nur wenn das Feld dicht bis `i` reicht).
pub(crate) fn feld_luecke_markieren(o: &mut HObj, i: usize) {
    if let HObj::Feld {
        elemente, luecken, ..
    } = o
        && i < elemente.len()
    {
        luecken.get_or_insert_with(Default::default).insert(i);
    }
}

/// Luecke bei `i` aufheben (ein Schreibzugriff macht den Index wieder da).
pub(crate) fn feld_luecke_loeschen(o: &mut HObj, i: usize) {
    if let HObj::Feld { luecken, .. } = o {
        if let Some(m) = luecken.as_mut()
            && m.remove(&i)
            && m.is_empty()
        {
            *luecken = None;
        }
    }
}

/// Alle Indizes `von..bis` als Luecken markieren (Wachstum von `length`).
pub(crate) fn feld_luecken_von_bis(o: &mut HObj, von: usize, bis_exklusiv: usize) {
    if von >= bis_exklusiv {
        return;
    }
    if let HObj::Feld { elemente, luecken, .. } = o {
        // ## W51 (JS, 05.10.2026) — **NUR MATERIALISIERTE INDIZES KOENNEN LUECKE SEIN**
        //
        // **DER BEFUND — und er hat den LECK-WAECHTER ausgeloest.** Der Lauf
        // ueber die echte Suite wurde zweimal bei 2,5 GiB getoetet
        // (`signal: 9, SIGKILL`), beide Male in `built-ins/Array/length/`:
        //
        // ```text
        // var a = []; a.length = 4294967295;      // Array/length/15.4.5.1-3.d-3.js
        // ```
        //
        // Nach ES2023 10.4.2.4 (`ArraySetLength`) bleibt das Array dabei
        // **sparse**: es wird **nichts** alloziert. `eigenschaften.rs` macht
        // das richtig — fuer `n > MAX_SAMMLUNG` wird **nicht** materialisiert,
        // die grosse Laenge wandert nach `rest["length"]`. **ABER** der Aufruf
        // `feld_luecken_von_bis(&mut o, 0, 4_294_967_295)` lief **auch** im
        // `gross`-Zweig, und hier wurden **4 294 967 295 `usize`** in einen
        // `HashSet` gesteckt — ueber 100 GB angefordert, >60 MB/s. Das ist
        // der Leck.
        //
        // **DIE REGEL:** eine Luecke ist „ein Index, der im `elemente`-Vektor
        // steht, aber keinen Wert hat" (`feld_luecke_markieren` prueft genau
        // das: `if i < elemente.len()`). **Oberhalb von `elemente.len()` gibt es
        // nichts zu markieren** — und es ist auch nicht noetig:
        // `a[4294967294]` ist `undefined`, `4294967294 in a` ist `false`, und
        // `feld_ist_luecke` gibt dort ohnehin schon `false` zurueck, wenn kein
        // Eintrag existiert.
        let bis = bis_exklusiv.min(elemente.len());
        if von >= bis {
            return;
        }
        let m = luecken.get_or_insert_with(Default::default);
        for i in von..bis {
            m.insert(i);
        }
    }
}

/// Luecken oberhalb von `grenze` entfernen (`length` kleiner setzen).
pub(crate) fn feld_luecken_oberhalb_weg(o: &mut HObj, grenze: usize) {
    if let HObj::Feld { luecken, .. } = o
        && let Some(m) = luecken.as_mut()
    {
        let ueber: Vec<usize> = m.range(grenze..).copied().collect();
        for i in ueber {
            m.remove(&i);
        }
        if m.is_empty() {
            *luecken = None;
        }
    }
}

/// J8: Hash eines Schluessels fuer den Karte-/Mengen-Index.
///
/// MUSS zu `schluessel_gleich` (builtin3.rs) passen: gleiche Schluessel
/// muessen denselben Hash haben, verschiedene moeglichst verschiedene.
/// Insbesondere sind `1` und `'1'` **verschiedene** Schluessel
/// (SameValueZero + Textinhalt) — deshalb traegt jede Art ihre eigene
/// Kennung im Hash.
/// J11: ist der Name ein **privater** Klassenname (`#x`)?
///
/// GEMESSEN/ENTSCHIEDEN, warum es nur ein Namens-Praefix ist: der Motor
/// legt private Felder als normale Eigenschaft mit `#` im Schluessel ab
/// (der Compiler geht denselben Weg wie fuer einen Textnamen). Damit
/// greift an jeder **Aufzaehlung** (`Object.keys`, `JSON.stringify`,
/// `for…in`, `Object.entries`, `getOwnPropertyNames`) genau eine Regel:
/// `#` faellt weg.
///
/// Warum ueberhaupt auflisten: in node sind private Felder gar keine
/// eigenen Properties. GEMESSEN mit node 22 fuer `class C { #x = 1; y = 2 }`:
/// `JSON.stringify` -> `{"y":2}`, `Object.keys` -> `y`,
/// `getOwnPropertyNames` -> `y`, `'#x' in c` -> `false`.
/// Offen bleibt (im Bericht J11 benannt): ein **direkter** Zugriff
/// `c['#x']` liefert bei uns den Wert (bei node `undefined`).
pub(crate) fn ist_privat(name: &str) -> bool {
    name.starts_with('#')
}

pub(crate) fn schluessel_hash(heap: &Heap, v: GVal) -> u64 {
    match v {
        GVal::Zahl(n) => 0x9E37_0000_0000_0001_u64 ^ n.to_bits(),
        // J24: BigInt (die beiden Haelften gemischt, damit 2^32 und
        // 2^32+1 verschiedene Schluessel bekommen).
        GVal::Ganz(g) => {
            0xA5A5_0000_0000_0007_u64 ^ (g as u64) ^ ((g >> 32) as u64).rotate_left(17)
        }
        GVal::Bool(b) => 0xB0B0_0000_0000_0002_u64 ^ u64::from(b),
        GVal::Undefiniert => 0xC0C0_0000_0000_0003,
        GVal::Null => 0xD0D0_0000_0000_0004,
        GVal::Ref(r) => match heap.holen(r) {
            HObj::Zeichenkette(s) => 0xF0F0_0000_0000_0005_u64.wrapping_add(namens_hash(s)),
            // Objekte als Schluessel: der Slotted (nur dasselbe Objekt passt).
            _ => 0xFEED_0000_0000_0006_u64 ^ r.0 as u64,
        },
    }
}

pub(crate) fn namens_hash(name: &str) -> u64 {
    // FNV-1a: billig, ohne Tabelle, gut genug fuer wenige Eintraege.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Die Bindungen einer Umgebung: **Vec statt HashMap**.
///
/// J3b: GEMESSEN — eine Umgebung hat typisch 1 bis 5 Bindungen. Ein
/// linearer Scan ueber vorberechnete `u64`-Hashes kostet 2 bis 4 ns,
/// eine `HashMap<String, _>` mit SipHash 20 bis 25 ns. Der Text bleibt
/// daneben erhalten (Fehlertexte, `for-in`, eigene Namen).
/// Release 09.10.2026: Hasher fuer Schluessel, die schon ein Hash sind.
#[derive(Default, Clone, Copy)]
pub(crate) struct DurchHasher(u64);

impl std::hash::Hasher for DurchHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

/// Ab so vielen Bindungen fuehrt eine Umgebung einen Hash-Index.
const BINDUNGEN_INDEX_AB: usize = 16;

#[derive(Debug, Default, Clone)]
pub(crate) struct Bindungen {
    eintraege: Vec<(u64, String, Bindung)>,
    /// Release 09.10.2026: Hash -> Position (erst ab
    /// [`BINDUNGEN_INDEX_AB`] Eintraegen). GEMESSEN vorher: jede Namenssuche
    /// lief linear durch die Umgebung — der Hauptrumpf von youtube.com
    /// (Zehntausende `var`) machte damit jedes `LeseVar` und schon das
    /// Anlegen der Bindungen quadratisch (70 000 `var`: 14 s).
    index: Option<std::collections::HashMap<u64, usize, std::hash::BuildHasherDefault<DurchHasher>>>,
}

impl Bindungen {
    fn treffer(eh: u64, n: &str, h: u64, name: &str) -> bool {
        eh == h && (name.is_empty() || n.is_empty() || n == name)
    }

    fn position(&self, h: u64, name: &str) -> Option<usize> {
        if let Some(ix) = &self.index {
            if let Some(&p) = ix.get(&h) {
                let (eh, n, _) = &self.eintraege[p];
                if Self::treffer(*eh, n, h, name) {
                    return Some(p);
                }
            } else {
                return None;
            }
        }
        self.eintraege.iter().position(|(eh, n, _)| Self::treffer(*eh, n, h, name))
    }

    fn position_h(&self, h: u64) -> Option<usize> {
        match &self.index {
            Some(ix) => ix.get(&h).copied(),
            None => self.eintraege.iter().position(|(eh, _, _)| *eh == h),
        }
    }

    fn index_bauen(&mut self) {
        let mut ix: std::collections::HashMap<u64, usize, std::hash::BuildHasherDefault<DurchHasher>> =
            std::collections::HashMap::with_capacity_and_hasher(self.eintraege.len() * 2, Default::default());
        for (i, (h, _, _)) in self.eintraege.iter().enumerate() {
            ix.entry(*h).or_insert(i);
        }
        self.index = Some(ix);
    }

    /// Release 09.10.2026: leere Bindungen mit Platz fuer `n` Eintraege (der
    /// Rahmenaufbau kennt die Zahl; sonst wuchs die Liste 0 -> 4 -> 8 -> 16).
    pub(crate) fn mit_kapazitaet(n: usize) -> Self {
        Bindungen {
            eintraege: Vec::with_capacity(n),
            index: None,
        }
    }

    pub(crate) fn find(&self, h: u64, name: &str) -> Option<&Bindung> {
        self.position(h, name).map(|p| &self.eintraege[p].2)
    }

    /// Wie [`Bindungen::find`], aber veraenderbar.
    pub(crate) fn find_mut(&mut self, h: u64, name: &str) -> Option<&mut Bindung> {
        let p = self.position(h, name)?;
        Some(&mut self.eintraege[p].2)
    }

    /// Wie [`Bindungen::find_mut`], aber **nur nach dem Hash** — der
    /// Aufrufer hat den Namen aus dem Bytecode und vergleicht ihn nicht
    /// noch einmal (der Hash ist aus genau diesem Text gebildet).
    pub(crate) fn find_mut_h(&mut self, h: u64) -> Option<&mut Bindung> {
        let p = self.position_h(h)?;
        Some(&mut self.eintraege[p].2)
    }

    pub(crate) fn find_h(&self, h: u64) -> Option<&Bindung> {
        self.position_h(h).map(|p| &self.eintraege[p].2)
    }

    /// Die Bindung **entfernen** und zurueckgeben (`DeleteBinding`, ES2023
    /// 9.1.1.4.2). Danach wird der Index neu gebaut (die Positionen ruecken).
    pub(crate) fn entfernen(&mut self, h: u64, name: &str) -> Option<Bindung> {
        let pos = self.position(h, name)?;
        let b = self.eintraege.remove(pos).2;
        if self.index.is_some() {
            self.index_bauen();
        }
        Some(b)
    }

    pub(crate) fn einfuegen(&mut self, h: u64, name: &str, b: Bindung) {
        if self.position_h(h).is_none() {
            self.eintraege.push((h, name.to_string(), b));
            let pos = self.eintraege.len() - 1;
            match &mut self.index {
                Some(ix) => {
                    ix.insert(h, pos);
                }
                None if self.eintraege.len() > BINDUNGEN_INDEX_AB => self.index_bauen(),
                None => {}
            }
        }
    }

    pub(crate) fn werte(&self) -> impl Iterator<Item = &Bindung> {
        self.eintraege.iter().map(|(_, _, b)| b)
    }

    /// J28: **Name und Wert** aller Bindungen — fuer die Locals-Anzeige
    /// des Debug-Hakens.
    ///
    /// GEMESSEN, warum nicht `namen()` plus `werte()`: die beiden
    /// lieferten zwei `Iterator` ueber **dieselbe** Liste, und der
    /// Aufrufer haette sie mit einem `zip` zusammenfuehren muessen. Hier
    /// ist die Zuordnung **eine** und damit nicht trennbar.
    pub(crate) fn namen_werte(&self) -> impl Iterator<Item = (&String, &Bindung)> {
        self.eintraege.iter().map(|(_, n, b)| (n, b))
    }

    /// Zahl der Bindungen (Kostenmodell).
    pub(crate) fn len(&self) -> usize {
        self.eintraege.len()
    }

    /// Die Namen in Einfuegereihenfolge (`for-in` ueber eine Umgebung,
    /// eigene Schluessel von `var`).
    pub(crate) fn namen(&self) -> impl Iterator<Item = &String> {
        self.eintraege.iter().map(|(_, n, _)| n)
    }
}

/// Eine Bindung: Wert + Veraenderbarkeit + Initialisierungszustand (TDZ).
#[derive(Debug, Clone)]
pub(crate) struct Bindung {
    pub wert: GVal,
    pub veraenderbar: bool,
    /// `false` bis zur Deklarations-Auswertung (TDZ fuer let/const).
    pub initialisiert: bool,
    pub art: BindungsArt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindungsArt {
    Var,
    Let,
    Const,
}

/// Eigenschaft mit Getter/Setter-Unterstuetzung.
#[derive(Debug, Clone)]
pub(crate) struct Eigenschaft {
    pub wert: GVal,
    pub getter: Option<HeapRef>,
    pub setter: Option<HeapRef>,
    pub aufzählbar: bool,
    pub beschreibbar: bool,
    /// J27: **`configurable`** — getrennt von der Sperre.
    ///
    /// GEMESSEN (node 22), warum es ein eigenes Feld sein muss und nicht
    /// aus `Sperre` abgeleitet werden kann:
    ///   `Object.getOwnPropertyDescriptor({a:1}, 'a').configurable` -> `true`
    ///   `Object.getOwnPropertyDescriptor([1,2], 'length').configurable` -> `false`
    ///   nach `Object.defineProperty(o,'g',{get:f})` -> `false`
    /// Die Sperre (`heap.rs:334`) unterscheidet nur Offen/Versiegelt/
    /// Gefroren; `configurable: false` bei offenem Objekt ist ein
    /// **eigener** Zustand. Ohne dieses Feld muesste der Motor `true`
    /// liefern und damit in zwei der drei Faelle etwas behaupten, das
    /// falsch ist.
    pub konfigurierbar: bool,
}

/// Sperrzustand eines Objekts (Object.preventExtensions/seal/freeze).
/// Versiegelt/Gefroren unterscheiden sich hier nur ueber isSealed/isFrozen
/// und Schreibtiefen (konfigurierbar wird nicht verfolgt — dokumentiert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sperre {
    Offen,
    NichtErweiterbar,
    Versiegelt,
    Gefroren,
}

/// Heap-Objekt. `markiert` ist das GC-Bit.
#[derive(Debug, Clone)]
pub(crate) enum HObj {
    Umgebung(Umgebung),
    Gegenstand {
        proto: Option<HeapRef>,
        eigenschaften: Karte,
        sperre: Sperre,
        /// REIHENFOLGE der eigenen Eigenschaften (Einfuegereihenfolge, wie in
        /// node). Ohne sie waere die Ausgabe eines HashMap von der
        /// Hashfunktion abhaengig — `JSON.stringify({value, done})` lieferte
        /// `{"done","value"}` (GEMESSEN, p3.js Z4).
        reihenfolge: Vec<String>,
    },
    Feld {
        proto: Option<HeapRef>,
        elemente: Vec<GVal>,
        /// Nicht-Index-Eigenschaften (`arr.foo = 1`).
        rest: Karte,
        sperre: Sperre,
        /// **Luecken** (sec-12.2.2 / sec-22.1.3.12): Indizes, die es nicht
        /// gibt. `None` heisst „dichtes Feld" (der Normalfall, dann kostet
        /// die Menge nichts). Gelesen wird eine Luecke wie `undefined`, aber
        /// `i in a` ist **falsch**, `Object.keys` ueberspringt sie und
        /// `length`-Wachstum erzeugt sie.
        ///
        /// GEMESSEN (J7): vorher war jedes Feld dicht — `a[3] = 4` machte
        /// auch `a[1]` zu einem Index (`1 in a` war true) und
        /// `a.length = 3` legte `Object.keys`-Eintraege `0,1,2` an
        /// (`tests/t262/faelle/arr-sparse.js`, `arr-length-set.js`).
        luecken: Option<std::collections::BTreeSet<usize>>,
    },
    /// Verpacktes Primitiv (`new String/Number/Boolean`): traegt den
    /// primitiven Wert plus eigene Eigenschaften (String-Box zusaetzlich
    /// Zeichen als Elemente + `length`).
    Verpackt {
        proto: Option<HeapRef>,
        eigenschaften: Karte,
        primitiv: GVal,
        elemente: Option<Vec<GVal>>,
        sperre: Sperre,
    },
    Zeichenkette(String),
    Funktion {
        name: String,
        /// Index in die Funktions-Tabelle des Interpreters (AST + Scope).
        fn_index: usize,
        scope: Option<HeapRef>,
        proto: Option<HeapRef>,
        /// Gebundenes `this` (fuer `bind`), plus gebundene Vorder-Argumente.
        gebundenes_this: Option<GVal>,
        gebundene_args: Vec<GVal>,
        /// Konstruktor-Prototyp-Objekt (`f.prototype`).
        prototyp_objekt: Option<HeapRef>,
        ist_pfeil: bool,
        ist_native: bool,
        /// Heimat-Objekt fuer `super` (der Prototyp der Klasse).
        heim: Option<HeapRef>,
        /// Elternkonstruktor fuer `super()`.
        eltern_ctor: Option<HeapRef>,
        /// Konstruktorfunktion? (dann `RueckgabeThis` am Ende).
        ist_konstruktor: bool,
        /// Eigene Eigenschaften (`f.cache = ...`, statische Methoden).
        eigenschaften: Karte,
    },
    /// Native Funktion (Builtin): ruft Rust-Code auf. `ziel` ist fuer
    /// gebundene Resolver (Promise) reserviert, sonst `None`.
    Nativ {
        name: String,
        proto: Option<HeapRef>,
        ziel: Option<HeapRef>,
        eigenschaften: Karte,
    },
    Fehler {
        proto: Option<HeapRef>,
        name: String,
        nachricht: String,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    Karte {
        proto: Option<HeapRef>,
        eintraege: Vec<(GVal, GVal)>,
        /// J8: Schluessel-Hash -> **Positionen** in `eintraege` (ein `Vec`,
        /// weil verschiedene Schluessel denselben Hash haben duerfen:
        /// `1` und `'1'`). Ohne diesen Index war jede Suche ein **linearer
        /// Scan** ueber alle Eintraege.
        ///
        /// GEMESSEN, warum das noetig war: `let m=new Map(); for(let i=0;i<
        /// 100000;i++) m.set('k'+i,i)` brauchte 129.581 ms (10^10
        /// Vergleiche). Mit Index: siehe Bericht J8B. Die Reihenfolge von
        /// `keys()/values()/forEach` bleibt `eintraege` (Einfuegereihen-
        /// folge, sec-23.1.9) — der Index aendert daran nichts.
        index: HashMap<u64, Vec<usize>>,
        /// W35 (QUIC, 03.10.2026, ~23:0x) — **der LEERE Record.**
        ///
        /// ECMA-262 24.5.3 / 23.2.5: `[[MapData]]` bzw. `[[SetData]]` ist eine
        /// **Liste von Records**, und `delete` setzt `p.[[Key]]` auf *empty* —
        /// es **entfernt den Record nicht**. `clear` leert jeden Record und
        /// behaelt die Liste. Der Grund steht woertlich im Fall
        /// `Map/prototype/clear/map-data-list-is-preserved`: *„The existing
        /// [[MapData]] List is preserved because there may be existing
        /// MapIterator objects that are suspended midway through iterating
        /// over that List."*
        ///
        /// **GEMESSEN, vorher (`node 22` als Soll):**
        /// ```text
        /// m.delete('b')   node: der Record bleibt, nur Key leer   bei uns: remove()
        /// set.add(3) nach iterator.next()   node: der Iterator sieht 3  bei uns: undefined
        /// forEach ueber einen geloeschten und wieder eingefuegten Schluessel
        ///                    node: der zweite Besuch passiert   bei uns: nicht
        /// ```
        ///
        /// **WARUM EINE FAHNE und kein Sentinel im Schluessel** (`GVal`):
        /// `kanten_von` (Zeile 908) schiebt **jedes** `GVal::Ref(r)` als Kante
        /// in die Markierung — ein `KEIN_REF` im Eintrag wuerde also
        /// *markiert*, und das ist ein **Absturz**, kein roter Test. Die
        /// Fahne laesst den Schluessel **unveraendert im Eintrag stehen**:
        /// GC, `schluessel_gleich` und `zu_text` sehen weiterhin echte Daten,
        /// und ein **vergessenes** `belegt[i]` faellt als **falsche Zahl**
        /// auf, nicht als Panic. *Ein Datenmodell, das beim Irrtum stirbt,
        /// taugt nicht fuer geteilte Dateien.*
        ///
        /// **WER SIE BENUTZT, und das ist die ganze Regel:** `index` ist die
        /// Wahrheit fuer **das Finden** (loescht `delete` die Position aus der
        /// Liste des Hash), `belegt` die Wahrheit fuer **das Zaehlen und das
        /// Aufzaehlen**.
        belegt: Vec<bool>,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    Menge {
        proto: Option<HeapRef>,
        eintraege: Vec<GVal>,
        /// J8: derselbe Index wie bei `Karte` (Hash -> Positionen).
        index: HashMap<u64, Vec<usize>>,
        /// W35: derselbe leere Record wie bei `Karte` — dort steht der Grund.
        belegt: Vec<bool>,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    Versprechen {
        proto: Option<HeapRef>,
        zustand: VersprechenZustand,
        reaktionen: Vec<Reaktion>,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    /// W16 (JS, 04.10.2026) — **Byte-Puffer** (`ArrayBuffer`,
    /// `SharedArrayBuffer`, und der Speicher jedes TypedArray/DataView).
    ///
    /// ## WARUM EINE EIGENE VARIANTE UND KEIN `Feld`
    ///
    /// `HObj::Feld` haelt `Vec<GVal>` — **GVal je Element**. Ein
    /// `ArrayBuffer` ist aber ein **Byte**speicher: `new ArrayBuffer(4)` sind
    /// 4 Bytes, und `Uint8Array` ueber denselben Puffer sieht dieselben 4
    /// Bytes als 4 vorzeichenlose Zahlen, `Float64Array` als **eine** Zahl
    /// aus 8 Bytes. Beides muss **derselbe** Speicher sein, sonst sind
    /// `new Float64Array(buf)` und `new Uint8Array(buf)` zwei verschiedene
    /// Speicher und `u8[0] !== f64[0]`.
    ///
    /// ## WER SIE BENUTZT
    ///
    /// `ArrayBuffer`, `SharedArrayBuffer`, `DataView` und alle elf
    /// TypedArray-Arten in `eingebaut_typed.rs`. Ohne sie gibt es kein
    /// `crypto.getRandomValues`, kein `TextDecoder(Uint8Array)`, kein
    /// `fetch().arrayBuffer()`, keinen WASM-Speicherzugriff, kein
    /// Canvas-`ImageData` und keine WebGL-Puffer.
    BytePuffer {
        proto: Option<HeapRef>,
        daten: Vec<u8>,
        /// `SharedArrayBuffer` (ES2023 25.2.5). Bei uns eine **Attrappe**:
        /// die bytes sind da, die Teilbarkeit nicht — es gibt keine
        /// Threads, also ist das unterscheidbare Merkmal nur der Name.
        geteilt: bool,
        /// Detached (ES2023 25.1.3.5 DetachArrayBuffer): the bytes are gone,
        /// `byteLength` is 0 and views over it are out of bounds. Set by
        /// `WebAssembly.Memory.grow` for the old buffer.
        detached: bool,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    /// ## W55 (JS, 05.10.2026) — **TypedArray** (`Uint8Array` …
    /// `BigUint64Array`).
    ///
    /// **ES2023 23.2.5 `%TypedArray%`: die vier internen Slots.**
    /// `[[ViewedArrayBuffer]]`, `[[ByteOffset]]`, `[[ArrayLength]]` und
    /// `[[TypedArrayName]]` (die Elementart).
    ///
    /// **WARUM KEIN `Feld` UND KEIN BYTE-PUFFER ALS SOLO:**
    /// * **kein `Feld`** — ein `Feld` haelt `Vec<GVal>`, **einen Wert je
    ///   Element**. `new Float64Array(new ArrayBuffer(8))` ist **eine** Zahl
    ///   aus 8 Bytes; als `Feld` waeren es acht `GVal`, und `u8[0] !== f64[0]`
    ///   waere nicht darstellbar. Getest wird deshalb ueber den **geteilten**
    ///   Byte-Puffer.
    /// * **kein `BytePuffer` allein** — ein `BytePuffer` weiss seine **Bytes**,
    ///   aber nicht, wie viele **Elemente** sie ergeben. Ohne
    ///   `[[ArrayLength]]` ist `length = byteLength / BYTES_PER_ELEMENT`
    ///   nicht beantwortbar, und `u8.subarray(4)` waere ein Bytepuffer, der
    ///   anfaengt — es gaebe keinen Index 0.
    ///
    /// **REGEL 16, und sie ist der Grund fuer die.naechste Datei:** eine neue
    /// Variante ist **erst** fertig, wenn sie in **jeder** Liste steht, die
    /// Varianten aufzaehlt. Der Beweis, dass das noetig ist, steht bei
    /// `BytePuffer` — `heap_proto_von` kannte es zwei Tage lang nicht, und der
    /// `_`-Arm hat es geschluckt (`eigenschaften.rs:993`).
    TypedArray {
        proto: Option<HeapRef>,
        /// Die Elementart — **nicht** im Objekt, sondern hier. Das ist
        /// `[[TypedArrayName]]`; `ElementArt` lebt in `eingebaut_typed.rs`.
        art: crate::eingebaut_typed::ElementArt,
        /// `[[ViewedArrayBuffer]]` — **muss** leben, solange das TypedArray
        /// lebt. Ohne GC-Kante waere der Puffer eingesammelt und `ta.buffer`
        /// zeigte auf einen toten Slot.
        puffer: HeapRef,
        /// `[[ByteOffset]]`.
        versatz: usize,
        /// `[[ArrayLength]]` — **Element**zahl, nicht Bytes.
        laenge: usize,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    Datum {
        proto: Option<HeapRef>,
        millis: f64,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    Regex {
        proto: Option<HeapRef>,
        muster: String,
        schalter: String,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    /// V3: Generator / angehaltener async-Rahmen.
    ///
    /// GEMESSEN/Begruendung: die angehaltenen Rahmen liegen als DATEN hier
    /// (nicht auf dem Rust-Stapel) — das ist der ganze Punkt der VM. Die
    /// Rahmenkette ist typischerweise 1 bis 3 tief; `MAX_GENERATOR_RAHMEN`
    /// (10.000, M5) deckt sie.
    Generator {
        proto: Option<HeapRef>,
        zustand: crate::vm::GenZustand,
        /// Angehaltene Rahmen (unterster zuerst).
        frames: Vec<crate::vm::Rahmen>,
        /// Zuletzt gelieferter `yield`-Wert.
        ertrag: Option<GVal>,
        /// 0 = `yield`, 1 = `yield*` (am Ende), 2 = `return`.
        ertrags_art: u8,
        /// Bei async: das Versprechen, das erfuellt/abgelehnt wird.
        versprechen: Option<HeapRef>,
        /// Materialisierte Inner-Werte von `yield*` (Wert, Index).
        ertrag_stern: Option<(GVal, usize)>,
        /// Stapelhoehe beim Anhalten (A2/W6B). Beim Fortsetzen liegen die
        /// Werte des AUFRUFERS hinter den Generator-Werten; ohne diese Zahl
        /// kann der Stapel nicht richtig gedreht werden (`s += await x`
        /// lieferte sonst einen Promise-String).
        stapel_hoch: usize,
        eigenschaften: Karte,
        sperre: Sperre,
    },
    /// Release 10.10.2026: `Proxy` (ES2023 10.5, 28.2). `ziel`/`handler`
    /// sind `None`, sobald der Proxy widerrufen ist (`Proxy.revocable`) —
    /// jede Operation wirft dann TypeError. `aufrufbar`/`konstruierbar`
    /// kommen vom Ziel beim Anlegen (ES2023 10.5.14 Schritt 7).
    Proxy {
        ziel: Option<HeapRef>,
        handler: Option<HeapRef>,
        aufrufbar: bool,
        konstruierbar: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum VersprechenZustand {
    Offen,
    Erfuellt(GVal),
    Abgelehnt(GVal),
}

/// Geparkte `then`-Reaktion (Funktions-Refs + Folge-Versprechen).
#[derive(Debug, Clone)]
pub(crate) struct Reaktion {
    pub bei_erfuellt: Option<HeapRef>,
    pub bei_fehler: Option<HeapRef>,
    pub folge: HeapRef,
    /// V3: Fortsetzung eines Generators/async-Rahmens, wenn sich das
    /// Versprechen entscheidet (das ist, was `await` braucht).
    pub fortsetzung: Option<HeapRef>,
    /// 0 = normale Reaktion, 1 = `finally` (Handler ohne Argument, Wert
    /// wird durchgereicht), 2 = `then` (nur `bei_erfuellt` wird befehlend
    /// aufgerufen, `bei_fehler` leitet nur durch).
    pub art: u8,
}

impl HObj {
    fn markiert(&self) -> bool {
        match self {
            HObj::Umgebung(_) => false, // Platzhalter: Markierung extern.
            _ => false,
        }
    }
}

/// Ein Heap-Slot: belegt oder frei (Freiliste).
#[derive(Debug, Clone)]
struct Slot {
    objekt: Option<HObj>,
    markiert: bool,
}

/// Arena mit Freiliste, Byte-Zaehler und Mark-and-Sweep.
#[derive(Debug)]
pub(crate) struct Heap {
    slots: Vec<Slot>,
    freie: Vec<u32>,
    /// Geschaetzte belegte Bytes (Strings + Slots + Eintraege).
    pub bytes: usize,
    /// Zahl der Allokationen seit letztem GC (fuer periodisches Sammeln).
    pub alloks_seit_gc: usize,
    /// J4: der groesste Byte-Stand, den dieser Heap je hatte.
    ///
    /// GEMESSEN, warum das noetig war: `heap_bytes()` zeigt den Stand am
    /// ENDE. Ein Leck, der zwischen zwei Sammellaeufen entsteht und
    /// wieder verschwindet, ist daran nicht zu sehen — und genau solche
    /// Luecken sind die gefaehrlichen (der Kollektor laeuft erst alle
    /// 4096 Allokationen). Mit der Spitze behauptet ein Test den
    /// tatsaechlichen Bedarf.
    pub spitze_bytes: usize,
    /// J3c: die seit dem letzten Sammeln allozierten Bytes.
    ///
    /// GEMESSEN, warum es den Zaehl-Ausloeser nicht ersetzt, sondern
    /// ergaenzt: `s = s + 'x'` in 10^5 Schritten alloziert eine
    /// Zeichenkette je Schritt, und der zaehlbasierte Ausloeser
    /// (`alloks_seit_gc > 4096`) liess bis zu 4096 grosse Objekte
    /// zusammenkommen — gemessen 181 MiB am Ende eines Laufs, dessen
    /// Ergebnis 100 KiB ist. J3b hatte das verschaerft, weil die Schleife
    /// nun je Durchlauf weniger Objekte anlegt und der Abstand zweier
    /// Sammellaeufe dadurch in *Durchlaeufen* wuchs.
    pub bytes_seit_gc: usize,
    /// J4: die Zahl der **belegten** Slots (der „lebenden" Objekte) am
    /// Ende des letzten Sammellaufs. Ein Objekt, das nur ueber einen
    /// anderen Objekt erreichbar ist (ZYKLUS), zaehlt hier mit und wird
    /// trotzdem freigegeben — Mark-and-Sweep kennt keine Zyklen.
    pub lebend_slots: usize,
    /// Release 10.10.2026: die Tabellen von `WeakMap`/`WeakSet` als
    /// EPHEMERONE (ECMA-262 §24.3, §9.10.3): Slot der Karte -> (Slot des
    /// Schluessels -> Wert). Ein Wert lebt nur, solange Karte UND Schluessel
    /// leben; stirbt der Schluessel, faellt der Eintrag beim Sammeln weg.
    ///
    /// GEMESSEN vorher: eine flache Liste aus Texten `"@ref<slot>"`, die
    /// jedes `get` KOPIERTE und linear verglich — youtube.com: 35 000
    /// `WeakMap.get` zu je 60-77 us (2,4 s je Seitenaufbau). Dazu war der
    /// Schluessel nur sein Slot-Text: nach dem Freigeben und Neuvergeben des
    /// Slots fand ein fremdes Objekt den alten Eintrag, und kein Wert wurde
    /// je freigegeben.
    pub(crate) schwach: HashMap<u32, HashMap<u32, GVal>>,
}

/// Kostenmodell (Schaetzungen, dokumentiert): Jeder Slot pauschal 64
/// Bytes, Strings zaehlen ihre Laenge, jede Eigenschaft/Element 48 Bytes.
pub(crate) const KOSTEN_SLOT: usize = 64;
pub(crate) const KOSTEN_EINTRAG: usize = 48;
/// Kosten eines angehaltenen Rahmens (V3, M4-Buchhaltung).
pub(crate) const KOSTEN_RAHMEN: usize = 256;

impl Heap {
    pub(crate) fn neu() -> Self {
        Heap {
            slots: Vec::new(),
            freie: Vec::new(),
            bytes: 0,
            alloks_seit_gc: 0,
            spitze_bytes: 0,
            bytes_seit_gc: 0,
            lebend_slots: 0,
            schwach: HashMap::new(),
        }
    }

    pub(crate) fn anzahl(&self) -> usize {
        self.slots.iter().filter(|s| s.objekt.is_some()).count()
    }

    /// Alloziert einen Slot. Gibt die Kosten VORHER bekannt, damit der
    /// Aufrufer die Grenze pruefen (und ggf. erst GC laufen lassen) kann.
    /// Release 10.10.2026 (Diagnose `NARSIL_JS_HEAP_ZAEHLEN`): die lebenden
    /// Objekte nach Art — (Art, Zahl, Bytes), absteigend nach Bytes.
    pub(crate) fn zaehlung(&self) -> Vec<(&'static str, usize, usize)> {
        let mut k: std::collections::HashMap<&'static str, (usize, usize)> = std::collections::HashMap::new();
        for s in &self.slots {
            let Some(o) = &s.objekt else { continue };
            let art = match o {
                HObj::Umgebung(_) => "Umgebung",
                HObj::Gegenstand { .. } => "Gegenstand",
                HObj::Feld { .. } => "Feld",
                HObj::Verpackt { .. } => "Verpackt",
                HObj::Zeichenkette(_) => "Zeichenkette",
                HObj::Funktion { .. } => "Funktion",
                HObj::Nativ { .. } => "Nativ",
                HObj::Fehler { .. } => "Fehler",
                HObj::Karte { .. } => "Karte",
                HObj::Menge { .. } => "Menge",
                HObj::Versprechen { .. } => "Versprechen",
                HObj::BytePuffer { .. } => "BytePuffer",
                HObj::TypedArray { .. } => "TypedArray",
                HObj::Datum { .. } => "Datum",
                HObj::Regex { .. } => "Regex",
                HObj::Generator { .. } => "Generator",
                HObj::Proxy { .. } => "Proxy",
            };
            let e = k.entry(art).or_default();
            e.0 += 1;
            e.1 += Self::kosten_fuer(o);
        }
        let mut v: Vec<(&'static str, usize, usize)> = k.into_iter().map(|(a, (n, b))| (a, n, b)).collect();
        v.sort_by(|a, b| b.2.cmp(&a.2));
        v
    }

    pub(crate) fn kosten_fuer(obj: &HObj) -> usize {
        KOSTEN_SLOT
            + match obj {
                HObj::Zeichenkette(s) => s.len(),
                HObj::Umgebung(u) => u.bindungen.len() * KOSTEN_EINTRAG,
                HObj::Gegenstand { eigenschaften, .. } => eigenschaften.len() * KOSTEN_EINTRAG,
                // W16: die Bytes selbst sind der Speicher — sie muessen in die
                // Kosten, sonst waere ein 1-MB-`ArrayBuffer` **kostenlos** und
                // die GC haette nie einen Grund zu sammeln.
                HObj::BytePuffer { daten, .. } => daten.len(),
                // ## W55: die **Bytes** liegen im Puffer, nicht hier. Ein
                // TypedArray kostet deshalb seine Property-Map und seine
                // Elementzahl **als Index**, aber **keine** Bytes — sonst
                // wuerde `new Uint8Array(buf)` denselben Speicher doppelt
                // kosten und die GC zu frueh sammeln. GEMESSEN als
                // Zusammenbruch der Quote, als `kosten_fuer` beides zaehlte.
                HObj::TypedArray { laenge, .. } => {
                    laenge * KOSTEN_EINTRAG / 8 + KOSTEN_EINTRAG
                }
                HObj::Feld {
                    elemente,
                    rest,
                    luecken,
                    ..
                } => {
                    elemente.len() * KOSTEN_EINTRAG
                        + rest.len() * KOSTEN_EINTRAG
                        + luecken.as_ref().map_or(0, |m| m.len() * KOSTEN_EINTRAG / 4)
                }
                HObj::Verpackt {
                    eigenschaften,
                    elemente,
                    ..
                } => {
                    eigenschaften.len() * KOSTEN_EINTRAG
                        + elemente
                            .as_ref()
                            .map(|e| e.len() * KOSTEN_EINTRAG)
                            .unwrap_or(0)
                }
                HObj::Karte {
                    eintraege,
                    index,
                    eigenschaften,
                    ..
                } => {
                    eintraege.len() * KOSTEN_EINTRAG * 2
                        + eigenschaften.len() * KOSTEN_EINTRAG
                        // Der Index ist billiger als ein Eintrag (nur Zahlen).
                        + index.len() * KOSTEN_EINTRAG / 2
                }
                HObj::Menge {
                    eintraege,
                    eigenschaften,
                    ..
                } => eintraege.len() * KOSTEN_EINTRAG + eigenschaften.len() * KOSTEN_EINTRAG,
                HObj::Funktion {
                    gebundene_args,
                    eigenschaften,
                    ..
                } => gebundene_args.len() * KOSTEN_EINTRAG + eigenschaften.len() * KOSTEN_EINTRAG,
                HObj::Nativ { eigenschaften, .. } => eigenschaften.len() * KOSTEN_EINTRAG,
                HObj::Proxy { .. } => 2 * KOSTEN_EINTRAG,
                // V3: angehaltene Rahmen kosten Platz (M4-Buchhaltung).
                HObj::Generator {
                    frames,
                    ertrag_stern,
                    ..
                } => {
                    frames.len() * KOSTEN_RAHMEN
                        + if ertrag_stern.is_some() {
                            KOSTEN_EINTRAG
                        } else {
                            0
                        }
                }
                HObj::Versprechen {
                    reaktionen,
                    eigenschaften,
                    ..
                } => reaktionen.len() * KOSTEN_EINTRAG + eigenschaften.len() * KOSTEN_EINTRAG,
                HObj::Fehler { eigenschaften, .. }
                | HObj::Datum { eigenschaften, .. }
                | HObj::Regex { eigenschaften, .. } => eigenschaften.len() * KOSTEN_EINTRAG,
            }
    }

    /// Slot belegen; der Aufrufer hat die Grenze bereits geprueft.
    pub(crate) fn belegen(&mut self, obj: HObj, kosten: usize) -> HeapRef {
        let r = match self.freie.pop() {
            Some(i) => {
                self.slots[i as usize] = Slot {
                    objekt: Some(obj),
                    markiert: false,
                };
                HeapRef(i)
            }
            None => {
                let i = self.slots.len();
                self.slots.push(Slot {
                    objekt: Some(obj),
                    markiert: false,
                });
                HeapRef(i as u32)
            }
        };
        self.bytes = self.bytes.saturating_add(kosten);
        // J3c: GEMESSEN, hier fehlte die Spitze — `belegen` erhoehte
        // `bytes`, aber nicht `spitze_bytes`, also zeigte
        // `heap_spitze_bytes()` 32 KiB fuer einen Lauf, der am Ende
        // 181 MiB auf dem Buch hatte (alle Slot-Allokationen gingen
        // daran vorbei).
        self.spitze_bytes = self.spitze_bytes.max(self.bytes);
        self.bytes_seit_gc = self.bytes_seit_gc.saturating_add(kosten);
        self.alloks_seit_gc += 1;
        r
    }

    pub(crate) fn holen(&self, r: HeapRef) -> &HObj {
        self.slots[r.0 as usize]
            .objekt
            .as_ref()
            .expect("Heap: toter Verweis (interner Fehler)")
    }

    pub(crate) fn holen_mut(&mut self, r: HeapRef) -> &mut HObj {
        self.slots[r.0 as usize]
            .objekt
            .as_mut()
            .expect("Heap: toter Verweis (interner Fehler)")
    }

    /// Release 15.10.2026: the number of slots, for scans over every object
    /// (`es_modern.rs`, resizing a buffer updates its tracking views).
    pub(crate) fn slot_zahl(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn lebt(&self, r: HeapRef) -> bool {
        (r.0 as usize) < self.slots.len() && self.slots[r.0 as usize].objekt.is_some()
    }

    /// Zusatzkosten nachtraeglich verbuchen (z.B. Eigenschaft angehaengt).
    pub(crate) fn kosten_verbuchen(&mut self, kosten: usize) {
        self.bytes = self.bytes.saturating_add(kosten);
        // J4: die Spitze steigt mit und faellt nie.
        self.spitze_bytes = self.spitze_bytes.max(self.bytes);
        // J3c: fuer den byte-basierten Ausloeser mitzaehlen.
        self.bytes_seit_gc = self.bytes_seit_gc.saturating_add(kosten);
    }

    pub(crate) fn kosten_freigeben(&mut self, kosten: usize) {
        self.bytes = self.bytes.saturating_sub(kosten);
    }

    /// Mark-and-Sweep ab `wurzeln`. Gibt die Zahl befreiter Slots zurueck.
    /// Iterativ mit eigenem Stapel (keine Rekursion — M5).
    pub(crate) fn sammeln(&mut self, wurzeln: &[GVal], umgebungen: &[HeapRef]) -> usize {
        // 1. Entmarkieren.
        for s in &mut self.slots {
            s.markiert = false;
        }
        // 2. Markieren (iterativ).
        //
        // J21: **KEINE KANTE DARF WEGGELASSEN WERDEN.** GEMESSEN/GEFUNDEN
        // (Abnahme 11:07, wikipedia.org — und mit derselben Ursache der
        // Panic in `tests/j21_gc_wurzel.rs`): der alte Stapel war
        // `Vec<HeapRef>` mit Duplikaten — jeder Knoten kam **so oft** auf den
        // Stapel, wie er Kanten hat. Bei `max_stapel = slots + 16` war der
        // Stapel also lange vor dem Ende des Bildes voll, und der Rest der
        // Kanten wurde **stillschweigend verworfen**
        // (`if stapel.len() < max_stapel`). Was danach unmarkiert war, raeumte
        // der Sweep — die **Dokumentwurzel** des DOM mit, mitten im
        // `innerHTML`-Bau. Der Fehler war nichtdeterministisch, weil die
        // Reihenfolge der `HashMap`-Kanten (Rusts `RandomState`) von Lauf zu
        // Lauf wechselt und damit der Fuellstand des Stapels.
        //
        // BEHEBUNG: `in_liste` (ein Bit je Slot) verhindert Doppel-Pops. Jeder
        // Slot steht damit **hoechstens einmal** auf dem Stapel, die
        // Stapel-Laenge ist durch die Slot-Zahl gedeckelt (M3 bleibt also
        // gewahrt) — und keine Kante geht verloren, weil nichts mehr
        // verworfen werden muss.
        let n = self.slots.len();
        let mut in_liste: Vec<bool> = vec![false; n];
        let mut stapel: Vec<HeapRef> = Vec::new();
        for w in wurzeln {
            if let GVal::Ref(r) = w {
                let i = r.0 as usize;
                if i < n && !in_liste[i] {
                    in_liste[i] = true;
                    stapel.push(*r);
                }
            }
        }
        for r in umgebungen {
            let i = r.0 as usize;
            if i < n && !in_liste[i] {
                in_liste[i] = true;
                stapel.push(*r);
            }
        }
        let mut besuche: usize = 0;
        // J21: mit `in_liste` wird jeder Slot **einmal** aufgepoppt, also
        // `besuche <= n`. Der Deckel ist damit ein reines Sicherheitsnetz
        // (4x Slot-Zahl + 1024) und wird im Normalfall nie erreicht.
        let max_besuche = n.saturating_mul(4).saturating_add(1024);
        // LEISTUNG (JS, 03.10.): `kanten_von` fuellt diesen Puffer; er wird
        // **einmal** fuer den ganzen Markierlauf allokiert und pro Objekt
        // nur noch geleert. Vorher allokierte `kanten_von` einen frischen
        // `Vec` je markiertem Objekt — bei zehntausend Objekten je GC-Lauf
        // zehntausend Allokationen im Hotloop (und die Mehrheit davon fuer
        // Blattobjekte ganz leer).
        let mut puffer: Vec<HeapRef> = Vec::new();
        // J8b: **abgebrochene Markierung darf NIE sweepen.** GEMESSEN, warum
        // das ein Fehler war: `max_besuche` zaehlt auch die *Doppel*-Pops
        // (jeder Stack-Eintrag, auch ein bereits markiertes Objekt). Ein
        // Skript, das viele Wurzeln ohne Freigabe stapelt (`JSON.parse` in
        // einer Schleife: die Wurzeln werden nie abgebaut), riss die Grenze
        // frueh — die Schleife brach ab, die **nicht besuchten** Objekte
        // blieben unmarkiert, und der Sweep raeumte sie weg. Danach
        // zeigte ein Wert auf einen freien Slot: `heap.rs` PANIKTE
        // („Heap: toter Verweis"), gefangen als „JS-Absturz (intern)".
        // Nachweis: crates/narsil-jsvm/examples (10.000x `JSON.parse` mit
        // verschachteltem Feld). Der M3-Deckel bleibt (Arbeit ist
        // begrenzt), aber ein unvollstaendiges Bild fuehrt zu **keinem**
        // Sweep — dann wird im naechsten Lauf neu gesammelt.
        let mut abgebrochen = false;
        // Release 10.10.2026: die Markierung laeuft, bis die Ephemerone
        // (WeakMap-Werte, deren Karte UND Schluessel markiert sind) nichts
        // Neues mehr erreichen.
        loop {
            while let Some(r) = stapel.pop() {
                besuche += 1;
                if besuche > max_besuche {
                    abgebrochen = true;
                    break; // Harter Deckel (M3).
                }
                let idx = r.0 as usize;
                if idx >= n {
                    continue;
                }
                in_liste[idx] = false;
                if self.slots[idx].objekt.is_none() || self.slots[idx].markiert {
                    continue;
                }
                self.slots[idx].markiert = true;
                // Kanten einsammeln (Slot kurz ausleihen, dann Referenzen legen).
                // LEISTUNG (JS, 03.10.): in den wiederverwendeten Puffer fuellen
                // (einmal allokiert), nicht je Objekt neu allokieren. Der Puffer
                // wird **vor** der Mutation von `self.slots` gefuellt, damit die
                // Schleife ihn nur lesend haelt.
                puffer.clear();
                kanten_von(self.slots[idx].objekt.as_ref().unwrap(), &mut puffer);
                for k in puffer.iter() {
                    let i = k.0 as usize;
                    // J21: nur einwerfen, was noch nicht im Stapel liegt und
                    // nicht schon markiert ist — dadurch **kein** Verwerfen und
                    // `stapel.len() <= n` (M3).
                    if i < n && !in_liste[i] && !self.slots[i].markiert {
                        in_liste[i] = true;
                        stapel.push(*k);
                    }
                }
            }
            if abgebrochen || self.schwach.is_empty() {
                break;
            }
            let mut neu = false;
            for (karte, tabelle) in &self.schwach {
                let ki = *karte as usize;
                if ki >= n || !self.slots[ki].markiert {
                    continue;
                }
                for (schluessel, wert) in tabelle {
                    let si = *schluessel as usize;
                    if si < n
                        && self.slots[si].markiert
                        && let GVal::Ref(w) = wert
                    {
                        let wi = w.0 as usize;
                        if wi < n && !self.slots[wi].markiert && !in_liste[wi] {
                            in_liste[wi] = true;
                            stapel.push(*w);
                            neu = true;
                        }
                    }
                }
            }
            if !neu {
                break;
            }
        }
        // 3. Sweepen — nur bei vollstaendiger Markierung (J8b, siehe oben).
        if abgebrochen {
            self.alloks_seit_gc = 0;
            self.bytes_seit_gc = 0;
            return 0;
        }
        // Release 10.10.2026: tote Karten und Eintraege mit totem Schluessel
        // fallen weg, BEVOR ihre Slots neu vergeben werden koennen.
        if !self.schwach.is_empty() {
            let slots = &self.slots;
            let lebt = |i: u32| (i as usize) < n && slots[i as usize].markiert && slots[i as usize].objekt.is_some();
            let mut frei_bytes = 0usize;
            self.schwach.retain(|karte, tabelle| {
                if !lebt(*karte) {
                    frei_bytes += tabelle.len() * KOSTEN_EINTRAG;
                    return false;
                }
                let vorher = tabelle.len();
                tabelle.retain(|s, _| lebt(*s));
                frei_bytes += (vorher - tabelle.len()) * KOSTEN_EINTRAG;
                true
            });
            self.bytes = self.bytes.saturating_sub(frei_bytes);
        }
        let mut frei = 0;
        let mut lebend = 0usize;
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.objekt.is_some() {
                if s.markiert {
                    // J4 (JS, 03.10.): die Zahl der lebenden Objekte wurde
                    // nach dem Sweep in einem **zweiten** Volldurchlauf
                    // ueber `slots` gezaehlt. Sie faellt hier ohne Mehrkosten
                    // ab: ein Slot, der den Sweep ueberlebt hat, ist genau
                    // ein lebender. Ein ZYKLUS (`a.b = b; b.a = a`) wird
                    // mitgezaehlt und trotzdem freigegeben — Mark-and-Sweep
                    // braucht keine Zyklenerkennung, weil kein Objekt eine
                    // Wurzel ist, sondern nur Kanten.
                    lebend += 1;
                } else {
                    let kosten = HObj::kosten_fuer_im_slot(s.objekt.as_ref().unwrap());
                    s.objekt = None;
                    self.freie.push(i as u32);
                    self.bytes = self.bytes.saturating_sub(kosten);
                    frei += 1;
                }
            }
        }
        self.alloks_seit_gc = 0;
        self.bytes_seit_gc = 0;
        self.lebend_slots = lebend;
        frei
    }

    /// Gibt den **mutablen Byte-Slice** eines TypedArray zurueck, falls der
    /// Verweis auf ein `HObj::TypedArray` zeigt. Nutzt den geteilten
    /// `BytePuffer` samt `versatz` und `laenge` (Elementzahl).
    ///
    /// **NÄHERUNG**: Der Rueckgabewert ist `&mut [u8]` direkt auf dem
    /// Puffer-Speicher. Der Aufrufer muss sicherstellen, dass keine
    /// gleichzeitige Mutation ueber einen anderen Pfad passiert (single-threaded
    /// VM, also kein Problem).
    pub(crate) fn typedarray_bytes_mut(&mut self, ta_ref: HeapRef) -> Option<&mut [u8]> {
        // Erst TypedArray-Daten auslesen (puffer, versatz, laenge, art)
        let (puffer, versatz, laenge, art) = {
            let obj = self.holen(ta_ref);
            if let HObj::TypedArray { puffer, versatz, laenge, art, .. } = obj {
                (*puffer, *versatz, *laenge, *art)
            } else {
                return None;
            }
        };
        let element_bytes = art.bytes();
        let byte_len = laenge * element_bytes;
        // Dann den Puffer mutabel borgen
        if let HObj::BytePuffer { daten, .. } = self.holen_mut(puffer) {
            let start = versatz;
            let end = start + byte_len;
            if end <= daten.len() {
                return Some(&mut daten[start..end]);
            }
        }
        None
    }

    /// Gibt die Bytes eines TypedArray als `Vec<u8>` zurueck (Kopie).
    /// Fuer Tests und Debugging.
    pub(crate) fn typedarray_bytes(&self, ta_ref: HeapRef) -> Option<Vec<u8>> {
        let obj = self.holen(ta_ref);
        if let HObj::TypedArray { puffer, versatz, laenge, art, .. } = obj {
            let element_bytes = art.bytes();
            let byte_len = *laenge * element_bytes;
            if let HObj::BytePuffer { daten, .. } = self.holen(*puffer) {
                let start = *versatz;
                let end = start + byte_len;
                if end <= daten.len() {
                    return Some(daten[start..end].to_vec());
                }
            }
        }
        None
    }

    /// Prueft, ob ein HeapRef auf ein TypedArray zeigt.
    pub(crate) fn ist_typedarray(&self, r: HeapRef) -> bool {
        matches!(self.holen(r), HObj::TypedArray { .. })
    }
}

impl HObj {
    fn kosten_fuer_im_slot(obj: &HObj) -> usize {
        Heap::kosten_fuer(obj)
    }

    pub(crate) fn markiert_unused(&self) -> bool {
        self.markiert()
    }
}

/// Alle Heap-Kanten eines Objekts (fuer die Markierung).
fn kanten_von(obj: &HObj, v: &mut Vec<HeapRef>) {
    let gval = |g: &GVal, v: &mut Vec<HeapRef>| {
        if let GVal::Ref(r) = g {
            v.push(*r);
        }
    };
    match obj {
        // Release 10.10.2026: Ziel und Handler eines Proxys leben mit ihm.
        HObj::Proxy { ziel, handler, .. } => {
            v.extend(ziel.iter().copied());
            v.extend(handler.iter().copied());
        }
        HObj::Umgebung(u) => {
            if let Some(e) = u.eltern {
                v.push(e);
            }
            if let Some(object) = u.objekt {
                v.push(object);
            }
            for b in u.bindungen.werte() {
                gval(&b.wert, v);
            }
        }
        HObj::Gegenstand {
            proto,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Feld {
            proto,
            elemente,
            rest,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            for e in elemente {
                gval(e, v);
            }
            for e in rest.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Verpackt {
            proto,
            eigenschaften,
            primitiv,
            elemente,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            gval(primitiv, v);
            if let Some(el) = elemente {
                for e in el {
                    gval(e, v);
                }
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Zeichenkette(_) => {}
        HObj::Funktion {
            scope,
            proto,
            gebundenes_this,
            gebundene_args,
            prototyp_objekt,
            heim,
            eltern_ctor,
            eigenschaften,
            ..
        } => {
            if let Some(s) = scope {
                v.push(*s);
            }
            if let Some(p) = proto {
                v.push(*p);
            }
            if let Some(t) = gebundenes_this {
                gval(t, v);
            }
            for a in gebundene_args {
                gval(a, v);
            }
            if let Some(po) = prototyp_objekt {
                v.push(*po);
            }
            if let Some(home) = heim {
                v.push(*home);
            }
            if let Some(parent) = eltern_ctor {
                v.push(*parent);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Nativ {
            proto,
            ziel,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            // Resolve/reject callbacks keep their pending promise alive.
            if let Some(z) = ziel {
                v.push(*z);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Fehler {
            proto,
            eigenschaften,
            ..
        }
        | HObj::Datum {
            proto,
            eigenschaften,
            ..
        }
        | HObj::Regex {
            proto,
            eigenschaften,
            ..
        } => {
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
            if let Some(p) = proto {
                v.push(*p);
            }
        }
        HObj::Karte {
            proto,
            eintraege,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            for (k, val) in eintraege {
                gval(k, v);
                gval(val, v);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Menge {
            proto,
            eintraege,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            for e in eintraege {
                gval(e, v);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        // W16 (JS, 04.10.2026): ein Byte-Puffer haelt **Bytes**, keine `GVal` —
        // die tragen also **keine** Kante. Nur der Prototyp ist ein Verweis.
        //
        // GEMESSEN/GEPRUEFT, warum das so genau stimmt: `Uint8Array` und
        // `Float64Array` ueber **denselben** `ArrayBuffer` muessen denselben
        // Speicher sehen. Das geht nur, wenn der Puffer die Bytes haelt und
        // **nicht** `Vec<GVal>` — eine `GVal` je Element wuerde hier eine
        // Kante je Byte bedeuten.
        HObj::BytePuffer {
            proto,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
            }
        }
        // ## W55 — DIE KANTE, OHNE DIE ALLES ZUSAMMENBRICHT
        //
        // `[[ViewedArrayBuffer]]` ist eine **GC-Kante**, kein Kommentar: ohne
        // sie sammelt die GC den `ArrayBuffer`, und `ta.buffer` zeigte danach
        // auf einen **toten** Slot. Das ist die Fehlerklasse aus W5C/J23 —
        // ein Objekt, dessen Prototyp oder Puffer eingesammelt wurde, sieht
        // danach aus wie ein Objekt aus einer anderen Welt.
        //
        // **`versatz` und `laenge` sind Zahlen und keine Kanten** — das ist der
        // ganze Grund, warum die Laenge im TypedArray steht und nicht
        // gerechnet wird.
        HObj::TypedArray {
            proto,
            puffer,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            v.push(*puffer);
            for e in eigenschaften.values() {
                gval(&e.wert, v);
            }
        }
        HObj::Versprechen {
            proto,
            zustand,
            reaktionen,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            match zustand {
                VersprechenZustand::Erfuellt(w) | VersprechenZustand::Abgelehnt(w) => gval(w, v),
                VersprechenZustand::Offen => {}
            }
            for r in reaktionen {
                if let Some(f) = r.bei_erfuellt {
                    v.push(f);
                }
                if let Some(f) = r.bei_fehler {
                    v.push(f);
                }
                if let Some(f) = r.fortsetzung {
                    v.push(f);
                }
                v.push(r.folge);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
        HObj::Generator {
            proto,
            frames,
            ertrag,
            versprechen,
            ertrag_stern,
            eigenschaften,
            ..
        } => {
            if let Some(p) = proto {
                v.push(*p);
            }
            // Die angehaltenen Rahmen sind GC-Wurzeln: Umgebungen, `this`,
            // Temps, Argumente, Heimat-/Elternbezug.
            for r in frames {
                v.push(r.umg);
                // Release 09.10.2026: Parameterumgebung und laufende
                // `for-in`/`for-of`-Listen des angehaltenen Rahmens.
                if let Some(p) = r.param_umg {
                    v.push(p);
                }
                if let Some(p) = r.prolog_rumpf {
                    v.push(p);
                }
                for (werte, _) in &r.iter_lauf {
                    for w in werte {
                        gval(w, v);
                    }
                }
                if let Some(h) = r.heim {
                    v.push(h);
                }
                if let Some(c) = r.eltern_ctor {
                    v.push(c);
                }
                gval(&r.this, v);
                gval(&r.neu, v);
                for t in &r.temps {
                    gval(t, v);
                }
                for a in &r.args {
                    gval(a, v);
                }
                // Release 15.10.2026: the operand stack of the suspended
                // frame and its pending `finally` completions — the same
                // edges as `vm::rahmen_wurzeln` gives a running frame.
                // MEASURED before: `async function f() { return [1, await p,
                // 3] }` lost the half-built array while `f` waited (a GC in
                // between swept it: "Feld korrupt", on chip.de "Heap: toter
                // Verweis" in an object literal resumed from a network
                // reaction, a panic on the JS thread).
                for s in &r.stapel {
                    gval(s, v);
                }
                for a in &r.fertig {
                    if let crate::vm::Aktion::Rueckgabe(w) | crate::vm::Aktion::Wurf(w) = a {
                        gval(w, v);
                    }
                }
                if let Some(crate::vm::Aufnahme::Wert(w)) = &r.aufnahme {
                    gval(w, v);
                }
                if let Some(crate::vm::Aufnahme::Wurf(w)) = &r.aufnahme {
                    gval(w, v);
                }
            }
            if let Some(e) = ertrag {
                gval(e, v);
            }
            if let Some(p) = versprechen {
                v.push(*p);
            }
            if let Some((w, _)) = ertrag_stern {
                gval(w, v);
            }
            for e in eigenschaften.values() {
                gval(&e.wert, v);
                if let Some(g) = e.getter {
                    v.push(g);
                }
                if let Some(s) = e.setter {
                    v.push(s);
                }
            }
        }
    }
}
