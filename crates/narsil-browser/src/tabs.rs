//! Tabs: Identitaet, Ladezustand, und die zwei Regeln, die entschieden
//! werden mussten.
//!
//! ERSTAELLT am 29.09.2026, TASK_ZUSTAND Aufgabe 1.
//!
//! DIE ZWEI ENTSCHEIDUNGEN, mit Begruendung. Beide sind in
//! `tests/tabs.rs` mit je einem Test festgehalten, und die Tests nennen
//! die Regel im Namen, damit man sie beim Lesen findet.
//!
//! **E1 — welcher Tab wird nach dem Schliessen aktiv?**
//! **Der Nachbar an der Stelle, an der geschlossen wurde: erst der
//! rechts, sonst der links.** Begruendung: die Oberflaeche (narsil-chrome,
//! fremder Agent) zeigt die Tabs in einer Leiste. Wird einer in der Mitte
//! geschlossen, rutschen die folgenden eine Stelle nach links. Faellt die
//! Oberflaeche fuer den aktiven auf die *naechste* Position, muss auch der
//! Zustand auf die naechste Position zeigen, sonst zeigt die Leiste "Tab 2
//! aktiv", während im Zustand Tab 3 aktiv ist. Das ist kein Detail: es
//! waere der Bug, der nach einer Woche niemand mehr zuordnen kann.
//! Es heisst auch: der zuletzt benutzte Nachbar ist *nicht* die Regel —
//! das waere eine zweite Wahrheit, und die wird bei "welcher war das?"
//! immer zur Vermutung.
//!
//! **E2 — was passiert beim Schliessen des LETZTEN Tabs?**
//! **Es entsteht sofort ein neuer, leerer Tab, und genau der wird aktiv.
//! Der Browser haelt immer mindestens einen Tab.** Begruendung: `Browser`
//! ist ein WERT, kein Fenster. Jeder Zugriff auf diesem Typen — `adresse()`,
//! `verlauf_laenge()`, `seite()`, `kann_zurueck()` — gibt bei null Tabs
//! einen leeren String, `0` oder `None` zurueck. Ein Browser ohne Tab hat
//! also keinen Fehler, sondern ein Loch: die Bedienflaeche kann nichts
//! anzeigen, und das weiss niemand. Ob das *Fenster* dann zugeht, ist
//! eine Entscheidung ueber das Fenster (narsil-window, fremder Agent) und
//! gehoert nicht hierher. Hier ist die Antwort: der Zustand ist nie leer.
//!
//! KEINE Geometrie, KEIN Zeichnen, KEINE Koordinaten. Das ist
//! narsil-chrome und gehoert einem anderen Agenten (ADR-012).

use crate::{Browser, Page, Tab};

/// STABILE Identitaet eines Tabs.
///
/// Nicht die Position: wird der erste von drei Tabs geschlossen, rutscht
/// der dritte auf Position 1, seine `TabId` bleibt 3. Wer einen Tab
/// wiederfinden will, braucht die `TabId` und nicht den Platz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

impl std::fmt::Display for TabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tab {}", self.0)
    }
}

/// Was in einem Tab gerade passiert.
///
/// `Leer` ist der Anfangszustand und der Zustand nach dem Schliessen des
/// letzten Tabs. `Fehler` traegt den Text, damit die Oberflaeche etwas
/// anzeigen kann, das mehr sagt als "Fehler".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ladezustand {
    /// Nichts geladen. Auch: neu geoeffnet.
    Leer,
    /// Angefragt, Antwort steht aus.
    Laedt,
    /// Geladen. Der Titel, falls es einen gab, steht am Verlaufseintrag.
    Fertig,
    /// Fehlgeschlagen, mit Text. Der Text kommt vom Aufrufer — diese
    /// Crate fuehrt keine Netzverbindung aus.
    Fehler(String),
}

