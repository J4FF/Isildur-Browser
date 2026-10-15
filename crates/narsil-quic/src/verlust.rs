//! Verlusterkennung und Staukontrolle nach RFC 9002.
//!
//! Was hier steht, folgt dem Pseudocode aus RFC 9002, Anhang A (Loss
//! Recovery) und Anhang B (Congestion Control, NewReno):
//!   * RTT-Schaetzung (smoothed_rtt, rttvar, min_rtt; Abschnitt 5.3),
//!   * PTO (Probe Timeout; Abschnitt 6.2.1),
//!   * Verlusterkennung ueber Paket- und Zeitschwelle (Abschnitt 6.1),
//!   * NewReno-Staukontrolle (Slow Start, Recovery, Congestion Avoidance;
//!     Abschnitt 7.3).
//!
//! Die Zeit ist eine ZAHL in Millisekunden, die der Aufrufer liefert. Es
//! gibt hier keine Uhr und keine Nebenlaeufigkeit — deshalb sind die Tests
//! deterministisch: dieselbe Ereignisfolge ergibt immer denselben Zustand.

/// Anfangswert fuer die RTT (RFC 9002 Abschnitt 6.2.2: 333 ms).
pub const INITIALE_RTT: u64 = 333;
/// Timer-Granularitaet (kGranularity, RFC 9002 Anhang A.2: 1 ms).
pub const KORN: u64 = 1;
/// Paket-Umordnungs-Schwelle (kPacketThreshold, RFC 9002 Abschnitt 6.1.1: 3).
pub const PAKET_SCHWELLE: u64 = 3;
/// Zeitschwelle als RTT-Vielfaches (kTimeThreshold, 9/8; Abschnitt 6.1.2).
pub const ZEIT_SCHWELLE_ZAHL: u64 = 9;
pub const ZEIT_SCHWELLE_NENNER: u64 = 8;
/// Dauerhaft-Stau-Schwelle (kPersistentCongestionThreshold, Abschnitt 7.6.1: 3).
pub const DAUERHAFT_SCHWELLE: u64 = 3;
/// Kleinster Datagramm-Inhalt (max_datagram_size, RFC 9000 Abschnitt 14.1).
pub const MAX_DATAGRAM: u64 = 1200;
/// Verlust-Reduktionsfaktor (kLossReductionFactor, 1/2; Anhang B.1).
pub const VERLUST_FAKTOR_ZAHL: u64 = 1;
pub const VERLUST_FAKTOR_NENNER: u64 = 2;

/// Ein gesendetes, noch nicht bestaetigtes Paket (fuer die Verlusterkennung).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gesendet {
    /// Paketnummer (monoton steigend im Nummernraum).
    pub paketnummer: u64,
    /// Sendezeitpunkt in Millisekunden (simulierte Uhr).
    pub zeit: u64,
    /// Groesse in Byte (fuer die Staukontrolle).
    pub bytes: u64,
    /// Loest das Paket ein ACK aus (ack-eliciting)?
    pub ack_ausloesend: bool,
}

/// RTT-Schaetzer (RFC 9002 Abschnitt 5.3, UpdateRtt aus Anhang A.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RttSchaetzer {
    /// Geglaettete RTT (smoothed_rtt).
    pub glatt: u64,
    /// Mittlere Abweichung (rttvar).
    pub schwankung: u64,
    /// Kleinste beobachtete RTT (min_rtt).
    pub min: u64,
    /// Letzte Probe (latest_rtt).
    pub letzter: u64,
    erste_probe: bool,
}

impl RttSchaetzer {
    /// Initialisiert mit `kInitialRtt` (RFC 9002 Abschnitt 5.3).
    pub fn neu() -> Self {
        RttSchaetzer {
            glatt: INITIALE_RTT,
            schwankung: INITIALE_RTT / 2,
            min: 0,
            letzter: 0,
            erste_probe: true,
        }
    }

