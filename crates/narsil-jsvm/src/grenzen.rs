//! Harte Grenzen der Engine (J5). Jede Schleife und jede Allokation, die
//! von der Eingabe abhaengt, ist durch eine dieser Schranken gedeckelt
//! (Speicher-Regeln M3/M4 aus `team/WELLE4.txt`).

/// Harte Grenzen fuer einen [`crate::Engine`]-Lauf.
///
/// Alle Felder sind Obergrenzen: Ueberschreiten endet mit
/// [`crate::JsFehler`], nie mit Panic oder wachsendem Speicher.
#[derive(Debug, Clone, Copy)]
pub struct Grenzen {
    /// Obergrenze des JS-Heaps in Bytes. Standard 64 MiB.
    /// Ueberschreitung -> `JsFehler::Speicher`.
    pub max_heap_bytes: usize,
    /// Obergrenze der Interpreter-Schritte (Schleifendurchlauf, Aufruf,
    /// Anweisung). Standard 50 Mio. Ueberschreitung -> `JsFehler::Zeitlimit`.
    /// Zusaetzlich wacht eine Wanduhr (`max_dauer_ms`): `while(true){}`
    /// endet immer, auch wenn 50 Mio Schritte auf langsurer Maschine
    /// laenger dauern wuerden.
    pub max_schritte: u64,
    /// Die **Wanduhr** eines `ausfuehren`-Aufrufs in Millisekunden.
    /// Standard 1900, hart 600.000 (10 min).
    ///
    /// GEMESSEN (J3): die Grenze lag vorher als feste Konstante
    /// (`MAX_WANDUHR_MS`) im Motor — ein Host konnte sie nicht anheben,
    /// und `examples/mess.rs` brach nach 1,9 s ab (alle vier Lasten). Der
    /// Wert ist jetzt eine Grenze wie jede andere: der Host darf einer
    /// Seite langer rechnen lassen, ein Endlosschleifen-Skript nicht.
    pub max_dauer_ms: u64,
    /// Maximale JS-Aufruftiefe — das sind **Rahmen in einem `Vec`**, keine
    /// Rust-Stapel-Ebenen (V1). Standard 10.000 (V4), harte Grenze 20.000.
    /// Ueberschreitung wirft einen FANGBAREN RangeError
    /// ("Maximum call stack size exceeded").
    pub max_aufruftiefe: u32,
    /// Maximale Laenge EINER Zeichenkette in UTF-16-Einheiten (-naeherungs-
    /// weise Zeichen). Standard 8 Mio. Ueberschreitung -> `Speicher`.
    pub max_string_laenge: usize,
    /// Maximale Quelltext-Laenge in Bytes. Standard 5 MiB.
    pub max_quelltext_bytes: usize,
    /// Maximale Token-Zahl. Standard 300.000.
    pub max_token: usize,
    /// Maximale Verschachtelungstiefe beim Parsen (M5). Standard 512.
    pub max_parser_tiefe: u32,
    /// Maximale Laenge eines Ursprungs (fuer `JSON.parse`). Standard 64 MiB.
    pub max_json_bytes: usize,
    /// Maximale Tiefe von `JSON.parse`/`stringify`. Standard 512.
    pub max_json_tiefe: u32,
    /// Maximale Zahl ausstehender Mikrotasks/Timer. Standard 100.000.
    pub max_mikrotasks: usize,
    /// Obergrenze der gleichzeitig geplanten Timer (A2). Ueberschreitung ->
    /// `JsFehler::Speicher`. Begrenzt die Liste, damit `setInterval` in
    /// einer Schleife nicht den Speicher frisst (M3/M4).
    pub max_timer: usize,
    /// Obergrenze der Timer-Ausfuehrungen je Lauf (A2). Danach endet die
    /// Schleife kontrolliert (kein Endloslaufen, kein Wachstum).
    pub max_timer_ausfuehrungen: usize,
    /// Obergrenze der gleichzeitig wartenden `fetch`-Anfragen (A4).
    /// Standard 64, hart 256 (M3/M4). Ueberschreitung -> das Versprechen
    /// wird mit einem `Error` abgelehnt, kein Panic.
    pub max_netz_aufgaben: usize,
    /// Obergrenze fuer den Text EINER Antwort (A4). Standard 1 MiB, hart
    /// 16 MiB. Eine groessere Antwort wird abgelehnt, BEVOR sie in den
    /// Heap wandert.
    pub netz_antwort_bytes: usize,
    /// Gesamtfrist einer Anfrage in Millisekunden (A4). Standard 15 s.
    /// Wird an `narsil_net::Limits::gesamt` weitergegeben.
    pub netz_frist_ms: u64,
    /// Obergrenze der `addEventListener`-Anmeldungen je Motor (A6).
    /// Standard 1024, hart 4096. Ueber der Grenze wird die Anmeldung
    /// stillschweigend verworfen (wie im Browser mit 0 Hoerern).
    pub max_hoerer: usize,
    /// Obergrenze der erzeugten DOM-Knoten je Lauf (J1). Standard 20.000,
    /// hart 200.000. Ueberschreitung -> `Error` (kein Panic), der Zaehler
    /// wird nie zurueckgesetzt (dokumentiert: „pro Lauf").
    pub max_dom_knoten: usize,
    /// Obergrenze der DOM-Tiefe (J1, M5). Standard 64, hart 512.
    pub max_dom_tiefe: u32,
    /// Obergrenze des `innerHTML`-Textes in Bytes (J1, M4). Standard
    /// 4 MiB, hart 32 MiB.
    pub max_dom_html: usize,
    /// **Schrittbudget je Frame** (J25) — Standard 400.000 Schritte.
    ///
    /// WARUM EINE EIGENE ZAHL: `max_schritte` gilt fuer den **ganzen**
    /// `ausfuehren`-Aufruf. Ein Skript kann aber in einem Bild-Frame
    /// 49,9 Mio Schritte verbrauchen und damit die Oberflaeche fuer
    /// **zweieinhalb Sekunden** blockieren, ohne dass irgendeine Grenze
    /// greift — bei 60 Hz sind das 150 Bilder. Das ist der einzige Weg,
    /// wie ein Skript im Browser den Bildaufbau anhalten kann, und es ist
    /// genau das, was J25 verhindern soll.
    ///
    /// Ueberschreitung -> `JsFehler::Zeitlimit` mit dem Text
    /// „Frame-Budget N Schritte ueberschritten". Der Host setzt das Budget
    /// je Frame neu (`Engine::frame_beginnen`), damit ein Skript ueber
    /// viele Frames arbeiten darf — aber **nicht** in einem einzigen.
    pub frame_schritte: u64,
}

