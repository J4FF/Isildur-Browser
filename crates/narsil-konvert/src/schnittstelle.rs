//! K11 — DIE **SCHNITTSTELLE**, die KONTEXT braucht: `konvertiere(bytes,
//! quelltyp, ziel)` und `ziele_fuer(typ)`.
//!
//! # WARUM EIGENE DATEI und nicht `pipeline.rs`
//!
//! GEMESSEN ist der Auftrag der LEITUNG (04.10.2026, `team/AKTUELLE_ANWEISUNGEN.txt`):
//!
//! > „Du lieferst NUR die Umwandlung in narsil-konvert: `pub fn konvertiere(bytes,
//! > quelltyp, ziel) -> Result<Vec<u8>>` fuer alle Ziele + `ziele_fuer(typ)`“
//!
//! Und GEMESSEN ist, dass `pipeline::konvertiere` **vier** Parameter hat
//! (`quelle_name`, `&Eingabe`, `ziel`, `Optionen`) — und dass **kein** `Quelltyp`
//! existiert (`grep "pub enum (Quell|Art|Typ)"` = 0 Treffer, nur `Eingabe` mit
//! `Bytes/Bild/Pcm/Bilder`).
//!
//! > **Ein Auftrag mit *drei* Parametern, gegen eine Funktion mit *vier*, ist kein
//! > Auftrag, sondern eine *Uebersetzung*** — und die **gehoert** in **eine eigene
//! > Datei**, weil sie **additiv** ist und **keinen** der 63 Aufrufer anfasst.
//!
//! # DIE **ZWEI** ENTSCHEIDUNGEN, und beide sind **gemessen**
//!
//! **1. `quelltyp` ist *ein* Typ mit *zwei* Feldern — nicht *ein* String.**
//!
//! GEMESSEN ist, dass `ziele::ziele_fuer(mime, endung)` **beide** braucht und
//! **beide** benutzt: die MIME entscheidet die Art, und die Endung ist der
//! **Rueckfall**, weil manche Server **keine** MIME schicken. Wer `quelltyp` als
//! **einen** String baut, muss sich entscheiden — und **beide** Entscheidungen
//! sind **falsch**:
//!
//! | `quelltyp` als | Ein Server **ohne** MIME | Eine URL **ohne** Endung |
//! |---|---|---|
//! | nur MIME | **kein** Untermenue | **nichts** |
//! | nur Endung | geht ueber Rueckfall | **kein** Untermenue |
//!
//! **2. Fuer Video und GIF ist die Zielliste **nicht** aus dem Typ allein
//! berechenbar** — und **das** ist der **scharfste** Befund dieser Datei.
//!
//! GEMESSEN: `ziele_fuer("video/x-msvideo", "avi")` liefert **genau ein** Ziel
//! (`Avi`). Und `ziele_fuer_film_daten(&bytes)` liefert **vier** (`Avi`, `Png`,
//! `Gif`, `Wav`). Also:
//!
//! > **Ein Menue, das `ziele_fuer(typ)` benutzt, bietet einem Video **eines** an,
//! > obwohl **vier** gebaut werden koennen.** Und fuer ein **animiertes** GIF
//! > entscheidet erst `daten` ob „animiertes GIF“ ueberhaupt in der Liste steht.
//!
//! Deshalb gibt es **beides**: `ziele()` fuer die **datenlose** Liste (das, was
//! KONTEXT **ohne** Bytes bekommt) und `ziele_mit_daten()` fuer die **volle**
//! (das, was KONTEXT **mit** Bytes bekommt — und **das** ist die **richtige**).

use crate::fehler::Zielformat;
use crate::pipeline::konvertiere as konvertiere_vier;
use crate::pipeline::{Eingabe, PipelineFehler};
use crate::ziele::{Optionen, ZielHinweis};

/// **Der Quelltyp** — MIME **und** Endung, weil GEMESSEN ist, dass `ziele_fuer`
/// **beide** braucht (siehe Datei-Kopf, Entscheidung 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quelltyp {
    mime: String,
    endung: String,
}

impl Quelltyp {
    /// **Baut** einen Quelltyp aus MIME und Endung — und **beide** werden
    /// **normiert**, weil GEMESSEN ist, dass `ziele_fuer` die Endung **ohne**
    /// Punkt und **kleingeschrieben** will (`Foto.JPG` ist `jpg`).
    #[must_use]
    pub fn aus(mime: &str, endung: &str) -> Self {
        Self {
            mime: mime.trim().to_ascii_lowercase(),
            endung: endung.trim().trim_start_matches('.').to_ascii_lowercase(),
        }
    }

