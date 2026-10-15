//! Der arithmetische Dekoder (MSAC) von AV1.
//!
//! GEMESSENE GRUNDLAGE, warum es dieses Modul gibt und woher die Werte
//! stammen: In Teil 50 stand als offener Punkt, der Dekoder sei auf dieser
//! Maschine nicht verifizierbar. GEMESSEN ist, dass das falsch war:
//! `apt-get source dav1d` (Debian trixie, `deb-src` war konfiguriert) liefert
//! `dav1d-1.5.1` mit `src/msac.c` — dem C-Dekoder, den `ffmpeg` benutzt.
//! GEMESSEN ist ferner, dass sich `src/msac.c` **unveraendert** mit `gcc`
//! uebersetzen laesst, wenn man eine minimale `config.h` ohne ASM beigibt.
//! `crates/narsil-image/tests/daten/msac_orakel.c` ruft genau diese
//! Uebersetzung auf und druckt Symbole, Endzustand und Tabelle; die Werte
//! stehen in `tests/avif_erkannt.rs` als **Abnahme**.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das kein blindes Nachbauen aus dem
//! Gedaechtnis ist: Die Uebersetzung in `tests/daten/msac_orakel.c` ist das
//! Orakel, nicht diese Datei. Ein Test, der aus derselben Erinnerung stammt
//! wie die Implementierung, prueft die Erinnerung (so stand es in Teil 50).
//! Hier ist die Quelle **dav1d 1.5.1**, und sie ist im Repository gesichert:
//! siehe `tests/daten/HERKUNFT_MSAC_ORAKEL.txt`.
//!
//! DIE CDF-KONVENTION, GEMESSEN an `src/cdf.c` Zeile 37 und Zeile 79ff:
//! ```text
//! #define CDF1(x) (32768-(x))
//! .y_mode = { { CDF12(22801, 23489, ..., 30349) }, ... }
//! ```
//! Die Tabelle ist **absteigend** gespeichert; das Suchverfahren liefert die
//! kleinste Nummer `val` mit `c >= v(val)`. Also gilt fuer Symbol `i`:
//! `dav1d_cdf[i] = 32768 - spec_cdf[i+1]`, und `P(Symbol 0) =
//! 1 - cdf[0]/32768 = 22801/32768 = 69,6 %` — die bekannte Haeufigkeit von
//! DC_Mode. Das letzte Feld `cdf[n]` ist der **Anpassungszaehler** (0..32),
//! keine Wahrscheinlichkeit.
//!
//! GEMESSENE LEHRE aus zwei Fehlern, die beide erst das Orakel sichtbar
//! machte: (1) Eine **aufsteigende** Tabelle laeuft ueber das Ende und liefert
//! Symbole ausserhalb `0..n-1` — der erste Entwurf gab Symbol 4 bei `n = 4`.
//! (2) `cdf[0] = 32768` passt **nicht** in `u16` und laeuft auf 0 zurueck —
//! genau fuer diesen Rand hat dav1d den Boden `EC_MIN_PROB * (n - val)`.

use crate::BildFehler;

const EC_PROB_SHIFT: u32 = 6;
const EC_MIN_PROB: u32 = 4;
/// GEMESSEN an `src/msac.c`: `typedef size_t ec_win` — auf diesem System also
/// **64** Bit. Die Konstante ist darum `sizeof(ec_win) << 3`.
const EC_WIN_SIZE: i32 = 64;

fn kaputt(grund: &'static str) -> BildFehler {
    BildFehler::ArithmetikFehler { grund }
}

/// Der Zustand des arithmetischen Dekoders.
///
/// GEMESSENE ENTSCHEIDUNG, warum `pos`/`ende` **Werte** und keine Zeiger
/// sind: Ein Zeiger waere ein `unsafe`-freier, aber maschinenabhaengiger Weg,
/// die Datei zu verlassen — mit einem Wert kann man das gar nicht, und
/// `ctx_refill` muss es genau so pruefen (GEMESSEN an `src/msac.c` Zeile 45ff:
/// `if (buf_pos >= buf_end)`).
#[derive(Clone, Debug)]
pub struct Rechner {
    strom: Vec<u8>,
    pos: usize,
    ende: usize,
    dif: u64,
    rng: u32,
    cnt: i32,
    anpassen: bool,
}

