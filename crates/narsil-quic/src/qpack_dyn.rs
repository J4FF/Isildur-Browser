//! QPACK dynamische Tabelle und Stroeme (RFC 9204, Abschnitt 3-4).
//!
//! Ergänzt die statische Tabelle aus `qpack.rs` um die dynamische Tabelle,
//! die Encoder-/Decoder-Anweisungen und die vollständige Dekodierung eines
//! Feldabschnitts MIT dynamischen Verweisen (Required Insert Count, Base,
//! Post-Base-Indizes).
//!
//! M3/M4: die Kapazitaet ist begrenzt, blockierte Stroeme sind gezaehlt und
//! begrenzt, jeder Index wird vor dem Zugriff geprueft.

use crate::qpack::{FeldZeile, QpackFehler, ganzzahl_lesen, statik_eintrag, zeichenkette_lesen};

/// Hoechste Zahl gleichzeitig blockierter Stroeme (M3, RFC 9204 Abschnitt 2.1.2).
pub const MAX_BLOCKIERTE_STROEME: usize = 64;

/// Die dynamische Tabelle (RFC 9204 Abschnitt 3.2).
#[derive(Debug, Clone, Default)]
pub struct DynamischeTabelle {
    /// Aelteste zuerst; der absolute Index von `eintraege[i]` ist
    /// `verworfen + i`.
    eintraege: Vec<FeldZeile>,
    /// Zahl der bereits verdraengten Eintraege.
    verworfen: usize,
    kapazitaet: u64,
    groesse: u64,
}

impl DynamischeTabelle {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Gesamtzahl der Einfuegungen (Insert Count).
    pub fn insert_count(&self) -> u64 {
        (self.verworfen + self.eintraege.len()) as u64
    }

    pub fn groesse(&self) -> u64 {
        self.groesse
    }

    pub fn kapazitaet(&self) -> u64 {
        self.kapazitaet
    }

    /// Setzt die Kapazitaet und verdraengt bei Bedarf (RFC 9204 Abschnitt 3.2.2).
    pub fn setze_kapazitaet(&mut self, kapazitaet: u64) {
        self.kapazitaet = kapazitaet;
        self.kuerze();
    }

    /// Fuegt einen Eintrag ein; verdraengt aeltere, bis er Platz hat.
    pub fn einfuegen(&mut self, name: &[u8], wert: &[u8]) -> Result<(), QpackFehler> {
        let zusatz = name.len() as u64 + wert.len() as u64 + 32;
        if zusatz > self.kapazitaet {
            // Groesser als die Kapazitaet: Tabelle leeren (RFC 9204 3.2.2).
            self.verworfen += self.eintraege.len();
            self.eintraege.clear();
            self.groesse = 0;
            return Ok(());
        }
        while self.groesse + zusatz > self.kapazitaet && !self.eintraege.is_empty() {
            let alt = self.eintraege.remove(0);
            self.verworfen += 1;
            self.groesse -= alt.name.len() as u64 + alt.wert.len() as u64 + 32;
        }
        self.eintraege.push(FeldZeile {
            name: name.to_vec(),
            wert: wert.to_vec(),
        });
        self.groesse += zusatz;
        Ok(())
    }

    fn kuerze(&mut self) {
        while self.groesse > self.kapazitaet && !self.eintraege.is_empty() {
            let alt = self.eintraege.remove(0);
            self.verworfen += 1;
            self.groesse -= alt.name.len() as u64 + alt.wert.len() as u64 + 32;
        }
    }

    /// Eintrag ueber den absoluten Index.
    pub fn absolut(&self, index: u64) -> Option<&FeldZeile> {
        let pos = index.checked_sub(self.verworfen as u64)? as usize;
        self.eintraege.get(pos)
    }

    /// Eintrag ueber den relativen Index (Base - Index - 1).
    pub fn relativ(&self, base: u64, index: u64) -> Option<&FeldZeile> {
        let abs = base.checked_sub(index + 1)?;
        self.absolut(abs)
    }

    /// Eintrag ueber den Post-Base-Index (Base + Index).
    pub fn post_base(&self, base: u64, index: u64) -> Option<&FeldZeile> {
        self.absolut(base + index)
    }

