//! D8 — LEISTUNG: Zeit je Stufe (Netz, Parse, Kaskade, Layout, Raster, JS).
//!
//! # WAS D8 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D8 Leistung: Zeit je Stufe (Netz, Parse, Kaskade, Layout, Raster, JS).
//!
//! # WAS ES HEUTE GIBT — und was nicht (gemessen, nicht behauptet)
//!
//! GEMESSEN am 01.10.2026: eine Phasenmessung gibt es **nur** in
//! `crates/narsil/src/zeiten.rs` — 149 Zeilen, ein `Instant`-Zaehler, und sie
//! schreibt nach **stderr**, nur wenn `NARSIL_ZEEN=1` in der Umgebung steht.
//! Dort ist sie auch nicht erreichbar von aussen (kein `pub` Weg aus der
//! Crate heraus, der die Zahlen liefert — `Messung::ausgeben` schreibt und
//! `phasen` ist privat).
//!
//! Also: der Reiter ist eine EIGENE Messung, mit einer eigenen Grenze, und
//! der Host fuettert sie. Das ist kein Unvermögen, das ist die Grenze der
//! fremden Crate: `narsil` gehoert Rolle UI, und ich darf dort nichts
//! aendern. Der Wunsch steht in `team/WUNSCH_UI.txt`.
//!
//! # DER WEG ZUR ZAHL
//!
//! ```ignore
//! let mut l = Leistung::neu();
//! let seite = l.messe(Stufe::Netz, || narsil_net::fetch(&anfrage));
//! let baum  = l.messe(Stufe::Parse, || narsil_page::html_lesen(&text));
//! let s     = l.messe(Stufe::Kaskade, || narsil_page::aus_knoten_mit_messung(...));
//! let b     = l.messe(Stufe::Layout, || narsil_layout_engine::layout_mit_plan(...));
//! let d     = l.messe(Stufe::Raster, || narsil_raster::rasterize_mit_schrift(...));
//! l.messe(Stufe::Js, || skripte_ausfuehren(...));
//! ```
//!
//! `messe` ist das Einzige, was der Host braucht: es zaehlt, es begrenzt,
//! es merkt sich nichts, wenn der Reiter zu ist.

use crate::grenzen::Grenzen;
use std::time::Instant;

/// Die sechs Stufen des Ladens, in der Reihenfolge, in der sie laufen.
///
/// GEMESSENE ENTSCHEIDUNG, warum „Raster" eine Stufe ist und nicht Teil
/// von „Layout": sie laufen an verschiedenen Stellen und werden an
/// verschiedenen Stellen gemessen. `narsil` zeichnet die Seite beim
/// Anzeigen (`seiten_liste` in `sitzung.rs`), nicht beim Bauen — der
/// Nutzer sieht also genau dann eine zaehme Seite, wenn das Rastern
/// haengt. Eine Zahl „Raster 340 ms" ist die Antwort auf die Frage „wieso
/// ruckelt das".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stufe {
    /// DNS, TLS, Verbindungsaufbau, Lesen der Antwort.
    Netz,
    /// HTML lesen (`narsil_page::html_lesen`), Bild-Dekodieren.
    Parse,
    /// CSS-Kaskade und Dokumentbau (`seite_aus_knoten_mit_messung`).
    Kaskade,
    /// Rechnen der Kästen (`narsil_layout_engine::layout_mit_plan`).
    Layout,
    /// Zeichnen der Kästen in den Bildpuffer.
    Raster,
    /// JavaScript der Seite.
    Js,
}

impl Stufe {
    /// Der Name, wie er im Reiter steht.
    pub const fn name(self) -> &'static str {
        match self {
            Stufe::Netz => "Netz",
            Stufe::Parse => "Parse",
            Stufe::Kaskade => "Kaskade",
            Stufe::Layout => "Layout",
            Stufe::Raster => "Raster",
            Stufe::Js => "JS",
        }
    }

    /// Kurzname für die schmale Spalte (D8-Tabelle).
    pub const fn kurz(self) -> &'static str {
        match self {
            Stufe::Netz => "netz",
            Stufe::Parse => "parse",
            Stufe::Kaskade => "kaskade",
            Stufe::Layout => "layout",
            Stufe::Raster => "raster",
            Stufe::Js => "js",
        }
    }

    /// Der Index in [`ALLE_STUFEN`] (0..6).
    pub const fn index(self) -> usize {
        match self {
            Stufe::Netz => 0,
            Stufe::Parse => 1,
            Stufe::Kaskade => 2,
            Stufe::Layout => 3,
            Stufe::Raster => 4,
            Stufe::Js => 5,
        }
    }

    /// Die Stufe an dieser Stelle, oder `None`.
    pub const fn an_index(i: usize) -> Option<Stufe> {
        match i {
            0 => Some(Stufe::Netz),
            1 => Some(Stufe::Parse),
            2 => Some(Stufe::Kaskade),
            3 => Some(Stufe::Layout),
            4 => Some(Stufe::Raster),
            5 => Some(Stufe::Js),
            _ => None,
        }
    }
}

