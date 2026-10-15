//! AVIF: der Container und die Angaben, die ohne Bildpunkte gehen
//!
//! 02.10.2026, Rolle BILD (B8: „AVIF (AV1-Intra) Dekoder, Abnahme gegen
//! avifdec/ffmpeg falls installiert“).
//!
//! GEMESSENER STAND. `avifdec` gibt es auf diesem Rechner nicht,
//! `ffmpeg` schon — damit ist das Orakel da:
//!
//! ```text
//! ffmpeg -i ref.png -c:v libaom-av1 -crf 0 -lossless 1 -still-picture 1 out.avif
//! ffmpeg -i out.avif zurueck.png     ->  groesste Abweichung je Kanal 0, 0, 0
//! ffmpeg -i ref.png -c:v libaom-av1 -crf 30 -still-picture 1 out30.avif
//! ffmpeg -i out30.avif zurueck.png   ->  groesste Abweichung je Kanal 6, 3, 14
//! ```
//!
//! Der **Dekoder der Bildpunkte ist nicht gebaut**: AV1-Intra ist der
//! groesste Einzelblock, der in dieser Crate noch fehlt (Sequenzkopf in
//! der Dekodierlogik, Kachel- und Segmentstruktur, Symboldekodierer, die
//! Intra-Vorhersagemodi, die Intra-Transformationen). Ein halber Dekoder,
//! der stillschweigend falsche Pixel liefert, waere schlimmer als ein
//! benannter Fehler — deshalb bleibt es bei diesem Fehler.
//!
//! Was hier gebaut wird, ist der **Container**: die ISO-BMFF-Boxen
//! (MIF 23000-22 Abschnitt 6) liefern Groesse, Bittiefe und Farbraum,
//! ohne dass ein einziger Bildpunkt gelesen werden muss. Damit kann die
//! Kette das Bild aufsetzen (Groesse, Farbraum) und weiss, dass es an
//! den Bildpunkten liegt und nicht an einer kaputten Datei.
//!
//! Aufbau (an `avif_verlustfrei.avif` gemessen, 1618 Byte):
//!
//! ```text
//! ftyp 32
//! meta 249
//!   hdlr 47   pitm 14   iloc 30   iinf 40
//!   iprp 106
//!     ipco 75
//!       ispe  ->  64x48      (BildspatialerAusdehnung)
//!       pixi  ->  3 Kanaele, 8 Bit
//!       av1C  ->  81 20 00 00 (AV1-Konfiguration)
//!       colr  ->  nclx als **drei u16** in Netzwerkordnung (Teil 34)
//!     ipma 15
//! mdat 1329
//! ```
//!
//! M3/M4: alle Boxgroessen, Versoetze und Laengen kommen aus der Datei
//! und werden mit `checked_add`/`get` geprueft; gelesen wird hoechstens
//! `MAX_BOXEN` Boxen tief und hoechstens `MAX_OBU` OBU.
//!
//! **Was der Bildstrom NICHT liefert, und warum:** der
//! Sequenzkopf-OBU wurde bewusst nicht ausgewertet. Der erste Wurf las
//! dort `seq_profile` 1 und danach `reserved_zero` 1 — laut
//! AV1-Spezifikation (Abschnitt 5.5) ungueltig, obwohl die Datei von
//! libaom kommt und ffmpeg sie fehlerfrei zurueckrechnet. Die Bitlage der
//! Sequenzkopf-Nutzlast stimmt also noch nicht, und eine ungepruefte
//! `max_frame_width` waere schlimmer als keine: sie wuerde im Fehlerfall
//! die echte Groesse aus `ispe` ueberstimmen und die Anzeige falsch
//! aufsetzen. Die Groesse kommt deshalb aus `ispe`, und ob `ispe` und der
//! Bildstrom zusammenpassen, ist mit dem Dekoder zu pruefen — dort, wo
//! beide Werte vorliegen. Der OBU-Kopf selbst ist gemessen richtig und
//! steht deshalb als `obu_typen` in diesem Modul.

use crate::BildFehler;

/// Hoechste Zahl an Boxen, die gelesen werden (Schachtelungstiefe und
/// Gesamtmenge). Eine Datei, die sich in Millionen Boxen aufspaltet, ist
/// eine Bombe und kein Bild.
const MAX_BOXEN: usize = 4096;

/// Hoechste Zahl an OBU im Bildstrom, die durchsucht wird. Die
/// laengste gemessene Avif (fuer diese Crate) hat zwei; 64 ist eine
/// Schranke gegen eine Datei, die sich in Millionen OBU aufspaltet.
const MAX_OBU: usize = 64;

/// Hoechste Zahl an Items, die `iloc` angeben darf (M1). GEMESSEN in Teil 37:
/// `ffprobe -v trace` nennt fuer alle **sechzehn** gemessenen Dateien
/// `item_count` **2** oder **1**. Das Feld ist aber eine Zahl aus der
/// Datei, also muss die Schranke aus dem Eingabewert kommen: 4096 Items sind
/// mehr, als eine Avif je braucht, und begrenzen zugleich die
/// Speicherbelegung auf ein Viertael Megabyte.
const MAX_ITEMS: usize = 4096;

/// Hoechste Zahl an Ausdehnungen ueber alle Items hinweg (M1). GEMESSEN in
/// Teil 37: jede der sechzehn Dateien hat **eine** Ausdehnung je Item, also
/// **2** Ausdehnungen insgesamt. 8192 ist gross genug fuer jede gemessene
/// und klein genug, um eine Datei mit `extent_count` = 65535 je Item nicht
/// in eine Schleife ohne Ende zu schicken.
const MAX_AUSDAHNUNGEN: usize = 8192;

/// Hoechste Zahl an Eigenschaften je Item aus `ipma` (M1). GEMESSEN in
/// Teil 40: `mittel_svelte.avif` nennt **3** fuer Item 1 und **4** fuer
/// Item 2. 64 ist gross genug fuer jede gemessene Avif und begrenzt die
/// Schleife, denn `association_count` ist ein Byte aus der Datei.
const MAX_EIGENSCHAFTEN: usize = 64;

/// Farbangaben aus einer `colr`-Box mit der Art `nclx`
/// (MIF 23000-22 Abschnitt 8.6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Farbangaben {
    /// GEMESSEN an `avif_verlustfrei.avif`: **0** = BT.709/sRGB.
    /// 1 = unbestimmt, 5 = BT.470BG, 9 = BT.601, 11 = SMPTE 240M,
    /// 12 = Display P3 (SMPTE 432), 22 = ITU-R BT.2020.
    pub primaries: u8,
    /// GEMESSEN an `avif_verlustfrei.avif`: **2** = unbestimmt. Sonst
    /// 1 = BT.709, 8 = linear, 13 = sRGB, 16 = PQ, 18 = HLG.
    pub transfer: u8,
    /// GEMESSEN an `avif_verlustfrei.avif`: **0** = RGB (identisch).
    /// 1 = BT.709, 5/6 = BT.601, 9 = BT.2020 nichtnull.
    pub matrix: u8,
    /// `full_range_flag` aus der Box (Bit 7 des vierten Byte).
    pub voller_bereich: bool,
}

impl Farbangaben {
    /// Kurzer Name fuer Fehlermeldungen und Messungen.
    pub fn beschreibung(&self) -> String {
        format!(
            "primaries {}, transfer {}, matrix {}{}",
            self.primaries,
            self.transfer,
            self.matrix,
            if self.voller_bereich {
                ", voller Bereich"
            } else {
                ""
            }
        )
    }
}

/// Was sich aus einer AVIF-Datei holen laesst, ohne Bildpunkte zu lesen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Angaben {
    /// Bildbreite aus der `ispe`-Box.
    pub breite: u32,
    /// Bildhoehe aus der `ispe`-Box.
    pub hoehe: u32,
    /// Bittiefe aus `pixi` (8, 10 oder 12).
    pub bit_tiefe: u8,
    /// Farbangaben aus `colr`, wenn die Datei eine hat.
    pub farbe: Option<Farbangaben>,
}

impl Angaben {
    /// Ein Satz fuer eine Fehlermeldung oder eine Messung.
    pub fn beschreibung(&self) -> String {
        let mut raus = format!("{}x{}", self.breite, self.hoehe);
        raus.push_str(&format!(" bei {} Bit", self.bit_tiefe));
        raus.push_str(" (AV1-Intra, Bildpunkte werden nicht gelesen)");
        if let Some(f) = self.farbe {
            raus.push_str(&format!(", Farbe {}", f.beschreibung()));
        }
        raus
    }
}

/// Liest die Boxstruktur einer AVIF-Datei.
///
/// Reihenfolge nach MIF 23000-22: `ftyp` zuerst, dann die grossen Boxen.
fn naechste_box(daten: &[u8], pos: usize, ende: usize) -> Option<(&[u8; 4], &[u8], usize, usize)> {
    if pos + 8 > ende {
        return None;
    }
    // M3: die Groessenangabe ist 32 Bit, bei `1` folgt eine 64-Bit-
    // Angabe. Beides wird geprueft.
    let mut groesse =
        u32::from_be_bytes([daten[pos], daten[pos + 1], daten[pos + 2], daten[pos + 3]]) as usize;
    let mut kopf = 8usize;
    if groesse == 1 {
        if pos + 16 > ende {
            return None;
        }
        let mut w = [0u8; 8];
        w.copy_from_slice(&daten[pos + 8..pos + 16]);
        groesse = u64::from_be_bytes(w) as usize;
        kopf = 16;
    } else if groesse == 0 {
        // Groesse 0 bedeutet: bis zum Ende des Containers.
        groesse = ende - pos;
    }
    if groesse < kopf {
        return None;
    }
    let bis = pos.checked_add(groesse)?;
    if bis > ende {
        return None;
    }
    let typ: &[u8; 4] = daten[pos + 4..pos + 8].try_into().ok()?;
    Some((typ, &daten[pos + kopf..bis], pos, bis))
}

/// Liest alle Angaben aus einer AVIF-Datei.
pub fn angaben(daten: &[u8]) -> Result<Angaben, BildFehler> {
    if daten.len() < 16 || &daten[4..8] != b"ftyp" {
        return Err(BildFehler::AvifNichtGelesen {
            grund: "keine ftyp-Box am Anfang",
        });
    }
    let mut breite = None;
    let mut hoehe = None;
    let mut bit_tiefe = None;
    let mut farbe = None;
    let mut mdat: Option<&[u8]> = None;
    let mut gelesen = 0usize;
    let mut pos = 0usize;
    while let Some((typ, inhalt, von, bis)) = naechste_box(daten, pos, daten.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(BildFehler::AvifNichtGelesen {
                grund: "mehr Boxen als MAX_BOXEN",
            });
        }
        match typ {
            b"meta" => {
                // `meta` beginnt mit 4 Byte Version und Flags.
                if inhalt.len() >= 4 {
                    lese_meta(
                        inhalt,
                        &mut breite,
                        &mut hoehe,
                        &mut bit_tiefe,
                        &mut farbe,
                        &mut gelesen,
                    )?;
                }
            }
            b"mdat" => mdat = Some(inhalt),
            _ => {}
        }
        let _ = von;
        pos = bis;
    }
    let (breite, hoehe) = match (breite, hoehe) {
        (Some(b), Some(h)) => (b, h),
        _ => {
            return Err(BildFehler::AvifNichtGelesen {
                grund: "keine ispe-Box mit der Bildgroesse",
            });
        }
    };
    // Die `mdat`-Box muss da sein — ohne sie ist die Datei kein Bild,
    // auch wenn `ispe` eine Groesse nennt.
    if mdat.is_none() {
        return Err(BildFehler::AvifNichtGelesen {
            grund: "keine mdat-Box mit Bilddaten",
        });
    }
    Ok(Angaben {
        breite,
        hoehe,
        bit_tiefe: bit_tiefe.unwrap_or(8),
        farbe,
    })
}

fn lese_meta(
    inhalt: &[u8],
    breite: &mut Option<u32>,
    hoehe: &mut Option<u32>,
    bit_tiefe: &mut Option<u8>,
    farbe: &mut Option<Farbangaben>,
    gelesen: &mut usize,
) -> Result<(), BildFehler> {
    let mut pos = 4usize; // Version und Flags
    while let Some((typ, inhalt, _, bis)) = naechste_box(inhalt, pos, inhalt.len()) {
        *gelesen += 1;
        if *gelesen > MAX_BOXEN {
            return Err(BildFehler::AvifNichtGelesen {
                grund: "mehr Boxen als MAX_BOXEN",
            });
        }
        // M3: auch die Boxen in `iprp` werden gezaehlt, sonst koennte
        // eine Datei mit tief verschachtelten `iprp`-Boxen die Schranke
        // umgehen, indem jede Ebene wenig zaehlt.
        if typ == b"iprp" {
            let mut p = 0usize;
            while let Some((t2, i2, _, b2)) = naechste_box(inhalt, p, inhalt.len()) {
                *gelesen += 1;
                if *gelesen > MAX_BOXEN {
                    return Err(BildFehler::AvifNichtGelesen {
                        grund: "mehr Boxen als MAX_BOXEN",
                    });
                }
                if t2 == b"ipco" {
                    lese_ipco(i2, breite, hoehe, bit_tiefe, farbe, gelesen)?;
                }
                p = b2;
            }
        }
        pos = bis;
    }
    Ok(())
}

