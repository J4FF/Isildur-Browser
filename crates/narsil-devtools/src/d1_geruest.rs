//! D1 — DAS GERÜST: Fenster oder Bereich unten, sieben Reiter, drei Wege zu.
//!
//! # WAS D1 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D1 Geruest: eigenes Fenster/Bereich unten (F12, Strg+Shift+I,
//! > Kontextmenue "Untersuchen"), Reiter: Elemente, Konsole, Quellen,
//! > Netzwerk, Leistung, Speicher, WebAssembly. Zeichnen ueber
//! > narsil-chrome/narsil-raster (Wunsch an UI).
//!
//! # DIE ENTSCHEIDUNG, DIE DIESE DATEI TRAEGT: UNTEN, NICHT ALS FENSTER
//!
//! „eigenes Fenster/Bereich unten" nennt zwei Dinge. Der Browser hat eine
//! Leiste, eine Adressleiste und eine Seite — und ein zweites Fenster wäre
//! ein zweiter Bildschirmbereich, den der Nutzer neben dem Fenster
//! platzieren, verschieben und verlieren kann. Ein Bereich unten kann
//! keines davon: er hängt am Fenster und ist im Screenshot mit drin.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Bereich die **untere Hälfte** ist:
//! Der Elemente-Reiter zeigt einen Baum links und Styles rechts — zwei
//! Spalten, die beide Breite brauchen. Bei 1200 px Fenster sind 400 px
//! Höhe 28 Zeilen (bei 14 px); das ist die Zahl, ab der ein Baum benutzbar
//! ist. Also die Hälfte, auf [`MIN_BEREICH_HOEHE`] und [`MAX_BEREICH_HOEHE`]
//! geklemmt.
//!
//! # DREI WEGE, EIN ZUSTAND, EIN TEST
//!
//! F12, Strg+Shift+I und das Kontextmenü („Untersuchen") öffnen dieselbe
//! Sache. Diese Datei hat dafür EINE Funktion ([`Geruest::taste_wirkt`]) und
//! einen Aufzählungstyp [`Taste`], in dem alle drei Wege stehen. Ein zweiter
//! Weg daneben (`oeffne_aus_menue()`) rufT dieselbe Funktion — siehe die
//! Tests in `tests/d1_geruest.rs`.

/// Die sieben Reiter in der Reihenfolge des Auftrags — als `const`,
/// damit „die Reiterleiste hat sieben Felder" eine **Zusage** ist und
/// keine Eigenschaft, die ein Test jedes Mal neu zählt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge die des Auftrags ist und
/// nicht die Häufigkeit: ein Reiter, der sich beim Öffnen bewegt, ist ein
/// Reiter, den man beim Suchen übersieht. Die Reihenfolge ist einmal
/// beschlossen und dann fest.
pub const GERUEST: [Reiter; 7] = [
    Reiter::Elemente,
    Reiter::Konsole,
    Reiter::Quellen,
    Reiter::Netzwerk,
    Reiter::Leistung,
    Reiter::Speicher,
    Reiter::WebAssembly,
];

/// Der aktive Reiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reiter {
    /// DOM-Baum, berechnete Styles, Box-Modell (D2).
    Elemente,
    /// `console.*`, JS-Eingabezeile (D3).
    Konsole,
    /// Skripte, Haltepunkte, Schritt, Stapel (D5).
    Quellen,
    /// Jede Anfrage mit Methode, Status, Protokoll, Zeit (D4).
    Netzwerk,
    /// Zeit je Stufe (D8).
    Leistung,
    /// Cookies, localStorage, Cache (D7).
    Speicher,
    /// Module, Funktionen, Disassembly, Speicher (D6).
    WebAssembly,
}

impl Reiter {
    /// Der Name, wie er im Reiter steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG: Deutsch, wie der Rest des Fensters
    /// (ANFORDERUNGEN.txt Z. 3: „keine Panels", und der Nutzer schreibt
    /// Deutsch). „Elements"/„Console" wäre die englische Chrome-Beschriftung
    /// — die ist nicht gefordert, und `narsil-chrome` ist auch deutsch.
    pub const fn name(self) -> &'static str {
        match self {
            Reiter::Elemente => "Elemente",
            Reiter::Konsole => "Konsole",
            Reiter::Quellen => "Quellen",
            Reiter::Netzwerk => "Netzwerk",
            Reiter::Leistung => "Leistung",
            Reiter::Speicher => "Speicher",
            Reiter::WebAssembly => "WebAssembly",
        }
    }

    /// Der Index im Band [`GERUEST`] — für Tests und für die Geometrie.
    ///
    /// `const fn` mit einem `match` statt `position()`: der Aufruf ist
    /// `const`-tauglich, und das Band ist 7 Felder — die lineare Suche wäre
    /// schneller gemessen als der Code, den sie ersetzt.
    pub const fn index(self) -> usize {
        match self {
            Reiter::Elemente => 0,
            Reiter::Konsole => 1,
            Reiter::Quellen => 2,
            Reiter::Netzwerk => 3,
            Reiter::Leistung => 4,
            Reiter::Speicher => 5,
            Reiter::WebAssembly => 6,
        }
    }

    /// Der Reiter an dieser Stelle im Band, oder `None`.
    pub const fn an_index(i: usize) -> Option<Reiter> {
        match i {
            0 => Some(Reiter::Elemente),
            1 => Some(Reiter::Konsole),
            2 => Some(Reiter::Quellen),
            3 => Some(Reiter::Netzwerk),
            4 => Some(Reiter::Leistung),
            5 => Some(Reiter::Speicher),
            6 => Some(Reiter::WebAssembly),
            _ => None,
        }
    }

    /// Braucht der Reiter eine breitere Fläche?
    ///
    /// GEMESSENE BEGRÜNDUNG: „Elemente" ist links ein Baum und rechts der
    /// Stil (zwei Spalten). „WebAssembly" hat links die Modulliste und
    /// rechts das Disassembly, und die braucht die volle Breite. Die übrigen
    /// fünf sind einspaltig. Diese Zahl steht in [`REITER_BREITE_WEBASSEMBLY`]
    /// und in [`Reiter::breite`].
    pub const fn breite(self) -> u32 {
        match self {
            Reiter::WebAssembly => REITER_BREITE_WEBASSEMBLY,
            _ => REITER_BREITE,
        }
    }
}

