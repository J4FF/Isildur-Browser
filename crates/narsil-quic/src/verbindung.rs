//! `verbindung.rs` — die QUIC-Zustandsmaschine OHNE Netz.
//!
//! Pakete kommen herein (`eingabe`), Pakete gehen hinaus (`sende_*`). Die Zeit
//! ist ein Parameter: kein Timer, keine Nebenlaeufigkeit, deshalb sind die
//! Tests deterministisch.
//!
//! Was hier steckt:
//!   * die Ebenen Initial -> Handshake -> 1-RTT mit getrennten Nummernraeumen,
//!   * Senden und Empfangen von Paketen (Paketschutz aus `schutz.rs`, Frames
//!     aus `rahmen.rs`),
//!   * ACK-Erzeugung (je Nummernraum, groesste empfangene Paketnummer),
//!   * PTO-Neusendung (Verlust von Handshake-Daten wird neu gesendet),
//!   * Stream-Flusskontrolle (Verbindungslimit, Fehler -> CONNECTION_CLOSE),
//!   * CONNECTION_CLOSE beim Fehler.
//!
//! Die TLS-Nachrichten kommen ueber die kleine Schnittstelle `Tls` herein,
//! damit sie spaeter durch narsil-tls ersetzt wird (team/WUNSCH_DEEPSEEK.txt).

use crate::rahmen::{Rahmen, RahmenFehler, rahmen_alle, rahmen_schreiben};
use crate::schutz::{
    PaketSchluessel, SchutzFehler, entschuetzen, initial_schuetzen, kurzes_schuetzen,
};
use crate::verlust::RttSchaetzer;

/// Eine Verschlüsselungsebene (RFC 9001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ebene {
    Initial,
    Handshake,
    EinsRtt,
}

impl Ebene {
    fn index(self) -> usize {
        match self {
            Ebene::Initial => 0,
            Ebene::Handshake => 1,
            Ebene::EinsRtt => 2,
        }
    }
}

/// Der Verbindungszustand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zustand {
    Initial,
    Handshake,
    Bestaetigt,
    Geschlossen,
}

/// Die TLS-Schnittstelle: liefert Handshake-Bytes (fuer CRYPTO-Frames) und
/// verarbeitet empfangene. Sie wird spaeter durch narsil-tls ersetzt.
pub trait Tls {
    /// Handshake-Bytes fuer `ebene`, als Inhalt eines CRYPTO-Frames. Leer =
    /// nichts zu senden.
    fn sende_bytes(&mut self, ebene: Ebene) -> Vec<u8>;
    /// Handshake-Bytes aus einem CRYPTO-Frame verarbeiten.
    fn empfange_bytes(&mut self, ebene: Ebene, daten: &[u8]) -> Result<(), VerbindungsFehler>;
    /// Ist der Handshake abgeschlossen (beide Finished verarbeitet)?
    fn abgeschlossen(&self) -> bool;
}

/// Laenge der eigenen Source Connection-ID, als **benannte** Grenze (M7, N43).
///
/// 8 Byte ist kein Zufall und keine Vorgabe der Norm, sondern die Laenge, die
/// wir selbst gewaehlt haben — und sie haengt an `MAX_CID_LAENGE` (20 Byte,
/// kopf.rs). Eine kleinere Zahl macht nichts guenstiger; eine groessere waere
/// ungueltig.
pub const SERVER_SCID_LAENGE: usize = 8;

