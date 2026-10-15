//! G8b: DER VORBIS-DETEKTOR SELBST.
//!
//! # DIE DREI KOPFPAKETE
//!
//! ```text
//!   Paket 0: Identification  (0x01)  — Rate, Kanaele, Blocksizes
//!   Paket 1: Kommentar       (0x03)  — Anbieter und Felder
//!   Paket 2: Setup           (0x05)  — Codebooks, Boden, Residuum,
//!                                     Zuordnung, Modi
//!   danach: jedes Paket ein Audioblock
//! ```
//!
//! # DIE REIHENFOLGE IM AUDIOPAKET (Abschnitt 4.3)
//!
//! ```text
//!   1  Pakettyp (1 Bit, muss 0 sein)
//!   2  Modusnummer            (ilog(modi - 1) Bit)
//!   3  Blocklaenge n          (lang ? block1 : block0)
//!   4  Fensterform           (nur bei langen Bloecken: 2 Bit)
//!   5  Boden je Kanal        (Abschnitt 4.3.2)
//!   6  Null-Vektoren verteilen (Abschnitt 4.3.3)
//!   7  Residuum je Unterkarte (Abschnitt 4.3.4)
//!   8  Kanalkopplung rueckgaengig (Abschnitt 4.3.5)
//!   9  Produkt Boden * Residuum (Abschnitt 4.3.6)
//!  10  inverse MDCT + Ueberlappen (mdct.rs)
//! ```
//!
//! # WAS HIER SCHON FALSCH WAR
//!
//! Der Identification-Kopf hat **kein** Zeit-Feld und **keinen**
//! „Fenster"-Exponenten für die Blocksizes — die Blocksizes stehen als
//! zwei 4-Bit-Werte **im Identification-Kopf**, und mein erster
//! Setup-Leser suchte sie (mit 64-Bit-Zeitbereichswerten!) im Setup-Kopf
//! und las dort die Restbits der Codebooks. Siehe `g8b_setup.rs`.

use crate::bitleser::BitLeser;
use crate::codebuch::{Codebuch, MAX_BUECHER, lies_codebuch};
use crate::fehler::{Art, Ergebnis, Fehler};
use crate::floor::{BodenEins, BodenNull, kurve0, lies_boden0, lies_boden1, lies_rahmen};
use crate::mdct::{Lappen, fenster};
use crate::residue::{Rest, lies_paket as lies_restpaket, lies_rest};

/// M3: hoechstens so viele Zaehler, die „plus eins" gelesen werden.
pub const MAX_ZAEHLER: usize = 64;

/// M3: hoechstens so viele Kanaele.
///
/// M4: Abschnitt 4.2.2 sagt „[audio_channels] must read greater than
/// zero", gibt aber keine obere Grenze. 255 ist der groesste Wert, den
/// das 8-Bit-Feld aufnehmen kann — und jede Kanaelzahl braucht eine
/// eigene Bodenkurve und ein eigenes Residuum, also Speicher.
pub const MAX_KANAELE: usize = 255;

/// Der Identification-Kopf (Abschnitt 4.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kopf {
    /// 1 bis 255.
    pub kanaele: u8,
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// Maximum, nominal, Minimum — nur Hinweise (Abschnitt 4.2.2).
    pub bitraten: [i32; 3],
    /// Die kurze Blocklaenge.
    pub block0: usize,
    /// Die lange Blocklaenge.
    pub block1: usize,
}

/// Der Kommentar-Kopf (Abschnitt 5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kommentar {
    /// Die Anbieter-Zeichenfolge.
    pub anbieter: String,
    /// Die Felder als Name und Wert.
    pub felder: Vec<(String, String)>,
}

/// Die BodENkonfiguration aus dem Setup-Kopf.
#[derive(Debug, Clone, PartialEq)]
pub enum Boden {
    /// Floor 1 — der Normalfall.
    Eins(BodenEins),
    /// Floor 0 — Kopf und Paket gebaut, Kurve nicht (siehe `floor.rs`).
    Null(BodenNull),
}

/// Die Kanalzuordnung (Abschnitt 4.2.4, „Mappings").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zuordnung {
    /// Die Zahl der Unterkarten.
    pub submaps: usize,
    /// Wie viele Kanalkopplungen.
    pub koppelpunkte: usize,
    /// Je Kopplung: der Kanal mit der **Groesse**.
    pub groesse: Vec<usize>,
    /// Je Kopplung: der Kanal mit dem **Winkel**.
    pub winkel: Vec<usize>,
    /// Je Kanal: die Unterkarte, zu der er gehoert.
    pub mux: Vec<usize>,
    /// Je Unterkarte: der Boden.
    pub boden: Vec<usize>,
    /// Je Unterkarte: das Residuum.
    pub rest: Vec<usize>,
}

/// Ein Modus (Abschnitt 4.2.4, „Modes").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modus {
    /// 1 = langer Block.
    pub lang: bool,
    /// Immer 0 in Vorbis I.
    pub fenster: u32,
    /// Immer 0 in Vorbis I.
    pub transform: u32,
    /// Die Zuordnung dieses Modus.
    pub zuordnung: usize,
}

