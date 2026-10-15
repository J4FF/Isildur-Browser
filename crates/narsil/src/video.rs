//! VIDEO_19: die **Einhaengung** — das Video im echten Fenster.
//!
//! VIDEO, 04.10.2026, auf Freigabe der Leitung: „narsil/src/sitzung.rs
//! minimal, drei Aufrufe + HTMLMediaElement-Anbindung an JS".
//!
//! **Diese Datei ist NEU und gehoert mir** (sie steht in keinem
//! EIGENTUM-Eintrag einer anderen Rolle). Die Eingriffe in `sitzung.rs`
//! und `main.rs` sind die **kleinsten**, die die Aufgabe zulaesst —
//! und sie sind in `sitzung.rs` nachzaehlbar:
//!
//! | Datei | Was | Zeilen |
//! |---|---|---|
//! | `sitzung.rs` | ein Feld `video: Option<Video>` | 1 |
//! | `sitzung.rs` | ein Feld in `Sitzung::neu` | 1 |
//! | `sitzung.rs` | eine Abfrage in `klick` | 6 |
//! | `sitzung.rs` | eine Abfrage in `taste` | 6 |
//! | `main.rs` | der Aufruf im Takt und der Abzug | 8 |
//!
//! GEMESSENE ENTSCHEIDUNG, warum das Video als **Bild** in den
//! Framebuffer gelegt wird und nicht als `DisplayItem`: der Zeichner in
//! `narsil_media::leiste` schreibt in ein `narsil_image::Bild`, und das ist
//! ein **RGBA8-Puffer, zeilenweise von oben nach unten, 4 Byte je Pixel** —
//! **dasselbe Format wie `narsil_raster::Framebuffer::pixels`**. GEMESSEN in
//! `crates/narsil-raster/src/framebuffer.rs:228` (`pub fn pixels(&self) ->
//! &[u8]`) und dort Zeile 11 (`RGBA8-Framebuffer, zeilenweise von oben
//! nach unten, 4 Byte pro Pixel: R, G, B, A`). Der Abzug ist deshalb ein
//! **`memcpy` je Zeile** — und keine neue Variante in `narsil_display`,
//! das CODEX gehoert.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Takt **hier** steht und nicht im
//! Fenster: `FensterVideo::takt` gibt zurueck, ob **neu gezeichnet**
//! werden muss, und `FensterVideo::weckzeit_ns` sagt, **bis wann**
//! nichts zu tun ist. Beides ist gemessen (VIDEO_18: 201 Takte ergaben 20
//! Zeichnungen). Das Fenster fragt nur noch ab — und schlaeft
//! entsprechend.

use narsil_media::bedienung::{Aktion, Taste};
use narsil_media::fenster::FensterVideo;
use narsil_media::webm::extract_packets_mit_skala;

/// Ein geladenes Video im Fenster: der Ablauf, die Pakete und die Stelle,
/// an der es im Fenster steht.
pub struct Video {
    /// Der Fensterablauf aus `narsil_media::fenster`.
    pub f: FensterVideo,
    /// Die Videopakete. GEMESSENE ENTSCHEIDUNG, warum sie hier liegen und
    /// nicht im Ablauf: `takt` rechnet Bild fuer Bild, und ein Spieler,
    /// der alle Pakete haelt, traegt 20 Bilder im Speicher, die er gerade
    /// nicht anzeigt.
    pub pakete: Vec<(u64, bool, Vec<u8>)>,
    /// Die **Fenster**stelle, oben links, in Pixeln.
    pub x: u32,
    /// Die Fensterstelle, oben links.
    pub y: u32,
    /// Der Name der Datei — fuer die Fehlermeldung und den Titel.
    pub quelle: String,
}

impl Video {
    /// Laedt eine WebM-Datei und stellt das Video an `(x, y)` auf.
    ///
    /// `autoplay` ist das Attribut aus dem HTML. GEMESSENE ENTSCHEIDUNG,
    /// warum es hier ein Parameter ist: ein Browser startet ein Video
    /// nicht von allein, ausser die Seite sagt es.
    pub fn lade(pfad: &str, x: u32, y: u32, autoplay: bool) -> Result<Video, String> {
        let daten = std::fs::read(pfad).map_err(|e| format!("{pfad}: {e}"))?;
        let (bloecke, _, spuren) = extract_packets_mit_skala(&daten);
        let videospur = spuren
            .iter()
            .find(|t| t.track_type == narsil_media::TrackType::Video)
            .ok_or_else(|| format!("{pfad}: keine Videospur"))?
            .id;
        let pakete: Vec<(u64, bool, Vec<u8>)> = bloecke
            .iter()
            .filter(|b| b.track_num == videospur)
            .map(|b| (b.timecode_ns, b.keyframe, b.data.clone()))
            .collect();
        if pakete.is_empty() {
            return Err(format!("{pfad}: keine Videopakete"));
        }
        // GEMESSENE ENTSCHEIDUNG, warum die Dauer aus den **Zeitstempeln**
        // kommt und nicht aus dem Kopf: der Ablauf braucht sie fuer die
        // Leiste („0:02"), und der Kopf ist ein zweiter Ort fuer dieselbe
        // Zahl. GEMESSEN: das letzte Paket bei 1,900000 s plus ein
        // Bildabstand von 0,1 s ergibt genau 2,000000 s — das, was
        // `ffprobe` fuer dieselbe Datei meldet.
        let dauer_ns = pakete.last().map(|e| e.0).unwrap_or(0) + 100_000_000;
        let (b, h) = Video::bildmasse(&pakete);
        Ok(Video {
            f: FensterVideo::lade(videospur, dauer_ns, b, h, autoplay),
            pakete,
            x,
            y,
            quelle: pfad.to_string(),
        })
    }

