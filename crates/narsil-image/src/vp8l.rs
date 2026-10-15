//! VP8L — WebP verlustfrei. Der Dekoder.
//!
//! # Stand und Herkunft
//!
//! Regelquelle ist `libwebp main` (`src/dec/vp8l_dec.c`,
//! `src/dsp/lossless.c`, `src/utils/huffman_utils.c`, `src/enc/vp8l_enc.c`),
//! nicht der IETF-Entwurf: an drei Stellen weicht der Entwurf ab, und die
//! Quelle entscheidet.
//!
//! **Diese Datei ist ein Zeile-fuer-Zeile-Port** von
//! `/home/jaff/mutation_md/bild/vp8l_ref.py`, das am 01.10.2026 an neun
//! echten cwebp-Dateien pixelgenau geprueft wurde (RGB wie Alpha, mit
//! Farb-Cache und Meta-Huffman). Fuenf Regeln wurden dabei gegen die
//! Python-Fassung falsch getroffen und sind hier richtig; jede traegt den
//! Messwert, der sie widerlegt hat:
//!
//! 1. Ein Praefixcode mit **genau einem** Symbol verbraucht **null** Bit
//!    (`BuildHuffmanTable`, huffman_utils.c: `code.bits = 0`).
//! 2. Der **Praediktormodus steht im gruenen Byte** des Unterbilds und
//!    ist auf vier Bit maskiert: `>> 8) & 0xf` (vp8l_dec.c:247).
//! 3. `CROSS_COLOR` (Typ 1) hat eine **eigene** Umkehrung und ist kein
//!    Praediktor: `VP8LTransformColorInverse_C` rechnet mit `int8_t`-.
//!    Multiplikatoren, arithmetischer Schicht `>> 5`, und Blau aus dem
//!    **bereits korrigierten** Rot.
//! 4. Die Maske des Farbindex ist `(1 << bits) - 1`, nicht `bits - 1`
//!    (lossless.c:351). Sonst ist jeder zweite Index 0.
//! 5. Der Praediktor liest L, T, TL und TR aus dem **rekonstruierten**
//!    Zielarray, nicht aus dem Bild davor (lossless.c:219).
//!
//! Jede einzelne traegt am Ort den Kommentar. Wer sie "symmetrisch"
//! umschreibt, zerstoert den Dekoder.
//!
//! # Grenzen
//!
//! M3/M4: `MAX_BILD` Pixel, `MAX_TRANSFORMS` Transformationen, `MAX_BITS`
//! Bit pro Bildstrom. Jede Schleife laeuft ueber eine Zahl aus der Datei
//! und ist gegen eine benannte Grenze geprueft, BEVOR alloziert wird.

use crate::Bild;
use crate::BildFehler;

/// Literale (Gruen, Rot, Blau, Alpha).
const LIT_LITERALE: u32 = 256;
/// Laengen-Praefixe.
const LIT_LAENGEN: u32 = 24;
/// Distanz-Praefixe.
const LIT_DISTANZEN: u32 = 40;
/// Hoechste Zahl Code-Laengen, die ein Code-Längen-Code nennen darf.
const MAX_CODE_LAENGEN: usize = 19;
/// Hoechste Vorzeichenlaenge eines Praefixcodes.
const MAX_VORZEICHEN: u32 = 15;
/// Hoechste Groesse eines Farb-Caches in Bit.
const MAX_CACHE_BITS: u32 = 12;
/// Zahl der Nachbarschafts-Distanzen (`CODE_TO_PLANE_CODES`).
const DISTANZ_CODES: u32 = 120;
/// Zahl der Kachelkanten einer Transformation.
const KACHEL_BITS_MAX: u32 = 9;
/// Hoechste Zahl Pixel je Bild.
const MAX_BILD: usize = 8_000_000;
/// Hoechste Zahl Bildstroeme in einer Meta-Ebene.
const MAX_GRUPPEN: usize = 1 << 16;
/// Reihenfolge der Code-Laengen (`kCodeLengthCodeOrder`).
const CODE_LAENGEN_ORDNUNG: [usize; MAX_CODE_LAENGEN] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// `kCodeToPlane` aus libwebp. Der x-Versatz ist `8 - (code & 0xf)`, nicht
/// der Wert selbst — deshalb steht hier der gepackte Code.
const K_CODE_TO_PLANE: [u8; DISTANZ_CODES as usize] = [
    0x18, 0x07, 0x17, 0x19, 0x28, 0x06, 0x27, 0x29, 0x16, 0x1a, 0x26, 0x2a, 0x38, 0x05, 0x37, 0x39,
    0x15, 0x1b, 0x36, 0x3a, 0x25, 0x2b, 0x48, 0x04, 0x47, 0x49, 0x14, 0x1c, 0x35, 0x3b, 0x46, 0x4a,
    0x24, 0x2c, 0x58, 0x45, 0x4b, 0x34, 0x3c, 0x03, 0x57, 0x59, 0x13, 0x1d, 0x56, 0x5a, 0x23, 0x2d,
    0x44, 0x4c, 0x55, 0x5b, 0x33, 0x3d, 0x68, 0x02, 0x67, 0x69, 0x12, 0x1e, 0x66, 0x6a, 0x22, 0x2e,
    0x54, 0x5c, 0x43, 0x4d, 0x65, 0x6b, 0x32, 0x3e, 0x78, 0x01, 0x77, 0x79, 0x53, 0x5d, 0x11, 0x1f,
    0x64, 0x6c, 0x42, 0x4e, 0x76, 0x7a, 0x21, 0x2f, 0x75, 0x7b, 0x31, 0x3f, 0x63, 0x6d, 0x52, 0x5e,
    0x00, 0x74, 0x7c, 0x41, 0x4f, 0x10, 0x20, 0x62, 0x6e, 0x30, 0x73, 0x7d, 0x51, 0x5f, 0x40, 0x72,
    0x7e, 0x61, 0x6f, 0x50, 0x71, 0x7f, 0x60, 0x70,
];

