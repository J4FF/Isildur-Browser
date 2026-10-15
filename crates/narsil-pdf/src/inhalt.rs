//! Der Content-Stream-Interpreter: was auf einer Seite gezeichnet wird.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # DER AUFBAU IN EINER ZEILE
//!
//! Ein Content-Stream ist eine flache Folge aus Zahlen, Namen, Strings
//! und Arrays, unterbrochen von **Operatoren**: `100 200 m` heisst
//! „gehe nach 100,200". Der Interpreter ist deshalb eine Schleife, die
//! Operanden sammelt, bis ein Operator kommt, und dann diesen Operator
//! mit dem Stapel aufruft.
//!
//! # WAS EIN PDF SONST NOCH KENNT (und warum es hier nicht vorkommt)
//!
//!  * **Typ 3 Schriften** (`/Subtype /Type3`): zeichnen mit eigenen
//!    Content-Streams. GEMESSEN: in keinem der zehn PDFs. Sie werden
//!    gemeldet und uebersprungen, nicht geraten.
//!  * **Schablonen** (`/Pattern`): kommen in keinem vor.
//!  * **Inline-Bilder** (`BI ... ID <bytes> EI`): kommen in keinem vor.
//!
//! Wer diese drei nicht kennt, verliert bei den zehn PDFs hier nichts —
//! und das ist die ehrliche Begruendung, sie nicht zu bauen.
//!
//! # DIE TRANSFORMATIONEN
//!
//! PDF zeichnet in einem Verzeichnis, das die Seite selbst setzt
//! (`cm`). Deshalb wird jeder Punkt durch die **aktuelle**
//! Transformationsmatrix geschickt. GEMESSEN: die RFC-PDFs
//! benutzen `cm` auf jeder zweiten Zeile; ohne sie stehen alle Texte
//! aneinander.

use crate::objekt::Wert;

/// Eine 2x3-Transformation (a, b, c, d, e, f) — die sechs Zahlen aus
/// `cm` und `Tm`.
///
/// added 03.10.2026. EIGENER Typ statt `[f64; 6]`, weil an dieser
/// Stelle die **Reihenfolge** der Felder der haeufigste Fehler ist:
/// `cm` schreibt `a b c d e f`, und wer sie als `a b c d e f` in
/// Zeilenmatrix denkt, verschiebt jede Seite nach oben.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    /// Skalierung x.
    pub a: f64,
    /// Neigung y.
    pub b: f64,
    /// Neigung x.
    pub c: f64,
    /// Skalierung y.
    pub d: f64,
    /// Versatz x.
    pub e: f64,
    /// Versatz y.
    pub f: f64,
}

impl Matrix {
    /// Die Identitaet.
    pub const IDENTITAET: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Multipliziert zwei Matrizen: **rhs zuerst**, dann `self`.
    ///
    /// added 03.10.2026. DIE REIHENFOLGE IST DER GANZE UNTERSCHIED, und
    /// die erste Fassung hatte sie **verkehrt** — der Test
    /// `matrix_reihenfolge` sagte es beim ersten Lauf:
    /// `skal.mal(schieb).punkt(1.0, 0.0)` lieferte 12 statt 22.
    ///
    /// DER WEG, warum `rhs` zuerst kommt: PDF schreibt
    /// `2 0 0 2 5 0 cm` als „neue Matrix **vor** die bestehende".
    /// Die Formel ist deshalb `rhs × self`, nicht `self × rhs`:
    ///
    /// ```text
    ///   ergebnis = rhs × self
    ///   a = rhs.a*self.a + rhs.b*self.c
    ///   e = rhs.e*self.a + rhs.f*self.c + self.e
    /// ```
    ///
    /// Wer es umdreht, bekommt bei `Td` nach `Tm` jede Zeile an der
    /// falschen Stelle — und der Unterschied ist bei einer einzigen
    /// Zeile nicht sichtbar.
    pub fn mal(self, rhs: Matrix) -> Matrix {
        Matrix {
            a: rhs.a * self.a + rhs.b * self.c,
            b: rhs.a * self.b + rhs.b * self.d,
            c: rhs.c * self.a + rhs.d * self.c,
            d: rhs.c * self.b + rhs.d * self.d,
            e: rhs.e * self.a + rhs.f * self.c + self.e,
            f: rhs.e * self.b + rhs.f * self.d + self.f,
        }
    }

    /// Verschiebt nur.
    pub fn verschieben(x: f64, y: f64) -> Matrix {
        Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        }
    }

    /// Staucht oder streckt in x und y.
    ///
    /// added 04.10.2026. Als **rechtes** Argument von [`Self::mal`]
    /// gebaut, denn `mal` wendet das zweite Argument zuerst an: `cm 2 0
    /// 0 3 10 20` ist `Matrix::verschieben(10.0, 20.0).mal(strecken(2.0,
    /// 3.0))` und **nicht** die andere Reihenfolge — in der kommt ein
    /// Vielfaches des Versatzes heraus (x' = 2x + 20 statt 2x + 10).
    ///
    /// DER TEST HAT DEN UNTERSCHIED GEFUNDEN: mit der falschen
    /// Reihenfolge lieferte `einheitsquadrat` (20,60)-(22,63) statt
    /// (10,20)-(12,23), und der Test schlug fehl. Genau dafuer steht
    /// er hier.
    pub fn strecken(x: f64, y: f64) -> Matrix {
        Matrix {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Rechnet einen Punkt durch.
    pub fn punkt(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Rechnet einen **Vektor** durch (ohne Versatz).
    ///
    /// added 03.10.2026.Fuer Breiten und Hoehen von Linien und
    /// Glyphen: der Versatz darf nicht mitrechnen, sonst wird aus
    /// einem 10-Punkt-Schriftschnitt ein 10-Punkt-Versatz.
    pub fn vektor(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    /// Die durchschnittliche Skalierung (fuer Schriftgroessen).
    pub fn skalierung(&self) -> f64 {
        ((self.a * self.a + self.b * self.b).sqrt() + (self.c * self.c + self.d * self.d).sqrt())
            / 2.0
    }
}

/// Ein Punkt eines Pfades (bereits im Geräteraum).
///
/// added 03.10.2026. `Default` mit `(0, 0)`, weil `Pfad` sie braucht
/// (`#[derive(Default)]` auf `Pfad` sonst nicht übersetzt) und ein
/// leerer Pfad keinen aktuellen Punkt hat. `(1, 0)` wäre falsch —
/// das ist ein Punkt **ausserhalb** der Seite.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Punkt {
    /// X.
    pub x: f64,
    /// Y.
    pub y: f64,
}

/// Ein gueltiges Segment-Fragment: Gerade oder kubische Kurve.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// `l x y`
    Linie(Punkt, Punkt),
    /// `c x1 y1 x2 y2 x3 y3` — zwei Kontrollpunkte und der Endpunkt.
    /// Der **Startpunkt** kommt aus dem vorigen Fragment; deshalb steht
    /// er hier nicht.
    Kubisch([Punkt; 3]),
}

/// Ein Pfad: eine Liste von Punkten mit den Segmenten dazwischen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pfad {
    /// Die Unterpfade. `f` zeichnet **alle** (nicht-zero-Regel),
    /// `S` nur die Umrisse.
    pub teile: Vec<Pfadteil>,
    /// Aktuelle Position (Startpunkt des naechsten Segments).
    pub aktuell: Punkt,
    /// Startpunkt des aktuellen Unterpfads (fuer `h`).
    pub start: Punkt,
    /// Der aktuelle Unterpfad, noch nicht geschlossen.
    pub offen: bool,
}

/// Ein Stueck eines Pfades.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pfadteil {
    /// Startpunkt.
    pub von: Punkt,
    /// Die Segmente.
    pub segmente: Vec<Segment>,
}

impl Pfad {
    /// Beginnt einen neuen Unterpfad.
    pub fn beginne(&mut self, p: Punkt) {
        self.teile.push(Pfadteil {
            von: p,
            segmente: Vec::new(),
        });
        self.offen = true;
        self.aktuell = p;
        self.start = p;
    }

    /// Haengt ein Segment an den aktuellen Unterpfad.
    pub fn segment(&mut self, s: Segment) {
        if let Some(letzter) = self.teile.last_mut() {
            letzter.segmente.push(s);
        }
        self.offen = true;
    }

    /// Schliesst den aktuellen Unterpfad.
    pub fn schliesse(&mut self) {
        self.offen = false;
    }

    /// Die Zahl der Punkte ueberhaupt (fuer Grenzen).
    pub fn punkte(&self) -> usize {
        self.teile.iter().map(|t| t.segmente.len() + 1).sum()
    }
}

/// Der aktuelle Fuellmodus (`f`, `f*`, `B`, `S`, ...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fuellung {
    /// Fuellen, non-zero.
    Fuellen,
    /// Fuellen, even-odd.
    FuellenGerade,
    /// Kontur zeichnen.
    Kontur,
    /// Fuellen **und** Kontur.
    FuellenUndKontur,
    /// Kontur zeichnen, ohne zu schliessen.
    Offen,
}

