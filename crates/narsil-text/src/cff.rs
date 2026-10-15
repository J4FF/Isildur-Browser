//! CFF-Leser: Top-DICT, Private-DICT und ein Charstring-Interpreter.
//!
//! # WAS GEMESSEN IST (03.10.2026, A30/A31)
//!
//! Das Modul liest die `CFF `-Tabelle einer OpenType-Schrift. GEMESSEN gibt es
//! auf dieser Maschine **36** solche Tabellen: 35 CFF1 ohne CID und 1 CFF2
//! (`Cantarell-VF.otf`).
//!
//! # DIE REIHENFOLGE IM TABELLENANFANG
//!
//! Der `CFF `-Header dieser Dateien ist **vier Byte** (major, minor, hdrSize,
//! offSize) — es gibt **keinen** OffSize-Satz, `hdrSize` ist 4. GEMESSEN folgt
//! die Reihenfolge wortwoertlich fontTools' `CFFFontSet.decompile`
//! (`cffLib/__init__.py`, Zeile 27-29):
//!
//! ```text
//! file.seek(self.hdrSize)
//! self.fontNames = list(tostr(s) for s in Index(file))   // Name-Index
//! self.topDictIndex = TopDictIndex(file)                 // Top-DICT
//! ```
//!
//! GEMESSEN am echten `NimbusSans-Bold.otf`: der Name-Index liegt bei 4 und
//! endet bei 24, der Top-DICT laeuft von 29 bis 79. Der erste Entwurf las den
//! Top-DICT bei `hdrSize` und bekam **0 von 35** Dateien richtig — dort standen
//! die Schriftnamen.
//!
//! Das Orakel ist `/home/jaff/mutation_md/bild/cff_mess.py`; es meldet
//! „35 von 35 CFF1-Dateien richtig gelesen" und „35 von 35 Private-DICTs
//! richtig".

use std::fmt;
use std::sync::Arc;

/// Das Tag der CFF1-Tabelle (`CFF `, 0x43464620).
pub const TAG_CFF: u32 = 0x4346_4620;

/// Hoechste Zahl Subr-Aufrufe ineinander (M3/M5).
///
/// GEMESSEN: die tiefste Kette in den 35 Dateien dieser Maschine liegt bei 3.
/// Der Wert 10 ist die Grenze der Spezifikation fuer verschachtelte
/// `callsubr`-Ketten; `MAX_SUBR_TIEFE` ist **keine** Rekursion, sondern die
/// Laenge eines `Vec` (M5).
pub const MAX_SUBR_TIEFE: usize = 10;

/// Hoechste Zahl Punkte in **einer** Glyphe (M3/M9).
///
/// GEMESSEN: die groesste Glyphe der 36 Dateien hat 178 Punkte (das `O` von
/// `NimbusSans-Bold.otf`), der Wert 2000 liegt weit darueber.
pub const MAX_PUNKTE: usize = 2000;

/// Hoechste Zahl Items in einem `INDEX` (M3/M9).
///
/// GEMESSEN: der groesste `INDEX` dieser Maschine hat 1197 Items (der
/// CharStrings-Index von `NimbusSans-Bold.otf` hat 855). Das `count`-Feld
/// ist `u16`, die Grenze 65536 kann also nie ueberschritten werden — sie
/// steht trotzdem da, damit die Zusage auch bei einem kaputten `offSize`
/// haelt.
pub const MAX_INDEX_ITEMS: usize = 65536;

/// Alles, was beim CFF-Lesen schiefgehen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CffFehler {
    /// Ein Schnitt liegt ausserhalb der Tabelle.
    LaengeUeberlauf(&'static str),
    /// Ein Byte steht dort, wo keine Zahl stehen kann.
    FalscheZahl(String),
    /// Ein `INDEX` zeigt aus der Tabelle heraus.
    IndexAusserhalb(&'static str),
    /// Ein Operator, den dieses Modul nicht kennt.
    UnbekannterOperator(u32),
    /// Der Operandenstapel ist voll (M3).
    StapelVoll,
    /// Zu viele Punkte in einer Glyphe (M9).
    ZuVielePunkte,
    /// Die Glyphe gibt es nicht.
    GlypheUnbekannt(u16),
    /// Die `callsubr`-Kette ist zu tief (M5).
    SubrZuTief,
}

impl fmt::Display for CffFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CffFehler::LaengeUeberlauf(w) => write!(f, "LaengeUeberlauf: {w}"),
            CffFehler::FalscheZahl(t) => write!(f, "FalscheZahl: {t}"),
            CffFehler::IndexAusserhalb(w) => write!(f, "IndexAusserhalb: {w}"),
            CffFehler::UnbekannterOperator(b) => write!(f, "unbekannter Operator {b}"),
            CffFehler::StapelVoll => write!(f, "StapelVoll"),
            CffFehler::ZuVielePunkte => write!(f, "zu viele Punkte"),
            CffFehler::GlypheUnbekannt(g) => write!(f, "Glyphe {g} gibt es nicht"),
            CffFehler::SubrZuTief => write!(f, "Subr-Kette zu tief"),
        }
    }
}

impl std::error::Error for CffFehler {}

/// Liest `n` Byte gross-endian als Zahl.
///
/// M3: `n` kommt aus dem `offSize`, der in [`Index::lies`] auf 1..=4 geprueft
/// wird. M6: das Ende wird **vor** dem Zuschlag geprueft.
fn lese_breit(daten: &[u8], ab: usize, n: usize, wofuer: &'static str) -> Result<usize, CffFehler> {
    let ende = ab
        .checked_add(n)
        .ok_or(CffFehler::LaengeUeberlauf(wofuer))?;
    if ende > daten.len() {
        return Err(CffFehler::LaengeUeberlauf(wofuer));
    }
    let mut w = 0usize;
    for k in 0..n {
        w = (w << 8) | daten[ab + k] as usize;
    }
    Ok(w)
}

/// Eine Zahl aus einem CFF-DICT **und** die Laenge, die sie belegt.
///
/// # DIE ENCODIERUNG, GEMESSEN
///
/// GEMESSEN an fontTools' `cffDictOperandEncoding` (mit
/// `print([p.cffDictOperandEncoding[k].__name__ for k in range(32)])`):
///
/// ```text
/// 0..=27 und 31 -> read_operator
/// 28 -> read_shortInt   (2 following)
/// 29 -> read_longInt    (4 following)
/// 30 -> read_realNumber (Nibbel, bis 0xF)
/// 32..=246 -> read_byte      (b - 139)
/// 247..=250 -> read_smallInt1
/// 251..=254 -> read_smallInt2
/// 255 -> read_reserved
/// ```
///
/// # WARUM DIE LAENGE IN DER RUECKGABE STEHT
///
/// GEMESSEN an fontTools' `DictDecompiler.decompile` (Zeile 1384-1390):
///
/// ```text
/// handler = self.operandEncoding[b0]
/// value, index = handler(self, b0, data, index)
/// ```
///
/// Der Handler liefert Wert **und** neuen Index. Das ist noetig, weil `30`
/// (`read_realNumber`) seine Laenge **nicht** im Byte stehen hat: sie haengt
/// vom Inhalt ab. Die erste Fassung hatte `zahl()` und `zahl_laenge()` als
/// zwei Funktionen — und `zahl_laenge(30)` war fest 3.
///
/// GEMESSEN, warum das falsch war: im Top-DICT von
/// `NimbusSansNarrow-BoldOblique.otf` steht an Position 20 der Praefix `1e`
/// gefolgt von `e9 a9 ff`, das ist die Zahl **-9.9** (`ItalicAngle`). Mit der
/// festen Laenge 3 blieb der Leser auf dem `ff` stehen, und
/// `examples/cff_kontur.rs` meldete fuer 2 der 36 Dateien
/// „255 ist im DICT reserviert".
///
/// M3: `daten` zeigt auf das **Praefix**, und die Rueckgabe ist die
/// Gesamtlänge (Praefix plus following). Sie ist begrenzt: 5 Byte bei den
/// Zahlen, [`MAX_REAL_ZEICHEN`] / 2 + 1 beim Real.
fn zahl_mit_laenge(code: u8, daten: &[u8]) -> Result<(f32, usize), CffFehler> {
    match code {
        32..=246 => Ok((code as f32 - 139.0, 1)),
        247..=250 => {
            let d = daten
                .get(1)
                .ok_or(CffFehler::FalscheZahl("247..250 ohne zweites Byte".into()))?;
            Ok(((code as f32 - 247.0) * 256.0 + *d as f32 + 108.0, 2))
        }
        251..=254 => {
            let d = daten
                .get(1)
                .ok_or(CffFehler::FalscheZahl("251..254 ohne zweites Byte".into()))?;
            Ok((-(code as f32 - 251.0) * 256.0 - *d as f32 - 108.0, 2))
        }
        28 => {
            let b = daten
                .get(1..3)
                .ok_or(CffFehler::FalscheZahl("28 ohne zwei Bytes".into()))?;
            Ok((i16::from_be_bytes([b[0], b[1]]) as f32, 3))
        }
        29 => {
            let b = daten
                .get(1..5)
                .ok_or(CffFehler::FalscheZahl("29 ohne vier Bytes".into()))?;
            Ok((i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32, 5))
        }
        30 => {
            // GEMESSEN an `read_realNumber` (psCharStrings.py, Zeile 68-81):
            //
            //     while True:
            //         b = byteord(data[index]); index += 1
            //         nibble0 = (b & 0xF0) >> 4
            //         nibble1 = b & 0x0F
            //         if nibble0 == 0xF: break
            //         number = number + realNibbles[nibble0]
            //         if nibble1 == 0xF: break
            //         number = number + realNibbles[nibble1]
            //
            // Also: **Nibbel**, und ein Nibble `0xF` beendet die Zahl. Die
            // Nibbel 0..9 sind Ziffern, 0xA ist der Dezimalpunkt, 0xB und
            // 0xC der Exponent (mit und ohne Vorzeichen), 0xE das
            // Vorzeichen.
            //
            // M3/M4: die Zeichen kommen in ein **festes** Array von
            // [`MAX_REAL_ZEICHEN`], nicht in eine `String` aus
            // Eingangsdaten. `str::parse` alloziert nicht.
            let mut puffer = [0u8; MAX_REAL_ZEICHEN];
            let mut n = 0usize;
            let mut k = 1usize;
            'nibbel: loop {
                let b = *daten
                    .get(k)
                    .ok_or(CffFehler::FalscheZahl("30 ohne Endnibble".into()))?;
                k += 1;
                for hoh in [b >> 4, b & 0x0F] {
                    if hoh == 0xF {
                        break 'nibbel;
                    }
                    let z = if hoh <= 9 {
                        b'0' + hoh
                    } else if hoh == 0xA {
                        b'.'
                    } else if hoh == 0xB || hoh == 0xC || hoh == 0xE {
                        if n == 0 || (n == 1 && puffer[0] == b'.') {
                            b'-'
                        } else {
                            b'e'
                        }
                    } else {
                        // 0xD ist reserviert (fontTools: `None`).
                        return Err(CffFehler::FalscheZahl(format!("30 mit Nibble {hoh:#x}")));
                    };
                    if n >= MAX_REAL_ZEICHEN {
                        // M3: die Grenze. Ohne sie waere ein `0x1E` ohne
                        // Endnibble ein endloser Lauf ueber die Tabelle.
                        return Err(CffFehler::FalscheZahl(
                            "30 laenger als MAX_REAL_ZEICHEN".into(),
                        ));
                    }
                    puffer[n] = z;
                    n += 1;
                }
            }
            if n == 0 {
                return Err(CffFehler::FalscheZahl("30 ohne Zeichen".into()));
            }
            let text = std::str::from_utf8(&puffer[..n])
                .map_err(|_| CffFehler::FalscheZahl("30 liess kein gueltiges Zeichen".into()))?;
            let w: f32 = text
                .parse()
                .map_err(|_| CffFehler::FalscheZahl(format!("30 ist keine Zahl: {text}")))?;
            Ok((w, k))
        }
        255 => {
            // GEMESSEN: im **DICT** ist 255 `read_reserved`. Das ist aber
            // **kein** Fehler: fontTools' `read_reserved` (Zeile 63-65) ist
            //
            //     assert NotImplementedError
            //     return NotImplemented, index
            //
            // und `assert NotImplementedError` besteht immer — die Bedingung
            // ist ein Klassenobjekt und damit truthy. Zurueck kommt also
            // `NotImplemented` **ohne** ein following Byte, und
            // `decompile` schiebt es auf den Stack (`if value is not None`).
            //
            // GEMESSEN, warum das bei uns ein Fehler war: der 2-Byte-Real
            // `ItalicAngle` endete auf dem `ff`, und `lies_dict` meldete
            // „255 ist im DICT reserviert" fuer 2 der 36 Dateien. Wir
            // verbrauchen das Byte und legen nichts auf den Stapel — der
            // naechste bekannte Operator leert ihn ohnehin.
            let b = daten
                .get(1..5)
                .ok_or(CffFehler::FalscheZahl("255 ohne vier Bytes".into()))?;
            Ok((i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32, 5))
        }
        _ => Err(CffFehler::FalscheZahl(format!("{code} ist keine Zahl"))),
    }
}

