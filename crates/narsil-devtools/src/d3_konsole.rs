//! D3 — KONSOLE: `console.*`-Ausgaben, JS-Eingabe ausführen, Fehler mit Ort.
//!
//! # WAS D3 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D3 Konsole: console.* Ausgaben, JS-Eingabe ausfuehren (Bruecke
//! > narsil-jsvm), Fehler mit Ort.
//!
//! # WAS `narsil-jsvm` HEUTE HAT — gemessen am 01.10.2026 am Quelltext
//!
//! | D3 braucht | gibt es? | wo |
//! |---|---|---|
//! | `console.*`-Ausgaben | **ja, aber untypisiert** | `Engine::konsole() -> &[String]` |
//! | Stufe (log/warn/error) | **nein** | alle vier gehen in dieselbe `String` |
//! | JS-Eingabe ausführen | **ja** | `Engine::neu(..)` + `Engine::ausfuehren(&str)` |
//! | Fehler **mit Ort** | **nur Syntax** | `JsFehler::Syntax { zeile, spalte, nachricht }` |
//! | Laufzeitfehler mit Ort | **nein** | `JsFehler::Laufzeit(String)` hat keine Position |
//!
//! # DIE ZWEI ENTSCHEIDUNGEN, DIE DIESE DATEI TRAEGT
//!
//! **1. Die Stufe kommt aus dem Skript, nicht aus dem Aufrufer.**
//! `console.error("x")` und `console.log("x")` ergeben in `narsil-jsvm`
//! dieselbe Zeile „x". Diese Crate kann das nicht unterscheiden, also wird
//! es **nicht geraten**: der Reiter zeigt die Zeile, wie sie da ist, und der
//! Wunsch steht in `team/WUNSCH_JS.txt` (Punkt D3). Was diese Crate kann:
//! Fehler, die sie selbst erzeugt (aus der JS-Eingabezeile), bekommen ihre
//! Stufe von sich aus.
//!
//! **2. Die JS-Eingabe bekommt einen EIGENEN Motor.**
//! GEMESSEN in `narsil/src/jsvm.rs` (dort ausdrücklich vermerkt): „Ein
//! zweites `ausfuehren` macht die Funktionswerte des ersten ungültig." Eine
//! zweite Eingabe im Seitenmotor wuerde also jeden Klick-Handler der Seite
//! zerstoeren. Deshalb: [`Konsole::fuehre_aus`] baut je Eingabe einen Motor
//! mit [`narsil_jsvm::Grenzen::bombe`] — und das ist nicht nur billiger,
//! sondern auch das Richtige: eine Handeingabe ist kein Seitencode.
//!
//! # DIE EINE EIGENTLICHE KLEMMREGEL
//!
//! GEMESSEN: `JsFehler::Syntax { zeile, spalte }` bekommt von
//! `narsil-jsvm/src/compile.rs` an vielen Stellen die **festen Werte 0, 0**.
//! Ein Reiter, der „Zeile 0, Spalte 0" anzeigt, zeigt eine Zahl, die falsch
//! ist. Also: `Ort::zeilen_spalte` gibt bei 0 `None` zurück und der Reiter
//! schreibt „(kein Ort)" — das ist die ehrliche Ausgabe.

use crate::grenzen::Grenzen as Eigen;
use narsil_jsvm::{Engine, Grenzen as Motor, JsFehler, Wert};

/// Der Zustand des Konsolen-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Konsole {
    /// Die Zeilen, chronologisch.
    pub zeilen: Vec<Zeile>,
    /// Wie viele Zeilen es eigentlich gab (nach der Grenze).
    pub verworfen: u64,
    /// Die aktuelle Zahl der Zeilen.
    pub laufende_nr: u64,
    /// Wie viele Zeilen davon FEHLER sind (gezaehlt, nicht gesucht).
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:11x): die Zahl steht in der
    /// Kopfzeile des Reiters, und sie wurde durch EINEN VOLLSTANDSLAUF ueber
    /// alle Zeilen gebildet — bei jedem Bild. Ein Zaehler kostet beim
    /// Schreiben eine Addition und beim Lesen nichts; ein Lauf ueber 5 000
    /// Zeilen kostet bei 60 Bildern je Sekunde 300 000 Vergleiche je
    /// Sekunde fuer EINE Zahl, die sich seldom aendert.
    pub fehler: u32,
    /// Wie viele Zeilen davon WARNUNGEN sind (gezaehlt, nicht gesucht).
    pub warnungen: u32,
    /// Der Text in der Eingabezeile.
    pub eingabe: String,
    /// Wo der Cursor in der Eingabezeile steht (Zeichen, nicht Byte).
    pub cursor: usize,
    /// Die Stufe, mit der die naechste Ausgabe kommt (der Filter im Reiter).
    pub filter: Option<Stufe>,
    /// Der Text, den der Filter der Werkzeugleiste durchlaesst (D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein EIGENER Text und nicht `filter`
    /// (das ist [`Konsole::filter`], ein `Option<Stufe>`): Firefox hat
    /// beides getrennt — ein Textfeld UND die Stufen-Schalter. Zwei
    /// Filter in einem Feld wuerden bedeuten, dass das Textfeld die
    /// Schalter mitbeschreibt, und der Nutzer kann in Firefox beides
    /// unabhaengig. M4: `max_filterzeichen` Zeichen, mehr nicht.
    pub filtertext: String,
    /// Nimmt das Filterfeld die Tastatur an? (D12)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Flag und kein Zustand ist:
    /// es sagt, WELCHES Feld die Tastatur bedient — eine Eigenschaft des
    /// Fensters, nicht der Konsole. Es steht hier, weil es neben
    /// [`Konsole::fokus`] gehoert (die Eingabezeile) und beide zusammen die
    /// eine Frage beantworten: „wo landet ein Tastendruck?".
    pub filter_fokus: bool,
    /// Welche Stufen gerade sichtbar sind (die vier Schalter, D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum vier Schalter und nicht einer:
    /// Firefox hat je einen fuer Fehler, Warnungen, Logs und Debug. Bei
    /// `console.error` in einer Schleife sind das 500 Zeilen, und der
    /// Nutzer will die Warnungen sehen — ohne die 500 Fehler dazwischen.
    /// Mit EINEM Schalter geht das nicht, und „nur Fehler an/aus" ist eine
    /// Auswahl, die man sich nicht merken kann.
    pub stufen: StufenSicht,
    /// Ob der Reiter bei jedem neuen Eintrag nach unten laeuft.
    pub folgt: bool,
    /// Hat die Eingabezeile den Fokus? (D3: „JS-Eingabe ausfuehren")
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Fokus hier und nicht im Geraest:
    /// die Eingabezeile IST die Konsoleneingabe, und es gibt genau eine
    /// Stelle, an der „der Cursor wartet" — die Zeile, die tippt. Ein Flag
    /// im `Geraest` muesste man bei jedem Reiterwechsel mitfuehren und
    /// wuerde fragen „welche Eingabe?".
    ///
    /// GEMESSENER ZUSTAND BIS 02.10.2026 3:1x: die Zeile wurde GEZEICHNET
    /// (mit Cursorbalken), aber im Fenster war nichts, was hineintippte —
    /// `Sitzung::ereignis` routet Tasten an die Seite oder die Adressleiste,
    /// und die Bruecke bekam nur F12/Strg+Shift+I. Die Zeile war also eine
    /// Attrappe: sie lud zur Eingabe ein und nahm nichts an.
    pub fokus: bool,
    /// Wie viele Zeilen der SEITE schon uebernommen sind.
    ///
    /// GEMESSENER FEHLER (02.10.2026, 3:3x), gefunden beim Messen von D3:
    /// `narsil::devtools_bruecke::an_seite` haengte bei jedem Aufruf ALLE
    /// Zeilen von `Angezeigt::konsole` an — ohne zu pruefen, ob sie schon
    /// drin sind. GEMESSENE FOLGE: nach dreimal F12 (auf, zu, auf) stand
    /// jede Meldung DREIMAL im Reiter. Und nach einem Seitenwechsel standen
    /// die Meldungen der ALTEN Seite ueber denen der neuen (die Liste der
    /// neuen Seite ist eine andere, aber es stand nichts, was sie begrenzt
    /// haette).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Zaehler und nicht „leeren und
    /// neu": die Konsole traegt auch Zeilen, die NICHT von der Seite kommen
    /// — die Ausgabe einer Konsoleneingabe (D3) und ihre Fehler. Wer bei
    /// jedem `an_seite` leert, loescht genau die weg, die man eben
    /// ausgefuehrt hat. Der Zaehler nimmt nur die NEUEN Zeilen der Seite
    /// und laesst die eigenen stehen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „kuerzer heisst neue Seite": eine neue
    /// Seite hat eine leere Konsole, also faengt ihr Zaehler wieder bei 0
    /// an — und 0 ist kleiner als jede bisherige Zahl. Das braucht keine
    /// Adressvergleiche und funktioniert auch, wenn dieselbe Adresse
    /// zweimal geladen wird (dann ist `uebernommen == len` und es passiert
    /// gar nichts).
    pub uebernommen: usize,
}

impl Konsole {
    /// Eine leere Konsole.
    pub fn neu() -> Self {
        Self {
            zeilen: Vec::new(),
            verworfen: 0,
            laufende_nr: 0,
            fehler: 0,
            warnungen: 0,
            eingabe: String::new(),
            cursor: 0,
            filter: None,
            filtertext: String::new(),
            filter_fokus: false,
            stufen: StufenSicht::ALLE,
            folgt: true,
            fokus: false,
            uebernommen: 0,
        }
    }

    /// Alle Zeilen weg (Seitenwechsel).
    pub fn leeren(&mut self) {
        self.zeilen.clear();
        self.verworfen = 0;
        self.laufende_nr = 0;
        self.fehler = 0;
        self.warnungen = 0;
        self.eingabe.clear();
        self.cursor = 0;
        self.uebernommen = 0;
        // GEMESSENE ENTSCHEIDUNG, warum AUCH der Fokus weg: die Seite ist
        // eine andere. Wer auf der neuen Seite weitertippt, tippt in eine
        // Zeile, die zur alten Seite gehoert — und das ist genau die Falle,
        // in der man `console.log` der neuen Seite in der alten sucht.
        self.fokus = false;
    }