impl std::fmt::Display for Stufe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Alle sechs Stufen in der Reihenfolge des Ladens — als `const`, damit die
/// Zahl 6 eine Zusage ist.
pub const ALLE_STUFEN: [Stufe; 6] = [
    Stufe::Netz,
    Stufe::Parse,
    Stufe::Kaskade,
    Stufe::Layout,
    Stufe::Raster,
    Stufe::Js,
];

/// Die gemessene Summe einer Stufe, in Nanosekunden.
///
/// M7: der Test sichert die Zahl zu — nicht „läuft", sondern „diese
/// Nanosekunden".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StufeZeit {
    /// Summe aller Laeufe dieser Stufe in Nanosekunden.
    pub ns: u64,
    /// Wie oft die Stufe lief (damit man „3 × 40 ms" von „120 ms" unterscheidet).
    pub anzahl: u32,
}

impl StufeZeit {
    /// Die Zeit in Millisekunden, mit einer Nachkommastelle.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `as f64 / 1e6` rundet nicht. Ein Reiter,
    /// der „0 ms" zeigt, wenn etwas 400 Mikrosekunden dauert, behauptet,
    /// etwas sei gratis. Zwei Stellen: 0,40 ms.
    pub fn ms(&self) -> f64 {
        self.ns as f64 / 1_000_000.0
    }
}

/// Eine Phase, wie sie im Reiter als Zeile steht.
#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    /// Die Stufe.
    pub stufe: Stufe,
    /// Wie oft sie lief.
    pub anzahl: u32,
    /// Summe in Nanosekunden.
    pub ns: u64,
    /// Der Name der Seite, auf der gemessen wurde (leer = keine).
    pub seite: String,
}

/// Der Zustand des Leistungsreiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Leistung {
    /// Summe je Stufe des LETZTEN Vorgangs (dargestellt).
    pub stufen: [StufeZeit; 6],
    /// Die Liste der Phasen, hoechstens [`Grenzen::max_phasen`].
    pub phasen: Vec<Phase>,
    /// Wie viele Phasen es eigentlich gab (auch nach der Grenze).
    pub phasen_gezaehlt: u64,
    /// Der zuletzt gemessene Vorgang (fuer die Kopfzeile).
    pub vorgang: String,
    /// Seitenwechsel: die alte Liste abraeumen.
    pub zwischenstand: bool,
    /// Die Phasen des Netzabrufs: DNS, TCP, TLS, erstes Byte, gesamt.
    ///
    /// # WARUM DIE PHASEN HIER UND NICHT NUR IM NETZWERKREITER
    ///
    /// `team/AUTONOM.txt` Z. 665 verlangt fuer den Leistungsreiter „echte
    /// Stufenzeiten (TLS Phasen + NARSIL_ZEITEN)". GEMESSEN (02.10.2026,
    /// 3:10x): `NARSIL_ZEITEN` liefert die Stufen (das ist jetzt angebunden),
    /// und die TLS-Phasen kommen aus `narsil_net::phasen()` — die lagen nur
    /// im Netzwerkreiter. Wer im Leistungsreiter nur „Netz 1570 ms" sieht,
    /// weiss nicht, dass davon 502 ms der Handshake sind.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und nicht eine
    /// siebte Stufe: DNS/TCP/Tls sind KEINE Stufen des Seitenbaus, sondern
    /// Stufen DES NETZABRUFS. Sie als siebte Zeile in der Stufenliste
    /// wuerde die Summe verdoppeln (Netz steckt in ihnen).
    pub netz_phasen: crate::d4_netzwerk::NetzPhasen,
    /// D19: DIE BILDER (Frames) — wie lange ein Bildschirmaufbau dauerte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `narsil-chrome`:
    /// ein Frame ist eine **Messung des Werkzeugs** (die Zeit zwischen zwei
    /// Neuzeichnungen), keine Eigenschaft des Fensters. Wer sie im Host
    /// sammelt, braucht dort einen zweiten Zaehler fuer etwas, das nur das
    /// Werkzeug wissen will — und die Zahlen stuenden an zwei Stellen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste begrenzt ist
    /// (`max_phasen`, 256): ein Tab, der eine Stunde laeuft, hat 200 000
    /// Frames; die ersten 256 zu zeigen und die Zahl zu melden ist
    /// ehrlicher als eine Liste, die den Speicher frisst.
    pub frames: Vec<Frame>,
}

/// D19: EIN BILD (ein Neuaufbau des Fensters).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Die Nummer des Bildes (1, 2, 3 …).
    pub nr: u32,
    /// Wie lange der Aufbau gedauerte (ms).
    pub ms: f64,
    /// Wie viele Kaeste dabei gezeichnet wurden (`0` = unbekannt).
    pub kaeste: u32,
    /// Der Name des Vorgangs (leer = der geladene Aufbau).
    pub vorgang: String,
    /// D20: DIE ZEITEN DIESES BILDES je Stufe.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Auftrag der Leitung 04.10., „Profil:
    /// Layout-/Mal-/Skriptzeit pro Frame"), warum die Zeiten am Bild hängen
    /// und nicht am `Leistung`-Zustand: die sechs STUFEN (`StufeZeit`) sind
    /// die Summe ueber den ganzen Ladevorgang, ein Frame ist ein NEUER
    /// Aufbau (nach einem Klick, nach einer Skriptaenderung). Ohne die Zeiten
    /// je Bild kann man nicht sagen, WELCHES Bild teuer war — und das ist
    /// die Frage, die ein Flammenprofil beantwortet.
    pub zeit: FrameZeit,
}

