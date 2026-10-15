//! Lineare Speicherinstanz (W2). Seitenweise, Grenzen aus `Grenzen`,
//! alle Zugriffe ueberprueft (kein Panic, kein wachsender Speicher).

use crate::fehler::{
    ART_SPEICHER, Fehler, T_SPEICHER_GROESSER, T_SPEICHER_LESEN, T_SPEICHER_SCHREIBEN,
};
use crate::grenzen::Grenzen;

/// Bytes je Seite (2^16). Feste Konstante der Spezifikation.
pub const SEITE: usize = 65_536;
/// Groesste erlaubte Seitenzahl (2^16 Seiten = 4 GiB, Spec-Grenze).
pub const MAX_SEITEN: u64 = 65_536;

/// Eine Speicherinstanz.
#[derive(Debug, Clone)]
pub struct Speicher {
    daten: Vec<u8>,
    max: Option<u64>,
    grenze: u64,
}

impl Speicher {
    /// Neuer Speicher. `min` Seiten, hoechstens `max` (oder `grenze`).
    pub fn neu(min: u64, max: Option<u64>, grenzen: &Grenzen) -> Result<Speicher, Fehler> {
        let grenze = grenzen.speicher_seiten.min(MAX_SEITEN);
        if min > grenze {
            return Err(Fehler::form(
                ART_SPEICHER,
                format!("Speicher: {min} Seiten ueber der Grenze {grenze}"),
            ));
        }
        let m = max.map(|x| x.min(grenze));
        if let Some(m) = m {
            if min > m {
                return Err(Fehler::form(
                    ART_SPEICHER,
                    format!("Speicher: min {min} > max {m}"),
                ));
            }
        }
        // M4: Groesse vor der Allokation geprueft (oben), jetzt allozieren.
        Ok(Speicher {
            daten: vec![0u8; (min as usize) * SEITE],
            max: m,
            grenze,
        })
    }

    /// Aktuelle Groesse in Seiten.
    pub fn seiten(&self) -> u64 {
        (self.daten.len() / SEITE) as u64
    }

    /// Nur lesende Sicht auf die Bytes (W4: `Memory.buffer`).
    pub fn daten_view(&self) -> &[u8] {
        &self.daten
    }

