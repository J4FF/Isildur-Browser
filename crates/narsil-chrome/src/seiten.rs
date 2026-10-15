//! A-SEITEN (Koordination 02.10., Runde 6) — `about:downloads` und
//! `about:verlauf`.
//!
//! # WAS VERLANGT IST
//!
//! > U-SEITEN about:downloads und about:verlauf (schlicht, Suchen/Loeschen).
//! > Abnahme: Fensterbilder mit echtem Inhalt (echte Seite).
//!
//! „SCHLICHT" ist die einzige Vorgabe, und sie ist eine Abgrenzung:
//! ANFORDERUNGEN.txt Zeile 15 verbietet Seitenleisten, Zeile 16 verbietet
//! Widgets und Empfehlungen. Eine Downloadliste mit Filtern, Zeitstrahl und
//! Kategorien waere ein Dashboard. Zwei Dinge kann man hier: **suchen** und
//! **loeschen** — mehr nicht, und beides an EINER Stelle.
//!
//! # DIE ZWEI SEITEN
//!
//! | Seite | Zustand | Zeile |
//! |-------|---------|-------|
//! | `about:downloads` | [`Downloads`] + Verlauf der Dateien | Datei, Groesse, Zustand |
//! | `about:verlauf` | die besuchten Seiten | Titel, Adresse, Zeit |
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Verlauf eine EIGENE Liste ist und
//! nicht die Liste der offenen Tabs: der Verlauf ist das, was war, und die
//! Tabs sind das, was ist. Wer „Seite schliessen" tippt, will die
//! Historie behalten.
//!
//! # WAS HIER NICHT GEHOERT
//!
//! * Das Aufraeumen der Datei `~/.config/narsil/verlauf.txt` — `narsil-verlauf`
//!   gehoert dem Cache; hier steht die Liste mit ihren Grenzen.

use super::{Chrome, Rechteck};
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, trf};

/// M3: hoechstens so viele Zeilen zeigt eine der beiden Seiten.
///
/// GEMESSENE BEGRUENDUNG fuer 100: 100 Zeilen zu je [`ZEILE`] (28 px) sind
/// 2800 px — mehr als jedes Fenster. Der Nutzer scrollt nicht (dafuer gibt
/// es kein Rad-Ereignis in der Chrome), also zeigt die Seite, was in ein
/// Fenster passt, und der Rest ist ueber die Suche erreichbar. GEMESSENE
/// ENTSCHEIDUNG, warum nicht 20: eine Verlaufsseite, die 20 Zeilen zeigt und
/// den Rest versteckt, ist eine Seite, die man nicht benutzt — mit Suche
/// findet man auch 10 000 Eintraege.
pub const MAX_ZEILEN: usize = 100;

/// M3: die Zeilenhoehe einer Zeile.
///
/// GEMESSENE ENTSCHEIDUNG fuer [`crate::einstellungen::ZEILE`] (24) + 4:
/// es stehen ZWEI Texte uebereinander (z. B. Titel und Adresse), und bei
/// 24 px waren sie 1 px von den Raendern entfernt. GEMESSEN: 28 gibt je
/// 7 px Luft bei 14 px Text — dieselbe Zahl wie bei der
/// Lesezeichenverwaltung (`lesezeichen::ZEILE_VERWALTUNG`), und dieselbe
/// Zahl aus demselben Grund.
pub const ZEILE: u32 = 28;

/// M3: der Rand der Seite (links und rechts).
pub const MARGIN: u32 = 24;

/// M3: die Breite des Suchfelds oben auf der Seite.
///
/// GEMESSENE ENTSCHEIDUNG fuer 320: es ist die Haelfte der
/// Adressleiste bei 800 px (`adressleiste().breite` = 616), und 320 px sind
/// 40 Zeichen — genug fuer „wikipedia" und lang genug fuer
/// „downloads/bericht.pdf".
pub const BREITE_SUCHE: u32 = 320;

/// M3: die Hoehe des Suchfelds (wie die Adressleiste).
pub const HOEHE_SUCHE: u32 = super::ADRESSE_HOEHE;

/// M4: die laengste Suche in einer dieser Seiten.
///
/// GEMESSENE BEGRUENDUNG fuer 512: dieselbe Zahl wie
/// [`crate::MAX_SUCHTEXT`] und [`crate::suche::MAX_SUCHANFRAGE`]. Es ist
/// Text in einem Feld, und ein Feld ist hoechstens so breit wie sein Fenster.
pub const MAX_SUCHTEXT: usize = 512;

/// M4: die laengste Zeile, die gespeichert wird.
///
/// GEMESSENE BEGRUENDUNG fuer 512: Titel und Adresse kommen aus dem Dokument
/// und aus dem Netz; ein `<title>` kann 10 000 Zeichen haben. 512 Zeichen
/// sind 4 KiB pro Zeile, und bei [`MAX_ZEILEN`] Zeilen sind das 400 KiB im
/// Speicher — das ist die Grenze, die man noch vertreten kann.
pub const MAX_ZEILEN_LAENGE: usize = 512;

/// EIN VERLAUFSEINTRAG — eine besuchte Seite.
///
/// GEMESSENE BEGRUENDUNG, warum `Download` nicht auch hier passt: ein
/// Download hat einen Zustand (laeuft/fertig/fehlgeschlagen) und einen
/// Dateinamen, eine Seite hat einen Titel und eine Zeit. Ein gemeinsamer Typ
/// wuerde bei beiden Felder haben, die immer leer sind — und die Leere
/// ist die Frage „leer oder unbekannt?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerlaufEintrag {
    /// Der Titel der Seite (aus `<title>`, M4: gekuerzt).
    pub titel: String,
    /// Die Adresse (M4: gekuerzt).
    pub adresse: String,
    /// Wann die Seite besucht wurde (Minuten seit dem Start des Browsers).
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer eine Zahl und kein `String`: die Zeit
    /// steht in einem Feld, das nach nichts sortiert werden muss (der
    /// Verlauf ist bereits in der Reihenfolge des Speicherns), und eine Zahl
    /// laesst sich vergleichen. GEMESSENE ENTSCHEIDUNG fuer MINUTEN und
    /// nicht Sekunden: „vor 3 Minuten" ist die Angabe, die man braucht; „vor
    /// 187 Sekunden" ist eine Rechnung fuer den Nutzer.
    pub vor_minuten: u32,
}

impl VerlaufEintrag {
    /// EIN EINTRAG, mit den Grenzen (M4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Titel LEER auf die Adresse zurueck-
    /// faellt: eine Seite ohne `<title>` ist selten, aber eine mit einem
    /// leeren Titel kommt vor. Ein leerer Titel in der Liste waere eine
    /// Zeile, in der nichts steht — und der Nutzer kann sie nicht von einer
    /// anderen leeren Zeile unterscheiden.
    pub fn neu(titel: &str, adresse: &str, vor_minuten: u32) -> VerlaufEintrag {
        let adresse: String = adresse.trim().chars().take(MAX_ZEILEN_LAENGE).collect();
        let titel: String = titel.trim().chars().take(MAX_ZEILEN_LAENGE).collect();
        let titel = if titel.is_empty() {
            adresse.clone()
        } else {
            titel
        };
        VerlaufEintrag {
            titel,
            adresse,
            vor_minuten,
        }
    }

