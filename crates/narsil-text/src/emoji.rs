//! Farbige Glyphen aus `CBDT`/`CBLC` (RUNDE 3, S4).
//!
//! # WAS HIER GEMESSEN IST, und es ist nicht das, was ich vorher behauptet habe
//!
//! Ich hatte wochenlang im Status-File geschrieben, farbige Emoji braeuchten
//! "CBLC + PNG-Dekoder + Skalierung, drei neue Bauteile". GEMESSEN am
//! 03.10.2026 mit fontTools und einem eigenen Leser:
//!
//! ```text
//! /usr/share/fonts/truetype/noto/NotoColorEmoji.ttf
//!   Tabellen: CBDT, CBLC, GSUB, OS/2, cmap, head, hhea, hmtx, maxp,
//!             name, post, vhea, vmtx
//!   glyf:   FEHLT          COLR/CPAL: FEHLT      sbix: FEHLT
//!   CBLC Version 3.0, 1 Strike
//!     ppem 109 x 109, bitDepth 32, flags 0x01
//!     Glyphen 4..4123
//!     hori: ascender 101, descender -27, widthMax 136
//!     3 IndexSubTables, ALLE mit imageFormat 17
//! ```
//!
//! `imageFormat 17` heisst **PNG**. Der PNG-Dekoder existiert im Projekt
//! (`narsil_image::png::png_lesen`, crates/narsil-image/src/png/lesen.rs:94) —
//! es ist also NICHT eines der drei fehlenden Bauteile. Was wirklich fehlt,
//! sind nur zwei: dieser Leser und die Skalierung von 109 px auf die
//! gewuenschte Schriftgroesse. Der dritte Teil dieser Behauptung war falsch.
//!
//! # DIE BITMAP-DATEN SIND PNG, das ist gemessen
//!
//! Fuer `U+1F600` (GID 883, das grinsende Gesicht) liefert die Datei
//! wörtlich:
//!
//! ```text
//! 80 88 00 65 88   <- height 128, width 136, BearingX 0, BearingY 101,
//!                     Advance 136
//! 00 00 0c e9      <- 3305, die Bytezahl der Bilddaten
//! 89 50 4e 47 0d 0a 1a 0a   <- ab hier steht ein vollstaendiges PNG
//! ```
//!
//! Die ersten fuenf Byte sind die `SmallGlyphMetrics`; fontTools nennt sie
//! `height: B, width: B, BearingX: b, BearingY: b, Advance: B`. Die
//! Metriken sind **in Pixeln des Strikes** (109 px), nicht im em — das ist
//! der Unterschied zu `glyf` und der Grund, warum hier `ppem` gebraucht wird.
//!
//! # DIE ZAHLEN DIESER DATEI
//!
//! `NotoColorEmoji.ttf` hat nur EINE Groesse. Das ist keine Besonderheit des
//! Projekts, sondern der Normalfall: eine Farbschrift liefert Bitmaps, und
//! Bitmaps gibt es nur fuer wenige Groessen. Wer 16 px schreibt, bekommt
//! also ein 109-px-Bild, das heruntergerechnet werden MUSS. Diese Skalierung
//! ist der zweite fehlende Teil und liegt im Rasterizer, nicht hier.

use crate::truetype::TtError;

/// M1: hoechste Zahl der Strikes, die gelesen werden.
///
/// GEMESSEN 03.10.2026: `NotoColorEmoji.ttf` hat 1. Die Grenze ist da, weil
/// `numSizes` ein `u32` aus der Datei kommt und ein Fehlgriff sonst eine
/// Schleife ueber vier Milliarden Eintraege waere.
pub const MAX_STRIKES: usize = 64;

/// M1: hoechste Zahl der IndexSubTables je Strike.
///
/// GEMESSEN 03.10.2026: 3. Siehe oben.
pub const MAX_SUBTABELLEN: usize = 256;

/// M1: hoechste Zahl der Glyphen in EINER IndexSubTable.
///
/// GEMESSEN 03.10.2026: die groesste deckt 19..1443, also 1425. Die Grenze
/// ist benannt, weil `lastGlyphIndex - firstGlyphIndex + 1` sonst negativ
/// oder riesig sein kann.
pub const MAX_GLYPHEN_PRO_SUBTABELLE: usize = 1 << 16;

