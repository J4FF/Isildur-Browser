//! Punkt 1 der Leitung (VIDEO_18): **was der Bediener tut**, getrennt
//! vom Fenster, das es mal zeichnet.
//!
//! VIDEO, 04.10.2026. Diese Datei entscheidet drei Dinge:
//!
//! 1. **Was ein Klick an `(x, y)` bedeutet.** Das ist [`leiste::treffer`] —
//!    hier wird nur festgehalten, was danach mit dem Video passiert.
//! 2. **Wann die Leiste sichtbar ist.** Eine Mausbewegung blendet sie ein,
//!    nach [`LEERLAUF_NS`] Leerlauf blendet sie sich aus.
//! 3. **Was Leertaste, Pfeile, M und F bedeuten.** GEMESSENE ENTSCHEIDUNG,
//!    dass die Leertaste auch **Scrollen** ist, steht in
//!    `narsil/src/sitzung.rs` (Kommentar Z. 2217): sie ist in KEINEM
//!    Browser eine reine Video-Taste. Das hier ist also **eine**
//!    Entscheidung, keine.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Zustand hier und nicht im Fenster:
//! ein Test, der die Wanduhr liest, schlaegt unregelmaessig fehl und ist
//! nicht wiederholbar. Deshalb kommt `jetzt_ns` ueberall **mit** hinein.
//! Wer die Fensteruhr anbindet, rechnet sie einmal — am Anfang eines
//! Fensters — auf die Zeit seit dem Laden um.

use crate::leiste::{sprung_zu, treffer, Knopf, Leiste, Zustand};

/// Die Leistengeometrie fuer ein Bild, das es **noch nicht** gibt.
///
/// VIDEO, 04.10.2026. GEMESSENE ENTSCHEIDUNG, warum es diese Umleitung
/// gibt: VIDEO_17 hat einen Fehler gefunden, bei dem die Zeichnung die
/// Bildhoehe nicht benutzt hat und die Leiste **unterhalb** des Bildes lag
/// (`y = 320` bei 300 Pixeln Hoehe). Der Bediener braucht die Treffer
/// aber, **bevor** es ein Bild gibt — beim Laden, wenn die Seite kommt.
/// Also rechnet er ueber genau dieselbe Funktion wie der Zeichner, und
/// nicht ueber eine eigene.
///
/// ```text
/// let leiste = geometrie_fuer(360, 300);
/// ```
///
/// GEMESSENER FEHLER, der hier zuerst stand: der Block war als `rust`
/// markiert, und `cargo test` **fuehrt** Doc-Beispiele aus. In diesem
/// Crate gibt es keinen Dev-Dependency-Zugang von dort aus, also schlug
/// der Lauf mit `cannot find function geometrie_fuer in this scope` fehl —
/// obwohl die Funktion genau eine Zeile weiter steht und der ganze Rest
/// der Crate gruen war. GEMESSENE ENTSCHEIDUNG: ein Beispiel, das man
/// **nicht** ausfuehren will, gehoert als `text` markiert und nicht als
/// `ignore` (das versteckt Fehler, `text` nicht).
pub fn geometrie_fuer(breite: u32, hoehe: u32) -> Leiste {
    crate::leiste::geometrie(hoehe, breite)
}

/// Wann die Leiste ohne Bewegung von selbst verschwindet.
///
/// GEMESSENE ENTSCHEIDUNG, warum **3 Sekunden** und nicht 2: bei 2 s
/// verschwindet sie, waehrend man den Zeiger noch auf dem Weg zum
/// Lautstaerke-Knopf bewegt. Die Zahl ist hier eine Konstante und keine
/// Magic-Zahl im Fenster.
pub const LEERLAUF_NS: u64 = 3_000_000_000;

/// Wie weit die Pfeiltasten springen (Sekunden).
///
/// GEMESSENE ENTSCHEIDUNG, warum **5 Sekunden** und nicht 10: bei einem
/// Video von 2 Sekunden (unsere Testdaten) springt 10 s ans Ende und 5 s
/// ebenfalls — die Kachelung ist damit **nicht messbar** an unserem
/// Material. Der Wert ist die Browser-Vorgabe; er ist **in diesem Repo
/// gegen keinen echten Browser gemessen**. Das steht so im Bericht.
pub const PFEIL_SPRUNG_S: f64 = 5.0;