    /// DER TEXT DER ZEILE (die Anzeige, M4).
    pub fn zeile(&self) -> String {
        // GEMESSENE ENTSCHEIDUNG, warum Zeit UND Adresse in einer Zeile
        // stehen: die Liste ist eine Liste von Orten, und „vor 3 Minuten"
        // steht links, damit man die Liste nach unten als Zeitachse lesen
        // kann.
        trf!(
            "{} — {} · {} min ago",
            self.titel, self.adresse, self.vor_minuten
        )
    }
}

/// DIE VERLAUFSSEITE — Zustand, Suche, Auswahl.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerlaufSeite {
    /// Die Eintraege (hoechstens `MAX_ZEILEN`).
    pub eintraege: Vec<VerlaufEintrag>,
    /// Der Text im Suchfeld.
    pub suchtext: String,
    /// Der ausgewaehlte Eintrag (`None` = keiner).
    pub ausgewaehlt: Option<usize>,
}

impl VerlaufSeite {
    /// EINEN EINTRAG HINZUFUEGEN (M3).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `remove(0)` bei voller Liste: wie bei
    /// den Lesezeichen — das neueste ist das, wonach man gerade gefragt hat.
    pub fn hinzufuegen(&mut self, eintrag: VerlaufEintrag) -> bool {
        // GEMESSENE ENTSCHEIDUNG: dieselbe Adresse nur einmal. GEMESSENE
        // BEGRUENDUNG: der Verlauf bekommt bei jedem Besuch einen Eintrag
        // (das ist der Verlauf), aber die LISTE zeigt ihn einmal — mit dem
        // NEUEN Zeitwert, weil „vor 1 Minute" juenger ist als „vor 60".
        //
        // GEMESSENER EIGENER FEHLER (02.10., Compiler „cannot borrow
        // `self.eintraege` as mutable more than once"): ich hatte
        // `iter_mut().find(...)` UND dann `remove(0)`/`insert(0, …)` im
        // selben Zweig — zwei geliehene Zeiger auf dieselbe Liste. DER WEG:
        // `position` (nur lesend) sucht die Nummer, dann wird an dieser
        // Nummer geschrieben und der Eintrag nach vorn verschoben.
        if let Some(nr) = self
            .eintraege
            .iter()
            .position(|e| e.adresse == eintrag.adresse)
        {
            let mut alt = self.eintraege.remove(nr);
            alt.titel = eintrag.titel;
            alt.vor_minuten = eintrag.vor_minuten;
            self.eintraege.insert(0, alt);
            return false;
        }
        //
        // GEMESSENER EIGENER FEHLER (02.10.,
        // `die_verlaufsliste_ist_begrenzt_und_sortiert`): hier stand
        // `remove(0)` + `push`. GEMESSEN: `left: "https://s20.example",
        // right: "https://s119.example"` — der NEUESTE Eintrag stand am
        // ENDE der Liste, weil `push` anhaengt. Der Kommentar am Feld
        // sagt aber „die Reihenfolge ist die des Speicherns, neueste
        // vorn". Zwei Orte, zwei Aussagen.
        //
        // GEMESSENE ENTSCHEIDUNG: NEUSTE VORN. Der Verlauf wird von oben
        // gelesen (oben ist „gerade eben"), und die Suche zeigt die
        // obersten Treffer zuerst. Damit das im Bild stimmt, wird vorn
        // eingefuegt — und bei voller Liste faellt der LETZTE raus, nicht
        // der erste.
        if self.eintraege.len() >= MAX_ZEILEN {
            self.eintraege.pop();
        }
        self.eintraege.insert(0, eintrag);
        true
    }

    /// IN DER LISTE SUCHEN (M4: der Text wird gekuerzt).
    pub fn suchen(&mut self, text: &str) {
        self.suchtext = text.trim().chars().take(MAX_SUCHTEXT).collect();
        self.ausgewaehlt = None;
    }

    /// DIE SICHTBAREN ZEILEN — nach der Suche gefiltert (M3).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier gefiltert wird und nicht im Bild:
    /// `treffer` und `display_list` brauchen dieselbe Liste. Wer sie an
    /// zwei Stellen bildet, zeigt an einer Stelle 30 Zeilen und trifft an der
    /// anderen den 31.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier GROSS/KLEIN egal ist und nicht wie
    /// bei den Adressvorschlaegen: hier wird in TITELN gesucht, und ein Titel
    /// fängt mit einem GROSSBUCHSTABEN an („Rust (Programmiersprache)"). Wer
    /// „rust" tippt, will genau diese Zeile finden — also wird auf
    /// Kleinschreibung verglichen (`to_lowercase`, ohne Zusatzkosten
    /// fuer den unveraenderten Fall, weil `str::to_lowercase` die
    /// Zeichenkette nur einmal anfasst).
    pub fn sichtbare_zeilen(&self) -> Vec<&VerlaufEintrag> {
        if self.suchtext.is_empty() {
            return self.eintraege.iter().take(MAX_ZEILEN).collect();
        }
        let suche = self.suchtext.to_lowercase();
        self.eintraege
            .iter()
            .filter(|e| {
                e.titel.to_lowercase().contains(&suche) || e.adresse.to_lowercase().contains(&suche)
            })
            .take(MAX_ZEILEN)
            .collect()
    }

    /// EINEN EINTRAG LOESCHEN — und seinen Text zurueckgeben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option<VerlaufEintrag>` und nicht
    /// `bool`: der Aufrufer soll in der Statuszeile sagen KÖNNEN, WELCHER
    /// Eintrag weg ist („wikipedia.org geloescht"). Aus `bool` kann er das
    /// nicht, und die Nachricht waere dann entweder gar keine oder eine
    /// ohne Namen — und „geloescht" ohne Namen ist bei einer Liste, in der
    /// zweimal „Wikipedia" stehen kann, eine halbe Auskunft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Eintrag VOR dem Entfernen kopiert
    /// wird: nach `retain` ist er nicht mehr in der Liste, und wer ihn dann
    /// lesen will, hat nichts.
    pub fn loeschen(&mut self, adresse: &str) -> Option<VerlaufEintrag> {
        let nr = self.eintraege.iter().position(|e| e.adresse == adresse)?;
        let weg = self.eintraege.remove(nr);
        self.ausgewaehlt = None;
        Some(weg)
    }

    /// DIE GANZE LISTE LOESCHEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und nicht
    /// `loeschen` je Adresse: ANFORDERUNGEN Zeile 15/16 — eine Liste, die
    /// sich auf einmal leeren laesst, ist die einzige Funktion, die man
    /// braucht, wenn man sich nicht sicher ist, was man loescht.
    pub fn alle_loeschen(&mut self) -> usize {
        let n = self.eintraege.len();
        self.eintraege.clear();
        self.ausgewaehlt = None;
        n
    }
}

/// DIE DOWNLOADS-SEITE — Zustand und Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Downloadliste hier ihre EIGENE Grenze
/// hat und nicht [`crate::downloads::MAX_DOWNLOADS`] (3) gilt: die Leiste
/// unten zeigt drei Zeilen (mehr verdeckten die Seite), die SEITE zeigt
/// hundert (mehr hat niemand). GEMESSENE BEGRUENDUNG, warum das dieselbe
/// Zahl ist: ein Browser, der 1000 Dateien heruntergelaaden hat, will die
/// finden — ueber die Suche, ueber den Filter, nicht ueber Scrollen.
///
/// GEMESSENER EIGENER FEHLER (02.10.): dieses `Eq` war falsch, weil
/// `Download` eine `Fortschritt` enthaelt (mit `f32`), und `Eq` fuer `f32`
/// nicht gilt. GEMESSENE ENTSCHEIDUNG, warum man `Download` NICHT auf `Eq`
/// umstellt: dasselbe gilt wie bei `Chrome` — `PartialEq` reicht, und ein
/// `Eq` fuer Flusszahlen zu behaupten waere eine Behauptung, die nicht
/// stimmt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DownloadSeite {
    /// Die Dateien (die Reihenfolge ist die des Speicherns, neueste vorn).
    pub eintraege: Vec<crate::downloads::Download>,
    /// Der Text im Suchfeld.
    pub suchtext: String,
    /// Der ausgewaehlte Eintrag (`None` = keiner).
    pub ausgewaehlt: Option<usize>,
}

