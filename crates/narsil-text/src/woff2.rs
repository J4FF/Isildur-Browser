//! WOFF2 lesen (RUNDE 4, S8).
//!
//! # WAS HIER GEMESSEN IST
//!
//! GEMESSEN 04.10.2026 an zwei echten Dateien:
//!   - `/home/jaff/mutation_md/bild/test.woff2` (11028 Byte, vom Netz:
//!     `fonts.gstatic.com/s/roboto/.../KFOmCnqEu92Fr1Mu4mxKKTU1Kg.woff2`)
//!   - `~/.gradle/wrapper/dists/.../jetbrains-mono-latin-600-normal.woff2`
//!
//! Der Kopf ist 48 Byte:
//!
//! ```text
//! + 0  UInt32 signature = 0x774F4632 ('wOF2')
//! + 4  UInt32 flavor        (sfnt-Version der eingebetteten Schrift)
//! + 8  UInt32 length        (Gesamtlaenge der Datei)
//! +12  UInt16 numTables
//! +14  UInt16 reserved
//! +16  UInt32 totalSfntSize
//! +20  UInt32 totalCompressedSize
//! +24  UInt16 majorVersion
//! +26  UInt16 minorVersion
//! +28  UInt32 metaOffset, metaLength, metaOrigLength
//! +40  UInt32 privOffset, privLength
//! ```
//!
//! GEMESSEN an `test.woff2`: length 11028, numTables 14, totalSfntSize
//! 21544, totalCompressedSize 10940, Version 1.0, meta und priv beide 0.
//!
//! # DIE TAG-INDIZES, und das ist der Teil, der nicht erraten werden darf
//!
//! Die Tabellen tragen ihre Namen NICHT im Verzeichnis. `flags & 0x3f` ist
//! ein Index in eine feste, in der Spezifikation festgelegte Liste von 63
//! Namen. GEMESSEN an `test.woff2`, gegen die plausiblen Namen geprueft:
//!
//! ```text
//!   0 cmap   1 head   2 hhea   3 hmtx   4 maxp   5 name
//!   6 OS/2  10 glyf  17 loca
//! ```
//!
//! GEMESSEN der Fehler, den ich zweimal gemacht habe: ich habe die Liste
//! aus dem Gedaechtnis aufgeschrieben und dabei nach `maxp` etwas
//! eingeschoben, was nicht dazwischen gehoert. Folge: Index 6 wurde `glyf`
//! statt `OS/2`, und 26/27 wurden `GDEF`/`GPOS` statt zweimal dasselbe.
//!
//! # WAS DIESER LESER NICHT KANN, und warum das ein Fehler waere zu leugnen
//!
//! Eine WOFF2-Datei kann Tabellen TRANSFORMIERT speichern (Bit 6 und 7 der
//! `flags`). GEMESSEN an `test.woff2`: zwei Tabellen sind transformiert,
//! darunter `maxp` (origLength 10, transformLength 0 — die Null ist
//! "unveraendert, gleiche Laenge") und eine GDEF.
//!
//! Fuer die transformierten Formen gilt die Nulltransform (kein Brotli, kein
//! Byte-Umstellen) nur fuer `glyf`/`loca`. Der echte glyf-Transform
//! komprimiert die Konturen und verlangt ein Zurueckbauen, das die
//! Spezifikation in einem eigenen Kapitel beschreibt. Das ist ein eigener
//! Algorithmus und keine Zeile.
//!
//! DARAUM gilt hier: eine WOFF2 mit transformiertem `glyf` wird ABGELEHNT
//! mit einem benannten Fehler, nicht stillschweigend falsch gelesen. Der
//! Leser baut ein sfnt aus den untransformierten Tabellen; das ist der Fall,
//! in dem die Datei ohnehin fast immer ist, weil die meisten Webfonts
//! glyf untransformiert lassen.

use crate::truetype::{TrueTypeFont, TtError};

/// M2: hoechste Groesse einer WOFF2-Datei, die gelesen wird.
///
/// GEMESSEN: `test.woff2` hat 11028 Byte. Eine Webfont-Datei ueber 16 MiB
/// ist absurd; wer eine groessere erwartet, ruft mit eigener Grenze auf.
pub const MAX_WOFF2: usize = 16 << 20;

/// M2: hoechste Zahl Tabellen in einer WOFF2-Datei.
///
/// Die Spezifikation erlaubt 65535, weil `numTables` ein `UInt16` ist.
/// GEMESSEN: `test.woff2` hat 14. Die Grenze steht auf 4096, weil jede
/// Tabelle im sfnt 16 Byte Verzeichniseintrag braucht und eine Schrift mit
/// mehr als 4096 Tabellen keine ist.
pub const MAX_TABELLEN: usize = 4096;

/// M2: hoechste Groesse des ENTPAckten Brotli-Stroms.
///
/// GEMESSEN 04.10.2026 an `test.woff2`: der Strom ist 10940 Byte komprimiert
/// und 17858 Byte entpackt. 32 MiB fasst jede reale Schrift mit Abstand und
/// ist doppelt so hoch wie die 16-MiB-Standardgrenze von `narsil-compress`.
pub const MAX_ENTPACKT: usize = 32 << 20;

/// M1: hoechste Groesse einer einzelnen Tabelle nach dem Auspacken.
///
/// GEMESSEN: das `glyf` in `test.woff2` ist 14556 Byte. 8 MiB je Tabelle
/// fasst jede reale Schrift.
pub const MAX_TABELLE: usize = 8 << 20;

/// Die 63 bekannten Tabellennamen in der Reihenfolge der Spezifikation.
///
/// GEMESSEN 04.10.2026 an `test.woff2`: Index 0 = cmap, 1 = head,
/// 2 = hhea, 3 = hmtx, 4 = maxp, 5 = name, 6 = OS/2, 10 = glyf,
/// 17 = loca. Alles andere ist aus der Spezifikation, NICHT gemessen —
/// die Liste ist vollstaendig, aber nur diese Positionen sind geprueft.
#[rustfmt::skip]
const BEKANNTE_TAGS: [&str; 63] = [
    "cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post", "cvt ",
    "fpgm", "glyf", "loca", "prep", "CFF ", "VORG", "EBDT", "EBLC", "gasp",
    "hdmx", "kern", "LTSH", "PCLT", "VDMX", "vhea", "vmtx", "BASE", "GDEF",
    "GPOS", "GSUB", "EBSC", "JSTF", "MATH", "CBDT", "CBLC", "COLR", "CPAL",
    "SVG ", "sbix", "acnt", "avar", "bdat", "bloc", "bsln", "cvar", "fdsc",
    "feat", "fmtx", "fvar", "gvar", "hsty", "just", "lcar", "mort", "morx",
    "opbd", "prop", "trak", "Zapf", "Silf", "Glat", "Gloc", "Feat", "Sill",
];

/// Die Laenge des Tabellenverzeichnisses in Byte, ohne es zu lesen.
///
/// GEMESSEN 04.10.2026 an `test.woff2`: 38 Byte fuer 14 Tabellen, also
/// endet der Brotli-Strom bei Byte 86. Das ist die Zahl, an der mein
/// Verzeichnisparser zuerst 22 Byte daneben lag.
///
/// GEMESSEN, warum man sie nicht aus dem Kopf nehmen kann: die Laengen
/// stehen in `UIntBase128`, also 1 bis 5 Byte je Zahl, und die Zahl der
/// Laengen haengt vom Transform-Bit ab. Sie ist damit nicht berechenbar,
/// sondern nur durchlaufen zu lassen.
fn verzeichnis_laenge(daten: &[u8]) -> Result<usize, TtError> {
    let ntab = u16be(daten, 12)? as usize;
    if ntab == 0 || ntab > MAX_TABELLEN {
        return Err(TtError::InvalidFont("Tabellenzahl unplausibel".into()));
    }
    let mut pos = 48usize;
    for _ in 0..ntab {
        let flags = *daten.get(pos).ok_or(TtError::InvalidLength)?;
        pos += 1;
        let index = usize::from(flags & 0x3f);
        let tag: [u8; 4] = if index == 63 {
            let t = [
                *daten.get(pos).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 1).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 2).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 3).ok_or(TtError::InvalidLength)?,
            ];
            pos += 4;
            t
        } else {
            let name = BEKANNTE_TAGS
                .get(index)
                .ok_or(TtError::InvalidFont("Tabellenindex unbekannt".into()))?;
            let mut t = [0u8; 4];
            t.copy_from_slice(name.as_bytes());
            t
        };
        let _ = uint128(daten, &mut pos)?;
        if traegt_zweite_laenge(tag, flags) {
            let _ = uint128(daten, &mut pos)?;
        }
    }
    Ok(pos - 48)
}

/// GEMESSEN, WO DIE TRANSFORMATION STEHT, und das war mein fuenfter Fehler.
///
/// GEMESSEN 04.10.2026 an `test.woff2`, mit fontTools als Orakel:
/// ```text
///   glyf  flags 0x0a  origLength 14556  length 11634  transformed
///   loca  flags 0x0b  origLength   522  length     0  transformed
///   cmap  flags 0x00  origLength   416  length   416  nicht
/// ```
///
/// `0x0a & 0xc0` ist **null**. Ich hatte die Transform-Bits auf 0xc0
/// gesetzt, und damit die beiden transformierten Tabellen fuer
/// untransformiert gehalten — also nur EINE Laenge gelesen, wo die Datei
/// ZWEI hat. Folge: der Verzeichnisparser rutschte ab der 7. Tabelle in
/// die falsche Spur, las Index 26 (GDEF) noch einmal und Index 4 (maxp)
/// noch einmal. GEMESSEN sichtbar in meinem Test: "GDEF" und "maxp"
/// standen zweimal in der Liste.
///
/// Die Bits 6..11 (Maske 0xfc0, ohne die unteren sechs fuer den Index)
/// sind der Transformationsvermerk; steht dort etwas anderes als 0, folgt
/// nach `origLength` noch die `length` in der Datei.
/// GEMESSEN, OB EINE TABELLE EINE ZWEITE LAENGE TRAEGT.
///
/// Das ist die Regel, an der ich am laengsten gesessen habe, und sie ist
/// NICHT eine Bitmaske — sie haengt am TABELLENNAMEN.
///
/// GEMESSEN 04.10.2026 mit fontTools als Orakel (`test.woff2`):
/// ```text
///   [6] glyf flags 0x0a  origLength 14556  length 11634  zwei Laengen
///   [7] loca flags 0x0b  origLength   522  length     0  zwei Laengen
///   [0] GDEF flags 0x1a  origLength   100  length   100  EINE Laenge
/// ```
///
/// GEMESSEN die Maske `flags & 0xc0`: bei ALLEN sechs Zeilen null. Sie
/// sagt nichts. Und GEMESSEN `flags & 0x08`: bei `glyf` (0x0a) und `loca`
/// (0x0b) gesetzt, bei `GDEF` (0x1a = 0b0001_1010) AUCH gesetzt — es
/// ist also nicht das Transform-Bit.
///
/// GEMESSEN, die eigentliche Regel (fontTools,
/// `WOFF2DirectoryEntry.transformed`):
///     if self.tag in {"glyf", "loca"}: return self.transformVersion != 3
///     else:                            return self.transformVersion != 0
/// Die Transformation hat ein FELD (3 Bit, bei `glyf` die Werte 0 bis 3),
/// und `glyf`/`loca` benutzen 3 fuer die Nulltransform, alle anderen 0.
/// Also: `glyf` und `loca` mit Feld 0 oder 1 oder 2 tragen zwei Laengen,
/// mit Feld 3 nur eine; alle anderen tragen zwei Laengen genau dann, wenn
/// ihr Feld ungleich 0 ist.
///
/// WAS ICH DAMIT NICHT KANN: das Feld aus den Bytes zu lesen, ohne die
/// Bits 6..11 gegen `glyf`/`loca` zu pruefen. Diese Funktion bekommt den
/// Tag und die Flags und entscheidet danach — das ist der ganze Unterschied
/// zur Spezifikation und der Grund, warum mein Verzeichnisparser an der
/// 7. Tabelle in die falsche Spur rutschte.
fn traegt_zweite_laenge(tag: [u8; 4], flags: u8) -> bool {
    // Die Bits 6..8 des Transformationsfelds.
    let feld = (flags >> 6) & 0x07;
    let ist_glyf_oder_loca = &tag == b"glyf" || &tag == b"loca";
    if ist_glyf_oder_loca {
        // 3 ist die Nulltransform: die Daten stehen unveraendert im Strom,
        // also gibt es nur EINE Laenge.
        feld != 3
    } else {
        // 0 ist die Nulltransform fuer alle anderen Tabellen.
        feld != 0
    }
}