impl Grenzen {
    /// Standardgrenzen nach J5 (heap 64 MiB, 50 Mio Schritte) mit W5B:
    /// Aufruftiefe 2000 im 256-MiB-Thread, Ueberschreitung fangbar.
    pub fn standard() -> Self {
        Grenzen {
            max_heap_bytes: 64 * 1024 * 1024,
            max_schritte: 50_000_000,
            max_dauer_ms: 1_900,
            max_aufruftiefe: 10_000,
            max_string_laenge: 8_000_000,
            max_quelltext_bytes: 5 * 1024 * 1024,
            max_token: 300_000,
            max_json_bytes: 64 * 1024 * 1024,
            max_json_tiefe: 512,
            // Parser-Tiefe zaehlt Frames (ca. 3 je Ebene): 128 echte Ebenen
            // brauchen unter 1 MiB Rust-Stapel (gemessen 09/2026) — sicher
            // auf 2-MiB-Test-Threads, grosszuegig fuer echten Code.
            max_parser_tiefe: 128,
            max_mikrotasks: 100_000,
            max_timer: 10_000,
            max_timer_ausfuehrungen: 100_000,
            // A4: 64 gleichzeitige Anfragen reichen fuer jede Seite; die
            // Antwortgrenze von 1 MiB deckt ein Bild, kein Video.
            max_netz_aufgaben: 64,
            netz_antwort_bytes: 1024 * 1024,
            netz_frist_ms: 15_000,
            max_hoerer: 1024,
            // J1: eine Seite mit 20.000 Knoten ist ein grosses Dokument.
            max_dom_knoten: 20_000,
            max_dom_tiefe: 64,
            max_dom_html: 4 * 1024 * 1024,
            // J25: **20.000 Schritte je Frame.** GEMESSEN (dieser Rechner,
            // Debug-Build — Release ist ~4x schneller, die **Verhaeltnisse**
            // gelten aber): eine Schleifendurchlauf-Iteration kostet
            // **9 Schritte** (100.000 Iterationen = 900.012). 20.000
            // Schritte sind damit ~2.200 Iterationen — eine Skript-
            // "Frame-Arbeit", wie sie ein echter Handler leistet (ein
            // typischer Klick-Handler: ein paar hundert Operationen).
            //
            // Der Wert ist **kein** 16-ms-Ziel: bei 9 Schritten je
            // Iteration und ~4 Mio Schritten/s (Debug) sind 20.000
            // Schritte ~5 ms. Im Release (~16 Mio/s) waeren es ~1,2 ms.
            // Der Wert steht auf **Arbeit**, nicht auf Zeit, weil Zeit
            // von der Maschine abhaengt und Arbeit nicht — und die
            // Zeitgrenze (`max_dauer_ms`, 1900 ms) deckt den Fall, dass
            // der Rechner langsam ist, bereits ab.
            frame_schritte: 20_000,
        }
    }

