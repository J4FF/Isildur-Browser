//! Bidi-Algorithmus nach UAX #9 (Unicode 15.1) — Aufgabe I1.
//!
//! Der Ablauf folgt der Regelkette der Spezifikation:
//! P2/P3 (Absatzebene), X1-X8 (explizite Ebenen und Richtungen),
//! X9 (Entfernen der Steuerzeichen), X10 (isolierende Laufsequenzen),
//! W1-W7 (schwache Typen), N0 (Klammerpaare, BidiBrackets.txt),
//! N1/N2 (neutrale Typen), I1/I2 (implizite Ebenen), L1 (Zeilenanfang
//! und -ende), L2 (Neuordnung) und L4 (Spiegelung als Abfrage).
//!
//! # Massgeblich
//!
//! UAX #9 in der Fassung fuer Unicode 15.1 (tr9-51). Alle Tabellen
//! stammen aus /usr/share/unicode (Unicode 15.1.0):
//! `extracted/DerivedBidiClass.txt`, `BidiBrackets.txt`,
//! `BidiMirroring.txt` — erzeugt, nicht abgetippt.
//!
//! # Grenzen
//!
//! * [`analysiere`] verarbeitet GENAU EINEN Absatz und GENAU EINE Zeile
//!   (so sind auch die offiziellen BidiCharacterTest-Faelle gebaut).
//!   Wer Absaetze trennt, teilt an Typ B; wer Zeilen bricht, ruft L1/L2
//!   je Zeile erneut auf (siehe `zeilen_ebenen` fuer L1).
//! * Hoechste explizite Tiefe: [`MAX_TIEFE`] = 125 (UAX #9). Tiefere
//!   Einbettungen werden nach X2-X5 ignoriert (Overflow), nicht
//!   abgeschnitten.
//! * Klammerstapel: [`MAX_KLAMMERN`] = 63 (BD16).
//! * L3 (Kombinationszeichen) ist in [`analysiere`] bewusst NICHT
//!   enthalten (die offizielle Testdatei schliesst L3/L4 aus); die
//!   Ausgabe-Hilfe [`umordnen`] zieht Kombinationszeichen hinter ihre
//!   Basis.
//! * Lineare Laufzeit, keine Panics, kein unsafe.

use crate::unicode_daten::bidi_klammern::{Klammerart, kanonisch, klammer};
use crate::unicode_daten::bidi_klassen::BidiKlasse;

pub use crate::unicode_daten::bidi_spiegel::spiegel;

/// Hoechste explizite Einbettungstiefe (UAX #9: max_depth = 125).
pub const MAX_TIEFE: u8 = 125;

/// Groesse des Klammerstapels in Regel BD16.
pub const MAX_KLAMMERN: usize = 63;

/// Richtung eines Absatzes fuer P2/P3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absatzrichtung {
    /// Feste Absatzebene 0 (links nach rechts).
    Ltr,
    /// Feste Absatzebene 1 (rechts nach links).
    Rtl,
    /// Automatisch nach P2/P3.
    Automatisch,
}

/// Ergebnis der Absatzanalyse eines einzelnen Absatzes auf einer Zeile.
#[derive(Debug, Clone, PartialEq)]
pub struct AbsatzAnalys {
    /// Aufgeloeste Absatzebene (P2/P3).
    pub ebene: u8,
    /// Originale Bidi-Klasse je Zeichen (vor X6; fuer P2/P3 und L1).
    pub klassen: Vec<BidiKlasse>,
    /// Typen nach X6, W1-W7 und N0-N2.
    pub typen: Vec<BidiKlasse>,
    /// Ebene je Zeichen nach I1/I2 und L1; `None` = von X9 entfernt.
    pub ebenen: Vec<Option<u8>>,
    /// Visuelle Reihenfolge nach L2 (Indizes ohne die entfernten
    /// Zeichen).
    pub reihenfolge: Vec<usize>,
}

/// Liefert die Bidi-Klasse eines Zeichens.
pub fn bidi_klasse(ch: char) -> BidiKlasse {
    crate::unicode_daten::bidi_klassen::bidi_klasse(ch)
}

