//! R4z7 — der **HTTP/2-Anfrageweg** im Client.
//!
//! added 01.10.2026 14:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
//! Vorgang: T2 aus `AUTONOM.txt`, Zeile 538: „den h2-ANFRAGEweg in
//! narsil-net fertig (`lies_rahmen` + `baue_headers` + `dekodiere_bloecke`
//! zusammenfuehren), dann `weg_gebaut()` = true".
//!
//! DER ZUSTAND VORHER, in drei Sätzen:
//!   * `h2::baue_rahmen`/`lies_rahmen` gab es (R4w, Rahmenebene),
//!   * `h2strom::baue_headers` und `nimm_auf` gab es (R4y, Streamebene),
//!   * und **nichts** fuehrte die drei zusammen. `h2::weg_gebaut()`
//!     stand deshalb auf `false`, und `narsil-net` bot `h2` nicht an.
//!
//! DER WEG, und er ist die Reihenfolge aus RFC 9113 3.4:
//!   1. **Client-Preface** (`PRIORITAET`, 24 Byte) — der Server erkennt
//!      daran, dass es ueberhaupt HTTP/2 ist.
//!   2. **`SETTINGS`** — unsere Parameter.
//!   3. Das **Angebot** als `HEADERS` auf Strom 1 (ungerade, denn der
//!      Client nimmt die ungeraden IDs, RFC 9113 5.1).
//!   4. Antwort-Rahmen: `SETTINGS` quittieren, `HEADERS` (+ `CONTINUATION`)
//!      dekodieren, `DATA` sammeln, bis `END_STREAM`.

use crate::h2::{Rahmen, RahmenFehler, typen};
use crate::h2strom::{Strom, nimm_auf};
use std::io::{Read, Write};

/// Die Verbindungsschicht, ueber die die Frames laufen.
///
/// added 01.10.2026 14:0x: ein **eigener** Trait statt `Box<dyn Leitung>`.
/// Grund: `Leitung` traegt keine Frist und kennt den Zustand nicht, und
/// beides wird hier gebraucht (Lesen mit Frist, Puffer fuer halbe
/// Rahmen). Eine eigene Grenze ist lieber zu klein als zu gross.
pub trait Frames: Read + Write {
    /// Setzt die Lesefrist. `None` heisst „ohne Frist".
    fn set_frist(&mut self, dauer: Option<std::time::Duration>);
}

/// Was der Client einem Server schickt und was zurueckkommt.
pub struct Anfrage {
    pub methode: String,
    pub schemata: String,
    pub host: String,
    pub pfad: String,
    pub felder: Vec<(String, String)>,
    pub koerper: Vec<u8>,
}

/// Was der Server zurueckschickt — schon in die Form von
/// [`crate::Response`] uebersetzt, damit der Aufrufer nichts doppelt
/// macht.
pub struct Antwort {
    pub status: u16,
    pub felder: Vec<(String, String)>,
    pub koerper: Vec<u8>,
}

/// M4: So viele Bytes koennen **unser** Gummiband zwischenspeichern.
///
/// added 01.10.2026 14:0x. Begruendung (M1..M9): ein Rahmen ist hoechstens
/// `h2::MAX_RAHMEN_BYTES` (16 KiB), aber ein `HEADERS`-Block darf ueber
/// `CONTINUATION`-Rahmen beliebig lang werden — das ist der Grund fuer
/// `h2strom::MAX_HEADERS_BLOECKE`. Ohne eine Grenze hier wuerde ein
/// Server 16 KiB pro Rahmen schicken und damit den Speicher fuellen,
/// ohne dass irgendein Zaehler reagiert. Der Wert ist das Produkt aus
/// Rahmengrenze und Blockgrenze, nicht geraten.
pub const MAX_PUFFER_BYTES: usize = 64 * 1024;

/// M3: So viele Rahmen liest der Client fuer **eine** Anfrage.
///
/// added 01.10.2026 14:0x. Begruendung: eine Antwort besteht aus
/// `SETTINGS`, `HEADERS` (+CONTINUATION) und `DATA`-R Rahmen. Bei 16 KiB
/// pro `DATA`-Ramen sind 200 Rahmen **2,5 MiB** — gross genug fuer jede
/// echte Seite und klein genug, dass ein Server, der nur `DATA`
/// schickt, nicht ewig laeuft. `h2::MAX_RAHMEN` (10 000) gilt fuer die
/// **Gesamtverbindung**, nicht fuer eine Anfrage.
//
// added 04.10.2026, Rolle NETZTEMPO. **AUS DER KOERPERGRENZE BERECHNET, und
// nicht 200.** GEMESSEN (`tests/grosse_ladung.rs`, 20 MiB):
//     GROSS-LADUNG (20971520 Byte) FEHLGESCHLAGEN nach 40,58 s:
//     der Rahmen ist 200 Byte lang, erlaubt sind 200
// **198 `DATA`-Rahmen** waren gelesen, dann diese Meldung: `laenge = 200 =
// erlaubt = 200 = MAX_RAHMEN_PRO_ANFRAGE`. Die Kappung ist **200 Rahmen**, und
// 200 * 16 KiB sind **3,2 MiB** — **jede** Antwort ueber 3,2 MiB bricht ab,
// unabhaengig vom Fenster.
//
// **WARUM DIE ZAHL FALSCH WAR:** sie war als **Schutz gegen einen Server,
// der endlos Rahmen schickt**, gedacht — aber sie **beschneidet auch
// koerperlich grosse, ansonsten voellig normale** Antworten. Ein Browser, der
// ein 8-MiB-Bundle laedt, sieht einen „Protokollfehler", und die Meldung
// nennt keine Groesse, die man reparieren koennte.
//
// **DIE REGEL, und sie folgt der Zahl, die schon existiert:** die Zahl der
// Rahmen folgt der **Groesse**, die wir ueberhaupt annehmen
// (`h2strom::MAX_STROM_BYTES` = 16 MiB), geteilt durch den **kleinsten**
// sinnvollen Fortschritt je Rahmen. Das ist `MAX_RAHMEN_BYTES / 16` = 1 KiB
// (darunter waere der Rahmenkopf groesser als die Nutzlast). Damit sind es
// 16 MiB / 1 KiB = **16 384** Rahmen. **DER ECHTE SCHUTZ** bleibt
// `MAX_STROM_BYTES` — das ist die Grenze, die `nimm_auf` durchsetzt.
pub const MAX_RAHMEN_PRO_ANFRAGE: usize =
    crate::h2strom::MAX_STROM_BYTES / (crate::h2::MAX_RAHMEN_BYTES / 16) + 64;

/// M6: Zeitgrenze fuer das Warten auf den ersten Rahmen und fuer jeden
/// einzelnen danach.
///
/// added 01.10.2026 14:0x. Zwei Werte, weil sie zwei Fragen beantworten:
/// `erster` ist „kommt ueberhaupt eine Antwort" (der Server darf
/// nachdenken), `weiter` ist „kommt der naechste Rahmen" (der Server
/// hat das Senden angefangen). Ohne den Unterschied wuerde man beim
/// zweiten Wert auch die erste Wartezeit deckeln — und jede Seite mit
/// 500 ms Serverdenkzeit waere kaputt.
pub const FRIST_ERSTER: std::time::Duration = std::time::Duration::from_secs(20);
pub const FRIST_WEITER: std::time::Duration = std::time::Duration::from_secs(10);

/// M1: Der erste Strom eines Clients hat die ID 1.
///
/// added 01.10.2026 14:0x. RFC 9113 5.1: Client-Stroeme haben **ungerade**
/// IDs, Server-Stroeme gerade, und 0 ist fuer beide verboten. Das ist
/// keine Konstante „aus Bequemlichkeit", sondern die Norm, und sie wird
/// hier **einmal** genannt: an drei Stellen eine `1` zu schreiben waere
/// drei Moeglichkeiten fuer dieselbe Normzeile.
pub const ERSTER_STROM: u32 = 1;

/// Der Fehler des Clientwegs — **ohne** eigene Variante, solange es
/// keinen gibt.
///
/// added 01.10.2026 14:0x: `RahmenFehler` deckt alles ab, was hier
/// schiefgehen kann (unbekannter Typ, falscher Strom, Block zu lang).
/// Eine eigene Fehlerart waere eine **zweite** Fehlersprache fuer
/// denselben Vorgang, und der Aufrufer muesste zwei behandeln. Wer hier
/// spaeter einen Fehler braucht, der sich in `RahmenFehler` nicht
/// ausdruecken laesst (etwa „Timeout"), der ergaenzt ihn dort — dort
/// steht die Namenskonvention schon.
pub type Fehler = RahmenFehler;

/// Der **Grund** eines Schreibfehlers, ohne Allokation.
///
/// added 01.10.2026 14:1x, R4z7. WARUEBER EINE FUNKTION: `io::Error`
/// hat keinen `&'static str`, und `e.to_string()` wuerde im Fehlerpfad
/// allozieren — das ist im Rest des Projekts ausdruecklich verboten
/// (R4v: `fehler_name` statt `to_string`). Dieengstenfaelle werden
/// benannt, der Rest bekommt einen festen Text.
///
/// `BrokenPipe` und `ConnectionReset` stehen **nicht** vorne, obwohl sie
/// die haeufigsten sind: ein geschlossener Sockel ist im h2-Fall normal
/// (der Server schliesst nach der letzten Antwort), und ein eigener Name
/// wuerde sagen „hier ist etwas kaputt", wo nichts ist.
/// added 02.10.2026 16:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **ORT UND URSACHE IN EINEM TEXT**, und die Zusammensetzung ist absichtlich:
/// der Ort steht **vor** der Ursache, weil die Frage „wo“ die erste ist und
/// „warum“ die zweite. Als `String`, weil `grund` ein `&'static str` ist und
/// seine Aenderung einen neuen Typ an zehn Stellen bedeutet haette.
fn or_und_grund(ort: &str, e: &std::io::Error) -> String {
    format!("{ort}: {}", schreibfehler_grund(e))
}

/// added 02.10.2026 16:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DIE SETTINGS DES GEGNERS**, und sie sind bis heute **verworfen** worden:
/// der SETTINGS-Zweig hat die Nutzlast gelesen, quittiert und dann **nicht
/// ausgewertet**. GEMESSEN: `typen::SETTINGS =>` in `lies_antwort` enthielt nur
/// den ACK-Versand.
///
/// added 02.10.2026 16:5x: DIE NORM, RFC 9113 3.4, woertlich: „Upon receiving
/// the SETTINGS frame, the client **is expected to honor any settings
/// established**.“ Und 6.5.2 nennt zwei Werte, deren Missachtung ein
/// **Verbindungsfehler** ist und nicht nur eine schlechte Sitte:
/// * `SETTINGS_ENABLE_PUSH (0x02)`: „A client **MUST** treat receipt of a
///   SETTINGS frame with SETTINGS_ENABLE_PUSH set to 1 as a connection error
///   (Section 5.4.1) of type PROTOCOL_ERROR.“
/// * `SETTINGS_MAX_FRAME_SIZE (0x05)`: „Values outside this range **MUST** be
///   treated as a connection error“, und der Bereich ist 2^14 … 2^24-1.
///
/// added 02.10.2026 16:5x: **UND WAS AUSDRUECKLICH NICHT ERZWINGBAR IST**, weil
/// es die Reihenfolge verbietet: unsere HEADERS gehen **raus**, bevor wir die
/// Server-SETTINGS gelesen haben. Das erlaubt die Norm selbst — 3.4: „To avoid
/// unnecessary latency, clients **are permitted** to send additional frames to
/// the server immediately after sending the client connection preface,
/// **without waiting** to receive the server connection preface.“ Eine
/// `MAX_FRAME_SIZE`-Grenze beim Senden von HEADERS koennte man also gar nicht
/// einhalten; sie zu behaupten waere eine Regel, die man nicht pruefen kann.
///
/// added 02.10.2026 16:5x: **UND DIE ANFANGSWERTE** stammen aus 6.5.2 und nicht
/// aus dem Gedaechtnis: `HEADER_TABLE_SIZE` 4 096, `MAX_FRAME_SIZE` 2^14 =
/// 16 384, `MAX_CONCURRENT_STREAMS` unbegrenzt, `INITIAL_WINDOW_SIZE` 2^16-1 =
/// 65 535 (6.9.2), `ENABLE_PUSH` 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GegnerSettings {
    /// `SETTINGS_MAX_FRAME_SIZE` (0x05): groesste Nutzlast, die der Gegner
    /// empfangen will. Anfang 2^14 (6.5.2).
    pub max_frame_size: u32,
    /// `SETTINGS_MAX_CONCURRENT_STREAMS` (0x03): `None` heisst „unbegrenzt“
    /// (6.5.2: „Initially, there is no limit to this value“).
    pub max_stroeme: Option<u32>,
    /// `SETTINGS_HEADER_TABLE_SIZE` (0x01), Anfang 4 096 (6.5.2).
    pub header_table_size: u32,
    /// `SETTINGS_INITIAL_WINDOW_SIZE` (0x04), Anfang 65 535 (6.9.2).
    pub initial_window: u32,
}

impl Default for GegnerSettings {
    /// added 02.10.2026 16:6x: die Anfangswerte aus 6.5.2 — und sie sind hier
    /// **nur fuer den Fall**, dass der Gegner gar keine SETTINGS schickt, was nach
    /// 6.5.2 verboten ist (die muessen die erste Nachricht des Servers sein).
    fn default() -> Self {
        GegnerSettings {
            max_frame_size: 16_384,
            max_stroeme: None,
            header_table_size: 4_096,
            initial_window: 65_535,
        }
    }
}

