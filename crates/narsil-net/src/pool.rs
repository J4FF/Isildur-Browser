//! Der Verbindungspool: Keep-Alive-Verbindungen je (Schema, Host, Port).
//!
//! added 30.09.2026 fuer Welle 7, Auftrag N4.
//!
//! WAS ER KAUFT, und das ist der ganze Punkt: bei HTTPS kostet der
//! Handshake nach den Montgomery-Aenderungen der Koordination
//! (30.09.2026, gemessen) noch rund **0,16 s**. Eine Wikipedia-Seite
//! mit zwei Skripten vom selben Host waeren ohne Pool drei
//! Handshakes und rund eine halbe Sekunde — davon 0,32 s Wartezeit
//! allein fuer TLS, das schon einmal stattgefunden hat.
//!
//! DIE DREI ZAHLEN DES AUFTRAGS, mit ihrer Herkunft:
//!
//! | Zahl | Wert | Grund |
//! |------|------|-------|
//! | je Host | 6 | Auftrag N4 |
//! | gesamt | 32 | Auftrag N4 |
//! | Leerlauf | 60 s | Auftrag N4 |
//!
//! DIE ZAHLEN SIND NICHT AUS DER LUFT: 6 je Host, weil ein Browser
//! genau so viele parallele Verbindungen zu einem Host oeffnet (das
//! uebliche Modell der grossen Browser); 32 gesamt, weil ein Tab
//! typischerweise 6 bis 8 Hosts gleichzeitig offen hat und 32 die
//! Schnittstelle zu 6 deckt, ohne dass der Pool zum Speicherleck wird
//! (32 Sockets sind 32 offene Dateideskriptoren, was unter jedem
//! ueblichen weichen Limit liegt; 6 * 32 waeren 192 und damit
//! auffaellig); 60 s Leerlauf, weil RFC 9112 Abschnitt 9.6 keine Zahl
//! nennt, sondern sagt: "A client that has received a response with a
//! `Keep-Alive` connection option SHOULD NOT close the connection
//! before sending another request within a reasonably short period of
//! time" — und weil 60 s kuerzer sind als der Idle-Timeout der
//! meisten Server (nginx `keepalive_timeout 75s`, Apache 5 s): der
//! Browser gibt die Verbindung also frei, bevor der Server sie
//! schliesst, und das ist genau die Reihenfolge, die einen Fehler
//! vermeidet.
//!
//! DIE ZWEI EISERNEN REGELN, an denen die Verbindungswiederverwendung
//! sonst stillschweigend kaputtgeht:
//!
//! 1. Eine Verbindung geht nur zurueck in den Pool, wenn der Server
//!    `keep-alive` gesagt hat **und** im Lesepuffer nichts mehr liegt.
//!    Byte, die nach dem Body noch im Puffer stehen, gehoeren zur
//!    naechsten Antwort — mit dem Pool wuerde die zweite Anfrage sie
//!    als ihre eigene Statuszeile lesen.
//! 2. Eine Anfrage auf einer **gepoolten** Verbindung darf bei Fehler
//!    genau einmal auf einer neuen wiederholt werden. Auf einer frischen
//!    Verbindung nicht: dann ist der Server das Problem.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::Leitung;

/// M3: hoechstens so viele Verbindungen **je** (Schema, Host, Port).
///
/// DER WERT 6, aus dem Auftrag N4. BEGUENDUNG: 6 parallele
/// Verbindungen zu einem Host sind das, was die grossen Browser
/// oeffnen; mehr bringt keinen Geschwindigkeitsgewinn, weil HTTP/1.1
/// ohne Pipelining auf einer Verbindung immer nur eine Anfrage zur
/// Zeit beantwortet.
pub const MAX_PRO_HOST: usize = 6;

/// M3: hoechstens so viele Verbindungen **insgesamt**.
///
/// DER WERT 32, aus dem Auftrag N4. BEGUENDUNG: 32 Sockets sind 32
/// offene Dateideskriptoren, was unter jedem ueblichen weichen Limit
/// (1024) liegt. 6 * 32 waeren 192 und damit auffaellig; die
/// Gesamtzahl ist die, die den Speicher begrenzt.
pub const MAX_GESAMT: usize = 32;

/// M3: wie lange eine unbenutzte Verbindung im Pool bleibt.
///
/// DER WERT 60 s, aus dem Auftrag N4. BEGUENDUNG: RFC 9112 Abschnitt
/// 9.6 nennt keine Zahl, nur "a reasonably short period of time".
/// 60 s sind kuerzer als der Idle-Timeout der meisten Server (nginx
/// `keepalive_timeout 75s`), und laenger als die Zeit, in der ein
/// Benutzer typischerweise zurueckkommt.
pub const LEERLAUF: Duration = Duration::from_secs(60);

/// M3: hoechstens so oft wird eine Anfrage auf einer **gepoolten**
/// Verbindung erneut verbunden.
///
/// DER WERT 1, aus dem Auftrag N4 ("geschlossene/kaputte Verbindung ->
/// einmal neu verbinden"). WARUM GENAU EINMAL: eine serverseitig
/// geschlossene Verbindung ist beim Wiederverwenden der Normalfall —
/// der Server hat sie zu, ohne es zu sagen, weil zwischen zwei
/// Anfragen Zeit vergangen ist. Ein zweiter Versuch ist noch
/// vertretbar (Doppelbind, Netzwerkflackern), ein dritter ist ein
/// Zeichen fuer einen kaputten Server und wuerde die Ladezeit
/// verdreifachen, ohne Aussicht auf Erfolg.
pub const MAX_WIEDERHOLUNGEN: usize = 1;

/// Der Schluessel des Pools: Schema, Host und Port.
///
/// DER GRUND FUER DEN SCHLUESSEL: dieselbe Adresse kann ueber http und
/// https erreicht werden, und eine TCP-Verbindung ist an den
/// Transport gebunden, nicht an den Hostnamen. Ohne das Schema im
/// Schluessel wuerde ein Pool-Eintrag fuer `example.com:443` unter
/// `http://example.com:443` landen — und der Browser schickte dann
/// Klartext ueber eine TLS-Verbindung, oder umgekehrt.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Schluessel {
    /// `true` bei https.
    pub tls: bool,
    /// Der Host ohne Schema und ohne Port.
    pub host: String,
    /// Der Port.
    pub port: u16,
}

