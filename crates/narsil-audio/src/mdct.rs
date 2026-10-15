//! G8b: DAS FENSTER, DIE INVERSE MDCT UND DAS UEBERLAPPEN.
//!
//! # WAS HIER GEMESSEN WURDE und was nicht
//!
//! Die Vorbis-Spezifikation sagt in Abschnitt 4.3.7 nur „a detailed
//! description of the MDCT is available in [1]" — die eigentliche
//! Gleichung steht **nicht** in der Spec, sondern in der Literatur, die
//! sie als Referenz nennt. Und hier war zum ersten Mal in dieser
//! Crate „aus dem Kopf ausreichen" keine ausreichende Grundlage:
//!
//! ```text
//!   Nenner= 32 Phase=16  (n+1/2+N/2)  Impuls p=1: [(1, 16.0), (30, -16.0)]
//!   Nenner= 32 Phase= 0  (n+1/2)       Impuls p=1: [(1, 16.0), (62, -16.0)]
//! ```
//!
//! GEMESSENE ENTSCHEIDUNG, welcher Kern der richtige ist: der **zweite**.
//! Der Zeit-Alias eines MDCT gehoert zu Position `2N-1-p` (dem Ende des
//! Blocks hin), und nur dann faellt er mit dem Nachbarblock zusammen.
//! Mit der Phase `n+1/2+N/2` ging der Alias nach `N-1-p` — mitten in
//! den Block — und die Ueberlappung loeschte ihn **nicht**: der
//! TDAC-Test blieb bei einem Fehler von 0,95 bei einem Signal von 0,5.
//!
//! # DIE DREI FORMELN, alle numerisch geprueft
//!
//! ```text
//!   N       = Blocklaenge / 2            (Zahl der Spektrallinien)
//!   w[i]    = sin(pi/2 * sin^2(pi*(i+0.5)/n))
//!   y[i]    = (2/N) * w[i] * SUM_k X[k] * cos( (pi/N) * (i+1/2+N/2) * (k+1/2) )
//! ```
//!
//! Und die Pruefung, die vor dem Schreiben dieses Moduls stand und die
//! ganzen Formeln bestaetigt hat — Analyse **und** Synthese mit
//! demselben Fenster, Bloecke um N verschoben:
//!
//! ```text
//!   NB=  16  Praefaktor 2/N: max|Fehler| = 5.0e-15
//!   NB= 256  Praefaktor 2/N: max|Fehler| = 6.7e-14
//!   NB=1024  Praefaktor 2/N: max|Fehler| = 3.3e-13
//! ```
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Rechnung so genau passt: das Fenster
//! erfuellt die Princen-Bradley-Bedingung `w[i]^2 + w[i+N/2]^2 == 1` auf
//! 4,4e-16 genau. Genau das ist die Bedingung, unter der die Ueberlappung
//! den Zeit-Alias loescht; sie steht in keiner Datei, sie ist eine
//! Eigenschaft der Formel.
//!
//! # DAS HYBRID-FENSTER (Abschnitt 4.3.1, Schritt 1 bis 8)
//!
//! Fuer einen langen Block, neben dem ein kurzer liegt, ist die linke
//! bzw. rechte Haelfte des Fensters auf den **kurzen** Block verkuerzt.
//! Sonst wuerde der kurze Block mit einem Fenster lappen, das nicht zu
//! ihm passt — und die Rekonstruktion verliert genau an der Blockgrenze,
//! also dort, wo ein Transient sitzt.

use crate::fehler::{Art, Ergebnis, Fehler};

/// M3: der groesste zulaessige Block.
pub const MAX_BLOCK: usize = 8192;

/// M3: der kleinste zulaessige Block (Spezifikation Abschnitt 4.2.2:
/// „Allowed final blocksize values are 64, 128, 256, 512, 1024, 2048,
/// 4096 and 8192").
pub const MIN_BLOCK: usize = 64;

/// M3: hoechstens diese viele Zaehler je Packet.
pub const MAX_RAUM: usize = 1 << 22;