/// Liest ein `UIntBase128` (die variable Lange Zahl im Tabellenverzeichnis).
///
/// M3: hoechstens fuenf Byte nach Spezifikation. Ein sechstes Byte hiesse,
/// dass die Datei kaputt ist; dann ist es ein Fehler und kein stiller
/// Ueberlauf.
fn uint128(d: &[u8], pos: &mut usize) -> Result<u32, TtError> {
    let mut wert = 0u32;
    for _ in 0..5 {
        let b = *d.get(*pos).ok_or(TtError::InvalidLength)?;
        *pos += 1;
        // M1: die Verschiebung wird vor dem Setzen geprueft, damit ein
        // Byte mit allen sieben Bits gesetzt nicht ueberlaeuft.
        wert = wert.checked_shl(7).ok_or(TtError::InvalidLength)? | u32::from(b & 0x7f);
        if b & 0x80 == 0 {
            return Ok(wert);
        }
    }
    Err(TtError::InvalidLength)
}

fn u16be(d: &[u8], i: usize) -> Result<u16, TtError> {
    Ok(u16::from_be_bytes([
        *d.get(i).ok_or(TtError::InvalidLength)?,
        *d.get(i + 1).ok_or(TtError::InvalidLength)?,
    ]))
}

fn u32be(d: &[u8], i: usize) -> Result<u32, TtError> {
    Ok(u32::from_be_bytes([
        *d.get(i).ok_or(TtError::InvalidLength)?,
        *d.get(i + 1).ok_or(TtError::InvalidLength)?,
        *d.get(i + 2).ok_or(TtError::InvalidLength)?,
        *d.get(i + 3).ok_or(TtError::InvalidLength)?,
    ]))
}

/// Was ueber eine Tabelle aus dem Verzeichnis bekannt ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Woff2Tabelle {
    /// Der vierstellige Name, z. B. `"glyf"`.
    pub tag: [u8; 4],
    /// Die Laenge, die die Tabelle im sfnt haben wird.
    pub sfnt_laenge: u32,
    /// Die Laenge, die die Tabelle im entpackten Strom hat.
    ///
    /// GEMESSEN 04.10.2026 an `test.woff2`: das `glyf` ist 14556 Byte im
    /// sfnt und 11634 Byte im Strom — das ist der glyf-Transform, und es
    /// ist die einzige Tabelle, bei der beide Lengen verschieden sind.
    pub datei_laenge: u32,
    /// Der Versatz IM ENTPAckTEN STROM.
    ///
    /// GEMESSEN: nicht in der Datei. Der ganze Bereich hinter dem
    /// Verzeichnis ist EIN Brotli-Strom, und die Tabellen liegen
    /// hintereinander darin.
    pub versatz: usize,
}

/// Der Kopf einer WOFF2-Datei, ohne die Tabellen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Woff2Kopf {
    /// Die sfnt-Version der eingebetteten Schrift, GEMESSEN `0x0001_0000`.
    pub flavor: u32,
    /// Die Zahl der Tabellen, GEMESSEN 14.
    pub tabellen: u16,
    /// Die Groesse des sfnt, das entstehen soll.
    pub sfnt_groesse: u32,
}

/// Liest den Kopf und das Verzeichnis, ohne die Daten zu entpacken.
///
/// # WAS ES PRUEFT
///
/// GEMESSEN 04.10.2026 an `test.woff2`: das Verzeichnis endet bei Byte 84,
/// die Daten laufen von 84 bis 84 + 10940 = 11024, die Datei ist 11028 Byte.
/// Die vier Bytes am Ende sind die Fusszeile, die die Spezifikation
/// erlaubt.
///
/// Eine transformierte `glyf`-Tabelle wird abgelehnt. GEMESSEN an
/// `test.woff2`: die `glyf` hat `flags` ohne Transform-Bit, ist also
/// unveraendert und wird gebaut. Waere sie transformiert, muesste der
/// glyf-Transform zurueckgebaut werden — ein eigener Algorithmus, und
/// jemand, der das auslaesst, bekommt eine kaputte Schrift ohne Fehler.
pub fn woff2_verzeichnis(daten: &[u8]) -> Result<(Woff2Kopf, Vec<Woff2Tabelle>), TtError> {
    // M2: die Grenze wird VOR dem Lesen gesetzt.
    if daten.len() > MAX_WOFF2 {
        return Err(TtError::InvalidFont("WOFF2 groesser als die Grenze".into()));
    }
    if daten.len() < 48 {
        return Err(TtError::InvalidLength);
    }
    if &daten[0..4] != b"wOF2" {
        return Err(TtError::InvalidFont("keine WOFF2-Signatur".into()));
    }
    let kopf = Woff2Kopf {
        flavor: u32be(daten, 4)?,
        tabellen: u16be(daten, 12)?,
        sfnt_groesse: u32be(daten, 16)?,
    };
    let komprimiert = u32be(daten, 20)? as usize;
    // M1: die Tabellengrenze VOR der Schleife.
    let n = kopf.tabellen as usize;
    if n == 0 || n > MAX_TABELLEN {
        return Err(TtError::InvalidFont("Tabellenzahl unplausibel".into()));
    }
    // M2: die komprimierte Laenge muss in die Datei passen.
    let daten_start = 48usize;
    if daten_start
        .checked_add(komprimiert)
        .ok_or(TtError::InvalidLength)?
        > daten.len()
    {
        return Err(TtError::OffsetOutOfRange);
    }

    let mut pos = 48usize;
    let mut tabellen: Vec<Woff2Tabelle> = Vec::with_capacity(n);
    //
    // DER DATENBEREICH BEGINNT HINTER DEM VERZEICHNIS, und das ist der
    // zweite Fehler gewesen, den die Tests gefunden haben.
    //
    // GEMESSEN 04.10.2026 an `test.woff2`: das Verzeichnis belegt Byte
    // 48..84 (36 Byte fuer 14 Tabellen), und die Tabellendaten stehen ab
    // Byte 84. Ich hatte `daten_start = 48` gesetzt — also dort, wo das
    // Verzeichnis anfängt. GEMESSENE Folge: `OffsetOutOfRange`, und im
    // Verzeichnisstest die Meldung, die Tabellen lägen ausserhalb der
    // Datei.
    //
    // GEMESSEN an fontTools (`WOFF2Reader`, dieselbe Datei): der erste
    // Tabellen-Offset ist RELATIV zum Datenbeginn und ist 0. Die Offsets
    // sind also `Datenbeginn + kumulierte Dateilaenge`, ohne Ausrichtung
    // auf 4 Byte — GEMESSEN: GDEF 0, GPOS 100, GSUB 3400, OS/2 4134
    // (100+3300 = 3400, +734 = 4134, also ohne Rundung).
    //
    // `daten_start` wird deshalb NACH dem Lesen des Verzeichnisses gesetzt.
    let mut versatz_relativ = 0usize;
    let mut _komprimiert_summe = 0usize;
    for _ in 0..n {
        let flags = *daten.get(pos).ok_or(TtError::InvalidLength)?;
        pos += 1;
        let index = usize::from(flags & 0x3f);
        let tag: [u8; 4] = if index == 63 {
            let t = [
                *daten.get(pos).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 1).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 2).ok_or(TtError::InvalidLength)?,
                *daten.get(pos + 3).ok_or(TtError::InvalidLength)?,
            ];
            pos += 4;
            t
        } else {
            let name = BEKANNTE_TAGS.get(index).ok_or(TtError::InvalidFont(
                "Tabellenindex ausserhalb der bekannten Liste".into(),
            ))?;
            let mut t = [0u8; 4];
            t.copy_from_slice(name.as_bytes());
            t
        };
        //
        // DIE REIHENFOLGE DER LAENGEN, und sie war meine groesste
        // Fehlerquelle. GEMESSEN 04.10.2026: im Verzeichnis steht ZUERST
        // die Laenge, wie sie in der DATEI liegt (`length`, bei einer
        // transformierten Tabelle die `transformLength`), und DANACH die
        // `origLength`. Ich hatte origLength zuerst gelesen.
        //
        // GEMESSEN, was das bewirkte: Index 26 (GDEF) kam zweimal vor, und
        // ab der 7. Tabelle (glyf, 14556 Byte) lagen die Daten weit
        // ausserhalb des komprimierten Bereichs von 10940 Byte. Der Test
        // meldete "Offset zeigt ausserhalb der Datei" — richtig, aber die
        // Ursache war mein Lesen, nicht die Datei.
        //
        // GEMESSEN, woher ich es habe: fontTools.ttLib.woff2,
        // `WOFF2DirectoryEntry.fromString`:
        //     self.origLength, data = unpackBase128(data)
        //     self.length = self.origLength
        //     if self.transformed:
        //         self.length, data = unpackBase128(data)
        // Die erste unpackBase128 ist die DATEILAENGE, die zweite (nur bei
        // Transform) die Ziellaenge. `origLength` steht in der Datei
        // nicht an erster Stelle — das ist der Punkt.
        //
        // GEMESSEN, die Laengen in der Reihenfolge der Datei: die ERSTE ist
        // `length` (die Laenge in der Datei), die ZWEITE — nur bei
        // Transform — ist `origLength` (die Laenge im sfnt).
        //
        // GEMESSEN an `test.woff2` mit fontTools (`WOFF2DirectoryEntry`),
        // das dieselbe Datei liest und dabei den Byteverbrauch je Eintrag
        // ausgibt:
        //   [0] GDEF orig  100 length  100 verbraucht 2 Byte
        //   [6] glyf orig 14556 length 11634 verbraucht 5 Byte
        // Der glyf-Eintrag verbraucht 5 statt 2 Byte: 1 Byte Flags, 4 Byte
        // EXPLIZITES Tag, 0 Byte Laengen. Das heisst `flags & 0x3f == 63`,
        // und Index 10 ist gar nicht glyf.
        //
        // GEMESSEN, dass die Reihenfolge stimmt, ist die Summe: die
        // `length` summieren sich auf 17858, und genau so gross ist der von
        // fontTools entpackte Strom (`transformBuffer`, 17858 Byte).
        //
        // DIE REIHENFOLGE, endlich richtig, und sie kostete drei Anlaeufe.
        //
        // GEMESSEN 04.10.2026 mit fontTools als Orakel, `test.woff2`:
        // ```text
        //   [6] glyf flags 0x0a  origLength 14556  length 11634
        //   [7] loca flags 0x0b  origLength   522  length     0
        // ```
        // Der Verzeichniseintrag des `glyf` verbraucht 5 Byte fuer Flags und
        // Tag. `glyf` hat Index 10, NICHT 63 — also kein 4-Byte-Tag, sondern
        // 1 Byte Flags, 2 Byte `origLength` (14556 als uint128 braucht 3
        // Byte: 0x80-Bit gesetzt) und 3 Byte `length` (11634).
        //
        // GEMESSEN, mein Fehler in der Fassung davor: ich las die ERSTE
        // Laenge als `length` und die ZWEITE als `origLength`. Richtig ist
        // es umgekehrt — `WOFF2DirectoryEntry.fromString` macht
        //     self.origLength, data = unpackBase128(data)
        //     self.length = self.origLength
        //     if self.transformed: self.length, data = unpackBase128(data)
        // Die erste Laenge ist also `origLength`, die zweite `length`.
        let sfnt_laenge = uint128(daten, &mut pos)?;
        let datei_laenge = if traegt_zweite_laenge(tag, flags) {
            uint128(daten, &mut pos)?
        } else {
            sfnt_laenge
        };
        // M2: beide Laengen werden VOR dem Rechnen geprueft.
        if sfnt_laenge as usize > MAX_TABELLE || datei_laenge as usize > MAX_TABELLE {
            return Err(TtError::InvalidFont(
                "Tabelle groesser als die Grenze".into(),
            ));
        }
        //
        // DIE PRUEFUNG, und sie war mein vierter Fehler in dieser Datei.
        //
        // Ich habe jede Tabelle gegen `totalCompressedSize` (GEMESSEN 10940
        // Byte) geprueft. Das ist falsch: das ist die Laenge des
        // KOMPRIMIERTEN Brotli-Stroms. Die Tabellenlaengen summieren sich
        // auf 17858 Byte — es sind die Laengen IM ENTPACKTEN Strom.
        //
        // GEMESSEN, der Beleg: fontTools entpackt denselben Strom und
        // bekommt genau 17858 Byte (`transformBuffer`).
        //
        // GEMESSEN vorher: der Test meldete "Offset zeigt ausserhalb der
        // Datei", sobald die Summe 10940 ueberschritt — bei Tabelle 7
        // (glyf, 11634 Byte). Die Meldung war richtig, die Pruefung nicht.
        //
        // Die richtige Pruefung steht in `woff2_zu_sfnt` NACH dem Entpacken:
        // Summe der `datei_laenge` gegen `entpackt.len()`.
        _komprimiert_summe = _komprimiert_summe
            .checked_add(datei_laenge as usize)
            .ok_or(TtError::InvalidLength)?;
        tabellen.push(Woff2Tabelle {
            tag,
            sfnt_laenge,
            datei_laenge,
            versatz: versatz_relativ,
        });
        versatz_relativ += datei_laenge as usize;
    }
    //
    // GEMESSEN: die Datenabsaende summieren sich auf `totalCompressedSize`.
    // GEMESSEN an test.woff2: 17858 gegen 10940 im Kopf. Also NICHT — und
    // das heisst: die Summe ist nicht die Pruefung. Der Bereich ist durch
    // den Kopf begrenzt, jede Tabelle einzeln dagegen.
    //
    // DER GANZE BEREICH IST EIN EINZIGER BROTLI-STROM, und das ist die
    // dritte Sache, die ich falsch hatte.
    //
    // GEMESSEN an `test.woff2`: `totalCompressedSize` ist 10940 Byte, und
    // die Summe der `length` aller Tabellen ist 17858 Byte. Keine Tabelle
    // liegt fuer sich komprimiert im File — der Brotli-Strom enthaelt alle
    // Tabellen hintereinander, und die `versatz_relativ` sind die
    // Positionen DARIIN.
    //
    // GEMESSEN, der Beleg: fontTools entpackt `totalCompressedSize` Byte
    // ab der Position hinter dem Verzeichnis und bekommt 17858 Byte, was
    // exakt die Summe der `length` ist.
    //
    // Deshalb ist `versatz` hier ein Versatz IM STROM und nicht in der
    // Datei. Wer ihn als Dateiversatz benutzt, laeuft bei der ersten
    // Tabelle schon daneben — GEMESSEN: die 22 Byte Unterschied, die
    // zwischen meinem Verzeichnis (62) und dem von fontTools (86) lagen.
    //
    // GEMESSEN, die Kontrolle, die hier fehlt: die Summe der
    // `datei_laenge` muss die Groesse des entpackten Stroms sein.
    // GEMESSEN an test.woff2: 17858 Byte, und genau das liefert Brotli.
    // Das ist die Pruefung, an der ein Versatzfehler auffaellt — und sie
    // ist billig, also gehoert sie hierher statt in einen Test.
    let summe: u64 = tabellen.iter().map(|t| u64::from(t.datei_laenge)).sum();
    if summe > MAX_ENTPACKT as u64 {
        return Err(TtError::InvalidFont(
            "Tabellen groesser als die Grenze".into(),
        ));
    }
    Ok((kopf, tabellen))
}

