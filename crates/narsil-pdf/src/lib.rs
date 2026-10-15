//! narsil-pdf — PDF lesen und Seiten als Bild rendern.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF aus
//! `team/AUTONOM.txt` (Abschnitt „NUTZER-ANFORDERUNGEN").
//!
//! # WAS DIESE CRATE KANN
//!
//!  * klassische Querverweis-Tabelle **und** XRef-Streams
//!  * Objekt-Streams (PDF 1.5+)
//!  * Filter: FlateDecode mit PNG- und TIFF-Prädiktor, LZWDecode,
//!    ASCII85Decode, ASCIIHexDecode, RunLengthDecode
//!  * Seitenbaum ueber `/Kids` (mit Zyklusschutz und Grenzen)
//!  * Content-Stream: Pfade (Linie, kubische Kurve, Rechteck),
//!    Fuellen nach non-zero und even-odd, Kontur mit Dicke,
//!    Text mit eingebetteten und mit System-Schriften,
//!    Formulare und Bilder
//!  * Bilder: DCTDecode (JPEG) ueber `narsil-image`
//!
//! # DIE DREI DINGE, die man wissen muss, bevor man Fehler sucht
//!
//!  1. **Ohne `/Length` stimmt die Streamlaenge nicht.** GEMESSEN: bei
//!     drei der zehn PDFs ist `/Length` kleiner als der echte Stream.
//!     Der Decoder nimmt dann `endstream` als Ende.
//!  2. **PDF zaehlt von unten links.** Die Umrechnung zum Bild steht in
//!     [`raster::Flaeche::zu_pixel`] und an keiner anderen Stelle.
//!  3. **Nicht jeder Operator ist ein Fehler.** `BX`/`EX`,
//!     `d0`/`d1` und Schattenoperatoren kommen in echten PDFs vor und
//!     werden gemeldet und uebersprungen.
//!
//! # BENUTZUNG
//!
//! ```no_run
//! use narsil_pdf::{Dokument, Rasteroptionen, seite_als_bild};
//!
//! let daten = std::fs::read("beispiel.pdf").map_err(|e| e.to_string())?;
//! let mut d = Dokument::oeffnen(daten).map_err(|e| e.to_string())?;
//! let seiten = d.seiten().map_err(|e| e.to_string())?;
//! // added 03.10.2026: `seite_als_bild` ist eine **freie Funktion**,
//! // keine Methode — `d.seite_als_bild(…)` gab es nie. Der Doc-Test
//! // hat es beim ersten Lauf gesagt (E0599), und die Fehlermeldung
//! // nannte die freie Funktion gleich mit.
//! for (i, seite) in seiten.iter().enumerate() {
//!     let bild = seite_als_bild(&mut d, seite, Rasteroptionen::standard())
//!         .map_err(|e| e.to_string())?;
//!     std::fs::write(format!("seite{}.png", i + 1), &bild.png)
//!         .map_err(|e| e.to_string())?;
//! }
//! # Ok::<(), String>(())
//! ```

#![forbid(unsafe_code)]

pub mod cff;
pub mod dokument;
pub mod filtern;
pub mod inhalt;
pub mod lexing;
pub mod objekt;
pub mod raster;
pub mod schrift;
mod standard14;
pub mod xref;

// added 03.10.2026: DIE OEFFENTLICHE API wird hier **neu exportiert**.
// Die Crate hatte vorher nur ihre Module `pub`, nicht ihre Typen —
// `narsil_pdf::Dokument` war nicht erreichbar, und der erste Testlauf
// sagte „struct `Dokument` is private". Fuer eine Bibliothek ist das
// kein Detail: sie ist ohne ihre Typen nicht benutzbar.
pub use dokument::{Dokument, DokumentFehler, Seite};
pub use filtern::FilterFehler;
pub use inhalt::{Aktion, Matrix, Pfad, Pfadteil, Punkt, Segment, Zustand};
pub use lexing::{Lexer, Token};
pub use objekt::{FehlenderVerweis, Wert};
pub use raster::Flaeche;
pub use schrift::{Ressource, SchriftArt};
pub use xref::{Eintrag, Querverweis, XrefFehler};

use raster::DPI_STANDARD;

/// Ein Fehler beim Rendern (nach dem Parsen).
#[derive(Debug, Clone, PartialEq)]
pub enum RenderFehler {
    /// Das Dokument liess sich nicht oeffnen.
    Dokument(DokumentFehler),
    /// Der Seiteninhalt liess sich nicht dekodieren.
    Inhalt(DokumentFehler),
    /// Die Seite hat keine brauchbare Groesse.
    Groesse(String),
    /// Das Zielbild waere zu gross.
    ZuGross(u32, u32),
    /// Eine Ressource liess sich nicht laden.
    Ressource(String),
}

impl std::fmt::Display for RenderFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderFehler::Dokument(e) => write!(f, "Dokument: {e}"),
            RenderFehler::Inhalt(e) => write!(f, "Seiteninhalt: {e}"),
            RenderFehler::Groesse(m) => write!(f, "Seitengroesse: {m}"),
            RenderFehler::ZuGross(b, h) => write!(f, "Ziel {b}x{h} ueber der Grenze"),
            RenderFehler::Ressource(m) => write!(f, "Ressource: {m}"),
        }
    }
}

impl From<DokumentFehler> for RenderFehler {
    fn from(e: DokumentFehler) -> Self {
        RenderFehler::Dokument(e)
    }
}

/// Wie eine Seite gerendert wird.
///
/// added 03.10.2026. Als **Optionen** und nicht als Parameter Liste,
/// weil die Abnahme genau eine Zahl aendert (die DPI) und sonst nichts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rasteroptionen {
    /// Aufloesung in Punkten je Zoll.
    pub dpi: f64,
    /// Hoechstbreite/-hoehe in Pixel (des **Ergebnisses**).
    pub max_kante: u32,
    /// Kantenlaenge der Ueberzeichnung: Seite wird `ueberzeichnung`
    /// mal so gross gezeichnet und dann gemittelt.
    ///
    /// added 03.10.2026. 3 ist der Standard, weil 3 durch jede
    /// Schriftgroesse teilbar ist und die Kanten bei 3 Stichproben
    /// sauber werden. 1 heisst „keine Glaettung" und liefert bei
    /// 72 dpi unlesbare Seiten (siehe `raster::verkleinere`).
    pub ueberzeichnung: u32,
}

impl Rasteroptionen {
    /// Die Vorgaben: 144 DPI (1224 x 1584 fuer US-Letter).
    ///
    /// added 03.10.2026. Warum 144 und nicht 96: bei 96 ist der
    /// kleinste lesbare Text aus `rfc2616.pdf` (6 Punkt) drei Pixel
    /// hoch und damit ein graues Feld. GEMESSEN an den zehn PDFs.
    pub fn standard() -> Rasteroptionen {
        Rasteroptionen {
            dpi: DPI_STANDARD,
            max_kante: raster::MAX_KANTE,
            // added 06.10.2026: DIE VORGABE IST **VIER**, seit der
            // Messung aus Abschnitt 19.5 — vorher 3.
            //
            // GEMESSEN, warum das eine Stellgroesse ist und kein
            // Kostenuetzer (`1207.7214.pdf`, alle 38 Seiten, 72 dpi, gegen
            // `pdftoppm`):
            //
            // ```text
            //   Ueberzeichnung 3   2 169 104 abweichende Pixel    16,5 s
            //   Ueberzeichnung 4   2 081 784                      17,2 s   (-4,0 %)
            //   Ueberzeichnung 6   2 028 087                      30,6 s   (-6,5 %)
            // ```
            //
            // `rfc9110.pdf`, Seiten 1–10: 168 891 -> 166 826 -> 166 023,
            // also -1,2 % und -1,7 %. Vier kostet **4 % mehr Zeit**, sechs
            // kostet **85 % mehr** fuer noch 2,5 % — deshalb vier.
            //
            // DER GRUND, warum man an dieser Zahl ueberhaupt dreht: die
            // Seite wird `n`-fach so gross gezeichnet und gemittelt, und
            // ein Versatz von **1/3** Pixel wird bei einem 9-Punkt-Font zu
            // einem **ganzen** Pixel. Genau das war an `rfc9110.pdf`
            // Seite 1 zu sehen (Bericht 19.2): gleiche Glyphen, eine
            // Zeile versetzt.
            //
            // DIE KOSTEN, und sie sind quadratisch — die Flaeche waechst
            // mit `n^2`: bei 144 dpi sind das 70 MB (3) gegen 124 MB (4)
            // je Seite, bei 300 dpi 303 MB gegen 538 MB. Fuer einen
            // Betrachter, der viele Seiten offen haelt, ist das der Punkt,
            // an dem man zurueck auf 3 geht — die Bruecken
            // [`Self::mit_zoom`] und [`Self::fuer_flaeche`] bleiben
            // deshalb bei **3** und sind damit die **sparsame** Wahl fuer
            // die Oberflaeche.
            ueberzeichnung: 4,
        }
    }

    /// Die Vorgaben fuer einen **Zoomfaktor** der Oberflaeche.
    ///
    /// added 04.10.2026. DIE BRUECKE ZUR UI, und sie ist genau eine
    /// Rechnung: bei `dpi = 72` ist ein Punkt genau ein Pixel, also
    /// gilt fuer jeden Zoomfaktor `dpi = 72 * faktor`.
    ///
    /// GEMESSEN: `1512.03385.pdf` Seite 1 ist 612 x 792 Punkte. Bei
    /// `faktor 1.0` kommen **612 x 792** Pixel heraus, bei `0.5`
    /// **306 x 396**, bei `5.0` waeren es 3 060 x 3 960 — und das
    /// **schlaegt fehl**, siehe [`Rasteroptionen::groesster_zoom`].
    ///
    /// DIE GRENZEN SIND DIE DER OBERFLAECHE, nicht geraten: die
    /// Werkzeugleiste in `narsil-chrome` (`pdf_viewer.rs`) laesst den Zoom
    /// zwischen `MIN_ZOOM` (25 %) und `MAX_ZOOM` (500 %) wandern, und
    /// ausserhalb davon wird auf den Rand geklemmt. 25 % sind 18 dpi,
    /// 500 % sind 360 dpi.
    ///
    /// Eine Zahl, die nicht in `[18, 360]` liegt (auch `NaN`), wird auf
    /// [`Rasteroptionen::standard`] gesetzt: ein `NaN` als
    /// Aufloesung ergibt bei `flaeche_fuer` eine Flaeche von **0 x 0**
    /// Pixeln, und „0 x 0" ist keine Fehlermeldung, sondern ein leeres
    /// Bild.
    pub fn mit_zoom(faktor: f32) -> Rasteroptionen {
        let dpi_min: f64 = 18.0;
        let dpi_max: f64 = 360.0;
        let f = if faktor.is_finite() {
            faktor as f64
        } else {
            0.0
        };
        if f <= 0.0 {
            return Rasteroptionen::standard();
        }
        Rasteroptionen {
            dpi: (72.0 * f).clamp(dpi_min, dpi_max),
            max_kante: raster::MAX_KANTE,
            // added 06.10.2026: hier bleibt es bei **3** — siehe die
            // Speicherkosten in [`Self::standard`]. Der Betrachter
            // rendert viele Seiten, die Abnahme eine.
            ueberzeichnung: 3,
        }
    }

    /// Die Vorgaben fuer ein **Zielrechteck** in Pixeln — die Bruecke zu
    /// einem Betrachter, dessen Zoom „Einpassen“ bedeutet.
    ///
    /// added 05.10.2026, Rolle SERVER. Anlass: [`Self::mit_zoom`] rechnet
    /// `dpi = 72 * faktor`, und das stimmt nur, wenn „Zoom 100 %“ ein
    /// Punkt-pro-Pixel-Fenster von 1224 Pixeln Breite meint. Der
    /// Betrachter in `narsil` hat aber einen anderen Zoom: sein `1.0` ist
    /// **Einpassen** in ein beliebig grosses Fenster, und seine Seite wird
    /// danach auf das Rechteck `narsil_chrome::pdf_viewer::seiten_rect`
    /// **gestreckt**.
    ///
    /// GEMESSEN, was das kostet (`1706.03762.pdf` Seite 1, gegen
    /// `pdftoppm -r <dpi>`):
    ///
    /// ```text
    ///   144 dpi   132 614 abweichende Pixel   mittel 3,86   93 % gleich
    ///   288 dpi   308 269 abweichende Pixel   mittel 2,86   96 % gleich
    /// ```
    ///
    /// Die **mittlere** Abweichung ist bei 288 dpi um 26 % kleiner. Ein
    /// Raster, das auf 144 dpi gerendert und dann gestreckt wird, kann
    /// nicht besser sein als seine eigene Aufloesung — der Verlust ist
    /// also mindestens dieser, und die Weichzeichnung, die die Zahl gar
    /// nicht sieht, kommt obendrauf.
    ///
    /// DIE RECHNUNG ist genau eine: aus `pixel = punkt * dpi / 72` folgt
    ///
    /// ```text
    ///   dpi = ziel_pixel / (seiten_punkt / 72)
    /// ```
    ///
    /// und von den **beiden** Achsen nimmt man die **kleinere**, weil das
    /// kleinere Ziel die Seite vollstaendig zeigt (die groessere wuerde
    /// nur daneben wegfallen).
    ///
    /// DIE OBERGRENZE ist 360 dpi, wie bei [`Self::mit_zoom`]: bei Zoom
    /// 500 % und einem 1224-Pixel-Fenster waeren es 4.212 Pixel in der
    /// Hoehe, das sind 12 Millionen Pixel — bei 4 Byte je Pixel rund
    /// 50 MB **pro Seite**, und der Betrachter darf bei 200 Seiten nicht
    /// 10 GB reservieren. Wer mehr braucht, rendert Seitenweise.
    ///
    /// Beispiele: US-Letter (612 x 792 Punkte) in ein 1224 x 1584
    /// grosses Fenster ergibt **144** dpi; dasselbe Fenster bei Zoom 2
    /// (2448 x 3168) ergibt **288** dpi; A4 (595 x 842) bei Zoom 1.0 im
    /// selben Fenster ergibt **106** dpi (die Hoehe ist die kleinere
    /// Achse).
    ///
    /// Eine Laenge von 0 (Schnitt bei `NaN`) oder ein nicht endlicher
    /// Zoom ergibt [`Self::standard`] — ein leeres Bild ist keine
    /// Fehlermeldung, sondern ein weisses Blatt.
    pub fn fuer_flaeche(seiten_punkt: [f64; 4], ziel_pixel: [u32; 2]) -> Rasteroptionen {
        let dpi_min: f64 = 18.0;
        let dpi_max: f64 = 360.0;
        let breite_p = (seiten_punkt[2] - seiten_punkt[0]).abs();
        let hoehe_p = (seiten_punkt[3] - seiten_punkt[1]).abs();
        if !breite_p.is_finite() || !hoehe_p.is_finite() || breite_p < 1.0 || hoehe_p < 1.0 {
            return Rasteroptionen::standard();
        }
        let zz = |v: u32| if v == 0 { f64::NAN } else { v as f64 };
        let dpi_b = zz(ziel_pixel[0]) / (breite_p / 72.0);
        let dpi_h = zz(ziel_pixel[1]) / (hoehe_p / 72.0);
        let kandidaten = [dpi_b, dpi_h].into_iter().filter(|v| v.is_finite());
        // added 05.10.2026: DIE KLEINERE ACHSE gewinnt. Die groessere
        // wuerde nur Bildflaeche daneben wegschneiden — der Betrachter
        // zeigt lieber eine kleinere Seite vollstaendig als eine grosse
        // zur Haelfte.
        // added 05.10.2026: `min_by` liefert ein `Option`, kein `Result` —
        // die erste Fassung schrieb `let Ok(dpi) = ...` und der Compiler
        // sagte es beim ersten Lauf (E0308).
        let Some(dpi) =
            kandidaten.min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        else {
            return Rasteroptionen::standard();
        };
        if dpi < dpi_min {
            // added 05.10.2026: UNTER 18 dpi ist die Seite unsichtbar
            // klein; statt ein leeres Blatt zu liefern, bleibt die
            // kleinste zulaessige Aufloesung. GEMESSEN an der Leiste von
            // `narsil-chrome`: `MIN_ZOOM` = 0.25, und `mit_zoom` klemmt
            // genauso.
            return Rasteroptionen {
                dpi: dpi_min,
                max_kante: raster::MAX_KANTE,
                // added 06.10.2026: siehe [`Self::standard`] — die
                // Viewer-Bruecke bleibt bei 3 wegen des Speichers.
                ueberzeichnung: 3,
            };
        }
        Rasteroptionen {
            dpi: dpi.clamp(dpi_min, dpi_max),
            max_kante: raster::MAX_KANTE,
            // added 06.10.2026: hier bleibt es bei **3** — siehe die
            // Speicherkosten in [`Self::standard`]. Der Betrachter
            // rendert viele Seiten, die Abnahme eine.
            ueberzeichnung: 3,
        }
    }

    /// Der groesste Zoomfaktor, bei dem diese Seite **noch** passt.
    ///
    /// added 04.10.2026. GEMESSEN UND DESHALB WICHTIG: der Renderer
    /// **klemmt nicht**, sondern **meldet** — `Renderer::seite` gibt
    /// `RenderFehler::ZuGross` zurueck, wenn die Zielgroesse
    /// `max_kante` (4.000 Pixel) uebersteigt. Ohne diese Zahl wuerde die
    /// Oberflaeche den Zoom-Knopf bis 500 % weiterdruecken und dann ein
    /// **Fehlerbild** bekommen.
    ///
    /// Die Rechnung ist die Umkehrung von `ziel_groesse`: aus
    /// `pixel = punkt * dpi / 72` folgt `zoom = max_kante / punkt`, und
    /// es zaehlt die **laengere** der beiden Seiten, weil eine Drehung
    /// sie nur vertauscht.
    ///
    /// GEMESSEN an A4 (595 x 842 Punkte, vier echte Testdateien):
    /// `4.75` — bei `5.0` (500 %, der Grenze der Leiste) waeren es
    /// 4 210 Pixel in der Hoehe, und der Lauf bricht mit `ZuGross` ab.
    pub fn groesster_zoom(media: [f64; 4]) -> f32 {
        let breite = (media[2] - media[0]).abs();
        let hoehe = (media[3] - media[1]).abs();
        let gross = breite.max(hoehe);
        if !gross.is_finite() || gross <= 0.0 {
            return 1.0;
        }
        (raster::MAX_KANTE as f64 / gross) as f32
    }
}

