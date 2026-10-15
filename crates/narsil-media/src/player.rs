//! Punkt 3 und 4 der Leitung: Wiedergabe mit Takt und die
//! HTMLMediaElement-API — ohne Ton und ohne Leiste.
//!
//! B13, 04.10.2026 (Rolle VIDEO). GEMESSENE ENTSCHEIDUNG, warum das ein
//! eigener Weg ist und nicht `VideoPlayer::push_frame` weiterbaut:
//! `VideoPlayer` war ein **Bilddekodierer** — er nahm VP8-Nutzlasten
//! entgegen und gab fertige Bilder heraus. Er hatte keine Zeit, keinen
//! Zustand `paused`, kein `currentTime`, kein `ended` und keine
//! Ereignisse. Genau das ist die Luecke, die `<video>` auf einer Seite
//! braucht; ein Dekodierer, der Bilder in die Reihenfolge schiebt, kann
//! sie nicht fuellen.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Takt **mitgegeben** wird und nicht aus
//! der Wanduhr gelesen: ein Test, der die Wanduhr liest, schlaegt
//! unregelmaessig fehl und ist nicht wiederholbar. `takt(&mut self, jetzt)`
//! bekommt die Zeit, und derselbe Aufruf mit derselben Zeit liefert
//! dasselbe Bild. Die Leiste und der Tonkoerper rufen ihn mit ihrer Uhr.

use std::collections::VecDeque;

use narsil_image::Bild;

// 2026-10-15: VP8 and VP9 through one decoder (see `frame_decoder`).
use crate::frame_decoder::{Decoded, FrameDecoder};

/// Wie viele Bilder der Puffer haelt. Aus `video.rs` uebernommen.
pub const MAX_BILDER: usize = 3;

/// Ein Ereignis, wie es die Seite zu sehen bekommt.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine eigene Aufzaehlung und nicht ein
/// String: ein String muesste im Aufrufer noch uebersetzt werden, und
/// genau an dieser Uebersetzung entstehen die Fehler („`playing` ist
/// `wiedergabe` ist `start`"). Die Namen hier sind die aus der
/// HTMLMediaElement-Spezifikation, damit die Uebersetzung eine Zeile ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ereignis {
    LoadStart,
    LoadedMetadata,
    LoadedData,
    Play,
    Pause,
    TimeUpdate,
    Ended,
    Seeking,
    Seeked,
    VolumeChange,
    /// `playbackRate` hat sich geaendert (VIDEO_21).
    RateChange,
    /// Kein HTML-Ereignis, sondern der Zustand „es gibt nichts mehr zu
    /// zeigen, weil kein Schluesselbild vor `zeit` liegt".
    Warten,
}

/// Was `takt()` getan hat. Ein Rueckgabewert statt einer Seiteneffekt-
/// Anzeige, damit der Aufrufer entscheiden kann, ob er zeichnen muss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaktErgebnis {
    /// Unveraendert: dieses Bild ist schon das angezeigte.
    Unveraendert,
    /// Ein neues Bild wurde gerechnet; es liegt in `bild()`.
    NeuesBild,
    /// Bis `zeit` gibt es kein Bild, weil kein Schluesselbild vor ihr liegt.
    /// Der Spieler rechnet derweil das naechste.
    AufSchluesselbildWarten,
    /// Am Ende der Spur.
    Ende,
}

/// Der Spieler eines Videos: Bilder, Zeit und Zustand.
///
/// B13, 04.10.2026 (Rolle VIDEO). Der Unterschied zu `VideoPlayer` in
/// `video.rs` ist der **Takt**: dort werden Bilder in der Reihenfolge
/// entgegengenommen, hier wird gefragt, welches Bild zu einer Zeit gehoert.
// GEMESSENE ENTSCHEIDUNG, warum hier KEIN `#[derive(Debug)]` steht:
// `Bildstrom` traegt drei Referenzbilder in voller Groesse und ist nicht
// `Debug`. Es gibt hier stattdessen einen eigenen, kleinen Weg, der fuer
// eine Fehlersuche reicht — siehe `kurz()`.
pub struct Abspielspieler {
    /// Die Bilder, die schon gerechnet sind, mit ihrer Zeit.
    gerechnet: VecDeque<(u64, Bild)>,
    /// Der Decoder-Zustand. Ohne ihn ist kein Zwischenbild rechenbar.
    strom: FrameDecoder,
    /// Die videospur, deren Pakete dekodiert werden.
    videospur: u32,
    /// Die Zeit in Nanosekunden, auf die der Spieler gerade steht.
    zeit_ns: u64,
    /// Die Dauer in Nanosekunden.
    dauer_ns: u64,
    /// Die Zeit des zuletzt gezeigten Bildes.
    gezeigt_ns: Option<u64>,
    /// Pausiert. `true` ist der Anfangszustand, wie im Browser.
    pausiert: bool,
    /// Am Ende der Spur angekommen.
    ende: bool,
    /// In Bearbeitung: von `seek()` oder vom Ende auf das naechste
    /// Schluesselbild zurueckgesetzt.
    laeuft: bool,
    /// Ton an oder aus.
    ton_an: bool,
    /// In Schleife.
    schleife: bool,
    /// Lautstaerke 0,0 bis 1,0.
    lautstaerke: f32,
    /// Ereignisse, die seit dem letzten Abholen entstanden sind.
    ereignisse: VecDeque<Ereignis>,
    /// Wie viele Bilder seit dem letzten Schluesselbild abgewiesen wurden.
    ohne_referenz: u32,
}

