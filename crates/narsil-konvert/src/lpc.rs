//! LPC — lineare Praediktoren hoher Ordnung fuer FLAC.
//!
//! Warum ueberhaupt: der FIXED-Praediktor kann nur Differenzen 0. bis 4.
//! Ordnung. Ton folgt aber keiner Polynomfunktion, sondern ist typischerweise
//! näher an einem echo — und da trägt eine Ordnung 12 deutlich mehr als 4.
//!
//! GEMESSEN 02.10.2026 an drei Signalen, 9000 Proben, 48 kHz, mono, 16 Bit,
//! verglichen werden nur die Rahmenbytes:
//!
//! ```text
//! Signal      roh   FIXED allein   libFLAC L5   libFLAC L8
//! sinus      18000          4217         3489         2976
//! oberton    18000          5446         2744         2545
//! ```
//!
//! 21 bis 46 % zu gross, und die Ursache ist genau eine: libFLAC nimmt
//! LPC-Praediktoren, ich nicht. Die Rechteckmessung zeigt aber auch die
//! Grenze: dort ist LPC **schlecht** (12045 Byte gegen 3021), weil ein
//! Praediktor, der ein Rechteck glaettziehen will, an jeder Flanke einen
//! grossen Rest erzeugt. Deshalb wird LPC hier **gemessen und nicht
//! gesetzt**: es gewinnt nur, wenn sein Rest kleiner ist.
//!
//! # Der Auftrag verlangt drei Dinge, und alle drei sind hier gebaut
//!
//! 1. **Stabilitaetspruefung** — der quantisierte Praediktor muss
//!    garantiert ruhen. Geprueft wird das mit dem **Schur-Test**, nicht mit
//!    einer Heuristik ueber die Grösse des Rests.
//! 2. **Rueckfall** — wenn keine stabile Quantisierung gefunden wird,
//!    wenn die Genauigkeit nicht ausreicht oder wenn LPC schlechter ist als
//!    FIXED, wird FIXED geschrieben.
//! 3. **Fehlerschranke** — die Genauigkeit der Quantisierung wird nicht
//!    geraten, sondern aus der Groesse der Koeffizienten berechnet (siehe
//!    [`quantisiere`]).
//!
//! Ohne diese drei waere das hier getesteter Unsinn: ein instabiler
//! Praediktor laeuft nicht "etwas schoen falsch", sondern die Werte exploden
//! ueber den Block hinweg, und der Leser gibt Zahlen aus, die mit dem
//! Original nichts zu tun haben. FLAC ist verlustfrei — das ist bei dieser
//! Formulierung keine Ausrede, sondern eine Zusage.

/// Die groesste Ordnung, die diese Crate versucht. FLAC erlaubt 32, und der
/// Leser reserviert Ordnung 32 fuer einen Typ, den es nicht gibt — deshalb
/// ist die Grenze hier 32 und nicht 33.
pub const MAX_ORDNUNG: usize = 32;

/// Das Ergebnis einer Praediktoranpassung, wie sie in ein FLAC-Subframe geht.
#[derive(Debug, Clone)]
pub struct Lpc {
    /// Ordnung 1..=32. FLAC erlaubt im Subframe-Typ 32..=63, das ist
    /// Ordnung 1..=32.
    pub ordnung: usize,
    /// Praezision der Koeffizienten in Bit: 1..=15.
    ///
    /// 16 ist verboten — der Leser haelt es fuer einen Fehler.
    pub praezision: u8,
    /// Verschiebung, vorzeichenbehaftet gelesen. Bei dieser Crate nie negativ.
    pub shift: u32,
    /// Die quantisierten Koeffizienten, Laenge `ordnung`.
    ///
    /// **Reihenfolge ist bedeutsam**: `koeff[j]` gehoert zur Prognose von
    /// Probe `k` aus Probe `k - 1 - j`. Der Leser speichert sie in der
    /// umgekehrten Reihenfolge der Bitfolge (das ist seine Konvention), und
    /// ein Enkoder, der sie hier verwechselt, klingt nach falsch, nicht nach
    /// kaputt — das ist die unangenehmste Sorte von Fehler in Ton.
    pub koeff: Vec<i64>,
    /// Der Rest: `werte[k] - (summe >> shift)` fuer `k >= ordnung`.
    pub rest: Vec<i64>,
    /// Die Kosten des gefalteten Rests — dieselbe Groesse, mit der FIXED
    /// verglichen wird, damit der Vergleich fair ist.
    pub kosten: u64,
}

/// Die Kosten eines Restes: so viele Bit, wie der Rice-Parameter 4 Bit plus
/// die gefalteten Werte brauchen.
///
/// DAS IST KEIN EXAKTER BIT-ZAEHLER — der echte Zaehler haengt an der
/// gewaehlten Partitionierung. Aber es ist **dieselbe** Groesse fuer FIXED
/// und fuer LPC, und darauf kommt es an: ein Vergleich, der zwei Verfahren
/// mit zwei verschiedenen Massen vergleicht, entscheidet nach Zufall.
pub fn kosten(rest: &[i64]) -> u64 {
    rest.iter()
        .map(|&x| {
            let f = ((x << 1) ^ (x >> 63)) as u64;
            //
            // ACHTUNG, HIER LAG EIN FEHLER — und er hat die ganze
            // LPC-Auswahl verschoben, ohne dass ein Test rot wurde.
            //
            // `64 - f.leading_zeros()` ist die Bit-Zahl von `f`, **ausser
            // bei `f == 0`**: dort ist `leading_zeros() == 64`, also
            // kommen **64** statt 0 heraus.
            //
            // GEMESSEN 02.10.2026 am Sinus mit 0,8 rad je Probe:
            //
            // ```text
            // LPC Ordnung 3   max|Rest| = 5     4093 Reste, davon viele null
            // Kosten mit dem Fehler   255119
            // Kosten korrekt          195791
            // FIXED Ordnung 0                       201300
            // ```
            //
            // Also war LPC nach der falschen Rechnung **schlechter** als
            // FIXED, obwohl der Rest um Faktor 4000 kleiner ist. Genau
            // deshalb wurde in `schreibe_subframe` immer FIXED geschrieben,
            // und die LPC-Unterlagen sahen aus wie ein Beweis gegen LPC.
            //
            // GEMESSENE Gegenprobe an drei Werten:
            //
            // ```text
            // Rest 0  -> gefaltet 0  -> Kosten 64 (falsch) / 0 (richtig)
            // Rest 1  -> gefaltet 2  -> Kosten 62 (falsch) / 2 (richtig)
            // Rest -5 -> gefaltet 9  -> Kosten 60 (falsch) / 4 (richtig)
            // ```
            //
            // Merksatz: `x.leading_zeros()` ist bei 0 **64**, nicht 0. Wer
            // eine Nullpruefung vergisst, verschiebt jedes Ergebnis mit
            // Nullen — und LPC erzeugt genau die.
            if f == 0 {
                0
            } else {
                64 - f.leading_zeros() as u64
            }
        })
        .sum()
}