/// Was beim Rendern einer Seite herauskam.
///
/// added 03.10.2026. Nicht nur das Bild, sondern auch die **Zahlen**:
/// gefuellte Pixel, gezeichnete Zeichen, gesetzte Bilder. Ohne die gibt
/// es keine Abnahme — „19 Seiten gerendert" sagt nichts darüber, ob
/// auf den Bildern etwas steht.
#[derive(Debug, Clone, PartialEq)]
pub struct Seitenbild {
    /// Die PNG-Bytes.
    pub png: Vec<u8>,
    /// Breite in Pixel.
    pub breite: u32,
    /// Hoehe in Pixel.
    pub hoehe: u32,
    /// Wie viele **Schreibvorgaenge** auf die Flaeche gingen.
    ///
    /// added 03.10.2026. DIE ERSTE FASSUNG HIESS `gefuellte_pixel` und
    /// behauptete damit „Pixel auf dem Bild". Das ist etwas anderes:
    /// `set_pixel` gibt `true` auch dann zurueck, wenn das Pixel schon
    /// schwarz war. GEMESSEN: `fw9.pdf` kam so auf **542.311.970** bei
    /// einem Bild von 1.990.000 Pixeln — Faktor 272. Zwei verschiedene
    /// Dateien (`rfc9110.pdf`, `rfc9111.pdf`) kamen auf **denselben**
    /// Wert, weil beide dieselbe Ueberzeichnung hatten.
    ///
    /// Der Name ist deshalb jetzt `schreibvorgaenge`, und die Aussage
    /// ueber das Bild steht in [`Seitenbild::sichtbare_pixel`].
    pub schreibvorgaenge: usize,
    /// Wie viele Textzeichen gezeichnet wurden.
    pub zeichen: usize,
    /// Wie viele Bilder gesetzt wurden.
    pub bilder: usize,
    /// Wie viele Pixel sich vom Hintergrund unterscheiden.
    ///
    /// added 03.10.2026. DAS ist die Zahl, auf die die Abnahme schaut.
    /// Sie enthaelt jeden Punkt genau einmal, unabhaengig davon, wie oft
    /// er geschrieben wurde, und sie ist damit auch die Zahl, an der
    /// man zwei Renderdurchlaeufe **vergleichen** kann.
    pub sichtbare_pixel: usize,
    ///
    /// added 03.10.2026. GEMESSEN: `rfc9110.pdf` hat 3502 davon. Die
    /// Zahl steht im Bericht, weil „3502 Formulare, 0 Pixel" ein
    /// Befund ist und „3502 Formulare, 900 Pixel" keiner.
    pub formulare: usize,
    /// added 07.10.2026: Formen, die **nicht** gezeichnet wurden, weil
    /// `MAX_FORM_TIEFE` (8) erreicht war — dazu die groesste Tiefe, die
    /// wirklich vorkam.
    ///
    /// GEMESSEN, warum die Zahl noetig ist: `bilanz.formulare` zaehlt
    /// nur die **gezeichneten** Formen. Ein Dokument, das seine Formen
    /// bei der Verschachtelung verliert, meldet damit eine gesunde Zahl
    /// und ein leeres Bild — das ist bei `2103.00020.pdf` Seite 42 genau
    /// passiert (Tinte 1,5 M gegen 20,7 M der Referenz).
    pub formen_abbruch_tiefe: usize,
    /// Die groesste Verschachtelungstiefe, die der Renderer gesehen hat.
    pub formen_tiefe_max: usize,
    /// added 07.10.2026: Formen, die als **Zyklus** erkannt und nicht
    /// gezeichnet wurden (ein Formular, das sich selbst eintraegt).
    pub formen_zyklus: usize,
    /// Operatoren, die es nicht gab.
    pub unbekannt: Vec<String>,
    /// Glyphen aus eingebetteten CFF-Programmen (`/FontFile3`).
    ///
    /// added 03.10.2026. GEMESSEN: `f1040.pdf` 9, `fw9.pdf` 6,
    /// `1207.7214.pdf` 17, `1512.03385.pdf` 10, `rfc9110.pdf` 1
    /// eingebettete CFF-Programme — zusammen **43**. Vor dem CFF-Weg
    /// war diese Zahl 0 und die Schriften wurden mit einem Systemfont
    /// gezeichnet.
    pub cff_glyphen: usize,
    /// added 05.10.2026: Zeichen, fuer die die Ersatzschrift **keine**
    /// Glyphe hat — nach dem Versuch ueber das Zeichen **und** ueber den
    /// Glyphennamen.
    pub ersatz_fehl: usize,
    /// added 05.10.2026: Konturen, die **mit** Strichmuster (`d`)
    /// gezeichnet wurden, und wie viele „aus“-Stuecke dabei **nicht**
    /// gezeichnet wurden.
    ///
    /// GEMESSEN, warum dieser Zaehler noetig ist: das Strichmuster
    /// aendert im Korpus **kein einziges Pixel** — der A/B ueber die vier
    /// Dateien mit Mustern (`werkzeuge/dash_abgleich.sh`, 100 Seiten)
    /// ergab **+0** auf jeder Seite, bytegleiche Bilder. Entweder werden
    /// die gestrichelten Konturen gar nicht gemalt, oder sie fallen nicht
    /// ins Bild. Dieser Zaehler sagt, welcher der beiden Faelle es ist —
    /// ohne ihn waere „+0“ eine Zahl ohne Erklaerung.
    ///
    /// `stuecke_ausgelassen` ist die **Tatsache der Aenderung**: jedes
    /// ausgelassene Stueck ist Strich, der **nicht** hingesetzt wurde, wo
    /// die Referenz welchen zeichnet.
    pub kontur_gemustert: usize,
    pub stuecke_ausgelassen: usize,
}

/// Schneidet Pfadteile an einem Rechteck — in **Seitenkoordinaten**.
///
/// added 05.10.2026. Der Umweg geht ueber Polygone, weil der Fuellvorgang
/// (`raster::pfad_fuellen`) Polygone fuellt und nichts ueber Kanten
/// weiss. Ein Teilpfad wird zu einer Punktliste (Startpunkt plus alle
/// Segmentendpunkte), die Liste wird geschnitten (Sutherland–Hodgman) und
/// als `Pfadteil` mit **Geraden** zurueckgegeben.
///
/// DIE PHASE der Even-Odd-Regel bleibt erhalten: `gerade` wird vom
/// Aufrufer durchgereicht und nicht angefasst.
///
/// added 05.10.2026: eine **leere** Liste bedeutet „liegt ganz ausserhalb“
/// und wird vom Aufrufer als „nichts zeichnen“ gelesen — nicht als Fehler.
fn pfadteile_geschnitten(teile: &[inhalt::Pfadteil], r: [f64; 4]) -> Vec<inhalt::Pfadteil> {
    let mut raus = Vec::with_capacity(teile.len());
    for teil in teile {
        let mut poly = vec![(teil.von.x, teil.von.y)];
        let mut passt = true;
        for seg in &teil.segmente {
            match seg {
                inhalt::Segment::Linie(a, b) => {
                    poly.push((a.x, a.y));
                    poly.push((b.x, b.y));
                }
                // added 05.10.2026: EINE KURVE IM CLIPP-PFAD heisst
                // „kein Rechteck“, und das entscheidet schon der
                // Aufrufer. Hier bleibt der Pfad **unveraendert**, statt
                // still zu verschwinden — sonst wuerde eine Kugelform im
                // Clipp einfach wegfallen.
                _ => {
                    passt = false;
                    break;
                }
            }
        }
        if !passt {
            raus.push(teil.clone());
            continue;
        }
        let geschnitten = raster::schneide_polygon(&poly, r);
        if geschnitten.len() < 3 {
            continue;
        }
        let mut segmente = Vec::with_capacity(geschnitten.len());
        for i in 0..geschnitten.len() {
            let a = geschnitten[i];
            let b = geschnitten[(i + 1) % geschnitten.len()];
            segmente.push(inhalt::Segment::Linie(
                inhalt::Punkt { x: a.0, y: a.1 },
                inhalt::Punkt { x: b.0, y: b.1 },
            ));
        }
        raus.push(inhalt::Pfadteil {
            von: inhalt::Punkt {
                x: geschnitten[0].0,
                y: geschnitten[0].1,
            },
            segmente,
        });
    }
    raus
}

/// Der Renderer: kennt das Dokument und setzt Aktionen um.
pub struct Renderer<'a> {
    /// Das Dokument (fuer Ressourcen und Formulare).
    pub dokument: &'a mut Dokument,

    /// Die Optionen.
    pub opt: Rasteroptionen,
    /// Wie tief Formulare betreten werden (M3).
    ///
    /// added 03.10.2026. 8. Ein Formular, das sich selbst als Kind
    /// eintraegt, waere sonst endlos; bei 8 sieht man so etwas nur,
    /// wenn man es baut.
    pub form_tiefe: usize,
    /// Wie viele Formulare insgesamt gezeichnet werden (M3).
    pub form_zaehler: usize,
    /// added 07.10.2026: Formen, die an `MAX_FORM_TIEFE` gescheitert
    /// sind — siehe [`Bilanz::formen_abbruch_tiefe`].
    pub form_abbruch_tiefe: usize,
    /// added 07.10.2026: die groesste Verschachtelungstiefe, die der
    /// Renderer gesehen hat — siehe [`Bilanz::formen_tiefe_max`].
    pub form_tiefe_gesehen: usize,
    /// added 07.10.2026: Formen, die als **Zyklus** erkannt und nicht
    /// gezeichnet wurden — siehe `form_kette`.
    pub form_zyklus: usize,
    /// added 07.10.2026: die **Objectnummern** der Formen auf dem
    /// aktuellen Weg. Das ist der eigentliche Schutz gegen Endlosigkeit;
    /// `MAX_FORM_TIEFE` ist nur die Rueckfallgrenze.
    pub form_kette: Vec<u32>,
    /// Die Schriften, einmal geladen, nach Objektnummer.
    ///
    /// added 03.10.2026. DER GRUND FUER DEN CACHE IST DIE MENGE, nicht
    /// die Bequemlichkeit: `narsil9110.pdf` hat **3502** Form-XObjects,
    /// `1706.03762.pdf` 2565. Ohne Cache wuerde fuer jedes Formular
    /// dieselbe eingebettete TrueType-Datei neu geparst — 3502 mal
    /// dieselbe Datei.
    ///
    /// `Rc` statt `Clone`, weil `Ressource` eine eingebettete Schrift
    /// besitzt und die nicht kopiert werden kann.
    schrift_cache: std::collections::HashMap<u32, std::rc::Rc<schrift::Ressource>>,
    /// Die Ersatzschrift, einmal geladen.
    ersatz: Ersatzsaetze,
}

/// Wie tief Formulare ineinander gezeichnet werden (M3).
///
/// added 03.10.2026 auf **8** gesetzt, am 07.10.2026 auf **64**
/// geaendert — und die alte Begruendung war **falsch gemessen**:
///
/// ```text
///   alt behauptet:  „die groesste Verschachtelung ist 1“
///   alt gemessen (`2103.00020.pdf` Seite 42):
///                    form_tiefe=8, form_abbruch=108
/// ```
///
/// 108 Formen wurden auf **einer** Seite stillschweigend verworfen, und
/// das Bild blieb bis auf eine Zeile leer (Tinte 1,5 M gegen 20,7 M der
/// Referenz). Die alte Zahl kam zustande, weil es **keinen Zaehler**
/// gab, der sie haette zeigen koennen — dieselbe Fehlerform wie beim
/// `BI`-Kommentar („in keinem der zehn PDFs“).
///
/// WARUM 64 und nicht unbegrenzt: eine unbegrenzte Verschachtelung ist
/// bei einem Formular, das sich selbst als Kind eintraegt, endlos. Der
/// Schutz dagegen ist jetzt die **Kette** (`form_kette`): sie merkt
/// sich die Objectnummern auf dem aktuellen Weg. Die Tiefenzahl ist
/// damit nur noch die **Rueckfallgrenze** fuer Ketten, die sich nicht
/// wiederholen, sondern nur sehr tief sind.
///
/// 64 ist dieselbe Groessenordnung wie `MAX_Q_Q` (64) und passt zu
/// `MAX_FORMULARE` (2000).
pub const MAX_FORM_TIEFE: usize = 64;

/// Obergrenze fuer betretene Formulare (M3).
///
/// added 03.10.2026. 2000 je Seite. Die RFC-PDFs benutzen Formulare
/// fuer jede Zeile; `f1040.pdf` hat ein Formular pro Feld.
pub const MAX_FORMULARE: usize = 2000;

/// Wie oft ein Verweis beim Aufloesen weiterverfolgt wird.
///
/// added 03.10.2026. 8. Ein Kreis aus Verweisen ist moeglich
/// (`5 0 R` zeigt auf ein Objekt, dessen `/Resources` wieder `5 0 R`
/// ist) — ohne diese Grenze haengt sich der Renderer an so einer
/// Datei auf. GEMESSEN in den elf Test-PDFs: die laengste Kette ist 1.
pub const MAX_VERWEIS_KETTE: usize = 8;