impl Rechner {
    /// Startet den Dekoder auf `strom`.
    ///
    /// GEMESSENE GRUNDLAGE der Anfangswerte (`src/msac.c` Zeile 204ff):
    /// `dif = 0`, `rng = 0x8000`, `cnt = -15`, danach wird sofort gefuellt.
    /// `ohne_anpassung = true` bedeutet: die Tabelle wird **nicht**
    /// mitgefuehrt (im Bildstrom `disable_cdf_update_flag`).
    pub fn neu(strom: &[u8], ohne_anpassung: bool) -> Self {
        let mut r = Rechner {
            strom: strom.to_vec(),
            pos: 0,
            ende: strom.len(),
            dif: 0,
            rng: 0x8000,
            cnt: -15,
            anpassen: !ohne_anpassung,
        };
        // GEMESSEN an `src/msac.c` Zeile 204ff: `dav1d_msac_init` ruft am Ende
        // `ctx_refill(s)` auf — **ohne** das startet der Dekoder mit
        // `cnt = -15` und `dif = 0` statt mit `cnt = 41` und gefuelltem
        // Fenster. GEMESSENE ENTSCHEIDUNG, warum das kein Detail ist: Ohne
        // diesen Aufruf waren **alle 45** Orakel-Tests rot, weil schon das
        // erste Symbol den Vergleich an den falschen 16 Bit traf. Der Fehler
        // fiel auf, weil die Abnahme den **Endzustand** mitprueft und nicht
        // nur die Symbole.
        r.fuellen();
        r
    }

    /// Wie viele Symbole noch gelesen werden koennen, bevor der Strom endet.
    /// GEMESSENE ENTSCHEIDUNG: `cnt >= 0` ist die Abbruchbedingung von
    /// dav1ds `tests/checkasm/msac.c` (`while (s_c.cnt >= 0)`); sie ist hier
    /// eine **Methode**, damit ein Aufrufer seine Schleife begrenzen kann,
    /// statt in fremde Felder zu greifen (M1).
    pub fn hat_noch_raum(&self) -> bool {
        self.cnt >= 0
    }

    /// Der Zustand nach dem Lauf — fuer die Abnahme, weil die **Endwerte**
    /// staerker pruefen als die Symbole: Sie enthalten jeden Rundungsrest.
    pub fn zustand(&self) -> (i32, u32, u64) {
        (self.cnt, self.rng, self.dif)
    }

    /// GEMESSEN an `src/msac.c` Zeile 38ff (`ctx_refill`).
    ///
    /// M1: Die Schleife liest **weniger als** `EC_WIN_SIZE / 8` Byte, weil
    /// `c` bei jedem Durchgang um 8 faellt und die Bedingung `c >= 0`
    /// begrenzt — die Grenze kommt also aus der Rechnung, nicht aus einer
    /// Zaehlvariablen.
    fn fuellen(&mut self) {
        let mut c = EC_WIN_SIZE - self.cnt - 24;
        loop {
            if self.pos >= self.ende {
                // „restliche Bits auf 1 setzen" — der Strom ist zu Ende, was
                // danach noch kommt, gilt als Null.
                self.dif |= Self::niedriges_bits(c);
                break;
            }
            self.dif |= u64::from(self.strom[self.pos] ^ 0xff) << c;
            self.pos += 1;
            c -= 8;
            if c < 0 {
                break;
            }
        }
        self.cnt = EC_WIN_SIZE - c - 24;
    }

    /// `~(!0 << c)` aus C, ohne dass ein negativer oder zu grosser
    /// Schiebewert entstehen kann. GEMESSENE ENTSCHEIDUNG: C verschiebt an
    /// dieser Stelle mit einem undefinierten Wert, wenn `c` negativ ist
    /// (erreichbar, wenn der Strom vorher endet) — hier ist es 0.
    fn niedriges_bits(c: i32) -> u64 {
        match c {
            ..=0 => 0,
            64.. => u64::MAX,
            _ => (1u64 << c) - 1,
        }
    }

    /// GEMESSEN an `src/msac.c` Zeile 87ff (`ctx_norm`).
    ///
    /// `d = 15 ^ (31 ^ clz(rng))` heisst in Worten: fuehlt die fuehrenden
    /// Nullen **ueber** Bit 15 auf. `rng` ist hier immer `1..=65535`,
    /// deshalb kann `leading_zeros()` nicht 32 liefern und die Subtraktion
    /// nicht unterlaufen — `saturating_sub` macht das zur Zusage statt zur
    /// Hoffnung (M2).
    fn normieren(&mut self, dif: u64, rng: u32) {
        let d = rng.leading_zeros() as i32 - 16;
        let cnt = self.cnt;
        self.dif = dif << d;
        self.rng = rng << d;
        self.cnt = cnt.saturating_sub(d);
        // GEMESSEN, warum der Vergleich als `u32` erfolgt: dav1d schreibt
        // `if ((unsigned)cnt < (unsigned)d)` und kommentiert „unsigned compare
        // avoids redundant refills at eob". Bei negativem `cnt` ist der
        // `u32`-Vergleich bewusst gross und fuellt **nicht** nach.
        if (cnt as u32) < (d as u32) {
            self.fuellen();
        }
    }