    /// Absoluter Index des neuesten Eintrags mit diesem Namen.
    pub fn name_index(&self, name: &[u8]) -> Option<u64> {
        for (i, e) in self.eintraege.iter().enumerate().rev() {
            if e.name == name {
                return Some((self.verworfen + i) as u64);
            }
        }
        None
    }
}

/// Eine Encoder-Anweisung (RFC 9204 Abschnitt 4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderAnweisung {
    SetzeKapazitaet(u64),
    EinfuegenNameRef {
        statisch: bool,
        index: u64,
        wert: Vec<u8>,
    },
    EinfuegenLiteralName {
        name: Vec<u8>,
        wert: Vec<u8>,
    },
    Duplizieren {
        relativ_index: u64,
    },
}

/// Liest eine Encoder-Anweisung. Liefert `(anweisung, verbraucht)`.
pub fn encoder_anweisung_lesen(daten: &[u8]) -> Result<(EncoderAnweisung, usize), QpackFehler> {
    if daten.is_empty() {
        return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    let erste = daten[0];
    if erste & 0x80 != 0 {
        // `1 T NameIndex(6+)` + Wert.
        let statisch = erste & 0x40 != 0;
        let (index, n) = ganzzahl_lesen(daten, 6)?;
        let (wert, n2) = zeichenkette_lesen(&daten[n..])?;
        Ok((
            EncoderAnweisung::EinfuegenNameRef {
                statisch,
                index,
                wert,
            },
            n + n2,
        ))
    } else if erste & 0x40 != 0 {
        // `01 H NameLaenge(5+)` + Name + Wert.
        let huffman = erste & 0x20 != 0;
        let (name_laenge, n) = ganzzahl_lesen(daten, 5)?;
        let name_ende = n + name_laenge as usize;
        if daten.len() < name_ende {
            return Err(QpackFehler::Abgeschnitten {
                braucht: name_ende,
                da: daten.len(),
            });
        }
        let name = if huffman {
            crate::qpack::huffman_dekodiere(&daten[n..name_ende])?
        } else {
            daten[n..name_ende].to_vec()
        };
        let (wert, n2) = zeichenkette_lesen(&daten[name_ende..])?;
        Ok((
            EncoderAnweisung::EinfuegenLiteralName { name, wert },
            name_ende + n2,
        ))
    } else if erste & 0x20 != 0 {
        // `001 Kapazitaet(5+)`.
        let (kapazitaet, n) = ganzzahl_lesen(daten, 5)?;
        Ok((EncoderAnweisung::SetzeKapazitaet(kapazitaet), n))
    } else {
        // `000 Index(5+)`.
        let (index, n) = ganzzahl_lesen(daten, 5)?;
        Ok((
            EncoderAnweisung::Duplizieren {
                relativ_index: index,
            },
            n,
        ))
    }
}

/// Eine Decoder-Anweisung (RFC 9204 Abschnitt 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderAnweisung {
    AbschnittBestaetigt { stream_id: u64 },
    StromAbgebrochen { stream_id: u64 },
    EinfuegeZaehlerInkrement { inkrement: u64 },
}