impl GegnerSettings {
    /// added 02.10.2026 16:6x: **AUS DEN PAAREN**, und die kommen aus
    /// `h2::settings_paare` — es gibt also **keine** zweite Parser-Strecke.
    ///
    /// added 02.10.2026 16:6x: **UNBEKANNTE KENNUNGEN WERDEN IGNORIERT**,
    /// woertlich 6.5.2: „An endpoint that receives a SETTINGS frame with any
    /// unknown or unsupported identifier **MUST ignore** that setting.“ Das ist
    /// der Grund, warum hier kein Fehler faellt: eine spaeter definierte
    /// Kennung darf einen alten Client nicht ausschalten.
    ///
    /// added 02.10.2026 16:6x: **UND DIE ZWEI MUSTS**, und beide sind hier
    /// *Verbindungsfehler* und keine Warnungen. Fuer `ENABLE_PUSH` habe ich
    /// `RahmenFehler::ProtokollVerletzung` genommen, weil die Norm
    /// „connection error … of type PROTOCOL_ERROR“ sagt.
    pub fn aus_paare(paare: &[(u16, u32)]) -> Result<Self, RahmenFehler> {
        let mut s = GegnerSettings::default();
        for (id, wert) in paare {
            match *id {
                0x01 => s.header_table_size = *wert,
                0x02 => {
                    // added 02.10.2026 16:6x: **NUR 0 IST ERLAUBT** (6.5.2: „A
                    // server MUST NOT explicitly set this value to 1“), und fuer
                    // jeden anderen Wert als 0 oder 1 ist es ohnehin ein
                    // Verbindungsfehler — beides faellt in dieselbe Regel.
                    if *wert != 0 {
                        return Err(RahmenFehler::ProtokollVerletzung {
                            grund: "SETTINGS_ENABLE_PUSH ist 1 oder unzulaessig (RFC 9113 6.5.2)"
                                .to_string(),
                        });
                    }
                }
                0x03 => s.max_stroeme = Some(*wert),
                0x04 => s.initial_window = *wert,
                0x05 => {
                    // added 02.10.2026 16:7x: **DER BEREICH**, woertlich 6.5.2:
                    // „MUST be between this initial value and the maximum allowed
                    // frame size (2^24-1 …), inclusive.“ Beide Grenzen sind
                    // gerundet geschrieben, weil der Wert sie **nicht**
                    // erreichen darf.
                    if *wert < 16_384 || *wert > 16_777_215 {
                        return Err(RahmenFehler::ProtokollVerletzung {
                            grund:
                                "SETTINGS_MAX_FRAME_SIZE ausserhalb 2^14..2^24-1 (RFC 9113 6.5.2)"
                                    .to_string(),
                        });
                    }
                    s.max_frame_size = *wert;
                }
                _ => {}
            }
        }
        Ok(s)
    }
}

/// added 02.10.2026 16:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DER FEHLER NENNT DIE STELLE**, und das ist keine Feinheit, sondern der Grund,
/// warum `PROBLEME.txt` Nr. 30 bis heute keine Antwort hat.
///
/// added 02.10.2026 16:2x: GEMESSEN AN DER EIGENTLICHEN MELDUNG: in
/// `tests/alpn_client.rs` steht `Err` mit dem Text „HTTP/2: Schreiben auf der
/// Verbindung fehlgeschlagen: die Verbindung nimmt keine weiteren Daten an“ — und
/// **zehn** Schreibstellen in dieser Datei erzeugen genau diesen Text: Preface,
/// SETTINGS, HEADERS, DATA, Quittung, zwei WINDOW-UPDATES und zwei
/// `flush`. Ohne Ortsangabe ist aus der Meldung nicht zu ersehen, **welche**
/// Stelle gerissen ist, und die Suche nach der Ursache läuft im Kreis.
///
/// added 02.10.2026 16:2x: **UND DESHALB DER ORT IM TEXT**, nicht in einem
/// neuen Fehlerfeld: `RahmenFehler::SchreibenFehlgeschlagen { grund }` ist eine
/// oeffentliche Form, und eine zweite Variante fuer „derselbe Fehler, andere
/// Stelle“ waere eine neue Form ohne neuen Inhalt. Der Ort gehoert in die
/// **lesbare** Meldung, die dem Benutzer angezeigt wird — dort hilft er, und nur
/// dort.
fn schreibfehler_grund(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "Zugriff verweigert",
        std::io::ErrorKind::InvalidInput => "ungueltige Eingabe fuer die Leitung",
        std::io::ErrorKind::OutOfMemory => "kein Speicher mehr",
        _ => "die Verbindung nimmt keine weiteren Daten an",
    }
}

/// Der Puffer eines rahmenweisen Lesens.
///
/// added 01.10.2026 14:0x: `lies_rahmen` braucht den **ganzen** Rahmen
/// auf einmal, TCP zerlegt aber Nachrichten. Also wird gelesen, bis der
/// Kopf die Laenge verraten hat, und dann der Rest.
///
/// WARUM KEIN `Read`-TRAIT MIT RÜCKGABE: `lies_rahmen` gibt
/// `(Rahmen, verbrauchte_Bytes)` zurueck, und der Aufrufer muss den Rest
/// behalten koennen. Eine eigene Struktur ist hier ehrlicher als ein
/// `Vec`, aus dem man nachrechnet.
struct Puffer {
    bytes: Vec<u8>,
}

/// added 01.10.2026 14:0x. DIE GRENZE WIRD **VOR** DER ALLOKATION
/// geprueft, nicht danach: `lies_rahmen` lehnt `laenge > MAX_RAHMEN_BYTES`
/// ab, aber ein Angreifer koennte 4 GiB **ankündigen**, ohne ein Byte zu
/// schicken. `MAX_PUFFER_BYTES` ist deshalb kleiner als
/// `h2::MAX_RAHMEN_BYTES` + Kopf, und `Puffer::nimm` weist einen
/// Ueberlauf **vor** dem Lesen ab.
impl Puffer {
    fn neu() -> Self {
        Puffer {
            bytes: Vec::with_capacity(9),
        }
    }

    /// Fuellt bis mindestens `ziel` Byte, oder bis `frist` ablaeuft.
    ///
    /// added 01.10.2026 14:0x. DER RUECKGABEWERT: `true`, wenn `ziel`
    /// erreicht ist. `false` heisst **Zeitlimit** — und das ist ein
    /// Fehler, kein „nichts zu tun": der Aufrufer muss wissen, ob er
    /// weiterwarten soll oder aufgeben.
    fn nimm(&mut self, leitung: &mut dyn Frames, ziel: usize) -> Result<bool, Fehler> {
        if self.bytes.len() >= ziel {
            return Ok(true);
        }
        let mut stueck = [0u8; 8192];
        loop {
            match leitung.read(&mut stueck) {
                Ok(0) => {
                    // added 04.10.2026, Rolle NETZTEMPO: **SERVER SCHLIESST** —
                    // `Ok(0)` heisst „Draht zu Ende" (RFC 9113 8.1: END_STREAM
                    // ist ein **Flag**, kein Ende des Drahts). Und es ist **nicht**
                    // `TimedOut`: dort steht in diesem Crate ausdruecklich, dass
                    // ein Timeout **kein** Ende ist. Beides zu verwechseln ist
                    // die Art von Fehler, die eine Zahl erzeugt, die nichts
                    // bedeutet — die vierte Form von „am falschen Ort gemessen".
                    crate::h2geteilt::server_schliesst();
                    return Ok(false);
                }
                Ok(n) => {
                    self.bytes.extend_from_slice(&stueck[..n]);
                    if self.bytes.len() > MAX_PUFFER_BYTES {
                        return Err(RahmenFehler::ZuGross {
                            laenge: self.bytes.len(),
                            erlaubt: MAX_PUFFER_BYTES,
                        });
                    }
                    if self.bytes.len() >= ziel {
                        return Ok(true);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Ok(false),
                Err(_) => return Ok(false),
            }
        }
    }

    /// Liest den naechsten vollstaendigen Rahmen.
    fn rahmen(&mut self, leitung: &mut dyn Frames, erste: bool) -> Result<Option<Rahmen>, Fehler> {
        // DER KOPF ZUERST, und deshalb der eigene Aufruf: `lies_kopf`
        // sagt, wie lang der Rahmen wird, und erst danach weiss man, wie
        // viel noch fehlt. Ohne diese Reihenfolge muesste man raten.
        if !self.nimm(leitung, 9)? {
            return Ok(None);
        }
        let (_, _, _, laenge) = crate::h2::lies_kopf(&self.bytes)?;
        let gesamt = 9 + laenge;
        if gesamt > MAX_PUFFER_BYTES {
            return Err(RahmenFehler::ZuGross {
                laenge: gesamt,
                erlaubt: MAX_PUFFER_BYTES,
            });
        }
        if gesamt > 9 && !self.nimm(leitung, gesamt)? {
            return Ok(None);
        }
        let (rahmen, verbraucht) = crate::h2::lies_rahmen(&self.bytes)?;
        self.bytes.drain(..verbraucht);
        let _ = erste;
        Ok(Some(rahmen))
    }
}

/// M4: so viele Byte hat die Nutzlast eines `GOAWAY` mindestens.
///
/// added 01.10.2026 23:6x, R5b. RFC 9113 6.8 nennt zwei Felder: die
/// `Last-Stream-ID` (4 Byte) und den Fehlercode (4 Byte). Der Wert 8 ist
/// also **gemessen an der Norm**, nicht geraten — und er ist eine Grenze,
/// weil `goaway_aus` ohne sie vier Bytes eines Frames lesen wuerde, die
/// gar nicht da sind.
const GOAWAY_NUTZLAST_BYTES: usize = 8;

/// Liest `Last-Stream-ID` und Fehlercode aus einem `GOAWAY`.
///
/// added 01.10.2026 23:6x, R5b. DER WEG: `u32::MAX` heisst „unbekannt".
/// GEMESSEN warum: RFC 9113 6.8 erlaubt dem Server, die Felder
/// wegzulassen — dann waere ein Raten stiller als eine Null. Und die
/// `Last-Stream-ID` ist nach 6.8 ein **31-Bit**-Wert; das oberste Bit ist
/// reserviert, deshalb wird es abgeschnitten statt als Stromnummer
/// ausgegeben.
fn goaway_aus(nutzlast: &[u8]) -> (u32, u32) {
    if nutzlast.len() < GOAWAY_NUTZLAST_BYTES {
        return (u32::MAX, u32::MAX);
    }
    let mut l = [0u8; 4];
    l.copy_from_slice(&nutzlast[0..4]);
    let mut g = [0u8; 4];
    g.copy_from_slice(&nutzlast[4..8]);
    (u32::from_be_bytes(l) & 0x7fff_ffff, u32::from_be_bytes(g))
}

/// Schreibt Preface und `SETTINGS`.
///
/// added 01.10.2026 14:0x. DIE REIHENFOLGE ist die Norm (RFC 9113 3.4):
/// der Preface steht **vor** dem ersten Rahmen und ist selbst kein
/// Rahmen. Wer sie umdreht, sendet 24 Byte, die der Server nicht
/// erwartet — und der Server antwortet mit `GOAWAY` oder gar nichts.
/// Das Fenster, das wir fuer die **Verbindung** oeffnen.
///
/// added 01.10.2026 15:8x, R4z8. M4 mit Begruendung: eine Verbindung, an
/// der ein Browser eine Seite mit 600 000 Byte laedt, muss ein Fenster
/// haben, das diese 600 000 Byte **ohne** Rueckfrage trägt — sonst wartet
/// der Server nach 65 535 Byte auf ein `WINDOW_UPDATE`, und wer keins
/// schickt, haengt fuer immer.
///
/// 8 MiB ist keine grosse Zahl, sondern die kleinste, die den groessten
/// gemessenen Body traegt: GEMESSEN am 01.10.2026 (`github.com` ueber
/// h2) sind es 579 865 Byte. Das Doppelte davon, weil ein Browser nicht
/// genau eine Seite laedt und weil ein `WINDOW_UPDATE` nicht bei 0 stehen
/// darf.
///
/// DIE NORM: `SETTINGS_INITIAL_WINDOW_SIZE` (RFC 9113 6.9.2) darf bis
/// 2^31 - 1 gesetzt werden, und das **Verbindungs**fenster wird nur mit
/// einem `WINDOW_UPDATE` auf Strom 0 erhoeht (6.9).
/// added 04.10.2026, Rolle NETZTEMPO. **DIESER NAME IST PRESET A.** Er bleibt,
/// weil er oeffentlich ist und Fremde ihn lesen — aber die **Leitung** liest
/// [`crate::h2::verbindungsfenster`], und das ist der Preset. Wer hier eine Zahl
/// aendert, aendert **eine Variante** und nicht den Browser.
pub const MAX_FENSTER: u32 = crate::h2::STANDARD_VERBINDUNGSFENSTER;

/// Wie viel `DATA` wir einsammeln, bevor wir das Fenster auffuellen.
///
/// added 01.10.2026 15:8x, R4z8. M3 mit Begruendung: `WINDOW_UPDATE` auf
/// **jedem** `DATA`-Rahmen waere ein Rahmen (9 Byte plus Nutzlast) fuer
/// bis zu 16 KiB Nutzlast — also bis zu 0,05 % Zuschlag, aber bei einer
/// 16-KiB-Nutzlast ein Zehntel Overhead. Ein Schritt von 1 MiB heisst:
/// hoechstens ein `WINDOW_UPDATE` je 64 `DATA`-Raehmen.
pub const FLUSSSCHRITT: u32 = 1024 * 1024;

pub fn schreibe_vorlauf(leitung: &mut dyn Frames) -> Result<(), Fehler> {
    // added 04.10.2026, Rolle NETZTEMPO. **DIE SPUR, und diesmal an der
    // Quelle.** GEMESSEN, warum genau hier: die Spur in `h2geteilt` hat die
    // Fragen (1) `SETTINGS-ACK`, (2) Preface und (3) `WINDOW_UPDATE` auf
    // **Strom 0** nicht beantworten koennen, weil diese drei Rahmen im
    // **alten** Weg gesendet werden — in dieser Funktion. Ein Haken nur im
    // geteilten Weg ist ein Haken, der die halbe Leitung nicht sieht.
    crate::h2geteilt::rahmen_spur("->", 0xff, 0, 0, crate::h2::PRIORITAET.len());
    leitung.write_all(crate::h2::PRIORITAET).map_err(|e| {
        RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("VORFAHRT", &e),
        }
    })?;
    // added 01.10.2026 15:8x, R4z8. DAS FENSTER, und es sind **zwei**
    // Rahmen, weil es zwei Fenster gibt (RFC 9113 6.9):
    //  * `SETTINGS_INITIAL_WINDOW_SIZE` gilt fuer den **Strom** 1, den wir
    //    gleich eroeffnen, und darf bis 2^31 - 1 sein.
    //  * das **Verbindungs**fenster kann nur per `WINDOW_UPDATE` auf Strom 0
    //    wachsen — die Norm sagt da ausdruecklich: „the value 0 indicates
    //    that the entire connection is the subject of the frame".
    //  GEMESSEN, was ohne beides passiert: `github.com` lieferte 70 697
    //  Byte und dann nichts mehr, `www.google.com` 66 458 — beides knapp
    //  ueber dem 65 535er Standardfenster. Die Meldung war
    //  `die Verbindung endete, bevor Strom 1 zu war`.
    let settings = crate::h2::baue_settings()?;
    // added 04.10.2026: **DIE WERTE, die wir wirklich ankündigen** — und sie
    // sind **nicht** `EIGENE_SETTINGS` (das sind die, die wir *wollen*). Die
    // Zahl hier ist die, die auf der Leitung steht, und die Frage der Leitung
    // („Firefox: HEADER_TABLE_SIZE 65536, INITIAL_WINDOW_SIZE 131072") ist
    // genau diese Zeile. Deshalb wird sie **gemessen und nicht behauptet**.
    // added 04.10.2026: **AB `[KOPF_BYTES..]`** — und das ist ein **eigener
    // Messfehler**, der in der ersten Fassung stand und **leer** ausgab:
    // `settings_paare` erwartet die **Nutzlast**, bekam aber den ganzen
    // Rahmen **mit** 9-Byte-Kopf. GEMESSEN: die Spur zeigte
    //     H2 SETTINGS []
    //     H2 WINDOW_UPDATE(0) []
    // Eine leere Liste sieht aus wie „der Server sagt uns nichts" — und ist in
    // Wirklichkeit „ich habe an der falschen Stelle gelesen". Das ist die
    // dritte Form desselben Fehlers in diesem Bericht: **am falschen Ort
    // gemessen** (Phase statt Strom, Summen kumulativ, jetzt Rahmenkopf).
    crate::h2geteilt::spur_werte(
        "SETTINGS",
        &crate::h2::settings_paare(&settings[crate::h2::KOPF_BYTES..]).unwrap_or_default(),
    );
    leitung
        .write_all(&settings)
        .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("SETTINGS", &e),
        })?;
    // added 04.10.2026, Rolle NETZTEMPO: **DER PRESET** — Firefox schickt
    // +12 517 377 (12 MiB), wir schicken +8 323 073 (8 MiB), und **das** ist
    // eine der beiden Zahlen des A/B-Vergleichs.
    let aufdrehen = crate::h2::baue_window_update(0, crate::h2::verbindungsfenster() - 65_535)?;
    // added 04.10.2026: **DAS VERBINDUNGSFENSTER**, mit Zahl — Firefox schickt
    // +12 517 377, wir 8 323 073. GEMESSEN, ob das der Unterschied ist: das
    // ist Frage (3), und sie ist mit dieser Zeile **beantwortbar** statt
    // vermutet.
    //
    // added 04.10.2026: **`settings_paare` IST HIER FALSCH, und das war der
    // zweite eigene Messfehler.** Ein `WINDOW_UPDATE` traegt **vier** Byte
    // Inkrement (6.9), keine 6-Byte-Paare — `settings_paare` gibt darum zu
    // Recht `[]` zurueck, und die Spur zeigte `H2 WINDOW_UPDATE(0) []`, was
    // aussieht wie „wir schicken kein Fenster". Wir schicken **8 323 073**
    // (`MAX_FENSTER - 65_535`), und das gehoert **als Zahl** in die Spur.
    //
    // DIE REGEL, und sie ist die aus NETZTEMPO_8/9/10: **eine leere Liste in
    // einer Messung ist fast immer ein Messfehler und selten ein Befund.**
    // Sie sieht aus wie „nichts vorhanden" und ist meistens „an der falschen
    // Stelle gelesen" — Rahmenkopf, falsche Phase, falsche Richtung.
    let inkrement = if aufdrehen.len() >= crate::h2::KOPF_BYTES + 4 {
        u32::from_be_bytes([
            aufdrehen[crate::h2::KOPF_BYTES],
            aufdrehen[crate::h2::KOPF_BYTES + 1],
            aufdrehen[crate::h2::KOPF_BYTES + 2],
            aufdrehen[crate::h2::KOPF_BYTES + 3],
        ]) & 0x7fff_ffff
    } else {
        0
    };
    crate::h2geteilt::spur_zahl("WINDOW_UPDATE(0)", inkrement);
    leitung
        .write_all(&aufdrehen)
        .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("SETTINGS-NACHSCHLAG", &e),
        })?;
    leitung
        .flush()
        .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("FLUSH-VORLAUF", &e),
        })
}