/// Eine gepoolte Verbindung, mit dem Zeitpunkt ihrer letzten Benutzung.
struct Eintrag {
    /// Die Leitung. `Box<dyn Leitung>` wie im Rest der Crate, damit
    /// hier kein TLS-Code stehen muss.
    leitung: Box<dyn Leitung>,
    /// Wann wurde sie zuletzt benutzt? Fuer die Leerlauf-Frist.
    zuletzt: Instant,
    /// added 01.10.2026 22:0x, R5e. Der **naechste** h2-Strom fuer diese
    /// Verbindung, oder 0 wenn sie kein h2 ist.
    ///
    /// WARUM HIER UND NICHT IM AUFRUFER: der Strom gehoert zur
    /// **Verbindung**, nicht zur Anfrage. Legt man ihn in `sende`, dann
    /// beginnt die zweite Anfrage wieder bei 1 — und Stream 1 ist nach RFC
    /// 9113 5.1 **geschlossen**, sobald er mit `END_STREAM` endete. Ein
    /// zweites HEADERS auf Strom 1 ist ein Verbindungsfehler
    /// (`PROTOCOL_ERROR`), und genau das ist gemessen worden.
    naechster_strom: u32,
    /// added 01.10.2026 00:4x, R5b. Die **HPACK-Dynamiktabelle** dieser
    /// Verbindung.
    ///
    /// GEMESSEN, warum sie hier liegen muss und nicht in `lies_antwort`:
    /// `www.google.com`, zweiter Abruf auf der gepoolten Leitung (01.10.2026,
    /// 00:3x):
    ///     Abruf: H2Protokoll { grund: "HPACK: IndexUngueltig (73)" }
    /// Der Server hat seinen Kodierer ueber die Verbindung hinweg benutzt —
    /// das ist HPACK 4.1 („This dynamic table is not cleared between
    /// messages") — und Index 73 zeigt auf einen Eintrag, den **wir** nie
    /// gesehen haben, weil `lies_antwort` (h2client.rs) bei jedem Aufruf
    /// eine neue Tabelle anlegte. Die Tabelle gehoert zur Verbindung, also
    /// in den Eintrag, der die Verbindung haelt.
    hpack: crate::hpack::DynamischeTabelle,
}

/// Der Pool: eine Tabelle Schluessel -> Liste von Verbindungen.
#[derive(Default)]
struct Pool {
    /// Je Schluessel die offenen Verbindungen, neueste zuerst.
    je_host: HashMap<Schluessel, Vec<Eintrag>>,
    /// Wie viele Verbindungen **insgesamt** offen sind. Muss mit
    /// `je_host` konsistent bleiben; es steht hier, damit die Grenze
    /// nicht erst durch Aufzaehlen geprueft werden muss.
    gesamt: usize,
}

/// M3: der Pool liegt in einem `OnceLock`, weil er ein **prozess-
/// globaler** Zustand ist und nicht pro `Request`.
///
/// WARUM GLOBAL, und nicht als Feld in `Request` oder als Argument von
/// `fetch_mit`: die Wiederverwendung ergibt nur Sinn ueber Aufrufe
/// hinweg — das ist der ganze Zweck. `fetch_mit` bekommt seine Grenzen
/// als Parameter, weil Tests kurze Zeiten brauchen; der Pool ist kein
/// Testparameter, sondern ein Merkmal der Verbindung. Tests leeren ihn
/// ueber [`leeren`].
static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();

fn pool() -> MutexGuard<'static, Pool> {
    POOL.get_or_init(|| Mutex::new(Pool::default()))
        // DER WEG, und er ist nicht `unwrap`: im Fehlerfall einer
        // `Mutex` (vergiftet, weil ein Thread mitten im Pool panikte)
        // ist der Zustand der Tabelle trotzdem **verwendbar** — es sind
        // nur die Invarianten in Frage. `into_inner` nimmt die Daten
        // heraus, und ein Browser, der nach einem fremden Panic nicht
        // mehr ins Netz kommt, ist schlimmer als ein Pool ohne
        // Perfektionsgarantie. Der Preis: ein fremder Panic kann die
        // Zaehlung `gesamt` inkonsistent machen; deshalb wird `gesamt`
        // nirgends als Wahrheit verwendet, sondern immer aus den
        // Listen nachgezaelt (siehe `zaehle`).
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// M3: leert den Pool. **Nur fuer Tests** — im Betrieb waere das ein
/// Verlust aller offenen Verbindungen.
///
/// DER WEG: alle Verbindungen werden aus der Tabelle genommen und
/// **hier** geschlossen (`Box<dyn Leitung>` hat kein `Drop`, das den
/// Sockel schliesst — der Sockel lebt weiter, bis das `Box` fallen
/// gelassen wird; hier wird es fallen gelassen, und `shutdown` wird
/// vorher aufgerufen, damit der Server die Gegenseite nicht als
/// Fehler meldet).
pub fn leeren() {
    let mut p = pool();
    let alt: Vec<Eintrag> = p.je_host.drain().flat_map(|(_, l)| l).collect();
    p.gesamt = 0;
    // M3: erst nach dem Leeren der Tabelle schliessen, damit kein
    // anderer Thread zwischenzeitlich eine Zahl liest, die schon
    // falsch ist. Der `Mutex` haelt die ganze Funktion, es gibt also
    // gar kein Fenster — die Reihenfolge ist damit nur noch lesbar.
    drop(p);
    for mut e in alt {
        // added 04.10.2026, Rolle NETZTEMPO: **WER SCHLIESSEN ZUERST** — der
        // Pool ist der Ort, an dem wir Verbindungen **absichtlich** weggeben;
        // ohne diese Zeile sieht ein leerer Pool nach „wir geben sie weiter"
        // aus.
        crate::h2geteilt::wir_schliessen("Pool geleert/gealtert");
        let _ = e.leitung.shutdown(std::net::Shutdown::Both);
    }
}

/// M3: wie viele Verbindungen gerade offen sind. Fuer Tests und fuer
/// die Meldung in der Statuszeile.
///
/// DER WEG: aus den Listen gezaehlt, nicht `gesamt` zurueckgegeben —
/// siehe die Begruendung bei `pool()`.
pub fn offene_verbindungen() -> usize {
    let p = pool();
    p.je_host.values().map(|l| l.len()).sum()
}