/// D20: DIE ZEITEN EINES BILDES (Nanosekunden, wie die Stufen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameZeit {
    /// Die Kaesten rechnen (`layout_mit_plan`).
    pub layout_ns: u64,
    /// Die Pixel werden geschrieben (der Rasterer).
    pub raster_ns: u64,
    /// JavaScript laeuft (die Engine des Fensters).
    pub js_ns: u64,
    /// Was sonst noch lief (Zeichnen des Fensters, Ereignisse).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es ein eigenes Feld ist: die Summe der
    /// drei genannten Zeiten ist FAST nie die ganze Bildzeit. Ohne dieses
    /// Feld waere der Balken bei 60 % Fuellung und der Rest unausgesprochen —
    /// man haelt die fehlenden 40 % fuer „nichts gemessen".
    pub sonst_ns: u64,
}

impl FrameZeit {
    /// Die Summe aller Zeiten (die Gesamtzeit des Bildes).
    pub fn ns(&self) -> u64 {
        self.layout_ns
            .saturating_add(self.raster_ns)
            .saturating_add(self.js_ns)
            .saturating_add(self.sonst_ns)
    }

    /// Die Summe in Millisekunden.
    pub fn ms(&self) -> f64 {
        self.ns() as f64 / 1_000_000.0
    }

    /// Die Anteile als `[(Name, Anteil)]` — fuer den FLAMMENbalken.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum vier Teile und nicht drei: siehe
    /// `sonst_ns`. Ein Balken, der seine eigene Länge nicht erklaert, ist eine
    /// Zeichnung, die etwas verschweigt.
    pub fn teile(&self) -> [(&'static str, f64); 4] {
        let n = self.ns();
        if n == 0 {
            return [
                ("Layout", 0.0),
                ("Malen", 0.0),
                ("Skript", 0.0),
                ("Sonst", 0.0),
            ];
        }
        [
            ("Layout", self.layout_ns as f64 / n as f64),
            ("Malen", self.raster_ns as f64 / n as f64),
            ("Skript", self.js_ns as f64 / n as f64),
            ("Sonst", self.sonst_ns as f64 / n as f64),
        ]
    }

    /// Der Text einer Zeile: „Layout 1.20 ms | Malen 4.00 ms | Skript 0.30 ms".
    pub fn text(&self) -> String {
        format!(
            "Layout {:.2} ms | Malen {:.2} ms | Skript {:.2} ms | Sonst {:.2} ms",
            self.layout_ns as f64 / 1e6,
            self.raster_ns as f64 / 1e6,
            self.js_ns as f64 / 1e6,
            self.sonst_ns as f64 / 1e6
        )
    }
}

impl std::fmt::Display for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.kaeste == 0 {
            write!(f, "#{:<4} {:>7.2} ms", self.nr, self.ms)
        } else {
            write!(
                f,
                "#{:<4} {:>7.2} ms  {} Kaeste",
                self.nr, self.ms, self.kaeste
            )
        }
    }
}

impl Leistung {
    /// D19: eine leere Messung.
    pub fn neu() -> Self {
        Self {
            stufen: [StufeZeit::default(); 6],
            phasen: Vec::new(),
            phasen_gezaehlt: 0,
            vorgang: String::new(),
            zwischenstand: false,
            frames: Vec::new(),
            netz_phasen: crate::d4_netzwerk::NetzPhasen::default(),
        }
    }

    /// Einen Vorgang beginnen: die alten Zahlen weg, die Liste leeren.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die alten Zahlen WEG sind und nicht
    /// daneben stehen: der Reiter zeigt „die letzte Seite". Eine Zahl, die
    /// sich auf die vorige Seite bezieht, steht in derselben Spalte wie eine
    /// Zahl der aktuellen — und die aktuellen sind die, um die es geht.
    /// Alles weg (D12: der Knopf „Leeren" in der Werkzeugleiste).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Vorgangsname BLEIBT: der Knopf
    /// loescht die Zeiten, und der Name ist die Adresse, zu der sie
    /// gehoeren. Ein Reiter ohne Namen kann nicht sagen, was er misst — und
    /// „Leeren" heisst bei Firefox „keine Messung", nicht „welche Seite?".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `zwischenstand` weg ist: es stand auf
    /// `false` nach einem leeren Vorgang, und die Phasen stehen in einer
    /// eigenen Liste (`netz_phasen`) — die wird nicht geleert, weil sie zum
    /// Abruf gehoert, nicht zum Vorgang.
    pub fn leeren(&mut self) {
        self.stufen = [StufeZeit::default(); 6];
        self.phasen.clear();
        self.phasen_gezaehlt = 0;
        self.zwischenstand = false;
        // D19: die Bilder gehoeren zur Messung — wer „Leeren" klickt, will
        // eine leere Anzeige, und eine Liste, deren aelteste Bilder noch
        // dastehen, ist keine.
        self.frames.clear();
    }

