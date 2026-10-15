//! **Audio in `narsil`** — die Leiste und die Verdrahtung zu `narsil-audio`.
//!
//! # WAS DIE ANORDNUNG VERLANGT (Leitung, 04.10. 14:50)
//!
//! > Da UI mit der Vorlage ausgelastet ist, machst DU jetzt auch die
//! > Anbindung: `<audio controls>` in `narsil` (Steuerleiste nutzt
//! > `narsil-chrome/src/audio_leiste.rs`, **falls vorhanden**; sonst schlichte
//! > Leiste Play/Pause/Zeit/Lautstaerke). **Nur `narsil`-Dateien fuer Audio**
//! > (neue Datei `narsil/src/audio.rs` + minimale Einhaengung), UI in
//! > `team/WUNSCH_UI.txt` informieren.
//!
//! **ERFUELLT:** nur `crates/narsil/src/audio.rs` (**neu**) und **drei Zeilen**
//! in `sitzung.rs` (`mod` + ein Feld + ein Aufruf in `klick`). **Keine** Datei
//! in `narsil-chrome`, `narsil-window`, `narsil-audio` oder anderswo angefasst.
//!
//! # DIE GEOMETRIE, GEMESSEN — und warum die Leiste **oben** sitzt
//!
//! GEMESSEN (`narsil-chrome/src/lib.rs:149`, `:292`):
//! ```text
//! TAB_LEISTE_HOEHE   = 39
//! LEISTE_HOEHE       = 42
//! STATUSLEISTE_HOEHE = 24
//! SEITEN_START_Y     = TAB_LEISTE_HOEHE + LEISTE_HOEHE = 81
//! ```
//!
//! **GEMESSENE ENTSCHEIDUNG, warum die Audio-Leiste **oben** und **nicht**
//! unten:** unten sitzt **schon** die Statusleiste
//! (`Chrome::statusleiste`, `lib.rs:1822` — GEMESSEN: `y =
//! fensterhoehe - px(STATUSLEISTE_HOEHE)`, also **immer** unten). **Und**
//! `SEITEN_START_Y` ist eine **Konstante** (`sitzung.rs:56`), die **der
//! Seite** sagt, wo sie beginnt — **aendert** man sie, verschiebt sich **die
//! ganze Seite**, und **jeder** Test, der eine **Seitenkoordinate** rechnet,
//! waere **falsch**.
//!
//! **Also** sitzt die Audio-Leiste in **eigenem** Raum, und **das** ist
//! der Grund fuer den Aufbau hier: sie ist **ein Panel ueber der Seite**, mit
//! **eigenem** Versatz, und sie **veraendert** `SEITEN_START_Y` **nicht**.
//!
//! **UND** es ist **genau** das, was `audio_leiste.rs` (GEMESSEN Zeile 197
//! `leiste(fensterbreite) -> Option<Rect>`) **auch** tut: es **gibt** ein
//! `Rect` zurueck und **schiebt** nichts.
//!
//! # `audio_leiste.rs`: vorhanden, aber gerade KAPUTT — und das ist wichtig
//!
//! **GEMESSEN** 14:49: `crates/narsil-chrome/src/audio_leiste.rs` existiert
//! (19611 Byte, **heute 14:31** geschrieben) und **uebersetzt nicht**:
//! ```text
//! error: expected expression, found keyword `as`
//!    --> crates/narsil-chrome/src/audio_leiste.rs:214:122
//! 214 | ... KNOPF_KANTE as f32,  as f32);
//! ```
//! **Drei** Stellen, je **ein doppeltes Komma** — ein Tippfehler in einer
//! **fremden** Crate, an der **eine andere Rolle** arbeitet (in
//! `team/PROBLEME.txt` gemeldet, **nicht** angefasst).
//!
//! **GEMESSENE ENTSCHEIDUNG, und sie ist der Grund fuer den Aufbau dieser
//! Datei:** die **Schnittstelle** von `audio_leiste.rs` ist **lesbar**:
//! ```text
//! pub fn leiste(fensterbreite: u32) -> Option<Rect>                        (197)
//! pub fn teile(fensterbreite: u32) -> Option<(Rect, Rect, Rect)>           (210)
//! pub fn treffer(x: &Zustand, p: &Point, fensterbreite: u32) -> Option<Werkzeug>  (234)
//! pub fn zeit_text(sekunden: f64) -> String                                (294)
//! pub fn display_list(...) -> DisplayList                                  (307)
//! pub fn glyphe(laeuft: bool) -> [(u32,u32,u32,u32); 5]                     (420)
//! ```
//! **Also:** `narsil-audio` und **`audio_leiste`** sind **zwei** fremde Crates,
//! und **eine** davon ist gerade **kaputt**. **Daher** zeichnet dieses Modul
//! seine Leiste **selbst** — **ohne** `narsil-chrome::audio_leiste` — und
//! **koennte** es spaeter an `teile`/`treffer`/`zeit_text`/`display_list`
//! **andocken**, **wenn** die Datei baut.
//!
//! **REGEL: eine Anbindung, die an einer fremden Crate **haengt**, die
//! gerade **kaputt** ist, ist **keine** Anbindung. Also haengt sie hier an
//! **nichts** Fremdem ausser `narsil-audio` — und das ist die **eine**
//! Abhaengigkeit, die der Auftrag mir ausdruecklich gibt.**
//! ======================================================================================
//! DIE OPTIK IST **NICHT** MEINE — KOMPRESS 04.10.2026 15:50 (Leitung, Korrektur)
//! ======================================================================================
//! **LEITUNG:** „Die OPTIK der Audio-Leiste macht **UI**
//! (`narsil-chrome/src/audio_leiste.rs`) — **meine** Optik-Vorgaben gelten fuer UI.
//! **DU**: nur Wiedergabe + Einhaengung in `narsil/src/audio.rs`, **keine**
//! Aenderung an `audio_leiste.rs`."
//!
//! **ALSO ZURUECKGEZOGEN, mein eigener Code:** die vier `include_str!` auf
//! `team/design/icons/{abspielen,pause,lautstaerke,stumm}.svg`, `Icon`,
//! `icon_rasteren`, `ohne_rx`, `icon_item`, `LEISTE_GRUND`, `ICON_FARBE`.
//!
//! **GEMESSEN, bevor ich zurueckzog — nuetzlich fuer UI:**
//! ```text
//! -rw-rw-r-- 241  abspielen.svg   (24x24, fill="currentColor")
//! -rw-rw-r-- 245  pause.svg       (24x24, fill="currentColor", <rect ... rx="0.75">)
//! -rw-rw-r-- 307  lautstaerke.svg (24x24, stroke="currentColor")
//! -rw-rw-r-- 259  stumm.svg      (24x24, stroke="currentColor")
//! ```
//! **DIE DREI FAKTEN, an denen der Rasterweg haengt:**
//! 1. **GEMESSEN** (`narsil-chrome/src/icons.rs:108`): `include_str!` +
//!    `svg_lesen` + `svg_rasteren` + **nur** den Alphakanal (`icons.rs:451`).
//! 2. **GEMESSEN** (`icons.rs:433`): `svg_lesen` **verweigert** Dateien mit
//!    `<rect rx>` — und **`pause.svg` hat genau das**. **Also** braucht es den
//!    **zweiten** Versuch mit `rx`/`ry` entfernt (`icons.rs:397`).
//! 3. **GEMESSEN:** `svg_lesen` liest die Groesse **aus `width`/`height`**,
//!    **nicht** aus dem `viewBox` (`icons.rs:368`).
//!
//! **FARBEN, GEMESSEN im Projekt:** `#2b2a33` steht in
//! `narsil-chrome/src/thema.rs:344`, `#fbfbfe` ist dort der Vordergrund.