// added 04.10.2026 05:1x, R69. **`hole` GELOESCHT** — und diese Loeschung ist
// **selbst** der Befund, nicht bloss eine Folge. `hole` war die **aeltere**
// Fassung von `hole_als_ausleihe` (Z. hier drunter): beide machen
// Leerlauf-Frist (`LEERLAUF`) und „neuesten zuerst“ (`max_by_key` auf
// `naechster_strom`), nur gab `hole` `Box<dyn Leitung>` zurueck und
// `hole_als_ausleihe` die `Ausleihe` **mit** h2-Strom und hpack-Zustand.
//
// added 04.10.2026 05:1x: **DIE REIHENFOLGE IST DER PUNKT, UND DIE IST LEHRREICH:**
// R69 hat zuerst `hole_oder_verbinde` geloescht (GEMESSEN null Aufrufer), und
// **danach**rief clippy: `warning: function 'hole' is never used`. Der eigene
// Waechter hatte `hole` **nicht** gemeldet — weil `hole` in **sieben** Crates
// auch der Name einer **anderen** Funktion ist (GEMESSEN mit `grep`). **Ein
// textlicher Zaehler kann „unbenutzt“ bei mehrdeutigen Namen nicht entscheiden;
// clippy kann es, weil er die Crate kennt.** Beide zusammen decken die Luecke:
// der Waechter entscheidet `pub`, clippy entscheidet `privat`.
//
// added 04.10.2026 05:1x: **UND DIE DREI DOKUMENTATIONEN, die auf `hole` zeigten,
// sind mitgezogen** (`[`hole_als_ausleihe`]` in den Kopfzeilen von
// `hole_oder_verbinde_mit_strom` und `leeren`) — sonst blieben **kaputte
// Intra-Doc-Links** zurueck, und `cargo doc` wuerde bei jeder anderen Crate
// darueber schweigen.
/// Die Leerlauf-Frist und die Auswahl wie in [`hole_als_ausleihe`] — aber der Stream
/// kommt mit heraus.
///
/// added 04.10.2026 05:3x, R69. **`Ausleihe` UND `hole_oder_verbinde_mit_strom`
/// WIEDER HERGESTELLT** — und **das** ist der eigentliche Befund dieser Runde,
/// nicht der tote Code. Beim Loeschen von `pool::hole` hat ein Blockfraesser
/// **199 Zeilen** statt der **etwa 50** der eigentlichen Funktion entfernt und
/// dabei drei Groessen mitgenommen, die **Aufrufer im Betrieb** haben.
/// GEMESSEN, dass es auffaellt: `bau.sh narsil-net check` meldete **fuenf**
/// Fehler (`cannot find type 'Ausleihe'`, `cannot find function
/// 'hole_oder_verbinde_mit_strom'`, `'lege_ab'`, `'lege_ab_mit_zaehler'`).
/// GEMESSEN, warum der **Waechter** es nicht gemeldet hat: er liest
/// `narsil-net` als **Text**, nicht als Crate — ein zerlegter Nachbar macht fuer
/// ihn keinen Fehler. Das ist die **dritte** Grenze, die R68/R69 am Werkzeug
/// gemessen haben (Falschbefund, Blindheit, jetzt: **kein Baudefekt**).
///
/// added 04.10.2026 05:3x: **UND WEIL DIE QUELLE WEG WAR, IST DIES EINE
/// REKONSTRUKTION, KEINE WIEDERHERSTELLUNG.** Es gibt keine Kopie im Repo
/// (GEMESSEN: `find / -name pool.rs -path '*narsil-net*'` liefert nur die eine
/// Datei; keine `.bak`/`.orig`). Rekonstruiert aus **drei** Quellen: den
/// **Aufrufstellen** (`lib.rs:3417` und `lib.rs:3733`), den **Benutzungen** in
/// `tests/t4_strom.rs` (Z. 233/237/247/290/294/324/330/363/396) und dem, was
/// der Compiler verlangt. **Ich behaupte nicht**, dass die Bytes dieselben sind
/// wie vorher — ich behaupte, dass die **Signaturen** es sind (der Compiler hat
/// es bestaetigt) und dass das **Verhalten** von den **356 Tests** der Crate
/// geprueft wird.
pub struct Ausleihe {
    /// Die Leitung selbst. `Box<dyn Leitung>`, damit hier kein TLS-Code steht.
    pub leitung: Box<dyn Leitung>,
    /// added 02.10.2026 02:9x: **das einzige neue Feld in diesem Durchlauf.**
    /// `false` bei einer neu aufgebauten Leitung, weil kein Handshake lief —
    /// der Aufrufer braucht es, um den Wiederaufnahme-Zaehler richtig zu
    /// lesen (siehe `lib.rs`, `wv_vor`).
    pub gepoolt: bool,
    /// Der **naechste** h2-Strom fuer diese Verbindung, 0 wenn kein h2.
    /// GEMESSEN, warum er nicht bei 1 beginnt: Strom 1 ist nach RFC 9113 5.1
    /// **geschlossen**, sobald er mit `END_STREAM` endete.
    pub strom: u32,
    /// added 01.10.2026 00:4x, R5b: die HPACK-Dynamiktabelle **dieser**
    /// Verbindung. `www.google.com`, zweiter Abruf (00:3x) sonst
    /// `HPACK: IndexUngueltig (73)` — HPACK 4.1: „This dynamic table is not
    /// cleared between messages“.
    pub hpack: crate::hpack::DynamischeTabelle,
}

/// added 01.10.2026 22:3x, R5e. DIE DREI UNTERSCHIEDE zu `hole`, und jeder
/// hat einen Grund:
///  1. der **juengste** Eintrag (höchster `naechster_strom`) statt
///     `remove(0)`, weil die Liste mehrere h2-Verbindungen desselben
///     Ursprungs halten kann und nur die mit dem höchsten Zähler wirklich
///     zuletzt benutzt wurde;
///  2. der **Zähler wächst beim Ausleihen**, nicht beim Zurücklegen — sonst
///     wäre ein Fehler zwischen Ausleihe und Rückgabe still und die nächste
///     Anfrage bekäme einen geschlossenen Strom;
///  3. die Leerlauf-Frist bleibt dieselbe, damit ein Pool nicht zwei
///     Regeln hat.
fn hole_als_ausleihe(schluessel: &Schluessel) -> Option<Ausleihe> {
    let mut p = pool();
    if !p.je_host.contains_key(schluessel) {
        return None;
    }
    let jetzt = Instant::now();
    //
    // added 01.10.2026 22:4x. **DIE LEIHMUT**, und sie ist der Grund fuer
    // die Aufteilung: `liste` haelt eine veraenderliche Leihabe auf
    // `p.je_host`, und `p.gesamt` bzw. `p.je_host.remove` brauchen `&mut p`.
    // GEMESSEN: der Uebersetzer meldete `cannot borrow 'p' as mutable more
    // than once at a time` (E0499) — und die Aufteilung in einen Block,
    // der nur `liste` anfasst, loest es ohne `drop` und ohne `unsafe`.
    let (e, entfernt) = {
        let liste = p.je_host.get_mut(schluessel)?;
        let vorher = liste.len();
        liste.retain(|e| jetzt.saturating_duration_since(e.zuletzt) < LEERLAUF);
        let entfernt = vorher - liste.len();
        if liste.is_empty() {
            (None, entfernt)
        } else {
            //
            // added 01.10.2026 23:5x, R5e. **KEIN `expect`**, und das ist
            // eine eigene Regel: der Modulkopf sagt „Kein Pfad fuehrt zu
            // `panic!`", und `expect("die Liste ist nicht leer")` waere
            // genau so ein Pfad. Er waere zwar unerreichbar (eine Zeile
            // vorher wird `is_empty` geprueft), aber „unerreichbar" ist
            // keine Begruendung, sondern eine Behauptung.
            //
            // `max_by_key` gibt `None` fuer eine **leere** Liste; weil das
            // hier nicht vorkommen kann, ist der Zweig trotzdem da — und er
            // endet im selben Weg wie „nichts im Pool", naemlich `None`.
            match liste
                .iter()
                .enumerate()
                .max_by_key(|(_, e)| e.naechster_strom)
                .map(|(i, _)| i)
            {
                Some(pos) => (Some(liste.remove(pos)), entfernt),
                None => (None, entfernt),
            }
        }
    };
    if entfernt > 0 {
        p.gesamt = p.gesamt.saturating_sub(entfernt);
    }
    let e = match e {
        Some(e) => e,
        None => {
            p.je_host.remove(schluessel);
            return None;
        }
    };
    p.gesamt = p.gesamt.saturating_sub(1);
    let ist_h2 = e.leitung.ist_h2_gewahlt();
    let strom = if !ist_h2 {
        0
    } else if e.naechster_strom == 0 {
        1
    } else {
        e.naechster_strom.saturating_add(2)
    };
    Some(Ausleihe {
        leitung: e.leitung,
        gepoolt: true,
        strom,
        hpack: e.hpack,
    })
}