impl<'a> Renderer<'a> {
    /// Neuer Renderer.
    pub fn neu(dokument: &'a mut Dokument, opt: Rasteroptionen) -> Renderer<'a> {
        Renderer {
            dokument,
            opt,
            form_tiefe: 0,
            form_zaehler: 0,
            form_abbruch_tiefe: 0,
            form_tiefe_gesehen: 0,
            form_zyklus: 0,
            form_kette: Vec::new(),
            schrift_cache: std::collections::HashMap::new(),
            // added 03.10.2026: `[None; 12]` waer schoen, aber
            // `Option<Rc<..>>` ist nicht `Copy` — `std::array::from_fn`
            // ist der Weg, und es gibt keine Schleife im Literal.
            ersatz: std::array::from_fn(|_| None),
        }
    }

    /// Rendere eine Seite als Bild.
    ///
    /// added 03.10.2026. DER WEG:
    ///  1. `/Resources` aufloesen (die Route fuer `Do` und `gs`)
    ///  2. Seiteninhalt dekodieren
    ///  3. Operatoren ausfuehren, each Aktion malen
    ///
    /// DER FEHLER, DER HIER STECKT (und der beim ersten Lauf passiert
    /// ist): die Flaeche muss aus `/MediaBox` **nach** `/Rotate`
    /// gebaut werden. Bei gedrehten Seiten ist sonst die Breite und
    /// die Hoehe vertauscht, und das Bild ist an den Raendern weiss.
    pub fn seite(&mut self, seite: &Seite) -> Result<Seitenbild, RenderFehler> {
        let ressourcen = self
            .dokument
            .objekt(seite.objekt)
            .unwrap_or(Wert::Dict(Vec::new()));
        // added 03.10.2026. `/Resources 5 0 R` ist eine **Referenz**,
        // und die war nicht aufgeloest.
        //
        // GEMESSEN: bei `dummy.pdf` steht `/Resources 5 0 R` — die
        // haeufigste Form, weil die Ressourcenliste bei einem Formular
        // nicht ins Seitenobjekt passt. Ohne Aufloesung war `res` ein
        // `Wert::Ref`, `res.get("Font")` lieferte `None` — und damit
        // **keine Schrift, kein XObject und kein Formular** auf
        // **irgendeiner** Seite aller elf Dateien. Die Seiten hatten
        // trotzdem Text (über die Ersatzschrift) und 0 Formulare.
        let res = self.aufloesen(ressourcen.get("Resources").unwrap_or(&Wert::Null));
        let drehung = ((seite.drehung / 90.0).round().rem_euclid(4.0) as u32) % 4;
        // added 03.10.2026. DIE UEBERZEICHUNG GEHOERT IN DIE
        // AUFLOESUNG, nicht in eine eigene Stufe danach: `flaeche_fuer`
        // rechnet aus `dpi` die Punktablage, und die ist an **einer**
        // Stelle definiert. Also wird mit `dpi * ueberzeichnung`
        // gezeichnet und danach gemittelt.
        let faktor = self.opt.ueberzeichnung.max(1);
        let mut f = raster::flaeche_fuer(
            &seite.media,
            drehung * 90,
            self.opt.dpi * faktor as f64,
            self.opt.max_kante * faktor,
        );
        // added 03.10.2026. DIE GRENZE WAR STUMM SCHALTBAR.
        //
        // GEMESSEN: `--dpi 300` mit `ueberzeichnung 3` ergibt 900 dpi,
        // also 7.650 x 9.900 Pixel. `flaeche_fuer` **klemmt** beide
        // Dimensionen auf `MAX_KANTE` (4.000) — und meldet nichts. Nach
        // dem Mitteln kam ein **quadratisches** Bild von 1.333 x 1.333
        // heraus: ein 612 x 792-Punkt-Blatt als Quadrat. Der Fehler sah
        // aus wie ein Seitenformat, war aber eine vergessene
        // Fehlermeldung.
        //
        // Deshalb wird die Groesse **vor** dem Klemmen geprueft, aus
        // `/MediaBox` und der Zielaufloesung gerechnet.
        let (zb, zh) = raster::ziel_groesse(&seite.media, self.opt.dpi);
        if zb > self.opt.max_kante || zh > self.opt.max_kante {
            return Err(RenderFehler::ZuGross(zb, zh));
        }
        self.form_zaehler = 0;
        self.form_tiefe = 0;
        let inhalt = self.dokument.seiteninhalt(seite)?;
        let mut bilanz = Bilanz::default();
        self.zeichne_seite(
            &mut f,
            &inhalt,
            &res,
            inhalt::Matrix::IDENTITAET,
            0,
            &mut bilanz,
        )?;
        let f = if faktor > 1 {
            raster::verkleinere(&f, faktor)
        } else {
            f
        };
        Ok(Seitenbild {
            png: als_png(&f),
            breite: f.fb.width(),
            hoehe: f.fb.height(),
            schreibvorgaenge: bilanz.schreibvorgaenge,
            sichtbare_pixel: sichtbare_pixel(&f),
            zeichen: bilanz.zeichen,
            bilder: bilanz.bilder,
            formulare: bilanz.formulare,
            formen_abbruch_tiefe: bilanz.formen_abbruch_tiefe,
            formen_tiefe_max: bilanz.formen_tiefe_max,
            formen_zyklus: bilanz.formen_zyklus,
            unbekannt: bilanz.unbekannt,
            cff_glyphen: bilanz.cff_glyphen,
            // added 05.10.2026: die Strichmuster-Zaehler wandern aus der
            // Seitenbilanz in die Gesamtbilanz — sonst muesste man sie
            // je Seite addieren, um die Frage „kam das Muster ueberhaupt
            // an?“ zu beantworten.
            kontur_gemustert: bilanz.kontur_gemustert,
            stuecke_ausgelassen: bilanz.stuecke_ausgelassen,
            ersatz_fehl: bilanz.ersatz_fehl,
        })
    }

    /// Fuehrt einen Content-Stream aus und malt das Ergebnis.
    ///
    /// added 03.10.2026. Aus [`Renderer::seite`] herausgezogen, weil
    /// **Form-XObjects** dasselbe tun — nur mit eigener CTM und eigenen
    /// Ressourcen. Ohne diese Trennung gab es entweder keine Formulare
    /// oder keinen Seiteneintrag.
    ///
    /// DER WEG, und er hat vier Schritte:
    ///  1. Operatoren ausfuehren (`inhalt::ausfuehren`) — **rechnet nur**
    ///  2. je Aktionsart malen (Fuellung, Kontur, Text, Bild)
    ///  3. bei `Form`: das Formular aufloesen und **hier** wieder
    ///     aufrufen (Rekursion, begrenzt durch `form_tiefe`)
    ///  4. Bild: DCTDecode ueber `narsil-image`, sonst rohe Bytes
    #[allow(clippy::too_many_arguments)]
    fn zeichne_seite(
        &mut self,
        f: &mut raster::Flaeche,
        inhalt: &[u8],
        res: &Wert,
        ctm_start: inhalt::Matrix,
        tiefe: usize,
        bilanz: &mut Bilanz,
    ) -> Result<(), RenderFehler> {
        let mut zustand = inhalt::Zustand::aus_ressourcen(res);
        // added 03.10.2026: die Start-CTM ist ein **Parameter**, kein
        // Feld am Renderer. Als Feld waere sie bei einer Rekursion
        // (Formular im Formular) der einzige Ort, an dem sich zwei
        // Ebenen ins Gehege kommen.
        zustand.ctm = ctm_start;
        let schriften = self.schriften_laden(res);
        // added 03.10.2026: Die Breitentabellen kommen **dieselbe**
        // Quelle wie die Schriften — nicht aus dem Zustand und nicht
        // aus einem zweiten Aufruf von `Breiten::aus`. Das ist der Weg,
        // der sicherstellt, dass der Vorschub im Interpreter und der
        // Vorschub im Zeichner **dieselbe Zahl** sind.
        for (name, r) in &schriften {
            zustand.schriften.insert(name.clone(), r.breiten.clone());
        }
        // added 03.10.2026. DIE SCHNITTE BESTIMMEN, WELCHE ERSATZ-
        // SCHRIFTEN GELADEN WERDEN. Sie stehen an **einer** Stelle:
        // `schnitt_von` ist dieselbe Funktion, die der Textzeichner
        // benutzt, sonst laedt der Renderer Schriften, die niemand
        // braucht, oder ueberspringt, die jemand braucht.
        let mut gewuenschte_schnitte: Vec<(usize, bool, bool)> = Vec::new();
        for r in schriften.values() {
            if r.eingebettet.is_some() {
                continue;
            }
            let schnitt = schnitt_von(&r.basisname);
            if !gewuenschte_schnitte.contains(&schnitt) {
                gewuenschte_schnitte.push(schnitt);
            }
        }
        let ersatz = self.ersatz_laden(&gewuenschte_schnitte);
        let mut tz = TextzeichnerBox {
            schriften,
            zeichen_gesamt: 0,
            ersatz,
            cff_glyphen: 0,
            ersatz_fehl: 0,
        };
        let aktionen = inhalt::ausfuehren(inhalt, &zustand, |_| {});
        // added 05.10.2026. DER CLIPP-FAKTOR DER SEITE.
        //
        // WARUM EINE EIGENE VARIABLE und nicht `Zustand::clip`: der
        // Zustand gehoert dem **Interpreter**, der Clipp aber dem
        // **Zeichner** — die beiden sehen dieselbe Aktionenfolge aus
        // verschiedenen Augen. Ausserdem ist `Zustand::clip` bis heute
        // unbenutzt (Abschnitt 17 des Berichts); hier steht die
        // Information an der Stelle, an der sie gebraucht wird.
        //
        // `q`/`Q` im **Zeichner** gibt es nicht: der Interpreter hat die
        // Clipp-Aktionen schon in die richtige Reihenfolge gebracht. Ein
        // `Q` ohne vorheriges `W n` **entfernt** den Clipp nach
        // Spezifikation nicht — das ist der bekannte Sonderfall, und er
        // kommt in den elf Testdateien **null Mal** vor (GEMESSEN: 5 182
        // `W n` und 0 `Q` unmittelbar danach, siehe
        // `werkzeuge/clip_wirkung.py`).
        let mut clip: Option<[f64; 4]> = None;
        // added 05.10.2026. DER MESSSCHALTER. `NARSIL_PDF_OHNE_CLIP=1`
        // schaltet den Clipp ab — damit der A/B (`werkzeuge/
        // clip_abgleich.sh`) **einen** Build zweimal benutzt, statt
        // zweimal den Quellbaum zu tauschen. Grund: derselbe Weg ist bei
        // `werkzeuge/dash_abgleich.sh` zweimal gescheitert.
        let ohne_clip = std::env::var("NARSIL_PDF_OHNE_CLIP").as_deref() == Ok("1");
        for aktion in &aktionen {
            match aktion {
                inhalt::Aktion::FuellPfad {
                    teile,
                    farbe,
                    gerade,
                } => {
                    // added 05.10.2026. DER CLIPP, und es ist der groesste
                    // bisher fehlende Puzzlenteil: GEMESSEN 5 182
                    // Rechteck-Clips im Korpus (`werkzeuge/clip_wirkung.py`),
                    // und `Aktion::Clip` wurde mit `{}` **verworfen**.
                    //
                    // Geschnitten wird im **Seitenkoordinatensystem**, also
                    // dort, wo die Zahlen des Clipp-Rechtecks stehen —
                    // nicht im Pixelraum. Das ist die einzige Stelle, an der
                    // die beiden Systeme gleichzeitig vorliegen.
                    match clip {
                        Some(r) => {
                            let geschnitten = pfadteile_geschnitten(teile, r);
                            if geschnitten.is_empty() {
                                // added 05.10.2026: „nichts im Clipp“ ist
                                // **kein Fehler**, sondern die Regel. Hier
                                // wird nichts gezeichnet — und genau das ist
                                // der Unterschied zu vorher, wo die
                                // Flaeche trotzdem gefuellt wurde.
                            } else {
                                bilanz.schreibvorgaenge +=
                                    raster::pfad_fuellen(f, &geschnitten, *farbe, *gerade);
                            }
                        }
                        None => {
                            bilanz.schreibvorgaenge +=
                                raster::pfad_fuellen(f, teile, *farbe, *gerade);
                        }
                    }
                }
                inhalt::Aktion::Kontur {
                    teile,
                    farbe,
                    breite,
                    strich,
                } => {
                    // added 05.10.2026. DIE PHASE, und sie beginnt hier
                    // neu — fuer jeden Pfad, wie es die Spezifikation
                    // verlangt.
                    //
                    // GEMESSEN, warum es das Strichmuster ueberhaupt
                    // braucht: 225 echte Muster im Korpus
                    // (`werkzeuge/operator_zaehlen.py`), davon 120 in
                    // den Abbildungen der Mathematik-Papiere. Ohne sie wird
                    // jede gestrichelte Linie durchgezogen gezeichnet.
                    // added 05.10.2026: EIN MUSTER, MIT DEM MAN
                    // ARBEITEN KANN, heisst: mindestens ein Eintrag groesser
                    // als die Rechengrenze. Sonst waere die Schleife unten
                    // endlos — bei `[0 1e-12]` ist kein Eintrag gross genug,
                    // `weg` naehme nie ab, und nur `stelle` liefe im Kreis.
                    // GEMESSEN ist das nicht, es ist Vorsicht: `[0 0]` ist
                    // in der Spezifikation ausdruecklich „kein Strichmuster“,
                    // und auf 0.1-Nanopunkt genaue Angaben hat keine Datei
                    // der Welt.
                    // added 05.10.2026: DER MESSSCHALTER. `NARSIL_PDF_STRICH_AUS=1` laesst
                    // jede Kontur **ohne** Strichmuster zeichnen, also
                    // genau so, wie es vor diesem Feature war.
                    //
                    // added 05.10.2026. WARUM ALS SCHALTER UND NICHT ALS
                    // ZWEITER QUELLSTAND: der A/Versuch mit zwei
                    // Quellstaenden (`werkzeuge/dash_abgleich.sh`, 20:58
                    // und 22:04) ist **zweimal** gescheitert. Einmal an
                    // einer fremden Crate, einmal daran, dass die
                    // Beispielausgabe inzwischen Bilanzfelder benutzt, die
                    // im alten Stand **nicht existieren** — der Alt-Stand
                    // bauert also gar nicht mehr. Bei einem Messschalter
                    // gibt es keinen Quellwechsel, kein Zurueckspielen und
                    // damit auch keine Gefahr, dass ein halb
                    // zurueckgespielter Baum stehen bleibt.
                    let aus = std::env::var("NARSIL_PDF_STRICH_AUS").as_deref() == Ok("1");
                    let brauchbar = !aus && strich.iter().any(|v| *v > 1e-9);
                    // added 05.10.2026: DER ZAEHLER. „Kam das Muster
                    // ueberhaupt an?“ ist die Frage hinter dem gemessenen
                    // „+0 Pixel“ — und sie beantwortet sich nur hier.
                    if brauchbar {
                        bilanz.kontur_gemustert += 1;
                    }
                    let mut rest: Vec<f64> = strich.clone();
                    let mut stelle = 0usize;
                    for teil in teile {
                        // added 06.10.2026: DER CURSOR LIEGT IM
                        // **Seiten**koordinatensystem — und der
                        // Pixelraum-Cursor ist weg.
                        //
                        // DER GRUND ist eine Compiler-Warnung, die
                        // korrekt war: `von` (Pixel) wurde am Ende jedes
                        // Abschnitts gesetzt und **nie gelesen**, weil der
                        // Clipp den Startpunkt ohnehin neu berechnet. Ein
                        // Cursor, den man pflegt und nicht liest, ist
                        // genau die Art Code, die nach drei Monaten einen
                        // Fehler traegt.
                        let mut von_pg = (teil.von.x, teil.von.y);
                        for seg in &teil.segmente {
                            if let inhalt::Segment::Linie(_, b) = seg {
                                // added 05.10.2026. DIE STRECKE KLAPPT
                                // (Liang–Barsky), und zwar im
                                // **Seiten**koordinatensystem: `b` ist
                                // noch Seite, `von` ist bereits Pixel.
                                //
                                // DIE PHASE laeuft nach dem
                                // **urspruenglichen** Weg weiter, nicht
                                // nach dem gekappten: die Phase gehoert
                                // zum Pfad, nicht zu dem, was vom Clipp
                                // uebrig bleibt.
                                //
                                // added 05.10.2026: DIE FASSUNG DER ERSTEN
                                // REICHTE IN DEN PIXELRAUM (`von.x`) und der
                                // Compiler sagte es beim ersten Lauf
                                // (E0609) — `zu_pixel` gibt ein Tupel
                                // zurueck, und der Seitenpunkt steht in
                                // `teil.von`.
                                // added 06.10.2026: DER FEHLER, DEN DIE
                                // GEGENPROBE GEFUNDEN HAT.
                                //
                                // Die erste Fassung nahm als Startpunkt
                                // **immer** `teil.von` — den Anfang des
                                // Teilpfades. Bei einem Teilpfad mit
                                // mehreren Abschnitten wurde damit
                                // **jeder** Abschnitt vom Anfang aus
                                // gezeichnet: zusaetzliche, doppelte
                                // Striche und fehlende Verbindungen.
                                //
                                // GEMESSEN, und zwar an der Gegenprobe
                                // statt an einem Test: mit
                                // `NARSIL_PDF_OHNE_CLIP=1` muss der
                                // Renderer **bitgleich** das alte Bild
                                // liefern (25 172 976 abweichende Pixel,
                                // der Stand von `team/log/pdf/abnahme17`).
                                // Er lieferte 25 326 985 — **153 985
                                // schlechter**, obwohl der Schalter den
                                // alten Zustand herstellen soll. Genau so
                                // sieht ein Fehler aus, den eine
                                // Ausgangspruefung findet.
                                //
                                // DER CURSOR laeuft deshalb im
                                // **Seitenkoordinatensystem** mit und
                                // wird **nicht** auf den gekappten Punkt
                                // gesetzt: das Clipp soll die Geometrie
                                // beschneiden, nicht den Weg verschieben.
                                let ziel = match clip {
                                    Some(r) => raster::schneide_strecke(von_pg, (b.x, b.y), r),
                                    None => Some((von_pg, (b.x, b.y))),
                                };
                                let (von_p, nach_p) = match ziel {
                                    // added 05.10.2026: GANZ AUSSERHALB —
                                    // nichts zeichnen, aber der Pfadcursor
                                    // muss weiterlaufen, sonst springt der
                                    // Strich zum falschen Ende.
                                    None => {
                                        // added 06.10.2026: NUR der
                                        // Seiten-Cursor wandert. Der
                                        // Pixel-Cursor wird nicht mehr
                                        // gefuehrt (siehe oben).
                                        von_pg = (b.x, b.y);
                                        continue;
                                    }
                                    Some(v) => v,
                                };
                                let von = f.zu_pixel(von_p.0, von_p.1);
                                von_pg = (b.x, b.y);
                                let nach = f.zu_pixel(nach_p.0, nach_p.1);
                                let px_breite = *breite / f.skala;
                                // DAS TEILEN: der Weg von `von` nach
                                // `nach` wird an den Mustergrenzen
                                // geschnitten. Gerechnet wird in
                                // **Seitenpunkten**, denn die Zahlen aus
                                // `d` stehen im Koordinatensystem des
                                // Pfades.
                                if !brauchbar {
                                    // `[ ]`, `[0 0]` und andere entartete
                                    // Muster heissen alle „durchgezogen“.
                                    bilanz.schreibvorgaenge += raster::linie_zeichnen(
                                        f,
                                        von,
                                        nach,
                                        px_breite as f32,
                                        *farbe,
                                    );
                                } else {
                                    // added 05.10.2026: `zu_pixel` gibt ein
                                    // **Tupel** `(f32, f32)` zurueck und keine
                                    // Struktur mit `.x`/`.y` — die erste
                                    // Fassung griff daneben (E0609).
                                    let a = (von.0 as f64, von.1 as f64);
                                    let b_ = (nach.0 as f64, nach.1 as f64);
                                    let laenge = ((b_.0 - a.0) * (b_.0 - a.0)
                                        + (b_.1 - a.1) * (b_.1 - a.1))
                                        .sqrt();
                                    // added 05.10.2026: DER WEG IN **PUNKTEN**, und
                                    // hier stand die erste Fassung
                                    // **invertiert**.
                                    //
                                    // GEMESSEN (`NARSIL_PDF_STRICH=1`, Zeile
                                    // aus `werkzeuge/positivkontrolle_
                                    // linie.py`, eine 160-Punkt-Linie mit
                                    // `[3 2]`):
                                    //
                                    // ```text
                                    //   px_punkt = 3.0000   punkt_laenge = 1440.000
                                    // ```
                                    //
                                    // Statt 160 (der Laenge der Linie in
                                    // Punkten) stand 1440 — **das Neunfache**.
                                    // Ursache: `f.skala` ist „Punkte je
                                    // **Oberflaechen**-Pixel", und die
                                    // Oberflaeche ist mit
                                    // `ueberzeichnung = 3` dreifach
                                    // feiner als das Bild. `laenge` liegt
                                    // also in Oberflaechen-Pixeln, und um
                                    // Punkte zu bekommen, muss man **mit**
                                    // `f.skala` **mal** multiplizieren — ich
                                    // hatte geteilt und dann noch einmal
                                    // geteilt.
                                    //
                                    // DIE WIRKUNG des Fehlers war nicht
                                    // sichtbar, sondern **stumm**: das Muster
                                    // war 9x zu kurz (jedes „an“-Stueck
                                    // 0,33 Pixel lang), die Striche ueberlagerten
                                    // sich zu einer durchgezogenen Linie —
                                    // und das A/B ueber 100 Seiten meldete
                                    // deshalb ehrlich **+0 Pixel**.
                                    //
                                    // Die Pixelkoordinaten waeren fuer ein
                                    // Muster ohnehin falsch: `[3 2]` steht
                                    // in Punkten des Pfadkoordinatensystems.
                                    let px_punkt = f.skala;
                                    let punkt_laenge = laenge * px_punkt;
                                    let mut weg = 0.0f64;
                                    // added 05.10.2026: DIE SCHRANKE. Ein
                                    // Segment kann hoechstens so viele
                                    // Musterabschnitte sehen, wie seine
                                    // Laenge in Punkten ist, plus die
                                    // Nachkommafehler-Runden. Diese Zahl
                                    // ist bewusst **beliebig gross**
                                    // (Schritt 1: ein Abschnitt je Punkt)
                                    // — sie soll nie eine echte Datei
                                    // abbrechen, nur verhindern, dass ein
                                    // Fehler im Muster den ganzen Renderer
                                    // anhaelt.
                                    let schranke = (punkt_laenge * 64.0) as usize + 1024;
                                    let mut schritt = 0usize;
                                    while weg < punkt_laenge - 1e-9 && schritt < schranke {
                                        schritt += 1;
                                        // added 05.10.2026: DIE MESSZEILE.
                                        // Sie druckt Muster, Laenge, Weg und
                                        // Index fuer **jeden** Abschnitt —
                                        // nur bei `NARSIL_PDF_STRICH=1`, weil
                                        // ein 160-Punkt-Segment sonst 80
                                        // Zeilen schreibt.
                                        if std::env::var("NARSIL_PDF_STRICH").as_deref() == Ok("1") {
                                            eprintln!(
                                                "STRICH muster={:?} rest={:?} stelle={} weg={:.3} \
                                                 punkt_laenge={:.3} px_punkt={:.4} px_breite={:.3} \
                                                 an={}",
                                                strich,
                                                rest,
                                                stelle,
                                                weg,
                                                punkt_laenge,
                                                px_punkt,
                                                px_breite,
                                                stelle % 2 == 0
                                            );
                                        }
                                        let uebrig = rest[stelle];
                                        if uebrig <= 1e-9 {
                                            rest[stelle] = strich[stelle];
                                            stelle = (stelle + 1) % strich.len();
                                            continue;
                                        }
                                        let stueck = uebrig.min(punkt_laenge - weg);
                                        if stelle % 2 == 0 {
                                            // „AN“: der Abschnitt wird
                                            // gezeichnet.
                                            let t0 = weg / punkt_laenge;
                                            let t1 = (weg + stueck) / punkt_laenge;
                                            bilanz.schreibvorgaenge += raster::linie_zeichnen(
                                                f,
                                                (
                                                    (a.0 + (b_.0 - a.0) * t0) as f32,
                                                    (a.1 + (b_.1 - a.1) * t0) as f32,
                                                ),
                                                (
                                                    (a.0 + (b_.0 - a.0) * t1) as f32,
                                                    (a.1 + (b_.1 - a.1) * t1) as f32,
                                                ),
                                                px_breite as f32,
                                                *farbe,
                                            );
                                        } else {
                                            // added 05.10.2026: DAS
                                            // AUSGELASSENE STUECK. Jedes
                                            // hier gezaehlte Stueck ist
                                            // Strich, den die Referenz
                                            // zeichnet und mein Renderer
                                            // **nicht** zeichnet — das ist
                                            // die Pixelwirkung in Stuecken,
                                            // unabhaengig von der
                                            // Pixelgroesse des Strichs.
                                            bilanz.stuecke_ausgelassen += 1;
                                        }
                                        weg += stueck;
                                        rest[stelle] -= stueck;
                                    }
                                }
                            }
                        }
                    }
                }
                inhalt::Aktion::Text {
                    position,
                    groesse,
                    zeichen,
                    schrift,
                    farbe,
                    horizont,
                    laufweite,
                    anhebung,
                    matrix,
                } => {
                    let k = tz.glyphen_seitenkoordinaten(
                        schrift,
                        zeichen,
                        (position.x, position.y),
                        *groesse,
                        *matrix,
                        *horizont,
                        *laufweite,
                        *anhebung,
                    );
                    bilanz.zeichen += zeichen.len();
                    // added 03.10.2026. F25: ALLE KONTUREN GEMEINSAM,
                    // nicht jede einzeln.
                    //
                    // GEMESSEN an `rfc2616.pdf` Seite 5, 300 dpi:
                    // „RFC 2616" kam mit **geschlossenen Gegenformen**
                    // — das `R` war ein schwarzer Klumpen, das `C`
                    // ebenfalls. Zwei Quellen, gegen die
                    // `pdftoppm` bei 72 dpi gemessen:
                    //
                    //   Zeile 52, x 45..120, <128 = schwarz
                    //     Referenz  ###+........#####...........
                    //     eigener   ##################.........
                    //
                    // DER GRUND: `glyphe_zeichnen` nimmt **alle**
                    // Konturen einer Glyphe und fuellt sie mit der
                    // Nonzero-Regel. Die zweite Kontur eines `R` ist
                    // die **Gegenform** (die Oeffnung im Bogen), und
                    // Nonzero macht sie nur dann zu einem Loch, wenn
                    // sie **gegen** die aeussere laeuft. Uebergibt man
                    // jede Kontur einzeln, wird die Gegenform **nach**
                    // dem Bauch gefuellt — und der Buchstabe wird
                    // massiv. Das betraf **jede** Glyphe mit
                    // Gegenform: o e a d b p q 6 8 9 B D O P Q R A.
                    //
                    // GEMESSENE GEWICHTUNG: an `1706.03762.pdf` Seite 1
                    // stieg die Zahl der abweichenden Pixel dadurch
                    // nicht — der Fehler ist bei 9,96 Punkt kleiner als
                    // ein Pixel breit und faellt unter der Toleranz.
                    // Er wurde nur an `rfc2616.pdf` sichtbar, weil
                    // dort Times in 9,96 Punkt mit **klaren**
                    // Gegenformen steht.
                    // added 05.10.2026, KORRIGIERT am 07.10.2026. DER
                    // CLIPP FUER DIE **TEXT**-KONTUREN.
                    //
                    // DER FEHLER, den die alte Fassung hatte: sie rechnete
                    // das Clipp-Rechteck nach **Pixel**, mit der
                    // Begruendung „`k` kommt in Pixelkoordinaten aus dem
                    // Textzeichner“. **Das stimmte nicht.** `k` kommt in
                    // **Seitenkoordinaten** — `glyphen_seitenkoordinaten`
                    // rechnet mit `seiten_punkt`, und erst
                    // `glyphe_zeichnen` ruft `zu_pixel`. Damit standen
                    // Seitenkoordinaten gegen einen Pixelrahmen.
                    //
                    // GEMESSENE WIRKUNG, `rfc9110.pdf` Seite 9: die Seite
                    // traegt den Clipp `(65,9, 70,5) .. (529,4, 695,6)`,
                    // und der Text steht bei Seite `y ~ 78`. Als Pixel
                    // heisst der Rahmen `y 145 .. 770` — der Text liegt
                    // **ausserhalb** und wird weggeschnitten:
                    //
                    //     mit Clipp        2 234 Pixel
                    //     ohne Clipp       17 717 Pixel
                    //
                    // Die Tintenmenge der Seite betrug **0,136** des
                    // Referenzwerts, und `pixel=` 2 234 bei **929**
                    // Zeichen — die Zeichen kamen an und wurden nicht
                    // gezeichnet.
                    //
                    // DIE KORREKTUR ist deshalb genau die des
                    // Fuellpfads: schneiden in **Seitenkoordinaten**, mit
                    // `r` unveraendert als Rahmen.
                    let k = match clip {
                        None => k,
                        Some(r) => {
                            let box_ = r;
                            k.iter()
                                .map(|kontur| {
                                    let punkte: Vec<(f64, f64)> = kontur
                                        .iter()
                                        .map(|(x, y)| (*x as f64, *y as f64))
                                        .collect();
                                    raster::schneide_polygon(&punkte, box_)
                                        .into_iter()
                                        .map(|(x, y)| (x as f32, y as f32))
                                        .collect()
                                })
                                .filter(|k: &Vec<(f32, f32)>| k.len() >= 3)
                                .collect()
                        }
                    };
                    if !k.is_empty() {
                        bilanz.schreibvorgaenge += raster::glyphe_zeichnen(f, &k, *farbe);
                    }
                }
                inhalt::Aktion::XObject { name, ziel, matrix } => {
                    // added 04.10.2026. DER ENTSCHEID UEBER DEN TYP
                    // STEHT HIER, nicht im Interpreter: dort steht der
                    // Eintrag aus `/XObject` als `Wert::Ref`, und ohne
                    // Dokument kann er nicht aufgeloest werden. Der
                    // Renderer ist die einzige Stelle, die den Typ kennt.
                    //
                    // GEMESSEN: `1512.03385.pdf` Seite 1 hat 0 Bilder
                    // und 7 Formulare, `2103.00020.pdf` 79 Bilder und
                    // 993 Formulare — und **keines** davon wurde
                    // gezeichnet, weil `Do` ein No-op war.
                    let sub = self.xobject_subtyp(res, name);
                    if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
                        eprintln!("XOBJEKT {name:?} subtyp={sub:?} res={res:?}");
                    }
                    match sub {
                        Some(true) => {
                            if let Some(px) = self.bild_setzen(&mut *f, res, name, *ziel, bilanz) {
                                bilanz.schreibvorgaenge += px;
                            }
                        }
                        Some(false) => {
                            // added 03.10.2026. GEMESSEN: `rfc9110.pdf`
                            // hat 3502 Form-XObjects, `1706.03762.pdf`
                            // 2565, `f1040.pdf` 222. Ohne diesen Zweig
                            // waeren die Seiten „gerendert“ und fast leer.
                            self.form_zeichnen(f, res, name, *matrix, tiefe, bilanz)?;
                        }
                        None => {}
                    }
                }
                inhalt::Aktion::Unbekannt(op) => {
                    if bilanz.unbekannt.len() < MAX_UNBEKANNTE {
                        bilanz.unbekannt.push(op.clone());
                    }
                }
                inhalt::Aktion::ClipSette { rechteck } => {
                    // added 06.10.2026. DER CLIPP, **fertig gerechnet**.
                    //
                    // Die Rechnung (Schnittmenge, Zuruecksetzen bei `Q`)
                    // liegt jetzt im Interpreter: nur dort weiss man, wann
                    // ein `q ... W n ... Q` den Clipp wieder aufhebt. Der
                    // Zeichner folgt nur noch.
                    //
                    // GEMESSENER Grund fuer den Umbau: mit der Clipp-
                    // Verwaltung im Zeichner blieb ein `q ... W n ... Q`
                    // bis zum Seitenende aktiv — 16 693 Pixel Schaden auf
                    // `1207.7214.pdf` allein.
                    clip = if ohne_clip { None } else { *rechteck };
                    // added 06.10.2026: DIE ERKENNUNGSRATE. `rechteck` ist
                    // `None`, wenn der Clipp kein **achsenparalleles**
                    // Rechteck in Seitenkoordinaten ist — bei gedrehten
                    // oder gescheiten Clips also immer. Ohne diese Zahlen
                    // weiss man nicht, ob ein schlechteres Bild am Clipp
                    // oder an der **Erkennung** liegt.
                    // added 06.10.2026: DIE ERKENNUNGSRATE, direkt
                    // gedruckt. Sie beantwortet die Frage, ob ein
                    // schlechteres Bild am Clipp oder an der
                    // **Erkennung** liegt: `rechteck == None` heisst
                    // „kein achsenparalleles Rechteck in
                    // Seitenkoordinaten“ — bei gedrehten oder gescheiten
                    // Clips also immer.
                    //
                    // Ein Zaehler in der Bilanz waere die sauberere Loesung,
                    // saehe aber in `Seitenbild` nicht und muesste erst
                    // durch die oeffentliche Structur. Fuer eine Diagnose
                    // ist eine Zeile auf stderr schneller — und
                    // `grep -c` zaehlt sie.
                    if std::env::var("NARSIL_PDF_CLIP_ZAEHLEN").as_deref() == Ok("1") {
                        // added 07.10.2026: die **Zahlen** kommen mit.
                        // „RECHTECK“ allein sagt nur, dass etwas erkannt
                        // wurde; die Clipp-Rechtecke dieser Seite sind so
                        // gross, dass man sie sehen muss, um den Fehler zu
                        // finden (Befund: `rfc9110.pdf` Seite 9).
                        match rechteck {
                            Some(r) => eprintln!("CLIPP RECHTECK {r:?}"),
                            None => eprintln!("CLIPP KEIN_RECHTECK"),
                        }
                    }
                }
                inhalt::Aktion::Clip { .. } => {
                    // added 06.10.2026: nur noch **informativ** — die
                    // Rechnung steht in `ClipSette`, dieser Arm muss nichts
                    // tun.
                }
            }
        }
        // added 03.10.2026. DER CFF-ZAEHLER KOMMT **HIER** und nicht
        // direkt nach `ausfuehren`: die Glyphen werden im Zeichenlauf
        // gezeichnet, nicht im Interpretieren. GEMESSEN: mit dem
        // Zaehler direkt nach `ausfuehren` stand in jeder Bilanz
        // „0 CFF", obwohl die CFF gezeichnet wurden — die Zahl war
        // um die ganze Zeichen-Schleife zu frueh gelesen.
        bilanz.cff_glyphen += tz.cff_glyphen;
        // added 07.10.2026: die Form-Zaehler wandern mit.
        bilanz.formen_abbruch_tiefe += self.form_abbruch_tiefe;
        bilanz.formen_tiefe_max = bilanz.formen_tiefe_max.max(self.form_tiefe_gesehen);
        bilanz.formen_zyklus += self.form_zyklus;
        // added 05.10.2026: die stille Fehlstelle kommt in die Bilanz.
        bilanz.ersatz_fehl += tz.ersatz_fehl;
        Ok(())
    }