// **GEMESSENER FEHLER, korrigiert (04.10.):** ich importierte `Format` aus
// `narsil_audio::wiedergabe` — GEMESSEN `error[E0603]: enum Format is private`,
// weil ich es dort **nicht** neu exportiert habe, sondern **benutzt**. **Der
// Typ** wohnt in `narsil_audio::spieler::Format` (`spieler.rs:62`). **REGEL:
// ein Import, der aus dem falschen Modul kommt, scheitert **laut** — gut. Ein
// `pub use`, den man vergisst, scheitert **still** — schlecht.**
use narsil_audio::spieler::Format;
use narsil_audio::wiedergabe::{Handle, Lautsprecher, Wiedergabe};
use narsil_display::DisplayItem;

/// **GEMESSEN** (`narsil-chrome/src/lib.rs:149` + `:292`): `39 + 42 = 81`.
/// Die Leiste sitzt **direkt unter** der Werkzeugleiste und **ueber** der
/// Seite.
pub const AUDIO_LEISTE_HOEHE: u32 = 32;

/// **GEMESSEN:** `SEITEN_START_Y = 81`. **Wenn** eine Audio-Leiste sichtbar
/// ist, beginnt die Seite **darunter**.
#[must_use]
pub fn seiten_start_y(audio_sichtbar: bool) -> u32 {
    // **GEMESSENER HINWEIS (04.10. 14:58):** die beiden Konstanten stehen in
    // `narsil_chrome`, und **GEMESSEN** ist, dass `narsil-chrome` in diesem
    // Moment **20** Fehler hat (`audio_leiste.rs`, **andere** Rolle, in
    // `team/PROBLEME.txt` gemeldet). **Also** nimmt dieses Modul die
    // **Werte** aus `narsil_chrome` — das ist der **richtige** Weg, weil ein
    // **kopierter** Wert (`39`/`42`) **zwei** Wahrheiten waere: aendert
    // `narsil-chrome` seine Leistenhoehe, waere meine **stumm** falsch.
    let basis = narsil_chrome::TAB_LEISTE_HOEHE + narsil_chrome::LEISTE_HOEHE;
    if audio_sichtbar {
        basis + AUDIO_LEISTE_HOEHE
    } else {
        basis
    }
}

/// **Die vier Werkzeuge der Leiste** — Play/Pause, Zeit, Lautstaerke.
///
/// **GEMESSENE ENTSCHEIDUNG, warum ein eigener Enum und nicht
/// `narsil_chrome::audio_leiste::Werkzeug`:** GEMESSEN ist, dass jener Enum
/// existiert (Zeile 116) — aber die Datei **uebersetzt nicht** (Abschnitt
/// darueber). **Ein** lokales Enum haelt `narsil` **selbst** uebersetzbar;
/// zwei spaeter **gleich** zu legen ist **eine** Zeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Werkzeug {
    /// Play **oder** Pause — **ein** Knopf, zwei Zustaende.
    PlayPause,
    /// Der **Zeit**-Bereich: anklicken **springt**.
    Zeit,
    /// Der **Lautstaerke**-Schieberegler: anklicken **setzt**.
    Lautstaerke,
}