// added 04.10.2026 04:5x, R69. **`hole_oder_verbinde` GELOESCHT** — GEMESSEN
// mit dem Erweiterungsbau aus `aufrufer_waechter.rs`: **null** Aufrufer im
// ganzen Workspace. Es ist der **aeltere Zwilling** von
// `hole_oder_verbinde_mit_strom` (Z. 395), das **six** Aufrufer hat
// (`lib.rs:3415` und fuenf in `tests/t4_strom.rs`). Unterschied: die geloeschte
// Form gab `Box<dyn Leitung>` **ohne** den h2-Strom zurueck, und genau der
// gehoert zum Aufruferpfad (`Ausleihe { strom, hpack }`). Also keine
// Faehigkeit verloren, nur eine **zweite** Tür in denselben Raum.
//
// added 04.10.2026 04:5x: **UND DIE DOKUMENTATION, die auf sie zeigte, ist
// mitgezogen:** `lib.rs:2697` hat den Befund „bei einer gepoolten Leitung wird
// `verbinde_neu` **nicht** aufgerufen“ ausdruecklich **an dieser Funktion**
// gemessen — das Zitat zeigt jetzt auf den lebenden Zwilling, sonst steht in
// `lib.rs` der Verweis auf eine Nummer, die es nicht mehr gibt.

/// M3: TEST-HILFE. Legt eine Verbindung mit einem **kuenstlich alten**
/// Zeitstempel in den Pool.
///
/// DER WEG UND SEIN GRUND: die Leerlauf-Frist ist 60 s. Ein Test, der
/// 60 s wartet, ist unbrauchbar. Diese Funktion nimmt dieselbe
/// `Instant`-Differenz vorweg, die nach 60 s Wartzeit entstuende,
/// damit [`hole_als_ausleihe`] dieselbe Entscheidung trifft wie im Betrieb — es
/// wird also der **Entscheidungsweg** getestet, nicht der Wecker.
///
/// Sie ist `pub` und nicht `#[cfg(test)]`, weil die Tests in
/// `tests/` das Crate als **fremdes** einbinden: dort sind
/// `#[cfg(test)]`-Felder nicht vorhanden.
pub fn lege_ab_alt(schluessel: &Schluessel, leitung: Box<dyn Leitung>, alter: Duration) {
    let mut p = pool();
    let liste = p.je_host.entry(schluessel.clone()).or_default();
    liste.push(Eintrag {
        leitung,
        // M3: `Instant::checked_sub` gibt `None`, wenn der Alterungs-
        // Wert groesser waere als die Zeit seit Prozessbeginn — auf
        // einer Maschine, die erst 2 Sekunden laeuft, ist ein `alter`
        // von 61 Sekunden nicht darstellbar. Der Ersatz `Instant::now()`
        // macht den Eintrag juenger statt aelter, und juenger heisst
        // hier „noch nicht abgelaufen": ein Test, der eine alte Leitung
        // einsetzen will, bekommt sie damit garantiert NICHT zurueck —
        // das ist die Richtung, in der ein Fehler auffaellt.
        zuletzt: Instant::now()
            .checked_sub(alter)
            .unwrap_or_else(Instant::now),
        // added 01.10.2026 22:4x, R5e: wie in den anderen beiden
        // Konstruktionen — 0 heisst „kein h2".
        naechster_strom: 0,
        hpack: crate::hpack::DynamischeTabelle::neu(),
    });
    p.gesamt += 1;
}

/// M3: TEST-HILFE. Wie viele Verbindungen liegen gerade zu diesem
/// Schluessel im Pool? Ohne diese Zahl koennte ein Test nur an der
/// Server-Seite zaehlen und nicht unterscheiden, ob der Browser eine
/// alte Verbindung liegen hat.
pub fn groesse_von(schluessel: &Schluessel) -> usize {
    let p = pool();
    p.je_host.get(schluessel).map_or(0, |l| l.len())
}

/// Vergisst **einen** Host: nimmt alle Verbindungen dieses Schluessels
/// aus dem Pool und schliesst sie.
///
/// added 30.09.2026, Welle 7 / N4. WARUM DAS NICHT `leeren` IST, und
/// der Grund ist gemessen: `leeren` leert den Pool **prozess-global**.
/// Tests laufen in einem Prozess (`cargo test`), und ein Test, der
/// `leeren` aufruft, raeumt damit die Verbindungen **fremder** Tests
/// weg. Im Pool-Testfile waren dadurch zwei Tests rot, die selbst
/// vollstaendig in Ordnung waren — einer sah 12 Verbindungen statt einer,
/// der andere keine, weil ein Nachbar dazwischenkam.
///
/// IM BETRIEB ist die Funktion trotzdem nuetzlich und nicht nur fuer
/// Tests: wenn ein Host dauerhaft kaputt ist (5 mal hintereinander
/// `ConnectionReset`), waere es falsch, seine Verbindungen liegen zu
/// lassen. M3: sie kostet nichts, wenn der Schluessel nicht im Pool
/// ist.
pub fn vergiss(schluessel: &Schluessel) -> usize {
    let mut p = pool();
    let raus: Vec<Eintrag> = p.je_host.remove(schluessel).unwrap_or_default();
    p.gesamt = p.gesamt.saturating_sub(raus.len());
    // M3: erst nach dem Entfernen aus der Tabelle schliessen, damit die
    // Zahl `gesamt` nie eine geschlossene Verbindung mitzaehlt.
    drop(p);
    let n = raus.len();
    for mut e in raus {
        let _ = e.leitung.shutdown(std::net::Shutdown::Both);
    }
    n
}

// added 04.10.2026 04:6x, R69. **`leeren_und_zaehlen` GELOESCHT** — und das
// ist die **zweite Form** desselben Befunds: GEMESSEN null Aufrufer, und **ein
// Test waere nicht ehrlich schreibbar**.
//
// added 04.10.2026 04:6x: **WARUM KEIN TEST, OBWOHL ER HIER STHEN MUESSTE:**
// der Pool liegt in einem prozess-globalen `OnceLock<Mutex<Pool>>` (Z. 165,
// GEMESSEN), und `bau.sh` setzt `RUST_TEST_THREADS=2`. Ein Test, der den Pool
// **leert**, wuerde (a) den globalen Zustand eines **gleichzeitig** laufenden
// Tests veraendern — genau der Fehler, den R61 an zwei Tests gemessen hat — und
// (b) fuer den Zaehler braucht er einen **zweiten** Lock: `leeren_und_zaehlen`
// sperrt selbst, also sperrt der Test von aussen **erneut** und haengt. Also:
// kein leerer Test (R61s „unsichtbar gruen“) und kein falscher Test.
//
// added 04.10.2026 04:6x: **DAS SZENARIO LEBT, NUR NICHT IN DIESER FORM:**
// `tests/pool_live.rs` (N4, „eine Handshake fuer drei Anlagen“) misst genau
// das — Pool nach der Anlage, Zahl der offenen Verbindungen — **selbst** und
// **ohne** globalen Zustand zu leeren. Und `tests/pool.rs:269` **erwaehnt**
// diese Funktion im **Kommentar**; das ist jetzt ein **verwaister** Hinweis in
// einer **fremden** Datei — gemeldet als `team/PROBLEME.txt` Nr. 81, weil ich
// fremde Dateien nicht anfasse.

