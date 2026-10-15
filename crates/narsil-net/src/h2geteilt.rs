//! added 03.10.2026, Rolle NETZTEMPO. **EINE h2-VERBINDUNG, VIELE ANFRAGEN.**
//!
//! # DER BEFUND, DER DIESES MODUL AUSLOEST
//!
//! GEMESSEN am 03.10.2026, 20 echte Stylesheets von **einem** Host
//! (`github.githubassets.com`), ueber `parallel::hole_alle`, HTTP-Cache aus,
//! Pool leer:
//!
//! ```text
//! Lauf 1: tls_n=6 tls=6166,0 ms tcp_n=6 Wanduhr=1845,2 ms  20 von 20 geholt
//! Lauf 2: tls_n=6 tls=5947,8 ms tcp_n=6 Wanduhr=1779,5 ms  20 von 20 geholt
//! ```
//!
//! **SECHS TLS-Handshakes fuer EINEN Host** — genau `MAX_GESAMT_LAUFEND`,
//! und je rund 1000 ms. Ueber **h2** haette **eine** Verbindung alle 20
//! Anfragen als 20 Stroeme tragen; HTTP/1.1 braucht sechs, weil dort nur
//! eine Anfrage pro Verbindung ohne Pipelining laeuft.
//!
//! # WARUM ES SO KAM, und das ist der eigentliche Fund
//!
//! `pool::hole_als_ausleihe` **nimmt** die Verbindung aus dem Pool heraus
//! (`liste.remove(pos)`) und gibt sie erst nach der Antwort wieder ab
//! (`lege_ab_mit_strom`). Solange sie herausgenommen ist, findet die naechste
//! Anfrage **dasselbe** Host **nichts** im Pool und baut eine **neue**
//! Verbindung. Bei 20 gleichzeitigen Anfragen heisst das: 6 Handshakes.
//!
//! **Die Regel des Pools ist also richtig fuer HTTP/1.1 und falsch fuer
//! HTTP/2** — und sie steht in `pool.rs` **ohne** diese Unterscheidung. Das
//! ist die Luecke, die dieses Modul fuellt: eine h2-Verbindung, die **waehrend**
//! der Anfragen im Pool bleibt und viele Stroeme traegt.
//!
//! # DER BAU, in vier Teilen, und jeder hat einen Grund
//!
//! 1. **Eine geteilte Verbindung** ([`Geteilte`]) haelt Sockel, Rahmenvorrat,
//!    HPACK-Tabelle und alle Stroeme unter **einem** `Mutex`. Es ist **ein**
//!    Zustand und damit **eine** Sperre; zwei Sperren haetten eine
//!    Sperrenreihenfolge, die man pruefen muss.
//! 2. **Ein Leser, viele Warter** (`pumpe` + `anfrage`): Rahmen duerfen nur
//!    **in Eingangsreihenfolge** dekodiert werden (HPACK 4.1 — die dynamische
//!    Tabelle ist ein Zustand ueber die Verbindung). Also liest immer nur
//!    einer, und die Frames, die nicht sein Strom sind, wandern in die
//!    Warteschlange des jeweiligen Stromes.
//! 3. **Fuehrerschaft ueber `try_lock`**: wer gerade liest, pumpt; die anderen
//!    schlafen auf derBedingungsvariable. Ohne das wuerden zwei Threads
//!    gleichzeitig aus demselben Sockel lesen — und **das** ist der Fehler,
//!    der stale Frames erzeugt, nicht ein Performanceproblem.
//! 4. **Eine Verwaltung der geteilten Verbindungen** je (Schema, Host, Port),
//!    mit den Zustaenden `Baut` und `Fertig`. `Baut` ist der eigentliche
//!    Hebel: die fuenf Threads, die sonst **fuenf eigene Handshakes**
//!    starten, warten auf **einen** und benutzen ihn dann mit.
//!
//! # DIE ZAHLEN, die dieses Modul belegt (nicht behauptet)
//!
//! * `stroeme()` — wie viele Stroeme ueber **eine** Verbindung gelaufen sind.
//! * `tls_n` aus `phasen_summen()` — die Handshakes. Vorher 6, nachher 1.
//!
//! # WAS **NICHT** BEHAUPTET WIRD
//!
//! * **Nicht**, dass es schneller wird. GEMESSEN ist bisher nur `tls_n`; die
//!   Wanduhr steht im Bericht daneben, mit beiden Laeufen.
//! * **Nicht**, dass es fuer jede Seite gilt: `hole_und_sende` faellt auf den
//!   alten Weg zurueck, wenn es keine geteilte Verbindung gibt (kein h2,
//!   kein Platz, `GOAWAY`, Abbruch).

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use crate::Leitung;
use crate::h2::{self, Rahmen, RahmenFehler, typen};
use crate::h2client::{self, Anfrage, Antwort, MAX_PUFFER_BYTES};
use crate::h2strom::{self, Strom};

// =============================================================================
// added 04.10.2026, Rolle NETZTEMPO. **DIE RAHMENSPUR** (`NARSIL_H2_TRACE=1`)
// =============================================================================
//
// # WARUM EIGENTLICH
//
// GEMESSEN am 04.10.2026: `github.githubassets.com` schickt nach **jeder**
// Antwort `GOAWAY, letzter Strom 3, Grund 0` (`NO_ERROR`). Die Leitung hat
// daraufhin die **sieben** moeglichen Client-Ursachen benannt — und darauf
// kommt es an, **was wir senden**, nicht darauf, was wir vermuten.
//
// # WAS GESENDET UND EMPFANGEN WIRD
//
// * **jeden** Rahmen, den wir schreiben (`anfrage`, WINDOW_UPDATE, SETTINGS-ACK,
//   PING-ACK), mit Typ, Flags, Strom-ID und Nutzlastlaenge,
// * **jeden** Rahmen, den wir lesen (`pumpe`), mit denselben vier Angaben,
// * und beim `GOAWAY` zusaetzlich `Last-Stream-ID` und Fehlercode.
//
// # DIE EIGENTLICHEN FRAGEN, und sie sind alle vier **beantwortbar**
//
//  1. **Schicken wir unsere `SETTINGS`-Quittung?** (`typ 0x04`, flag ACK=0x01)
//  2. **Ist der Preface richtig?** 24 Byte, dann `SETTINGS` mit `ENABLE_PUSH=0`.
//  3. **Schicken wir ein `WINDOW_UPDATE` auf die Verbindung** nach dem Preface?
//  4. **Sind die Strom-IDs aufsteigend und ungerade** — auf der **Leitung**, nicht
//     in unserem Zaehler? GENAU das ist der Verdacht der Leitung, und er ist
//     berechtigt: Strom-ID und Senden lagen bis eben in **zwei** Sperren-
//     bloecken (siehe `anfrage`), also konnte ein Thread mit der ID 5 senden,
//     **bevor** ein anderer mit ID 3 dran war. Nach RFC 9113 5.1 ist eine
//     nicht-aufsteigende ID ein **Verbindungsfehler**, und ein Server, der
//     sowas sieht, ist berechtigt, mit `GOAWAY` zu antworten.
//
// # DIE KOSTEN
//
// **Null** im Betrieb: die Spur ist eine `static OnceLock<bool>`, und ohne
// `NARSIL_H2_TRACE` wird **kein** Byte und **kein** `String` fuer sie angelegt.
// Sie ist **nur** eine `env::var`-Abfrage beim ersten Aufruf — und ein Browser,
// der auf eine (falsch gesetzte) Umgebung wartet, ist ein Browser, der
// haengt. Deshalb wird der Wert **einmal** gelesen und dann nie wieder.

static TRACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// added 04.10.2026, Rolle NETZTEMPO. **Soll die Spur laufen?** `NARSIL_H2_TRACE=1`
/// **oder** ein Aufruf von [`spur_einschalten`].
///
/// **UND WARUM ES EIN `AtomicBool` IST UND KEIN `OnceLock<env::var>`**, und das
/// ist gemessen: `werkzeuge/bau.sh` startet den Test ueber
/// `systemd-run --user --scope`, und ein **Scope** bekommt von systemd eine
/// **minimale** Umgebung. `NARSIL_H2_TRACE=1 werkzeuge/bau.sh …` lieferte
/// darum **null** Spurzeilen — nicht weil nichts gesendet wurde, sondern weil
/// die Variable den Prozess nie erreicht hat. Ein Schalter, den der einzige
/// Aufrufer setzen kann, der den Prozess **kennt** (der Test), ist darum der
/// richtige; die Umgebung bleibt als Zweitweg fuer Handlaeufe.
fn spur() -> bool {
    TRACE.load(Ordering::Relaxed)
}

/// added 04.10.2026, Rolle NETZTEMPO. **Die Spur einschalten** — fuer Tests,
/// die **beweisen** wollen, dass Rahmen wirklich gesendet und empfangen wurden.
///
/// **UND WAS ES NICHT TUT:** nichts zurueckschalten, nichts loeschen. Wer die
/// Spur in einem Test einschaltet, schaltet sie fuer den **Prozess** ein — und
/// `bau.sh` setzt `RUST_TEST_THREADS=2`, also koennte ein zweiter Test
/// mitschreiben. Deshalb ist es eine **Einbahnstrasse** und kein Schalter: das
/// Entfernen wuerde eine zweite Wahrheit darueber sein, ob gerade protokolliert
/// wird.
pub fn spur_einschalten() {
    TRACE.store(true, Ordering::Relaxed);
}

/// added 04.10.2026, Rolle NETZTEMPO. **EIN RAHMEN IN DIE SPUR** — und die vier
/// Angaben sind die, die die Leitung genannt hat.
///
/// **UND DER NAMEN DES TYPS**, weil `0x01` in einem Protokoll-Protokoll
/// nichts bedeutet: `typen::HEADERS` heisst `0x01`. Ohne den Namen muesste
/// jemand beim Lesen der Spur wissen, dass 1 = HEADERS und 7 = GOAWAY — und
/// **das** ist der Fehler, den eine Spur verdecken soll.
pub fn rahmen_spur(richtung: &str, typ: u8, flags: u8, strom: u32, laenge: usize) {
    if !spur() {
        return;
    }
    eprintln!(
        "H2 {richtung} typ={typ:#04x}({}) flags={flags:#04x} strom={strom} len={laenge}",
        typen_name(typ)
    );
}

/// added 04.10.2026, Rolle NETZTEMPO. **Die PAARE eines `SETTINGS`- oder
/// `WINDOW_UPDATE`-Rahmens in die Spur** — und das ist die Frage der Leitung
/// („schicken wir 65 536 oder 4 096?").
///
/// **WARUM EINE EIGENE FORM:** `{:?}` auf einem `Vec<(u16,u32)>` gibt
/// `[(4096, 16384), ...]` — als Dezimalzahlen **ohne Kennung**. Ein Leser muss
/// dann wissen, dass 4 096 die Kennung und 16 384 der Wert ist. Genau die
/// Verwechslung, die ein Protokoll-Protokoll versteckt. Deshalb wird **jede**
/// Zeile als `0x0004=4096` geschrieben — Kennung **und** Wert.
pub fn spur_werte(was: &str, paare: &[(u16, u32)]) {
    if !spur() {
        return;
    }
    let text: Vec<String> = paare
        .iter()
        .map(|(k, w)| format!("0x{k:04x}={w}"))
        .collect();
    eprintln!("H2 {was} [{}]", text.join(", "));
}

/// added 04.10.2026, Rolle NETZTEMPO. **WER SCHIESST ZUERST — als ZAEHLER und
/// nicht als Textsuche.** Die Frage der Leitung war, wer die Verbindung
/// zuerst schliesst, und die naheliegende Antwort waere `grep` in der
/// Rahmenspur. Das ist **wieder „am falsche Ort gemessen"**: der Trace geht
/// nach `stderr`, ein Test laeuft aber in eigener Ausgabe, und was man
/// zurueckbekommt, haengt vom Aufrufer ab.
///
/// **UND WARUM ZWEI ZAEHLER:** „Client zu" und „Server zu" sind **zwei**
/// Ereignisse, und die Frage ist nicht „wurde geschlossen", sondern **welches
/// zuerst**. Ein Zaehler beantwortet das nicht; die Reihenfolge beantwortet es
/// ueber `spur_notiz`, und **beide** gehoeren in die Antwort: die Zahl allein
/// liesse offen, ob der Client 5-mal vor dem Server schloss oder 5-mal danach.
pub static WIR_SCHLIESSEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SERVER_SCHLIESST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static WIR_VERWERFEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// added 04.10.2026: **EINE FREIE NOTIZ IN DER SPUR** — fuer Ereignisse, die
/// **kein Rahmen** sind (Verbindungsende, Wiederverwendung, Neuversuch).
pub fn spur_notiz(text: &str) {
    if spur() {
        eprintln!("H2 * {text}");
    }
}

/// added 04.10.2026: **WIR SCHLIESSEN**, mit dem **Grund** als Text. `warum`
/// steht in der Notiz **und** im Zaehler-Feld, weil eine Zahl ohne Grund die
/// halbe Antwort ist: 3 Abbrueche wegen „Dial verworfen" und 3 wegen „Verbindung
/// tot" sind verschiedene Befunde.
pub fn wir_schliessen(warum: &str) {
    WIR_SCHLIESSEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    spur_notiz(&format!("WIR SCHLIESSEN: {warum}"));
}