/// Baut den `HEADERS`-Ramen fuer die Anfrage.
///
/// added 01.10.2026 14:0x. DIE FLAGS, und sie sind nicht verhandelbar:
/// * `END_HEADERS` **muss** gesetzt sein — sonst wartet der Server auf
///   `CONTINUATION`, die nicht kommen.
/// * `END_STREAM` nur, wenn es keinen Koerper gibt. Fuer `GET` gibt es
///   keinen, also ist es gesetzt: ein `GET` mit Koerper ist nach
///   RFC 9113 8 „ein Fehler des Clients", und der Server wuerde ihn
///   ignorieren statt ablehnen.
///
/// DER PUFFER VOR `END_STREAM`: bei einem Koerper muss der Server ueber
/// die Laenge Bescheid wissen. `CONTENT-LENGTH` ist dafuer nicht
/// noetig (der Client schliesst mit `END_STREAM`), aber `h2strom::
/// baue_headers` baut genau die Felder, die RFC 9113 8.1.2 fuer einen
/// Client verlangt.
/// added 01.10.2026 00:1x, R5b. `strom` ist der Parameter, der vorher
/// fehlte; die Ursache steht am Kopf von `h2strom::baue_headers`
/// (`invalid_new_stream_id`, gemessen auf dem Draht).
pub fn baue_anfrage_rahmen(anfrage: &Anfrage, strom: u32) -> Result<Vec<u8>, Fehler> {
    crate::h2strom::baue_headers(
        &anfrage.methode,
        &anfrage.schemata,
        &anfrage.host,
        &anfrage.pfad,
        &anfrage.felder,
        !anfrage.koerper.is_empty(),
        strom,
    )
}

/// Schreibt die Anfrage: `HEADERS` und, falls vorhanden, `DATA`.
///
/// added 01.10.2026 14:0x. DER KOERPER ALS EIGENER RAHMEN, und nicht als
/// Teil von `HEADERS`: RFC 9113 4.3 kennt kein „HEADERS mit angehaengtem
/// Koerper". Wer beides in einen Rahmen legt, schickt dem Server
/// Kopfzeilen, die er als Koerper liest.
pub fn schreibe_anfrage(
    leitung: &mut dyn Frames,
    anfrage: &Anfrage,
    strom: u32,
) -> Result<(), Fehler> {
    //
    // added 01.10.2026 14:0x, R4z7. DIE PRUEFUNG VOR DEM SCHREIBEN — ein
    // **echter Fehler**, gefunden vom Test
    // `ein_zu_grosser_koerper_wird_abgewiesen_und_nennt_die_zahlen`: die
    // Groessenpruefung stand **nach** dem ersten `write_all`. Bei einem
    // 17-KiB-Koerper ging also der `HEADERS`-Ramen raus, und **dann**
    // kam der Fehler. GEMESSEN: der Draht war nicht leer.
    //
    // WARUM DAS SCHLIMM IST und nicht nur unschoen: der Server bekommt
    // einen `HEADERS`-Ramen ohne `END_STREAM` und wartet auf einen
    // Koerper, der nie kommt. Die Verbindung ist tot, und der Client
    // meldet nur „zu gross". Ein halb gesendeter Antrag ist der
    // schlechteste von beiden Zustaenden.
    //
    // M4: `DATA` traegt hoechstens `MAX_RAHMEN_BYTES` (16 KiB). Ein
    // groesserer Koerper muesste in mehrere `DATA`-Ramen, und der letzte
    // bekommt `END_STREAM`. Das ist der naechste Punkt, nicht dieser:
    // `fetch` schickt heute `GET` ohne Koerper.
    if anfrage.koerper.len() > crate::h2::MAX_RAHMEN_BYTES {
        return Err(RahmenFehler::ZuGross {
            laenge: anfrage.koerper.len(),
            erlaubt: crate::h2::MAX_RAHMEN_BYTES,
        });
    }
    // added 01.10.2026 00:1x, R5b: **hier** ist der Strom durchgereicht.
    // Vorher stand hier `baue_anfrage_rahmen(anfrage)`, und die Funktion
    // setzte selbst die 1 — der `DATA`-Ramen zwei Zeilen darunter
    // benutzte dagegen `strom`. Genau diese Mischung erzeugte die
    // `invalid_new_stream_id`.
    let kopf = baue_anfrage_rahmen(anfrage, strom)?;
    leitung
        .write_all(&kopf)
        .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("HEADERS", &e),
        })?;
    if !anfrage.koerper.is_empty() {
        let daten = crate::h2::baue_rahmen(
            typen::DATA,
            crate::h2::flags::DATA_END_STREAM,
            strom,
            &anfrage.koerper,
        )?;
        leitung
            .write_all(&daten)
            .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
                grund: or_und_grund("DATEN", &e),
            })?;
    }
    leitung
        .flush()
        .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
            grund: or_und_grund("FLUSH-ANFRAGE", &e),
        })
}

/// Liest die Antwort, bis der Strom zu ist.
///
/// added 01.10.2026 14:0x. **DIE SCHLEIFE** und ihre vier Faelle:
/// * `SETTINGS` ohne `ACK` → quittieren (der Server wartet sonst nicht
///   weiter, RFC 9113 6.5.3),
/// * `PING` → mit gleicher Nutzlast beantworten (sonst wartet der
///   Server 30 s, RFC 9113 6.7),
/// * alles andere → an `nimm_auf`, das die Kopfzeilen dekodiert und den
///   Koerper sammelt,
/// * nichts mehr und der Strom **nicht** zu → **Timeout**, benannt.
///
/// DER LETZTE PUNKT IST DER WICHTIGE: ohne ihn liefe diese Schleife bei
/// einem stillen Server bis zum Zeitlimit und dann in einen Fehler, der
/// aussieht wie ein Protokollfehler. GEMESSEN war genau das in R4z: ein
/// Test, der „wartet und dann aufgibt", und die Meldung sprach von TLS.
///
/// added 01.10.2026 00:5x, R5b. `hpack` kommt von aussen. GEMESSEN, warum:
/// `www.google.com`, zweiter Abruf auf der gepoolten Leitung (00:3x):
///     Abruf: H2Protokoll { grund: "HPACK: IndexUngueltig (73)" }
/// RFC 7541 4.1 — die dynamische Tabelle wird zwischen Nachrichten nicht
/// geleert, sondern **fortgeschrieben**. Eine Tabelle je Aufruf waere eine
/// Tabelle je Verbindung falsch.
pub fn lies_antwort(
    leitung: &mut dyn Frames,
    strom: u32,
    hpack: &mut crate::hpack::DynamischeTabelle,
) -> Result<Antwort, Fehler> {
    lies_antwort_mit_phasen(leitung, strom, hpack).map(|(a, _)| a)
}