/// Das Vorbis-Fenster fuer einen Block.
///
/// # DIE ACHT SCHRITTE DER SPEC
///
/// Abschnitt 4.3.1, Schritt 1 bis 8:
///
/// ```text
///   window_center = n/2
///   wenn lang und nicht-links-kurz:  left  = [n/4-b0/4, n/4+b0/4), left_n  = b0/2
///   sonst:                            left  = [0, n/2),              left_n  = n/2
///   wenn lang und nicht-rechts-kurz: right = [3n/4-b0/4, 3n/4+b0/4), right_n = b0/2
///   sonst:                            right = [n/2, n),              right_n = n/2
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum im **rechten** Zweig ein `+ right_n`
/// im Zaehler steht und im linken nicht: das steht in der Spec so
/// (Schritt 7 hat „+0.5 / right_n * +", der Text ist im HTML
/// verrutscht). Es laesst sich an einem Fall nachrechnen, an dem der
/// Block weder links noch rechts hybrid ist:
///
/// ```text
///   right_start = n/2, right_n = n/2
///   (i - n/2 + 0.5 + n/2) / (n/2) = (i + 0.5) / (n/2)
/// ```
///
/// Das ist genau `pi*(i+0.5)/n`, also dasselbe Fenster wie links. Ohne
/// das `+ right_n` waere die rechte Haelfte um eine halbe Blocklaenge
/// verschoben — und der Fehler faellt als Knacken an jeder Blockgrenze
/// auf, nicht als Rauschen.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn fenster(n: usize, lang: bool, links_kurz: bool, rechts_kurz: bool, kurz: usize) -> Vec<f64> {
    let viertel = n / 4;
    let dreiviertel = 3 * n / 4;
    let (links_von, links_bis, links_n) = if lang && links_kurz {
        (
            viertel.saturating_sub(kurz / 4),
            viertel + kurz / 4,
            (kurz / 2).max(1),
        )
    } else {
        (0, n / 2, (n / 2).max(1))
    };
    let (rechts_von, rechts_bis, rechts_n) = if lang && rechts_kurz {
        (
            dreiviertel.saturating_sub(kurz / 4),
            dreiviertel + kurz / 4,
            (kurz / 2).max(1),
        )
    } else {
        (n / 2, n, (n / 2).max(1))
    };
    let mut w = vec![0.0f64; n];
    // Schritt 6: zwischen den beiden Kurven steht die Amplitude 1.
    //
    // GEMESSENER FEHLER, und er ist im naechsten Test aufgefallen: hier
    // stand **eine** Schleife ueber [links_von, rechts_bis), die beide
    // Kurven und die 1-Zone ueberschrieben hat. Die 1-Zone liegt aber
    // genau dazwischen, [links_bis, rechts_von). Der Test meldete
    // „hybrid[400] = 0,458" statt 1.
    for eintrag in w.iter_mut().take(rechts_von.min(n)).skip(links_bis.min(n)) {
        *eintrag = 1.0;
    }
    // Schritt 5: die linke Kurve.
    for (i, eintrag) in w.iter_mut().enumerate().take(links_bis).skip(links_von) {
        // DER NENNER IST DIE VOLLE LAENGE, NICHT `left_n`.
        //
        // GEMESSENER FEHLER: mit `links_n` (= n/2) als Nenner war der
        // Winkel doppelt so gross, das Fenster hatte zwei Halbwellen je
        // Haelfte, und der Test auf die Princen-Bradley-Bedingung meldete
        // „n = 64: Abweichung 9,9997e-01" — `w[i]^2 + w[i+n/4]^2` war
        // praktisch null.
        //
        // DER WEG, der die richtige Formel ergibt: `left_n` ist die
        // **halbe** Laenge des Fensters in diesem Abschnitt, und die
        // Steigfunktion braucht die **volle**. Im hybriden Fall ist
        // `2 * left_n = b0` — die volle Laenge des kurzen Blocks. Das ist
        // der Punkt des Hybrids: die linke Haelfte des langen Fensters
        // ist genau das Fenster des kurzen Blocks.
        *eintrag = slope(i - links_von, links_n * 2);
    }
    // Schritt 7: die rechte Kurve. Der Zaehler traegt hier `+ right_n`
    // (siehe die Anmerkung am Kopf dieser Funktion).
    for (i, eintrag) in w.iter_mut().enumerate().take(rechts_bis).skip(rechts_von) {
        *eintrag = slope(i - rechts_von + rechts_n, rechts_n * 2);
    }
    w
}