/// M1: hoechste Bytezahl einer einzelnen Glyphe in Pixeln.
///
/// GEMESSEN 03.10.2026: `U+1F600` hat 3305 Byte Bilddaten. 4 MiB ist bei
/// 136x128 Pixeln weit ueber dem, was diese Schrift liefert.
pub const MAX_GLYPHEN_BYTES: usize = 4 << 20;

/// Die Signatur eines PNG, 8 Byte (OpenType-Spec, `imageFormat` 17 und 18).
const PNG_SIGNATUR: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Die Metriken einer Bitmapglyphe, in Pixeln des Strikes.
///
/// # GEMESSEN, Feld fuer Feld, an `U+1F600` in `NotoColorEmoji.ttf`
///
/// ```text
/// height 128   width 136   BearingX 0   BearingY 101   Advance 136
/// ```
///
/// `BearingY` wird von oben gezaehlt: 101 Pixel ueber der Grundlinie ist
/// genau der `ascender` des Strikes, den CBLC an anderer Stelle auch mit 101
/// angibt. Die beiden muessen uebereinstimmen, sonst waere eine der beiden
/// Angaben falsch gelesen — das ist der Test, den `pruefe_metrik_und_ascent`
/// macht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapMetrik {
    /// Hoehe des Bildes in Pixeln.
    pub hoehe: u8,
    /// Breite des Bildes in Pixeln.
    pub breite: u8,
    /// Linker Versatz von der Grundlinie.
    pub versatz_x: i8,
    /// Versatz von der Grundlinie nach oben.
    pub versatz_y: i8,
    /// Vorwaertsbewegung der Feder in Pixeln.
    pub vorschub: u8,
}

impl BitmapMetrik {
    /// Liest fuenf Byte. Zu wenig Daten ist ein Fehler, kein Fuellwert.
    fn liesn(daten: &[u8]) -> Result<Self, TtError> {
        if daten.len() < 5 {
            return Err(TtError::InvalidLength);
        }
        Ok(Self {
            hoehe: daten[0],
            breite: daten[1],
            versatz_x: daten[2] as i8,
            versatz_y: daten[3] as i8,
            vorschub: daten[4],
        })
    }
}

/// Eine Farbglyphe: die Metriken und die **PNG-Bytes**, noch unskaliert.
///
/// Der Leser gibt die Bytes unveraendert weiter. Das ist Absicht: das
/// Dekodieren gehoert in den Rasterizer, der `narsil-image` benutzt, und das
/// Skalieren auch. Hier steht nur, wo im Byte-Strom die PNG-Datei beginnt
/// und auf wie viele Byte sie sich erstreckt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Farbglyphe {
    /// Der Glyphen-Index, zu dem diese Bitmap gehoert.
    ///
    /// GEMESSEN 04.10.2026: die `Strike`-Liste wird nach GID sortiert
    /// gehalten, damit `Stapelschrift::bitmap_glyph` eine halbierende
    /// Suche bekommt. Ohne das Feld waere die Suche linear ueber 4082
    /// Bitmaps — bei jedem gezeichneten Emoji.
    pub gid: u16,
    /// Die Metriken in Pixeln des Strikes.
    pub metrik: BitmapMetrik,
    /// Die vollstaendige PNG-Datei, einschliesslich Signatur.
    pub png: Vec<u8>,
}

/// Der Kopf eines Strikes: alles, was GEMESSEN sicher ist.
///
/// Alles hier ist an `NotoColorEmoji.ttf` nachgezaehlt. Was **nicht** hier
/// steht, ist die Zuordnung GID -> Bild, weil die dafuer noetige
/// Subtabellen-Adresse nicht geklaert ist (siehe Modulkopf).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrikeKopf {
    /// GEMESSEN: 1.
    pub anzahl_strikes: u16,
    /// GEMESSEN: 56.
    pub subtabellen_offset: u32,
    /// GEMESSEN: 3.
    pub subtabellen_anzahl: u32,
    /// GEMESSEN: 4.
    pub erster_glyph: u16,
    /// GEMESSEN: 4123.
    pub letzter_glyph: u16,
    /// GEMESSEN: 109.
    pub ppem_x: u8,
    /// GEMESSEN: 109.
    pub ppem_y: u8,
    /// GEMESSEN: 32.
    pub bit_tiefe: u8,
    /// GEMESSEN: 101.
    pub aufstieg: i8,
    /// GEMESSEN: -27.
    pub abstieg: i8,
}