    /// **Nur** aus der Endung, wenn keine MIME da ist (manche Server schicken
    /// keine) — GEMESSEN begruendet in `ziele_fuer`.
    #[must_use]
    pub fn aus_endung(endung: &str) -> Self {
        Self::aus("", endung)
    }

    /// **Die** MIME — leer, wenn keine geschickt wurde.
    #[must_use]
    pub fn mime(&self) -> &str {
        &self.mime
    }

    /// **Die** Endung — **ohne** Punkt, **klein**.
    #[must_use]
    pub fn endung(&self) -> &str {
        &self.endung
    }

    /// **Der** Dateiname, den `pipeline::konvertiere` fuer die Endung braucht —
    /// und GEMESSEN ist, dass `pipeline.rs:416` sie sich aus **dem Namen** holt
    /// (`quelle_name.rsplit_once('.')`). Also muss hier **ein** Punkt **drin** sein,
    /// sonst **verliert** die Bytes-Arm die Endung **stillschweigend**.
    #[must_use]
    pub fn datei_name(&self) -> String {
        format!("quelle.{}", self.endung)
    }

    /// **Die datenlose Zielliste** — das ist `ziele_fuer(typ)` **woertlich**,
    /// unverandert weitergereicht. Keine Zweitlogik, denn **zwei** Logiken fuer
    /// **eine** Liste sind **immer** eine zu viel.
    ///
    /// Und **das** ist die **Fallhoehle**: fuer Video liefert sie **ein** Ziel,
    /// `ziele_mit_daten` **vier** (siehe Datei-Kopf).
    #[must_use]
    pub fn ziele(&self) -> Vec<Zielformat> {
        crate::ziele::ziele_fuer(&self.mime, &self.endung)
    }

    /// **Die volle Zielliste** — und **hier** werden die **Daten** benutzt, weil
    /// GEMESSEN ist, dass **zwei** Faelle datenabhaengig sind:
    ///
    /// | Fall | datenlos | mit Daten |
    /// |------|----------|-----------|
    /// | Video (AVI/WebM) | **1** (`Avi`) | **4** (`Avi`,`Png`,`Gif`,`Wav`) |
    /// | GIF, **animiert** | wie ein Standbild | **eigene** Zeile „animiertes GIF“ |
    ///
    /// Und **das** ist **kein** Rechenfehler, sondern die **Wahrheit** ueber die
    /// Bytes: „nur Ton“ haengt **am Ton**, „animiertes GIF“ an der **Bildzahl**.
    #[must_use]
    pub fn ziele_mit_daten(&self, daten: &[u8]) -> Vec<ZielHinweis> {
        if self.mime.starts_with("video/") || self.endung == "avi" || self.endung == "webm" {
            return crate::ziele::ziele_fuer_film_daten(daten);
        }
        if self.endung == "gif" {
            return crate::ziele::ziele_fuer_gif_mit_hinweis(daten);
        }
        crate::ziele::ziele_mit_hinweis(&self.mime, &self.endung)
    }
}

/// **KONVERTIEREN** — die **drei** Parameter, die KONTEXT braucht.
///
/// # DIE **GEMESSENE** BEGRUENDUNG fuer die Reihenfolge
///
/// `konvertiere(daten, &quelltyp, ziel) -> Result<Vec<u8>>` — und **nicht**
/// `konvertiere(quelle_name, ...)`, weil GEMESSEN ist, dass `pipeline::konvertiere`
/// **den Dateinamen nur fuer die Endung** braucht (`pipeline.rs:416`) und
/// **sonst** nichts damit macht. Der Name ist also **eine** Angabe ueber den
/// **Quelltyp** — und gehoert in `Quelltyp`, nicht in **eigenen** Parameter.
///
/// Und GEMESSEN ist, dass `Optionen::default()` das ist, was das Untermenue
/// anbietet: `qualitaet` 85, `ton_bits` 16, `fps` 10 — und **diese** Vorgaben
/// stehen in **jedem** Hinweistext, den K9 nachgemessen hat.
///
/// # DIE **ZWEITE** REGEL: **kein** stilles Scheitern
///
/// Der Auftrag sagt „fuer **alle** Ziele“. Also ist **das** die Zusage, die
/// K5 fuer das Untermenue gemessen hat (**keine** toten Eintraege) — hier an
/// **einer** anderen Stelle: **was** `ziele_mit_daten` anbietet, muss
/// `konvertiere` auch **liefern**. Und **wenn** es das nicht tut, ist der
/// **Fehler** benannt (`PipelineFehler`) und **nicht** eine leere Datei.
pub fn konvertiere(
    daten: &[u8],
    quelltyp: &Quelltyp,
    ziel: Zielformat,
) -> Result<Vec<u8>, PipelineFehler> {
    let (eingabe, optionen) = quelltyp.als_eingabe(daten, ziel)?;
    konvertiere_vier(&quelltyp.datei_name(), &eingabe, ziel, optionen)
}