impl Abspielspieler {
    /// Ein Spieler ohne Bilder, ohne Spur und pausiert — der Zustand eines
    /// `<video>`-Elements, dem noch kein `src` gegeben wurde.
    pub fn neu(videospur: u32, dauer_ns: u64) -> Self {
        Self {
            gerechnet: VecDeque::with_capacity(MAX_BILDER + 1),
            strom: FrameDecoder::new(),
            videospur,
            zeit_ns: 0,
            dauer_ns,
            gezeigt_ns: None,
            pausiert: true,
            ende: false,
            laeuft: false,
            ton_an: true,
            schleife: false,
            lautstaerke: 1.0,
            ereignisse: VecDeque::new(),
            ohne_referenz: 0,
        }
    }

    // ------------------------------------------------------------------
    // HTMLMediaElement-API. Die Namen sind die der Spezifikation.
    // ------------------------------------------------------------------

    /// `HTMLMediaElement.play()`
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das am Ende wieder anfaengt: im
    /// Browser laeuft ein Video nach dem Ende mit `play()` von vorn. Ohne
    /// das ist ein Video nach einmaligem Ansehen tot, und das ist der
    /// haeufigste Fall in einer Schleife.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `play()` das Zuruecksetzen selbst
    /// macht und nicht ueber `auf_schluesselbild`: diese Methode braucht
    /// die Paketliste, `play()` hat sie nicht. Der Spieler setzt hier nur
    /// den Zustand, und der **naechste** `takt()` holt das Schluesselbild
    /// bei 0 nach — der Takt ist die einzige Stelle, die Pakete sieht.
    pub fn play(&mut self) {
        if self.ende {
            self.zeit_ns = 0;
            self.gerechnet.clear();
            self.strom = FrameDecoder::new();
            self.ohne_referenz = 0;
            self.gezeigt_ns = None;
            self.ende = false;
            self.laeuft = true;
        }
        self.pausiert = false;
        self.ereignisse.push_back(Ereignis::Play);
    }

    /// Ein kurzer Zustandsbericht fuer eine Fehlersuche.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine Methode und kein `Debug`:
    /// ein `Debug` auf dem ganzen Spieler wuerde drei Referenzbilder in
    /// voller Groesse ausgeben — auf einem 4K-Video Hunderte von
    /// Megabyte. Das ist fuer eine Fehlersuche nuetzlich und fuer eine
    /// Fehlersuche im Protokoll unbrauchbar.
    pub fn kurz(&self) -> String {
        format!(
            "zeit {:.3}/{:.3}s{} bild {} puffer {} ohne_referenz {} wartet {} ende {}",
            self.zeit(),
            self.dauer(),
            if self.pausiert { " pausiert" } else { "" },
            match self.gerechnet.back() {
                Some((t, _)) => format!("{:.3}s", *t as f64 / 1e9),
                None => "keins".to_string(),
            },
            self.gerechnet.len(),
            self.ohne_referenz,
            self.laeuft,
            self.ende
        )
    }

    /// `HTMLMediaElement.pause()`
    pub fn pause(&mut self) {
        if !self.pausiert {
            self.pausiert = true;
            self.ereignisse.push_back(Ereignis::Pause);
        }
    }

