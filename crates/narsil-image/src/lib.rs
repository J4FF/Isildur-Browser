//! narsil-image — Bilder lesen und schreiben: PNG, PPM, BMP.
//!
//! WAS DIESE CRATE KANN (Stand 29.09.2026):
//!   * PNG SCHREIBEN: 8 Bit Graustufen, 8 Bit RGB, 8 Bit RGBA, echter
//!     zlib-Strom mit DEFLATE (feste Huffman-Tabelle, LZ77 mit Fenster).
//!   * PNG LESEN: zlib/INFLATE nach RFC 1950/1951 (gespeicherte, feste und
//!     DYNAMISCHE Huffman-Bloecke), Filter 0-4, Farbtypen Graustufen 1/2/4/8,
//!     Palette 1/2/4/8, RGB 8, RGBA 8, tRNS.
//!   * PPM LESEN: P1/P2/P3 (ASCII) und P4/P5/P6 (binaer), 8 und 16 Bit.
//!   * BMP LESEN: BI_RGB, 24 und 32 Bit, Zeilen auf 4 Byte ausgerichtet,
//!     negative Hoehe = von oben nach unten.
//!   * `skalieren` (naechster Nachbar) und `erkenne`.
//!
//! WAS DIESE CRATE NICHT KANN, und warum das ein FEHLER ist und kein
//! Absturz: Interlace (Adam7), 16 Bit pro Kanal, RLE- oder
//! Adobe-Flate-Bitmaps. Jeder dieser Faelle ergibt einen BENANNTEN
//! `BildFehler`; siehe [`BildFehler`].
//!
//! -------------------------------------------------------------------------
//! WARUM KEINE ABHAENGIGKEIT (ADR-030).
//! Kein `image`, kein `flate2`, kein `png`. Diese Crate rechnet nur mit
//! Zahlen und Bytes und haengt an keinem `narsil-*`. Zwei Gruende:
//!   1. Sie muss ohne den Rest des Projekts pruefbar sein — ein Bildformat
//!      laesst sich vollstaendig gegen die Spezifikation nachrechnen.
//!   2. Jede Abhaengigkeit, die ich nicht selbst geschrieben habe, ist eine
//!      Fremdentscheidung ueber Sicherheit, Groessen und Formate. Fuer einen
//!      Browser ist das ein Risiko, kein Komfort.
//! Also: CRC32, Adler32, INFLATE und DEFLATE liegen hier, aus RFC 1950/1951
//! und RFC 2083 nachgerechnet. Das sind rund 900 Zeilen, und jede ist getestet.
//!
//! -------------------------------------------------------------------------
//! SICHERHEIT. Jedes Byte, das von aussen kommt, ist erst einmal feindlich.
//! Diese Crate paniked deshalb nicht:
//!   * Alle Laengen aus der Datei werden vor der Allokation geprueft
//!     ([`MAX_KANTE`], [`MAX_BILDPUNKTE`]).
//!   * Der INFLATE-Schreibzeiger laeuft gegen eine HARTE Obergrenze (die
//!     erwartete Bildgroesse). Ein „Endlosschleifendekomprimieren" kann
//!     nicht 40 GB anfordern.
//!   * `png_schreiben` liest das `Bild` des Aufrufers mit Klemmung: ein
//!     zu kurzes `rgba` liefert Nullen, keine Panik.
//! Kein `unsafe` in der ganzen Crate (`unsafe_code = "forbid"`).

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------
pub mod avif;
pub mod bildweg;
pub mod bmp;
pub mod cache;
pub mod deflate;
pub mod gif;
pub mod icc;
pub mod ico;
pub mod inflate;
pub mod jpeg;
pub mod kachel;
pub mod msac;
pub mod platzierung;
pub mod png;
pub mod ppm;
pub mod pruefsummen;
pub mod skalieren;
pub mod svg;
pub mod vp8;
pub mod vp8_tabellen;
pub mod vp8l;
pub mod webp;
pub mod werkzeuge;
pub mod zlib;

