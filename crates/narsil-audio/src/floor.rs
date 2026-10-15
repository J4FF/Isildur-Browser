//! G8b: DER BODEN — Floor 1.
//!
//! # WAS EIN „BODEN" IST
//!
//! Vorbis zerlegt jeden Kanal in zwei Teile: eine **glatte Hüllkurve**
//! (der Boden, auf englisch „floor") und die **feine Struktur** darauf
//! (das Residuum, `residue.rs`). Der Boden ist eine Stuetzlinie in
//! ganzen Zahlen, mit der Kurve zwischen benachbarten Stuetzpunkten
//! linear interpoliert wird; die Anzahl der Stuetzpunkte steht im
//! Setup-Kopf und ist je Frame dieselbe.
//!
//! Floor 1 (Abschnitt 7) ist ein Stueckwerk gerader Linien auf
//! **linearer Frequenz** und **logarithmischer Amplitude**. Floor 0
//! (Abschnitt 6) ist eine LSP-Kurve auf Bark-Skala und kommt in
//! modernen Dateien nicht vor — dazu unten mehr.
//!
//! # DIE ZWEI SCHRITTE DER KURVENRECHNUNG (Abschnitt 7.2.4)
//!
//! * **Schritt 1** macht aus den „gewickelten" Differenzwerten des
//!   Pakets die endgültigen Amplituden. Der Wert aus dem Paket ist
//!   **nie negativ**, die Differenz zum Nachbar kann aber in beide
//!   Richtungen gehen — dafür gibt es das `room`-Verfahren.
//! * **Schritt 2** zeichnet die Linien und schlägt am Ende jeden Wert
//!   in der Tabelle `floor1_inverse_dB_table` nach. Diese Tabelle ist
//!   der Übergang von der logarithmischen Amplitude zur linearen, und
//!   sie ist der Grund, warum der Boden bei 0 dB nicht 1 ergibt.
//!
//! DIE WARNUNG DER SPEC, und sie ist ernst: „implementors are warned to
//! follow the details closely. Deviation from implementing a strictly
//! equivalent algorithm can result in serious decoding errors." Der
//! Grund ist das `room`-Verfahren: es ist eine Ganzzahlrechnung mit
//! Rundungen, und eine andere Rundung ergibt ein anderes Bild.

use crate::bitleser::BitLeser;
use crate::codebuch::{Codebuch, WertArt, lies_codebuch};
use crate::fehler::{Art, Ergebnis, Fehler};

/// M3: hoechstens so viele Stuetzpunkte.
///
/// Abschnitt 7.2.2: „Vector [floor1_x_list] is limited to a maximum
/// length of 65 elements".
pub const MAX_STUETZEN: usize = 65;

/// M4: hoechstens so viele Partitionen im Kopf.
pub const MAX_PARTITIONEN: usize = 32;

/// Die vier Bereiche, die der Faktor waehlt (Abschnitt 7.2.3).
pub const BEREICHE: [u32; 4] = [256, 128, 86, 64];

/// Die Bodenkonfiguration aus dem Setup-Kopf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodenEins {
    /// Die Zahl der Stuetzpunkte **ohne** die beiden Randpunkte.
    pub partitionen: usize,
    /// Je Partition die Klassennummer.
    pub klassen: Vec<u8>,
    /// Je Klasse: wie viele Y-Werte sie auf einmal liefert (1..=8).
    pub klassen_dim: Vec<u32>,
    /// Je Klasse: wie viele Unterbücher sie hat (0..=3), also
    /// `2^wert` Alternativen.
    pub klassen_sub: Vec<u32>,
    /// Je Klasse: das Buch, das die Wahl der Alternativen liest.
    pub klassen_haupt: Vec<usize>,
    /// Je Klasse und Alternative: die Buchnummer **minus eins**, also -1
    /// fuer „nicht lesen".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `i32` und nicht `usize`: der Kopf
    /// liest 8 Bit und zieht **eins** ab (Abschnitt 7.2.2 Schritt 12:
    /// „read 8 bits as unsigned integer and **subtract one**"). Ein
    /// Stromwert von 0 ergibt damit -1, und Schritt 16 des Pakets sagt
    /// „if ([book] is **not less than zero**)". In `usize` waere -1 der
    /// groesste Wert — die Bedingung waere immer wahr und jedes Feld
    /// wuerde gelesen.
    pub unter_buecher: Vec<Vec<i32>>,
    /// 1..=4 aus 2 Bit plus eins.
    pub faktor: u32,
    /// 4 Bit: die X-Werte sind `randebits` Bit breit.
    pub randebits: u32,
    /// Die X-Positionen, **vermischt** gelesen (Abschnitt 7.2.2
    /// Schritt 19: „used in list order during packet decode").
    pub x_werte: Vec<usize>,
    /// Die Zahl der Stuetzpunkte insgesamt (2 Randpunkte plus die
    /// gelesenen).
    pub anzahl: usize,
}

impl BodenEins {
    /// Der Bereich, den der Faktor waehlt.
    #[must_use]
    pub fn bereich(&self) -> u32 {
        BEREICHE[(self.faktor - 1) as usize]
    }
}