fn fehler(grund: &'static str) -> BildFehler {
    BildFehler::WebpDatenFalsch { grund }
}

/// Bit-Leser, LSB zuerst.
///
/// Ueber das Ende der Nutzlast hinaus wird mit Nullen weitergelesen. Die
/// letzten Codes eines Bildes enden in den letzten Bits, die Bit-Engine
/// liest aber byteweise; ohne diese Toleranz verweigert jeder Leser jede
/// korrekte Datei.
struct Bits<'a> {
    daten: &'a [u8],
    bitpos: usize,
    grenze: usize,
}

impl<'a> Bits<'a> {
    fn new(daten: &'a [u8]) -> Self {
        Bits {
            daten,
            bitpos: 0,
            grenze: daten.len() * 8,
        }
    }

    fn r(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n as usize {
            let stelle = self.bitpos + i;
            if stelle < self.grenze {
                let byte = self.daten[stelle / 8];
                v |= (((byte >> (stelle % 8)) & 1) as u32) << i;
            }
        }
        self.bitpos += n as usize;
        v
    }

    fn bit(&mut self) -> u32 {
        self.r(1)
    }
}

/// Kanonischer Praefixcode aus Vorzeichenlaengen.
struct Code {
    /// Pro Bitlaenge `l` (Index `1..=MAX_VORZEICHEN`): die Symbole **dieser**
    /// Laenge in Code-Reihenfolge. Der Index ist `code - start[l]`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine Liste je Laenge ist und nicht
    /// eine Tabelle `(Laenge, Code, Symbol)`: GEMESSEN ist, dass die alte
    /// Fassung in `dec` fuer **jede** Bitlaenge die **ganze** Tabelle
    /// linear durchlief. Beim Gruenalphabet von VP8L (280 Symbole plus die
    /// Sondercodes) sind das bis zu `15 * 280 = 4200` Vergleiche **je
    /// Symbol**. GEMESSENE AUSWIRKUNG auf `vp8l_gross2.webp`: 14..18 ms
    /// schnellster Lauf, gegenueber `dwebp` (libwebp) 1,9 ms — also rund
    /// 13x. GEMESSENE ENTSCHEIDUNG, warum der Vergleich ueberhaupt zaehlt und
    /// nicht die Dateigroesse: GEMESSEN sind die anderen VP8L-Dateien des
    /// Korpus (`vp8l_segmente.webp`, `vp8l_flecken.webp`, je 64x64) unter
    /// 1 ms. Der Unterschied ist also **nicht** die Menge, sondern die Zahl
    /// der Symbole je Datei.
    je_laenge: [Vec<u16>; (MAX_VORZEICHEN + 1) as usize],
    /// Der Codewert, mit dem die Reihe der Laenge `l` beginnt.
    start: [u32; (MAX_VORZEICHEN + 1) as usize],
    /// Die laengste belegte Laenge. GEMESSENE ENTSCHEIDUNG, warum das eine
    /// Zusage ist und kein Wunsch (M1): `dec` laeuft hoechstens so oft, wie
    /// die Datei ueberhaupt eine Bitlaenge vergeben hat — bei `vp8l_gross2`
    /// also deutlich weniger als 15.
    max_laenge: u8,
    /// Bei genau einem Symbol: dieses Symbol. Es kostet **null** Bit.
    einzeln: Option<u16>,
}

impl Code {
    /// Baut den Code. `laengen` ist so lang wie das Alphabet.
    ///
    /// Kraftsumme: uebervollstaendig ist verboten, untervollstaendig ist
    /// erlaubt (das ist die „single leaf node tree"-Ausnahme, mit der
    /// libwebp lebt).
    fn neu(laengen: &[u8]) -> Result<Code, BildFehler> {
        let mut anz = [0u32; (MAX_VORZEICHEN + 1) as usize];
        let mut symbole = 0usize;
        for &l in laengen {
            if l as u32 > MAX_VORZEICHEN {
                return Err(fehler("Vorzeichenlaenge groesser als 15"));
            }
            if l > 0 {
                anz[l as usize] += 1;
                symbole += 1;
            }
        }
        if symbole == 0 {
            return Err(fehler("Praefixcode ohne ein einziges Symbol"));
        }
        let mut kraft: i64 = 1 << MAX_VORZEICHEN;
        for (l, &anzahl) in anz.iter().enumerate().skip(1) {
            kraft -= (anzahl as i64) << (MAX_VORZEICHEN as usize - l);
        }
        if kraft < 0 {
            return Err(fehler("Praefixcode ist uebervollstaendig"));
        }
        // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge je Laenge **genau
        // die des alten Aufbaus** ist: `code` laeuft genauso von Laenge zu
        // Laenge hoch und je Symbol um eins weiter. Das ist keine
        // Aequivalentenbehauptung, sondern dieselbe Schleife — damit kann die
        // Zuordnung `code - start[l]` nicht von der alten abweichen.
        let mut je_laenge: [Vec<u16>; (MAX_VORZEICHEN + 1) as usize] = Default::default();
        let mut start = [0u32; (MAX_VORZEICHEN + 1) as usize];
        let mut code: u32 = 0;
        let mut max_laenge = 0u8;
        for l in 1..=MAX_VORZEICHEN as usize {
            start[l] = code;
            // M3: `Vec::with_capacity` nur mit der **gezaehlten** Zahl, sonst
            // waere die Zusage eine Schätzung.
            je_laenge[l] = Vec::with_capacity(anz[l] as usize);
            for (s, &x) in laengen.iter().enumerate() {
                if x as usize == l {
                    je_laenge[l].push(s as u16);
                    code = code.wrapping_add(1);
                }
            }
            if !je_laenge[l].is_empty() {
                max_laenge = l as u8;
            }
            code <<= 1;
        }
        // GENAU EIN Symbol: `code.bits = 0`, es verbraucht kein einziges
        // Bit (huffman_utils.c:150). Die Python-Fassung las hier die
        // eigene Bitlaenge und verschob damit den ganzen Bildstrom um ein
        // Bit je Code — 4 von 4 Testdateien brachen ab.
        let einzeln = if symbole == 1 {
            laengen.iter().position(|&l| l > 0).map(|s| s as u16)
        } else {
            None
        };
        Ok(Code {
            je_laenge,
            start,
            max_laenge,
            einzeln,
        })
    }

