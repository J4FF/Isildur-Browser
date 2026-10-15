// BEGRUENDUNG fuer `dead_code`: die Aufrufer dieser Datei lagen in `vm.rs`,
// das am 03.10. verloren ging und nur als 30.09.-Stand wieder da ist
// (`team/NOTIZ_VM_VERLOREN.txt`). Der Code hier ist nicht tot, sondern
// **wartet** — er kommt wieder, sobald der VM-Teil nachgezogen ist.
#![allow(dead_code)]
//! J28 — die **Hakenlogik**: wann angehalten wird, und was von aussen
//! sichtbar ist.
//!
//! Die Regeln stehen hier, weil sie an **einer** Stelle sein muessen:
//! die vier Schrittarten unterscheiden sich nur darin, **wann** sie an
//!halten. An mehreren Stellen gäbe es „Schritt über" nicht.

use crate::debugger::{Haken, HakenZustand, Haltepunkt, NamWert, RahmenInfo, SchrittArt};
use crate::heap::GVal;
use crate::interp::Engine;
use crate::op::Op;
use crate::vm::LaufErgebnis;

impl Engine {
    /// J28: laeuft ein Haken? (Der Zweig, den die Schleife nimmt.)
    pub(crate) fn haken_aktiv(&self) -> bool {
        self.haken.is_some()
    }

    /// J28: die Zeile, die als Naechstes kommt.
    ///
    /// Steht der naechste Befehl (`code[pc]`) auf `Op::Zeile`, ist das die
    /// Zeile der kommenden Anweisung. Sonst gilt die zuletzt gesetzte
    /// Zeile des Rahmens (dann laeuft gerade eine Anweisung, die schon
    /// angefueangen hat).
    pub(crate) fn zeile_gelaufen(&self) -> u32 {
        self.rahmen.last().map(|r| r.zeile).unwrap_or(0)
    }

    pub(crate) fn zeile_kommt(&self) -> u32 {
        let Some(r) = self.rahmen.last() else {
            return 0;
        };
        if let Some(Op::Zeile(z)) = r.code.get(r.pc) {
            return *z;
        }
        // J28: die Startzeile des Rahmens (die Zeile von `function f`).
        //
        // GEMESSEN/VERWORFEN: eine Fassung zeigte hier die **erste
        // Rumpfzeile** an, damit der erste Halt in einer Funktion auf der
        // ersten Anweisung stuende. Sie hielt aber **vor** dem Prolog —
        // also vor `Param(0)` — und die Locals zeigten dann weder `x` noch
        // `y` (gemessen: `s=undefined` ohne Parameter). Ein Halt vor dem
        // Prolog ist keine bessere Anzeige, sondern eine schlechtere: es
        // fehlen genau die Variablen, die man sehen will. Die Startzeile
        // ist stattdessen **eine** Zeile mit einer echten Bedeutung.
        r.zeile
    }

    /// J28: darf der Lauf ueberhaupt anhalten? (`Haken::haelt`)
    pub(crate) fn haken_haelt(&self) -> bool {
        self.haken.as_ref().map(|h| h.haelt).unwrap_or(false)
    }

    /// J28: die Erlaubnis wieder entziehen (nach einem Halt im
    /// `ausfuehren_mit_haken`).
    pub(crate) fn haken_ohne_halt(&mut self) {
        if let Some(h) = self.haken.as_mut() {
            h.haelt = false;
        }
    }