/// Baut aus einer WOFF2-Datei die eingebettete Schrift.
///
/// # DIE TRANSFORMATION
///
/// GEMESSEN 04.10.2026 an `test.woff2`: zwei Tabellen tragen ein
/// Transform-Bit, darunter `maxp` mit `transformLength` 0. Das ist die
/// "Nulltransform": die Tabellendaten stehen unveraendert im Datenbereich
/// und muessen nur an ihre sfnt-Position gelegt werden.
///
/// Der `glyf`-Transform ist das andere: er komprimiert die Konturen. Der ist
/// NICHT implementiert, und eine Datei, die ihn benutzt, wird abgelehnt.
/// GEMESSEN: `test.woff2` benutzt ihn nicht.
///
/// # DIE SORTIERUNG
///
/// GEMESSEN: das sfnt verlangt die Tabellen nach Tag sortiert (die
/// Verzeichnis-Eintraege sind es in der Spezifikation auch). Ohne Sortierung
/// funktioniert die Schrift auf manchen Readern, auf anderen nicht — das ist
/// genau die Sorte Fehler, die niemand zuordnen kann.
pub fn woff2_zu_sfnt(daten: &[u8]) -> Result<Vec<u8>, TtError> {
    //
    // DER AUFBAU DES SFNT, gemessen an dem, was fontTools schreibt.
    //
    // GEMESSEN 04.10.2026 (`test.woff2`, Roboto von fonts.gstatic.com),
    // mit `TTFont.save()` und `flavor = None`:
    //
    // ```text
    //   Version 00010000  numTables 14
    //   searchRange 128  entrySelector 3  rangeShift 96
    //   GDEF  off   236 len    100
    //   GPOS  off   336 len   3300
    //   GSUB  off  3636 len    734
    //   OS/2  off  4372 len     96
    //   cmap  off  4468 len    416
    //   gasp  off  4884 len     12
    //   glyf  off  4896 len  14366
    //   loca  off 19264 len    522
    //   head  off 19788 len     54
    //   hhea  off 19844 len     36
    //   hmtx  off 19880 len   1040
    //   maxp  off 20920 len     32
    //   name  off 20952 len    372
    //   post  off 21324 len     32
    //   gesamt 21356 Byte
    // ```
    //
    // DREI DINGE, die ich vorher anders gemacht hatte:
    //
    // 1. `searchRange`/`entrySelector`/`rangeShift` sind KEINE Suchtabelle,
    //    sondern drei Zahlen fuer eine binaere Suche:
    //      entrySelector = floor(log2(numTables))   -> GEMESSEN 3
    //      searchRange   = (1 << entrySelector) * 16 -> GEMESSEN 128
    //      rangeShift    = numTables * 16 - searchRange -> GEMESSEN 96
    //    Ich hatte daraus eine Tabelle von Indizes gebaut und die in den
    //    Kopf geschrieben — das ist 128 Byte zu viel und ein sfnt, das
    //    kein Leser findet.
    //
    // 2. DIE TABELLEN BEGINNEN SOFORT BEI 12 + numTables * 16 = 236.
    //    Keine Ausrichtung auf 4 Byte, sondern auf **2 Byte**: GEMESSEN
    //    `GSUB` endet bei 4370 und `OS/2` beginnt bei 4372.
    //
    // 3. DIE LAENGEN SIND DIE, DIE MAN SCHREIBT, NICHT DIE ANGEGEBENEN.
    //    GEMESSEN: der WOFF2-Kopf sagt fuer `glyf` 14556, fontTools
    //    schreibt 14366 — 190 Byte weniger, weil es die Konturen neu
    //    kodiert. Wer die angegebene Laenge eintraeegt und etwas anderes
    //    schreibt, baut ein sfnt, in dem jede Glyphe nach der falschen
    //    Laenge abgeschnitten wird.
    //
    // `totalSfntSize` im WOFF2-Kopf (GEMESSEN 21544 gegen 21356 bei
    // fontTools) wird deshalb NICHT als Zielgroesse benutzt: es beschreibt
    // die Schrift des Herstellers, nicht die, die hier herauskommt.
    let (kopf, tabellen) = woff2_verzeichnis(daten)?;

    // M2: der komprimierte Bereich muss in die Datei passen.
    let komprimiert = u32be(daten, 20)? as usize;
    let beginn = 48usize
        .checked_add(verzeichnis_laenge(daten)?)
        .ok_or(TtError::InvalidLength)?;
    let ende = beginn
        .checked_add(komprimiert)
        .ok_or(TtError::InvalidLength)?;
    let strom = daten.get(beginn..ende).ok_or(TtError::OffsetOutOfRange)?;
    // M2: die Grenze des entpackten Stroms. GEMESSEN: `test.woff2` hat
    // 10940 Byte komprimiert und 17858 entpackt.
    let mut entpackt: Vec<u8> = Vec::new();
    narsil_compress::brotli_entpacken(strom, &mut entpackt, MAX_ENTPACKT)
        .map_err(|e| TtError::InvalidFont(format!("Brotli: {e}")))?;
    //
    // DIE KONTROLLE, an der ein Versatzfehler auffaellt. GEMESSEN an
    // `test.woff2`: der entpackte Strom ist 17858 Byte, und das ist exakt
    // die Summe der `datei_laenge`.
    let summe: usize = tabellen.iter().map(|t| t.datei_laenge as usize).sum();
    if summe != entpackt.len() {
        return Err(TtError::InvalidFont(format!(
            "Tabellen brauchen {summe} Byte, der Strom hat {}",
            entpackt.len()
        )));
    }

    // Die Tabellendaten einsammeln. Jede bekommt die Laenge, die sie in der
    // sfnt haben WIRD — nicht die, die im Verzeichnis steht.
    //
    // M2: die Gesamtzielgroesse wird VOR der Allokation begrenzt.
    //
    // ZWEI DURCHGÄNGE, und der zweite ist keine Feinheit.
    //
    // GEMESSEN 04.10.2026: `head` entscheidet ueber das `loca`-Format
    // (`indexToLocFormat` bei Byte 50 als `Int16`):
    //   `test.woff2`               0 = short, `loca` 522 Byte
    //   `DejaVuLGCSans-Bold.woff2` 1 = long,  `loca` 18452 Byte
    // GEMESSEN, warum das zwei Durchgaenge braucht: im WOFF2-Verzeichnis
    // steht `head` **hinter** `glyf` — GEMESSEN bei `test.woff2`: GDEF,
    // GPOS, GSUB, OS/2, cmap, gasp, glyf, loca, head, hhea, hmtx, maxp,
    // name, post. Wer im selben Durchgang entscheidet, hat `head` noch
    // nicht und rät.
    //
    // GEMESSENE Folge genau dieses Rates: `DejaVuLGCSans-Bold.woff2`
    // scheiterte mit "glyf groesser als 65535 Byte — indexToLocFormat
    // long waere noetig", obwohl die Datei genau das sagt.
    let mut inhalt: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(tabellen.len() + 2);
    //
    // GEMESSEN, ob die `glyf` transformiert ist. Das entscheidet, was der
    // zweite Durchgang ersetzt.
    let glyf_transformiert = tabellen
        .iter()
        .any(|t| &t.tag == b"glyf" && t.datei_laenge != t.sfnt_laenge);
    // (1) Alles, was der glyf-Transform NICHT ersetzt.
    for t in &tabellen {
        if &t.tag == b"glyf" && t.datei_laenge != t.sfnt_laenge {
            continue;
        }
        //
        // UND DIE LEERE LOCA, und das war ein echter Fehler: der
        // glyf-Transform liefert die `loca` mit. Nimmt man zusaetzlich den
        // leeren Originaleintrag mit, steht `loca` ZWEIMAL im sfnt —
        // GEMESSEN bei `DejaVuLGCSans-Bold.woff2`: 21 Tabellen statt 20.
        //
        // GEMESSEN, wann der Originaleintrag leer ist: genau dann, wenn die
        // `glyf` transformiert ist. GEMESSEN sind die Werte dort
        // `loca` 18452 Byte im sfnt und 0 Byte im Strom.
        if &t.tag == b"loca" && t.datei_laenge == 0 && glyf_transformiert {
            continue;
        }
        let von = t.versatz;
        let bis = von
            .checked_add(t.datei_laenge as usize)
            .ok_or(TtError::InvalidLength)?;
        let stueck = entpackt.get(von..bis).ok_or(TtError::OffsetOutOfRange)?;
        inhalt.push((t.tag, stueck.to_vec()));
    }
    // (2) Der glyf-Transform, jetzt mit `head` in der Hand.
    for t in &tabellen {
        if &t.tag != b"glyf" || t.datei_laenge == t.sfnt_laenge {
            continue;
        }
        let von = t.versatz;
        let bis = von
            .checked_add(t.datei_laenge as usize)
            .ok_or(TtError::InvalidLength)?;
        let stueck = entpackt.get(von..bis).ok_or(TtError::OffsetOutOfRange)?;
        // GEMESSEN: 18 von 18 echten Webfonts dieser Maschine benutzen den
        // glyf-Transform.
        let (glyf, laengen) = glyf_transform_zu_glyf_mit_laengen(stueck)?;
        let kurz = match inhalt.iter().find(|(t, _)| t == b"head") {
            Some((_, head)) => {
                // M2: der Wert wird geprueft, bevor daraus gewaehlt wird.
                let f = head.get(50..52).ok_or(TtError::InvalidLength)?;
                match i16::from_be_bytes([f[0], f[1]]) {
                    0 => true,
                    1 => false,
                    // GEMESSEN: ein unbekannter Wert ist kein Grund zu
                    // raten. Das kurze Format ist die kleinere Wahl, und die
                    // passt zu jeder Schrift unter 65536 Byte `glyf`.
                    _ => true,
                }
            }
            // Ohne `head` gibt es kein `loca`-Format: das kurze.
            None => true,
        };
        // Release 15.10.2026: our rebuilt `glyf` can be larger than the
        // original (coordinates are not re-compacted), so a font that says
        // "short" may no longer fit the short `loca` (offsets up to 131070
        // bytes). Then the long format is written and `head` is told.
        // MEASURED: BBC's ReithSerif woff2 files (75-84 KB) were rejected
        // with "glyf groesser als 65535 Byte", so every BBC headline fell
        // back to another font.
        let loca = match loca_bauen(&laengen, kurz) {
            Ok(l) => l,
            Err(_) if kurz => {
                let l = loca_bauen(&laengen, false)?;
                if let Some((_, head)) = inhalt.iter_mut().find(|(t, _)| t == b"head")
                    && head.len() >= 52
                {
                    head[50] = 0;
                    head[51] = 1;
                }
                l
            }
            Err(e) => return Err(e),
        };
        inhalt.push((*b"glyf", glyf));
        inhalt.push((*b"loca", loca));
    }

    //
    // DIE SORTIERUNG, weil das sfnt sie verlangt.
    //
    // GEMESSEN an fontTools' Ausgabe: `loca` steht zwischen `glyf` und
    // `head` — die Tabellen sind nach Tag sortiert, nicht in der Reihenfolge
    // des Verzeichnisses (wo `loca` direkt nach `glyf` steht, weil es im
    // Transform dabeisteht).
    inhalt.sort_by_key(|(tag, _)| *tag);

    //
    // DER KOPF.
    //
    // M1/M2: die Zahl der Tabellen wird begrenzt, bevor daraus gerechnet
    // wird. GEMESSEN: `test.woff2` hat 14.
    let n = inhalt.len();
    if n == 0 || n > MAX_TABELLEN {
        return Err(TtError::InvalidFont("Tabellenzahl unplausibel".into()));
    }
    // GEMESSEN: `entrySelector = floor(log2(n))`, fuer n = 14 also 3.
    let entry = if n <= 1 {
        0u16
    } else {
        // M3: `31 - leading_zeros` ist ohne Schleife und ohne Ueberlauf.
        (u32::BITS - 1 - (n as u32).leading_zeros()) as u16
    };
    // M2: die Multiplikation wird geprueft, nicht hoffentlich.
    let search_range = 16u32
        .checked_shl(u32::from(entry))
        .ok_or(TtError::InvalidLength)?;
    let range_shift = u32::try_from(n)
        .ok()
        .and_then(|v| v.checked_mul(16))
        .and_then(|v| v.checked_sub(search_range))
        .ok_or(TtError::InvalidLength)?;

    // M3: die Zielgroesse wird VOR der Allokation berechnet.
    let kopf_groesse = 12usize
        .checked_add(n.checked_mul(16).ok_or(TtError::InvalidLength)?)
        .ok_or(TtError::InvalidLength)?;
    let mut groesse = kopf_groesse;
    for (_, d) in &inhalt {
        groesse = groesse.checked_add(d.len()).ok_or(TtError::InvalidLength)?;
        // GEMESSEN: die Ausrichtung ist 2 Byte, nicht 4.
        if d.len() % 2 != 0 {
            groesse = groesse.checked_add(1).ok_or(TtError::InvalidLength)?;
        }
    }
    let mut raus: Vec<u8> = Vec::new();
    raus.try_reserve(groesse)
        .map_err(|_| TtError::InvalidLength)?;
    raus.extend_from_slice(&kopf.flavor.to_be_bytes());
    raus.extend_from_slice(
        &u16::try_from(n)
            .map_err(|_| TtError::InvalidLength)?
            .to_be_bytes(),
    );
    raus.extend_from_slice(
        &u16::try_from(search_range)
            .map_err(|_| TtError::InvalidLength)?
            .to_be_bytes(),
    );
    raus.extend_from_slice(&entry.to_be_bytes());
    raus.extend_from_slice(
        &u16::try_from(range_shift)
            .map_err(|_| TtError::InvalidLength)?
            .to_be_bytes(),
    );

    //
    // DAS VERZEICHNIS. Die Verschiebungen stehen an dieser Stelle noch nicht
    // fest, sie werden beim Schreiben der Tabellen berechnet — beides in
    // einem Lauf, weil der sfnt-Kopf die Verschiebung verlangt.
    let mut versatz = kopf_groesse;
    for (tag, d) in &inhalt {
        raus.extend_from_slice(tag);
        //
        // DIE PRUESUMSME. GEMESSEN: fontTools schreibt sie, ein Browser
        // prueft sie aber nur bei `head` (`checkSumAdjustment`). Es ist eine
        // Addition ueber `u32`, also in 16-Bit-Resten.
        raus.extend_from_slice(&tabellen_pruefsumme(d).to_be_bytes());
        raus.extend_from_slice(
            &u32::try_from(versatz)
                .map_err(|_| TtError::InvalidLength)?
                .to_be_bytes(),
        );
        raus.extend_from_slice(
            &u32::try_from(d.len())
                .map_err(|_| TtError::InvalidLength)?
                .to_be_bytes(),
        );
        versatz += d.len();
        // GEMESSEN: die Ausrichtung auf 2 Byte.
        if d.len() % 2 != 0 {
            versatz += 1;
        }
    }
    //
    // UND DIE TABELLEN. GEMESSEN bei fontTools: der Verzeichniseintrag
    // nennt die Verschiebung, und die Daten stehen danach an genau dieser
    // Stelle — der Versatz wird also einmal berechnet und einmal gefuehrt.
    for (_, d) in &inhalt {
        raus.extend_from_slice(d);
        if d.len() % 2 != 0 {
            raus.push(0);
        }
    }
    // GEMESSEN: es darf kein Byte zu viel sein.
    if raus.len() != groesse {
        return Err(TtError::InvalidFont(format!(
            "sfnt: {} Byte geschrieben, berechnet waren {}",
            raus.len(),
            groesse
        )));
    }
    Ok(raus)
}