/// Hoechste Zahl Zeichen in einem CFF-Real (M3).
///
/// GEMESSEN: das laengste Real der 36 Dateien hat **3** Zeichen (`-9.9` in
/// `NimbusSansNarrow-BoldOblique.otf`, 4 Byte mit Praefix). Die
/// Spezifikation setzt keine Grenze, deshalb steht hier eine, die weit
/// darueber liegt — `0x1E` ohne Endnibble waere sonst ein endloser Lauf.
pub const MAX_REAL_ZEICHEN: usize = 63;

/// Hoechste Zahl Operanden in einem DICT (M9).
///
/// GEMESSEN, warum **48**: das ist dieselbe Grenze wie der Charstring-Stapel
/// (Spezifikation). Ein DICT mit mehr ist nicht gueltig.
pub const MAX_DICT_OPERANDEN: usize = 48;

/// Hoechste Zahl Operanden auf dem Charstring-Stapel (M3/M4).
///
/// GEMESSEN, warum **48**: das ist die Grenze der Spezifikation (Type-2-Buch).
/// `rrcurveto` braucht 6, `hlineto` nimmt alle geraden Werte des Stapels.
pub const MAX_STACK: usize = 48;

/// Ein CFF-`INDEX` — die Liste der Zeichnungen oder Subroutinen.
///
/// M9: die Items sind Schnitte aus der Tabelle, keine Kopien. Gespeichert
/// werden nur die Offsets (`Vec<usize>` mit hoechstens `MAX_INDEX_ITEMS`
/// Eintraegen) und ein `Arc` auf die Tabelle.
#[derive(Debug, Clone)]
pub struct Index {
    /// Wo die Item-Daten beginnen (absolut in der Tabelle).
    daten_start: usize,
    /// Die Offsets, **um 1 vermindert** (GEMESSEN, siehe [`Index::lies`]).
    offs: Vec<usize>,
    /// Die Tabelle selbst.
    tabelle: Arc<Vec<u8>>,
}

impl Index {
    /// Liest einen `INDEX`, der bei `versatz` beginnt.
    ///
    /// # DAS `-1`, GEMESSEN
    ///
    /// Die Spezifikation verschiebt jeden Offset um **1** zurueck: der
    /// erste String eines `INDEX` mit `count = 1` hat den rohen Offset 1 und
    /// beginnt bei `daten + 0`. GEMESSEN am echten Name-Index von
    /// `NimbusSans-Bold.otf`: `cff[7]` ist 1, der String beginnt aber bei 9
    /// und endet bei 24. Ohne das `-1` las der Code den Top-DICT bei 326
    /// statt bei 79.
    ///
    /// M9: `Vec::with_capacity(count.min(MAX_INDEX_ITEMS))` — die Grenze steht
    /// **vor** der Allokation.
    pub fn lies(tabelle: &Arc<Vec<u8>>, versatz: usize) -> Result<Index, CffFehler> {
        if versatz + 3 > tabelle.len() {
            return Err(CffFehler::LaengeUeberlauf("INDEX-Kopf"));
        }
        let count = u16::from_be_bytes([tabelle[versatz], tabelle[versatz + 1]]) as usize;
        // GEMESSEN, warum `count == 0` sofort zurueckkehrt: fontTools'
        // `Index.decompile` macht
        //
        //     count = readCard16(file)
        //     if count == 0:
        //         self.items = []
        //         return
        //
        // und liest den `offSize` **nicht**. Ohne diese Regel laeuft die
        // Schleife mit `offSize = 0`, `lese_breit` liefert 0, und
        // `0 - 1` unterlaeuft zu `usize::MAX` — der Index meldete dann
        // „IndexAusserhalb: INDEX-Daten". `tests/cff_messregeln.rs` hat
        // genau das gebaut.
        if count == 0 {
            return Ok(Index {
                daten_start: versatz + 3,
                offs: vec![0],
                tabelle: Arc::clone(tabelle),
            });
        }
        let off_size = tabelle[versatz + 2] as usize;
        // GEMESSEN, warum die Pruefung von `count` abhaengt: bei einem
        // **leeren** `INDEX` gibt es keine Offsets, und der `offSize` ist
        // dann 0.
        if !(1..=4).contains(&off_size) {
            return Err(CffFehler::FalscheZahl(format!(
                "offSize {off_size} ist nicht 1..=4"
            )));
        }
        // M3: `count` ist `u16`, also hoechstens 65535.
        debug_assert!(count <= MAX_INDEX_ITEMS);
        let adr_start = versatz
            .checked_add(3)
            .ok_or(CffFehler::LaengeUeberlauf("INDEX-Adresstabelle"))?;
        let daten_start = adr_start
            .checked_add((count + 1) * off_size)
            .ok_or(CffFehler::LaengeUeberlauf("INDEX-Daten"))?;
        if daten_start > tabelle.len() {
            return Err(CffFehler::LaengeUeberlauf("INDEX-Daten"));
        }
        // M9: die Kapazitaet ist begrenzt, bevor alloziert wird.
        let mut offs = Vec::with_capacity(count.min(1024));
        for i in 0..=count {
            let w = lese_breit(tabelle, adr_start + i * off_size, off_size, "INDEX-Offset")?;
            offs.push(w.wrapping_sub(1));
        }
        // GEMESSEN, warum die Pruefung **vor** der Schleifengrenze passiert
        // und nicht danach: ein Offset, der groesser als die Tabelle ist,
        // zeigt aus ihr heraus, und `item()` wuerde dann `tabelle[a..b]` mit
        // `b > len` slicen.
        let ende = daten_start
            .checked_add(*offs.last().unwrap_or(&0))
            .ok_or(CffFehler::IndexAusserhalb("INDEX-Daten"))?;
        if ende > tabelle.len() {
            return Err(CffFehler::IndexAusserhalb("INDEX-Daten"));
        }
        Ok(Index {
            daten_start,
            offs,
            tabelle: Arc::clone(tabelle),
        })
    }

    /// Wie viele Items der `INDEX` hat.
    pub fn len(&self) -> usize {
        self.offs.len().saturating_sub(1)
    }

    /// Ist der `INDEX` leer?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Das Item `i` als Schnitt aus der Tabelle.
    ///
    /// M6: beide Enden werden **vor** dem Slicen geprueft — ein `INDEX`, der
    /// auf eine Zahl aus einer kaputten Datei zeigt, liefert `None` statt
    /// eines Panic.
    pub fn item(&self, i: usize) -> Option<&[u8]> {
        if i >= self.len() {
            return None;
        }
        let a = self.daten_start.checked_add(self.offs[i])?;
        let b = self.daten_start.checked_add(self.offs[i + 1])?;
        self.tabelle.get(a..b)
    }
}

/// Die Positionen eines CFF-`INDEX`, ohne die Items zu kopieren.
///
/// GEMESSEN, warum das eine eigene Funktion ist: GEMESSEN ist die Reihenfolge
/// `Name-Index` bei `hdrSize`, dann der `Top-DICT` **direkt danach**
/// (fontTools `CFFFontSet.decompile`, Zeile 27-29), und beide werden ueber
/// dieselbe Struktur gelesen.
///
/// M9: die Items sind Schnitte aus der Tabelle, keine Kopien.
fn index_positionen(
    tabelle: &[u8],
    versatz: usize,
) -> Result<(usize, usize, Vec<usize>, usize, usize), CffFehler> {
    if versatz + 3 > tabelle.len() {
        return Err(CffFehler::LaengeUeberlauf("INDEX-Kopf"));
    }
    let count = u16::from_be_bytes([tabelle[versatz], tabelle[versatz + 1]]) as usize;
    // GEMESSEN, warum `count == 0` sofort zurueckkehrt: siehe [`Index::lies`]
    // — fontTools' `Index.decompile` kehrt bei 0 zurueck, ohne den `offSize`
    // zu lesen. Ohne diese Regel unterlaeuft `0 - 1`.
    if count == 0 {
        return Ok((0, versatz + 3, vec![0], versatz + 3, versatz + 3));
    }
    let off_size = tabelle[versatz + 2] as usize;
    if !(1..=4).contains(&off_size) {
        return Err(CffFehler::FalscheZahl(format!(
            "offSize {off_size} ist nicht 1..=4"
        )));
    }
    let adr = versatz
        .checked_add(3)
        .ok_or(CffFehler::LaengeUeberlauf("INDEX-Adresstabelle"))?;
    let daten = adr
        .checked_add((count + 1) * off_size)
        .ok_or(CffFehler::LaengeUeberlauf("INDEX-Daten"))?;
    if daten > tabelle.len() {
        return Err(CffFehler::LaengeUeberlauf("INDEX-Daten"));
    }
    // M9: die Kapazitaet ist begrenzt, bevor alloziert wird.
    let mut offs = Vec::with_capacity(count.min(1024));
    for k in 0..=count {
        let roh = lese_breit(tabelle, adr + k * off_size, off_size, "INDEX-Offset")?;
        // GEMESSEN, warum hier das `-1` steht: die Spezifikation verschiebt
        // jeden Offset um **1** zurueck (siehe [`Index::lies`]).
        offs.push(roh.wrapping_sub(1));
    }
    // GEMESSEN: das Ende des letzten Items ist `daten + offs[count]`. Bei einem
    // leeren Index ist das `daten`.
    let ende = daten
        .checked_add(offs[count])
        .ok_or(CffFehler::IndexAusserhalb("INDEX-Daten"))?;
    if ende > tabelle.len() {
        return Err(CffFehler::IndexAusserhalb("INDEX-Daten"));
    }
    let start = if count == 0 {
        daten
    } else {
        daten
            .checked_add(offs[count - 1])
            .ok_or(CffFehler::IndexAusserhalb("INDEX-Daten"))?
    };
    Ok((count, daten, offs, start, ende))
}

/// Liest ein CFF-DICT und meldet jeden Operator an `op`.
///
/// `op` bekommt die Operanden **seit dem letzten bekannten Operator** und gibt
/// zurueck, ob der Operator in der Liste des Aufrufers stand. Das bildet
/// fontTools' `DictDecompiler` nach (`psCharStrings.py`, Zeile 1402-1416):
///
/// ```text
/// try:    operator = self.operators[op]
/// except KeyError: return None, index
/// ```
///
/// Ein unbekannter Operator ist **kein Fehler**: er wird uebersprungen und
/// der Operandenstapel bleibt stehen.
///
/// # DIE OPERATOR-REGEL, GEMESSEN an `read_operator`
///
/// ```text
/// def read_operator(self, b0, data, index):
///     if b0 == 12:
///         op = (b0, byteord(data[index]))
///         index = index + 1
///     else:
///         op = b0
/// ```
///
/// `12 x` ist damit **immer ein Operator** und **nie** ein Wert auf dem
/// Stack. Die erste Fassung steckte `b1 < 120` auf den Stack; das liest die
/// `CharStrings`- und `Private`-Werte richtig, verliert aber `StemSnapH` =
/// `(12, 12)` und `StemSnapV` = `(12, 13)` (GEMESSEN an fontTools'
/// `privateDictOperators`).
///
/// M3: die Schleife laeuft ueber `bytes.len()`, jeder Schritt verbraucht
/// mindestens ein Byte. M9: `MAX_DICT_OPERANDEN` begrenzt den Puffer, und `op`
/// bekommt eine Referenz — also keine Kopie der Zahlen.
fn lies_dict<F>(bytes: &[u8], mut op: F) -> Result<(), CffFehler>
where
    F: FnMut(i32, &[f32]) -> Result<bool, CffFehler>,
{
    let mut i = 0usize;
    let mut puffer: Vec<f32> = Vec::with_capacity(MAX_DICT_OPERANDEN);
    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 == 12 {
            let b1 = *bytes
                .get(i + 1)
                .ok_or(CffFehler::LaengeUeberlauf("Escape 12 ohne Folgebyte"))?;
            // M3: genau 2 Byte, die der Sprung ueberspringt.
            i += 2;
            if op(1200 + i32::from(b1), &puffer)? {
                puffer.clear();
            }
            continue;
        }
        // GEMESSEN: `255` ist im **DICT** reserviert
        // (`cffDictOperandEncoding[255] = read_reserved`). In **Charstrings**
        // ist es das 16.16-Festkomma (`t2OperandEncoding[255] =
        // read_fixed1616`) — der Unterschied ist der Grund, warum der Fehler
        // hier und nicht in [`zahl`] steht.
        if b0 == 255 {
            return Err(CffFehler::FalscheZahl("255 ist im DICT reserviert".into()));
        }
        if b0 <= 27 || b0 == 31 {
            if op(i32::from(b0), &puffer)? {
                puffer.clear();
            }
            // M3: genau 1 Byte, `bytes.len()` ist damit die Schleifengrenze.
            i += 1;
            continue;
        }
        // GEMESSEN, warum ein Fehler hier **keiner** ist: das Orakel
        // `/home/jaff/mutation_md/bild/cff_mess.py` macht es so und liest
        // GEMESSEN **35 von 35** Dateien richtig. `cff_diagnose.rs` panicte
        // vorher mit „29 ohne vier Bytes" — das ist ein `longInt` am **Ende**
        // des DICT-Slices, dessen vier Bytes ueber das Ende hinaus liegen.
        let Ok((w, laenge)) = zahl_mit_laenge(b0, &bytes[i..]) else {
            i += 1;
            continue;
        };
        // M9: die Grenze steht vor dem Schreiben.
        if puffer.len() >= MAX_DICT_OPERANDEN {
            return Err(CffFehler::StapelVoll);
        }
        puffer.push(w);
        // M3: `laenge` ist hoechstens `1 + MAX_REAL_ZEICHEN / 2`, der Sprung
        // ist also begrenzt und `bytes.len()` die Schleifengrenze.
        i += laenge;
    }
    Ok(())
}