    /// `HTMLMediaElement.currentTime` als setter, in Sekunden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum Sekunden und nicht Nanosekunden: das
    /// ist die Einheit der HTMLMediaElement-API, und eine Seite schreibt
    /// `video.currentTime = 1.5`. Wer die Nanosekunden nach aussen gibt,
    /// baut eine Umrechnung, die jemand vergisst.
    pub fn setze_zeit(&mut self, sekunden: f64) {
        let ziel = if sekunden.is_finite() && sekunden > 0.0 {
            (sekunden * 1e9) as u64
        } else {
            0
        };
        self.zeit_ns = ziel.min(self.dauer_ns);
        self.laeuft = true;
        self.gerechnet.clear();
        self.strom = FrameDecoder::new();
        self.ereignisse.push_back(Ereignis::Seeking);
        self.ereignisse.push_back(Ereignis::Seeked);
    }

    /// `HTMLMediaElement.currentTime` als getter.
    pub fn zeit(&self) -> f64 {
        self.zeit_ns as f64 / 1e9
    }

    /// `HTMLMediaElement.duration`, in Sekunden.
    pub fn dauer(&self) -> f64 {
        self.dauer_ns as f64 / 1e9
    }

    /// `HTMLMediaElement.paused`
    pub fn pausiert(&self) -> bool {
        self.pausiert
    }

    /// `HTMLMediaElement.ended`
    pub fn ende(&self) -> bool {
        self.ende
    }

    /// `HTMLMediaElement.muted`
    pub fn ton_an(&self) -> bool {
        self.ton_an
    }

    /// Setzt `HTMLMediaElement.muted` (Attribut `muted`).
    pub fn setze_ton_an(&mut self, an: bool) {
        if self.ton_an != an {
            self.ton_an = an;
            self.ereignisse.push_back(Ereignis::VolumeChange);
        }
    }

    /// `HTMLMediaElement.volume`, 0,0 bis 1,0 (Attribut und API).
    pub fn lautstaerke(&self) -> f32 {
        self.lautstaerke
    }

    /// Setzt `HTMLMediaElement.volume`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Volumen den Ton **nicht** stumm
    /// schaltet: im Browser sind `volume` und `muted` zwei verschiedene
    /// Dinge, und `volume = 0` ist nicht `muted`. Wer sie vermischt, kann
    /// den Zustand nicht mehr darstellen.
    pub fn setze_lautstaerke(&mut self, wert: f32) {
        let wert = if wert.is_finite() { wert.clamp(0.0, 1.0) } else { 1.0 };
        if (wert - self.lautstaerke).abs() > f32::EPSILON {
            self.lautstaerke = wert;
            self.ereignisse.push_back(Ereignis::VolumeChange);
        }
    }

    /// Attribut `loop`.
    pub fn schleife(&self) -> bool {
        self.schleife
    }

    /// Setzt das Attribut `loop`.
    pub fn setze_schleife(&mut self, an: bool) {
        self.schleife = an;
    }

    /// Wie viele Bilder seit dem letzten Schluesselbild abgewiesen wurden.
    /// Die Geschwindigkeit hat sich geaendert.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Ereignis** ist und kein
    /// stiller Feldwechsel: die HTML-Spezifikation hat dafuer `ratechange`,
    /// und eine Seite, die darauf hoert, soll es bekommen. Es ist das
    /// **einzige** neue Ereignis gegenueber VIDEO_15 — und es kam nicht
    /// dazu, weil `playbackRate` gebaut wurde, sondern weil die
    /// Geschwindigkeit vorher nirgends lag.
    pub fn geschwindigkeit_geaendert(&mut self) {
        self.ereignisse.push_back(Ereignis::RateChange);
    }

    pub fn ohne_referenz(&self) -> u32 {
        self.ohne_referenz
    }

    /// Das Bild, das zu `zeit_ns` gehoert, oder `None`.
    pub fn bild(&self) -> Option<&Bild> {
        self.gerechnet.back().map(|(_, b)| b)
    }

    /// Die Ereignisse seit dem letzten Aufruf, in der Reihenfolge.
    pub fn ereignisse_holen(&mut self) -> Vec<Ereignis> {
        self.ereignisse.drain(..).collect()
    }

    // ------------------------------------------------------------------
    // Der Takt
    // ------------------------------------------------------------------