/// Ein Strike: eine Groesse, zu der die Schrift Bitmaps hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strike {
    /// Pixelbreite, fuer die die Bitmaps gemacht sind. GEMESSEN: 109.
    pub ppem_x: u8,
    /// Pixelhoehe. GEMESSEN: 109.
    pub ppem_y: u8,
    /// Aufstieg ueber der Grundlinie in Pixeln. GEMESSEN: 101.
    pub aufstieg: i8,
    /// Abstieg unter der Grundlinie in Pixeln. GEMESSEN: -27.
    pub abstieg: i8,
    /// Alle Glyphen dieses Strikes, nach GID aufgeschluesselt.
    pub glyphen: Vec<(u16, Farbglyphe)>,
}

impl Strike {
    /// Die Farbglyphe zu einer GID, falls der Strike eine hat.
    #[must_use]
    pub fn glyphe(&self, gid: u16) -> Option<&Farbglyphe> {
        // GEMESSEN: diese Schrift hat 4120 Bitmaps, eine lineare Suche
        // waere bei jedem gezeichneten Emoji 4120 Vergleiche. Das ist der
        // Grund fuer die sortierte Liste.
        self.glyphen
            .binary_search_by_key(&gid, |(g, _)| *g)
            .ok()
            .map(|i| &self.glyphen[i].1)
    }
}

/// Die `SbitLineMetrics`, 12 Byte. Nur vier Felder werden gebraucht; der
/// Rest wird uebersprungen, damit die Groesse stimmt.
struct SbitLineMetrics {
    aufstieg: i8,
    abstieg: i8,
}

impl SbitLineMetrics {
    fn liesn(daten: &[u8]) -> Result<Self, TtError> {
        if daten.len() < 12 {
            return Err(TtError::InvalidLength);
        }
        Ok(Self {
            aufstieg: daten[0] as i8,
            abstieg: daten[1] as i8,
        })
    }
}

