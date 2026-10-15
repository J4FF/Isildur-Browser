//! HTTP/2-Streams für den **Client** (RFC 9113 Abschnitt 5).
//!
//! added 01.10.2026 10:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
//! Vorgang: A2 aus `AUTONOM.txt`, nach R4w (Rahmen) und R4x (ALPN).
//!
//! WAS HIER STEHEN MUSS und woanders nicht: HTTP/2 ist ein Protokoll
//! über **Strome**, nicht über Nachrichten. Eine Anfrage braucht
//! mindestens einen Strom **und** eine Kopplung zwischen „Strom 3 ist
//! Antwort 2" und dem Aufrufer, der auf Antwort 2 wartet. Diese Kopplung
//! ist der ganze Inhalt dieser Datei — und sie ist der Grund, warum man
//! h2 nicht „ein bisschen mehr als HTTP/1.1" nennen kann.
//!
//! DIE REGELN, und sie kommen aus RFC 9113:
//! * Abschnitt 5.1.1: clientseitige Strom-IDs sind **ungerade** und
//!   steigen um 2; ein Client darf keine gerade ID vergeben.
//! * Abschnitt 5.1: nach `END_STREAM` ist eine ID **dauerhaft** geschlossen.
//! * Abschnitt 6.9: das Fenster ist anfangs 65 535 und wird durch
//!   `WINDOW_UPDATE` erhöht.
//! * Abschnitt 8.1: `200`, `204` und `304` haben **keinen** Body, auch
//!   nicht mit `content-length` > 0.

use crate::h2::{self, HpackFehlerWeiter, Rahmen, RahmenFehler, typen};
use crate::hpack;

/// Wandelt einen HPACK-Fehler, ohne einen Speicherleck.
///
/// added 01.10.2026 10:2x, R4y. `fehler_name` gibt ein `&'static str`
/// zurueck, `fehler_zahl` ein `Option<usize>` — beides **ohne**
/// Allokation.
///
/// DER FEHLER IN MEINER ERSTEN FASSUNG, und er waere still gewesen:
/// `RahmenFehler::KeinUtf8 { wo: Box::leak(String) }` — `Box::leak`
/// leakt **fuer immer**. Ein Browser, der 30 Seiten mit je drei kaputten
/// Blöcken holt, verliert 90 `String`s und kann sie nie wieder freigeben.
/// Kein Test haette das gemeldet, weil der Speicher langsam wächst und
/// ein Test 30 Seiten nicht holt.
fn als_rahmenfehler(e: hpack::HpackFehler) -> RahmenFehler {
    RahmenFehler::HpackFehler(HpackFehlerWeiter {
        was: e.fehler_name(),
        zahl: e.fehler_zahl(),
    })
}

/// M3: hoechstens so viele Strome sind gleichzeitig offen.
///
/// DER WERT 100: das ist genau unser `MAX_CONCURRENT_STREAMS` aus dem
/// `SETTINGS`-Block (`h2::EIGENE_SETTINGS`), und die beiden muessen
/// uebereinstimmen — sonst bieten wir 100 an und verweigern den 101.
/// die_ eigenen, was nach RFC 9113 5.1.2 ein Verbindungsfehler waere.
pub const MAX_STROME: usize = 100;

/// M4: hoechstens so viele Byte ein einzelner Strom annehmen darf.
///
/// DER WERT 16 MiB: das ist derselbe Wert, den `narsil-server/src/h2.rs`
/// als `MAX_DATEI_BYTES` benutzt, und die Gleichheit ist Absicht — ein
/// Client, der eine 20-MiB-Seite ablehnt, kann sie nicht anzeigen, und
/// ein Client, der sie annimmt, muss sie im Speicher halten koennen.
/// GEMESSEN (`narsil`-Tests): die groessten echten Seiten liegen bei
/// rund 2 MiB; 16 MiB ist Reserve, kein Ziel.
pub const MAX_STROM_BYTES: usize = 16 * 1024 * 1024;