    /// Fuettert **ein** Videopaket ein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Spieler Pakete annimmt und nicht
    /// Bilder: die Zeit steht am Paket, nicht am Bild. Wer die Zeit
    /// ausserhalb fuehrt, verliert sie beim Nachladen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Paket einer anderen Spur verworfen
    /// wird statt als Fehler: `extract_packets` liefert Video- **und**
    /// Tonpakete in einer Liste. Der Bilddekodierer darf die Tonpakete
    /// nicht anfassen und darf sich nicht daran stoeren.
    ///
    /// Rueckgabe: `true`, wenn das Bild dekodierbar war.
    pub fn paket_ein(&mut self, spur: u32, zeit_ns: u64, schluessel: bool, daten: &[u8]) -> bool {
        if spur != self.videospur {
            return false;
        }
        if schluessel {
            // Ein Schluesselbild beendet die Kette: der Decoderzustand
            // wird neu, sonst mischt der Zustand die Referenzen zweier
            // Schluesselbilder.
            self.strom = FrameDecoder::new();
        }
        match self.strom.decode(daten) {
            Ok(Decoded::Frame(b)) => {
                if self.gerechnet.len() >= MAX_BILDER + 1 {
                    self.gerechnet.pop_front();
                }
                self.gerechnet.push_back((zeit_ns, b));
                true
            }
            // A hidden VP9 frame: the shown picture stays.
            Ok(Decoded::Hidden) => true,
            Err(_) => {
                self.ohne_referenz = self.ohne_referenz.saturating_add(1);
                false
            }
        }
    }

    /// Setzt den Spieler auf ein neues Schluesselbild zurueck und wartet auf
    /// das Paket, das zu `ziel_ns` gehoert.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Spieler das Schluesselbild SELBER
    /// sucht und nicht dem Aufrufer die Wahl laesst: ein seeking auf 1,7 s
    /// in ein Bild, das alle 0,5 s ein Schluesselbild hat, kann nicht bei
    /// 1,7 s rechnen — es braucht das von 1,5 s. Wer das dem Aufrufer
    /// laesst, baut in jede Leiste dieselbe Schleife.
    ///
    /// `pakete` sind alle Videopakete der Spur, in Zeitordnung. Sie werden
    /// **nicht** kopiert; der Spieler nimmt nur einen Zeiger.
    pub fn auf_schluesselbild(&mut self, pakete: &[(u64, bool, Vec<u8>)], ziel_ns: u64) {
        // Das letzte Schluesselbild, das nicht nach dem Ziel liegt.
        let mut treffer: Option<usize> = None;
        for (i, (zeit, schluessel, _)) in pakete.iter().enumerate() {
            if *schluessel && *zeit <= ziel_ns {
                treffer = Some(i);
            }
            if *zeit > ziel_ns {
                break;
            }
        }
        let start = match treffer {
            Some(i) => i,
            // Kein Schluesselbild vor dem Ziel: es gibt kein Bild fuer diese
            // Zeit. Das ist kein Fehler, sondern ein Warten — und der
            // Aufrufer soll das wissen, damit er die Leiste nicht einfriert.
            None => {
                self.laeuft = false;
                self.ereignisse.push_back(Ereignis::Warten);
                return;
            }
        };
        self.gerechnet.clear();
        self.strom = FrameDecoder::new();
        self.ohne_referenz = 0;
        self.laeuft = true;
        for (zeit, schluessel, daten) in pakete.iter().skip(start) {
            if *zeit > ziel_ns {
                break;
            }
            self.paket_ein(self.videospur, *zeit, *schluessel, daten);
            self.gezeigt_ns = Some(*zeit);
        }
    }