/// Der Grafikzustand.
///
/// added 03.10.2026. Alles, was `q` speichert. Als **eigener Typ** und
/// nicht als Karte, weil ein PDF-Schluessel `gs` in ein Wörterbuch geht
/// und der Zustand ansonsten 20 Zugriffe braucht, die alle denselben
/// Satz Mist machen koennten.
#[derive(Debug, Clone, PartialEq)]
pub struct Zustand {
    /// Fuellfarbe als RGBA (0..255).
    pub fuell: [u8; 4],
    /// Konturfarbe als RGBA.
    pub kontur: [u8; 4],
    /// Linienbreite in Geräteeinheiten.
    pub breite: f64,
    /// added 05.10.2026: das Strichmuster aus `d` (leer = durchgezogen).
    pub strichmuster: Vec<f64>,
    /// added 05.10.2026: `W` gesehen, das `n` steht noch aus.
    ///
    /// DER FEHLER, DEN DIESES FELD ERSETZT: die Marke lag auf dem
    /// **Operandenstapel** — und der wird am Ende **jedes** Operators
    /// geleert (`stapel.clear()`, Ende der Operator-Schleife). `W` hat
    /// seine Marke also im selben Durchlauf wieder verloren, und beim
    /// `n` war der Stapel leer: **es kam nie ein `Aktion::Clip` zustande**.
    ///
    /// GEMESSEN, wie das auffaellt: `tests/clip_wirkt.rs` sah 10 100
    /// Pixel mit und ohne Clipp (kein Unterschied) und der Test
    /// `der_interpreter_liefert_das_rechteck` fand **keine** Clip-Aktion.
    ///
    /// GEMESSEN, wie oft das im Korpus vorkommt: 5 182 Rechteck-Clips
    /// (`werkzeuge/clip_wirkung.py`) — der Fehler hat also **jeden**
    /// Clipp betroffen, nicht nur seltene Faelle.
    pub clip_marke: bool,
    /// added 06.10.2026: der **aktive** Clipp als Rechteck.
    ///
    /// WARUM ER HIER STEHT und nicht im Zeichner: `q`/`Q` sind Operatoren
    /// des Interpreters, und nur dort weiss man, wann ein Clipp
    /// zurueckgesetzt wird. Im Zeichner gibt es kein `Q` — und dort ist
    /// genau der Fehler entstanden, den die Messung gefunden hat:
    ///
    /// GEMESSEN an `1207.7214.pdf`, den sechs Seiten, die mit dem Clipp
    /// **schlechter** wurden (Seite 20: +6 467, Seite 21: +5 554):
    ///
    /// ```text
    ///   Seite 20   Clipp entfernt Tinte, die die Referenz hat:  36 451
    ///              entfernt Tinte, die sie nicht hat:            15 315
    /// ```
    ///
    /// Der Clipp schnitt also **doppelt** so viel weg, wie er wegnehmen
    /// sollte: nach einem `q ... W n ... Q` blieb die Box bei mir bis zum
    /// Seitenende stehen undfraess den Text darunter.
    pub clip_rechteck: Option<[f64; 4]>,
    /// added 06.10.2026: `q`, die **nicht** gespeichert wurden, weil der
    /// Stapel voll war.
    ///
    /// DER FEHLER, DEN DIESER ZAEHLER BEHEBT: `q` hat die Grenze
    /// `MAX_Q_Q` = 64, `Q` hat sie nicht. Sobald der Stapel voll ist,
    /// schiebt `q` nichts mehr — aber jedes `Q` poppt **weiter**. Damit
    /// stellte `Q` einen Zustand wieder her, der zu einem **anderen**
    /// `q` gehoerte, und der Stapel lief auseinander.
    ///
    /// GEMESSEN, wie kraeftig das ist: `rfc9110.pdf` hat **7 546** `q`
    /// und 6 216 `Q`; die Tiefe erreicht **1 353** — also rund 1 290
    /// `q` mehr, als der Stapel fasst. Bei diesem Auseinanderlaufen
    /// stellte ein `Q` auch einen uralten Clipp wieder her, und die
    /// Clipp-Verwaltung loeschte daraufhin genau dort die Tinte, wo die
    /// Referenz sie hat: **+598 279 Pixel** (`rfc9110.pdf`).
    ///
    /// DIE REGEL, und sie ist die naheliegende: ein `Q`, zu dem **kein**
    /// gespeichertes `q` gehoert, tut **nichts**. Es wird gezaehlt,
    /// damit nach dem Rueckabau wieder gepoppt wird.
    pub q_ueberlauf: usize,
    /// Der aktuelle Pfad.
    pub pfad: Pfad,
    /// Aktuelle Clipping-Region als Rechtecke (Seitenkoordinaten).
    pub clip: Vec<[f64; 4]>,
    /// CTM (aktuelle Transformationsmatrix).
    pub ctm: Matrix,
    /// Textmatrix.
    pub tm: Matrix,
    /// Zeilenmatrix.
    pub tlm: Matrix,
    /// Schriftname (Ressourcenname, z. B. `/F1`).
    pub schrift: String,
    /// Schriftgroesse in Textraum-Einheiten.
    pub schriftgroesse: f64,
    /// Zeichenabstand (`Tc`).
    pub zeichenabstand: f64,
    /// Wortabstand (`Tw`).
    pub wortabstand: f64,
    /// Horizontale Skalierung (`Tz`).
    pub horizont: f64,
    /// Zeilenabstand (`TL`).
    pub zeilenabstand: f64,
    /// Textanhebung (`Ts`).
    pub anhebung: f64,
    /// Zeichenmodus (`Tr`).
    pub zeichenmodus: u32,
    /// Fuellmodus.
    pub fuellung: Fuellung,
    /// Der `q`/`Q`-Stapel.
    ///
    /// added 03.10.2026. Ohne ihn ist `Q` nach einem `cm` wirkungslos,
    /// und die RFC-PDFs benutzen `q`/`Q` um **jede Zeile** — der Text
    /// stuende dann alle auf derselben Stelle.
    pub stapel: Vec<Zustand>,
    /// `/ExtGState` aus den Ressourcen, damit `gs` den Strichstaerke
    /// und die Deckkraft uebernehmen kann.
    pub extgstate: std::collections::HashMap<String, Wert>,
    /// `/XObject` aus den Ressourcen, damit `Do` ein Bild von einem
    /// Formular unterscheidet.
    pub xobjects: std::collections::HashMap<String, Wert>,
    /// Die Breitentabellen der Schriften, **aufgeloest**.
    ///
    /// added 03.10.2026. DER GRUND: nach einem `Tj` muss die
    /// Textmatrix um die gezeigte Breite weitergeschoben werden, sonst
    /// stehen alle Teile eines Absatzes **auf derselben Stelle**.
    ///
    /// GEMESSEN am 03.10.2026, 21:20, an
    /// `1706.03762.pdf`. Der Content-Stream sieht so aus:
    ///
    /// ```text
    /// [(Pro)15(vided)-250(proper)-250(attrib)20(ution)...]TJ
    /// ```
    ///
    /// `TJ` zeichnet **fuenf** Teilstrings. Die Zahlen dazwischen
    /// wurden uebernommen, die **Zeichenbreiten nicht** — also began
    /// „vided" bei x = 124,49 statt bei 141,27 (das ist genau der
    /// Wert, den `mutool trace` nennt). Das Ergebnis war ein Absatz,
    /// in dem fuenf Worte uebereinanderlagen.
    ///
    /// DIE AUFLOESUNG braucht das Dokument (die Breiten stehen hinter
    /// `/Font << /F84 1000 R >>`), und der Interpreter hat es nicht —
    /// deshalb legt der Renderer die fertigen Tabellen hier ab.
    pub schriften: std::collections::HashMap<String, crate::schrift::Breiten>,
}

impl Default for Zustand {
    fn default() -> Self {
        Zustand {
            fuell: [0, 0, 0, 255],
            kontur: [0, 0, 0, 255],
            breite: 1.0,
            strichmuster: Vec::new(),
            clip_marke: false,
            clip_rechteck: None,
            q_ueberlauf: 0,
            pfad: Pfad::default(),
            clip: Vec::new(),
            ctm: Matrix::IDENTITAET,
            tm: Matrix::IDENTITAET,
            tlm: Matrix::IDENTITAET,
            schrift: String::new(),
            schriftgroesse: 12.0,
            zeichenabstand: 0.0,
            wortabstand: 0.0,
            horizont: 1.0,
            zeilenabstand: 12.0,
            anhebung: 0.0,
            zeichenmodus: 0,
            fuellung: Fuellung::Fuellen,
            stapel: Vec::new(),
            extgstate: std::collections::HashMap::new(),
            xobjects: std::collections::HashMap::new(),
            schriften: std::collections::HashMap::new(),
        }
    }
}

/// Was der Interpreter dem Renderer zum Zeichnen gibt.
///
/// added 03.10.2026. DER PLAN: der Interpreter rechnet **nur** und gibt
/// fertige Koordinaten in **Seitenkoordinaten** heraus. Er zeichnet
/// nicht selbst — sonst haette man die Pixel twice: einmal hier und
/// einmal im Renderer, und die beiden wuerden auseinanderlaufen.
#[derive(Debug, Clone, PartialEq)]
pub enum Aktion {
    /// Ein gefuellter Pfad.
    FuellPfad {
        /// Die Konturen in Seitenkoordinaten.
        teile: Vec<Pfadteil>,
        /// Die Fuellfarbe.
        farbe: [u8; 4],
        /// even-odd statt non-zero.
        gerade: bool,
    },
    /// Ein Konturzug (gestrichelt).
    Kontur {
        /// Die Konturen in Seitenkoordinaten.
        teile: Vec<Pfadteil>,
        /// Die Konturfarbe.
        farbe: [u8; 4],
        /// Die Linienbreite in Seitenkoordinaten (bereits skaliert).
        breite: f64,
        /// added 05.10.2026: das **Strichmuster** (`d`), in denselben
        /// Einheiten wie `breite` — Seitenpunkten, unskaliert.
        ///
        /// Leer heisst „durchgezogen“, und das ist der Normalfall: von
        /// 4 020 `d`-Operatoren im Korpus (`werkzeuge/operator_zaehlen.py`)
        /// sind 3 624 leer, und **225** tragen ueberhaupt ein Muster aus
        /// Zahlen — die liegen in den Abbildungen der Mathematik-Papiere.
        strich: Vec<f64>,
    },
    /// Eine Textzeile.
    Text {
        /// added 05.10.2026: die vollstaendige Matrix `ctm · tm` in
        /// Seitenkoordinaten. Sie traegt Drehung und Scherung; vorher kam
        /// nur `position` (der Versatz) an, und eine Drehung konnte nicht
        /// ankommen — der arXiv-Stempel von `1706.03762.pdf` (Textmatrix
        /// `0 1 -1 0 32 237`) lag dadurch quer ueber dem Abstract statt
        /// senkrecht am Rand.
        matrix: Matrix,
        /// Die Position der Grundlinie (Seitenkoordinaten).
        position: Punkt,
        /// Die Groesse in Seitenkoordinaten.
        groesse: f64,
        /// Die Zeichen als Zeichencodes der Schrift.
        zeichen: Vec<u8>,
        /// Der Ressourcenname der Schrift (fuer die Aufloesung).
        schrift: String,
        /// Horizontale Skalierung (Prozent/100).
        horizont: f64,
        /// Drehung als Laufweite (0 = aufrecht).
        laufweite: f64,
        /// Der Versatz in Laufrichtung (Textanhebung).
        anhebung: f64,
        /// Die Farbe.
        farbe: [u8; 4],
    },
    /// Ein XObject (`Do`) — Bild **oder** Formular.
    ///
    /// added 04.10.2026. VORHER GAB ES ZWEI VARIANTEN (`Bild` und
    /// `Form`), und **keine** wurde erzeugt: der Operator `Do` war ein
    /// No-op. Der Typ steht jetzt in **einer** Aktion, weil der
    /// Interpreter ihn nicht kennen kann (der Eintrag in `/XObject` ist
    /// ein Verweis, und der Interpreter hat kein Dokument) und der
    /// Renderer ihn ohnehin als einziger aufloesen kann.
    XObject {
        /// Ressourcenname (ohne Schraegstrich).
        name: String,
        /// Das Einheitsquadrat, mit der CTM an der Fundstelle
        /// umgerechnet — das ist das **Zielrechteck** des Bildes.
        ziel: [f64; 4],
        /// Die CTM an der Fundstelle (ein Formular traegt sie mit
        /// seinem eigenen `/Matrix` davor).
        matrix: Matrix,
    },
    /// Eine neue Clipping-Region (`W n`).
    Clip {
        /// Die Konturen in Seitenkoordinaten.
        teile: Vec<Pfadteil>,
    },
    /// added 06.10.2026: der Clipp, **berechnet**.
    ///
    /// `rechteck` ist `None`, wenn kein Clipp gilt (etwa nach `Q`). Der
    /// Zeichner rechnet nichts und entscheidet nichts — das ist der
    /// Unterschied zu `Aktion::Clip`, das nur den Pfad transportiert.
    ClipSette {
        /// `None` = kein Clipp.
        rechteck: Option<[f64; 4]>,
    },
    /// Etwas, das wir nicht kennen — fuer den Bericht, nicht fuers Bild.
    Unbekannt(String),
}