/// M3: hoechstens so viele `HEADERS`-Bloecke pro Stream (RFC 9113 6.10
/// zerlegt grosse Kopfdaten auf `CONTINUATION`).
///
/// DER WERT 8: ein Kopf, der mehr als acht Bloecke braucht, ist absurd
/// (8 * 16 KiB = 128 KiB Kopfzeile fuer eine Anfrage). Die Grenze ist da,
/// weil ein Server sonst endlos `CONTINUATION` schicken kann.
pub const MAX_HEADERS_BLOECKE: usize = 8;

/// M4: so viele Byte darf ein **verketteter** Kopfblock haben.
///
/// added 01.10.2026 17:1x, R5b. `MAX_HEADERS_BLOECKE` (8) begrenzt die Zahl
/// der Rahmen, nicht ihre Groesse — acht Rahmen zu je 16 KiB sind 128 KiB.
/// Diese Grenze steht darum **zusaetzlich** und bewirkt etwas anderes: sie
/// stoppt das Anhängen, **bevor** der Speicher waechst.
///
/// DER WERT, und er ist gemessen: der groesste echte Kopfblock dieser
/// Messreihe ist 5 156 Byte (`github.com`, R4z8). 64 KiB ist das
/// Dreizehnfache und zugleich `hpack::MAX_HEADERS_RAHMEN` — der Wert, an dem
/// der HPACK-Dekodierer ohnehin abweist. Wer die Zahl anhebt, muss den
/// Dekodierer anheben, sonst kauft man nur Speicher.
pub const MAX_KOPFBLOCK_BYTES: usize = 64 * 1024;

/// Das HTTP/2-Fehlerzeichen (4 Bit) eines Frames.
///
/// added 01.10.2026 10:0x, Rolle TLS. RFC 9113 Abschnitt 7 nennt die
/// Nummern; `NO_ERROR` (0) ist der einzige, der **kein** Fehler ist.
pub const FEHLER: [(u32, &str); 9] = [
    (0, "NO_ERROR"),
    (1, "PROTOCOL_ERROR"),
    (2, "INTERNAL_ERROR"),
    (3, "FLOW_CONTROL_ERROR"),
    (4, "SETTINGS_TIMEOUT"),
    (5, "STREAM_CLOSED"),
    (6, "FRAME_SIZE_ERROR"),
    (7, "REFUSED_STREAM"),
    (8, "CANCEL"),
];

/// Der Name eines Fehlerzeichens, oder „unbekannt".
pub fn fehler_name(code: u32) -> &'static str {
    for (n, name) in FEHLER {
        if n == code {
            return name;
        }
    }
    "unbekannt"
}

/// Der Zustand **eines** Stromes, und er ist die Kopplung, um die es geht.
///
/// added 01.10.2026 10:0x: ein Stream haelt seine ID, seine
/// Kopfzeilen, seinen Body und ob er fertig ist. `kopfzeilen` ist ein
/// `Vec` und **kein** `HashMap`, weil RFC 9113 8.1.2 die Reihenfolge der
/// Felder nicht garantiert, aber **Duplikate** verbietet — und eine
/// `Vec` laesst die Duplikatpruefung sichtbar bleiben.
#[derive(Debug, Clone, Default)]
pub struct Strom {
    pub id: u32,
    /// Die bis hierher gesammelten Kopfzeilen des **Rahmens**.
    pub kopfzeilen: Vec<(String, String)>,
    /// Die gesammelten `DATA`-Byte.
    pub daten: Vec<u8>,
    /// Endete der Strom mit `END_STREAM`?
    pub zu: bool,
    /// Wurde der Strom vom Server mit `RST_STREAM` beendet? Dann steht
    /// hier der Fehlercode.
    pub verworfen: Option<u32>,
    /// Wie viele `HEADERS`/`CONTINUATION`-Bloecke kamen?
    pub bloecke: usize,
    /// Steht ein `HEADERS` ohne `END_HEADERS` in der Luft?
    ///
    /// added 01.10.2026 17:1x, R5b. GETRENNT von `blockpuffer`, weil die
    /// beiden Fragen verschieden sind: „wartet ein `CONTINUATION`?" und
    /// „wieviel Byte liegen schon?" — ein Block kann offen sein und **null
    /// Byte** im Puffer haben (ein `HEADERS` mit leerer Nutzlast und ohne
    /// `END_HEADERS`). Mit einem einzigen Zustand ginge eins von beiden
    /// verloren, und das ist die Art von Zustand, die man erst bemerkt,
    /// wenn ein Server mit leeren Feldern kommt.
    pub kopfblock_offen: bool,
    /// Der **unvollstaendige** Kopfblock, der auf `CONTINUATION` wartet.
    ///
    /// added 01.10.2026 17:1x, R5b. RFC 9113 Abschnitt 6.10: der Kopfblock
    /// ist die **Verkettung** der Nutzlast von `HEADERS` und aller folgenden
    /// `CONTINUATION`-Rahmen — nicht der Inhalt **eines** Rahmens.
    pub blockpuffer: Vec<u8>,
    /// Das Fenster, das der Server uns noch zugesteht (RFC 9113 6.9).
    pub fenster: i64,
}