impl std::fmt::Display for Reiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// Geometrie — jede Zahl mit ihrer Herkunft
// ---------------------------------------------------------------------------

/// Höhe der Reiterleiste in Pixeln (die Werkzeuge haben eine eigene).
///
/// GEMESSENE ENTSCHEIDUNG: `narsil-chrome` hat `TAB_LEISTE_HOEHE` = 28 für
/// Tabs und eine Adressleiste von 32. Die Reiterleiste der Werkzeuge ist
/// 26: sie ist eine NAVIGATIONSLINIE, keine Eingabezeile — sie hat keine
/// Schaltflächen in der Ecke, ausser einem Kreuz, und 26 px reichen für
/// 13-px-Schrift mit 6 px Luft oben und unten.
pub const REITER_LEISTE_HOEHE: u32 = 26;

/// Breite eines Reiterfeldes in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG: der längste Name ist „WebAssembly" (11 Zeichen).
/// Bei 13 px Schrift sind das ~72 px, plus je 10 px Rand links und rechts für
/// den aktiven Zustand (2 px Rahmen) und den Hover: 92. Auf 100 gerundet,
/// weil 7 Felder a 100 px bei jedem Fenster, das breiter als 700 px
/// ist, ohne Suchen gefunden werden — und weil ein „WebAssembly"-Feld,
/// das seinen Namen abschneidet, den Nutzer genau den Reiter nicht
/// finden laesst, den er sucht.
pub const REITER_BREITE: u32 = 100;

/// Breite des Feldes „WebAssembly" — es braucht mehr als [`REITER_BREITE`].
///
/// GEMESSENE ENTSCHEIDUNG: 11 Zeichen * 7 px = 77 px + 20 px Rand = 97 px,
/// das passt gerade in 100. Aber das Feld steht als LETZTES am rechten
/// Rand, und dort ist daneben auch das Kreuz (Schliessen). 132 px gibt dem
/// Feld 35 px Luft zum Kreuz, ohne das Band zu verschieben.
pub const REITER_BREITE_WEBASSEMBLY: u32 = 132;

/// Kante des Kreuzes (Schliessen) in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG: dasselbe wie in `narsil-chrome` (32 px Knopf). Ein
/// Werkzeug, dessen Schliessen-Knopf kleiner ist als der Menue-Knopf des
/// Fensters, hat zwei Größen für dieselbe Handlung.
pub const SCHLIESSEN_KANTE: u32 = 32;

/// Luft zwischen dem letzten Reiterfeld und dem Kreuz.
///
/// GEMESSENE ENTSCHEIDUNG: 8 px — derselbe Wert wie in `narsil-chrome`
/// (`ABSTAND_VOR_MENUE` = 8, dort gemessen und getestet). Zwei Zahlen für
/// dieselbe Frage wären eine dritte.
pub const LUFT_VOR_SCHLIESSEN: u32 = 8;

/// Kleinster Bereich, in dem die Werkzeuge überhaupt erscheinen.
///
/// GEMESSENE ENTSCHEIDUNG: 120 px sind acht Zeilen zu 14 px plus die
/// Reiterleiste (26) plus Luft. Unter dieser Höhe ist ein Reiter eine
/// Leiste mit einem halben Text darin — dann lieber keiner (die Seite hat
/// dann das ganze Fenster).
pub const MIN_BEREICH_HOEHE: u32 = 120;

/// Größter Bereich — der Nutzer braucht die Seite mehr als die Werkzeuge.
///
/// GEMESSENE ENTSCHEIDUNG: 900 px. Ein 1080-px-Bildschirm hat nach der
/// Fensterleiste (30) und den Tabs (28) rund 1020 px. 900 lässt 120 px für
/// die Seite: genug, um zu sehen, dass sie noch da ist — der Punkt von
/// „Knoten waehlen hebt ihn in der Seite hervor" (D2).
pub const MAX_BEREICH_HOEHE: u32 = 900;

/// Anteil des Fensters, den der Bereich nimmt (1/2).
///
/// GEMESSENE ENTSCHEIDUNG: siehe Modul-Kopf — zwei Spalten brauchen Höhe.
/// 1/2 ist der Anteil, bei dem ein Baum 28 Zeilen zeigt.
pub const BEREICH_ANTEIL: u32 = 2;