    /// Winzige Grenzen fuer `tests/bomben.rs`: jeder Angriff muss in
    /// Millisekunden mit benanntem Fehler enden.
    pub fn bombe() -> Self {
        Grenzen {
            max_heap_bytes: 2 * 1024 * 1024,
            max_schritte: 200_000,
            max_dauer_ms: 1_900,
            max_aufruftiefe: 64,
            max_string_laenge: 200_000,
            max_quelltext_bytes: 1024 * 1024,
            max_token: 50_000,
            max_json_bytes: 1024 * 1024,
            max_json_tiefe: 64,
            max_parser_tiefe: 64,
            max_mikrotasks: 10_000,
            max_timer: 200,
            max_timer_ausfuehrungen: 500,
            // A4: kleine Bomben-Grenzen (M1: benannte Konstante je
            // Eingabeschleife — der Antworttext zaehlt hier doppelt).
            max_netz_aufgaben: 4,
            netz_antwort_bytes: 4 * 1024,
            netz_frist_ms: 500,
            max_hoerer: 64,
            // J1: Bomben-Grenzen — der Test in `tests/bomben.rs` soll
            // schnell mit einem benannten Fehler enden.
            max_dom_knoten: 64,
            max_dom_tiefe: 8,
            max_dom_html: 8 * 1024,
            // Bomben-Test: 2.000 Schritte je Frame — der Test soll an der
            // **anderen** Grenze scheitern (Schrittgrenze oder Wanduhr).
            frame_schritte: 2_000,
        }
    }
}