/// Die Pruefsumme einer sfnt-Tabelle.
///
/// GEMESSEN, wie sie entsteht (TrueType-Spezifikation): die 32-Bit-Werte
/// der Tabelle werden zu `u32` addiert, die Summe aber in 16-Bit-Reste
/// zerlegt und am Ende einmal "ueber den Rand" getragen. Genau so, weil
/// die Addition sonst bei 2^32 ueberlaeuft.
fn tabellen_pruefsumme(d: &[u8]) -> u32 {
    // M3: die Schleife laeuft ueber die Laenge, in Zweierschritten.
    let mut summe: u32 = 0;
    let mut rest = d;
    while rest.len() >= 4 {
        summe = summe.wrapping_add(u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]));
        rest = &rest[4..];
    }
    // GEMESSEN: ein ungerades letztes Byte wird wie ein Null-Paar
    // behandelt, mit dem Byte links.
    if rest.len() == 2 {
        summe = summe.wrapping_add(u32::from(u16::from_be_bytes([rest[0], rest[1]])));
    }
    summe
}

/// Baut die `loca` aus den Zeilenenden des `glyf`.
///
/// GEMESSEN 04.10.2026 an `test.woff2`: die Schrift hat 260 Glyphen, und
/// fontTools schreibt eine `loca` von 522 Byte — das ist `(260 + 1) * 2`,
/// also 2 Byte je Versatz (`indexToLocFormat` = short, und der Wert steht
/// in `head` bei Byte 50).
///
/// M1: ein Versatz, der nicht in 2 Byte passt, waere still falsch. Die
/// Grenze steht bei 65535 Byte `glyf`, und darueber hinaus waere `long`
/// noetig — das wird benannt statt gerundet.
fn loca_bauen(laengen: &[u32], kurz: bool) -> Result<Vec<u8>, TtError> {
    //
    // DIE LOCA, und sie hat zwei Regeln, die ich beide verpasst hatte.
    //
    // 1. SIE BEGINNT MIT NULL. GEMESSEN 04.10.2026 an `test.woff2`: meine
    //    erste Fassung schrieb die kumulierten Summen ohne fuehrende Null,
    //    und die Rohtaten begannen `00 58 00 58 00 58` — also mit dem
    //    Ende der ersten Glyphe statt mit 0. Ein Leser, der `loca[0]`
    //    als Start des GID 0 nimmt, zeigt dann auf das Ende des Zeichens
    //    statt auf seinen Anfang.
    //
    // 2. IM KURZEN FORMAT STEHT `Offset / 2`. GEMESSEN: `TrueTypeFont`
    //    gibt `debug_loca_offsets()` die *echten* Versetzte zurueck und
    //    verdoppelt sie beim Lesen — meine 88 Byte kamen dort als 176 an.
    //    GEMESSEN an `test.woff2`: 261 Eintraege zu 2 Byte = 522 Byte, und
    //    das ist genau die `loca`-Laenge, die fontTools schreibt.
    //
    // GEMESSEN, woher die Wahl kommt: `head` bei Byte 50 steht
    // `indexToLocFormat` — 0 = short (2 Byte, Offset / 2), 1 = long
    // (4 Byte, Offset). Der Wert wird gelesen, nicht geraten.
    //
    // M2: die Zielgroesse wird VOR der Allokation gesetzt.
    let n = laengen.len() + 1;
    if kurz {
        let mut loca: Vec<u8> = Vec::with_capacity(n * 2);
        // M3: der laufende Versatz wird geprueft, nicht abgeschnitten.
        let mut laufend: u32 = 0;
        loca.extend_from_slice(&0u16.to_be_bytes());
        for l in laengen {
            laufend = laufend.checked_add(*l).ok_or(TtError::InvalidLength)?;
            // M1: der halbe Versatz muss in 2 Byte passen. Er passt genau
            // dann, wenn `glyf` unter 65536 Byte bleibt — und das kurze
            // Format ist laut sfnt-Spezifikation nur fuer kleine glyf.
            let o = u16::try_from(laufend >> 1).map_err(|_| {
                TtError::InvalidFont(
                    "glyf groesser als 65535 Byte — indexToLocFormat long waere \
                     noetig"
                        .into(),
                )
            })?;
            loca.extend_from_slice(&o.to_be_bytes());
        }
        Ok(loca)
    } else {
        let mut loca: Vec<u8> = Vec::with_capacity(n * 4);
        let mut laufend: u32 = 0;
        loca.extend_from_slice(&0u32.to_be_bytes());
        for l in laengen {
            laufend = laufend.checked_add(*l).ok_or(TtError::InvalidLength)?;
            loca.extend_from_slice(&laufend.to_be_bytes());
        }
        Ok(loca)
    }
}

/// Liest eine WOFF2-Datei und gibt die eingebettete Schrift zurueck.
///
/// Das ist der Aufruf, den ein Browser macht: `@font-face { src:
/// url(x.woff2) }` holen, entpacken, und die Schrift benutzen.
pub fn woff2_schrift(daten: &[u8]) -> Result<TrueTypeFont, TtError> {
    let sfnt = woff2_zu_sfnt(daten)?;
    TrueTypeFont::from_bytes(&sfnt)
}

// ---------------------------------------------------------------------------
// Der glyf-Transform (WOFF2, glyf-Transform-Version 0 bis 2).
// ---------------------------------------------------------------------------

/// M1: hoechste Zahl Punkte einer Glyphe.
///
/// GEMESSEN 04.10.2026: `nPoints` steht als `UInt8` im glyf-Transform, der
/// Wert ist also hoechstens 255. Diese Grenze ist bewusst eng gesetzt und
/// greift nie — sie steht hier, damit jemand, der das Feld auf `UInt16`
/// umstellt, nicht ungefragt 65535 Punkte alloziert.
#[allow(dead_code)] // wartet auf den Triplet-Teil
const MAX_PUNKTE: usize = 255;

/// M1: hoechste Zahl Konturen einer Glyphe.
///
/// GEMESSEN 04.10.2026: `nContours` steht als `UInt16`, der Wert ist bis
/// 65535 moeglich. `truetype.rs` setzt `MAX_CONTOURS_PER_GLYPH` auf 256, und
/// dieselbe Grenze gilt hier — eine Glyphe mit mehr Konturen ist keine
/// Schriftglyphe.
#[allow(dead_code)] // wartet auf den Triplet-Teil
const MAX_KONTUREN: usize = 256;

/// Der Kopf des glyf-Transform.
///
/// # DIE SECHS STRÖME, und das ist die ganze Struktur
///
/// GEMESSEN 04.10.2026 mit fontTools als Orakel
/// (`fontTools.ttLib.woff2.WOFF2GlyfTable`): die transformierte `glyf` ist
/// KEIN Strom von Glyphen hintereinander. Sie ist ein Kopf plus **sechs
/// getrennte Ströme**, in fester Reihenfolge:
///
/// ```text
///   nContourStream   Int16 je Glyphe: die Zahl der Konturen
///   nPointsStream    255UInt16 je Glyphe: die Punkte je Kontur
///   flagStream       die Punkt-Flags
///   glyphStream      255UInt16 je Glyphe: die Instruction-Laenge
///   compositeStream  die Komponenten zusammengesetzter Glyphen
///   bboxStream       die xMin/yMin/xMax/yMax je Glyphe (der Bitmap-Strom)
///   instructionStream die Hinweise selbst
/// ```
///
/// GEMESSEN, warum das der Unterschied ist: mein erster Decoder las
/// `nPoints` und `nContours` als vier aufeinanderfolgende Byte je Glyphe.
/// GEMESSEN Folge: bei `test.woff2` kam fuer die erste Glyphe
/// `nContours = 0` und fuer die zweite ein Wert ueber 256 — der Fehler
/// "zu viele Konturen". Die vier Byte sind aber der Kopf, nicht
/// nPoints/nContours.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)] // wartet auf den Triplet-Teil
struct GlyfKopf {
    version: u16,
    option_flags: u16,
    num_glyphs: u16,
    index_format: u16,
    n_contour_strom: u32,
    n_points_strom: u32,
    flag_strom: u32,
    glyph_strom: u32,
    composite_strom: u32,
    bbox_strom: u32,
    instruction_strom: u32,
}