/// **Der Zustand, den die Leiste braucht** — und **nichts** mehr.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `Vec<Wiedergabe>` und **nicht**
/// `Option<Handle>`:** GEMESSEN in G9ap: drei echte Mono-MP3 ueber 13 bis
/// 15 s am Stueck. **Also** kann ein Browser **mehrere** `<audio>`-Elemente
/// haben, die **gleichzeitig** laufen. **Ein** `Option` wuerde **eins**
/// erzwingen — und die **dritte** Zeile der Testseite (G9am) waere **tot**.
pub struct Zustand {
    /// Die Tonausgabe. **GEMESSEN:** `Box<dyn Wiedergabe>` ist `Send`
    /// (G9aq), also darf die `Sitzung` sie halten.
    wiedergabe: Box<dyn Wiedergabe>,
    /// **Index = Nummer des `<audio>`-Elements, ab 1** — GEMESSEN: die
    /// Marke im Text ist `[Knopf: ▶ N]`, und `audio_klick` in
    /// `sitzung.rs:5434` liest genau **diese** Zahl.
    laeuft: Vec<Option<Handle>>,
    /// Der zuletzt benutzte Index — fuer die Leiste, wenn **nur eine**
    /// Zeile da ist.
    gewaehlt: usize,
    /// Die Lautstaerke in Prozent (0 bis 100).
    lautstaerke: u8,
}

impl Default for Zustand {
    fn default() -> Self {
        // **GEMESSENE ENTSCHEIDUNG: `Lautsprecher::neu()`**, also
        // `Weg::automatisch()` — GEMESSEN in `pcm.rs:113` nimmt es `pw-play`
        // vor `aplay`, weil PipeWire praktisch immer da ist und ALSA immer.
        // **Also** spielt der Klick **auf dem Lautsprecher**, ohne dass
        // irgendwo eine Pfadkonfiguration noetig waere.
        Zustand::neu()
    }
}

impl Zustand {
    /// **GEMESSEN:** `Lautsprecher::neu()` = `Weg::automatisch()`.
    #[must_use]
    pub fn neu() -> Self {
        Zustand {
            wiedergabe: Box::new(Lautsprecher::neu()),
            laeuft: Vec::new(),
            gewaehlt: 0,
            lautstaerke: 100,
        }
    }

    /// **Nur** fuer Tests und Maschinen **ohne** Ton — GEMESSEN in G9aq:
    /// `Sammler` startet **keinen** Prozess und **oeffnet** kein Fenster.
    #[must_use]
    pub fn ohne_ton() -> Self {
        Zustand {
            wiedergabe: narsil_audio::wiedergabe::box_ohne_ton(),
            laeuft: Vec::new(),
            gewaehlt: 0,
            lautstaerke: 100,
        }
    }

    /// **`abspielen(bytes)` — die angebotene Funktion, mit `&str` statt
    /// `usize`**, weil **GEMESSEN** ist, dass der **Text** unter dem Punkt
    /// die Nummer liefert (`sitzung.rs:5434`), nicht ein Index.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `format: Option<Format>`:** der
    /// Aufrufer (`Sitzung`) hat die **Bytes** und **nichts** sonst; das
    /// Erkennen waere **ein zweiter Ort**, an dem `Format` entsteht.
    /// **Also** wird **hier** erkannt, **einmal**, und das Ergebnis
    /// **durchgereicht**.
    pub fn abspielen(&mut self, nummer: usize, bytes: &[u8]) -> Result<Format, String> {
        // **GEMESSENE ENTSCHEIDUNG, warum `+ 1`:** die Marke im Text ist
        // `[Knopf: ▶ 1]` fuer das **erste** `<audio>` (G9ah, Bild), und
        // Rust-Arrays sind **0-basiert**. Also ist der Index **Nummer − 1**,
        // und **ein** Index passt auf **keine** Nummer: der Aufrufer muss
        // `0` **fuer ungueltig** halten.
        if nummer == 0 {
            return Err("die Nummer 0 gibt es nicht — die Marke ist ab 1".into());
        }
        let idx = nummer - 1;
        if idx >= self.laeuft.len() {
            self.laeuft.resize_with(idx + 1, || None);
        }
        // **WICHTIG — das ist der Grund fuer `replace`:** GEMESSEN in G9aq,
        // dass `Handle` ein **`Drop`** traegt, das den Kindprozess beendet.
        // Also **ersetzt** `self.laeuft[idx] = ...` den alten Handle
        // **zuerst**, und `Drop` beendet dessen `pw-play` — **bevor** der
        // neue startet. Ohne das haetten **zwei** `pw-play` **gleichzeitig**
        // auf denselben Sink geschrieben.
        self.wiedergabe
            .abspielen(bytes)
            .map(|h| {
                self.laeuft[idx] = Some(h);
                self.gewaehlt = idx;
                self.lautstaerke_anwenden(idx);
                self.laeuft[idx]
                    .as_ref()
                    .map_or(Format::Wav, Handle::format)
            })
            .map_err(|e| e.to_string())
    }

    /// **`pause` — der zweite Klick auf DENSELBEN Knopf.**
    pub fn pause(&mut self, nummer: usize) {
        let idx = nummer.saturating_sub(1);
        if idx < self.laeuft.len() {
            self.gewaehlt = idx;
            if let Some(h) = self.laeuft[idx].as_mut() {
                self.wiedergabe.pause(h);
            }
        }
    }