/// **Und** dieselbe Funktion fuer **einen** Quelltyp **ohne** MIME — weil
/// GEMESSEN ist, dass manche Server **keine** schicken, und **ein** Aufrufer dann
/// **sonst** `Quelltyp::aus("", "jpg")` tippen muss.
pub fn konvertiere_endung(
    daten: &[u8],
    endung: &str,
    ziel: Zielformat,
) -> Result<Vec<u8>, PipelineFehler> {
    konvertiere(daten, &Quelltyp::aus_endung(endung), ziel)
}

impl Quelltyp {
    /// **GEMESSEN** ist das der **Fehler**, den diese Datei behebt: `Eingabe::Bytes`
    /// geht durch `narsil_image::werkzeuge::lesen`, und **das** kann **keinen** AVI und
    /// **keinen** Ton lesen. GEMESSEN in `k11_schnittstelle.rs`: **6 von 12** Zusagen
    /// waren unerfuellt — Bild **6/6**, Ton **0/2**, Film **0/4**.
    ///
    /// > **Ein Aufrufer, der nur *Bytes* hat, kann keinen `Eingabe`-Typ waehlen —
    /// > und genau deshalb muss die *Erkennung* in der Schnittstelle stehen.**
    ///
    /// Und **hier** ist **auch** die Bildzahl ermittelt: bei einem Film **ohne**
    /// lesbaren Ton ist „nur Ton“ **nicht** moeglich, und GEMESSEN ist, dass der
    /// Fehler **dann** `FilmOhneTon` heissen muss — **nicht** „geht nicht“.
    pub fn als_eingabe(
        &self,
        daten: &[u8],
        ziel: Zielformat,
    ) -> Result<(Eingabe, Optionen), PipelineFehler> {
        // Und **WebM** zuerst — GEMESSEN, warum die Reihenfolge so ist: der
        // AVI-Weg `crate::video::lesen` sucht ein `movi`, und ein WebM hat
        // keins. Also wuerde er **jedes** WebM als „kein AVI-Bildleser“ ablehnen
        // — und der Nutzer laesst sich einen **Container** vorwerfen, den er
        // gar nicht gefragt hat.
        if crate::webm::ist_webm(daten) {
            return self.als_eingabe_webm(daten, ziel);
        }
        if self.ist_video() {
            let film =
                crate::video::lesen(daten).map_err(|_| PipelineFehler::QuelleNichtLesbar {
                    endung: self.endung.clone(),
                    grund: "kein AVI-Bildleser in diesem Weg",
                })?;
            let ton = film.ton.ok_or(PipelineFehler::FilmOhneTon { ziel })?;
            return Ok((
                Eingabe::Film {
                    bilder: film.bilder,
                    ton,
                },
                Optionen {
                    fps: film.fps,
                    ..Optionen::default()
                },
            ));
        }
        // Und **jetzt** der **Ton**-Zweig — und GEMESSEN ist, warum er **vor** dem
        // Bild-Zweig stehen **muss**: der Bildleser gibt bei WAV-Bytes
        // „`narsil-audio` hat keinen FLAC- und keinen MP3-Dekoder“, und **das** ist
        // **falsch** (K11 Teil 1). Also wird **zuerst** gefragt, ob es **Ton** ist.
        if self.ist_ton() {
            let pcm = self.ton_lesen(daten)?;
            return Ok((Eingabe::Pcm(pcm), Optionen::default()));
        }
        Ok((Eingabe::Bytes(daten.to_vec()), Optionen::default()))
    }