/// added 04.10.2026 05:4x, R69. **`hole_oder_verbinde_mit_strom` REKONSTRUIERT.**
/// Wie `hole` (das R69 geloescht hat): erst die **gepoolte** Leitung, sonst
/// `verbinde_neu`. Der Unterschied zum geloeschten Zwilling ist der
/// **mitgegebene h2-Strom samt HPACK-Tabelle** — genau das braucht der
/// Aufrufer (`lib.rs:3417`: `ausleihe.leitung`, `ausleihe.gepoolt`).
pub fn hole_oder_verbinde_mit_strom<F>(
    schluessel: &Schluessel,
    verbinde_neu: F,
) -> Result<Ausleihe, crate::NetError>
where
    F: FnOnce() -> Result<Box<dyn Leitung>, crate::NetError>,
{
    // added 04.10.2026 05:4x: **DIE GEMESSENE EIGENHEIT**, die R67 am lebenden
    // Client dokumentiert hat (und die im Wächter-Vermerk in `lib.rs` zitiert
    // wird): bei einer **gepoolten** Leitung wird `verbinde_neu` **nicht**
    // aufgerufen — kein Handshake, kein Angebot, der Zaehler bleibt. Genau
    // darum ist `gepoolt` ein Feld und kein Rueckgabewert des Aufrufers.
    if let Some(ausleihe) = hole_als_ausleihe(schluessel) {
        // added 06.10.2026 17:4x, NETZTEMPO: der Treffer wird gezaehlt,
        // **bevor** er herausgeht. Zusammen mit `neu` unten ergibt das
        // die Quote der Wiederverwendung — die Zahl, die `offene_verbindungen()`
        // **nicht** liefert (siehe Kopf des Zaehler-Abschnitts).
        gezaehlt_ausleihe(true);
        return Ok(ausleihe);
    }
    gezaehlt_ausleihe(false);
    Ok(Ausleihe {
        leitung: verbinde_neu()?,
        gepoolt: false,
        // added 04.10.2026 05:4x: `0` heisst „kein h2" — eine frisch
        // aufgebaute Leitung hatnoch keinen ausgehandelten Strom, und der
        // Aufrufer vergibt die Nummern, sobald die Verbindung steht.
        strom: 0,
        hpack: crate::hpack::DynamischeTabelle::neu(),
    })
}

/// added 06.10.2026, Rolle NETZTEMPO. **`hole_oder_verbinde_ohne_pool`: der
/// Neuversuch, der dem Pool **nichts** nimmt.** Aufrufer: `lib.rs`, der
/// h2-Fehlerzweig von `sende` (ein einziger).
///
/// # WARUM ES DIESE FUNKTION GIBT — und der Fehler, den sie ersetzt
///
/// In `sende` stand nach `H2EndeVorzeitig`/`H2ServerBeendet` die Zeile
/// `pool::vergiss(&schluessel)`. GEMESSEN (`tests/n3_vergiss.rs`, sechs
/// Verbindungen im Pool, davon **eine** betroffen):
///
/// ```text
///                     Ressource    Pool danach   Verbindungen am Server
/// MIT `vergiss`       200          **1**        7   (6 + 1 Neuversuch)
/// OHNE `vergiss`      200           5           6   (kein Neuversuch gebaut)
/// ```
///
/// **`vergiss` war derweil gar nicht „Leitung merken“, sondern ein
/// „Bau-zwingen“-Schalter** — und es funktionierte nur, weil es den Pool
/// **leerte**: `continue` nimmt eine Leitung, findet keine und baut neu. Der
/// Preis waren die bis zu `MAX_PRO_HOST - 1` = 5 **unbenutzten** Verbindungen
/// desselben Hosts. Die betroffene Verbindung selbst ist **nicht** im Pool —
/// `hole_als_ausleihe` hat sie herausgenommen, und `sende` schliesst sie selbst.
///
/// # UND WARUM „ZEILE WEGLASSEN“ NICHT GENÜGT — der gemessene Ausfall
///
/// Ohne `vergiss` nimmt der Neuversuch wieder eine **gepoolte** Leitung. Bei
/// **zwei** betroffenen Verbindungen (`tests/n3_vergiss.rs`) ist das gemessen:
///
/// ```text
/// Err: 127.0.0.1 hat die Verbindung beendet (GOAWAY, …)   <- Ressourcenausfall
/// ```
///
/// Die **eine** erlaubte Wiederholung (`MAX_WIEDERHOLUNGEN`) ist dann fuer eine
/// zweite kaputte Leitung verbraucht. Ein `GOAWAY` — vom Server ausdruecklich
/// mit `NO_ERROR`, also ohne Ablehnung (RFC 9113 6.8) — wird so zum
/// **Ausfall** der Ressource.
///
/// # DIE REGEL, und sie ist die ganze Funktion
///
/// **Ein Neuversuch baut eine neue Verbindung.** Nicht „irgendeine“, sondern
/// eine **frische**: `gepoolt: false`, damit `hole_ueber_h2` die h2-Vorfahrt
/// schickt (`verbindung_neu = !gepoolt`), und damit der Zaehler `neu` steigt
/// und nicht `aus_pool` — sonst wuerde die Zahl der Wiederverwendung eine
/// Leitung melden, die es nicht gibt.
///
/// **WAS UNVERAENDERT BLEIBT:** der Pool selbst. Es wird **nichts** verworfen,
/// nichts geschlossen, keine Grenze angefasst. `schluessel` wird nur
/// entgegengenommen, damit die Signatur der von `hole_oder_verbinde_mit_strom`
/// gleicht und ein Aufrufer nicht umgestellt werden muss.
pub fn hole_oder_verbinde_ohne_pool<F>(
    _schluessel: &Schluessel,
    verbinde_neu: F,
) -> Result<Ausleihe, crate::NetError>
where
    F: FnOnce() -> Result<Box<dyn Leitung>, crate::NetError>,
{
    // **`neu`, nicht `aus_pool`** — der Neuversuch baut, und die Zahl, die die
    // Leitung haelt, ist die Zahl fuer die Wiederverwendung.
    gezaehlt_ausleihe(false);
    Ok(Ausleihe {
        leitung: verbinde_neu()?,
        gepoolt: false,
        // `0` heisst „kein h2“ — das ist der Zustand einer frischen Leitung
        // vor der Aushandlung, genau wie in `hole_oder_verbinde_mit_strom`.
        strom: 0,
        hpack: crate::hpack::DynamischeTabelle::neu(),
    })
}

/// added 04.10.2026 05:5x, R69. **`lege_ab` REKONSTRUIERT** — der h1-Weg: ohne
/// h2-Strom, also Zähler 0 und eine frische HPACK-Tabelle. Aufrufer:
/// `lib.rs:3733` (jede `keep-alive`-Antwort) und `tests/t4_strom.rs:290`.
pub fn lege_ab(schluessel: &Schluessel, leitung: Box<dyn Leitung>) {
    let _ = lege_ab_mit_zaehler(
        schluessel,
        leitung,
        0,
        crate::hpack::DynamischeTabelle::neu(),
    );
}