/// Harte Obergrenze der Quelltext-Laenge als benannte Konstante (M3/M4):
/// laengere Eingaben werden abgelehnt, bevor Token alloziert werden.
pub(crate) const MAX_QUELLTEXT_HART: usize = 32 * 1024 * 1024;
/// Harte Obergrenze der Token-Zahl, unabhaengig von `Grenzen` (M4).
///
/// 05.10.2026: von 2 Mio auf **8 Mio** gehoben. GEMESSEN an
/// www.youtube.com: das Haupt-Bundle ist 10 847 774 Byte und lief bei
/// 1,5 Mio Token in `zu viele Token` — auf jeder Seite mit modernem
/// Bundling stirbt damit das ganze Skript. Das Lexen ist billig
/// (2 Mio Token = 0,82 s, gemessen mit `examples/js_probe.rs`), und die
/// AUSFUEHRUNG bleibt durch `max_dauer_ms`/`max_schritte` begrenzt: ein
/// zu grosses Programm kostet Zeit, aber es reisst die Seite nicht mit.
pub(crate) const MAX_TOKEN_HART: usize = 8_000_000;
/// Harte Obergrenze der Parser-Verschachtelung (M5).
pub(crate) const MAX_PARSER_TIEFE_HART: u32 = 4096;
/// Harte Obergrenze der gleichzeitig wartenden `fetch`-Anfragen (A4, M4).
pub(crate) const MAX_NETZ_AUFGABEN_HART: usize = 256;
/// Harte Obergrenze des Antworttextes einer `fetch`-Anfrage (A4, M4).
pub(crate) const MAX_NETZ_ANTWORT_HART: usize = 16 * 1024 * 1024;
/// Harte Obergrenze der Frist einer `fetch`-Anfrage (A4, M4).
pub(crate) const MAX_NETZ_FRIST_HART_MS: u64 = 120_000;
/// Harte Obergrenze der `addEventListener`-Anmeldungen (A6, M4).
pub(crate) const MAX_HOERER_HART: usize = 4096;
/// Harte Obergrenze der Wanduhr eines `ausfuehren`-Aufrufs (J3, M4):
/// 10 Minuten. Das ist zugleich die Zeitgrenze von `werkzeuge/bau.sh`.
pub(crate) const MAX_DAUER_HART_MS: u64 = 600_000;
/// Harte Obergrenze der erzeugten DOM-Knoten je Lauf (J1, M4).
pub(crate) const MAX_KNOTEN_HART: usize = crate::dom::MAX_KNOTEN_HART;
/// Harte Obergrenze der Interpreter-Aufruftiefe (M5): schuetzt den
/// Rust-Stapel, auch wenn `Grenzen` groessere Werte erlaubt. Mit 256 MiB
/// Kind-Stapel (W5B) und ~18 KiB je Aufruf im Debug-Profil sind 8192
/// Aufrufe ~150 MiB — sicher mit Reserve.
pub(crate) const MAX_AUFRUFTIEFE_HART: u32 = 20_000;
/// Der Stapel JEDES Laufs (W5B, W6D).
///
/// GEMESSEN am 14:35 (W6D, Test `crates/narsil/tests/_stapel_messen.rs`,
/// danach geloescht): ein Aufruf dieser Engine braucht **26.800 Bytes**
/// Rust-Stapel. Das ist viel mehr, weil der Baum-Interpreter fuer jeden
/// `Ausdruck` einen eigenen Rahmen baut.
///
/// `max_aufruftiefe: 2000` verlangt damit rund **54 MiB**. Deswegen laeuft
/// JEDER Weg in die Engine (`Engine::ausfuehren`, `Engine::handler_aufrufen`)
/// ueber `Engine::auf_dickem_stapel` mit 256 MiB. Ohne diesen Thread
/// endet ein rekursiver Handler mit `has overflowed its stack` (SIGABRT),
/// statt mit dem fangbaren `RangeError` (M5).
pub const STAPEL_LAUF: usize = 256 * 1024 * 1024;
/// Hoechstzahl Argumente, die ein Handler-Aufruf annimmt (M3/M4, W6D).
///
/// GEMESSEN, warum 8: das sind die Argumente, die die Ereignisse in
/// echtem Code bekommen (`ev` plus ein bis zwei Werte). Mehr anzunehmen
/// waere eine Wuerfelei fuer einen Browser ohne `arguments`-Objekt.
pub const MAX_HANDLER_ARGUMENTE: usize = 8;
/// Harte Obergrenze des Heaps (M4): 1 GiB, selbst bei grossen `Grenzen`.
pub(crate) const MAX_HEAP_HART: usize = 1024 * 1024 * 1024;
/// Harte Obergrenze der Mikrotask-Schlange (M6: keine unbegrenzte Warteschlange).
pub(crate) const MAX_MIKROTASK_HART: usize = 1_000_000;