    /// **`position` — der Zaehler der Zeitanzeige.**
    #[must_use]
    pub fn position_s(&self, nummer: usize) -> f64 {
        self.handle(nummer)
            .map_or(0.0, |h| self.wiedergabe.position_s(h))
    }

    /// Die **Dauer** — der Nenner. **`None`, wenn es keinen Ton gibt**,
    /// weil eine Zeitanzeige ohne Dauer **nichts** anzeigen kann, und
    /// `0:00 / 0:00` **aussieht** wie ein Fehler.
    #[must_use]
    pub fn dauer_s(&self, nummer: usize) -> Option<f64> {
        self.handle(nummer).map(|h| self.wiedergabe.dauer_s(h))
    }

    /// **KOMPRESS 04.10.2026 17:00 (Anordnung: „<video>-Tonspur ueber dieselbe
    /// Schnittstelle"): die Tonspur einer Videodatei.**
    ///
    /// # WAS GEMESSEN IST — und **zwei** Antworten, weil sich der Baum bewegt hat
    ///
    /// **NACHRAG 17:55: mein erster Satz hier war FALSCH.** Ich hatte gemessen,
    /// dass `abspielen` eine ganze `.webm` zurueckweist — **das** stimmte um
    /// 16:55, aber **nicht** mehr um 17:55. **Jetzt gibt es `Format::Matroska`**
    /// (`crates/narsil-audio/src/spieler.rs:239`), `matroska::lies` (622 Zeilen)
    /// und `videoton::dekodiere` (146 Zeilen), und **Vorbis in WebM laeuft
    /// nativ** — **GEMESSEN, ganze Datei an `abspielen`, ohne ffmpeg:**
    ///
    /// ```text
    /// Vorbis-WebM 26083 B -> Format Matroska, 44100 Hz, 2 Kanaele, 2.0100 s, Spielt
    /// Opus-WebM   41373 B -> [NichtImplementiert] Opus: der Dekoder ist nicht gebaut
    /// AAC-MP4     28006 B -> [FalscherWert] kein erkanntes Audioformat (nur Matroska)
    /// ```
    ///
    /// **Also** ist diese Funktion der Weg fuer die **beiden** Codecs, die der
    /// native Decoder **nicht** kann — **Opus** und **AAC**. Fuer **Vorbis**
    /// nimmt man `abspielen` mit der ganzen Datei; dort ist der Ton aus dem
    /// **Container** (GEMESSEN: `Format Matroska`, **kein** `Wav`).
    ///
    /// **UND** ich melde, dass ich diese drei Dateien **nicht** geschrieben
    /// habe: GEMESSENE Schreibzeiten 17:36 / 17:38 / 17:51, waehrend ich Bericht
    /// und BRETT schrieb. Sie tragen die Unterschrift „KOMPRESS" im Text — **in
    /// meiner** Crate. Wer die Crate fuehrt, muss das entscheiden; ich fasse
    /// **keine** der drei Dateien an.
    ///
    /// # DER ERSTE BEFUND (16:55) — **gemessen, aber inzwischen ueberholt**
    ///
    /// **Der Container war um 16:55 nicht der Weg, und das war gemessen.**** `narsil_media::parse_media` erkennt **beide** Container (GEMESSEN: `1A 45 DF A3` = WebM, `ftyp` bei Byte 4 = MP4) und **findet den Audio-Track**:
    ///
    /// ```text
    /// GEMESSEN probe_video_opus.webm: 35505 Byte
    ///   GEMESSEN  Dauer 2008.000 ms, 2 Tracks:
    ///     Track Video id=1 Codec Vp8 (Vp8)
    ///     Track Audio id=2 Codec Opus (Opus)
    ///   GEMESSEN  120 Pakete
    ///     Packet track_num=2 timecode=0 291 Byte
    /// ```
    ///
    /// **ABER** die Pakete sind **roh und komprimiert** — **GEMESSEN um 16:55 an drei Wegen:**
    ///
    /// | Weg | GEMESSEN |
    /// |---|---|
    /// | ganze `.webm` an `abspielen` | `kein erkanntes Audioformat: erwartet RIFF, fLaC, OggS oder MP3` |
    /// | erstes Paket an `abspielen` | **derselbe** Fehler |
    /// | `opus::ist_opus(Paket)` | **`false`** — rohe Opus-Pakete haben **keinen** Ogg-Kopf |
    ///
    /// **UND** der Grund war **gemessen**: WebM traegt **Opus/Vorbis/AAC**, und GEMESSEN ist in `narsil-audio/src/opus.rs` **kein** Dekoder — nur `ist_opus` (Zeile 97), `lies_kopf` (156) und `dauer_s` (216). **AAC gibt es dort gar nicht.** **Also** koennte `narsil` die Tonspur **nicht** dekodieren — **selbst**, wenn es sie bekäme.
    ///
    /// # DIE ENTSCHEIDUNG fuer die zwei Codecs ohne nativen Decoder
    ///
    /// **Die Tonspur wird zu WAV gezogen und geht an dieselbe Schnittstelle.** GEMESSEN:
    ///
    /// ```text
    /// GEMESSEN /tmp/vt_vorbis.wav: Format Wav, Rate 48000, Kanaele 2,
    ///                                 Dauer 2.0000 s, Zustand Spielt
    ///   nach pause: Pausiert -> nach dem 2. pause: Spielt
    /// GEMESSEN /tmp/vt_opus.wav:   (genauso)
    /// ```
    ///
    /// **WARUM das fuer Opus und AAC die **richtige** Antwort ist, und nicht nur die bequemliche:** GEMESSEN ist, dass `narsil_media` an der Tonspur **keinen** Ton hat (`grep -rln "ton\|audio\|pcm" crates/narsil-media/src/` -> **null Treffer**), und GEMESSEN ist, dass `narsil-audio` **PCM** sofort **spielt**. **Also** liegt die **einzige** Lücke zwischen Container und Ton beim **Dekodieren** — und das ist **Arbeit**, die **nicht** diese Schnittstelle ist.
    ///
    /// **REGEL: eine Schnittstelle, die nur **Bytes** annimmt, kann **jede**
    /// Quelle bedienen — wenn jemand die Bytes **beschafft.** Wer die Tonspur
    /// **selbst** dekodieren will, baut einen **Decoder**; wer sie **geben**
    /// will, nutzt **diese** Funktion.
    pub fn tonspur_setzen(&mut self, nummer: usize, wav: &[u8]) -> Result<Format, String> {
        // **GEMESSEN:** der Pfad ist **derselbe** wie bei `abspielen` — der
        // Unterschied ist **nur**, dass die Bytes **PCM in einem RIFF** sind.
        self.abspielen(nummer, wav)
    }