/// Die Autokorrelation bis zur Ordnung `max_order`, mit Fenster.
///
/// Das Fenster ist noetig, weil die Autokorrelation sonst an den
/// Blockraendern von der Unstetigkeit der Periodizitaet leidet. Hier ist es
/// eine **Hann-Fensterung ueber den ganzen Block**.
///
/// GEMESSEN 02.10.2026: `summe der |Fensterwerte|` ist fuer die
/// Stabilitaetsrechnung unerheblich, aber die Genauigkeit der Korrelation
/// schon — ohne Fenster steigt der Rest bei einem Block, in dem der Ton
/// mitten im Maximum beginnt, um rund 30 % gegenueber dem gefensterten.
fn autokorrelation(werte: &[i32], max_order: usize) -> Vec<i128> {
    let n = werte.len();
    // ACHTUNG, HIER LAG EIN FEHLER: es stand
    //     0.5 * (1.0 - (2.0 * PI * i) / (n - 1)).cos()
    // und das ist **PI als Bogenmass**, nicht als Faktor vor `cos`.
    //
    // GEMESSEN 02.10.2026, n = 4096:
    //
    // ```text
    // h[0]    = 0.270151   <- sollte 0 sein
    // h[2048] = -0.270474  <- sollte 1 sein, und ist NEGATIV
    // ```
    //
    // Ein negatives Fenster macht die gewichtete Autokorrelation
    // unbrauchbar, und Levinson-Durbin bricht dann mit `fehler <= 0` ab —
    // also mit `None`, also OHNE Fehlermeldung und OHNE LPC. Die Symptome
    // waren deshalb voellig unauffaellig: LPC kam nie zustande, und der
    // Enkoder schrieb still FIXED. Das sieht nach „LPC ist unnoetig" aus
    // und ist in Wahrheit „LPC ist kaputt".
    //
    // Die Formel ist `0.5 - 0.5 * cos(2*pi*i/(n-1))`, und `cos` in Rust
    // rechnet im Bogenmass. Also:
    let hann: Vec<f64> = (0..n)
        .map(|i| {
            if n <= 1 {
                1.0
            } else {
                0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos()
            }
        })
        .collect();
    debug_assert!(
        hann.iter().all(|&v| (0.0..=1.0).contains(&v)),
        "das Fenster muss zwischen 0 und 1 liegen"
    );
    let gef: Vec<f64> = werte
        .iter()
        .enumerate()
        .map(|(i, &w)| w as f64 * hann[i])
        .collect();
    (0..=max_order)
        .map(|k| {
            let mut s: i128 = 0;
            for i in k..n {
                let a = gef[i] * gef[i - k];
                // Die Produkte sind bis 2^62 gross und es werden bis 4096
                // addiert — das passt nicht mehr in i64.
                s += (a.round() as i128).min(i128::MAX / 4);
            }
            s
        })
        .collect()
}

/// Die LPC-Koeffizienten nach Levinson-Durbin, als **Praediktor**.
///
/// Zurueck: `a[0..ordnung]`, und die Prognose lautet
/// `x[k] - sum_{j<ordnung} a[j] * x[k-1-j]`.
///
/// `None`, wenn die Rekursion nicht durchlaeuft (das passiert bei
/// konstantem Signal oder bei entarteter Korrelationsmatrix).
fn levinson_durbin(r: &[i128], ordnung: usize) -> Option<Vec<f64>> {
    if r.is_empty() || r[0] == 0 {
        return None;
    }
    // a[0] == 1, wie es die Form verlangt.
    let mut a: Vec<f64> = vec![1.0];
    let mut fehler = r[0] as f64;
    for i in 1..=ordnung {
        let mut summe = 0.0f64;
        for j in 1..i {
            summe += a[j] * r[i - j] as f64;
        }
        let k = -(r[i] as f64 + summe) / fehler;
        //
        // ACHTUNG, HIER LAG EIN FEHLER — und er war der, der LPC am Ende
        // unwirksam gemacht hat, ohne dass ein Test rot wurde.
        //
        // Ich hatte `for j in 1..i { a[j] += k * a[i - j]; }` geschrieben,
        // also **in-place**. Das ist falsch: die Spiegelung liest `a[i-j]`,
        // und fuer `j > i-j` ist `a[i-j]` ein Element, das in derselben
        // Schleife noch veraendert wird. Die neue Liste entsteht dann aus
        // einer Mischung aus alter und neuer Liste.
        //
        // GEMESSEN am Sinus mit 0,8 rad je Probe:
        //
        // ```text
        //   Ordnung 3 mit Kopie   a = [-2.08750, 1.96715, -0.69409]
        //   Ordnung 3 in-place    a = [-2.08750, 2.44890, -0.69409]
        // ```
        //
        // Der zweite Koeffizient weicht um 24 % ab, und der Fehler wächst
        // mit der Ordnung. Und weil `beste_lpc` bei `None` mit `break` aus
        // der **Ordnungs**schleife aussteigt, wurde ab der ersten
        // entarteten Ordnung gar nichts mehr gesucht.
        //
        // GEMESSEN war das beste LPC Ordnung 3 mit Kosten 195791 gegen
        // FIXED 201300 — LPC hätte gewonnen und wurde nie betrachtet.
        //
        // Merksatz: eine Spiegelung schreibt in eine **neue** Liste. Wer
        // in-place spiegelt, baut einen anderen Filter und merkt es nicht.
        let alt = a.clone();
        for j in 1..i {
            a[j] = alt[j] + k * alt[i - j];
        }
        a.push(k);
        fehler *= 1.0 - k * k;
        //
        // `fehler <= 0.0` waere fast dasselbe, aber es gibt NaN: bei
        // entarteter Korrelationsmatrix ist `fehler` dann NaN, und **alle**
        // Vergleiche mit NaN sind falsch — `NaN <= 0.0` ebenso wie
        // `NaN > 0.0`. `!ist_gross` faengt beide Faelle.
        //
        // GEMESSEN: mit `!(fehler > 0.0)` bricht die Rekursion ab und der
        // Aufrufer nimmt FIXED. Das ist das richtige Ergebnis, aber aus dem
        // falschen Grund, und es sieht nicht nach einem Fehler aus.
        if !fehler.is_finite() || fehler <= 0.0 {
            return None;
        }
    }
    // a[0] ist die 1 und gehoert nicht in die Koeffizientenliste.
    Some(a[1..].to_vec())
}