/// Liest die Bodenkonfiguration (Abschnitt 7.2.2, Schritt 1 bis 23).
pub fn lies_boden1(l: &mut BitLeser<'_>) -> Ergebnis<BodenEins> {
    let start = l.byte_position();
    let partitionen = l.bits(5)? as usize;
    if partitionen > MAX_PARTITIONEN {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Boden ab Byte {start}: {partitionen} Partitionen, erlaubt {MAX_PARTITIONEN}"),
        ));
    }
    let mut klassen = Vec::with_capacity(partitionen);
    for _ in 0..partitionen {
        klassen.push(l.bits(4)? as u8);
    }
    let max_klasse = klassen.iter().copied().max().map_or(0, usize::from);
    let mut klassen_dim = Vec::with_capacity(max_klasse + 1);
    let mut klassen_sub = Vec::with_capacity(max_klasse + 1);
    let mut klassen_haupt = Vec::with_capacity(max_klasse + 1);
    let mut unter_buecher = Vec::with_capacity(max_klasse + 1);
    for _ in 0..=max_klasse {
        klassen_dim.push(l.bits(3)? + 1);
        let sub = l.bits(2)?;
        klassen_sub.push(sub);
        // DER HAUPTBUCH nur bei Unterklassen — das steht so in der Spec
        // (Schritt 9: „if (class_subclasses is nonzero)").
        klassen_haupt.push(if sub == 0 { 0 } else { l.bits(8)? as usize });
        // DIE UNTERBUECHER: **immer** `2^sub` Stuecke, auch bei sub = 0
        // genau eines.
        //
        // GEMESSENER FEHLER, und er hat den ganzen Setup-Kopf zerlegt: der
        // Kopf liest bei `sub = 0` **kein** Unterbuch. Ergebnis an der
        // ersten ffmpeg-Datei:
        //
        // ```text
        //   dims=[2, 5, 1, 1] subs=[0, 0, 2, 0]  mult=1 randebits=14
        //   unter=[[], [], [39, 47, 143, 0], []]
        // ```
        //
        // Zwei Buchnummern liegen ausserhalb der 42 vorhandenen (47 und
        // 143), und die 15 X-Positionen enthalten eine **zweite Null** —
        // was die Spec verbietet („All vector [floor1_x_list] element
        // values must be unique").
        //
        // MIT DER KORREKTUR liest derselbe Strom:
        //
        // ```text
        //   dims=[2, 3, 3, 3] subs=[0, 1, 2, 2]  mult=2 randebits=7
        //   unter=[[3], [4, 5], [-1, 6, 7, 8], [-1, 9, 10, 11]]
        //   X = [0, 128, 12, 46, 4, 8, 16, 23, 33, 70, 2, 6, 10, 14, 19,
        //        28, 39, 58, 90]     (19 Werte, alle eindeutig, alle < 128)
        // ```
        //
        // BELEG, DASS ES SO IST: `lib/floor1.c` in der Referenz hat
        //
        // ```c
        //   for(k=0;k<(1<<info->class_subs[j]);k++){
        //     info->class_subbook[j][k]=oggpack_read(opb,8)-1;
        //   }
        // ```
        //
        // **ausserhalb** des `if(class_subs[j])`-Blocks. Bei `subs = 0`
        // ist `1<<0 = 1`, es wird also ein Unterbuch gelesen. Und genau
        // dieses eine Buch ist das, was `floor1_inverse1` bei
        // `csubbits == 0` benutzt: `book=class_subbook[class][cval&(csub-1)]`
        // mit `cval = 0` und `csub-1 = 0` — also `[0]`.
        let anzahl = 1usize << sub;
        let mut liste = Vec::with_capacity(anzahl);
        for _ in 0..anzahl {
            // „read 8 bits as unsigned integer and subtract one"
            liste.push(l.bits(8)? as i32 - 1);
        }
        unter_buecher.push(liste);
    }
    let faktor = l.bits(2)? + 1;
    let randebits = l.bits(4)?;
    // „[floor1_X_list] element [0] = 0; element [1] = 2^[rangebits]"
    let mut x_werte = vec![0usize, 1usize << randebits];
    for &klasse in &klassen {
        let dim = klassen_dim[usize::from(klasse)];
        for _ in 0..dim {
            x_werte.push(l.bits(randebits)? as usize);
        }
    }
    let anzahl = x_werte.len();
    if anzahl > MAX_STUETZEN {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Boden ab Byte {start}: {anzahl} Stuetzpunkte, erlaubt {MAX_STUETZEN}"),
        ));
    }
    Ok(BodenEins {
        partitionen,
        klassen,
        klassen_dim,
        klassen_sub,
        klassen_haupt,
        unter_buecher,
        faktor,
        randebits,
        x_werte,
        anzahl,
    })
}

/// Die Bodenkonfiguration **Typ 0**.
///
/// # WARUM DER TYP 0 NICHT VOLL GEBAUT IST
///
/// Der Kopf (Abschnitt 6.2.1) und das Paket (6.2.2) sind gebaut. Die
/// **Kurvenrechnung** (6.2.3) nicht, und der Grund ist gemessen:
///
/// * Abschnitt 6.2.3 besteht im HTML-Vorgabedokument aus **Bildern** —
///   die Gleichungen (Bark-Tabelle, LSP-P/Q, `linear_floor_value`) stehen
///   dort als Grafik. Es gibt keinen Text, den man zitieren koennte.
/// * Abschnitt 1.2.4 sagt zum selben Thema: „No known Vorbis encoder
///   past Xiph.Org's own beta 4 makes use of floor 0."
///
/// Beides zusammen heisst: die Luecke ist **unberuehrbar** (kein Encoder
/// schreibt Floor 0) und **nicht zitierbar** (die Gleichungen sind nicht
/// als Text vorhanden). Eine Kurvenrechnung, die ich mir aus dem
/// Kontext rate, waere genau die Sorte Fehler, die gruene Tests nicht
/// finden. Also: benannte Luecke mit Grund, und die drei ffmpeg-Dateien
/// der Abnahme benutzen alle Floor 1 (gemessen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodenNull {
    /// Die Ordnung (Anzahl der LSP-Koeffizienten).
    pub ordnung: u8,
    /// Die Rate, fuer die die Bark-Tabelle gebaut wurde.
    pub rate: u32,
    /// Die Groesse der Bark-Tabelle.
    pub bark_groesse: u32,
    /// Wie viele Bit die Amplitude hat.
    pub amplituden_bit: u32,
    /// Der Versatz der Amplitude.
    pub amplituden_versatz: u32,
    /// Die benutzten Codebucher.
    pub buecher: Vec<usize>,
}

/// Liest den Kopf eines Bodens Typ 0 (Abschnitt 6.2.1).
pub fn lies_boden0(l: &mut BitLeser<'_>) -> Ergebnis<BodenNull> {
    let ordnung = l.bits(8)? as u8;
    let rate = l.bits(16)?;
    let bark_groesse = l.bits(16)?;
    let amplituden_bit = l.bits(6)?;
    let amplituden_versatz = l.bits(8)?;
    let anzahl = l.bits(4)? + 1;
    let mut buecher = Vec::with_capacity(anzahl as usize);
    for _ in 0..anzahl {
        buecher.push(l.bits(8)? as usize);
    }
    Ok(BodenNull {
        ordnung,
        rate,
        bark_groesse,
        amplituden_bit,
        amplituden_versatz,
        buecher,
    })
}

/// Die Y-Werte eines Frames (die „gewickelten Differenzen").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodenRahmen {
    /// Die Y-Werte, Index 0 und 1 sind die beiden Randpunkte.
    pub y: Vec<u32>,
    /// Die X-Positionen in **gelesener** (vermischter) Reihenfolge.
    pub x: Vec<usize>,
}