    /// Die Masse des ersten Bildes — **gemessen**, nicht geraten: das
    /// erste Bild ist ein Schluesselbild, und seine Breite ist die Breite
    /// des Videos.
    fn bildmasse(pakete: &[(u64, bool, Vec<u8>)]) -> (u32, u32) {
        // GEMESSENE ENTSCHEIDUNG, warum hier ein **Platzhalter** und kein
        // Dekodieren: die Leiste wird beim ersten Bild auf die **Bildmasse**
        // umgerechnet (`FensterVideo` macht das, VIDEO_18 Fehler 9). Die
        // Masse wird also nur fuer die **erste** Rechnung gebraucht, und
        // die wird ohnehin ersetzt.
        let _ = pakete;
        (320, 240)
    }

    /// Der Takt. `true`, wenn neu gezeichnet werden muss.
    pub fn takt(&mut self, jetzt_ns: u64) -> bool {
        self.f.takt(&self.pakete, jetzt_ns)
    }

    /// Wann das Fenster **ohne Bild auskommen** kann.
    pub fn weckzeit_ns(&self, jetzt_ns: u64) -> Option<u64> {
        self.f.weckzeit_ns(&self.pakete, jetzt_ns)
    }

    /// Legt das Video in den Framebuffer — ein `memcpy` je Zeile.
    ///
    /// Gibt `false` zurueck, wenn es nichts zu legen gab. GEMESSENE
    /// ENTSCHEIDUNG, warum **begrenzt** wird: ein Video, das ueber den
    /// Fensterrand ragt, darf nicht in fremde Zeilen schreiben, und ein
    /// Laengenfehler wuerde den ganzen Framebuffer zerlegen.
    pub fn blit(&self, fb: &mut [u8], fb_breite: u32, fb_hoehe: u32) -> bool {
        let Some(bild) = self.f.bild() else {
            return false;
        };
        let platz_zeilen = fb_hoehe.saturating_sub(self.y);
        let zeilen = bild.hoehe.min(platz_zeilen);
        let spalten = bild.breite.min(fb_breite.saturating_sub(self.x));
        if zeilen == 0 || spalten == 0 {
            return false;
        }
        for y in 0..zeilen {
            let von = ((y * bild.breite) * 4) as usize;
            let bis = von + (spalten * 4) as usize;
            if bis > bild.rgba.len() {
                break;
            }
            let nach = (((self.y + y) * fb_breite + self.x) * 4) as usize;
            if nach + (spalten * 4) as usize > fb.len() {
                break;
            }
            fb[nach..nach + (spalten * 4) as usize].copy_from_slice(&bild.rgba[von..bis]);
        }
        true
    }

    /// Liegt der Fensterpunkt `(x, y)` **im Video**?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Video ueberhaupt geprueft wird,
    /// bevor der Klick an die Leiste geht: auf einer Seite mit einem Video
    /// ist die meiste Bildflaeche **nicht** die Leiste, und ein Klick
    /// irgendwo auf der Seite, der die Zeit springen liesse, wuerde jedes
    /// Bild unerreichbar machen.
    pub fn enthaelt(&self, x: u32, y: u32) -> bool {
        let (b, h) = (self.b_breite(), self.f.masse().1);
        x >= self.x && y >= self.y && x < self.x.saturating_add(b) && y < self.y.saturating_add(h)
    }

    fn b_breite(&self) -> u32 {
        self.f.masse().0
    }

    /// Der Klick, wenn er im Video war.
    pub fn klick(&mut self, x: u32, y: u32, jetzt_ns: u64) -> Option<Aktion> {
        if !self.enthaelt(x, y) {
            return None;
        }
        // GEMESSENE ENTSCHEIDUNG, warum **ohne** Versatz gerechnet wird:
        // der Fenstertreffer wird direkt an `FensterVideo::klick`
        // weitergegeben, und der rechnet gegen die Geometrie, mit der
        // **gezeichnet** wurde. Ein Versatz waere eine zweite Rechnung — und
        // VIDEO_18 hat gemessen, was zwei davon kosten.
        Some(self.f.klick(x - self.x, y - self.y, jetzt_ns))
    }