/// Liest eine Decoder-Anweisung. Liefert `(anweisung, verbraucht)`.
pub fn decoder_anweisung_lesen(daten: &[u8]) -> Result<(DecoderAnweisung, usize), QpackFehler> {
    if daten.is_empty() {
        return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    let erste = daten[0];
    if erste & 0x80 != 0 {
        let (stream_id, n) = ganzzahl_lesen(daten, 7)?;
        Ok((DecoderAnweisung::AbschnittBestaetigt { stream_id }, n))
    } else if erste & 0x40 != 0 {
        let (stream_id, n) = ganzzahl_lesen(daten, 6)?;
        Ok((DecoderAnweisung::StromAbgebrochen { stream_id }, n))
    } else {
        let (inkrement, n) = ganzzahl_lesen(daten, 6)?;
        Ok((DecoderAnweisung::EinfuegeZaehlerInkrement { inkrement }, n))
    }
}

/// Rekonstruiert den Required Insert Count (RFC 9204 Abschnitt 4.5.1.1).
fn ric_rekonstruieren(
    enc: u64,
    total_einfuegungen: u64,
    max_eintraege: u64,
) -> Result<u64, QpackFehler> {
    if enc == 0 {
        return Ok(0);
    }
    if max_eintraege == 0 {
        return Err(QpackFehler::RicUngueltig { ist: enc });
    }
    let vollbereich = 2 * max_eintraege;
    if enc > vollbereich {
        return Err(QpackFehler::RicUngueltig { ist: enc });
    }
    let max_wert = total_einfuegungen + max_eintraege;
    let max_gewickelt = (max_wert / vollbereich) * vollbereich;
    let mut ric = max_gewickelt + enc - 1;
    if ric > max_wert {
        if ric <= vollbereich {
            return Err(QpackFehler::RicUngueltig { ist: enc });
        }
        ric -= vollbereich;
    }
    if ric == 0 {
        return Err(QpackFehler::RicUngueltig { ist: enc });
    }
    Ok(ric)
}

/// Ein vollständiger QPACK-Dekoder: dynamische Tabelle, Encoder-/Decoder-
/// Anweisungen und Feldabschnitte.
pub struct QpackDekoder {
    tabelle: DynamischeTabelle,
    /// Maximal erlaubte Kapazitaet (SETTINGS_QPACK_MAX_TABLE_CAPACITY).
    max_kapazitaet: u64,
    /// Zahl der gerade blockierten Stroeme.
    blockierte_stroeme: usize,
    /// Obergrenze fuer blockierte Stroeme (SETTINGS_QPACK_BLOCKED_STREAMS).
    max_blockiert: usize,
}

impl QpackDekoder {
    pub fn neu(max_kapazitaet: u64, max_blockiert: usize) -> Self {
        QpackDekoder {
            tabelle: DynamischeTabelle::neu(),
            max_kapazitaet,
            blockierte_stroeme: 0,
            max_blockiert: max_blockiert.min(MAX_BLOCKIERTE_STROEME),
        }
    }

    pub fn insert_count(&self) -> u64 {
        self.tabelle.insert_count()
    }

    fn max_eintraege(&self) -> u64 {
        self.max_kapazitaet / 32
    }

    /// Verarbeitet eine Encoder-Anweisung und pflegt die Tabelle.
    pub fn encoder_anweisung(&mut self, daten: &[u8]) -> Result<(), QpackFehler> {
        let (anweisung, _) = encoder_anweisung_lesen(daten)?;
        match anweisung {
            EncoderAnweisung::SetzeKapazitaet(k) => {
                if k > self.max_kapazitaet {
                    return Err(QpackFehler::KapazitaetZuGross {
                        ist: k,
                        maximum: self.max_kapazitaet,
                    });
                }
                self.tabelle.setze_kapazitaet(k);
            }
            EncoderAnweisung::EinfuegenNameRef {
                statisch,
                index,
                wert,
            } => {
                let name = if statisch {
                    statik_eintrag(index)?.0.to_vec()
                } else {
                    // Relativer Index in der Encoder-Anweisung: 0 = neuester.
                    let abs = self
                        .tabelle
                        .insert_count()
                        .checked_sub(index + 1)
                        .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?;
                    self.tabelle
                        .absolut(abs)
                        .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?
                        .name
                        .clone()
                };
                self.tabelle.einfuegen(&name, &wert)?;
            }
            EncoderAnweisung::EinfuegenLiteralName { name, wert } => {
                self.tabelle.einfuegen(&name, &wert)?;
            }
            EncoderAnweisung::Duplizieren { relativ_index } => {
                let abs = self
                    .tabelle
                    .insert_count()
                    .checked_sub(relativ_index + 1)
                    .ok_or(QpackFehler::UngueltigerDynamikIndex {
                        index: relativ_index,
                    })?;
                let eintrag = self
                    .tabelle
                    .absolut(abs)
                    .ok_or(QpackFehler::UngueltigerDynamikIndex {
                        index: relativ_index,
                    })?
                    .clone();
                self.tabelle.einfuegen(&eintrag.name, &eintrag.wert)?;
            }
        }
        Ok(())
    }

    /// Dekodiert einen Feldabschnitt (mit dynamischen Verweisen).
    pub fn feldabschnitt(&mut self, daten: &[u8]) -> Result<Vec<FeldZeile>, QpackFehler> {
        if daten.len() < 2 {
            return Err(QpackFehler::Abgeschnitten {
                braucht: 2,
                da: daten.len(),
            });
        }
        let (enc_ric, n1) = ganzzahl_lesen(daten, 8)?;
        let rest = &daten[n1..];
        if rest.is_empty() {
            return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
        }
        let sign = rest[0] & 0x80 != 0;
        let (delta_base, n2) = ganzzahl_lesen(rest, 7)?;
        let rest = &rest[n2..];

        let ric = ric_rekonstruieren(enc_ric, self.insert_count(), self.max_eintraege())?;
        let base = if sign {
            if ric <= delta_base {
                return Err(QpackFehler::RicUngueltig { ist: ric });
            }
            ric - delta_base - 1
        } else {
            ric + delta_base
        };

        // Blockierte Stroeme (RFC 9204 Abschnitt 2.2.1).
        if ric > self.insert_count() {
            self.blockierte_stroeme += 1;
            if self.blockierte_stroeme > self.max_blockiert {
                return Err(QpackFehler::ZuVieleBlockierteStroeme);
            }
            return Err(QpackFehler::RicUngueltig { ist: ric });
        }

        let mut felder = Vec::new();
        let mut rest = rest;
        while !rest.is_empty() {
            if felder.len() >= crate::qpack::MAX_FELDER {
                return Err(QpackFehler::ZuVieleFelder {
                    ist: felder.len() + 1,
                    maximum: crate::qpack::MAX_FELDER,
                });
            }
            let erste = rest[0];
            if erste & 0x80 != 0 {
                // Indexed: `1 T index(6)`.
                if erste & 0x40 != 0 {
                    let (index, n) = ganzzahl_lesen(rest, 6)?;
                    let (name, wert) = statik_eintrag(index)?;
                    felder.push(FeldZeile {
                        name: name.to_vec(),
                        wert: wert.to_vec(),
                    });
                    rest = &rest[n..];
                } else {
                    let (index, n) = ganzzahl_lesen(rest, 6)?;
                    let e = self
                        .tabelle
                        .relativ(base, index)
                        .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?;
                    felder.push(e.clone());
                    rest = &rest[n..];
                }
            } else if erste & 0x40 != 0 {
                // Literal mit Namensverweis: `01 N T index(4)` + Wert.
                let t_statisch = erste & 0x10 != 0;
                let (index, n) = ganzzahl_lesen(rest, 4)?;
                let (wert, n2) = zeichenkette_lesen(&rest[n..])?;
                let name = if t_statisch {
                    statik_eintrag(index)?.0.to_vec()
                } else {
                    self.tabelle
                        .relativ(base, index)
                        .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?
                        .name
                        .clone()
                };
                felder.push(FeldZeile { name, wert });
                rest = &rest[n + n2..];
            } else if erste & 0x20 != 0 {
                // Literal mit Literalnamen: `001 N H laenge(3)`.
                let name_huffman = erste & 0x08 != 0;
                let (name_laenge, n) = ganzzahl_lesen(rest, 3)?;
                let name_ende = n + name_laenge as usize;
                if rest.len() < name_ende {
                    return Err(QpackFehler::Abgeschnitten {
                        braucht: name_ende,
                        da: rest.len(),
                    });
                }
                let name = if name_huffman {
                    crate::qpack::huffman_dekodiere(&rest[n..name_ende])?
                } else {
                    rest[n..name_ende].to_vec()
                };
                let (wert, n2) = zeichenkette_lesen(&rest[name_ende..])?;
                felder.push(FeldZeile { name, wert });
                rest = &rest[name_ende + n2..];
            } else if erste & 0x10 != 0 {
                // Indexed mit Post-Base-Index: `0001 index(4)`.
                let (index, n) = ganzzahl_lesen(rest, 4)?;
                let e = self
                    .tabelle
                    .post_base(base, index)
                    .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?;
                felder.push(e.clone());
                rest = &rest[n..];
            } else {
                // Literal mit Post-Base-Namensverweis: `0000 N index(3)`.
                let (index, n) = ganzzahl_lesen(rest, 3)?;
                let (wert, n2) = zeichenkette_lesen(&rest[n..])?;
                let e = self
                    .tabelle
                    .post_base(base, index)
                    .ok_or(QpackFehler::UngueltigerDynamikIndex { index })?;
                felder.push(FeldZeile {
                    name: e.name.clone(),
                    wert,
                });
                rest = &rest[n + n2..];
            }
        }
        Ok(felder)
    }
}