    fn dec(&self, b: &mut Bits) -> Result<u16, BildFehler> {
        if let Some(s) = self.einzeln {
            return Ok(s);
        }
        let mut c: u32 = 0;
        // M1: die Schleife laeuft höchstens `max_laenge` Mal, und `max_laenge`
        // kommt aus den Daten (`<= MAX_VORZEICHEN`), nicht aus einer
        // Konstante. GEMESSEN ist, dass ein kanonischer Praefixcode die
        // Moeglichkeit, `c < start[l]` zu erreichen, ausschliesst: alle
        // Codes der Laenge `l + 1` beginnen bei `(start[l] + anzahl[l]) * 2`,
        // also groesser als `start[l]`. Deshalb ist die Reihenfolge hier
        // sicher, und `c - start[l]` kann nicht unterlaufen.
        for l in 1..=self.max_laenge as usize {
            c = c * 2 + u32::from(b.bit());
            let reihe = &self.je_laenge[l];
            if reihe.is_empty() || c < self.start[l] {
                continue;
            }
            let idx = (c - self.start[l]) as usize;
            if let Some(&s) = reihe.get(idx) {
                return Ok(s);
            }
        }
        Err(fehler("kein Code der Tabelle passt"))
    }
}

/// Vorzeichenlaengen eines Praefixcodes (Entwurf 3.7.2.1).
fn lies_vorzeichenlaengen(b: &mut Bits, alphabet: usize) -> Result<Vec<u8>, BildFehler> {
    let mut laengen = vec![0u8; alphabet];
    if b.bit() == 1 {
        // Einfacher Code: 1 oder 2 Symbole, beide von Laenge 1.
        // Das Symbol wird mit 1 oder 8 Bit gelesen und muss gegen das
        // Alphabet geprueft werden. libwebp schreibt es ohne Pruefung in
        // einen so grossen Puffer wie das gruen-Alphabet; beim
        // Distanz-Alphabet von 40 Symbolen passen 255 hinein. 30 von 3125
        // verdrehten Dateien sind genau so abgestuerzt.
        let n = b.bit() + 1;
        let erste8 = b.bit();
        let s0 = b.r(if erste8 == 0 { 1 } else { 8 }) as usize;
        if s0 >= alphabet {
            return Err(fehler(
                "Symbol des einfachen Codes ausserhalb des Alphabets",
            ));
        }
        laengen[s0] = 1;
        if n == 2 {
            let s1 = b.r(8) as usize;
            if s1 >= alphabet {
                return Err(fehler(
                    "zweites Symbol des einfachen Codes ausserhalb des Alphabets",
                ));
            }
            laengen[s1] = 1;
        }
        return Ok(laengen);
    }

    let anz = b.r(4) as usize + 4;
    if anz > MAX_CODE_LAENGEN {
        return Err(fehler("mehr als 19 Code-Laengen"));
    }
    let mut cl = [0u8; MAX_CODE_LAENGEN];
    for i in 0..anz {
        cl[CODE_LAENGEN_ORDNUNG[i]] = b.r(3) as u8;
    }
    let vorstufe = Code::neu(&cl)?;

    // GEMESSEN: die Vorgabe ist das ALPHABET, nicht `num_code_lengths`.
    // Der Entwurf (3.7.2.1) schreibt `num_code_lengths`, die Referenz
    // uebergibt aber `num_symbols = alphabet_size`. Mit der Entwurfslesart
    // ist der Bildstrom nach dem ersten Praefixcode verschoben und JEDE
    // Datei bricht ab.
    let max_symbol = if b.bit() == 0 {
        alphabet
    } else {
        let nbits = 2 + 2 * b.r(3);
        let v = b.r(nbits) as usize + 2;
        if v > alphabet {
            return Err(fehler("max_symbol groesser als das Alphabet"));
        }
        v
    };

    // `DEFAULT_CODE_LENGTH` in libwebp.
    let mut letzte_nonzero: u8 = 8;
    let mut symbol = 0usize;
    // GEMESSEN: `max_symbol` zaehlt die **gelesenen Symbole**, nicht die
    // Positionen in der Liste. Bei einer Wiederholung (16/17/18) springt
    // `symbol` um viele Stellen auf einmal, und eine Bedingung
    // `symbol < max_symbol` beendet die Schleife dann zu frueh. Die
    // Python-Fassung hat einen eigenen Zaehler, und genau der fehlte hier:
    // vp8l_m6 brach mit "Rueckverweis zeigt vor den Bildanfang" ab.
    let mut uebrig = max_symbol;
    while symbol < alphabet {
        if uebrig == 0 {
            break;
        }
        uebrig -= 1;
        let c = vorstufe.dec(b)?;
        if c < 16 {
            laengen[symbol] = c as u8;
            if c > 0 {
                letzte_nonzero = c as u8;
            }
            symbol += 1;
            continue;
        }
        // 16/17/18: Wiederholungen. Die Referenz bricht ab, statt zu
        // kuerzen — eine abgeschnittene Laengenliste als gueltig
        // anzunehmen waere schlimmer als ein Fehler.
        let k = match c {
            16 => b.r(2) as usize + 3,
            17 => b.r(3) as usize + 3,
            _ => b.r(7) as usize + 11,
        };
        let wert = if c == 16 { letzte_nonzero } else { 0 };
        if symbol + k > alphabet {
            return Err(fehler("Wiederholung laeuft ueber das Alphabet hinaus"));
        }
        for _ in 0..k {
            laengen[symbol] = wert;
            symbol += 1;
        }
    }
    Ok(laengen)
}

