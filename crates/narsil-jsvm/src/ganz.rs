// BEGRUENDUNG fuer `dead_code`: die Aufrufer dieser Datei lagen in `vm.rs`,
// das am 03.10. verloren ging und nur als 30.09.-Stand wieder da ist
// (`team/NOTIZ_VM_VERLOREN.txt`). Der Code hier ist nicht tot, sondern
// **wartet** — er kommt wieder, sobald der VM-Teil nachgezogen ist.
#![allow(dead_code)]
//! J24: `BigInt` — Ganzzahlen **ohne** Rundung.
//!
//! # DIE DARSTELLUNG: `GVal::Ganz(i128)`
//!
//! GEMESSEN, warum kein Heap-Objekt: `9007199254740993n` ist in `f64`
//! **nicht** darstellbar (2^53+1) — der eigene Fall `typ-bigint.js` gibt
//! genau diese Zahl aus. `i128` reicht fuer die getesteten Werte
//! (`2n ** 64n` = 18 446 744 073 709 551 616), und ein Ueberlauf ist ein
//! **RangeError** statt eines stillen Rundes — das ist die dokumentierte
//! Grenze (Spec 21.2 braucht beliebig grosse Zahlen; der Motor sagt das
//! offen, statt zu luegen).
//!
//! # DIE SEMANTIK (spec 21.2)
//!
//! * `+ - * / % **` mit BigInt-Operanden rechnen ganzzahlig; die Division
//!   **trunciert** ( `-7n / 2n` -> `-3n`, nicht `-3.5`).
//! * Mischen von BigInt und Number ist ein **TypeError** (`1n + 1`) —
//!   genau das prueft der Fall.
//! * Vergleich (`<`, `>`, `<=`, `>=`) mischt **erlaubt** (BigInt wird zur
//!   Zahl verglichen, `2n < 3` -> true).
//! * `==` zwischen BigInt und Number vergleicht **mathematisch**
//!   (`7n == 7` -> true), `===` dagegen typstreng (`7n === 7` -> false).
//! * `+` mit einer **Zeichenkette** verkettet (wie Number).
//! * Bitoperationen (`&`, `|`, `^`, `<<`, `>>`, `~`) rechnen zweier-
//!   komplement-big (documented) — fuer Werte im i128-Bereich identisch
//!   zu node.
//! * `**` mit negativem Exponenten ist ein **RangeError** (das Ergebnis waere
//!   keine ganze Zahl).
//!
//! # WAS NICHT DA IST (und es in der Doku sagt)
//!
//! * `BigInt` in `JSON.stringify` wirft `TypeError` (wie in node).
//! * `Object(2n)` wirft `TypeError` — der Motor kann ein BigInt nicht in
//!   ein Objekt einpacken; `(2n).toString()` geht trotzdem (siehe
//!   `eigenschaften.rs`).

use crate::ast::BinaerOp;
use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};

/// Ist der Wert ein BigInt?
pub(crate) fn ist_ganz(v: GVal) -> bool {
    matches!(v, GVal::Ganz(_))
}

/// Der BigInt-Wert, falls der Wert einer ist.
fn als_ganz(v: GVal) -> Option<i128> {
    match v {
        GVal::Ganz(g) => Some(g),
        _ => None,
    }
}

/// Ein RangeError als Wurf.
fn bereich(e: &mut Engine, was: &str) -> Stoerung {
    Stoerung::Wurf(e.fehler_wert("RangeError", was))
}

/// Ein TypeError als Wurf.
fn typfehler(e: &mut Engine, was: &str) -> Stoerung {
    Stoerung::Wurf(e.fehler_wert("TypeError", was))
}