    /// Verarbeitet eine RTT-Probe. `ack_verzoegerung` ist die vom Gegenueber
    /// gemeldete ACK-Verzoegerung (Mikrosekunden -> hier Millisekunden als
    /// Zahl), `handshake_bestaetigt` schaltet die Begrenzung auf
    /// `max_ack_verzoegerung` ein.
    pub fn probe(
        &mut self,
        letzter_rtt: u64,
        ack_verzoegerung: u64,
        handshake_bestaetigt: bool,
        max_ack_verzoegerung: u64,
    ) {
        self.letzter = letzter_rtt;
        if self.erste_probe {
            // Erste Probe: der Schaetzer verliert jede Vorgeschichte.
            self.min = letzter_rtt;
            self.glatt = letzter_rtt;
            self.schwankung = letzter_rtt / 2;
            self.erste_probe = false;
            return;
        }
        self.min = self.min.min(letzter_rtt);

        // ACK-Verzoegerung begrenzen und — falls plausibel — abziehen.
        let mut verzoegerung = ack_verzoegerung;
        if handshake_bestaetigt {
            verzoegerung = verzoegerung.min(max_ack_verzoegerung);
        }
        let mut angepasst = letzter_rtt;
        if letzter_rtt >= self.min + verzoegerung {
            angepasst = letzter_rtt - verzoegerung;
        }

        let abweichung = self.glatt.abs_diff(angepasst);
        self.schwankung = (3 * self.schwankung + abweichung) / 4;
        self.glatt = (7 * self.glatt + angepasst) / 8;
    }

    /// Der Probe Timeout in Millisekunden (RFC 9002 Abschnitt 6.2.1).
    pub fn pto(&self, max_ack_verzoegerung: u64) -> u64 {
        self.glatt + (4 * self.schwankung).max(KORN) + max_ack_verzoegerung
    }

    /// Die Zeitschwelle fuer Verlusterkennung (RFC 9002 Abschnitt 6.1.2):
    /// `max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)`.
    pub fn zeit_schwelle(&self) -> u64 {
        let groesste = self.letzter.max(self.glatt);
        (ZEIT_SCHWELLE_ZAHL * groesste / ZEIT_SCHWELLE_NENNER).max(KORN)
    }
}

impl Default for RttSchaetzer {
    fn default() -> Self {
        Self::neu()
    }
}

/// Erkennt verlorene Pakete (RFC 9002 Abschnitt 6.1, Anhang A.10).
///
/// Liefert die Indizes der verlorenen Pakete in `gesendet`. Ein Paket ist
/// verloren, wenn es unbestätigt ist, VOR einem bestätigten Paket gesendet
/// wurde und entweder die Paket- oder die Zeitschwelle reisst.
pub fn verlorene_pakete(
    gesendet: &[Gesendet],
    groesste_bestaetigt: u64,
    jetzt: u64,
    rtt: &RttSchaetzer,
) -> Vec<usize> {
    let verlust_verzug = rtt.zeit_schwelle();
    let verlust_zeit = jetzt.saturating_sub(verlust_verzug);

    let mut verloren = Vec::new();
    for (i, paket) in gesendet.iter().enumerate() {
        // Nur Pakete betrachten, die vor oder auf dem groessten bestätigten
        // liegen (RFC 9002 Anhang A.10: `packet_number <= largest_acked`).
        if paket.paketnummer > groesste_bestaetigt {
            continue;
        }
        let zeitschwelle_gerissen = paket.zeit <= verlust_zeit;
        let paketschwelle_gerissen = groesste_bestaetigt >= paket.paketnummer + PAKET_SCHWELLE;
        if zeitschwelle_gerissen || paketschwelle_gerissen {
            verloren.push(i);
        }
    }
    verloren
}

/// NewReno-Staukontrolle (RFC 9002 Abschnitt 7.3, Anhang B).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stau {
    /// Stau-Fenster in Byte (congestion_window).
    pub fenster: u64,
    /// Slow-Start-Schwelle in Byte (ssthresh).
    pub schwelle: u64,
    /// Bytes in Flug (bytes_in_flight).
    pub in_flug: u64,
    /// Startzeit der Erholungsphase; 0 = keine Erholung.
    erholung_beginn: u64,
    /// Groesse des Datagramms (max_datagram_size).
    pub max_datagram: u64,
}

impl Stau {
    /// Initialisiert das Fenster nach RFC 9002 Abschnitt 7.2:
    /// `min(10 * mds, max(2 * mds, 14720))`.
    pub fn neu(max_datagram: u64) -> Self {
        let fenster = (10 * max_datagram).min((2 * max_datagram).max(14_720));
        Stau {
            fenster,
            schwelle: u64::MAX,
            in_flug: 0,
            erholung_beginn: 0,
            max_datagram,
        }
    }