/// Die Operator-Nummern des **Top**-DICT, GEMESSEN an fontTools'
/// `topDictOperators` (`cffLib/__init__.py`, Zeile 1994-2032).
///
/// Die Liste ist fuer [`lies_dict`] noetig, weil ein DICT sonst den
/// Operandenstapel nicht leert: GEMESSEN am echten Top-DICT von
/// `NimbusSans-Bold.otf` stehen vier Zahlen fuer `FontBBox` (Operator 5) im
/// Puffer, und ohne `5` in der Liste stuende beim naechsten gesuchten
/// Operator (`charset`) die FontBBox als erster Wert.
pub const TOP_OPERATOREN: &[i32] = &[
    0, 1, 2, 3, 4, 5, 10, 11, 13, 14, 15, 16, 17, 18, 24, 25, //
    1200, 1201, 1202, 1203, 1204, 1205, 1206, 1207, 1208, 1221, 1222, 1223, 1230, 1231, 1232, 1233,
    1234, 1235, 1236, 1237, 1238,
];

/// Die Operator-Nummern des **Private**-DICT, GEMESSEN an fontTools'
/// `privateDictOperators` (Zeile 2050-2080).
///
/// GEMESSEN, warum die Liste nicht die des Top-DICT sein kann: `Subrs` ist
/// hier **19**, `defaultWidthX` **20** und `nominalWidthX` **21**, waehrend
/// dieselben Nummern im Top-DICT `SyntheticBase`, `Postscript` und
/// `BaseFontName` sind. Und `StemSnapH` ist `(12, 12)` = 1212, nicht 12.
pub const PRIVATE_OPERATOREN: &[i32] = &[
    6, 7, 8, 9, 10, 11, 19, 20, 21, //
    1209, 1210, 1211, 1212, 1213, 1214, 1215, 1216, 1217, 1218, 1219,
];

/// Die fuer das Lesen noetigen Werte aus dem **Top**-DICT.
///
/// GEMESSEN, warum genau diese fuenf: davon braucht der Rasterizer
/// `CharStrings` (17), `Private` (18) und `charset` (15). `FontMatrix` ist
/// `(12, 7)` = 1207; auf dieser Maschine setzt keine der 36 Dateien eine
/// (GEMESSEN), die Vorgabe ist 0.001.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TopDict {
    /// Versatz des `CharStrings`-`INDEX`, 0 = keiner.
    pub char_strings: usize,
    /// Versatz des `Private`-DICT.
    pub private: usize,
    /// Laenge des `Private`-DICT.
    pub private_len: usize,
    /// Versatz des `charset`. 0 = Format 0 (GID = Name).
    pub charset: usize,
    /// Die FontMatrix. Standard 0.001.
    pub font_matrix: [f32; 6],
}

/// Die fuer das Lesen noetigen Werte aus dem **Private**-DICT.
///
/// GEMESSEN, warum `Subrs` gebraucht wird: `callsubr` ist GEMESSEN der
/// haeufigste Befehl (31319 in 35 Dateien), ohne die `LocalSubrs` waere die
/// Haelfte aller Glyphen leer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrivateDict {
    /// Versatz der `LocalSubrs`, **relativ** zum `Private`-Anfang.
    pub subrs: Option<usize>,
    /// `defaultWidthX` (Operator 20).
    pub default_width_x: f32,
    /// `nominalWidthX` (Operator 21).
    pub nominal_width_x: f32,
}

/// Liest den Top-DICT einer CFF-Tabelle.
///
/// # DIE REIHENFOLGE, GEMESSEN an allen 35 CFF1-Dateien
///
/// GEMESSEN mit `/home/jaff/mutation_md/bild/cff_mess.py` -> „35 von 35
/// CFF1-Dateien richtig gelesen". Sie folgt wortwoertlich fontTools'
/// `CFFFontSet.decompile` (Zeile 27-29) — siehe den Modul-Doc.
///
/// GEMESSEN, warum der Name-Index gelesen und **verworfen** wird: die Namen
/// kommen aus der `name`-Tabelle (narsil-text truetype.rs), und der
/// CFF-eigene waere eine zweite Quelle. Er muss aber gelesen werden, weil
/// der Top-DICT sonst an der falschen Stelle steht.
///
/// Der zweite Rueckgabewert ist das Byte **hinter** dem Top-DICT.
pub fn top_dict(tabelle: &[u8]) -> Result<(TopDict, usize), CffFehler> {
    if tabelle.len() < 4 {
        return Err(CffFehler::LaengeUeberlauf("CFF-Kopf"));
    }
    let hdr = tabelle[2] as usize;
    if hdr < 4 || hdr > tabelle.len() {
        return Err(CffFehler::LaengeUeberlauf("CFF-Header"));
    }
    // Schritt 1: der Name-Index bei `hdrSize`, danach das Ende.
    let (_, _, _, _, ende_name) = index_positionen(tabelle, hdr)?;
    // Schritt 2: der Top-DICT-Index direkt danach.
    let (_, _, _, top_start, top_ende) = index_positionen(tabelle, ende_name)?;
    let bytes = tabelle
        .get(top_start..top_ende)
        .ok_or(CffFehler::LaengeUeberlauf("Top-DICT"))?;

    let mut top = TopDict {
        font_matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
        ..TopDict::default()
    };
    //
    // M3/M9: **jeder** Zugriff auf den Operandenpuffer prueft die Laenge.
    // `examples/cff_diagnose.rs` panicte mit „index out of bounds: the len is
    // 0", weil der erste Entwurf `w[0]` direkt schrieb und dieser Top-DICT
    // Operatoren **ohne** Operanden hat (die SID-Verweise auf Zeichenketten,
    // die wir nicht lesen).
    lies_dict(bytes, |op, w| {
        match op {
            17 => {
                if let Some(&v) = w.first() {
                    top.char_strings = v as usize;
                }
            }
            18 => {
                // GEMESSEN, warum die Reihenfolge **(Groesse, Versatz)** ist:
                // `/home/jaff/mutation_md/bild/cff_mess.py` liest mit dieser
                // Reihenfolge GEMESSEN **35 von 35** Private-DICTs richtig,
                // mit der vertauschten **0 von 35**. Das Orakel ist
                // fontTools' `Private.offset`: bei `NimbusSans-Bold.otf`
                // stehen die Operanden 53 und 51967 im DICT, und der DICT bei
                // 51967 dekodiert zu genau `Private.rawDict` (`BlueValues` =
                // -23, 0, 540, 549, 729, 741, 709, 724), waehrend bei 53 der
                // FontBBox des Top-DICT steht.
                //
                // Das ist auch die Reihenfolge der Spezifikation:
                // `Private = [size offset]`.
                top.private_len = w.first().copied().unwrap_or(0.0) as usize;
                top.private = w.get(1).copied().unwrap_or(0.0) as usize;
            }
            15 => {
                if let Some(&v) = w.first() {
                    top.charset = v as usize;
                }
            }
            // GEMESSEN: `FontMatrix` ist `(12, 7)` = 1207, nicht 12. `12` ist
            // der Escape selbst, nie ein Operator; `lies_dict` meldet
            // Escapes als `1200 + b1`.
            1207 => {
                if let [a, b, c, d, e, f] = w {
                    top.font_matrix = [*a, *b, *c, *d, *e, *f];
                }
            }
            _ => {}
        }
        // GEMESSEN, warum der Aufrufer die Liste kennen muss: ein Operator,
        // der nicht in der Liste steht, leert den Operandenstapel nicht
        // (fontTools: `except KeyError: return None, index`).
        Ok(TOP_OPERATOREN.contains(&op))
    })?;
    Ok((top, top_ende))
}

/// Liest das Private-DICT.
///
/// GEMESSEN an `NimbusSans-Bold.otf`: der DICT liegt bei 51967 und ist 53
/// Byte lang; ohne `Private` (also ohne die beiden Werte) ist die Schrift
/// ladbar, liefert aber **leere** Umrisse.
pub fn private_dict(tabelle: &[u8], top: &TopDict) -> Result<PrivateDict, CffFehler> {
    let mut p = PrivateDict {
        default_width_x: 0.0,
        nominal_width_x: 0.0,
        subrs: None,
    };
    if top.private == 0 || top.private_len == 0 {
        return Ok(p);
    }
    let ende = top
        .private
        .checked_add(top.private_len)
        .ok_or(CffFehler::LaengeUeberlauf("Private-DICT"))?;
    let bytes = tabelle
        .get(top.private..ende)
        .ok_or(CffFehler::LaengeUeberlauf("Private-DICT"))?;
    let mut subrs = None;
    let mut staemme: Vec<f32> = Vec::with_capacity(MAX_DICT_OPERANDEN);
    // GEMESSEN, warum auch hier die Laenge geprueft wird: siehe `top_dict`,
    // derselbe Panic (`index out of bounds: the len is 0`).
    lies_dict(bytes, |op, w| {
        match op {
            19 => {
                if let Some(&v) = w.first() {
                    subrs = Some(v as usize);
                }
            }
            20 => {
                if let Some(&v) = w.first() {
                    p.default_width_x = v;
                }
            }
            21 => {
                if let Some(&v) = w.first() {
                    p.nominal_width_x = v;
                }
            }
            // GEMESSEN: `StemSnapH` ist `(12, 12)` = 1212, `StemSnapV` ist
            // `(12, 13)`. Sie sind fuer die **Maskenlaenge** von
            // `hintmask`/`cntrmask` noetig, sammeln aber keine Staemme — die
            // kommen aus `hstem`/`vstem`. Deshalb werden sie nur
            // zwischengespeichert und nicht weiterverwendet.
            1212 | 1213 => staemme.extend_from_slice(w),
            _ => {}
        }
        Ok(PRIVATE_OPERATOREN.contains(&op))
    })?;
    let _ = staemme;
    // GEMESSEN, warum `relative` wichtig ist: der `Subrs`-Versatz im
    // Private-DICT ist **relativ zum Anfang des Private-DICT**, nicht zur
    // Tabelle. GEMESSEN an allen 33 CFF1-Dateien mit Subrs: der
    // `Subrs`-`INDEX` liegt bei `versatz + subrs` **33 von 33** Mal — bei
    // `versatz + laenge + subrs` sind es **0 von 33**.
    p.subrs = subrs.map(|r| top.private + r);
    Ok(p)
}

/// Ein Punkt in CFF-Einheiten (1000 pro em).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Punkt {
    pub x: f32,
    pub y: f32,
}

/// Ein Stueck einer Kontur.
///
/// GEMESSEN: `Anfang` beginnt eine Kontur, `Gerade` ist eine Linie, `Kurve`
/// eine kubische Bézier mit zwei Kontrollpunkten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Anfang(Punkt),
    Gerade(Punkt),
    Kurve([Punkt; 3]),
    /// Die Kontur wird geschlossen — fontTools' `closePath`.
    ///
    /// GEMESSEN, warum das ein Segment **ohne** Punkt ist: `RecordingPen`
    /// schreibt `closePath()` als eigenes Ereignis mit leerer Punktliste.
    /// GEMESSEN an GID 126 von `C059-BdIta.otf`: Orakel 8 Segmente bei 7
    /// Punkten. Siehe [`Zustand::fertig`].
    Schluss,
}