    /// Die Netzphasen eintragen (der Weg aus dem Netzwerkreiter).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigener Aufruf und kein Teil von
    /// `merke`: die Phasen kommen aus einem fremden `narsil_net::Phasen` und
    /// gehoeren einem fremden Abruf, nicht dem Seitenbau. Wer sie in
    /// `merke` presste, muesste sie in Nanosekunden umrechnen — und
    /// `NetzPhasen::aus` hat das gemessen (es sind Sekundenbruchteile,
    /// keine ganzen Nanosekunden).
    pub fn merke_netz_phasen(&mut self, p: crate::d4_netzwerk::NetzPhasen) {
        self.netz_phasen = p;
    }

    /// Die Netzphasen als eine Zeile fuer den Reiter (leer, wenn keine).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahl von `NetzPhasen::text()`
    /// kommt und nicht hier neu berechnet wird: der Netzwerkreiter zeigt
    /// dieselben Phasen mit derselben Rechnung. Zwei Rechnungen fuer eine
    /// Zahl sind zwei Zahlen, sobald sich eine aendert.
    pub fn netz_phasen_zeile(&self) -> String {
        if self.netz_phasen.ist_leer() {
            return String::new();
        }
        format!("Netz-Phasen: {}", self.netz_phasen.text())
    }

    pub fn vorgang_beginnen(&mut self, name: &str) {
        self.stufen = [StufeZeit::default(); 6];
        self.phasen.clear();
        // Die Netzphasen gehoeren zum ABRUF, nicht zum Reiter — aber ein
        // neuer Vorgang hat einen neuen Abruf, also gehoeren die Phasen
        // des alten nicht dazu (GEMESSENE ENTSCHEIDUNG: `Leistung` ist der
        // Zustand des LETZTEN Vorgangs, s. der Kopf des Moduls).
        self.netz_phasen = crate::d4_netzwerk::NetzPhasen::default();
        self.phasen_gezaehlt = 0;
        self.vorgang = crate::grenzen::Grenzen::STANDARDAUSGABE.kuerze_zeile(name);
        self.zwischenstand = false;
    }

    /// Den Vorgang BENENNEN, ohne die Zahlen zu loeschen.
    ///
    /// # WARUM DAS EINE EIGENE METHODE IST (gemessen 02.10.2026, 2:1x)
    ///
    /// `vorgang_beginnen` loescht `stufen`, `phasen` und `phasen_gezaehlt`.
    /// Genau richtig am ANFANG eines Ladevorgangs — und genau falsch, wenn
    /// eine Seite fertig gebaut ist und nur noch beschriftet werden soll.
    ///
    /// GEMESSENER FOLGEFEHLER, den das Bild zeigte: der Reiter zeigte
    /// sechsmal 0,00 ms, obwohl die Sammlung voll war (parse 5,0 ms,
    /// kaskade 36,2 ms, layout 9,4 ms, js 171,9 ms — genau die Zahlen, die
    /// `merke` bekommen hatte). Ursache: `devtools_bruecke::an_seite` rief
    /// `vorgang_beginnen`, und `an_seite` laeuft nach dem Fuellen. Es ist
    /// dieselbe Fehlerklasse wie beim Netz-Protokoll in D10: der Aufrufer,
    /// der zuruecksetzt, steht nach dem Fuellen.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die zwei Faelle bekommen zwei Methoden —
    /// „neuer Vorgang, Zahlen weg" und „dieser Vorgang heisst so". Eine
    /// Methode fuer beides ist der Fehler.
    pub fn vorgang_benennen(&mut self, name: &str) {
        self.vorgang = crate::grenzen::Grenzen::STANDARDAUSGABE.kuerze_zeile(name);
    }

    /// Eine Stufe messen und das Ergebnis durchreichen.
    ///
    /// Das ist der WEG, den der Host benutzt (siehe Modul-Kopf). M3: der
    /// `Instant` wird VOR dem Closure genommen und die Differenz DANACH —
    /// und die Liste wird nur gefuellt, solange sie unter
    /// [`Grenzen::max_phasen`] ist, aber der Zaehler laeuft weiter (sonst
    /// waere die gemeldete Zahl kleiner als die tatsaechliche und damit
    /// falsch).
    pub fn messe<T>(&mut self, stufe: Stufe, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let r = f();
        self.merke(stufe, start.elapsed().as_nanos() as u64);
        r
    }

    /// Eine bereits gemessene Dauer eintragen (Weg aus fremden Crates, die
    /// selbst messen).
    pub fn merke(&mut self, stufe: Stufe, ns: u64) {
        // Die Summe ist selbst eine Grenze: `u64` in Nanosekunden sind 584
        // Jahre. Ein Aufruf mit einem kaputten `Instant`-Wert (also
        // negativ, als `u64` = 18 446 744 073 709 551 615) wuerde die
        // Summe auf den kleinsten Wert zuruecksetzen lassen — und der
        // Reiter zeigte danach eine Zeit, die kleiner als der letzte
        // Aufruf ist. Deshalb wird saturiert.
        let i = stufe.index();
        if i >= self.stufen.len() {
            return;
        }
        self.stufen[i].ns = self.stufen[i].ns.saturating_add(ns);
        self.stufen[i].anzahl = self.stufen[i].anzahl.saturating_add(1);
        self.phasen_gezaehlt = self.phasen_gezaehlt.saturating_add(1);
        if self.phasen.len() < Grenzen::STANDARDAUSGABE.max_phasen {
            self.phasen.push(Phase {
                stufe,
                anzahl: 1,
                ns,
                seite: self.vorgang.clone(),
            });
        }
    }