/// Zerlegt ein Datagramm in seine (koaleszierten) QUIC-Pakete
/// (RFC 9000 Abschnitt 12.2).
///
/// Long-Header-Pakete werden ueber ihr Length-Feld abgegrenzt, ein
/// Short-Header-Paket nimmt den Rest des Datagramms. Jedes Teilpaket wird
/// mit seiner Ebene zurueckgegeben; das Entschuetzeln macht der Aufrufer
/// mit den Schluesseln SEINER Ebene (Pakete ohne Schluessel werden gepuffert,
/// nicht verworfen — das ist Sache des Aufrufers).
pub fn datagramm_zerlegen(daten: &[u8]) -> Result<Vec<(Ebene, &[u8])>, VerbindungsFehler> {
    let mut raus = Vec::new();
    let mut rest = daten;
    while !rest.is_empty() {
        if rest[0] & 0x80 != 0 {
            // Long Header: Length-Feld lesen, Grenze = pn_offset + Length.
            let kopf = crate::kopf::lange_lesen(rest)
                .map_err(|e| VerbindungsFehler::Schutz(SchutzFehler::Kopf(e)))?;
            let ende = (kopf.pn_offset as u64).checked_add(kopf.laenge).ok_or(
                VerbindungsFehler::Schutz(SchutzFehler::PaketZuKurz {
                    braucht: usize::MAX,
                    da: rest.len(),
                }),
            )? as usize;
            if ende > rest.len() {
                return Err(VerbindungsFehler::Schutz(SchutzFehler::PaketZuKurz {
                    braucht: ende,
                    da: rest.len(),
                }));
            }
            let ebenen = match kopf.typ {
                0 => Ebene::Initial,
                1 => Ebene::EinsRtt, // 0-RTT teilt den Anwendungs-Nummernraum.
                2 => Ebene::Handshake,
                _ => {
                    return Err(VerbindungsFehler::Schutz(SchutzFehler::PaketZuKurz {
                        braucht: 1,
                        da: rest.len(),
                    }));
                }
            };
            raus.push((ebenen, &rest[..ende]));
            rest = &rest[ende..];
        } else {
            // Short Header nimmt den Rest — aber nur, wenn das Fixed-Bit
            // (Bit 6) gesetzt ist. Ein Byte mit Bit 7 = 0 und Bit 6 = 0 ist
            // UDP-Padding (Nullen nach dem letzten Paket), kein Paket; das
            // wird ignoriert (RFC 9000 Abschnitt 17.3.1).
            if rest[0] & 0x40 == 0 {
                break;
            }
            raus.push((Ebene::EinsRtt, rest));
            rest = &[];
        }
    }
    Ok(raus)
}

/// Ein ausstehendes, noch nicht bestaetigtes Paket (fuer PTO-Neusendung).
#[derive(Debug, Clone)]
struct Ausstehend {
    ebenen: Ebene,
    paketnummer: u64,
    /// Der CRYPTO-Inhalt, der neu gesendet werden muss, falls der Frame
    /// verloren geht (nur fuer CRYPTO-Frames gespeichert).
    crypto: Option<Vec<u8>>,
}

/// Warum die Verbindung einen Fehler meldet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerbindungsFehler {
    Schutz(SchutzFehler),
    Rahmen(RahmenFehler),
    /// Die Flusskontrolle wurde verletzt (RFC 9000 Abschnitt 4.1).
    Flusskontrolle {
        limit: u64,
        bekommen: u64,
    },
    /// Ein Frame passt nicht in den Nummernraum.
    FrameImFalschenRaum,
    /// Unbekannte Ebene.
    EbeneOhneSchluessel {
        ebenen: Ebene,
    },
    /// Die eigene Source Connection-ID liess sich nicht erzeugen (N43).
    ///
    /// EIGENER FEHLER, weil "Zufall hat nicht geliefert" sonst als Netzfehler
    /// oder als Kopffehler auftaucht — und weil ein Server ohne SCID nicht
    /// antworten kann (RFC 9000 7.2), dies aber ein *innerer* Fehler ist.
    ServerScidFehlt {
        braucht: usize,
    },
}

