//! HTTP/2 im Server — Frames, Streams, Flusskontrolle, Angriffsschutz.
//!
//! added 01.10.2026, SERVER V2 (S7 aus `team/AUTONOM.txt`: "Frames,
//! HPACK-Dekoder mit Tabellengrenze (narsil-net/src/hpack.rs nur LESEN),
//! Streams, Flusskontrolle; Schutz gegen Rapid Reset (RST-Flut),
//! SETTINGS-/PING-Flut, CONTINUATION-Flut. Beweis: `curl -k --http2
//! https://127.0.0.1:PORT/` liefert 200").
//!
//! DER AUFBAU IN DREI STUECKEN, und jeder hat seinen Grund:
//!
//! 1. **Rahmen** ([`Kopf`], [`Typ`], [`rahmen`]) — 9 Byte Kopf plus
//!    Nutzlast. Reines Byte-Format, keine Logik, keine Zustaende.
//! 2. **Zustandsmaschine** ([`Maschine`]) — nimmt Bytes ueber
//!    [`Maschine::fuetter`], gibt [`Aktion`]en zurueck. Sie kennt
//!    **keinen** Socket, **keinen** Router und **keine** Datei. Genau
//!    darum kann der Fuzzer sie mitten in einem Fall fuettern (kein
//!    Faden, der haengen koennte) und ein Test sie mit einem Byte-Array
//!    pruefen.
//! 3. **Fahrdienst** ([`h2_bediene`]) — liest vom `BufRead`, ruft die
//!    Handler-Kette auf, schreibt die Antwort.
//!
//! DIE NORM, und sie wird nicht aus dem Gedaechtnis zitiert: die
//! Rahmentypen und ihre Flaggen stehen in RFC 9113 Abschnitt 6, die
//! Fenster in 6.9, die Fehlercodes in 7, die Pseudo-Header-Regeln in
//! 8.3. Wo eine Zahl nicht aus der Norm kommt, steht sie als Konstante
//! daneben — mit der Begruendung, warum der Wert so ist.
//!
//! WAS DIESER SERVER **NICHT** KANN, und das ist Absicht (lieber eine
//! ehrliche Grenze als ein halbes Protokoll):
//! * **kein Server-Push** — `ENABLE_PUSH = 0` in unseren SETTINGS, ein
//!   `PUSH_PROMISE` vom Client ist ein `PROTOCOL_ERROR` (RFC 9113 8.4).
//! * **keine Prioritaeten** — `PRIORITY` wird gelesen und ignoriert,
//!   wie RFC 9113 5.3 es erlaubt.
//! * **kein CONNECT** — es gibt hier keinen Tunnel (RFC 9113 8.5).
//! * **kein h2c-Upgrade** — der Client schickt nach der TLS-Sitzung
//!   direkt Prior Knowledge; ein Upgrade-Verfahren gibt es in HTTP/2
//!   nicht (RFC 9113 3.2).

use std::collections::VecDeque;
use std::fmt;

use crate::hpack::{Dekoder, HpackFehler, MAX_HEADER_BYTES, dekodiere};
use crate::http::{Method, Request, Response};

/// Die Client-Vorfahrt, die nach dem TLS-Handshake kommen muss.
///
/// RFC 9113 Abschnitt 3.4: 24 Byte, exakt. Sie wird **vor** jedem
/// Rahmen gelesen; wer sie weglässt, redet ab da an ein anderes
/// Protokoll (HTTP/1.1 ueber TLS, was der Server nicht spricht, wenn
/// der Client `h2` ausgehandelt hat).
pub const CLIENT_VORFAHRT: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Unsere SETTINGS, als erster Rahmen der Sitzung.
///
/// added 01.10.2026, SERVER V2. DER INHALT, und jede Zeile hat einen
/// Grund:
/// * `ENABLE_PUSH = 0` — wir machen keinen Push.
/// * `MAX_CONCURRENT_STREAMS = 100` — der Server verarbeitet Anfragen
///   **nacheinander** (ein Faden je Verbindung). 100 offene Streams sind
///   mehr, als ein Browser oeffnet, und begrenzen die Zahl der Stuecke,
///   die eine Verbindung binden.
/// * `INITIAL_WINDOW_SIZE = 1 MiB` — **kein** Vorgabewert (das waere
///   65.535), und der Grund ist gemessen: mit 65.535 blieb eine
///   152-KiB-Seite bei 65.536 Byte stehen, bis die Lesefrist ablief.
///   `nghttp2` (in `curl`) schickte in dieser Sitzung **kein**
///   WINDOW_UPDATE fuer den Strom, sondern nur eines fuer die
///   Verbindung — mit 1 MiB Fenster ist die Seite in einem Rutsch
///   durch (gemessen: `curl -k --http2` auf `/gross`, 152.064 Byte,
///   HTTP 200). Der Speicherpreis ist **null**: der Server puffert die
///   Antwort nicht im Fenster, sondern im `Response` des Handlers (oder
///   streamt die Datei). Wer den trotzdem klein haben will, setzt
///   `INITIAL_WINDOW_SIZE` kleiner — dann greift der WINDOW_UPDATE-Pfad,
///   der getestet ist (`grosse_antwort_wartet_auf_window_update`).
/// * `MAX_FRAME_SIZE = 16384` — der Vorgabewert. Wir schicken keine
///   groesseren DATA-Rahmen; das haelt den Puffer des klein.
pub const EIGENE_SETTINGS: [(u16, u32); 4] = [
    (0x0002, 0),       // ENABLE_PUSH = 0
    (0x0003, 100),     // MAX_CONCURRENT_STREAMS
    (0x0004, 1 << 20), // INITIAL_WINDOW_SIZE = 1 MiB
    (0x0005, 16384),   // MAX_FRAME_SIZE = 16384
];

/// M4: hoechstens so viele Byte in einem einzelnen Rahmen, den **wir**
/// annehmen.
///
/// DER WERT 65536: RFC 9113 6.5.2 erlaubt bis 2^24-1, aber nur, wenn
/// beide Seiten `SETTINGS_MAX_FRAME_SIZE` entsprechend gesetzt haben. Wir
/// bieten 16 KiB an und nehmen nichts groesseres an: 64 KiB ist der
/// Wert, den der eigene Client (`narsil-net`) ebenfalls begrenzt, und ein
/// 16-MiB-Rahmen waere eine Einladung, den Speicher des Servers mit
/// einer Zeile zu fuellen.
pub const MAX_RAHMEN_BYTES: usize = 64 * 1024;

/// M3: hoechstens so viele Byte fuer die Kopfzeilen EINES Streams
/// (HEADERS plus alle CONTINUATION zusammen).
///
/// DER WERT 16384: derselbe Wert wie im HTTP/1.1-Pfad
/// ([`crate::MAX_HEADER_BYTES`]). Zwei Wege, eine Grenze — ein Browser
/// schickt unter 4 KiB, alles darueber ist entweder ein Fehler oder ein
/// Angriff.
pub const MAX_KOPFZEILEN_BYTES: usize = MAX_HEADER_BYTES;

/// M3: hoechstens so viele CONTINUATION-Rahmen je Kopfblock.
///
/// DER WERT 4: RFC 9113 6.10 verbietet nur das Einreihen eines anderen
/// Rahmens zwischen HEADERS und CONTINUATION, nicht die Zahl. Vier
/// Teile zu je 16 KiB sind 64 KiB Kopfdaten — das ist eine Datei, keine
/// Anfrage. Wer mehr braucht, schickt mehr HEADERS.
pub const MAX_KONTINUATION: usize = 4;

/// M3: hoechstens so viele gleichzeitige Stroeme je Verbindung.
///
/// DER WERT 100: siehe [`EIGENE_SETTINGS`]. Serverseitig ist die Grenze
/// strenger als der Zaehler im Kopf, denn der Server haelt fuer jeden
/// offenen Stream Kopfzeilen und Koerper.
pub const MAX_STREAMS: usize = 100;

/// Hoechste Dateigroesse, die der h2-Weg in einen Puffer lesen kann
/// (added SERVER V11).
///
/// DER GRUND, warum es diese Zahl gibt: HTTP/2 kennt kein Streamen
/// einer Datei wie HTTP/1.1 — die DATA-Frames wollen den Koerper als
/// Puffer. Damit ist dieser Weg ein Weg, auf dem eine Datei im
/// Speicher landet, und ohne Grenze waere `GET /film.mkv` eine
/// Einladung, das Dateisystem in den RAM zu kopieren. 16 MiB ist
/// gross genug fuer jede Seite und kleiner als die 4-GiB-Sperre von
/// `bau.sh`.
///
/// Was passiert, wenn die Datei groesser ist: `datei_als_bytes`
/// liefert `Err`, der Aufrufer loggt und schickt 0 Byte bei einem
/// `content-length`, der nicht passt. Das ist ein Fehler, der als
/// Fehler sichtbar ist (im Protokoll: Status 200, Koerper 0, und der
/// Leser sieht die Differenz) — und nicht ein still gekuerzter
/// Koerper, der als vollstaendig aussaehe.
pub const MAX_DATEI_BYTES: usize = 16 * 1024 * 1024;

/// M3: hoechstens so viele RST_STREAM je Verbindung, **bevor** der
/// Server mit GOAWAY schliesst.
///
/// DER WERT 50, und die Zahl kommt aus dem Angriff, den sie abwehrt:
/// "Rapid Reset" (CVE-2023-44487) schickt Tausende HEADERS- und
/// RST_STREAM-Paare. Jedes Paar kostet den Server sonst eine volle
/// Anfrageverarbeitung, die niemand sieht. 50 Abbrueche sind fuer einen
/// echten Client reichlich (der bricht ab, wenn der Nutzer wegclickt).
pub const MAX_RST: usize = 50;

/// M3: hoechstens so viele SETTINGS-Rahmen je Verbindung.
///
/// DER WERT 20: ein Browser schickt 1 bis 2. Jedes SETTINGS ist ein
/// Durchgang durch die Logik (beim Tabellenwert eine Kuerzung), und 20
/// pro Verbindung reichen fuer jede echte Verbindung, auch wenn sie
/// in mehreren Fenstern kommt.
pub const MAX_SETTINGS: usize = 20;

/// M3: hoechstens so viele PING-Rahmen je Verbindung.
///
/// DER WERT 50: RFC 9113 6.7 — ein PING **muss** mit ACK beantwortet
/// werden, kostet also je eine Antwort. 50 reichen fuer jede
/// Zeitmessung; mehr ist ein Flood.
pub const MAX_PING: usize = 50;

/// M3: hoechstens so viele Rahmen je Verbindung, die **ohne**
/// Anfragebezug kommen (SETTINGS, PING, WINDOW_UPDATE, PRIORITY).
///
/// DER WERT 10.000: die Decke ueber den Einzelfallgrenzen
/// (SETTINGS 20, PING 50). Sie faengt den Fall, den die anderen nicht
/// fangen: 10.000 WINDOW_UPDATE mit je 4 Byte, jeder einzeln erlaubt.
pub const MAX_STEUERRAHMEN: usize = 10_000;

/// M3: hoechstens so viele Rahmen einer Art, insgesamt, je Verbindung.
///
/// DER WERT 100.000: die Gesamtzahl aller Rahmen. Ohne sie koennte ein
/// Client 10.000 PINGs, 20 SETTINGS, 10.000 WINDOW_UPDATEs und dann
/// 80.000 PRIORITY-Rahmen schicken — jeder einzelne erlaubt, die
/// Gesamtzahl nicht begrenzt.
pub const MAX_RAHMEN: usize = 100_000;

/// M3: hoechstens so viele DATA-Rahmen je Stream, die **ohne**
/// END_STREAM kommen.
///
/// DER WERT 1.000: bei einer grossen Datei schickt ein Browser einige
/// Dutzend. 1.000 Datenrahmen ohne Ende sind ein Client, der die
/// Verbindung offen haelt, ohne etwas zu sagen.
pub const MAX_DATENRAHMEN: usize = 1000;