impl Ladezustand {
    /// Ob gerade geladen wird. Die Oberflaeche braucht das fuer einen
    /// Ladeanzeiger, ohne die Variante zu zerlegen.
    pub fn ist_geladen_wird(&self) -> bool {
        matches!(self, Ladezustand::Laedt)
    }

    /// Ob ein Fehlertext da ist.
    pub fn fehlertext(&self) -> Option<&str> {
        match self {
            Ladezustand::Fehler(t) => Some(t.as_str()),
            _ => None,
        }
    }
}

impl Browser {
    // ------------------------------------------------------------------
    // Oeffnen und schliessen
    // ------------------------------------------------------------------

    /// Einen neuen Tab oeffnen. Er wird der AKTIVE und bekommt als Inhalt
    /// die Startseite — also eine leere Seite, denn das Laden gehoert
    /// nicht in diese Crate.
    ///
    /// Die neue `TabId` kommt zurueck, damit der Aufrufer sie behalten
    /// kann, wenn er sie braucht.
    pub fn tab_oeffnen(&mut self) -> TabId {
        self.naechste_tab_id += 1;
        let id = TabId(self.naechste_tab_id);
        self.tabs.push(Tab::neu_mit_id(id));
        // Der neue Tab ist der aktive. Wer einen Tab oeffnet, will ihn sehen.
        self.aktiv = self.tabs.len() - 1;
        id
    }

    /// Den Tab mit dieser `TabId` schliessen. `false`, wenn es den Tab
    /// nicht gibt — dann passiert NICHTS.
    ///
    /// ## E1: welcher Tab wird danach aktiv?
    ///
    /// Der Nachbar an der geschlossenen Stelle: erst der rechts, sonst der
    /// links. Siehe Modul-Doku fuer die Begruendung.
    ///
    /// ## E2: was passiert beim letzten Tab?
    ///
    /// Ein neuer leerer Tab entsteht und wird aktiv. Der Browser hat danach
    /// nie null Tabs. Siehe Modul-Doku.
    pub fn tab_schliessen(&mut self, id: TabId) -> bool {
        let position = match self.tabs.iter().position(|t| t.id == id) {
            Some(p) => p,
            None => return false,
        };

        // E2: der letzte Tab wird ersetzt, nicht zu null Tabs gefuehrt.
        if self.tabs.len() == 1 {
            self.naechste_tab_id += 1;
            let neu = Tab::neu_mit_id(TabId(self.naechste_tab_id));
            self.tabs.clear();
            self.tabs.push(neu);
            self.aktiv = 0;
            return true;
        }

        self.tabs.remove(position);

        // E1: rechts, sonst links. Beide Randfaelle abgedeckt:
        //   - der LETZTE Tab wird geschlossen -> position war len-1,
        //     rechts gibt es keinen, also der neue letzte links davon.
        //   - ein mittlerer Tab wird geschlossen -> rechts existiert.
        self.aktiv = if position < self.tabs.len() {
            position // der rechts ist jetzt an dieser Stelle
        } else {
            position - 1 // der links ist jetzt der letzte
        };
        true
    }