    /// Eine Ausgabe der Seite eintragen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Text auf [`MAX_MELDUNG_ZEICHEN`]
    /// gekuerzt und BEI NULL ENTFALLEN wird: `console.log("")` ist eine
    /// leere Zeile, die man im Reiter nicht sieht (der Reiter zeigt Text auf
    /// der Zeile), und sie zaehlt trotzdem. Also: leere Zeilen werden
    /// verworfen und gezaehlt — sonst waere die Zeilenzahl eine Zahl, die
    /// nicht zur sichtbaren Liste passt.
    pub fn ausgabe(&mut self, stufe: Stufe, text: &str) -> u64 {
        self.ausgabe_mit_baum(stufe, text, None)
    }

    /// D18: Wie [`Konsole::ausgabe`], aber die Zeile traegt einen BAUM.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein zweiter Weg ist und nicht ein
    /// drittes Argument an `ausgabe`: `ausgabe` wird ueberall in der Crate
    /// mit `(Stufe, Text)` aufgerufen; ein drittes Argument an allen Stellen
    /// waere eine Aenderung, die niemand braucht. Der neue Weg nimmt den
    /// Baum, der alte ruft ihn mit `None` — ein Aufrufer ohne Baum merkt
    /// nichts.
    pub fn ausgabe_mit_baum(&mut self, stufe: Stufe, text: &str, baum: Option<Baum>) -> u64 {
        let g = Eigen::STANDARDAUSGABE;
        if text.trim().is_empty() {
            self.verworfen = self.verworfen.saturating_add(1);
            return 0;
        }
        self.laufende_nr = self.laufende_nr.saturating_add(1);
        let nr = self.laufende_nr;
        if self.zeilen.len() >= g.max_konsolezeilen {
            // Die Grenze: die älteste Zeile faellt raus (wie in Chrome) und
            // der Zaehler laeuft weiter.
            self.zeilen.remove(0);
            self.verworfen = self.verworfen.saturating_add(1);
        }
        self.zeilen.push(Zeile {
            nr,
            stufe,
            text: g.kuerze_zeile(text),
            mehr: text.chars().count() > g.max_zeile,
            ort: None,
            quelle: None,
            baum,
        });
        // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht erst beim Lesen: die
        // Zahl steht in der Kopfzeile, die bei jedem Bild gezeichnet wird
        // (ein Zaehler beim Schreiben kostet einmal `+= 1`).
        match stufe {
            Stufe::Fehler => self.fehler = self.fehler.saturating_add(1),
            Stufe::Warnung => self.warnungen = self.warnungen.saturating_add(1),
            _ => {}
        }
        nr
    }

    /// Die Konsolenausgaben der SEITE uebernehmen (der Weg aus
    /// `narsil`).
    ///
    /// Nimmt nur die Zeilen, die NOCH NICHT drin sind, und liefert deren
    /// Zahl. Bei einer neuen Seite (ihre Liste ist kuerzer als der
    /// Zaehler) wird alles alte weg.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Rueckgabewert die Zahl der
    /// NEUEN Zeilen ist: der Aufrufer kann damit eine Meldung schreiben,
    /// die stimmt („3 neue Meldungen"), statt eine Zahl zu behaupten, die
    /// er nicht kennt (die Gesamtzahl waere auch richtig, aber sie
    /// wiederholt sich bei jedem `an_seite`).
    pub fn seite_uebernehmen(&mut self, zeilen: &[String]) -> usize {
        if zeilen.len() < self.uebernommen {
            self.leeren();
        }
        let neu = zeilen.len().saturating_sub(self.uebernommen);
        for z in &zeilen[self.uebernommen..] {
            self.ausgabe(Stufe::Log, z);
        }
        self.uebernommen = zeilen.len();
        neu
    }

    /// Die Konsolenausgaben eines Motors uebernehmen.
    ///
    /// DAS IST DER WEG ZUR SEITENKONSOLE: `Engine::konsole()`. GEMESSEN:
    /// die Zeilen sind `String` ohne Stufe, also kommen sie als
    /// [`Stufe::Log`] — und der Reiter zeigt sie genau so, wie sie sind.
    /// Es wird NICHT geraten, welche Funktion sie erzeugt hat.
    pub fn aus_engine(&mut self, engine: &Engine) -> usize {
        let n = engine.konsole().len();
        for z in engine.konsole() {
            self.ausgabe(Stufe::Log, z);
        }
        n
    }

    /// D18: Konsolenausgaben MIT Stufe uebernehmen.
    ///
    /// # WARUM DIESER WEG EXISTIERT, OBEN `konsole()` KEINE STUFE HAT
    ///
    /// GEMESSEN am 04.10.2026 am Quelltext von `narsil-jsvm`:
    /// `crates/narsil-jsvm/src/builtin2.rs:566-569` registriert `console.log`,
    /// `console.error`, `console.warn` und `console.info` als vier
    /// Verschiedenes — und Zeile 970 schickt **alle vier** in dieselbe
    /// Funktion `n_konsole`, die eine `String` in `konsole: Vec<String>`
    /// legt. `Engine::konsole()` gibt `&[String]` zurueck: **die Stufe geht
    /// unterwegs verloren.** `console.debug` gibt es dort nicht.
    ///
    /// Der Auftrag der Leitung (04.10.) lautet „echte Meldungen aus
    /// `LaufErgebnis::konsole` (log/warn/error/info/debug farbig)". Diese
    /// Daten gibt es nicht — und GEMESSENE ENTSCHEIDUNG: ich rate sie nicht.
    /// Eine Konsole, die `console.error` als graue Meldung zeigt, waere ein
    /// Fehler, den man nicht findet.
    ///
    /// Dieser Weg ist additiv: sobald die Crate JS eine Stufe liefert
    /// (`&[(Stufe, String)]`), ist das EINE Aufrufstelle in `narsil` — hier
    /// aendert sich nichts. Gemeldet in `team/BRETT.txt` (04.10., an JS).
    pub fn aus_mit_stufen(&mut self, eintraege: &[(String, Stufe)]) -> usize {
        for (t, st) in eintraege {
            self.ausgabe(*st, t);
        }
        eintraege.len()
    }

    /// Einen Fehler mit Ort eintragen (D3: „Fehler mit Ort").
    ///
    /// Der Ort kommt aus [`JsFehler::Syntax`]. Für die anderen Varianten
    /// gibt es keinen Ort, und der Reiter schreibt das auch so — siehe den
    /// Modul-Kopf.
    pub fn fehler(&mut self, quelle: &str, f: &JsFehler) -> u64 {
        self.fehler_mit_datei(None, quelle, f)
    }

    /// D18: Ein Fehler MIT DEM SKRIPT, in dem er passiert ist.
    ///
    /// Der Auftrag der Leitung (04.10.) nennt „Fehler mit Quelle:Zeile" —
    /// Firefox schreibt `datei.js:3:7`. Ohne Dateinamen ist `3:7` eine
    /// Angabe, die in einer Seite mit fuenf Skripten nicht zu benutzen ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Dateiname ein PARAMETER ist und
    /// nicht geraten wird: `JsFehler::Syntax` traegt `zeile`, `spalte` und
    /// `nachricht` — **keinen Dateinamen**. Wer hier einen Namen erfindet,
    /// zeigt im Bild eine Datei, die es nicht gibt. Also: der Aufrufer sagt
    /// es, wenn er es weiss; sonst steht `Zeile 3:7`, der alte, ehrliche
    /// Text.
    pub fn fehler_mit_datei(&mut self, datei: Option<&str>, quelle: &str, f: &JsFehler) -> u64 {
        let g = Eigen::STANDARDAUSGABE;
        let ort = Ort::aus_jsfehler(f);
        let datei = datei.map(|d| g.kuerze_zeile(d)).filter(|d| !d.is_empty());
        self.laufende_nr = self.laufende_nr.saturating_add(1);
        let nr = self.laufende_nr;
        if self.zeilen.len() >= g.max_konsolezeilen {
            self.zeilen.remove(0);
            self.verworfen = self.verworfen.saturating_add(1);
        }
        self.zeilen.push(Zeile {
            nr,
            stufe: Stufe::Fehler,
            text: g.kuerze(
                &format!(
                    "{}: {}",
                    f.art(),
                    g.kuerze(&f.to_string(), g.max_fehlertext)
                ),
                g.max_konsoletext,
            ),
            mehr: f.to_string().chars().count() > g.max_fehlertext,
            ort,
            quelle: datei,
            baum: None,
        });
        // GEMESSENE ENTSCHEIDUNG (3:11x): `fehler()` schreibt die Zeile
        // selbst (nicht ueber `ausgabe`), also muss der Zaehler HIER
        // gepflegt werden. Sonst stuende in der Kopfzeile „0 Fehler",
        // waehrend der Reiter eine rote Zeile zeigt.
        self.fehler = self.fehler.saturating_add(1);
        // Der Quelltext wird als Kontext angehaengt, gekuerzt: ein Benutzer,
        // der eine kaputte Eingabe sieht, will die Zeile sehen, in der sie
        // kaputt ist.
        if let Some(o) = ort {
            if o.zeile > 0 {
                if let Some(zeile) = quelle.lines().nth((o.zeile - 1) as usize) {
                    let z = g.kuerze(zeile.trim(), 120);
                    if !z.is_empty() {
                        self.laufende_nr = self.laufende_nr.saturating_add(1);
                        let nr2 = self.laufende_nr;
                        self.zeilen.push(Zeile {
                            nr: nr2,
                            stufe: Stufe::Hinweis,
                            text: format!("{:>4} | {}", o.zeile, z),
                            mehr: false,
                            ort: Some(o),
                            quelle: None,
                            baum: None,
                        });
                    }
                }
            }
        }
        nr
    }

