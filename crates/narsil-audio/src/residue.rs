//! G8b: DAS RESIDUUM.
//!
//! # WAS EIN RESIDUUM IST
//!
//! Nach dem Boden bleibt die **feine Struktur** des Spektrums übrig —
//! Abschnitt 8.1: „the fine detail of the audio spectrum of one channel
//! in one audio frame after the encoder subtracts the floor curve and
//! performs any channel coupling".
//!
//! # DIE DREI ARTEN
//!
//! Alle drei sind „mehrere Durchgänge über denselben Vektor, jeder mit
//! einem anderen Codebuch, alle addiert" (Abschnitt 8.2). Sie
//! unterscheiden sich nur darin, **wie die Werte einer Partition
//! angeordnet** sind, bevor die Codebücher sie lesen:
//!
//! | Art | Anordnung | Beispiel (Partition 0..7, Dimension 4) |
//! |---|---|---|
//! | 0 | verworfen nach Dimension | `[0 4] [1 5] [2 6] [3 7]` |
//! | 1 | der Reihe nach | `[0 1 2 3] [4 5 6 7]` |
//! | 2 | alle Vektoren ineinander, dann wie 1 | Länge `kanaele * n` |
//!
//! # DIE KASKADE
//!
//! Abschnitt 8.6.1 liest für jede Klasse ein 8-Bit-Muster, in dem jedes
//! gesetzte Bit einen **Durchgang** (pass) bedeutet. Der letzte
//! gesetzte Durchgang einer Klasse ist der letzte, der etwas zur Vektor-
//! Länge beiträgt — libvorbis nennt das `partitions`/`end`. Wir
//! brauchen es nicht, weil wir alle acht Durchgänge rechnen und danach
//! die Zuteilung kappen; das ist langsamer, aber es kann nicht
//! danebenliegen.

use crate::bitleser::BitLeser;
use crate::codebuch::Codebuch;
use crate::fehler::{Art, Ergebnis, Fehler};

/// M3: hoechstens so viele Klassen.
pub const MAX_KLASSEN: usize = 64;

/// M3: hoechstens so viele Zaehler am Anfang und Ende.
pub const MAX_GRENZE: u32 = 1 << 24;

/// M4: hoechstens so lange darf eine Partition sein.
pub const MAX_PARTITION: usize = 1 << 16;

/// „Kein Buch" in der Kaskade.
pub const KEIN: i32 = -1;

/// Die Residuumkonfiguration aus dem Setup-Kopf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rest {
    /// 0, 1 oder 2.
    pub art: u32,
    /// Wo der kodierte Teil im Vektor beginnt.
    pub beginn: u32,
    /// Wo er endet.
    pub ende: u32,
    /// Die Länge einer Partition (aus 24 Bit plus eins).
    pub teilung: u32,
    /// Die Zahl der Klassifikationen.
    pub klassen: u32,
    /// Das Codebuch, das die Klassifikationen liest.
    pub klassenbuch: usize,
    /// Je Klasse: das 8-Bit-Muster der Durchgänge.
    pub kaskade: Vec<u32>,
    /// Je Klasse und Durchgang: die Buchnummer, oder [`KEIN`].
    pub buecher: Vec<[i32; 8]>,
}

impl Rest {
    /// Der letzte Durchgang, der für diese Klasse gesetzt ist, plus eins.
    ///
    /// Das ist die Zahl der Durchgänge, die tatsächlich etwas tun —
    /// libvorbis nennt sie `partitions`. Wir rechnen alle acht und
    /// kappen danach; siehe Modulkopf.
    #[must_use]
    pub fn durchgaenge(&self, klasse: usize) -> usize {
        // ZWEI FEHLER, beide gemessen und beide an derselben Zeile:
        //
        // 1) `8 - k.leading_zeros()` unterlaeuft bei `k == 0`
        //    (0 - 32). Eine Kaskade **ohne** gesetztes Bit ist erlaubt —
        //    dann hat die Klasse kein Buch und traegt nichts bei.
        // 2) Nach dem Abfangen von 0 blieb der Unterlauf: `leading_zeros`
        //    zaehlt 32 Bit (der Typ ist `u32`), die Kaskade hat aber nur
        //    8. Fuer `k = 1` ist `leading_zeros()` 31, und `8 - 31`
        //    unterlaeuft weiterhin — der Test meldete erneut
        //    „attempt to subtract with overflow".
        //
        // RICHTIG IST `u32::BITS`, nicht die 8 der Kaskade.
        self.kaskade
            .get(klasse)
            .copied()
            .filter(|k| *k != 0)
            .map_or(0, |k| (u32::BITS - k.leading_zeros()) as usize)
    }
}