impl DownloadSeite {
    /// DIE SICHTBAREN ZEILEN — nach der Suche gefiltert (M3).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Suche den DATEINAMEN nimmt und
    /// nicht den Pfad: wer „bericht" tippt, sucht eine Datei namens bericht.
    /// Der Pfad enthaelt den Benutzernamen („/home/jaff/…"), und der
    /// Benutzername steht in keinem Dateinamen.
    pub fn sichtbare_zeilen(&self) -> Vec<&crate::downloads::Download> {
        if self.suchtext.is_empty() {
            return self.eintraege.iter().take(MAX_ZEILEN).collect();
        }
        let suche = self.suchtext.to_lowercase();
        self.eintraege
            .iter()
            .filter(|d| d.name.to_lowercase().contains(&suche))
            .take(MAX_ZEILEN)
            .collect()
    }

    /// IN DER LISTE SUCHEN (M4).
    pub fn suchen(&mut self, text: &str) {
        self.suchtext = text.trim().chars().take(MAX_SUCHTEXT).collect();
        self.ausgewaehlt = None;
    }

    /// DEN AUSGEWAEHLTEN EINTRAG LOESCHEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das LOESCHEN der Datei ist und nicht
    /// nur das Eintragen aus der Liste: eine Downloadseite, deren Zeile man
    /// loeschen kann, ohne dass die Datei weg ist, ist eine Seite ueber
    /// Eintraege — und die Datei laege beim naechsten Plattenlauf wieder da.
    /// GEMESSENE BEGRUENDUNG fuer `bool` und nicht `Option<String>`: es gibt
    /// hier keinen Text zurueck (der Dateiname steht in der Zeile), und der
    /// Aufrufer loescht die Datei.
    pub fn gewaehlten_loeschen(&mut self) -> bool {
        let Some(nr) = self.ausgewaehlt else {
            return false;
        };
        if nr >= self.eintraege.len() {
            self.ausgewaehlt = None;
            return false;
        }
        self.eintraege.remove(nr);
        self.ausgewaehlt = None;
        true
    }
}

/// WELCHE DER VIER SEITEN GEZEIGT WIRD.
///
/// GEMESSENE ENTSCHEIDUNG (Koordination 02.10., „Feinschliff aller Seiten
/// (about:*)"): das Enum kennt jetzt ALLE VIER `about:*`-Seiten und nicht
/// nur die zwei aus Runde 6. GEMESSENE BEGRUENDUNG: „welche Seite ist das"
/// ist eine Frage, die `narsil` bei jedem Aufruf stellt — mit vier Werten in
/// zwei Listen (zwei hier, zwei in den Modulen `startseite.rs` und
/// `einstellungen.rs`) gibt es zwei Orte fuer dieselbe Antwort. GEMESSENE
/// BEGRUENDUNG, warum die Seiten trotzdem in VIER Dateien bleiben: die
/// Verlaufseite und die Downloadseite sind fast gleich, aber ihre Zustaende
/// sind es nicht (`VerlaufEintrag` gegen `Download`); eine Datei fuer alles
/// waere eine Datei mit zwei halben Seiten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seite {
    /// `about:start` — das Suchfeld in der Mitte.
    Start,
    /// `about:einstellungen` — die fuenf Kategorien.
    Einstellungen,
    /// `about:downloads`.
    Downloads,
    /// `about:verlauf`.
    Verlauf,
    /// `about:tasten` — die Liste der Tastenkuerzel (Koordination 02.10.).
    Tasten,
}

impl Seite {
    /// DER NAME, der im Tab steht UND als UEBERSCHRIFT auf der Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Koordination, Feinschliff): der Name steht an
    /// BEIDEN Stellen. GEMESSENE BEGRUENDUNG: die Adresse in der Adressleiste
    /// ist gekuerzt („about:verl…") und ist ein Schema; „Verlauf" ist der
    /// Name der Seite, und eine Seite, die ihren Namen nicht zeigt, ist ein
    /// Bildschirm ohne Titel. GEMESSENE ENTSCHEIDUNG fuer die Startseite:
    /// sie heisst „Start" (nicht „Startseite") — das ist der Name, den der
    /// Tab traegt und der kuerzeste, der eindeutig ist.
    pub fn text(&self) -> &'static str {
        match self {
            Seite::Start => tr("Start"),
            Seite::Einstellungen => tr("Settings"),
            Seite::Downloads => tr("Downloads"),
            Seite::Verlauf => tr("History"),
            Seite::Tasten => tr("Keyboard shortcuts"),
        }
    }

    /// DIE ADRESSE dieser Seite.
    pub fn adresse(&self) -> &'static str {
        match self {
            Seite::Start => "about:start",
            Seite::Einstellungen => "about:einstellungen",
            Seite::Downloads => "about:downloads",
            Seite::Verlauf => "about:verlauf",
            Seite::Tasten => "about:tasten",
        }
    }

    /// WELCHE ZEICHENLISTE gehoert zu dieser Seite? (der Name, den `narsil`
    /// im `match` braucht — mit Datei und Funktion, damit `narsil` nicht
    /// raten muss).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Text und kein Zeiger ist: eine
    /// Funktion in einem Text kann man nicht aufrufen, und ein `enum` mit
    /// Funktionszeigern braucht `dyn` und einen lebenslangen Kasten fuer
    /// vier Funktionen. GEMESSENE BEGRUENDUNG fuer die Reihenfolge: die Liste
    /// ist die Reihenfolge, in der `narsil` sie in einem `match` aufschreibt —
    /// Start, Einstellungen, Downloads, Verlauf (alphabetisch ist es
    /// Download, Einstellungen, Start, Verlauf; die Lesereihenfolge ist
    /// angenehmer als die Wörterbuchsordnung).
    pub fn zeichnen(&self) -> &'static str {
        match self {
            Seite::Start => "startseite::display_list",
            Seite::Einstellungen => "einstellungen::display_list",
            Seite::Downloads => "seiten::downloads_display_list",
            Seite::Verlauf => "seiten::verlauf_display_list",
            Seite::Tasten => "tasten::display_list",
        }
    }

    /// WELCHE TREFFER-FUNktion gehoert zu dieser Seite?
    pub fn treffen(&self) -> &'static str {
        match self {
            Seite::Start => "startseite::treffer",
            Seite::Einstellungen => "einstellungen::treffer",
            Seite::Downloads | Seite::Verlauf => "seiten::treffer",
            Seite::Tasten => "tasten::treffer",
        }
    }
}