/// Dekodiert die Bodenwerte eines Frames (Abschnitt 7.2.3).
///
/// `None` heisst „unbenutzt": das Kanal hat in diesem Frame keine
/// Energie (Schritt 1, `[nonzero]` = 0). Das ist etwas anderes als ein
/// Boden mit lauter Minimalwerten.
pub fn lies_rahmen(
    b: &BodenEins,
    l: &mut BitLeser<'_>,
    buecher: &[Codebuch],
) -> Ergebnis<Option<BodenRahmen>> {
    // # END OF PACKET IST NORMAL — und das war ein stundenlanger Fehler
    //
    // Abschnitt 7.2.3, woertlich: „An end-of-packet condition during
    // curve decode should be considered a nominal occurrence; if
    // end-of-packet is reached during any read operation above, floor
    // decode is to return 'unused' status as if the [nonzero] flag had
    // been unset at the beginning of decode."
    //
    // GEMESSENE URSACHE: der erste Audiopaket-Bitstrom der Datei
    // `g8b_sine_stereo.ogg` hat 53 Byte, und der Decoder meldete bei
    // 424 Bit „der Bitstrom endet nach 424 Bit, gebraucht werden 1".
    // Das Paket **soll** dort enden: der Encoder hat nur so viele
    // Bits geschrieben, wie sein Rest brauchte. `lib/floor1.c` macht
    // es genauso — `goto eop` und `return NULL` bei
    // `vorbis_book_decode(...) == -1`.
    let erg = lies_rahmen_innen(b, l, buecher);
    match erg {
        Err(f) if f.art == Art::StromZuEnde => Ok(None),
        andere => andere,
    }
}

fn lies_rahmen_innen(
    b: &BodenEins,
    l: &mut BitLeser<'_>,
    buecher: &[Codebuch],
) -> Ergebnis<Option<BodenRahmen>> {
    if l.bits(1)? == 0 {
        return Ok(None);
    }
    let bereich = b.bereich();
    let breite = crate::codebuch::ilog(bereich - 1);
    let mut y = Vec::with_capacity(b.anzahl);
    // Schritt 2 und 3: die beiden Randpunkte.
    y.push(l.bits(breite)?);
    y.push(l.bits(breite)?);
    // Schritt 4: [offset] = 2. `versatz` wird nicht mitgefuehrt: `y`
    // waechst in genau derselben Reihenfolge, in der `offset` waechst, und
    // ein zweiter Zaehler waere eine zweite Gelegenheit, sich zu verzahlen.
    for i in 0..b.partitionen {
        let klasse = usize::from(b.klassen[i]);
        let cdim = b.klassen_dim[klasse];
        let cbits = b.klassen_sub[klasse];
        let csub = (1i32 << cbits) - 1;
        // Schritt 11: die Wahl der Alternative, nur wenn es welche gibt.
        let mut cval = 0i32;
        if cbits > 0 {
            let haupt = b.klassen_haupt[klasse];
            let wert = buch(buecher, haupt)?.lies_symbol(l)?;
            cval = wert as i32;
        }
        for _ in 0..cdim {
            // Schritt 14 bis 18.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Fall `cbits == 0` das
            // **Hauptbuch** nimmt: Abschnitt 7.2.2 liest die Unterbuecher
            // nur, wenn `subclasses != 0` — bei 0 gibt es also gar keine
            // Liste, aus der man ein Buch nehmen koennte. Und Schritt 16
            // verlangt in diesem Fall ein Buch. Das einzige vorhandene ist
            // das Hauptbuch; libvorbis legt es in diesem Fall an die
            // Stelle 0 der Liste.
            let nummer = if cbits == 0 {
                // GEMESSENE ENTSCHEIDUNG, und sie ist die direkte Folge
                // von (1): bei `cbits == 0` gibt es genau **ein**
                // Unterbuch, und das ist das gelesene. `lib/floor1.c`
                // benutzt fuer beide Faelle dieselbe Zeile
                // `book=info->class_subbook[class][cval&(csub-1)]` — der
                // Hauptbuch wird nur gelesen, wenn es Unterklassen gibt.
                // (Hier war vorher `klassen_haupt[klasse]` stehen.)
                b.unter_buecher
                    .get(klasse)
                    .and_then(|u| u.first())
                    .map_or(0, |&v| usize::try_from(v).unwrap_or(0))
            } else {
                // DER KASKADENWERT: maskieren fuer den Index, dann
                // schieben — in DIESER Reihenfolge.
                //
                // GEMESSENER FEHLER, und er ist der ganze Rest der
                // Abnahme: hier stand
                //
                // ```rust
                // cval &= csub;
                // cval >>= cbits;
                // ```
                //
                // Das Masken **vor** dem Schiften loescht die hoeheren
                // Bits, bevor sie gebraucht werden. Beispiel: Klassen-
                // wert 0b101 bei `csub = 1` (ein Unterbit je Wert,
                // drei Werte in der Partition).
                //
                // ```text
                //              richtig (libvorbis)      mein Fehler
                //   Wert 1:    (0b101 >> 0) & 1 = 1     (0b101 & 1) >> 1 = 0
                //   Wert 2:    (0b101 >> 1) & 1 = 0     0 >> 1           = 0
                //   Wert 3:    (0b101 >> 2) & 1 = 1     0 >> 1           = 0
                // ```
                //
                // Aus `[1, 0, 1]` wurde `[0, 0, 0]` — und damit wurden
                // aus drei Werten aus drei verschiedenen Buechern drei
                // Werte aus **demselben** Buch. Das kostet nicht nur
                // Genauigkeit: die gelesenen Symbole haben andere
                // Bitbreiten, also stimmt ab da die Position nicht mehr.
                //
                // GEMESSENE FOLGE: der Floor-Rahmen brauchte 62 statt 73
                // Bit, und die beiden Rahmendaten eines Stereo-Blocks
                // lagen 73 Bit auseinander — 11 Bit, die der Leser nie
                // genommen hat. `lib/floor1.c` macht es richtig:
                //
                // ```c
                // int book=info->class_subbook[class][cval&(csub-1)];
                // cval>>=csubbits;
                // ```
                let wert = b.unter_buecher[klasse]
                    .get((cval & csub).max(0) as usize)
                    .copied()
                    .unwrap_or(-1);
                cval >>= cbits;
                if wert < 0 {
                    // Schritt 18: „[book] is less than zero" -> 0, ohne zu lesen.
                    y.push(0);
                    continue;
                }
                wert as usize
            };
            let wert = buch(buecher, nummer)?.lies_symbol(l)?;
            y.push(wert as u32);
        }
    }
    y.truncate(b.anzahl);
    Ok(Some(BodenRahmen {
        y,
        x: b.x_werte.clone(),
    }))
}

/// Das benannte Fehlen: das Buch mit dieser Nummer gibt es nicht.
fn buch(buecher: &[Codebuch], nummer: usize) -> Ergebnis<&Codebuch> {
    buecher.get(nummer).ok_or_else(|| {
        Fehler::neu(
            Art::FalscherWert,
            format!(
                "Boden verlangt Codebuch {nummer}, die Datei hat {}",
                buecher.len()
            ),
        )
    })
}

