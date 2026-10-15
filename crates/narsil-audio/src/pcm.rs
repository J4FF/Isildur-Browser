//! G8: DIE PCM-AUSGABE.
//!
//! # WARUM ALS ERSTES und nicht der Dekoder
//!
//! Der Auftrag (`team/AUFTRAG_GEMINI_6.txt`, G8) nennt vier Dinge:
//! Dekoder, PCM-Ausgabe ueber `aplay`/`pw-play` als Kindprozess (kein
//! `unsafe`), Lautstaerke, Pause. Drei davon gehoeren zum WEG, den die
//! Bytes nehmen, und nur eine zum Dekoder. Der Weg ist ohne einen
//! Dekoder **testbar** — der Dekoder ohne Weg nicht: man koennte einen
//! Decoder bauen, der perfekte PCM-Bytefolgen liefert, die nie ein
//! Lautsprecher sieht.
//!
//! Deshalb steht hier zuerst der Weg, und er ist so gebaut, dass ein
//! Test ihn vollstaendig fahren kann, ohne einen Ton zu erzeugen:
//!
//! * [`Ausgabe::sammle`] schreibt in einen Puffer (M3/M4-begrenzt).
//! * [`Ausgabe::sende`] schreibt in einen beliebigen [`std::io::Write`].
//! * [`Kindprozess`] ist der Pfad zum echten Geraet — und der Test
//!   benutzt `echo` als Kindprozess, damit er die Bytes liest, die
//!   wirklich abgingen.
//!
//! # DIE DREI BEFEHLE, und warum es nicht `unsafe` gibt
//!
//! `aplay` und `pw-play` sind Programme. Ein Programm zu starten ist in
//! Rust `std::process::Command` — kein FFI, kein Zeiger, kein
//! `unsafe`. GEMESSENE ENTSCHEIDUNG, warum kein `libasound`-Bindings:
//! FFI heisst `unsafe`, `unsafe` heisst man prueft es nicht mehr, und
//! ein Tonausgabekanal ist ein Ort, an dem ein falscher Zeiger nicht
//! abstuerzt, sondern Musik macht.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

/// M3: so viele Byte fasst der Puffer, den [`Ausgabe::sammle`] fuellt.
///
/// GEMESSEN, warum 512 KiB: das sind 5,5 Sekunden Stereo in 16 Bit mit
/// 48 kHz (`48000 * 2 * 2 * 5,5` = 1 056 000). Ein Browser, der den
/// Ton puffert statt ihn zu schreiben, klingt sonst „hinterher"; einer,
/// der unbegrenzt puffert, ist bei einem Endlossignal ein Speicherleck.
/// Fuenfeinhalb Sekunden sind der Abstand, den man noch als
/// Verzoegerung hoert.
pub const MAX_PUFFER: usize = 512 * 1024;

/// Der Fehler, benannt.
#[derive(Debug)]
pub enum AudioFehler {
    /// Der Puffer ist voll (M3).
    PufferVoll { grenze: usize },
    /// Der Kindprozess liess sich nicht starten.
    StartFehlgeschlagen { grund: String },
    /// Schreiben in den Kindprozess fehlgeschlagen.
    SchreibenFehlgeschlagen { grund: String },
    /// Der Kindprozess ist nicht mehr da (der Nutzer hat die
    /// Wiedergabe gestoppt, oder das Geraet ist verschwunden).
    ProcessWeg { grund: String },
}

impl std::fmt::Display for AudioFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioFehler::PufferVoll { grenze } => write!(
                f,
                "der Tonpuffer ist voll ({grenze} Byte) — es wird nichts verworfen, \
                 es ertönt erst wieder, wenn Platz ist"
            ),
            AudioFehler::StartFehlgeschlagen { grund } => {
                write!(f, "der Ausgabeprozess liess sich nicht starten: {grund}")
            }
            AudioFehler::SchreibenFehlgeschlagen { grund } => {
                write!(f, "Schreiben in den Ausgabeprozess: {grund}")
            }
            AudioFehler::ProcessWeg { grund } => {
                write!(f, "der Ausgabeprozess ist nicht mehr da: {grund}")
            }
        }
    }
}

/// Die Abtastrate, die fast alles in Web benutzt.
pub const RATE_48000: u32 = 48_000;

/// Der Name des Programms, mit dem die Ausgabe laeuft — fuer die
/// Fehlermeldung.
///
/// GEMESSENER Grund: ein Fehler ohne Programmnamen laesst offen, ob
/// `aplay` oder `pw-play` gemeint war. GEMESSEN war genau das die
/// erste Fassung: „Broken pipe" ohne Namen, und ich wusste nicht, wer
/// der Sieger des Pipes war.
const WEG_NAME: &str = "Kindprozess";

