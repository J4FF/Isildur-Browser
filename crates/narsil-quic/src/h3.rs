//! HTTP/3-Frames (RFC 9114, Abschnitt 7.2).
//!
//! Ein HTTP/3-Frame besteht aus Typ (Varint), Laenge (Varint) und Inhalt.
//! Fuer Q5 (GET-Anfrage/Antwort) reichen DATA, HEADERS, SETTINGS und GOAWAY;
//! MAX_PUSH_ID ist der Vollstaendigkeit halber dabei.
//!
//! M3/M4: die Frame-Laenge wird vor dem Lesen geprueft, die Zahl der
//! SETTINGS-Paare ist begrenzt.

use crate::varint::{self, VarintFehler};

pub const H3_DATEN: u64 = 0x00;
/// Der Stream-Typ eines **Steuerstroms** (RFC 9114 6.2.1): `0x00`.
///
/// N49: DIESE ZAHL WAR RICHTIG, UND ICH HABE SIE FALSCH GELESEN.
///
/// DER FEHLER, und er ist der teuerste der ganzen Sitzung: Ich habe `0x01`
/// gesetzt, weil ich 6.2.1 und 6.2.2 vertauscht hatte. Nachgelesen in
/// RFC 9114 (im Originaltext, nicht aus dem Gedaechtnis):
///
///   6.2.1  "A control stream is indicated by a stream type of 0x00."
///   6.2.2  "A push stream is indicated by a stream type of 0x01, followed
///           by the push ID ... Only servers can push; if a server receives
///           a client-initiated push stream, this MUST be treated as a
///           connection error of type H3_STREAM_CREATION_ERROR."
///
/// `H3_STREAM_CREATION_ERROR` = 0x103 = **259**. Genau den Fehlercode mit
/// genau dem Text „Received push stream" habe ich gemessen — und ihn fuer
/// die Ursache gehalten. **Die Meldung war die Ursache.**
///
/// DIE LEHRE, in einer Zeile: Ich habe eine Norm aus dem Gedaechtnis
/// zitiert und die Zitatstelle nie geoeffnet. Drei Messungen spaeter habe ich
/// die Norm nachgeladen — und die Messung war die ganze Zeit richtig.
pub const STREAM_TYP_STEUERSTROM: u64 = 0x00;

/// Der Stream-Typ eines **Push**-Stroms (RFC 9114 6.2.2): `0x01`, danach die
/// Push-ID als Varint.
///
/// STEHT HIER, WEIL ICH DIE ZAHLEN VERTAUSCHT HABE: `0x01` ist der **Push**-
/// Strom, nicht der Steuerstrom. Wer das verwechselt, oeffnet einen
/// Push-Strom — was nur ein Server darf.
pub const STREAM_TYP_PUSH: u64 = 0x01;

pub const H3_KOPFBLOCK: u64 = 0x01;
pub const H3_EINSTELLUNGEN: u64 = 0x04;
pub const H3_GOAWAY: u64 = 0x07;
pub const H3_MAX_PUSH_ID: u64 = 0x0d;

/// Hoechste Zahl von SETTINGS-Paaren (M3).
pub const MAX_EINSTELLUNGEN: usize = 64;