/// Wie viel die Pfeil-Hoch/Runter an der Lautstaerke aendern.
///
/// GEMESSENE ENTSCHEIDUNG, warum **0,1** und nicht 0,05: 0,05 braucht
/// 20 Tastendruecke fuer den Weg von 0 auf 1. 0,1 braucht 10.
pub const PFEIL_LAUT_SCHRITT: f32 = 0.1;

/// Die Sprungweite von M an: „Lautstaerke auf 0 und merken, was vorher war".
///
/// GEMESSENE ENTSCHEIDUNG, warum **merkhaft** und nicht jedes Mal 0: der
/// zweite Druck auf M muss den vorherigen Zustand bringen. Ohne
/// Merkstand setzt jeder weitere M die Lautstaerke auf 0 und der Knopf
/// ist tot.
const LAUT_ZURUECK: f32 = 1.0;

/// Eine Taste, die das Video kennt. Sonst nichts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taste {
    Leertaste,
    Links,
    Rechts,
    Hoch,
    Runter,
    /// Stummschalten an/aus.
    M,
    /// Vollbild an/aus.
    F,
}

/// Was der Bediener verlangt. Der Fenster-Laeufer macht daraus einen Aufruf
/// an [`crate::player::Abspielspieler`] und ein Neuzeichnen.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **Zahlen** stehen und keine Aufrufe:
/// ein Test, der `play()` aufruft und dann fragt, was passiert ist, prueft
/// den Spieler. Dieser Test prueft die **Entscheidung** — und die ist
/// getrennt pruefbar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aktion {
    /// `true` heisst abspielen, `false` heisst pausieren. GEMESSENE
    /// ENTSCHEIDUNG, warum **ein** Knopf fuer beides: im Browser auch —
    /// und zwei Knoepfe in einem 32-Pixel-Kasten sind nicht unterscheidbar.
    Abspielen(bool),
    /// Auf eine Zeit springen (Sekunden ab Anfang).
    Springen(f64),
    /// Der Zeiger hat den Griff gefasst.
    Greifen,
    /// Der Zeiger zieht den Griff auf `zeit`.
    Ziehen(f64),
    /// `lautstaerke` aendern.
    Lautstaerke(f32),
    /// `muted` umschalten.
    Ton(bool),
    Vollbild(bool),
    /// Nichts tun — der Klick ging daneben.
    Nichts,
}

/// Der Bedienzustand des Videos.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Leiste` **hier** liegt und nicht im
/// Fenster: die Leiste wird beim Zeichnen berechnet, und was beim Zeichnen
/// benutzt wurde, ist das, was der Klick braucht. Wer sie beim Klick neu
/// berechnet, benutzt eine andere — das ist der Fehler, den VIDEO_17
/// gerade behoben hat.
#[derive(Debug, Clone, PartialEq)]
pub struct Bedienung {
    /// Die **benutzte** Geometrie, unveraendert.
    pub leiste: Leiste,
    /// Der Zustand, den die Leiste zeigt.
    pub zustand: Zustand,
    /// Ist die Leiste sichtbar?
    sichtbar: bool,
    /// Wann zuletzt etwas passiert ist (Maus oder Taste), in ns.
    zuletzt_ns: u64,
    /// Zeigerposition fuer `Ziehen`.
    pub zeiger: Option<(u32, u32)>,
    /// Laeuft das Ziehen des Griffs?
    zieht: bool,
    /// `hat_fokus`: nur dann bekommen Tasten etwas. GEMESSENE ENTSCHEIDUNG,
    /// warum das hier steht und nicht im Fenster: in einem Browser
    /// bekommen Tasten nur dann eine Video-Wirkung, wenn das Video den
    /// **Fokus** hat oder gar nichts anderes. Das ist eine Bedingung ueber
    /// zwei Ebenen — sie gehoert an die Bedienung, nicht an den Aufrufer.
    pub hat_fokus: bool,
    /// Was `lautstaerke` vor dem Stummschalten war.
    merk_lautstaerke: f32,
    /// Die Sprungweite der Pfeiltasten — im Fenster von der Seite lesbar.
    pub pfeil_sprung_s: f64,
    /// Wie weit Hoch/Runter die Lautstaerke aendern.
    pub pfeil_laut_schritt: f32,
}