pub use bildweg::{
    Auswahl, Formate, Kandidat, MAX_BREITE, MAX_KANDIDATEN, MAX_SIZES_TEXT, PictureZeile, Quelle,
    nach_breite, picture_waehlen, platz_breite,
};
pub use bmp::bmp_lesen;
pub use cache::{BilderCache, Kennzahlen};
pub use gif::{
    Entsorgung, Gif, GifFrame, MAX_FRAMES, MAX_PIXEL_ALLE_BILDER, gif_gesamtdauer_ms,
    gif_hintergrund, gif_lesen, gif_lesen_alle,
};
pub use ico::{
    Eintrag, EintragsArt, MAX_EINTRAEGE, ico_eintrag_lesen, ico_lesen, ico_lesen_groesse,
    ico_verzeichnis,
};
pub use jpeg::{Verkleinerung, jpeg_lesen, jpeg_lesen_mit, jpeg_masse, jpeg_orientierung, jpeg_schreiben};
pub use kachel::{Kachelplan, MAX_KACHELN, Wiederholung, kachelplan};
pub use platzierung::{
    Angabe, Anker, Bildposition, MAX_KASTEN, MAX_LAGE, MAX_PASSUNG_TEXT, MAX_POSITION_TEXT,
    Passung, Rechteck, benutzte_groesse, breite_aus_hoehe, groesse_erklaeren, groesse_ersetzt,
    hoehe_aus_breite, passung_lesen, platziere_bild, platziere_bild_mit_schnitt, position_lesen,
    rechteck_fuer_bild, rechteck_in_kasten, runde_px, seitenverhaeltnis,
};
pub use png::{Farbwahl, png_lesen, png_schreiben, png_schreiben_modus, zlib_ohne_kompression};
pub use ppm::ppm_lesen;
pub use svg::{
    Auflosbar, MAX_ELEMENTE as MAX_SVG_ELEMENTE, MAX_KANTE as MAX_SVG_KANTE,
    SUBSAMPLE as SVG_SUBSAMPLE, Svg, Weggelassen, fuellung_aufloesbar, svg_lesen,
    svg_lesen_mit_notizen, svg_rasteren,
};
pub use webp::{
    MAX_CACHE_BITS, MAX_TRANSFORMS, MAX_WEBP_BILDER, Webp, WebpArt, WebpEntsorgung, WebpFrame,
    webp_chunks, webp_lesen, webp_lesen_alle,
};
pub use werkzeuge::{erkenne, lesen, lesen_klein, skalieren};

/// Farbwahl beim PNG-Schreiben: 8 Bit Graustufen, 8 Bit RGB, 8 Bit RGBA.
///
/// Warum ein eigener Typ: das `Bild` des Projekts ist immer RGBA8. Ein
/// 8-Bit-Graustufen-PNG ist aber rund ein Drittel so gross wie RGBA, und auf
/// einem Screenshot ist der Unterschied sofort sichtbar. Die Entscheidung
/// liegt beim Aufrufer; `Farbwahl::Auto` entscheidet selbst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farbmodus {
    /// 8 Bit Graustufen (PNG-Farbtyp 0). Der Rotkanal wird geschrieben.
    Graustufen,
    /// 8 Bit RGB (PNG-Farbtyp 2). Der Alphakanal wird verworfen.
    Rgb,
    /// 8 Bit RGBA (PNG-Farbtyp 6). Verlustfrei aus dem `Bild`.
    Rgba,
}

// ---------------------------------------------------------------------------
// Bild
// ---------------------------------------------------------------------------

/// Ein Bild in RGBA8, zeilenweise von oben nach unten.
///
/// Die Invariante `rgba.len() == breite * hoehe * 4` wird von [`Bild::new`]
/// hergestellt. Sie kann durch `Vec::truncate` verletzt werden, weil
/// `rgba` oeffentlich ist — genau deshalb liest der Enkoder mit Klemmung
/// und rennt nie in einen Indexfehler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bild {
    pub breite: u32,
    pub hoehe: u32,
    /// RGBA8, 4 Byte pro Pixel, zeilenweise von oben nach unten.
    pub rgba: Vec<u8>,
}

impl Bild {
    /// Neues, mit [`TRANSPARENT`](crate::Bild::TRANSPARENT) gefuelltes Bild.
    ///
    /// Kein `unsafe`, keine Panik: zu grosse Werte werden auf die Grenze
    /// geklemmt, weil `Vec::with_capacity` bei einem 100000x100000-Bild
    /// sonst den Prozess abbricht.
    pub fn new(breite: u32, hoehe: u32) -> Bild {
        let b = breite.min(MAX_KANTE);
        let h = hoehe.min(MAX_KANTE);
        let n = (b as usize).saturating_mul(h as usize).saturating_mul(4);
        Bild {
            breite: b,
            hoehe: h,
            rgba: vec![0; n.min(MAX_SPEICHER)],
        }
    }

    /// Fuellt das ganze Bild mit einer Farbe.
    pub fn aus_farbe(breite: u32, hoehe: u32, farbe: [u8; 4]) -> Bild {
        let mut b = Bild::new(breite, hoehe);
        for p in b.rgba.chunks_exact_mut(4) {
            p.copy_from_slice(&farbe);
        }
        b
    }

    /// Vollstaendige Transparenz. Als Konstante, weil sie der Fuellwert von
    /// [`Bild::new`] ist und das an drei Stellen im Code entschieden wird.
    pub const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