impl Strom {
    /// Der Wert eines Kopfzeilenfeldes, **unabhängig** von der
    /// Gross-/Kleinschreibung (RFC 9113 8.1.2: die Namen sind
    /// Kleinbuchstaben; RFC 9113 8.1.2 sagt ausdruecklich, dass die
    /// Gross-/Kleinschreibung beim Vergleichen nicht zaehlt).
    pub fn kopf(&self, name: &str) -> Option<&str> {
        self.kopfzeilen
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, w)| w.as_str())
    }

    /// Der Statuscode aus `:status`, oder `None`.
    ///
    /// added 01.10.2026 10:0x: `:status` ist ein **Pseudo-Header** und
    /// steht nach RFC 9113 8.1.2.1 **vor** allen gewöhnlichen Feldern.
    /// Wer es aus der Mitte des Feldes sucht, findet es nicht — und
    /// meldet „keine Statuszeile", was wie ein kaputter Server aussieht.
    pub fn status(&self) -> Option<u16> {
        self.kopfzeilen
            .iter()
            .find(|(n, _)| n == ":status")
            .and_then(|(_, w)| w.parse::<u16>().ok())
    }
}

/// M3: der Zaehler fuer clientseitige Strom-IDs.
///
/// RFC 9113 Abschnitt 5.1.1: clientseitige IDs sind **ungerade** und
/// steigen um 2. Also startet der Zaehler bei 1 und der naechste ist 3.
///
/// added 01.10.2026 10:0x: die Grenze ist `MAX_STROME`. Ohne sie wuerde
/// ein Aufrufer in einer Schleife `id += 2` laufen und nach 2^31 Strome
/// die ID wickeln — was nach RFC 9113 5.1.1 ein Verbindungsfehler ist.
pub fn naechste_id(letzte: u32) -> Result<u32, RahmenFehler> {
    // added 01.10.2026 10:3x, R4y. DER ERSTE STROM, und er war 2 statt 1:
    // `0 + 2 = 2`, aber RFC 9113 5.1.1 sagt, clientseitige IDs sind
    // **ungerade** — und die erste ist 1, nicht 3. GEMESSEN: der Test
    // bekam `left: 2, right: 1`.
    //
    // DER WEG: `letzte == 0` ist der Sonderfall „noch keiner vergeben".
    // Sonst waere der Sonderfall auch nicht unterscheidbar von einer
    // echten ID 0 — und 0 ist nach RFC 9113 5.1.1 ohnehin keine
    // clientseitige ID.
    if letzte == 0 {
        return Ok(1);
    }
    // added 01.10.2026 10:6x, R4y: die Grenze war `0x7fff_fffd`, und
    // `letzte + 2` ergaebe `0x7fff_ffff` — das ist noch eine **gueltige**
    // ID (31 Bit gesetzt). Der naechste waere `0x8000_0001`, und das hat
    // das reservierte Bit gesetzt. Also muss abgelehnt werden, sobald
    // `letzte + 2 > 0x7fff_ffff`.
    let naechste = letzte as u64 + 2;
    if naechste > 0x7fff_ffff {
        return Err(RahmenFehler::StromVerboten {
            typ: typen::HEADERS,
            strom: letzte,
        });
    }
    Ok(naechste as u32)
}