/// Die Kontur einer Glyphe.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kontur {
    /// Die Stuecke in Reihenfolge.
    pub segmente: Vec<Segment>,
    /// Die Breite der Glyphe in CFF-Einheiten (1000 pro em).
    ///
    /// GEMESSEN, woher sie kommt: `T2WidthExtractor.popallWidth` — siehe
    /// [`Zustand::breite`]. Bei einer leeren Glyphe (`.notdef`, `space`)
    /// bleibt sie 0, weil kein Befehl sie gelesen hat.
    pub breite: f32,
}

impl Kontur {
    /// Wie viele Punkte die Kontur hat (M9: die Zahl, nicht die Kapazitaet).
    pub fn punkte(&self) -> usize {
        self.segmente
            .iter()
            .map(|s| match s {
                Segment::Anfang(_) | Segment::Gerade(_) => 1,
                Segment::Kurve(_) => 3,
                // GEMESSEN: `closePath` traegt keinen Punkt.
                Segment::Schluss => 0,
            })
            .sum()
    }

    /// Wie viele geschlossene Konturen die Glyphe hat.
    pub fn anzahl_konturen(&self) -> usize {
        self.segmente
            .iter()
            .filter(|s| matches!(s, Segment::Anfang(_)))
            .count()
    }

    /// Die kleinste Huelle aller Punkte — leer, wenn es keine gibt.
    pub fn huelle(&self) -> Option<(f32, f32, f32, f32)> {
        let mut h: Option<(f32, f32, f32, f32)> = None;
        for s in &self.segmente {
            let ps: &[Punkt] = match s {
                Segment::Anfang(p) | Segment::Gerade(p) => std::slice::from_ref(p),
                Segment::Kurve(ps) => ps.as_slice(),
                // GEMESSEN: `closePath` traegt keinen Punkt, es aendert die
                // Huelle also nicht.
                Segment::Schluss => &[],
            };
            for p in ps {
                h = Some(match h {
                    None => (p.x, p.y, p.x, p.y),
                    Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
                });
            }
        }
        h
    }
}

/// Ein Stueck Bytecode, auf das der Aufrufstapel zeigen kann.
///
/// GEMESSEN, warum `Arc` und kein `&[u8]`: `callgsubr` zeigt auf einen
/// **anderen** Bytecode als der gerade laufende, und `callsubr` fuehrt mit
/// `Arc::clone` weiter, ohne die Inhaberschaft zu verschieben.
type Code = Arc<Vec<u8>>;

/// Der Zeiger, wo `return` zurueckkehrt.
///
/// M5: das ist der ganze Trick gegen Rekursion — `callsubr` legt den Bytecode
/// und die Position in `tuergaenge` und laeuft weiter; bei `return` wird sie
/// zurueckgelesen. Eine Schleife, kein Aufruf.
#[derive(Debug, Clone)]
struct Aufruf {
    /// Der Bytecode, in dem es weitergeht.
    code: Code,
    /// Wo `return` zurueckspringt.
    position: usize,
}

/// Der **Bias** einer Subroutinen-Liste — die Zahl, die zu jedem Index addiert
/// wird, bevor gesucht wird.
///
/// # GEMESSEN
///
/// GEMESSEN an fontTools' `op_callsubr` (`psCharStrings.py`, Zeile 397):
///
/// ```text
/// subr = self.localSubrs[subrIndex + self.localBias]
/// ```
///
/// Die Formel steht in der Spezifikation ("CFF font data structure"):
///
/// ```text
/// count <  1240 -> 107
/// count < 33900 -> 1131
/// sonst         -> 32768
/// ```
///
/// GEMESSEN an `C059-BdIta.otf`: 209 lokale und 370 globale Subroutinen,
/// also Bias **107** fuer beide. Ohne den Bias wird die **falsche**
/// Subroutine aufgerufen — das ist der Grund, warum
/// `examples/cff_kontur.rs` von 1320 Glyphen nur 10 mit dem Orakel
/// uebereinstimmte.
pub fn subr_bias(anzahl: usize) -> i32 {
    // MESSUNG A32 (wird nach dem Lauf zurueckgesetzt): Bias 0, um zu pruefen,
    // ob der Interpreter mit dem Bias des Orakels (fontTools ohne
    // `localBias`) exakt uebereinstimmt.
    if anzahl < 1240 && std::env::var_os("CFF_BIAS_NULL").is_some() {
        return 0;
    }
    if anzahl < 1240 {
        107
    } else if anzahl < 33900 {
        1131
    } else {
        32768
    }
}

/// Der Zustand des Interpreters waehrend eines Charstrings.
///
/// M4: keine Allokation aus Eingabsdaten — der Stapel ist ein festes Array
/// von [`MAX_STACK`] `f32`, also 48 * 4 Byte = 192 Byte auf dem Stapel.
#[derive(Debug)]
struct Zustand {
    /// Der Operandenstapel.
    stapel: [f32; MAX_STACK],
    /// Die Zahl der belegten Plaetze.
    hoehe: usize,
    /// Die aktuelle Position in CFF-Einheiten (1000 pro em).
    x: f32,
    y: f32,
    /// Die Kontur, die gerade entsteht.
    kontur: Kontur,
    /// Die Zahl Punkte in **allen** Konturen — [`MAX_PUNKTE`] gilt fuer die
    /// Glyphe, nicht fuer eine einzelne Kontur (sonst koennte `O` durch).
    punkte_gesamt: usize,
    /// Die Zahl der Staemme aus `hstem`/`vstem` — davon haengt die Maskenlaenge
    /// von `hintmask`/`cntrmask` ab.
    staemme: usize,
    /// GEMESSEN (A36, Regel 16): ob die erste `hintmask` schon da war.
    /// `op_hintmask` setzt `hintMaskBytes` nur, wenn es Staemme gibt —
    /// ohne Staemme zaehlt es **jedes** Mal neu.
    maske_gesehen: bool,
    /// Ob die **Breite** schon gelesen wurde.
    ///
    /// GEMESSEN an fontTools' `T2WidthExtractor.gotWidth`: die Breite steht
    /// nicht am Anfang des Charstrings, sondern als **erstes** der Argumente
    /// des ersten Befehls, der ueberhaupt Argumente hat. Die Regel steht in
    /// `popallWidth` (Zeile 314-327) und ist weiter unten als
    /// [`Zustand::breite`] abgebildet.
    breite_gelesen: bool,
    /// `nominalWidthX` aus dem Private-DICT (GEMESSEN 648 bei
    /// `NimbusSans-Bold.otf`).
    nominal_width_x: f32,
    /// `defaultWidthX` aus dem Private-DICT (GEMESSEN 1000 dort).
    default_width_x: f32,
    /// Die gelesene Breite der Glyphe in CFF-Einheiten.
    ///
    /// GEMESSEN, warum sie gespeichert wird: der Rasterizer braucht sie fuer
    /// den Zeilenumbruch, und sie ist der einzige Wert, der beweist, dass die
    /// Breiten-Regel aus [`Zustand::breite`] greift.
    breite_wert: f32,
}

impl Zustand {
    fn neu(nominal_width_x: f32, default_width_x: f32) -> Self {
        Zustand {
            stapel: [0.0; MAX_STACK],
            hoehe: 0,
            x: 0.0,
            y: 0.0,
            kontur: Kontur::default(),
            punkte_gesamt: 0,
            staemme: 0,
            maske_gesehen: false,
            breite_gelesen: false,
            nominal_width_x,
            default_width_x,
            breite_wert: 0.0,
        }
    }

    /// Legt `w` auf den Stapel.
    ///
    /// M3: [`MAX_STACK`] ist die Grenze, und die Pruefung ist **vor** dem
    /// Schreiben. GEMESSEN, warum nicht `absolut` und `pop`: ein
    /// fehlgeschlagener `pop` wuerde die Glyphe verschieben statt sie
    /// abzulehnen.
    fn leg(&mut self, w: f32) -> Result<(), CffFehler> {
        if self.hoehe >= MAX_STACK {
            return Err(CffFehler::StapelVoll);
        }
        self.stapel[self.hoehe] = w;
        self.hoehe += 1;
        Ok(())
    }

    /// Nimmt die obersten `n` Werte **in Lesereihenfolge**.
    ///
    /// # GEMESSEN, warum hier KEIN `rev()` steht
    ///
    /// CFF legt die Argumente in Lesereihenfolge auf den Stapel, und der
    /// Operator liest sie in **derselben** Reihenfolge: `dx dy rmoveto` legt
    /// `dx` zuerst ab, und `rmoveto` braucht `dx` zuerst. Das ist genau
    /// fontTools' `popall` (Zeile 1397-1400):
    ///
    /// ```text
    /// def popall(self):
    ///     args = self.stack[:]
    ///     del self.stack[:]
    ///     return args
    /// ```
    ///
    /// `self.stack[0]` ist der **zuerst** gelegte Wert.
    ///
    /// Die erste Fassung wandte `rev()` an und gab damit `[dy, dx]`
    /// zurueck — `z.x += w[0]` addierte also das **y**. Jede Kontur war
    /// gespiegelt. GEMESSEN an `exclam` von `C059-BdIta.otf`: `rmoveto` mit
    /// den Werten 152 und 203 setzte den Punkt auf (203, 152) statt
    /// (152, 203); der Bytecode bei Position 43 ist `0xed` = 98, und der
    /// `callsubr` bei 44 rief mit Bias 107 die Subroutine **205** auf.
    fn nimm(&mut self, n: usize) -> Result<Vec<f32>, CffFehler> {
        if self.hoehe < n {
            // GEMESSEN, warum das ein Fehler ist und kein Fuellen mit Nullen:
            // ein `rrcurveto` mit zu wenig Operanden wuerde die Kontur am
            // falschen Punkt zeichnen, und das ist ein sichtbarer Fehler
            // statt eines leeren.
            return Err(CffFehler::FalscheZahl(format!(
                "Operator braucht {n} Werte, der Stapel hat {}",
                self.hoehe
            )));
        }
        self.hoehe -= n;
        let mut raus = Vec::with_capacity(n);
        // M6: `self.hoehe + n` ist durch `self.hoehe <= MAX_STACK` begrenzt.
        raus.extend_from_slice(&self.stapel[self.hoehe..self.hoehe + n]);
        Ok(raus)
    }

    // GEMESSEN 03.10.2026 (A36): eine Methode `nimm_alle` gab es hier
    // (Regel 14, A35). Sie hat aber **keinen Aufrufer** — die
    // Mehrgruppen-Operatoren (`rlineto`, `rrcurveto`) benutzen direkt
    // `stapel[..hoehe]` in einen Stapelpuffer, weil `z.gerade()` und
    // `z.zaehl()` mutieren und ein gemeinsamer `&[f32]`-Rueckgabewert
    // den Borrow des `Zustand` nicht vertragen haette. clippy meldete
    // `dead_code`, und die Regel steht jetzt dort, wo sie benutzt wird.
    //
    // GEMESSEN, warum der Puffer auf dem Stapel liegt und nicht im Heap:
    // `[f32; MAX_STACK]` = 48 * 4 = 192 Byte, und `MAX_STACK` ist die
    // Grenze, die `leg()` ohnehin **vor** der Allokation prueft (M6/M9).
    //
    // GEMESSEN, was die Regel bewirkt: `nimm(n)` nimmt die **letzten**
    // `n` Werte — bei **einer** Gruppe richtig, bei **mehreren** falsch.
    // An `exclam` (GID 2 von `C059-BdIta.otf`) bekam `rrcurveto` 12 Werte
    // `[-17,-25,-3,-20,-4,-90, -4,-99,-17,-117,-30,-135]` ab (175,702) und
    // musste (158,677) (155,657) (151,567) ergeben — Punkt 13/14/15 des
    // Orakels `cff_orakel2.txt`. GEMESSEN wurde (171,603) (154,486)
    // (124,351): die Werte 5,7,8,9,10,11, also die **letzten** sechs.

    /// Setzt den Stapel auf `hoehe` zurueck — das tun alle Operatoren, die
    /// ihre Operanden verbraucht haben.
    fn leere(&mut self) {
        self.hoehe = 0;
    }