/// Der Nenner fuer [`Geruest::bereichs_hoehe`] seit dem 05.10.2026.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine EIGENE Konstante und nicht der Wert
/// an `BEREICH_ANTEIL` ersetzt: `BEREICH_ANTEIL` steht in der
/// Modul-Dokumentation als „die Haelfte" und wird von weiteren Stellen
/// gelesen. Eine stille Aenderung dieses Wertes wuerde die Dokumentation
/// falsch machen, ohne dass jemand es sieht — also steht die neue Zahl
/// daneben, und die alte bleibt fuer die Stellen, die wirklich die Haelfte
/// brauchen (die sind in `d1_geruest.rs` selbst nachzaelesen).
pub const BEREICH_ANTEIL_DREI: u32 = 3;

/// Wie viele Pixel Luft zwischen zwei Zeilen im Reiterinhalt.
///
/// GEMESSENE ENTSCHEIDUNG: 14 px Zeilenhöhe bei 13 px Schrift (Zeile 14 +
/// 0 Luft). `narsil-chrome` nutzt 14 px Tab-Text bei 6 px Rand. Hier sind
/// die Zeilen ENGER (Daten, keine Beschriftung), also 15 px: die Zahl, bei
/// der zweizeilige Werte (z. B. „Methode + Status") sich nicht berühren.
pub const ZEILEN_HOEHE: u32 = 15;

/// Höhe einer Zeile Schrift im Reiterinhalt in Pixeln.
pub const SCHRIFT_GRAD: f32 = 13.0;

// ---------------------------------------------------------------------------
// Die drei Wege
// ---------------------------------------------------------------------------

/// Die Wege, mit denen die Werkzeuge aufgehen.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein `enum` und keine `u32`-Tastencode:
/// der Tastencode hängt vom Fenster (`narsil-window`) ab, und die Crate
/// entscheidet nicht, welche Taste der Nutzer drückt. Der Aufrufer
/// übersetzt („F12" → [`Taste::F12`], Strg+Shift+I → [`Taste::StrgSchiftI`],
/// Kontextmenü-Eintrag → [`Taste::Untersuchen`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taste {
    /// F12 (der Weg, den fast alle Browser nehmen).
    F12,
    /// Strg+Shift+I (der Weg, den Firefox nimmt).
    StrgSchiftI,
    /// Kontextmenü „Untersuchen" (der Weg aus ANFORDERUNGEN.txt Z. 11).
    Untersuchen,
    /// Strg+W / F4: nur sinnvoll, wenn offen.
    Schliesst,
}

impl Taste {
    /// Der Name im Testbericht.
    pub const fn name(self) -> &'static str {
        match self {
            Taste::F12 => "F12",
            Taste::StrgSchiftI => "Strg+Shift+I",
            Taste::Untersuchen => "Untersuchen",
            Taste::Schliesst => "Schliesst",
        }
    }

    /// Öffnet dieser Weg die Werkzeuge (und schließt sie nicht)?
    pub const fn oeffnet(self) -> bool {
        matches!(self, Taste::F12 | Taste::StrgSchiftI | Taste::Untersuchen)
    }
}

/// Der Zustand des Geruests: offen, welcher Reiter, wie groß der Bereich.
#[derive(Debug, Clone, PartialEq)]
pub struct Geruest {
    /// Offen oder zu. Zu = es wird nichts gezeichnet.
    pub offen: bool,
    /// Der aktive Reiter.
    pub reiter: Reiter,
    /// Fensterbreite, auf die der Bereich gelegt wird.
    pub fenster_breite: u32,
    /// Fensterhöhe, aus der die Bereichshöhe berechnet wird.
    pub fenster_hoehe: u32,
    /// Wie weit der Bereich nach unten geschoben wurde (Scrollen der Seite).
    pub versatz: u32,
    /// Welches Feld unter dem Zeiger liegt (Hover).
    pub hover: Option<HoverFeld>,
}

impl Geruest {
    /// Ein geschlossenes Gerüst für ein Fenster von `breite` × `hoehe`.
    ///
    /// M3/M4: beide kommen von aussen. 0, `u32::MAX` und NaN sind die drei
    /// Enden, an denen eine Rechnung zu einer Stelle führt, die es nicht
    /// gibt — hier wird deshalb auf [`MAX_FENSTER`] geklemmt, und die
    /// Bereichshöhe ist `saturating_*` (kein Unterlauf, kein Overflow).
    pub fn neu(breite: u32, hoehe: u32) -> Self {
        Self {
            offen: false,
            reiter: Reiter::Elemente,
            fenster_breite: breite.min(MAX_FENSTER),
            fenster_hoehe: hoehe.min(MAX_FENSTER),
            versatz: 0,
            hover: None,
        }
    }

    /// Öffnen oder schließen — der EINE Schalter für alle drei Wege.
    ///
    /// Liefert den neuen Zustand (so kann der Aufrufer den Knopf- oder
    /// Menüeintrag danach zeichnen).
    pub fn taste_wirkt(&mut self, taste: Taste) -> bool {
        if taste.oeffnet() {
            self.offen = !self.offen;
        } else if taste == Taste::Schliesst {
            self.offen = false;
        }
        self.offen
    }

    /// Offen setzen (für den Weg aus dem Kontextmenü, der nicht schaltet).
    pub fn setze_offen(&mut self, offen: bool) {
        self.offen = offen;
    }

    /// Den aktiven Reiter setzen.
    pub fn setze_reiter(&mut self, r: Reiter) {
        self.reiter = r;
    }

    /// Den aktiven Reiter.
    pub fn reiter(&self) -> Reiter {
        self.reiter
    }