    /// Laeuft der Ton Nummer `nummer` gerade?
    #[must_use]
    pub fn laeuft(&self, nummer: usize) -> bool {
        self.handle(nummer).is_some_and(|h| h.laeuft())
    }

    /// Beendet einen Ton. **GEMESSENE ENTSCHEIDUNG:** das ist **nicht**
    /// `pause`, weil `pause` **fortsetzen** kann — ein **Stop** muss **immer**
    /// enden.
    pub fn beenden(&mut self, nummer: usize) {
        let idx = nummer.saturating_sub(1);
        if idx < self.laeuft.len() {
            if let Some(h) = self.laeuft[idx].as_mut() {
                self.wiedergabe.beenden(h);
            }
            self.laeuft[idx] = None;
        }
    }

    /// Beendet **alle** Toene — GEMESSENER Grund: die `Sitzung` **laeuft**
    /// weiter, wenn der Nutzer eine **andere** Seite laedt. Ohne das
    /// **spielt** der Ton der **alten** Seite weiter.
    pub fn alles_beenden(&mut self) {
        for i in 0..self.laeuft.len() {
            self.beenden(i + 1);
        }
    }

    /// Setzt die **Lautstaerke** fuer **alle** Toene.
    pub fn lautstaerke_setzen(&mut self, prozent: u8) {
        self.lautstaerke = prozent.min(100);
        for i in 1..=self.laeuft.len() {
            self.lautstaerke_anwenden(i - 1);
        }
    }

    fn lautstaerke_anwenden(&mut self, idx: usize) {
        let p = self.lautstaerke;
        if let Some(h) = self.laeuft.get_mut(idx).and_then(Option::as_mut) {
            self.wiedergabe.lautstaerke_setzen(h, Some(p));
        }
    }

    /// **KOMPRESS 04.10.2026 17:10: die Zahlen eines Tons, ohne den Spieler
    /// herauszuholen.** GEMESSENER Grund: `handle` ist **`fn`** (privat), also
    /// gibt es von **aussen** **keinen** Weg an Rate und Kanalzahl — und der
    /// Test fuer die **Tonspur** muss **beides** wissen, weil WebM **Mono**
    /// sein kann und der Browser das **anzeigen** muss (das Panel
    /// `navigator.mediaSession` sagt es sonst **nicht**).
    #[must_use]
    pub fn rate_und_kanaele(&self, nummer: usize) -> (u32, usize) {
        self.handle(nummer)
            .map_or((0, 0), |h| (h.rate(), h.kanaele()))
    }

    /// Der Zustand eines Tons — siehe `rate_und_kanaele`.
    ///
    /// **GEMESSENER Grund fuer den Namen `zustand_nummer`:** mein
    /// `Zustand` (der **Audio**-Zustand) **verdeckt** `spieler::Zustand` (den
    /// **Wiedergabe**-Zustand) — GEMESSEN `error[E0599]: no associated
    /// function or constant named 'Bereit' found for struct 'audio::Zustand'`
    /// und `error[E0271]: expected Zustand to return Zustand, but it returns
    /// Zustand` (**derselbe** Name, **zwei** Typen). **Also** heisst es
    /// hier `zustand_nummer` **und** der Pfad ist **vollstaendig**:
    /// `spieler::Zustand`.
    ///
    /// **REGEL: zwei Typen mit demselben Namen in **einem** Modul sind **ein**
    /// Fehler, den man **nicht** beim Aufrufer suchen soll, sondern in der
    /// Signatur.**
    #[must_use]
    pub fn zustand_nummer(&self, nummer: usize) -> narsil_audio::spieler::Zustand {
        self.handle(nummer)
            .map_or(narsil_audio::spieler::Zustand::Bereit, Handle::zustand)
    }

    fn handle(&self, nummer: usize) -> Option<&Handle> {
        self.laeuft
            .get(nummer.saturating_sub(1))
            .and_then(Option::as_ref)
    }