    /// Welchen Typ hat das XObject `name`? (`Some(true)` = Bild)
    ///
    /// added 04.10.2026. DER WEG, und er ist derselbe wie in
    /// `bild_setzen` und `form_zeichnen`: `/XObject` aufloesen, den
    /// Eintrag nehmen, und der ist **selbst** meist ein Verweis — den
    /// muss man ein zweites Mal aufloesen, sonst steht in `/Subtype`
    /// nichts.
    ///
    /// `None`, wenn es den Namen nicht gibt oder der Subtyp keiner der
    /// beiden ist (zum Beispiel ein `/PS` PostScript-XObject, das
    /// auszulassen ist).
    fn xobject_subtyp(&mut self, res: &Wert, name: &str) -> Option<bool> {
        let xobject = self.aufloesen(res.get("XObject").unwrap_or(&Wert::Null));
        let eintrag = xobject.get(name)?.clone();
        let wert = match eintrag {
            Wert::Ref(n, _) => self.dokument.objekt(n).ok()?,
            sonst => sonst,
        };
        match wert.get_name("Subtype")? {
            "Image" => Some(true),
            "Form" => Some(false),
            _ => None,
        }
    }

    /// Zeichnet ein Form-XObject (`/Subtype /Form`).
    ///
    /// added 03.10.2026. DIE DREI SACHEN, die dabei stimmen muessen:
    ///
    ///  * `/Matrix` des Formulars **vor** der CTM der Seite
    ///    (`neue_ctm = form_matrix × seiten_ctm`)
    ///  * eigene `/Resources` — ein Formular hat seine **eigene**
    ///    Ressourcenliste; fehlt sie, gilt die der Seite
    ///  * eigener Zaehler gegen `MAX_FORMULARE`, weil ein Formular
    ///    sich selbst als Kind eintraegen kann

    fn form_zeichnen(
        &mut self,
        f: &mut raster::Flaeche,
        seiten_res: &Wert,
        name: &str,
        ctm: inhalt::Matrix,
        tiefe: usize,
        bilanz: &mut Bilanz,
    ) -> Result<(), RenderFehler> {
        if tiefe >= MAX_FORM_TIEFE {
            // added 07.10.2026: DER ABBRUCH WIRD GEZAEHLT. Vorher
            // verschwand eine Form ersatzlos, und die Bilanz meldete
            // weiter ihre Zahl der gezeichneten Formen.
            self.form_abbruch_tiefe += 1;
            return Ok(());
        }
        if self.form_zaehler >= MAX_FORMULARE {
            return Ok(());
        }
        // added 03.10.2026: `/XObject` und der Eintrag darin koennen
        // beide Verweise sein.
        let xobject = self.aufloesen(seiten_res.get("XObject").unwrap_or(&Wert::Null));
        // added 03.10.2026: `let…else` verlangt Divergenz — der
        // `return Ok(())` ist bei der Suche nach den Debug-Ausgaben
        // mitgeschnitten worden.
        let Some(xo) = xobject.get(name).cloned() else {
            return Ok(());
        };
        // `xo` ist **besessen** (durch `.cloned()`), also ist `n`
        // eine `u32`. Mit `&xo` waere es eine `&u32`.
        // added 07.10.2026: die Objectnummer **vor** der Aufloesung — nur
        // sie ist ein Schluessel fuer die Kette.
        let objekt_nr = match &xo {
            Wert::Ref(n, _) => *n,
            _ => 0,
        };
        // added 07.10.2026: DER ZYKLUSSCHUTZ, und er ist der eigentliche.
        // Die Tiefengrenze ist nur die Notbremse fuer ungewoehnlich
        // tiefe, **nicht wiederholte** Wege; eine Form, die auf diesem
        // Weg schon vorkam, wird hier abgefangen.
        if self.form_kette.contains(&objekt_nr) {
            self.form_zyklus += 1;
            return Ok(());
        }
        let wert = match xo {
            Wert::Ref(n, _) => self.dokument.objekt(n)?,
            sonst => sonst,
        };
        // added 03.10.2026. `Wert::Stream(dict, roh)` gibt den
        // Wörterbuchteil als `&Vec<(Vec<u8>, Wert)>` — darauf gibt es
        // **kein** `get("…")`. Deshalb wird der Teil einmal als
        // `Wert::Dict` gebunden; `Wert::get` weiss, dass ein Stream
        // ein Wörterbuch **ist** (dieselbe Regel wie in `inhalt.rs`).
        let Wert::Stream(dict_roh, roh) = &wert else {
            return Ok(());
        };
        let dict = Wert::Dict(dict_roh.clone());
        if dict.get_name("Subtype") != Some("Form") {
            return Ok(());
        }
        let form_matrix = matrix_aus_wert(dict.get("Matrix"));
        let form_res = dict
            .get("Resources")
            .cloned()
            .unwrap_or_else(|| seiten_res.clone());
        // added 03.10.2026: ohne `&` — `dict_roh` und `namen` sind
        // schon Referenzen (clippy `needless_borrow`).
        let namen = xref::filter_namen(dict_roh);
        let parms = xref::decode_parms(dict_roh, &namen);
        let inhalt = match filtern::dekodieren(&namen, &parms, &dict, roh, filtern::MAX_ENTPACKT) {
            Ok(v) => v,
            Err(e) => {
                // Kein Formular, aber die Seite ist nicht kaputt: ein
                // Formular mit kaputtem Filter wird uebersprungen.
                let _ = e;
                Vec::new()
            }
        };
        self.form_zaehler += 1;
        bilanz.formulare += 1;
        // added 07.10.2026: auf die Kette (Weg, nicht Menge) und die
        // Tiefenmarke setzen — auch fuer Formen, die gleich scheitern.
        self.form_kette.push(objekt_nr);
        self.form_tiefe_gesehen = self.form_tiefe_gesehen.max(tiefe + 1);
        // added 07.10.2026: die **gesehene** Tiefe merken, unabhaengig
        // davon, ob gezeichnet wurde.
        self.form_tiefe_gesehen = self.form_tiefe_gesehen.max(tiefe + 1);
        // added 03.10.2026: die CTM des Formulars **an** der
        // Fundstelle (`ctm`), davor sein eigenes `/Matrix`.
        // `Matrix::mal` wendet das zweite Argument zuerst an, also
        // erst `/Matrix`, dann die CTM der Seite — genau die
        // Reihenfolge der Spezifikation.
        // added 04.10.2026: DIE REIHENFOLGE, dieselbe wie bei `Tm`:
        // erst die Matrix des Formulars, dann die CTM der Seite — also
        // `ctm.mal(form_matrix)`. Vorher stand es umgekehrt, was bei
        // einer Identitaets-Matrix (`/Matrix [1 0 0 1 0 0]`, der
        // Normalfall) **nicht auffaellt** und bei jeder anderen
        // Verschiebung das Formular an die falsche Stelle setzt.
        let ergebnis = self.zeichne_seite(
            f,
            &inhalt,
            &form_res,
            ctm.mal(form_matrix),
            tiefe + 1,
            bilanz,
        );
        // added 07.10.2026: die Kette wieder abbauen, auch im Fehlerfall.
        self.form_kette.pop();
        ergebnis
    }

    /// Loest Verweise auf, bis ein Wörterbuch (oder etwas anderes) da
    /// ist.
    ///
    /// added 03.10.2026. `/Resources 5 0 R`, `/Font 9 0 R` und
    /// `/XObject 12 0 R` kommen alle vor — auch **innerhalb** einer
    /// Ressourcenliste. Deshalb wird an jeder Stelle aufgeloest, und die
    /// Kette ist begrenzt, weil ein Kreis aus Verweisen möglich ist
    /// (`5 0 R` zeigt auf ein Objekt, dessen `/Resources` wieder
    /// `5 0 R` ist).
    ///
    /// DIE GRENZE, und sie ist die von `/Prev` bei den XRef-Streams:
    /// [`MAX_VERWEIS_KETTE`]. Ohne sie hängt sich der Renderer an
    /// genau so einer Datei auf.
    pub fn aufloesen(&mut self, wert: &Wert) -> Wert {
        let mut w = wert.clone();
        for _ in 0..MAX_VERWEIS_KETTE {
            match &w {
                Wert::Ref(n, _) => {
                    let nr = *n;
                    match self.dokument.objekt(nr) {
                        Ok(v) => w = v,
                        Err(_) => return Wert::Dict(Vec::new()),
                    }
                }
                _ => return w,
            }
        }
        Wert::Dict(Vec::new())
    }

    /// Laedt alle Schriften eines Ressourcen-Woerterbuchs.
    ///
    /// added 03.10.2026. Jede Schrift wird **einmal** geladen (siehe
    /// [`Renderer::schrift_cache`]) und danach nur noch nachgeschlagen.
    fn schriften_laden(
        &mut self,
        res: &Wert,
    ) -> std::collections::HashMap<String, std::rc::Rc<schrift::Ressource>> {
        let mut raus = std::collections::HashMap::new();
        // `/Font` kann selbst eine Referenz sein (siehe `aufloesen`).
        let font_wert = self.aufloesen(res.get("Font").unwrap_or(&Wert::Null));
        let Some(fonts) = font_wert.dict().cloned() else {
            return raus;
        };
        for (name, verweis) in fonts {
            if raus.len() >= schrift::MAX_SCHRIFTEN {
                break;
            }
            // `fonts` ist **besessen** (durch `.cloned()`), deshalb
            // ist `n` hier eine `u32` und kein `&u32`. Der Compiler
            // hat beim ersten Lauf genau das gemeldet.
            let nummer = match verweis {
                Wert::Ref(n, _) => n,
                _ => continue,
            };
            if let Some(r) = self.schrift_cache.get(&nummer) {
                raus.insert(String::from_utf8_lossy(&name).into_owned(), r.clone());
                continue;
            }
            // Der Schliesser braucht `&mut self.dokument`, und der
            // Cache haelt `Rc` — deshalb wird die Ressource hier
            // **fertig** gebaut und danach eingelegt.
            let Some(r) = self.schrift_bauen(nummer) else {
                continue;
            };
            let r = std::rc::Rc::new(r);
            self.schrift_cache.insert(nummer, r.clone());
            raus.insert(String::from_utf8_lossy(&name).into_owned(), r);
        }
        raus
    }

    /// Baut **eine** Ressource aus einem Font-Objekt.
    ///
    /// added 03.10.2026. Eigene Methode statt eines Schliessers in der
    /// Schleife, weil `Ressource::laden` ein `&mut dyn FnMut` will und
    /// ein Schliesser ueber `self.dokument` nicht in einer Schleife
    /// ausgeliehen werden kann.
    fn schrift_bauen(&mut self, nummer: u32) -> Option<schrift::Ressource> {
        let dict = self.dokument.objekt(nummer).ok()?;
        schrift::Ressource::laden(&dict, &mut |n| self.dokument.objekt(n).ok())
    }