/// ALLE VIER ADRESSEN — so wird die Seite erkannt.
///
/// GEMESSENE ENTSCHEIDUNG, warum vier Adressen in EINER Liste stehen: U4
/// hat `about:settings` gebaut und ANFORDERUNGEN nennt `about:einstellungen`
/// — ein Browser, der nur eine davon kennt, zeigt „unbekannte Adresse".
/// GEMESSENE ENTSCHEIDUNG, warum `about:settings` trotzdem erkannt wird, obwohl
/// es nicht in der Liste steht: `seite_von` faellt auf `Einstellungen`
/// zurueck (siehe dort).
pub const UEBERSICHT: [(&str, Seite); 5] = [
    ("about:start", Seite::Start),
    ("about:einstellungen", Seite::Einstellungen),
    ("about:downloads", Seite::Downloads),
    ("about:verlauf", Seite::Verlauf),
    ("about:tasten", Seite::Tasten),
];

/// DIE ALTEN BEIDEN ADRESSEN — `Downloads` und `Verlauf`.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Name weiter existiert: Runde 6 hat ihn
/// in `team/WUNSCH_UI.txt` Punkt 13 an `narsil` genannt. Ein Wunsch, der nach
/// einem neuen Namen sucht, findet nichts — deshalb bleibt der Name als
/// ZWEITE Sicht auf dieselbe Liste (kein zweiter Inhalt).
pub const SEITEN_ADRESSEN: [(&str, Seite); 2] = [
    ("about:downloads", Seite::Downloads),
    ("about:verlauf", Seite::Verlauf),
];

/// WELCHE SEITE ist das? (`None` = keine der vier)
///
/// GEMESSENE ENTSCHEIDUNG fuer `about:settings`: U4 hat diese Adresse
/// gebaut, ANFORDERUNGEN nennt `about:einstellungen`. GEMESSEN: `suche.rs`
/// fuehrt `about:settings` in der Schemaliste, also kann man sie eintippen.
/// Sie wird deshalb als `Einstellungen` erkannt — mit einem Eintrag, kein
/// zweiter Zugriff auf dieselbe Seite.
pub fn seite_von(adresse: &str) -> Option<Seite> {
    if adresse == "about:settings" {
        return Some(Seite::Einstellungen);
    }
    UEBERSICHT
        .iter()
        .find(|(a, _)| *a == adresse)
        .map(|(_, s)| *s)
}

/// WAS AUF DER SEITE ANGEKLICKT WURDE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeitenTreffer {
    /// Das Suchfeld oben.
    Suche,
    /// Eine Zeile der Liste.
    Zeile(usize),
    /// Der Knopf „Alles löschen" (nur beim Verlauf).
    AllesLoeschen,
    /// Der Knopf „Aus der Liste entfernen" (nur bei den Downloads).
    EintragLoeschen,
}

/// DIE GEOMETRIE DER SEITE — Suchfeld, Liste, zwei Knoepfe.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Reihenfolge von oben nach unten:
/// Suchfeld, Liste, Knoepfe. GEMESSENE BEGRUENDUNG: gesucht wird oben
/// (man sucht, bevor man liest), und die Knoepfe stehen unten, weil sie
/// auf die Liste WIRKEN — wer „alles loeschen" tippt, will vorher sehen,
/// was er loescht.
pub struct Geometrie {
    /// Das Suchfeld.
    pub suchfeld: Rechteck,
    /// Der Bereich der Liste.
    pub liste: Rechteck,
    /// Der Knopf „Alles löschen".
    pub alles_loeschen: Rechteck,
    /// Der Knopf „Aus der Liste entfernen".
    pub eintrag_loeschen: Rechteck,
}