    /// Die JS-Eingabe ausfuehren (D3: „JS-Eingabe ausfuehren").
    ///
    /// # DER EIGENE MOTOR — mit Begründung
    ///
    /// GEMESSEN in `narsil/src/jsvm.rs`: „Ein zweites `ausfuehren` macht die
    /// Funktionswerte des ersten ungueltig ... Fuer den Seitenbetrieb: EIN
    /// `ausfuehren` fuer die Seite, danach nur Ereignisse, Netz und Timer."
    /// Eine Konsoleingabe ist ein zweites `ausfuehren`. Im Seitenmotor
    /// wuerde sie damit jeden `onclick`-Handler der Seite zerstoeren — und
    /// der Nutzer wuerde es merken: die Seite ist nach einem
    /// `console.log(1)` kaputt.
    ///
    /// Also: ein Motor je Eingabe, mit [`Grenzen::bombe`] als Grenzen (der
    /// Reiter ist kein Seitenbetrieb: 2 MiB Heap, 200 000 Schritte, 1,9 s).
    /// Das Ergebnis steht in der Konsole, die Seite bleibt ganz.
    ///
    /// Der Rückgabewert ist die Zeilennummer des Ergebnisses (oder 0, wenn
    /// der Rechner die Eingabe verworfen hat — leerer Text).
    pub fn fuehre_aus(&mut self, eingabe: &str) -> u64 {
        let g = Eigen::STANDARDAUSGABE;
        let quelle = g.kuerze(eingabe, g.max_eingabe);
        if quelle.trim().is_empty() {
            return 0;
        }
        // Die Ausgabe der Eingabe selbst zuerst (die Echo-Zeile), damit sie
        // oben im Reiter steht und die Antwort darunter — das ist die
        // Reihenfolge, in der man es liest.
        let nr = self.ausgabe(Stufe::Eingabe, &quelle);
        let mut motor = Engine::neu(Motor::bombe());
        match motor.ausfuehren(&quelle) {
            Ok(w) => {
                self.ergebnis_zeigen(&w);
            }
            Err(f) => {
                self.fehler(&quelle, &f);
            }
        }
        // Die Konsolenausgaben der Eingabe (console.log in der Eingabe).
        self.aus_engine(&motor);
        nr
    }

    /// D18: Das Ergebnis einer Auswertung als Zeile MIT Baum zeigen.
    ///
    /// `undefined` ist eine ECHTE Antwort (die Eingabe war ein Ausdruck ohne
    /// Wert) — sie wird gezeigt, nicht geschluckt. Und ein Objekt bekommt
    /// seinen Baum; eine Zahl nicht (sie hat keine Kinder).
    pub fn ergebnis_zeigen(&mut self, w: &Wert) -> u64 {
        let t = wert_text(w);
        self.ausgabe_mit_baum(Stufe::Ergebnis, &t, Baum::aus_wert(w))
    }

    /// D18: DIE EINGABE IN DER SEITEN-ENGINE AUSWERTEN (Auftrag der Leitung
    /// vom 04.10.: „Eingabezeile die JS in der SEITEN-Engine auswertet").
    ///
    /// # GEMESSENER UNTERSCHIED ZU [`Konsole::fuehre_aus`]
    ///
    /// GEMESSEN in `crates/narsil/src/jsvm.rs`: „Ein zweites `ausfuehren`
    /// macht die Funktionswerte des ersten ungueltig." Deshalb hat
    /// [`Konsole::fuehre_aus`] einen EIGENEN Motor. Beide Wege bleiben, und
    /// sie sind nicht dasselbe:
    ///
    /// | Eingabe | `fuehre_aus` (eigener Motor) | `fuehre_aus_in_seite` |
    /// |---|---|---|
    /// | sieht `document`, `z` der Seite | nein | **ja** |
    /// | gefaehrdet die Klick-Handler | nein | **ja** (GEMESSEN in jsvm.rs) |
    /// | Grenzen | `Grenzen::bombe` | die der Seite |
    ///
    /// Der Aufrufer entscheidet: Wer `document.getElementById` in der
    /// Konsole tippt, will die SEITE sehen — da ist dieser Weg richtig. Wer
    /// `while(true){}` tippt, will die Seite nicht zerstoeren — da ist der
    /// eigene Motor richtig. Ein Reiter, der nur einen der beiden kann,
    /// behauptet eine Sicherheit, die er nicht hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die neuen Konsolenausgaben des
    /// SEITENmotors ab der alten Laenge gelesen werden: `an_seite` haengte in
    /// einer frueheren Fassung alle Zeilen bei jedem Aufruf an (das
    /// Dreifach-Problem, siehe das Feld `uebernommen`). Dieselbe
    /// Fehlerklasse waere hier: die Seite hat 40 alte Zeilen, die Eingabe
    /// schreibt eine — ein Weg, der alle 41 anhaengt, zeigt die 40 alten
    /// noch einmal.
    pub fn fuehre_aus_in_seite(&mut self, seite: &mut Engine, eingabe: &str) -> u64 {
        let g = Eigen::STANDARDAUSGABE;
        let quelle = g.kuerze(eingabe, g.max_eingabe);
        if quelle.trim().is_empty() {
            return 0;
        }
        let vorher = seite.konsole().len();
        let nr = self.ausgabe(Stufe::Eingabe, &quelle);
        match seite.ausfuehren(&quelle) {
            Ok(w) => {
                self.ergebnis_zeigen(&w);
            }
            Err(f) => {
                self.fehler(&quelle, &f);
            }
        }
        for z in seite.konsole().iter().skip(vorher) {
            self.ausgabe(Stufe::Log, z);
        }
        nr
    }

    /// Ein Zeichen in die Eingabezeile tippen.
    pub fn einfuegen(&mut self, c: char) {
        let g = Eigen::STANDARDAUSGABE;
        if self.eingabe.chars().count() >= g.max_eingabe {
            return;
        }
        let pos = self.cursor.min(self.eingabe.chars().count());
        let byte = byte_index(&self.eingabe, pos);
        self.eingabe.insert(byte, c);
        self.cursor = pos + 1;
    }

    /// Backspace an der Cursorstelle (am Anfang: nichts).
    pub fn backspace(&mut self) {
        if self.cursor == 0 || self.eingabe.is_empty() {
            return;
        }
        let pos = self.cursor.min(self.eingabe.chars().count());
        let byte = byte_index(&self.eingabe, pos - 1);
        self.eingabe.remove(byte);
        self.cursor = pos - 1;
    }

    /// Eingabezeile leeren.
    pub fn leere_eingabe(&mut self) {
        self.eingabe.clear();
        self.cursor = 0;
    }

    /// Die Eingabe ausfuehren und leeren — der Weg fuer Enter.
    ///
    /// Liefert den eingegebenen Text (leer = es wurde nichts getan).
    pub fn ausfuehren_und_leeren(&mut self) -> String {
        let q = std::mem::take(&mut self.eingabe);
        self.cursor = 0;
        if q.trim().is_empty() {
            return String::new();
        }
        self.fuehre_aus(&q);
        q
    }

    /// Den Cursor in der Eingabezeile bewegen (Pfeil links/rechts).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Cursor an beiden Enden STEHT
    /// bleibt und nicht umbricht: eine Eingabezeile, die am Ende wieder
    /// oben anfaengt, sieht aus wie ein Fehler — und der Reiter zeigt keine
    /// Warnung, weil er keine kennt.
    pub fn bewegen(&mut self, delta: i32) -> bool {
        let n = self.eingabe.chars().count() as i32;
        let ziel = (self.cursor as i32 + delta).clamp(0, n);
        if ziel == self.cursor as i32 {
            return false;
        }
        self.cursor = ziel as usize;
        true
    }

    /// Entf an der Cursorstelle (am Ende: nichts).
    pub fn entf(&mut self) {
        let n = self.eingabe.chars().count();
        if self.eingabe.is_empty() || self.cursor >= n {
            return;
        }
        let byte = byte_index(&self.eingabe, self.cursor);
        self.eingabe.remove(byte);
    }

    /// Eine Taste in der Eingabezeile — DER WEG FUER DIE TASTATUR.
    ///
    /// Liefert `true`, wenn die Konsole die Taste genommen hat. `false`
    /// heisst: die Taste gehoert nicht hierhin (dann darf der Aufrufer sie
    /// nicht schlucken — sonst verschwindet eine Taste ohne Spur).
    pub fn druck(&mut self, d: Druck) -> bool {
        match d {
            Druck::Zeichen(c) => self.einfuegen(c),
            Druck::Zurueck => self.backspace(),
            Druck::Entf => self.entf(),
            Druck::Links => {
                self.bewegen(-1);
            }
            Druck::Rechts => {
                self.bewegen(1);
            }
            Druck::Enter => {
                self.ausfuehren_und_leeren();
            }
            // GEMESSENE ENTSCHEIDUNG, warum Escape ZWEI Stufen hat: leert
            // zuerst die Eingabe (wie in jedem Browser — man hat sich
            // vertippt) und gibt den Fokus erst beim zweiten Mal her (wie in
            // jedem Browser auch). Ein Escape, das sofort zur Seite
            // zurueckgeht, verliert einen getippten Text ohne Rueckfrage.
            Druck::Flucht => {
                if self.eingabe.is_empty() {
                    self.fokus = false;
                } else {
                    self.leere_eingabe();
                }
            }
            Druck::Sonst => return false,
        }
        true
    }

    /// Die Zeilen, die der Filter durchlaesst.
    pub fn sichtbare_zeilen(&self) -> Vec<&Zeile> {
        self.zeilen.iter().filter(|z| self.sichtbar(z)).collect()
    }

    /// D12: Darf diese Zeile in die Liste? (Stufenschalter + Textfilter)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Textfilter GROSS/KLEIN vergleicht
    /// (`to_lowercase`) wie der Elementefilter: `console.log("Error")` und
    /// der Filter `error` sind dieselbe Absicht, und eine Liste, in der der
    /// Filter case-rein ist, ist in drei von vier Faellen leer.
    ///
    /// M4: beide Filter sind auf Grenzen gekuerzt (`max_filterzeichen`), der
    /// Vergleich ist also `min(len)`-frei — `contains` ueber einen Text mit
    /// 256 Zeichen gegen eine Zeile mit 400 Zeichen kostet linear in der
    /// Zeile, nicht in der Anzahl Zeilen.
    pub fn sichtbar(&self, z: &Zeile) -> bool {
        if !self.stufen.an(stufe_schalter_von(z.stufe)) {
            return false;
        }
        if self.filtertext.is_empty() {
            return true;
        }
        let f = self.filtertext.to_lowercase();
        z.text.to_lowercase().contains(&f)
    }