    /// Und der **WebM**-Weg: „nur Ton“ und „animiertes GIF“.
    ///
    /// # WARUM EIGENE DATEI und nicht ein Zweig im AVI-Weg
    ///
    /// GEMESSEN: `crate::video::lesen` sucht `movi`. Ein WebM hat keins. Und
    /// `videoton::dekodiere` **liest** den Ton aus dem **Container**, waehrend
    /// `video::lesen` ihn aus dem AVI-Stream liest — zwei **voellig**
    /// verschiedene Wege fuer dieselbe Frage.
    ///
    /// Also: „nur Ton“ geht als `Pcm` (der Weg, den `ton_nach_inner` kennt),
    /// und „animiertes GIF“ als `Bilder` (der Weg, den `schreiben_aus_bildern`
    /// kennt). Beides sind **bestehende** Wege — es ist **kein** neuer.
    ///
    /// Und **GEMESSEN**, was der WebM-Weg heute **nicht** kann: **Opus**.
    /// `videoton.rs` Z. 38-40 sagt „Opus / WebM — Dekoder **nicht gebaut**“.
    /// Also **nennt** der Fehler den Codec, statt eine leere Datei zu liefern.
    fn als_eingabe_webm(
        &self,
        daten: &[u8],
        ziel: Zielformat,
    ) -> Result<(Eingabe, Optionen), PipelineFehler> {
        // --- Ton-Weg ---
        // Und `Ogg` ist seit KONVERT_12 **hier** mitgemeint — GEMESSEN: vorher
        // fiel der Klick in den Bild-Zweig und endete mit „Aus einem Bild entsteht
        // kein Ogg“, also mit einer **falschen** Begruendung fuer einen Ton-Weg.
        if matches!(ziel, Zielformat::Wav | Zielformat::Flac | Zielformat::Ogg) {
            let hat = crate::webm::hat_ton(daten);
            if !hat {
                return Err(PipelineFehler::QuelleNichtLesbar {
                    endung: self.endung.clone(),
                    grund: "dieses WebM hat keine Audiospur",
                });
            }
            let pcm = crate::webm::ton(daten).map_err(|_| PipelineFehler::QuelleNichtLesbar {
                endung: self.endung.clone(),
                grund: "die Tonspur liess sich nicht dekodieren (Vorbis in WebM ist gebaut, Opus nicht)",
            })?;
            return Ok((Eingabe::NurTon(pcm), Optionen::default()));
        }
        // --- Bild-Weg: animiertes GIF und Standbild ---
        let mut bilder = crate::webm::bilder(daten, crate::grenzen::MAX_VIDEO_BILDER);
        // Und GEMESSEN (KONVERT_12): die Bildrate gehoert **hierher**, sonst
        // gewinnt die Vorgabe `10` und ein 5-fps-Film laeuft **doppelt** so
        // schnell. `bilder_pro_sekunde` rechnet aus `duration_ms` und der
        // **gemessenen** Bildzahl.
        let fps = crate::webm::bilder_pro_sekunde(daten, bilder.len());
        let optionen = Optionen {
            fps,
            ..Optionen::default()
        };
        if bilder.is_empty() {
            return Err(PipelineFehler::QuelleNichtLesbar {
                endung: self.endung.clone(),
                grund: "aus diesem WebM kam kein dekodierbares Bild (nur Keyframes sind ohne Referenz lesbar)",
            });
        }
        // Und **beide** Filmziele bekommen **alle** Bilder — GEMESSEN, warum
        // `Avi` vorher **immer** scheiterte:
        //
        // ```text
        //   GEMESSEN: Avi -> FEHLER ZielAusBildNichtMoeglich { ziel: Avi,
        //   weg: "der Film-Weg ist `Eingabe::Bilder` oder `Eingabe::Film`" }
        // ```
        //
        // Ich hatte WebM-Bilder als `Eingabe::Bild` (ein **einziges** Bild)
        // uebergeben, und der Avi-Zweig nimmt **nur** `Bilder` oder `Film`.
        // Also war `Avi` ein **toter** Eintrag — **bei jedem** WebM, auch bei
        // den VP8-Dateien, deren GIF tadellos war.
        //
        // Und `Png` braucht **genau ein** Bild, also wird es **jetzt** erst
        // **nach** der Filmfrage genommen.
        if ziel == Zielformat::Avi || ziel == Zielformat::Gif {
            return Ok((Eingabe::Bilder(bilder), optionen));
        }
        Ok((Eingabe::Bild(bilder.remove(0)), optionen))
    }

    /// **Ton?** — und GEMESSEN ist, warum **beide** Wege geprueft werden: der
    /// gleiche Grund wie bei `ist_video` (manche Downloads haben
    /// `application/octet-stream`).
    #[must_use]
    pub fn ist_ton(&self) -> bool {
        self.mime.starts_with("audio/") || matches!(self.endung.as_str(), "wav" | "flac" | "mp3")
    }