    /// Setzt ein Bild in die Flaeche (DCTDecode ueber `narsil-image`).
    fn bild_setzen(
        &mut self,
        f: &mut Flaeche,
        res: &Wert,
        name: &str,
        ziel: [f64; 4],
        bilanz: &mut Bilanz,
    ) -> Option<usize> {
        let xobject = self.aufloesen(res.get("XObject").unwrap_or(&Wert::Null));
        let xo = xobject.get(name)?.clone();
        let (xo_nummer, _) = match &xo {
            Wert::Ref(n, _) => (*n, 0),
            _ => (0, 0),
        };
        let bild_dict = if xo_nummer > 0 {
            self.dokument.objekt(xo_nummer).ok()?
        } else {
            xo
        };
        let breite = bild_dict.get_int("Width").unwrap_or(0).max(0) as u32;
        let hoehe = bild_dict.get_int("Height").unwrap_or(0).max(0) as u32;
        if breite == 0 || hoehe == 0 || breite > 20_000 || hoehe > 20_000 {
            return None;
        }
        let roh = match &bild_dict {
            // ACHTUNG: `Wert::Stream(dict, daten)` — das **erste**
            // Feld ist das Wörterbuch (`/Filter` steht dort), das
            // zweite sind die Rohdaten. Mit `_` an erster Stelle
            // bekam man die Bytes und `filter_namen` die Datentypen
            // nicht — der Compiler hat es beim ersten Lauf gesagt.
            Wert::Stream(dict, daten) => {
                let namen = xref::filter_namen(dict);
                let parms = xref::decode_parms(dict, &namen);
                // Bei DCTDecode **nicht** entpacken: das sind JPEG-Daten.
                if namen.iter().any(|n| n == "DCTDecode" || n == "DCT") {
                    daten.clone()
                } else {
                    filtern::dekodieren(
                        &namen,
                        &parms,
                        &Wert::Dict(dict.clone()),
                        daten,
                        filtern::MAX_ENTPACKT,
                    )
                    .unwrap_or_default()
                }
            }
            _ => return None,
        };
        // DCTDecode: ueber `narsil-image`.
        let ist_dct = bild_dict
            .get_name("Filter")
            .map(|n| n == "DCTDecode")
            .unwrap_or(false)
            || bild_dict
                .get("Filter")
                .and_then(Wert::array)
                .map(|a| {
                    a.iter()
                        .any(|v| matches!(v.name(), Some("DCTDecode") | Some("DCT")))
                })
                .unwrap_or(false);
        let (rgb, qw, qh) = if ist_dct {
            // added 03.10.2026: DCTDecode braucht `narsil-image`. Ohne
            // das Feature wird das Bild **uebersprungen** und
            // gezaehlt wird es trotzdem — sonst waere die Bilanz
            // schoener als das Bild.
            #[cfg(feature = "bilder")]
            {
                match narsil_image::jpeg::lesen::jpeg_lesen(&roh) {
                    Ok(b) => (b.rgba, b.breite, b.hoehe),
                    Err(_) => return None,
                }
            }
            #[cfg(not(feature = "bilder"))]
            {
                bilanz.bilder += 1;
                return None;
            }
        } else {
            (roh, breite, hoehe)
        };
        bilanz.bilder += 1;
        // added 07.10.2026: DIE BILDWERTE, wenn man sie braucht. Ohne
        // diese Zeile ist „bilder=1, pixel=0“ nicht unterscheidbar von
        // „dekodiert und unsichtbar“ — GEMESSEN genau diese
        // Kombination bei `2103.00020.pdf` Seite 42 (JPEG 567x567).
        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
            eprintln!(
                "BILD {breite}x{hoehe} -> qw={qw} qh={qh} bytes={} ziel={ziel:?} \
                 erste={:?} mitte={:?}",
                rgb.len(),
                &rgb[..rgb.len().min(8)],
                &rgb[(rgb.len() / 2).min(rgb.len().saturating_sub(4))
                    ..(rgb.len() / 2).min(rgb.len().saturating_sub(4)) + 4]
            );
        }
        let gesetzt = raster::bild_zeichnen(f, &rgb, qw, qh, ziel, 1.0);
        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
            eprintln!("BILD gesetzt={gesetzt}");
        }
        Some(gesetzt)
    }
    /// Laedt die Ersatzschrift, **einmal** (M3/M4), und haelt sie im
    /// Renderer.
    ///
    /// added 03.10.2026. `find_best_font` braucht eine Liste, und die
    /// Liste braucht ein Wurzelverzeichnis. Gesucht wird an den drei
    /// ueblichen Orten; jeder Fehlschlag ist erlaubt, dann bleibt der
    /// Wert `None` und der Text wird nur nicht gezeichnet (nicht: der
    /// Lauf stuerzt ab).
    ///
    /// DER GRUND FUER DEN PLATZ IM RENDERER: bei 3502 Formularen
    /// waere das 3502 mal dieselbe Datei von der Platte.
    /// Laedt eine der **URW**-Dateien (Nimbus), also genau die Schrift,
    /// die der Referenz-Renderer fuer die Standard-14-Namen nimmt.
    ///
    /// added 04.10.2026. GEMESSEN mit `fc-match`, und das ist die
    /// Kopplung, die zaehlt:
    ///
    /// ```text
    /// Times     -> NimbusRoman-Regular.otf
    /// Helvetica -> NimbusSans-Regular.otf
    /// Courier   -> NimbusMonoPS-Regular.otf
    /// ```
    ///
    /// Die Dateien sind OpenType mit CFF-Tabelle (kein `glyf`), also
    /// geht `narsil_text::load_system_font` an ihnen vorbei — `Cff::aus_otf`
    /// und `Cff::cmap_lesen` sind der Weg.
    ///
    /// DIE NAMEN, und sie sind aus der Liste der Dateien abgelesen, nicht
    /// geraten: `NimbusRoman`, `NimbusSans`, `NimbusMonoPS` mit den
    /// Endungen `Regular`, `Bold`, `Italic`, `BoldItalic`.
    fn urw_laden(
        &mut self,
        kategorie: usize,
        fett: bool,
        kursiv: bool,
        schriften: &[narsil_text::SystemFont],
    ) -> Option<std::rc::Rc<Ersatzschrift>> {
        const FAMILIEN: [[&str; 3]; 3] = [
            ["nimbussans", "nimbussansnarrow", "nimbusmono"],
            ["nimbusroman", "nimbusroman", "nimbusroman"],
            ["nimbusmonops", "nimbusmonops", "nimbusmonops"],
        ];
        let endung = match (fett, kursiv) {
            (false, false) => "regular",
            (true, false) => "bold",
            (false, true) => "italic",
            (true, true) => "bolditalic",
        };
        for familie in FAMILIEN[kategorie.min(2)] {
            for f in schriften {
                let datei = f
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if !datei.starts_with(familie) || !datei.ends_with(endung) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&f.path) else {
                    continue;
                };
                let (Ok(cff), Ok(cmap)) = (
                    crate::cff::Cff::aus_otf(&bytes),
                    crate::cff::Cff::cmap_lesen(&bytes),
                ) else {
                    continue;
                };
                if cmap.is_empty() {
                    continue;
                }
                // added 04.10.2026. **GEHE NUR, WENN DIE GLYPHEN WIRKLICH
                // GEHEN.** Ohne diese Pruefung ist der Weg schlimmer als
                // nichts.
                //
                // GEMESSEN am 04.10.2026 an `NimbusRoman-Regular.otf`:
                // `cmap` (732 Zeichen), 855 Glyphen, `A` -> GID 34 — und
                // `umriss(34)` bricht ab mit
                // `ZuViel { was: "Subr-Index", n: 59 }`: der Leser findet
                // **keine** Subroutinen (`subrs_je_fd() == [0]`), obwohl
                // `fontTools` **270** zaehlt. Folge war, dass Seite 1 von
                // `1512.03385.pdf` nur **8 455** Pixel hatte statt
                // 74 920 — fast leer. Ein Fehler, den man nur durch
                // Nachmessen sieht: die Aehnlichkeitsquote stieg auf
                // 88,4 %, weil eine fast leere Seite „Aehnlichkeit“
                // erhaelt, wenn die Referenz auch leer waere.
                //
                // DER WEG: `A` holen, und wenn der Umriss nicht mindestens
                // eine Kontur mit zwei Punkten liefert, diese Datei
                // **verwerfen** und die naechste probieren. Das ist eine
                // Messung beim Laden, keine Vermutung.
                let roh = cmap.get(&'A').copied().map(|g| cff.umriss(g));
                let haken = roh
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|k| k.iter().any(|c| c.len() >= 2))
                    .unwrap_or(false);
                if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
                    eprintln!(
                        "URW {datei} cmap={} glyphen={} A_gid={:?} subrs={:?} tiefen={:?} brauchbar={haken} A={:?}",
                        cmap.len(),
                        cff.glyphen_anzahl(),
                        cmap.get(&'A'),
                        cff.subrs_je_fd(),
                        cff.tiefen_hochwasser(),
                        roh.as_ref().map(|r| match r {
                            Ok(k) => format!("{} Konturen", k.len()),
                            Err(e) => format!("{e:?}"),
                        })
                    );
                }
                if !haken {
                    continue;
                }
                return Some(std::rc::Rc::new(Ersatzschrift::Cff(
                    std::rc::Rc::new(cff),
                    cmap,
                )));
            }
        }
        None
    }

    /// Lade die benoetigten Ersatzschriften, **einmal** (M3/M4), und haelt
    /// sie im Renderer.
    ///
    /// added 03.10.2026. `find_best_font` braucht eine Liste, und die
    /// Liste braucht ein Wurzelverzeichnis. Gesucht wird an den drei
    /// ueblichen Orten; jeder Fehlschlag ist erlaubt, dann bleibt der
    /// Wert `None` und der Text wird nur nicht gezeichnet (nicht: der
    /// Lauf stuerzt ab).
    ///
    /// DER GRUND FUER DEN PLATZ IM RENDERER: bei 3502 Formularen
    /// waere das 3502 mal dieselbe Datei von der Platte.
    pub fn ersatz_laden(&mut self, gewuenschte_schnitte: &[(usize, bool, bool)]) -> Ersatzsaetze {
        if self.ersatz.iter().any(|f| f.is_some()) {
            return self.ersatz.clone();
        }
        // Release 09.10.2026: WINDOWS. GEMESSEN an `1706.03762.pdf` (Type1-
        // Schriften von pdfTeX, ohne Umrisse fuer uns): auf Windows stand
        // KEIN Buchstabe auf der Seite, nur die Linien — die Suche unten
        // kennt nur die Linux-Ordner, und ihre Einstufung geht nach dem
        // Dateinamen („serif" im Namen), den `times.ttf` nicht traegt.
        // Also hier die festen Dateien der Windows-Schriften je Schnitt.
        for (kategorie, fett, kursiv) in gewuenschte_schnitte.iter().copied() {
            let index = ersatz_index(kategorie, fett, kursiv);
            if self.ersatz[index].is_none()
                && let Some(f) = windows_ersatz(kategorie, fett, kursiv)
            {
                self.ersatz[index] = Some(f);
            }
        }
        const WURZELN: [&str; 3] = [
            "/usr/share/fonts",
            "/usr/local/share/fonts",
            "/usr/share/texmf/fonts",
        ];
        for wurzel in WURZELN {
            let Ok(pfad) = std::path::Path::new(wurzel).canonicalize() else {
                continue;
            };
            let Ok(schriften) = narsil_text::find_system_fonts(&pfad) else {
                continue;
            };
            if schriften.is_empty() {
                continue;
            }
            // added 03.10.2026. DIE RANGFOLGE IST JETZT JE SCHNITT
            // (Kategorie x fett x kursiv), und es wird nur geladen, was
            // das Dokument auch wirklich benutzt.
            //
            // GEMESSEN, in dieser Reihenfolge:
            //
            //  a) `rfc2616.pdf` bittet um `/BaseFont /Times-Roman` —
            //     **nicht** fett. Die Auswahl nahm
            //     `LiberationSerif-Bold.ttf`, weil `find_system_fonts`
            //     alphabetisch laeuft und „Bold" vor „Regular" kommt,
            //     und beide haben denselben Rang. Die Rasterung stimmte,
            //     aber die Buchstaben waren so dick, dass die
            //     Gegenformen von `R`, `C` und `6` **zu** waren.
            //  b) `SystemFont::category` ist nach dem **Dateinamen**
            //     eingestuft, `SystemFont::family` nicht. GEMESSEN:
            //     `LiberationSerif-Regular.ttf` wird von `family` zu
            //     `liberation` verkuerzt, weil `Serif` als Stilangabe
            //     wegfällt. Die erste Fassung filterte nach `family`
            //     und fand **null** Serif-Kandidaten — Seite 5 von
            //     `rfc2616.pdf` war danach **leer** (0 Pixel bei 7353
            //     Zeichen).
            //  c) `NimbusRoman-Regular.otf` liegt als OpenType mit
            //     **CFF**-Umrissen vor. `narsil-text` kann keine
            //     CFF-Umrisse zeichnen, `has_glyf()` ist `false`.
            const RANG: [[&str; 5]; 3] = [
                // 0: Serifenlos
                [
                    "dejavusans",
                    "liberationsans",
                    "freesans",
                    "notosans",
                    "nimbussans",
                ],
                // 1: Serif
                [
                    "liberationserif",
                    "dejavuserif",
                    "freeserif",
                    "notoserif",
                    "nimbusroman",
                ],
                // 2: Fest
                [
                    "dejavusansmono",
                    "liberationmono",
                    "freemono",
                    "notosansmono",
                    "nimbusmono",
                ],
            ];
            const VERBOTEN: [&str; 5] = ["symbol", "unifont", "hint", "dingbat", "wingding"];
            let gewuenscht = [
                narsil_text::FontCategory::SansSerif,
                narsil_text::FontCategory::Serif,
                narsil_text::FontCategory::Monospace,
            ];
            // Nur die Schnitte laden, die die Ressourcen des Blattes
            // brauchen: zwölf Schriftdateien zu laden, weil zwölf Felder
            // existieren, waere 12 MB Speicher fuer einen Bilderrahmen.
            for (kategorie, fett, kursiv) in gewuenschte_schnitte.iter().copied() {
                let index = ersatz_index(kategorie, fett, kursiv);
                if self.ersatz[index].is_some() {
                    continue;
                }
                // added 04.10.2026. ZUERST DIE URW-DATEIEN, und das ist
                // der ganze Punkt dieser Zeile.
                //
                // GEMESSEN: `fc-match Times` -> `NimbusRoman-Regular.otf`,
                // `fc-match Helvetica` -> `NimbusSans-Regular.otf`,
                // `fc-match Courier` -> `NimbusMonoPS-Regular.otf`. Das
                // sind genau die Dateien, die der Referenz-Renderer
                // nimmt, und sie liegen in
                // `/usr/share/fonts/opentype/urw-base35/`.
                //
                // Sie haben **keine** `glyf`-Tabelle, sondern `CFF`
                // (GEMESSEN an `NimbusRoman-Regular.otf`: `OTTO`, 12
                // Tabellen, `CFF` bei Offset 204 mit 70 824 Byte) — der
                // Weg dafuer ist `Cff::aus_otf` mit `cmap_lesen`.
                //
                // GEMESSENE WIRKUNG, wenn man sie nimmt: der Textblock
                // von `1512.03385.pdf` Seite 1 hatte **148 %** der
                // Tintenmenge der Referenz (LiberationSerif ist fetter
                // als NimbusRoman).
                if self.ersatz[index].is_none()
                    && let Some(f) = self.urw_laden(kategorie, fett, kursiv, &schriften)
                {
                    self.ersatz[index] = Some(f);
                }
                if self.ersatz[index].is_some() {
                    continue;
                }
                let mut passend: Vec<(usize, usize, &narsil_text::SystemFont)> = schriften
                    .iter()
                    .filter(|f| {
                        let datei = f
                            .path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_ascii_lowercase())
                            .unwrap_or_default();
                        f.category == gewuenscht[kategorie]
                            && f.bold == fett
                            && f.italic == kursiv
                            && !VERBOTEN.iter().any(|v| datei.contains(v))
                    })
                    .map(|f| {
                        let datei = f
                            .path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_ascii_lowercase())
                            .unwrap_or_default();
                        let rang = RANG[kategorie]
                            .iter()
                            .position(|n| datei.contains(n))
                            .unwrap_or(RANG[kategorie].len());
                        (rang, 0, f)
                    })
                    .collect();
                // added 03.10.2026. DIE ZWEITE RANGLISTE: eine
                // **Regula**-Datei ist besser als eine Bold-Datei,
                // wenn die Auswahl fatten muss. GEMESSEN: `narsil-text`
                // liefert fuer `FreeSans` drei Familiennamen und die
                // fette Datei ist die erste im Verzeichnis.
                passend.sort_by_key(|(rang, _, f)| {
                    let datei = f
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    let regula = datei.contains("regular")
                        || datei.contains("-roman")
                        || datei.contains("-book");
                    (*rang, usize::from(!regula))
                });
                for (_, _, kandidat) in &passend {
                    if let Ok(f) = narsil_text::load_system_font(&kandidat.path) {
                        if f.has_glyf() {
                            self.ersatz[index] =
                                Some(std::rc::Rc::new(Ersatzschrift::Glyph(std::rc::Rc::new(f))));
                            break;
                        }
                    }
                }
            }
        }
        // added 03.10.2026. DIE RÜCKFALLEBENE, und sie ist Pflicht.
        //
        // GEMESSEN: ohne sie war Seite 5 von `rfc2616.pdf` **leer**,
        // als die Serif-Kategorie keinen Kandidaten fand. Ein leeres
        // Bild ist schlimmer als ein Text in der falschen Schrift.
        //
        // DIE REIHENFOLGE DES RÜCKFALLS: erst derselbe Schnitt, dann
        // dieselbe Kategorie in regula, dann die erste Kategorie. So
        // steht hinter einem fehlenden `LiberationSerif-BoldItalic`
        // wenigstens `LiberationSerif-Bold` statt eines Sans-Serif.
        for kategorie in 0..3 {
            for fett in [false, true] {
                for kursiv in [false, true] {
                    let index = ersatz_index(kategorie, fett, kursiv);
                    if self.ersatz[index].is_some() {
                        continue;
                    }
                    for (kat, fet, kur) in [
                        (kategorie, fett, kursiv),
                        (kategorie, false, false),
                        (0, fett, kursiv),
                        (0, false, false),
                    ] {
                        let q = ersatz_index(kat, fet, kur);
                        if self.ersatz[q].is_some() {
                            self.ersatz[index] = self.ersatz[q].clone();
                            break;
                        }
                    }
                }
            }
        }
        self.ersatz.clone()
    }
}

/// Die drei Ersatzschriften, je Kategorie.
///
/// added 03.10.2026. GEMESSEN: `rfc2616.pdf` bittet um
/// `/BaseFont /Times-Roman` (Serif) und bekam FreeSans (SansSerif).
/// Die **Vorschubweiten** stimmten nach F24, die **Formen** nicht —
/// ein Times-Satz in einer Sans-Schrift sieht aus wie ein
/// ueberzeichnetes Telegramm. Also laedt der Renderer jetzt **je
/// Kategorie** eine Schrift.
/// Eine geladene **Ersatzschrift** — in beiden Formen, die es gibt.
///
/// added 04.10.2026. DER GRUND FUER DEN WRAPPER: die Dateien, die der
/// Referenz-Renderer nimmt, sind **OpenType mit CFF-Tabelle**, und
/// `narsil_text` kann nur `glyf`.
///
/// GEMESSEN, und das ist der ganze Punkt:
///
/// ```text
/// fc-match Times     -> NimbusRoman-Regular.otf   (URW base-35)
/// fc-match Helvetica -> NimbusSans-Regular.otf
/// fc-match Courier   -> NimbusMonoPS-Regular.otf
/// fc-match Arial     -> LiberationSans-Regular.ttf
/// ```
///
/// Die Nimbus-Dateien liegen in `/usr/share/fonts/opentype/urw-base35/`
/// und haben **keine** `glyf`-Tabelle, sondern `CFF` (GEMESSEN an
/// `NimbusRoman-Regular.otf`: `OTTO`, 12 Tabellen, `CFF` bei Offset
/// 204 mit 70 824 Byte). Mit `narsil_text::load_system_font` sind sie
/// nicht ladbar — die Auswahl fiel auf `LiberationSerif`, und die ist
/// **deutlich fetter**: GEMESSEN an `1512.03385.pdf` Seite 1 hatte der
/// Textblock **148 %** der Tintenmenge der Referenz. (Mit der
/// Nimbus-Schrift sind es **138 %**; die verbleibenden zehn Prozent
/// sind die Rasterisierung, nicht die Form — Bericht Abschnitt 31.1.)
///
/// DIE DREI METHODEN, die der Zeichner braucht, und mehr nicht.
#[derive(Clone)]
pub enum Ersatzschrift {
    /// Eine echte TrueType-Datei mit `glyf` (aus `narsil-text`).
    Glyph(std::rc::Rc<narsil_text::truetype::TrueTypeFont>),
    /// Eine OpenType-Datei mit CFF-Tabelle, dazu ihre `cmap`.
    ///
    /// DIE CMAP IST NICHT OPTIONAL: eine CFF kennt nur **Namen**
    /// (`gid_von_name`), und die Standard-Strings benennen 140 Zeichen
    /// anders, als man sie schreibt (32 = `space`, 33 = `exclam`).
    /// Ohne `cmap` bekaeme die Ersatzschrift fuer den Buchstaben `e`
    /// keine Glyphe.
    Cff(
        std::rc::Rc<crate::cff::Cff>,
        std::collections::HashMap<char, u16>,
    ),
}