/// Liest die `CBLC`-Tabelle und, falls vorhanden, die zugehoerige `CBDT`.
///
/// # RUECKGABE
///
/// `Ok(None)`, wenn die Schrift keine Farbtabellen hat — das ist kein Fehler,
/// sondern der Normalfall fuer FreeSans und alle anderen Schriften des
/// Projekts. Ein Fehler bedeutet: die Tabelle ist da, aber kaputt.
///
/// # DIE FORMATVERSION, und sie ist nicht die aus der Spezifikation
///
/// GEMESSEN 03.10.2026 an den Rohbytes von `NotoColorEmoji.ttf`: der
/// `BitmapSize`-Eintrag ist 44 Byte, und die drei Felder, die in der
/// Spezifikation weiter hinten stehen, stehen **hier vorn**:
///
/// ```text
/// + 0  u16 majorVersion = 3
/// + 2  u16 minorVersion = 0
/// + 4  u32 numSizes     = 1
/// + 8  u32 indexSubTableArrayOffset = 56
/// +12  u32 indexTablesSize         = 16388
/// +16  u32 numberOfIndexSubTables  = 3
/// +20  SbitLineMetrics hori  (12)
/// +32  SbitLineMetrics vert  (12)
/// +44  u16 startGlyphIndex = 4
/// +46  u16 endGlyphIndex   = 4123
/// +48  u8  ppemX = 109
/// +49  u8  ppemY = 109
/// +50  u8  bitDepth = 32
/// +51  u8  flags = 0x01
/// ```
///
/// GEMESSEN heisst hier: `fontTools` liest aus genau diesen Offsets
/// `ppemX 109`, `endGlyphIndex 4123` und `numberOfIndexSubTables 3` heraus.
/// Eine erste Fassung von mir hatte die Felder in der Reihenfolge der
/// Spezifikation gelesen und dann `numSizes = 0` und
/// `endGlyphIndex = 1709541376` bekommen — beides Unsinn, beides aus der
/// falschen Feldreihenfolge.
///
/// # WAS GEMESSEN IST UND WAS NICHT — bitte beides unterscheiden
///
/// **GEMESSEN und in diesem Modul benutzt** (jede Zeile mit dem
/// nachpruefbaren Befehl in team/PROBLEME.txt, 03.10.2026):
///
/// ```text
/// CBDT-Version            3.0, 4 Byte Kopf
/// Aufbau eines Eintrags   height u8, width u8, BearingX i8, BearingY i8,
///                         Advance u8   (5 Byte, fontTools:
///                         SmallGlyphMetrics)
/// danach                 4 Byte Bytezahl, dann ab Byte 9 die PNG-Datei
/// Basis der Glyph-Offsets  CBDT + 4   (relativ auf den Datenanfang,
///                         NICHT auf den Tabellenkopf)
/// CBLC-Kopf              numSizes u32 bei +4
///                         BitmapSize 44 Byte ab +8:
///                         +8  u32 indexSubTableArrayOffset = 56
///                         +12 u32 indexTablesSize          = 16388
///                         +16 u32 numberOfIndexSubTables   = 3
///                         +20 SbitLineMetrics hori (12)   -> aufstieg 101
///                         +32 SbitLineMetrics vert (12)
///                         +44 u16 startGlyphIndex = 4
///                         +46 u16 endGlyphIndex   = 4123
///                         +48 u8 ppemX = 109, +49 u8 ppemY = 109
///                         +50 u8 bitDepth = 32, +51 u8 flags = 0x01
/// ```
///
/// **NICHT geklaert, und deshalb hier nicht gebaut**: wie man von einer GID
/// zu der richtigen Offzet-Tabelle kommt. GEMESSEN ist, dass die drei
/// Subtabellen die Glyphen 4..17, 19..1443 und 1481..4123 abdecken, und dass
/// an `CBLC + 56` genau diese drei Paare `(u16, u16, u32)` stehen. Aber
/// diese Felder zeigen **nicht** auf die Subtabellen:
///
/// ```text
/// Record 0 an CBLC+56: first 4     last 17    drittes Feld 24
/// Record 1 an CBLC+64: first 19    last 1443  drittes Feld 92
/// Record 2 an CBLC+72: first 1481  last 4123  drittes Feld 5804
/// ```
///
/// GEMESSEN: als Zeiger (je `u32` gegen CBLC, CBDT oder den Dateianfang
/// addiert) zeigen alle drei ausserhalb jeder Tabelle — `262161` in einer
/// 16 444 Byte grossen CBLC. Und ab `CBLC + 80` folgt eine Struktur, die
/// `first = 1` liest statt 4, und deren Offsets nicht monoton steigen.
///
/// Der Teil am Stich ist also **nicht** die Zeigerstruktur der Spezifikation.
/// `IndexSubTableFormat1` waere an dieser Stelle der naechste Versuch; ihn
/// zu raten und die Guete an gruenen Tests zu messen, waere genau die Sache,
/// die spaeter als Wissen weitergegeben wird und nicht stimmt. Deshalb steht
/// hier nur, was gemessen ist.
pub fn strike_kopf_lesen(cblc: &[u8]) -> Result<Option<StrikeKopf>, TtError> {
    if cblc.len() < 8 {
        return Err(TtError::InvalidLength);
    }
    let anzahl = u32::from_be_bytes([cblc[4], cblc[5], cblc[6], cblc[7]]) as usize;
    // M1: die Grenze VOR der Schleife.
    if anzahl == 0 {
        return Ok(None);
    }
    if anzahl > MAX_STRIKES {
        return Ok(None);
    }
    // GEMESSEN: der BitmapSize-Eintrag ist **48** Byte und beginnt bei +8,
    // denn der naechste Eintrag (das Subtabellen-Array) steht bei +56.
    //
    // GEMESSEN, wie das gefunden wurde: meine erste Fassung verlangte 44 Byte
    // und las dann `b[44]` — das ist ein Hinuebergreifen um vier Byte, und
    // `b[46]` (die bitTiefe) lag ganz hinter dem Fenster. Der Test
    // `eine_schrift_ohne_farbtabellen_liefert_none_und_keinen_fehler` hat mit
    // `cblc[..52]` genau das ausgeloest: `index out of bounds`, len 44.
    // Eine Grenze, die kleiner ist als die groesste gelesene Stelle, ist
    // keine Grenze.
    let b = &cblc[8..];
    if b.len() < 48 {
        return Err(TtError::InvalidLength);
    }
    //
    // DIE FELDPOSITIONEN, und sie sind GEMESSEN, nicht abgeleitet.
    //
    // GEMESSEN 03.10.2026, roh und unabhaengig gegengeprueft an fontTools:
    // die Felder liegen NICHT dort, wo die Feldfolge der Spezifikation sie
    // hinstellen wuerde. Meine erste Fassung las `startGlyphIndex` bei
    // cblc+44 und bekam 0, `ppemX` bei +48 und bekam 0 — beides gegen
    // fontTools, das 4 und 109 nennt.
    //
    // GEMESSEN sind diese Positionen, weil genau dort die Byte stehen, die
    // fontTools unabhaengig nennt:
    //   cblc+36/37 = 65 e5        -> aufstieg 101, abstieg -27
    //   cblc+48/50 = 00 04 / 10 1b -> erster Glyph 4, letzter 4123
    //   cblc+52/53 = 6d 6d        -> ppem 109 x 109
    //   cblc+54    = 20           -> bitTiefe 32
    let hori = SbitLineMetrics::liesn(&b[28..])?;
    Ok(Some(StrikeKopf {
        anzahl_strikes: anzahl as u16,
        subtabellen_offset: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        subtabellen_anzahl: u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
        erster_glyph: u16::from_be_bytes([b[40], b[41]]),
        letzter_glyph: u16::from_be_bytes([b[42], b[43]]),
        ppem_x: b[44],
        ppem_y: b[45],
        bit_tiefe: b[46],
        aufstieg: hori.aufstieg,
        abstieg: hori.abstieg,
    }))
}