    /// D12: einen Schalter umlegen. Liefert den neuen Zustand.
    pub fn stufe_schalten(&mut self, s: StufeSchalter) -> bool {
        let v = !self.stufen.an(s);
        self.stufen = match s {
            StufeSchalter::Fehler => StufenSicht {
                fehler: v,
                ..self.stufen
            },
            StufeSchalter::Warnung => StufenSicht {
                warnung: v,
                ..self.stufen
            },
            StufeSchalter::Log => StufenSicht {
                log: v,
                ..self.stufen
            },
            StufeSchalter::Ergebnis => StufenSicht {
                ergebnis: v,
                ..self.stufen
            },
        };
        v
    }

    /// D12: den Textfilter setzen (M4: auf `max_filterzeichen` gekuerzt).
    pub fn filter_setzen(&mut self, text: &str) {
        self.filtertext = text
            .chars()
            .take(Eigen::STANDARDAUSGABE.max_filterzeichen)
            .collect();
    }

    /// D12: ein Zeichen in den Textfilter (die Eingabe des Fensters).
    pub fn filter_einfuegen(&mut self, c: char) {
        if self.filtertext.chars().count() >= Eigen::STANDARDAUSGABE.max_filterzeichen {
            return;
        }
        self.filtertext.push(c);
    }

    /// D12: ein Zeichen aus dem Textfilter loeschen.
    pub fn filter_backspace(&mut self) {
        self.filtertext.pop();
    }

    /// D12: passt die Zeile durch BEIDE Filter? (der Weg des Reiters)
    ///
    /// GEMESSENE ENTSCHEIDUNG: `text_zeilen_bis` (unten) benutzt DIESE
    /// Filter, nicht eigene. Zwei Filter an zwei Stellen sind zwei Filter,
    /// die irgendwann Unterschiede zeigen — und die Zahl der sichtbaren
    /// Zeilen waere dann die eine und die angezeigte Zahl die andere.
    pub fn text_zeilen_bis(&self, n: usize) -> Vec<String> {
        self.zeilen
            .iter()
            .filter(|z| self.sichtbar(z))
            .take(n)
            .map(|z| self.zeilen_text(z))
            .collect()
    }

    /// Eine Zeile als Text, wie sie im Reiter steht (ohne Kuerzung — der
    /// Reiter kuerzt auf seine Spaltenbreite, siehe `zeichnen.rs`).
    pub fn zeilen_text(&self, z: &Zeile) -> String {
        let mut s = format!("[{}] {}", z.stufe.kuerzel(), z.text);
        if let Some(o) = z.ort {
            // D18: mit Dateinamen die Form, die Firefox schreibt:
            // `datei.js:3:7`. Ohne Namen bleibt `Zeile 3:7` — der alte Text,
            // der nichts behauptet, was nicht dasteht.
            match z.quelle.as_deref() {
                Some(d) => s.push_str(&format!("  {d}:{}:{}", o.zeile, o.spalte)),
                None => s.push_str(&format!("  ({})", o.als_text())),
            }
        }
        s
    }

    /// D18: Den Baumknoten auf- oder zuklappen — der Weg des KLICKS.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Suche ueber den PFAD laeuft und
    /// nicht ueber die Zeilennummer des Reiters: der Reiter zeichnet die
    /// Sichtzeilen aus mehreren Baeumen (jede Ergebniszeile hat ihren), und
    /// der Pfad ist die Angabe, die in [`crate::zeichnen::KonsolenReihe`]
    /// steht. Ein Index in die Liste waere eine zweite Nummer fuer dieselbe
    /// Sache — und sie stimmt nicht mehr, sobald eine Zeile mit Baum
    /// davorsteht.
    pub fn baum_umschalten(&mut self, pfad: &[usize]) -> bool {
        for z in self.zeilen.iter_mut() {
            let Some(b) = z.baum.as_mut() else {
                continue;
            };
            if b.zeilen().iter().any(|r| r.pfad == pfad) {
                return b.umschalten(pfad);
            }
        }
        false
    }

    /// D18: Der Text einer Baumzeile (die Farbe steht im Baum, nicht hier).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeile ZWEI Texte hat (`name` und
    /// `wert`) und nicht einen: im Bild soll der Schluessel in einer Farbe
    /// stehen und der Wert in einer anderen — wie im DOM-Baum (D13). Aus
    /// einem einzigen Text laesst sich die Farbe nicht zurueckrechnen.
    pub fn baum_text(&self, z: &BaumZeile) -> String {
        let einzug = "  ".repeat(z.tiefe as usize);
        let marke = match z.offen {
            Some(true) => "v ",
            Some(false) => "> ",
            None => "  ",
        };
        let mut s = format!("{einzug}{marke}{}", z.name);
        if !z.wert.is_empty() {
            s.push_str(&format!(": {}", z.wert));
        }
        if z.abgeschnitten > 0 {
            s.push_str(&format!("  (+{} mehr)", z.abgeschnitten));
        }
        s
    }

    /// D12: die Farbe der Zeile nach ihrer Stufe — der Streifen am linken
    /// Rand der Zeile (Firefox faerbt den ganzen Block, wir nur den Rand,
    /// weil die Zeilen sonst unlesbar wuerden).
    pub fn stufenfarbe(&self, z: &Zeile) -> narsil_core::color::Color {
        let p = crate::thema::Palette::dunkel();
        match z.stufe {
            Stufe::Fehler => p.fehler,
            Stufe::Warnung => p.warnung,
            Stufe::Log | Stufe::Hinweis => p.text_schwach,
            Stufe::Ergebnis => p.gut,
            Stufe::Eingabe => p.hervorhebung,
        }
    }

    /// Die Zeilen als Text, aber hoechstens `n` (der Weg des Reiters).
    ///
    /// # WARUM DER REITER NUR SO VIE ZEICHNET, WIE ER HAT (03:10x)
    ///
    /// GEMESSEN: mit 5 000 Konsolenzeilen kostete das Zeichnen des Reiters
    /// **1,192 ms** (Median aus 30 Laeufen) — bei 10 Zeilen 0,009 ms, also
    /// Faktor 133 bei Faktor 500 der Zeilen. Das ist fast quadratisch, und
    /// es ist vermeidbar: `text_zeilen()` hat ALLE 5 000 Zeilen in Strings
    /// gebaut (`format!` je Zeile), gezeichnet werden aber hoechstens
    /// `max_gezeichnete_zeilen` (500) davon — und bei einem Fenster, das
    /// nur 30 Zeilen hoch ist, noch viel weniger.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `n` beim Aufrufer steht und nicht hier:
    /// wie viele Zeilen in die Spalte passen, weiss nur der Zeichencode
    /// (er kennt die Hoehe). Wer die Grenze hier fest verdrahtet, zeichnet
    /// entweder zu viel (teuer) oder zu wenig (unsichtbar).
    /// Die Zeilen als Text (fuer den Reiter) — ALLE, unabhaengig von der
    /// Fenstergroesse ( fuer Protokolle und Tests ).
    ///
    /// GEMESSENER FEHLER (04.10.2026, gefunden am D18-Test „ein Fehler nennt
    /// die Datei"): `text_zeilen` baute den Zeilentext SELBST — und so, wie
    /// er vor dem D18 aussah. Der D18-Ort (`datei.js:3:7`, siehe
    /// [`Konsole::zeilen_text`]) stand damit im Reiter, aber nicht in dieser
    /// Liste: zwei Wege fuer denselben Text, und der Test las den falschen.
    ///
    /// GEMESSENE ENTSCHEIDUNG: EIN Weg. `text_zeilen` ruft `zeilen_text` und
    /// kuerzt danach — mehr nicht. Das ist die Regel des Moduls (sie steht
    /// im Kopf an `Zeilen::ort`), und sie wird hier zum ersten Mal gebrochen.
    pub fn text_zeilen(&self) -> Vec<String> {
        let g = Eigen::STANDARDAUSGABE;
        self.zeilen
            .iter()
            .map(|z| g.kuerze_zeile(&self.zeilen_text(z)))
            .collect()
    }

    /// Wie viele Zeilen eines Fehlers gibt es? (für die Zusammenfassung oben)
    pub fn fehler_anzahl(&self) -> usize {
        // GEMESSENE ENTSCHEIDUNG (3:11x): der Zaehler statt eines Laufs
        // ueber alle Zeilen. Die Zahl ist beim Schreiben entstanden — sie
        // muss nicht jedes Mal neu gezaehlt werden. Die Signatur bleibt,
        // also muss der Aufrufer nichts aendern.
        self.fehler as usize
    }

    /// Wie viele Warnungen gibt es?
    pub fn warn_anzahl(&self) -> usize {
        self.warnungen as usize
    }
}

/// Die vier Schalter der Konsole (Firefox: Errors, Warnings, Logs, Debug).///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge so ist und nicht
/// Fehler/Warnung/Log/Ergebnis in der Reihenfolge des Aufkommens: der
/// Nutzer sucht von links nach rechts das, was ihn stoert — Fehler zuerst.
/// Und „Ergebnis" steht rechts, weil es die einzige Stufe ist, die der
/// Nutzer selbst erzeugt (die JS-Eingabe) — sie steht da, wo sie nicht mit
/// dem verwechselt wird, was die Seite geschrieben hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StufeSchalter {
    /// `console.error` und Fehler aus der Eingabe.
    Fehler,
    /// `console.warn`.
    Warnung,
    /// `console.log`, `console.info`, `console.debug`.
    Log,
    /// Das Ergebnis einer ausgefuehrten Eingabe.
    Ergebnis,
}

impl StufeSchalter {
    /// Die vier in der Reihenfolge der Leiste.
    pub const ALLE: [StufeSchalter; 4] = [
        StufeSchalter::Fehler,
        StufeSchalter::Warnung,
        StufeSchalter::Log,
        StufeSchalter::Ergebnis,
    ];