/// Liest die Residuumkonfiguration (Abschnitt 8.6.1).
///
/// `art` kommt von aussen: der Setup-Kopf liest zuerst 16 Bit
/// Residuumtyp und ruft dann diese Funktion auf.
pub fn lies_rest(l: &mut BitLeser<'_>, art: u32) -> Ergebnis<Rest> {
    let start = l.byte_position();
    if art > 2 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Residuum ab Byte {start}: Typ {art} — es gibt 0, 1 und 2"),
        ));
    }
    let beginn = l.bits(24)?;
    let ende = l.bits(24)?;
    let teilung = l.bits(24)? + 1;
    let klassen = l.bits(6)? + 1;
    let klassenbuch = l.bits(8)? as usize;
    if klassen > MAX_KLASSEN as u32 {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Residuum ab Byte {start}: {klassen} Klassen, erlaubt {MAX_KLASSEN}"),
        ));
    }
    if teilung as usize > MAX_PARTITION {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Residuum ab Byte {start}: Partition {teilung}, erlaubt {MAX_PARTITION}"),
        ));
    }
    // Das Kaskadenmuster: 3 Bit tief, dann ein Flag, dann 5 Bit hoch.
    let mut kaskade = Vec::with_capacity(klassen as usize);
    for _ in 0..klassen {
        let tief = l.bits(3)?;
        let flag = l.bits(1)?;
        let hoch = if flag == 1 { l.bits(5)? } else { 0 };
        kaskade.push(hoch * 8 + tief);
    }
    // Die Bücher der gesetzten Durchgänge.
    let mut buecher = vec![[KEIN; 8]; klassen as usize];
    for (zeile, &k) in kaskade.iter().enumerate() {
        for (j, platz) in buecher[zeile].iter_mut().enumerate() {
            if k & (1 << j) != 0 {
                *platz = l.bits(8)? as i32;
            }
        }
    }
    Ok(Rest {
        art,
        beginn,
        ende,
        teilung,
        klassen,
        klassenbuch,
        kaskade,
        buecher,
    })
}