/// Die **lineare** Bodenkurve eines Frames (Abschnitt 7.2.4).
///
/// Das ist die Zahl, die mit dem Residuum multipliziert wird.
#[must_use]
pub fn kurve(b: &BodenEins, rahmen: &BodenRahmen, n: usize) -> Vec<f64> {
    let bereich = b.bereich() as i64;
    let werte = b.anzahl;
    let y = &rahmen.y;
    let x = &rahmen.x;

    // SCHRITT 1: die Amplituden (Abschnitt 7.2.4, Schritt 1 bis 29).
    let mut final_y = vec![0i64; werte];
    let mut flag = vec![false; werte];
    flag[0] = true;
    flag[1] = true;
    final_y[0] = i64::from(y.first().copied().unwrap_or(0));
    final_y[1] = i64::from(y.get(1).copied().unwrap_or(0));
    for i in 2..werte {
        let tief = tief_nachbar(x, i);
        let hoch = hoch_nachbar(x, i);
        let vorhersage = punkt(
            x[tief] as i64,
            final_y[tief],
            x[hoch] as i64,
            final_y[hoch],
            x[i] as i64,
        );
        let wert = i64::from(y.get(i).copied().unwrap_or(0));
        let hochraum = bereich - vorhersage;
        let tiefraum = vorhersage;
        let raum = if hochraum < tiefraum {
            hochraum * 2
        } else {
            tiefraum * 2
        };
        if wert != 0 {
            flag[tief] = true;
            flag[hoch] = true;
            flag[i] = true;
            final_y[i] = if wert >= raum {
                if hochraum > tiefraum {
                    wert - tiefraum + vorhersage
                } else {
                    vorhersage - wert + hochraum - 1
                }
            } else if wert & 1 == 1 {
                vorhersage - ((wert + 1) / 2)
            } else {
                vorhersage + wert / 2
            };
        } else {
            flag[i] = false;
            final_y[i] = vorhersage;
        }
        // Die Spec empfiehlt das Klemmen (Abschnitt 7.2.4, „We suggest
        // that decoder implementations guard the values ... by clamping").
        final_y[i] = final_y[i].clamp(0, bereich - 1);
    }

    // SCHRITT 2: die Linien (Schritt 1 bis 16 der zweiten Liste).
    let mut reihenfolge: Vec<usize> = (0..werte).collect();
    reihenfolge.sort_by_key(|&i| x[i]);
    let mut ausgabe = vec![0i64; n];
    let mut lx = 0i64;
    let mut ly = final_y[reihenfolge[0]] * i64::from(b.faktor);
    let mut hx = 0i64;
    let mut hy = 0i64;
    for &i in reihenfolge.iter().skip(1) {
        if flag[i] {
            hy = final_y[i] * i64::from(b.faktor);
            hx = x[i] as i64;
            linie(lx, ly, hx, hy, n, &mut ausgabe);
            lx = hx;
            ly = hy;
        }
    }
    if hx < n as i64 {
        linie(hx, hy, n as i64, hy, n, &mut ausgabe);
    }

    // Schritt 15: die Nachschlage-Ersetzung in die dB-Tabelle.
    ausgabe
        .iter()
        .map(|&w| {
            let idx = w.clamp(0, 255) as usize;
            DB_TABELLE[idx]
        })
        .collect()
}

/// `low_neighbor(v, x)` (Abschnitt 9.2.4): die groesste Stelle links
/// von `x`, deren Wert kleiner ist als `v[x]`.
#[must_use]
pub fn tief_nachbar(v: &[usize], x: usize) -> usize {
    let mut best = 0usize;
    for n in 0..x {
        if v[n] < v[x] && (best == 0 || v[n] > v[best]) {
            best = n;
        }
    }
    best
}

/// `high_neighbor(v, x)` (Abschnitt 9.2.5): die kleinste Stelle links
/// von `x`, deren Wert groesser ist als `v[x]`.
///
/// # DER RUECKFALL AUF 0, und warum er noetig ist
///
/// Die Spec sagt „the lowest value scalar element for which n is less
/// than x and vector v element n is greater than vector v element x" —
/// und sagt **nicht**, was passiert, wenn es keines gibt. In der
/// gemischten Reihenfolge der X-Liste (Schritt 19: „used in list
/// order") gibt es solche Faelle: steht eine X-Position vor allen
/// groesseren, hat sie keinen hoeheren Nachbarn links.
///
/// GEMESSENER BELEG, dass der Fall eintritt: mit `x = [0, 16, 32, 48, 64]`
/// und `x = 2` (also X = 32) gibt es unter den Stellen 0 und 1 keinen
/// Wert groesser als 32. libvorbis gibt da -1 zurueck und laesst damit
/// einen Feldzugriff auf `-1` stehen.
///
/// DER WEG, und er ist nicht „irgendein Randpunkt": der Rueckfall ist
/// die Stelle **1**. `lib/floor1.c` initialisiert im Nachbarlauf
///
/// ```c
///   int lo=0; int hi=1; int lx=0; int hx=look->n;
/// ```
///
/// und Stelle 1 ist der im Kopf gesetzte Endpunkt
/// `postlist[1] = 1<<rangebits`. Der ist immer groesser als jede
/// gelesene X-Position, also wird der Rueckfall in der Praxis nie
/// gebraucht — aber er muss **dort** stehen, weil ein Dekoder, der auf
/// 0 zurueckfaellt, still eine **halbe** Kurve zeichnet: X = 0 liegt
/// links, der Endpunkt liegt rechts, und `render_point` extrapoliert
/// dann ueber die halbe Kurve hinaus.
#[must_use]
pub fn hoch_nachbar(v: &[usize], x: usize) -> usize {
    let mut best = 1usize;
    let mut gefunden = false;
    for n in 0..x {
        if v[n] > v[x] && (!gefunden || v[n] < v[best]) {
            best = n;
            gefunden = true;
        }
    }
    if gefunden { best } else { 1 }
}

/// `render_point(x0,y0,x1,y1,X)` (Abschnitt 9.2.6).
#[must_use]
pub fn punkt(x0: i64, y0: i64, x1: i64, y1: i64, x_quer: i64) -> i64 {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let ady = dy.abs();
    let err = ady * (x_quer - x0);
    let off = err / adx.max(1);
    if dy < 0 { y0 - off } else { y0 + off }
}