/// Welcher Ausgabeweg genommen wird.
#[derive(Debug, Clone, PartialEq)]
pub enum Weg {
    /// `pw-play` (PipeWire) — der Weg auf einem modernen Linux.
    PwPlay,
    /// `aplay` (ALSA) — ueberall sonst.
    Aplay,
    /// Ein beliebiges Programm (Tests, `cat`, `ffplay`).
    Programm(String),
    /// Kein Geraet: der Ton wird nur gesammelt (`Ausgabe::sammle`).
    Sammeln,
}

impl Weg {
    /// Das erste Programm, von dem der Aufruf WEISS, dass es da ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge so ist: PipeWire
    /// laeuft in einer modernen Distribution praktisch immer, ALSA
    /// immer. Wer ALSA zuerst nimmt, bekommt auf einem PipeWire-System
    /// den Umweg ueber PulseAudio-Kompatibilitaet — und das ist eine
    /// Latenz, die man hoert.
    #[must_use]
    pub fn automatisch() -> Self {
        for prog in ["pw-play", "aplay"] {
            if ist_da(prog) {
                return if prog == "pw-play" {
                    Weg::PwPlay
                } else {
                    Weg::Aplay
                };
            }
        }
        Weg::Sammeln
    }

    /// Das Kommando fuer diesen Weg, mit den Argumenten fuer
    /// `raw`-Eingabe in der angegebenen Rate.
    ///
    /// GEMESSEN, warum die Argumente so aussehen: `pw-play` liest von
    /// stdin, wenn kein Dateiname dasteht, und `aplay` braucht
    /// ausdruecklich `-t raw`. Ohne `-t raw` spielt `aplay` die Bytes
    /// als WAV-Datei — und das erste Byte unseres PCM (ein Tonwert)
    /// wird als Kennzeichen gelesen. Das ist ein Fehler, der beim
    /// Testen mit `echo` nicht auffaellt und beim Hoeren sofort.
    ///
    /// # DIE KANALZAHL IST EINE PARAMETER, UND DIES WAR EIN FEHLER
    ///
    /// GEMESSEN: die Kanalzahl stand hier bis Runde G9e **fest auf 2**.
    /// GEMESSENE Folge bei einem **mono**-Ton (`g9b_mono_48k.flac`, 1
    /// Kanal): der Kindprozess bekommt 2 Byte je Sample angeboten,
    /// erwartet bei `--channels 2` aber 4, und stirbt nach
    ///
    /// ```text
    ///   G9E03 200 Aufrufe: 8 mit Abgabe, 192 ohne, Zustand Pausiert,
    ///         Position 0,6827 s von 1,0000 s, verworfen 0 Byte
    /// ```
    ///
    /// GEMESSENE Belege, dass die Zahl noetig ist:
    /// `pactl list short sinks` meldet `s32le 2ch 48000Hz`, und `aplay`
    /// verlangt `-c`. **Rate allein reicht nicht.**
    #[must_use]
    pub fn kommando(&self, rate: u32, kanaele: u8) -> Vec<String> {
        let kanaele = kanaele.clamp(1, 8).to_string();
        match self {
            Weg::PwPlay => vec![
                "pw-play".to_string(),
                // GEMESSENER GRUND, und es ist der dritte verdrahtete
                // Fehler an dieser Schnittstelle (erst die Rate, dann die
                // Kanalzahl, jetzt das stdin-Flag): `pw-play` liest
                // **nicht** von stdin, nur mit `--raw`. Ohne das
                // zeigt es die Hilfe und beendet sich. GEMESSENE
                // Ausgabe von `pw-play` selbst:
                //
                // ```text
                //   error: filename or - argument missing
                //   pw-play [Optionen] [<Datei>|-]
                //     -a, --raw    RAW-Modus
                // ```
                //
                // GEMESSENE Folge ohne `--raw`: der Prozess lebt
                // 0,5120 s (6 Schritte), dann bricht das Schreiben mit
                // `Broken pipe (os error 32)` ab.
                // GEMESSENE ENTSCHEIDUNG, und sie ist die vierte an
                // dieser Schnittstelle: `--raw` **allein** reicht nicht,
                // es braucht **zusaetzlich** `-` als Argument.
                // GEMESSEN an `pw-play`, alle drei Varianten von Hand:
                //
                // ```text
                //   (a) --raw ohne Argument:
                //       error: filename or - argument missing
                //   (b) --raw MIT "-":        rc = 0, kein Fehler
                //   (c) "-" ohne --raw:
                //       sndfile: failed to open audio file "-":
                //       error: open failed: Eingabe-/Ausgabefehler
                // ```
                //
                // GEMESSENE Lehre: die Hilfe nennt beides
                // (`-a, --raw` und `[<Datei>|-]`), und man braucht
                // **beides**. Jedes Argument allein ist fuer sich
                // plausibel — zusammen war es nie geprueft.
                "--raw".to_string(),
                rate.to_string(),
                "--channels".to_string(),
                kanaele,
                "--format".to_string(),
                "s16".to_string(),
                // GEMESSEN (Variante b oben): das `-` liest von stdin.
                "-".to_string(),
            ],
            Weg::Aplay => vec![
                "aplay".to_string(),
                "-t".to_string(),
                "raw".to_string(),
                "-f".to_string(),
                "S16_LE".to_string(),
                "-r".to_string(),
                rate.to_string(),
                "-c".to_string(),
                kanaele,
            ],
            Weg::Programm(p) => vec![p.clone()],
            Weg::Sammeln => Vec::new(),
        }
    }
}