/// M3/M4: hoechstens so viele Byte Koerper je Stream.
///
/// DER WERT [`crate::MAX_BODY_BYTES`] (10 MiB): dieselbe Grenze wie im
/// HTTP/1.1-Pfad. Wer mehr braucht, streamt.
pub const MAX_KOERPER: usize = crate::MAX_BODY_BYTES;

/// M3: hoechstens so viele offene Aktionen in der Maschine.
///
/// DER WERT 256: die Schlange haelt fertige Anfragen, bis der Handler
/// sie abgearbeitet hat. 256 ist mehr, als ein Browser auf einmal
/// offen haelt (`MAX_STREAMS` = 100) — die Zahl ist also eine **echte**
/// Grenze und keine Kosmetik: ohne sie koennte ein Client 7.000 fertige
/// Anfragen in einem TCP-Segment schicken, und der Server haette alle
/// gleichzeitig im Speicher.
pub const MAX_WARTESCHLANGE: usize = 256;

/// M3: hoechstens so viele Byte in einer CONTINUATION-Kette (alle
/// Teile zusammen, ungeprueft der Inhalt).
///
/// DER WERT [`MAX_KOPFZEILEN_BYTES`] * 4: die Kette darf nicht mehr
/// Bytes sammeln, als ein Stream laut [`MAX_KOPFZEILEN_BYTES`] haben
/// darf. Diese Grenze greift **vor** der HPACK-Dekodierung — ein Client,
/// der 1 MiB Kettendaten schickt, laeuft hier gegen und nicht erst im
/// Dekoder.
pub const MAX_KETTE_BYTES: usize = MAX_KOPFZEILEN_BYTES * (MAX_KONTINUATION + 1);

/// M3: hoechstens so viele Runden im Fahrdienst, in denen **nichts**
/// geschrieben wurde.
///
/// DER WERT 10.000: eine Verbindung, die nur WINDOW_UPDATE und PING
/// schickt, wuerde den Faden sonst fuer immer halten. Jede Runde, in
/// der der Server antwortet, zaehlt nicht — die Grenze gilt fuer
/// Warteschleifen ohne Arbeit.
pub const MAX_LEERE_RUNDEN: u64 = 10_000;

/// Fehlercodes nach RFC 9113 Abschnitt 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FehlerCode {
    /// 0x1
    Protokoll,
    /// 0x2
    Intern,
    /// 0x3 FLOW_CONTROL_ERROR
    Fluss,
    /// 0x4 SETTINGS_TIMEOUT
    Einstellungen,
    /// 0x5 STREAM_CLOSED
    StromZu,
    /// 0x6 FRAME_SIZE_ERROR
    RahmenGroesse,
    /// 0x7 REFUSED_STREAM
    Abgelehnt,
    /// 0x8 CANCEL
    Abgebrochen,
    /// 0x9 COMPRESSION_ERROR
    Kompression,
    /// 0xa CONNECT_ERROR
    Verbinden,
    /// 0xb ENHANCE_YOUR_CALM
    Ruhe,
    /// 0xc INADEQUATE_SECURITY
    Sicherheit,
    /// 0xd HTTP_1_1_REQUIRED
    Http11,
}

impl FehlerCode {
    /// Der Zahlenwert im GOAWAY- bzw. RST_STREAM-Rahmen.
    pub fn zahl(self) -> u32 {
        match self {
            FehlerCode::Protokoll => 0x1,
            FehlerCode::Intern => 0x2,
            FehlerCode::Fluss => 0x3,
            FehlerCode::Einstellungen => 0x4,
            FehlerCode::StromZu => 0x5,
            FehlerCode::RahmenGroesse => 0x6,
            FehlerCode::Abgelehnt => 0x7,
            FehlerCode::Abgebrochen => 0x8,
            FehlerCode::Kompression => 0x9,
            FehlerCode::Verbinden => 0xa,
            FehlerCode::Ruhe => 0xb,
            FehlerCode::Sicherheit => 0xc,
            FehlerCode::Http11 => 0xd,
        }
    }

    /// Der Klartextname (im Log und im Test).
    pub fn name(self) -> &'static str {
        match self {
            FehlerCode::Protokoll => "PROTOCOL_ERROR",
            FehlerCode::Intern => "INTERNAL_ERROR",
            FehlerCode::Fluss => "FLOW_CONTROL_ERROR",
            FehlerCode::Einstellungen => "SETTINGS_TIMEOUT",
            FehlerCode::StromZu => "STREAM_CLOSED",
            FehlerCode::RahmenGroesse => "FRAME_SIZE_ERROR",
            FehlerCode::Abgelehnt => "REFUSED_STREAM",
            FehlerCode::Abgebrochen => "CANCEL",
            FehlerCode::Kompression => "COMPRESSION_ERROR",
            FehlerCode::Verbinden => "CONNECT_ERROR",
            FehlerCode::Ruhe => "ENHANCE_YOUR_CALM",
            FehlerCode::Sicherheit => "INADEQUATE_SECURITY",
            FehlerCode::Http11 => "HTTP_1_1_REQUIRED",
        }
    }
}

impl fmt::Display for FehlerCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Fehler der HTTP/2-Sitzung.
///
/// Die Unterscheidung ist die der Norm: `Strom` beendet **nur** den
/// genannten Stream (RST_STREAM), alles andere beendet die Verbindung
/// (GOAWAY). Der Fahrdienst entscheidet danach, welcher Code in den
/// Rahmen wandert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H2Fehler {
    /// Die Client-Vorfahrt fehlt oder stimmt nicht. Wer dafuer keine
    /// TLS-Sitzung hat, redet HTTP/1.1 ueber TLS.
    Vorfahrt,
    /// Der Puffer endet mitten in einem Rahmen (kein Fehler, sondern
    /// "warte auf mehr") — wird von `fuetter` **nicht** als Fehler
    /// geliefert, sondern ueber den Rueckgabewert gemeldet.
    RahmenKaputt { bei: usize },
    /// Ein Rahmentyp mit falscher Laenge (z. B. PING mit 7 Byte).
    LaengeFalsch { typ: u8, war: usize },
    /// Ein Rahmen oder ein Kopfdatenblock ist groesser als erlaubt.
    ///
    /// added 01.10.2026, SERVER V2: eigener Fehler, weil der Code
    /// **FRAME_SIZE_ERROR** sein muss (RFC 9113 6.5.2) und nicht
    /// PROTOCOL_ERROR — die Unterscheidung ist der Unterschied zwischen
    /// "dein Rahmen ist zu gross" und "du hast die Norm gebrochen".
    ZuGross { war: usize, erlaubt: usize },
    /// Ein Stream passt nicht (Nachricht auf dem falschen Strom,
    /// geschlossen, gerade oder zu klein).
    Strom { nummer: u32 },
    /// Die Norm selbst wurde verletzt, mit Begründung.
    Norm(String),
    /// Zu viele gleichzeitige Stroeme.
    ZuVieleStroeme { war: usize },
    /// Zu viele Datenrahmen ohne END_STREAM.
    ZuVieleDaten { war: usize },
    /// RST-Flut (Rapid Reset).
    RapidReset { war: u32 },
    /// SETTINGS-Flut.
    SettingsFlut { war: usize },
    /// PING-Flut.
    PingFlut { war: usize },
    /// Flut von Rahmen ohne Anfragebezug.
    SteuerFlut { war: usize },
    /// Zu viele Rahmen insgesamt.
    RahmenFlut { war: usize },
    /// Die Warteschlange der Maschine ist voll.
    WarteschlangeVoll { war: usize },
    /// CONTINUATION-Flut.
    KontinuationFlut { war: usize },
    /// Die Kopfdaten einer Kette sind zu gross.
    KetteZuGross { war: usize },
    /// Der Koerper eines Streams ist zu gross.
    KoerperZuGross { war: usize },
    /// Der HPACK-Dekoder hat abgewiesen.
    Hpack(HpackFehler),
    /// Das Fenster wurde ueberzogen (Inkrement 0 oder > 2^31-1).
    FlussUeberzogen { war: u32 },
    /// Der Client will Server-Push.
    Push,
    /// CONNECT ist hier nicht unterstuetzt.
    Connect,
    /// Die Sitzung endet mit GOAWAY und diesem Code.
    GehtZu(FehlerCode),
}

impl H2Fehler {
    /// Der Fehlercode, der in GOAWAY oder RST_STREAM wandert.
    ///
    /// added 01.10.2026, SERVER V2. DIE ZUORDNUNG IST EINE TABELLE
    /// und keine Vermutung: RFC 9113 7 sagt, welcher Fehler in welche
    /// Kategorie gehoert. Wer `RapidReset` als `PROTOCOL_ERROR`
    /// meldet, verschleiert genau das, was passiert ist.
    pub fn code(&self) -> FehlerCode {
        match self {
            H2Fehler::Vorfahrt => FehlerCode::Http11,
            H2Fehler::RahmenKaputt { .. } | H2Fehler::LaengeFalsch { .. } => {
                FehlerCode::RahmenGroesse
            }
            H2Fehler::FlussUeberzogen { .. } => FehlerCode::Fluss,
            H2Fehler::Hpack(_) => FehlerCode::Kompression,
            H2Fehler::Connect => FehlerCode::Verbinden,
            H2Fehler::Push => FehlerCode::Protokoll,
            H2Fehler::KetteZuGross { .. }
            | H2Fehler::KoerperZuGross { .. }
            | H2Fehler::ZuGross { .. } => FehlerCode::RahmenGroesse,
            H2Fehler::RahmenFlut { .. }
            | H2Fehler::SteuerFlut { .. }
            | H2Fehler::SettingsFlut { .. }
            | H2Fehler::PingFlut { .. }
            | H2Fehler::RapidReset { .. }
            | H2Fehler::WarteschlangeVoll { .. } => FehlerCode::Ruhe,
            H2Fehler::GehtZu(c) => *c,
            _ => FehlerCode::Protokoll,
        }
    }
}

impl fmt::Display for H2Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            H2Fehler::Vorfahrt => write!(f, "die Client-Vorfahrt fehlt oder stimmt nicht"),
            H2Fehler::RahmenKaputt { bei } => {
                write!(f, "der Rahmen bei Byte {bei} ist unvollstaendig")
            }
            H2Fehler::LaengeFalsch { typ, war } => {
                write!(
                    f,
                    "der Rahmentyp {typ:#x} hat {war} Byte Nutzlast, das ist falsch"
                )
            }
            H2Fehler::ZuGross { war, erlaubt } => {
                write!(f, "{war} Byte sind zu viel, erlaubt sind {erlaubt}")
            }
            H2Fehler::Strom { nummer } => write!(f, "der Stream {nummer} laesst das nicht zu"),
            H2Fehler::Norm(was) => write!(f, "Verstoss gegen die Norm: {was}"),
            H2Fehler::ZuVieleStroeme { war } => {
                write!(f, "{war} gleichzeitige Stroeme, erlaubt sind {MAX_STREAMS}")
            }
            H2Fehler::ZuVieleDaten { war } => {
                write!(
                    f,
                    "{war} Datenrahmen ohne Ende, erlaubt sind {MAX_DATENRAHMEN}"
                )
            }
            H2Fehler::RapidReset { war } => {
                write!(f, "{war} RST_STREAM, erlaubt sind {MAX_RST} (Rapid Reset)")
            }
            H2Fehler::SettingsFlut { war } => {
                write!(f, "{war} SETTINGS-Rahmen, erlaubt sind {MAX_SETTINGS}")
            }
            H2Fehler::PingFlut { war } => {
                write!(f, "{war} PING-Rahmen, erlaubt sind {MAX_PING}")
            }
            H2Fehler::SteuerFlut { war } => {
                write!(
                    f,
                    "{war} Rahmen ohne Anfrage, erlaubt sind {MAX_STEUERRAHMEN}"
                )
            }
            H2Fehler::RahmenFlut { war } => {
                write!(f, "{war} Rahmen, erlaubt sind {MAX_RAHMEN}")
            }
            H2Fehler::WarteschlangeVoll { war } => {
                write!(f, "{war} offene Aktionen, erlaubt sind {MAX_WARTESCHLANGE}")
            }
            H2Fehler::KontinuationFlut { war } => {
                write!(f, "{war} CONTINUATION, erlaubt sind {MAX_KONTINUATION}")
            }
            H2Fehler::KetteZuGross { war } => {
                write!(
                    f,
                    "die Kette hat {war} Byte, erlaubt sind {MAX_KETTE_BYTES}"
                )
            }
            H2Fehler::KoerperZuGross { war } => {
                write!(f, "der Koerper hat {war} Byte, erlaubt sind {MAX_KOERPER}")
            }
            H2Fehler::Hpack(e) => write!(f, "HPACK: {e}"),
            H2Fehler::FlussUeberzogen { war } => {
                write!(f, "das Fenster-Update {war} ist ungueltig")
            }
            H2Fehler::Push => write!(f, "der Client will Server-Push — machen wir nicht"),
            H2Fehler::Connect => write!(f, "CONNECT ist hier nicht unterstuetzt"),
            H2Fehler::GehtZu(c) => write!(f, "die Sitzung endet mit GOAWAY {c}"),
        }
    }
}