    /// Auf den Tab mit dieser `TabId` umschalten. `false`, wenn es ihn
    /// nicht gibt.
    ///
    /// Regel: der Verlauf des aktiven Tabs wird von einem Umschalten
    /// NICHTS angefasst. Das ist kein Nebensatz, das ist getestet
    /// (`umschalten_aendert_keinen_verlauf`).
    pub fn tab_waehlen(&mut self, id: TabId) -> bool {
        match self.tabs.iter().position(|t| t.id == id) {
            Some(p) => {
                self.aktiv = p;
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------------
    // Fragen. Alle ohne Index-Zugriff, der ins Leere greifen koennte.
    // ------------------------------------------------------------------

    /// Die Identitaet des aktiven Tabs. `None`, wenn es keinen gibt —
    /// was nach den Regeln nie vorkommt, aber der Typ sagt es.
    pub fn aktive_tab_id(&self) -> Option<TabId> {
        self.tabs.get(self.aktiv).map(|t| t.id)
    }

    /// Die Identitaet des Tabs an dieser Position. `None` wenn die
    /// Position nicht existiert.
    pub fn tab_id_an_position(&self, position: usize) -> Option<TabId> {
        self.tabs.get(position).map(|t| t.id)
    }

    /// Die Position der `TabId` — fuer die Leiste, die von links zaehlt.
    pub fn position_von(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Alle Identitaeten in Reihenfolge der Leiste.
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    /// Ladezustand des aktiven Tabs.
    pub fn ladezustand(&self) -> &Ladezustand {
        static LEER: Ladezustand = Ladezustand::Leer;
        self.tabs
            .get(self.aktiv)
            .map(|t| &t.ladezustand)
            .unwrap_or(&LEER)
    }

    /// Ladezustand des Tabs mit dieser `TabId`.
    pub fn ladezustand_von(&self, id: TabId) -> Option<&Ladezustand> {
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| &t.ladezustand)
    }

    /// Den Ladezustand des aktiven Tabs setzen. Benutzt der Aufrufer,
    /// der das Laden macht: vorher [`Ladezustand::Laedt`], bei Erfolg
    /// [`Ladezustand::Fertig`], sonst `Fehler(text)`.
    pub fn ladezustand_setzen(&mut self, zustand: Ladezustand) {
        if let Some(tab) = self.tabs.get_mut(self.aktiv) {
            tab.ladezustand = zustand;
        }
    }

    /// Den Titel der aktuellen Seite setzen — nach dem Laden.
    ///
    /// DER TITEL GEHOERT ZUM VERLAUFSEINTRAG, nicht zum Tab: es koennen
    /// zwei Seiten im selben Tab verschiedene Titel haben. Getestet in
    /// `titel_ist_je_verlaufseintrag_eigen`.
    pub fn titel_setzen(&mut self, titel: &str) {
        if let Some(tab) = self.tabs.get_mut(self.aktiv) {
            if let Some(seite) = tab.verlauf.get_mut(tab.index) {
                seite.titel = titel.to_string();
            }
        }
    }

    /// Der Titel der aktuellen Seite, leer wenn es keinen gab.
    pub fn titel(&self) -> &str {
        self.tabs
            .get(self.aktiv)
            .and_then(|t| t.verlauf.get(t.index))
            .map(|p| p.titel.as_str())
            .unwrap_or("")
    }

    /// Scrollposition des aktiven Tabs in Pixeln von oben.
    pub fn versatz(&self) -> u32 {
        self.tabs.get(self.aktiv).map(|t| t.versatz).unwrap_or(0)
    }

    /// Scrollposition setzen. Kommt vom Aufrufer; diese Crate rechnet
    /// keine Fenstergroessen (das ist Layout, fremder Agent).
    pub fn versatz_setzen(&mut self, pixel: u32) {
        if let Some(tab) = self.tabs.get_mut(self.aktiv) {
            tab.versatz = pixel;
        }
    }

    /// Kann der aktive Tab ueberhaupt scrollen? Das ist
    /// `inhalts_hoehe - fenster_hoehe`, beides vom Aufrufer — ohne diese
    /// beiden Zahlen ist die Frage nicht beantwortbar, und eine erfundene
    /// Fenstergroesse waere hier eine Luege.
    pub fn kann_scrollen(&self, fenster_hoehe: u32, inhalt_hoehe: u32) -> bool {
        inhalt_hoehe > fenster_hoehe && self.versatz() + fenster_hoehe < inhalt_hoehe
    }

    // ------------------------------------------------------------------
    // CRATE-INTERN: Aufbau aus geparsten Teilen. Nicht oeffentlich, weil
    // es die Invarianten umgeht -- es nimmt ihnen nichts weg (es laeuft
    // durch `verlauf_einkuerzen`), aber es traegt auch nichts bei.
    //
    // `sitzung_lesen` braucht das: es rekonstruiert einen Zustand aus
    // einer Zeichenkette, und das ist etwas anderes als "benutze den
    // Browser". Oeffentlich wuerde es eine zweite Art geben, einen Tab zu
    // erzeugen -- und zwei Arten sind zwei Regeln.
    // ------------------------------------------------------------------

    /// Ein Browser mit `anzahl` leeren Tabs, Ids 1..=anzahl.
    pub(crate) fn leer_mit_0_tabs() -> Self {
        Browser {
            tabs: Vec::new(),
            aktiv: 0,
            naechste_tab_id: 1,
            lesezeichen: Vec::new(),
            //
            // KONTEXT (04.10.2026, 10:40): Der aus einer Datei **gelesene**
            // Zustand hat **keinen** geschlossenen Tab — das Dateiformat kennt
            // das Feld noch nicht (EIGENTUM: `sitzung_lesen` liest Verlauf,
            // Ids, Ladezustand und Versatz). GEMESSENE ENTSCHEIDUNG, warum hier
            // `None` und nicht „der letzte Tab der Liste“: ein aus der Datei
            // gelesener Browser **hat** nichts geschlossen, und „der letzte der
            // Liste“ waere eine erfundene Vergangenheit. Wer das Feld
            // serialisiert (KONVERT), ergaenzt es dort — diese Datei ist
            // fremd, deshalb steht hier nur das `None`.
            // KONTEXT (04.10.2026, 12:50): der aus einer Datei gelesene Stapel
            // ist **leer** (das Format fuehrt keine geschlossenen Tabs).
            zuletzt_geschlossen: Vec::new(),
            // KONTEXT (04.10.2026, 14:30): und **kein** Zoom (das Format fuehrt
            // auch keine Zooms — das entscheidet KONVERT, wenn es sie fuehrt).
            zoom_je_host: std::collections::HashMap::new(),
            // KONTEXT (05.10.2026, 07:05): und **keine** Ladewünsche — der
            // aus einer Datei gelesene Browser laedt nichts (das Dateiformat
            // kennt die beiden Wünsche noch nicht, genau wie den Stapel).
            // GEMESSENE ENTSCHEIDUNG, warum hier kein `Kommentar` noetig ist:
            // „gelesen heisst nicht geladen“ ist dieselbe Antwort wie beim Stapel.
            ladung_hart: false,
            ladung_abgebrochen: false,
        }
    }

    /// Einen Tab mit vorgegebener Id und Inhalt anhaengen.
    pub(crate) fn haenge_tab_an(
        &mut self,
        id: TabId,
        verlauf: Vec<Page>,
        ladezustand: Ladezustand,
        versatz: u32,
        index: usize,
    ) {
        // Der Verlauf ist nie leer und der Index liegt darin: beides sind
        // Invarianten, die auch ein aus der Datei gelesener Zustand haben
        // muss. Ohne diese zwei Zeilen haette ein gelesener Browser einen
        // Zustand, den kein Bedienbefehl erreichen kann.
        let verlauf = if verlauf.is_empty() {
            vec![Page::leer()]
        } else {
            verlauf
        };
        // Der Index wird begrenzt, nicht uebernommen: eine Datei von aussen
        // darf keinen Index 9999 in einen Verlauf von 3 Seiten setzen.
        let index = index.min(verlauf.len() - 1);
        self.tabs.push(Tab {
            verlauf,
            index,
            id,
            ladezustand,
            versatz,
            //
            // KONTEXT (04.10.2026, 10:40): Ein aus der Datei gelesener Tab ist
            // **nicht** angeheftet und **nicht** lautlos (siehe oben).
            gepinnt: false,
            lautlos: false,
        });
    }

    /// Der Verlauf eines Tabs, unabh. davon ob er aktiv ist.
    pub fn seiten_von(&self, id: TabId) -> &[Page] {
        static LEER: &[Page] = &[];
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.verlauf.as_slice())
            .unwrap_or(LEER)
    }

    /// Die Position IM VERLAUF eines Tabs — nicht die Position des Tabs in
    /// der Leiste. Zwei verschiedene Zahlen, und der Rundlauf braucht
    /// beide: welcher Tab war aktiv, und wo in seinem Verlauf stand man.
    pub fn index_von(&self, id: TabId) -> usize {
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.index)
            .unwrap_or(0)
    }

    /// Der Versatz eines Tabs, unabh. davon ob er aktiv ist.
    pub fn versatz_von(&self, id: TabId) -> u32 {
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.versatz)
            .unwrap_or(0)
    }