    /// Move the bytes out (an embedder keeps them in its own buffer object
    /// while its code runs, e.g. the JS `ArrayBuffer` of
    /// `WebAssembly.Memory.buffer`). Until [`Speicher::put_bytes`] the memory
    /// looks empty; nothing may run against it in between.
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.daten)
    }

    /// Give back the bytes taken with [`Speicher::take_bytes`].
    pub fn put_bytes(&mut self, daten: Vec<u8>) {
        self.daten = daten;
    }
    pub fn max_seiten(&self) -> Option<u64> {
        self.max
    }
    pub fn byte_len(&self) -> usize {
        self.daten.len()
    }

    /// Wachsen. `delta` Seiten; Rueckgabe = **alte** Seitenzahl, `-1` wenn
    /// es nicht passt (Wasm-Semantik von `memory.grow`).
    pub fn waechst(&mut self, delta: u64) -> i32 {
        let alt = self.seiten();
        // M4/M3: Wachstum hart begrenzen, damit `memory.grow` nicht GiB
        // anfordert.
        if delta > self.grenze {
            return -1;
        }
        let neu = alt.saturating_add(delta);
        let grenze = match self.max {
            Some(m) => m.min(self.grenze),
            None => self.grenze,
        };
        if neu > grenze || neu > MAX_SEITEN {
            return -1;
        }
        // Bytes pruefen, damit die Multiplikation nicht reisst.
        let bytes = match neu.checked_mul(SEITE as u64) {
            Some(b) => b,
            None => return -1,
        };
        // Speichergrenze des Prozesses nie ueberschreiten: die Grenze kommt
        // aus `Grenzen` (Standard 4096 Seiten = 256 MiB).
        self.daten.resize(bytes as usize, 0);
        alt as i32
    }

    /// Bytes lesen (`len` Bytes ab `adresse`). Trap bei OOB.
    pub fn lies(&self, adresse: u64, len: usize) -> Result<&[u8], Fehler> {
        let ende = adresse.saturating_add(len as u64);
        if ende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_LESEN,
                format!(
                    "Lesen {len} Byte ab {adresse}: Speicher hat {} Byte",
                    self.daten.len()
                ),
            ));
        }
        Ok(&self.daten[adresse as usize..ende as usize])
    }

    /// Bytes schreiben. Trap bei OOB, **Rest wird nicht veraendert** —
    /// die Spezifikation verlangt, dass der Schreibzugriff vor dem Trap
    /// wirkungslos ist; wir pruefen vorher vollstaendig.
    pub fn schreib(&mut self, adresse: u64, wert: &[u8]) -> Result<(), Fehler> {
        let ende = adresse.saturating_add(wert.len() as u64);
        if ende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_SCHREIBEN,
                format!(
                    "Schreiben {} Byte ab {adresse}: Speicher hat {} Byte",
                    wert.len(),
                    self.daten.len()
                ),
            ));
        }
        self.daten[adresse as usize..ende as usize].copy_from_slice(wert);
        Ok(())
    }

    /// Schreiben mit Nullbytes (memory.fill).
    pub fn fuelle(&mut self, adresse: u64, wert: u8, len: usize) -> Result<(), Fehler> {
        let ende = adresse.saturating_add(len as u64);
        if ende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_SCHREIBEN,
                format!(
                    "memory.fill: {len} Byte ab {adresse} passen nicht in {} Byte",
                    self.daten.len()
                ),
            ));
        }
        for b in &mut self.daten[adresse as usize..ende as usize] {
            *b = wert;
        }
        Ok(())
    }

    /// Kopieren im Speicher (memory.copy), mit Ueberschneidung erlaubt.
    pub fn kopiere(&mut self, ziel: u64, quelle: u64, len: usize) -> Result<(), Fehler> {
        let zende = ziel.saturating_add(len as u64);
        let qende = quelle.saturating_add(len as u64);
        if zende > self.daten.len() as u64 || qende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_SCHREIBEN,
                format!("memory.copy {len} Byte {quelle}->{ziel} passt nicht"),
            ));
        }
        self.daten
            .copy_within(quelle as usize..qende as usize, ziel as usize);
        Ok(())
    }

    /// Stueck aus einem ANDEREN Speicher an eine Stelle dieses Speichers
    /// schreiben (Multi-Memory `memory.copy ziel quelle`).
    ///
    /// GEMESSEN 03.10. (`memory_copy1.wast`).
    pub fn kopiere_von(&mut self, ziel: u64, stueck: &[u8]) -> Result<(), Fehler> {
        let zende = ziel.saturating_add(stueck.len() as u64);
        if zende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_SCHREIBEN,
                format!("memory.copy {} Byte -> {ziel} passt nicht", stueck.len()),
            ));
        }
        let an = ziel as usize;
        self.daten[an..an + stueck.len()].copy_from_slice(stueck);
        Ok(())
    }

    /// i32 lesen (4 Byte little endian).
    pub fn lies_u32(&self, adresse: u64) -> Result<u32, Fehler> {
        let b = self.lies(adresse, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn lies_u64(&self, adresse: u64) -> Result<u64, Fehler> {
        let b = self.lies(adresse, 8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    pub fn schreib_u32(&mut self, adresse: u64, wert: u32) -> Result<(), Fehler> {
        self.schreib(adresse, &wert.to_le_bytes())
    }
    pub fn schreib_u64(&mut self, adresse: u64, wert: u64) -> Result<(), Fehler> {
        self.schreib(adresse, &wert.to_le_bytes())
    }

    /// Kurztext fuer Fehler/Disassembly.
    pub fn kurz(&self) -> String {
        format!("{} Seiten ({} KiB)", self.seiten(), self.daten.len() / 1024)
    }

    /// Wohlergehen als benanntes Ergebnis (fuer `memory.grow`-Tests).
    pub fn passt_vermehrung(&self, delta: u64) -> bool {
        let neu = self.seiten().saturating_add(delta);
        let grenze = match self.max {
            Some(m) => m.min(self.grenze),
            None => self.grenze,
        };
        neu <= grenze && neu <= MAX_SEITEN && delta <= self.grenze
    }

    /// Seitengrenze (aus `Grenzen`).
    pub fn grenze(&self) -> u64 {
        self.grenze
    }

    /// Trap, wenn der Speicher zu klein fuer einen Zugriff ist (fuer
    /// Schnellpruefungen im Interpreter).
    pub fn pruefe_zugriff(&self, adresse: u64, len: usize) -> Result<(), Fehler> {
        let ende = adresse.saturating_add(len as u64);
        if ende > self.daten.len() as u64 {
            return Err(Fehler::trap(
                T_SPEICHER_LESEN,
                format!(
                    "Zugriff {len} Byte ab {adresse} > {} Byte",
                    self.daten.len()
                ),
            ));
        }
        Ok(())
    }

    /// Speichergrenze fuer `memory.grow`-Fehlermeldungen.
    pub fn max_text(&self) -> String {
        match self.max {
            Some(m) => format!("max {m} Seiten"),
            None => format!("max {} Seiten (Grenze)", self.grenze),
        }
    }

    /// Der Fehler bei einem unmöglichen Wachstum (fuer Tests).
    pub fn zu_gross_fehler(&self, delta: u64) -> Fehler {
        Fehler::trap(
            T_SPEICHER_GROESSER,
            format!(
                "memory.grow({delta}): {} Seiten, {}",
                self.seiten(),
                self.max_text()
            ),
        )
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    fn g() -> Grenzen {
        Grenzen::standard()
    }

    /// M7: Groesse, Wert, Fehlerart sichern.
    #[test]
    fn anlegen_wachsen_traps() {
        let mut s = Speicher::neu(1, Some(3), &g()).unwrap();
        assert_eq!(s.seiten(), 1);
        assert_eq!(s.byte_len(), 65536);
        // i32 schreiben/lesen
        s.schreib_u32(16, 0x1234_5678).unwrap();
        assert_eq!(s.lies_u32(16).unwrap(), 0x1234_5678);
        // wachsen: Rueckgabe ist die ALTE Seitenzahl
        assert_eq!(s.waechst(1), 1);
        assert_eq!(s.seiten(), 2);
        // noch eine Seite (alt = 2)
        assert_eq!(s.waechst(1), 2);
        assert_eq!(s.seiten(), 3);
        // ueber max hinaus -> -1
        assert_eq!(s.waechst(5), -1);
        assert_eq!(s.seiten(), 3);
        // OOB -> benannter Trap (3 Seiten = 196608 Byte)
        assert_eq!(s.byte_len(), 3 * SEITE);
        assert_eq!(s.lies(196_605, 4).unwrap_err().art(), T_SPEICHER_LESEN);
        assert_eq!(s.lies(196_604, 4).unwrap().len(), 4);
        assert_eq!(
            s.schreib(s.byte_len() as u64, &[1, 2]).unwrap_err().art(),
            T_SPEICHER_SCHREIBEN
        );
        assert_eq!(
            s.pruefe_zugriff(u64::MAX, 4).unwrap_err().art(),
            T_SPEICHER_LESEN
        );
    }

    #[test]
    fn kopieren_und_fuellen() {
        let mut s = Speicher::neu(1, None, &g()).unwrap();
        s.schreib(0, &[1, 2, 3, 4, 5]).unwrap();
        // memmove-Semantik: Quelle 0..4 -> Ziel 2..6
        s.kopiere(2, 0, 4).unwrap();
        assert_eq!(s.lies(0, 8).unwrap(), &[1, 2, 1, 2, 3, 4, 0, 0]);
        s.fuelle(0, 0xab, 3).unwrap();
        assert_eq!(s.lies(0, 3).unwrap(), &[0xab, 0xab, 0xab]);
        assert_eq!(
            s.fuelle(65_535, 1, 2).unwrap_err().art(),
            T_SPEICHER_SCHREIBEN
        );
    }

    #[test]
    fn bomben_grenzen() {
        let k = Grenzen::bombe();
        assert_eq!(Speicher::neu(2, None, &k).unwrap_err().art(), ART_SPEICHER);
        let mut s = Speicher::neu(1, None, &k).unwrap();
        assert_eq!(s.waechst(1_000_000), -1);
        assert_eq!(s.seiten(), 1);
        assert!(!s.passt_vermehrung(2));
    }
}