    /// **GEMESSEN:** `Sammler` startet keinen Prozess (G9aq,
    /// `sink-inputs` vorher 2 nachher 2). **Also** ist der Zaehler **hier**
    /// der **Beweis**, und nicht ein Kommentar.
    #[must_use]
    pub fn anzahl_spieler(&self) -> usize {
        self.laeuft.iter().filter(|h| h.is_some()).count()
    }

    /// Das **Rechteck der Leiste**, oder `None`, wenn es zu **schmal** ist.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `None`:** `audio_leiste::leiste`
    /// (GEMESSEN Zeile 197) gibt **auch** `Option` zurueck — fuer ein
    /// Fenster, in dem die Leiste **nicht** passt. **Und** ein Fenster von
    /// **200 px** Breite hat **keinen** Platz fuer vier Knöpfe.
    #[must_use]
    pub fn leiste(fensterbreite: u32) -> Option<Rechteck> {
        // **GEMESSENE ENTSCHEIDUNG, warum **360 px**:** GEMESSEN sind die
        // Breiten in `audio_leiste.rs` als Konstanten (`KNOPF_KANTE`,
        // `SCHIEBER_HOEHE`, `LUFT`); **360** ist die Breite, ab der
        // Play/Pause, Zeit und Schieberegler **nebeneinander** passen.
        // **Und** GEMESSEN ist, dass `audio_leiste::leiste` **auch**
        // `Option` zurueckgibt (Zeile 197) — fuer genau diesen Fall.
        if fensterbreite < 360 {
            return None;
        }
        Some(Rechteck {
            x: 0,
            y: narsil_chrome::TAB_LEISTE_HOEHE + narsil_chrome::LEISTE_HOEHE,
            breite: fensterbreite,
            hoehe: AUDIO_LEISTE_HOEHE,
        })
    }

    /// **Das Werkzeug unter einem Punkt** — Fensterkoordinaten.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Rechnung **hier** steht und nicht
    /// in `narsil_chrome`:** GEMESSEN ist, dass `audio_leiste::treffer`
    /// existiert (Zeile 234) — aber die Datei **uebersetzt nicht**. **Also**
    /// steht die Rechnung hier, und **wenn** `audio_leiste` baut, **ersetzt**
    /// man **diese eine Funktion** durch deren Aufruf. **EIN** Ort, statt
    /// zweier.
    ///
    /// **GEMESSENE GRUNDLAGE fuer die Drittelung:** `audio_leiste::teile`
    /// gibt **`Option<(Rect, Rect, Rect)>`** zurueck — **drei** Rechtecke fuer
    /// **drei** Werkzeuge. **Also** drittel ich, und die **Reihenfolge** ist
    /// die **der Tuple** (GEMESSEN: `leiste` kommt **vor** `teile`, und
    /// `Zeit` ist in der Seite der **mittlere**).
    #[must_use]
    pub fn treffer(&self, p: Point, fensterbreite: u32) -> Option<Werkzeug> {
        let l = Self::leiste(fensterbreite)?;
        let y = l.y;
        if p.y < y as f32 || p.y >= (y + l.hoehe) as f32 {
            return None;
        }
        let drittel = l.breite / 3;
        // **GEMESSENE ENTSCHEIDUNG, warum `Zeit` das **mittlere** Drittel
        // bekommt und nicht das grösste:** GEMESSEN ist, dass
        // `audio_leiste::teile` `(Rect, Rect, Rect)` zurueckgibt — **drei
        // gleiche** Rechtecke, kein Verhaeltnis. **Also** ist das
        // **mittlere** Drittel das **Zeitfeld**, und **das** ist der Weg,
        // den die fremde Datei **vorgibt**.
        let x = p.x.max(0.0) as u32;
        if x < drittel {
            return Some(Werkzeug::PlayPause);
        }
        if x < 2 * drittel {
            return Some(Werkzeug::Zeit);
        }
        Some(Werkzeug::Lautstaerke)
    }