    /// Das kleinste erlaubte Fenster (RFC 9002 Abschnitt 7.2: 2 * mds).
    pub fn minimum(&self) -> u64 {
        2 * self.max_datagram
    }

    /// Ein Paket wurde gesendet (Anhang B.4).
    pub fn paket_gesendet(&mut self, bytes: u64) {
        self.in_flug += bytes;
    }

    /// Ein Paket wurde bestätigt (Anhang B.5). `zeit_gesendet` ist der
    /// Sendezeitpunkt des bestätigten Pakets, `jetzt` die aktuelle Zeit,
    /// `app_begrenzt` true, wenn der Sender nicht das ganze Fenster fuellt.
    pub fn paket_bestaetigt(&mut self, bytes: u64, zeit_gesendet: u64, app_begrenzt: bool) {
        self.in_flug = self.in_flug.saturating_sub(bytes);
        if app_begrenzt || self.in_erholung(zeit_gesendet) {
            return;
        }
        if self.fenster < self.schwelle {
            // Slow Start: exponentielles Wachstum.
            self.fenster += bytes;
        } else {
            // Congestion Avoidance: hoechstens ein Datagramm je Fenster.
            let wachstum = self.max_datagram * bytes / self.fenster;
            self.fenster += wachstum.max(1);
        }
    }

    /// Ein neues Stau-Ereignis (Verlust oder ECN-CE-Zunahme; Anhang B.6).
    /// Wenn schon eine Erholung laeuft, wird NICHT erneut reduziert.
    pub fn stau_ereignis(&mut self, jetzt: u64) {
        if self.erholung_beginn != 0 {
            return;
        }
        self.erholung_beginn = jetzt;
        self.schwelle = self.fenster * VERLUST_FAKTOR_ZAHL / VERLUST_FAKTOR_NENNER;
        self.fenster = self.schwelle.max(self.minimum());
    }

    /// Ist das Paket (an seinem Sendezeitpunkt) Teil der Erholungsphase?
    fn in_erholung(&self, zeit_gesendet: u64) -> bool {
        self.erholung_beginn != 0 && zeit_gesendet <= self.erholung_beginn
    }