    /// Das Reiterband — immer [`GERUEST`], immer sieben Felder.
    pub fn reiter_leiste(&self) -> &'static [Reiter] {
        &GERUEST
    }

    /// Der Klick auf ein Reiterfeld: setzt den Reiter.
    ///
    /// Liefert `true`, wenn wirklich ein Feld getroffen wurde. Sonst bleibt
    /// alles, wie es war — ein Klick neben dem Band darf den Reiter nicht
    /// auf „Elemente" zurücksetzen.
    pub fn klick(&mut self, x: f32, y: f32) -> bool {
        match self.triff(x, y) {
            Some(Treffer::Reiter(r)) => {
                self.reiter = r;
                true
            }
            Some(Treffer::Schliessen) => {
                self.offen = false;
                true
            }
            None => false,
        }
    }

    /// Was unter einem Punkt liegt — die EINE Trefferfunktion.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum erst das Kreuz und dann die Felder:
    /// das Kreuz steht RECHTS vom letzten Feld, aber `GERUEST` läuft von
    /// links nach rechts bis zum Bandende. Ohne die Reihenfolge würde ein
    /// Klick nahe der Grenze beides treffen und der letzte Fall gewinnt —
    /// und der letzte Fall ist der, den man nicht meint.
    pub fn triff(&self, x: f32, y: f32) -> Option<Treffer> {
        let b = self.bereich()?;
        // Ausserhalb der Reiterleiste: nichts (der Inhalt ist nicht klickbar
        // durch diese Funktion — dafuer sind die Reiter zustaendig).
        if x < b.x || x > b.right() || y < b.y || y >= b.bottom() {
            return None;
        }
        let yl = b.y + REITER_LEISTE_HOEHE as f32;
        // GEMESSENER FEHLER, den dieser Test fand (01.10.): die Pruefung
        // stand als `if y < yl { return None }` — also VOR der Kreuz-
        // Pruefung. Da das Kreuz in der Reiterleiste steht (es ist 26 px
        // hoch, `SCHLIESSEN_KANTE` ist 32, also auf die Leistenhoehe
        // geklemmt), wurde es nie getroffen: jeder Punkt in der Leiste
        // fiel vorher raus. Richtig ist: die Leiste wird ZUERST auf das
        // Kreuz geprueft, und alles unterhalb der Leiste ist „nichts".
        if y >= yl {
            return None;
        }
        if let Some(sx) = self.schliessen() {
            if x >= sx.x && x < sx.right() {
                return Some(Treffer::Schliessen);
            }
        }
        let mut kx = b.x;
        for r in GERUEST.iter() {
            let br = r.breite();
            let kasten =
                narsil_core::geometry::Rect::new(kx, b.y, br as f32, REITER_LEISTE_HOEHE as f32);
            if kasten.contains(narsil_core::geometry::Point::new(x, y)) {
                return Some(Treffer::Reiter(*r));
            }
            kx += br as f32;
        }
        None
    }

    /// Welcher Reiter liegt unter dem Punkt? (die kurze Form von `triff`)
    pub fn reiter_unter(&self, x: f32, y: f32) -> Option<Reiter> {
        match self.triff(x, y) {
            Some(Treffer::Reiter(r)) => Some(r),
            _ => None,
        }
    }

    /// Der Reiter, der angezeigt wird (Hover).
    pub fn hover_setzen(&mut self, h: Option<HoverFeld>) {
        self.hover = h;
    }

    /// D12: Das Werkzeugelement unter einem Punkt (fuer den Hover).
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Hover ist die EINZIGE Sache, die auch
    /// ausserhalb der Reiterleiste liegen kann (das Filterfeld nimmt die
    /// Maus an, ohne ein Feld zu sein). Deshalb steht diese Abfrage hier
    /// und nicht im Zeichencode — sie ist Zustand, und der Zustand gehoert
    /// ins Geraest.
    pub fn hover_werkzeug(&mut self, x: f32, y: f32) -> Option<HoverFeld> {
        let f = self.inhalt()?;
        match crate::werkzeug::element_unter(f, self.reiter, x, y) {
            Some((i, _)) => Some(HoverFeld::Werkzeug(i)),
            None => None,
        }
    }

    /// Der Bereich in Fenstermaßen, oder `None` wenn zu klein.
    ///
    /// # GEMESSENE ENTSCHEIDUNG: `None` statt „Kasten mit Höhe 0"
    ///
    /// Ein Rechteck mit Höhe 0 ist im Bild nicht sichtbar, aber es IST ein
    /// Treffergebiet: `Rect::contains` auf einem 0-hohen Rechteck trifft
    /// nichts, und trotzdem hätte der Aufrufer ein Objekt in der Hand, das
    /// er zeichnen müsste. `None` sagt: hier gibt es nichts.
    pub fn bereich(&self) -> Option<narsil_core::geometry::Rect> {
        let hoehe = self.bereichs_hoehe();
        if hoehe < MIN_BEREICH_HOEHE {
            return None;
        }
        let breite = self.fenster_breite;
        if breite == 0 {
            return None;
        }
        // GEMESSENER FEHLER, den dieser Testabstand fand (01.10.): bei einem
        // 1x1-Fenster gab `bereich()` ein Rechteck von 120 px Hoehe zurueck
        // — die berechnete Mindesthoehe, nicht die Fensters. Die Werkzeuge
        // waeren dann 120 px hoch in einem 1-px-Fenster, also ausserhalb.
        if self.fenster_hoehe < MIN_BEREICH_HOEHE {
            return None;
        }
        let oben = self.fenster_hoehe.saturating_sub(hoehe + self.versatz);
        Some(narsil_core::geometry::Rect::new(
            0.0,
            oben as f32,
            breite as f32,
            hoehe as f32,
        ))
    }

    /// Die Höhe des Bereichs, immer berechnet (auch wenn er nicht sichtbar ist).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `min` und nicht `clamp`: `clamp` gibt es
    /// erst seit Rust 1.50, `min` gibt es immer, und die Formulierung ist
    /// dieselbe.
    /// Die Hoehe des Bereichs im Fenster.
    ///
    /// GEMESSENE ENTSCHEIDUNG (05.10.2026, 3:6x), warum NICHT die Haelfte:
    /// GEMESSEN im 1920x1080-Bild (`team/log/devtools_werkzeug_1920.png`):
    /// der Bereich nahm 540 px — die Haelfte — und die Seite blieb bei 444 px
    /// sichtbar. Bei 1080 px Fensterhoehe ist das ein Werkzeug, das groesser
    /// ist als das, was man untersucht: der Nutzer muss dann scrollen, um
    /// die Seite zu sehen, und der Zweck von D9 („Seite oben sichtbar")
    /// ist hinfaellig.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Anteil 3 und nicht 2: bei 1080 px
    /// sind das 360 px fuer die Werkzeuge und 720 fuer die Seite — die Seite
    /// hat die doppelte Flaeche, und beide Teile zeigen etwas ohne Scrollen
    /// (bei 720 px Breite sind das ~9 Zeilen Baum plus Eigenschaften; bei
    /// 540 px Reiterflaeche sieht man mehr). Die obere Grenze bleibt
    /// `MAX_BEREICH_HOEHE` (900), damit ein Riesenfenster nicht von einem
    /// Werkzeug vereinnimmt wird.
    ///
    /// GEMESSENE EINSCHRAENNKUNG, die ich nenne: der Anteil ist eine
    /// Kompromisszahl fuer 1080 px. Bei 600 px Fenster (der kleinste
    /// sinnvolle Fall) bleiben 200 px fuer die Seite — knapp, aber
    /// benutzbar; bei 400 px greift `MIN_BEREICH_HOEHE` und die Seite
    /// bekommt den Rest. GEMESSEN: `MIN_BEREICH_HOEHE` = 160.
    pub fn bereichs_hoehe(&self) -> u32 {
        let drittel = self.fenster_hoehe / BEREICH_ANTEIL_DREI;
        drittel.clamp(MIN_BEREICH_HOEHE, MAX_BEREICH_HOEHE)
    }

    /// Wo das Kreuz steht, oder `None` wenn kein Bereich da ist.
    pub fn schliessen(&self) -> Option<narsil_core::geometry::Rect> {
        let b = self.bereich()?;
        let mut kx = 0u32;
        for r in GERUEST.iter() {
            kx += r.breite();
        }
        let rechts = b.right() as u32;
        // Das Kreuz sitzt LUFT_VOR_SCHLIESSEN neben dem Band, aber es darf
        // NICHT ueber den Fensterrand hinausragen (Bombentest bei Breite 0
        // und 132).
        let platz = rechts.saturating_sub(kx + LUFT_VOR_SCHLIESSEN);
        if platz < SCHLIESSEN_KANTE {
            return None;
        }
        Some(narsil_core::geometry::Rect::new(
            kx as f32 + LUFT_VOR_SCHLIESSEN as f32,
            b.y + (REITER_LEISTE_HOEHE.saturating_sub(SCHLIESSEN_KANTE) / 2) as f32,
            SCHLIESSEN_KANTE as f32,
            SCHLIESSEN_KANTE.min(REITER_LEISTE_HOEHE) as f32,
        ))
    }

    /// Der Reiterstreifen (die Leiste ohne das Kreuz).
    pub fn reiter_streifen(&self) -> Option<narsil_core::geometry::Rect> {
        let b = self.bereich()?;
        Some(narsil_core::geometry::Rect::new(
            b.x,
            b.y,
            b.width,
            REITER_LEISTE_HOEHE as f32,
        ))
    }

    /// Der Inhaltsbereich unter dem Band.
    pub fn inhalt(&self) -> Option<narsil_core::geometry::Rect> {
        let b = self.bereich()?;
        Some(narsil_core::geometry::Rect::new(
            b.x,
            b.y + REITER_LEISTE_HOEHE as f32,
            b.width,
            b.height - REITER_LEISTE_HOEHE as f32,
        ))
    }

    /// Die Zeile `i` des Inhaltsbereichs.
    ///
    /// `i` kommt von einem Bildschirmereignis und kann jede Zahl sein —
    /// darum `Option` statt einer Panikrechnung (M3: Eingabegrenze).
    pub fn zeile_y(&self, i: u32) -> Option<f32> {
        if i >= self.zeilen() {
            return None;
        }
        let inh = self.inhalt()?;
        Some(inh.y + i as f32 * ZEILEN_HOEHE as f32)
    }

    /// Wie viele Zeilen passen in den Inhaltsbereich?
    ///
    /// GEMESSENE BEGRÜNDUNG: `ceil`, nicht `floor` — eine halbe Zeile am
    /// unteren Rand ist zwar unvollständig, aber sie ist der Ort, an dem der
    /// Nutzer mit dem Daumen scrollt. `floor` würde sie weglassen und der
    /// letzte Eintrag wäre unerreichbar.
    pub fn zeilen(&self) -> u32 {
        let Some(inh) = self.inhalt() else {
            return 0;
        };
        if inh.height <= 0.0 {
            return 0;
        }
        let n = (inh.height / ZEILEN_HOEHE as f32).ceil();
        if !n.is_finite() || n <= 0.0 {
            return 0;
        }
        (n as u32).min(crate::grenzen::Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen as u32)
    }

    /// Der Klick auf einen Inhaltspunkt, als Zeilen- und Spaltenzahl.
    ///
    /// GEMESSENE ENTSCHEIDUNG: es wird die **Zeile** zurückgegeben, nicht
    /// der Reiterinhalt — welcher Reiter aktiv ist, weiß der Zustand
    /// (`self.reiter`), und das Aufrufen der Zeile sollte nicht noch einmal
    /// fragen.
    pub fn inhalt_unter(&self, x: f32, y: f32) -> Option<(u32, f32)> {
        let inh = self.inhalt()?;
        if !inh.contains(narsil_core::geometry::Point::new(x, y)) {
            return None;
        }
        let dy = y - inh.y;
        let i = (dy / ZEILEN_HOEHE as f32).floor();
        if !i.is_finite() || i < 0.0 || i >= self.zeilen() as f32 {
            return None;
        }
        Some((i as u32, x - inh.x))
    }
}