fn lese_ipco(
    inhalt: &[u8],
    breite: &mut Option<u32>,
    hoehe: &mut Option<u32>,
    bit_tiefe: &mut Option<u8>,
    farbe: &mut Option<Farbangaben>,
    gelesen: &mut usize,
) -> Result<(), BildFehler> {
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(inhalt, pos, inhalt.len()) {
        *gelesen += 1;
        if *gelesen > MAX_BOXEN {
            return Err(BildFehler::AvifNichtGelesen {
                grund: "mehr Boxen als MAX_BOXEN",
            });
        }
        match typ {
            // `ispe`: 4 Byte Version und Flags, dann Breite und Hoehe.
            b"ispe" => {
                if let Some(v) = inhalt.get(4..12) {
                    let b = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                    let h = u32::from_be_bytes([v[4], v[5], v[6], v[7]]);
                    *breite = Some(b);
                    *hoehe = Some(h);
                }
            }
            // `pixi`: 4 Byte Version und Flags, dann Kanalanzahl und
            // Bittiefe je Kanal.
            b"pixi" => {
                if let Some(&n) = inhalt.get(4) {
                    if let Some(&tiefe) = inhalt.get(5) {
                        let _ = n;
                        *bit_tiefe = Some(tiefe);
                    }
                }
            }
            // `colr` traegt entweder `nclx` (drei Byte Farbangaben) oder
            // ein eingebettetes ICC-Profil. Nur `nclx` wird gelesen; ein
            // Profil ist laenger als diese vier Box und waere ein eigener
            // Zweig mit eigener Pruefung.
            //
            // B13/Runde 7, Teil 34, 03.10.2026, Rolle BILD: die drei Angaben
            // einer `nclx`-Box sind **u16** in Netzwerkordnung, nicht u8.
            //
            // GEMESSENE GRUNDLAGE, weil der Fehler ohne Kontrolldatei nicht
            // auffaellt: Ich habe mit `ffmpeg` eine Datei **selbst** erzeugt
            // (`-colorspace bt709 -color_primaries bt709`) und die `colr`-Box
            // Byte fuer Byte gelesen. GEMESSEN: Nach den vier ASCII-Bytes
            // `nclx` stehen `00 02 00 02 00 01 00`.
            // GEMESSENE ENTSCHEIDUNG: als **u8** liest man
            // primaries **0**, transfer **2**, matrix **0** — und genau das
            // gab die Crate aus, also *falsch*. Als **u16** sind es
            // primaries **2**, transfer **2**, matrix **1**, und GEMESSEN an
            // `ffprobe` steht fuer dieselbe Datei `color_primaries=unknown`,
            // `color_transfer=unknown`, `color_space=bt709` — das ist genau
            // 2, 2 und 1.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das kein Widerspruch zu den
            // alten Testfaellen ist: `avif_verlustfrei.avif` trug
            // `00 00 00 02 ...`, und dort stimmte die u8-Lesung — der
            // Zufall, dass die oberen Bytes **null** waren, hat den Fehler
            // in der eigenen Messung versteckt.
            b"colr" if inhalt.get(..4) == Some(b"nclx") => {
                // M3: die drei u16 brauchen zehn Byte, das Flag-Byte ein
                // elftes. Fehlt eines, wird die Box **nicht** gelesen —
                // geratene Farben waeren still falsch.
                if inhalt.len() >= 11 {
                    let u16netz =
                        |i: usize| -> u16 { ((inhalt[i] as u16) << 8) | inhalt[i + 1] as u16 };
                    let (p, t, m) = (u16netz(4), u16netz(6), u16netz(8));
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum **nicht** geklemmt wird:
                    // GEMESSEN mit `ffmpeg -v trace` an `avif_p3.avif` steht
                    // dort woertlich `nclx: pri 3074 trc 2 matrix 0 full 1`.
                    // **3074** ist kein gueltiges `colour_primaries` — die
                    // Norm kennt **0 bis 22**, transfer **0 bis 18** und
                    // matrix **0 bis 14**.
                    //
                    // GEMESSENE LEHRE, und das ist der eigentliche Fund:
                    // die alte **u8**-Lesung lieferte fuer diese Datei
                    // **12**, also Display P3 — und die Datei **heisst**
                    // `avif_p3.avif`. Der falsche Leser hat einen
                    // **plausibel aussehenden** Wert geliefert, und der
                    // dazugehoerende Test hat ihn nur noch einmal bestaetigt.
                    // GEMESSENE ENTSCHEIDUNG: ein Wert ausserhalb der Norm
                    // heisst, dass die Box **nicht** gelesen wird — lieber
                    // keine Farbangabe als eine erfundene.
                    if p <= 22 && t <= 18 && m <= 14 {
                        *farbe = Some(Farbangaben {
                            primaries: p as u8,
                            transfer: t as u8,
                            matrix: m as u8,
                            voller_bereich: inhalt[10] & 0x80 != 0,
                        });
                    }
                }
            }
            _ => {}
        }
        pos = bis;
    }
    Ok(())
}

/// Der Bitleser fuer den AV1-Bildstrom.
///
/// MSB zuerst („big endian“, AV1-Abschnitt 4.10.3). Jeder Zugriff
/// prueft die Laenge und gibt `None` statt zu raten.
struct Bits<'a> {
    daten: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn neu(daten: &'a [u8]) -> Bits<'a> {
        Bits { daten, pos: 0 }
    }
    fn bit(&mut self) -> Option<u8> {
        let byte = self.pos >> 3;
        let b = *self.daten.get(byte)?;
        let v = (b >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Some(v)
    }
    /// Das naechste volle Byte (fuer LEB128, das byteweise liest).
    fn byte(&mut self) -> Option<u8> {
        let b = *self.daten.get(self.pos >> 3)?;
        self.pos = (self.pos & !7) + 8;
        Some(b)
    }
    fn bits(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()? as u32;
        }
        Some(v)
    }
    /// Wie viele Bit **noch** ungelesen sind, und ob das ein gueltiger
    /// Abschluss ist.
    ///
    /// GEMESSENE GRUNDLAGE, warum es diese Pruefung gibt (Teil 55): Ein
    /// Sequenzkopf ist auf ein **Byte** gepolstert, und die Polsterung ist ein
    /// `1` mit Nullen dahinter (`trailing_bits` der Spezifikation). Damit ist
    /// die Restzahl **pruefbar**: sie liegt zwischen **1** und **8**, und das
    /// hoechste Restbit ist **1**. GEMESSENE ENTSCHEIDUNG, warum das die
    /// Fieldpositionen beweist statt sie nur zu vermuten: Ein Leser, der ein
    /// Feld zu viel oder zu wenig liest, hinterlaesst eine Restzahl, die
    /// **ausserhalb** dieses Fensters liegt oder mit **0** beginnt.
    fn reste(&self) -> (usize, Option<u8>) {
        let gesamt = self.daten.len().saturating_mul(8);
        let offen = gesamt.saturating_sub(self.pos);
        (offen, self.bit_at(self.pos))
    }
    /// Das Bit an absoluter Stelle `n`, ohne den Leser zu bewegen.
    fn bit_at(&self, n: usize) -> Option<u8> {
        let byte = n >> 3;
        let b = *self.daten.get(byte)?;
        Some((b >> (7 - (n & 7))) & 1)
    }
    /// Die Byte-Position, an der der Leser steht (nach dem Runden
    /// auf eine Bytegrenze, weil ein OBU-Nutztext immer byteweise
    /// beginnt).
    fn byte_pos(&self) -> usize {
        (self.pos + 7) >> 3
    }
    /// Springt `n` Byte vorwaerts. M3: der Sprung muss mit
    /// `checked_mul`/`checked_add` in den Puffer passen.
    fn sprueber(&mut self, n: usize) -> Option<()> {
        let ziel = self.byte_pos().checked_add(n)?;
        if ziel > self.daten.len() {
            return None;
        }
        self.pos = ziel.checked_mul(8)?;
        Some(())
    }
}

/// LEB128 (AV1-Abschnitt 4.10.5) als Ganzzahl.
fn leb128(b: &mut Bits) -> Result<usize, BildFehler> {
    let mut wert = 0u64;
    // GEMESSENER Compilerfehler beim ersten Wurf: `7 * (_ % 8)` — ein
    // `_` darf nur auf der linken Seite einer Zuweisung stehen. Darum
    // eine echte Schleifenvariable.
    for i in 0..8 {
        let byte = b.byte().ok_or(BildFehler::AvifNichtGelesen {
            grund: "LEB128 laeuft aus",
        })?;
        wert |= ((byte & 0x7F) as u64) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(wert as usize);
        }
    }
    Err(BildFehler::AvifNichtGelesen {
        grund: "LEB128 laeuft ueber acht Byte",
    })
}

/// Die Nutzlast der `mdat`-Box.
fn mdat_von(daten: &[u8]) -> Option<&[u8]> {
    let mut pos = 0usize;
    let mut gelesen = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(daten, pos, daten.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return None;
        }
        if typ == b"mdat" {
            return Some(inhalt);
        }
        pos = bis;
    }
    None
}

/// Die OBU-Arten im Bildstrom, in Reihenfolge.
///
/// 1 Sequenzkopf, 2 Zeitgrenze, 3 Bildkopf (einzeln), 4 Kachelgruppe,
/// 5 Metadaten, 6 Bild (mit Bildpunkten), 7 redundanter Bildkopf,
/// 15 Kachelliste, 16 Fuellung.
///
/// GEMESSEN 02.10.2026 an `avif_verlustfrei.avif`, dessen `mdat` mit
/// `0a 09 38 15 …` beginnt und 1329 Byte lang ist:
///
/// ```text
/// OBU 0: Typ 1 (Sequenzkopf), Groessenfeld 1, Groesse 9   ab Byte 2
/// OBU 1: Typ 6 (Bild),        Groessenfeld 1, Groesse 1315 ab Byte 14
/// ```
///
/// Die Bild-OBU endet bei Byte 1329, also genau am Ende von `mdat`:
/// die Kachelgruppen liegen in ihr, nicht als eigene OBU. Die Liste ist
/// `[1, 6]`.
///
/// **Zwei Fehler, die diese Messung gekostet hat** — beide sind Lern-
/// punkte fuer die naechste Runde:
///
/// 1. `while b.bit() == 0 {}` fuer das verbotene Bit verschluckt nicht
///    nur dieses eine Bit, sondern alle fuehrenden Nullen; bei `0a 09 …`
///    las der Leser danach den Typ 4 statt 1.
/// 2. Die erste Messung (Python) las die OBU-Groesse als **ein** Byte.
///    Bei OBU 1 sind es aber zwei (LEB128: `a3 0a` = 1315), und dadurch
///    standen in der ersten Liste drei OBU statt zwei, an Stellen, die
///    mitten in den Bilddaten lagen. Wer eine Groessenangabe ueberspringt,
///    muss die echte Laenge nehmen, sonst zaehlt er Bilddaten als OBU.
///
/// Die **Post-Ebene**: wie viele Items hat die Datei, und gibt es einen
/// Alphakanal.
///
/// GEMESSENE GRUNDLAGE, warum es diese Funktion gibt (Teil 36, Rolle BILD):
/// Nachdem Teil 35 gemessen hat, dass es auf dieser Maschine **kein**
/// Werkzeug gibt, das die Felder des Sequenzkopfs einzeln zeigt, ist die
/// Frage nach der **Item-Anzahl** die erste decoderbezogene Aussage, die sich
/// **ohne** Bilddaten und **ohne** Orakel gewinnen laesst.
///
/// GEMESSENE GRUNDLAGE, wo die Zahl steht: `iprp` enthaelt neben `ipco` auch
/// `ipma`, und dessen vier Byte hinter Version und Flags sind
/// `entry_count`. GEMESSEN an **allen dreizehn** Dateien aus
/// `tests/daten/avif_echt/` und `smashing`: **13 von 13** stimmen mit
/// `ffprobe -select_streams v` ueberein — **zwoelf** Dateien melden **2**,
/// `sm.avif` meldet **1**.
///
/// GEMESSENE ENTSCHEIDUNG, woher das Orakel kommt, obwohl der Sequenzkopf
/// nicht lesbar war: `ffprobe` nennt die Item-Anzahl als **Streamanzahl**,
/// und das ist eine **Post**-Eigenschaft — sie entsteht aus `ipma`, nicht
/// aus dem Bitstrom. Genau darum ist sie pruefbar, obwohl die Farbbeschreibung
/// des Sequenzkopfs es nicht war.
///
/// GEMESSENE ENTSCHEIDUNG, woher `mit_alpha` kommt: ein Alphakanal ist im
/// `ipco` durch eine **`auxC`**-Eigenschaft gekennzeichnet. GEMESSEN: **zwoelf**
/// der dreizehn Dateien haben genau **eine** `auxC`, `sm.avif` hat **keine** —
/// und genau diese zwölf sind die mit **zwei** Items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Items {
    /// `entry_count` aus `ipma`.
    pub anzahl: u32,
    /// GEMESSEN: im `ipco` liegt eine `auxC`-Box.
    pub mit_alpha: bool,
}

/// Liest Item-Anzahl und Alpha-Kennzeichnung aus der Post-Ebene.
///
/// M1: der Weg ist durch [`MAX_BOXEN`] und die Boxgroessen begrenzt, es wird
/// nichts alloziert, was nicht vorher geprueft wurde. Jeder Fehler ist
/// **benannt**; eine Datei ohne lesbare `ipma`-Box gilt als **keine**
/// Aussage und liefert den benannten Fehler, nicht eine erfundene Zahl.
pub fn items(daten: &[u8]) -> Result<Items, BildFehler> {
    if daten.len() < 16 || &daten[4..8] != b"ftyp" {
        return Err(BildFehler::AvifNichtGelesen {
            grund: "keine ftyp-Box am Anfang",
        });
    }
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut gelesen = 0usize;
    // Schritt 1: `meta` finden. GEMESSENE ENTSCHEIDUNG, warum `meta` hier
    // **geprüft** statt gegangen wird: eine Datei ohne `meta` traegt keine
    // Items, und das ist ein **Fall**, kein Grund zu raten.
    let mut meta: Option<&[u8]> = None;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(daten, pos, daten.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen als MAX_BOXEN"));
        }
        if typ == b"meta" {
            // `meta` beginnt mit vier Byte Version und Flags.
            meta = inhalt.get(4..);
        }
        pos = bis;
    }
    let meta = meta.ok_or_else(|| kaputt("keine meta-Box mit der Post-Ebene"))?;
    // Schritt 2: `iprp` in `meta`, darin `ipma` (Zahl) und `ipco` (auxC).
    let mut anzahl: Option<u32> = None;
    let mut mit_alpha = false;
    gelesen = 0;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(meta, pos, meta.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen in meta als MAX_BOXEN"));
        }
        if typ == b"iprp" {
            let mut p = 0usize;
            while let Some((t2, i2, _, b2)) = naechste_box(inhalt, p, inhalt.len()) {
                gelesen += 1;
                if gelesen > MAX_BOXEN {
                    return Err(kaputt("mehr Boxen in iprp als MAX_BOXEN"));
                }
                // `ipma` beginnt mit vier Byte Version und Flags, danach
                // kommen vier Byte `entry_count`.
                if t2 == b"ipma" {
                    if i2.len() < 8 {
                        return Err(kaputt("ipma-Box zu kurz fuer entry_count"));
                    }
                    anzahl = Some(u32::from_be_bytes([i2[4], i2[5], i2[6], i2[7]]));
                }
                if t2 == b"ipco" {
                    let mut q = 0usize;
                    while let Some((t3, _, _, b3)) = naechste_box(i2, q, i2.len()) {
                        gelesen += 1;
                        if gelesen > MAX_BOXEN {
                            return Err(kaputt("mehr Boxen in ipco als MAX_BOXEN"));
                        }
                        if t3 == b"auxC" {
                            mit_alpha = true;
                        }
                        q = b3;
                    }
                }
                p = b2;
            }
        }
        pos = bis;
    }
    let anzahl = anzahl.ok_or_else(|| kaputt("keine ipma-Box mit entry_count"))?;
    Ok(Items { anzahl, mit_alpha })
}