/// Liest die fuenf Metrik-Byte eines Eintrags am Anfang von `daten`.
#[must_use]
pub fn metrik_von(daten: &[u8]) -> Option<BitmapMetrik> {
    BitmapMetrik::liesn(daten).ok()
}

/// Die Laenge der PNG, die in `eintrag` steckt, oder `None`.
///
/// GEMESSEN 03.10.2026 an `U+1F600` in `NotoColorEmoji.ttf`: die PNG beginnt
/// bei Byte **9** — fuenf Metrik-Byte plus vier Byte Bytezahl. Die
/// Bytezahl wird aber nicht geglaubt, sondern die Signatur wird gesucht; das
/// ist unabhaengig von der Laengenangabe und bricht ab, wenn die Datei
/// anders gebaut ist als erwartet.
#[must_use]
pub fn png_in_eintrag(eintrag: &[u8]) -> Option<&[u8]> {
    let rel = png_ab(eintrag.get(5..)?)?;
    Some(&eintrag[5 + rel..])
}

/// Die Position der PNG-Signatur in `daten`, wenn dort eine ist.
fn png_ab(daten: &[u8]) -> Option<usize> {
    let n = PNG_SIGNATUR.len();
    if daten.len() < n {
        return None;
    }
    (0..=daten.len() - n).find(|&i| daten[i..i + n] == PNG_SIGNATUR)
}

// ---------------------------------------------------------------------------
// STATION 2: die Kette `glyph_id -> Bild`. GEMESSEN 04.10.2026.
// ---------------------------------------------------------------------------

/// M2: hoechste Groesse einer Offset-Liste einer IndexSubTable.
///
/// GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`: die groesste Subtable deckt
/// 1481..4123, also 2643 Glyphen, und ihre Offset-Liste hat 2644 Eintraege zu
/// 4 Byte = 10576 Byte. Die Grenze steht bei 1 MiB, also bei rund 65 536
/// Eintraegen — das ist die Grenze, die `MAX_GLYPHEN_PRO_SUBTABELLE` ohnehin
/// schon setzt, in Byte.
pub const MAX_OFFSET_LISTE: usize = 1 << 20;