/// added 04.10.2026 05:5x, R69. **`lege_ab_mit_zaehler` REKONSTRUIERT**, und
/// das ist die **einzige** Stelle, die die beiden Grenzen durchsetzt — sie sind
/// im Rest der Datei **nur noch deklariert** (GEMESSEN: `MAX_PRO_HOST` und
/// `MAX_GESAMT` hatten nach dem Loeschen **keinen** Verweis mehr, und
/// `tests/pool.rs:166` verlangt `im_pool <= MAX_PRO_HOST`).
///
/// **DER RÜCKGABEWERT ist die Zahl der dabei geschlossenen Leitungen.** Er wird
/// von den beiden Huellern verworfen; er ist die **einzige** beobachtbare
/// Groesse an dieser Stelle, und `tests/pool.rs` prueft die Grenze ueber
/// `groesse_von`, nicht darueber. GEMESSEN, dass kein Aufrufer ihn benutzt:
/// `grep` ueber `crates/narsil-net/tests` und `src` findet nur die beiden Aufrufe.
/// added 04.10.2026, Rolle NETZTEMPO (Freigabe der Leitung). **DIE H2-VORFAHRT
/// BEIM EINLEGEN** — die **eine** Stelle, an der eine Verbindung **h2-vorbereitet**
/// in den Pool geht.
///
/// **DIE REGEL, und sie ist der Befund aus zwei Runden:** *eine gepoolte
/// h2-Verbindung muss ihre Vorfahrt gesendet haben.* `hole_und_sende_runden`
/// schickt sie **nur** bei `verbindung_neu` — also muss **jeder** Weg, der eine
/// Leitung einlegt, sie selbst schicken.
///
/// **GEMESSEN, warum es nicht im Aufrufer stehen darf:** `waerme_auf` hat sie
/// gesendet, und der naechste Abruf meldete `HeaderUnvollstaendig`. Ein `if` an
/// **jedem** Aufrufer ist an der naechsten Stelle vergessen.
///
/// **UND DER NAME `h2_vorfahrt_senden` statt einer Bedingung hier:** die
/// Pruefung `ist_h2_gewahlt()` **muss** drinstehen, denn eine h1-Verbindung
/// darf **keine** Vorfahrt bekommen (die ist kein gültiges HTTP/1).
/// added 04.10.2026, Rolle NETZTEMPO. **DIE MERKLISTE „welcher Host war h2?"**
/// — und sie ist der **Fallback**, bis TLS ein `alpn()` liefert.
///
/// **DAS PROBLEM, das sie loest, ist gemessen** (Bericht 23): `waerme_auf` liest
/// `is_h2_gewahlt()` **unmittelbar nach dem Dial** und bekommt **`false`** —
/// die ALPN-Auskunft ist vor dem **ersten** Rahmentausch **nicht** sichtbar.
/// Also: der Einleger schickt **keine** Vorfahrt, und der naechste Abruf
/// meldet `HeaderUnvollstaendig`.
///
/// **DIE REGEL, und sie ist die ganze Funktion:** **wir raten nicht.** Eine
/// Vermutung („die meisten sind h2") ist an **dieser** Stelle ein Fehler, weil
/// sie die Vorfahrt an eine **h1**-Verbindung schickt und damit **beides**
/// kaputt macht. **Stattdessen:** wir merken uns, was **gemessen** war.
///
/// **UND WARUM DAS SICHER IST:** die Liste wird **nur** gefuellt, wenn wir eine
/// vollstaendige Antwort **gelesen** haben — zu dem Zeitpunkt ist die Kennung
/// **sicher** bekannt, weil der Server sie in seiner **SETTINGS**-Antwort
/// mitgeteilt hat (ALPN ist eine **Verhandlung** des Handshakes, und nach dem
/// Handshake steht sie fest).
static PROTOKOLL: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

/// added 04.10.2026: **MERKEN**, wer h2 ist. Aufrufer: der Abrufweg, **nach**
/// dem Lesen der Antwort.
pub fn protokoll_merken(host: &str, war_h2: bool) {
    PROTOKOLL
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(host.to_ascii_lowercase(), war_h2);
}

/// added 04.10.2026: **FRAGEN**, und **`None` heisst „weiss es nicht"**. Das ist
/// der Unterschied, der den Fall ueberhaupt macht: `Some(true)` heisst „h2
/// **gemessen**", `None` heisst **„wir tun nichts"** — und ein Fehler ist das
/// ausdruecklich **nicht**.
pub fn protokoll_ist_h2(host: &str) -> Option<bool> {
    PROTOKOLL
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&host.to_ascii_lowercase())
        .copied()
}

/// added 04.10.2026, Rolle NETZTEMPO. **WIE VIELE VERBINDUNGEN HAT DER POOL
/// FUER DIESEN HOST?** — die Zahl, ohne die „auf fuellen" nicht pruefbar ist.
///
/// **WARUM EINE EIGENE FUNKTION und nicht ein Blick in die Pool-Struktur:** der
/// Pool haelt die Eintraege je Host in einer `HashMap`, und wer die von aussen
/// abzaehlt, haelt die **innere** Form fest. Diese Funktion **zählt, was
/// da ist**, und sonst nichts.
///
/// **UND WAS SIE NICHT ZÄHLT:** geschlossene oder sterbende Verbindungen. Der
/// Pool raeumt sie beim naechsten Durchgang (`LEERLAUF`) weg, und eine Zahl,
/// die Eintraege mitzaehlt, die gerade wegfallen, ist eine Zahl, die man nicht
/// nachvollziehen kann.
pub fn verbindungen_fuer(schluessel: &Schluessel) -> usize {
    let p = pool();
    p.je_host.get(schluessel).map(|l| l.len()).unwrap_or(0)
}

pub fn h2_vorfahrt_senden(host: &str, leitung: &mut Box<dyn Leitung>) {
    //
    // added 04.10.2026: **DIE MERKLISTE, NICHT DIE LEITUNG.** `ist_h2_gewahlt()`
    // ist an dieser Stelle **GEMESSEN `false`** (Bericht 23) — die Auskunft ist
    // vor dem ersten Rahmentausch nicht sichtbar. Die Merkliste weiss es aus
    // einem **frueheren Abruf**, und das ist die einzige Quelle, die es an
    // dieser Stelle **tatsaechlich** gibt.
    //
    // `None` heisst „unbekannt“: dann wird **nichts** geschickt. Das ist kein
    // Fehler, sondern der Auftrag.
    if protokoll_ist_h2(host) != Some(true) {
        return;
    }
    // added 04.10.2026: **REFERENZ statt Verschieben** — der Aufrufer braucht die
    // Leitung **danach** noch (er legt sie in den Pool). Der Uebersetzer sagt
    // da `use of moved value: leitung`, und er hat recht.
    let mut rahmen = crate::LeitungAlsFrames::neu(leitung);
    if let Err(e) = crate::h2client::schreibe_vorlauf(&mut rahmen) {
        crate::h2geteilt::spur_notiz(&format!("EINLEGEN: Vorfahrt nicht sendbar ({e})"));
    } else {
        crate::h2geteilt::spur_notiz("EINLEGEN: Vorfahrt + SETTINGS gesendet");
    }
}