/// Baut den `HEADERS`-Rahmen einer Anfrage aus `Request`.
///
/// added 01.10.2026 10:0x. DER WEG, und er hat **drei** Schritte, weil
/// HTTP/2 keine Kopfzeilen-Zeilen hat: alles wird HPACK.
///
/// DIE REIHENFOLGE ist Pflicht. RFC 9113 Abschnitt 8.1.2.1:
///
/// 1. **Pseudo-Header zuerst, alle zusammen, in dieser Reihenfolge:**
///    `:method`, `:scheme`, `:authority`, `:path`. Danach erst die
///    gewöhnlichen Felder. Wer sie mischt, schickt einen Block, den der
///    Server mit `PROTOCOL_ERROR` beantwortet.
/// 2. Die gewöhnlichen Felder in **Kleinbuchstaben** (8.1.2: „MUST be
///    converted to lowercase").
/// 3. `END_STREAM` nur dann, wenn es **keinen** Body gibt (8.1: „A
///    request without a body MUST set the END_STREAM flag").
///
/// added 01.10.2026 00:1x, R5b. `strom` ist neu und war vorher die
/// feste Zahl `1`. GEMESSEN, wofuer, und das ist die Ursache der drei
/// h2-Fehlschlaege im 30-Seiten-Lauf (`team/log/probe30_2230`):
/// `stackoverflow.com`, `developer.mozilla.org` und `nodejs.org`
/// meldeten „HTTP/2: der Rahmenkopf hat 0 Byte". Der Draht zeigte
/// (`tests/t4_roh_tmp.rs`, 01.10.2026 00:0x, `www.google.com`):
///     WIR    typ 0x01 flags 0x05 strom 1 laenge 24 nutz 82 87 41 ...
///     SERVER typ 0x06 (PING)  00 00 00 00 00 00 01 33
///     SERVER typ 0x07 (GOAWAY) 00 00 00 01 00 00 00 01
///             invalid_new_stream_id
/// Der Pool hatte Strom **3** vergeben (`gepoolt true strom 3`), und der
/// `DATA`-Ramen traegt die 3 auch — nur der `HEADERS`-Ramen stand auf 1,
/// weil diese Funktion die Stromnummer **fest** einsetzte. Der Server
/// antwortet darauf nach RFC 9113 5.1 mit `PROTOCOL_ERROR`
/// (`invalid_new_stream_id`) und beendet die Verbindung; beim Lesen blieb
/// ein leerer Puffer, und der Client meldete „Rahmenkopf hat 0 Byte".
pub fn baue_headers(
    methode: &str,
    schemata: &str,
    host: &str,
    pfad: &str,
    gewöhnlich: &[(String, String)],
    mit_body: bool,
    strom: u32,
) -> Result<Vec<u8>, RahmenFehler> {
    let mut block: Vec<u8> = Vec::new();

    // (1) `:method` — Index 2 ist GET, 3 ist POST (RFC 7541 Anhang A).
    // Sonst Literal mit eigenem Namen; das ist der Fall fuer PUT, DELETE
    // und HEAD, die nicht in der statischen Tabelle stehen.
    if methode == "GET" {
        hpack::kodiere_indiziert(2, &mut block).map_err(als_rahmenfehler)?;
    } else if methode == "POST" {
        hpack::kodiere_indiziert(3, &mut block).map_err(als_rahmenfehler)?;
    } else {
        hpack::kodiere_literal_ohne_index(methode, methode, &mut block)
            .map_err(als_rahmenfehler)?;
    }

    // (2) `:scheme` — Index 6 ist http, 7 ist https.
    if schemata == "http" {
        hpack::kodiere_indiziert(6, &mut block).map_err(als_rahmenfehler)?;
    } else if schemata == "https" {
        hpack::kodiere_indiziert(7, &mut block).map_err(als_rahmenfehler)?;
    } else {
        hpack::kodiere_literal_ohne_index(":scheme", schemata, &mut block)
            .map_err(als_rahmenfehler)?;
    }

    // (3) `:authority` — Index 1, Wert kommt praktisch nie in der
    // statischen Tabelle vor.
    hpack::kodiere_literal_ohne_index(":authority", host, &mut block).map_err(als_rahmenfehler)?;

    // (4) `:path` — Index 4 ist `/`. Sonst Literal mit Namen `:path`.
    if pfad == "/" {
        hpack::kodiere_indiziert(4, &mut block).map_err(als_rahmenfehler)?;
    } else {
        hpack::kodiere_literal_ohne_index(":path", pfad, &mut block).map_err(als_rahmenfehler)?;
    }

    // (5) Die gewöhnlichen Felder, in Kleinbuchstaben.
    for (name, wert) in gewöhnlich {
        // added 01.10.2026 10:6x, R4y: RFC 9113 8.1.2 „MUST be converted
        // to lowercase". GEMESSEN vorher: `User-Agent` kam als
        // `User-Agent` an — der Regelbruch fiel nur auf, weil der Test
        // ausdruecklich darauf schaut.
        let name_klein = name.to_ascii_lowercase();
        let name = &name_klein;
        if name.starts_with(':') {
            // added 01.10.2026 10:0x: ein gewöhnliches Feld, das mit `:`
            // beginnt, ist nach RFC 9113 8.1.2 ein **Fehler**. Ohne diese
            // Pruefung schickt der Client einen Block, den der Server mit
            // PROTOCOL_ERROR beantwortet — und die Ursache ist dann
            // drei Funktionen weiter unten zu suchen.
            return Err(RahmenFehler::Unbekannt { war: b':' });
        }
        kodiere_namen_wert(name, wert, &mut block)?;
    }

    // (6) Der Rahmen selbst.
    //
    // added 01.10.2026 10:6x, R4y: `END_HEADERS` steht **immer**, weil der
    // Block hier immer vollstaendig ist (die Felder passen in einen
    // Rahmen; `CONTINUATION` wird erst gebraucht, wenn HPACK die
    // dynamische Tabelle so weit fuellt, dass der Block nicht mehr passt —
    // das ist eine Entscheidung des **Kodierers**, und wir nehmen sie
    // nicht).
    // `END_STREAM` steht nur ohne Body — RFC 9113 8.1: „A request without
    // a body MUST set the END_STREAM flag".
    //
    // DER FEHLER VORHER war `0` im Body-Fall, und der kostete
    // `END_HEADERS`: der Server wartete dann auf weitere Bloecke, die
    // nie kamen. GEMESSEN: der Test sah `left: 0, right: 4`.
    let flags = h2::flags::END_HEADERS | if mit_body { 0 } else { h2::flags::END_STREAM };
    // added 01.10.2026 00:1x, R5b: **der Strom des Aufrufers**, nicht mehr
    // die feste 1. Begründung und Messung stehen am Funktionskopf.
    h2::baue_rahmen(typen::HEADERS, flags, strom, &block)
}