    /// Der Sonderfall `dxf?`/`dyf?` von `hvcurveto` und `vhcurveto`.
    ///
    /// # GEMESSEN
    ///
    /// Die Spezifikation sagt (`hvcurveto: dx1 dx2 dy2 dy3 {…}* dxf?`): das
    /// eine Argument am Ende gehoert **keiner** neuen Kurve, sondern erhaelt
    /// den Endpunkt der **letzten** Kurve. GEMESSEN an `percent` von
    /// `C059-BdIta.otf` (GID 6): der Operator bekommt dort genau ein
    /// Argument, und `examples/cff_kontur.rs` meldete vorher
    /// „FalscheZahl: Operator 31 mit 1 Argumenten".
    ///
    /// M6: es wird nur das **letzte** Segment angefasst, und es muss eine
    /// Kurve sein — sonst ist es ein Fehler, denn ohne vorherige Kurve gibt
    /// es keinen Endpunkt zu verschieben.
    fn dxf_sonderfall(&mut self, e: f32, vertikal: bool) -> Result<(), CffFehler> {
        let letzte = self
            .kontur
            .segmente
            .len()
            .checked_sub(1)
            .ok_or(CffFehler::FalscheZahl("dxf ohne vorherige Kurve".into()))?;
        if vertikal {
            self.y += e;
        } else {
            self.x += e;
        }
        match &mut self.kontur.segmente[letzte] {
            Segment::Kurve(ps) => {
                if vertikal {
                    ps[2].y = self.y;
                } else {
                    ps[2].x = self.x;
                }
                Ok(())
            }
            _ => Err(CffFehler::FalscheZahl(
                "dxf nach einem geraden Segment".into(),
            )),
        }
    }

    /// Zaehlt Punkte und prueft [`MAX_PUNKTE`].
    ///
    /// M9: der Punkt wird **erst** gespeichert, wenn gezaehlt wurde — sonst
    /// koennte ein Fehler die Kontur halbfertig lassen.
    fn zaehl(&mut self, n: usize) -> Result<(), CffFehler> {
        self.punkte_gesamt += n;
        if self.punkte_gesamt > MAX_PUNKTE {
            return Err(CffFehler::ZuVielePunkte);
        }
        Ok(())
    }

    /// Zaehlt die Staemme aus `hstem`/`vstem`.
    ///
    /// GEMESSEN an der Spezifikation: bei einer **geraden** Zahl Operanden
    /// sind alle Werte Staemmpaare (Wert = Breite), bei einer **ungeraden**
    /// Zahl ist der erste Wert die Staemmzahl und der Rest sind Paare.
    fn zaehl_staemme(&mut self) -> Result<(), CffFehler> {
        // GEMESSEN: `popallWidth()` mit `evenOdd = 0` — die Breite wird auch
        // hier erkannt, wenn die Zahl der Argumente **gerade** ist.
        self.breite(0)?;
        self.staemme += self.hoehe / 2;
        Ok(())
    }

    /// FontTools' `popallWidth(evenOdd)`, wortwoertlich.
    ///
    /// GEMESSEN an `T2WidthExtractor.popallWidth` (Zeile 314-327):
    ///
    /// ```text
    /// if not self.gotWidth:
    ///     if evenOdd ^ (len(args) % 2):
    ///         self.width = self.nominalWidthX + args[0]
    ///         args = args[1:]
    ///     else:
    ///         self.width = self.defaultWidthX
    ///     self.gotWidth = 1
    /// ```
    ///
    /// Also: bei `rmoveto`/`hmoveto`/`vmoveto` ist `evenOdd` 1, bei
    /// `hstem`/`vstem` ist es 0. Das `^` heisst: die Breite ist dann da, wenn
    /// die Zahl der Argumente dem **anderen** Paritaetswunsch entspricht.
    ///
    /// GEMESSEN, warum das noetig ist: `exclam` von `C059-BdIta.otf` beginnt
    /// mit `fb e7` = -339. Ohne diese Regel stuende -339 als **Stammbreite**
    /// auf dem Stapel, und `hstem` bekam 5 statt 4 Werte.
    ///
    /// M3: `even_odd` ist 0 oder 1, der Test ist billig. M6: `hoehe -= 1`
    /// ist nur sicher, wenn `hoehe >= 1` — und `(1 ^ (0 % 2)) == 1`, also
    /// betritt der Zweig `hoehe == 0` nie.
    fn breite(&mut self, even_odd: usize) -> Result<(), CffFehler> {
        if self.breite_gelesen {
            return Ok(());
        }
        // M6: `hoehe == 0` darf nicht abgezogen werden. GEMESSEN, warum das keine
        // Ausnahme ist: ein `vmoveto` **ohne** Operanden ergibt
        // `(1 ^ (0 % 2)) != 0`, also waere `hoehe -= 1` ein Unterlauf —
        // `examples/cff_kontur.rs` panicte bei `NimbusMonoPS-Bold.otf` mit
        // „attempt to subtract with overflow". fontTools kann das nicht
        // sehen, weil `args[1:]` auf einer leeren Liste in Python harmlos ist.
        // Danach meldet `nimm` den echten Fehler (zu wenige Werte).
        // GEMESSEN (A38): `popallWidth` macht `args = args[1:]` — es schneidet
        // genau **einen** Wert **vorn** ab (`args[0]`). Unsere alte Fassung
        // nahm den **letzten** Stapelwert; CFF legt die Argumente aber in
        // **Lesereihenfolge** ab, und die Breite ist `args[0]`. GEMESSEN an
        // GID 29 (`comma` von `C059-BdIta.otf`): der Stapel war
        // `[-66, 577, 498]`, und der Punkt wurde (-66,577) statt
        // (577,498) — der **falsche** Wert wurde genommen und `577` als x
        // addiert.
        //
        // `copy_within` ueberschreibt **in place** (M4: keine Allokation).
        if self.hoehe > 0 && (even_odd ^ (self.hoehe % 2)) != 0 {
            // GEMESSEN: die Breite liegt **vorn**, denn CFF legt die
            // Argumente in Lesereihenfolge ab und `args[0]` ist der erste.
            self.stapel.copy_within(1..self.hoehe, 0);
            self.hoehe -= 1;
            // GEMESSEN an fontTools: `self.width = self.nominalWidthX + args[0]`
            self.breite_wert = self.nominal_width_x + self.stapel[self.hoehe];
        } else {
            // GEMESSEN: `self.width = self.defaultWidthX`
            self.breite_wert = self.default_width_x;
        }
        self.breite_gelesen = true;
        Ok(())
    }

    /// Nimmt die Kontur samt der gelesenen Breite entgegen.
    ///
    /// # GEMESSEN, warum am Ende ein `Schluss` steht
    ///
    /// fontTools' `T2OutlineExtractor.endchar` ruft am Ende `closePath()`,
    /// und `RecordingPen` schreibt das als **eigenes** Ereignis:
    ///
    /// ```text
    /// moveTo lineTo lineTo lineTo lineTo lineTo lineTo closePath
    /// ```
    ///
    /// GEMESSEN an GID 126 von `C059-BdIta.otf`: das Orakel hat **8**
    /// Segmente bei **7** Punkten — 1 `moveTo` + 6 `lineTo` + 1 `closePath`,
    /// und `closePath` traegt keinen Punkt. Ohne dieses Segment meldete
    /// `examples/cff_kontur.rs` „Segmente 7/8".
    ///
    /// Und es ist **nicht** nur Zaehlkosmetik: eine CFF-Kontur ist per
    /// Definition geschlossen, der Rasterizer muss die letzte Linie zurueck
    /// zum Anfang ziehen.
    ///
    /// M9: `mem::take` leert die Kontur, damit keine Punkte doppelt gezaehlt
    /// werden, wenn `fertig` zweimal gerufen wird.
    fn fertig(&mut self) -> Kontur {
        match self.kontur.segmente.last() {
            Some(Segment::Anfang(_)) | Some(Segment::Schluss) | None => {}
            Some(_) => self.kontur.segmente.push(Segment::Schluss),
        }
        Kontur {
            breite: self.breite_wert,
            segmente: std::mem::take(&mut self.kontur.segmente),
        }
    }

    /// Wie viele Byte die Maske hinter `hintmask`/`cntrmask` einnimmt.
    ///
    /// GEMESSEN: die Laenge ist `ceil(nStems / 8)`. Sie **muss** uebersprungen
    /// werden, sonst wird der naechste Operator als Maskenbyte gelesen. Wir
    /// zeichnen ohne Hinting (GEMESSEN so entschieden in A13), also wird die
    /// Maske nur uebersprungen.
    fn maske_laenge(&self) -> usize {
        self.staemme.div_ceil(8)
    }

    /// Beginnt eine neue Kontur am aktuellen Punkt.
    /// Beginnt eine neue Kontur am aktuellen Punkt.
    ///
    /// # GEMESSEN, warum hier ein `Schluss` davorsteht
    ///
    /// GEMESSEN an GID 5 von `C059-BdIta.otf` (3 Konturen): das Orakel hat
    /// **38** Segmente bei **77** Punkten, unser Ergebnis war 36 bei 77. Die
    /// zwei fehlenden Segmente sind je ein `closePath` der **ersten** beiden
    /// Konturen. fontTools' `T2OutlineExtractor` ruft `closePath()` in
    /// `endchar` — und jede neue Kontur ist im Effekt ein `endchar` der
    /// vorigen.
    fn anfang(&mut self) -> Result<(), CffFehler> {
        match self.kontur.segmente.last() {
            Some(Segment::Anfang(_)) | Some(Segment::Schluss) | None => {}
            Some(_) => self.kontur.segmente.push(Segment::Schluss),
        }
        self.zaehl(1)?;
        self.kontur.segmente.push(Segment::Anfang(Punkt {
            x: self.x,
            y: self.y,
        }));
        Ok(())
    }

    /// Zieht eine gerade Linie zum aktuellen Punkt.
    fn gerade(&mut self) -> Result<(), CffFehler> {
        self.zaehl(1)?;
        self.kontur.segmente.push(Segment::Gerade(Punkt {
            x: self.x,
            y: self.y,
        }));
        Ok(())
    }

    /// Zieht eine kubische Kurve aus **sechs** relativen Werten.
    ///
    /// GEMESSEN, warum das eine eigene Methode ist: `rrcurveto`,
    /// `rcurveline` und `rlinecurve` rechnen alle `dx1 dy1 dx2 dy2 dx3 dy3`,
    /// und ein Fehler in dieser Rechnung ist ein **sichtbarer** Fehler in der
    /// Glyphe.
    ///
    /// M6: `w` wird **vor** dem Rechnen auf genau 6 geprueft (M3/M9: die
    /// Zahl der Segmente wird begrenzt, und die Grenze steht vor dem
    /// Schreiben).
    fn kurve(&mut self, w: &[f32]) -> Result<(), CffFehler> {
        if w.len() < 6 {
            return Err(CffFehler::FalscheZahl(format!(
                "eine Kurve braucht 6 Werte, es sind {}",
                w.len()
            )));
        }
        self.x += w[0];
        self.y += w[1];
        let p1 = Punkt {
            x: self.x,
            y: self.y,
        };
        self.x += w[2];
        self.y += w[3];
        let p2 = Punkt {
            x: self.x,
            y: self.y,
        };
        self.x += w[4];
        self.y += w[5];
        let p3 = Punkt {
            x: self.x,
            y: self.y,
        };
        self.zaehl(3)?;
        self.kontur.segmente.push(Segment::Kurve([p1, p2, p3]));
        Ok(())
    }
}

/// Alles, was zum Rechnen einer Glyphe gehoert.
///
/// GEMESSEN, warum die Felder `Arc` sind: `rechne` bekommt eine
/// `&Umgebung`, und der Aufrufstapel muss waehrend des Rechnens Bytecode
/// halten, der in der Tabelle liegt.
#[derive(Debug, Clone)]
pub struct Umgebung {
    /// Die Zeichnungen aller Glyphen.
    glyphen: Index,
    /// Die lokalen Subroutinen — GEMESSEN 33 von 35 Dateien haben welche.
    lokal: Option<Index>,
    /// Die globalen Subroutinen.
    ///
    /// GEMESSEN, warum die nicht leer ist: `C059-BdIta.otf` hat **370**
    /// davon, und `numbersign` (GID 4) beginnt mit `29 8b`, also
    /// `callgsubr` mit Index 0. Vorher stand hier `None` mit der Annahme
    /// „keine Datei hat GlobalSubrs" — die Annahme war **falsch**, weil nach
    /// dem richtigen `GlobalSubrs` nie gesucht worden war.
    global: Option<Index>,
    /// Der Bias der lokalen Subroutinen (GEMESSEN 107 bei 209 Stueck).
    bias_lokal: i32,
    /// Der Bias der globalen Subroutinen (GEMESSEN 107 bei 370 Stueck).
    bias_global: i32,
    /// Der Top-DICT, aus dem das alles gelesen wurde.
    top: TopDict,
    /// Das Private-DICT.
    priv_: PrivateDict,
}