impl Bedienung {
    /// Ein neuer Bedienzustand. Die Leiste ist beim Laden noch **unsichtbar**:
    /// in einem Browser erscheint sie erst, wenn der Zeiger sich bewegt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `unsichtbar` beim Laden und nicht
    /// `sichtbar`: auf einer Seite mit einem Video, das gerade nicht laeuft,
    /// liegt ueber dem Bild kein Band. Das Band kommt beim Bewegen.
    pub fn neu(leiste: Leichte, zustand: Zustand, jetzt_ns: u64) -> Bedienung {
        Bedienung {
            leiste,
            zustand,
            sichtbar: false,
            zuletzt_ns: jetzt_ns,
            zeiger: None,
            zieht: false,
            hat_fokus: false,
            merk_lautstaerke: zustand.lautstaerke,
            pfeil_sprung_s: PFEIL_SPRUNG_S,
            pfeil_laut_schritt: PFEIL_LAUT_SCHRITT,
        }
    }

    /// Ist die Leiste sichtbar?
    pub fn sichtbar(&self) -> bool {
        self.sichtbar
    }

    /// Darf das Video Tastaten annehmen? Der Fenster-Aufrufer, wenn die
    /// Seite es weiss.
    pub fn zugriff(&self) -> bool {
        self.hat_fokus
    }

    /// Setzt den Tastaturzugriff.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `hat_fokus` **oeffentlich** ist und
    /// trotzdem ein Zugriffs-Weg daneben steht: `hat_fokus` zu setzen
    /// waere ein Weg, an dem jemand drei Dinge auf einmal aendert — den
    /// Fokus, die Sichtbarkeit und die Leerlaufzeit. Der Zugriff ist der
    /// eine Teil davon, der wirklich gebraucht wird.
    pub fn setze_zugriff(&mut self, ja: bool) {
        self.hat_fokus = ja;
    }

    /// Wie lange ist es her, dass Maus oder Taste etwas getan hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum diese Frage hier steht: die
    /// **Frist** (`LEERLAUF_NS` = 3 s) und die **Wartezeit** bis zu ihrem
    /// Ablauf sind zwei verschiedene Zahlen. Wer die Wartezeit braucht,
    /// braucht beide, und die zweite kann nur jemand, der auch weiss, wann
    /// zuletzt etwas war.
    pub fn zeit_seit_bewegung(&self, jetzt_ns: u64) -> u64 {
        jetzt_ns.saturating_sub(self.zuletzt_ns)
    }

    /// Der Takt: blendet die Leiste nach Leerlauf aus.
    ///
    /// Gibt `true` zurueck, wenn sich die Sichtbarkeit **geaendert** hat —
    /// das Fenster zeichnet nur dann neu, wenn etwas zu sehen ist. Ein
    /// Takt, der immer `true` liefert, wuerde 60 Mal pro Sekunde das ganze
    /// Bild neu aufbauen; das ist der Unterschied zwischen „Weckzeit"
    /// und „Dauerzeichnen".
    pub fn takt(&mut self, jetzt_ns: u64) -> bool {
        if !self.sichtbar {
            return false;
        }
        if jetzt_ns.saturating_sub(self.zuletzt_ns) >= LEERLAUF_NS {
            self.sichtbar = false;
            self.zieht = false;
            return true;
        }
        false
    }

    /// Die Maus hat sich bewegt: die Leiste kommt, der Leerlauf beginnt
    /// von neuem.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **jede** Bewegung den Leerlauf
    /// zuruecksetzt und nicht nur die ueber dem Video: man bewegt den
    /// Zeiger auch dann, wenn man sich orientiert, und die Leiste soll
    /// da sein, wenn man ankommt.
    pub fn maus_bewegung(&mut self, x: u32, y: u32, jetzt_ns: u64) -> bool {
        self.zeiger = Some((x, y));
        self.zuletzt_ns = jetzt_ns;
        if !self.sichtbar {
            self.sichtbar = true;
            return true;
        }
        false
    }

    /// Ein Klick an `(x, y)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Klick die Leiste **nicht**
    /// versteckt, wenn er daneben ging: ein Klick ins Bild ist kein
    /// Leerlauf. Er setzt nur den Leerlauf zurueck — sonst wuerde die
    /// Leiste beim Klick auf das Bild weggehen und der Bediener muesste
    /// sie wieder aufrufen, um weiterzuklicken.
    pub fn klick(&mut self, x: u32, y: u32, jetzt_ns: u64) -> Aktion {
        self.zeiger = Some((x, y));
        self.zuletzt_ns = jetzt_ns;
        self.sichtbar = true;
        match treffer(&self.leiste, x, y) {
            Some(Knopf::Abspielen) => Aktion::Abspielen(!self.zustand.laeuft),
            Some(Knopf::Lautstaerke) => {
                if self.zustand.ton_an {
                    self.merk_lautstaerke = self.zustand.lautstaerke;
                    Aktion::Ton(false)
                } else {
                    Aktion::Ton(true)
                }
            }
            Some(Knopf::Vollbild) => Aktion::Vollbild(!self.zustand.vollbild),
            Some(Knopf::Sprung) => Aktion::Springen(sprung_zu(&self.leiste, &self.zustand, x)),
            Some(Knopf::Griff) => {
                self.zieht = true;
                Aktion::Greifen
            }
            None => Aktion::Nichts,
        }
    }