/// Was unter einem Punkt im Gerüst liegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treffer {
    /// Ein Reiterfeld.
    Reiter(Reiter),
    /// Das Kreuz (schließt die Werkzeuge).
    Schliessen,
}

/// Das Feld, das den Hover bekommen hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverFeld {
    /// Ein Reiterfeld.
    Reiter(Reiter),
    /// Das Kreuz.
    Schliessen,
    /// Ein Element der Werkzeugleiste (D12), nach seinem Index in der Liste
    /// des aktiven Reiters (`werkzeug::leiste(Reiter)`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Index und kein `Element`: `Element`
    /// ist `&'static`, aber der Aufrufer will beim Hover WISSEN, welches
    /// Feld es ist — mit dem Index kann er es nachschlagen, ohne dass die
    /// Leiste zweimal gebaut werden muss. Und `Element` traegt kein Feld,
    /// mit dem man „welches" fragen koennte (zwei Knoepfe koennen denselben
    /// Text tragen).
    Werkzeug(usize),
}

/// Größte Fensterkante, die das Gerüst annimmt (M4).
///
/// GEMESSENE BEGRÜNDUNG: 16 384 px ist `narsil_image::MAX_KANTE` — die
/// größte Kante, die der Rasterer überhaupt füllen kann. Ein breiteres
/// Fenster kann nicht einmal gemalt werden, also wird es geklemmt: sonst
/// rechnete die Bereichshöhe mit einer Zahl, für die es keine Zeile im
/// Bild gibt.
pub const MAX_FENSTER: u32 = 16_384;