    /// Ein Pixel als `[r, g, b, a]`. Ausserhalb des Bildes: `[0,0,0,0]`.
    ///
    /// Kein Panic: der Aufrufer darf fragen, ohne zu pruefen — das ist der
    /// Normalfall bei Clipping-Grenzen im Browser.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.breite || y >= self.hoehe {
            return Bild::TRANSPARENT;
        }
        let i = ((y as usize) * (self.breite as usize) + (x as usize)) * 4;
        match self.rgba.get(i..i + 4) {
            Some(v) => [v[0], v[1], v[2], v[3]],
            None => Bild::TRANSPARENT,
        }
    }

    /// Setzt einen Pixel. Ausserhalb des Bildes passiert nichts.
    pub fn setze_pixel(&mut self, x: u32, y: u32, farbe: [u8; 4]) {
        if x >= self.breite || y >= self.hoehe {
            return;
        }
        let i = ((y as usize) * (self.breite as usize) + (x as usize)) * 4;
        if i + 4 <= self.rgba.len() {
            self.rgba[i..i + 4].copy_from_slice(&farbe);
        }
    }

    /// Fuellt ein Rechteck mit `farbe`. Ausserhalb des Bildes wird
    /// abgeschnitten, nichts ueberschrieben.
    ///
    /// M3/M4: `x + w` und `y + h` sind mit `saturating_add` gerechnet,
    /// damit ein riesiges `w` nicht ueberlaeuft.
    pub fn feld_aufraeumen(&mut self, x: u32, y: u32, w: u32, h: u32, farbe: [u8; 4]) {
        if w == 0 || h == 0 || x >= self.breite || y >= self.hoehe {
            return;
        }
        let x_ende = (x.saturating_add(w)).min(self.breite);
        let y_ende = (y.saturating_add(h)).min(self.hoehe);
        for yy in y..y_ende {
            let zeilen_start = ((yy as usize) * (self.breite as usize)) * 4;
            let von = zeilen_start + (x as usize) * 4;
            let bis = zeilen_start + (x_ende as usize) * 4;
            if bis > self.rgba.len() {
                break;
            }
            for p in self.rgba[von..bis].chunks_exact_mut(4) {
                p.copy_from_slice(&farbe);
            }
        }
    }

    /// Kopiert `quelle` mit Versatz `(x, y)` in dieses Bild und ersetzt
    /// die Pixel dabei (kein Mischen). Was ausserhalb liegt, wird
    /// abgeschnitten.
    pub fn ersetzen(&mut self, quelle: &Bild, x: u32, y: u32) {
        if quelle.breite == 0 || quelle.hoehe == 0 {
            return;
        }
        for yy in 0..quelle.hoehe {
            let ziel_y = y.saturating_add(yy);
            if ziel_y >= self.hoehe {
                break;
            }
            for xx in 0..quelle.breite {
                let ziel_x = x.saturating_add(xx);
                if ziel_x >= self.breite {
                    break;
                }
                let q = ((yy as usize) * (quelle.breite as usize) + (xx as usize)) * 4;
                if q + 4 > quelle.rgba.len() {
                    break;
                }
                self.setze_pixel(
                    ziel_x,
                    ziel_y,
                    [
                        quelle.rgba[q],
                        quelle.rgba[q + 1],
                        quelle.rgba[q + 2],
                        quelle.rgba[q + 3],
                    ],
                );
            }
        }
    }

    /// Legt `quelle` mit Versatz `(x, y)` gemischt ueber dieses Bild:
    /// Quell-Alpha 0 laesst den Zielpixel unberuehrt, 255 ersetzt ihn
    /// ganz. Die Mischung rechnet in ganzen Bytern:
    ///
    /// ```text
    /// a = (sa * 255 + 127) / 255
    /// z = (sz * (255 - a) + q * a + 127) / 255
    /// ```
    ///
    /// Das ist die Formel, die auch die GIF-Zusammensetzung in
    /// `crate::gif` benutzt — beide Stellen muessen dasselbe rechnen,
    /// sonst unterschieden sich GIF und WebP an derselben Kante.
    pub fn ueberlagern(&mut self, quelle: &Bild, x: u32, y: u32) {
        if quelle.breite == 0 || quelle.hoehe == 0 {
            return;
        }
        for yy in 0..quelle.hoehe {
            let ziel_y = y.saturating_add(yy);
            if ziel_y >= self.hoehe {
                break;
            }
            for xx in 0..quelle.breite {
                let ziel_x = x.saturating_add(xx);
                if ziel_x >= self.breite {
                    break;
                }
                let q = ((yy as usize) * (quelle.breite as usize) + (xx as usize)) * 4;
                if q + 4 > quelle.rgba.len() {
                    break;
                }
                let sa = quelle.rgba[q + 3] as u32;
                if sa == 0 {
                    continue;
                }
                let zi = ((ziel_y as usize) * (self.breite as usize) + (ziel_x as usize)) * 4;
                if zi + 4 > self.rgba.len() {
                    break;
                }
                if sa == 255 {
                    self.rgba[zi..zi + 4].copy_from_slice(&quelle.rgba[q..q + 4]);
                    continue;
                }
                let a = (sa * 255 + 127) / 255;
                for k in 0..3 {
                    let sz = self.rgba[zi + k] as u32;
                    let qv = quelle.rgba[q + k] as u32;
                    self.rgba[zi + k] = ((sz * (255 - a) + qv * a + 127) / 255) as u8;
                }
                // Das Ziel-Alpha wächst nach der Quell-Deckkraft.
                let za = self.rgba[zi + 3] as u32;
                self.rgba[zi + 3] = (za + ((255 - za) * a + 127) / 255) as u8;
            }
        }
    }

    /// Bytezahl, die ein perfekt auf dieses Bild passendes PNG braucht.
    /// Nur als Groessenabschaetzung fuer Tests und Logs gedacht.
    pub fn bytezahl_png(&self) -> usize {
        let px = (self.breite as usize) * (self.hoehe as usize);
        let zeilen = px * 3 + 1; // RGB8 + Filterbyte
        8 + 25 + 12 + zeilen + 12 // Signatur + IHDR + IEND
    }
}