impl Ersatzschrift {
    /// Die Glyphe eines **Namens**, falls die Schrift sie hat.
    ///
    /// added 05.10.2026: der Weg ueber den Namen, den die Koordination
    /// fuer die Ligaturen nennt. Eine CFF kennt nur **Namen** (`fi`,
    /// `fl`, `Lslash`) — die Zeichencodes der `cmap` sind der
    /// Schnellweg, aber nicht der einzige.
    ///
    /// GEMESSEN an `NimbusRoman-Regular.otf`: `fi` ist GID 109,
    /// `fl` 110, `Lslash` 140, `dotlessi` 145 — dieselben GIDs, die
    /// `fontTools` nennt.
    pub fn gid_von_name(&self, name: &str) -> Option<u16> {
        match self {
            // added 05.10.2026: BEIM TRUE-TYPE-SCHNITT GIBT ES KEINEN
            // NAMEN, und das ist eine **Antwort**, kein Versehen. Eine
            // CFF hat ein `charset` aus Namen; ein `glyf` aus
            // `post`-Tafeln, und die sind in den Systemfonts der
            // Abnahme oft leer. Der Aufrufer fragt damit „welche GLYPHE
            // hat dieser Name“ und bekommt fuer den Schnitt, der Namen
            // nicht fuehrt, `None` — wie bei einem unbekannten Namen.
            Ersatzschrift::Glyph(_) => None,
            Ersatzschrift::Cff(c, _) => c.gid_von_name(name),
        }
    }

    /// Wie viele Einheiten ein `em` hat.
    pub fn units_per_em(&self) -> f64 {
        match self {
            Ersatzschrift::Glyph(f) => f.units_per_em() as f64,
            Ersatzschrift::Cff(c, _) => c.units_per_em() as f64,
        }
    }

    /// Die Glyphe eines Zeichens, falls die Schrift sie hat.
    pub fn glyph_index(&self, z: char) -> Option<u16> {
        match self {
            // added 04.10.2026: `narsil-text` liefert `u32`, hier wird
            // `u16` gebraucht — eine CFF mit mehr als 65535 Glyphen
            // kann es nicht geben.
            Ersatzschrift::Glyph(f) => f.glyph_index(z).and_then(|g| u16::try_from(g).ok()),
            Ersatzschrift::Cff(_, cmap) => cmap.get(&z).copied(),
        }
    }

    /// Deren Umriss in Schrifteinheiten.
    ///
    /// added 04.10.2026. DER FEHLER IST EIN **TEXT**, nicht `()`:
    /// clippy hat auf `result_unit_err` hingewiesen, und zu Recht — ein
    /// leerer Fehler bekommt im Aufrufer keine eigene Meldung. Bei der
    /// CFF ist der Grund fast immer derselbe (`Subr-Index`, `ZuViel`),
    /// und der gehoert in die Ausgabe, weil er die Ursache der leeren
    /// Seiten war.
    pub fn umriss(&self, gid: u16) -> Result<Vec<Vec<(f32, f32)>>, String> {
        match self {
            Ersatzschrift::Glyph(f) => f
                .glyph_outline(gid as u32)
                .map(|u| {
                    u.contours
                        .iter()
                        .map(|k| k.points.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>())
                        .collect()
                })
                .map_err(|_| "glyf-Tabelle unlesbar".to_string()),
            Ersatzschrift::Cff(c, _) => c.umriss(gid).map_err(|e| format!("CFF: {e:?}")),
        }
    }

    /// Die natuerliche Vorschubbreite, **wenn** sie bekannt ist.
    ///
    /// added 04.10.2026. `None` bei einer CFF, und das ist Absicht: die
    /// Ersatzschrift wird nur fuer Schriften **ohne** eingebettete
    /// Datei benutzt, und die erklaeren fast immer die Breiten der
    /// Standard-14-Familien — in der die URW-Klone metrisch
    /// uebereinstimmen. Ohne die Zahl zu kennen, wird **nicht**
    /// gestreckt (`dehnung` 1.0), und das ist fuer diese Faelle
    /// richtig. Die Streckung bleibt fuer die `glyf`-Ersatzschriften,
    /// weil dort `hmtx` gelesen werden kann und die Schrift FreiSans
    /// oder DejaVu **nicht** metrisch passt.
    pub fn vorschub(&self, gid: u16) -> Option<f64> {
        match self {
            Ersatzschrift::Glyph(f) => f
                .glyph_metrics(gid as u32)
                .map(|m| m.advance_width as f64)
                .filter(|w| *w > 1.0),
            // added 05.10.2026: DIE CFF HAT DOCH EINEN VORSCHUB, und er
            // stand nur nicht zur Verfuegung: die natuerliche Breite steht
            // in `hmtx` der OTF-Datei, nicht in der CFF-Tabelle. `None`
            // hiess fuer den Zeichner „nicht strecken", und dadurch
            // behielten alle Ersatzglyphen ihre eigene Breite — bei
            // `1706.03762.pdf` 4,9 % zu breit (500 aus `hmtx` gegen
            // 524,9 aus `/Widths`), also beruehrten sich die Buchstaben.
            // GEMESSEN: siehe `team/BERICHT_SERVER_PDF_GLYPHEN.txt`.
            Ersatzschrift::Cff(c, _) => c.vorschub(gid).map(|w| w as f64).filter(|w| *w > 1.0),
        }
    }
}

pub type Ersatzsaetze = [Option<std::rc::Rc<Ersatzschrift>>; 12];

/// Der Index der Ersatzschrift fuer einen Schnitt.
///
/// Release 09.10.2026: die Dateien der Windows-Schriften je Schnitt
/// (`kategorie * 4 + fett * 2 + kursiv`, wie [`ersatz_index`]) — Times New
/// Roman, Arial und Courier New sind die Metrik-Zwillinge der Standard-14
/// (Times, Helvetica, Courier), danach Georgia, Segoe UI und Consolas.
const WINDOWS_ERSATZ: [[&str; 2]; 12] = [
    ["arial.ttf", "segoeui.ttf"],
    ["ariali.ttf", "segoeuii.ttf"],
    ["arialbd.ttf", "segoeuib.ttf"],
    ["arialbi.ttf", "segoeuiz.ttf"],
    ["times.ttf", "georgia.ttf"],
    ["timesi.ttf", "georgiai.ttf"],
    ["timesbd.ttf", "georgiab.ttf"],
    ["timesbi.ttf", "georgiaz.ttf"],
    ["cour.ttf", "consola.ttf"],
    ["couri.ttf", "consolai.ttf"],
    ["courbd.ttf", "consolab.ttf"],
    ["courbi.ttf", "consolaz.ttf"],
];

/// Release 09.10.2026: eine Ersatzschrift aus `%WINDIR%\Fonts` (siehe
/// [`WINDOWS_ERSATZ`]); `None` ausserhalb von Windows oder ohne Datei.
fn windows_ersatz(kategorie: usize, fett: bool, kursiv: bool) -> Option<std::rc::Rc<Ersatzschrift>> {
    if !cfg!(windows) {
        return None;
    }
    let ordner = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
        .join("Fonts");
    let namen = WINDOWS_ERSATZ.get(ersatz_index(kategorie, fett, kursiv))?;
    namen.iter().find_map(|datei| {
        let f = narsil_text::load_system_font(&ordner.join(datei)).ok()?;
        f.has_glyf()
            .then(|| std::rc::Rc::new(Ersatzschrift::Glyph(std::rc::Rc::new(f))))
    })
}

/// added 03.10.2026. `kategorie * 4 + fett * 2 + kursiv`, damit ein
/// Feld statt einer Suche genuegt. `Option`, weil nicht jeder Schnitt
/// installiert sein muss — siehe die Rueckfallebene in `ersatz_laden`.
pub fn ersatz_index(kategorie: usize, fett: bool, kursiv: bool) -> usize {
    ((kategorie.min(2)) * 4) + usize::from(fett) * 2 + usize::from(kursiv)
}

/// Der Schnitt, den ein `/BaseFont`-Name verlangt: Kategorie, fett,
/// kursiv.
///
/// added 03.10.2026. GEMESSEN: `rfc2616.pdf` bittet um
/// `/BaseFont /Times-Roman` — **nicht** fett. Die Auswahl nahm
/// `LiberationSerif-Bold.ttf`, weil `find_system_fonts` alphabetisch
/// laeuft und „Bold" vor „Regular" kommt, und beide haben denselben
/// Rang. Die Seitenraster stimmten, aber die Buchstaben waren so dick,
/// dass die Gegenformen von `R`, `C` und `6` **zu** waren — es sah
/// aus wie ein Fettsatz, nicht wie ein Times-Satz.
///
/// DIE REIHENFOLGE im Namen ist beweisbar: `/BaseFont /Times-Bold`
/// nennt „Bold" vor „Roman", `/Times-BoldItalic` dazwischen. Also wird
/// der Name **zerlegt**, statt ihn nur auf „times" zu pruefen.
pub fn schnitt_von(basisname: &str) -> (usize, bool, bool) {
    let kurz = crate::standard14::ohne_praefix(basisname).to_ascii_lowercase();
    // added 04.10.2026: „medi" und „demi" sind **fett**. GEMESSEN an
    // `1512.03385.pdf` und `2103.00020.pdf`: dort stehen
    // `NimbusRomNo9L-Medi` und `HelveticaNeueLTStd-BlkCn`. In der
    // URW-Sammlung (und in `psnfss`) ist `Medi` die **halbfette** Form
    // der Familie — `m/sb` heisst „medium, semibold" —, und `BlkCn`
    // ist Black Condensed, also fett. Beide standen vorher als regula
    // und bekamen einen duennen Schnitt.
    //
    // „bd" steht **nicht** in der Liste: „Bodoni" enthaelt es, und
    // Bodoni ist regula.
    let fett = kurz.contains("bold")
        || kurz.contains("blk")
        || kurz.contains("black")
        || kurz.contains("heavy")
        || kurz.contains("demi")
        || kurz.contains("medi")
        || kurz.contains("semib");
    // added 04.10.2026: `Ital` ist die Kurzform der URW-Namen.
    // GEMESSEN: `1512.03385.pdf` und `2103.00020.pdf` benutzen
    // `NimbusRomNo9L-ReguItal` — „ital" steckt in keinem der bisherigen
    // Stichworte, der Name wurde also als **regula** eingestuft und
    // bekam einen aufrechten Schnitt fuer einen kursiven Text.
    //
    // „capital" enthaelt „ital" und wird deshalb ausgenommen: ein
    // `SmallCapital` ist nicht kursiv.
    let kursiv = kurz.contains("italic")
        || kurz.contains("oblique")
        || (kurz.contains("ital") && !kurz.contains("capital"));
    (kategorie_von(&kurz), fett, kursiv)
}

/// Die Kategorie, die ein `/BaseFont`-Name verlangt.
///
/// added 03.10.2026. 0 = Serifenlos, 1 = Serif, 2 = Fest. Die Reihen-
/// folge ist die von `narsil_text::FontCategory`, aber als Zahl, weil
/// die Ersatzschriften in einem Feld liegen und ein `Enum` darin
/// unnoetig Gewicht haette.
///
/// added 04.10.2026. DIE URW-NAMEN FEHLTEN, und sie sind genau die,
/// die in den Test-PDFs am haeufigsten sind. `NimbusRomNo9L-Regu`
/// ist ein **Times**-Klon: `ohne_praefix` liefert
/// „nimbusromno9l-regu", und das enthaelt weder „roman" noch
/// „serif" — also fiel es durch bis zur Vorgabe **serifenlos**.
///
/// GEMESSEN, welche Namen in den elf PDFs vorkommen (`/BaseFont`,
/// `mutool show | grep`): `NimbusRomNo9L-Regu/-Medi/-ReguItal`,
/// `NimbusMonL-Regu`, `NimbusSanL-Regu`, `Times-Roman`,
/// `TimesNewRomanPSMT`, `NotoSerif-*`, `CMR10`, `CMMI10`,
/// `DejaVuSans`, `Helvetica`, `HelveticaLTStd-*`,
/// `HelveticaNeueLTStd-*`, `ArialMT`, `Roboto*`, `CourierNewPSMT`.
///
/// GEMESSEN die Wirkung: `ersatz_index` stand bei **allen** Schriften
/// auf **0** (Serifenlos), weil `NimbusRomNo9L` als sans klassifiziert
/// wurde. `rfc2616.pdf` hat 465 solcher Schriftverweise, `rfc9110.pdf`
/// 680 — der gesamte Serifentext bekam die Sans-Ersatzschrift.
pub fn kategorie_von(basisname: &str) -> usize {
    let n = crate::standard14::ohne_praefix(basisname).to_ascii_lowercase();
    // added 04.10.2026: „fest" zuerst, und `monl` ist dabei — das ist
    // der Kern von `NimbusMonoPS`/`NimbusMonL`, und `monl` enthaelt
    // **kein** „mono" (GEMESSEN: der Name wird sonst als sans gefuehrt).
    if n.contains("courier") || n.contains("mono") || n.contains("monl") {
        2
    } else if n.contains("times")
        || n.contains("serif")
        || n.contains("roman")
        // added 04.10.2026: die drei URW-Familien der Standard-14.
        || n.contains("romno")     // NimbusRomNo9L
        || n.contains("romanji")   // NimbusRomanNo9L
        || n.contains("z003")      // URW-Namen der drei Serifen
        || n.contains("c059")      // Century Schoolbook (C059)
        || n.contains("p052")      // Palatino (P052)
        || n.contains("book")
        || n.contains("georgia")
        || n.contains("garamond")
        || n.contains("charter")
        || n.contains("minion")
        || n.contains("cmr")       // CMR10, CMR7 …
        || n.contains("cmmi")      // CMMI10 (Mathematik kursiv)
        || n.contains("cmsy")      // CMSY10 (Symbole)
        || n.contains("cmex")      // CMEX10
        || n.contains("cmbx")      // CMBX10 (fett)
        || n.contains("cmu")
    // CMUNSS, CMUITT
    {
        1
    } else {
        0
    }
}

/// Liest eine Matrix aus einem `/Matrix`-Array.
///
/// added 03.10.2026. `Matrix::IDENTITAET` bei allem, was kein
/// sechsstelliges Array ist — ein Formular **ohne** `/Matrix` wird mit
/// der Identitaet gezeichnet, das ist die Vorgabe der Spezifikation.
/// Eine Liste mit drei Werten ist **kein** gueltiges `/Matrix` und wird
/// nicht „irgendwie" gedeutet.
fn matrix_aus_wert(wert: Option<&Wert>) -> inhalt::Matrix {
    let Some(arr) = wert.and_then(Wert::array) else {
        return inhalt::Matrix::IDENTITAET;
    };
    if arr.len() != 6 {
        return inhalt::Matrix::IDENTITAET;
    }
    let mut z = [0.0f64; 6];
    for (i, slot) in z.iter_mut().enumerate() {
        let v = arr.get(i).and_then(Wert::zahl).unwrap_or(0.0);
        *slot = if v.is_finite() { v } else { 0.0 };
    }
    // `unendlich` in einer Matrix wuerde jeden Punkt auf `inf`
    // setzen; `0` gibt eine entartete (unsichtbare) Form, was der
    // ehrlichere Befund ist.
    if z.iter().any(|v| v.abs() > 1e6) {
        return inhalt::Matrix::IDENTITAET;
    }
    inhalt::Matrix {
        a: z[0],
        b: z[1],
        c: z[2],
        d: z[3],
        e: z[4],
        f: z[5],
    }
}

/// Die Zaehlung fuer den Bericht.
#[derive(Debug, Clone, Default)]
struct Bilanz {
    schreibvorgaenge: usize,
    zeichen: usize,
    bilder: usize,
    formulare: usize,
    unbekannt: Vec<String>,
    /// Glyphen aus eingebetteten CFF-Programmen.
    ///
    /// added 03.10.2026. GEMESSEN, weil die erste Fassung des
    /// CFF-Zweigs **nie** durchlaufen wurde und der Pixelvergleich
    /// das nicht zeigen konnte (mit und ohne den Zweig bitweise
    /// gleich). Die Zahl steht deshalb in der Bilanz, nicht nur im
    /// Quelltext.
    cff_glyphen: usize,
    /// added 05.10.2026: Zeichen, fuer die die Ersatzschrift **keine**
    /// Glyphe hat — nach dem Versuch ueber das Zeichen **und** ueber den
    /// Glyphennamen. Bis dahin stand hier 0, obwohl in
    /// `1706.03762.pdf` Buchstaben fehlten: das ist der Grund, warum die
    /// Ligaturen so lange unentdeckt blieben.
    ersatz_fehl: usize,
    /// added 05.10.2026: siehe [`Bilanz::kontur_gemustert`].
    kontur_gemustert: usize,
    /// added 05.10.2026: siehe [`Bilanz::stuecke_ausgelassen`].
    stuecke_ausgelassen: usize,
    /// added 07.10.2026: Formen, die an `MAX_FORM_TIEFE` scheiterten.
    /// Siehe die oeffentliche Ausgabe in der Bilanzzeile.
    formen_abbruch_tiefe: usize,
    /// added 07.10.2026: die groesste gesehene Form-Verschachtelung.
    formen_tiefe_max: usize,
    /// added 07.10.2026: als Zyklus erkannte Formen.
    formen_zyklus: usize,
}

/// Wie viele verschiedene unbekannte Operatoren gemeldet werden.
///
/// added 03.10.2026 (M3/M4). 200. Eine Seite kann 40.000 `BX` haben;
/// das Log soll die **Arten** zeigen, nicht die Menge.
pub const MAX_UNBEKANNTE: usize = 200;

/// Der Textzeichner: macht aus Zeichencodes Konturen.
///
/// added 03.10.2026.
///
/// DER WEG, und er hat drei Stufen, weil die Fonts nicht alle gleich
/// sind:
///
///  1. **Eingebettete TrueType** (`/FontFile2`): `glyph_outline(gid)`
///     aus `narsil-text`. Das ist der einzige Weg, der die *echten*
///     Glyphen zeigt.
///  2. **Eingebettet, aber kein Index** (`/CIDToGIDMap` als Stream):
///     GID aus dem Stream.
///  3. **Kein eingebetteter Font**: ein Systemfont nach `BaseFont`
///     (Helvetica, Times, Courier), und der Zeichencode wird ueber
///     `/ToUnicode` oder WinAnsi zu Unicode.
///
/// GEMESSEN: bei den RFC-PDFs (Helvetica, Courier) ist Schritt 3 der
/// Normalfall, bei den arXiv-PDFs Schritt 1. Ohne Schritt 3 waeren
/// alle RFC-Seiten leer.
struct TextzeichnerBox {
    schriften: std::collections::HashMap<String, std::rc::Rc<schrift::Ressource>>,
    zeichen_gesamt: usize,
    /// Die Ersatzschrift fuer Schriften **ohne** eingebettete Datei.
    ///
    /// added 03.10.2026. GEMESSEN: `rfc2616.pdf`, `rfc9110.pdf`,
    /// `rfc9111.pdf` und `rfc9113.pdf` benutzen Helvetica, Times und
    /// Courier **ohne** `/FontFile` — sie stammen aus `enscript`.
    /// Ohne Ersatzschrift waeren alle 421 Seiten dieser vier PDFs
    /// **ohne Text**, und die Abnahme haette es nicht gemerkt: die
    /// Seiten sind gerendert, nur leer.
    ///
    /// Sie wird **einmal** geladen (der teuerste Schritt) und fuer
    /// alle fehlenden Schriften benutzt. `Rc`, weil der Renderer sie
    /// haelt und jeder Formular-Aufruf sie nur leiht.
    /// added 03.10.2026. **Je Kategorie eine** (0 serifenlos, 1 Serif,
    /// 2 fest) — siehe [`Ersatzsaetze`].
    ersatz: Ersatzsaetze,
    /// Wie viele Glyphen aus einer eingebetteten **CFF** gezeichnet
    /// wurden.
    ///
    /// added 03.10.2026. DER GRUND FUER DEN ZAEHLER: die erste Fassung
    /// hatte den CFF-Zweig im Zeichner, und der Pixelvergleich gegen
    /// `pdftoppm` war **bitweise gleich** mit und ohne ihn — die
    /// Dateien wurden also nie ueber diesen Weg gezeichnet. „Wird nicht
    /// benutzt" und „wirkt nicht" sind ohne Zaehler nicht zu
    /// unterscheiden.
    cff_glyphen: usize,
    /// added 05.10.2026: Wie oft die Ersatzschrift fuer ein Zeichen
    /// **keine** Glyphe hat.
    ///
    /// DER GRUND FUER DEN ZAEHLER: bis dahin gab es dafuer keinen, und
    /// im Bild fehlte der Buchstabe, waehrend die Bilanz 0 Fehler
    /// meldete. Genau das ist den Ligaturen von `1706.03762.pdf`
    /// widerfahren: drei Faelle, die die Koordination gemeldet hat, und
    /// kein Zaehler hat sie gezeigt.
    ///
    /// Der Wert wandert ueber [`Bilanz::ersatz_fehl`] in die Zeile, die
    /// `examples/pdf_als_png` am Ende jeder Seite schreibt.
    ersatz_fehl: usize,
}