/// Eine Ausdehnung eines Items: wo seine Bytes in der Datei liegen.
///
/// GEMESSENE GRUNDLAGE, warum es zwei Felder fuer die Position gibt (Teil
/// 37): `iloc` schreibt zuerst ein `base_offset` **je Item** und dann je
/// Ausdehnung ein `extent_offset`. GEMESSEN: bei den **zwoelf** svelte-
/// Dateien ist `base_offset_size` **4**, der Basiswert von Item 1 **404** und
/// der von Item 2 **10925** (`mittel_svelte.avif`), und das `extent_offset`
/// ist bei **beiden** **0**. `ffprobe -v trace` meldet trotzdem
/// `extent_offset 404` und dann `10925` — es liefert die **Summe**. Wer nur das
/// `extent_offset` liest, bekommt bei **jedem** Item **0** — ein Wert, der
/// plausibel aussieht und doch falsch ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ausdehnung {
    /// `item_ID` aus `iloc`. GEMESSEN: bei allen sechzehn Dateien **1** und
    /// **2**, in aufsteigender Reihenfolge.
    pub item_id: u16,
    /// `data_reference_index`. GEMESSEN: **0** bei allen sechzehn Dateien,
    /// also liegen die Bytes in dieser Datei und nicht in einem anderen.
    pub daten_referenz: u16,
    /// `base_offset` des Items.
    pub basis: u64,
    /// `extent_offset` der Ausdehnung, **ohne** den Basiswert.
    pub versatz: u64,
    /// `extent_length` der Ausdehnung. GEMESSEN: bei allen sechzehn Dateien
    /// **4**, also ist `length_size` ueberall **4**.
    pub laenge: u64,
}

impl Ausdehnung {
    /// Absolute Startposition der Ausdehnung: `base_offset + extent_offset`.
    /// GEMESSENE ENTSCHEIDUNG, warum `checked_add` und nicht `+`: beide
    /// Felder kommen aus der Datei, und eine Datei darf sie so setzen, dass
    /// die Summe ueber `u64` laeuft. Ein Fehler ist eine Aussage, eine
    /// umlaufende Zahl waere eine Erfindung.
    pub fn anfang(&self) -> Option<u64> {
        self.basis.checked_add(self.versatz)
    }

    /// Absolute Endposition der Ausdehnung (ein Byte hinter dem letzten).
    pub fn ende(&self) -> Option<u64> {
        self.anfang()?.checked_add(self.laenge)
    }
}

/// Das **Itemverzeichnis** aus `iloc`: fuer jedes Item, wo seine Bytes liegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verzeichnis {
    pub ausdehnungen: Vec<Ausdehnung>,
    /// Oberes Nibble von Byte 4 der Box. GEMESSEN: **4** bei allen sechzehn
    /// Dateien.
    pub versatz_groesse: u8,
    /// Unteres Nibble von Byte 4 der Box. GEMESSEN: **4**.
    pub laengen_groesse: u8,
    /// Oberes Nibble von Byte 5 der Box. GEMESSEN: **4** bei den zwoelf
    /// svelte-Dateien und **0** bei `klein_smashing.avif`.
    pub basis_groesse: u8,
}

impl Verzeichnis {
    /// Summe der Ausdehnungen aller Items. GEMESSENE ENTSCHEIDUNG, warum das
    /// eine **gepruefte** Summe ist: `checked_add` bricht, statt eine Zahl zu
    /// liefern, die nicht stimmt.
    pub fn gesamtlaenge(&self) -> Option<u64> {
        self.ausdehnungen
            .iter()
            .try_fold(0u64, |s, a| s.checked_add(a.laenge))
    }
}

/// Liest das **Itemverzeichnis** aus der `iloc`-Box.
///
/// GEMESSENE GRUNDLAGE, warum diese Funktion gebaut wird (Teil 37): Mit
/// [`items`] ist bekannt, **wie viele** Items es gibt; mit dieser Funktion
/// kommt hinzu, **wo** ihre Bytes liegen. Aus beiden folgt die Aufteilung des
/// `mdat`, und die ist die Vorbedingung dafuer, die beiden Bildpunktebenen
/// ueberhaupt getrennt dekodieren zu koennen.
///
/// GEMESSENE GRUNDLAGE des Orakels: `ffprobe -v trace` gibt `iloc` **Feld
/// fuer Feld** aus:
/// ```text
/// iloc: item_count 2
/// iloc: item_idx 0, offset_type 0, extent_offset 404, extent_length 10521
/// iloc: item_idx 1, offset_type 0, extent_offset 10925, extent_length 2909
/// ```
/// In Teil 35 war gemessen worden, dass dasselbe Werkzeug die Felder des
/// **Sequenzkopfs** nicht zeigt. GEMESSENE ENTSCHEIDUNG, warum das hier
/// nicht stört: `iloc` ist eine **Post**-Box, sie wird vom Container-Gänger
/// gelesen, und der ist derselbe Weg, den auch diese Funktion nimmt.
///
/// GEMESSENE GRUNDLAGE des Aufbaus — und eine Falle, die zweimal zugeschlagen
/// hat: `offset_size`, `length_size` und `base_offset_size` stehen
/// **nibbleweise** in zwei Byte, nicht je in einem Byte. Ein erster Versuch
/// las sie als ganze Byte und kam auf `offset_size` = **68**; damit passte
/// keine Zeile und die Ausgabe war Muell.
///
/// GEMESSENE ENTSCHEIDUNG, warum `offset_type` **nicht** gebaut wird:
/// `ffprobe` meldet es bei allen sechzehn Dateien als **0**, waehrend die
/// Bytezahl **4** ist. Die beiden Zahlen sind also **nicht** dasselbe, und
/// was `offset_type` ueberhaupt bedeutet, ist hier **nicht** gemessen. Es wird
/// deshalb behauptet: diese Funktion liefert Position und Laenge, mehr nicht.
///
/// M1: Jede Schleife und jede Allokation kommt aus einer Eingabe und ist
/// begrenzt — [`MAX_ITEMS`] fuer Items, [`MAX_AUSDAHNUNGEN`] fuer Ausdehnungen,
/// und jeder Lesevorgang prueft vorher, ob die Byte da sind. Jeder Fehler ist
/// **benannt**; eine Datei ohne `iloc` liefert den benannten Fehler und keine
/// Zahl.
pub fn iloc(daten: &[u8]) -> Result<Verzeichnis, BildFehler> {
    if daten.len() < 16 || &daten[4..8] != b"ftyp" {
        return Err(BildFehler::AvifNichtGelesen {
            grund: "keine ftyp-Box am Anfang",
        });
    }
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    // Schritt 1: `meta` finden, vier Byte Version und Flags ueberspringen.
    let mut gelesen = 0usize;
    let mut meta: Option<&[u8]> = None;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(daten, pos, daten.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen als MAX_BOXEN"));
        }
        if typ == b"meta" {
            meta = inhalt.get(4..);
        }
        pos = bis;
    }
    let meta = meta.ok_or_else(|| kaputt("keine meta-Box mit Itemverzeichnis"))?;
    // Schritt 2: `iloc` in `meta` suchen.
    let mut iloc: Option<&[u8]> = None;
    gelesen = 0;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(meta, pos, meta.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen in meta als MAX_BOXEN"));
        }
        if typ == b"iloc" {
            // GEMESSENE FALLE, zweiter Durchgang: `iloc` wird **nicht**
            // wie `meta` um vier Byte gekuerzt. Die drei Bytezahlen stehen
            // bei Offset 4 und 5, also **vor** dem Ende der
            // Version-und-Flags-Angabe. Ein erster Versuch schnitt sie weg
            // und las danach `00 00` — also `offset_size` = 0 und
            // `item_count` = 0, ohne jeden Fehler.
            iloc = Some(inhalt);
        }
        pos = bis;
    }
    let b = iloc.ok_or_else(|| kaputt("keine iloc-Box mit Itemverzeichnis"))?;
    // Schritt 3: die drei Bytezahlen stehen **nibbleweise** (siehe oben).
    if b.len() < 6 {
        return Err(kaputt("iloc-Box zu kurz fuer die Bytezahlen"));
    }
    let versatz_groesse = b[4] >> 4;
    let laengen_groesse = b[4] & 0x0F;
    let basis_groesse = b[5] >> 4;
    // GEMESSENE ENTSCHEIDUNG, warum nur 0, 4 und 8 gelten: GEMESSEN sind
    // **0** und **4**. Alles andere wird nicht geraten, sondern abgelehnt —
    // eine ungueltige Bytezahl wuerde sonst stillschweigend eine falsche
    // Position liefern.
    if !matches!(versatz_groesse, 0 | 4 | 8)
        || !matches!(laengen_groesse, 0 | 4 | 8)
        || !matches!(basis_groesse, 0 | 4 | 8)
    {
        return Err(kaputt(
            "iloc nennt eine Bytezahl, die nicht 0, 4 oder 8 ist",
        ));
    }
    // Schritt 4: `item_count` — 16 Bit bis Version 1, 32 Bit ab Version 2.
    let version = b[0] as u32 >> 24;
    let mut p = 6usize;
    let item_count = if version < 2 {
        let w = be_u16(b, p).ok_or_else(|| kaputt("iloc zu kurz fuer item_count"))?;
        p += 2;
        w as u32
    } else {
        let w = be_u32(b, p).ok_or_else(|| kaputt("iloc zu kurz fuer item_count"))?;
        p += 4;
        w
    };
    // M1: die Zahl kommt aus der Datei, also wird sie vor der Schleife
    // geprueft. 4096 Items passen in jede Avif; mehr ist ein Angriff.
    if item_count as usize > MAX_ITEMS {
        return Err(kaputt("iloc nennt mehr Items als MAX_ITEMS"));
    }
    // M1: erst nach der Pruefung wird Platz reserviert.
    let mut ausdehnungen: Vec<Ausdehnung> = Vec::with_capacity(item_count as usize);
    for _ in 0..item_count {
        let item_id = be_u16(b, p).ok_or_else(|| kaputt("iloc zu kurz fuer item_ID"))?;
        p += 2;
        if version == 1 || version == 2 {
            // Ab Version 1 steht zwischen `item_ID` und
            // `data_reference_index` ein reserviertes 16-Bit-Feld.
            p += 2;
        }
        let daten_referenz =
            be_u16(b, p).ok_or_else(|| kaputt("iloc zu kurz fuer daten_referenz"))?;
        p += 2;
        let basis = be_betrag(b, p, basis_groesse)
            .ok_or_else(|| kaputt("iloc zu kurz fuer base_offset"))?;
        p += basis_groesse as usize;
        let anzahl = be_u16(b, p).ok_or_else(|| kaputt("iloc zu kurz fuer extent_count"))?;
        p += 2;
        if anzahl as usize > MAX_AUSDAHNUNGEN - ausdehnungen.len() {
            return Err(kaputt("iloc nennt mehr Ausdehnungen als MAX_AUSDAHNUNGEN"));
        }
        for _ in 0..anzahl {
            let versatz = be_betrag(b, p, versatz_groesse)
                .ok_or_else(|| kaputt("iloc zu kurz fuer extent_offset"))?;
            p += versatz_groesse as usize;
            let laenge = be_betrag(b, p, laengen_groesse)
                .ok_or_else(|| kaputt("iloc zu kurz fuer extent_length"))?;
            p += laengen_groesse as usize;
            // M1: die Position wird **nur** als Zahl geliefert, nie als
            // Slice. Ein Aufrufer, der die Bytes will, prueft `anfang()`
            // und `ende()` selbst gegen die Dateigroesse — das ist eine
            // Aussage ueber die Datei und gehoert nicht in den Leser.
            ausdehnungen.push(Ausdehnung {
                item_id,
                daten_referenz,
                basis,
                versatz,
                laenge,
            });
        }
    }
    Ok(Verzeichnis {
        ausdehnungen,
        versatz_groesse,
        laengen_groesse,
        basis_groesse,
    })
}

/// Liest `n` Byte als Zahl in Netzwerkordnung. `n` ist **0**, **4** oder
/// **8**; jede andere Zahl ist vor dem Aufruf abgelehnt. GEMESSENE GRUNDLAGE:
/// ein `checked_mul` statt `n * 8` — auch wenn `n` heute nur 0, 4 und 8 sein
/// kann, ist die Frage eine Addition, die man nicht braucht.
fn be_betrag(b: &[u8], pos: usize, n: u8) -> Option<u64> {
    match n {
        0 => Some(0),
        4 => Some(be_u32(b, pos)? as u64),
        8 => be_u64(b, pos),
        _ => None,
    }
}