    /// J28: **muss der Haken hier anhalten?** — und wo er dann steht.
    ///
    /// GEMESSEN, warum die Reihenfolge so ist und nicht anders:
    /// * `debugger;` zuerst — es ist die **ausdrueckliche** Anweisung des
    ///   Programms, und ein Haltepunkt auf derselben Zeile darf sie nicht
    ///   ueberstimmen (bis der Entwickler weiterdrueckt).
    /// * dann die **Sperre** (`Haken::verbot`): auf der Position, auf der
    ///   gerade angehalten wurde, wird **nicht** wieder angehalten —
    ///   sonst haelt `weiter()` sofort wieder am selben Haltepunkt
    ///   (gemessen in MESS 1: sechs `weiter`, alle auf `(0, 7)`).
    /// * dann der **Auftrag** (Rein/Ueber/Raus/Weiter),
    /// * dann ein **Haltepunkt** auf der Zeile.
    ///
    /// GEMESSEN, warum bei `Ueber` und `Raus` die **Tiefe** zaehlt und
    /// nicht der Name der Funktion: zwei Aufrufe derselben Funktion
    /// (Rekursion) haben denselben Namen, und „Schritt ueber" muss in
    /// der **tieferen** nicht anhalten.
    ///
    /// GEMESSEN, warum die Zeile die **kommende** ist: `Rahmen::zeile`
    /// wird von `Op::Zeile` gesetzt, also nach dem Befehl. Beim Laden
    /// eines Skripts mit einem Haltepunkt auf Zeile 7 endete der Lauf
    /// ohne zu halten, weil `Rahmen::zeile` noch die Zeile 3 war. Ein
    /// Haltepunkt gilt fuer die Zeile, deren Befehle **noch nicht**
    /// gelaufen sind.
    pub(crate) fn haken_halt(&mut self) -> bool {
        let tiefe = self.rahmen.len();
        let zeile = self.zeile_kommt();
        // GEMESSEN, warum die gelaufene Zeile **vor** der Ausleihe des
        // Hakens geholt wird: `self.haken.as_mut()` leiht `self` fuer die
        // Dauer des Feldes aus, und `self.zeile_gelaufen()` braucht `&self`
        // (E0502).
        let gel = self.zeile_gelaufen();
        let datei = self.skripte.datei_von(zeile);
        let Some(h) = self.haken.as_mut() else {
            return false;
        };
        // Ohne Erlaubnis haelt **nichts** — `haltepunkte_setzen` allein
        // darf einen normalen Seitenaufbau nicht anhalten (gemessen: sonst
        // bricht jeder Seitenaufbau am ersten Haltepunkt mit „Hauptlauf
        // hat angehalten (intern)" ab).
        if !h.haelt {
            return false;
        }
        // Die Sperre zuerst: sie gilt fuer **jeden** Weg, sonst wuerde
        // `debugger;` auf einer gesperrten Position nie greifen.
        if h.verbot == Some((datei, zeile)) {
            return false;
        }
        // `debugger;` — der Befehl hat es gesetzt.
        if std::mem::take(&mut h.debugger_anweisung) {
            h.auftrag = Some(SchrittArt::Weiter);
            h.erledigt = false;
            h.tiefe = tiefe;
            h.datei = datei;
            // GEMESSEN, warum die **gelaufene** Zeile: `debugger;` steht
            // auf seiner eigenen Zeile, und die kommt erst danach. Mit der
            // kommenden Zeile zeigte das Panel die Folgeleile (2 -> 3).
            h.debugger_gerade = true;
            h.steht_in = Some((datei, gel));
            h.verbot = h.steht_in;
            return true;
        }
        if h.erledigt {
            h.erledigt = false;
            h.auftrag = None;
        }
        // Ein Haken, der steht, merkt sich die Position — **eine** Quelle
        // fuer `haken_stand` und `aufrufstapel` (sonst zeigen die beiden
        // verschiedene Zeilen, je nachdem, welchen der beiden Formeln sie
        // benutzt).
        let haltet = match h.auftrag {
            None => h.stehplatz(datei, zeile),
            Some(SchrittArt::Weiter) => h.stehplatz(datei, zeile),
            // „Rein": die naechste Zeile, in welcher Tiefe auch immer.
            Some(SchrittArt::Rein) => zeile != h.zeile,
            // „Ueber": die naechste Zeile **derselben** Funktion.
            Some(SchrittArt::Ueber) => tiefe != h.tiefe || zeile != h.zeile,
            // „Raus": bis die aufrufende Funktion wieder an der Reihe ist.
            Some(SchrittArt::Raus) => tiefe < h.tiefe,
        };
        if !haltet {
            return false;
        }
        // `debugger;` nicht: dort ist die Zeile die gelaufene.
        let gemeldet = if h.debugger_gerade {
            (datei, gel)
        } else {
            (datei, zeile)
        };
        h.debugger_gerade = false;
        if h.auftrag.is_some() {
            h.auftrag = None;
            h.erledigt = false;
            h.verbot = None;
        }
        h.steht_in = Some(gemeldet);
        h.verbot = h.steht_in;
        true
    }