impl From<HpackFehler> for H2Fehler {
    fn from(e: HpackFehler) -> Self {
        H2Fehler::Hpack(e)
    }
}

// ---------------------------------------------------------------------------
// Rahmen
// ---------------------------------------------------------------------------

/// Ein Rahmentyp nach RFC 9113 Abschnitt 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Typ {
    /// 0x0 DATA
    Data = 0x0,
    /// 0x1 HEADERS
    Headers = 0x1,
    /// 0x2 PRIORITY (gelesen, ignoriert)
    Priority = 0x2,
    /// 0x3 RST_STREAM
    RstStream = 0x3,
    /// 0x4 SETTINGS
    Settings = 0x4,
    /// 0x5 PUSH_PROMISE (nur als Ablehnung)
    PushPromise = 0x5,
    /// 0x6 PING
    Ping = 0x6,
    /// 0x7 GOAWAY
    Goaway = 0x7,
    /// 0x8 WINDOW_UPDATE
    WindowUpdate = 0x8,
    /// 0x9 CONTINUATION
    Continuation = 0x9,
}

impl Typ {
    /// Der Typ aus seinem Byte, sonst `None`.
    ///
    /// UNBEKANNTE TYPEN SIND ERLAUBT: RFC 9113 4.1 sagt, dass ein
    /// Endpunkt einen Rahmen **muss** ignorieren, dessen Typ er nicht
    /// kennt. Der Aufrufer entscheidet, ob er das tut.
    pub fn aus_byte(b: u8) -> Option<Typ> {
        Some(match b {
            0x0 => Typ::Data,
            0x1 => Typ::Headers,
            0x2 => Typ::Priority,
            0x3 => Typ::RstStream,
            0x4 => Typ::Settings,
            0x5 => Typ::PushPromise,
            0x6 => Typ::Ping,
            0x7 => Typ::Goaway,
            0x8 => Typ::WindowUpdate,
            0x9 => Typ::Continuation,
            _ => return None,
        })
    }

    /// Ob der Typ **nur** auf einem Stream (Strom ungleich 0) erlaubt
    /// ist (RFC 9113 5.1).
    pub fn nur_auf_strom(self) -> bool {
        matches!(
            self,
            Typ::Data | Typ::Headers | Typ::Priority | Typ::RstStream
        )
    }

    /// Ob der Typ auf einem Stream (Strom ungleich 0) erlaubt ist —
    /// auch wenn er **nur** dort erlaubt ist.
    ///
    /// added 01.10.2026, SERVER V2. DER GRUND FUER DEN ZWEITEN WEG:
    /// CONTINUATION und PUSH_PROMISE werden auf einem Stream
    /// geschickt, sind aber keine "nur-auf-Strom"-Raehmen — die erste
    /// Fassung hat CONTINUATION darum wie einen Verbindungsrahmen
    /// behandelt und **jede** zweite HEADERS-Folge abgewiesen (der
    /// Test `kontinuations_kette_wird_zusammengesetzt` schlug fehl).
    pub fn darf_auf_strom(self) -> bool {
        matches!(
            self,
            Typ::Data
                | Typ::Headers
                | Typ::Priority
                | Typ::RstStream
                | Typ::PushPromise
                | Typ::Continuation
                // WINDOW_UPDATE gehoert dazu (6.9): ein Client, der
                // nur 1 KiB Fenster fuer einen Strom freigibt, schickt
                // genau so einen Rahmen. Ohne ihn in dieser Liste
                // wurde **jedes** Stream-Fenster-Update als
                // Verbindungsrahmen mit Streamnummer abgewiesen — und
                // damit jede Antwort groesser als 65 KiB unmoeglich.
                | Typ::WindowUpdate
        )
    }
}

/// Der 9-Byte-Kopf eines Rahmens (RFC 9113 4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kopf {
    /// Nutzlastlaenge in Byte (24 Bit).
    pub laenge: u32,
    /// Rahmentyp.
    pub typ: Typ,
    /// Flaggen (jeder Typ liest seine eigenen).
    pub flaggen: u8,
    /// Stream-Kennung; das Hochbit ist reserviert und immer 0.
    pub strom: u32,
}

impl Kopf {
    /// Liest die 9 Kopfbyte. Der Typ muss bekannt sein.
    ///
    /// M4: hier wird **nichts** alloziert; die Laenge wird nur gelesen.
    /// Die Grenze ([`MAX_RAHMEN_BYTES`]) prueft der Aufrufer, weil er
    /// sie mit seinen SETTINGS verrechnen muss.
    pub fn lesen(b: &[u8]) -> Result<Kopf, H2Fehler> {
        if b.len() < 9 {
            return Err(H2Fehler::RahmenKaputt { bei: b.len() });
        }
        let typ = Typ::aus_byte(b[3]).ok_or(H2Fehler::LaengeFalsch { typ: b[3], war: 0 })?;
        Ok(Kopf {
            laenge: ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32),
            typ,
            flaggen: b[4],
            strom: u32::from_be_bytes([b[5] & 0x7f, b[6], b[7], b[8]]),
        })
    }

    /// Schreibt die 9 Kopfbyte in `raus`.
    pub fn schreiben(&self, raus: &mut Vec<u8>) {
        raus.push((self.laenge >> 16) as u8);
        raus.push((self.laenge >> 8) as u8);
        raus.push(self.laenge as u8);
        raus.push(self.typ as u8);
        raus.push(self.flaggen);
        raus.extend_from_slice(&(self.strom & 0x7fff_ffff).to_be_bytes());
    }
}

/// END_STREAM an einem HEADERS- oder DATA-Rahmen (RFC 9113 6.1/6.2).
pub const FLAG_END_STREAM: u8 = 0x1;
/// END_HEADERS: letzter Teil des Kopfdatenblocks.
pub const FLAG_END_HEADERS: u8 = 0x4;
/// PADDED: die Nutzlast beginnt mit einem Pad-Laenge-Oktett.
pub const FLAG_PADDED: u8 = 0x8;
/// PRIORITY: die Nutzlast beginnt mit 5 Byte Prioritaet.
pub const FLAG_PRIORITY: u8 = 0x20;
/// ACK bei SETTINGS und PING.
pub const FLAG_ACK: u8 = 0x1;

/// Baut einen Rahmen (feste Groesse, M4: die Laenge kommt von hier und
/// nicht aus der Eingabe).
pub fn rahmen(typ: Typ, flaggen: u8, strom: u32, nutzlast: &[u8]) -> Vec<u8> {
    let mut raus = Vec::with_capacity(9 + nutzlast.len());
    Kopf {
        laenge: nutzlast.len() as u32,
        typ,
        flaggen,
        strom,
    }
    .schreiben(&mut raus);
    raus.extend_from_slice(nutzlast);
    raus
}

// ---------------------------------------------------------------------------
// Aktionen
// ---------------------------------------------------------------------------

/// Was die Maschine als Naechstes tun will.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aktion {
    /// Roh auf die Leitung schreiben (SETTINGS, PING-ACK, RST_STREAM).
    Bytes(Vec<u8>),
    /// Eine vollstaendige Anfrage fuer den Handler.
    Anfrage {
        /// Stream-Kennung.
        strom: u32,
        /// Zusammenfassung der Pseudo-Header.
        kopf: AnfrageKopf,
        /// Koerper (schon begrenzt gelesen).
        koerper: Vec<u8>,
    },
    /// Der Client hat GOAWAY geschickt: die Sitzung endet sauber.
    FertigSitzung,
}

/// Die Pseudo-Header und Kopfdaten einer Anfrage, dekodiert.
///
/// added 01.10.2026, SERVER V2. WARUM KEIN [`Request`] DIREKT: die
/// Maschine kennt den Router nicht. Ausserdem ist die Pruefung der
/// Pseudo-Header (Reihenfolge, Pflicht, Grossbuchstaben) eine
/// **Normsache** und gehoert nebenan, nicht in den HTTP/1.1-Typ.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnfrageKopf {
    /// `:method`, z. B. `GET`.
    pub methode: String,
    /// `:path`, z. B. `/bilder/a.png?x=1`.
    pub pfad: String,
    /// `:authority` (der Host). Leer, wenn keiner kam.
    pub host: String,
    /// `:scheme`, z. B. `https`.
    pub schema: String,
    /// Die uebrigen Kopfzeilen, Namen kleingeschrieben.
    pub kopfzeilen: Vec<(String, String)>,
    /// Der Client hat mit HEADERS schon END_STREAM gesetzt (dann gibt
    /// es keinen DATA-Teil).
    pub nur_kopf: bool,
}

impl AnfrageKopf {
    /// Der Pfad ohne Query, wie der HTTP/1.1-Parser ihn liefert.
    pub fn pfad_ohne_query(&self) -> &str {
        match self.pfad.split_once('?') {
            Some((p, _)) => p,
            None => &self.pfad,
        }
    }

    /// Der Query ohne `?`, wie der HTTP/1.1-Parser ihn liefert.
    pub fn query(&self) -> &str {
        match self.pfad.split_once('?') {
            Some((_, q)) => q,
            None => "",
        }
    }