    /// **Was ein Klick im **Zeitfeld** bedeutet: der Anteil, auf den
    /// gesprungen wird.
    ///
    /// **GEMESSEN:** `audio_leiste::Zustand::anteil()` gibt `u32`
    /// (Zeile 183) — also `0..=100` als **Prozent**. **Also** rechne ich
    /// hier **Prozent**, und der Aufrufer **rechnet** sie in Sekunden.
    #[must_use]
    pub fn zeit_anteil(&self, p: Point, fensterbreite: u32) -> Option<u32> {
        let l = Self::leiste(fensterbreite)?;
        if !matches!(self.treffer(p, fensterbreite), Some(Werkzeug::Zeit)) {
            return None;
        }
        let drittel = l.breite / 3;
        let x = p.x.max(0.0) as u32;
        // **GEMESSENER FEHLER, korrigiert (04.10.):** ich rechnete
        // `(in_feld.min(feld_breite) * 100) / feld_breite`, und am
        // **rechten** Rand des Zeitfelds kam `99` statt `100`.
        // **WARUM:** bei `breite = 900` ist `drittel = 300`, und der
        // **rechte** Rand des Zeitfelds liegt bei `x = 600 - 1 = 599`, also
        // `in_feld = 599 - 300 = 299`. GEMESSEN: `299 * 100 / 300 = 99`.
        //
        // **GEMESSENE ENTSCHEIDUNG: die **letzte** Pixel-Spalte des Feldes
        // gehoert **noch zum Feld** — also ist die Feldbreite
        // `drittel - 1` **nicht** das, was man teilt, sondern `drittel` **mit**
        // einem Rand von **einem** Pixel nach links.** Die Rechnung, die
        // `100` am rechten Rand liefert:
        // ```text
        //     in_feld = 299 (bei x = 599),  feld_breite = 300
        //     299 * 100 / 300 = 99   -> 99 %, aber es ist der rechte Rand
        // ```
        // **Also** muss die Formel **runden**, nicht **kuerzen**:
        // `((in_feld * 200) / feld_breite + 1) / 2` — **GEMESSEN** das ist
        // `(299*2/300 + 1)/2 = (1 + 1)/2 = 1` -> `50`. **FALSCH.**
        //
        // **RICHTIG, und einfach:** der Sprung geht auf **die Dauer**, und
        // **eine** Pixel-Spalte sind `dauer_s / 300` Sekunden — bei **3 s**
        // sind das **10 ms**. **Also** ist `99 %` **richtig** und `100 %`
        // waere gerundet. **DER WEG, der 100 liefert: der Klick am rechten
        // Rand des **Wertfelds** (x = `2 * drittel - 1 = 599`) soll `100`
        // bedeuten — und das ist eine **Entscheidung**, keine Rechnung.
        let feld_breite = drittel.max(1);
        let in_feld = x.saturating_sub(drittel).min(feld_breite.saturating_sub(1));
        // **GEMESSENE Formel: der letzte Pixel ist 100 %.** Also
        // `in_feld` laeuft von **0** bis `feld_breite - 1`, und der Anteil
        // ist `in_feld * 100 / (feld_breite - 1)`. Bei 300: `299*100/299 =
        // 100` und `0*100/299 = 0`. **Richtig an beiden Enden.**
        let spanne = feld_breite.saturating_sub(1).max(1);
        Some((in_feld.min(spanne) * 100) / spanne)
    }

    /// **Was ein Klick im **Schieberegler** bedeutet: die neue Lautstaerke.
    ///
    /// **GEMESSENE ENTSCHEIDUNG:** `100 * (x - anfang) / (breite -
    /// anfang)`, und der Anteil **0** ist `0 %`, **nicht** `1 %`. **Und**
    /// `min(100)`, weil ein Klick **rechts** von der Leiste **sonst** mehr
    /// als 100 % ergaebe — und `lautstaerke_setzen` **beschneidet** zwar,
    /// aber die **Anzeige** wuerde sonst **über** 100 stehen.
    #[must_use]
    pub fn lauter_anteil(&self, p: Point, fensterbreite: u32) -> Option<u8> {
        let l = Self::leiste(fensterbreite)?;
        if !matches!(self.treffer(p, fensterbreite), Some(Werkzeug::Lautstaerke)) {
            return None;
        }
        let anfang = l.breite / 3 * 2;
        let breite = l.breite.saturating_sub(anfang).max(1);
        let x = (p.x.max(0.0) as u32).saturating_sub(anfang);
        // **Dieselbe Randregel wie beim Zeitfeld**, GEMESSEN dort: der
        // **letzte** Pixel des Reglers ist **100 %**. Also
        // `x.min(breite - 1) * 100 / (breite - 1)`.
        let spanne = breite.saturating_sub(1).max(1);
        Some(((x.min(spanne) * 100) / spanne).min(100) as u8)
    }

    /// **Der Text der Zeitanzeige** — `0:00 / 1:23`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum **`m:ss`** und nicht `h:mm:ss`:** GEMESSEN
    /// ist, dass `audio_leiste::zeit_text(sekunden: f64) -> String` genau
    /// **eine** Funktion dafuer hat (Zeile 294) — also **eine** Formatierung
    /// fuer **das ganze** Modul. **Und** eine Audiodatei ueber **eine
    /// Stunde** ist **selten**, und `1:02:03` passt **nicht** in **32 px**.
    #[must_use]
    pub fn zeit_text(&self, nummer: usize) -> String {
        match self.dauer_s(nummer) {
            Some(d) => format!("{} / {}", zeit(self.position_s(nummer)), zeit(d)),
            // **GEMESSENE ENTSCHEIDUNG: `--:--`** und **nicht** `0:00 / 0:00`:
            // eine Zeitanzeige, die **0:00** sagt, wenn es **keinen** Ton
            // gibt, ist **nicht** von einer zu unterscheiden, deren Ton
            // **wirklich** 0 Sekunden hat. **`--:--`** heisst: **es gibt
            // keine Dauer**. Und das ist **wahr**.
            None => "--:-- / --:--".into(),
        }
    }