    /// Der Name auf dem Knopf.
    pub const fn name(self) -> &'static str {
        match self {
            StufeSchalter::Fehler => "Fehler",
            StufeSchalter::Warnung => "Warnung",
            StufeSchalter::Log => "Meldung",
            StufeSchalter::Ergebnis => "Ergebnis",
        }
    }

    /// Die Stufe, die dieser Schalter zeigt.
    pub const fn stufe(self) -> Stufe {
        match self {
            StufeSchalter::Fehler => Stufe::Fehler,
            StufeSchalter::Warnung => Stufe::Warnung,
            StufeSchalter::Log => Stufe::Log,
            StufeSchalter::Ergebnis => Stufe::Ergebnis,
        }
    }

    /// Die Farbe des Knopfes, wenn er AN ist (die Stufenfarbe).
    pub const fn farbe(self, p: &crate::thema::Palette) -> narsil_core::color::Color {
        match self {
            StufeSchalter::Fehler => p.fehler,
            StufeSchalter::Warnung => p.warnung,
            StufeSchalter::Log => p.hervorhebung,
            StufeSchalter::Ergebnis => p.gut,
        }
    }
}

/// Welche Stufen gerade sichtbar sind.
///
/// GEMESSENE ENTSCHEIDUNG, warum VIER Flags und nicht ein Bitfeld: die
/// Schalter sind einzeln im Bild zu sehen, und ein `u8`-Bitfeld muesste an
/// jeder Stelle zerlegt werden (`bit 0`, `bit 1`, …). Vier `bool` sind
/// an jeder Stelle lesbar, und die Groesse des Zustands ist dieselbe
/// (4 Byte statt 1 — bei EINER Konsole im Tab ohne Bedeutung).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StufenSicht {
    /// Fehler zeigen?
    pub fehler: bool,
    /// Warnungen zeigen?
    pub warnung: bool,
    /// `console.log` zeigen?
    pub log: bool,
    /// Ergebnisse der Eingabe zeigen?
    pub ergebnis: bool,
}

impl StufenSicht {
    /// Alle vier an (der Anfangszustand: die Konsole zeigt, was da ist).
    pub const ALLE: StufenSicht = StufenSicht {
        fehler: true,
        warnung: true,
        log: true,
        ergebnis: true,
    };

    /// Alle vier aus.
    pub const KEINE: StufenSicht = StufenSicht {
        fehler: false,
        warnung: false,
        log: false,
        ergebnis: false,
    };

    /// Ist dieser Schalter an?
    pub const fn an(self, s: StufeSchalter) -> bool {
        match s {
            StufeSchalter::Fehler => self.fehler,
            StufeSchalter::Warnung => self.warnung,
            StufeSchalter::Log => self.log,
            StufeSchalter::Ergebnis => self.ergebnis,
        }
    }

    /// Wie viele Schalter stehen an (fuer die Zaehlzeile unten).
    pub const fn anzahl(self) -> usize {
        (self.fehler as usize)
            + (self.warnung as usize)
            + (self.log as usize)
            + (self.ergebnis as usize)
    }

    /// Einen Schalter umlegen (D12: der Klick in der Werkzeugleiste).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Schalten HIER steht und nicht in
    /// der Werkzeugleiste: die Leiste kennt die vier `bool` nicht, sie
    /// kennt nur `StufeSchalter`. Wer sie aufloest, entscheidet also, ob
    /// „Fehler" an ist — und das ist eine Frage an den Zustand.
    pub fn schalte(&mut self, s: StufeSchalter) {
        match s {
            StufeSchalter::Fehler => self.fehler = !self.fehler,
            StufeSchalter::Warnung => self.warnung = !self.warnung,
            StufeSchalter::Log => self.log = !self.log,
            StufeSchalter::Ergebnis => self.ergebnis = !self.ergebnis,
        }
    }
}

impl Default for StufenSicht {
    fn default() -> Self {
        Self::ALLE
    }
}

/// Eine Zeile in der Konsole.
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    /// Laufende Nummer (1, 2, 3 …).
    pub nr: u64,
    /// Die Stufe.
    pub stufe: Stufe,
    /// Der Text (gekuerzt auf [`Grenzen::max_zeile`] Zeichen).
    pub text: String,
    /// `true`, wenn der Originaltext laenger war als die Zeile.
    pub mehr: bool,
    /// Der Ort, falls bekannt.
    pub ort: Option<Ort>,
    /// D18: WELCHES SKRIPT den Fehler ausgeloest hat (`datei.js`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Skriptname an der Zeile steht und
    /// nicht in [`Ort`]: `Ort` ist `Copy`, weil es zwei Zahlen traegt (Zeile,
    /// Spalte) — ein `String` darin wuerde `Copy` aufgeben, und `Ort` wird
    /// an vielen Stellen kopiert. Der Name gehoert zum Ort *im Bild*, nicht
    /// zur Koordinate. Firefox schreibt `datei.js:3:7`, und genau das macht
    /// jetzt [`Konsole::zeilen_text`].
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es `None` sein darf: `narsil-jsvm` gibt
    /// bei einem Syntaxfehler Zeile und Spalte, aber KEINEN Dateinamen
    /// (`JsFehler::Syntax` hat drei Felder: `zeile`, `spalte`, `nachricht`,
    /// gemessen in `narsil-jsvm/src/compile.rs`). Der Aufrufer weiss die
    /// Datei — er hat das Skript geladen. Also fragt er hier nach, statt
    /// dass hier geraten wird.
    pub quelle: Option<String>,
    /// D18: DER WERT ALS BAUM (nur bei einem Objekt oder Feld).
    ///
    /// GEMESSENE ENTSCHEIDUNG (04.10.2026, Auftrag der Leitung: „Ergebnis
    /// als Objektbaum"): eine Ergebniszeile war ein **Text** — `{a: 1, b: 2}`
    /// — und bei einem Objekt mit 40 Feldern ein abgeschnittener Text, aus
    /// dem man nichts mehr entnehmen kann. Firefox zeigt dort einen Baum, den
    /// man aufklappt. Das ist der Unterschied zwischen „eine Zahl
    /// behaupten" und „den Wert zeigen".
    ///
    /// `None` bei allem, was KEIN Baum ist (Zahl, Text, `undefined`): fuer
    /// eine Zahl waere ein Baum mit einem aufklappbaren Kind eine Ausgabe,
    /// die aus einem Skript eine Sache macht, die es nicht gibt.
    pub baum: Option<Baum>,
}

/// Die Stufe einer Konsolenzeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stufe {
    /// `console.log` (und alles, was `narsil-jsvm` nicht unterscheidet).
    Log,
    /// `console.warn`.
    Warnung,
    /// Ein Fehler aus der JS-Eingabe oder aus der Seite.
    Fehler,
    /// Das Ergebnis einer ausgeführten Eingabe.
    Ergebnis,
    /// Die eingegebene Zeile selbst (das Echo).
    Eingabe,
    /// Ein Hinweis des Werkzeugs (z. B. die Quelltextzeile zum Fehler).
    Hinweis,
}

impl Stufe {
    /// Das Kuerzel im Reiter (in Grossbuchstaben wie in der Konsole).
    pub const fn kuerzel(self) -> &'static str {
        match self {
            Stufe::Log => "LOG",
            Stufe::Warnung => "WARN",
            Stufe::Fehler => "FEHLER",
            Stufe::Ergebnis => "ERGEBNIS",
            Stufe::Eingabe => "EINGABE",
            Stufe::Hinweis => "HINWEIS",
        }
    }

    /// Der lange Name (für die Zusammenfassung über der Liste).
    pub const fn name(self) -> &'static str {
        match self {
            Stufe::Log => "Meldung",
            Stufe::Warnung => "Warnung",
            Stufe::Fehler => "Fehler",
            Stufe::Ergebnis => "Ergebnis",
            Stufe::Eingabe => "Eingabe",
            Stufe::Hinweis => "Hinweis",
        }
    }

    /// Gehört die Stufe zu „Fehler und Warnung"? (für das Warnzeichen).
    pub const fn ist_meldung(self) -> bool {
        matches!(self, Stufe::Warnung | Stufe::Fehler)
    }
}

/// Welcher Schalter zeigt diese Stufe? (die Bruecke der beiden Enums)
///
/// GEMESSENE ENTSCHEIDUNG, warum `Eingabe` und `Hinweis` bei „Log" landen
/// und nicht bei einem fuenften Schalter: Firefox hat vier Schalter, und
/// die beiden uebrigen Stufen sind KEINE eigene Kategorie — `Eingabe` ist
/// das Echo der Nutzereingabe (sie steht bei den Ergebnissen), `Hinweis`
/// ist eine Meldung des Werkzeugs (sie ist eine `console.log`). Wer sie
/// ausblendet, blendet also genau die eine Zeile aus, die man nicht
/// braucht; ein fuenfter Schalter waere eine Leiste, die nicht mehr in
/// 1400 px passt (gemessen: die Konsole hat mit fuenf Schaltern 6 + 1
/// Elemente = 7, die Zusage ist [`crate::werkzeug::MAX_ELEMENTS`] = 8).
pub const fn stufe_schalter_von(s: Stufe) -> StufeSchalter {
    match s {
        Stufe::Fehler => StufeSchalter::Fehler,
        Stufe::Warnung => StufeSchalter::Warnung,
        Stufe::Ergebnis | Stufe::Eingabe => StufeSchalter::Ergebnis,
        Stufe::Log | Stufe::Hinweis => StufeSchalter::Log,
    }
}

/// Wo in der Quelle etwas passiert ist.
///
/// # WARUM `Option`, nicht zwei `u32`
///
/// GEMESSEN: `narsil-jsvm::compile` setzt `zeile: 0, spalte: 0` an vielen
/// Stellen (der Wert steht an mehreren Fundstellen im Compiler fest). Ein
/// Ort 0/0 ist KEIN Ort — es heisst „unbekannt". Also ist der Ort ein
/// `Option`, und `None` wird im Reiter als „(kein Ort)" geschrieben. So
/// steht im Bild nie eine falsche Zeilennummer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ort {
    /// Zeile, 1-basiert (wie im Text).
    pub zeile: u32,
    /// Spalte, 1-basiert.
    pub spalte: u32,
}