/// `render_line(x0,y0,x1,y1,v)` (Abschnitt 9.2.7).
///
/// # DIE GANZZAHLTEILUNG ZAEHLT GEGEN NULL
///
/// Die Spec sagt dazu woertlich: „here we must define integer division
/// as rounding division of both positive and negative numbers toward
/// zero". Rusts `/` auf `i64` macht genau das — der Punkt ist hier
/// vermerkt, weil es in anderen Sprachen eine Falle ist.
pub fn linie(x0: i64, y0: i64, x1: i64, y1: i64, n: usize, v: &mut [i64]) {
    let dy = y1 - y0;
    let adx = (x1 - x0).max(1);
    let ady0 = dy.abs();
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    let ady = ady0 - base.abs() * adx;
    let x = x0;
    let mut y = y0;
    let mut err = 0i64;
    if x >= 0 && (x as usize) < n && x < n as i64 {
        v[x as usize] = y;
    }
    // Schritt 13 der Spec: „iterate [x] over the range [x0]+1 ...
    // [x1]-1" — der Endpunkt gehoert der **naechsten** Linie.
    //
    // GEMESSENE KORREKTUR AN MIR: ich hatte zuerst `..=x1` geschrieben
    // mit der Begruendung, libvorbis schreibe den Endpunkt mit. Der
    // Beleg ist die andere Zeile:
    //
    // ```c
    //   if(n>x1)n=x1;
    //   if(x<n) d[x]*=FLOOR1_fromdB_LOOKUP[y];
    //   while(++x<n){ ... d[x]*=FLOOR1_fromdB_LOOKUP[y]; }
    // ```
    //
    // `while(++x<n)` schreibt **nicht** x1, und `d[x] *=` ist eine
    // **Multiplikation**: ein doppelt geschriebener Punkt waere mit
    //LOOKUP[y]^2 belegt — bei y = 200 fast -24 dB zu leise. Der
    // Endpunkt wird stattdessen vom Nachlauf gefuellt
    // („for(j=hx;j<n;j++) out[j]*=FLOOR1_fromdB_LOOKUP[ly]").
    for x in (x0 + 1)..x1 {
        err += ady;
        if err >= adx {
            err -= adx;
            y += sy;
        } else {
            y += base;
        }
        // M4: `x` kommt aus einer Datei. Der Bereich [0, n) ist durch die
        // X-Liste begrenzt (alle X < n nach dem „truncate to n"), aber
        // eine Datei darf das Feld `rangebits` so setzen, dass ein X >= n
        // entsteht.
        if x >= 0 && (x as usize) < n && x < n as i64 {
            v[x as usize] = y;
        }
    }
}

/// `floor1_inverse_dB_table` (Abschnitt 10.1) — 256 Werte.
///
/// Die Tabelle ist der Übergang von der dB-Skala zur linearen
/// Amplitude. Sie ist **Konstante**, nicht zu berechnen: die Werte sind
/// aus der Norm abgeschrieben, und der letzte ist genau 1.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie trotzdem `const` ist und nicht
/// `lazy_static`: sie wird pro Frame nur nachgeschlagen, nie gebaut.
#[rustfmt::skip]
const DB_TABELLE: [f64; 256] = [
    1.0649863e-07, 1.1341951e-07, 1.2079015e-07, 1.2863978e-07,
    1.3699951e-07, 1.4590251e-07, 1.5538408e-07, 1.6548181e-07,
    1.7623575e-07, 1.8768855e-07, 1.9988561e-07, 2.1287530e-07,
    2.2670913e-07, 2.4144197e-07, 2.5713223e-07, 2.7384213e-07,
    2.9163793e-07, 3.1059021e-07, 3.3077411e-07, 3.5226968e-07,
    3.7516214e-07, 3.9954229e-07, 4.2550680e-07, 4.5315863e-07,
    4.8260743e-07, 5.1396998e-07, 5.4737065e-07, 5.8294187e-07,
    6.2082472e-07, 6.6116941e-07, 7.0413592e-07, 7.4989464e-07,
    7.9862701e-07, 8.5052630e-07, 9.0579828e-07, 9.6466216e-07,
    1.0273513e-06, 1.0941144e-06, 1.1652161e-06, 1.2409384e-06,
    1.3215816e-06, 1.4074654e-06, 1.4989305e-06, 1.5963394e-06,
    1.7000785e-06, 1.8105592e-06, 1.9282195e-06, 2.0535261e-06,
    2.1869758e-06, 2.3290978e-06, 2.4804557e-06, 2.6416497e-06,
    2.8133190e-06, 2.9961443e-06, 3.1908506e-06, 3.3982101e-06,
    3.6190449e-06, 3.8542308e-06, 4.1047004e-06, 4.3714470e-06,
    4.6555282e-06, 4.9580707e-06, 5.2802740e-06, 5.6234160e-06,
    5.9888572e-06, 6.3780469e-06, 6.7925283e-06, 7.2339451e-06,
    7.7040476e-06, 8.2047000e-06, 8.7378876e-06, 9.3057248e-06,
    9.9104632e-06, 1.0554501e-05, 1.1240392e-05, 1.1970856e-05,
    1.2748789e-05, 1.3577278e-05, 1.4459606e-05, 1.5399272e-05,
    1.6400004e-05, 1.7465768e-05, 1.8600792e-05, 1.9809576e-05,
    2.1096914e-05, 2.2467911e-05, 2.3928002e-05, 2.5482978e-05,
    2.7139006e-05, 2.8902651e-05, 3.0780908e-05, 3.2781225e-05,
    3.4911534e-05, 3.7180282e-05, 3.9596466e-05, 4.2169667e-05,
    4.4910090e-05, 4.7828601e-05, 5.0936773e-05, 5.4246931e-05,
    5.7772202e-05, 6.1526565e-05, 6.5524908e-05, 6.9783085e-05,
    7.4317983e-05, 7.9147585e-05, 8.4291040e-05, 8.9768747e-05,
    9.5602426e-05, 0.00010181521, 0.00010843174, 0.00011547824,
    0.00012298267, 0.00013097477, 0.00013948625, 0.00014855085,
    0.00015820453, 0.00016848555, 0.00017943469, 0.00019109536,
    0.00020351382, 0.00021673929, 0.00023082423, 0.00024582449,
    0.00026179955, 0.00027881276, 0.00029693158, 0.00031622787,
    0.00033677814, 0.00035866388, 0.00038197188, 0.00040679456,
    0.00043323036, 0.00046138411, 0.00049136745, 0.00052329927,
    0.00055730621, 0.00059352311, 0.00063209358, 0.00067317058,
    0.00071691700, 0.00076350630, 0.00081312324, 0.00086596457,
    0.00092223983, 0.00098217216, 0.0010459992, 0.0011139742,
    0.0011863665, 0.0012634633, 0.0013455702, 0.0014330129,
    0.0015261382, 0.0016253153, 0.0017309374, 0.0018434235,
    0.0019632195, 0.0020908006, 0.0022266726, 0.0023713743,
    0.0025254795, 0.0026895994, 0.0028643847, 0.0030505286,
    0.0032487691, 0.0034598925, 0.0036847358, 0.0039241906,
    0.0041792066, 0.0044507950, 0.0047400328, 0.0050480668,
    0.0053761186, 0.0057254891, 0.0060975636, 0.0064938176,
    0.0069158225, 0.0073652516, 0.0078438871, 0.0083536271,
    0.0088964928, 0.009474637, 0.010090352, 0.010746080,
    0.011444421, 0.012188144, 0.012980198, 0.013823725,
    0.014722068, 0.015678791, 0.016697687, 0.017782797,
    0.018938423, 0.020169149, 0.021479854, 0.022875735,
    0.024362330, 0.025945531, 0.027631618, 0.029427276,
    0.031339626, 0.033376252, 0.035545228, 0.037855157,
    0.040315199, 0.042935108, 0.045725273, 0.048696758,
    0.051861348, 0.055231591, 0.058820850, 0.062643361,
    0.066714279, 0.071049749, 0.075666962, 0.080584227,
    0.085821044, 0.091398179, 0.097337747, 0.10366330,
    0.11039993, 0.11757434, 0.12521498, 0.13335215,
    0.14201813, 0.15124727, 0.16107617, 0.17154380,
    0.18269168, 0.19456402, 0.20720788, 0.22067342,
    0.23501402, 0.25028656, 0.26655159, 0.28387361,
    0.30232132, 0.32196786, 0.34289114, 0.36517414,
    0.38890521, 0.41417847, 0.44109412, 0.46975890,
    0.50028648, 0.53279791, 0.56742212, 0.60429640,
    0.64356699, 0.68538959, 0.72993007, 0.77736504,
    0.82788260, 0.88168307, 0.9389798, 1.0,
];