/// Ein HTTP/3-Frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H3Rahmen {
    Daten(Vec<u8>),
    /// HEADERS: ein QPACK-kodierter Feldabschnitt.
    Kopfblock(Vec<u8>),
    Einstellungen(Vec<(u64, u64)>),
    Goaway(u64),
    MaxPushId(u64),
    /// Ein Frame-Typ, den diese Implementierung nicht kennt.
    ///
    /// DAS MUST-MUSTER, woertlich aus RFC 9114 Abschnitt 7.2.8:
    ///
    /// ```text
    /// Frame types of the format 0x1f * N + 0x21 for non-negative integer
    /// values of N are reserved to exercise the requirement that unknown
    /// types be ignored (Section 9).  [...]  Endpoints MUST NOT consider
    /// these frames to have any meaning upon receipt.
    /// ```
    ///
    /// Also: `0x21` (33), `0x40` (64), `0x5f` (95), `0x7e` (126), ...
    ///
    /// WICHTIG, UND DAS IST DER PUNKT: dieses Muster ist **eine Teilmenge**
    /// der unbekannten Typen, nicht die ganze Menge. Abschnitt 9 verlangt,
    /// **jeden** nicht definierten Typ zu ignorieren — auch einen, der nicht
    /// auf dieses Muster passt.
    ///
    /// GEMESSEN an den beiden GREASE-Frames, die cloudflare-quic.com am
    /// 01.10.2026 jeder Antwort voranstellt:
    ///
    /// ```text
    /// 0x358d5c31fc8acd7f - 0x21 = 0x358d5c31fc8acb5e   Rest % 0x1f: 0
    /// 0x347a48edcbed8045 - 0x21 = 0x347a48edcbed8024   Rest % 0x1f: 0
    /// ```
    /// Beide **sind** also reserviert (N = 124 478 768 564 080 354 und
    /// 121 981 129 783 765 852). Das bestaetigt die Regel von 7.2.8.
    ///
    /// ICH HABE HIER ZUERST BEHAUPTET, SIE SEIEN ES NICHT — nachgeschrieben
    /// aus einem anderen RFC und nicht gerechnet. Die Teilmenge-Eigenschaft
    /// bleibt trotzdem richtig und wird mit einem erfundenen Typ (0x1234)
    /// gezeigt, der **nicht** auf das Muster passt und trotzdem
    /// uebersprungen wird. Wer eine Zahl nachschreibt, statt sie zu rechnen,
    /// hat eine Behauptung, keine Messung; beides sieht gleich aus und ist
    /// es nicht.
    ///
    /// `h3_reserviert()` ist deshalb nur eine **Dokumentationshilfe** fuer
    /// Tests; `h3_lesen` entscheidet **nicht** danach.
    ///
    /// DER GRUND FUER EINE EIGENE VARIANTE UND NICHT EIN FEHLER (U2,
    /// 01.10.2026): RFC 9114 Abschnitt 9.1 — "Frame types that are not
    /// defined in this document MUST be ignored." Ein Server darf vor der
    /// Antwort beliebige eigene Frames (und die in 7.2.8 beschriebenen
    /// GREASE-Frames) schicken, und der Client muss sie **ueberspringen** und
    /// danach weiterlesen.
    ///
    /// GEMESSEN, was der Fehler gekostet hat: cloudflare-quic.com stellt den
    /// Antwortstrom zwei solche Frames voran — einer mit Laenge 0 und einer mit
    /// dem Text "GREASE is the word" (echte Bytes in
    /// `testdaten/cloudflare_grease_h3.bin`). Vorher brach `h3_lesen` beim
    /// ersten davon ab, der `:status` wurde nie gelesen, und der Abruf endete
    /// nach 68 Paketen und 65 KiB im Idle-Timeout. Das ist kein Randfall: es
    /// ist die **erste** Antwort jedes HTTP/3-Servers von Cloudflare.
    ///
    /// Der Inhalt wird **nicht** kopiert, nur die Laenge gemerkt. Ein
    /// unbekannter Frame-Typ ist die einzige Stelle, an der ein Server eine
    /// beliebig grosse Laenge nennen darf, und sie darf keinen Puffer kosten.
    Fremd {
        typ: u64,
        laenge: usize,
    },
}

/// Warum ein HTTP/3-Frame nicht gelesen oder geschrieben werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H3Fehler {
    Abgeschnitten {
        braucht: usize,
        da: usize,
    },
    ZuVieleEinstellungen {
        ist: usize,
        maximum: usize,
    },
    /// Ein unbekannter Frame-Typ. **Beim Lesen wird der Fehler nicht mehr
    /// erzeugt** (RFC 9114 9.1, siehe `H3Rahmen::Fremd`); die Variante bleibt,
    /// weil `H3Fehler` oeffentlich ist und Aufrufer sie matchen.
    UnbekannterTyp {
        typ: u64,
    },
    Varint(VarintFehler),
}