    /// Baut daraus einen [`Request`] fuer den Handler — derselbe Typ,
    /// den der HTTP/1.1-Pfad liefert, damit Router, Middleware und
    /// Handler nichts vom Protokoll wissen muessen.
    pub fn als_request(&self, peer: &str, koerper: Vec<u8>) -> Request {
        let mut headers =
            std::collections::HashMap::<String, String>::with_capacity(self.kopfzeilen.len() + 1);
        for (n, w) in &self.kopfzeilen {
            // Doppelte Kopfzeilen mit Komma verbinden, wie im
            // HTTP/1.1-Pfad.
            match headers.get_mut(n) {
                Some(alt) => {
                    if !alt.is_empty() {
                        alt.push_str(", ");
                    }
                    alt.push_str(w);
                }
                None => {
                    headers.insert(n.clone(), w.clone());
                }
            }
        }
        if !self.host.is_empty() {
            headers
                .entry("host".to_string())
                .or_insert_with(|| self.host.clone());
        }
        Request {
            methode: Method::aus_str(&self.methode),
            methode_roh: self.methode.clone(),
            pfad: self.pfad_ohne_query().to_string(),
            query: self.query().to_string(),
            // `will_schliessen()` gibt bei allem, was nicht mit
            // `HTTP/1.1` beginnt, `true` zurueck — im HTTP/2-Pfad
            // entscheidet aber die **Sitzung** ueber die Verbindung,
            // nicht die Versionsangabe. Deshalb steht hier `HTTP/1.1`
            // und nicht `HTTP/2.0`: sonst wuerde jeder Handler, der
            // `will_schliessen` aufruft, "Connection: close"
            // verlangen, und das gibt es in HTTP/2 nicht.
            version: "HTTP/1.1".to_string(),
            headers,
            body: koerper,
            params: std::collections::HashMap::new(),
            peer: peer.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Die Maschine
// ---------------------------------------------------------------------------

/// Der Zustand eines noch offenen Streams.
#[derive(Debug, Clone)]
struct Strom {
    /// Der gesammelte Kopfdatenblock (HEADERS plus CONTINUATION).
    kopf: Vec<u8>,
    /// Wie viele CONTINUATION bisher kamen.
    kontinuationen: usize,
    /// Wie viele Byte die Kette bisher hat (ohne Dekodierung).
    kettengroesse: usize,
    /// Der Koerper, begrenzt auf [`MAX_KOERPER`].
    koerper: Vec<u8>,
    /// Wie viele DATA-Rahmen **ohne** END_STREAM kamen.
    datenrahmen: usize,
    /// Ist der Kopfdatenblock vollstaendig (END_HEADERS war gesetzt)?
    ///
    /// added 01.10.2026, SERVER V2. DER GRUND FUER DAS ZWEITE
    /// MERKMAL: `abgeschickt` allein reicht nicht. Ein HEADERS mit
    /// END_HEADERS, aber **ohne** END_STREAM ist noch keine Anfrage —
    /// der DATA-Teil kommt danach. Ohne dieses Merkmal wurde die
    /// Anfrage schon nach den Kopfzeilen an den Handler gegeben, der
    /// erste DATA-Rahmen kam als "Daten nach der Antwort" zurueck, und
    /// **jeder** POST ueber h2 schlug fehl.
    kopf_fertig: bool,
    /// Ist die Anfrage schon an den Handler gegeben?
    abgeschickt: bool,
    /// Hat der Client END_STREAM geschickt?
    ende_vom_client: bool,
    /// Unser Fenster auf diesem Stream (`None` = noch keins gesetzt,
    /// dann gilt `INITIAL_WINDOW_SIZE`).
    fenster: Option<i64>,
}

/// Die HTTP/2-Sitzung als reine Zustandsmaschine.
///
/// added 01.10.2026, SERVER V2. **KEIN SOCKET, KEIN ROUTER.** Die
/// Maschine nimmt Bytes ueber [`Maschine::fuetter`] und gibt
/// [`Aktion`]en zurueck. Genau darum kann der Fuzzer sie mitten in
/// einem Fall fuettern (kein Faden, der haengen koennte) und ein Test
/// sie mit einem Byte-Array pruefen.
///
/// DAS FENSTER ALS `i64`, und der Grund ist ein Fehler, den ein `u32`
/// nicht zeigen kann: ein Fenster darf **negativ** werden, wenn der
/// Client zu viel schickt oder `INITIAL_WINDOW_SIZE` senkt. Mit `u32`
/// wuerde aus "4096 Byte zu viel" ein Sprung auf 4 Milliarden — also
/// wuerde der Server einem Client, der zu viel schickt, noch mehr
/// erlauben.
#[derive(Debug)]
pub struct Maschine {
    dekodierer: Dekoder,
    /// Fertige Aktionen, in Reihenfolge.
    warteschlange: VecDeque<Aktion>,
    /// Offene Stroeme. Die Zahl ist durch [`MAX_STREAMS`] begrenzt,
    /// die *Reihenfolge* nicht (ein alter Stream kann mit WINDOW_UPDATE
    /// aufleben) — deshalb wird linear gesucht.
    stroeme: Vec<(u32, Strom)>,
    /// Der Stream, dessen Kette (HEADERS ohne END_HEADERS) offen ist.
    /// Nach RFC 9113 6.2 darf das **nur einer** sein.
    offene_kette: Option<u32>,
    /// Die Nummer des letzten Client-Streams; alle weiteren muessen
    /// groesser sein (RFC 9113 5.1.1).
    letzter_client_strom: u32,
    /// Die Settings des Clients, Index = Setting-Nummer.
    client_settings: [u32; 16],
    /// Unser Fenster auf der **Verbindung**.
    fenster_verbindung: i64,
    /// Rahmen dieser Art gesamt (Grenzen).
    settings_gesamt: usize,
    ping_gesamt: usize,
    rst_gesamt: u32,
    steuer_gesamt: usize,
    rahmen_gesamt: usize,
    /// Hat der Client die Vorfahrt geschickt?
    vorfahrt: bool,
    /// Soll jeder gelesene Rahmen protokolliert werden?
    ///
    /// added 01.10.2026, SERVER V2, NUR FUER DIE FEHLERSUCHE. DER WEG:
    /// `NARSIL_H2_TRACE=1` in der Umgebung. Begruendet, weil der erste
    /// Beweis (`curl --http2` auf eine 136-KiB-Seite) bei 65.536 Byte
    /// stehen blieb und **nur** am Mitschnitt erkennbar war, ob der
    /// Client ein WINDOW_UPDATE schickt oder der Server es nicht liest.
    /// Aus ist der Schalter per Voreinstellung; die Ausgabe geht auf
    /// stderr, damit sie die Antwort auf der Leitung nicht verfaelscht.
    trace: bool,
    /// Die hoechste Stromnummer, die wir **geschlossen** haben.
    ///
    /// added 01.10.2026, SERVER V2. DER GRUND: RFC 9113 5.1 erlaubt
    /// nach dem END_STREAM des Clients noch WINDOW_UPDATE, PRIORITY und
    /// RST_STREAM auf diesem Strom — `curl` schickt genau das
    /// (WINDOW_UPDATE, um das Fenster freizugeben), und der Server hat
    /// den Strom da schon geschlossen. Ohne diese Merkmal war jede
    /// **zweite** Anfrage auf einer Verbindung ein
    /// `PROTOCOL_ERROR`: der erste `curl`-Aufruf lieferte 200, der
    /// zweite 000.
    geschlossen_bis: u32,
}

impl Maschine {
    /// Eine neue Sitzung mit den Vorgabe-Einstellungen (RFC 9113 6.5.2).
    pub fn neu() -> Maschine {
        Maschine {
            dekodierer: Dekoder::neu(),
            warteschlange: VecDeque::new(),
            stroeme: Vec::new(),
            offene_kette: None,
            letzter_client_strom: 0,
            client_settings: [
                0,     // 0x0 unbenutzt
                4096,  // 0x1 HEADER_TABLE_SIZE
                0,     // 0x2 ENABLE_PUSH
                0,     // 0x3 MAX_CONCURRENT_STREAMS
                65535, // 0x4 INITIAL_WINDOW_SIZE
                16384, // 0x5 MAX_FRAME_SIZE
                0,     // 0x6 MAX_HEADER_LIST_SIZE
                0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            fenster_verbindung: 65535,
            settings_gesamt: 0,
            ping_gesamt: 0,
            rst_gesamt: 0,
            steuer_gesamt: 0,
            rahmen_gesamt: 0,
            vorfahrt: false,
            trace: std::env::var_os("NARSIL_H2_TRACE").is_some(),
            geschlossen_bis: 0,
        }
    }

    /// Fuettert Bytes und sagt, wie viele davon **verbraucht** sind.
    ///
    /// DER WEG, und warum der Verbrauch zurueckkommt: ein angekommener
    /// Puffer kann mitten in einem Rahmen enden. Der Aufrufer darf nur
    /// die verbrauchten Bytes wegnehmen — der Rest muss fuer den
    /// naechsten Rahmen stehen bleiben. Ein `read`, das alles frisst und
    /// den Rest wegwirft, verliert bei jedem TCP-Segment einen halben
    /// Rahmen.
    pub fn fuetter(&mut self, daten: &[u8]) -> Result<usize, H2Fehler> {
        let mut pos = 0usize;
        if !self.vorfahrt {
            if daten.len() < CLIENT_VORFAHRT.len() {
                return Ok(0); // warten
            }
            if &daten[..CLIENT_VORFAHRT.len()] != CLIENT_VORFAHRT {
                return Err(H2Fehler::Vorfahrt);
            }
            self.vorfahrt = true;
            pos = CLIENT_VORFAHRT.len();
            // Unsere SETTINGS zuerst (RFC 9113 3.4.1).
            self.warteschlange
                .push_back(Aktion::Bytes(self.settings_rahmen()));
        }
        while pos + 9 <= daten.len() {
            let kopf = match Kopf::lesen(&daten[pos..]) {
                Ok(k) => k,
                Err(_) => return Err(H2Fehler::RahmenKaputt { bei: pos }),
            };
            // M4: die Laenge kommt aus der Eingabe. **Vor** dem Lesen
            // geprueft, und geprueft wird gegen unsere Grenze und gegen
            // das, was wirklich da ist.
            if kopf.laenge as usize > MAX_RAHMEN_BYTES {
                return Err(H2Fehler::ZuGross {
                    war: kopf.laenge as usize,
                    erlaubt: MAX_RAHMEN_BYTES,
                });
            }
            if daten.len() - pos - 9 < kopf.laenge as usize {
                return Ok(pos); // halber Rahmen: mehr erwarten
            }
            let nutzlast = &daten[pos + 9..pos + 9 + kopf.laenge as usize];
            self.rahmen_lesen(kopf, nutzlast)?;
            pos += 9 + kopf.laenge as usize;
        }
        Ok(pos)
    }

    /// Die naechste Aktion, oder `None`.
    pub fn naechste(&mut self) -> Option<Aktion> {
        self.warteschlange.pop_front()
    }

    /// Wie viele Aktionen noch anliegen (Tests und die Schleife).
    pub fn wartend(&self) -> usize {
        self.warteschlange.len()
    }

    /// Läuft der Mitschnitt (`NARSIL_H2_TRACE=1`)?
    pub fn trace(&self) -> bool {
        self.trace
    }

    /// Ob die Vorfahrt gelesen wurde.
    pub fn hat_vorfahrt(&self) -> bool {
        self.vorfahrt
    }

    /// Wie viele RST_STREAM diese Sitzung gesehen hat — der Wert, an dem
    /// der Rapid-Reset-Schutz ausloest.
    pub fn rst_gesamt(&self) -> u32 {
        self.rst_gesamt
    }

    /// Wie viele SETTINGS-Rahmen diese Sitzung gesehen hat.
    pub fn settings_gesamt(&self) -> usize {
        self.settings_gesamt
    }

    /// Wie viele PING-Rahmen diese Sitzung gesehen hat.
    pub fn ping_gesamt(&self) -> usize {
        self.ping_gesamt
    }

    /// Wie viele Stroeme gerade offen sind.
    pub fn offene_stroeme(&self) -> usize {
        self.stroeme.len()
    }

    /// Unser Fenster auf der Verbindung.
    pub fn fenster_verbindung(&self) -> i64 {
        self.fenster_verbindung
    }

    /// Unser Fenster auf einem Stream, oder `None` wenn er nicht offen
    /// ist.
    pub fn fenster_strom(&self, strom: u32) -> Option<i64> {
        self.strom_index(strom).map(|i| {
            self.stroeme[i]
                .1
                .fenster
                .unwrap_or(self.client_settings[4] as i64)
        })
    }

    /// Die Nummer des letzten Client-Streams (in GOAWAY).
    pub fn letzter_strom(&self) -> u32 {
        self.letzter_client_strom
    }

    /// Unsere SETTINGS als Rahmen.
    pub fn settings_rahmen(&self) -> Vec<u8> {
        let mut nutzlast = Vec::with_capacity(EIGENE_SETTINGS.len() * 6);
        for (nummer, wert) in EIGENE_SETTINGS {
            nutzlast.extend_from_slice(&nummer.to_be_bytes());
            nutzlast.extend_from_slice(&wert.to_be_bytes());
        }
        rahmen(Typ::Settings, 0, 0, &nutzlast)
    }

    /// Unser GOAWAY.
    pub fn goaway(&self, code: FehlerCode) -> Vec<u8> {
        let mut nutzlast = Vec::with_capacity(8);
        nutzlast.extend_from_slice(&self.letzter_client_strom.to_be_bytes());
        nutzlast.extend_from_slice(&code.zahl().to_be_bytes());
        rahmen(Typ::Goaway, 0, 0, &nutzlast)
    }

    /// Unser RST_STREAM fuer einen Stream.
    pub fn rst_stream(&self, strom: u32, code: FehlerCode) -> Vec<u8> {
        rahmen(Typ::RstStream, 0, strom, &code.zahl().to_be_bytes())
    }

    /// Unser WINDOW_UPDATE (Verbindung: `strom = 0`).
    pub fn window_update(&self, inkrement: u32, strom: u32) -> Vec<u8> {
        rahmen(Typ::WindowUpdate, 0, strom, &inkrement.to_be_bytes())
    }

    /// Baut die **Kopfzeilen** der Antwort: HEADERS plus noetigenfalls
    /// CONTINUATION, in Stuecken von `MAX_FRAME_SIZE` des Clients.
    ///
    /// added 01.10.2026, SERVER V2. DIE DREI ENTSCHEIDUNGEN:
    /// * `:status` wird **statisch indiziert**, wenn es in der Tabelle
    ///   steht (200 = Index 8), sonst als Literal. Das ist ein Byte
    ///   statt zehn und nutzt die Norm, wie sie gedacht ist.
    /// * Alle anderen Kopfzeilen gehen als Literal **ohne
    ///   Indexierung** raus (RFC 7541 6.2.2). Der Server fuehrt
    ///   bewusst **keine** dynamische Tabelle: sie waere ein zweiter
    ///   Zustand, der zwischen zwei Anfragen auseinanderlaufen kann
    ///   (und dann ist die Antwort des naechsten Requests kaputt).
    /// * Der Block wird auf `MAX_FRAME_SIZE` des Clients geteilt, wie
    ///   RFC 9113 6.5.2 verlangt.
    pub fn kopf_bytes(
        &mut self,
        strom: u32,
        status: u16,
        kopfzeilen: &[(String, String)],
        ende_stream: bool,
    ) -> Vec<u8> {
        let mut block = Vec::with_capacity(64 + kopfzeilen.len() * 24);
        kodiere_status(status, &mut block);
        for (name, wert) in kopfzeilen {
            kodiere_literal(name, wert, &mut block);
        }
        let stueck = (self.client_settings[5] as usize).clamp(1, MAX_RAHMEN_BYTES);
        let mut raus = Vec::with_capacity(9 + block.len());
        let mut rest = &block[..];
        let mut erster = true;
        loop {
            let n = rest.len().min(stueck);
            let mut flaggen = if n == rest.len() { FLAG_END_HEADERS } else { 0 };
            if erster && ende_stream {
                flaggen |= FLAG_END_STREAM;
            }
            raus.extend_from_slice(&rahmen(
                if erster {
                    Typ::Headers
                } else {
                    Typ::Continuation
                },
                flaggen,
                strom,
                &rest[..n],
            ));
            rest = &rest[n..];
            if rest.is_empty() {
                break;
            }
            erster = false;
        }
        raus
    }

    /// Baut so viel DATA, wie **beide** Fenster erlauben, und sagt, wie
    /// viele Byte aus `nutzlast` dabei verbraucht wurden.
    ///
    /// added 01.10.2026, SERVER V2. DIE BEIDEN FENSTER, und warum beide:
    /// RFC 9113 6.9 gibt der Verbindung **und** jedem Stream ein
    /// Fenster. Wer nur eines beachtet, schickt mehr, als der Client
    /// annimmt — und der Client verarbeitet das als
    /// `FLOW_CONTROL_ERROR` (7.1: "The receiver... MUST treat... as a
    /// connection error of type FLOW_CONTROL_ERROR").
    ///
    /// DER RUECKGABEWERT, und warum er nicht `Option` ist: 0 verbrauchte
    /// Byte ist ein **Fall**, kein Fehler — dann wartet der Server auf
    /// WINDOW_UPDATE. Der Aufrufer fragt danach das Fenster ab.
    pub fn daten_bytes(
        &mut self,
        strom: u32,
        nutzlast: &[u8],
        ende_stream: bool,
    ) -> (Vec<u8>, usize) {
        let stueck = (self.client_settings[5] as usize).clamp(1, MAX_RAHMEN_BYTES);
        let fenster_stream = self.fenster_strom(strom).unwrap_or(0);
        let moeglich = self
            .fenster_verbindung
            .min(fenster_stream)
            .max(0)
            .min(MAX_RAHMEN_BYTES as i64) as usize;
        let n = nutzlast.len().min(moeglich).min(stueck);
        if n == 0 {
            return (Vec::new(), 0);
        }
        let letzter = ende_stream && n == nutzlast.len();
        if self.trace {
            eprintln!(
                "h2 -> DATA strom={strom} len={n} ende={letzter} fenster danach={}",
                self.fenster_verbindung - n as i64
            );
        }
        let bytes = rahmen(
            Typ::Data,
            if letzter { FLAG_END_STREAM } else { 0 },
            strom,
            &nutzlast[..n],
        );
        self.fenster_verbindung -= n as i64;
        if let Some(i) = self.strom_index(strom) {
            let vorher = self.stroeme[i]
                .1
                .fenster
                .unwrap_or(self.client_settings[4] as i64);
            self.stroeme[i].1.fenster = Some(vorher - n as i64);
        }
        (bytes, n)
    }

    /// Schliesst einen Stream und gibt seinen Speicher frei.
    pub fn strom_schliessen(&mut self, strom: u32) {
        // DER MERKER, siehe `geschlossen_bis`: danach sind WINDOW_UPDATE,
        // PRIORITY und RST_STREAM auf diesem Strom erlaubt (5.1) und
        // werden nicht als Fehler behandelt.
        self.geschlossen_bis = self.geschlossen_bis.max(strom);
        if let Some(i) = self.strom_index(strom) {
            self.stroeme.remove(i);
        }
        if self.offene_kette == Some(strom) {
            self.offene_kette = None;
        }
    }

    // -- innen ------------------------------------------------------------

    /// Der Index des Streams, oder `None`.
    fn strom_index(&self, strom: u32) -> Option<usize> {
        self.stroeme.iter().position(|(n, _)| *n == strom)
    }

    /// Eine Aktion einreihen — mit Grenze.
    fn einreihen(&mut self, aktion: Aktion) -> Result<(), H2Fehler> {
        if self.warteschlange.len() >= MAX_WARTESCHLANGE {
            return Err(H2Fehler::WarteschlangeVoll {
                war: self.warteschlange.len() + 1,
            });
        }
        self.warteschlange.push_back(aktion);
        Ok(())
    }

    /// Einen gelesenen Rahmen verarbeiten.
    fn rahmen_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        if self.trace {
            eprintln!(
                "h2 <- typ={:?} strom={} len={} flags={:#x} fenster_verbindung={} fenster_strom={:?}",
                kopf.typ,
                kopf.strom,
                kopf.laenge,
                kopf.flaggen,
                self.fenster_verbindung,
                self.fenster_strom(kopf.strom)
            );
        }
        // M3: die Zaehler stehen **vor** jeder Auswertung, weil auch ein
        // ignorierter Rahmen (unbekannter Typ) zaehlt — sonst waere
        // "10.000 Priority-Rahmen" kein Angriff, sondern nur Rechenzeit.
        self.rahmen_gesamt += 1;
        if self.rahmen_gesamt > MAX_RAHMEN {
            return Err(H2Fehler::RahmenFlut {
                war: self.rahmen_gesamt,
            });
        }
        if !kopf.typ.nur_auf_strom() {
            self.steuer_gesamt = self.steuer_gesamt.saturating_add(1);
            if self.steuer_gesamt > MAX_STEUERRAHMEN {
                return Err(H2Fehler::SteuerFlut {
                    war: self.steuer_gesamt,
                });
            }
        }
        // Stream 0 gehoert der Verbindung, alles andere muss ungerade
        // sein (der Client oeffnet nur ungerade Nummern, RFC 9113 5.1.1).
        if kopf.strom % 2 == 0 && kopf.strom != 0 {
            return Err(H2Fehler::Strom { nummer: kopf.strom });
        }
        if kopf.typ.nur_auf_strom() && kopf.strom == 0 {
            return Err(H2Fehler::Strom { nummer: 0 });
        }
        if !kopf.typ.darf_auf_strom() && kopf.strom != 0 {
            return Err(H2Fehler::Strom { nummer: kopf.strom });
        }
        // AUF EINEM GESCHLOSSENEN STROM (RFC 9113 5.1): WINDOW_UPDATE,
        // PRIORITY und RST_STREAM sind dort noch erlaubt, DATA nicht.
        // `curl` schickt nach jeder Antwort ein WINDOW_UPDATE — ohne
        // diese Zeile waere die zweite Anfrage einer Verbindung ein
        // GOAWAY.
        let war_geschlossen = kopf.strom != 0
            && self.strom_index(kopf.strom).is_none()
            && kopf.strom <= self.geschlossen_bis;
        if war_geschlossen {
            match kopf.typ {
                Typ::WindowUpdate | Typ::Priority | Typ::RstStream => return Ok(()),
                _ => return Err(H2Fehler::Strom { nummer: kopf.strom }),
            }
        }
        match kopf.typ {
            Typ::Headers => self.headers_lesen(kopf, nutzlast),
            Typ::Continuation => self.continuation_lesen(kopf, nutzlast),
            Typ::Data => self.data_lesen(kopf, nutzlast),
            Typ::Settings => self.settings_lesen(kopf, nutzlast),
            Typ::Ping => self.ping_lesen(kopf, nutzlast),
            Typ::WindowUpdate => self.window_update_lesen(kopf, nutzlast),
            Typ::RstStream => self.rst_lesen(kopf, nutzlast),
            // RFC 9113 5.3: PRIORITY wird gelesen und ignoriert.
            Typ::Priority => Ok(()),
            // Der Client sagt, dass er aufhoert (RFC 9113 6.8). Das ist
            // **kein** Fehler: ein sauberes GOAWAY ist der normale Weg,
            // mit dem ein Browser eine Verbindung schliesst. Die erste
            // Fassung hat daraus ein GOAWAY PROTOCOL_ERROR **des
            // Servers** gemacht — jede Antwort war 200, und im Log stand
            // trotzdem "Sitzung endet mit GOAWAY PROTOCOL_ERROR".
            Typ::Goaway => {
                self.einreihen(Aktion::FertigSitzung)?;
                Ok(())
            }
            // RFC 9113 8.4: PUSH_PROMISE ist nur ein Server-Client.
            Typ::PushPromise => Err(H2Fehler::Push),
        }
    }

    /// HEADERS: Sammlung beginnen.
    fn headers_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        // RFC 9113 6.2: waehrend einer Kette darf kein neuer HEADERS
        // kommen.
        if self.offene_kette.is_some() {
            return Err(H2Fehler::Norm(
                "HEADERS waehrend einer CONTINUATION-Kette".into(),
            ));
        }
        if kopf.strom <= self.letzter_client_strom {
            return Err(H2Fehler::Strom { nummer: kopf.strom });
        }
        if self.stroeme.len() >= MAX_STREAMS {
            return Err(H2Fehler::ZuVieleStroeme {
                war: self.stroeme.len() + 1,
            });
        }
        self.letzter_client_strom = kopf.strom;
        // PADDED und PRIORITY abbauen (RFC 9113 6.2).
        let rest = kopf_nutzlast(nutzlast, kopf.flaggen, Typ::Headers)?;
        if rest.len() > MAX_KETTE_BYTES {
            return Err(H2Fehler::KetteZuGross { war: rest.len() });
        }
        let ende = kopf.flaggen & FLAG_END_STREAM != 0;
        let ende_headers = kopf.flaggen & FLAG_END_HEADERS != 0;
        self.stroeme.push((
            kopf.strom,
            Strom {
                kopf: Vec::with_capacity(rest.len().min(1024)),
                kontinuationen: 0,
                kettengroesse: rest.len(),
                koerper: Vec::new(),
                datenrahmen: 0,
                kopf_fertig: false,
                abgeschickt: false,
                ende_vom_client: ende,
                fenster: None,
            },
        ));
        let index = self.stroeme.len() - 1;
        self.stroeme[index].1.kopf.extend_from_slice(rest);
        if ende_headers {
            self.stroeme[index].1.kopf_fertig = true;
            if ende {
                // END_STREAM gesetzt: die Anfrage ist vollstaendig.
                self.kopf_abschliessen(kopf.strom)?;
            }
            // Sonst: der DATA-Teil kommt noch (siehe `Strom::kopf_fertig`).
        } else {
            self.offene_kette = Some(kopf.strom);
        }
        Ok(())
    }

    /// CONTINUATION: naechster Teil des Kopfdatenblocks.
    fn continuation_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        // Es MUSS eine offene Kette geben, und sie MUSS zu diesem Stream
        // gehoeren (RFC 9113 6.10).
        let strom = self
            .offene_kette
            .ok_or_else(|| H2Fehler::Norm("CONTINUATION ohne HEADERS".into()))?;
        if strom != kopf.strom {
            return Err(H2Fehler::Norm(format!(
                "CONTINUATION fuer Stream {} waehrend die Kette von {strom} offen ist",
                kopf.strom
            )));
        }
        // M3: Zahl der CONTINUATION und Groesse der Kette — **beide**,
        // weil 5 Teile zu 1 KiB und 2 Teile zu 60 KiB verschiedene
        // Angriffe sind.
        {
            let index = self
                .strom_index(strom)
                .ok_or(H2Fehler::Strom { nummer: strom })?;
            let eintrag = &mut self.stroeme[index].1;
            eintrag.kontinuationen += 1;
            if eintrag.kontinuationen > MAX_KONTINUATION {
                return Err(H2Fehler::KontinuationFlut {
                    war: eintrag.kontinuationen,
                });
            }
            eintrag.kettengroesse = eintrag.kettengroesse.saturating_add(nutzlast.len());
            if eintrag.kettengroesse > MAX_KETTE_BYTES {
                return Err(H2Fehler::KetteZuGross {
                    war: eintrag.kettengroesse,
                });
            }
            eintrag.kopf.extend_from_slice(nutzlast);
        }
        // DER ENDE STEHT AUCH HIER: END_STREAM darf auf der **letzten**
        // CONTINUATION stehen (RFC 9113 6.10), nicht nur auf dem ersten
        // HEADERS. Der erste Testlauf hat es ignoriert, und die Anfrage
        // aus der Kette kam nie beim Handler an.
        let ende = kopf.flaggen & FLAG_END_STREAM != 0;
        if kopf.flaggen & FLAG_END_HEADERS != 0 || ende {
            let index = self
                .strom_index(strom)
                .ok_or(H2Fehler::Strom { nummer: strom })?;
            if ende {
                self.stroeme[index].1.ende_vom_client = true;
            }
            if kopf.flaggen & FLAG_END_HEADERS != 0 {
                self.stroeme[index].1.kopf_fertig = true;
            }
            if self.stroeme[index].1.kopf_fertig && self.stroeme[index].1.ende_vom_client {
                self.kopf_abschliessen(strom)?;
            }
        }
        Ok(())
    }

    /// DATA: Koerperteile eines Streams.
    fn data_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        let rest = kopf_nutzlast(nutzlast, kopf.flaggen, Typ::Data)?;
        let ende = kopf.flaggen & FLAG_END_STREAM != 0;
        let index = self
            .strom_index(kopf.strom)
            .ok_or(H2Fehler::Strom { nummer: kopf.strom })?;
        {
            let strom = &mut self.stroeme[index].1;
            if strom.abgeschickt {
                // Daten nach unserer Antwort: STREAM_CLOSED
                // (RFC 9113 5.1 closed state).
                return Err(H2Fehler::Strom { nummer: kopf.strom });
            }
            if !ende {
                strom.datenrahmen += 1;
                if strom.datenrahmen > MAX_DATENRAHMEN {
                    return Err(H2Fehler::ZuVieleDaten {
                        war: strom.datenrahmen,
                    });
                }
            }
            // M4: der Koerper kommt aus der Eingabe — erst pruefen, dann
            // anhaengen. `saturating_sub` verhindert, dass die
            // Pruefung im Debug-Modus panickt.
            if rest.len() > MAX_KOERPER.saturating_sub(strom.koerper.len()) {
                return Err(H2Fehler::KoerperZuGross {
                    war: strom.koerper.len().saturating_add(rest.len()),
                });
            }
            strom.koerper.extend_from_slice(rest);
            if ende {
                strom.ende_vom_client = true;
            }
        }
        if ende {
            // Der Kopf muss vollstaendig sein, sonst waere die Anfrage
            // unvollstaendig (und ein Fehler, kein halbes Parsen).
            let index = self
                .strom_index(kopf.strom)
                .ok_or(H2Fehler::Strom { nummer: kopf.strom })?;
            if !self.stroeme[index].1.kopf_fertig {
                return Err(H2Fehler::Norm(
                    "DATA mit END_STREAM vor den Kopfzeilen".into(),
                ));
            }
            self.kopf_abschliessen(kopf.strom)?;
        }
        Ok(())
    }