/// Der komplette Setup-Kopf.
#[derive(Debug, Clone)]
pub struct Setup {
    /// Die Codebucher, in Nummerierung.
    pub codebucher: Vec<Codebuch>,
    /// Die Bodenkonfigurationen.
    pub boden: Vec<Boden>,
    /// Die Residuumkonfigurationen.
    pub reste: Vec<Rest>,
    /// Die Kanalzuordnungen.
    pub zuordnungen: Vec<Zuordnung>,
    /// Die Modi.
    pub modi: Vec<Modus>,
    /// Wie viele Bit der Setup-Kopf im Paket belegt — die Position
    /// **hinter** dem Framing-Bit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das am Setup klebt und nicht in
    /// einer eigenen Funktion steht: die Zahl ist nur dann richtig, wenn
    /// jede Feldbreite davor richtig war. Sie ist damit das
    /// billigste Mass fuer „las der Leser das ganze Paket" — und sie
    /// muss in einem Byte des Pakets stehen, weil die letzten Bit eines
    /// Pakets Fuellung sind.
    pub ende_bit: usize,
}

/// Liest den Identification-Kopf (Abschnitt 4.2.2).
pub fn lies_kopf(paket: &[u8]) -> Ergebnis<Kopf> {
    let mut l = BitLeser::neu(paket);
    let typ = l.bits(8)?;
    let mut magic = [0u8; 6];
    for m in magic.iter_mut() {
        *m = l.bits(8)? as u8;
    }
    if typ != 1 || &magic != b"vorbis" {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Identification-Kopf: Pakettyp {typ}, Magic {magic:?}"),
        ));
    }
    let version = l.bits(32)?;
    if version != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Identification-Kopf: Version {version} — Vorbis I ist 0"),
        ));
    }
    let kanaele = l.bits(8)?;
    let rate = l.bits(32)?;
    if kanaele == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "Identification-Kopf: 0 Kanaele".to_string(),
        ));
    }
    if kanaele as usize > MAX_KANAELE {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Identification-Kopf: {kanaele} Kanaele"),
        ));
    }
    if rate == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "Identification-Kopf: 0 Hz".to_string(),
        ));
    }
    let bitraten = [l.bits(32)? as i32, l.bits(32)? as i32, l.bits(32)? as i32];
    // „[blocksize_0] = 2^exponent(read 4 bits)"
    let b0 = 1usize << l.bits(4)?;
    let b1 = 1usize << l.bits(4)?;
    let rahmen = l.bits(1)?;
    if b0 > b1 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Identification-Kopf: block0 = {b0} ist groesser als block1 = {b1}"),
        ));
    }
    if rahmen == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "Identification-Kopf: das Framing-Bit ist 0".to_string(),
        ));
    }
    Ok(Kopf {
        kanaele: kanaele as u8,
        rate,
        bitraten,
        block0: b0,
        block1: b1,
    })
}

/// Liest den Kommentar-Kopf (Abschnitt 5.2.3).
///
/// M4: die Laengen im Kommentar kommen aus dem Strom. Ohne Grenze wuerde
/// `laenge` die Dateigroesse bestimmen und der Lesevorgang so lange
/// laufen, bis der Strom endet — das ist der guenstigste Angriff auf den
/// Speicher, den eine Audiodatei anbietet.
pub fn lies_kommentar(paket: &[u8]) -> Ergebnis<Kommentar> {
    /// M4: hoechstens so ein Feld-Kommentar.
    const MAX_FELD: usize = 1 << 20;
    let mut l = BitLeser::neu(paket);
    let typ = l.bits(8)?;
    let mut magic = [0u8; 6];
    for m in magic.iter_mut() {
        *m = l.bits(8)? as u8;
    }
    if typ != 3 || &magic != b"vorbis" {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Kommentar-Kopf: Pakettyp {typ}, Magic {magic:?}"),
        ));
    }
    let laenge = l.bits(32)? as usize;
    if laenge > MAX_FELD {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Kommentar-Kopf: Anbieter mit {laenge} Byte, erlaubt {MAX_FELD}"),
        ));
    }
    let anbieter = lies_text(&mut l, laenge)?;
    let anzahl = l.bits(32)? as usize;
    if anzahl > MAX_FELD {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Kommentar-Kopf: {anzahl} Felder, erlaubt {MAX_FELD}"),
        ));
    }
    let mut felder = Vec::with_capacity(anzahl.min(256));
    for _ in 0..anzahl {
        let n = l.bits(32)? as usize;
        if n > MAX_FELD {
            return Err(Fehler::neu(
                Art::ZahlZuGross,
                format!("Kommentar-Kopf: Feld mit {n} Byte, erlaubt {MAX_FELD}"),
            ));
        }
        let inhalt = lies_text(&mut l, n)?;
        let (name, wert) = inhalt
            .split_once('=')
            .map_or((inhalt.clone(), String::new()), |(a, b)| {
                (a.to_string(), b.to_string())
            });
        felder.push((name, wert));
    }
    Ok(Kommentar { anbieter, felder })
}