    /// Der Verlaufseintrag der aktuellen Seite, mit Titel.
    pub fn aktuelle_seite(&self) -> Option<&Page> {
        let tab = self.tabs.get(self.aktiv)?;
        tab.verlauf.get(tab.index)
    }
}

// ===========================================================================
// DAS SICHTBARE FENSTER DER TABLEISTE — welche Tabs stehen nebeneinander
// ===========================================================================
//
// GEMESSENER AUFTRAG (UI, 04.10.2026, 05:0x, waehrend `narsil-chrome` rot war):
// „Logik in `narsil-browser` bauen und testen“ — und die einzige Luecke, die die
// Tableiste dort noch hat, ist der **Ueberlauf**: bei 32 Tabs x GEMESSEN 76 px
// sind das 2432 px, und ein 800-px-Fenster zeigt davon GEMESSEN 10.
//
// GEMESSENE ENTSCHEIDUNG, warum die Rechnung **hier** steht und nicht in
// `narsil-chrome`: die Frage „wie viele Tabs passen in die Leiste“ ist eine
// Frage ueber **Tabs** (wie viele es gibt, welcher aktiv ist) — und beides
// weiss nur diese Crate. `narsil-chrome` weiss die **Pixel** (TAB_RAND, Plus,
// Fenstertasten) und fragt dann hier nach der Zahl.