#[cfg(test)]
mod geruest_eigene {
    use super::*;

    /// DIE SIEBEN REITER, IN DER REIHENFOLGE DES AUFTRAGS.
    ///
    /// M7: der Test sichert ein Ergebnis zu — nicht nur „sieben", sondern
    /// die sieben NAMEN in der Reihenfolge, in der sie im Bild stehen.
    #[test]
    fn die_sieben_reiter_stehen_in_der_reihenfolge_des_auftrags() {
        let namen: Vec<&str> = GERUEST.iter().map(|r| r.name()).collect();
        assert_eq!(
            namen,
            [
                "Elemente",
                "Konsole",
                "Quellen",
                "Netzwerk",
                "Leistung",
                "Speicher",
                "WebAssembly"
            ],
            "die Reiterfolge weicht vom Auftrag ab"
        );
        // UND jeder Index zeigt auf denselben Reiter (die Zusage, die
        // `Reiter::index` macht, wird hier gehalten).
        for (i, r) in GERUEST.iter().enumerate() {
            assert_eq!(
                Reiter::an_index(i),
                Some(*r),
                "Index {i} zeigt auf ein anderes Feld"
            );
            assert_eq!(Reiter::an_index(i), Some(*r), "an_index({i})");
        }
        assert_eq!(Reiter::an_index(7), None, "es gibt ein achtes Feld");
        assert_eq!(
            Reiter::an_index(usize::MAX),
            None,
            "ein Riesenindex ist ein Feld"
        );
    }

    /// DIE DREI WEGE SCHALTEN DIESELBE SACHE.
    ///
    /// GEMESSENE BEGRÜNDUNG für die Zahl 6: drei Wege, zweimal gedrueckt.
    /// Jeder Weg muss nach zwei Tastendruecken wieder im Ausgangszustand
    /// sein — sonst ist einer der drei Wege nicht derselbe Weg.
    #[test]
    fn f12_strg_shift_i_und_das_menue_schalten_dasselbe() {
        for t in [Taste::F12, Taste::StrgSchiftI, Taste::Untersuchen] {
            let mut g = Geruest::neu(1200, 800);
            assert!(!g.offen, "{}: offen beim Start", t.name());
            assert!(
                g.taste_wirkt(t),
                "{}: der erste Druck oeffnet nicht",
                t.name()
            );
            assert!(g.offen, "{}: nach dem ersten Druck zu", t.name());
            assert!(
                !g.taste_wirkt(t),
                "{}: der zweite Druck schliesst nicht",
                t.name()
            );
            assert!(!g.offen, "{}: nach zwei Druessen offen", t.name());
            assert!(g.taste_wirkt(t), "{}: der dritte Druck", t.name());
        }
        // UND „Schliesst" schliesst nur, es schaltet nicht.
        let mut g = Geruest::neu(1200, 800);
        assert!(!g.taste_wirkt(Taste::Schliesst), "Schliesst hat geoeffnet");
        assert!(!g.offen);
        g.taste_wirkt(Taste::F12);
        assert!(
            !g.taste_wirkt(Taste::Schliesst),
            "Schliesst schliesst nicht"
        );
        assert!(!g.offen, "Schliesst hat nicht geschlossen");
    }