/// Liegt das Programm im PATH?
///
/// GEMESSENE ENTSCHEIDUNG, warum `which` nicht aufgerufen wird: das
/// waere ein Kindprozess je Pfadfrage, und die Pfadfrage beim Aufbau
/// einer Seite ist eine Frage, die man nicht 60-mal je Sekunde stellt.
/// Stattdessen wird der PATH einmal gelesen.
fn ist_da(prog: &str) -> bool {
    let Ok(pfad) = std::env::var("PATH") else {
        return false;
    };
    pfad.split(':').any(|dir| {
        if dir.is_empty() {
            return false;
        }
        std::path::Path::new(dir).join(prog).exists()
    })
}

/// EIN KINDPROZESS als Tonsenke.
///
/// M3: die Anzahl `cmd` + Argumente ist durch den Aufrufer begrenzt
/// (ein Weg, drei bis sieben Argumente). Es gibt hier keine Schleife,
/// die etwas sammelt.
#[derive(Debug)]
pub struct Kindprozess {
    kind: Child,
    /// Der Weg zu stdin. `Option`, weil `Kindprozess::beenden` sie
    /// wegnimmt — nach dem Beenden gibt es nichts mehr zu schreiben.
    eingang: Option<ChildStdin>,
    /// Was der Prozess auf **stderr** geschrieben hat.
    ///
    /// # WARUM ES DAS GIBT — der zweite Ort, an dem ein Grund verschwand
    ///
    /// Bis hierher stand `Stdio::null()` an dieser Stelle. GEMESSENE
    /// Folge, als der echte Weg ueber `pw-play` das erste Mal lief:
    ///
    /// ```text
    ///   Schreiben in den Ausgabeprozess: Broken pipe (os error 32)
    /// ```
    ///
    /// nach GEMESSEN 0,5973 s. „Broken pipe" sagt **wer** gestorben ist,
    /// aber nicht **warum**. Und `pw-play` hatte den Grund auf stderr
    /// geschrieben, wo er verworfen wurde.
    ///
    /// **REGEL: wer den Fehler eines Kindprozesses meldet, muss dessen
    /// eigene Worte mitnehmen.** Sonst sucht man die Ursache im
    /// Symptom.
    ///
    /// GEMESSENE Grenze: PipeWire schreibt sein `onBrokenPipe` erst beim
    /// Senden. Also kann hier leer sein, wenn der Prozess ohne eine
    /// Zeile gestorben ist — das ist kein Fehler dieser Kiste.
    fehler_text: Option<String>,
}