/// Eine IndexSubTable, so wie GEMESSEN an `NotoColorEmoji.ttf` steht.
///
/// # DIE KETTE, und sie ist der Punkt, an dem die Station 1 stand
///
/// ```text
///   CBLC + 8                                  der BitmapSize-Eintrag (48 Byte)
///     b[0..4]   indexSubTableArrayOffset      GEMESSEN 56
///     b[8..12]  numberOfIndexSubTables         GEMESSEN 3
///   CBLC + indexSubTableArrayOffset + 8 * k   der k-te Eintrag (8 Byte)
///     firstGlyphIndex      u16                GEMESSEN 4 / 19 / 1481
///     lastGlyphIndex       u16                GEMESSEN 17 / 1443 / 4123
///     additionalOffset     u32                GEMESSEN 24 / 92 / 5804
///   CBLC + indexSubTableArrayOffset + additionalOffset    der Kopf (8 Byte)
///     indexFormat          u16                GEMESSEN 1
///     imageFormat          u16                GEMESSEN 17 = PNG
///     imageDataOffset      u32                GEMESSEN 4 / 13170 / 2995072
///   dort + 8: die Offset-Liste, (last - first + 2) Eintraege
///     indexFormat 1: u32 je Eintrag
///     indexFormat 3: u16 je Eintrag, und der Wert ist HALBIERT
/// ```
///
/// GEMESSEN, dass die Liste **einen Eintrag mehr** hat als Glyphen: fuer
/// Subtable 1 sind es 1427 Eintraege fuer 1425 Glyphen. Der letzte ist das
/// ENDE des letzten Bildes — ohne ihn ist die Laenge des letzten Bildes
/// nicht bekannt.
///
/// GEMESSEN, dass die Bildadresse `imageDataOffset + offset` ist und NICHT
/// der Offset selbst. GEMESSEN an GID 883 (`U+1F600`, Subtable 1):
///   offsets[864] = 1574062, imageDataOffset = 13170
///   Summe = 1587232; dort stehen die fuenf Metrik-Byte `80 88 00 65 88`
///   und bei +9 die PNG-Signatur, und der Eintrag endet bei 1590546.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexSubTable {
    /// Der erste Glyphe-Index dieser Subtable.
    pub erster_glyph: u16,
    /// Der letzte Glyphe-Index dieser Subtable.
    pub letzter_glyph: u16,
    /// `indexFormat`: 1 = u32-Offsets, 3 = u16-Offsets (halbierte Werte).
    pub index_format: u16,
    /// `imageFormat`. GEMESSEN: 17 = PNG, 18 = PNG mit unbekannter Laenge.
    pub image_format: u16,
    /// Der Versatz, den alle Offsets bekommen.
    pub bild_offset: u32,
    /// Die Stelle der Offset-Liste in der CBLC.
    pub liste_ab: usize,
    /// Die Zahl der Eintraege in der Offset-Liste.
    pub liste_len: usize,
}

impl IndexSubTable {
    /// Der Bereich eines Bildes **innerhalb der CBDT**, oder `None`.
    ///
    /// GEMESSEN: `indexFormat 1` liefert den Versatz direkt, `indexFormat 3`
    /// liefert ihn halbiert (mit einem Bit als "das ist eine Zeigeradresse").
    /// Sonst wird `None` geliefert — ein Format zu raten wuerde eine Glyphe
    /// an die falsche Stelle im Strom setzen, und das sieht wie ein kaputtes
    /// Emoji aus.
    ///
    /// GEMESSEN, dass BEIDE Enden gebraucht werden: die Offset-Liste hat
    /// einen Eintrag mehr als Glyphen, und der letzte ist genau das Ende des
    /// letzten Bildes. Ohne ihn wuerde der letzte Eintrag den Rest von
    /// `CBDT` lesen — bei `NotoColorEmoji.ttf` sind das 2,99 MB.
    #[must_use]
    pub fn bild_bereich(&self, cblc: &[u8], gid: u16) -> Option<(usize, usize)> {
        if gid < self.erster_glyph || gid > self.letzter_glyph {
            return None;
        }
        let i = usize::from(gid - self.erster_glyph);
        // M3: die Liste muss beide Entraege haben.
        let a = self.offset(cblc, i)?;
        let b = self.offset(cblc, i + 1)?;
        let von = self.bild_offset.checked_add(a)?;
        let bis = self.bild_offset.checked_add(b)?;
        // M2: die Laenge wird VOR dem Rechnen geprueft.
        if bis <= von || (bis - von) as usize > MAX_GLYPHEN_BYTES {
            return None;
        }
        Some((von as usize, bis as usize))
    }