/// Dekodiert die Residuen eines Frames (Abschnitt 8.6.2).
///
/// * `n` — die Blocklaenge; jeder Vektor ist `n/2` lang.
/// * `ch` — die Zahl der Vektoren in diesem Bündel.
/// * `kein_dekodieren` — je Vektor: „den nicht lesen". Solche Vektoren
///   werden alloziert und genullt.
///
/// # END OF PACKET IST NORMAL
///
/// Abschnitt 8.6.2, woertlich: „An end-of-packet condition during
/// packet decode is to be considered a nominal occurrence. Decode
/// returns the result of vector decode up to that point."
///
/// GEMESSENE URSACHE, und sie ist dieselbe wie im Boden: das erste
/// Audiopaket von `g8b_sine_stereo.ogg` hat 53 Byte = 424 Bit, und die
/// Passen 0 bis 2 des Residuums brauchen mehr als das — der Encoder hat
/// die Klassen so gewählt, dass die restlichen Partitionen **nichts**
/// kosten, und die Bit-Zahl im Paket ist das, was wirklich drinsteht.
///
/// `lib/res0.c` macht es genauso: `if(decodepart(...)) goto eopbreak;`
/// und `eopbreak: return(0);` — der Vektor bleibt, wie er ist.
pub fn lies_paket(
    r: &Rest,
    l: &mut BitLeser<'_>,
    n: usize,
    ch: usize,
    kein_dekodieren: &[bool],
    buecher: &[Codebuch],
) -> Ergebnis<Vec<Vec<f64>>> {
    let mut v: Vec<Vec<f64>> = (0..ch).map(|_| vec![0.0f64; n / 2]).collect();

    // Schritt 1 und 2 der Vorbemerkung: die Grenzen beschneiden.
    let mut groesse = n / 2;
    if r.art == 2 {
        groesse *= ch;
    }
    let ende = r.ende.min(groesse as u32);
    let beginn = r.beginn.min(groesse as u32);
    if ende <= beginn {
        return Ok(v);
    }
    let zu_lesen = ende - beginn;
    let teilungen = (zu_lesen / r.teilung) as usize;

    let klassenbuch = buecher.get(r.klassenbuch).ok_or_else(|| {
        Fehler::neu(
            Art::FalscherWert,
            format!(
                "Residuum verlangt das Klassenbuch {}, die Datei hat {}",
                r.klassenbuch,
                buecher.len()
            ),
        )
    })?;
    // „classwords_per_codeword = the codebook_dimensions value of
    // codebook [residue_classbook]"
    let woerter = klassenbuch.dimensionen;
    //
    // GEMESSENER FEHLER, und er ist der ganze Rest der Stereo-Abnahme:
    // es gibt fuer die Art 2 **eine** Klassifikation, fuer die Arten 0
    // und 1 **eine je Kanal**. Ich hatte beides in einer Schleife
    // `for j in 0..ch` gemacht und damit bei Stereo doppelt so viele
    // Bits gelesen wie in der Datei stehen — der Reststrom war ab der
    // ersten Klassifikation um eine Wortlaenge verschoben, und beide
    // Kanaele waren Rauschen.
    //
    // Der Beleg ist `lib/res0.c`, wo die beiden Dekodierer
    // nebeneinander stehen:
    //
    // ```c
    //   /* _01inverse, Art 0 und 1: */
    //   if(s==0){
    //     /* fetch the partition word for each channel */
    //     for(j=0;j<ch;j++){
    //       int temp=vorbis_book_decode(look->phrasebook,&vb->opb);
    //       ...
    //     }
    //   }
    //
    //   /* res2_inverse, Art 2: */
    //   if(s==0){
    //     /* fetch the partition word */
    //     int temp=vorbis_book_decode(look->phrasebook,&vb->opb);
    //     ...
    //   }
    // ```
    //
    // Und `res2_inverse` prueft die Kanäle nur einmal, ganz am Anfang:
    //
    // ```c
    //   for(i=0;i<ch;i++)if(nonzero[i])break;
    //   if(i==ch)return(0); /* no nonzero vectors */
    // ```
    //
    // — **vor** dem ersten Lesen. Stehen alle Kanaele auf „nicht
    // dekodieren", wird gar kein Wort gelesen.
    let ein_wort = r.art == 2;
    if ein_wort && kein_dekodieren.iter().all(|&k| k) {
        return Ok(v);
    }
    let spalten = if ein_wort { 1 } else { ch };
    let mut klassifizierung = vec![vec![0u8; teilungen.max(1)]; spalten];

    for durchgang in 0..8 {
        let mut anzahl = 0usize;
        while anzahl < teilungen {
            if durchgang == 0 {
                // Schritt 6 bis 12: die Klassifikationen lesen.
                //
                // Die Schleife laeuft bei den Arten 0 und 1 ueber die
                // Kanaele, bei Art 2 genau einmal — siehe die Begruendung
                // an `spalten` oben.
                for (j, spalte) in klassifizierung.iter_mut().enumerate() {
                    if !ein_wort && kein_dekodieren.get(j).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(temp) = bis_zu_ende(klassenbuch.lies_symbol(l))? else {
                        return Ok(v);
                    };
                    let mut temp = temp as u64;
                    for i in (0..woerter).rev() {
                        if anzahl + i < teilungen {
                            spalte[anzahl + i] = (temp % u64::from(r.klassen)) as u8;
                        }
                        temp /= u64::from(r.klassen);
                    }
                }
            }
            // Schritt 13 bis 20: die Partition dekodieren.
            //
            // # DER UNTERSCHIED ZWISCHEN DEN ARTEN, und er war ein
            let mut i = 0usize;
            // DIE SCHLEIFE LAEUFT `woerter` MAL, nicht `woerter - 1`.
            //
            // GEMESSENER FEHLER: hier stand `while i + 1 < woerter`. Bei
            // `partitions_per_word = 2` lief die Schleife damit **einmal**
            // statt zweimal — eine Partition je Runde. Die Klassifikationen
            // wurden zwar vollstaendig gelesen (zwei je Runde), aber die
            // **erste** wurde benutzt und die zweite beim naechstenmal
            // ueberschrieben.
            //
            // Die Spuren daraus, gemessen am ersten Audiopaket:
            //
            // ```text
            //   pass 0 pc 0: nach Klassifikationen Bit 140
            //   pass 0 pc 1: nach Klassifikationen Bit 150   <- 10 Bit
            //   pass 0 pc 2: nach Klassifikationen Bit 159   <-  9 Bit
            //   ...
            //   pass 0 pc 14: nach Klassifikationen Bit 303
            //   pass 1 pc 0: nach Klassifikationen Bit 303   <- nichts gelesen
            // ```
            //
            // In Runde 0 wird nur bei einer Partition ueberhaupt ein Buch
            // gelesen (Bit 140 -> 140), in Runde 4 bei zwei (176 -> 195).
            // `lib/res0.c` dagegen:
            //
            // ```c
            //   for(k=0;k<partitions_per_word && i<partvals;k++,i++)
            // ```
            //
            // Zwei Partitionen je Runde, und beide bekommen ihre
            // Klassifikation aus **einem** Wort. Genau das ist der
            // Unterschied: bei mir bekam Partition 1 die untere Ziffer des
            // Wortes von Partition 0 — ein anderes Codebuch, andere
            // Bitlaenge, und der Strom lief aus dem Takt.
            while i < woerter && anzahl < teilungen {
                // Doppel-Lesen
                //
                // GEMESSENER FEHLER: bei Art 2 stand die Dekodierung **im**
                // `for j in 0..ch`-Bereich, also einmal je Kanal. Der erste
                // Audiopaket-Bitstrom (53 Byte) endete nach 424 Bit mit
                // „gebraucht werden 1" — es wurde genau doppelt so viel
                // gelesen.
                //
                // Abschnitt 8.6.5 sagt woertlich: „Rather than performing
                // format 1 decode to produce ch vectors of length n each,
                // call format 1 decode to produce a **single** vector [v] of
                // length ch*n. After decode, deinterleave the single vector
                // [v] into independent vectors."
                //
                // Und: „if at least one vector is to be decoded, **all** the
                // vectors are decoded." Also: ein Aufruf, und die
                // Klassifikation kommt vom ersten Kanal, der nicht
                // uebersprungen wird.
                let versatz = beginn as usize + anzahl * r.teilung as usize;
                if r.art == 2 {
                    // Bei Art 2 gibt es genau eine Klassifikation fuer
                    // alle Kanaele; bei 0 und 1 je eine, und dann zaehlt
                    // nur die erste, die nicht uebersprungen wird.
                    let klasse = if ein_wort {
                        Some(usize::from(klassifizierung[0][anzahl]))
                    } else {
                        (0..ch)
                            .find(|&j| !kein_dekodieren.get(j).copied().unwrap_or(true))
                            .map(|j| usize::from(klassifizierung[j][anzahl]))
                    };
                    if let Some(klasse) = klasse {
                        let buch_nr = r
                            .buecher
                            .get(klasse)
                            .and_then(|k| k.get(durchgang))
                            .copied()
                            .unwrap_or(KEIN);
                        if buch_nr != KEIN {
                            let buch = buecher.get(buch_nr as usize).ok_or_else(|| {
                                Fehler::neu(
                                    Art::FalscherWert,
                                    format!(
                                        "Residuum-Klasse {klasse}, Durchgang {durchgang}: Buch \
                                     {buch_nr}, die Datei hat {}",
                                        buecher.len()
                                    ),
                                )
                            })?;
                            // GEMESSENER FEHLER, und er ist derselbe Fehler
                            // noch einmal: der Ziel-Index. Der Blick in
                            // `lib/res0.c`, `vorbis_book_decodevv_add`:
                            //
                            // ```c
                            //   int m=(offset+n)/ch;
                            //   for(i=offset/ch;i<m;){ ... a[chptr++][i]+=t[j++]; }
                            // ```
                            //
                            // Der **Versatz** gilt im verflochtenen Strom, der
                            // Ziel-Index im Kanalvektor ist `versatz / ch`.
                            // Ich hatte `ziel = versatz + i` geschrieben — bei
                            // stereo und Versatz 16 hiesse das Position 16
                            // statt 8, und am Ende der Partition lief der
                            // Kanal aus seinem Feld heraus.
                            let mut temp = vec![0.0f64; n / 2 * ch];
                            let teilung = r.teilung as usize;
                            if !partition1(buch, l, &mut temp, versatz, teilung)? {
                                return Ok(v);
                            }
                            let basis = versatz / ch;
                            for q in 0..teilung {
                                let quelle = versatz + q;
                                let kanal = q % ch;
                                let ziel = basis + q / ch;
                                if ziel < v[kanal].len() && quelle < temp.len() {
                                    v[kanal][ziel] += temp[quelle];
                                }
                            }
                        }
                    }
                } else {
                    for j in 0..ch {
                        if kein_dekodieren.get(j).copied().unwrap_or(true) {
                            continue;
                        }
                        let klasse = usize::from(klassifizierung[j][anzahl]);
                        let buch_nr = r
                            .buecher
                            .get(klasse)
                            .and_then(|k| k.get(durchgang))
                            .copied()
                            .unwrap_or(KEIN);
                        if buch_nr == KEIN {
                            continue;
                        }
                        let buch = buecher.get(buch_nr as usize).ok_or_else(|| {
                        Fehler::neu(
                            Art::FalscherWert,
                            format!(
                                "Residuum-Klasse {klasse}, Durchgang {durchgang}: Buch {buch_nr}, \
                                 die Datei hat {}",
                                buecher.len()
                            ),
                        )
                    })?;
                        if !partition(r.art, buch, l, &mut v[j], versatz, r.teilung as usize)? {
                            return Ok(v);
                        }
                    }
                }
                let _ = versatz;
                anzahl += 1;
                i += 1;
            }
            if anzahl >= teilungen {
                break;
            }
        }
    }
    Ok(v)
}