/// Ein Punkt des Glyphenumrisses aus dem **Textraum** in die Seite.
///
/// added 05.10.2026. DIE FORMEL, und sie ist die ganze Drehung:
///
/// ```text
/// x = a · ux + c · uy + e
/// y = b · ux + d · uy + f
/// ```
///
/// GEMESSEN an `1706.03762.pdf` Seite 1, dem arXiv-Stempel mit der
/// Textmatrix `0 1 -1 0 32 237`. Fuer den Punkt (10, 0) des Textraums
/// sagt die Formel (32, 247) — senkrecht am linken Rand, zehn Punkt
/// oberhalb des Ursprungs. Ohne `b` und `c` waere (42, 237) herausgekommen:
/// waagerecht, und zwar quer ueber den Abstract.
pub fn seiten_punkt(m: &inhalt::Matrix, ux: f64, uy: f64) -> (f32, f32) {
    (
        (m.a * ux + m.c * uy + m.e) as f32,
        (m.b * ux + m.d * uy + m.f) as f32,
    )
}

impl TextzeichnerBox {
    /// added 03.10.2026. `clippy::too_many_arguments (8/7)` ist hier
    /// **bewusst** zugelassen — dieselbe Entscheidung wie in
    /// `narsil-layout-engine/src/box.rs` und `flow.rs`, und aus dem
    /// gleichen Grund: die acht Parameter sind der **gesamte**
    /// Textzustand nach ISO 32000-1, Abschnitt 9.4.4 (`Tfs`, `Th`,
    /// `Tmode`, `Rise`, `Tc`, `Tw`, `Tz` plus der Font). Ein
    /// Sammel-`Textzustand`-Struct wuerde nichts gewinnen: Es wird
    /// genau einmal pro gezeigter Zeichenfolge gebraucht, und das
    /// Struct wuerde nur eine Ebene tiefer liegen.
    #[allow(clippy::too_many_arguments)]
    fn glyphen_seitenkoordinaten(
        &mut self,
        name: &str,
        zeichen: &[u8],
        basis: (f64, f64),
        groesse: f64,
        // added 05.10.2026: die Matrix `ctm · tm`. `basis` ist nur ihr
        // Versatz, und ein Versatz kann keine Drehung tragen.
        matrix: inhalt::Matrix,
        horizont: f64,
        laufweite: f64,
        anhebung: f64,
    ) -> Vec<Vec<(f32, f32)>> {
        self.zeichen_gesamt += zeichen.len();
        if zeichen.len() > schrift::MAX_TEXTZEICHEN {
            return Vec::new();
        }
        let r = self.schriften.get(name);
        let Some(r) = r else {
            return Vec::new();
        };
        // Identity-H: zwei Byte je Zeichen.
        let codes = if r.art == schrift::SchriftArt::Type0 {
            zeichen
                .chunks(2)
                .map(|c| {
                    ((c.first().copied().unwrap_or(0) as u32) << 8)
                        | c.get(1).copied().unwrap_or(0) as u32
                })
                .collect::<Vec<u32>>()
        } else {
            zeichen.iter().map(|b| *b as u32).collect::<Vec<u32>>()
        };
        let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
        // added 05.10.2026. VORSCHUB UND GRUNDLINIE LAUFEN IM **TEXTRAUM**,
        // nicht in Seitenkoordinaten — und das ist der Fehler, den der
        // erste Versuch gemacht hat.
        //
        // GEMESSEN, wie er aussah: `vorschub` began bei `basis.0`, und
        // `basis` ist der **Versatz der Matrix** (also schon Seite). Wurde
        // dieser Wert dann noch einmal durch `auf_seite` geschickt, kam
        // `a · e + e` heraus. Fuer die Abstractzeile mit `a = 1,02` und
        // `e = 143,87` sind das 290,6 statt 143,87 — die Zeile stand um
        // eine halbe Spaltenbreite zu weit rechts, und die Abweichung
        // gegen `pdftoppm` stieg von 5,91 auf **14,34** mittlere
        // Abweichung (55 094 auf 84 925 abweichende Pixel).
        //
        // RICHTIG IST: ab hier ist `basis` nur noch die Kontrolle, dass
        // eine Matrix da ist; die Seite entsteht ausschliesslich aus
        // `auf_seite`.
        let mut vorschub = 0.0f64;
        let grundlinie = anhebung;
        // added 03.10.2026. DIE GLYPHEN SIND IN **SCHRIFT-EINHEITEN**,
        // nicht in Punkte.
        //
        // DER FEHLER: `sx`/`sy` waren `groesse` und `groesse *
        // horizont`. Eine Glyphe ist aber typischerweise 700 bis 1400
        // Einheiten hoch, und die Schrift hat 1000 oder 2048 Einheiten
        // je Schriftgroesse (`units_per_em`). Bei 12 Punkt Text war
        // eine Zeile damit 12 * 1400 = 16.800 Punkte hoch — sie lag
        // vollstaendig **ausserhalb** der Seite.
        //
        // GEMESSEN: `dummy.pdf` zeichnete 14 Zeichen und **0 Pixel**.
        // Das ist das Bild des Fehlers: `zeichen` zaehlt die Zeichen
        // aus dem Content-Stream, `gefuellte_pixel` zaehlt das, was
        // wirklich auf die Flaeche kam — und es kam nichts.
        //
        // Die Breite kommt aus `/Widths` und ist in 1/1000
        // Schriftgroesse, die Umrechnung `(breite / 1000.0) * groesse`
        // stimmt also bereits.
        let sx = groesse * horizont;
        let sy = groesse;
        // added 05.10.2026. DIE ABBILDUNG, und sie ist der Kern der
        // Sache: ein Punkt des Glyphenumrisses liegt in **Textraum**-Einheiten
        // bei `(ux, uy)`, und der Weg in die Seite fuehrt ueber
        //
        //     x = a · ux + c · uy + e
        //     y = b · ux + d · uy + f
        //
        // Vorher stand hier `x = vorschub + px · gsx` und
        // `y = grundlinie + py · gsy` — also **ohne** `b` und `c`. Fuer
        // `a = 1, b = 0, c = 0, d = 1` (der Normalfall, 99 % des Textes)
        // ist das dasselbe, und darum fiel es nicht auf. Sobald `b` oder
        // `c` ungleich null sind, ist es eine Drehung.
        //
        // GEMESSEN an `1706.03762.pdf` Seite 1: `0 1 -1 0 32 237 Tm` dreht
        // den arXiv-Stempel um 90 Grad. `pdftoppm` zeichnet ihn senkrecht
        // am linken Rand nach oben, der eigene Renderer legte ihn
        // waagerecht ueber den Abstract.
        // added 05.10.2026. `basis` IST DER VERSATZ DERSELBEN MATRIX
        // (`Aktion::Text::position` ist `matrix · (0,0)`). Seit der
        // Vorschub im Textraum laeuft, wird `basis` nicht mehr gerechnet —
        // und clippy sagt es. Statt das Argument zu verstecken, wird die
        // Zusage **geprueft**, die zwischen beiden steht: laesst jemand
        // `position` und `matrix` auseinanderlaufen, faellt es hier und
        // nicht erst als Text an falscher Stelle.
        let (me, mf) = (matrix.e, matrix.f);
        debug_assert!(
            (basis.0 - me).abs() < 1e-6 && (basis.1 - mf).abs() < 1e-6,
            "position ({}, {}) passt nicht zum Versatz der Matrix ({me}, {mf})",
            basis.0,
            basis.1
        );
        let auf_seite = |ux: f64, uy: f64| seiten_punkt(&matrix, ux, uy);
        for code in codes {
            let breite = r.breite(code);
            // GID: Identity heisst, der CID ist die Glyphe.
            let gid = r.gid(code);
            let mut gezeichnet = false;
            if let Some(font) = &r.eingebettet {
                if let Ok(umriss) = font.glyph_outline(gid) {
                    // added 03.10.2026: die Umrisse kommen in
                    // **Schrift-Einheiten** (`units_per_em` typisch
                    // 1000 oder 2048). Ohne die Umrechnung war eine
                    // Zeile 12 Punkt hoch 12 * 1400 = 16.800 Punkte
                    // und lag vollstaendig ausserhalb der Seite.
                    let upm = font.units_per_em().max(1) as f64;
                    let gsx = sx / upm;
                    let gsy = sy / upm;
                    for kontur in &umriss.contours {
                        if kontur.points.len() < 2 {
                            continue;
                        }
                        out.push(
                            kontur
                                .points
                                .iter()
                                .map(|p| {
                                    auf_seite(
                                        vorschub + p.x as f64 * gsx,
                                        grundlinie + p.y as f64 * gsy,
                                    )
                                })
                                .collect(),
                        );
                    }
                    gezeichnet = true;
                }
            }
            // added 03.10.2026. DER CFF-WEG, und er steht **nach** dem
            // TrueType-Weg, weil beides nie zugleich da ist: eine Datei
            // mit `glyf` ist keine CFF und umgekehrt.
            //
            // GEMESSEN, warum das noetig war: `f1040.pdf`, `fw9.pdf`,
            // `1207.7214.pdf`, `1512.03385.pdf` und `rfc9110.pdf` haben
            // **43** eingebettete CFF-Programme (Modulkopf von
            // `cff.rs`). Ohne diesen Zweig bekam keine dieser Schriften
            // einen Umriss, alle wurden mit einem Systemfont gezeichnet,
            // und die Bildabweichung gegen `pdftoppm` lag bei 10 bis 17 %
            // allein an der **Form**.
            //
            // DIE UMRECHNUNG ist dieselbe wie beim TrueType:
            // `sx / units_per_em`. Bei einer CFF ist das fast immer
            // 1000, weil `/FontMatrix` `[0.001 0 0 0.001 0 0]` ist —
            // `Cff::units_per_em` rechnet das aus dem FontMatrix.
            if !gezeichnet && let Some(cff) = &r.eingebettet_cff {
                if let Ok(konturen) = cff.umriss(gid as u16) {
                    let upm = cff.units_per_em().max(1.0) as f64;
                    let gsx = sx / upm;
                    let gsy = sy / upm;
                    for kontur in konturen {
                        if kontur.len() < 2 {
                            continue;
                        }
                        out.push(
                            kontur
                                .into_iter()
                                .map(|(px, py)| {
                                    auf_seite(
                                        vorschub + px as f64 * gsx,
                                        grundlinie + py as f64 * gsy,
                                    )
                                })
                                .collect(),
                        );
                    }
                    // added 03.10.2026. AUCH EINE **LEERE**
                    // KONTURLISTE ZAEHLT ALS GEZEICHNET.
                    //
                    // DER GRUND: ein Leerzeichen (Code 32) hat in einer
                    // CFF **keine** Kontur. Ohne diese Zeile faellt der
                    // Zeichner auf die Ersatzschrift zurueck und
                    // zeichnet dort ein Leerzeichen — das faellt nie
                    // auf, kostet aber bei jedem Wort eine
                    // `glyph_outline`-Abfrage.
                    gezeichnet = true;
                    self.cff_glyphen += 1;
                }
            }
            if !gezeichnet {
                // DIE ERSATZSCHRIFT. added 03.10.2026: ohne diesen
                // Zweig bleiben alle Schriften **ohne** eingebettete
                // Datei unsichtbar — bei den RFC-PDFs sind das
                // Helvetica, Times und Courier, also der Grossteil
                // des Textes von 421 Seiten.
                if let Some(font) = &self.ersatz[ersatz_index(
                    kategorie_von(&r.basisname),
                    schnitt_von(&r.basisname).1,
                    schnitt_von(&r.basisname).2,
                )] {
                    let upm = font.units_per_em().max(1.0);
                    let gsx = sx / upm;
                    let gsy = sy / upm;
                    // added 03.10.2026. DIE GLYPHE AUF DIE BREITE AUS
                    // `/Widths` STRECKEN.
                    //
                    // DER GRUND: die Positionen stimmen jetzt exakt
                    // (gemessen: „vided" beginnt bei 141,09 wie in
                    // `mutool trace`), aber die **Formen** kommen aus
                    // FreeSans statt NimbusRoman, und FreeSans ist
                    // breiter. Bei 11,96 Punkt Schrift sind die
                    // Versalien in FreeSans rund 8 % breiter als der
                    // Vorschub nach NimbusRoman — die Buchstaben
                    // beruehren sich.
                    //
                    // DER WEG: die natuerliche Breite der Glyphe
                    // (`glyph_metrics().advance_width`) wird auf die
                    // Sollbreite aus `/Widths` gestreckt. Das ist die
                    // uebliche Losung fuer Ersatzzschriften, und sie
                    // aendert nichts an den Positionen — nur an der
                    // Breite der gezeichneten Form.
                    let soll = breite / 1000.0;
                    let z = r
                        .unicode_von(code)
                        .or_else(|| char::from_u32(code))
                        .unwrap_or(' ');
                    // added 05.10.2026: DIE **STILLE** FEHLSTELLE. Sie war
                    // unsichtbar, weil `glyph_index` `None` liefert und der
                    // Zweig dann kommentarlos uebersprungen wird: im Bild
                    // fehlt der Buchstabe, in der Bilanz steht 0.
                    let g = match font.glyph_index(z) {
                        Some(g) => Some(g),
                        // DER NAME, zweiter Versuch. `fi` ist U+FB01 und
                        // steht nicht in jeder `cmap`; der Name `fi` steht
                        // im `charset` der Ersatzschrift.
                        None => r.glyphenname(code).and_then(|n| font.gid_von_name(&n)),
                    };
                    let g = match g {
                        Some(g) => g,
                        None => {
                            // added 05.10.2026: DAS **LEERZEICHEN**
                            // ZAEHLT MIT, und das ist eine bewusste
                            // Ungenauigkeit des Zaehlers: ein
                            // Leerzeichen hat in einer CFF keine Kontur,
                            // `glyph_index` gibt dafuer `None`, und der
                            // Zaehler nennt es damit „Fehlstelle“.
                            //
                            // GEMESSEN, warum das so entschieden wurde:
                            // die Zahl der Zeichen **ohne** Leerzeichen
                            // laesst sich aus dem Abgleich mit
                            // `NARSIL_PDF_DEBUG` (jede Fehlstelle wird
                            // dort mit Zeichen und Code gedruckt) nicht
                            // gewinnen, weil `bau.sh` in dieser Runde an
                            // `narsil-image` ROT war. Der Zaehler bleibt
                            // deshalb grob, und wer ihn braucht, liest
                            // die Debug-Ausgabe.
                            self.ersatz_fehl += 1;
                            if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
                                eprintln!(
                                    "FEHLSTELLE {:?} (code {code}) in {} — Name {:?}",
                                    z,
                                    r.basisname,
                                    r.glyphenname(code)
                                );
                            }
                            continue;
                        }
                    };
                    if let Some(g) = Some(g) {
                        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
                            eprintln!(
                                "ERSATZ {:?} gid={g} umriss={:?} schrift={}",
                                z,
                                font.umriss(g).map(|k| k.len()),
                                r.basisname
                            );
                        }
                        if let Ok(umriss) = font.umriss(g) {
                            // added 04.10.2026: `vorschub` gibt `None`
                            // bei einer CFF-Ersatzschrift, und dann wird
                            // **nicht** gestreckt — siehe `Ersatzschrift`.
                            let dehnung = match font.vorschub(g) {
                                Some(natuerlich) => (soll * upm / natuerlich).clamp(0.5, 2.0),
                                None => 1.0,
                            };
                            let gsx = gsx * dehnung;
                            for kontur in &umriss {
                                if kontur.len() < 2 {
                                    continue;
                                }
                                out.push(
                                    kontur
                                        .iter()
                                        .map(|p| {
                                            auf_seite(
                                                vorschub + p.0 as f64 * gsx,
                                                grundlinie + p.1 as f64 * gsy,
                                            )
                                        })
                                        .collect(),
                                );
                            }
                        }
                    }
                }
            }
            // DER VORSCHUB WIRD IMMER WEITERGEZOGEN — auch wenn nichts
            // gezeichnet wurde. Ohne das stapeln sich die Buchstaben
            // an **einer** Stelle, und der Absatz sieht aus wie eine
            // senkrechte Linie.
            vorschub += (breite / 1000.0) * groesse * horizont;
        }
        let _ = laufweite;
        out
    }
}

/// Wie viele Bytes ein gerendertes Bild haben darf (M4).
///
/// added 03.10.2026. 8 MiB. Ein 4000x4000-Bild als PNG ist ~1 MiB;
/// darueber ist es etwas anderes.
pub const MAX_PNG: usize = 8 * 1024 * 1024;

/// Codiere eine Flaeche als PNG (RGBA8).
///
/// added 03.10.2026. `narsil_image::png::png_schreiben` ist die Crate,
/// die im Projekt ueberall Bilder schreibt — ein eigener PNG-Encoder
/// waere eine zweite Wahrheit ueber dasselbe Format.
#[cfg(feature = "bilder")]
pub fn als_png(f: &Flaeche) -> Vec<u8> {
    use narsil_image::Bild;
    let bild = Bild {
        breite: f.fb.width(),
        hoehe: f.fb.height(),
        // `Framebuffer::pixels()` ist RGBA8, genau das Format von
        // `narsil_image::Bild`.
        rgba: f.fb.pixels().to_vec(),
    };
    let daten = narsil_image::png::png_schreiben(&bild);
    if daten.len() <= MAX_PNG {
        daten
    } else {
        // Zu gross: ein leeres Bild mit der richtigen Groesse. Der
        // Bericht sieht daran, dass etwas nicht passt — und es ist
        // besser als ein Absturz mitten im Lauf.
        Vec::new()
    }
}

/// Wie [`als_png`], aber ohne das Feature `bilder`.
///
/// added 03.10.2026. Es gibt genau **eine** Sorte PNG in diesem
/// Modul, und sie braucht `narsil-image`. Diese Variante gibt es nur,
/// damit die Crate ohne die Bild-Crate baut; sie gibt eine leere
/// Datei zurueck und **nicht** ein kaputtes Bild — wer sie aufruft,
/// sieht es am Dateigroesse-Null, nicht an einem zerstoerten Bild.
#[cfg(not(feature = "bilder"))]
pub fn als_png(_f: &Flaeche) -> Vec<u8> {
    Vec::new()
}

/// Zaehlt die Pixel, die sich vom Fuellton der Flaeche unterscheiden.
///
/// added 03.10.2026. Der Grund fuer diese Funktion: die Bilanz
/// zaehlte Schreibvorgaenge (siehe [`Seitenbild::schreibvorgaenge`]),
/// und damit war „auf dem Bild steht etwas" nicht beantwortbar.
///
/// DER PREIS: ein Durchgang ueber den Puffer, also 8 Byte je Pixel —
/// bei 1190 x 1684 rund 16 MiB gelesen. Das ist der Grund, warum die
/// Zahl erst **am Ende** gezaehlt wird und nicht beim Fuellen.
pub fn sichtbare_pixel(f: &Flaeche) -> usize {
    let px = f.fb.pixels();
    // Die Flaeche wird mit Weiss angelegt (siehe `flaeche_fuer`).
    let hintergrund = [255u8, 255, 255, 255];
    px.chunks_exact(4)
        .filter(|p| {
            p[0] != hintergrund[0]
                || p[1] != hintergrund[1]
                || p[2] != hintergrund[2]
                || p[3] != hintergrund[3]
        })
        .count()
}