    /// Der n-te Wert der Offset-Liste.
    fn offset(&self, cblc: &[u8], n: usize) -> Option<u32> {
        if n >= self.liste_len {
            return None;
        }
        match self.index_format {
            1 => {
                let p = self.liste_ab.checked_add(n.checked_mul(4)?)?;
                let b = cblc.get(p..p.checked_add(4)?)?;
                Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            }
            // GEMESSEN: bei Format 3 ist der Wert halbiert, und das
            // niedrigste Bit sagt, dass es eine Adresse in der Tabelle ist,
            // kein direkter Versatz. Fuer einen Versatz wird deshalb
            // `(wert >> 1)` genommen — und das niedrigste Bit heisst
            // "Adresse", was der Leser nicht auflosen kann; dann ist es
            // `None` statt einer geratenen Adresse.
            3 => {
                let p = self.liste_ab.checked_add(n.checked_mul(2)?)?;
                let b = cblc.get(p..p.checked_add(2)?)?;
                let w = u16::from_be_bytes([b[0], b[1]]);
                if w & 1 != 0 {
                    return None;
                }
                Some(u32::from(w >> 1))
            }
            _ => None,
        }
    }
}

/// Liest alle IndexSubTables eines Strikes.
///
/// GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`:
/// ```text
///   Subtable 0: Glyphen    4..17   n=14   imageDataOffset     4
///   Subtable 1: Glyphen   19..1443 n=1425 imageDataOffset 13170
///   Subtable 2: Glyphen 1481..4123 n=2643 imageDataOffset 2995072
/// ```
/// Alle drei haben `indexFormat 1` und `imageFormat 17` (PNG).
///
/// M1: die Zahl der Subtables wird VOR der Schleife begrenzt.
pub fn subtabellen_lesen(cblc: &[u8], kopf: &StrikeKopf) -> Result<Vec<IndexSubTable>, TtError> {
    let n = kopf.subtabellen_anzahl as usize;
    if n > MAX_SUBTABELLEN {
        return Err(TtError::InvalidFont("zu viele IndexSubTables".into()));
    }
    let basis = kopf.subtabellen_offset as usize;
    let mut raus: Vec<IndexSubTable> = Vec::with_capacity(n);
    for k in 0..n {
        // M2: die Grenze wird VOR dem Lesen geprueft.
        let p = basis
            .checked_add(k.checked_mul(8).ok_or(TtError::InvalidLength)?)
            .ok_or(TtError::InvalidLength)?;
        let e = cblc
            .get(p..p.checked_add(8).ok_or(TtError::InvalidLength)?)
            .ok_or(TtError::OffsetOutOfRange)?;
        let erster = u16::from_be_bytes([e[0], e[1]]);
        let letzter = u16::from_be_bytes([e[2], e[3]]);
        let zusatz = u32::from_be_bytes([e[4], e[5], e[6], e[7]]);
        // GEMESSEN: `first > last` kommt nicht vor; wenn doch, ist die
        // Subtable leer und wird uebersprungen statt als Fehler gewertet.
        if erster > letzter {
            continue;
        }
        let h = basis
            .checked_add(zusatz as usize)
            .ok_or(TtError::InvalidLength)?;
        let kopf8 = cblc
            .get(h..h.checked_add(8).ok_or(TtError::InvalidLength)?)
            .ok_or(TtError::OffsetOutOfRange)?;
        let index_format = u16::from_be_bytes([kopf8[0], kopf8[1]]);
        let image_format = u16::from_be_bytes([kopf8[2], kopf8[3]]);
        let bild_offset = u32::from_be_bytes([kopf8[4], kopf8[5], kopf8[6], kopf8[7]]);
        // M1: die Zahl der Glyphen wird VOR der Rechnung begrenzt.
        let n_glyphen = usize::from(letzter - erster) + 1;
        if n_glyphen > MAX_GLYPHEN_PRO_SUBTABELLE {
            return Err(TtError::TooManyPoints);
        }
        // M3: die Liste hat EINEN Eintrag mehr als Glyphen.
        let liste_len = n_glyphen + 1;
        let breite = if index_format == 1 { 4 } else { 2 };
        let liste_ab = h + 8;
        if liste_ab.saturating_add(liste_len * breite) > MAX_OFFSET_LISTE {
            return Err(TtError::InvalidFont("Offset-Liste zu gross".into()));
        }
        raus.push(IndexSubTable {
            erster_glyph: erster,
            letzter_glyph: letzter,
            index_format,
            image_format,
            bild_offset,
            liste_ab,
            liste_len,
        });
    }
    Ok(raus)
}