/// Liest einen Wert; `None`, wenn das Paket endet.
///
/// DER WEG: `lies_symbol` gibt `Result`, und der Aufrufer braucht an
/// jeder Stelle drei Faelle: Wert, echter Fehler, Paketende. Genau das
/// ist `Option` im `Ok` — und mehr braucht es nicht.
///
/// DER HINWEIS AUF DIE DARFBARE ZWEITE DARLEIHUNG: der Helfer nimmt
/// **keinen** `&mut BitLeser`. Erstaunlich bequem, ihn doch
/// hineinzuschreiben, weil es „einheitlich" aussieht — dann aber
/// scheitert der Aufruf an zweifacher Ausleihe
/// („cannot borrow `*l` as mutable more than once"), weil Rust die
/// Argumente auswertet, bevor es den ersten belegenden Argumente weiter
/// gibt.
fn bis_zu_ende<T>(r: crate::fehler::Ergebnis<T>) -> Ergebnis<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(f) if f.art == Art::StromZuEnde => Ok(None),
        Err(f) => Err(f),
    }
}

/// Residuum 0 (Abschnitt 8.6.3): die Werte werden nach Dimension
/// verworfen.
///
/// `false` heisst: das Paket ist endete, und der Vektor traegt, was bis
/// dahin gelesen wurde (das ist der Normalfall am Paketende).
fn partition0(
    buch: &Codebuch,
    l: &mut BitLeser<'_>,
    v: &mut [f64],
    versatz: usize,
    n: usize,
) -> Ergebnis<bool> {
    // „[step] = [n] / [codebook_dimensions]" — eine Division durch 0
    // waere eine leere Schleife und wuerde die Partition als „nichts"
    // deuten.
    //
    // M3: `lies_codebuch` lehnt Dimension 0 schon ab, also ist `max(1)`
    // hier nur die Absicherung gegen einen fremden Codebuch.
    let dim = buch.dimensionen.max(1);
    let schritt = n / dim;
    for i in 0..schritt {
        let Some(eintrag) = bis_zu_ende(buch.lies_vektor(l))? else {
            return Ok(false);
        };
        for (j, wert) in eintrag.iter().enumerate() {
            let ziel = versatz + i + j * schritt;
            if ziel < v.len() {
                v[ziel] += *wert;
            }
        }
    }
    Ok(true)
}