    /// SETTINGS: die Einstellungen des Clients uebernehmen.
    fn settings_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        if kopf.flaggen & FLAG_ACK != 0 {
            // ACK auf unsere Settings: nichts zu tun. Mit Nutzlast ist
            // es ein Protokollfehler (RFC 9113 6.5).
            if !nutzlast.is_empty() {
                return Err(H2Fehler::Norm("SETTINGS-ACK mit Nutzlast".into()));
            }
            return Ok(());
        }
        self.settings_gesamt += 1;
        if self.settings_gesamt > MAX_SETTINGS {
            return Err(H2Fehler::SettingsFlut {
                war: self.settings_gesamt,
            });
        }
        if nutzlast.len() % 6 != 0 {
            return Err(H2Fehler::Norm(format!(
                "SETTINGS mit {} Byte Nutzlast (kein Vielfaches von 6)",
                nutzlast.len()
            )));
        }
        // M3: die Schleife laeuft ueber die Nutzlast, und deren Laenge
        // ist durch die Rahmengrenze (64 KiB) begrenzt: hoechstens
        // 10.922 Paare. Die Grenze ist damit die Rahmengrenze — sie
        // steht hier nur, damit das fuer den naechsten Leser da ist.
        let mut i = 0usize;
        while i < nutzlast.len() {
            let nummer = u16::from_be_bytes([nutzlast[i], nutzlast[i + 1]]);
            let wert = u32::from_be_bytes([
                nutzlast[i + 2],
                nutzlast[i + 3],
                nutzlast[i + 4],
                nutzlast[i + 5],
            ]);
            i += 6;
            if nummer == 0x2 && wert == 1 {
                // ENABLE_PUSH = 1 vom Client: er **darf** das verlangen
                // (RFC 9113 6.5.2 sagt ausdruecklich "can be used by the
                // client"), aber nur die Antwort des Server zaehlt —
                // und die ist "nein". Wir nehmen es nicht als Fehler,
                // sondern schicken unsere Settings erneut mit
                // ENABLE_PUSH = 0. Das ist die hoeflichere Form und sie
                // ist normkonform.
                self.einreihen(Aktion::Bytes(self.settings_rahmen()))?;
            }
            if (nummer as usize) < self.client_settings.len() {
                // INITIAL_WINDOW_SIZE aendert sich: nach RFC 9113 6.9.2
                // muss die **Differenz** auf alle offenen Fenster
                // wirken. Ohne das wuerde ein Client, der das Fenster
                // auf 0 senkt, trotzdem weiter Daten bekommen.
                if nummer == 0x4 {
                    self.fenster_verschieben(wert as i64 - self.client_settings[4] as i64);
                }
                self.client_settings[nummer as usize] = wert;
            }
        }
        // HEADER_TABLE_SIZE: der Wert, den der Client fuer **unsere**
        // Kodierung zulässt. Wir fuehren keine dynamische Tabelle, also
        // ist er fuer uns nur eine Obergrenze — und ein Wert darueber
        // ist nach RFC 9113 6.5.2 ein PROTOCOL_ERROR.
        let tabelle = self.client_settings[1] as usize;
        if tabelle > crate::hpack::MAX_DYNAMISCHE_TABELLE {
            return Err(H2Fehler::Norm(format!(
                "HEADER_TABLE_SIZE {tabelle} ist groesser als {}",
                crate::hpack::MAX_DYNAMISCHE_TABELLE
            )));
        }
        // ACK: die Norm verlangt die Antwort auf jedes SETTINGS.
        self.einreihen(Aktion::Bytes(rahmen(Typ::Settings, FLAG_ACK, 0, &[])))?;
        Ok(())
    }

    /// PING: mit ACK beantworten (RFC 9113 6.7).
    fn ping_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        if nutzlast.len() != 8 {
            return Err(H2Fehler::Norm(format!(
                "PING mit {} Byte Nutzlast (muss 8 sein)",
                nutzlast.len()
            )));
        }
        if kopf.flaggen & FLAG_ACK != 0 {
            return Ok(());
        }
        self.ping_gesamt += 1;
        if self.ping_gesamt > MAX_PING {
            return Err(H2Fehler::PingFlut {
                war: self.ping_gesamt,
            });
        }
        self.einreihen(Aktion::Bytes(rahmen(Typ::Ping, FLAG_ACK, 0, nutzlast)))?;
        Ok(())
    }

    /// WINDOW_UPDATE: Fenster vergroessern (RFC 9113 6.9).
    fn window_update_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        if nutzlast.len() != 4 {
            return Err(H2Fehler::Norm(format!(
                "WINDOW_UPDATE mit {} Byte Nutzlast (muss 4 sein)",
                nutzlast.len()
            )));
        }
        let inkrement = u32::from_be_bytes([nutzlast[0], nutzlast[1], nutzlast[2], nutzlast[3]]);
        // M4: Inkrement 0 und Inkrement > 2^31-1 sind nach
        // RFC 9113 6.9 beides PROTOCOL_ERROR. Ohne die zweite Grenze
        // wuerde ein einzelnes WINDOW_UPDATE das Fenster auf 2^32
        // heben — und damit jede Flusskontrolle abschalten.
        if inkrement == 0 || inkrement > 0x7fff_ffff {
            return Err(H2Fehler::FlussUeberzogen { war: inkrement });
        }
        if kopf.strom == 0 {
            self.fenster_verbindung = self
                .fenster_verbindung
                .saturating_add(inkrement as i64)
                .min(0x7fff_ffff);
            return Ok(());
        }
        let index = self
            .strom_index(kopf.strom)
            .ok_or(H2Fehler::Strom { nummer: kopf.strom })?;
        let vorher = self.stroeme[index]
            .1
            .fenster
            .unwrap_or(self.client_settings[4] as i64);
        self.stroeme[index].1.fenster =
            Some(vorher.saturating_add(inkrement as i64).min(0x7fff_ffff));
        Ok(())
    }

    /// RST_STREAM: der Rapid-Reset-Zaehler.
    fn rst_lesen(&mut self, kopf: Kopf, nutzlast: &[u8]) -> Result<(), H2Fehler> {
        if nutzlast.len() != 4 {
            return Err(H2Fehler::Norm(format!(
                "RST_STREAM mit {} Byte Nutzlast (muss 4 sein)",
                nutzlast.len()
            )));
        }
        // DER ANGRIFF, gegen den diese Zahl steht: "Rapid Reset"
        // (CVE-2023-44487). Der Client schickt HEADERS und
        // RST_STREAM in hoher Folge; jede HEADERS loest beim Server eine
        // volle Anfrageverarbeitung aus, das RST_STREAM macht sie
        // ungueltig. Der Server bezahlt zweimal und antwortet niemandem.
        //
        // DER ZAEHLER IST VERBINDUNGSWEIT, weil der Angriff viele
        // Streams nutzt — ein Zaehler je Stream waere blind. Und er
        // zaehlt auch RST_STREAM **nach** unserer Antwort: ein Client,
        // der 50 Antworten verwirft, ist kein normaler Browser.
        self.rst_gesamt = self.rst_gesamt.saturating_add(1);
        if self.rst_gesamt as usize > MAX_RST {
            return Err(H2Fehler::RapidReset {
                war: self.rst_gesamt,
            });
        }
        if let Some(i) = self.strom_index(kopf.strom) {
            self.stroeme.remove(i);
        }
        if self.offene_kette == Some(kopf.strom) {
            self.offene_kette = None;
        }
        Ok(())
    }

    /// Verschiebt alle offenen Fenster (und die Vorgabe) um `delta`.
    fn fenster_verschieben(&mut self, delta: i64) {
        for (_, strom) in self.stroeme.iter_mut() {
            let vorher = strom.fenster.unwrap_or(self.client_settings[4] as i64);
            strom.fenster = Some(vorher.saturating_add(delta));
        }
    }

    /// Der Kopfdatenblock ist vollstaendig: HPACK dekodieren, pruefen,
    /// an den Handler geben.
    fn kopf_abschliessen(&mut self, strom: u32) -> Result<(), H2Fehler> {
        let index = self
            .strom_index(strom)
            .ok_or(H2Fehler::Strom { nummer: strom })?;
        let block = std::mem::take(&mut self.stroeme[index].1.kopf);
        if self.offene_kette == Some(strom) {
            self.offene_kette = None;
        }
        // M4: die Blocklaenge kommt aus der Eingabe; geprueft wird hier
        // noch einmal (die Kette ist schon begrenzt, aber diese Grenze
        // ist die, die in RFC 9113 6.5.2 steht).
        if block.len() > MAX_KOPFZEILEN_BYTES {
            return Err(H2Fehler::ZuGross {
                war: block.len(),
                erlaubt: MAX_KOPFZEILEN_BYTES,
            });
        }
        let zeilen = dekodiere(&block, &mut self.dekodierer)?;
        let kopf = anfrage_kopf_aus(zeilen)?;
        // Der Koerper wird mitgeliefert, wenn END_STREAM schon da ist;
        // sonst kommt er spaeter mit DATA.
        let (nur_kopf, koerper) = {
            let s = &self.stroeme[index].1;
            (
                s.ende_vom_client,
                std::mem::take(&mut self.stroeme[index].1.koerper),
            )
        };
        self.stroeme[index].1.abgeschickt = true;
        let kopf = AnfrageKopf { nur_kopf, ..kopf };
        self.einreihen(Aktion::Anfrage {
            strom,
            kopf,
            koerper,
        })?;
        // Der Stream bleibt offen: solange die Antwort nicht ganz draussen
        // ist, braucht er sein Fenster fuer WINDOW_UPDATE. Der Fahrer
        // schliesst ihn mit `strom_schliessen`.
        Ok(())
    }
}