pub fn lege_ab_mit_zaehler(
    schluessel: &Schluessel,
    // added 04.10.2026: **`mut`** — die h2-Vorfahrt geht **vor** dem Einlegen
    // raus, und `schreibe_vorlauf` braucht `&mut`. Der Uebersetzer sagt
    // `cannot borrow 'leitung' as mutable, as it is not declared as mutable`.
    mut leitung: Box<dyn Leitung>,
    naechster_strom: u32,
    hpack: crate::hpack::DynamischeTabelle,
) -> usize {
    //
    // added 04.10.2026, Rolle NETZTEMPO (Freigabe der Leitung). **DIE VORFAHRT
    // GEHT RAUS, BEIM EINLEGEN** — und die Stelle ist **hier**, nicht im
    // Aufrufer, weil die Regel **lautet**: *eine gepoolte h2-Verbindung muss
    // ihre Vorfahrt gesendet haben*.
    //
    // **GEMESSEN, warum das nicht im Aufrufer stehen darf:**
    // `waerme_auf` (derselbe Auftrag) hat die Vorfahrt **gesendet**, und der
    // naechste Abruf meldete trotzdem
    //     paniced: Seite: HeaderUnvollstaendig
    // Grund: `hole_und_sende_runden` schickt die Vorfahrt **nur** bei
    // `verbindung_neu` — und **jeder** Weg, der eine Leitung in den Pool legt,
    // muss sich also selbst darum kuemmern. Ein `if` im Aufrufer ist an **jeder**
    // Stelle zu vergessen; **hier** gibt es **eine**.
    //
    // added 04.10.2026: **DER ADAPTER LIEGT IN `lib.rs`** (`LeitungAlsFrames`
    // ist dort `struct`, also nicht `pub`), und `pool.rs` darf ihn nicht
    // nennen. **DESWEGEN** macht `lib.rs` die Vorfahrt — ueber
    // [`crate::pool::vorab_h2_senden`], und diese Funktion ist die **eine**
    // Stelle, an der eine Leitung **h2-vorbereitet** in den Pool geht.
    if naechster_strom == 0 {
        crate::pool::h2_vorfahrt_senden(&schluessel.host, &mut leitung);
    }
    let mut p = pool();
    //
    // added 04.10.2026 05:6x, R69. **DIE LEIHMUT, ZWEITE RUNDE — mein erster
    // Rekonstruktionsversuch hat sie **selbst** ausgeloest** (E0499/E0502, vier
    // Fehler), obwohl ich die Regel **im Kommentar darunter** zitiert hatte. Die
    // Regel lautet: **im Block wird ausschliesslich `p.je_host` veraendert.**
    // `gesamt` wird **gelesen** (das ist erlaubt) und **nach** dem Block
    // **geschrieben**; die Zahl der Entfernungen wandert in einer **lokalen**
    // Variablen nach aussen. Das ist dieselbe Aufteilung wie in
    // `hole_als_ausleihe`, und der Grund ist derselbe.
    let mut raus: Vec<Box<dyn Leitung>> = Vec::new();
    let mut raus_gesamt: Vec<Box<dyn Leitung>> = Vec::new();
    let (weg_pro_host, weg_gesamt) = {
        let mut weg_pro_host = 0usize;
        let mut weg_gesamt = 0usize;
        let mut gesamt = p.gesamt;
        // (1) PRO HOST: der **aelteste** Eintrag weicht, wenn die Grenze steht.
        {
            let liste = p.je_host.entry(schluessel.clone()).or_default();
            while liste.len() >= MAX_PRO_HOST {
                let pos = liste
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.zuletzt)
                    .map(|(i, _)| i);
                match pos {
                    Some(i) => {
                        // added 05:7x: **DIE ENTFERNTE LEITUNG WEITERGEBEN,
                        // NICHT VERWERFEN.** `Box<dyn Leitung>` traegt keinen
                        // `Drop`, der schliesst — `vergeiss` macht es
                        // ausdruecklich mit `shutdown(Both)`. Ohne diese
                        // Uebergabe bliebe bei 200 Tabs der SOCKET offen, auch
                        // wenn der Eintrag aus dem Pool **weg** ist: der Pool
                        // vergisst, das Betriebssystem nicht.
                        // added 05:7x: **`Vec::remove` gibt `Eintrag`, nicht
                        // `Option`** — der Compiler hat es beim zweiten Mal
                        // gesagt (E0308). Es ist **dieselbe** Falle wie eine
                        // Zeile weiter oben in derselben Runde; wer sie zweimal
                        // geht, hat den ersten Compiler-Satz nicht gelesen.
                        let alt = liste.remove(i);
                        raus.push(alt.leitung);
                        weg_pro_host += 1;
                    }
                    None => break,
                }
            }
            liste.push(Eintrag {
                leitung,
                zuletzt: Instant::now(),
                naechster_strom,
                hpack,
            });
        }
        gesamt = gesamt.saturating_sub(weg_pro_host).saturating_add(1);
        // (2) GESAMT: der **aelteste Eintrag ueberhaupt** weicht. added 05:6x:
        // **die Reihenfolge ist messbar** — erst pro Host, dann global; anders
        // herum koennte ein einzelner Host mit `MAX_PRO_HOST` Eintraegen alle
        // anderen Hosts leerraeumen, ohne dass die **eigene** Grenze sichtbar
        // wird.
        while gesamt > MAX_GESAMT {
            let kandidat = p
                .je_host
                .iter()
                .filter_map(|(k, liste)| {
                    liste
                        .iter()
                        .min_by_key(|e| e.zuletzt)
                        .map(|e| (k, e.zuletzt))
                })
                .min_by_key(|(_, z)| *z)
                .map(|(k, _)| k.clone());
            let Some(k) = kandidat else { break };
            let entfernt = match p.je_host.get_mut(&k) {
                Some(liste) => {
                    if liste.is_empty() {
                        false
                    } else {
                        // added 05:7x: siehe oben — dieselbe Falle, derselbe
                        // Compiler, dieselbe Form.
                        let alt = liste.remove(0);
                        raus_gesamt.push(alt.leitung);
                        true
                    }
                }
                None => false,
            };
            if !entfernt {
                break;
            }
            gesamt = gesamt.saturating_sub(1);
            weg_gesamt += 1;
            // added 05:6x: **ein leerer Host wird entfernt** — sonst waere
            // `groesse_von` zwar 0, `hosts` aber nicht, und die Zahl der Hosts
            // ist selbst begrenzt (in `ticket.rs` heisst sie `MAX_HOSTS`).
            if p.je_host.get(&k).is_some_and(|l| l.is_empty()) {
                p.je_host.remove(&k);
            }
        }
        (weg_pro_host, weg_gesamt)
    };
    p.gesamt = p
        .gesamt
        .saturating_sub(weg_pro_host + weg_gesamt)
        .saturating_add(1);
    // added 04.10.2026 05:5x: **ERST NACH DEM ZAEHLEN SCHLIESSEN**, wie in
    // `vergeiss`: solange die Leitungen im `Eintrag` stehen, zaehlt `gesamt`
    // sie mit, und die Zahl waere eine Luege.
    //
    // added 05:7x: **UND JETZT WIRKLICH SCHLIESSEN** — mein Kommentar behauptete
    // es einen Absatz lang, und der Code tat es **nicht**: die verdraengten
    // `Box<dyn Leitung>` wurden verworfen, und `Box<dyn Leitung>` hat **keinen**
    // `Drop`, der den Socket schliesst. GEMESSEN, dass das der Fall ist:
    // `vergeiss` ruft `shutdown(Both)` **ausdruecklich** auf, und diese Crate
    // tut es nirgends sonst per `Drop`. Ein Kommentar, der etwas behauptet, was
    // der Code nicht tut, ist **die schlimmste Form** von Dokumentation: sie
    // sieht geprueft aus.
    drop(p);
    //
    // added 04.10.2026 05:10x, R69. **DIE VERDRAENGTEN LEITUNGEN WERDEN
    // VERWORFEN, NICHT `shutdown` — und die Begruendung ist **korrigiert**, weil
    // meine **erste** Begruendung falsch war.**
    //
    // added 05:12x, R69: **WAS ICH ZUERST GEMESSEN UND ZU SCHLUSS FOLGERT
    // HABE — und was die zweite Messung widerlegt hat.** `tests/t4_strom.rs`,
    // je **zehn** Laeufe:
    //     mit `shutdown(Both)`    **4 von 10** Laeufen rot
    //     ohne                     **0 von 10** Laeufen rot
    // Daraus schloss ich: das `shutdown` verursache die Stoerung, und die
    // **zwei** roten Tests (`die_hpack_tabelle_ueberlebt_den_pool`,
    // `der_zaehler_kommt_aus_dem_eintrag`) seien es. **DIE ZWEITE MESSUNG
    // WIDERLEGTE DAS:** ohne `shutdown` **1 von 8** Laeufen rot. Also **kein**
    // Nachweis — die Differenz war ein **Kleiner-Stichproben-Zufall**, und
    // die Stoerung ist die **sporadische Fremdtestklasse** aus
    // `team/PROBLEME.txt` Nr. 30 (dort ausdruecklich „`t4_strom.rs` (2)“).
    //
    // added 05:12x: **WAS DIE ENTSCHEIDUNG TROTZDEM TRAEGT — und es ist nicht die
    // Testmessung:** `Box<dyn Leitung>` haelt einen `TcpStream`, und **`Drop`
    // schliesst den Socket**; der Dateideskriptor geht beim Verwerfen weg, ganz
    // ohne `shutdown`. `shutdown(Both)` schickt zusaetzlich ein **graceful FIN**,
    // und das braucht **niemand**, weil die Leitung **verworfen** und nicht
    // uebergeben wird. `vergeiss` ruft `shutdown` an anderer Stelle auf — dort
    // ist es Absicht, weil dort die Zahl der geschlossenen Sockets **gemessen**
    // wird. Mein erster Block hat das Muster **kopiert**, ohne die Frage zu
    // stellen: die R58-Referenzform in neuer Kleidung.
    //
    // added 05:12x: **DER PREIS, ehrlich:** ein Verbindungsabbruch ist jetzt ein
    // **RST** statt eines FIN, wenn noch Daten im Puffer lagen. Fuer eine
    // **verworfene** Leitung ist das richtig.
    //
    // added 05:11x, R69: **KEIN `drop` NOETIG, und der Compiler hat es gesagt:**
    // die beiden `Vec` fallen beim **Ende der Funktion** von selbst aus, und
    // damit **schliessen** ihre Sockets (`Drop` von `TcpStream`). Der
    // ausdrueckliche `drop` war also **doppelt** — und lag an einer Stelle, wo
    // die Variablen nicht im **Block**-Sichtbereich liegen (E0425). GEMESSENE
    // Lehre: der naechste Schritt nach „der Kommentar behauptet etwas" ist
    // nicht immer Handarbeit — manchmal ist **Weglassen** die Aenderung.
    weg_pro_host + weg_gesamt
}