impl Browser {
    /// WIE VIELE TABS PASSEN NEBENEINANDER IN `leisten_breite`?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ganzzahlig** und nicht „der letzte
    /// passt noch halb“: ein halber Tab ist kein Tab, und die Leiste zeigt
    /// immer ganze Tabs. GEMESSENE ENTSCHEIDUNG, warum **mindestens 1**:
    /// ein Fenster mit **null** sichtbaren Tabs hat keine Leiste — und die
    /// Leiste ist immer da.
    pub fn tabs_passen(&self, leisten_breite: u32, tab_breite: u32) -> usize {
        let je_zeile = tab_breite.max(1);
        let passen = (leisten_breite / je_zeile) as usize;
        passen.clamp(1, self.tabs.len().max(1))
    }

    /// DAS SICHTFENSTER als `(von, bis)` — die Tab-**Positionen**, die
    /// nebeneinander stehen. `bis` ist **exklusiv** (wie ein Schnitt).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Fenster den **aktiven** Tab zeigt
    /// und nicht „immer von vorn“: wer den Tab wechselt, will ihn sehen.
    /// GEMESSENE ENTSCHEIDUNG, warum der aktive Tab **rechts** steht und
    /// nicht in der Mitte: das Fenster rutscht nur, wenn es muss, und
    /// „rutscht nur, wenn es muss“ heisst: der aktive Tab ist der **letzte**
    /// sichtbare — ausser er ist der letzte Tab ueberhaupt, dann steht er
    /// links, weil rechts nichts mehr folgt.
    pub fn sichtfenster(&self, leisten_breite: u32, tab_breite: u32) -> (usize, usize) {
        let platz = self.tabs_passen(leisten_breite, tab_breite);
        if self.tabs.is_empty() {
            return (0, 0);
        }
        let aktiv = self.aktiv.min(self.tabs.len() - 1);
        let von = aktiv.saturating_add(1).saturating_sub(platz).min(self.tabs.len() - platz);
        (von, von + platz.min(self.tabs.len() - von))
    }

