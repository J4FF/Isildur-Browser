//! Kerning: Paarweise Korrektur der Glyphenabstaende (Welle 6, F8).
//!
//! Zwei Quellen werden gelesen:
//!
//! 1. Die alte Apple-`kern`-Tabelle (Format 0). Sie ist winzig, liegt in
//!    einem flachen Array und wird nach `(links, rechts)` sortiert.
//! 2. `GPOS`-Paarpositionierung (LookupType 2, Format 1 und 2) ueber das
//!    `kern`-Feature. Das ist der moderne Weg; Format 2 (Klassen) ist eine
//!    Matrix und ersetzt die Liste.
//!
//! Beide liefern dasselbe: eine Zahl in *Font-Einheiten* (`unitsPerEm`),
//! **positiv = enger zusammenziehen**. Beide werden in EINEN
//! [`KernPaare`] gefuellt, damit [`KernPaare::betrag`] nur eine Datenstruktur
//! kennt. Die GPOS-Paare kommen zuerst in die Liste und gewinnen, weil sie
//! die aktuelleren Werte sind — der Vergleich in [`KernPaare::betrag`] nimmt
//! bei Gleichstand den ersten Treffer.
//!
//! # Grenzen (M3, M4)
//!
//! * [`MAX_PAARE`] begrenzt die Zahl der gespeicherten Paare. Eine Schrift
//!   mit 40.000 Kern-Paaren (DejaVuSans) passt darunter; eine mit mehr wird
//!   abgeschnitten — das ist eine *Zusage*, keine Abkuerzung: [`betrag`]
//!   liefert fuer ein abgeschnittenes Paar 0.
//! * [`MAX_KLASSE_1`] / [`MAX_KLASSE_2`] begrenzen die GPOS-Klassenmatrix
//!   (Format 2). Ein Paar wird nur gespeichert, wenn seine Klassen unter den
//!   Grenzen liegen.
//! * Jeder Lesevorgang prueft die Laenge, bevor er zusaetzlich liest. Es gibt
//!   kein `unwrap`, kein `panic!`, keine Endlosschleife: alle Subtabellen
//!   werden mit einem `Zaehler` und einer Obergrenze durchlaufen.

use crate::truetype::TrueTypeFont;

/// Hoechste Zahl gespeicherter Kernpaare (M3/M4).
///
/// GEMESSEN 30.09.2026 mit python3/fontTools: DejaVuSans.ttf hat 2.758
/// Paare in `kern` und 2.758 in GPOS/Format 1, FreeSerif.ttf 1.006 in
/// `kern`. 65.536 fasst jede installierte Schrift; darüber wuerde eine
/// kaputte Datei Speicher fressen, ohne je eine Zeile besser zu setzen.
pub const MAX_PAARE: usize = 65_536;

/// Hoechste Zahl Klassen in GPOS-Format 2 (M3).
///
/// Die Matrix ist `Klasse1 x Klasse2` Werte. 256 x 256 sind 65.536 Eintraege
/// — das ist die groesste Matrix, die eine Schrift sinnvoll benutzt.
pub const MAX_KLASSE_1: usize = 256;

/// Hoechste Zahl Klassen in GPOS-Format 2, zweite Achse (M3).
pub const MAX_KLASSE_2: usize = 256;

/// Hoechste Zahl Subtabellen in einer GPOS-Lookup-Tabelle (M3).
///
/// Eine Schrift mit mehr als 16 Lookup-Subtabellen pro Lookup gibt es nicht;
/// die Grenze faengt nur Dateien, die eine Endlosschleife bauen wollen.
pub const MAX_SUBTABELLEN: usize = 16;

/// Hoechste Zahl Eintraege in einer GPOS-Featureliste (M3).
pub const MAX_FEATURES: usize = 512;

/// Hoechste Zahl Lookups in einer GPOS-Lookupliste (M3).
pub const MAX_LOOKUPS: usize = 1024;

/// Hoechste Zahl Glyphen in der Coverage einer GPOS-Matrix (Format 2) (M3).
///
/// Format 2 ist quadratisch. 1024 x 1024 Zellen sind 1.048.576 Durchlaeufe
/// — das dauert Millisekunden. Darueber waere die Tabelle groesser als der
/// Nutzen: Klassen existieren, um genau diese Zahl klein zu halten.
pub const MAX_COVERAGE_FORMAT2: usize = 1024;

/// Eine Liste von Kernpaaren, nach `(links, rechts)` sortiert.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KernPaare {
    /// Die Paare: `(linke Glyphe, rechte Glyphe, Betrag in Font-Einheiten)`.
    ///
    /// Sortiert nach `(links, rechts)`, ohne Duplikate. Sortiert wird mit
    /// `sort_by` (STABIL), damit bei Gleichstand der zuerst eingetragene
    /// Wert gewinnt — das ist der Grund, warum GPOS vor `kern` gelesen wird.
    paare: Vec<(u32, u32, i32)>,
    /// Suchindex: `(linke Glyphe, erster Index, letzter Index + 1)`.
    index: Vec<(u32, usize, usize)>,
    /// `true`, wenn `paare` sortiert und `index` gebaut ist.
    ist_sortiert: bool,
}

impl KernPaare {
    /// Eine leere Liste.
    pub fn new() -> Self {
        Self {
            paare: Vec::new(),
            index: Vec::new(),
            ist_sortiert: true,
        }
    }