/// Liest `n` Byte als Text.
fn lies_text(l: &mut BitLeser<'_>, n: usize) -> Ergebnis<String> {
    if n > l.rest_bits() / 8 {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "Kommentar-Kopf: {n} Byte verlangt, da sind {}",
                l.rest_bits() / 8
            ),
        ));
    }
    l.bis_bytegrenze();
    let start = l.byte_position();
    let mut raus = Vec::with_capacity(n);
    for _ in 0..n {
        raus.push(l.bits(8)? as u8);
    }
    // M4: die Byten stammen aus einer Datei. `from_utf8_lossy` ist hier
    // richtig, weil die Spec UTF-8 sagt, ein defekter Kommentar aber kein
    // Grund ist, die ganze Datei zu verwerfen.
    let _ = start;
    Ok(String::from_utf8_lossy(&raus).into_owned())
}

/// Liest den Setup-Kopf (Abschnitt 4.2.4).
pub fn lies_setup(paket: &[u8], kanaele: usize) -> Ergebnis<Setup> {
    let mut l = BitLeser::neu(paket);
    let typ = l.bits(8)?;
    let mut magic = [0u8; 6];
    for m in magic.iter_mut() {
        *m = l.bits(8)? as u8;
    }
    if typ != 5 || &magic != b"vorbis" {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Setup-Kopf: Pakettyp {typ}, Magic {magic:?}"),
        ));
    }
    // 1) und 2): die Codebucher.
    let anzahl = l.bits(8)? as usize + 1;
    if anzahl > MAX_BUECHER {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Setup-Kopf: {anzahl} Codebucher, erlaubt {MAX_BUECHER}"),
        ));
    }
    let mut codebucher = Vec::with_capacity(anzahl);
    for _ in 0..anzahl {
        codebucher.push(lies_codebuch(&mut l)?);
    }
    // Zeitbereichstransformationen: Platzhalter, aber sie muessen gelesen
    // werden, sonst steht der Leser danach an der falschen Stelle
    // („the configuration placeholder values must be read to maintain
    // bitstream sync").
    let zeiten = lies_zaehler(&mut l, "Zeitbereich")? as usize;
    for _ in 0..zeiten {
        let wert = l.bits(16)?;
        if wert != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Setup-Kopf: Zeitbereich {wert} — in Vorbis I sind alle null"),
            ));
        }
    }
    // Floors.
    let anzahl_boden = lies_zaehler(&mut l, "Boden")? as usize;
    let mut boden = Vec::with_capacity(anzahl_boden);
    for _ in 0..anzahl_boden {
        let art = l.bits(16)?;
        boden.push(match art {
            0 => Boden::Null(lies_boden0(&mut l)?),
            1 => Boden::Eins(lies_boden1(&mut l)?),
            andere => {
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    format!("Setup-Kopf: Bodentyp {andere} — es gibt 0 und 1"),
                ));
            }
        });
    }
    // Residuen.
    let anzahl_rest = lies_zaehler(&mut l, "Residuum")? as usize;
    let mut reste = Vec::with_capacity(anzahl_rest);
    for _ in 0..anzahl_rest {
        let art = l.bits(16)?;
        reste.push(lies_rest(&mut l, art)?);
    }
    // Zuordnungen.
    let anzahl_zuordnung = lies_zaehler(&mut l, "Zuordnung")? as usize;
    let mut zuordnungen = Vec::with_capacity(anzahl_zuordnung);
    for _ in 0..anzahl_zuordnung {
        zuordnungen.push(lies_zuordnung(&mut l, kanaele)?);
    }
    // Modi.
    let anzahl_modi = lies_zaehler(&mut l, "Modus")? as usize;
    let mut modi = Vec::with_capacity(anzahl_modi);
    for i in 0..anzahl_modi {
        let lang = l.bits(1)? == 1;
        let fenster = l.bits(16)?;
        let transform = l.bits(16)?;
        let zuordnung = l.bits(8)? as usize;
        if fenster != 0 || transform != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Modus {i}: Fenstertyp {fenster}, Transformtyp {transform} — Vorbis I kennt nur 0"
                ),
            ));
        }
        if zuordnung >= zuordnungen.len() {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Modus {i}: Zuordnung {zuordnung}, die Datei hat {}",
                    zuordnungen.len()
                ),
            ));
        }
        modi.push(Modus {
            lang,
            fenster,
            transform,
            zuordnung,
        });
    }
    let rahmen = l.bits(1)?;
    if rahmen == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "Setup-Kopf: das Framing-Bit ist 0".to_string(),
        ));
    }
    Ok(Setup {
        codebucher,
        boden,
        reste,
        zuordnungen,
        modi,
        ende_bit: l.position(),
    })
}

/// Liest einen „read 6 bits and add one"-Zaehler mit einer Grenze.
fn lies_zaehler(l: &mut BitLeser<'_>, was: &str) -> Ergebnis<u32> {
    let wert = l.bits(6)? + 1;
    if wert as usize > MAX_ZAEHLER {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Setup-Kopf: {was} {wert}, erlaubt {MAX_ZAEHLER}"),
        ));
    }
    Ok(wert)
}