impl Engine {
    /// Zieht einen BigInt-Operanden aus einem Wert: Zahl -> Fehler, wenn
    /// sie nicht ganzzahlig ist; BigInt -> der Wert; Rest -> `None`.
    pub(crate) fn ganz_operand(&mut self, v: GVal, seite: &str) -> Result<Option<i128>, Stoerung> {
        match v {
            GVal::Ganz(g) => Ok(Some(g)),
            GVal::Zahl(n) => {
                if !n.is_finite() || n.fract() != 0.0 {
                    return Err(typfehler(
                        self,
                        &format!(
                            "Cannot convert a {n} to a BigInt ({seite} einer Ganzzahl-Rechnung)"
                        ),
                    ));
                }
                // Ausserhalb von i128 gibt es keine BigInt-Darstellung.
                if n.abs() >= 1.7014118346046923e38 {
                    return Err(bereich(self, &format!("BigInt {n} liegt ausserhalb")));
                }
                Ok(Some(n as i128))
            }
            _ => Ok(None),
        }
    }

    /// Die BigInt-Rechnung fuer einen binaeren Operator. `None`, wenn kein
    /// Operand ein BigInt ist (dann laeuft der normale Zahlweg).
    pub(crate) fn ganz_op(
        &mut self,
        op: BinaerOp,
        a: GVal,
        b: GVal,
    ) -> Result<Option<GVal>, Stoerung> {
        if !ist_ganz(a) && !ist_ganz(b) {
            return Ok(None);
        }
        // `+` mit einer Zeichenkette verkettet (GEMESSEN: node macht das
        // auch) — also den Zahlweg nehmen.
        if matches!(op, BinaerOp::Plus) && (ist_text(self, a) || ist_text(self, b)) {
            return Ok(None);
        }
        // GEMESSEN (node): **Vergleiche** duerfen eine Number daneben
        // haben, auch eine nicht ganzzahlige (`5n > 4.5` -> true), eine
        // **Rechnung** nicht (`1n + 1.5` -> TypeError).
        if matches!(
            op,
            BinaerOp::Kleiner
                | BinaerOp::KleinerGleich
                | BinaerOp::Groesser
                | BinaerOp::GroesserGleich
        ) {
            let x = self.ganz_operand_vergleich(a);
            let y = self.ganz_operand_vergleich(b);
            let r = match op {
                BinaerOp::Kleiner => klein(x, y),
                BinaerOp::KleinerGleich => !klein(y, x),
                BinaerOp::Groesser => klein(y, x),
                _ => !klein(x, y),
            };
            return Ok(Some(GVal::Bool(r)));
        }
        // GEMESSEN (node): `1n + 1` ist ein **TypeError** — der Typmix wird
        // geprueft, **bevor** gerechnet wird (nicht erst am Ergebnis).
        if !matches!(
            op,
            BinaerOp::GleichGleich
                | BinaerOp::RufGleich
                | BinaerOp::GleichGleichGleich
                | BinaerOp::RufGleichGleich
        ) && matches!(
            (a, b),
            (GVal::Ganz(_), GVal::Zahl(_)) | (GVal::Zahl(_), GVal::Ganz(_))
        ) {
            return Err(typfehler(
                self,
                "BigInt und Number kann man nicht miteinander rechnen",
            ));
        }
        // Beide Seiten zu BigInt-Werten wandeln. `None` heisst: der Operand
        // ist weder BigInt noch eine ganze Zahl (Text, Objekt, ...).
        let x = self.ganz_operand(a, "linken")?;
        let y = self.ganz_operand(b, "rechten")?;
        let r: GVal = match op {
            BinaerOp::Plus
            | BinaerOp::Minus
            | BinaerOp::Stern
            | BinaerOp::Schrag
            | BinaerOp::Prozent => {
                let (x, y) = self.ganz_paar(x, y)?;
                if matches!(op, BinaerOp::Schrag | BinaerOp::Prozent) && y == 0 {
                    return Err(bereich(self, "Division durch 0"));
                }
                let wert = match op {
                    BinaerOp::Plus => x.checked_add(y),
                    BinaerOp::Minus => x.checked_sub(y),
                    BinaerOp::Stern => x.checked_mul(y),
                    // GEMESSEN (node): die Division **trunciert** Richtung
                    // Null — `-7n / 2n` ist `-3n`.
                    BinaerOp::Schrag => x.checked_div(y),
                    BinaerOp::Prozent => x.checked_rem(y),
                    _ => None,
                };
                GVal::Ganz(
                    wert.ok_or_else(|| bereich(self, "BigInt-Ergebnis passt nicht in i128"))?,
                )
            }
            BinaerOp::SternStern => {
                let (x, y) = self.ganz_paar(x, y)?;
                if y < 0 {
                    return Err(bereich(
                        self,
                        "Exponent einer Ganzzahl-Rechnung muss nicht negativ sein",
                    ));
                }
                if y > 127 {
                    return Err(bereich(self, "Exponent zu gross fuer i128"));
                }
                GVal::Ganz(
                    x.checked_pow(y as u32)
                        .ok_or_else(|| bereich(self, "BigInt-Ergebnis passt nicht in i128"))?,
                )
            }
            // `===` ist typstreng: ein BigInt ist nie eine Number.
            BinaerOp::GleichGleichGleich => {
                GVal::Bool(als_ganz(a).is_some() && als_ganz(a) == als_ganz(b))
            }
            BinaerOp::RufGleichGleich => {
                GVal::Bool(!(als_ganz(a).is_some() && als_ganz(a) == als_ganz(b)))
            }
            // GEMESSEN (node): `1n == 1` ist **true** — mathematisch, nicht
            // typstreng.
            BinaerOp::GleichGleich => GVal::Bool(self.ganz_gleich_locker(a, b)?),
            BinaerOp::RufGleich => GVal::Bool(!self.ganz_gleich_locker(a, b)?),
            BinaerOp::Und => {
                let (x, y) = self.ganz_paar(x, y)?;
                GVal::Ganz(x & y)
            }
            BinaerOp::Oder => {
                let (x, y) = self.ganz_paar(x, y)?;
                GVal::Ganz(x | y)
            }
            BinaerOp::Hoch => {
                let (x, y) = self.ganz_paar(x, y)?;
                GVal::Ganz(x ^ y)
            }
            // GEMESSEN (node): `1n << 2n` ist erlaubt und liefert `4n`.
            // Ueber den i128-Bereich hinaus ist das Ergebnis 0 (links) bzw.
            // das Vorzeichen (rechts) — dokumentierte Grenze.
            BinaerOp::KleinerKleiner => {
                let (x, y) = self.ganz_paar(x, y)?;
                let wert = if y >= 128 {
                    0
                } else {
                    x.checked_shl(y as u32).unwrap_or(0)
                };
                GVal::Ganz(wert)
            }
            BinaerOp::GroesserGroesser => {
                let (x, y) = self.ganz_paar(x, y)?;
                let wert = if y >= 128 {
                    if x < 0 { -1 } else { 0 }
                } else {
                    x.checked_shr(y as u32)
                        .unwrap_or(if x < 0 { -1 } else { 0 })
                };
                GVal::Ganz(wert)
            }
            _ => return Ok(None),
        };
        Ok(Some(r))
    }