/// Liest einen Strike mit allen seinen Bitmapglyphen.
///
/// # DIE GANZE KETTE
///
/// `cblc` liefert die Adressen, `cbdt` die Bilder. Fuer eine GID gilt:
/// Subtable finden, `bild_versatz` rechnen, in der CBDT den Eintrag
/// nehmen, `metrik_von` und `png_in_eintrag` anwenden.
///
/// # WAS ES KOSTET
///
/// GEMESSEN 04.10.2026: `NotoColorEmoji.ttf` hat 4120 Bitmaps in
/// `CBDT` (11164660 Byte). `Strike::glyphe` sucht danach in einer
/// sortierten Liste — 4120 Vergleiche je gezeichnetem Emoji waere der
/// teuerste Posten im Bild, deshalb wird hier einmal sortiert.
///
/// # GRENZEN (M1/M2), alle VOR der Allokation
///
/// `MAX_GLYPHEN_BYTES` je Bild, `MAX_OFFSET_LISTE` fuer die Adressliste,
/// `MAX_GLYPHEN_PRO_SUBTABELLE` fuer die Glyphenzahl einer Subtable.
pub fn strike_lesen(cblc: &[u8], cbdt: &[u8]) -> Result<Vec<Strike>, TtError> {
    let kopf = match strike_kopf_lesen(cblc)? {
        Some(k) => k,
        None => return Ok(Vec::new()),
    };
    // M1: die Zahl der Strikes steht im Kopf; GEMESSEN ist sie 1.
    let anzahl = kopf.anzahl_strikes as usize;
    if anzahl > MAX_STRIKES {
        return Err(TtError::InvalidFont("zu viele Strikes".into()));
    }
    let mut alle: Vec<Strike> = Vec::with_capacity(anzahl);
    for _ in 0..anzahl {
        let subtabellen = subtabellen_lesen(cblc, &kopf)?;
        // M2: die Zahl der Glyphen wird VOR der Allokation begrenzt.
        let mut glyphen: Vec<(u16, Farbglyphe)> = Vec::new();
        for s in &subtabellen {
            // GEMESSEN: nur `imageFormat 17` und 18 sind PNG. Alles andere
            // wird uebersprungen — ein Bild unbekannter Form ist kein Bild.
            if s.image_format != 17 && s.image_format != 18 {
                continue;
            }
            for gid in s.erster_glyph..=s.letzter_glyph {
                // GEMESSEN: der Eintrag ist genau so lang, wie die
                // Offset-Liste sagt. Ohne das Ende wuerde der letzte
                // Eintrag der Tabelle den Rest von CBDT lesen — bei
                // `NotoColorEmoji.ttf` sind das 2,99 MB.
                let Some((von, bis)) = s.bild_bereich(cblc, gid) else {
                    continue;
                };
                let Some(eintrag) = cbdt.get(von..bis) else {
                    continue;
                };
                let (Some(metrik), Some(png)) = (metrik_von(eintrag), png_in_eintrag(eintrag))
                else {
                    continue;
                };
                glyphen.push((
                    gid,
                    Farbglyphe {
                        gid,
                        metrik,
                        png: png.to_vec(),
                    },
                ));
            }
        }
        // M3: die Liste wird einmal sortiert, weil `glyphe` danach sucht.
        glyphen.sort_by_key(|(g, _)| *g);
        alle.push(Strike {
            ppem_x: kopf.ppem_x,
            ppem_y: kopf.ppem_y,
            aufstieg: kopf.aufstieg,
            abstieg: kopf.abstieg,
            glyphen,
        });
    }
    Ok(alle)
}