    /// DIE TABS, DIE DAS FENSTER ZEIGT — als Scheibe, in Fensterreihenfolge.
    pub fn sichtbare_tabs(&self, leisten_breite: u32, tab_breite: u32) -> &[Tab] {
        let (von, bis) = self.sichtfenster(leisten_breite, tab_breite);
        &self.tabs[von..bis]
    }

    /// DAS FENSTER UM EINEN TAB VERSCHIEBEN — die **Ueberlauf-Pfeile**.
    ///
    /// `richtung`: `1` = ein Tab nach rechts, `-1` = ein Tab nach links,
    /// `0` = unveraendert. `None`, wenn es **nicht** geht: dann darf der
    /// Pfeil nicht klickbar sein (sonst ist er ein Knopf ohne Wirkung —
    /// die Fehlerklasse, die `funktions_rundgang` misst).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option` und kein `bool`: „nicht
    /// geht“ ist eine **Aussage ueber den Zustand**, die der Zeichner
    /// braucht (Pfeil blass oder gar nicht da), und `bool` plus
    /// `sichtfenster()` waere **zwei** Fragen an zwei Stellen.
    pub fn sichtfenster_um(
        &self,
        leisten_breite: u32,
        tab_breite: u32,
        richtung: i32,
    ) -> Option<(usize, usize)> {
        let (von, bis) = self.sichtfenster(leisten_breite, tab_breite);
        match richtung.cmp(&0) {
            std::cmp::Ordering::Equal => Some((von, bis)),
            std::cmp::Ordering::Greater => {
                // nach rechts: nur, wenn rechts noch ein Tab steht
                if bis >= self.tabs.len() {
                    return None;
                }
                Some((von + 1, (bis + 1).min(self.tabs.len())))
            }
            std::cmp::Ordering::Less => {
                if von == 0 {
                    return None;
                }
                Some((von - 1, bis - 1))
            }
        }
    }
}

#[cfg(test)]
mod sichtfenster_tests {
    use super::*;

    /// GEMESSENE RECHNUNG (UI_18 (4)): 32 Tabs x 76 px = 2432 px; in einem
    /// 800-px-Fenster passen GEMESSEN `800 / 76 = 10` Tabs nebeneinander.
    ///
    /// GEMESSENER EIGENER FEHLER (zweimal, 05:00 und 05:01): mein Helfer
    /// ging von **einem** Tab am Anfang aus. GEMESSEN ist
    /// [`Browser::neu`] mit **drei** Tabs (KONTEXT, 04.10.) — also hatte der
    /// Test GEMESSEN 34 Tabs und pruefte eine Zahl, die er nicht genannt
    /// hatte. Die Lehre steht im Kommentar: **ein Test prueft die Zahl, die
    /// er behauptet** — und die behauptet man aus dem Zustand, nicht aus der
    /// Erinnerung.
    fn browser_mit(n: usize) -> Browser {
        let mut b = Browser::neu();
        while b.tabs.len() > n {
            let id = b.aktive_tab_id().expect("es gibt einen aktiven Tab");
            assert!(b.tab_schliessen(id), "der aktive Tab laesst sich schliessen");
        }
        while b.tabs.len() < n {
            b.tab_oeffnen();
        }
        assert_eq!(b.tabs.len(), n, "GEMESSEN: der Helfer liefert genau {n} Tabs");
        b
    }

    /// Der Tab an **Position** `i` (0-basiert) — als `TabId`.
    fn id_an(b: &Browser, i: usize) -> TabId {
        b.tab_ids().get(i).copied().expect("Tab an der Position")
    }

    #[test]
    fn ein_tab_zeigt_genau_einen() {
        let b = browser_mit(1);
        assert_eq!(b.tabs.len(), 1, "`Browser::neu()` legt genau einen Tab an");
        assert_eq!(b.sichtfenster(800, 76), (0, 1), "1 Tab -> Fenster (0, 1)");
        assert_eq!(b.sichtbare_tabs(800, 76).len(), 1);
    }