    /// **DER TAKT.** Setzt den Spieler auf die Zeit `jetzt_ns` und
    /// berechnet, was zu dieser Zeit gehoert.
    ///
    /// GEMESSENE GRUNDLAGE fuer die Reihenfolge: erst die Frage „welches
    /// Bild gehoert zu dieser Zeit", dann das Rechnen. Wer andersherum
    /// rechnet, holt Bilder, die niemandem gezeigt werden, und traegt
    /// Rechenzeit in die Leiste ein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `jetzt` ein Argument ist: dieselbe
    /// Zeit muss dasselbe Bild liefern, sonst ist der Spieler nicht
    /// testbar. Siehe den Kopfkommentar.
    pub fn takt(&mut self, pakete: &[(u64, bool, Vec<u8>)], mut jetzt_ns: u64) -> TaktErgebnis {
        if jetzt_ns > self.dauer_ns {
            jetzt_ns = self.dauer_ns;
        }
        self.zeit_ns = jetzt_ns;

        //
        // VIDEO, 17.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier stand
        // `if self.pausiert { return TaktErgebnis::Unveraendert; }`.
        //
        // GEMESSENE FOLGE, und sie faellt bei Punkt 1 der Leitung auf:
        // `examples/video_serie` rief `takt` bei 0,0 s mit einem
        // **pausierten** Spieler auf und bekam **kein Bild** —
        // „bei 0.0 s kam kein Bild". Ein `<video>` mit `preload` zeigt im
        // Browser aber sein **erstes Bild**, auch wenn es pausiert ist;
        // das ist der Poster-Ersatz. Ohne dieses Bild hat ein pausiertes
        // Video einen leeren Kasten, und die Seite wirkt kaputt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nur einmal dekodiert wird und nicht
        // bei jedem Takt: ein Takt mit derselben Zeit soll dasselbe Bild
        // liefern (das ist der Grund fuer die mitgegebene Zeit). Also
        // laeuft die Rechnung, wenn noch **kein** Bild fuer diese Zeit da
        // ist — danach ist der Spieler unveraendert, wie vorher.
        if self.pausiert {
            if self.gezeigt_ns.is_none() && !self.laeuft {
                self.laeuft = true;
            }
            if !self.laeuft {
                return TaktErgebnis::Unveraendert;
            }
        }

        // Das Bild, das zu dieser Zeit gehoert: das letzte Paket mit
        // Zeitstempel <= jetzt.
        let mut ziel: Option<(u64, bool, &Vec<u8>)> = None;
        for eintrag in pakete.iter() {
            if eintrag.0 <= jetzt_ns {
                ziel = Some((eintrag.0, eintrag.1, &eintrag.2));
            } else {
                break;
            }
        }
        let (ziel_ns, ziel_schluessel, ziel_daten) = match ziel {
            Some(z) => z,
            None => {
                self.ereignisse.push_back(Ereignis::Warten);
                return TaktErgebnis::AufSchluesselbildWarten;
            }
        };

        if self.gezeigt_ns == Some(ziel_ns) && !self.laeuft {
            return TaktErgebnis::Unveraendert;
        }

        // Ist das Zielbild schon gerechnet? Dann ist nichts zu tun.
        if let Some((t, _)) = self.gerechnet.back() {
            if *t == ziel_ns {
                self.laeuft = false;
                self.ereignisse.push_back(Ereignis::TimeUpdate);
                return TaktErgebnis::Unveraendert;
            }
        }

        // Muss vom naechsten Schluesselbild vor der Zielzeit neu gerechnet
        // werden? Dann von dort.
        if self.laeuft || self.gezeigt_ns.is_none() || self.gezeigt_ns > Some(ziel_ns) {
            self.auf_schluesselbild(pakete, ziel_ns);
            self.laeuft = false;
            self.ereignisse.push_back(Ereignis::TimeUpdate);
            return TaktErgebnis::NeuesBild;
        }

        // Normalfall: das naechste Bild in der Kette rechnen.
        //
        // 2026-10-15: the packets a slow clock skipped are decoded too,
        // from the last key frame among them: inter frames predict from
        // their predecessor, and a skipped one leaves a wrong reference
        // behind (VP9 with 30 fps ticks on a 60 fps stream).
        let g = self.gezeigt_ns.unwrap_or(0);
        let between: Vec<usize> = (0..pakete.len()).filter(|&i| pakete[i].0 > g && pakete[i].0 < ziel_ns).collect();
        let from = between.iter().rposition(|&i| pakete[i].1).unwrap_or(0);
        if ziel_schluessel {
            self.strom = FrameDecoder::new();
        } else {
            for &i in &between[from..] {
                let (zeit, schluessel, daten) = &pakete[i];
                self.paket_ein(self.videospur, *zeit, *schluessel, daten);
            }
        }
        if self.paket_ein(self.videospur, ziel_ns, ziel_schluessel, ziel_daten) {
            self.gezeigt_ns = Some(ziel_ns);
            self.laeuft = false;
            self.ereignisse.push_back(Ereignis::TimeUpdate);
            TaktErgebnis::NeuesBild
        } else {
            self.laeuft = false;
            self.ereignisse.push_back(Ereignis::Warten);
            TaktErgebnis::AufSchluesselbildWarten
        }
    }