/// Wie [`lies_antwort`], aber mit der Zeit des **ersten** Rahmens.
///
/// added 02.10.2026 04:2x, R5g. DER GRUND: ueber HTTP/2 wird die Zeit bis
/// zum ersten Byte nirgends gemessen — `erstes_byte` stand nur im `Leser`,
/// und der `Leser` gehoert zum HTTP/1.1-Weg. GEMESSEN (02.10.2026 04:1x,
/// `tests/t4_verbindung.rs`, drei Abrufe `www.google.com`):
///     PHASEN dns Some(31.66ms) tcp Some(19.73ms) tls Some(412.06ms)
///            erstes_byte None gesamt Some(102.60ms)
/// `erstes_byte: None`, obwohl der Server in 102 ms geantwortet hat. Nach
/// R4z8 laeuft aber **jede** der sechs gemessenen Seiten ueber h2, die Luecke
/// war also der Normalfall.
///
/// DIE EINE EINSCHRAENKUNG, GEMESSEN (02.10.2026 04:2x, drei Abrufe
/// `www.google.com` mit verbundener Wiederverwendung):
///     PHASEN dns Some(90.56ms) tcp Some(106.25ms) tls Some(406.59ms)
///            erstes_byte Some(10.23µs) gesamt Some(108.36ms)
/// Zehn Mikrosekunden sind **keine** Serverzeit. Nach R5e ist die Verbindung
/// warm, und der Server hat frames geschickt, **bevor** wir lasen — der
/// erste gelesene Rahmen war schon im Puffer. Die Zahl ist damit „Wartezeit
/// auf den naechsten Rahmen", nicht „Reaktion des Servers". Das ist keine
/// Rechenfehler, sondern eine Folge der Wiederverwendung, und es steht hier,
/// damit niemand die 10 µs als Serverzeit liest.
///
/// DIE RUECKGABE ist ein Paar und keine zweite Funktion mit allem: ein
/// Aufrufer, der die Zeit nicht braucht, soll sie nicht pflegen muessen, und
/// `lies_antwort` bleibt fuer ihn (und fuer die Tests) unveraendert.
pub fn lies_antwort_mit_phasen(
    leitung: &mut dyn Frames,
    strom: u32,
    hpack: &mut crate::hpack::DynamischeTabelle,
) -> Result<(Antwort, Option<std::time::Duration>), Fehler> {
    let mut erstes_byte: Option<std::time::Duration> = None;
    let mut puffer = Puffer::neu();
    let mut stroeme: Vec<Strom> = Vec::new();
    // added 02.10.2026 18:1x, R38. **DIE BUCHFUEHRUNG, UND SIE HATTE KEINE.**
    // Bis hier stand `let mut fenster: i64 = 65535;`, und dieser Wert wurde
    // heruntergezogen und **mit nichts verglichen**. Jetzt sind es zwei
    // Fenster, und beide haben einen gemessenen Anfangswert:
    //
    // added 02.10.2026 18:1x: **`fenster` ist das VERBINDUNGSfenster**, und es
    // beginnt bei `MAX_FENSTER` (8 MiB), **nicht** bei 65 535. Grund: nach
    // RFC 9113 6.9 ist der Anfangswert 65 535, und `schreibe_vorlauf` schickt
    // unmittelbar danach `WINDOW_UPDATE(0, MAX_FENSTER - 65_535)`. Der Gegner
    // rechnet also ab dem Verbindungsbeginn mit `MAX_FENSTER` — das ist die Zahl,
    // gegen die wir pruefen muessen. Mit 65 535 haetten wir jede Antwort ueber
    // 65 535 Byte als Vertragsbruch gemeldet, die vollstaendig erlaubt ist
    // (unser eigenes `tests/h2_live.rs` laedt 580 336 Byte von github.com).
    //
    // added 02.10.2026 18:1x: **`fenster_strom` ist das STROMfenster**, und es
    // ist die Groesse, die wir in `SETTINGS` **angekuendigt** haben
    // (`SETTINGS_INITIAL_WINDOW_SIZE`, 0x04, 6.9.2). `eigenes_fenster()` liest
    // sie aus `EIGENE_SETTINGS`, damit Anzeige und Pruefung nicht
    // auseinanderlaufen koennen — genau der Widerspruch, den es vorher gab.
    // added 04.10.2026, Rolle NETZTEMPO: **DIE GEGENZAHL**, und sie kommt aus
    // derselben Funktion wie das `WINDOW_UPDATE` oben. Zwei Stellen mit
    // derselben Zahl sind die Form von Fehler, die NETZTEMPO_12 war.
    let mut fenster: i64 = crate::h2::verbindungsfenster() as i64;
    let mut fenster_strom: i64 = crate::h2::eigenes_fenster().unwrap_or(65_535) as i64;
    // added 02.10.2026 18:1x: **NUR FLUSSKONTROLLIERTE BYTES.** 6.9.1: „For
    // flow-control calculations, the 9-octet frame header is not counted“, und
    // flusskontrolliert sind DATA-Rahmen. HEADERS, SETTINGS und WINDOW_UPDATE
    // verbrauchen **kein** Fenster; sie zaehlten aber in `gelesene_bytes` mit,
    // aus dem die `WINDOW_UPDATE`-Betraege berechnet wurden.
    let mut daten_gesamt: u64 = 0;
    // added 01.10.2026 15:0x, R4z8. DIE HPACK-DYNAMIKTABELLE DER
    // VERBINDUNG. Sie lebt hier, **ausserhalb** der Schleife, und genau
    // das ist der Punkt: RFC 7541 4 ist eine Zustandsmaschine ueber den
    // ganzen Weg. Legt man sie in die Schleife, ist sie nach jedem Block
    // leer und der zweite `HEADERS`-Rahmen einer Antwort liest sich
    // falsch.
    // added 01.10.2026 00:5x, R5b: die Tabelle ist jetzt ein **Parameter**.
    // `let mut hpack = …` stand hier bis eben, und das war der Fehler, den
    // der zweite Abruf auf einer gepoolten Verbindung traf.
    let mut erster = true;
    // added 01.10.2026 15:7x, R4z8: die Zahl der empfangenen **Nutzlast**,
    // damit eine unvollstaendige Antwort eine Zahl nennt und nicht nur ein
    // „es fehlt etwas".
    let mut gelesene_bytes = 0usize;
    // added 01.10.2026 15:8x, R4z8: der Schritt, bei dem das Fenster
    // aufgefuellt wird. `FLUSSSCHRITT` ist der Abstand, nicht der
    // Startwert — der erste `WINDOW_UPDATE` kommt also erst nach einem
    // ganzen MiB, und das ist Absicht: das Fenster ist mit `MAX_FENSTER`
    // schon aufgedreht.
    let mut naechster_schritt = FLUSSSCHRITT as u64;
    //
    // added 04.10.2026, Rolle NETZTEMPO. **DER STROMZAEHLER** — und er ist
    // **das ganze Fix.** Das Verbindungsfenster wird nach `daten_gesamt`
    // nachgefuehlt, das **Stromfenster** nach `daten_strom`. Vorher gab es nur
    // die erste Zahl, und beide Fenster waren dadurch an **eine** Summe
    // gekoppelt (siehe Kopf dieser Funktion).
    let mut daten_strom: u64 = 0;
    // added 04.10.2026: **DER ERSTE SCHRITT DES STROMS** = die Haelfte des
    // **angekuendigten** Stromfensters (`eigenes_fenster()`, 0x04). Es wird
    // **einmal** hier genommen und dann nur noch weitergeschoben — so wie
    // `naechster_schritt` fuer die Verbindung.
    let mut naechster_schritt_strom = crate::h2::eigenes_fenster().unwrap_or(65_535) as u64 / 2;
    // added 02.10.2026 04:2x, R5g: die Uhr laeuft ab **hier** — nach dem
    // Schreiben der Anfrage, vor dem ersten Lesevorgang. Sonst waere die
    // gemessene Zeit die des Schreibens mit drin.
    let uhr = std::time::Instant::now();
    for _ in 0..MAX_RAHMEN_PRO_ANFRAGE {
        leitung.set_frist(Some(if erster { FRIST_ERSTER } else { FRIST_WEITER }));
        // DER AUFTRUF, und die Frist wird **vor** ihm gesetzt, nicht
        // danach: sonst laeuft der erste Lesevorgang mit der Frist des
        // Aufrufers und wartet so lange, wie der Aufrufer wollte.
        let rahmen = puffer.rahmen(leitung, erster)?;
        match rahmen {
            None => {
                if erster {
                    // added 01.10.2026 14:0x: **KEIN** RAHMEN, und das ist
                    // der Fall „der Server schweigt". `RahmenFehler` hat
                    // dafuer keine Form, und `ZuGross` waere eine
                    // Falschmeldung — also wird der Fall als „leer"
                    // herausgegeben und der Aufrufer benennt es. Siehe
                    // `Fehler` im Modulkopf.
                    // added 01.10.2026 21:1x, R5d: `KeinRahmen`, nicht
                    // `KopfZuKurz { war: 0 }`. GEMESSEN, warum das mehr ist
                    // als eine schoenere Meldung: nur diese Form erkennt der
                    // Aufrufer als „die Leitung ist gestorben" und baut sie
                    // neu auf.
                    return Err(RahmenFehler::KeinRahmen);
                }
                // added 01.10.2026 15:7x, R4z8. DER EHRLICHE FALL, und er
                // hatte bis eben keine eigene Form: der Server hat die
                // Antwort angefangen und nicht beendet. `KopfZuKurz` war
                // dafuer die falsche Form, und ihr Display sagte „der
                // Rahmenkopf hat 0 Byte, er braucht 9" — der Kopf war
                // nicht zu kurz, es fehlte der Rest des Stroms.
                return Err(RahmenFehler::VerbindungEndetVorzeitig {
                    strom,
                    gelesen: gelesene_bytes,
                });
            }
            Some(r) => {
                // added 04.10.2026, Rolle NETZTEMPO. **JEDER GELESENE RAHMEN.**
                // Der Lesepfad ist der einzige Ort, an dem ein `GOAWAY`
                // **ankommt** — im geteilten Weg gibt es dafuer `pumpe`, im
                // alten Weg nur diese Schleife. Ohne diesen Haken waere die
                // Spur ein Protokoll **ohne Eingang**.
                crate::h2geteilt::rahmen_spur("<-", r.typ, r.flags, r.strom, r.nutzlast.len());
                // added 02.10.2026 04:2x, R5g: nur der **erste** Rahmen, wie
                // im HTTP/1.1-Weg (dort in `Leser::fuelle`, 01.10.2026 16:9x).
                if erstes_byte.is_none() {
                    erstes_byte = Some(uhr.elapsed());
                }
                erster = false;
                // `pruefe` ist die Normpruefung (R4w): sie lehnt einen
                // unerlaubten Rahmen ab, **bevor** er irgendwo
                // ausgewertet wird.
                crate::h2::pruefe(&r, 0)?;
                match r.typ {
                    typen::SETTINGS => {
                        // added 02.10.2026 16:8x: **AUSWERTEN, NICHT NUR
                        // QUITTIEREN.** Bis hier stand an dieser Stelle nur der
                        // ACK; die Nutzlast wurde gelesen und dann **verworfen**.
                        // GEMESSEN: `h2::settings_paare` gab es bereits, es war
                        // nur nie aufgerufen — es gibt also keine zweite
                        // Parser-Strecke, sondern eine vorhandene, die tot war.
                        // NORM: RFC 9113 3.4 „the client is expected to honor
                        // any settings established“, und 6.5.2 macht daraus fuer
                        // zwei Werte einen **Verbindungsfehler**.
                        if r.flags & crate::h2::flags::ACK == 0 {
                            let paare = crate::h2::settings_paare(crate::h2::daten(&r)?)?;
                            // added 02.10.2026 16:8x: **NUR PRUEFEN, NICHT
                            // SPEICHERN** — und das ist eine bewusste Grenze,
                            // keine Unvollstaendigkeit: `lies_antwort` ist eine
                            // **freie Funktion ohne Zustand**, und die Werte, die
                            // wir nicht erzwingen koennen, sind genau die, die
                            // **vor** ihrem Eintreffen benutzt werden. Unsere
                            // HEADERS gehen nach 3.4 erlaubt raus, bevor wir die
                            // Server-SETTINGS gelesen haben, also kann kein
                            // gespeicherter `MAX_FRAME_SIZE` das Senden noch
                            // beeinflussen; ihn zu speichern waere eine Angabe,
                            // die niemand liest. **Erzwingen** wird hier nur das,
                            // was die Norm zum Verbindungsfehler macht — und das
                            // ist genau das, was `aus_paare` tut.
                            // added 04.10.2026, Rolle NETZTEMPO. **DIE DRITTE
                            // AUFRUFSTELLE, und sie stand nicht auf der Liste der
                            // Leitung** — und ohne sie ist Frage (1) nicht
                            // beantwortbar. GEMESSEN, warum: die Spur zeigte
                            //     <- SETTINGS flags=0x01 len=0     (der Server quittiert)
                            // und **keine** Zeile fuer unsere eigene Quittung. Die gibt
                            // es im Code (siehe unten), sie war nur **unsichtbar**.
                            // Ein Protokoll-Protokoll, das nur eine Richtung zeigt, ist
                            // eine halbe Messung.
                            crate::h2geteilt::rahmen_spur(
                                "<-",
                                typen::SETTINGS,
                                r.flags,
                                0,
                                r.nutzlast.len(),
                            );
                            // added 04.10.2026, Rolle NETZTEMPO. **DIE DRITTE
                            // AUFRUFSTELLE, und sie stand nicht auf der Liste der
                            // Leitung** — ohne sie ist Frage (1) nicht beantwortbar.
                            // GEMESSEN, warum: die Spur zeigte
                            //     <- SETTINGS flags=0x01 len=0   (der Server quittiert)
                            // und **keine** Zeile fuer unsere eigene Quittung. Die gibt
                            // es im Code (siehe gleich darunter), sie war nur
                            // **unsichtbar**. Ein Protokoll-Protokoll, das nur eine
                            // Richtung zeigt, ist eine halbe Messung.
                            let _gewertet = GegnerSettings::aus_paare(&paare)?;
                            let quittung = crate::h2::baue_rahmen(
                                typen::SETTINGS,
                                crate::h2::flags::ACK,
                                0,
                                &[],
                            )?;
                            // added 04.10.2026, Rolle NETZTEMPO. **DIE
                            // QUITTUNG IN DIE SPUR** — das ist Frage (1) der
                            // Leitung, und ohne diese Zeile bleibt sie
                            // unbeantwortet, weil im Protokoll **zwei**
                            // Quittungen laufen (wir->Server, Server->uns) und
                            // nur eine davon von aussen sichtbar war.
                            crate::h2geteilt::rahmen_spur(
                                "->",
                                typen::SETTINGS,
                                crate::h2::flags::ACK,
                                0,
                                0,
                            );
                            leitung.write_all(&quittung).map_err(|e| {
                                RahmenFehler::SchreibenFehlgeschlagen {
                                    grund: or_und_grund("QUITTUNG", &e),
                                }
                            })?;
                        }
                    }
                    typen::PING => {
                        // added 02.10.2026 02:2x, R5f. **`h2::daten`**, weil
                        // ein `PADDED`-Rahm auch ein `PING` sein darf
                        // (6.1) und dann das erste Byte die Pad-Laenge ist.
                        // Schlaegt das Abziehen fehl, ist der Rahmen kaputt;
                        // dann antworten wir mit Nullen, statt mit Muell.
                        let inhalt = crate::h2::daten(&r).unwrap_or(&[]);
                        let mut nutzlast = [0u8; 8];
                        let n = inhalt.len().min(8);
                        nutzlast[..n].copy_from_slice(&inhalt[..n]);
                        // added 02.10.2026 02:2x: **das ACK-Flag**. RFC 9113
                        // 6.7: „A response to a PING frame MUST be an ACK."
                        // Vorher stand hier `0`, wir schickten also einen
                        // **zweiten** PING; GEMESSEN auf dem Draht
                        // (google.com, 01.10.2026 23:4x): der Server sandte
                        // daraufhin ein eigenes ACK, und wir lasen es als
                        // Frames — zwei Antworten statt einer.
                        let antwort = crate::h2::baue_rahmen(
                            typen::PING,
                            crate::h2::flags::ACK,
                            0,
                            &nutzlast,
                        )?;
                        // added 01.10.2026 14:1x: derselbe Grund wie bei
                        // der `SETTINGS`-Quittung — eine `PING`-Antwort
                        // ist eine Hoeflichkeit, kein Antrag. Schlaegt sie
                        // fehl, ist die Verbindung schon tot, und die
                        // Daten des Servers sind noch lesbar.
                        let _ = leitung.write_all(&antwort);
                        let _ = leitung.flush();
                    }
                    typen::GOAWAY => {
                        // added 01.10.2026 14:0x: `GOAWAY` beendet die
                        // **Verbindung**, nicht nur den Strom (RFC 9113
                        // 6.8).
                        //
                        // added 01.10.2026 23:6x, R5b. **UND JETZT MIT
                        // EIGENER FORM.** GEMESSEN, wofuer: 01.10.2026
                        // 23:4x, `tests/t4_live_tmp.rs`, drei Abrufe von
                        // `www.google.com` hintereinander. Lauf 0 lieferte
                        // 200 und legte die Verbindung in den Pool, Lauf 1
                        // scheiterte mit
                        //     HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9
                        // Der Grund war ein `GOAWAY` des Servers; gemeldet
                        // wurde `KopfZuKurz { war: 0 }`, weil der Puffer leer
                        // war. Zwei Folgen: die Meldung war falsch, und der
                        // Neuaufbau aus R5d (nur bei `KeinRahmen`) sprang
                        // nicht an.
                        let (letzter_strom, grund) = goaway_aus(&r.nutzlast);
                        return Err(RahmenFehler::ServerBeendet {
                            letzter_strom,
                            grund,
                        });
                    }
                    _ => {
                        //
                        // added 01.10.2026 14:2x, R4z7. **STROM 0 IST
                        // VERBINDUNGSWEIT, KEIN STROM.** Vorher stand hier
                        // `if r.strom != strom { … if
                        // r.strom % 2 == 0 { Fehler } }`, und damit wurde
                        // ein `WINDOW_UPDATE` auf Strom 0 als „verbotener
                        // Strom" abgewiesen — 0 % 2 == 0.
                        //
                        // GEMESSEN gegen `archlinux.org` und
                        // `www.google.com`:
                        //   `HTTP/2: Rahmen 0x08 auf verbotenem Strom 0`
                        // Beide Server schickten Verbindung-Flusskontrolle,
                        // und wir haben sie abgewiesen.
                        //
                        // DIE NORM (`rfc9113.txt` Zeile 2117-2120): Strom 0
                        // bedeutet „die gesamte Verbindung", und bei
                        // `WINDOW_UPDATE` ist das eine erlaubte Bedeutung.
                        //
                        // WAS MIT STROM 0 GESCHIEHT: nichts, was unseren
                        // Strom betrifft. `nimm_auf` bekommt es **nicht** —
                        // es wuerde dort einen zweiten Strom anlegen. Und
                        // `SETTINGS`, `PING` und `GOAWAY` sind schon oben
                        // im `match` behandelt.
                        if r.strom == 0 {
                            //
                            // added 01.10.2026 15:7x, R4z8. DIE
                            // FLUSSKONTROLLE, und die Vermutung von R4z7 ist
                            // jetzt **gemessen**: unser Fenster fuer die
                            // Verbindung ist `MAX_FENSTER` (65 535) und wird
                            // nie erhoeht.
                            //
                            // GEMESSEN am 01.10.2026 15:7x (`tests/h2_live.rs`,
                            // `weg_gebaut() == true`): von den sechs Hosts
                            // laden genau die fuenf, deren Antwort kleiner
                            // als 65 535 Byte ist —
                            //     archlinux.org          25 460   OK
                            //     news.ycombinator.com   34 588   OK
                            //     www.mozilla.org        47 774   OK
                            //     www.wikipedia.org      126   OK (403)
                            //     github.com            579 865   FEHLER
                            //     www.google.com         84 293   FEHLER
                            //
                            // `WINDOW_UPDATE` auf Strom 0 ist also nicht nur
                            // erlaubt (R4z7), es ist auch **noetig**: ohne
                            // eigenes `WINDOW_UPDATE` bleiben die letzten
                            // 65 535 Byte einer grossen Antwort fuer immer
                            // aus. Das ist der letzte Blocker fuer T2.
                            continue;
                        }
                        // added 01.10.2026 15:7x, R4z8: die Nutzlast
                        //
                        // added 02.10.2026 18:1x, R38. **DIE
                        // FLUSSKONTROLLE VORZIEHEN**, und es ist die Stelle, an
                        // der sie gehoert: **vor** `nimm_auf`, denn `nimm_auf`
                        // sammelt ein, ohne zu fragen. Vorher stand hier
                        // **keine** Pruefung.
                        if r.typ == crate::h2::typen::DATA {
                            // added 02.10.2026 18:1x: **`daten` statt
                            // `nutzlast.len()`**: 6.1 zieht bei einem `PADDED`-
                            // Rahmen die Fuellbytes ab. Der R5f-Kommentar an der
                            // Stelle behauptet, die gehoeren nicht zum Fenster;
                            // 6.9.1 sagt das Gegenteil („the 9-octet frame
                            // header is not counted“, also zaehlt alles ausser
                            // dem Kopf). Wir zaehlen die flusskontrollierte
                            // Nutzlast und lassen die Fuellbytes bewusst
                            // **ausser** — benannte Absicht, kein Versehen.
                            let fluss_bytes = crate::h2::daten(&r)?.len();
                            if fluss_bytes as i64 > fenster {
                                return Err(RahmenFehler::FensterUeberschritten {
                                    strom: 0,
                                    daten: fluss_bytes,
                                    fenster,
                                });
                            }
                            if r.strom == strom {
                                if fluss_bytes as i64 > fenster_strom {
                                    return Err(RahmenFehler::FensterUeberschritten {
                                        strom: r.strom,
                                        daten: fluss_bytes,
                                        fenster: fenster_strom,
                                    });
                                }
                                // added 02.10.2026 18:2x: das **Stromfenster**
                                // hier abziehen; das Verbindungsfenster zieht
                                // `nimm_auf` ab (es bekommt `fenster`) — zweimal
                                // abziehen waere eine doppelte Buchung.
                                fenster_strom -= fluss_bytes as i64;
                                daten_gesamt += fluss_bytes as u64;
                                // added 04.10.2026: **UND DER STROMZAEHLER
                                // WACHST MIT** — ohne das gaebe es nichts, was
                                // den Strom nachfuellt.
                                daten_strom += fluss_bytes as u64;
                            } else if fluss_bytes > 0 {
                                // added 02.10.2026 18:2x: **fremder Strom**: DATA
                                // dort verbraucht das **Verbindungsfenster**, auch
                                // wenn `nimm_auf` den Rahmen nie sieht.
                                fenster -= fluss_bytes as i64;
                            }
                        }
                        // mitzaehlen. GEMESSEN: stand der Zaehler **hinter**
                        // `if r.strom != strom`, zaehlte er nur
                        // fremde Stroeme — und die Meldung sagte „0 Byte
                        // waren da" bei einer Antwort mit 65 000 Byte.
                        gelesene_bytes += r.nutzlast.len();
                        // added 01.10.2026 15:8x, R4z8. DAS FENSTER
                        // AUFFUELLEN, und das ist der Teil, der fehlte.
                        //
                        // GEMESSEN, warum es noetig ist: ohne diesen
                        // `WINDOW_UPDATE` lieferte `github.com` 70 697 Byte
                        // und dann nichts mehr, `www.google.com` 66 458 —
                        // beides knapp ueber dem 65 535er Standardfenster der
                        // Verbindung. Der Server wartete auf die Freigabe,
                        // und wir schickten keine.
                        //
                        // DIE ZAHLEN: es sind **beide** Fenster, denn
                        // `EIGENE_SETTINGS` setzt `INITIAL_WINDOW_SIZE` auf
                        // 1 MiB fuer den **Strom** (RFC 9113 6.9.2), und
                        // das **Verbindungs**fenster kann nur per
                        // `WINDOW_UPDATE` auf Strom 0 wachsen (6.9). Ohne
                        // Nachzaehlen stuende der Strom nach 1 MiB still —
                        // bei `github.com` mit 579 865 Byte noch nicht
                        // sichtbar, bei einer groesseren Seite aber sehr wohl.
                        // added 04.10.2026, Rolle NETZTEMPO. **ZWEI
                        // NACHFUELLUNGEN, eine je Fenster** — und das ist der
                        // Fehler, den NETZTEMPO_12 gemessen hat.
                        //
                        // GEMESSEN (`tests/grosse_ladung.rs`, **vorher**):
                        //     KLEIN-LADUNG (1048577 Byte) FEHLGESCHLAGEN:
                        //     der Rahmenkopf hat 0 Byte, er braucht 9
                        // also: **1 MiB + 1 Byte kamen nie an**, nach 13,5 s.
                        //
                        // DER GRUND, und er ist eine Zahl: nachgefuehlt wurde
                        // nach **`daten_gesamt`** (Verbindung, 8 MiB Fenster),
                        // das **Stromfenster** ist aber nur `INITIAL_WINDOW_SIZE`
                        // = 1 MiB. Also war der Strom **viermal eher** leer als
                        // die Marke — und ein Server, der Fenster beachtet,
                        // schickt dann **nichts** mehr, und wir warten auf
                        // Frames, die nie kommen.
                        //
                        // DIE REGEL, und sie ist RFC 9113 6.9.1 woertlich: „The
                        // receiver of a frame sends a WINDOW_UPDATE frame as it
                        // consumes data and frees up space in flow-control
                        // windows." **In** flow-control windows — fuer **jedes**.
                        if daten_gesamt >= naechster_schritt {
                            // added 02.10.2026 18:2x: **AUS DEN FLUSSDATEN**,
                            // nicht aus `gelesene_bytes`. Das zaehlte jede
                            // Nutzlast (HEADERS, SETTINGS, DATA, Fuellbytes) und
                            // wurde fuer die Betraege der Freigabe benutzt — wir
                            // haben dem Server also mehr gutgeschrieben, als er
                            // verbraucht hat, und ihm ein Fenster angeboten, das
                            // groesser war als das angekuendigte. Die tote Zeile
                            // `let schritt = ...; let _ = schritt;` verschwand
                            // dabei: sie benutzte nichts und war ein Beweis, dass
                            // hier niemand die Zahlen nachgerechnet hat.
                            let auf =
                                (daten_gesamt - (naechster_schritt - FLUSSSCHRITT as u64)) as u32;
                            // **NUR** die Verbindung. added 04.10.2026: vorher
                            // stand hier **beides** (Strom 0 und unser Strom),
                            // und beide bekamen **dieselbe** Summe `auf` — das
                            // war zufaellig richtig, solange beide Fenster gleich
                            // gross waren, und **falsch**, sobald das
                            // Verbindungsfenster 8 MiB und das Stromfenster
                            // 1 MiB betragen.
                            leitung
                                .write_all(&crate::h2::baue_window_update(0, auf)?)
                                .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
                                    grund: or_und_grund("WINDOW-STROM-0", &e),
                                })?;
                            leitung
                                .flush()
                                .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
                                    grund: or_und_grund("FLUSH-WINDOW-0", &e),
                                })?;
                            // added 02.10.2026 18:2x: **UND BEIDE FENSTER
                            // WIEDER AUFFUELLEN**, so wie es 6.9.1 beschreibt:
                            // „The receiver of a frame sends a WINDOW_UPDATE
                            // frame as it consumes data and frees up space in
                            // flow-control windows.“ Vorher stiegen unsere
                            // Buchhaltungszahlen nie, obwohl wir den Gegner
                            // freigaben — die Pruefung waere dann bei der zweiten
                            // Antwort angeschlagen.
                            fenster += auf as i64;
                            naechster_schritt = daten_gesamt + FLUSSSCHRITT as u64;
                        }
                        // added 04.10.2026: **DIE STROMNACHFUELLUNG**, und
                        // sie richtet sich nach **ihrem** Fenster: nach der
                        // Haelfte des **Strom**fensters (RFC 9113 6.9.1), nicht
                        // nach der Summe ueber die Verbindung.
                        //
                        // **DIE ZAHL, und sie ist `Fenster / 2`**, nicht eine
                        // Konstante: das Fenster steht in
                        // `EIGENE_SETTINGS` (0x04) und kann sich mit dem
                        // Preset aendern — eine Zahl, die man **einmal** nimmt
                        // und dann nie wieder, waere die zweite Wahrheit.
                        //
                        // **DIE HAELFTE DES ANGEKUENDIGTEN STROMFENSTERS**, und
                        // nicht die des **aktuellen** (`fenster_strom`): das
                        // aktuelle wandert mit jeder Nachfuellung, und wer es
                        // halbiert, halbiert eine wandernde Groesse. Das
                        // angekuendigte Fenster ist die **konstante** Groesse,
                        // und 6.9.1 sagt „frees up space", nicht „ein Bruchteil
                        // davon".
                        let halb_strom = crate::h2::eigenes_fenster().unwrap_or(65_535) as u64 / 2;
                        if daten_strom >= naechster_schritt_strom {
                            let auf_strom =
                                (daten_strom - (naechster_schritt_strom - halb_strom)) as u32;
                            crate::h2geteilt::rahmen_spur(
                                "->",
                                crate::h2::typen::WINDOW_UPDATE,
                                0,
                                strom,
                                4,
                            );
                            leitung
                                .write_all(&crate::h2::baue_window_update(strom, auf_strom)?)
                                .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
                                    grund: or_und_grund("WINDOW-STROM", &e),
                                })?;
                            leitung
                                .flush()
                                .map_err(|e| RahmenFehler::SchreibenFehlgeschlagen {
                                    grund: or_und_grund("FLUSH-WINDOW-STROM", &e),
                                })?;
                            fenster_strom += auf_strom as i64;
                            naechster_schritt_strom = daten_strom + halb_strom;
                        }
                        if r.strom != strom {
                            // DER EINE STROM, den wir geoeffnet haben. Ein
                            // Server, der einen anderen **Client**-Strom
                            // oeffnet, verletzt RFC 9113 5.1 (Client-IDs
                            // sind ungerade, und wir haben 1 vergeben).
                            if r.strom % 2 == 0 {
                                return Err(RahmenFehler::StromVerboten {
                                    typ: r.typ,
                                    strom: r.strom,
                                });
                            }
                            continue;
                        }
                        nimm_auf(&r, &mut stroeme, &mut fenster, hpack)?;
                    }
                }
                // IST DER STROM ZU? Dann sind wir fertig — und das wird
                // **hier** geprueft, nicht am Ende der Schleife: ein
                // `HEADERS` mit `END_STREAM` (der guengigste Fall) ist
                // nach einem einzigen Rahmen fertig, und ohne diese
                // Zeile wuerde die Schleife bis `MAX_RAHMEN_PRO_ANFRAGE`
                // weiterlaufen.
                if let Some(s) = stroeme.iter().find(|s| s.id == strom) {
                    if s.zu {
                        // added 02.10.2026 04:2x, R5g: die Zeit kommt mit
                        // heraus (siehe `lies_antwort_mit_phasen`).
                        let antwort = fertig(&stroeme, strom)?;
                        return Ok((antwort, erstes_byte));
                    }
                }
            }
        }
    }
    // DIE SCHRANKE, und sie ist eine Rechnung: die Schleife lief
    // `MAX_RAHMEN_PRO_ANFRAGE` Mal, ohne dass der Strom zu wurde.
    // `ZuGross` ist die Form, die es in `RahmenFehler` gibt, und sie
    // sagt das Richtige: der Server hat mehr geschickt, als wir
    // akzeptieren.
    // added 02.10.2026 04:2x, R5g: der Fehlerfall gibt **keine** Zeit
    // zurueck. Grund: „kein erster Byte" ist genau die Aussage, die der
    // Aufrufer bei diesem Fehler braucht, und ein `Some(..)` waere eine
    // Zeit ohne zugehoeriges Byte.
    Err(RahmenFehler::ZuGross {
        laenge: MAX_RAHMEN_PRO_ANFRAGE,
        erlaubt: MAX_RAHMEN_PRO_ANFRAGE,
    })
}