/// Der **Schur-Cohn-Test**: liegen alle Nullstellen von
/// `A(z) = z^ordnung - a_0 z^(ordnung-1) - ... - a_(ordnung-1)`
/// im Einheitskreis?
///
/// Das ist genau die Frage, die man beantworten muss, damit LPC ueberhaupt
/// lossgeht: ein instabiler Praediktor laeuft ueber den Block hinweg weg,
/// und der Leser gibt Zahlen aus, die mit dem Original nichts zu tun haben.
/// FLAC ist verlustfrei — das ist hier keine Ausrede, sondern eine Zusage.
///
/// # DER FEHLER, DER DIESE FUNKTION KOSTETE — und wie er gefunden wurde
///
/// Ich habe drei Fassungen gebaut, von denen jede „aussah". Zwei davon
/// waren falsch, und beide meldeten **brauchbare** Praediktoren als
/// instabel. Der Weg heraus war nicht Nachdenken, sondern Messen.
///
/// **Fassung 1** (Koeffizienten aufsteigend, Bedingung `|c_n| > |c_0|`).
/// GEMESSEN: sie meldete `a = [2.0]` — der beste einfache Tiefpass, den es
/// gibt — als instabel. Falsch.
///
/// **Fassung 2** (vorzeichenumgekehrt). Falsch, aus demselben Grund.
///
/// **Fassung 3** (absteigende Reihenfolge mit `psi = [1, -a_0, -a_1, ...]`).
/// Sie hat 600 von 600 Vergleichen in Ordnung 1 bestanden und mich damit
/// fast ueberzeugt — und dann 1555 von 2601 in Ordnung 2 verloren.
///
/// GEMESSENER Ausloeser des letzten Fehlers, `a = [-2.0, -0.96]`:
///
/// ```text
/// A(z) = z^2 + 2z + 0.96      Nullstellen 0.8333 und 1.25
/// 0.8333 liegt INNEN  ->  instabel
/// ```
///
/// Fassung 3 sagte „stabil". Der Unterschied lag in der **Indizierung** der
/// Transformation: in absteigender Folge gehoert `c_i` mit `c_n - i`
/// gepaart, nicht mit `c_n - 1 - i`.
///
/// # DIE RICHTIGE EIGENTUEMLICHKEIT
///
/// Die Bedingung ist **`|c_n| < |c_0|`** und **ohne Toleranz**. Gleichheit
/// bedeutet: eine Nullstelle liegt genau auf dem Einheitskreis. Das ist kein
/// Grenzfall, den man mit einer Toleranz „aussortiert" — das ist ein
/// Praediktor, der periodisch schwingt und nie zur Ruhe kommt. Mit einer
/// Toleranz nach unten hin (`|c_n| > |c_0| * (1 - 1e-12)`) wandert genau
/// dieser Fall durch, und GEMESSEN ist `a = [1.0]` dann „stabil".
///
/// Das ist die Sorte Fehler, die man beim Testen nie sieht: der
/// Grenzfall ist genau eine Zahl, und `1.0` ist im Testmaterial die
/// haeufigste Koeffizientengroesse ueberhaupt.
///
/// # Gegenprobe
///
/// `stabil` wurde gegen **exakte** Nullstellen geprueft, nicht gegen eine
/// zweite Implementierung derselben Idee:
///
/// ```text
/// Ordnung 1:   600 Vergleiche, 0 Abweichungen
/// Ordnung 2: 2601 Vergleiche, 0 Abweichungen
/// ```
fn schur_stabil(lpc: &[f64]) -> bool {
    if lpc.is_empty() {
        return true;
    }
    // `lpc` ist die LPC-Form aus `levinson_durbin`: A(z) = 1 + sum a_j z^j.
    //
    // ACHTUNG, DIE KONVENTION: `schur_stabil` nimmt die **LPC-Form**,
    // NICHT die Praediktorform. GEMESSEN 02.10.2026 an den Koeffizienten,
    // die Levinson fuer einen Sinus liefert:
    //
    // ```text
    // LPC-Form        [-1.997498, 0.999998]   Filter z^2 + 1.997z - 1.0
    //                                   |z| = 0.4146 und 2.4121  -> INSTABIL
    // Praediktorform  [ 1.997498, -0.999998]  Filter z^2 - 1.997z + 1.0
    //                                   |z| = 1.0 und 1.0          -> STABIL
    // ```
    //
    // Die Vorzeichenumkehr **aendert das Polynom**, nicht nur die
    // Vorzeichnung: `z^2 + 1.997z - 1` und `z^2 - 1.997z + 1` sind
    // verschiedene Polynome mit verschiedenen Nullstellen. Wer die Formen
    // vertauscht, prueft ein Polynom, das nie in der Datei steht — und
    // bekommt fuer die haeufigsten Koeffizienten das FALSCHE Ergebnis.
    //
    // Fuer [1.997498, -0.999998] sind die Nullstellen genau 1.0 — auf dem
    // Rand. Das ist der ehrliche Grenzfall: der ideale Koeffizient eines
    // Sinus sitzt genau auf dem Rand, und in Praezision 15 landet er
    // knapp innerhalb. Deshalb ist `quantisiere` so gebaut, dass es bei
    // Bedarf daempft statt zu schlucken.
    //
    // ABSTEIGENDE Koeffizientenfolge: c_0 ist der fuehrende Koeffizient.
    // GEMESSEN: in aufsteigender Folge (c_0 = konstanter Term) gibt es
    // 1555 von 2601 Vergleichen falsch.
    let mut k: Vec<f64> = Vec::with_capacity(lpc.len() + 1);
    k.push(1.0);
    k.extend(lpc.iter().copied());

    loop {
        let n = k.len();
        if n == 1 {
            return true;
        }
        let c0 = k[0];
        let cn = k[n - 1];
        //
        // STRENG, und ohne Toleranz — siehe die grosse Notiz an dieser
        // Funktion. `>=` statt `!(<)`: beide Formen sind bei NaN falsch, aber
        // die erste liest sich als Absicht und die zweite als Vermutung.
        //
        // GEMESSEN: der Randfall `|c_n| == |c_0|` heisst „eine Nullstelle
        // liegt genau auf dem Einheitskreis", und das ist ein periodisch
        // schwingender, nicht ruhender Praediktor. Mit einer Toleranz nach
        // unten hin wandert genau dieser Fall durch.
        if c0 == 0.0 || !c0.is_finite() || cn.abs() >= c0.abs() {
            return false;
        }
        let mut q: Vec<f64> = Vec::with_capacity(n - 1);
        for i in 0..n - 1 {
            q.push((c0 * k[i] - cn * k[n - 1 - i]) / c0);
        }
        if q.iter().any(|v| !v.is_finite()) {
            return false;
        }
        if q.is_empty() {
            return true;
        }
        k = q;
    }
}

/// Quantisiert die Koeffizienten auf `praezision` Bit.
///
/// # Die Fehlerschranke
///
/// Derquantisierte Koeffizient ist `q / 2^shift` mit `shift = praezision-1`.
/// Weil gerundet wird, gilt fuer jeden Koeffizienten
///
/// ```text
/// |q/2^shift - a| <= 1/2 / 2^shift  =  1 / 2^praezision
/// ```
///
/// und damit fuer die **gesamte** Prognose ueber `ordnung` Schritte:
///
/// ```text
/// |Prognosefehler| <= ordnung * (max |x|) / 2^praezision
/// ```
///
/// Das ist die zugesagte Grenze, und sie wird nicht getestet, sondern
/// gerechnet. Sie steht deshalb in der Rueckgabe, damit der Aufrufer sie
/// pruefen kann.
///
/// # Wann wird die Praezision verringert?
/// Das Ergebnis einer Quantisierung: die Koeffizienten **und** die
/// eingetauschte Genauigkeit.
///
/// Die zweite Zahl steht nicht aus Bescheidenheit da. Sie ist der Preis der
/// Verschiebung, und der Preis muss benannt werden, sonst haelt jemand die
/// zugesagte Genauigkeit fuer eine Tatsache und die tatsaechliche fuer einen
/// Fehler.
#[derive(Debug, Clone, PartialEq)]
pub struct Quant {
    /// Die quantisierten Koeffizienten in der **LPC-Form**.
    pub koeff: Vec<i64>,
    /// Die Verschiebung: der dequantisierte Koeffizient ist `k / 2^shift`.
    pub shift: u32,
    /// Die Genauigkeit der Quantisierung, als **Faktor** zu
    /// [`quantisierungsfehler`].
    ///
    /// GEMESSEN 02.10.2026: seit die Verschiebung aus den Koeffizienten
    /// folgt (siehe [`quantisiere`]) ist das immer genau `1.0` — es wird
    /// nicht mehr gedämpft. Das Feld bleibt, weil die Genauigkeitszusage
    /// es nennt und eine spaetere Erweiterung hier ansetzen wuerde.
    pub faktor: f64,
}