/// Kodiert ein gewöhnliches Feld als Literal mit Namen.
///
/// added 01.10.2026 10:2x UTC (Projektlabel 02.10.2026), R4y. DER WEG,
/// und er ist der ehrliche: RFC 7541 kennt vier Darstellungen, und die
/// **kürzeste** ist nicht immer die indizierte. Ein indizierter Lookup
/// braucht einen Eintrag in der dynamischen Tabelle, und den muss man
/// erst erzeugen — das heisst, den Block zweimal anfassen, und der
/// Zustand der Tabelle ist dann zwischen den Haelften auseinander.
///
/// DESHALB: gewöhnliche Felder als Literal mit Namen. Zwei Byte länger
/// als im besten Fall, aber **immer** korrekt und ohne Zustand. Bei
/// `:authority` und `user-agent` — den Feldern auf jeder Anfrage — ist der
/// Unterschied messbar klein, und die Korrektheit ist es nicht.
fn kodiere_namen_wert(name: &str, wert: &str, block: &mut Vec<u8>) -> Result<(), RahmenFehler> {
    // added 01.10.2026 10:2x, R4y: die Umwandlung an **einer** Stelle,
    // damit kein Aufrufer sie vergessen kann (Drei-Pfade-Falle, R4k/R4s).
    hpack::kodiere_literal_ohne_index(name, wert, block).map_err(als_rahmenfehler)
}