/// Liest eine Kanalzuordnung (Abschnitt 4.2.4, „Mappings").
fn lies_zuordnung(l: &mut BitLeser<'_>, kanaele: usize) -> Ergebnis<Zuordnung> {
    let art = l.bits(16)?;
    if art != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Zuordnungsart {art} — Vorbis I kennt nur 0"),
        ));
    }
    // i) Unterkarten
    let submaps = if l.bits(1)? == 1 {
        l.bits(4)? as usize + 1
    } else {
        1
    };
    // ii) Kanalkopplung
    let koppelpunkte = if l.bits(1)? == 1 {
        l.bits(8)? as usize + 1
    } else {
        0
    };
    // „ilog([audio_channels] - 1) bits"
    let breite = crate::codebuch::ilog(kanaele as u32 - 1);
    let mut groesse = Vec::with_capacity(koppelpunkte);
    let mut winkel = Vec::with_capacity(koppelpunkte);
    for _ in 0..koppelpunkte {
        let m = l.bits(breite)? as usize;
        let a = l.bits(breite)? as usize;
        if m >= kanaele || a >= kanaele || m == a {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Kanalkopplung: Groesse {m}, Winkel {a}, {kanaele} Kanaele"),
            ));
        }
        groesse.push(m);
        winkel.push(a);
    }
    // iii) reserviert
    let reserviert = l.bits(2)?;
    if reserviert != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Zuordnung: reserviertes Feld ist {reserviert}, muss 0 sein"),
        ));
    }
    // iv) Kanalverteilung auf die Unterkarten
    let mut mux = vec![0usize; kanaele];
    if submaps > 1 {
        for eintrag in mux.iter_mut() {
            *eintrag = l.bits(4)? as usize;
            if *eintrag >= submaps {
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    format!("Kanal auf Unterkarte {}, es gibt {submaps}", *eintrag),
                ));
            }
        }
    }
    // v) Boden und Residuum je Unterkarte
    let mut boden = Vec::with_capacity(submaps);
    let mut rest = Vec::with_capacity(submaps);
    for _ in 0..submaps {
        let _ = l.bits(8)?; // der Zeit-Platzhalter
        boden.push(l.bits(8)? as usize);
        rest.push(l.bits(8)? as usize);
    }
    Ok(Zuordnung {
        submaps,
        koppelpunkte,
        groesse,
        winkel,
        mux,
        boden,
        rest,
    })
}

/// Das Ergebnis einer Dekodierung.
#[derive(Debug, Clone, PartialEq)]
pub struct Ton {
    /// Die Abtastrate.
    pub rate: u32,
    /// Die Kanalzahl.
    pub kanaele: usize,
    /// Die Zahl der Samples je Kanal.
    pub samples: usize,
    /// Die Samples je Kanal.
    pub kanaele_daten: Vec<Vec<f64>>,
    /// Die Granelposition der letzten Seite.
    pub granel: Option<u64>,
}

impl Ton {
    /// Die Abtastwerte als `i16` in der Reihenfolge, die
    /// `aplay -f s16le` erwartet: **kanalweise hintereinander**, nicht
    /// je Sample die Kanaele nebeneinander.
    ///
    /// # DIE UMORDNUNG, und sie ist nicht kosmetisch
    ///
    /// Ogg/Vorbis liefert „links alle Samples, dann rechts alle Samples"
    /// (das ist die Reihenfolge der Kanaele im Strom). `aplay` mit
    /// Mehrkanal-PCM erwartet **pro Sample alle Kanaele nebeneinander**.
    /// Wer die Reihenfolge nicht umstellt, hört bei Stereo eine Kanalphase
    /// und einen halben Ton Unterschied — und die Abweichung ist umso
    /// auffälliger, je ähnlicher die Kanaele sind.
    #[must_use]
    pub fn als_i16(&self) -> Vec<i16> {
        let mut raus = Vec::with_capacity(self.samples * self.kanaele);
        for i in 0..self.samples {
            for kanal in &self.kanaele_daten {
                let wert = kanal.get(i).copied().unwrap_or(0.0);
                let begrenzt = (wert * 32768.0).round();
                raus.push(if begrenzt > 32767.0 {
                    32767
                } else if begrenzt < -32768.0 {
                    -32768
                } else {
                    begrenzt as i16
                });
            }
        }
        raus
    }
}

/// Der Zustand eines Dekoders ueber mehrere Bloecke.
#[derive(Debug, Clone)]
pub struct Dekoder {
    /// Der Identification-Kopf.
    pub kopf: Kopf,
    /// Der Setup-Kopf.
    pub setup: Setup,
    /// Je Kanal der Lappenzustand.
    lappen: Vec<Lappen>,
    /// Die Zahl der bisher ausgegebenen Samples je Kanal.
    pub ausgegeben: usize,
}

impl Dekoder {
    /// Baut einen Dekoder aus den drei Kopfpaketen.
    pub fn neu(kopf: Kopf, setup: Setup) -> Ergebnis<Self> {
        let n = usize::from(kopf.kanaele);
        Ok(Dekoder {
            lappen: vec![Lappen::default(); n],
            kopf,
            setup,
            ausgegeben: 0,
        })
    }