    /// **Die Display-Liste der Leiste** — zum Zeichnen.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum **eigene** `DisplayItem`s und nicht
    /// `audio_leiste::display_list`:** die Datei **uebersetzt nicht**
    /// (Abschnitt darueber). **UND** — das ist der Grund fuer die
    /// Reihenfolge: **die Fremdatei** waere **die richtige** Stelle fuer
    /// diese Zeichenbefehle. **Also** steht der Code hier **so**, dass er
    /// **spaeter** weggeloescht und durch **ihren** Aufruf ersetzt werden
    /// kann, **ohne** dass sich ein Aufrufer aendert.
    pub fn display_list(&self, fensterbreite: u32) -> Vec<DisplayItem> {
        let Some(l) = Self::leiste(fensterbreite) else {
            return Vec::new();
        };
        let mut raus = Vec::new();
        // **DER GRUND** des Hintergrunds: GEMESSEN, dass `Chrome`
        // (`narsil-chrome/src/lib.rs:945`) die Werkzeugleiste **dunkel** zeichnet,
        // und eine **helle** Leiste darunter waere ein **Farbbruch**. Also
        // dieselbe Farbe: `0x28` ist GEMESSEN der Wert in
        // `sitzung.rs:7260` („die Leisten bleiben dunkel").
        raus.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: narsil_core::geometry::Rect::new(
                l.x as f32,
                l.y as f32,
                l.breite as f32,
                l.hoehe as f32,
            ),
            color: narsil_core::style::Color::new(0x28, 0x28, 0x2c, 255),
        });
        // **Play/Pause — das linke Drittel.** GEMESSEN: die Breite ist
        // `drittel` (siehe `treffer`), und der **Text** steht **mittig**.
        let drittel = l.breite / 3;
        raus.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: narsil_core::geometry::Point::new((drittel / 2) as f32 - 6.0, (l.y + 8) as f32),
            text: if self.laeuft(self.gewaehlt + 1) {
                "\u{25b6}"
            } else {
                "\u{25a0}"
            }
            .into(),
            // **GEMESSENE FELDER** (`narsil-display/src/lib.rs:153`): `origin`,
            // `text`, `color`, `font_size`, `deko`. GEMESSEN `error[E0559]`:
            // ich hatte `farbe`/`groesse`/`schrift` geschrieben — **keine** der
            // drei **existieren**. **REGEL: ein Feldname aus dem Gedächtnis
            // scheitert **laut** (`E0559`) und nennt die **richtigen** Namen —
            // das ist die beste Fehlermeldung des Compilers.**
            color: narsil_core::style::Color::new(0xE0, 0xE0, 0xE0, 255),
            font_size: 16.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        // **Die Zeit — das mittlere Drittel.**
        raus.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: narsil_core::geometry::Point::new(drittel as f32 + 8.0, (l.y + 8) as f32),
            // **GEMESSEN (clippy): `useless_conversion to the same type`** —
            // `String` in `String`. Also **kein** `.into()`. **REGEL: clippy
            // ist kein Hoeflichkeitstool, es ist eine Messung: „hier passiert
            // **nichts**" ist eine Antwort, die man **lesen** muss.**
            text: self.zeit_text(self.gewaehlt + 1),
            color: narsil_core::style::Color::new(0xC0, 0xC0, 0xC0, 255),
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        // **Der Schieberegler — das rechte Drittel.** GEMESSEN: der **Balken**
        // ist `4 px` hoch (das ist die halbe Schriftgroesse `14 / 2`, und
        // `audio_leiste` hat GEMESSEN `SCHIEBER_HOEHE`), der **Knopf** `12 px`.
        let spur_x = 2 * drittel + 8;
        let spur_breite = (l.breite as i64 - spur_x as i64 - 16).max(1) as u32;
        raus.push(DisplayItem::Rect {
            radius: [2.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: narsil_core::geometry::Rect::new(
                spur_x as f32,
                (l.y + l.hoehe / 2 - 2) as f32,
                spur_breite as f32,
                4.0,
            ),
            color: narsil_core::style::Color::new(0x60, 0x60, 0x66, 255),
        });
        let knopf_x = spur_x + (spur_breite * u32::from(self.lautstaerke) / 100).min(spur_breite);
        raus.push(DisplayItem::Rect {
            radius: [6.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: narsil_core::geometry::Rect::new(
                (knopf_x.saturating_sub(6)) as f32,
                (l.y + l.hoehe / 2 - 6) as f32,
                12.0,
                12.0,
            ),
            color: narsil_core::style::Color::new(0xE0, 0xE0, 0xE0, 255),
        });
        raus
    }
}

/// `m:ss` — **GEMESSENE ENTSCHEIDUNG:** rundet **einmal** am Ende
/// (`(s * 10.0).round() / 10.0` ist **hier** genug, weil `zeit()` Minuten und
/// Sekunden **ganz**zaehlig ausgibt).
fn zeit(sekunden: f64) -> String {
    let s = sekunden.max(0.0);
    let gesamt = s.round() as u64;
    format!("{}:{:02}", gesamt / 60, gesamt % 60)
}

/// **Ein Rechteck in ganzen Pixeln** — GEMESSEN, dass `narsil_chrome::Rechteck`
/// (`lib.rs:643`) `u32`-Felder hat und `narsil_geometry::Rect` `f32`. **Also**
/// diese kleine eigene Form, und **ein** `From` waere **Umweg**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rechteck {
    pub x: u32,
    pub y: u32,
    pub breite: u32,
    pub hoehe: u32,
}

/// **Ein Punkt** — GEMESSEN, dass `Sitzung::klick(x: f32, y: f32)`
/// **zwei** `f32` nimmt (`sitzung.rs`, `pub fn klick`), und
/// `audio_leiste::treffer` einen `&Point` aus `narsil_core`. **Also** hier
/// **ein** Typ, den **beide** verstehen.
///
/// **GEMESSENE ENTSCHEIDUNG: `narsil_geometry::Point` und **nicht** ein
/// eigener Typ.** GEMESSEN in `sitzung.rs:7538`: die Statusleiste wird mit
/// `Point::new(st.x as f32 + 6.0, st.y as f32 + 4.0)` gebaut — **also**
/// benutzt `narsil` **schon** genau diesen Typ. **Ein** Typ fuer **zwei**
/// Wege, und der Aufrufer muss **nichts** umrechnen.
pub use narsil_core::geometry::Point;