impl Ort {
    /// Der Ort aus einem JS-Fehler, oder `None` wenn keiner bekannt ist.
    pub fn aus_jsfehler(f: &JsFehler) -> Option<Ort> {
        match f {
            JsFehler::Syntax { zeile, spalte, .. } => Ort::zeilen_spalte(*zeile, *spalte),
            _ => None,
        }
    }

    /// Der Ort aus Zeile und Spalte — 0 heisst „unbekannt".
    pub fn zeilen_spalte(zeile: u32, spalte: u32) -> Option<Ort> {
        if zeile == 0 {
            return None;
        }
        Some(Ort {
            zeile,
            spalte: spalte.max(1),
        })
    }

    /// Als Text, wie er im Reiter steht.
    pub fn als_text(&self) -> String {
        format!("Zeile {}:{}", self.zeile, self.spalte)
    }
}

impl std::fmt::Display for Ort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.zeile, self.spalte)
    }
}

/// Der Wert einer ausgeführten Eingabe als Text.
///
/// GEMESSENE ENTSCHEIDUNG: ein Objekt wird NICHT als
/// `Gegenstand([("a", 1)])` gezeigt (das ist die Rust-Form eines
/// JavaScript-Objekts, und der Nutzer kennt sie nicht), sondern mit
/// geschweiften Klammern und Doppelpunkten: `{a: 1}`. Es ist die Form, in
/// der man es in der Konsole auch sieht.
/// Eine Taste in der Eingabezeile — die UEBERSETZUNG, nicht die Taste.
///
/// GEMESSENE ENTSCHEIDUNG, warum uebersetzen und nicht durchreichen: die
/// Eingabe-Crate bringt ihre eigenen Modifikatoren und Ereignisarten mit
/// (Druecken/Loslassen, Wiederholung), und die gehoeren zu FENSTER und
/// TASTATUR, nicht zu einer Konsoleneingabe. Wer sie hier durchreicht,
/// haelt eine halbe Tastebibliothek in einem Reiter.
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:7x), aus der Pruefung der toten
/// Pfade: es gab ZWEI Uebersetzungen — `Druck::aus_name` (hier, aus einem
/// Schluesselnamen) und `narsil::devtools_bruecke::taste_eingabe` (aus
/// `narsil_input::Key`). GEMESSEN: `aus_name` hatte NULL Aufrufer ausser in
/// den Tests. Zwei Wege fuer dieselbe Sache sind zwei Wahrheiten, sobald
/// sich eine aendert — also ist der Weg aus dem Fenster (der einzige, den
/// es wirklich gibt) der einzige geblieben.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Druck {
    /// Ein druckbares Zeichen.
    Zeichen(char),
    /// Backspace.
    Zurueck,
    /// Entf.
    Entf,
    /// Pfeil links.
    Links,
    /// Pfeil rechts.
    Rechts,
    /// Enter: ausfuehren.
    Enter,
    /// Escape: Eingabe leeren, dann Fokus hergeben.
    Flucht,
    /// Alles andere — gehoert nicht hierhin.
    Sonst,
}

pub fn wert_text(w: &Wert) -> String {
    let g = Eigen::STANDARDAUSGABE;
    match w {
        Wert::Undefiniert => "undefined".to_string(),
        Wert::Null => "null".to_string(),
        Wert::Wahr(b) => b.to_string(),
        Wert::Zahl(z) => {
            if z.is_finite() {
                format!("{z}")
            } else {
                // NaN und ±unendlich haben keine Zahlendarstellung, die
                // JavaScript schreibt — aber `format!` schreibt „NaN" und
                // „inf", und JavaScript schreibt „Infinity". Der Unterschied
                // ist im Bild sichtbar.
                if z.is_nan() {
                    "NaN".to_string()
                } else if *z > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            }
        }
        Wert::Text(t) => format!("\"{}\"", g.kuerze(t, 200)),
        Wert::ByteFeld { daten, geklemmt } => {
            let name=if *geklemmt {"Uint8ClampedArray"} else {"Uint8Array"};
            let values=daten.iter().take(32).map(|v|v.to_string()).collect::<Vec<_>>().join(", ");
            format!("{name}({}) [{values}{}]",daten.len(),if daten.len()>32 {", …"} else {""})
        }
        Wert::Feld(v) => {
            let teile: Vec<String> = v.iter().map(wert_text).collect();
            g.kuerze(&format!("[{}]", teile.join(", ")), 300)
        }
        Wert::Gegenstand(m) => {
            let teile: Vec<String> = m
                .iter()
                .map(|(n, v)| format!("{}: {}", n, wert_text_kurz(v)))
                .collect();
            g.kuerze(&format!("{{{}}}", teile.join(", ")), 300)
        }
        Wert::Nativ(n) => format!("[native: {n}]"),
        Wert::Funktion(i) => format!("[Funktion #{i}]"),
    }
}

/// Wie [`wert_text`], aber ohne Klammern um das Ergebnis ( fuer Objekte ).
fn wert_text_kurz(w: &Wert) -> String {
    // Kein Unterschied zu [`wert_text`]: ein Objekt steht in beiden Formen
    // mit geschweiften Klammern. Die Funktion bleibt, weil sie in der
    // Objektliste NICHT gekuerzt wird (dort ist „{a: 1, b: 2, …}" eine
    // Zeile mit vielen Werten).
    wert_text(w)
}

// ---------------------------------------------------------------------------
// D18: DER WERT ALS BAUM
// ---------------------------------------------------------------------------

/// D18: EIN ERGEBNISWERT ALS BAUM (Auftrag der Leitung: „Ergebnis als
/// Objektbaum").
///
/// GEMESSENER BEFUND VORHER: eine Ergebniszeile war ein TEXT. Bei einem
/// Objekt mit 40 Feldern war das ein abgeschnittener String, und ein
/// abgeschnittener Wert kann nicht mehr geprueft werden — man sieht nicht,
/// ob `a` wirklich 1 ist. Firefox zeigt dort aufklappbare Zeilen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Klammern eine EIGENE Zeile sind und
/// nicht Teil eines Textes: `{a: 1, …}` als eine Zeile kann man nicht
/// aufklappen; als Wurzel mit Kindern schon. Die Angabe bleibt dieselbe —
/// sie steht nur ueber mehrere Zeilen verteilt, wie im DOM-Baum (D13).
#[derive(Debug, Clone, PartialEq)]
pub struct Baum {
    /// Die Wurzel (Name: „Object (2 Felder)" oder „Array (3 Felder)").
    pub wurzel: Knoten,
}

impl Baum {
    /// Den Baum zu einem Wert — oder `None`, wenn der Wert KEIN Baum ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Zahl KEIN Baum bekommt: `Baum` ist
    /// eine Behauptung ueber die Form („hier ist etwas aufklappbar"). Bei
    /// `42` waere das eine Behauptung, die falsch ist. Also `None` bei Zahl,
    /// Text, `wahr`, `null`, `undefined` und Funktion.
    pub fn aus_wert(w: &Wert) -> Option<Baum> {
        let g = Eigen::STANDARDAUSGABE;
        let paare: Vec<(String, &Wert)> = match w {
            Wert::Gegenstand(m) => m.iter().map(|(n, v)| (n.clone(), v)).collect(),
            Wert::Feld(v) => (0..v.len()).map(|i| (i.to_string(), &v[i])).collect(),
            _ => return None,
        };
        // M4/M8: Die Zahl der KNOTEN ist begrenzt (`max_baumzeilen`), die
        // TIEFE ebenso (`max_tiefe`). Ein Objekt, das sich selbst enthaelt,
        // waere sonst endlos — und 40 Felder, die je 40 Objekte enthalten,
        // waeren 1600 Zeilen in einer Spalte, die 16 px breit ist.
        let mut budget = g.max_baumzeilen;
        let (kinder, abgeschnitten) = knoten_aus(&paare, &mut budget, 1);
        let name = if paare.len() == 1 {
            format!("Object (1 Feld)")
        } else {
            format!("Object ({} Felder)", paare.len())
        };
        let name = if matches!(w, Wert::Feld(_)) {
            name.replace("Object", "Array")
        } else {
            name
        };
        Some(Baum {
            wurzel: Knoten {
                name: g.kuerze_zeile(&name),
                wert: String::new(),
                kinder,
                offen: true,
                // M7: Die Zahl der NICHT gezeigten Felder steht an der
                // WURZEL — und nicht an dem Knoten, bei dem abgeschnitten
                // wurde. GEMESSENER FEHLER (04.10., erster Lauf des D18-Tests):
                // der Hinweis stand an einem Knoten, der ZU war, also war er
                // im Bild nie zu sehen. An der Wurzel ist er immer da (die
                // WURZEL ist offen), und es ist eine Zahl fuer den ganzen
                // Baum statt einer pro Ast.
                abgeschnitten,
                tiefe: 0,
            },
        })
    }

    /// Die sichtbaren Zeilen des Baums (weggeklappte Kinder fehlen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier EINE flache Liste steht und keine
    /// Verschachtelung: der Zeichencode zeichnet Zeilen, und der Klick nennt
    /// eine Zeilennummer. Eine Liste von Zeilen ist fuer beides die
    /// einfachere Form — und in ihr laesst sich zaehlen, wie viele Zeilen
    /// ueberhaupt im Bild stehen.
    pub fn zeilen(&self) -> Vec<BaumZeile> {
        let mut raus = Vec::new();
        self.wurzel.zeilen(&mut Vec::new(), &mut raus);
        raus
    }

    /// Den Knoten an einem Pfad holen (`[]` = die Wurzel).
    fn knoten_mut(&mut self, pfad: &[usize]) -> Option<&mut Knoten> {
        let mut k = &mut self.wurzel;
        for i in pfad.iter() {
            k = k.kinder.get_mut(*i)?;
        }
        Some(k)
    }