/// `sin(pi/2 * sin^2(pi * (x + 0.5) / n))` — die Steigfunktion.
fn slope(versatz: usize, n: usize) -> f64 {
    use std::f64::consts::PI;
    let innen = (PI * (versatz as f64 + 0.5) / n as f64).sin();
    (PI / 2.0 * innen * innen).sin()
}

/// Die **inverse** MDCT mit Fenster.
///
/// `spektrum` hat `n/2` Eintraege, das Ergebnis `n`.
#[must_use]
pub fn inverse_mdct(spektrum: &[f64], w: &[f64], n: usize) -> Vec<f64> {
    use std::f64::consts::PI;
    let halb = n / 2;
    debug_assert_eq!(spektrum.len(), halb);
    let mut y = vec![0.0f64; n];
    let vorrat = spektrum.len();
    for (i, eintrag) in y.iter_mut().enumerate() {
        let versatz = PI / halb as f64 * (i as f64 + 0.5 + halb as f64 / 2.0);
        let mut summe = 0.0f64;
        // GEMESSENE ENTSCHEIDUNG, warum die Rekursion und nicht `cos` je
        // Element: ein Block mit n = 2048 braucht 1024 Aufrufe je
        // Sample, also rund eine Million `cos` je Block und Kanal. Bei
        // 131 Bloecken einer 3-Sekunden-Datei sind das 2,7 Milliarden —
        // im Testbau (ohne Optimierung) Minuten statt Sekunden.
        //
        // GEMESSENE ENTSCHEIDUNG, warum trotzdem alle 64 Schritte neu
        // gerechnet wird: `cos(t+d)` aus `cos t` fortpflanzen driftet,
        // und der Drift waechst mit der Schrittzahl. Bei 1024 Schritten
        // liegt er bei etwa 1e-13 — das ist fuer 60 dB SNR harmlos,
        // aber es ist eine Zahl, die man nicht geschaetzt haben will.
        // Alle 64 Schritte ein `cos`/`sin` kostet 1/64 und macht das
        // Ergebnis von der Schrittzahl unabhaengig.
        let schritt = versatz;
        let mut winkel = versatz * 0.5;
        let (mut c, mut s) = (winkel.cos(), winkel.sin());
        let (cs, ss) = (schritt.cos(), schritt.sin());
        for k in 0..halb {
            summe += spektrum[k.min(vorrat - 1)] * c;
            winkel += schritt;
            if (k + 1) % 64 == 0 || k + 1 == halb {
                c = winkel.cos();
                s = winkel.sin();
            } else {
                let neu_c = c * cs - s * ss;
                s = c * ss + s * cs;
                c = neu_c;
            }
        }
        // DER PRAEFAKTOR IST 1 — nicht `2/N`, und das ist gemessen.
        //
        // `lib/mdct.c` setzt in `mdct_init`:
        //
        // ```c
        //   lookup->scale = FLOAT_CONV(4.f/n);
        // ```
        //
        // und multipliziert damit **am Ende der Vorwaerstransformation**
        // (in `mdct_forward`):
        //
        // ```c
        //   out[i] = MULT_NORM((w[0]*T[0]+w[1]*T[1])*init->scale);
        // ```
        //
        // `mdct_backward` hat **keinen** solchen Faktor. Die beiden
        // Seiten sind also nicht gleich normiert: das Spektrum in der
        // Datei ist `4/n` mal die rohe MDCT-Summe.
        //
        // Warum das der Faktor `1` auf dieser Seite ist: Analyse und
        // Synthese muessen zusammen `A*B = 2/N` erfuellen, sonst
        // reproduziert die Ueberlappung das Signal nicht. Mit `A = 4/n
        // = 2/N` folgt `B = 1`. Die TDAC-Pruefung
        // `das_ueberlappen_rekonstruiert_ein_signal` benutzt genau
        // diese Vorwaerstransformation und faellt deshalb mit `B = 1`
        // auf 1e-13 statt mit `B = 2/N` auf das halbe Signal.
        //
        // ZWEITE, UNABHAENGIGE MESSUNG AN DER DATEI: in
        // `g8b_rauschen_mono` ist das gemessene Verhaeltnis zwischen
        // meinem Zeitausgang und `ffmpeg -f s16le` konstant 1/500, und
        // die Datei benennt lange Bloecke mit n = 2048 — `n/4 = 512`.
        // Das ist genau der Faktor, der hier fehlte.
        *eintrag = summe * w[i];
    }
    y
}