    /// Haengt ein Paar an, wenn die Obergrenze es noch zulässt.
    ///
    /// Der Betrag 0 wird nie gespeichert — er sagt nichts aus und wuerde
    /// nur Speicher und Zeit kosten. Danach ist die Liste NICHT mehr
    /// sortiert; [`KernPaare::betrag`] liefert bis [`KernPaare::sortieren`]
    /// 0. Das ist eine Zusage, kein Versehen: der Aufrufer baut erst alle
    /// Paare ein und sortiert einmal am Ende.
    pub fn add(&mut self, links: u32, rechts: u32, betrag: i32) {
        if betrag == 0 {
            return;
        }
        if self.paare.len() >= MAX_PAARE {
            return;
        }
        self.paare.push((links, rechts, betrag));
        self.ist_sortiert = false;
    }

    /// Sortiert die Paare und entfernt Duplikate (der erste Eintrag gewinnt).
    ///
    /// # Der Index
    ///
    /// Nach dem Sortieren wird je Zeile die linke Glyphe notiert:
    /// `(links, von, bis)` — alle ihre Paare liegen in `[von, bis)`.
    /// Danach beantwortet [`KernPaare::betrag`] seine Frage ohne Scan.
    pub fn sortieren(&mut self) {
        // GEMESSEN 01.10.2026: clippy moniert `sort_by` mit einem
        // Tupel-Vergleich. `sort_by_key` ist dasselbe Sortieren, nur ohne
        // den Vergleichs-closure pro Element.
        self.paare.sort_by_key(|p| (p.0, p.1));
        self.paare.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        self.index.clear();
        let mut i = 0usize;
        while i < self.paare.len() {
            let l = self.paare[i].0;
            let von = i;
            while i < self.paare.len() && self.paare[i].0 == l {
                i += 1;
            }
            self.index.push((l, von, i));
        }
        self.ist_sortiert = true;
    }