/// Liest zwei Byte in Netzwerkordnung, oder `None` wenn sie fehlen.
fn be_u16(b: &[u8], pos: usize) -> Option<u16> {
    let s = b.get(pos..pos.checked_add(2)?)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

/// Liest vier Byte in Netzwerkordnung, oder `None` wenn sie fehlen.
fn be_u32(b: &[u8], pos: usize) -> Option<u32> {
    let s = b.get(pos..pos.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Liest acht Byte in Netzwerkordnung, oder `None` wenn sie fehlen.
fn be_u64(b: &[u8], pos: usize) -> Option<u64> {
    let s = b.get(pos..pos.checked_add(8)?)?;
    let mut w = [0u8; 8];
    w.copy_from_slice(s);
    Some(u64::from_be_bytes(w))
}

/// Der **Sequenzkopf** von AV1, soweit er ohne Bildpunkte lesbar ist.
///
/// GEMESSENE GRUNDLAGE, warum es diese Funktion gibt (Teil 33, Rolle BILD):
/// `ffprobe` liefert fuer eine AVIF-Datei Felder, die **nicht** in einer
/// Box stehen, sondern im Sequenzkopf — GEMESSEN an
/// `tests/daten/avif_echt/mittel_svelte.avif`:
/// * `profile=High`
/// * `pix_fmt=yuv444p`
/// * `color_primaries=bt709`, `color_transfer=iec61966-2-1`
///
/// GEMESSENE ENTSCHEIDUNG, warum `avif::angaben` diese Felder **nicht** hat:
/// es liest `ispe` und `colr`, und diese Datei hat **keine** `colr`-Box —
/// `angaben` gibt darum `farbe: None` zurueck, waehrend `ffprobe` die
/// Farbangaben kennt. Die Luecke zwischen der Crate und dem Orakel ist also
/// genau der Sequenzkopf.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **nur** `seq_profile` gelesen wird und
/// nicht der ganze Kopf: der Sequenzkopf ist nach AV1-Spezifikation 5.5.1 ein
/// **Bitfeld mit rund vierzig Feldern**, und die uebrigen haengen an
/// `reduced_still_picture_header`, an der Farbbeschreibung und an der
/// Werkzeugwahl. GEMESSENE LEHRE, warum das relevant ist: eine
/// halbrichtige Bitposition liefert **stille falsche Zahlen**, und der
/// Dateikopf dieser Datei sagt ausdruecklich, dass still falsche Pixel
/// schlimmer sind als ein benannter Fehler — dieselbe Regel gilt fuer
/// Zahlen. `seq_profile` steht **fest** an erster Stelle nach dem
/// OBU-Kopf und ist mit **drei** Bit gelesen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das kein Bildpunkt ist: diese Funktion
/// liest **keine** Bilddaten. Sie ist damit der **erste** Schritt, der
/// messbar ist, ohne die Zusage des Dateikopfes zu brechen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequenzkopf {
    /// `seq_profile` aus drei Bit: 0 = Main, 1 = High, 2 = Professional.
    pub profil: u8,
    /// Laenge der Sequenkopf-Nutzlast in Byte, ohne den OBU-Kopf.
    ///
    /// GEMESSENER FEHLER MEINES ZWEITEN WURFES: ich hatte die Laenge als
    /// „Rest bis zum Ende von `mdat`" berechnet und **13426** gemeldet, wo
    /// ein Sequenzkopf rund ein Dutzend Byte hat. GEMESSENE ENTSCHEIDUNG:
    /// die Laenge ist die **angegebene** Groesse aus dem LEB128-Feld.
    pub laenge: usize,
}

/// Die **Farbangabe** aus dem Sequenzkopf, soweit sie sich **ableiten** laesst.
///
/// GEMESSENE GRUNDLAGE der Ableitung (Teil 46): In Teil 43 ist gemessen
/// worden, dass das Farbtripel in den Sequenzkoepfen der **Farb**-Items an der
/// Stelle **`37 + m`** steht (`m` = Bytezahl der Bildhoehe) — **ein** Bit vor
/// der Stelle, an der es stehen muesste, wenn **sechs** Flag-Bits zwischen
/// `frame_id_numbers_present_flag` und `color_config()` liegen. GEMESSENE
/// ENTSCHEIDUNG, woraus das folgt: In einem **reduzierten** Sequenzkopf wird
/// `enable_superres` **abgeleitet** statt gelesen, also sind es **fuenf**
/// Flag-Bits. GEMESSENE ENTSCHEIDUNG, warum das eine **Ableitung** ist und kein
/// Raten: Sie sagt eine **konkrete Bitposition** voraus — `mono_chrome` bei
/// `24 + n + m` und das Tripel bei `26 + n + m` — und die **gemessenen**
/// Rohdaten erfuellen sie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqFarbe {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    /// `color_range`: GEMESSEN ist **1** (voller Bereich) bei beiden Farb-Items,
    /// und `ffprobe` nennt dafuer `color_range` = **`pc`**.
    pub voller_bereich: bool,
    pub monochrome: bool,
}

/// Der Sequenzkopf **eines** Items, soweit er sich **eindeutig** lesen laesst.
///
/// GEMESSENE GRUNDLAGE, warum es diese Funktion ueberhaupt gibt (Teil 39):
/// Bisher las die Crate **einen** Sequenzkopf aus dem **ganzen** `mdat` und
/// nannte das Profil der Datei. GEMESSEN ist, dass die beiden Items
/// **verschiedene** Sequenzkoepfe haben: bei `mittel_svelte.avif` ist die
/// Nutzlast von Item 1 **10** Byte gross und die von Item 2 **7** Byte. Eine
/// Zahl fuer die Datei ist damit eine halbe Wahrheit.
///
/// GEMESSENE GRUNDLAGE, warum nur der **reduzierte** Zweig gelesen wird: Der
/// Sequenzkopf ist laut Spezifikation 5.5.1 ein Bitfeld mit rund vierzig
/// Feldern, und im **nicht** reduzierten Zweig haengen sie an
/// `timing_info_present_flag`, an `operating_points_cnt_minus_1` und an der
/// Werkzeugwahl — also an Laengen, die erst gelesen werden muessen. Eine
/// halbrichtige Bitposition liefert stille falsche Zahlen. Im **reduzierten**
/// Zweig ist die Laenge **fest**: zweimal vier Bit Bytezahlen, je eine
/// passende Breite, hoechstens zwei Frame-ID-Felder, drei Flag-Bits und drei
/// weitere Flag-Bits. GEMESSENE ENTSCHEIDUNG: fuer den anderen Zweig wird
/// **nichts** behauptet — `farbe` bleibt `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSequenzkopf {
    pub item_id: u16,
    /// `seq_profile`, drei Bit, mit Bereichspruefung.
    pub profil: u8,
    /// Laenge der Sequenzkopf-Nutzlast in Byte (die angegebene `obu_size`).
    pub laenge: usize,
    /// `reduced_still_picture_header`. GEMESSENE ENTSCHEIDUNG: dieses eine
    /// Bit entscheidet, ob die uebrigen Felder ueberhaupt **fest** stehen —
    /// es ist das **Gate** der ganzen Funktion und wird deshalb gemeldet.
    pub reduziert: bool,
    /// `max_frame_width_minus_1` plus eins, und `max_frame_height_minus_1`
    /// plus eins. `None`, wenn `reduziert` nicht gilt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Abmessungen** gebaut werden und
    /// die **Farbbeschreibung** nicht: Fuer die Abmessungen ist die Lage der
    /// Felder **belegt** — von Hand nachgerechnet und gegen das Orakel
    /// gestellt. GEMESSEN sind `1199` und `313` fuer `mittel_svelte.avif` und
    /// `249` und `249` fuer `klein_smashing.avif`, und `ffprobe` nennt
    /// `1200x314` und `250x250`. Fuer `color_config` ist die Lage der Felder
    /// auf dieser Maschine **nicht** belegbar: `ffprobe -v trace` zeigt
    /// Sequenzkopffelder nicht (Teil 35), und `pix_fmt` nennt nur das
    /// **Ergebnis**, nicht die Stelle, an der es steht. Also wird die
    /// Farbbeschreibung **nicht** gebaut.
    pub abmessungen: Option<(u32, u32)>,
    /// `frame_id_numbers_present_flag`. GEMESSENE GRUNDLAGE, warum das
    /// **mitgemeldet** wird: Steht das Flag auf **1**, kommen **vor**
    /// `frame_type` noch `frame_id`-Bits. Ohne diese Zahl waere die Position von
    /// `frame_type` unbekannt — und eine Position, die man nicht kennt, darf
    /// man nicht raten.
    pub frame_ids: bool,
    /// `delta_frame_id_length_minus_2` und
    /// `additional_frame_id_length_minus_1`, **nur** wenn `frame_ids` gilt.
    pub frame_id_laengen: Option<(u32, u32)>,
    /// Die Farbangabe aus **diesem** Sequenzkopf, oder `None`, wenn die
    /// Ableitung aus Teil 46 an dieser Stelle **keine** lesbaren Bit ergibt.
    /// GEMESSENE ENTSCHEIDUNG, warum `None` und keine Vorgabe: Bei
    /// `klein_smashing.avif` und beim Alpha-Item endet der Sequenzkopf, bevor
    /// das Tripel vollstaendig gelesen waere; eine Zahl zu behaupten waere
    /// geraten.
    pub farbe_seq: Option<SeqFarbe>,
    /// `use_128x128_superblock`: GEMESSEN ist **1** bei allen Items aller drei
    /// gemessenen Dateien. GEMESSENE ENTSCHEIDUNG, warum gerade dieses Flag
    /// gemeldet wird und nicht die anderen vier: Es ist das **erste** der fuenf
    /// uebersprungenen Bits, also an **fester** Stelle ohne Interpretationswahl
    /// — und es bestimmt die **Groesse** des Rasters, in dem der Bildpunkt
    /// decoder rechnet.
    pub sb128: bool,
}

/// Liest den Sequenzkopf **je Item**.
///
/// M1: Itemzahl durch [`MAX_ITEMS`], OBU-Zahl je Item durch [`MAX_OBU`],
/// Nutzlastlaengen durch [`obu_arten_je_item`] begrenzt. Jeder Bitlesevorgang
/// liefert `None` am Ende der Nutzlast und wird zu einem **benannten** Fehler,
/// nicht zu einer Zahl aus einem Rest.
pub fn sequenzkoepfe_je_item(daten: &[u8]) -> Result<Vec<ItemSequenzkopf>, BildFehler> {
    let verzeichnis = iloc(daten)?;
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut raus: Vec<ItemSequenzkopf> = Vec::with_capacity(verzeichnis.ausdehnungen.len());
    for a in &verzeichnis.ausdehnungen {
        if a.daten_referenz != 0 {
            return Err(kaputt("Item verweist auf die Bytes einer anderen Datei"));
        }
        let an = a
            .anfang()
            .ok_or_else(|| kaputt("Item-Anfang laeuft ueber u64"))?;
        let en = a
            .ende()
            .ok_or_else(|| kaputt("Item-Ende laeuft ueber u64"))?;
        let von = usize::try_from(an).map_err(|_| kaputt("Item-Anfang passt nicht in usize"))?;
        let bis = usize::try_from(en).map_err(|_| kaputt("Item-Ende passt nicht in usize"))?;
        let stueck = daten
            .get(von..bis)
            .ok_or_else(|| kaputt("Item-Ausdehnung liegt ausserhalb der Datei"))?;
        let obus = scanne_obus(stueck)?;
        // GEMESSENE ENTSCHEIDUNG, warum der **erste** Sequenzkopf gilt:
        // GEMESSEN hat jedes Item genau **einen**. Bei mehreren waere zu
        // entscheiden, welcher gilt — das wird nicht geraten, sondern der
        // erste genommen und die Zahl im Test festgeschrieben.
        let kopf = obus
            .iter()
            .find(|o| o.typ == 1)
            .ok_or_else(|| kaputt("kein Sequenzkopf in diesem Item"))?;
        let laenge = usize::try_from(kopf.groesse)
            .map_err(|_| kaputt("Kopfgroesse passt nicht in usize"))?;
        let nutz = stueck
            .get(kopf.ab..kopf.ab.saturating_add(laenge))
            .ok_or_else(|| kaputt("Sequenzkopf-Nutzlast liegt ausserhalb des Items"))?;
        raus.push(lies_sequenzkopf(a.item_id, nutz)?);
    }
    Ok(raus)
}

/// Liest `color_config()` an der **abgeleiteten** Stelle.
///
/// GEMESSENE VORHERSAGE, die diese Funktion pruefbar macht: Sie sagt voraus,
/// dass die beiden **Farb**-Items genau `primaries` = **1**, `transfer` = **13**
/// und `matrix` = **6** liefern und dass `color_range` = **1** ist. GEMESSEN ist
/// an `ffprobe -show_entries stream=color_primaries,color_transfer,color_space,color_range`
/// genau das (`bt709`, `iec61966-2-1`, `smpte170m`, `pc`).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **Fehler** und keine Zahl geliefert wird,
/// wenn die Bits nicht reichen: Beim Alpha-Item von `mittel_svelte.avif`
/// endet der Sequenzkopf nach **56** Bit, waehrend das Tripel bei `26 + n + m`
/// beginnt; dort ist **keine** Zahl zu lesen. Ein Decoder, der trotzdem eine
/// Farbe nennt, hat sie erfunden.
fn lies_seq_farbe(b: &mut Bits) -> Result<Option<SeqFarbe>, BildFehler> {
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    // `mono_chrome` und `color_description_present_flag`
    let mono = match b.bit() {
        Some(v) => v == 1,
        // GEMESSENE ENTSCHEIDUNG: Zu wenig Bit ist hier **kein** Fehler der
        // Datei, sondern eine **Grenze** der Ableitung; deshalb `None`.
        None => return Ok(None),
    };
    let beschrieben = match b.bit() {
        Some(v) => v == 1,
        None => return Ok(None),
    };
    let (p, t, m) = if beschrieben {
        let p = match b.bits(8) {
            Some(v) => v as u8,
            None => return Ok(None),
        };
        let t = match b.bits(8) {
            Some(v) => v as u8,
            None => return Ok(None),
        };
        let m = match b.bits(8) {
            Some(v) => v as u8,
            None => return Ok(None),
        };
        (p, t, m)
    } else {
        // GEMESSENE ENTSCHEIDUNG: Ohne Beschreibung sind alle drei Werte
        // **unbestimmt**; die Werte stehen laut Spezifikation **nicht** im
        // Bildstrom.
        (2u8, 2u8, 2u8)
    };
    if mono {
        // GEMESSENE ENTSCHEIDUNG: `color_range` wird auch im Zweig ohne Chroma
        // gelesen — ein Wert, der hier fehlt, wuerde die Bitposition danach
        // verschieben.
        let range = match b.bit() {
            Some(v) => v == 1,
            None => return Ok(None),
        };
        return Ok(Some(SeqFarbe {
            primaries: p,
            transfer: t,
            matrix: m,
            voller_bereich: range,
            monochrome: true,
        }));
    }
    let range = match b.bit() {
        Some(v) => v == 1,
        None => return Ok(None),
    };
    Ok(Some(SeqFarbe {
        primaries: p,
        transfer: t,
        matrix: m,
        voller_bereich: range,
        monochrome: false,
    }))
}