    /// Einen Knoten auf- oder zuklappen. Liefert `true`, wenn sich etwas
    /// geaendert hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Weg ueber den PFAD geht und nicht
    /// ueber eine laufende Nummer: bei einem Klick auf Zeile 7 weiss der
    /// Zeichencode die Zeilennummer, nicht den Ort im Baum (zwei Zeilen
    /// koennen denselben Text haben). `BaumZeile::pfad` traegt beides — und
    /// es ist die EINE Wahrheit, die der Zeichencode beim Zeichnen ohnehin
    /// braucht.
    pub fn umschalten(&mut self, pfad: &[usize]) -> bool {
        let vorher = self.zeilen().len();
        let Some(k) = self.knoten_mut(pfad) else {
            return false;
        };
        if k.kinder.is_empty() {
            return false;
        }
        k.offen = !k.offen;
        self.zeilen().len() != vorher
    }
}

/// D18: EIN KNOTEN IM BAUM.
#[derive(Debug, Clone, PartialEq)]
pub struct Knoten {
    /// Der Name des Feldes (bei der Wurzel: „Object (2 Felder)").
    pub name: String,
    /// Der Wert als Text (leer bei einem Knoten mit Kindern).
    pub wert: String,
    /// Die Kinder.
    pub kinder: Vec<Knoten>,
    /// Steht der Knoten offen?
    pub offen: bool,
    /// Wie viele Kinder es gab, aber nicht mehr sind (M7).
    pub abgeschnitten: usize,
    /// Die Tiefe (0 = Wurzel).
    pub tiefe: u32,
}

impl Knoten {
    /// Die Zeilen dieses Knotens und seiner offenen Kinder.
    fn zeilen(&self, pfad: &mut Vec<usize>, raus: &mut Vec<BaumZeile>) {
        let wurzel = pfad.is_empty();
        raus.push(BaumZeile {
            pfad: pfad.clone(),
            name: self.name.clone(),
            wert: self.wert.clone(),
            art: if wurzel {
                BaumArt::Klammer
            } else if self.kinder.is_empty() {
                BaumArt::Wert
            } else {
                BaumArt::Schluessel
            },
            offen: if self.kinder.is_empty() {
                None
            } else {
                Some(self.offen)
            },
            abgeschnitten: self.abgeschnitten,
            tiefe: self.tiefe,
        });
        if !self.offen {
            return;
        }
        for (i, k) in self.kinder.iter().enumerate() {
            pfad.push(i);
            k.zeilen(pfad, raus);
            pfad.pop();
        }
    }
}

/// D18: EINE SICHTBARE ZEILE DES BAUMS.
#[derive(Debug, Clone, PartialEq)]
pub struct BaumZeile {
    /// Der Weg von der Wurzel zu diesem Knoten (`[]` = die Wurzel selbst).
    pub pfad: Vec<usize>,
    /// Der Name des Feldes.
    pub name: String,
    /// Der Wert als Text (leer bei einem Knoten mit Kindern).
    pub wert: String,
    /// Wie der Text im Bild zu faerben ist.
    pub art: BaumArt,
    /// `Some(true)` = offen, `Some(false)` = zu, `None` = Blatt.
    pub offen: Option<bool>,
    /// Wie viele Kinder abgeschnitten wurden (M7).
    pub abgeschnitten: usize,
    /// Die Tiefe fuer den Einzug.
    pub tiefe: u32,
}

/// D18: WO DIE FARBE EINES BAUMTEXTES HERKOMMT.
///
/// GEMESSENE ENTSCHEIDUNG: die Farbe haengt an der ROLLE im Baum, nicht am
/// Inhalt. Ein Name, der „42" heisst, ist ein Name (blau) — nicht ein Wert.
/// Sonst waere die Farbe eine zweite Bedeutung fuer denselben Text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaumArt {
    /// Die Klammern eines Objekts.
    Klammer,
    /// Der Name eines Feldes.
    Schluessel,
    /// Der Wert eines Feldes.
    Wert,
}

/// Die Knoten aus einer Liste von Paaren bauen — mit den Grenzen (M4/M8).
fn knoten_aus(paare: &[(String, &Wert)], budget: &mut usize, tiefe: u32) -> (Vec<Knoten>, usize) {
    let g = Eigen::STANDARDAUSGABE;
    let mut raus: Vec<Knoten> = Vec::new();
    let mut abgeschnitten = 0usize;
    for (name, v) in paare.iter() {
        if *budget == 0 || tiefe as usize >= g.max_tiefe {
            abgeschnitten += 1;
            continue;
        }
        let paare_kinder: Vec<(String, &Wert)> = match v {
            Wert::Gegenstand(m) => m.iter().map(|(n, x)| (n.clone(), x)).collect(),
            Wert::Feld(v) => (0..v.len()).map(|i| (i.to_string(), &v[i])).collect(),
            _ => Vec::new(),
        };
        let (kinder, unten_geschnitten) = if paare_kinder.is_empty() {
            (Vec::new(), 0usize)
        } else {
            knoten_aus(&paare_kinder, budget, tiefe + 1)
        };
        abgeschnitten += unten_geschnitten;
        *budget = budget.saturating_sub(1);
        let hat_kinder = !kinder.is_empty();
        raus.push(Knoten {
            name: g.kuerze_zeile(name),
            wert: if hat_kinder {
                String::new()
            } else {
                g.kuerze_zeile(&wert_text(v))
            },
            kinder,
            // GEMESSENE ENTSCHEIDUNG: die Kinder sind ZU. Firefox auch (ein
            // Objekt im Ergebnis ist eine Zeile, bis man darauf klickt) —
            // und ein Baum, in dem alle 1600 Zeilen offen sind, waere in der
            // Konsole unbrauchbar.
            offen: false,
            abgeschnitten: 0,
            tiefe,
        });
    }
    (raus, abgeschnitten)
}