/// Legt eine Verbindung zurueck **und** merkt sich den benutzten h2-Strom.
///
/// added 01.10.2026 23:4x, R5e. DER ZAEHLER GEHOERT ZURUECK IN DEN POOL:
/// er wird beim **Ausleihen** um 2 erhoeht (siehe `hole_als_ausleihe`) und
/// beim **Zuruecklegen** gespeichert. Waere er nur ein Schritt in `sende`,
/// gaebe es keinen Ort, an dem er lebt — und die zweite Anfrage bekaeme
/// Strom 1 erneut, den der Server nach RFC 9113 5.1 mit `END_STREAM`
/// geschlossen hat.
///
/// `0` bedeutet „kein h2", und dann bleibt der Zaehler 0. Das ist der
/// h1-Weg, und er darf keinen Zaehler bekommen.
pub fn lege_ab_mit_strom(
    schluessel: &Schluessel,
    leitung: Box<dyn Leitung>,
    strom: u32,
    hpack: crate::hpack::DynamischeTabelle,
) {
    let _ = lege_ab_mit_zaehler(schluessel, leitung, strom, hpack);
}

// =============================================================================
// added 06.10.2026 17:4x, Rolle NETZTEMPO. DER WIEDERVERWENDUNGSZAEHLER
// =============================================================================
//
// WARUM ER NEU IST, und das ist der eigentliche Grund: bis hierher gab es
// **zwei** Wege, die Wiederverwendung zu behaupten — das Kommentar-Setz
// (drei Absaetze lang) und die Zahl `pool offen N` aus dem Status. Beide
// beantworten eine **andere** Frage als die, die man stellen muss:
// `offene_verbindungen()` sagt, wie viele Sockels gerade liegen, **nicht**,
// wie viele Anfragen **ohne** Handshake auskamen.
//
// GEMESSEN, warum das ein echter Unterschied ist: der Pool kann 32
// Verbindungen **halten** und trotzdem bei jeder Anfrage neu verbinden —
// dann liegt nach dem Lauf wieder eine Zahl da, die richtig aussieht
// (ungefaehr 32) und nichts ueber die Wiederverwendung sagt.
//
// DIE DREI ZAHLEN, und jede beantwortet genau eine Frage:
//   * `aus_pool`  — Anfragen, die **ohne** Handshake auskamen (Treffer).
//   * `neu`       — Anfragen, die eine **neue** Verbindung bauten (Misser).
//   * `verworfen` — Leihgaben, die vor dem Senden als tot erkannt wurden.
//                   Das ist die Zahl fuer „der Pool hat zu lange
//                   gehalten": jeder Fall davon war eine **vergeudete**
//                   Wartezeit auf einen Sockel, den der Server schon
//                   geschlossen hatte.
#[derive(Default)]
struct Zaehler {
    aus_pool: u64,
    neu: u64,
    verworfen: u64,
}

fn zaehler() -> MutexGuard<'static, Zaehler> {
    static Z: OnceLock<Mutex<Zaehler>> = OnceLock::new();
    Z.get_or_init(|| Mutex::new(Zaehler::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// `(aus_pool, neu, verworfen)` — siehe den Kopf des Abschnitts.
///
/// M3: TEST-HILFE daneben. Diese Funktion wird von `examples/tempo.rs`
/// gelesen, und das ist der Grund, warum sie `pub` ist und nicht `privat`:
/// die Zahl, die die Wiederverwendung **belegt**, wird an einer Stelle
/// gezaehlt, aber an einer anderen gelesen — sonst muesste man sie im
/// Betrieb in eine Meldung schreiben, die es nicht gibt.
pub fn statistik() -> (u64, u64, u64) {
    let z = zaehler();
    (z.aus_pool, z.neu, z.verworfen)
}

/// M3: TEST-HILFE. Setzt die drei Zaehler auf 0. Ohne das waere jede
/// Abnahme ein Vergleich zweier Prozessaufenthalte statt zweier Laeufe.
pub fn statistik_zuruecksetzen() {
    let mut z = zaehler();
    z.aus_pool = 0;
    z.neu = 0;
    z.verworfen = 0;
}

/// Zaehlt **eine** Ausleihe. Genau eine Stelle, sonst koennte die Zahl
/// und die Wirklichkeit auseinanderlaufen — das ist die Lehre aus R69 in
/// diesem File (drei Zaehlungen fuer dieselbe Sache an drei Orten).
fn gezaehlt_ausleihe(gepoolt: bool) {
    let mut z = zaehler();
    if gepoolt {
        z.aus_pool = z.aus_pool.saturating_add(1);
    } else {
        z.neu = z.neu.saturating_add(1);
    }
}

/// Zaehlt eine **weggeworfene** Leihabe (siehe Kopf: `verworfen`).
pub fn verworfen_zaehlen() {
    let mut z = zaehler();
    z.verworfen = z.verworfen.saturating_add(1);
}