    /// J28: den Auftrag eintragen, damit `haken_halt` ihn auswertet.
    pub(crate) fn haken_auftrag(&mut self, art: SchrittArt) {
        let tiefe = self.rahmen.len();
        // J28: **dieselbe** Quelle wie `haken_halt` — gemessen, warum das
        // zwingend ist: mit `Rahmen::zeile` (die gelaufene) verglich der
        // Auftrag gegen eine andere Zahl als der Vergleich im Haken, und
        // „Schritt rein" hielt sofort auf der Stelle, statt hineinzugehen
        // (gemessen: `nach Rein: steht_in=Some((0, 7))`).
        let zeile = self.zeile_kommt();
        let datei = self.skripte.datei_von(zeile);
        if self.haken.is_none() {
            self.haken = Some(Haken::leer());
        }
        if let Some(h) = self.haken.as_mut() {
            h.auftrag = Some(art);
            h.erledigt = false;
            h.tiefe = tiefe;
            h.zeile = zeile;
            h.datei = datei;
            // Die aktuelle Position wird gesperrt: ein Schritt muss erst
            // **etwas** tun, bevor wieder angehalten wird.
            h.verbot = Some((datei, zeile));
            h.steht_in = Some((datei, zeile));
        }
    }

    // =================================================================
    // DIE OEFFENTLICHE FLAECHE (der Wunsch aus WUNSCH_DEVTOOLS.txt)
    // =================================================================

    /// J28: Haltepunkte setzen (ersetzen alle).
    ///
    /// Die Liste wird **einmal** sortiert und entdoppelt
    /// (`Haken::haltepunkte_setzen`), damit die Haltepruefung je Anweisung
    /// ein `O(1)`-Zweiersuchvergleich ist.
    pub fn haltepunkte_setzen(&mut self, h: &[Haltepunkt]) -> Result<(), String> {
        if self.haken.is_none() {
            self.haken = Some(Haken::leer());
        }
        // GEMESSEN, warum die Zeilen hier **nicht** gegen den Quelltext
        // geprueft werden: der Motor hat die Zeilennummern erst beim
        // Uebersetzen, und der Host darf einen Haltepunkt auf eine Zeile
        // setzen, die im **eingebetteten** Skript (HTML-Attribut) steht —
        // die sieht der Compiler als Datei 0. Eine Pruefung wuerde
        // Haltepunkte verwerfen, die richtig sind.
        if let Some(k) = self.haken.as_mut() {
            k.haltepunkte_setzen(h);
        }
        Ok(())
    }

    /// J28: die Zeilen, an denen die Skripte der Seite anfangen.
    ///
    /// GEMESSEN, warum das der Host meldet: `narsil` legt alle Skripte in
    /// **einen** Quelltext (`narsil/src/jsvm.rs:437`), und ohne diese
    /// Tabelle ist „Datei 1, Zeile 3" nicht von „Datei 0, Zeile 3" zu
    /// unterscheiden. Ein Aufrufer, der nur einen Quelltext hat, ruft
    /// nichts — dann ist alles Datei 0.
    pub fn skriptgrenzen_setzen(&mut self, beginne: &[u32]) {
        self.skripte = crate::debugger::SkriptTabelle::neu(beginne);
    }

    /// J28: **ein Lauf, der beim ersten Haltepunkt stehenbleibt.**
    ///
    /// Das ist der Weg, den eine Oberflaeche braucht: Seite laden und
    /// anhalten, in einem Panel Locals lesen, dann weiter.
    ///
    /// GEMESSEN, warum das **nicht** `ausfuehren` mit gesetzten
    /// Haltepunkten ist: `ausfuehren` laeuft bis zum Ende und raeumt die
    /// Rahmen ab — danach gibt es keine Locals mehr zu lesen. Und warum
    /// es **nicht** `weiter()` vor dem Lauf ist: es gibt dann nichts, in
    /// dem man stehen bleiben koennte.
    pub fn ausfuehren_mit_haken(
        &mut self,
        quelle: &str,
        haltepunkte: &[Haltepunkt],
    ) -> Result<crate::debugger::HakenEnde, crate::JsFehler> {
        self.haltepunkte_setzen(haltepunkte)
            .map_err(crate::JsFehler::Laufzeit)?;
        if self.haken.is_none() {
            self.haken = Some(Haken::leer());
        }
        if let Some(h) = self.haken.as_mut() {
            h.haelt = true;
            h.auftrag = Some(SchrittArt::Weiter);
        }
        self.start = std::time::Instant::now();
        self.lauf_ende = 0;
        let out = self.programm_ausfuehren(quelle);
        match out {
            // Der Wert -1.0 ist die **Marke** aus `vm.rs` (Angehalten mit
            // Haken). Sie ist unmoeglich, weil der Hauptlauf nie eine Zahl
            // -1 zurueckgibt: `HakenEnde` unterscheidet sonst nicht.
            Ok(GVal::Zahl(-1.0)) => Ok(crate::debugger::HakenEnde::Steht),
            Ok(v) => {
                self.haken_ohne_halt();
                Ok(crate::debugger::HakenEnde::Fertig(self.export_wert(v, 0)))
            }
            Err(e) => {
                self.haken_ohne_halt();
                Err(e)
            }
        }
    }

