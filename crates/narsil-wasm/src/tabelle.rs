//! Tabelleninstanzen (W2 Referenztypen). Feste Groesse aus Limits,
//! `table.grow`/`size`/`fill` mit Grenzen, Trap bei OOB.

use crate::fehler::{
    ART_TABELLE, Fehler, T_TABELLE_GROESSER, T_TABELLE_LESEN, T_TABELLE_SCHREIBEN,
};
use crate::grenzen::Grenzen;
use crate::modell::Reftyp;
use crate::wert::Ref;

/// Eine Tabelle: `Vec<Option<Ref>>` mit fester Anfangsgroesse.
#[derive(Debug, Clone)]
pub struct Tabelle {
    reftyp: Reftyp,
    eintraege: Vec<Option<Ref>>,
    max: Option<u64>,
    grenze: u64,
}

impl Tabelle {
    /// M4: `min` wird VOR der Allokation geprueft (das ist der
    /// `Vec::with_capacity`-Angriff aus M4).
    pub fn neu(
        min: u64,
        max: Option<u64>,
        reftyp: Reftyp,
        grenzen: &Grenzen,
    ) -> Result<Tabelle, Fehler> {
        let grenze = grenzen
            .tabelle_plaetze
            .min(crate::grenzen::MAX_TABELLENPLAETZE_HART);
        if min > grenze {
            return Err(Fehler::form(
                ART_TABELLE,
                format!("Tabelle: {min} Eintraege ueber der Grenze {grenze}"),
            ));
        }
        if let Some(m) = max {
            if min > m {
                return Err(Fehler::form(
                    ART_TABELLE,
                    format!("Tabelle: min {min} > max {m}"),
                ));
            }
        }
        // Der Speicherbedarf je Eintrag ist klein, aber 2^32 Eintraege
        // wären 32 GiB -> die Grenze aus `Grenzen` ist die eigentliche
        // Bremse (Standard 1 Mi Eintraege = 8 MiB).
        Ok(Tabelle {
            reftyp,
            eintraege: vec![None; min as usize],
            max,
            grenze,
        })
    }

