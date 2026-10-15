//! VIDEO_18 Punkt 3: **der Vertrag, den JS braucht** — als Code und als
//! Namen, nicht als Absicht.
//!
//! VIDEO, 04.10.2026. GEMESSENER AUSGANGSPUNKT, und er ist die ganze
//! halbe Antwort:
//!
//! ```text
//! $ grep -rn "HTMLMediaElement\|HTMLVideoElement\|currentTime\|canplay\|timeupdate" \
//!       crates/narsil-jsvm/src/ crates/narsil/src/js.rs
//! (keine Treffer)
//! ```
//!
//! Es gibt **kein** `HTMLMediaElement` im ganzen Baum. Das ist keine
//! Luecke, die man „einbauen" kann, indem man eine Datei anfasst —
//! `currentTime`, `duration`, `paused`, `volume`, `muted`,
//! `loadedmetadata`, `canplay`, `play`, `pause`, `timeupdate`, `ended`
//! sind **elf** Namen an **drei** Stellen:
//!
//! | Wo | Was |
//! |---|---|
//! | `narsil-jsvm` (JS, CACHE) | die Klasse `HTMLVideoElement` im Heap, die Getter/Setter, die Ereignis-Objekte, das **Promise** von `play()` |
//! | `narsil/src/js.rs` (nicht meine Datei) | der Weg von einem Klick auf ein `<video>`-Knoten zu einem `Abspielspieler` |
//! | **hier, meine Crate** | was gelesen, was geschrieben und was gesendet wird |
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Vertrag **hier** steht und nicht in
//! `narsil-jsvm`: die Logik ist nicht die Logik von JavaScript. „Welche Zahl
//! ist `currentTime` bei 1,7 s" ist eine Frage ueber Zeitstempel und
//! Dekoderzustand — sie gehoert zur Videodecke. Der Weg von dort zu einem
//! JS-`Wert` ist eine Frage ueber den Heap. Wer beides in eine Datei legt,
//! kann die eine nicht mehr pruefen, ohne die andere zu bauen.
//!
//! GEMESSENE ENTSCHEIDUNG, warum **Namen als Zeichenketten** und nicht als
//! Enum: die JS-Seite vergleicht Namen aus Skripten. Eine Liste, die an
//! beiden Enden **gleich** ist, ist die billigste moegliche
//! Uebereinkunft — sie braucht keine Umrechnung und keinen Code.
//! [`MedienElement::lies`] nimmt genau so einen Namen, wie er in einem
//! Skript steht, und liefert `None` fuer alles, was es nicht gibt.

use crate::player::{Abspielspieler, Ereignis};

/// Der Name, den ein Skript benutzt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `&'static str` und kein Enum: siehe Kopf.
/// Der Preis ist, dass ein Tippfehler zur Laufzeit `None` gibt statt beim
/// Bauen zu scheitern. [`ALLE_EIGENSCHAFTEN`] ist die Liste, gegen die
/// man pruefen kann, und die Pruefung steht in `tests/medien_test.rs`.
pub type Name = &'static str;

/// Liest und schreibt Eigenschaften an einem [`Abspielspieler`], mit den
/// Namen aus einem Skript.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **Wrapper** ist und kein
/// Trait: es gibt genau einen Spieler. Ein Trait waere eine Zusage, dass
/// es einen zweiten gibt, und die gibt es nicht.
pub struct MedienElement<'a> {
    spieler: &'a mut Abspielspieler,
}