    /// J28: weiter bis zum naechsten Haltepunkt.
    ///
    /// Antwortet `Err`, wenn gerade **nichts** laeuft: dann gibt es kein
    /// „Weiter", und ein `Ok` waerde eine erfundene Zusage.
    pub fn weiter(&mut self) -> Result<(), String> {
        if self.rahmen.is_empty() {
            return Err("weiter: es laeuft kein Skript".to_string());
        }
        self.haken_auftrag(SchrittArt::Weiter);
        self.laufen_bis_haken()
    }

    /// J28: einen Schritt (rein/ueber/raus/weiter).
    ///
    /// GEMESSEN, warum der Name `haken_schritt` und nicht `schritt`:
    /// `Engine::schritt()` ist die **interne** Schrittzählung fuer die
    /// Grenzen (`interp.rs:1059`), und sie wird an jeder Aufrufstelle
    /// benutzt. Ein zweites `schritt` (dieses, mit Argument) waere eine
    /// zweite Methode mit demselben Namen und anderer Bedeutung — genau
    /// die Sorte Zweiheit, an der jemand die falsche aufruft. Der
    /// Wunsch nannte `schritt(art)`; der Aufrufer der Oberflaeche
    /// braucht ohnehin `schritt_beauftragen` (das steht in
    /// `d5_quellen.rs:297` schon).
    pub fn haken_schritt(&mut self, art: SchrittArt) -> Result<(), String> {
        if self.rahmen.is_empty() {
            return Err("schritt: es laeuft kein Skript".to_string());
        }
        self.haken_auftrag(art);
        self.laufen_bis_haken()
    }

    /// J28: die Erlaubnis zum Anhalten geben (ein Schrittauftrag gibt
    /// sie **nur fuer seinen Lauf**).
    /// J28: wo steht der Haken?
    ///
    /// GEMESSEN, warum hier `zeile_kommt()` und **nicht** `Rahmen::zeile`:
    /// `Rahmen::zeile` ist die zuletzt **gelaufene** Zeile, und der Haken
    /// haelt, **bevor** die Zeile laeuft. Mit der falschen Quelle zeigte
    /// das Panel eine Zeile zu weit hinten — und beide Stellen hätten
    /// je fuer sich „recht" gehabt. Es gibt jetzt **eine** Quelle
    /// (`zeile_kommt`) fuer Halten und Anzeigen.
    pub fn haken_stand(&self) -> HakenZustand {
        let laeuft = !self.rahmen.is_empty();
        if !laeuft {
            return HakenZustand {
                laeuft,
                steht_in: None,
            };
        }
        // EINE Quelle: der Haken merkt sich, wo er steht.
        if let Some(h) = self.haken.as_ref()
            && let Some(paar) = h.steht_in
        {
            return HakenZustand {
                laeuft,
                steht_in: Some(paar),
            };
        }
        let z = self.zeile_kommt();
        HakenZustand {
            laeuft,
            steht_in: Some((self.skripte.datei_von(z), z)),
        }
    }