    pub fn reftyp(&self) -> Reftyp {
        self.reftyp
    }
    /// Passt zu `len` (Clippy).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> u64 {
        self.eintraege.len() as u64
    }
    pub fn max(&self) -> Option<u64> {
        self.max
    }
    pub fn leeren_darstellung(&self, ende: usize) -> Vec<String> {
        self.eintraege
            .iter()
            .take(ende)
            .enumerate()
            .map(|(i, r)| match r {
                None => format!("{i}: null"),
                Some(Ref::Func(f)) => format!("{i}: funcref({f})"),
                Some(Ref::Extern(e)) => format!("{i}: externref({e})"),
                Some(Ref::Gcref(g)) => format!("{i}: arrayref({g})"),
            })
            .collect()
    }

    /// Lesen mit Trap.
    pub fn lies(&self, index: u64) -> Result<Option<Ref>, Fehler> {
        if index >= self.len() {
            return Err(Fehler::trap(
                T_TABELLE_LESEN,
                format!("Index {index} ausserhalb der Tabelle (0..{})", self.len()),
            ));
        }
        Ok(self.eintraege[index as usize])
    }

    /// Schreiben mit Trap.
    pub fn schreib(&mut self, index: u64, wert: Option<Ref>) -> Result<(), Fehler> {
        if index >= self.len() {
            return Err(Fehler::trap(
                T_TABELLE_SCHREIBEN,
                format!("Index {index} ausserhalb der Tabelle (0..{})", self.len()),
            ));
        }
        // Typpruefung (der Validator kann den Hostwert nicht kennen).
        if let Some(r) = wert {
            // Wie in `init`: zur Laufzeit zaehlt nur funcref gegen externref;
            // ein konkreter Typindex (`(ref null $t)`) ist zur Laufzeit
            // ebenfalls eine Funktionsreferenz — die Typtreue prueft der
            // Validator. GEMESSEN 02.10. (`table.wast` Z. 34).
            let passt = match r {
                Ref::Func(_) => self.reftyp.ist_funktion(),
                // Ein EXTERNALISIERTER Hostwert (`externref`) gehoert in
                // eine `externref`-Tabelle; ein INTERNALISIERTER
                // (`any.convert_extern`, also ein `anyref`-Wert) in eine
                // GC-Tabelle. `Ref::Extern` traegt immer einen Wert — ein
                // `null` steht als `None` im Tabelleneintrag.
                //
                // GEMESSEN 03.10. (`extern.wast` Z. 45/60: „irgendein
                // nicht-nuller externref erwartet, null" und „externref(0)
                // erwartet, null" — der `table.set` in `init` wurde
                // abgelehnt).
                Ref::Extern(_) => self.reftyp.ist_extern() || self.reftyp.nimmt_gc(),
                Ref::Gcref(_) => {
                    self.reftyp.ist_array_wildcard()
                        || self.reftyp.ist_funktion()
                        || self.reftyp.nimmt_gc()
                }
            };
            if !passt {
                return Err(Fehler::trap(
                    T_TABELLE_SCHREIBEN,
                    format!("Wert passt nicht zum Tabellentyp {}", self.reftyp.text()),
                ));
            }
        }
        self.eintraege[index as usize] = wert;
        Ok(())
    }

    /// `table.grow`: Rueckgabe = **alte** Laenge, `-1` wenn es nicht passt.
    pub fn waechst(&mut self, delta: u64, wert: Option<Ref>) -> i32 {
        let alt = self.len();
        let grenze = match self.max {
            Some(m) => m.min(self.grenze),
            None => self.grenze,
        };
        if delta > self.grenze {
            return -1;
        }
        let neu = alt.saturating_add(delta);
        if neu > grenze {
            return -1;
        }
        // M4: `resize` ist begrenzt, weil `neu <= grenze`.
        self.eintraege.resize(neu as usize, wert);
        alt as i32
    }

    /// `table.fill`.
    pub fn fuelle(&mut self, start: u64, wert: Option<Ref>, len: u64) -> Result<(), Fehler> {
        let ende = start.saturating_add(len);
        if ende > self.len() {
            return Err(Fehler::trap(
                T_TABELLE_SCHREIBEN,
                format!("table.fill {start}..{ende} > {} Eintraege", self.len()),
            ));
        }
        if let Some(r) = wert {
            // Wie in `init`: zur Laufzeit zaehlt nur funcref gegen externref;
            // ein konkreter Typindex (`(ref null $t)`) ist zur Laufzeit
            // ebenfalls eine Funktionsreferenz — die Typtreue prueft der
            // Validator. GEMESSEN 02.10. (`table.wast` Z. 34).
            let passt = match r {
                Ref::Func(_) => self.reftyp.ist_funktion(),
                // Ein EXTERNALISIERTER Hostwert (`externref`) gehoert in
                // eine `externref`-Tabelle; ein INTERNALISIERTER
                // (`any.convert_extern`, also ein `anyref`-Wert) in eine
                // GC-Tabelle. `Ref::Extern` traegt immer einen Wert — ein
                // `null` steht als `None` im Tabelleneintrag.
                //
                // GEMESSEN 03.10. (`extern.wast` Z. 45/60: „irgendein
                // nicht-nuller externref erwartet, null" und „externref(0)
                // erwartet, null" — der `table.set` in `init` wurde
                // abgelehnt).
                Ref::Extern(_) => self.reftyp.ist_extern() || self.reftyp.nimmt_gc(),
                Ref::Gcref(_) => {
                    self.reftyp.ist_array_wildcard()
                        || self.reftyp.ist_funktion()
                        || self.reftyp.nimmt_gc()
                }
            };
            if !passt {
                return Err(Fehler::trap(
                    T_TABELLE_SCHREIBEN,
                    "table.fill: Typ passt nicht",
                ));
            }
        }
        for i in start..ende {
            self.eintraege[i as usize] = wert;
        }
        Ok(())
    }

    /// `table.copy`.
    pub fn kopiere(&mut self, ziel: u64, quelle: u64, len: u64) -> Result<(), Fehler> {
        let zende = ziel.saturating_add(len);
        let qende = quelle.saturating_add(len);
        if zende > self.len() || qende > self.len() {
            return Err(Fehler::trap(
                T_TABELLE_SCHREIBEN,
                format!(
                    "table.copy {quelle}+{len} -> {ziel}+{len} > {} Eintraege",
                    self.len()
                ),
            ));
        }
        // Ueberschneidung: ueber einen Zwischenpuffer kopieren.
        let stuecke: Vec<Option<Ref>> = (quelle..qende)
            .map(|i| self.eintraege[i as usize])
            .collect();
        for (k, w) in stuecke.into_iter().enumerate() {
            self.eintraege[(ziel + k as u64) as usize] = w;
        }
        Ok(())
    }

    /// `table.init` aus einem (passiven) Elementabschnitt.
    pub fn init(
        &mut self,
        element: &[Option<Ref>],
        ziel: u64,
        quelle: u64,
        len: u64,
    ) -> Result<(), Fehler> {
        let zende = ziel.saturating_add(len);
        let qende = quelle.saturating_add(len);
        if zende > self.len() || qende > element.len() as u64 {
            return Err(Fehler::trap(
                T_TABELLE_SCHREIBEN,
                format!(
                    "table.init {quelle}+{len} -> {ziel}+{len}; Element hat {}, Tabelle {}",
                    element.len(),
                    self.len()
                ),
            ));
        }
        for k in 0..len {
            let w = element[(quelle + k) as usize];
            if let Some(r) = w {
                // Zur Laufzeit unterscheiden sich nur `funcref` und
                // `externref`; ein konkreter Typindex (`(ref null $t)`)
                // ist zur Laufzeit ebenfalls eine Funktionsreferenz — die
                // Typtreue prueft der Validator.
                //
                // GEMESSEN 02.10. (`br_table.wast`).
                let passt = match r {
                    Ref::Func(_) => self.reftyp.ist_funktion(),
                    // Ein EXTERNALISIERTER Hostwert (`externref`) gehoert in
                    // eine `externref`-Tabelle; ein INTERNALISIERTER
                    // (`any.convert_extern`, also ein `anyref`-Wert) in eine
                    // GC-Tabelle. `Ref::Extern` traegt immer einen Wert — ein
                    // `null` steht als `None` im Tabelleneintrag.
                    //
                    // GEMESSEN 03.10. (`extern.wast` Z. 45/60: „irgendein
                    // nicht-nuller externref erwartet, null" und „externref(0)
                    // erwartet, null" — der `table.set` in `init` wurde
                    // abgelehnt).
                    Ref::Extern(_) => self.reftyp.ist_extern() || self.reftyp.nimmt_gc(),
                    Ref::Gcref(_) => {
                        self.reftyp.ist_array_wildcard()
                            || self.reftyp.ist_funktion()
                            || self.reftyp.nimmt_gc()
                    }
                };
                if !passt {
                    return Err(Fehler::trap(
                        T_TABELLE_SCHREIBEN,
                        "table.init: Typ passt nicht",
                    ));
                }
            }
            self.eintraege[(ziel + k) as usize] = w;
        }
        Ok(())
    }

    /// Fehler bei unmöglichem Wachstum (fuer Tests/Debug-Ausgabe).
    pub fn zu_gross_fehler(&self, delta: u64) -> Fehler {
        Fehler::trap(
            T_TABELLE_GROESSER,
            format!(
                "table.grow({delta}): {} Eintraege, max {:?}",
                self.len(),
                self.max
            ),
        )
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Laenge, Wert, Fehlerart sichern.
    #[test]
    fn lesen_schreiben_wachsen() {
        let mut t = Tabelle::neu(3, Some(5), Reftyp::Func, &Grenzen::standard()).unwrap();
        assert_eq!(t.len(), 3);
        t.schreib(1, Some(Ref::Func(7))).unwrap();
        assert_eq!(t.lies(1).unwrap(), Some(Ref::Func(7)));
        assert_eq!(t.lies(0).unwrap(), None);
        // OOB
        assert_eq!(t.lies(3).unwrap_err().art(), T_TABELLE_LESEN);
        assert_eq!(t.schreib(9, None).unwrap_err().art(), T_TABELLE_SCHREIBEN);
        // falscher Typ
        assert_eq!(
            t.schreib(0, Some(Ref::Extern(1))).unwrap_err().art(),
            T_TABELLE_SCHREIBEN
        );
        // wachsen: Rueckgabe = alte Laenge
        assert_eq!(t.waechst(2, None), 3);
        assert_eq!(t.len(), 5);
        assert_eq!(t.waechst(1, None), -1);
        assert_eq!(t.len(), 5);
    }

    #[test]
    fn fill_copy_init() {
        let mut t = Tabelle::neu(4, None, Reftyp::Func, &Grenzen::standard()).unwrap();
        t.fuelle(1, Some(Ref::Func(3)), 2).unwrap();
        assert_eq!(t.lies(2).unwrap(), Some(Ref::Func(3)));
        assert_eq!(t.fuelle(3, None, 2).unwrap_err().art(), T_TABELLE_SCHREIBEN);
        t.kopiere(0, 1, 2).unwrap();
        assert_eq!(t.lies(0).unwrap(), Some(Ref::Func(3)));
        let el = vec![Some(Ref::Func(9)), Some(Ref::Func(8))];
        t.init(&el, 2, 0, 2).unwrap();
        assert_eq!(t.lies(3).unwrap(), Some(Ref::Func(8)));
        assert_eq!(t.init(&el, 3, 0, 2).unwrap_err().art(), T_TABELLE_SCHREIBEN);
    }

    #[test]
    fn grenzen() {
        let k = Grenzen::bombe();
        assert_eq!(
            Tabelle::neu(100, None, Reftyp::Func, &k).unwrap_err().art(),
            ART_TABELLE
        );
        let mut t = Tabelle::neu(1, Some(4), Reftyp::Extern, &k).unwrap();
        assert_eq!(t.waechst(100, None), -1);
        assert_eq!(t.len(), 1);
        assert_eq!(t.leeren_darstellung(5).len(), 1);
    }
}