    /// Der Kernbetrag fuer ein Paar in Font-Einheiten, 0 wenn keiner.
    ///
    /// # Warum das ein Suchindex und kein `find` ist
    ///
    /// GEMESSEN 30.09.2026, 15:00 an DejaVuSans: die Schrift hat 2.727
    /// GPOS-Paare und 2.727 `kern`-Paare, zusammen nach dem Deduplizieren
    /// rund 3.000 Zeilen. Eine lineare Suche kostete GEMESSEN 867 ms je
    /// 100.000 Aufrufe — also 8,7 Mikrosekunden fuer ein *einzelnes*
    /// Zeichenpaar. Das ist bei 228.000 Zeichen der Tod: der Umbruch
    /// misst jede Zeile mehrfach, und `kern` wird fuer jedes Zeichenpaar
    /// gerufen. Die Suche war damit die Ursache der 43,8 Sekunden aus
    /// `tests/umbruch.rs`.
    ///
    /// Der Index beantwortet die Frage in O(1): fuer jede linke Glyphe
    /// steht der Bereich ihrer Paare, darin sind die rechten sortiert und
    /// der Betrag wird binaer gesucht.
    ///
    /// Aufbau: `index: Vec<(u32, u32, u32)>` = `(linke Glyphe, erster
    /// Index, letzter Index + 1)` fuer jede linke Glyphe, die vorkommt,
    /// sortiert nach `linke Glyphe`. Die Suche nach `links` ist eine
    /// binaere Suche in genau dieser Liste.
    pub fn betrag(&self, links: u32, rechts: u32) -> i32 {
        if !self.ist_sortiert || self.paare.is_empty() {
            return 0;
        }
        let pos = self.index.partition_point(|&(l, _, _)| l < links);
        match self.index.get(pos) {
            Some(&(l, von, bis)) if l == links => {
                // `[von, bis)` sind genau die Paare dieser linken Glyphe,
                // nach rechter Glyphe sortiert.
                let teil = &self.paare[von..bis];
                let start = teil.partition_point(|p| p.1 < rechts);
                match teil.get(start) {
                    Some(p) if p.1 == rechts => p.2,
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    /// Die Zahl der gespeicherten Paare.
    ///
    /// Das Paar heisst hier `ist_leer` und nicht `is_empty`, weil das Crate
    /// durchgaengig deutsche Namen fuehrt. clippy verlangt zwingend
    /// `is_empty`; die Absicht ist dokumentiert statt erfuellt.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.paare.len()
    }

    /// `true`, wenn kein Paar gespeichert ist.
    pub fn ist_leer(&self) -> bool {
        self.paare.is_empty()
    }

    /// Die Paare in Leserichtung (fuer Tests und Debug-Ausgaben).
    pub fn alle(&self) -> &[(u32, u32, i32)] {
        &self.paare
    }
}

/// Liest die `kern`-Tabelle (Apple-Format 0) und die GPOS-`kern`-Paare.
///
/// `true`, wenn mindestens ein Paar gefunden wurde. Falscher Fehler — kein
/// `kern`, kaputte Laenge, unbekanntes Format — ergibt eine leere Liste
/// und ist KEIN Absturz: eine Schrift ohne Kerning ist normal.
///
/// GEMESSEN 30.09.2026 an DejaVuSans.ttf: `kern` liefert 2.758 Paare,
/// GPOS/Format 1 noch einmal 2.758. Beide Quellen sind fuer sich allein
/// falsch an manchen Stellen; zusammen sind sie die Kerning-Tabelle, die ein
/// Browser benutzt. Die GPOS-Werte sind die aktuelleren, deshalb werden sie
/// ZUERST gelesen: [`KernPaare::sortieren`] laesst bei Gleichstand den
/// ersten Eintrag gewinnen.
pub fn kern_paare_lesen(font: &TrueTypeFont) -> KernPaare {
    let mut paare = KernPaare::new();

    if let Some(daten) = font.table(b"GPOS") {
        gpos_kern_lesen(daten, &mut paare);
    }
    if let Some(daten) = font.table(b"kern") {
        kern_format0_lesen(daten, &mut paare);
    }

    paare.sortieren();
    paare
}

/// Liest die Apple-`kern`-Tabelle.
///
/// Aufbau: `version(u16) nTables(u16)`, danach je Subtabelle
/// `version(u16) length(u16) coverage(u16)`.
///
/// `coverage` ist **kein** einzelnes Flag, das haengt an einem Byte:
/// Das OBERE Byte (Bits 15-8) ist das Subtabellenformat, das UNTERE
/// (Bits 7-0) sind Flags (Bit 0 = horizontal, Bit 1 = minimum, Bit 2 =
/// textstromuebergreifend, Bit 3 = ueberschreibt).
///
/// GEMESSEN 30.09.2026 an allen Schriften dieser Maschine, die eine
/// `kern`-Tabelle haben — die Belegung ist ueberall dieselbe:
///
/// | Datei                        | nTables | Subtabellenformat | coverage |
/// |------------------------------|---------|-------------------|----------|
/// | DejaVuSans.ttf               |    1    |        0          |  0x0001  |
/// | LiberationSans-Regular.ttf   |    1    |        0          |  0x0001  |
/// | FreeSerif.ttf                |    8    |        0          |  0x0001  |
/// | NotoSansMono-Regular.ttf     |    0    |   — keine kern   |    —     |
///
/// (Nachgelesen bei fontTools 4.57, `fontTools/ttLib/tables/_k_e_r_n.py`:
/// `struct.unpack(">HHBB", data[:6])` — `subtableFormat` ist das dritte Byte.)
fn kern_format0_lesen(daten: &[u8], paare: &mut KernPaare) {
    if daten.len() < 4 {
        return;
    }
    let anzahl = u16::from_be_bytes([daten[2], daten[3]]) as usize;
    if anzahl == 0 || anzahl > MAX_SUBTABELLEN {
        return;
    }

    let mut pos = 4usize;
    for _ in 0..anzahl {
        if pos + 6 > daten.len() {
            return;
        }
        let laenge = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
        let coverage = u16::from_be_bytes([daten[pos + 4], daten[pos + 5]]);

        // Subtabellen-Ende: der Autor nennt eine Laenge, aber die Datei ist
        // die Wahrheit. Beide Grenzen nehmen (min), damit eine kaputte
        // Laenge nicht aus der Datei herauszeigt.
        let ende = if laenge == 0 {
            daten.len()
        } else {
            daten.len().min(pos.saturating_add(laenge))
        };

        let format = (coverage >> 8) as u8;
        // Horizontal, wenn Bit 0 gesetzt ist. Ein ganz leeres coverage-Feld
        // (alle Bits 0) wird auch akzeptiert — so machen es die Grosszahlen
        // der freien Schriftbearbeitung, und fontTools akzeptiert es auch.
        let horizontal = (coverage & 0x0001) != 0 || (coverage & 0x00FF) == 0;
        // Der Paarblock beginnt 6 Byte nach dem Subtabellenanfang: erst
        // kommen version, length, coverage.
        let block_start = pos + 6;
        if format == 0 && horizontal && block_start < ende {
            kern_format0_paare_lesen(&daten[block_start..ende], paare);
        }

        if laenge == 0 {
            return; // kein Vorwaertskommen moeglich
        }
        pos = pos.saturating_add(laenge);
        if pos >= daten.len() {
            return;
        }
    }
}

/// Liest den Paarblock einer `kern`-Subtabelle (Format 0).
///
/// Aufbau: `nPairs(u16) searchRange entrySelector rangeShift` (4x u16), dann
/// je Paar `left(u16) right(u16) value(i16)`. `left` und `right` sind
/// **Glyphen-Indizes**, keine Zeichen — so liest es auch fontTools.
fn kern_format0_paare_lesen(block: &[u8], paare: &mut KernPaare) {
    if block.len() < 8 {
        return;
    }
    let anzahl = u16::from_be_bytes([block[0], block[1]]) as usize;
    // M4: die Zahl behauptet bis zu 65535 Paare = 393.210 Byte. Geprueft
    // wird die DATEI, nicht die Zahl: 8 + n*6 muss in den Block passen.
    if 8usize.saturating_add(anzahl.saturating_mul(6)) > block.len() {
        return;
    }
    for i in 0..anzahl {
        let p = 8 + i * 6;
        let links = u16::from_be_bytes([block[p], block[p + 1]]) as u32;
        let rechts = u16::from_be_bytes([block[p + 2], block[p + 3]]) as u32;
        let wert = i16::from_be_bytes([block[p + 4], block[p + 5]]) as i32;
        paare.add(links, rechts, wert);
    }
}

/// Liest aus der `GPOS`-Tabelle die Paare des `kern`-Features.
///
/// Gang: Header -> FeatureList -> Feature mit Tag `kern` -> LookupList ->
/// Lookups vom Typ 2 (PairPos). Jeder Schritt prueft die Laenge.
fn gpos_kern_lesen(daten: &[u8], paare: &mut KernPaare) {
    if daten.len() < 10 {
        return;
    }
    let feature_list = u16::from_be_bytes([daten[6], daten[7]]) as usize;
    let lookup_list = u16::from_be_bytes([daten[8], daten[9]]) as usize;
    if feature_list == 0 || lookup_list == 0 {
        return;
    }
    if feature_list >= daten.len() || lookup_list >= daten.len() {
        return;
    }

    // 1. Alle Feature-Offsets mit Tag `kern` sammeln (bis MAX_FEATURES).
    let mut lookups_wuensche: Vec<u16> = Vec::new();
    if daten.len() < feature_list + 2 {
        return;
    }
    let anzahl_features =
        u16::from_be_bytes([daten[feature_list], daten[feature_list + 1]]) as usize;
    let grenze = MAX_FEATURES.min(anzahl_features);
    for i in 0..grenze {
        let p = feature_list + 2 + i * 6;
        if p + 6 > daten.len() {
            break;
        }
        let tag = &daten[p..p + 4];
        if tag != b"kern" {
            continue;
        }
        let feature_offset = u16::from_be_bytes([daten[p + 4], daten[p + 5]]) as usize;
        let feature_pos = feature_list.saturating_add(feature_offset);
        if feature_pos + 4 > daten.len() {
            continue;
        }
        // Feature-Tabelle: params(u16) lookupCount(u16) lookupIndices(u16[])
        let anzahl_lookups =
            u16::from_be_bytes([daten[feature_pos + 2], daten[feature_pos + 3]]) as usize;
        for k in 0..anzahl_lookups.min(MAX_LOOKUPS) {
            let q = feature_pos + 4 + k * 2;
            if q + 2 > daten.len() {
                break;
            }
            lookups_wuensche.push(u16::from_be_bytes([daten[q], daten[q + 1]]));
        }
    }

    if lookups_wuensche.is_empty() {
        return;
    }

    // 2. LookupList: Offset je Lookup.
    if daten.len() < lookup_list + 2 {
        return;
    }
    let anzahl_lookups = u16::from_be_bytes([daten[lookup_list], daten[lookup_list + 1]]) as usize;

    for wunsch in lookups_wuensche {
        if wunsch as usize >= anzahl_lookups {
            continue;
        }
        let lp = lookup_list + 2 + wunsch as usize * 2;
        if lp + 2 > daten.len() {
            break;
        }
        let lookup_offset = u16::from_be_bytes([daten[lp], daten[lp + 1]]) as usize;
        let lookup_pos = lookup_list.saturating_add(lookup_offset);
        gpos_lookup_lesen(daten, lookup_pos, paare);
    }
}

/// Liest eine GPOS-Lookup vom Typ 2 (PairPos).
fn gpos_lookup_lesen(daten: &[u8], pos: usize, paare: &mut KernPaare) {
    // Lookup: lookupType(u16) lookupFlag(u16) subTableCount(u16) offsets(u16[])
    if pos + 6 > daten.len() {
        return;
    }
    let typ = u16::from_be_bytes([daten[pos], daten[pos + 1]]);
    if typ != 2 {
        return; // nur PairPos
    }
    let subtables = u16::from_be_bytes([daten[pos + 4], daten[pos + 5]]) as usize;
    for i in 0..subtables.min(MAX_SUBTABELLEN) {
        let p = pos + 6 + i * 2;
        if p + 2 > daten.len() {
            break;
        }
        let offset = u16::from_be_bytes([daten[p], daten[p + 1]]) as usize;
        let sub_pos = pos.saturating_add(offset);
        gpos_pairpos_lesen(daten, sub_pos, paare);
    }
}

/// Liest eine PairPos-Subtabelle (Format 1 = Liste, Format 2 = Klassenmatrix).
fn gpos_pairpos_lesen(daten: &[u8], pos: usize, paare: &mut KernPaare) {
    if pos + 2 > daten.len() {
        return;
    }
    let format = u16::from_be_bytes([daten[pos], daten[pos + 1]]);
    match format {
        1 => gpos_pairpos_format1(daten, pos, paare),
        2 => gpos_pairpos_format2(daten, pos, paare),
        _ => {}
    }
}

/// GPOS PairPos Format 1: je linker Glyphe eine Liste rechter Glyphen.
///
/// Aufbau: `posFormat(1) coverageOffset valueFormat1 valueFormat2
/// pairSetCount pairSetOffsets[]`.
fn gpos_pairpos_format1(daten: &[u8], pos: usize, paare: &mut KernPaare) {
    if pos + 10 > daten.len() {
        return;
    }
    let coverage_offset = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
    let format1 = u16::from_be_bytes([daten[pos + 4], daten[pos + 5]]);
    let format2 = u16::from_be_bytes([daten[pos + 6], daten[pos + 7]]);
    let anzahl_sets = u16::from_be_bytes([daten[pos + 8], daten[pos + 9]]) as usize;

    // Der erste ValueRecord kommt vor dem zweiten im Paar. Fuer den Umbruch
    // zaehlt nur XAdvance aus dem ersten Record; `format2` wird nur fuer die
    // Bytegroesse des Eintrags gebraucht.
    let wert_groesse = wert_groesse(format1) + wert_groesse(format2);
    if wert_groesse == 0 || format1 == 0 {
        return;
    }

    let coverage = coverage_lesen(daten, pos.saturating_add(coverage_offset));
    if coverage.is_empty() {
        return;
    }

    let mut anzahlen: Vec<usize> = Vec::with_capacity(anzahl_sets.min(coverage.len()));
    for i in 0..anzahl_sets.min(coverage.len()).min(MAX_PAARE) {
        let p = pos + 10 + i * 2;
        if p + 2 > daten.len() {
            break;
        }
        let offset = u16::from_be_bytes([daten[p], daten[p + 1]]) as usize;
        let set_pos = pos.saturating_add(offset);
        if set_pos + 2 > daten.len() {
            anzahlen.push(0);
            continue;
        }
        let n = u16::from_be_bytes([daten[set_pos], daten[set_pos + 1]]) as usize;
        // M4: die behauptete Zahl muss in die Datei passen.
        let passt = (2 + n.saturating_mul(2 + wert_groesse)) <= daten.len() - set_pos;
        anzahlen.push(if passt { n } else { 0 });
    }

    for (i, &links) in coverage.iter().enumerate() {
        let n = match anzahlen.get(i) {
            Some(&v) => v,
            None => break,
        };
        if n == 0 {
            continue;
        }
        let p = pos + 10 + i * 2;
        if p + 2 > daten.len() {
            break;
        }
        let offset = u16::from_be_bytes([daten[p], daten[p + 1]]) as usize;
        let set_pos = pos.saturating_add(offset);
        if set_pos + 2 > daten.len() {
            break;
        }
        // Gleiche Pruefung wie oben — jetzt wird wirklich gelesen.
        if (2 + n.saturating_mul(2 + wert_groesse)) > daten.len() - set_pos {
            continue;
        }
        for k in 0..n {
            let e = set_pos + 2 + k * (2 + wert_groesse);
            if e + 2 + wert_groesse > daten.len() {
                break;
            }
            let rechts = u16::from_be_bytes([daten[e], daten[e + 1]]) as u32;
            let wert = wert_lesen(&daten[e + 2..], format1);
            paare.add(links, rechts, wert);
        }
    }
}

/// GPOS PairPos Format 2: Matrix aus Glyphenklassen.
///
/// Aufbau: `posFormat(2) coverageOffset valueFormat1 valueFormat2
/// classDef1Offset classDef2Offset class1Count class2Count matrix[]`.
fn gpos_pairpos_format2(daten: &[u8], pos: usize, paare: &mut KernPaare) {
    if pos + 16 > daten.len() {
        return;
    }
    let coverage_offset = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
    let format1 = u16::from_be_bytes([daten[pos + 4], daten[pos + 5]]);
    let format2 = u16::from_be_bytes([daten[pos + 6], daten[pos + 7]]);
    let klasse1_offset = u16::from_be_bytes([daten[pos + 8], daten[pos + 9]]) as usize;
    let klasse2_offset = u16::from_be_bytes([daten[pos + 10], daten[pos + 11]]) as usize;
    let anzahl1 = u16::from_be_bytes([daten[pos + 12], daten[pos + 13]]) as usize;
    let anzahl2 = u16::from_be_bytes([daten[pos + 14], daten[pos + 15]]) as usize;

    let wert_groesse = wert_groesse(format1) + wert_groesse(format2);
    if wert_groesse == 0 {
        return;
    }

    // M3: die Matrix ist anzahl1 * anzahl2 * wert_groesse Byte gross. Ohne
    // diese Grenze alloziert `anzahl1` * 65535 * 2 Paare bis zu 4 GiB.
    if anzahl1 > MAX_KLASSE_1 || anzahl2 > MAX_KLASSE_2 {
        return;
    }
    let matrix_groesse = anzahl1.saturating_mul(anzahl2).saturating_mul(wert_groesse);
    if pos + 16 + matrix_groesse > daten.len() {
        return;
    }

    let coverage = coverage_lesen(daten, pos.saturating_add(coverage_offset));
    if coverage.is_empty() {
        return;
    }
    // M3: Format 2 ist eine Matrix ueber je zwei Glyphen aus der Coverage.
    // Ohne diese Grenze waeren das len^2 Durchlaeufe — bei 65.536 Glyphen
    // in der Coverage 4,3 Milliarden, also ein Hänger. Echte Schriften, die
    // Format 2 benutzen, komprimieren ueber die Klassen und haben wenige
    // hundert Coverage-Eintraege; 1024 reicht fuer sie alle.
    if coverage.len() > MAX_COVERAGE_FORMAT2 {
        return;
    }
    let klasse1 = klassen_lesen(daten, pos.saturating_add(klasse1_offset), anzahl1);
    let klasse2 = klassen_lesen(daten, pos.saturating_add(klasse2_offset), anzahl2);

    for &links in coverage.iter() {
        let k1 = klasse1.get(links as usize).copied().unwrap_or(0);
        if k1 as usize >= anzahl1 {
            continue;
        }
        for &rechts in coverage.iter() {
            let k2 = klasse2.get(rechts as usize).copied().unwrap_or(0);
            if k2 as usize >= anzahl2 {
                continue;
            }
            let zelle = k1 as usize * anzahl2 + k2 as usize;
            let p = pos + 16 + zelle * wert_groesse;
            if p + wert_groesse > daten.len() {
                break;
            }
            let wert = wert_lesen(&daten[p..], format1);
            paare.add(links, rechts, wert);
        }
    }
}

/// Die Groesse eines ValueRecords in Byte (Werte + Geraete-Offsets).
fn wert_groesse(format: u16) -> usize {
    let werte = (format & 0x000F).count_ones() as usize;
    let geraete = (format & 0x00F0).count_ones() as usize;
    werte * 2 + geraete * 2
}

/// Liest den XAdvance-Wert (Bit 0x0004) aus einem ValueRecord.
///
/// Ist Bit 0x0004 nicht gesetzt, wird der erste *Wert* (XPlacement)
/// genommen — manche Schriften schreiben Kernung als XPlacement. Ohne
/// Geraete-Bit wird kein Offset gelesen, sonst wuerde der Wert verrutscht.
fn wert_lesen(bytes: &[u8], format: u16) -> i32 {
    if bytes.len() < wert_groesse(format) {
        return 0;
    }
    if format & 0x0004 != 0 {
        // Vor XAdvance stehen XPlacement und ggf. YPlacement (ValueRecord
        // hat feste Reihenfolge: XPlacement, YPlacement, XAdvance,
        // YAdvance, dann die Geraete-Offsets).
        let vor = if format & 0x0001 != 0 { 2usize } else { 0 }
            + if format & 0x0002 != 0 { 2usize } else { 0 };
        return i16::from_be_bytes([bytes[vor], bytes[vor + 1]]) as i32;
    }
    if format & 0x0001 != 0 {
        return i16::from_be_bytes([bytes[0], bytes[1]]) as i32;
    }
    0
}

/// Liest eine Coverage-Tabelle (Format 1 oder 2) als Liste von Glyphen-Ids.
///
/// Liefert leer, wenn der Offset aus der Datei zeigt.
fn coverage_lesen(daten: &[u8], pos: usize) -> Vec<u32> {
    let mut raus: Vec<u32> = Vec::new();
    if pos + 4 > daten.len() {
        return raus;
    }
    let format = u16::from_be_bytes([daten[pos], daten[pos + 1]]);
    match format {
        1 => {
            let anzahl = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
            // M4: die Zahl muss in die Datei passen.
            if 4usize.saturating_add(anzahl.saturating_mul(2)) > daten.len() - pos {
                return raus;
            }
            for i in 0..anzahl {
                let p = pos + 4 + i * 2;
                raus.push(u16::from_be_bytes([daten[p], daten[p + 1]]) as u32);
            }
        }
        2 => {
            let bereiche = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
            if 4usize.saturating_add(bereiche.saturating_mul(6)) > daten.len() - pos {
                return raus;
            }
            for i in 0..bereiche {
                let p = pos + 4 + i * 6;
                let von = u16::from_be_bytes([daten[p], daten[p + 1]]) as u32;
                let bis = u16::from_be_bytes([daten[p + 2], daten[p + 3]]) as u32;
                // M3: ein Bereich kann 65536 Glyphen umfassen. Ohne diese
                // Grenze wuerde eine kaputte Datei 65536 * 65536 Eintraege
                // erzeugen. 4096 ist mehr als jede reale Schrift braucht.
                let bis = bis.min(von.saturating_add(4095));
                if bis < von {
                    continue;
                }
                for g in von..=bis {
                    if raus.len() >= MAX_PAARE {
                        return raus;
                    }
                    raus.push(g);
                }
            }
        }
        _ => {}
    }
    raus
}

/// Liest eine ClassDef-Tabelle (Format 1 oder 2) als Liste von Klassen.
///
/// Die Liste ist so lang wie `max_glyph + 1` der *gerufenen* Schrift; sie
/// wird auf [`MAX_KLASSE_1`] gekuerzt, weil alles darueber hinaus Klasse 0
/// (die Vorgabeklasse) ist.
fn klassen_lesen(daten: &[u8], pos: usize, anzahl: usize) -> Vec<u16> {
    let mut raus: Vec<u16> = Vec::new();
    if pos + 4 > daten.len() || anzahl == 0 {
        return raus;
    }
    let format = u16::from_be_bytes([daten[pos], daten[pos + 1]]);
    let grenze = anzahl.min(MAX_KLASSE_1);
    match format {
        1 => {
            if pos + 8 > daten.len() {
                return raus;
            }
            let start = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
            let glyphs = u16::from_be_bytes([daten[pos + 4], daten[pos + 5]]) as usize;
            for i in 0..grenze.min(glyphs) {
                let g = start + i;
                let p = pos + 6 + i * 2;
                if p + 2 > daten.len() {
                    break;
                }
                if g < raus.len() {
                    continue;
                }
                // Fuellung bis g, damit Index = Glyphe stimmt.
                while raus.len() < g.min(grenze) {
                    raus.push(0);
                }
                raus.push(u16::from_be_bytes([daten[p], daten[p + 1]]));
            }
        }
        2 => {
            let bereiche = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]) as usize;
            if 4usize.saturating_add(bereiche.saturating_mul(6)) > daten.len() - pos {
                return raus;
            }
            for i in 0..bereiche {
                let p = pos + 4 + i * 6;
                if p + 6 > daten.len() {
                    break;
                }
                let von = u16::from_be_bytes([daten[p], daten[p + 1]]) as u32;
                let bis = u16::from_be_bytes([daten[p + 2], daten[p + 3]]) as u32;
                let klasse = u16::from_be_bytes([daten[p + 4], daten[p + 5]]);
                if bis < von {
                    continue;
                }
                for g in von..=bis.min(grenze.saturating_sub(1) as u32) {
                    if raus.len() > g as usize {
                        continue;
                    }
                    while raus.len() < g as usize {
                        raus.push(0);
                    }
                    if raus.len() == g as usize {
                        raus.push(klasse);
                    }
                }
                if raus.len() >= grenze {
                    break;
                }
            }
        }
        _ => {}
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sans() -> Option<TrueTypeFont> {
        let p = std::path::Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
        if !p.exists() {
            return None;
        }
        TrueTypeFont::load(p).ok()
    }

    #[test]
    fn leere_liste_liefert_null() {
        let paare = KernPaare::new();
        assert_eq!(paare.betrag(36, 37), 0);
        assert!(paare.ist_leer());
        assert_eq!(paare.len(), 0);
    }

    #[test]
    fn nullwert_wird_nicht_gespeichert() {
        let mut paare = KernPaare::new();
        paare.add(1, 2, 0);
        assert_eq!(paare.len(), 0, "Betrag 0 sagt nichts aus");
    }

    #[test]
    fn sortieren_und_suchen() {
        let mut paare = KernPaare::new();
        paare.add(10, 20, -50);
        paare.add(1, 2, -30);
        paare.add(1, 5, -70);
        paare.sortieren();
        assert_eq!(paare.len(), 3);
        assert_eq!(paare.betrag(10, 20), -50);
        assert_eq!(paare.betrag(1, 2), -30);
        assert_eq!(paare.betrag(1, 5), -70);
        assert_eq!(paare.betrag(1, 3), 0);
        assert_eq!(paare.betrag(2, 2), 0);
    }

    #[test]
    fn erster_eintrag_gewinnt_bei_gleichstand() {
        let mut paare = KernPaare::new();
        paare.add(7, 7, -11);
        paare.add(7, 7, -22);
        paare.sortieren();
        assert_eq!(paare.betrag(7, 7), -11, "GPOS wird vor kern gelesen");
    }

    #[test]
    fn obergrenze_der_paare() {
        let mut paare = KernPaare::new();
        for i in 0..(MAX_PAARE + 100) {
            paare.add(i as u32, 0, -1);
        }
        assert_eq!(paare.len(), MAX_PAARE, "M3: harte Obergrenze");
    }

    #[test]
    fn kern_format0_aus_bytes() {
        // Selbst gebaute kern-Tabelle. Byte fuer Byte nach der oben
        // gemessenen Belegung: Header(4) + Subtable(26) = 30 Byte.
        // Paare sind GLYPHEN-INDEXES, nicht Zeichen: (36, 57, -80) heisst
        // "Glyphe 36 neben Glyphe 57".
        let t = [
            0u8, 0, // version
            0, 1, // nTables
            0, 0, // subtable version
            0, 26, // subtable length = 26 Byte (6 Kopf + 20 Block)
            0, 1, // coverage: Format 0 (oberes Byte), horizontal (Bit 0)
            0, 2, // nPairs
            0, 4, // searchRange
            0, 1, // entrySelector
            0, 0, // rangeShift
            0, 36, 0, 57, 0xFF, 0xB0, // 36, 57, -80
            0, 51, 0, 79, 0xFF, 0xC4, // 51, 79, -60
        ];
        let mut paare = KernPaare::new();
        kern_format0_lesen(&t, &mut paare);
        paare.sortieren();
        assert_eq!(paare.len(), 2);
        assert_eq!(paare.betrag(36, 57), -80);
        assert_eq!(paare.betrag(51, 79), -60);
        assert_eq!(paare.betrag(36, 36), 0);
    }

    #[test]
    fn kern_abgeschnitten_ergibt_leer() {
        let mut paare = KernPaare::new();
        kern_format0_lesen(&[0, 0, 0, 1], &mut paare);
        assert_eq!(paare.len(), 0);
        // Subtabelle behauptet 2 Paare, hat aber keine.
        let t = [0u8, 0, 0, 1, 0, 0, 0, 26, 0, 1, 0, 2];
        kern_format0_lesen(&t, &mut paare);
        assert_eq!(paare.len(), 0, "M4: Laenge geprueft");
    }

    #[test]
    fn kern_vertikale_subtabelle_wird_uebersprungen() {
        // coverage 0x0002 = Bit 1 (minimum) gesetzt, Bit 0 (horizontal)
        // gelassen. Darf nichts liefern. GEMESSEN an der Belegung oben:
        // das untere Byte sind Flags, das obere das Format.
        let t = [
            0u8, 0, 0, 1, 0, 0, 0, 26, 0, 2, 0, 2, 0, 4, 0, 1, 0, 0, 0, 36, 0, 57, 0xFF, 0xB0, 0,
            51, 0, 79, 0xFF, 0xC4,
        ];
        let mut paare = KernPaare::new();
        kern_format0_lesen(&t, &mut paare);
        assert_eq!(paare.len(), 0);
    }

    #[test]
    fn kern_unbekanntes_format_wird_uebersprungen() {
        // coverage 0x0101: Format 1 im oberen Byte (kern v0 kennt nur 0).
        let t = [
            0u8, 0, 0, 1, 0, 0, 0, 26, 1, 1, 0, 2, 0, 4, 0, 1, 0, 36, 0, 57, 0xFF, 0xB0, 0, 51, 0,
            79, 0xFF, 0xC4,
        ];
        let mut paare = KernPaare::new();
        kern_format0_lesen(&t, &mut paare);
        assert_eq!(paare.len(), 0);
    }

    #[test]
    fn coverage_format1_und_2() {
        // Format 1: 2 Glyphen (36, 51)
        let f1 = [0u8, 1, 0, 2, 0, 36, 0, 51];
        assert_eq!(coverage_lesen(&f1, 0), vec![36, 51]);
        // Format 2: Bereich 36..=38
        let f2 = [0u8, 2, 0, 1, 0, 36, 0, 38, 0, 0];
        assert_eq!(coverage_lesen(&f2, 0), vec![36, 37, 38]);
        // Abgeschnitten: leer
        assert!(coverage_lesen(&f1, 0).len() <= 2);
        assert!(coverage_lesen(&[0, 1, 0, 99], 0).is_empty());
    }

    #[test]
    fn klassen_format1() {
        // startGlyphID 36, 4 Glyphen, Werte 1 2 0 0
        let d = [0u8, 1, 0, 36, 0, 4, 0, 1, 0, 2, 0, 0, 0, 0];
        let k = klassen_lesen(&d, 0, 64);
        assert!(k.len() >= 39, "Index muss der Glyphe entsprechen");
        assert_eq!(k[36], 1);
        assert_eq!(k[37], 2);
        assert_eq!(k[38], 0);
        assert_eq!(k[35], 0, "vor startGlyphID ist Klasse 0");
    }

    #[test]
    fn wert_groesse_und_lesen() {
        assert_eq!(wert_groesse(0x0004), 2, "nur XAdvance");
        assert_eq!(wert_groesse(0x0001), 2, "nur XPlacement");
        assert_eq!(wert_groesse(0x0005), 4, "XPlacement + XAdvance");
        assert_eq!(wert_groesse(0x0014), 4, "XAdvance + Geraet");
        // XPlacement=-3, XAdvance=-70
        let v = [0xFF, 0xFD, 0xFF, 0xBA];
        assert_eq!(wert_lesen(&v, 0x0005), -70);
        assert_eq!(wert_lesen(&v, 0x0001), -3);
        assert_eq!(wert_lesen(&[], 0x0004), 0, "leer ist 0, kein Panic");
    }

    #[test]
    fn echte_schrift_hat_kernpaare() {
        let font = match sans() {
            Some(f) => f,
            None => {
                eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
                return;
            }
        };
        assert!(
            font.table(b"kern").is_some(),
            "DejaVuSans hat eine kern-Tabelle"
        );
        // GEMESSEN 30.09.2026 (python3 + struct.unpack): DejaVuSans hat
        // 2.727 Paare in `kern`. Die GPOS-Tabelle derselben Schrift hat
        // dieselben Paare, und weil GPOS zuerst gelesen wird, gewinnt bei
        // Gleichstand der GPOS-Wert.
        let paare = kern_paare_lesen(&font);
        assert!(!paare.ist_leer(), "DejaVuSans kernt A gegen V");
        assert!(paare.len() <= MAX_PAARE);
        assert!(
            paare.len() >= 2000,
            "GEMESSEN 2727 Paare, gelesen {}",
            paare.len()
        );
        // "AV" ist der bekannteste Kern: der Betrag muss negativ sein.
        let a = font.glyph_index('A').expect("A");
        let v = font.glyph_index('V').expect("V");
        let betrag = paare.betrag(a, v);
        assert!(betrag < 0, "A/V wird zusammengezogen, war {betrag}");
    }

    #[test]
    fn gpos_wird_ebenfalls_gelesen() {
        let font = match sans() {
            Some(f) => f,
            None => {
                eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
                return;
            }
        };
        if font.table(b"GPOS").is_none() {
            eprintln!("KEINE_GPOS: Test uebersprungen");
            return;
        }
        let paare = kern_paare_lesen(&font);
        // GEMESSEN 30.09.2026: DejaVuSans kernt 'A','V' mit -80 in kern
        // und mit -94 in GPOS. Beide Quellen muessen mindestens eine
        // negative Zahl fuer irgendein Paar liefern.
        assert!(
            paare.alle().iter().any(|p| p.2 < 0),
            "mindestens ein negativer Kernbetrag"
        );
        // Sanity: die Liste ist nach (links, rechts) sortiert.
        for fenster in paare.alle().windows(2) {
            assert!(
                (fenster[0].0, fenster[0].1) <= (fenster[1].0, fenster[1].1),
                "nicht sortiert"
            );
        }
    }

    #[test]
    fn schrift_ohne_kern_liefert_leer() {
        // NotoMono-Regular hat keine kern- und keine GPOS-Tabelle
        // (GEMESSEN 30.09.2026 mit python3/fontTools).
        let p = std::path::Path::new("/usr/share/fonts/truetype/noto/NotoMono-Regular.ttf");
        if !p.exists() {
            eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
            return;
        }
        let font = TrueTypeFont::load(p).expect("NotoMono laesst sich laden");
        let paare = kern_paare_lesen(&font);
        assert!(paare.ist_leer(), "NotoMono kernt nicht");
        assert_eq!(paare.betrag(1, 2), 0);
    }

    #[test]
    fn bomben_zufallsbytes_in_kern() {
        // 4 KiB Pseudozufall als 'kern' und als 'GPOS'. Darf nichts tun.
        let mut x: u32 = 0x1234_5678;
        let mut daten = Vec::with_capacity(4096);
        for _ in 0..4096 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            daten.push((x >> 16) as u8);
        }
        let mut paare = KernPaare::new();
        kern_format0_lesen(&daten, &mut paare);
        assert!(paare.len() <= MAX_PAARE);
        paare.sortieren();

        let mut paare2 = KernPaare::new();
        gpos_kern_lesen(&daten, &mut paare2);
        assert!(paare2.len() <= MAX_PAARE);
    }

    #[test]
    fn klassenmatrix_ueber_grenze_wird_verworfen() {
        // class1Count = 65535 > MAX_KLASSE_1 -> keine Allokation, kein Panic.
        let mut d = vec![0u8; 64];
        d[0] = 0;
        d[1] = 2; // Format 2
        d[12] = 0xFF;
        d[13] = 0xFF; // class1Count
        d[14] = 0xFF;
        d[15] = 0xFF; // class2Count
        d[4] = 0;
        d[5] = 4; // valueFormat1 = XAdvance
        let mut paare = KernPaare::new();
        gpos_pairpos_format2(&d, 0, &mut paare);
        assert_eq!(paare.len(), 0, "M3: Matrix ueber der Grenze wird verworfen");
    }
}