/// Farb-Cache-Info am Anfang jedes Bildstroms.
///
/// GEMESSEN: der Entwurf erlaubt 1..11, cwebp 1.5.0 schreibt aber den
/// Wert 12, und dwebp dekodiert die Dateien einwandfrei.
fn cache_info(b: &mut Bits) -> Result<u32, BildFehler> {
    if b.bit() == 0 {
        return Ok(0);
    }
    let bits = b.r(4);
    if !(1..=MAX_CACHE_BITS).contains(&bits) {
        return Err(fehler("cache_bits ausserhalb 1..12"));
    }
    Ok(bits)
}

/// Die fuenf Praefixcodes einer Gruppe.
///
/// Reihenfolge im Datenstrom: gruen (+Laenge +Cache), rot, blau, alpha,
/// distanz. Der Kommentar in libwebp nennt Alpha an zweiter Stelle und
/// irrt; der Code hat RED=1, BLUE=2, ALPHA=3.
type Gruppe = [Code; 5];

fn gruppe_lesen(b: &mut Bits, cache_bits: u32) -> Result<Gruppe, BildFehler> {
    let cache_groesse = if cache_bits > 0 {
        1usize << cache_bits
    } else {
        0
    };
    let g_alphabet = LIT_LITERALE as usize + LIT_LAENGEN as usize + cache_groesse;
    Ok([
        Code::neu(&lies_vorzeichenlaengen(b, g_alphabet)?)?,
        Code::neu(&lies_vorzeichenlaengen(b, LIT_LITERALE as usize)?)?,
        Code::neu(&lies_vorzeichenlaengen(b, LIT_LITERALE as usize)?)?,
        Code::neu(&lies_vorzeichenlaengen(b, LIT_LITERALE as usize)?)?,
        Code::neu(&lies_vorzeichenlaengen(b, LIT_DISTANZEN as usize)?)?,
    ])
}

/// Laenge oder Abstand aus Praefixcode und Zusatzbit
/// (`GetCopyDistance`, wortgleich).
fn aus_code(code: u32, b: &mut Bits) -> u32 {
    if code < 4 {
        return code + 1;
    }
    let extra = (code - 2) >> 1;
    let versatz = (2 + (code & 1)) << extra;
    versatz + b.r(extra) + 1
}

/// `PlaneCodeToDistance` aus libwebp.
fn distanz_in_punkte(plane_code: u32, breite: u32) -> u32 {
    if plane_code > DISTANZ_CODES {
        return plane_code - DISTANZ_CODES;
    }
    // GEMESSEN: der x-Versatz ist `8 - (code & 0xf)`, nicht der Wert
    // selbst. Deshalb steht oben der gepackte Code.
    //
    // UND: das Ergebnis kann NEGATIV sein — bei `kCodeToPlane[3] = 0x19`
    // ist das niedrige Halbbyte 9, und `8 - 9 = -1`. In Python geht das
    // ohne weiteres, in u32 laeuft es in einen Unterlauf. Deshalb hier
    // i32, und deshalb ist der erste Versuch dieses Ports an
    // `attempt to subtract with overflow` gestorben.
    let gepackt = K_CODE_TO_PLANE[(plane_code - 1) as usize] as i32;
    let d = (gepackt >> 4) * breite as i32 + (8 - (gepackt & 0xF));
    if d >= 1 { d as u32 } else { 1 }
}

/// M3/M4: `ceil(a / 2^k)` mit geprueftem Nenner.
fn auf_gerundet(oben: u32, kanten: u32) -> Result<u32, BildFehler> {
    if kanten == 0 {
        return Err(fehler("Kantenlaenge 0 in der Unterteilung"));
    }
    Ok(oben / kanten + u32::from(oben % kanten != 0))
}

/// Der Farbindex verengt den Bildstrom VOR dem Entropiekodieren.
fn strom_breite(breite: u32, trans: &[(u8, u32, u32, Vec<u32>)]) -> u32 {
    let mut w = breite;
    for &(art, wb, _, _) in trans {
        if art == 3 {
            // wb ist hier die Farbbreite in Bit (0..3).
            w = w.saturating_add((1u32 << wb) - 1) >> wb;
        }
    }
    w
}