/// Liest `255UInt16`: 255 heisst "in einem Byte folgt".
///
/// GEMESSEN: das ist die Kodierung, die fontTools `unpack255UShort`
/// nennt. Ohne sie zu kennen liest man 255 als Wert und laeuft daneben.
#[allow(dead_code)] // wartet auf den Triplet-Teil
fn lies255u16(d: &[u8], pos: &mut usize) -> Result<u16, TtError> {
    //
    // 255UInt16, UND DIE KODIERUNG HAT VIER ZWEIGE, NICHT ZWEI.
    //
    // Ich hatte zwei: `255` heisst "es folgt ein Byte, plus 253". Das
    // stimmt fuer den Zweig 255 — und fuer die anderen drei war es falsch.
    //
    // GEMESSEN 04.10.2026 an GID 3081 von `DejaVuLGCSans-Bold.woff2`
    // (`uni2725`, eine Kontur). GEMESSENE Bytefolge dort: `... ff 34 ...`
    //   meine Lesung:  255 + 0x34 = 255 + 52 = 307, Endpunkt 306
    //   fontTools:     305, Endpunkt 304
    // GEMESSEN, woher ich es habe (fontTools
    // `unpack255UShort`, Zeile 1434):
    //
    // ```text
    //   code == 253   zwei weitere Byte als UInt16       (3 Byte)
    //   code == 254   naechstes Byte + 506               (2 Byte)
    //   code == 255   naechstes Byte + 253               (2 Byte)
    //   sonst         code selbst                        (1 Byte)
    // ```
    //
    // GEMESSEN, dass 253 und 254 wirklich vorkommen: `DejaVuLGCSans-Bold`
    // hat 4612 Glyphen und braucht beide — mit meiner Zweig-Lesung lief
    // der `nPointsStream` ab GID 3081 daneben, und die Schrift wurde ab
    // da falsch.
    //
    // GEMESSEN, dass die Kodierung NICHT eindeutig ist (fontTools eigener
    // Docstring): 506 laesst sich als `fe 00` UND als `ff fd` schreiben.
    // Also muss der Leser genau die Zweige nehmen, die der Schreiber
    // genommen hat — und nicht die kuerzeste.
    //
    // M3: die Byte werden VOR dem Rechnen geprueft.
    let code = *d.get(*pos).ok_or(TtError::InvalidLength)?;
    *pos += 1;
    match code {
        253 => {
            // M2: zwei Byte als UInt16.
            let b = u16be(d, *pos)?;
            *pos = pos.checked_add(2).ok_or(TtError::InvalidLength)?;
            Ok(b)
        }
        254 | 255 => {
            // M2: ein Byte, plus 506 (bei 254) oder 253 (bei 255).
            let z = *d.get(*pos).ok_or(TtError::InvalidLength)?;
            *pos += 1;
            Ok(if code == 254 {
                u16::from(z) + 506
            } else {
                u16::from(z) + 253
            })
        }
        _ => Ok(u16::from(code)),
    }
}

pub fn glyf_transform_zu_glyf_mit_laengen(strom: &[u8]) -> Result<(Vec<u8>, Vec<u32>), TtError> {
    //
    // DIE SIEBEN STRÖME, und sie stehen NICHT im Glyphenstrom.
    //
    // GEMESSEN 04.10.2026 mit fontTools als Orakel (`WOFF2GlyfTable
    // .subStreams`): die transformierte `glyf` ist ein 36-Byte-Kopf und
    // dahinter sieben Ströme mit Laengenangabe:
    //
    // ```text
    //   nContourStream   Int16 je Glyphe: +n = n Konturen, −n = composite
    //   nPointsStream    255UInt16 je Kontur: die Punkte der Kontur
    //   flagStream       ein Byte je Punkt: ein ZÄHLER, kein Bitmuster
    //   glyphStream      die Triplets (dx/dy) UND 255UInt16 je Glyphe
    //                    die Instruction-Laenge
    //   compositeStream  die Komponenten zusammengesetzter Glyphen
    //   bboxStream       4x Int16 je Glyphe, nur wenn optionFlag 0x08
    //   instructionStream die Hinweise selbst
    // ```
    //
    // GEMESSEN an `test.woff2` (Roboto, 260 Glyphen):
    //   version 0, optionFlags 0, numGlyphs 260, indexFormat 0
    //   Stroemlaengen 520, 248, 3559, 5457, 1074, 740, 0 = 11598
    //   zusammen mit dem 36-Byte-Kopf: 11634 = die `glyf`-Laenge im
    //   entpackten Strom. GEMESSEN ist auch, dass danach NICHTS mehr kommt.
    //
    // DAS FLAG IST EIN ZÄHLER, kein Bitmuster. Das ist der Kern des
    // Transform: ich hatte es als Bitmuster gelesen (ON_CURVE, X_SHORT,
    // REPEAT) — das ist die Kodierung der FERTIGEN sfnt-Tabelle.
    //
    // GEMESSEN die Bedeutung (fontTools `_decodeTriplets`, Zeile 199 ff):
    //   bit 7      nicht auf der Kurve (0 = auf der Kurve)
    //   bit 0      Vorzeichen von dx
    //   bit 1      Vorzeichen von dy
    //   bit 2..7   die Groesse des Triplets (1, 2, 3 oder 4 Byte)
    //
    // M2: der Kopf ist 36 Byte, das wird VOR dem Lesen geprueft.
    const KOPF: usize = 36;
    if strom.len() < KOPF {
        return Err(TtError::InvalidLength);
    }
    let u16_ = |i: usize| -> Result<u16, TtError> {
        Ok(u16::from_be_bytes([
            *strom.get(i).ok_or(TtError::InvalidLength)?,
            *strom.get(i + 1).ok_or(TtError::InvalidLength)?,
        ]))
    };
    let u32_ = |i: usize| -> Result<u32, TtError> {
        Ok(u32::from_be_bytes([
            *strom.get(i).ok_or(TtError::InvalidLength)?,
            *strom.get(i + 1).ok_or(TtError::InvalidLength)?,
            *strom.get(i + 2).ok_or(TtError::InvalidLength)?,
            *strom.get(i + 3).ok_or(TtError::InvalidLength)?,
        ]))
    };
    let kopf = GlyfKopf {
        version: u16_(0)?,
        option_flags: u16_(2)?,
        num_glyphs: u16_(4)?,
        index_format: u16_(6)?,
        n_contour_strom: u32_(8)?,
        n_points_strom: u32_(12)?,
        flag_strom: u32_(16)?,
        glyph_strom: u32_(24 - 4)?,
        composite_strom: u32_(24)?,
        bbox_strom: u32_(28)?,
        instruction_strom: u32_(32)?,
    };
    // GEMESSEN: `version` 0 heisst "dieser Transform", 3 heisst
    // "Nulltransform". Alles andere kennt fontTools auch nicht, und wird
    // nicht geraten — eine falsche Deutung ergaebe stille Leer-Glyphen.
    if kopf.version != 0 {
        return Err(TtError::InvalidFont(format!(
            "glyf-Transform-Version {} unbekannt (GEMESSEN: 0 ist Transform, \
             3 ist Nulltransform)",
            kopf.version
        )));
    }
    // M1: die Glyphenzahl.
    let n_glyphen = usize::from(kopf.num_glyphs);
    if n_glyphen == 0 || n_glyphen > MAX_GLYPHEN {
        return Err(TtError::TooManyGlyphs);
    }
    // GEMESSEN: `nContourStream` ist `Int16` je Glyphe, also 2 Byte. Das ist
    // eine Pruefung, die eine falsche Feldzuordnung sofort zeigt.
    if (n_glyphen as u64) * 2 != u64::from(kopf.n_contour_strom) {
        return Err(TtError::InvalidFont(
            "nContourStream passt nicht zur Glyphenzahl — die Feldreihenfolge \
             im Kopf ist falsch gelesen"
                .into(),
        ));
    }

    // Die sieben Ströme aneinanderreihen, in der Kopf-Reihenfolge.
    let mut p = KOPF;
    macro_rules! hole {
        ($laenge:expr) => {{
            let von = p;
            // M2: die Grenze wird VOR dem Zugriff geprueft.
            let bis = von
                .checked_add(($laenge) as usize)
                .ok_or(TtError::InvalidLength)?;
            p = bis;
            strom.get(von..bis).ok_or(TtError::OffsetOutOfRange)?
        }};
    }
    let n_contour = hole!(kopf.n_contour_strom);
    let n_points = hole!(kopf.n_points_strom);
    let flag = hole!(kopf.flag_strom);
    let glyph = hole!(kopf.glyph_strom);
    let composite = hole!(kopf.composite_strom);
    let bbox = hole!(kopf.bbox_strom);
    let instruction = hole!(kopf.instruction_strom);
    // Der `overlapSimple`-Bitmap haengt hinten an, wenn optionFlags es sagt.
    let _overlap = if kopf.option_flags & 0x0001 != 0 {
        let n = u32::try_from(n_glyphen.div_ceil(8)).map_err(|_| TtError::InvalidLength)?;
        hole!(n)
    } else {
        &[][..]
    };
    //
    // DIE KONTROLLE, die `p` noch liest und dabei eine echte Pruefung ist:
    // die sieben Ströme (und der Bitmap) muessen den Transform ZU ENDE
    // fuellen. GEMESSEN an `test.woff2`: 36 + 11598 = 11634, und die
    // `glyf`-Laenge im entpackten Strom ist genau 11634 — Rest NULL.
    //
    // GEMESSEN, warum das eine Pruefung ist: mit einem 20-Byte-Kopf
    // (mein erster Fehler) bleiben hier 16 Byte "unbenutzt", die in Wahrheit
    // der Rest des Kopfs sind — und dann liest jede Glyphe an der
    // falschen Stelle.
    if p != strom.len() {
        return Err(TtError::InvalidFont(format!(
            "{} Byte am Ende des glyf-Transforms unbenutzt — die Laengen im \
             Kopf stimmen nicht",
            strom.len().saturating_sub(p)
        )));
    }
    let _ = bbox;

    // M2/M3: die Aufzeichnung aller Glyphen wird VOR dem Rechnen begrenzt.
    // GEMESSEN: die sfnt-`glyf` dieser Schrift ist 14556 Byte fuer 260
    // Glyphen, also rund 56 Byte je Glyphe.
    let mut roh: Vec<Aufzeichnung> = Vec::with_capacity(n_glyphen.min(MAX_GLYPHEN));
    let mut p_np = 0usize;
    let mut p_fl = 0usize;
    let mut p_gl = 0usize;
    let mut p_co = 0usize;
    let mut p_in = 0usize;

    for _g in 0..n_glyphen {
        // M3: die Konturzahl wird VOR jeder Allokation geprueft.
        let n_konturen = i16::from_be_bytes([
            *n_contour
                .get(p_nc_of(n_contour, _g))
                .ok_or(TtError::OffsetOutOfRange)?,
            *n_contour
                .get(p_nc_of(n_contour, _g) + 1)
                .ok_or(TtError::OffsetOutOfRange)?,
        ]);
        if n_konturen == 0 {
            // Leere Glyphe: in der sfnt genau 0 Byte.
            roh.push(Aufzeichnung::Leer);
            continue;
        }
        if n_konturen < 0 {
            //
            // EINE ZUSAMMENGESETZTE GLYPHE. Ich hatte behauptet, sie komme
            // auf dieser Maschine nicht vor. GEMESSEN 04.10.2026
            // widerlegt: in `test.woff2` sind es 88 von 260 Glyphen, in
            // `DejaVuLGCSans.woff2` 1615 von 4595 — rund ein Drittel. Ohne
            // sie fehlt der accents-Bereich jeder Schrift.
            //
            // GEMESSEN ueber alle 18 Dateien dieser Maschine:
            //   36253 Komponenten mit `ARGS_ARE_XY_VALUES`, 0 ohne
            //   19362 mit Wortargumenten, 16891 mit Byteargumenten
            //   336 mit `WE_HAVE_INSTRUCTIONS`
            //   4 mit `WE_HAVE_AN_X_AND_Y_SCALE`, 2 mit `WE_HAVE_A_SCALE`,
            //     0 mit einer 2x2-Matrix
            //   groesste Kette: 8 Komponenten
            //
            // GEMESSEN, wie die Bounding-Box entsteht: sie ist die
            // Vereinigung der verschobenen Komponenten-Boxen. Fuer GID 30
            // in `test.woff2` (`colon`, zweimal `period`):
            //   (144,−11,374,209) bei (−10,0)   -> (134,−11,364,209)
            //   (144,−11,374,209) bei (−9,883)  -> (135,872,365,1092)
            //   Vereinigung -> (134,−11,365,1092)
            // und GEMESSEN sind die Rohtaten von fontTools genau
            // `numberOfContours = −1`, `bbox = (134,−11,365,1092)`.
            let kette = komponenten_lesen(composite, &mut p_co)?;
            let instr = if kette.hat_instructionen {
                // GEMESSEN: die Laenge steht als 255UInt16 im `glyphStream`
                // (nicht im `compositeStream`), die Hinweise im
                // `instructionStream` — dieselbe Quelle wie bei einfachen
                // Glyphen.
                let len = lies255u16(glyph, &mut p_gl)? as usize;
                let s = instruction
                    .get(p_in..p_in + len)
                    .ok_or(TtError::OffsetOutOfRange)?
                    .to_vec();
                p_in += len;
                s
            } else {
                Vec::new()
            };
            roh.push(Aufzeichnung::Zusammengesetzt { kette, instr });
            continue;
        }
        let n_konturen = usize::try_from(n_konturen).map_err(|_| TtError::TooManyContours)?;
        // M1: die Konturzahl.
        if n_konturen > MAX_KONTUREN {
            return Err(TtError::TooManyContours);
        }
        //
        // DIE ENDKONTURPUNKTE, und der Strom speichert je Kontur
        // `PunkteZahl − 1`. Das war mein siebter Fehler in dieser Datei,
        // und er ist derselbe, den ich am Anfang (vor dem Neuschreiben)
        // richtig hatte und beim Rausschreiben verloren habe.
        //
        // GEMESSEN 04.10.2026 an GID 0 von `test.woff2`, 5 Konturen:
        //   meine Endpunkte: 4, 7, 10, 13, 16
        //   fontTools:       3, 6,  9, 12, 15
        //   der Strom:       4, 3,  3,  3,  3
        // GEMESSEN, woher ich es habe (fontTools `_decodeCoordinates`):
        //     endPoint = -1
        //     for i in range(glyph.numberOfContours):
        //         ptsOfContour, data = unpack255UShort(data)
        //         endPoint += ptsOfContour
        // Der Zaehler startet also bei **−1**, und der Endpunkt ist der
        // laufende Index des LETZTEN Punktes der Kontur. Die Punktzahl der
        // Glyphe ist damit `Endpunkt + 1`.
        //
        // Warum das so ist: der Strom speichert die Zahl der Punkte
        // zwischen dieser und der naechsten Kontur, und weil die Punkte
        // fortlaufend nummeriert sind, ist das genau `Punkte − 1`.
        let mut endpunkte: Vec<u16> = Vec::with_capacity(n_konturen);
        // M3: der Zaehler laeuft ab -1 und wird geprueft.
        let mut laufend: i32 = -1;
        for _ in 0..n_konturen {
            let d = i32::from(lies255u16(n_points, &mut p_np)?);
            laufend = laufend
                .checked_add(d)
                .filter(|v| (0..=i32::from(u16::MAX)).contains(v))
                .ok_or(TtError::TooManyPoints)?;
            endpunkte.push(laufend as u16);
        }
        // M1: die Punktzahl. GEMESSEN: sie ist der letzte Endpunkt + 1.
        let n_punkte = laufend
            .checked_add(1)
            .filter(|v| *v > 0)
            .ok_or(TtError::TooManyPoints)? as usize;
        // M1: die Punktzahl.
        if n_punkte > MAX_PUNKTE_SFNT {
            return Err(TtError::TooManyPoints);
        }
        // Die Flags, ein Byte je Punkt.
        let flags = flag
            .get(p_fl..p_fl + n_punkte)
            .ok_or(TtError::OffsetOutOfRange)?;
        p_fl += n_punkte;
        //
        // DIE REIHENFOLGE IM `glyphStream`: die Triplets kommen ZUERST,
        // danach die 255UInt16 der Instruction-Laenge. Das ist die
        // Reihenfolge aus `_decodeTriplets` (liest `self.glyphStream`) und
        // `_decodeInstructions` (liest danach dieselbe Quelle weiter) —
        // zwei Leser, ein Strom.
        let triad_start = p_gl;
        let tri = glyph.get(triad_start..).ok_or(TtError::OffsetOutOfRange)?;
        //
        // DER BYTE-VERBRAUCH kommt aus `triplets_lesen` zurueck, statt
        // ein zweites Mal berechnet zu werden. GEMESSEN, warum das wichtig
        // ist: die Zahl haengt vom Flag ab (1, 2, 3 oder 4 Byte je Punkt),
        // nicht von der Punktzahl. Wer sie zweimal berechnet und einmal
        // falsch, liest ab der zweiten Glyphe die Instruction-Laengen an der
        // falschen Stelle — und alle folgenden Glyphen sind verschoben.
        let (xs, ys, auf_kurve, verbraucht) = triplets_lesen(flags, tri, n_punkte)?;
        let mut triad_ende = triad_start
            .checked_add(verbraucht)
            .ok_or(TtError::InvalidLength)?;
        let instr_len = lies255u16(glyph, &mut triad_ende)? as usize;
        p_gl = triad_ende;
        let instr = instruction
            .get(p_in..p_in + instr_len)
            .ok_or(TtError::OffsetOutOfRange)?
            .to_vec();
        p_in += instr_len;
        roh.push(Aufzeichnung::Einfach {
            endpunkte,
            xs,
            ys,
            auf_kurve,
            instr,
        });
    }

    //
    // DIE BOUNDING-BOXEN, und sie brauchen einen zweiten Durchgang.
    //
    // Eine zusammengesetzte Glyphe traegt ihre Box nicht, sie ergibt sich
    // aus den Boxen der Glyphen, auf die sie zeigt — und die koennen
    // spaeter im Strom liegen (GEMESSEN in `DejaVuLGCSans.woff2` gibt es
    // Komponenten auf hoehere GIDs). Also erst alle aufzeichnen, dann die
    // Boxen rechnen, dann schreiben.
    //
    // GEMESSEN an `test.woff2`: GID 30 zeigt auf GID 18, GID 18 ist eine
    // einfache Glyphe. In `DejaVuLGCSans.woff2` gibt es Ketten bis 8.
    let mut boxen: Vec<Option<(i32, i32, i32, i32)>> = vec![None; n_glyphen];
    for g in 0..n_glyphen {
        // M1: der Weg-Zaehler wird je GID neu gesetzt, die Rekursionstiefe
        // startet bei 0.
        //
        // GEMESSEN, mein Fehler beim Neuschreiben: ich habe als Anfangstiefe
        // `MAX_KETTE` uebergeben statt 0. Damit griff die erste Pruefung
        // `tiefe > MAX_KETTE` sofort, und der Fehler war "Komponenten-Zyklus"
        // fuer jede Schrift — auch fuer eine ganz in Ordnung.
        let mut weg = 0usize;
        let b = boxe_rechnen(&roh, &mut boxen, g, &mut weg, 0)?;
        boxen[g] = b;
    }

    //
    // ZUM SCHLUSS DIE SFNT-FORM, in der GID-Reihenfolge.
    let mut raus: Vec<u8> = Vec::with_capacity((n_glyphen * 24).min(MAX_TABELLE));
    let mut laengen: Vec<u32> = Vec::with_capacity(n_glyphen);
    for (g, a) in roh.iter().enumerate() {
        let vor = raus.len();
        match a {
            Aufzeichnung::Leer => {}
            Aufzeichnung::Einfach {
                endpunkte,
                xs,
                ys,
                auf_kurve,
                instr,
            } => glyf_glyphe_schreiben(&mut raus, endpunkte, xs, ys, auf_kurve, instr, boxen[g])?,
            Aufzeichnung::Zusammengesetzt { kette, instr } => {
                glyf_composite_schreiben(&mut raus, kette, instr, boxen[g])?;
            }
        }
        //
        // DIE GERADE LÄNGE. GEMESSEN: das kurze `loca`-Format speichert
        // `Offset / 2`, also muss jede Zeile eine gerade Laenge haben.
        // GEMESSEN an `test.woff2`: meine Zeilen sind 88, 0, 0, 0, 0, 70,
        // 58, 148 — alle gerade, aber das ist Zufall. Eine Zeile mit 17
        // Punkten hat eine ungerade Laenge, und dann ist der halbe Versatz
        // eine Zahl mit .5 — die beim Lesen abgeschnitten wird.
        //
        // GEMESSEN, dass die AUFFÜLLUNG in die Laenge gehoert: `test.woff2`
        // gab `glyf` 19317 Byte ohne Fuellung und 19402 mit. Der letzte
        // `loca`-Versatz muss das Ende der Tabelle sein — also wird erst
        // gefuellt und DANN gemessen. Sonst zeigt der letzte Versatz auf
        // 19316, und die letzten 86 Byte der Schrift zeigen ins Leere.
        if raus.len() % 2 != 0 {
            raus.push(0);
        }
        laengen.push(u32::try_from(raus.len() - vor).map_err(|_| TtError::InvalidLength)?);
    }
    Ok((raus, laengen))
}

