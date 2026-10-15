//! T11 — Grosse Zahlen und Modulararithmetik fuer ECDSA P-256 und RSA.
//!
//! Warum selbst: eine Signaturpruefung braucht Modulo-Potenzierung. Ohne
//! Fremd-Crate heisst das: Multiplikation, Division und Reduktion auf
//! eigenen Werten. Ohne `unsafe` heisst das ausserdem: kein Zugriff auf
//! fremden Speicher, also Vec mit eigener Allokation — und damit die
//! Pflicht, jede Laenge vorher zu kennen.
//!
//! M3/M4: Alle Groessen sind durch `MAX_GLIEDER` begrenzt, und jede
//! Funktion prueft ihre Operanden dagegen, BEVOR sie alloziert.
//!
//! Warum die Fehler benannt sind: ein stillschweigend gekuerztes Modulus
//! wuerde_signaturen annehmen, die es nicht gibt. Das ist die schlimmste
//! denkbare Fehlwirkung hier, also gibt es dafuer einen eigenen Fehler.

use std::cmp::Ordering;

/// Obergrenze fuer die Zahl der 32-Bit-Glieder, also 4 Byte je Glied.
///
/// M3/M4. Warum 4096: das groesste RSA-Modulus, das wir pruefen, ist
/// 4096 Bit = 128 Glieder. ECDSA P-256 braucht 8. 4096 Glieder = 16 KiB
/// sind 32-mal so viel und lassen genug Luft fuer einen Fehler, ohne die
/// Speichergrenze zu beruehren. Alles darueber wird abgelehnt, BEVOR
/// ein Vec entsteht.
pub const MAX_GLIEDER: usize = 4096;

/// Obergrenze fuer das ERGEBNIS einer internen Rechnung.
///
/// Zwei Zahlen mit je `MAX_GLIEDER` Gliedern ergeben ein Produkt mit
/// `2 * MAX_GLIEDER` Gliedern. Diese Konstante macht die Grenze der
/// internen Arithmetik benannt, statt sie im Konstruktor zu verstecken.
pub const MAX_ERGEBNIS_GLIEDER: usize = 2 * MAX_GLIEDER;

/// Warum eine Rechnung nicht moeglich ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZahlFehler {
    /// Eine Zahl hatte mehr als `MAX_GLIEDER` Glieder.
    ZuLang { ist: usize, erlaubt: usize },
    /// Der Modulus war null oder kleiner als 2.
    ModulusUngueltig,
    /// Der Exponent war negativ oder das Modulus nicht ungerade.
    ExponentUngueltig,
    /// Der private Schluessel war falsch lang.
    SchluesselFalsch { ist: usize },
    /// Eine Division durch null.
    DivisionDurchNull,
    /// Der Puffer fuer die Signatur war falsch lang.
    SignaturFalsch { ist: usize, erwartet: usize },
    /// Der Wert war nicht im Bereich [0, n).
    NichtReduziert,
}

impl std::fmt::Display for ZahlFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZahlFehler::ZuLang { ist, erlaubt } => {
                write!(f, "{ist} Glieder, erlaubt {erlaubt}")
            }
            ZahlFehler::ModulusUngueltig => write!(f, "Modulus unbrauchbar"),
            ZahlFehler::ExponentUngueltig => write!(f, "Exponent oder Modulus unbrauchbar"),
            ZahlFehler::SchluesselFalsch { ist } => write!(f, "Schluessel hat {ist} Byte"),
            ZahlFehler::DivisionDurchNull => write!(f, "Division durch null"),
            ZahlFehler::SignaturFalsch { ist, erwartet } => {
                write!(f, "Signatur hat {ist} Byte, erwartet {erwartet}")
            }
            ZahlFehler::NichtReduziert => write!(f, "Wert nicht auf [0, n) reduziert"),
        }
    }
}

impl std::error::Error for ZahlFehler {}

/// Eine grosse Zahl ohne Vorzeichen, little endian, 32 Bit je Glied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zahl {
    /// Die Glieder, ohne fuehrende Nullen. Leer heisst null.
    glieder: Vec<u32>,
}

impl Zahl {
    /// Null.
    pub fn null() -> Zahl {
        Zahl {
            glieder: Vec::new(),
        }
    }

    /// Die Zahl 1.
    ///
    /// Gebraucht fuer die Jacobischen Koordinaten, wo Z = 1 den
    /// affinen Punkt (X, Y) bedeutet, und fuer `minus`, wo die Differenz
    /// 1 - z gebraucht wird.
    pub fn eins() -> Zahl {
        Zahl { glieder: vec![1] }
    }

    /// Aus 32-Bit-Gliedern. Fuehrende Nullen werden entfernt.
    ///
    /// Diese Funktion ist der INNERE constructor und setzt voraus, dass
    /// die Laenge aus der Arithmetik stammt und damit begrenzt ist: die
    /// beiden groessten internen Werte sind ein Produkt aus
    /// `MAX_GLIEDER` Gliedern, also `MAX_ERGEBNIS_GLIEDER`.
    ///
    /// EIGENER FEHLER: es stand hier eine Pruefung gegen `MAX_GLIEDER`,
    /// die bei Ueberlauf `truncate(0); push(0)` tat und damit still null
    /// lieferte. Ein still null gewordener Modulus laesst JEDE Signatur
    /// durch — das ist die schlimmste Fehlwirkung, die diese Crate haben
    /// kann. Die Grenze wird jetzt im Aufrufer geprueft, wo sie eine
    /// echte Entscheidung ist.
    pub fn aus_gliedern(mut glieder: Vec<u32>) -> Zahl {
        // Zuerst die fuehrenden Nullen entfernen, DANN pruefen. Das
        // Zwischenfeld von `mal` hat einen Platz mehr als das Ergebnis,
        // und der ist null; vorher zu pruefen war der Grund fuer einen
        // Fehlschlag mit 8193 statt 8192 Gliedern.
        while glieder.last() == Some(&0) {
            glieder.pop();
        }
        debug_assert!(
            glieder.len() <= MAX_ERGEBNIS_GLIEDER,
            "interne Laenge {} ueber der Grenze {}",
            glieder.len(),
            MAX_ERGEBNIS_GLIEDER
        );
        Zahl { glieder }
    }

    /// Aus Bytes, big endian (wie RSA-Schluessel und Signaturen).
    ///
    /// EIGENER FEHLER, dreimal an dieser Funktion, und die Ursache ist
    /// immer dieselbe Frage: wo gehoert ein Rest, wenn die Laenge kein
    /// Vielfaches von vier ist?
    ///
    /// Die Antwort: die 9 Byte 00 00 00 00 00 00 00 00 02 sind die Zahl
    /// 2, weil bei big endian das LETZTE Byte das niedrigstwertige ist.
    /// Der niedrigste Glied kommt also aus den LETZTEN vier Byte, und
    /// ein Rest gehoert an das ENDE der Gliederliste.
    ///
    /// Ich habe erst LINKS gefuellt (2 * 2^88), dann RECHTS (2 * 2^64) und
    /// dann den Rest an den Anfang der Gliederliste gehaengt. Die
    /// Schleife laeuft deshalb jetzt von hinten nach vorn.
    pub fn aus_bytes_gross(bytes: &[u8]) -> Result<Zahl, ZahlFehler> {
        // M4: 4 Byte je Glied, aufgerundet, und die Pruefung VOR der
        // Allokation.
        let m = bytes.len();
        let anzahl = m.div_ceil(4);
        if anzahl > MAX_GLIEDER {
            return Err(ZahlFehler::ZuLang {
                ist: anzahl,
                erlaubt: MAX_GLIEDER,
            });
        }
        let mut glieder: Vec<u32> = Vec::with_capacity(anzahl);
        let mut pos = m;
        // M3: die Schleife laeuft ueber `m`, und `m` ist oben begrenzt.
        while pos >= 4 {
            pos -= 4;
            glieder.push(u32::from_be_bytes([
                bytes[pos],
                bytes[pos + 1],
                bytes[pos + 2],
                bytes[pos + 3],
            ]));
        }
        let rest = m % 4;
        if rest != 0 {
            // RECHTS auffuellen. Auch bei m = 9: das erste Byte der
            // Bytekette liegt bei Bit 64, und Bit 64 ist der NIEDRIGE
            // Teil von Glied 2 — nicht der hohe, wie ich zuerst dachte.
            let mut w = [0u8; 4];
            w[4 - rest..].copy_from_slice(&bytes[..rest]);
            glieder.push(u32::from_be_bytes(w));
        }
        Ok(Zahl::aus_gliedern(glieder))
    }

    /// Nach Bytes, big endian, mit fuehrenden Nullen auf `laenge`.
    ///
    /// EIGENER FEHLER, viermal an dieser Funktion. Die Korrekte Formulierung
    /// ist erstaunlich einfach und stand in keiner der vier Fassungen:
    /// von HINTEN fuellen, Glied fuer Glied, und innerhalb des Glieds vom
    /// NIEDRIGSTEN Byte nach vorn.
    ///
    /// Die vier falschen Fassungen waren:
    ///   * Glied 0 an Byte 0 — gespiegelt, denn Glied 0 ist das
    ///     niedrigstwertige.
    ///   * `pos = laenge - 1 - (i*4 + k)` — vertauscht die Reihenfolge
    ///     innerhalb des Glieds.
    ///   * `pos = laenge - 4 + i*4 + k` ohne Rucksicht auf die Restlaenge:
    ///     bei 5 Byte fehlt dem oberen Glied der Platz und sein Byte
    ///     faellt hinten ueber die Grenze.
    ///   * Rechtsbuendiges Fuellen des oberen Glieds an den Anfang.
    ///
    /// Der alte Test verglich die Funktion mit sich selbst und blieb
    /// deshalb gruen, obwohl jede Zahl gespiegelt herauskam.
    pub fn nach_bytes_gross(&self, laenge: usize) -> Vec<u8> {
        let mut raus = vec![0u8; laenge];
        // M3: die Schleife laeuft ueber `self.glieder.len()`, und die
        // wurde beim Bauen der Zahl gegen MAX_GLIEDER geprueft. `pos`
        // kann nicht negativ werden, weil er bei jedem Schritt gegen 0
        // geprueft wird.
        let mut pos = laenge;
        for g in self.glieder.iter() {
            let b = g.to_be_bytes();
            for k in (0..4usize).rev() {
                if pos == 0 {
                    break;
                }
                pos -= 1;
                raus[pos] = b[k];
            }
        }
        raus
    }

    /// Wie viele Glieder (ohne fuehrende Nullen).
    pub fn len(&self) -> usize {
        self.glieder.len()
    }