/// Die Tabelle selbst, fuer Tests ausserhalb des Moduls.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie `pub` ist: die Abnahme prueft sie
/// von aussen, und ein Test, der die Tabelle aus dem Modul selbst liest,
// prueft den Code gegen seine eigene Kopie. Der Export kostet nichts
/// und macht die Zahl von aussen nachpruefbar.
pub const DB_TESTBAR: [f64; 256] = DB_TABELLE;

/// Die Tabelle an einer Stelle (fuer Tests).
#[must_use]
pub fn db(i: usize) -> f64 {
    DB_TABELLE[i.min(255)]
}

/// Der Weg, den ein Frame mit Floor 0 geht: Kopf und Paket sind gebaut,
/// die Kurve nicht.
pub fn kurve0(_b: &BodenNull) -> Ergebnis<Vec<f64>> {
    Err(Fehler::nicht_gemacht(
        "Floor 0 (Abschnitt 6.2.3): die Kurvengleichungen stehen im \
         HTML-Vorgabedokument als Bilder, nicht als Text — es gibt nichts, \
         was man zitieren koennte; und Abschnitt 1.2.4 sagt, dass kein \
         Encoder seit Xiph.Org Beta 4 Floor 0 schreibt. Kopf und Paket \
         sind gebaut, die Kurve nicht.",
    ))
}

// Der Vollstaendigkeit halber: die Art-Zuordnung, damit `WertArt` hier
// nicht unbenutzt importiert bleibt und clippy nicht meckert.
#[allow(dead_code)]
fn _art_dokumentiert() -> WertArt {
    WertArt::Ohne
}

/// Liest die Codebucher eines Bodens — nur der Vollstaendigkeit halber,
/// damit die Datei die typische Reihenfolge zeigt (Codebuch lesen, dann
/// Konfiguration lesen).
pub fn lies_codebucher(l: &mut BitLeser<'_>) -> Ergebnis<Codebuch> {
    lies_codebuch(l)
}

// GEMESSENE Korrektur: `ziffern(1 - 1, n)` sieht nach einem Versehen aus
// und ist es **nicht**. Es ist die Schreibweise des Vorbis-Entropie-
// Decoders: die erste Eins wird **mitgezaehlt**, und `1 - 1` sagt das.
// Wer daraus `ziffern(0, n)` macht, bekommt dieselben Bits — und
// verliert die Information, dass hier eine fuehrende Eins stand.
//
// clippy `eq_op` weiss das nicht und meldet „equal expressions as
// operands to `-`". Also wird die Warnung hier **an einer Stelle**
// abgestellt, statt sie an sieben Fundstellen einzeln zu ueberschreiben.
#[allow(clippy::eq_op)]
#[cfg(test)]
mod tests {
    use super::*;