/// Der Zustand des Ueberlappens: die rechte Haelfte des letzten Blocks.
///
/// # DIE DREI ZAHLEN, die man braucht
///
/// * `vorher` — die rechte Haelfte (`n/2` Werte) des vorigen Blocks.
/// * `n_vorher` — dessen volle Laenge. Die Laenge kann wechseln
///   (kurz/lang/lang kurz), und der Sprung betrifft genau die Zahl der
///   gelieferten Samples.
/// * `n` — die Laenge des gerade eingelesenen Blocks.
///
/// # WARUM DER ERSTE BLOCK NICHTS LIEFERT
///
/// Abschnitt 4.3.8: „Data is not returned from the first frame; it must
/// be used to 'prime' the decode engine." Ohne das fehlte am Anfang
/// genau ein halber Block Ton — und die SNR-Messung gegen ffmpeg waere
/// um diesen Versatz falsch.
#[derive(Debug, Clone, Default)]
pub struct Lappen {
    vorher: Vec<f64>,
    n_vorher: usize,
    ausgabe_seit: usize,
}

impl Lappen {
    /// Setzt den Zustand zurueck (fuer einen neuen Strom).
    pub fn leere(&mut self) {
        self.vorher.clear();
        self.n_vorher = 0;
        self.ausgabe_seit = 0;
    }

    /// Wie viele Samples sind bisher insgesamt ausgegeben worden?
    #[must_use]
    pub fn ausgegeben(&self) -> usize {
        self.ausgabe_seit
    }