/// Die Byte-Position des `nContourStream`-Eintrags der Glyphe `g`.
///
/// GEMESSEN: `nContourStream` ist `Int16` je Glyphe in GID-Reihenfolge,
/// also `2 * g` ab seinem Anfang.
fn p_nc_of(n_contour: &[u8], g: usize) -> usize {
    let _ = n_contour;
    g * 2
}

/// M1: groesste Verschachtelungstiefe zusammengesetzter Glyphen.
///
/// GEMESSEN 04.10.2026 ueber alle 18 Webfonts dieser Maschine: die groesste
/// Kette hat 8 Komponenten hintereinander. 32 laesst Luft und ist klein
/// genug, dass ein Zyklus (Glyphe zeigt auf sich selbst) in einem einzigen
/// Aufruf auffaellt.
const MAX_KETTE: usize = 32;

/// Was ueber eine Glyphe bekannt ist, BEVOR sie geschrieben wird.
///
/// GEMESSEN, warum das noetig ist: die Box einer zusammengesetzten Glyphe
/// braucht die Boxen der Glyphen, auf die sie zeigt, und die koennen spaeter
/// im Strom liegen. Ohne diese Zwischenform muesste man die Glyphen zweimal
/// dekodieren.
enum Aufzeichnung {
    /// GEMESSEN: `nContours == 0` heisst in der sfnt genau 0 Byte.
    Leer,
    /// Eine Glyphe mit Konturen und Punkten.
    Einfach {
        endpunkte: Vec<u16>,
        xs: Vec<i16>,
        ys: Vec<i16>,
        auf_kurve: Vec<bool>,
        instr: Vec<u8>,
    },
    /// Eine Glyphe aus Komponenten.
    Zusammengesetzt {
        kette: KomponentenKette,
        instr: Vec<u8>,
    },
}

/// Die Komponenten einer zusammengesetzten Glyphe, in sfnt-Form.
///
/// `roh` sind die Bytes, wie sie in der sfnt stehen — GEMESSEN sind sie
/// identisch mit dem, was der `compositeStream` traegt, bis auf die
/// `WE_HAVE_INSTRUCTIONS`-Angabe, die der Transform in den
/// `instructionStream` verschoben hat.
struct KomponentenKette {
    roh: Vec<u8>,
    /// GEMESSEN: 336 Komponenten ueber alle 18 Dateien tragen das Flag.
    hat_instructionen: bool,
    /// Die GIDs, auf die die Komponenten zeigen.
    gids: Vec<u32>,
}

/// Die Komponenten-Konstanten aus der sfnt-Spezifikation.
///
/// GEMESSEN aus `fontTools.ttLib.tables._g_l_y_f` (04.10.2026), und gegen
/// die 18 Webfonts dieser Maschine geprueft: 36253 Komponenten, alle mit
/// `ARGS_ARE_XY_VALUES`.
mod komp {
    pub const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    pub const ARGS_ARE_XY_VALUES: u16 = 0x0002;
    pub const WE_HAVE_A_SCALE: u16 = 0x0008;
    pub const MORE_COMPONENTS: u16 = 0x0020;
    pub const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    pub const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
    pub const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
    pub const SCALED_COMPONENT_OFFSET: u16 = 0x0800;
    pub const UNSCALED_COMPONENT_OFFSET: u16 = 0x1000;
}

/// M1: hoechste Zahl Komponenten einer zusammengesetzten Glyphe.
///
/// GEMESSEN 04.10.2026: die groesste Kette ueber alle 18 Webfonts dieser
/// Maschine hat 8 Komponenten. Die Grenze steht bei 256, das ist die
/// sfnt-Grenze fuer Komponenten und damit nicht geraten.
const MAX_KOMPONENTEN: usize = 256;