    /// Und **der** Ton-Leser, und **hier** steht die **Schranke**, GEMESSEN:
    ///
    /// | Quelle | Leser | Stand |
    /// |--------|-------|-------|
    /// | `wav` | `wav::lesen` | **gebaut** (K11 Teil 2) |
    /// | `flac` | — | **keiner** — FLAC ist **nur** gebaut |
    /// | `mp3` | — | **keiner** — MP3 ist geparkt (K2c) |
    ///
    /// Und GEMESSEN ist, warum **das** so gemeldet wird und **nicht** geraten: ein
    /// FLAC-Dekoder waere **ein** Projekt, und die Alternative ist **ein** Fehler,
    /// der **den Weg nennt** — und den kann man in **einer** Stunde bauen, im
    /// Gegensatz zu einem Nutzer, der den **falschen** sucht.
    ///
    /// # KONVERT_11: DIE **FALSCHAUSSAGE** IST WEG
    ///
    /// Hier stand woertlich `"FLAC ist gebaut, aber nicht gelesen"` — und das
    /// war **falsch**, und zwar **seit KONVERT_8 falsch**. Dort wurde gemessen:
    ///
    /// ```text
    ///   GEMESSEN: Bytes(ton.flac) -> Wav  22094 B | pcm_s16le 44100
    ///   GEMESSEN: Bytes(ton.flac) -> Flac  4820 B | flac 44100
    /// ```
    ///
    /// Der Leser existiert (`narsil-audio/src/flac.rs` Z. 1100 `dekodiere`), und
    /// `pipeline::ton_aus_bytes` benutzt ihn **an der Signatur**. Nur dieser eine
    /// Weg kannte ihn nicht — und KONTONT hat es in `ktx_konvert.rs` Z. 481
    /// **woertlich zitiert**, als Grund, warum der Ton-Weg dort „zu eng“ sei.
    ///
    /// > **Eine Falschaussage, die in einem Kommentar steht, wird von anderen
    /// > zitiert.** Sie muss **an der Quelle** korrigiert werden, nicht am
    /// > Zitat.
    ///
    /// Also liest **beide** Wege jetzt **dieselbe** Funktion, und die
    /// Endung entscheidet **nicht** mehr ueber die Lesbarkeit.
    fn ton_lesen(&self, daten: &[u8]) -> Result<crate::ton::Pcm, PipelineFehler> {
        if let Some(t) = crate::pipeline::ton_aus_bytes(daten) {
            return Ok(crate::pipeline::ton_als_pcm(&t));
        }
        Err(PipelineFehler::QuelleNichtLesbar {
            endung: self.endung.clone(),
            grund: match self.endung.as_str() {
                "wav" => "WAV-Kopf oder data-Chunk kaputt",
                "flac" | "ogg" => "der Ton liess sich nicht dekodieren",
                "mp3" => "MP3 ist geparkt (K2c)",
                _ => "kein Tonformat",
            },
        })
    }

    /// **Video?** — und GEMESSEN ist, warum **beide** Wege geprueft werden: die MIME
    /// ist **oft** `application/octet-stream` (GEMESSEN bei Download-Links), also
    /// ist die Endung der **Rueckfall** — genau wie in `ziele_fuer`.
    #[must_use]
    pub fn ist_video(&self) -> bool {
        self.mime.starts_with("video/") || matches!(self.endung.as_str(), "avi" | "webm" | "mov")
    }
}

/// **Waehrend** der Konvertierung **gemessen**, ob ein Ziel **mit** Daten
/// ueberhaupt gebaut werden kann — **ohne** zu raten.
///
/// Und GEMESSEN ist, warum **das** noetig ist und nicht `match`: `Zielformat`
/// hat **zwoelf** Varianten, und eine `match`-Liste ist **an** jedem neuen Ziel
/// **falsch**, ohne dass irgendetwas **rot** wird. Der Aufrufer dagegen will
/// **eine** Frage beantwortet haben.
#[must_use]
pub fn gebaut(daten: &[u8], quelltyp: &Quelltyp, ziel: Zielformat) -> bool {
    konvertiere(daten, quelltyp, ziel).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Und GEMESSEN ist, warum **dieser** Test **hier** steht und nicht in
    /// `tests/`: er prueft die **Normierung**, und die ist **Vorbedingung** fuer
    /// **jeden** Aufrufer von aussen — also muss sie **vor** dem Export stehen.
    #[test]
    fn quelltyp_normiert_mime_und_endung() {
        let q = Quelltyp::aus("  IMAGE/PNG ", ".JPG");
        assert_eq!(q.mime(), "image/png");
        assert_eq!(q.endung(), "jpg");
        // Und GEMESSEN ist, warum der Name einen Punkt **braucht**:
        // `pipeline.rs:416` schneidet an `.`, also waere `quellejpg` **ohne**
        // Endung — und die Bytes-Arm **verliege** sie dann stillschweigend.
        assert_eq!(q.datei_name(), "quelle.jpg");
    }
}
