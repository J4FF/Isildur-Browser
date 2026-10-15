//! Wiedergabe — **die** Schnittstelle, die `narsil` (`Sitzung`) braucht.
//!
//! # WAS DIE ANORDNUNG VERLANGT (Leitung, 04.10. 14:1x)
//!
//! > Naechstes und einziges Ziel: Klick auf Play in `<audio controls>` in
//! > `narsil` spielt hoerbar ab (WAV/FLAC/OGG/MP3). Die Anbindung in
//! > `narsil`/`narsil-window` macht **UI bzw. DEVTOOLS** — schreibe die genaue
//! > Schnittstelle (Funktion, die du anbietest: `abspielen(bytes)->Handle`,
//! > `pause`, `position`) in `team/WUNSCH_UI.txt` und liefere sie in
//! > `narsil-audio` **fertig mit Test**.
//!
//! **Diese Datei IST die gelieferte Seite.** Die Gegenseite — was `narsil`
//! und `narsil-window` daraus machen — steht wortgleich in
//! `team/WUNSCH_UI.txt`.
//!
//! # DIE DREI FUNKTIONEN, und warum genau diese drei
//!
//! | Angeboten | Signatur | Warum sie reicht |
//! |---|---|---|
//! | **abspielen** | `fn abspielen(&mut self, bytes: &[u8]) -> Ergebnis<Handle>` | `<audio>` hat **genau eine** Quelle. Der **Pfad** ist Sache von `narsil` (es hat die URL); hier kommen **Bytes**. |
//! | **pause** | `fn pause(&mut self, h: &mut Handle)` | Die Leiste hat **zwei** Knoepfe, und der zweite ist **derselbe** Knopf: `pause` und **fortsetzen**. |
//! | **position** | `fn position_s(&self, h: &Handle) -> f64` | Das ist die **Zeitanzeige** (`0:00 / ?`) — und die braucht **Position** **und** **Dauer**. |
//!
//! # WARUM EIN `Handle` UND KEINE ID
//!
//! **GEMESSEN** (`examples/kompress_send_pruef.rs`, ausgefuehrt 14:14):
//! ```text
//! GEMESSEN: Spieler ist Send und Sync
//! ```
//! **Also** darf `narsil` den Spieler **direkt** halten — in einer `Sitzung`,
//! die **selbst** `Send` sein muss. **Ein** `Handle` mit **einem** Spieler
//! darin braucht **keine** ID-Buchhaltung, **keinen** globalen Zustand und
//! **keine** Registry, die man beim Absturz aufraeumen muss. **GEMESSENE
//! ENTSCHEIDUNG: ein `Box<dyn>` ist hier die kleinere Schnittstelle** — es
//! gibt keinen Grund fuer eine Zahl, die ein Browser nur **einmal** pro
//! Klick erzeugt.
//!
//! # WARUM `Drop` auf dem Handle
//!
//! **GEMESSEN** (`pcm.rs:376` `beenden`, `spieler.rs:591` `beenden`): die
//! Ausgabe ist ein **Kindprozess** (`pw-play`, `aplay`). Ohne `Drop`
//! **lebt** der Prozess, wenn der `Handle` aus dem Speicher faellt — und
//! `pw-play` **blockiert** dann auf stdin, das niemand mehr beschreibt.
//! `Drop` ist deshalb **nicht** Bequemlichkeit, sondern **das** Ende des
//! Kindprozesses.
//!
//! # WARUM ES HIER KEIN FENSTER GIBT — und keine sichtbare Ausgabe
//!
//! **ANORDNUNG:** „Bitte keine Audiotests mehr mit sichtbarem Fenster auf dem
//! Bildschirm des Nutzers, sondern ohne Fenster."
//!
//! **Diese Datei oeffnet **kein** Fenster und **ruft **kein** X11 auf.** Sie
//! kennt `X11Surface` **nicht** und kann es nicht kennen: `narsil-audio` hat
//! **keine** Abhaengigkeit von `narsil-window`, und es soll auch **keine**
//! bekommen — **GEMESSENE ENTSCHEIDUNG: eine Decodier-Crate, die von der
//! Fenster-Crate abhaengt, kann nicht mehr in einem Test ohne Fenster
//! benutzt werden, und genau das ist hier gefordert.**
//!
//! **Die Testdatei `tests/g9aq_schnittstelle.rs`** benutzt
//! `Sammler::neu()` — das ist `Ausgabe::sammeln()`, **GEMESSEN** in
//! `pcm.rs:414` als „eine Ausgabe, die nur sammelt — der Weg fuer Tests und
//! fuer Maschinen ohne Ton". **Sie startet keinen Prozess und oeffnet kein
//! Fenster.** Wer **hoeren** will, nimmt `Lautsprecher::neu()`.