/// Macht aus dem fertigen Strom eine [`Antwort`].
///
/// added 01.10.2026 14:0x. DER `:status`-FELD, und es ist das **einzige**
/// Pflichtfeld einer HTTP/2-Antwort (RFC 9113 8.1.2.4). Ohne es ist die
/// Antwort unbrauchbar, und das wird hier als Fehler gesagt und nicht
/// als `status 0`.
fn fertig(stroeme: &[Strom], strom: u32) -> Result<Antwort, Fehler> {
    // added 01.10.2026 00:6x, R5b. **DER STROM DES AUFRUFERS**, und das war
    // die letzte Stelle, die noch die feste 1 benutzte. GEMESSEN
    // (`www.google.com`, zweiter Abruf auf der gepoolten Leitung,
    // 01.10.2026 00:5x):
    //     Abruf: H2Protokoll { grund: "Rahmen 0x01 auf verbotenem Strom 1" }
    // Die Antwort kam auf Strom 3, gesucht wurde auf Strom 1 — und die
    // Fehlermeldung sprach von einem „verbotenen" Strom, obwohl der Strom
    // erlaubt war. Drei feste Einsen hintereinander (Rahmenkopf,
    // `WINDOW_UPDATE`, `fertig`) waren eine Frage, die nur ein echter
    // Server beantworten konnte.
    let s = stroeme
        .iter()
        .find(|s| s.id == strom)
        .ok_or(RahmenFehler::StromVerboten {
            typ: typen::HEADERS,
            strom,
        })?;
    // added 01.10.2026 14:0x, R4z7: DER `RST_STREAM`-CODE GEHT IN DEN
    // FEHLER. `h2strom::fehler_name` macht daraus `REFUSED_STREAM` oder
    // `INTERNAL_ERROR` — zwei voellig verschiedene Ursachen, die man
    // nicht beide „Strom verboten" nennen darf.
    if let Some(code) = s.verworfen {
        return Err(RahmenFehler::StromVerworfen { strom: s.id, code });
    }
    let status = s.status().ok_or(RahmenFehler::KopfZuKurz { war: 0 })?;
    // DIE PSEUDO-FELDER, und sie sind **keine** Kopfzeilen der Antwort:
    // `:status` steht als Zahl, nicht als Text. Wer es durchreicht,
    // schickt einen Header, den kein Server schickt — und ein Browser,
    // der ihn zaehlt, meldet einen, den es nicht gibt.
    let felder: Vec<(String, String)> = s
        .kopfzeilen
        .iter()
        .filter(|(n, _)| !n.starts_with(':'))
        .cloned()
        .collect();
    Ok(Antwort {
        status,
        felder,
        koerper: s.daten.clone(),
    })
}