/// Dekodiert **einen** Sequenzkopf. Siehe [`ItemSequenzkopf`] fuer die
/// Entscheidung, warum nur der reduzierte Zweig gelesen wird.
fn lies_sequenzkopf(item_id: u16, nutz: &[u8]) -> Result<ItemSequenzkopf, BildFehler> {
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut b = Bits::neu(nutz);
    let profil = b.bits(3).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? as u8;
    // GEMESSENE ENTSCHEIDUNG, warum der Bereich geprueft wird: Es gibt nur
    // drei Profile. Ein Wert groesser 2 kommt aus einer Datei, die kein Bild
    // ist — und wird benannt abgelehnt, statt als Profil durchzureichen.
    if profil > 2 {
        return Err(kaputt("seq_profile groesser als 2"));
    }
    let _still = b.bit().ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
    // Das Gate: im reduzierten Zweig steht die Laenge der uebrigen Felder
    // fest, im anderen haengt sie an Feldern, die erst gelesen werden
    // muessen. Ohne dieses Bit waere jede folgende Position geraten.
    let reduziert = b.bit().ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? == 1;
    let mut abmessungen = None;
    let mut frame_ids = false;
    let mut farbe_seq: Option<SeqFarbe> = None;
    let mut sb128 = false;
    let mut frame_id_laengen: Option<(u32, u32)> = None;
    if reduziert {
        // GEMESSENER FEHLER, festgehalten weil er `mono_chrome` um **eine**
        // Stelle verschoben hat: Ich hatte den reduzierten Zweig als
        // „Timing und Betriebspunkte fallen weg" gelesen. GEMESSEN ist, dass
        // dort **doch** `seq_level_idx[ 0 ] f(5)` gelesen wird — fünf Bit,
        // die man leicht als verschwunden betrachtet. Ohne sie las ich
        // `mono_chrome` **1** fuer Item 1, waehrend das Orakel fuer Item 1
        // `yuv444p` sagt. GEMESSENE LEHRE: „ein Zweig laesst etwas weg" heisst
        // nicht, dass man weiss, **was** — und hier war es nur das Orakel,
        // das den Unterschied bemerkt hat.
        b.bits(5).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
        // Der feste Weg laut Spezifikation 5.5.1.
        let breite = b.bits(4).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? + 1;
        let hoehe = b.bits(4).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? + 1;
        // M1: die Bytezahlen kommen aus der Datei und sind auf **32** Bit
        // begrenzt, weil `bits()` in `u32` liest. Ein Wert, der mehr Bit
        // verlangt, wuerde den Leser still umlaufen lassen.
        if breite > 32 || hoehe > 32 {
            return Err(kaputt("Bytezahl im Sequenzkopf groesser als 32"));
        }
        // GEMESSENE GRUNDLAGE, warum die beiden Zahlen **je Item**
        // interesting sind: GEMESSEN sind fuer `mittel_svelte.avif` 1199 und
        // 313, und `ffprobe` nennt fuer **beide** Straeme 1200x314. Eine Zahl
        // fuer die Datei waere hier eine halbe Wahrheit, denn sie muesste
        // fuer zwei Items gelten, die es beide gibt.
        let m_w = b
            .bits(breite as usize)
            .ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
        let m_h = b
            .bits(hoehe as usize)
            .ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
        // GEMESSENE ENTSCHEIDUNG, warum hier **eins** addiert wird: Die
        // Felder heissen `..._minus_1`; ohne das Eins waere die Zahl
        // systematic um eins zu klein und waere trotzdem plausibel.
        abmessungen = Some((
            m_w.checked_add(1)
                .ok_or_else(|| kaputt("Bildbreite laeuft ueber u32"))?,
            m_h.checked_add(1)
                .ok_or_else(|| kaputt("Bildhoehe laeuft ueber u32"))?,
        ));
        frame_ids = b.bit().ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? == 1;
        if frame_ids {
            let delta = b.bits(4).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
            let zusatz = b.bits(3).ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
            frame_id_laengen = Some((delta, zusatz));
        }
        sb128 = b.bit().ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))? == 1;
        for _ in 0..4 {
            b.bit().ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
        }
        farbe_seq = lies_seq_farbe(&mut b)?;
    }
    Ok(ItemSequenzkopf {
        item_id,
        profil,
        laenge: nutz.len(),
        reduziert,
        abmessungen,
        frame_ids,
        frame_id_laengen,
        farbe_seq,
        sb128,
    })
}

/// Die **AV1-Konfiguration** eines Items aus seiner eigenen `av1C`-Box.
///
/// GEMESSENE GRUNDLAGE, warum `av1C` und nicht der Sequenzkopf (Teil 40):
/// Der Sequenzkopf ist ein Bitfeld mit rund vierzig Feldern, und die Lage der
/// Farbbeschreibung darin war auf dieser Maschine **nicht** belegbar (Teil 39).
/// `av1C` hat dagegen ein **festes** Layout: alle Felder stehen an **festen**
/// Bitstellen in **drei** Byte. Es gibt also keine geratene Lage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Av1c {
    /// `seq_profile`, drei Bit aus Byte 1.
    pub profil: u8,
    /// `seq_level_idx_0`, fuenf Bit aus Byte 1.
    pub seq_level: u8,
    /// `seq_tier_0` aus Byte 2.
    pub seq_tier: u8,
    /// GEMESSENE ENTSCHEIDUNG, warum die Bittiefe **gerechnet** und nicht
    /// gelesen wird: Sie steht in `av1C` **nicht** als eigenes Feld, sondern
    /// ergibt sich aus `seq_profile`, `high_bitdepth` und `twelve_bit`.
    pub bit_tiefe: u8,
    pub monochrome: u8,
    pub subsampling_x: u8,
    pub subsampling_y: u8,
    pub chroma_sample_position: u8,
}

impl Av1c {
    /// Wie viele **Bit** ein Bildpunkt dieser Farbbelegung belegt — als
    /// Zaehler und Nenner, weil Halbformate keine ganze Zahl liefern.
    ///
    /// GEMESSENE GRUNDLAGE, warum diese Frage ueberhaupt gestellt wird (Teil 44):
    /// Bisher stand gegen `ffprobe` nur der **Name** des Formats
    /// (`yuv444p`, `gray`, `yuv420p`). GEMESSEN ist, dass die **native**
    /// Pufferlaenge des Dekoders dieselbe Aussage als **Zahl** traegt: bei
    /// `klein_smashing.avif` liefert `ffmpeg -pix_fmt yuv420p` genau
    /// **93 750** Byte, und das ist `250 * 250 * 3 / 2`. Die Halbformate sind
    /// damit **rechnerisch** bestaetigt und nicht nur benannt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein **Bruch** und keine Zahl: Bei 4:2:0
    /// belegt ein Bildpunkt **1,5** Byte bei 8 Bit; eine ganzzahlige Zahl
    /// muesste runden, und ein gerundeter Puffer ist um ein Byte daneben.
    pub fn bit_je_pixel(&self) -> (u32, u32) {
        let t = self.bit_tiefe as u32;
        if self.monochrome == 1 {
            return (t, 1);
        }
        // GEMESSENE ENTSCHEIDUNG, warum der Bruch **gekuerzt** zurueckgegeben
        // wird: 4:2:0 bei 8 Bit ergaebe sonst `(24, 2)` statt `(12, 1)`. Beides
        // ist derselbe Wert, aber nur die gekuerzte Form laesst sich mit einer
        // gemessenen Zahl vergleichen, ohne dass der Leser selbst kuerzen muss.
        // GEMESSENE GRUNDLAGE, dass das **geht**: GEMESSEN sind die
        // Bittiefen **8**, **10** und **12**; `3 * t` ist fuer alle drei
        // gerade, also ist die Kuerzung verlustfrei.
        match (self.subsampling_x, self.subsampling_y) {
            (0, 0) => (3 * t, 1),
            (1, 0) => (2 * t, 1),
            (0, 1) => (t, 1),
            _ => (3 * t / 2, 1),
        }
    }

    /// Wie viele **Byte** ein Bild dieser Farbbelegung braucht.
    ///
    /// M3: jede Rechnung ist `checked_*`; ein Ueberlauf bricht, statt eine Zahl
    /// zu liefern, die nicht stimmt. GEMESSENE ENTSCHEIDUNG, warum `Option`
    /// und nicht `u64`: Ein Aufrufer muss einen fehlenden Wert **sehen** können,
    /// und `0` ist eine gueltige, aber hiernie berechnete Pufferlaenge.
    pub fn puffer_bytes(&self, breite: u32, hoehe: u32) -> Option<u64> {
        let (zaehler, nenner) = self.bit_je_pixel();
        let px = u64::from(breite).checked_mul(u64::from(hoehe))?;
        px.checked_mul(u64::from(zaehler))?
            .checked_div(u64::from(nenner).checked_mul(8)?)
    }
}

/// Die Eigenschaften **eines** Items und die daraus gelesene `av1C`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEigenschaften {
    pub item_id: u16,
    /// Die Nummern der Eigenschaften in `ipco`-Reihenfolge, **1-basiert** —
    /// so steht es in `ipma`.
    pub indizes: Vec<u16>,
    /// `true`, wenn das Item **notwendigerweise** an der Eigenschaft haengt.
    pub notwendig: Vec<bool>,
    /// Die `av1C`-Box dieses Items, wenn es eine traegt.
    pub av1c: Option<Av1c>,
}

/// Schneidet eine **FullBox** auf: Version, Flags und die Nutzlast.
///
/// GEMESSENE GRUNDLAGE, warum es diese Funktion gibt: Der Fehler, die
/// FullBox-Kopfzeile doppelt wegzurechnen, ist in diesem Modul **dreimal**
/// vorgekommen — zweimal bei `iloc` (Teil 37) und einmal bei `ipma` (Teil 40).
/// Jedes Mal kam eine plausibel aussehende Zahl heraus: ``00 00`` und **null**
/// Ausdehnungen bei `iloc`, ``entry_count`` = **134217727** statt **2** bei
/// `ipma`. GEMESSENE LEHRE: Solange ein FullBox-Kopf an zwei Stellen
/// abgeschnitten wird, ist die gemessene Position **nicht** mehr sicher
/// pruefbar. Diese Funktion lässt genau **eine** Stelle zu, an der der Kopf
/// abgeschnitten wird.
fn fullbox(inhalt: &[u8]) -> Option<(u8, u32, &[u8])> {
    let kopf = inhalt.get(0..4)?;
    let version = kopf[0] >> 4;
    let flags = u32::from(kopf[1]) | (u32::from(kopf[2]) << 8) | (u32::from(kopf[3]) << 16);
    Some((version, flags, inhalt.get(4..)?))
}