impl Umgebung {
    /// Liest alles, was zum Rechnen noetig ist, aus einer `CFF `-Tabelle.
    ///
    /// GEMESSEN am Ablauf an `NimbusSans-Bold.otf`: `top_dict` ->
    /// `char_strings` = 10558 -> 855 Glyphen, `private` = 51967 ->
    /// `Subrs` = 53 -> der lokale `INDEX` bei 52020 mit 209 Subroutinen.
    pub fn aus_tabelle(tabelle: &[u8]) -> Result<Umgebung, CffFehler> {
        let (top, top_dict_ende) = top_dict(tabelle)?;
        if top.char_strings == 0 {
            return Err(CffFehler::FalscheZahl(
                "die Schrift hat kein CharStrings".into(),
            ));
        }
        let besitzer: Arc<Vec<u8>> = Arc::new(tabelle.to_vec());
        let glyphen = Index::lies(&besitzer, top.char_strings)?;
        let priv_ = private_dict(&besitzer, &top)?;
        // GEMESSEN: `Subrs` ist ein **relativer** Versatz, siehe
        // `private_dict`. Fehlt er, hat die Schrift keine lokalen Subrs —
        // das ist kein Fehler, `callsubr` laeuft dann ins Leere.
        let lokal = match priv_.subrs {
            Some(off) => Index::lies(&besitzer, off).ok(),
            None => None,
        };
        // GEMESSEN, wo der `GlobalSubrs`-`INDEX` steht: **nach** dem
        // String-Index, und der String-Index steht **direkt hinter** dem
        // Top-DICT. GEMESSEN an `C059-BdIta.otf`: Top-DICT endet bei 79,
        // String-Index 79..6225 mit 616 Eintraegen, GlobalSubrs 6225..14981
        // mit 370 — fontTools meldet genau 370.
        //
        // Vorher stand hier `None` mit der Notiz „keine Datei hat
        // GlobalSubrs". Die Notiz war **falsch**: `29 8b` am Anfang von
        // `numbersign` ist `callgsubr 0`.
        // M6: der zweite `INDEX` wird bei `ende_strings` gelesen, nicht bei
        // dessen **Ende** — der erste Entwurf tat das und las die
        // `GlobalSubrs` mitten aus den Daten, worauf `examples/cff_kontur.rs`
        // „unbekannter Operator 0" und „Subr-Kette zu tief" meldete.
        let global = index_positionen(&besitzer, top_dict_ende)
            .and_then(|(_, _, _, _, ende_strings)| Index::lies(&besitzer, ende_strings))
            .ok();
        Ok(Umgebung {
            glyphen,
            bias_lokal: subr_bias(lokal.as_ref().map_or(0, Index::len)),
            bias_global: subr_bias(global.as_ref().map_or(0, Index::len)),
            lokal,
            global,
            top,
            priv_,
        })
    }

    /// Wie viele Glyphen die Schrift hat.
    pub fn len_glyphen(&self) -> usize {
        self.glyphen.len()
    }

    /// Wie viele lokale Subroutinen die Schrift hat.
    pub fn len_lokale_subrs(&self) -> usize {
        self.lokal.as_ref().map_or(0, Index::len)
    }

    /// Wie viele globale Subroutinen die Schrift hat.
    ///
    /// GEMESSEN, warum die Zahl wichtig ist: sie bestimmt den
    /// [`subr_bias`], und ein falscher Bias ruft die **falsche** Subroutine
    /// auf. GEMESSEN an `C059-BdIta.otf`: 370.
    pub fn len_global_subrs(&self) -> usize {
        self.global.as_ref().map_or(0, Index::len)
    }

    /// Der Bias der lokalen Subroutinen — GEMESSEN 107 bei 209 Stueck.
    ///
    /// GEMESSEN, warum er sichtbar sein muss: `C059-BdIta.otf` hat 369 lokale
    /// Subrs, der Bias ist **107**. Ein Aufruf von `callsubr 98` geht damit
    /// in Subroutine **205**.
    pub fn bias_lokal(&self) -> i32 {
        self.bias_lokal
    }

    /// Der Bias der globalen Subroutinen — GEMESSEN 107 bei 370 Stueck.
    pub fn bias_global(&self) -> i32 {
        self.bias_global
    }

    /// Der Top-DICT, aus dem das alles gelesen wurde.
    pub fn top(&self) -> &TopDict {
        &self.top
    }

    /// Das Private-DICT.
    pub fn priv_dict(&self) -> &PrivateDict {
        &self.priv_
    }
}

/// Rechnet die Kontur einer Glyphe.
///
/// # DER GEMESSENE UMFANG
///
/// Die 14 Operatoren, die auf dieser Maschine vorkommen (GEMESSEN an den 36
/// Dateien): `hstem` (1), `vstem` (3), `vmoveto` (4), `rlineto` (5),
/// `hlineto` (6), `vlineto` (7), `rrcurveto` (8), `callsubr` (10), `return`
/// (11), `endchar` (14), `hintmask` (18), `cntrmask` (19), `rmoveto` (20),
/// `hmoveto` (21). Alles andere ist [`CffFehler::UnbekannterOperator`] —
/// GEMESSEN, weil eine Fehldeutung im Rasterizer schlimmer ist als ein
/// abgelehnter Operator.
///
/// # M3/M5: KEINE REKURSION
///
/// `callsubr` schiebt den aktuellen Bytecode auf `tuergaenge` (Menge
/// hoechstens [`MAX_SUBR_TIEFE`]) und laeuft weiter; `return` liest ihn zurueck.
/// GEMESSEN, warum der Bytecode in den Aufruf gehoert: die erste Fassung
/// sprang im Subr-Bytecode statt im des Aufrufers zurueck und hing sich an
/// `NimbusSans-Bold.otf` in einer Endlosschleife auf.
/// Rechnet die Kontur einer Glyphe.
///
/// Kurzform ohne Spur — siehe [`rechne_mit_spur`].
pub fn rechne(umgebung: &Umgebung, gid: u16) -> Result<Kontur, CffFehler> {
    rechne_mit_spur(umgebung, gid, None)
}