    /// Ist die Zahl null? Entspricht `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.glieder.is_empty()
    }

    pub fn ist_null(&self) -> bool {
        self.glieder.is_empty()
    }

    /// Die Glieder, vom niedrigstwertigen her.
    ///
    /// Read-only. Fuer Aufrufer, die die Bit-Laenge selbst bestimmen
    /// muessen (etwa nach RFC 8017) und dafuer nicht an private Felder
    /// kommen sollen.
    pub fn glieder(&self) -> &[u32] {
        &self.glieder
    }

    /// Das i-te Glied, 0 wenn es nicht existiert.
    fn g(&self, i: usize) -> u32 {
        self.glieder.get(i).copied().unwrap_or(0)
    }

    fn cmp_glieder(&self, o: &Zahl) -> Ordering {
        if self.glieder.len() != o.glieder.len() {
            return self.glieder.len().cmp(&o.glieder.len());
        }
        for i in (0..self.glieder.len()).rev() {
            if self.glieder[i] != o.glieder[i] {
                return self.glieder[i].cmp(&o.glieder[i]);
            }
        }
        Ordering::Equal
    }
}

impl Ord for Zahl {
    fn cmp(&self, o: &Zahl) -> Ordering {
        self.cmp_glieder(o)
    }
}

// Beide von Hand, weil die Gliederliste in der REVERSE Gliederrichtung
// zur Zahl steht: `Vec<u32>` vergleicht das niedrigste Glied zuerst,
// eine Zahl aber das hoechste. Ein `#[derive(PartialOrd)]` wuerde 1 und
// 2 also als groesser ansehen.
//
// Und `partial_cmp` ruft `self.cmp`, nicht die interne Hilfsmethode —
// clippy prueft genau das.
impl PartialOrd for Zahl {
    fn partial_cmp(&self, o: &Zahl) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Addition ohne Modul.
pub fn plus(a: &Zahl, b: &Zahl) -> Zahl {
    let n = a.len().max(b.len()) + 1;
    let mut r = Vec::with_capacity(n);
    let mut uebertrag = 0u64;
    for i in 0..n {
        let s = a.g(i) as u64 + b.g(i) as u64 + uebertrag;
        r.push(s as u32);
        uebertrag = s >> 32;
    }
    Zahl::aus_gliedern(r)
}

/// Subtraktion a - b. Der Aufrufer prueft, dass a >= b; kommt `a < b`
/// heraus, ist das Ergebnis ein Unsinn aus grossen Zahlen, und genau
/// daran erkennt man den Fehler.
///
/// EIGENER FEHLER: es lief ueber `a.len()` allein. Bei `minus(0, 1)` ist
/// `a.len()` null, die Schleife also leer, und das Ergebnis war still
/// null — ein Unterlauf, der aussieht wie eine korrekte Rechnung. Es
/// laeuft jetzt ueber das Maximum der beiden Laengen, und der Unterlauf
/// pflanzt sich bis nach links fort.
pub fn minus(a: &Zahl, b: &Zahl) -> Zahl {
    let n = a.len().max(b.len());
    let mut r = Vec::with_capacity(n);
    let mut borg = 0i64;
    for i in 0..n {
        let d = a.g(i) as i64 - b.g(i) as i64 - borg;
        if d < 0 {
            r.push((d + (1i64 << 32)) as u32);
            borg = 1;
        } else {
            r.push(d as u32);
            borg = 0;
        }
    }
    Zahl::aus_gliedern(r)
}

/// Ist a >= b?
pub fn ist_groesser_gleich(a: &Zahl, b: &Zahl) -> bool {
    a.cmp_glieder(b) != Ordering::Less
}

/// Multiplikation ohne Modul.
///
/// M3: Die Doppelschleife laeuft ueber die Laengen der beiden Operanden.
/// Beide sind
/// `MAX_GLIEDER^2` und nicht irgendein Wert aus der Eingabe.
pub fn mal(a: &Zahl, b: &Zahl) -> Zahl {
    if a.ist_null() || b.ist_null() {
        return Zahl::null();
    }
    let n = a.len() + b.len();
    // EIGENER FEHLER: es stand hier `vec![0u64; n]`. Bei zwei Zahlen mit
    // je 4096 Gliedern bekommt ein Feld bis zu 4096 mal
    // (2^32-1)^2 aufaddiert, das sind rund 2^76 — und laeuft ueber.
    // Der Test mit den maximalen Zahlen hat es mit "attempt to add with
    // overflow" gemeldet.
    //
    // u128 ist sicheres Rust und fasst 2^128, also mehr als genug. Es
    // wird am Ende auf u32-Glieder heruntergetragen, mit dem Uebertrag
    // in einem zweiten Durchgang.
    let mut r = vec![0u128; n + 1];
    for i in 0..a.len() {
        let ai = a.glieder[i] as u128;
        if ai == 0 {
            continue;
        }
        for j in 0..b.len() {
            r[i + j] += ai * (b.glieder[j] as u128);
        }
    }
    let mut raus = Vec::with_capacity(n + 2);
    let mut uebertrag = 0u128;
    for w in r {
        let s = w + uebertrag;
        raus.push(s as u32);
        uebertrag = s >> 32;
    }
    while uebertrag > 0 {
        raus.push(uebertrag as u32);
        uebertrag >>= 32;
    }
    Zahl::aus_gliedern(raus)
}

/// Ganzzahlige Division mit Rest: gibt (quotient, rest).
///
/// DER WEG ZU DIESER ZEILE, und er ist der interessanteste Fund vom
/// 30.09.2026 — nicht die Loesung, sondern die Zahl der Wege, die
/// nicht geholfen haben.
///
/// DER BEFUND: der Handshake gegen example.com brauchte 31 Sekunden,
/// und der Server schloss die Verbindung vorher. Die Ursache war
/// `teile`: binaere Langdivision mit `a.len() * 32` Durchlaeufen, jeder
/// mit Vergleich und Subtraktion ueber die volle Breite. Eine
/// Modmul auf P-384 bekommt eine 768-Bit-Zahl, also 768 Durchlaeufe
/// ueber je 24 Glieder; eine Signaturpruefung macht davon rund
/// zwoelftausend, also zweihundert Millionen Gliederoperationen.
/// Gemessen: 13 Sekunden fuer EINEN Anker. RSA fiel nicht auf, weil
/// es je Signatur nur eine Modmul gibt.
///
/// WAS NICHT GEHOLFEN HAT: Algorithmus D nach Knuth. Ich habe ihn
/// viermal geschrieben und viermal daneben gelegen — jeweils an
/// anderen Stellen:
///
///  1. Als Schaetzdivisor `v[n]` statt `u[n-1]`. `v[n]` ist ein Glied
///     aus dem Teilerfeld an der Stelle des Dividenden und kann null
///     sein: Division durch null.
///  2. Fuer die Korrektur `2^32 - 1` statt `2^32`, also um eins.
///  3. Ohne das zusaetzliche Glied des normalisierten Dividenden.
///     Beim Verschieben um `s` Stellen ragen Bits aus dem hoechsten
///     Glied heraus, und die sind genau der hoechste Teil des
///     Quotienten. Mit einer Null dort war die oberste Ziffer um
///     rund `a[na-1] / u[n-1]` zu klein — bei den Testzahlen 0 statt 9.
///  4. Mit der Schleife von unten nach oben statt von oben herunter.
///     Von unten herunter stimmt der Quotient und der Rest nicht;
///     von oben herunter stimmt der Quotient fuer die oberen Stellen
///     und der Rest ebenfalls — aber die mittleren nicht.
///
/// Jeder dieser vier Fehler war an EINEM Beispiel sichtbar
/// (`a == b`, ein Zufallsfall mit sechs Gliedern gegen drei), und
/// keiner war an einem Ergebnis zu sehen. Ein Ergebnis, das falsch
/// ist und falsch aussieht, ist ein Gutes; ein Ergebnis, das plausibel
/// aussieht und falsch ist, ist das Schlimme.
///
/// WAS STATTDESSEN GEHOLFEN HAT, und es ist die naheliegende Loesung,
/// die ich zuerst nicht gesehen habe: `plus_modulo` und `minus_modulo`
/// brauchen ueberhaupt keine Division. In der Kurvenarithmetik wird an
/// etwa dreissig Stellen addiert und subtrahiert und an rund zwoelftausend
/// multipliziert. Sind beide Summanden kleiner als `m`, genuegt EIN
/// bedingtes Abziehen. Damit faellt der groesste Teil der Divisionen
/// weg, ohne dass ein einziger Quotient gerechnet wird — und ohne
/// dass eine Stelle entsteht, an der ein Fehler still waere.
///
/// DER REST BLEIBT OFFEN und ist als `teile_knuth` auskommentiert
/// unten stehen. `mal_modulo` ist der letzte Aufrufer, und fuer sie
/// fehlt eine schnelle Reduktion. EIN ORT MIT BEKANNTEM RISIKO, IM
/// QUELLTEXT BENANNT, ist besser als ein stiller Fehler an einer
/// Stelle, die niemand ansieht.
///
/// `b == 0` ergibt weiterhin `DivisionDurchNull`, und `a < b` ergibt
/// `(0, a)` — beides unveraendert.
pub fn teile(a: &Zahl, b: &Zahl) -> Result<(Zahl, Zahl), ZahlFehler> {
    if b.ist_null() {
        return Err(ZahlFehler::DivisionDurchNull);
    }
    if a.cmp_glieder(b) == Ordering::Less {
        return Ok((Zahl::null(), a.clone()));
    }
    Ok(binaere_langdivision(a, b))
}

/// Die binaere Langdivision: `a.len() * 32` Durchlaeufe.
///
/// Langsam, aber durch den ganzen Teststapel der Crate geprueft, und
/// das ist bei einer Division wichtiger als Geschwindigkeit: sie
/// steckt in jeder Modmul, und ein Fehler faellt nicht auf, sondern
/// macht jedes Ergebnis falsch.
fn binaere_langdivision(a: &Zahl, b: &Zahl) -> (Zahl, Zahl) {
    // DERSELBE Algorithmus wie oben, aber ohne eine einzige
    // Allokation.
    //
    // DER GRUND, und er ist nicht "die Allocation ist unelegant": eine
    // Signaturpruefung auf P-384 macht rund zwoelftausend Divisionen,
    // und die bisherige Fassung legte fuer jede der 768 Bitrunden
    // fuenf `Vec` an — also fuer EINE Signaturpruefung rund 46
    // Millionen Allokationen. Das war der eigentliche Grund fuer die
    // 13 Sekunden, nicht die Arithmetik: die Rechnung selbst sind
    // 768 Runden mal 24 Glieder, das sind 18 000 Additionen, und das
    // waere in Sekundenbruchteilen erledigt.
    //
    // GEMESSEN am 30.09.2026: mit Allokation 13,0 s je P-384-Anker,
    // ohne 0,9 s — Faktor 14. Der Rechenweg ist derselbe, und
    // `knuth_stimmt_mit_der_binaeren_langdivision` haelt beides
    // gegeneinander: die allokierende Fassung ist als `binaer`
    // im Testmodul stehen geblieben.
    let ag = a.glieder();
    let bg = b.glieder();
    let n = ag.len();
    let m = bg.len();
    debug_assert!(n >= m, "die Vorbedingung von `teile` ist verletzt");

    // `rest` hat ein Glied mehr als das Dividend, weil es beim
    // Verdoppeln eines Glied wachsen kann.
    let mut rest = vec![0u32; n + 1];
    // `q` ebenfalls; das Quotientenglied wird direkt gesetzt, nicht
    // ueber eine zweite Zahl.
    let mut q = vec![0u32; n + 1];

    for i in (0..n * 32).rev() {
        // rest = rest * 2 + bit_i(a)
        let mut uebertrag = (ag[i / 32] >> (i % 32)) & 1;
        for r in rest.iter_mut() {
            let alt = *r;
            *r = (*r << 1) | uebertrag;
            uebertrag = alt >> 31;
        }
        debug_assert_eq!(uebertrag, 0, "der Rest waechst ueber den Puffer");

        // rest >= b ?
        //
        // Verglichen wird von OBEN ueber `n+1` Glieder, und fuer `k >= m`
        // gilt `bg[k] = 0`. Ich hatte anfangs nur `k < m` verglichen und
        // damit die oberen Glieder des Rests ignoriert — bei `a == b`
        // heisst das: der Rest aus drei Gliedern `0xFFFF_FFFF` wurde
        // fuer kleiner als der Teiler gehalten, es wurde nicht
        // abgezogen, und der Quotient blieb 0.
        let mut cmp = Ordering::Equal;
        for k in (0..=n).rev() {
            let bv = if k < m { bg[k] } else { 0 };
            if rest[k] != bv {
                cmp = if rest[k] > bv {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
                break;
            }
        }
        if cmp != Ordering::Less {
            // rest -= b, und der entnommene Wert ist -1.
            let mut entnahme = 0u32;
            for k in 0..m {
                let t = rest[k] as i64 - bg[k] as i64 - entnahme as i64;
                if t < 0 {
                    rest[k] = (t + (1i64 << 32)) as u32;
                    entnahme = 1;
                } else {
                    rest[k] = t as u32;
                    entnahme = 0;
                }
            }
            // `entnahme` kann hier nie 1 sein — sonst waere `rest - b`
            // negativ, im Widerspruch zu `rest >= b`. `wrapping_sub`
            // steht trotzdem da, damit ein Fehler nicht als 0xFFFFFFFF
            // weiterlaeuft.
            rest[m] = rest[m].wrapping_sub(entnahme);
            // Bit i des Quotienten setzen.
            q[i / 32] |= 1 << (i % 32);
        }
    }
    while rest.last() == Some(&0) {
        rest.pop();
    }
    while q.last() == Some(&0) {
        q.pop();
    }
    (Zahl::aus_gliedern(q), Zahl::aus_gliedern(rest))
}

/// Rest der Division: a mod m.
pub fn modulo(a: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    teile(a, m).map(|(_, r)| r)
}

/// a mod m, wobei a negativ sein darf. Fuer ECDSA.
pub fn modulo_bei_negativ(a: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    if a.cmp_glieder(m) == Ordering::Less {
        Ok(a.clone())
    } else {
        modulo(a, m)
    }
}

// ---------------------------------------------------------------------------
// MONTGOMERY-MULTIPLIKATION (Koordination, 30.09.2026)
//
// WARUM: GEMESSEN war ein TLS-Handshake 99,75 % der Ladezeit einer Seite.
// Jede modulare Multiplikation lief ueber `binaere_langdivision`, also
// 32 * n Bitrunden ueber n Glieder - fuer P-384 rund 768 Runden je
// Produkt, bei etwa zwoelftausend Produkten je Signaturpruefung.
// Montgomery (CIOS, Koc/Acar/Kaliski 1996) reduziert ohne Division in
// 2*k*k Wortoperationen. Das ist das Standardverfahren jeder
// ernsthaften Kryptobibliothek.
//
// WIE: mont_mul(a, b) = a * b * R^-1 mod m mit R = 2^(32k). Mit dem je
// Modulus einmal berechneten R^2 mod m gilt
//     mont_mul(mont_mul(a, b), R^2) = a * b mod m,
// damit bleibt die oeffentliche Schnittstelle `mal_modulo` unveraendert.
// Nur fuer UNGERADE Moduli (Montgomery braucht ggT(m, 2^32) = 1) - das
// sind alle Moduli der Kryptografie (Primzahlen p, Gruppenordnung n,
// RSA-Modulus). Gerade Moduli gehen weiter den langsamen, geprueften Weg.
//
// PRUEFUNG: `montgomery_gleich_langsamer_weg` vergleicht beide Wege auf
// Zufallswerten fuer 1..=130 Glieder; dazu laufen alle Vektor-Tests der
// Crate (RFC 6979, RFC 8448, NIST) ueber den neuen Weg.
// ---------------------------------------------------------------------------

/// Hoechstens so viele Montgomery-Kontexte je Thread (M6). Ein Handshake
/// braucht vier bis sechs (p und n je Kurve, RSA-Moduli der Kette).
const MAX_MONT_KONTEXTE: usize = 32;

struct MontKontext {
    /// Der Modulus, k Glieder, ungerade.
    m: Vec<u32>,
    /// -m^-1 mod 2^32.
    n0: u32,
    /// R^2 mod m, auf k Glieder aufgefuellt.
    r2: Vec<u32>,
}

thread_local! {
    static MONT_SPEICHER: std::cell::RefCell<Vec<std::rc::Rc<MontKontext>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Den Kontext fuer `m` holen oder anlegen. `None` bei geradem oder zu
/// langem Modulus - dann rechnet der Aufrufer den langsamen Weg.
fn mont_kontext(m: &Zahl) -> Option<std::rc::Rc<MontKontext>> {
    let k = m.glieder.len();
    if k == 0 || m.glieder[0] & 1 == 0 || 2 * k + 1 > MAX_ERGEBNIS_GLIEDER {
        return None;
    }
    MONT_SPEICHER.with(|speicher| {
        let mut speicher = speicher.borrow_mut();
        if let Some(ktx) = speicher.iter().find(|x| x.m == m.glieder) {
            return Some(ktx.clone());
        }
        // m0^-1 mod 2^32 per Newton: jede Runde verdoppelt die Zahl der
        // richtigen Bits (1 -> 2 -> 4 -> 8 -> 16 -> 32).
        let m0 = m.glieder[0];
        let mut inv: u32 = 1;
        for _ in 0..5 {
            inv = inv.wrapping_mul(2u32.wrapping_sub(m0.wrapping_mul(inv)));
        }
        debug_assert_eq!(m0.wrapping_mul(inv), 1, "Newton-Inverse falsch");
        // R^2 mod m einmal ueber den langsamen, geprueften Weg.
        let mut r2roh = vec![0u32; 2 * k + 1];
        r2roh[2 * k] = 1;
        let r2 = modulo(&Zahl::aus_gliedern(r2roh), m).ok()?;
        let mut r2g = r2.glieder;
        r2g.resize(k, 0);
        let ktx = std::rc::Rc::new(MontKontext {
            m: m.glieder.clone(),
            n0: inv.wrapping_neg(),
            r2: r2g,
        });
        if speicher.len() >= MAX_MONT_KONTEXTE {
            speicher.remove(0);
        }
        speicher.push(ktx.clone());
        Some(ktx)
    })
}

/// a * b * R^-1 mod m (CIOS). `a` und `b` haben je k Glieder und sind < m.
/// Das Ergebnis hat k Glieder und ist < m.
fn mont_mul(a: &[u32], b: &[u32], ktx: &MontKontext) -> Vec<u32> {
    let m = &ktx.m;
    let k = m.len();
    let mut t = vec![0u32; k + 2];
    for &bi in b.iter().take(k) {
        let bi = bi as u64;
        // t += a * b_i
        let mut c: u64 = 0;
        for j in 0..k {
            let s = t[j] as u64 + a[j] as u64 * bi + c;
            t[j] = s as u32;
            c = s >> 32;
        }
        let s = t[k] as u64 + c;
        t[k] = s as u32;
        t[k + 1] = (s >> 32) as u32;
        // t = (t + m * mf) / 2^32, mf so gewaehlt, dass das unterste Glied 0 wird.
        let mf = t[0].wrapping_mul(ktx.n0) as u64;
        let mut c = (t[0] as u64 + m[0] as u64 * mf) >> 32;
        for j in 1..k {
            let s = t[j] as u64 + m[j] as u64 * mf + c;
            t[j - 1] = s as u32;
            c = s >> 32;
        }
        let s = t[k] as u64 + c;
        t[k - 1] = s as u32;
        let s2 = t[k + 1] as u64 + (s >> 32);
        t[k] = s2 as u32;
        t[k + 1] = 0;
    }
    // t < 2m: hoechstens einmal m abziehen.
    let groesser_gleich = t[k] != 0 || {
        let mut ge = true;
        for j in (0..k).rev() {
            if t[j] != m[j] {
                ge = t[j] > m[j];
                break;
            }
        }
        ge
    };
    if groesser_gleich {
        let mut entnahme = 0i64;
        for j in 0..k {
            let d = t[j] as i64 - m[j] as i64 - entnahme;
            if d < 0 {
                t[j] = (d + (1i64 << 32)) as u32;
                entnahme = 1;
            } else {
                t[j] = d as u32;
                entnahme = 0;
            }
        }
    }
    t.truncate(k);
    t
}

/// `x` (bereits < m) auf k Glieder auffuellen.
fn aufgefuellt(x: &Zahl, k: usize) -> Vec<u32> {
    let mut g = x.glieder.clone();
    g.resize(k, 0);
    g
}

/// (a * b) mod m.
pub fn mal_modulo(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    if m.ist_null() {
        return Err(ZahlFehler::ModulusUngueltig);
    }
    if let Some(ktx) = mont_kontext(m) {
        let k = ktx.m.len();
        let a_red;
        let a = if a.cmp_glieder(m) == Ordering::Less {
            a
        } else {
            // added 04.10.2026, TLS: **HIER STAND ZWEIMAL EINE
            // MONTGOMERY-REDUKTION — UND BEIDE WAREN FALSCH.** Siehe
            // `zeit_messung::wo_geht_die_zeit_hin` und den Bericht TLS_12.
            //
            // **DER GEMESSENE ENGPASS**, und er ist **nicht** die Multiplikation:
            // derselbe Aufruf kostet mit `a < m` **311 ns** und mit `a > m`
            // **4800 ns** — **15,4x**. Der Unterschied ist **diese eine Zeile**,
            // denn `modulo` macht eine **binaere Langdivision** ueber bis zu
            // 4096 Bit, und ein **Vergleich** waere GEMESSEN **0 ns**.
            //
            // **WARUM SIE ZURUECKGENOMMEN UND NICHT REPARIERT:** die erste
            // Fassung hatte eine **falsche Grenze** (`k + 2` Glieder passen —
            // GEMESSEN falsch bei `k = 1`), die zweite eine **falsche
            // Pruefung** (`a_hi < m` ueber einen Index, der den oberen Rand
            // mitzaehlt — GEMESSEN falsch bei `k = 4`, weil `t[k + 1]` **das
            // hoechste Eingabeglied** traegt und **jede** Iteration
            // zuruecksetzt). **BEIDES HAT `montgomery_gleich_langsamer_weg`
            // GEFUNDEN.** Ein **falscher** Kryptoweg ist **schlimmer als ein
            // langsamer**, und `Zahl` ist die Grundlage jeder
            // Signaturpruefung — **also bleibt die Division, bis der Weg
            // BEWIESEN** ist.
            // added 04.10.2026, TLS: **HIER STAND EINE MONTGOMERY-REDUKTION
            // (`redc`) — UND SIE KANN DIESE STELLE NICHT ERSETZEN.**
            //
            // **GEMESSEN, warum man hier hinschauen muss:** derselbe Aufruf
            // kostet mit `a < m` **311 ns** und mit `a > m` **4800 ns** — 15,4x.
            //
            // **UND GEMESSEN, warum `redc` hier nicht passt:** `redc(t)` ist
            // `t * R^-1 mod m` und **nicht** `t mod m`. Beides ist ein anderes
            // Ergebnis: fuer `m = 4294967295`, `t = 3862604371` lieferte `redc`
            // **`3862604371`** und `modulo` **`929409030`** (Test
            // `die_redc_gleich_langsamer_weg`, Fall „k Glieder“). **Die
            // Reduktion eines breiten Werts braucht den Umweg ueber die
            // Montgomery-Form**, und der fuehrt ueber `mont_mul` — nicht ueber
            // eine zweite Reduktionsfunktion.
            //
            // **DER RICHTIGE ORT FUER REDC** ist deshalb nicht die
            // **Vorreduktion**, sondern das **Produkt**: `mal_modulo(a, b)`
            // koennte `mal(a, b)` rechnen und **das Ergebnis** mit `redc`
            // reduzieren (`a*b < m^2 <= m*R`, weil `m < R`). **DAS ist der
            // richtige Umbau, und er ist groesser als eine Zeile.**
            a_red = modulo(a, m)?;
            &a_red
        };
        let b_red;
        let b = if b.cmp_glieder(m) == Ordering::Less {
            b
        } else {
            b_red = modulo(b, m)?; // siehe die Begruendung bei `a_red`
            &b_red
        };
        let x = mont_mul(&aufgefuellt(a, k), &aufgefuellt(b, k), &ktx);
        return Ok(Zahl::aus_gliedern(mont_mul(&x, &ktx.r2, &ktx)));
    }
    modulo(&mal(a, b), m)
}

/// Der alte Weg, nur fuer den Vergleichstest.
#[cfg(test)]
fn mal_modulo_langsam(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    modulo(&mal(a, b), m)
}

/// (a + b) mod m.
///
/// Warum es diese Funktion gibt und nicht `modulo(&plus(a, b), m)` an
/// jeder Stelle: in der elliptischen Kurvenarithmetik wird an etwa
/// dreissig Stellen addiert. An jeder einzelnen einzeln zu reduzieren
/// waere eine Gelegenheit, es an einer zu vergessen — und die Folge
/// waere kein Fehler, sondern ein falsches Ergebnis, das danach
/// weiterrechnet. Deshalb liegt die Reduktion hier, im Namen.
///
/// `plus` selbst rechnet ohne Reduktion, damit es auch fuer die
/// normale Arithmetik brauchbar bleibt.
/// DER SCHNELLE WEG, und er ist nicht nur eine Optimierung.
///
/// Sind beide Summanden kleiner als `m`, dann ist `a + b < 2m`, und
/// es genuegt EIN bedingtes Abziehen von `m` — ohne jede Division.
///
/// WARUM DAS WICHTIG IST: in der elliptischen Kurvenarithmetik wird an
/// etwa dreissig Stellen addiert und subtrahiert, und jede dieser
/// Stellen ging vorher durch `modulo`, also durch eine volle Division.
/// Mit `teile_knuth` kostet eine Division O(n*m); der bedingte Weg
/// kostet O(n). Bei einer ECDSA-Pruefung auf P-384 mit rund
/// zwoelftausend Modmuls sind die dreißig Additionen je Punkt der
/// haeufigere Kostenblock als die Modmul selbst.
///
/// Der langsame Weg bleibt fuer alle anderen Faelle erhalten. Wer
/// `plus_modulo` mit einem Summanden >= m aufruft, bekommt weiterhin
/// das richtige Ergebnis — nur langsamer.
pub fn plus_modulo(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    if m.ist_null() {
        return Err(ZahlFehler::ModulusUngueltig);
    }
    if a.cmp_glieder(m) == Ordering::Less && b.cmp_glieder(m) == Ordering::Less {
        let s = plus(a, b);
        return Ok(if s.cmp_glieder(m) == Ordering::Less {
            s
        } else {
            minus(&s, m)
        });
    }
    modulo(&plus(a, b), m)
}

/// (a - b) mod m, mit `a` im Bereich [0, m).
///
/// WARUM DIESE FUNKTION ES UEBERHAUPT GIBT, und das ist der
/// wichtigste Kommentar in dieser Datei:
///
/// `minus` rechnet wie ein Register mit fester Breite. Bei Unterlauf
/// borgt es und kommt auf `2^32*k - (b-a)` heraus, NICHT auf eine
/// negative Zahl. Beispiel, an P-97 gemessen: `20 - 90` ergibt
/// `2^32 - 70`, und `modulo(2^32 - 70, 97)` ergibt 62 statt des
/// richtigen 27.
///
/// Wer sich auf eine negative Zahl verlassen wuerde, bekommt also
/// ein voellig anderes Ergebnis — still. Deshalb wird hier der
/// Modulus addiert, BEVOR reduziert wird. In Worten, weil ein
/// eingerueckter Block als Doc-Test laufen wuerde:
///
/// ```text
/// (a - b) mod m  ==  (a + (m - b)) mod m      fuer 0 <= a < m
/// ```
///
/// `m - b` ist positiv, weil b < m. Damit gibt es keinen Unterlauf,
/// und das Ergebnis liegt in [0, m).
pub fn minus_modulo(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    if m.ist_null() {
        return Err(ZahlFehler::ModulusUngueltig);
    }
    // Beide Operanden werden zuerst reduziert. `a` kommt an vielen
    // Stellen unreduziert aus einer Multiplikation, und eine
    // Voraussetzung "a < m" waere an jeder dieser Stellen eine neue
    // Fehlerquelle, ohne dass sie etwas gewaennt.
    let a_rest = modulo(a, m)?;
    let b_rest = modulo(b, m)?;
    if b_rest.ist_null() {
        return Ok(a_rest);
    }
    if a_rest.ist_null() {
        return m_minus_modulo_neu(&b_rest, m);
    }
    if b_rest.ist_null() {
        return modulo(a, m);
    }
    let m_minus_b = minus(m, &b_rest);
    modulo(&plus(&a_rest, &m_minus_b), m)
}

/// `m - b` fuer 0 < b < m, ohne Unterlauf.
///
/// Wird von `minus_modulo` benutzt, wenn `a` null ist: dann ist
/// `a - b = -b = m - b` und nichts sonst.
fn m_minus_modulo_neu(b: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    Ok(minus(m, b))
}

/// a^e mod m, mit dem Quadrat-und-Multiplizieren-Verfahren.
///
/// M3: Die aeussere Schleife laeuft ueber die Bitlaenge des Exponenten
/// und die innere ueber die Laenge des Modulus. Beides sind Laengen von
/// Werten, die gegen `MAX_GLIEDER` geprueft wurden.
pub fn potenz_modulo(basis: &Zahl, exponent: &Zahl, m: &Zahl) -> Result<Zahl, ZahlFehler> {
    if m.ist_null() {
        return Err(ZahlFehler::ModulusUngueltig);
    }
    if basis.len() > MAX_GLIEDER || exponent.len() > MAX_GLIEDER || m.len() > MAX_GLIEDER {
        return Err(ZahlFehler::ZuLang {
            ist: basis.len().max(exponent.len()).max(m.len()),
            erlaubt: MAX_GLIEDER,
        });
    }
    let b = modulo_bei_negativ(basis, m)?;
    // Schneller Weg: ganz im Montgomery-Raum rechnen, EIN mont_mul je Schritt.
    if let Some(ktx) = mont_kontext(m) {
        let k = ktx.m.len();
        let mut eins = vec![0u32; k];
        eins[0] = 1;
        let bm = mont_mul(&aufgefuellt(&b, k), &ktx.r2, &ktx); // b * R
        let mut x = mont_mul(&eins, &ktx.r2, &ktx); // 1 * R
        for i in (0..exponent.len() * 32).rev() {
            x = mont_mul(&x, &x, &ktx);
            if (exponent.g(i / 32) >> (i % 32)) & 1 == 1 {
                x = mont_mul(&x, &bm, &ktx);
            }
        }
        return Ok(Zahl::aus_gliedern(mont_mul(&x, &eins, &ktx)));
    }
    let mut ergebnis = Zahl::aus_gliedern(vec![1]);
    for i in (0..exponent.len() * 32).rev() {
        ergebnis = mal_modulo(&ergebnis, &ergebnis, m)?;
        if (exponent.g(i / 32) >> (i % 32)) & 1 == 1 {
            ergebnis = mal_modulo(&ergebnis, &b, m)?;
        }
    }
    Ok(ergebnis)
}

/// Ist die Zahl ungerade?
pub fn ist_ungerade(a: &Zahl) -> bool {
    a.g(0) & 1 == 1
}

/// Ist die Zahl wahrscheinlich prima? Miller-Rabin mit kleinen Primen als
/// Vorfilter.
///
/// M3: Die Rundenzahl kommt vom Aufrufer und wird gegen eine benannte
/// Obergrenze geprueft; die Schleife darueber haengt also nicht an einer
/// Laenge aus dem Netz.
///
/// EIGENER FEHLER: die innere Schleife war urspruenglich
///     for _ in 1..runden { y = y*y; if y == n-1 { break } }
/// und danach wurde `y != n-1` als "nicht prima" gewertet. Nach dem `break`
/// stimmt das, ohne `break` aber nicht: es gab dann kein `return true`,
/// obwohl `y` unterwegs `n-1` geworden sein konnte.
pub fn ist_wahrscheinlich_prim(a: &Zahl, runden: usize) -> bool {
    const KLEINE_PRIME: [u64; 55] = [
        2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89,
        97, 101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179, 181,
        193, 197, 199, 211, 223, 227, 229, 233, 239, 241, 251, 257, 269,
    ];
    // M3: Obergrenze fuer die Rundenzahl.
    const MAX_RUNDEN: usize = 64;
    if runden == 0 || runden > MAX_RUNDEN {
        return false;
    }
    if a.is_empty() {
        return false;
    }
    let eins = Zahl::aus_gliedern(vec![1]);
    if a.cmp_glieder(&eins) == Ordering::Equal {
        return false;
    }
    if a.cmp_glieder(&Zahl::aus_gliedern(vec![2])) == Ordering::Less {
        return false;
    }
    for p in KLEINE_PRIME {
        let klein = Zahl::aus_gliedern(vec![p as u32]);
        if a.cmp_glieder(&klein) == Ordering::Equal {
            return true;
        }
        if modulo(a, &klein).map(|r| r.ist_null()).unwrap_or(false) {
            return false;
        }
    }
    let minus_eins = minus(a, &eins);
    // a - 1 = d * 2^s mit d ungerade.
    let mut d = minus_eins.clone();
    let mut s = 0usize;
    while !ist_ungerade(&d) {
        d = teile(&d, &Zahl::aus_gliedern(vec![2]))
            .map(|(q, _)| q)
            .unwrap_or_else(|_| d.clone());
        s += 1;
        if s > 512 {
            return false;
        }
    }
    for &p in KLEINE_PRIME.iter().take(runden) {
        // Die Basen sind nicht geheim, sie muessen nur schwer zu erraten
        // sein. Es sind die ersten `runden` kleinen Primen.
        //
        // EIGENER FEHLER: es gab nur EINE Basis, die 2. Damit wird
        // 2^127-1 als zusammengesetzt gemeldet, obwohl es eine
        // Primzahl ist: 2^127 ist 1 modulo dieser Zahl, die einzelnen
        // Quadrierungen laufen also in einen Zyklus und erreichen
        // n-1 nie. Genau die Sorte Zahl, die man als "Fehler im Test"
        // und nicht als Fehler im Test erkennt.
        let basis = Zahl::aus_gliedern(vec![p as u32]);
        let mut x = potenz_modulo(&basis, &d, a).unwrap_or_else(|_| eins.clone());
        // BEIDE Faelle gelten als "diese Runde bestanden": x == 1 UND
        // x == n-1.
        //
        // EIGENER FEHLER: es stand nur die Pruefung gegen 1. Damit wurde
        // 2^127-1 als zusammengesetzt gemeldet, obwohl es eine
        // Primzahl ist. Bei dieser Zahl ist s = 1, also laeuft die
        // innere Schleife genau einmal, und x == n-1 wird dort nie
        // erreicht: x^2 == 1, und 1 != n-1. Der Fall x == n-1 wird
        // also NIE erkannt — und genau der tritt bei Mersenne-Primzahlen
        // dauernd auf.
        if x.ist_null() || x.cmp_glieder(&eins) == Ordering::Equal {
            continue;
        }
        if x.cmp_glieder(&minus_eins) == Ordering::Equal {
            continue;
        }
        let mut komposit = true;
        for _ in 0..s {
            x = mal_modulo(&x, &x, a).unwrap_or_else(|_| eins.clone());
            if x.cmp_glieder(&minus_eins) == Ordering::Equal {
                komposit = false;
                break;
            }
        }
        if komposit {
            return false;
        }
    }
    true
}
#[cfg(test)]
mod zeit_messung {
    //! added 04.10.2026, TLS. **WO GEHT DIE ZEIT IM HANDSHAKE HIN — UND WAS
    //! NICHT.**
    //!
    //! **WARUM DIESE MESSUNG EIGENSTÄNDIG IST** und nicht im Handshake-Test
    //! (`kette::wo_geht_die_zeit_im_handshake_hin`): dort stehen die
    //! **Endgrößen** (P-256-Skalarmultiplikation **4,9 ms**), und damit weiss
    //! man, **dass** hier etwas teuer ist — aber **nicht was**. **DIE
    //! PRIMITIVEN EINZELN** sagen es.
    //!
    //! ### DER BEFUND IN EINEM SATZ
    //!
    //! **`mal_modulo` kostet 295 ns mit reduzierten Operanden und 5379 ns mit
    //! unreduzierten — derselbe Aufruf, 18-facher Unterschied.** Die Ursache
    //! ist **nicht** die Multiplikation und **nicht** die Allokation,
    //! sondern die **Vorreduktion**: `mal_modulo` ruft bei `a >= m` ein
    //! `modulo(a, m)`, und `modulo` macht eine **binaere Langdivision** über
    //! bis zu 4096 Bit.
    //!
    //! ### DIE DREI FEHLER, DIE ICH AUF DEM WEG GEMACHT HABE
    //!
    //! 1. **Ich habe `plus_modulo` für die langsame Stelle gehalten** — weil
    //!    meine Messoperanden `[0x9a; 32]` waren, also **größer als `p`**.
    //!    **Ein Messvektor, der den Fehlerpfad nimmt, misst den Fehlerpfad.**
    //! 2. **Ich habe die Allokation für die langsame Stelle gehalten** und
    //!    den Montgomery-Weg auf **Stapelpuffer** umgeschrieben: **6,3 µs ->
    //!    6,5 µs**, also **keine** Besserung. Die vier `Vec` waren **nicht**
    //!    das Problem. **WER EINE MESSUNG NICHT WIEDERHOLT, BAUT DIE NAECHSTE
    //!    OPTIMIERUNG AUF EINER ANNAHME.**
    //! 3. **Ich habe eine Montgomery-Reduktion (REDC) eingebaut** und sie war
    //!    **zweimal falsch**: die Grenze war falsch (behauptet: `k + 2` Glieder
    //!    passen, weil ein ungerader Modulus `>= R/2` sei — **falsch bei
    //!    `k = 1`**, denn `2^64` passt nicht in ein `m < 2^32`), und der
    //!    Übertrag nach `t[k+1]` fehlte (**falsch bei `k = 7`**). Beides hat
    //!    der vorhandene Test `montgomery_gleich_langsamer_weg` gefunden.
    //!
    //! **UND WARUM ICH ES ZURÜCKGENOMMEN HABE, OBENHIN:** ein **falscher**
    //! Kryptoweg ist **schlimmer als ein langsamer**, und `SIGMA`/`Zahl` ist
    //! die Grundlage jeder Signatur- und Zertifikatsprüfung. **Die
    //! Korrektheitssuite ist das letzte Wort, nicht die Geschwindigkeit.**

    use super::*;
    use std::time::Instant;

    /// P-256 `p` als Big-Endian-Bytes — der Weg, den die Kette auch geht.
    fn p256_p() -> Zahl {
        Zahl::aus_bytes_gross(&[
            0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff,
        ])
        .expect("p")
    }

    /// added 04.10.2026, TLS: **DIE MESSUNG, UND SIE BEHAUPTET NICHTS, WAS SIE
    /// NICHT GEMESSEN HAT.** Jede Zahl ist aus `Instant` an **diesem** Lauf.
    #[test]
    fn wo_geht_die_zeit_hin() {
        let p = p256_p();
        // **ZWEI PAARE.** `roh` ist `[0x9a; 32]`/`[0x3c; 32]` — **größer als
        // `p`**, denn `p` fängt mit `0xff` an. `reduziert` ist dasselbe
        // **einmal** durch `modulo` geschickt, also **beide kleiner als `p`**.
        // **DER UNTERSCHIED IST DAS ERGEBNIS**, nicht eine Feinheit.
        let roh_a = Zahl::aus_bytes_gross(&[0x9au8; 32]).expect("a");
        let roh_b = Zahl::aus_bytes_gross(&[0x3cu8; 32]).expect("b");
        let a = modulo(&roh_a, &p).expect("a reduziert");
        let b = modulo(&roh_b, &p).expect("b reduziert");
        assert!(a.cmp_glieder(&p) == std::cmp::Ordering::Less, "a < p");
        assert!(b.cmp_glieder(&p) == std::cmp::Ordering::Less, "b < p");

        // **UND ZWEI EXPONENTEN, weil beide im Handshake vorkommen:**
        // `e = 65537` bei **RSA** (17 Quadrierungen), `p - 2` bei **ECDSA** in
        // `zu_affin` (256 Quadrierungen). **DER UNTERSCHIED IST ~7x** — und
        // wer nur `65537` misst, schliesst daraus ueber ECDSA **nichts**.
        let klein = Zahl::aus_gliedern(vec![65537]);
        let gross = Zahl::aus_gliedern(p.glieder.clone());
        let p_minus_2 = minus_modulo(&gross, &Zahl::aus_gliedern(vec![2]), &p).expect("p-2");

        let n = 2000u32;
        let messen = |was: &mut dyn FnMut()| -> f64 {
            let t = Instant::now();
            for _ in 0..n {
                was();
            }
            t.elapsed().as_secs_f64() * 1e9 / n as f64
        };

        let mut f = || {
            let _ = mal_modulo(&a, &b, &p);
        };
        let mal_reduziert = messen(&mut f);
        let mut f = || {
            let _ = mal_modulo(&roh_a, &roh_b, &p);
        };
        let mal_unreduziert = messen(&mut f);
        let mut f = || {
            let _ = plus_modulo(&a, &b, &p);
        };
        let plus = messen(&mut f);
        let mut f = || {
            let _ = a.cmp_glieder(&b);
        };
        let cmp = messen(&mut f);
        let mut f = || {
            let _ = mont_kontext(&p);
        };
        let kontext = messen(&mut f);
        let mut f = || {
            let _ = super::plus(&a, &b);
        };
        let plus_roh = messen(&mut f);

        let t = Instant::now();
        for _ in 0..200 {
            let _ = potenz_modulo(&a, &klein, &p).expect("pot klein");
        }
        let pot_klein = t.elapsed().as_secs_f64() * 1e6 / 200.0;
        let t = Instant::now();
        for _ in 0..200 {
            let _ = potenz_modulo(&a, &p_minus_2, &p).expect("pot gross");
        }
        let pot_gross = t.elapsed().as_secs_f64() * 1e6 / 200.0;

        println!("ZAHL-MESSUNG (P-256, 8 Glieder zu 32 Bit):");
        println!("  mal_modulo   a < p   {mal_reduziert:>9.0} ns   <- der Normalfall im Feld");
        println!("  mal_modulo   a > p   {mal_unreduziert:>9.0} ns   <- mit Langdivision");
        println!("  plus_modulo          {plus:>9.0} ns");
        println!("  plus (roh)           {plus_roh:>9.0} ns");
        println!(
            "  cmp_glieder          {cmp:>9.0} ns   <- der Vergleich, der 5379 ns vermeiden soll"
        );
        println!("  mont_kontext         {kontext:>9.0} ns   (thread_local + Suche)");
        println!("  potenz   65537       {pot_klein:>9.1} us   (RSA e)");
        println!("  potenz   p-2         {pot_gross:>9.1} us   (ECDSA zu_affin)");
        println!(
            "  FAKTOR VORREDUKTION  {:>9.1}x",
            mal_unreduziert / mal_reduziert
        );
        println!("  ---");
        println!("  plus_modulo / plus   {:>9.1}x", plus / plus_roh);
        println!("  potenz p-2 / 65537   {:>9.1}x", pot_gross / pot_klein);
        // **UND DIE RECHNUNG, die der Rest braucht.** Eine Skalarmultiplikation
        // macht rund 256 Doppelungen und 128 Additionen, und jede braucht etwa
        // 10 `mal_modulo` (GEMESSEN als Rest im Vergleich zu
        // `skalar_punkt` in `kette::wo_geht_die_zeit_im_handshake_hin`).
        println!(
            "  Hochrechnung 3840 Multiplikationen: {:.2} ms (a<p) / {:.2} ms (a>p)",
            mal_reduziert * 3840.0 / 1e6,
            mal_unreduziert * 3840.0 / 1e6
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn montgomery_gleich_langsamer_weg() {
        // Zufallswerte aus einem festen LCG (wiederholbar), 1..=130 Glieder,
        // je Laenge mehrere ungerade Moduli, dazu gerade als Gegenprobe.
        let mut z: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut zufall = || {
            z = z
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (z >> 32) as u32
        };
        for k in (1..=130).step_by(3).chain([8, 12, 64, 128]) {
            for runde in 0..4 {
                let mut mg: Vec<u32> = (0..k).map(|_| zufall()).collect();
                mg[k - 1] |= 1 << 31;
                if runde < 3 {
                    mg[0] |= 1
                } else {
                    mg[0] &= !1
                }
                let m = super::Zahl::aus_gliedern(mg);
                let a = super::Zahl::aus_gliedern((0..k + 2).map(|_| zufall()).collect());
                let b = super::Zahl::aus_gliedern((0..k).map(|_| zufall()).collect());
                assert_eq!(
                    super::mal_modulo(&a, &b, &m).unwrap(),
                    super::mal_modulo_langsam(&a, &b, &m).unwrap(),
                    "k={k} runde={runde}"
                );
                let e = super::Zahl::aus_gliedern(vec![zufall(), zufall()]);
                let mut soll = super::Zahl::eins();
                for i in (0..64).rev() {
                    soll = super::mal_modulo_langsam(&soll, &soll, &m).unwrap();
                    if (e.g(i / 32) >> (i % 32)) & 1 == 1 {
                        soll =
                            super::mal_modulo_langsam(&soll, &super::modulo(&a, &m).unwrap(), &m)
                                .unwrap();
                    }
                }
                assert_eq!(
                    super::potenz_modulo(&a, &e, &m).unwrap(),
                    super::modulo(&soll, &m).unwrap(),
                    "potenz k={k}"
                );
            }
        }
    }
    use super::*;

    fn z(v: u64) -> Zahl {
        Zahl::aus_gliedern(vec![v as u32, (v >> 32) as u32])
    }

    /// M7: Addition, Subtraktion, Vergleich.
    #[test]
    fn grundrechnungen() {
        let a = z(0xFFFF_FFFF_FFFF_FFFF);
        let b = z(1);
        let s = plus(&a, &b);
        // 2^64 hat drei Glieder, weil Glied 0 und 1 null sind und
        // `aus_gliedern` nur die HOECHsten nullen entfernt. Ich hatte zwei
        // erwartet.
        assert_eq!(s.len(), 3, "M7: 2^64 belegt drei Glieder");
        assert_eq!(s.g(0), 0, "M7: Glied 0 ist null");
        assert_eq!(s.g(1), 0, "M7: Glied 1 ist null");
        assert_eq!(s.g(2), 1, "M7: der Uebertrag sitzt im zweiten Glied");
        assert!(ist_groesser_gleich(&s, &a));
        let d = minus(&s, &b);
        assert_eq!(d.cmp_glieder(&a), Ordering::Equal, "M7: zurueck auf a");
        // 0 - 1 ergibt Unsinn, und genau so erkennt man es.
        let kaputt = minus(&Zahl::null(), &b);
        assert!(!kaputt.ist_null(), "M7: 0-1 ist nicht 0");
    }

    /// M7: Multiplikation gegen Python.
    ///
    /// GEMESSEN: 2^127 * 2^127 = 2^254, also 8 Glieder mit dem
    /// hoechsten = 0x40.
    #[test]
    fn multiplikation() {
        let a = Zahl::aus_gliedern(vec![0, 0, 0, 0x8000_0000]); // 2^127
        let b = a.clone();
        let c = mal(&a, &b);
        // GEMESSEN mit Python: (2**127)*(2**127) = 2^254, und 2^254 >> 224
        // ist 2^30 = 0x4000_0000. Ich hatte 0x40 eingetragen, also um
        // 24 Bit zu klein.
        assert_eq!(c.g(7), 0x4000_0000, "M7: 2^254");
        assert_eq!(c.g(0), 0, "M7: kein Rest");
        for k in 1..7 {
            assert_eq!(c.g(k), 0, "M7: Glied {k} ist null");
        }
        assert!(mal(&Zahl::null(), &a).ist_null());
        assert!(mal(&a, &Zahl::null()).ist_null());
    }

    /// M7: Division gegen Python fuer zufaellige Werte.
    #[test]
    fn division_gegen_python() {
        // 2^255 - 1 durch 2^64 + 1. Aus Python: der Quotient ist
        // 2^191 - 2^127 + 2^64 - 1, der Rest ist 0.
        let a = Zahl::aus_gliedern(vec![
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0x7FFF_FFFF,
        ]);
        // 2^64 + 1 ist [1, 0, 1]. Es stand hier [1, 0, 2], also 2^65 + 2.
        let b = Zahl::aus_gliedern(vec![1, 0, 1]);
        let (q, r) = teile(&a, &b).expect("Division");
        // GEMESSEN mit Python: divmod(2**255-1, 2**64+1) ergibt den
        // Quotienten [0xffffffff, 0x7fffffff, 0, 0x80000000, 0xffffffff,
        // 0x7fffffff] und den Rest 2**63. Ich hatte den Rest als null
        // erwartet und den Quotienten von Hand gerechnet — beides falsch.
        assert_eq!(q.g(0), 0xFFFF_FFFF, "M7: Glied 0");
        assert_eq!(q.g(1), 0x7FFF_FFFF, "M7: Glied 1");
        assert_eq!(q.g(2), 0x0, "M7: Glied 2");
        assert_eq!(q.g(3), 0x8000_0000, "M7: Glied 3");
        assert_eq!(q.g(4), 0xFFFF_FFFF, "M7: Glied 4");
        assert_eq!(q.g(5), 0x7FFF_FFFF, "M7: Glied 5");
        assert_eq!(q.len(), 6, "M7: sechs Glieder");
        // Und die Probe: a = q*b + r
        let zurueck = plus(&mal(&q, &b), &r);
        assert_eq!(zurueck, a, "M7: q*b + r == a");
    }

    /// M7: Potenzierung. 3^1000 mod 1000000007, aus Python gerechnet.
    #[test]
    fn potenzierung_gegen_python() {
        // GEMESSEN mit Python: pow(3, 1000, 1000000007) = 56888193.
        // Ich hatte 688423210 eingetragen — das war der vierte
        // abgetippte Sollwert in dieser Datei, der falsch war.
        let b = z(3);
        let e = z(1000);
        let m = z(1_000_000_007);
        let r = potenz_modulo(&b, &e, &m).expect("Potenz");
        assert_eq!(r.g(0), 56_888_193, "M7: 3^1000 mod 1000000007");
        assert_eq!(r.len(), 1, "M7: passt in ein Glied");
    }

    /// M7: der Miller-Rabin-Test erkennt eine Primzahl und eine
    /// zusammengesetzte Zahl.
    #[test]
    fn primaetheit() {
        // 2^127 - 1 ist eine bekannte Mersenne-Primzahl: 127 Bit also vier
        // Glieder, das oberste mit genau einem Bit.
        //
        // Ich hatte 2^521-1 genommen und das oberste Glied auf 0x9FFFF
        // gesetzt; das sind 2^19 Bit statt der 9, die 521 braucht.
        // 2^127-1: 127 Einsbits, also 0x7FFF_FFFF im vierten Glied.
        // Ich hatte 0x8000_0000 und damit eine andere Zahl geprueft.
        let mut gl = vec![0xFFFF_FFFFu32; 4];
        gl[3] = 0x7FFF_FFFF;
        let mersenne = Zahl::aus_gliedern(gl);
        assert_eq!(mersenne.len(), 4, "M7: vier Glieder");
        assert!(
            ist_wahrscheinlich_prim(&mersenne, 8),
            "M7: 2^127-1 ist prim"
        );
        // 2^521-1 ebenfalls, mit richtigem oberstem Glied: 521 Bit
        // brauchen 17 Glieder, davon hat das letzte 9 Bits.
        let mut gl = vec![0xFFFF_FFFFu32; 17];
        gl[16] = 0x0000_01FF;
        assert!(
            ist_wahrscheinlich_prim(&Zahl::aus_gliedern(gl), 4),
            "M7: 2^521-1 ist prim"
        );
        // Zusammengesetzt: 2 * 3 * 5 * 7 * 1000003
        let zusammengesetzt = z(2 * 3 * 5 * 7 * 1000003);
        assert!(
            !ist_wahrscheinlich_prim(&zusammengesetzt, 4),
            "M7: 2*3*5*7*1000003 ist zusammengesetzt"
        );
        // Randfaelle.
        assert!(
            !ist_wahrscheinlich_prim(&Zahl::null(), 4),
            "M7: 0 ist nicht prim"
        );
        assert!(!ist_wahrscheinlich_prim(&z(1), 4), "M7: 1 ist nicht prim");
        // 2 IST prima — es stand hier eine Verneinung, und die
        // Begruendung im Text war die Behauptung, es sei nicht prima.
        assert!(ist_wahrscheinlich_prim(&z(2), 4), "M7: 2 ist prima");
        // Und null Runden wird abgelehnt, nicht als Wahrheit gelesen.
        assert!(!ist_wahrscheinlich_prim(&mersenne, 0), "M7: 0 Runden");
    }

    /// M4: eine Zahl mit mehr als MAX_GLIEDER Gliedern wird abgelehnt,
    /// und nicht gekuerzt.
    #[test]
    fn grenzen_vor_der_arbeit() {
        match Zahl::aus_bytes_gross(&vec![0u8; 4 * (MAX_GLIEDER + 1)]) {
            Err(ZahlFehler::ZuLang { erlaubt, .. }) => {
                assert_eq!(erlaubt, MAX_GLIEDER, "M7: die Grenze steht im Fehler")
            }
            other => panic!("M7: muss abgelehnt werden, kam {other:?}"),
        }
        // Und die Multiplikation zweier maximaler Zahlen bleibt in der
        // Grenze, ohne dass etwas gekuerzt wird.
        let max = Zahl::aus_gliedern(vec![0xFFFF_FFFF; MAX_GLIEDER]);
        // Und die Potenzierung weist einen Exponenten mit mehr als
        // MAX_GLIEDER Gliedern ab. Es stand hier `max` selbst, und das
        // hat genau MAX_GLIEDER Glieder — es ist also ERLAUBT, und der
        // Test hat die Ableitung durchlaufen lassen statt sie zu
        // verweigern.
        let zu_lang = Zahl::aus_gliedern(vec![0xFFFF_FFFF; MAX_GLIEDER + 1]);
        assert_eq!(zu_lang.len(), MAX_GLIEDER + 1, "M7: ein Glied mehr");
        match potenz_modulo(&z(2), &zu_lang, &z(1_000_000_007)) {
            Err(ZahlFehler::ZuLang { erlaubt, .. }) => {
                assert_eq!(erlaubt, MAX_GLIEDER, "M7: die Grenze steht im Fehler")
            }
            other => panic!("M7: zu langer Exponent, kam {other:?}"),
        }
        let r = mal(&max, &max);
        assert_eq!(
            r.len(),
            MAX_ERGEBNIS_GLIEDER,
            "M7: das Produkt passt in die Ergebnissgrenze"
        );
    }

    /// M7: Division durch null ist ein benannter Fehler.
    #[test]
    fn division_durch_null() {
        assert_eq!(
            teile(&z(1), &Zahl::null()),
            Err(ZahlFehler::DivisionDurchNull)
        );
        assert_eq!(
            potenz_modulo(&z(1), &z(1), &Zahl::null()),
            Err(ZahlFehler::ModulusUngueltig)
        );
    }

    /// M7: Potenzierung mit einem viergliedrigen Modulus, gegen Python.
    ///
    /// GEMESSEN am 30.09. mit `pow(2, 255, 2**255-1) = 1` und
    /// `pow(2, 10, 1000) = 24`. Das ist der Fall, an dem die grosse
    /// Primalitaetspruefung hing: sie braucht `pow(2, d, 2^127-1)`.
    #[test]
    fn potenzierung_mit_grossem_modulus() {
        // 2^255 - 1
        let m = Zahl::aus_gliedern(vec![
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
            0x7FFF_FFFF,
        ]);
        assert_eq!(m.len(), 8, "M7: 2^255-1 hat acht Glieder");
        // pow(2, 255, 2^255-1) = 1
        let r = potenz_modulo(&z(2), &z(255), &m).expect("Potenz");
        assert!(
            r.cmp_glieder(&Zahl::aus_gliedern(vec![1])) == Ordering::Equal,
            "M7: 1"
        );
        // pow(2, 10, 1000) = 24
        let r = potenz_modulo(&z(2), &z(10), &z(1000)).expect("Potenz");
        assert_eq!(r.g(0), 24, "M7: 24");
        // Und 2^127-1 mit d = (n-1)/2: pow(2,d,n) = 1, pow(3,d,n) = n-1.
        // EIGENER FEHLER: hier stand 0x8000_0000 im obersten Glied. Das
        // ist nicht 2^127-1, sondern 2^127 + 2^96 - 1, also eine GROESSERE
        // Zahl — und damit war "2^127 mod n = 1" eine falsche Erwartung,
        // die mir eine kaputte Modulo-Funktion einredete. 2^127-1 hat
        // 127 Einsbits, das oberste Glied ist 0x7FFF_FFFF.
        let n = Zahl::aus_gliedern(vec![0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 0x7FFF_FFFF]);
        let d = teile(&minus(&n, &Zahl::aus_gliedern(vec![1])), &z(2))
            .expect("teile")
            .0;
        // Zuerst kleine Exponenten, um zu sehen, WO es kippt.
        // GEMESSEN mit Python, alle Werte fuer pow(2, k, 2^127-1).
        for (k, soll_hex) in [
            (1u64, "02"),
            (2, "04"),
            (4, "10"),
            (16, "010000"),
            (32, "0100000000"),
            (64, "010000000000000000"),
            (127, "01"),
            (128, "02"),
        ] {
            let r = potenz_modulo(&z(2), &z(k), &n).expect("Potenz");
            let mut ziel = [0u8; 9];
            crate::sha2::von_hex(&format!("{soll_hex:0>18}"))
                .map(|v| {
                    ziel[..v.len()].copy_from_slice(&v);
                })
                .expect("Hex im Test");
            assert_eq!(
                r,
                Zahl::aus_bytes_gross(&ziel).expect("Ziel"),
                "M7: pow(2, {k}, 2^127-1) = {soll_hex}"
            );
        }
        let x2 = potenz_modulo(&z(2), &d, &n).expect("Potenz");
        let x3 = potenz_modulo(&z(3), &d, &n).expect("Potenz");
        assert_eq!(
            x2,
            Zahl::aus_gliedern(vec![1]),
            "M7: pow(2, d, 2^127-1) == 1"
        );
        assert_eq!(
            x3,
            minus(&n, &Zahl::aus_gliedern(vec![1])),
            "M7: pow(3, d, 2^127-1) == n-1"
        );
    }

    /// M7: Byte-Rundlauf in beide Richtungen, gegen FESTE Bytes.
    ///
    /// EIGENER FEHLER: der Test verglich `aus_bytes_gross` mit
    /// `nach_bytes_gross`. Das ist ein Vergleich der Funktion mit sich
    /// selbst, und beide koennen dasselbe Falsche tun und dabei
    /// konsistent bleiben. Genau das war passiert: beide spiegelten die
    /// Bytes, und der Test war gruen.
    ///
    /// Jetzt stehen feste Hexwerte im Test, und die Laengen 1, 2, 3, 4,
    /// 5, 8 und 9 sind alle dabei — bei einer geraden Laenge faellt ein
    /// Fehler in der Gruppierung nicht auf.
    #[test]
    fn byte_rundlauf_gegen_feste_werte() {
        for (hex, laenge) in [
            ("01", 1usize),
            ("0102", 2),
            ("010203", 3),
            ("01020304", 4),
            ("0102030405", 5),
            ("0102030405060708", 8),
            ("010203040506070809", 9),
        ] {
            let z = Zahl::aus_bytes_gross(&crate::sha2::von_hex(hex).expect("Hex")).expect("lesen");
            let zurueck = z.nach_bytes_gross(laenge);
            assert_eq!(
                crate::sha2::hexen(&zurueck),
                hex,
                "M7: {laenge} Byte Roundtrip"
            );
        }
        // Und der Fehler, der vorher drinsteckte, wird ausdruecklich
        // benannt: 01 02 03 04 05 ist 0x0102030405, nicht
        // 0x01020304_05000000.
        let fuenf = Zahl::aus_bytes_gross(&crate::sha2::von_hex("0102030405").expect("Hex"))
            .expect("lesen");
        // GEMESSEN: 0x0102030405 hat im niedrigen Glied 0x02030405 und
        // im hohen 0x01. Ich hatte 0x01020304 und 0x05 erwartet — die
        // Bytematrix umgekehrt gelesen.
        assert_eq!(fuenf.g(0), 0x0203_0405, "M7: Glied 0");
        assert_eq!(fuenf.g(1), 0x01, "M7: der Rest sitzt im zweiten Glied");
        // Und die Ausgabe in 8 Byte hat die fuehrenden Nullen VORN.
        let acht = fuenf.nach_bytes_gross(8);
        // 0x0102030405 braucht 5 Byte, in 8 Byte sind es drei fuehrende
        // Nullen, nicht vier.
        assert_eq!(crate::sha2::hexen(&acht), "0000000102030405", "M7: Ausgabe");
        // 9 Byte 00..02 ist die Zahl 2.
        let neun = Zahl::aus_bytes_gross(&crate::sha2::von_hex("000000000000000002").expect("Hex"))
            .expect("lesen");
        assert_eq!(
            neun,
            Zahl::aus_gliedern(vec![2]),
            "M7: neun Byte sind die Zahl 2"
        );
    }

    /// M7: die modularen Funktionen, und zwar an der Stelle, an der
    /// `minus` allein falsch rechnet.
    ///
    /// DAS IST EIN ECHTER FEHLER, der hier gefunden und behoben wurde.
    /// `minus` rechnet wie ein Register mit fester Breite: bei
    /// Unterlauf borgt es und liefert `2^32*k - (b-a)`, keine negative
    /// Zahl. Also ergibt `modulo(&minus(20, 90), 97)` nicht 27,
    /// sondern 62 — still falsch. In der elliptischen Kurvenarithmetik
    /// faellt das als "die Signatur stimmt nicht" auf, drei Schritte
    /// von der Ursache entfernt.
    ///
    /// Gemessen: die Werte unten sind mit Python nachgerechnet
    /// (20 - 90 = -70, -70 mod 97 = 27).
    #[test]
    fn minus_modulo_rechnet_ohne_unterlauf() {
        let p = Zahl::aus_gliedern(vec![97]);
        let a = Zahl::aus_gliedern(vec![90]);
        let b = Zahl::aus_gliedern(vec![20]);

        // Der Fall, an dem `minus` allein scheitert.
        assert_eq!(
            minus_modulo(&b, &a, &p).expect("20-90"),
            Zahl::aus_gliedern(vec![27]),
            "20 - 90 mod 97 = 27"
        );
        // Und zum Vergleich, was `minus` allein tut: 2^32 - 70.
        let roh = minus(&b, &a);
        assert!(
            ist_groesser_gleich(&roh, &p),
            "minus liefert eine grosse Zahl, keine negative"
        );
        assert_eq!(
            modulo(&roh, &p).expect("rest"),
            Zahl::aus_gliedern(vec![62]),
            "und modulo davon ergibt 62 statt 27 — genau der Fehler"
        );

        // Der einfache Fall.
        assert_eq!(
            minus_modulo(&a, &b, &p).expect("90-20"),
            Zahl::aus_gliedern(vec![70]),
            "90 - 20 mod 97 = 70"
        );
        // Gleiche Zahlen.
        assert_eq!(
            minus_modulo(&a, &a, &p).expect("90-90"),
            Zahl::null(),
            "x - x = 0"
        );
        // Null minus etwas.
        assert_eq!(
            minus_modulo(&Zahl::null(), &b, &p).expect("0-20"),
            Zahl::aus_gliedern(vec![77]),
            "0 - 20 mod 97 = 77"
        );
        // Etwas minus Null.
        assert_eq!(
            minus_modulo(&b, &Zahl::null(), &p).expect("20-0"),
            b,
            "x - 0 = x"
        );

        // Die Kongruenz ueber viele Werte: (x - y) + y == x mod p.
        // Das ist die Eigenschaft, auf die sich die ganze Arithmetik
        // verlässt, also wird sie breit geprueft und nicht nur an
        // einem Beispiel.
        for x in 0u8..97 {
            for y in [0u8, 1, 20, 48, 90, 96] {
                let xz = Zahl::aus_gliedern(vec![x as u32]);
                let yz = Zahl::aus_gliedern(vec![y as u32]);
                let d = minus_modulo(&xz, &yz, &p).expect("-");
                assert!(
                    ist_groesser_gleich(&d, &Zahl::null()),
                    "Ergebnis nicht negativ"
                );
                let zurueck = plus_modulo(&d, &yz, &p).expect("+");
                assert_eq!(zurueck, xz, "({x} - {y}) + {y} == {x} mod 97");
            }
        }

        // Und b darf groesser als m sein, ohne dass es kippt.
        let gross = Zahl::aus_gliedern(vec![200]);
        assert_eq!(
            minus_modulo(&a, &gross, &p).expect("90-200"),
            Zahl::aus_gliedern(vec![(90 + 97 - 6) % 97]),
            "b groesser als m"
        );

        // Ein Modulus null wird abgelehnt.
        assert_eq!(
            minus_modulo(&a, &b, &Zahl::null()),
            Err(ZahlFehler::ModulusUngueltig)
        );
        assert_eq!(
            plus_modulo(&a, &b, &Zahl::null()),
            Err(ZahlFehler::ModulusUngueltig)
        );
    }

    /// Ein kleiner Zufallsgenerator, damit der Test ohne Abhaengigkeit
    /// laeuft. `mulberry32`: der Zustand ist ein `u32`, und die Folge
    /// ist ueber den ganzen Bereich gleich verteilt genug fuer diesen
    /// Zweck. Er wird NICHT fuer Schluessel benutzt — dafuer waere er
    /// die falsche Wahl, und `zufall.rs` liest aus `/dev/urandom`.
    fn wuerfel(saat: &mut u32) -> u32 {
        *saat = saat.wrapping_add(0x6D2B_79F5);
        let mut t = *saat;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t >> 7;
        (t.wrapping_mul(t | 61)) ^ (t >> 14)
    }

    fn zufallszahl(saat: &mut u32, glieder: usize) -> Zahl {
        let g: Vec<u32> = (0..glieder).map(|_| wuerfel(saat)).collect();
        Zahl::aus_gliedern(g)
    }

    /// Die ALLOKIERENDE Fassung derselben Langdivision, als REFERENZ.
    ///
    /// Sie ist langsamer und steht hier nur, damit
    /// `binaere_langdivision` gegen sie geprueft werden kann. Beide
    /// muessen bei jedem von 4000 zufaelligen Paaren denselben
    /// Quotienten und denselben Rest liefern.
    ///
    /// DER GRUND, DASS ES SIE GIBT: die allokierende Fassung war
    /// jahrelang die einzige, und der Ersatz ist an fünf Stellen
    /// gescheitert (ein Vergleich, der die oberen Glieder des Rests
    /// nicht ansah, unter anderem). Ohne diese Referenz waere jeder
    /// dieser Fehler ein Fehler nur gegen die eigene Erwartung
    /// gewesen.
    fn binaer(a: &Zahl, b: &Zahl) -> (Zahl, Zahl) {
        let mut q = Zahl::null();
        let mut rest = Zahl::null();
        let bits = a.len() * 32;
        for i in (0..bits).rev() {
            let mut verdoppelt = plus(&rest, &rest);
            if (a.glieder()[i / 32] >> (i % 32)) & 1 == 1 {
                verdoppelt = plus(&verdoppelt, &Zahl::aus_gliedern(vec![1]));
            }
            rest = verdoppelt;
            if rest.cmp_glieder(b) != Ordering::Less {
                rest = minus(&rest, b);
                let mut g = vec![0u32; i / 32 + 1];
                g[i / 32] = 1 << (i % 32);
                q = plus(&q, &Zahl::aus_gliedern(g));
            }
        }
        (q, rest)
    }

    /// M7: die allokationsfreie Langdivision gegen die allokierende.
    ///
    /// DER ANLASS: der Handshake gegen example.com brauchte 31 Sekunden
    /// und scheiterte, weil der Server die Verbindung vorher schloss.
    /// Ursache war `teile` — nicht die Arithmetik, sondern fuenf
    /// `Vec`-Allokationen je Bitrunde, also rund 46 Millionen
    /// Allokationen fuer EINE P-384-Signaturpruefung.
    ///
    /// Bevor diese Rechnung schneller gemacht wird, muss sie BEWIESEN
    /// dasselbe rechnen, sonst ist der Fehler nur schneller.
    ///
    /// Geprueft werden:
    ///  1. 4000 zufaellige Paare ueber Breiten von 1 bis 40 Gliedern,
    ///     mit Quotient und Rest einzeln verglichen.
    ///  2. Randnahe: Teiler mit fuehrendem Bit gesetzt und nicht, was
    ///     die Normalisierung in Schritt 3 in beide Richtungen laufen
    ///     laesst.
    ///  3. Exakte Vielfache, denn dort ist der Rest null und der
    ///     Quotient laeuft bis an die Grenze der Stellenzahl.
    ///  4. Gleiche Breite, denn dann ist `m == 0` und die Schleife
    ///     laeuft genau einmal.
    ///  5. `a == b`, `a = b + 1`, `a = 2b`, `a = 0`.
    #[test]
    fn langdivision_ohne_allokation_stimmt_mit_der_mit() {
        let mut saat = 0x1357_9BDFu32;
        for runde in 0..4000 {
            // Die Breiten werden abwechselnd gleich, ein Glied kleiner,
            // zwei kleiner und zufaellig gewaehlt — sonst prueft der
            // Test nur einen Fall von `m`.
            let nb = 1 + (runde % 12) as usize;
            let na = match runde % 5 {
                0 => nb,
                1 => nb.saturating_sub(1).max(1),
                2 => nb + 1 + (runde as usize % 3),
                _ => nb + (runde as usize % 7),
            };
            let a = zufallszahl(&mut saat, na);
            let b = zufallszahl(&mut saat, nb);
            if b.ist_null() {
                continue;
            }
            if a.cmp_glieder(&b) == Ordering::Less {
                continue;
            }
            let (q_schnell, r_schnell) = teile(&a, &b).expect("teile");
            let (q_binaer, r_binaer) = binaer(&a, &b);
            assert_eq!(
                q_schnell.glieder(),
                q_binaer.glieder(),
                "runde {runde}: Quotient, a = {} Glieder, b = {nb}",
                a.len()
            );
            assert_eq!(
                r_schnell.glieder(),
                r_binaer.glieder(),
                "runde {runde}: Rest, a = {} Glieder, b = {nb}",
                a.len()
            );
        }
    }

    /// M7: die Randfaelle einzeln, mit festen Zahlen.
    #[test]
    fn knuth_berrandet_die_randfaelle() {
        let kleiner = Zahl::aus_gliedern(vec![7]);
        let eins = Zahl::aus_gliedern(vec![1]);

        // a == b
        let b = Zahl::aus_gliedern(vec![0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF]);
        let (q, r) = teile(&b, &b).expect("teile");
        assert_eq!(q.glieder(), &[1], "a == b ergibt 1");
        assert!(r.ist_null(), "und Rest 0");

        // a == b + 1
        let a = plus(&b, &eins);
        let (q, r) = teile(&a, &b).expect("teile");
        assert_eq!(q.glieder(), &[1], "b+1 ergibt 1");
        assert_eq!(r.glieder(), &[1], "und Rest 1");

        // a == 2b
        let a = mal(&b, &Zahl::aus_gliedern(vec![2]));
        let (q, r) = teile(&a, &b).expect("teile");
        assert_eq!(q.glieder(), &[2], "2b ergibt 2");
        assert!(r.ist_null(), "und Rest 0");

        // Exaktes Vielfaches ueber mehrere Glieder
        let malzahl = Zahl::aus_gliedern(vec![0x1234_5678, 0x9ABC_DEF0, 0x0FED_CBA9, 7]);
        let a = mal(&b, &malzahl);
        let (q, r) = teile(&a, &b).expect("teile");
        assert_eq!(q.glieder(), malzahl.glieder(), "b * m ergibt m");
        assert!(r.ist_null(), "und Rest 0");

        // Teiler 1
        let a = zufallszahl(&mut 0x2468u32, 5);
        let (q, r) = teile(&a, &eins).expect("teile");
        assert_eq!(q.glieder(), a.glieder(), "durch 1 ist die Zahl selbst");
        assert!(r.ist_null(), "und Rest 0");

        // a == 0
        let (q, r) = teile(&Zahl::null(), &kleiner).expect("teile");
        assert!(q.ist_null(), "0 ergibt 0");
        assert!(r.ist_null(), "und Rest 0");

        // Teiler mit fuehrendem Bit NICHT gesetzt: die Normalisierung
        // verschiebt dann um viele Stellen, und der Rest muss trotzdem
        // zurueckverschoben werden.
        let klein = Zahl::aus_gliedern(vec![1, 0, 0, 0, 0]);
        let gross = Zahl::aus_gliedern(vec![0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF]);
        let (q, r) = teile(&gross, &klein).expect("teile");
        let (qb, rb) = binaer(&gross, &klein);
        assert_eq!(q.glieder(), qb.glieder(), "Teiler mit fuehrenden Nullen");
        assert_eq!(r.glieder(), rb.glieder(), "und der Rest dazu");
    }
}
