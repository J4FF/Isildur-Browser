//! Bequemer Lesekopf ueber eine Bytekette mit harten Grenzen (M3/M4).
//!
//! Alles, was aus der Eingabe kommt, laeuft hier durch:
//!   * LEB128 wird mit einer MAX-BYTE-ZAHL gelesen, damit eine Kette von
//!     `0x80`-Bytes nicht ewig laeuft und keine `u64` ueberlaeuft,
//!   * Laengen werden VOR der Allokation geprueft (`nimm` reserviert nichts,
//!     sondern leiht sich ein Fenster aus dem Original - null Kopie),
//!   * `name` prueft UTF-8 und die Namentabelle des Projekts.

use crate::fehler::{ART_GRENZE, ART_LAENGE, ART_LEB, ART_UNVOLLSTAENDIG, ART_UTF8, Fehler};

/// LEB128 fuer u32: 5 Bytes (Spec-Obergrenze).
pub const MAX_LEB_U32: usize = 5;
/// LEB128 fuer u64: 10 Bytes (Spec-Obergrenze).
pub const MAX_LEB_U64: usize = 10;
/// s33 (Blocktyp als vorzeichenbehaftete LEB128): ceil(33/7) = 5 Bytes.
pub const MAX_LEB_S33: usize = 5;

/// Lesekopf. `pos` wandert; jede Leseart prueft vorher gegen das Ende.
#[derive(Debug, Clone)]
pub struct Leser<'a> {
    daten: &'a [u8],
    pos: usize,
    grenze_name: usize,
    /// Zusaetzliches hartes Feldlimit (M4). `None` = nur die Modulgrenze.
    max_feld: Option<usize>,
}

impl<'a> Leser<'a> {
    pub fn neu(daten: &'a [u8]) -> Self {
        Leser {
            daten,
            pos: 0,
            grenze_name: crate::grenzen::MAX_NAME_BYTES_HART,
            max_feld: None,
        }
    }

    /// Feldgrenze setzen (aus `Grenzen`).
    pub fn mit_feldgrenze(mut self, g: usize) -> Self {
        self.max_feld = Some(g);
        self
    }

    /// Namensgrenze setzen (aus `Grenzen::name_bytes`).
    pub fn mit_namengrenze(mut self, g: usize) -> Self {
        self.grenze_name = g.min(crate::grenzen::MAX_NAME_BYTES_HART);
        self
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn uebrig(&self) -> usize {
        self.daten.len() - self.pos
    }
    pub fn ist_ende(&self) -> bool {
        self.pos >= self.daten.len()
    }
    /// Ganze Kette (z. B. fuer Fehlertexte).
    pub fn alle(&self) -> &'a [u8] {
        self.daten
    }

    /// Setze den Leser an eine absolute Position (fuer Unterketten).
    pub fn gehe_zu(&mut self, p: usize) {
        self.pos = p.min(self.daten.len());
    }

    fn braucht(&self, n: usize, was: &str) -> Result<(), Fehler> {
        if self.uebrig() < n {
            return Err(Fehler::form(
                ART_UNVOLLSTAENDIG,
                format!(
                    "{was}: {} Byte noetig, nur {} da (Pos {})",
                    n,
                    self.uebrig(),
                    self.pos
                ),
            ));
        }
        Ok(())
    }

    /// Hartgrenze (M3/M4).
    pub fn grenze(&self, n: usize, was: &str) -> Result<(), Fehler> {
        if n > self.grenzen_max() {
            return Err(Fehler::form(
                ART_GRENZE,
                format!("{was}: {n} ueber der Grenze {}", self.grenzen_max()),
            ));
        }
        Ok(())
    }
    /// Hartgrenze aus den Modulgrenzen; im Decoder sind es die MAX_*-Werte.
    fn grenzen_max(&self) -> usize {
        // Im Binaerdecoder wird `max_feld` gesetzt; sonst gilt das groesste
        // Feld, das ueberhaupt geprueft wird (Modulbytes).
        self.max_feld.unwrap_or(crate::grenzen::MAX_WAST_BYTES_HART)
    }