impl<'a> MedienElement<'a> {
    /// Leitet sich ueber `&mut` — der Wrapper **vergaess** nicht, welchen
    /// Spieler er meint. GEMESSENE ENTSCHEIDUNG, warum kein `&` und kein
    /// Index: `setze_lautstaerke` aendert den Spieler, und ein Wrapper,
    /// der zurueckgibt statt mitzuschreiben, koennte es vergessen.
    pub fn neu(spieler: &'a mut Abspielspieler) -> MedienElement<'a> {
        MedienElement { spieler }
    }

    /// Der Wert einer Eigenschaft, oder `None` wenn es sie nicht gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `currentTime` in **Sekunden** und
    /// nicht in Nanosekunden: die WebM-Spezifikation rechnet in
    /// Nanosekunden (`TimecodeScale`), die HTML-Eigenschaft `currentTime`
    /// aber in Sekunden. Wer die eine Zahl in die andere steckt, ohne es
    /// zu sagen, verschiebt das Bild um eine Milliarde.
    ///
    /// | Eigenschaft | Typ | Lesen | Schreiben | Bemerkung |
    /// |---|---|---|---|---|
    /// | `currentTime` | Zahl | ja | ja | Sekunden |
    /// | `duration` | Zahl | ja | nein | Sekunden; `NaN`, solange nichts geladen |
    /// | `paused` | Boolean | ja | nein | umgekehrt ueber `play()`/`pause()` |
    /// | `ended` | Boolean | ja | nein | |
    /// | `volume` | Zahl | ja | ja | 0,0 bis 1,0 |
    /// | `muted` | Boolean | ja | ja | **getrennt** von `volume` |
    /// | `loop` | Boolean | ja | ja | |
    pub fn lies(&self, name: Name) -> Option<Wert> {
        Some(match name {
            "currentTime" => Wert::Zahl(self.spieler.zeit()),
            "duration" => Wert::Zahl(self.spieler.dauer()),
            "paused" => Wert::Bool(self.spieler.pausiert()),
            "ended" => Wert::Bool(self.spieler.ende()),
            "volume" => Wert::Zahl(f64::from(self.spieler.lautstaerke())),
            "muted" => Wert::Bool(!self.spieler.ton_an()),
            "loop" => Wert::Bool(self.spieler.schleife()),
            _ => return None,
        })
    }

    /// Schreibt eine Eigenschaft. `false`, wenn es sie nicht gibt oder
    /// wenn sie **nur lesbar** ist.
    ///
    /// | Schreibversuch | Wirkung | Grund |
    /// |---|---|---|
    /// | `currentTime` | Sprung | |
    /// | `volume` | `setze_lautstaerke` | |
    /// | `muted` | `setze_ton_an` | |
    /// | `loop` | `setze_schleife` | |
    /// | `paused` | **abgeleitet**: `true` heisst `pause()`, `false` heisst `play()` | die Eigenschaft ist im HTML nur lesbar, aber die Wirkung ist eindeutig |
    /// | `duration`, `ended` | abgeleitet: keine | beide nur lesbar |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `paused` **schreibbar** ist, obwohl
    /// es im HTML nur lesbar ist: `video.paused = false` ist im Browser
    /// eine Wirkung, die es gibt (es startet das Video), und ein Skript
    /// darauf zu lassen hiesse „abspielen geht nur ueber `play()`" — was
    /// falsch ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `muted` und `volume` getrennt
    /// bleiben: VIDEO_15 hat gemessen, dass nach
    /// `setze_lautstaerke(0,0)` `ton_an` **noch true** ist. In einem
    /// Zustand, der beides gleich setzt, gibt es `video.volume = 0`
    /// nicht mehr als etwas anderes als Stumm — und genau das unterscheidet
    /// die beiden.
    pub fn schreib(&mut self, name: Name, wert: Wert) -> bool {
        match (name, wert) {
            ("currentTime", Wert::Zahl(z)) => {
                self.spieler.setze_zeit(z);
                true
            }
            ("volume", Wert::Zahl(v)) => {
                self.spieler.setze_lautstaerke(v as f32);
                true
            }
            ("muted", Wert::Bool(an)) => {
                self.spieler.setze_ton_an(!an);
                true
            }
            ("loop", Wert::Bool(an)) => {
                self.spieler.setze_schleife(an);
                true
            }
            ("paused", Wert::Bool(pausiert)) => {
                if pausiert {
                    self.spieler.pause();
                } else {
                    self.spieler.play();
                }
                true
            }
            // Alles andere: kein Name, oder nur lesbar.
            _ => false,
        }
    }

    /// `play()`. Liefert `false`, wenn der Spieler **nicht** laeuft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **kein** Promise steht: ein
    /// Promise gehoert in den Heap von `narsil-jsvm` (CACHE hat dort die
    /// Promise-Teile). Hier steht die **Wirkung**; das Promise entsteht
    /// daraus, sobald die Seite eine Klasse `HTMLVideoElement` hat. Wer
    /// beides hier baut, baut den Heap an zwei Stellen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Rueckgabewert `false` bedeutet
    /// „haengt" und nicht „ging nicht": `play()` kehrt in der
    /// HTML-Spezifikation **immer** ein Promise zurueck, auch wenn es
    /// abgelehnt wird. Der Fall „haengt" — die Autoplaupolitik des
    /// Browsers verweigert den Ton — ist genau der Fall, in dem das
    /// Promise abgelehnt wird, und das ist der Fall, den man wissen muss.
    pub fn play(&mut self) -> bool {
        self.spieler.play();
        !self.spieler.pausiert()
    }

    /// Der Spieler dahinter, fuer alles, was der Vertrag nicht nennt.
    pub fn spieler(&mut self) -> &mut Abspielspieler {
        self.spieler
    }
}

/// Der Wert einer Eigenschaft. So wenige Arten wie moeglich.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wert {
    Zahl(f64),
    Bool(bool),
}