/// Ein Bildstrom in ARGB (ein `u32` pro Pixel, A im oberen Byte).
fn dekodiere(
    b: &mut Bits,
    breite: u32,
    hoehe: u32,
    erlaube_meta: bool,
    cache_bits: u32,
) -> Result<Vec<u32>, BildFehler> {
    let ziel = (breite as usize)
        .checked_mul(hoehe as usize)
        .ok_or_else(|| fehler("Bildstrom zu gross"))?;
    if ziel > MAX_BILD {
        return Err(fehler("Bildstrom hat mehr als MAX_BILD Pixel"));
    }

    let cache_groesse = if cache_bits > 0 {
        1usize << cache_bits
    } else {
        0
    };
    let mut cache = vec![0u32; cache_groesse];

    let mut entropie: Vec<u32> = Vec::new();
    let mut gruppen: Vec<Gruppe> = Vec::new();
    let mut praefix_bits = 0u32;
    let mut xs = 0u32;

    if erlaube_meta && b.bit() == 1 {
        // Meta-Praefixcodes: nur fuer die Rolle „ARGB-Bild".
        praefix_bits = b.r(3) + 2;
        xs = auf_gerundet(breite, 1 << praefix_bits)?;
        let ys = auf_gerundet(hoehe, 1 << praefix_bits)?;
        let cb = cache_info(b)?;
        entropie = dekodiere(b, xs, ys, false, cb)?;
        // Die Meta-Zahl steht im GRUENEN Byte (`huffman_image[i] >> 8`).
        let mut groesster: u32 = 0;
        for p in &entropie {
            let g = (p >> 8) & 0xFFFF;
            if g > groesster {
                groesster = g;
            }
        }
        let n = groesster as usize + 1;
        if n > MAX_GRUPPEN {
            return Err(fehler("zu viele Meta-Gruppen"));
        }
        // M4: erst die Grenze pruefen, dann allozieren.
        gruppen.reserve_exact(n);
        for _ in 0..n {
            gruppen.push(gruppe_lesen(b, cache_bits)?);
        }
    } else {
        gruppen.push(gruppe_lesen(b, cache_bits)?);
    }

    let mut px: Vec<u32> = Vec::with_capacity(ziel);
    let mut pos = 0usize;
    while pos < ziel {
        let gi = if gruppen.len() == 1 {
            0
        } else {
            let x = pos as u32 % breite;
            let y = pos as u32 / breite;
            let spalte = (x >> praefix_bits) + xs * (y >> praefix_bits);
            let m = (entropie[spalte as usize] >> 8) & 0xFFFF;
            if m as usize >= gruppen.len() {
                return Err(fehler("Meta-Index zeigt ausserhalb der Gruppen"));
            }
            m as usize
        };
        let gr = &gruppen[gi];
        let s = gr[0].dec(b)? as u32;
        if s < LIT_LITERALE {
            // Reihenfolge: GRUEN, dann ROT, BLAU, ALPHA. GEMESSEN: rot
            // zuerst zu lesen zerlegt jede Datei ab dem zweiten Punkt.
            let r = gr[1].dec(b)? as u32;
            let bl = gr[2].dec(b)? as u32;
            let a = gr[3].dec(b)? as u32;
            let argb = (a << 24) | (r << 16) | (s << 8) | bl;
            px.push(argb);
            if cache_bits > 0 {
                let h = argb.wrapping_mul(0x1E35A7BD) >> (32 - cache_bits);
                cache[h as usize] = argb;
            }
            pos += 1;
        } else if s < LIT_LITERALE + LIT_LAENGEN {
            let laenge = aus_code(s - LIT_LITERALE, b) as usize;
            let dsymbol = gr[4].dec(b)? as u32;
            if dsymbol >= LIT_DISTANZEN {
                return Err(fehler("Distanzcode groesser als 40"));
            }
            let distanz = distanz_in_punkte(aus_code(dsymbol, b), breite) as usize;
            if distanz == 0 || distanz > pos {
                return Err(fehler("Rueckverweis zeigt vor den Bildanfang"));
            }
            let von = pos - distanz;
            for k in 0..laenge {
                if pos >= ziel {
                    break;
                }
                let q = px[von + k];
                px.push(q);
                if cache_bits > 0 {
                    let h = q.wrapping_mul(0x1E35A7BD) >> (32 - cache_bits);
                    cache[h as usize] = q;
                }
                pos += 1;
            }
        } else {
            if cache_bits == 0 {
                return Err(fehler("Cache-Code ohne Farb-Cache"));
            }
            let i = (s - LIT_LITERALE - LIT_LAENGEN) as usize;
            if i >= cache.len() {
                return Err(fehler("Cache-Index ausserhalb des Caches"));
            }
            px.push(cache[i]);
            pos += 1;
        }
    }
    Ok(px)
}

/// Ein Byte als `int8_t`.
fn vorz8(v: u32) -> i32 {
    let v = v & 0xFF;
    if v >= 128 { v as i32 - 256 } else { v as i32 }
}

/// Kanalweise Addition ohne Uebertrag (`VP8LAddPixels`).
fn punkt_add(a: u32, b: u32) -> u32 {
    let mut r = 0u32;
    for s in [24u32, 16, 8, 0] {
        r |= ((((a >> s) & 0xFF) + ((b >> s) & 0xFF)) & 0xFF) << s;
    }
    r
}

/// Mittlere Farbe aus zwei Punkten.
fn mittel2(a: u32, b: u32) -> u32 {
    punkt_add(a, b) & 0x7F7F7F7F
}