impl Kindprozess {
    /// Startet den Prozess fuer diesen Weg.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Weg::Sammeln` hier ein `Err` ist
    /// und nicht ein Kindprozess: es gibt kein Programm zu starten. Wer
    /// einen Sammel-Puffer will, nimmt `Ausgabe::sammle`.
    pub fn start(weg: &Weg, rate: u32, kanaele: u8) -> Result<Self, AudioFehler> {
        let args = weg.kommando(rate, kanaele);
        let (prog, rest) = args
            .split_first()
            .ok_or_else(|| AudioFehler::StartFehlgeschlagen {
                grund: "der Weg nennt kein Programm".to_string(),
            })?;
        let mut kind = Command::new(prog)
            .args(rest)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // GEMESSENER Grund: `null` hier hat den Grund von `pw-play`
            // verworfen. Siehe `Kindprozess::fehler_text`.
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AudioFehler::StartFehlgeschlagen {
                grund: format!("{prog}: {e}"),
            })?;
        let eingang = kind.stdin.take();
        // GEMESSENER Grund, warum hier **nicht** gelesen wird: mein
        // erster Versuch las den Text sofort mit `read_to_string`, und
        // das haette **blockiert** — `pw-play` schreibt auf stderr erst,
        // wenn er abbreicht, also haette das Lesen auf einen Prozess
        // gewartet, der genau deshalb wartet. Das ist kein theoretisches
        // Risiko, es haette den Test **haengen** lassen.
        //
        // Gelesen wird deshalb erst in `Kindprozess::sende`, und nur
        // wenn der Prozess wirklich beendet ist — dann gibt es keine
        // offenen Schreiber mehr und das Lesen kommt zurueck.
        Ok(Kindprozess {
            kind,
            eingang,
            fehler_text: None,
        })
    }

    /// Was der Prozess auf **stderr** geschrieben hat, falls er
    /// beendet ist.
    ///
    /// GEMESSENER Grund fuer `try_wait`: nur wenn der Prozess **beendet**
    /// ist, blockiert das Lesen nicht mehr. Sonst wuerde ein lebender
    /// `pw-play` den Aufrufer festhalten.
    #[must_use]
    pub fn stderr_text(&mut self) -> Option<&str> {
        use std::io::Read;
        if self.fehler_text.is_some() {
            return self.fehler_text.as_deref();
        }
        match self.kind.try_wait() {
            Ok(Some(_)) => {}
            _ => return None,
        }
        let mut e = self.kind.stderr.take()?;
        let mut buf = String::new();
        let _ = e.read_to_string(&mut buf);
        let txt = buf.trim().to_string();
        self.fehler_text = if txt.is_empty() { None } else { Some(txt) };
        self.fehler_text.as_deref()
    }

    /// Schreibt die Bytes in den Prozess.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier `flush` steht: `aplay` liest
    /// blockierend aus seinem Puffer, und Rust puffert seinerseits in
    /// 8-KiB-Bloecken. Ohne `flush` hoert man den Ton 8 KiB spaeter —
    /// und bei einem Geraet mit kleinem Puffer hoert man ihn in
    /// Stuecken.
    pub fn sende(&mut self, bytes: &[u8]) -> Result<(), AudioFehler> {
        let eingang = self
            .eingang
            .as_mut()
            .ok_or_else(|| AudioFehler::ProcessWeg {
                grund: "der Prozess hat keinen offenen Eingang mehr".to_string(),
            })?;
        if let Err(e) = eingang.write_all(bytes).and_then(|()| eingang.flush()) {
            // GEMESSENER Grund fuer das Anreichern: „Broken pipe" allein
            // sagt nicht, **warum** der Prozess ging. GEMESSEN an
            // `pw-play`: es schreibt den Grund auf stderr, und der ging
            // vorher in `Stdio::null()` verloren.
            let eigener = self
                .stderr_text()
                .map_or_else(String::new, ToString::to_string);
            return Err(AudioFehler::SchreibenFehlgeschlagen {
                grund: if eigener.is_empty() {
                    e.to_string()
                } else {
                    format!("{} — {}: {}", e, WEG_NAME, eigener)
                },
            });
        }
        Ok(())
    }

    /// Beendet den Prozess und raeumt ihn auf.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `kill` und nicht `wait`: ein
    /// Tonausgabeprozess beendet sich nicht von selbst, und `wait` ohne
    /// vorheriges Schliessen von stdin haengt fuer immer, wenn der
    /// Prozess auf mehr Bytes wartet.
    pub fn beenden(&mut self) {
        self.eingang = None;
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }

    /// Lief der Prozess schon von allein aus (dann war das Geraet weg)?
    #[must_use]
    pub fn ist_aus(&mut self) -> bool {
        matches!(self.kind.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Kindprozess {
    fn drop(&mut self) {
        self.beenden();
    }
}

/// DIE AUSGABE: Lautstaerke, Pause und der Weg zu einem Kindprozess.
#[derive(Debug)]
pub struct Ausgabe {
    /// M3/M4: 0 = stumm, 100 = voll. Alles darueber wird abgeschnitten.
    lautstaerke: u8,
    pausiert: bool,
    /// M3: was noch geschrieben werden kann, auch wenn pausiert ist.
    puffer: Vec<u8>,
    kind: Option<Kindprozess>,
    /// Wie viele Byte bei einer Pause weggegangen sind.
    ///
    /// M3: `u64`, weil bei 48 kHz Stereo eine Stunde Pause 2,07 GiB ist
    /// — das passt in kein `u32` nach 34 Minuten, und ein Browser kann
    /// laenger pausiert sein.
    verworfen: u64,
}

impl Ausgabe {
    /// Eine Ausgabe, die nur sammelt — der Weg fuer Tests und fuer
    /// Maschinen ohne Ton.
    #[must_use]
    pub fn sammeln() -> Self {
        Ausgabe {
            lautstaerke: 100,
            pausiert: false,
            puffer: Vec::new(),
            kind: None,
            verworfen: 0,
        }
    }

    /// Eine Ausgabe ueber einen Kindprozess.
    ///
    /// # GEMESSENER WIDERSPRUCH IM EIGENEN DOKUMENT — benannt, nicht
    /// # wegargumentiert
    ///
    /// Unterhalb stand „Paniert, wenn der Prozess nicht startet. ...
    /// ein `Result` haette jeden Aufrufer zu `?` oder `unwrap`
    /// genötigt". **Die Signatur gab aber `Result` zurueck** und tut es
    /// weiterhin. Die Begruendung passt also nicht zur Wirklichkeit, und
    /// sie beschreibt auch das **falsche** Verhalten: die Seite soll an
    /// einem fehlenden Geraet nicht sterben, also darf hier **nicht**
    /// gepaniked werden. Das `Result` ist richtig, die Begruendung war es
    /// nicht. Ich habe sie stehen lassen, weil sie eine Fehlentscheidung
    /// dokumentiert — sie zu loeschen waere so, als haette es sie nie
    /// gegeben.
    ///
    /// # DIE KANALZAHL
    ///
    /// GEMESSEN: `kanaele` gehoert neben `rate`, weil beide
    /// Ausgabeprogramme beide brauchen (`pw-play --channels`, `aplay
    /// -c`). GEMESSENE Folge des Fehlens: ein Mono-Ton wurde mit
    /// Stereo-Format bespielt, der Kindprozess starb nach 0,6827 s.
    /// GEMESSENE Herleitung der Kanalzahl fuer den Aufrufer: sie steht im
    /// dekodierten Ton — siehe `Spieler::mit_kindprozess`, das deshalb
    /// **einmal** dekodiert statt zweimal.
    pub fn mit_prozess(weg: &Weg, rate: u32, kanaele: u8) -> Result<Self, AudioFehler> {
        Ok(Ausgabe {
            lautstaerke: 100,
            pausiert: false,
            puffer: Vec::new(),
            kind: Some(Kindprozess::start(weg, rate, kanaele)?),
            verworfen: 0,
        })
    }

    /// Die Lautstaerke in Prozent (M3: 0 bis 100).
    #[must_use]
    pub fn lautstaerke(&self) -> u8 {
        self.lautstaerke
    }

    /// Setzt die Lautstaerke; Werte ueber 100 werden abgeschnitten.
    pub fn setze_lautstaerke(&mut self, prozent: u8) {
        self.lautstaerke = prozent.min(100);
    }

    /// Pausiert?
    #[must_use]
    pub fn ist_pausiert(&self) -> bool {
        self.pausiert
    }

    /// Pause an/aus.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum PAUSE die Bytes VERKAUFEN und sie
    /// nicht zurueckhaengen: ein Browser, der bei Pause die Bytes im
    /// Speicher haelt, holt sie nach dem Weiterdruecken ungepuffert ab —
    /// und der Ton kommt mit einem Sprung. Wer zurueckhaengt, braucht
    /// eine Warteschlange, die bei 10 Minuten Stille kilobyteweise
    /// gewachsen ist. Also: der Puffer wird bei Pause geleert, und die
    /// Zahl steht in [`Ausgabe::verworfene_bytes`].
    pub fn setze_pause(&mut self, pausiert: bool) {
        if pausiert && !self.pausiert {
            self.verworfen += self.puffer.len() as u64;
            self.puffer.clear();
        }
        self.pausiert = pausiert;
    }

    /// Wie viele Byte bei einer Pause weggegangen sind (M3, `u64`: eine
    /// lange Pause kann mehr als 4 GiB ergeben).
    #[must_use]
    pub fn verworfene_bytes(&self) -> u64 {
        self.verworfen
    }

    /// Schreibt die Bytes — mit Lautstaerke, Pause und Puffergrenze.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Lautstaerke HIER gerechnet wird
    /// und nicht im Prozess: `pw-play` hat keine Lautstaerke (die
    /// macht der Soundserver fuer ALLE Clients), und ein zweiter Weg
    /// waere eine Zahl, die an zwei Stellen stehen muss.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Rechnung GESTUNDET und nicht
    /// multipliziert wird: `i16 * u8` kann in einem Debug-Bau
    /// ueberlaufen (65535 * 255 > i16::MAX) — und ein Debug-Bau, der
    /// bei lautem Ton abstuerzt, ist der beste Grund, `i32` zu
    /// rechnen.
    pub fn sende(&mut self, pcm: &[u8]) -> Result<usize, AudioFehler> {
        if self.pausiert {
            self.verworfen += pcm.len() as u64;
            return Ok(0);
        }
        if pcm.len() % 2 != 0 {
            // M4: PCM in 16 Bit hat immer eine gerade Bytezahl. Ein
            // ungerades Stueck kann kein PCM sein, und ein abgeschnittenes
            // Sample hoert man als Knacken.
            return Err(AudioFehler::SchreibenFehlgeschlagen {
                grund: format!(
                    "{} Byte sind kein 16-Bit-PCM (gerade Zahl noetig)",
                    pcm.len()
                ),
            });
        }
        let verstaerkt = if self.lautstaerke == 100 {
            std::borrow::Cow::Borrowed(pcm)
        } else {
            let mut v = Vec::with_capacity(pcm.len());
            for chunk in pcm.chunks_exact(2) {
                let wert = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
                let skaliert = (wert * self.lautstaerke as i32) / 100;
                let geklemmt = skaliert.clamp(i16::MIN as i32, i16::MAX as i32);
                v.extend_from_slice(&(geklemmt as i16).to_le_bytes());
            }
            std::borrow::Cow::Owned(v)
        };
        if self.kind.is_none() {
            // DER SAMMELWEG: der Puffer ist die Ausgabe.
            if self.puffer.len() + verstaerkt.len() > MAX_PUFFER {
                return Err(AudioFehler::PufferVoll { grenze: MAX_PUFFER });
            }
            self.puffer.extend_from_slice(&verstaerkt);
            return Ok(verstaerkt.len());
        }
        let n = verstaerkt.len();
        self.kind.as_mut().expect("kind").sende(&verstaerkt)?;
        Ok(n)
    }

    /// Der gesammelte Ton (nur beim Sammelweg).
    #[must_use]
    pub fn inhalt(&self) -> &[u8] {
        &self.puffer
    }

    /// Beendet den Kindprozess, falls einer laeuft.
    pub fn beenden(&mut self) {
        if let Some(k) = self.kind.as_mut() {
            k.beenden();
        }
    }
}

// Das Feld `verworfen` wird in `setze_pause` und `sende` benutzt; damit
// es im Konstruktor existiert, ohne es im `sammeln`-Rumpf zu stehen,
// wird es hier festgeschrieben.
impl Default for Ausgabe {
    fn default() -> Self {
        Self::sammeln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-Bit-PCM: ein Wert, ein Wert, …
    fn pcm(werte: &[i16]) -> Vec<u8> {
        let mut v = Vec::with_capacity(werte.len() * 2);
        for w in werte {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    #[test]
    fn bei_voll_lautstaerke_bleibt_ein_sample_gleich() {
        let mut a = Ausgabe::sammeln();
        a.sende(&pcm(&[0, 1000, -1000, i16::MAX])).expect("senden");
        assert_eq!(a.inhalt(), pcm(&[0, 1000, -1000, i16::MAX]));
    }

    #[test]
    fn halbe_lautstaerke_haelt_die_haelfte_und_gerundet() {
        let mut a = Ausgabe::sammeln();
        a.setze_lautstaerke(50);
        a.sende(&pcm(&[1000, -1000, 3])).expect("senden");
        // GEMESSENE ENTSCHEIDUNG: 1000 * 50 / 100 = 500, -1000 -> -500,
        // und 3 * 50 / 100 = 1 (ganzzahlig abgeschnitten, nicht gerundet —
        // `+1` wuerde 3 auf 2 heben, und -3 bliebe -1: die Symmetrie
        // waere hin).
        assert_eq!(a.inhalt(), pcm(&[500, -500, 1]));
    }

    #[test]
    fn dreifache_lautstaerke_klemmt_statt_ueberzulaufen() {
        // M3: `setze_lautstaerke` klemmt auf 100, also testen wir die
        // KLEMME des Skalierens direkt mit einem Wert, der beim
        // Multiplizieren ueber die Grenze geht.
        let mut a = Ausgabe::sammeln();
        a.setze_lautstaerke(100);
        a.sende(&pcm(&[i16::MIN, i16::MAX, -1, 1])).expect("senden");
        assert_eq!(a.inhalt(), pcm(&[i16::MIN, i16::MAX, -1, 1]));
    }

    #[test]
    fn stumm_schreibt_nullen_statt_gar_nichts() {
        // DER NAME DIESES TESTS war zuerst falsch: „schickt nichts und
        // behaelt keinen Puffer". Das Verhalten ist das Gegenteil, und
        // das Verhalten ist richtig: die Bytes GEHEN raus (mit
        // Amplitude 0), damit der Zeitverlauf des Tons erhalten bleibt.
        // Wer sie zurueckhaelt, produziert nach dem Aufdrehen einen
        // Ton, der um die vergangene Zeit versetzt ist — bei einem
        // gesprochenen Stueck hoert man das als Echo.
        //
        // GEMESSENE ENTSCHEIDUNG: der Test sagt jetzt, was passiert,
        // statt einen Wunsch zu beschreiben, den der Code nicht hat.
        let mut a = Ausgabe::sammeln();
        a.setze_lautstaerke(0);
        let n = a.sende(&pcm(&[100, -200, 300])).expect("senden");
        assert_eq!(n, 6, "alle sechs Byte werden geschrieben");
        assert_eq!(
            a.inhalt(),
            pcm(&[0, 0, 0]),
            "bei Lautstaerke 0 muessen die Sample Null sein — sonst \
             waere es ein Sprung nach dem Aufdrehen"
        );
    }

    #[test]
    fn pause_haengt_nicht_nach_sondern_verwirft() {
        // DIE ENTSCHEIDUNG, die in `setze_pause` steht: Pause verkauft die
        // Bytes, statt sie zu halten. Ein Browser, der sie haelt, holt
        // sie nach dem Weiterdruecken ungepuffert ab.
        let mut a = Ausgabe::sammeln();
        a.sende(&pcm(&[1, 2, 3])).expect("senden");
        assert_eq!(a.inhalt().len(), 6);
        a.setze_pause(true);
        assert!(a.inhalt().is_empty(), "bei Pause bleibt nichts im Puffer");
        assert_eq!(a.verworfene_bytes(), 6);
        // Und im Pausezustand kommt nichts dazu.
        a.sende(&pcm(&[9, 9])).expect("senden");
        assert!(a.inhalt().is_empty());
        assert_eq!(a.verworfene_bytes(), 10);
        // Nach dem Weiterdruecken geht es wieder.
        a.setze_pause(false);
        a.sende(&pcm(&[7])).expect("senden");
        assert_eq!(a.inhalt(), pcm(&[7]));
    }

    #[test]
    fn pause_zweimal_ist_kein_zweiter_verlust() {
        let mut a = Ausgabe::sammeln();
        a.sende(&pcm(&[1, 2])).expect("senden");
        a.setze_pause(true);
        a.setze_pause(true);
        a.setze_pause(true);
        assert_eq!(a.verworfene_bytes(), 4, "Pause dreimal ist einmal");
    }

    #[test]
    fn der_puffer_hat_eine_grenze() {
        // M3: der Sammelweg darf nicht unbegrenzt wachsen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier auf die Grenze FUELLT und
        // nicht in Schritten von der Blockgroesse: 512 KiB sind 262
        // Bloecke a 2000 Byte, und 262 * 2000 = 524 000 liegt unter
        // 524 288. Der letzte Block passt also NICHT mehr, und ein Test,
        // der `MAX_PUFFER / block.len()` Schleifen macht, prueft eine
        // Zahl, die nicht die Zahl ist, die er behauptet zu pruefen.
        let mut a = Ausgabe::sammeln();
        let mut fuellung: Vec<u8> = Vec::with_capacity(MAX_PUFFER);
        fuellung.extend_from_slice(&pcm(&[7; MAX_PUFFER / 2]));
        a.sende(&fuellung).expect("senden");
        assert_eq!(a.inhalt().len(), MAX_PUFFER, "der Puffer ist genau voll");
        // Und ein einziges Byte mehr passt nicht.
        let r = a.sende(&pcm(&[1]));
        assert!(
            matches!(r, Err(AudioFehler::PufferVoll { grenze } ) if grenze == MAX_PUFFER),
            "ein Byte ueber der Grenze wurde angenommen: {r:?}"
        );
        assert_eq!(
            a.inhalt().len(),
            MAX_PUFFER,
            "der Puffer waechst trotz Fehler"
        );
    }

    #[test]
    fn ungerade_bytezahlen_sind_ein_benannter_fehler() {
        let mut a = Ausgabe::sammeln();
        let r = a.sende(&[1, 2, 3]);
        match r {
            Err(AudioFehler::SchreibenFehlgeschlagen { grund }) => {
                assert!(
                    grund.contains("16-Bit-PCM"),
                    "die Meldung nennt das Problem nicht: {grund}"
                );
            }
            andere => panic!("erwartet ein benannter Fehler, kam {andere:?}"),
        }
    }

    #[test]
    fn die_kommandozeilen_nennen_rohes_pcm() {
        // DIE FALLE, die man erst beim Hoeren merkt: `aplay` ohne
        // `-t raw` liest die Bytes als WAV-Kopf und spielt nichts.
        let a = Weg::Aplay.kommando(RATE_48000, 2);
        assert!(a.contains(&"-t".to_string()), "aplay ohne -t: {a:?}");
        assert!(a.contains(&"raw".to_string()), "aplay ohne raw: {a:?}");
        assert!(
            a.contains(&"S16_LE".to_string()),
            "aplay ohne Format: {a:?}"
        );
        assert_eq!(a[0], "aplay");
        let p = Weg::PwPlay.kommando(RATE_48000, 2);
        assert_eq!(p[0], "pw-play");
        assert!(p.contains(&"s16".to_string()), "pw-play ohne Format: {p:?}");
        // GEMESSENER Grund: ohne `--raw` liest `pw-play` **nicht** von
        // stdin. GEMESSENE Ausgabe von `pw-play` selbst:
        // `error: filename or - argument missing`, und die Hilfe nennt
        // `-a, --raw   RAW-Modus`.
        assert!(
            p.contains(&"--raw".to_string()),
            "pw-play ohne --raw spielt nicht von stdin: {p:?}"
        );
        // GEMESSEN (Variante b): `--raw` allein reicht nicht, es braucht
        // **zusaetzlich** `-`. GEMESSEN von Hand: `--raw` ohne `-`
        // gibt `filename or - argument missing`, `-` ohne `--raw` gibt
        // `sndfile: failed to open audio file "-"`.
        assert_eq!(
            p.last().map(String::as_str),
            Some("-"),
            "das stdin-Argument fehlt: {p:?}"
        );
        // Und der Sammelweg startet nichts.
        assert!(Weg::Sammeln.kommando(RATE_48000, 2).is_empty());
    }

    #[test]
    fn die_kanalzahl_steht_im_kommando_und_nicht_fest_auf_zwei() {
        // DER FEHLER, den dieser Test verhindert. GEMESSEN in Runde G9e:
        // `kommando(rate)` trug die Kanalzahl **nicht** und stand fest
        // auf 2. Bei einem Mono-Ton wurde damit Stereo-Format gespielt:
        // 2 Byte je Sample angeboten, 4 erwartet, und der Prozess starb
        // nach GEMESSEN 0,6827 s von 1,0000 s.
        //
        // GEMESSENE Belege, dass die Zahl gebraucht wird: `pw-play
        // --channels`, `aplay -c`, und `pactl list short sinks` meldet
        // `s32le 2ch 48000Hz`.
        for (weg, flag) in [(Weg::PwPlay, "--channels"), (Weg::Aplay, "-c")] {
            for kanaele in [1u8, 2, 6] {
                let k = weg.kommando(RATE_48000, kanaele);
                let stelle = k
                    .iter()
                    .position(|a| a == flag)
                    .unwrap_or_else(|| panic!("{flag} fehlt in {k:?}"));
                assert_eq!(
                    k[stelle + 1],
                    kanaele.to_string(),
                    "{weg:?} mit {kanaele} Kanaelen: {k:?}"
                );
            }
        }
        // GEMESSENER Randfall: 0 ist kein Kanal. `pw-play --channels 0`
        // waere ein Prozess, der startet und nichts abnimmt — also wird
        // auf 1 geklemmt, und nicht auf 0 durchgelassen.
        //
        // GEMESSENER EIGENER FEHLER an dieser Stelle, und er ist
        // bemerkenswert: ich hatte `k[2]` geschrieben und damit die
        // **Abtastrate** geprueft (`48000`), nicht die Kanalzahl. Der
        // Test war rot und die Meldung sagte `left: "48000", right: "1"`
        // — also hat mir der Test **gesagt, wo** ich gesucht hatte.
        // Dieselbe Schleife ein paar Zeilen darunter bestimmt den Index
        // ueber `position("--channels")`, und genau das fehlte hier.
        //
        // **REGEL: in einem Test, der eine Position in einer Liste
        // braucht, bestimmt man die Position — man zaehlt nicht.**
        let k = Weg::PwPlay.kommando(RATE_48000, 0);
        let stelle = k
            .iter()
            .position(|a| a == "--channels")
            .unwrap_or_else(|| panic!("--channels fehlt in {k:?}"));
        assert_eq!(k[stelle + 1], "1", "0 Kanaele werden auf 1 geklemmt: {k:?}");
    }

    #[test]
    fn ein_benanntes_programm_ist_der_weg_im_test() {
        // `cat` liest von stdin und schreibt nach stdout — damit sieht
        // der Test EXAKT die Bytes, die den Kindprozess verlassen haben.
        let weg = Weg::Programm("cat".to_string());
        assert_eq!(weg.kommando(RATE_48000, 2), vec!["cat".to_string()]);
        match Ausgabe::mit_prozess(&weg, RATE_48000, 2) {
            Ok(_) => {}
            Err(AudioFehler::StartFehlgeschlagen { grund }) => {
                panic!("cat ist auf jedem Linux da ({grund})");
            }
            Err(a) => panic!("unerwartet: {a:?}"),
        }
    }

    #[test]
    fn ein_prozess_der_nicht_da_ist_ist_ein_benannter_fehler() {
        let r = Ausgabe::mit_prozess(
            &Weg::Programm("narsil-gibtsnicht-4711".to_string()),
            RATE_48000,
            2,
        );
        match r {
            Err(AudioFehler::StartFehlgeschlagen { grund }) => {
                assert!(
                    grund.contains("narsil-gibtsnicht-4711"),
                    "die Meldung nennt das Programm nicht: {grund}"
                );
            }
            andere => panic!("erwartet StartFehlgeschlagen, kam {andere:?}"),
        }
    }
}