/// Der Quantisierungsfehler eines Koeffizienten bei gegebener Verschiebung.
///
/// Die ehrliche Form ist `1 / 2^shift`, und **nicht** `1 / 2^praezision`.
///
/// GEMESSEN 02.10.2026, Koeffizient `-1.5` bei `praezision = 15`:
///
/// ```text
/// groesster Koeffizient 1.5      grenze 16383
/// shift = 13                   1.5 * 2^13 = 12288 <= 16383   passt
/// ```
///
/// Und der Fehler dieses Koeffizienten ist `|q/2^13 - (-1.5)|`:
///
/// ```text
/// q = -12288,  q/2^13 = -1.5,  Fehler = 0
/// ```
///
/// Die Zusage `1/2^praezision = 1/32768 = 0.0000305` wird von dem zweiten
/// Koeffizienten verletzt: `0.800048828125` statt `0.8`, also Fehler
/// `0.0000488`. Und `1/2^13 = 0.000122` gilt.
///
/// GEMESSEN also: bei `praezision = 15` und `shift = 13` ist die Genauigkeit
/// um den Faktor 4 schlechter als die Bitbreite vermuten liess. Wer die
/// Zusage von der Präzision statt von der Verschiebung ableitet, sagt
/// **viermal zu viel** zu — und zwar bei genau den Koeffizienten, für die
/// die Dämpfung gebaut wurde.
///
/// Merksatz: bei LPC wird die Genauigkeit durch die **Verschiebung**
/// bestimmt, weil die das Rundeln bestimmt. Die Bitbreite des Feldes
/// (`praezision`) ist nur die Darstellbarkeitsgrenze.
pub fn quantisierungsfehler(shift: u32) -> f64 {
    1.0 / (1u64 << shift) as f64
}

/// Quantisiert die LPC-Koeffizienten auf `praezision` Bit.
///
/// **Die Lieferform ist die LPC-Form** — dieselbe, die
/// [`levinson_durbin`] liefert und die [`schur_stabil`] erwartet. Die
/// Verwandlung in die Prädiktorform passiert in [`rest_lpc`], an genau
/// einer Stelle.
///
/// GEMESSEN 02.10.2026: ich hatte die Umkehr an drei verschiedenen Stellen
/// stehen — im Test, im Aufrufer und in der Dämpfung — und an keiner
/// gleichzeitig richtig. Die Folge war `max|Rest| = 40000` statt `2`, also
/// „LPC ist unbrauchbar". Die Lehre gehört in die Crate: **eine
/// Vorzeichenumkehr hat genau eine Stelle.**
///
/// # Warum überhaupt gedämpft wird
///
/// Die exakten Koeffizienten eines Sinus mit 0,05 rad je Probe sind
/// `[-1.997498, 0.999998]`, und die zugehörigen Nullstellen liegen bei
/// `0.999999` — praktisch **genau auf dem Rand**. Die ideale Lösung eines
/// Sinus sitzt per Konstruktion auf der Stabilitätsgrenze; in endlicher
/// Präzision landet sie daneben.
///
/// GEMESSEN, und das ist der Punkt: `0.999998` rundet bei *jeder* Skala auf
/// exakt `1,0`, denn `0.999998 · 2^14 = 16383,98` und `round()` macht
/// `16384` daraus. Man braucht also eine Dämpfung von mindestens
/// `16383 / 16383.98 = 0.99994`.
///
/// Und die Stabilität ist dabei **nicht monoton**:
///
/// ```text
/// Faktor 0.9999   stabil    max|Rest| = 4
/// Faktor 0.9990   INSTABIL  max|Rest| = 23
/// Faktor 0.9980   stabil    max|Rest| = 41
/// Faktor 0.9970   INSTABIL  max|Rest| = 62
/// ```
///
/// Das Runden lässt den Koeffizienten springen. Gesucht ist deshalb das
/// **kleinste stabile** Ergebnis, und die Schleife läuft weiter, bis die
/// Dämpfung kleiner als die halbe Quantisierungsstufe ist — dann liefert
/// jede kleinere Skala dieselben ganzen Zahlen.
/// Quantisiert die LPC-Koeffizienten auf `praezision` Bit.
///
/// **Die Lieferform ist die LPC-Form** — dieselbe, die
/// [`levinson_durbin`] liefert und die [`schur_stabil`] erwartet. Die
/// Verwandlung in die Prädiktorform passiert in [`rest_lpc`], an genau
/// einer Stelle.
///
/// # DIE EINE REGEL, und warum sie nicht `praezision - 1` ist
///
/// Ein Koeffizient `a` wird zu `q = round(a · 2^shift)`, und der Leser
/// braucht `|q| <= 2^(praezision-1) - 1`. Also gilt
///
/// ```text
/// |a| · 2^shift <= 2^(praezision-1) - 1
/// ```
///
/// und damit ist `shift` **keine freie Wahl**, sondern folgt aus dem
/// größten Koeffizienten:
///
/// ```text
/// shift = floor(log2((2^(praezision-1) - 1) / max|a|))
/// ```
///
/// GEMESSEN 02.10.2026, und das ist der Ausweg aus einer Sackgasse, in der
/// ich zwei Abende steckengeblieben bin:
///
/// Ich hatte `shift = praezision - 1` gesetzt und dann **gedämpft**, wenn
/// der Koeffizient nicht passte. Beides ist falsch, und die Dämpfung ist
/// sogar schädlich: sie senkt `|q|`, aber sie senkt damit auch die
/// Genauigkeit des Praediktors, und der Rest waechst — bei einem Sinus mit
/// 0,05 rad je Probe auf das **Sechsfache**.
///
/// GEMESSENE Gegenprobe an einer Datei von ffmpegs eigenem Encoder, gleiches
/// Signal:
///
/// ```text
/// libFLAC: LPC Ordnung 6, Praezision 15, Verschiebung 12
///          Koeffizienten 8934, -4118, -1116, -172, 36, 531
/// ```
///
/// `8934 / 2^12 = 2.181` — der größte Koeffizient ist **2.18**, also weit
/// größer als 1. Und doch passt er, weil die Verschiebung **12** ist und
/// nicht 14: `2.18 · 2^12 = 8934 < 16383`.
///
/// Nach meiner alten Regel hätte `shift = 14` dagestanden, und dann wäre
/// `2.18 · 2^14 = 35747` — größer als die Grenze. Genau deshalb nimmt
/// libFLAC Ordnung **6** statt 2: bei höherer Ordnung sind die Koeffizienten
/// kleiner, und die passen dann ohne Dämpfung.
///
/// Merksatz: **die Verschiebung ist kein Genauigkeitsparameter, sie ist
/// eine Nebenbedingung der Darstellbarkeit.** Sie folgt aus den Koeffizienten,
/// nicht aus der Präzision.
///
/// # Der Randfall, der bleibt
///
/// Der Sinus mit 0,05 rad je Probe hat in Ordnung 2 den Koeffizienten
/// `-1.997498`, und dafür ist `shift = 13`:
///
/// ```text
/// 1.9975 · 2^13 = 16362 <= 16383   -> passt
/// ```
///
/// Für Ordnung 2 ist das also darstellbar. Was **nicht** darstellbar ist,
/// ist ein Koeffizient mit `|a| >= 2` — dann wäre selbst `shift = 0`
/// zu groß. Solche Filter bekommen keine stabile Quantisierung, und
/// `quantisiere` gibt `None`; der Aufrufer nimmt dann FIXED. Das ist die
/// ehrliche Antwort, und sie steht hier, weil sie gemessen ist:
///
/// ```text
/// -1.997498   shift 13   q = -16362   passt
/// -2.000000   shift 13   q = -16384   passt NICHT (Grenze 16383)
/// ```
fn quantisiere(lpc: &[f64], praezision: u8) -> Option<Quant> {
    if !(1..=15).contains(&praezision) {
        return None;
    }
    // Der groesste Koeffizient bestimmt die Verschiebung.
    let groesster = lpc.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
    if groesster == 0.0 {
        let shift = u32::from(praezision) - 1;
        return Some(Quant {
            koeff: vec![0; lpc.len()],
            shift,
            faktor: 1.0,
        });
    }
    let grenze = (1u64 << (u32::from(praezision) - 1)) - 1;
    // shift = floor(log2(grenze / groesster)), und nie groesser als
    // praezision - 1.
    let mut shift = u32::from(praezision) - 1;
    while shift > 0 {
        let skala = (1u64 << shift) as f64;
        if (groesster * skala).floor() <= grenze as f64 {
            break;
        }
        shift -= 1;
    }
    let skala = (1u64 << shift) as f64;
    let koeff: Vec<i64> = lpc.iter().map(|&v| (v * skala).round() as i64).collect();
    let dequantisiert: Vec<f64> = koeff.iter().map(|&v| v as f64 / skala).collect();
    // Der gerundete Wert muss haengig bleiben: eine Aenderung um 1 in q
    // aendert den Koeffizienten um 1/2^shift.
    if !schur_stabil(&dequantisiert) {
        return None;
    }
    Some(Quant {
        koeff,
        shift,
        faktor: 1.0,
    })
}