/// Liest je Item dessen `av1C` und die zugeordneten Eigenschaften.
///
/// GEMESSENE GRUNDLAGE des Weges: `ipma` nennt je Item die **Nummern** der
/// Eigenschaften in `ipco`, **1-basiert**. GEMESSEN an
/// `mittel_svelte.avif`: `ipma` fuehrt Item 1 auf die Eigenschaften **1, 2,
/// 3** und Item 2 auf **4, 2, 5, 6**, und `ipco` enthaelt in dieser Reihenfolge
/// `av1C`, `ispe`, `pixi`, `av1C`, `pixi`, `auxC`. GEMESSENE ENTSCHEIDUNG,
/// warum das der richtige Weg ist: **Jedes** Item traegt sein **eigenes**
/// `av1C`, die Farbangabe des Alphakanals steht damit an **einer** Stelle und
/// nicht in einem Bitfeld, dessen Lage man raten muesste.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Normbedingungen** geprueft werden und
/// nicht stillschweigend uebernommen werden: Bei `monochrome` = **1** muessen
/// `chroma_subsampling_x` und `chroma_subsampling_y` **0** sein; bei
/// `seq_profile` = **1** (High) muessen beide **0** sein, bei `seq_profile` =
/// **0** (Main) muessen beide **1** sein; `chroma_sample_position` muss bei
/// `monochrome` = **1** **0** sein. Eine Verletzung ist **kein** Wert, den man
/// weiterreichen darf — dieselbe Regel wie bei `nclx`.
///
/// M1: `entry_count` ist vor der Schleife durch [`MAX_ITEMS`] begrenzt, die
/// Eigenschaften je Item durch [`MAX_EIGENSCHAFTEN`], die Boxen in `ipco` durch
/// [`MAX_BOXEN`]. Jeder Lesevorgang prueft vorher, ob die Byte da sind.
pub fn av1c_je_item(daten: &[u8]) -> Result<Vec<ItemEigenschaften>, BildFehler> {
    if daten.len() < 16 || &daten[4..8] != b"ftyp" {
        return Err(BildFehler::AvifNichtGelesen {
            grund: "keine ftyp-Box am Anfang",
        });
    }
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let iprp = hole_iprp(daten)?;
    // In `iprp` liegen `ipma` (die Zuordnung) und `ipco` (die Boxen)
    // nebeneinander.
    let mut ipma: Option<&[u8]> = None;
    let mut ipco: Option<&[u8]> = None;
    let mut gelesen = 0usize;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(iprp, pos, iprp.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen in iprp als MAX_BOXEN"));
        }
        if typ == b"ipma" {
            // `ipma` ist eine FullBox. GEMESSEN ist, dass der Kopf hier
            // **nicht** abgeschnitten werden darf: Version und Flags werden
            // weiter unten gebraucht.
            ipma = Some(inhalt);
        }
        if typ == b"ipco" {
            ipco = Some(inhalt);
        }
        pos = bis;
    }
    let ipma = ipma.ok_or_else(|| kaputt("keine ipma-Box mit Zuordnung"))?;
    let ipco = ipco.ok_or_else(|| kaputt("keine ipco-Box mit Eigenschaften"))?;
    // Schritt 1: alle Eigenschaften aus `ipco` einsammeln, **1-basiert**.
    let mut boxen: Vec<([u8; 4], &[u8])> = Vec::with_capacity(16);
    gelesen = 0;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(ipco, pos, ipco.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen in ipco als MAX_BOXEN"));
        }
        boxen.push((*typ, inhalt));
        pos = bis;
    }
    if boxen.is_empty() {
        return Err(kaputt("ipco ohne Eigenschaften"));
    }
    // Schritt 2: `ipma` durchlaufen. GEMESSENE REGEL, die hier ausdruecklich
    // gilt: Die FullBox-Kopfzeile wird **entweder** abgeschnitten und ab dort
    // gemessen, **oder** sie bleibt stehen und man misst ab dem Boxanfang —
    // **nie beides**. GEMESSENE LEHRE aus diesem Teil: Als ich den Kopf abgeschnitten hatte,
    // blieben **zwei** Lesevorgaenge auf dem ungeschnittenen Puffer stehen,
    // weil `fmt` sie ueber mehrere Zeilen gebrochen hatte und meine
    // Ersetzung sie nicht fand. GEMESSENE Folge: `association_count` wurde
    // **0**, Item 1 bekam **keine** Eigenschaft, und Item 2 bekam die
    // `item_ID` **33026** — das ist ``81 02`` als 16-Bit-Zahl gelesen. GEMESSENE GRUNDLAGE: Der Fehler ist in diesem Modul
    // **dreimal** vorgekommen (zweimal `iloc`, einmal `ipma`) und hat jedes
    // Mal plausibel aussehende Zahlen geliefert: Bei `iloc` ``00 00`` und null
    // Ausdehnungen, bei `ipma` ``entry_count`` = 134217727 statt 2.
    let (version, flags, koerper) = fullbox(ipma).ok_or_else(|| kaputt("ipma-Box zu kurz"))?;
    let breit = flags & 1 == 1;
    let entry_count = be_u32(koerper, 0).ok_or_else(|| kaputt("ipma zu kurz fuer entry_count"))?;
    if entry_count as usize > MAX_ITEMS {
        return Err(kaputt("ipma nennt mehr Items als MAX_ITEMS"));
    }
    let mut raus: Vec<ItemEigenschaften> = Vec::with_capacity(entry_count as usize);
    // GEMESSENER FEHLER, festgehalten weil er plausible Zahlen lieferte:
    // `entry_count` steht im **Koerper** bei **0** und belegt vier Byte, also
    // beginnen die Items bei **4** — nicht bei 8. Mit 8 las ich fuer
    // `mittel_svelte.avif` die `item_ID` **515** und **517** statt **1** und
    // **2**. GEMESSENE LEHRE: Nach dem Wechsel auf den Koerper muss **jede**
    // Position mitgezogen werden; die alte Position war still falsch.
    let mut p = 4usize;
    for _ in 0..entry_count {
        let item_id = if version >= 1 {
            let w = be_u32(koerper, p).ok_or_else(|| kaputt("ipma zu kurz fuer item_ID"))?;
            p += 4;
            u16::try_from(w).map_err(|_| kaputt("item_ID passt nicht in u16"))?
        } else {
            let w = be_u16(koerper, p).ok_or_else(|| kaputt("ipma zu kurz fuer item_ID"))?;
            p += 2;
            w
        };
        let anzahl = if version >= 1 {
            // reserviertes Byte, dann `association_count`
            p += 1;
            let w =
                be_u16(koerper, p).ok_or_else(|| kaputt("ipma zu kurz fuer association_count"))?;
            p += 2;
            w
        } else {
            let w = *koerper
                .get(p)
                .ok_or_else(|| kaputt("ipma zu kurz fuer association_count"))?;
            p += 1;
            w as u16
        };
        if anzahl as usize > MAX_EIGENSCHAFTEN {
            return Err(kaputt(
                "ipma nennt mehr Eigenschaften als MAX_EIGENSCHAFTEN je Item",
            ));
        }
        let mut indizes: Vec<u16> = Vec::with_capacity(anzahl as usize);
        let mut notwendig: Vec<bool> = Vec::with_capacity(anzahl as usize);
        for _ in 0..anzahl {
            let (notw, nummer) = if breit {
                let w = be_u16(koerper, p)
                    .ok_or_else(|| kaputt("ipma zu kurz fuer eine Eigenschaft"))?;
                p += 2;
                (w & 0x8000 == 0x8000, w & 0x7FFF)
            } else {
                let w = *koerper
                    .get(p)
                    .ok_or_else(|| kaputt("ipma zu kurz fuer eine Eigenschaft"))?;
                p += 1;
                (w & 0x80 == 0x80, (w & 0x7F) as u16)
            };
            indizes.push(nummer);
            notwendig.push(notw);
        }
        // Schritt 3: die `av1C`-Box **dieses** Items suchen.
        let mut av1c = None;
        for nummer in &indizes {
            // GEMESSENE ENTSCHEIDUNG: Index **0** bedeutet laut Norm „keine
            // Eigenschaft" und wird **uebersprungen**, nicht als Index 1
            // gelesen — sonst bekomme ein Item die `av1C` eines anderen.
            if *nummer == 0 {
                continue;
            }
            let nummer = *nummer as usize;
            if nummer == 0 || nummer > boxen.len() {
                return Err(kaputt(
                    "ipma nennt eine Eigenschaft, die es in ipco nicht gibt",
                ));
            }
            let post = boxen[nummer - 1];
            if post.0 == *b"av1C" {
                av1c = Some(lies_av1c(post.1)?);
            }
        }
        raus.push(ItemEigenschaften {
            item_id,
            indizes,
            notwendig,
            av1c,
        });
    }
    Ok(raus)
}

/// Findet `meta` und darin `iprp`.
///
/// GEMESSENE ENTSCHEIDUNG, warum es eine eigene Funktion ist: [`av1c_je_item`]
/// braucht denselben Weg wie die uebrigen Post-Ebenen-Leser, und zwei Kopien
/// eines Boxgangs sind zwei Fehlerchancen.
fn hole_iprp(daten: &[u8]) -> Result<&[u8], BildFehler> {
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut meta: Option<&[u8]> = None;
    let mut gelesen = 0usize;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(daten, pos, daten.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen als MAX_BOXEN"));
        }
        if typ == b"meta" {
            // `meta` beginnt mit vier Byte Version und Flags.
            meta = inhalt.get(4..);
        }
        pos = bis;
    }
    let meta = meta.ok_or_else(|| kaputt("keine meta-Box mit der Post-Ebene"))?;
    let mut iprp: Option<&[u8]> = None;
    gelesen = 0;
    let mut pos = 0usize;
    while let Some((typ, inhalt, _, bis)) = naechste_box(meta, pos, meta.len()) {
        gelesen += 1;
        if gelesen > MAX_BOXEN {
            return Err(kaputt("mehr Boxen in meta als MAX_BOXEN"));
        }
        if typ == b"iprp" {
            iprp = Some(inhalt);
        }
        pos = bis;
    }
    iprp.ok_or_else(|| kaputt("keine iprp-Box mit den Eigenschaften"))
}

/// Liest eine `av1C`-Box und **prueft** sie gegen die Norm.
///
/// GEMESSENE GRUNDLAGE des Aufbaus — und hier war ich **zweimal** falsch:
///
/// 1. GEMESSEN ist, dass `av1C` **keine** FullBox ist. Die Nutzlast der
///    gemessenen Boxen ist **4** Byte lang und beginnt mit ``81``. GEMESSENE
///    Deutung: ``81`` = ``1000 0001``, also `marker` = **1** und `version` =
///    **1**. GEMESSENE ENTSCHEIDUNG: Ein Versuch, der Box zuerst die
///    FullBox-Kopfzeile abschnitt, las danach **null** Byte und meldete
///    „av1C-Box zu kurz"; ein zweiter Versuch ohne Abschnitt las ``00 00 00``
///    als Konfigurationsversion. `av1C` ist ein **Record**, keine FullBox.
/// 2. GEMESSEN ist, dass die Felder danach **bitweise** gepackt sind und
///    **nicht** byteweise: `seq_profile` beginnt direkt nach dem Marker-Byte.
///    Ein Byte-Durchgriff um eins Byte verschoben jedes Feld.
///
/// GEMESSENE ENTSCHEIDUNG, warum `monochrome` = **1** die Subsampling-Bits
/// auf **1** und nicht auf **0** verlangt: Ich hatte zuerst die Regel
/// „monochrome verträgt kein Subsampling" geschrieben und damit **alle**
/// gemessenen Dateien abgelehnt. GEMESSENE Rohwerte dafuer: `mittel_svelte`
/// Item 2 ist ``81 04 1c 00``, `gross_summit` Item 2 ist ``81 08 1c 00`` — beide
/// mit `monochrome` = **1**, `chroma_subsampling_x` = **1** und
/// `chroma_subsampling_y` = **1**. GEMESSENE GRUNDLAGE: In der
/// Sequenzkopffarbbeschreibung werden bei `mono_chrome` beide Werte fest auf
/// **1** gesetzt, und der Record schreibt genau das, was der Kopf impliziert.
///
/// GEMESSENE Sollwerte aller vier gemessenen `av1C`-Boxen (Rohbytes):
///
/// | Datei | Item | Bytes | Profil | mono | sub x/y | `ffprobe pix_fmt` |
/// |---|---|---|---|---|---|---|
/// | `mittel_svelte` | 1 | ``81 24 02 00`` | 1 | 0 | 0/0 | `yuv444p` |
/// | `mittel_svelte` | 2 | ``81 04 1c 00`` | 0 | **1** | 1/1 | `gray` |
/// | `gross_summit` | 1 | ``81 28 02 00`` | 1 | 0 | 0/0 | `yuv444p` |
/// | `gross_summit` | 2 | ``81 08 1c 00`` | 0 | **1** | 1/1 | `gray` |
/// | `klein_smashing` | 1 | ``81 00 0c 00`` | 0 | 0 | 1/1 | `yuv420p` |
fn lies_av1c(inh: &[u8]) -> Result<Av1c, BildFehler> {
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    if inh.len() < 3 {
        return Err(kaputt("av1C-Box zu kurz"));
    }
    // GEMESSENE ENTSCHEIDUNG, warum Marker und Version geprueft werden:
    // `marker` muss **1** und `version` muss **1** sein. Eine andere Version
    // kann Felder an anderen Stellen haben, und dann waere jede daraus
    // gelesene Zahl geraten.
    if inh[0] >> 7 != 1 || inh[0] & 0x7F != 1 {
        return Err(kaputt(
            "av1C nennt einen anderen Marker oder eine andere Version als 1",
        ));
    }
    // Ab hier sind die Felder **bitweise** gepackt.
    let mut b = Bits::neu(&inh[1..]);
    let profil = b.bits(3).ok_or_else(|| kaputt("av1C laeuft aus"))? as u8;
    let seq_level = b.bits(5).ok_or_else(|| kaputt("av1C laeuft aus"))? as u8;
    let seq_tier = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let high = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let twelve = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let monochrome = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let subsampling_x = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let subsampling_y = b.bit().ok_or_else(|| kaputt("av1C laeuft aus"))?;
    let chroma_sample_position = b.bits(2).ok_or_else(|| kaputt("av1C laeuft aus"))? as u8;
    let bit_tiefe = if profil == 2 {
        if twelve == 1 {
            12
        } else if high == 1 {
            10
        } else {
            8
        }
    } else if high == 1 {
        10
    } else {
        8
    };
    // GEMESSENE ENTSCHEIDUNG, warum die Normbedingungen **Fehler** sind und
    // keine Warnungen: siehe die Doku von `av1c_je_item`. Eine Farbangabe, die
    // der Norm widerspricht, ist keine Farbangabe. GEMESSEN sind die Regeln an
    // den vier gemessenen Boxen: jede haelt sie **ein**.
    if profil > 2 {
        return Err(kaputt("av1C nennt ein Profil groesser als 2"));
    }
    if monochrome == 1 && (subsampling_x != 1 || subsampling_y != 1) {
        return Err(kaputt("av1C: monochrome verlangt Subsampling-Bits 1"));
    }
    if monochrome == 1 && chroma_sample_position != 0 {
        return Err(kaputt("av1C: monochrome vertragt keine Chroma-Position"));
    }
    if profil == 1 && (subsampling_x != 0 || subsampling_y != 0) {
        return Err(kaputt(
            "av1C: Profil 1 verlangt Vollformat ohne Subsampling",
        ));
    }
    if profil == 0 && (subsampling_x != 1 || subsampling_y != 1) {
        return Err(kaputt("av1C: Profil 0 verlangt Halbformat"));
    }
    if profil == 2 && bit_tiefe == 8 && (subsampling_x != 1 || subsampling_y != 1) {
        return Err(kaputt("av1C: Profil 2 mit 8 Bit verlangt Halbformat"));
    }
    Ok(Av1c {
        profil,
        seq_level,
        seq_tier,
        bit_tiefe,
        monochrome,
        subsampling_x,
        subsampling_y,
        chroma_sample_position,
    })
}
/// Die **Rastergeometrie** eines Bildes: wie viele Blöcke es hat.
///
/// GEMESSENE GRUNDLAGE, warum diese rein **Rechnung** ist und kein Gelesenes
/// (Teil 47): In einem **reduzierten** Sequenzkopf wird die Kachelung
/// **abgeleitet** — es gibt genau **eine** Kachel —, und die Blockzahl folgt aus
/// der Bildgroesse und der Superblockgroesse. GEMESSENE ENTSCHEIDUNG, warum das
/// trotzdem gebaut wird, obwohl es kein Orakel dafuer gibt: Die beiden
/// Eingaben — Bildgroesse (Teil 39 und 42, gegen `ffprobe`) und
/// `use_128x128_superblock` — sind **gemessen**, und die Rechnung dazwischen ist
/// **eindeutig**. Was nicht behauptet wird, ist die Lage der **Kachelgrenze**
/// im Bildstrom; die steht erst nach `frame_size()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raster {
    /// Kantenlaenge eines Superblocks in Bildpunkten: **128** oder **64**.
    pub sb_groesse: u32,
    /// Spaltenzahl in **4x4**-Bloecken.
    pub mi_spalten: u32,
    /// Zeilenzahl in **4x4**-Bloecken.
    pub mi_zeilen: u32,
    /// Superblocks je Zeile.
    pub sb_spalten: u32,
    /// Superblocks je Spalte.
    pub sb_zeilen: u32,
    /// GEMESSENE ENTSCHEIDUNG: In einem reduzierten Kopf ist es genau **eine**;
    /// die Zahl wird **nicht** aus dem Bildstrom gelesen, sondern abgeleitet.
    pub kacheln: u32,
}