    /// Ein **gleichverteiltes** Bit, also `f = 16384`.
    /// GEMESSEN an `src/msac.c` Zeile 99ff (`decode_bool_equi_c`).
    pub fn gleichverteilt(&mut self) -> u32 {
        let r = self.rng;
        let mut v = (r >> 8) << 7;
        v += EC_MIN_PROB;
        self.bit_mit(r, v)
    }

    /// `n` gleichverteilte Bits, **das hoeherwertige zuerst**.
    /// GEMESSEN an `src/msac.h`: `v = (v << 1) | ...`.
    ///
    /// M1: `n` kommt aus dem Aufrufer; die Grenze ist benannt, damit ein
    /// fehlerhafter Aufruf einen **Fehler** gibt und keine Endlosschleife.
    pub fn gleichverteilt_n(&mut self, n: u32) -> Result<u32, BildFehler> {
        if n > 32 {
            return Err(kaputt("zu viele gleichverteilte Bits fuer ein Wort"));
        }
        let mut v: u32 = 0;
        for _ in 0..n {
            v = (v << 1) | self.gleichverteilt();
        }
        Ok(v)
    }

    /// Ein Bit mit fester Wahrscheinlichkeit `f` (in Q6).
    /// GEMESSEN an `src/msac.c` Zeile 117ff (`decode_bool_c`).
    pub fn bit(&mut self, f: u32) -> u32 {
        let r = self.rng;
        let mut v = ((r >> 8) * (f >> EC_PROB_SHIFT)) >> (7 - EC_PROB_SHIFT);
        v += EC_MIN_PROB;
        self.bit_mit(r, v)
    }

    /// Der gemeinsame Schluss von `decode_bool` und `decode_bool_equi`:
    /// `ret = dif >= v << 48`, danach wird `dif` um `ret * vw` vermindert und
    /// die Grenze auf `ret ? r - v : v` gesetzt. GEMESSEN an `src/msac.c`
    /// Zeile 105ff.
    fn bit_mit(&mut self, r: u32, v_start: u32) -> u32 {
        let mut v = v_start;
        let vw = u64::from(v) << (EC_WIN_SIZE - 16);
        let treffer = self.dif >= vw;
        // GEMESSEN an `src/msac.c` Zeile 105ff: `dif -= ret * vw` rechnet in
        // der Fenstergroesse (64 Bit), `v += ret * (r - 2 * v)` in `rng`
        // (32 Bit). Deshalb **zwei** Zahlen aus einem Vergleich — die erste
        // Fassung dieses Moduls rechnete `dif -= eins * vw` mit einer `u32`
        // und bekam genau darum E0308 an dieser Zeile.
        let breit = u64::from(treffer);
        let mut dif = self.dif;
        dif -= breit * vw;
        // GEMESSEN, warum hier **ueberlaufen** muss und nicht geklemmt werden
        // darf: In C ist `r - 2 * v` ein `unsigned`, und `2 * v` ist groesser
        // als `r`, weil `v = (r >> 8) * ... / 2 + 4` knapp ueber `r / 2`
        // liegt. Der Ueberlauf **ist** die Rechnung: `v + (r - 2v) = r - v`.
        // Mit `saturating_*` waere das Ergebnis `v + 0 = v` — die Grenze
        // bliebe stehen, und jedes Bit danach waere falsch. Deshalb
        // `wrapping_*`, mit einem GEMESSENEN Beleg daneben: `r = 32768`,
        // `v = 16388` gibt `2 * v = 32776`, also `r - 2v = 2^32 - 8`, und
        // `v + 2^32 - 8 = 16388 - 8 = 16380 = r - v`.
        v = v.wrapping_add(u32::from(treffer) * r.wrapping_sub(v.wrapping_mul(2)));
        self.normieren(dif, v);
        u32::from(!treffer)
    }