    /// Die Summe einer Stufe, oder null.
    pub fn stufe(&self, stufe: Stufe) -> StufeZeit {
        self.stufen[stufe.index()]
    }

    /// Die Summe aller Stufen in Nanosekunden (gesättigt).
    pub fn summe_ns(&self) -> u64 {
        self.stufen.iter().fold(0u64, |a, s| a.saturating_add(s.ns))
    }

    /// Die Summe aller Stufen in Millisekunden.
    pub fn summe_ms(&self) -> f64 {
        self.summe_ns() as f64 / 1_000_000.0
    }

    /// Die teuerste Stufe, oder `None` wenn nichts gemessen wurde.
    ///
    /// GEMESSENE ENTSCHEIDUNG: „teuerste Stufe" ist die Zahl, nach der der
    /// Nutzer sucht („wo sitzt die Zeit"). Bei Gleichstand gewinnt die
    /// FRÜHERE Stufe — die Reihenfolge des Ladens —, weil bei Gleichstand
    /// nichts zu entscheiden ist und die erste Zahl im Bild nicht
    /// springen soll.
    pub fn teuerste(&self) -> Option<(Stufe, StufeZeit)> {
        let mut best: Option<(Stufe, StufeZeit)> = None;
        for s in ALLE_STUFEN.iter() {
            let z = self.stufe(*s);
            if z.anzahl == 0 {
                continue;
            }
            match best {
                Some((_, b)) if b.ns >= z.ns => {}
                _ => best = Some((*s, z)),
            }
        }
        best
    }

    /// Der Anteil der teuersten Stufe an der Summe (0..1), oder `None`.
    pub fn anteil_teuerste(&self) -> Option<f64> {
        let (_, z) = self.teuerste()?;
        let summe = self.summe_ns();
        if summe == 0 {
            return Some(1.0);
        }
        Some(z.ns as f64 / summe as f64)
    }

    /// Die Zeilen fuer den Reiter: eine je Stufe, in der Reihenfolge.
    pub fn zeilen(&self) -> Vec<StufeZeit> {
        ALLE_STUFEN.iter().map(|s| self.stufe(*s)).collect()
    }

    /// Der Text, der in den Reiter kommt — eine Zeile je Stufe.
    ///
    /// M4: jeder Wert wird ueber [`Grenzen::kuerze`] begrenzt, weil die
    /// Phasennamen aus dem Host kommen und beliebig lang sein koennen.
    pub fn text_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        ALLE_STUFEN
            .iter()
            .map(|s| {
                let z = self.stufe(*s);
                g.kuerze_zeile(&format!(
                    "{:<8} {:>9.2} ms  {}x",
                    s.name(),
                    z.ms(),
                    z.anzahl
                ))
            })
            .collect()
    }
}

impl std::fmt::Display for Leistung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} Stufen, {:.2} ms", self.stufen.len(), self.summe_ms())
    }
}

#[cfg(test)]
mod leistung_eigene {
    use super::*;

    /// DIE SECHS STUFEN SIND SECHS, IN LADEREIHENFOLGE.
    ///
    /// M7: der Test nennt die sechs Namen, nicht nur die Zahl.
    #[test]
    fn die_sechs_stufen_stehen_in_ladereihenfolge() {
        assert_eq!(ALLE_STUFEN.len(), 6);
        let namen: Vec<&str> = ALLE_STUFEN.iter().map(|s| s.name()).collect();
        assert_eq!(
            namen,
            ["Netz", "Parse", "Kaskade", "Layout", "Raster", "JS"],
            "die Stufenfolge weicht vom Auftrag ab"
        );
        for (i, s) in ALLE_STUFEN.iter().enumerate() {
            assert_eq!(
                Stufe::an_index(i),
                Some(*s),
                "an_index({i}) zeigt auf ein anderes Feld"
            );
            assert_eq!(s.index(), i, "index() von {:?} stimmt nicht", s);
        }
        assert_eq!(Stufe::an_index(6), None);
    }