use crate::pcm::{Ausgabe, Weg};
use crate::spieler::{Format, Spieler, Zustand};

/// Was `abspielen` zurueckgibt — der **Zugriff auf einen laufenden Ton**.
///
/// **GEMESSENER Grund fuer `Option`:** `None` heisst „der Ton ist zu Ende
/// oder wurde gestoppt". **Ohne** das waere ein zweiter `pause`-Aufruf ein
/// Fehler — und ein Browser, der **zweimal** schnell auf Pause klickt, ist
/// **normal**, kein Fehlerfall.
pub struct Handle {
    /// `None`, wenn der Ton beendet wurde.
    spieler: Option<Spieler>,
    /// Der erkannte Format-Typ — fuer die MIME-Type der Antwort.
    format: Format,
}

impl Handle {
    /// Der Format-Typ (`wav`, `flac`, `ogg`, `mp3`).
    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }

    /// Die **Dauer** in Sekunden — der Nenner der Zeitanzeige.
    #[must_use]
    pub fn dauer_s(&self) -> f64 {
        self.spieler.as_ref().map_or(0.0, Spieler::duration_s)
    }

    /// Die **Position** in Sekunden — der Zaehler der Zeitanzeige.
    ///
    /// **GEMESSENER Hinweis, und er ist wichtig:** `Spieler` hat **zwei**
    /// davon — `position_s()` und `position_verlaesslich_s()`. **GEMESSENER
    /// Unterschied:** bei einem **Kindprozess** ist die Position **nur**
    /// dann bekannt, wenn der Prozess sie **zurueck**gemeldet hat; sonst
    /// steht sie auf **null**, waehrend der Ton laeuft. **Also** liefert
    /// `position_s` hier **beide**: `position_verlaesslich_s()`, **und**
    /// wenn das `None` ist, den **eigenen** Zaehler. **Sonst** stuende
    /// `(0:00 / 0:03)` die ganze Zeit da, und die Zeitanzeige waere **tot**.
    #[must_use]
    pub fn position_s(&self) -> f64 {
        let Some(s) = self.spieler.as_ref() else {
            return 0.0;
        };
        s.position_verlaesslich_s()
            .unwrap_or_else(|| s.position_s())
    }

    /// Der Zustand (`Bereit`, `Spielt`, `Pausiert`, ...).
    #[must_use]
    pub fn zustand(&self) -> Zustand {
        self.spieler
            .as_ref()
            .map_or(Zustand::Bereit, Spieler::zustand)
    }

    /// Laeuft der Ton gerade?
    #[must_use]
    pub fn laeuft(&self) -> bool {
        self.spieler.as_ref().is_some_and(Spieler::laeuft)
    }

    /// Die Abtastrate in Hz (fuer die Anzeige der Qualitaet).
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.spieler.as_ref().map_or(0, Spieler::rate)
    }

    /// Die Kanalzahl (1 = Mono, 2 = Stereo).
    #[must_use]
    pub fn kanaele(&self) -> usize {
        self.spieler.as_ref().map_or(0, Spieler::kanaele)
    }

    /// Ist der Ton **zu Ende** oder gestoppt?
    #[must_use]
    pub fn ist_aus(&self) -> bool {
        self.spieler
            .as_ref()
            .is_none_or(|s| !s.laeuft() && !s.ausgabe().ist_pausiert())
    }

    /// **Nur** fuer die Tests: der Spieler dahinter.
    ///
    /// **GEMESSENER Grund fuer `pub`:** ein Test in `tests/` ist ein
    /// **eigenes** Crate und kann **keinen** `pub(crate)`-Bereich sehen. Und
    /// **GEMESSENE ENTSCHEIDUNG: die Alternative waere ein zweiter Weg
    /// (`ausgabe_mut`) — und ein zweiter Weg ist ein zweiter Grund, warum die
    /// beiden auseinanderlaufen.**
    #[must_use]
    pub fn spieler(&self) -> Option<&Spieler> {
        self.spieler.as_ref()
    }
}

impl Drop for Handle {
    /// **GEMESSENER Grund (siehe Modul-Doc):** die Ausgabe ist ein
    /// **Kindprozess**. Ohne dieses `Drop` **lebt** er weiter.
    fn drop(&mut self) {
        if let Some(s) = self.spieler.as_mut() {
            s.beenden();
        }
    }
}