    /// Setzt den Spieler ans Ende und loest `ended` aus.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Schleife hier und nicht im
    /// Aufrufer: `ended` feuert **vor** dem Zuruecksetzen, und das ist die
    /// Reihenfolge, in der eine Seite es beobachtet. Wer die Schleife
    /// ausserhalb hat, muss das nachbauen und vergisst es.
    /// Wann das **naechste** Bild an der Reihe ist — die Weckzeit.
    ///
    /// VIDEO, 04.10.2026 (VIDEO_18, Punkt 2). GEMESSENE ENTSCHEIDUNG,
    /// warum das ueberhaupt eine eigene Frage ist: der Aufruf von `takt`
    /// ist der, der ein Bild macht. Wer ihn 60 Mal pro Sekunde ruft,
    /// zeichnet 60 Mal pro Sekunde ein Bild, das 5 von 60 Mal neu ist —
    /// das ist **Dauerzeichnen**, und es kostet Rechenzeit fuer nichts.
    /// Der Weg dagegen ist: **bis zu dieser Zeit ist nichts zu tun.** Der
    /// Aufrufer schlaeft bis dahin und zeichnet einmal.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `gezeigt_ns` und nicht `zeit_ns` das
    ///_mass_ ist: massgeblich ist, welches Bild **gezeigt** ist. Die
    /// Abspielzeit kann durch einen Sprung weit vor dem gezeigten Bild
    /// liegen (dasBild bei 1,7 s setzt das bei 0,0 s voraus) — dann ist
    /// die Weckzeit trotzdem die des gezeigten Bildes, denn bis dahin
    /// aendert sich am Bild nichts.
    ///
    /// `None` heisst: **es gibt nichts zu wecken.** Der Aufrufer soll
    /// dann auf ein Ereignis warten, nicht auf eine Zeit.
    ///
    /// | Zustand | Weckzeit | Grund |
    /// |---|---|---|
    /// | pausiert | `None` | ein stehendes Bild braucht keinen Takt |
    /// | am Ende, ohne Schleife | `None` | es gibt kein weiteres Bild |
    /// | am Ende, mit Schleife | Zeit bis 0 | die Schleife beginnt |
    /// | laeuft, Bild `g` gezeigt | Zeitstempel des ersten Pakets danach | das Bild ist fertig |
    pub fn weckzeit_ns(&self, pakete: &[(u64, bool, Vec<u8>)]) -> Option<u64> {
        if self.pausiert {
            return None;
        }
        let g = self.gezeigt_ns.unwrap_or(0);
        // Das erste Paket, dessen Zeit **nach** dem gezeigten liegt.
        if let Some((t, _, _)) = pakete.iter().find(|(t, _, _)| *t > g) {
            return Some(*t);
        }
        // Kein weiteres Bild: dann ist Schluss — ausser die Schleife laeuft.
        if self.schleife {
            return Some(0);
        }
        None
    }

    /// Wie lange der Aufrufer schlafen darf, ohne ein Bild zu verpassen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `0` bei `None`: es gibt nichts zu
    /// verpassen, also „sofort wieder nachsehen" waere die falsche
    /// Antwort — es waere ein Wecken im Sekundentakt fuer ein stehendes
    /// Bild. `None` sagt dem Aufrufer: warte auf ein Ereignis.
    pub fn wartezeit_ns(&self, pakete: &[(u64, bool, Vec<u8>)], jetzt_ns: u64) -> Option<u64> {
        self.weckzeit_ns(pakete)
            .map(|w| w.saturating_sub(jetzt_ns))
    }

    pub fn ans_ende(&mut self) {
        if self.ende {
            return;
        }
        self.ende = true;
        self.pausiert = true;
        self.ereignisse.push_back(Ereignis::Ended);
        if self.schleife {
            self.auf_schluesselbild_und_start();
        }
    }

    fn auf_schluesselbild_und_start(&mut self) {
        self.zeit_ns = 0;
        self.gerechnet.clear();
        self.strom = FrameDecoder::new();
        self.ende = false;
        self.pausiert = false;
        self.laeuft = true;
        self.ereignisse.push_back(Ereignis::Play);
    }

    /// Setzt `LoadedMetadata` und `LoadedData` — der Zustand, in dem ein
    /// `<video>` mit `preload="metadata"` steht: Dauer bekannt, Bild da,
    /// aber nicht laufend.
    pub fn geladen(&mut self, dauer_ns: u64) {
        self.dauer_ns = dauer_ns;
        self.ereignisse.push_back(Ereignis::LoadStart);
        self.ereignisse.push_back(Ereignis::LoadedMetadata);
        self.ereignisse.push_back(Ereignis::LoadedData);
    }
}

impl Default for Abspielspieler {
    fn default() -> Self {
        Self::neu(1, 0)
    }
}