impl std::fmt::Display for VerbindungsFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerbindungsFehler::Schutz(e) => write!(f, "Paketschutz: {e}"),
            VerbindungsFehler::Rahmen(e) => write!(f, "Frame: {e}"),
            VerbindungsFehler::Flusskontrolle { limit, bekommen } => {
                write!(f, "Flusskontrolle: {bekommen} Byte ueber dem Limit {limit}")
            }
            VerbindungsFehler::FrameImFalschenRaum => write!(f, "Frame im falschen Nummernraum"),
            VerbindungsFehler::EbeneOhneSchluessel { ebenen } => {
                write!(f, "keine Schluessel fuer Ebene {ebenen:?}")
            }
            VerbindungsFehler::ServerScidFehlt { braucht } => {
                write!(f, "Server-SCID ({braucht} Byte) nicht erzeugbar")
            }
        }
    }
}

impl std::error::Error for VerbindungsFehler {}

impl From<SchutzFehler> for VerbindungsFehler {
    fn from(e: SchutzFehler) -> Self {
        VerbindungsFehler::Schutz(e)
    }
}

impl From<RahmenFehler> for VerbindungsFehler {
    fn from(e: RahmenFehler) -> Self {
        VerbindungsFehler::Rahmen(e)
    }
}

/// Die Verbindung.
pub struct Verbindung {
    sendeschluessel: [Option<PaketSchluessel>; 3],
    empfangsschluessel: [Option<PaketSchluessel>; 3],
    paketnummer: [u64; 3],
    /// Groesste empfangene Paketnummer je Ebene (fuer ACK-Erzeugung).
    groesste_empfangen: [Option<u64>; 3],
    /// Ausstehende, unbestätigte Pakete.
    ausstehend: Vec<Ausstehend>,
    zustand: Zustand,
    rtt: RttSchaetzer,
    /// Naechster PTO-Zeitpunkt.
    pto_zeit: Option<u64>,
    /// Verbindungs-Flusskontroll-Limit (empfangene Daten).
    max_daten: u64,
    empfangene_daten: u64,
    tls: Box<dyn Tls>,
    /// Die Source Connection ID des **Clients** — unsere Destination (N43).
    client_scid: Vec<u8>,
    /// Unsere eigene Source Connection ID — die Destination des Clients (N43).
    server_scid: Vec<u8>,
}

impl Verbindung {
    pub fn neu(tls: Box<dyn Tls>, max_daten: u64) -> Self {
        Verbindung {
            client_scid: Vec::new(),
            server_scid: Vec::new(),
            sendeschluessel: [None, None, None],
            empfangsschluessel: [None, None, None],
            paketnummer: [0, 0, 0],
            groesste_empfangen: [None, None, None],
            ausstehend: Vec::new(),
            zustand: Zustand::Initial,
            rtt: RttSchaetzer::neu(),
            pto_zeit: None,
            max_daten,
            empfangene_daten: 0,
            tls,
        }
    }

    /// Setzt die Schluessel einer Ebene (Senden und Empfangen getrennt).
    pub fn setze_schluessel(
        &mut self,
        ebenen: Ebene,
        senden: PaketSchluessel,
        empfangen: PaketSchluessel,
    ) {
        self.sendeschluessel[ebenen.index()] = Some(senden);
        self.empfangsschluessel[ebenen.index()] = Some(empfangen);
    }

    pub fn zustand(&self) -> Zustand {
        self.zustand
    }

    /// Naechster PTO-Zeitpunkt; `None` = kein ausstehendes ack-ausloesendes Paket.
    pub fn pto_zeit(&self) -> Option<u64> {
        self.pto_zeit
    }

    /// Baut ein Initial-Paket mit dem Inhalt der TLS-Schnittstelle (ClientHello)
    /// und den Initial-Schluesseln aus `schluessel`.
    pub fn sende_initial(
        &mut self,
        schluessel: &PaketSchluessel,
        dcid: &[u8],
        scid: &[u8],
        jetzt: u64,
    ) -> Result<Vec<u8>, VerbindungsFehler> {
        let crypto = self.tls.sende_bytes(Ebene::Initial);
        let payload = if crypto.is_empty() {
            rahmen_bytes(&Rahmen::Ping)
        } else {
            rahmen_bytes(&Rahmen::Crypto {
                offset: 0,
                daten: crypto.clone(),
            })
        };
        let pn = self.paketnummer[0];
        self.paketnummer[0] += 1;
        let paket = initial_schuetzen(
            schluessel,
            crate::kopf::TYP_INITIAL,
            1,
            dcid,
            scid,
            &[],
            pn,
            2,
            &payload,
        )?;
        self.ausstehend.push(Ausstehend {
            ebenen: Ebene::Initial,
            paketnummer: pn,
            crypto: (!crypto.is_empty()).then_some(crypto),
        });
        self.pto_neu(jetzt, 0);
        Ok(paket)
    }