impl Default for Maschine {
    fn default() -> Self {
        Maschine::neu()
    }
}

/// Schneidet PADDED und PRIORITY von der Nutzlast ab.
///
/// added 01.10.2026, SERVER V2. DER WEG: RFC 9113 6.1/6.2 — bei PADDED
/// beginnt die Nutzlast mit einem Oktett Pad-Laenge, und die letzten
/// `pad` Oktett sind Fuellung. Wer das nicht abschneidet, gibt
/// Fuellbytes als Koerper weiter; wer die Pad-Laenge **nicht** prueft,
/// liest hinter das Ende des Rahmens (das ist der Fehler, aus dem
/// "Padding-Attacken" werden).
fn kopf_nutzlast(nutzlast: &[u8], flaggen: u8, typ: Typ) -> Result<&[u8], H2Fehler> {
    let mut rest = nutzlast;
    if flaggen & FLAG_PADDED != 0 {
        if rest.is_empty() {
            return Err(H2Fehler::Norm(format!(
                "{typ:?} mit PADDED, aber ohne Laengenoctett"
            )));
        }
        let pad = rest[0] as usize;
        rest = &rest[1..];
        if pad > rest.len() {
            return Err(H2Fehler::Norm(format!(
                "{typ:?}: Pad-Laenge {pad} ist groesser als die Nutzlast"
            )));
        }
        rest = &rest[..rest.len() - pad];
    }
    if flaggen & FLAG_PRIORITY != 0 {
        if rest.len() < 5 {
            return Err(H2Fehler::Norm(format!(
                "{typ:?} mit PRIORITY, aber weniger als 5 Byte"
            )));
        }
        rest = &rest[5..];
    }
    Ok(rest)
}