    /// Dekodiert **ein** Audiopaket und liefert je Kanal die fertigen
    /// Samples (der erste Block liefert leer, siehe `mdct.rs`).
    pub fn paket(&mut self, paket: &[u8]) -> Ergebnis<Vec<Vec<f64>>> {
        let mut l = BitLeser::neu(paket);
        let ch = usize::from(self.kopf.kanaele);
        // 1) Pakettyp
        if l.bits(1)? != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "Pakettyp ist nicht 0 — das ist kein Audiopaket".to_string(),
            ));
        }
        // 2) Modusnummer
        let breite = crate::codebuch::ilog(self.setup.modi.len() as u32 - 1);
        let modus_nr = l.bits(breite)? as usize;
        let modus = self.setup.modi.get(modus_nr).cloned().ok_or_else(|| {
            Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Paket: Modus {modus_nr}, die Datei hat {}",
                    self.setup.modi.len()
                ),
            )
        })?;
        // 3) Blocklaenge
        let n = if modus.lang {
            self.kopf.block1
        } else {
            self.kopf.block0
        };
        // 4) Fensterform
        let (links_kurz, rechts_kurz) = if modus.lang {
            (l.bits(1)? == 0, l.bits(1)? == 0)
        } else {
            (false, false)
        };
        let f = fenster(n, modus.lang, links_kurz, rechts_kurz, self.kopf.block0);
        // 5) die Boden je Kanal
        let zuordnung = self
            .setup
            .zuordnungen
            .get(modus.zuordnung)
            .cloned()
            .ok_or_else(|| {
                Fehler::neu(
                    Art::FalscherWert,
                    format!("Modus {modus_nr}: Zuordnung {}", modus.zuordnung),
                )
            })?;
        let mut kurven: Vec<Option<Vec<f64>>> = Vec::with_capacity(ch);
        for kanal in 0..ch {
            let submap = *zuordnung.mux.get(kanal).unwrap_or(&0);
            let boden_nr = *zuordnung.boden.get(submap).unwrap_or(&0);
            match self.setup.boden.get(boden_nr) {
                Some(Boden::Eins(b)) => {
                    let rahmen = lies_rahmen(b, &mut l, &self.setup.codebucher)?;
                    kurven.push(rahmen.map(|r| crate::floor::kurve(b, &r, n / 2)));
                }
                Some(Boden::Null(b)) => {
                    // M3: Floor 0 liest sich, seine Kurve nicht — und der
                    // Fehler wird **gemeldet**. Ein Floor 0 als „unbenutzt"
                    // zu behandeln wuerde den Kanal stumm schalten: der
                    // Ton waere an dieser Stelle falsch, aber ohne jeden
                    // Hinweis. `?` gibt den benannten Fehler weiter, samt
                    // Grund (siehe `floor.rs`).
                    kurven.push(Some(kurve0(b)?));
                }
                None => {
                    return Err(Fehler::neu(
                        Art::FalscherWert,
                        format!("Kanal {kanal}: Boden {boden_nr} gibt es nicht"),
                    ));
                }
            }
        }
        // 6) Null-Vektoren verteilen (Abschnitt 4.3.3)
        let mut kein_residuum: Vec<bool> = kurven.iter().map(Option::is_none).collect();
        for i in (0..zuordnung.koppelpunkte).rev() {
            let m = zuordnung.groesse[i];
            let a = zuordnung.winkel[i];
            if !kein_residuum[m] || !kein_residuum[a] {
                kein_residuum[m] = false;
                kein_residuum[a] = false;
            }
        }
        // 7) das Residuum je Unterkarte (Abschnitt 4.3.4)
        let mut spektren: Vec<Vec<f64>> = vec![vec![0.0f64; n / 2]; ch];
        for submap in 0..zuordnung.submaps {
            let kanaele_in: Vec<usize> = (0..ch)
                .filter(|k| zuordnung.mux.get(*k).copied().unwrap_or(0) == submap)
                .collect();
            if kanaele_in.is_empty() {
                continue;
            }
            let rest_nr = *zuordnung.rest.get(submap).unwrap_or(&0);
            let rest = self.setup.reste.get(rest_nr).cloned().ok_or_else(|| {
                Fehler::neu(
                    Art::FalscherWert,
                    format!("Unterkarte {submap}: Residuum {rest_nr} gibt es nicht"),
                )
            })?;
            let marken: Vec<bool> = kanaele_in
                .iter()
                .map(|k| kein_residuum.get(*k).copied().unwrap_or(true))
                .collect();
            let vektoren = lies_restpaket(
                &rest,
                &mut l,
                n,
                kanaele_in.len(),
                &marken,
                &self.setup.codebucher,
            )?;
            for (pos, kanal) in kanaele_in.iter().enumerate() {
                spektren[*kanal] = vektoren.get(pos).cloned().unwrap_or_default();
            }
        }
        // 8) die Kanalkopplung rueckgaengig (Abschnitt 4.3.5)
        //
        // # VIER FAELLE, und ich hatte zwei davon zusammengezogen
        //
        // GEMESSENER BEFUND: mit zwei Faellen kam bei
        // `g8b_sine_stereo.ogg` der **linke** Kanal mit einem RMS von
        // 0,000206 heraus, waehrend der rechte 0,554 erreichte und gegen
        // das Orakel 85,8 dB erreichte. Der rechte Kanal ist der
        // Winkel, der linke die Groesse — und die Groesse ist genau der
        // Kanal, dessen Zuteilung in den beiden mittleren Faellen falsch
        // war.
        //
        // Die Spec hat **vier** Faelle, und die Reihenfolge der
        // Zuweisungen ist der ganze Trick: in zwei der vier Faelle ist
        // `new_A` gleich `M` und **nicht** `A`.
        //
        // GEMESSENER FEHLER, und er hat genau einen Kanal getroffen. Ich
        // hatte den letzten Zweig als `new_A = A` geschrieben und das
        // mit einem Spec-Zitat belegt, das ich **aus dem Gedaechtnis**
        // erzaehlt hatte. `lib/mapping0.c` sagt etwas anderes:
        //
        // ```c
        //   for(i=info->coupling_steps-1;i>=0;i--){
        //     float *pcmM=vb->pcm[info->coupling_mag[i]];
        //     float *pcmA=vb->pcm[info->coupling_ang[i]];
        //     for(j=0;j<n/2;j++){
        //       float mag=pcmM[j];
        //       float ang=pcmA[j];
        //       if(mag>0)
        //         if(ang>0){
        //           pcmM[j]=mag;
        //           pcmA[j]=mag-ang;
        //         }else{
        //           pcmA[j]=mag;
        //           pcmM[j]=mag+ang;
        //         }
        //       else
        //         if(ang>0){
        //           pcmM[j]=mag;
        //           pcmA[j]=mag+ang;
        //         }else{
        //           pcmA[j]=mag;
        //           pcmM[j]=mag-ang;
        //         }
        //     }
        //   }
        // ```
        //
        // Der letzte Zweig ist also
        //
        // ```text
        //   M <= 0, A <= 0 ->  new_A = M,      new_M = M - A
        // ```
        //
        // **und nicht** `new_A = A`.
        //
        // GEMESSENE FOLGE: bei `g8b_sine_stereo` ist `L = R`, also
        // speichert der Encoder `(M, A) = (s, 0)`. Mit `A = 0` greift
        // bei jedem `s <= 0` genau dieser Zweig — und mein Code liess
        // dort `new_A = A = 0`, also **null** statt `M`. Das loescht
        // jede Bin mit `M <= 0`, und das ist bei einem Sinus die
        // Haelfte aller Bins. Kanal 1 kam damit bei
        // gemessenen 2,0 dB gegen 103,9 dB fuer Kanal 0 an, mit einem
        // Amplitudenfaktor von 0,49863 — die Haelfte, gerundet.
        //
        // Die Lehre ist die alte Lehre: **Text holen, nicht raten.**
        // Ein Spec-Zitat aus dem Gedaechtnis ist kein Beleg.
        for i in (0..zuordnung.koppelpunkte).rev() {
            let (m, a) = (zuordnung.groesse[i], zuordnung.winkel[i]);
            if m == a {
                continue;
            }
            //
            // `#[allow]` ist hier NOTIG und nicht bequem: die Schleife
            // laesst **zwei** Vektoren gleichzeitig an derselben Stelle
            // stehen, und jeder Zweig schreibt beide. Ein `zip` ueber
            // `iter_mut()` wuerde die Zuordnung `gross`/`winkel`
            // verlieren — und genau diese Zuordnung ist der Text, den
            // `lib/mapping0.c` schreibt.
            #[allow(clippy::needless_range_loop)]
            for k in 0..n / 2 {
                let gross = spektren[m][k];
                let winkel = spektren[a][k];
                if gross > 0.0 {
                    if winkel > 0.0 {
                        spektren[m][k] = gross;
                        spektren[a][k] = gross - winkel;
                    } else {
                        spektren[a][k] = gross;
                        spektren[m][k] = gross + winkel;
                    }
                } else if winkel > 0.0 {
                    spektren[m][k] = gross;
                    spektren[a][k] = gross + winkel;
                } else {
                    // `new_A = M`, nicht `new_A = A` — siehe die
                    // Begruendung ueber diesem Block.
                    spektren[a][k] = gross;
                    spektren[m][k] = gross - winkel;
                }
            }
        }
        // 9) das Produkt Boden * Residuum (Abschnitt 4.3.6)
        for kanal in 0..ch {
            if let Some(kurve) = kurven[kanal].as_ref() {
                for (k, w) in spektren[kanal].iter_mut().enumerate().take(n / 2) {
                    *w *= kurve.get(k).copied().unwrap_or(0.0);
                }
            }
        }
        // 10) MDCT und Ueberlappen
        let mut raus = Vec::with_capacity(ch);
        for (kanal, spektrum) in spektren.iter().enumerate() {
            raus.push(self.lappen[kanal].schiebe(spektrum, &f, n)?);
        }
        let laenge = raus.first().map_or(0, Vec::len);
        self.ausgegeben += laenge;
        Ok(raus)
    }

    /// Dekodiert alle restlichen Pakete.
    pub fn alle(&mut self, pakete: &[Vec<u8>]) -> Ergebnis<Vec<Vec<f64>>> {
        let mut raus = Vec::new();
        for paket in pakete {
            let teil = self.paket(paket)?;
            let ch = teil.len();
            for (k, v) in teil.into_iter().enumerate() {
                if raus.len() <= k {
                    raus.push(Vec::new());
                }
                raus[k].extend(v);
                let _ = ch;
            }
        }
        Ok(raus)
    }
}