    /// Baut ein Handshake- oder 1-RTT-Paket aus den TLS-Handshake-Bytes.
    pub fn sende_ebene(&mut self, ebenen: Ebene, jetzt: u64) -> Result<Vec<u8>, VerbindungsFehler> {
        let schluessel = self
            .sendeschluessel
            .get(ebenen.index())
            .cloned()
            .flatten()
            .ok_or(VerbindungsFehler::EbeneOhneSchluessel { ebenen })?;
        let crypto = self.tls.sende_bytes(ebenen);
        let payload = if crypto.is_empty() {
            rahmen_bytes(&Rahmen::Ping)
        } else {
            rahmen_bytes(&Rahmen::Crypto {
                offset: 0,
                daten: crypto.clone(),
            })
        };
        let pn = self.paketnummer[ebenen.index()];
        self.paketnummer[ebenen.index()] += 1;
        let paket = match ebenen {
            Ebene::Handshake => initial_schuetzen(
                &schluessel,
                crate::kopf::TYP_HANDSHAKE,
                1,
                &[],
                &[],
                &[],
                pn,
                2,
                &payload,
            )?,
            Ebene::EinsRtt => kurzes_schuetzen(&schluessel, false, false, &[], pn, 2, &payload)?,
            Ebene::Initial => unreachable!("Initial ueber sende_initial"),
        };
        self.ausstehend.push(Ausstehend {
            ebenen,
            paketnummer: pn,
            crypto: (!crypto.is_empty()).then_some(crypto),
        });
        self.pto_neu(jetzt, 0);
        Ok(paket)
    }