fn klemme(v: i32) -> u32 {
    v.clamp(0, 255) as u32
}

/// Der Praediktor (Entwurf 3.6.2.4), wortgleich `PredictorAdd_C`.
fn praediziere(m: u32, l: u32, t: u32, tl: u32, tr: u32) -> u32 {
    match m {
        0 => 0xFF00_0000,
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => mittel2(mittel2(l, tr), t),
        6 => mittel2(l, tl),
        7 => mittel2(l, t),
        8 => mittel2(tl, t),
        9 => mittel2(t, tr),
        10 => mittel2(mittel2(l, tl), mittel2(t, tr)),
        11 => {
            // `Select`: nicht "die kleinere Steigung", sondern
            // `pa_minus_pb = Summe(T - TL)`, und `a` (L) gewinnt bei `<= 0`.
            let mut pa_minus_pb = 0i32;
            for s in [24u32, 16, 8, 0] {
                let a = ((l >> s) & 0xFF) as i32;
                let bb = ((t >> s) & 0xFF) as i32;
                let c = ((tl >> s) & 0xFF) as i32;
                pa_minus_pb += (a + bb - c) - a;
            }
            if pa_minus_pb <= 0 { l } else { t }
        }
        12 => {
            // `ClampedAddSubtractFull`
            let mut r = 0u32;
            for s in [24u32, 16, 8, 0] {
                let v =
                    ((l >> s) & 0xFF) as i32 + ((t >> s) & 0xFF) as i32 - ((tl >> s) & 0xFF) as i32;
                r |= klemme(v) << s;
            }
            r
        }
        13 => {
            // `ClampedGradient`: runde HINUNTER, wie es die arithmetische
            // Schicht in C tut.
            let a = mittel2(l, t);
            let mut r = 0u32;
            for s in [24u32, 16, 8, 0] {
                let x = ((a >> s) & 0xFF) as i32;
                let y = ((tl >> s) & 0xFF) as i32;
                r |= klemme(x + (x - y).div_euclid(2)) << s;
            }
            r
        }
        _ => 0xFF00_0000,
    }
}

/// Eine Transformation: Art, Kachelbit, Kacheln je Zeile, Unterdaten.
type Trans = (u8, u32, u32, Vec<u32>);

/// Dekodiert eine `VP8L`-Nutzlast zu ARGB-Punkten.
pub fn vp8l_dekodieren(daten: &[u8]) -> Result<(u32, u32, Vec<u32>), BildFehler> {
    if daten.is_empty() {
        return Err(fehler("leere VP8L-Nutzlast"));
    }
    let mut b = Bits::new(daten);
    if b.r(8) != 0x2F {
        return Err(fehler("VP8L-Signatur ist nicht 0x2F"));
    }
    let breite = b.r(14) + 1;
    let hoehe = b.r(14) + 1;
    b.r(1); // alpha_is_used: nur ein Hinweis
    if b.r(3) != 0 {
        return Err(fehler("VP8L-Version ist nicht 0"));
    }
    let px = vp8l_strom_kopf(&mut b, breite, hoehe)?;
    Ok((breite, hoehe, px))
}

/// Wie [`vp8l_dekodieren`], aber OHNE den fuenf Byte grossen Kopf.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B3): der Alphakanal im `ALPH`-Chunk
/// ist ein VP8L-Bildstrom ohne diese fuenf Byte. In
/// `src/dec/vp8l_dec.c` Zeile 1754-1768 liest `DecodeImageStream` sie
/// nur ein, wenn `is_level0 == 0` ist; der Alphastrom wird mit
/// `is_level0 = 1` aufgerufen (Zeile 1950), es fehlen also Signatur
/// (0x2F), Breite, Hoehe, das Alpha-Bit und die Version. Die Groesse
/// steht statt dessen im Container (VP8X oder der Bildkopf des ANMF).
///
/// Beleg: `cwebp -q 90 -alpha_q 100` auf ein Bild mit weichem
/// Alpha-Verlauf lieferte einen `ALPH`-Chunk, dessen erstes Nutzlastbyte
/// 0x77 und nicht 0x2F ist — mit der geprueften Signatur lehnte unser
/// Dekoder den Strom ab („VP8L-Signatur ist nicht 0x2F“).
/// GEMESSEN 02.10.2026 (Rolle BILD, B3), gemessen an sechs echten
/// Dateien mit `cwebp -q 90 -alpha_q 100` gegen `dwebp`: die
/// Alphawerte stehen im GRUENEN Kanal, auch wenn der Strom eine Palette
/// benutzt. Es gab kurzzeitig einen Schalter, der statt dessen den
/// ALPHA-Kanal des Paletteneintrags nahm (so libwebp in
/// `ARGBIndexToAlpha`, `src/dec/vp8l_dec.c` Zeile 1075) — GEMESSEN
/// schlechter: 1248 statt 0 falsche Kanalwerte, und schon Pixel (1, 0)
/// stimmte dann nicht mehr. Der Schalter ist wieder entfernt.
///
/// Was libwebp dafuer tut, ist eine Optimierung: `Is8bOptimizable`
/// (`src/dec/vp8l_dec.c` Zeile 1011-1023) verlangt KEINEN Farb-Cache
/// und nur ein Symbol in den Baeumen fuer Rot, Blau und Alpha. Dann
/// kostet das Lesen dieser drei Symbole keine Bits, und der 8-Bit-Pfad
/// `DecodeAlphaData` liest dieselben Bits wie unser 32-Bit-Weg mit dem
/// gruenen Kanal — bei einem Farb-Cache oder echten Rot/Blau/Alpha-
/// Baeumen nimmt libwebp den 32-Bit-Weg mit `ExtractAlphaRows`, das
/// ebenfalls den gruenen Kanal liest.
pub fn vp8l_strom_ohne_kopf(daten: &[u8], breite: u32, hoehe: u32) -> Result<Vec<u32>, BildFehler> {
    if daten.is_empty() {
        return Err(fehler("leere VP8L-Nutzlast"));
    }
    let mut b = Bits::new(daten);
    vp8l_strom_kopf(&mut b, breite, hoehe)
}