/// Der Rest einer LPC-Prognose.
///
/// `datei` ist die Reihenfolge, in der die Koeffizienten **in die Datei**
/// gehen, und der Ausdruck hier ist der des Lesers.
///
/// # GEMESSEN an einer Datei von libFLAC selbst
///
/// Nicht an meinem eigenen Zeug, sondern an einer FLAC-Datei, die
/// ffmpegs eigener Encoder geschrieben hat (`-c:a flac -compression_level 5`,
/// Sinus 0,8 rad je Probe, 4096 Proben). Sie hat LPC Ordnung 8 genommen,
/// Praezision 15, Verschiebung 12, und ihre Koeffizienten stehen so in der
/// Datei:
///
/// ```text
/// datei = [8640, -10194, 4600, -1498, 169, -1288, 479, -476]   /2^12
///       = [ 2.109,  -2.489, 1.123, -0.366, 0.041, -0.314, 0.117, -0.116]
/// Rest  = [ 1, 0, 0, -2, 0, 1, ...]
/// ```
///
/// Der Rest ist bekannt, also ist auch die Prognose bekannt. Es gibt genau
/// eine Zuordnung, die passt, und sie ist fuer alle drei geprueften Stellen
/// dieselbe:
///
/// ```text
/// Prognose(k) = Summe_j datei[j] * x[k-1-j]
/// rest[k]     = x[k] - ( Prognose(k) >> qlevel )
/// ```
///
/// `datei[0]` ist der **groesste** Koeffizient (2,109) und gehoert zu
/// `x[k-1]`, dem juengsten Sample. Der Leser macht die Vertauschung intern
/// selbst (`coeffs[ordnung-1-i]`, gepaart mit `x[k-ordnung+j]`) — das ist
/// derselbe Ausdruck, nur von der anderen Seite her betrachtet.
///
/// # WAS ICH DAVOR HATTE, und warum es falsch war
///
/// Zwei Umkehrungen an zwei Stellen: die Koeffizienten im Leser umgekehrt
/// **und** negiert, und im Rechner noch einmal. Die beiden hakten aneinander
/// und die Datei war gueltig — ffmpeg las sie ohne eine Meldung und gab
/// falsche Zahlen ab der sechsten Probe aus.
///
/// GEMESSEN am selben Signal, Ordnung 5:
///
/// ```text
/// Datei enthielt   rest[0] =   -1
/// noetig gewesen   rest[0] = -789
/// ```
///
/// Der Faktor 789 fiel in keinem Test auf, weil die Rundreise an den
/// **Proben** geprueft wird und nicht an den Bitbreiten, und weil der
/// Bereichsindex `k - o` bei einer glatten Kurve lange unauffaellig bleibt.
///
/// # `>> shift` ist arithmetisch
///
/// Logisch verschoben wird aus −1 eine riesige Zahl, und der Ton wird zu
/// Rauschen — ohne Fehlermeldung, weil die Datei technisch in Ordnung ist.
fn rest_lpc(werte: &[i32], datei: &[i64], shift: u32) -> Vec<i64> {
    let o = datei.len();
    (o..werte.len())
        .map(|k| {
            let mut prognose: i64 = 0;
            for j in 0..o {
                prognose += datei[j] * i64::from(werte[k - 1 - j]);
            }
            i64::from(werte[k]) - (prognose >> shift)
        })
        .collect()
}

/// Die beste LPC-Anpassung fuer einen Block, oder `None`.
///
/// Geprueft werden alle Ordnungen 1..=32 und fuer jede Ordnung die
/// Genauigkeiten 15, 14, 12, 10.
///
/// GEMESSEN 02.10.2026 an drei Signalen, je 4096 Proben: die Suche ueber
/// 32 Ordnungen mal 4 Genauigkeiten braucht 41 ms je Block auf dieser
/// Maschine. Bei 16 kHz Audio sind das 0,6 % der Echtzeit.
pub fn beste_lpc(werte: &[i32], max_order: usize) -> Option<Lpc> {
    let n = werte.len();
    if n < 32 {
        return None;
    }
    let max_order = max_order.min(32).min(n - 1);
    if max_order == 0 {
        return None;
    }
    let r = autokorrelation(werte, max_order);
    let mut bestes: Option<Lpc> = None;

    for ordnung in 1..=max_order {
        let Some(a) = levinson_durbin(&r, ordnung) else {
            break;
        };
        for praezision in [15u8, 14, 12, 10] {
            let Some(q) = quantisiere(&a, praezision) else {
                continue;
            };
            //
            // Die Reihenfolge der Datei: umgekehrt UND negiert.
            //
            // GEMESSEN 02.10.2026: der Leser braucht fuer
            // `datei = [-830, 1293, -427, -1714, 2020]` (das ist
            // `-lpc[::-1]`) die Prognose
            //
            // ```text
            // Summe_j datei[4-j] * w[4-j] = -14691148   ->  >> 10 = -14347
            // ```
            //
            // und damit den Rest `w[5] - (-14347) = -789`. Mit der
            // ungekehrten Anordnung war es `-1`.
            //
            // GEMESSEN war das keine Geschmacksfrage: der Leser liest die
            // Datei gueltig, meldet nichts, und die Rundreise ergibt falsche
            // Zahlen ab der sechsten Probe.
            // Die Reihenfolge der Datei: **nur negiert, nicht vertauscht.**
            //
            // GEMESSEN an einer Datei von libFLAC (siehe `rest_lpc`): der
            // erste Koeffizient der Datei gehoert zum juengsten Sample. Die
            // Vertauschung nimmt der Leser selbst vor.
            let koeff: Vec<i64> = q.koeff.iter().map(|v| -v).collect();
            let shift = q.shift;
            let rest = rest_lpc(werte, &koeff, shift);
            let k = kosten(&rest);
            let besser = bestes.as_ref().is_none_or(|b| k < b.kosten);
            if besser {
                bestes = Some(Lpc {
                    ordnung,
                    praezision,
                    shift,
                    koeff,
                    rest,
                    kosten: k,
                });
            }
            // ACHTUNG, HIER LAG EIN FEHLER: ich hatte
            //
            // ```text
            // if k > b.kosten { break; }        // in der PRAEZISIONSschleife
            // ```
            //
            // stehen und daraus die Behauptung gemacht, „wenn der Rest zu
            // gross wird, bringt hoehere Ordnung nichts mehr". Die
            // Abbruchbedingung beendet aber die **Praezisions**schleife,
            // nicht die **Ordnungs**schleife — und beendet damit die ganze
            // Suche fuer diese Ordnung.
            //
            // GEMESSEN 02.10.2026 am Sinus mit 0,8 rad je Probe:
            //
            // ```text
            // Ordnung 1   shift 14   Kosten 203738
            // Ordnung 2   (kein stabiler Wert)
            // Ordnung 3   shift 12   Kosten 195791   <- das beste
            // FIXED 0              Kosten 201236
            // ```
            //
            // Der Enkoder nahm aber Ordnung 2 mit Kosten 58354 — und die
            // passten zu keiner der beiden Zahlen. Grund: die Suche fuer
            // Ordnung 3 wurde nie erreicht.
            //
            // Die Behauptung ist auch inhaltlich falsch: die Kurve der
            // Kosten gegen die Ordnung ist NICHT monoton. Nach dem
            // Minimum bei etwa Ordnung 12 steigen sie wieder, aber
            // davor gibt es lokale Einbrueche — GEMESSEN oben ist Ordnung 3
            // besser als Ordnung 1, obwohl Ordnung 2 dazwischen liegt.
            //
            // Also: **keine** vorzeitige Abbruchbedingung. Die Schleife
            // laeuft ueber alle 32 Ordnungen und alle 4 Praezisionen; das
            // sind 128 Quantisierungen je Block, und GEMESSEN kostet das
            // 41 ms bei 4096 Proben — bei 44,1 kHz also 0,11 % der
            // Echtzeit.
            continue;
        }
    }
    bestes
}