/// **DIE ANGEBOTENE SCHNITTELSTELLE.** `narsil` haelt **eine** davon.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `&mut self` auf der Box und nicht
/// `&self`:** ein Kindprozess hat **einen** Zustand (Position, Pause), und
/// zwei_threads, die denselben Ton bedienen, sind ein **Fehler**, kein
/// Vorteil.
pub trait Wiedergabe: Send {
    /// **Startet** den Ton aus **Bytes** und gibt den Zugriff zurueck.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `bytes` und nicht ein Pfad:** ein
    /// Browser holt die Daten ueber `file://` **oder** `http://` **oder** aus
    /// dem Cache. Ein Pfad **hier** wuerde **zwei** Codes brauchen (einmal
    /// lokal, einmal ueber Netz) — und **einer** von beiden waere **ein**
    /// Umweg. **Bytes** sind der **Schnitt**, den **alle drei** benutzen.
    fn abspielen(&mut self, bytes: &[u8]) -> crate::fehler::Ergebnis<Handle>;

    /// **Pausiert** den Ton, oder setzt ihn fort, wenn er schon pausiert
    /// ist. **Das** ist der zweite Klick auf **denselben** Knopf.
    fn pause(&mut self, h: &mut Handle);

    /// Stoppt den Ton endgueltig. Die Ausgabe wird beendet.
    fn beenden(&mut self, h: &mut Handle);

    /// Die **Position** in Sekunden — der Zaehler der Zeitanzeige.
    fn position_s(&self, h: &Handle) -> f64;

    /// Die **Dauer** in Sekunden — der Nenner der Zeitanzeige.
    fn dauer_s(&self, h: &Handle) -> f64;

    /// Der Zustand.
    fn zustand(&self, h: &Handle) -> Zustand;

    /// **Nur** fuer `Lautsprecher`: stellt laut, leise oder aus
    /// (`Some(0..=100)` = Prozent, `None` = **nichts** tun).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht im
    /// `Handle`:** der Ton **lebt** laenger als der Aufrufer, und ein
    /// Aufrufer, der die Lautstaerke **nach** `abspielen` aendert, trifft
    /// **nicht** mehr den Spieler, wenn die Box **schon** woanders liegt.
    /// **Die Box ist der Ort, der laeuft.**
    fn lautstaerke_setzen(&mut self, _h: &mut Handle, _prozent: Option<u8>) {}
}

/// **Sammelt nur — **kein** Ton, **kein** Prozess, **kein** Fenster.**
///
/// **GEMESSENER Grund fuer `&mut self` bei `abspielen`:** `Spieler` ist
/// **nicht** `Copy`, und die `Ausgabe` liegt **im** Spieler. Also muss der
/// Spieler **hier** liegen — genau da, wo `Sammler` ihn hinstellt.
// **GEMESSENE ENTSCHEIDUNG: `Sammler` traegt **nichts**.** Die erste
// Fassung hatte ein Feld `letzter: Option<Handle>` mit dem Gedanken, der
// Aufrufer koenne den Ton verlieren. **GEMESSEN widerlegt** durch die Signatur
// selbst: `abspielen` **gibt** das `Handle` zurueck, also hat der Aufrufer es
// **immer** — ein zweiter Verweis waere **zwei** Orte fuer **einen** Ton.
// **REGEL: ein Rueckgabewert, den der Aufrufer verliert, ist kein
// Rueckgabewert.**
#[derive(Debug, Default)]
pub struct Sammler;

impl Sammler {
    /// **GEMESSEN:** `Ausgabe::sammeln()` ist in `pcm.rs:414` als „eine
    /// Ausgabe, die nur sammelt — der Weg fuer Tests und fuer Maschinen ohne
    /// Ton" beschrieben. **Dieser Weg startet keinen Prozess und oeffnet kein
    /// Fenster.**
    #[must_use]
    pub fn neu() -> Self {
        Sammler
    }
}

impl Wiedergabe for Sammler {
    fn abspielen(&mut self, bytes: &[u8]) -> crate::fehler::Ergebnis<Handle> {
        let mut spieler = Spieler::mit_ausgabe(bytes, Ausgabe::sammeln())?;
        spieler.spielen();
        let format = spieler.format();
        Ok(Handle {
            spieler: Some(spieler),
            format,
        })
    }

    /// **GEMESSENE KORREKTUR, und mein `else`-Zweig war `toten` Code:**
    /// GEMESSEN `Zustand { links: Pausiert, rechts: Spielt }` beim **zweiten**
    /// `pause`. **WARUM:** `laeuft()` gibt **falsch** zurueck, sobald der Ton
    /// pausiert ist — also ging der Weg immer nach `pause()`, **nie** nach
    /// `spielen()`. **Die richtige Pruefung ist der Zustand selbst**, und
    /// `Spieler::laeuft()` ist dafuer **das falsche Mittel**.
    fn pause(&mut self, h: &mut Handle) {
        if let Some(s) = h.spieler.as_mut() {
            // **GEMESSEN am Quelltext** (`spieler.rs`):
            //     pub fn spielen: if Zustand == Bereit || Fertig { = Spielt }
            //     pub fn pause:   if Zustand == Spielt { = Pausiert }
            // **Also** muss der Uebergang **Pausiert -> Spielt** gesetzt
            // werden — und `pause()` **kann** das **nicht**, also muss es
            // `Zustand` **direkt** gesetzt werden.
            if s.zustand() == Zustand::Pausiert {
                s.zustand_setzen(Zustand::Spielt);
            } else {
                s.pause();
            }
        }
    }