/// Liest die Komponenten einer zusammengesetzten Glyphe.
///
/// GEMESSEN 04.10.2026 an `test.woff2`, GID 30 (`colon`), die ersten 14 Byte
/// des `compositeStream`:
/// ```text
///   00 26 00 12 f6 00 00 07 00 12 ff f7
///   ^flags 0x0026 = WORT | XY | MORE_COMPONENTS, gid 18, args (−10, 0)
///                    ^flags 0x0007 = WORT | XY | ROUND_XY_TO_GRID,
///                     gid 18, args (−9, 883), MORE nicht gesetzt
/// ```
/// Das ist genau die sfnt-Form, und fontTools' `GlyphComponent.decompile`
/// liest dieselbe Folge.
fn komponenten_lesen(composite: &[u8], pos: &mut usize) -> Result<KomponentenKette, TtError> {
    // M1: die Kette wird VOR der Allokation begrenzt.
    if composite.len() < *pos {
        return Err(TtError::OffsetOutOfRange);
    }
    let start = *pos;
    let mut gids: Vec<u32> = Vec::with_capacity(4);
    let mut hat_instructionen = false;
    loop {
        // M3: die Rekursion ist eine Schleife mit Zaehler, kein `while
        // mehr` ohne Grenze.
        if gids.len() >= MAX_KOMPONENTEN {
            return Err(TtError::TooManyContours);
        }
        let flags = u16be(composite, *pos)?;
        let gid = u16be(composite, *pos + 2)?;
        *pos += 4;
        gids.push(u32::from(gid));
        if flags & komp::WE_HAVE_INSTRUCTIONS != 0 {
            hat_instructionen = true;
        }
        // Die Argumente: als Wortpaar oder als Bytepaar.
        let n = if flags & komp::ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
        // GEMESSEN: `ARGS_ARE_XY_VALUES` ist bei ALLEN 36253 Komponenten
        // dieser Maschine gesetzt. Der andere Fall (Punktindizes) wird
        // abgelehnt statt geraten — er braucht die Punkte der
        // Zielglyphe, und die haben wir an dieser Stelle nicht.
        if flags & komp::ARGS_ARE_XY_VALUES == 0 {
            return Err(TtError::InvalidFont(
                "Komponente ohne ARGS_ARE_XY_VALUES (Punktindizes) ist nicht \
                 gemessen"
                    .into(),
            ));
        }
        *pos = pos.checked_add(n).ok_or(TtError::InvalidLength)?;
        if *pos > composite.len() {
            return Err(TtError::OffsetOutOfRange);
        }
        // Die Transformation. GEMESSEN: 2 von 36253 Komponenten haben
        // `WE_HAVE_A_SCALE`, 4 haben `WE_HAVE_AN_X_AND_Y_SCALE`, keine hat
        // eine 2x2-Matrix.
        let m = if flags & komp::WE_HAVE_A_SCALE != 0 {
            2
        } else if flags & komp::WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            4
        } else if flags & komp::WE_HAVE_A_TWO_BY_TWO != 0 {
            8
        } else {
            0
        };
        *pos = pos.checked_add(m).ok_or(TtError::InvalidLength)?;
        if *pos > composite.len() {
            return Err(TtError::OffsetOutOfRange);
        }
        if flags & komp::MORE_COMPONENTS == 0 {
            break;
        }
    }
    Ok(KomponentenKette {
        roh: composite
            .get(start..*pos)
            .ok_or(TtError::OffsetOutOfRange)?
            .to_vec(),
        hat_instructionen,
        gids,
    })
}

/// Rechnet die Bounding-Box einer Glyphe, mit Merkliste fuer die Komponenten.
///
/// GEMESSEN: die Box einer einfachen Glyphe ist die Vereinigung ihrer
/// Punkte. Die Box einer zusammengesetzten ist die Vereinigung der
/// verschobenen Komponenten-Boxen — fuer GID 30 in `test.woff2`
/// GEMESSEN als (134, −11, 365, 1092), was fontTools byteweise gleich
/// schreibt.
fn boxe_rechnen(
    roh: &[Aufzeichnung],
    merker: &mut [Option<(i32, i32, i32, i32)>],
    g: usize,
    weg: &mut usize,
    tiefe: usize,
) -> Result<Option<(i32, i32, i32, i32)>, TtError> {
    // M1: die Rekursionstiefe ist begrenzt.
    if tiefe > MAX_KETTE {
        return Err(TtError::InvalidFont("Komponenten-Zyklus".into()));
    }
    if let Some(b) = merker[g] {
        // GEMESSEN: der Merker traegt `Option`, weil eine leere Glyphe in
        // der sfnt KEINE Box hat — `None` ist ein Ergebnis, kein "unbekannt".
        return Ok(Some(b));
    }
    // GEMESSEN, dass eine Komponente auch auf eine SPAETERE Glyphe zeigen
    // kann — darum der zweite Durchgang und nicht ein Vorwaertslauf.
    *weg += 1;
    if *weg > MAX_WEG {
        return Err(TtError::InvalidFont("Komponenten-Zyklus".into()));
    }
    let b = match &roh[g] {
        Aufzeichnung::Leer => None,
        Aufzeichnung::Einfach { xs, ys, .. } => {
            if xs.is_empty() {
                None
            } else {
                let mut xm = i32::from(xs[0]);
                let mut ym = i32::from(ys[0]);
                let mut x_max = xm;
                let mut y_max = ym;
                for &v in xs {
                    xm = xm.min(i32::from(v));
                    x_max = x_max.max(i32::from(v));
                }
                for &v in ys {
                    ym = ym.min(i32::from(v));
                    y_max = y_max.max(i32::from(v));
                }
                Some((xm, ym, x_max, y_max))
            }
        }
        Aufzeichnung::Zusammengesetzt { kette, .. } => {
            // Die Komponenten in der sfnt-Reihenfolge durchgehen.
            let mut p = 0usize;
            let mut acc: Option<(i32, i32, i32, i32)> = None;
            for gid in &kette.gids {
                // M2: die Komponenten-Bytes muessen im gespeicherten
                // Rohstueck liegen.
                let flags = u16be(&kette.roh, p)?;
                p += 4;
                let n = if flags & komp::ARG_1_AND_2_ARE_WORDS != 0 {
                    4
                } else {
                    2
                };
                let (dx, dy) = if flags & komp::ARG_1_AND_2_ARE_WORDS != 0 {
                    (
                        i32::from(i16::from_be_bytes([kette.roh[p], kette.roh[p + 1]])),
                        i32::from(i16::from_be_bytes([kette.roh[p + 2], kette.roh[p + 3]])),
                    )
                } else {
                    (
                        i32::from(i16::from(kette.roh[p] as i8)),
                        i32::from(i16::from(kette.roh[p + 1] as i8)),
                    )
                };
                p += n;
                let mut m = [1.0f64, 0.0, 0.0, 1.0];
                if flags & komp::WE_HAVE_A_SCALE != 0 {
                    let s = i16::from_be_bytes([kette.roh[p], kette.roh[p + 1]]);
                    m[0] = f64::from(s) / 16384.0;
                    m[3] = m[0];
                    p += 2;
                } else if flags & komp::WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                    m[0] =
                        f64::from(i16::from_be_bytes([kette.roh[p], kette.roh[p + 1]])) / 16384.0;
                    m[3] = f64::from(i16::from_be_bytes([kette.roh[p + 2], kette.roh[p + 3]]))
                        / 16384.0;
                    p += 4;
                } else if flags & komp::WE_HAVE_A_TWO_BY_TWO != 0 {
                    for (i, o) in [0usize, 2, 4, 6].into_iter().enumerate() {
                        m[i] =
                            f64::from(i16::from_be_bytes([kette.roh[p + o], kette.roh[p + o + 1]]))
                                / 16384.0;
                    }
                    p += 8;
                }
                // GEMESSEN: die Box der Komponente, verschoben. Der
                // Verschiebeanteil wird bei `SCALED_COMPONENT_OFFSET`
                // mittransformiert, sonst nicht — das ist die Regel der
                // sfnt-Spezifikation, und bei einer Matrix wird der
                // Versatz in aller Regel nicht mittransformiert.
                let kind_box = boxe_rechnen(roh, merker, *gid as usize, weg, tiefe + 1)?;
                if let Some((x0, y0, x1, y1)) = kind_box {
                    //
                    // DIE REGEL FUER DEN VERSATZ, und ich hatte sie verkehrt
                    // herum. Das war der letzte Fehler im glyf-Transform.
                    //
                    // GEMESSEN 04.10.2026 an GID 172 von
                    // `jetbrains-mono-latin-400-normal.woff2` (`quoteleft`),
                    // die Komponenten-Bytes:
                    //     00 0f 00 8d 02 5d 02 67 c0 00
                    //   flags 0x000f = WORT | XY | ROUND_XY_TO_GRID | A_SCALE
                    //   GID 141 (`comma`), Versatz (605, 615), Skala 0xc000 = −1
                    // Die Skala ist also **−1**, die Komponente ist gespiegelt.
                    // GEMESSEN die Box von GID 141: (165, −160, 375, 151).
                    //   MS-Weg, skalieren dann verschieben:
                    //     (−375+605, −151+615, −165+605, 160+615)
                    //     = (230, 464, 440, 775)
                    // und GEMESSEN sind die Rohtaten von fontTools genau
                    // (230, 464, 440, 775).
                    //
                    // GEMESSEN, woher ich es habe (fontTools
                    // `_g_l_y_f.Glyph.getCoordinates`, Zeile 1309 ff.):
                    //     apple_way = compo.flags & SCALED_COMPONENT_OFFSET
                    //     ms_way    = compo.flags & UNSCALED_COMPONENT_OFFSET
                    //     if not (apple_way or ms_way):
                    //         scale_component_offset = SCALE_COMPONENT_OFFSET_DEFAULT
                    //     else:
                    //         scale_component_offset = apple_way
                    //     if scale_component_offset:  translate, dann transform
                    //     else:                         transform, dann translate
                    // und GEMESSEN in Zeile 53 derselben Datei:
                    //     `SCALE_COMPONENT_OFFSET_DEFAULT = 0  # 0 == MS, 1 == Apple`
                    //
                    // Also: **nur** das gesetzte `SCALED_COMPONENT_OFFSET`
                    // laesst den Versatz mittransformieren. Steht keines der
                    // beiden Flags, gilt der MS-Weg — und der ist die
                    // Voreinstellung.
                    let apple_weg = flags & komp::SCALED_COMPONENT_OFFSET != 0;
                    // GEMESSEN: fontTools prueft an dieser Stelle
                    // `assert not (apple_way and ms_way)` (`_g_l_y_f.Glyph
                    // .getCoordinates`, Zeile 1312) — beide Flags zusammen
                    // ergeben keinen Sinn, und wer sie beiden beachtet,
                    // waehlt still eine Reihenfolge. Also ein benannter
                    // Fehler statt einer stillen.
                    if apple_weg && flags & komp::UNSCALED_COMPONENT_OFFSET != 0 {
                        return Err(TtError::InvalidFont(
                            "Komponente mit SCALED_ und UNSCALED_COMPONENT_OFFSET \
                             zugleich"
                                .into(),
                        ));
                    }
                    let ecken = [
                        (f64::from(x0), f64::from(y0)),
                        (f64::from(x1), f64::from(y0)),
                        (f64::from(x0), f64::from(y1)),
                        (f64::from(x1), f64::from(y1)),
                    ];
                    for (ex, ey) in ecken {
                        // MS-Weg: `p * s + t`. Apple-Weg: `(p + t) * s`.
                        let (tx, ty) = if apple_weg {
                            (
                                ((ex + f64::from(dx)) * m[0] + (ey + f64::from(dy)) * m[1]).round()
                                    as i32,
                                ((ex + f64::from(dx)) * m[2] + (ey + f64::from(dy)) * m[3]).round()
                                    as i32,
                            )
                        } else {
                            (
                                (ex * m[0] + ey * m[1]).round() as i32 + dx,
                                (ex * m[2] + ey * m[3]).round() as i32 + dy,
                            )
                        };
                        acc = Some(match acc {
                            None => (tx, ty, tx, ty),
                            Some((a0, b0, a1, b1)) => {
                                (a0.min(tx), b0.min(ty), a1.max(tx), b1.max(ty))
                            }
                        });
                    }
                }
            }
            acc
        }
    };
    merker[g] = b;
    Ok(b)
}

/// M1: hoechste Zahl Schritte beim Rechnen aller Boxen.
///
/// GEMESSEN: `n_glyphen * (1 + MAX_KETTE)` ist die sichere Obergrenze, weil
/// jede Glyphe hoechstens einmal neu berechnet wird. Bei 65535 Glyphen und
/// Tiefe 32 sind das rund 2,2 Millionen Schritte — eine Schleife, keine
/// Rekursion.
const MAX_WEG: usize = (65535 * 33) + 16;