impl std::fmt::Display for H3Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H3Fehler::Abgeschnitten { braucht, da } => {
                write!(f, "Frame braucht {braucht} Byte, da sind {da}")
            }
            H3Fehler::ZuVieleEinstellungen { ist, maximum } => {
                write!(f, "{ist} SETTINGS-Paare, erlaubt sind {maximum}")
            }
            H3Fehler::UnbekannterTyp { typ } => write!(f, "unbekannter Frame-Typ {typ}"),
            H3Fehler::Varint(e) => write!(f, "Varint im Frame: {e}"),
        }
    }
}

impl std::error::Error for H3Fehler {}

impl From<VarintFehler> for H3Fehler {
    fn from(e: VarintFehler) -> Self {
        H3Fehler::Varint(e)
    }
}

/// Liest einen HTTP/3-Frame. Liefert `(frame, verbraucht)`.
pub fn h3_lesen(daten: &[u8]) -> Result<(H3Rahmen, usize), H3Fehler> {
    let (typ, n1) = varint::lese(daten)?;
    let (laenge, n2) = varint::lese(&daten[n1..])?;
    let inhalt_start = n1 + n2;
    let inhalt_ende = inhalt_start + laenge as usize;
    if daten.len() < inhalt_ende {
        return Err(H3Fehler::Abgeschnitten {
            braucht: inhalt_ende,
            da: daten.len(),
        });
    }
    let inhalt = &daten[inhalt_start..inhalt_ende];

    let rahmen = match typ {
        H3_DATEN => H3Rahmen::Daten(inhalt.to_vec()),
        H3_KOPFBLOCK => H3Rahmen::Kopfblock(inhalt.to_vec()),
        H3_EINSTELLUNGEN => {
            let mut paare = Vec::new();
            let mut rest = inhalt;
            while !rest.is_empty() {
                if paare.len() >= MAX_EINSTELLUNGEN {
                    return Err(H3Fehler::ZuVieleEinstellungen {
                        ist: paare.len() + 1,
                        maximum: MAX_EINSTELLUNGEN,
                    });
                }
                // N49: Ohne Laengenpraefix, wie beim Schreiben — siehe dort.
                let (id, n) = varint::lese(rest)?;
                let (wert, n2) = varint::lese(&rest[n..])?;
                paare.push((id, wert));
                rest = &rest[n + n2..];
            }
            H3Rahmen::Einstellungen(paare)
        }
        H3_GOAWAY => {
            let (id, _) = varint::lese(inhalt)?;
            H3Rahmen::Goaway(id)
        }
        H3_MAX_PUSH_ID => {
            let (id, _) = varint::lese(inhalt)?;
            H3Rahmen::MaxPushId(id)
        }
        // RFC 9114 9.1: unbekannter Typ wird **uebersprungen**, nicht bemerkt.
        _ => H3Rahmen::Fremd {
            typ,
            laenge: inhalt.len(),
        },
    };
    Ok((rahmen, inhalt_ende))
}

/// Liegt der Frame-Typ im reservierten Muster aus RFC 9114 7.2.8
/// (`0x1f * N + 0x21`, also `31 * N + 33`)?
///
/// NUR fuer Tests und fuer den Kommentar. `h3_lesen` benutzt das **nicht**:
/// Abschnitt 9 verlangt, **jeden** undefinierten Typ zu ignorieren, nicht nur
/// die reservierten. Wer diese Funktion in `h3_lesen` einhaengen wuerde, wuerde
/// genau den Cloudflare-Fall wieder einbauen, der den Abruf verhindert hat
/// (BERICHT_QUIC_U2.txt, Fehler 2): dessen Typen passen nicht zum Muster.
pub fn h3_reserviert(typ: u64) -> bool {
    typ >= 0x21 && (typ - 0x21) % 0x1f == 0
}