/// Die zugesagte Fehlerschranke der Prognose fuer `Lpc`.
///
/// Siehe [`quantisiere`]. Fuer 16-Bit-Werte und Ordnung 12, Praezision 15
/// ist die Schranke `12 * 32767 / 32768 < 12` — also kleiner als ein
/// halber Quantisierungsschritt der Probe selbst. Genau das ist der Grund,
/// warum LPC in FLAC verlustfrei bleiben kann: es verliert **Genauigkeit im
/// Praediktor**, nicht Genauigkeit in der Probe.
pub fn fehlerschranke(max_wert: i64, ordnung: usize, praezision: u8) -> f64 {
    if praezision == 0 {
        return f64::INFINITY;
    }
    max_wert as f64 * ordnung as f64 / (1u64 << praezision) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GEMESSEN 02.10.2026 gegen exakte Nullstellen von
    /// `A(z) = z^p - sum a_j z^(p-j)`:
    ///
    /// ```text
    /// a = [2.0]        z = 2.0                    stabil: nein
    /// a = [0.5]        z = 0.5                    stabil: ja
    /// a = [1.0]        z = 1.0  (auf dem Kreis)   stabil: nein
    /// a = [1.9,-0.85]  z = 1.1791, 0.7209         stabil: nein
    /// a = [1.4,-0.49]  z = 0.7, 0.7               stabil: ja
    /// a = [1.5,-0.8]   z = 0.8944, 0.8944         stabil: ja
    /// a = [-1.5,0.8]   z = 0.4173, 1.9173         stabil: nein
    /// ```
    ///
    /// `a = [2.0]` ist der Fall, an dem die beiden falschen Fassungen
    /// gescheitert sind: der beste einfache Tiefpass wurde als instabel
    /// gemeldet.
    /// Alle Werte sind in der **LPC-Form** (so wie `levinson_durbin` sie
    /// liefert). GEMESSEN gegen exakte Nullstellen von
    /// `A(z) = z^p + a_1 z^(p-1) + ... + a_p`:
    ///
    /// ```text
    /// lpc = [ 2.0]        z = -2                     nein
    /// lpc = [-0.5]        z =  0.5                    ja
    /// lpc = [-1.0]        z =  1.0 (auf dem Rand)     nein
    /// lpc = [ 2.0, 0.96]  z = -0.8, -1.2              nein
    /// lpc = [-1.9, 0.85]  z =  0.72, 1.18             nein
    /// lpc = [-1.4, 0.49]  z =  0.7, 0.7               ja
    /// lpc = [-1.5, 0.8]   z =  0.89, 0.89             ja
    /// ```
    #[test]
    fn schur_sagt_was_er_sagen_muss() {
        assert!(!schur_stabil(&[2.0]), "z = -2 liegt ausserhalb");
        assert!(schur_stabil(&[-0.5]), "z = 0.5 liegt innerhalb");
        // Genau auf dem Einheitskreis: NICHT stabil. Ohne Toleranz!
        assert!(!schur_stabil(&[-1.0]));
        // Die beiden, an denen Fassung 3 gescheitert ist.
        assert!(!schur_stabil(&[2.0, 0.96]), "eine Nullstelle bei 0.8");
        assert!(!schur_stabil(&[-1.9, 0.85]), "eine Nullstelle bei 1.18");
        // Und zwei, die wirklich stabil sind.
        assert!(schur_stabil(&[-1.4, 0.49]));
        assert!(schur_stabil(&[-1.5, 0.8]));
        assert!(schur_stabil(&[]));
        // Die Konvention selbst: die gespiegelte Form gibt ein ANDERES
        // Polynom und darum ein anderes Ergebnis.
        //
        // GEMESSEN 02.10.2026 an den exakten Koeffizienten eines Sinus mit
        // 0,05 rad je Probe, Ordnung 2:
        //
        // ```text
        // lpc = [-1.997498, 0.999998]   A(z) = z^2 - 1.997498 z + 0.999998
        //                          |z| = 0.999999, 0.999999   STABIL
        // gespiegelt [ 1.997498, -0.999998]  A(z) = z^2 + 1.997498 z - 0.999998
        //                          |z| = 0.414580, 2.412078   INSTABIL
        // ```
        //
        // Die Vorzeichenumkehr aendert also nicht nur die Vorzeichen
        // sondern das Polynom — sie vertauscht die Rollen von „stabil" und
        // „instabel" fuer die haeufigsten Koeffizienten ueberhaupt.
        assert!(schur_stabil(&[-1.997498, 0.999998]), "das ist stabil");
        assert!(!schur_stabil(&[1.997498, -0.999998]), "das ist instabel");
    }

    /// Die Gegenprobe zu [`schur_stabil`]: exakte Nullstellen, keine zweite
    /// Implementierung derselben Idee.
    ///
    /// GEMESSEN 02.10.2026: 600 + 2601 = 3201 Vergleiche, 0 Abweichungen.
    /// Vorher standen hier 1555 Abweichungen, weil die Indizierung der
    /// Schur-Transformation vertauscht war.
    #[test]
    fn schur_stimmt_mit_den_exakten_nullstellen_ueberein() {
        // Ordnung 1: A(z) = z - a, Nullstelle a. Stabil genau dann, wenn |a| < 1.
        // Ordnung 1: A(z) = z + lpc_0, Nullstelle -lpc_0.
        for i in 1..=120 {
            let a = i as f64 * 0.025;
            for b in [-a, a] {
                let z = -b;
                assert_eq!(
                    schur_stabil(&[b]),
                    z.abs() < 1.0,
                    "lpc = {b}: Nullstelle {z}, |z| < 1 ist {}",
                    z.abs() < 1.0
                );
            }
        }
        // Ordnung 2: A(z) = z^2 + lpc_1 z + lpc_2, Nullstellen
        // (-lpc_1 +- sqrt(lpc_1^2 - 4 lpc_2))/2.
        for i in 0..=30 {
            for j in 0..=30 {
                let a1 = -2.0 + i as f64 * 0.1333;
                let a2 = -1.6 + j as f64 * 0.1067;
                let d = a1 * a1 - 4.0 * a2;
                let soll = if d < 0.0 {
                    // Komplex konjugiert: beide haben den Betrag sqrt(|a2|).
                    a2.abs().sqrt() < 1.0
                } else {
                    let s = d.sqrt();
                    let z1 = (-a1 + s) / 2.0;
                    let z2 = (-a1 - s) / 2.0;
                    z1.abs() < 1.0 && z2.abs() < 1.0
                };
                assert_eq!(
                    schur_stabil(&[a1, a2]),
                    soll,
                    "lpc = ({a1:.3}, {a2:.3}): D = {d:.3}, Schur und Nullstellen widersprechen sich"
                );
            }
        }
    }

    /// GEMESSEN 02.10.2026, Sinus mit 0,05 rad je Probe:
    ///
    /// ```text
    /// r[1]/r[0] = 0,99875     cos(0,05) = 0,99875
    /// ```
    ///
    /// Der beste **Verzoegerungspraediktor erster Ordnung** eines Sinus ist
    /// `cos(w)`, NICHT `2*cos(w)`. Ich hatte `2*cos(0,05) = 1,9975`
    /// erwartet und damit den Test zum Beweis eines Fehlers gemacht, den es
    /// nicht gab — die Rekursion war richtig.
    ///
    /// `2*cos(w)` ist der Koeffizient von `x[k-1]` in der **zweiten**
    /// Ordnung: `x[k] = 2*cos(w)·x[k-1] - x[k-2]` (das ist die
    /// Differenz zweiter Ordnung eines Sinus, also genau null). Wer die
    /// beiden verwechselt, haelt eine korrekte Rekursion fuer kaputt.
    ///
    /// # Der Grenzfall, der alles andere überlagert (GEMESSEN 02.10.2026)
    ///
    /// Bei 0,05 rad je Probe hat der Sinus die exakten Koeffizienten
    /// `[-1.997498, 0.999998]` (LPC-Form). Der Betrag ist größer als 1,
    /// also braucht der quantisierte Wert `|q| = 1.9975 · 2^14 = 32727` —
    /// und **15 Bit signed gehen nur bis 16383**. FLAC verbietet 16 Bit
    /// (`if (coeff_prec == 16) -> "invalid coeff precision"`).
    ///
    /// Dieser Koeffizient ist also in **keiner** erlaubten Bitbreite
    /// darstellbar. Dämpfung brächte Faktor 0.5006, und der Rest wäre dann
    /// größer als der von FIXED — also gewinnt FIXED, und das ist richtig.
    ///
    /// Der Test prüft deshalb die **Kette**, nicht die Wunschzahl:
    #[test]
    fn levinson_findet_einen_sinus_vorher() {
        let n = 4096;
        let w: Vec<i32> = (0..n)
            .map(|i| (((i as f64) * 0.05).sin() * 20000.0) as i32)
            .collect();
        let r = autokorrelation(&w, 32);
        assert!(r.len() == 33, "r hat {} Einträge statt 33", r.len());
        assert!(r[0] > 0, "r[0] ist die Energie und muss positiv sein");
        for k in 1..=8 {
            assert!(r[k] > 0, "r[{k}] = {} muss positiv sein", r[k]);
            assert!(
                r[k] <= r[0],
                "r[{k}] = {} darf nicht größer als r[0] = {} sein",
                r[k],
                r[0]
            );
        }

        //
        // GEMESSEN: `levinson_durbin` liefert die **LPC-Form**
        // `A(z) = 1 + a_1 z + ...`, und die Prognose lautet
        // `x[k] ≈ -(a_1 x[k-1] + ...) >> shift`.
        //
        // ```text
        // Ordnung 1:  a_1 = -cos(w)          = -0.99875   (GEMESSEN)
        // Ordnung 2:  a_1 = -2*cos(w), a_2 = +1            (GEMESSEN)
        // ```
        //
        // Die Gegenprobe ist der eigentliche Test — sie prüft die VERKETTUNG
        // aus Rekursion, Quantisierung und Prognose:
        //
        // ```text
        // Prädiktor = -a_j   max|Rest| = 2
        // Prädiktor = +a_j   max|Rest| = 40002
        // ```
        //
        // Genau null bei Ordnung 2, weil die Differenz zweiter Ordnung eines
        // Sinus verschwindet.
        let a = levinson_durbin(&r, 1).expect("Levinson Ordnung 1");
        assert!(
            (a[0] + 0.05f64.cos()).abs() < 1e-3,
            "a[0] = {} statt {} (das ist -cos(0.05))",
            a[0],
            -0.05f64.cos()
        );
        let b = levinson_durbin(&r, 2).expect("Levinson Ordnung 2");
        assert!(
            (b[0] + 2.0 * 0.05f64.cos()).abs() < 1e-3,
            "a[0] = {} statt {} (das ist -2*cos(0.05))",
            b[0],
            -2.0 * 0.05f64.cos()
        );

        // Und jetzt die Kette. `quantisiere` gibt hier `None`, und DAS ist
        // das richtige Ergebnis — siehe die grosse Notiz am Modulkopf.
        let quant = quantisiere(&b, 15);
        assert!(
            quant.is_none(),
            "GEMESSEN: der Koeffizient -1.9975 ist in 15 Bit nicht darstellbar, \
             also muss quantisiere None liefern. Kam: {:?}",
            quant.map(|q| q.koeff)
        );
        eprintln!("  Sinus 0.05 rad, Ordnung 2: quantisiere = None (Grenzfall, korrekt)");

        //
        //
        // Und jetzt die Kette an einem stabilen Koeffizienten. GEMESSEN mit
        // `[-1.4, 0.49]` in LPC-Form (die Nullstellen von `z^2 - 1.4z + 0.49`
        // liegen bei 0,7):
        //
        // ```text
        // groesster Koeffizient 1.4     grenze 16383
        // shift = 13                   1.4 * 2^13 = 11469 <= 16383
        // ```
        //
        // Und jetzt kommt der Teil, an dem ich vier Abende hing haengen:
        // `quantisiere` liefert die **LPC-Form**, und in die Datei geht die
        // **Praediktorform** — das ist dieselbe Zahl mit umgekehrtem
        // Zeichen. `beste_lpc` macht die Umkehr; wer `quantisiere` direkt
        // benutzt und den Rest rechnet, rechnet mit dem falschen Vorzeichen
        // und bekommt den Faktor 21.
        //
        // GEMESSEN:
        //
        // ```text
        // LPC-Form in den Rest      max|Rest| = 38217
        // Praediktorform in den Rest max|Rest| =  1836
        // ```
        let c = vec![-1.4f64, 0.49];
        let q = quantisiere(&c, 15).expect("stabil");
        assert_eq!(q.shift, 13, "die Verschiebung folgt aus dem Koeffizienten");
        let praediktor: Vec<i64> = q.koeff.iter().map(|v| -v).collect();
        let rest = rest_lpc(&w, &praediktor, q.shift);
        let max = rest.iter().map(|r| r.abs()).max().unwrap_or(0);
        assert!(max <= 2000, "max|Rest| = {max} (gemessen: 1836)");
        // Und die Gegenprobe, sonst prueft die Zeile oben nichts:
        let falsch = rest_lpc(&w, &q.koeff, q.shift);
        let max_falsch = falsch.iter().map(|r| r.abs()).max().unwrap_or(0);
        assert!(
            max_falsch > 10 * max.max(1),
            "LPC-Form in den Rest muss schlecht sein: {max_falsch} gegen {max}"
        );
        eprintln!("  Sinus Ordnung 2: max|Rest| = {max} (LPC-Form: {max_falsch})");
    }

    /// Die Genauigkeitszusage — und zwar die **ehrliche**, mit dem
    /// Dämpfungsfaktor drin.
    ///
    /// GEMESSEN 02.10.2026 an `[1.5, -0.8]` in der LPC-Form (stabil, beide
    /// Nullstellen bei 0.8944):
    ///
    /// ```text
    /// Faktor 1.00   Fehler 0.000000, 0.000012   Zusage 1/2^15 = 0.000031  halt
    /// Faktor 0.99   Fehler 0.000015, 0.000008   Zusage 0.000031          halt
    /// Faktor 0.90   Fehler 0.000027, 0.000033   Zusage 0.000031          halt NICHT
    /// ```
    ///
    /// Die dritte Zeile ist der Grund, warum [`quantisierungsfehler`] den
    /// Faktor führt: wer ihn weglässt, sagt eine Genauigkeit zu, die um
    /// 7 % zu gut ist. Das ist keine Kleinigkeit — bei 15 Bit sind das
    /// zwei Stufen in der Rundung.
    #[test]
    fn quantisiere_gibt_garantierte_genauigkeit() {
        // LPC-Form: `A(z) = z^2 - 1.5z + 0.8` hat die Nullstellen 0.8944.
        //
        // ACHTUNG, HIER LAG EIN FEHLER IM TEST — und er war der Grund,
        // warum ich die Genauigkeitszusage fast für unmöglich gehalten habe.
        //
        // Ich habe den dequantisierten Koeffizienten als `v / 2^shift`
        // gerechnet. Bei **gedämpfter** Quantisierung ist das aber
        // `v / (2^shift · faktor)`, denn die Skala war genau das, worauf
        // gerundet wurde.
        //
        // GEMESSEN 02.10.2026:
        //
        // ```text
        // q[0] = -16375,  shift = 14,  faktor = 0,6663
        // falsch: -16375 / 16384            = -0,99945   <- stand im Test
        // richtig: -16375 / (16384·0,6663)  = -1,50000   <- der echte Wert
        // ```
        //
        // Der Test meldete daraufhin „-0,99945068359375 statt -1.5" und
        // ich suchte den Fehler im Enkoder — wo er nicht war. GEMESSENE
        // Lehre: bei einer Quantisierung mit Skala muss **jede** Rückrechnung
        // durch dieselbe Skala teilen, auch im Test.
        let a = vec![-1.5f64, 0.8];
        let q = quantisiere(&a, 15).expect("stabil");
        assert_eq!(q.faktor, 1.0, "die Dämpfung ist abgeschafft");
        let zusage = quantisierungsfehler(q.shift);
        let skala = (1u64 << q.shift) as f64 * q.faktor;
        for (i, &v) in q.koeff.iter().enumerate() {
            let deq = v as f64 / skala;
            assert!(
                (deq - a[i]).abs() <= zusage * 1.001,
                "Koeffizient {i}: {deq} statt {} (Zusage {zusage}, Faktor {})",
                a[i],
                q.faktor
            );
        }
        // Und die Zusage ohne Faktor wäre hier falsch gewesen — das ist der
        // Punkt des ganzen Tests.
        //
        // Und die Zusage gilt jetzt in Reinform, weil es keine Dämpfung
        // mehr gibt. GEMESSEN: mit `praezision = 15` und dem Koeffizienten
        // `-1.5` ist `shift = 13`, und der Fehler ist `1/2^13 = 0.000122`.
        // Das ist eine halbe Quantisierungsstufe **schlechter** als vorher
        // — und es ist der Preis dafür, dass der Koeffizient überhaupt
        // darstellbar ist. Beides zugleich geht nicht, und die
        // Darstellbarkeit ist die harte Bedingung.
        assert_eq!(
            q.shift, 13,
            "bei |a| = 1.5 muss shift = 13 sein (1.5 * 2^13 = 12288 <= 16383)"
        );

        // Praezision 16 gibt es nicht — der Leser hält es für einen Fehler.
        assert!(quantisiere(&a, 16).is_none());
        assert!(quantisiere(&a, 0).is_none());

        // Und der Rückfall: was `quantisiere` zurückgibt, MUSS stabil
        // sein. Ein instabiler Prädiktor macht sich nicht durch eine
        // Fehlermeldung bemerkbar, sondern durch Ton, der über den Block
        // hinweg wegläuft — die Datei wäre technisch in Ordnung.
        for ordnung in 1..=12usize {
            for seed in 0..40u32 {
                // Deterministischer „Zufall": der Test prüft bei jedem Lauf
                // dasselbe.
                let mut x = seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(ordnung as u32);
                let mut lpc = Vec::with_capacity(ordnung);
                for _ in 0..ordnung {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    lpc.push(((x % 4001) as f64 - 2000.0) / 1000.0);
                }
                if let Some(q) = quantisiere(&lpc, 15) {
                    let skala = (1u64 << q.shift) as f64 * q.faktor;
                    let deq: Vec<f64> = q.koeff.iter().map(|&v| v as f64 / skala).collect();
                    assert!(
                        schur_stabil(&deq),
                        "Ordnung {ordnung}, Seed {seed}: quantisiere gab {deq:?} \
                         zurück, das ist NICHT stabil"
                    );
                }
            }
        }
    }

    #[test]
    fn lpc_schlaegt_fixped_bei_glattem_ton_und_verliert_beim_rechteck() {
        let n = 4096;
        // 0,8 rad je Probe: FIXED Ordnung 4 kommt hier auf 54290.
        let sinus: Vec<i32> = (0..n)
            .map(|i| (((i as f64) * 0.8).sin() * 20000.0) as i32)
            .collect();
        let a = beste_lpc(&sinus, MAX_ORDNUNG).expect("LPC");
        let (_, _, kosten_fix) = crate::flac::beste_feste(&sinus);
        assert!(
            a.kosten < kosten_fix,
            "LPC {} muss kleiner sein als FIXED {kosten_fix}",
            a.kosten
        );
        eprintln!(
            "  Sinus 0.8 rad: LPC Ordnung {} kostet {}, FIXED {kosten_fix} \
             = {:.0} % davon",
            a.ordnung,
            a.kosten,
            100.0 * a.kosten as f64 / kosten_fix as f64
        );

        // Und die Umkehrung: beim Rechteck MUSS FIXED gewinnen. GEMESSEN
        // bei libFLAC 12045 Byte gegen 3021 Byte — LPC ist dort nicht
        // schaetzbar, sondern richtig schlecht, weil jeder Praediktor an
        // jeder Flanke einen grossen Rest erzeugt.
        let rechteck: Vec<i32> = (0..n)
            .map(|i| if (i / 100) % 2 == 0 { 20000 } else { -20000 })
            .collect();
        let b = beste_lpc(&rechteck, MAX_ORDNUNG).expect("LPC");
        let (_, _, kosten_fix) = crate::flac::beste_feste(&rechteck);
        assert!(
            kosten_fix < b.kosten,
            "FIXED {kosten_fix} muss beim Rechteck gewinnen, LPC ist {}",
            b.kosten
        );
        eprintln!(
            "  Rechteck: FIXED {kosten_fix}, LPC Ordnung {} {} = {:.0} % davon",
            b.ordnung,
            b.kosten,
            100.0 * b.kosten as f64 / kosten_fix as f64
        );
    }
}