    /// Ein Byte.
    pub fn u8(&mut self, was: &str) -> Result<u8, Fehler> {
        self.braucht(1, was)?;
        let b = self.daten[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Peek ohne Fortschritt.
    pub fn sieht(&self) -> Option<u8> {
        self.daten.get(self.pos).copied()
    }

    /// `n` Byte als Fenster auf die Originalkette: KEINE Allokation.
    pub fn nimm(&mut self, n: usize, was: &str) -> Result<&'a [u8], Fehler> {
        self.braucht(n, was)?;
        let s = &self.daten[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Zahl als `n` Byte kopieren (fuer konstante Felder).
    pub fn nimm_kopie(&mut self, n: usize, was: &str) -> Result<Vec<u8>, Fehler> {
        Ok(self.nimm(n, was)?.to_vec())
    }

    /// 2 Byte little endian (DWARF-Versionsangabe).
    pub fn u16le(&mut self, was: &str) -> Result<u16, Fehler> {
        let b = self.nimm(2, was)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// 4 Byte little endian.
    pub fn u32le(&mut self, was: &str) -> Result<u32, Fehler> {
        let b = self.nimm(4, was)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// LEB128 u32, hoechstens `MAX_LEB_U32` Byte. Prueft zusaetzlich, dass
    /// die Kodierung kanonisch ist, wenn `kanonisch` gefordert wird
    /// (ueberlaengige Formen sind laut Spec ungueltig).
    pub fn leb_u32(&mut self, kanonisch: bool) -> Result<u32, Fehler> {
        let mut ergebnis: u32 = 0;
        let mut schiebe: u32 = 0;
        for i in 0..MAX_LEB_U32 {
            let b = self.u8("LEB128 u32")?;
            let nutzlast = (b & 0x7f) as u32;
            if i == MAX_LEB_U32 - 1 {
                // Fuenftes Byte: nur 4 Bit erlaubt (Rest passt in 32 Bit)
                if b & 0x70 != 0 {
                    return Err(Fehler::form(
                        ART_LEB,
                        format!("LEB128 u32: {b:#04x} am Byte {i} ragt ueber 32 Bit"),
                    ));
                }
            }
            // GEMESSEN 01.10. (`binary-leb128.wast`): nicht-kanonische
            // (verlaengerte) Kodierung ist bei u32/u64 ERLAUBT, solange die
            // Bytegrenze stimmt ("\00\82\80\80\80\00" ist gueltig). Nur
            // Bits oberhalb der 32/64 Bit werden abgelehnt — siehe die
            // Byte-Pruefung oben.
            let _ = kanonisch;
            ergebnis |= nutzlast << schiebe;
            if b & 0x80 == 0 {
                return Ok(ergebnis);
            }
            schiebe += 7;
        }
        Err(Fehler::form(
            ART_LEB,
            format!(
                "LEB128 u32 nicht beendet nach {MAX_LEB_U32} Byte (Pos {})",
                self.pos
            ),
        ))
    }

    /// LEB128 u64.
    pub fn leb_u64(&mut self, kanonisch: bool) -> Result<u64, Fehler> {
        let _ = kanonisch; // siehe leb_s32
        let mut ergebnis: u64 = 0;
        let mut schiebe: u32 = 0;
        for i in 0..MAX_LEB_U64 {
            let b = self.u8("LEB128 u64")?;
            let nutzlast = (b & 0x7f) as u64;
            if i == MAX_LEB_U64 - 1 && (b & 0x7e) != 0 {
                return Err(Fehler::form(
                    ART_LEB,
                    format!("LEB128 u64: {b:#04x} am Byte {i} ragt ueber 64 Bit"),
                ));
            }
            ergebnis |= nutzlast << schiebe;
            if b & 0x80 == 0 {
                return Ok(ergebnis);
            }
            schiebe += 7;
        }
        Err(Fehler::form(
            ART_LEB,
            format!(
                "LEB128 u64 nicht beendet nach {MAX_LEB_U64} Byte (Pos {})",
                self.pos
            ),
        ))
    }

    /// LEB128 i32 (vorzeichenbehaftet, max. 5 Byte).
    ///
    /// `kanonisch` wird nicht mehr erzwungen: die Spezifikation erlaubt
    /// verlaengerte Kodierung (GEMESSEN 01.10., `binary-leb128.wast`,
    /// `\00\82\80\80\80\00` = 2). Der Parameter bleibt fuer die
    /// Aufrufstellen und ist als Absicht dokumentiert.
    pub fn leb_s32(&mut self, kanonisch: bool) -> Result<i32, Fehler> {
        let _ = kanonisch; // verlaengerte Kodierung ist erlaubt (Spec)
        let mut ergebnis: i64 = 0;
        let mut schiebe: u32 = 0;
        for i in 0..MAX_LEB_U32 {
            let b = self.u8("LEB128 i32")?;
            let nutzlast = (b & 0x7f) as i64;
            if i == MAX_LEB_U32 - 1 {
                // Fuenftes Byte: die Bits ueber Bit 31 muessen eine
                // korrekte Vorzeichenerweiterung sein — erlaubt sind nur
                // 0x00..0x07 (positiv) und 0x78..0x7f (negativ).
                // GEMESSEN 02.10. (`binary-leb128.wast` Z. 73/74:
                // „integer too large").
                let fenster = b & 0x78;
                if fenster != 0 && fenster != 0x78 {
                    return Err(Fehler::form(
                        ART_LEB,
                        format!("LEB128 i32: {b:#04x} am Byte {i} ragt ueber 32 Bit"),
                    ));
                }
            }
            ergebnis |= nutzlast << schiebe;
            schiebe += 7;
            if b & 0x80 == 0 {
                if schiebe < 64 && (b & 0x40) != 0 {
                    ergebnis |= -1i64 << schiebe;
                }
                return Ok(ergebnis as i32);
            }
        }
        Err(Fehler::form(
            ART_LEB,
            format!(
                "LEB128 i32 nicht beendet nach {MAX_LEB_U32} Byte (Pos {})",
                self.pos
            ),
        ))
    }

    /// LEB128 i64 (vorzeichenbehaftet, max. 10 Byte). Siehe `leb_s32`.
    pub fn leb_s64(&mut self, kanonisch: bool) -> Result<i64, Fehler> {
        let _ = kanonisch; // siehe leb_s32
        let mut ergebnis: i64 = 0;
        let mut schiebe: u32 = 0;
        for i in 0..MAX_LEB_U64 {
            let b = self.u8("LEB128 i64")?;
            let nutzlast = (b & 0x7f) as i64;
            if i == MAX_LEB_U64 - 1 {
                // Zehntes Byte: 0x00 (positiv) oder 0x7f (negativ), sonst
                // ragt die Zahl ueber 64 Bit (GEMESSEN 02.10.,
                // `binary-leb128.wast` Z. 78).
                if b != 0x00 && b != 0x7f {
                    return Err(Fehler::form(
                        ART_LEB,
                        format!("LEB128 i64: {b:#04x} am Byte {i} ragt ueber 64 Bit"),
                    ));
                }
            }
            ergebnis |= nutzlast.wrapping_shl(schiebe);
            schiebe += 7;
            if b & 0x80 == 0 {
                if schiebe < 64 && (b & 0x40) != 0 {
                    ergebnis |= (-1i64).wrapping_shl(schiebe);
                }
                return Ok(ergebnis);
            }
        }
        Err(Fehler::form(
            ART_LEB,
            format!(
                "LEB128 i64 nicht beendet nach {MAX_LEB_U64} Byte (Pos {})",
                self.pos
            ),
        ))
    }

    /// s33 (Blocktyp): negativ = Valtyp, >= 0 = Typindex. Siehe `leb_s32`.
    pub fn leb_s33(&mut self, kanonisch: bool) -> Result<i64, Fehler> {
        let _ = kanonisch; // siehe leb_s32
        let mut ergebnis: i64 = 0;
        let mut schiebe: u32 = 0;
        for _ in 0..MAX_LEB_S33 {
            let b = self.u8("LEB128 s33")?;
            ergebnis |= ((b & 0x7f) as i64).wrapping_shl(schiebe);
            schiebe += 7;
            if b & 0x80 == 0 {
                if schiebe < 64 && (b & 0x40) != 0 {
                    ergebnis |= (-1i64).wrapping_shl(schiebe);
                }
                return Ok(ergebnis);
            }
        }
        Err(Fehler::form(
            ART_LEB,
            format!(
                "LEB128 s33 nicht beendet nach {MAX_LEB_S33} Byte (Pos {})",
                self.pos
            ),
        ))
    }

    /// Name: Laenge (u32, kanonisch) + UTF-8-Bytes, Laenge hart begrenzt.
    pub fn name(&mut self) -> Result<String, Fehler> {
        let laenge = self.leb_u32(true)? as usize;
        if laenge > self.grenze_name {
            return Err(Fehler::form(
                ART_GRENZE,
                format!("Name: {laenge} Byte ueber der Grenze {}", self.grenze_name),
            ));
        }
        let roh = self.nimm(laenge, "Name")?;
        match std::str::from_utf8(roh) {
            Ok(s) => Ok(s.to_string()),
            Err(e) => Err(Fehler::form(
                ART_UTF8,
                format!(
                    "Name: ungueltiges UTF-8 bei {e} (Pos {})",
                    self.pos - laenge
                ),
            )),
        }
    }

    /// Laengenbyte eines Vektor: prueft die Grenze, BEVOR alloziert wird (M4).
    /// `was` nur fuer die Fehlermeldung, `grenze` aus `Grenzen`.
    pub fn vektor_len(&mut self, grenze: usize, was: &str) -> Result<usize, Fehler> {
        let n = self.leb_u32(true)? as usize;
        if n > grenze {
            return Err(Fehler::form(
                ART_GRENZE,
                format!("{was}: {n} Eintraege ueber der Grenze {grenze}"),
            ));
        }
        // Noch eine Sanity-Grenze: jeder Eintrag braucht mindestens 1 Byte.
        if n > self.uebrig() {
            return Err(Fehler::form(
                ART_LAENGE,
                format!(
                    "{was}: {n} Eintraege angekündigt, nur {} Byte Daten",
                    self.uebrig()
                ),
            ));
        }
        Ok(n)
    }
}

#[cfg(test)]
mod einheit {
    use super::*;
    use crate::fehler::ART_LEB;

    fn w(b: &[u8]) -> Leser<'_> {
        Leser::neu(b)
    }

    /// M7: Werte + Fehlerarten sichern.
    #[test]
    fn leb128_werte_und_fehler() {
        assert_eq!(w(&[0x00]).leb_u32(true).unwrap(), 0);
        assert_eq!(w(&[0xe5, 0x8e, 0x26]).leb_u32(true).unwrap(), 624_485);
        assert_eq!(
            w(&[0xff, 0xff, 0xff, 0xff, 0x0f]).leb_u32(true).unwrap(),
            u32::MAX
        );
        // 5 Fuellbytes: kein Ende -> benannter Fehler
        let e = w(&[0x80, 0x80, 0x80, 0x80, 0x80])
            .leb_u32(true)
            .unwrap_err();
        assert_eq!(e.art(), ART_LEB);
        // 12x0x80: mehr als die 10 erlaubten Byte -> benannter Fehler
        let e = w(&[0x80; 12]).leb_u64(true).unwrap_err();
        assert_eq!(e.art(), ART_LEB);
        // nach 8 Fuellbytes ist die Kette zu kurz -> ebenfalls benannt
        assert_eq!(
            w(&[0x80; 8]).leb_u64(true).unwrap_err().art(),
            ART_UNVOLLSTAENDIG
        );
        // Verlaengerte Kodierung ist erlaubt (Spec: `binary-leb128.wast`),
        // nur Bits oberhalb 32 werden abgelehnt.
        assert_eq!(w(&[0x80, 0x00]).leb_u32(true).unwrap(), 0);
        assert_eq!(w(&[0x82, 0x80, 0x80, 0x80, 0x00]).leb_u32(true).unwrap(), 2);
        // 6 Fuellbytes: ueber der Grenze
        assert_eq!(
            w(&[0x82, 0x80, 0x80, 0x80, 0x80, 0x00])
                .leb_u32(true)
                .unwrap_err()
                .art(),
            ART_LEB
        );
    }

    #[test]
    fn leb128_vorzeichen() {
        assert_eq!(w(&[0x7f]).leb_s32(true).unwrap(), -1);
        assert_eq!(w(&[0x40]).leb_s32(true).unwrap(), -64);
        assert_eq!(w(&[0xc0, 0x00]).leb_s32(true).unwrap(), 64);
        assert_eq!(
            w(&[0xff, 0xff, 0xff, 0xff, 0x7f]).leb_s32(true).unwrap(),
            -1
        );
        assert_eq!(
            w(&[0x80, 0x80, 0x80, 0x80, 0x78]).leb_s32(true).unwrap(),
            i32::MIN
        );
        // u32::MAX gehoert als ...ff 0f kodiert (binary-leb128.wast)
        assert_eq!(
            w(&[0xff, 0xff, 0xff, 0xff, 0x0f]).leb_u32(true).unwrap(),
            u32::MAX
        );
        // ...ff 4f ist „integer too large" (Bit 6 gesetzt)
        assert_eq!(
            w(&[0xff, 0xff, 0xff, 0xff, 0x4f])
                .leb_u32(true)
                .unwrap_err()
                .art(),
            ART_LEB
        );
    }

    #[test]
    fn name_utf8_und_grenzen() {
        let mut l = w(&[0x03, b'a', b'b', b'c']);
        assert_eq!(l.name().unwrap(), "abc");
        let mut l = w(&[0x01, 0xff]);
        assert_eq!(l.name().unwrap_err().art(), ART_UTF8);
        // Laenge 200 > Grenze 8
        let mut l = Leser::neu(&[0xc8, 0x01]).mit_namengrenze(8);
        assert_eq!(l.name().unwrap_err().art(), ART_GRENZE);
        let e = w(&[]).name().unwrap_err();
        assert_eq!(e.art(), ART_UNVOLLSTAENDIG);
    }

    #[test]
    fn vektor_len_prueft_vor_der_allokation() {
        // (a) Laenge ueber der Grenze -> ART_GRENZE (noch keine Allokation)
        let mut l = w(&[0xff, 0xff, 0xff, 0x0f, 1, 2]);
        assert_eq!(l.vektor_len(100, "Test").unwrap_err().art(), ART_GRENZE);
        // (b) Laenge passt zur Grenze, aber Daten fehlen -> ART_LAENGE
        let mut l = w(&[0x05, 1, 2]);
        assert_eq!(l.vektor_len(100, "Test").unwrap_err().art(), ART_LAENGE);
        // (c) 2 Eintraege mit je 1 Byte: passt
        let mut l = w(&[0x02, 7, 8]);
        assert_eq!(l.vektor_len(100, "Test").unwrap(), 2);
        assert_eq!(l.u8("x").unwrap(), 7);
    }
}