/// Bestimmt die Absatzebene (P2/P3).
///
/// `richtung` Ltr/Rtl setzt die Ebene fest; `Automatisch` sucht das
/// erste starke Zeichen (L, R oder AL) und ignoriert dabei die
/// Zeichen zwischen einem Isolationsanfang und seinem passenden PDI.
pub fn absatz_ebene(text: &str, richtung: Absatzrichtung) -> u8 {
    match richtung {
        Absatzrichtung::Ltr => 0,
        Absatzrichtung::Rtl => 1,
        Absatzrichtung::Automatisch => {
            let klassen: Vec<BidiKlasse> = text.chars().map(bidi_klasse).collect();
            let partner = isolat_partner(&klassen);
            p2p3(&klassen, &partner, 0, klassen.len())
        }
    }
}

/// Fuehrt den vollstaendigen Bidi-Algorithmus fuer einen Absatz aus.
pub fn analysiere(text: &str, richtung: Absatzrichtung) -> AbsatzAnalys {
    let zeichen: Vec<char> = text.chars().collect();
    analysiere_zeichen(&zeichen, richtung)
}

/// Wie [`analysiere`], aber auf einer bereits zerlegten Zeichenfolge.
pub fn analysiere_zeichen(zeichen: &[char], richtung: Absatzrichtung) -> AbsatzAnalys {
    let n = zeichen.len();
    let klassen: Vec<BidiKlasse> = zeichen.iter().map(|&c| bidi_klasse(c)).collect();
    let partner = isolat_partner(&klassen);
    let ebene = match richtung {
        Absatzrichtung::Ltr => 0,
        Absatzrichtung::Rtl => 1,
        Absatzrichtung::Automatisch => p2p3(&klassen, &partner, 0, n),
    };

    // ------------------------------------------------------------------
    // X1-X8: explizite Ebenen, Uebersteuerungen, Isolate.
    // ------------------------------------------------------------------
    #[derive(Clone, Copy, PartialEq)]
    enum Ueber {
        Neutral,
        L,
        R,
    }
    #[derive(Clone, Copy)]
    struct Eintrag {
        ebene: u8,
        ueber: Ueber,
        isolat: bool,
    }

    let mut stapel: Vec<Eintrag> = vec![Eintrag {
        ebene,
        ueber: Ueber::Neutral,
        isolat: false,
    }];
    let mut overflow_isolat: usize = 0;
    let mut overflow_einbettung: usize = 0;
    let mut gueltige_isolate: usize = 0;

    let mut ebenen: Vec<Option<u8>> = vec![Some(ebene); n];
    let mut typen: Vec<BidiKlasse> = klassen.clone();

    for i in 0..n {
        let oberste = *stapel.last().expect("Stapel ist nie leer");
        match klassen[i] {
            BidiKlasse::Rle | BidiKlasse::Lre | BidiKlasse::Rlo | BidiKlasse::Lro => {
                let gerade = matches!(klassen[i], BidiKlasse::Lre | BidiKlasse::Lro);
                let neu = if gerade {
                    (oberste.ebene + 2) & !1
                } else {
                    (oberste.ebene + 1) | 1
                };
                if neu <= MAX_TIEFE && overflow_isolat == 0 && overflow_einbettung == 0 {
                    let ueber = match klassen[i] {
                        BidiKlasse::Rlo => Ueber::R,
                        BidiKlasse::Lro => Ueber::L,
                        _ => Ueber::Neutral,
                    };
                    stapel.push(Eintrag {
                        ebene: neu,
                        ueber,
                        isolat: false,
                    });
                } else if overflow_isolat == 0 {
                    overflow_einbettung += 1;
                }
                ebenen[i] = Some(oberste.ebene);
            }
            BidiKlasse::Pdf => {
                if overflow_isolat > 0 {
                    // nichts
                } else if overflow_einbettung > 0 {
                    overflow_einbettung -= 1;
                } else if stapel.len() >= 2 && !oberste.isolat {
                    stapel.pop();
                }
                let oberste = *stapel.last().expect("Stapel ist nie leer");
                ebenen[i] = Some(oberste.ebene);
            }
            BidiKlasse::Rli | BidiKlasse::Lri | BidiKlasse::Fsi => {
                ebenen[i] = Some(oberste.ebene);
                if oberste.ueber != Ueber::Neutral {
                    typen[i] = if oberste.ueber == Ueber::L {
                        BidiKlasse::L
                    } else {
                        BidiKlasse::R
                    };
                }
                let rechts = match klassen[i] {
                    BidiKlasse::Rli => true,
                    BidiKlasse::Lri => false,
                    // X5c: P2/P3 auf den eingeschlossenen Text.
                    BidiKlasse::Fsi => {
                        let ende = partner[i].unwrap_or(n);
                        p2p3(&klassen, &partner, i + 1, ende) == 1
                    }
                    _ => false,
                };
                let neu = if rechts {
                    (oberste.ebene + 1) | 1
                } else {
                    (oberste.ebene + 2) & !1
                };
                if neu <= MAX_TIEFE && overflow_isolat == 0 && overflow_einbettung == 0 {
                    stapel.push(Eintrag {
                        ebene: neu,
                        ueber: Ueber::Neutral,
                        isolat: true,
                    });
                    gueltige_isolate += 1;
                } else {
                    overflow_isolat += 1;
                }
            }
            BidiKlasse::Pdi => {
                // X6a
                if overflow_isolat > 0 {
                    overflow_isolat -= 1;
                } else if gueltige_isolate > 0 {
                    overflow_einbettung = 0;
                    while !stapel.last().expect("Stapel ist nie leer").isolat {
                        stapel.pop();
                    }
                    stapel.pop();
                    gueltige_isolate -= 1;
                }
                let oberste = *stapel.last().expect("Stapel ist nie leer");
                ebenen[i] = Some(oberste.ebene);
                if oberste.ueber != Ueber::Neutral {
                    typen[i] = if oberste.ueber == Ueber::L {
                        BidiKlasse::L
                    } else {
                        BidiKlasse::R
                    };
                }
            }
            BidiKlasse::B => {
                // X8: Absatztrenner gehoeren keiner Einbettung an.
                ebenen[i] = Some(ebene);
            }
            BidiKlasse::Bn => {
                ebenen[i] = Some(oberste.ebene);
            }
            _ => {
                // X6: alle uebrigen Typen.
                ebenen[i] = Some(oberste.ebene);
                if oberste.ueber != Ueber::Neutral {
                    typen[i] = if oberste.ueber == Ueber::L {
                        BidiKlasse::L
                    } else {
                        BidiKlasse::R
                    };
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // X9: entfernte Zeichen.
    // ------------------------------------------------------------------
    let entfernt: Vec<bool> = klassen
        .iter()
        .map(|k| {
            matches!(
                k,
                BidiKlasse::Rle
                    | BidiKlasse::Lre
                    | BidiKlasse::Rlo
                    | BidiKlasse::Lro
                    | BidiKlasse::Pdf
                    | BidiKlasse::Bn
            )
        })
        .collect();
    // X9: entfernte Zeichen bekommen keine Ebene mehr (die Testdatei
    // zeigt fuer sie 'x').
    for i in 0..n {
        if entfernt[i] {
            ebenen[i] = None;
        }
    }

    // ------------------------------------------------------------------
    // X10: Level-Laeufe und isolierende Laufsequenzen (BD7/BD13).
    // ------------------------------------------------------------------
    let sichtbar: Vec<usize> = (0..n).filter(|&i| !entfernt[i]).collect();
    if sichtbar.is_empty() {
        for e in ebenen.iter_mut() {
            *e = None;
        }
        return AbsatzAnalys {
            ebene,
            klassen,
            typen,
            ebenen,
            reihenfolge: Vec::new(),
        };
    }
    let mut ebenen_x10: Vec<u8> = sichtbar
        .iter()
        .map(|&i| ebenen[i].expect("sichtbare Zeichen haben eine Ebene"))
        .collect();
    let mut pos_von: Vec<usize> = vec![usize::MAX; n];
    for (k, &i) in sichtbar.iter().enumerate() {
        pos_von[i] = k;
    }

    // Level-Laeufe: (start, ende) als Positionen in `sichtbar`.
    let mut laeufe: Vec<(usize, usize)> = Vec::new();
    let mut lauf_von: Vec<usize> = vec![usize::MAX; n];
    for (k, &i) in sichtbar.iter().enumerate() {
        if k == 0 || ebenen_x10[k] != ebenen_x10[k - 1] {
            laeufe.push((k, k + 1));
        } else {
            laeufe.last_mut().expect("Lauf existiert").1 = k + 1;
        }
        lauf_von[i] = laeufe.len() - 1;
    }

    // BD13: Sequenzen bilden.
    let mut pdi_gematcht: Vec<bool> = vec![false; n];
    for i in 0..n {
        if let Some(j) = partner[i] {
            if matches!(
                klassen[i],
                BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi
            ) {
                pdi_gematcht[j] = true;
            }
        }
    }
    let mut sequenzen: Vec<Vec<usize>> = Vec::new();
    let mut verbraucht: Vec<bool> = vec![false; laeufe.len()];
    for r in 0..laeufe.len() {
        if verbraucht[r] {
            continue;
        }
        let erster = sichtbar[laeufe[r].0];
        if klassen[erster] == BidiKlasse::Pdi && pdi_gematcht[erster] {
            continue; // gehoert zur Sequenz eines Isolationsanfangs
        }
        let mut seq: Vec<usize> = Vec::new();
        let mut r_aktuell = r;
        loop {
            verbraucht[r_aktuell] = true;
            let (a, b) = laeufe[r_aktuell];
            seq.extend_from_slice(&sichtbar[a..b]);
            let letzter = *seq.last().expect("Lauf ist nicht leer");
            if matches!(
                klassen[letzter],
                BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi
            ) {
                if let Some(pdi) = partner[letzter] {
                    let r_pdi = lauf_von[pdi];
                    if !verbraucht[r_pdi] {
                        r_aktuell = r_pdi;
                        continue;
                    }
                }
            }
            break;
        }
        sequenzen.push(seq);
    }

    // ------------------------------------------------------------------
    // W1-W7, N0-N2, I1-I2 je isolierender Laufsequenz.
    // ------------------------------------------------------------------
    for seq in &sequenzen {
        let f = seq[0];
        let l = *seq.last().expect("Sequenz ist nicht leer");
        let ebene_f = ebenen_x10[pos_von[f]];
        let ebene_l = ebenen_x10[pos_von[l]];
        let vor = if f > 0 {
            (0..f)
                .rev()
                .find(|&i| !entfernt[i])
                .map(|i| ebenen[i].expect("sichtbare Zeichen haben eine Ebene"))
        } else {
            None
        };
        let sos_ebene = ebene_f.max(vor.unwrap_or(ebene));
        let sos = if sos_ebene % 2 == 1 {
            BidiKlasse::R
        } else {
            BidiKlasse::L
        };
        let unmatched_isolat = matches!(
            klassen[l],
            BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi
        ) && partner[l].is_none();
        let nach = if unmatched_isolat {
            None
        } else {
            (l + 1..n)
                .find(|&i| !entfernt[i])
                .map(|i| ebenen[i].expect("sichtbare Zeichen haben eine Ebene"))
        };
        let eos_ebene = ebene_l.max(nach.unwrap_or(ebene));
        let eos = if eos_ebene % 2 == 1 {
            BidiKlasse::R
        } else {
            BidiKlasse::L
        };

        // W1: NSM nimmt den Typ des Vorgaengers (oder sos).
        for p in 0..seq.len() {
            let i = seq[p];
            if typen[i] == BidiKlasse::Nsm {
                typen[i] = if p == 0 {
                    sos
                } else {
                    let vor_typ = typen[seq[p - 1]];
                    if matches!(
                        vor_typ,
                        BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi | BidiKlasse::Pdi
                    ) {
                        BidiKlasse::On
                    } else {
                        vor_typ
                    }
                };
            }
        }
        // W2: EN nach AL wird AN.
        let mut stark = sos;
        for &i in seq {
            match typen[i] {
                BidiKlasse::R | BidiKlasse::L | BidiKlasse::Al => stark = typen[i],
                BidiKlasse::En if stark == BidiKlasse::Al => typen[i] = BidiKlasse::An,
                _ => {}
            }
        }
        // W3: AL wird R.
        for &i in seq {
            if typen[i] == BidiKlasse::Al {
                typen[i] = BidiKlasse::R;
            }
        }
        // W4: einzelne Trenner zwischen gleichen Zahlen.
        for p in 1..seq.len().saturating_sub(1) {
            let mitte = typen[seq[p]];
            let links = typen[seq[p - 1]];
            let rechts = typen[seq[p + 1]];
            if mitte == BidiKlasse::Es && links == BidiKlasse::En && rechts == BidiKlasse::En {
                typen[seq[p]] = BidiKlasse::En;
            } else if mitte == BidiKlasse::Cs {
                if links == BidiKlasse::En && rechts == BidiKlasse::En {
                    typen[seq[p]] = BidiKlasse::En;
                } else if links == BidiKlasse::An && rechts == BidiKlasse::An {
                    typen[seq[p]] = BidiKlasse::An;
                }
            }
        }
        // W5: ET-Ketten neben EN werden EN.
        let mut p = 0;
        while p < seq.len() {
            if typen[seq[p]] == BidiKlasse::Et {
                let mut q = p;
                while q < seq.len() && typen[seq[q]] == BidiKlasse::Et {
                    q += 1;
                }
                let links_en = p > 0 && typen[seq[p - 1]] == BidiKlasse::En;
                let rechts_en = q < seq.len() && typen[seq[q]] == BidiKlasse::En;
                if links_en || rechts_en {
                    for k in p..q {
                        typen[seq[k]] = BidiKlasse::En;
                    }
                }
                p = q;
            } else {
                p += 1;
            }
        }
        // W6: uebrige Trenner werden ON.
        for &i in seq {
            if matches!(typen[i], BidiKlasse::Et | BidiKlasse::Es | BidiKlasse::Cs) {
                typen[i] = BidiKlasse::On;
            }
        }
        // W7: EN nach L wird L.
        let mut stark = sos;
        for &i in seq {
            match typen[i] {
                BidiKlasse::L | BidiKlasse::R => stark = typen[i],
                BidiKlasse::En if stark == BidiKlasse::L => typen[i] = BidiKlasse::L,
                _ => {}
            }
        }

        // --------------------------------------------------------------
        // N0: Klammerpaare (BD16) und ihre Richtung.
        // --------------------------------------------------------------
        let einbettung = if ebenen_x10[pos_von[f]] % 2 == 1 {
            BidiKlasse::R
        } else {
            BidiKlasse::L
        };
        let mut stapel_klammer: Vec<(char, usize)> = Vec::new();
        let mut paare: Vec<(usize, usize)> = Vec::new();
        let mut klammer_ueberlauf = false;
        for (p, &i) in seq.iter().enumerate() {
            if typen[i] != BidiKlasse::On {
                continue; // BD14/BD15: nur ON
            }
            match klammer(zeichen[i]) {
                Some((Klammerart::Oeffnend, partnerzeichen)) => {
                    if stapel_klammer.len() >= MAX_KLAMMERN {
                        klammer_ueberlauf = true;
                        break;
                    }
                    stapel_klammer.push((partnerzeichen, p));
                }
                Some((Klammerart::Schliessend, _)) => {
                    let treffer = stapel_klammer.iter().rposition(|&(partnerzeichen, _)| {
                        partnerzeichen == zeichen[i]
                            || kanonisch(partnerzeichen) == kanonisch(zeichen[i])
                    });
                    if let Some(pos) = treffer {
                        let (_, p_auf) = stapel_klammer[pos];
                        paare.push((p_auf, p));
                        stapel_klammer.truncate(pos);
                    }
                }
                None => {}
            }
        }
        if klammer_ueberlauf {
            paare.clear();
        }
        paare.sort_unstable();
        for &(p_auf, p_zu) in &paare {
            // Innerhalb: starke Typen suchen (EN/AN zaehlen als R).
            let mut gefunden_innen: Option<BidiKlasse> = None;
            for p in p_auf + 1..p_zu {
                match typen[seq[p]] {
                    BidiKlasse::L => {
                        if einbettung == BidiKlasse::L {
                            gefunden_innen = Some(BidiKlasse::L);
                            break;
                        }
                        gefunden_innen = Some(BidiKlasse::L);
                    }
                    BidiKlasse::R | BidiKlasse::En | BidiKlasse::An => {
                        if einbettung == BidiKlasse::R {
                            gefunden_innen = Some(BidiKlasse::R);
                            break;
                        }
                        gefunden_innen = Some(BidiKlasse::R);
                    }
                    _ => {}
                }
            }
            let richtung = match gefunden_innen {
                Some(typ) if typ == einbettung => typ,
                Some(gegenteilig) => {
                    // Kontext davor suchen (sos, wenn keiner).
                    let mut vorher = None;
                    for p in (0..p_auf).rev() {
                        match typen[seq[p]] {
                            BidiKlasse::L => {
                                vorher = Some(BidiKlasse::L);
                                break;
                            }
                            BidiKlasse::R | BidiKlasse::En | BidiKlasse::An => {
                                vorher = Some(BidiKlasse::R);
                                break;
                            }
                            _ => {}
                        }
                    }
                    let vorher = vorher.unwrap_or(sos);
                    if vorher == gegenteilig {
                        gegenteilig
                    } else {
                        einbettung
                    }
                }
                None => continue,
            };
            typen[seq[p_auf]] = richtung;
            typen[seq[p_zu]] = richtung;
            // NSM nach einer geaenderten Klammer uebernimmt deren Typ.
            for &p in &[p_auf, p_zu] {
                let mut k = p + 1;
                while k < seq.len() && klassen[seq[k]] == BidiKlasse::Nsm {
                    typen[seq[k]] = richtung;
                    k += 1;
                }
            }
        }

        // --------------------------------------------------------------
        // N1/N2: neutrale Typen.
        // --------------------------------------------------------------
        let ist_neutral = |t: BidiKlasse| {
            matches!(
                t,
                BidiKlasse::B
                    | BidiKlasse::S
                    | BidiKlasse::Ws
                    | BidiKlasse::On
                    | BidiKlasse::Fsi
                    | BidiKlasse::Lri
                    | BidiKlasse::Rli
                    | BidiKlasse::Pdi
            )
        };
        let richtung_von = |t: BidiKlasse| -> BidiKlasse {
            match t {
                BidiKlasse::L => BidiKlasse::L,
                BidiKlasse::R | BidiKlasse::En | BidiKlasse::An => BidiKlasse::R,
                _ => t,
            }
        };
        let mut p = 0;
        while p < seq.len() {
            if !ist_neutral(typen[seq[p]]) {
                p += 1;
                continue;
            }
            let mut q = p;
            while q < seq.len() && ist_neutral(typen[seq[q]]) {
                q += 1;
            }
            let links = if p > 0 {
                richtung_von(typen[seq[p - 1]])
            } else {
                sos
            };
            let rechts = if q < seq.len() {
                richtung_von(typen[seq[q]])
            } else {
                eos
            };
            let neu = if links == rechts { links } else { einbettung };
            for k in p..q {
                typen[seq[k]] = neu;
            }
            p = q;
        }

        // --------------------------------------------------------------
        // I1/I2: implizite Ebenen.
        // --------------------------------------------------------------
        for &i in seq {
            let k = pos_von[i];
            let level = ebenen_x10[k];
            let typ = typen[i];
            if level % 2 == 0 {
                match typ {
                    BidiKlasse::R => ebenen_x10[k] = level + 1,
                    BidiKlasse::An | BidiKlasse::En => ebenen_x10[k] = level + 2,
                    _ => {}
                }
            } else if matches!(typ, BidiKlasse::L | BidiKlasse::En | BidiKlasse::An) {
                ebenen_x10[k] = level + 1;
            }
        }
    }

    // ------------------------------------------------------------------
    // Zurueck auf die Originalpositionen.
    // ------------------------------------------------------------------
    for (k, &i) in sichtbar.iter().enumerate() {
        ebenen[i] = Some(ebenen_x10[k]);
    }

    // ------------------------------------------------------------------
    // L1: Absatztrenner und abschliessende Leerzeichen auf Absatzebene.
    // ------------------------------------------------------------------
    let ist_l1_zeichen = |k: BidiKlasse| {
        matches!(
            k,
            BidiKlasse::Ws | BidiKlasse::Fsi | BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Pdi
        )
    };
    let mut zuruecksetzen = vec![false; n];
    for i in 0..n {
        if matches!(klassen[i], BidiKlasse::B | BidiKlasse::S) {
            zuruecksetzen[i] = true;
            let mut j = i;
            while j > 0 && ist_l1_zeichen(klassen[j - 1]) {
                j -= 1;
                zuruecksetzen[j] = true;
            }
        }
    }
    let mut j = n;
    while j > 0 && ist_l1_zeichen(klassen[j - 1]) {
        j -= 1;
        zuruecksetzen[j] = true;
    }
    for (k, &i) in sichtbar.iter().enumerate() {
        if zuruecksetzen[i] {
            ebenen_x10[k] = ebene;
        }
    }
    for (k, &i) in sichtbar.iter().enumerate() {
        ebenen[i] = Some(ebenen_x10[k]);
    }

    // ------------------------------------------------------------------
    // L2: Neuordnung.
    // ------------------------------------------------------------------
    let mut reihenfolge = sichtbar.clone();
    let max_ebene = reihenfolge
        .iter()
        .map(|&i| ebenen_x10[pos_von[i]])
        .max()
        .unwrap_or(ebene);
    let min_ungerade = reihenfolge
        .iter()
        .map(|&i| ebenen_x10[pos_von[i]])
        .filter(|l| l % 2 == 1)
        .min();
    if let Some(min_ungerade) = min_ungerade {
        for level in (min_ungerade..=max_ebene).rev() {
            let mut p = 0;
            while p < reihenfolge.len() {
                let k = pos_von[reihenfolge[p]];
                if ebenen_x10[k] >= level {
                    let mut q = p;
                    while q < reihenfolge.len() {
                        let kq = pos_von[reihenfolge[q]];
                        if ebenen_x10[kq] >= level {
                            q += 1;
                        } else {
                            break;
                        }
                    }
                    reihenfolge[p..q].reverse();
                    p = q;
                } else {
                    p += 1;
                }
            }
        }
    }

    AbsatzAnalys {
        ebene,
        klassen,
        typen,
        ebenen,
        reihenfolge,
    }
}

/// Ordnet einen Absatz in die visuelle Reihenfolge um (Textform).
///
/// Wendet zusaetzlich die Anzeige-Regel L3 an: Kombinationszeichen
/// bleiben direkt hinter ihrer Basis, auch wenn die Neuordnung sie
/// davor gestellt haette. Fuer die reine L2-Reihenfolge siehe
/// [`AbsatzAnalys::reihenfolge`].
pub fn umordnen(text: &str) -> String {
    let analys = analysiere(text, Absatzrichtung::Automatisch);
    let zeichen: Vec<char> = text.chars().collect();
    let mut reihe = analys.reihenfolge.clone();
    l3_korrigieren(&analys.klassen, &mut reihe);
    reihe.iter().map(|&i| zeichen[i]).collect()
}

/// L3: Kombinationszeichen hinter ihre Basis ziehen.
fn l3_korrigieren(klassen: &[BidiKlasse], reihe: &mut Vec<usize>) {
    let n = klassen.len();
    let mut i = 0;
    while i < n {
        if klassen[i] != BidiKlasse::Nsm {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && klassen[i] == BidiKlasse::Nsm {
            i += 1;
        }
        if start == 0 {
            continue;
        }
        let basis = start - 1;
        if klassen[basis] == BidiKlasse::Nsm {
            continue;
        }
        let pos_basis = reihe.iter().position(|&x| x == basis);
        let pos_erste_marke = reihe.iter().position(|&x| x == start);
        if let (Some(pb), Some(pm)) = (pos_basis, pos_erste_marke) {
            if pm < pb {
                let marken: Vec<usize> = (start..i).collect();
                reihe.retain(|x| !marken.contains(x));
                let pb = reihe
                    .iter()
                    .position(|&x| x == basis)
                    .expect("Basis ist noch da");
                for (k, &m) in marken.iter().enumerate() {
                    reihe.insert(pb + 1 + k, m);
                }
            }
        }
    }
}

/// P2/P3 auf `klassen[von..bis]`.
fn p2p3(klassen: &[BidiKlasse], partner: &[Option<usize>], von: usize, bis: usize) -> u8 {
    let mut i = von;
    while i < bis {
        match klassen[i] {
            BidiKlasse::L => return 0,
            BidiKlasse::R | BidiKlasse::Al => return 1,
            BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi => {
                if let Some(pdi) = partner[i] {
                    if pdi < bis {
                        i = pdi + 1;
                        continue;
                    }
                    return 0;
                }
                return 0;
            }
            _ => {}
        }
        i += 1;
    }
    0
}

/// BD9: passender PDI je Isolationsanfang.
fn isolat_partner(klassen: &[BidiKlasse]) -> Vec<Option<usize>> {
    let mut partner = vec![None; klassen.len()];
    let mut stapel: Vec<usize> = Vec::new();
    for (i, &k) in klassen.iter().enumerate() {
        match k {
            BidiKlasse::Lri | BidiKlasse::Rli | BidiKlasse::Fsi => stapel.push(i),
            BidiKlasse::Pdi => {
                if let Some(j) = stapel.pop() {
                    partner[j] = Some(i);
                }
            }
            _ => {}
        }
    }
    partner
}