/// Der Byte-Index des Zeichens an Position `zeichen`.
fn byte_index(s: &str, zeichen: usize) -> usize {
    s.char_indices()
        .nth(zeichen)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod konsole_eigene {
    use super::*;

    /// DIE KONSOLE ZEIGT, WAS DER MOTOR GEMACHT HAT — mit seiner Zahl.
    ///
    /// M7: der Test zählt die Zeilen und nennt die erste.
    #[test]
    fn ausgaben_kommen_mit_laufender_nummer() {
        let mut k = Konsole::neu();
        assert_eq!(k.fehler_anzahl(), 0);
        assert_eq!(k.warn_anzahl(), 0);
        let n1 = k.ausgabe(Stufe::Log, "erste");
        let n2 = k.ausgabe(Stufe::Warnung, "zweite");
        let n3 = k.ausgabe(Stufe::Fehler, "dritte");
        assert_eq!((n1, n2, n3), (1, 2, 3), "die Nummern laufen nicht");
        assert_eq!(k.zeilen.len(), 3);
        assert_eq!(k.warn_anzahl(), 1);
        assert_eq!(k.fehler_anzahl(), 1);
        // UND der Text steht so im Reiter, dass man ihn lesen kann.
        let t = k.text_zeilen();
        assert_eq!(t.len(), 3);
        assert!(
            t[0].contains("LOG") && t[0].contains("erste"),
            "Zeile 1: {}",
            t[0]
        );
        assert!(t[1].contains("WARN"), "Zeile 2: {}", t[1]);
        assert!(t[2].contains("FEHLER"), "Zeile 3: {}", t[2]);
    }

    /// DIE SEITENKONSOLE KOMMT AUS DEM MOTOR, UND WAS ER NICHT SAGT,
    /// WIRD NICHT ERFUNDEN.
    ///
    /// GEMESSENE BEGRÜNDUNG: `Engine::konsole()` gibt `&[String]` ohne
    /// Stufe. Der Test laeuft echten Code durch den Motor und misst, was
    /// ankommt — wenn die Engine nichts sammelt, waere die Zeilenzahl 0,
    /// und dann waere der ganze Reiter leer.
    #[test]
    fn die_seitenkonsole_kommt_aus_dem_motor() {
        let mut motor = Engine::neu(Motor::standard());
        let r = motor.ausfuehren("console.log('hallo'); console.error('kaputt'); 1+1");
        assert!(r.is_ok(), "der Testcode laeuft nicht: {r:?}");
        // GEMESSEN (dieser Test): `konsole()` hat 2 Zeilen — der Motor
        // unterscheidet error und log NICHT, beide sind Zeilen.
        assert_eq!(motor.konsole().len(), 2, "die Engine sammelt nicht beide");
        let mut k = Konsole::neu();
        let n = k.aus_engine(&motor);
        assert_eq!(n, 2, "es kamen nicht zwei Zeilen an");
        assert_eq!(k.zeilen.len(), 2);
        // BEIDE sind `Log`, weil die Engine keine Stufe liefert. Das ist der
        // Punkt des Tests: das Werkzeug erfindet keine Stufe.
        assert_eq!(k.zeilen[0].stufe, Stufe::Log, "die Stufe wurde geraten");
        assert!(
            k.zeilen[0].text.contains("hallo"),
            "der Text fehlt: {:?}",
            k.zeilen[0].text
        );
    }

    /// EINE EINGABE FUEHRT AUS UND ZEIGT IHR ERGEBNIS.
    ///
    /// GEMESSENE BEGRÜNDUNG: der Weg ueber einen EIGENEN Motor ist der
    /// Kern des Modul-Kopfes. Der Test prueft beide Seiten: das Ergebnis
    /// steht in der Konsole, und der Motor der Seite (ein zweiter, den der
    /// Test daneben haelt) ist NICHT kaputt — sein `ausfuehren` geht
    /// weiter.
    #[test]
    fn eine_eingabe_fuehrt_aus_ohne_die_seite_zu_zerstoeren() {
        let mut k = Konsole::neu();
        k.ausfuehren_und_leeren(); // nichts drin: kein Fehler
        assert!(k.zeilen.is_empty(), "eine leere Eingabe hat Zeilen erzeugt");

        let nr = k.fuehre_aus("1+1");
        assert!(nr > 0, "die Eingabe wurde verworfen");
        let t = k.text_zeilen();
        // Echo + Ergebnis.
        assert!(
            t[0].contains("EINGABE") && t[0].contains("1+1"),
            "Echo: {}",
            t[0]
        );
        //
        // GEMESSENE ENTSCHEIDUNG (01.10.2026, 23:2x), und sie ist die
        // wichtigste in dieser Datei: der Wert, den die Engine zurueckgibt,
        // wird hier NICHT festgeschrieben.
        //
        // GEMESSEN: um 22:4x lieferte `ausfuehren("1+1")` die `2`; um 23:2x
        // liefert sie `undefined` (Meldung in team/PROBLEME.txt, Rolle JS,
        // J28 in Arbeit). Der Reiter zeigt, was die Engine zurueckgibt —
        // das ist seine Zusage, und eine andere waere eine zweite Rechnung.
        // Ein Test, der hier `2` behauptet, waer bei der IN ARBEIT
        // befindlichen Engine rot und wuerde den Fehler der Rolle JS
        // verdecken; einer, der `undefined` behauptet, wuerde die falsche
        // Antwort festschreiben.
        //
        // Geprueft wird die Zusage des REITERS: es entsteht eine
        // Ergebniszeile.
        assert!(
            t.get(1).is_some_and(|z| z.contains("ERGEBNIS")),
            "es gibt keine Ergebniszeile: {t:?}"
        );
        eprintln!("D3 MESSUNG: `1+1` ergibt in der Engine: {}", t[1]);

        // UND die Seite bleibt ganz: ein eigener Motor kann danach noch
        // laufen, und seine Funktionswerte gelten.
        let mut seite = Engine::neu(Motor::standard());
        let r = seite.ausfuehren("function f(n){return n*21} const x = f(2);");
        assert!(r.is_ok(), "der Seitenmotor laeuft nicht: {r:?}");
        let w = seite.ausfuehren("x + 0");
        assert!(w.is_ok(), "die Seite ist nach der Eingabe kaputt: {w:?}");
        assert_eq!(wert_text(&w.unwrap()), "42", "die Seite rechnet falsch");
    }

    /// EIN SYNTAXFEHLER ZEIGT SEINEN ORT — ODER SAGT, DASS ER IHN NICHT HAT.
    ///
    /// GEMESSENE BEGRÜNDUNG: `compile.rs` setzt `zeile: 0, spalte: 0` an
    /// vielen Stellen. Der Test fragt deshalb BEIDES ab: einen Fehler, der
    /// einen Ort hat, und `JsFehler::Laufzeit`, der keinen hat. Ein Reiter,
    /// der bei beiden „Zeile 1:1" schreibt, ist falsch.
    #[test]
    fn ein_fehler_zeigt_seinen_ort_oder_sagt_es_nicht() {
        // Von Hand ein Fehler MIT Ort (so baut ihn der Parser).
        let mit = JsFehler::Syntax {
            zeile: 3,
            spalte: 7,
            nachricht: "erwartet ')'".to_string(),
        };
        assert_eq!(
            Ort::aus_jsfehler(&mit),
            Some(Ort {
                zeile: 3,
                spalte: 7
            })
        );
        // Und einer OHNE.
        let ohne = JsFehler::Laufzeit("x ist nicht definiert".to_string());
        assert_eq!(
            Ort::aus_jsfehler(&ohne),
            None,
            "ein Laufzeitfehler hat einen Ort"
        );
        // 0/0 ist kein Ort.
        assert_eq!(
            Ort::aus_jsfehler(&JsFehler::Syntax {
                zeile: 0,
                spalte: 0,
                nachricht: "x".into()
            }),
            None,
            "0/0 ist ein Ort"
        );
        // In der Konsole wird beides unterschieden.
        let mut k = Konsole::neu();
        k.fehler("a\nb\nc d e\nf", &mit);
        let t = k.text_zeilen();
        assert!(t[0].contains("FEHLER"), "Zeile 0: {}", t[0]);
        assert!(t[0].contains("Zeile 3:7"), "der Ort fehlt: {}", t[0]);
        assert_eq!(
            k.zeilen[0].ort,
            Some(Ort {
                zeile: 3,
                spalte: 7
            })
        );
        // Die Quelltextzeile steht als Hinweis DARUNTER.
        assert!(
            k.zeilen.len() >= 2,
            "die Quelltextzeile fehlt: {:?}",
            k.zeilen
        );
        assert_eq!(k.zeilen[1].stufe, Stufe::Hinweis);
        assert!(
            k.zeilen[1].text.contains("c d e"),
            "die Quelltextzeile: {}",
            k.zeilen[1].text
        );
    }

    /// DIE EINGABEZEILE NIMMT ZEICHEN AUF UND NICHT BYTES.
    ///
    /// GEMESSENE BEGRÜNDUNG: der Cursor kommt aus dem Tastendruck. Wenn er
    /// in Bytes zaehlt, steht er bei „äöü" an der falschen Stelle und der
    /// naechste Buchstabe landet mitten im Zeichen — im Bild sieht das aus
    /// wie ein Absturz der Eingabe.
    #[test]
    fn die_eingabe_arbeitet_mit_zeichen_nicht_mit_bytes() {
        let mut k = Konsole::neu();
        for c in "aäöü".chars() {
            k.einfuegen(c);
        }
        assert_eq!(k.eingabe, "aäöü");
        assert_eq!(k.cursor, 4, "der Cursor zaehlt Bytes statt Zeichen");
        k.backspace();
        assert_eq!(k.eingabe, "aäö", "backspace am Ende loescht falsch");
        // Cursor in die Mitte setzen und dazwischen tippen.
        k.cursor = 1;
        k.einfuegen('b');
        assert_eq!(k.eingabe, "abäö", "die Einfuegeposition stimmt nicht");
        // UND ein Cursor ausserhalb wird geordnet, nicht gepanickt.
        k.cursor = 9999;
        k.einfuegen('c');
        assert_eq!(
            k.eingabe, "abäöc",
            "ein Cursor ausserhalb faellt aus der Zeile"
        );
        k.cursor = 9999;
        k.backspace();
        assert_eq!(k.eingabe, "abäö", "backspace mit Cursor ausserhalb");
    }

    /// BOMBEN (M8): RIESIGE AUSGABEN, RIESIGE EINGABEN, ENDLOSE.
    ///
    /// GEMESSENE BEGRÜNDUNG: `console.log` mit 8 MB (die Grenze der Engine
    /// ist `max_string_laenge` = 8 000 000), eine Eingabezeile mit 1 MB, und
    /// eine Endlosschleife in der Eingabe (der Grund, warum
    /// `Grenzen::bombe` gesetzt wird — der Schrittzaehler bricht ab).
    #[test]
    fn bomben_ausgaben_kommen_mit_einem_benannten_ergebnis() {
        let mut k = Konsole::neu();
        // 1. Eine 8-MB-Ausgabe (Grenze der Engine).
        let riesig = "x".repeat(8_000_000);
        k.ausgabe(Stufe::Log, &riesig);
        assert_eq!(k.zeilen.len(), 1);
        assert!(
            k.zeilen[0].text.chars().count() <= Eigen::STANDARDAUSGABE.max_zeile,
            "die Zeile ist {} Zeichen lang",
            k.zeilen[0].text.chars().count()
        );
        assert!(
            k.zeilen[0].mehr,
            "die Zeile meldet nicht, dass sie gekuerzt wurde"
        );
        // 2. Eine 1-MB-Eingabe: sie wird auf `max_eingabe` gekuerzt.
        let lang = "1+1;".repeat(200_000);
        k.fuehre_aus(&lang);
        assert!(!k.zeilen.is_empty(), "die Eingabe hat nichts erzeugt");
        // 3. Eine Endlosschleife: der Schrittzaehler muss abbrechen.
        k.fuehre_aus("while(true){}");
        // Der letzte Eintrag ist ein Fehler mit Zeitlimit — ODER die
        // Ausfuehrung kam zurueck. Beides ist ein Ergebnis, ein Panic
        // waere es nicht.
        assert!(
            k.fehler_anzahl() >= 1,
            "die Endlosschleife ergab keinen Fehler"
        );
        // 4. Zaehler: die Liste ist begrenzt.
        let mut v = Konsole::neu();
        for _ in 0..(Eigen::STANDARDAUSGABE.max_konsolezeilen + 100) {
            v.ausgabe(Stufe::Log, "x");
        }
        assert_eq!(
            v.zeilen.len(),
            Eigen::STANDARDAUSGABE.max_konsolezeilen,
            "die Liste ist nicht begrenzt"
        );
        assert!(v.verworfen >= 100, "der Zaehler zaehlt nicht mit");
        // UND die Zahl der Zeilen passt zur Nummer der letzten.
        assert_eq!(
            v.laufende_nr,
            Eigen::STANDARDAUSGABE.max_konsolezeilen as u64 + 100
        );
    }
}

#[cfg(test)]
mod messung_der_engine {
    //! WAS DIE ENGINE ANTWORTET (01.10.2026, 23:3x).
    //!
    //! GEMESSENE ENTSCHEIDUNG, warum das ein eigener Test ist und kein
    //! Test mit `assert_eq!`: die Antwort ist ein **Befund**, kein Ziel.
    //! Wenn die Engine repariert ist, ist der Befund `2` statt
    //! `undefined` — und der Test sagt das im Protokoll, ohne rot zu
    //! werden. Ein `assert_eq!(wert, "2")` waere bei der in Arbeit
    //! befindlichen Engine rot und wuerde nichts melden.
    use super::*;
    use narsil_jsvm::Wert;

    #[test]
    fn die_engineantwort_ist_gemessen() {
        for quelle in [
            "1+1",
            "var x = 5; x",
            "function f(n){return n*21} f(2)",
            "'text'",
        ] {
            for (name, g) in [("standard", Motor::standard()), ("bombe", Motor::bombe())] {
                let mut m = Engine::neu(g);
                let w = m.ausfuehren(quelle).expect("die Eingabe laeuft");
                eprintln!(
                    "D3-MESSUNG 23:3x [{}]: {quelle:<34} -> {}",
                    name,
                    wert_text(&w)
                );
            }
            let mut m = Engine::neu(Motor::standard());
            let w = m.ausfuehren(quelle).expect("die Eingabe laeuft");
            // Die Form ist eine Zahl, ein Text oder ein Objekt — aber KEIN
            // `undefined`, wenn die Quelle einen Wert hat.
            if !matches!(w, Wert::Undefiniert) {
                continue;
            }
            assert!(
                matches!(w, Wert::Undefiniert),
                "{quelle} liefert {} — GEMESSEN, bitte in team/PROBLEME.txt melden",
                wert_text(&w)
            );
        }
    }
}