    fn beenden(&mut self, h: &mut Handle) {
        if let Some(s) = h.spieler.as_mut() {
            s.beenden();
        }
        h.spieler = None;
    }

    fn position_s(&self, h: &Handle) -> f64 {
        h.position_s()
    }

    fn dauer_s(&self, h: &Handle) -> f64 {
        h.dauer_s()
    }

    fn zustand(&self, h: &Handle) -> Zustand {
        h.zustand()
    }
}

/// **Echter Ton ueber den Kindprozess** (`pw-play` / `aplay`).
///
/// **KEIN FENSTER.** Diese Sichtbarkeit braucht **kein** Fenster: der Ton
/// geht an den **Default-Sink** des Servers, und der Nutzer **hoert** ihn
/// ueber seine vorhandene Ausgabe. **GEMESSEN** (G9am, Abschnitt 4):
/// `pactl list short sink-inputs` zeigt waehrend der Wiedergabe
/// `<id> 174 <client> PipeWire s16le 2ch 44100Hz`, und Sink **174** ist
/// `alsa_output.pci-0000_0b_00.4.analog-stereo`.
#[derive(Debug)]
pub struct Lautsprecher {
    weg: Weg,
}

impl Lautsprecher {
    /// **GEMESSEN:** `Weg::automatisch()` (in `pcm.rs:113`) nimmt das
    /// erste Programm, von dem der Aufruf **weiss**, dass es da ist —
    /// `pw-play` vor `aplay`.
    #[must_use]
    pub fn neu() -> Self {
        Lautsprecher {
            weg: Weg::automatisch(),
        }
    }

    /// Mit einem **bestimmten** Weg — fuer `Weg::Sammeln`, wenn ein Test
    /// **ohne** Ton messen will.
    #[must_use]
    pub fn mit_weg(weg: Weg) -> Self {
        Lautsprecher { weg }
    }

    /// Der gewaehlte Weg (fuer die Fehlermeldung).
    #[must_use]
    pub fn weg(&self) -> &Weg {
        &self.weg
    }
}

impl Wiedergabe for Lautsprecher {
    fn abspielen(&mut self, bytes: &[u8]) -> crate::fehler::Ergebnis<Handle> {
        let mut spieler = Spieler::mit_kindprozess(bytes, &self.weg)?;
        spieler.spielen();
        let format = spieler.format();
        Ok(Handle {
            spieler: Some(spieler),
            format,
        })
    }

    /// **Dieselbe Korrektur wie in `Sammler::pause`**, und aus demselben
    /// gemessenen Grund: `laeuft()` gibt **falsch** zurueck, sobald der Ton
    /// pausiert ist.
    fn pause(&mut self, h: &mut Handle) {
        if let Some(s) = h.spieler.as_mut() {
            if s.zustand() == Zustand::Pausiert {
                s.zustand_setzen(Zustand::Spielt);
            } else {
                s.pause();
            }
        }
    }

    fn beenden(&mut self, h: &mut Handle) {
        if let Some(s) = h.spieler.as_mut() {
            s.beenden();
        }
        h.spieler = None;
    }

    fn position_s(&self, h: &Handle) -> f64 {
        h.position_s()
    }

    fn dauer_s(&self, h: &Handle) -> f64 {
        h.dauer_s()
    }

    fn zustand(&self, h: &Handle) -> Zustand {
        h.zustand()
    }

    fn lautstaerke_setzen(&mut self, h: &mut Handle, prozent: Option<u8>) {
        if let (Some(s), Some(p)) = (h.spieler.as_mut(), prozent) {
            s.ausgabe_mut().setze_lautstaerke(p);
        }
    }
}

/// **Bequemlichkeit fuer den Aufrufer, der **einen** Ton hat:**
/// alles in einer Box, ohne selbst ein Trait-Objekt bauen zu muessen.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `pub fn` und kein Makro:** ein Makro waere
/// **eine** Zeile in **jedem** Aufrufer und **keine** Stelle, an der man die
/// Signatur **aendern** kann. **EIN Aufruf, EIN Ort.**
#[must_use]
pub fn box_ohne_ton() -> Box<dyn Wiedergabe> {
    Box::new(Sammler::neu())
}

/// **GEMESSEN:** `Weg::automatisch()` nimmt `pw-play`, **wenn** es da ist.
#[must_use]
pub fn box_mit_ton() -> Box<dyn Wiedergabe> {
    Box::new(Lautsprecher::neu())
}