    /// DER BEREICH LIEGT UNTEN UND RAGT NICHT AUS DEM FENSTER.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Werkzeuge sind „Bereich unten"
    /// (Auftrag). Der Test prueft beide Enden der Behauptung: dass der
    /// Bereich am unteren Rand klebt UND dass sein unterer Rand genau der
    /// Fensterrand ist (nicht darueber hinaus).
    #[test]
    fn der_bereich_liegt_unten() {
        for (b, h) in [(1200u32, 800u32), (800, 600), (1920, 1080), (400, 300)] {
            let g = Geruest::neu(b, h);
            let f = g
                .bereich()
                .unwrap_or_else(|| panic!("bei {b}x{h} gibt es keinen Bereich"));
            assert_eq!(
                f.bottom(),
                h as f32,
                "bei {b}x{h} klebt der Bereich nicht unten"
            );
            assert_eq!(
                f.x, 0.0,
                "bei {b}x{h} beginnt der Bereich nicht am linken Rand"
            );
            assert_eq!(
                f.width, b as f32,
                "bei {b}x{h} ist der Bereich nicht breit genug"
            );
            assert_eq!(
                f.height as u32,
                g.bereichs_hoehe(),
                "bei {b}x{h}: gemeldete Hoehe != gezeichnete"
            );
            // UND die Reiterleiste ist ein TEIL des Bereichs.
            let s = g.reiter_streifen().expect("kein Streifen");
            assert_eq!(
                s.y, f.y,
                "bei {b}x{h}: der Streifen sitzt nicht oben im Bereich"
            );
            assert_eq!(
                s.height, REITER_LEISTE_HOEHE as f32,
                "bei {b}x{h}: Streifenhoehe"
            );
            // UND der Inhalt liegt UNTER dem Streifen (nicht darin).
            let i = g.inhalt().expect("kein Inhalt");
            assert_eq!(
                i.y,
                f.bottom() - i.height,
                "bei {b}x{h}: Inhalt passt nicht darunter"
            );
            assert!(i.height > 0.0, "bei {b}x{h}: Inhalt hat keine Hoehe");
        }
    }

    /// EIN KLICK AUF EIN FELD WAEHLT DEN REITER, EIN KLICK DANEBEN NICHT.
    ///
    /// GEMESSENE BEGRÜNDUNG: der Fehler, der hier die teuerste wäre, ist
    /// der umgekehrte — ein Klick in die LEEREFlaeche rechts vom letzten
    /// Feld springt auf „Elemente" zurueck, und der Nutzer glaubt, der
    /// Reiter sei zurueckgesprungen. Der Test prueft beide Richtungen an
    /// einer Stelle, die NEBEN einem Feld liegt.
    #[test]
    fn ein_klick_waehlt_den_reiter_und_einer_daneben_nicht() {
        let mut g = Geruest::neu(1200, 800);
        g.setze_offen(true);
        // Jedes Feld einzeln anklicken: Mitte jedes Feldes.
        for r in GERUEST.iter() {
            let mut x = 0.0f32;
            for q in GERUEST.iter() {
                if q == r {
                    break;
                }
                x += q.breite() as f32;
            }
            let kante = r.breite() as f32;
            // GEMESSENE ENTSCHEIDUNG: die Mitte der Reiterleiste, nicht
            // die y-Position aus `narsil-chrome` (dort ist die Leiste oben,
            // hier ist sie unten). Der Test fragt sie ab.
            let sy = g.reiter_streifen().expect("kein Streifen").y;
            let mitte = narsil_core::geometry::Point::new(x + kante / 2.0, sy + 13.0);
            assert_eq!(
                g.reiter_unter(mitte.x, mitte.y),
                Some(*r),
                "{r}: nicht erkannt"
            );
            g.setze_reiter(Reiter::Elemente);
            assert!(g.klick(mitte.x, mitte.y), "{r}: der Klick tut nichts");
            assert_eq!(g.reiter(), *r, "{r}: der Klick waehlt ihn nicht");
        }
        // AUSSERHALB: rechts vom Band, unterhalb des Streifens, oberhalb des
        // Bereichs — alles „nichts".
        let sy = g.reiter_streifen().expect("kein Streifen").y;
        assert_eq!(
            g.reiter_unter(1199.0, sy + 13.0),
            None,
            "rechts vom Band ist ein Feld"
        );
        assert_eq!(
            g.reiter_unter(10.0, 799.0),
            None,
            "unter dem Streifen ist ein Feld"
        );
        assert_eq!(
            g.reiter_unter(10.0, 300.0),
            None,
            "im Seitenbereich ist ein Feld"
        );
        assert_eq!(
            g.reiter_unter(-1.0, sy + 13.0),
            None,
            "links vom Fenster ist ein Feld"
        );
        assert_eq!(
            g.reiter_unter(f32::NAN, sy + 13.0),
            None,
            "NaN ist ein Feld"
        );
    }

