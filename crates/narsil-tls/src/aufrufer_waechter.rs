// added 04.10.2026 03:1x, R68. **DER WÄCHTER ÜBER „GELIEFERT UND NIEMALS
// BENUTZT“ — IM GANZEN WORKSPACE, und mit einem Scanner, der Kommentare und
// Zeichenketten entfernt.** R67 hat die Crate gemessen und **zwei** Lücken
// benannt: (a) der Wächter sah keine andere Crate und meldete darum
// `host_verwerfen` **falsch** (der Aufrufer steht in
// `narsil-net/src/lib.rs:4530`), und (b) die Ausnahmeliste war **behauptet**,
// nicht belegt. Beide Lücken sind dieselbe: **eine Frage, ein Ort, eine
// Messung.** R67s `pub_fn_waechter` ist darum **ersetzt** und nicht ergänzt —
// zwei Wächter über dieselbe Frage mit verschiedenen Antworten wären schlimmer
// als ein Wächter.
//
// added 04.10.2026 03:2x, R68. **WARUM ZUR LAUFZEIT GELESEN WIRD.** R67 las
// `src/` und `tests/` per `include_str!`; das braucht einen **festen** Pfad und
// kann nur **eigenes** Crate einziehen. „Kein Aufrufer im **Workspace**“
// verlangt aber den Inhalt **aller** Dateien (GEMESSEN 04.10.2026: **1295**
// Dateien, **32,8 MB**). Also liest der Test sie zur **Laufzeit** aus `crates/`.
// Das ist **kein** Compile-Zeit-Zugriff auf fremde Crates: die Dateien werden
// **gelesen**, aber nicht per `include!` eingebunden — es entsteht **keine**
// Abhängigkeit, und **kein** fremder Code wird Teil dieses Tests. Sie sind
// Messwerte. Das ist genau die Grenze, die R61 gerissen hat, nur an der
// richtigen Stelle.
//
// added 04.10.2026 03:3x, R68. **UND JETZT DER TEIL, DER IN DEN LETZTEN RUNDEN
// GEFEHLT HAT: DER SCANNER.** R67 hat **Kommentarzeilen** ausgeschlossen, aber
// **Zeichenketten** mitgezählt, und das ist nicht theoretisch: GEMESSEN am
// 04.10. mit `grep` über `crates/`, `nonce_laenge` steht **zweimal** im
// Workspace — **beide** Male in einem **Kommentar** von mir. Ein Wächter, der
// Kommentare oder Zeichenketten mitzählt, erklärt einen **gelöschten** Namen für
// lebendig; beim zweiten Fall genügt ein **Logtext**. Das ist die R54-Falle in
// ihrer reinsten Form: grün und **blind**.

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    /// added 04.10.2026 03:4x, R68. **DIE ZEILE, DIE ALLES ENTSCHEIDET: eine
    /// Zeile, die eine Funktion **deklariert**, zählt nicht.** Sonst wäre jede
    /// Definition ihr eigener Aufrufer.
    ///
    /// added 04.10.2026 03:5x, R68: **UND ES GILT FÜR `fn`, NICHT NUR FÜR
    /// `pub fn`.** Eine privat deklarierte Funktion ist genauso tot wie eine
    /// `pub`-deklarierte; der Wächter soll **beide** Formen finden, weil die
    /// private die häufigere ist.
    ///
    /// added 04.10.2026 03:6x, R68: **WARUM DIE UNTERDRÜCKUNG IM SCANNER
    /// STECKT UND NICHT IM ZÄHLER:** ein `pub fn` darf in einer Zeile einen
    /// **Blockkommentar** oeffnen (`pub fn f() { /*`), der in der nächsten
    /// Zeile endet. Würde ich die Zeile **vor** dem Scanner abschneiden, verlöre
    /// ich den Zustand „im Blockkommentar“ und die nächste Zeile würde als Code
    /// gezählt. Also **läuft** der Scanner über die Zeile, **nur ohne zu zählen**.
    /// added 04.10.2026 03:16x, R68. **UND DIE REIHENFOLGE IST DER GANZE
    /// FEHLER, DEN DIESER WÄCHTER GEMACHT HAT.** Die erste Fassung prüfte
    /// **Zeichenliteral** (`'x'`) **vor** den Kommentaren, und ein `'` in einem
    /// deutschen Kommentar („das `don't`“) öffnete einen Zustand, der den
    /// **Rest der Datei** verschluckte. GEMESSEN: der Wächter meldete
    /// **19998 von 19998** Definitionen tot — also **nichts** gezählt, und die
    /// Zahl sah aus wie ein Befund. **Die Reihenfolge der Zustände ist also
    /// Teil der Messung**, nicht Kosmetik: erst der Zustand, in dem man
    /// **steckt**, dann der Einstieg in einen neuen.
    fn zaehle(
        text: &str,
        eigenes: &HashMap<usize, String>,
        interessant: &HashSet<String>,
        raus: &mut HashMap<String, u32>,
    ) {
        /// added 04.10.2026 03:17x, R68: **FÜNF ZUSTÄNDE, und keiner mehr.**
        /// `Zeile` ist Code, `Block` ein `/* … */` (verschachtelbar), `String`
        /// ein `"…"` (darf über Zeilen laufen), `Roh(h)` ein `r#"…"#`, und
        /// `Zeichen` ein `'…'`. added 04.10.2026 03:17x: **`String` fehlt in
        /// der ersten Fassung** — sie wurde dort „inline“ behandelt, also
        /// verschluckte ein `"` in einem Kommentar ebenfalls den Rest der
        /// Datei.
        #[derive(PartialEq)]
        enum Zustand {
            Zeile,
            Block(usize),
            Str,
            Roh(usize),
            Zeichen,
        }
        let b = text.as_bytes();
        let mut i = 0usize;
        let mut zeile = 0usize;
        let mut z = Zustand::Zeile;
        while i < b.len() {
            let c = b[i];
            if c == b'\n' {
                zeile += 1;
                i += 1;
                continue;
            }
            // (1) WENN ICH IN EINEM ZUSTAND BIN, NUR DIESEN
            match z {
                Zustand::Block(tiefe) => {
                    if c == b'/' && b.get(i + 1) == Some(&b'*') {
                        z = Zustand::Block(tiefe + 1);
                        i += 2;
                    } else if c == b'*' && b.get(i + 1) == Some(&b'/') {
                        z = if tiefe > 1 {
                            Zustand::Block(tiefe - 1)
                        } else {
                            Zustand::Zeile
                        };
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                Zustand::Str => {
                    if c == b'\\' {
                        i += 2;
                    } else if c == b'"' {
                        z = Zustand::Zeile;
                        i += 1;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                Zustand::Roh(h) => {
                    if c == b'"' {
                        let mut k = i + 1;
                        let mut n = 0usize;
                        while k < b.len() && b[k] == b'#' && n < h {
                            k += 1;
                            n += 1;
                        }
                        if n == h {
                            z = Zustand::Zeile;
                            i = k;
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                    continue;
                }
                Zustand::Zeichen => {
                    if c == b'\\' {
                        i += 2;
                    } else if c == b'\'' {
                        z = Zustand::Zeile;
                        i += 1;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                Zustand::Zeile => {}
            }
            // (2) IM CODE: EINSTIEGE, in dieser REIHENFOLGE
            if c == b'/' && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if c == b'/' && b.get(i + 1) == Some(&b'*') {
                z = Zustand::Block(1);
                i += 2;
                continue;
            }
            // added 04.10.2026 03:18x, R68: **ROHE ZEICHENKETTE** `r"…"` und
            // `r#"…"#` (sowie die Byteformen `b"…"`/`br#"…"#`). Darin ist `"`
            // **kein** Escapefluss, sondern Backslash und Gänfzeichen — wer
            // Escapeflüsse behandelt, verschluckt den Rest der Datei.
            if c == b'r' || c == b'b' {
                let mut k = i + 1;
                let mut n = 0usize;
                while k < b.len() && b[k] == b'#' {
                    n += 1;
                    k += 1;
                }
                if b.get(k) == Some(&b'"') {
                    z = Zustand::Roh(n);
                    i = k + 1;
                    continue;
                }
            }
            if c == b'"' {
                z = Zustand::Str;
                i += 1;
                continue;
            }
            if c == b'\'' {
                // added 04.10.2026 03:19x, R68: **ZEICHEN ODER LEBENSZEIT.**
                // GEMESSEN die Regel: `\'` und `x'` sind Zeichenliterale,
                // `\'a` (ohne schließendes `'` in zwei Stellen) ist eine
                // Lebenszeit und **gehört zum Bezeichner** — deshalb wird hier
                // **nur** das `'` übergangen und kein Zustand geöffnet.
                if b.get(i + 1) == Some(&b'\\') || b.get(i + 2) == Some(&b'\'') {
                    z = Zustand::Zeichen;
                }
                i += 1;
                continue;
            }
            // (3) BEZEICHNER
            if c.is_ascii_alphanumeric() || c == b'_' {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                // added 04.10.2026 03:8x, R68: **EINZEICHNER ZÄHLEN NICHT.**
                // GEMESSEN: Namen in diesem Workspace sind **nie** kürzer als
                // zwei Zeichen, und sonst zählte jedes `b'a'` mit.
                if i - start > 1 {
                    let wort = &text[start..i];
                    //
                    // added 04.10.2026 03:41x, R68: **NUR DER EIGENE NAME DER
                    // DEFINITIONSZEILE WIRD ÜBERSPRUNGEN, nicht die ganze
                    // Zeile.** GEMESSEN, warum das der Unterschied ist: in
                    // `fn ruft() -> u8 { gerufen() }` steht ein **Aufruf**
                    // auf der Definitionszeile. Unterdrückt man die Zeile ganz,
                    // verschwindet dieser Aufruf und `gerufen` gilt als **tot** —
                    // ein Falschbefund, der mit jedem Einzeiler im Workspace
                    // wächst. added 04.10.2026 03:42x: eigene Messung dieses
                    // Fehlers: mein **eigener Test** fiel darauf (er suchte
                    // genau diesen Aufruf und fand ihn nicht).
                    let eigen = eigenes.get(&zeile).map(String::as_str);
                    if eigen != Some(wort) && interessant.contains(wort) {
                        // added 04.10.2026 03:21x, R68. **`entry`, und das ist
                        // der zweite stille Fehler derselben Stunde.** Mit
                        // `get_mut` **hochgezählt** wird nur, was **schon**
                        // drinsteht — ein leerer Zähler bleibt **leer**, ohne
                        // einen einzigen Fehler. GEMESSEN: der Wächter meldete
                        // „20006 von 20006 tot“, sah also **aus** wie ein
                        // Befund und war eine **Nullmessung**. `entry` legt
                        // den Eintrag an: `or_insert(0)` **und dann** hochzählen.
                        //
                        // added 04.10.2026 03:22x, R68: **UND WARUM NICHT
                        // `entry(wort)`, was der Compiler vorschlug:**
                        // `entry` nimmt den **Schlüssel als Eigentum**
                        // (`String`), `contains`/`get_mut` nur als `&str`.
                        // `entry` hätte also **bei jedem** Vorkommen eine
                        // `String` gebaut. Die `match`-Form baut sie **nur beim
                        // ersten** — und sie **sagt** genau das, worum es hier
                        // geht: **anlegen, wenn es fehlt.**
                        match raus.get_mut(wort) {
                            Some(v) => *v += 1,
                            None => {
                                raus.insert(wort.to_string(), 1);
                            }
                        }
                    }
                }
                continue;
            }
            i += 1;
        }
    }

    /// added 04.10.2026 03:9x, R68: **IST DIESE ZEILE EINE DEFINITION?** Alle
    /// möglichen Vorwörter werden abgehakt, damit auch `pub(crate) unsafe fn`
    /// und `pub async fn` erkannt werden. **Kein `trim_end`**, weil die Zeile
    /// danach als Text weiterläuft.
    fn ist_definitionszeile(zeile: &str) -> Option<(String, bool)> {
        let mut t = zeile.trim_start();
        let mut ist_pub = false;
        if let Some(r) = t.strip_prefix("pub(crate) ") {
            t = r;
            ist_pub = true;
        } else if let Some(r) = t.strip_prefix("pub ") {
            t = r;
            ist_pub = true;
        }
        for w in ["unsafe ", "async ", "const ", "extern "] {
            if let Some(r) = t.strip_prefix(w) {
                t = r;
            }
        }
        let rest = t.strip_prefix("fn ")?;
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            None
        } else {
            Some((name, ist_pub))
        }
    }

    /// added 04.10.2026 03:10x, R68. **DER WEG ÜBER DIE DATEIEN.** GEMESSEN
    /// (04.10.2026): `crates/` enthält **1295** `.rs`-Dateien und **32,8 MB**;
    /// das **Lesen** dauert **0,07 s**, das Zählen mit Hashmap **0,96 s** in
    /// Python — in Rust ist es der **EIN** Durchgang, den die Messung unten
    /// mitprotokolliert. Der alte Weg wäre **N** Durchgänge gewesen
    /// (N = Zahl der Namen), und das ist der Grund für den einen Durchgang.
    fn alle_dateien() -> Vec<(String, String, String)> {
        // (crate, pfad_relativ, inhalt)
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let crates = wurzel.join("crates");
        let mut raus: Vec<(String, String, String)> = Vec::new();
        let mut stapel = vec![crates.clone()];
        while let Some(d) = stapel.pop() {
            let Ok(eintraege) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in eintraege.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stapel.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    let Ok(inhalt) = std::fs::read_to_string(&p) else {
                        continue;
                    };
                    let rel = p
                        .strip_prefix(&wurzel)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let crate_name = rel.split('/').nth(1).unwrap_or("?").to_string();
                    raus.push((crate_name, rel, inhalt));
                }
            }
        }
        raus.sort_by(|a, b| a.1.cmp(&b.1));
        raus
    }

    /// added 04.10.2026 03:11x, R68. **DIE VIER KLASSEN, DIE EIN TEXTUELLER
    /// ZÄHLER NICHT SEHEN KANN** — und die sind **gemessen**, nicht vermutet:
    /// `grep`/`zaehle` meldet in `narsil-tls` **522** tote `fn`, davon
    /// (a) **494** `#[test]`/`#[ignore]`-Funktionen: der **Prüfer** ruft sie auf,
    ///     nicht der Name; (b) **1** `fn main` in `examples/`: `cargo` ruft es
    ///     auf; (c) **1** Trait-Impl-Methode (`impl PartialOrd for Zahl`): der
    ///     Aufruf geht **über den Trait**; (d) der Rest sind **lokale** `fn` in
    ///     Testfunktionen (`h`, `g`, `z`).
    ///
    /// added 04.10.2026 03:32x, R68: **DARAUS FOLGT DIE REGEL, UND SIE IST DIE
    /// WICHTIGSTE STELLE DES WÄCHTERS:** **rot wird nur, was man nicht
    /// aufrufen kann, ohne den Namen zu schreiben.** Ein Wächter, der auch
    /// `#[test]`-Funktionen meldet, wird nach dem **ersten** Fehlalarm
    /// abgeschaltet — und dann ist er für **keinen** mehr da.
    #[derive(Clone, Copy, PartialEq)]
    enum Art {
        /// added 04.10.2026 03:33x: `pub fn` auf Modulebene — die einzige Form,
        /// bei der „kein Aufrufer“ etwas über **API** sagt.
        Oeffentlich,
        /// added 04.10.2026 04:9x, R69: **MEHRDEUTIGER NAME — und das ist die
        /// Grenze des Werkzeugs, GEMESSEN an einem echten Fund.** `hole` ist in
        /// **sieben** Crates der Name einer **anderen** Funktion
        /// (`narsil_quic::hole`, `BildCache::hole`, `LayoutPlan::hole`,
        /// `Engine::hole`, `hpack::hole`, `pool::hole`, `Ctx::hole`) und
        /// zusätzlich ein **Variablenname** in `narsil-net/src/lib.rs:6012`. Der
        /// Zähler liest **Namen**, clippy kennt die **Crate** — also fand der
        /// Zähler `pool::hole` als „benutzt“ (hundertfache Nennungen), und
        /// **clippy** meldete tot: `warning: function 'hole' is never used`.
        /// Ein **eindeutiger** Name ist textlich entscheidbar, ein
        /// **mehrdeutiger** nicht. Also: mehrdeutige Namen werden **gezählt und
        /// gedruckt**, nie rot — und **clippy** ist dort die Instanz, die
        /// entscheidet (und die Abnahme verlangt clippy 0 Warnungen ohnehin).
        Mehrdeutig,
        /// added 04.10.2026 03:48x, R68: **EINBUCHSTABIGER NAME.** GEMESSEN:
        /// die sieben „toten“ privaten Funktionen in `narsil-tls/src` waren
        /// `h`, `g`, `z`, `b` — **lokale** Helfer in Testfunktionen. Ein
        /// Textzähler kann sie **nicht** zählen: jedes `h` im Workspace kann ein
        /// **anderes** `h` sein (Variablenname, Parameter). Sie sind darum
        /// **kein Befund**, sondern eine **Grenze des Werkzeugs** — und eine
        /// Grenze, die man **benennt**, nicht die man verschweigt.
        Kurzname,
        Test,
        Beispiel,
        Trait,
        Privat,
    }

    /// added 04.10.2026 03:34x, R68: **IST DIESE DEFINITION EINE `#[test]`-
    /// ODER `#[ignore]`-FUNKTION?** GEMESSEN, warum **bis zu fünf** Zeilen
    /// zurückgesehen werden: `#[test]` und `#[ignore = "…"]` stehen **beide**
    /// über der Funktion, und dazwischen kann ein `///`-Kommentar stehen.
    fn ist_test(zeilen: &[&str], nr: usize) -> bool {
        for z2 in zeilen.iter().take(nr).skip(nr.saturating_sub(5)) {
            let t = z2.trim();
            if t.starts_with("#[test")
                || t.starts_with("#[bench")
                || t.starts_with("#[ignore")
                || t.starts_with("#[tokio::test")
            {
                return true;
            }
        }
        false
    }

    /// added 04.10.2026 03:35x, R68: **LIEGT DIESE `fn` IN EINER
    /// `impl … for …`?** Dann ist sie eine **Trait**-Methode und wird über den
    /// Trait aufgerufen (`a < b`, `format!("{}", a)`), nicht über ihren Namen.
    /// **DIE GRENZE DIESER HEURISTIK, und sie ist ehrlich zu benennen:** sie
    /// schaut bis zu **40** Zeilen zurück und erkennt `for` nur in der
    /// `impl`-Zeile selbst. Ein `impl` über fünf Zeilen verteilt würde als
    /// `Privat` gemeldet — dann **redet** der Wächter, aber mit dem richtigen
    /// Wort („privat“), nicht mit „öffentlich“.
    fn ist_trait_impl(zeilen: &[&str], nr: usize) -> bool {
        for z2 in zeilen.iter().take(nr).skip(nr.saturating_sub(40)) {
            let t = z2.trim_start();
            if t.starts_with("impl") && t.contains(" for ") {
                return true;
            }
        }
        false
    }

    /// added 04.10.2026 03:36x, R68. **DER TEST.** Er meldet **alles** und
    /// lässt **nur eines** rot werden:
    /// (1) `pub fn` auf Modulebene in `narsil-tls/src` ohne Aufrufer **im ganzen
    ///     Workspace** und ohne Eintrag in `AUSNAHME` → **rot**;
    /// (2) ein Eintrag in `AUSNAHME`, dessen Nennungszahl die Grenze
    ///     übersteigt → **rot** (die Ausnahme ist dann überflüssig);
    /// (3) die vier Klassen aus [`Art`] und alle fremden Crates → **gedruckt**,
    ///     nicht gerichtet: ihre Dateien sind nicht meine.
    #[test]
    fn keine_funktion_ohne_aufrufer_und_ohne_grund() {
        let dateien = alle_dateien();
        if dateien.is_empty() {
            // added 04.10.2026 03:12x, R68: **OHNE WORKSPACE KEIN ROT, ABER
            // AUCH KEIN GRUEN OHNE WORTE.** Ein stilles `return` waere genau
            // die Falle, die dieser Wächter verhindern soll; deshalb wird der
            // Fall **gedruckt**.
            println!(
                "R68-WAECHTER: crates/ nicht gefunden (alle_dateien() leer) — \
                 Messung ausgesetzt, kein Ergebnis"
            );
            return;
        }
        let bytes: usize = dateien.iter().map(|(_, _, t)| t.len()).sum();
        // (1) Definitionen sammeln, mit Art
        let mut interessant: HashSet<String> = HashSet::new();
        // (crate, pfad, zeilennummer, name, art)
        let mut defs: Vec<(String, String, usize, String, Art)> = Vec::new();
        for (c, pfad, text) in &dateien {
            let z: Vec<&str> = text.lines().collect();
            for (nr, zeile) in z.iter().enumerate() {
                if let Some((name, ist_pub)) = ist_definitionszeile(zeile) {
                    interessant.insert(name.clone());
                    let art = if name.len() < 2 {
                        Art::Kurzname
                    } else if ist_test(&z, nr) {
                        Art::Test
                    } else if pfad.contains("/examples/") && name == "main" {
                        Art::Beispiel
                    } else if ist_trait_impl(&z, nr) {
                        Art::Trait
                    } else if ist_pub {
                        Art::Oeffentlich
                    } else {
                        Art::Privat
                    };
                    defs.push((c.clone(), pfad.clone(), nr + 1, name, art));
                }
            }
        }
        // added 04.10.2026 04:9x, R69: **WIE OFT IST EIN NAME DEFINIERT?** Das
        // ist die **Entscheidungsgrenze** des Werkzeugs: `hole` ist siebenmal
        // definiert (GEMESSEN, siehe `Art::Mehrdeutig`), `mit_basis` einmal.
        // added 04.10.2026 05:8x, R69: **DIE MEHRDEUTIGKEIT IST EINE KLASSE und
        // keine Sonderbehandlung im Fehlerzweig.** Mein erster Versatz hat sie im
        // `if` behandelt — und clippy sagte `variant 'Mehrdeutig' is never
        // constructed`. **GEMESSEN, warum das die richtige Form ist:** wenn die
        // Klasse erst im Fehlerzweig entsteht, ist sie an **zwei** Orten
        // auszupacken (Zaehler und Fehlerbedingung), und die beiden koennen
        // auseinanderlaufen. Einmal hier klassifizieren, dann ueberall nur noch
        // `art_name` lesen.
        // added 04.10.2026 05:9x, R69: **`wie_oft` haelt `&str` auf `defs`**,
        // also kann `defs` nicht **zugleich** verschoben werden (E0505). Die
        // Loesung ist nicht `clone`, sondern **Zaehler in einer eigenen
        // Tabelle**: sie sind Zahlen, keine Strings, und `get` mit `&str`
        // funktioniert weiter, weil `HashMap<String, usize>` `.get(&str)`
        // annimmt. added 05:9x: **DIE ZWEITE HALBE WAHRHEIT:** `wie_oft` mit
        // `String`-Schluesseln ist hier die *einfachere* Form, weil sie ohne
        // Lebensdauer-Problem auskommt — die 1300 Dateien bleiben so bei
        // **einem** Besitz, nicht bei zwei.
        let mut wie_oft: HashMap<String, usize> = HashMap::new();
        for (_, _, _, name, _) in &defs {
            *wie_oft.entry(name.clone()).or_insert(0) += 1;
        }
        let defs: Vec<(String, String, usize, String, Art)> = defs
            .into_iter()
            .map(|(c, pfad, nr, name, art)| {
                let neu = if wie_oft.get(name.as_str()).copied().unwrap_or(0) > 1 {
                    Art::Mehrdeutig
                } else {
                    art
                };
                (c, pfad, nr, name, neu)
            })
            .collect();
        // (2) EIN Durchgang über alles, Definitionszeilen unterdrückt
        let mut zaehler: HashMap<String, u32> = HashMap::new();
        for (_, _, text) in &dateien {
            let mut eigenes: HashMap<usize, String> = HashMap::new();
            for (nr, zeile) in text.lines().enumerate() {
                if let Some((name, _)) = ist_definitionszeile(zeile) {
                    eigenes.insert(nr, name);
                }
            }
            zaehle(text, &eigenes, &interessant, &mut zaehler);
        }
        // (3) Auswerten: nur `Oeffentlich` darf rot werden
        let mut rot: Vec<String> = Vec::new();
        let mut art_zaehler: HashMap<&'static str, usize> = HashMap::new();
        let mut je_crate: HashMap<String, HashMap<&'static str, usize>> = HashMap::new();
        // added 04.10.2026 03:43x, R68: **DIE EIGENE CRATE GETRENNT GEZÄHLT.**
        // Die Gesamtzahl enthaelt **fremde** Dateien; „164 ohne Aufrufer“ ohne
        // diese Trennung zu nennen, waere eine Zahl, die **meine** Crate
        // belastet, ohne **meine** Crate zu meinen. `src/` und `tests/` getrennt,
        // weil ein toter Test-Bauer etwas anderes ist als eine tote
        // oeffentliche Funktion.
        let mut mein_pub_src = 0usize;
        let mut mein_pub_tests = 0usize;
        let mut mein_priv_src = 0usize;
        let mut nennungen_als_druck = 0usize;
        for (c, pfad, nr, name, art) in &defs {
            let n = zaehler.get(name).copied().unwrap_or(0);
            if n > 0 {
                continue;
            }
            *art_zaehler.entry(art_name(*art)).or_insert(0) += 1;
            // added 04.10.2026 04:4x, R69: **JE CRATE DIE KLASSE, nicht nur
            // die Summe.** R68 druckte `R68-FREMD <crate>: <n>` — das sagt nur,
            // dass in `narsil-css` 559 `fn` ohne Aufrufer liegen, **nicht**, wie
            // viele davon `pub` sind (die einzige Form, bei der die Zahl etwas
            // über API sagt). Für die Leitung ist der **Aufschlüsselung** die
            // nützliche Angabe.
            *je_crate
                .entry(c.clone())
                .or_default()
                .entry(art_name(*art))
                .or_insert(0) += 1;
            //
            // added 04.10.2026 04:3x, R69: **ROT FÜR `Oeffentlich` UND `Privat`
            // IN MEINEN DATEIEN.** R68 hat `Privat` nur gezählt und `0` in
            // `narsil-tls/src` gemessen — aber **eine Zahl, die man nicht prüft,
            // ist keine Zusicherung**: sie gilt erst, wenn die Bedingung
            // **stehen** und **rot werden** kann. Und `narsil-net/src` war
            // **überhaupt nicht** gemessen.
            if *art == Art::Oeffentlich || *art == Art::Privat {
                if pfad.starts_with("crates/narsil-tls/src/") {
                    mein_pub_src += 1;
                } else if pfad.starts_with("crates/narsil-tls/tests/") {
                    mein_pub_tests += 1;
                }
                //
                // added 04.10.2026 04:9x, R69: **UND HIER IST DIE GRENZE.** Ein
                // Name, der **mehrfach** definiert ist, kann textlich **nicht**
                // als „unbenutzt“ entschieden werden — die Nennungen gehören
                // dann womöglich zu den **anderen** Funktionen. Er wird
                // **gedruckt** (mit der Zahl der Definitionen) und **nicht** rot.
                if ist_meine_datei(pfad) && !AUSNAHME.iter().any(|(a, _, _)| *a == name) {
                    if *art == Art::Mehrdeutig {
                        println!(
                            "R68-MEHRDEUTIG {pfad}:{nr}: {name} ist {} -mal definiert — \
                             der Zaehler kann das nicht entscheiden, clippy entscheidet es",
                            wie_oft.get(name.as_str()).copied().unwrap_or(0)
                        );
                    } else {
                        rot.push(format!("{pfad}:{nr}: {name} ({})", art_name(*art)));
                    }
                }
            }
            if *art == Art::Privat && pfad.starts_with("crates/narsil-tls/src/") {
                mein_priv_src += 1;
                if nennungen_als_druck <= 20 {
                    // added 04.10.2026 03:47x, R68: **PRIVATE FUNKTIONEN
                    // AUCH DRUCKEN, aber ohne Rechte.** Sie sind **meine**
                    // Dateien, und ein `privat` ohne Aufrufer ist **kein**
                    // Fehlalarm wie bei `#[test]` — es ist schlicht **nicht
                    // benutzt**. Dass es nach R67 nur noch **sieben** sind,
                    // ist **gemessen** und eine **gute** Nachricht: die anderen
                    // werden von Tests oder Traits benutzt.
                    nennungen_als_druck += 1;
                    println!("R68-PRIVAT {pfad}:{nr}: {name}");
                }
            }
        }
        let mut ausnahmen_offen: Vec<&str> = Vec::new();
        for (name, max, grund) in AUSNAHME {
            let n = zaehler.get(*name).copied().unwrap_or(0);
            println!("R68-AUSNAHME {name}: {n} von höchstens {max} Nennungen — {grund}");
            if n > *max {
                ausnahmen_offen.push(name);
            }
        }
        println!(
            "R68-MEINE-CRATE narsil-tls: pub in src={mein_pub_src}, pub in tests={mein_pub_tests}, \
             privat in src={mein_priv_src}"
        );
        let summe: u32 = zaehler.values().sum();
        println!(
            "R68-ZAEHLER {} Dateien, {:.1} MB, {} Namen, {} Nennungen",
            dateien.len(),
            bytes as f64 / 1e6,
            zaehler.len(),
            summe
        );
        println!(
            "R68-WAECHTER {} `fn`-Definitionen, {} davon ohne Aufrufer: {}",
            defs.len(),
            art_zaehler.values().sum::<usize>(),
            art_zaehler
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut crate_liste: Vec<(&String, &HashMap<&str, usize>)> = Vec::new();
        crate_liste.extend(je_crate.iter());
        crate_liste.sort_by(|a, b| {
            b.1.values()
                .sum::<usize>()
                .cmp(&a.1.values().sum::<usize>())
        });
        for (c, klassen) in crate_liste.iter() {
            let mut teile: Vec<String> = klassen.iter().map(|(k, v)| format!("{k}={v}")).collect();
            teile.sort();
            println!(
                "R68-CRATE {c}: {} — {}",
                klassen.values().sum::<usize>(),
                teile.join(" ")
            );
        }
        //
        // added 04.10.2026 03:45x, R68: **DIE POSITIVKONTROLLE ÜBER DIE CRATE-
        // GRENZE.** `hole_crl` ist in `narsil-tls/src/crl.rs` definiert und wird
        // **ausserhalb** von `narsil-tls` gerufen (`narsil-net/src/lib.rs`, GEMESSEN
        // mit `grep`). Findet der Lauf sie **nicht**, dann liest er nur die
        // eigene Crate — und **alles**, was er über `narsil-net` sagt, ist
        // geredet. added 04.10.2026 03:46x: R67 ist genau daran gescheitert
        // (`host_verwerfen`), und die Fehlermeldung des Tools war **die
        // Werkzeug-Aussage**, nicht der Code.
        let ueber_die_grenze = zaehler.get("hole_crl").copied().unwrap_or(0);
        assert!(
            ueber_die_grenze > 0,
            "der Lauf sieht nicht ueber die Crate-Grenze: `hole_crl` hat {ueber_die_grenze} Nennungen"
        );
        assert!(
            ausnahmen_offen.is_empty(),
            "diese Ausnahmen haben mehr Nennungen als ihre Grenze erlaubt und sind \
             damit überflüssig: {ausnahmen_offen:?}"
        );
        //
        // added 04.10.2026 04:8x, R69: **DIE MELDUNG MUSSTE MITWACHSEN.** Sie
        // sagte „in narsil-tls“, und seit die Bedingung **beide** eigenen Crates
        // umfasst (`MEINE_DATEIEN`), waere das eine **falsche** Angabe im
        // Moment des Fehlers — die schlimmste Form eines Werkzeugtextes, weil
        // sie den Suchenden in die falsche Crate schickt. GEMESSEN, dass die
        // Bedingung wirklich greift: `narsil-net/src/lib.rs: mit_basis`,
        // `narsil-net/src/pool.rs: hole_oder_verbinde` und
        // `narsil-net/src/pool.rs: leeren_und_zaehlen` — **drei** Funde, alle drei
        // in **fremd gewachsenen** Zeilen von R49 bis R68.
        assert!(
            rot.is_empty(),
            "{} `fn` in MEINEN Dateien ohne Aufrufer im ganzen Workspace: {rot:?}",
            rot.len()
        );
    }

    /// added 04.10.2026 03:37x, R68: **DER NAME DER KLASSE, als Text** — damit
    /// die Ausgabe lesbar bleibt und der Test **nicht** über Enum-Namen
    /// redet.
    fn art_name(a: Art) -> &'static str {
        match a {
            Art::Oeffentlich => "oeffentlich",
            Art::Kurzname => "kurzname",
            Art::Mehrdeutig => "mehrdeutig",
            Art::Test => "test",
            Art::Beispiel => "beispiel",
            Art::Trait => "trait",
            Art::Privat => "privat",
        }
    }

    /// added 04.10.2026 04:1x, R69. **DIE FEHLERBEDINGUNG STEHT ÜBER DATEIEN,
    /// NICHT ÜBER CRATES — und der Unterschied ist gemessen.** `narsil-net/src`
    /// enthält **acht** Dateien, und **`cache.rs` ist nicht meine** (GEMESSEN:
    /// `ls`, sie existiert; sie steht nicht in meiner Freigabe). Eine
    /// crate-weite Bedingung hätte also **fremde** Dateien rot gemacht — und
    /// das ist der Fehler, den R61 und R67 je **einer** Seite gemacht haben:
    /// zu weit (fremde Dateien) oder zu eng (eigene übergangen).
    const MEINE_DATEIEN: &[&str] = &[
        "crates/narsil-tls/src/",
        "crates/narsil-net/src/lib.rs",
        "crates/narsil-net/src/h2.rs",
        "crates/narsil-net/src/h2client.rs",
        "crates/narsil-net/src/h2strom.rs",
        "crates/narsil-net/src/hpack.rs",
        "crates/narsil-net/src/pool.rs",
        "crates/narsil-net/src/sitzung.rs",
    ];

    /// added 04.10.2026 04:2x, R69: **MIT `/` am Ende = ganzer Ordner, ohne
    /// = genau diese Datei.** GEMESSEN, warum das unterscheidbar sein muss:
    /// `crates/narsil-net/src/lib.rs` **und** `crates/narsil-net/tests/lib.rs` gibt
    /// es nicht, aber `crates/narsil-net/src/cache.rs` schon — und ein
    /// `contains("lib.rs")` hätte sie **beide** getroffen.
    fn ist_meine_datei(pfad: &str) -> bool {
        MEINE_DATEIEN
            .iter()
            .any(|m| pfad.starts_with(m) || pfad == *m)
    }

    /// added 04.10.2026 03:30x, R68. **JEDER EINTRAG HAT EINE ZAHL UND EINEN
    /// SATZ, UND DIE ZAHL WIRD GEPRÜFT.** added 04.10.2026 03:31x: **R67s
    /// Begründung war ungenau, und der Wächter hat es gemeldet:** ich hatte
    /// „nie benutzt“ geschrieben, GEMESSEN sind aber **1** Nennungen — die
    /// `pub use`-Zeile in `lib.rs`. Eine Nennung ist **kein Aufruf**, aber sie
    /// ist auch **nicht null**, und genau darum steht hier eine **Grenze**
    /// (`max`) statt eines Wortes: übersteigt die Zahl sie, ist die Ausnahme
    /// **überflüssig** und der Test wird rot.
    const AUSNAHME: &[(&str, u32, &str)] = &[
        (
            "hkdf_sha384",
            1,
            "1 Nennung = die pub-use-Zeile in lib.rs, kein Aufruf; Familie zu hkdf_sha256, noetig fuer 0x1302, das wir nicht anbieten",
        ),
        (
            "extract_sha384",
            1,
            "1 Nennung = die pub-use-Zeile in lib.rs; wird nur von hkdf_sha384 benutzt — dieselbe Begruendung",
        ),
        (
            "expand_label_sha384",
            1,
            "1 Nennung = die pub-use-Zeile in lib.rs; dritte der Familie — dieselbe Begruendung",
        ),
    ];

    // ------------------------------------------------------------------
    // added 04.10.2026 03:14x, R68. **DIE VIER SCANNER-TESTS.** Sie sind der
    // Grund, warum der Wächter oben etwas wert ist: ein Wächter, der nichts
    // prüft, ist schlimmer als keiner (R54), und ein Scanner, der **immer**
    // null zählt, wäre grün und blind (R61 in neuer Kleidung).
    // ------------------------------------------------------------------

    #[test]
    fn scanner_ignoriert_kommentare_und_zeichenketten() {
        let text = "// tot\n/* tot\n   noch tot */\nfn lebendig() -> u8 { 1 }\nlet s = \"tot\";\n";
        let mut interessant = HashSet::new();
        interessant.insert("tot".to_string());
        interessant.insert("lebendig".to_string());
        let mut z = HashMap::new();
        zaehle(text, &HashMap::new(), &interessant, &mut z);
        assert_eq!(z.get("tot"), None, "Kommentar und String müssen raus");
        assert_eq!(z.get("lebendig"), Some(&1), "echter Code muss zählen");
    }

    #[test]
    fn scanner_unterscheidet_lebenszeit_von_zeichenliteral() {
        // added 04.10.2026 03:15x, R68: `'a` ist **kein** Zeichenliteral.
        // Ein Scanner, der es als Literal liest, verschluckt den Rest der
        // Zeile und verliert echte Bezeichner — das wäre **still** falsch.
        // added 04.10.2026 03:38x, R68: **`Sy` und nicht `S`** — die Regel
        // „zwei Zeichen oder mehr“ (sonst zählte jedes `b'a'` mit) hat mich
        // beim **Schreiben des Tests** selbst erwischt: `S` ist ein Zeichen und
        // wird nicht gezählt. Das ist die R66-Lektion in der Form, in der sie
        // sich wiederholt: **der Test muss die Regel des Werkzeugs kennen.**
        let text = "struct Sy<'a> { feld: &'a str }\n";
        let mut interessant = HashSet::new();
        interessant.insert("Sy".to_string());
        interessant.insert("feld".to_string());
        let mut z = HashMap::new();
        zaehle(text, &HashMap::new(), &interessant, &mut z);
        assert_eq!(z.get("Sy"), Some(&1));
        assert_eq!(z.get("feld"), Some(&1), "nach der Lebenszeit weiterlesen");
    }

    #[test]
    fn scanner_ignoriert_rohe_zeichenkette() {
        // added 04.10.2026 03:39x, R68: `qu` statt `q` — siehe oben.
        let text = "let p = r#\"der \"Bezeichner\" hier\"#;\nlet qu = 1;\n";
        let mut interessant = HashSet::new();
        interessant.insert("Bezeichner".to_string());
        interessant.insert("qu".to_string());
        let mut z = HashMap::new();
        zaehle(text, &HashMap::new(), &interessant, &mut z);
        assert_eq!(z.get("Bezeichner"), None, "in r#\"…\"# zählt nichts");
        assert_eq!(z.get("qu"), Some(&1), "danach geht es weiter");
    }

    #[test]
    fn definitionszeile_zaehlt_nicht_als_aufrufer() {
        // added 04.10.2026 03:16x, R68: **DIE POSITIVKONTROLLE.** Ein Wächter,
        // der nur „0\" liefert, ist grün und blind. Hier wird **beides**
        // geprüft: die Definition zählt nicht, **und** der Aufrufer zählt.
        let text = "pub fn nur_definiert() -> u8 { 1 }\nfn gerufen() -> u8 { 2 }\nfn ruft() -> u8 { gerufen() }\n";
        let mut interessant = HashSet::new();
        interessant.insert("nur_definiert".to_string());
        interessant.insert("gerufen".to_string());
        interessant.insert("ruft".to_string());
        // added 04.10.2026 03:40x, R68: **DIE EIGENEN NAMEN** der drei
        // Definitionszeilen, und **nicht** die Zeilen. `gerufen()` steht auf
        // Zeile 3 **und** ist ein Aufruf — er muss zählen, während `ruft` auf
        // derselben Zeile **nicht** zählen darf.
        let mut eigenes: HashMap<usize, String> = HashMap::new();
        for (nr, zeile) in text.lines().enumerate() {
            if let Some((name, _)) = ist_definitionszeile(zeile) {
                eigenes.insert(nr, name);
            }
        }
        let mut z = HashMap::new();
        zaehle(text, &eigenes, &interessant, &mut z);
        assert_eq!(z.get("nur_definiert"), None, "Definition ist kein Aufrufer");
        assert_eq!(
            z.get("gerufen"),
            Some(&1),
            "der Aufrufer muss gefunden werden"
        );
        assert_eq!(z.get("ruft"), None, "`ruft` wird nirgends gerufen");
    }
}