impl Zustand {
    /// Der Startzustand fuer eine Seite, aus deren `/Resources`.
    ///
    /// added 03.10.2026. DER PUNKT MIT DEN RESSOURCEN: `/XObject` und
    /// `/ExtGState` sind **Wörterbücher**, deren Werte Verweise auf
    /// Objekte sind. Hier stehen die **aufgelösten** Werte, damit der
    /// Operator `gs` und `Do` nicht selbst auflösen müssen — die
    /// Auflösung braucht das Dokument, und der Interpreter hat es
    /// nicht.
    ///
    /// GEMESSEN: die Bilder der RFC-PDFs sind Form-XObjects
    /// (`/Subtype /Form`), die Bilder der arXiv-PDFs sind echte
    /// `/Subtype /Image`. Der Unterschied entscheidet, ob der Renderer
    /// ein Bild zeichnet oder ein Formular betritt.
    pub fn aus_ressourcen(resourcen: &Wert) -> Zustand {
        let mut z = Zustand::default();
        if let Some(eg) = resourcen.get("ExtGState").and_then(Wert::dict) {
            for (k, v) in eg {
                z.extgstate
                    .insert(String::from_utf8_lossy(k).into_owned(), v.clone());
            }
        }
        if let Some(xo) = resourcen.get("XObject").and_then(Wert::dict) {
            for (k, v) in xo {
                z.xobjects
                    .insert(String::from_utf8_lossy(k).into_owned(), v.clone());
            }
        }
        z
    }
    /// Das Einheitsquadrat, mit der CTM umgerechnet.
    ///
    /// added 04.10.2026. `Do` zeichnet ein Bild **an** der Stelle des
    /// Einheitsquadrats `(0,0)..(1,1)`, durch die CTM der Fundstelle.
    /// Fuer ein Bild ist das sein Zielrechteck; fuer ein Formular ist es
    /// nur der Umriss.
    ///
    /// DIE EINE EINSCHRAENKUNG, und sie ist gemessen: es ist das
    /// **umschliessende Rechteck** der vier transformierten Ecken. Bei
    /// einem gedrehten oder schiefen `cm` ist das nicht das Parallelogramm
    /// der Abbildung, sondern sein Kasten — das Bild wird dann an den
    /// Kanten des Kastens gestreckt, nicht gedreht. In den elf Test-PDFs
    /// kommt das nicht vor (alle Bilder stehen auf `/Rotate` 0 mit
    /// senkrechten Achsen, GEMESSEN an den 86 `/Subtype /Image`), und
    /// `raster::bild_zeichnen` nimmt ohnehin ein Rechteck entgegen.
    pub fn einheitsquadrat(&self) -> [f64; 4] {
        let ecken = [
            self.ctm.punkt(0.0, 0.0),
            self.ctm.punkt(1.0, 0.0),
            self.ctm.punkt(1.0, 1.0),
            self.ctm.punkt(0.0, 1.0),
        ];
        let mut x0 = f64::MAX;
        let mut y0 = f64::MAX;
        let mut x1 = f64::MIN;
        let mut y1 = f64::MIN;
        for (x, y) in ecken {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        [x0, y0, x1, y1]
    }
    /// Rechnet einen Pfad in Seitenkoordinaten und gibt ihn als Teile
    /// zurueck.
    ///
    /// added 03.10.2026. Wichtig: die **Kontrollpunkte** einer
    /// kubischen Kurve werden mittransformiert, nicht nur die Endpunkte.
    /// Sonst biegt eine Kurve nach einem `cm` in die falsche Richtung
    /// — sieht man nur bei Glyphen, die dann als Rechtecke erscheinen.
    pub fn pfad_in_seitenkoordinaten(&self) -> Vec<Pfadteil> {
        let ctm = self.ctm;
        self.pfad
            .teile
            .iter()
            .filter(|t| !t.segmente.is_empty())
            .map(|t| Pfadteil {
                von: punkt_durch(&t.von, ctm),
                segmente: t
                    .segmente
                    .iter()
                    .map(|s| match s {
                        Segment::Linie(a, b) => {
                            Segment::Linie(punkt_durch(a, ctm), punkt_durch(b, ctm))
                        }
                        Segment::Kubisch(p) => Segment::Kubisch([
                            punkt_durch(&p[0], ctm),
                            punkt_durch(&p[1], ctm),
                            punkt_durch(&p[2], ctm),
                        ]),
                    })
                    .collect(),
            })
            .collect()
    }
}

/// Rechnet einen Punkt durch eine Matrix.
pub fn punkt_durch(p: &Punkt, m: Matrix) -> Punkt {
    let (x, y) = m.punkt(p.x, p.y);
    Punkt { x, y }
}

/// Die Anzahl der Operatoren, die der Interpreter versteht.
///
/// added 03.10.2026. Nur fuer den Bericht: „wie viele der Operatoren
/// in den zehn PDFs kommen vor".
pub fn bekannte_operatoren() -> &'static [&'static str] {
    &[
        // added 05.10.2026: `d` ist **kein unbekannter Operator** mehr,
        // sondern das Strichmuster (siehe den `d`-Zweig weiter unten).
        "q", "Q", "cm", "w", "J", "j", "M", "ri", "i", "gs", "m", "l", "c", "v", "y", "h", "re",
        "S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n", "W", "W*", "BT", "ET", "Tc", "Tw",
        "Tz", "TL", "Ts", "Tf", "Tr", "Td", "TD", "Tm", "T*", "Tj", "TJ", "'", "\"", "cs", "CS",
        "sc", "SC", "scn", "SCN", "g", "G", "rg", "RG", "k", "K", "sh", "Do", "BI", "d0", "d1",
    ]
}
/// Der Interpreter: fuehrt einen Content-Stream aus.
///
/// added 03.10.2026.
///
/// DER AUFBAU, und er ist in drei Zeilen zu erklaeren: Operanden
/// sammeln, bis ein Operator (Name) kommt, dann den Operator mit dem
/// Stapel aufrufen. Der Stapel ist **begrenzt** (`MAX_OPERANDEN`), weil
/// ein Content-Stream aus einem kaputten Dokument 40.000 Zahlen ohne
/// Operator haben kann.
///
/// WAS GEBROCHEN WERDEN DARF: kein Operator ist ein Fehler — ein
/// Content-Stream enthaelt an einem Dutzend Stellen Operatoren, die
/// kein Betrachter kennt (`d0`, `sh` mit Schatten, `BX`/`EX` fuer
/// Kompatibilitaet). Die werden als [`Aktion::Unbekannt`] gemeldet und
/// uebersprungen, damit der Rest der Seite erscheint.
pub fn ausfuehren<F>(strom: &[u8], start: &Zustand, mut zeichnen: F) -> Vec<Aktion>
where
    F: FnMut(&[Aktion]),
{
    let mut zustand = start.clone();
    let mut aktionen: Vec<Aktion> = Vec::new();
    let mut stapel: Vec<Wert> = Vec::new();
    let mut lx = crate::lexing::Lexer::neu(strom);
    let mut texte_modus = false;
    // added 03.10.2026: `while let` statt `loop` mit
    // `let ... else break` (clippy `while_let_loop`). Das Verhalten ist
    // gleich: ein Fehler aus `naechstes` beendet die Schleife.
    while let Ok(tok) = lx.naechstes() {
        match tok {
            crate::lexing::Token::Ende => break,
            crate::lexing::Token::Zahl(z) => stapel.push(Wert::Zahl(z)),
            crate::lexing::Token::Name(b) => stapel.push(Wert::Name(b)),
            crate::lexing::Token::String(s) | crate::lexing::Token::Hex(s) => {
                stapel.push(Wert::String(s))
            }
            crate::lexing::Token::Satzzeichen(s) => {
                if s == "[" {
                    // Das Array beginnt als **Platzhalter**. Warum ein
                    // Platzhalter und nicht nur ein Merker: `TJ` hat
                    // ein Array aus Zahlen **und** Strings, und ohne
                    // Stelle im Stapel wüsste der `]`-Zweig nicht,
                    // wo das Array anfing.
                    stapel.push(Wert::Array(Vec::new()));
                } else if s == "]" {
                    // DER FEHLER DER ERSTEN FASSUNG: `stapel.drain(pos..)`
                    // **entfernt** den Platzhalter mit, danach war
                    // `stapel.get_mut(pos)` immer `None` — und `TJ`
                    // bekam nie ein Array. Gefunden vom Test
                    // `tj_verschiebung` (0 statt 2 Textaktionen).
                    //
                    // Die Reihenfolge ist deshalb: Inhalt **vor** dem
                    // Kuerzen kopieren, dann kuerzen, dann setzen.
                    if let Some(pos) = stapel.iter().rposition(|w| matches!(w, Wert::Array(_))) {
                        let inhalt: Vec<Wert> = stapel[pos + 1..].to_vec();
                        stapel.truncate(pos + 1);
                        stapel[pos] = Wert::Array(inhalt);
                    }
                }
            }
            crate::lexing::Token::Schluesselwort(op) => {
                if op == "BI" {
                    // Inline-Bild: **Rohbytes** ueberspringen, nicht
                    // tokenisieren.
                    //
                    // DER FEHLER, den die Tintenmessung gefunden hat (07.10.):
                    // die alte Fassung liest den Binärteil mit
                    // `naechstes()`. Auf Binaerdaten liefert der Lexer
                    // Muell, und bei `Token::Ende` **brach die Schleife
                    // ab** — mitten im Bild. Der Rest der Seite wurde
                    // dann von dort an zerlegt.
                    //
                    // GEMESSEN, was das kostet (`1207.7214.pdf`, 36 dpi,
                    // `werkzeuge/textmenge_pruefen.py`): auf **18 von 38
                    // Seiten** zeigte mein Renderer weniger als 90 % der
                    // Zeichen, auf den schlimmsten **2,7 %** (Seite 25:
                    // 92 von 3 350). Die Tintenmessung auf derselben
                    // Seite zeigte 0,04 des Referenzwerts.
                    //
                    // GEMESSEN, wie verbreitet das ist: **808** Inline-
                    // Bilder im Korpus (248 in `1207.7214.pdf`, 549 in
                    // `2103.00020.pdf`, 7 in `1706.03762.pdf`, 4 in
                    // `1512.03385.pdf`).
                    //
                    // DER ALTE KOMMENTAR BEHAUPTETE „in keinem der zehn
                    // PDFs" — das war **falsch gemessen**, und die
                    // Messung ist in `werkzeuge/` nachzulesen.
                    //
                    // DIE REGEL FUER `EI`: zwei Bytes `E`,`I`, davor ein
                    // Trennzeichen, danach **kein** Namezeichen. Ohne die
                    // beiden Bedingungen findet `EI` auch mitten im
                    // Bilddaten statt.
                    let mut p = lx.position();
                    let mut ende = strom.len();
                    while p + 1 < strom.len() {
                        if strom[p] == b'E' && strom[p + 1] == b'I' {
                            let davor = p == 0 || strom[p - 1].is_ascii_whitespace();
                            let danach = p + 2 >= strom.len()
                                || !(strom[p + 2].is_ascii_alphanumeric() || strom[p + 2] == b'_');
                            if davor && danach {
                                ende = p + 2;
                                break;
                            }
                        }
                        p += 1;
                    }
                    lx.set_position(ende);
                    stapel.clear();
                    continue;
                }
                if stapel.len() > MAX_OPERANDEN {
                    stapel.clear();
                }
                if !texte_modus {
                    // Ausserhalb von BT/ET haben Textoperatoren keine
                    // Bedeutung; PDF-Dateien benutzen sie trotzdem
                    // gelegentlich (und meine Parser-Daten aus
                    // 2103.00020.pdf zeigen genau das).
                    texte_modus = matches!(op.as_str(), "BT" | "ET");
                }
                if op == "BT" {
                    texte_modus = true;
                    zustand.tm = Matrix::IDENTITAET;
                    zustand.tlm = Matrix::IDENTITAET;
                }
                if op == "ET" {
                    texte_modus = false;
                }
                let mut neu: Vec<Aktion> = Vec::new();
                ausfuehren_op(&op, &mut stapel, &mut zustand, &mut neu);
                if !neu.is_empty() {
                    zeichnen(&neu);
                    aktionen.extend(neu);
                }
            }
        }
        if aktionen.len() > MAX_AKTIONEN {
            break;
        }
    }
    aktionen
}