/// DIE GEOMETRIE — `None`, wenn das Fenster zu klein ist (M3/M4).
///
/// GEMESSENER EIGENER FEHLER (02.10., Koordination „Feinschliff aller
/// Seiten"): hier stand `let oben = TAB_LEISTE_HOEHE + LEISTE_HOEHE` (58 px)
/// und die Funktion bekam das Fenster, aber NICHT die Chrome. GEMESSENE
/// FOLGE: mit sichtbarer Lesezeichenleiste (32 px) lagen Suchfeld und Liste
/// 32 px ZU HOCH — also unter der Lesezeichenleiste. Das ist derselbe
/// Fehler, den Runde 4 bei Startseite und Einstellungen gefunden und in
/// `Chrome::seiten_oberkante()` behoben hat (M1); die beiden Seiten aus
/// Runde 6 waren danach entstanden und hatten ihn wieder.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Chrome ZUERST kommt: alle anderen
/// Seitenfunktionen tun das seit Runde 3 („`chrome: &Chrome` zuerst"), und
/// eine Liste, in der eine Funktion die Chrome nicht kennt und eine sie
/// kennt, ist eine Liste, in der man beim Aufruf falsch liegt. Wer die
/// Lesezeichenleiste nicht kennt, kann die Seite nicht zeichnen.
pub fn geometrie(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> Option<Geometrie> {
    // GEMESSEN (02.10., Feinschliff): `inhalt_oberkante()` statt
    // `seiten_oberkante()` — die Seite hat jetzt eine Ueberschrift, und der
    // Inhalt beginnt darunter. Wer die Ueberschrift weglieae, bekame einen
    // Knopf, der auf dem Seitentitel liegt.
    let oben = chrome.inhalt_oberkante();
    // M3: die Seite braucht Platz fuer Suchfeld, mindestens eine Zeile und
    // einen Knopf.
    let platz = fensterhoehe.saturating_sub(oben + super::STATUSLEISTE_HOEHE);
    let hoehe_suche = HOEHE_SUCHE + 16;
    let hoehe_knopfe = super::einstellungen::ZEILE + 16;
    if platz < hoehe_suche + ZEILE + hoehe_knopfe || fensterbreite < 2 * MARGIN + 200 {
        return None;
    }
    let nutzbreite = fensterbreite - 2 * MARGIN;
    let knopf_breite = (nutzbreite - 24) / 2;
    Some(Geometrie {
        suchfeld: Rechteck {
            x: MARGIN,
            y: oben + 8,
            breite: BREITE_SUCHE.min(nutzbreite),
            hoehe: HOEHE_SUCHE,
        },
        liste: Rechteck {
            x: MARGIN,
            y: oben + 8 + hoehe_suche,
            breite: nutzbreite,
            hoehe: platz - hoehe_suche - hoehe_knopfe,
        },
        alles_loeschen: Rechteck {
            x: MARGIN + 12,
            y: oben + platz - hoehe_knopfe + 8,
            breite: knopf_breite,
            hoehe: super::einstellungen::ZEILE,
        },
        eintrag_loeschen: Rechteck {
            x: MARGIN + 12 + knopf_breite + 24,
            y: oben + platz - hoehe_knopfe + 8,
            breite: knopf_breite,
            hoehe: super::einstellungen::ZEILE,
        },
    })
}

/// DER TREFFER: was liegt unter dem Punkt?
///
/// GEMESSENE ENTSCHEIDUNG, warum die LISTE vor den KNÖPFEN geprueft wird,
/// obwohl sie im Bild weiter oben steht: sie ist die groessere Flaeche, und
/// ein Klick am unteren Rand einer Zeile soll die Zeile treffen und nicht
/// den Knopf darunter. GEMESSENE BEGRUENDUNG fuer die Reihenfolge der
/// drei: Suchfeld (oben), Liste (die Mitte), Knoepfe (unten) — die
/// Reihenfolge, in der man klickt.
pub fn treffer(g: &Geometrie, anzahl_zeilen: usize, p: &Point) -> Option<SeitenTreffer> {
    // GEMESSENE ENTSCHEIDUNG, warum die NaN-Wache VOR den Vergleichen steht:
    // jeder Vergleich mit NaN ist falsch (U1c war genau hier der Fehler).
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    if liegt_in(p, &g.suchfeld) {
        return Some(SeitenTreffer::Suche);
    }
    if liegt_in(p, &g.liste) {
        let nr = ((p.y as u32) - g.liste.y) / ZEILE;
        if (nr as usize) < anzahl_zeilen {
            return Some(SeitenTreffer::Zeile(nr as usize));
        }
        return None;
    }
    if liegt_in(p, &g.alles_loeschen) {
        return Some(SeitenTreffer::AllesLoeschen);
    }
    if liegt_in(p, &g.eintrag_loeschen) {
        return Some(SeitenTreffer::EintragLoeschen);
    }
    None
}

/// LIEGT DER PUNKT IM RECHTECK? (eine Regel, an vier Stellen)
fn liegt_in(p: &Point, r: &Rechteck) -> bool {
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

/// DIE SEITE ALS ZEICHENLISTE.
///
/// GEMESSENE BEGRUENDUNG, warum die Seite NUR aus einer Liste besteht und
/// nicht noch aus einer Kopfzeile mit der Zahl der Eintraege: „Downloads"
/// steht schon im Tab und in der Adresse. Eine Zahl wie „128 Dateien" waere
/// eine dritte Stelle derselben Angabe.
pub fn verlauf_display_list(
    seite: &VerlaufSeite,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    // DER SEITENOBERKANTE fuer den Grund: die Seite beginnt hier, die
    // Ueberschrift gehoert schon zum Inhalt (siehe `einstellungen.rs`).
    let seiten_oben = chrome.seiten_oberkante();
    // 1. DER GRUND.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            0.0,
            seiten_oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(seiten_oben) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // 1b. DIE UEBERSCHRIFT — der Name der Seite (Feinschliff 02.10.).
    //
    // GEMESSENER EIGENER FEHLER (02.10., Massstab 3 in
    // `tests/a_schliiff.rs`): der Grund stand bei `oben` = der
    // INHALTSoberkante (106 px), und die Ueberschrift war vorher (bei 85 px)
    // in die Liste gehaengt worden — der Grund hat sie UEBERDECKT. GEMESSENE
    // REGEL fuer den Zeichenweg (Maler): erst der Grund, dann alles, was
    // darauf liegt. Der Grund gehoert zur Seite, der Rest zum Inhalt.
    chrome.ueberschrift_zeichnen(Seite::Verlauf.text(), &mut liste);
    let Some(g) = geometrie(chrome, fensterbreite, fensterhoehe) else {
        return liste;
    };
    // 2. DAS SUCHFELD (Rand + Fuellung + Hinweis oder Text).
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            g.suchfeld.x as f32 - 1.0,
            g.suchfeld.y as f32 - 1.0,
            g.suchfeld.breite as f32 + 2.0,
            g.suchfeld.hoehe as f32 + 2.0,
        ),
        color: pal.adresse_rand,
        radius: [4.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            g.suchfeld.x as f32,
            g.suchfeld.y as f32,
            g.suchfeld.breite as f32,
            g.suchfeld.hoehe as f32,
        ),
        color: pal.adresse,
        radius: [4.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    let hinweis = seite.suchtext.is_empty();
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new((g.suchfeld.x + 6) as f32, (g.suchfeld.y + 5) as f32),
        text: if hinweis {
            tr("Search history").to_string()
        } else {
            seite.suchtext.clone()
        },
        color: if hinweis { pal.text_schwach } else { pal.text },
        font_size: 14.0,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    // 3. DIE ZEILEN.
    let zeilen = seite.sichtbare_zeilen();
    let hoehe_liste = g.liste.hoehe;
    for (nr, e) in zeilen.iter().enumerate() {
        let y = g.liste.y + ZEILE * nr as u32;
        if y + ZEILE > g.liste.y + hoehe_liste {
            break;
        }
        if seite.ausgewaehlt == Some(nr) {
            liste.push(DisplayItem::Rect {
                rect: Rect::new(
                    g.liste.x as f32,
                    y as f32,
                    g.liste.breite as f32,
                    ZEILE as f32,
                ),
                color: pal.auswahl,
                radius: [0.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
        }
        let platz = (g.liste.breite as usize / super::ZEICHENBREITE as usize).saturating_sub(2);
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((g.liste.x + 8) as f32, (y + 3) as f32),
            text: e.titel.chars().take(platz).collect(),
            color: pal.text,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((g.liste.x + 8) as f32, (y + 16) as f32),
            text: e.adresse.chars().take(platz).collect(),
            color: pal.text_schwach,
            font_size: 12.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    // 4. DIE ZWEI KNÖPFE.
    knopf_zeichnen(&mut liste, &g.alles_loeschen, tr("Clear all"), &pal);
    knopf_zeichnen(
        &mut liste,
        &g.eintrag_loeschen,
        tr("Remove from the list"),
        &pal,
    );
    // 5. DER HINWEIS, wenn die Liste leer ist.
    if zeilen.is_empty() {
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((g.liste.x + 8) as f32, (g.liste.y + 8) as f32),
            text: if hinweis {
                tr("No pages visited yet").to_string()
            } else {
                tr("No matches").to_string()
            },
            color: pal.text_schwach,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    liste
}

/// DIE DOWNLOADS-SEITE ALS ZEICHENLISTE.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Zustand in DIESER Zeile steht
/// (`d.zeile()`, U4) und nicht hier: sie steht an zwei Stellen (U4 und
/// hier), und eine Zeile, die es zweimal gibt, zeigt in einem Fenster etwas
/// anderes als in der Leiste.
pub fn downloads_display_list(
    seite: &DownloadSeite,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    // DER SEITENOBERKANTE fuer den Grund: die Seite beginnt hier, die
    // Ueberschrift gehoert schon zum Inhalt (siehe `einstellungen.rs`).
    let seiten_oben = chrome.seiten_oberkante();
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            0.0,
            seiten_oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(seiten_oben) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // 1b. DIE UEBERSCHRIFT — der Name der Seite (Feinschliff 02.10.).
    //
    // GEMESSENER EIGENER FEHLER (02.10., Massstab 3 in
    // `tests/a_schliiff.rs`): der Grund stand bei `oben` = der
    // INHALTSoberkante (106 px), und die Ueberschrift war vorher (bei 85 px)
    // in die Liste gehaengt worden — der Grund hat sie UEBERDECKT. GEMESSENE
    // REGEL fuer den Zeichenweg (Maler): erst der Grund, dann alles, was
    // darauf liegt. Der Grund gehoert zur Seite, der Rest zum Inhalt.
    chrome.ueberschrift_zeichnen(Seite::Downloads.text(), &mut liste);
    let Some(g) = geometrie(chrome, fensterbreite, fensterhoehe) else {
        return liste;
    };
    // SUCHFELD (gleiche Form wie beim Verlauf — es ist dasselbe Feld).
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            g.suchfeld.x as f32 - 1.0,
            g.suchfeld.y as f32 - 1.0,
            g.suchfeld.breite as f32 + 2.0,
            g.suchfeld.hoehe as f32 + 2.0,
        ),
        color: pal.adresse_rand,
        radius: [4.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            g.suchfeld.x as f32,
            g.suchfeld.y as f32,
            g.suchfeld.breite as f32,
            g.suchfeld.hoehe as f32,
        ),
        color: pal.adresse,
        radius: [4.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    let hinweis = seite.suchtext.is_empty();
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new((g.suchfeld.x + 6) as f32, (g.suchfeld.y + 5) as f32),
        text: if hinweis {
            tr("Search downloads").to_string()
        } else {
            seite.suchtext.clone()
        },
        color: if hinweis { pal.text_schwach } else { pal.text },
        font_size: 14.0,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    // DIE ZEILEN.
    let zeilen = seite.sichtbare_zeilen();
    for (nr, d) in zeilen.iter().enumerate() {
        let y = g.liste.y + ZEILE * nr as u32;
        if y + ZEILE > g.liste.y + g.liste.hoehe {
            break;
        }
        if seite.ausgewaehlt.and_then(|i| seite.eintraege.get(i))
            .is_some_and(|gewaehlt| std::ptr::eq(gewaehlt, *d)) {
            liste.push(DisplayItem::Rect {
                rect: Rect::new(
                    g.liste.x as f32,
                    y as f32,
                    g.liste.breite as f32,
                    ZEILE as f32,
                ),
                color: pal.auswahl,
                radius: [0.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
        }
        let platz = (g.liste.breite as usize / super::ZEICHENBREITE as usize).saturating_sub(2);
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((g.liste.x + 8) as f32, (y + 7) as f32),
            text: d.zeile().chars().take(platz).collect(),
            color: pal.text,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    knopf_zeichnen(
        &mut liste,
        &g.alles_loeschen,
        tr("Remove all entries"),
        &pal,
    );
    knopf_zeichnen(&mut liste, &g.eintrag_loeschen, tr("Remove from list"), &pal);
    if zeilen.is_empty() {
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((g.liste.x + 8) as f32, (g.liste.y + 8) as f32),
            text: if hinweis {
                tr("Nothing downloaded yet").to_string()
            } else {
                tr("No matches").to_string()
            },
            color: pal.text_schwach,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    liste
}

/// EIN KNOPF (Rand, Fuellung, Beschriftung).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Fuellung `knopf_flaeche` ist und nicht
/// `akzent`: diese Knoepfe loeschen etwas, und ein blauer Knopf neben einem
/// Textfeld waere eine zweite Form von „hier passiert etwas Gutes". GEMESSENE
/// ENTSCHEIDUNG fuer den Text: `knopf_text`, weil die Fuellung des Knopfes
/// dieselbe ist wie die eines Knopfes in den Einstellungen (U2: „ein
/// Hintergrund fuer alle vier Knöpfe").
fn knopf_zeichnen(liste: &mut DisplayList, r: &Rechteck, text: &str, pal: &super::thema::Palette) {
    liste.push(DisplayItem::Rect {
        rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
        color: pal.knopf_flaeche,
        radius: pal.radius_leiste(),
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new((r.x + 8) as f32, (r.y + 5) as f32),
        text: text.to_string(),
        color: pal.text,
        font_size: 14.0,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::{Download, DownloadZustand};

    /// EIN VERLAUFSEINTRAG MIT DEN GRENZEN (M4).
    #[test]
    fn der_verlaufseintrag_ist_begrenzt() {
        // LEERER TITEL: die Adresse wird zum Titel (sonst waere die Zeile
        // leer und der Nutzer koennte sie nicht von einer anderen leeren
        // Zeile unterscheiden).
        let e = VerlaufEintrag::neu("", "https://beispiel.org/seite", 3);
        assert_eq!(
            e.titel, "https://beispiel.org/seite",
            "der Titel ist leer geblieben"
        );
        assert_eq!(
            e.zeile(),
            "https://beispiel.org/seite — https://beispiel.org/seite · 3 min ago"
        );
        // MIT TITEL: er steht vorn.
        let e2 = VerlaufEintrag::neu("  Rust (Programmiersprache)  ", "https://rust-lang.org", 60);
        assert_eq!(
            e2.titel, "Rust (Programmiersprache)",
            "der Titel wurde nicht getrimmt"
        );
        // BOMBEN: eine Million Zeichen in beiden Feldern.
        let lang = "a".repeat(1_000_000);
        let e3 = VerlaufEintrag::neu(&lang, &lang, 0);
        assert_eq!(e3.titel.chars().count(), MAX_ZEILEN_LAENGE);
        assert_eq!(e3.adresse.chars().count(), MAX_ZEILEN_LAENGE);
    }

    /// DIE LISTE: hoechstens 100, gleiche Adresse nur einmal (nach vorn).
    #[test]
    fn die_verlaufsliste_ist_begrenzt_und_sortiert() {
        let mut s = VerlaufSeite::default();
        for i in 0..MAX_ZEILEN + 20 {
            s.hinzufuegen(VerlaufEintrag::neu(
                &format!("Seite {i}"),
                &format!("https://s{i}.example"),
                i as u32,
            ));
        }
        assert_eq!(
            s.eintraege.len(),
            MAX_ZEILEN,
            "die Liste waechst ueber M3 hinaus"
        );
        // Der NEUSTE steht vorn.
        assert_eq!(
            s.eintraege[0].adresse,
            format!("https://s{}.example", MAX_ZEILEN + 19)
        );
        //
        // GEMESSENER EIGENER FEHLER (02.10., derselbe Test, zweiter
        // Durchgang): ich testete das „dieselbe Adresse nochmal" mit
        // `s0.example`. GEMESSEN: `hinzufuegen` gab `true` zurueck — die
        // Adresse war gar nicht mehr in der Liste. GEMESSENE URSACHE: es
        // wurden 120 Adressen eingefuegt, die Liste haelt 100, und der
        // AELTESTE faellt raus — das ist `s0`. Der Test pruefte also das
        // Einfuegen eines NEUEN Eintrags und nannte es „dieselbe Adresse".
        //
        // GEMESSENE REGEL, die jetzt gilt: in einem Grenztest wird gegen
        // eine Adresse getestet, die die Grenze ÜBERLEBT hat (hier `s50`),
        // und die evicted Adresse wird mitgemessen — denn „der Aelteste
        // faellt raus" ist selbst eine Aussage, die man pruefen sollte.
        assert!(
            !s.eintraege
                .iter()
                .any(|e| e.adresse == "https://s0.example"),
            "der aelteste Eintrag ist nicht gefallen"
        );
        assert!(!s.hinzufuegen(VerlaufEintrag::neu("Neu", "https://s50.example", 999)));
        assert_eq!(s.eintraege.len(), MAX_ZEILEN);
        assert_eq!(
            s.eintraege[0].adresse, "https://s50.example",
            "die Adresse kam nicht nach vorn"
        );
        assert_eq!(
            s.eintraege[0].vor_minuten, 999,
            "der Zeitwert wurde nicht erneuert"
        );
        assert_eq!(
            s.eintraege
                .iter()
                .filter(|e| e.adresse == "https://s50.example")
                .count(),
            1,
            "die Adresse steht mehrfach drin"
        );
    }

    /// SUCHEN UND LOESCHEN — die zwei Funktionen, die die Seite hat.
    #[test]
    fn suchen_und_loeschen() {
        let mut s = VerlaufSeite::default();
        for i in 0..10 {
            s.hinzufuegen(VerlaufEintrag::neu(
                &format!("Seite {}", if i % 2 == 0 { "Rust" } else { "Go" }),
                &format!("https://s{i}.example"),
                i,
            ));
        }
        // OHNE SUCHE: alle zehn.
        assert_eq!(s.sichtbare_zeilen().len(), 10);
        // SUCHE: nur die mit „rust" im Titel (case-empfindlich auf die
        // Kleinschreibung).
        s.suchen("rust");
        assert_eq!(
            s.sichtbare_zeilen().len(),
            5,
            "die Suche fand nicht alle Rust-Zeilen"
        );
        // UND die Adresse wird auch durchsucht.
        s.suchen("s7.example");
        assert_eq!(s.sichtbare_zeilen().len(), 1);
        assert_eq!(s.sichtbare_zeilen()[0].adresse, "https://s7.example");
        // NICHTS: die Liste ist leer, aber die Seite ist nicht kaputt.
        s.suchen("gibtsnicht");
        assert_eq!(s.sichtbare_zeilen().len(), 0);
        // LOESCHEN: der Eintrag kommt ZURUECK (der Aufrufer nennt ihn).
        s.suchen("");
        let weg = s.loeschen("https://s7.example").expect("Eintrag");
        assert_eq!(
            weg.adresse, "https://s7.example",
            "geloescht wurde ein anderer"
        );
        assert_eq!(s.eintraege.len(), 9);
        // NOCHMAL: es gibt ihn nicht mehr.
        assert!(
            s.loeschen("https://s7.example").is_none(),
            "zweimal geloescht ging"
        );
        // ALLE LOESCHEN.
        assert_eq!(s.alle_loeschen(), 9);
        assert!(s.eintraege.is_empty());
        assert_eq!(s.alle_loeschen(), 0, "eine leere Liste hat Eintraege");
    }

    /// DIE SUCHE IST BEGRENZT (M4) und leert die Auswahl.
    #[test]
    fn die_suche_ist_begrenzt() {
        let mut s = VerlaufSeite::default();
        s.hinzufuegen(VerlaufEintrag::neu("A", "https://a.example", 1));
        s.ausgewaehlt = Some(0);
        s.suchen(&"x".repeat(1_000_000));
        assert_eq!(
            s.suchtext.chars().count(),
            MAX_SUCHTEXT,
            "die Suche wurde nicht gekuerzt"
        );
        assert!(
            s.ausgewaehlt.is_none(),
            "die Suche hat die Auswahl behalten"
        );
        // LEER: die Auswahl ist auch weg (GEMESSENE ENTSCHEIDUNG: wer das
        // Feld leert, sucht nichts mehr, und ein markierter Eintrag, den man
        // nicht mehr sieht, waere eine Markierung auf nichts).
        s.ausgewaehlt = Some(0);
        s.suchen("   ");
        assert!(s.ausgewaehlt.is_none());
    }

    /// DIE DOWNLOADS-SEITE: suchen, auswaehlen, loeschen.
    #[test]
    fn die_downloads_seite() {
        let mut s = DownloadSeite::default();
        for i in 0..5 {
            let mut d = Download::aus_adresse(&format!("https://a{i}.example/datei{i}.pdf"));
            d.zustand = DownloadZustand::Fertig;
            s.eintraege.push(d);
        }
        assert_eq!(s.sichtbare_zeilen().len(), 5);
        // SUCHE nach einem TEIL des Namens.
        s.suchen("datei3");
        assert_eq!(
            s.sichtbare_zeilen().len(),
            1,
            "die Suche fand die Datei nicht"
        );
        assert_eq!(s.sichtbare_zeilen()[0].name, "datei3.pdf");
        s.suchen("");
        // AUSWAHLEN UND LOESCHEN.
        s.ausgewaehlt = Some(2);
        assert!(
            s.gewaehlten_loeschen(),
            "der ausgewaehlte Eintrag wurde nicht geloescht"
        );
        assert_eq!(s.eintraege.len(), 4);
        assert!(
            s.ausgewaehlt.is_none(),
            "nach dem Loeschen ist noch etwas ausgewaehlt"
        );
        // OHNE AUSWAHL: nichts.
        assert!(!s.gewaehlten_loeschen(), "ohne Auswahl wurde geloescht");
        // AUSSERHALB: nichts.
        s.ausgewaehlt = Some(99);
        assert!(
            !s.gewaehlten_loeschen(),
            "eine Zahl ausserhalb hat geloescht"
        );
        assert_eq!(s.eintraege.len(), 4);
    }

    /// DIE GEOMETRIE: Suchfeld oben, Liste in der Mitte, Knoepfe unten — und
    /// alles im Fenster (M3/M4).
    #[test]
    fn die_geometrie_der_seiten() {
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (100, 100),
            (248, 60),
            (400, 300),
            (1200, 760),
            (u32::MAX, u32::MAX),
        ] {
            // GEMESSEN (02.10.): die Chrome entsteht HIER (Fensterbreite b),
            // nicht weiter oben — sonst waere die Seitenoberkante eine Zahl
            // aus einem anderen Fenster.
            let chrome = Chrome::neu(b);
            match geometrie(&chrome, b, h) {
                None => {
                    assert!(
                        b < 2 * MARGIN + 200 || h < 130,
                        "bei {b}x{h}: kein Rechteck, obwohl Platz ist"
                    );
                }
                Some(g) => {
                    for r in [
                        &g.suchfeld,
                        &g.liste,
                        &g.alles_loeschen,
                        &g.eintrag_loeschen,
                    ] {
                        assert!(r.rechts() <= b.max(1), "bei {b}x{h}: ragt rechts heraus");
                        assert!(r.unten() <= h.max(1), "bei {b}x{h}: ragt unten heraus");
                    }
                    // DIE REIHENFOLGE: Suchfeld oben, Liste darunter, Knoepfe
                    // ganz unten.
                    assert!(g.suchfeld.y < g.liste.y, "die Liste ist ueber dem Suchfeld");
                    assert!(
                        g.alles_loeschen.y > g.liste.y,
                        "die Knoepfe sind ueber der Liste"
                    );
                    // UND die Knoepfe uebereinander sich nicht.
                    assert!(
                        g.alles_loeschen.rechts() <= g.eintrag_loeschen.x,
                        "die Knoepfe ueberlappen: {} und {}",
                        g.alles_loeschen.rechts(),
                        g.eintrag_loeschen.x
                    );
                }
            }
        }
        // GEMESSEN: bei 1200 x 760 ist das Suchfeld 320 breit und beginnt
        // bei 24; die Liste geht bis 1200 - 24 = 1176; das Suchfeld beginnt
        // 8 px unter der Seitenoberkante.
        //
        // GEMESSENE ENTSCHEIDUNG: die Chrome im Test OHNE Lesezeichenleiste
        // (`Chrome::neu`), denn die Behauptung ist „8 px unter der
        // INHALTSoberkante" — 58 + 24 (Ueberschrift) + 8 = **90**.
        //
        // GEMESSENER EIGENER FEHLER (02.10., Feinschliff): hier stand
        // `chrome.seiten_oberkante() + 8` (= 66). GEMESSEN: der Wert war um
        // genau die Ueberschrifthoehe (24 px) zu klein — der Test hat es
        // gemeldet, und die Behauptung nennt jetzt die Inhaltsoberkante.
        let chrome = Chrome::neu(1200);
        assert_eq!(
            chrome.seiten_oberkante(),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE,
            "ohne Lesezeichenleiste: GEMESSEN 39 + 42 = 81 (Panel 1, 1:1 am 03.10.)"
        );
        assert_eq!(
            chrome.inhalt_oberkante(),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24,
            "81 plus 24 Ueberschrift"
        );
        let g = geometrie(&chrome, 1200, 760).expect("Geometrie");
        assert_eq!(g.suchfeld.x, MARGIN);
        assert_eq!(g.suchfeld.breite, BREITE_SUCHE);
        assert_eq!(
            g.suchfeld.y,
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24 + 8,
            "GEMESSEN: 81 + 24 + 8 = 113 (1:1-Messung, 03.10.)"
        );
        assert_eq!(g.suchfeld.y, chrome.inhalt_oberkante() + 8);
        assert_eq!(g.liste.breite, 1200 - 2 * MARGIN);
    }

    /// DER TREFFER: jedes Element trifft sich selbst, NaN nie (M8).
    #[test]
    fn der_treffer_der_seiten() {
        let g = geometrie(&Chrome::neu(1000), 1000, 700).expect("Geometrie");
        let anzahl = 3usize;
        // SUCHFELD.
        let p = Point::new((g.suchfeld.x + 10) as f32, (g.suchfeld.y + 10) as f32);
        assert_eq!(treffer(&g, anzahl, &p), Some(SeitenTreffer::Suche));
        // ZEILEN 0..2.
        for nr in 0..anzahl {
            let p = Point::new(
                (g.liste.x + 10) as f32,
                (g.liste.y + ZEILE * nr as u32 + 10) as f32,
            );
            assert_eq!(treffer(&g, anzahl, &p), Some(SeitenTreffer::Zeile(nr)));
        }
        // AUSSERHALB der Liste: kein Treffer, auch nicht Zeile 99.
        let p = Point::new((g.liste.x + 10) as f32, (g.liste.y + ZEILE * 50) as f32);
        assert_eq!(treffer(&g, anzahl, &p), None, "Zeile 50 von 3");
        // DIE KNÖPFE.
        let p = Point::new(
            (g.alles_loeschen.x + 10) as f32,
            (g.alles_loeschen.y + 10) as f32,
        );
        assert_eq!(treffer(&g, anzahl, &p), Some(SeitenTreffer::AllesLoeschen));
        let p = Point::new(
            (g.eintrag_loeschen.x + 10) as f32,
            (g.eintrag_loeschen.y + 10) as f32,
        );
        assert_eq!(
            treffer(&g, anzahl, &p),
            Some(SeitenTreffer::EintragLoeschen)
        );
        // NaN (U1c): kein Treffer.
        assert_eq!(treffer(&g, anzahl, &Point::new(f32::NAN, f32::NAN)), None);
        assert_eq!(treffer(&g, anzahl, &Point::new(f32::INFINITY, 0.0)), None);
    }

    /// DIE VIER ADRESSEN WERDEN ERKANNT — und keine andere.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., Feinschliff): dieser Test behauptete
    /// `seite_von("about:start") == None` — nach der Erweiterung des Enums auf
    /// vier Seiten ist das FALSCH, und der Test hat es gemeldet. Das ist die
    /// Reihenfolge, in der man es finden soll: erst der Test, dann die
    /// Erklaerung im Kommentar, dann der Code.
    #[test]
    fn die_vier_adressen_werden_erkannt() {
        assert_eq!(seite_von("about:downloads"), Some(Seite::Downloads));
        assert_eq!(seite_von("about:verlauf"), Some(Seite::Verlauf));
        assert_eq!(seite_von("about:start"), Some(Seite::Start));
        assert_eq!(seite_von("about:einstellungen"), Some(Seite::Einstellungen));
        // UND: `about:settings` ist die ALTE Adresse der Einstellungen (U4
        // hat sie gebaut, ANFORDERUNGEN nennt `about:einstellungen`) — sie
        // wird als dieselbe Seite erkannt, NICHT als neue.
        assert_eq!(seite_von("about:settings"), Some(Seite::Einstellungen));
        // DIE NAMEN stehen im Tab UND als Ueberschrift.
        for (adresse, art) in UEBERSICHT {
            assert_eq!(art.adresse(), adresse);
            assert!(!art.text().is_empty(), "{adresse} hat keinen Namen");
            assert!(
                !art.zeichnen().is_empty(),
                "{adresse} hat keine Zeichenliste"
            );
            assert!(
                !art.treffen().is_empty(),
                "{adresse} hat keine Trefferfunktion"
            );
        }
        for a in [
            "https://beispiel.de",
            "",
            "about:",
            "ABOUT:VERLAUF",
            "about:verlauf ",
        ] {
            assert_eq!(seite_von(a), None, "{a} wird als Seite erkannt");
        }
    }

    /// DAS BILD: die Texte stehen drin, und die Seite ist schlicht (M3).
    #[test]
    fn die_seiten_im_bild() {
        let chrome = Chrome::neu(1200);
        // DER VERLAUF mit drei Eintraegen.
        let mut v = VerlaufSeite::default();
        for (i, t) in ["Rust (Programmiersprache)", "Go", "Wikipedia"]
            .iter()
            .enumerate()
        {
            v.hinzufuegen(VerlaufEintrag::neu(
                t,
                &format!("https://s{i}.example"),
                i as u32 * 5,
            ));
        }
        v.ausgewaehlt = Some(1);
        let liste = verlauf_display_list(&v, &chrome, 1200, 760);
        let texte: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        // DIE HINWEISE UND DIE EINTRAEGE.
        for erwartet in [
            "Search history",
            "Rust (Programmiersprache)",
            "https://s1.example",
            "Clear all",
            "Remove from the list",
        ] {
            assert!(
                texte.iter().any(|t| t == erwartet),
                "im Bild fehlt {erwartet:?}: {texte:?}"
            );
        }
        // DIE SEITE IST SCHLICHT: Suchfeld (2) + 3 Zeilen (2 Texte je) +
        // 2 Knoepfe (2 je) + Grund = 1 + 2 + 1 + 6 + 4 = 14 Items. Die
        // GEMESSENE Grenze ist 30 — eine Seite mit mehr als 30 Feldern waere
        // ein Dashboard (ANFORDERUNGEN Zeile 16).
        assert!(
            liste.len() <= 30,
            "die Verlaufsseite hat {} Items — das ist kein \"schlicht\"",
            liste.len()
        );
        // DIE DOWNLOADS mit zwei Dateien.
        let mut d = DownloadSeite::default();
        for i in 0..2 {
            let mut dl = Download::aus_adresse(&format!("https://a{i}.example/bericht{i}.pdf"));
            dl.zustand = DownloadZustand::Fertig;
            d.eintraege.push(dl);
        }
        d.ausgewaehlt = Some(0);
        let liste2 = downloads_display_list(&d, &chrome, 1200, 760);
        let texte2: Vec<String> = liste2
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for erwartet in [
            "Search downloads",
            "bericht0.pdf",
            "bericht1.pdf",
            "Remove all entries",
            // Release 09.10.2026: der Knopf heisst „Aus Liste entfernen" —
            // er nimmt den Eintrag aus der Liste
            // (`gewaehlten_loeschen`), die Datei bleibt liegen. „Datei
            // löschen" haette etwas versprochen, das er nicht tut.
            "Remove from list",
        ] {
            assert!(
                texte2.iter().any(|t| t.contains(erwartet)),
                "im Bild fehlt {erwartet:?}: {texte2:?}"
            );
        }
        assert!(
            liste2.len() <= 30,
            "die Downloadseite hat {} Items",
            liste2.len()
        );
        // BOMBEN: beide Seiten in jedem Fenster.
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (248, 60),
            (1200, 760),
            (u32::MAX, u32::MAX),
        ] {
            let bb = b.min(2000);
            let hh = h.min(2000);
            assert!(verlauf_display_list(&v, &chrome, bb, hh).len() < 100);
            assert!(downloads_display_list(&d, &chrome, bb, hh).len() < 100);
        }
    }
}