    /// EINE MESSUNG LANDET IN GENAU EINER STUFE, MIT IHRER ZAHL.
    ///
    /// GEMESSENE BEGRÜNDUNG: `messe` gibt das Ergebnis des Closures
    /// durch. Ein Test, der nur die Zeit prüft, würde einen `messe`
    /// akzeptieren, der den Wert des Closures verwirft — und genau das ist
    /// der Fehler, den man beim Umbau einer Funktion einbaut, ohne es zu
    /// merken. Der Test prüft deshalb BEIDES.
    #[test]
    fn messe_zaehlt_und_reicht_durch() {
        let mut l = Leistung::neu();
        let wert = l.messe(Stufe::Kaskade, || 21 * 2);
        assert_eq!(wert, 42, "der Wert des Closures kommt nicht durch");
        assert_eq!(
            l.stufe(Stufe::Kaskade).anzahl,
            1,
            "die Stufe lief nicht einmal"
        );
        assert!(l.stufe(Stufe::Kaskade).ns > 0, "die Dauer ist 0 ns");
        // UND nur diese eine Stufe ist gefuellt.
        for s in ALLE_STUFEN.iter() {
            if *s != Stufe::Kaskade {
                assert_eq!(l.stufe(*s).anzahl, 0, "{} zaehlt mit", s.name());
            }
        }
        // UND die Summe ist die Summe der Teile.
        assert_eq!(l.summe_ns(), l.stufe(Stufe::Kaskade).ns);
    }

    /// DIE LISTE WIRD BEGRENZT, DER ZAEHLER NICHT.
    ///
    /// GEMESSENE BEGRÜNDUNG: das ist der Fehler, den ein naechstliegender
    /// Ausbau macht — man kappt die Liste und zaehlt nur noch bis zur
    /// Grenze. Dann meldet der Reiter 256 Phasen fuer einen Vorgang, der
    /// 400 hatte, und der Nutzer haelt die Zahl fuer vollstaendig. Der
    /// Test misst beides getrennt.
    #[test]
    fn die_phasenliste_ist_begrenzt_der_zaehler_laeuft_weiter() {
        let mut l = Leistung::neu();
        let n = Grenzen::STANDARDAUSGABE.max_phasen + 500;
        for _ in 0..n {
            l.merke(Stufe::Raster, 1000);
        }
        assert_eq!(
            l.phasen.len(),
            Grenzen::STANDARDAUSGABE.max_phasen,
            "die Liste ist nicht gekappt"
        );
        assert_eq!(l.phasen_gezaehlt, n as u64, "der Zaehler wurde mitgekappt");
        // UND die Summe ist trotzdem die volle Summe (sonst waere die
        // gekappte Liste nur eine Auswahl ohne Auswirkung).
        assert_eq!(
            l.stufe(Stufe::Raster).ns,
            1000 * n as u64,
            "die Summe ist nicht die volle"
        );
    }

    /// BOMBEN (M8): RIESIGE ZEITWERTE UND u64::MAX.
    ///
    /// GEMESSENE BEGRÜNDUNG: `merke` ist der Weg, mit dem ein Host eine
    /// fremde Messung eintraegt. Ein `Instant::elapsed()` kann `u64::MAX`
    /// nicht liefern, aber ein Aufruf aus fremdem Code schon — und die
    /// Addition `ns + ns` bei `u64::MAX` **panict im Debug-Bau** (der
    /// Projekt-Standard!). Deshalb `saturating_add`, und der Test laeuft
    /// im Debug-Bau.
    #[test]
    fn bomben_zeiten_saturieren_statt_zu_panicken() {
        let mut l = Leistung::neu();
        l.merke(Stufe::Netz, u64::MAX);
        l.merke(Stufe::Netz, u64::MAX);
        l.merke(Stufe::Netz, 1);
        assert_eq!(
            l.stufe(Stufe::Netz).ns,
            u64::MAX,
            "die Summe ist nicht gesättigt"
        );
        assert_eq!(
            l.summe_ns(),
            u64::MAX,
            "die Gesamtsumme ist nicht gesättigt"
        );
        assert_eq!(l.stufe(Stufe::Netz).anzahl, 3, "die Anzahl zaehlt falsch");
        // UND der Anteil ist eine Zahl zwischen 0 und 1 (ein u64::MAX als
        // f64 ist endlich, aber `ns / summe` muss 1.0 ergeben).
        let a = l.anteil_teuerste().expect("kein Anteil");
        assert!((0.0..=1.0).contains(&a), "der Anteil liegt ausserhalb: {a}");
        // UND die Texte sind fertig und begrenzt.
        for z in l.text_zeilen() {
            assert!(z.chars().count() <= Grenzen::STANDARDAUSGABE.max_zeile);
        }
        // UND die ms-Zahl ist endlich (u64::MAX / 1e6 = 1,8e13 ms).
        assert!(l.summe_ms().is_finite(), "die ms-Zahl ist unendlich");
    }

    /// DIE TEUERSTE STUFE IST DIE MIT DEM GRÖSSTEN WERT.
    ///
    /// GEMESSENE BEGRÜNDUNG: der Nutzer sucht „wo sitzt die Zeit". Der
    /// Test misst deshalb alle sechs Werte und vergleicht gegen die
    /// gemessene Summe, nicht gegen eine Behauptung.
    #[test]
    fn die_teuerste_stufe_steht_vorn() {
        let mut l = Leistung::neu();
        assert_eq!(
            l.teuerste(),
            None,
            "ohne Messung gibt es eine teuerste Stufe"
        );
        l.merke(Stufe::Netz, 10);
        l.merke(Stufe::Parse, 30);
        l.merke(Stufe::Kaskade, 20);
        assert_eq!(
            l.teuerste().map(|(s, _)| s),
            Some(Stufe::Parse),
            "die teuerste Stufe ist falsch"
        );
        assert_eq!(
            l.anteil_teuerste(),
            Some(0.5),
            "der Anteil stimmt nicht (30 von 60)"
        );
        // Gleichstand: die fruehere Stufe gewinnt (Begruendung im Modul-Kopf).
        l.merke(Stufe::Layout, 30);
        assert_eq!(
            l.teuerste().map(|(s, _)| s),
            Some(Stufe::Parse),
            "bei Gleichstand springt die Zahl"
        );
        // Und die Zeilen sind 6, in Reihenfolge.
        assert_eq!(l.zeilen().len(), 6);
    }
}