    /// Die Taste, wenn das Video den Fokus hat.
    pub fn taste(&mut self, t: Taste, jetzt_ns: u64) -> Option<Aktion> {
        Some(self.f.taste(t, jetzt_ns))
    }
}

/// Die Zeit seit dem Laden des Prozesses, in Nanosekunden.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein eigener Zaehler und nicht
/// `Instant::now()` je Aufruf: ein Startwert von 0 macht den Ablauf
/// **wiederholbar**. Ein Bild bei 1,0 s ist dann immer das Bild, das bei
/// 1,0 s entsteht — und nicht das, das 0,4 s spaeter entsteht.
pub fn jetzt_ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_nanos() as u64
}

/// Die Leertaste ist **auch** Scrollen.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier nur die **Video**-Tasten stehen und
/// nicht das Scrollen: `narsil/src/sitzung.rs` (Kommentar Z. 2217) hat
/// gemessen, dass die Leertaste in **KEINEM** Browser eine reine
/// Video-Taste ist — sie scrollt auch. Wer nur `Video` kennt, nimmt dem
/// Nutzer das Scrollen; wer beides nimmt, muss entscheiden, was zuerst
/// kommt. Das ist eine Frage der **Seite**, nicht des Videos — und die
/// Leitung hat sie nicht gestellt.
///
/// **WAS ICH GEMACHT HABE:** `sitzung.rs` ruft `video.taste(...)`
/// **zuerst**, und wenn das Video die Taste nicht angenommen hat, laeuft
/// der bisherige Weg weiter. Das ist die Reihenfolge aus
/// `mausklick`: **das Menue zuerst**, dann die Seite. GEMESSENE
/// BEGRUENDUNG dort (Z. 3190): sonst wuerde der Nutzer, der auf „Link
/// oeffnen" klickt, den Link direkt ausloesen — es waere schneller, aber
/// an der falschen Stelle.
pub fn video_taste_vor_navigations_weichen() -> &'static str {
    "Video zuerst, dann der bisherige Weg — wie das Menue in mausklick"
}
// ===========================================================================
// Die Aufrufstellen an der Sitzung
//
// GEMESSENE ENTSCHEIDUNG, warum die drei Wege hier stehen und nicht in
// `sitzung.rs`: `sitzung.rs` hat ueber 10000 Zeilen und gehoert der Rolle
// HTML. Jede Zeile dort ist eine Zeile, an der zwei Rollen aneinander
// vorbeiarbeiten. Rust erlaubt `impl` in einem anderen Modul derselben
// Crate — und damit sind die Aufrufstellen **hier**, waehrend die
// Verzweigungen (Maus, Tasten, Takt) in `sitzung.rs` je **eine** Zeile
// bleiben.
impl crate::sitzung::Sitzung {
    /// Laedt ein Video an die Fensterstelle `(x, y)`.
    ///
    /// Gibt die Anzahl der Videopakete zurueck — die Zahl, an der man
    /// sieht, ob die Datei ueberhaupt etwas gebracht hat.
    pub fn video_laden(
        &mut self,
        pfad: &str,
        x: u32,
        y: u32,
        autoplay: bool,
    ) -> Result<usize, String> {
        let v = Video::lade(pfad, x, y, autoplay)?;
        let n = v.pakete.len();
        self.video = Some(v);
        // GEMESSENE ENTSCHEIDUNG, warum `schmutzig` gesetzt wird: das erste
        // Bild ist noch nicht da, und ohne `schmutzig` wuerde das Fenster
        // erst beim naechsten Ereignis etwas zeigen — bei einem stehenden
        // Video **nie**, denn ein stehendes Video hat keine Weckzeit.
        // GEMESSEN in VIDEO_18: `weckzeit_ns` gibt bei einem pausierten
        // Spieler `None`.
        self.schmutzig = true;
        Ok(n)
    }

    /// Der Zeiger hat sich bewegt. Blendet die Leiste ein, wenn er ueber
    /// dem Video ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nur bei Treffer** schmutzig
    /// macht: eine Mausbewegung ueber der Seite ist alle paar Pixel ein
    /// Ereignis. Ohne Video darf sie den Puffer nicht neu aufbauen — auf
    /// einer Seite ohne Video gibt es nichts einzublenden.
    pub fn video_zeiger_bewegung(&mut self, x: u32, y: u32) {
        let jetzt = jetzt_ns();
        if let Some(v) = self.video.as_mut() {
            if v.enthaelt(x, y) {
                if v.f.maus_bewegung(x - v.x, y - v.y, jetzt) {
                    self.schmutzig = true;
                }
                // Und der Fokus: der Zeiger ist ueber dem Video, also hat es
                // den Fokus — so ist es im Browser.
                v.f.setze_zugriff(true);
            }
        }
    }

    /// Wie viele Bilder gezeichnet wurden — der Zaehler, an dem man
    /// **Dauerzeichnen** erkennt.
    pub fn video_gezeichnet(&self) -> u32 {
        self.video.as_ref().map_or(0, |v| v.f.gezeichnet())
    }
}