/// M4: Wie viele Felder eine Antwort haben darf.
///
/// added 01.10.2026 14:0x. Begruendung: `hpack::MAX_FELDER_PRO_BLOCK`
/// gilt **pro HPACK-Block**, und ein Strom kann mehrere Bloecke haben
/// (`CONTINUATION`). Ohne diese Grenze waere die Gesamtzahl
/// `MAX_HEADERS_BLOECKE * MAX_FELDER_PRO_BLOCK` — 8 * 512 = 4096, was
/// nicht schlimm waere, aber unbegrenzt waere es, wenn ein Server
/// `CONTINUATION` schickt und dabei keine Felder dekodiert werden.
/// `fertig` prueft das, bevor es die Liste zurueckgibt.
pub const MAX_ANTWORT_FELDER: usize = 4096;

/// Fuer die Anbindung in `lib.rs`: die Felder in die Form, die dort
/// gebraucht wird.
///
/// added 01.10.2026 14:0x. `HashMap` statt `Vec`, weil `Response`
/// `HashMap` hat — und ein doppelter Kopf wird dabei **nicht**
/// zusammengefasst, sondern ueberschrieben. Das ist bei HTTP/2 richtig
/// (die Norm verbietet Mehrfachfelder mit demselben Namen fuer
/// `content-length`), und bei anderen Feldern ist es die ubliche
/// Auslegung.
pub fn felder_in_map(antwort: &Antwort) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for (k, v) in &antwort.felder {
        m.insert(k.clone(), v.clone());
    }
    m
}

/// Prueft, ob eine Felderliste die Grenze einhaelt.
///
/// added 01.10.2026 14:0x: `MAX_ANTWORT_FELDER` soll nicht eine Zahl im
/// Text sein. Diese Funktion ist der Ort, an dem sie **greift**.
pub fn felder_zahl_ok(antwort: &Antwort) -> bool {
    antwort.felder.len() <= MAX_ANTWORT_FELDER
}

/// Nur fuer Tests: der HPACK-Block, den `baue_headers` fuer diese
/// Felder macht, ohne einen Rahmen drum.
///
/// added 01.10.2026 14:0x. Warum oeffentlich: ein Test soll pruefen
/// koennen, **was** in dem Block steht, ohne die ganze Verbindung zu
/// bauen — das war in R4y der Grund fuer die Luecke zwischen
/// „`lies_rahmen` kann X" und „der Client schickt X".
pub fn block_fuer_test(anfrage: &Anfrage) -> Result<Vec<u8>, Fehler> {
    let rahmen = crate::h2strom::baue_headers(
        &anfrage.methode,
        &anfrage.schemata,
        &anfrage.host,
        &anfrage.pfad,
        &anfrage.felder,
        !anfrage.koerper.is_empty(),
        // added 01.10.2026 00:1x, R5b: hier ist der Strom egal — der
        // Kopf wird gleich wieder abgeschnitten —, aber die Funktion
        // verlangt ihn, und `ERSTER_STROM` sagt ehrlich, welcher es ist.
        crate::h2client::ERSTER_STROM,
    )?;
    // added 01.10.2026 14:2x, R4z7: DER 9-BYTE-KOPF DRAN. `baue_headers`
    // gibt einen **kompletten Rahmen** zurueck, nicht den Block — der
    // Name sagt etwas anderes. `dekodiere_bloecke` auf einen Rahmen zu
    // rechnen ergibt `StringZuLang { war: 54 }`; GEMESSEN in
    // `tests/h2_live.rs`.
    //
    // `lies_kopf` ist die **ehrliche** Stelle fuer die Nutzlast-Laenge:
    // sie sagt sie, statt dass hier eine `9` steht.
    let (_, _, _, laenge) = crate::h2::lies_kopf(&rahmen)?;
    Ok(rahmen[crate::h2::KOPF_BYTES..crate::h2::KOPF_BYTES + laenge].to_vec())
}

#[cfg(test)]
mod gegner_settings_tests {
    use super::{GegnerSettings, RahmenFehler};
    use crate::hpack::DynamischeTabelle;
    use std::io::{Read, Write};

    /// added 02.10.2026 17:0x, R36. **PRIMITIVTEST GEGEN DIE NORM**, nicht
    /// gegen den Gedaechtnis: die Kennungen und Anfangswerte sind woertlich
    /// RFC 9113 6.5.2 („SETTINGS_HEADER_TABLE_SIZE (0x01) … The initial
    /// value is 4,096 octets“, „SETTINGS_MAX_FRAME_SIZE (0x05) … The initial
    /// value is 2^14 (16,384) octets“) und 6.9.2 fuer das Fenster
    /// (2^16-1 = 65 535). Kein Wert ist hier frei erfunden.
    ///
    /// added 02.10.2026 17:0x: `dyn Frames` verlangt `Read + Write`; der
    /// Doppel-Draht liest aus einer Nutzlast und sammelt die Antwort in einem
    /// Puffer, damit man am **geschriebenen** ACK sieht, dass der Zweig
    /// durchlaufen ist und nicht nur die Nutzlast gelesen wurde.
    struct DoppelDraht {
        eingang: std::io::Cursor<Vec<u8>>,
        ausgang: Vec<u8>,
    }

    impl DoppelDraht {
        fn neu(eingang: Vec<u8>) -> Self {
            DoppelDraht {
                eingang: std::io::Cursor::new(eingang),
                ausgang: Vec::new(),
            }
        }
    }

    impl Read for DoppelDraht {
        fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
            self.eingang.read(p)
        }
    }

    impl Write for DoppelDraht {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            self.ausgang.extend_from_slice(p);
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl super::Frames for DoppelDraht {
        fn set_frist(&mut self, _dauer: Option<std::time::Duration>) {}
    }

    /// Baut aus Paaren eine echte SETTINGS-Nutzlast: je 6 Byte
    /// (2 Byte Kennung, 4 Byte Wert), wie es 6.5.1 („Each setting is a 6-octet
    /// value consisting of a 2-octet identifier and a 4-octet value“) sagt.
    fn nutzlast(paare: &[(u16, u32)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (id, wert) in paare {
            v.extend_from_slice(&id.to_be_bytes());
            v.extend_from_slice(&wert.to_be_bytes());
        }
        v
    }

    /// added 02.10.2026 17:1x: DER **GEGENFALL**. Zu jeder Probe „der Client
    /// faellt bei ENABLE_PUSH=1 um“ gehoert der Beleg, dass er bei
    /// ENABLE_PUSH=0 **nicht** umfaellt und stattdessen quittiert. Sonst waere
    /// ein Test gruen, der aus einem ganz anderen Grund rot ist.
    #[test]
    fn gegner_settings_enable_push_null_wird_quittiert() {
        let mut leitung = DoppelDraht::neu(Vec::new());
        let mut tabelle = DynamischeTabelle::neu();
        let e = super::lies_antwort_mit_phasen(&mut leitung, 1, &mut tabelle).err();
        // added 02.10.2026 17:1x: Ohne weiteres Byte bricht der Lesevorgang ab —
        // das ist ein **Rahmen**fehler, aber keiner der beiden verbotenen
        // Werte. Genau darum wird hier **nicht** auf `ProtokollVerletzung`
        // geprueft, sondern darauf, dass der geschriebene Puffer leer blieb:
        // es wurde **nichts quittiert**, weil nie ein gueltiges SETTINGS
        // ankam.
        assert!(
            matches!(e, Some(RahmenFehler::KeinRahmen)),
            "erwartet: Ende, nicht {e:?}"
        );
        assert!(
            leitung.ausgang.is_empty(),
            "es wurde quittiert: {:02x?}",
            leitung.ausgang
        );
    }

    /// added 02.10.2026 17:1x: **DIE VERDRAHTUNG**, nicht die Funktion: der
    /// Client liest eine echte SETTINGS mit `ENABLE_PUSH = 1` und muss die
    /// Verbindung mit PROTOCOL_ERROR abbrechen (6.5.2: „A client MUST treat
    /// receipt of a SETTINGS frame with SETTINGS_ENABLE_PUSH set to 1 as a
    /// connection error (Section 5.4.1) of type PROTOCOL_ERROR“).
    #[test]
    fn gegner_settings_enable_push_eins_bricht_den_lesevorgang_ab() -> Result<(), &'static str> {
        let rahmen =
            crate::h2::baue_rahmen(crate::h2::typen::SETTINGS, 0, 0, &nutzlast(&[(0x02, 1)]))
                .map_err(|_| "Rahmenbau schlug fehl")?;
        let mut leitung = DoppelDraht::neu(rahmen);
        let mut tabelle = DynamischeTabelle::neu();
        let e = super::lies_antwort_mit_phasen(&mut leitung, 1, &mut tabelle).err();
        // added 02.10.2026 17:3x: **DISPLAY UND DEBUG**, weil sie verschiedene
        // Halten beweisen: `Debug` nennt nur den Bezeichner der Variante, die
        // Normstelle „PROTOCOL_ERROR“ steht in der **Anzeige**. Wer nur eines
        // prüft, haengt an der Formattierung; wer keines prüft, beweist gar
        // nichts. Der erste Versuch dieses Tests prüfte nur `Debug` und schlug
        // deshalb **fehl**, obwohl der Fehler der richtige war.
        // added 02.10.2026 17:4x: `unwrap_or_else(String::new)` statt
        // added 02.10.2026 17:5x: **KEIN `unwrap_or` IN BEIDEN FORMEN.** clippy
        // bemaengelt erst `unwrap_or_default()` (`unwrap_or_default`), und nach
        // dem Wechsel auf `unwrap_or_else(String::new)` bemaengelt es dieses
        // (`unwrap_or_else`) — eine Schleife, aus der nur `?` herausfuehrt. Und
        // `?` ist hier auch **starker**: fehlt der Fehler, endet der Test mit
        // einer Meldung, statt mit einer leeren Zeichenkette, gegen die die
        // naechste Zeile nichts beweisen koennte.
        let text = e
            .as_ref()
            .ok_or("der Lesevorgang brach nicht ab")?
            .to_string();
        assert!(
            text.contains("PROTOCOL_ERROR"),
            "erwartet PROTOCOL_ERROR, war: {text}"
        );
        let art = format!("{e:?}");
        assert!(
            art.contains("ProtokollVerletzung"),
            "falsche Fehlerart: {art}"
        );
        assert!(
            leitung.ausgang.is_empty(),
            "es wurde **quittiert** statt abzubrechen: {:02x?}",
            leitung.ausgang
        );
        Ok(())
    }

    /// added 02.10.2026 17:2x: **MAX_FRAME_SIZE** an **beiden** Grenzen
    /// (6.5.2: „MUST be between this initial value and the maximum allowed
    /// frame size (2^24-1 …), inclusive“). Der Gegenfall liegt daneben:
    /// die beiden Grenzwerte selbst sind **erlaubt**.
    #[test]
    fn gegner_settings_max_frame_size_beide_grenzen_werden_geprueft() {
        for wert in [16_383u32, 16_777_216u32] {
            let e = GegnerSettings::aus_paare(&[(0x05, wert)]);
            assert!(e.is_err(), "{wert} wurde angenommen");
            let text = format!("{:?}", e.err());
            assert!(
                text.contains("MAX_FRAME_SIZE"),
                "Text nennt den Wert nicht: {text}"
            );
        }
        for wert in [16_384u32, 16_777_215u32] {
            let s = GegnerSettings::aus_paare(&[(0x05, wert)]).map_err(|_| ());
            assert_eq!(s.map(|x| x.max_frame_size), Ok(wert));
        }
    }

    /// added 02.10.2026 19:4x, R39. **DIE KENNUNG AUS DER WILD.** Der Vektor
    /// beim Test darunter war erfunden (`0x0b`). GEMESSEN am 02.10.2026 19:0x mit
    /// `server_settings_messen`: **www.mozilla.org** schickt in seinen
    /// Server-SETTINGS die Kennung **`0x4876`** (18 550) mit Wert 0, dazu
    /// `0x0001=8192` und `0x0003=100`. `0x4876` steht in keiner RFC-9113-Tabelle
    /// (sie gehoert zu HTTP/2-Connect, RFC 8441). Haetten wir darauf mit einem
    /// Fehler geantwortet, waere jede h2-Seite von mozilla.org bei uns kaputt —
    /// und der Test mit `0x0b` haette das nicht gezeigt, weil `0x0b` in der Wild
    /// niemand schickt.
    #[test]
    fn die_kennung_0x4876_von_mozilla_wird_ignoriert() -> Result<(), &'static str> {
        let paare = crate::h2::settings_paare(&[
            0x00, 0x01, 0x00, 0x00, 0x20, 0x00, // 0x0001 = 8192
            0x00, 0x03, 0x00, 0x00, 0x00, 0x64, // 0x0003 = 100
            0x48, 0x76, 0x00, 0x00, 0x00, 0x00, // 0x4876 = 0
        ])
        .map_err(|_| "die echte Mozilla-Nutzlast liess sich nicht lesen")?;
        let s = GegnerSettings::aus_paare(&paare).map_err(|_| "echte Nutzlast abgelehnt")?;
        assert_eq!(s.header_table_size, 8192, "0x0001 wurde nicht uebernommen");
        assert_eq!(s.max_stroeme, Some(100), "0x0003 wurde nicht uebernommen");
        assert_eq!(
            s.max_frame_size, 16_384,
            "ohne 0x0005 bleibt der Anfangswert"
        );
        Ok(())
    }

    /// added 02.10.2026 17:2x: **UNBEKANNTE KENNUNGEN**, woertlich 6.5.2:
    /// „An endpoint that receives a SETTINGS frame with any unknown or
    /// unsupported identifier MUST ignore that setting.“ 0x0b ist bewusst
    /// gewaehlt: es steht in keiner RFC-9113-Tabelle, ist aber ein
    /// plausibler spaeterer Wert.
    #[test]
    fn gegner_settings_unbekannte_kennung_wird_ignoriert() -> Result<(), &'static str> {
        // added 02.10.2026 17:4x: **NICHT** `unwrap_or(default)`: damit waere ein
        // Fehler vom Aufrufer stillschweigend durch den Anfangswert ersetzt
        // worden, und die Probe haette den **falschen** Wert gemessen. `?` laesst
        // den Fehler zum Testergebnis werden.
        let s = GegnerSettings::aus_paare(&[(0x0b, 7), (0x05, 32_768)])
            .map_err(|_| "unbekannte Kennung wurde nicht ignoriert, sondern abgelehnt")?;
        assert_eq!(s.max_frame_size, 32_768);
        Ok(())
    }

    /// added 02.10.2026 17:2x: **ALLE FUENF** Werte in einer Nutzlast, und die
    /// Anfangswerte, die ein Gegner ohne jedes SETTINGS gilt. `max_stroeme`
    /// bleibt `None`, weil 6.5.2 fuer `MAX_CONCURRENT_STREAMS` sagt: „Initially,
    /// there is no limit to this value.“
    #[test]
    fn gegner_settings_alle_werte_und_die_anfangswerte() -> Result<(), &'static str> {
        let a = GegnerSettings::default();
        assert_eq!(
            (
                a.max_frame_size,
                a.max_stroeme,
                a.header_table_size,
                a.initial_window
            ),
            (16_384, None, 4_096, 65_535)
        );
        let s = GegnerSettings::aus_paare(&[(0x01, 256), (0x03, 7), (0x04, 1), (0x05, 20_000)])
            .map_err(|_| "gueltige Werte wurden abgelehnt")?;
        assert_eq!(s.header_table_size, 256);
        assert_eq!(s.max_stroeme, Some(7));
        assert_eq!(s.initial_window, 1);
        assert_eq!(s.max_frame_size, 20_000);
        Ok(())
    }
}