// ---------------------------------------------------------------------------
// Grenzen
// ---------------------------------------------------------------------------

/// Hoechste Kantenlaenge in EINER Achse, in Pixel.
///
/// Das ist eine KOSTENGRENZE, kein Sicherheitsversprechen (dieselbe Art wie
/// `MAX_FONT_SIZE`): eine PNG-Datei, die "100000x100000" behauptet, darf
/// keine 40 GB anfordern. 16384 ist gewaehlt, weil das die groesste
/// Oberflaeche ist, die der Browser heute ueberhaupt darstellen kann.
pub const MAX_KANTE: u32 = 16384;

/// Hoechste Pixelzahl eines Bildes (Punkte, nicht Bytes).
///
/// Begruendung: 16384 x 16384 waeren 268 Millionen Punkte und damit 1 GiB
/// RGBA — das ist eine Zahl, die ein Tab auf einmal anfaedt, ohne dass die
/// Laenge der Datei etwas damit zu tun hat. 64 Millionen Punkte bleiben bei
/// 256 MiB, und das ist eine Kopie, die der Browser auch im schlimmsten Fall
/// noch tragen kann. Ueber beiden Grenzen derselbe benannte Fehler, damit
/// der Aufrufer nicht zwei Faelle unterscheiden muss.
pub const MAX_BILDPUNKTE: u64 = 64 * 1024 * 1024;

/// Hoechste Zahl RGBA-Bytes, die diese Crate je belegt: 256 MiB.
pub const MAX_SPEICHER: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// BildFehler
// ---------------------------------------------------------------------------