/// Prueft die Pseudo-Header und baut daraus den [`AnfrageKopf`].
///
/// added 01.10.2026, SERVER V2. DIE DREI NORMREGELN, alle aus
/// RFC 9113 8.3.1, und jede ist ein Angriff, wenn man sie auslaesst:
/// * **Reihenfolge** — alle Pseudo-Header vor den normalen. Wer sie
///   mischt, kann einen `:path` hinter einem `Cookie` verstecken.
/// * **Pflicht** — `:method`, `:path` und `:scheme`. Ein fehlender
///   `:path` ist kein "default /", sondern ein Fehler: der Server wuerde
///   sonst die Handler-Route `/` treffen.
/// * **Grossbuchstaben** — RFC 9113 8.1.2 verlangt kleingeschriebene
///   Feldnamen. `Host` statt `host` ist ein Fehler, kein Detail: die
///   Streams einer Verbindung duerfen sich nicht darin unterscheiden,
///   ob ein Proxy die Kopfzeile zusammenfasst hat oder nicht.
pub fn anfrage_kopf_aus(zeilen: Vec<(String, String)>) -> Result<AnfrageKopf, H2Fehler> {
    let mut kopf = AnfrageKopf::default();
    let mut normale_sehen = false;
    for (name, wert) in zeilen {
        if name.starts_with(':') {
            if normale_sehen {
                return Err(H2Fehler::Norm(format!(
                    "Pseudo-Header {name} steht nach normalen Kopfzeilen"
                )));
            }
            match name.as_str() {
                ":method" => kopf.methode = wert,
                ":path" => kopf.pfad = wert,
                ":authority" => kopf.host = wert,
                ":scheme" => kopf.schema = wert,
                // 8.3.1: unbekannte Pseudo-Header sind ein Fehler.
                _ => return Err(H2Fehler::Norm(format!("unbekannter {name}"))),
            }
        } else {
            normale_sehen = true;
            if name.bytes().any(|b| b.is_ascii_uppercase()) {
                return Err(H2Fehler::Norm(format!(
                    "Feldname {name} enthaelt Grossbuchstaben"
                )));
            }
            // 8.2.2: die verbotenen Kopfzeilen der Verbindung.
            if verbotene_kopfzeile(&name, &wert) {
                return Err(H2Fehler::Norm(format!("{name} ist in HTTP/2 verboten")));
            }
            if kopf.kopfzeilen.len() >= crate::hpack::MAX_HEADER_ANZAHL {
                return Err(H2Fehler::Norm(format!(
                    "mehr als {} Kopfzeilen",
                    crate::hpack::MAX_HEADER_ANZAHL
                )));
            }
            kopf.kopfzeilen.push((name, wert));
        }
    }
    if kopf.methode.is_empty() {
        return Err(H2Fehler::Norm(":method fehlt".into()));
    }
    if kopf.methode == "CONNECT" {
        return Err(H2Fehler::Connect);
    }
    if kopf.pfad.is_empty() {
        return Err(H2Fehler::Norm(":path fehlt".into()));
    }
    if kopf.schema.is_empty() {
        return Err(H2Fehler::Norm(":scheme fehlt".into()));
    }
    Ok(kopf)
}

/// Ist das eine in HTTP/2 verbotene Kopfzeile (RFC 9113 8.2.2)?
///
/// DER WERT GEHOERT ZUR FRAGE, und `te` ist der Fall: RFC 9113 8.2.2
/// sagt, dass `te` die **einzige** Ausnahme ist und "MUST NOT contain
/// any value other than \"trailers\"" enthalten darf. Ohne den Wert zu
/// sehen, waere `te: trailers` (was jeder Browser schickt) furchtbar.
fn verbotene_kopfzeile(name: &str, wert: &str) -> bool {
    if name == "te" {
        return !wert.eq_ignore_ascii_case("trailers");
    }
    matches!(
        name,
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
    )
}

// ---------------------------------------------------------------------------
// Der Kodierer (nur die Antwortseite)
// ---------------------------------------------------------------------------

/// Schreibt `:status` in den Block: statischer Index, wenn es einen
/// gibt, sonst Literal.
///
/// added 01.10.2026, SERVER V2. DIE INDIZES aus RFC 7541 Anhang A:
/// 8 = 200, 9 = 204, 10 = 206, 11 = 304, 12 = 400, 13 = 404, 14 = 500.
/// Das sind genau die, die ein Server wirklich schickt; alles andere
/// geht als Literal raus (4 Byte mehr, aber immer richtig).
fn kodiere_status(status: u16, block: &mut Vec<u8>) {
    let index = match status {
        200 => 8,
        204 => 9,
        206 => 10,
        304 => 11,
        400 => 12,
        404 => 13,
        500 => 14,
        _ => 0,
    };
    if index != 0 {
        // 6.1 Indexed Header Field: ein Byte.
        block.push(0x80 | index as u8);
    } else {
        // 6.2.2 Literal without indexing, neuer Name.
        block.push(0x00);
        kodiere_zahl(7, block);
        block.extend_from_slice(b":status");
        // DER WERT IST EIN **TEXT**, nicht die Zahl. Das war der
        // schwerste Fehler dieses Decoders und `nghttp2` hat ihn
        // gefunden: `curl --http2` antwortete auf jeden POST mit
        // "nghttp2 shuts down connection with error 9:
        // COMPRESSION_ERROR", waehrend alle GET-Antworten (200, 404 —
        // beide statisch indiziert) durchgingen. Der Beweis: der
        // Status 405 wurde als `ff 96 02` geschrieben, und das liest
        // jeder HPACK-Dekoder als **Stringlaenge 405** — es folgt also
        // eine 405-Byte-Nutzlast, die es nicht gibt.
        // Korrekt sind drei Byte: Laenge 3, dann "405".
        let text = status.to_string();
        kodiere_zahl(text.len() as u64, block);
        block.extend_from_slice(text.as_bytes());
    }
}

/// Schreibt `name: wert` als Literal **ohne** Indexierung.
///
/// added 01.10.2026, SERVER V2. WARUM OHNE HUFFMANN UND OHNE
/// DYNAMISCHE TABELLE: beides waere ein **zweiter** Zustand neben dem
/// des Clients. Der Server fuehrt keinen HPACK-Encoder-Zustand (keine
/// Tabelle, kein Groessen-Update), und das ist erlaubt — die
/// "Literal without indexing"-Form ist in RFC 7541 6.2.2 genau dafuer da.
/// Der Preis sind ein paar Byte mehr auf der Leitung; der Gewinn ist,
/// dass ein Fehler im Encoder nicht erst beim **naechsten** Request
/// sichtbar wird (als kaputter Header des Client).
fn kodiere_literal(name: &str, wert: &str, block: &mut Vec<u8>) {
    // 6.2.2 mit neuem Namen (Index 0 im 4-Bit-Praefix): 0x00.
    block.push(0x00);
    kodiere_zahl(name.len() as u64, block);
    block.extend_from_slice(name.as_bytes());
    kodiere_zahl(wert.len() as u64, block);
    block.extend_from_slice(wert.as_bytes());
}