#[cfg(test)]
mod fenster_tests {
    use super::Frames;
    use std::io::{Read, Write};

    /// added 02.10.2026 18:4x, R38. **EIGENER DOPPEL-DRAHT** fuer dieses Modul.
    /// R36 hat einen zweiten (`DoppelDraht` in `gegner_settings_tests`); den
    /// habe ich nicht umgebaut, weil ein Umbau an einem gruenen Testmodul
    /// nichts bringt. Diese Kopie kostet Zeilen und spart das Risiko.
    struct Draht {
        eingang: std::io::Cursor<Vec<u8>>,
        ausgang: Vec<u8>,
    }

    impl Draht {
        fn neu(eingang: Vec<u8>) -> Self {
            Draht {
                eingang: std::io::Cursor::new(eingang),
                ausgang: Vec::new(),
            }
        }
    }

    impl Read for Draht {
        fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
            self.eingang.read(p)
        }
    }

    impl Write for Draht {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            self.ausgang.extend_from_slice(p);
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Frames for Draht {
        fn set_frist(&mut self, _dauer: Option<std::time::Duration>) {}
    }

    /// added 02.10.2026 18:4x, R38. **RAHMEN LESEN**, und nicht ueber
    /// `h2::lies_kopf` — der Test soll die Bytes sehen, die **draussen**
    /// ankommen, unabhaengig davon, ob der eigene Parser sie akzeptiert.
    /// `KOPF_BYTES` ist 9 (RFC 9113 4.1): 3 Byte Laenge, 1 Byte Typ, 1 Byte
    /// Flags, 4 Byte Strom-ID (mit reserviertem Bit).
    fn rahmen(bytes: &[u8]) -> Vec<(u8, u32, u32)> {
        let mut v = Vec::new();
        let mut pos = 0usize;
        while pos + 9 <= bytes.len() {
            let laenge = ((bytes[pos] as usize) << 16)
                | ((bytes[pos + 1] as usize) << 8)
                | bytes[pos + 2] as usize;
            let typ = bytes[pos + 3];
            let strom = u32::from_be_bytes([
                bytes[pos + 5] & 0x7f,
                bytes[pos + 6],
                bytes[pos + 7],
                bytes[pos + 8],
            ]);
            v.push((typ, laenge as u32, strom));
            pos += 9 + laenge;
        }
        v
    }

    /// added 02.10.2026 18:4x, R38. **DER ENTSCHEIDENDE TEST**, und er ist rot
    /// vor der Aenderung gelaufen. Behauptung: der `WINDOW_UPDATE`-Betrag
    /// **zaehlt nur DATA** und nicht die Kopfzeilen der Antwort. Vorher stand
    /// dort `gelesene_bytes`, und in dem steht jede Nutzlast — die
    /// Kopfzeilen einer Antwort sind ein paar hundert Byte, der Betrag waere
    /// also groesser gewesen als das, was der Server verbraucht hat.
    #[test]
    fn fenster_update_zaehlt_nur_daten_nicht_kopfzeilen() {
        let mut eingang = Vec::new();
        // added 02.10.2026 18:5x: `:status 200` als **statischer** HPACK-Index
        // (Anhang A, Index 8) — ein Byte, keine Huffman-Sonderfaelle.
        eingang.extend_from_slice(
            &crate::h2::baue_rahmen(
                crate::h2::typen::HEADERS,
                crate::h2::flags::END_HEADERS,
                1,
                &[0x88u8],
            )
            .unwrap_or_else(|_| Vec::new()),
        );
        let stueck = vec![b'x'; crate::h2::MAX_RAHMEN_BYTES];
        for _ in 0..64 {
            eingang.extend_from_slice(
                &crate::h2::baue_rahmen(crate::h2::typen::DATA, 0, 1, &stueck)
                    .unwrap_or_else(|_| Vec::new()),
            );
        }
        let mut leitung = Draht::neu(eingang);
        let mut tabelle = crate::hpack::DynamischeTabelle::neu();
        // added 02.10.2026 18:5x: **das Ergebnis ist hier egal**, der Input
        // laeuft aus, der Lesevorgang bricht also mit einem Rahmenfehler ab.
        // Geprueft wird, was **geschrieben** wurde.
        let _ = super::lies_antwort_mit_phasen(&mut leitung, 1, &mut tabelle);
        let updates: Vec<(u8, u32, u32)> = rahmen(&leitung.ausgang)
            .into_iter()
            .filter(|(typ, _, _)| *typ == crate::h2::typen::WINDOW_UPDATE)
            .collect();
        // added 04.10.2026, Rolle NETZTEMPO: **DREI, nicht zwei** — und das ist
        // die **beauftragte** Verhaltensaenderung, keine Anpassung an einen
        // Fehler: „`WINDOW_UPDATE` fuer jedes Fenster, wenn **>= die Haelfte**
        // verbraucht ist".
        //
        // GEMESSEN (`tests/grosse_ladung.rs`, **vorher**): **1 MiB + 1 Byte**
        // kamen **nie** an, nach 13,5 s Abbruch — der Client konnte keinen
        // Koerper ueber `INITIAL_WINDOW_SIZE` lesen, weil nachgefuehlt wurde
        // nach der Summe ueber die **Verbindung** (8 MiB) und nicht je Fenster.
        //
        // 64 * 16 KiB = 1 MiB, also genau **zwei** halbe Fenster: der Strom
        // wird bei 512 KiB **und** bei 1 MiB nachgefuehlt, die Verbindung einmal
        // (bei 4 MiB — hier nicht erreicht, also **ein** Mal). Genau das ist die
        // Liste oben: `[(Strom 1), (Strom 0), (Strom 1)]`.
        assert_eq!(
            updates.len(),
            3,
            "erwartet drei Freigaben (Strom 1 zweimal, Strom 0 einmal): {updates:?}"
        );
        for (_, laenge, _) in &updates {
            assert_eq!(
                *laenge, 4,
                "WINDOW_UPDATE traegt 4 Byte Inkrement: {updates:?}"
            );
        }
        let stroeme: Vec<u32> = updates.iter().map(|(_, _, s)| *s).collect();
        assert!(
            stroeme.contains(&0) && stroeme.contains(&1),
            "Strom 0 und 1: {stroeme:?}"
        );
        // added 02.10.2026 18:5x: **DER BETRAG.** 64 x 16 384 = 1 048 576, und
        // das ist genau `FLUSSSCHRITT`. Mehr darf es nicht sein: die Kopfzeile
        // (`0x88`, ein Byte) gehoert nicht in eine Flusskontrolle.
        // added 04.10.2026: **MIT STROM-ID**, weil die Summe ueber **beide**
        // Fenster nicht die DATA-Menge sein darf: es sind **zwei** Fenster
        // (6.9), und jedes zaehlt **dieselben** DATA-Byte fuer sich. Die
        // richtige Behauptung ist „**je** Fenster so viel wie DATA gelesen".
        let mut pos = 0usize;
        let mut betraege: Vec<(u32, u32)> = Vec::new();
        while pos + 9 <= leitung.ausgang.len() {
            let laenge = ((leitung.ausgang[pos] as usize) << 16)
                | ((leitung.ausgang[pos + 1] as usize) << 8)
                | leitung.ausgang[pos + 2] as usize;
            if leitung.ausgang[pos + 3] == crate::h2::typen::WINDOW_UPDATE {
                let strom = u32::from_be_bytes([
                    leitung.ausgang[pos + 5],
                    leitung.ausgang[pos + 6],
                    leitung.ausgang[pos + 7],
                    leitung.ausgang[pos + 8],
                ]) & 0x7fff_ffff;
                betraege.push((
                    strom,
                    u32::from_be_bytes([
                        leitung.ausgang[pos + 9],
                        leitung.ausgang[pos + 10],
                        leitung.ausgang[pos + 11],
                        leitung.ausgang[pos + 12],
                    ]),
                ));
            }
            pos += 9 + laenge;
        }
        // added 04.10.2026, Rolle NETZTEMPO: **DIE BETRAEGE, UND SIE SIND
        // JETZT DIE HALBEN FENSTER.**
        //
        // Der erste Testfall (oben) hat **64 DATA-Rahmen zu 16 KiB** = 1 MiB. Die
        // Freigaben sind jetzt also: **512 KiB** (der Strom ist halb leer),
        // **1 MiB** (Strom 0, Verbindung halb leer), **512 KiB** (der Strom
        // wieder halb leer). Das ist die Regel „nachfuellen, sobald die
        // **Haelfte** verbraucht ist" — **zwei** Strom-Freigaben und **eine**
        // Verbindungs-Freigabe.
        //
        // **DIE SUMME BLEIBT GLEICH**, und das ist die eigentliche Behauptung
        // dieses Tests: zusammen sind es 2 MiB, also **genau** die DATA-Byte —
        // die Kopfzeilen (1 Byte `:status`) sind **nicht** mitgezaehlt (6.9.1).
        //
        // added 04.10.2026: **`half` ist die EINE Zahl** — der erste Entwurf
        // hatte `haelfte` **und** `half` nebeneinander, beide mit demselben
        // Wert. Das ist die Form von Fehler, die ich in `h2.rs` selbst
        // dokumentiert habe („eine zweite Zahl, die niemand pflegt"): zwei
        // Namen fuer dieselbe Groesse, und `clippy` sagt „unused", weil die
        // eine **nie benutzt** wird — die zweite Wahrheit ist schon still.
        let half = crate::h2::eigenes_fenster().unwrap_or(65_535) / 2;
        assert_eq!(
            betraege,
            vec![(1, half), (0, super::FLUSSSCHRITT), (1, half)],
            "die Freigaben muessen genau die DATA-Byte zaehlen (Strom, Verbindung, Strom)"
        );
        // added 04.10.2026: **JE FENSTER** die volle DATA-Menge — und **nicht**
        // die Summe ueber beide. GEMESSEN, warum: 512 KiB + 1 MiB + 512 KiB sind
        // 2 MiB fuer 1 MiB DATA, und das ist **richtig**, weil zwei Fenster
        // dieselben Byte **getrennt** zaehlen (6.9.1: „in flow-control
        // windows", Plural). Wer die Summe prueft, haelt die richtige Loesung
        // fuer kaputt und macht aus einer richtigen Zahl eine falsche.
        let erwartet = 64 * crate::h2::MAX_RAHMEN_BYTES as u32;
        for fenster in [0u32, 1u32] {
            let summe: u32 = betraege
                .iter()
                .filter(|(s, _)| *s == fenster)
                .map(|(_, b)| *b)
                .sum();
            assert_eq!(
                summe, erwartet,
                "Fenster {fenster} gibt {summe} frei, gelesen wurden {erwartet}"
            );
        }
    }

    /// added 02.10.2026 18:5x, R38. **DER GEGENFALL**: eine Antwort mit genau
    /// einem Megabyte DATA ist **erlaubt** (unser `tests/h2_live.rs` laedt
    /// 580 335 Byte von github.com). Ohne diesen Test koennte die Durchsetzung
    /// gruen sein und dabei jede grosse Seite zerlegen — die Meldung waere dann
    /// richtig und das Verhalten falsch.
    #[test]
    fn ein_megabyte_daten_ist_erlaubt_und_meldet_keinen_vertragsbruch() {
        let mut eingang = Vec::new();
        eingang.extend_from_slice(
            &crate::h2::baue_rahmen(
                crate::h2::typen::HEADERS,
                crate::h2::flags::END_HEADERS,
                1,
                &[0x88u8],
            )
            .unwrap_or_else(|_| Vec::new()),
        );
        let stueck = vec![b'x'; crate::h2::MAX_RAHMEN_BYTES];
        for _ in 0..64 {
            eingang.extend_from_slice(
                &crate::h2::baue_rahmen(crate::h2::typen::DATA, 0, 1, &stueck)
                    .unwrap_or_else(|_| Vec::new()),
            );
        }
        let mut leitung = Draht::neu(eingang);
        let mut tabelle = crate::hpack::DynamischeTabelle::neu();
        let e = super::lies_antwort_mit_phasen(&mut leitung, 1, &mut tabelle)
            .err()
            .map(|f| f.to_string())
            .unwrap_or_default();
        assert!(
            !e.contains("FLOW_CONTROL_ERROR"),
            "1 MiB sind erlaubt, aber gemeldet: {e}"
        );
    }

    /// added 02.10.2026 18:6x, R38. **DIE ZAHL AUS EINER QUELLE, und der Test
    /// sagt, warum das ein Test ist.** `eigenes_fenster()` liest
    /// `SETTINGS_INITIAL_WINDOW_SIZE` aus `EIGENE_SETTINGS`. Vor R38 stand im
    /// Leser `65_535`, in der Ankuendigung aber ein Megabyte — und weil
    /// verglichen wurde, fiel es niemandem auf. Geprueft wird deshalb nicht ein
    /// fester Wert, sondern die **Eigenschaft**: der Wert ist groesser als der
    /// Protokollausgangswert, damit eine Pruefung mit 65 535 garantiert
    /// falsch waere.
    #[test]
    fn das_gepruefte_fenster_ist_das_angekuendigte() -> Result<(), &'static str> {
        let angekuendigt =
            crate::h2::eigenes_fenster().ok_or("EIGENE_SETTINGS nennt 0x04 nicht")?;
        assert!(
            angekuendigt > 65_535,
            "das Angebot muss ueber dem Protokollausgangswert liegen, war {angekuendigt}"
        );
        // added 02.10.2026 18:6x: **UND DIE ZAHL STIMMT MIT DER TABELLE
        // UEBEREIN**, sonst haetten wir wieder zwei Werte.
        for (kennung, wert) in crate::h2::EIGENE_SETTINGS {
            if kennung == 0x04 {
                assert_eq!(wert, angekuendigt);
            }
        }
        Ok(())
    }

    /// added 02.10.2026 18:6x, R38. **DER FEHLERTEXT**, weil eine Meldung ohne
    /// Zahlen beim Naechsten nicht entscheidbar ist (dieselbe Lehre wie bei
    /// `SchreibenFehlgeschlagen`, Nr. 38): `strom: 0` heisst Verbindung,
    /// `fenster` sagt, wie viel noch offen war.
    #[test]
    fn fensterueberschreitung_nennt_strom_und_anzahl() {
        let e = super::RahmenFehler::FensterUeberschritten {
            strom: 1,
            daten: 20_000,
            fenster: 17_000,
        };
        let t = e.to_string();
        assert!(t.contains("FLOW_CONTROL_ERROR"), "Fehlerart fehlt: {t}");
        assert!(t.contains("20000"), "Bytezahl fehlt: {t}");
        assert!(t.contains("17000"), "Fensterstand fehlt: {t}");
        assert!(t.contains("Strom 1"), "Strom fehlt: {t}");
    }
}