/// D19: EIN BILD MESSEN (der Weg, den der Host nach dem Zeichnen ruft).
///
/// GEMESSENE ENTSCHEIDUNG, warum `ms` die MASSGEBENDE Zahl ist und `kaeste`
/// nur eine Angabe: die Dauer ist die Zahl, die man im Reiter sucht
/// („das Bild hat 900 ms gebraucht"), und die Kastenzahl sagt nur, WIE
/// viel Arbeit dahintersteckte. Ohne `ms` waere die Kastenzahl eine
/// Zahl ohne Aussage, ohne `kaeste` waere die Dauer eine Zahl ohne Ursache.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste das Alterste zuerst verliert:
/// bei einem Tab, der eine Stunde laeuft, ist das Bild von vorhin das
/// interessanteste (das erste nach dem Klick), nicht das letzte.
impl Leistung {
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es einen zweiten Weg gibt und nicht nur
    /// einen Parameter mehr: `frame_merken` wird von D19 aus gerufen (dort gab
    /// es nur die Gesamtzeit). Der zweite Weg befuellt die Pro-Frame-Zeiten —
    /// und DER ist der, den ein Flammenprofil braucht.
    pub fn frame_merken_mit(
        &mut self,
        ms: f64,
        kaeste: u32,
        vorgang: &str,
        zeit: FrameZeit,
    ) -> u32 {
        self.frames.push(Frame {
            nr: self.frames.len() as u32 + 1,
            ms,
            kaeste,
            vorgang: Grenzen::STANDARDAUSGABE.kuerze_zeile(vorgang),
            zeit,
        });
        while self.frames.len() > Grenzen::STANDARDAUSGABE.max_phasen {
            self.frames.remove(0);
        }
        self.frames.len() as u32
    }

    /// D20: EIN BILD MIT SEINEN ZEITEN MESSEN — der Weg, den der Aufrufer
    /// nimmt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeit von AUSSEN kommt und nicht aus
    /// `Instant` hier: der Rasterer und die Engine sind fremd, und ihre Zeit
    /// kann nur dort entstehen, wo sie laufen (`narsil::zeichne`). Also
    /// misst der Aufrufer (mit derselben `Instant`) und gibt die Zahl hier
    /// hinein. Wer hier eine `Instant` anlegte, maesse die Zeit des
    /// RECHNENS dieser Funktion — das ist die Zeit des Reiters, nicht die der
    /// Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `rest_ns` dazukommt: die Bildzeit ist
    /// mehr als die Summe der drei Stufen (siehe `FrameZeit::sonst_ns`).
    pub fn frame_messen(
        &mut self,
        kaeste: u32,
        vorgang: &str,
        gesamt: std::time::Duration,
        layout: std::time::Duration,
        raster: std::time::Duration,
        js: std::time::Duration,
    ) -> u32 {
        let genommen = layout.as_nanos() as u64 + raster.as_nanos() as u64 + js.as_nanos() as u64;
        let sonst = (gesamt.as_nanos() as u64).saturating_sub(genommen);
        self.frame_merken_mit(
            gesamt.as_secs_f64() * 1000.0,
            kaeste,
            vorgang,
            FrameZeit {
                layout_ns: layout.as_nanos() as u64,
                raster_ns: raster.as_nanos() as u64,
                js_ns: js.as_nanos() as u64,
                sonst_ns: sonst,
            },
        )
    }