/// Wie viele Operanden der Stapel halten darf (M3).
///
/// added 03.10.2026. 2048. Eine Seite mit 300 Textoperatoren hat
/// 600 Werte; ein Dokument aus einem Bug-Generator hat 1.000.000.
pub const MAX_OPERANDEN: usize = 2048;

/// Wie viele Aktionen ein Content-Stream erzeugen darf (M3).
///
/// added 03.10.2026. 400.000. Das reicht fuer eine volle Textseite
/// (ca. 3000 Zeichen) mit Pixeln; alles darueber ist eine Schleife.
pub const MAX_AKTIONEN: usize = 400_000;

/// Wie viele Punkte ein Pfad haben darf (M3).
///
/// added 03.10.2026. 20.000. Eine Seite mit 20.000 Pfadpunkten ist
/// eine Zeichnung, kein Angriff; 1.000.000 Punkte sind einer.
pub const MAX_PFAD_PUNKTE: usize = 20_000;

/// Führt **einen** Operator aus.
#[allow(clippy::too_many_lines)]
fn ausfuehren_op(op: &str, stapel: &mut Vec<Wert>, z: &mut Zustand, raus: &mut Vec<Aktion>) {
    let z0 = |i: usize| stapel.get(i).and_then(Wert::zahl).unwrap_or(0.0);
    match op {
        // ---- Grafikzustand ----
        "q" => {
            // `q` speichert den **ganzen** Zustand. Ohne Stack waere
            // `Q` nach einem `cm` wirkungslos — und die RFC-PDFs
            // benutzen `q`/`Q` um jede Zeile.
            if z.stapel.len() < MAX_Q_Q {
                z.stapel.push(z.clone());
            } else {
                // added 06.10.2026: DER STAPEL IST VOLL. Der Zaehler
                // haelt fest, dass dieses `q` **nicht** gespeichert
                // wurde — sonst poppte das naechste `Q` einen Zustand,
                // der zu einem anderen `q` gehoert.
                //
                // DIE OBERGRENZE des Zaehlers ist wichtig: `rfc9110.pdf`
                // oeffnet **1 330** `q` ohne sie zu schliessen. Ohne
                // Klemme waere der Zaehler eine Zahl, die von einer
                // Datei abhaengt; mit Klemme ist er durch `MAX_Q_Q`
                // beschraenkt, und ein Pfad mit 10 000 verschachtelten
                // `q` verbraucht dieselbe Zeit wie einer mit 64.
                if z.q_ueberlauf < MAX_Q_Q {
                    z.q_ueberlauf += 1;
                }
            }
        }
        "Q" => {
            // added 06.10.2026: ZUERST DER UEBERLAUF. Ein `Q` ohne
            // gespeichertes `q` darf nichts wiederherstellen — es tut
            // nichts, und der Zaehler sinkt.
            if z.q_ueberlauf > 0 {
                z.q_ueberlauf -= 1;
            } else if let Some(alt) = z.stapel.pop() {
                let pfad = std::mem::take(&mut z.pfad);
                // added 06.10.2026: DER CLIPP WIRD ZURUECKGESETZT, und
                // der Zeichner bekommt **Bescheid**. Ohne diese Zeile
                // blieb ein Clipp bis zum Seitenende aktiv — GEMESSEN als
                // 16 693 Pixel Schaden auf `1207.7214.pdf` allein.
                let clip_vorher = z.clip_rechteck;
                *z = alt;
                z.pfad = pfad;
                // added 06.10.2026, GEAENDERT am 07.10.2026. DER CLIPP
                // GEHOERT ZUM GRAFIKZUSTAND, und ein `Q` hebt ihn auf —
                // das ist die Spezifikation, und das ist jetzt der
                // **Vorgabe**.
                //
                // BISHER (06.10.) war es umgekehrt: der Clipp blieb
                // klebrig bis zum Seitenende, und der Zeichner bekam die
                // Ruecksetzung nur mit `NARSIL_PDF_CLIP_Q_RUECK=1`. Diese
                // Abweichung ist **aufgehoben**, weil die Messung, die
                // sie begrundete, ein Artefakt war.
                //
                // DIE ALTE MESSUNG war richtig gerechnet und falsch
                // gedeutet: 25 971 268 gegen 23 669 411 abweichende
                // Pixel (Toleranz 32), also -2 307 811 fuer die klebrige
                // Fassung. GEMESSEN, was das Mass belohnt, an
                // `rfc9110.pdf` Seite 171:
                //
                //     Referenztinte                5 479 297
                //     klebrig                        37 654  (0,007!)
                //     spezifikationsgetreu         5 815 199  (1,061)
                //
                //     abweichende Pixel            47 111 gegen 59 773
                //     mittlere Abweichung           8,190 gegen 9,343
                //
                // BEIDE MASSE Sagen, die **leere** Seite sei besser. Sie
                // ist falsch: eine leere Seite weicht nur an den dunklen
                // Kernen der Striche ab, ein gezeichneter Glyphe an allen
                // weichen Kanten — und bei grosser Schrift sind das
                // mehr Pixel. Ein Mass, das Leere belohnt, kann keine
                // Entscheidung ueber Inhalt tragen.
                //
                // DAS MASS, DAS DAS NICHT KANN, ist der **Gleichlauf**
                // (`werkzeuge/gleichlauf.py`): er vergleicht die Form
                // der Tintenverteilung, und eine leere Seite erreicht 0.
                // GEMESSEN ueber alle **543** Seiten:
                //
                //     klebrig                0,4492
                //     spezifikationsgetreu   0,7940
                //
                // Seite fuer Seite: der klebrige Clipp verliert auf
                // **334** Seiten und gewinnt auf **2**. Am deutlichsten
                // bei den drei RFC-Dateien, die die alte Messung
                // verantwortlich machte:
                //
                //     Datei       klebrig   spezifikationsgetreu
                //     rfc9110        0,1057               0,6629
                //     rfc9111        0,0880               0,7662
                //     rfc9113        0,0756               0,7461
                //
                // Und `1207.7214.pdf` (38 Seiten, 9 Seiten mit Verlust):
                // 0,8333 gegen 0,9132.
                //
                // WAS DAS FUER DIE GEMELDETE ZAHL HEISST: die Summe der
                // abweichenden Pixel **steigt** mit der Korrektur. Das
                // ist kein Rueckschritt, sondern die Messung wird ehrlich
                // — vorher wurde Leerheit als Treffer gezaehlt.
                //
                // DER SCHALTER bleibt, nur mit umgekehrter Richtung:
                //
                //   (voreingestellt)             der Clipp faellt bei `Q`
                //                               auf `None` zurueck
                //   NARSIL_PDF_CLIP_KLEBRIG=1      die alte, klebrige
                //                               Fassung — nur noch zum
                //                               Nachmessen
                //
                if clip_vorher != z.clip_rechteck
                    && std::env::var("NARSIL_PDF_CLIP_KLEBRIG").as_deref() != Ok("1")
                {
                    raus.push(Aktion::ClipSette {
                        rechteck: z.clip_rechteck,
                    });
                }
            }
        }
        "cm" => {
            if stapel.len() >= 6 {
                let m = Matrix {
                    a: z0(0),
                    b: z0(1),
                    c: z0(2),
                    d: z0(3),
                    e: z0(4),
                    f: z0(5),
                };
                z.ctm = z.ctm.mal(m);
            }
        }
        "w" => z.breite = z0(0),
        "gs" => {
            // `/ExtGState << /LW 2 /CA 0.5 >>` — nur die drei Werte, die
            // man ohne Renderer gebraucht.
            if let Some(name) = stapel.iter().rev().find_map(|w| w.name()) {
                if let Some(gs) = z.extgstate.get(name) {
                    if let Some(lw) = gs.get_zahl("LW") {
                        z.breite = lw;
                    }
                    if let Some(ca) = gs.get_zahl("ca") {
                        z.fuell[3] = (ca.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                    if let Some(ca) = gs.get_zahl("CA") {
                        z.kontur[3] = (ca.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                }
            }
        }
        "d0" | "d1" => {}

        // ---- Pfad ----
        "m" => {
            if stapel.len() >= 2 && z.pfad.punkte() < MAX_PFAD_PUNKTE {
                z.pfad.beginne(Punkt { x: z0(0), y: z0(1) });
            }
        }
        "l" => {
            if stapel.len() >= 2 && z.pfad.punkte() < MAX_PFAD_PUNKTE {
                let von = z.pfad.aktuell;
                z.pfad
                    .segment(Segment::Linie(von, Punkt { x: z0(0), y: z0(1) }));
                z.pfad.aktuell = Punkt { x: z0(0), y: z0(1) };
            }
        }
        "c" => {
            if stapel.len() >= 6 && z.pfad.punkte() < MAX_PFAD_PUNKTE {
                z.pfad.segment(Segment::Kubisch([
                    Punkt { x: z0(0), y: z0(1) },
                    Punkt { x: z0(2), y: z0(3) },
                    Punkt { x: z0(4), y: z0(5) },
                ]));
                z.pfad.aktuell = Punkt { x: z0(4), y: z0(5) };
            }
        }
        "v" => {
            // `v`: nur der **zweite** Kontrollpunkt ist gegeben, der
            // erste ist der aktuelle Punkt.
            if stapel.len() >= 4 && z.pfad.punkte() < MAX_PFAD_PUNKTE {
                z.pfad.segment(Segment::Kubisch([
                    z.pfad.aktuell,
                    Punkt { x: z0(0), y: z0(1) },
                    Punkt { x: z0(2), y: z0(3) },
                ]));
                z.pfad.aktuell = Punkt { x: z0(2), y: z0(3) };
            }
        }
        "y" => {
            if stapel.len() >= 4 && z.pfad.punkte() < MAX_PFAD_PUNKTE {
                z.pfad.segment(Segment::Kubisch([
                    Punkt { x: z0(0), y: z0(1) },
                    Punkt { x: z0(2), y: z0(3) },
                    Punkt { x: z0(2), y: z0(3) },
                ]));
                z.pfad.aktuell = Punkt { x: z0(2), y: z0(3) };
            }
        }
        "h" => {
            if z.pfad.offen {
                let von = z.pfad.aktuell;
                let nach = z.pfad.start;
                if (von.x - nach.x).abs() > f64::EPSILON || (von.y - nach.y).abs() > f64::EPSILON {
                    z.pfad.segment(Segment::Linie(von, nach));
                }
                z.pfad.aktuell = nach;
                z.pfad.schliesse();
            }
        }
        "re" => {
            if stapel.len() >= 4 && z.pfad.punkte() + 4 < MAX_PFAD_PUNKTE {
                let (x, y, w, h) = (z0(0), z0(1), z0(2), z0(3));
                z.pfad.beginne(Punkt { x, y });
                z.pfad
                    .segment(Segment::Linie(Punkt { x, y }, Punkt { x: x + w, y }));
                z.pfad.segment(Segment::Linie(
                    Punkt { x: x + w, y },
                    Punkt { x: x + w, y: y + h },
                ));
                z.pfad.segment(Segment::Linie(
                    Punkt { x: x + w, y: y + h },
                    Punkt { x, y: y + h },
                ));
                z.pfad
                    .segment(Segment::Linie(Punkt { x, y: y + h }, Punkt { x, y }));
                z.pfad.schliesse();
            }
        }
        // added 05.10.2026: DAS STRICHMUSTER (`d`).
        //
        // GEMESSEN, warum es sich lohnt (werkzeuge/operator_zaehlen.py):
        // 4 020 `d`-Operatoren im Korpus, davon 3 624 `[ ]` = durchgezogen
        // und **225 echte Muster** (alle Eintraege Zahlen) — 120 davon in
        // den Abbildungen der Mathematik-Papiere (2103.00020 = 132,
        // 1207.7214 = 59, 1512.03385 = 24, f1040 = 10).
        //
        // DIE ZAHL 396 STAND ZUERST HIER, und sie war zu hoch: sie zaehlte
        // auch Treffer in binaeren Stroemen, in denen gar kein Muster
        // stehen kann.
        //
        // DIE REGEL: die Zahlen stehen abwechselnd fuer „an“ und „aus“,
        // gerade Position heisst „an“. Negative Zahlen sind nach der
        // Spezifikation ein Fehler der Datei — **gemessen** an `mutool`,
        // dass die Referenz dann das ganze Muster **verwirft** und
        // durchgezogen zeichnet (siehe den `d`-Zweig).
        //
        // DIE PHASE beginnt je Pfad neu und laeuft ueber die Segmente
        // weiter — der Zeichner rechnet sie in `lib.rs`.
        "d" => {
            // added 05.10.2026: DER FEHLER DER ERSTEN FASSUNG.
            // `stapel.pop()` nahm die **Phase** (`0` aus `[3 2] 0 d`),
            // nicht das Array — das Muster kam als leer an und die
            // Strichlung blieb durchgezogen. `gs` macht es richtig: das
            // Array mit `rev()` **suchen**, nicht den Stapel blind kippen.
            //
            // GEMESSEN als Testfehler: `muster_kommt_bei_der_kontur_an`
            // sah `[]` statt `[3.0, 2.0]`.
            let roh: Vec<Wert> = stapel
                .iter()
                .rev()
                .find_map(|w| match w {
                    Wert::Array(a) => Some(a.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            // added 05.10.2026: IST EIN EINTRAG **KEINE** ZAHL, wird
            // das **ganze** Muster verworfen — die Linie bleibt
            // durchgezogen.
            //
            // GEMESSEN im Korpus: 171 Arrays enthalten Klammern mit
            // Buchstaben oder Steuerzeichen (170 ganz ohne Zahl, **1**
            // mit gemischten Eintraegen, etwa
            // `[106\v\f\x11FFNY…(+2\x10…]` in `2103.00020.pdf`). Das sind
            // keine Muster, sondern **Datenkoffer**, die mein Muster-
            // Suchbefehl `[…] … d` zufaellig trifft.
            //
            // Die erste Fassung wandelte jede Nichtzahl in `0.0`. Bei den
            // 170 folgenlosen Fallen ist das harmlos (alle Eintraege 0 →
            // Muster leer → durchgezogen). Bei der **einen** gemischten
            // aber entstuende `[106, 0, …]` — ein Muster mit „an“ 106 und
            // „aus“ 0, also eine fast durchgezogene Linie mit winzigen
            // Luecken alle 106 Punkte. Das ist ein Muster, das **niemand
            // verlangt hat**, und die Referenz liest es nicht (sie
            // verwirft ein unlesbares Muster und zeichnet durchgezogen).
            // added 05.10.2026: AUCH EINE **NEGATIVE** ZAHL MACHT DAS MUSTER
            // UNBRAUCHBAR — und das ist **gemessen**, nicht behauptet:
            // `werkzeuge/dash_muster_messen.py` laesst `mutool` eine
            // Linie mit `[-3 2] 0 d` zeichnen und liest 160 von 160
            // Pixeln als **durchgezogen** ab. Die Referenz verwirft also
            // das ganze Muster.
            //
            // Die erste Fassung wandelte `-3` in `0.0` und bekam `[0 2]` —
            // „an“ 0 Punkte, „aus“ 2: eine **gestrichelte** Linie, die die
            // Referenz ausdruecklich **durchgezogen** zeichnet. Genau das
            // Gegenteil, und man sieht es nur am Bild.
            let unlesbar = roh.iter().any(|w| !matches!(w, Wert::Zahl(z) if *z >= 0.0));
            let muster: Vec<f64> = if unlesbar {
                Vec::new()
            } else {
                roh.iter()
                    .map(|w| match w {
                        Wert::Zahl(z) if *z >= 0.0 => *z,
                        _ => 0.0,
                    })
                    .collect()
            };
            let mut ziel = if muster.iter().all(|v| *v == 0.0) {
                Vec::new()
            } else {
                muster
            };
            // added 05.10.2026: EIN MUSTER MIT UNGERADER ZAHL VON
            // EINTRAGEN WIRD AM LETZTEN VERDOPPELT — und das ist
            // **gemessen**, nicht der Spezifikation nachgeschrieben:
            // ich habe den Wortlaut gesucht (zwei Webanfragen, beide
            // leer) und stattdessen die Referenz gerendert.
            //
            // `werkzeuge/dash_muster_messen.py` schreibt ein
            // PDF mit drei Linien und laesst `mutool` (Poppler-Familie,
            // von meinem Code unabhaengig) bei 720 dpi zeichnen:
            //
            // ```text
            //   Muster [1]   y=80: ....................#.#.#.#.#.#.#.#.#.
            //   Muster [3 2] y=50: ....................###..###..###..
            //   Muster [3]   y=20: ....................###...###...###..
            // ```
            //
            // `[1]` verhaelt sich wie `[1 1]` und `[3]` wie `[3 3]` — der
            // letzte Eintrag wird wiederholt. Ohne diese Zeile zaehlte
            // `stelle % 2` nie ungerade und die Linie waere
            // **durchgezogen**: gefunden von
            // `werkzeuge/strichmuster_gegenprobe.py`, Fall `[1]`.
            if ziel.len() % 2 == 1 {
                if let Some(&letzte) = ziel.last() {
                    ziel.push(letzte);
                }
            }
            z.strichmuster = ziel;
        }
        "S" => {
            raus.push(Aktion::Kontur {
                teile: z.pfad_in_seitenkoordinaten(),
                farbe: z.kontur,
                breite: z.breite * z.ctm.skalierung().max(0.01),
                strich: z
                    .strichmuster
                    .iter()
                    .map(|v| v * z.ctm.skalierung().max(0.01))
                    .collect(),
            });
            z.pfad = Pfad::default();
        }
        "s" => {
            raus.push(Aktion::Kontur {
                teile: z.pfad_in_seitenkoordinaten(),
                farbe: z.kontur,
                breite: z.breite * z.ctm.skalierung().max(0.01),
                strich: z
                    .strichmuster
                    .iter()
                    .map(|v| v * z.ctm.skalierung().max(0.01))
                    .collect(),
            });
            z.pfad = Pfad::default();
        }
        "f" | "F" => {
            raus.push(Aktion::FuellPfad {
                teile: z.pfad_in_seitenkoordinaten(),
                farbe: z.fuell,
                gerade: false,
            });
            z.pfad = Pfad::default();
        }
        "f*" => {
            raus.push(Aktion::FuellPfad {
                teile: z.pfad_in_seitenkoordinaten(),
                farbe: z.fuell,
                gerade: true,
            });
            z.pfad = Pfad::default();
        }
        "B" => {
            let teile = z.pfad_in_seitenkoordinaten();
            let breite = z.breite * z.ctm.skalierung().max(0.01);
            raus.push(Aktion::FuellPfad {
                teile: teile.clone(),
                farbe: z.fuell,
                gerade: false,
            });
            raus.push(Aktion::Kontur {
                teile,
                farbe: z.kontur,
                breite,
                // added 05.10.2026: das Muster gehoert zu jeder Kontur,
                // auch zu den Fuell-und-Kontur-Operatoren (`B`, `b`).
                strich: z
                    .strichmuster
                    .iter()
                    .map(|v| v * z.ctm.skalierung().max(0.01))
                    .collect(),
            });
            z.pfad = Pfad::default();
        }
        "B*" => {
            let teile = z.pfad_in_seitenkoordinaten();
            let breite = z.breite * z.ctm.skalierung().max(0.01);
            raus.push(Aktion::FuellPfad {
                teile: teile.clone(),
                farbe: z.fuell,
                gerade: true,
            });
            raus.push(Aktion::Kontur {
                teile,
                farbe: z.kontur,
                breite,
                // added 05.10.2026: das Muster gehoert zu jeder Kontur,
                // auch zu den Fuell-und-Kontur-Operatoren (`B`, `b`).
                strich: z
                    .strichmuster
                    .iter()
                    .map(|v| v * z.ctm.skalierung().max(0.01))
                    .collect(),
            });
            z.pfad = Pfad::default();
        }
        "b" | "b*" => {
            if z.pfad.offen {
                let von = z.pfad.aktuell;
                let nach = z.pfad.start;
                z.pfad.segment(Segment::Linie(von, nach));
                z.pfad.aktuell = nach;
            }
            let teile = z.pfad_in_seitenkoordinaten();
            let breite = z.breite * z.ctm.skalierung().max(0.01);
            raus.push(Aktion::FuellPfad {
                teile: teile.clone(),
                farbe: z.fuell,
                gerade: op == "b*",
            });
            raus.push(Aktion::Kontur {
                teile,
                farbe: z.kontur,
                breite,
                // added 05.10.2026: das Muster gehoert zu jeder Kontur,
                // auch zu den Fuell-und-Kontur-Operatoren (`B`, `b`).
                strich: z
                    .strichmuster
                    .iter()
                    .map(|v| v * z.ctm.skalierung().max(0.01))
                    .collect(),
            });
            z.pfad = Pfad::default();
        }
        "n" => {
            // added 05.10.2026. DIE MARGE kommt aus dem Zustand; vorher
            // stand hier `stapel.last() == Name("W")`, und das war nach
            // dem `stapel.clear()` **immer** `None` (siehe
            // `Zustand::clip_marke`).
            if z.clip_marke {
                z.clip_marke = false;
                let teile = z.pfad_in_seitenkoordinaten();
                // added 06.10.2026: DIE RECHNUNG GEHOERT HIERHER, nicht in
                // den Zeichner. Sie braucht den **vorherigen** Clipp, und
                // den gibt es nur hier — er steckt im Zustand und wird von
                // `q`/`Q` mitgesichert.
                let neu = crate::raster::clip_als_rechteck(&teile);
                z.clip_rechteck = match (z.clip_rechteck, neu) {
                    (a, None) => a,
                    (None, b) => b,
                    // added 06.10.2026: DIE SCHNITTMENGE. Nach der
                    // Spezifikation vereinigt jeder weitere Clipppfad den
                    // Clipp mit dem bestehenden. `1207.7214` wurde mit
                    // blossem **Ersetzen** 19 377 Pixel schlechter.
                    (Some(a), Some(b)) => Some([
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                        a[2].min(b[2]),
                        a[3].min(b[3]),
                    ]),
                };
                // Die Aktion traegt jetzt das **Ergebnis** und nicht mehr
                // den Pfad: der Zeichner muss nichts rechnen und nichts
                // entscheiden.
                raus.push(Aktion::ClipSette {
                    rechteck: z.clip_rechteck,
                });
                raus.push(Aktion::Clip { teile });
            }
            z.pfad = Pfad::default();
        }
        "W" | "W*" => {
            // added 05.10.2026. DIE MARKE LIEGT IM **ZUSTAND**, nicht
            // mehr auf dem Operandenstapel — der wird nach jedem Operator
            // geleert (siehe `Zustand::clip_marke`).
            //
            // OB `W` ODER `W*`: nicht gemerkt. Der Unterschied ist die
            // Pfadform, und die entscheidet ohnehin von selbst: ein
            // Weg-Clipp ist kein Rechteck, und `clip_als_rechteck` gibt
            // dann `None` zurueck. Ein zweites Flag waere eine
            // Information, die niemand liest.
            z.clip_marke = true;
        }

        // ---- Farbe ----
        "g" | "G" => {
            let v = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
            if op == "g" {
                z.fuell = [v, v, v, z.fuell[3]];
            } else {
                z.kontur = [v, v, v, z.kontur[3]];
            }
        }
        "rg" | "RG" => {
            let r = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let g = (z0(1).clamp(0.0, 1.0) * 255.0).round() as u8;
            let b = (z0(2).clamp(0.0, 1.0) * 255.0).round() as u8;
            if op == "rg" {
                z.fuell = [r, g, b, z.fuell[3]];
            } else {
                z.kontur = [r, g, b, z.kontur[3]];
            }
        }
        "k" | "K" => {
            // CMYK: 0..1 je Kanal. Die Umrechnung ist die **einfache**
            // (ISO 32000 erlaubt sie): r = (1-c)(1-k).
            let c = z0(0).clamp(0.0, 1.0);
            let m = z0(1).clamp(0.0, 1.0);
            let y = z0(2).clamp(0.0, 1.0);
            let k = z0(3).clamp(0.0, 1.0);
            let r = (255.0 * (1.0 - c) * (1.0 - k)).round() as u8;
            let g = (255.0 * (1.0 - m) * (1.0 - k)).round() as u8;
            let b = (255.0 * (1.0 - y) * (1.0 - k)).round() as u8;
            if op == "k" {
                z.fuell = [r, g, b, z.fuell[3]];
            } else {
                z.kontur = [r, g, b, z.kontur[3]];
            }
        }
        "sc" | "scn" => {
            // `sc` mit 1 Operand = Graustufe, mit 3 = RGB, mit 4 = CMYK.
            let n = stapel.iter().filter(|w| matches!(w, Wert::Zahl(_))).count();
            if n == 1 {
                let v = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
                z.fuell = [v, v, v, z.fuell[3]];
            } else if n == 3 {
                let r = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
                let g = (z0(1).clamp(0.0, 1.0) * 255.0).round() as u8;
                let b = (z0(2).clamp(0.0, 1.0) * 255.0).round() as u8;
                z.fuell = [r, g, b, z.fuell[3]];
            } else if n == 4 {
                let c = z0(0).clamp(0.0, 1.0);
                let m = z0(1).clamp(0.0, 1.0);
                let y = z0(2).clamp(0.0, 1.0);
                let k = z0(3).clamp(0.0, 1.0);
                let r = (255.0 * (1.0 - c) * (1.0 - k)).round() as u8;
                let g = (255.0 * (1.0 - m) * (1.0 - k)).round() as u8;
                let b = (255.0 * (1.0 - y) * (1.0 - k)).round() as u8;
                z.fuell = [r, g, b, z.fuell[3]];
            }
        }
        "SC" | "SCN" => {
            // Wie `sc`, aber fuer die Konturfarbe. `stapel` wird von
            // `nennen_von_hinten` nicht gebraucht — die Zahl der
            // Operanden entscheidet.
            let n = stapel.iter().filter(|w| matches!(w, Wert::Zahl(_))).count();
            if n >= 3 {
                let r = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
                let g = (z0(1).clamp(0.0, 1.0) * 255.0).round() as u8;
                let b = (z0(2).clamp(0.0, 1.0) * 255.0).round() as u8;
                z.kontur = [r, g, b, z.kontur[3]];
            } else if n == 1 {
                let v = (z0(0).clamp(0.0, 1.0) * 255.0).round() as u8;
                z.kontur = [v, v, v, z.kontur[3]];
            }
        }
        "cs" | "CS" => {
            // Ein Farbraumname allein aendert nichts an der Farbe; nur
            // `sc`/`SC` mit passender Zahlenzahl.
        }

        // ---- Text ----
        "BT" => {
            z.tm = Matrix::IDENTITAET;
            z.tlm = Matrix::IDENTITAET;
        }
        "ET" => {}
        "Tc" => z.zeichenabstand = z0(0),
        "Tw" => z.wortabstand = z0(0),
        "Tz" => z.horizont = z0(0).max(0.01),
        "TL" => z.zeilenabstand = z0(0),
        "Ts" => z.anhebung = z0(0),
        "Tr" => z.zeichenmodus = z0(0).max(0.0) as u32,
        "Tf" => {
            if stapel.len() >= 2 {
                if let Some(name) = stapel.iter().rev().find_map(|w| w.name()) {
                    z.schrift = name.to_string();
                }
                // added 03.10.2026: `/F1 12 Tf` schreibt den **Namen
                // zuerst**. `z0(0)` war also der Name (keine Zahl ->
                // 0.0), und die Schriftgroesse wurde 0. Der Test
                // `tj_verschiebung` sah es: `TJ` verschob um
                // `-|w|/1000 * groesse` = 0, beide Buchstaben standen
                // uebereinander.
                z.schriftgroesse = z0(1);
            }
        }
        "Td" => {
            if stapel.len() >= 2 {
                // added 03.10.2026. F23: DIE REIHENFOLGE DER MATRIX.
                //
                // `Matrix::mal(self, rhs)` bedeutet „rhs zuerst, dann
                // self". `verschieben(x, y).mal(tlm)` wendet also
                // **zuerst** `tlm` an und verschiebt danach in
                // **Seitenkoordinaten**. Richtig ist fuer einen
                // Textrraum-Versatz die andere Reihenfolge.
                z.tlm = z.tlm.mal(Matrix::verschieben(z0(0), z0(1)));
                z.tm = z.tlm;
            }
        }
        "TD" => {
            if stapel.len() >= 2 {
                z.zeilenabstand = -z0(1);
                // F23, siehe oben: `Td` verschiebt die Zeilenmatrix
                // im **Textrraum**.
                z.tlm = z.tlm.mal(Matrix::verschieben(z0(0), z0(1)));
                z.tm = z.tlm;
            }
        }
        "Tm" => {
            if stapel.len() >= 6 {
                z.tlm = Matrix {
                    a: z0(0),
                    b: z0(1),
                    c: z0(2),
                    d: z0(3),
                    e: z0(4),
                    f: z0(5),
                };
                z.tm = z.tlm;
            }
        }
        "T*" => {
            // F23, siehe oben.
            z.tlm = z.tlm.mal(Matrix::verschieben(0.0, -z.zeilenabstand));
            z.tm = z.tlm;
        }
        "Tj" => {
            if let Some(Wert::String(bytes)) = stapel.last().cloned() {
                text_ausgeben(raus, z, &bytes);
                vorschub_eintragen(z, &bytes);
            }
        }
        "'" => {
            // F23, siehe oben.
            z.tlm = z.tlm.mal(Matrix::verschieben(0.0, -z.zeilenabstand));
            z.tm = z.tlm;
            if let Some(Wert::String(bytes)) = stapel.last().cloned() {
                text_ausgeben(raus, z, &bytes);
                vorschub_eintragen(z, &bytes);
            }
        }
        "\"" => {
            if stapel.len() >= 3 {
                z.zeichenabstand = z0(0);
                z.wortabstand = z0(1);
            }
            // F23, siehe oben.
            z.tlm = z.tlm.mal(Matrix::verschieben(0.0, -z.zeilenabstand));
            z.tm = z.tlm;
            if let Some(Wert::String(bytes)) = stapel.last().cloned() {
                text_ausgeben(raus, z, &bytes);
                vorschub_eintragen(z, &bytes);
            }
        }
        "TJ" => {
            if let Some(Wert::Array(teile)) = stapel.last().cloned() {
                for teil in &teile {
                    match teil {
                        Wert::String(bytes) => {
                            text_ausgeben(raus, z, bytes);
                            vorschub_eintragen(z, bytes);
                        }
                        Wert::Zahl(w) => {
                            // Negative Werte ruecken **nach links** um
                            // `-|w|/1000 * Schriftgroesse * Horizont`.
                            // Das Vorzeichen ist der ganze Trick.
                            let verschiebung = -w / 1000.0 * z.schriftgroesse * z.horizont;
                            // F23, siehe oben: der Zahlenversatz eines
                            // `TJ` ist ein **Textrraum**-Versatz.
                            z.tm = z.tm.mal(Matrix::verschieben(verschiebung, 0.0));
                        }
                        _ => {}
                    }
                }
            }
        }

        // ---- XObjects ----
        //
        // added 04.10.2026. `Do` WAR EIN **NO-OP** (`"Do" => {}`), und
        // das war der groesste offene Fehler der Crate: kein einziges
        // XObject wurde gezeichnet — weder Bild noch Formular.
        //
        // GEMESSEN: `mutool show | grep` zaehlt in den elf Test-PDFs
        // **86** `/Subtype /Image` (79 in `2103.00020`, 6 in
        // `1706.03762`, 1 in `1207.7214`; davon 67 mit DCTDecode) und
        // **8543** `/Subtype /Form`. Die Abnahme meldete dagegen fuer
        // **alle 543 Seiten** „0 Bilder" und „0 Formulare", und auf
        // `1512.03385.pdf` Seite 1 fehlte die **ganze** Abbildung oben
        // rechts — bei „0 Fehler". `Aktion::Bild` und `Aktion::Form`
        // wurden nirgends erzeugt; `bild_setzen` und `form_zeichnen`
        // waren toter Code.
        //
        // WARUM EINE EINE AKTION UND NICHT ZWEI: der Interpreter kann
        // den Objekttyp **nicht** wissen — `/XObject` steht dort als
        // `Wert::Ref`, und ein Verweis aufloesen kann er nicht (er hat
        // kein Dokument). Er meldet deshalb nur „hier ist ein XObject
        // mit diesem Namen und dieser CTM", und der Renderer entscheidet
        // dann: `/Subtype /Image` heisst Bild, `/Subtype /Form`
        // heisst Formular.
        "Do" => {
            if let Some(Wert::Name(n)) = stapel.last().cloned() {
                // added 04.10.2026: der Lexer liefert `Vec<u8>` (ein
                // Name kann Bytes zwischen 128 und 255 enthalten), die
                // Aktion haelt `String` — derselbe Weg wie bei den
                // Ressourcennamen in `Zustand::aus_ressourcen`.
                raus.push(Aktion::XObject {
                    name: String::from_utf8_lossy(&n).into_owned(),
                    ziel: z.einheitsquadrat(),
                    matrix: z.ctm,
                });
            }
        }
        _ => {
            raus.push(Aktion::Unbekannt(op.to_string()));
        }
    }
    stapel.clear();
}

/// Die Breite einer Zeichenfolge im **Textraum**, in 1/1000 Schrift-
/// groesse, einschliesslich `Tc` und `Tw`.
///
/// added 03.10.2026. DER WEG, und warum die drei getrennt addiert
/// werden:
///
///  * die Zeichenbreiten aus `/Widths` bzw. `/W` — **ohne** die ist
///    der Vorschub 0 und alles steht uebereinander (der Fehler, den
///    dieser Wert behebt);
///  * `Tc` (`/Tc`) je **Zeichen** — nicht je Byte; bei Identity-H sind
///    das zwei Byte je Zeichen;
///  * `Tw` (`/Tw`) nur vor dem **Leerzeichen mit einem Byte** (Code 32).
///    Das ist die Regel der Spezifikation, und sie ist nicht
///    offensichtlich: bei Identity-H ist 0x00 20 **kein** Leerzeichen.
fn vorschub_textraum(z: &Zustand, bytes: &[u8]) -> f64 {
    let cid = z.schriften.get(&z.schrift).map(|b| b.cid).unwrap_or(false);
    let breiten = z.schriften.get(&z.schrift);
    let schritt = if cid { 2 } else { 1 };
    let mut summe = 0.0;
    let mut i = 0usize;
    while i < bytes.len() {
        let code: u32 = if cid {
            let hi = bytes.get(i).copied().unwrap_or(0) as u32;
            let lo = bytes.get(i + 1).copied().unwrap_or(0) as u32;
            (hi << 8) | lo
        } else {
            bytes[i] as u32
        };
        let breite = breiten.map(|b| b.breite(code)).unwrap_or(500.0);
        summe += breite / 1000.0;
        summe += z.zeichenabstand;
        if !cid && code == 32 {
            summe += z.wortabstand;
        }
        i += schritt;
    }
    summe
}

/// Schreibt den Vorschub einer gezeigten Zeichenfolge in die
/// Textmatrix.
///
/// added 03.10.2026. OHNE DIESE FUNKTION STEHT JEDER TEIL EINES
/// `TJ`-ARRAYS AUF DERSELBEN STELLE — der Fehler, der
/// `1706.03762.pdf` unlesbar machte (Absatz in Fuenf Worten
/// uebereinander). Die Rechnung ist in [`vorschub_textraum`], weil sie
/// an **einer** Stelle gehoert und der Renderer dieselbe Breite
/// braucht.
///
/// DIE FORMEL: `tx = summe * Tfs * Th`, und die Matrix wird mit
/// `verschieben(tx, 0).mal(tm)` weitergeschoben — `Matrix::mal` wendet
/// das zweite Argument zuerst an, also den Versatz in den aktuellen
/// Textraum.
fn vorschub_eintragen(z: &mut Zustand, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let summe = vorschub_textraum(z, bytes);
    if summe == 0.0 || !summe.is_finite() {
        return;
    }
    let tx = summe * z.schriftgroesse * z.horizont;
    // F23, siehe oben: der Vorschub ist ein **Textrraum**-Versatz
    // und wird von `tm` mitskaliert.
    z.tm = z.tm.mal(Matrix::verschieben(tx, 0.0));
    // `tlm` wird **nicht** verschoben: `Td` bezieht sich auf die
    // Zeilenmatrix, und die aendert sich nur bei `Td`/`TD`/`T*`/`Tm`.
}

/// Gibt eine Textzeile aus (ein `Tj`, `'` oder `"`).
fn text_ausgeben(raus: &mut Vec<Aktion>, z: &Zustand, bytes: &[u8]) {
    // added 03.10.2026. DIE GROESSE WAR FALSCH, und das war der
    // Grund fuer „0 sichtbare Pixel" bei `rfc2616.pdf` und
    // `rfc9110.pdf`.
    //
    // DIE REGEL: eine Glyphe wird durch `Tm × CTM` transformiert. Ihre
    // sichtbare Groesse ist `Tfs` mal der Skalierung dieser Matrix —
    // **beider**, nicht nur der CTM.
    //
    // GEMESSEN an `fw9.pdf`, Seite 1: der zweite Textoperator hat
    // `Tm = [24 0 0 24 …]`, also Skalierung 24. Die alte Rechnung
    // `Tfs × ctm.skalierung()` ergab **1.0** (die CTM war die
    // Identitaet) — und eine Glyphe von 1 Punkt ist auf 144 dpi
    // ein Haekchen von einem Pixel. Die Seite hatte 7382 Zeichen und
    // **0** Pixel.
    //
    // `Matrix::skalierung()` nimmt nur a, b, c, d; der Versatz e, f
    // gehoert nicht dazu (sonst waere die „Skalierung" von der
    // Lage abhaengig).
    // added 04.10.2026. DIE REIHENFOLGE WAR VERKEHRT, und das war der
    // Grund, warum **jeder** Text in einem Formular unsichtbar war.
    //
    // `mal(selbst, rhs)` wendet **rhs zuerst** an (so ist `cm`
    // gebaut: `ctm = ctm.mal(m)` = erst `m`, dann die alte CTM). Fuer
    // den Text gilt dasselbe Muster: erst `Tm`, dann `CTM` — also
    // `ctm.mal(tm)`. Hier stand `tm.mal(ctm)`, das ist **CTM zuerst**.
    //
    // GEMESSEN an `1512.03385.pdf` Seite 1, Formular `Im1`
    // (`teaser.pdf`, 78 Textoperatoren):
    //
    //     tm  = (4.5656, 0, 0, 4.5676, 14.32, 11.00)
    //     ctm = (0.7314, 0, 0, 0.7314, 308.86, 489.28)
    //     richtig:  0.7314 · 14.32 + 308.86 =  319.3
    //     falsch:  4.5656 · 308.86 + 14.32 = 1424.5
    //
    // Der Text landete bei x = 1424 auf einer 612-Punkt-Seite: **ausser
    //halb des Blattes**, also unsichtbar. Die Umkehrung ist auf einer
    // Seite mit Identitaets-CTM unauffaellig — dort ist
    // `tm.mal(ctm) = ctm.mal(tm)` — und genau deshalb ist es erst an
    // einem Formular aufgefallen, wo die CTM einen Versatz hat.
    let m = z.ctm.mal(z.tm);
    let position = punkt_durch(&Punkt { x: 0.0, y: 0.0 }, m);
    raus.push(Aktion::Text {
        position,
        // added 05.10.2026. DIE **MATRIX** kommt mit, nicht nur ihr
        // Versatz. Vorher stand hier `groesse: z.schriftgroesse *
        // m.skalierung()` — also eine **Zahl** statt der Matrix, und eine
        // Zahl kann keine Drehung tragen.
        //
        // GEMESSEN an `1706.03762.pdf` Seite 1, dem arXiv-Stempel:
        //     /arXivStAmP 20 Tf 0 1 -1 0 32 237 Tm
        // `a = 0, b = 1, c = -1, d = 0` ist eine Drehung um 90 Grad, und
        // der Stempel laeuft im Referenzbild **senkrecht** am linken Rand
        // nach oben. Der eigene Renderer legte ihn **waagerecht** ueber
        // den Abstract — dieselben Zeichen, dieselbe Schriftgroesse, nur
        // die Lage um 90 Grad verdreht.
        matrix: m,
        groesse: z.schriftgroesse,
        zeichen: bytes.to_vec(),
        schrift: z.schrift.clone(),
        horizont: z.horizont,
        laufweite: 0.0,
        // added 05.10.2026: der Textriss (`Ts`) ist ein Textraum-Wert und
        // wird von der Matrix mitgedreht — vorher stand hier
        // `z.anhebung * z.ctm.skalierung()`, das nur die **Gleichmaessige**
        // Skalierung der CTM kannte.
        anhebung: z.anhebung,
        farbe: z.fuell,
    });
}

/// Wie tief `q`/`Q` verschachtelt sein darf (M3).
///
/// added 03.10.2026. 64. Eine PDF mit 10.000 verschachtelten `q` hat
/// einen Zustandsstapel, der den Speicher frisst — und der Grund ist
/// immer derselbe: ein Generator, der ein `q` nicht schliesst.
pub const MAX_Q_Q: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    /// `Tm × CTM`: erst die Textmatrix, dann die CTM — **nicht** anders.
    ///
    /// added 04.10.2026. DER TEST FUER DIE REIHENFOLGE, und er ist der
    /// Fund dieser Runde: `text_ausgeben` rechnete `tm.mal(ctm)`,
    /// und `mal` wendet das zweite Argument zuerst an — damit war die
    /// CTM dran, **vor** der Textmatrix.
    ///
    /// GEMESSEN an `1512.03385.pdf` Seite 1, Formular `Im1`: der Text
    /// des Formulars (78Operatoren, alle unsichtbar) landete bei
    /// `x = 1424,5` auf einem 612-Punkt-Blatt. Richtig ist
    /// `150 = 100 + 50`.
    ///
    /// WARUM ES AUF EINER SEITE MIT IDENTITAETS-CTM NICHT AUFFIEL:
    /// dann ist `tm.mal(ctm) = ctm.mal(tm)`. Es braucht einen
    /// Versatz — und der kommt in einem Formular immer vor.
    #[test]
    fn text_matrix_vor_ctm() {
        // added 04.10.2026: `clippy::field_reassign_with_default` — der
        // Stapel wird gleich beim Bauen gesetzt, nicht danach. Der
        // Test sah vorher nach einem Zufall aus, in dem jemand das Feld
        // vergessen hatte.
        let z = Zustand {
            ctm: Matrix::verschieben(100.0, 200.0),
            ..Zustand::default()
        };
        // added 04.10.2026: `Tm` braucht **SECHS** Operanden
        // (a b c d e f) — der erste Test hier hatte nur `50 60`
        // geschrieben, damit blieb die Textmatrix die Identitaet und
        // der Text sass bei (100, 200) statt bei (150, 260). Der
        // Test pruefte damit die falsche Sache.
        let strom = b"BT /F1 12 Tf 1 0 0 1 50 60 Tm (A) Tj ET";
        let a = ausfuehren(strom, &z, |_| {});
        let texte: Vec<&Aktion> = a
            .iter()
            .filter(|x| matches!(x, Aktion::Text { .. }))
            .collect();
        assert_eq!(texte.len(), 1, "es kam kein Text heraus");
        if let Aktion::Text { position, .. } = texte[0] {
            assert!(
                (position.x - 150.0).abs() < 0.01 && (position.y - 260.0).abs() < 0.01,
                "Text sitzt bei ({:.1},{:.1}), nicht bei (150, 260)",
                position.x,
                position.y
            );
        }
    }

    /// Und dieselbe Reihenfolge fuer `cm`: erst `cm`, dann die alte CTM.
    #[test]
    fn cm_vor_der_alten_ctm() {
        let z = Zustand {
            ctm: Matrix::verschieben(100.0, 200.0),
            ..Zustand::default()
        };
        let strom = b"1 0 0 1 10 20 cm 1 0 0 1 30 40 cm BT /F1 12 Tf 1 0 0 1 5 6 Tm (A) Tj ET";
        let a = ausfuehren(strom, &z, |_| {});
        let texte: Vec<&Aktion> = a
            .iter()
            .filter(|x| matches!(x, Aktion::Text { .. }))
            .collect();
        assert_eq!(texte.len(), 1);
        if let Aktion::Text { position, .. } = texte[0] {
            // 100 + 10 + 30 + 5 = 145, 200 + 20 + 40 + 6 = 266
            assert!(
                (position.x - 145.0).abs() < 0.01 && (position.y - 266.0).abs() < 0.01,
                "Text sitzt bei ({:.1},{:.1}), nicht bei (145, 266)",
                position.x,
                position.y
            );
        }
    }

    /// `Do` meldet das XObject — **und nicht nichts**.
    ///
    /// added 04.10.2026. DER TEST FUER DEN NO-OP: `Do` war als `"Do" => {}`
    /// notiert, und damit wurde in **keinem** der elf PDFs ein einziges
    /// Bild oder Formular gezeichnet (GEMESSEN: 86 `/Subtype /Image` und
    /// 8543 `/Subtype /Form` in den Dateien, aber „0 Bilder" und „0
    /// Formulare“ auf allen 543 Seiten der Abnahme). Der Test schlaegt
    /// bei einem No-gar-nichts fehl und hat keine Ausnahme: der Operator
    /// hat genau eine Aufgabe.
    ///
    /// ZUSAETZLICH das Zielrechteck, weil es der Teil ist, der still
    /// falsch sein kann: `100 0 0 50 20 30 cm` verschiebt und staucht,
    /// also muss das Einheitsquadrat `[20,30,120,80]` ergeben.
    #[test]
    fn do_meldet_das_xobject() {
        let mut z = Zustand {
            ctm: Matrix::verschieben(10.0, 20.0).mal(Matrix::strecken(2.0, 3.0)),
            ..Zustand::default()
        };
        let mut stapel = vec![Wert::Name(b"Im1".to_vec())];
        let mut raus = Vec::new();
        ausfuehren_op("Do", &mut stapel, &mut z, &mut raus);
        assert_eq!(raus.len(), 1, "Do hat {} Aktionen geliefert", raus.len());
        match &raus[0] {
            Aktion::XObject { name, ziel, .. } => {
                assert_eq!(name, "Im1");
                let erwartet = [10.0, 20.0, 12.0, 23.0];
                for (ist, soll) in ziel.iter().zip(erwartet.iter()) {
                    assert!(
                        (ist - soll).abs() < 0.001,
                        "Zielrechteck ist {ziel:?}, nicht {erwartet:?}"
                    );
                }
            }
            anders => panic!("Do hat {anders:?} geliefert"),
        }
    }

    /// Ein `Do` **ohne** Namen erzeugt nichts (und panickt nicht).
    #[test]
    fn do_ohne_namen_erzeugt_nichts() {
        let mut z = Zustand::default();
        let mut stapel: Vec<Wert> = Vec::new();
        let mut raus = Vec::new();
        ausfuehren_op("Do", &mut stapel, &mut z, &mut raus);
        assert!(raus.is_empty());
    }

    /// Der Vorschub nach einem gezeigten String landet in der
    /// Textmatrix — das war der Fehler, der `1706.03762.pdf`
    /// unlesbar machte.
    ///
    /// added 03.10.2026. GEMESSEN am Content-Stream von
    /// `1706.03762.pdf`:
    ///
    /// ```text
    /// [(Pro)15(vided)-250(proper)]TJ
    /// ```
    ///
    /// `TJ` zeichnet **drei** Teilstrings. Die Zahlen zwischen ihnen
    /// wurden uebernommen, die **Zeichenbreiten nicht** — also begann
    /// „vided" bei x = 124,49 statt bei 141,09 (der Wert aus
    /// `mutool trace`). Fuenf Worte uebereinander.
    ///
    /// DER TEST ist bewusst klein: eine Schrift mit drei bekannten
    /// Breiten, eine Zeichenfolge, und die Erwartung, dass die
    /// Textmatrix danach um **genau** diese Summe weitergeschoben ist.
    #[test]
    fn vorschub_nach_tj() {
        // added 03.10.2026: als Wörtbuch-Literal statt
        // `default()` und Einzelzuweisungen (clippy
        // `field_reassign_with_default`).
        let b = crate::schrift::Breiten {
            first_char: b'A' as i64,
            breiten: vec![500.0, 600.0, 700.0], // A, B, C
            fehlbreite: 500.0,
            ..Default::default()
        };
        let mut z = Zustand {
            schrift: "F1".into(),
            schriftgroesse: 10.0,
            horizont: 1.0,
            schriften: [("F1".to_string(), b)].into_iter().collect(),
            ..Default::default()
        };
        let start = z.tm.e;
        // „AB": 0.5 + 0.6 em, bei 10 Punkt also 11 Punkte.
        vorschub_eintragen(&mut z, b"AB");
        assert!(
            (z.tm.e - start - 11.0).abs() < 1e-9,
            "Vorschub {} statt 11",
            z.tm.e - start
        );
        // Die Zeilenmatrix bleibt, wo sie war: `Td` bezieht sich auf
        // sie, nicht auf die vorgeschobene Textmatrix.
        assert!((z.tlm.e - z.tm.e).abs() > 1e-9, "tlm darf nicht mitgehen");
    }

    /// `Tc` (Zeichenabstand) und `Tw` (Wortabstand) gehoeren in die
    /// Summe — sonst stimmt der Vorschub bei jedem gesperrten Satz
    /// nicht.
    #[test]
    fn vorschub_mit_abstaenden() {
        // added 03.10.2026: als Wörtbuch-Literal, siehe oben.
        let b = crate::schrift::Breiten {
            first_char: 32,
            breiten: vec![278.0, 500.0], // Leerzeichen, A
            fehlbreite: 500.0,
            ..Default::default()
        };
        let z = Zustand {
            schrift: "F1".into(),
            schriftgroesse: 10.0,
            zeichenabstand: 0.05, // 5 % von 10 Punkt = 0,5 Punkt je Zeichen
            wortabstand: 0.25,    // nur vor dem Leerzeichen
            schriften: [("F1".to_string(), b)].into_iter().collect(),
            ..Default::default()
        };
        let summe = vorschub_textraum(&z, b" A");
        // Leerzeichen: 0.278 + 0.05 Zeichenabstand + 0.25 Wortabstand
        // A: 0.5 + 0.05
        assert!(
            (summe - (0.278 + 0.05 + 0.25 + 0.5 + 0.05)).abs() < 1e-9,
            "{summe}"
        );
    }

    /// Ein zweibyte-Satz (Type0) zaehlt **Zeichen**, nicht Bytes: der
    /// Zeichenabstand wird nur alle zwei Byte addiert.
    #[test]
    fn vorschub_type0_zwei_byte() {
        // added 03.10.2026: als Wörtbuch-Literal, siehe oben.
        let b = crate::schrift::Breiten {
            cid: true,
            cid_breite: 1000.0,
            ..Default::default()
        };
        let z = Zustand {
            schrift: "F0".into(),
            schriftgroesse: 10.0,
            zeichenabstand: 0.1,
            schriften: [("F0".to_string(), b)].into_iter().collect(),
            ..Default::default()
        };
        // Zwei CIDs: 0x0041 und 0x0042, je 1 em.
        let summe = vorschub_textraum(&z, &[0x00, 0x41, 0x00, 0x42]);
        assert!((summe - 2.2).abs() < 1e-9, "{summe}");
    }

    /// Die Reihenfolge der Felder in `cm`.
    ///
    /// added 03.10.2026. DER KLASSISERFEHLER: `2 0 0 2 0 0 cm` soll
    /// verdoppeln. Wer die Felder als Zeilenmatrix denkt, bekommt
    /// eine Spiegelung.
    #[test]
    fn cm_verdoppelt() {
        let m = Matrix {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 0.0,
            f: 0.0,
        };
        let (x, y) = m.punkt(10.0, 20.0);
        assert_eq!((x, y), (20.0, 40.0));
    }

    /// Matrixmultiplikation: erst `self`, dann `rhs`.
    #[test]
    fn matrix_reihenfolge() {
        let skal = Matrix {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 0.0,
            f: 0.0,
        };
        let schieb = Matrix::verschieben(10.0, 0.0);
        // `skal * schieb`: verschieben **dann** skalieren -> 22.
        let (x, _) = skal.mal(schieb).punkt(1.0, 0.0);
        assert_eq!(x, 22.0);
        // `schieb * skal`: skalieren **dann** verschieben -> 12.
        let (x, _) = schieb.mal(skal).punkt(1.0, 0.0);
        assert_eq!(x, 12.0);
    }

    /// `10 20 m 30 40 l S` zeichnet eine Linie.
    ///
    /// added 03.10.2026. Und der Test sieht, dass `S` den Pfad
    /// leert — sonst wächst der Pfad ueber die Seite.
    #[test]
    fn linie_wird_gezeichnet() {
        let strom = b"10 20 m 30 40 l S";
        let aktionen = ausfuehren(strom, &Zustand::default(), |_| {});
        assert_eq!(aktionen.len(), 1);
        match &aktionen[0] {
            // added 05.10.2026: `..` statt dreier Felder. Das neue Feld
            // `strich` kam hinzu, und dieses Muster liess es zu — ein
            // Test, der alle Felder aufzaehlt, bricht bei jedem neuen
            // Feld, auch wenn er es gar nicht prueft.
            Aktion::Kontur {
                teile,
                farbe,
                breite,
                strich,
                ..
            } => {
                assert_eq!(farbe, &[0, 0, 0, 255]);
                assert!(*breite > 0.0);
                assert_eq!(teile.len(), 1);
                assert_eq!(teile[0].segmente.len(), 1);
                // added 05.10.2026: ohne `d` gibt es kein Muster.
                assert!(strich.is_empty(), "ohne `d` kein Muster: {strich:?}");
            }
            other => panic!("sollte Kontur sein: {other:?}"),
        }
    }

    /// `q` / `Q` stellt den Zustand wieder her.
    #[test]
    fn q_q_stellt_wieder_her() {
        let strom = b"0.5 g q 1 g Q";
        let aktionen = ausfuehren(strom, &Zustand::default(), |_| {});
        // Danach muss die Fuellfarbe wieder 0.5 sein — beweisen wir
        // das indirekt: ein Fuellvorgang danach nimmt 0.5.
        let strom2 = b"0.5 g q 1 g Q 0 0 10 10 re f";
        let a2 = ausfuehren(strom2, &Zustand::default(), |_| {});
        match &a2[0] {
            Aktion::FuellPfad { farbe, .. } => assert_eq!(farbe[0], 128),
            other => panic!("sollte Fuellpfad sein: {other:?}"),
        }
        let _ = aktionen;
    }

    /// `re f` erzeugt **vier** Segmente (ein geschlossenes Rechteck).
    ///
    /// added 03.10.2026. Drei waeren eine offene Figur; der Unterschied
    /// ist an einem Kreis sichtbar, an einem Rechteck nicht — deshalb
    /// der Test.
    #[test]
    fn rechteck_vier_segmente() {
        let a = ausfuehren(b"0 0 10 10 re f", &Zustand::default(), |_| {});
        match &a[0] {
            Aktion::FuellPfad { teile, gerade, .. } => {
                assert_eq!(teile[0].segmente.len(), 4);
                assert!(!gerade);
            }
            other => panic!("sollte Fuellpfad sein: {other:?}"),
        }
    }

    /// Ein unbekannter Operator wird **gemeldet**, nicht zum Absturz.
    ///
    /// added 03.10.2026. `BX`/`EX` stehen in vielen PDFs (Kompatibilitaet
    /// mit Acrobat 3).
    #[test]
    fn unbekannter_operator_wird_gemeldet() {
        let a = ausfuehren(b"BX 10 20 m 30 40 l EX", &Zustand::default(), |_| {});
        assert!(
            a.iter()
                .any(|x| matches!(x, Aktion::Unbekannt(k) if k == "BX"))
        );
    }

    /// `TJ` mit einer Zahl von -250 rueckt nach links.
    ///
    /// added 03.10.2026. Das Vorzeichen ist der ganze Trick und der
    /// haeufigste Vorwand fuer „der Text steht schief".
    #[test]
    fn tj_verschiebung() {
        let strom = b"BT /F1 12 Tf 100 700 Td [(A) -250 (B)] TJ ET";
        let a = ausfuehren(strom, &Zustand::default(), |_| {});
        let texte: Vec<&Aktion> = a
            .iter()
            .filter(|x| matches!(x, Aktion::Text { .. }))
            .collect();
        assert_eq!(texte.len(), 2);
        if let Aktion::Text { position: p1, .. } = texte[0] {
            if let Aktion::Text { position: p2, .. } = texte[1] {
                assert!(p2.x > p1.x, "zweites Zeichen muss rechts stehen");
            }
        }
    }
}