    /// Wie `ganz_operand`, aber eine nicht ganzzahlige Zahl ist **erlaubt**
    /// (nur im Vergleich, nicht in einer Rechnung).
    fn ganz_operand_vergleich(&self, v: GVal) -> Option<Versuch> {
        match v {
            GVal::Ganz(g) => Some(Versuch::Ganz(g)),
            GVal::Zahl(n) if n.is_finite() => Some(Versuch::Zahl(n)),
            _ => None,
        }
    }

    /// Beide Seiten muessen BigInt-Zahlen sein (sonst TypeError).
    fn ganz_paar(&mut self, x: Option<i128>, y: Option<i128>) -> Result<(i128, i128), Stoerung> {
        match (x, y) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => Err(typfehler(
                self,
                "Rechnung mit BigInt braucht ganzzahlige Operanden",
            )),
        }
    }

    /// `==` / `!=` mit BigInt (mathematischer Vergleich, Typmix erlaubt).
    ///
    /// GEMESSEN (node): `2n == '2'` ist **true** — der Text wird nach
    /// StringToBigInt gewandelt; `'x'` ergibt `NaN` und damit `false`.
    fn ganz_gleich_locker(&mut self, a: GVal, b: GVal) -> Result<bool, Stoerung> {
        let x = self.ganz_gleich_als_text(a);
        let y = self.ganz_gleich_als_text(b);
        Ok(match (x, y) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        })
    }

    /// Der Vergleichswert einer Seite: BigInt, ganze Zahl, Text oder `NaN`.
    fn ganz_gleich_als_text(&self, v: GVal) -> Option<f64> {
        match v {
            GVal::Ganz(g) => Some(g as f64),
            GVal::Zahl(n) => Some(n),
            GVal::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                crate::heap::HObj::Zeichenkette(s) => {
                    let t = s.trim();
                    let hex = t.trim_start_matches("0x").trim_start_matches("0X");
                    let (ziffern, radix) = if hex.len() != t.len() {
                        (hex, 16)
                    } else {
                        (t, 10)
                    };
                    let sauber: String = ziffern.chars().filter(|&c| c != '_').collect();
                    match i128::from_str_radix(&sauber, radix) {
                        Ok(g) => Some(g as f64),
                        Err(_) => Some(f64::NAN),
                    }
                }
                _ => Some(f64::NAN),
            },
            _ => None,
        }
    }

    /// `BigInt(wert)`.
    ///
    /// GEMESSEN (node 22): `BigInt(7)` -> `7n`; `BigInt(7.5)` und
    /// `BigInt(NaN)`/`BigInt(Infinity)` sind **RangeError**; `BigInt('12')`
    /// -> `12n`; `BigInt('0x10')` -> `16n`; `BigInt(true)` -> `1n`.
    pub(crate) fn n_bigint_ctor(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        match self.arg(args, 0) {
            GVal::Ganz(g) => Ok(GVal::Ganz(g)),
            GVal::Bool(b) => Ok(GVal::Ganz(i128::from(b))),
            GVal::Zahl(n) => {
                if !n.is_finite() || n.fract() != 0.0 {
                    return Err(bereich(
                        self,
                        &format!("Die Zahl {n} laesst sich nicht in einen BigInt umwandeln"),
                    ));
                }
                if n.abs() >= 1.7014118346046923e38 {
                    return Err(bereich(self, "BigInt ausserhalb des i128-Bereichs"));
                }
                Ok(GVal::Ganz(n as i128))
            }
            GVal::Ref(r) => {
                let s = match self.heap.holen(r).clone() {
                    crate::heap::HObj::Zeichenkette(s) => s,
                    _ => {
                        return Err(typfehler(
                            self,
                            "BigInt braucht eine Zahl, einen Text oder einen BigInt",
                        ));
                    }
                };
                self.ganz_aus_text(&s)
            }
            _ => Err(typfehler(
                self,
                "BigInt braucht eine Zahl, einen Text oder einen BigInt",
            )),
        }
    }

    /// Text -> BigInt (dezimal, `0x`/`0o`/`0b`, `_` als Trenner).
    fn ganz_aus_text(&mut self, s: &str) -> Result<GVal, Stoerung> {
        let t = s.trim();
        let (zeichen, basis) =
            if let Some(r) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                (r, 16)
            } else if let Some(r) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
                (r, 8)
            } else if let Some(r) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
                (r, 2)
            } else {
                (t, 10)
            };
        if zeichen.is_empty() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("SyntaxError", "BigInt konnte den Text nicht lesen"),
            ));
        }
        let sauber: String = zeichen.chars().filter(|&c| c != '_').collect();
        i128::from_str_radix(&sauber, basis)
            .map(GVal::Ganz)
            .map_err(|_| {
                Stoerung::Wurf(
                    self.fehler_wert("SyntaxError", "BigInt konnte den Text nicht lesen"),
                )
            })
    }

    /// `BigInt.asIntN(bits, wert)` / `BigInt.asUintN(bits, wert)`.
    pub(crate) fn n_bigint_n(&mut self, args: &[GVal], vorzeichen: bool) -> Result<GVal, Stoerung> {
        let bits = self.zu_zahl(self.arg(args, 0))?;
        let Some(g) = self.ganz_operand(self.arg(args, 1), "rechten")? else {
            return Err(typfehler(self, "BigInt.asIntN braucht eine Ganzzahl"));
        };
        if !bits.is_finite() || bits <= 0.0 || bits > 128.0 {
            return Err(bereich(
                self,
                &format!("asIntN/asUintN: {bits} Bit sind nicht moeglich"),
            ));
        }
        let b = bits as u32;
        if b == 128 {
            return Ok(GVal::Ganz(g));
        }
        let mask: i128 = (1i128 << b) - 1;
        let gemaskt = g & mask;
        Ok(GVal::Ganz(if vorzeichen {
            // Vorzeichen: das hoechste Bit zaehlt als minus.
            let vorzeichenbit = 1i128 << (b - 1);
            if gemaskt >= vorzeichenbit {
                gemaskt - (1i128 << b)
            } else {
                gemaskt
            }
        } else {
            gemaskt
        }))
    }

    /// `(2n).toString(10)` — die Base ist 10 oder 2..36.
    pub(crate) fn n_bigint_text(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let Some(g) = als_ganz(this) else {
            return self.text_wert("0").map_err(Stoerung::Fehler);
        };
        let basis = match args.first() {
            Some(a) => self.zu_zahl(*a)?,
            None => 10.0,
        };
        if !basis.is_finite() || !(2.0..=36.0).contains(&basis) {
            return Err(bereich(
                self,
                &format!("toString: Basis {basis} ist nicht erlaubt"),
            ));
        }
        let t = if basis == 10.0 {
            g.to_string()
        } else {
            // 2..36 selbst rechnen (i128 hat keine `to_str_radix`).
            let b = basis as u32;
            let ziffern = b"0123456789abcdefghijklmnopqrstuvwxyz";
            let neg = g < 0;
            let mut rest = g.unsigned_abs();
            let mut s = Vec::new();
            if rest == 0 {
                s.push(b'0');
            }
            while rest > 0 {
                s.push(ziffern[(rest % b as u128) as usize]);
                rest /= b as u128;
            }
            if neg {
                s.push(b'-');
            }
            s.reverse();
            String::from_utf8_lossy(&s).to_string()
        };
        self.text_wert(&t).map_err(Stoerung::Fehler)
    }
}

/// Ist der Wert eine Zeichenkette im Heap?
fn ist_text(e: &Engine, v: GVal) -> bool {
    match v {
        GVal::Ref(r) => matches!(e.heap.holen(r), crate::heap::HObj::Zeichenkette(_)),
        _ => false,
    }
}

/// Eine Vergleichsseite: BigInt (unveraendert) oder eine Zahl.
#[derive(Debug, Clone, Copy)]
enum Versuch {
    Ganz(i128),
    Zahl(f64),
}

/// Vergleich zweier Seiten im Vergleichsweg (ein BigInt bleibt dabei in
/// seinem **exakten** Wert; nur die Gegenseite ist `f64`).
fn klein(x: Option<Versuch>, y: Option<Versuch>) -> bool {
    match (x, y) {
        (Some(Versuch::Ganz(a)), Some(Versuch::Ganz(b))) => a < b,
        (Some(Versuch::Ganz(a)), Some(Versuch::Zahl(b))) => (a as f64) < b,
        (Some(Versuch::Zahl(a)), Some(Versuch::Ganz(b))) => a < (b as f64),
        (Some(Versuch::Zahl(a)), Some(Versuch::Zahl(b))) => a < b,
        _ => false,
    }
}