    /// J28: der Aufrufstapel, oben zuerst.
    pub fn aufrufstapel(&self) -> Vec<RahmenInfo> {
        let mut raus: Vec<RahmenInfo> = Vec::with_capacity(self.rahmen.len());
        for (i, r) in self.rahmen.iter().enumerate().rev() {
            let name = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(r.fun))
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "<wert>".to_string());
            // Die Zeile des **obersten** Rahmens ist die, auf der der
            // Haken steht (`Haken::steht_in` — dieselbe Quelle wie
            // `haken_stand`); die der anderen sind die gelaufenen — dort
            // steht der Aufrufer ja schon.
            // GEMESSEN, warum `r.zeile` und nicht `i == 0`: die Liste
            // laeuft von **oben** nach unten (`rev()`), der obere Rahmen
            // ist also der **erste** Eintrag — `i` zaehlt aber die
            // urspruengliche Reihenfolge (0 = Hauptlauf). Mit `i == 0`
            // bekam der Hauptlauf die Zeile des Hakens und `plus` seine
            // Startzeile (gemessen: Stapel `plus[0:0] <- <main>[0:7]`,
            // obwohl der Haken in `plus` auf Zeile 4 stand).
            let z = r.zeile;
            raus.push(crate::debugger::rahmen_info(
                r,
                &name,
                self.skripte.datei_von(z),
                i as u32,
            ));
        }
        raus
    }

    /// Captured before exception unwinding removes the running frames.
    pub fn letzte_fehlerstelle(&self) -> Option<&str> {
        self.letzte_fehlerstelle.as_deref()
    }

    /// J28: die **Locals** — des obersten Rahmens zuerst, dann die der
    /// aeusseren.
    ///
    /// GEMESSEN, warum die Namen aus dem **Namenspool der Funktion**
    /// kommen und nicht aus den Bindungen: eine Umgebung speichert zu
    /// jeder Bindung nur den **Hash** (J3b, `heap.rs:261` — der Name
    /// waere eine Allokation je Zugriff), und `deklariere_h` legt die
    /// Namen absichtlich **leer** an (`interp.rs:1708`). Der Pool
    /// `Fn::namen` mit `Fn::namen_hash` ist genau die Liste, die der
    /// Motor beim Zugriff benutzt (`LeseVar`, `vm.rs:704`) — dieselbe
    /// Quelle, damit die Anzeige nicht eine zweite ist.
    pub fn locals(&mut self) -> Vec<NamWert> {
        let mut raus: Vec<NamWert> = Vec::new();
        // M3: die Kette ist begrenzt (64 Ebenen sind mehr, als
        // `max_dom_tiefe` erlaubt) und die Namenspools sind es auch
        // (ein Skript mit 10.000 Namen soll das Panel nicht sprengen).
        const MAX_EBENEN: usize = 64;
        const MAX_NAMEN: usize = 2000;
        // GEMESSEN, warum Rahmen und Namenspool **vorher** kopiert
        // werden: `lese_h`/`zu_text` brauchen `&mut self` (E0502) — die
        // Liste ist ein paar hundert Byte und wird nur gelesen.
        let paare: Vec<(crate::heap::HeapRef, Vec<String>, Vec<u64>)> = self
            .rahmen
            .iter()
            .rev()
            .take(MAX_EBENEN)
            .filter_map(|r| {
                let f = self
                    .programm
                    .as_ref()
                    .and_then(|p| p.funktionen.get(r.fun))?;
                Some((r.umg, f.namen.as_ref().clone(), f.namen_hash.clone()))
            })
            .collect();
        for (umg, namen, hashes) in paare {
            for (k, name) in namen.iter().enumerate().take(MAX_NAMEN) {
                let Some(h) = hashes.get(k).copied() else {
                    continue;
                };
                let name = name.clone();
                // GEMESSEN, warum `lese_h` mit **leerem** Namen: das ist
                // der heisse Weg des Motors (kein Textvergleich), und er
                // vergleicht nur den Hash — dieselbe Bindung, die ein
                // `LeseVar` im Skript finden wuerde.
                // Kein Wert in dieser Umgebung: die Variable gehoert
                // einer aeusseren oder gar nicht — beides ist keine Zeile
                // fuer das Panel, also wird sie nicht gezeigt.
                if let Ok(v) = self.lese_h(umg, h, "") {
                    let wert = self
                        .zu_text(v)
                        .unwrap_or_else(|_| "<kein Text>".to_string());
                    raus.push(NamWert { name, wert });
                }
            }
        }
        raus
    }

    /// J28: der Operandenstapel **und** die Wert-Slots des obersten
    /// Rahmens.
    ///
    /// GEMESSEN, warum beides noetig ist: `Rahmen::stapel` ist am Ende
    /// **leer** (gemessen: `[]` am Haltepunkt auf Zeile 4 und auf 5).
    /// Der Grund ist der J20-Wert-Slot-Mechanismus: der Compiler legt
    /// jedes Ergebnis einer Anweisung in einen **Temp** (`Rahmen::temps`),
    /// nicht auf den Operandenstapel. Ein Panel, das nur den
    /// Operandenstapel zeigte, waere also **immer leer** — gemessen so,
    /// bevor diese Zeile dazukam.
    ///
    /// Die Reihenfolge ist die Lesereihenfolge des Motors: erst der
    /// Operandenstapel (die Zwischenwerte eines Ausdrucks), dann die
    /// Slots (die Werte der Anweisungen). Die Namen stehen in Klammern,
    /// damit der slot0 nicht wie eine Variable aussieht.
    pub fn wertstapel(&mut self) -> Vec<NamWert> {
        let Some(r) = self.rahmen.last() else {
            return Vec::new();
        };
        // M3: die Anzeige darf nicht unbegrenzt gross werden — 1000
        // Eintraege sind mehr, als ein Panel zeigen kann.
        const GRENZE: usize = 1000;
        let basis = r.basis.min(r.stapel.len());
        // GEMESSEN (E0502): die Laengen **vor** der Schleife lesen — ab
        // dort leiht `zu_text` `self` mutabel aus.
        let stapel_anzahl = r.stapel.len();
        let temps_anzahl = r.temps.len();
        let args_anzahl = r.args.len();
        // GEMESSEN, warum die Werte **vor** der Textwandlung kopiert
        // werden: `zu_text` braucht `&mut self` (E0502, wie oben).
        let werte: Vec<(String, GVal)> = r
            .stapel
            .iter()
            .skip(basis)
            .take(GRENZE)
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), *v))
            .chain(
                r.temps
                    .iter()
                    .take(GRENZE)
                    .enumerate()
                    .map(|(i, v)| (format!("(slot{i})"), *v)),
            )
            .collect();
        let _ = (stapel_anzahl, temps_anzahl, args_anzahl);
        let mut raus: Vec<NamWert> = Vec::with_capacity(werte.len());
        for (name, v) in werte {
            let t = self
                .zu_text(v)
                .unwrap_or_else(|_| "<kein Text>".to_string());
            raus.push(NamWert { name, wert: t });
        }
        raus
    }

    /// J28: **bis zum naechsten Haken laufen** — der Weg, den
    /// `weiter()`/`schritt()` gehen.
    ///
    /// GEMESSEN, warum das ein eigener Lauf ist und nicht ein Flag am
    /// `Engine`: der Lauf muss **bis zum Haken** laufen und dann
    /// **stehenbleiben** — mit dem Zustand (Rahmen, Stapel, Umgebung)
    /// vollstaendig erhalten, damit `locals()` danach etwas zeigt. Ein
    /// Flag wuerde den Lauf anhalten, aber nicht speichern, wo er war;
    /// und ein Wegwerfen des Laufs wuerde die Locals wieder zerstoeren.
    fn laufen_bis_haken(&mut self) -> Result<(), String> {
        // Der Haken muss **an** sein, sonst laeuft das Skript bis zum
        // Ende und `haken_halt` fasst nie zu.
        if self.haken.is_none() {
            self.haken = Some(Haken::leer());
        }
        if self.rahmen.is_empty() {
            return Err("laufen_bis_haken: es laeuft kein Skript".to_string());
        }
        // M3/M5: ein Haken darf nicht **endlos** laufen. Ohne diese Grenze
        // waere `weiter()` auf `while(true){}` eine Endlosschleife in der
        // Engine — der Aufrufer bekommt nie wieder eine Antwort.
        // GEMESSEN, warum `ende = 0` und nicht `rahmen.len() - 1`: der
        // Hakenlauf darf **zurueck** auf eine kleinere Tiefe („Schritt
        // raus" aus einer Funktion heraus heisst: beim Aufrufer anhalten).
        // Mit `len - 1` brach der Lauf genau dann ab, wenn er das Ziel
        // erreichte — gemessen: „Schritt raus" aus `plus` meldete
        // „das Skript ist zu Ende", statt auf Zeile 7 des Hauptlaufs zu
        // halten.
        let ende = 0;
        let r = self.schleife_mit_haken(ende);
        match r {
            // GEMESSEN, warum „Ende" von „Halt" getrennt werden muss:
            // die Schleife liefert in **beiden** Faellen `Angehalten` —
            // einmal, weil der Haken gegriffen hat, einmal, weil alle
            // Rahmen beendet sind. Ohne diese Unterscheidung meldet
            // `weiter()` nach dem letzten Haltepunkt Erfolg, und die
            // Oberflaeche glaubt, es laeuft noch.
            LaufErgebnis::Angehalten => {
                if self.rahmen.len() <= ende {
                    Err("schritt: das Skript ist zu Ende (kein Haltepunkt erreicht)".to_string())
                } else {
                    Ok(())
                }
            }
            LaufErgebnis::Fertig(_) | LaufErgebnis::Wurf(_) => {
                // Das Skript ist zu Ende: die Haltekette steht noch, aber
                // es gibt nichts mehr, wohin man zurueck springen koennte.
                Err("schritt: das Skript ist zu Ende (kein Haltepunkt erreicht)".to_string())
            }
            LaufErgebnis::Fehler(e) => Err(e.to_string()),
        }
    }
}