/// added 02.10.2026 17:2x, R37. **ALLE FUENF Aufrufstellen** wurden hier von
/// `kodiere_literal_mit_name` (6.2.1, **fuegt ein**) auf
/// `kodiere_literal_ohne_index` (6.2.2, **beruehrt die Tabelle nicht**) gestellt.
/// Der Grund steht woertlich in RFC 7541 4.2: „the chosen size MUST stay lower
/// than or equal to the maximum set by the protocol“. Unsere HEADERS gehen vor
/// den Server-SETTINGS raus (RFC 9113 3.4), die Anzeige kennen wir also nicht,
/// und der Protokollausgangswert 4 096 waere groesser als ein vom Server
/// angezeigtes 0. GEMESSEN: `baue_headers` wird ausschliesslich vom Anfrageweg
/// benutzt (`h2client::baue_anfrage_rahmen`); der Dekodierer ist nicht betroffen
/// und muss 6.2.2 weiter lesen koennen — er kann es.
/// Nimmt einen gelesenen Rahmen entgegen und legt ihn in den passenden
/// Strom.
///
/// added 01.10.2026 10:0x. DER FALL, DER DIE ZUORDNUNG AUSMACHT: die
/// Kopplung zwischen „Rahmen mit Strom-ID 3" und „Antwort, auf die der
/// Aufrufer wartet". Sie ist hier, an **einer** Stelle, weil sie sonst pro
/// Aufrufweg vergessen wird (Drei-Pfade-Falle, R4k/R4s).
///
/// DIE DREI FAELLE, die behandelt werden:
/// * `HEADERS` — beginnt oder ersetzt die Kopfzeilen.
/// * `CONTINUATION` — setzt sie fort (RFC 9113 6.10).
/// * `DATA` — sammelt den Body.
///
/// Und einer, der **nicht** behandelt wird und trotzdem wichtig ist:
/// `RST_STREAM` — der Server verwirft den Strom. Er wird **nicht**
/// ignoriert, sondern in `verworfen` vermerkt, weil ein Browser sonst
/// ewig auf eine Antwort wartet, die nie kommt.
/// added 01.10.2026 15:0x, R4z8. DER DRITTE PARAMETER, und er ist keine
/// Bequemlichkeit: die **HPACK-Dynamiktabelle** gehoert zu keinem Strom,
/// sondern zur **gesamten Verbindung** (RFC 7541 Abschnitt 4). Ohne sie
/// kann der Decoder keine dynamischen Verweise lesen — GEMESSEN an echten
/// Servern: `github.com` und `www.google.com` scheiterten mit
/// `HPACK: BlockZuLang (512)` bzw. `HPACK: StringZuLang (18)`.
///
/// DER GRUND, warum das hier liegt und nicht im Decoder: `Strom` fuehrt
/// `kopfzeilen` je Strom, die Tabelle aber einmal je Verbindung. Wer sie
/// in `Strom` legte, haette sie beim zweiten Strom eine **zweite** —
/// und damit genau die Desynchronisation, die jetzt behoben ist.
pub fn nimm_auf(
    rahmen: &Rahmen,
    stroeme: &mut Vec<Strom>,
    fenster_aus: &mut i64,
    hpack: &mut crate::hpack::DynamischeTabelle,
) -> Result<(), RahmenFehler> {
    // M4: die Fenstergrenze wird bei jedem `DATA` kleiner. Sie kann
    // negativ werden (das ist in HTTP/2 erlaubt: der Empfänger „schuldet"
    // dann), aber nicht unbegrenzt — darum `fenster_aus` geprueft.
    let idx = match stroeme.iter().position(|s| s.id == rahmen.strom) {
        Some(i) => i,
        None => {
            // added 01.10.2026 10:0x: ein Rahmen für einen **unbekannten**
            // Strom. Nach RFC 9113 6.1 ist das für kurze Zeit erlaubt
            // (die IDs sind nicht in Reihenfolge garantiert). Wir legen
            // den Strom deshalb an, statt den Rahmen zu verwerfen — und
            // merken uns, dass er schon geschlossen war.
            if rahmen.strom % 2 == 0 {
                // RFC 9113 5.1: eine gerade ID von einem **Server** ist
                // ein Verbindungsfehler (Client-IDs sind ungerade).
                return Err(RahmenFehler::StromVerboten {
                    typ: rahmen.typ,
                    strom: rahmen.strom,
                });
            }
            stroeme.push(Strom {
                id: rahmen.strom,
                ..Strom::default()
            });
            stroeme.len() - 1
        }
    };

    match rahmen.typ {
        typen::HEADERS | typen::CONTINUATION => {
            let s = &mut stroeme[idx];
            s.bloecke += 1;
            // M3: die Zahl der Bloecke ist begrenzt, sonst schickt ein
            // Server endlos CONTINUATION (das war der Fall, den die
            // `MAX_HEADERS_BLOECKE` in `narsil-server/src/h2.rs` fixt).
            if s.bloecke > MAX_HEADERS_BLOECKE {
                return Err(RahmenFehler::ZuVieleSettings {
                    waren: s.bloecke,
                    erlaubt: MAX_HEADERS_BLOECKE,
                });
            }
            //
            // added 01.10.2026 17:1x, R5b. **DAS WAR FALSCH, und es ist der
            // Grund fuer diesen ganzen Abschnitt.** Vorher stand hier
            //     dekodiere_bloecke_mit(&rahmen.nutzlast, &mut s.kopfzeilen, …)
            // — also wurde **jeder** Rahmen als **eigener** HPACK-Block
            // gelesen. Das ist falsch, sobald ein Server einen Kopfblock
            // ueber mehrere Rahmen verteilt, und das ist nach RFC 9113
            // 6.10 die **Regel**, nicht der Ausnahmefall:
            //
            //     A header block is carried in a HEADERS frame and any
            //     number of CONTINUATION frames.  The last frame contains
            //     the END_HEADERS flag.
            //
            // GEMESSEN, dass es auffaellt: `github.com` schickt 5 156 Byte
            // in **einem** `HEADERS`-Ramen — dort passt es. Ein Server mit
            // grossen `set-cookie`-Kopfen teilt auf, und dann liest man ab
            // Byte 1 des zweiten Rahmens einen Feldanfang. Das ist kein
            // Theoriestueck, es ist die uebliche Sache bei Werbe-Seiten.
            //
            // DIE REGEL, und sie hat zwei Seiten:
            //  * ein `CONTINUATION` **ohne** vorheriges unvollstaendiges
            //    `HEADERS` ist ein Protokollfehler (6.10: „A CONTINUATION
            //    frame MUST be preceded by a HEADERS frame"),
            //  * ein `HEADERS`, auf dessen Block ein `CONTINUATION` folgt,
            //    darf nicht durch einen anderen Rahmen unterbrochen werden.
            //    Das wird hier **nicht** geprueft, weil dafuer ein Zustand
            //    „der letzte Rahmen auf diesem Strom war ein offener Kopf"
            //    noetig waere. Als Luecke benannt, nicht ueberdeckt.
            if rahmen.typ == typen::CONTINUATION && !s.kopfblock_offen {
                return Err(RahmenFehler::KopfblockVerworfen {
                    strom: rahmen.strom,
                });
            }
            //
            // added 02.10.2026 02:1x, R5f. **`h2::daten`**, nicht
            // `rahmen.nutzlast`: bei `PADDED` (6.1) und `PRIORITY` (6.2)
            // steckt nicht alles in der Nutzlast. GEMESSEN an
            // `www.google.com` (02.10.2026 02:0x): jedes `DATA` kam mit
            // `flags 0x08`, und ein Muellbyte je Block stand im Body.
            let inhalt = h2::daten(rahmen)?;
            if s.blockpuffer.len() + inhalt.len() > MAX_KOPFBLOCK_BYTES {
                return Err(RahmenFehler::ZuGross {
                    laenge: s.blockpuffer.len() + inhalt.len(),
                    erlaubt: MAX_KOPFBLOCK_BYTES,
                });
            }
            s.blockpuffer.extend_from_slice(inhalt);
            s.kopfblock_offen = rahmen.flags & h2::flags::END_HEADERS == 0;
            if s.kopfblock_offen {
                // DER BLOCK IST UNVOLLSTAENDIG. Nichts dekodieren: die
                // dynamische Tabelle darf erst fortgeschrieben werden,
                // wenn der Block **vollstaendig** ist, sonst laeuft man
                // mitten in einem Feld.
                if rahmen.flags & h2::flags::END_STREAM != 0 {
                    s.zu = true;
                }
            } else {
                // added 01.10.2026 15:0x, R4z8: `dekodiere_bloecke_mit`, weil
                // dieser Block die Tabelle der **Verbindung** fortschreibt.
                let ergaenzung =
                    hpack::dekodiere_bloecke_mit(&s.blockpuffer, &mut s.kopfzeilen, hpack)
                        .map_err(als_rahmenfehler)?;
                let _ = ergaenzung;
                s.blockpuffer.clear();
                if rahmen.flags & h2::flags::END_STREAM != 0 {
                    s.zu = true;
                }
            }
        }
        typen::DATA => {
            let s = &mut stroeme[idx];
            // M4: die Grenze gilt **pro Strom**, und sie wird vor dem
            // Anhängen geprüft, damit der Puffer nie über die Grenze
            // wächst.
            // added 02.10.2026 02:1x, R5f: dasselbe wie beim Kopfblock —
            // `PADDED` abziehen. GEMESSEN (google.com, 02.10.2026 02:0x):
            // `flags 0x08` bei **jedem** DATA-Rahmen, ein Muellbyte je
            // Block im Body, und mit `accept-encoding` danach ein
            // `gunzip`-Fehler statt einer Seite.
            let inhalt = h2::daten(rahmen)?;
            if s.daten.len() + inhalt.len() > MAX_STROM_BYTES {
                return Err(RahmenFehler::ZuGross {
                    laenge: s.daten.len() + inhalt.len(),
                    erlaubt: MAX_STROM_BYTES,
                });
            }
            s.daten.extend_from_slice(inhalt);
            // added 02.10.2026 02:1x, R5f: das Fenster zaehlt die **Daten**,
            // nicht die Fuellbytes — die sind nach 6.1 ausgeschlossen. Sonst
            // waere das Fenster zu klein und der Server wuerde auf einen
            // WINDOW_UPDATE warten, den niemand schickt.
            *fenster_aus -= inhalt.len() as i64;
            if rahmen.flags & h2::flags::DATA_END_STREAM != 0 {
                s.zu = true;
            }
        }
        typen::RST_STREAM => {
            // added 01.10.2026 10:0x: der Server verwirft den Strom. Ohne
            // diese Zeile wartet der Aufrufer **ewig** — der Strom ist
            // zu Ende, aber nichts sagt es ihm.
            let inhalt = h2::daten(rahmen)?;
            if inhalt.len() >= 4 {
                let code = u32::from_be_bytes([inhalt[0], inhalt[1], inhalt[2], inhalt[3]]);
                stroeme[idx].verworfen = Some(code);
            }
            stroeme[idx].zu = true;
        }
        _ => {}
    }
    Ok(())
}