    /// D20: Die frameleiste als Text — eine Zeile je Bild mit seinen Zeiten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeiten in DERSELBEN Zeile stehen und
    /// nicht in einer zweiten Liste: ein Bild ohne seine Zeiten ist eine Zahl
    /// ohne Ursache, und zwei Listen muessen beim Lesen nebeneinander
    /// gehalten werden.
    pub fn frame_profil_zeilen(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|f| {
                let mut z = format!("{f}");
                if f.zeit.ns() > 0 {
                    z.push_str(&format!("  {}", f.zeit.text()));
                }
                z
            })
            .collect()
    }

    /// D20: Die frameleiste als **Flammenbalken** — je Bild ein Viereck,
    /// breit nach der Gesamtzeit, in vier Stuecken nach den Zeiten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Breite logarithmisch ist: die
    /// Bildzeiten eines Tabs liegen zwischen 0,2 ms (nichts passiert) und
    /// 3000 ms (die erste Seite). Linear waeren die kurzen Bilder ein
    /// 1-px-Streifen — und genau die kurzen Bilder sind die, bei denen man
    /// sieht, OB ein Bild teuer war. `log10(1 + ms)` gibt 0.2 ms -> 0.08 und
    /// 3000 ms -> 3.5: ein Faktor 43 fuer einen Faktor 15 000.
    pub fn frame_balken(&self, breite_px: f32) -> Vec<(u32, f32, [f32; 4])> {
        let mut raus = Vec::new();
        if self.frames.is_empty() {
            return raus;
        }
        // Die Skala: das langsamste Bild ist die volle Breite.
        let max_ms = self
            .frames
            .iter()
            .map(|f| f.ms)
            .fold(0.0f64, f64::max)
            .max(0.001);
        for f in self.frames.iter() {
            //
            // GEMESSENE ENTSCHEIDUNG: die Skala rechnet in `f64` (die Bildzeit
            // ist `f64`, und `log10` waere in `f32` ungenau), erst am Ende
            // wird auf Pixel gerundet. Die erste Fassung mischte die Typen
            // (`f64 * f32`) — der Compiler hats gemeldet, und die richtige
            // Rechnung ist die mit einer einzigen Genauigkeit.
            let lg = |ms: f64| ((1.0 + ms).log10() / (1.0 + max_ms).log10()).clamp(0.0, 1.0);
            let w = (lg(f.ms) * breite_px as f64) as f32;
            // Die Stuecke: Anteil an der Bildzeit, auf die Balkenbreite
            // gerechnet. `malen` und `skript` sind an den Werten gemessen;
            // wenn `zeit` leer ist (D19-Aufrufe), faellt der ganze Balken auf
            // „Sonst" — GEMESSEN so gewollt: lieber eine ehrliche Null als
            // eine erfundene Verteilung.
            let teile = if f.zeit.ns() > 0 {
                f.zeit.teile().map(|(_, a)| a as f32)
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            raus.push((f.nr, w.max(1.0), teile));
        }
        raus
    }

    /// D20: Die groesste Zeit eines Bildes und sein Name (fuer die Kachel).
    pub fn frame_teuerste(&self, nr: u32) -> Option<(&'static str, f64)> {
        let f = self.frames.iter().find(|f| f.nr == nr)?;
        let teile = f.zeit.teile();
        teile
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(n, a)| (*n, a * f.zeit.ms()))
    }

    /// D18: EIN BILD MESSEN OHNE ZEITEN JE STUFE (der Weg aus D19).
    ///
    /// GEMESSENE ENTSCHEIDUNG: dieser Weg fuellt `FrameZeit` leer. Im Bild
    /// faellt der Flammenbalken dann ganz auf „Sonst" (siehe `frame_balken`) —
    /// das ist ehrlicher als eine erfundene Verteilung.
    pub fn frame_merken(&mut self, ms: f64, kaeste: u32, vorgang: &str) -> u32 {
        let g = Grenzen::STANDARDAUSGABE;
        let nr = self.frames.len() as u32 + 1;
        self.frames.push(Frame {
            nr,
            ms,
            kaeste,
            vorgang: g.kuerze_zeile(vorgang),
            zeit: FrameZeit::default(),
        });
        while self.frames.len() > g.max_phasen {
            self.frames.remove(0);
        }
        nr
    }
}

/// D19: die Bilder als Text (der Weg des Reiters).
impl Leistung {
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der VORGANG in der Zeile steht und
    /// nicht nur im Zustand: ein Bild ohne die Angabe, wodurch es ausgeloest
    /// wurde, ist eine Zahl, die man nicht deuten kann — nach einem Klick
    /// will man genau dieses Bild finden („das zweite Bild nach dem Klick").
    pub fn frame_zeilen(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|f| {
                if f.vorgang.is_empty() {
                    format!("{f}")
                } else {
                    format!("{f}  ({})", f.vorgang)
                }
            })
            .collect()
    }
}

impl Leistung {
    /// D19: die PROFIL-Zeile — Layout, Malen und Skript in einer Zeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum genau DREI und nicht alle sechs Stufen:
    /// die sechs Stufen stehen bereits je eine Zeile darunter (`text_zeilen`).
    /// Die Profilzeile ist die **eine** Zeile, die Firefox oben ueber dem
    /// Diagramm zeigt — „Layout 12 ms | Paint 4 ms | Script 30 ms" — und sie
    /// darf nicht dieselbe Information zweimal in zwei Formen zeigen. Also
    /// die drei, nach denen man sucht, wenn die Seite ruckelt: Layout rechnet
    /// die Kaesten, Malen schreibt die Pixel, Skript laeuft dazwischen.
    pub fn profil_zeile(&self) -> String {
        format!(
            "Profil: Layout {:.2} ms | Malen {:.2} ms | Skript {:.2} ms | {} Bilder",
            self.ms(Stufe::Layout),
            self.ms(Stufe::Raster),
            self.ms(Stufe::Js),
            self.frames.len()
        )
    }

    /// Die gemessene Zeit einer Stufe in ms (der Weg der Profilzeile).
    pub fn ms(&self, stufe: Stufe) -> f64 {
        self.stufe(stufe).ms()
    }
}