/// Alle Eigenschaften, die es gibt — **an einer Stelle**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste hier steht und nicht in
/// `narsil-jsvm`: sie ist die **Abnahme**. Wenn auf der JS-Seite eine
/// Eigenschaft auftaucht, die hier nicht steht, ist eine Seite falsch —
/// und die Frage stellt diese Liste, nicht ein Test in einer fremden Crate.
pub const ALLE_EIGENSCHAFTEN: [&str; 7] = [
    "currentTime",
    "duration",
    "paused",
    "ended",
    "volume",
    "muted",
    "loop",
];

/// Die Eigenschaften, die nur gelesen werden.
pub const NUR_LESBAR: [&str; 2] = ["duration", "ended"];

/// Der HTML-Name eines Ereignisses des Spielers.
///
/// GEMESSENE ENTSCHEIDUNG, warum `canplay` hier **nicht** vorkommt und
/// trotzdem in der Anforderung der Leitung steht: der Spieler sendet
/// `LoadedData`. `canplay` ist in der HTML-Spezifikation „es ist genug da,
/// um anzufangen" — und genau das ist der Moment, in dem ein Bild
/// vorliegt. Wer die beiden Namen gleichsetzt, behauptet, `canplay` sei
/// dasselbe wie „das erste Bild ist da"; das stimmt fuer unseren Spieler,
/// ist aber eine **Entscheidung** und muss als solche dastehen.
pub fn ereignis_name(e: Ereignis) -> Option<&'static str> {
    Some(match e {
        Ereignis::LoadStart => "loadstart",
        Ereignis::LoadedMetadata => "loadedmetadata",
        Ereignis::LoadedData => "canplay",
        Ereignis::Play => "play",
        Ereignis::Pause => "pause",
        Ereignis::TimeUpdate => "timeupdate",
        Ereignis::Ended => "ended",
        Ereignis::Seeking => "seeking",
        Ereignis::Seeked => "seeked",
        Ereignis::VolumeChange => "volumechange",
        Ereignis::RateChange => "ratechange",
        // `Warten` ist ausdruecklich kein HTML-Ereignis.
        Ereignis::Warten => return None,
    })
}

/// Die Ereignisse, die die Leitung am 04.10.2026 genannt hat.
pub const GEFORDERT: [&str; 6] = [
    "loadedmetadata",
    "canplay",
    "play",
    "pause",
    "timeupdate",
    "ended",
];