/// Warum genau ein Bild nicht geladen werden konnte.
///
/// Jede Variante ist BENANNT. Das ist der ganze Punkt: ein Browser, der
/// ein kaputtes Bild nicht anzeigt, soll dem Menschen sagen warum, und das
/// kann er nur, wenn der Fehler unterscheidbar ist. `Sonstiges(String)`
/// waere eine Sackgasse — kein Fall kann abgegrenzt werden, kein Test kann
/// rot werden, und jeder Aufrufer muesste raten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BildFehler {
    // ---- gemeinsam, alle Formate ----
    /// Kein bekanntes Bildformat erkannt (`erkenne` gab `None`).
    KeinBildformat,
    /// Die Eingabe ist kuerzer, als das kleinste denkbare Format braucht.
    ZuKurz { vorhanden: usize, gebraucht: usize },
    /// Breite oder Hoehe ist 0. Ein 0x0-Bild hat keine Zeile, aber die
    /// Datei behauptet, eine zu haben — das ist kaputt, nicht leer.
    GroesseNull,
    /// Die Datei behauptet mehr Punkte, als die Grenze erlaubt.
    ///
    /// `kante` ist [`MAX_KANTE`] oder [`MAX_BILDPUNKTE`], je nachdem, was
    /// zuerst gerissen ist.
    ZuGross { breite: u32, hoehe: u32, kante: u32 },
    /// Die Datei endet mitten in einem Block, den sie selbst angekündigt hat.
    Abgeschnitten { position: usize, gebraucht: usize },

    // ---- PNG: Container ----
    /// Die acht Bytes `\x89PNG\r\n\x1a\n` fehlen.
    KeineSignatur,
    /// IHDR fehlt oder ist nicht der erste Chunk.
    KeinIhdr,
    /// IHDR muss genau 13 Byte lang sein (RFC 2083 Abschnitt 4.1).
    IhdrLaengeFalsch { gefunden: u32 },
    /// Der CRC eines Chunks stimmt nicht. `erwartet` steht im Chunk,
    /// `gefunden` ist ueber die Bytes gerechnet.
    CrcFalsch {
        typ: [u8; 4],
        position: usize,
        erwartet: u32,
        gefunden: u32,
    },
    /// Ein Chunk-Typ, den es nicht gibt, dessen Name aber KRITISCH ist
    /// (Grossbuchstabe). Kritische Chunks darf man laut Spezifikation
    /// nicht uebergehen — der Leser hat sie nicht verstanden.
    UnbekannterChunk { typ: [u8; 4], position: usize },
    /// Ein Chunk-Typ mit ungueltigen Zeichen (kein A-Z, kein a-z, kein 0-9).
    UngueltigerChunkTyp { typ: [u8; 4], position: usize },
    /// Kein IDAT gesehen, bevor IEND kam.
    IdatFehlt,
    /// IEND fehlt (die Datei ist zu Ende, ohne dass sie sich beendet hat).
    IendFehlt,

    // ---- PNG: Bildaufbau ----
    /// Interlace-Verfahren ungleich 0 (Adam7 ist nicht implementiert).
    Interlace { verfahren: u8 },
    /// 16 Bit pro Kanal. Bewusst nicht implementiert: der Browser kann
    /// keine 16-Bit-Pipeline zeichnen, also muesste der Decoder still
    /// herunterrechnen — und das waere eine stille Veraenderung des Bildes.
    SechzehnBit { farbtyp: u8 },
    /// Farbtyp ungleich 0, 2, 3, 4 oder 6.
    UnbekannterFarbtyp { wert: u8 },
    /// Bittiefe passt nicht zum Farbtyp (oder ist un bekannt).
    UnbekannteBittiefe { farbtyp: u8, tiefe: u8 },
    /// Kompressionsverfahren oder Filtermethode ungleich 0.
    UnbekanntesVerfahren { kompression: u8, filter: u8 },
    /// Farbtyp 3 ohne PLTE, oder PLTE mit 0, 3 oder mehr als 256 Einträgen.
    PaletteFalsch { gefunden: usize },
    /// Nach dem Entpacken fehlen Zeilen. Die Datei hat weniger geliefert,
    /// als ihr eigener Header verlangt.
    ZuWenigDaten { erwartet: usize, gefunden: usize },
    /// Mehr Zeilen entpackt als der Header erlaubt. Klassischer Angriff:
    /// ein winziges Bild, das eine Riesenmenge entpackt.
    EntpacktZuViel { erwartet: usize },
    /// Unbekannter Filtertyp in einer Bildzeile (nur 0-4 sind gueltig).
    FilterUnbekannt { typ: u8, zeile: usize },
    /// tRNS passt nicht zum Farbtyp oder nicht zur Bittiefe.
    TrnsFalsch { gefunden: usize },
    /// Der zlib-Strom ist defekt. Der Grund liegt immer im Unterfehler.
    Zlib(FehlerZlib),

    // ---- PPM ----
    /// Die ersten zwei Byte sind nicht `P1`..`P6`.
    PpmKopfFalsch { gefunden: u8 },
    /// PPM: maxval ist 0 oder groesser als 65535.
    PpmMaxvalFalsch { maxval: u32 },
    /// In den Headerwerten steht dort, wo eine Zahl stehen sollte, keine.
    PpmZahlFalsch { position: usize },

    // ---- BMP ----
    /// Der BMP-Kopf oder der DIB-Header passt nicht.
    BmpKopfFalsch { grund: &'static str },
    /// Die BMP-Variante (Bittiefe, Kompression) wird nicht gelesen.
    BmpVariante { grund: &'static str },

    // ---- ICC ----
    /// Das eingebettete ICC-Profil ist an einer benannten Stelle kaputt
    /// (Laenge, Signatur, Markentabelle). `grund` sagt welche.
    IccProfilFalsch { grund: &'static str },

    // ---- AVIF ----
    /// Die Datei ist ein AVIF/AV1-Intra-Bild, und diese Crate hat den
    /// Dekoder noch nicht. `grund` sagt, was fehlt.
    ///
    /// GEMESSEN 02.10.2026 (Rolle BILD, B8): der Container ist lesbar
    /// (ffmpeg: `codec_name=av1`, 64x48), der Bildstrom ist AV1-Intra.
    /// Statt „kein bekanntes Bildformat“ (das klingt nach einer kaputten
    /// Datei) nennt der Fehler das Format.
    AvifNichtGelesen { grund: &'static str },

    /// Der arithmetische Dekoder (AV1/MSAC) kann an der Stelle nicht weiter:
    /// der Bildstrom endet, oder die Verteilungsfunktion gibt kein Symbol
    /// her. `grund` sagt, welcher von beiden Faellen es war.
    ///
    /// GEMESSEN 04.10.2026 (Rolle BILD, Teil 51): Der Wert dieser Variante
    /// ist **kein** Misserfolg, sondern eine Zusage ueber den Rand der
    /// Tabelle. In C laeuft die Suche in den Anpassungszaehler hinein und
    /// liest ihn als Wahrscheinlichkeit; hier endet sie und benennt es.
    ArithmetikFehler { grund: &'static str },

    // ---- SVG ----
    /// Die SVG-Quelle ist an einer benannten Stelle kaputt (Attribut,
    /// `d`-Befehl, Form). `grund` sagt welche.
    SvgDatenFalsch { grund: &'static str },
    /// Im `d`-Attribut eines `<path>` steht ein Befehl, den diese Crate
    /// nicht liest. `befehl` nennt ihn buchstablich.
    ///
    /// GEMESSEN 02.10.2026 (Rolle BILD, B7): der Fehler war vorher
    /// `SvgDatenFalsch` mit einem statischen Text, der nur die Liste der
    /// GELESENEN Befehle nannte. Bei echten Seiten blieb damit offen, was
    /// fehlte — es waren `a`, `q`, `s`, `t` und `A`.
    SvgBefehlFalsch { befehl: char },

    // ---- WebP ----
    /// Die WebP-Datei ist an einer benannten Stelle kaputt (RIFF, Chunk,
    /// Praefixcode, Transformation). `grund` sagt welche.
    ///
    /// Eigene Variante statt `JpegDatenFalsch`: „JPEG-Daten defekt" bei
    /// einer .webp waere eine Luege.
    WebpDatenFalsch { grund: &'static str },

    // ---- ICO ----
    /// Die ICO-Datei ist kaputt (Kopf, Verzeichnis, DIB). `grund` sagt welche.
    IcoKopfFalsch { grund: &'static str },

    // ---- GIF ----
    /// Die GIF-Datei ist an einer benannten Stelle kaputt (LZW, Blockkette,
    /// Bilddeskriptor). `grund` sagt welche.
    ///
    /// Eigene Variante statt `JpegDatenFalsch`: „JPEG-Daten defekt" bei
    /// einer GIF-Datei waere eine Luege.
    GifDatenFalsch { grund: &'static str },

    // ---- JPEG ----
    /// Der JPEG-Kopf oder ein Segment passt nicht.
    JpegKopfFalsch { grund: &'static str },
    /// Eine Huffman-Tabelle ist defekt.
    JpegHuffmanFalsch { grund: &'static str },
    /// Eine Quantisierungstabelle ist defekt.
    JpegQuantFalsch { grund: &'static str },
    /// Die entropiekodierten Daten sind defekt.
    JpegDatenFalsch { grund: &'static str },
    /// Ein Restart-Marker ist falsch.
    JpegRestartFalsch,
    /// B9: die verlangte Verkleinerungsstufe ist fuer diese Datei nicht
    /// belegt. GEMESSEN: bei senkrechter Unterabtastung (4:2:0) weicht der
    /// Achtelweg bis 37 von `djpeg -scale 1/8` ab, bei waagerechter (4:2:2)
    /// und ohne Unterabtastung nicht. Siehe `jpeg/lesen.rs`.
    JpegVerkleinerungFalsch { grund: &'static str },
    /// Zu viele Segmente in der JPEG-Datei.
    JpegSegmenteZuViel,
}

/// Warum genau der zlib-Strom defekt ist.
///
/// Eigener Typ, weil INFLATE rund ein Dutzend unterscheidbare Fehler hat
/// und der Aufrufer bei "Zlib kaputt" nichts anfangen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FehlerZlib {
    /// CMF/FLG: Kompressionsmethode ungleich 8 (nur DEFLATE gibt es).
    UnbekannteMethode { cmf: u8, flg: u8 },
    /// CMF/FLG: FCHECK stimmt nicht — `(CMF*256 + FLG) % 31 != 0`.
    FalschePruefsumme { cmf: u8, flg: u8 },
    /// FLG: FDICT gesetzt, ein Praediktionswörterbuch liegt bei. Dafuer
    /// gibt es in PNG keine Verwendung, also ist es ein Fehler.
    Praediktionswoerterbuch,
    /// Unbekannter Blocktyp (die Bits 01 und 10 sind reserviert).
    UnbekannterBlocktyp { typ: u8 },
    /// Die Daten enden mitten im Block.
    DatenZuEnde { byte: usize },
    /// Gespeicherter Block: NLEN ist nicht das Komplement von LEN.
    GespeichertFalsch { len: u16, nlen: u16 },
    /// Eine Huffman-Codierung ist unvollstaendig oder ueberzaehlt.
    HuffmanFalsch { grund: &'static str },
    /// Literale/Laengen-Symbol 286 oder 287 (in der festen Tabelle nicht belegt).
    SymbolNichtBelegt { wert: u16 },
    /// Distanz 0 oder groesser als 32768.
    DistanzFalsch { wert: u16 },
    /// Match laeuft ueber das Ende der bisherigen Ausgabe.
    MatchUeberEnde { laenge: u16, position: usize },
    /// Die Ausgabe waechst ueber die erlaubte Groesse hinaus.
    ZuGross { grenze: usize },
    /// Fuellbyte und Kompressionsbit stimmen nicht (gilt nur bei festen
    /// Distanzen, die es in DEFLATE nicht gibt — bleibt fuer Vollstaendigkeit).
    UnerwartetesEnde,
}

impl std::fmt::Display for BildFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use BildFehler::*;
        match self {
            KeinBildformat => write!(f, "kein bekanntes Bildformat"),
            ZuKurz {
                vorhanden,
                gebraucht,
            } => write!(
                f,
                "Eingabe zu kurz: {vorhanden} Byte, gebraucht {gebraucht}"
            ),
            GroesseNull => write!(f, "Bild hat Breite 0 oder Hoehe 0"),
            ZuGross {
                breite,
                hoehe,
                kante,
            } => write!(
                f,
                "Bild {breite}x{hoehe} ueber der Grenze {kante} (Kante oder Punkte)"
            ),
            Abgeschnitten {
                position,
                gebraucht,
            } => write!(
                f,
                "abgeschnitten bei Byte {position}: {gebraucht} Byte gebraucht"
            ),
            KeineSignatur => write!(f, "keine PNG-Signatur"),
            KeinIhdr => write!(f, "IHDR fehlt oder steht nicht zuerst"),
            IhdrLaengeFalsch { gefunden } => {
                write!(f, "IHDR-Laenge {gefunden}, muss 13 sein")
            }
            CrcFalsch {
                typ,
                position,
                erwartet,
                gefunden,
            } => write!(
                f,
                "CRC von Chunk {:?} bei Byte {position}: steht {erwartet:#010x}, gerechnet {gefunden:#010x}",
                String::from_utf8_lossy(typ)
            ),
            UnbekannterChunk { typ, position } => write!(
                f,
                "unbekannter kritischer Chunk {:?} bei Byte {position}",
                String::from_utf8_lossy(typ)
            ),
            UngueltigerChunkTyp { typ, position } => write!(
                f,
                "Chunk-Typ {:?} bei Byte {position} hat ungueltige Zeichen",
                String::from_utf8_lossy(typ)
            ),
            IdatFehlt => write!(f, "kein IDAT-Chunk"),
            IendFehlt => write!(f, "kein IEND-Chunk"),
            Interlace { verfahren } => {
                write!(
                    f,
                    "Interlace-Verfahren {verfahren} (Adam7) nicht unterstuetzt"
                )
            }
            SechzehnBit { farbtyp } => {
                write!(f, "16 Bit bei Farbtyp {farbtyp} nicht unterstuetzt")
            }
            UnbekannterFarbtyp { wert } => write!(f, "Farbtyp {wert} unbekannt"),
            UnbekannteBittiefe { farbtyp, tiefe } => {
                write!(f, "Bittiefe {tiefe} bei Farbtyp {farbtyp} unbekannt")
            }
            UnbekanntesVerfahren {
                kompression,
                filter,
            } => write!(
                f,
                "Kompression {kompression} oder Filter {filter} ungleich 0"
            ),
            PaletteFalsch { gefunden } => {
                write!(
                    f,
                    "PLTE mit {gefunden} Eintraegen (0, 3 oder >256 sind ungueltig)"
                )
            }
            ZuWenigDaten { erwartet, gefunden } => {
                write!(f, "nur {gefunden} Bildbytes entpackt, erwartet {erwartet}")
            }
            EntpacktZuViel { erwartet } => {
                write!(f, "mehr Bildbytes entpackt als erlaubt ({erwartet})")
            }
            FilterUnbekannt { typ, zeile } => {
                write!(f, "Filtertyp {typ} in Zeile {zeile} (nur 0-4 erlaubt)")
            }
            TrnsFalsch { gefunden } => {
                write!(f, "tRNS mit {gefunden} Bytes passt nicht zum Farbtyp")
            }
            Zlib(e) => write!(f, "zlib-Stream defekt: {e}"),
            PpmKopfFalsch { gefunden } => write!(
                f,
                "PPM-Kopf falsch: 'P{}' ist keine der sechs Formen",
                *gefunden
            ),
            PpmMaxvalFalsch { maxval } => {
                write!(f, "PPM maxval {maxval} (erlaubt 1..=65535)")
            }
            PpmZahlFalsch { position } => {
                write!(f, "PPM: bei Byte {position} steht keine Zahl")
            }
            BmpKopfFalsch { grund } => write!(f, "BMP-Kopf falsch: {grund}"),
            BmpVariante { grund } => write!(f, "BMP-Variante nicht unterstuetzt: {grund}"),
            SvgBefehlFalsch { befehl } => write!(
                f,
                "SVG-Pfadbefehl '{befehl}' wird nicht gelesen (gelesen: M, L, C, Z, Q, T, S, A, m, l, h, H, v, V, q, t, s, a)"
            ),
            IccProfilFalsch { grund } => write!(f, "ICC-Profil defekt: {grund}"),
            AvifNichtGelesen { grund } => write!(f, "AVIF wird nicht gelesen: {grund}"),
            ArithmetikFehler { grund } => {
                write!(f, "arithmetischer Dekoder nicht weiter: {grund}")
            }
            SvgDatenFalsch { grund } => write!(f, "SVG-Quelle defekt: {grund}"),
            WebpDatenFalsch { grund } => write!(f, "WebP-Daten defekt: {grund}"),
            IcoKopfFalsch { grund } => write!(f, "ICO-Kopf falsch: {grund}"),
            GifDatenFalsch { grund } => write!(f, "GIF-Daten defekt: {grund}"),
            JpegKopfFalsch { grund } => write!(f, "JPEG-Kopf falsch: {grund}"),
            JpegVerkleinerungFalsch { grund } => {
                write!(f, "JPEG-Verkleinerung nicht belegt: {grund}")
            }
            JpegHuffmanFalsch { grund } => write!(f, "JPEG-Huffman-Tabelle defekt: {grund}"),
            JpegQuantFalsch { grund } => write!(f, "JPEG-Quantisierungstabelle defekt: {grund}"),
            JpegDatenFalsch { grund } => write!(f, "JPEG-Daten defekt: {grund}"),
            JpegRestartFalsch => write!(f, "JPEG-Restart-Marker falsch"),
            JpegSegmenteZuViel => write!(f, "JPEG hat zu viele Segmente"),
        }
    }
}

impl std::fmt::Display for FehlerZlib {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use FehlerZlib::*;
        match self {
            UnbekannteMethode { cmf, flg } => {
                write!(f, "CM/FLG {cmf:#04x}/{flg:#04x}: Methode ungleich 8")
            }
            FalschePruefsumme { cmf, flg } => {
                write!(f, "CMF/FLG {cmf:#04x}/{flg:#04x}: (CMF*256+FLG) % 31 != 0")
            }
            Praediktionswoerterbuch => write!(f, "FDICT gesetzt (Praediktionswoerterbuch)"),
            UnbekannterBlocktyp { typ } => write!(f, "Blocktyp {typ:#04b} reserviert"),
            DatenZuEnde { byte } => write!(f, "Datenende bei Byte {byte}"),
            GespeichertFalsch { len, nlen } => {
                write!(
                    f,
                    "gespeicherter Block: LEN {len}, NLEN {nlen} ist kein Komplement"
                )
            }
            HuffmanFalsch { grund } => write!(f, "Huffman-Tabelle defekt: {grund}"),
            SymbolNichtBelegt { wert } => write!(f, "Symbol {wert} nicht belegt"),
            DistanzFalsch { wert } => write!(f, "Distanz {wert} unzulaessig (1..=32768)"),
            MatchUeberEnde { laenge, position } => {
                write!(
                    f,
                    "Match der Laenge {laenge} laeuft bei {position} ueber die Ausgabe"
                )
            }
            ZuGross { grenze } => write!(f, "Ausgabe ueber {grenze} Byte"),
            UnerwartetesEnde => write!(f, "Daten enden unerwartet"),
        }
    }
}

impl std::error::Error for BildFehler {}

impl BildFehler {
    /// Kurzname des Falls, stabil fuer Logs und Tests.
    pub fn name(&self) -> &'static str {
        use BildFehler::*;
        match self {
            KeinBildformat => "KeinBildformat",
            ZuKurz { .. } => "ZuKurz",
            GroesseNull => "GroesseNull",
            ZuGross { .. } => "ZuGross",
            Abgeschnitten { .. } => "Abgeschnitten",
            KeineSignatur => "KeineSignatur",
            KeinIhdr => "KeinIhdr",
            IhdrLaengeFalsch { .. } => "IhdrLaengeFalsch",
            CrcFalsch { .. } => "CrcFalsch",
            UnbekannterChunk { .. } => "UnbekannterChunk",
            UngueltigerChunkTyp { .. } => "UngueltigerChunkTyp",
            IdatFehlt => "IdatFehlt",
            IendFehlt => "IendFehlt",
            Interlace { .. } => "Interlace",
            SechzehnBit { .. } => "SechzehnBit",
            UnbekannterFarbtyp { .. } => "UnbekannterFarbtyp",
            UnbekannteBittiefe { .. } => "UnbekannteBittiefe",
            UnbekanntesVerfahren { .. } => "UnbekanntesVerfahren",
            PaletteFalsch { .. } => "PaletteFalsch",
            ZuWenigDaten { .. } => "ZuWenigDaten",
            EntpacktZuViel { .. } => "EntpacktZuViel",
            FilterUnbekannt { .. } => "FilterUnbekannt",
            TrnsFalsch { .. } => "TrnsFalsch",
            Zlib(_) => "Zlib",
            PpmKopfFalsch { .. } => "PpmKopfFalsch",
            PpmMaxvalFalsch { .. } => "PpmMaxvalFalsch",
            PpmZahlFalsch { .. } => "PpmZahlFalsch",
            BmpKopfFalsch { .. } => "BmpKopfFalsch",
            BmpVariante { .. } => "BmpVariante",
            IccProfilFalsch { .. } => "IccProfilFalsch",
            SvgBefehlFalsch { .. } => "SvgBefehlFalsch",
            AvifNichtGelesen { .. } => "AvifNichtGelesen",
            ArithmetikFehler { .. } => "ArithmetikFehler",
            SvgDatenFalsch { .. } => "SvgDatenFalsch",
            WebpDatenFalsch { .. } => "WebpDatenFalsch",
            IcoKopfFalsch { .. } => "IcoKopfFalsch",
            GifDatenFalsch { .. } => "GifDatenFalsch",
            JpegKopfFalsch { .. } => "JpegKopfFalsch",
            JpegVerkleinerungFalsch { .. } => "JpegVerkleinerungFalsch",
            JpegHuffmanFalsch { .. } => "JpegHuffmanFalsch",
            JpegQuantFalsch { .. } => "JpegQuantFalsch",
            JpegDatenFalsch { .. } => "JpegDatenFalsch",
            JpegRestartFalsch => "JpegRestartFalsch",
            JpegSegmenteZuViel => "JpegSegmenteZuViel",
        }
    }
}

// ---------------------------------------------------------------------------
// Bildformat
// ---------------------------------------------------------------------------

/// Was `erkenne` erkannt hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bildformat {
    Png,
    Bmp,
    Ppm,
    Jpeg,
    Gif,
    Ico,
    Webp,
    /// AVIF/AV1-Intra. GEMESSEN 02.10.2026 (Rolle BILD, B8): an der
    /// ISO-BMFF-Struktur erkannt, aber noch nicht gelesen — der
    /// AV1-Intra-Dekoder fehlt.
    Avif,
    /// SVG — der einzige Textformat-Eintrag. GEMESSEN 02.10.2026 (Rolle
    /// BILD, B-HN): fehlte hier, und deshalb war `erkenne` bei
    /// `y18.svg` von news.ycombinator.com `None` und `lesen` meldete
    /// „kein bekanntes Bildformat“ — die Datei konnte gar nicht erst
    /// gelesen werden.
    Svg,
}

impl std::fmt::Display for Bildformat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Bildformat::Png => "PNG",
            Bildformat::Bmp => "BMP",
            Bildformat::Ppm => "PPM",
            Bildformat::Jpeg => "JPEG",
            Bildformat::Gif => "GIF",
            Bildformat::Ico => "ICO",
            Bildformat::Webp => "WebP",
            Bildformat::Avif => "AVIF",
            Bildformat::Svg => "SVG",
        };
        f.write_str(s)
    }
}