    #[test]
    fn fuenf_zehn_tabs_passen_in_800_pixel() {
        let b = browser_mit(32);
        assert_eq!(b.tabs.len(), 32);
        assert_eq!(b.tabs_passen(800, 76), 10, "GEMESSEN: 800 / 76 = 10");
        // Der aktive Tab ist der **letzte** (Position 31) -> Fenster 22..32.
        assert_eq!(b.sichtfenster(800, 76), (22, 32), "GEMESSEN: 31 - 10 + 1 = 22");
        assert_eq!(b.sichtbare_tabs(800, 76).len(), 10, "das Fenster zeigt genau zehn Tabs");
    }

    #[test]
    fn das_fenster_zeigt_immer_den_aktiven_tab() {
        let mut b = browser_mit(32);
        for i in [0usize, 5, 15, 31] {
            b.tab_waehlen(id_an(&b, i));
            let (von, bis) = b.sichtfenster(800, 76);
            assert!(
                von <= i && i < bis,
                "Tab {i} liegt nicht im Fenster {von}..{bis}"
            );
        }
    }

    #[test]
    fn der_pfeil_nach_rechts_hoert_am_ende_auf() {
        let mut b = browser_mit(32);
        // In der Mitte ist ein Schritt nach rechts moeglich …
        b.tab_waehlen(id_an(&b, 15));
        let vorher = b.sichtfenster(800, 76);
        let nachher = b
            .sichtfenster_um(800, 76, 1)
            .expect("in der Mitte geht es nach rechts");
        assert_eq!(vorher, (6, 16), "GEMESSEN: der aktive Tab 15 ist der letzte sichtbare");
        assert_eq!(nachher, (7, 17), "ein Schritt nach rechts: 7..17");
        // … und ganz rechts nicht mehr: das Fenster endet bei 32.
        b.tab_waehlen(id_an(&b, 31));
        assert_eq!(b.sichtfenster(800, 76).1, 32);
        assert_eq!(
            b.sichtfenster_um(800, 76, 1),
            None,
            "am rechten Ende gibt es keinen Weg nach rechts — der Pfeil darf dann nicht klickbar sein"
        );
    }

    #[test]
    fn der_pfeil_nach_links_hoert_am_ende_auf() {
        let mut b = browser_mit(32);
        b.tab_waehlen(id_an(&b, 15));
        assert_eq!(
            b.sichtfenster_um(800, 76, -1),
            Some((5, 15)),
            "ein Schritt nach links: 5..15"
        );
        // Ganz links: Fenster ab 0, also kein Weg nach links.
        b.tab_waehlen(id_an(&b, 0));
        assert_eq!(b.sichtfenster(800, 76).0, 0);
        assert_eq!(
            b.sichtfenster_um(800, 76, -1),
            None,
            "am linken Ende gibt es keinen Weg nach links"
        );
    }

    #[test]
    fn ein_riesen_fenster_zeigt_25_tabs_und_der_pfeil_ist_still() {
        let b = browser_mit(32);
        assert_eq!(b.tabs_passen(1920, 76), 25, "GEMESSEN: 1920 / 76 = 25");
        let (von, bis) = b.sichtfenster(1920, 76);
        assert_eq!(bis, 32, "GEMESSEN: 32 - 25 = 7 -> Fenster 7..32");
        assert_eq!(von, 7);
        assert_eq!(
            b.sichtfenster_um(1920, 76, 1),
            None,
            "es fehlt rechts nichts — der Pfeil ist nicht klickbar"
        );
    }

    #[test]
    fn eine_kleine_leiste_zeigt_mindestens_einen_tab() {
        let b = browser_mit(4);
        assert_eq!(b.tabs_passen(10, 76), 1, "GEMESSEN: 10 / 76 = 0 -> mindestens 1");
        let (von, bis) = b.sichtfenster(10, 76);
        assert_eq!(bis - von, 1);
    }
}