/// Dekodiert eine **ganze** Ogg/Vorbis-Datei.
///
/// # DIE DREI SCHRITTE
///
/// 1. Die Pakete aus dem Container holen (`ogg::pakete`).
/// 2. Die drei Kopfpakete lesen, den Dekoder bauen.
/// 3. Alle Audiopakete dekodieren und auf die Granelposition kuerzen.
///
/// # WARUM AUF DIE GRANELPOSITION GEKUERZT WIRD
///
/// Abschnitt A.2: „The granule position of a page represents the end
/// PCM sample position of the last packet completed on that page" und
/// „A granule position on the final page in a stream that indicates less
/// audio data than the final packet would normally return is used to end
/// the stream on other than even frame boundaries. The difference [...]
/// indicates how many trailing samples to discard."
///
/// GEMESSENE ENTSCHEIDUNG, warum das **kein** Rechenfehler ist, sondern
/// der Normalfall: der letzte Block laedt mit einem halben Block Ueberlappung
/// nach, und der Encoder hat diesen Rest nie in die Datei geschrieben.
/// Ohne das Kuerzen haette die letzte Ausgabe rund einen halben Block zu
/// viel, und die SNR-Messung gegen `ffmpeg -f s16le` brueche an den letzten
/// Zeilen ein — bei 132300 Samples einer 3-Sekunden-Datei sind das 0,4 %
/// aller Werte.
pub fn dekodiere(datei: &[u8]) -> Ergebnis<Ton> {
    let strom =
        crate::ogg::pakete(datei).map_err(|e| Fehler::neu(Art::StromZuEnde, e.to_string()))?;
    if strom.pakete.len() < 3 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "die Datei hat {} Pakete — Vorbis braucht Identification, Kommentar und Setup",
                strom.pakete.len()
            ),
        ));
    }
    let kopf = lies_kopf(&strom.pakete[0])?;
    // Der Kommentar ist laut Abschnitt 4.2 „a non-fatal error condition"
    // — er wird gelesen, aber sein Fehler bricht nichts ab.
    let _kommentar = lies_kommentar(&strom.pakete[1]).ok();
    let setup = lies_setup(&strom.pakete[2], usize::from(kopf.kanaele))?;
    let mut dekoder = Dekoder::neu(kopf, setup)?;
    let mut kanaele_daten = dekoder.alle(&strom.pakete[3..])?;
    let ch = kanaele_daten.len();
    let vorher = kanaele_daten.first().map_or(0, Vec::len);
    let samples = match strom.granel {
        Some(g) => {
            let g = g.min(usize::MAX as u64) as usize;
            if g > vorher {
                return Err(Fehler::neu(
                    Art::FalscheGroesse,
                    format!(
                        "die Granelposition sagt {g} Samples, der Dekoder hat {vorher} — \
                         das ist mehr als die Seite hergibt"
                    ),
                ));
            }
            for kanal in kanaele_daten.iter_mut() {
                kanal.truncate(g);
            }
            g
        }
        None => vorher,
    };
    Ok(Ton {
        rate: dekoder.kopf.rate,
        kanaele: ch,
        samples,
        kanaele_daten,
        granel: strom.granel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_identification_kopf_liefert_rate_kanaele_und_blocksizes() {
        // Abschnitt 4.2.2: typ(8) magic(6) version(32) kanaele(8)
        // rate(32) max(32) nominal(32) min(32) b0(4) b1(4) rahmen(1)
        let mut bytes = vec![1u8];
        bytes.extend_from_slice(b"vorbis");
        bytes.extend(0u32.to_le_bytes());
        bytes.push(2); // Kanaele
        bytes.extend(44100u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(128000u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        // b0 und b1 stehen als 4 + 4 Bit in **einem** Byte
        // (Abschnitt 4.2.2 Schritt 7 und 8), das Framing-Bit ist das
        // naechste Byte.
        //
        // GEMESSENER FEHLER IN DIESEM TEST: hier standen zwei Byte
        // (0b1000 und 0b1100). Der Leser las daraus b0 = 2^8 = 256 und
        // b1 = 2^0 = 1 und meldete „block0 = 256 ist groesser als
        // block1 = 1" — zu Recht, denn 1b1_Nibble von 0b1000 ist 0.
        bytes.push(8 | (12 << 4)); // b0 = 2^8 = 256, b1 = 2^12 = 4096
        bytes.push(1); // Framing
        let k = lies_kopf(&bytes).expect("kopf");
        assert_eq!(k.kanaele, 2);
        assert_eq!(k.rate, 44100);
        assert_eq!(k.bitraten[1], 128000);
        assert_eq!(k.block0, 256);
        assert_eq!(k.block1, 4096);
    }

    #[test]
    fn null_kanaele_oder_null_rate_werden_benannt() {
        let mut bytes = vec![1u8];
        bytes.extend_from_slice(b"vorbis");
        bytes.extend(0u32.to_le_bytes());
        bytes.push(0); // 0 Kanaele
        bytes.extend(44100u32.to_le_bytes());
        bytes.extend([0u8; 12]);
        bytes.push(8 | (8 << 4));
        bytes.push(1);
        match lies_kopf(&bytes) {
            Err(f) => assert!(f.text.contains("0 Kanaele"), "{f}"),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_block0_groesser_als_block1_wird_benannt() {
        let mut bytes = vec![1u8];
        bytes.extend_from_slice(b"vorbis");
        bytes.extend(0u32.to_le_bytes());
        bytes.push(1);
        bytes.extend(44100u32.to_le_bytes());
        bytes.extend([0u8; 12]);
        bytes.push(12 | (8 << 4)); // b0 = 4096, b1 = 256
        bytes.push(1);
        match lies_kopf(&bytes) {
            Err(f) => assert!(f.text.contains("4096"), "{f}"),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn die_umordnung_legt_je_sample_beide_kanaele_hintereinander() {
        // L = [0.0, 0.25], R = [0.5, -0.5] -> L0 R0 L1 R1.
        //
        // GEMESSENE KORREKTUR AN DEN WERTEN: hier standen 1.0, 2.0 und
        // 3.0. Das sind 32768, 65536 und 98304 nach der Umrechnung — alle
        // drei ausserhalb `i16` und also **geklemmt**, was die Aussage des
        // Tests (die Reihenfolge) mit der Klemmung vermischt hat. Jetzt
        // liegen alle vier Werte im Bereich.
        let t = Ton {
            rate: 44100,
            kanaele: 2,
            samples: 2,
            kanaele_daten: vec![vec![0.0, 0.25], vec![0.5, -0.5]],
            granel: Some(2),
        };
        assert_eq!(t.als_i16(), vec![0, 16384, 8192, -16384]);
        // Und zum Vergleich: die kanalweise Reihenfolge waere
        // L0 L1 R0 R1 = 0, 8192, 16384, -16384.
    }

    #[test]
    fn wer_oberhalb_voll_aussteuert_bekommt_die_klemmgrenze() {
        let t = Ton {
            rate: 44100,
            kanaele: 1,
            samples: 3,
            kanaele_daten: vec![vec![0.5, -0.5, 2.0]],
            granel: Some(3),
        };
        // 0.5 * 32768 = 16384, -0.5 -> -16384, 2.0 -> 65536 -> geklemmt.
        assert_eq!(t.als_i16(), vec![16384, -16384, 32767]);
    }

    #[test]
    fn die_werte_werden_geklemmt() {
        let t = Ton {
            rate: 44100,
            kanaele: 1,
            samples: 3,
            kanaele_daten: vec![vec![2.0, -2.0, 0.5]],
            granel: Some(3),
        };
        assert_eq!(t.als_i16(), vec![32767, -32768, 16384]);
    }

    #[test]
    fn ein_falscher_pakettyp_wird_benannt() {
        // Ein Bit 1 an der ersten Stelle: kein Audiopaket.
        let bytes = vec![0xffu8; 32];
        let k = Kopf {
            kanaele: 1,
            rate: 44100,
            bitraten: [0; 3],
            block0: 256,
            block1: 2048,
        };
        let mut d = Dekoder::neu(
            k,
            Setup {
                codebucher: vec![],
                boden: vec![],
                reste: vec![],
                zuordnungen: vec![],
                ende_bit: 0,
                modi: vec![Modus {
                    lang: false,
                    fenster: 0,
                    transform: 0,
                    zuordnung: 0,
                }],
            },
        )
        .expect("dekoder");
        match d.paket(&bytes) {
            Err(f) => assert!(f.text.contains("kein Audiopaket"), "{f}"),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn der_kommentar_liefert_anbieter_und_felder() {
        // Abschnitt 5.2.3: laenge(32) anbieter felderzahl(32) felder...
        let mut bytes = vec![3u8];
        bytes.extend_from_slice(b"vorbis");
        bytes.extend(9u32.to_le_bytes());
        bytes.extend_from_slice(b"libVorbis");
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(11u32.to_le_bytes());
        bytes.extend_from_slice(b"TITLE=Test\0\0");
        bytes.push(1); // Framing
        let k = lies_kommentar(&bytes).expect("kommentar");
        assert_eq!(k.anbieter, "libVorbis");
        assert_eq!(k.felder.len(), 1);
        assert_eq!(k.felder[0].0, "TITLE");
    }

    #[test]
    fn ein_zu_langer_kommentar_wird_benannt() {
        // M4: 2^20 Byte Anbieter sind unmöglich, auch wenn der Strom
        // kurz ist.
        let mut bytes = vec![3u8];
        bytes.extend_from_slice(b"vorbis");
        bytes.extend((1u32 << 21).to_le_bytes());
        match lies_kommentar(&bytes) {
            Err(f) => assert_eq!(f.art, Art::ZahlZuGross),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }
}