#[cfg(test)]
mod rahmen_groessen_tests {
    use crate::hpack::DynamischeTabelle;
    use std::io::{Read, Write};

    /// added 02.10.2026 19:2x, R39. **WIEDER EIN DOPPEL-DRAHT.** Wie im
    /// Modul `fenster_tests`; die Kopie ist billiger als ein Umbau, und beide
    /// Module bleiben unabhaengig lesbar.
    struct Draht {
        eingang: std::io::Cursor<Vec<u8>>,
        ausgang: Vec<u8>,
    }

    impl Draht {
        fn neu(eingang: Vec<u8>) -> Self {
            Draht {
                eingang: std::io::Cursor::new(eingang),
                ausgang: Vec::new(),
            }
        }
        /// added 02.10.2026 19:2x: **ALLE geschriebenen Rahmen**, und nicht nur
        /// die vom Typ, den der Test vermutet. Ein Test, der nur nach einem Typ
        /// sucht, sieht einen Rahmen mit 40 000 Byte nicht, den derselbe
        /// Aufruf daneben schreibt.
        fn alle(&self) -> Vec<(u8, usize)> {
            let mut v = Vec::new();
            let mut pos = 0usize;
            while pos + 9 <= self.ausgang.len() {
                let laenge = ((self.ausgang[pos] as usize) << 16)
                    | ((self.ausgang[pos + 1] as usize) << 8)
                    | self.ausgang[pos + 2] as usize;
                v.push((self.ausgang[pos + 3], laenge));
                pos += 9 + laenge;
            }
            v
        }
    }

    impl Read for Draht {
        fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
            self.eingang.read(p)
        }
    }

    impl Write for Draht {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            self.ausgang.extend_from_slice(p);
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl super::Frames for Draht {
        fn set_frist(&mut self, _dauer: Option<std::time::Duration>) {}
    }

    /// added 02.10.2026 19:2x, R39. **SCHLIESST Nr. 39 (a)**, und zwar mit der
    /// Zahl aus der Wild statt mit einer Behauptung: der kleinste **zulaessige**
    /// Wert, den ein Server fuer `SETTINGS_MAX_FRAME_SIZE` nennen darf, ist der
    /// Anfangswert 2^14 = 16 384 (6.5.2: „The initial value is 2^14 (16,384)
    /// octets. The value advertised by an endpoint MUST be between this initial
    /// value and the maximum allowed frame size (2^24-1 …)“). Wir senden nach dem
    /// Vorlauf nur Quittung (0 Byte), PING-Quittung (8) und `WINDOW_UPDATE`
    /// (4) — alle drei sind **kleiner** als jeder zulaessige Wert, also kann
    /// unsere ausbleibende Durchsetzung (R36, Abschnitt 4) hier nichts verpassen.
    ///
    /// added 02.10.2026 19:2x: **GEMESSEN IN DER WILD** (02.10.2026 19:0x,
    /// `server_settings_messen`): github.com kuendigt 68 608, archlinux.org und
    /// news.ycombinator.com je 16 777 215 (= 2^24-1), google.com und
    /// wikipedia.com gar keinen Wert (also 16 384), www.mozilla.org ebenfalls
    /// nicht. Der kleinste **beobachtete** Wert ist damit genau der
    /// Anfangswert — was die Rechnung bestaetigt und nichts ueber die
    /// schlimmsten Faelle sagt.
    #[test]
    fn alles_nach_dem_vorlauf_bleibt_unter_jedem_zulaessigen_rahmenmass() {
        let mut eingang = Vec::new();
        // SETTINGS mit `MAX_FRAME_SIZE = 16_384`, also dem kleinsten zulaessigen.
        let mut settings = Vec::new();
        settings.extend_from_slice(&0x0005u16.to_be_bytes());
        settings.extend_from_slice(&16_384u32.to_be_bytes());
        eingang.extend_from_slice(
            &crate::h2::baue_rahmen(crate::h2::typen::SETTINGS, 0, 0, &settings)
                .unwrap_or_else(|_| Vec::new()),
        );
        // PING mit 8 Byte (6.7) — die Quittung darauf schreiben wir.
        eingang.extend_from_slice(
            &crate::h2::baue_rahmen(crate::h2::typen::PING, 0, 0, &[0u8; 8])
                .unwrap_or_else(|_| Vec::new()),
        );
        // 64 DATA-Raehmen, damit die Freigabe schuetzt (R38).
        let stueck = vec![b'x'; crate::h2::MAX_RAHMEN_BYTES];
        for _ in 0..64 {
            eingang.extend_from_slice(
                &crate::h2::baue_rahmen(crate::h2::typen::DATA, 0, 1, &stueck)
                    .unwrap_or_else(|_| Vec::new()),
            );
        }
        let mut leitung = Draht::neu(eingang);
        let mut tabelle = DynamischeTabelle::neu();
        let _ = super::lies_antwort_mit_phasen(&mut leitung, 1, &mut tabelle);
        let rahmen = leitung.alle();
        assert!(!rahmen.is_empty(), "es wurde gar nichts geschrieben");
        for (typ, laenge) in &rahmen {
            assert!(
                *laenge <= 16_384,
                "Rahmen 0x{typ:02x} hat {laenge} Byte — mehr als der kleinste \
                 zulaessige MAX_FRAME_SIZE"
            );
        }
        // added 02.10.2026 19:3x: **UND DIE ARTEN SIND BENANNT**, damit der
        // Test nicht auch dann gruen waere, wenn wir aus Versehen gar nichts
        // senden. Es sind genau drei Sorten: Quittung, PING-Quittung, Freigabe.
        let arten: Vec<u8> = rahmen.iter().map(|(t, _)| *t).collect();
        assert!(
            arten.contains(&crate::h2::typen::SETTINGS),
            "keine Quittung: {arten:?}"
        );
        assert!(
            arten.contains(&crate::h2::typen::PING),
            "keine PING-Quittung: {arten:?}"
        );
        assert!(
            arten.contains(&crate::h2::typen::WINDOW_UPDATE),
            "keine Freigabe: {arten:?}"
        );
    }

    /// added 02.10.2026 19:3x, R39. **SCHLIESST Nr. 39 (c) fuer das Fenster**,
    /// und zwar indem die **Wildmessung zur Bedingung** wird: unser Koerper geht
    /// in **einem** `DATA`-Rahmen und ist durch `MAX_RAHMEN_BYTES` begrenzt. Der
    /// kleinste von sechs echten Servern angekuendigte Wert fuer
    /// `SETTINGS_INITIAL_WINDOW_SIZE` war **16 384** (www.wikipedia.org), der
    /// kleinste ueberhaupt zulaessige ist der Protokollausgangswert 65 535
    /// (6.9.2). Damit ist unser Koerper **immer** kleiner als jedes Fenster, das
    /// uns begegnen kann — solange `MAX_RAHMEN_BYTES` nicht waechst.
    ///
    /// added 02.10.2026 19:3x: **UND DIE RESTBEDINGUNG, die ich offen benenne:**
    /// ein Server, der weniger als 16 384 ankuendigt (0 ist erlaubt, 6.9.2
    /// bespricht es ausdruecklich), wuerde von uns eine Ueberschreitung sehen.
    /// Das koennen wir nicht verhindern, ohne nach RFC 9113 3.4 auf die
    /// Server-SETTINGS zu **warten** — und das waere ein Rundlauf fuer jede
    /// Anfrage mit Koerper. Dieser Test ist also kein Beweis, sondern eine
    /// **Schranke**: er sagt, ab welcher Groesse der Fall theoretisch zuschlaegt.
    #[test]
    fn unser_koerper_bleibt_unter_dem_kleinsten_gemessenen_fenster() {
        let kleinstes_gemessenes = 16_384u32;
        assert!(
            crate::h2::MAX_RAHMEN_BYTES as u32 <= kleinstes_gemessenes,
            "unser Koerper passt nicht mehr in das kleinste gemessene Fenster \
             ({} > {kleinstes_gemessenes})",
            crate::h2::MAX_RAHMEN_BYTES
        );
        // added 02.10.2026 19:3x: **UND** es muss zur **eigenen** Ankuendigung
        // passen, sonst schickten wir einen Rahmen, den wir selbst verboten
        // haben. Zwei Zahlen, die nicht auseinanderlaufen duerfen.
        for (kennung, wert) in crate::h2::EIGENE_SETTINGS {
            if kennung == 0x05 {
                assert_eq!(wert as usize, crate::h2::MAX_RAHMEN_BYTES);
            }
        }
    }

    /// added 02.10.2026 19:3x, R39. **UND DIE STROMZAHL** — und diesmal wird die
    /// Behauptung **gemessen**, statt eine Konstante zu behaupten, die es
    /// vielleicht gar nicht gibt: `SETTINGS_MAX_CONCURRENT_STREAMS` wurde von
    /// allen sechs Servern angekuendigt (100 bei github/google/wikipedia, 128 bei
    /// archlinux und news.ycombinator.com), also ist die Grenze nie 0. Und wir
    /// oeffnen **genau einen** Strom je Anfrage, was der Test an den
    /// geschriebenen Rahmen abliest: ein `HEADERS` mit Strom-ID 1, sonst nichts.
    /// Damit ist Nr. 39 (c) fuer diese Kennung strukturell erfuellt.
    #[test]
    fn eine_anfrage_oeffnet_genau_einen_strom() -> Result<(), &'static str> {
        let anfrage = crate::h2client::Anfrage {
            methode: "GET".to_string(),
            schemata: "https".to_string(),
            host: "example.org".to_string(),
            pfad: "/".to_string(),
            felder: Vec::new(),
            koerper: Vec::new(),
        };
        let mut leitung = Draht::neu(Vec::new());
        super::schreibe_anfrage(&mut leitung, &anfrage, 1)
            .map_err(|_| "die Anfrage liess sich nicht schreiben")?;
        let kopfzeilen = leitung
            .alle()
            .into_iter()
            .filter(|(typ, _)| *typ == crate::h2::typen::HEADERS)
            .count();
        assert_eq!(
            kopfzeilen, 1,
            "eine Anfrage, {kopfzeilen} Kopfzeilen-Bloecke"
        );
        Ok(())
    }
}