/// Schreibt eine Zahl im HPACK-Format (RFC 7541 5.1).
///
/// M3: hoechstens 9 Oktette (2^63). Die Grenze haengt an
/// `hpack::lies_zahl`, damit Schreiben und Lesen dieselbe Grenze haben.
fn kodiere_zahl(wert: u64, raus: &mut Vec<u8>) {
    if wert < 127 {
        raus.push(wert as u8);
        return;
    }
    raus.push(0x80 | 127);
    let mut rest = wert - 127;
    while rest >= 128 {
        raus.push(0x80 | (rest % 128) as u8);
        rest /= 128;
    }
    raus.push(rest as u8);
}

// ---------------------------------------------------------------------------
// Der Fahrdienst
// ---------------------------------------------------------------------------

/// Eine Antwort, die noch nicht ganz draussen ist.
struct AntwortLaeuft {
    strom: u32,
    /// Der Rest des Koerpers.
    koerper: Vec<u8>,
    /// Wie viele Byte schon draussen sind.
    raus: usize,
}

/// Bedient EINE HTTP/2-Sitzung ueber einen beliebigen Strom.
///
/// added 01.10.2026, SERVER V2. DER AUFBAU:
///
/// ```text
/// lesen -> Maschine::fuetter -> Aktionen abarbeiten -> schreiben
///                     ^                                     |
///                     +--- WINDOW_UPDATE, PING, RST --------+/
/// ```
///
/// DER FALL, DER DIE SCHLEIFE BRAUCHT: ist die Antwort groesser als das
/// Fenster (65 KiB Vorgabe), bleibt ihr Rest in `AntwortLaeuft`, und
/// die Schleife macht mit den Steuerrahmen weiter. Ohne das wuerde der
/// Server auf WINDOW_UPDATE warten **und** dabei die Haelfte der
/// Verbindungen nicht bedienen.
///
/// DIE ZEITLICHEN GRENZEN kommen vom Aufrufer: der Sockel traegt
/// Lese- und Schreibfristen (siehe [`crate::tls::https_aufbauen`]). Eine
/// eigene Uhr gibt es hier nicht, weil der Faden sie nicht_shared hat.
pub fn h2_bediene<R: std::io::BufRead, W: std::io::Write>(
    server: &crate::Server,
    leser: &mut R,
    schreiber: &mut W,
    peer: &str,
) -> Result<u64, H2Fehler> {
    let mut maschine = Maschine::neu();
    let mut antworten = 0u64;
    let mut offene_antwort: Option<AntwortLaeuft> = None;
    let mut leere_runden = 0u64;
    // M4: der Lesefenster des `BufReader` ist fest (nicht aus der
    // Eingabe). Der Leser liefert, was der Sockel hat.
    loop {
        // 1) Anliegende Aktionen abarbeiten (Reihenfolge: SETTINGS und
        //    PING-ACK zuerst, sie liegen vorn in der Schlange).
        let mut geschrieben = false;
        while let Some(aktion) = maschine.naechste() {
            match aktion {
                Aktion::Bytes(b) => {
                    if schreiber.write_all(&b).is_err() {
                        return Ok(antworten);
                    }
                    geschrieben = true;
                }
                Aktion::Anfrage {
                    strom,
                    kopf,
                    koerper,
                } => {
                    // DER GLEICHE ROUTER WIE HTTP/1.1 — das ist der ganze
                    // Punkt der Verdrahtung: ein Handler weiss nicht,
                    // ueber welchem Protokoll er aufgerufen wurde.
                    let anfrage = kopf.als_request(peer, koerper);
                    let antwort = server.antwort_fuer(&anfrage);
                    let status = antwort.status.0;
                    let kopfzeilen = antwort_kopfzeilen(&antwort, anfrage.methode);
                    // HEAD: die Laenge steht im `content-length`, der
                    // Koerper wird **nicht** geschickt (RFC 9113 8.1).
                    //
                    // UND DIE DATEI, added 02.10.2026, SERVER V11. Die
                    // Fassung davor nahm `antwort.body` — und fuer
                    // eine statische Datei ist das **leer**, weil der
                    // Inhalt als offenes Handle in `antwort.datei`
                    // steht (M4: die Datei liegt nie vollstaendig im
                    // Speicher). Die Folge war: `GET /index.html` ueber
                    // h2c lieferte `200` mit
                    // `content-length: 1297` und **null** Byte. Der
                    // Browser zeigte eine leere Seite, und der Status
                    // war trotzdem richtig — die schlimmste Art von
                    // Fehler. Gefunden von
                    // `tests/v11_demo_seite.rs::demo_seite_antwortet_
                    // ueber_h2c_genauso`.
                    //
                    // Die Grenze ist [`MAX_DATEI_BYTES`]: 16 MiB. Der
                    // h2-Weg kann nicht streamen (die Frames wollen
                    // den Koerper als Puffer), also begrenzt er.
                    let koerper = if anfrage.methode == Method::Head {
                        Vec::new()
                    } else {
                        match &antwort.datei {
                            Some(d) => crate::http::datei_als_bytes(d, MAX_DATEI_BYTES)
                                .unwrap_or_else(|e| {
                                    eprintln!("h2: Datei nicht lesbar: {e}");
                                    Vec::new()
                                }),
                            None => antwort.body.clone(),
                        }
                    };
                    let leer = koerper.is_empty();
                    let bytes = maschine.kopf_bytes(strom, status, &kopfzeilen, leer);
                    if schreiber.write_all(&bytes).is_err() {
                        return Ok(antworten);
                    }
                    geschrieben = true;
                    antworten += 1;
                    if leer {
                        maschine.strom_schliessen(strom);
                    } else {
                        // DER REST WARTET AUF DAS FENSTER, und der
                        // Stream bleibt bis dahin **offen**: sein
                        // Fenster braucht der Server, um zu wissen, wie
                        // viel DATA noch raus darf. Wuerde er jetzt
                        // geschlossen, waere `fenster_strom` `None`, also
                        // 0 — und die Antwort kaeme nie zu Ende.
                        let mut offen = AntwortLaeuft {
                            strom,
                            koerper,
                            raus: 0,
                        };
                        if fenster_senden(&mut maschine, schreiber, &mut offen)? {
                            maschine.strom_schliessen(strom);
                        } else {
                            offene_antwort = Some(offen);
                        }
                    }
                }
                Aktion::FertigSitzung => return Ok(antworten),
            }
        }
        // 2) Eine wartende Antwort, soweit das Fenster reicht.
        if let Some(offen) = offene_antwort.as_mut() {
            if fenster_senden(&mut maschine, schreiber, offen)? {
                let strom = offen.strom;
                offene_antwort = None;
                // Erst jetzt ist der Stream wirklich fertig.
                maschine.strom_schliessen(strom);
                geschrieben = true;
            }
        }
        if geschrieben {
            leere_runden = 0;
        } else {
            leere_runden += 1;
            if leere_runden > MAX_LEERE_RUNDEN {
                return Err(H2Fehler::GehtZu(FehlerCode::Ruhe));
            }
        }
        // 3) Lesen. `fill_buf` nimmt nichts weg — der Rest bleibt fuer
        //    den naechsten Rahmen stehen.
        let puffer: &[u8] = match leser.fill_buf() {
            Ok(b) => b,
            // Lesefehler: der Client ist weg oder der Sockel kaputt.
            // Beides ist kein Protokollfehler — aber **was** es war,
            // gehoert in die Spur, sonst ist ein Fehler wie "Reentrant-
            // Lesen im TLS-Strom" (ein Fehler im Rechner, nicht im
            // Protokoll) von einem normalen Ende nicht zu unterscheiden.
            Err(e) => {
                if maschine.trace() {
                    eprintln!("h2: Lesen beendet: {e} (bei {antworten} Antworten)");
                }
                return Ok(antworten);
            }
        };
        if puffer.is_empty() {
            return Ok(antworten); // EOF
        }
        let verbraucht = match maschine.fuetter(puffer) {
            Ok(n) => n,
            Err(e) => {
                let code = e.code();
                let _ = schreiber.write_all(&maschine.goaway(code));
                return Err(e);
            }
        };
        leser.consume(verbraucht);
    }
}

/// Schickt so viel DATA, wie das Fenster zulaesst. `true`, wenn die
/// Antwort damit ganz draussen ist.
fn fenster_senden<W: std::io::Write>(
    maschine: &mut Maschine,
    schreiber: &mut W,
    offen: &mut AntwortLaeuft,
) -> Result<bool, H2Fehler> {
    while offen.raus < offen.koerper.len() {
        let rest = &offen.koerper[offen.raus..];
        let letzter = rest.len() <= 16 * 1024;
        let (bytes, genutzt) = maschine.daten_bytes(offen.strom, rest, letzter);
        if genutzt == 0 {
            // Fenster voll: auf WINDOW_UPDATE warten.
            return Ok(false);
        }
        // DER FEHLER MIT TEXT, und das ist der Unterschied zwischen
        // "irgendwas ging schief" und "EPIPE nach 65.536 Byte": der
        // Fahrer gibt die Meldung weiter, sie landet im Log des
        // Aufrufers und im Test.
        if let Err(e) = schreiber.write_all(&bytes) {
            return Err(H2Fehler::Norm(format!(
                "DATA auf Strom {} nicht schreibbar: {e}",
                offen.strom
            )));
        }
        offen.raus += genutzt;
    }
    Ok(true)
}

/// Die Kopfzeilen einer Antwort fuer HTTP/2.
///
/// added 01.10.2026, SERVER V2. WAS RAUS FAELLT, und warum:
/// * `connection` — in HTTP/2 verboten (RFC 9113 8.2.2). Ueber die
///   Verbindung entscheidet der Server, nicht ein Header.
/// * `transfer-encoding` — es gibt kein Chunked in HTTP/2; die
///   Laenge steht in `content-length` oder im Ende des DATA-Stroms.
/// * `content-length` und `content-type` bei koerperlosen Status
///   (1xx, 204, 304) und bei HEAD — dort ist die Laenge undefiniert,
///   und eine erfundene `0` ist eine Luege, die Clients anders
///   auslegen als wir.
pub fn antwort_kopfzeilen(antwort: &Response, methode: Method) -> Vec<(String, String)> {
    // DER ENDE MIT `HEAD`, und er ist **kein** koerperloser Status:
    // RFC 9110 9.3.2 sagt, dass eine HEAD-Antwort genau die Kopfzeilen
    // einer GET-Antwort traegt — also **mit** `content-length` der
    // vollen Ressource. Nur der Koerper fehlt, und den schickt der
    // Fahrer nicht. (Die erste Fassung hat `content-length` auch bei
    // HEAD weggelassen; das ist eine erfundene Angabe.)
    let koerperlos = antwort.status.0 == 204
        || antwort.status.0 == 304
        || (100..200).contains(&antwort.status.0);
    let _ = methode;
    let laenge: u64 = match &antwort.datei {
        Some(d) => d.laenge,
        None => antwort.body.len() as u64,
    };
    let mut raus: Vec<(String, String)> = Vec::with_capacity(antwort.headers.len() + 2);
    let mut hat_typ = false;
    let mut hat_laenge = false;
    for (name, wert) in &antwort.headers {
        let klein = name.to_ascii_lowercase();
        if klein == "connection" || klein == "transfer-encoding" {
            continue;
        }
        if klein == "content-length" {
            hat_laenge = true;
        }
        if klein == "content-type" {
            hat_typ = true;
        }
        raus.push((klein, wert.clone()));
    }
    if !koerperlos {
        if !hat_laenge {
            raus.push(("content-length".to_string(), laenge.to_string()));
        }
        if !hat_typ {
            raus.push((
                "content-type".to_string(),
                "application/octet-stream".to_string(),
            ));
        }
    }
    raus
}