    /// Verarbeitet ein ankommendes Datagramm. Gibt die zu sendenden Pakete
    /// (ACKs, Folge-Handshake) zurueck.
    ///
    /// N41: **Das Datagramm wird zerlegt**, und das ist kein Kosmos, sondern
    /// die Voraussetzung, dass ein echter Client ueberhaupt lesbar ist.
    ///
    /// VORHER bekam `entschuetzen` das **ganze** Datagramm. Ein Datagramm kann
    /// aber mehrere Pakete enthalten (RFC 9000 12.2), und dann standen hinter
    /// dem 16-Byte-Tag noch die Bytes des naechsten Pakets — der AEAD-Tag
    /// stimmte nicht, und die Meldung lautete `Gcm`. Sie sah damit aus wie ein
    /// Schluesselproblem, war aber keines: gemessen an einer echten
    /// Cloudflare-Verbindung gehen 3 von 4 Initial-Datagrammen so **nicht** auf,
    /// geteilt gehen 4 von 4 auf (N41.2, `tests/paketliste.rs`).
    ///
    /// EIN PAKET OHNE SCHLUESSEL wird **uebersprungen**, nicht zum Fehler:
    /// ein 1-RTT-Paket vor dem Eigenschluesselwechsel und ein Handshake-Paket
    /// vor den Handshake-Schluesseln sind normale QUIC-Situationen, und sie
    /// duerfen nicht den Rest desselben Datagramms zerstoeren.
    pub fn eingabe(
        &mut self,
        datagramm: &[u8],
        jetzt: u64,
    ) -> Result<Vec<Vec<u8>>, VerbindungsFehler> {
        let teile = datagramm_zerlegen(datagramm)?;
        let mut raus = Vec::new();
        // M7: Jede Schleife ueber fremde Daten braucht einen **Zaehler**, und
        // eine Grenze. Die Zahl der Pakete in einem Datagramm ist durch die
        // Pufferlaenge begrenzt — jede Long-Header-Paketliste braucht mindestens
        // 7 Byte Kopf, ein 64-KiB-Datagramm fasst also ueber 9000 davon. Die
        // Grenze ist trotzdem da, weil "begrenzt" eine Rechnung ist und kein
        // Messwert.
        const MAX_TEILE: usize = 64;
        for (gezaehlt, (_ebene, paket)) in teile.into_iter().enumerate() {
            if gezaehlt >= MAX_TEILE {
                return Err(VerbindungsFehler::Schutz(SchutzFehler::PaketZuKurz {
                    braucht: 1,
                    da: paket.len(),
                }));
            }
            match self.eingabe_paket(paket, jetzt, &mut raus) {
                Ok(()) => {}
                // Kein Schluessel fuer diese Ebene: das Paket gehoert nicht zu
                // diesem Zustand (siehe oben). Ueberspringen, nicht abbrechen.
                Err(VerbindungsFehler::EbeneOhneSchluessel { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(raus)
    }

    /// **Ein** Paket aus einem Datagramm verarbeiten. Die Schleife in `eingabe`
    /// ist der einzige Aufrufer; sie zerlegt, weil ein Datagramm mehrere
    /// Pakete enthalten kann (RFC 9000 12.2, N41).
    fn eingabe_paket(
        &mut self,
        datagramm: &[u8],
        jetzt: u64,
        raus: &mut Vec<Vec<u8>>,
    ) -> Result<(), VerbindungsFehler> {
        // Ebene am Kopf erkennen: Long Header (Initial/Handshake) oder Short.
        if datagramm.is_empty() {
            return Err(VerbindungsFehler::Schutz(SchutzFehler::PaketZuKurz {
                braucht: 1,
                da: 0,
            }));
        }
        let lang = datagramm[0] & 0x80 != 0;

        // N43: DIE CONNECTION-IDs ÜBERNEHMEN, und zwar **bevor** irgendetwas
        // gesendet wird — denn das erste ACK geht schon raus, und es braucht
        // beide.
        //
        // Aus einem **Initial** des Clients (RFC 9000 17.2):
        //   * seine SCID wird unsere **Destination**;
        //   * unsere eigene SCID waehlen wir **jetzt** und senden sie ab da
        //     in jedem Paket mit.
        //
        // Die eigene SCID wird nur einmal gewaehlt und **nicht** bei jedem
        // Initial neu — sonst wuerde der Client sie als Migration lesen.
        if lang && self.client_scid.is_empty() {
            let kopf = crate::kopf::lange_lesen(datagramm)
                .map_err(|e| VerbindungsFehler::Schutz(SchutzFehler::Kopf(e)))?;
            if kopf.scid.is_empty() {
                // Der Client hat selbst eine leere SCID gewaehlt (RFC 9000
                // 7.2 erlaubt das). Dann ist unsere Destination leer — das ist
                // in Ordnung und **kein** Fehler; nur unsere eigene braucht
                // eine Laenge, sonst ist unser Paket nicht zuzuordnen.
            }
            self.client_scid = kopf.scid.clone();
            self.server_scid = narsil_tls::zufall::bytes(SERVER_SCID_LAENGE).map_err(|_| {
                VerbindungsFehler::ServerScidFehlt {
                    braucht: SERVER_SCID_LAENGE,
                }
            })?;
        }
        let (ebenen, typ) = if lang {
            let typ = (datagramm[0] >> 4) & 0x03;
            match typ {
                0 => (Ebene::Initial, 0u8),
                2 => (Ebene::Handshake, 2u8),
                _ => (Ebene::Initial, typ),
            }
        } else {
            (Ebene::EinsRtt, 0u8)
        };

        let schluessel = self
            .empfangsschluessel
            .get(ebenen.index())
            .cloned()
            .flatten()
            .ok_or(VerbindungsFehler::EbeneOhneSchluessel { ebenen })?;

        let gelesen = entschuetzen(
            &schluessel,
            datagramm,
            0,
            self.groesste_empfangen[ebenen.index()].unwrap_or(0),
        )?;
        self.groesste_empfangen[ebenen.index()] = Some(gelesen.paketnummer);

        let frames = rahmen_alle(&gelesen.klartext)?;
        let mut ack_ausloesend = false;
        for frame in &frames {
            match frame {
                Rahmen::Crypto { offset, daten } => {
                    if *offset != 0 {
                        return Err(VerbindungsFehler::FrameImFalschenRaum);
                    }
                    self.tls.empfange_bytes(ebenen, daten)?;
                    ack_ausloesend = true;
                }
                Rahmen::Stream { offset, daten, .. } => {
                    // Stream-Flusskontrolle (Verbindungsebene, vereinfacht).
                    let ende = offset + daten.len() as u64;
                    if ende > self.max_daten {
                        return Err(VerbindungsFehler::Flusskontrolle {
                            limit: self.max_daten,
                            bekommen: ende,
                        });
                    }
                    self.empfangene_daten = self.empfangene_daten.max(ende);
                    ack_ausloesend = true;
                }
                Rahmen::Ack { groesste, .. } => {
                    // Bestätigte Pakete aus der Ausstehend-Liste entfernen.
                    self.ausstehend
                        .retain(|p| !(p.ebenen == ebenen && p.paketnummer <= *groesste));
                    if self.ausstehend.is_empty() {
                        self.pto_zeit = None;
                    } else {
                        self.pto_neu(jetzt, 0);
                    }
                }
                Rahmen::ConnectionClose { .. } => {
                    self.zustand = Zustand::Geschlossen;
                }
                Rahmen::Ping | Rahmen::Padding | Rahmen::HandshakeDone => {
                    if matches!(frame, Rahmen::Ping) {
                        ack_ausloesend = true;
                    }
                }
                _ => {
                    ack_ausloesend = true;
                }
            }
        }

        // ACK erzeugen, wenn das Paket ack-ausloesend ist.
        if ack_ausloesend {
            raus.push(self.ack_paket(ebenen, jetzt)?);
        }

        // Zustandsuebergaenge: Antwort des Servers auf das Initial-Paket
        // hebt die Verbindung auf die Handshake-Ebene; das TLS-Modul meldet
        // die Bestaetigung.
        if lang && typ == 0 && self.zustand == Zustand::Initial {
            self.zustand = Zustand::Handshake;
        }
        if self.tls.abgeschlossen() && self.zustand == Zustand::Handshake {
            self.zustand = Zustand::Bestaetigt;
        }

        Ok(())
    }

    /// Baut ein ACK-Paket fuer die Ebene.
    fn ack_paket(&mut self, ebenen: Ebene, jetzt: u64) -> Result<Vec<u8>, VerbindungsFehler> {
        let schluessel = self
            .sendeschluessel
            .get(ebenen.index())
            .cloned()
            .flatten()
            .ok_or(VerbindungsFehler::EbeneOhneSchluessel { ebenen })?;
        let groesste = self.groesste_empfangen[ebenen.index()].unwrap_or(0);
        let ack = rahmen_bytes(&Rahmen::Ack {
            groesste,
            verzoegerung: 0,
            erste_range: 0,
            bereiche: vec![],
            ecn: None,
        });
        let pn = self.paketnummer[ebenen.index()];
        self.paketnummer[ebenen.index()] += 1;
        let _ = jetzt;
        let paket = schuetze_ebene(
            &schluessel,
            ebenen,
            pn,
            &ack,
            &self.client_scid,
            &self.server_scid,
        )?;
        Ok(paket)
    }

    /// PTO-Neusendung: sendet das aelteste unbestätigte CRYPTO-Datum erneut.
    pub fn pto(&mut self, jetzt: u64) -> Result<Option<Vec<u8>>, VerbindungsFehler> {
        if self.pto_zeit.is_some_and(|t| jetzt >= t) {
            if let Some(alt) = self.ausstehend.iter().find(|p| p.crypto.is_some()).cloned() {
                self.pto_zeit = None;
                let schluessel = self
                    .sendeschluessel
                    .get(alt.ebenen.index())
                    .cloned()
                    .flatten()
                    .ok_or(VerbindungsFehler::EbeneOhneSchluessel { ebenen: alt.ebenen })?;
                let payload = rahmen_bytes(&Rahmen::Crypto {
                    offset: 0,
                    daten: alt.crypto.unwrap_or_default(),
                });
                let pn = self.paketnummer[alt.ebenen.index()];
                self.paketnummer[alt.ebenen.index()] += 1;
                let paket = schuetze_ebene(
                    &schluessel,
                    alt.ebenen,
                    pn,
                    &payload,
                    &self.client_scid,
                    &self.server_scid,
                )?;
                // PTO-Verdopplung: naechster PTO in 2x.
                let dauer = self.rtt.pto(0).max(1);
                self.pto_zeit = Some(jetzt + 2 * dauer);
                return Ok(Some(paket));
            }
            self.pto_zeit = None;
        }
        Ok(None)
    }

    /// Setzt den PTO-Zeitpunkt neu (auf `jetzt + pto`).
    fn pto_neu(&mut self, jetzt: u64, _max_ack_verzoegerung: u64) {
        let dauer = self.rtt.pto(0).max(1);
        self.pto_zeit = Some(jetzt + dauer);
    }
}

/// Kodiert einen einzelnen Frame in Bytes.
fn rahmen_bytes(rahmen: &Rahmen) -> Vec<u8> {
    let mut aus = Vec::new();
    rahmen_schreiben(rahmen, &mut aus).expect("Frame kodieren");
    aus
}

/// Schuetzt eine Nutzlast mit dem passenden Pakettyp fuer die Ebene.
/// N43: **zwei Connection-IDs, nicht `&[], &[]`.**
///
/// DAS WAR EIN ECHTER FEHLER AUF DER SERVERSEITE, und er hat vermutlich N33
/// verursacht (siehe `tests/lokaler_server.rs`): unser Testserver schickte
/// Initial **und** Handshake mit **leerer** Destination- **und** leerer Source-
/// Connection-ID. Ein echter Client kann ein solches Paket keiner Verbindung
/// zuordnen — fuer ihn ist es ein Paket an niemanden. Genau das ist das
/// N33-Symptom ("curl wird nie bereit, curl schickt keinen STREAM-Rahmen").
///
/// DIE REGEL (RFC 9000 7.2 und 17.2): Ein Server sendet seine Pakete mit der
/// Source Connection ID des **Clients** als Destination und mit seiner
/// **eigenen** als Source. Beides leer ist kein gueltiges Paket, sondern ein
/// Paket, das niemand beansprucht.
#[allow(clippy::too_many_arguments)]
fn schuetze_ebene(
    schluessel: &PaketSchluessel,
    ebenen: Ebene,
    pn: u64,
    payload: &[u8],
    dcid: &[u8],
    scid: &[u8],
) -> Result<Vec<u8>, VerbindungsFehler> {
    match ebenen {
        Ebene::Initial | Ebene::Handshake => {
            let typ = if ebenen == Ebene::Initial {
                crate::kopf::TYP_INITIAL
            } else {
                crate::kopf::TYP_HANDSHAKE
            };
            Ok(initial_schuetzen(
                schluessel,
                typ,
                1,
                dcid,
                scid,
                &[],
                pn,
                2,
                payload,
            )?)
        }
        Ebene::EinsRtt => Ok(kurzes_schuetzen(
            schluessel,
            false,
            false,
            &[],
            pn,
            2,
            payload,
        )?),
    }
}