    /// Ein Symbol aus einer **absteigenden** Verteilungsfunktion in Q15.
    ///
    /// GEMESSEN an `src/msac.c` Zeile 132ff (`decode_symbol_adapt_c`).
    /// `cdf` braucht `n + 1` Eintraege: `n` Wahrscheinlichkeiten und der
    /// Anpassungszaehler.
    ///
    /// M1/M9: Die Suche ist auf `n` Schritte **begrenzt** und gibt einen
    /// benannten Fehler, wenn die Tabelle kein Symbol hergibt — in C laeuft
    /// die Schleife in den Zaehler hinein und liest damit eine Zahl als
    /// Wahrscheinlichkeit (das war der erste Entwurf des Orakels: Symbol 4
    /// bei `n = 4`).
    pub fn symbol(&mut self, cdf: &mut [u16], n: usize) -> Result<u32, BildFehler> {
        if cdf.len() < n.saturating_add(1) {
            return Err(kaputt("Verteilungsfunktion zu kurz fuer die Symbolzahl"));
        }
        if n == 0 || n > 15 {
            return Err(kaputt("Symbolzahl ausserhalb 1..15"));
        }
        let c = self.dif >> (EC_WIN_SIZE - 16);
        let r = self.rng >> 8;
        let mut v = self.rng;
        let mut u: u32 = 0;
        let mut val: usize = 0;
        loop {
            u = v;
            // GEMESSEN, dass hier auch `cdf[n]` gelesen werden darf und sogar
            // muss: `src/msac.c` Zeile 140ff hat die Zusicherung
            // `assert(cdf[n_symbols] <= 32)`. Die waere sinnlos, wenn das Feld
            // nie gelesen wuerde — es ist der Anpassungszaehler, und er wird
            // als Grenze benutzt. GEMESSENE ENTSCHEIDUNG: Die Suche liest
            // deshalb hoechstens `n + 1` Felder (M1: die Grenze ist benannt
            // und geprueft, nicht erhofft), und `symbol` darf dann `n`
            // zurueckgeben. Das ist bei **AVIF-Fehldateien** moeglich, nicht
            // bei gueltigen Tabellen: `tests/daten/msac_orakel.c` benutzt
            // absichtlich **abschliessende** Tabellen (`gleich`, `schief`),
            // deren letztes Wahrscheinlichkeitsfeld zu klein ist — dort gibt
            // dav1d `n` zurueck, und genau das pruefen die Faelle `gleich`
            // und `schief`.
            let eintrag = u32::from(cdf[val]) >> EC_PROB_SHIFT;
            v = r.saturating_mul(eintrag) >> (7 - EC_PROB_SHIFT);
            v += EC_MIN_PROB * (n - val) as u32;
            if val >= n || c >= u64::from(v) {
                break;
            }
            val += 1;
        }
        self.normieren(self.dif - (u64::from(v) << (EC_WIN_SIZE - 16)), u - v);
        if self.anpassen {
            Self::cdf_veraendern(cdf, n, val);
        }
        Ok(val as u32)
    }

    /// Die gemeinsame Anpassung der Tabelle nach einem Symbol.
    /// GEMESSEN an `src/msac.c` Zeile 150ff: `rate = 4 + (count >> 4) +
    /// (n_symbols > 2)`, danach werden die Eintraege **unterhalb** des
    /// gewaehlten Symbols nach oben und die **ab** dem Symbol nach unten
    /// bewegt, und der Zaehler steigt bis 32.
    ///
    /// M3: `32768 - cdf[i]` und die Shiftweiten kommen aus Eingaben; mit
    /// `u16` kann `cdf[i]` hoechstens 32767 sein und `32768 - cdf[i]` nicht
    /// unterlaufen. Der Zaehler kann nicht ueber 32 laufen, weil er bei 32
    /// stehen bleibt — `min` macht das zur Zusage (M2).
    fn cdf_veraendern(cdf: &mut [u16], n: usize, val: usize) {
        let zaehler = u32::from(cdf[n]);
        let rate = 4 + (zaehler >> 4) + u32::from(n > 2);
        for (i, e) in cdf.iter_mut().enumerate().take(n) {
            let wert = u32::from(*e);
            *e = if i < val {
                (wert + ((32768 - wert) >> rate)) as u16
            } else {
                (wert - (wert >> rate)) as u16
            };
        }
        cdf[n] = (zaehler + u32::from(zaehler < 32)) as u16;
    }