    /// Beendet die Erholungsphase, wenn ein NACH ihrem Beginn gesendetes
    /// Paket bestätigt wurde (RFC 9002 Abschnitt 7.3.2).
    pub fn erholung_ende(&mut self) {
        self.erholung_beginn = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9002 Abschnitt 5.3: erste Probe setzt den Schaetzer zurueck,
    /// danach laeuft der EWMA.
    #[test]
    fn rtt_schaetzung_aus_rfc9002_5_3() {
        let mut r = RttSchaetzer::neu();
        // Initial: kInitialRtt = 333, rttvar = 166.
        assert_eq!((r.glatt, r.schwankung), (333, 166), "M7: Initialwert");

        // Erste Probe setzt zurueck.
        r.probe(100, 0, true, 25);
        assert_eq!(r.glatt, 100, "M7: smoothed nach 1. Probe");
        assert_eq!(r.schwankung, 50, "M7: rttvar = latest/2");
        assert_eq!(r.min, 100, "M7: min_rtt");

        // Zweite Probe: EWMA.
        //   smoothed = (7*100 + 120)/8 = 102, rttvar = (3*50 + 20)/4 = 42.
        r.probe(120, 0, true, 25);
        assert_eq!(r.glatt, 102, "M7: smoothed nach 2. Probe");
        assert_eq!(r.schwankung, 42, "M7: rttvar nach 2. Probe");

        // Dritte Probe mit ACK-Verzoegerung 10 (begrenzt auf 25, abgezogen).
        //   adjusted = 130 - 10 = 120; smoothed = (7*102+120)/8 = 104;
        //   rttvar = (3*42 + |102-120|)/4 = 36.
        r.probe(130, 10, true, 25);
        assert_eq!(r.glatt, 104, "M7: smoothed nach 3. Probe");
        assert_eq!(r.schwankung, 36, "M7: rttvar nach 3. Probe");
    }

    /// RFC 9002 Abschnitt 6.2.1: PTO = smoothed + max(4*rttvar, 1) + max_ack_delay.
    #[test]
    fn pto_aus_rfc9002_6_2_1() {
        let mut r = RttSchaetzer::neu();
        r.probe(100, 0, true, 25);
        // PTO = 100 + max(4*50, 1) + 25 = 100 + 200 + 25 = 325.
        assert_eq!(r.pto(25), 325, "M7: PTO");
        // Ohne max_ack_delay (Initial/Handshake): 100 + 200 = 300.
        assert_eq!(r.pto(0), 300, "M7: PTO ohne ACK-Verzoegerung");
    }

    /// RFC 9002 Abschnitt 6.1.1: Paketschwelle (3) meldet Paket 0 verloren,
    /// sobald Paket 3 bestätigt ist.
    #[test]
    fn verlust_ueber_paketschwelle() {
        let r = RttSchaetzer::neu();
        let gesendet = vec![
            Gesendet {
                paketnummer: 0,
                zeit: 0,
                bytes: 1200,
                ack_ausloesend: true,
            },
            Gesendet {
                paketnummer: 1,
                zeit: 10,
                bytes: 1200,
                ack_ausloesend: true,
            },
            Gesendet {
                paketnummer: 2,
                zeit: 20,
                bytes: 1200,
                ack_ausloesend: true,
            },
        ];
        // Paket 3 (nicht in der Liste) ist bestätigt -> groesste_bestaetigt = 3.
        let verloren = verlorene_pakete(&gesendet, 3, 0, &r);
        assert_eq!(verloren, vec![0], "M7: Paket 0 ist verloren (3 >= 0+3)");
        // Paket 1 und 2: 3 < 1+3 bzw. 3 < 2+3 -> noch nicht verloren.
    }

    /// RFC 9002 Abschnitt 6.1.2: Zeitschwelle meldet ein altes Paket verloren.
    #[test]
    fn verlust_ueber_zeitschwelle() {
        let mut r = RttSchaetzer::neu();
        r.probe(100, 0, true, 25); // glatt = letzter = 100 -> Schwelle 112 ms.
        let gesendet = vec![
            Gesendet {
                paketnummer: 0,
                zeit: 0,
                bytes: 1200,
                ack_ausloesend: true,
            },
            Gesendet {
                paketnummer: 1,
                zeit: 400,
                bytes: 1200,
                ack_ausloesend: true,
            },
        ];
        // groesste_bestaetigt = 1, jetzt = 500: Paket 0 ist 500 ms alt (> 112).
        let verloren = verlorene_pakete(&gesendet, 1, 500, &r);
        assert_eq!(verloren, vec![0], "M7: Paket 0 ist ueber die Zeitschwelle");
        // Paket 1 ist zu frisch (Alter 100 < 112) und nicht unter der
        // Paketschwelle (1 < 1+3).
    }

    /// RFC 9002 Abschnitt 7.2/7.3: NewReno-Fenster waechst im Slow Start und
    /// schrumpft bei Verlust auf die Haelfte.
    #[test]
    fn newreno_slow_start_und_verlust() {
        let mut s = Stau::neu(1200);
        // Initial: min(10*1200, max(2*1200, 14720)) = min(12000, 14720) = 12000.
        assert_eq!(s.fenster, 12_000, "M7: Initialfenster");
        assert_eq!(s.minimum(), 2_400, "M7: Minimum");

        // Slow Start: Fenster waechst um die bestätigten Bytes.
        s.paket_gesendet(1200);
        s.paket_bestaetigt(1200, 0, false);
        assert_eq!(s.fenster, 13_200, "M7: Slow Start +1200");
        assert_eq!(s.in_flug, 0, "M7: nichts mehr in Flug");

        // Verlust: das Fenster (13200) wird halbiert.
        s.stau_ereignis(100);
        assert_eq!(s.schwelle, 6_600, "M7: ssthresh halbiert");
        assert_eq!(s.fenster, 6_600, "M7: Fenster auf ssthresh");

        // Ein zweites Stau-Ereignis in derselben Erholung reduziert NICHT erneut.
        s.stau_ereignis(200);
        assert_eq!(s.fenster, 6_600, "M7: keine doppelte Reduktion");
    }

    /// RFC 9002 Anhang B.5: Congestion Avoidance waechst additiv.
    #[test]
    fn newreno_congestion_avoidance() {
        let mut s = Stau::neu(1200);
        s.schwelle = 5_000; // unter das Initialfenster, also Congestion Avoidance.
        s.paket_gesendet(1200);
        let vorher = s.fenster;
        s.paket_bestaetigt(1200, 0, false);
        // Wachstum = 1200 * 1200 / 12000 = 120.
        assert_eq!(s.fenster, vorher + 120, "M7: additiv +120");
    }
}