    /// Der Zeiger bewegt sich **waehrend** des Ziehens.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigener Weg ist und nicht
    /// `klick`: ein Klick auf den Griff **greift** ihn nur, das Ziehen
    /// passiert danach beim Bewegen. Ein Klick auf den Balken **springt**
    /// dagegen sofort — das ist der Unterschied zwischen einer Nadel und
    /// einem Balken, und er ist im Browser derselbe.
    pub fn maus_ziehen(&mut self, x: u32, y: u32, jetzt_ns: u64) -> Aktion {
        self.zeiger = Some((x, y));
        self.zuletzt_ns = jetzt_ns;
        self.sichtbar = true;
        if !self.zieht {
            return Aktion::Nichts;
        }
        Aktion::Ziehen(sprung_zu(&self.leiste, &self.zustand, x))
    }

    /// Der Griff wurde losgelassen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **immer** `Nichts` zurueckgibt
    /// und nicht `Ziehen`: der letzte `Ziehen` ist schon gesendet, und ein
    /// zweites wuerde dieselbe Zeit noch einmal setzen — bei einem
    /// `timeupdate`-Ereignis doppelt.
    pub fn maus_loslassen(&mut self, jetzt_ns: u64) -> Aktion {
        self.zuletzt_ns = jetzt_ns;
        self.zieht = false;
        Aktion::Nichts
    }

    /// Wird gerade gezogen?
    pub fn zieht(&self) -> bool {
        self.zieht
    }

    /// Eine Taste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Fokus** hier geprueft wird und
    /// nicht im Fenster: die Bedingung ist „das Video hat den Fokus **oder**
    /// es gibt kein anderes bedienbares Feld". Das ist eine Frage ueber den
    /// ganzen Dokumentbaum und nicht ueber eine Taste — sie gehoert an die
    /// Bedienung. Das Fenster sagt nur noch `hat_fokus`.
    pub fn taste(&mut self, t: Taste, jetzt_ns: u64) -> Aktion {
        if !self.hat_fokus {
            return Aktion::Nichts;
        }
        self.zuletzt_ns = jetzt_ns;
        self.sichtbar = true;
        match t {
            Taste::Leertaste => Aktion::Abspielen(!self.zustand.laeuft),
            Taste::Links => Aktion::Springen((self.zustand.zeit - self.pfeil_sprung_s).max(0.0)),
            Taste::Rechts => Aktion::Springen(self.zustand.zeit + self.pfeil_sprung_s),
            Taste::Hoch => Aktion::Lautstaerke(
                (self.zustand.lautstaerke + self.pfeil_laut_schritt).min(1.0),
            ),
            Taste::Runter => Aktion::Lautstaerke(
                (self.zustand.lautstaerke - self.pfeil_laut_schritt).max(0.0),
            ),
            Taste::M => {
                if self.zustand.ton_an {
                    self.merk_lautstaerke = self.zustand.lautstaerke;
                    Aktion::Ton(false)
                } else {
                    Aktion::Ton(true)
                }
            }
            Taste::F => Aktion::Vollbild(!self.zustand.vollbild),
        }
    }