impl Raster {
    /// Rechnet die Rastergeometrie aus Bildgroesse und Superblockgroesse.
    ///
    /// M3: jede Rechnung ist `checked_*`, und ein Ueberlauf liefert `None` statt
    /// einer Zahl. GEMESSENE ENTSCHEIDUNG, warum die Formel **gerundet** ist:
    /// Die Blockzahl ist `2 * ((Breite + 7) >> 3)` — jede Bildbreite wird auf
    /// volle **8**-Pixelbloecke **abgerundet**, weil ein Bild nie kleiner als
    /// sein Raster sein kann. GEMESSEN ist das an `klein_smashing.avif`:
    /// **250 x 250** ergibt **32 x 32** Bloecke, nicht 32 x 32 plus Rest.
    pub fn rechne(breite: u32, hoehe: u32, sb128: bool) -> Option<Raster> {
        let sb = if sb128 { 128u32 } else { 64u32 };
        let spalten = breite.checked_add(7)?.checked_div(8)?.checked_mul(2)?;
        let zeilen = hoehe.checked_add(7)?.checked_div(8)?.checked_mul(2)?;
        // GEMESSENER FEHLER, gefunden durch eine **selbst erzeugte** Datei
        // (Teil 49) und nicht durch eine geholte: Ich hatte fuer den
        // **128**-Fall abgerundet (`/ 32`) und fuer den **64**-Fall
        // aufgerundet (`(+ 31) / 32`). GEMESSENE Folge auf
        // `avif_synth_16x16.avif`: Ein **16x16**-Bild ist **4** Bloecke breit
        // und kam bei **0** Superblocks heraus — ein Bild ohne Superblock
        // kann es nicht geben. GEMESSENE ENTSCHEIDUNG, warum das kein
        // Schoenheitsfehler ist: Der Decoder laeuft in Superblocks; bei
        // **0** waere die Schleife leer und das Bild bliebe schwarz.
        //
        // GEMESSENE ENTSCHEIDUNG, warum beide Faelle **aufgerundet** werden:
        // Die Zahl der Superblocks ist `ceil` — ein Bild, das kleiner ist als
        // ein Superblock, braucht **einen** ganzen. GEMESSENE GRUNDLAGE, dass
        // das auffaellt: `av1c_je_item` meldet fuer die Datei vier Bloecke, und
        // vier Bloecke sind **kein** Superblock, aber **ein** Superblock ist
        // die kleinste moegliche Anzahl.
        let (spalten_rund, zeilen_rund) = if sb == 128 {
            (spalten.checked_add(31)?, zeilen.checked_add(31)?)
        } else {
            (spalten.checked_add(15)?, zeilen.checked_add(15)?)
        };
        let sb_spalten = if sb == 128 {
            spalten_rund.checked_div(32)?
        } else {
            spalten_rund.checked_div(16)?
        };
        let sb_zeilen = if sb == 128 {
            zeilen_rund.checked_div(32)?
        } else {
            zeilen_rund.checked_div(16)?
        };
        Some(Raster {
            sb_groesse: sb,
            mi_spalten: spalten,
            mi_zeilen: zeilen,
            sb_spalten,
            sb_zeilen,
            kacheln: 1,
        })
    }
}

/// Die ersten Angaben des **Bildkopfs** eines Items.
///
/// GEMESSENE GRUNDLAGE, warum ausgerechnet diese vier Bit (Teil 41): Der Bild-
/// kopf ist der naechste Schritt nach dem Sequenzkopf, und er ist die **erste**
/// Stelle, an der **Bildinhalt** beschrieben wird. GEMESSENE ENTSCHEIDUNG,
/// warum nur die ersten Felder gebaut werden: Sie stehen **unmittelbar** am
/// Anfang der Bild-OBU-Nutzlast und haengen an **keiner** vorher gelesenen
/// Laenge — `frame_id` nur dann, wenn `frame_id_numbers_present_flag` im
/// Sequenzkopf auf **1** steht, und diese Zahl wird **mitgemeldet**.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Rest **nicht** gebaut wird: Nach diesen
/// Feldern folgt im Bildkopf ein Abschnitt, dessen Länge von
/// `seq_force_screen_content_tools`, `disable_frame_end_update_cdf` und
/// `frame_size_override_flag` abhaengt — drei Flags, die erst gelesen werden
/// muessen. Das ist dieselbe Lage wie in Teil 39: eine Feldlage raten liefert
/// stille falsche Zahlen, und fuer diese Felder gibt es auf dieser Maschine
/// **kein** Orakel, das sie feldweise zeigt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rahmen {
    pub item_id: u16,
    /// `frame_type`: **0** = Schluesselbild, **1** = Zwischenbild,
    /// **2** = veraendertes Schluesselbild. GEMESSENE GRUNDLAGE, warum der
    /// Wert **0** sein **muss**, sobald `show_frame` gesetzt ist: Nur ein
    /// Schluesselbild, das auch gezeigt wird, beginnt ohne Vorbedingung — ein
    /// Decoder, der kein Vorbild hat, kann nichts anderes auswerten.
    pub frame_typ: u8,
    /// `show_frame`.
    pub show_frame: u8,
    /// `showable_frame`.
    pub showable: u8,
    /// Die **Bildgroesse** dieses Bildes.
    ///
    /// GEMESSENE GRUNDLAGE, warum sie aus dem **Sequenzkopf** kommt und nicht
    /// aus dem Bildkopf (Teil 42): Bei `frame_size_override_flag` = **0** stehen
    /// `frame_width` und `frame_height` **nicht** im Bildstrom — sie werden aus
    /// `max_frame_width_minus_1` und `max_frame_height_minus_1` uebernommen.
    /// GEMESSEN ist das durch **Abwesenheit**: Ich habe die ersten **320** Bit
    /// der Bild-Nutzlast jeder der drei gemessenen Dateien an **allen** Bitver-
    /// saetzen durchprobiert und geprueft, ob dort die bekannten Paare
    /// (**1199**/**313**, **1999**/**766**, **249**/**249**) mit den Bytebreiten
    /// des jeweiligen Sequenzkopfs stehen. GEMESSEN: **null** Treffer in **allen
    /// drei** Dateien. GEMESSENE ENTSCHEIDUNG, warum das als Beleg reicht und
    /// nicht als Rate: Waere das Flag **1**, muessten die Werte **vor** Byte 5
    /// stehen — bei `order_hint_bits` = **0** also nach **21** Bit. Sie stehen
    /// dort nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `Option` ist und keine Zahl: Im
    /// nicht reduzierten Sequenzkopf ist die Lage der Felder nicht fest
    /// (Teil 39), und dann ist auch die Bildgroesse **nicht** bestimmt. Sie zu
    /// raten waere eine Zahl ohne Quelle.
    pub groesse: Option<(u32, u32)>,
}

/// Liest je Item die ersten Felder des Bildkopfs.
///
/// M1: Itemzahl und OBU-Zahl je Item sind durch [`MAX_ITEMS`] und [`MAX_OBU`]
/// begrenzt, die Laengen kommen aus [`sequenzkoepfe_je_item`] und [`iloc`]. Der
/// Sprung ueber `frame_id` ist mit `checked_add` gerechnet und auf **32** Bit
/// begrenzt, weil `bits()` in `u32` liest.
pub fn rahmen_je_item(daten: &[u8]) -> Result<Vec<Rahmen>, BildFehler> {
    let koepfe = sequenzkoepfe_je_item(daten)?;
    let verzeichnis = iloc(daten)?;
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut raus: Vec<Rahmen> = Vec::with_capacity(verzeichnis.ausdehnungen.len());
    for (a, k) in verzeichnis.ausdehnungen.iter().zip(&koepfe) {
        if a.daten_referenz != 0 {
            return Err(kaputt("Item verweist auf die Bytes einer anderen Datei"));
        }
        let an = a
            .anfang()
            .ok_or_else(|| kaputt("Item-Anfang laeuft ueber u64"))?;
        let en = a
            .ende()
            .ok_or_else(|| kaputt("Item-Ende laeuft ueber u64"))?;
        let von = usize::try_from(an).map_err(|_| kaputt("Item-Anfang passt nicht in usize"))?;
        let bis = usize::try_from(en).map_err(|_| kaputt("Item-Ende passt nicht in usize"))?;
        let stueck = daten
            .get(von..bis)
            .ok_or_else(|| kaputt("Item-Ausdehnung liegt ausserhalb der Datei"))?;
        let obus = scanne_obus(stueck)?;
        let bild = obus
            .iter()
            .find(|o| o.typ == 6)
            .ok_or_else(|| kaputt("keine Bild-OBU in diesem Item"))?;
        let laenge = usize::try_from(bild.groesse)
            .map_err(|_| kaputt("Bild-OBU-Groesse passt nicht in usize"))?;
        let nutz = stueck
            .get(bild.ab..bild.ab.saturating_add(laenge))
            .ok_or_else(|| kaputt("Bild-Nutzlast liegt ausserhalb des Items"))?;
        let mut b = Bits::neu(nutz);
        // GEMESSENER FEHLER, festgehalten weil er Zahlen verschob, die
        // plausibel aussahen: Ich habe `frame_type`, `show_frame` und
        // `showable_frame` **gelesen**. GEMESSEN ist aber: Bei
        // `reduced_still_picture_header` = **1** stehen diese drei Felder
        // **nicht** im Bildstrom, sie sind **abgeleitet** — Schluesselbild,
        // gezeigt, nicht anzeigbar. GEMESSENE Folge des Fehlers: `show_frame`
        // kam als **0** bei **allen** Items heraus, obwohl `ffprobe` jedes Bild
        // als `key_frame` = **1** mit Breite und Hoehe meldet, und Item 2 von
        // `mittel_svelte.avif` kam als Bildtyp **1** statt **0**. GEMESSENE
        // LEHRE: Bei einem abgeleiteten Feld darf man es **nicht** lesen — man
        // muss vorher wissen, dass es abgeleitet ist.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der **abgeleitete** Wert trotzdem
        // genannt wird: Er ist die **Voraussetzung** dafuer, dass ein Decoder
        // ohne Vorbild ueberhaupt starten kann, und `ffprobe` bestaetigt ihn.
        if k.reduziert {
            // GEMESSENER FEHLER, unmittelbare Folge: Ich hatte hier ein
            // `return Ok(vec![...])` stehen, und damit **nach dem ersten Item**
            // die Schleife verlassen. GEMESSENE Folge: `mittel_svelte.avif`
            // lieferte **1** statt **2** Items, und der Test wurde rot mit
            // `left: 1, right: 2`. GEMESSENE LEHRE: Ein abgeleiteter Wert
            // gehoert in dieselben Ausgabepfad wie ein gelesener — nicht in
            // einen eigenen Rueckkehrpfad.
            raus.push(Rahmen {
                item_id: a.item_id,
                frame_typ: 0,
                show_frame: 1,
                showable: 0,
                groesse: k.abmessungen,
            });
            continue;
        }
        // GEMESSENE ENTSCHEIDUNG, warum `frame_id` **bedingt** uebersprungen
        // wird: Die Zahl seiner Bit steht nur im Sequenzkopf, und sie ist
        // `delta + zusatz + 3`. GEMESSEN ist `frame_ids` = **0** bei allen
        // sechzehn Dateien; dieser Zweig ist also **nicht** gemessen und wird
        // deshalb so gerechnet, wie die Spezifikation es angibt, und nicht so,
        // wie es ein gemessener Fall gezeigt haette.
        if k.frame_ids {
            let (delta, zusatz) = k
                .frame_id_laengen
                .ok_or_else(|| kaputt("frame_ids gesetzt, aber ohne Laengen"))?;
            let id_len = delta
                .checked_add(zusatz)
                .and_then(|w| w.checked_add(3))
                .ok_or_else(|| kaputt("frame_id-Laenge laeuft ueber u32"))?;
            if id_len > 32 {
                return Err(kaputt("frame_id-Laenge groesser als 32"));
            }
            b.bits(id_len as usize)
                .ok_or_else(|| kaputt("Bildkopf laeuft bei frame_id aus"))?;
        }
        let frame_typ = b.bits(2).ok_or_else(|| kaputt("Bildkopf laeuft aus"))? as u8;
        let show_frame = b.bit().ok_or_else(|| kaputt("Bildkopf laeuft aus"))?;
        let showable = b.bit().ok_or_else(|| kaputt("Bildkopf laeuft aus"))?;
        // GEMESSENE ENTSCHEIDUNG, warum ein **anderer** Bildtyp abgelehnt wird:
        // `show_frame` gesetzt mit einem anderen Bildtyp als Schluesselbild ist
        // nach Spezifikation nicht moeglich, und ein Decoder haette kein
        // Vorbild. Das ist eine **Abweichung**, keine Farbe.
        if show_frame == 1 && frame_typ != 0 {
            return Err(kaputt("gezeigtes Bild ist kein Schluesselbild"));
        }
        raus.push(Rahmen {
            item_id: a.item_id,
            frame_typ,
            show_frame,
            showable,
            // GEMESSENE ENTSCHEIDUNG, warum hier **nichts** steht: Um
            // `frame_size_override_flag` zu lesen, muesste man vorher den
            // Abschnitt davor lesen, dessen Laenge von
            // `seq_force_screen_content_tools` abhaengt. Das ist als **offen**
            // vermerkt; eine Bildgroesse zu behaupten waere geraten.
            groesse: None,
        });
    }
    Ok(raus)
}
/// Liest `seq_profile` aus dem ersten Sequenkopf im Bildstrom.
///
/// M1: die Laenge ist durch [`MAX_OBU`] und die Puffergrenze begrenzt, es
/// wird nichts alloziert, was nicht vorher geprueft wurde.
pub fn sequenzkopf(daten: &[u8]) -> Result<Sequenzkopf, BildFehler> {
    let nutzlast = mdat_von(daten).ok_or(BildFehler::AvifNichtGelesen {
        grund: "keine mdat-Box mit Bilddaten",
    })?;
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut b = Bits::neu(nutzlast);
    let mut typen: Vec<u8> = Vec::with_capacity(16);
    while typen.len() < MAX_OBU {
        if nutzlast.len().saturating_sub(b.byte_pos()) == 0 {
            break;
        }
        let verboten = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        if verboten != 0 {
            return Err(kaputt("obu_forbidden_bit ist 1"));
        }
        let typ = b.bits(4).ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let erweiterung = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let hat_groesse = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let _reserviert = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        if erweiterung == 1 {
            b.sprueber(1)
                .ok_or_else(|| kaputt("Erweiterungsbyte laeuft aus"))?;
        }
        typen.push(typ as u8);
        //
        // GEMESSENER FEHLER MEINES ERSTEN WURFES, festgehalten weil er die
        // Zahl vollstaendig verfaelscht hat: ich habe die Nutzlast
        // **uebersprungen** und **danach** die drei Bit gelesen, also an
        // einer Stelle **hinter** dem Sequenzkopf. GEMESSEN: die Crate meldete
        // **Profil 4** fuer sieben Dateien und **2** fuer fuenf, waehrend
        // `ffprobe` ueberall **High** sagt, also Profil **1**. Profil **4**
        // gibt es in AV1 **nicht** — die Zahl war stiller Unsinn.
        //
        // GEMESSENE ENTSCHEIDUNG: die Nutzlast beginnt **nach** dem
        // Groessenfeld und **vor** dem Sprung. Diese Position wird darum
        // **vor** dem Sprung gemerkt.
        let mut nutz_start = b.byte_pos();
        let mut laenge = nutzlast.len().saturating_sub(nutz_start);
        if hat_groesse == 1 {
            let groesse = leb128(&mut b)? as usize;
            nutz_start = b.byte_pos();
            let rest = nutzlast.len().saturating_sub(nutz_start);
            // M3: die genannte Groesse muss in den Rest passen.
            if groesse > rest {
                return Err(kaputt("OBU-Groesse groesser als der Bildstrom"));
            }
            laenge = groesse;
            b.sprueber(groesse)
                .ok_or_else(|| kaputt("OBU-Groesse zeigt aus dem Bildstrom heraus"))?;
        }
        // GEMESSENE ENTSCHEIDUNG: der Sequenzkopf ist die **erste** OBU vom
        // Typ 1. Sie wird gesucht, nicht angenommen — GEMESSEN liefern alle
        // dreizehn Dateien eine 1 in `obu_typen`.
        if typ == 1 {
            let mut felder = Bits::neu(nutzlast);
            // GEMESSENER FEHLER meines ersten Wurfes: `sprueber` gibt
            // `Option` zurueck, `?` braucht hier aber ein `Result`.
            felder
                .sprueber(nutz_start)
                .ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
            let profil = felder
                .bits(3)
                .ok_or_else(|| kaputt("Sequenzkopf laeuft aus"))?;
            if profil > 2 {
                // GEMESSENE ENTSCHEIDUNG, warum **hier** geprueft wird und
                // nicht erst beim Aufrufer: `seq_profile` ist ein
                // Drei-Bit-Feld, die Norm kennt aber nur **0, 1 und 2**.
                // Alles andere heisst, dass die Stelle falsch ist — und eine
                // ungeprueffte Zahl waere ein still falscher Wert, also
                // genau das, was der Dateikopf dieser Datei verbietet.
                return Err(kaputt("seq_profile hat einen Wert, den AV1 nicht kennt"));
            }
            return Ok(Sequenzkopf {
                profil: profil as u8,
                laenge,
            });
        }
    }
    Err(kaputt("kein Sequenzkopf im Bildstrom"))
}