/// added 04.10.2026: **SERVER SCHLIESST** — und das ist **nicht** dasselbe wie
/// `GOAWAY`: `GOAWAY` ist eine **Ankuendigung** (der Server sagt, dass er
/// aufhoert, und nennt den letzten Strom), EOF ist das **Ende des Drahts**.
/// Wir haben beides getrennt gezaehlt, weil die Frage der Leitung genau die
/// Reihenfolge betrifft.
pub fn server_schliesst() {
    SERVER_SCHLIESST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    spur_notiz("SERVER SCHLIESST: EOF gelesen");
}

/// added 04.10.2026: **WIR VERWERFEN eine gepoolte Verbindung**, ohne den
/// Draht zu schliessen (sie wandert in die Ablage). Das ist der dritte
/// Ausgang — ohne ihn sieht „wir schliessen 0-mal" aus wie „wir geben sie
/// weiter", und das ist ein Unterschied von **TCP RST** gegen **Lesefehler**.
pub fn wir_verwerfen(warum: &str) {
    WIR_VERWERFEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    spur_notiz(&format!("WIR VERWERFEN: {warum}"));
}

/// added 04.10.2026: **DIE DREI ZAHLEN**, fuer den Bericht.
pub fn schluss_zaehler() -> (u64, u64, u64) {
    (
        WIR_SCHLIESSEN.load(std::sync::atomic::Ordering::Relaxed),
        SERVER_SCHLIESST.load(std::sync::atomic::Ordering::Relaxed),
        WIR_VERWERFEN.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// added 04.10.2026: **NULLEN**, weil ein Test in einem Prozess laeuft und die
/// zweite Messung sonst die erste mitzaehlt — die dritte Form von „am falschen
/// Ort gemessen" (diesmal: Bestand statt Geschehen).
pub fn schluss_zaehler_nullen() {
    WIR_SCHLIESSEN.store(0, std::sync::atomic::Ordering::Relaxed);
    SERVER_SCHLIESST.store(0, std::sync::atomic::Ordering::Relaxed);
    WIR_VERWERFEN.store(0, std::sync::atomic::Ordering::Relaxed);
}

pub fn spur_zahl(was: &str, wert: u32) {
    if spur() {
        eprintln!("H2 {was} +{wert}");
    }
}

/// added 04.10.2026: **`0xff` ist die VORFAHRT** — sie ist nach RFC 9113 3.4
/// **kein Rahmen**. Sie in einen Rahmenkopf zu packen waere eine Erfindung,
/// also traegt sie eine Kennung ausserhalb der Bits, die ein Typ haben kann.
/// added 04.10.2026: **Der Name eines Rahmentyps** — die sechs, die hier
/// vorkommen koennen. `0x02` (`PRIORITY`) wird **nicht** beantwortet, und
/// `0x09` (`CONTINUATION`) nur bei einem offenen Kopfblock.
fn typen_name(typ: u8) -> &'static str {
    match typ {
        typen::DATA => "DATA",
        typen::HEADERS => "HEADERS",
        typen::PRIORITY => "PRIORITY",
        typen::RST_STREAM => "RST_STREAM",
        typen::SETTINGS => "SETTINGS",
        typen::PUSH_PROMISE => "PUSH_PROMISE",
        typen::PING => "PING",
        typen::GOAWAY => "GOAWAY",
        typen::WINDOW_UPDATE => "WINDOW_UPDATE",
        typen::CONTINUATION => "CONTINUATION",
        0xff => "VORFAHRT",
        _ => "UNBEKANNT",
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Wie lange ein Thread wartet, ohne zu
/// blockieren, bevor er die Fuehrerschaft uebernimmt.** Und es ist eine
/// **Lese**-Frist, keine Schreibfrist.
///
/// DER GRUND FUER DEN WERT: der Pump-Thread haelt waehrend des Lesens die
/// **eine** Sperre der Verbindung, und ein Schreibvorgang eines anderen
/// Threads braucht dieselbe. Ohne Frist wuerde der Leser auf einen Sockel
/// blockieren, auf den niemand schreibt (der Antwort kommt kein Byte mehr),
/// und **jeder** andere Thread stuende bis zum Zeitlimit still.
///
/// DER WERT 20 ms: gross genug, dass ein pollender Leser keinen Frame
/// verliert (TCP schickt nichts kleiner als ein MTU-Lappen), klein genug,
/// dass ein Schreibvorgang nicht wahrnehmbar wartet. GEMESSEN wird er an der
/// Wanduhr im Bericht; wenn die Zahl dort nicht passt, ist **dieser** Wert
/// schuld und nicht der Planer.
pub const FRIST_LESE: Duration = Duration::from_millis(5);

/// added 03.10.2026, Rolle NETZTEMPO. **Wie lange ein Wartender schlaeft, wenn
/// der Pump nichts gelesen hat.** Und der Wert ist **klein**, weil er gar
/// nicht der Regler ist: wer schreibt, **weckt** (`anfrage` ruft
/// `notify_all`), und wer Frames zustellt, auch. Diese Frist ist nur das
/// **Netz**, falls ein `notify_all` einmal nicht ankommt.
///
/// GEMESSEN, warum sie nicht `FRIST_LESE` sein darf: bei 20 gleichzeitigen
/// Anfragen und 5 ms Lesefrist waeren das bis zu **100 ms** Wartezeit je
/// Wartendem, in der nichts passiert — bei einer Wanduhr von 1 845 ms sind
/// 5 % des ganzen Vorgangs, und sie waeren **gemessen an der eigenen
/// Bequemlichkeit** statt an der Serverzeit.
pub const FRIST_WARTEN: Duration = Duration::from_millis(2);

/// added 03.10.2026, Rolle NETZTEMPO. So viele Rahmen liest **ein** Pump-Durchgang,
/// bevor er die Fuehrerschaft wieder abgibt.
///
/// DER WEG, und er ist eine Frage der **Fairness**: wer pumping ist und
/// endlos liest, ist ein Thread, der niemanden hungern laesst. Der Wert 64
/// ist nicht aus der Luft gegriffen: eine Antwort aus `HEADERS` + 2 `DATA` +
/// `END_STREAM` braucht 4 Rahmen, und 64 fasst 16 **solche** Antworten — also
/// eine ganze Seite aus einem Host, ohne einmal die Fuehrerschaft zu wechseln.
pub const RAHMEN_PRO_DURCHGANG: usize = 64;

/// DER WEG ZUM `Mutex` OHNE PANIK, und es ist derselbe wie in `pool.rs` und
/// `parallel.rs`: die Daten sind noch da, nur die Invariante ist in Frage. Ein
/// Browser, der nach einem fremden Panic nicht mehr ins Netz kommt, ist
/// schlimmer als ein Zustand ohne Perfektionsgarantie.
fn nimm(m: &Mutex<Zustand>) -> MutexGuard<'_, Zustand> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// DER GESAMTE ZUSTAND EINER GETEILTEN h2-VERBINDUNG.
///
/// **WARUM ALLES IN EINER STRUKTUR**, und es ist nicht Bequemlichkeit: der
/// Rahmenvorrat gehoert zum Sockel, die HPACK-Tabelle zur Verbindung (HPACK
/// 4.1), die Stroeme zur Verbindung (RFC 9113 5.1), das Fenster zur
/// Verbindung (6.9). Jede dieser Groessen wird **während** eines anderen
/// Threads gelesen. Verteilt man sie auf zwei `Mutex`, braucht man eine
/// Sperrenreihenfolge — und die ist bei einem Leser, der im Lesen ein
/// `WINDOW_UPDATE` **schreibt**, nicht mehr local begruendbar.
struct Zustand {
    /// Die Leitung. `None`, wenn sie gestorben ist — dann wird hier nichts
    /// mehr angefasst, und jeder Wartende bekommt den Fehler.
    leitung: Option<Box<dyn Leitung>>,
    /// added 03.10.2026: **der Rahmenvorrat gehoert zur VERBINDUNG**, nicht zu
    /// einem Abruf. GEMESSEN, warum das der Kern ist: TCP zerlegt Nachrichten;
    /// `lies_antwort` hatte den Vorrat **lokal**, und bei zwei gleichzeitigen
    /// Abrufen auf einer Verbindung haette der zweite die Bytes des ersten
    /// als Rahmen **seines** Stromes gelesen — der stillstemoegliche
    /// Datenverlust im ganzen Browser.
    puffer: Vec<u8>,
    /// Die HPACK-Dynamiktabelle der Verbindung (HPACK 4.1: sie wird zwischen
    /// Nachrichten **fortgeschrieben**). Sie wandert aus dem Pool-Eintrag hier
    /// herueber, damit ein Uebergang den Zustand nicht verliert.
    hpack: crate::hpack::DynamischeTabelle,
    /// Alle Stroeme, die der Server noch nicht geschlossen hat.
    stroeme: Vec<Strom>,
    /// Die Strom-IDs, auf die gerade **gewartet** wird. Zweiter Zweck:
    /// `stroeme` wird beim Zurueckgeben eines Aufrufers um seinen Strom
    /// gekuerzt, und nur diese Mengenlaenge ist erlaubt — ein fremder, noch
    /// offener Strom darf nicht weg.
    wartende: HashMap<u32, bool>,
    /// Der letzte vergebene clientseitige Strom (ungerade, RFC 9113 5.1).
    letzter: u32,
    /// `SETTINGS_MAX_CONCURRENT_STREAMS` des Servers. `usize::MAX` = noch
    /// keine `SETTINGS` gesehen (6.5.2: „Initially, there is no limit").
    grenze: usize,
    /// Wie viele Stroeme gerade offen sind.
    aktiv: usize,
    /// Das **Verbindungs**fenster (6.9). Anfangswert `MAX_FENSTER`, weil
    /// `schreibe_vorlauf` unmittelbar nach `SETTINGS` ein `WINDOW_UPDATE`
    /// schickt — genau so rechnet `h2client` (siehe dort, R38).
    fenster: i64,
    /// Wie viele `DATA`-Byte auf dieser Verbindung **flusskontrolliert**
    /// verbraucht wurden (6.9.1 zaehlt den 9-Byte-Kopf **nicht** mit).
    daten_gesamt: u64,
    /// Der naechste `WINDOW_UPDATE`-Schritt (siehe `FLUSSSCHRITT`).
    naechster_schritt: u64,
    /// `true`, wenn der Server `GOAWAY` geschickt hat (6.8) oder der Sockel
    /// EOF geliefert hat. Ab dann werden **keine** neuen Stroeme vergeben.
    geschlossen: bool,
    /// `Some((letzter_strom, grund))` aus einem `GOAWAY`.
    goaway: Option<(u32, u32)>,
    /// Der Grund, warum die Verbindung tot ist — fuer die Fehlermeldung.
    fehler: Option<String>,
    /// Wie viele Threads gerade auf ihre Antwort warten. Das ist die
    /// **Fairness** des Pumpers: wer niemandem mehr helfen kann, legt die
    /// Fuehrerschaft ab.
    warte_anzahl: usize,
    /// added 03.10.2026: `SETTINGS_INITIAL_WINDOW_SIZE` des Servers fuer
    /// **neue** Stroeme. DER UNTERSCHIED ZU `Strom::fenster`: eine Aenderung
    /// dieses Wertes gilt nach RFC 9113 6.9.2 **nur** fuer Stroeme, die
    /// **danach** eroeffnet werden — die laufenden bekommen ihre Anpassung
    /// per `WINDOW_UPDATE`. Ohne dieses Feld wuerde jeder neue Strom mit dem
    /// Wert starten, den **wir** angekuendigt haben (1 MiB), und bei einem
    /// Server, der weniger anbietet, kaeme als erstes ein
    /// `FensterUeberschritten`, das die Verbindung toetet.
    stromfenster: i64,
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine h2-Verbindung fuer viele
/// gleichzeitige Anfragen.**
pub struct Geteilte {
    lage: Mutex<Zustand>,
    /// Die Bedingungsvariable **derselben** Sperre wie `lage`. DER WEG: eine
    /// `Condvar` gehoert zu genau einem `Mutex`, und der Zustand liegt in
    /// `lage` — also gehoert sie zu `lage`. `notify_all` wird **unter**
    /// `lage` aufgerufen, damit kein Wecken verloren geht: wer wartet, hat
    /// die Sperre vorher **freigegeben**, und der Pump haelt sie noch, also
    /// ist der Wartende garantiert schon eingetragen.
    wach: Condvar,
    /// Wer gerade liest. `try_lock` **ohne** Warten ist die Wahl
    /// (Fairness): ein Wartender, der 20 ms auf die Fuehrerschaft wartet,
    /// ist genauso langsam wie einer, der wartet.
    pumpt: Mutex<()>,
    /// added 03.10.2026: wie viele Stroeme ueber geteilte Verbindungen
    /// gelaufen sind. GEMESSEN in `tests/h2_geteilt.rs` und im Bericht — ohne
    /// diese Zahl waere „20 Anfragen auf 1 Verbindung" eine Behauptung.
    stroeme: AtomicU64,
}

/// DER WEG ZUM `pumpt`-Mutex: `try_lock` liefert `Err`, wenn ein anderer
/// pumpt (**gut**, dann ist dieser Thread fertig) **und** wenn der Mutex
/// vergiftet ist (**dann** darf niemand mehr pumpen, das waere ein Haenger).
/// Deshalb wird der vergiftete Fall **genommen**, nicht verworfen: die Daten
/// sind noch da, und ein Browser, der nach einem fremden Panic nicht mehr
/// liest, ist schlimmer als ein Browser ohne Perfektionsgarantie.
fn fuehrerschaft(g: &Mutex<()>) -> Option<MutexGuard<'_, ()>> {
    match g.try_lock() {
        Ok(w) => Some(w),
        Err(std::sync::TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

impl Geteilte {
    /// added 03.10.2026, Rolle NETZTEMPO. **Nimmt eine bereits verhandelte
    /// h2-Verbindung in Besitz.** Kein Handshake, kein `SETTINGS` von uns:
    /// die Vorfahrt hat die Verbindung schon geschickt, ueber die sie kam
    /// (der Weg ist `fetch_mit` → `hole_ueber_h2`, und dort wird sie
    /// uebergeben). Ein zweiter Preface auf derselben Verbindung waere nach
    /// RFC 9113 3.4 ein Verbindungsfehler.
    ///
    /// `letzter_strom` ist der **zuletzt benutzte** Strom (nicht 0): die
    /// naechste Anfrage muss weiterzaehlen, denn ein Strom, der mit
    /// `END_STREAM` endete, ist nach 5.1 geschlossen.
    pub fn uebernehmen(
        leitung: Box<dyn Leitung>,
        letzter_strom: u32,
        hpack: crate::hpack::DynamischeTabelle,
    ) -> Arc<Self> {
        VERBINDUNGEN_GEBAUT.fetch_add(1, Ordering::Relaxed);
        Arc::new(Geteilte {
            lage: Mutex::new(Zustand {
                leitung: Some(leitung),
                puffer: Vec::with_capacity(9),
                hpack,
                stroeme: Vec::new(),
                wartende: HashMap::new(),
                letzter: letzter_strom,
                // added 03.10.2026: **unbegrenzt**, nicht 100. `100` ist
                // **unsere** angekuendigte Grenze (`h2::EIGENE_SETTINGS`), und
                // die gilt fuer Stroeme, die **der Server** eroeffnet. Fuer
                // unsere eigenen gilt die **des Servers**, und bis wir seine
                // `SETTINGS` gelesen haben, sagt RFC 9113 6.5.2: „Initially,
                // there is no limit imposed by the peer."
                grenze: usize::MAX,
                aktiv: 0,
                fenster: h2client::MAX_FENSTER as i64,
                daten_gesamt: 0,
                naechster_schritt: h2client::FLUSSSCHRITT as u64,
                geschlossen: false,
                goaway: None,
                fehler: None,
                warte_anzahl: 0,
                // added 03.10.2026: der **anfangs** angebotene Wert, und er
                // kommt aus derselben Stelle wie bei `Strom::fenster` —
                // `h2::eigenes_fenster()` liest `EIGENE_SETTINGS`, damit
                // Anzeige und Pruefung nicht auseinanderlaufen (R38).
                stromfenster: h2::eigenes_fenster().unwrap_or(65_535) as i64,
            }),
            wach: Condvar::new(),
            pumpt: Mutex::new(()),
            stroeme: AtomicU64::new(0),
        })
    }

    /// added 03.10.2026, Rolle NETZTEMPO. **Wie viele Stroeme ueber diese
    /// Verbindung gelaufen sind.
    pub fn ströme_gesamt(&self) -> u64 {
        self.stroeme.load(Ordering::Relaxed)
    }

    /// `true`, wenn die Verbindung noch **neue** Stroeme annimmt.
    ///
    /// added 03.10.2026, Rolle NETZTEMPO. **DER GRUND FUER DIE GETRENNTE
    /// FRAGE**: `ist_offen` ist die *Bedingung*, `platz()` die *Zahl*. Ohne
    /// die zweite Frage wuerde der Aufrufer bei `MAX_CONCURRENT_STREAMS`
    /// einen Strom vergeben, den der Server zurueckweist — und der
    /// zurueckgewiesene Strom ist nach RFC 9113 5.1 ein **Verbindungs**fehler
    /// bei `REFUSED_STREAM`, nicht etwas, das man im Betrieb wiederholt, weil
    /// es einen **Strom** kostet und den niemand braucht.
    pub fn ist_offen(&self) -> bool {
        let z = nimm(&self.lage);
        !z.geschlossen && z.leitung.is_some() && z.fehler.is_none()
    }

    /// `true`, wenn ein weiterer Strom erlaubt ist (`MAX_CONCURRENT_STREAMS`).
    pub fn platz(&self) -> bool {
        let z = nimm(&self.lage);
        !z.geschlossen && z.leitung.is_some() && z.fehler.is_none() && z.aktiv < z.grenze
    }

    /// `SETTINGS_MAX_CONCURRENT_STREAMS` des Servers, oder `usize::MAX`.
    /// Fuer die Meldung in der Statuszeile und fuer die Tests.
    pub fn grenze(&self) -> usize {
        nimm(&self.lage).grenze
    }

    /// added 03.10.2026, Rolle NETZTEMPO. **Die Anfrage ueber einen neuen
    /// Strom.** Sie schreibt, wartet und baut die Antwort zusammen — und
    /// laesst andere Threads waehrenddessen auf derselben Verbindung lesen.
    ///
    /// DER FEHLER, den sie zurueckgibt, ist **immer** ein Fehler **dieses**
    /// Stromes oder **dieser** Verbindung; die Frage „darf ich neu
    /// verbinden?" entscheidet der Aufrufer ([`hole_und_sende`]), weil erst
    /// dort die **Methode** bekannt ist — und ein `POST` darf nach
    /// RFC 9110 9.2.2 nicht einfach wiederholt werden.
    pub fn anfrage(self: &Arc<Self>, anfrage: &Anfrage) -> Result<Antwort, RahmenFehler> {
        //
        // added 04.10.2026, Rolle NETZTEMPO. **STROM-ID UND SENDEN UNTER EINER
        // SPERRE — Frage (4) der Leitung, und sie ist der wahrscheinlichste
        // Client-Fehler hinter dem `GOAWAY`.
        //
        // BISHER WAREN ES ZWEI BLOCKE: erst die ID unter `lage` vergeben, dann
        // nach dem Loslassen der Sperre gesendet. Zwei Threads koennen die
        // Sperren so abwechseln, dass der mit der ID **5** sendet, **bevor** der
        // mit ID **3** dran ist — und nach RFC 9113 5.1 ist eine nicht
        // aufsteigende clientseitige Strom-ID ein **Verbindungsfehler**. Ein
        // Server darf darauf mit `GOAWAY` antworten, und genau das ist die
        // Meldung, die gemessen wurde (`letzter Strom 3, Grund 0` nach **jeder**
        // Antwort, waehrend Firefox dort 100+ Stroeme auf **einer** Verbindung
        // buendelt).
        //
        // DIE REGEL: **was zugeordnet wird, geht auch raus — ohne die Sperre
        // dazwischen freizugeben.**
        //
        // `strom` steht **vor** dem Block, weil `auf_strom_warten` ohne die
        // Sperre laufen muss (sonst waere das ein Selbsthaenger) und der
        // Aufraeumeblock weiter unten `strom` noch braucht.
        let strom;
        {
            let mut z = nimm(&self.lage);
            // 1. **Der Strom.** `naechste_id` ist `h2strom::naechste_id` — es gibt
            //    keinen zweiten Zaehler fuer clientseitige IDs, und der zaehlt
            //    auch die Grenze `0x7fff_ffff` (R4y).
            if z.geschlossen || z.leitung.is_none() {
                // added 03.10.2026: **`GOAWAY`/`EOF` ist **kein**
                // `ProtokollVerletzung`, sondern eine beendete Verbindung
                // (RFC 9113 6.8) — dieselbe Unterscheidung, die
                // `hole_und_sende_innen` bei `ServerBeendet` trifft.
                return Err(RahmenFehler::ServerBeendet {
                    letzter_strom: z.goaway.map_or(z.letzter, |(l, _)| l),
                    grund: z.goaway.map_or(0, |(_, g)| g),
                });
            }
            if z.aktiv >= z.grenze {
                // added 03.10.2026: **DIE ZAHL, NICHT DER NAME.** Der
                // Aufrufer braucht eine **unterscheidbare** Form, weil er
                // daraufhin eine **zweite Verbindung** oeffnet und nicht
                // wiederholt.
                return Err(RahmenFehler::ProtokollVerletzung {
                    grund: format!(
                        "h2-geteilt: MAX_CONCURRENT_STREAMS erreicht ({} offen)",
                        z.aktiv
                    ),
                });
            }
            strom = h2strom::naechste_id(z.letzter)?;
            z.letzter = strom;
            z.aktiv += 1;
            // 2. **Die Anfrage raus**, ohne die Sperre zu verlassen. Zwei
            //    Threads, die gleichzeitig denselben Rahmen halb schreiben,
            //    erzeugen auf der Leitung einen Rahmen, den es nicht gibt:
            //    `h2client` schreibt `HEADERS` und `DATA` mit **zwei**
            //    `write_all` (R4z7), und ein Byte dazwischen eines anderen
            //    Threads ist ein `PROTOCOL_ERROR`.
            let kopf = h2client::baue_anfrage_rahmen(anfrage, strom)?;
            //
            // added 03.10.2026: **DER `DATA`-RAHMEN WIRD VOR** der Schreibkette
            // **gebaut**. Grund: `baue_rahmen` meint `RahmenFehler`, die Kette
            // meint `io::Error`, und `?` in einem Verschluss, der zur Kette
            // gehoert, verlangt **dieselbe** Art (E0277). Der Fehler der
            // **Groesse** gehoert ohnehin **vor** dem Schreiben (R4z7).
            let daten = if anfrage.koerper.is_empty() {
                None
            } else {
                Some(h2::baue_rahmen(
                    typen::DATA,
                    h2::flags::DATA_END_STREAM,
                    strom,
                    &anfrage.koerper,
                )?)
            };
            //
            // added 04.10.2026: **DIE SPUR VOR** dem Schreiben — und nicht
            // danach: ein `write_all`, das scheitert, hat nichts gesendet, und
            // eine Spur, die auch Fehlversuche zeigt, ist eine Spur von
            // **Absichten**.
            rahmen_spur(
                "->",
                typen::HEADERS,
                h2::flags::END_HEADERS,
                strom,
                kopf.len(),
            );
            if daten.is_some() {
                rahmen_spur(
                    "->",
                    typen::DATA,
                    h2::flags::DATA_END_STREAM,
                    strom,
                    anfrage.koerper.len(),
                );
            }
            let geschrieben = match z.leitung.as_mut() {
                Some(l) => l
                    .write_all(&kopf)
                    .and_then(|()| match &daten {
                        Some(d) => l.write_all(d),
                        None => Ok(()),
                    })
                    .and_then(|()| l.flush()),
                None => {
                    z.aktiv = z.aktiv.saturating_sub(1);
                    return Err(RahmenFehler::KeinRahmen);
                }
            };
            if let Err(e) = geschrieben {
                // added 03.10.2026: **DIE VERBINDUNG IST TOT**, und das wird
                // **hier** festgehalten und nicht beim naechsten Wartenden
                // geraten: `anfrage` bekommt den Fehler, und jeder, der noch
                // wartet, sieht ihn beim Aufwachen.
                z.fehler = Some(format!("h2-geteilt: Schreiben: {e}"));
                z.geschlossen = true;
                z.aktiv = z.aktiv.saturating_sub(1);
                return Err(RahmenFehler::SchreibenFehlgeschlagen {
                    grund: format!("h2-geteilt: {e}"),
                });
            }
            // Der Strom kommt **vor** dem Warten in die Liste: sonst wuerde
            // ein `DATA`, das vor unserem ersten Lesen ankommt, einen
            // unbekannten Strom anlegen (h2strom.rs, `nimm_auf`) und die
            // Reihenfolge im Antwortaufbau verfaelschen.
            // DER WERT VOR `push`: `push` leiht `z.stroeme` mutabel, und
            // `z.stromfenster` waere in diesem Moment eine zweite Leihmut.
            let fenster = z.stromfenster;
            z.stroeme.push(Strom {
                id: strom,
                fenster,
                ..Strom::default()
            });
            z.wartende.insert(strom, true);
            z.warte_anzahl += 1;
            // **UND WECKEN**, unter derselben Sperre. Wer geschrieben hat,
            // weckt die Wartenden — sonst wuerde ein Wartender nach seiner
            // 5-ms-Lesefrist **erst** merken, dass es Arbeit gibt.
            self.wach.notify_all();
        }
        self.stroeme.fetch_add(1, Ordering::Relaxed);
        STROEME_GESAMT.fetch_add(1, Ordering::Relaxed);
        // 3. **Auf die Antwort warten.** Der Leser ist ein **anderer**
        //    moeglicherweise; siehe `pumpe`.
        let r = self.auf_strom_warten(strom);
        // 4. **Aufraeumen.** Nur **eigener** Strom, und nur wenn wirklich
        //    niemand mehr darauf wartet. **EIGENER WAE CHTER**, weil der obere
        //    Block absichtlich geschlossen ist, bevor `auf_strom_warten`
        //    laeuft — sonst waere das ein Selbsthaenger.
        {
            let mut z = nimm(&self.lage);
            z.wartende.remove(&strom);
            z.warte_anzahl = z.warte_anzahl.saturating_sub(1);
            z.aktiv = z.aktiv.saturating_sub(1);
            z.stroeme.retain(|s| s.id != strom);
            self.wach.notify_all();
        }
        r
    }
    fn auf_strom_warten(self: &Arc<Self>, strom: u32) -> Result<Antwort, RahmenFehler> {
        // added 03.10.2026: **`z` wird erst im Aste-Zweig gebunden.** Vorher
        // stand `let mut z = nimm(&self.lage);` **vor** der Schleife — und der
        // Uebersetzer nannte E0382 „borrow of moved value": der Zweig, der
        // zurueck in die Schleife geht, nimmt `z` mit, und die naechste
        // Iteration haette kein `z` mehr. Die Sperre **einmal** am Anfang zu
        // nehmen ist hier auch die falsche Form: dieser Thread darf sie
        // zwischen zwei Runden nicht halten, sonst schreibt niemand.
        loop {
            //
            // added 03.10.2026 03:1x, Rolle NETZTEMPO. **DIE SPERRE ENDET MIT
            // DEM PRUEFBLOCK UNTEN — und das ist der Satz, an dem der ganze Plan
            // haengt.**
            //
            // GEMESSEN, und zwar als **Haenger**: `bau.sh narsil-net test --test
            // h2_geteilt zwanzig` stand **600 s** in der Zeitgrenze und bekam
            // **keinen** einzigen Rahmen, waehrend der Testserver die Antwort
            // auf den ersten Strom **schon geschrieben** hatte (er hat es
            // protokolliert: „antworte strom=1"). Ursache: der `MutexGuard`
            // der Verbindung lebte **ueber** den Aufruf von [`Geteilte::pumpe`]
            // hinaus, und `pumpe` nimmt **dieselbe** Sperre — ein
            // `std::sync::Mutex` ist **nicht** wiederaufnehmbar. Der Thread
            // blockierte **sich selbst**; weil 20 Threads auf dieselbe Sperre
            // warten, sah es nach einem Serverproblem aus.
            //
            // DIE LEHRE, und sie ist groesser als dieser Test: „ich halte eine
            // Sperre und rufe eine Funktion, die dieselbe braucht" ist ein
            // **Haenger ohne jede Fehlermeldung** — kein Panic, kein Timeout
            // der Bibliothek, nur Stillstand. Deshalb liegt der Wecker
            // **innerhalb** des Pruefblocks und der Block endet, **bevor**
            // irgendetwas anderes aufgerufen wird.
            //
            // added 03.10.2026: **DER PRUEFBLOCK IN EIGENEN KLAMMERN**, und
            // der Grund ist der `?`: der erste Fehler, der aus diesem Block
            // herausfliegt (`KopfZuKurz` ohne `:status`), wuerde **mit** der
            // gehaltenen Sperre aus der Funktion gehen. Eine Sperre, die den
            // Aufrufer verlassen darf, ist eine Sperre, die jemand anders
            // nicht bekommt — also endet jeder Weg aus diesem Block hier im
            // Block.
            {
                let z = nimm(&self.lage);
                if let Some(s) = z.stroeme.iter().find(|s| s.id == strom) {
                    if s.zu {
                        // added 03.10.2026: **`RST_STREAM` VOR `END_STREAM`.**
                        // Ein verworfener Strom hat zwar `zu = true`, ist aber
                        // **kein** Erfolg — `h2client::fertig` prueft deshalb
                        // zuerst `verworfen`. Diese Reihenfolge steht an genau
                        // zwei Stellen im Crate (hier und dort), und sie ist die
                        // einzige Stelle, an der ein `RST_STREAM` als Fehler
                        // durchschlaegt.
                        if let Some(code) = s.verworfen {
                            return Err(RahmenFehler::StromVerworfen { strom, code });
                        }
                        // added 03.10.2026: **DER KOERPER WIRD KOPIERT, BEVOR
                        // DIE SPERRE WEITERGEHT.** `Strom::daten` liegt im
                        // gemeinsamen Zustand, und `anfrage` entfernt den Strom
                        // gleich danach — ohne die Kopie wuerde die zweite Zeile
                        // ins Leere zeigen.
                        let antwort = Antwort {
                            status: s
                                .kopfzeilen
                                .iter()
                                .find(|(n, _)| n == ":status")
                                .and_then(|(_, w)| w.parse::<u16>().ok())
                                .ok_or(RahmenFehler::KopfZuKurz { war: 0 })?,
                            // `:status` ist ein **Pseudo-Feld** und gehoert nicht
                            // in die Kopfzeilen der Antwort (RFC 9113 8.1.2.1) —
                            // dieselbe Faustregel wie in `h2client::fertig`.
                            felder: s
                                .kopfzeilen
                                .iter()
                                .filter(|(n, _)| !n.starts_with(':'))
                                .cloned()
                                .collect(),
                            koerper: s.daten.clone(),
                        };
                        //
                        // added 03.10.2026 04:0x, Rolle NETZTEMPO. **EINE FERTIGE
                        // ANTWORT WIRD NICHT VERWORFEN, WEIL EIN `GOAWAY` DA WAR —
                        // und das war ein gemessener Fehler.**
                        //
                        // GEMESSEN (20 Stylesheets, `github.githubassets.com`):
                        //     der Server beendet die Verbindung (GOAWAY, letzter
                        //     Strom 3, Grund 0)
                        // bei **jeder** Anfrage, obwohl der Strom **fertig**
                        // gelesen war — Koerper und Kopfzeilen lagen vor. Danach
                        // rueckfallend „die Verbindung endete, bevor Strom 3 zu
                        // war; 0 Byte waren da" fuer die **anderen** Stroeme.
                        //
                        // DIE NORM, und sie sagt genau das: RFC 9113 6.8 nennt
                        // `Last-Stream-ID` die **letzte verarbeitete** Strom-ID.
                        // Ein Strom **darunter** ist bearbeitet, und wer ihn
                        // vollstaendig gelesen hat, hat die Antwort. `GOAWAY`
                        // sagt nichts darueber, was auf einem bereits
                        // abgeschlossenen Strom steht.
                        //
                        // DIE KONSTRUKTION, und sie ist billig: `z.geschlossen`
                        // verhindert **neue** Stroeme ([`Geteilte::platz`]) —
                        // genau die Wirkung, die `GOAWAY` haben soll. Die fertige
                        // Antwort gehoert dem Aufrufer.
                        return Ok(antwort);
                    }
                }
                if z.fehler.is_some() {
                    return Err(RahmenFehler::VerbindungEndetVorzeitig {
                        strom,
                        gelesen: z
                            .stroeme
                            .iter()
                            .find(|s| s.id == strom)
                            .map_or(0, |s| s.daten.len()),
                    });
                }
            }
            // Sonst: lesen oder warten.
            //
            // added 03.10.2026: **LESEN ODER WARTEN, UND DIE ENTSCHEIDUNG
            // IST, OB GELESEN WURDE.** `pumpe` gibt zurueck, ob es einen
            // Rahmen gab. Gab es keinen, hat dieser Thread die Fuehrerschaft
            // ohne Erfolg in der Hand gehabt — entweder weil ein anderer
            // pumpt, oder weil der Server **denkt**. In beiden Faellen ist
            // Warten richtig: weiter zu pollen hiesse, die `lage` sofort
            // wieder zu nehmen und dem naechsten Schreiber die Sperfe fuer
            // `FRIST_LESE` wegzunehmen. GEMESSEN an der Wanduhr: `tls_n` ist
            // die Zahl, die diese Datei will; die Wartezeit waere sonst der
            // still teurere Preis dafuer.
            let gelesen = self.pumpe();
            if !gelesen {
                // added 03.10.2026: **SCHLAFEN MIT ABGEGEBENER SPERRE.** Die
                // zweite Zeile gibt `z` an die Bedingungsvariable ab und
                // **nimmt** es danach wieder (`neu`) — nicht `z` selbst.
                let z = nimm(&self.lage);
                let _ = self
                    .wach
                    .wait_timeout(z, FRIST_WARTEN)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    /// added 03.10.2026, Rolle NETZTEMPO. **Einen Durchgang lesen**, solange
    /// jemand wartet.
    ///
    /// DER AUFBAU IN EINEM SATZ: **wer liest, liest fuer alle.** Die Rahmen
    /// werden in der Reihenfolge gelesen, in der sie kommen, und an den
    /// Strom verteilt, den sie nennen — das ist der Grund, warum es hier
    /// genau **einen** Leser gibt.
    fn pumpe(&self) -> bool {
        // added 03.10.2026: **WER SCHON PUMPT, DER PUMPT NICHT.** Ohne diese
        // Zeile wuerden zwei Threads aus demselben Sockel lesen, jeder eine
        // halbe Nachricht, und der Browser wuerde Antworten **zuordnen**, die
        // gar nicht zusammengehoeren. Das ist kein Performanceproblem, das
        // ist Datenverlust, und er faellt **nicht** auf: die Antwort ist
        // 200, nur die falsche.
        let Some(_fuehrerschaft) = fuehrerschaft(&self.pumpt) else {
            return false;
        };
        let mut gelesen = false;
        for _ in 0..RAHMEN_PRO_DURCHGANG {
            let mut z = nimm(&self.lage);
            if z.warte_anzahl == 0 || z.leitung.is_none() {
                return gelesen;
            }
            // DER LESEVORGANG, und er haelt die Sperre — das ist die Spannung,
            // die man hier ausdiskutieren muss. Ein blockierendes Lesen
            // wuerde **jeden** anderen Thread von seinem `HEADERS` abhalten.
            // Deshalb liest der Pump **mit Frist** (`FRIST_LESE`) und gibt
            // danach die Fuehrerschaft ab: die 20 ms sind der Preis fuer
            // „ein Sockel, kein Leser, sondern ein Lesefenster".
            //
            // added 03.10.2026: **EIN LESEFFEHLER BEENDET DIE VERBINDUNG**,
            // und er wird **nicht** dem einen Aufrufer gegeben. Grund: die
            // Formen, die `holen` liefert, sind `ZuGross` (ein Rahmen, der
            // unsere Grenze ueberschreitet) und die `RahmenFehler` aus
            // `lies_kopf`/`lies_rahmen` — und **keine** davon ist ein Fehler
            // dieses einen Stromes. Sie sind Fehler der **Verbindung** (5.4.1,
            // 6.9.2), und die anderen Wartenden muessen sie auch sehen.
            let gelesen_rahmen = match holen(&mut z, FRIST_LESE) {
                Ok(l) => l,
                Err(e) => {
                    z.geschlossen = true;
                    z.fehler.get_or_insert_with(|| e.to_string());
                    self.wach.notify_all();
                    return true;
                }
            };
            match gelesen_rahmen {
                // added 03.10.2026: **PAUSE IST KEIN FEHLER**, und sie ist der
                // Normalfall, sobald der Server denkt: der Wartende **schlaeft**
                // danach (siehe `auf_strom_warten`), und wer in der Zwischenzeit
                // schreibt, weckt ihn. Ohne das pollte jeder Wartende die
                // `lage` und **n**aehme dem Schreiber die Sperre fuer je 5 ms weg.
                Lese::Pause => return gelesen,
                Lese::Ende => {
                    // added 03.10.2026: **EOF BEENDET ALLE STROEME.** Es ist
                    // nicht „mein Strom ist zu Ende", sondern „die
                    // Verbindung ist zu Ende" — und das ist der Fall, an dem
                    // ein Browser sonst **haengt**: die wartenden Threads
                    // bekommen hier den Fehler und nicht beim Zeitlimit.
                    z.geschlossen = true;
                    z.fehler
                        .get_or_insert_with(|| "h2-geteilt: Verbindung zu (EOF)".to_string());
                    self.wach.notify_all();
                    return gelesen;
                }
                Lese::Rahmen(r) => {
                    gelesen = true;
                    // added 04.10.2026, Rolle NETZTEMPO: **JEDER EMPFANGENE
                    // RAHMEN IN DIE SPUR**, und **vor** der Verteilung — sonst
                    // waere ein Rahmen, den `verteile` wegwirft (PRIORITY,
                    // SETTINGS-ACK), unsichtbar, und genau diese Faelle sind
                    // die interessanten.
                    rahmen_spur("<-", r.typ, r.flags, r.strom, r.nutzlast.len());
                    if r.typ == typen::GOAWAY {
                        let (l, g) = if r.nutzlast.len() >= 8 {
                            let mut a = [0u8; 4];
                            a.copy_from_slice(&r.nutzlast[0..4]);
                            let mut b = [0u8; 4];
                            b.copy_from_slice(&r.nutzlast[4..8]);
                            (u32::from_be_bytes(a) & 0x7fff_ffff, u32::from_be_bytes(b))
                        } else {
                            (0, 0)
                        };
                        eprintln!("H2 GOAWAY letzter_strom={l} grund={g}");
                    }
                    let verlassen = match self.verteile(&mut z, r) {
                        Ok(v) => v,
                        // added 03.10.2026: **EIN PROTOKOLLFEHLER BEENDET DIE
                        // VERBINDUNG**, und das ist die Form, die `lies_antwort`
                        // auch nimmt (5.4.1, 6.9.2): ein Strom, der zu gross
                        // wird, ist kein Fehler **dieses** Aufrufers, sondern
                        // einer der Verbindung. Wer ihn nur dem einen Aufrufer
                        // gibt, laesst die anderen weiter auf Frames warten,
                        // die nie kommen.
                        Err(_) => self
                            .verbindung_aus(&mut z, "Protokollfehler im Rahmen")
                            .unwrap_or(true),
                    };
                    // **UND JETZT WACHEN** — unter derselben Sperre, mit der
                    // gerade geschrieben wurde. Wer zwischen Pruefen und
                    // Wartem wartet, verpasst kein `notify_all`.
                    self.wach.notify_all();
                    if verlassen {
                        return gelesen;
                    }
                }
            }
        }
        gelesen
    }

    /// added 03.10.2026, Rolle NETZTEMPO. **Einen Rahmen dem richtigen Strom
    /// geben.** `true`, wenn die Verbindung dabei **beendet** wurde.
    ///
    /// DER GRUND FUER DEN EIGENEN SCHRITT: die frames, die **nicht** zum
    /// gesuchten Strom gehoeren, muessen beim Aufräumen **verschluckt**
    /// werden, weil der Pump sie fuer einen anderen Strom liest. Und sie
    /// muessen **vor** dem Aufräumen durch die Flaegekontrolle, denn ein
    /// fremdes `DATA` verbraucht das **Verbindungsfenster** (6.9) — genau das
    /// ist die Korrektur, die in `h2client` (02.10.2026 18:1x) an
    /// `gelesene_bytes` gemessen worden ist.
    fn verteile(&self, z: &mut Zustand, r: Rahmen) -> Result<bool, RahmenFehler> {
        match r.typ {
            typen::SETTINGS => {
                if r.flags & h2::flags::ACK == 0 {
                    // added 03.10.2026: **`SETTINGS` AUSWERTEN.** GEMESSEN,
                    // warum das hier Pflicht ist und nicht Komfort: der Wert
                    // `MAX_CONCURRENT_STREAMS` (0x03) sagt, wie viele Stroeme
                    // der Server **gleichzeitig** annimmt, und ohne ihn
                    // wuerde die Verwaltung Stroeme vergeben, die der Server
                    // zurueckweist. RFC 9113 3.4: „The client **is expected
                    // to honor** any settings established."
                    if let Ok(s) = h2client::GegnerSettings::aus_paare(&paare(&r.nutzlast)) {
                        // added 03.10.2026: **`Option`, nicht 0.** `None`
                        // heisst nach RFC 9113 6.5.2 **unbegrenzt** („Initially,
                        // there is no limit imposed by the peer") — und `0
                        // als Ersatz` waere eine stille Verkleinerung: der
                        // Server wuerde keine einzige Anfrage mehr annehmen.
                        if let Some(max) = s.max_stroeme {
                            z.grenze = max as usize;
                        }
                        {
                            let fenster = s.initial_window;
                            // added 03.10.2026: **DAS STROMFENSTER NEUER
                            // STROEME**, und es aendert sich nur fuer
                            // Stroeme, die danach eroeffnet werden (6.9.2:
                            // „Changes to SETTINGS_INITIAL_WINDOW_SIZE can
                            // only take effect for streams opened after the
                            // change"). Die laufenden bekommen ihre
                            // Anpassung ueber `WINDOW_UPDATE`.
                            z.stromfenster = fenster as i64;
                        }
                    }
                    // QUITTUNGEN, sonst arbeitet der Server nicht weiter
                    // (RFC 9113 6.5.3).
                    if let Some(l) = z.leitung.as_mut() {
                        // added 04.10.2026: **DIE SETTINGS-QUITTUNG MIT IN DIE
                        // SPUR** — das ist Frage (1) der Leitung, und sie ist
                        // ohne Spur nicht beantwortbar.
                        rahmen_spur("->", typen::SETTINGS, h2::flags::ACK, 0, 0);
                        let _ = l.write_all(&h2::baue_settings()?);
                    }
                }
                Ok(false)
            }
            typen::PING => {
                // added 03.10.2026: **PING BEANTWORTEN**, sonst wartet der
                // Server nach RFC 9113 6.7 auf die Quittung — bei manchen
                // Servern 30 s.
                if r.flags & h2::flags::ACK == 0 {
                    if let Some(l) = z.leitung.as_mut() {
                        let _ = l.write_all(&h2::baue_rahmen(
                            typen::PING,
                            h2::flags::ACK,
                            0,
                            &r.nutzlast,
                        )?);
                    }
                }
                Ok(false)
            }
            typen::GOAWAY => {
                // added 03.10.2026, **UND DAS IST DER WICHTIGSTE EINZELNE
                // PUNKT DIESER DATEI**: RFC 9113 6.8. Der Server sagt, welche
                // Strom-IDs er **noch** verarbeitet, und alles darueber ist
                // **nicht** gelaufen. Wird das ignoriert, wartet der Aufrufer
                // auf eine Antwort, die es nicht geben wird — bei 20 Stroemen
                // heisst das 20 Wartezeiten von je 20 ms **pro** Ladevorgang.
                let (letzter, grund) = if r.nutzlast.len() >= 8 {
                    let mut l = [0u8; 4];
                    l.copy_from_slice(&r.nutzlast[0..4]);
                    let mut g = [0u8; 4];
                    g.copy_from_slice(&r.nutzlast[4..8]);
                    (u32::from_be_bytes(l) & 0x7fff_ffff, u32::from_be_bytes(g))
                } else {
                    (z.letzter, 0)
                };
                z.goaway = Some((letzter, grund));
                z.geschlossen = true;
                // Stroeme **ueber** der letzten ID sind **nicht** gelaufen.
                for s in z.stroeme.iter_mut() {
                    if s.id > letzter && !s.zu {
                        s.verworfen = Some(grund);
                        s.zu = true;
                    }
                }
                Ok(true)
            }
            typen::WINDOW_UPDATE => {
                // added 03.10.2026: **NUR STROM 0 INTERESSIERT UNS HIER.**
                // Eine Erhoehung des **Strom**fensters gehoert zu dem Strom,
                // und `h2client` erhaelt sie ueber `nimm_auf`... nein: sie
                // gehoert in die Buchfuehrung **dieses** Stromes, und genau
                // das steht hier.
                if r.nutzlast.len() >= 4 {
                    let mut w = [0u8; 4];
                    w.copy_from_slice(&r.nutzlast[0..4]);
                    let inkrement = u32::from_be_bytes(w) & 0x7fff_ffff;
                    if r.strom == 0 {
                        z.fenster += inkrement as i64;
                    } else if let Some(s) = z.stroeme.iter_mut().find(|s| s.id == r.strom) {
                        s.fenster += inkrement as i64;
                    }
                }
                Ok(false)
            }
            typen::RST_STREAM | typen::HEADERS | typen::CONTINUATION | typen::DATA => {
                // added 03.10.2026: **DIE FLUESSEKONTROLLE VOR `nimm_auf`**
                // und nicht danach: `nimm_auf` zieht nur das
                // **Verbindungs**fenster ab (R38). Das **Stromfenster** muss
                // jemand anderes buchen, und das ist die Stelle hier — sonst
                // wuerde ein Server mehr schicken, als er darf, und wir
                // wuersten es nicht bemerken.
                if r.typ == typen::DATA {
                    let inhalt = h2::daten(&r)?;
                    if let Some(s) = z.stroeme.iter_mut().find(|s| s.id == r.strom) {
                        if inhalt.len() as i64 > s.fenster {
                            return self.verbindung_aus(z, "Stromfenster gerissen");
                        }
                        s.fenster -= inhalt.len() as i64;
                    }
                    z.daten_gesamt += inhalt.len() as u64;
                }
                h2strom::nimm_auf(&r, &mut z.stroeme, &mut z.fenster, &mut z.hpack)?;
                // added 03.10.2026: **DER SCHRITT, OHNE DEN EIN 500-KB-BODY
                // HAENGT.** GEMESSEN (h2client, 01.10.2026 15:8x): ohne
                // `WINDOW_UPDATE` lieferte `github.com` 70 697 Byte und dann
                // nichts mehr. Das ist **pro Verbindung**, nicht pro Strom —
                // und deshalb wird hier **beides** nachgefuehrt.
                if z.daten_gesamt >= z.naechster_schritt {
                    let auf = (z.daten_gesamt
                        - (z.naechster_schritt - h2client::FLUSSSCHRITT as u64))
                        as u32;
                    let strom = r.strom;
                    //
                    // added 03.10.2026: **DIE RAHMEN VOR** dem Schreiben
                    // bauen, und die Schreibfehler **be**nennen. Grund: der
                    // Uebersetzer unterscheidet die beiden `?` — der eine
                    // meint `RahmenFehler` (`baue_window_update`), der andere
                    // `io::Error` (`write_all`), und eine Funktion hat nur
                    // **eine** Fehlerart. Ein Weg ist, das Schreiben wie in
                    // `h2client` ueber `SchreibenFehlgeschlagen` zu melden —
                    // und das ist nicht nur Formsache: nur **diese** Form
                    // baut der Aufrufer neu auf (R5d, 02.10.2026 11:1x).
                    let auf_0 = h2::baue_window_update(0, auf)?;
                    let auf_s = if strom != 0 {
                        Some(h2::baue_window_update(strom, auf)?)
                    } else {
                        None
                    };
                    rahmen_spur("->", typen::WINDOW_UPDATE, 0, 0, 4);
                    if strom != 0 {
                        rahmen_spur("->", typen::WINDOW_UPDATE, 0, strom, 4);
                    }
                    if let Some(l) = z.leitung.as_mut() {
                        let schreiben = l
                            .write_all(&auf_0)
                            .and_then(|()| match &auf_s {
                                Some(d) => l.write_all(d),
                                None => Ok(()),
                            })
                            .and_then(|()| l.flush());
                        if let Err(e) = schreiben {
                            return self
                                .verbindung_aus(z, &format!("h2-geteilt: WINDOW_UPDATE: {e}"));
                        }
                    }
                    z.fenster += auf as i64;
                    if let Some(s) = z.stroeme.iter_mut().find(|s| s.id == strom) {
                        s.fenster += auf as i64;
                    }
                    z.naechster_schritt = z.daten_gesamt + h2client::FLUSSSCHRITT as u64;
                }
                Ok(false)
            }
            // added 03.10.2026: **`PRIORITY`** ist nach RFC 9113 6.3 die
            // einzige Anfrage, die ein Client **verwerfen** darf, und das
            // tuen wir (es waere eine Verletzung, es zu beantworten).
            _ => Ok(false),
        }
    }

    /// added 03.10.2026, Rolle NETZTEMPO. **Die Verbindung ist kaputt** — und
    /// die Frage geht an **alle** wartenden Threads, nicht an einen.
    ///
    /// DER WEG: `fehler` **und** `geschlossen`, und die Prioritaet ist
    /// wichtig. `geschlossen` allein waere eine Behauptung ohne Grund, und
    /// `fehler` allein wuerde bei [`Geteilte::platz`] eine **neue** Verbindung
    /// erlauben, die sofort wieder an derselben Stelle stirbt.
    fn verbindung_aus(&self, z: &mut Zustand, grund: &str) -> Result<bool, RahmenFehler> {
        z.geschlossen = true;
        z.fehler.get_or_insert_with(|| grund.to_string());
        self.wach.notify_all();
        Ok(true)
    }
}

// =============================================================================
// added 03.10.2026, Rolle NETZTEMPO. DER RAHMENVORRAT DER VERBINDUNG
// =============================================================================
//
// DER GRUND FUER EINE EIGENE FORM: `h2client::Puffer` ist **privat** und gehoert
// zu `lies_antwort`, das **einen** Strom liest. Die geteilte Verbindung liest
// **alle** Ströme **nacheinander** aus **einem** Vorrat — und wer den Puffer je
// Abruf neu macht, verliert die Byte eines halben Rahmens.

/// Was ein Lesevorgang ergeben hat.
enum Lese {
    /// Ein vollstaendiger Rahmen.
    Rahmen(Rahmen),
    /// **Jetzt** kam nichts. Die Frist ist abgelaufen — das ist kein Fehler
    /// und **kein** EOF, und dieser Unterschied ist der ganze Unterschied
    /// zwischen „der Server denkt noch" und „die Verbindung ist zu".
    Pause,
    /// EOF oder ein Lesefehler: die Verbindung ist zu Ende.
    Ende,
}

/// added 03.10.2026, Rolle NETZTEMPO. **Ein Rahmen aus dem Vorrat.**
///
/// DIE REIHENFOLGE, und sie ist zwingend: erst der **Kopf** (9 Byte), dann
/// die Laenge daraus, dann der Rest. Wer umgekehrt arbeitet, raet — und
/// `h2client::Puffer::rahmen` sagt im Kommentar genau das: „sonst muesste
/// man raten".
fn holen(z: &mut Zustand, frist: Duration) -> Result<Lese, RahmenFehler> {
    let mut stueck = [0u8; 8192];
    loop {
        if z.puffer.len() >= h2::KOPF_BYTES {
            let (_, _, _, laenge) = h2::lies_kopf(&z.puffer)?;
            let gesamt = h2::KOPF_BYTES + laenge;
            if gesamt > MAX_PUFFER_BYTES {
                return Err(RahmenFehler::ZuGross {
                    laenge: gesamt,
                    erlaubt: MAX_PUFFER_BYTES,
                });
            }
            if z.puffer.len() >= gesamt {
                let (rahmen, verbraucht) = h2::lies_rahmen(&z.puffer)?;
                z.puffer.drain(..verbraucht);
                return Ok(Lese::Rahmen(rahmen));
            }
        }
        let l = match z.leitung.as_mut() {
            Some(l) => l,
            None => return Ok(Lese::Ende),
        };
        // added 03.10.2026: **DIE FRIST VOR DEM LESEN, NICHT DANACH.** Sonst
        // laeuft der erste Lesevorgang mit der Frist des Aufrufers und
        // blockiert die Verbindung so lange, wie er wollte (h2client, R5g).
        let _ = l.set_read_timeout(Some(frist));
        let erg = l.read(&mut stueck);
        match erg {
            Ok(0) => return Ok(Lese::Ende),
            Ok(n) => {
                if z.puffer.len() + n > MAX_PUFFER_BYTES {
                    return Err(RahmenFehler::ZuGross {
                        laenge: z.puffer.len() + n,
                        erlaubt: MAX_PUFFER_BYTES,
                    });
                }
                z.puffer.extend_from_slice(&stueck[..n]);
            }
            // added 03.10.2026: **`WouldBlock` UND `TimedOut` sind dasselbe**:
            // beide sagen „jetzt nichts", und keines davon sagt „EOF".
            // GEMESSEN, warum der Unterschied hier entscheidend ist: wer
            // `TimedOut` wie EOF behandelt, beendet nach **einem** leeren
            // Lesefenster die Verbindung — und 20 Stroeme wuerden dann
            // nacheinander mit „Verbindung zu" abbrechen, weil der Server
            // zwischen zwei Antworten 20 ms nachdenkt.
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(Lese::Pause);
            }
            Err(_) => return Ok(Lese::Ende),
        }
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. Die `SETTINGS`-Nutzlast als Paare, weil
/// `GegnerSettings::aus_paare` sie so will und der Fehlerfall (RST_STREAM mit
/// einer halben Nutzlast) hier **nicht** vorkommen kann.
fn paare(nutzlast: &[u8]) -> Vec<(u16, u32)> {
    let mut r = Vec::new();
    let mut i = 0usize;
    while i + 6 <= nutzlast.len() {
        let k = u16::from_be_bytes([nutzlast[i], nutzlast[i + 1]]);
        let mut w = [0u8; 4];
        w.copy_from_slice(&nutzlast[i + 2..i + 6]);
        r.push((k, u32::from_be_bytes(w)));
        i += 6;
    }
    r
}
// =============================================================================
// added 03.10.2026, Rolle NETZTEMPO. DIE VERWALTUNG: eine geteilte Verbindung
// je (Schema, Host, Port)
// =============================================================================
//
// WARUM EINE EIGENE TABELLE und nicht der Pool: der Pool **verleiht** eine
// Verbindung und nimmt sie heraus, bis die Antwort da ist (siehe Kopf). Genau
// das ist der Grund fuer `tls_n = 6`. Eine geteilte Verbindung wird **nicht**
// verliehen, sondern **benutzt**: sie bleibt liegen, und wer sie braucht, geht
// auf einem **neuen Strom** darauf. Zwei Regeln, ein Ort.
//
// DER ZUSTAND, und warum es zwei Stuecke sind:
//   * `verbindungen` — die **fertigen** geteilten Verbindungen. Eine Liste pro
//     Host, weil es mehr als eine geben **darf**: wenn `MAX_CONCURRENT_STREAMS`
//     erreicht ist, kommt eine zweite dazu (das ist die „erst bei Limit"
//     aus dem Auftrag, und der Server entscheidet die Grenze, nicht wir).
//   * `bauende` — Threads, die gerade **eine** erste Verbindung bauen. Ohne
//     diese Zahl wuerden fuenf Threads, die gleichzeitig ankommen, alle
//     fuenf eigene Handshakes starten — das ist genau der Befund, den
//     NETZTEMPO_7 gemessen hat.
#[derive(Default)]
struct Verwaltung {
    verbindungen: HashMap<crate::pool::Schluessel, Vec<Arc<Geteilte>>>,
    /// Die Hosts, fuer die **gerade eine** erste Verbindung gebaut wird.
    /// added 03.10.2026 03:7x, Rolle NETZTEMPO: **EINE MENGE, KEIN ZAEHLER** —
    /// und das ist ein gemessener Fehler, kein Entwurf.
    ///
    /// GEMESSEN (`tests/h2_geteilt.rs`, `nur_ein_thread_baut_die_erste_
    /// verbindung`): mit einem `usize` bekam nach **acht** Threads der
    /// Eintrag den Wert **8**, obwohl **genau einer** `true` bekommen hatte:
    ///     DIAG vor abmelden: 8   DIAG nach abmelden: 7
    /// Grund: `bauen_anmelden` hat bei **jedem** Aufruf hochgezaehlt — auch
    /// bei den sieben, die `false` bekamen und nur **warten** wollten. Und
    /// abgemeldet wird **nur** von dem einen, der baut. Ergebnis: der Eintrag
    /// kam **nie** auf 0, der Host blieb fuer **immer** als „im Bau"
    /// markiert, und jeder spaetere Aufruf wartete `FRIST_ERSTER` = **20 s**
    /// auf einen Bau, den es nicht gab.
    ///
    /// DIE REGEL, und sie ist kuerzer als der Fehler: **wer nicht baut, meldet
    /// sich nicht ab** — also darf nur der Bauer die Marke setzen. Eine Menge
    /// kann das nicht falsch machen; ein Zaehler kann es, und das hat er.
    bauende: HashMap<crate::pool::Schluessel, u64>,
    /// added 04.10.2026 04:3x, Rolle NETZTEMPO. **Warte wirklich jemand?** —
    /// und diese Zahl ist die Bedingung fuer [`uebernehmen`].
    ///
    /// GEMESSEN, warum: ohne diese Bedingung baute **jede** Aufgabe eine
    /// geteilte Verbindung, auch dann, wenn niemand sie wollte. 20 Anfragen
    /// an `www.wikipedia.org` ergaben
    ///     tls_n=14  straeme=8  gebaut=8
    /// statt der 6 aus NETZTEMPO_7 — also **schlechter** als vorher. Grund:
    /// `bauende` enthielt den Host auch dann, wenn der Bauer **allein** war;
    /// er bot seine Verbindung an, sie wurde geteilt, der Server beendete sie
    /// nach dem ersten Strom, und die naechste Aufgabe fand wieder nichts und
    /// baute wieder eine. Jede geteilte Verbindung trug **genau einen** Strom
    /// (`straeme=8`, `gebaut=8`) — sie war nutzlos und teuer.
    ///
    /// DIE REGEL: **eine geteilte Verbindung entsteht aus einem Wartenden.**
    /// Kein Wartender, kein geteilter Zustand — dann bleibt der alte Weg in
    /// jeder Hinsicht unberuehrt, und der Normalfall (`fetch_mit` ohne
    /// `parallel`) ist unveraendert.
    interessierte: HashMap<crate::pool::Schluessel, usize>,
    /// Der naechste **Bau-Auftrag**. added 03.10.2026 03:9x: eine Zahl, die
    /// nur waehrend des Bauens etwas bedeutet und danach **vergessen** wird —
    /// sie ist kein Zaehler im Sinne einer Statistik, sondern ein **Erkennungs-
    /// mittel** (siehe `bauen_abmelden`).
    bau_nummer: u64,
}

static VERWALTUNG: OnceLock<Mutex<Verwaltung>> = OnceLock::new();

//
// added 04.10.2026 04:2x, Rolle NETZTEMPO. **DIE ZAEHLER LEBEN **AUSSERHALB**
// DER VERWALTUNG — und das ist die Korrektur eines Messfehlers, der mir
// **zweimal** dieselbe Zahl als falsch gemeldet hat.
//
// GEMESSEN: 20 Anfragen an `www.wikipedia.org`, `geteilt = 5`, aber
// `straeme = 0`. Der Widerspruch ist nicht moeglich, wenn Strom 1 einmal
// gelaufen ist — also habe ich nachgesehen: `ströme_gesamt()` lief ueber
// `v.verbindungen` und **summerte nur die Verbindungen, die noch lagen**. Eine
// Verbindung, die `vergiss` aus der Verwaltung entfernt hat, nimmt ihre
// **eigenen** Ströme mit: die Zahl geht auf 0 zurueck, obwohl 5 Ströme
// gelaufen sind.
//
// DIE REGEL, und sie ist die aus NETZTEMPO_7 (dort war es die kumulierte
// Phasensumme): **eine Zahl, die man einem Ereignis zurechnen will, gehoert an
// eine Stelle, die das Ereignis ueberlebt.** Wer in der Verwaltung zaehlt, zaehlt
// den **Bestand**; wer zaehlen will, was **war**, braucht einen Zaehler, den
// niemand zuruecksetzt.
static STROEME_GESAMT: AtomicU64 = AtomicU64::new(0);
static VERBINDUNGEN_GEBAUT: AtomicU64 = AtomicU64::new(0);

/// added 04.10.2026 04:2x: **Wie viele geteilte Verbindungen wurden ueberhaupt
/// gebaut** (seit Prozessbeginn, ohne Zuruecksetzen ausser ueber
/// [`zaehler_zuruecksetzen`]).
pub fn verbindungen_gebaut() -> u64 {
    VERBINDUNGEN_GEBAUT.load(Ordering::Relaxed)
}

/// added 04.10.2026 04:2x: **Nur fuer Tests: beide Zaehler auf 0.**
pub fn zaehler_zuruecksetzen() {
    STROEME_GESAMT.store(0, Ordering::Relaxed);
    VERBINDUNGEN_GEBAUT.store(0, Ordering::Relaxed);
}
/// Die Bedingungsvariable der Verwaltung. `Condvar::new()` ist `const`, also
/// geht ein `static` ohne `OnceLock` — und das ist hier der kuerzere Weg.
static WACH: Condvar = Condvar::new();

fn verwaltung_mutex() -> &'static Mutex<Verwaltung> {
    VERWALTUNG.get_or_init(|| Mutex::new(Verwaltung::default()))
}

fn verwaltung() -> MutexGuard<'static, Verwaltung> {
    verwaltung_mutex().lock().unwrap_or_else(|e| e.into_inner())
}

/// added 04.10.2026 04:3x, Rolle NETZTEMPO. **Das Interesse an einer geteilten
/// Verbindung — und es loest sich von selbst.**
///
/// WARUM EIN **`Drop`** UND KEIN `bauen_abmelden`: [`warte_fertig`] hat
/// **vier** Ausgaenge (gefunden, kein Bauer, Frist, Zeitablauf). Ein
/// Zuruecknehmen an **jedem** davon waere viermal dieselbe Zeile — und die
/// vierte wird irgendwann beim Umbau vergessen. Dann gilt der Host fuer
/// **immer** als „interessiert", jede geteilte Verbindung wird angenommen, und
/// `tls_n` laeuft wieder hoch (GEMESSEN: 14 statt 6, 03/04.10.2026).
///
/// DER BEWEIS, dass es sich loest: `Drop` laeuft bei **jeder** Art von Ausgang,
/// auch beim `return` aus einem `?`-Zweig und beim **Panic** im Aufrufer. Ein
/// Zaehler, den man von Hand zuruecksetzt, gilt nur in dem einen Fall, an den
/// der Schreiber gedacht hat.
struct Interesse {
    schluessel: crate::pool::Schluessel,
}

impl Drop for Interesse {
    fn drop(&mut self) {
        let mut v = verwaltung();
        if let Some(n) = v.interessierte.get_mut(&self.schluessel) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                v.interessierte.remove(&self.schluessel);
            }
        }
        drop(v);
        // added 04.10.2026: **AUCH WECKEN.** Wer gerade eine geteilte
        // Verbindung anbietet, soll das nicht gegen eine geschlossene
        // Bedingungsvariable tun.
        WACH.notify_all();
    }
}

/// added 04.10.2026 04:3x: **Das Interesse anmelden** (siehe [`Interesse`]).
fn interesse_anmelden(schluessel: &crate::pool::Schluessel) -> Interesse {
    let mut v = verwaltung();
    *v.interessierte.entry(schluessel.clone()).or_insert(0) += 1;
    drop(v);
    Interesse {
        schluessel: schluessel.clone(),
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine offene geteilte Verbindung mit
/// Platz** — oder `None`. Kein Warten: das Warten ist eine eigene Frage
/// ([`warte_fertig`]) und hat eine eigene Frist.
pub fn hole_offene(schluessel: &crate::pool::Schluessel) -> Option<Arc<Geteilte>> {
    let v = verwaltung();
    for g in v.verbindungen.get(schluessel)? {
        eprintln!(
            "HOLE: gefunden, platz={} offen={}",
            g.platz(),
            g.ist_offen()
        );
        if g.platz() {
            return Some(Arc::clone(g));
        }
    }
    eprintln!("HOLE: nichts (verb={})", v.verbindungen.len());
    None
}

/// added 03.10.2026, Rolle NETZTEMPO. **Ich baue die erste Verbindung.**
/// `true` heisst: **dieser** Thread baut sie (und alle anderen warten).
///
/// DER WEG, und er ist ein Zaehler und kein Schalter: zwei Threads, die
/// gleichzeitig `true` bekommen, bauen **zwei** Verbindungen, und damit waere
/// genau der Befund wieder da, den diese Datei abstellen soll.
pub fn bauen_anmelden(schluessel: &crate::pool::Schluessel) -> Option<u64> {
    let mut v = verwaltung();
    //
    // added 03.10.2026 03:9x, Rolle NETZTEMPO: **DIE MARKE TRAEGT EINEN
    // NAMEN, UND DAS IST DER DRITTE FEHLER IN DIESER FUNKTION.**
    //
    // GEMESSEN (20 echte Stylesheets eines h2-Hosts): `tls_n = 15`, also
    // **mehr** Handshakes als vorher (6), und `geteilt = 0` — es kam **keine
    // einzige** geteilte Verbindung zustande. Grund: mit einer blosen Menge
    // loescht **jedes** `bauen_abmelden` die Marke, auch wenn es zu einem
    // **frueheren** Auftrag gehoert. Ein Worker, der seine Aufgabe abgeschlossen
    // hat, raeumt also die Marke eines Workers ab, der gerade baut — und die
    // Wartenden sehen „niemand baut" und machen **ihre eigene** Verbindung.
    // Genau das ist die 15.
    //
    // DIE REGEL: **wer abmeldet, muss sagen, fuer wen.** Der Bau-Auftrag
    // bekommt eine Nummer, und `bauen_abmelden` loescht nur, wenn die Nummer
    // noch stimmt. Ein spaeter Auftrag hat eine **andere** Nummer und ueberlebt.
    v.bau_nummer = v.bau_nummer.wrapping_add(1);
    let nummer = v.bau_nummer;
    if v.bauende.contains_key(schluessel) {
        return None;
    }
    v.bauende.insert(schluessel.clone(), nummer);
    Some(nummer)
}

/// added 03.10.2026, Rolle NETZTEMPO. **Der Bau ist vorbei** — erfolgreich
/// oder nicht. Wach machen **immer**: ein Thread, der wartet, darf nicht auf
/// eine `SETTINGS` warten, die nie kommt.
pub fn bauen_abmelden(schluessel: &crate::pool::Schluessel, nummer: u64) {
    let mut v = verwaltung();
    // added 03.10.2026 03:9x: **NUR DIE EIGENE MARKE LOESCHEN.** Der Vergleich
    // ist der ganze Fix, und er ist billig: ein Auftrag mit anderer Nummer
    // gehoert einem anderen Thread.
    if v.bauende.get(schluessel) == Some(&nummer) {
        v.bauende.remove(schluessel);
    }
    drop(v);
    WACH.notify_all();
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine gerade benutzte h2-Verbindung aus
/// dem normalen Weg entgegennehmen** — `true`, wenn sie jetzt geteilt wird.
///
/// DER ZUSTAND, und die Bedingung ist das Eigentliche: uebernommen wird nur,
/// wenn es fuer diesen Host **einen Grund** gibt — also wenn gerade jemand
/// baut oder schon geteilte Verbindungen liegen. Sonst geht die Verbindung
/// unveraendert in den normalen Pool, und der **einzelne** Abrufweg
/// (`fetch_mit` ohne `parallel`) verhaelt sich genau wie vorher. GEMESSEN
/// wichtig, weil das der haeufigere Fall ist: eine Verwandlung, die den
/// Normalfall verschlechtert, faellt auf keinem der 10 Referenzseiten auf und
/// trotzdem auf.
/// **GIBT DIE VERBINDUNG ZURUECK**, wenn sie **nicht** uebernommen wurde:
/// `Some((Leitung, Strom, HPACK))` heisst „du hast sie behalten".
///
/// DER GRUND FUER DIESE FORM, und sie ist nicht bequem: der Aufrufer hat die
/// Leitung **schon bewegt**, wenn er hier anruft, und muss sie im
/// ablehnenden Fall wiederbekommen. Ein `bool` zurueckgeben waere die
/// hübschere Form und **ginge nicht**: `if uebernommen(… stream …) { } else {
/// lege_ab(… stream …) }` verlangt `stream` **zweimal**, und der Uebersetzer
/// sagt E0382 „use of moved value". Ein „erst fragen, dann bewegen" waere die
/// andere Loesung und hat einen **Wettlauf**: zwischen Frage und Bewegung
/// koennte ein anderer Thread die Verwaltung leeren, und die Leitung faellt
/// dann **durchs Raster** — ein Socket, den niemand mehr schliesst.
pub fn uebernehmen(
    schluessel: &crate::pool::Schluessel,
    leitung: Box<dyn Leitung>,
    letzter_strom: u32,
    hpack: crate::hpack::DynamischeTabelle,
) -> Option<(Box<dyn Leitung>, u32, crate::hpack::DynamischeTabelle)> {
    let mut v = verwaltung();
    //
    // added 04.10.2026 04:3x: **NUR WENN WIRKLICH JEMAND WARTET** (siehe
    // `interessierte`). Der alte Ausdruck — „der Host baut gerade" — war die
    // Bedingung dafuer, dass der **Bauer** seine eigene Verbindung in den
    // geteilten Zustand legte, und damit fuer `tls_n` 14 statt 6.
    let wartende = v.interessierte.get(schluessel).copied().unwrap_or(0);
    let hat_grund = wartende > 0 || flight_waehlt(schluessel);
    if !hat_grund {
        return Some((leitung, letzter_strom, hpack));
    }
    v.verbindungen
        .entry(schluessel.clone())
        .or_default()
        .push(Geteilte::uebernehmen(leitung, letzter_strom, hpack));
    drop(v);
    WACH.notify_all();
    None
}

/// added 03.10.2026, Rolle NETZTEMPO. **Warten, bis eine geteilte Verbindung
/// fertig ist** — hoechstens [`h2client::FRIST_ERSTER`].
///
/// DER WEG FUER DIE FRIST, und er ist nicht `Instant`-Gerede: gewartet wird nur
/// auf **den einen** Handshake, den ein anderer Thread macht. Der ist nach
/// `FRIST_ERSTER` entweder da oder nicht; laenger warten hiesse, auf einen
/// Server zu warten, der nicht mehr antwortet — und **nicht** auf eine
/// Antwort, denn die kommt nach dem Handshake.
pub fn warte_fertig(schluessel: &crate::pool::Schluessel) -> Option<Arc<Geteilte>> {
    //
    // added 04.10.2026 04:3x, Rolle NETZTEMPO: **ZUERST DAS INTERESSE, DANN
    // DAS WARTEN.** Die Reihenfolge ist die ganze Sache: der Bauer darf seine
    // Verbindung nur dann in den geteilten Zustand legen, wenn **hier**
    // jemand wirklich wartet — und das erfahren kann er nur, wenn die Zahl vor
    // dem Warten steht. Ein Bauer, der zu spaet davon erfaehrt, gibt seine
    // Verbindung an den Pool zurueck, und der naechste Thread baut wieder eine
    // (GEMESSEN als `tls_n = 14` statt 6).
    let _interesse = interesse_anmelden(schluessel);
    let mut v = verwaltung();
    let ende = std::time::Instant::now() + crate::h2client::FRIST_ERSTER;
    loop {
        for g in v.verbindungen.get(schluessel).into_iter().flatten() {
            if g.platz() {
                return Some(Arc::clone(g));
            }
        }
        //
        // added 03.10.2026 03:8x: **WARTEN NUR, WENN WIRKLICH JEMAND BAUT.**
        // GEMESSEN (`tests/parallel.rs`): `parallel=20151ms` gegenueber 154 ms,
        // weil fuenf Wartenden 20 s lang auf eine **h2**-Verbindung gewartet
        // haben, obwohl der Testserver **HTTP/1.1** spricht — eine h1-Leitung
        // gibt `uebernehmen` **nicht** her. Sie warteten auf etwas, das nie
        // kommen konnte.
        if !v.bauende.contains_key(schluessel) {
            return None;
        }
        let jetzt = std::time::Instant::now();
        if jetzt >= ende {
            return None;
        }
        let (neu, timeout) = WACH
            .wait_timeout(v, ende - jetzt)
            .unwrap_or_else(|e| e.into_inner());
        v = neu;
        if timeout.timed_out() {
            return None;
        }
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine geteilte Verbindung aus der
/// Verwaltung nehmen, weil sie tot ist.** Gesucht wird ueber
/// `Arc::ptr_eq` und nicht ueber den Host: die Verbindung weiss ihren Host
/// nicht (sie ist schliesslich nur eine Leitung), und ihr eine
/// `Schluessel`-Rundenzahl zu geben, wuerde eine zweite Wahrheit sein.
pub fn vergiss(g: &Arc<Geteilte>) {
    let mut v = verwaltung();
    let mut raus: Vec<crate::pool::Schluessel> = Vec::new();
    for (k, liste) in v.verbindungen.iter_mut() {
        liste.retain(|x| !Arc::ptr_eq(x, g));
        if liste.is_empty() {
            raus.push(k.clone());
        }
    }
    for k in raus {
        v.verbindungen.remove(&k);
    }
    drop(v);
    WACH.notify_all();
}

/// added 03.10.2026, Rolle NETZTEMPO. **Nur fuer Tests: alle geteilten
/// Verbindungen weg.** `pool::leeren()` leert den **Pool**, und der leert
/// diese Tabelle nicht — sie sind zwei verschiedene Orte, und ein Test, der
/// nur einen davon leert, misst die falsche Zahl.
pub fn leeren() {
    let mut v = verwaltung();
    v.verbindungen.clear();
    v.bauende.clear();
    drop(v);
    WACH.notify_all();
}

/// added 03.10.2026, Rolle NETZTEMPO. **Wie viele geteilte Verbindungen liegen
/// gerade?** Fuer die Meldung in der Statuszeile und fuer die Tests.
pub fn offene_verbindungen() -> usize {
    verwaltung().verbindungen.values().map(|l| l.len()).sum()
}

/// added 03.10.2026, Rolle NETZTEMPO. **Wie viele Stroeme sind ueber geteilte
/// Verbindungen gelaufen?** Die Zahl, die „20 Anfragen auf 1 Verbindung" belegt.
pub fn ströme_gesamt() -> u64 {
    STROEME_GESAMT.load(Ordering::Relaxed)
}

// =============================================================================
// added 04.10.2026, Rolle NETZTEMPO. **SINGLE FLIGHT: EIN DIAL JE (HOST, PORT)**
// =============================================================================
//
// # DIE URSACHE, DIE DIE LEITUNG GESEHEN HAT — bestaetigt, mit Zahlen
//
// GEMESSEN am 04.10.2026 (`tests/phasen_wohin_live.rs`,
// `ein_host_der_verbindungen_haelt`, 20 Anfragen an **einen** Host):
//
//     MIT Buendelung: tls_n=18  straeme=11  gebaut=11  Wanduhr=10 940 ms
//                     tls_n=11  straeme= 6  gebaut= 6  Wanduhr= 6 893 ms
//     OHNE (alt):     tls_n= 6  straeme= 0  gebaut= 0  Wanduhr= 4 343 ms
//
// **`gebaut = 11` bei 6 Arbeitern.** Es wurden also **elf** Verbindungen
// gebaut, obwohl `parallel` nur **sechs** gleichzeitig laufen laesst. Das ist
// das, was die Leitung „Thundering Herd" nennt, und die Erzaehlung stimmt:
// alle Anfragen starten **gleichzeitig**, bevor die erste Verbindung steht,
// sehen „nichts im Pool" und waehlen **jede** einen eigenen Handshake. Wer
// einen Moment spaeter kommt, findet die Marke des Bauherrn — und wartet. Wer
// **nach** dem Bauherrn kommt, baut wieder, denn die geteilte Verbindung ist
// nach dem ersten Strom schon wieder tot (`GOAWAY`, s. Bericht 8).
//
// # DIE REGEL
//
// **Pro (Host, Port) darf zur Zeit genau EIN Dial laufen.** Der erste Thread
// dialt; alle anderen warten auf derselben `Condvar`, bis die Verbindung
// **steht** (`Fertig`) oder **nicht zustande kam** (`Fehl`). Danach benutzt
// jeder den geteilten h2-Strom.
//
// **UND DER H1-AUSWEG**, den die Leitung verlangt hat: kam **kein** h2
// zustande (ALPN waehlte `http/1.1`), oder ist der Zustand `Fehl`, dann
// duerfen die Wartenden **ihre eigenen** Verbindungen bauen — bis zu sechs je
// Host, so wie es `pool::MAX_PRO_HOST` sagt. Das ist kein Notbehelf, sondern
// der Normalfall fuer h1: dort gibt es nichts zu buendeln.

/// added 04.10.2026, Rolle NETZTEMPO. **Der Zustand eines Fluges.** Und es sind
/// **drei**, nicht zwei: `Waehlt` (es dialt jemand), `Fertig` (die Verbindung
/// steht, alle duerfen sie nehmen) und `Fehl` (es hat nicht geklappt — dann
/// baut jeder selbst).
#[derive(Clone)]
enum FlugZustand {
    Waehlt,
    Fertig(Arc<Geteilte>),
    Fehl,
}

/// added 04.10.2026, Rolle NETZTEMPO. **Ein laufender Dial.** `Arc`, weil der
/// Aufrufer die Tabelle (und damit den Eintrag) verlassen darf, ohne dass der
/// Wartende seinen Zustand verliert — genau der Grund, warum hier kein `Mutex`
/// ueber die ganze Verwaltung liegt.
struct Flug {
    zustand: Mutex<FlugZustand>,
    /// added 04.10.2026 04:5x: **Wie viele Threads gerade auf diesen Dial
    /// warten.** Ohne diese Zahl wird der Eintrag zu frueh entfernt, und die
    /// spaeteren Anrufer fangen **von vorn** an.
    ///
    /// GEMESSEN (`tests/h2_geteilt.rs`, `ein_fehlgeschlagener_dial_wird_nicht_
    /// wiederholt`): `dialer = 2` statt 1. Der Gewinner loeschte den Eintrag
    /// **sofort** nach seinem (erfolglosen) Dial; die letzten Threads kamen
    /// danach, fanden nichts, machten einen **neuen** Flug und dialten erneut.
    /// Das ist nicht nur eine Zahl, die falsch ist — es ist **wieder** der
    /// Fehler aus NETZTEMPO_8, nur eine Ebene tiefer.
    wartende: Mutex<usize>,
    wach: Condvar,
}

/// added 04.10.2026 04:5x: **Der Waechter eines Fluges.** Zaehlt mit und
/// raeumt auf, und beides ueber `Drop` — die Wartephase hat **vier** Ausgaenge
/// (gefunden, Fehl, Zeitablauf, Wegwerfen des Aufrufers), und ein
/// Zurueckzählen an **jedem** davon waere eine Zeile, die beim naechsten Umbau
/// fehlt.
struct FlugWartung {
    flug: Arc<Flug>,
    schluessel: crate::pool::Schluessel,
}

impl Drop for FlugWartung {
    fn drop(&mut self) {
        {
            let mut n = self.flug.wartende.lock().unwrap_or_else(|e| e.into_inner());
            *n = n.saturating_sub(1);
        }
        // added 04.10.2026: **DER LETZTE WARTENDE RAEMT AUF** — und nur der.
        // Wer als Letzter geht, weiss, dass niemand mehr auf diesen Flug zeigt.
        {
            let mut f = fluege();
            let leer = {
                let mut n = self.flug.wartende.lock().unwrap_or_else(|e| e.into_inner());
                *n == 0
            };
            if leer
                && f.get(&self.schluessel)
                    .is_some_and(|x| Arc::ptr_eq(x, &self.flug))
            {
                let fertig = !matches!(*nimm_flug(&self.flug.zustand), FlugZustand::Waehlt);
                if fertig {
                    f.remove(&self.schluessel);
                }
            }
        }
    }
}

static FLUEGE: OnceLock<Mutex<HashMap<crate::pool::Schluessel, Arc<Flug>>>> = OnceLock::new();

fn fluege() -> MutexGuard<'static, HashMap<crate::pool::Schluessel, Arc<Flug>>> {
    FLUEGE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// added 04.10.2026, Rolle NETZTEMPO. **Laeft fuer diesen Host gerade ein
/// Dial?** — die Bedingung, unter der [`uebernehmen`] eine Verbindung in den
/// geteilten Zustand uebernimmt.
///
/// **UND WARUM DAS DIE MARKE VON `bauende` ERSETZT:** „es baut gerade
/// jemand" beantwortet die Frage „**darf** ich eine geteilte Verbindung
/// abgeben?" mit „ja", auch wenn **niemand wartet** — und genau daran ist der
/// Rueschritt `tls_n` 6 -> 18 gescheitert (GEMESSEN: jede geteilte Verbindung
/// trug genau einen Strom, weil sie ohne Wartenden entstand und sofort wieder
/// verdorrte). Die Frage lautet nicht „baut jemand?", sondern „**wartet**
/// jemand?".
fn flight_waehlt(schluessel: &crate::pool::Schluessel) -> bool {
    let f = fluege();
    let Some(flug) = f.get(schluessel) else {
        return false;
    };
    let z = nimm_flug(&flug.zustand);
    matches!(*z, FlugZustand::Waehlt)
}

/// added 04.10.2026, Rolle NETZTEMPO. **Der Weg zum `FlugZustand`** — und er
/// ist ein eigener, weil [`nimm`] fuer den **Verbindungs**zustand dasteht und
/// ein zweiter Weg fuer „`Mutex` ohne `unwrap`" eine dritte Form waere.
fn nimm_flug(m: &Mutex<FlugZustand>) -> MutexGuard<'_, FlugZustand> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// added 04.10.2026, Rolle NETZTEMPO. **SINGLE FLIGHT.** Der erste Aufrufer
/// fuehrt `dial` aus, alle anderen warten und bekommen danach **dieselbe**
/// Verbindung. Liefert `dial` `None` (kein h2, oder es hat nicht geklappt),
/// ist der Aufrufer **selbst** fuer den Abruf zustaendig — das ist der h1-Ausweg.
///
/// **DER `None`-VERTRAG, und er ist die wichtigste Zeile hier:** `None`
/// heisst **nicht** „Fehler", sondern „**hier gibt es nichts zu teilen**".
/// Genau deshalb entscheidet der Aufrufer, nicht diese Funktion: nur er weiss,
// ob sein `Request` wiederholt werden darf (RFC 9110 9.2.2), und diese Frage
/// gehoert **nicht** in eine Funktion, die einen `Box<dyn Leitung>` annimmt.
pub fn single_flight<F>(schluessel: &crate::pool::Schluessel, dial: F) -> Option<Arc<Geteilte>>
where
    F: FnOnce() -> Option<Arc<Geteilte>>,
{
    let frist = crate::h2client::FRIST_ERSTER;
    // 1. **DER FLUG HOLEN ODER ANLEGEN** — und wer anlegt, ist der Erste.
    let (flug, war_ich) = {
        let mut f = fluege();
        match f.entry(schluessel.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => (Arc::clone(e.get()), false),
            std::collections::hash_map::Entry::Vacant(e) => {
                let neu = Arc::new(Flug {
                    zustand: Mutex::new(FlugZustand::Waehlt),
                    wartende: Mutex::new(0),
                    wach: Condvar::new(),
                });
                (Arc::clone(e.insert(neu)), true)
            }
        }
    };
    let ende = std::time::Instant::now() + frist;
    if !war_ich {
        // added 04.10.2026 04:5x: **MITZAHLEN, BEVOR GEWARTET WIRD.** Wer erst
        // nach dem Warten zaehlt, ist nach dem Gewinner bereits draussen — und
        // der Eintrag waere schon weg, bevor er ihn sehen konnte.
        {
            let mut n = flug.wartende.lock().unwrap_or_else(|e| e.into_inner());
            *n += 1;
        }
        let _wartung = FlugWartung {
            flug: Arc::clone(&flug),
            schluessel: schluessel.clone(),
        };
        // 2. **WARTEN.** Und die Bedingung ist **nicht** „es hat sich etwas
        //    geaendert", sondern „es ist **nicht mehr** `Waehlt`" — gewartet
        //    wird auf ein **Ende**, nicht auf ein Ereignis. GEMESSEN, warum
        //    das Wort wichtig ist: wer nur auf „irgendetwas" wartet, wacht
        //    bei jedem `notify_all` auf und sieht `Waehlt` immer noch — und
        //    legt sich wieder hin, waehrend der Dial laeuft.
        let mut z = nimm_flug(&flug.zustand);
        while matches!(*z, FlugZustand::Waehlt) {
            let jetzt = std::time::Instant::now();
            if jetzt >= ende {
                return None;
            }
            let (neu, _) = flug
                .wach
                .wait_timeout(z, ende - jetzt)
                .unwrap_or_else(|e| e.into_inner());
            z = neu;
        }
        let r = match &*z {
            FlugZustand::Fertig(g) => Some(Arc::clone(g)),
            // added 04.10.2026: **`Fehl` UND `Waehlt` (Zeitablauf) sind beide
            // `None`** — und beide heissen fuer den Aufrufer dasselbe: „ich
            // habe keine geteilte Verbindung, mach es selbst". Der Unterschied
            // ist nur die **Begruendung** fuer die Meldung, nicht das Ergebnis.
            _ => None,
        };
        drop(z);
        return r;
    }
    // 3. **DIALEN.** Und `dial` darf **paniken**: dann bleibt der Zustand auf
    //    `Waehlt` stehen, und jeder Wartende wartet bis `FRIST_ERSTER` und
    //    macht dann selbst weiter. Das ist die harmloseste Form: ein fremder
    //    Panic darf keinen **Dauerzustand** erzeugen. (GEMESSEN nicht — aber
    //    ein haengender Wartender waere schlimmer als ein Fehler.)
    let erg = dial();
    // 4. **DER ZUSTAND**, unter derselben Sperre und **vor** dem Wecken — sonst
    //    wartet jemand auf einen Eintrag, den es noch nicht gibt.
    {
        let mut z = nimm_flug(&flug.zustand);
        *z = match &erg {
            Some(g) => FlugZustand::Fertig(Arc::clone(g)),
            None => FlugZustand::Fehl,
        };
        flug.wach.notify_all();
    }
    // 5. **DER EINTRAG BLEIBT, SOLANGE WARTENDE DA SIND.** Er hat die Bedeutung
    //    „es laeuft gerade ein Dial" **oder** „es lief einer, und die Späteren
    //    sollen sein Ergebnis sehen". Wegraeumen darf deshalb **nur** der
    //    letzte Wartende ([`FlugWartung`]) — GEMESSEN: sonst dialten die
    //    letzten drei von zehn Threads **noch einmal** (`dialer = 2`).
    {
        let mut n = flug.wartende.lock().unwrap_or_else(|e| e.into_inner());
        if *n == 0 {
            drop(n);
            let mut f = fluege();
            if f.get(schluessel).is_some_and(|x| Arc::ptr_eq(x, &flug)) {
                f.remove(schluessel);
            }
        }
    }
    erg
}

/// added 03.10.2026, Rolle NETZTEMPO. **Der Abruf ueber eine geteilte
/// Verbindung**, mit Rueckfall auf den normalen Weg.
///
/// DER ABLAUF IN DREI SCHRITTEN, und jeder Schritt ist eine **Frage**, nicht
/// eine Funktion:
///  1. **Gibt es eine offene geteilte Verbindung mit Platz?** Dann Strom
///     drauf. Das ist der Normalfall ab der zweiten Anfrage eines Hosts.
///  2. **Baut gerade jemand?** Dann warten (hoechstens `FRIST_ERSTER`) und
///     nochmal 1. DER WEG, und er ist der eigentliche Hebel: die fuenf
///     Threads, die sonst **fuenf Handshakes** starten, warten hier auf
///     **einen**.
///  3. **Sonst selbst bauen** — ueber `fetch_mit`, den Weg, der auch die
///     Vorfahrt und die `SETTINGS` schickt. Die dabei benutzte Verbindung wird
///     von [`uebernehmen`] entgegengenommen, wenn es dafuer einen Grund gibt.
///
/// UND DER RUECKFALL, der in Schritt 1 steckt: faellt eine geteilte Verbindung
/// mitten in der Antwort aus, geht die Anfrage **einmal** ueber den alten Weg.
/// Das ist bei `GET`/`HEAD` erlaubt (RFC 9110 9.1, 9.2.2) und bei allem
/// anderen **verboten** — `method.ist_wiederholbar()` entscheidet das, und es
/// ist dieselbe Regel wie im Abrufweg (R5d), damit es nicht zwei davon gibt.
pub fn hole_und_sende(
    request: &crate::Request,
    limits: crate::Limits,
) -> Result<crate::Response, crate::NetError> {
    let key = crate::pool_schluessel_of(request);
    //
    // added 04.10.2026, Rolle NETZTEMPO. **SINGLE FLIGHT, und der Weg ist
    // `fetch_mit` statt eines eigenen Dialers.**
    //
    // DER WEG, und er ist der wichtigste Satz dieser Funktion: der Dial
    // **bleibt** `fetch_mit` — der einzige Weg, der Vorfahrt, `SETTINGS`,
    // ALPN, Sitzung, HSTS, DNS und HTTP-Cache richtig macht. `single_flight`
    // fragt nur, **wer** ihn macht. Ein zweiter Dialer waere eine zweite
    // vollstaendige TLS-/HTTP-Vorlage, und die kann man nicht pflegen.
    //
    // **DER `None`-FALL IST DER H1-WEG**, den die Leitung verlangt hat: kein
    // h2 (ALPN waehlte `http/1.1`) oder der Dial schlug fehl — dann baut der
    // Aufrufer seine eigene Verbindung, ganz wie vorher, bis zu `MAX_PRO_HOST`
    // je Host. `hole_und_sende` ist damit **nie schlechter** als `fetch_mit`.
    let roh = request.to_bytes();
    let h2_anfrage = crate::h2_anfrage_aus(request, &roh);
    let flug = single_flight(&key, || {
        // Der Bauer macht den alten Weg und holt danach die Verbindung aus der
        // Verwaltung, die `uebernehmen` (in `fetch_mit`) dort abgelegt hat.
        let _ = crate::fetch_mit(request, limits);
        hole_offene(&key)
    });
    match flug {
        Some(g) => abschicken(request, limits, g, h2_anfrage),
        // added 04.10.2026: **OHNE GETEILTE VERBINDUNG: `fetch_mit`.**
        None => crate::fetch_mit(request, limits),
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine Anfrage ueber die geteilte
/// Verbindung** — und bei einem Fehler **einmal** ueber den alten Weg.
///
/// DER FEHLERFALL, und warum die geteilte Verbindung dabei aus der Verwaltung
/// verschwindet: sie ist an diesem Punkt entweder tot oder hat `GOAWAY`
/// gesagt, und beides ist nach RFC 9113 6.8 ein Ende fuer **diese**
/// Verbindung. Sie weiter anzubieten hiesse, den naechsten Thread auf
/// denselben Fehler zu schicken — und das ist die Art von Schleife, die man
/// erst merkt, wenn der Browser „haengt".
fn abschicken(
    request: &crate::Request,
    limits: crate::Limits,
    g: Arc<Geteilte>,
    h2_anfrage: crate::h2client::Anfrage,
) -> Result<crate::Response, crate::NetError> {
    // added 04.10.2026: **DIE ANFRAGE KOMMT VOM AUFRUFER.** `h2_anfrage_aus`
    // braucht den HTTP/1.1-Text fuer den Pfad, und der Aufrufer hat ihn schon
    // gebaut (`hole_und_sende`). Ein zweites Bauen hiesse, dieselbe Anfrage
    // zweimal zu zerlegen — und die **zweite** Kopie koennte eine andere
    // `:authority` tragen als die erste (R4z8: genau daran bekam Wikipedia 403).
    match g.anfrage(&h2_anfrage) {
        Ok(a) => crate::response_aus_h2(request, a),
        Err(e) => {
            eprintln!("ABSCHICKEN-FEHLER: {e}");
            vergiss(&g);
            if !request.method.ist_wiederholbar() {
                return Err(crate::rahmenfehler_als_netzfehler(request, e));
            }
            crate::fetch_mit(request, limits)
        }
    }
}