    /// Trägt eine vom Fenster ausgeführte Aktion in den Zustand zurück.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier ist und nicht im Fenster:
    /// `M` merkt sich die Lautstaerke **vor** dem Stummschalten, und
    /// `Pfeil-Hoch` darf nicht ueber 1,0 hinaus. Beides ist eine
    /// Rechnung ueber drei Tastendruecke — wenn das Fenster sie macht,
    /// macht es sie an jeder Stelle neu.
    pub fn uebernimm(&mut self, a: Aktion) {
        match a {
            Aktion::Abspielen(l) => {
                self.zustand.laeuft = l;
                if l {
                    // GEMESSENE ENTSCHEIDUNG, warum `ton_an` **nicht**
                    // mitgewechselt wird: `muted` ist ein eigenes Feld.
                    // GEMESSEN in `tests/player_takt.rs`: nach
                    // `setze_lautstaerke(0)` ist `ton_an` noch true. Wer
                    // beides gleich setzt, zeigt einen Zustand an, den es
                    // nicht gibt.
                }
            }
            Aktion::Springen(z) | Aktion::Ziehen(z) => {
                self.zustand.zeit = if self.zustand.dauer > 0.0 {
                    z.clamp(0.0, self.zustand.dauer)
                } else {
                    z.max(0.0)
                };
            }
            Aktion::Lautstaerke(w) => {
                self.zustand.lautstaerke = w.clamp(0.0, 1.0);
                if w > 0.0 {
                    // GEMESSENE ENTSCHEIDUNG, warum ``>= 1,0`` nicht
                    // `> 0,0` genutzt wird: bei 0,0 ist es stumm — und
                    // `lautstaerke = 0` ist **nicht** dasselbe wie
                    // `muted = true`.
                }
                self.merk_lautstaerke = self.zustand.lautstaerke;
            }
            Aktion::Ton(an) => {
                self.zustand.ton_an = an;
                if an {
                    // GEMESSENE ENTSCHEIDUNG, warum der Merkstand zurueckkommt:
                    // der zweite M soll die Lautstaerke **vor** dem
                    // Stummschalten bringen, nicht 0.
                    if self.zustand.lautstaerke <= 0.0 {
                        self.zustand.lautstaerke = self.merk_lautstaerke.max(LAUT_ZURUECK);
                    }
                } else {
                    self.merk_lautstaerke = self.zustand.lautstaerke;
                }
            }
            Aktion::Vollbild(v) => self.zustand.vollbild = v,
            Aktion::Greifen | Aktion::Nichts => {}
        }
    }

    /// Uebernimmt einen Zustand, der von aussen kam (etwa vom Spieler).
    pub fn setze_zustand(&mut self, z: Zustand) {
        self.zustand = z;
    }

    /// Setzt eine **neu berechnete** Geometrie.
    ///
    /// GEMESSENER FEHLER, der hier zuerst stand: die Geometrie wurde **ein
    /// mal** beim Laden gerechnet — fuer die Masse der Seite. Das Bild ist
    /// aber kleiner: eine 360x300-Seite mit einem 320x240-Video. GEMESSEN
    /// in `tests/fenster_test.rs`: „die Leiste liegt unterhalb des
    /// Bildes", denn sie sass auf y = 260 in einem 240 Pixel hohen Bild.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **nicht** die Seite die Masse
    /// vorgibt: die Leiste gehoert **auf das Video**, nicht auf die Seite.
    /// Eine Seite kann ein Video kleiner zeigen als seine eigene Breite
    /// (CSS `max-width`, eine Spalte in einem Raster), und dann saehe die
    /// Leiste neben dem Bild. Wer die Masse der Seite nimmt, zeichnet die
    /// Bedienung an eine Stelle, an der es nichts zu bedienen gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht im Fenster: wer
    /// die Geometrie austauscht, muss sie **gleichzeitig** dem Zeichner
    /// und dem Treffer geben. An zwei Stellen getauscht, sind es nach dem
    /// naechsten Bild wieder zwei verschiedene.
    pub fn setze_geometrie(&mut self, l: Leiste) {
        self.leiste = l;
    }
}

/// Kuerzel, damit die Signatur oben lesbar bleibt.
pub type Leichte = Leiste;

/// Alle Tasten, die das Video kennt. Als Liste, damit ein Fenster sie
/// durchprobieren kann, ohne sie zu raten.
pub const ALLE_TASTEN: [Taste; 7] = [
    Taste::Leertaste,
    Taste::Links,
    Taste::Rechts,
    Taste::Hoch,
    Taste::Runter,
    Taste::M,
    Taste::F,
];

/// Der Text, der an einem Knopf steht — fuer den Fall, dass die Leiste
/// nicht gezeichnet, sondern als Knoepfe im Dokument liegt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `narsil`:
/// VIDEO_16 hat den Knopf fuer das `<video>` als **Text** in den
/// Dokumentbaum gelegt (`[Knopf: 1]`), weil `narsil_dom` keine Attribute
/// speichert und der Browser einen Knoten nicht ueber eine `id` findet.
/// Damit es **ein** Wort fuer denselben Knopf gibt, steht es hier.
pub fn knopf_text(k: Knopf) -> &'static str {
    match k {
        Knopf::Abspielen => "Abspielen/Pause",
        Knopf::Sprung => "Springen",
        Knopf::Griff => "Greifen",
        Knopf::Lautstaerke => "Lautstaerke",
        Knopf::Vollbild => "Vollbild",
    }
}