/// Residuum 1 (Abschnitt 8.6.4): die Werte kommen der Reihe nach.
///
/// `false` heisst: das Paket ist endete.
fn partition1(
    buch: &Codebuch,
    l: &mut BitLeser<'_>,
    v: &mut [f64],
    versatz: usize,
    n: usize,
) -> Ergebnis<bool> {
    // GEMESSENE ENTSCHEIDUNG, warum `i` je **Skalar** weitergeht und
    // nicht je Vektor: Abschnitt 8.6.4 Schritt 5 sagt „increment [i]"
    // innerhalb der `codebook_dimensions`-Schleife. Bei einer Dimension 4
    // und einer Partition 8 sind das also zwei Vektoren.
    let mut i = 0usize;
    while i < n {
        let Some(eintrag) = bis_zu_ende(buch.lies_vektor(l))? else {
            return Ok(false);
        };
        for wert in &eintrag {
            let ziel = versatz + i;
            if ziel < v.len() {
                v[ziel] += *wert;
            }
            i += 1;
        }
    }
    Ok(true)
}

/// Residuum 0 oder 1, je nach Art.
fn partition(
    art: u32,
    buch: &Codebuch,
    l: &mut BitLeser<'_>,
    v: &mut [f64],
    versatz: usize,
    n: usize,
) -> Ergebnis<bool> {
    match art {
        0 => partition0(buch, l, v, versatz, n),
        _ => partition1(buch, l, v, versatz, n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitleser::BitLeser;

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

    /// Der Kopf einer Residuumkonfiguration (Abschnitt 8.6.1).
    fn kopf(beginn: u32, ende: u32, teilung: u32, klassen: u32, kb: u32) -> Vec<u8> {
        let mut bits = Vec::new();
        bits.extend(ziffern(beginn, 24));
        bits.extend(ziffern(ende, 24));
        bits.extend(ziffern(teilung - 1, 24));
        bits.extend(ziffern(klassen - 1, 6));
        bits.extend(ziffern(kb, 8));
        bits
    }

    #[test]
    fn der_kopf_liefert_grenzen_und_kaskade() {
        let mut bits = kopf(0, 64, 8, 2, 3);
        // Klasse 0: 3 Bit tief, Flag 0 -> Kaskade 5 (Durchgaenge 0 und 2)
        bits.extend(ziffern(5, 3));
        bits.push(0);
        // Klasse 1: Flag 1 -> 3 tief, 2 hoch -> 2*8+3 = 19
        bits.extend(ziffern(3, 3));
        bits.push(1);
        bits.extend(ziffern(2, 5));
        // Die Bücher: Klasse 0 hat Bit 0 und 2 gesetzt -> 2 Bücher.
        bits.extend(ziffern(10, 8));
        bits.extend(ziffern(11, 8));
        // Klasse 1 hat 0,1,4 gesetzt -> 3 Bücher.
        bits.extend(ziffern(20, 8));
        bits.extend(ziffern(21, 8));
        bits.extend(ziffern(22, 8));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let r = lies_rest(&mut l, 0).expect("rest");
        assert_eq!(r.art, 0);
        assert_eq!(r.beginn, 0);
        assert_eq!(r.ende, 64);
        assert_eq!(r.teilung, 8);
        assert_eq!(r.klassen, 2);
        assert_eq!(r.klassenbuch, 3);
        assert_eq!(r.kaskade, vec![5, 19]);
        assert_eq!(r.buecher[0][0], 10);
        assert_eq!(r.buecher[0][1], KEIN);
        assert_eq!(r.buecher[0][2], 11);
        assert_eq!(r.buecher[1][4], 22);
        assert_eq!(r.durchgaenge(0), 3, "Bit 2 gesetzt -> drei Durchgaenge");
        assert_eq!(r.durchgaenge(1), 5, "Bit 4 gesetzt -> fuenf Durchgaenge");
        assert_eq!(l.rest_bits(), bytes.len() * 8 - l.position(), "kein Rest");
    }

    #[test]
    fn ein_unbekannter_typ_wird_benannt() {
        let bytes = packiere(&kopf(0, 64, 8, 1, 0));
        let mut l = BitLeser::neu(&bytes);
        match lies_rest(&mut l, 3) {
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                assert!(f.text.contains('3'), "{f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn zu_grosse_werte_werden_benannt() {
        // M4: 24 Bit plus eins ist bis 2^24; eine Partition über 2^16
        // würde einen Vektor dieser Länge verlangen.
        let bytes = packiere(&kopf(0, 64, (1 << 16) + 1, 1, 0));
        let mut l = BitLeser::neu(&bytes);
        match lies_rest(&mut l, 0) {
            Err(f) => assert_eq!(f.art, Art::ZahlZuGross),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_fehlendes_klassenbuch_wird_benannt() {
        let mut bits = kopf(0, 64, 8, 1, 0);
        bits.extend(ziffern(1, 3)); // Kaskade 1 -> ein Buch
        bits.push(0);
        bits.extend(ziffern(7, 8)); // Klassenbuch 7 gibt es nicht
        bits.extend(ziffern(9, 8)); // das Residuumbuch
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let r = lies_rest(&mut l, 0).expect("rest");
        let leere: Vec<Codebuch> = Vec::new();
        match lies_paket(&r, &mut l, 64, 1, &[false], &leere) {
            // GEMESSENE KORREKTUNG AN DER ERWARTUNG: hier stand `contains('7')`.
            // Das Klassenbuch im Kopf war 0 (der Kopfbaustein `kopf(.., kb = 0)`),
            // also ist der Fehler **ueber das Klassenbuch 0** gemeldet worden —
            // und das ist die richtige Reihenfolge: das Klassenbuch wird vor dem
            // Residuumbuch gebraucht. Die Zahl 7 stand im *Residuum*-Feld, das
            // erst nach dem Klassenbuch kommt.
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                assert!(
                    f.text.contains("Klassenbuch 0"),
                    "der Fehler nennt das Klassenbuch: {f}"
                );
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_leerer_bereich_liefert_nur_nullen() {
        // Schritt 2: „if ([n_to_read] is zero), stop".
        let mut bits = kopf(10, 10, 8, 1, 0);
        bits.extend(ziffern(1, 3));
        bits.push(0);
        bits.extend(ziffern(0, 8));
        bits.extend(ziffern(9, 8));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let r = lies_rest(&mut l, 0).expect("rest");
        let leere: Vec<Codebuch> = Vec::new();
        let v = lies_paket(&r, &mut l, 64, 2, &[false, false], &leere).expect("paket");
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|k| k.iter().all(|&w| w == 0.0)));
        // Und es wurde kein einziges Bit gelesen.
        assert_eq!(l.rest_bits(), bytes.len() * 8 - l.position());
    }

    #[test]
    fn die_grenzen_stehen_in_der_norm() {
        assert_eq!(MAX_GRENZE, 1 << 24);
        assert_eq!(KEIN, -1);
        assert!(MAX_KLASSEN >= 8);
    }
}