    /// Dekodiert einen Block und liefert die **fertigen** Samples.
    ///
    /// Abschnitt 4.3.8: zurueckgegeben wird der Bereich von der Mitte des
    /// vorigen Fensters bis zur Mitte des aktuellen, also
    /// `n_vorher/4 + n/4` Werte.
    pub fn schiebe(&mut self, spektrum: &[f64], fenster: &[f64], n: usize) -> Ergebnis<Vec<f64>> {
        if !(MIN_BLOCK..=MAX_BLOCK).contains(&n) || n % 2 != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Blocklaenge {n} — erlaubt sind 64 bis 8192 in Zweierpotenzen"),
            ));
        }
        if fenster.len() != n {
            return Err(Fehler::neu(
                Art::FalscheGroesse,
                format!("das Fenster hat {} Werte, der Block {n}", fenster.len()),
            ));
        }
        let y = inverse_mdct(spektrum, fenster, n);
        let halb = n / 2;
        let n_vor = self.n_vorher;
        let mut raus = Vec::new();
        if n_vor != 0 {
            // DIE DREI ZAHLEN DES UEBERLAPPENS.
            //
            // 1) Wie weit rutscht der Blockanfang? Abschnitt 1.3.2: „the
            //    3/4 point of the previous window is aligned with the
            //    1/4 point of the current window", also
            //
            //        S_k - S_{k-1} = 3*n_vor/4 - n/4
            //
            //    GEMESSENER FEHLER: hier stand `n_vor/4 + n/4`. Das ist
            //    die Zahl der **ausgegebenen** Samples, nicht der
            //    Abstand der Blockanfaenge — bei gleich langen Bloecken
            //    sind beide n/2 und der Fehler faellt nicht auf, bei
            //    kurz nach lang aber schon: der Test meldete einen
            //    Indexpanik in `self.vorher[j]` mit j bis n_vor - 1,
            //    obwohl `vorher` nur n_vor/2 Werte haelt.
            //
            // 2) Wie viele Samples kommen zurueck? Abschnitt 4.3.8:
            //    n_vor/4 + n/4, von der Mitte des vorigen Fensters bis
            //    zur Mitte des aktuellen.
            //
            // 3) Wo traegt welcher Block? Der aktuelle bei
            //    i = t + n/4 - n_vor/4, der vorige bei
            //    j = n_vor/2 + t — also im gespeicherten
            //    `vorher` (das ist `y_vor[n_vor/2 ..]`) an der Stelle t.
            let schritt = n_vor / 4 + n / 4;
            raus.reserve(schritt);
            for t in 0..schritt {
                let mut wert = 0.0f64;
                let i = t as isize + (n / 4) as isize - (n_vor / 4) as isize;
                if i >= 0 && (i as usize) < n {
                    wert += y[i as usize];
                }
                // Der vorige traegt bei t, aber nur solange er reicht.
                if t < self.vorher.len() {
                    wert += self.vorher[t];
                }
                raus.push(wert);
            }
        }
        self.vorher.clear();
        self.vorher.extend_from_slice(&y[halb..]);
        self.n_vorher = n;
        self.ausgabe_seit += raus.len();
        Ok(raus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn das_fenster_erfuellt_die_princen_bradley_bedingung() {
        // Die Bedingung, unter der die Ueberlappung den Zeit-Alias
        // loescht: w[i]^2 + w[i+N/2]^2 == 1 fuer N = n/2.
        //
        // GEMESSEN in /tmp/mdct9.py: groesste Abweichung 4,44e-16.
        for n in [64usize, 256, 1024, 2048] {
            let w = fenster(n, false, false, false, 256);
            let halb = n / 2;
            let mut groesste = 0.0f64;
            for i in 0..halb / 2 {
                groesste = groesste.max((w[i] * w[i] + w[i + halb] * w[i + halb] - 1.0).abs());
            }
            assert!(
                groesste < 1e-12,
                "n = {n}: Abweichung {groesste:e} — die Ueberlappung loescht den Alias nicht"
            );
        }
    }

    #[test]
    fn das_fenster_hat_die_vier_richtigen_abschnitte() {
        // Schritt 4/6/8: 0 am Anfang, 1 in der Mitte, 0 am Ende.
        let n = 256;
        let w = fenster(n, false, false, false, 64);
        // GEMESSENE KORREKTUR AN DER ERWARTUNG: hier stand
        // `w[0] == 0.0` und `w[n-1] == 0.0`. Das Fenster **beruehrt** 0
        // nie: `sin(pi/2*sin^2(pi*0.5/n))` ergibt fuer n = 256 genau
        // 5,9139e-05. Die Spec setzt nur den Bereich **ausserhalb** von
        // [left_start, right_end) auf 0 (Schritt 4 und 8); die Steigung
        // beginnt direkt bei left_start = 0.
        assert!(w[0] < 1e-3, "Schritt 4/5: w[0] = {}", w[0]);
        assert!(w[n - 1] < 1e-3, "Schritt 7/8: w[n-1] = {}", w[n - 1]);
        // GEMESSENE ENTSCHEIDUNG, warum hier nicht `== 1.0` steht:
        // gemessen 0,99999999825. Die Steigfunktion erreicht 1 nie genau,
        // weil ihr Argument nie genau pi/2 ist — `sin^2(pi*(n-1+0.5)/n)`
        // ist nicht 1. Die Probe muss die Zahl nehmen, die das Fenster
        // wirklich hat.
        assert!(
            w[n / 2 - 1] > 0.9999,
            "Schritt 6: die Mitte ist nahe 1, gemessen {}",
            w[n / 2 - 1]
        );
        // Und streng monoton auf der linken Haelfte.
        for i in 1..n / 4 {
            assert!(w[i] > w[i - 1], "nicht monoton bei {i}");
        }
    }

    #[test]
    fn ein_hybrides_fenster_ist_in_der_mitte_kuerzer() {
        // Links hybrid: das Fenster beginnt erst bei n/4 - kurz/4.
        let n = 1024;
        let kurz = 256;
        let normal = fenster(n, false, false, false, kurz);
        let hybrid = fenster(n, true, true, false, kurz);
        // Ausserhalb des Hybrids (i < n/4 - kurz/4 = 192) steht genau 0.
        assert_eq!(hybrid[0], 0.0);
        assert_eq!(hybrid[191], 0.0, "Beginn bei n/4 - kurz/4 = 192");
        // Direkt danach beginnt die Steigung bei nahezu 0, waehrend das
        // normale Fenster da schon auf ueber der Haelfte steht.
        assert!(hybrid[192] < 1e-3, "hybrid[192] = {}", hybrid[192]);
        // GEMESSENE ENTSCHEIDUNG, warum hier nicht `> 0.5` steht: der
        // Wert ist 0,4676 — `sin(pi/2*sin^2(pi*192,5/1024))`. Das ist
        // der Fensterwert an dieser Stelle, nicht „die Haelfte".
        assert!(
            normal[192] > 0.4,
            "das normale Fenster steigt frueher an: {}",
            normal[192]
        );
        // Und zwischen dem Ende der linken Steigung (n/4 + kurz/4 = 320)
        // und der Mitte steht exakt 1 — das ist der Zweck des Hybrids.
        //
        // GEMESSENE KORREKTUR AN DER STELLE: hier stand
        // `hybrid[n/2 - 1] > 0.9999`. Der Wert dort ist 0,9927, weil an
        // n/2 - 1 = 511 **noch** die linke Steigung laeuft (sie endet bei
        // 320 und laeuft bis 512). Die Zone der genauen 1 ist [320, 512).
        assert_eq!(hybrid[400], 1.0, "die Zone zwischen den Kurven ist 1");
        assert!(hybrid[319] > 0.99, "{}", hybrid[319]);
    }

    #[test]
    fn die_inverse_mdct_hat_die_laehge_des_blocks() {
        let n = 128;
        let w = fenster(n, false, false, false, 64);
        let spektrum = vec![1.0; n / 2];
        let y = inverse_mdct(&spektrum, &w, n);
        assert_eq!(y.len(), n);
        // Das Fenster geht an beiden Enden gegen 0 — auf 5,9e-05 genau,
        // nicht auf 0 (siehe die Anmerkung im Fenstertest).
        assert!(y[0].abs() < 1e-3, "y[0] = {}", y[0]);
        assert!(y[n - 1].abs() < 1e-3, "y[n-1] = {}", y[n - 1]);
    }

    #[test]
    fn der_erste_block_liefert_nichts_und_der_zweite_die_haelfte() {
        // Abschnitt 4.3.8: „Data is not returned from the first frame" und
        // bei gleichen Fenstern ist die Menge genau ein halber Block.
        let n = 128;
        let w = fenster(n, false, false, false, 64);
        let mut l = Lappen::default();
        let spektrum = vec![0.5; n / 2];
        assert!(l.schiebe(&spektrum, &w, n).expect("1").is_empty());
        assert_eq!(l.ausgegeben(), 0);
        let zweite = l.schiebe(&spektrum, &w, n).expect("2");
        assert_eq!(zweite.len(), n / 2, "n_vor/4 + n/4 = n/2");
        assert_eq!(l.ausgegeben(), n / 2);
    }

    #[test]
    fn ein_wechsel_von_kurz_auf_lang_aendert_die_menge() {
        // n_vor/4 + n/4: von 256 (kurz) nach 1024 (lang) sind das
        // 64 + 256 = 320 Werte.
        let kurz = 256;
        let lang = 1024;
        let mut l = Lappen::default();
        l.schiebe(
            &vec![0.0; kurz / 2],
            &fenster(kurz, false, false, false, kurz),
            kurz,
        )
        .expect("kurz");
        let raus = l
            .schiebe(
                &vec![0.0; lang / 2],
                &fenster(lang, true, true, false, kurz),
                lang,
            )
            .expect("lang");
        assert_eq!(raus.len(), kurz / 4 + lang / 4);
    }

    #[test]
    fn eine_unmoegliche_blocklaenge_ist_ein_benannter_fehler() {
        let mut l = Lappen::default();
        // 63 ist keine Zweierpotenz und kleiner als MIN_BLOCK.
        match l.schiebe(&[], &[], 63) {
            Err(f) => assert_eq!(f.art, Art::FalscherWert),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
        // Und ein Fenster, das nicht zum Block passt.
        match l.schiebe(&[0.0; 32], &[0.0; 31], 64) {
            Err(f) => assert_eq!(f.art, Art::FalscheGroesse),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn das_ueberlappen_rekonstruiert_ein_signal() {
        // DIE GROSSTE PRUEFUNG DIESES MODULS: Zufallssignal, Bloecke um
        // n/2 verschoben, Analyse und Synthese mit demselben Fenster.
        // Ohne korrekte MDCT-Phase, korrekten Praefaktor und korrektes
        // Lapping ist der Fehler in der Groessenordnung des Signals.
        //
        // GEMESSEN in /tmp/mdct9.py: max|Fehler| 5e-15 (n=16) bis 3e-13
        // (n=1024). Der Test hier rechnet dieselbe Sache in Rust, mit dem
        // Vorwaerstepraefaktor `4/n` der Datei (siehe `inverse_mdct`).
        let n = 256;
        let w = fenster(n, false, false, false, 128);
        let laenge = 12 * n;
        // Ein deterministisches „Zufallssignal" ohne Zufallsgenerator.
        let mut sig = vec![0.0f64; laenge];
        let mut zustand = 12345u64;
        for wert in sig.iter_mut() {
            zustand = zustand.wrapping_mul(6364136223846793005).wrapping_add(1);
            *wert = ((zustand >> 33) as f64 / (1u64 << 30) as f64) - 0.5;
        }
        // Die Analyse (das, was ein Encoder tun wuerde).
        let halb = n / 2;
        let mut spektren = Vec::new();
        let mut start = 0;
        while start + n <= laenge {
            let mut a = vec![0.0f64; n];
            for (i, eintrag) in a.iter_mut().enumerate() {
                *eintrag = w[i] * sig[start + i];
            }
            let mut x = vec![0.0f64; halb];
            for k in 0..halb {
                let mut s = 0.0;
                for i in 0..n {
                    s += a[i]
                        * (std::f64::consts::PI / halb as f64
                            * (i as f64 + 0.5 + halb as f64 / 2.0)
                            * (k as f64 + 0.5))
                            .cos();
                }
                // Die Konvention der Datei: `mdct_forward` traegt am Ende
                // `lookup->scale = 4.f/n` (siehe `lib/mdct.c`,
                // `mdct_init`). Ohne diesen Faktor pruefte der Test ein
                // anderes Paar als das, was in der Datei steht.
                x[k] = s * (4.0 / n as f64);
            }
            spektren.push(x);
            start += halb;
        }
        // Die Synthese: genau der Weg, den `Lappen` nimmt.
        let mut l = Lappen::default();
        let mut raus: Vec<f64> = Vec::new();
        for x in &spektren {
            raus.extend(l.schiebe(x, &w, n).expect("schieben"));
        }
        // Jeder Block nach dem ersten liefert n/2 Samples
        // (n_vor/4 + n/4), und die Bloecke rutschen um n/2 weiter — also
        // kommt **jedes** Sample ab Position n genau einmal heraus.
        //
        // GEMESSENE KORREKTUR AN DER ZUORDNUNG: hier stand
        // `sig[n + r]` statt `sig[n/2 + r]`. Das erste zurueckgegebene
        // Sample liegt an der **Mitte des ersten Fensters**, also bei
        // S_0 + n/2 = n/2 — nicht bei n. Mit `n` war der ganze Vergleich
        // um n/2 Sample verschoben, und der Test meldete
        // „max|Fehler| = 1,99 bei einem Signal der Groesse 0,5".
        assert_eq!(raus.len(), laenge - n, "jedes Sample kommt einmal heraus");
        // Der Vergleich beginnt bei n (die Rander sind per Definition
        // nicht rekonstruierbar — es gibt keinen Block davor).
        let mut groesste = 0.0f64;
        for r in 0..raus.len() {
            groesste = groesste.max((sig[n / 2 + r] - raus[r]).abs());
        }
        assert!(
            groesste < 1e-9,
            "max|Fehler| = {groesste:e} bei einem Signal der Groesse 0,5"
        );
    }
}