/// Rendere eine Seite (Kurzform ohne Renderer-Objekt).
///
/// added 03.10.2026. Fuer Werkzeuge, die nur eine Seite brauchen.
pub fn seite_als_bild(
    dokument: &mut Dokument,
    seite: &Seite,
    opt: Rasteroptionen,
) -> Result<Seitenbild, RenderFehler> {
    Renderer::neu(dokument, opt).seite(seite)
}

/// Baut ein minimales PDF mit **berechneter** Querverweistabelle.
///
/// added 03.10.2026. DIE ERSTE FASSUNG DER DREI TESTS
/// (`xref::minimale_tabelle`, `dokument::eine_seite`,
/// `tests::minimale_seite_wird_png`) HATTE DIE OFFSETS GERATEN: 200,
/// 280 und 300 — bei allen drei war der Dateianhang falsch, und der
/// Parser meldete zu Recht „kein stream". Man suchte den Fehler dann
/// im Parser statt im Test.
///
/// WARUM EIN BAUER UND KEIN FESTER BYTE-STROM: die Offsets aendern
/// sich mit jeder Aenderung am Koerper, und ein geratener Offset ist
/// die Sorte Zahl, die drei Versionen lang „richtig" aussieht und
/// dann still falsch ist.
///
/// Der Inhalt des Contents-Streams (`re f` auf einem Rechteck) ist
/// fest: er zeichnet etwas, damit „gerendert" etwas bedeutet.
#[cfg(test)]
pub(crate) fn mini_pdf(media: [f64; 4]) -> Vec<u8> {
    let mut raus: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offs: Vec<usize> = Vec::new();
    let m = format!("[{} {} {} {}]", media[0], media[1], media[2], media[3]);
    let koerper: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox {m} /Contents 4 0 R >>"),
    ];
    for (i, dict) in koerper.iter().enumerate() {
        offs.push(raus.len());
        raus.extend_from_slice(format!("{} 0 obj\n{dict}\nendobj\n", i + 1).as_bytes());
    }
    let stream = b"0.9 0.9 0.9 rg 10 10 180 80 re f 0 0 0 rg";
    offs.push(raus.len());
    raus.extend_from_slice(
        format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n",
            stream.len(),
            String::from_utf8_lossy(stream)
        )
        .as_bytes(),
    );
    let tabellen_start = raus.len();
    raus.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in &offs {
        raus.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    raus.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{tabellen_start}\n%%EOF\n")
            .as_bytes(),
    );
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Release 09.10.2026: AUF WINDOWS GIBT ES ERSATZSCHRIFTEN — vorher stand
    /// auf `1706.03762.pdf` kein Buchstabe, weil nur Linux-Ordner gesucht
    /// wurden. Die Tabelle folgt `ersatz_index` (Serif normal = Times New
    /// Roman, Fest fett kursiv = Courier New fett kursiv).
    #[test]
    fn windows_ersatzschriften_folgen_dem_index() {
        assert_eq!(WINDOWS_ERSATZ[ersatz_index(1, false, false)][0], "times.ttf");
        assert_eq!(WINDOWS_ERSATZ[ersatz_index(0, true, false)][0], "arialbd.ttf");
        assert_eq!(WINDOWS_ERSATZ[ersatz_index(2, true, true)][0], "courbi.ttf");
        if cfg!(windows) {
            let ordner = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()));
            if ordner.join("Fonts").join("times.ttf").exists() {
                assert!(windows_ersatz(1, false, false).is_some(), "Times New Roman laedt nicht");
            }
        } else {
            assert!(windows_ersatz(1, false, false).is_none());
        }
    }

    /// Die Kategorien der **echten** `/BaseFont`-Namen aus den elf PDFs.
    ///
    /// added 04.10.2026. DER TEST FUER DIE ERSATZSCHRIFT, und die Liste
    /// ist **gemessen** (`mutool show … | grep '/BaseFont'` ueber alle
    /// elf Test-PDFs), nicht erfunden.
    ///
    /// DER FEHLER, den der Test haelt: `NimbusRomNo9L-Regu` ist ein
    /// **Times**-Klon aus der URW-Sammlung, und `ohne_praefix` liefert
    /// „nimbusromno9l-regu" — das enthaelt weder „roman" noch
    /// „serif". Die Funktion stufte den Namen als **serifenlos** ein, und
    /// damit bekam der gesamte Serifentext die Sans-Ersatzschrift.
    /// GEMESSEN: `ersatz_index` stand bei **allen** Schriften auf 0.
    ///
    /// GEMESSEN die Wirkung, jeweils 72 dpi, Toleranz 32, gegen
    /// `pdftoppm`, erste drei Seiten:
    ///
    ///     1512.03385   85,9 % -> 88,0 %
    ///     2103.00020   84,6 % -> 86,7 %
    ///     1706.03762   89,7 % -> 91,1 %
    ///     rfc2616      93,9 % -> 93,9 %  (unveraendert: dort steht
    ///                                          `Times-Roman` im
    ///                                          Blatt, und das war
    ///                                          schon richtig)
    ///     rfc9110      94,4 % -> 94,4 %  (unveraendert: der Text ist
    ///                                          eingebettetes TrueType)
    #[test]
    fn kategorien_der_echten_schriftnamen() {
        for (name, erwartet) in [
            // Serif
            ("XORMUP+NimbusRomNo9L-Medi", 1),
            ("CCHXUK+NimbusRomNo9L-Regu", 1),
            ("RRPAQA+NimbusRomNo9L-ReguItal", 1),
            ("Times-Roman", 1),
            ("TimesNewRomanPSMT", 1),
            ("NotoSerif-Regular2", 1),
            ("YRTIFL+NotoSerif-Bold", 1),
            ("XICQZS+CMR10", 1),
            ("YYBXCM+CMMI10", 1),
            ("C059-Roman", 1),
            ("P052-Roman", 1),
            // Fest
            ("JQUDPS+NimbusMonL-Regu", 2),
            ("NimbusMonoPS-Regular", 2),
            ("CourierNewPSMT", 2),
            ("MUFUZY+RobotoMono-Regular", 2),
            ("XNPTFK+RobotoMono-Regular", 2),
            // Serifenlos
            ("DejaVuSans", 0),
            ("IMFXLT+DejaVuSans", 0),
            ("Helvetica", 0),
            ("HelveticaLTStd-Bold", 0),
            ("XWOYTY+HelveticaNeueLTStd-BdOu", 0),
            ("MUFUZY+ArialMT", 0),
            ("MUFUZY+Arial-BoldMT", 0),
            ("TNCACH+NotoSansCJKjp-Regular", 0),
        ] {
            assert_eq!(
                kategorie_von(name),
                erwartet,
                "{name} wird als Kategorie {} eingestuft, erwartet {erwartet}",
                kategorie_von(name)
            );
        }
    }

    /// Und die Schnitte: **fett** und **kursiv** muessen getrennt sein.
    #[test]
    fn schnitte_der_echten_schriftnamen() {
        for (name, fett, kursiv) in [
            ("NimbusRomNo9L-Regu", false, false),
            ("NimbusRomNo9L-Medi", true, false),
            ("NimbusRomNo9L-ReguItal", false, true),
            ("NimbusRomNo9L-BoldItal", true, true),
            ("Times-Bold", true, false),
            ("Times-Italic", false, true),
            ("Helvetica-BoldOblique", true, true),
            ("CourierNewPS-BoldItalic", true, true),
            // „Oblique" ist kursiv, „Medium" ist **nicht** fett — das
            // war der Fehler, der bei `rfc2616.pdf` fuer Fettsatz sorgte.
            ("Arial-BoldMT", true, false),
            ("ArialMT", false, false),
        ] {
            let (kat, f, k) = schnitt_von(name);
            assert_eq!((f, k), (fett, kursiv), "{name} liefert fett={f} kursiv={k}");
            assert!(kat <= 2, "{name} liefert die Kategorie {kat}");
        }
    }

    /// Die kleinste moegliche Seite wird gerendert.
    ///
    /// added 03.10.2026. Der Test prueft **nicht** das Aussehen (das
    /// waere ein Bildvergleich, siehe `tests/abnahme.rs`), sondern dass
    /// die Kette vom Byte bis zum PNG laeuft — und dass auf dem Bild
    /// **etwas** steht: `gefuellte_pixel > 0`.
    // added 03.10.2026: Der Test ist an das Feature `bilder`
    // gebunden. Ohne das Feature gibt es **kein** PNG, und die
    // Erwartung „PNG-Magic" waere eine Behauptung ueber eine
    // Faehigkeit, die man abgeschaltet hat.
    /// added 04.10.2026. DIE BRUECKE ZUM ZOOM DER OBERFLAECHE.
    ///
    /// GEMESSEN an `1512.03385.pdf` Seite 1 (612 x 792 Punkte):
    /// Zoom 1,0 -> 612 x 792 Pixel; Zoom 0,5 -> 306 x 396. Ohne den
    /// genauen zweiten Wert waere „dpi = 72 * faktor" eine Behauptung.
    #[cfg(feature = "bilder")]
    #[test]
    fn zoom_stufe_gibt_die_punkte_pixel() {
        let pdf = crate::mini_pdf([0.0, 0.0, 200.0, 100.0]);
        let mut d = Dokument::oeffnen(pdf).expect("minimales PDF");
        let seiten = d.seiten().expect("Seitenbaum");
        let gross =
            seite_als_bild(&mut d, &seiten[0], Rasteroptionen::mit_zoom(1.0)).expect("Zoom 1,0");
        assert_eq!((gross.breite, gross.hoehe), (200, 100));
        let klein =
            seite_als_bild(&mut d, &seiten[0], Rasteroptionen::mit_zoom(0.5)).expect("Zoom 0,5");
        assert_eq!((klein.breite, klein.hoehe), (100, 50));
    }

    /// added 04.10.2026. DIE GRENZEN, und sie sind die der Leiste:
    /// `MIN_ZOOM` 25 % und `MAX_ZOOM` 500 % aus `narsil-chrome`.
    #[test]
    fn zoom_faktor_bleibt_in_den_oberflaechengrenzen() {
        assert_eq!(Rasteroptionen::mit_zoom(1.0).dpi, 72.0);
        assert_eq!(Rasteroptionen::mit_zoom(0.25).dpi, 18.0, "MIN_ZOOM");
        assert_eq!(Rasteroptionen::mit_zoom(5.0).dpi, 360.0, "MAX_ZOOM");
        // AUSSERHALB: geklemmt, nicht geraten.
        assert_eq!(Rasteroptionen::mit_zoom(9.0).dpi, 360.0);
        assert_eq!(Rasteroptionen::mit_zoom(0.01).dpi, 18.0);
        // added 04.10.2026: `NaN` ergibt sonst eine Flaeche von 0 x 0.
        assert_eq!(Rasteroptionen::mit_zoom(f32::NAN).dpi, DPI_STANDARD);
        assert_eq!(Rasteroptionen::mit_zoom(f32::INFINITY).dpi, DPI_STANDARD);
        assert_eq!(Rasteroptionen::mit_zoom(-1.0).dpi, DPI_STANDARD);
    }

    /// added 05.10.2026. DIE RECHNUNG FUER DAS ZIELREECHTECK.
    ///
    /// Die drei Faelle, an denen sich die Rechnung von `mit_zoom`
    /// unterscheidet, sind dieselben wie im Doc-Kommentar: US-Letter im
    /// Fenster 1224 x 1584 ergibt 144 dpi, dasselbe Fenster doppelt so
    /// gross 288 dpi, und bei A4 entscheidet die **kleinere** Achse.
    ///
    /// GEMESSEN an `1706.03762.pdf` Seite 1 (595 x 842 Punkte) bei 144 dpi:
    /// 1 190 x 1 684 Pixel. Das passt in 1224 x 1584, ist also **schon zu
    /// gross** fuer „Einpassen“ — der Betrachter verkleinert es anschliessend
    /// auf 1 146 x 1 584, und damit um etwa 4 % gegenueber dem Raster,
    /// das er eigentlich brauchte.
    #[test]
    fn fuer_flaeche_rechnet_aus_dem_ziel() {
        // US-Letter, Fenster wie der Betrachter bei Zoom 1.0
        let o = Rasteroptionen::fuer_flaeche([0.0, 0.0, 612.0, 792.0], [1224, 1584]);
        assert!(
            (o.dpi - 144.0).abs() < 0.01,
            "1224 / (612/72) = 144 dpi, bekam {}",
            o.dpi
        );
        // Zoom 2: dasselbe Fenster doppelt so gross
        let o = Rasteroptionen::fuer_flaeche([0.0, 0.0, 612.0, 792.0], [2448, 3168]);
        assert!((o.dpi - 288.0).abs() < 0.01, "bekam {}", o.dpi);
        // added 05.10.2026: DIE **KLEINERE** ACHSE GEWINNT. A4 hochkant
        // in ein quadratisches Fenster: die Breite 1224 braucht 148 dpi,
        // die Hoehe 1224 nur 104,6 — gewaehlt wird die kleinere, damit die
        // Seite **vollstaendig** ins Fenster passt.
        let o = Rasteroptionen::fuer_flaeche([0.0, 0.0, 595.0, 842.0], [1224, 1224]);
        assert!(
            (o.dpi - 1224.0 / (842.0 / 72.0)).abs() < 0.01,
            "die kleinere Achse muss gewinnen, bekam {}",
            o.dpi
        );
        // Obergrenze 360 dpi: ein Riesenfenster wird geklemmt, statt ein
        // Bild von 50 MB zu bauen.
        let o = Rasteroptionen::fuer_flaeche([0.0, 0.0, 612.0, 792.0], [10_000, 12_000]);
        assert_eq!(o.dpi, 360.0, "Obergrenze");
        // Untergrenze 18 dpi
        let o = Rasteroptionen::fuer_flaeche([0.0, 0.0, 612.0, 792.0], [40, 50]);
        assert_eq!(o.dpi, 18.0, "Untergrenze");
        // kaputte Eingaben: kein leeres Bild, sondern die Vorgabe
        for (media, ziel) in [
            ([0.0f64; 4], [1224u32, 1584]),
            ([0.0, 0.0, 612.0, 792.0], [0, 1584]),
            ([0.0, 0.0, f64::NAN, 792.0], [1224, 1584]),
        ] {
            let o = Rasteroptionen::fuer_flaeche(media, ziel);
            assert_eq!(
                o.dpi,
                Rasteroptionen::standard().dpi,
                "media {media:?} ziel {ziel:?} muss die Vorgabe liefern"
            );
        }
    }

    /// added 04.10.2026. DIE ZAHL, MIT DER DER ZOOM-KNOPF AUFGEHT.
    ///
    /// GEMESSEN: A4 (595 x 842) laesst `4.75` zu; ein Zehntel mehr
    /// scheitert mit `ZuGross`, und **nicht** mit einem still
    /// beschnittenen Bild. Genau das ist der Punkt des Tests.
    #[test]
    fn groesster_zoom_sagt_die_grenze_und_hält_sie() {
        let a4 = Rasteroptionen::groesster_zoom([0.0, 0.0, 595.0, 842.0]);
        assert!(
            (a4 - 4.75).abs() < 0.01,
            "A4 laesst 4000/842 = 4.75 zu, nicht {a4}"
        );
        // Querformat: die **laengere** Kante zaehlt.
        assert_eq!(Rasteroptionen::groesster_zoom([0.0, 0.0, 842.0, 595.0]), a4);
        // Ein kaputtes MediaBox liefert keine Zahl, sondern 1.0.
        assert_eq!(Rasteroptionen::groesster_zoom([0.0; 4]), 1.0);
        // added 04.10.2026: UND DIE GRENZE WIRD WIRKLICH EINGEHALTEN.
        let pdf = crate::mini_pdf([0.0, 0.0, 595.0, 842.0]);
        let mut d = Dokument::oeffnen(pdf).expect("minimales PDF");
        let seiten = d.seiten().expect("Seitenbaum");
        let passt = seite_als_bild(&mut d, &seiten[0], Rasteroptionen::mit_zoom(a4));
        assert!(
            passt.is_ok(),
            "Zoom {a4} muss noch passen, kam aber: {:?}",
            passt.err()
        );
        let zu_gross = seite_als_bild(&mut d, &seiten[0], Rasteroptionen::mit_zoom(a4 + 0.1));
        assert!(
            matches!(zu_gross, Err(RenderFehler::ZuGross(_, _))),
            "eine Zehntelstufe mehr muss FEHLER sagen, nicht schweigen: {:?}",
            zu_gross.map(|b| (b.breite, b.hoehe))
        );
    }

    #[cfg(feature = "bilder")]
    #[test]
    fn minimale_seite_wird_png() {
        let pdf = crate::mini_pdf([0.0, 0.0, 200.0, 100.0]);
        let Ok(mut d) = Dokument::oeffnen(pdf) else {
            panic!("minimales PDF laesst sich nicht oeffnen");
        };
        let seiten = d.seiten().expect("Seitenbaum");
        assert_eq!(seiten.len(), 1);
        let b = seite_als_bild(&mut d, &seiten[0], Rasteroptionen::standard())
            .expect("Seite laesst sich nicht rendern");
        assert!(b.png.starts_with(b"\x89PNG"), "kein PNG");
        assert!(b.breite > 0 && b.hoehe > 0);
        // DER WICHTIGE TEIL: eine Seite mit 0 Pixeln ist gerendert und
        // trotzdem kaputt. Ohne diese Zeile waere der Test gruen bei
        // einem leeren Bild.
        assert!(
            b.sichtbare_pixel > 0,
            "Bild ohne einen einzigen gesetzten Pixel ({breite}x{hoehe})",
            breite = b.breite,
            hoehe = b.hoehe
        );
        println!(
            "minimale Seite: {}x{} Pixel, {} Fuellpixel",
            b.breite, b.hoehe, b.sichtbare_pixel
        );
    }

    /// Ohne das Feature `bilder`: die Seite wird gerendert, aber es
    /// entsteht **keine** Datei.
    ///
    /// added 03.10.2026. Das ist kein Notbehaufen, sondern eine
    /// Zusicherung: wer das Feature abschaltet, bekommt leere Bytes
    /// und keine halbe Datei. Und der Weg laeuft trotzdem bis zum
    /// Fuellen — sonst wuerde der Test nur `Dokument::oeffnen` pruefen.
    #[cfg(not(feature = "bilder"))]
    #[test]
    fn ohne_bilder_keine_datei_aber_pixels() {
        let pdf = crate::mini_pdf([0.0, 0.0, 200.0, 100.0]);
        let Ok(mut d) = Dokument::oeffnen(pdf) else {
            panic!("minimales PDF laesst sich nicht oeffnen");
        };
        let seiten = d.seiten().expect("Seitenbaum");
        let b = seite_als_bild(&mut d, &seiten[0], Rasteroptionen::standard())
            .expect("Seite laesst sich nicht rendern");
        assert!(b.png.is_empty(), "ohne bilder darf keine Datei entstehen");
        assert!(
            b.sichtbare_pixel > 0,
            "aber es müssen Pixel gesetzt worden sein"
        );
    }

    /// Ein Byte kaputt ist ein Fehler, kein Absturz.
    ///
    /// added 03.10.2026. Der Fuzzer-Ziel-Test in `tests/` ist der
    /// breite Fall; hier steht der kleinste: `%PDF-` und sonst nichts.
    #[test]
    fn panik_fuer_schrott() {
        assert!(Dokument::oeffnen(b"%PDF-1.7".to_vec()).is_err());
        assert!(Dokument::oeffnen(Vec::new()).is_err());
    }
}