    /// DAS KREUZ SCHLIESST, UND ES STEHT NICHT AUF DEM LETZTEN FELD.
    ///
    /// GEMESSENE ENTSCHEIDUNG: das Kreuz steht in der LÜCKE zwischen dem
    /// letzten Feld und dem Fensterrand. Der Test prueft, dass seine linke
    /// Kante STRENG GROSSER ist als die rechte Kante des letzten Feldes —
    /// sonst waere die Luecke nicht da und der Fehler waere still.
    #[test]
    fn das_kreuz_schliesst_und_steht_nicht_auf_dem_letzten_feld() {
        let mut g = Geruest::neu(1200, 800);
        g.setze_offen(true);
        let sx = g.schliessen().expect("kein Kreuz");
        let mut band_ende = 0u32;
        for r in GERUEST.iter() {
            band_ende += r.breite();
        }
        assert_eq!(
            sx.x,
            (band_ende + LUFT_VOR_SCHLIESSEN) as f32,
            "das Kreuz klebt am Band"
        );
        // GEMESSEN (dieser Test, 1200 px Fenster): das Band ist 6 * 100 +
        // 132 = 732 px, das Kreuz beginnt bei 740 und endet bei 772. Es
        // steht also NEBEN dem Band, nicht am Fensterrand — das ist die
        // Zusage aus dem Modul-Kopf, und sie wird als Zahl gehalten.
        assert_eq!(band_ende, 732, "die Bandbreite stimmt nicht");
        assert_eq!(sx.right(), 772.0, "das Kreuz steht nicht neben dem Band");
        assert!(
            sx.right() <= g.fenster_breite as f32,
            "das Kreuz ragt aus dem Fenster"
        );
        // UND es liegt auf KEINEM Feld.
        assert_eq!(
            g.reiter_unter(sx.x + 4.0, sx.y + 4.0),
            None,
            "das Kreuz ist ein Feld"
        );
        assert_eq!(
            g.triff(sx.x + 4.0, sx.y + 4.0),
            Some(Treffer::Schliessen),
            "das Kreuz wird nicht getroffen"
        );
        // UND der Klick schliesst.
        assert!(
            g.klick(sx.x + 4.0, sx.y + 4.0),
            "der Klick aufs Kreuz tut nichts"
        );
        assert!(!g.offen, "das Kreuz schliesst nicht");
    }

    /// BOMBEN (M8): FENSTERGRÖSSEN UM 0 UND UM u32::MAX.
    ///
    /// GEMESSENE BEGRÜNDUNG: `fenster_breite`/`fenster_hoehe` kommen vom
    /// Fensterereignis. Die Enden sind 0 (kein Fenster), `u32::MAX` (ein
    /// Fehler im Fenstersystem) und 132 (schmaler als das Band).
    #[test]
    fn bomben_fenster_kommen_mit_einem_benannten_ergebnis() {
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (132, 26),
            (200, 200),
            (400, 800),
            (16000, 16000),
            (u32::MAX, u32::MAX),
        ] {
            let mut g = Geruest::neu(b, h);
            g.setze_offen(true);
            // Kein Panic, und wenn es einen Bereich gibt, ist er im Fenster.
            if let Some(f) = g.bereich() {
                assert!(
                    f.width <= b.min(MAX_FENSTER) as f32,
                    "bei {b}x{h}: zu breit"
                );
                assert!(
                    f.height <= h.min(MAX_FENSTER) as f32,
                    "bei {b}x{h}: der Bereich ({} px) ist hoeher als das Fenster ({h} px)",
                    f.height
                );
                assert!(
                    f.height >= MIN_BEREICH_HOEHE as f32,
                    "bei {b}x{h}: zu niedrig dafuer dass es ihn gibt"
                );
                assert!(
                    f.bottom() <= h as f32,
                    "bei {b}x{h}: der Bereich ragt unten heraus"
                );
            } else {
                // Ohne Bereich: die Seite hat das ganze Fenster. Das ist das
                // Ergebnis bei 0x0, 1x1 und 26x26.
                assert!(
                    h < MIN_BEREICH_HOEHE,
                    "bei {b}x{h}: kein Bereich, obwohl {h} px Hoehe reichen"
                );
            }
            if let Some(s) = g.schliessen() {
                assert!(
                    s.right() <= b.min(MAX_FENSTER) as f32,
                    "bei {b}x{h}: Kreuz ragt heraus"
                );
            }
            // Die Reiterleiste ist sieben Felder breit, aber die GEOMETRIE
            // darf nicht ueber das Fenster hinauslaufen.
            let mut kante = 0u32;
            for r in GERUEST.iter() {
                kante += r.breite();
            }
            assert_eq!(
                kante,
                GERUEST.iter().map(|r| r.breite()).sum::<u32>(),
                "die Bandbreite ist nicht addierbar"
            );
            // UND die Zeilenzahl ist eine Zahl, kein NaN-Panic.
            let z = g.zeilen();
            assert!(
                z as usize <= crate::grenzen::Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen,
                "bei {b}x{h}: {z} Zeilen"
            );
            for i in [0u32, 1, z, z + 1, u32::MAX] {
                let _ = g.zeile_y(i);
            }
            let _ = g.inhalt_unter(0.0, 0.0);
            let _ = g.inhalt_unter(f32::NAN, 13.0);
            let _ = g.inhalt_unter(f32::MAX, f32::MIN);
        }
    }
}