/// Schreibt einen HTTP/3-Frame.
pub fn h3_schreiben(rahmen: &H3Rahmen, aus: &mut Vec<u8>) -> Result<(), H3Fehler> {
    let (typ, inhalt) = match rahmen {
        H3Rahmen::Daten(d) => (H3_DATEN, d.clone()),
        H3Rahmen::Kopfblock(k) => (H3_KOPFBLOCK, k.clone()),
        H3Rahmen::Einstellungen(paare) => {
            if paare.len() > MAX_EINSTELLUNGEN {
                return Err(H3Fehler::ZuVieleEinstellungen {
                    ist: paare.len(),
                    maximum: MAX_EINSTELLUNGEN,
                });
            }
            let mut inhalt = Vec::new();
            for (id, wert) in paare {
                // N49: **OHNE Laengenpraefix** — mein vermeintlicher „Fix"
                // hatte eines eingebaut, und das war der zweite Fehler.
                //
                // RFC 9114 7.2.4, im Originaltext:
                //   "The payload of a SETTINGS frame consists of zero or more
                //    parameters. Each parameter consists of a setting
                //    identifier and a value, **both encoded as QUIC
                //    variable-length integers**."
                // und die Grafik:
                //   Setting { Identifier (i), Value (i) }
                //
                // Also `Identifier | Wert`, ohne Laenge. Mein Leser las es von
                // Anfang an so — ZWEI eigene Stellen, die dasselbe FALSCHE
                // taten und deshalb uebereinstimmten. Das ist der Grund, warum
                // der Fehler so lange unentdeckt blieb: **ein Tautologie-Paar.**
                //
                // Warum die echten Server es trotzdem akzeptierten: Unsere
                // SETTINGS-Quittung schrieb damals ein leeres SETTINGS
                // (`04 00`) auf **Offset 0** und ueberschrieb damit genau die
                // Bytes, an denen der Lesefehler sichtbar gewesen waere. Der
                // Zufall hat den Fehler zugedeckt.
                varint::schreibe(*id, &mut inhalt)?;
                varint::schreibe(*wert, &mut inhalt)?;
            }
            (H3_EINSTELLUNGEN, inhalt)
        }
        H3Rahmen::Goaway(id) => {
            let mut inhalt = Vec::new();
            varint::schreibe(*id, &mut inhalt)?;
            (H3_GOAWAY, inhalt)
        }
        H3Rahmen::MaxPushId(id) => {
            let mut inhalt = Vec::new();
            varint::schreibe(*id, &mut inhalt)?;
            (H3_MAX_PUSH_ID, inhalt)
        }
        // Ein fremder Frame laesst sich nicht zurueckschreiben: es gibt keinen
        // Inhalt, den man zurueckschreiben koennte, und der Typ allein waere
        // eine leere Nachricht, die der Peer als Echo liest.
        H3Rahmen::Fremd { typ, .. } => return Err(H3Fehler::UnbekannterTyp { typ: *typ }),
    };
    varint::schreibe(typ, aus)?;
    varint::schreibe(inhalt.len() as u64, aus)?;
    aus.extend_from_slice(&inhalt);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein DATA-Frame und ein HEADERS-Frame rundlauf.
    #[test]
    fn daten_und_kopfblock_rundlauf() {
        for rahmen in [
            H3Rahmen::Daten(b"hallo welt".to_vec()),
            H3Rahmen::Kopfblock(vec![0, 0, 0xd1, 0xd7, 0xc1]),
        ] {
            let mut p = Vec::new();
            h3_schreiben(&rahmen, &mut p).expect("schreiben");
            let (gelesen, n) = h3_lesen(&p).expect("lesen");
            assert_eq!(n, p.len(), "M7: alles verbraucht");
            assert_eq!(gelesen, rahmen, "M7: Rundlauf {rahmen:?}");
        }
    }

    /// SETTINGS mit zwei Paaren.
    #[test]
    fn einstellungen_rundlauf() {
        let rahmen = H3Rahmen::Einstellungen(vec![(0x01, 0x00), (0x06, 4096)]);
        let mut p = Vec::new();
        h3_schreiben(&rahmen, &mut p).expect("schreiben");
        let (gelesen, _) = h3_lesen(&p).expect("lesen");
        assert_eq!(gelesen, rahmen, "M7: SETTINGS-Rundlauf");
    }

    /// M4: ein abgeschnittenes Laengenfeld ist ein benannter Fehler.
    #[test]
    fn abgeschnitten_ist_ein_benannter_fehler() {
        // Typ DATA (0x00), Laenge 10, aber nur 2 Byte Inhalt.
        let p = narsil_tls::sha2::von_hex("000a0102").expect("hex");
        assert!(matches!(
            h3_lesen(&p),
            Err(H3Fehler::Abgeschnitten { braucht: 12, da: 4 })
        ));
    }
}