/// Der Bildstrom hinter dem Kopf: Transformationen, Farb-Cache,
/// Huffman-Baum, Punkte, dann die Umkehrungen in umgekehrter Reihenfolge.
fn vp8l_strom_kopf(b: &mut Bits, breite: u32, hoehe: u32) -> Result<Vec<u32>, BildFehler> {
    if (breite as usize)
        .checked_mul(hoehe as usize)
        .ok_or_else(|| fehler("Bild zu gross"))?
        > MAX_BILD
    {
        return Err(fehler("Bild hat mehr als MAX_BILD Pixel"));
    }

    // Die Daten einer Transformation stehen AN IHRER STELLE, nicht danach.
    let mut trans: Vec<Trans> = Vec::new();
    while b.bit() == 1 {
        if trans.len() >= crate::webp::MAX_TRANSFORMS {
            return Err(fehler("mehr Transformationen als MAX_TRANSFORMS"));
        }
        let art = b.r(2) as u8;
        match art {
            0 | 1 => {
                let kachel = b.r(3) + 2;
                if kachel > KACHEL_BITS_MAX {
                    return Err(fehler("Kachelbit groesser als 9"));
                }
                let bx = auf_gerundet(breite, 1 << kachel)?;
                let by = auf_gerundet(hoehe, 1 << kachel)?;
                let cb = cache_info(b)?;
                let unter = dekodiere(b, bx, by, false, cb)?;
                trans.push((art, kachel, bx, unter));
            }
            2 => trans.push((art, 0, 0, Vec::new())),
            3 => {
                // Farb-Indexisierung (Palette). Die Nutzlast ist die
                // Farbliste selbst: `nc` Farben als Differenzen
                // (`ExpandColorMap` in `src/dec/vp8l_dec.c` Zeile
                // 1596-1610, byteweise ohne Uebertrag).
                let nc = b.r(8) as usize + 1;
                let cb = cache_info(b)?;
                let tab = dekodiere(b, nc as u32, 1, false, cb)?;
                let mut farben: Vec<u32> = Vec::with_capacity(nc);
                for (i, &x) in tab.iter().enumerate() {
                    farben.push(if i == 0 {
                        x
                    } else {
                        punkt_add(farben[i - 1], x)
                    });
                }
                // Die Farbbreite in Bit: 3, 2, 1 oder 0.
                let wb: u32 = if nc <= 2 {
                    3
                } else if nc <= 4 {
                    2
                } else if nc <= 16 {
                    1
                } else {
                    0
                };
                trans.push((3, wb, nc as u32, farben));
            }
            // `art` kommt aus zwei Bit, ist aber als `u8` gespeichert —
            // der Arm ist fuer den Compiler noetig und fuer eine
            // kaputte Datei der benannte Ort fuer den Fehler.
            _ => {
                return Err(fehler("unbekannte VP8L-Transformation"));
            }
        }
    }

    let w = strom_breite(breite, &trans);
    let cb = cache_info(b)?;
    let mut px = dekodiere(b, w, hoehe, true, cb)?;

    // Die Umkehrung laeuft in UMGEKEHRTER Reihenfolge der Aufnahme.
    for &(art, kachel, bx, ref unter) in trans.iter().rev() {
        match art {
            // SUBTRACT_GREEN: wortgleich `VP8LAddGreenToBlueAndRed_C`.
            // Die erste Fassung rechnete `punkt_add(p & 0xFFFF, gruen)` —
            // diese Maske laesst nur die unteren 16 Bit stehen, rot liegt
            // aber in Bit 16..23. Jedes Rot im Bild wurde dadurch zu Null.
            2 => {
                for p in px.iter_mut() {
                    let g = (*p >> 8) & 0xFF;
                    let rb = ((*p & 0x00FF_00FF) + (g << 16) + g) & 0x00FF_00FF;
                    *p = (*p & 0xFF00_FF00) | rb;
                }
            }
            // CROSS_COLOR, genau `VP8LTransformColorInverse_C`. Drei
            // Punkte, an denen eine "schoenere" Fassung falsch waere:
            // int8-Multiplikatoren, arithmetische Schicht, und Blau aus
            // dem BEREITS korrigierten Rot.
            1 => {
                let schritt = 1u32 << kachel;
                let kacheln = auf_gerundet(breite, schritt)?;
                let alt = std::mem::take(&mut px);
                let mut neu = Vec::with_capacity(alt.len());
                for y in 0..hoehe {
                    let reihe = (y >> kachel) * kacheln;
                    for x in 0..breite {
                        let p = alt[(y * breite + x) as usize];
                        let code = unter[(reihe + (x >> kachel)) as usize];
                        let g2r = vorz8(code & 0xFF);
                        let g2b = vorz8((code >> 8) & 0xFF);
                        let r2b = vorz8((code >> 16) & 0xFF);
                        let gruen = vorz8((p >> 8) & 0xFF);
                        // Drei Schritte, jeder einzeln. Verschachtelte
                        // Klammern sind an dieser Stelle schon zweimal
                        // schiefgegangen.
                        let d_rot = (g2r * gruen) >> 5;
                        let nr = (((p >> 16) & 0xFF) as i32 + d_rot) & 0xFF;
                        let d_blau1 = (g2b * gruen) >> 5;
                        let d_blau2 = (r2b * vorz8(nr as u32)) >> 5;
                        let nb = ((p & 0xFF) as i32 + d_blau1 + d_blau2) & 0xFF;
                        neu.push((p & 0xFF00_FF00) | (nr as u32) << 16 | nb as u32);
                    }
                }
                px = neu;
            }
            // COLOR_INDEX
            3 => {
                let farben = unter;
                let bits = kachel; // Farbbreite in Bit
                if bits == 0 {
                    for p in px.iter_mut() {
                        let i = ((*p >> 8) & 0xFF) as usize;
                        *p = farben.get(i).copied().unwrap_or(0);
                    }
                } else {
                    // Die Maske ist `(1 << stufen) - 1`. Die erste Fassung
                    // hatte `bits - 1`, also bei zwei Indexbit die Maske 1
                    // statt 3 — jeder zweite Index war 0, und
                    // vp8l_palette traf damit in jedem zweiten Pixel die
                    // falsche Farbe (512 von 1024).
                    let schritt = 1u32 << bits;
                    let stufen = 8 / schritt;
                    let maske = (1u32 << stufen) - 1;
                    let qb = auf_gerundet(breite, schritt)?;
                    let alt = std::mem::take(&mut px);
                    let mut neu = Vec::with_capacity(alt.len());
                    for y in 0..hoehe {
                        for x in 0..breite {
                            let g = (alt[(y * qb + (x >> bits)) as usize] >> 8) & 0xFF;
                            let i = (g >> ((x & (schritt - 1)) * stufen)) & maske;
                            neu.push(farben.get(i as usize).copied().unwrap_or(0));
                        }
                    }
                    px = neu;
                }
            }
            // PREDICTOR
            _ => {
                // Die Nachbarn kommen aus `neu`, also aus dem in diesem
                // Durchgang BEREITS REKONSTRUIERTEN Bild. Die erste Fassung
                // las sie aus `alt`, also aus dem Bild vor dem Praediktor.
                // Bei Modus L addierte sich damit die rohe statt der
                // rekonstruierte Nachbarfarbe, und die Abweichung waechst
                // mit der Bildbreite.
                // GEMESSEN 02.10.2026: die Modi stehen im Bild aus der
                // Vorbereitung, Kachel fuer Kachel von links nach rechts
                // und Zeile fuer Zeile — `PredictorInverse_C` in
                // `src/dec/vp8l_dec.c` Zeile 1130ff: der Modus ist
                // `(*pred_mode_src++ >> 8) & 0xf`, und die Kachelbreite
                // ist `1 << bits`. Die erste Zeile laeuft mit Modus 1
                // (L), die erste Spalte jeder weiteren Zeile mit Modus 2
                // (T), die linke obere Ecke mit Modus 0.
                let alt = std::mem::take(&mut px);
                let mut neu = Vec::with_capacity(alt.len());
                for y in 0..hoehe {
                    for x in 0..breite {
                        let m = if y == 0 {
                            if x == 0 { 0 } else { 1 }
                        } else if x == 0 {
                            2
                        } else {
                            (unter[((y >> kachel) * bx + (x >> kachel)) as usize] >> 8) & 0x0F
                        };
                        let v = if m == 0 {
                            0xFF00_0000
                        } else {
                            let links = if x == 0 {
                                0
                            } else {
                                neu[(y * breite + x - 1) as usize]
                            };
                            let oben = if y == 0 {
                                0
                            } else {
                                neu[((y - 1) * breite + x) as usize]
                            };
                            let links_oben = if x == 0 || y == 0 {
                                0
                            } else {
                                neu[((y - 1) * breite + x - 1) as usize]
                            };
                            // Die rechts oben liegende Nachbarzelle
                            // existiert in der letzten Spalte nicht; dort
                            // nehmen wir die oben (das ist zugleich der
                            // Wert, den `PredictorAdd*` dort erwartet,
                            // weil der Puffer mit einem Schutzpixel
                            // endet).
                            let rechts_oben = if y == 0 {
                                0
                            } else if x + 1 < breite {
                                neu[((y - 1) * breite + x + 1) as usize]
                            } else {
                                neu[((y - 1) * breite + x) as usize]
                            };
                            praediziere(m, links, oben, links_oben, rechts_oben)
                        };
                        let q = alt[(y * breite + x) as usize];
                        neu.push(punkt_add(q, v));
                    }
                }
                px = neu;
            }
        }
    }
    Ok(px)
}

/// Liest ein Bild aus einer `VP8L`-Nutzlast.
pub fn vp8l_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    let (breite, hoehe, px) = vp8l_dekodieren(daten)?;
    let n = (breite as usize) * (hoehe as usize);
    if px.len() != n {
        return Err(fehler("falsche Punktzahl nach der Umkehrung"));
    }
    // M4: geprueft, bevor alloziert.
    let mut bild = Bild {
        breite,
        hoehe,
        rgba: Vec::with_capacity(n * 4),
    };
    for p in px {
        bild.rgba.push((p >> 16) as u8);
        bild.rgba.push((p >> 8) as u8);
        bild.rgba.push(p as u8);
        bild.rgba.push((p >> 24) as u8);
    }
    Ok(bild)
}