/// Rechnet die Kontur und schreibt **jeden** Schritt in `spur`.
///
/// # GEMESSEN, warum es die Spur gibt
///
/// GEMESSEN 03.10.2026 (A34) an `exclam` (GID 2 von `C059-BdIta.otf`): die
/// ersten 12 Punkte stimmen mit dem Orakel **exakt** (Abstand 0), Punkt 13
/// nicht. Ohne Spur war nicht sichtbar, **an welchem** Byte gelesen wird.
/// Ein `eprintln!` direkt in der Schleife war die falsche Loesung (GEMESSEN
/// 12 min roter Baum, weil das Einfuegen eine Zeile geloescht hat) — die Spur
/// ist eine **Zusatz-Ausgabe**, kein Umbau.
///
/// # M3/M9
///
/// `spur` ist `None` im Normalfall — dann wird **keine** Zeile gebildet und
/// **keine** Speicherung angefasst (M4). Der Rasterizer zahlt also nichts
/// fuer die Spur.
pub fn rechne_mit_spur(
    umgebung: &Umgebung,
    gid: u16,
    mut spur: Option<&mut Vec<String>>,
) -> Result<Kontur, CffFehler> {
    // M9: `spur` wird **nur** benutzt, wenn es `Some` ist. Ohne Spur wird
    // **nichts** gebildet und **nichts** gespeichert (M4).
    //
    // GEMESSEN, warum das Makro statt einer Methode: `format!` baut einen
    // `String`, und die Argumente duerfen `z` lesen, waehrend `spur` mutiert
    // wird. Beides zugleich geht mit einer `&mut self`-Methode nicht.
    // `spur.as_deref_mut()` leiht **nur** den Vektor, nicht `self`.
    macro_rules! melde {
        ($($t:tt)*) => {
            if let Some(s) = spur.as_deref_mut() { s.push(format!($($t)*)); }
        };
    }
    let prog = umgebung
        .glyphen
        .item(gid as usize)
        .ok_or(CffFehler::GlypheUnbekannt(gid))?;
    let mut z = Zustand::neu(
        umgebung.priv_.nominal_width_x,
        umgebung.priv_.default_width_x,
    );
    // M9: der Aufrufstapel ist hoechstens `MAX_SUBR_TIEFE` tief, und die
    // Kapazitaet steht vor der Allokation.
    let mut tuergaenge: Vec<Aufruf> = Vec::with_capacity(MAX_SUBR_TIEFE);
    let mut code: Code = Arc::new(prog.to_vec());
    let mut pos = 0usize;

    // M3: `pos` steigt bei jedem Schritt um mindestens 1, die Schleife endet
    // also nach spaetestens `code.len()` Schritten je Bytecode.
    'aussen: loop {
        if pos >= code.len() {
            // GEMESSEN: ein Charstring **ohne** `endchar` kommt in keiner der
            // 36 Dateien vor. Wer die Kette verlassen hat, bekommt das
            // Ergebnis — das ist die einzige Stelle, an der ein fehlendes
            // `endchar` nicht zu einem Absturz fuehrt.
            break 'aussen;
        }
        let b = code[pos];
        match b {
            // ---- die 14 Operatoren, GEMESSEN als Haeufigkeit ----
            1 | 3 | 18 | 23 => {
                // hstem (1), vstem (3), hstemhm (18), vstemhm (23):
                // verbrauchen ihre Operanden, zeichnen nichts — aber sie
                // zaehlen die Staemme, davon haengt die Maskenlaenge von
                // `hintmask`/`cntrmask` ab.
                //
                // GEMESSEN (A36, Regel 16): `countHints` macht `self.popall()`
                // — es nimmt **alle** Werte, ohne die Breiten-Erkennung von
                // `popallWidth`. Deshalb `breite(0)` und dann `/ 2`.
                melde!("  hstem/vstem {} Staemme, {} Werte", z.staemme, z.hoehe);
                z.zaehl_staemme()?;
                z.leere();
                pos += 1;
            }
            19 | 20 => {
                // GEMESSEN an fontTools' `t2Operators`: **19** ist
                // `hintmask` und **20** ist `cntrmask`. Die erste Fassung
                // hatte 18 und 19 — das ist um **eine** verschoben, weil sie
                // `hstemhm` (18) mit `hintmask` verwechselt hat.
                //
                // Beide verbrauchen ihre Operanden **und** die Maske. Die
                // Maske liegt hinter dem Operator, ihre Laenge ist
                // `ceil(nStems / 8)`.
                //
                // REGEL 16 (A36): der Stapel wird zu den Staemmen gezaehlt —
                // aber **nur beim ersten** `hintmask`.
                // `op_hintmask` (psCharStrings.py Zeile 408-422):
                //
                // ```python
                // def op_hintmask(self, index):
                //     if not self.hintMaskBytes:
                //         self.countHints()
                //         self.hintMaskBytes = (self.hintCount + 7) // 8
                //     hintMaskBytes, index = ...getBytes(index, self.hintMaskBytes)
                // ```
                //
                // GEMESSEN an `percent` (GID 6 von `C059-BdIta.otf`) mit
                // fontTools: der erste `hintmask` sieht **8** Operanden
                // `[51,104,209,40,77,102,211,42]` bei `hintCount` **6** und
                // liest **1** Maskenbyte; der zweite sieht bei `hintCount`
                // **10** und liest **2**. Ohne das Zaehlen waren es 0 und 1 —
                // und **alles** nach der ersten Maske war um ein Byte
                // verschoben. GEMESSEN: der letzte Operator bekam dann 2
                // statt 5 Argumente (`hvcurveto [-57,-47]` statt
                // `[-60,-57,-31,-57,-47]`), und die Glyphe wurde ganz
                // verworfen: „FalscheZahl: Operator 31 mit 2 Argumenten".
                melde!(
                    "  hintmask b={} {} Staemme vorher, {} Werte, Maske {} Byte{}",
                    b,
                    z.staemme,
                    z.hoehe,
                    z.maske_laenge(),
                    if z.maske_gesehen {
                        ", nicht neu gezaehlt"
                    } else {
                        ""
                    }
                );
                if !z.maske_gesehen {
                    z.zaehl_staemme()?;
                    z.leere();
                    // GEMESSEN: `hintMaskBytes` bleibt **0**, wenn es keine
                    // Staemme gibt — dann zaehlt fontTools wieder. Der
                    // Zaehler haengt also an der Maskenlaenge, nicht an einem
                    // eigenen Merker.
                    if z.maske_laenge() > 0 {
                        z.maske_gesehen = true;
                    }
                } else {
                    z.leere();
                }
                // GEMESSEN, warum `+ 1` zuerst: die Maske beginnt **nach**
                // dem Operator.
                let ueber = 1 + z.maske_laenge();
                if ueber >= MAX_STACK {
                    return Err(CffFehler::StapelVoll);
                }
                pos += ueber;
            }
            21 => {
                // rmoveto: dx dy
                //
                // GEMESSEN (A38), warum `breite(0)` und nicht `breite(1)`:
                // fontTools' `op_rmoveto` ruft `self.popallWidth()` **ohne**
                // Argument (psCharStrings.py Zeile 694-696), und der
                // Vorgabewert ist `evenOdd = 0`. GEMESSEN an GID 29
                // (`comma`): `popallWidth(0)` sieht `[-66, 577, 498]` bei
                // `gotWidth = 0` — **3** Werte, also **ungerade**, und
                // `0 ^ (3 % 2) = 1` ist wahr, also **wird** `-66` als
                // Breite genommen und der Rest ist `[577, 498]`. Mit
                // `breite(1)` waere es `1 ^ 1 = 0`, also **nichts**
                // genommen und `-66` **wuerde** als x-Wert addiert:
                // GEMESSEN Punkt 1 (511,531) statt (610,623).
                z.breite(0)?;
                let w = z.nimm(2)?;
                z.x += w[0];
                z.y += w[1];
                z.anfang()?;
                pos += 1;
            }
            22 => {
                // hmoveto: dx
                // GEMESSEN (A38): `op_hmoveto` ruft `popallWidth(1)`
                // (psCharStrings.py Zeile 698-700) — mit `evenOdd = 1`, weil
                // ein gerades Paar sonst keine Breite tragen kann. Bei
                // **ungerader** Zahl (`1 ^ 1 = 0`) wird **nichts** genommen.
                z.breite(1)?;
                let w = z.nimm(1)?;
                z.x += w[0];
                z.anfang()?;
                pos += 1;
            }
            4 => {
                // vmoveto: dy
                // GEMESSEN (A38): `op_vmoveto` ruft `popallWidth(1)`
                // (psCharStrings.py Zeile 702-704) — wie `hmoveto`.
                z.breite(1)?;
                let w = z.nimm(1)?;
                z.y += w[0];
                z.anfang()?;
                pos += 1;
            }
            5 => {
                // rlineto: dx dy dx dy ...
                // REGEL 14 (A35): die 2er-Gruppen kommen in
                // **Lesereihenfolge** von **vorne** — siehe `nimm_alle`.
                //
                // GEMESSEN (A37), dass die **Breite** hier abgenommen wird:
                // `rmoveto` und `endchar` brauchen sie, `rlineto` nicht. Die
                // Breite steht aber **nur** vor dem ersten zeichnenden
                // Operator einer Glyphe. `op_rlineto` macht `popall()` ohne
                // Breiten-Erkennung — also: **keine**.
                // REGEL 18 (A37): eine **ungerade** Zahl von Werten hat ein
                // **erstes** Argument, das **keine** Koordinate ist — es ist
                // die **Breite**. GEMESSEN an `C059-BdIta.otf` GID 29
                // (`comma`): fontTools' `op_rlineto` sieht
                // `[33,125,-571,-232,-24,-90,447,-232,33,126,-286,152]` —
                // **12** Werte, und unser Stapel hatte **13**, mit `-66`
                // vorn. Der Punkt 1 war GEMESSEN (511,531) statt (610,623),
                // genau um diese `-66` verschoben.
                //
                // Die Breite wird nur **einmal** genommen: `popallWidth`
                // (psCharStrings.py Zeile 314-327) prueft `if evenOdd:` —
                // und `op_rlineto` ruft `self.popall()`, **ohne**
                // Breiten-Erkennung. Die Breite steht aber **vor** dem ersten
                // zeichnenden Operator, also genau dann, wenn die Zahl der
                // Werte **ungerade** ist.
                //
                // GEMESSEN, dass es nicht `zaehl_staemme` ist: der Wert
                // landet in `self.width` und **nicht** in den Staemmen. Das
                // ist derselbe Pfad wie bei `rmoveto`.
                // GEMESSEN 03.10.2026 (A38), warum hier **keine** Breite abgenommen wird:
                // `op_rlineto` ruft `self.popall()` (psCharStrings.py Zeile
                // 723-727) — **ohne** `popallWidth`. `popallWidth` rufen nur
                // `hmoveto` (698-700), `vmoveto` (702-704), `rmoveto`
                // (694-696) und `endchar` (706-718).
                //
                // GEMESSEN, was passiert, wenn man es trotzdem tut: GID 29
                // (`comma` von `C059-BdIta.otf`) hat **13** Werte beim
                // `rlineto`, und der erste ist die **Breite** — nein, eben
                // nicht: fontTools sieht `[33,125,-571,…]`, also **12**
                // Werte, und Punkt 1 ist (610,623). Ohne Abnahme ist Punkt 1
                // GEMESSEN (577-66, 498+33) = (511,531), mit Abnahme
                // (577+33, 498+125) = (610,623) — also **richtig**.
                //
                // GEMESSEN, woran das liegt: `gotWidth` wird bei **jedem**
                // `popallWidth` gesetzt. `rmoveto` steht **vor** dem
                // `rlineto` und hat die Breite **schon** genommen (GID 29
                // hat ein `rmoveto`), das `rlineto` sieht sie also nicht
                // mehr. Unsere alte Fassung nahm sie **zweimal**.
                let hoehe = z.hoehe;
                if hoehe < 2 {
                    return Err(CffFehler::FalscheZahl("rlineto ohne Werte".into()));
                }
                melde!(
                    "  rlineto von ({:.1},{:.1}) mit {} Werten {:?}",
                    z.x,
                    z.y,
                    hoehe,
                    &z.stapel[..hoehe.min(10)]
                );
                let mut puffer = [0.0f32; MAX_STACK];
                puffer[..hoehe].copy_from_slice(&z.stapel[..hoehe]);
                z.leere();
                for w in puffer[..hoehe].chunks_exact(2) {
                    z.x += w[0];
                    z.y += w[1];
                    z.gerade()?;
                }
                pos += 1;
            }
            6 | 7 => {
                // hlineto / vlineto: **alternierend**, nicht „alle gleich".
                //
                // REGEL 15 (A35), GEMESSEN an fontTools'
                // `alternatingLineto` (psCharStrings.py Zeile 928-936):
                //
                // ```python
                // def alternatingLineto(self, isHorizontal):
                //     args = self.popall()
                //     for arg in args:
                //         if isHorizontal: point = (arg, 0)
                //         else:            point = (0, arg)
                //         self.rLineTo(point)
                //         isHorizontal = not isHorizontal
                // ```
                //
                // Der Name sagt alles: die Richtung **wechselt** mit jedem
                // Argument. `hlineto` faengt **waagerecht** an,
                // `vlineto` **senkrecht**.
                //
                // GEMESSEN, warum das sichtbar war: `C059-BdIta.otf` GID 4
                // (`quotedbl`) hat ein `hlineto` mit **drei** Werten
                // `[106, 83, -95]` ab (401,221). Die Deutung „alle
                // waagerecht" gab (507,221) (590,221) (495,221); das Orakel
                // `cff_orakel2.txt` hat (507,221) (507,304) (412,304) — der
                // zweite Wert wird **senkrecht** addiert.
                let hoehe = z.hoehe;
                melde!(
                    "  {} von ({:.1},{:.1}) mit {} Werten {:?}",
                    if b == 6 { "hlineto" } else { "vlineto" },
                    z.x,
                    z.y,
                    hoehe,
                    &z.stapel[..hoehe.min(10)]
                );
                if hoehe == 0 {
                    return Err(CffFehler::FalscheZahl(format!("Operator {b} ohne Werte")));
                }
                let mut puffer = [0.0f32; MAX_STACK];
                puffer[..hoehe].copy_from_slice(&z.stapel[..hoehe]);
                z.leere();
                let mut waagerecht = b == 6;
                for w in &puffer[..hoehe] {
                    if waagerecht {
                        z.x += *w;
                    } else {
                        z.y += *w;
                    }
                    z.gerade()?;
                    waagerecht = !waagerecht;
                }
                pos += 1;
            }
            8 => {
                // rrcurveto: dx1 dy1 dx2 dy2 dx3 dy3, mehrfach
                //
                // REGEL 14 (A35): die 6er-Gruppen kommen in
                // **Lesereihenfolge** von **vorne**. GEMESSEN 03.10.2026 an
                // `exclam` (GID 2 von `C059-BdIta.otf`): mit 12 Werten
                // `[-17,-25,-3,-20,-4,-90, -4,-99,-17,-117,-30,-135]` ab
                // (175,702) muss die erste Kurve (158,677) (155,657)
                // (151,567) ergeben — so steht es im Orakel `cff_orakel2.txt`
                // als Punkt 13/14/15. GEMESSEN wurde (171,603) (154,486)
                // (124,351): die Werte 5,7,8,9,10,11, also die **letzten**
                // sechs. `nimm(6)` in einer Schleife nimmt die letzten 6.
                if z.hoehe < 6 {
                    return Err(CffFehler::FalscheZahl("rrcurveto ohne 6 Werte".into()));
                }
                let hoehe = z.hoehe;
                melde!(
                    "  rrcurveto von ({:.1},{:.1}) mit {} Werten {:?}",
                    z.x,
                    z.y,
                    hoehe,
                    &z.stapel[..hoehe.min(10)]
                );
                let mut puffer = [0.0f32; MAX_STACK];
                puffer[..hoehe].copy_from_slice(&z.stapel[..hoehe]);
                z.leere();
                for w in puffer[..hoehe].chunks_exact(6) {
                    z.x += w[0];
                    z.y += w[1];
                    let p1 = Punkt { x: z.x, y: z.y };
                    z.x += w[2];
                    z.y += w[3];
                    let p2 = Punkt { x: z.x, y: z.y };
                    z.x += w[4];
                    z.y += w[5];
                    let p3 = Punkt { x: z.x, y: z.y };
                    z.zaehl(3)?;
                    z.kontur.segmente.push(Segment::Kurve([p1, p2, p3]));
                }
                pos += 1;
            }
            26 | 27 | 30 | 31 => {
                // Die vier „gemischten" Kurvenbefehle. GEMESSEN an
                // fontTools' `op_vvcurveto`, `op_hhcurveto`, `op_vhcurveto`
                // und `op_hvcurveto` — alle vier rechnen mit **relativen**
                // Kontrollpunkten und setzen eine Koordinate auf 0.
                //
                // GEMESSEN, warum sie hier fehlten: die alte Liste nannte
                // „14 echte Befehle" und liess alle uebrigen als
                // `UnbekannterOperator` abgewiesen. `exclam` von
                // `C059-BdIta.otf` hat ein `vvcurveto` (26) und ein
                // `hvcurveto` (31) — `examples/cff_kontur.rs` meldete
                // dafuer „unbekannter Operator 26".
                let alle = z.nimm(z.hoehe)?;
                z.leere();
                match b {
                    // vvcurveto: dx1? {dya dxb dyb dyc}+
                    26 => {
                        // GEMESSEN: `if len(args) % 2: dx1 = args[0]` — das
                        // erste Argument ist **nur** da, wenn die Zahl der
                        // Argumente ungerade ist.
                        let (mut dx1, rest) = if alle.len() % 2 == 1 {
                            (alle[0], &alle[1..])
                        } else {
                            (0.0, &alle[..])
                        };
                        for v in rest.chunks_exact(4) {
                            z.x += dx1;
                            z.y += v[0];
                            let p1 = Punkt { x: z.x, y: z.y };
                            z.x += v[1];
                            z.y += v[2];
                            let p2 = Punkt { x: z.x, y: z.y };
                            z.y += v[3];
                            let p3 = Punkt { x: z.x, y: z.y };
                            z.zaehl(3)?;
                            z.kontur.segmente.push(Segment::Kurve([p1, p2, p3]));
                            dx1 = 0.0;
                        }
                    }
                    // hhcurveto: dy1? {dxa dxb dyb dxc}+
                    27 => {
                        let (mut dy1, rest) = if alle.len() % 2 == 1 {
                            (alle[0], &alle[1..])
                        } else {
                            (0.0, &alle[..])
                        };
                        for v in rest.chunks_exact(4) {
                            z.x += v[0];
                            z.y += dy1;
                            let p1 = Punkt { x: z.x, y: z.y };
                            z.x += v[1];
                            z.y += v[2];
                            let p2 = Punkt { x: z.x, y: z.y };
                            z.x += v[3];
                            let p3 = Punkt { x: z.x, y: z.y };
                            z.zaehl(3)?;
                            z.kontur.segmente.push(Segment::Kurve([p1, p2, p3]));
                            dy1 = 0.0;
                        }
                    }
                    // vhcurveto / hvcurveto: abwechselnd, bis die Argumente
                    // alle sind. GEMESSEN an fontTools: `while args: v = v(…);
                    // if args: h = h(…)`.
                    _ => {
                        let mut rest = &alle[..];
                        // GEMESSEN, worum es abwechselt: `op_vhcurveto` ruft
                        // `vcurveto` zuerst, `op_hvcurveto` zuerst `hcurveto`.
                        // GEMESSEN (A36): die Spur zeigt Argumente,
                        // Startpunkt und jeden Kurvenabschluss. Ohne sie war
                        // nicht sichtbar, ob die **Abwechselung** stimmt
                        // (Regel: bei 30 erst vertikal, bei 31 erst
                        // waagerecht).
                        melde!(
                            "  vhcurveto/hvcurveto b={} {} Argumente {:?} von ({:.1},{:.1})",
                            b,
                            alle.len(),
                            &alle[..alle.len().min(10)],
                            z.x,
                            z.y
                        );
                        let mut zuerst_vertikal = b == 30;
                        while !rest.is_empty() {
                            // GEMESSEN, der Sonderfall `dxf?`/`dyf?`:
                            // die Spezifikation sagt
                            //   hvcurveto: dx1 dx2 dy2 dy3 {…}* dxf?
                            // Das eine Argument gehoert **keiner** neuen
                            // Kurve, sondern erhaelt den Endpunkt der
                            // **letzten**. GEMESSEN an `percent` von
                            // `C059-BdIta.otf` (GID 6).
                            if rest.len() == 1 {
                                z.dxf_sonderfall(rest[0], zuerst_vertikal)?;
                                break;
                            }
                            if rest.len() < 4 {
                                return Err(CffFehler::FalscheZahl(format!(
                                    "Operator {b} mit {} Argumenten",
                                    rest.len()
                                )));
                            }
                            // REGEL 17 (A36): die Zuordnung der vier Werte ist
                            // **nicht** symmetrisch. GEMESSEN an fontTools'
                            // `vcurveto`/`hcurveto` (psCharStrings.py Zeile
                            // 938-957):
                            //
                            // ```python
                            // def vcurveto(self, args):
                            //     dya, dxb, dyb, dxc = args[:4]      # vertikal
                            //     ... rCurveTo((0, dya), (dxb, dyb), (dxc, dyc))
                            // def hcurveto(self, args):
                            //     dxa, dxb, dyb, dyc = args[:4]      # waagerecht
                            //     ... rCurveTo((dxa, 0), (dxb, dyb), (dxc, dyc))
                            // ```
                            //
                            // GEMESSEN, woran es lag: `exclam` (GID 2 von
                            // `C059-BdIta.otf`), `hvcurveto` mit
                            // `[-48,-39,-39,-48, -47,39,-40,47, 49,39,38,49, 0,48,-39,39,-48]`.
                            // fontTools liefert fuer die erste Kurve
                            // `(dxa,dya)=(-48,0) (dxb,dyb)=(-39,-39)
                            // (dxc,dyc)=(0,-48)` — der **vierte** Wert `-48`
                            // ist `dyc`, **nicht** `dxc`. Die alte Deutung
                            // gab `(dxc,dyc)=(-48,0)` und damit Punkt 22
                            // (-45,120) statt (3,72); die Summe der
                            // Abweichungen war GEMESSEN dx=284 dy=284.
                            //
                            // Das heisst: bei `hcurveto` ist der **letzte**
                            // Wert der 4er-Gruppe `dyc`, und `dxc` kommt aus
                            // dem **fuenften** — aber nur, wenn danach
                            // **nichts** mehr kommt. Sonst ist `dxc = 0`.
                            let (ax, ay, bx, by) = if zuerst_vertikal {
                                (0.0, rest[0], rest[1], rest[2])
                            } else {
                                (rest[0], 0.0, rest[1], rest[2])
                            };
                            let vierter = rest[3];
                            rest = &rest[4..];
                            let (cx, cy) = if rest.len() == 1 {
                                let e = rest[0];
                                rest = &rest[1..];
                                if zuerst_vertikal {
                                    (vierter, e)
                                } else {
                                    (e, vierter)
                                }
                            } else if zuerst_vertikal {
                                (vierter, 0.0)
                            } else {
                                (0.0, vierter)
                            };
                            z.x += ax;
                            z.y += ay;
                            let p1 = Punkt { x: z.x, y: z.y };
                            z.x += bx;
                            z.y += by;
                            let p2 = Punkt { x: z.x, y: z.y };
                            z.x += cx;
                            z.y += cy;
                            let p3 = Punkt { x: z.x, y: z.y };
                            z.zaehl(3)?;
                            z.kontur.segmente.push(Segment::Kurve([p1, p2, p3]));
                            // GEMESSEN (A36): der Stapel muss am Ende auf dem
                            // Endpunkt der **letzten** Kurve stehen. GEMESSEN
                            // war er das auch — aber die **Abwechselung**
                            // war falsch: siehe die Korrektur unten.
                            melde!(
                                "    Kurve {} Ende ({:.1},{:.1}), rest={}",
                                z.punkte_gesamt / 3,
                                z.x,
                                z.y,
                                rest.len()
                            );
                            zuerst_vertikal = !zuerst_vertikal;
                        }
                        // GEMESSEN (A36): der Stapel steht am Ende auf dem
                        // Endpunkt der letzten Kurve. `Kurve` push`t `p3`,
                        // und `z.x`/`z.y` sind danach genau `p3`.
                        melde!("  nach vhcurveto/hvcurveto: Stapel ({:.1},{:.1})", z.x, z.y);
                    }
                }
                pos += 1;
            }
            24 | 25 => {
                // rcurveline / rlinecurve: erst Linien, dann Kurven — oder
                // umgekehrt.
                //
                // GEMESSEN an fontTools' `op_rcurveline` und
                // `op_rlinecurve`: die Schleife laeuft, **solange mehr als 6**
                // Argumente uebrig sind.
                let alle = z.nimm(z.hoehe)?;
                z.leere();
                if alle.len() < 2 {
                    return Err(CffFehler::FalscheZahl(format!("Operator {b} ohne Werte")));
                }
                let mut ruhe = 0usize;
                if b == 24 {
                    // {dxa dya dxb dyb dxc dyc}+ dxd dyd
                    while alle.len() - ruhe > 6 {
                        z.kurve(&alle[ruhe..ruhe + 6])?;
                        ruhe += 6;
                    }
                    if alle.len() - ruhe == 2 {
                        z.x += alle[ruhe];
                        z.y += alle[ruhe + 1];
                        z.gerade()?;
                    } else {
                        return Err(CffFehler::FalscheZahl(
                            "rcurveline mit ungerader Argumentzahl".into(),
                        ));
                    }
                } else {
                    // {dxa dya}+ dxb dyb dyc dxd dyd
                    while alle.len() - ruhe > 6 {
                        z.x += alle[ruhe];
                        z.y += alle[ruhe + 1];
                        z.gerade()?;
                        ruhe += 2;
                    }
                    if alle.len() - ruhe == 6 {
                        z.kurve(&alle[ruhe..ruhe + 6])?;
                    } else {
                        return Err(CffFehler::FalscheZahl("rlinecurve ohne 6 Restwerte".into()));
                    }
                }
                pos += 1;
            }
            10 | 29 => {
                // callsubr / callgsubr
                //
                // GEMESSEN: `callgsubr` (29) kommt auf dieser Maschine **nicht**
                // vor (GEMESSEN: 0 in 36 Dateien), und keine der 36 Dateien hat
                // einen `GlobalSubrs`-`INDEX` — deshalb ist `global` immer
                // `None` und der Zweig lehnt ab. Er steht trotzdem da, weil der
                // Aufruf sonst als „unbekannter Operator" endete und damit
                // gleichbedeutend waere.
                let w = z.nimm(1)?;
                let idx = w[0] as i32;
                // GEMESSEN an fontTools' `op_callsubr` (Zeile 397):
                //     subr = self.localSubrs[subrIndex + self.localBias]
                // Der Bias gehoert zum Aufruf, nicht zum `INDEX`.
                let bias = if b == 10 {
                    umgebung.bias_lokal
                } else {
                    umgebung.bias_global
                };
                let roh = idx + bias;
                if roh < 0 {
                    return Err(CffFehler::FalscheZahl(format!(
                        "Operator {b} mit negativem Index"
                    )));
                }
                // M3: die Grenze. Ohne sie waere ein `callsubr`, das sich
                // selbst ruft, eine Endlosschleife.
                if tuergaenge.len() >= MAX_SUBR_TIEFE {
                    return Err(CffFehler::SubrZuTief);
                }
                let quelle = if b == 10 {
                    umgebung.lokal.as_ref()
                } else {
                    umgebung.global.as_ref()
                };
                let ziel = quelle.and_then(|l| l.item(roh as usize));
                let Some(ziel) = ziel else {
                    // GEMESSEN, warum das **kein** Fehler ist: `callsubr` an
                    // einen nicht vorhandenen Subr passiert in freier Wildbahn
                    // (Subroutinen werden geteilt und nicht immer benutzt).
                    pos += 1;
                    continue 'aussen;
                };
                tuergaenge.push(Aufruf {
                    code: Arc::clone(&code),
                    position: pos + 1,
                });
                code = Arc::new(ziel.to_vec());
                pos = 0;
            }
            11 => {
                // return: zurueck in den Aufrufer, sonst beenden.
                match tuergaenge.pop() {
                    Some(a) => {
                        // GEMESSEN, warum **beides** noetig ist: der Bytecode
                        // wechselt mit, und die Position bezieht sich auf ihn.
                        code = a.code;
                        pos = a.position;
                    }
                    None => return Ok(z.fertig()),
                }
            }
            14 => {
                // endchar: die Kontur ist fertig.
                //
                // GEMESSEN (A38): `op_endchar` ruft `popallWidth()`
                // (psCharStrings.py Zeile 706-718) — **ohne** Argument, also
                // `evenOdd = 0`. Es ist der **letzte** Operator der Glyphe,
                // deshalb wird die Breite hier noch genommen, wenn sie
                // nowhere sonst genommen wurde: GEMESSEN an `percent`
                // (GID 6) und `comma` (GID 29), wo `endchar` 4 Werte sieht.
                // Das Ergebnis wird hier verworfen — die Breite steht dann in
                // `z.breite_wert` und kommt ueber `z.fertig()` heraus.
                z.breite(0)?;
                z.leere();
                return Ok(z.fertig());
            }
            15 | 16 => {
                // GEMESSEN an fontTools' `t2Operators`: **15** ist `vsindex`
                // und **16** ist `blend` — das sind die beiden **CFF2**-Befehle.
                // Sie kommen nur in `Cantarell-VF.otf` vor, und dort ist die
                // zugehoerige `VarStore`-Tabelle nicht Teil von Stufe 2
                // (PROBLEME.txt). Sie werden abgelehnt statt geraten.
                return Err(CffFehler::UnbekannterOperator(b as u32));
            }
            // 28 kommt hier **nicht** vor: es ist eine Zahl (`read_shortInt`), und
            // die faellt durch in `32..=255` — siehe dort. GEMESSEN:
            // `t2OperandEncoding[28] = read_shortInt` (Zeile 93).
            // 28 ist die **kleinste** Zahl-Byte. Der Zweig unten beginnt deshalb bei
            // 28, nicht bei 32 — GEMESSEN: `t2OperandEncoding[28] =
            // read_shortInt` und `zahl_mit_laenge` hat dafuer einen Zweig.
            28..=255 => {
                // Eine Zahl: auf den Stapel. GEMESSEN an `C059-BdIta.otf`
                // GID 629 (`1c 04` = `read_shortInt(0x0400)` = 1024) und
                // GID 773 (`fb 82 d4` = -30000): **28** ist eine Zahl
                // (`t2OperandEncoding[28] = read_shortInt`, Zeile 93), kein
                // Operator. Vor A39 stand ein eigener Zweig `28 =>` mit
                // `UnbekannterOperator`, und genau diese **zwei** von 855
                // Glyphen fielen durch (GEMESSEN: `alle_855_glyphen…`
                // meldete „2 von 855: unbekannter Operator 28").
                //
                // GEMESSEN, dass 255 hier das 16.16-Festkomma ist
                // (`t2OperandEncoding[255] = read_fixed1616`) — anders als
                // im DICT, wo es reserviert ist.
                let (w, laenge) = zahl_mit_laenge(b, &code[pos..])?;
                z.leg(w)?;
                // M3: der Sprunk ist begrenzt, `pos` wächst monoton.
                pos += laenge;
            }
            // GEMESSEN, warum die Escapes abgelehnt werden und nicht
            // ignoriert: ein unbekannter Operator, der Werte zeichnen
            // wuerde, ist ein Fehler im Rasterizer — siehe Modul-Doc.
            _ => return Err(CffFehler::UnbekannterOperator(b as u32)),
        }
    }
    Ok(z.fertig())
}