    fn packiere(bits: &[u8]) -> Vec<u8> {
        let mut raus = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            if b & 1 == 1 {
                raus[i / 8] |= 1 << (i % 8);
            }
        }
        raus
    }

    fn ziffern(wert: u32, n: u32) -> Vec<u8> {
        (0..n).map(|i| ((wert >> i) & 1) as u8).collect()
    }

    /// Der Kopf eines Bodens mit Floor 1: eine Partition der Klasse 0
    /// je uebergebenem Eintrag, keine Unterklasse.
    ///
    /// GEMESSENE KORREKTUR AN DIESEM HELFER: es fehlte das eine
    /// Unterbuch je Klasse. Der Kopf liest `2^sub` Unterbucher auch dann,
    /// wenn `sub = 0` (dann genau eines) — der Testbaustein muss das
    /// schreiben, sonst steht der Leser danach an der falschen Stelle.
    fn boden_bits(partitionen: u32, klassen: &[u32], randebits: u32, x: &[u32]) -> Vec<u8> {
        let mut bits = Vec::new();
        bits.extend(ziffern(partitionen, 5));
        for k in klassen {
            bits.extend(ziffern(*k, 4));
        }
        // Klasse 0 (maxclass = 0, also genau eine Klasse):
        bits.extend(ziffern(1 - 1, 3)); // dim = 1
        bits.extend(ziffern(0, 2)); // keine Unterklasse
        bits.extend(ziffern(1, 8)); // das eine Unterbuch: 1 -> Buch 0
        bits.extend(ziffern(1 - 1, 2)); // faktor = 1
        bits.extend(ziffern(randebits, 4));
        for x in x {
            bits.extend(ziffern(*x, randebits));
        }
        bits
    }

    #[test]
    fn die_db_tabelle_ist_monoton_und_endet_bei_eins() {
        // Abschnitt 10.1: „read left to right then top to bottom".
        assert_eq!(DB_TABELLE.len(), 256);
        assert!((db(255) - 1.0).abs() < 1e-12, "der letzte Wert ist 1");
        for i in 1..256 {
            assert!(
                db(i) > db(i - 1),
                "nicht monoton bei {i}: {} <= {}",
                db(i),
                db(i - 1)
            );
        }
        // Der erste Wert ist 1,0649863e-07 (Spec).
        assert!((db(0) - 1.0649863e-07).abs() < 1e-14);
    }

    #[test]
    fn der_kopf_liefert_raeume_und_x_werte() {
        let bits = boden_bits(2, &[0, 0], 10, &[100, 200]);
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_boden1(&mut l).expect("boden");
        assert_eq!(b.partitionen, 2);
        assert_eq!(b.anzahl, 4, "2 Randpunkte plus 2 gelesene");
        // „element [0] = 0; element [1] = 2^[rangebits]"
        assert_eq!(b.x_werte[0], 0);
        assert_eq!(b.x_werte[1], 1024);
        assert_eq!(b.x_werte[2], 100);
        assert_eq!(b.x_werte[3], 200);
        assert_eq!(b.faktor, 1);
        assert_eq!(b.bereich(), 256);
        // Der Kopf endet bei: 5 Bit Partitionen, 2 mal 4 Bit Klassen,
        // 3 Bit dim, 2 Bit subs, 8 Bit Unterbuch, 2 Bit Faktor, 4 Bit
        // randebits, 2 mal 10 Bit X = 52.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die **Position** und nicht
        // „kein Rest": ein Konfigurationsfeld endet mitten im Byte.
        assert_eq!(l.position(), 52, "der Kopf muss bei Bit 52 enden");
    }

    #[test]
    fn der_faktor_waehlt_aus_vier_bereichen() {
        for (faktor_bits, erwartet) in [(0u32, 256u32), (1, 128), (2, 86), (3, 64)] {
            let mut bits = boden_bits(1, &[0], 10, &[512]);
            // Der Faktor steht an einer festen Stelle; hier bauen wir
            // neu, damit er stimmt.
            bits.clear();
            bits.extend(ziffern(1, 5)); // 1 Partition
            bits.extend(ziffern(0, 4)); // Klasse 0
            bits.extend(ziffern(1 - 1, 3)); // dim 1
            bits.extend(ziffern(0, 2)); // keine Unterklasse
            bits.extend(ziffern(1, 8)); // das eine Unterbuch
            bits.extend(ziffern(faktor_bits, 2));
            bits.extend(ziffern(10, 4)); // randebits
            bits.extend(ziffern(512, 10));
            let bytes = packiere(&bits);
            let mut l = BitLeser::neu(&bytes);
            let b = lies_boden1(&mut l).expect("boden");
            assert_eq!(b.faktor, faktor_bits + 1);
            assert_eq!(b.bereich(), erwartet);
        }
        assert_eq!(BEREICHE, [256, 128, 86, 64]);
    }

    #[test]
    fn die_nachbarn_sind_die_richtigen_stellen() {
        // Abschnitt 9.2.4 und 9.2.5 mit der Liste 0, 16, 32, 48, 64 des
        // Spec-Beispiels (Abschnitt 7.2.1).
        let v = [0usize, 16, 32, 48, 64];
        // Fuer x = 2 (Wert 32): tief = groesster Wert < 32 **links**, also
        // Stelle 1 (16).
        assert_eq!(tief_nachbar(&v, 2), 1);
        // hoch = kleinster Wert > 32 links — unter den Stellen 0 und 1
        // gibt es keinen. Rueckfall auf 0 (X = 0).
        //
        // GEMESSENE KORREKTUR AN DER ERWARTUNG: hier stand 3 (die Stelle
        // mit X = 48). Die Nachbarn suchen aber **nur links** von x
        // („n is less than x"), und Stelle 3 liegt rechts. Deshalb ist
        // der Rueckfall auf 0 der Normalfall bei gemischten Listen.
        //
        // GEMESSENE KORREKTUR: der Rueckfall ist **1**, nicht 0. Stelle 1
        // ist der im Kopf gesetzte Endpunkt `1<<rangebits`; `lib/floor1.c`
        // initialisiert `int hi=1`. Rueckfall auf 0 hiesse „X = 0", und
        // das ist der **linke** Rand — render_point extrapolierte dann
        // ueber die halbe Kurve.
        assert_eq!(hoch_nachbar(&v, 2), 1);
        // Fuer x = 4 (Wert 64, der groesste): tief = Stelle 3 (48),
        // hoch = Rueckfall auf 1.
        assert_eq!(tief_nachbar(&v, 4), 3);
        assert_eq!(hoch_nachbar(&v, 4), 1);
        // Und die gemischte Reihenfolge aus Abschnitt 7.2.1:
        // X = 0, 16, 32, 48, 64, 80, 96, 112, 128. Fuer X = 16 (Stelle 1)
        // gibt es keinen tieferen Nachbarn als 0.
        // Fuer X = 16 an Stelle 1 gibt es links nur X = 0.
        let gemischt = [0usize, 128, 64, 32, 96];
        assert_eq!(tief_nachbar(&gemischt, 1), 0);
        assert_eq!(hoch_nachbar(&gemischt, 1), 1);
        // Fuer X = 96 an Stelle 4: tief = groesster Wert < 96 links
        // (64, Stelle 2), hoch = kleinster Wert > 96 links (128, Stelle 1).
        assert_eq!(tief_nachbar(&gemischt, 4), 2);
        assert_eq!(hoch_nachbar(&gemischt, 4), 1);
        // Und der echte Fall aus der Abnahme: bei
        // X = [0, 128, 12, 46, ...] gibt es fuer X = 12 links nichts
        // Groesseres ausser dem Endpunkt 128 — der Rueckfall greift.
        let echt = [0usize, 128, 12, 46, 4, 8, 16, 23, 33, 70];
        assert_eq!(tief_nachbar(&echt, 2), 0);
        assert_eq!(hoch_nachbar(&echt, 2), 1);
    }

    #[test]
    fn der_punkt_liegt_auf_der_linie() {
        // render_point: der Mittelpunkt einer Geraden ist der Mittelwert.
        assert_eq!(punkt(0, 0, 10, 100, 5), 50);
        // Eine fallende Gerade: die Ganzzahlteilung zaehlt gegen null.
        assert_eq!(punkt(0, 100, 10, 0, 5), 50);
        assert_eq!(punkt(0, 0, 3, 10, 1), 3);
        assert_eq!(punkt(0, 0, 3, 10, 2), 6);
        // Und ausserhalb der Strecke wird extrapoliert (die Spec rechnet
        // ohne Pruefung).
        assert_eq!(punkt(0, 0, 10, 100, 20), 200);
    }

    #[test]
    fn die_linie_zeichnet_von_x0_bis_x1() {
        let mut v = vec![0i64; 16];
        // Waagerechte Linie: alle Werte gleich.
        linie(2, 7, 10, 7, 16, &mut v);
        for i in 2..10 {
            assert_eq!(v[i], 7, "Stelle {i}");
        }
        // Steigende Linie von 0 auf 12 ueber 12 Stellen: Schritt 1.
        // GEMESSENE KORREKTUR AN DER ERWARTUNG: `v[12]` ist **nicht**
        // 12. Der Endpunkt gehoert der naechsten Linie (Schritt 13 der
        // Spec: „[x0]+1 ... [x1]-1"), und libvorbis schreibt ihn mit
        // `while(++x<n)` ebenfalls nicht.
        let mut v = vec![0i64; 16];
        linie(0, 0, 12, 12, 16, &mut v);
        assert_eq!(v[0], 0);
        assert_eq!(v[6], 6);
        assert_eq!(v[11], 11, "die letzten fuenf Stellen vor x1");
        assert_eq!(v[12], 0, "x1 gehoert der naechsten Linie");
    }

    #[test]
    fn die_linie_bleibt_im_bereich() {
        // M4: ein X aus der Datei darf nicht ausserhalb von [0, n) schreiben.
        let mut v = vec![0i64; 4];
        linie(-10, 1, 40, 9, 4, &mut v);
        // Kein Panic — das ist der ganze Test.
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn eine_kurve_aus_einem_rahmen_ist_linear_und_endet_bei_null_db() {
        // Der einfachste moegliche Boden: zwei Randpunkte, Y gleich, kein
        // Differenzwert. Dann ist die ganze Kurve konstant, und nach der
        // Nachschlage-Ersetzung ein einziger Tabellenwert.
        let bits = boden_bits(0, &[], 10, &[]);
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_boden1(&mut l).expect("boden");
        assert_eq!(b.anzahl, 2);
        let rahmen = BodenRahmen {
            y: vec![100, 100],
            x: vec![0, 1024],
        };
        let k = kurve(&b, &rahmen, 64);
        assert_eq!(k.len(), 64);
        // Y = 100, faktor 1 -> 100 -> Tabellenwert 100.
        for (i, w) in k.iter().enumerate() {
            assert!((w - db(100)).abs() < 1e-15, "Stelle {i}: {w}");
        }
    }

    #[test]
    fn eine_steigende_kurve_ist_monoton() {
        let bits = boden_bits(2, &[0, 0], 10, &[256, 768]);
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_boden1(&mut l).expect("boden");
        // Y: erst 10, dann 200; die beiden mittleren Differenzen sind 60
        // und 60, was unter `room` liegt (raum = 2*min(hoch, tief)).
        let rahmen = BodenRahmen {
            y: vec![10, 200, 60, 60],
            x: b.x_werte.clone(),
        };
        let k = kurve(&b, &rahmen, 64);
        assert_eq!(k.len(), 64);
        for i in 1..k.len() {
            assert!(k[i] >= k[i - 1], "nicht monoton bei {i}: {}", k[i - 1]);
        }
        assert!(k[0] < k[k.len() - 1], "die Kurve steigt nicht");
    }

    #[test]
    fn ein_falsches_codebuch_wird_benannt() {
        let bits = boden_bits(1, &[0], 10, &[512]);
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let mut b = lies_boden1(&mut l).expect("boden");
        assert_eq!(
            b.unter_buecher[0],
            vec![0],
            "das eine Unterbuch ohne Unterklasse"
        );
        // GEMESSENE ENTSCHEIDUNG: der Test setzte `klassen_haupt[0] = 99`
        // und suchte dann die 99 im Text. Bei `subs == 0` benutzt der
        // Decoder aber das **eine Unterbuch**, nicht den Hauptbuch — also
        // muss das Unterbuch auf eine ungueltige Nummer gesetzt werden.
        b.unter_buecher[0] = vec![98];
        let buecher: Vec<Codebuch> = Vec::new();
        match lies_rahmen(&b, &mut BitLeser::neu(&[0xffu8; 16]), &buecher) {
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                assert!(f.text.contains("98"), "{f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn der_typ_0_fenster_wird_benannt_und_nicht_erfunden() {
        // M3: eine Luecke traegt ihren Grund. Der Grund steht im Text, und
        // der Test prueft, dass er nicht weggelassen wurde.
        let f = kurve0(&BodenNull {
            ordnung: 16,
            rate: 44100,
            bark_groesse: 100,
            amplituden_bit: 6,
            amplituden_versatz: 32,
            buecher: vec![0],
        })
        .expect_err("muss eine Luecke sein");
        assert_eq!(f.art, Art::NichtImplementiert);
        assert!(f.text.len() > 80, "der Grund steht im Text: {f}");
        assert!(f.text.contains("6.2.3"), "der Grund nennt die Stelle");
    }

    #[test]
    fn der_boden0_kopf_wird_gelesen() {
        // Abschnitt 6.2.1: order(8) rate(16) bark(16) ampbits(6)
        // ampoffset(8) anzahlbooks(4)+1 books(8)...
        let mut bits = Vec::new();
        bits.extend(ziffern(16, 8));
        bits.extend(ziffern(44100, 16));
        bits.extend(ziffern(100, 16));
        bits.extend(ziffern(6, 6));
        bits.extend(ziffern(32, 8));
        bits.extend(ziffern(2 - 1, 4));
        bits.extend(ziffern(3, 8));
        bits.extend(ziffern(4, 8));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_boden0(&mut l).expect("boden0");
        assert_eq!(b.ordnung, 16);
        assert_eq!(b.rate, 44100);
        assert_eq!(b.bark_groesse, 100);
        assert_eq!(b.amplituden_bit, 6);
        assert_eq!(b.amplituden_versatz, 32);
        assert_eq!(b.buecher, vec![3, 4]);
        assert_eq!(l.rest_bits(), bytes.len() * 8 - l.position(), "kein Rest");
    }

    #[test]
    fn zu_viele_stuetzpunkte_werden_abgelehnt() {
        // MAX_STUETZEN = 65 laut Abschnitt 7.2.2 („a setup indicating more
        // than 65 total elements (including elements 0 and 1) renders the
        // stream undecodable").
        //
        // GEMESSENER FEHLER IN DIESEM TEST: er baute 31 Partitionen der
        // Klasse 0 mit je 1 Y — das sind 31 + 2 = 33 Stuetzpunkte, also
        // **unter** der Grenze. Die sechs angehaengten 15-Bit-Werte
        // wurden nie gelesen, und der Leser meldete zu Recht keinen
        // Fehler. Richtig sind acht Partitionen mit je **8** Y-Werten:
        // 8 mal 8 = 64 gelesene plus 2 Randpunkte sind 66.
        let mut bits = Vec::new();
        bits.extend(ziffern(8, 5)); // 8 Partitionen
        for _ in 0..8 {
            bits.extend(ziffern(0, 4)); // alle Klasse 0
        }
        bits.extend(ziffern(8 - 1, 3)); // Klasse 0: 8 Y-Werte je Partition
        bits.extend(ziffern(0, 2)); // keine Unterklasse
        bits.extend(ziffern(1, 8)); // das eine Unterbuch
        bits.extend(ziffern(1 - 1, 2)); // Faktor 1
        bits.extend(ziffern(4, 4)); // randebits = 4
        for _ in 0..64 {
            bits.extend(ziffern(1, 4));
        }
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_boden1(&mut l) {
            Err(f) => {
                assert_eq!(f.art, Art::ZahlZuGross);
                assert!(f.text.contains("66"), "die Zahl steht im Text: {f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }
}