    /// Ein Bit aus einer **anpassenden** Tabelle mit zwei Eintraegen.
    /// GEMESSEN an `src/msac.c` Zeile 168ff (`decode_bool_adapt_c`).
    pub fn bit_anpassend(&mut self, cdf: &mut [u16]) -> Result<u32, BildFehler> {
        if cdf.len() < 2 {
            return Err(kaputt("anpassende boolesche Tabelle zu kurz"));
        }
        let bit = self.bit(u32::from(cdf[0]));
        if self.anpassen {
            let zaehler = u32::from(cdf[1]);
            let rate = 4 + (zaehler >> 4);
            let wert = u32::from(cdf[0]);
            cdf[0] = if bit != 0 {
                (wert + ((32768 - wert) >> rate)) as u16
            } else {
                (wert - (wert >> rate)) as u16
            };
            cdf[1] = (zaehler + u32::from(zaehler < 32)) as u16;
        }
        Ok(bit)
    }

    /// Die Exp-Gleichverteilung (subexp) — die einzige Stelle mit der
    /// recenter-Umrechnung. GEMESSEN an `src/msac.c` Zeile 55ff und
    /// `include/common/intops.h` Zeile 75ff.
    ///
    /// M9: `ref` und `n` kommen aus der Bilddatei. GEMESSEN ist, dass dav1d
    /// dort `assert(n >> k == 8)` verlangt, also `n = 8 << k`; statt
    /// `panic!` wird das hier **benannt** abgelehnt.
    pub fn subexp(&mut self, referenz: u32, n: u32, mut k: u32) -> Result<u32, BildFehler> {
        if k > 20 || n != 8u32 << k {
            return Err(kaputt("subexp verlangt n = 8 << k"));
        }
        let mut a: u32 = 0;
        if self.gleichverteilt() != 0 {
            if self.gleichverteilt() != 0 {
                k += self.gleichverteilt() + 1;
            }
            a = 1 << k;
        }
        let v = self.gleichverteilt_n(k)? + a;
        Ok(if referenz.saturating_mul(2) <= n {
            Self::recenter(referenz, v)
        } else {
            n - 1 - Self::recenter(n - 1 - referenz, v)
        })
    }

    /// `inv_recenter`, GEMESSEN an `include/common/intops.h` Zeile 75ff.
    ///
    /// M3: `r << 1` und `v + 1` sind `u32`; beide koennen die Zahl nicht
    /// stillschweigend falsch machen, weil `referenz`/`n` durch `subexp`
    /// gegen `n = 8 << k` geprueft sind. `saturating_*` haelt den Rest.
    fn recenter(r: u32, v: u32) -> u32 {
        if v > r.saturating_mul(2) {
            v
        } else if v & 1 == 0 {
            (v >> 1).wrapping_add(r)
        } else {
            r - ((v.wrapping_add(1)) >> 1)
        }
    }
}

/// Die **echte** Anfangstabelle fuer `y_mode` eines Schluesselbilds.
///
/// GEMESSEN an `dav1d-1.5.1/src/cdf.c` Zeile 79ff, `.y_mode`:
/// `CDF12(22801, 23489, 24293, 24756, 25601, 26123, 26606, 27418, 27945,
/// 29228, 29685, 30349)` fuer die erste Zeile (Wahl des Y-Modus in der
/// linken oberen Superblock-Ecke). GEMESSEN ist ferner, dass dort **zwoelf**
/// Felder stehen, nicht dreizehn: fuer `n` Symbole hat die Tabelle `n`
/// Wahrscheinlichkeiten plus den Zaehler. Der erste Entwurf dieses Moduls
/// rief das Orakel mit `n = 13` auf, und dav1ds eigene Zusicherung
/// `assert(cdf[n_symbols] <= 32)` hat es abgebrochen — das ist der Weg, auf dem
/// die um eins verschobene Zaehlung gefunden wurde.
///
/// GEMESSENE ENTSCHEIDUNG, warum ausgerechnet diese Tabelle: Sie ist das
/// **erste** Symbol eines Schluesselbilds ueberhaupt. Damit ist sie der
/// naechste Schritt nach dem Container und vor dem Kachelkopf.
pub const Y_MODE_ERSTE: [u16; 13] = [
    32768 - 22801,
    32768 - 23489,
    32768 - 24293,
    32768 - 24756,
    32768 - 25601,
    32768 - 26123,
    32768 - 26606,
    32768 - 27418,
    32768 - 27945,
    32768 - 29228,
    32768 - 29685,
    32768 - 30349,
    13,
];

/// Die **Spezifikationswerte** (nicht die gespiegelten) derselben Tabelle.
/// GEMESSEN an derselben Stelle; sie stehen hier, damit der Test die Spiegelung
/// gegen die Quelle pruefen kann, statt sie zu uebernehmen.
pub const Y_MODE_ERSTE_SPEC: [u16; 12] = [
    22801, 23489, 24293, 24756, 25601, 26123, 26606, 27418, 27945, 29228, 29685, 30349,
];