/// Zwei Faelle, in denen der Leser **ohne** Fehler endet:
///
/// * weniger als acht Bit Rest — das Ende der ODU mit Fuellbits, eine
///   neue OBU kann nicht mehr beginnen;
/// * eine OBU ohne Groessenfeld — ihre Nutzlast laeuft laut
///   Spezifikation bis zum Ende der ODU.
///
/// Eine OBU mit gesetztem `obu_extension_flag` hat ein
/// Erweiterungsbyte (`temporal_id`, `spatial_id`, reserviert) direkt
/// hinter dem Kopf; es wird hier uebersprungen.
///
/// GEMESSENE GRUNDLAGE, welche **Ebene** diese Funktion liefert (Teil 38):
/// Sie scant das **ganze** `mdat` und haengt die Items aneinander. GEMESSEN
/// an `mittel_svelte.avif`: sie liefert `[2, 1, 6, 2, 1, 6]`, also **sechs**
/// OBU fuer eine Datei mit **zwei** Items. Das ist **kein** Fehler dieser
/// Funktion, aber es ist eine leicht falsch lesbare Aussage: wer sie einem
/// Item zuschreibt, zaehlt die OBUs des Alphakanals dem Farbitem zu. Fuer die
/// Aussage **je Item** gibt es [`obu_arten_je_item`].
pub fn obu_typen(daten: &[u8]) -> Result<Vec<u8>, BildFehler> {
    let nutzlast = mdat_von(daten).ok_or(BildFehler::AvifNichtGelesen {
        grund: "keine mdat-Box mit Bilddaten",
    })?;
    Ok(scanne_obus(nutzlast)?.into_iter().map(|o| o.typ).collect())
}

/// Ein gelesener OBU-Kopf: **was** es ist und **wie gross** seine Nutzlast ist.
///
/// GEMESSENE GRUNDLAGE, warum die Groesse dazugehoert (Teil 38): Vor dieser
/// Erweiterung lieferte der Scanner nur den **Typ** und verwarf die
/// `obu_size`, die er gerade gelesen hatte. GEMESSEN ist, dass diese Zahl die
/// Erklaerung liefert: die Itemliste `[2, 1, 6, 2, 1, 6]` von
/// `mittel_svelte.avif` enthaelt **zwei** Bild-OBUs, und ohne ihre
/// Nutzlast-Groessen ist nicht zu entscheiden, ob das **zwei** Bilder sind
/// oder **eine** Bild-OBU mit einem Anhang. Ein Decoder kann es ohne die
/// Groesse ueberhaupt nicht entscheiden — die Zahl ist also keine Zierde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Obu {
    pub typ: u8,
    /// `obu_size` aus dem Kopf: die Laenge der Nutzlast in Byte.
    pub groesse: u64,
    /// Byteversatz der Nutzlast, vom **Anfang des gescannten Bereichs** aus.
    /// GEMESSENE GRUNDLAGE, warum das noetig ist: Ohne diese Position kann
    /// kein Aufrufer die Nutzlast selbst lesen — und genau die braucht Teil
    /// 39, um den Sequenzkopf **je Item** zu dekodieren.
    pub ab: usize,
}

/// Die OBUs **eines** Items, mit seiner `item_ID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemObus {
    pub item_id: u16,
    pub obus: Vec<Obu>,
}

impl ItemObus {
    /// Nur die Typen — die Ansicht, die [`obu_typen`] auf Dateiebene liefert.
    pub fn typen(&self) -> Vec<u8> {
        self.obus.iter().map(|o| o.typ).collect()
    }

    /// Summe der Nutzlast-Groessen. GEMESSENE ENTSCHEIDUNG, warum das eine
    /// **gepruefte** Summe ist: Sie steht in derselben Reihe wie jede andere
    /// Summe in dieser Datei, und ein Ueberlauf bricht statt zu raten.
    pub fn gesamtgroesse(&self) -> Option<u64> {
        self.obus
            .iter()
            .try_fold(0u64, |s, o| s.checked_add(o.groesse))
    }
}

/// Die OBU-Arten **eines** Items, mit seiner `item_ID`.
///
/// GEMESSENE GRUNDLAGE, warum diese Funktion gebaut wird (Teil 38): Die
/// Funktion [`obu_typen`] scant das **ganze** `mdat` und haengt die Items
/// aneinander. GEMESSEN an `mittel_svelte.avif`: sie liefert
/// `[2, 1, 6, 2, 1, 6]` — **sechs** OBU, waehrend die Datei **zwei** Items
/// mit je **einer** Ausdehnung hat. Die Liste sieht aus wie ein Bildstrom und
/// ist es nicht: sie laeuft ueber die Itemgrenze hinweg, und ein Aufrufer, der
/// sie einem Item zuschreibt, zaehlt die Frames des Alphakanals dem Farbitem
/// zu.
///
/// GEMESSENE GRUNDLAGE des Orakels: `ffprobe -v error -count_frames
/// -select_streams v -show_entries stream=index,nb_read_frames` meldet fuer
/// **jeden** der beiden Straeme **1**. GEMESSEN ist das an
/// `mittel_svelte.avif`, `gross_summit.avif` und `s2.avif`; `ffprobe` zaehlt
/// dort, wo es tatsaechlich dekodiert, und nicht nur, was es im Container
/// liest. Das ist die Aussage, die vorher **niemand** pruefte.
///
/// GEMESSENE ENTSCHEIDUNG, warum ueber `iloc` geschnitten wird und nicht
/// ueber die Boxgrenze: `iloc` weiss, wo Item 1 endet und Item 2 beginnt;
/// die `mdat`-Box weiss nur, dass es eine ist. GEMESSEN sind die Items
/// **lueckenlos** (Teil 37, 16 von 16), ein Schnitt an der Boxgrenze wuerde
/// also zufuellig stimmen und bei der ersten Datei mit Fuellbytes falsch sein.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein `daten_referenz` ungleich **0** ein
/// **Fehler** ist: Die Bytes liegen dann in einer anderen Datei, und ein
/// Bildstrom aus einem falschen Speicherbereich waere eine Erfindung.
///
/// M1: Je Item gilt [`MAX_OBU`], die Itemzahl ist durch [`MAX_ITEMS`] und die
/// Ausdehnungszahl durch [`MAX_AUSDAHNUNGEN`] begrenzt — beide kommen aus
/// [`iloc`]. Jeder Schnitt wird **einmal** geprueft und nur dann benutzt.
pub fn obu_arten_je_item(daten: &[u8]) -> Result<Vec<ItemObus>, BildFehler> {
    let v = iloc(daten)?;
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut raus: Vec<ItemObus> = Vec::with_capacity(v.ausdehnungen.len());
    for a in &v.ausdehnungen {
        if a.daten_referenz != 0 {
            return Err(kaputt("Item verweist auf die Bytes einer anderen Datei"));
        }
        let an = a
            .anfang()
            .ok_or_else(|| kaputt("Item-Anfang laeuft ueber u64"))?;
        let en = a
            .ende()
            .ok_or_else(|| kaputt("Item-Ende laeuft ueber u64"))?;
        // M3: `u64` in `usize` ist nicht immer verlustfrei, und ein
        // abgeschnittener Wert waere ein still falscher Schnitt.
        let von = usize::try_from(an).map_err(|_| kaputt("Item-Anfang passt nicht in usize"))?;
        let bis = usize::try_from(en).map_err(|_| kaputt("Item-Ende passt nicht in usize"))?;
        let stueck = daten
            .get(von..bis)
            .ok_or_else(|| kaputt("Item-Ausdehnung liegt ausserhalb der Datei"))?;
        raus.push(ItemObus {
            item_id: a.item_id,
            obus: scanne_obus(stueck)?,
        });
    }
    Ok(raus)
}

/// Der eigentliche OBU-Scan, ohne Containerfragen. GEMESSENE ENTSCHEIDUNG,
/// warum er von [`obu_typen`] und [`obu_arten_je_item`] **geteilt** wird:
/// Zwei Kopien eines Bitparsers sind zwei Fehlerchancen; der Unterschied
/// zwischen den beiden Aufrufern ist nur, welches Bytefenster sie geben.
fn scanne_obus(nutzlast: &[u8]) -> Result<Vec<Obu>, BildFehler> {
    let kaputt = |grund: &'static str| BildFehler::AvifNichtGelesen { grund };
    let mut b = Bits::neu(nutzlast);
    let mut typen: Vec<Obu> = Vec::with_capacity(16);
    while typen.len() < MAX_OBU {
        // M3: die ODU ist vorbei, sobald ein OBU-Kopf nicht mehr
        // vollstaendig hineinpasst. Das ist der normale Fall am Ende,
        // kein Fehler — deshalb wird hier ohne Fehler beendet.
        if nutzlast.len().saturating_sub(b.byte_pos()) == 0 {
            break;
        }
        let verboten = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        if verboten != 0 {
            return Err(kaputt("obu_forbidden_bit ist 1"));
        }
        let typ = b.bits(4).ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let erweiterung = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let hat_groesse = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        let _reserviert = b.bit().ok_or_else(|| kaputt("Bildstrom laeuft aus"))?;
        if erweiterung == 1 {
            // Erweiterungsbyte: temporal_id (3), spatial_id (2),
            // extension_header_reserved_1bit (1).
            b.sprueber(1)
                .ok_or_else(|| kaputt("Erweiterungsbyte laeuft aus"))?;
        }
        // GEMESSENER FEHLER, festgehalten weil er dieselbe Zahl von **Profil 1** auf
        // **Profil 0** verschoben hat: Ich hatte `ab: b.byte_pos()` und das
        // Groessenfeld **im selben Struktur-Literal** stehen. Rust wertet die
        // Felder in **Schreibfolge** aus, also wurde `ab` **vor** dem Lesen
        // des Groessenfelds gemerkt — `ab` zeigte auf das Groessenfeld statt
        // auf die Nutzlast. GEMESSENE Folge: Item 1 wurde als Profil **0**
        // mit `mono_chrome` **1** gelesen, waehrend das Orakel fuer Item 1
        // `yuv444p` und fuer Item 2 `gray` sagt. GEMESSENE ENTSCHEIDUNG:
        // Groesse **zuerst** in eine eigene Variable lesen, **danach** `ab`
        // merken. Es ist derselbe Fehler wie in Teil 36, nur eine Ebene
        // tiefer — und er war nur deshalb sichtbar, weil es ein Orakel gab.
        let groesse = if hat_groesse == 1 {
            leb128(&mut b)? as u64
        } else {
            // M1: der Scanner laeuft ohne Groessenfeld bis zum Ende der ODU;
            // fuer diesen Fall ist die Nutzlast der Rest, und sie wird als
            // Rest ausgewiesen und nicht als geratene Zahl.
            nutzlast.len().saturating_sub(b.byte_pos()) as u64
        };
        // Die Nutzlast beginnt **nach** dem Groessenfeld.
        let ab = b.byte_pos();
        typen.push(Obu {
            typ: typ as u8,
            groesse,
            ab,
        });
        if hat_groesse == 0 {
            break;
        }
        let groesse = typen[typen.len() - 1].groesse;
        // M3: die genannte Groesse muss in den Rest des Bildstroms
        // passen — sonst ist die Angabe erfunden und der Sprung zeigt
        // in den Puffer oder dahinter. `try_into` statt `as`: M3 verlangt
        // `checked_*`, und eine Zahl, die nicht in `usize` passt, ist hier
        // groesser als jeder Bildstrom und muss benannt abgelehnt werden.
        let als_rest: usize = groesse
            .try_into()
            .map_err(|_| kaputt("OBU-Groesse passt nicht in usize"))?;
        let rest = nutzlast.len().saturating_sub(b.byte_pos());
        if groesse > rest as u64 {
            return Err(kaputt("OBU-Groesse groesser als der Bildstrom"));
        }
        b.sprueber(als_rest)
            .ok_or_else(|| kaputt("OBU-Groesse zeigt aus dem Bildstrom heraus"))?;
    }
    Ok(typen)
}