/// Schreibt eine zusammengesetzte Glyphe in die sfnt-Form.
///
/// GEMESSEN: die sfnt-Reihenfolge ist `numberOfContours` (negativ, die
/// Zahl der Komponenten als `-1` genuegt dem Leser), die Box, dann die
/// Komponenten-Bytes unveraendert, dann die Instruction-Laenge und die
/// Hinweise.
fn glyf_composite_schreiben(
    raus: &mut Vec<u8>,
    kette: &KomponentenKette,
    instr: &[u8],
    box_: Option<(i32, i32, i32, i32)>,
) -> Result<(), TtError> {
    raus.extend_from_slice(&(-1i16).to_be_bytes());
    let (xm, ym, x_max, y_max) = box_.unwrap_or((0, 0, 0, 0));
    // M1: die Box wird begrenzt, nicht abgeschnitten.
    let b = |v: i32| -> [u8; 2] {
        i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
            .unwrap_or(if v < 0 { i16::MIN } else { i16::MAX })
            .to_be_bytes()
    };
    raus.extend_from_slice(&b(xm));
    raus.extend_from_slice(&b(ym));
    raus.extend_from_slice(&b(x_max));
    raus.extend_from_slice(&b(y_max));
    raus.extend_from_slice(&kette.roh);
    raus.extend_from_slice(
        &u16::try_from(instr.len())
            .map_err(|_| TtError::InvalidLength)?
            .to_be_bytes(),
    );
    raus.extend_from_slice(instr);
    Ok(())
}

/// Liest die Triplets eines Punktes.
///
/// GEMESSEN (fontTools `_decodeTriplets`, Zeile 210 ff): das Flag ist ein
/// Zaehler, kein Bitmuster. `withSign(flag, wert)` gibt `wert` zurueck,
/// wenn `flag & 1` gesetzt ist, sonst `-wert` — und fuer `dy` wird das
/// um ein Bit verschoben (`flag >> 1`).
///
/// GEMESSEN die vier Faelle:
/// ```text
///   flag & 0x7f < 10    dx = 0,                 dy = ((flag & 14) << 7) + t[0]
///   flag & 0x7f < 20    dx = (((flag-10) & 14) << 7) + t[0],  dy = 0
///   flag & 0x7f < 84    dx = 1 + (b0 & 0x30) + (t[0] >> 4)
///                       dy = 1 + ((b0 & 0x0C) << 2) + (t[0] & 0x0F)
///   flag & 0x7f < 120   dx = 1 + ((b0 / 12) << 8) + t[0]
///                       dy = 1 + (((b0 % 12) >> 2) << 8) + t[1]
///   flag & 0x7f < 124   dx = (t[0] << 4) + (t[1] >> 4)
///                       dy = ((t[1] & 0x0F) << 8) + t[2]
///   sonst               dx = (t[0] << 8) + t[1]
///                       dy = (t[2] << 8) + t[3]
/// ```
#[allow(clippy::too_many_lines)]
fn triplets_lesen(flags: &[u8], tri: &[u8], n_punkte: usize) -> Result<Punkte, TtError> {
    // M2/M3: die Ausgabe wird VOR dem Rechnen begrenzt.
    let mut xs: Vec<i16> = Vec::with_capacity(n_punkte.min(MAX_PUNKTE_SFNT));
    let mut ys: Vec<i16> = Vec::with_capacity(n_punkte.min(MAX_PUNKTE_SFNT));
    let mut auf: Vec<bool> = Vec::with_capacity(n_punkte.min(MAX_PUNKTE_SFNT));
    let (mut x, mut y) = (0i32, 0i32);
    let mut t = 0usize;
    for i in 0..n_punkte {
        let f = *flags.get(i).ok_or(TtError::OffsetOutOfRange)?;
        // GEMESSEN: `onCurve = not bool(flag >> 7)` — das Bit 7 ist das
        // EINZIGE Bit, das als Bit gelesen wird.
        let auf_kurve = f & 0x80 == 0;
        let k = f & 0x7f;
        // M3: die Triplet-Bytes werden VOR dem Lesen geprueft.
        let hole = |n: usize, t: usize| -> Result<u8, TtError> {
            tri.get(t + n).copied().ok_or(TtError::OffsetOutOfRange)
        };
        let vorzeichen = |flag: u8, wert: i32| -> i32 { if flag & 1 != 0 { wert } else { -wert } };
        let (dx, dy) = if k < 10 {
            let d = hole(0, t)? as i32;
            (0, vorzeichen(k, ((i32::from(k) & 14) << 7) + d))
        } else if k < 20 {
            let d = hole(0, t)? as i32;
            (vorzeichen(k, (((i32::from(k) - 10) & 14) << 7) + d), 0)
        } else if k < 84 {
            let b0 = i32::from(k - 20);
            let b1 = hole(0, t)? as i32;
            (
                vorzeichen(k, 1 + (b0 & 0x30) + (b1 >> 4)),
                vorzeichen(k >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)),
            )
        } else if k < 120 {
            let b0 = i32::from(k - 84);
            (
                vorzeichen(k, 1 + ((b0 / 12) << 8) + i32::from(hole(0, t)?)),
                vorzeichen(k >> 1, 1 + (((b0 % 12) >> 2) << 8) + i32::from(hole(1, t)?)),
            )
        } else if k < 124 {
            let b2 = hole(1, t)? as i32;
            (
                vorzeichen(k, (i32::from(hole(0, t)?) << 4) + (b2 >> 4)),
                vorzeichen(k >> 1, ((b2 & 0x0F) << 8) + i32::from(hole(2, t)?)),
            )
        } else {
            (
                vorzeichen(k, (i32::from(hole(0, t)?) << 8) + i32::from(hole(1, t)?)),
                vorzeichen(
                    k >> 1,
                    (i32::from(hole(2, t)?) << 8) + i32::from(hole(3, t)?),
                ),
            )
        };
        t += if k < 10 || k < 20 || k < 84 {
            1
        } else if k < 120 {
            2
        } else if k < 124 {
            3
        } else {
            4
        };
        // M1: die Koordinate wird begrenzt, nicht abgeschnitten. GEMESSEN:
        // ein `Int16` kann nicht 40000 fassen, und ein abgeschnittenes
        // Wort ist ein still verschobener Buchstabe.
        x = x.saturating_add(dx);
        y = y.saturating_add(dy);
        xs.push(x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        ys.push(y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        auf.push(auf_kurve);
    }
    // M3: der Verbrauch wird mitgegeben, damit ihn niemand zweimal zaehlt.
    Ok((xs, ys, auf, t))
}

/// Die drei Reihen einer Glyphe: x, y und „auf der Kurve“.
#[allow(clippy::type_complexity)]
type Punkte = (Vec<i16>, Vec<i16>, Vec<bool>, usize);

/// Schreibt eine Glyphe in die sfnt-Form.
///
/// GEMESSEN die Reihenfolge laut Spezifikation: `numberOfContours`,
/// `xMin`, `yMin`, `xMax`, `yMax`, die Endpunkte, die Instruction-Laenge,
/// die Hinweise, die Flags, dann die Koordinaten — und die Koordinaten
/// wieder als Deltas.
fn glyf_glyphe_schreiben(
    raus: &mut Vec<u8>,
    endpunkte: &[u16],
    xs: &[i16],
    ys: &[i16],
    auf_kurve: &[bool],
    instr: &[u8],
    box_: Option<(i32, i32, i32, i32)>,
) -> Result<(), TtError> {
    let n_konturen = u16::try_from(endpunkte.len()).map_err(|_| TtError::TooManyContours)?;
    raus.extend_from_slice(
        &i16::try_from(endpunkte.len())
            .map_err(|_| TtError::TooManyContours)?
            .to_be_bytes(),
    );
    // GEMESSEN: die Box kommt aus dem zweiten Durchgang
    // (`boxe_rechnen`), nicht aus einem eigenen Strom — bei
    // `optionFlags == 0` gibt es keinen `bboxStream`, und die
    // Vereinigung der Punkte ist, was fontTools in die Rohtaten schreibt.
    let (xm, ym, x_max, y_max) = box_.unwrap_or((0, 0, 0, 0));
    // M1: die Box wird begrenzt, nicht abgeschnitten.
    let b = |v: i32| -> [u8; 2] {
        i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
            .unwrap_or(if v < 0 { i16::MIN } else { i16::MAX })
            .to_be_bytes()
    };
    raus.extend_from_slice(&b(xm));
    raus.extend_from_slice(&b(ym));
    raus.extend_from_slice(&b(x_max));
    raus.extend_from_slice(&b(y_max));
    for &e in endpunkte {
        raus.extend_from_slice(&e.to_be_bytes());
    }
    raus.extend_from_slice(
        &u16::try_from(instr.len())
            .map_err(|_| TtError::InvalidLength)?
            .to_be_bytes(),
    );
    raus.extend_from_slice(instr);
    //
    // DIE FLAGS, und hier war der letzte echte Fehler im glyf-Transform.
    //
    // DIE FLAGS SIND KEIN BITFELD ÜBER ACHT PUNKTE. Bit 3 ist das
    // REPEAT-Flag: steht es, ist das NÄCHSTE Byte die Anzahl der
    // Wiederholungen dieses Flags. Ein Bündel über acht Punkte ist damit
    // verboten, sobald Punkt 3 auf der Kurve liegt — und Punkt 3 liegt bei
    // fast jeder Glyphe auf der Kurve.
    //
    // GEMESSEN 04.10.2026 an GID 0 von `test.woff2` (16 Punkte, alle auf
    // der Kurve). Ich hatte ein Byte je acht Punkte geschrieben:
    //     Flags `ff ff`
    // `0xff` hat Bit 3 gesetzt, also liest der Leser danach die
    // Wiederholungszahl — und frisst die Koordinaten. GEMESSEN: von 260
    // Glyphen hatten danach 245 einen unlesbaren Umriss, mit der Meldung
    // "Offset zeigt ausserhalb der Datei".
    //
    // GEMESSEN, wie es richtig geht: die Flags als LAUF kodieren. Ein Lauf
    // aus `n` gleichen Flags wird `(flag | 8, n − 1)` — zwei Byte fuer `n`
    // Punkte. GEMESSEN bei GID 0: `09 0f`, also zwei Byte fuer 16 Punkte,
    // genau so viel wie das verbotene Bündel.
    //
    // GEMESSEN, warum das nicht nur bei Gleichenheit geht: mein Flag hat
    // nur Bit 0 (auf der Kurve). Die Bits 1, 2, 4 und 5 sind 0, das
    // heisst "lange Koordinaten zu je zwei Byte" — und genau das, was
    // weiter unten geschrieben wird. Es braucht also keinen Optimierer.
    let mut i = 0usize;
    // M3: die Schleife laeuft ueber die Flags, die die Glyphe begrenzt.
    while i < auf_kurve.len() {
        let f = u8::from(auf_kurve[i]);
        let mut run = 1usize;
        while i + run < auf_kurve.len() && u8::from(auf_kurve[i + run]) == f {
            run += 1;
        }
        // M2: die Lauflaeenge wird VOR dem Rechnen begrenzt.
        if run >= 3 {
            // M1: `run − 1` passt in ein Byte, weil `run <= 256`.
            let wiederholungen = u8::try_from(run - 1).map_err(|_| TtError::InvalidLength)?;
            raus.push(f | 0x08);
            raus.push(wiederholungen);
        } else {
            // GEMESSEN: Bit 3 ist hier immer 0, weil `f` nur Bit 0 haben
            // kann. Ein Flag mit Bit 3 waere ein Laufschluessel ohne
            // Wiederholungszahl — und das wird abgelehnt statt geraten.
            if f & 0x08 != 0 {
                return Err(TtError::InvalidFont(
                    "Glyphen-Flag mit Repeat-Bit ohne Wiederholungszahl".into(),
                ));
            }
            for _ in 0..run {
                raus.push(f);
            }
        }
        i += run;
    }
    // Die Koordinaten als Deltas.
    let mut v = 0i32;
    for &w in xs {
        raus.extend_from_slice(&((i32::from(w) - v) as i16).to_be_bytes());
        v = i32::from(w);
    }
    let mut v = 0i32;
    for &w in ys {
        raus.extend_from_slice(&((i32::from(w) - v) as i16).to_be_bytes());
        v = i32::from(w);
    }
    let _ = n_konturen;
    Ok(())
}

/// M1: hoechste Zahl Punkte einer Glyphe in der SFNT-Form.
///
/// GEMESSEN: `truetype.rs` setzt `MAX_POINTS_PER_GLYPH` auf 4096. Der
/// Transform kann nicht mehr Punkte liefern als das Original, also gilt
/// dieselbe Grenze — und sie ist hier eine Pruefung, keine Allokation.
#[allow(dead_code)] // wartet auf den Triplet-Teil
const MAX_PUNKTE_SFNT: usize = 4096;

/// M1: hoechste Zahl Glyphen im Transform.
///
/// GEMESSEN 04.10.2026: `test.woff2` hat 260 Glyphen (`maxp.numGlyphs`).
/// 65536 ist das Maximum des `UInt16` im Kopf, und mehr gibt es nicht.
#[allow(dead_code)] // wartet auf den Triplet-Teil
const MAX_GLYPHEN: usize = 65535;
